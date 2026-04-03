//! Multi-route query via CCH separator nodes — **Basic Approach (Dual-Metric)**.
//!
//! Reimplements the bidirectional elimination tree walk from
//! `rust_road_router::algo::customizable_contraction_hierarchy::query` and
//! collects *all* common-ancestor nodes (Set A) from the highest-level
//! separator as candidates.  Each candidate is then checked against two
//! admissibility conditions:
//!
//! 1. **Bounded Stretch** — the candidate's *geographic path length*
//!    (meters, computed via a caller-supplied callback) must not exceed
//!    `stretch_factor × geo_len(main_path)`.
//! 2. **Limited Sharing** — the fraction of the candidate's edges that
//!    overlap with the *main* (shortest-time) path must stay below
//!    `SHARING_THRESHOLD`.
//!
//! The CCH is customized with `travel_time` so the shortest-time path is
//! found first. A broad travel-time prefilter (`DEFAULT_STRETCH`) provides
//! early termination before the more expensive geographic check.
//!
//! This module does NOT modify `rust_road_router` — it uses only public API:
//! `EliminationTreeWalk`, `Customized` trait, and `CCHT` trait.

use std::collections::HashSet;

use rust_road_router::algo::customizable_contraction_hierarchy::query::stepped_elimination_tree::EliminationTreeWalk;
use rust_road_router::algo::customizable_contraction_hierarchy::{CCHT, Customized};
use rust_road_router::datastr::graph::{EdgeId, EdgeIdT, Graph, NodeId, NodeIdT, Weight, INFINITY};

/// Default stretch factor: candidates up to 400% longer than optimal.
pub const DEFAULT_STRETCH: f64 = 1.3;

/// Maximum sharing ratio with the main (shortest) path.  A candidate whose
/// edge-overlap with the shortest path exceeds this fraction is rejected.
const SHARING_THRESHOLD: f64 = 0.80;

/// Maximum geographic distance ratio: alternatives whose geographic distance
/// exceeds this multiplier of the shortest path's geographic distance are
/// rejected as detours.  Applied by callers that have coordinate data.
pub const MAX_GEO_RATIO: f64 = 2.0;

/// Over-request multiplier: callers should request this many times
/// `max_alternatives` from `multi_query` so that geographic filtering still
/// leaves enough candidates.
pub const GEO_OVER_REQUEST: usize = 3;

/// A single alternative route result.
#[derive(Debug, Clone)]
pub struct AlternativeRoute {
    /// Travel time in the weight unit of the graph (milliseconds).
    pub distance: Weight,
    /// Geographic path length in meters, computed via the caller-supplied
    /// `path_geo_len` callback.
    pub geo_distance_m: f64,
    /// Ordered node IDs along the path (original graph IDs, not ranks).
    pub path: Vec<NodeId>,
}

/// Multi-route query server. Borrows customized CCH data and provides
/// `multi_query()` to find K alternative routes between two nodes.
pub struct MultiRouteServer<'a, C> {
    customized: &'a C,
    fw_distances: Vec<Weight>,
    bw_distances: Vec<Weight>,
    fw_parents: Vec<(NodeId, EdgeId)>,
    bw_parents: Vec<(NodeId, EdgeId)>,
}

impl<'a, C: Customized> MultiRouteServer<'a, C> {
    pub fn new(customized: &'a C) -> Self {
        let n = customized.forward_graph().num_nodes();
        let m = customized.forward_graph().num_arcs();
        MultiRouteServer {
            customized,
            fw_distances: vec![INFINITY; n],
            bw_distances: vec![INFINITY; n],
            fw_parents: vec![(n as NodeId, m as EdgeId); n],
            bw_parents: vec![(n as NodeId, m as EdgeId); n],
        }
    }

