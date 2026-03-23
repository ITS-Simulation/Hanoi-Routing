use std::io::{Error, ErrorKind, Result};
use std::path::Path;

use rust_road_router::datastr::graph::{EdgeId, FirstOutGraph, NodeId, Weight};
use rust_road_router::io::Load;

/// Graph data loaded from RoutingKit binary format (CSR representation).
///
/// Expects files in the directory: `first_out`, `head`, `travel_time`, `latitude`, `longitude`.
pub struct GraphData {
    pub first_out: Vec<EdgeId>,
    pub head: Vec<NodeId>,
    pub travel_time: Vec<Weight>,
    pub latitude: Vec<f32>,
    pub longitude: Vec<f32>,
}

fn graph_check(cond: bool, msg: impl Into<String>) -> std::io::Result<()> {
    if cond {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::InvalidData, msg.into()))
    }
}

impl GraphData {
    /// Load graph data from a directory containing RoutingKit binary files.
    pub fn load(dir: &Path) -> Result<Self> {
        tracing::debug!(?dir, "loading graph data from disk");
        let first_out: Vec<EdgeId> = Vec::load_from(dir.join("first_out"))?;
        let head: Vec<NodeId> = Vec::load_from(dir.join("head"))?;
        let travel_time: Vec<Weight> = Vec::load_from(dir.join("travel_time"))?;
        let latitude: Vec<f32> = Vec::load_from(dir.join("latitude"))?;
        let longitude: Vec<f32> = Vec::load_from(dir.join("longitude"))?;

        let num_nodes = first_out.len().checked_sub(1).ok_or_else(|| {
            Error::new(ErrorKind::InvalidData, "first_out is empty — no sentinel")
        })?;
        let num_edges = head.len();

        // Validate CSR invariants
        let sentinel = first_out.last().ok_or_else(|| {
            Error::new(ErrorKind::InvalidData, "first_out is empty — no sentinel")
        })?;
        graph_check(
            *sentinel as usize == num_edges,
            format!(
                "first_out sentinel ({}) != num_edges ({})",
                sentinel, num_edges
            ),
        )?;
        graph_check(
            latitude.len() == num_nodes,
            format!(
                "latitude.len() ({}) != num_nodes ({})",
                latitude.len(),
                num_nodes
            ),
        )?;
        graph_check(
            longitude.len() == num_nodes,
            format!(
                "longitude.len() ({}) != num_nodes ({})",
                longitude.len(),
                num_nodes
            ),
        )?;
        graph_check(
            travel_time.len() == num_edges,
            format!(
                "travel_time.len() ({}) != num_edges ({})",
                travel_time.len(),
                num_edges
            ),
        )?;

        tracing::debug!(num_nodes, num_edges, "graph data loaded");
        Ok(GraphData {
            first_out,
            head,
            travel_time,
            latitude,
            longitude,
        })
    }

    /// Number of nodes in the graph.
    pub fn num_nodes(&self) -> usize {
        self.first_out.len() - 1
    }

    /// Number of edges in the graph.
    pub fn num_edges(&self) -> usize {
        self.head.len()
    }

    /// Create a borrowed CSR graph view (zero-copy) using baseline travel_time as weights.
    pub fn as_borrowed_graph(&self) -> FirstOutGraph<&[EdgeId], &[NodeId], &[Weight]> {
        FirstOutGraph::new(&self.first_out[..], &self.head[..], &self.travel_time[..])
    }

    /// Create a borrowed CSR graph view with custom weights.
    pub fn as_borrowed_graph_with_weights<'a>(
        &'a self,
        weights: &'a [Weight],
    ) -> FirstOutGraph<&'a [EdgeId], &'a [NodeId], &'a [Weight]> {
        FirstOutGraph::new(&self.first_out[..], &self.head[..], weights)
    }
}
