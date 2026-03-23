# Audit Round 3 — Logic Loophole Fixes

Third-pass audit after the multi-via-way, bounds-check, lexicographic sorting, day-mask cleanup, and pipeline fail-fast changes from round 2. All findings below are logic-level correctness, safety, or silent-data-loss issues.

---

## Findings

### F1 — Resolver [node_count()](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/conditional_restriction_resolver.cpp#63-64) underflow on empty graph

**File:** [RoutingKit/src/conditional_restriction_resolver.cpp](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/conditional_restriction_resolver.cpp) — line 63

```cpp
unsigned node_count() const { return first_out.size() - 1; }
```

If `first_out` is empty (e.g., a corrupt or trivially empty graph directory), `first_out.size()` returns 0  and the subtraction wraps to [unsigned(-1)](file:///home/thomas/VTS/Hanoi-Routing/CCH-Generator/src/validate_graph.cpp#69-75) = 4,294,967,295. Every subsequent consumer of [node_count()](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/conditional_restriction_resolver.cpp#63-64) — `tail.resize(ac)`, the `is_endpoint_of_a(nc, false)` allocation in [find_junction_node](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/conditional_restriction_resolver.cpp#126-167), etc. — will either allocate gigabytes or silently produce garbage.

The validator's [GraphData](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/conditional_restriction_resolver.cpp#47-66) at `validate_graph.cpp:41` already handles this correctly (`return first_out.empty() ? 0 : first_out.size()-1;`), but the resolver's version does not.

**Fix:** Add an empty guard: `return first_out.empty() ? 0 : first_out.size() - 1;`

---

### F2 — Missing [way](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/osm_profile.cpp#694-708) vector consistency check in resolver's [load_graph](file:///home/thomas/VTS/Hanoi-Routing/CCH-Generator/src/validate_graph.cpp#119-131)

**File:** [RoutingKit/src/conditional_restriction_resolver.cpp](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/conditional_restriction_resolver.cpp) — [load_graph()](file:///home/thomas/VTS/Hanoi-Routing/CCH-Generator/src/validate_graph.cpp#119-131) (lines 67–103)

After loading all vectors, the function validates nothing. In particular, `way.size()` must equal [arc_count](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/conditional_restriction_resolver.cpp#64-65) (= `head.size()`) for the way→arc index to be meaningful. If the [way](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/osm_profile.cpp#694-708) file is truncated or missing entries (e.g., from a partial write), the downstream `way_count` computation (lines 90–93) reads garbage indices, and `compute_sort_permutation_using_key` may access out-of-bounds memory.

[generate_graph.cpp](file:///home/thomas/VTS/Hanoi-Routing/CCH-Generator/src/generate_graph.cpp) has a similar check at line 185 (`routing_graph.way.size() != out.travel_time.size()`) and the line graph generator validates all inputs upfront. The resolver omits this.

**Fix:** After loading, add consistency checks:
- `way.size() == arc_count`
- `latitude.size() == node_count && longitude.size() == node_count`

Throw on mismatch with a descriptive error.

---

### F3 — Step 2b timer/log swallowed on early-return paths inside IIFE

**File:** [RoutingKit/src/conditional_turn_extract.cpp](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/conditional_turn_extract.cpp) — lines 156–202

Step 2b is wrapped in an IIFE lambda. The `timer += get_micro_time()` and log output (lines 198–201) are inside the lambda. When the lambda hits an early `return` (catch at line 164–167 or size-mismatch at line 170–172), the timer is never finalized and no Step 2b timing log is printed.

Step 3 then resets `timer = -get_micro_time()` (line 206), so the stale timer value is never consumed — no data corruption. But the missing log makes it look like Step 2b didn't run at all, which is confusing when debugging.

**Fix:** Move `timer += get_micro_time()` and the timing log outside the lambda, immediately after the `}();` call at line 202. Inside the early-return paths, add a brief log before returning (e.g., "skipping overlap filter").

---

### F4 — `motorcar` variable shadowed in `is_osm_way_used_by_cars` bicycle_road block (UPSTREAM)

**File:** `RoutingKit/src/osm_profile.cpp` — lines 191 and 228

At line 191, the outer scope declares `const char*motorcar = tags["motorcar"];` and checks it for `"no"` at line 192.

At line 228 inside the `bicycle_road` block, `motorcar` is re-declared: `auto motorcar = tags["motorcar"];`. This shadows the outer `motorcar`. The shadowed re-declaration works correctly (it re-fetches the tag), but it's misleading dead code — the outer `motorcar` is already available and already holds the same value.

**Verdict**: Upstream RoutingKit original code — will not be modified. Harmless (produces correct behavior).

---

## Proposed Changes

### RoutingKit — Conditional Restriction Resolver

#### [MODIFY] [conditional_restriction_resolver.cpp](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/conditional_restriction_resolver.cpp)

- **F1**: Change [node_count()](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/conditional_restriction_resolver.cpp#63-64) to return 0 when `first_out` is empty.
- **F2**: Add consistency checks in [load_graph()](file:///home/thomas/VTS/Hanoi-Routing/CCH-Generator/src/validate_graph.cpp#119-131) after loading all vectors: `way.size() == ac`, `latitude.size() == nc`, `longitude.size() == nc`.

---

### RoutingKit — Conditional Turn Extract

#### [MODIFY] [conditional_turn_extract.cpp](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/conditional_turn_extract.cpp)

- **F3**: Move Step 2b timer finalization and log outside the lambda. Add early-return log messages inside catch and size-mismatch paths.

---

### Docs

#### [MODIFY] [CHANGELOGS.md](file:///home/thomas/VTS/Hanoi-Routing/docs/CHANGELOGS.md)

- Add changelog entry for this audit round with all fixes.

---

## Verification Plan

### Manual Verification
- You (the user) build each component to confirm compilation succeeds:
  - `cd RoutingKit && make -j"$(nproc)"`
  - `cmake --build CCH-Generator/build -j"$(nproc)"`
- You run the pipeline against `hanoi.osm.pbf` and verify all checks pass