    /// Find up to `max_alternatives` alternative routes between `from` and `to`
    /// using the **Basic Approach** with **dual-metric** stretch.
    ///
    /// The CCH is customized with `travel_time` to find the shortest-time path.
    /// *Bounded Stretch* is evaluated on **geographic distance** via the
    /// caller-supplied `path_geo_len` callback, while *Limited Sharing* remains
    /// edge-based.
    ///
    /// `path_geo_len` receives a path (original node IDs) and returns its
    /// geographic length in meters.
    pub fn multi_query(
        &mut self,
        from: NodeId,
        to: NodeId,
        max_alternatives: usize,
        stretch_factor: f64,
        path_geo_len: impl Fn(&[NodeId]) -> f64,
    ) -> Vec<AlternativeRoute> {
        if max_alternatives == 0 {
            return Vec::new();
        }

        let from_rank = self.customized.cch().node_order().rank(from);
        let to_rank = self.customized.cch().node_order().rank(to);

        // --- Phase 1: Collect Set A (common-ancestor separator nodes) ---
        let meeting_candidates = self.collect_meeting_nodes(from_rank, to_rank);

        if meeting_candidates.is_empty() {
            return Vec::new();
        }

        let best_distance = meeting_candidates[0].1;

        // --- Phase 2: Reconstruct the main (shortest-time) path ---
        let main_path = self.reconstruct_path(from_rank, to_rank, meeting_candidates[0].0);
        if main_path.is_empty() {
            return Vec::new();
        }
        let main_geo = path_geo_len(&main_path);
        let geo_stretch_limit = main_geo * stretch_factor;
        let main_edge_set: HashSet<(NodeId, NodeId)> = main_path.windows(2).map(|w| (w[0], w[1])).collect();

        let mut accepted: Vec<AlternativeRoute> = Vec::with_capacity(max_alternatives);
        accepted.push(AlternativeRoute {
            distance: best_distance,
            geo_distance_m: main_geo,
            path: main_path,
        });

        // --- Phase 3: Admissibility check for remaining candidates ---
        for &(meeting_node, dist) in meeting_candidates.iter().skip(1) {
            if accepted.len() >= max_alternatives {
                break;
            }
            let path = self.reconstruct_path(from_rank, to_rank, meeting_node);
            if path.is_empty() {
                continue;
            }

            // Bounded Stretch (geographic distance)
            let candidate_geo = path_geo_len(&path);
            if candidate_geo > geo_stretch_limit {
                continue;
            }

            // Limited Sharing: reject if too many edges overlap with the main path
            let edge_set: HashSet<(NodeId, NodeId)> = path.windows(2).map(|w| (w[0], w[1])).collect();
            if sharing_ratio(&edge_set, &main_edge_set) > SHARING_THRESHOLD {
                continue;
            }

            accepted.push(AlternativeRoute {
                distance: dist,
                geo_distance_m: candidate_geo,
                path,
            });
        }

        accepted
    }

    /// Run the bidirectional elimination tree walk and collect all meeting nodes.
    ///
    /// Returns `Vec<(meeting_node_rank, total_distance)>` sorted by distance
    /// ascending. The fw_parents and bw_parents arrays are populated as a side
    /// effect (matching the walk in `query.rs::distance()`).
    fn collect_meeting_nodes(&mut self, from: NodeId, to: NodeId) -> Vec<(NodeId, Weight)> {
        let _n = self.customized.forward_graph().num_nodes();

        let fw_graph = self.customized.forward_graph();
        let bw_graph = self.customized.backward_graph();

        let mut tentative_distance = INFINITY;
        let mut meeting_candidates: Vec<(NodeId, Weight)> = Vec::new();

        let mut fw_walk = EliminationTreeWalk::query_with_resetted(
            &fw_graph,
            self.customized.cch().elimination_tree(),
            &mut self.fw_distances,
            &mut self.fw_parents,
            from,
        );
        let mut bw_walk = EliminationTreeWalk::query_with_resetted(
            &bw_graph,
            self.customized.cch().elimination_tree(),
            &mut self.bw_distances,
            &mut self.bw_parents,
            to,
        );

        loop {
            match (fw_walk.peek(), bw_walk.peek()) {
                (Some(fw_node), Some(bw_node)) if fw_node < bw_node => {
                    fw_walk.next();
                    // Do NOT reset: we need parent pointers for path reconstruction.
                    // Instead, we'll do a full cleanup pass after the walk.
                }
                (Some(fw_node), Some(bw_node)) if fw_node > bw_node => {
                    bw_walk.next();
                }
                (Some(node), Some(_node)) => {
                    debug_assert_eq!(node, _node);

                    // Always relax edges at meeting nodes — we need correct
                    // distances for ALL candidates, not just the single best.
                    // (The original query.rs prunes here because it only needs
                    // one optimal meeting node; skip_next() would cause
                    // ancestor nodes to never receive propagated distances,
                    // losing entire subtrees of valid candidates.)
                    fw_walk.next();
                    bw_walk.next();

                    let fw_dist = fw_walk.tentative_distance(node);
                    let bw_dist = bw_walk.tentative_distance(node);

                    if fw_dist < INFINITY && bw_dist < INFINITY {
                        let dist = fw_dist + bw_dist;
                        meeting_candidates.push((node, dist));
                        if dist < tentative_distance {
                            tentative_distance = dist;
                        }
                    }
                }
                (Some(_fw_node), None) => {
                    fw_walk.next();
                }
                (None, Some(_bw_node)) => {
                    bw_walk.next();
                }
                (None, None) => break,
            }
        }

        // Sort candidates by total distance (ascending)
        meeting_candidates.sort_unstable_by_key(|&(_, dist)| dist);

        // Deduplicate by node (keep lowest distance, which is first after sort)
        meeting_candidates.dedup_by_key(|&mut (node, _)| node);

        meeting_candidates
    }

