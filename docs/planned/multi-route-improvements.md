# Multi-Route Improvements Plan

All work in `hanoi-core/src/multi_route.rs` and `cch.rs`/`line_graph.rs`.
No `rust_road_router` changes.

---

## Hyperparameters

### Current vs paper-standard values


| Parameter              | Current | Paper | Const name                  | Role                     |
| ---------------------- | ------- | ----- | --------------------------- | ------------------------ |
| γ (sharing)            | 0.80    | 0.80  | `SHARING_THRESHOLD`         | Max shared cost ratio    |
| ε (bounded stretch)    | —       | 0.25  | `BOUNDED_STRETCH_EPS` (new) | Max subpath stretch      |
| α (local optimality)   | 0.25    | 0.25  | `LOCAL_OPT_T_FRACTION`      | T-test interval width    |
| ε_T (T-test tolerance) | 0.10    | 0.00  | `LOCAL_OPT_EPSILON`         | T-test cost slack        |
| μ (recursion stop)     | —       | 0.30  | `RECURSION_MIN_RATIO` (new) | Min subproblem size      |
| stretch_factor         | 1.30    | 1.25  | `DEFAULT_STRETCH`           | Overall stretch limit    |
| geo ratio              | 2.00    | —     | `MAX_GEO_RATIO`             | Geo distance post-filter |


### What needs to change

```rust
// --- Keep as-is ---
const SHARING_THRESHOLD: f64 = 0.80;     // γ — matches paper
const LOCAL_OPT_T_FRACTION: f64 = 0.25;  // α — matches paper
pub const MAX_GEO_RATIO: f64 = 2.0;      // our addition, not in paper
pub const GEO_OVER_REQUEST: usize = 3;   // our addition, not in paper

// --- Change ---
pub const DEFAULT_STRETCH: f64 = 1.25;   // was 1.30, paper uses ε=0.25 → 1+ε=1.25
const LOCAL_OPT_EPSILON: f64 = 0.0;      // was 0.10, paper uses exact equality

// --- Add ---
/// Bounded stretch tolerance for deviation subpaths.
/// c(P_{a,v,b}) ≤ (1 + BOUNDED_STRETCH_EPS) × d(a,b)
const BOUNDED_STRETCH_EPS: f64 = 0.25;

/// Recursion stops when subproblem distance < μ × d(s,t).
const RECURSION_MIN_RATIO: f64 = 0.30;
```

Note on `LOCAL_OPT_EPSILON`: the paper's T-test uses exact equality
(`d(a',b') = c(subpath)`). Our 0.10 tolerance was added to compensate for
the missing bounded stretch check. Once Q1 is implemented, tighten to 0.0.

---

## Q1. Bounded Stretch via deviation points

Find where candidate diverges from shortest path, check subpath cost.

**File:** `multi_route.rs`

```rust
/// Deviation points where candidate path diverges from / rejoins the reference path.
struct DeviationPoints {
    /// Index of divergence point `a` in the candidate path.
    a_pos: usize,
    /// Index of rejoin point `b` in the candidate path.
    b_pos: usize,
    /// Cost of the shared prefix s→a.
    cost_s_a: Weight,
    /// Cost of the shared suffix b→t.
    cost_b_t: Weight,
}

/// Find deviation points by walking candidate and reference paths in parallel.
/// Both paths share the same source and target. Returns None if paths are identical.
fn find_deviation_points(
    candidate: &[NodeId],
    reference: &[NodeId],
    edge_cost: &impl Fn(NodeId, NodeId) -> Weight,
) -> Option<DeviationPoints> {
    // Forward: find first divergence point `a`
    let mut a_pos = 0;
    let mut cost_s_a: Weight = 0;
    let min_len = candidate.len().min(reference.len());
    for i in 0..min_len - 1 {
        if candidate[i + 1] != reference[i + 1] {
            a_pos = i;
            break;
        }
        cost_s_a = cost_s_a.saturating_add(edge_cost(candidate[i], candidate[i + 1]));
        if i == min_len - 2 {
            return None; // identical paths
        }
    }

    // Backward: find rejoin point `b`
    let mut b_pos = candidate.len() - 1;
    let mut cost_b_t: Weight = 0;
    let mut ci = candidate.len() - 1;
    let mut ri = reference.len() - 1;
    while ci > a_pos && ri > 0 {
        if candidate[ci] != reference[ri] {
            b_pos = ci;
            break;
        }
        if ci < candidate.len() - 1 {
            cost_b_t = cost_b_t.saturating_add(edge_cost(candidate[ci], candidate[ci + 1]));
        }
        ci -= 1;
        ri -= 1;
    }

    Some(DeviationPoints { a_pos, b_pos, cost_s_a, cost_b_t })
}
```

