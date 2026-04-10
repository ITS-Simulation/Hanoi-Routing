use std::path::Path;

use rust_road_router::algo::customizable_contraction_hierarchy::CustomizedBasic;
use rust_road_router::algo::customizable_contraction_hierarchy::query::Server as CchQueryServer;
use rust_road_router::algo::customizable_contraction_hierarchy::{
    CCH, CCHT, DirectedCCH, customize_directed,
};
use rust_road_router::algo::{Query, QueryServer};
use rust_road_router::datastr::graph::{EdgeId, EdgeIdT, FirstOutGraph, INFINITY, NodeId, Weight};
use rust_road_router::datastr::node_order::NodeOrder;
use rust_road_router::io::Load;
use rust_road_router::util::Storage;

use crate::bounds::{BoundingBox, CoordRejection, ValidationConfig};
use crate::cch::{
    QueryAnswer, append_destination_geometry, prepend_source_geometry, route_distance_m,
    select_tiered_snap_pair,
};
use crate::cch_cache::CchCache;
use crate::geometry::{annotate_distances, compute_turns, refine_turns};
use crate::graph::GraphData;
use crate::multi_route::{AlternativeServer, GEO_OVER_REQUEST, MAX_GEO_RATIO};
use crate::spatial::{SNAP_MAX_CANDIDATES, SnapResult, SpatialIndex};

/// CCH context for line graphs. Uses `DirectedCCH` (pruned — no always-INFINITY
/// edges) for efficient turn-expanded graph routing.
pub struct LineGraphCchContext {
    /// Line graph data (CSR).
    pub graph: GraphData,

    /// Pruned directed CCH topology.
    pub directed_cch: DirectedCCH,

    /// Baseline line-graph weights.
    pub baseline_weights: Storage<Weight>,

    /// Original graph's CSR offset array (for building the original-space spatial index).
    pub original_first_out: Storage<EdgeId>,

    /// Original graph's tail array: `original_tail[edge_i]` = source node of edge i.
    /// Reconstructed from the original graph's `first_out` at load time.
    pub original_tail: Storage<NodeId>,

    /// Original graph's head array: `original_head[edge_i]` = target node of edge i.
    pub original_head: Storage<NodeId>,

    /// Original graph's node latitudes (for path coordinate output).
    pub original_latitude: Storage<f32>,

    /// Original graph's node longitudes (for path coordinate output).
    pub original_longitude: Storage<f32>,

    /// Original graph's per-edge shape-point offsets.
    pub original_first_modelling_node: Option<Storage<u32>>,

    /// Original graph's modelling-node latitudes.
    pub original_modelling_node_latitude: Option<Storage<f32>>,

    /// Original graph's modelling-node longitudes.
    pub original_modelling_node_longitude: Option<Storage<f32>>,

    /// Original graph's travel_time (for source-edge correction at query time).
    pub original_travel_time: Storage<Weight>,

    /// Maps each line-graph node back to the original directed arc it
    /// represents. Split nodes clone an original arc and therefore point back
    /// to that arc ID.
    pub original_arc_id_of_lg_node: Storage<u32>,

    /// Per-LG-node flag: true if the original arc belongs to a roundabout way.
    pub is_arc_roundabout: Storage<u8>,
}

fn validate_edge_mappings(cch: &DirectedCCH, num_metric_edges: usize) -> std::io::Result<()> {
    let check = |cond: bool, msg: String| -> std::io::Result<()> {
        if cond {
            Ok(())
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::InvalidData, msg))
        }
    };

    for (edge_idx, arcs) in cch.forward_cch_edge_to_orig_arc().iter().enumerate() {
        for &EdgeIdT(arc) in arcs {
            check(
                (arc as usize) < num_metric_edges,
                format!(
                    "fw edge_to_orig[{edge_idx}] contains arc {arc} >= num_metric_edges {num_metric_edges}"
                ),
            )?;
        }
    }

    for (edge_idx, arcs) in cch.backward_cch_edge_to_orig_arc().iter().enumerate() {
        for &EdgeIdT(arc) in arcs {
            check(
                (arc as usize) < num_metric_edges,
                format!(
                    "bw edge_to_orig[{edge_idx}] contains arc {arc} >= num_metric_edges {num_metric_edges}"
                ),
            )?;
        }
    }

    Ok(())
}

fn update_turns_after_coordinate_patch(
    turns: &mut Vec<crate::geometry::TurnAnnotation>,
    coordinates: &[(f32, f32)],
    prepended_count: usize,
    clipped_source_count: usize,
    clipped_destination_count: usize,
    original_route_len: usize,
) {
    if turns.is_empty() {
        return;
    }

    let retained_route_end = original_route_len.saturating_sub(clipped_destination_count);
    let mut remapped = Vec::with_capacity(turns.len());

    for mut turn in std::mem::take(turns) {
        if turn.coordinate_index == 0 {
            remapped.push(turn);
            continue;
        }

        let original_index = turn.coordinate_index as usize;
        if original_index < clipped_source_count || original_index >= retained_route_end {
            continue;
        }

        turn.coordinate_index = (prepended_count + original_index - clipped_source_count) as u32;
        remapped.push(turn);
    }

    *turns = remapped;
    annotate_distances(turns, coordinates);
}

