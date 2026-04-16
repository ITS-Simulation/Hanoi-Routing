use std::io::{Error, ErrorKind, Result};
use std::path::Path;

use rust_road_router::datastr::graph::{EdgeId, FirstOutGraph, NodeId, Weight};
use rust_road_router::util::Storage;

/// Graph data loaded from RoutingKit binary format (CSR representation).
///
/// Expects files in the directory: `first_out`, `head`, `travel_time`, `latitude`, `longitude`.
/// Shape-point files are optional: `first_modelling_node`, `modelling_node_latitude`,
/// `modelling_node_longitude`.
pub struct GraphData {
    pub first_out: Storage<EdgeId>,
    pub head: Storage<NodeId>,
    pub travel_time: Storage<Weight>,
    pub latitude: Storage<f32>,
    pub longitude: Storage<f32>,
    pub first_modelling_node: Option<Storage<u32>>,
    pub modelling_node_latitude: Option<Storage<f32>>,
    pub modelling_node_longitude: Option<Storage<f32>>,
}

fn graph_check(cond: bool, msg: impl Into<String>) -> Result<()> {
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
        let first_out = Storage::<EdgeId>::mmap(dir.join("first_out"))?;
        let head = Storage::<NodeId>::mmap(dir.join("head"))?;
        let travel_time = Storage::<Weight>::mmap(dir.join("travel_time"))?;
        let latitude = Storage::<f32>::mmap(dir.join("latitude"))?;
        let longitude = Storage::<f32>::mmap(dir.join("longitude"))?;
        let first_modelling_node_path = dir.join("first_modelling_node");
        let modelling_node_latitude_path = dir.join("modelling_node_latitude");
        let modelling_node_longitude_path = dir.join("modelling_node_longitude");
        let first_modelling_node = if first_modelling_node_path.exists() {
            Some(Storage::<u32>::mmap(first_modelling_node_path)?)
        } else {
            None
        };
        let modelling_node_latitude = if modelling_node_latitude_path.exists() {
            Some(Storage::<f32>::mmap(modelling_node_latitude_path)?)
        } else {
            None
        };
        let modelling_node_longitude = if modelling_node_longitude_path.exists() {
            Some(Storage::<f32>::mmap(modelling_node_longitude_path)?)
        } else {
            None
        };

        let num_nodes = first_out.len().checked_sub(1).ok_or_else(|| {
            Error::new(ErrorKind::InvalidData, "first_out is empty — no sentinel")
        })?;
        let num_edges = head.len();

        // Validate CSR invariants
        graph_check(
            first_out.first().copied() == Some(0),
            "first_out must start at 0",
        )?;
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
        for window in first_out.windows(2) {
            graph_check(
                window[0] <= window[1],
                "first_out must be monotonically non-decreasing",
            )?;
        }
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
        for &target in head.iter() {
            graph_check(
                (target as usize) < num_nodes,
                format!("head value {} >= num_nodes {}", target, num_nodes),
            )?;
        }
        match (
            &first_modelling_node,
            &modelling_node_latitude,
            &modelling_node_longitude,
        ) {
            (None, None, None) => {}
            (Some(first), Some(lat), Some(lng)) => {
                graph_check(
                    first.len() == num_edges + 1,
                    format!(
                        "first_modelling_node.len() ({}) != num_edges + 1 ({})",
                        first.len(),
                        num_edges + 1
                    ),
                )?;
                graph_check(
                    first.first().copied() == Some(0),
                    "first_modelling_node must start at 0",
                )?;
                let modelling_sentinel = first.last().ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidData,
                        "first_modelling_node is empty — no sentinel",
                    )
                })?;
                graph_check(
                    *modelling_sentinel as usize == lat.len(),
                    format!(
                        "first_modelling_node sentinel ({}) != modelling_node_latitude.len() ({})",
                        modelling_sentinel,
                        lat.len()
                    ),
                )?;
                graph_check(
                    lat.len() == lng.len(),
                    format!(
                        "modelling_node_latitude.len() ({}) != modelling_node_longitude.len() ({})",
                        lat.len(),
                        lng.len()
                    ),
                )?;
                for window in first.windows(2) {
                    graph_check(
                        window[0] <= window[1],
                        "first_modelling_node must be monotonically non-decreasing",
                    )?;
                }
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "shape-point files must be present either all together or not at all",
                ));
            }
        }

        tracing::debug!(num_nodes, num_edges, "graph data loaded");
        Ok(GraphData {
            first_out,
            head,
            travel_time,
            latitude,
            longitude,
            first_modelling_node,
            modelling_node_latitude,
            modelling_node_longitude,
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
