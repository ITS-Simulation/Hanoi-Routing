# Turn Refinement Pipeline — Implementation Plan

**Module:** `CCH-Hanoi/crates/hanoi-core/src/geometry.rs`
**Status:** Turn detection implemented; refinement pipeline pending
**Goal:** Reduce raw per-intersection turn annotations into actionable maneuvers
via a two-pass post-processing pipeline (S-curve cancellation, then straight
merging)

---

## 1. Problem Statement

The current `compute_turns()` emits one `TurnAnnotation` per consecutive
line-graph edge pair. On a real Hanoi motorcycle route (481 edges), this produces
480 raw turn annotations:

- **466 straights** (97%) — road segments where no steering decision is needed
- **14 non-straights** (3%) — actual maneuvers (left, right, U-turn)

Two issues:
1. Long runs of consecutive straights are noise — they represent "follow this
   road" and should be a single entry, not hundreds.
2. Some adjacent opposite-sign turn pairs (right immediately followed by left,
   or vice versa) are geometric artifacts (road kinks, short connector edges)
   rather than real navigation maneuvers.

Additionally, the phantom U-turn previously caused by `patch_coordinates()`
injecting user coordinates into the path has been eliminated by the coordinate
pollution fix (origin/destination now live in `QueryAnswer` metadata, not in
`coordinates[]`).

---

## 2. Data Model

### Current

```rust
pub struct TurnAnnotation {
    pub direction: TurnDirection,
    pub angle_degrees: f64,
}
```

One entry per line-graph transition. Turn `i` corresponds to the intersection at
`coordinates[i + 1]` in the output path.

### After Refinement

```rust
pub struct TurnAnnotation {
    pub direction: TurnDirection,
    pub angle_degrees: f64,     // for turns: single angle; for merged straights: cumulative sum
    pub edge_count: u32,        // number of original LG transitions this entry spans
    pub coordinate_index: u32,  // index into coordinates[] where this maneuver occurs
}
```

New fields:
- **`edge_count`**: How many raw transitions this entry represents. A merged
  straight spanning 50 edges has `edge_count: 50`. A single real turn has
  `edge_count: 1`. Cancelled S-curves produce a straight with `edge_count: 2`.
- **`coordinate_index`**: The index into the output `coordinates[]` array where
  this maneuver occurs. For a turn, this is the intersection node. For a merged
  straight, this is the **last** intersection in the straight run (the node
  just before the next turn). Preserves the spatial mapping that would otherwise
  be lost during merging.

### Why `coordinate_index` Points to the End of a Straight

A merged straight represents "continue on this road from intersection A to
intersection B." The start is implied by the previous entry. The end (where the
driver needs to pay attention next) is what matters for navigation — it's where
the next maneuver will happen. For the first entry in the sequence (if it's a
straight), the start is implicitly `coordinates[0]`.

### Coordinate Index Mapping

Given `N` line-graph nodes → `N + 1` coordinates (tails + final head):

```
coordinates:  [0]    [1]    [2]    [3]    [4]    [5]    [6]    [7]
               |      |      |      |      |      |      |      |
raw turns:     turn0  turn1  turn2  turn3  turn4  turn5         ∅
               S      S      R      S      S      L        (destination)
```

With N=7 LG nodes: 6 raw turns (N-1), 8 coordinates (N+1, indices 0-7).
Raw turn `i` occurs at `coordinates[i + 1]`. The final coordinate [7] is the
route destination — no turn occurs there. After merging:

```
merged[0]: Straight(edge_count=2, angle=sum(S,S), coordinate_index=2)
merged[1]: Right   (edge_count=1, angle=-87°,     coordinate_index=3)
merged[2]: Straight(edge_count=2, angle=sum(S,S), coordinate_index=5)
merged[3]: Left    (edge_count=1, angle=+91°,     coordinate_index=6)
```

Note: the last entry's `coordinate_index` is 6, not 7. The destination
coordinate [7] is unreferenced — there is no maneuver at the endpoint.

---

## 3. Algorithm — Two-Pass Pipeline

### Overview

```
compute_turns()          →  raw Vec<TurnAnnotation>     (480 entries)
  ↓
Pass 1: cancel_s_curves()  →  S-curves replaced with straights
  ↓
Pass 2: merge_straights()  →  consecutive straights collapsed
  ↓
                             final Vec<TurnAnnotation>   (~25 entries)
```

