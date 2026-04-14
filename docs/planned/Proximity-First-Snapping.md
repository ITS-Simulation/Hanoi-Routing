# Proximity-First Snapping & Road-Conforming Connectors

**Status:** Planned  
**Date:** 2026-04-10  
**Primary target:** Line-graph engine (`LineGraphQueryEngine`)

---

## Problems

Both problems are observed in **line-graph mode**, which is the production
engine. Normal-graph (`QueryEngine` in `cch.rs`) has the same code pattern
and should receive the same fixes for consistency, but line-graph is the
priority and all code references below target it unless noted otherwise.

### 1. Straight-line connector segments

The route geometry between the user's pin and the route path does not follow
any road. It's a raw straight line from the pin to the first/last route
coordinate.

**How it happens:**

`LineGraphQueryEngine::patch_coordinates()` (`line_graph.rs:671`) splices
modelling nodes between the projected snap point and the graph entry/exit
node — but the projected point *itself* is not part of the `coordinates`
array. It's stored separately as `snapped_origin` / `snapped_destination`.

Then `connect_query_coordinates()` (`engine.rs:236`) assembles:

```
[origin, snapped_origin, ...coordinates..., snapped_destination, destination]
```

Two segments are straight lines with no road geometry:
- `origin → snapped_origin` (user pin to projected point)
- `snapped_destination → destination`

The first modelling-node connector point may be far from `snapped_origin` if
the projection lands mid-segment, creating a visible gap or diagonal.

### 2. Cost-based ranking produces wrong-edge snaps

`composite_snap_score()` (`spatial.rs:462`) ranks snap candidates by:

```
route_cost_ms + 50 * src_snap_distance_m + 50 * dst_snap_distance_m
```

On divided roads where opposing carriageways are 10-20m apart, the penalty
difference between correct side (5m) and wrong side (15m) is only 500ms.
Any route-cost savings > 500ms through the wrong edge wins. Result:
destination snaps across the street.

The 20×20 nested loop in `LineGraphQueryEngine::query_coords()`
(`line_graph.rs:405-479`) tries up to 400 CCH queries per request, all
ranked by this composite score.

---

## Proposed Design

### Part A: Tiered proximity-first candidate selection

**Principle:** The user picks a location. Snap to the closest edge. Only
explore farther candidates if no route is found from nearby ones.

Replace the flat 20×20 loop in `LineGraphQueryEngine::query_coords()`
(`line_graph.rs:405`) with tiered exploration:

```
Tier 1 — Nearest pair (1 query)
  src = candidates[0], dst = candidates[0]
  Route found? → done.

Tier 2 — Small grid (up to K² queries, K ≈ 4)
  src = candidates[0..K], dst = candidates[0..K]
  Try all pairs. Among pairs that produce a valid route,
  pick the one with smallest snap_distance_m sum.
  Found? → done.

Tier 3 — Full fallback (remaining pairs up to SNAP_MAX_CANDIDATES²)
  Only reached when nearby edges are all dead-ends or disconnected.
  Same rule: smallest snap distance sum among valid-route pairs.
```

**No route cost in ranking.** Within each tier, the only ranking criterion
is `src.snap_distance_m + dst.snap_distance_m`. Cost optimization is the
CCH engine's job once entry/exit edges are chosen.

Same rewrite applies to `LineGraphQueryEngine::multi_query_coords()`
(`line_graph.rs:894`).

**Line-graph specific note:** In the line-graph engine, each snap candidate's
`edge_id` is used directly as a line-graph node ID (unified snap space via
`original_spatial`). The tiered approach works identically — the CCH query
`(src.edge_id → dst.edge_id)` is just a line-graph node query.

**Performance:**

| Scenario | Queries (current) | Queries (proposed) |
|----------|-------------------|--------------------|
| Common case | up to 400 | 1 |
| One-way street | up to 400 | up to 16 (K=4) |
| Disconnected edge | up to 400 | up to 400 |

**What gets deleted from `spatial.rs`:**
- `composite_snap_score()` — removed
- `snap_penalty_ms()` — removed
- `SNAP_PENALTY_FACTOR_MS_PER_M` — removed

