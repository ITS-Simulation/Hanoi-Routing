use rust_road_router::algo::customizable_contraction_hierarchy::{CCHT, DirectedCCH};
use rust_road_router::datastr::graph::{EdgeIdT, NodeId, Weight};

use super::context::LineGraphCchContext;

pub(crate) fn validate_edge_mappings(
    cch: &DirectedCCH,
    num_metric_edges: usize,
) -> std::io::Result<()> {
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

impl LineGraphCchContext {
    pub(crate) fn original_arc_id(&self, lg_node: NodeId) -> usize {
        self.original_arc_id_of_lg_node[lg_node as usize] as usize
    }

    pub(crate) fn lg_tail_node(&self, lg_node: NodeId) -> NodeId {
        self.original_tail[self.original_arc_id(lg_node)]
    }

    pub(crate) fn lg_head_node(&self, lg_node: NodeId) -> NodeId {
        self.original_head[self.original_arc_id(lg_node)]
    }

    pub(crate) fn lg_travel_time(&self, lg_node: NodeId) -> Weight {
        self.original_travel_time[self.original_arc_id(lg_node)]
    }
}
