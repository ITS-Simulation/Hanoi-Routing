# Turn Direction Detection — Implementation Plan

**Module:** `CCH-Hanoi/crates/hanoi-core/` + `CCH-Hanoi/crates/hanoi-server/`
**Status:** Not started
**Goal:** Detect turn directions (left, right, straight, U-turn) at each
intersection along a line-graph query path and embed them in the GeoJSON
response `properties`

---

## 1. Why The Line Graph Makes This Possible

In a normal graph, a query path is a sequence of **intersection nodes**. Between
two intersection nodes there may be parallel edges (e.g., a divided road), so
you cannot determine which physical road segment was used — bearing computation
is ambiguous.

In a line graph, a query path is a sequence of **original edge IDs** (because
line-graph node N = original edge N). Each original edge has a definite
`(tail, head)` pair with known coordinates. For any two consecutive line-graph
nodes `(lg_i, lg_i+1)`, the transition represents a **turn** from one road
segment to the next. The geometry is fully determined:

```
Segment A:  original_tail[lg_i]   → original_head[lg_i]
Segment B:  original_tail[lg_i+1] → original_head[lg_i+1]
Turn point: original_head[lg_i] = original_tail[lg_i+1]  (shared intersection)
```

This means turn detection is **only available for line-graph queries**. Normal
graph queries will not include turn annotations.

---

## 2. Angle Computation — Two Approaches Compared

### Approach A: Bearing Difference

Compute the forward azimuth (bearing) of each road segment independently, then
take the difference.

```
bearing(P1, P2) = atan2(sin(Δλ)·cos(φ₂),
                        cos(φ₁)·sin(φ₂) − sin(φ₁)·cos(φ₂)·cos(Δλ))

turn_angle = normalize(bearing_B − bearing_A)    // → [-180, +180]
```

**Pros:**

- Standard geodetic formula, works globally
- Correctly handles short and long segments alike
- Each bearing is independently meaningful (compass direction)

**Cons:**

- Requires careful angle normalization (wraparound at ±180°)
- Two `atan2` calls + one subtraction + normalization per turn
- The bearing of a segment depends on the segment's length and curvature; for
  very short segments the bearing can be noisy (amplifies GPS coordinate
  imprecision)

### Approach B: Dot Product + Cross Product → atan2 (Recommended)

Treat each road segment as a 2D vector in a local equirectangular projection,
then compute the signed angle directly from the vector pair.

```
// Local equirectangular projection (accurate for short urban segments)
cos_lat = cos(turn_point.latitude)

// Direction vectors in projected (East, North) space
//   x = Easting  = Δlng · cos(lat)
//   y = Northing = Δlat
A = ((head_A.lng − tail_A.lng) · cos_lat,  head_A.lat − tail_A.lat)
B = ((head_B.lng − tail_B.lng) · cos_lat,  head_B.lat − tail_B.lat)

dot   = A.x·B.x + A.y·B.y       // measures alignment
cross = A.x·B.y − A.y·B.x       // measures signed perpendicularity

angle = atan2(cross, dot)         // signed angle in radians, [-π, π]
```

**Axis convention:** x = East, y = North. This is the standard math/ENU
(East-North-Up) convention where counter-clockwise is positive. With this
orientation, the 2D cross product `A×B` is positive when B is to the **left**
of A (counter-clockwise), matching the driver's perspective.

**Turn direction from `cross` product sign:**

- `cross > 0` → **left turn** (B veers left of A)
- `cross < 0` → **right turn** (B veers right of A)
- `|angle|` near 0 → **straight**
- `|angle|` near π → **U-turn**

**Pros:**

- Single `atan2` call per turn (vs. two for bearing approach)
- No angle normalization needed — `atan2` returns [-π, π] directly
- The cross product sign gives left/right immediately without thresholds
- Natural for 2D vector geometry; conceptually simpler
- Equirectangular projection is already used in `spatial.rs`
  (`haversine_perpendicular_distance_with_t`) so it's a proven pattern in this
  codebase

**Cons:**

- Requires projecting to local planar coordinates (trivial for Hanoi-scale
  distances)
- Equirectangular approximation breaks down near the poles (irrelevant for Hanoi
  at ~21°N)

