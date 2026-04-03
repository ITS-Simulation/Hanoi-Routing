use std::collections::HashMap;
use std::num::NonZero;

use kiddo::ImmutableKdTree;
use kiddo::SquaredEuclidean;
use rust_road_router::datastr::graph::{EdgeId, NodeId};

use crate::bounds::{BoundingBox, CoordRejection, ValidationConfig};

/// Result of snapping a coordinate to the nearest edge in the graph.
pub struct SnapResult {
    /// The closest edge index.
    pub edge_id: EdgeId,
    /// Source node of the closest edge.
    pub tail: NodeId,
    /// Target node of the closest edge.
    pub head: NodeId,
    /// Projection parameter along the edge: 0.0 = at tail, 1.0 = at head.
    /// Determines which endpoint the query point is closer to.
    pub t: f64,
    /// Haversine distance in meters from the query point to the snapped point.
    pub snap_distance_m: f64,
}

impl SnapResult {
    /// The nearest endpoint node based on the projection parameter.
    /// t < 0.5 → closer to tail, t >= 0.5 → closer to head.
    pub fn nearest_node(&self) -> NodeId {
        if self.t < 0.5 { self.tail } else { self.head }
    }
}

/// Spatial index for snapping coordinates to the nearest graph edge.
///
/// Uses a KD-tree on node coordinates, then post-filters by Haversine
/// perpendicular distance to incident edges (hybrid snap-to-edge approach).
pub struct SpatialIndex {
    tree: ImmutableKdTree<f32, 2>,
    first_out: Vec<EdgeId>,
    head: Vec<NodeId>,
    lat: Vec<f32>,
    lng: Vec<f32>,
    bbox: BoundingBox,
}

const K_NEAREST_NODES: usize = 30;
const SNAP_CANDIDATE_FILTER_BUFFER: usize = 2;
pub(crate) const SNAP_MAX_CANDIDATES: usize = 20;

impl SpatialIndex {
    /// Build a spatial index from graph node coordinates and CSR adjacency.
    #[tracing::instrument(skip_all, fields(num_nodes = lat.len()))]
    pub fn build(lat: &[f32], lng: &[f32], first_out: &[EdgeId], head: &[NodeId]) -> Self {
        let bbox = BoundingBox::from_coords(lat, lng);
        tracing::info!(
            min_lat = bbox.min_lat,
            max_lat = bbox.max_lat,
            min_lng = bbox.min_lng,
            max_lng = bbox.max_lng,
            "bounding box computed"
        );

        let points: Vec<[f32; 2]> = lat
            .iter()
            .zip(lng.iter())
            .map(|(&la, &lo)| [la, lo])
            .collect();
        let tree = ImmutableKdTree::new_from_slice(&points);

        SpatialIndex {
            tree,
            first_out: first_out.to_vec(),
            head: head.to_vec(),
            lat: lat.to_vec(),
            lng: lng.to_vec(),
            bbox,
        }
    }

    /// Return up to `max_results` snap candidates, sorted by ascending
    /// Haversine distance. Deduplicates by edge_id.
    #[tracing::instrument(skip(self), fields(lat, lng, max_results))]
    pub fn snap_candidates(&self, lat: f32, lng: f32, max_results: usize) -> Vec<SnapResult> {
        if max_results == 0 {
            return Vec::new();
        }

        let query_point = [lat, lng];
        let k = NonZero::new(K_NEAREST_NODES).unwrap();
        let nearest = self.tree.nearest_n::<SquaredEuclidean>(&query_point, k);

        let mut best_by_edge: HashMap<EdgeId, SnapResult> = HashMap::new();

        for nn in &nearest {
            let node = nn.item as NodeId;
            let start = self.first_out[node as usize] as usize;
            let end = self.first_out[node as usize + 1] as usize;

            for edge_idx in start..end {
                let tail_node = node;
                let head_node = self.head[edge_idx];
                let (dist, t) = haversine_perpendicular_distance_with_t(
                    lat as f64,
                    lng as f64,
                    self.lat[tail_node as usize] as f64,
                    self.lng[tail_node as usize] as f64,
                    self.lat[head_node as usize] as f64,
                    self.lng[head_node as usize] as f64,
                );

                let edge_id = edge_idx as EdgeId;
                let candidate = SnapResult {
                    edge_id,
                    tail: tail_node,
                    head: head_node,
                    t,
                    snap_distance_m: dist,
                };

                match best_by_edge.get_mut(&edge_id) {
                    Some(best) if candidate.snap_distance_m < best.snap_distance_m => {
                        *best = candidate;
                    }
                    Some(_) => {}
                    None => {
                        best_by_edge.insert(edge_id, candidate);
                    }
                }
            }
        }

        let mut candidates: Vec<SnapResult> = best_by_edge.into_values().collect();
        candidates.sort_by(|a, b| {
            a.snap_distance_m
                .total_cmp(&b.snap_distance_m)
                .then_with(|| a.edge_id.cmp(&b.edge_id))
        });
        candidates.truncate(max_results);
        candidates
    }

