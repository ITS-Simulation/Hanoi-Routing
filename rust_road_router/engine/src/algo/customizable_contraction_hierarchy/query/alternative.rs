use std::collections::{HashMap, HashSet};

use super::stepped_elimination_tree::EliminationTreeWalk;
use super::*;
use crate::datastr::timestamped_vector::TimestampedVector;

/// Default geographic stretch factor for filtering alternatives by geo distance.
/// Loại tuyến thay thế nếu quãng đường địa lý vượt quá (ngắn nhất × stretch).
pub const DEFAULT_STRETCH: f64 = 1.25;

/// Max ratio of shared cost between two routes before the candidate is rejected as too similar.
/// Nếu hai tuyến trùng nhau quá ngưỡng chi phí này thì tuyến mới bị loại vì quá giống.
const SHARING_THRESHOLD: f64 = 0.8; /// Original value: 0.80

/// Epsilon for bounded stretch test: detour(A→B) must be ≤ optimal(A→B) × (1 + eps).
/// Đoạn phân nhánh từ A đến B phải nhỏ hơn đường ngắn nhất A→B * (1 + eps).
/// HIGHLY INFLUENTIAL PARAMETER - THAM SỐ QUAN TRỌNG
const BOUNDED_STRETCH_EPS: f64 = 0.4; /// Original value: 0.25

/// T-test half-window as a fraction of shortest-path cost: defines the [v'-v''] subpath around the via node.
/// Bán kính cửa sổ T-test tính theo phần trăm chi phí đường ngắn nhất, dùng để cắt đoạn quanh nút trung gian v'.
/// HIGHLY INFLUENTIAL PARAMETER - THAM SỐ QUAN TRỌNG
const LOCAL_OPT_T_FRACTION: f64 = 0.4; /// Original value: 0.25

/// T-test tolerance: subpath cost must be ≤ optimal × (1 + epsilon). 0 = exact local optimality.
/// Cho phép đoạn con trong vùng T-test dài hơn đường tối ưu tối đa 1 + epsilon lần. Đặt 0 nghĩa là phải tối ưu tuyệt đối.
/// HIGHLY INFLUENTIAL PARAMETER - THAM SỐ QUAN TRỌNG
const LOCAL_OPT_EPSILON: f64 = 0.1; /// Original value: 0

/// Recursion stops when subproblem distance < this fraction of original distance.
/// Dừng đệ quy khi bài toán con quá ngắn so với quãng đường gốc.
const RECURSION_MIN_RATIO: f64 = 0.3; /// Original value: 0.30

/// Travel-time stretch cap: candidates exceeding shortest × this factor are skipped entirely.
/// Bỏ qua luôn những ứng viên có thời gian di chuyển vượt quá đường ngắn nhất nhân hệ số này.
const TRAVEL_TIME_STRETCH: f64 = 1.5; /// Original value: 1.5

#[derive(Debug, Clone)]
pub struct AlternativeRoute {
    pub distance: Weight,
    pub path: Vec<NodeId>,
    pub edge_costs: Vec<Weight>,
}

pub struct AlternativeServer<'a, C> {
    customized: &'a C,
    fw_distances: Vec<Weight>,
    bw_distances: Vec<Weight>,
    fw_parents: Vec<(NodeId, EdgeId)>,
    bw_parents: Vec<(NodeId, EdgeId)>,
    ttest_fw_dist: TimestampedVector<Weight>,
    ttest_bw_dist: TimestampedVector<Weight>,
    ttest_fw_par: Vec<(NodeId, EdgeId)>,
    ttest_bw_par: Vec<(NodeId, EdgeId)>,
}