### Decision: Use Approach B

The dot/cross product approach is more efficient (one `atan2` per turn vs. two),
avoids angle-wraparound bugs, and the cross product sign directly gives the turn
direction. It also aligns with the equirectangular projection pattern already
established in `spatial.rs`.

---

## 3. Turn Classification Thresholds

```
|angle| < 25°                → "straight"
25° ≤ angle and cross > 0   → "left"     (angle in [25°, 155°])
25° ≤ angle and cross < 0   → "right"    (angle in [-155°, -25°])
|angle| ≥ 155°              → "u_turn"
```

These thresholds (25° for straight, 155° for U-turn) are industry-standard
values used in OSRM and Valhalla. They can be tuned later.

**Threshold constants should be defined as `const` values** in the geometry
module so they're easy to find and adjust.

---

## 4. Data Structures

### New types (in `hanoi-core`)

```rust
/// Classification of a turn maneuver at an intersection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnDirection {
    Straight,
    Left,
    Right,
    UTurn,
}

/// A single turn annotation along a route.
#[derive(Debug, Clone, Serialize)]
pub struct TurnAnnotation {
    /// Classified turn direction.
    pub direction: TurnDirection,

    /// Signed turn angle in degrees. Positive = left, negative = right.
    /// Range: [-180, 180].
    pub angle_degrees: f64,
}
```

### Extended QueryAnswer (in `hanoi-core/src/cch.rs`)

```rust
pub struct QueryAnswer {
    pub distance_ms: Weight,
    pub distance_m: f64,
    pub path: Vec<NodeId>,
    pub coordinates: Vec<(f32, f32)>,
    /// Turn annotations along the path.
    /// Empty for normal-graph queries (no turn info available).
    pub turns: Vec<TurnAnnotation>,
}
```

---

## 5. Files to Modify

All changes are within `CCH-Hanoi/` only (no RoutingKit or rust_road_router
changes).

### Step 1: New geometry module — `hanoi-core/src/geometry.rs`

Create a new module with:

- `compute_turn_angle(tail_a, head_a, head_b, lat, lng) -> (f64, f64)` — returns
  `(angle_radians, cross_product)` using the dot/cross approach with
  equirectangular projection
- `classify_turn(angle_degrees, cross) -> TurnDirection` — applies thresholds
- `TurnDirection` enum and `TurnAnnotation` struct (as defined above)
- `compute_turns(lg_path, original_tail, original_head, original_lat, original_lng) -> Vec<TurnAnnotation>`
  — the main entry point that iterates over consecutive line-graph node pairs

**Why a separate module:** This is geometric math, distinct from the spatial
indexing in `spatial.rs` and the CCH logic in `line_graph.rs`. It keeps
`line_graph.rs` focused on the CCH query mechanics.

### Step 2: Register module — `hanoi-core/src/lib.rs`

Add `pub mod geometry;` and re-export `TurnDirection` and `TurnAnnotation`.

### Step 3: Extend QueryAnswer — `hanoi-core/src/cch.rs`

Add `pub turns: Vec<TurnAnnotation>` field to `QueryAnswer`. Update all
construction sites:

| Location | Change |
|----------|--------|
| `cch.rs` → `QueryEngine::query()` | Set `turns: vec![]` (normal graph, no turn info) |
| `cch.rs` → `QueryEngine::query_coords()` | Same — empty turns |
| `line_graph.rs` → `LineGraphQueryEngine::query()` | Call `compute_turns()` on the `lg_path` before coordinate mapping, set result |

### Step 4: Embed turns in GeoJSON — `hanoi-server/src/engine.rs`

Modify `answer_to_geojson()` to include the `turns` array in `properties` when
it is non-empty:

```json
{
  "properties": {
    "distance_ms": 5000,
    "distance_m": 2500.5,
    "turns": [
      { "direction": "left", "angle_degrees": 87.4 },
      { "direction": "straight", "angle_degrees": 5.2 }
    ]
  }
}
```

Also update `answer_to_response()` (`?format=json`) to include turns in the
`QueryResponse` struct in `types.rs`.

### Step 5: Update QueryResponse — `hanoi-server/src/types.rs`

