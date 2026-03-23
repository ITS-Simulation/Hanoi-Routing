# CCH-Advanced-Generator: Line Graph Generator Plan

> **Goal**: A Rust project that reads a pre-generated base graph (from CCH-Generator) and produces a turn-expanded line graph in RoutingKit binary format, reusing `rust_road_router::engine::line_graph()` directly. The output is fed to InertialFlowCutter for node ordering.

---

## Pipeline Context

```
CCH-Generator (C++)           CCH-Advanced-Generator (Rust)          InertialFlowCutter (C++)
┌───────────────────┐         ┌──────────────────────────┐          ┌────────────────────────┐
│  OSM PBF          │         │  Base graph dir          │          │  Line graph dir        │
│  → binary graph   │────────►│  → line graph            │─────────►│  → cch_perm            │
│  (first_out,head, │         │  (engine::line_graph())  │          │  (node ordering)       │
│   travel_time,    │         │                          │          │                        │
│   lat,lng,        │         │  Output: line_graph/     │          │                        │
│   forbidden_*)    │         │    first_out,head,       │          │                        │
│                   │         │    travel_time,lat,lng   │          │                        │
└───────────────────┘         └──────────────────────────┘          └────────────────────────┘
     existing                      this project                          existing
```

Each tool does one thing:
- **CCH-Generator** (C++): OSM loading via RoutingKit — already done, no changes needed
- **CCH-Advanced-Generator** (Rust): Line graph construction — reuses engine directly
- **InertialFlowCutter** (C++): Node ordering — already done, no changes needed

---

## Part 0: Project Setup

> **Owner**: Human + AI

### 0.1 Cargo.toml

```toml
[package]
name = "cch-advanced-generator"
version = "0.1.0"
edition = "2021"

[dependencies]
rust_road_router = { path = "../rust_road_router/engine" }

[[bin]]
name = "generate_line_graph"
path = "src/generate_line_graph.rs"
```

No `build.rs`, no `cc` crate, no FFI — just a path dependency on the engine.

### 0.2 Rust toolchain

The engine crate uses nightly features (`array_windows`, `slice_group_by`, etc.):

```toml
# rust-toolchain.toml
[toolchain]
channel = "nightly"
```

### 0.3 Project structure

```
CCH-Advanced-Generator/
├── Cargo.toml
├── rust-toolchain.toml
├── src/
│   └── generate_line_graph.rs
└── .gitignore
```

Minimal. One binary, one source file.

---

## Part 1: Line Graph Generator (`generate_line_graph.rs`)

> **Owner**: Human + AI

### 1.1 Command-line interface

```
Usage: generate_line_graph <graph_dir> [<output_dir>]

  graph_dir   Path to base graph directory (must contain: first_out, head,
              travel_time, latitude, longitude, forbidden_turn_from_arc,
              forbidden_turn_to_arc)
  output_dir  Optional; defaults to <graph_dir>/line_graph/
```

### 1.2 Implementation

This is a cleaned-up version of [turn_expand_osm.rs](rust_road_router/cchpp/src/bin/turn_expand_osm.rs) with configurable output path and statistics:

