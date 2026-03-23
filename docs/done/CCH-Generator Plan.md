# CCH-Generator: Phase 1 Testing Plan

> **Goal**: Turn `CCH-Generator/` into a C++ project that links against RoutingKit as a local library, loads OSM `.pbf` maps, produces RoutingKit binary graph files, and validates the output. This becomes the canonical graph generation tool for the project.

---

## Architecture Overview

```
Maps/hanoi.osm.pbf ─────┐
                         ▼
              ┌──────────────────────┐
              │    CCH-Generator     │
              │  (C++ / RoutingKit)  │
              │                      │
              │  1. Load OSM PBF     │
              │  2. Save binary      │
              │  3. Validate graph   │
              │  4. Print statistics │
              └──────────┬───────────┘
                         ▼
              output_dir/
              ├── first_out
              ├── head
              ├── travel_time
              ├── geo_distance
              ├── latitude
              ├── longitude
              ├── forbidden_turn_from_arc
              └── forbidden_turn_to_arc
                         ▼
              InertialFlowCutter
              ├── cch_perm          (standard)
              └── cch_perm_cuts     (→ rename to cch_exp_perm)
```

---

## Part 0: Build RoutingKit as a Local Library

> **Owner**: Human only

RoutingKit is already built (`lib/libroutingkit.a`, `lib/libroutingkit.so` exist). If you need to rebuild:

```bash
cd /home/thomas/VTS/Hanoi-Routing/RoutingKit
./generate_make_file
make -j$(nproc)
```

> **When to re-run `./generate_make_file`**: The script scans `src/` for `.cpp` files and generates the Makefile with the discovered sources. You **must** re-run it whenever you **add or remove** `.cpp` files (e.g., new modules like `conditional_restriction_decoder.cpp`). If you only **modify** existing `.cpp`/`.h` files, `make` alone is sufficient — the Makefile already tracks those dependencies.

This produces:
- `lib/libroutingkit.a` — static library (preferred for CCH-Generator)
- `lib/libroutingkit.so` — shared library
- Headers in `include/routingkit/`

### Verify the build

```bash
# Check that the OSM loader symbols are present:
nm lib/libroutingkit.a | grep simple_load_osm_car
nm lib/libroutingkit.a | grep simple_load_osm_motorcycle
```

Both should show `T` (text/code) symbols. If motorcycle symbols are missing, the osm_profile/osm_simple changes haven't been compiled in — re-run `make`.

---

## Part 1: Project Setup (CMakeLists.txt)

> **Owner**: Human + AI

### 1.1 Update CMakeLists.txt to link RoutingKit

The current `CMakeLists.txt` is a bare stub. It needs to:

1. **Set C++17** (RoutingKit requires it — uses `std::optional`, structured bindings, etc. Also uses `std::is_pod<T>` in `vector_io.h` which is deprecated in C++20)
2. **Add RoutingKit include path** (`../RoutingKit/include`)
3. **Link against `libroutingkit.a`** (`../RoutingKit/lib`)
4. **Link system dependencies**: RoutingKit's OSM loader uses `libz` for PBF decompression (it has its own protobuf parser — no external protobuf dependency)

```cmake
cmake_minimum_required(VERSION 3.16)
project(CCH_Generator)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# RoutingKit paths (relative to this CMakeLists.txt)
set(ROUTINGKIT_DIR "${CMAKE_SOURCE_DIR}/../RoutingKit")

# Main executable
add_executable(cch_generator src/generate_graph.cpp)

target_include_directories(cch_generator PRIVATE ${ROUTINGKIT_DIR}/include)
target_link_directories(cch_generator PRIVATE ${ROUTINGKIT_DIR}/lib)
target_link_libraries(cch_generator routingkit z pthread m)

# Validation test executable
add_executable(validate_graph src/validate_graph.cpp)

target_include_directories(validate_graph PRIVATE ${ROUTINGKIT_DIR}/include)
target_link_directories(validate_graph PRIVATE ${ROUTINGKIT_DIR}/lib)
target_link_libraries(validate_graph routingkit z pthread m)
```

> **Why `-lz -pthread -lm` and NOT `-lprotobuf-lite`?** RoutingKit bundles its own protobuf parser (`src/protobuf.cpp`) and doesn't depend on Google's libprotobuf. The link dependencies match RoutingKit's own Makefile exactly: `-lm -lz -pthread`.

### 1.2 Project structure

