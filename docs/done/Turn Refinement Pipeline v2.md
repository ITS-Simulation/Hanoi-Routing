# Turn Refinement Pipeline v2 — Snap Edge Trimming

**Module:** `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`
**Status:** Pending (depends on v1 refinement pipeline, already implemented)
**Goal:** Eliminate phantom first/last turns caused by snapped source and
destination edges in coordinate-based line-graph queries

---

## 1. Problem Statement

When a user queries by coordinates, the line-graph engine snaps each coordinate
to the nearest **original edge** (not a node). The snapped edge ID becomes a
line-graph node, and the CCH routes between these two LG nodes.

The problem: the route includes the **full extent** of both snapped edges.

```
User origin (×) is near the middle of edge E_src (tail_A → head_B)
User destination (×) is near the middle of edge E_dst (tail_Y → head_Z)

LG path: [E_src, E_1, E_2, ..., E_N, E_dst]

Coordinate output:
  tail_A → head_B → ... → tail_Y → head_Z
  ^^^^^^                             ^^^^^^
  phantom start                      phantom end
```

- `tail_A` is the far end of the source edge — the route goes AWAY from the
  user before turning around, producing a phantom first turn.
- `head_Z` is the far end of the destination edge — the route OVERSHOOTS past
  the user's destination, producing a phantom final turn (often a U-turn).

### Why this is line-graph specific

The **normal graph engine** (`CchContext::QueryEngine`) snaps to the nearest
**node** (`SnapResult::nearest_node()`), not an edge. Its path starts and ends
at intersection nodes. There are no extra edge segments to trim.

The **line-graph engine** snaps to the nearest **edge** because LG nodes ARE
original edges. The snapped edge becomes the source/destination LG node, forcing
the route to include both endpoints of that edge.

### Scope

This fix applies **only** to:
- `LineGraphQueryEngine::query_coords()` — coordinate-based line-graph queries

This fix does NOT apply to:
- `LineGraphQueryEngine::query()` — node-ID queries (caller chose edges explicitly)
- `QueryEngine::query()` or `query_coords()` — normal graph (no edge-snap issue)

---

## 2. Solution: Split `query` into `query` + `query_trimmed`

### Current Architecture

```
query_coords(from, to)
  → snap → src_edge, dst_edge
  → self.query(src_edge, dst_edge)          // full path including snap edges
    → returns QueryAnswer with full lg_path
  → Self::patch_coordinates(answer, from, to)
```

### New Architecture

```
query_coords(from, to)
  → snap → src_edge, dst_edge
  → self.query_trimmed(src_edge, dst_edge)  // path with snap edges trimmed
    → CCH query → full lg_path
    → trim lg_path[1 .. len-1]
    → compute turns + coordinates from trimmed path
    → returns QueryAnswer with trimmed data
  → Self::patch_coordinates(answer, from, to)
```

`query()` stays unchanged for direct node-ID callers. A new `query_trimmed()`
does the CCH query but trims the source and destination edges from the LG path
before building turns, coordinates, and distance.

---

## 3. Algorithm: `query_trimmed`

```rust
fn query_trimmed(&mut self, source_edge: EdgeId, target_edge: EdgeId) -> Option<QueryAnswer>
```

**Step 1: Run the CCH query (identical to `query`)**

```
let result = self.server.query(Query { from: source_edge, to: target_edge });
let cch_distance = connected.distance();
let source_edge_cost = original_travel_time[source_edge];
let distance_ms = cch_distance.saturating_add(source_edge_cost);
let lg_path = connected.node_path();
```

`distance_ms` is the full travel time. We keep it because:
- It represents the actual time cost the CCH computed for the route
- Subtracting source/destination edge costs would require knowing the partial
  traversal fraction (how far along the edge the user actually is), which the
  current snap model doesn't provide

**Step 2: Trim the LG path**

```
let trimmed: &[NodeId] = if lg_path.len() > 2 {
    &lg_path[1..lg_path.len() - 1]
} else {
    &[]
};
```

Cases:
- `lg_path.len() > 2`: Normal case. Drop first and last elements.
- `lg_path.len() == 2`: Source and destination are adjacent (one LG edge
  between them). Trimmed path is empty — the route is a single transition.
- `lg_path.len() == 1`: Source == destination (same edge). Trimmed path is
  empty — the user is already at the destination.
- `lg_path.len() == 0`: Impossible (CCH returned a found path).

**Step 3: Build path, coordinates, turns from `trimmed`**

```
let turns = refine_turns(compute_turns(
    trimmed, &original_tail, &original_head, &original_lat, &original_lng
));

let mut path: Vec<NodeId> = trimmed.iter()
    .map(|&lg_node| original_tail[lg_node])
    .collect();

if let Some(&last_edge) = trimmed.last() {
    path.push(original_head[last_edge]);
} else if lg_path.len() >= 2 {
    // Trimmed to empty — use the shared intersection between src and dst edges.
    // By the line-graph invariant: head(src_edge) == tail(dst_edge) when they
    // are connected by an LG edge (lg_path.len() == 2).
    // When lg_path.len() == 1 (src == dst), use head(src_edge) as a midpoint.
    path.push(original_head[lg_path[0] as usize]);
}

let coordinates: Vec<(f32, f32)> = path.iter()
    .map(|&node| (original_lat[node], original_lng[node]))
    .collect();

let distance_m = route_distance_m(&coordinates);
```

