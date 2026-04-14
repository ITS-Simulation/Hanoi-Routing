# Turn Refinement Pipeline v3

## Context

The v1 turn post-processing pipeline was wiped after discovering a fundamental
flaw: pair-based S-curve cancellation could accidentally consume a real turn
adjacent to a phantom turn (the Co Linh Rd bug). The codebase currently outputs
raw `compute_turns()` output with no filtering — producing hundreds of phantom
turns from OSM road-curvature discretization.

This plan restores refined turn processing with a cleaner, safer design based on
two key signals: **intersection degree** (topological) and **angle magnitude**
(geometric). It also introduces **roundabout labelling** via a new per-arc flag
propagated through the data pipeline.

### What changed from v1

| v1 feature                | v3 replacement                             |
| ------------------------- | ------------------------------------------ |
| Pair-based S-curve cancel | Run-based degree-2 collapsing              |
| 4-class turn enum         | 8-class graduated enum + roundabout prefix |
| `suppress_close_turns`    | Removed (degree filter handles it)         |
| Roundabout detection      | Explicit OSM tag propagation (new)         |
| Strip straights blindly   | Strip straights, but keep leading one      |

---

## Step 0 — Roundabout flag propagation (pipeline change)

### Problem

Roundabout detection at query time is unreliable without OSM data — heuristics
based on turn patterns are indistinguishable from dense urban zigzags. The
`junction=roundabout` OSM tag is the authoritative signal, but it's currently
discarded during graph construction.

### Solution

Capture the roundabout flag during graph generation in CCH-Generator (the
`way_callback` already receives the full `TagMap`) and propagate it through the
pipeline as a per-arc binary vector.

### Data flow

```
OSM PBF
  → CCH-Generator way_callback: check junction=roundabout tag
    → way_is_roundabout[routing_way_id] = true/false
      → expand to per-arc: is_arc_roundabout[arc] = way_is_roundabout[way[arc]]
        → save as binary file: <graph_dir>/is_arc_roundabout (u8 × num_arcs)
          → generate_line_graph: load & propagate (LG node = original arc)
            → save as: <line_graph_dir>/is_arc_roundabout (u8 × num_lg_nodes)
              → LineGraphCchContext loads at startup
                → compute_turns reads per-turn
```

### 0a. CCH-Generator changes

**File: `CCH-Generator/src/generate_graph.cpp`**

Add `way_is_roundabout` vector alongside existing `way_speed`:

```cpp
std::vector<unsigned>way_speed(mapping.is_routing_way.population_count());
std::vector<bool>way_is_roundabout(mapping.is_routing_way.population_count(), false);  // NEW
```

In the `way_callback` lambda (line ~164), capture the roundabout tag:

```cpp
[&](uint64_t osm_way_id, unsigned routing_way_id, const RoutingKit::TagMap&way_tags){
    way_speed[routing_way_id] = profile_callbacks.get_way_speed(osm_way_id, way_tags, log_fn);

    // NEW: capture roundabout flag
    const char* junction = way_tags["junction"];
    if(junction != nullptr && std::string(junction) == "roundabout")
        way_is_roundabout[routing_way_id] = true;

    return profile_callbacks.get_direction(osm_way_id, way_tags, log_fn);
}
```

After graph construction, expand per-way flag to per-arc and save:

```cpp
// Expand way-level roundabout flag to arc-level
std::vector<uint8_t> is_arc_roundabout(routing_graph.head.size());
for(std::size_t a = 0; a < routing_graph.head.size(); ++a)
    is_arc_roundabout[a] = way_is_roundabout[routing_graph.way[a]] ? 1 : 0;
```

Add `is_arc_roundabout` to `GeneratedGraph` struct and `save_graph()`:

```cpp
struct GeneratedGraph{
    // ... existing fields ...
    std::vector<uint8_t>is_arc_roundabout;  // NEW
};

// In save_graph():
save_named_vector(output_dir, "is_arc_roundabout", graph.is_arc_roundabout);
```

### 0b. Line graph generator changes

**File: `CCH-Hanoi/crates/hanoi-tools/src/bin/generate_line_graph.rs`**

After loading the base graph, load the roundabout flag:

```rust
let is_arc_roundabout: Vec<u8> = Vec::load_from(graph_path.join("is_arc_roundabout"))?;
assert_eq!(is_arc_roundabout.len(), num_arcs);
```

Each LG node = original arc, so copy directly. For split nodes, inherit from
the original:

```rust
let mut lg_is_roundabout: Vec<u8> = is_arc_roundabout.clone();
for &original in &split_result.split_map {
    lg_is_roundabout.push(is_arc_roundabout[original as usize]);
}
// Save to line graph output directory
lg_is_roundabout.write_to(&output_dir.join("is_arc_roundabout"))?;
```