fn point_distance_m(a: (f32, f32), b: (f32, f32)) -> f64 {
    crate::spatial::haversine_m(a.0 as f64, a.1 as f64, b.0 as f64, b.1 as f64)
}

/// Clip backtrack protrusion at the start of a coordinate chain.
///
/// coordinates[0] = P2 (projected point on snapped edge).
/// The route may backtrack past P2 before heading toward the destination.
/// We find where the route polyline passes closest to P2 (by perpendicular
/// projection onto each segment) and replace everything before that point
/// with [P2, projection_on_segment].
fn clip_backtrack_protrusion_from_start(
    coordinates: &mut Vec<(f32, f32)>,
    anchor: (f32, f32),
) -> usize {
    if coordinates.len() < 3 {
        return 0;
    }

    // Skip coordinates[0] (= anchor) and coordinates[1] (first route point).
    // Search segments [1→2], [2→3], ... for the one closest to anchor.
    // The segment where the route "crosses back" past P2 will have the
    // smallest perpendicular distance.
    let mut best_dist = f64::MAX;
    let mut best_seg = 0usize; // index of segment start
    let mut best_t = 0.0f64;

    for seg in 1..(coordinates.len() - 1) {
        let a = coordinates[seg];
        let b = coordinates[seg + 1];
        let (dist, t) = perpendicular_distance_and_t(anchor, a, b);
        if dist < best_dist {
            best_dist = dist;
            best_seg = seg;
            best_t = t;
        }
        // Once we found a close segment and distance is growing, stop.
        if best_dist < 10.0 && dist > best_dist * 3.0 {
            break;
        }
    }

    // If the best segment is [0→1] or [1→2] with t=0, no real backtrack.
    if best_seg <= 1 && best_t < 0.01 {
        return 0;
    }

    // Check that this is actually closer than coordinates[1] to anchor.
    // If not, there's no backtrack to clip.
    let dist_to_first = point_distance_m(anchor, coordinates[1]);
    if best_dist >= dist_to_first {
        return 0;
    }

    // Interpolate the crossing point on the best segment.
    let a = coordinates[best_seg];
    let b = coordinates[best_seg + 1];
    let crossing = (
        a.0 + (b.0 - a.0) * best_t as f32,
        a.1 + (b.1 - a.1) * best_t as f32,
    );

    // Clip: [P2, crossing, seg+1, seg+2, ...]
    let clip_idx = best_seg + 1;
    let tail = coordinates.split_off(clip_idx);
    coordinates.truncate(1); // keep [P2]
    coordinates.push(crossing);
    coordinates.extend(tail);
    // We removed indices 1..clip_idx (= clip_idx-1 points) and added 1 (crossing)
    // Net removed = clip_idx - 1 - 1 = clip_idx - 2... but the return value is
    // "number of coordinates removed" for turn index adjustment.
    // Original had clip_idx points between [0] and [clip_idx].
    // We replaced them with 1 point (crossing). So removed = clip_idx - 1 - 1.
    // Actually: we had coordinates[1..clip_idx] removed = clip_idx-1 points,
    // and inserted 1 crossing point. Net shift = clip_idx - 2.
    if clip_idx >= 2 { clip_idx - 2 } else { 0 }
}

/// Perpendicular distance from point P to segment A→B, plus the projection
/// parameter t ∈ [0,1].
fn perpendicular_distance_and_t(
    p: (f32, f32),
    a: (f32, f32),
    b: (f32, f32),
) -> (f64, f64) {
    let dx = (b.0 - a.0) as f64;
    let dy = (b.1 - a.1) as f64;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-14 {
        return (point_distance_m(p, a), 0.0);
    }
    let t = (((p.0 - a.0) as f64 * dx + (p.1 - a.1) as f64 * dy) / len_sq).clamp(0.0, 1.0);
    let proj = (a.0 as f64 + dx * t, a.1 as f64 + dy * t);
    let dist = point_distance_m(p, (proj.0 as f32, proj.1 as f32));
    (dist, t)
}

fn clip_backtrack_protrusion_from_end(
    coordinates: &mut Vec<(f32, f32)>,
    anchor: (f32, f32),
) -> usize {
    coordinates.reverse();
    let clipped = clip_backtrack_protrusion_from_start(coordinates, anchor);
    coordinates.reverse();
    clipped
}

