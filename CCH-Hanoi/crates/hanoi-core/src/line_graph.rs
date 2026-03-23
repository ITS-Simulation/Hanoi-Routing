use std::path::Path;

use rust_road_router::algo::customizable_contraction_hierarchy::CustomizedBasic;
use rust_road_router::algo::customizable_contraction_hierarchy::query::Server as CchQueryServer;
use rust_road_router::algo::customizable_contraction_hierarchy::{
    CCH, DirectedCCH, customize_directed,
};
use rust_road_router::algo::{Query, QueryServer};
use rust_road_router::datastr::graph::{EdgeId, FirstOutGraph, INFINITY, NodeId, Weight};
use rust_road_router::datastr::node_order::NodeOrder;
use rust_road_router::io::Load;

use crate::bounds::{BoundingBox, CoordRejection, ValidationConfig};
use crate::cch::{QueryAnswer, route_distance_m};
use crate::graph::GraphData;
use crate::spatial::SpatialIndex;

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

        tracing::info!(
            num_original_edges,
            split_nodes = split_map.len(),
            "reconstruction arrays built"
        );

        let num_nodes = graph.num_nodes();
        let num_edges = graph.num_edges();
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

            let distance_m = route_distance_m(&coordinates);
            Some(QueryAnswer {
                distance_ms,
                distance_m,
                path,
                coordinates,
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
    ///
    /// Falls back to both endpoints of each snapped edge, then to outgoing
    /// edges from each endpoint (expanded candidate set).
    pub fn query_coords(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
    ) -> Result<Option<QueryAnswer>, CoordRejection> {
        // Snap in original-graph intersection-node space
        let src_snap =
            self.original_spatial
                .validated_snap("origin", from.0, from.1, &self.validation_config)?;
        let dst_snap =
            self.original_spatial
                .validated_snap("destination", to.0, to.1, &self.validation_config)?;

        // The snapped edge_id IS the line-graph node ID (line-graph node N = original edge N).
        let src_edge = src_snap.edge_id;
        let dst_edge = dst_snap.edge_id;

        tracing::debug!(
            src_edge,
            dst_edge,
            src_snap_dist_m = src_snap.snap_distance_m,
            dst_snap_dist_m = dst_snap.snap_distance_m,
            "unified snap: original edge IDs → line-graph node IDs"
        );

        // Primary: route directly between snapped original edges
        if let Some(answer) = self.query(src_edge, dst_edge) {
            return Ok(Some(Self::patch_coordinates(answer, from, to)));
        }

        // Fallback: try all candidate original edges from both snap results.
        // Candidates include both endpoints of the snapped edge plus outgoing
        // edges from both endpoints (to handle one-way streets, etc.).
        let src_candidates = self.collect_original_edge_candidates(&src_snap);
        let dst_candidates = self.collect_original_edge_candidates(&dst_snap);

        let mut best: Option<QueryAnswer> = None;

        for &s in &src_candidates {
            for &d in &dst_candidates {
                if s == src_edge && d == dst_edge {
                    continue; // Already tried
                }
                if let Some(answer) = self.query(s, d) {
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

    /// Prepend the user's origin coordinate and append the destination coordinate
    /// to the query result's coordinate path, so map visualizations show the full
    /// journey from the user's actual position.
    fn patch_coordinates(mut answer: QueryAnswer, from: (f32, f32), to: (f32, f32)) -> QueryAnswer {
        answer.coordinates.insert(0, from);
        answer.coordinates.push(to);
        answer.distance_m = route_distance_m(&answer.coordinates);
        answer
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

    /// Collect candidate original **edge IDs** (= line-graph node IDs) from a
    /// snap result in original-graph space.
    ///
    /// The snap gives us one original edge directly (`edge_id`). We also include
    /// all other outgoing edges from both the tail and head intersection nodes
    /// of the snapped edge, which covers bidirectional roads and nearby turns.
    fn collect_original_edge_candidates(
        &self,
        snap: &crate::spatial::SnapResult,
    ) -> Vec<EdgeId> {
        let primary = snap.edge_id;
        let mut edges = vec![primary];

        // Add all outgoing original edges from the tail node
        for (edge, _, _) in self.original_spatial.edges_incident_to(snap.tail) {
            if edge != primary && !edges.contains(&edge) {
                edges.push(edge);
            }
        }

        // Add all outgoing original edges from the head node
        for (edge, _, _) in self.original_spatial.edges_incident_to(snap.head) {
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }

        edges
    }

    /// Access the original-graph spatial index (used for unified snapping).
    pub fn spatial(&self) -> &SpatialIndex {
        &self.original_spatial
    }

    /// Access the underlying line graph CCH context.
    pub fn context(&self) -> &'a LineGraphCchContext {
        self.context
    }
}
