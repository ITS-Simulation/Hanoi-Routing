# Motorcycle Routing Profile Implementation Plan

## Overview

Add a motorcycle routing profile to RoutingKit following the same pattern as the existing car, bicycle, and pedestrian profiles. The profile is **purely additive** — zero modifications to existing files. It slots into the same two-pass PBF loader API that all other profiles use.

The profile is tuned for **Hanoi, Vietnam**, where motorcycles are the dominant vehicle type (~80% of traffic) and where narrow alleys (`path`, `track`) are primary travel arteries that the car profile excludes.

---

## How the Profile API Works

RoutingKit's OSM loader is parameterized by exactly four callbacks:

```
Pass 1:  load_osm_id_mapping_from_pbf
           └── is_osm_way_used_by_X(way_id, tags) → bool

Pass 2:  load_osm_routing_graph_from_pbf
           ├── way_callback(way_id, routing_way_id, tags)
           │     ├── get_osm_X_way_speed(...)  → unsigned km/h
           │     └── get_osm_X_direction_category(...) → OSMWayDirectionCategory
           └── turn_restriction_decoder(relation_id, members, tags, emit_fn)
                 └── decode_osm_X_turn_restrictions(...)
```

`simple_load_osm_X_routing_graph_from_pbf` wires these together, then converts:
```
travel_time[arc] = geo_distance[arc] (m) × 3600 / speed[way] (km/h)  →  milliseconds
```

---

## Files to Add / Modify

| File | Action | What |
|------|---------|------|
| `RoutingKit/src/osm_profile.cpp` | Add | 4 new functions |
| `RoutingKit/include/routingkit/osm_profile.h` | Add | 4 declarations |
| `RoutingKit/include/routingkit/osm_simple.h` | Add | `SimpleOSMMotorcycleRoutingGraph` struct + loader declaration |
| `RoutingKit/src/osm_simple.cpp` | Add | `simple_load_osm_motorcycle_routing_graph_from_pbf` |

No need to build afterwards

---

## Function Specifications

### 1. `is_osm_way_used_by_motorcycles`

**Base**: start from `is_osm_way_used_by_cars`, then apply these diffs:

