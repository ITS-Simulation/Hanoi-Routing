# Alternative Route Quality Investigation

> **Scope**: Root-cause analysis of bizarre turning behaviour in K-alternative
> route generation — U-turns, unnecessary lengthening, missed better
> alternatives.
>
> **Modules under investigation**:
>
> - `CCH-Hanoi/crates/hanoi-core/src/multi_route.rs` — via-node K-alternatives
> - `CCH-Hanoi/crates/hanoi-core/src/cch.rs` — normal-graph multi-query wrapper
> - `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs` — line-graph multi-query
> - `CCH-Hanoi/crates/hanoi-tools/src/bin/generate_line_graph.rs` — line-graph
>   construction & turn cost assignment
> - `rust_road_router/engine/src/algo/customizable_contraction_hierarchy/query.rs`
>   — upstream CCH query (reference implementation)
> - `rust_road_router/engine/src/datastr/graph.rs:181` — `line_graph()` function
>   (weight formula)

---

## Table of Contents

1. [Background](#1-background)
2. [How the Current Algorithm Works](#2-how-the-current-algorithm-works)
3. [Problem 1 — Via-Node Alternatives Lack Admissibility Guarantees](#3-problem-1--via-node-alternatives-lack-admissibility-guarantees)
4. [Problem 2 — Zero Turn Costs in the Line Graph](#4-problem-2--zero-turn-costs-in-the-line-graph)
5. [Problem 3 — Pure Travel-Time Ranking](#5-problem-3--pure-travel-time-ranking)
6. [How the Problems Combine](#6-how-the-problems-combine)
7. [Proposed Fixes](#7-proposed-fixes)
8. [Fix Priority Matrix](#8-fix-priority-matrix)

---

## 1. Background

The alternative-route feature uses a **via-node** approach on top of CCH. In a
standard CCH query, **both** the forward and backward elimination tree walks go
all the way **up to the root** — there is no early stopping. Every common
ancestor of the source and target in the elimination tree is a potential meeting
node. The standard query finds the single best meeting node (minimum
`d(s,v) + d(v,t)`). The multi-route extension collects *all* such common
ancestors within a stretch factor and uses each one as a via-vertex to
reconstruct a candidate alternative path.

Observed symptoms:

- Routes with unnecessary U-turns (going forward, turning back, then taking a
  different road).
- Unnecessarily long detours when shorter, more natural alternatives exist.
- Alternative routes that closely follow the optimal path but with bizarre
  deviations at junctions.

Two initial theories were proposed:

1. The unpacking of CCH shortcuts causes some roads to be unpacked
   non-optimally.
2. The default weight (travel time) causes certain turns to behave bizarrely.

**Finding**: Theory 2 is directly confirmed (zero turn costs). Theory 1 is
partially confirmed — not due to unpacking bugs, but due to a more fundamental
issue: via-node paths through non-optimal meeting nodes unpack to paths that are
**correct but not necessarily sensible**, because no admissibility checks
(bounded stretch, local optimality) are applied to subpaths. A third
contributing factor — pure travel-time ranking without geographic awareness —
is also identified.

---

## 2. How the Current Algorithm Works

### The Elimination Tree Walk — Key Mechanics

The CCH elimination tree walk is **not** like bidirectional Dijkstra. Each walk
follows a single, deterministic path from the source (or target) upward through
the elimination tree to the root. There is no frontier, no priority queue, no
stopping criterion. Both walks **always reach the root**.

At each node along the upward path, the walk relaxes edges in the CCH upward
graph — these edges reach higher-rank nodes (other ancestors or nodes on
adjacent branches). This propagates shortest-path distances upward so that every
common ancestor `v` has valid `d(s,v)` and `d(v,t)` values after both walks
complete.

The `reset_distance()` calls in the standard query (`query.rs:77, 82, 102–103`)
are **cleanup for reusability** of the distance array across multiple queries —
they are not correctness-critical for the walk itself. The multi-route walk
correctly omits these resets because it needs distances to persist for path
reconstruction.

### Phase 1 — Collect Meeting Nodes

`multi_route.rs:collect_meeting_nodes()` (lines 148–222) runs both elimination
tree walks to completion and records every common ancestor where both
`fw_dist < INFINITY` and `bw_dist < INFINITY`.

| Aspect | Standard query (`query.rs`) | Multi-route (`multi_route.rs`) |
|--------|---------------------------|-------------------------------|
| Distance reset after settle | **Yes** — cleanup for next query | **No** — distances kept for reconstruction |
| Pruning at meeting nodes | **Yes** — `skip_next()` optimization | **No** — always `next()` to propagate all distances |
| Meeting node tracking | Single best | All common ancestors |
| Walks reach root? | **Yes** | **Yes** |

The difference in pruning (`skip_next` vs always `next`) is an optimization in
the standard query: when a meeting node's tentative distance already exceeds the
best known, there's no point relaxing its edges further. The multi-route walk
must relax everywhere because a non-optimal meeting node still needs correct
distances propagated to its ancestors.

**Parent pointers are correct**: After the walk completes, `fw_parents[v]`
records the shortest upward path from source to `v` for every visited `v`.
Tracing `fw_parents` from any meeting node back to source gives the true
shortest `source → v` path in the upward CCH graph. Similarly for `bw_parents`.

### Phase 2 — Reconstruct & Unpack

For each meeting node, `reconstruct_path()` (lines 230–293) traces `fw_parents`
backward from the meeting node to the source, and `bw_parents` forward from the
meeting node to the target. Each CCH shortcut edge is recursively unpacked via
`unpack_edge_recursive()` (lines 305–324) into original-graph edges.

The unpacking is correct — it mirrors the standard `query.rs::unpack_path()`
logic.

### Phase 3 — Filter

- **Stretch filter**: reject candidates > `1.3×` optimal distance.
- **Diversity filter**: Jaccard edge-set overlap > `0.85` → rejected as too
  similar.
- **Geographic filter** (applied by caller): reject if geographic distance >
  `2.0×` the shortest route's geographic distance.

---

## 3. Problem 1 — Via-Node Alternatives Lack Admissibility Guarantees

**Severity: High** — This is the primary structural cause of bizarre paths.

### The Mechanism

The current approach treats every common ancestor in the elimination tree as a
via-vertex candidate and reconstructs paths through them. The parent pointers
and unpacking are **correct** — each path truly is the shortest `s → v → t`
path through that meeting node `v`. However, **being the shortest path through
`v` does not make it a good alternative**.

The problem is what happens when these paths are unpacked to the original graph.
A via-vertex `v` high in the elimination tree may cause the path to:

1. **Share most of its length with the optimal path**, diverging only briefly
   near `v` before rejoining — producing a route that looks nearly identical
   with a small bizarre detour at one junction.

2. **Include subpaths that are not locally optimal**: the `s → v` segment is
   the shortest path from `s` to `v`, but a subpath `a → b` within it might
   be much longer than `d(a,b)`. This happens because the path is optimized
   end-to-end for reaching `v`, not for every intermediate segment.

3. **Contain U-turns or backtracking**: when unpacked, a path through a
   high-rank separator vertex may route through a junction, overshoot, make a
   U-turn, and come back — because this is genuinely the shortest path to reach
   that particular `v`, even though a human would never drive that way.

The literature (Bacherle, Bläsius, Zündorf — ATMOS 2025, documented in
`docs/walkthrough/separator-based-alternative-paths-cch.md`) identifies three
admissibility criteria that the current implementation does **not** check:

| Criterion | Description | Current status |
|-----------|-------------|----------------|
| **Bounded stretch** | Every deviation subpath `a → v → b` must satisfy `c(a→v→b) ≤ (1+ε) · d(a,b)` | **Not checked** |
| **Limited sharing** | Overlap with shortest path and other alternatives must be ≤ γ · d(s,t) | **Approximated by Jaccard (edge count), not cost-weighted** |
| **Local optimality** | Any subpath ≤ α·d(s,t) must itself be a shortest path (T-test) | **Not checked** |

Without bounded stretch, a path that detours wildly near `v` but is still
globally short enough will pass the stretch filter. Without local optimality,
subpaths with U-turns are tolerated. The Jaccard filter uses unweighted edge
counts, which doesn't capture cost-sharing accurately.

### Why This Produces Bizarre Routes

Consider a query from point A to point B along Duong Lang:

1. The optimal path goes straight along Duong Lang (meeting node `M1`).
2. A meeting node `M2` (a separator vertex higher in the tree) yields a path
   that goes south on Duong Lang, passes through `M2`'s separator region, and
   returns north.
3. The `A → M2 → B` path is the legitimate shortest path *through `M2`*. When
   unpacked, the detour near `M2` may include U-turns or sharp turns because
   that's how the road network connects through the separator region.
4. The path is only ~20% longer (within the 1.3× stretch), and uses enough
   different edges to pass the Jaccard filter (overlap < 0.85).
5. No bounded-stretch or local-optimality check catches the problematic detour
   subpath.

---

## 4. Problem 2 — Zero Turn Costs in the Line Graph

**Severity: High** — Directly enables U-turn abuse.

### The Code

In `generate_line_graph.rs:217–232`:

```rust
let exp_graph = line_graph(&graph, |edge1_idx, edge2_idx| {
    // ... forbidden turn check (return None if forbidden) ...

    if tail[edge1_idx as usize] == graph.head()[edge2_idx as usize] {
        return Some(0); // U-turn penalty: 0 ms (FREE)
    }
    Some(0) // All other turns: also 0 ms (FREE)
});
```

### How Line Graph Weights Work

The `line_graph()` function in `rust_road_router/engine/src/datastr/graph.rs:194`
computes each line-graph edge weight as:

```rust
weight.push(next_link.weight + turn_cost);
//          ^^^^^^^^^^^^^^^^   ^^^^^^^^^
//          travel_time of     turn cost callback
//          the TARGET edge    (always 0 currently)
```

So every line-graph edge weight = `travel_time[target_edge] + 0`. The turn cost
is **uniformly zero** for all non-forbidden turns, including U-turns.

### Why This Is a Problem

- **U-turns are free**: The optimizer sees no difference between going straight
  through an intersection and making a U-turn. A route like `A → B → A → C`
  costs only the travel time of the extra edge traversals, making U-turn-heavy
  alternatives appear nearly competitive with direct routes.

- **No angle-based differentiation**: All turns (slight left, sharp right,
  U-turn) have identical zero cost, so the algorithm has no incentive to prefer
  natural-flowing routes over ones with sharp direction changes.

- **Amplifies Problem 1**: When a via-node path unpacks to a route containing
  a U-turn, zero turn costs mean this path is not penalized relative to cleaner
  alternatives, so it survives the stretch and diversity filters.

---

## 5. Problem 3 — Pure Travel-Time Ranking

**Severity: Moderate** — Contributes to poor alternative selection.

### The Code

In `multi_route.rs:216`:

```rust
meeting_candidates.sort_unstable_by_key(|&(_, dist)| dist);
```

Candidates are ranked purely by CCH travel-time distance. The geographic
distance filter only runs *after* full path reconstruction, in the caller:

```rust
// cch.rs:270–276
if distance_m > base * MAX_GEO_RATIO {
    continue;  // reject detours
}
```

### Why This Is a Problem

Two candidates with nearly identical travel times but vastly different geographic
shapes (one direct, one with a backtrack) are treated as equally good. The
Jaccard diversity filter checks edge-set overlap, but:

- A route that makes a U-turn and returns shares many edges with the direct
  route — it may fail the diversity test and be rejected (good), **or** it may
  use slightly different edges on the detour segment and pass (bad).
- The `MAX_GEO_RATIO = 2.0` filter is very loose — a route twice the
  geographic distance is still accepted, which covers most U-turn detours
  in an urban network.

---

## 6. How the Problems Combine

The three issues create a feedback loop:

```
Problem 1: No admissibility checks on via-node paths
    → Subpaths with detours and U-turns survive if globally short enough
    → Alternatives look like the optimal path with bizarre local deviations

Problem 2: Zero turn costs
    → U-turns and sharp turns have no penalty
    → Via-node paths with U-turns appear cost-competitive

Problem 3: Travel-time-only ranking
    → No geographic sanity check during candidate selection
    → Bizarre routes survive the stretch filter
    → Geographic filter (2×) applied too late and too loosely
```

Example scenario matching the observed screenshots:

1. Optimal path goes straight along Duong Lang.
2. Multi-route collects meeting node `M2` (a separator higher in the tree) with
   a slightly longer via-path distance.
3. The shortest `s → M2 → t` path, when unpacked, routes south on Duong Lang,
   loops through the separator region near Cau Hua Muc, and returns north —
   this is the genuine shortest path through `M2`, but includes a U-turn.
4. Because U-turns cost zero, this detour adds only the raw travel time of the
   extra edges — easily within the 1.3× stretch factor.
5. No bounded-stretch check catches the locally suboptimal detour subpath.
6. The geographic distance filter (2×) doesn't catch it because the total
   geographic length is still under the threshold.

---

## 7. Proposed Fixes

### Fix 1 — Add Admissibility Checks (Fixes Problem 1)

**Zero changes to `rust_road_router`.** Enhances the existing via-node approach
with the admissibility pipeline from the SeArCCH literature.

After collecting meeting-node candidates and reconstructing paths, apply three
checks before accepting each candidate:

#### 1a. Bounded Stretch Check

For each candidate path through via-vertex `v`, find the deviation points `a`
(where it diverges from the shortest path) and `b` (where it rejoins). Verify:

```
c(a → v → b) ≤ (1 + ε) · d(a, b)    where ε = 0.25
```

This requires one additional CCH query per candidate (for `d(a, b)`), but
rejects paths with locally suboptimal detour segments — the primary source of
bizarre turns.

**Efficient implementation**: Find `a` and `b` by comparing the via-path with
the shortest path in the shortcut graph before full unpacking. Only the
deviating edge needs recursive unpacking to locate `a` and `b`.

#### 1b. Local Optimality (T-test)

On the unpacked via-path, find vertices `a'` and `b'` at distance `α · d(a,b)`
from `v`. Verify that `c(a' → v → b')` equals `d(a', b')` via a CCH query.
Use `α = 0.25`.

This catches paths where short subpaths near the via-vertex are not locally
shortest — which is exactly the scenario that produces U-turns and unnecessary
zigzags.

#### 1c. Cost-Weighted Sharing Check

Replace the Jaccard edge-count overlap with cost-weighted sharing:

```
c(P_via ∩ P_shortest) ≤ γ · d(s, t)    where γ = 0.8
```

This ensures alternatives are meaningfully different in terms of actual travel
cost, not just edge count.

**Implementation location**: Add checks after `reconstruct_path()` in
`multi_route.rs`, before the Jaccard diversity filter.

### Fix 1-ALT — Penalty-Based K-Shortest Paths (Simpler alternative)

If implementing the full admissibility pipeline is too complex, a simpler
approach that avoids the problem entirely:

```
1. Query optimal path P₁ using standard CCH query.
2. For k = 2..K:
   a. Create penalty weights: multiply travel_time by 2× for edges in
      previously accepted paths.
   b. Re-customize CCH: customize_with(penalty_weights).
   c. Query for shortest path under penalty metric.
   d. Accept if diverse enough (Jaccard check with original edge sets).
3. Return all accepted paths with ORIGINAL (non-penalized) distances.
```

This produces clean, independently-computed paths. CCH customization is fast
(~100–300 ms for Hanoi), so running it K times is practical.

### Fix 2 — Add Turn Penalties (Fixes Problem 2)

**One-file change in `generate_line_graph.rs`. Requires re-running the line graph
generation pipeline.**

Change the turn cost callback:

```rust
let exp_graph = line_graph(&graph, |edge1_idx, edge2_idx| {
    // ... forbidden turn check ...

    // Forbid U-turns (or apply heavy penalty)
    if tail[edge1_idx as usize] == graph.head()[edge2_idx as usize] {
        return None; // FORBIDDEN
        // Or: return Some(30_000); // 30-second penalty
    }

    // Optional: angle-based penalty for sharp turns
    // let angle = compute_turn_angle(edge1_idx, edge2_idx, &lat, &lng, &tail, &head);
    // Some(angle_penalty(angle))

    Some(0) // straight-through and gentle turns: no penalty
});
```

**Minimum viable change** (just forbid U-turns):

```rust
if tail[edge1_idx as usize] == graph.head()[edge2_idx as usize] {
    return None; // was: Some(0)
}
```

**Full version** (angle-based penalties — the infrastructure exists in
`hanoi-core/src/geometry.rs` which already computes turn directions):

| Turn type | Angle range | Suggested penalty |
|-----------|-------------|-------------------|
| U-turn | > 160° | Forbidden (`None`) or 30 000 ms |
| Sharp turn | 120°–160° | 10 000 ms (10 s) |
| Moderate turn | 60°–120° | 5 000 ms (5 s) |
| Slight turn | 30°–60° | 2 000 ms (2 s) |
| Straight | < 30° | 0 ms |

### Fix 3 — Hybrid Candidate Scoring (Fixes Problem 3)

**Change in `cch.rs` and `line_graph.rs` multi-query wrappers.**

After reconstructing each candidate path and computing its geographic distance,
apply a composite score instead of using raw travel time for ordering:

```rust
let direct_dist = haversine_m(from_lat, from_lng, to_lat, to_lng);
let detour_ratio = distance_m / direct_dist;
let score = distance_ms as f64 * detour_ratio.sqrt();
```

Sort candidates by this composite score. Routes that are fast but meander
geographically get a higher (worse) score than routes that are equally fast but
more direct.

Also tighten `MAX_GEO_RATIO` from `2.0` to `1.5` — a route 50% longer in
geographic distance is already a substantial detour for an urban network.

---

## 8. Fix Priority Matrix

| Priority | Fix | Effort | Impact | Changes to `rust_road_router` |
|----------|-----|--------|--------|-------------------------------|
| **1** | Fix 2: U-turn / turn penalties | Small | Prevents U-turn abuse in all queries | None |
| **2** | Fix 1: Admissibility checks (or 1-ALT penalty K-paths) | Medium–Large | Eliminates locally-suboptimal detours | None |
| **3** | Fix 3: Hybrid scoring | Small | Better alternative quality ranking | None |

All fixes are confined to `CCH-Hanoi` code and the pipeline
(`generate_line_graph`). No modifications to `rust_road_router` or `RoutingKit`
are required.

Fix 2 is the simplest to implement and test (single callback change + pipeline
re-run) and has immediate impact on all queries, not just alternatives. Fix 1
addresses the root structural issue but requires more implementation effort. Fix
1-ALT (penalty-based) is a simpler alternative to full admissibility that still
eliminates the problem. Fix 3 is a refinement best applied alongside Fix 1 or
1-ALT.
