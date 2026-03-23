# CCH-Hanoi Hub Rework

## Summary

Promote `CCH-Hanoi` into the Hanoi-specific Rust integration hub around `rust_road_router`, structured as a three-crate workspace.

The architecture has three concerns:
- **`hanoi-core`** — the primary CCH implementation library. Will contain Hanoi-specific logic wrapping `rust_road_router` algorithms (graph loading, customization, query orchestration) and expose stable Rust APIs for programmatic use. Starts as an empty stub.
- **`hanoi-cli`** — thin CLI wrapper over `hanoi-core`. Exposes core APIs as operator-facing commands. Starts as a skeleton.
- **`hanoi-tools`** — independent, self-contained utility binaries. Each tool is its own entity under `src/bin/`, depends on `rust_road_router` directly, and optionally on `hanoi-core` if needed. Tools are operational pipeline utilities, not wrappers over core.

This plan covers workspace structure, config files, and directory layout only. Concrete API design, function signatures, and code implementation are deferred to future sprints.

## Implementation Changes

### 1. Restructure `CCH-Hanoi` as a workspace hub

Convert `CCH-Hanoi/` into a Cargo workspace with exactly three crates:
- `crates/hanoi-core` — library crate (empty stub)
- `crates/hanoi-cli` — binary crate (skeleton)
- `crates/hanoi-tools` — binary crate (contains `generate_line_graph`)

No placeholder crates. Future crates are added when they have implementation, not before.

All sub-crates use **edition 2024**. The `rust_road_router/engine` dependency uses edition 2021 internally, but Cargo handles cross-edition dependencies correctly — no compatibility issue.

Keep `rust_road_router` as a sibling dependency, not merged into the `CCH-Hanoi` workspace. Sub-crates that need engine types declare their own path dependency.

The existing `CCH-Hanoi/rust-toolchain.toml` (pinning `channel = "nightly"`) must remain at the workspace root. Required because `rust_road_router/engine` uses `#![feature(impl_trait_in_assoc_type)]`.

#### Dependency direction

```
rust_road_router/engine    (upstream — generic algorithms, types, I/O)
        ↑
   hanoi-core              (future: Hanoi-specific CCH implementation, API surface)
        ↑
   hanoi-cli               (CLI skin over core)

   hanoi-tools             (independent utilities)
        ↑
   rust_road_router/engine (direct dependency)
   hanoi-core              (optional, per-tool, only if needed)
```

Tools do NOT depend on core by default. Each tool declares only the dependencies it actually uses. Core never depends on tools at the Cargo level.

#### Config files to create

Workspace root (`CCH-Hanoi/`):
- `Cargo.toml` — workspace manifest (replaces current single-crate manifest)
  ```toml
  [workspace]
  members = ["crates/*"]
  resolver = "2"
  ```
- `rust-toolchain.toml` — already exists, keep as-is (`channel = "nightly"`)

Per-crate `Cargo.toml` files:
- `crates/hanoi-core/Cargo.toml` — library crate
  - `edition = "2024"`, `name = "hanoi-core"`
  - depends on `rust_road_router = { path = "../../../rust_road_router/engine" }`
- `crates/hanoi-cli/Cargo.toml` — binary crate
  - `edition = "2024"`, `name = "hanoi-cli"`
  - depends on `hanoi-core = { path = "../hanoi-core" }`
  - declares `[[bin]] name = "cch-hanoi"`
- `crates/hanoi-tools/Cargo.toml` — binary crate
  - `edition = "2024"`, `name = "hanoi-tools"`
  - depends on `rust_road_router = { path = "../../../rust_road_router/engine" }`
  - declares `[[bin]] name = "generate_line_graph"`
  - additional `[[bin]]` entries auto-discovered from `src/bin/` as tools are added

Per-crate source stubs:
- `crates/hanoi-core/src/lib.rs` — empty stub (modules added in future implementation sprints)
- `crates/hanoi-cli/src/main.rs` — minimal main (no subcommands until core has APIs)
- `crates/hanoi-tools/src/bin/generate_line_graph.rs` — moved from current `CCH-Hanoi/src/generate_line_graph.rs`, self-contained, depends on `rust_road_router` directly

#### Directory layout after rework

```
CCH-Hanoi/
├── Cargo.toml                          (workspace root)
├── rust-toolchain.toml                 (nightly, existing)
└── crates/
    ├── hanoi-core/
    │   ├── Cargo.toml
    │   └── src/
    │       └── lib.rs                  (empty stub)
    ├── hanoi-cli/
    │   ├── Cargo.toml
    │   └── src/
    │       └── main.rs                 (skeleton — no subcommands yet)
    └── hanoi-tools/
        ├── Cargo.toml
        └── src/
            └── bin/
                └── generate_line_graph.rs  (self-contained, moved from src/)
```