| Change | Detail |
|--------|--------|
| Access tag | Check `motorcycle=no` instead of `motorcar=no` (keep `motor_vehicle=no`) |
| `path` and `track` | **Include** unless `motorcycle=no`; critical for Hanoi alleys |
| `bicycle_road` | Allow if `motorcycle=yes` (mirrors car's `motorcar=yes` check) |
| `motorway`, `trunk` | Keep allowed (motorcycles can use them in Vietnam unless explicitly excluded) |

OSM tag priority for access:
```
motorcycle=no              → excluded
motor_vehicle=no           → excluded
access=no (no other override) → excluded
access=yes/permissive/destination/delivery/designated → included
(no access tag)            → highway class decides
```

---

### 2. `get_osm_motorcycle_direction_category`

Check `oneway:motorcycle` **before** falling back to the standard `oneway` logic:

```
oneway:motorcycle=-1 / opposite  → only_open_backwards
oneway:motorcycle=yes / 1 / true → only_open_forwards
oneway:motorcycle=no / 0 / false → open_in_both   ← overrides a car-only oneway

(no oneway:motorcycle tag)       → delegate to get_osm_car_direction_category logic
```

The fallback is identical to `get_osm_car_direction_category` (roundabout → forwards, motorway → forwards, otherwise → open_in_both).

---

### 3. `get_osm_motorcycle_way_speed`

Check tags in this priority order:

1. `maxspeed:motorcycle` — motorcycle-specific speed limit (parse with same `parse_maxspeed_value` helper)
2. `maxspeed` — general speed limit (fallback, same as car)
3. Highway-class defaults (Vietnam-adjusted):

| Highway class | Car default (km/h) | Motorcycle default (km/h) | Rationale |
|---------------|--------------------|---------------------------|-----------|
| `motorway` | 90 | 80 | Vietnam expressway limit for motorcycles |
| `motorway_link` | 45 | 40 | |
| `trunk` | 85 | 70 | National road limit for motorcycles |
| `trunk_link` | 40 | 35 | |
| `primary` | 65 | 60 | Urban arterial |
| `primary_link` | 30 | 30 | |
| `secondary` | 55 | 50 | |
| `secondary_link` | 25 | 25 | |
| `tertiary` | 40 | 40 | |
| `tertiary_link` | 20 | 20 | |
| `unclassified` | 25 | 30 | Motorcycles navigate these more efficiently |
| `residential` | 25 | 25 | |
| `living_street` | 10 | 15 | |
| `service` | 8 | 15 | |
| `track` | 8 | 15 | **New** — motorbikes use dirt tracks |
| `path` | *(not in car)* | 15 | **New** — Hanoi alleys (ngõ/hẻm) |
| `ferry` | 5 | 5 | |

---

### 4. `decode_osm_motorcycle_turn_restrictions`

**Base**: start from `decode_osm_car_turn_restrictions`, then apply:

| Change | Detail |
|--------|--------|
| Tag priority | Check `restriction:motorcycle` first, fall back to `restriction` |
| `except` tag guard | If using the general `restriction` tag, check if the relation's `except` tag contains `motorcycle` or `motor_vehicle` — if so, skip (restriction does not apply) |

```
restriction:motorcycle present  → use it directly
restriction:motorcycle absent:
  restriction present:
    except contains "motorcycle" or "motor_vehicle" → skip (motorcycle exempt)
    else → apply restriction
```

---

### 5. `SimpleOSMMotorcycleRoutingGraph` (in `osm_simple.h`)

Same fields as `SimpleOSMCarRoutingGraph`:

```cpp
struct SimpleOSMMotorcycleRoutingGraph {
    std::vector<unsigned> first_out;
    std::vector<unsigned> head;
    std::vector<unsigned> travel_time;
    std::vector<unsigned> geo_distance;
    std::vector<float>    latitude;
    std::vector<float>    longitude;
    std::vector<unsigned> forbidden_turn_from_arc;
    std::vector<unsigned> forbidden_turn_to_arc;

    unsigned node_count() const { return first_out.size() - 1; }
    unsigned arc_count()  const { return head.size(); }
};

SimpleOSMMotorcycleRoutingGraph simple_load_osm_motorcycle_routing_graph_from_pbf(
    const std::string& pbf_file,
    const std::function<void(const std::string&)>& log_message = nullptr,
    bool all_modelling_nodes_are_routing_nodes = false,
    bool file_is_ordered_even_though_file_header_says_that_it_is_unordered = false
);
```

---

### 6. `simple_load_osm_motorcycle_routing_graph_from_pbf` (in `osm_simple.cpp`)

Structurally identical to `simple_load_osm_car_routing_graph_from_pbf`:

```
load_osm_id_mapping_from_pbf  ← is_osm_way_used_by_motorcycles
load_osm_routing_graph_from_pbf
  way_callback  ← get_osm_motorcycle_way_speed + get_osm_motorcycle_direction_category
  turn decoder  ← decode_osm_motorcycle_turn_restrictions
travel_time = geo_distance × 18000 / speed / 5  (same formula)
move fields into SimpleOSMMotorcycleRoutingGraph
```

---

## Usage (after implementation)

```cpp
#include <routingkit/osm_simple.h>
#include <routingkit/vector_io.h>

auto graph = simple_load_osm_motorcycle_routing_graph_from_pbf("hanoi.osm.pbf");
save_vector("moto_dir/first_out",                 graph.first_out);
save_vector("moto_dir/head",                      graph.head);
save_vector("moto_dir/travel_time",               graph.travel_time);
save_vector("moto_dir/geo_distance",              graph.geo_distance);
save_vector("moto_dir/latitude",                  graph.latitude);
save_vector("moto_dir/longitude",                 graph.longitude);
save_vector("moto_dir/forbidden_turn_from_arc",   graph.forbidden_turn_from_arc);
save_vector("moto_dir/forbidden_turn_to_arc",     graph.forbidden_turn_to_arc);
```

The resulting binary files are drop-in compatible with the rust_road_router CCH pipeline — run `flow_cutter_cch_order.sh moto_dir` to generate `cch_perm`, then the server loads it identically to the car graph.

---

## Phase 2: Conditional Restrictions for Motorcycle

The `conditional_turn_extract` tool currently hardcodes `is_osm_way_used_by_cars` in the ID-mapping rebuild pass ([conditional_restriction_resolver.cpp:301](../RoutingKit/src/conditional_restriction_resolver.cpp#L301)). To support motorcycle conditional restrictions:

- Add a `--profile [car|motorcycle]` flag to `conditional_turn_extract`
- Pass the appropriate `is_osm_way_used_by_X` function to `resolve_conditional_restrictions`
- Also handle `restriction:motorcar:conditional` → `restriction:motorcycle:conditional` in `conditional_restriction_decoder.cpp`

This is deferred to Phase 2 — the motorcycle base graph is fully functional without it.

---

## Known Limitations (Phase 1 scope)

- `maxspeed:motorcycle:conditional` (time-varying speed limits for motorcycles) — not parsed
- Multi-modal `access` hierarchies (e.g. `motor_vehicle=yes` + `motorcycle=no` at node level) — not handled (same limitation as car profile)
- Speed defaults are Vietnam-tuned; international use may need adjustment