Usage in `multi_query` Phase 3, after loop detection, before sharing:

```rust
// Bounded Stretch check
if let Some(dev) = find_deviation_points(&path, &main_path, &edge_cost) {
    // Cost of detour segment a→v→b
    let detour_cost: Weight = (dev.a_pos..dev.b_pos)
        .map(|i| edge_cost(path[i], path[i + 1]))
        .fold(0u32, |acc, c| acc.saturating_add(c));

    // Exact shortest distance d(a, b)
    let a_node = path[dev.a_pos];
    let b_node = path[dev.b_pos];
    let a_rank = self.customized.cch().node_order().rank(a_node);
    let b_rank = self.customized.cch().node_order().rank(b_node);
    let exact_ab = self.cch_point_distance(a_rank, b_rank);

    if exact_ab < INFINITY {
        let limit = (exact_ab as f64 * (1.0 + BOUNDED_STRETCH_EPS)) as Weight;
        if detour_cost > limit {
            continue;
        }
    }
}
```

---

## Q2. Cost-based sharing

**File:** `multi_route.rs`

Replace edge-count sharing with cost-weighted sharing.

```rust
use std::collections::HashMap;

/// Cost-weighted edge set for a path.
fn build_cost_edge_set(
    path: &[NodeId],
    edge_cost: &impl Fn(NodeId, NodeId) -> Weight,
) -> HashMap<(NodeId, NodeId), Weight> {
    path.windows(2)
        .map(|w| ((w[0], w[1]), edge_cost(w[0], w[1])))
        .collect()
}

/// Shared cost: sum of costs of edges in `candidate` that also appear in `reference`.
fn shared_cost(
    candidate: &HashMap<(NodeId, NodeId), Weight>,
    reference: &HashMap<(NodeId, NodeId), Weight>,
) -> Weight {
    candidate
        .iter()
        .filter(|(edge, _)| reference.contains_key(edge))
        .map(|(_, &cost)| cost)
        .fold(0u32, |acc, c| acc.saturating_add(c))
}
```

Usage in Phase 3 (replaces `sharing_ratio` call):

```rust
let candidate_edges = build_cost_edge_set(&path, &edge_cost);

// Pairwise: shared cost with any accepted route must stay below γ × d(s,t)
let sharing_limit = (SHARING_THRESHOLD * best_distance as f64) as Weight;
let too_similar = accepted_edge_sets.iter().any(|prev| {
    shared_cost(&candidate_edges, prev) > sharing_limit
});
if too_similar {
    continue;
}

// ... later, if accepted:
accepted_edge_sets.push(candidate_edges);
```

Type changes:

- `accepted_edge_sets: Vec<HashSet<...>>` → `Vec<HashMap<(NodeId, NodeId), Weight>>`

---

## Q3. Two-step recursive approach

**File:** `multi_route.rs`

New method on `MultiRouteServer`:

