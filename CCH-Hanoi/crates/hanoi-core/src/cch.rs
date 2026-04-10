use std::path::Path;

use rust_road_router::algo::customizable_contraction_hierarchy::CustomizedBasic;
use rust_road_router::algo::customizable_contraction_hierarchy::query::Server as CchQueryServer;
use rust_road_router::algo::customizable_contraction_hierarchy::{CCH, customize};
use rust_road_router::algo::{Query, QueryServer};
use rust_road_router::datastr::graph::{FirstOutGraph, INFINITY, NodeId, Weight};
use rust_road_router::datastr::node_order::NodeOrder;
use rust_road_router::io::Load;
use rust_road_router::util::Storage;

use crate::bounds::{BoundingBox, CoordRejection, ValidationConfig};
use crate::cch_cache::CchCache;
use crate::geometry::TurnAnnotation;
use crate::graph::GraphData;
use crate::multi_route::{AlternativeServer, GEO_OVER_REQUEST, MAX_GEO_RATIO};
use crate::spatial::{SNAP_MAX_CANDIDATES, SnapResult, SpatialIndex, haversine_m};

/// Answer from a shortest-path query.
pub struct QueryAnswer {
    /// Total travel time in milliseconds.
    pub distance_ms: Weight,
    /// Total route distance in meters (Haversine sum of coordinate path).
    pub distance_m: f64,
    /// Ordered sequence of original graph arc IDs traversed by the route.
    pub route_arc_ids: Vec<u32>,
    /// Ordered sequence of graph-specific path IDs used to replay this route
    /// under the active weight space. For normal graphs this is identical to
    /// `route_arc_ids`; for line graphs it carries the line-graph node path.
    pub weight_path_ids: Vec<u32>,
    /// Ordered sequence of node IDs along the shortest path.
    pub path: Vec<NodeId>,
    /// Expanded (lat, lng) route geometry. When shape points are present this
    /// includes modelling nodes, and coordinate queries may also prepend/append
    /// snap-edge connector points near the route endpoints.
    pub coordinates: Vec<(f32, f32)>,
    /// Turn annotations along the path.
    /// Empty for normal-graph queries (no turn info available).
    pub turns: Vec<TurnAnnotation>,
    /// User-supplied origin coordinate (set by coordinate queries).
    pub origin: Option<(f32, f32)>,
    /// User-supplied destination coordinate (set by coordinate queries).
    pub destination: Option<(f32, f32)>,
    /// Projected origin point on the snapped edge.
    pub snapped_origin: Option<(f32, f32)>,
    /// Projected destination point on the snapped edge.
    pub snapped_destination: Option<(f32, f32)>,
}

/// Sum Haversine distances between consecutive coordinates to get total route
/// distance in meters.
pub fn route_distance_m(coordinates: &[(f32, f32)]) -> f64 {
    coordinates
        .windows(2)
        .map(|w| {
            crate::spatial::haversine_m(w[0].0 as f64, w[0].1 as f64, w[1].0 as f64, w[1].1 as f64)
        })
        .sum()
}

pub(crate) const TIER2_SNAP_CANDIDATES: usize = 4;
const PROJECTED_POINT_DEDUP_DISTANCE_M: f64 = 1.0;

fn snap_distance_sum(src: &SnapResult, dst: &SnapResult) -> f64 {
    src.snap_distance_m + dst.snap_distance_m
}

fn search_snap_pairs<T, F, I>(
    src_snaps: &[SnapResult],
    dst_snaps: &[SnapResult],
    mut include_pair: I,
    evaluate: &mut F,
) -> Option<(SnapResult, SnapResult, T)>
where
    F: FnMut(&SnapResult, &SnapResult) -> Option<T>,
    I: FnMut(usize, usize) -> bool,
{
    let mut best: Option<(SnapResult, SnapResult, T, f64)> = None;

    for (src_idx, src) in src_snaps.iter().enumerate() {
        for (dst_idx, dst) in dst_snaps.iter().enumerate() {
            if !include_pair(src_idx, dst_idx) {
                continue;
            }

            let Some(result) = evaluate(src, dst) else {
                continue;
            };
            let pair_distance = snap_distance_sum(src, dst);

            if best.as_ref().map_or(true, |(_, _, _, best_distance)| {
                pair_distance < *best_distance
            }) {
                best = Some((*src, *dst, result, pair_distance));
            }
        }
    }

    best.map(|(src, dst, result, _)| (src, dst, result))
}