```
CCH-Generator/
├── CMakeLists.txt                          ← Build config linking RoutingKit
├── src/
│   ├── generate_graph.cpp                  ← Main graph generator (Part 2)
│   ├── validate_graph.cpp                  ← Structural validation tool (Part 3)
│   └── graph_utils.h                       ← Shared helpers (tail array, stats printing)
├── data/                                   ← Generated graph output (gitignored)
│   ├── hanoi_car/                          ← Car profile output
│   │   ├── first_out
│   │   ├── head
│   │   ├── travel_time
│   │   ├── geo_distance
│   │   ├── latitude
│   │   ├── longitude
│   │   ├── forbidden_turn_from_arc
│   │   ├── forbidden_turn_to_arc
│   │   ├── cch_perm                        ← From InertialFlowCutter (Part 4.2)
│   │   └── cch_exp_perm                    ← From IFC cut order (Part 4.3)
│   └── hanoi_motorcycle/                   ← Motorcycle profile output
│       └── ... (same files)
├── scripts/
│   ├── run_pipeline.sh                     ← End-to-end: generate → validate → IFC
│   └── compare_profiles.sh                 ← Cross-profile diff (car vs motorcycle)
├── build/                                  ← CMake build directory (gitignored)
└── .gitignore
```

### File purposes

| File | Role | Description |
|------|------|-------------|
| `src/generate_graph.cpp` | **Executable** | Calls `simple_load_osm_{car,motorcycle}_routing_graph_from_pbf`, saves all vectors to output dir. One binary, profile selected via `--profile` flag. |
| `src/validate_graph.cpp` | **Executable** | Loads binary vectors, runs 10+ structural checks (CSR validity, coordinate bounds, connectivity, etc.). Returns exit code 0 on pass, 1 on failure. |
| `src/graph_utils.h` | **Header-only** | Shared between both executables. Contains: `build_tail()` (derive source node per arc from `first_out`), `print_graph_stats()` (node/arc/turn counts), and `ensure_directory()` wrapper. |
| `scripts/run_pipeline.sh` | **Shell script** | Chains: `cch_generator` → `validate_graph` → `flow_cutter_cch_order.sh` → `flow_cutter_cch_cut_order.sh` → rename `cch_perm_cuts` → validate permutations. Stops on first failure. |
| `scripts/compare_profiles.sh` | **Shell script** | Generates both car and motorcycle graphs from the same PBF, then reports: node/arc count diff, travel time distribution, forbidden turn count diff. |
| `data/` | **Output** | All generated binary files. Gitignored — these are large binary blobs regenerated from PBF. |
| `.gitignore` | **Config** | Ignores `build/`, `data/`, and `cmake-build-*` (CLion). |

### Why `graph_utils.h` is header-only

Both `generate_graph` and `validate_graph` need `build_tail()` and stats printing. Rather than creating a third `.cpp` → `.o` → link step, a single header with inline functions keeps the build simple. The functions are small (< 20 lines each) and don't warrant a separate compilation unit.

### The `.gitignore`

```gitignore
build/
cmake-build-*/
data/
```

---

## Part 2: Graph Generator (`generate_graph.cpp`)

> **Owner**: Human + AI

This is the main tool that loads an OSM PBF file and produces RoutingKit binary files.

### 2.1 Command-line interface

```
Usage: cch_generator <input.osm.pbf> <output_dir> [--profile car|motorcycle]
```

- `input.osm.pbf` — path to the OSM PBF file (e.g., `../Maps/hanoi.osm.pbf`)
- `output_dir` — directory to write binary files into (created if absent)
- `--profile` — routing profile (default: `car`). Selects which `simple_load_osm_*_routing_graph_from_pbf` to call.

### 2.2 Implementation outline

```cpp
#include <routingkit/osm_simple.h>
#include <routingkit/vector_io.h>

#include <iostream>
#include <string>
#include <filesystem>
#include <cassert>

int main(int argc, char* argv[]) {
    // 1. Parse arguments: pbf_path, output_dir, profile

    // 2. Create output directory if needed
    std::filesystem::create_directories(output_dir);

    // 3. Load graph based on profile
    //    - "car":        simple_load_osm_car_routing_graph_from_pbf(pbf, log_fn)
    //    - "motorcycle": simple_load_osm_motorcycle_routing_graph_from_pbf(pbf, log_fn)
    //    The log_fn callback prints RoutingKit progress messages to stderr.

    // 4. Save all vectors to output_dir
    //    save_vector(output_dir + "/first_out",               graph.first_out);
    //    save_vector(output_dir + "/head",                    graph.head);
    //    save_vector(output_dir + "/travel_time",             graph.travel_time);
    //    save_vector(output_dir + "/geo_distance",            graph.geo_distance);
    //    save_vector(output_dir + "/latitude",                graph.latitude);
    //    save_vector(output_dir + "/longitude",               graph.longitude);
    //    save_vector(output_dir + "/forbidden_turn_from_arc", graph.forbidden_turn_from_arc);
    //    save_vector(output_dir + "/forbidden_turn_to_arc",   graph.forbidden_turn_to_arc);

    // 5. Print summary statistics (node count, arc count, turn restriction count, etc.)
}
```

