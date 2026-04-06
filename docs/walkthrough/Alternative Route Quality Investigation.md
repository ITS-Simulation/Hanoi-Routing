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
8b. [Cross-Reference: kientx Branch vs dev-haihm](#8b-cross-reference-kientx-branch-vs-current-dev-haihm)
9. [Concurrency Architecture Analysis](#9-concurrency-architecture-analysis)
10. [Penalty-Based K-Shortest Paths — Concurrency Flow](#10-penalty-based-k-shortest-paths--concurrency-flow)
11. [SeArCCH Feasibility in Current Architecture](#11-searcch-separator-based-alternative-paths--feasibility-in-current-architecture)
12. [Solution Comparison: Penalty-Based vs SeArCCH](#12-solution-comparison-penalty-based-vs-searcch)

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

---

## 8b. Cross-Reference: `kientx` Branch vs Current `dev-haihm`

> Added 2026-04-06. The `kientx` branch (commit `8f02165`) contains the original
> multi-route implementation. This section verifies the three problems against
> that code and assesses portability to `dev-haihm`.

### All Three Problems Are Confirmed in kientx

| Problem | kientx evidence | Status on dev-haihm |
|---------|----------------|---------------------|
| **P1: No admissibility** | `multi_route.rs:216` — sort by raw distance only. No bounded stretch, no T-test, Jaccard is unweighted `HashSet<(NodeId, NodeId)>`. Geo filter `MAX_GEO_RATIO=2.0` applied late. | Same — `multi_route.rs` not present on dev-haihm (removed/never merged) |
| **P2: Zero turn costs** | `generate_line_graph.rs:229` — U-turns `Some(0)`, all turns `Some(0)` | **Partially fixed**: U-turns now `Some(20_000)` (20s). Non-U-turns still `Some(0)`. User reports accurate angle-based turn costs further improve quality. |
| **P3: Travel-time ranking** | `multi_route.rs:216` — `sort_unstable_by_key(…dist)` | Same — no composite scoring exists |

### Impact of Turn Cost Fixes on Problem Severity

With accurate turn costs (as the user confirms are now producing more reasonable
paths), Problem 2 is substantially mitigated. This also indirectly reduces
Problem 1's severity: via-node paths containing U-turns or sharp turns now carry
real cost, making them less likely to survive the stretch filter. Problem 3
remains unaffected — ranking is still pure travel-time.

**Bottom line:** Accurate turn costs buy significant quality improvement even
without admissibility checks. The via-node approach becomes viable for most
urban queries once the cost model is realistic.

### Structural Differences: What kientx Has That dev-haihm Doesn't

| Component | kientx | dev-haihm |
|-----------|--------|-----------|
| `multi_route.rs` | Full via-node K-alternative engine (339 lines) | **Missing** |
| `cch.rs` multi_query methods | `multi_query()`, `multi_query_coords()` on `QueryEngine` | **Missing** |
| `line_graph.rs` multi_query methods | `multi_query()`, `multi_query_coords()`, `build_answer_from_lg_path()` on `LineGraphQueryEngine` | **Missing** |
| `engine.rs` alternatives dispatch | `dispatch_normal/line_graph` accept `alternatives`+`stretch` | **Missing** |
| `state.rs` QueryMsg fields | `alternatives: u32`, `stretch: f64` | **Missing** |
| `types.rs` FormatParam fields | `alternatives: Option<u32>`, `stretch: Option<f64>` | **Missing** |
| `handlers.rs` wiring | Passes `alternatives`+`stretch` from URL params to `QueryMsg` | **Missing** |
| `engine.rs` multi-response | `format_multi_response()`, `answers_to_geojson()`, color palette | **Missing** |

### Structural Differences: What dev-haihm Has That kientx Doesn't

| Component | dev-haihm | kientx |
|-----------|-----------|--------|
| `QueryAnswer.route_arc_ids` | Per-path arc IDs for traffic overlay replay | **Missing** |
| `QueryAnswer.weight_path_ids` | LG node path for weight-space replay | **Missing** |
| `line_graph.rs` `original_arc_id_of_lg_node` | Maps LG nodes → original arcs (for split nodes) | **Missing** |
| `line_graph.rs` `is_arc_roundabout` | Per-arc roundabout flag | **Missing** |
| `geometry.rs` `refine_turns()` | Post-processing turn annotations | **Missing** (only `compute_turns()`) |
| `camera_overlay.rs` | Camera location overlay endpoint | **Missing** |
| `traffic.rs` | Traffic overlay endpoint | **Missing** |
| `route_eval.rs` | Route replay/evaluation from GeoJSON | **Missing** |
| `ui.rs` | Static UI serving | **Missing** |
| `static/` (app.js, index.html, styles.css) | Full map UI | **Missing** |
| `generate_line_graph.rs` turn costs | U-turn = 20s penalty | U-turn = 0ms (free) |

### Can kientx Multi-Route Logic Be Ported to dev-haihm?

**Yes**, with adaptation. The core `multi_route.rs` is self-contained and
depends only on `rust_road_router` public API. The integration points need
updates:

1. **`multi_route.rs`** — drop in as-is. No changes needed.

2. **`cch.rs` multi_query methods** — must add `route_arc_ids` and
   `weight_path_ids` to `QueryAnswer` construction. kientx builds
   `QueryAnswer` without them. Fix: call `reconstruct_arc_ids()` on each
   alternative's node path (method already exists on `QueryEngine`).

3. **`line_graph.rs` multi_query / build_answer_from_lg_path** — must add
   `route_arc_ids` (map via `original_arc_id_of_lg_node`), `weight_path_ids`
   (the LG node path), `is_arc_roundabout` param to `compute_turns`, and
   `refine_turns()` call. The `build_answer_from_lg_path` helper from kientx
   needs updating to match dev-haihm's richer `QueryAnswer` and turn pipeline.

4. **Server integration** — add `alternatives`+`stretch` to `QueryMsg`,
   `FormatParam`, `handlers.rs`. Add `format_multi_response` and
   `answers_to_geojson` to `engine.rs`. These are additive — no conflicts
   with existing dev-haihm server features.

---

## 9. Concurrency Architecture Analysis

### 9.1 Upstream rust_road_router Server — Single-Threaded Query Loop

The Rocket server in `rust_road_router/server/src/main.rs` uses a
**channel-per-request, single-consumer** architecture:

```
Rocket HTTP threads ──→ Mutex<Sender<Request>> ──→ mpsc channel ──→ single background thread
                                                                      │
                                                                      ├── Query: lock Arc<Mutex<Server>>, run, send result back
                                                                      └── Customize: spawn scoped thread, clone travel_time,
                                                                          re-customize, lock Server, swap weights
```

**Key objects:**

| Object | Type | Sharing | Role |
|--------|------|---------|------|
| `Server<CustomizedBasic>` | `Arc<Mutex<_>>` | Shared via Arc+Mutex | CCH query engine (owns fw/bw distances, parents) |
| `tx_query` | `Mutex<Sender<Request>>` | Rocket `State` | All HTTP threads send requests through this |
| `rx_query` | `Receiver<Request>` | Owned by background thread | Single consumer processes queries sequentially |
| `cch` | `CCH` | Borrowed (`&`) within `crossbeam::scope` | Immutable CCH topology |
| `travel_time` | `Vec<Weight>` | Cloned per customization | Mutable weights — each customize gets a fresh copy |

**Concurrency model:** All queries are **serialized** through one background
thread. The `Arc<Mutex<Server>>` lock is held for the duration of each query.
Customization runs on a separate `crossbeam` scoped thread, acquires the same
mutex to swap in the new `CustomizedBasic`, then releases. During customization,
queries block on the mutex.

### 9.2 CCH-Hanoi — hanoi-server (Axum, Sequential Background Engine)

`hanoi-server` uses **Axum 0.8** with a single background engine thread,
communicating via `tokio::sync::mpsc` (queries) and `tokio::sync::watch`
(weight updates).

```
Axum HTTP handlers ──→ mpsc::Sender<QueryMsg> ──→ background engine thread
                                                      │
                                                      ├── recv query → dispatch_normal/line_graph → reply via oneshot
                                                      └── watch_rx.has_changed()? → engine.update_weights()
                                                          (checked every 50ms timeout loop)
```

**No mutex on queries.** The engine thread processes queries sequentially — no
lock contention. The `watch` channel for customization is checked between
queries (non-blocking).

**Key objects:**

| Object | Type | Role |
|--------|------|------|
| `AppState` | `#[derive(Clone)]`, injected into Axum handlers | Shared state: `mpsc::Sender`, `watch::Sender`, `Arc<AtomicBool>` flags |
| `QueryMsg` | struct with `QueryRequest` + `oneshot::Sender` | Per-request message to engine thread |
| `CchContext` | Owns `GraphData`, `CCH`, `baseline_weights` | Immutable after construction. Metric-independent. |
| `QueryEngine<'a>` | Borrows `&'a CchContext`, owns `CchQueryServer` | Per-engine mutable state (distances, parents inside CchQueryServer) |
| `LineGraphCchContext` | Owns `GraphData`, `DirectedCCH`, reconstruction arrays | Immutable after construction |
| `LineGraphQueryEngine<'a>` | Borrows `&'a LineGraphCchContext`, owns `CchQueryServer` | Per-engine mutable state |
| `CchQueryServer` (aliased from upstream `query::Server`) | Owns `fw_distances`, `bw_distances`, `fw_parents`, `bw_parents` | Mutable scratch space — reset each query |

Source files:
- `CCH-Hanoi/crates/hanoi-server/src/state.rs` — `AppState`, `QueryMsg`
- `CCH-Hanoi/crates/hanoi-server/src/engine.rs` — `run_normal()`, `run_line_graph()`
- `CCH-Hanoi/crates/hanoi-server/src/main.rs` — Axum setup, dual-port listeners

### 9.3 The CCH Query Server's Internal State

`rust_road_router/engine/src/algo/customizable_contraction_hierarchy/query.rs`
defines `Server<Customized>` with these per-query mutable fields:

- `fw_distances: Vec<Weight>` — forward distances (reset to INFINITY after each
  node via `reset_distance()`)
- `bw_distances: Vec<Weight>` — backward distances (same)
- `fw_parents: Vec<(NodeId, EdgeId)>` — forward parent pointers
- `bw_parents: Vec<(NodeId, EdgeId)>` — backward parent pointers
- `meeting_node: NodeId` — single best meeting node
- `customized: C` — the customized CCH data (forward/backward shortcut weights
  + unpacking info)

The `query()` method takes `&mut self` — it is inherently **not thread-safe**.
Each concurrent query needs its own `Server<Customized>` instance, or
serialization via mutex.

After a standard query, **distances are already reset** — common ancestors are
not retained. Only the single best `meeting_node` survives.

---

## 10. Penalty-Based K-Shortest Paths — Concurrency Flow

### 10.1 How It Would Work

```
Client request (s, t, k=3)
│
├── Query 1: standard CCH query → P₁ (shortest path)
│   └── Collect edge set E₁
│
├── Query 2: penalize E₁ → re-customize → query → P₂
│   └── penalty_weights[e] = 2× for e ∈ E₁
│   └── customize_with(penalty_weights) → new CustomizedBasic
│   └── Create temporary Server with new customization → query
│   └── Collect edge set E₂
│
├── Query 3: penalize E₁ ∪ E₂ → re-customize → query → P₃
│   └── ...
│
└── Return [P₁, P₂, P₃] with ORIGINAL distances
```

### 10.2 Key Rust Objects Involved

**From hanoi-core (existing):**

- `CchContext::customize_with(&self, weights: &[Weight])` — already exists,
  creates `CustomizedBasic` from arbitrary weights. **This is the re-customize
  entry point.**
- `QueryEngine::update_weights(&mut self, weights: &[Weight])` — already exists,
  calls `customize_with` + `server.update()`. Swaps customization in-place.
- `LineGraphCchContext::customize_with(...)` — same for line graphs.
- `LineGraphQueryEngine::update_weights(...)` — same.

**From rust_road_router (upstream, read-only):**

- `customize(&CCH, &metric)` / `customize_directed(&DirectedCCH, &metric)` —
  Phase 2 functions.
- `Server::new(customized)` — creates a fresh query server from a customization.
- `Server::update(&mut self, customized)` — swaps in new customization, reuses
  scratch buffers.
- `Server::query(&mut self, Query)` — runs the bidirectional elimination tree
  walk.

**Existing upstream penalty code (read-only reference):**

- `rust_road_router/engine/src/algo/ch_potentials/penalty.rs` — `Penalty<P>` and
  `PenaltyIterative<'a>` structs. These use bidirectional A* with CH potentials
  and an edge-penalty tracking vector (`times_penalized: Vec<u8>`). **Not CCH-based**
  — uses Dijkstra on the topocore graph, not the elimination tree. Not integrated
  with the CCH server. Useful as a design reference but not directly reusable for
  our CCH pipeline.

**New objects needed (in CCH-Hanoi):**

- `penalty_weights: Vec<Weight>` — per-query temporary weight vector (clone of
  `baseline_weights` with multiplied edges). Allocated per k-alternative
  iteration.
- No new upstream types needed.

### 10.3 Concurrency for Multi-Client Serving

**hanoi-server already uses the sequential background engine pattern.** The
penalty-based k-paths flow fits naturally into `engine.rs::run_normal()` and
`run_line_graph()` — the engine thread already owns a `&mut QueryEngine` and
calls `engine.update_weights()` for customization.

**Integration path for penalty k-paths in the existing architecture:**

In the background engine loop (`engine.rs`), when a k-alternative query arrives:
1. Uses its `QueryEngine` for query 1.
2. Clones `baseline_weights` (from `CchContext`), applies penalties.
3. Calls `engine.update_weights(&penalty_weights)` — re-customizes in-place.
4. Queries again. Repeat for k iterations.
5. Calls `engine.update_weights(&baseline_weights)` to restore.

**Cost:** ~100–300ms per customization × (k−1) iterations. For k=3: ~200–600ms
total penalty overhead on Hanoi-scale graphs.

**Caveat:** While the engine is running a k-alternative query (300–900ms), all
other queued requests wait. For a single-client scenario this is fine. For
multi-client, consider:

**Option A — Sequential (current model, simplest):** Accept the 300–900ms
blocking. For Hanoi deployment with low QPS this is adequate.

**Option B — Engine pool (parallel clients):** Spawn multiple background engine
threads, each with its own `QueryEngine`. Route requests round-robin. The
`CchContext` is immutable and can be shared via `Arc`.

| Primitive | Use case |
|-----------|----------|
| `Arc<CchContext>` | Share immutable CCH topology + graph across engine threads |
| Multiple `mpsc` channels | One per engine thread |
| `tokio::task::spawn_blocking` | Offload CPU-heavy penalty loops from async runtime |

---

## 11. SeArCCH (Separator-Based Alternative Paths) — Feasibility in Current Architecture

### 11.1 What Would Need to Change in rust_road_router

The SeArCCH approach (§4 of the walkthrough doc) requires access to the
**intermediate state** of the CCH query — specifically the forward and backward
distances at all common ancestors **before they are reset**.

Currently, `query.rs:77,82,102-103` calls `reset_distance()` on every visited
node immediately after processing. The distances are gone by the time `query()`
returns.

**Minimal upstream changes needed:**

1. **Expose elimination tree walk distances before reset.** Two sub-options:

   - **(a) New `query_alternatives()` method** on `Server<C>` that runs the same
     bidirectional walk but **skips `reset_distance()`** and returns a
     `Vec<(NodeId, Weight, Weight)>` of `(common_ancestor, d(s,v), d(v,t))`
     tuples. This is essentially what the existing multi-route code in the
     investigation doc's §2 already does — but it needs to be done inside
     `Server<C>` where the distances live.

   - **(b) Expose `fw_distances` and `bw_distances` as public** (or
     provide accessors). Let the caller read distances before they're reset.
     Requires the caller to also trigger reset manually afterward.

2. **Expose shortcut unpacking for partial path comparison.** The `unpack_path()`
   method is already on `Server<C>` but is private. The `Customized` trait
   already exposes `unpack_outgoing()` / `unpack_incoming()`. Deviation-point
   finding (§5.1 of the walkthrough) can be done externally using the
   `Customized` trait methods.

3. **No structural changes to CCH topology, customization, or the elimination
   tree.** The elimination tree is already accessible via `cch.elimination_tree()`.
   Node order conversions via `cch.node_order().rank()` / `.node()` are public.

**Summary: 1 new method or 2 accessor methods on `Server<C>`. No changes to
CCH, Customized, EliminationTreeWalk, or any data structure.**

### 11.2 What Would Be Built in CCH-Hanoi (No Upstream Changes)

All admissibility logic lives in CCH-Hanoi:

- **Common ancestor collection** — iterate elimination tree parent pointers,
  intersect forward/backward visited sets.
- **Four-check pipeline** — bounded stretch, limited sharing, T-test, total
  stretch pruning.
- **Recursive decomposition** (two-step / recursive variants).
- **Edge marking infrastructure** — bitset over original edges for sharing checks.

### 11.3 Elimination Tree Accessibility

The elimination tree is **fully accessible** via:
```
cch.elimination_tree()  →  &[InRangeOption<NodeId>]
```
Each entry maps a node (by rank) to its parent in the elimination tree.
`InRangeOption<NodeId>` is `None` for the root. Walking from any node to root:
```rust
let mut cur = Some(rank);
while let Some(node) = cur {
    cur = cch.elimination_tree()[node as usize].value();
}
```

Common ancestors of `s` and `t` = intersection of their root-paths. This is
already how the multi-route code works conceptually.

---

## 12. Solution Comparison: Penalty-Based vs SeArCCH

### 12.1 Performance

| Metric | Penalty-Based K-Paths | SeArCCH (Recursive, µ=0.3) |
|--------|----------------------|---------------------------|
| **Base query** | ~0.3ms (standard CCH) | ~0.3ms (standard CCH) |
| **Per-alternative cost** | ~100–300ms (re-customization) + ~0.3ms (query) | ~2ms (extra CCH queries for T-test + deviation check) |
| **Total for k=3** | ~300–900ms | ~6–10ms |
| **Latency class** | Sub-second | Real-time (<50ms) |
| **Scales with graph size** | Customization is O(m·log n) — grows with graph | Extra queries are O(n^0.5) — barely grows |

**Verdict:** SeArCCH is **50–100× faster** for k-alternative generation. Penalty
re-customization is the bottleneck — it's designed for weight updates, not
per-query use.

### 12.2 Feasibility

| Aspect | Penalty-Based | SeArCCH |
|--------|--------------|---------|
| **Changes to rust_road_router** | **None** — uses existing `customize_with()` + `query()` | **Minimal** — 1 new method or 2 accessors on `Server<C>` |
| **Changes to CCH-Hanoi** | Small — penalty weight logic + loop | Medium — admissibility pipeline, edge marking, recursive decomposition |
| **Data requirements** | Same graph data, no extras | Same graph data, no extras |
| **Risk of breaking upstream** | Zero | Very low (additive-only change) |

### 12.3 Development Effort & Deployment

| Aspect | Penalty-Based | SeArCCH |
|--------|--------------|---------|
| **Dev effort** | ~1–2 days | ~5–10 days |
| **Testing complexity** | Simple (correctness = "is the penalty applied?") | Moderate (admissibility criteria have edge cases) |
| **Deployment** | Drop-in — no pipeline changes, no data regeneration | Drop-in — no pipeline changes, no data regeneration |
| **Incremental delivery** | Full solution in one PR | Can ship basic variant first (65% success), then two-step, then recursive |

### 12.4 Maintenance

| Aspect | Penalty-Based | SeArCCH |
|--------|--------------|---------|
| **Code surface** | ~50 lines in CCH-Hanoi | ~300–500 lines in CCH-Hanoi + ~20 lines upstream |
| **Coupling to upstream** | Uses only public API | Requires 1 new upstream method (or pub accessors) |
| **Parameter tuning** | Penalty multiplier (2×) — one knob | γ, ε, α, µ — four knobs (but literature defaults work well) |
| **Future-proofing** | Limited — fundamentally a heuristic | Strong — same approach used by KIT's production systems |

### 12.5 Quality of Alternatives

| Aspect | Penalty-Based | SeArCCH |
|--------|--------------|---------|
| **Admissibility guarantees** | **None** — penalized paths may still share heavily or contain locally suboptimal segments | **Full** — bounded stretch, limited sharing, local optimality (T-test) |
| **U-turn handling** | Indirectly mitigated by penalties on shared edges | Explicitly caught by bounded stretch + T-test |
| **Success rate (1st alt)** | ~70–80% (empirical, no literature benchmark) | 90% (published benchmark on DIMACS Europe) |
| **Success rate (3rd alt)** | Unknown | 44.7% (published) |
| **Determinism** | Order-dependent — path k depends on paths 1..k−1 | Greedy but near-optimal (validated in paper §5.5) |

### 12.6 Logic Core Comparison

**Penalty-Based:**
```
for k in 1..K:
    weights = baseline.clone()
    for prev_path in accepted:
        for edge in prev_path:
            weights[edge] *= 2
    customize(weights)  ← expensive
    path = query(s, t)  ← cheap
    if diverse(path, accepted):
        accepted.push(path)
```
Core logic: weight manipulation → re-customize → query. Simple loop.

**SeArCCH:**
```
(d_s, d_t, common_ancestors) = cch_query_with_distances(s, t)
candidates = sort(common_ancestors, by: d_s[v] + d_t[v])
for v in candidates:
    if d_s[v] + d_t[v] > (1+ε) · d(s,t): break        ← total stretch
    (a, b) = find_deviation_points(v, shortest_path)     ← partial unpack
    if c(a→v→b) > (1+ε) · query(a,b): continue          ← bounded stretch
    if sharing(path_via_v, marked_edges) > γ·d: continue ← limited sharing
    if !t_test(v, a, b, path_via_v): continue            ← local optimality
    accept(v); mark_edges(path_via_v)
```
Core logic: candidate filtering through 4 checks. More complex but more
principled.

### 12.7 Execution Flow Comparison

```
                    Penalty-Based                    SeArCCH
                    ─────────────                    ───────
Request arrives     │                                │
                    ▼                                ▼
              CCH query (P₁)                   CCH query (modified: keep distances)
                    │                                │
                    ▼                                ▼
              Clone weights                    Collect common ancestors A
              Apply 2× penalty                 Sort by via-path length
                    │                                │
                    ▼                                ▼
              Re-customize (100-300ms)         For each v ∈ A:
                    │                            4-check pipeline (~0.5ms each)
                    ▼                            Extra CCH query for T-test
              CCH query (P₂)                        │
                    │                                ▼
                    ▼                          Recurse if needed (two-step)
              Diversity check                       │
                    │                                ▼
              (repeat for P₃...)               Return accepted alternatives
                    │                                │
                    ▼                                ▼
              Restore weights                  Done — no weight restoration needed
              Return results                   Return results
                    │                                │
              Total: 300-900ms                 Total: 5-15ms
```

### 12.8 Recommendation

**For immediate deployment: Penalty-Based** — zero upstream changes, trivial to
implement, good enough for most use cases. Ship it alongside Fix 2 (turn
penalties) for immediate quality improvement.

**For production quality: SeArCCH** — superior in every dimension except
development effort. The ~20-line upstream change is minimal and additive. The
50–100× performance advantage makes it viable for real-time routing with
multiple concurrent clients. Plan this as a follow-up after the penalty-based
MVP proves the value of k-alternatives to stakeholders.

**Phased approach:**
1. **Now:** Fix 2 (turn penalties) + Fix 1-ALT (penalty-based k-paths) — 2–3 days
2. **Next sprint:** SeArCCH basic variant (65% success, ~0.5ms) — 3–5 days
3. **Following sprint:** SeArCCH recursive (90% success, ~2ms) — 3–5 days
