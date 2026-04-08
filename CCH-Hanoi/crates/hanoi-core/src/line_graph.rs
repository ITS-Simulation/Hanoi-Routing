use std::path::Path;

use rust_road_router::algo::customizable_contraction_hierarchy::query::Server as CchQueryServer;
use rust_road_router::algo::customizable_contraction_hierarchy::CustomizedBasic;
use rust_road_router::algo::customizable_contraction_hierarchy::{
    customize_directed, DirectedCCH, CCH,
};
use rust_road_router::algo::{Query, QueryServer};
use rust_road_router::datastr::graph::{EdgeId, FirstOutGraph, NodeId, Weight, INFINITY};
use rust_road_router::datastr::node_order::NodeOrder;
use rust_road_router::io::Load;

use crate::bounds::{BoundingBox, CoordRejection, ValidationConfig};
use crate::cch::{route_distance_m, QueryAnswer};
use crate::geometry::{compute_turns, refine_turns};
use crate::graph::GraphData;
use crate::multi_route::{AlternativeServer, GEO_OVER_REQUEST, MAX_GEO_RATIO};
use crate::spatial::{SpatialIndex, SNAP_MAX_CANDIDATES};

/// CCH context for line graphs. Uses `DirectedCCH` (pruned — no always-INFINITY
/// edges) for efficient turn-expanded graph routing.
pub struct LineGraphCchContext {
    /// Line graph data (CSR).
    pub graph: GraphData,

    /// Pruned directed CCH topology.
    pub directed_cch: DirectedCCH,

    /// Baseline line-graph weights.
    pub baseline_weights: Vec<Weight>,

    /// Original graph's CSR offset array (for building the original-space spatial index).
    pub original_first_out: Vec<EdgeId>,

    /// Original graph's tail array: `original_tail[edge_i]` = source node of edge i.
    /// Reconstructed from the original graph's `first_out` at load time.
    pub original_tail: Vec<NodeId>,

    /// Original graph's head array: `original_head[edge_i]` = target node of edge i.
    pub original_head: Vec<NodeId>,

    /// Original graph's node latitudes (for path coordinate output).
    pub original_latitude: Vec<f32>,

    /// Original graph's node longitudes (for path coordinate output).
    pub original_longitude: Vec<f32>,

    /// Original graph's travel_time (for source-edge correction at query time).
    pub original_travel_time: Vec<Weight>,

    /// Maps each line-graph node back to the original directed arc it
    /// represents. Split nodes clone an original arc and therefore point back
    /// to that arc ID.
    pub original_arc_id_of_lg_node: Vec<u32>,

    /// Per-LG-node flag: true if the original arc belongs to a roundabout way.
    pub is_arc_roundabout: Vec<u8>,
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
        let original_first_out: Vec<EdgeId> = Vec::load_from(original_graph_dir.join("first_out"))?;
        let mut original_head: Vec<NodeId> = Vec::load_from(original_graph_dir.join("head"))?;
        let original_latitude: Vec<f32> = Vec::load_from(original_graph_dir.join("latitude"))?;
        let original_longitude: Vec<f32> = Vec::load_from(original_graph_dir.join("longitude"))?;
        let mut original_travel_time: Vec<Weight> =
            Vec::load_from(original_graph_dir.join("travel_time"))?;
        let mut original_arc_id_of_lg_node: Vec<u32> = (0..original_head.len())
            .map(|arc_id| arc_id as u32)
            .collect();

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

