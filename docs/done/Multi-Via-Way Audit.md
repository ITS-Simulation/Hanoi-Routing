# Audit Round 2 — Logic Loophole Fixes

Second-pass audit of the Hanoi-Routing codebase after the multi-via-way, conditional filter, and degree-2 compression changes. All findings below are logic-level oversights — correctness, safety, or silent-data-loss issues.

---

## Findings

### F1 — Line graph generator loads `geo_distance` but never outputs it

**File:** `CCH-Advanced-Generator/src/generate_line_graph.rs`

The generator loads and validates `geo_distance` (lines 83–110) but never writes a corresponding `geo_distance` vector for the line graph. The loaded data is unused — pure dead code.

**Downstream impact**: Neither the validator nor the server load `geo_distance` from the line graph directory. The CCH pipeline (ordering, customization, queries) does not use `geo_distance` at all. The only consumers of `geo_distance` are conversion tools (`import_mapbox_live`, `gps_to_td_for_here`, etc.) that compute `travel_time` from speed data — and these only run on the original graph, not the line graph.

**Decision**: Either remove the dead load/validation code, or add the output write for future-proofing. The correct line-graph `geo_distance` for an edge `(e1 → e2)` in the expanded graph is `geo_distance[e1]` (the physical distance of the source arc), mirroring how `travel_time` is embedded via `link.weight + turn_cost`.

---

### ~~F2 — Timer not reset inside Step 2b IIFE early-return paths~~ (WITHDRAWN)

**File:** `RoutingKit/src/conditional_turn_extract.cpp`

**Original claim**: Early returns in the Step 2b lambda leave `timer` holding garbage, causing wrong timing output.

**Correction**: Both the `timer += get_micro_time()` call (line 198) and the log message (line 199) are **inside the lambda**. On early return, neither executes — the lambda just returns silently with a warning log. The next step (Step 3, line 206) resets `timer` independently via `timer = -get_micro_time()`. No garbage timer value is ever printed.

**Verdict**: Not a bug. No fix needed.

---

### F3 — `find_junction_node` has no bounds check on way ID access to `first_index_of_way`

**File:** `RoutingKit/src/conditional_restriction_resolver.cpp`

`find_junction_node` at line 132 accesses `g.first_index_of_way[way_a]` and `g.first_index_of_way[way_a+1]` without checking that `way_a + 1 < first_index_of_way.size()` (and same for `way_b`). If a routing way ID exceeds the way count computed during `load_graph` (lines 90–93), this is an out-of-bounds read.

**Practical risk**: Low. The way IDs are produced by `routing_way.to_local()` which maps from the same PBF scan that built `first_index_of_way`, so the IDs should always be in range. However, the mapping is rebuilt from a second PBF scan (line 299), and if the PBF or way-filter callback differs between graph generation and restriction resolution, IDs could mismatch.

**Fix:** Add an early check `if (way_a >= way_count || way_b >= way_count) return invalid_id;` where `way_count = first_index_of_way.size() - 1`.

---

### ~~F4 — Decoder `type` tag filter too strict~~ (WITHDRAWN)

**Verdict:** Self-dismissed during original audit. Not a bug — the existing logic is correct.

---

### F5 — Validator conditional turn sorting check only verifies primary key

**File:** `CCH-Generator/src/validate_graph.cpp`

At line 527, the validator checks:
```cpp
const bool pass = std::is_sorted(conditional_from_arc.begin(), conditional_from_arc.end());
```

This only verifies that `conditional_turn_from_arc` is sorted, not that the full `(from_arc, to_arc)` pair sequence is lexicographically sorted. The resolver sorts by full `(from_arc, to_arc)` pairs (lines 504–511), so the stronger invariant is what should be validated.

**Fix:** Replace `std::is_sorted` on `from_arc` alone with a loop checking lexicographic order of `(from_arc[i], to_arc[i])` pairs.

---

### F6 — `parse_day_spec` redundant bit-set for `last_day`

**File:** `RoutingKit/src/osm_condition_parser.cpp`

At lines 59–61:
```cpp
for(int d = first_day; d != (last_day + 1) % 7; d = (d + 1) % 7)
    mask |= (1 << d);
mask |= (1 << last_day);
```

The loop iterates from `first_day` through `last_day` (inclusive, with wrapping). The stop condition `d != (last_day + 1) % 7` means `last_day` is the last value processed by the loop body. The subsequent `mask |= (1 << last_day)` is redundant (setting an already-set bit). Same pattern repeats at lines 84–86.

**Verdict:** Harmless (idempotent bit-set). Clean up for clarity only.