impl<'a, C: Customized> AlternativeServer<'a, C> {
    pub fn new(customized: &'a C) -> Self {
        let n = customized.forward_graph().num_nodes();
        let m = customized.forward_graph().num_arcs();

        Self {
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

    pub fn reset(&mut self) {
        self.fw_distances.fill(INFINITY);
        self.bw_distances.fill(INFINITY);
    }

    pub fn alternatives<F>(&mut self, from: NodeId, to: NodeId, max_alternatives: usize, stretch_factor: f64, path_geo_len: F) -> Vec<AlternativeRoute>
    where
        F: Fn(&[NodeId]) -> f64,
    {
        if max_alternatives == 0 {
            return Vec::new();
        }

        self.reset();
        let from_rank = self.customized.cch().node_order().rank(from);
        let to_rank = self.customized.cch().node_order().rank(to);
        let meeting_candidates = self.collect_meeting_nodes(from_rank, to_rank);
        if meeting_candidates.is_empty() {
            return Vec::new();
        }
        let best_distance = meeting_candidates[0].1;

        self.multi_query_recursive_inner(
            from,
            to,
            max_alternatives,
            stretch_factor,
            SHARING_THRESHOLD,
            LOCAL_OPT_T_FRACTION,
            best_distance,
            meeting_candidates,
            &path_geo_len,
        )
    }

    fn multi_query_recursive<F>(
        &mut self,
        from: NodeId,
        to: NodeId,
        max_alternatives: usize,
        stretch_factor: f64,
        sharing_threshold: f64,
        local_opt_fraction: f64,
        original_distance: Weight,
        path_geo_len: &F,
    ) -> Vec<AlternativeRoute>
    where
        F: Fn(&[NodeId]) -> f64 + ?Sized,
    {
        self.reset();
        let from_rank = self.customized.cch().node_order().rank(from);
        let to_rank = self.customized.cch().node_order().rank(to);
        let meeting_candidates = self.collect_meeting_nodes(from_rank, to_rank);
        if meeting_candidates.is_empty() {
            return Vec::new();
        }

        self.multi_query_recursive_inner(
            from,
            to,
            max_alternatives,
            stretch_factor,
            sharing_threshold,
            local_opt_fraction,
            original_distance,
            meeting_candidates,
            path_geo_len,
        )
    }

    fn multi_query_recursive_inner<F>(
        &mut self,
        from: NodeId,
        to: NodeId,
        max_alternatives: usize,
        stretch_factor: f64,
        sharing_threshold: f64,
        local_opt_fraction: f64,
        original_distance: Weight,
        meeting_candidates: Vec<(NodeId, Weight)>,
        path_geo_len: &F,
    ) -> Vec<AlternativeRoute>
    where
        F: Fn(&[NodeId]) -> f64 + ?Sized,
    {
        let from_rank = self.customized.cch().node_order().rank(from);
        let to_rank = self.customized.cch().node_order().rank(to);
        let best_distance = meeting_candidates[0].1;

        if (best_distance as f64) < RECURSION_MIN_RATIO * original_distance as f64 {
            let (path, edge_costs) = self.reconstruct_path_with_costs(from_rank, to_rank, meeting_candidates[0].0);
            if path.is_empty() {
                return Vec::new();
            }
            return vec![AlternativeRoute {
                distance: best_distance,
                path,
                edge_costs,
            }];
        }

        let mut accepted = self.run_basic_selection(
            from_rank,
            to_rank,
            &meeting_candidates,
            max_alternatives,
            stretch_factor,
            sharing_threshold,
            local_opt_fraction,
            path_geo_len,
        );
        if accepted.is_empty() {
            return Vec::new();
        }
        if accepted.len() >= max_alternatives {
            return accepted;
        }

        let main_path = accepted[0].path.clone();
        let main_costs = accepted[0].edge_costs.clone();
        let order = self.customized.cch().node_order();
        let Some((v_pos, &separator)) = main_path.iter().enumerate().max_by_key(|(_, node)| order.rank(**node)) else {
            return accepted;
        };

        if v_pos == 0 || v_pos >= main_path.len() - 1 {
            return accepted;
        }

        let v_s = main_path[v_pos - 1];
        let v_t = main_path[v_pos + 1];

        let d_s_vs = main_costs[..v_pos - 1].iter().copied().fold(0u32, |acc, cost| acc.saturating_add(cost));
        let d_vt_t = main_costs[v_pos + 1..].iter().copied().fold(0u32, |acc, cost| acc.saturating_add(cost));
        let d_vs_vt = main_costs[v_pos - 1].saturating_add(main_costs[v_pos]);

        let gamma_left = if d_s_vs > 0 {
            (sharing_threshold * best_distance as f64 - d_vs_vt as f64) / d_s_vs as f64
        } else {
            return accepted;
        };
        let alpha_left = local_opt_fraction * best_distance as f64 / d_s_vs as f64;

        let gamma_right = if d_vt_t > 0 {
            (sharing_threshold * best_distance as f64 - d_vs_vt as f64) / d_vt_t as f64
        } else {
            return accepted;
        };
        let alpha_right = local_opt_fraction * best_distance as f64 / d_vt_t as f64;

        let left_alts = if alpha_left < 1.0 && gamma_left > 0.0 {
            self.multi_query_recursive(
                from,
                v_s,
                max_alternatives,
                stretch_factor,
                gamma_left,
                alpha_left,
                original_distance,
                path_geo_len,
            )
        } else {
            Vec::new()
        };

        let right_alts = if alpha_right < 1.0 && gamma_right > 0.0 {
            self.multi_query_recursive(
                v_t,
                to,
                max_alternatives,
                stretch_factor,
                gamma_right,
                alpha_right,
                original_distance,
                path_geo_len,
            )
        } else {
            Vec::new()
        };

        let main_geo = path_geo_len(&main_path);
        let geo_stretch_limit = main_geo * stretch_factor;
        let sharing_limit = (sharing_threshold * best_distance as f64) as Weight;
        let meeting_node_rank = order.rank(separator);
        let main_middle = &main_path[v_pos - 1..=v_pos + 1];
        let mut accepted_edge_sets: Vec<HashMap<(NodeId, NodeId), Weight>> =
            accepted.iter().map(|route| build_cost_edge_set(&route.path, &route.edge_costs)).collect();

        let mut combined_candidates = Vec::new();
        for left in &left_alts {
            for right in &right_alts {
                let mut path = left.path[..left.path.len() - 1].to_vec();
                path.extend_from_slice(main_middle);
                path.extend_from_slice(&right.path[1..]);

                let mut edge_costs = left.edge_costs.clone();
                edge_costs.extend_from_slice(&main_costs[v_pos - 1..=v_pos]);
                edge_costs.extend_from_slice(&right.edge_costs);

                let candidate = AlternativeRoute {
                    distance: left.distance.saturating_add(d_vs_vt).saturating_add(right.distance),
                    path,
                    edge_costs,
                };
                combined_candidates.push(candidate);
            }
        }

        combined_candidates.sort_unstable_by_key(|candidate| candidate.distance);
        for candidate in combined_candidates {
            if accepted.len() >= max_alternatives {
                return accepted;
            }

            if let Ok(candidate_edges) = self.evaluate_candidate(
                &candidate,
                &main_path,
                geo_stretch_limit,
                best_distance,
                sharing_limit,
                local_opt_fraction,
                &accepted_edge_sets,
                meeting_node_rank,
                path_geo_len,
            ) {
                accepted_edge_sets.push(candidate_edges);
                accepted.push(candidate);
            }
        }

        accepted
    }

    fn run_basic_selection<F>(
        &mut self,
        from_rank: NodeId,
        to_rank: NodeId,
        meeting_candidates: &[(NodeId, Weight)],
        max_alternatives: usize,
        stretch_factor: f64,
        sharing_threshold: f64,
        local_opt_fraction: f64,
        path_geo_len: &F,
    ) -> Vec<AlternativeRoute>
    where
        F: Fn(&[NodeId]) -> f64 + ?Sized,
    {
        if meeting_candidates.is_empty() {
            return Vec::new();
        }

        let best_distance = meeting_candidates[0].1;
        let (main_path, main_edge_costs) = self.reconstruct_path_with_costs(from_rank, to_rank, meeting_candidates[0].0);
        if main_path.is_empty() {
            return Vec::new();
        }

        let main_geo = path_geo_len(&main_path);
        let geo_stretch_limit = main_geo * stretch_factor;
        let sharing_limit = (sharing_threshold * best_distance as f64) as Weight;

        let main_route = AlternativeRoute {
            distance: best_distance,
            path: main_path.clone(),
            edge_costs: main_edge_costs,
        };

        let mut accepted = Vec::with_capacity(max_alternatives);
        let mut accepted_edge_sets = Vec::with_capacity(max_alternatives);
        accepted_edge_sets.push(build_cost_edge_set(&main_route.path, &main_route.edge_costs));
        accepted.push(main_route);

        let mut rejected_empty_path = 0u32;
        let mut rejected_loop = 0u32;
        let mut rejected_stretch = 0u32;
        let mut rejected_sharing = 0u32;
        let mut rejected_ttest = 0u32;

        for &(meeting_node, dist) in meeting_candidates.iter().skip(1) {
            if accepted.len() >= max_alternatives {
                break;
            }

            if dist > (best_distance as f64 * TRAVEL_TIME_STRETCH) as Weight {
                break;
            }

            let (path, edge_costs) = self.reconstruct_path_with_costs(from_rank, to_rank, meeting_node);
            if path.is_empty() {
                rejected_empty_path += 1;
                tracing::trace!(meeting_node, "alternative candidate rejected: empty path");
                continue;
            }

            let candidate = AlternativeRoute {
                distance: dist,
                path,
                edge_costs,
            };

            match self.evaluate_candidate(
                &candidate,
                &main_path,
                geo_stretch_limit,
                best_distance,
                sharing_limit,
                local_opt_fraction,
                &accepted_edge_sets,
                meeting_node,
                path_geo_len,
            ) {
                Ok(candidate_edges) => {
                    accepted_edge_sets.push(candidate_edges);
                    accepted.push(candidate);
                }
                Err(CandidateRejection::Loop) => {
                    rejected_loop += 1;
                    tracing::trace!(meeting_node, "alternative candidate rejected: repeated node");
                }
                Err(CandidateRejection::Stretch) => {
                    rejected_stretch += 1;
                    tracing::trace!(meeting_node, "alternative candidate rejected: stretch");
                }
                Err(CandidateRejection::Sharing) => {
                    rejected_sharing += 1;
                    tracing::trace!(meeting_node, "alternative candidate rejected: sharing");
                }
                Err(CandidateRejection::TTest) => {
                    rejected_ttest += 1;
                    tracing::trace!(meeting_node, "alternative candidate rejected: T-test");
                }
            }
        }

        tracing::debug!(
            total_candidates = meeting_candidates.len().saturating_sub(1),
            rejected_empty_path,
            rejected_loop,
            rejected_stretch,
            rejected_sharing,
            rejected_ttest,
            accepted = accepted.len(),
            "candidate filtering summary"
        );

        accepted
    }

    fn evaluate_candidate<F>(
        &mut self,
        candidate: &AlternativeRoute,
        reference_path: &[NodeId],
        geo_stretch_limit: f64,
        best_distance: Weight,
        sharing_limit: Weight,
        local_opt_fraction: f64,
        accepted_edge_sets: &[HashMap<(NodeId, NodeId), Weight>],
        meeting_node_rank: NodeId,
        path_geo_len: &F,
    ) -> Result<HashMap<(NodeId, NodeId), Weight>, CandidateRejection>
    where
        F: Fn(&[NodeId]) -> f64 + ?Sized,
    {
        if has_repeated_nodes(&candidate.path) {
            return Err(CandidateRejection::Loop);
        }

        let candidate_geo = path_geo_len(&candidate.path);
        if candidate_geo > geo_stretch_limit {
            return Err(CandidateRejection::Stretch);
        }

        if let Some(dev) = find_deviation_points(&candidate.path, &candidate.edge_costs, reference_path) {
            let total_cost = candidate.edge_costs.iter().copied().fold(0u32, |acc, cost| acc.saturating_add(cost));
            let detour_cost = total_cost.saturating_sub(dev.cost_s_a).saturating_sub(dev.cost_b_t);
            let order = self.customized.cch().node_order();
            let a_rank = order.rank(candidate.path[dev.a_pos]);
            let b_rank = order.rank(candidate.path[dev.b_pos]);
            let exact_ab = self.cch_point_distance(a_rank, b_rank);
            if exact_ab < INFINITY {
                let limit = (exact_ab as f64 * (1.0 + BOUNDED_STRETCH_EPS)) as Weight;
                if detour_cost > limit {
                    return Err(CandidateRejection::Stretch);
                }
            }
        }

        let candidate_edges = build_cost_edge_set(&candidate.path, &candidate.edge_costs);
        if accepted_edge_sets
            .iter()
            .any(|previous| shared_cost(&candidate_edges, previous) > sharing_limit)
        {
            return Err(CandidateRejection::Sharing);
        }

        if !self.check_local_optimality(&candidate.path, &candidate.edge_costs, meeting_node_rank, best_distance, local_opt_fraction) {
            return Err(CandidateRejection::TTest);
        }

        Ok(candidate_edges)
    }

    fn collect_meeting_nodes(&mut self, from_rank: NodeId, to_rank: NodeId) -> Vec<(NodeId, Weight)> {
        let fw_graph = self.customized.forward_graph();
        let bw_graph = self.customized.backward_graph();
        let mut meeting_candidates = Vec::new();

        let mut fw_walk = EliminationTreeWalk::query_with_resetted(
            &fw_graph,
            self.customized.cch().elimination_tree(),
            &mut self.fw_distances,
            &mut self.fw_parents,
            from_rank,
        );
        let mut bw_walk = EliminationTreeWalk::query_with_resetted(
            &bw_graph,
            self.customized.cch().elimination_tree(),
            &mut self.bw_distances,
            &mut self.bw_parents,
            to_rank,
        );

        loop {
            match (fw_walk.peek(), bw_walk.peek()) {
                (Some(fw_node), Some(bw_node)) if fw_node < bw_node => {
                    fw_walk.next();
                }
                (Some(fw_node), Some(bw_node)) if fw_node > bw_node => {
                    bw_walk.next();
                }
                (Some(node), Some(other_node)) => {
                    debug_assert_eq!(node, other_node);
                    fw_walk.next();
                    bw_walk.next();

                    let fw_dist = fw_walk.tentative_distance(node);
                    let bw_dist = bw_walk.tentative_distance(node);

                    if fw_dist < INFINITY && bw_dist < INFINITY {
                        let dist = fw_dist + bw_dist;
                        meeting_candidates.push((node, dist));
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

        meeting_candidates.sort_unstable_by_key(|&(_, dist)| dist);
        meeting_candidates.dedup_by_key(|&mut (node, _)| node);

        tracing::debug!(
            num_candidates = meeting_candidates.len(),
            best_distance = meeting_candidates.first().map(|candidate| candidate.1),
            "meeting nodes collected"
        );

        meeting_candidates
    }

    fn reconstruct_path_with_costs(&self, from_rank: NodeId, to_rank: NodeId, meeting_node: NodeId) -> (Vec<NodeId>, Vec<Weight>) {
        let max_steps = self.fw_parents.len();

        let mut fw_edges = Vec::new();
        let mut node = meeting_node;
        let mut steps = 0;
        while node != from_rank {
            let (parent, edge) = self.fw_parents[node as usize];
            if parent == node || steps >= max_steps {
                return (Vec::new(), Vec::new());
            }
            fw_edges.push((parent, node, edge));
            node = parent;
            steps += 1;
        }
        fw_edges.reverse();

        let mut bw_edges = Vec::new();
        node = meeting_node;
        steps = 0;
        while node != to_rank {
            let (successor, edge) = self.bw_parents[node as usize];
            if successor == node || steps >= max_steps {
                return (Vec::new(), Vec::new());
            }
            bw_edges.push((node, successor, edge));
            node = successor;
            steps += 1;
        }

        let mut path = vec![from_rank];
        let mut edge_costs = Vec::new();

        for &(tail, head, edge) in &fw_edges {
            Self::unpack_edge_with_costs(tail, head, edge, self.customized, &mut path, &mut edge_costs);
            path.push(head);
        }
        for &(tail, head, edge) in &bw_edges {
            Self::unpack_edge_with_costs(tail, head, edge, self.customized, &mut path, &mut edge_costs);
            path.push(head);
        }

        let order = self.customized.cch().node_order();
        for node in &mut path {
            *node = order.node(*node);
        }

        debug_assert_eq!(path.len(), edge_costs.len() + 1);

        (path, edge_costs)
    }

    fn unpack_edge_with_costs(tail: NodeId, head: NodeId, edge: EdgeId, customized: &C, path: &mut Vec<NodeId>, edge_costs: &mut Vec<Weight>) {
        let unpacked = if tail < head {
            customized.unpack_outgoing(EdgeIdT(edge))
        } else {
            customized.unpack_incoming(EdgeIdT(edge))
        };

        if let Some((EdgeIdT(down_edge), EdgeIdT(up_edge), NodeIdT(middle))) = unpacked {
            Self::unpack_edge_with_costs(tail, middle, down_edge, customized, path, edge_costs);
            path.push(middle);
            Self::unpack_edge_with_costs(middle, head, up_edge, customized, path, edge_costs);
        } else {
            let cost = if tail < head {
                customized.forward_graph().weight()[edge as usize]
            } else {
                customized.backward_graph().weight()[edge as usize]
            };
            edge_costs.push(cost);
        }
    }

    fn cch_point_distance(&mut self, from_rank: NodeId, to_rank: NodeId) -> Weight {
        if from_rank == to_rank {
            return 0;
        }

        let fw_graph = self.customized.forward_graph();
        let bw_graph = self.customized.backward_graph();
        let elimination_tree = self.customized.cch().elimination_tree();

        let mut fw_walk = EliminationTreeWalk::query(&fw_graph, elimination_tree, &mut self.ttest_fw_dist, &mut self.ttest_fw_par, from_rank);
        let mut bw_walk = EliminationTreeWalk::query(&bw_graph, elimination_tree, &mut self.ttest_bw_dist, &mut self.ttest_bw_par, to_rank);

        let mut best = INFINITY;
        loop {
            match (fw_walk.peek(), bw_walk.peek()) {
                (Some(fw_node), Some(bw_node)) if fw_node < bw_node => {
                    fw_walk.next();
                }
                (Some(fw_node), Some(bw_node)) if fw_node > bw_node => {
                    bw_walk.next();
                }
                (Some(node), Some(other_node)) => {
                    debug_assert_eq!(node, other_node);
                    fw_walk.next();
                    bw_walk.next();

                    let fw_dist = fw_walk.tentative_distance(node);
                    let bw_dist = bw_walk.tentative_distance(node);
                    if fw_dist < INFINITY && bw_dist < INFINITY {
                        let distance = fw_dist + bw_dist;
                        if distance < best {
                            best = distance;
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

    fn check_local_optimality(
        &mut self,
        path: &[NodeId],
        edge_costs: &[Weight],
        meeting_node_rank: NodeId,
        best_distance: Weight,
        local_opt_fraction: f64,
    ) -> bool {
        if path.len() < 3 {
            return true;
        }

        let order = self.customized.cch().node_order();
        let via_node = order.node(meeting_node_rank);
        let Some(via_pos) = path.iter().position(|&node| node == via_node) else {
            return true;
        };

        let mut cumulative_costs: Vec<Weight> = Vec::with_capacity(path.len());
        cumulative_costs.push(0);
        for &cost in edge_costs {
            let previous: Weight = *cumulative_costs.last().unwrap();
            cumulative_costs.push(previous.saturating_add(cost));
        }

        let t_half = (local_opt_fraction * best_distance as f64) as Weight;
        let via_cost = cumulative_costs[via_pos];
        let target_start = via_cost.saturating_sub(t_half);
        let target_end = via_cost.saturating_add(t_half);

        let v_prime_pos = cumulative_costs[..=via_pos].iter().rposition(|&cost| cost <= target_start).unwrap_or(0);
        let v_double_pos = cumulative_costs[via_pos..]
            .iter()
            .position(|&cost| cost >= target_end)
            .map(|offset| via_pos + offset)
            .unwrap_or(path.len() - 1);

        if v_prime_pos == 0 && v_double_pos == path.len() - 1 {
            return true;
        }

        let subpath_cost = cumulative_costs[v_double_pos].saturating_sub(cumulative_costs[v_prime_pos]);
        if subpath_cost == 0 {
            return true;
        }

        let exact_distance = self.cch_point_distance(order.rank(path[v_prime_pos]), order.rank(path[v_double_pos]));
        if exact_distance >= INFINITY {
            return false;
        }

        let threshold = (exact_distance as f64 * (1.0 + LOCAL_OPT_EPSILON)) as Weight;
        subpath_cost <= threshold
    }
}

struct DeviationPoints {
    a_pos: usize,
    b_pos: usize,
    cost_s_a: Weight,
    cost_b_t: Weight,
}

enum CandidateRejection {
    Loop,
    Stretch,
    Sharing,
    TTest,
}

fn find_deviation_points(candidate: &[NodeId], candidate_costs: &[Weight], reference: &[NodeId]) -> Option<DeviationPoints> {
    if candidate.len() < 2 || reference.len() < 2 {
        return None;
    }

    debug_assert_eq!(candidate.len(), candidate_costs.len() + 1);

    let mut a_pos = 0;
    let mut cost_s_a: Weight = 0;
    let min_len = candidate.len().min(reference.len());
    for i in 0..min_len - 1 {
        if candidate[i + 1] != reference[i + 1] {
            a_pos = i;
            break;
        }

        cost_s_a = cost_s_a.saturating_add(candidate_costs[i]);
        if i == min_len - 2 {
            return None;
        }
    }

    let mut cost_b_t: Weight = 0;
    let mut candidate_index = candidate.len() - 1;
    let mut reference_index = reference.len() - 1;

    while candidate_index > a_pos && reference_index > 0 && candidate[candidate_index] == reference[reference_index] {
        if candidate_index < candidate.len() - 1 {
            cost_b_t = cost_b_t.saturating_add(candidate_costs[candidate_index]);
        }
        candidate_index -= 1;
        reference_index -= 1;
    }

    Some(DeviationPoints {
        a_pos,
        b_pos: candidate_index + 1,
        cost_s_a,
        cost_b_t,
    })
}

fn build_cost_edge_set(path: &[NodeId], edge_costs: &[Weight]) -> HashMap<(NodeId, NodeId), Weight> {
    debug_assert_eq!(path.len(), edge_costs.len() + 1);
    path.windows(2)
        .zip(edge_costs.iter().copied())
        .map(|(window, cost)| ((window[0], window[1]), cost))
        .collect()
}

fn shared_cost(candidate: &HashMap<(NodeId, NodeId), Weight>, reference: &HashMap<(NodeId, NodeId), Weight>) -> Weight {
    candidate
        .iter()
        .filter(|(edge, _)| reference.contains_key(edge))
        .map(|(_, &cost)| cost)
        .fold(0u32, |acc, cost| acc.saturating_add(cost))
}

fn has_repeated_nodes(path: &[NodeId]) -> bool {
    let mut seen = HashSet::with_capacity(path.len());
    for &node in path {
        if !seen.insert(node) {
            return true;
        }
    }
    false
}