Both passes are pure functions (`Vec<TurnAnnotation> → Vec<TurnAnnotation>`) in
`geometry.rs`. The call site in `line_graph.rs` chains them after `compute_turns`.

### Pass 1: S-Curve Cancellation (`cancel_s_curves`)

**Purpose:** Replace adjacent opposite-sign turn pairs that are geometric
artifacts with a single straight carrying the residual angle.

**Input/Output:** `fn cancel_s_curves(turns: Vec<TurnAnnotation>) -> Vec<TurnAnnotation>`

**Algorithm (single forward scan):**

```
if turns.len() < 2: return turns   // guard against usize underflow

i = 0
while i < turns.len() - 1:
    a = turns[i], b = turns[i + 1]

    if BOTH are non-straight
       AND have opposite signs (a.angle * b.angle < 0)
       AND |a.angle + b.angle| < S_CURVE_NET_THRESHOLD  (15°)
       AND max(|a.angle|, |b.angle|) < S_CURVE_MAX_THRESHOLD  (60°):

        → emit Straight(angle = a.angle + b.angle, edge_count = 2)
        → skip i + 1 (advance i by 2)
    else:
        → emit turns[i] as-is
        → advance i by 1

emit final turns[last] if not consumed
```

**Constants:**

```rust
const S_CURVE_NET_THRESHOLD_DEG: f64 = 15.0;
const S_CURVE_MAX_THRESHOLD_DEG: f64 = 60.0;
```

**Threshold rationale:**

- `S_CURVE_NET_THRESHOLD (15°)`: If two opposite turns nearly cancel
  (residual < 15°), the overall direction barely changed — it's a kink, not
  two real maneuvers. 15° is well below `STRAIGHT_THRESHOLD (25°)`, ensuring
  the residual classifies as straight.
- `S_CURVE_MAX_THRESHOLD (60°)`: Protects genuine large-angle maneuvers.
  A 90° right followed by a 90° left at two real intersections should NOT be
  cancelled even if the net is ~0°. The 60° threshold separates:
  - **Artifact range (< 60°):** 37°/36° and 40°/39° pairs from the real data
    — moderate deflections caused by road geometry.
  - **Genuine range (>= 60°):** 87°/91° pairs — real intersection turns that
    happen to partially cancel.

**What this does NOT cancel:**

| Pattern | Why preserved |
|---------|---------------|
| Right(-87°), straight, left(+91°) | Not adjacent — separated by a straight |
| Right(-87°), left(+91°) adjacent | `max(87, 91) >= 60` — too large to be artifact |
| Left(+37°), right(-36°) adjacent | `max(37, 36) < 60` AND `|37-36| < 15` — cancelled (correct) |
| Left(+50°), right(-55°) adjacent | `max(50, 55) < 60` AND `|-5| < 15` — cancelled |
| Left(+55°), right(-30°) adjacent | `max(55, 30) < 60` BUT `|25| >= 15` — NOT cancelled (significant net turn) |

### Pass 2: Straight Merging (`merge_straights`)

**Purpose:** Collapse consecutive straight entries into a single entry with
cumulative angle and edge count. Assign `coordinate_index` to every entry.

**Input/Output:** `fn merge_straights(turns: Vec<TurnAnnotation>) -> Vec<TurnAnnotation>`

Note: at this point, input entries still have `edge_count = 1` (or `2` for
cancelled S-curves) and no `coordinate_index`. This pass sets both.

**Algorithm (single forward scan):**

```
result = []
i = 0
raw_index = 0   // tracks position in the original raw turn sequence

while i < turns.len():
    if turns[i].direction == Straight:
        // Start a merge run
        cumulative_angle = 0.0
        total_edges = 0
        while i < turns.len() AND turns[i].direction == Straight:
            cumulative_angle += turns[i].angle_degrees
            total_edges += turns[i].edge_count
            i += 1
        raw_index += total_edges
        // coordinate_index = raw_index (the intersection AFTER the last merged straight)
        emit Straight(angle=cumulative_angle, edge_count=total_edges, coordinate_index=raw_index)
    else:
        // Non-straight turn — emit as-is with its coordinate_index
        raw_index += turns[i].edge_count
        emit turns[i] with coordinate_index = raw_index
        i += 1
```