impl LineGraphCchContext {
    /// Load line graph + original graph metadata, build DirectedCCH.
    ///
    /// `line_graph_dir` — directory with line graph CSR files
    /// `original_graph_dir` — directory with original graph files (head, lat, lng, travel_time)
    /// `perm_path` — path to the line graph's `cch_perm` file
    #[tracing::instrument(skip_all, fields(
        line_graph_dir = %line_graph_dir.display(),
        original_graph_dir = %original_graph_dir.display()
    ))]
    pub fn load_and_build(
        line_graph_dir: &Path,
        original_graph_dir: &Path,
        perm_path: &Path,
    ) -> std::io::Result<Self> {
        let graph = GraphData::load(line_graph_dir)?;
        let perm: Vec<NodeId> = Vec::load_from(perm_path)?;
        let order = NodeOrder::from_node_order(perm);

        // Load original graph metadata needed for coordinate mapping and final-edge correction
        let original_graph = GraphData::load(original_graph_dir)?;
        let original_first_out = original_graph.first_out.clone();
        let original_head = original_graph.head.clone();
        let original_latitude = original_graph.latitude.clone();
        let original_longitude = original_graph.longitude.clone();
        let original_first_modelling_node = original_graph.first_modelling_node.clone();
        let original_modelling_node_latitude = original_graph.modelling_node_latitude.clone();
        let original_modelling_node_longitude = original_graph.modelling_node_longitude.clone();
        let original_travel_time = original_graph.travel_time.clone();

        // Reconstruct tail array from original first_out (CSR → per-edge tail node).
        // tail[edge_i] = the node whose adjacency list contains edge i.
        let num_original_edges = original_head.len();
        let mut original_tail = Vec::with_capacity(num_original_edges);
        for node in 0..(original_first_out.len() - 1) {
            let degree = (original_first_out[node + 1] - original_first_out[node]) as usize;
            for _ in 0..degree {
                original_tail.push(node as NodeId);
            }
        }
        let original_tail = Storage::from_vec(original_tail);

        // Load via-way split map — mandatory. Extends reconstruction arrays for
        // split nodes so that path unpacking maps them back to original arcs.
        let split_map = Storage::<u32>::mmap(line_graph_dir.join("via_way_split_map")).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "Missing required file 'via_way_split_map' in {}: {}. Re-run generate_line_graph.",
                        line_graph_dir.display(),
                        e
                    ),
                )
            })?;

        let mut original_arc_id_of_lg_node: Vec<u32> = (0..num_original_edges)
            .map(|arc_id| arc_id as u32)
            .collect();

        // Extend the LG-node→original-arc mapping for split nodes.
        // Split node i (graph node num_original_edges + i) was cloned from
        // original LG node split_map[i], so it maps to the same original arc.
        for &original in split_map.iter() {
            if original as usize >= num_original_edges {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "via_way_split_map contains original arc {} outside original edge count {}",
                        original, num_original_edges
                    ),
                ));
            }
            original_arc_id_of_lg_node.push(original);
        }
        let original_arc_id_of_lg_node = Storage::from_vec(original_arc_id_of_lg_node);

        // Consistency check: the LG-node→original-arc mapping must cover all LG nodes
        // (base nodes = original edges, plus split nodes).
        let num_lg_nodes = graph.num_nodes();
        if original_arc_id_of_lg_node.len() != num_lg_nodes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "original arc reconstruction length ({}) does not match line graph node count ({})",
                    original_arc_id_of_lg_node.len(),
                    num_lg_nodes
                ),
            ));
        }

        tracing::info!(
            num_original_edges,
            split_nodes = split_map.len(),
            "reconstruction arrays built"
        );

        let num_nodes = graph.num_nodes();
        let num_edges = graph.num_edges();
        let is_arc_roundabout = Storage::<u8>::mmap(line_graph_dir.join("is_arc_roundabout"))
            .unwrap_or_else(|_| Storage::from_vec(vec![0u8; num_lg_nodes]));

        if is_arc_roundabout.len() != num_lg_nodes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "is_arc_roundabout length ({}) does not match line graph node count ({})",
                    is_arc_roundabout.len(),
                    num_lg_nodes
                ),
            ));
        }

        tracing::info!(num_nodes, num_edges, "preparing DirectedCCH for line graph");

        let borrowed = graph.as_borrowed_graph();
        let cache = CchCache::new(line_graph_dir);
        let source_files = [
            line_graph_dir.join("first_out"),
            line_graph_dir.join("head"),
            perm_path.to_path_buf(),
        ];
        let source_refs: Vec<&Path> = source_files.iter().map(|path| path.as_path()).collect();
        let num_metric_edges = graph.num_edges();
        let directed_cch = 'build: {
            if cache.is_valid(&source_refs) {
                match cache.load() {
                    Ok(loaded) => {
                        if let Err(err) = validate_edge_mappings(&loaded, num_metric_edges) {
                            tracing::warn!(
                                "cached DirectedCCH edge mappings invalid: {err}; rebuilding"
                            );
                        } else {
                            tracing::info!("loaded DirectedCCH from cache");
                            break 'build loaded;
                        }
                    }
                    Err(err) => {
                        tracing::warn!("cached DirectedCCH failed validation: {err}; rebuilding");
                    }
                }
            }

            tracing::info!("building DirectedCCH from scratch");
            let cch = CCH::fix_order_and_build(&borrowed, order);
            let built = cch.to_directed_cch();
            if let Err(err) = cache.save(&built, &source_refs) {
                tracing::warn!("failed to write DirectedCCH cache: {err}");
            }
            built
        };

        let baseline_weights = graph.travel_time.clone();

        Ok(LineGraphCchContext {
            graph,
            directed_cch,
            baseline_weights,
            original_first_out,
            original_tail,
            original_head,
            original_latitude,
            original_longitude,
            original_first_modelling_node,
            original_modelling_node_latitude,
            original_modelling_node_longitude,
            original_travel_time,
            original_arc_id_of_lg_node,
            is_arc_roundabout,
        })
    }

    fn original_arc_id(&self, lg_node: NodeId) -> usize {
        self.original_arc_id_of_lg_node[lg_node as usize] as usize
    }

    fn lg_tail_node(&self, lg_node: NodeId) -> NodeId {
        self.original_tail[self.original_arc_id(lg_node)]
    }

    fn lg_head_node(&self, lg_node: NodeId) -> NodeId {
        self.original_head[self.original_arc_id(lg_node)]
    }

    fn lg_travel_time(&self, lg_node: NodeId) -> Weight {
        self.original_travel_time[self.original_arc_id(lg_node)]
    }

    /// Customize with baseline weights (Phase 2, directed variant).
    #[tracing::instrument(skip_all)]
    pub fn customize(&self) -> CustomizedBasic<'_, DirectedCCH> {
        let metric = FirstOutGraph::new(
            &self.graph.first_out[..],
            &self.graph.head[..],
            &self.baseline_weights[..],
        );
        customize_directed(&self.directed_cch, &metric)
    }

    /// Customize with caller-provided weights.
    #[tracing::instrument(skip_all, fields(num_weights = weights.len()))]
    pub fn customize_with(&self, weights: &[Weight]) -> CustomizedBasic<'_, DirectedCCH> {
        let metric = FirstOutGraph::new(&self.graph.first_out[..], &self.graph.head[..], weights);
        customize_directed(&self.directed_cch, &metric)
    }
}

