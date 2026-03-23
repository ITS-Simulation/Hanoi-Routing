# Conditional Turn Integration Plan

> **Goal**: Extend CCH-Generator to produce conditional turn restriction output (`conditional_turn_from_arc`, `conditional_turn_to_arc`, `conditional_turn_time_windows`) alongside the existing fixed turn restrictions, so that a single pipeline run yields everything needed for time-dependent routing.

---

## Background

The existing `conditional_turn_extract` tool (`RoutingKit/bin/conditional_turn_extract`) is a standalone binary that:

1. Re-scans the PBF for `restriction:conditional` relations and unconditional via-way restrictions
2. Loads the graph from disk (requires `first_out`, `head`, **`way`**, `latitude`, `longitude`)
3. Resolves raw restrictions to `(from_arc, to_arc)` pairs using geometry-based junction matching
4. Parses condition strings into `TimeWindow` structs
5. Saves three output files: `conditional_turn_from_arc`, `conditional_turn_to_arc`, `conditional_turn_time_windows`

CCH-Generator currently cannot feed this tool because it does not save the `way` vector.

---

## Change 1: Save the `way` vector in `generate_graph.cpp`

### Why the `way` vector is missing

The `way` vector maps each arc to its **routing way ID** — an internal RoutingKit integer that identifies which OSM way an arc belongs to. It exists in `OSMRoutingGraph` (the full internal graph struct returned by `load_osm_routing_graph_from_pbf`), but is deliberately excluded from the simplified public structs `SimpleOSMCarRoutingGraph` and `SimpleOSMMotorcycleRoutingGraph`.

Here is the data flow inside `simple_load_osm_car_routing_graph_from_pbf` (in `RoutingKit/src/osm_simple.cpp`):

```
load_osm_id_mapping_from_pbf(pbf, ...)     → OSMRoutingIDMapping
    ↓
load_osm_routing_graph_from_pbf(pbf, ...)  → OSMRoutingGraph  ← has .way
    ↓
compute travel_time from geo_distance / way_speed[routing_graph.way[a]]
    ↓
move fields into SimpleOSMCarRoutingGraph  ← does NOT have .way
    ↓
routing_graph goes out of scope            ← .way is destroyed
```

The simple loader uses `routing_graph.way[a]` as a lookup index during the travel-time calculation loop (line 55 of `osm_simple.cpp`), then never touches it again. When `routing_graph` goes out of scope, `way` is destroyed. The `SimpleOSMCarRoutingGraph` struct was designed for end-user simplicity — it only exposes what a basic routing query needs.

The conditional turn resolver, however, **requires `way` on disk** because it must map OSM way IDs back to graph arcs to resolve via-way restrictions. Specifically, `conditional_restriction_resolver.cpp` line 74 does:

```cpp
g.way = load_vector<unsigned>(graph_dir + "/way");
```

If this file doesn't exist, the resolver crashes.

### Options

| Option | Approach | Pros | Cons |
|--------|----------|------|------|
| **A** | Bypass the simple loader — call `load_osm_id_mapping_from_pbf` + `load_osm_routing_graph_from_pbf` directly in `generate_graph.cpp`, replicate the travel-time computation, and save `routing_graph.way` | Full control, no RoutingKit API changes, matches what `osm_simple.cpp` does internally | More code in generator (≈30 lines per profile), duplicates simple loader logic |
| **B** | Modify `SimpleOSMCarRoutingGraph` / `SimpleOSMMotorcycleRoutingGraph` to include an optional `way` field | Clean API, one-line change in generator | Modifies RoutingKit's public API — may conflict with upstream or surprise other users |
| **C** | Call the simple loader as-is, then re-load the PBF a second time just to extract `way` | Zero changes to existing code | Wasteful — full PBF re-parse for one vector. PBF loading is the slowest step (≈10s for Hanoi). |

### Recommendation: Option A

