# CCH-Hanoi: Hanoi-Specific Integration Hub

## Purpose

`CCH-Hanoi` is the Hanoi-specific Rust integration hub that sits on top of `rust_road_router`. While `rust_road_router` provides generic routing algorithms and data structures (CCH, Dijkstra, graph types, I/O), `CCH-Hanoi` provides the Hanoi-specific orchestration layer — tools, conventions, and (in the future) library APIs tailored to the Hanoi road network pipeline.

**The boundary rule:**
- Reusable algorithm/data-structure work → `rust_road_router`
- Hanoi-specific orchestration, conventions, and utility APIs → `CCH-Hanoi`

## Vision

CCH-Hanoi is structured around two design surfaces with distinct timelines:

1. **Immediate: Independent tools** — self-contained utility binaries in `hanoi-tools` that serve the pipeline today (e.g. `generate_line_graph`). Each tool is its own entity, directly using `rust_road_router` engine types.
2. **Future: Rust library APIs** — exposed through `hanoi-core` for programmatic use by other Rust code. The long-term goal is stable, composable APIs for graph loading, customization setup, and query orchestration. Building for API consumption is a first-class objective in future sprints.

The CLI (`hanoi-cli`) will wrap `hanoi-core` once it has APIs to expose. Until then, it is a skeleton.

## Workspace Structure

CCH-Hanoi is a Cargo workspace with exactly three crates:

```
CCH-Hanoi/
├── Cargo.toml                          (workspace root: members = ["crates/*"])
├── rust-toolchain.toml                 (channel = "nightly")
└── crates/
    ├── hanoi-core/                     (library — future CCH implementation and API surface)
    │   ├── Cargo.toml
    │   └── src/
    │       └── lib.rs                  (empty stub)
    ├── hanoi-cli/                      (binary — operator/interactive CLI over core)
    │   ├── Cargo.toml
    │   └── src/
    │       └── main.rs                 (skeleton — no subcommands yet)
    └── hanoi-tools/                    (binaries — independent pipeline utilities)
        ├── Cargo.toml
        └── src/
            └── bin/
                └── generate_line_graph.rs
```

### Crate responsibilities

| Crate | Type | Purpose |
|---|---|---|
| `hanoi-core` | Library | Future home of Hanoi-specific CCH implementation wrapping `rust_road_router` algorithms. Will expose stable Rust APIs for graph loading, customization, query orchestration. Currently an empty stub. |
| `hanoi-cli` | Binary | Operator-facing CLI (`cch-hanoi`) that wraps `hanoi-core` as subcommands. Skeleton until core has APIs. |
| `hanoi-tools` | Binaries | Independent, self-contained utility binaries. Each `.rs` file under `src/bin/` is its own tool. Tools depend on `rust_road_router` directly and optionally on `hanoi-core` if needed. |

### Dependency direction

```
rust_road_router/engine    (upstream — generic algorithms)
        ↑
   hanoi-core              (future Hanoi-specific CCH implementation)
        ↑
   hanoi-cli               (CLI skin over core)

   hanoi-tools             (independent utilities)
        ↑
   rust_road_router/engine (direct dependency)
   hanoi-core              (optional, per-tool, only if needed)
```

Key rules:
- Tools do NOT route through core by default. Each tool declares only what it uses.
- Core never depends on tools at the Cargo level.
- If core ever needs tool functionality, it would invoke the tool binary as a subprocess or the logic would be refactored at that time.

> [!NOTE]
> `rust_road_router` remains a sibling dependency — it is NOT merged into the CCH-Hanoi workspace. Sub-crates that need engine types declare their own path dependency (from `CCH-Hanoi/crates/*`: `../../../rust_road_router/engine`).

### Edition and toolchain

All crates use **edition 2024**. The `rust-toolchain.toml` at the workspace root pins to `nightly`, which is required because `rust_road_router/engine` uses `#![feature(impl_trait_in_assoc_type)]`.

## Current Tool: `generate_line_graph`

The primary tool in CCH-Hanoi today. It reads a base graph (produced by CCH-Generator) and generates a turn-expanded line graph suitable for turn-aware CCH routing.

### What it does