### 0c. LineGraphCchContext loading

**File: `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`**

Add field to `LineGraphCchContext`:

```rust
/// Per-LG-node flag: true if the original arc belongs to a roundabout way.
pub is_arc_roundabout: Vec<u8>,
```

In `load_and_build()`, load with graceful fallback (so existing datasets without
the file still work):

```rust
let is_arc_roundabout: Vec<u8> = Vec::load_from(line_graph_dir.join("is_arc_roundabout"))
    .unwrap_or_else(|_| vec![0u8; num_lg_nodes]);
```

No split-node extension is needed here — `generate_line_graph` (Step 0b)
already propagated split nodes into the saved file, so the loaded vector
covers all LG nodes (base + split) directly.

### 0d. Usage in compute_turns

Pass `is_arc_roundabout` to `compute_turns()`. For each turn (transition from
`edge_a` to `edge_b`), a turn is roundabout-related if **either** arc is a
roundabout arc:

```rust
let is_roundabout = is_arc_roundabout[edge_a] != 0 || is_arc_roundabout[edge_b] != 0;
```

This captures both entry turns (normal → roundabout) and exit turns
(roundabout → normal).

---

## Step 1 — Eight-class turn classification with roundabout prefix

### Enum expansion

Replace the 4-variant `TurnDirection` with 16 variants (8 base + 8
roundabout-prefixed):

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
    RoundaboutStraight,
    RoundaboutSlightLeft,
    RoundaboutSlightRight,
    RoundaboutLeft,
    RoundaboutRight,
    RoundaboutSharpLeft,
    RoundaboutSharpRight,
    RoundaboutUTurn,
}
```

`#[serde(rename_all = "snake_case")]` produces: `"slight_left"`,
`"sharp_right"`, `"roundabout_left"`, `"roundabout_u_turn"`, etc.

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

If `is_roundabout` is true, map to the corresponding `Roundabout*` variant.

### Updated classify_turn signature

```rust
pub fn classify_turn(angle_degrees: f64, cross: f64, is_roundabout: bool) -> TurnDirection
```

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
     Use `net_angle.signum()` as the synthetic cross product for
     `classify_turn` (positive = left, negative = right).
   - **Roundabout inheritance**: if **any** turn in the collapsed sub-run had
     a `Roundabout*` direction, the replacement turn (or the Straight that
     replaces a zeroed-out sub-run) inherits the roundabout flag. This
     preserves roundabout context through interior ring curves.

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

After distance annotation, remove **non-roundabout `Straight`** entries from the
output. Regular Straights carry no navigational value — the `distance_to_next_m`
on the preceding real turn already tells the driver how far until the next
action.

**`RoundaboutStraight` is never stripped** — "continue straight through the
roundabout" is an active navigation instruction (the driver must choose the
correct exit), unlike a regular Straight which means "just keep driving."

### Distance absorption

When removing a Straight, its `distance_to_next_m` is absorbed into the nearest
preceding **retained** turn. Iterate the turn list in order:

- If a Straight is removed and there is a preceding retained turn, add the
  Straight's `distance_to_next_m` to it.
- If a Straight is removed at the **start** of the list (before any retained
  turn), it becomes a candidate for the leading straight (see below).

### Preserve leading straight

**Exception**: if the route begins with one or more `Straight` entries before the
first non-Straight turn, keep a **single leading Straight** so the driver knows
"head straight for X meters" at the start of navigation. Subsequent leading
Straights are absorbed into this first one — its `distance_to_next_m` covers
the full distance from route start to the first real turn.

### Edge case: all-straight route

If the route has no non-Straight turns at all (completely straight route), return
a single `Straight` with `distance_to_next_m` set to the total route distance.

---

## Step 6 — Integration

### Wrapper function

```rust
pub fn refine_turns(turns: &mut Vec<TurnAnnotation>, coordinates: &[(f32, f32)]) {
    collapse_degree2_curves(turns);         // Step 2
    merge_straights(turns);                 // Step 3
    annotate_distances(turns, coordinates); // Step 4
    strip_straights(turns);                 // Step 5
}
```

Single entry point. Called after `compute_turns()` in both `query()` and
`query_trimmed()`, after coordinates are built.

### Updated compute_turns signature

```rust
pub fn compute_turns(
    lg_path: &[NodeId],
    original_tail: &[NodeId],
    original_head: &[NodeId],
    original_first_out: &[EdgeId],
    original_lat: &[f32],
    original_lng: &[f32],
    is_arc_roundabout: &[u8],          // NEW
) -> Vec<TurnAnnotation>
```

