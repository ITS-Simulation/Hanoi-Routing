# Live Weight MVP — Lightweight Plan

**Status:** Draft **Date:** 2026-03-30 **Relation:** Extracted from
[Live Weight Pipeline.md](Live%20Weight%20Pipeline.md) (full plan); this
document covers the minimum viable path to push synthetic weights into a running
CCH server and verify route changes.

---

## 0. Goal

Prove the end-to-end loop:

```
Generate synthetic weight vector → POST /customize → query returns different route
```

No Kafka, no smoothing, no dual-lane architecture, no influence map. Those come
later per the full plan. The MVP exists to **validate the plumbing** — binary
format, vector dimensions, server acceptance, and route-level verification.

---

## 1. Scope


| In scope                                                        | Out of scope (deferred to full plan) |
| --------------------------------------------------------------- | ------------------------------------ |
| Read original graph binary files                                | Huber DES smoothing                  |
| YAML camera config with per-road speed + occupancy overrides    | Camera simulation coroutines         |
| Occupancy scaling factor for camera-covered edges               | Neighbor congestion propagation      |
| Highway-class-scaled ToD fallback for uncovered edges           | Database-backed camera mapping       |
| POST to `/customize` endpoint                                   | Influence map BFS                    |
| Verify route changes via `/query`                               | Dual-lane aggregation pipeline       |
| Road identity export from CCH-Generator (`road_manifest.arrow`) |                                      |
| Edge → line-graph fan-out mapping                               |                                      |


---

## 2. The Three Questions

### Q1: Road Identity Mapping (road name ↔ edge)

#### Current state

CCH-Generator ([generate_graph.cpp](../../CCH-Generator/src/generate_graph.cpp))
calls RoutingKit's `load_osm_id_mapping_from_pbf()` and
`load_osm_routing_graph_from_pbf()` to build the graph. During this process:

- RoutingKit assigns each OSM way a **routing_way_id** (0-based contiguous
index) via `IDMapper`
- The `way` binary file stores `way[arc_id] = routing_way_id` (one entry per
edge) — this file **already exists** in the pipeline
- Road names and way speeds are computed in memory but **never saved to disk**
- At line 187 the mapping object is released, discarding names and speeds

#### What RoutingKit already provides

RoutingKit's `osm_profile.cpp` (lines 694-707) has `get_osm_way_name()` which
extracts the `name` and `ref` tags from OSM ways. CCH-Generator already calls
`get_osm_way_speed()` in the same callback — it just doesn't call the name
function.

**No RoutingKit modification needed.**

#### What to change in CCH-Generator

**Modification scope:**
[generate_graph.cpp](../../CCH-Generator/src/generate_graph.cpp) only. Moderate
manifest-writing changes; no new RoutingKit binary vector formats.

1. **Capture `way_osm_id`, `way_name`, and `way_highway` in the way callback**
  (around line 167):

```cpp
// Add alongside existing way_speed and way_is_roundabout vectors:
std::vector<uint64_t> way_osm_id(mapping.is_routing_way.population_count());
std::vector<std::string> way_name(mapping.is_routing_way.population_count());
std::vector<std::string> way_highway(mapping.is_routing_way.population_count());

// In the way callback lambda, add:
way_osm_id[routing_way_id] = osm_way_id;
way_name[routing_way_id] = RoutingKit::get_osm_way_name(osm_way_id, way_tags, log_fn);
const char* hw = way_tags["highway"];
if (hw) way_highway[routing_way_id] = hw;

// Later in load_graph(), move these onto GeneratedGraph so the manifest writers
// can access them after graph construction:
out.way_osm_id = std::move(way_osm_id);
out.way_name = std::move(way_name);
out.way_highway = std::move(way_highway);
```

1. **Generate `road_manifest.arrow`** after graph construction (before or after
  `save_graph`). Uses Apache Arrow IPC format via the C++ Arrow library:

```cpp
#include <arrow/api.h>
#include <arrow/io/file.h>
#include <arrow/ipc/writer.h>

// Group arc_ids by routing_way_id
std::vector<std::vector<unsigned>> arcs_per_way(graph.way_osm_id.size());
for (unsigned a = 0; a < graph.way.size(); ++a)
    arcs_per_way[graph.way[a]].push_back(a);

// Build Arrow arrays
arrow::UInt32Builder way_id_builder;
arrow::UInt64Builder osm_way_id_builder;
arrow::StringBuilder name_builder;
arrow::UInt32Builder speed_builder;
arrow::StringBuilder highway_builder;
auto arc_value_builder = std::make_shared<arrow::UInt32Builder>();
arrow::ListBuilder arc_list_builder(arrow::default_memory_pool(), arc_value_builder);

for (unsigned w = 0; w < graph.way_osm_id.size(); ++w) {
    way_id_builder.Append(w);
    osm_way_id_builder.Append(graph.way_osm_id[w]);
    name_builder.Append(graph.way_name[w]);
    speed_builder.Append(graph.way_speed[w]);
    highway_builder.Append(graph.way_highway[w]);
    arc_list_builder.Append();
    for (unsigned a : arcs_per_way[w])
        arc_value_builder->Append(a);
}

// Finalize and write IPC file
auto schema = arrow::schema({
    arrow::field("routing_way_id", arrow::uint32()),
    arrow::field("osm_way_id",     arrow::uint64()),
    arrow::field("name",           arrow::utf8()),
    arrow::field("speed_kmh",      arrow::uint32()),
    arrow::field("highway",        arrow::utf8()),
    arrow::field("arc_ids",        arrow::list(arrow::uint32())),
});
auto batch = arrow::RecordBatch::Make(schema, graph.way_osm_id.size(), {
    way_id_builder.Finish().ValueOrDie(),
    osm_way_id_builder.Finish().ValueOrDie(),
    name_builder.Finish().ValueOrDie(),
    speed_builder.Finish().ValueOrDie(),
    highway_builder.Finish().ValueOrDie(),
    arc_list_builder.Finish().ValueOrDie(),
});
auto out = arrow::io::FileOutputStream::Open(
    (output_dir / "road_manifest.arrow").string()).ValueOrDie();
auto writer = arrow::ipc::MakeFileWriter(out, schema).ValueOrDie();
writer->WriteRecordBatch(*batch);
writer->Close();
```