### 2.3 Log callback

RoutingKit's loader accepts a `std::function<void(const std::string&)>` for progress logging. Wire it to stderr:

```cpp
auto log_fn = [](const std::string& msg) { std::cerr << msg << std::endl; };
```

### 2.4 Available map files

| File | Description | Expected size |
|------|-------------|---------------|
| `Maps/hanoi.osm.pbf` | Hanoi city extract | Small (~50MB) — good for testing |
| `Maps/vietnam-260305.osm.pbf` | Full Vietnam | Large (~500MB+) — production test |

**Start with `hanoi.osm.pbf`** for fast iteration.

---

## Part 3: Graph Validation (`validate_graph.cpp`)

> **Owner**: Human + AI

A standalone tool that reads the binary output and checks structural invariants. This is the core "testing module."

### 3.1 Command-line interface

```
Usage: validate_graph <graph_dir> [--turn-expanded]
```

- Without `--turn-expanded`: validates a standard graph
- With `--turn-expanded`: additionally checks the line graph invariants (requires running InertialFlowCutter first for `cch_exp_perm`, but we validate the graph structure itself, not the permutation)

### 3.2 Standard graph checks

These checks apply to the output of `generate_graph`:

| # | Check | What it validates |
|---|-------|-------------------|
| 1 | **CSR structure** | `first_out` is monotonically non-decreasing, `first_out[0] == 0`, `first_out[last] == head.size()` |
| 2 | **Head bounds** | Every `head[i] < node_count` (no out-of-range target nodes) |
| 3 | **No self-loops** | `head[i] != source(i)` for all edges (source derived from `first_out`) |
| 4 | **Vector length consistency** | `head.size() == travel_time.size() == geo_distance.size()`, `latitude.size() == longitude.size() == node_count` |
| 5 | **Coordinate sanity** | Latitude in `[0, 30]`, longitude in `[100, 115]` (Vietnam bounding box) |
| 6 | **Travel time sanity** | No zero travel times (except possibly ferries), no travel times > 24h (86,400,000 ms) |
| 7 | **Turn restriction sorting** | `forbidden_turn_from_arc` is sorted (RoutingKit asserts this internally, but we verify the saved file) |
| 8 | **Turn restriction arc bounds** | Every `forbidden_turn_from_arc[i] < arc_count` and `forbidden_turn_to_arc[i] < arc_count` |
| 9 | **No isolated nodes** | Count nodes with degree 0; warn if > 1% (some are expected from data cleaning) |
| 10 | **Graph connectivity** | BFS/DFS from a random node; warn if largest connected component < 90% of nodes |

### 3.3 Implementation skeleton

```cpp
#include <routingkit/vector_io.h>
#include <iostream>
#include <cassert>
#include <algorithm>
#include <string>
#include <vector>
#include <queue>

struct ValidationResult {
    unsigned node_count;
    unsigned arc_count;
    unsigned forbidden_turn_count;
    unsigned isolated_nodes;
    unsigned largest_component_size;
    bool all_passed;
};

// Derive source node for each arc from first_out (build tail array)
std::vector<unsigned> build_tail(const std::vector<unsigned>& first_out, unsigned arc_count) {
    std::vector<unsigned> tail(arc_count);
    for (unsigned v = 0; v + 1 < first_out.size(); ++v) {
        for (unsigned e = first_out[v]; e < first_out[v + 1]; ++e) {
            tail[e] = v;
        }
    }
    return tail;
}

// BFS to find largest connected component (treating graph as undirected)
unsigned largest_component_bfs(const std::vector<unsigned>& first_out,
                                const std::vector<unsigned>& head,
                                unsigned node_count) {
    // Build adjacency for reverse edges too (undirected)
    // BFS from node 0, return component size
    // Then try remaining unvisited nodes
    // Return max component size
}

int main(int argc, char* argv[]) {
    // 1. Parse args: graph_dir, --turn-expanded flag

    // 2. Load vectors
    auto first_out    = RoutingKit::load_vector<unsigned>(dir + "/first_out");
    auto head         = RoutingKit::load_vector<unsigned>(dir + "/head");
    auto travel_time  = RoutingKit::load_vector<unsigned>(dir + "/travel_time");
    auto geo_distance = RoutingKit::load_vector<unsigned>(dir + "/geo_distance");
    auto latitude     = RoutingKit::load_vector<float>(dir + "/latitude");
    auto longitude    = RoutingKit::load_vector<float>(dir + "/longitude");

    unsigned node_count = first_out.size() - 1;
    unsigned arc_count  = head.size();

    // 3. Run each check, report PASS/FAIL with details
    // 4. Print summary
}
```