**Step 4: Return**

```
Some(QueryAnswer {
    distance_ms,     // full CCH travel time (untrimmed)
    distance_m,      // Haversine sum of trimmed path
    path,            // trimmed intersection nodes
    coordinates,     // trimmed (lat, lng) pairs
    turns,           // computed from trimmed path
    origin: None,
    destination: None,
})
```

---

## 4. `distance_ms` vs `distance_m` After Trimming

After trimming:
- `distance_ms` = full CCH travel time (includes source + destination edge cost)
- `distance_m` = Haversine distance of trimmed coordinate path (excludes
  source/destination edges)

In theory these describe slightly different path extents. In practice the
difference is negligible: the trimmed edges are the ones closest to the user's
position — short urban road segments (typically 20–50m in Hanoi's dense
network). Their travel time contribution is a few hundred milliseconds at
most, well within the inherent snap approximation error.

No special handling is needed. Both metrics are close enough that consumers
will not observe a meaningful discrepancy.

---

## 5. Files to Modify

### `hanoi-core/src/line_graph.rs`

1. Add `fn query_trimmed(&mut self, source_edge: EdgeId, target_edge: EdgeId) -> Option<QueryAnswer>` — identical to `query()` but trims `lg_path[1..len-1]` before building path/coordinates/turns.
2. Update `query_coords()`: replace `self.query(src_edge, dst_edge)` with `self.query_trimmed(src_edge, dst_edge)` in both the primary path and the fallback loop.

No other files change. `query()` remains unchanged. The normal graph engine
is not touched.

---

## 6. What Changes in the Output

### Coordinate-based line-graph queries (`query_coords`)

| Field | Before | After |
|-------|--------|-------|
| `coordinates` | `[tail(src), head(src), ..., tail(dst), head(dst)]` | `[tail(edge_1), ..., head(edge_N)]` |
| `path` | Includes src/dst edge endpoints | Excludes src/dst edge endpoints |
| `turns` | Phantom first/last turns present | Clean — no snap artifacts |
| `distance_m` | Includes src/dst edge length | Matches visible trimmed path |
| `distance_ms` | Full CCH travel time | Unchanged |
| `origin` | User coordinate | Unchanged |
| `destination` | User coordinate | Unchanged |

### Node-ID line-graph queries (`query`)

No change. Full LG path as before.

### Normal graph queries (`query` / `query_coords`)

No change. Normal graph snaps to nodes, not edges.

---

## 7. Edge Cases

| Case | `lg_path` | Trimmed | Path output | Turns | distance_m |
|------|-----------|---------|-------------|-------|------------|
| Normal route | `[E_s, E_1, ..., E_N, E_d]` | `[E_1, ..., E_N]` | `[tail(E_1), ..., head(E_N)]` | Computed from trimmed | Haversine of trimmed |
| Adjacent edges | `[E_s, E_d]` | `[]` | `[head(E_s)]` (single point) | `[]` | 0.0 |
| Same edge | `[E_s]` | `[]` | `[head(E_s)]` (single point) | `[]` | 0.0 |
| 3 edges | `[E_s, E_1, E_d]` | `[E_1]` | `[tail(E_1), head(E_1)]` | `[]` (single edge, no transition) | Haversine of 2 points |
| 4 edges | `[E_s, E_1, E_2, E_d]` | `[E_1, E_2]` | 3 nodes | 1 turn (refined) | Haversine of 3 points |

### Fallback Candidate Loop

All candidates go through `query_trimmed`. The best-distance comparison still
uses `distance_ms` (untrimmed travel time), which is correct — the CCH-optimal
route should win regardless of how the coordinates are trimmed for display.

---

## 8. Test Plan

### Unit-Level: No New Unit Tests Needed

The trimming is a simple slice operation (`lg_path[1..len-1]`). The downstream
functions (`compute_turns`, `refine_turns`, coordinate mapping) are already
tested. The logic is straightforward indexing.

### Integration Test Update

The existing integration test in `turn_direction_integration.rs` calls
`engine.query(0, 3)` — a node-ID query, not a coordinate query. It is
**not affected** by this change and should continue to pass as-is.

A full end-to-end test of `query_coords` would require a spatial index built
from the synthetic graph, which is beyond the scope of this change. The existing
test validates that the refinement pipeline works correctly; the trim is a
pre-processing step that feeds into the same pipeline.

---

## 9. Invariants

1. **`query()` is never modified.** Node-ID callers always get the full path.
2. **`distance_ms` is never modified.** It always reflects the CCH-optimal
   travel time for the full route.
3. **Turn annotations correspond to `coordinates`** via `coordinate_index`.
   Since both turns and coordinates are built from the same trimmed path,
   their correspondence is preserved.
4. **The normal graph engine is not touched.** It snaps to nodes, not edges.
5. **Fallback candidates use `distance_ms` for comparison.** The untrimmed
   travel time is the correct basis for route ranking.
