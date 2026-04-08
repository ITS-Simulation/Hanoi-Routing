# Multi-Route Improvements Plan

All work in `hanoi-core/src/multi_route.rs` and `cch.rs`/`line_graph.rs`.
R1 (final step) adds one file and one line to `rust_road_router` — see
R1 section for non-destructive guarantee.

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
| tt cap                 | —       | —     | `TRAVEL_TIME_STRETCH` (new) | Travel-time early break  |


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

/// Travel-time cap for early termination of candidate loop.
/// Looser than geo stretch — only prunes extreme outliers.
const TRAVEL_TIME_STRETCH: f64 = 1.5;
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
///
/// `candidate_costs` is a per-edge cost vector from P3's
/// `reconstruct_path_with_costs` (length = path.len() - 1). This eliminates
/// the ambiguous `edge_cost(NodeId, NodeId)` closure. Only candidate costs
/// are needed — deviation points are node-identity-based, and accumulated
/// costs (cost_s_a, cost_b_t) are always on the candidate side.
fn find_deviation_points(
    candidate: &[NodeId],
    candidate_costs: &[Weight],
    reference: &[NodeId],
) -> Option<DeviationPoints> {
    // Degenerate: need at least 2 nodes (1 edge) in each path.
    if candidate.len() < 2 || reference.len() < 2 {
        return None;
    }

    // Forward: find first divergence point `a`
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
            return None; // identical paths
        }
    }

    // Backward: find rejoin point `b` by walking both paths inward from t.
    //
    // The detour may change path length, so the indices are NOT aligned by
    // absolute position — they are aligned by *distance from the tail*.
    // Walk ci (candidate) and ri (reference) backward simultaneously,
    // comparing node values. The first mismatch marks the detour boundary;
    // the rejoin point b is the node one step closer to t (ci + 1).
    //
    // Example: candidate s-a-x-b-t (len 5), reference s-a-b-t (len 4)
    //   ci=4,ri=3: t==t ✓  ci=3,ri=2: b==b ✓  ci=2,ri=1: x!=a → b_pos=3 ✓
    let mut b_pos = candidate.len() - 1;
    let mut cost_b_t: Weight = 0;
    let mut ci = candidate.len() - 1;
    let mut ri = reference.len() - 1;
    while ci > a_pos && ri > 0 && candidate[ci] == reference[ri] {
        if ci < candidate.len() - 1 {
            cost_b_t = cost_b_t.saturating_add(candidate_costs[ci]);
        }
        ci -= 1;
        ri -= 1;
    }
    // ci is now at the last mismatching position; b = ci + 1
    b_pos = ci + 1;

    Some(DeviationPoints { a_pos, b_pos, cost_s_a, cost_b_t })
}
```

Usage in `multi_query` Phase 3, after loop detection, before sharing.
Assumes `reconstruct_path_with_costs` (P3) returns `(path, edge_costs)`:

```rust
// Bounded Stretch check (uses P3 edge_costs vectors, not the closure)
if let Some(dev) = find_deviation_points(&path, &edge_costs, &main_path) {
    // Cost of detour segment a→v→b (sum edge costs from P3 vector)
    let detour_cost: Weight = edge_costs[dev.a_pos..dev.b_pos]
        .iter()
        .copied()
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
/// Uses the per-edge cost vector from P3 (not the edge_cost closure).
fn build_cost_edge_set(
    path: &[NodeId],
    edge_costs: &[Weight],
) -> HashMap<(NodeId, NodeId), Weight> {
    path.windows(2)
        .zip(edge_costs.iter())
        .map(|(w, &cost)| ((w[0], w[1]), cost))
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
let candidate_edges = build_cost_edge_set(&path, &edge_costs);

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
    // No edge_cost closure — P3's edge_costs vectors in AlternativeRoute
    // provide per-edge costs for sub-distance computation.
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
        let (path, edge_costs) = self.reconstruct_path_with_costs(from_rank, to_rank, meeting_candidates[0].0);
        if path.is_empty() { return Vec::new(); }
        return vec![AlternativeRoute {
            distance: best_distance,
            path,
            edge_costs,
        }];
    }

    // run_basic_selection: extracted helper containing current multi_query's
    // Phase 2 (reconstruct main path) + Phase 3 (filter loop) logic.
    // It always seeds accepted[0] with the shortest path before filtering
    // candidates — same as multi_query lines 144–205 in the current code.
    // Input: meeting candidates + filter params. Output: Vec<AlternativeRoute>
    // with accepted[0] = shortest path (if any path exists).
    let mut accepted = self.run_basic_selection(
        from_rank, to_rank, &meeting_candidates,
        max_alternatives, stretch_factor, sharing_threshold,
        local_opt_fraction, path_geo_len,
    );

    if accepted.is_empty() {
        return Vec::new();
    }
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

    // Sub-distances from P3 edge costs of main path.
    // accepted[0] must also store its edge_costs vector.
    let main_costs = &accepted[0].edge_costs;
    let d_s_vs: Weight = main_costs[..v_pos - 1]
        .iter().copied()
        .fold(0u32, |a, c| a.saturating_add(c));
    let d_vt_t: Weight = main_costs[v_pos + 1..]
        .iter().copied()
        .fold(0u32, |a, c| a.saturating_add(c));
    let d_vs_vt = main_costs[v_pos - 1]
        .saturating_add(main_costs[v_pos]);

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
            path_geo_len,
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
            path_geo_len,
        )
    } else {
        vec![]
    };

    // Combine: pair each left alt with each right alt through fixed v_s→v→v_t.
    //
    // Path segments:
    //   left.path  = [s, ..., v_s]         (ends at v_s)
    //   middle     = [v_s, v, v_t]         (fixed bridge through separator)
    //   right.path = [v_t, ..., t]         (starts at v_t)
    //
    // To avoid duplicate nodes at the joins:
    //   left.path[..last] + middle + right.path[1..]
    //   i.e., skip v_s at end of left, skip v_t at start of right.
    let main_middle = &main_path[v_pos - 1..=v_pos + 1]; // [v_s, v, v_t]

    for left in &left_alts {
        for right in &right_alts {
            if accepted.len() >= max_alternatives {
                return accepted;
            }
            // Stitch: left.path (drop last v_s) + main_middle + right.path (skip v_t)
            let mut combined = left.path[..left.path.len() - 1].to_vec();
            combined.extend_from_slice(main_middle);
            combined.extend_from_slice(&right.path[1..]); // skip duplicate v_t

            let combined_dist = left.distance
                .saturating_add(d_vs_vt)
                .saturating_add(right.distance);
            // Geo length for admissibility re-check (inline, not stored in struct)
            let combined_geo = path_geo_len(&combined);

            // Stitch edge_costs: ALL of left + middle bridge + ALL of right.
            //
            // Node stitch drops the trailing v_s from left.path and leading v_t
            // from right.path to avoid duplicates, but the EDGES are different:
            //   left.edge_costs  covers s→...→v_s  (all edges, including →v_s)
            //   middle           covers v_s→v→v_t   (2 edges from main path)
            //   right.edge_costs covers v_t→...→t   (all edges, including v_t→)
            // No edges are dropped — the node dedup doesn't remove edges.
            let mut combined_costs = left.edge_costs.clone();
            combined_costs.extend_from_slice(&main_costs[v_pos - 1..=v_pos]); // v_s→v, v→v_t
            combined_costs.extend_from_slice(&right.edge_costs);

            // Re-check full admissibility on the combined path
            // (sharing, bounded stretch, T-test against root-level main path)
            // ... omitted — same checks as basic approach Phase 3 ...

            accepted.push(AlternativeRoute {
                distance: combined_dist,
                path: combined,
                edge_costs: combined_costs,
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

**Important:** `stretch_factor` is the caller's **geographic** stretch
bound (currently used at line 149 of `multi_route.rs` for
`geo_stretch_limit`). The travel-time early-break uses a **separate**
constant so the two concerns don't conflate:

```rust
/// Travel-time cap for candidate pre-filtering.
/// Looser than geo stretch — only purpose is to avoid reconstructing
/// paths that are obviously too long. Not a quality guarantee.
const TRAVEL_TIME_STRETCH: f64 = 1.5;
```

```rust
for &(meeting_node, dist) in meeting_candidates.iter().skip(1) {
    if accepted.len() >= max_alternatives {
        break;
    }

    // Early termination: candidates are sorted by distance.
    // Once past travel-time cap, all remaining are worse.
    // This is a cheap prefilter — geo stretch still applies later.
    if dist > (best_distance as f64 * TRAVEL_TIME_STRETCH) as Weight {
        break;
    }

    let path = self.reconstruct_path(from_rank, to_rank, meeting_node);
    // ... rest of checks (geo stretch unchanged at line 177) ...
}
```

The existing geo-stretch check (`candidate_geo > geo_stretch_limit`,
line 179) remains as the quality bound. `TRAVEL_TIME_STRETCH` is
intentionally looser (1.5 vs 1.3) so it only prunes extreme outliers
without accidentally rejecting geographically-short but travel-time-long
alternatives (e.g., shorter highway vs longer local road).

---

## P1. Persistent `MultiRouteServer`

**Files:** `cch.rs`, `line_graph.rs`, `multi_route.rs`

In `multi_route.rs` — add `reset()` (shown in Q3 above).

### Why caching is not straightforward

`QueryEngine` owns `server` (which owns `CustomizedBasic`).
A `MultiRouteServer` borrows `server.customized()`. Storing both in the
same struct is self-referential — Rust forbids this.

`CchContext` doesn't help either: it owns `cch` and `baseline_weights`
but does NOT own a `CustomizedBasic`. Its `customize()` method creates a
new one each call — there's nothing persistent to borrow.

### Recommended approach: per-query creation with `reset()`

Keep the current per-query pattern. The real cost isn't the `MultiRouteServer`
construction — it's `Vec::new()` for the distance/parent arrays. Add
`reset()` so a *caller-owned* server can be reused across queries without
reallocating:

```rust
// In multi_route.rs:
impl<'a, C: Customized> MultiRouteServer<'a, C> {
    pub fn reset(&mut self) {
        self.fw_distances.fill(INFINITY);
        self.bw_distances.fill(INFINITY);
        // Parent arrays: overwritten during walk, no reset needed.
        // TimestampedVector: auto-resets on next query() call.
    }
}
```

Callers continue to create `MultiRouteServer` per-query (borrowing from
the `CustomizedBasic` returned by `self.server.customized()`). The
server is dropped at the end of `multi_query()`, so no self-referential
issue arises.

```rust
// In cch.rs::multi_query() — unchanged pattern:
let customized = self.server.customized();
let mut multi = MultiRouteServer::new(customized);
let candidates = multi.multi_query(/* ... */);
// multi dropped here
```

If profiling shows per-query allocation is a bottleneck (unlikely —
`fill(INFINITY)` is ~1µs for 500k nodes), the fix is to hoist the
allocation to the `engine.rs` background loop, where a long-lived
`MultiRouteServer` can borrow the `CustomizedBasic` across queries
within a single `&mut` borrow scope. This is an engine-level change,
not a `QueryEngine` struct change.

Same pattern in `LineGraphQueryEngine`.

---

## P2. Filter reorder

Merged with Q4. New Phase 3 order in `multi_query`:

```rust
for &(meeting_node, dist) in meeting_candidates.iter().skip(1) {
    if accepted.len() >= max_alternatives { break; }

    // 1. Travel-time cap (free, enables break — separate from geo stretch)
    if dist > (best_distance as f64 * TRAVEL_TIME_STRETCH) as Weight { break; }

    // 2. Reconstruct path with per-edge costs (P3)
    let (path, edge_costs) = self.reconstruct_path_with_costs(from_rank, to_rank, meeting_node);
    if path.is_empty() { continue; }

    // 3. Loop detection
    if has_repeated_nodes(&path) { continue; }

    // 4. Bounded stretch (Q1) — needs CCH point query for d(a,b)
    // ... deviation point check ...

    // 5. Cost-based sharing (Q2) — uses P3 edge costs
    let candidate_edges = build_cost_edge_set(&path, &edge_costs);
    let sharing_limit = (SHARING_THRESHOLD * best_distance as f64) as Weight;
    if accepted_edge_sets.iter().any(|prev| shared_cost(&candidate_edges, prev) > sharing_limit) {
        continue;
    }

    // 6. T-test (most expensive — uses P3 edge_costs for subpath cost)
    if !self.check_local_optimality(&path, &edge_costs, meeting_node, best_distance) {
        continue;
    }

    accepted_edge_sets.push(candidate_edges);
    accepted.push(AlternativeRoute { distance: dist, path, edge_costs });
}
```

**Geo stretch unchanged:** The travel-time early-break uses
`TRAVEL_TIME_STRETCH` (a separate, looser constant). The existing
geo-stretch check inside `multi_route.rs` (`candidate_geo >
geo_stretch_limit` at line 179, using `stretch_factor`) is NOT
displaced — it remains the quality-controlling filter. The callers'
`MAX_GEO_RATIO` post-filter also stays as an absolute safety cap.

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

### Struct change: add `edge_costs` to `AlternativeRoute`

P3 produces per-edge costs alongside the path. Q3's recursive
decomposition needs these costs to compute sub-distances (see Q3 line
referencing `accepted[0].edge_costs`). Update the struct in
`multi_route.rs`:

```rust
#[derive(Debug, Clone)]
pub struct AlternativeRoute {
    pub distance: Weight,
    pub path: Vec<NodeId>,
    /// Per-edge costs from P3 unpacking (length = path.len() - 1).
    pub edge_costs: Vec<Weight>,
}
```

`geo_distance_m` is intentionally absent. The geo-stretch filter calls
`path_geo_len(&path)` inline without storing the result — it's a
pass/fail check, not a field. Callers (`cch.rs`, `line_graph.rs`)
compute geographic distance independently from coordinates when building
`QueryAnswer`. This keeps the struct consistent with the R1 version in
`rust_road_router` (which has no coordinate concept).

All code that constructs `AlternativeRoute` must populate `edge_costs`
from `reconstruct_path_with_costs`. Callers outside `multi_route.rs`
(`cch.rs`, `line_graph.rs`) only read `.path` and `.distance` — the
`edge_costs` field is internal to the multi-route pipeline.

### Cleanup: remove `edge_cost` closure from callers

After P3, the `edge_cost` parameter on `MultiRouteServer::multi_query` is
dead. Remove it from the signature and delete the closure construction in
all three call sites:

- `cch.rs::multi_query` ([line 267](CCH-Hanoi/crates/hanoi-core/src/cch.rs#L267))
- `line_graph.rs::multi_query` ([line 598](CCH-Hanoi/crates/hanoi-core/src/line_graph.rs#L598))
- `line_graph.rs::multi_query_coords` ([line 679](CCH-Hanoi/crates/hanoi-core/src/line_graph.rs#L679))

Single-route queries (`query`, `query_coords`, `query_trimmed`) never
used this closure — no changes needed there.

---

## P4. Lazy edge-set construction

Move `build_cost_edge_set` after bounded stretch check (already shown in P2
ordering). No separate code change needed.

---

## L1. Strategic tracing for multi-route pipeline

Currently `multi_route.rs` has zero tracing. `cch.rs` multi_query methods have
TODO comments but no instrumentation. Only `engine.rs` logs the final count.

**Goal:** trace the full pipeline at appropriate levels so that operators can
diagnose slow queries, low alternative yield, and filter rejection patterns
without enabling verbose debug logging.

### Files and locations

**`multi_route.rs` — algorithm internals (DEBUG/TRACE level):**

```rust
// In collect_meeting_nodes(), after sort+dedup:
tracing::debug!(
    num_candidates = meeting_candidates.len(),
    best_distance = meeting_candidates.first().map(|c| c.1),
    "meeting nodes collected"
);

// In multi_query(), after Phase 3 loop — rejection breakdown:
tracing::debug!(
    total_candidates,
    rejected_empty_path,
    rejected_loop,
    rejected_stretch,
    rejected_sharing,
    rejected_ttest,
    accepted = accepted.len(),
    "candidate filtering summary"
);
```

Track rejections with simple counters in the Phase 3 loop:

```rust
let mut rejected_empty_path = 0u32;
let mut rejected_loop = 0u32;
let mut rejected_stretch = 0u32;
let mut rejected_sharing = 0u32;
let mut rejected_ttest = 0u32;

for &(meeting_node, dist) in meeting_candidates.iter().skip(1) {
    // ... each `continue` increments the corresponding counter ...
}

tracing::debug!(
    total_candidates = meeting_candidates.len() - 1,
    rejected_empty_path,
    rejected_loop,
    rejected_stretch,
    rejected_sharing,
    rejected_ttest,
    accepted = accepted.len(),
    "candidate filtering summary"
);
```

**`cch.rs` — query entry points (INFO level):**

```rust
// multi_query — add instrument + result log
#[tracing::instrument(skip(self), fields(from, to, max_alternatives, stretch_factor))]
pub fn multi_query(
    &mut self, from: NodeId, to: NodeId,
    max_alternatives: usize, stretch_factor: f64,
) -> Vec<QueryAnswer> {
    // ... existing logic ...
    tracing::info!(
        requested = max_alternatives,
        returned = results.len(),
        shortest_ms = results.first().map(|r| r.distance_ms),
        "multi_query completed"
    );
    results
}

// multi_query_coords — add instrument + snap info
#[tracing::instrument(skip(self), fields(
    from_lat = from.0, from_lng = from.1,
    to_lat = to.0, to_lng = to.1
))]
pub fn multi_query_coords(
    &mut self, from: (f32, f32), to: (f32, f32),
    max_alternatives: usize, stretch_factor: f64,
) -> Result<Vec<QueryAnswer>, CoordRejection> {
    // ... existing logic ...
}
```

**`line_graph.rs` — same pattern for `multi_query` / `multi_query_coords`.**

### Level guide

| Level | What gets logged | When to enable |
|---|---|---|
| INFO | Entry/exit of `multi_query`/`multi_query_coords`, result count, shortest distance | Always (production) |
| DEBUG | Candidate count, rejection breakdown by filter, snap candidate pairs | Investigating low alternative yield |
| TRACE | Per-candidate decisions (which filter rejected which meeting node) | Deep debugging only |

---

## Implementation Order

1. **L1** — tracing instrumentation (independent, do first for observability
   during all subsequent work)
2. **P3** — edge costs during unpacking (**moved up**: Q1/Q2/Q3 all need
   accurate per-edge costs. The current `edge_cost(NodeId, NodeId)` closure
   does a CSR linear scan and returns the *first* matching arc — wrong when
   parallel arcs exist. P3 eliminates this ambiguity by extracting costs
   during shortcut unpacking, where the exact edge ID is known. Must land
   before any code that computes subpath costs or edge-set costs.)
3. **Q4 + P2** — early termination + filter reorder (trivial, pairs naturally)
4. **Q2** — cost-based sharing (moderate, swap data structure; uses P3 costs)
5. **Q1** — bounded stretch with deviation points (quality, uses P3 costs +
   `cch_point_distance`)
6. **P1** — persistent server (performance, independent)
7. **Q3** — recursive approach (biggest quality win, depends on Q1+Q2)
8. **R1** — migrate proven code into `rust_road_router/query/alternative.rs`
   (mechanical move after Q1–Q3 + P1–P3 are tested)

### Dependency: `edge_cost` → P3

Q1's `find_deviation_points`, Q2's `build_cost_edge_set`, and Q3's
sub-distance computation all need per-edge costs. The current
`edge_cost(tail, head)` closure scans the CSR adjacency list and returns
the first arc matching `head` — ambiguous when parallel arcs connect the
same node pair (common in OSM data, e.g., service roads alongside
highways).

After P3, `reconstruct_path_with_costs` returns `(Vec<NodeId>,
Vec<Weight>)` — one cost per edge, extracted during unpacking where the
exact `EdgeId` is known. All downstream consumers use this vector
instead of the closure. This makes `edge_cost(NodeId, NodeId)` obsolete
for the multi-route pipeline.

## R1. Migrate algorithm core into `rust_road_router`

The modified bidirectional walk, non-destructive path reconstruction, **and**
admissibility filters all depend exclusively on public API (`Customized`,
`CCHT`, `EliminationTreeWalk`). They can live as a new peer module alongside
`query.rs` — zero existing code touched.

**Precedent:** `query/nearest_neighbor.rs` was added as a self-contained query
type using the same pattern.

### File structure

```
rust_road_router/engine/src/algo/customizable_contraction_hierarchy/
  query.rs              ← add one line: pub mod alternative;
  query/
    stepped_elimination_tree.rs  (untouched)
    nearest_neighbor.rs          (untouched)
    alternative.rs               (NEW)
```

### What moves into `alternative.rs`

**Algorithm core (walk + reconstruction):**

```rust
use super::*;
use super::stepped_elimination_tree::EliminationTreeWalk;
use crate::datastr::timestamped_vector::TimestampedVector;
use std::collections::HashMap;

/// Tuning constants — paper-standard values (ATMOS 2025).
pub const DEFAULT_STRETCH: f64 = 1.25;
const SHARING_THRESHOLD: f64 = 0.80;
const BOUNDED_STRETCH_EPS: f64 = 0.25;
const LOCAL_OPT_T_FRACTION: f64 = 0.25;
const LOCAL_OPT_EPSILON: f64 = 0.0;
const RECURSION_MIN_RATIO: f64 = 0.30;
const TRAVEL_TIME_STRETCH: f64 = 1.5;

/// `geo_distance_m` is absent from this type — `rust_road_router` has
/// no concept of geographic coordinates. Callers (`cch.rs`,
/// `line_graph.rs`) compute geographic distance from lat/lng arrays
/// when building `QueryAnswer`. The hanoi-core pre-R1 struct also
/// omits this field; the geo-stretch filter uses `path_geo_len` inline
/// without storing the result.
#[derive(Debug, Clone)]
pub struct AlternativeRoute {
    pub distance: Weight,
    pub path: Vec<NodeId>,
    /// Per-edge costs extracted during unpacking (length = path.len() - 1).
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
    pub fn new(customized: &'a C) -> Self { /* same as current MultiRouteServer::new */ }
    pub fn reset(&mut self) { /* fill distances with INFINITY */ }

    /// Find up to K alternative routes. Pure algorithm — no geo/coordinate logic.
    /// Edge costs are extracted during unpacking (P3) — no edge_cost closure needed.
    pub fn alternatives(
        &mut self,
        from: NodeId,           // original node IDs
        to: NodeId,
        max_alternatives: usize,
        stretch_factor: f64,    // geo stretch bound
        path_geo_len: impl Fn(&[NodeId]) -> f64,
    ) -> Vec<AlternativeRoute> { /* orchestration: walk → reconstruct_with_costs → filter */ }

    // --- Private methods (all move verbatim) ---
    fn collect_meeting_nodes(&mut self, from_rank: NodeId, to_rank: NodeId)
        -> Vec<(NodeId, Weight)> { /* ... */ }

    fn reconstruct_path_with_costs(&self, from: NodeId, to: NodeId, meeting_node: NodeId)
        -> (Vec<NodeId>, Vec<Weight>) { /* P3: returns path + per-edge costs */ }

    fn unpack_edge_with_costs(
        tail: NodeId, head: NodeId, edge: EdgeId,
        customized: &C, path: &mut Vec<NodeId>, costs: &mut Vec<Weight>,
    ) { /* ... */ }

    fn cch_point_distance(&mut self, from_rank: NodeId, to_rank: NodeId)
        -> Weight { /* ... */ }
}
```

**Admissibility filters (also move into `alternative.rs`):**

```rust
// All filters are methods on AlternativeServer or free functions.
// They depend only on Customized trait + node order — no external types.

impl<'a, C: Customized> AlternativeServer<'a, C> {
    /// T-test local optimality check. Uses P3 edge_costs, not a closure.
    fn check_local_optimality(
        &mut self,
        path: &[NodeId],
        edge_costs: &[Weight],
        meeting_node_rank: NodeId,
        best_distance: Weight,
    ) -> bool { /* prefix sums from edge_costs, CCH query for d(v',v'') */ }
}

/// Deviation points for bounded stretch check.
struct DeviationPoints {
    a_pos: usize,
    b_pos: usize,
    cost_s_a: Weight,
    cost_b_t: Weight,
}

fn find_deviation_points(
    candidate: &[NodeId],
    candidate_costs: &[Weight],
    reference: &[NodeId],
) -> Option<DeviationPoints> { /* pure function on slices + candidate cost vector */ }

/// Cost-weighted edge set for sharing check.
fn build_cost_edge_set(
    path: &[NodeId],
    edge_costs: &[Weight],
) -> HashMap<(NodeId, NodeId), Weight> { /* zip path edges with cost vector */ }

fn shared_cost(
    candidate: &HashMap<(NodeId, NodeId), Weight>,
    reference: &HashMap<(NodeId, NodeId), Weight>,
) -> Weight { /* ... */ }

fn has_repeated_nodes(path: &[NodeId]) -> bool { /* ... */ }
```

### What stays in `hanoi-core/src/multi_route.rs`

After R1, `multi_route.rs` is a constants-and-re-exports shim. It owns
no wrapper function — `cch.rs` and `line_graph.rs` call
`AlternativeServer::alternatives()` directly, construct the
`path_geo_len` closure locally (binding to `GraphData` coordinates),
and apply post-filters (`MAX_GEO_RATIO`) when building `QueryAnswer`.
This matches the existing pattern: today those callers already construct
the `edge_cost` closure and do post-filtering inline.

```rust
// hanoi-core/src/multi_route.rs — constants + re-exports only

pub use rust_road_router::algo::customizable_contraction_hierarchy::query::alternative::{
    AlternativeServer, AlternativeRoute, DEFAULT_STRETCH,
};

pub const MAX_GEO_RATIO: f64 = 2.0;
pub const GEO_OVER_REQUEST: usize = 3;
```

### Non-destructive guarantee

| rr file | Change | Nature |
|---|---|---|
| `query.rs` | `pub mod alternative;` (1 line added) | Additive — no existing line modified/removed |
| `query/alternative.rs` | New file | Did not exist before |
| Everything else | **None** | Untouched |

No existing struct, trait, method, function, or constant is modified.
Rollback: delete `alternative.rs`, remove the `pub mod` line. Zero residue.

### Internal API access assessment

Being inside the crate grants access to private items. Audit of what's
relevant:

| Private item | Location | Useful? | Why / why not |
|---|---|---|---|
| `Server::distance()` | `query.rs:44` | No | We need a different walk (no pruning, no reset) |
| `Server::path()` | `query.rs:131` | No | Destructive (overwrites `bw_parents`), single-meeting-node only |
| `Server::unpack_path()` | `query.rs:159` | No | Same issue — mutates parents in-place |
| `CustomizedBasic::upward/downward` | `mod.rs:437-438` | No | Already accessible via `forward_graph().weight()` / `backward_graph().weight()` |
| `CustomizedBasic::up/down_unpacking` | `mod.rs:439-440` | No | Already accessible via `forward_unpacking()` / `backward_unpacking()` |

**Conclusion:** crate-level access grants no additional useful API.
All required interfaces (`Customized`, `CCHT`, `EliminationTreeWalk`,
`BorrowedGraph::weight()`) are already public. The migration is purely
organizational — the code would be identical either way.

### Why migrate anyway

- **Same pattern** as `nearest_neighbor.rs` — self-contained query variant
  under `query/`, following the existing module convention.
- **Locality** — algorithm code lives next to the data structures it operates
  on, making it easier for anyone reading rr to discover.
- **Reusability** — any future project using `rust_road_router` gets
  alternative route support without pulling in `hanoi-core`.

### Implementation note

Do this **after** Q1–Q4 and P1–P3 are implemented and tested in `hanoi-core`.
Migrate the proven code, not the planned code. The migration is mechanical —
move functions, update imports, verify tests still pass.

---

## Bug Resolutions

Bugs identified during review and resolved inline:

| # | Severity | Section | Bug | Resolution |
|---|---|---|---|---|
| 1 | High | P1 | Self-referential struct: `QueryEngine` can't own `multi_server` borrowing from `self.server` | Dropped caching approach; use per-query creation with `reset()`. Hoist to engine loop if profiling warrants |
| 2 | High | Q4/P2 | Travel-time early-break reused `stretch_factor`, silently changing its geo-stretch meaning | Introduced separate `TRAVEL_TIME_STRETCH` constant (1.5); geo stretch unchanged |
| 3 | High | Q1 | Backward scan set `b_pos = ci` (mismatch index) instead of `ci + 1` (rejoin node) | Fixed to `b_pos = ci + 1` with worked example in comments |
| 4 | High | Q3 | `left.path` ends at `v_s`, `main_middle` starts at `v_s` → duplicate → `has_repeated_nodes` rejects | Stitch uses `left.path[..last]` to drop trailing `v_s` before appending middle |
| 5 | Medium | Q1/Q2/Q3 | `edge_cost(NodeId, NodeId)` returns first CSR match; wrong with parallel arcs | Moved P3 to step 2; all snippets use `edge_costs: &[Weight]` vectors |
| 6 | High | P1 | Sibling borrow via `CchContext` unworkable: `CchContext` doesn't own a `CustomizedBasic` | Replaced with per-query pattern + `reset()`; sibling borrow removed entirely |
| 7 | High | Q4/P2 | Caller-side geo-stretch `retain()` runs after max_alternatives cutoff — misses valid candidates | Removed `retain()`; geo stretch stays inside `multi_route.rs` Phase 3 loop (already correctly positioned) |
| 8 | Medium | P3/Q3 | `AlternativeRoute.edge_costs` field used but never declared | Added explicit struct definition with `edge_costs: Vec<Weight>` field |
| 9 | Medium | R1 | Header said "No rust_road_router changes"; R1 code used stale `edge_cost` closure signatures | Updated header; aligned all R1 signatures with P3 (`edge_costs` vectors, no closure) |
| 10 | High | Q3 | Edge-cost stitch dropped `c(a,v_s)` and `c(v_t,b)` — off-by-one from mirroring node dedup onto edges | Keep ALL of left/right edge_costs; only nodes are deduped, not edges |
| 11 | Medium | Q3 | `run_basic_selection` never defined; `accepted[0]` indexed without emptiness guard | Added definition note (extracted from current Phase 2/3) + `if accepted.is_empty()` guard |
| 12 | Medium | Q1 | `find_deviation_points` has no length guard (`min_len - 1` underflows for len-0/1 paths) and accepts unused `reference_costs` parameter | Added `len < 2` early return; removed `reference_costs` from signature (only candidate costs needed) |
| 13 | Low | R1 | R1 `AlternativeRoute` omits `geo_distance_m` present in hanoi-core version — unclear if intentional | Dropped `geo_distance_m` from both types. Field was never consumed inside `multi_route.rs` — geo-stretch filter uses `path_geo_len` inline. Callers compute geo distance independently for `QueryAnswer`. |
| 14 | Medium | R1 | Adapter section re-exports `AlternativeRoute` from rr but Bug 13 says hanoi-core keeps its own version with `geo_distance_m` — contradictory | Resolved by dropping `geo_distance_m` from both structs. Adapter re-exports `DEFAULT_STRETCH` only; callers use `AlternativeRoute` directly from rr without wrapper |
| 15 | Low | R1 | Adapter described as "thin wrapper" providing closures/post-filters, but code snippet was just constants + re-exports with no wrapper function | Clarified: `multi_route.rs` is a constants+re-exports shim only. `cch.rs`/`line_graph.rs` call `AlternativeServer` directly and do closure construction + post-filtering inline (matches existing pattern) |