pub(crate) fn select_tiered_snap_pair<T, F>(
    src_snaps: &[SnapResult],
    dst_snaps: &[SnapResult],
    mut evaluate: F,
) -> Option<(SnapResult, SnapResult, T)>
where
    F: FnMut(&SnapResult, &SnapResult) -> Option<T>,
{
    let (Some(src), Some(dst)) = (src_snaps.first(), dst_snaps.first()) else {
        return None;
    };

    if let Some(result) = evaluate(src, dst) {
        return Some((*src, *dst, result));
    }

    let tier2_src_limit = src_snaps.len().min(TIER2_SNAP_CANDIDATES);
    let tier2_dst_limit = dst_snaps.len().min(TIER2_SNAP_CANDIDATES);

    if let Some(result) = search_snap_pairs(
        src_snaps,
        dst_snaps,
        |src_idx, dst_idx| {
            src_idx < tier2_src_limit
                && dst_idx < tier2_dst_limit
                && !(src_idx == 0 && dst_idx == 0)
        },
        &mut evaluate,
    ) {
        return Some(result);
    }

    search_snap_pairs(
        src_snaps,
        dst_snaps,
        |src_idx, dst_idx| src_idx >= tier2_src_limit || dst_idx >= tier2_dst_limit,
        &mut evaluate,
    )
}

fn coords_within_projected_dedup_threshold(a: (f32, f32), b: (f32, f32)) -> bool {
    haversine_m(a.0 as f64, a.1 as f64, b.0 as f64, b.1 as f64) < PROJECTED_POINT_DEDUP_DISTANCE_M
}

pub(crate) fn prepend_source_geometry(
    coordinates: &mut Vec<(f32, f32)>,
    projected_point: (f32, f32),
    connector_points: Vec<(f32, f32)>,
) -> usize {
    let mut prefix = connector_points;
    let first_existing = prefix
        .first()
        .copied()
        .or_else(|| coordinates.first().copied());

    if first_existing.map_or(true, |point| {
        !coords_within_projected_dedup_threshold(projected_point, point)
    }) {
        prefix.insert(0, projected_point);
    }

    let prepended_count = prefix.len();
    if prepended_count > 0 {
        prefix.extend(std::mem::take(coordinates));
        *coordinates = prefix;
    }

    prepended_count
}

pub(crate) fn append_destination_geometry(
    coordinates: &mut Vec<(f32, f32)>,
    connector_points: Vec<(f32, f32)>,
    projected_point: (f32, f32),
) -> usize {
    let mut suffix = connector_points;
    let last_existing = suffix
        .last()
        .copied()
        .or_else(|| coordinates.last().copied());

    if last_existing.map_or(true, |point| {
        !coords_within_projected_dedup_threshold(projected_point, point)
    }) {
        suffix.push(projected_point);
    }

    let appended_count = suffix.len();
    coordinates.extend(suffix);
    appended_count
}

/// Metric-independent CCH context. Owns the graph data, the CCH topology,
/// and the baseline weight vector. Reusable across customizations.
pub struct CchContext {
    pub graph: GraphData,
    pub cch: CCH,
    pub baseline_weights: Storage<Weight>,
}