```rust
/// Recursive alternative search. Falls back to subproblem decomposition
/// when the basic approach yields fewer than `max_alternatives` routes.
fn multi_query_recursive(
    &mut self,
    from: NodeId,                    // original node ID
    to: NodeId,                      // original node ID
    max_alternatives: usize,
    stretch_factor: f64,
    sharing_threshold: f64,          // γ — adjusted per recursion level
    local_opt_fraction: f64,         // α — adjusted per recursion level
    original_distance: Weight,       // d(s,t) of the root problem
    path_geo_len: &impl Fn(&[NodeId]) -> f64,
    edge_cost: &impl Fn(NodeId, NodeId) -> Weight,
) -> Vec<AlternativeRoute> {
    let from_rank = self.customized.cch().node_order().rank(from);
    let to_rank = self.customized.cch().node_order().rank(to);

    // --- Basic approach first ---
    let meeting_candidates = self.collect_meeting_nodes(from_rank, to_rank);
    if meeting_candidates.is_empty() {
        return Vec::new();
    }
    let best_distance = meeting_candidates[0].1;

    // Recursion stop: subproblem too small
    if (best_distance as f64) < RECURSION_MIN_RATIO * original_distance as f64 {
        let path = self.reconstruct_path(from_rank, to_rank, meeting_candidates[0].0);
        if path.is_empty() { return Vec::new(); }
        return vec![AlternativeRoute {
            distance: best_distance,
            geo_distance_m: path_geo_len(&path),
            path,
        }];
    }

    let mut accepted = self.run_basic_selection(
        from_rank, to_rank, &meeting_candidates,
        max_alternatives, stretch_factor, sharing_threshold,
        local_opt_fraction, path_geo_len, edge_cost,
    );

    if accepted.len() >= max_alternatives {
        return accepted;
    }

    // --- Two-step decomposition ---
    let main_path = &accepted[0].path;
    let order = self.customized.cch().node_order();

    // Find highest-rank node on main path (= deepest separator vertex)
    let (v_pos, _) = main_path.iter().enumerate()
        .max_by_key(|(_, &node)| order.rank(node))
        .unwrap();

    // v_s = predecessor, v_t = successor of v on the path
    if v_pos == 0 || v_pos >= main_path.len() - 1 {
        return accepted; // v is at an endpoint, can't decompose
    }
    let v_s = main_path[v_pos - 1];
    let v_t = main_path[v_pos + 1];

    // Sub-distances (from cumulative costs of main path)
    let d_s_vs: Weight = (0..v_pos - 1)
        .map(|i| edge_cost(main_path[i], main_path[i + 1]))
        .fold(0u32, |a, c| a.saturating_add(c));
    let d_vt_t: Weight = (v_pos + 1..main_path.len() - 1)
        .map(|i| edge_cost(main_path[i], main_path[i + 1]))
        .fold(0u32, |a, c| a.saturating_add(c));
    let d_vs_vt = edge_cost(main_path[v_pos - 1], main_path[v_pos])
        .saturating_add(edge_cost(main_path[v_pos], main_path[v_pos + 1]));

    // Adjusted parameters
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

    // Recurse on left subproblem (s → v_s) if parameters are valid
    let left_alts = if alpha_left < 1.0 && gamma_left > 0.0 {
        // NOTE: need fresh walk arrays — reset or use a second MultiRouteServer
        self.reset();
        self.multi_query_recursive(
            from, v_s, max_alternatives, stretch_factor,
            gamma_left, alpha_left, original_distance,
            path_geo_len, edge_cost,
        )
    } else {
        vec![]
    };

    // Recurse on right subproblem (v_t → t)
    let right_alts = if alpha_right < 1.0 && gamma_right > 0.0 {
        self.reset();
        self.multi_query_recursive(
            v_t, to, max_alternatives, stretch_factor,
            gamma_right, alpha_right, original_distance,
            path_geo_len, edge_cost,
        )
    } else {
        vec![]
    };

    // Combine: pair each left alt with each right alt through fixed v_s→v→v_t
    let main_prefix = &main_path[..v_pos - 1];
    let main_middle = &main_path[v_pos - 1..=v_pos + 1]; // v_s, v, v_t
    let main_suffix = &main_path[v_pos + 2..];

    for left in &left_alts {
        for right in &right_alts {
            if accepted.len() >= max_alternatives {
                return accepted;
            }
            // Stitch: left.path + main_middle + right.path
            let mut combined = left.path.clone();
            combined.extend_from_slice(main_middle);
            combined.extend_from_slice(&right.path[1..]); // skip duplicate v_t

            let combined_dist = left.distance
                .saturating_add(d_vs_vt)
                .saturating_add(right.distance);
            let combined_geo = path_geo_len(&combined);

            // Re-check full admissibility on the combined path
            // (sharing, bounded stretch, T-test against root-level main path)
            // ... omitted — same checks as basic approach Phase 3 ...

            accepted.push(AlternativeRoute {
                distance: combined_dist,
                geo_distance_m: combined_geo,
                path: combined,
            });
        }
    }

    accepted
}

/// Reset distance arrays for reuse across recursive calls.
fn reset(&mut self) {
    self.fw_distances.fill(INFINITY);
    self.bw_distances.fill(INFINITY);
    // Parent arrays: no reset needed (only read after walk writes them)
    // TimestampedVector: auto-resets on next query() call
}
```

---

## Q4. Travel-time early termination

**File:** `multi_route.rs`, in `multi_query` Phase 3 loop.

```rust
for &(meeting_node, dist) in meeting_candidates.iter().skip(1) {
    if accepted.len() >= max_alternatives {
        break;
    }

    // Early termination: candidates are sorted by distance.
    // Once past stretch limit, all remaining are worse.
    if dist > (best_distance as f64 * stretch_factor) as Weight {
        break;
    }

    let path = self.reconstruct_path(from_rank, to_rank, meeting_node);
    // ... rest of checks ...
}
```

---

## P1. Persistent `MultiRouteServer`