/// Query engine for line graphs. Uses DirectedCCH and applies final-edge
/// correction automatically.
///
/// Coordinate queries snap in the **original graph's** intersection-node
/// coordinate space (unified snap), then use the snapped original edge ID
/// directly as a line-graph node ID. This ensures the line graph engine
/// selects the same physical road segment as the normal `QueryEngine`.
pub struct LineGraphQueryEngine<'a> {
    server: CchQueryServer<CustomizedBasic<'a, DirectedCCH>>,
    context: &'a LineGraphCchContext,
    /// Spatial index on the **original graph's** intersection-node coordinates.
    /// Used by `query_coords` for unified snapping.
    original_spatial: SpatialIndex,
    validation_config: ValidationConfig,
}

impl<'a> LineGraphQueryEngine<'a> {
    /// Create a line graph query engine. Performs initial customization
    /// with baseline weights and builds the original-graph spatial index
    /// for unified coordinate snapping.
    pub fn new(context: &'a LineGraphCchContext) -> Self {
        Self::with_validation_config(context, ValidationConfig::default())
    }

    pub fn with_validation_config(
        context: &'a LineGraphCchContext,
        validation_config: ValidationConfig,
    ) -> Self {
        let customized = context.customize();
        let server = CchQueryServer::new(customized);

        // Build spatial index on the ORIGINAL graph's intersection-node coordinates.
        // This ensures coordinate queries snap to the same physical road as the
        // normal QueryEngine. The snapped original edge ID is then used directly
        // as a line-graph node ID (line-graph node N = original edge N).
        let original_spatial = SpatialIndex::build_with_shape_points(
            &context.original_latitude,
            &context.original_longitude,
            &context.original_first_out,
            &context.original_head,
            context.original_first_modelling_node.as_deref(),
            context.original_modelling_node_latitude.as_deref(),
            context.original_modelling_node_longitude.as_deref(),
        );

        LineGraphQueryEngine {
            server,
            context,
            original_spatial,
            validation_config,
        }
    }

    /// Query by snapped coordinates in line-graph space while preserving the
    /// exact projected entry and exit positions on the snapped arcs.
    fn query_trimmed(&mut self, src: &SnapResult, dst: &SnapResult) -> Option<QueryAnswer> {
        self.query_coordinate_candidate(src, dst)
    }

