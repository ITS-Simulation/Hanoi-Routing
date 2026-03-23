# Repository Guidelines

## Project Structure & Module Organization
- `RoutingKit/`: C++ routing library (CH/CCH, OSM loaders, `bin/test_*` executables).
- `CCH-Generator/`: C++17 tools for generating and validating RoutingKit-format graphs (`build/cch_generator`, `build/validate_graph`).
- `CCH-Hanoi/`: Rust workspace hub (`hanoi-core`, `hanoi-cli`, `hanoi-tools`) for Hanoi-specific integration, CLI exposure, and turn-expanded graph tooling.
- `rust_road_router/`: Rust workspace (engine, server, conversion, research crates).
- `Maps/`: source `.osm.pbf` inputs and generated graph vectors under `Maps/data/*`.
- `scripts/`: helper scripts (`pipeline_test`, `graph_binary_viewer`).
- `docs/`: design notes and `CHANGELOGS.md` (update this for every code/doc change).

## Build, Test, and Development Commands
- Build CCH generator:
  - `cmake -S CCH-Generator -B CCH-Generator/build -DCMAKE_BUILD_TYPE=Release`
  - `cmake --build CCH-Generator/build -j"$(nproc)"`
- Build RoutingKit:
  - `cd RoutingKit && ./generate_make_file && make -j"$(nproc)"`
- Build line-graph generator:
  - `cargo build --release --manifest-path CCH-Hanoi/Cargo.toml -p hanoi-tools --bin generate_line_graph`
- Build CCH-Hanoi workspace crates:
  - `cargo build --release --manifest-path CCH-Hanoi/Cargo.toml --workspace`
- Build Rust workspace:
  - `cargo build --release --manifest-path rust_road_router/Cargo.toml`
- Run end-to-end graph pipeline:
  - `CCH-Generator/scripts/run_pipeline Maps/hanoi.osm.pbf`

## Coding Style & Naming Conventions
- C++: C++17, existing files use tab-indented blocks, braces on same line, `snake_case` for functions/variables, `CamelCase` for types.
- Rust: follow `rustfmt` defaults (`rust_road_router/rustfmt.toml` sets `max_width = 160`); run `cargo fmt --all` in `rust_road_router`.
- Scripts: Bash with `set -euo pipefail`; Python/Rust helpers use descriptive `snake_case` file names.
- Keep generated binaries and temporary outputs out of source directories; write derived data to `Maps/data/`.

## Time Unit Conventions
- Persisted `travel_time` values in RoutingKit-format graph vectors are **milliseconds** (`u32`).
- Canonical OSM conversion used in this repo (`geo_distance[m] * 18000 / speed[km/h] / 5`) yields milliseconds.
- In `rust_road_router`, integer TD routing (`time_dependent` module) also uses millisecond timestamps/weights (for example, one-day period is `86_400_000`).
- `tt_units_per_s` is the authoritative metadata for integer datasets; current project pipelines/importers use `tt_units_per_s = 1000` (ms).
- `floating_time_dependent` / CATCHUp code uses **seconds internally** (`f64`) and performs explicit conversion from/to millisecond integer inputs.
- Some legacy comments/messages still mention seconds in RoutingKit; treat executable formulas and persisted data contracts as source of truth.

## Testing Guidelines
- Run Rust tests: `cargo test --workspace --manifest-path rust_road_router/Cargo.toml`.
- Run CCH-Hanoi tests/checks: `cargo test --workspace --manifest-path CCH-Hanoi/Cargo.toml`.
- Validate generated graphs:
  - `CCH-Generator/build/validate_graph <graph_dir>`
  - `CCH-Generator/build/validate_graph <graph_dir> --turn-expanded <graph_dir>/line_graph`
- RoutingKit regression checks are executable tests in `RoutingKit/bin/test_*` (run relevant binaries for touched components).
- No fixed coverage threshold is enforced; include at least one regression path for behavior changes.

## Commit & Pull Request Guidelines
- This root folder currently has no usable git history; follow the established changelog pattern in `docs/CHANGELOGS.md` (`YYYY-MM-DD — short title` + concise change bullets).
- Use imperative, scoped commit subjects (example: `CCH-Generator: validate conditional turn bounds`).
- PRs should include:
  - problem statement and scope,
  - affected paths/modules,
  - commands run for build/test/validation,
  - sample output or metrics when pipeline/data behavior changes.

## Planning Guidelines
- For Plan mode, all plans must be generated to `docs/planned/` folder
