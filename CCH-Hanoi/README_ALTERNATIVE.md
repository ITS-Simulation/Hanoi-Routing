# K-Alternative Routes — Implementation Walkthrough

How `alternative.rs` (in `rust_road_router/engine`) produces K alternative
shortest paths on top of the existing CCH infrastructure, integrated into
CCH-Hanoi via a thin re-export shim in `hanoi-core/src/multi_route.rs`.

---

## 1. Theoretical Foundation

Based on the separator-based alternative paths approach (Bacherle, Bläsius,
Zündorf — ATMOS 2025). Core insight: in a CCH, the elimination tree encodes
balanced separators. For any query (s, t), the **common ancestors** of s and t
in the elimination tree form a natural set of via-vertex candidates — and the
standard CCH query already computes `d(s,v)` and `d(v,t)` for every such vertex
at zero additional cost.

Each candidate via-vertex yields a path `s → v → t` that is reconstructed and
checked against admissibility criteria to produce diverse, high-quality
alternatives.

---

## 2. Algorithm Core: `AlternativeServer`

**File:** `rust_road_router/engine/src/algo/customizable_contraction_hierarchy/query/alternative.rs`

### 2.1 Data Structure

```
AlternativeServer<'a, C: Customized>
├── customized: &C           // borrowed CCH customization (forward/backward graphs)
├── fw_distances: Vec<Weight> // forward walk distances (rank-indexed)
├── bw_distances: Vec<Weight> // backward walk distances (rank-indexed)
├── fw_parents: Vec<(NodeId, EdgeId)>  // forward parent pointers for path reconstruction
├── bw_parents: Vec<(NodeId, EdgeId)>  // backward parent pointers
├── ttest_fw_dist: TimestampedVector   // independent scratch for T-test distance queries
├── ttest_bw_dist: TimestampedVector   // (avoids clobbering main walk parents)
├── ttest_fw_par, ttest_bw_par         // T-test parent pointers
```

Key design choice: the T-test requires point-to-point CCH queries between
subpath endpoints. These use **separate scratch arrays**
(`ttest_fw_dist`/`ttest_bw_dist`) so they don't overwrite the main
`fw_parents`/`bw_parents` that are still needed for path reconstruction across
all candidates.

`TimestampedVector` provides O(1) amortized reset between successive T-test
queries — a timestamp increment logically clears the vector without touching
memory.

### 2.2 Phase 1 — Bidirectional Elimination Tree Walk

**Method:** `collect_meeting_nodes(from_rank, to_rank)`

Reimplements `rust_road_router`'s elimination tree walk with one critical
difference: the original `query.rs` prunes via `skip_next()` at meeting nodes
(since it only needs the single optimal meeting node). `multi_route.rs`
**always calls `next()`** to relax edges at every meeting node, ensuring correct
distances propagate to ancestor nodes that may serve as alternative via-vertices.

```
Walk logic (simplified):
  fw_walk starts at from_rank, walks up elimination tree
  bw_walk starts at to_rank, walks up elimination tree

  At each step, advance whichever walk has the smaller current node.
  When both walks reach the same node (a meeting node):
    → always relax edges (call next() on both)
    → record (node, fw_dist + bw_dist) as a candidate
    → update tentative_distance if this is a new best

  After walk completes:
    → sort candidates by total distance (ascending)
    → deduplicate by node ID
```

**Output:** `Vec<(meeting_node_rank, total_distance)>` sorted ascending by
distance. The first entry is the shortest path's meeting node.

**Side effect:** `fw_parents` and `bw_parents` are populated with parent
pointers in rank space — needed by Phase 2.

### 2.3 Phase 2 — Path Reconstruction with Edge Costs

**Method:** `reconstruct_path_with_costs(from_rank, to_rank, meeting_node)`

Unlike the original `query.rs` which reverses forward pointers into
`bw_parents` (destructive — only works for one path), `alternative.rs` traces
forward and backward halves independently using **read-only** access to the
parent arrays. This allows reconstructing paths for multiple meeting nodes from
the same walk data.

```
Forward half:   meeting_node → ... → from  (via fw_parents, then reversed)
Backward half:  meeting_node → ... → to    (via bw_parents, naturally ordered)
```

