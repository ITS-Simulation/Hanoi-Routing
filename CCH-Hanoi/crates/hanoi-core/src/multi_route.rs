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
use rust_road_router::datastr::timestamped_vector::TimestampedVector;

/// Default stretch factor for the geographic distance prefilter.  Candidates whose geographic distance exceeds `stretch_factor × geo_len(main_path)` are rejected as detours.  This
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

/// Fraction of the shortest-path distance (in cost units) used as the
/// T-test interval *half-width*. The subpath extends
/// `T_FRACTION × d(s,t)` in each direction from the via-vertex.
///
/// Smaller values produce a narrower interval that is less likely to span
/// the entire path, so the bypass condition fires less often and the T-test
/// actually runs — effectively filtering out U-turn detours.
const LOCAL_OPT_T_FRACTION: f64 = 0.25;

/// Tolerance factor for the local optimality check (T-test).
/// A subpath passes if `subpath_cost ≤ (1 + ε) × d(v', v'')`.
///
/// A tighter epsilon rejects subpaths with even a modest detour, which
/// catches small U-turn loops whose cost is only slightly above optimal.
const LOCAL_OPT_EPSILON: f64 = 0.1;

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
    /// Scratch space for T-test point-to-point distance queries (forward).
    ttest_fw_dist: TimestampedVector<Weight>,
    /// Scratch space for T-test point-to-point distance queries (backward).
    ttest_bw_dist: TimestampedVector<Weight>,
    ttest_fw_par: Vec<(NodeId, EdgeId)>,
    ttest_bw_par: Vec<(NodeId, EdgeId)>,
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
            ttest_fw_dist: TimestampedVector::new(n),
            ttest_bw_dist: TimestampedVector::new(n),
            ttest_fw_par: vec![(n as NodeId, m as EdgeId); n],
            ttest_bw_par: vec![(n as NodeId, m as EdgeId); n],
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
        edge_cost: impl Fn(NodeId, NodeId) -> Weight,
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
        let mut accepted_edge_sets: Vec<HashSet<(NodeId, NodeId)>> = Vec::with_capacity(max_alternatives);
        accepted.push(AlternativeRoute {
            distance: best_distance,
            geo_distance_m: main_geo,
            path: main_path,
        });
        accepted_edge_sets.push(main_edge_set);

        // --- Phase 3: Admissibility check for remaining candidates ---
        for &(meeting_node, dist) in meeting_candidates.iter().skip(1) {
            if accepted.len() >= max_alternatives {
                break;
            }
            let path = self.reconstruct_path(from_rank, to_rank, meeting_node);
            if path.is_empty() {
                continue;
            }

            // Loop/backtrack detection: reject paths that visit any node
            // more than once — a clear sign of a U-turn loop.
            if has_repeated_nodes(&path) {
                continue;
            }

            // Bounded Stretch (geographic distance)
            let candidate_geo = path_geo_len(&path);
            if candidate_geo > geo_stretch_limit {
                continue;
            }

            // Pairwise Limited Sharing: reject if too many edges overlap
            // with ANY already-accepted route (not just the main path).
            let edge_set: HashSet<(NodeId, NodeId)> = path.windows(2).map(|w| (w[0], w[1])).collect();
            let too_similar = accepted_edge_sets.iter().any(|prev| {
                sharing_ratio(&edge_set, prev) > SHARING_THRESHOLD
            });
            if too_similar {
                continue;
            }

            // Local Optimality (T-test): reject if the subpath around the
            // via-vertex is not approximately a shortest path.
            if !self.check_local_optimality(&path, meeting_node, best_distance, &edge_cost) {
                continue;
            }

            accepted_edge_sets.push(edge_set);
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

    /// Run a fresh CCH point-to-point distance query between two rank-space
    /// nodes. Uses dedicated scratch arrays (`ttest_*`) that are independent
    /// of the main walk arrays, so T-test queries do not disturb ongoing path
    /// reconstruction from `fw_parents`/`bw_parents`.
    ///
    /// `TimestampedVector` provides O(1) amortised reset between successive
    /// queries, avoiding the cost of re-allocating per query.
    fn cch_point_distance(&mut self, from_rank: NodeId, to_rank: NodeId) -> Weight {
        if from_rank == to_rank {
            return 0;
        }

        let fw_graph = self.customized.forward_graph();
        let bw_graph = self.customized.backward_graph();
        let elim_tree = self.customized.cch().elimination_tree();

        let mut fw_walk = EliminationTreeWalk::query(
            &fw_graph,
            elim_tree,
            &mut self.ttest_fw_dist,
            &mut self.ttest_fw_par,
            from_rank,
        );
        let mut bw_walk = EliminationTreeWalk::query(
            &bw_graph,
            elim_tree,
            &mut self.ttest_bw_dist,
            &mut self.ttest_bw_par,
            to_rank,
        );

        let mut best = INFINITY;

        loop {
            match (fw_walk.peek(), bw_walk.peek()) {
                (Some(fw_node), Some(bw_node)) if fw_node < bw_node => {
                    fw_walk.next();
                }
                (Some(fw_node), Some(bw_node)) if fw_node > bw_node => {
                    bw_walk.next();
                }
                (Some(node), Some(_node)) => {
                    debug_assert_eq!(node, _node);
                    fw_walk.next();
                    bw_walk.next();

                    let fw_d = fw_walk.tentative_distance(node);
                    let bw_d = bw_walk.tentative_distance(node);
                    if fw_d < INFINITY && bw_d < INFINITY {
                        let d = fw_d + bw_d;
                        if d < best {
                            best = d;
                        }
                    }
                }
                (Some(_), None) => {
                    fw_walk.next();
                }
                (None, Some(_)) => {
                    bw_walk.next();
                }
                (None, None) => break,
            }
        }

        best
    }

    /// Local Optimality check (T-test) for a candidate alternative path.
    ///
    /// Verifies that the subpath around the via-vertex is approximately a
    /// shortest path:
    ///
    /// 1. Find the via-vertex's position in the unpacked path.
    /// 2. Walk `T = LOCAL_OPT_T_FRACTION × best_distance` in each direction
    ///    along the path to locate the interval endpoints v' and v''.
    /// 3. Sum edge costs along this subpath.
    /// 4. Run a CCH distance query for `d(v', v'')`.
    /// 5. Accept iff `subpath_cost ≤ (1 + ε) × d(v', v'')`.
    ///
    /// Returns `true` if the candidate passes (locally optimal within
    /// tolerance).
    fn check_local_optimality(
        &mut self,
        path: &[NodeId],
        meeting_node_rank: NodeId,
        best_distance: Weight,
        edge_cost: &impl Fn(NodeId, NodeId) -> Weight,
    ) -> bool {
        if path.len() < 3 {
            return true;
        }

        let order = self.customized.cch().node_order();
        let via_node = order.node(meeting_node_rank);

        // Find the via-vertex's position in the unpacked path.
        let via_pos = match path.iter().position(|&n| n == via_node) {
            Some(pos) => pos,
            None => return true, // via-vertex may have been elided during unpacking
        };

        // Build cumulative edge-cost prefix sums along the path.
        let mut cum_costs: Vec<Weight> = Vec::with_capacity(path.len());
        cum_costs.push(0);
        for w in path.windows(2) {
            let cost = edge_cost(w[0], w[1]);
            let prev = *cum_costs.last().unwrap();
            cum_costs.push(prev.saturating_add(cost));
        }

        // T-interval half-width (cost units, milliseconds).
        let t_half = (LOCAL_OPT_T_FRACTION * best_distance as f64) as Weight;
        let via_cost = cum_costs[via_pos];

        // v' — walk backward from the via-vertex by t_half.
        let target_start = via_cost.saturating_sub(t_half);
        let v_prime_pos = cum_costs[..=via_pos]
            .iter()
            .rposition(|&c| c <= target_start)
            .unwrap_or(0);

        // v'' — walk forward from the via-vertex by t_half.
        let target_end = via_cost.saturating_add(t_half);
        let v_double_pos = cum_costs[via_pos..]
            .iter()
            .position(|&c| c >= target_end)
            .map(|off| via_pos + off)
            .unwrap_or(path.len() - 1);

        // If the T-interval spans the entire path the stretch filter already
        // handles the global case — no local test needed.
        if v_prime_pos == 0 && v_double_pos == path.len() - 1 {
            return true;
        }

        let v_prime = path[v_prime_pos];
        let v_double = path[v_double_pos];
        let subpath_cost = cum_costs[v_double_pos] - cum_costs[v_prime_pos];

        if subpath_cost == 0 {
            return true;
        }

        // Exact shortest distance via CCH point query.
        let v_prime_rank = order.rank(v_prime);
        let v_double_rank = order.rank(v_double);
        let exact_dist = self.cch_point_distance(v_prime_rank, v_double_rank);

        if exact_dist >= INFINITY {
            return false;
        }

        // Admissibility criterion.
        let threshold = (exact_dist as f64 * (1.0 + LOCAL_OPT_EPSILON)) as Weight;
        subpath_cost <= threshold
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

/// Returns `true` if any node appears more than once in the path,
/// indicating a U-turn loop or backtrack.
fn has_repeated_nodes(path: &[NodeId]) -> bool {
    let mut seen = HashSet::with_capacity(path.len());
    for &node in path {
        if !seen.insert(node) {
            return true;
        }
    }
    false
}
