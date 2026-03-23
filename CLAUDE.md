# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with
code in this repository.

## Project Overview

Multi-language routing framework for road networks, implementing Customizable
Contraction Hierarchies (CCH) and time-dependent routing algorithms. Primary
focus on fast shortest-path queries using HERE map data for the Hanoi road
network.

## Components

### rust_road_router (Rust workspace — main component)

Cargo workspace with 9 crates:

- **engine**: Core library — graph data structures, routing algorithms
(Dijkstra, CH, CCH, TD-CCH, CATCHUp, A/ALT), I/O for RoutingKit binary
format
- **server**: Rocket HTTP API (`/query`, `/here_query`, `/customize`) with
KD-tree spatial indexing. Requires **nightly Rust**.
- **conversion**: Data import/export tools (HERE CSV → RoutingKit binary,
DIMACS, Mapbox)
- **catchup, cchpp, chpot, tdpot**: Research algorithm variants (time-dependent
routing, CH potentials)
- **utils, visualization**: Helpers and debug visualization

### RoutingKit (C++ library)

Industry-standard routing library from KIT. Provides CH, CCH, OSM import (PBF),
and spatial indexing. Built via Makefile.

### InertialFlowCutter (C++ CMake, at `rust_road_router/lib/InertialFlowCutter`)

Computes nested dissection orderings for CCH preprocessing. Contains KaHIP and
RoutingKit as git submodules.

### CCH-Generator (C++ CMake)

Graph generation and structural validation from OSM PBF. Produces
RoutingKit-format binary files for car/motorcycle profiles. Binaries built to
`CCH-Generator/lib/`. Includes `validate_graph` for comprehensive graph
integrity checks.

### CCH-Hanoi (Rust workspace hub, edition 2024, nightly)

Hanoi-specific integration workspace with:

- `hanoi-core`: reusable library API surface (currently includes line-graph
generation logic)
- `hanoi-tools`: standalone utility binaries (currently `generate_line_graph`)
- `hanoi-cli`: operator-facing CLI wrapper over `hanoi-core`

### Live_Network_Routing (Kotlin/Gradle)

Stub/placeholder project.

## Build Commands

```bash
# Rust (from rust_road_router/)
cargo build --release                          # Build all workspace crates
cargo build --release -p server                # Build just the server
cargo build --release -p rust_road_router --bin import_here  # Single binary
cargo test                                     # Run all tests

# RoutingKit (from RoutingKit/)
./generate_make_file && make                   # Regenerate Makefile, build all

# CCH-Generator (from CCH-Generator/)
mkdir -p build && cd build && cmake .. && make # Builds cch_generator + validate_graph

# CCH-Hanoi (from CCH-Hanoi/)
cargo build --release --workspace                                  # Build all CCH-Hanoi crates
cargo build --release -p hanoi-tools --bin generate_line_graph     # Build standalone line-graph tool

# InertialFlowCutter (from rust_road_router/lib/InertialFlowCutter/)
mkdir -p build && cd build && cmake -DCMAKE_BUILD_TYPE=Release -DGIT_SUBMODULE=OFF -DUSE_KAHIP=OFF .. && make console

# Live_Network_Routing (from Live_Network_Routing/)
./gradlew build
./gradlew shadowJar                            # Fat JAR: live-routing.jar
```

## Data Pipelines

### OSM PBF pipeline (primary, car/motorcycle profiles)

```
OSM PBF file
  → CCH-Generator/lib/cch_generator <pbf> <output_dir> --profile car|motorcycle
    → RoutingKit binary format (first_out, head, travel_time, way, latitude, longitude, forbidden_turn_*)
      → RoutingKit/bin/conditional_turn_extract <pbf> <graph_dir> --profile car|motorcycle
        → conditional_turn_from_arc, conditional_turn_to_arc, conditional_turn_time_windows
          → CCH-Hanoi generate_line_graph <graph_dir>
            → line_graph/ subdirectory (turn-expanded graph)
              → flow_cutter_cch_order.sh <graph_dir> → cch_perm
              → flow_cutter_cch_cut_order.sh <graph_dir> → cch_perm_cuts (arc ordering for line graph)
```