1. **Generate `road_arc_manifest.arrow`** with one row per original/base arc.
  This is the flat, operator-friendly manifest for DuckDB/CSV inspection:

```cpp
// Keep per-arc direction relative to the OSM way.
// GeneratedGraph needs:
// std::vector<bool> is_arc_antiparallel_to_way;
out.is_arc_antiparallel_to_way = routing_graph.is_arc_antiparallel_to_way;

// Build tail[arc_id] from CSR once.
std::vector<unsigned> tail(graph.head.size());
for (unsigned node = 0; node + 1 < graph.first_out.size(); ++node) {
    for (unsigned a = graph.first_out[node]; a < graph.first_out[node + 1]; ++a)
        tail[a] = node;
}

auto bearing_deg = [](float tail_lat, float tail_lon, float head_lat, float head_lon) {
    // Use the standard initial-bearing formula; normalize to [0, 360).
    ...
};

arrow::UInt32Builder arc_id_builder;
arrow::UInt32Builder routing_way_id_builder;
arrow::UInt64Builder arc_osm_way_id_builder;
arrow::StringBuilder arc_name_builder;
arrow::StringBuilder arc_highway_builder;
arrow::FloatBuilder tail_lat_builder, tail_lon_builder, head_lat_builder, head_lon_builder;
arrow::FloatBuilder bearing_builder;
arrow::BooleanBuilder antiparallel_builder;

for (unsigned a = 0; a < graph.head.size(); ++a) {
    const unsigned w = graph.way[a];
    const unsigned u = tail[a];
    const unsigned v = graph.head[a];

    arc_id_builder.Append(a);
    routing_way_id_builder.Append(w);
    arc_osm_way_id_builder.Append(graph.way_osm_id[w]);
    arc_name_builder.Append(graph.way_name[w]);
    arc_highway_builder.Append(graph.way_highway[w]);
    tail_lat_builder.Append(graph.latitude[u]);
    tail_lon_builder.Append(graph.longitude[u]);
    head_lat_builder.Append(graph.latitude[v]);
    head_lon_builder.Append(graph.longitude[v]);
    bearing_builder.Append(bearing_deg(
        graph.latitude[u], graph.longitude[u],
        graph.latitude[v], graph.longitude[v]
    ));
    antiparallel_builder.Append(graph.is_arc_antiparallel_to_way[a]);
}

auto arc_schema = arrow::schema({
    arrow::field("arc_id",                 arrow::uint32()),
    arrow::field("routing_way_id",         arrow::uint32()),
    arrow::field("osm_way_id",             arrow::uint64()),
    arrow::field("name",                   arrow::utf8()),
    arrow::field("highway",                arrow::utf8()),
    arrow::field("tail_lat",               arrow::float32()),
    arrow::field("tail_lon",               arrow::float32()),
    arrow::field("head_lat",               arrow::float32()),
    arrow::field("head_lon",               arrow::float32()),
    arrow::field("bearing_deg",            arrow::float32()),
    arrow::field("is_antiparallel_to_way", arrow::boolean()),
});
// Finish builders, create RecordBatch, and write road_arc_manifest.arrow
// using the same Arrow IPC FileWriter pattern as road_manifest.arrow.
```

No new RoutingKit binary files. The existing `way` binary stays (it's already
produced and used by `conditional_turn_extract`). The two Arrow manifests are
the only new outputs.

#### Build dependency

Add Apache Arrow C++ as a CMake dependency in `CCH-Generator/CMakeLists.txt`:

```cmake
find_package(Arrow REQUIRED)
target_link_libraries(cch_generator PRIVATE Arrow::arrow_shared)
```

Arrow C++ is available via system package managers (`libarrow-dev` on
Debian/Ubuntu, `arrow-devel` on Fedora) or vcpkg/conan.

#### Output: `road_manifest.arrow`

Apache Arrow IPC file with schema:


| Column           | Type           | Description                                                |
| ---------------- | -------------- | ---------------------------------------------------------- |
| `routing_way_id` | `uint32`       | Internal index (matches `way[arc_id]` values)              |
| `osm_way_id`     | `uint64`       | Original OSM way ID for this routing way                   |
| `name`           | `utf8`         | From OSM `name` + `ref` tags (empty string if unnamed)     |
| `speed_kmh`      | `uint32`       | Free-flow speed assigned by the profile                    |
| `highway`        | `utf8`         | OSM `highway` tag (e.g. `primary`, `residential`, `trunk`) |
| `arc_ids`        | `list<uint32>` | All original graph edges belonging to this road            |


**~73K rows, ~1.9M total arc_id values.** Arrow's columnar layout with zero-copy
reads makes this significantly faster than JSON for programmatic access (~8 MB
on disk vs ~60 MB as JSON), while remaining inspectable via Python
(`pyarrow.ipc.open_file`) or DuckDB (`SELECT * FROM 'road_manifest.arrow'`).

#### Output: `road_arc_manifest.arrow`

Apache Arrow IPC file with one row per **original/base graph arc**:


| Column                   | Type      | Description                                                                |
| ------------------------ | --------- | -------------------------------------------------------------------------- |
| `arc_id`                 | `uint32`  | Original/base graph arc ID. For base LG nodes, `lg_node_id == arc_id`      |
| `routing_way_id`         | `uint32`  | RoutingKit routing-way ID (`way[arc_id]`)                                  |
| `osm_way_id`             | `uint64`  | Original OSM way ID for the parent routing way                             |
| `name`                   | `utf8`    | Human-readable OSM road name (may be empty)                                |
| `highway`                | `utf8`    | OSM `highway` tag                                                          |
| `tail_lat`               | `float32` | Latitude of the arc's source intersection                                  |
| `tail_lon`               | `float32` | Longitude of the arc's source intersection                                 |
| `head_lat`               | `float32` | Latitude of the arc's destination intersection                             |
| `head_lon`               | `float32` | Longitude of the arc's destination intersection                            |
| `bearing_deg`            | `float32` | Travel direction of the arc, clockwise from geographic north               |
| `is_antiparallel_to_way` | `bool`    | Whether this directed arc runs opposite to the OSM way's stored node order |


This file is the operator/debugging view: it is intentionally flat and
DuckDB/CSV-friendly, unlike `road_manifest.arrow`'s nested `arc_ids` list.
Use it to inspect candidate camera arcs, verify bearings, and cross-check that
a coordinate-based camera resolved to the expected directed road segment.

