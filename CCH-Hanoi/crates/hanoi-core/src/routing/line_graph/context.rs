use std::path::Path;

use rust_road_router::algo::customizable_contraction_hierarchy::CustomizedBasic;
use rust_road_router::algo::customizable_contraction_hierarchy::{
    CCH, DirectedCCH, customize_directed,
};
use rust_road_router::datastr::graph::{EdgeId, FirstOutGraph, NodeId, Weight};
use rust_road_router::datastr::node_order::NodeOrder;
use rust_road_router::io::Load;
use rust_road_router::util::Storage;

use super::mapping::validate_edge_mappings;
use crate::graph::cache::CchCache;
use crate::graph::data::GraphData;

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
