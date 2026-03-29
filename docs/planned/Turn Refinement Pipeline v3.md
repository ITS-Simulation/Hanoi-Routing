# Turn Refinement Pipeline v3

## Context

The v1 turn post-processing pipeline was wiped after discovering a fundamental
flaw: pair-based S-curve cancellation could accidentally consume a real turn
adjacent to a phantom turn (the Co Linh Rd bug). The codebase currently outputs
raw `compute_turns()` output with no filtering — producing hundreds of phantom
turns from OSM road-curvature discretization.

This plan restores refined turn processing with a cleaner, safer design based on
two key signals: **intersection degree** (topological) and **angle magnitude**
(geometric).

### What changed from v1

| v1 feature                | v3 replacement                        |
| ------------------------- | ------------------------------------- |
| Pair-based S-curve cancel | Run-based degree-2 collapsing         |
| 4-class turn enum         | 8-class graduated enum                |
| `suppress_close_turns`    | Removed (degree filter handles it)    |
| Roundabout detection      | Deferred (needs OSM tag propagation)  |
| Strip straights blindly   | Strip straights, but keep leading one |

---

## Step 1 — Eight-class turn classification

### Enum expansion

Replace the 4-variant `TurnDirection` with 8 variants:

```rust
pub enum TurnDirection {
    Straight,
    SlightLeft,
    SlightRight,
    Left,
    Right,
    SharpLeft,
    SharpRight,
    UTurn,
}
```

`#[serde(rename_all = "snake_case")]` produces: `"slight_left"`, `"sharp_right"`,
`"u_turn"`, etc.

### New thresholds

```rust
const STRAIGHT_THRESHOLD_DEG: f64 = 15.0;
const SLIGHT_THRESHOLD_DEG: f64 = 40.0;
const SHARP_THRESHOLD_DEG: f64 = 110.0;
const U_TURN_THRESHOLD_DEG: f64 = 155.0;
```

### Classification logic

```
|angle| < 15°           → Straight
15° ≤ |angle| < 40°     → SlightLeft / SlightRight
40° ≤ |angle| < 110°    → Left / Right
110° ≤ |angle| < 155°   → SharpLeft / SharpRight
|angle| ≥ 155°          → UTurn
```

Cross product sign determines left (cross > 0) vs right (cross < 0).

### Rationale

- **15° straight ceiling**: Tighter than the old 25°. A 20° drift is
  perceptible but never a conscious maneuver. 15° absorbs GPS/OSM noise.
- **40° veer→turn boundary**: Below ~40° the steering adjustment is minor
  ("bear left"). Above it, the driver is clearly turning.
- **110° turn→sharp boundary**: Beyond ~110° requires significant deceleration.
  Matches Google Maps' "sharp turn" iconography.
- **155° U-turn**: Unchanged from v1, matches OSRM/Valhalla.

---

## Step 2 — Degree-2 curve collapsing (run-based)

### Problem

A degree-2 node has exactly one road in and one road out — no branching. Most
turns at degree-2 nodes are OSM road-curvature noise (smooth curves discretized
into short edges). However, degree-2 turns **can** be genuine: sharp alley
corners, switchbacks, or hairpin bends where the road itself turns sharply.

### Key insight

A **Slight** turn (15–40°) at degree-2 is almost always curvature noise — roads
veer gently to follow terrain, rivers, property lines. A **standard turn**
(40°+) at degree-2 is almost always a genuine road bend.

### Algorithm

1. Scan the turn list for **maximal runs** of consecutive turns where every turn
   has `intersection_degree == 2`.

2. A degree-3+ turn (or the list boundary) always **breaks** a run. This makes
   it impossible to accidentally touch a real intersection turn.

3. Within each degree-2 run, identify the **anchor turns**: any turn with
   `|angle| >= SLIGHT_THRESHOLD_DEG` (40°+). These are genuine bends — keep
   them.

4. Between anchors (or between run boundaries and anchors), collapse the
   **Slight sub-runs**:
   - Compute the net angle: sum of `angle_degrees` across the sub-run.
   - If `|net_angle| < STRAIGHT_THRESHOLD_DEG` (15°): the sub-run nets to
     roughly straight → remove all turns in the sub-run.
   - If `|net_angle| >= STRAIGHT_THRESHOLD_DEG`: the road genuinely drifts →
     replace the sub-run with a **single turn** classified by the net angle,
     placed at the coordinate index of the last turn in the sub-run.

### Worked example

```
Raw turns at degree-2 nodes:
  [SlightRight -18°, SlightLeft +22°, Right -72°, SlightLeft +15°, SlightRight -17°]

Anchors: Right -72° (index 2)

Sub-run before anchor: [SlightRight -18°, SlightLeft +22°]
  Net = +4° → |4| < 15 → collapse to nothing (road went straight)

Anchor kept: Right -72°

Sub-run after anchor: [SlightLeft +15°, SlightRight -17°]
  Net = -2° → |2| < 15 → collapse to nothing

Result: [Right -72°]  ← only the genuine alley bend survives
```

### Invariant

After this pass, every remaining non-Straight turn either:
- Has `intersection_degree >= 3` (real intersection), OR
- Has `|angle| >= 40°` at degree-2 (genuine road bend)

---

## Step 3 — Straight merging

Collapse consecutive `Straight` entries into a single `Straight`. Update
`coordinate_index` to point to the last merged entry's index (preserving the
span for distance computation).

