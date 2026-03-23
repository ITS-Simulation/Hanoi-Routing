# Profile-Aware Conditional Restriction Resolver

> **Goal**: Make `conditional_turn_extract` and the underlying resolver/decoder profile-aware, so motorcycle (and future) conditional turn restrictions are extracted using the correct way filter and tag hierarchy — mirroring the callback pattern already used by `load_osm_routing_graph_from_pbf`.

---

## The Problem

### Two hardcoded car-only coupling points

**Point 1 — Resolver (`conditional_restriction_resolver.cpp` line 294)**

When resolving restrictions, the resolver must rebuild the OSM ID mapping to convert OSM way IDs back to routing way IDs. It hardcodes the car way filter:

```cpp
auto mapping = load_osm_id_mapping_from_pbf(
    pbf_file,
    [](uint64_t, const TagMap&){ return false; },
    [](uint64_t osm_way_id, const TagMap& tags){
        return is_osm_way_used_by_cars(osm_way_id, tags);  // ← hardcoded
    },
    log_message
);
```

When run against a motorcycle graph directory, the `way` vector on disk contains motorcycle routing way IDs (motorcycle includes `highway=track` and `highway=path`, car does not). The rebuilt mapping uses a different set of routing ways, so routing way ID 42 in the car mapping points to a different OSM way than routing way ID 42 in the motorcycle mapping. The mismatch causes:
- Restrictions referencing motorcycle-only ways to be silently dropped (`to_local()` returns `invalid_id`)
- Correctly resolved pairs to reference wrong arcs if ID ranges happen to collide

**Point 2 — Decoder (`conditional_restriction_decoder.cpp` lines 200–207)**

The decoder hardcodes car-specific tag names when scanning restriction relations:

```cpp
const char* conditional_tag = tags["restriction:conditional"];
if(!conditional_tag)
    conditional_tag = tags["restriction:motorcar:conditional"];

const char* unconditional_tag = tags["restriction"];
if(!unconditional_tag)
    unconditional_tag = tags["restriction:motorcar"];
```

For motorcycles, the correct priority order is `restriction:motorcycle:conditional` → `restriction:conditional`, and `restriction:motorcycle` → `restriction`. Without this, motorcycle-specific time-window restrictions are missed or misattributed.

---

## Proposed Solution: Callback Parameters

Rather than switching on a profile string inside the decoder/resolver, expose the profile-specific behaviour as **callbacks at the call site** — the same pattern `load_osm_routing_graph_from_pbf` already uses for the four profile functions.

---

## Change A: `ConditionalTagPriority` struct + decoder API

### New struct (in `conditional_restriction_decoder.h`)

```cpp
struct ConditionalTagPriority {
    // Primary tag for conditional restrictions (profile-specific)
    // e.g. "restriction:motorcycle:conditional"
    const char* primary_conditional;

    // Fallback tag for conditional restrictions (generic)
    // e.g. "restriction:conditional"
    const char* fallback_conditional;

    // Primary tag for unconditional restrictions (profile-specific)
    // e.g. "restriction:motorcycle"
    const char* primary_unconditional;

    // Fallback tag for unconditional restrictions (generic)
    // e.g. "restriction"
    const char* fallback_unconditional;
};

// Factory functions — one per profile
inline ConditionalTagPriority car_conditional_tag_priority() {
    return {
        "restriction:motorcar:conditional",
        "restriction:conditional",
        "restriction:motorcar",
        "restriction"
    };
}

inline ConditionalTagPriority motorcycle_conditional_tag_priority() {
    return {
        "restriction:motorcycle:conditional",
        "restriction:conditional",
        "restriction:motorcycle",
        "restriction"
    };
}
```

### Updated `scan_conditional_restrictions_from_pbf` signature

```cpp
// New primary overload:
void scan_conditional_restrictions_from_pbf(
    const std::string& pbf_file,
    std::function<void(RawConditionalRestriction)> on_restriction,
    ConditionalTagPriority tag_priority,                           // ← NEW
    std::function<void(const std::string&)> log_message = nullptr
);

// Backward-compatible overload (defaults to car):
inline void scan_conditional_restrictions_from_pbf(
    const std::string& pbf_file,
    std::function<void(RawConditionalRestriction)> on_restriction,
    std::function<void(const std::string&)> log_message = nullptr
){
    scan_conditional_restrictions_from_pbf(
        pbf_file, on_restriction,
        car_conditional_tag_priority(),
        log_message
    );
}
```

### Implementation change in `conditional_restriction_decoder.cpp`

Replace the hardcoded tag lookups:

```cpp
// Before:
const char* conditional_tag = tags["restriction:conditional"];
if(!conditional_tag)
    conditional_tag = tags["restriction:motorcar:conditional"];
const char* unconditional_tag = tags["restriction"];
if(!unconditional_tag)
    unconditional_tag = tags["restriction:motorcar"];

// After:
const char* conditional_tag = tags[tag_priority.primary_conditional];
if(!conditional_tag)
    conditional_tag = tags[tag_priority.fallback_conditional];
const char* unconditional_tag = tags[tag_priority.primary_unconditional];
if(!unconditional_tag)
    unconditional_tag = tags[tag_priority.fallback_unconditional];
```

---

## Change B: Way-filter callback + resolver API

### Updated `resolve_conditional_restrictions` signature

