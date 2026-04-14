//! Via-way turn restriction support: types, I/O, and node-splitting algorithm.
//!
//! Via-way restrictions forbid (or mandate) specific multi-edge paths through the
//! line graph. They cannot be encoded as simple forbidden-turn pairs without
//! over-restricting legal paths. Instead, we use **node splitting** to create
//! "tainted" copies of intermediate nodes that track whether a vehicle has entered
//! a forbidden chain, and only restrict the specific forbidden exit.

use std::io::{Error as IoError, ErrorKind};
use std::path::Path;

use rust_road_router::datastr::graph::{EdgeId, NodeId, OwnedGraph, Weight};
use rust_road_router::io::Load;

/// A via-way restriction chain expressed as a sequence of line-graph node IDs
/// (= original-graph arc IDs).
///
/// For a prohibitive restriction `from_way -> [via_ways] -> to_way`, the arcs
/// field contains `[from_arc, v1, v2, ..., vN, to_arc]` — the full path through
/// the line graph that must be forbidden.
///
/// For a mandatory restriction, the same chain describes the *only* allowed path;
/// all other exits from intermediate nodes are forbidden.
#[derive(Debug, Clone)]
pub struct ViaWayChain {
    /// Ordered sequence of line-graph node IDs (original arc IDs).
    /// Length >= 3 (from_arc, at least one via_arc, to_arc).
    pub arcs: Vec<u32>,
    /// `false` = prohibitive (this specific path is forbidden).
    /// `true`  = mandatory (this is the only allowed path through intermediates).
    pub mandatory: bool,
}

/// Result of applying node splits to a line graph.
pub struct SplitResult {
    /// Expanded line graph with split nodes appended.
    pub graph: OwnedGraph,
    /// For each split node `i`, `split_map[i]` is the original LG node ID
    /// that was cloned. The split node's ID in the expanded graph is
    /// `base_node_count + i`.
    pub split_map: Vec<u32>,
}

/// Load via-way restriction chains from the graph directory.
///
/// Reads three mandatory files:
/// - `via_way_chain_offsets` (u32 CSR offsets)
/// - `via_way_chain_arcs` (u32 packed arc sequences)
/// - `via_way_chain_mandatory` (u8: 0 = prohibitive, 1 = mandatory)
///
/// Returns an error if any file is missing (this is intentional — the files are
/// a mandatory output of `conditional_turn_extract`).
pub fn load_via_way_chains(graph_dir: &Path) -> Result<Vec<ViaWayChain>, IoError> {
    let offsets_path = graph_dir.join("via_way_chain_offsets");
    let arcs_path = graph_dir.join("via_way_chain_arcs");
    let mandatory_path = graph_dir.join("via_way_chain_mandatory");

    let offsets: Vec<u32> = Vec::load_from(&offsets_path).map_err(|e| {
        IoError::new(
            ErrorKind::NotFound,
            format!(
                "Missing required file '{}': {}. Run conditional_turn_extract first.",
                offsets_path.display(),
                e
            ),
        )
    })?;

    let arcs: Vec<u32> = Vec::load_from(&arcs_path).map_err(|e| {
        IoError::new(
            ErrorKind::NotFound,
            format!(
                "Missing required file '{}': {}. Run conditional_turn_extract first.",
                arcs_path.display(),
                e
            ),
        )
    })?;

    let mandatory_bytes: Vec<u8> = Vec::load_from(&mandatory_path).map_err(|e| {
        IoError::new(
            ErrorKind::NotFound,
            format!(
                "Missing required file '{}': {}. Run conditional_turn_extract first.",
                mandatory_path.display(),
                e
            ),
        )
    })?;

    // Validate structure
    if offsets.is_empty() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            "via_way_chain_offsets is empty (must have at least one entry)",
        ));
    }

    let num_chains = offsets.len() - 1;
    if mandatory_bytes.len() != num_chains {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            format!(
                "via_way_chain_mandatory length ({}) does not match chain count ({})",
                mandatory_bytes.len(),
                num_chains
            ),
        ));
    }

    if let Some(&last_offset) = offsets.last() {
        if last_offset as usize != arcs.len() {
            return Err(IoError::new(
                ErrorKind::InvalidData,
                format!(
                    "via_way_chain_offsets sentinel ({}) does not match arcs length ({})",
                    last_offset,
                    arcs.len()
                ),
            ));
        }
    }

    let mut chains = Vec::with_capacity(num_chains);
    for i in 0..num_chains {
        let start = offsets[i] as usize;
        let end = offsets[i + 1] as usize;
        chains.push(ViaWayChain {
            arcs: arcs[start..end].to_vec(),
            mandatory: mandatory_bytes[i] != 0,
        });
    }

    Ok(chains)
}

/// Outgoing edge in the adjacency list representation.
#[derive(Clone)]
struct AdjEdge {
    target: u32,
    weight: Weight,
}