**Critical addition**: edge costs are collected during unpacking via
`unpack_edge_with_costs`. Each base-graph edge yields its weight from the
customized forward/backward graph. This produces `Vec<Weight>` of per-edge
costs alongside the path — essential for the cost-based sharing filter,
bounded stretch test, and T-test.

Each edge along the traced path is recursively unpacked via
`unpack_edge_with_costs`, which mirrors the CCH contraction structure:
- If `tail < head` → `customized.unpack_outgoing(edge)` → returns `(down_edge, up_edge, middle_node)` or None
- If `tail > head` → `customized.unpack_incoming(edge)`
- `None` = base-graph edge, recursion bottoms out; push the edge weight

**Output**: `(Vec<NodeId>, Vec<Weight>)` — unpacked path in original node IDs
plus per-edge costs. Invariant: `path.len() == edge_costs.len() + 1`.

Final step: convert all rank-space node IDs back to original graph node IDs via
`order.node(rank)`.

### 2.4 Phase 3 — Admissibility Filtering

Each candidate is evaluated by `evaluate_candidate()`, which runs five checks
in order. Rejection is fast — the first failing check short-circuits.

**Check 1 — Loop Detection:**
`has_repeated_nodes(&path)` — reject paths that visit any node twice (U-turn
loops from CCH shortcut structure).

**Check 2 — Geographic Stretch:**
The stretch is evaluated on **geographic distance** (meters, via Haversine)
rather than travel-time cost. The caller supplies a `path_geo_len` closure.

```
geo_stretch_limit = path_geo_len(main_path) × stretch_factor
reject if path_geo_len(candidate) > geo_stretch_limit
```

`DEFAULT_STRETCH = 1.25` (25% geographic detour allowed).

**Check 3 — Bounded Stretch at Deviation Points:**
Finds where the candidate first diverges from the reference path (point A)
and where it rejoins (point B), then verifies the detour segment is
near-optimal:

```
1. find_deviation_points(candidate, reference):
   → walk forward: find first index where candidate[i+1] ≠ reference[i+1] → a_pos
   → walk backward: find last index where candidate re-joins reference → b_pos
   → compute cost_s_a (shared prefix cost) and cost_b_t (shared suffix cost)

2. detour_cost = total_candidate_cost - cost_s_a - cost_b_t
3. exact_ab = cch_point_distance(A, B)   // exact shortest path A→B
4. reject if detour_cost > exact_ab × (1 + BOUNDED_STRETCH_EPS)
   where BOUNDED_STRETCH_EPS = 0.4
```

This catches routes that take needless detours between deviation/rejoin points
even when the global stretch looks acceptable.

**Check 4 — Limited Sharing (pairwise, cost-based):**
Builds a cost edge set `{(tail, head) → cost}` for the candidate and computes
cost overlap against **every** already-accepted route:

```
shared_cost = Σ min(candidate_cost[e], accepted_cost[e]) for shared edges e
reject if shared_cost > SHARING_THRESHOLD × best_distance
where SHARING_THRESHOLD = 0.80
```

This ensures pairwise diversity: each new route must contribute at least 20%
unique cost relative to every previously accepted route. Using cost-weighted
sharing (not just edge counts) prevents a few expensive shared edges from
dominating.

**Check 5 — Local Optimality (T-test):**
Verifies that the subpath around the via-vertex is approximately a shortest
path. This catches deceptive routes that appear diverse globally but contain
local U-turn detours.

```
1. Locate via-vertex (meeting node) position in the unpacked path
2. Build cumulative edge-cost prefix sums from edge_costs
3. Walk ±T from via-vertex (T = LOCAL_OPT_T_FRACTION × best_distance, fraction = 0.4)
   to find interval endpoints v' and v''
4. Compute subpath_cost = cum_cost[v''] - cum_cost[v']
5. Run CCH distance query d(v', v'') using cch_point_distance()
6. Accept iff subpath_cost ≤ (1 + LOCAL_OPT_EPSILON) × d(v', v'')
   where LOCAL_OPT_EPSILON = 0.1
```

If the T-interval spans the entire path, the stretch filter already handles
the global case — the T-test is skipped.