Option A is the cleanest fit. The generator already has profile-specific branches in `load_graph()`. Replacing the simple loader call with the two-step `load_osm_id_mapping_from_pbf` → `load_osm_routing_graph_from_pbf` sequence gives us `routing_graph.way` for free. The travel-time computation is 4 lines (identical to what the simple loader does).

#### Implementation sketch for Option A

```cpp
// In load_graph(), replace:
//   auto graph = simple_load_osm_car_routing_graph_from_pbf(...)
// with:

auto mapping = RoutingKit::load_osm_id_mapping_from_pbf(
    pbf_path,
    nullptr, // no node filter
    [&](uint64_t osm_way_id, const RoutingKit::TagMap& tags) {
        return RoutingKit::is_osm_way_used_by_cars(osm_way_id, tags, log_fn);
    },
    log_fn
);

unsigned routing_way_count = mapping.is_routing_way.population_count();
std::vector<unsigned> way_speed(routing_way_count);

auto routing_graph = RoutingKit::load_osm_routing_graph_from_pbf(
    pbf_path,
    mapping,
    [&](uint64_t osm_way_id, unsigned routing_way_id, const RoutingKit::TagMap& way_tags) {
        way_speed[routing_way_id] = RoutingKit::get_osm_way_speed(osm_way_id, way_tags, log_fn);
        return RoutingKit::get_osm_car_direction_category(osm_way_id, way_tags, log_fn);
    },
    [&](uint64_t osm_relation_id, const std::vector<RoutingKit::OSMRelationMember>& members,
        const RoutingKit::TagMap& tags, std::function<void(RoutingKit::OSMTurnRestriction)> on_new) {
        return RoutingKit::decode_osm_car_turn_restrictions(osm_relation_id, members, tags, on_new, log_fn);
    },
    log_fn
);

mapping = RoutingKit::OSMRoutingIDMapping(); // release memory

// Compute travel_time (same formula as osm_simple.cpp)
out.travel_time = routing_graph.geo_distance;
for (unsigned a = 0; a < out.travel_time.size(); ++a) {
    out.travel_time[a] *= 18000;
    out.travel_time[a] /= way_speed[routing_graph.way[a]];
    out.travel_time[a] /= 5;
}

// Move all vectors including way
out.way = std::move(routing_graph.way);  // ← the key addition
out.first_out = std::move(routing_graph.first_out);
out.head = std::move(routing_graph.head);
// ... etc
```

The motorcycle branch is identical but calls `is_osm_way_used_by_motorcycles`, `get_osm_motorcycle_way_speed`, `get_osm_motorcycle_direction_category`, and `decode_osm_motorcycle_turn_restrictions`.

#### What the `GeneratedGraph` struct gains

```cpp
struct GeneratedGraph {
    // ... existing fields ...
    std::vector<unsigned> way;  // arc → routing way ID (new)
};
```

And `save_graph()` adds:

```cpp
save_named_vector(output_dir, "way", graph.way);
```

---

## Change 2: Invoke conditional turn extraction as a pipeline step

### Option 2a: External binary (recommended for now)

`run_pipeline.sh` calls `RoutingKit/bin/conditional_turn_extract` after graph generation:

```bash
CONDITIONAL_BIN="${REPO_ROOT}/RoutingKit/bin/conditional_turn_extract"

echo "[2.5/N] Extract conditional turns for car graph"
"${CONDITIONAL_BIN}" "${INPUT_PBF}" "${CAR_DIR}"

echo "[4.5/N] Extract conditional turns for motorcycle graph"
"${CONDITIONAL_BIN}" "${INPUT_PBF}" "${MOTORCYCLE_DIR}"
```

**Prerequisite**: `conditional_turn_extract` must be built as part of RoutingKit (`make` builds it automatically via `generate_make_file`).

### Option 2b: Link resolver into CCH-Generator (future)