### Call sites in `line_graph.rs`

Both `query()` and `query_trimmed()` change from:

```rust
let turns = compute_turns(/* ... */);
```

to:

```rust
let mut turns = compute_turns(/* ... including is_arc_roundabout */);
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

| File                                                            | Changes                                                                                                  |
| --------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `CCH-Generator/src/generate_graph.cpp`                          | Capture `junction=roundabout` in way callback, expand to per-arc, add to struct, save                    |
| `CCH-Hanoi/crates/hanoi-tools/src/bin/generate_line_graph.rs`   | Load `is_arc_roundabout`, propagate through split map, save to line graph dir                            |
| `CCH-Hanoi/crates/hanoi-core/src/geometry.rs`                   | Enum expansion (16 variants), new thresholds, updated classify_turn, 4 new post-processing functions, `refine_turns` wrapper |
| `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`                 | Load `is_arc_roundabout` in context, pass to `compute_turns`, integrate `refine_turns()`                 |
| `docs/CHANGELOGS.md`                                            | Document all changes                                                                                     |

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
/// Classify a turn angle into one of 16 directions (8 base + 8 roundabout).
pub fn classify_turn(angle_degrees: f64, cross: f64, is_roundabout: bool) -> TurnDirection;

/// Collapse degree-2 curvature noise using run-based analysis.
fn collapse_degree2_curves(turns: &mut Vec<TurnAnnotation>);

/// Merge consecutive Straight entries into one.
fn merge_straights(turns: &mut Vec<TurnAnnotation>);

/// Populate distance_to_next_m via Haversine summation.
fn annotate_distances(turns: &mut Vec<TurnAnnotation>, coordinates: &[(f32, f32)]);

/// Strip interior Straights (not RoundaboutStraight); preserve leading Straight.
fn strip_straights(turns: &mut Vec<TurnAnnotation>);

/// Full post-processing pipeline (calls all four above in order).
pub fn refine_turns(turns: &mut Vec<TurnAnnotation>, coordinates: &[(f32, f32)]);
```

---

## TurnAnnotation struct

```rust
pub struct TurnAnnotation {
    pub direction: TurnDirection,
    pub angle_degrees: f64,
    #[serde(skip)]
    pub coordinate_index: u32,
    pub distance_to_next_m: f64,
    #[serde(skip)]
    pub intersection_degree: u32,
}
```

No new fields needed — the roundabout information is encoded in the
`TurnDirection` enum variant itself.

---

## Testable invariants

1. After `collapse_degree2_curves`: every non-Straight turn has either
   `intersection_degree >= 3` or `|angle| >= 40°`.
2. After `merge_straights`: no two consecutive turns are both `Straight`.
3. After `annotate_distances`: `distance_to_next_m > 0` for all turns (except
   possibly the last, which covers distance to route end and may be 0 for
   trivial paths).
4. After `strip_straights`: no interior `Straight` entries (but
   `RoundaboutStraight` may appear anywhere). At most one leading `Straight`
   (index 0). If a leading `Straight` is present, the next entry is
   non-Straight.
5. Sum of all `distance_to_next_m` across final turns ≈ total route distance
   (within floating-point tolerance).
6. Every `Roundabout*` variant has a corresponding arc with
   `is_arc_roundabout != 0` in the input data.

---

## Pipeline rebuild required

After implementing Step 0, the full pipeline must be re-run to produce the new
`is_arc_roundabout` binary file:

```bash
# Re-run CCH-Generator (produces is_arc_roundabout alongside existing files)
CCH-Generator/lib/cch_generator <pbf> <output_dir> --profile motorcycle

# Re-run line graph generation (propagates roundabout flag)
cargo run --release -p hanoi-tools --bin generate_line_graph -- <graph_dir>

# Re-run IFC ordering (unchanged, but needed for fresh line graph)
flow_cutter_cch_order.sh <graph_dir>
```

Existing datasets without `is_arc_roundabout` continue to work — the loading
code falls back to an all-zeros vector (no roundabouts detected).

---

## Verification

After implementation:
- `cargo check --workspace` from `CCH-Hanoi/` must pass
- `CCH-Generator` must compile: `cd CCH-Generator/build && make`
- Inspect GeoJSON output from test queries to verify:
  - Phantom slight veers on straight roads are collapsed
  - Sharp alley bends at degree-2 nodes are preserved
  - Leading "head straight" annotation appears when route starts straight
  - `distance_to_next_m` values are populated and sum to ~route distance
  - Roundabout turns show `roundabout_*` direction labels
- Cross-reference roundabout output against `Maps/hn_round.osm` (557 ways)