Interactive wrapper: `scripts/pipeline <map_source> <profile>` Automated
wrapper: `CCH-Generator/scripts/run_pipeline <input.osm.pbf>`

### HERE CSV pipeline (legacy)

```
HERE CSV files
  → cargo run --release -p conversion --bin import_here -- <input> <output>
    → RoutingKit binary format → flow_cutter_cch_order.sh → cch_perm → server
```

The `cch_complete.sh` script and `Dockerfile` automate the HERE pipeline.

## Data Format

RoutingKit binary format: raw headerless binary vectors. Graph stored as CSR
(Compressed Sparse Row):

- `first_out` (u32): node → edge offset
- `head` (u32): edge → target node
- `travel_time` (u32): edge weights in milliseconds
- `latitude`/`longitude` (f32): node coordinates
- `way` (u32): edge → routing way ID (produced by RoutingKit's OSM pipeline,
needed by conditional turn resolver)
- `forbidden_turn_from_arc`/`forbidden_turn_to_arc` (u32): turn restrictions
(sorted)
- `conditional_turn_from_arc`/`conditional_turn_to_arc` (u32): conditional turn
restrictions (sorted, from Phase 1)
- `conditional_turn_time_windows`: packed offset + TimeWindow data for
conditional turns
- `cch_perm` (u32): node permutation for CCH

### Time Unit Conventions

- Persisted RoutingKit-format `travel_time` vectors in this repository are in
**milliseconds**.
- Canonical OSM conversion (`geo_distance[m] * 18000 / speed[km/h] / 5`)
produces milliseconds.
- In `rust_road_router` integer TD (`datastr::graph::time_dependent`),
timestamps and weights are millisecond-scale (`Timestamp = Weight`, period
`86_400_000`).
- `tt_units_per_s` is the dataset authority for integer graphs; repo
pipelines/importers currently write `tt_units_per_s = 1000`.
- `floating_time_dependent` / CATCHUp uses seconds (`f64`) internally and
explicitly converts from/to millisecond integer graph data.
- If comments conflict (some legacy RoutingKit text still says seconds), trust
code formulas plus dataset metadata (`tt_units_per_s`).

## Architecture Notes

- The Rust `engine` crate uses generic `Ops` traits for pluggable algorithm
behavior (e.g., different weight types, objectives)
- Cargo features control TD-CCH variants, reporting, and parallelization (rayon)
- Server uses a background thread for weight customization while serving queries
- RoutingKit's OSM loading is two-pass: ID discovery → graph construction
- `rustfmt.toml` sets `max_width = 160`

## Key Design Docs

- `docs/OSM Loading.md` — RoutingKit's OSM loading pipeline architecture
- `docs/Conditional Turns Implementation.md` — Time-based turn restriction
design
- `docs/Manual Pipeline Guide.md` — Step-by-step pipeline walkthrough (PBF →
graph → conditionals → line graph → IFC ordering)
- `docs/CCH Walkthrough.md` — CCH phases: Contraction, Customization, Query
- `rust_road_router/engine/README.md` — Engine module organization and algorithm
inventory
- `rust_road_router/server/README.md` — Server API documentation

## Notes

- All docs are meant to be created in `docs/` folder
  - Implementation plans should be generated to `docs/planned/` folder
  - Walkthroughs should be generated to `docs/walkthrough/` folder
- All file changes are logged in `docs/CHANGELOGS.md`
- After implementation, no need to build anything. I have a seperate audit
workflow
- For Plan mode: Every plan must be generated to `docs/planned/` folder
- RoutingKit and rust_road_router is strictly forbidden to be touched. Under no circumstances should any plan consider touching these 2 repos