The `cch_point_distance` method runs a fresh bidirectional elimination tree walk
using the dedicated `ttest_*` scratch arrays, returning the exact shortest
distance without disturbing the main walk's parent pointers.

### 2.5 Phase 4 — Recursive Decomposition

After the basic selection phase produces initial alternatives, the algorithm
attempts to find more diverse routes by recursively decomposing the main path
around its highest-rank separator node.

**Method:** `multi_query_recursive_inner()`

```
1. Identify separator v: node with highest rank in the main path
2. Extract v_s (predecessor of v) and v_t (successor of v)
3. Compute adjusted parameters for left (S→v_s) and right (v_t→T) subproblems:
   - gamma (sharing threshold) = (γ × best_dist - d(v_s,v_t)) / d(sub)
   - alpha (T-test fraction) = α × best_dist / d(sub)
4. Recurse on left/right subproblems with tightened thresholds
5. Combine: stitch left_alt + [v_s, v, v_t] + right_alt for all combinations
6. Sort combined candidates by distance, apply evaluate_candidate() again
7. Append passing candidates up to max_alternatives
```

**Recursion termination** — `RECURSION_MIN_RATIO = 0.3`:
Subproblems whose optimal distance is less than 30% of the original query
distance are not further decomposed — they return only their shortest path.

This recursive structure discovers alternatives that differ specifically in how
they bypass the main path's highest separator — yielding structurally distinct
routes that the flat meeting-node scan might miss.

---

## 3. Integration Layer: `cch.rs` / `line_graph.rs`

### 3.1 Normal Graph — `QueryEngine::multi_query`

**File:** `hanoi-core/src/cch.rs`

The `QueryEngine` wraps `AlternativeServer` and provides the `path_geo_len`
closure it needs:

```
path_geo_len: |path| → Haversine sum using graph latitude/longitude arrays
```

Edge costs are now produced internally by `AlternativeServer` during
`reconstruct_path_with_costs()` — no external `edge_cost` closure needed.

Post-processing pipeline:
1. Over-request candidates: `request_count = max_alternatives × GEO_OVER_REQUEST (3)`, minimum `max_alternatives + 12`
2. For each accepted candidate from `AlternativeServer::alternatives()`:
   - Skip empty paths
   - Map node IDs → coordinates
   - Apply `MAX_GEO_RATIO` (2.0×) geographic distance cap relative to shortest route
   - Reconstruct arc IDs via `reconstruct_arc_ids` (linear scan of CSR adjacency); skip candidate on failure
   - Assemble `QueryAnswer` with all metadata

### 3.2 Line Graph — `LineGraphQueryEngine::multi_query`

**File:** `hanoi-core/src/line_graph.rs`

Same structure as normal graph, but with line-graph-specific adaptations:

- **Nodes are edges:** LG node IDs correspond to original graph arc IDs. The
  query takes `source_edge` and `target_edge` as parameters.
- **Geographic length:** `lg_path_geo_len()` maps each LG node to its
  original-graph tail node coordinates, plus the final head node coordinate.
- **Source-edge correction:** `distance_ms = cch_distance + source_edge_cost`
  (the CCH distance starts from the LG node representing the source edge, but
  the user experiences the full edge traversal cost).
- **Answer construction:** `build_answer_from_lg_path` handles LG→original
  mapping, turn annotation via `compute_turns`/`refine_turns`, and optional
  path trimming for coordinate-based queries.

### 3.3 Coordinate Queries

Both engines provide `multi_query_coords` which:
1. Snap origin/destination using `SpatialIndex::validated_snap_candidates`
   (same snap logic as single-path queries)
2. Iterate snap candidate pairs
3. Run `multi_query` on first pair that produces results
4. Patch origin/destination metadata onto all returned answers

---

## 4. Server Integration

### 4.1 Request Flow

```
HTTP POST /query?alternatives=3&stretch=1.25
  → handlers.rs: parse QueryRequest + FormatParam (alternatives, stretch)
  → mpsc channel → engine thread (QueryMsg with alternatives + stretch)
  → engine.rs: dispatch_normal() or dispatch_line_graph()
    → if alternatives > 0:
        engine.multi_query_coords() or engine.multi_query()
        → format_multi_response() → GeoJSON FeatureCollection or JSON array
    → else:
        single-path query (existing flow)
```

