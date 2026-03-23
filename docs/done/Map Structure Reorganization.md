# Reorganize Maps/data Output Folder Structure

## Goal

Split only **conditional turn** data into a subfolder. Graph, node, and turn files stay flat in the profile root for full rust_road_router compatibility (0 Rust source changes). [line_graph/](file:///home/thomas/VTS/Hanoi-Routing/rust_road_router/engine/src/datastr/graph/time_dependent/graph.rs#489-501) already lives in a subfolder—no change needed.

## Layout

```
hanoi_motorcycle/
├── first_out                         ← Graph + nodes + turns (flat, unchanged)
├── head
├── travel_time
├── geo_distance
├── way
├── latitude
├── longitude
├── forbidden_turn_from_arc
├── forbidden_turn_to_arc
├── conditional_turns/                ← NEW subfolder
│   ├── conditional_turn_from_arc
│   ├── conditional_turn_to_arc
│   └── conditional_turn_time_windows
└── line_graph/                       ← Already a subfolder (unchanged)
    ├── first_out
    ├── head
    ├── travel_time
    ├── latitude
    └── longitude
```

> [!NOTE]  
> Based on [eligibility analysis](#separation-eligibility-analysis): separating `conditional_turns/` has **zero impact** on rust_road_router (no binaries read these files). Keeping everything else flat avoids touching 50+ upstream Rust binaries.

---

## Proposed Changes

### [MODIFY] [conditional_turn_extract.cpp](file:///home/thomas/VTS/Hanoi-Routing/RoutingKit/src/conditional_turn_extract.cpp)

- When `output_dir` equals `graph_dir` (the default), write to `graph_dir + "/conditional_turns/"` instead of `graph_dir + "/"`.
- When an explicit `output_dir` is given, write to `output_dir + "/conditional_turns/"`.
- Create the `conditional_turns/` subdirectory before writing.
- Update usage message to reflect the new output structure.

### [MODIFY] [validate_graph.cpp](file:///home/thomas/VTS/Hanoi-Routing/CCH-Generator/src/validate_graph.cpp)

- Change conditional file paths from `graph_dir / "conditional_turn_*"` to `graph_dir / "conditional_turns" / "conditional_turn_*"`.

### [MODIFY] [run_pipeline](file:///home/thomas/VTS/Hanoi-Routing/CCH-Generator/scripts/run_pipeline)

- No changes expected (binaries handle paths internally). Validate invocations still work after the above changes.

---

## Separation Eligibility Analysis

*(Retained for reference — this is what drove the decision to keep graph+nodes+turns flat.)*

| Separation | Library OK? | Rust binaries to modify | Risk |
|-----------|-------------|------------------------|------|
| `conditional_turns/` | ✅ | **0** | **None** ← chosen |
| `turns/` | ✅ | 6 research binaries | Low |
| [graph/](file:///home/thomas/VTS/Hanoi-Routing/rust_road_router/engine/src/datastr/graph/time_dependent/graph.rs#314-317) | ✅ | All ~50 binaries | High |
| [nodes/](file:///home/thomas/VTS/Hanoi-Routing/rust_road_router/engine/src/datastr/graph/time_dependent/graph.rs#508-511) | ✅ | ~12 binaries | Medium |

---

## Verification Plan

1. Build CCH-Generator and RoutingKit (user runs builds per project rules).
2. Run `CCH-Generator/scripts/run_pipeline Maps/hanoi.osm.pbf`.
3. Confirm `conditional_turns/` subfolder exists with the 3 expected files.
4. Confirm all other files remain flat in the profile root.
5. Run `validate_graph` — all checks should pass including conditional turn validation.