This is purely cosmetic cleanup — reduces output verbosity.

---

## Step 4 — Distance annotation

Populate `distance_to_next_m` on each turn using Haversine summation over the
coordinate path.

```
For each turn[i]:
  start = turn[i].coordinate_index
  end   = turn[i+1].coordinate_index  (or coordinates.len()-1 for the last turn)
  distance_to_next_m = Σ haversine(coords[j], coords[j+1]) for j in start..end
```

Uses existing `haversine_m()` from `spatial.rs`.

---

## Step 5 — Strip straights & leading-straight handling

### Strip interior straights

After distance annotation, remove `Straight` entries from the output. Straights
carry no navigational value — the `distance_to_next_m` on the preceding real
turn already tells the driver how far until the next action.

When removing a Straight, its `distance_to_next_m` is **absorbed** into the
preceding turn's distance (or, if there is no preceding turn, into the leading
straight — see below).

### Preserve leading straight

**Exception**: if the route begins with one or more `Straight` entries before the
first real turn, keep a **single leading Straight** so the driver knows "head
straight for X meters" at the start of navigation. Its `distance_to_next_m`
covers the entire straight segment from route start to the first real turn.

### Edge case: all-straight route

If the route has no non-Straight turns at all (completely straight route), return
a single `Straight` with `distance_to_next_m` set to the total route distance.

---

## Step 6 — Integration

### Wrapper function

```rust
pub fn refine_turns(turns: &mut Vec<TurnAnnotation>, coordinates: &[(f32, f32)]) {
    collapse_degree2_curves(turns);    // Step 2
    merge_straights(turns);            // Step 3
    annotate_distances(turns, coordinates); // Step 4
    strip_straights(turns);            // Step 5
}
```

Single entry point. Called after `compute_turns()` in both `query()` and
`query_trimmed()`, after coordinates are built.

### Call sites in `line_graph.rs`

Both `query()` and `query_trimmed()` currently do:

```rust
let turns = compute_turns(/* ... */);
```

Change to:

```rust
let mut turns = compute_turns(/* ... */);
refine_turns(&mut turns, &coordinates);
```

The `coordinates` vec is already built by the time turns are assigned, so we
just need to reorder slightly: build coordinates first, then compute and refine
turns.

### Import changes

`line_graph.rs` import changes from:
```rust
use crate::geometry::compute_turns;
```
to:
```rust
use crate::geometry::{compute_turns, refine_turns};
```

### `lib.rs` re-export

Current: `pub use geometry::{TurnAnnotation, TurnDirection};`

No change needed — the new enum variants are part of the same type.

---

## Files modified

| File                                          | Changes                                                                |
| --------------------------------------------- | ---------------------------------------------------------------------- |
| `CCH-Hanoi/crates/hanoi-core/src/geometry.rs` | Enum expansion (8 variants), new thresholds, new classify_turn, 4 new post-processing functions, `refine_turns` wrapper |
| `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs` | Integrate `refine_turns()` in `query()` and `query_trimmed()`        |
| `docs/CHANGELOGS.md`                          | Document all changes                                                   |

---

## Constants summary

```rust
const STRAIGHT_THRESHOLD_DEG: f64 = 15.0;    // below = Straight
const SLIGHT_THRESHOLD_DEG: f64 = 40.0;      // below = Slight, above = standard turn
const SHARP_THRESHOLD_DEG: f64 = 110.0;      // above = Sharp
const U_TURN_THRESHOLD_DEG: f64 = 155.0;     // above = UTurn
```

---

## New function signatures

```rust
/// Collapse degree-2 curvature noise using run-based analysis.
fn collapse_degree2_curves(turns: &mut Vec<TurnAnnotation>);

/// Merge consecutive Straight entries into one.
fn merge_straights(turns: &mut Vec<TurnAnnotation>);

/// Populate distance_to_next_m via Haversine summation.
fn annotate_distances(turns: &mut Vec<TurnAnnotation>, coordinates: &[(f32, f32)]);

/// Strip interior Straights; preserve leading Straight if present.
fn strip_straights(turns: &mut Vec<TurnAnnotation>);

/// Full post-processing pipeline (calls all four above in order).
pub fn refine_turns(turns: &mut Vec<TurnAnnotation>, coordinates: &[(f32, f32)]);
```

---

## Testable invariants

1. After `collapse_degree2_curves`: every non-Straight turn has either
   `intersection_degree >= 3` or `|angle| >= 40°`.
2. After `merge_straights`: no two consecutive turns are both `Straight`.
3. After `annotate_distances`: `distance_to_next_m > 0` for all turns (except
   possibly the last, which covers distance to route end and may be 0 for
   trivial paths).
4. After `strip_straights`: no interior `Straight` entries. At most one leading
   `Straight` (index 0). If present, the next entry is non-Straight.
5. Sum of all `distance_to_next_m` across final turns ≈ total route distance
   (within floating-point tolerance).

---

## Verification

After implementation:
- `cargo check --workspace` from `CCH-Hanoi/` must pass
- Inspect GeoJSON output from test queries to verify:
  - Phantom slight veers on straight roads are collapsed
  - Sharp alley bends at degree-2 nodes are preserved
  - Leading "head straight" annotation appears when route starts straight
  - `distance_to_next_m` values are populated and sum to ~route distance