Bootstrap expectation for the initial rework:
- the workspace should be created with all config files and stub sources listed above
- `cargo build --workspace` must succeed from the workspace root
- stubs compile cleanly but expose no production behavior

### 2. Move `generate_line_graph` into `hanoi-tools`

The current `CCH-Hanoi/src/generate_line_graph.rs` moves to `crates/hanoi-tools/src/bin/generate_line_graph.rs` with its logic unchanged. It is a self-contained tool that depends on `rust_road_router/engine` directly — it does NOT depend on `hanoi-core`.

The line-graph generation logic belongs solely at the tools level. If `hanoi-core` ever needs line-graph functionality in the future, it would either invoke the tool binary as a subprocess or the logic would be refactored at that time. This plan does not prescribe that path.

Behavioral contract (unchanged from the current binary):
- input files stay the same (`first_out`, `head`, `travel_time`, `latitude`, `longitude`, `forbidden_turn_from_arc`, `forbidden_turn_to_arc`)
- default output dir remains `<graph_dir>/line_graph`
- output files remain `first_out`, `head`, `travel_time`, `latitude`, `longitude`

No conditional-turn semantics or new graph behavior are added in this rework.

### 3. CLI as future wrapper over core

`hanoi-cli` produces the `cch-hanoi` binary. It is a skeleton until `hanoi-core` has APIs to expose.

Once `hanoi-core` implements graph loading, customization, or query APIs, the CLI will expose them as subcommands (e.g. `cch-hanoi customize <graph_dir>`). Until then, the CLI prints a "no commands available" message.

Pipeline scripts (`scripts/pipeline` and `CCH-Generator/scripts/run_pipeline`) invoke standalone tools directly (e.g. `cargo run --release -p hanoi-tools --bin generate_line_graph -- <graph_dir>`). The CLI is for operator/interactive use only.

### 4. Define the future hub boundary now

Document `CCH-Hanoi` as the Hanoi-specific integration/orchestration layer for:
- CCH implementation wrapping `rust_road_router` algorithms
- graph loading orchestration
- customization setup
- query orchestration
- Hanoi-specific dataset/config conventions
- API exposure for programmatic use by downstream Rust code

Document `rust_road_router` as remaining the upstream algorithm/toolkit workspace:
- core graph types
- CCH/customization/query engines
- generic research crates

Rule for future work:
- reusable algorithm/data-structure improvements belong upstream in `rust_road_router`
- Hanoi-specific orchestration, conventions, and utility APIs belong in `CCH-Hanoi`

### 5. Documentation and changelog alignment

Update active docs to reflect:
- `CCH-Hanoi` is now the Hanoi Rust hub with a three-crate workspace
- `hanoi-core` is the future API surface (currently empty)
- `hanoi-tools` contains independent, self-contained utilities
- CLI is a future wrapper over core, not over tools

Add a new entry in `docs/CHANGELOGS.md` describing the rework and migration.

## Public Interfaces

Standalone utility interface:
- tool: `generate_line_graph <graph_dir> [<output_dir>]`

Independent build requirement:
- utilities must remain buildable separately (e.g. `cargo build --release -p hanoi-tools --bin generate_line_graph`)

Not included yet:
- no CLI subcommands (core has no APIs yet)
- no config file schema
- no graph-load/customize/query implementation
- no new `rust_road_router` public API commitments
- Rust API signatures deferred to the code implementation phase
- the long-term goal for `hanoi-core` is to expose stable Rust APIs for programmatic use (graph loading, customization, query). Building for API consumption is a first-class objective in future sprints.

## Test Plan

1. Workspace build:
   - `cargo build --release --manifest-path CCH-Hanoi/Cargo.toml --workspace` compiles all crates without errors or warnings

2. Independent tool build:
   - `cargo build --release -p hanoi-tools --bin generate_line_graph` succeeds in isolation
   - confirm the tool binary is produced at `CCH-Hanoi/target/release/generate_line_graph`

3. CLI build:
   - `cargo build --release -p hanoi-cli` produces the `cch-hanoi` binary

4. Workspace tests:
   - `cargo test --manifest-path CCH-Hanoi/Cargo.toml --workspace` passes

5. Pipeline smoke test:
   - run the migrated line-graph stage through the standalone tool path
   - validate with `validate_graph <graph_dir> --turn-expanded <graph_dir>/line_graph`

## Assumptions

- `CCH-Hanoi` is the Hanoi-specific Rust integration hub.
- Exactly three crates: `hanoi-core`, `hanoi-cli`, `hanoi-tools`. No placeholders.
- Tools are independent, self-contained entities. They do not route through core.
- Core is reserved for the primary CCH implementation and future API exposure.
- Standalone utility binaries are first-class outputs, not wrappers over core.
- This plan covers workspace structure, config files, and directory layout only — code implementation is a separate phase.
- The long-term goal for `hanoi-core` is to expose stable Rust APIs for programmatic use (graph loading, customization, query). Building for API consumption is a first-class objective in future sprints.