### 3.4 Turn-expanded graph checks (when `--turn-expanded`)

When `--turn-expanded` is passed, `validate_graph` should ALSO load and check:

| # | Check | What it validates |
|---|-------|-------------------|
| 11 | **Line graph node count = original arc count** | Load original graph's `head` to get original `arc_count`. Line graph's `node_count` (from `first_out.size() - 1`) should equal original `arc_count`. |
| 12 | **No forbidden turns in edges** | The line graph should have NO edge corresponding to a forbidden turn. Cross-check against `forbidden_turn_from_arc`/`forbidden_turn_to_arc`. |
| 13 | **No U-turns in edges** | No line graph edge connects node `e1` to node `e2` where `tail[e1] == head[e2]` in the original graph. |
| 14 | **Permutation validity** | If `cch_exp_perm` exists: it has length == line graph node count, contains each value in `[0, N)` exactly once. |

> **Note**: For check 11-13, we need both the original graph AND the line graph in the same directory, or two directories. The simplest approach: `validate_graph <original_dir> --turn-expanded <line_graph_dir>`.

**However**, the line graph is built by Rust code (`rust_road_router`), not by CCH-Generator. So the turn-expanded checks are a **secondary goal** — the primary testing module validates the *original* graph produced by `generate_graph`.

### 3.5 Alternative: turn expansion validation without Rust

If we want to validate turn expansion purely in C++, we can implement a minimal line graph builder in CCH-Generator to cross-check against Rust's output. This is **optional** and can be deferred.

---

## Part 4: Build InertialFlowCutter & Generate Node Orderings

> **Owner**: Human only

InertialFlowCutter produces the nested dissection orderings needed for CCH. It must be built and run separately after graph generation.

### 4.1 Build InertialFlowCutter

```bash
cd /home/thomas/VTS/Hanoi-Routing/rust_road_router/lib/InertialFlowCutter

# Initialize submodules (KaHIP + RoutingKit dependencies)
git submodule update --init --recursive

# Build
mkdir -p build && cd build
cmake -DCMAKE_BUILD_TYPE=Release ..
make -j$(nproc) console
```

**Expected output**: `build/console` binary.

### 4.2 Generate standard node ordering (`cch_perm`)

This orders the graph's **nodes** for the standard (no-turns) CCH:

```bash
cd /home/thomas/VTS/Hanoi-Routing/rust_road_router

# Run on the graph produced by CCH-Generator
./flow_cutter_cch_order.sh /path/to/output_dir

# Produces: output_dir/cch_perm
```

**What the script does** (from `flow_cutter_cch_order.sh`):
1. Loads the unweighted graph (`first_out`, `head`) + coordinates
2. Cleans the graph (remove multi-arcs, loops, add back-arcs, sort)
3. Runs InertialFlowCutter's accelerated CCH ordering (8 geo-position cutters)
4. Saves `cch_perm` — a permutation `Vec<u32>` mapping rank → original node ID

**Validation**: The output `cch_perm` should have exactly `node_count` elements, with each value in `[0, node_count)` appearing exactly once.

### 4.3 Generate line graph ordering (`cch_exp_perm`)

This orders the original graph's **arcs** — which become the line graph's **nodes**:

```bash
cd /home/thomas/VTS/Hanoi-Routing/rust_road_router

# Run on the SAME original graph directory (NOT the line graph — IFC works on original arcs)
./flow_cutter_cch_cut_order.sh /path/to/output_dir

# Produces: output_dir/cch_perm_cuts
# Rename for the turn-aware pipeline:
mv /path/to/output_dir/cch_perm_cuts /path/to/output_dir/cch_exp_perm
```

**What the script does** (from `flow_cutter_cch_cut_order.sh`):
1. Loads the unweighted graph + coordinates (same as standard script)
2. Runs `reorder_arcs_in_accelerated_flow_cutter_cch_order` (arc-based, not node-based)
3. Saves `cch_perm_cuts` — an **arc** permutation