#### Data model summary

```
Existing (no changes):
  way[arc_id] = routing_way_id     (binary, already produced)

New (two files):
  road_manifest.arrow              (routing_way_id → osm_way_id, name, speed, highway, arc_ids)
  road_arc_manifest.arrow          (arc_id → routing_way_id, osm_way_id, name, highway, geometry, bearing)

Relationships:
  way[arc_id] → routing_way_id
  road_manifest[routing_way_id] → OSM way ID + human-readable name + highway + all arc_ids on that road
  road_arc_manifest[arc_id] → exact directed road segment metadata

  base LG node N = original arc N
  split LG nodes = internal duplicates for via-way restrictions; not exposed in camera YAML
```

#### How cameras will use this

1. **Road lookup:** Search `road_manifest.arrow` by name / OSM way ID to find
  all candidate arcs for a road.
2. **Directed arc selection:** Use `road_arc_manifest.arrow` to inspect the
  exact arc geometry and bearing, then either copy an `arc_id` directly or let
  coordinate-based camera resolution choose it automatically.
3. **Runtime targeting:** Cameras always resolve to an **original/base arc ID**.
  The weight generator then fans that road-segment weight out onto line-graph
  edges via `reverse_index[original_edge]`.

---

### Q2: Line Graph Weight Format — Camera Data → LG Weight Vector

This is the central technical challenge of the MVP. Cameras observe **roads**
(original edges), but the server expects weights for **turns** (line graph
edges). Understanding the encoding is essential.

#### Dimensions (Hanoi motorcycle graph)


| Entity         | Count     | What it represents                          |
| -------------- | --------- | ------------------------------------------- |
| Original nodes | 929,366   | Intersections                               |
| Original edges | 1,942,872 | Road segments                               |
| LG nodes       | 1,943,051 | Original edges (1,942,872 base + 179 split) |
| LG edges       | 4,396,227 | Valid turns between consecutive edges       |
| Routing ways   | ~73,000   | Distinct OSM ways (one way → many edges)    |


estimated from `way_speed` vector size

#### The shifted encoding

When the line graph is built, each LG edge gets weight:

```
lg_weight(E1 → E2) = travel_time(E2) + turn_cost(E1, E2)
```

Where E1, E2 are original edge IDs. The weight of the **source** edge E1 is
excluded — it's added back at query time:

```rust
// line_graph.rs:269-270
let source_edge_cost = self.context.original_travel_time[source_edge as usize];
let distance_ms = cch_distance.saturating_add(source_edge_cost);
```

**This means:** an LG edge's weight is dominated by the travel time of its
**target** original edge. If a camera observes that original edge E has changed
speed, ALL LG edges whose target is E must be updated.

#### The fan-out mapping: original edge → LG edges

For each original edge E, the LG edges that carry E's weight are exactly those
LG edges `(*, E)` — all turns **into** E. In CSR terms, these are the entries in
the line graph's adjacency list that have `head[lg_edge] == E`.

**Building the reverse index** (LG edge target → list of LG edge IDs):

```
reverse_index: Map<OriginalEdgeId, Vec<LgEdgeId>>

for lg_edge_id in 0..num_lg_edges:
    target_lg_node = lg_head[lg_edge_id]    // = original edge ID
    reverse_index[target_lg_node].push(lg_edge_id)
```

This is a one-time precomputation (~4.4M entries, ~35 MB). For Hanoi, each
original edge is the target of ~2.3 LG edges on average (most intersections have
2-3 incoming roads).

#### Weight vector construction algorithm

Given a camera observation that original edge E now has travel time T_new (in
milliseconds):

```
for each lg_edge_id in reverse_index[E]:
    source_lg_node = find_source_of_lg_edge(lg_edge_id)  // from CSR
    turn_cost = existing_turn_cost(source_lg_node, E)     // usually 0
    lg_weights[lg_edge_id] = T_new + turn_cost
```

For the MVP (where turn costs are 0 for all non-forbidden turns), this
simplifies to:

```
for each lg_edge_id in reverse_index[E]:
    lg_weights[lg_edge_id] = T_new
```

#### Camera mapping config

The MVP uses a YAML config file with two sections: reusable **speed profiles**
(defined once, shared across cameras) and **cameras** (each mapping one
physical sensor to one directed road segment and one profile).

Each camera supports **two placement modes**:

1. **Explicit `arc_id` mode:** The operator already knows the exact original/base
  arc ID and enters it directly.
2. **Coordinate mode:** The operator provides `lat`, `lon`, and
  `flow_bearing_deg`; the loader resolves those coordinates to the correct
  original/base arc ID at startup.

Exactly one placement mode must be provided per camera. `arc_id` and
`lat`/`lon`/`flow_bearing_deg` are mutually exclusive.

```yaml
# cameras.yaml

# Speed profiles — define congestion behaviour by road character.
# free_flow_kmh / free_flow_occupancy: baseline when no peak is active.
# peaks: Gaussian-blended congestion events, each with a centre hour.
profiles:
  arterial_rush:            # heavy arterial — slow during both peaks
    free_flow_kmh: 45.0
    free_flow_occupancy: 0.18
    peaks:
      - hour: 7.5           # 7:30 AM morning rush
        speed_kmh: 8.0
        occupancy: 0.80
      - hour: 17.5          # 5:30 PM evening rush
        speed_kmh: 10.0
        occupancy: 0.75

  bridge_evening:           # bridge — only bad in the evening
    free_flow_kmh: 50.0
    free_flow_occupancy: 0.15
    peaks:
      - hour: 17.5
        speed_kmh: 15.0
        occupancy: 0.65

  ring_road:                # ring road — stays fast all day
    free_flow_kmh: 80.0
    free_flow_occupancy: 0.12
    # no peaks — always free-flow

# Cameras — one entry per physical sensor, one profile per camera.
# To cover consecutive edges on the same road, add one entry per directed edge.
cameras:
  - id: 0
    label: "Đường Láng southbound"
    arc_id: 42
    profile: arterial_rush

  - id: 1
    label: "Cầu Giấy bridge southbound"
    lat: 21.03612
    lon: 105.79043
    flow_bearing_deg: 182.0
    profile: bridge_evening

  - id: 2
    label: "Ring Road 3 eastbound"
    lat: 21.02480
    lon: 105.82155
    flow_bearing_deg: 90.0
    profile: ring_road
```