**Key invariant:** `coordinate_index` for entry `k` equals the sum of all
`edge_count` values from entries `0..=k`. This is the index into the
`coordinates[]` array where the maneuver occurs (for turns) or ends (for
straights).

**The `direction` field stays `Straight` regardless of cumulative angle.** A
merged straight with a cumulative angle of +75° means the road curves gradually
left, but no single intersection required a turn decision. The `angle_degrees`
field captures the curve for informational purposes. Navigation: "follow the
road." Not: "turn left."

---

## 4. Files to Modify

### `hanoi-core/src/geometry.rs`

1. Add constants: `S_CURVE_NET_THRESHOLD_DEG`, `S_CURVE_MAX_THRESHOLD_DEG`
2. Add `edge_count: u32` and `coordinate_index: u32` fields to `TurnAnnotation`
3. Add `pub fn cancel_s_curves(turns: Vec<TurnAnnotation>) -> Vec<TurnAnnotation>)`
4. Add `pub fn merge_straights(turns: Vec<TurnAnnotation>) -> Vec<TurnAnnotation>`
5. Add `pub fn refine_turns(turns: Vec<TurnAnnotation>) -> Vec<TurnAnnotation>`
   — convenience wrapper that chains `cancel_s_curves` then `merge_straights`
6. Update `compute_turns()` to set `edge_count: 1` and `coordinate_index: 0`
   (placeholder — `merge_straights` assigns the real values)
7. Add unit tests for both passes and the combined pipeline

### `hanoi-core/src/line_graph.rs`

1. Update the import: `use crate::geometry::{compute_turns, refine_turns};`
2. Replace the bare `compute_turns(...)` call with
   `refine_turns(compute_turns(...))`

### `hanoi-core/tests/turn_direction_integration.rs`

1. Update test expectations: the existing `Straight → Left → Straight` path
   (3 raw turns) becomes a refined sequence. After refinement, the two straights
   at positions 0 and 2 remain as single-edge straights, the left remains.
   Verify `edge_count` and `coordinate_index` values.

---

## 5. Serialization Contract

The new fields serialize naturally via `#[derive(Serialize)]`:

```json
{
  "direction": "straight",
  "angle_degrees": 12.45,
  "edge_count": 47,
  "coordinate_index": 48
}
```

No changes needed in `hanoi-server/src/types.rs` or `engine.rs` — they pass
`Vec<TurnAnnotation>` through without inspecting individual fields.

---

## 6. Test Plan

### Unit Tests (geometry.rs)

| Test | Input | Expected Output |
|------|-------|-----------------|
| `cancel_s_curves_adjacent_opposite_small` | `[Right(-40), Left(+39)]` | `[Straight(-1.0, edge_count=2)]` |
| `cancel_s_curves_adjacent_opposite_large_preserved` | `[Right(-87), Left(+91)]` | `[Right(-87), Left(+91)]` unchanged |
| `cancel_s_curves_separated_by_straight` | `[Right(-40), Straight(0), Left(+39)]` | Unchanged (not adjacent) |
| `cancel_s_curves_net_above_threshold` | `[Left(+55), Right(-30)]` | Unchanged (`|+25| >= 15`) |
| `merge_straights_run` | `[S(2), S(3), S(-1)]` | `[Straight(4, edge_count=3, coord_idx=3)]` |
| `merge_straights_with_turns` | `[S(1), S(2), R(-90), S(0), L(+85)]` | `[S(3, ec=2, ci=2), R(-90, ec=1, ci=3), S(0, ec=1, ci=4), L(+85, ec=1, ci=5)]` |
| `refine_s_curve_then_merge` | `[S(1), R(-40), L(+39), S(2)]` | `[Straight(2.0, edge_count=4, coord_idx=4)]` — S-curve cancelled, then all 4 straights merge |
| `refine_preserves_empty` | `[]` | `[]` |
| `refine_single_turn` | `[Left(+90)]` | `[Left(+90, edge_count=1, coord_idx=1)]` |

### Integration Test Update