/// Apply node splitting to a line graph to enforce via-way restrictions.
///
/// For each restriction chain, creates tainted copies of intermediate nodes and
/// redirects/removes edges to structurally forbid (or mandate) the specific path.
///
/// The input `graph` is the base line graph produced by `line_graph()`. The output
/// is an expanded graph with split nodes appended, plus a `split_map` that records
/// which original node each split node was cloned from.
///
/// If `chains` is empty, returns the input graph unchanged with an empty split_map.
pub fn apply_node_splits(graph: OwnedGraph, chains: &[ViaWayChain]) -> SplitResult {
    if chains.is_empty() {
        return SplitResult {
            graph,
            split_map: Vec::new(),
        };
    }

    let (first_out, head, weight) = graph.decompose();
    let base_n = first_out.len() - 1; // original node count

    // Convert CSR to adjacency lists
    let mut adj: Vec<Vec<AdjEdge>> = Vec::with_capacity(base_n);
    for node in 0..base_n {
        let start = first_out[node] as usize;
        let end = first_out[node + 1] as usize;
        let mut edges = Vec::with_capacity(end - start);
        for idx in start..end {
            edges.push(AdjEdge {
                target: head[idx],
                weight: weight[idx],
            });
        }
        adj.push(edges);
    }

    // split_map[i] = original node that split node (base_n + i) was cloned from
    let mut split_map: Vec<u32> = Vec::new();

    for chain in chains {
        if chain.arcs.len() < 3 {
            // A valid via-way chain needs at least from_arc, one via_arc, and to_arc.
            continue;
        }

        let from_node = chain.arcs[0];
        let to_node = *chain.arcs.last().unwrap();
        // Intermediate nodes: chain.arcs[1..chain.arcs.len()-1]
        let intermediates = &chain.arcs[1..chain.arcs.len() - 1];

        // Verify chain connectivity in the current LG adjacency list
        let chain_connected = {
            let mut ok = true;
            for pair in chain.arcs.windows(2) {
                let src = pair[0] as usize;
                let dst = pair[1];
                if src >= adj.len() || !adj[src].iter().any(|e| e.target == dst) {
                    ok = false;
                    break;
                }
            }
            ok
        };
        if !chain_connected {
            // Chain already broken by a direct forbidden turn or prior split — skip.
            continue;
        }

        // Create tainted copies for each intermediate node
        let mut tainted_ids: Vec<u32> = Vec::with_capacity(intermediates.len());
        for &intermediate in intermediates {
            let new_id = adj.len() as u32;
            tainted_ids.push(new_id);
            split_map.push(intermediate);

            // Clone the original node's outgoing edges
            let original_edges = adj[intermediate as usize].clone();
            adj.push(original_edges);
        }

        if chain.mandatory {
            // Mandatory: tainted copies keep ONLY the chain-continuation edge.
            for (idx, &tainted_id) in tainted_ids.iter().enumerate() {
                let next_in_chain = if idx + 1 < tainted_ids.len() {
                    // Point to next tainted copy
                    tainted_ids[idx + 1]
                } else {
                    // Last intermediate: point to to_node (the actual target)
                    to_node
                };

                // Replace all outgoing edges with just the chain edge
                let original_target = intermediates[idx + 1..].first().copied().unwrap_or(to_node);
                let chain_weight = adj[tainted_id as usize]
                    .iter()
                    .find(|e| e.target == original_target)
                    .map(|e| e.weight);

                if let Some(w) = chain_weight {
                    adj[tainted_id as usize] = vec![AdjEdge {
                        target: next_in_chain,
                        weight: w,
                    }];
                } else {
                    // Edge to chain continuation not found — dead end
                    adj[tainted_id as usize] = Vec::new();
                }
            }
        } else {
            // Prohibitive: tainted copies keep all outgoing edges EXCEPT the
            // chain-continuation edge. Additionally, redirect chain-continuation
            // edges from non-last tainted copies to the next tainted copy.
            for (idx, &tainted_id) in tainted_ids.iter().enumerate() {
                let original_next = if idx + 1 < intermediates.len() {
                    intermediates[idx + 1]
                } else {
                    to_node
                };

                if idx + 1 < tainted_ids.len() {
                    // Non-last intermediate: redirect chain edge to next tainted copy
                    let next_tainted = tainted_ids[idx + 1];
                    for edge in &mut adj[tainted_id as usize] {
                        if edge.target == original_next {
                            edge.target = next_tainted;
                        }
                    }
                } else {
                    // Last intermediate: remove the edge to to_node
                    adj[tainted_id as usize].retain(|e| e.target != original_next);
                }
            }
        }

        // Redirect entry: the edge from from_node to intermediates[0]
        // must now point to tainted_ids[0] instead.
        let original_first_intermediate = intermediates[0];
        let first_tainted = tainted_ids[0];
        for edge in &mut adj[from_node as usize] {
            if edge.target == original_first_intermediate {
                edge.target = first_tainted;
            }
        }
    }

    // Convert adjacency lists back to CSR
    let expanded_n = adj.len();
    let total_edges: usize = adj.iter().map(|edges| edges.len()).sum();

    let mut new_first_out: Vec<EdgeId> = Vec::with_capacity(expanded_n + 1);
    let mut new_head: Vec<NodeId> = Vec::with_capacity(total_edges);
    let mut new_weight: Vec<Weight> = Vec::with_capacity(total_edges);

    new_first_out.push(0);
    for node_edges in &adj {
        for edge in node_edges {
            new_head.push(edge.target);
            new_weight.push(edge.weight);
        }
        new_first_out.push(new_head.len() as EdgeId);
    }

    let expanded_graph = OwnedGraph::new(new_first_out, new_head, new_weight);

    SplitResult {
        graph: expanded_graph,
        split_map,
    }
}