**Key insight**: Since line graph node `k` = original arc `k`, an arc permutation on the original graph IS a node permutation for the line graph. The rename from `cch_perm_cuts` to `cch_exp_perm` is just a naming convention for the Rust pipeline.

**Validation**: The output `cch_exp_perm` should have exactly `arc_count` elements (original graph arc count), with each value in `[0, arc_count)` appearing exactly once.

### 4.4 Validation helper (can be done in `validate_graph`)

Add a `--check-perm <file> <expected_size>` flag to `validate_graph`:
- Load the permutation file
- Check length == expected_size
- Check it's a valid permutation (each value in `[0, N)` appears exactly once)

---

## Part 5: End-to-End Test Procedure

> **Owner**: Human (running commands) + AI (reviewing output)

### Full pipeline for `hanoi.osm.pbf`

```bash
# 0. Build CCH-Generator
cd /home/thomas/VTS/Hanoi-Routing/CCH-Generator
mkdir -p build && cd build
cmake -DCMAKE_BUILD_TYPE=Release ..
make -j$(nproc)

# 1. Generate car graph
./cch_generator ../../Maps/hanoi.osm.pbf ../../data/hanoi_car --profile car

# 2. Validate car graph
./validate_graph ../../data/hanoi_car

# 3. Generate motorcycle graph
./cch_generator ../../Maps/hanoi.osm.pbf ../../data/hanoi_motorcycle --profile motorcycle

# 4. Validate motorcycle graph
./validate_graph ../../data/hanoi_motorcycle

# 5. Generate InertialFlowCutter orderings (standard + cut-based)
cd /home/thomas/VTS/Hanoi-Routing/rust_road_router
./flow_cutter_cch_order.sh ../data/hanoi_car
./flow_cutter_cch_cut_order.sh ../data/hanoi_car
mv ../data/hanoi_car/cch_perm_cuts ../data/hanoi_car/cch_exp_perm

# 6. Validate permutations
cd /home/thomas/VTS/Hanoi-Routing/CCH-Generator/build
./validate_graph ../../data/hanoi_car --check-perm ../../data/hanoi_car/cch_perm
./validate_graph ../../data/hanoi_car --check-perm ../../data/hanoi_car/cch_exp_perm
```

### Expected output dimensions (rough estimates for Hanoi)

| Metric | Hanoi estimate |
|--------|---------------|
| Node count | ~200K–400K |
| Arc count | ~500K–1M |
| Forbidden turns | ~5K–20K |
| `cch_perm` length | = node count |
| `cch_exp_perm` length | = arc count |

### Cross-profile comparison

After generating both car and motorcycle graphs from the same PBF, verify:
- **Same topology** (`first_out`, `head` may differ because different ways are included — motorcycle adds `track` and `path`)
- **Motorcycle has more arcs** (includes `highway=track` and `highway=path` which car excludes)
- **Different travel times** (motorcycle speed table differs from car)
- **Different forbidden turns** (motorcycle checks `restriction:motorcycle` first, has `except` exemptions)

---

## Summary: Task Ownership

| Part | Task | Owner |
|------|------|-------|
| 0 | Build RoutingKit (`make`) | Human |
| 1.1 | CMakeLists.txt setup | Human + AI |
| 2 | `generate_graph.cpp` | Human + AI |
| 3 | `validate_graph.cpp` | Human + AI |
| 4.1 | Build InertialFlowCutter | Human |
| 4.2 | Run `flow_cutter_cch_order.sh` (standard perm) | Human |
| 4.3 | Run `flow_cutter_cch_cut_order.sh` + rename (line graph perm) | Human |
| 5 | End-to-end testing | Human + AI (review) |

---

## Dependency Order

```
Part 0 (Build RoutingKit)
  ↓
Part 1 (CMakeLists.txt)
  ↓
Part 2 (generate_graph.cpp)  ──→  Part 3 (validate_graph.cpp)
  ↓                                  ↓
  ↓                           [validate standard graph]
  ↓
Part 4.1 (Build InertialFlowCutter)
  ↓
Part 4.2 (cch_perm)  ←── needs graph output from Part 2
Part 4.3 (cch_exp_perm) ←── needs graph output from Part 2
  ↓
Part 5 (End-to-end test)  ←── needs Parts 2, 3, 4
```

Parts 2 and 3 can be developed in parallel. Parts 4.2 and 4.3 can run in parallel (they read the same input, write different files).