1. Loads the base graph (`first_out`, `head`, `travel_time`, `latitude`, `longitude`)
2. Loads forbidden turn restrictions (`forbidden_turn_from_arc`, `forbidden_turn_to_arc`)
3. Validates input consistency (array lengths, bounds, sort order)
4. Calls `rust_road_router::datastr::graph::line_graph()` with a turn-cost callback that:
   - Rejects forbidden turns (from the sorted restriction arrays)
   - Rejects U-turns (where `tail[edge1] == head[edge2]`)
   - Allows all other turns with zero penalty
5. Maps coordinates to the expanded graph (each line graph node gets the coordinates of its source node)
6. Writes the expanded graph to the output directory

### Usage

```bash
# Build
cargo build --release -p hanoi-tools --bin generate_line_graph

# Run (output defaults to <graph_dir>/line_graph/)
cargo run --release -p hanoi-tools --bin generate_line_graph -- <graph_dir>

# Run with explicit output directory
cargo run --release -p hanoi-tools --bin generate_line_graph -- <graph_dir> <output_dir>
```

### Input files (from `<graph_dir>/`)

| File | Type | Required |
|---|---|---|
| `first_out` | `Vec<u32>` | Yes |
| `head` | `Vec<u32>` | Yes |
| `travel_time` | `Vec<u32>` | Yes |
| `latitude` | `Vec<f32>` | Yes |
| `longitude` | `Vec<f32>` | Yes |
| `forbidden_turn_from_arc` | `Vec<u32>` | Yes |
| `forbidden_turn_to_arc` | `Vec<u32>` | Yes |

### Output files (to `<graph_dir>/line_graph/` or `<output_dir>/`)

| File | Type | Description |
|---|---|---|
| `first_out` | `Vec<u32>` | Line graph CSR offsets |
| `head` | `Vec<u32>` | Line graph edge targets |
| `travel_time` | `Vec<u32>` | Line graph edge weights (milliseconds) |
| `latitude` | `Vec<f32>` | Coordinates per line graph node (= source node of original edge) |
| `longitude` | `Vec<f32>` | Coordinates per line graph node (= source node of original edge) |

## Adding New Tools

To add a new tool, create a new `.rs` file with `fn main()` under `crates/hanoi-tools/src/bin/`. Cargo auto-discovers it — no `Cargo.toml` changes needed.

```bash
# Build all tools at once
cargo build --release -p hanoi-tools

# Build/run a specific tool
cargo build --release -p hanoi-tools --bin <tool_name>
cargo run --release -p hanoi-tools --bin <tool_name> -- <args>
```

Each tool should be self-contained: own argument parsing, own validation, own I/O. Add dependencies to `hanoi-tools/Cargo.toml` only as individual tools require them.

## Role in the Pipeline

CCH-Hanoi sits in the middle of the OSM pipeline, between graph extraction and CCH ordering:

```
OSM PBF
  → CCH-Generator (cch_generator --profile car|motorcycle)
    → base graph files (first_out, head, travel_time, forbidden_turn_*, ...)
      → ★ CCH-Hanoi (generate_line_graph) ★
        → line_graph/ subdirectory (turn-expanded graph)
          → InertialFlowCutter (flow_cutter_cch_order.sh → cch_perm)
          → InertialFlowCutter (flow_cutter_cch_cut_order.sh → cch_perm_cuts)
            → rust_road_router server
```

Pipeline scripts invoke it as:
```bash
# In scripts/pipeline and CCH-Generator/scripts/run_pipeline:
cargo run --release -p hanoi-tools --bin generate_line_graph -- "$GRAPH_DIR"
```

## Build Commands

```bash
# Build all crates in the workspace
cd CCH-Hanoi && cargo build --release --workspace

# Build just the tools
cargo build --release -p hanoi-tools

# Build just generate_line_graph
cargo build --release -p hanoi-tools --bin generate_line_graph

# Build the CLI
cargo build --release -p hanoi-cli

# Run all tests
cargo test --workspace
```

## Future Roadmap

The three-crate model is the final structure. Future work adds implementation to existing crates, not new crates:

1. **`hanoi-core`** — implement CCH wrapping: graph loading, customization orchestration, query orchestration, API exposure.
2. **`hanoi-cli`** — add subcommands as core gains APIs (e.g. `cch-hanoi customize`, `cch-hanoi query`).
3. **`hanoi-tools`** — add new independent utilities as pipeline needs arise (each as a new `src/bin/*.rs` file).