```rust
#![feature(array_windows)]

use rust_road_router::{
    cli::CliErr,
    datastr::graph::*,
    io::*,
};
use std::{env, error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let graph_dir = args.next().ok_or(CliErr("No graph directory given"))?;
    let graph_path = Path::new(&graph_dir);

    let output_dir = match args.next() {
        Some(s) => std::path::PathBuf::from(s),
        None => graph_path.join("line_graph"),
    };
    fs::create_dir_all(&output_dir)?;

    // 1. Load original graph
    let graph = WeightedGraphReconstructor("travel_time").reconstruct_from(&graph_path)?;
    let lat = Vec::<f32>::load_from(graph_path.join("latitude"))?;
    let lng = Vec::<f32>::load_from(graph_path.join("longitude"))?;

    let forbidden_from = Vec::<EdgeId>::load_from(graph_path.join("forbidden_turn_from_arc"))?;
    let forbidden_to = Vec::<EdgeId>::load_from(graph_path.join("forbidden_turn_to_arc"))?;

    eprintln!("Original graph: {} nodes, {} arcs, {} forbidden turns",
        graph.num_nodes(), graph.num_arcs(), forbidden_from.len());

    // 2. Build tail array (needed for U-turn detection + coordinate mapping)
    let mut tail = Vec::with_capacity(graph.num_arcs());
    for node in 0..graph.num_nodes() {
        for _ in 0..graph.degree(node as NodeId) {
            tail.push(node as NodeId);
        }
    }

    // 3. Build line graph with merge-scan forbidden turn filter
    let mut iter = forbidden_from.iter().zip(forbidden_to.iter()).peekable();

    let exp_graph = line_graph(&graph, |edge1_idx, edge2_idx| {
        // Advance past entries lexicographically before (edge1, edge2)
        while let Some((&from_arc, &to_arc)) = iter.peek() {
            if from_arc < edge1_idx || (from_arc == edge1_idx && to_arc < edge2_idx) {
                iter.next();
            } else {
                break;
            }
        }
        // Forbidden turn check
        if iter.peek() == Some(&(&edge1_idx, &edge2_idx)) {
            return None;
        }
        // U-turn check: tail of edge1 == head of edge2
        if tail[edge1_idx as usize] == graph.head()[edge2_idx as usize] {
            return None;
        }
        Some(0) // allowed turn, 0ms penalty
    });

    eprintln!("Line graph: {} nodes, {} arcs (avg degree: {:.2})",
        exp_graph.num_nodes(), exp_graph.num_arcs(),
        exp_graph.num_arcs() as f64 / exp_graph.num_nodes().max(1) as f64);

    // 4. Map coordinates: line graph node i → tail of original edge i
    let new_lat: Vec<_> = (0..exp_graph.num_nodes())
        .map(|idx| lat[tail[idx] as usize]).collect();
    let new_lng: Vec<_> = (0..exp_graph.num_nodes())
        .map(|idx| lng[tail[idx] as usize]).collect();

    // 5. Save
    exp_graph.first_out().write_to(&output_dir.join("first_out"))?;
    exp_graph.head().write_to(&output_dir.join("head"))?;
    exp_graph.weight().write_to(&output_dir.join("travel_time"))?;
    new_lat.write_to(&output_dir.join("latitude"))?;
    new_lng.write_to(&output_dir.join("longitude"))?;

    eprintln!("Output: {}", output_dir.display());
    Ok(())
}
```

### 1.3 Output files

| File | Type | Entries | Description |
|------|------|---------|-------------|
| `first_out` | `Vec<u32>` | m + 1 | CSR offsets (m = original arc count = line graph node count) |
| `head` | `Vec<u32>` | t | CSR targets (t = allowed turn count = line graph arc count) |
| `travel_time` | `Vec<u32>` | t | Edge weight = `original_travel_time[edge1] + 0` |
| `latitude` | `Vec<f32>` | m | `lat[tail[edge_id]]` per line graph node |
| `longitude` | `Vec<f32>` | m | `lng[tail[edge_id]]` per line graph node |

### 1.4 Why this produces byte-identical output to `turn_expand_osm.rs`

- Same `line_graph()` function (from engine crate)
- Same merge-scan iteration pattern for forbidden turns
- Same U-turn check: `tail[e1] == head[e2]`
- Same `Some(0)` turn cost
- Same coordinate mapping: `lat[tail[i]]`
- Same binary I/O: engine's `write_to`

---

## Part 2: Validation (Cross-Check)

> **Owner**: Human (running) + AI (reviewing)

### 2.1 Cross-validate against Rust `turn_expand_osm`