Add a `turns` field to `QueryResponse`:

```rust
pub struct QueryResponse {
    pub distance_ms: Option<Weight>,
    pub distance_m: Option<f64>,
    pub path_nodes: Vec<u32>,
    pub coordinates: Vec<[f32; 2]>,
    pub turns: Vec<TurnAnnotation>,   // new
}
```

---

## 6. Detailed Algorithm — `compute_turns()`

```rust
pub fn compute_turns(
    lg_path: &[NodeId],
    original_tail: &[NodeId],
    original_head: &[NodeId],
    original_lat: &[f32],
    original_lng: &[f32],
) -> Vec<TurnAnnotation> {
    let mut turns = Vec::new();

    for i in 0..lg_path.len().saturating_sub(1) {
        let edge_a = lg_path[i] as usize;
        let edge_b = lg_path[i + 1] as usize;

        // Segment A: tail_a → head_a
        let tail_a = original_tail[edge_a] as usize;
        let head_a = original_head[edge_a] as usize;

        // Segment B: tail_b → head_b
        let tail_b = original_tail[edge_b] as usize;
        let head_b = original_head[edge_b] as usize;

        // Turn point is head_a = tail_b (the shared intersection)
        let turn_lat = original_lat[head_a];

        // Direction vectors in equirectangular (East, North) projection
        let cos_lat = (turn_lat as f64).to_radians().cos();

        // x = Easting (Δlng · cos_lat), y = Northing (Δlat)
        let ax = (original_lng[head_a] - original_lng[tail_a]) as f64 * cos_lat;
        let ay = (original_lat[head_a] - original_lat[tail_a]) as f64;

        let bx = (original_lng[head_b] - original_lng[tail_b]) as f64 * cos_lat;
        let by = (original_lat[head_b] - original_lat[tail_b]) as f64;

        let dot = ax * bx + ay * by;
        let cross = ax * by - ay * bx;

        let angle_rad = cross.atan2(dot);
        let angle_deg = angle_rad.to_degrees();

        let direction = classify_turn(angle_deg, cross);

        turns.push(TurnAnnotation {
            direction,
            angle_degrees: angle_deg,
        });
    }

    turns
}
```

### Ordering note

The `turns` vector has exactly `lg_path.len() - 1` entries — one for each
consecutive pair of line-graph edges. Entry `turns[i]` corresponds to the
transition from `lg_path[i]` to `lg_path[i+1]`, i.e. the turn at the
shared intersection between those two original edges.

---

## 7. Integration with `patch_coordinates()`

`LineGraphQueryEngine::patch_coordinates()` currently prepends the user's origin
and appends the destination. Since `TurnAnnotation` no longer carries positional
fields (`at_index`, `coord`), `patch_coordinates` needs **no changes** for turn
support. The `turns` vector is computed before coordinate patching and remains
valid regardless of prepended/appended points.

---

## 8. Gateway Impact

**None.** The gateway (`hanoi-gateway`) is a stateless passthrough proxy. It
forwards the JSON response body unchanged. The new `turns` field in the GeoJSON
properties will flow through transparently.

---

## 9. What This Does NOT Cover

- **Street names at turns** — would require loading OSM name data (not currently
  in the pipeline)
- **Turn-by-turn navigation instructions** — "In 200 meters, turn left on Pho
  Hue" requires name data + distance-to-next-turn computation
- **Lane guidance** — which lane to be in before a turn
- **Multi-segment curves** — a curved road may show as many small "straight"
  turns; could be smoothed with a look-ahead window in the future

These are all possible future extensions but out of scope for this plan.

---

## 10. Testing Strategy

1. **Unit tests in `geometry.rs`:**
   - Known angle pairs → verify direction classification
   - Collinear segments → straight
   - 90° left/right → correct classification
   - 180° → U-turn
   - Degenerate zero-length segment → should not panic

2. **Integration test:**
   - Build a small synthetic line graph (4-5 edges forming an L-shape)
   - Run a query, verify `turns` array has expected entries

3. **Manual verification:**
   - Query the Hanoi graph via HTTP, overlay the GeoJSON on a map
   - Visually confirm turn annotations match actual road geometry