### 4.2 Response Format

**GeoJSON (default):** FeatureCollection with one Feature per route. Each
Feature has:
- `route_index` (0 = shortest path)
- `distance_ms`, `distance_m`
- `path_nodes`, `route_arc_ids`, `weight_path_ids`
- `turns` (line-graph mode only)
- Color coding when `?colors` is set (10 distinct colors, primary route thicker)

**JSON (`?format=json`):** Array of `QueryResponse` objects.

### 4.3 CLI Integration

`hanoi-cli query --alternatives N --stretch F` routes through the same engine
methods. Both normal and line-graph paths are supported.

---

## 5. Tuning Constants

**Algorithm core** (`alternative.rs`):

| Constant | Value | Purpose |
|---|---|---|
| `DEFAULT_STRETCH` | 1.25 | Max geographic stretch factor (25% longer) |
| `SHARING_THRESHOLD` | 0.80 | Max cost-overlap ratio with any accepted route |
| `BOUNDED_STRETCH_EPS` | 0.40 | Detour segment must be ≤ optimal × (1 + ε) |
| `LOCAL_OPT_T_FRACTION` | 0.40 | T-test half-window as fraction of optimal distance |
| `LOCAL_OPT_EPSILON` | 0.10 | T-test tolerance for local subpath optimality |
| `RECURSION_MIN_RATIO` | 0.30 | Skip recursion if subproblem < 30% of original |
| `TRAVEL_TIME_STRETCH` | 1.50 | Pre-filter: skip candidates > 1.5× shortest time |

**Hanoi integration** (`multi_route.rs`):

| Constant | Value | Purpose |
|---|---|---|
| `MAX_GEO_RATIO` | 2.0 | Post-filter: reject routes >2× shortest geo distance |
| `GEO_OVER_REQUEST` | 3 | Over-request multiplier to survive post-filtering |

---

## 6. Key Design Decisions

**Why reimplemented elimination tree walk instead of reusing `Server::query`?**
The standard query prunes the walk at meeting nodes via `skip_next()` — correct
for single-path queries but loses candidate information needed for alternatives.
The multi-route walk must relax edges at every meeting node to ensure correct
distance propagation to all ancestor candidates.

**Why independent T-test scratch arrays?**
The T-test runs sub-queries `d(v', v'')` between path endpoints. If these
queries reused `fw_parents`/`bw_parents`, they would overwrite parent pointers
needed to reconstruct paths for later candidates. Dedicated
`TimestampedVector`-backed arrays solve this with minimal memory overhead.

**Why read-only path reconstruction?**
The original `query.rs::path()` reverses forward pointers into `bw_parents`
in-place — fast but destructive. Since multi-route needs to reconstruct
multiple paths from the same walk data, the reconstruction traces forward and
backward halves independently without mutating parent arrays.

**Why dual-metric stretch (geographic vs travel-time)?**
Travel-time stretch alone can accept routes that take huge geographic detours
if roads happen to be fast. Geographic stretch ensures alternatives look
reasonable on a map. The CCH walk uses travel-time as the primary metric; the
geographic check is a secondary post-filter.

**Why pairwise sharing instead of just vs. shortest path?**
Checking overlap only against the shortest path allows multiple alternatives
that are diverse from the shortest but nearly identical to each other.
Pairwise checking ensures every pair of accepted routes is sufficiently
distinct.

**Why cost-based sharing instead of edge-count sharing?**
Edge-count sharing treats all edges equally, so a handful of expensive shared
highway edges could be outweighed by many cheap shared local-road edges. Using
per-edge cost weights the overlap by actual impact on travel time.

**Why bounded stretch at deviation points?**
The geographic stretch catches globally long detours, but a route could share
most of its prefix/suffix with the shortest path and only deviate briefly via
an unnecessarily long segment. The deviation-point bounded stretch test
isolates exactly the divergent segment and checks it against the true shortest
path between those two points.

**Why recursive decomposition?**
The flat meeting-node scan finds alternatives that differ at CCH separator
nodes. But the best separator might not split the route into structurally
different geographic regions. Recursion splits around the highest-rank
separator and seeks alternatives in each half independently, then stitches
them — discovering routes the flat scan cannot reach.