    /// Reconstruct and unpack the path going through a given meeting node.
    ///
    /// Traces the forward and backward halves independently using read-only
    /// access to `fw_parents` and `bw_parents`, avoiding the parent pointer
    /// collision that occurs when reversing fw pointers into a shared array.
    /// Each shortcut edge is recursively unpacked in-place.
    fn reconstruct_path(
        &self,
        from: NodeId,
        to: NodeId,
        meeting_node: NodeId,
    ) -> Vec<NodeId> {
        let max_steps = self.fw_parents.len();

        // --- Forward half: trace fw_parents from meeting_node back to `from` ---
        // Produces edges in reverse order (meeting→from), so collect then reverse.
        let mut fw_edges: Vec<(NodeId, NodeId, EdgeId)> = Vec::new();
        let mut node = meeting_node;
        let mut steps = 0;
        while node != from {
            let (parent, edge) = self.fw_parents[node as usize];
            if parent == node || steps >= max_steps {
                return Vec::new();
            }
            // Edge goes parent → node in path order
            fw_edges.push((parent, node, edge));
            node = parent;
            steps += 1;
        }
        fw_edges.reverse();

        // --- Backward half: trace bw_parents from meeting_node to `to` ---
        // bw_parents[node] = (pred, edge) where pred is closer to `to`.
        // In the backward CCH graph, parent pointers go meeting→to in the
        // upward direction, so we trace: meeting_node → ... → to.
        let mut bw_edges: Vec<(NodeId, NodeId, EdgeId)> = Vec::new();
        node = meeting_node;
        steps = 0;
        while node != to {
            let (succ, edge) = self.bw_parents[node as usize];
            if succ == node || steps >= max_steps {
                return Vec::new();
            }
            // Edge goes node → succ in path order
            bw_edges.push((node, succ, edge));
            node = succ;
            steps += 1;
        }

        // --- Build the unpacked path ---
        let mut path = vec![from];

        for &(tail, head, edge) in &fw_edges {
            Self::unpack_edge_recursive(tail, head, edge, self.customized, &mut path);
            path.push(head);
        }

        for &(tail, head, edge) in &bw_edges {
            Self::unpack_edge_recursive(tail, head, edge, self.customized, &mut path);
            path.push(head);
        }

        // Convert ranks to original node IDs
        let order = self.customized.cch().node_order();
        for node in &mut path {
            *node = order.node(*node);
        }

        path
    }

    /// Recursively unpack a single CCH shortcut edge `tail → head` via `edge`.
    ///
    /// Pushes only intermediate (middle) nodes to `path`; the caller is
    /// responsible for pushing `head`. This mirrors the contraction structure:
    /// a shortcut `(tail, head)` was created by contracting a `middle` node,
    /// producing two sub-edges that are themselves either shortcuts or
    /// base-graph edges.
    ///
    /// Recursion depth is bounded by the CCH contraction depth (~20–25 for
    /// road networks).
    fn unpack_edge_recursive(
        tail: NodeId,
        head: NodeId,
        edge: EdgeId,
        customized: &C,
        path: &mut Vec<NodeId>,
    ) {
        let unpacked = if tail < head {
            customized.unpack_outgoing(EdgeIdT(edge))
        } else {
            customized.unpack_incoming(EdgeIdT(edge))
        };
        if let Some((EdgeIdT(down_edge), EdgeIdT(up_edge), NodeIdT(middle))) = unpacked {
            // tail → middle (down edge) then middle → head (up edge)
            Self::unpack_edge_recursive(tail, middle, down_edge, customized, path);
            path.push(middle);
            Self::unpack_edge_recursive(middle, head, up_edge, customized, path);
        }
        // else: base-graph edge, nothing to unpack
    }
}

/// Sharing ratio: fraction of `candidate`'s edges that also appear in `main`.
///
/// Returns `|candidate ∩ main| / |candidate|`, a value in [0.0, 1.0].
/// A value of 1.0 means every edge in the candidate is shared with the main path.
fn sharing_ratio(candidate: &HashSet<(NodeId, NodeId)>, main: &HashSet<(NodeId, NodeId)>) -> f64 {
    if candidate.is_empty() {
        return 1.0;
    }
    let shared = candidate.iter().filter(|e| main.contains(e)).count();
    shared as f64 / candidate.len() as f64
}