Add the resolver sources to `CMakeLists.txt` and call `resolve_conditional_restrictions()` directly from `generate_graph.cpp`. This eliminates the separate binary but adds CMake complexity (must link `osm_condition_parser.cpp`, `conditional_restriction_decoder.cpp`, `conditional_restriction_resolver.cpp` plus their transitive dependencies).

Not recommended now — the external binary already works and keeps the build simple.

---

## Change 3: Validate conditional turn output in `validate_graph.cpp`

Add an optional `--conditional` flag (or auto-detect by file existence) that runs these checks:

| # | Check | What it validates |
|---|-------|-------------------|
| 15 | **Conditional turn vector consistency** | `conditional_turn_from_arc.size() == conditional_turn_to_arc.size()` |
| 16 | **Conditional turn arc bounds** | Every `conditional_turn_{from,to}_arc[i] < arc_count` |
| 17 | **Conditional turn sorting** | `conditional_turn_from_arc` is sorted (same invariant as forbidden turns) |
| 18 | **Time window file integrity** | Offset array has `n+1` entries, offsets are monotonically non-decreasing, packed data size matches `offsets[n] * 5` bytes |
| 19 | **No overlap with forbidden turns** | No `(from, to)` pair appears in both forbidden turns and conditional turns (an unconditional ban supersedes any conditional — if overlap exists, the conditional is redundant/stale) |

### Auto-detection vs explicit flag

Auto-detect is simpler: if `conditional_turn_from_arc` exists in the graph directory, run the checks. This avoids adding yet another CLI flag and matches the pattern of the `cch_exp_perm` auto-detection in the turn-expanded block.

---

## Change 4: Update scripts

### `run_pipeline.sh`

Insert conditional extraction steps after each graph generation + validation pair:

```
[1/10] Generate car graph
[2/10] Validate car graph (standard)
[3/10] Extract conditional turns for car graph          ← NEW
[4/10] Validate car graph (with conditional turns)      ← NEW (re-run with conditionals present)
[5/10] Generate motorcycle graph
[6/10] Validate motorcycle graph (standard)
[7/10] Extract conditional turns for motorcycle graph   ← NEW
[8/10] Validate motorcycle graph (with conditionals)    ← NEW
[9/10] Generate IFC permutations + validate (car)
[10/10] Generate IFC permutations + validate (motorcycle)
```

### `compare_profiles.sh`

Add conditional turn count to the profile comparison report:

```python
cond_from = load_u32(graph_dir / "conditional_turn_from_arc")
# ...
print(f"Conditional turns  car={car['cond_turns']}  motorcycle={motor['cond_turns']}  delta=...")
```

---

## Profile-awareness caveat

The current `conditional_restriction_resolver.cpp` hardcodes `is_osm_way_used_by_cars` when rebuilding ID mappings (line 294). For motorcycle profiles, this produces incorrect mappings — motorcycle includes `highway=track` and `highway=path` which car excludes, so the routing way IDs would differ.

This is a **known Phase 2 concern** documented in `docs/Motorcycle Profile Implementation.md`. For now, conditional turn extraction only works correctly with the car profile. Adding a `--profile` parameter to the resolver (or making the way-filter callback configurable) is deferred.

---

## Dependency order

```
Change 1 (save way vector)
  ↓
Change 2 (invoke conditional_turn_extract — needs way on disk)
  ↓
Change 3 (validate conditional output — needs files to exist)
  ↓
Change 4 (script updates — wires everything together)
```

Changes 3 and 4 can be developed in parallel once Change 2 is working.

---

## Task ownership

| Task | Owner |
|------|-------|
| Change 1: Save `way` vector (Option A) | Human + AI |
| Change 2: Pipeline integration (Option 2a) | Human + AI |
| Change 3: Conditional turn validation checks | Human + AI |
| Change 4: Script updates | Human + AI |
| Profile flag for resolver (Phase 2) | Deferred |