`**flow_bearing_deg` definition:** Direction of **vehicle movement** observed by
the camera, measured clockwise from geographic north. It is **not** the camera
lens orientation.

- `0` / `360` = northbound
- `90` = eastbound
- `180` = southbound
- `270` = westbound

**Why this is needed:** On a two-way road, the forward and reverse arcs can lie
on almost identical geometry. Distance alone cannot reliably choose the correct
one. `flow_bearing_deg` disambiguates which travel direction the camera sees.

**How `flow_bearing_deg` is used:**

1. Find nearby candidate **base/original arcs** around `(lat, lon)`.
2. Compute each candidate arc's travel heading from its tail node to its head
  node.
3. Compare that heading to `flow_bearing_deg` using circular angle difference:
  `min(|a-b|, 360-|a-b|)`.
4. Prefer candidates that are both spatially close and heading-consistent;
  reject candidates whose heading mismatch exceeds a threshold (for example  
  45 degrees.
5. Log the resolved `arc_id`, road name, and bearing at startup.

This logic resolves cameras to **original/base arc IDs**, even though routing
uses the line graph. Base line-graph node `N` is original arc `N`; split
line-graph nodes are internal only and must not appear in `cameras.yaml`.

**Why `is_antiparallel_to_way` is not enough:** That flag only tells whether an
arc runs with or against the OSM way's stored node order. It does **not** tell
you whether that arc is the one your camera observes, because OSM way order is
arbitrary from an operator's perspective. Cameras care about traffic flow
direction and physical location, not OSM node ordering.

**How to find `arc_id` manually:** Use `road_manifest.arrow` to find the road,
then inspect `road_arc_manifest.arrow` in DuckDB/Python to choose the exact
directed arc by location and bearing. If you do not want to inspect manifests
manually, use coordinate mode and let startup resolution pick the arc.

**Precedence:** Camera-covered edges use their profile's interpolated speed +
occupancy. Edges not covered by any camera get the highway-class-scaled ToD
fallback.

#### Speed profile interpolation

Each camera's speed and occupancy at a given hour is computed by Gaussian-
blending between the profile's free-flow baseline and its defined peaks:

```kotlin
data class PeakPoint(val hour: Double, val speedKmh: Double, val occupancy: Double)
data class SpeedProfile(
    val freeFlowKmh: Double,
    val freeFlowOccupancy: Double,   // configurable per profile
    val peaks: List<PeakPoint>
)

fun gaussian(x: Double, center: Double, sigma: Double = 1.2) =
    exp(-0.5 * ((x - center) / sigma).pow(2))

fun profileSpeed(profile: SpeedProfile, hour: Double): Pair<Double, Double> {
    var speed     = profile.freeFlowKmh
    var occupancy = profile.freeFlowOccupancy

    // Each peak pulls speed down and occupancy up proportional to its Gaussian weight.
    // Multiple peaks compose naturally — morning + evening both active at noon = ~0.
    for (peak in profile.peaks) {
        val w = gaussian(hour, center = peak.hour)
        speed     = lerp(speed,     peak.speedKmh,  w)
        occupancy = lerp(occupancy, peak.occupancy, w)
    }
    return Pair(speed, occupancy)
}
// lerp(a, b, t) = a + (b - a) * t
```

At `hour = 7.5` with one morning peak at 7.5: `w ≈ 1.0` → full congestion
values. At `hour = 12.0`: `w ≈ 0.02` → nearly free-flow. Two peaks compose
independently — the evening peak has near-zero weight at noon so they don't
interfere.

#### Time-of-day Gaussian + highway-class scaling

A global Gaussian ToD multiplier is too coarse — a residential alley and a
primary arterial both getting +50% at evening rush is physically unrealistic.
The fix is to scale the ToD *deviation* by a per-road congestion susceptibility
factor derived from the OSM `highway` tag.

**Step 1 — Global Gaussian baseline** (same as before):

```kotlin
fun timeOfDayFactor(hour: Double): Double {
    val morning = 0.35 * gaussian(hour, center = 7.5,  sigma = 1.2)  // +40% at 7:30 AM
    val evening = 0.45 * gaussian(hour, center = 17.5, sigma = 1.5)  // +50% at 5:30 PM
    val night   = -0.15 * gaussian(hour, center = 2.0, sigma = 2.0)  // -15% at 2:00 AM
    return 1.0 + morning + evening + night
}
```

**Step 2 — Highway-class congestion susceptibility**:

```kotlin
// How much of the global ToD deviation applies to each road class.
// Roads with high capacity (motorway, primary) are congestion targets;
// residential streets are already at their natural speed and don't fluctuate.
fun highwayCongestionScale(highway: String): Double = when (highway) {
    "motorway", "motorway_link"   -> 1.00  // full ToD effect
    "trunk", "trunk_link"         -> 0.80
    "primary", "primary_link"     -> 0.85
    "secondary", "secondary_link" -> 0.55
    "tertiary", "tertiary_link"   -> 0.35
    "residential", "living_street"-> 0.35  
    "service"                     -> 0.15  
    else                          -> 0.30  // unknown → conservative
}
```

**Step 3 — Effective per-road factor**:

```kotlin
val todRaw = timeOfDayFactor(hour)               // e.g. 1.45 at evening rush
val scale  = highwayCongestionScale(highway)     // e.g. 0.35 for residential
val factor = 1.0 + (todRaw - 1.0) * scale        // residential: 1.0 + 0.45*0.35 = 1.16
weight = (baselineTravelTime * factor).toInt()
```

**Example at 17:30 (global ToD = 1.45, deviation = +0.45):**


| `highway` class | Scale | Effective factor | Interpretation               |
| --------------- | ----- | ---------------- | ---------------------------- |
| `motorway`      | 1.00  | 1.45             | +45% — heavy rush congestion |
| `primary`       | 0.85  | 1.38             | +38% — arterial congestion   |
| `secondary`     | 0.55  | 1.25             | +25% — collector road        |
| `residential`   | 0.35  | 1.16             | +16% — moderate local impact |
| `service`       | 0.15  | 1.07             | +7% — mild access-road shift |


The `highway` value is read from `road_manifest.arrow` at startup, then looked
up per edge via the existing `way` binary vector:
`way[arc_id] → routing_way_id → road_manifest.highway`.

#### Camera resolution in a line-graph world

Even though the server runs on the line graph, a camera still observes a
physical **road segment**, not a turn. In this codebase:

- base line-graph node `N` is original/base arc `N`,
- line-graph weights live on LG **edges** (turns),
- split LG nodes are internal duplicates created for via-way restrictions.

So camera placement must resolve to an **original/base arc ID** first. Only
after that do we fan the camera-covered road-segment weight out to all LG edges
targeting that base arc.

This is why `cameras.yaml` should never expose split LG node IDs directly. The
operator-facing concept remains "which directed road segment does this camera
observe?", not "which internal split node was introduced by line-graph
expansion?".

#### Complete MVP weight generation pseudocode

```kotlin
// At startup:
val originalTravelTime = loadU32Vector("travel_time")    // [1,942,872]
val geoDistance        = loadU32Vector("geo_distance")   // [1,942,872]
val wayIndex           = loadU32Vector("way")            // [1,942,872] arc→routing_way_id
val lgHead             = loadU32Vector("line_graph/head")// [4,396,227]
val numLgEdges         = lgHead.size                     // 4,396,227
val cameras            = loadCameraConfig("cameras.yaml")
val manifest           = loadRoadManifest("road_manifest.arrow")
val arcManifest        = loadRoadArcManifest("road_arc_manifest.arrow")
// manifest:    routing_way_id → { osm_way_id, name, speed_kmh, highway, arc_ids }
// arcManifest: arc_id → { osm_way_id, name, highway, tail/head coords, bearing, is_antiparallel }

// Per-edge highway lookup array (avoids repeated Arrow table scan)
val edgeHighway = Array(originalTravelTime.size) { i ->
    manifest[wayIndex[i]]?.highway ?: ""
}

// Build reverse index: original_edge → list of LG edges targeting it
val reverseIndex = Array(originalTravelTime.size) { mutableListOf<Int>() }
for (lgEdge in 0 until numLgEdges) {
    reverseIndex[lgHead[lgEdge]].add(lgEdge)
}

// Resolve each camera to an ORIGINAL/base arc ID.
// arc_id mode uses the provided ID directly.
// coordinate mode snaps to nearby base arcs, then uses flow_bearing_deg
// to pick the candidate whose travel heading best matches the observed flow.
fun resolveCameraArc(cam: Camera, arcManifest: ArcManifest): Int {
    if (cam.arcId != null) {
        require(cam.lat == null && cam.lon == null && cam.flowBearingDeg == null)
        return cam.arcId
    }

    require(cam.lat != null && cam.lon != null && cam.flowBearingDeg != null)
    val candidates = findNearbyBaseArcs(cam.lat, cam.lon, arcManifest)
    val best = candidates
        .map { arc ->
            val distanceM = haversineMeters(cam.lat, cam.lon, arc.tailLat, arc.tailLon)
            val bearingDiff = circularAngleDiff(cam.flowBearingDeg, arc.bearingDeg)
            CandidateScore(arc.arcId, distanceM, bearingDiff)
        }
        .filter { it.bearingDiffDeg <= 60.0 }
        .minByOrNull { it.distanceM + 2.0 * it.bearingDiffDeg }
        ?: error("No heading-consistent arc found for camera ${cam.id} (${cam.label})")

    log.info(
        "camera '{}' resolved to arc_id={} (distance={}m, bearing_diff={}deg)",
        cam.label, best.arcId, best.distanceM, best.bearingDiffDeg
    )
    return best.arcId
}

// Build camera coverage: resolved original/base arc_id → SpeedProfile
val cameraProfiles = mutableMapOf<Int, SpeedProfile>()
for (cam in cameras) {
    val profile = profiles[cam.profileName]
        ?: error("Unknown profile '${cam.profileName}' for camera ${cam.id}")
    val arcId = resolveCameraArc(cam, arcManifest)
    cameraProfiles[arcId] = profile
}

// Occupancy scaling: inflates travel time when density is high.
// At occupancy 0.8: factor = 1.0 + 0.5*(0.8-0.2) = 1.30 (+30%)
// At free-flow occupancy (e.g. 0.12): factor ≈ 0.96 (slight reduction)
fun occupancyFactor(occupancy: Double) =
    1.0 + 0.5 * (occupancy - 0.2)

// Generate weight vector:
fun generateWeights(hour: Double): IntArray {
    val todRaw    = timeOfDayFactor(hour)   // global Gaussian baseline
    val lgWeights = IntArray(numLgEdges)

    for (origEdge in originalTravelTime.indices) {
        val weight: Int
        val profile = cameraProfiles[origEdge]
        if (profile != null) {
            // Camera-covered: interpolate speed + occupancy from profile
            val (speed, occupancy) = profileSpeed(profile, hour)
            // tt_ms = geo_distance_m * 3600 / speed_kmh
            val baseTt = geoDistance[origEdge] * 3600.0 / speed
            weight = (baseTt * occupancyFactor(occupancy)).toInt()
                .coerceIn(1, Int.MAX_VALUE - 1)
        } else {
            // Uncovered: highway-class-scaled ToD deviation
            val scale  = highwayCongestionScale(edgeHighway[origEdge])
            val factor = 1.0 + (todRaw - 1.0) * scale
            weight = (originalTravelTime[origEdge] * factor).toInt()
                .coerceIn(1, Int.MAX_VALUE - 1)
        }

        for (lgEdge in reverseIndex[origEdge]) {
            lgWeights[lgEdge] = weight
        }
    }
    return lgWeights
}
```

#### Why the latitude/longitude files are usable

Line graph `latitude[lg_node]` and `longitude[lg_node]` store the coordinates of
`tail[original_edge]` — the **source intersection** of the original edge that LG
node represents. This means:

- LG node coordinates are in the **original graph's coordinate space**
- Base LG node `N` corresponds to original edge `N`
- Split LG nodes inherit those same source-intersection coordinates and are not
suitable as operator-facing camera targets
- The server already uses original graph coordinates for KD-tree snapping
([line_graph.rs:234-239](../../CCH-Hanoi/crates/hanoi-core/src/line_graph.rs))

For finding an `arc_id` to put in `cameras.yaml`, load `road_manifest.arrow`
and `road_arc_manifest.arrow` in Python/DuckDB, search by road name, then
inspect the directed arc candidates by location and bearing. Coordinate mode
exists precisely so operators do **not** have to decode raw RoutingKit binary
vectors by hand.

#### Visualization of the encoding

```
Original graph:         A ──E0──→ B ──E1──→ C ──E2──→ D
                                    ↘ E3 → E

Line graph:             (E0) ──T01──→ (E1) ──T12──→ (E2)
                               ↘ T03 → (E3)

LG edge weights:
  T01.weight = travel_time(E1)     ← target edge's travel time
  T12.weight = travel_time(E2)     ← target edge's travel time
  T03.weight = travel_time(E3)     ← target edge's travel time

If camera observes E1 is now 2× slower:
  → Update T01.weight = 2 × travel_time(E1)   (turn A→B entering E1)
  → Any other LG edge targeting E1 also updated

Query correction:
  Route E0→E1→E2:
  CCH returns: T01 + T12 = tt(E1) + tt(E2)
  + source correction: tt(E0)
  = tt(E0) + tt(E1) + tt(E2)  ✓ correct total
```

---

### Q3: MVP Implementation — Minimal Weight Generator

#### Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                      KOTLIN MVP APPLICATION                       │
│                                                                  │
│  ┌──────────────────┐  ┌─────────────────────────────────────┐   │
│  │   Graph Loader    │  │          Camera Config               │   │
│  │  travel_time      │  │  cameras.yaml                        │   │
│  │  geo_distance     │  │  profiles: name → SpeedProfile       │   │
│  │  way              │  │    freeFlowKmh, freeFlowOccupancy    │   │
│  │  line_graph/head  │  │    peaks: [hour, speed, occupancy]   │   │
│  │  road_manifest.arrow │  │  cameras:                            │   │
│  │  road_arc_manifest.arrow │  │    arc_id OR lat/lon/bearing → profile │   │
│  └────────┬──────────┘  └───────────────┬─────────────────────┘   │
│           │                             │                         │
│           ▼                             ▼                         │
│  ┌──────────────────────────────────────────────────────┐         │
│  │ Weight Generator                                     │         │
│  │                                                      │         │
│  │  Camera-covered edges:                               │         │
│  │    (speed, occ) = profileSpeed(profile, hour)        │         │
│  │    tt = geo_distance * 3600 / speed                  │         │
│  │       * occupancyFactor(occ)                         │         │
│  │                                                      │         │
│  │  Uncovered edges:                                    │         │
│  │    tt = baseline * (1 + (todRaw-1)                   │         │
│  │           * highwayCongestionScale(highway))         │         │
│  │                                                      │         │
│  │  Fan out to LG edges via reverse index               │         │
│  └─────────────────────┬────────────────────────────────┘         │
│                        │                                          │
│                        ▼                                          │
│  ┌───────────────┐  POST /customize                               │
│  │  HTTP Client  │  17.6 MB binary (little-endian u32)            │
│  └───────────────┘                                                │
│          │                                                        │
│          ▼                                                        │
│      hanoi-server                                                 │
│      CCH re-customization (~100-500ms)                            │
│      Queries now reflect profile weights + highway ToD            │
└──────────────────────────────────────────────────────────────────┘
```

#### Existing CCH_Data_Pipeline scaffold

The `CCH_Data_Pipeline/` project already exists as a multi-module Gradle build
with complete infrastructure but no implementation (5 lines of Kotlin — a
"Hello, World!" in `app/Main.kt`).

**What's already set up:**


| Component                                                        | Status   | Reusable for MVP?                        |
| ---------------------------------------------------------------- | -------- | ---------------------------------------- |
| Gradle multi-module (`app`, `simulation`, `smoother`, `modeler`) | Complete | Yes — matches full pipeline plan exactly |
| Version catalog (`gradle/libs.versions.toml`)                    | Complete | Yes — already declares needed deps       |
| `app/build.gradle.kts` (Clikt CLI, Shadow JAR)                   | Complete | Yes — main class, fat JAR config ready   |
| `simulation/build.gradle.kts` (coroutines)                       | Complete | Not for MVP (synchronous)                |
| `smoother/build.gradle.kts`                                      | Complete | Not for MVP                              |
| `modeler/build.gradle.kts`                                       | Complete | Not for MVP                              |
| `hanoi_motorcycle.http` (test queries)                           | Complete | Yes — useful for verification            |


**Dependencies already in the version catalog (`libs.versions.toml`):**


| Catalog entry                                        | Version | MVP uses?                                                                |
| ---------------------------------------------------- | ------- | ------------------------------------------------------------------------ |
| `clikt`                                              | 5.1.0   | Yes (CLI) — already imported by `app`                                    |
| `ktor-client-core`, `ktor-client-cio`                | 3.4.1   | Yes (HTTP POST) — catalog only, add to `app`                             |
| `arrow-vector`, `arrow-memory`, `arrow-memory-netty` | 18.3.0  | Yes (read Arrow manifests) — catalog only, add to `app`                  |
| `log4j-core`, `log4j-slf4j2-impl`                    | 2.25.3  | Yes (logging) — catalog only, add to `app`                               |
| `shadow`                                             | 9.3.1   | Yes (fat JAR) — already applied in `app`                                 |
| `coroutines`                                         | 1.10.2  | No (MVP is synchronous)                                                  |
| `serialization`                                      | 1.10.0  | No (no JSON in MVP)                                                      |
| `duckdb-jdbc`                                        | 1.5.0   | Optional (ad-hoc manifest inspection / CSV export, not runtime-critical) |
| `snakeyaml`                                          | 2.5     | Yes (read `cameras.yaml`) — catalog only, add to `app`                   |


**Conclusion:** No new dependencies need to be *added* to the version catalog.
The MVP just needs to wire existing catalog entries into `app/build.gradle.kts`
(`ktor`, Arrow Java, Log4j, SnakeYAML) and write the implementation files.

#### Project structure (MVP additions)

```
CCH_Data_Pipeline/                          (existing scaffold)
├── build.gradle.kts                        (no changes)
├── settings.gradle.kts                     (no changes)
├── gradle/libs.versions.toml               (no changes — deps already declared)
├── app/
│   ├── build.gradle.kts                    (add ktor, arrow, log4j, snakeyaml from catalog)
│   └── src/main/kotlin/com/thomas/cch_app/
│       ├── Main.kt              # Replace Hello World with Clikt CLI
│       ├── GraphLoader.kt       # Binary I/O (u32/f32 vectors)
│       ├── LineGraphFanOut.kt   # Reverse index builder
│       ├── CameraConfig.kt      # YAML camera config loader
│       ├── CameraResolver.kt    # Coordinate/bearing -> base arc resolution
│       ├── WeightGenerator.kt   # Camera overrides + time-of-day fallback
│       └── CustomizeClient.kt   # HTTP POST via Ktor
├── simulation/                             (untouched for MVP)
├── smoother/                               (untouched for MVP)
└── modeler/                                (untouched for MVP)
```

Only `app/` is modified. The other three modules stay as empty scaffolding,
ready for the full pipeline implementation.

#### CLI interface

```bash
# Basic: apply camera overrides + time-of-day weights once
./mvp --graph-dir /path/to/hanoi_motorcycle/graph \
      --cameras cameras.yaml \
      --server http://localhost:9080 \
      --hour 7.5

# Loop: update every 30s with advancing simulated time
./mvp --graph-dir /path/to/hanoi_motorcycle/graph \
      --cameras cameras.yaml \
      --server http://localhost:9080 \
      --loop --time-accel 60   # 1 real second = 1 sim minute

# Without cameras: pure time-of-day modulation only
./mvp --graph-dir /path/to/hanoi_motorcycle/graph \
      --server http://localhost:9080 \
      --hour 17.5
```

`--cameras` is optional. Without it, all edges get the global ToD multiplier.
With it, camera-covered edges use their profile's interpolated speed and
occupancy at the selected hour, while uncovered edges still get the
highway-class-scaled ToD fallback.

#### Acceptance criteria


| Test                  | Expected outcome                                                                                            |
| --------------------- | ----------------------------------------------------------------------------------------------------------- |
| POST weight vector    | Server returns 200, logs "Customization complete"                                                           |
| Vector size           | Exactly 4,396,227 × 4 = 17,584,908 bytes                                                                    |
| All weights valid     | Every weight ∈ [1, 2,147,483,646]                                                                           |
| Camera override       | Profile with peak 5 km/h at `hour=7.5` → query avoids that edge at morning rush                             |
| Camera free-flow      | Profile with `free_flow_kmh: 80` → query prefers that edge at off-peak hours                                |
| Occupancy inflation   | Same speed, higher peak `occupancy` → higher weight at peak hour                                            |
| Profile time-variance | Same camera: weight at `hour=7.5` (peak) > `hour=12.0` (free-flow)                                          |
| Coordinate resolution | Same physical sensor configured by `arc_id` and by `lat/lon/flow_bearing_deg` resolves to the same base arc |
| Highway-class ToD     | `--hour 17.5` without cameras → `primary` roads penalised more than `residential` ones                      |
| Rush hour ToD         | `--hour 17.5` without cameras → routes shift to avoid arterials                                             |
| Night ToD             | `--hour 2.0` → route matches or improves on baseline                                                        |
| Round-trip            | POST baseline weights (no cameras, hour=12) → same route as uncustomized                                    |


---

## 3. Implementation Steps

### Step 1: CCH-Generator road identity export

**Files to modify:**

- `CCH-Generator/src/generate_graph.cpp`

**Changes:**

1. Add Arrow C++ dependency to `CMakeLists.txt`
2. Add `way_osm_id`, `way_name`, and `way_highway` vectors in the way callback
3. Write `road_manifest.arrow` (Arrow IPC format, road-level) — schema:
  `routing_way_id`, `osm_way_id`, `name`, `speed_kmh`, `highway`, `arc_ids`
4. Write `road_arc_manifest.arrow` (Arrow IPC format, arc-level) — one row per
  original/base arc with `arc_id`, road identity, tail/head coordinates,
  `bearing_deg`, and `is_antiparallel_to_way`; carry the per-arc direction
  flag through from `routing_graph.is_arc_antiparallel_to_way`
5. Keep camera-facing IDs in terms of **original/base arc IDs**, not split LG
  nodes introduced later by via-way expansion

**Estimated scope:** modest C++ manifest work plus camera-resolution code on the
Kotlin side; still no new RoutingKit binary file formats required

**Validation:** Rebuild CCH-Generator, regenerate Hanoi motorcycle graph, verify
both manifest files exist and are loadable via `pyarrow.ipc.open_file()` or
DuckDB. Spot-check road names and arc bearings against OpenStreetMap. Verify
road-manifest arc_id counts sum to total edge count and that every original arc
appears exactly once in `road_arc_manifest.arrow`.

### Step 2: Kotlin binary I/O + reverse index

**New files:**

- `CCH_Data_Pipeline/app/src/main/kotlin/.../GraphLoader.kt`
- `CCH_Data_Pipeline/app/src/main/kotlin/.../LineGraphFanOut.kt`

**What:**

- `GraphLoader`: Read little-endian u32/f32 vectors from RoutingKit binary
format. Load `travel_time`, `geo_distance`, `way` (original graph), and
`line_graph/head`, `line_graph/first_out`. Also reads `road_manifest.arrow`
and `road_arc_manifest.arrow` (via Apache Arrow Java) and builds a flat
`edgeHighway: Array<String>` by joining
`way[arc_id] → routing_way_id → manifest.highway`.
- `LineGraphFanOut`: Build `reverse_index[original_edge] → List<lg_edge_id>`
from `lg_head`. This is the core mapping that translates camera observations
(per original edge) into LG weight vector entries.

**Validation:** Assert `reverse_index` total entries == num_lg_edges. Assert
every LG edge is accounted for exactly once. Assert base LG node `N`
corresponds to original edge `N`, and keep split nodes internal-only for camera
resolution.

### Step 3: Weight generator + HTTP client

**New files:**

- `CCH_Data_Pipeline/app/src/main/kotlin/.../WeightGenerator.kt`
- `CCH_Data_Pipeline/app/src/main/kotlin/.../CameraConfig.kt`
- `CCH_Data_Pipeline/app/src/main/kotlin/.../CameraResolver.kt`
- `CCH_Data_Pipeline/app/src/main/kotlin/.../CustomizeClient.kt`
- `CCH_Data_Pipeline/app/src/main/kotlin/.../Main.kt`

**What:**

- `CameraConfig`: Load `cameras.yaml`. Parse `profiles` map
(`name → SpeedProfile(freeFlowKmh, freeFlowOccupancy, peaks)`), then parse
`cameras` list in one of two mutually exclusive placement modes:
  - explicit `arc_id`, or
  - `lat` + `lon` + `flow_bearing_deg`.
  `flow_bearing_deg` is traffic-flow direction in degrees clockwise from north,
  not camera lens orientation. Fail fast if a camera mixes modes or omits part
  of coordinate mode.
- `CameraResolver`: For coordinate-mode cameras, find nearby **original/base**
arcs, compare candidate arc bearings against `flow_bearing_deg`, reject
heading-inconsistent candidates, and resolve each camera to exactly one
original/base `arc_id`. Log the chosen road name, arc ID, and bearing for
operator verification. Do not expose split LG node IDs in YAML.
- `WeightGenerator`: For camera-covered edges, call `profileSpeed(profile, hour)`
to get Gaussian-interpolated `(speed, occupancy)`, compute
`tt = geoDistance * 3600 / speed`, apply `occupancyFactor`. For uncovered
edges, apply `highwayCongestionScale(highway)`-scaled ToD deviation. Fan out
all weights to LG edges via reverse index. Clamp to [1, INFINITY-1].
- `CustomizeClient`: Serialize `IntArray` to little-endian bytes, POST to
`/customize`.
- `Main`: Clikt CLI with `--graph-dir`, `--cameras` (optional), `--server`,
`--hour`, `--loop`, `--time-accel` flags.

**Validation:** Run against live `hanoi-server`, verify 200 response. Set one
camera to 5 km/h and verify the query avoids that road. Remove the camera and
verify the route returns to normal. Validate that a coordinate-mode camera and
an explicit-`arc_id` camera can be configured for the same physical sensor and
resolve to the same original/base arc.

### Step 4: Verification queries

**Manual verification:**

1. Start `hanoi-server` with line graph
2. Query a known route (e.g., across Hanoi center) — record baseline
3. Run MVP with `--hour 7.5` (morning rush)
4. Re-query — route should prefer less-congested alternatives
5. Run MVP with `--hour 2.0` (night)
6. Re-query — route should match or improve on baseline

---

## 4. Data Flow Diagram — End to End

```
                        ┌─── CCH-Generator (modified) ───┐
                        │                                  │
OSM PBF ──→ generate_graph ──→ graph/                      │
                        │      ├── first_out               │
                        │      ├── head                    │
                        │      ├── travel_time             │
                        │      ├── way                     │
                        │      ├── road_manifest.arrow(NEW) │
                        │      ├── road_arc_manifest.arrow(NEW) │
                        │      ├── latitude                │
                        │      ├── longitude               │
                        │      └── ...                     │
                        └──────────────────────────────────┘
                                      │
                    ┌─────────────────┤
                    ▼                 ▼
            generate_line_graph   conditional_turn_extract
                    │                 │
                    ▼                 ▼
            line_graph/           conditional_turn_*
            ├── first_out
            ├── head          ←── key file for fan-out
            ├── travel_time
            ├── latitude
            └── longitude
                    │
                    ├───────────── flow_cutter_cch_order.sh
                    │                      │
                    │                      ▼
                    │              line_graph/perms/cch_perm
                    │
                    ▼
            ┌── Kotlin MVP ────────────────────────────┐
            │                                           │
            │  Load: travel_time, geo_distance, way,      │
            │        line_graph/head, road_manifest.arrow,│
            │        road_arc_manifest.arrow              │
            │  Build: reverse_index, edgeHighway,         │
            │         camera arc resolver                 │
            │  Generate: lg_weights (profiles +         │
            │            occupancy + highway ToD)       │
            │  POST: /customize (17.6 MB)               │
            │                                           │
            └───────────────┬───────────────────────────┘
                            │
                            ▼
            ┌── hanoi-server ──────────────────────────┐
            │                                           │
            │  Validate weight vector                   │
            │  DirectedCCH re-customization             │
            │  Serve queries with updated weights       │
            │                                           │
            └───────────────────────────────────────────┘
```

---

## 5. From MVP to Full Pipeline

The MVP validates the plumbing. Graduating to the full pipeline (per
[Live Weight Pipeline.md](Live%20Weight%20Pipeline.md)) means:


| MVP component                                        | Becomes in full pipeline                                         |
| ---------------------------------------------------- | ---------------------------------------------------------------- |
| `GraphLoader`                                        | `simulation/graph/GraphData.kt` (adds geo_distance, lat/lng)     |
| `LineGraphFanOut`                                    | Stays, used by `WeightModel`                                     |
| `WeightGenerator` (camera + occupancy + highway ToD) | `modeler/LiveWeightModel.kt` (adds smoothing, staleness, joiner) |
| `CustomizeClient`                                    | `modeler/output/CustomizeClient.kt` (unchanged)                  |
| `Main.kt` (single-shot/loop)                         | `app/Main.kt` (coroutine pipeline orchestration)                 |
| —                                                    | `simulation/` module (camera sim, aggregators)                   |
| —                                                    | `smoother/` module (Huber DES)                                   |
| —                                                    | `modeler/EdgeJoiner.kt` (alignment)                              |
| —                                                    | `modeler/InfluenceMap.kt` (neighbor propagation)                 |


The reverse index (`LineGraphFanOut`) is the permanent bridge between the
camera-observes-roads world and the server-expects-turns world. It doesn't
change between MVP and full pipeline.

---

## 6. Key Numbers Reference


| Metric                         | Value                              |
| ------------------------------ | ---------------------------------- |
| Original nodes                 | 929,366                            |
| Original edges                 | 1,942,872                          |
| LG nodes                       | 1,943,051 (incl. 179 split)        |
| LG edges                       | 4,396,227                          |
| LG weight vector size          | 17,584,908 bytes (17.6 MB)         |
| Original weight vector size    | 7,771,488 bytes (7.8 MB)           |
| Avg LG edges per original edge | ~2.3                               |
| CCH customization time         | ~100-500 ms                        |
| Time-of-day factor range       | 0.85 (night) – 1.50 (evening rush) |


*All numbers for the Hanoi motorcycle graph.*