        // Load via-way split map — mandatory. Extends reconstruction arrays for
        // split nodes so that path unpacking maps them back to original arcs.
        let split_map: Vec<u32> =
            Vec::load_from(line_graph_dir.join("via_way_split_map")).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "Missing required file 'via_way_split_map' in {}: {}. Re-run generate_line_graph.",
                        line_graph_dir.display(),
                        e
                    ),
                )
            })?;

        // Extend reconstruction arrays for split nodes.
        // Split node i (graph node num_original_edges + i) was cloned from
        // original LG node split_map[i], so it maps to the same original arc.
        for &original in &split_map {
            let tail_val = original_tail[original as usize];
            let head_val = original_head[original as usize];
            let tt_val = original_travel_time[original as usize];
            original_tail.push(tail_val);
            original_head.push(head_val);
            original_travel_time.push(tt_val);
            original_arc_id_of_lg_node.push(original);
        }

        // Consistency check: the reconstruction arrays must cover all LG nodes
        // (base nodes = original edges, plus split nodes).
        let num_lg_nodes = graph.num_nodes();
        if original_tail.len() != num_lg_nodes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Reconstruction array length ({}) does not match line graph node count ({}). \
                     Expected {} original edges + {} split nodes.",
                    original_tail.len(),
                    num_lg_nodes,
                    num_original_edges,
                    split_map.len()
                ),
            ));
        }
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
        let is_arc_roundabout: Vec<u8> = Vec::load_from(line_graph_dir.join("is_arc_roundabout"))
            .unwrap_or_else(|_| vec![0u8; num_lg_nodes]);

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

        tracing::info!(num_nodes, num_edges, "building DirectedCCH for line graph");

        // Build undirected CCH first, then convert to directed (prune always-INFINITY edges)
        let borrowed = graph.as_borrowed_graph();
        let cch = CCH::fix_order_and_build(&borrowed, order);
        let directed_cch = cch.to_directed_cch();

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
            original_travel_time,
            original_arc_id_of_lg_node,
            is_arc_roundabout,
        })
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
        let original_spatial = SpatialIndex::build(
            &context.original_latitude,
            &context.original_longitude,
            &context.original_first_out,
            &context.original_head,
        );

        LineGraphQueryEngine {
            server,
            context,
            original_spatial,
            validation_config,
        }
    }

    /// Query by line-graph node IDs (= original edge indices), with snap-edge
    /// trimming. Runs the same CCH query as `query()` but removes the first
    /// and last elements from the LG path before building turns, coordinates,
    /// and distance. This eliminates phantom first/last turns caused by the
    /// full extent of snapped source and destination edges.
    ///
    /// Used exclusively by `query_coords()` for coordinate-based queries.
    fn query_trimmed(&mut self, source_edge: EdgeId, target_edge: EdgeId) -> Option<QueryAnswer> {
        let result = self.server.query(Query {
            from: source_edge as NodeId,
            to: target_edge as NodeId,
        });

        if let Some(mut connected) = result.found() {
            let cch_distance = connected.distance();
            if cch_distance >= INFINITY {
                return None;
            }

            // Source-edge correction (identical to query())
            let source_edge_cost = self.context.original_travel_time[source_edge as usize];
            let distance_ms = cch_distance.saturating_add(source_edge_cost);

            let lg_path = connected.node_path();

            // Trim: drop source and destination edges from the LG path
            let trimmed: &[NodeId] = if lg_path.len() > 2 {
                &lg_path[1..lg_path.len() - 1]
            } else {
                &[]
            };
            let route_arc_ids: Vec<u32> = trimmed
                .iter()
                .map(|&lg_node| self.context.original_arc_id_of_lg_node[lg_node as usize])
                .collect();
            let weight_path_ids: Vec<u32> = lg_path.iter().map(|&lg_node| lg_node as u32).collect();

            // Map trimmed LG path to original intersection node IDs
            let mut path: Vec<NodeId> = trimmed
                .iter()
                .map(|&lg_node| self.context.original_tail[lg_node as usize])
                .collect();

            if let Some(&last_edge) = trimmed.last() {
                // Normal case: append head of the last trimmed edge
                path.push(self.context.original_head[last_edge as usize]);
            } else if lg_path.len() >= 2 {
                // Trimmed to empty (adjacent edges or same-edge): use the shared
                // intersection. By the LG invariant: head(src) == tail(dst).
                path.push(self.context.original_head[lg_path[0] as usize]);
            }

            let coordinates: Vec<(f32, f32)> = path
                .iter()
                .map(|&node| {
                    (
                        self.context.original_latitude[node as usize],
                        self.context.original_longitude[node as usize],
                    )
                })
                .collect();

            let mut turns = compute_turns(
                trimmed,
                &self.context.original_tail,
                &self.context.original_head,
                &self.context.original_first_out,
                &self.context.original_latitude,
                &self.context.original_longitude,
                &self.context.is_arc_roundabout,
            );
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
            })
        } else {
            None
        }
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
            let source_edge_cost = self.context.original_travel_time[source_edge as usize];
            let distance_ms = cch_distance.saturating_add(source_edge_cost);

            let lg_path = connected.node_path();
            let route_arc_ids: Vec<u32> = lg_path
                .iter()
                .map(|&lg_node| self.context.original_arc_id_of_lg_node[lg_node as usize])
                .collect();
            let weight_path_ids: Vec<u32> = lg_path.iter().map(|&lg_node| lg_node as u32).collect();
            // Map line-graph path to original intersection node IDs.
            // Each line-graph node is an original edge index; we map it to the
            // tail node of that original edge, then append the head of the final edge.
            let mut path: Vec<NodeId> = lg_path
                .iter()
                .map(|&lg_node| self.context.original_tail[lg_node as usize])
                .collect();

            // Append the destination intersection (head of the final original edge)
            if let Some(&last_edge) = lg_path.last() {
                let dest_node = self.context.original_head[last_edge as usize];
                path.push(dest_node);
            }

            // Coordinate mapping: same intersection sequence → (lat, lng)
            let coordinates: Vec<(f32, f32)> = path
                .iter()
                .map(|&node| {
                    (
                        self.context.original_latitude[node as usize],
                        self.context.original_longitude[node as usize],
                    )
                })
                .collect();

            let mut turns = compute_turns(
                &lg_path,
                &self.context.original_tail,
                &self.context.original_head,
                &self.context.original_first_out,
                &self.context.original_latitude,
                &self.context.original_longitude,
                &self.context.is_arc_roundabout,
            );
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
            })
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

        let mut best: Option<QueryAnswer> = None;

        for src in &src_snaps {
            for dst in &dst_snaps {
                tracing::debug!(
                    src_edge = src.edge_id,
                    dst_edge = dst.edge_id,
                    src_snap_dist_m = src.snap_distance_m,
                    dst_snap_dist_m = dst.snap_distance_m,
                    "unified snap candidate pair: original edge IDs -> line-graph node IDs"
                );

                if let Some(answer) = self.query_trimmed(src.edge_id, dst.edge_id) {
                    let is_better = best
                        .as_ref()
                        .map_or(true, |b| answer.distance_ms < b.distance_ms);
                    if is_better {
                        best = Some(answer);
                    }
                }
            }
        }

        Ok(best.map(|answer| Self::patch_coordinates(answer, from, to)))
    }

    /// Attach the user's origin and destination coordinates to the query answer
    /// as metadata — the coordinate path itself stays pure (graph nodes only).
    fn patch_coordinates(mut answer: QueryAnswer, from: (f32, f32), to: (f32, f32)) -> QueryAnswer {
        answer.origin = Some(from);
        answer.destination = Some(to);
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
            .map(|&lg_node| self.context.original_tail[lg_node as usize])
            .collect();

        if trimmed {
            if let Some(&last_edge) = effective_path.last() {
                path.push(self.context.original_head[last_edge as usize]);
            } else if lg_path.len() >= 2 {
                path.push(self.context.original_head[lg_path[0] as usize]);
            }
        } else if let Some(&last_edge) = lg_path.last() {
            path.push(self.context.original_head[last_edge as usize]);
        }

        let coordinates: Vec<(f32, f32)> = path
            .iter()
            .map(|&node| {
                (
                    self.context.original_latitude[node as usize],
                    self.context.original_longitude[node as usize],
                )
            })
            .collect();

        let mut turns = compute_turns(
            effective_path,
            &self.context.original_tail,
            &self.context.original_head,
            &self.context.original_first_out,
            &self.context.original_latitude,
            &self.context.original_longitude,
            &self.context.is_arc_roundabout,
        );
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

        let source_edge_cost = self.context.original_travel_time[source_edge as usize];

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
        &self,
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

        for src in &src_snaps {
            for dst in &dst_snaps {
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

                if candidates.is_empty() {
                    continue;
                }

                let source_edge_cost = self.context.original_travel_time[src.edge_id as usize];

                let mut answers: Vec<QueryAnswer> = Vec::with_capacity(max_alternatives);
                let mut shortest_geo_dist: Option<f64> = None;

                for alt in candidates {
                    if answers.len() >= max_alternatives {
                        break;
                    }
                    if let Some(answer) = self.build_answer_from_lg_path(
                        alt.distance,
                        &alt.path,
                        source_edge_cost,
                        true,
                    ) {
                        let answer = Self::patch_coordinates(answer, from, to);
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

                if !answers.is_empty() {
                    tracing::info!(
                        requested = max_alternatives,
                        returned = answers.len(),
                        shortest_ms = answers.first().map(|route| route.distance_ms),
                        "line_graph multi_query_coords completed"
                    );
                    return Ok(answers);
                }
            }
        }

        tracing::info!(
            requested = max_alternatives,
            returned = 0usize,
            "line_graph multi_query_coords completed"
        );
        Ok(Vec::new())
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
        let orig_tail = &self.context.original_tail;
        let orig_head = &self.context.original_head;
        let orig_lat = &self.context.original_latitude;
        let orig_lng = &self.context.original_longitude;

        move |lg_path: &[NodeId]| -> f64 {
            let mut coords: Vec<(f32, f32)> = lg_path
                .iter()
                .map(|&lg_node| {
                    let node = orig_tail[lg_node as usize];
                    (orig_lat[node as usize], orig_lng[node as usize])
                })
                .collect();
            if let Some(&last) = lg_path.last() {
                let node = orig_head[last as usize];
                coords.push((orig_lat[node as usize], orig_lng[node as usize]));
            }
            route_distance_m(&coords)
        }
    }
}
