# Audit Findings — 2026-03-12

Comprehensive audit of all files touched by recent changelog entries (multi-via-way resolution, conditional turns, line graph generator, validator, pipeline scripts).

## Confirmed Bugs

### 1. `osm_condition_parser.cpp:36` — Accepts invalid times `24:30`, `24:59`

**Severity**: Medium — produces semantically invalid time windows (1470+ minutes) that would cause incorrect conditional turn activation.

**Current**:
```cpp
if(h > 24 || m > 59)
```

**Fix**:
```cpp
if(h > 24 || (h == 24 && m != 0) || m > 59)
```

**Rationale**: `24:00` (1440 minutes) is valid as end-of-day boundary per OSM spec. Any minute > 0 with hour 24 is meaningless.

---

### 2. `validate_graph.cpp:461-466, 563-565` — Uninitialized `inversion_index` in diagnostic output

**Severity**: High — causes out-of-bounds array access (index `SIZE_MAX`) when forbidden/conditional turn vectors have different sizes.

**Root cause**: When `forbidden_turn_from_arc.size() != forbidden_turn_to_arc.size()`, the sorting loop at line 447 is skipped (since `pass` is already `false`). The diagnostic branch at line 463 (`else`) then accesses `forbidden_turn_from_arc[inversion_index - 1]` where `inversion_index` is still 0, wrapping to `SIZE_MAX`.

**Fix**: Restructure the diagnostic output so the size-mismatch branch is checked before the inversion-index branch. The current structure is:

```cpp
if(pass) { ... }
else if(sizes_differ) { ... }
else { /* accesses inversion_index - 1 */ }
```

This is actually structurally correct — if sizes differ, we enter the second branch, not the third. The third branch is only reached when sizes are equal AND `pass` is false, which means the loop ran and set `inversion_index > 0`.

**Re-analysis**: On closer inspection, this is **NOT a bug** — the `else if(sizes_differ)` branch catches the size mismatch case before the inversion-index branch. The third `else` is only reachable when sizes are equal and the loop found an inversion (setting `inversion_index >= 1`). The same pattern holds for conditional turns at lines 558-565.

**Status**: FALSE POSITIVE — the branching logic is actually correct.

---

### ~~3–5. `scripts/pipeline` — Hard-coded paths, relative paths, no pre-flight checks~~

**Status**: INTENTIONAL — these are by-design choices for the interactive pipeline script.

---

## Defensive Hardening Opportunities (not bugs)

### A. `conditional_turn_extract.cpp:250-252` — No try-catch around output writes
If one of three output files fails to write, the others may already be written, leaving inconsistent state.

### B. `conditional_restriction_resolver.cpp` — No bounds validation on node IDs
`g.head[arc]` and `g.tail[arc]` are used as array indices without validating they are < `node_count()`. Relies on graph well-formedness invariant.

### C. `generate_graph.cpp:201` — Potential u32 overflow
`geo_distance * 18000` can overflow `unsigned` for edges > ~238 km. Real-world Hanoi edges are well under this limit.

### D. `validate_graph.cpp:758` — Redundant `tail` recomputation
The `tail` array is already built at line 356 but recomputed at line 758 for the line-graph validation section.

---

## Verified Non-Issues

| Finding | Verdict |
|---------|---------|
| `generate_graph.cpp` travel-time formula `*18000 /speed /5` | Matches RoutingKit's canonical `osm_simple.cpp:54-56`. Produces milliseconds via integer arithmetic on geo_distance (meters) and speed (km/h). |
| `flow_cutter_cch_cut_order.sh:25` `normal\` line continuation | Correct — backslash-newline consumed by shell, next line's leading spaces separate arguments. |
| `scripts/pipeline:32-33` binary paths `/lib/` | Verified correct — binaries at `CCH-Generator/lib/cch_generator` and `CCH-Generator/lib/validate_graph`. |
| `generate_line_graph.rs` merge-scan logic | Correct — peekable iterator advances past all lexicographically smaller pairs before checking match. |
| `conditional_restriction_resolver.cpp` `node_count()` | Correct — empty `first_out` returns 0 instead of unsigned underflow. |
| `validate_graph.cpp` inversion_index branching | Correct — `else if(sizes_differ)` catches mismatch before `else` branch that uses `inversion_index`. |