**Edge cases (line-graph context):**

- *One-way wrong direction:* In line-graph mode, a one-way edge is a single
  LG node with outgoing transitions only in one direction. However, a CCH
  query from a wrong-direction edge can still succeed — the route goes
  forward along the edge, U-turns somewhere, and returns. Tier 1 will
  accept this as a valid route (longer, but routable). This is consistent
  with the proximity-first principle: the nearest edge wins regardless of
  cost. Only if the edge is truly unreachable (disconnected component)
  does tier 2 activate.
- *Dual carriageway:* Two original edges very close together → two LG nodes.
  Closest edge wins. If the nearest edge routes via a U-turn, that's
  accepted — the snap point is still correct. Tier 2 only activates if
  the nearest edge is unreachable.
- *Same-edge src/dst:* `direct_same_edge_coordinate_answer()` handles this
  before the tiered loop. **No Part A change needed.** However, Part B
  geometry changes DO apply: the same-edge branch in `patch_coordinates()`
  (`line_graph.rs:679`) fills `coordinates` via `open_interval_between_snaps()`
  but still sets `snapped_origin`/`snapped_destination` separately. The
  projected endpoints must be prepended/appended to `coordinates` in this
  branch too — see Part B same-edge note below.
- *Same-edge cycle:* `query_same_edge_cycle_candidate()` also runs before
  tiers. No Part A change needed. Part B applies through `patch_coordinates()`.

### Part B: Road-conforming connector geometry

**Principle:** The only straight-line segment should be the perpendicular
drop from the user's pin onto the road surface.

**Current flow:**
```
patch_coordinates() builds:
  [connector_modelling_nodes..., route_coords..., connector_modelling_nodes...]
  + sets snapped_origin/snapped_destination separately

connect_query_coordinates() wraps:
  [origin, snapped_origin, <patch result>, snapped_destination, destination]
```

The gap: `snapped_origin` (projected point) and the first connector
modelling node may be far apart, with nothing between them on the road.

**Proposed flow:**

Move all geometry assembly into `patch_coordinates()`. Build the full
chain:

```
[origin]                              ← perpendicular drop (straight, correct)
[projected snap point]                ← on the road
[connector modelling nodes → entry]   ← follow edge polyline
[route coordinates]                   ← core route
[exit → connector modelling nodes]    ← follow edge polyline
[projected snap point]                ← on the road
[destination]                         ← perpendicular drop (straight, correct)
```

`connect_query_coordinates()` in engine.rs becomes a pass-through — it no
longer needs to insert `snapped_origin`/`snapped_destination` because
`patch_coordinates()` already includes them in polyline order.

**Specific changes in `LineGraphQueryEngine::patch_coordinates()`
(`line_graph.rs:671`):**

**Connector-path case** (the `else` branch, line 682):