---

### F7 — Pipeline script missing `set -euo pipefail`

**File:** `scripts/pipeline`

The pipeline script has no `set -euo pipefail`. If any step fails (e.g. `mkdir`, a binary crashes), the script continues to the next `read -p` prompt rather than aborting. The interactive `read -p` provides some mitigation (user sees the error before pressing Enter), but programmatic safety is still missing.

**Fix:** Add `set -euo pipefail` after the shebang line.

---

## New Findings from Multi-Via-Way Implementation

### F8 — Multi-via-way mandatory turn decomposition may over-restrict at interior junctions

**File:** `RoutingKit/src/conditional_restriction_resolver.cpp` (lines 471–484)

For mandatory via-way restrictions (`only_straight_on`, etc.), `add_mandatory_turn` is called at every junction in the chain (line 472–483). At interior junctions, this forbids all outgoing arcs except the via-way continuation. But the mandatory semantics should only apply to the entry junction (from_way → via_way) and exit junction (via_way → to_way) — at interior junctions between consecutive via-ways, the path through the chain is already the only valid continuation by construction.

**Practical impact**: Minimal for Hanoi — mandatory multi-via-way restrictions are extremely rare in OSM. The over-restriction at interior junctions would only matter if there are other ways branching off at those junctions, which is uncommon for the short intermediate segments typical of multi-via-way restrictions.

**Verdict**: Noted for correctness, but not a priority fix. The conservative behavior errs on the side of safety.

---

### F9 — Step 2b overlap filter cannot catch unconditional via-way overlaps (UPSTREAM LIMITATION)

**File:** `RoutingKit/src/conditional_turn_extract.cpp` (lines 156–202) / `RoutingKit/src/osm_profile.cpp` (lines 973–976, 1125–1128)

Step 2b removes conditional turns that overlap with unconditional forbidden turns. However, unconditional via-way restrictions are **silently dropped** by RoutingKit's upstream `osm_profile.cpp` (lines 973–976 for car, 1125–1128 for motorcycle) — they never make it into `forbidden_turn_from_arc`/`forbidden_turn_to_arc`. This means:

1. If a turn restriction exists as both unconditional via-way AND conditional (different time windows), the unconditional one is silently dropped, and the conditional one survives — producing a conditional-only restriction when it should be unconditional.

2. This is a **persistent upstream weakness** in RoutingKit's original `osm_profile.cpp` decoder, which has no via-way support at all. The code at lines 973–976 explicitly returns early with a commented-out log message — this was a deliberate "not supported" decision by the original RoutingKit authors.

**Verdict**: Upstream limitation in RoutingKit's original codebase — will not be modified. Phase 2 of the multi-via-way plan mitigates this by routing unconditional via-way restrictions through the conditional pipeline with an "always active" condition, bypassing the upstream decoder entirely.

---

## Proposed Changes

### CCH-Advanced-Generator

#### [MODIFY] `CCH-Advanced-Generator/src/generate_line_graph.rs`

- Either write `geo_distance` to line graph output, or remove the dead load/validation code (lines 83, 100–110). Recommended: remove the dead code since `geo_distance` is not needed in the line graph for the current pipeline.

---

### RoutingKit — Conditional Restriction Resolver

#### [MODIFY] `RoutingKit/src/conditional_restriction_resolver.cpp`

- Add bounds check for way IDs before accessing `first_index_of_way` in `find_junction_node` (line 128).

---

### RoutingKit — Condition Parser

#### [MODIFY] `RoutingKit/src/osm_condition_parser.cpp`

- Remove redundant `mask |= (1 << last_day)` after the wrapping-range loop (lines 61 and 86).

---

### CCH-Generator — Validator

#### [MODIFY] `CCH-Generator/src/validate_graph.cpp`

- Strengthen conditional turn sorting check (line 527) to verify full lexicographic `(from_arc, to_arc)` ordering.

---

### Scripts

#### [MODIFY] `scripts/pipeline`

- Add `set -euo pipefail` after the shebang.

---

### Docs

#### [MODIFY] `docs/CHANGELOGS.md`

- Add changelog entry for this audit round with all fixes.

---

## Verification Plan

### Manual Verification
- You (the user) run the build commands for each component to confirm compilation succeeds:
  - `cmake --build CCH-Generator/build -j"$(nproc)"`
  - `cd RoutingKit && make -j"$(nproc)"`
  - `cargo build --release --manifest-path CCH-Advanced-Generator/Cargo.toml`
- You run `validate_graph` against the generated graph and confirm all checks pass