**Files:** `cch.rs`, `line_graph.rs`, `multi_route.rs`

In `multi_route.rs` — add `reset()` (shown in Q3 above).

In `cch.rs` — change `QueryEngine`:

```rust
pub struct QueryEngine<'a> {
    server: CchQueryServer<CustomizedBasic<'a, CCH>>,
    context: &'a CchContext,
    spatial: SpatialIndex,
    validation_config: ValidationConfig,
    multi_server: Option<MultiRouteServer<'a, CustomizedBasic<'a, CCH>>>,
}

// In multi_query():
let multi = self.multi_server.get_or_insert_with(|| {
    MultiRouteServer::new(self.server.customized())
});
multi.reset();
let candidates = multi.multi_query(/* ... */);
```

Same pattern in `LineGraphQueryEngine`.

Note: `update_weights` must invalidate the cached server
(`self.multi_server = None`).

---

## P2. Filter reorder

Merged with Q4. New Phase 3 order in `multi_query`:

```rust
for &(meeting_node, dist) in meeting_candidates.iter().skip(1) {
    if accepted.len() >= max_alternatives { break; }

    // 1. Travel-time stretch (free, enables break)
    if dist > (best_distance as f64 * stretch_factor) as Weight { break; }

    // 2. Reconstruct path
    let path = self.reconstruct_path(from_rank, to_rank, meeting_node);
    if path.is_empty() { continue; }

    // 3. Loop detection
    if has_repeated_nodes(&path) { continue; }

    // 4. Bounded stretch (Q1) — needs CCH point query for d(a,b)
    // ... deviation point check ...

    // 5. Cost-based sharing (Q2) — needs edge cost scan
    let candidate_edges = build_cost_edge_set(&path, &edge_cost);
    let sharing_limit = (SHARING_THRESHOLD * best_distance as f64) as Weight;
    if accepted_edge_sets.iter().any(|prev| shared_cost(&candidate_edges, prev) > sharing_limit) {
        continue;
    }

    // 6. T-test (most expensive)
    if !self.check_local_optimality(&path, meeting_node, best_distance, &edge_cost) {
        continue;
    }

    accepted_edge_sets.push(candidate_edges);
    accepted.push(AlternativeRoute { distance: dist, geo_distance_m: path_geo_len(&path), path });
}
```

Geo stretch stays in `cch.rs`/`line_graph.rs` as a post-filter (callers own
coordinate data, `multi_route.rs` doesn't).

---

## P3. Edge costs during unpacking

**File:** `multi_route.rs`

Change `reconstruct_path` return type:

```rust
fn reconstruct_path_with_costs(
    &self,
    from: NodeId,
    to: NodeId,
    meeting_node: NodeId,
) -> (Vec<NodeId>, Vec<Weight>) {
    // ... same tracing logic ...

    let mut path = vec![from];
    let mut edge_costs = Vec::new();

    for &(tail, head, edge) in &fw_edges {
        // Read edge weight from customized graph during unpacking
        Self::unpack_edge_with_costs(tail, head, edge, self.customized, &mut path, &mut edge_costs);
        path.push(head);
    }
    // ... same for bw_edges ...

    // Convert ranks to original IDs (edge costs stay the same)
    let order = self.customized.cch().node_order();
    for node in &mut path { *node = order.node(*node); }

    (path, edge_costs)
}
```

This eliminates the `edge_cost` closure for T-test and sharing cost
computation — costs come from unpacking, not CSR adjacency scans.

---

## P4. Lazy edge-set construction

Move `build_cost_edge_set` after bounded stretch check (already shown in P2
ordering). No separate code change needed.

---

## Implementation Order

1. **Q4 + P2** — early termination + filter reorder (trivial, pairs naturally)
2. **Q2** — cost-based sharing (moderate, swap data structure)
3. **Q1** — bounded stretch with deviation points (quality, needs `cch_point_distance`)
4. **P1** — persistent server (performance, independent)
5. **Q3** — recursive approach (biggest quality win, depends on Q1+Q2)
6. **P3** — edge costs during unpacking (cleanup, depends on Q1)

## Why no `rust_road_router` changes

Evaluated three candidates:

- `all_meeting_nodes` on query Server — the walk in `multi_route.rs` is
necessarily different (no pruning, no distance reset). Embedding it would
mix concerns.
- Non-destructive `path_via` — would duplicate unpacking or refactor core path
infrastructure. Read-only reconstruction already works.
- Edge weight accessors on `Customized` — marginal; P3 reads from the same
arrays during unpacking without needing new API.