For the source side:
1. Prepend `(src.projected_lat, src.projected_lng)` to the connector points
   (before the modelling nodes, since it's closer to origin along the edge)
2. Set `answer.origin = Some(from)` (user pin coordinates)

For the destination side:
1. Append `(dst.projected_lat, dst.projected_lng)` after the connector points
2. Set `answer.destination = Some(to)`

**Same-edge case** (the `if answer.coordinates.is_empty() && src.edge_id == dst.edge_id` branch, line 679):

`open_interval_between_snaps()` returns interior polyline points between
the two snap positions, but does NOT include the projected endpoints
themselves. After that call:
1. Prepend `(src.projected_lat, src.projected_lng)` to `coordinates`
2. Append `(dst.projected_lat, dst.projected_lng)` to `coordinates`

This ensures the same-edge geometry also starts/ends at the on-road
projected points, not at the first interior polyline vertex.

Dedup: if projected point coincides with the first/last route coordinate
(within 1m), skip it. Inline a haversine < 1m check directly in
`patch_coordinates()` — the existing `coords_within_dedup_threshold()`
in `engine.rs:232` has the right semantics but lives in `hanoi-server`
(different crate, not importable by `hanoi-core`). `push_unique` in
`spatial.rs` is NOT suitable (exact float equality, not distance-based).

`connect_query_coordinates()` then only prepends `origin` and appends
`destination` — both are perpendicular drops to already-on-road projected
points.

---

## Files to Modify

| Priority | File | Changes |
|----------|------|---------|
| **P0** | `hanoi-core/src/line_graph.rs` | Rewrite `query_coords()` and `multi_query_coords()` with tiered exploration. Update `patch_coordinates()` to include projected points in polyline order. **Primary target.** |
| **P0** | `hanoi-core/src/spatial.rs` | Delete `composite_snap_score`, `snap_penalty_ms`, `SNAP_PENALTY_FACTOR_MS_PER_M`. |
| **P1** | `hanoi-core/src/geometry.rs` | Make `annotate_distances()` `pub(crate)` (currently private, needed by `patch_coordinates()` in same crate for distance recomputation after index shift). |
| **P1** | `hanoi-server/src/engine.rs` | Simplify `connect_query_coordinates()` — stop inserting `snapped_origin`/`snapped_destination` (already in the coordinate list from `patch_coordinates`). |
| **P2** | `hanoi-core/src/cch.rs` | Same tiered rewrite and `patch_coordinates()` update for normal-graph path (consistency — not production-critical). |

**Files NOT modified:** `snap_candidates()`, `validated_snap_candidates()`,
`bounds.rs`, `route_eval.rs`.

---

## multi_query_coords interaction (line-graph)

Tiered exploration picks the snap pair. Once selected,
`LineGraphQueryEngine::multi_query_coordinate_candidates()`
(`line_graph.rs:618`) → `AlternativeServer::alternatives()` generates
alternative routes between those fixed entry/exit LG nodes. Alternative
routes = different LG paths, not different snap points.

---

## Part B: GeoJSON Output Hazards (line-graph data flow)

The full data flow from line-graph query to final GeoJSON output:

```
materialize_lg_path()               → QueryAnswer { coordinates, origin=None, snapped_origin=None }
  ↓                                    (line_graph.rs:731)
LG::patch_coordinates()             → prepend/append connector nodes to coordinates,
  ↓                                    set origin, destination, snapped_origin, snapped_destination
  ↓                                    (line_graph.rs:671)
connect_query_coordinates()          → assemble [origin, snapped_origin?, ...coords..., snapped_dest?, dest]
  ↓                                    (engine.rs:236)
answer_to_geojson()                  → convert to [lng, lat], emit as LineString geometry
  OR answer_to_response()            → keep [lat, lng], emit as coordinates array
  OR answers_to_geojson()            → multi-route, same per-answer flow
```

All 3 output paths (JSON, GeoJSON, multi-GeoJSON) funnel through
`connect_query_coordinates()`, so a single fix there propagates everywhere.
But moving projected points into `coordinates` in `patch_coordinates()`
introduces three secondary hazards:

### Hazard 1: Duplicate projected points

If `patch_coordinates()` embeds projected points in `coordinates`, and
`connect_query_coordinates()` also inserts `snapped_origin/destination`,
the projected point appears **twice** in the output LineString.

**Fix:** After embedding projected points in `coordinates`, set
`answer.snapped_origin = None` and `answer.snapped_destination = None`.
`connect_query_coordinates()` naturally skips `None` values. If the
projected point is still needed as metadata in GeoJSON properties, add it
there explicitly (separate from geometry coordinates).

### Hazard 2: `distance_m` double-counts

`route_distance_with_snapped_endpoints_m()` in
`LG::patch_coordinates()` (`line_graph.rs:708`) adds haversine distance
from `snapped_origin → coordinates[0]` and
`coordinates.last() → snapped_destination`. If projected points are now
inside `coordinates`, this double-counts the first/last segment.

**Fix:** After moving projected points into `coordinates`, switch to
plain `route_distance_m(&answer.coordinates)`. The projected points are
already in the coordinate chain — no endpoint extension needed.

### Hazard 3: `turn.coordinate_index` offsets

Turn annotations carry `coordinate_index` pointing into `coordinates`.
Prepending projected point + connector modelling nodes shifts all
indices.

**Note:** This is a **pre-existing latent bug** — `patch_coordinates()`
already prepends connector modelling nodes but does NOT adjust
`turn.coordinate_index`. The current code only avoids visible breakage
because connector nodes are usually 0-1 points. Adding the projected
point would make this off by one more.

**Why it matters even though `coordinate_index` is `#[serde(skip)]`:**
`coordinate_index` is not serialized to API output, but it IS used
internally by `annotate_distances()` (`geometry.rs:426`) to compute
`distance_to_next_m` — which IS serialized. Wrong indices → wrong
distance segments in the API response.

**Timing constraint:** `refine_turns()` (which calls `annotate_distances()`)
runs inside `materialize_lg_path()` at `line_graph.rs:803`, BEFORE
`patch_coordinates()` is called. So the shift in `patch_coordinates()`
happens after distances are already computed. This means:

**Fix:** After prepending connector + projected points, shift all turn
indices AND recompute distances:

```rust
let prepended_count = connected.len(); // connector nodes + projected point
if prepended_count > 0 {
    for turn in &mut answer.turns {
        turn.coordinate_index += prepended_count as u32;
    }
    // Recompute distance_to_next_m since annotate_distances() ran
    // before the coordinate shift (in materialize_lg_path)
    annotate_distances(&mut answer.turns, &answer.coordinates);
}
```

Appended connector + projected points at the destination end don't
affect indices (they're after the route coordinates), but they DO
extend `coordinates` — so the final turn's `distance_to_next_m` must
also be recomputed. The `annotate_distances()` call above handles both
cases since it recomputes all segments including the last one.

---

## Implementation Order

All steps target line-graph engine first. Normal-graph (`cch.rs`) receives
the same changes afterward for consistency (P2 — not blocking).

1. **Part A** — tiered exploration in `LineGraphQueryEngine::query_coords()`
   and `multi_query_coords()`. Delete composite scoring from `spatial.rs`.
2. **Part B** — road-conforming connectors in `LG::patch_coordinates()`.
   Must address all 3 hazards (duplicate points, distance_m, turn indices).
   Update `connect_query_coordinates()` in engine.rs accordingly.
3. **Cleanup** — purge all stale artifacts (see below).
4. **P2: cch.rs parity** — apply same changes to `QueryEngine` for
   normal-graph consistency.

## Cleanup: Stale Artifact Removal

After Parts A and B, the following artifacts become dead code and must
be removed to avoid confusion and compile errors.

### Part A cleanup (after tiered exploration replaces composite scoring)

**Delete from `spatial.rs`:**
- `composite_snap_score()` (line 462)
- `snap_penalty_ms()` (line 458)
- `SNAP_PENALTY_FACTOR_MS_PER_M` (line 79)
- Unit tests referencing these: `composite_snap_score` and
  `snap_penalty_ms` calls in test module (lines 649, 801-805)

**Remove imports and call sites in `line_graph.rs`:**
- Import line 21: remove `composite_snap_score`, `snap_penalty_ms` from
  the `use crate::spatial::` statement
- `query_coords()` loop (lines 430, 433, 442, 459): entire composite
  scoring logic replaced by tiered exploration
- `multi_query_coords()` loop (lines 921, 924, 933, 948): same

**Remove imports and call sites in `cch.rs` (P2):**
- Import line 17: remove `composite_snap_score`, `snap_penalty_ms`
- `query_coords()` loop (lines 263, 266, 275, 284): replaced
- `multi_query_coords()` loop (lines 425, 428, 437, 452): replaced

### Part B cleanup (after projected points move into coordinates)

**`route_distance_with_snapped_endpoints_m` (`cch.rs:61`):**
- `line_graph.rs:708` switches to `route_distance_m()` — projected
  points are already in the coordinate chain
- `cch.rs:599` same switch (P2)
- Delete `route_distance_with_snapped_endpoints_m()` once both callers
  are migrated

**`snapped_origin` / `snapped_destination` on `QueryAnswer` (`cch.rs:45-47`):**
- After Part B, `patch_coordinates()` sets these to `None`. Keep the
  fields for now — `connect_query_coordinates()` checks them harmlessly
  (`None` → skip). Remove the fields entirely only after
  `connect_query_coordinates()` is simplified and no output path reads
  them.

**`connect_query_coordinates()` (`engine.rs:236`):**
- After Part B, snapped values are always `None`. Simplify the function
  signature to drop `snapped_origin`/`snapped_destination` params.
- Remove the dedup blocks at lines 250-255 and 260-265 (no snapped
  values to insert).
- All three callers (`answer_to_response` line 295, `answer_to_geojson`
  line 349, `answers_to_geojson` line 465) update to pass only
  `coordinates`, `origin`, `destination`.

**Dead helpers in `engine.rs` after snapped-point removal:**
- `SNAP_NODE_DEDUP_DISTANCE_M` (line 230) — only used by
  `coords_within_dedup_threshold`. Delete.
- `coords_within_dedup_threshold()` (line 232) — only called inside the
  snapped-point insertion blocks (lines 252, 262) which are removed.
  Delete.

**Dead tests in `engine.rs`:**
- `connect_query_coordinates_inserts_projected_snap_points_in_order`
  (line 527) — tests snapped-point insertion behavior being removed.
  Delete or rewrite to test simplified signature.
- `connect_query_coordinates_skips_projected_points_near_graph_nodes`
  (line 550) — tests dedup of snapped points. Delete.

## Verification

All verification against **line-graph server mode**:

- Divided road: route starts from correct carriageway side
- One-way: route starts from nearest edge; may U-turn if wrong direction
  (acceptable — proximity-first principle)
- Visual: no off-road diagonals between pin and route start/end
- GeoJSON output: projected points appear exactly once in LineString geometry
- `distance_m` matches haversine sum of the output coordinate chain
- Turn annotation `coordinate_index` values point to correct coordinates
- Multi-route (`?alternatives=N`): same connector quality for all alternatives
- Performance: query latency improvement (1 query vs 400 in common case)
- Existing tests in `spatial.rs` still pass (minus deleted composite
  scoring tests)

**Required new test coverage:**

- **Tiered snap selection:** tier 1 picks nearest pair (succeeds even on
  wrong-direction one-ways via U-turn); tier 2 activates only on
  unreachable edges; tier 3 fallback on disconnected components
- **Same-edge projected endpoints:** `patch_coordinates()` same-edge
  branch includes projected points in `coordinates` (not just interior
  polyline vertices)
- **Duplicate projected-point avoidance:** when projected point is within
  1m of first/last route coordinate, it is not duplicated
- **`distance_to_next_m` after index shift:** turn distances recomputed
  correctly after connector geometry is prepended
- **`connect_query_coordinates()` simplified:** test that origin +
  coordinates + destination assemble correctly without snapped params

---

## Part C: Clip Backtrack Protrusions at Source/Destination

**Status:** Post-implementation amendment  
**Depends on:** Parts A and B completed

### Problem

After Parts A+B, a visible protrusion remains at route start/end. Three
points define the geometry:

```
P1 = user pin (origin/destination)
P2 = 90° projected point on snapped edge (SnapResult)
P3 = route start coordinate (coordinates[0] from materialize_lg_path)
```

P3 is the head of the source edge. When the snapped edge faces away from
the destination, P3 is "behind" P2 along the travel direction. The
geometry backtracks from P2 to P3 before going forward past P2 again:

```
[P1, P2, ...backtrack to P3..., ...forward past P2..., ...route...]
         ^^^^^^^^^^^^^^^^^^^^^^
         visible protrusion
```

~80% of queries: P2 ≈ P3 (or P3 is ahead of P2), no issue.  
~20% of queries: P3 is behind P2, producing the protrusion.

### Root cause

The LG CCH query `src.edge_id → dst.edge_id` commits to exiting
through the source edge's head node. When the edge faces away from the
destination, the route exits at head, U-turns at the next intersection,
and comes back past the snap point. The connector geometry faithfully
traces this backtrack.

### Fix: Clip the backtrack segment

After `patch_coordinates()` assembles the full coordinate chain, detect
and remove the backtrack between P2 and the point where the route
re-crosses P2's position.

**Two-way roads only.** On a one-way road, the backtrack IS the real
route — the vehicle must physically go to the head, U-turn, and come
back. Clipping would hide actual road geometry.

**Detection:** After prepending P2 (projected point) to coordinates,
check whether `coordinates[1]` (= P3, the route start) is "behind" P2
relative to the route's forward direction. Concretely:

```rust
// P2 = coordinates[0] (projected point, just prepended)
// P3 = coordinates[1] (route start = head of source edge)
// P4 = coordinates[2] (next route point after P3)
//
// If bearing(P2→P3) and bearing(P3→P4) differ by > 90°,
// P3 is behind P2 and the route backtracks.
```

**Clipping algorithm (source side):**

1. After prepending `src_projected`, examine `coordinates[0..N]`
2. Find the first index `i` where the route is moving **away** from P2
   (i.e., haversine distance to P2 is increasing), then find the next
   index `j > i` where the route passes closest to P2 again (distance
   starts decreasing then stops)
3. Replace `coordinates[0..j]` with just `[P2]` — clip the entire
   backtrack loop
4. Only apply when the source edge is **two-way** (check: an edge with
   `edge_id'` exists going `head → tail` of the source edge)

**Same logic applies to destination side** (symmetric — clip coordinates
from the end where the route overshoots past `dst_projected` and comes
back).

**Pseudo-code for source-side clipping in `patch_coordinates()`:**

```rust
// After prepending src_projected to answer.coordinates:
if answer.coordinates.len() >= 3 {
    let p2 = answer.coordinates[0]; // projected
    let p3 = answer.coordinates[1]; // route start

    // Check if source edge is two-way (reverse edge exists)
    let is_two_way = self.has_reverse_edge(src.edge_id);

    if is_two_way {
        // Find where route crosses back near P2 after backtracking
        let mut max_dist: f64 = 0.0;
        let mut clip_end: Option<usize> = None;
        for i in 1..answer.coordinates.len() {
            let dist = haversine_m(p2, answer.coordinates[i]);
            if dist >= max_dist {
                max_dist = dist;
            } else if clip_end.is_none() && dist < haversine_m(p2, p3) {
                // Route is now closer to P2 than P3 was — it crossed back
                clip_end = Some(i);
                break;
            }
        }
        if let Some(j) = clip_end {
            // Keep P2, skip backtrack, continue from j onward
            let tail = answer.coordinates.split_off(j);
            answer.coordinates.truncate(1); // keep [P2]
            answer.coordinates.extend(tail);
        }
    }
}
```

### Two-way detection

Need a helper to check if a reverse edge exists for the source edge:

```rust
fn has_reverse_edge(&self, edge_id: EdgeId) -> bool {
    let tail = self.context.original_tail[edge_id as usize];
    let head = self.context.original_head[edge_id as usize];
    // Check if any edge goes head → tail
    let start = self.context.original_first_out[head as usize] as usize;
    let end = self.context.original_first_out[head as usize + 1] as usize;
    (start..end).any(|e| self.context.original_head[e] == tail)
}
```

### Edge cases

- **One-way road:** `has_reverse_edge` returns false → no clipping.
  Route shows the real U-turn path. Correct behavior.
- **P2 ≈ P3:** Distance too small to trigger clipping → no change.
  Common case, already correct.
- **Same-edge src/dst:** Handled before `patch_coordinates()` connector
  branch. Uses `open_interval_between_snaps`. No backtrack possible.
- **Short routes (< 3 coords):** Guard `coordinates.len() >= 3` prevents
  out-of-bounds.

### What does NOT change

- `connector_points_from_snap_to_node()` — removed in Part C (connector
  geometry already dropped; clipping operates on the route coordinates
  from `materialize_lg_path`, not on connector points).
- `open_interval_between_snaps()` — same-edge, unrelated.
- Turn index shifting / `distance_to_next_m` recomputation — clipping
  happens BEFORE the shift/recompute, so prepended_count adjusts
  naturally.
- `connect_query_coordinates()` — unchanged.

### Impact on `distance_m`

Clipping removes backtrack distance from the coordinate chain.
`route_distance_m()` reports the clipped (shorter, more realistic)
distance. This is correct — the clipped path represents what the user
sees on screen and the practical travel distance from the snap point.