```bash
# Generate with existing Rust binary
cd rust_road_router
cargo run --release -p cchpp --bin turn_expand_osm -- \
    ../Maps/data/hanoi_car /tmp/line_graph_rust

# Generate with CCH-Advanced-Generator
cd ../CCH-Advanced-Generator
cargo run --release -- ../Maps/data/hanoi_car /tmp/line_graph_new

# Binary diff — should be identical
diff <(xxd /tmp/line_graph_rust/first_out)   <(xxd /tmp/line_graph_new/first_out)
diff <(xxd /tmp/line_graph_rust/head)         <(xxd /tmp/line_graph_new/head)
diff <(xxd /tmp/line_graph_rust/travel_time)  <(xxd /tmp/line_graph_new/travel_time)
diff <(xxd /tmp/line_graph_rust/latitude)     <(xxd /tmp/line_graph_new/latitude)
diff <(xxd /tmp/line_graph_rust/longitude)    <(xxd /tmp/line_graph_new/longitude)
```

### 2.2 Validate with C++ tool

```bash
cd CCH-Generator/build
./validate_graph ../../Maps/data/hanoi_car \
    --turn-expanded ../../Maps/data/hanoi_car/line_graph
```

### 2.3 Run InertialFlowCutter on the line graph

```bash
cd rust_road_router
./flow_cutter_cch_order.sh ../Maps/data/hanoi_car/line_graph
# Produces: line_graph/cch_perm (node ordering for the line graph)
```

Then validate the permutation:

```bash
ARC_COUNT=$(( $(stat -c%s ../Maps/data/hanoi_car/head) / 4 ))
cd ../CCH-Generator/build
./validate_graph ../../Maps/data/hanoi_car \
    --check-perm ../../Maps/data/hanoi_car/line_graph/cch_perm "${ARC_COUNT}"
```

---

## Part 3: Pipeline Integration

> **Owner**: Human + AI

### 3.1 Updated `run_pipeline` steps

Insert line graph generation after base graph validation:

```
 [1/12]  Generate car graph                      (cch_generator — C++)
 [2/12]  Validate car graph                      (validate_graph — C++)
 [3/12]  Extract conditional turns (car)         (conditional_turn_extract — C++)
 [4/12]  Validate with conditionals              (validate_graph — C++)
 [5/12]  Generate car line graph                 (generate_line_graph — Rust)     ← NEW
 [6/12]  Validate car line graph                 (validate_graph --turn-expanded) ← NEW
 [7/12]  Generate motorcycle graph               (cch_generator — C++)
 [8/12]  Validate motorcycle graph               (validate_graph — C++)
 [9/12]  Extract conditional turns (motorcycle)  (conditional_turn_extract — C++)
[10/12]  Validate with conditionals              (validate_graph — C++)
[11/12]  Generate motorcycle line graph          (generate_line_graph — Rust)     ← NEW
[12/12]  Validate motorcycle line graph          (validate_graph --turn-expanded) ← NEW
```

IFC permutation generation stays as a separate step (run manually or in a follow-up script).

### 3.2 Output directory structure

```
Maps/data/hanoi_car/
├── first_out, head, travel_time, ...   ← base graph (CCH-Generator)
├── forbidden_turn_from_arc/to_arc
├── cch_perm                            ← IFC node order (base graph)
├── cch_exp_perm                        ← IFC arc order (base graph)
└── line_graph/                         ← NEW (CCH-Advanced-Generator)
    ├── first_out
    ├── head
    ├── travel_time
    ├── latitude
    ├── longitude
    └── cch_perm                        ← IFC node order (line graph)
```

---

## Expected Dimensions (Hanoi Estimate)

| Metric | Estimate |
|--------|----------|
| Original nodes | ~200K–400K |
| Original arcs (= line graph nodes) | ~500K–1M |
| Forbidden turns | ~5K–20K |
| U-turns skipped | ~400K–800K |
| Line graph arcs (= allowed turns) | ~800K–2M |
| Average out-degree | ~1.5–2.5 |

---

## Summary

| What | Tool | Status |
|------|------|--------|
| OSM → binary graph | CCH-Generator (C++) | **Done** |
| Binary graph → line graph | CCH-Advanced-Generator (Rust) | **This plan** |
| Node ordering | InertialFlowCutter (C++) | **Done** |
| Validation | CCH-Generator `validate_graph` (C++) | **Done** |