The existing synthetic test (`Straight → Left → Straight`, 3 raw turns) should
verify that after refinement:
- 3 entries remain (no S-curves to cancel, no adjacent straights to merge)
- `edge_count` is `[1, 1, 1]`
- `coordinate_index` is `[1, 2, 3]`

---

## 7. Example: Real Data Walkthrough

Using the Hanoi motorcycle route (480 raw turns, 14 non-straight):

**After Pass 1 (S-curve cancellation):**

| Raw Indices | Before | After |
|-------------|--------|-------|
| 431, 432 | Right(-40.0°), Left(+39.3°) | Straight(-0.7°, ec=2) |
| 445, 446 | Left(+36.9°), Right(-36.3°) | Straight(+0.65°, ec=2) |

Other non-straights preserved (all have `max(|a|,|b|) >= 60` or are not
adjacent opposites). Total: 478 entries (480 - 2 cancelled pairs + 2 replacements).

**After Pass 2 (straight merging):**

Approximate output (~24 entries):

```
Straight(cumulative, ec=1,   ci=1)     ← single first edge before first turn
Right(-88.9°,        ec=1,   ci=2)
Straight(cumulative, ec=11,  ci=13)
Right(-87.4°,        ec=1,   ci=14)
Straight(0.27°,      ec=1,   ci=15)
Left(+91.4°,         ec=1,   ci=16)
Straight(cumulative, ec=3,   ci=19)
Right(-92.4°,        ec=1,   ci=20)
Straight(cumulative, ec=8,   ci=28)
Left(+85.5°,         ec=1,   ci=29)
Straight(cumulative, ec=145, ci=174)
Right(-80.4°,        ec=1,   ci=175)
Straight(cumulative, ec=115, ci=290)
Left(+52.9°,         ec=1,   ci=291)
Straight(cumulative, ec=141, ci=432)   ← includes absorbed S-curve from 431-432
...
Left(+85.0°,         ec=1,   ci=478)
Straight(cumulative, ec=1,   ci=479)   ← final edge (no phantom U-turn)
```

From 480 raw entries → ~24 actionable maneuvers.

---

## 8. Edge Cases and Invariants

### Edge Cases

| Case | Behavior |
|------|----------|
| Empty path (0-1 LG nodes) | `compute_turns` returns `[]`; refinement is no-op |
| Single edge pair (2 LG nodes) | 1 raw turn → no adjacent pair for S-curve check → merge is trivial |
| All straights | S-curve pass is no-op → merge collapses to 1 entry |
| All non-straights | S-curve pass checks each pair → merge pass emits each individually |
| Three consecutive non-straights A,B,C where A,B is an S-curve | A,B cancelled → replacement straight is now adjacent to C → NOT re-checked (single forward pass, no cascading) |
| S-curve at start of path | Handled: the forward scan starts at i=0 |
| S-curve at end of path | Handled: the "emit final" clause catches the last element |

### Invariants

1. **Sum of `edge_count` across all output entries = `len(raw_turns)`.**
   No turns are created or destroyed, only reclassified and grouped.
2. **Last entry's `coordinate_index` = `len(coordinates) - 2`.**
   Equivalently, `= len(raw_turns)`. The final coordinate
   (`coordinates[len - 1]`) is the route destination — no turn occurs there,
   so it is unreferenced by any turn annotation.
3. **`coordinate_index` is strictly increasing** across the output sequence.
4. **Non-straight entries always have `edge_count = 1`** (S-curve cancellation
   replaces pairs with straights, never with non-straights).
5. **Pass order matters:** Cancellation before merging ensures cancelled S-curves
   are absorbed into adjacent straight runs. Reversing the order would miss this.

---

## 9. Constants Summary

| Constant | Value | Location | Purpose |
|----------|-------|----------|---------|
| `STRAIGHT_THRESHOLD_DEG` | 25.0 | geometry.rs (existing) | Single-intersection classification |
| `U_TURN_THRESHOLD_DEG` | 155.0 | geometry.rs (existing) | Single-intersection classification |
| `S_CURVE_NET_THRESHOLD_DEG` | 15.0 | geometry.rs (new) | Max residual angle for S-curve cancellation |
| `S_CURVE_MAX_THRESHOLD_DEG` | 60.0 | geometry.rs (new) | Max individual angle for S-curve cancellation |
