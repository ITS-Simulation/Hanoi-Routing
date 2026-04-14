use std::collections::HashMap;
use std::num::NonZero;

use kiddo::ImmutableKdTree;
use kiddo::SquaredEuclidean;
use rust_road_router::datastr::graph::{EdgeId, NodeId, Weight};

use crate::bounds::{BoundingBox, CoordRejection, ValidationConfig};

/// Result of snapping a coordinate to the nearest edge in the graph.
#[derive(Clone, Copy, Debug)]
pub struct SnapResult {
    /// The closest edge index.
    pub edge_id: EdgeId,
    /// Source node of the closest edge.
    pub tail: NodeId,
    /// Target node of the closest edge.
    pub head: NodeId,
    /// Projection parameter along the full edge geometry: 0.0 = at tail,
    /// 1.0 = at head.
    pub t: f64,
    /// Haversine distance in meters from the query point to the snapped point.
    pub snap_distance_m: f64,
    /// Latitude of the projected snap point.
    pub projected_lat: f32,
    /// Longitude of the projected snap point.
    pub projected_lng: f32,
}

impl SnapResult {
    /// The nearest endpoint node based on the projection parameter.
    /// t < 0.5 → closer to tail, t >= 0.5 → closer to head.
    pub fn nearest_node(&self) -> NodeId {
        if self.t < 0.5 { self.tail } else { self.head }
    }

    /// The geographic point on the edge closest to the query point.
    pub fn projected_point(&self, _lat: &[f32], _lng: &[f32]) -> (f32, f32) {
        (self.projected_lat, self.projected_lng)
    }
}

#[derive(Clone)]
struct EdgePolyline {
    points: Vec<(f32, f32)>,
    positions_m: Vec<f64>,
    total_length_m: f64,
}

#[derive(Clone, Copy)]
struct PolylineProjection {
    projected: (f32, f32),
    snap_distance_m: f64,
    distance_along_m: f64,
}

/// Spatial index for snapping coordinates to the nearest graph edge.
///
/// Uses a KD-tree on node coordinates, then post-filters by Haversine
/// perpendicular distance to the full edge polyline (tail, modelling nodes,
/// head when shape points are available).
pub struct SpatialIndex {
    tree: ImmutableKdTree<f32, 2>,
    first_out: Vec<EdgeId>,
    head: Vec<NodeId>,
    tail: Vec<NodeId>,
    lat: Vec<f32>,
    lng: Vec<f32>,
    first_modelling_node: Option<Vec<u32>>,
    modelling_node_latitude: Option<Vec<f32>>,
    modelling_node_longitude: Option<Vec<f32>>,
    bbox: BoundingBox,
}

const K_NEAREST_NODES: usize = 30;
const SNAP_CANDIDATE_FILTER_BUFFER: usize = 2;
const POLYLINE_POSITION_EPSILON_M: f64 = 1e-3;
pub(crate) const SNAP_MAX_CANDIDATES: usize = 20;

impl SpatialIndex {
    /// Build a spatial index from graph node coordinates and CSR adjacency.
    #[tracing::instrument(skip_all, fields(num_nodes = lat.len()))]
    pub fn build(lat: &[f32], lng: &[f32], first_out: &[EdgeId], head: &[NodeId]) -> Self {
        Self::build_with_shape_points(lat, lng, first_out, head, None, None, None)
    }

