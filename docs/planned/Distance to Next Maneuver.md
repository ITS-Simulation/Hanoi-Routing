# Distance to Next Maneuver

## Context

Turn annotations currently tell the user *what* to do and *where* (direction,
angle, coordinate_index), but not *how far* until the next action. Navigation
systems universally show "in 200m, turn left" — this requires a
`distance_to_next_m` field on each turn annotation. The coordinates array
already contains all the data needed; this is a post-processing step.

---

## 1. Current State

### TurnAnnotation (geometry.rs:20-36)

```rust
pub struct TurnAnnotation {
    pub direction: TurnDirection,
    pub angle_degrees: f64,
    pub edge_count: u32,
    pub coordinate_index: u32,
}
```

### How turns and coordinates relate

After `refine_turns()`, each annotation has a `coordinate_index` pointing into
the `coordinates[]` array. For a route with coordinates `[c0, c1, c2, c3, c4]`
and turns `[T0(idx=2), T1(idx=4)]`:

```
c0 ──── c1 ──── c2 ──── c3 ──── c4
                 ↑                ↑
                T0(idx=2)       T1(idx=4)
```

The distance from T0 to T1 = `haversine(c2,c3) + haversine(c3,c4)`.
The distance from T1 to the route end = 0 (T1 is at the last coordinate with a
turn; remaining distance to the final coordinate is the "distance after last
maneuver").

### Where turns are assembled with coordinates

Both `query()` and `query_trimmed()` in `line_graph.rs` build `turns` and
`coordinates` from the same path, then package them into `QueryAnswer`. The
distance computation is a post-processing step that runs after both are ready.

---

## 2. Design

### 2.1 New field on TurnAnnotation

```rust
pub struct TurnAnnotation {
    pub direction: TurnDirection,
    pub angle_degrees: f64,
    pub edge_count: u32,
    pub coordinate_index: u32,
    /// Distance in meters from this maneuver to the next maneuver (or to the
    /// route end for the last entry).
    pub distance_to_next_m: f64,
}
```

Since `TurnAnnotation` derives `Serialize`, the field appears in JSON/GeoJSON
output automatically.

### 2.2 New function: `annotate_distances()`

Add to `geometry.rs`:

```rust
/// Assign `distance_to_next_m` to each turn annotation using the
/// coordinates array. Each entry gets the haversine distance from its
/// coordinate_index to the next entry's coordinate_index (or to the end
/// of the coordinates array for the last entry).
pub fn annotate_distances(
    turns: &mut [TurnAnnotation],
    coordinates: &[(f32, f32)],
)
```

**Algorithm:**

```
for i in 0..turns.len():
    start = turns[i].coordinate_index as usize
    end   = if i + 1 < turns.len():
                turns[i + 1].coordinate_index as usize
            else:
                coordinates.len() - 1

    turns[i].distance_to_next_m = sum of haversine_m(
        coordinates[j], coordinates[j+1]
    ) for j in start..end
```

This is O(N) over the coordinates — each coordinate pair is visited exactly
once across all turns.

### 2.3 Placeholder value in pipeline

`compute_turns()`, `cancel_s_curves()`, and `merge_straights()` all construct
`TurnAnnotation` with `distance_to_next_m: 0.0` as a placeholder (same pattern
as `coordinate_index: 0`). The real value is assigned by `annotate_distances()`
after `refine_turns()` returns and coordinates are available.

### 2.4 Call site in query methods

In both `query()` and `query_trimmed()` (line_graph.rs), insert one call after
turns and coordinates are both built:

```rust
let mut turns = refine_turns(compute_turns(...));
// ... build coordinates ...
annotate_distances(&mut turns, &coordinates);
```

---

## 3. Files to Modify

| File | Change |
|------|--------|
| `hanoi-core/src/geometry.rs` | Add `distance_to_next_m: f64` to `TurnAnnotation`. Add `annotate_distances()`. Update all `TurnAnnotation { ... }` constructors in `compute_turns`, `cancel_s_curves`, `merge_straights` to include `distance_to_next_m: 0.0`. |
| `hanoi-core/src/line_graph.rs` | Import `annotate_distances`. Call it in `query()` and `query_trimmed()` after coordinates are built. |
| `docs/CHANGELOGS.md` | Log changes. |

**Not modified:**
- `cch.rs` — normal graph engine has no turns (empty vec), nothing to annotate
- `hanoi-server/*` — `TurnAnnotation` serialization is automatic via `Serialize`
- `hanoi-cli/*` — same
- `bounds.rs`, `spatial.rs` — unrelated

---

## 4. Edge Cases

| Case | Behavior |
|------|----------|
| Empty turns vec | `annotate_distances` is a no-op (loop doesn't execute) |
| Single turn | `distance_to_next_m` = haversine sum from `coordinate_index` to end of coordinates |
| Last turn at final coordinate | `start == end`, sum is 0.0 |
| Trimmed path with 0 turns | No turns to annotate, function is a no-op |

---

## 5. Example Output

For a route: `c0 → c1 → c2(right) → c3 → c4 → c5(left) → c6`

```json
{
  "turns": [
    {
      "direction": "right",
      "angle_degrees": -87.3,
      "edge_count": 1,
      "coordinate_index": 2,
      "distance_to_next_m": 340.5
    },
    {
      "direction": "left",
      "angle_degrees": 92.1,
      "edge_count": 1,
      "coordinate_index": 5,
      "distance_to_next_m": 85.2
    }
  ]
}
```

Here, 340.5m = `haversine(c2,c3) + haversine(c3,c4) + haversine(c4,c5)`, and
85.2m = `haversine(c5,c6)`.