```cpp
// In conditional_restriction_resolver.h:

ResolvedConditionalTurns resolve_conditional_restrictions(
    const std::string& graph_dir,
    const std::string& pbf_file,
    const std::vector<RawConditionalRestriction>& raw_restrictions,
    std::function<bool(uint64_t, const TagMap&)> is_way_used,    // ← NEW
    std::function<void(const std::string&)> log_message = nullptr
);

// Backward-compatible overload (defaults to car):
inline ResolvedConditionalTurns resolve_conditional_restrictions(
    const std::string& graph_dir,
    const std::string& pbf_file,
    const std::vector<RawConditionalRestriction>& raw_restrictions,
    std::function<void(const std::string&)> log_message = nullptr
){
    return resolve_conditional_restrictions(
        graph_dir, pbf_file, raw_restrictions,
        [](uint64_t id, const TagMap& tags){ return is_osm_way_used_by_cars(id, tags); },
        log_message
    );
}
```

### Implementation change in `conditional_restriction_resolver.cpp`

Replace the hardcoded lambda at line 294:

```cpp
// Before:
[](uint64_t osm_way_id, const TagMap& tags){
    return is_osm_way_used_by_cars(osm_way_id, tags);
}

// After:
[&](uint64_t osm_way_id, const TagMap& tags){
    return is_way_used(osm_way_id, tags);
}
```

---

## Change C: `--profile` flag in `conditional_turn_extract.cpp`

### Updated CLI

```
Usage: conditional_turn_extract <pbf_file> <graph_dir> [<output_dir>] [--profile car|motorcycle]
```

Default profile remains `car` for backward compatibility.

### Implementation

```cpp
// Parse --profile flag, then select callbacks:

ConditionalTagPriority tag_priority;
std::function<bool(uint64_t, const TagMap&)> is_way_used;

if(profile == "motorcycle"){
    tag_priority = motorcycle_conditional_tag_priority();
    is_way_used = [](uint64_t id, const TagMap& tags){
        return is_osm_way_used_by_motorcycles(id, tags);
    };
} else {
    // Default: car
    tag_priority = car_conditional_tag_priority();
    is_way_used = [](uint64_t id, const TagMap& tags){
        return is_osm_way_used_by_cars(id, tags);
    };
}

// Pass to both steps:
scan_conditional_restrictions_from_pbf(pbf_file, [&](auto r){ raw.push_back(r); },
    tag_priority, log);

auto resolved = resolve_conditional_restrictions(graph_dir, pbf_file, raw,
    is_way_used, log);
```

---

## Change D: Update pipeline scripts

Once the `--profile` flag exists, both scripts pass it explicitly:

### `run_pipeline.sh`

```bash
echo "[3/10] Extract conditional turns for car graph"
"${CONDITIONAL_BIN}" "${INPUT_PBF}" "${CAR_DIR}" --profile car

echo "[7/10] Extract conditional turns for motorcycle graph"
"${CONDITIONAL_BIN}" "${INPUT_PBF}" "${MOTORCYCLE_DIR}" --profile motorcycle
```

The `Warning: resolver profile support is car-based today...` stderr message in both scripts can be removed.

### `compare_profiles.sh`

```bash
if [[ -x "${CONDITIONAL_BIN}" ]]; then
    echo "Extracting conditional turns for car graph..."
    "${CONDITIONAL_BIN}" "${INPUT_PBF}" "${CAR_DIR}" --profile car
    echo "Extracting conditional turns for motorcycle graph..."
    "${CONDITIONAL_BIN}" "${INPUT_PBF}" "${MOTORCYCLE_DIR}" --profile motorcycle
fi
```

---

## Why callbacks over a profile enum

The callback pattern matches RoutingKit's existing architecture — `load_osm_routing_graph_from_pbf` takes four function callbacks rather than a profile string. The benefits:

1. **Open for extension**: Adding a bicycle profile requires only providing callbacks at the call site. The decoder and resolver implementations are untouched.
2. **Consistent with RoutingKit idioms**: The `ProfileCallbacks` struct already introduced in `generate_graph.cpp` uses exactly this pattern.
3. **No interior switch statements**: A profile string enum forces an internal `if/else` ladder that must be updated for every new profile. Callbacks push that decision to the caller.

---

## Backward compatibility

Both the decoder and resolver gain an additional overload that defaults to the car tag priority / car way filter. The existing call site in `conditional_turn_extract.cpp` (before this change) continues to compile and behave identically. No other files in the codebase call these functions directly.

---

## File change summary

| File | Change |
|------|--------|
| `RoutingKit/include/routingkit/conditional_restriction_decoder.h` | Add `ConditionalTagPriority` struct, factory functions, new overload |
| `RoutingKit/src/conditional_restriction_decoder.cpp` | Replace hardcoded tag names with `tag_priority` parameter |
| `RoutingKit/include/routingkit/conditional_restriction_resolver.h` | Add `is_way_used` parameter overload |
| `RoutingKit/src/conditional_restriction_resolver.cpp` | Replace hardcoded `is_osm_way_used_by_cars` with callback |
| `RoutingKit/src/conditional_turn_extract.cpp` | Parse `--profile` flag, select and pass callbacks |
| `CCH-Generator/scripts/run_pipeline.sh` | Pass `--profile` to conditional extractor, remove warning |
| `CCH-Generator/scripts/compare_profiles.sh` | Pass `--profile` to conditional extractor, remove warning |

---

## Task ownership

| Task | Owner |
|------|-------|
| Change A: Decoder struct + overload | Human + AI |
| Change B: Resolver callback + overload | Human + AI |
| Change C: CLI `--profile` flag | Human + AI |
| Change D: Script updates | Human + AI |