    /// Build a spatial index and optionally attach per-edge shape points.
    #[tracing::instrument(skip_all, fields(num_nodes = lat.len(), num_edges = head.len()))]
    pub fn build_with_shape_points(
        lat: &[f32],
        lng: &[f32],
        first_out: &[EdgeId],
        head: &[NodeId],
        first_modelling_node: Option<&[u32]>,
        modelling_node_latitude: Option<&[f32]>,
        modelling_node_longitude: Option<&[f32]>,
    ) -> Self {
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
            tail: build_tail(first_out, head.len()),
            lat: lat.to_vec(),
            lng: lng.to_vec(),
            first_modelling_node: first_modelling_node.map(|values| values.to_vec()),
            modelling_node_latitude: modelling_node_latitude.map(|values| values.to_vec()),
            modelling_node_longitude: modelling_node_longitude.map(|values| values.to_vec()),
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
                let edge_id = edge_idx as EdgeId;
                let tail_node = node;
                let head_node = self.head[edge_idx];
                let polyline = self.edge_polyline(edge_id);
                let projection = project_onto_polyline((lat, lng), &polyline);
                let candidate = SnapResult {
                    edge_id,
                    tail: tail_node,
                    head: head_node,
                    t: if polyline.total_length_m > 0.0 {
                        projection.distance_along_m / polyline.total_length_m
                    } else {
                        0.0
                    },
                    snap_distance_m: projection.snap_distance_m,
                    projected_lat: projection.projected.0,
                    projected_lng: projection.projected.1,
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

    /// Expand a route's arc IDs into full geometry coordinates, including
    /// modelling nodes. Returns `(coordinates, arc_end_coordinate_indices)`.
    pub fn expand_route_arc_geometry_with_boundaries(
        &self,
        route_arc_ids: &[u32],
    ) -> (Vec<(f32, f32)>, Vec<u32>) {
        let mut coordinates = Vec::new();
        let mut arc_end_coordinate_indices = Vec::with_capacity(route_arc_ids.len());

        for &arc_id in route_arc_ids {
            let polyline = self.edge_polyline(arc_id);
            for point in polyline.points {
                push_unique(&mut coordinates, point);
            }

            if !coordinates.is_empty() {
                arc_end_coordinate_indices.push((coordinates.len() - 1) as u32);
            }
        }

        (coordinates, arc_end_coordinate_indices)
    }

    pub fn expand_route_arc_geometry(&self, route_arc_ids: &[u32]) -> Vec<(f32, f32)> {
        self.expand_route_arc_geometry_with_boundaries(route_arc_ids)
            .0
    }

    /// Return the exact travel cost from the snapped point to one endpoint of
    /// the snapped directed edge, scaled by traversed polyline length.
    pub fn partial_edge_cost_to_node_ms(
        &self,
        snap: &SnapResult,
        endpoint: NodeId,
        full_edge_cost: Weight,
    ) -> Weight {
        if endpoint != snap.tail && endpoint != snap.head {
            return 0;
        }

        let polyline = self.edge_polyline(snap.edge_id);
        let snap_m = snap_distance_along_polyline_m(snap, &polyline);
        let traversed_m = if endpoint == snap.head {
            (polyline.total_length_m - snap_m).max(0.0)
        } else {
            snap_m.max(0.0)
        };

        proportional_cost_ms(full_edge_cost, traversed_m, polyline.total_length_m)
    }

    /// Return the exact travel cost between two snapped points on the same
    /// directed edge, or `None` when the interval is not forward-feasible.
    pub fn partial_edge_cost_between_snaps_ms(
        &self,
        from_snap: &SnapResult,
        to_snap: &SnapResult,
        full_edge_cost: Weight,
    ) -> Option<Weight> {
        if from_snap.edge_id != to_snap.edge_id {
            return None;
        }

        let polyline = self.edge_polyline(from_snap.edge_id);
        let from_m = snap_distance_along_polyline_m(from_snap, &polyline);
        let to_m = snap_distance_along_polyline_m(to_snap, &polyline);
        if to_m + POLYLINE_POSITION_EPSILON_M < from_m {
            return None;
        }

        Some(proportional_cost_ms(
            full_edge_cost,
            (to_m - from_m).max(0.0),
            polyline.total_length_m,
        ))
    }

    /// Return modelling-node coordinates strictly between the snapped point and
    /// the chosen endpoint node, ordered along travel.
    pub fn connector_points_from_snap_to_node(
        &self,
        snap: &SnapResult,
        endpoint: NodeId,
    ) -> Vec<(f32, f32)> {
        if endpoint != snap.tail && endpoint != snap.head {
            return Vec::new();
        }

        let polyline = self.edge_polyline(snap.edge_id);
        let snap_m = snap_distance_along_polyline_m(snap, &polyline);

        if endpoint == snap.head {
            collect_polyline_interval_points(&polyline, snap_m, polyline.total_length_m, true)
        } else {
            collect_polyline_interval_points(&polyline, 0.0, snap_m, false)
        }
    }

    /// Return modelling-node coordinates strictly between two projected points
    /// on the same edge, ordered from `from` toward `to`.
    pub fn open_interval_between_snaps(
        &self,
        from_snap: &SnapResult,
        to_snap: &SnapResult,
    ) -> Vec<(f32, f32)> {
        if from_snap.edge_id != to_snap.edge_id {
            return Vec::new();
        }

        let polyline = self.edge_polyline(from_snap.edge_id);
        let from_m = snap_distance_along_polyline_m(from_snap, &polyline);
        let to_m = snap_distance_along_polyline_m(to_snap, &polyline);

        if from_m <= to_m {
            collect_polyline_interval_points(&polyline, from_m, to_m, true)
        } else {
            collect_polyline_interval_points(&polyline, to_m, from_m, false)
        }
    }

    /// Return modelling-node coordinates strictly between the snapped point and
    /// the chosen endpoint node, ordered along travel.
    pub fn connector_points_to_node(
        &self,
        _query: (f32, f32),
        snap: &SnapResult,
        endpoint: NodeId,
    ) -> Vec<(f32, f32)> {
        self.connector_points_from_snap_to_node(snap, endpoint)
    }

    /// Return modelling-node coordinates strictly between two projected points
    /// on the same edge, ordered from `from` toward `to`.
    pub fn open_interval_between_snaps_on_same_edge(
        &self,
        from: (f32, f32),
        from_snap: &SnapResult,
        to: (f32, f32),
        to_snap: &SnapResult,
    ) -> Vec<(f32, f32)> {
        let _ = (from, to);
        self.open_interval_between_snaps(from_snap, to_snap)
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

    fn edge_polyline(&self, edge_id: EdgeId) -> EdgePolyline {
        let edge = edge_id as usize;
        let tail = self.tail[edge] as usize;
        let head = self.head[edge] as usize;
        let mut points = Vec::new();
        points.push((self.lat[tail], self.lng[tail]));

        if let (Some(first_modelling_node), Some(modelling_lat), Some(modelling_lng)) = (
            self.first_modelling_node.as_ref(),
            self.modelling_node_latitude.as_ref(),
            self.modelling_node_longitude.as_ref(),
        ) {
            let start = first_modelling_node[edge] as usize;
            let end = first_modelling_node[edge + 1] as usize;
            for idx in start..end {
                points.push((modelling_lat[idx], modelling_lng[idx]));
            }
        }

        points.push((self.lat[head], self.lng[head]));

        let mut positions_m = Vec::with_capacity(points.len());
        let mut cumulative = 0.0;
        positions_m.push(cumulative);
        for window in points.windows(2) {
            cumulative += haversine_m(
                window[0].0 as f64,
                window[0].1 as f64,
                window[1].0 as f64,
                window[1].1 as f64,
            );
            positions_m.push(cumulative);
        }

        EdgePolyline {
            points,
            positions_m,
            total_length_m: cumulative,
        }
    }
}

fn build_tail(first_out: &[EdgeId], num_edges: usize) -> Vec<NodeId> {
    let mut tail = vec![0; num_edges];
    for node in 0..first_out.len().saturating_sub(1) {
        let start = first_out[node] as usize;
        let end = first_out[node + 1] as usize;
        for edge_idx in start..end {
            tail[edge_idx] = node as NodeId;
        }
    }
    tail
}

fn push_unique(coordinates: &mut Vec<(f32, f32)>, point: (f32, f32)) {
    if coordinates.last().copied() != Some(point) {
        coordinates.push(point);
    }
}

fn collect_polyline_interval_points(
    polyline: &EdgePolyline,
    start_m: f64,
    end_m: f64,
    forward: bool,
) -> Vec<(f32, f32)> {
    if polyline.points.len() <= 2 || (end_m - start_m).abs() <= POLYLINE_POSITION_EPSILON_M {
        return Vec::new();
    }

    let lower = start_m.min(end_m) + POLYLINE_POSITION_EPSILON_M;
    let upper = start_m.max(end_m) - POLYLINE_POSITION_EPSILON_M;
    if lower > upper {
        return Vec::new();
    }

    let mut points = Vec::new();
    if forward {
        for idx in 1..polyline.points.len() - 1 {
            let pos = polyline.positions_m[idx];
            if pos >= lower && pos <= upper {
                points.push(polyline.points[idx]);
            }
        }
    } else {
        for idx in (1..polyline.points.len() - 1).rev() {
            let pos = polyline.positions_m[idx];
            if pos >= lower && pos <= upper {
                points.push(polyline.points[idx]);
            }
        }
    }
    points
}

fn project_onto_polyline(query: (f32, f32), polyline: &EdgePolyline) -> PolylineProjection {
    if polyline.points.len() == 1 {
        return PolylineProjection {
            projected: polyline.points[0],
            snap_distance_m: haversine_m(
                query.0 as f64,
                query.1 as f64,
                polyline.points[0].0 as f64,
                polyline.points[0].1 as f64,
            ),
            distance_along_m: 0.0,
        };
    }

    let mut best: Option<PolylineProjection> = None;

    for (segment_idx, window) in polyline.points.windows(2).enumerate() {
        let (dist, seg_t) = haversine_perpendicular_distance_with_t(
            query.0 as f64,
            query.1 as f64,
            window[0].0 as f64,
            window[0].1 as f64,
            window[1].0 as f64,
            window[1].1 as f64,
        );
        let projected = (
            window[0].0 + (window[1].0 - window[0].0) * seg_t as f32,
            window[0].1 + (window[1].1 - window[0].1) * seg_t as f32,
        );
        let segment_length_m =
            polyline.positions_m[segment_idx + 1] - polyline.positions_m[segment_idx];
        let candidate = PolylineProjection {
            projected,
            snap_distance_m: dist,
            distance_along_m: polyline.positions_m[segment_idx] + segment_length_m * seg_t,
        };

        match best {
            Some(current) if candidate.snap_distance_m >= current.snap_distance_m => {}
            _ => best = Some(candidate),
        }
    }

    best.expect("polyline with at least two points must produce a projection")
}

fn snap_distance_along_polyline_m(snap: &SnapResult, polyline: &EdgePolyline) -> f64 {
    snap.t.clamp(0.0, 1.0) * polyline.total_length_m
}

fn proportional_cost_ms(full_edge_cost: Weight, traversed_m: f64, total_length_m: f64) -> Weight {
    if traversed_m <= POLYLINE_POSITION_EPSILON_M {
        return 0;
    }
    if total_length_m <= POLYLINE_POSITION_EPSILON_M {
        return full_edge_cost;
    }

    let ratio = (traversed_m / total_length_m).clamp(0.0, 1.0);
    (f64::from(full_edge_cost) * ratio).round() as Weight
}

/// Earth radius in meters.
const EARTH_RADIUS_M: f64 = 6_371_000.0;

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
/// Returns `(distance_meters, t)` where t ∈ [0, 1] is the segment-local
/// projection parameter.
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