impl CchContext {
    /// Load graph data and CCH ordering, then build the CCH (Phase 1).
    ///
    /// `graph_dir` — directory with `first_out`, `head`, `travel_time`, `latitude`, `longitude`
    /// `perm_path` — path to the `cch_perm` file (typically `<graph_dir>/perms/cch_perm`)
    #[tracing::instrument(skip_all, fields(graph_dir = %graph_dir.display()))]
    pub fn load_and_build(graph_dir: &Path, perm_path: &Path) -> std::io::Result<Self> {
        let graph = GraphData::load(graph_dir)?;
        let perm: Vec<NodeId> = Vec::load_from(perm_path)?;
        let order = NodeOrder::from_node_order(perm);

        tracing::info!(
            num_nodes = graph.num_nodes(),
            num_edges = graph.num_edges(),
            "preparing CCH"
        );

        let borrowed = graph.as_borrowed_graph();
        let cache = CchCache::new(graph_dir);
        let source_files = [
            graph_dir.join("first_out"),
            graph_dir.join("head"),
            perm_path.to_path_buf(),
        ];
        let source_refs: Vec<&Path> = source_files.iter().map(|path| path.as_path()).collect();
        let cch = 'build: {
            if cache.is_valid(&source_refs) {
                match cache.load_cch(&borrowed) {
                    Ok(loaded) => {
                        tracing::info!("loaded CCH from cache");
                        break 'build loaded;
                    }
                    Err(err) => {
                        tracing::warn!("cached CCH failed validation: {err}; rebuilding");
                    }
                }
            }

            tracing::info!("building CCH from scratch");
            let built = CCH::fix_order_and_build(&borrowed, order);
            if let Err(err) = cache.save_cch(&built, &source_refs) {
                tracing::warn!("failed to write CCH cache: {err}");
            }
            built
        };

        let baseline_weights = graph.travel_time.clone();

        Ok(CchContext {
            graph,
            cch,
            baseline_weights,
        })
    }

    /// Customize with baseline weights (Phase 2).
    #[tracing::instrument(skip_all)]
    pub fn customize(&self) -> CustomizedBasic<'_, CCH> {
        let metric = FirstOutGraph::new(
            &self.graph.first_out[..],
            &self.graph.head[..],
            &self.baseline_weights[..],
        );
        customize(&self.cch, &metric)
    }

    /// Customize with caller-provided weights (Phase 2).
    #[tracing::instrument(skip_all, fields(num_weights = weights.len()))]
    pub fn customize_with(&self, weights: &[Weight]) -> CustomizedBasic<'_, CCH> {
        let metric = FirstOutGraph::new(&self.graph.first_out[..], &self.graph.head[..], weights);
        customize(&self.cch, &metric)
    }
}

/// Query engine wrapping a CCH query server. Borrows a `CchContext` for its
/// lifetime so the CCH topology is guaranteed to outlive the customized data.
pub struct QueryEngine<'a> {
    server: CchQueryServer<CustomizedBasic<'a, CCH>>,
    context: &'a CchContext,
    spatial: SpatialIndex,
    validation_config: ValidationConfig,
}

impl<'a> QueryEngine<'a> {
    /// Create a query engine from a CCH context. Performs initial customization
    /// with baseline weights and builds the spatial index.
    pub fn new(context: &'a CchContext) -> Self {
        Self::with_validation_config(context, ValidationConfig::default())
    }

    pub fn with_validation_config(
        context: &'a CchContext,
        validation_config: ValidationConfig,
    ) -> Self {
        let customized = context.customize();
        let server = CchQueryServer::new(customized);

        let spatial = SpatialIndex::build_with_shape_points(
            &context.graph.latitude,
            &context.graph.longitude,
            &context.graph.first_out,
            &context.graph.head,
            context.graph.first_modelling_node.as_deref(),
            context.graph.modelling_node_latitude.as_deref(),
            context.graph.modelling_node_longitude.as_deref(),
        );

        QueryEngine {
            server,
            context,
            spatial,
            validation_config,
        }
    }