    /// Query by line-graph node IDs (= original edge indices).
    /// Adds the source edge's travel_time (excluded from the shifted
    /// line-graph encoding where each LG edge carries `tt(next_edge)`).
    pub fn query(&mut self, source_edge: EdgeId, target_edge: EdgeId) -> Option<QueryAnswer> {
        let result = self.server.query(Query {
            from: source_edge as NodeId,
            to: target_edge as NodeId,
        });

        if let Some(mut connected) = result.found() {
            let cch_distance = connected.distance();
            if cch_distance >= INFINITY {
                return None;
            }

            // Source-edge correction: the shifted encoding includes tt(target_edge)
            // in cch_distance but excludes tt(source_edge). Since source_edge is
            // constant for all candidate paths, adding it post-hoc is correct.
            let source_edge_cost = self.context.lg_travel_time(source_edge);
            let lg_path = connected.node_path();
            self.build_answer_from_lg_path(cch_distance, &lg_path, source_edge_cost, false)
        } else {
            None
        }
    }

    /// Query by coordinates using **unified snap space**.
    ///
    /// Snaps coordinates in the **original graph's** intersection-node coordinate
    /// space (identical to `QueryEngine::query_coords`), then uses the snapped
    /// original edge ID directly as a line-graph node ID. This eliminates the
    /// coordinate-space divergence that previously caused different edge selection
    /// between normal and line-graph engines.
    pub fn query_coords(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
    ) -> Result<Option<QueryAnswer>, CoordRejection> {
        let src_snaps = self.original_spatial.validated_snap_candidates(
            "origin",
            from.0,
            from.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;
        let dst_snaps = self.original_spatial.validated_snap_candidates(
            "destination",
            to.0,
            to.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;

        Ok(
            match select_tiered_snap_pair(&src_snaps, &dst_snaps, |src, dst| {
                tracing::debug!(
                    src_edge = src.edge_id,
                    dst_edge = dst.edge_id,
                    src_snap_dist_m = src.snap_distance_m,
                    dst_snap_dist_m = dst.snap_distance_m,
                    "unified snap candidate pair: original edge IDs -> line-graph node IDs"
                );
                self.query_trimmed(src, dst)
            }) {
                Some((src, dst, answer)) => {
                    Some(self.patch_coordinates(answer, from, to, &src, &dst))
                }
                _ => None,
            },
        )
    }

    /// Attach the user's origin/destination metadata and splice snap-edge
    /// connector geometry around the routed path when needed.
    fn direct_same_edge_coordinate_answer(
        &self,
        src: &SnapResult,
        dst: &SnapResult,
    ) -> Option<QueryAnswer> {
        let edge_cost = self.context.lg_travel_time(src.edge_id);
        let distance_ms = self
            .original_spatial
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

    fn coordinate_distance_ms(
        &self,
        cch_distance: Weight,
        src: &SnapResult,
        dst: &SnapResult,
    ) -> Weight {
        let src_edge_cost = self.context.lg_travel_time(src.edge_id);
        let dst_edge_cost = self.context.lg_travel_time(dst.edge_id);

        cch_distance
            .saturating_sub(dst_edge_cost)
            .saturating_add(self.original_spatial.partial_edge_cost_to_node_ms(
                src,
                src.head,
                src_edge_cost,
            ))
            .saturating_add(self.original_spatial.partial_edge_cost_to_node_ms(
                dst,
                dst.tail,
                dst_edge_cost,
            ))
    }

    fn build_coordinate_answer_from_lg_path(
        &self,
        cch_distance: Weight,
        lg_path: &[NodeId],
        src: &SnapResult,
        dst: &SnapResult,
    ) -> Option<QueryAnswer> {
        let distance_ms = self.coordinate_distance_ms(cch_distance, src, dst);
        self.materialize_lg_path(distance_ms, lg_path, true)
    }

    fn query_same_edge_cycle_candidate(
        &mut self,
        src: &SnapResult,
        dst: &SnapResult,
    ) -> Option<QueryAnswer> {
        let start = self.context.graph.first_out[src.edge_id as usize] as usize;
        let end = self.context.graph.first_out[src.edge_id as usize + 1] as usize;
        let mut best: Option<QueryAnswer> = None;

        for edge_idx in start..end {
            let next = self.context.graph.head[edge_idx];
            let first_transition_cost = self.context.graph.travel_time[edge_idx];
            let result = self.server.query(Query {
                from: next,
                to: dst.edge_id as NodeId,
            });

            let Some(mut connected) = result.found() else {
                continue;
            };
            let rest_distance = connected.distance();
            if rest_distance >= INFINITY {
                continue;
            }

            let mut lg_path = Vec::with_capacity(connected.node_path().len() + 1);
            lg_path.push(src.edge_id as NodeId);
            lg_path.extend(connected.node_path());

            if let Some(answer) = self.build_coordinate_answer_from_lg_path(
                first_transition_cost.saturating_add(rest_distance),
                &lg_path,
                src,
                dst,
            ) {
                if best
                    .as_ref()
                    .map_or(true, |current| answer.distance_ms < current.distance_ms)
                {
                    best = Some(answer);
                }
            }
        }

        best
    }

    fn query_coordinate_candidate(
        &mut self,
        src: &SnapResult,
        dst: &SnapResult,
    ) -> Option<QueryAnswer> {
        if src.edge_id == dst.edge_id {
            return self
                .direct_same_edge_coordinate_answer(src, dst)
                .or_else(|| self.query_same_edge_cycle_candidate(src, dst));
        }

        let result = self.server.query(Query {
            from: src.edge_id as NodeId,
            to: dst.edge_id as NodeId,
        });

        if let Some(mut connected) = result.found() {
            let cch_distance = connected.distance();
            if cch_distance >= INFINITY {
                return None;
            }

            let lg_path = connected.node_path();
            self.build_coordinate_answer_from_lg_path(cch_distance, &lg_path, src, dst)
        } else {
            None
        }
    }

    fn multi_query_coordinate_candidates(
        &mut self,
        src: &SnapResult,
        dst: &SnapResult,
        max_alternatives: usize,
        stretch_factor: f64,
    ) -> Vec<QueryAnswer> {
        if src.edge_id == dst.edge_id {
            return self
                .direct_same_edge_coordinate_answer(src, dst)
                .or_else(|| self.query_same_edge_cycle_candidate(src, dst))
                .into_iter()
                .collect();
        }

        let customized = self.server.customized();
        let mut multi = AlternativeServer::new(customized);
        let request_count = max_alternatives
            .saturating_mul(GEO_OVER_REQUEST)
            .max(max_alternatives + 10);
        let geo_len = self.lg_path_geo_len();
        let candidates = multi.alternatives(
            src.edge_id as NodeId,
            dst.edge_id as NodeId,
            request_count,
            stretch_factor,
            geo_len,
        );

        let mut answers: Vec<QueryAnswer> = Vec::with_capacity(max_alternatives);
        let mut shortest_geo_dist: Option<f64> = None;

        for alt in candidates {
            if answers.len() >= max_alternatives {
                break;
            }
            if let Some(answer) =
                self.build_coordinate_answer_from_lg_path(alt.distance, &alt.path, src, dst)
            {
                if let Some(base) = shortest_geo_dist {
                    if answer.distance_m > base * MAX_GEO_RATIO {
                        continue;
                    }
                } else {
                    shortest_geo_dist = Some(answer.distance_m);
                }
                answers.push(answer);
            }
        }

        answers
    }

    fn has_reverse_edge(&self, edge_id: EdgeId) -> bool {
        let tail = self.context.original_tail[edge_id as usize];
        let head = self.context.original_head[edge_id as usize];
        let start = self.context.original_first_out[head as usize] as usize;
        let end = self.context.original_first_out[head as usize + 1] as usize;

        (start..end).any(|edge_idx| self.context.original_head[edge_idx] == tail)
    }

    fn patch_coordinates(
        &self,
        mut answer: QueryAnswer,
        from: (f32, f32),
        to: (f32, f32),
        src: &SnapResult,
        dst: &SnapResult,
    ) -> QueryAnswer {
        let src_projected = src.projected_point(
            &self.context.original_latitude,
            &self.context.original_longitude,
        );
        let dst_projected = dst.projected_point(
            &self.context.original_latitude,
            &self.context.original_longitude,
        );
        let original_route_len = answer.coordinates.len();
        let prepended_count = if answer.coordinates.is_empty() && src.edge_id == dst.edge_id {
            answer.coordinates = self.original_spatial.open_interval_between_snaps(src, dst);
            let prepended =
                prepend_source_geometry(&mut answer.coordinates, src_projected, Vec::new());
            let _ = append_destination_geometry(&mut answer.coordinates, Vec::new(), dst_projected);
            prepended
        } else {
            // Add connector geometry from snap point along the snapped edge to
            // the route entry/exit node.
            //
            // In line-graph trimmed paths:
            //   path[0]  = tail(first_interior_arc) = head(src_edge) = src.head
            //   path[-1] = head(last_interior_arc)  = tail(dst_edge) = dst.tail
            //
            // So connector_points_from_snap_to_node(src, src.head) returns the
            // source edge polyline from snap→head, and
            // connector_points_from_snap_to_node(dst, dst.tail) returns the
            // dest edge polyline from snap→tail (reversed for appending).

            let src_connector = if let Some(start_node) = answer.path.first().copied() {
                self.original_spatial
                    .connector_points_from_snap_to_node(src, start_node)
            } else {
                Vec::new()
            };
            let dst_connector = if let Some(end_node) = answer.path.last().copied() {
                let mut conn = self
                    .original_spatial
                    .connector_points_from_snap_to_node(dst, end_node);
                conn.reverse();
                conn
            } else {
                Vec::new()
            };

            let prepended =
                prepend_source_geometry(&mut answer.coordinates, src_projected, src_connector);
            let appended =
                append_destination_geometry(&mut answer.coordinates, dst_connector, dst_projected);

            // Clip source-side backtrack: when the route starts by going away
            // from the projected point (edge faces wrong direction), find where
            // the route crosses back and clip the protrusion.
            let clipped_source_count = if prepended > 0 && self.has_reverse_edge(src.edge_id) {
                clip_backtrack_protrusion_from_start(&mut answer.coordinates, src_projected)
            } else {
                0
            };

            // Clip destination-side backtrack: mirror of source clipping.
            let clipped_destination_count = if appended > 0 && self.has_reverse_edge(dst.edge_id) {
                clip_backtrack_protrusion_from_end(&mut answer.coordinates, dst_projected)
            } else {
                0
            };

            answer.origin = Some(from);
            answer.destination = Some(to);
            answer.snapped_origin = None;
            answer.snapped_destination = None;
            update_turns_after_coordinate_patch(
                &mut answer.turns,
                &answer.coordinates,
                prepended,
                clipped_source_count,
                clipped_destination_count,
                original_route_len,
            );
            answer.distance_m = route_distance_m(&answer.coordinates);
            return answer;
        };

        answer.origin = Some(from);
        answer.destination = Some(to);
        answer.snapped_origin = None;
        answer.snapped_destination = None;
        update_turns_after_coordinate_patch(
            &mut answer.turns,
            &answer.coordinates,
            prepended_count,
            0,
            0,
            original_route_len,
        );
        answer.distance_m = route_distance_m(&answer.coordinates);
        answer
    }

    fn build_answer_from_lg_path(
        &self,
        cch_distance: Weight,
        lg_path: &[NodeId],
        source_edge_cost: Weight,
        trimmed: bool,
    ) -> Option<QueryAnswer> {
        if lg_path.is_empty() {
            return None;
        }

        let distance_ms = cch_distance.saturating_add(source_edge_cost);
        self.materialize_lg_path(distance_ms, lg_path, trimmed)
    }

    fn materialize_lg_path(
        &self,
        distance_ms: Weight,
        lg_path: &[NodeId],
        trimmed: bool,
    ) -> Option<QueryAnswer> {
        if lg_path.is_empty() {
            return None;
        }

        let effective_path: &[NodeId] = if trimmed && lg_path.len() > 2 {
            &lg_path[1..lg_path.len() - 1]
        } else if trimmed {
            &[]
        } else {
            lg_path
        };

        let route_arc_ids: Vec<u32> = effective_path
            .iter()
            .map(|&lg_node| self.context.original_arc_id_of_lg_node[lg_node as usize])
            .collect();
        let weight_path_ids: Vec<u32> = lg_path.iter().map(|&lg_node| lg_node as u32).collect();

        let mut path: Vec<NodeId> = effective_path
            .iter()
            .map(|&lg_node| self.context.lg_tail_node(lg_node))
            .collect();

        if trimmed {
            if let Some(&last_edge) = effective_path.last() {
                path.push(self.context.lg_head_node(last_edge));
            } else if lg_path.len() >= 2 {
                path.push(self.context.lg_head_node(lg_path[0]));
            }
        } else if let Some(&last_edge) = lg_path.last() {
            path.push(self.context.lg_head_node(last_edge));
        }

        let (coordinates, arc_end_coordinate_indices) = if route_arc_ids.is_empty() {
            (
                path.iter()
                    .map(|&node| {
                        (
                            self.context.original_latitude[node as usize],
                            self.context.original_longitude[node as usize],
                        )
                    })
                    .collect(),
                Vec::new(),
            )
        } else {
            self.original_spatial
                .expand_route_arc_geometry_with_boundaries(&route_arc_ids)
        };

        let mut turns = compute_turns(
            effective_path,
            &self.context.original_arc_id_of_lg_node,
            &self.context.original_tail,
            &self.context.original_head,
            &self.context.original_first_out,
            &self.context.original_latitude,
            &self.context.original_longitude,
            &self.context.is_arc_roundabout,
        );
        for turn in &mut turns {
            let arc_idx = turn.coordinate_index.saturating_sub(1) as usize;
            if let Some(&coordinate_index) = arc_end_coordinate_indices.get(arc_idx) {
                turn.coordinate_index = coordinate_index;
            }
        }
        refine_turns(&mut turns, &coordinates);

        let distance_m = route_distance_m(&coordinates);

        Some(QueryAnswer {
            distance_ms,
            distance_m,
            route_arc_ids,
            weight_path_ids,
            path,
            coordinates,
            turns,
            origin: None,
            destination: None,
            snapped_origin: None,
            snapped_destination: None,
        })
    }

    /// Find up to `max_alternatives` alternative routes by line-graph node IDs
    /// (= original edge indices). Each route gets source-edge correction,
    /// LG->original node mapping, and turn annotation.
    #[tracing::instrument(
        skip(self),
        fields(source_edge, target_edge, max_alternatives, stretch_factor)
    )]
    pub fn multi_query(
        &self,
        source_edge: EdgeId,
        target_edge: EdgeId,
        max_alternatives: usize,
        stretch_factor: f64,
    ) -> Vec<QueryAnswer> {
        let customized = self.server.customized();
        let mut multi = AlternativeServer::new(customized);
        let request_count = max_alternatives
            .saturating_mul(GEO_OVER_REQUEST)
            .max(max_alternatives + 10);
        let geo_len = self.lg_path_geo_len();
        let candidates = multi.alternatives(
            source_edge as NodeId,
            target_edge as NodeId,
            request_count,
            stretch_factor,
            geo_len,
        );

        let source_edge_cost = self.context.lg_travel_time(source_edge);

        let mut results: Vec<QueryAnswer> = Vec::with_capacity(max_alternatives);
        let mut shortest_geo_dist: Option<f64> = None;

        for alt in candidates {
            if results.len() >= max_alternatives {
                break;
            }
            if let Some(answer) =
                self.build_answer_from_lg_path(alt.distance, &alt.path, source_edge_cost, false)
            {
                if let Some(base) = shortest_geo_dist {
                    if answer.distance_m > base * MAX_GEO_RATIO {
                        continue;
                    }
                } else {
                    shortest_geo_dist = Some(answer.distance_m);
                }
                results.push(answer);
            }
        }

        tracing::info!(
            requested = max_alternatives,
            returned = results.len(),
            shortest_ms = results.first().map(|route| route.distance_ms),
            "line_graph multi_query completed"
        );

        results
    }

    /// Find up to `max_alternatives` alternative routes by coordinates.
    /// Snaps in the original graph coordinate space, then runs multi_query on
    /// the snapped edge IDs with trimming.
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
        let src_snaps = self.original_spatial.validated_snap_candidates(
            "origin",
            from.0,
            from.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;
        let dst_snaps = self.original_spatial.validated_snap_candidates(
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
            let answers: Vec<QueryAnswer> = answers
                .into_iter()
                .map(|answer| self.patch_coordinates(answer, from, to, &src, &dst))
                .collect();
            tracing::info!(
                requested = max_alternatives,
                returned = answers.len(),
                shortest_ms = answers.first().map(|route| route.distance_ms),
                "line_graph multi_query_coords completed"
            );
            Ok(answers)
        } else {
            tracing::info!(
                requested = max_alternatives,
                returned = 0usize,
                "line_graph multi_query_coords completed"
            );
            Ok(Vec::new())
        }
    }

    /// Apply new weights and re-customize (directed variant).
    pub fn update_weights(&mut self, weights: &[Weight]) {
        let new_customized = self.context.customize_with(weights);
        self.server.update(new_customized);
    }

    pub fn bbox(&self) -> &BoundingBox {
        self.original_spatial.bbox()
    }

    pub fn validation_config(&self) -> &ValidationConfig {
        &self.validation_config
    }

    /// Access the original-graph spatial index (used for unified snapping).
    pub fn spatial(&self) -> &SpatialIndex {
        &self.original_spatial
    }

    /// Access the underlying line graph CCH context.
    pub fn context(&self) -> &'a LineGraphCchContext {
        self.context
    }

    fn lg_path_geo_len(&self) -> impl Fn(&[NodeId]) -> f64 + '_ {
        let lg_to_orig = &self.context.original_arc_id_of_lg_node;
        let orig_tail = &self.context.original_tail;
        let orig_head = &self.context.original_head;
        let orig_lat = &self.context.original_latitude;
        let orig_lng = &self.context.original_longitude;

        move |lg_path: &[NodeId]| -> f64 {
            let mut coords: Vec<(f32, f32)> = lg_path
                .iter()
                .map(|&lg_node| {
                    let node = orig_tail[lg_to_orig[lg_node as usize] as usize];
                    (orig_lat[node as usize], orig_lng[node as usize])
                })
                .collect();
            if let Some(&last) = lg_path.last() {
                let node = orig_head[lg_to_orig[last as usize] as usize];
                coords.push((orig_lat[node as usize], orig_lng[node as usize]));
            }
            route_distance_m(&coords)
        }
    }
}