    /// Snap a coordinate to the nearest edge in the graph.
    #[tracing::instrument(skip(self), fields(lat, lng))]
    pub fn snap_to_edge(&self, lat: f32, lng: f32) -> SnapResult {
        self.snap_candidates(lat, lng, 1)
            .into_iter()
            .next()
            .expect("graph must have at least one edge near the query point")
    }

    /// Find all edges incident to a given node (outgoing edges).
    /// Returns (edge_id, tail, head) tuples.
    pub fn edges_incident_to(&self, node: NodeId) -> Vec<(EdgeId, NodeId, NodeId)> {
        let start = self.first_out[node as usize] as usize;
        let end = self.first_out[node as usize + 1] as usize;
        (start..end)
            .map(|edge_idx| (edge_idx as EdgeId, node, self.head[edge_idx]))
            .collect()
    }

    pub fn bbox(&self) -> &BoundingBox {
        &self.bbox
    }

    /// Validate coordinates and snap to edge, or return a rejection.
    ///
    /// Validation order:
    /// 1. Finite check, geographic range check, bounding box check
    /// 2. Snap to nearest edge
    /// 3. Snap distance check
    #[tracing::instrument(skip(self, config), fields(label, lat, lng))]
    pub fn validated_snap(
        &self,
        label: &'static str,
        lat: f32,
        lng: f32,
        config: &ValidationConfig,
    ) -> Result<SnapResult, CoordRejection> {
        self.validated_snap_candidates(label, lat, lng, config, 1)
            .map(|mut candidates| candidates.remove(0))
    }

    /// Validate coordinates, then return up to `max_results` snap candidates
    /// within `max_snap_distance_m`.
    #[tracing::instrument(skip(self, config), fields(label, lat, lng, max_results))]
    pub fn validated_snap_candidates(
        &self,
        label: &'static str,
        lat: f32,
        lng: f32,
        config: &ValidationConfig,
        max_results: usize,
    ) -> Result<Vec<SnapResult>, CoordRejection> {
        crate::bounds::validate_coordinate(label, lat, lng, &self.bbox, config)?;

        let all = self.snap_candidates(
            lat,
            lng,
            max_results.saturating_add(SNAP_CANDIDATE_FILTER_BUFFER),
        );
        let best_distance = all
            .first()
            .map_or(f64::MAX, |candidate| candidate.snap_distance_m);

        let mut filtered: Vec<SnapResult> = all
            .into_iter()
            .filter(|candidate| candidate.snap_distance_m <= config.max_snap_distance_m)
            .collect();

        if filtered.is_empty() {
            return Err(CoordRejection::SnapTooFar {
                label,
                lat,
                lng,
                snap_distance_m: best_distance,
                max_distance_m: config.max_snap_distance_m,
            });
        }

        filtered.truncate(max_results);
        Ok(filtered)
    }
}

/// Earth radius in meters.
const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Haversine distance between two geographic points in meters.
/// Haversine distance in meters between two geographic points.
pub fn haversine_m(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    let dlat = (lat2 - lat1).to_radians();
    let dlng = (lng2 - lng1).to_radians();
    let lat1_r = lat1.to_radians();
    let lat2_r = lat2.to_radians();

    let a = (dlat / 2.0).sin().powi(2) + lat1_r.cos() * lat2_r.cos() * (dlng / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * a.sqrt().asin()
}

/// Compute the Haversine-based perpendicular distance (in meters) and projection
/// parameter t from a query point to a geographic line segment.
///
/// Returns (distance_meters, t) where t ∈ [0, 1] is the projection parameter.
/// t = 0 means the closest point is at the tail, t = 1 at the head.
fn haversine_perpendicular_distance_with_t(
    px: f64,
    py: f64,
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
) -> (f64, f64) {
    // Use equirectangular projection around the segment midpoint for the
    // perpendicular projection calculation. This is accurate for short segments.
    let mid_lat = ((ax + bx) / 2.0).to_radians();
    let cos_mid = mid_lat.cos();

    // Convert to local planar coordinates (scaled degrees)
    let ax_local = ax;
    let ay_local = ay * cos_mid;
    let bx_local = bx;
    let by_local = by * cos_mid;
    let px_local = px;
    let py_local = py * cos_mid;

    let dx = bx_local - ax_local;
    let dy = by_local - ay_local;
    let len_sq = dx * dx + dy * dy;

    // Degenerate edge (zero-length) — distance to the single point
    if len_sq < 1e-20 {
        return (haversine_m(px, py, ax, ay), 0.0);
    }

    // Project point onto the line, clamped to [0, 1]
    let t = ((px_local - ax_local) * dx + (py_local - ay_local) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);

    // Compute the projected point in geographic coordinates
    let proj_lat = ax + t * (bx - ax);
    let proj_lng = ay + t * (by - ay);

    (haversine_m(px, py, proj_lat, proj_lng), t)
}