    /// Query by node IDs. Returns None if no path exists.
    #[tracing::instrument(skip(self), fields(from, to))]
    pub fn query(&mut self, from: NodeId, to: NodeId) -> Option<QueryAnswer> {
        let result = self.server.query(Query { from, to });

        if let Some(mut connected) = result.found() {
            let distance_ms = connected.distance();
            if distance_ms >= INFINITY {
                return None;
            }
            let path = connected.node_path();
            let route_arc_ids = self.reconstruct_arc_ids(&path)?;
            let weight_path_ids = route_arc_ids.clone();
            let coordinates = self.coordinates_for_path(&path, &route_arc_ids);
            let distance_m = route_distance_m(&coordinates);
            Some(QueryAnswer {
                distance_ms,
                distance_m,
                route_arc_ids,
                weight_path_ids,
                path,
                coordinates,
                turns: vec![],
                origin: None,
                destination: None,
                snapped_origin: None,
                snapped_destination: None,
            })
        } else {
            None
        }
    }

    /// Query by coordinates using ranked snap candidates in the original graph.
    #[tracing::instrument(skip(self), fields(
        from_lat = from.0, from_lng = from.1,
        to_lat = to.0, to_lng = to.1
    ))]
    pub fn query_coords(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
    ) -> Result<Option<QueryAnswer>, CoordRejection> {
        let src_snaps = self.spatial.validated_snap_candidates(
            "origin",
            from.0,
            from.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;
        let dst_snaps = self.spatial.validated_snap_candidates(
            "destination",
            to.0,
            to.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;

        Ok(
            match select_tiered_snap_pair(&src_snaps, &dst_snaps, |src, dst| {
                self.query_coordinate_candidate(src, dst)
            }) {
                Some((src, dst, answer)) => {
                    Some(self.patch_coordinates(answer, from, to, &src, &dst))
                }
                _ => None,
            },
        )
    }

    /// Find up to `max_alternatives` alternative routes by node IDs.
    #[tracing::instrument(skip(self), fields(from, to, max_alternatives, stretch_factor))]
    pub fn multi_query(
        &mut self,
        from: NodeId,
        to: NodeId,
        max_alternatives: usize,
        stretch_factor: f64,
    ) -> Vec<QueryAnswer> {
        let customized = self.server.customized();
        let mut multi = AlternativeServer::new(customized);
        let request_count = max_alternatives
            .saturating_mul(GEO_OVER_REQUEST)
            .max(max_alternatives + 12);
        let lat = &self.context.graph.latitude;
        let lng = &self.context.graph.longitude;
        let path_geo_len = |path: &[NodeId]| -> f64 {
            path.windows(2)
                .map(|w| {
                    crate::spatial::haversine_m(
                        lat[w[0] as usize] as f64,
                        lng[w[0] as usize] as f64,
                        lat[w[1] as usize] as f64,
                        lng[w[1] as usize] as f64,
                    )
                })
                .sum()
        };
        let candidates = multi.alternatives(from, to, request_count, stretch_factor, path_geo_len);

        let mut results: Vec<QueryAnswer> = Vec::with_capacity(max_alternatives);
        let mut shortest_geo_dist: Option<f64> = None;

        for alt in candidates {
            if results.len() >= max_alternatives {
                break;
            }
            if alt.path.is_empty() {
                continue;
            }

            let route_arc_ids = match self.reconstruct_arc_ids(&alt.path) {
                Some(ids) => ids,
                None => continue,
            };
            let coordinates = self.coordinates_for_path(&alt.path, &route_arc_ids);
            let distance_m = route_distance_m(&coordinates);

            if let Some(base) = shortest_geo_dist {
                if distance_m > base * MAX_GEO_RATIO {
                    continue;
                }
            } else {
                shortest_geo_dist = Some(distance_m);
            }

            let weight_path_ids = route_arc_ids.clone();

            results.push(QueryAnswer {
                distance_ms: alt.distance,
                distance_m,
                route_arc_ids,
                weight_path_ids,
                path: alt.path,
                coordinates,
                turns: vec![],
                origin: None,
                destination: None,
                snapped_origin: None,
                snapped_destination: None,
            });
        }

        tracing::info!(
            requested = max_alternatives,
            returned = results.len(),
            shortest_ms = results.first().map(|route| route.distance_ms),
            "multi_query completed"
        );

        results
    }

    /// Find up to `max_alternatives` alternative routes by coordinates.
    #[tracing::instrument(skip(self), fields(
        from_lat = from.0,
        from_lng = from.1,
        to_lat = to.0,
        to_lng = to.1,
        max_alternatives,
        stretch_factor
    ))]
    pub fn multi_query_coords(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
        max_alternatives: usize,
        stretch_factor: f64,
    ) -> Result<Vec<QueryAnswer>, CoordRejection> {
        let src_snaps = self.spatial.validated_snap_candidates(
            "origin",
            from.0,
            from.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;
        let dst_snaps = self.spatial.validated_snap_candidates(
            "destination",
            to.0,
            to.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;

        if let Some((src, dst, answers)) =
            select_tiered_snap_pair(&src_snaps, &dst_snaps, |src, dst| {
                let answers = self.multi_query_coordinate_candidates(
                    src,
                    dst,
                    max_alternatives,
                    stretch_factor,
                );
                (!answers.is_empty()).then_some(answers)
            })
        {
            let patched: Vec<QueryAnswer> = answers
                .into_iter()
                .map(|answer| self.patch_coordinates(answer, from, to, &src, &dst))
                .collect();
            tracing::info!(
                requested = max_alternatives,
                returned = patched.len(),
                shortest_ms = patched.first().map(|route| route.distance_ms),
                "multi_query_coords completed"
            );
            Ok(patched)
        } else {
            tracing::info!(
                requested = max_alternatives,
                returned = 0usize,
                "multi_query_coords completed"
            );
            Ok(Vec::new())
        }
    }

    /// Attach the user's origin/destination metadata and splice snap-edge
    /// connector geometry around the routed path when needed.
    fn direct_same_edge_coordinate_answer(
        &self,
        src: &SnapResult,
        dst: &SnapResult,
    ) -> Option<QueryAnswer> {
        let edge_cost = self.context.graph.travel_time[src.edge_id as usize];
        let distance_ms = self
            .spatial
            .partial_edge_cost_between_snaps_ms(src, dst, edge_cost)?;

        Some(QueryAnswer {
            distance_ms,
            distance_m: 0.0,
            route_arc_ids: Vec::new(),
            weight_path_ids: vec![src.edge_id],
            path: Vec::new(),
            coordinates: Vec::new(),
            turns: Vec::new(),
            origin: None,
            destination: None,
            snapped_origin: None,
            snapped_destination: None,
        })
    }

    fn exact_coordinate_overhead_ms(&self, src: &SnapResult, dst: &SnapResult) -> Weight {
        let src_edge_cost = self.context.graph.travel_time[src.edge_id as usize];
        let dst_edge_cost = self.context.graph.travel_time[dst.edge_id as usize];

        self.spatial
            .partial_edge_cost_to_node_ms(src, src.head, src_edge_cost)
            .saturating_add(
                self.spatial
                    .partial_edge_cost_to_node_ms(dst, dst.tail, dst_edge_cost),
            )
    }

    fn query_coordinate_candidate(
        &mut self,
        src: &SnapResult,
        dst: &SnapResult,
    ) -> Option<QueryAnswer> {
        if let Some(answer) = self.direct_same_edge_coordinate_answer(src, dst) {
            return Some(answer);
        }

        let mut answer = self.query(src.head, dst.tail)?;
        answer.distance_ms = answer
            .distance_ms
            .saturating_add(self.exact_coordinate_overhead_ms(src, dst));
        Some(answer)
    }

    fn multi_query_coordinate_candidates(
        &mut self,
        src: &SnapResult,
        dst: &SnapResult,
        max_alternatives: usize,
        stretch_factor: f64,
    ) -> Vec<QueryAnswer> {
        if let Some(answer) = self.direct_same_edge_coordinate_answer(src, dst) {
            return vec![answer];
        }

        let overhead = self.exact_coordinate_overhead_ms(src, dst);
        let mut answers = self.multi_query(src.head, dst.tail, max_alternatives, stretch_factor);
        for answer in &mut answers {
            answer.distance_ms = answer.distance_ms.saturating_add(overhead);
        }
        answers
    }

    fn patch_coordinates(
        &self,
        mut answer: QueryAnswer,
        from: (f32, f32),
        to: (f32, f32),
        src: &SnapResult,
        dst: &SnapResult,
    ) -> QueryAnswer {
        let src_projected =
            src.projected_point(&self.context.graph.latitude, &self.context.graph.longitude);
        let dst_projected =
            dst.projected_point(&self.context.graph.latitude, &self.context.graph.longitude);

        if answer.coordinates.is_empty() && src.edge_id == dst.edge_id {
            answer.coordinates = self.spatial.open_interval_between_snaps(src, dst);
            prepend_source_geometry(&mut answer.coordinates, src_projected, Vec::new());
            append_destination_geometry(&mut answer.coordinates, Vec::new(), dst_projected);
        } else {
            if let Some(start_node) = answer.path.first().copied() {
                let connected = self
                    .spatial
                    .connector_points_from_snap_to_node(src, start_node);
                prepend_source_geometry(&mut answer.coordinates, src_projected, connected);
            } else {
                prepend_source_geometry(&mut answer.coordinates, src_projected, Vec::new());
            }
            if let Some(end_node) = answer.path.last().copied() {
                let mut connector = self
                    .spatial
                    .connector_points_from_snap_to_node(dst, end_node);
                connector.reverse();
                append_destination_geometry(&mut answer.coordinates, connector, dst_projected);
            } else {
                append_destination_geometry(&mut answer.coordinates, Vec::new(), dst_projected);
            }
        }

        answer.origin = Some(from);
        answer.destination = Some(to);
        answer.snapped_origin = None;
        answer.snapped_destination = None;
        answer.distance_m = route_distance_m(&answer.coordinates);
        answer
    }

    fn coordinates_for_path(&self, path: &[NodeId], route_arc_ids: &[u32]) -> Vec<(f32, f32)> {
        if route_arc_ids.is_empty() {
            path.iter()
                .map(|&node| {
                    (
                        self.context.graph.latitude[node as usize],
                        self.context.graph.longitude[node as usize],
                    )
                })
                .collect()
        } else {
            self.spatial.expand_route_arc_geometry(route_arc_ids)
        }
    }

    fn reconstruct_arc_ids(&self, path: &[NodeId]) -> Option<Vec<u32>> {
        if path.len() < 2 {
            return Some(Vec::new());
        }

        let mut arc_ids = Vec::with_capacity(path.len() - 1);
        for window in path.windows(2) {
            let tail = window[0] as usize;
            let head = window[1];
            let start = self.context.graph.first_out[tail] as usize;
            let end = self.context.graph.first_out[tail + 1] as usize;
            let edge_idx =
                (start..end).find(|&edge_idx| self.context.graph.head[edge_idx] == head)?;
            arc_ids.push(edge_idx as u32);
        }
        Some(arc_ids)
    }

    /// Apply new weights and re-customize. The CCH topology is reused.
    /// The old customized data is dropped and replaced atomically via swap.
    pub fn update_weights(&mut self, weights: &[Weight]) {
        let new_customized = self.context.customize_with(weights);
        self.server.update(new_customized);
    }

    pub fn bbox(&self) -> &BoundingBox {
        self.spatial.bbox()
    }

    pub fn validation_config(&self) -> &ValidationConfig {
        &self.validation_config
    }

    /// Access the snap-to-edge spatial index.
    pub fn spatial(&self) -> &SpatialIndex {
        &self.spatial
    }

    /// Access the underlying CCH context.
    pub fn context(&self) -> &'a CchContext {
        self.context
    }
}