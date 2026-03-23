# Manual Pipeline Guide: From OSM PBF to Turn-Expanded Graph

A hands-on, step-by-step walkthrough of the graph generation pipeline — the same steps that `CCH-Generator/scripts/run_pipeline` automates — but executed manually so you can inspect every intermediate result.

---

## Table of Contents

1. [Prerequisites](#1-prerequisites)
2. [Understanding the Data Format](#2-understanding-the-data-format)
3. [Phase 1 — Generate the Base Graph](#3-phase-1--generate-the-base-graph)
4. [Phase 2 — Validate the Base Graph](#4-phase-2--validate-the-base-graph)
5. [Phase 3 — Extract Conditional Turn Restrictions](#5-phase-3--extract-conditional-turn-restrictions)
6. [Phase 4 — Validate After Conditional Turns](#6-phase-4--validate-after-conditional-turns)
7. [Phase 5 — Generate the Turn-Expanded Line Graph](#7-phase-5--generate-the-turn-expanded-line-graph)
8. [Phase 6 — Validate the Line Graph](#8-phase-6--validate-the-line-graph)
9. [Phase 7 — CCH Node Ordering (Normal Graph)](#9-phase-7--cch-node-ordering-normal-graph)
10. [Phase 8 — CCH Node Ordering (Line Graph)](#10-phase-8--cch-node-ordering-line-graph)
11. [Inspecting the Output](#11-inspecting-the-output)
12. [Conceptual Deep Dive](#12-conceptual-deep-dive)

---

## 1. Prerequisites

### 1.1 Required Software

| Tool | Purpose | Install |
|------|---------|---------|
| **CMake ≥ 3.16** | Build CCH-Generator (C++) | `sudo dnf install cmake` |
| **GCC/G++ with C++17** | Compile C++ code | `sudo dnf install gcc-c++` |
| **zlib** | PBF decompression (RoutingKit dependency) | `sudo dnf install zlib-devel` |
| **Rust (nightly)** | Build CCH-Hanoi tools/workspace | [rustup.rs](https://rustup.rs) |
| **Cargo** | Rust package manager (comes with Rust) | Included with rustup |
| **Python 3** | Inspecting binary files (optional) | Usually pre-installed |

### 1.2 Input Data

You need an OpenStreetMap PBF extract covering Hanoi. Place it somewhere accessible, e.g.:

```
~/VTS/Hanoi-Routing/Maps/vietnam-latest.osm.pbf
```

> **Tip**: Download a Vietnam extract from [Geofabrik](https://download.geofabrik.de/asia/vietnam.html) or use `osmium-tool` to clip a Hanoi-specific region from a larger extract.

### 1.3 Build the C++ Binaries

The pipeline uses two C++ binaries from `CCH-Generator/`. Build them first:

```bash
cd ~/VTS/Hanoi-Routing/CCH-Generator
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build -j$(nproc)
```

This produces:
- `build/cch_generator` — converts PBF → RoutingKit binary graph
- `build/validate_graph` — validates graph integrity

Verify they exist:

```bash
ls -la build/cch_generator build/validate_graph
```

### 1.4 Build the RoutingKit Conditional Turn Extractor

This binary should already be built if you've previously built RoutingKit. Verify:

```bash
ls -la ~/VTS/Hanoi-Routing/RoutingKit/bin/conditional_turn_extract
```

If missing, rebuild RoutingKit:

```bash
cd ~/VTS/Hanoi-Routing/RoutingKit
./generate_make_file
make -j$(nproc)
```

### 1.5 Build the Rust Line Graph Generator

```bash
cd ~/VTS/Hanoi-Routing/CCH-Hanoi
cargo build --release -p hanoi-tools --bin generate_line_graph
```

This produces `target/release/generate_line_graph`.

### 1.6 Set Up Working Variables

For the rest of this guide, define these variables in your shell:

```bash
REPO=~/VTS/Hanoi-Routing
INPUT_PBF="$REPO/Maps/vietnam-latest.osm.pbf"   # adjust to your PBF path
OUTPUT_DIR="$REPO/Maps/data/hanoi_car"            # or hanoi_motorcycle
GRAPH_DIR="$OUTPUT_DIR/graph"
PROFILE="car"                                      # or "motorcycle"

CCH_GEN="$REPO/CCH-Generator/build/cch_generator"
VALIDATOR="$REPO/CCH-Generator/build/validate_graph"
COND_EXTRACT="$REPO/RoutingKit/bin/conditional_turn_extract"
LINE_GRAPH_GEN="$REPO/CCH-Hanoi/target/release/generate_line_graph"
```

---

## 2. Understanding the Data Format

Before diving in, understand what you'll be working with.

### RoutingKit Binary Format

All graph files use **headerless raw binary vectors** — no magic numbers, no length prefix, just a flat array of values dumped to disk. The type is inferred by convention:

| File | Element type | Meaning |
|------|-------------|---------|
| `first_out` | `u32` (4 bytes) | CSR row offsets — `first_out[i]` is the index of node `i`'s first outgoing edge |
| `head` | `u32` | Target node of each edge |
| `travel_time` | `u32` | Edge weight in **milliseconds** |
| `geo_distance` | `u32` | Geometric distance |
| `latitude` | `f32` | Node latitude |
| `longitude` | `f32` | Node longitude |
| `way` | `u32` | RoutingKit routing-way ID for each edge |
| `forbidden_turn_from_arc` | `u32` | Source arc of each forbidden turn |
| `forbidden_turn_to_arc` | `u32` | Destination arc of each forbidden turn |

### CSR (Compressed Sparse Row) Encoding

The graph is stored as a CSR adjacency list:

```
Node 0's edges: head[first_out[0]] .. head[first_out[1] - 1]
Node 1's edges: head[first_out[1]] .. head[first_out[2] - 1]
...
Node N's edges: head[first_out[N]] .. head[first_out[N+1] - 1]
```

Key invariants:
- `first_out` has `num_nodes + 1` elements
- `first_out[0] == 0`
- `first_out[num_nodes] == num_arcs`
- `head`, `travel_time`, `geo_distance`, `way` all have `num_arcs` elements

### How to Read a Binary File (Quick Inspection)

```python
import struct, sys

def read_u32_vector(path):
    with open(path, 'rb') as f:
        data = f.read()
    return struct.unpack(f'<{len(data)//4}I', data)

def read_f32_vector(path):
    with open(path, 'rb') as f:
        data = f.read()
    return struct.unpack(f'<{len(data)//4}f', data)

# Example: read first_out and compute graph dimensions
first_out = read_u32_vector('Maps/data/hanoi_car/graph/first_out')
head = read_u32_vector('Maps/data/hanoi_car/graph/head')
print(f"Nodes: {len(first_out) - 1:,}")
print(f"Arcs:  {len(head):,}")
print(f"Node 0 has {first_out[1] - first_out[0]} outgoing edges")
```

---

## 3. Phase 1 — Generate the Base Graph

```bash
mkdir -p "$GRAPH_DIR"
"$CCH_GEN" "$INPUT_PBF" "$GRAPH_DIR" --profile "$PROFILE"
```

### What Happens Internally

1. **Two-pass OSM loading** (RoutingKit's architecture):
   - **Pass 1**: Scan PBF to discover all node/way/relation IDs used by the road network
   - **Pass 2**: Build the graph, computing travel times and extracting turn restrictions

2. **Profile-specific callbacks**: The `--profile car` flag selects:
   - `is_osm_way_used_by_cars()` — filters relevant road types
   - `get_osm_way_speed()` — speed limits per road class
   - `get_osm_car_direction_category()` — one-way / two-way
   - `decode_osm_car_turn_restrictions()` — forbidden turn relations

3. **Travel time calculation**: `travel_time[arc] = geo_distance[arc] × 18000 / speed / 5` (result in milliseconds)

### What It Produces

```
$GRAPH_DIR/
├── first_out                    # CSR structure
├── head                         # Edge targets
├── travel_time                  # Edge weights (ms)
├── geo_distance                 # Raw distances
├── way                          # Routing-way IDs (needed later for conditional turns)
├── latitude                     # Node coordinates
├── longitude
├── forbidden_turn_from_arc      # Unconditional turn restrictions
└── forbidden_turn_to_arc
```

### Quick Sanity Check

```bash
# File sizes should be consistent
python3 -c "
import os
d = '$GRAPH_DIR'
fo = os.path.getsize(f'{d}/first_out') // 4
h  = os.path.getsize(f'{d}/head') // 4
tt = os.path.getsize(f'{d}/travel_time') // 4
ft = os.path.getsize(f'{d}/forbidden_turn_from_arc') // 4
print(f'Nodes:           {fo - 1:>10,}')
print(f'Arcs:            {h:>10,}')
print(f'travel_time len: {tt:>10,}  (should == arcs)')
print(f'Forbidden turns: {ft:>10,}')
"
```

For the current Hanoi car graph, expect roughly: **276,372 nodes**, **654,787 arcs**, **403 forbidden turns**.

---

## 4. Phase 2 — Validate the Base Graph

```bash
"$VALIDATOR" "$GRAPH_DIR"
```

### What It Checks

The validator runs ~15 checks across several categories:

**Structural integrity:**
- CSR invariants (`first_out[0] == 0`, monotonically increasing, consistent with `head` length)
- No self-loops (`head[i] != tail[i]` for all edges)
- All `head` values within `[0, num_nodes)`

**Geographic sanity:**
- All coordinates within Vietnam bounds (lat 0–30°, lon 100–115°)

**Weight sanity:**
- No zero travel times (warns for ferries)
- No travel time > 24 hours

**Turn restriction integrity:**
- `forbidden_turn_from_arc` is sorted
- Both arrays have equal length
- All arc indices are valid

**Connectivity:**
- Reports isolated node count and largest connected component

### Reading the Output

```
[PASS] CSR structure valid
[PASS] No self-loops
[PASS] Coordinates within Vietnam bounds
[WARN] 142 isolated nodes (0.05%)
[PASS] Forbidden turns: 403 pairs, sorted, valid indices
...
Validation result: PASS
```

- `[PASS]` — check succeeded
- `[WARN]` — non-critical issue (does not fail validation)
- `[FAIL]` — critical issue (exit code 1)

---

## 5. Phase 3 — Extract Conditional Turn Restrictions

This is where it gets interesting. The base graph only has **unconditional** forbidden turns (always active). Many real-world restrictions are **time-dependent** (e.g., "no left turn Monday–Friday 7:00–9:00") or use **via-way** routing (where the restriction path goes through a way, not just a node).

```bash
"$COND_EXTRACT" "$INPUT_PBF" "$GRAPH_DIR" "$OUTPUT_DIR" --profile "$PROFILE"
```

### What Happens Internally

1. **PBF scan**: Reads all OSM relations looking for:
   - `restriction:conditional` tags (e.g., `no_right_turn @ (Mo-Fr 07:00-09:00)`)
   - `restriction:motorcar:conditional` tags (car-specific, higher priority)
   - Unconditional restrictions with `via` member pointing to a **way** (not a node) — these were silently dropped by RoutingKit's standard extractor

2. **Resolution to arc pairs**: Using the loaded graph (`first_out`, `head`, `way`, coordinates):
   - **Via-node restrictions**: Map directly to one `(from_arc, to_arc)` pair at the junction
   - **Via-way restrictions**: Decomposed into **two** arc pairs:
     - `(from_arc, via_entry_arc)` at junction A (where from-way meets via-way)
     - `(via_exit_arc, to_arc)` at junction B (where via-way meets to-way)

3. **Condition parsing**: Converts condition strings into `TimeWindow` structs:
   - Day ranges: `Mo-Fr`, `Sa,Su`, `Mo-Su`
   - Time ranges: `07:00-09:00`, `16:00-18:00`
   - Combined: `Mo-Fr 07:00-09:00,16:00-18:00`

4. **Sorting**: Output is sorted by `(from_arc, to_arc)` — required by the peekable-iterator consumption pattern downstream.

### What It Produces

Three new files under the profile root:

```
$OUTPUT_DIR/
├── graph/
│   └── ... (existing graph files) ...
└── conditional_turns/
    ├── conditional_turn_from_arc        # u32 vector — source arcs
    ├── conditional_turn_to_arc          # u32 vector — destination arcs
    └── conditional_turn_time_windows    # packed binary — time window data
```

### Time Window Format (Deep Dive)

The `conditional_turn_time_windows` file uses a custom packed format:

```
┌─────────────────────────── Offset array ───────────────────────────┐
│ offset[0]: u32 │ offset[1]: u32 │ ... │ offset[N]: u32            │
└────────────────┴────────────────┴─────┴───────────────────────────┘
┌─────────────────────────── TimeWindow data ────────────────────────┐
│ window[0]: 5 bytes │ window[1]: 5 bytes │ ...                     │
└────────────────────┴────────────────────┴─────────────────────────┘
```

Each `TimeWindow` is 5 bytes, packed:

| Field | Type | Meaning |
|-------|------|---------|
| `day_mask` | `u8` | Bit 0 = Monday, ..., Bit 6 = Sunday. `0x7F` = all days |
| `start_minutes` | `u16` | Minutes since midnight (0–1440) |
| `end_minutes` | `u16` | Minutes since midnight (can exceed 1440 for overnight) |

Time windows for turn pair `i` are at byte offsets `[offset[i], offset[i+1])`.
If `offset[i] == offset[i+1]`, the restriction has **no time windows** — it's unconditional (always active). This is how via-way restrictions that were missed by the standard extractor are represented.

### Quick Inspection

```python
import struct

def read_u32(path):
    with open(path, 'rb') as f:
        data = f.read()
    return struct.unpack(f'<{len(data)//4}I', data)

from_arcs = read_u32(f'{OUTPUT_DIR}/conditional_turns/conditional_turn_from_arc')
to_arcs   = read_u32(f'{OUTPUT_DIR}/conditional_turns/conditional_turn_to_arc')
print(f"Conditional turn pairs: {len(from_arcs)}")

# Inspect time windows
with open(f'{OUTPUT_DIR}/conditional_turns/conditional_turn_time_windows', 'rb') as f:
    raw = f.read()

num_pairs = len(from_arcs)
offsets = struct.unpack(f'<{num_pairs + 1}I', raw[:4 * (num_pairs + 1)])
tw_data = raw[4 * (num_pairs + 1):]

for i in range(min(5, num_pairs)):
    start, end = offsets[i], offsets[i+1]
    num_windows = (end - start) // 5
    print(f"  Pair ({from_arcs[i]}, {to_arcs[i]}): {num_windows} time window(s)")
    for j in range(num_windows):
        chunk = tw_data[start + j*5 : start + j*5 + 5]
        day_mask, t_start, t_end = struct.unpack('<BHH', chunk)
        days = ''.join(d for bit, d in enumerate('MTWTFSS') if day_mask & (1 << bit))
        print(f"    days={days} {t_start//60:02d}:{t_start%60:02d}-{t_end//60:02d}:{t_end%60:02d}")
```

---

## 6. Phase 4 — Validate After Conditional Turns

Re-run the validator — it will automatically detect and check the new conditional turn files:

```bash
"$VALIDATOR" "$GRAPH_DIR"
```

### Additional Checks (beyond Phase 2)

When conditional turn files are present, the validator also checks:

- `conditional_turn_from_arc` is sorted
- Both arrays have equal length
- All arc indices are valid
- Time window offsets are monotonically increasing
- Time window byte count matches expectations
- **No overlap** between conditional and forbidden turns (a turn can't be both)

---

## 7. Phase 5 — Generate the Turn-Expanded Line Graph

This is the conceptual heart of the pipeline. The **line graph transformation** converts the original road graph into a new graph where **edges become nodes** and **valid turns become edges**.

```bash
"$LINE_GRAPH_GEN" "$GRAPH_DIR" "$OUTPUT_DIR/line_graph"
```

Or with a custom output directory:

```bash
"$LINE_GRAPH_GEN" "$GRAPH_DIR" "$OUTPUT_DIR/line_graph"
```

### What Happens Internally

1. **Load the original graph**: Reads `first_out`, `head`, `travel_time`, `latitude`, `longitude`, `forbidden_turn_from_arc`, `forbidden_turn_to_arc`.

2. **Build the tail array**: Reconstructs the source node of each edge (the CSR format only stores targets):
   ```
   For each node n:
     For each outgoing edge of n:
       tail[edge_id] = n
   ```

3. **Enumerate all possible turns**: For each original edge `e1` ending at node `v`, consider every outgoing edge `e2` from `v`.

4. **Filter forbidden turns**: A turn `(e1, e2)` is **excluded** if:
   - It appears in `(forbidden_turn_from_arc, forbidden_turn_to_arc)` — the sorted peekable-iterator pattern efficiently checks this in O(1) amortized
   - It's a **U-turn**: `tail[e1] == head[e2]` (going back to where you came from)

5. **Build the line graph CSR**: Valid turns become edges. The weight of a line-graph edge is the travel time of the *first* original edge (the turn cost is 0).

6. **Remap coordinates**: Each line-graph node (= original edge) gets the coordinates of its source node in the original graph.

### What It Produces

```
$OUTPUT_DIR/line_graph/
├── first_out       # CSR structure of the line graph
├── head            # Turn targets
├── travel_time     # Weights (inherited from original edges)
├── latitude        # Remapped coordinates
└── longitude
```

### Expected Dimensions

For Hanoi car graph:
- **Line graph nodes** = original arcs ≈ **654,787**
- **Line graph arcs** ≈ **1.3M** (each intersection contributes `in_degree × out_degree` turns, minus forbidden/U-turns)
- **Average degree** ≈ 2.0 (most roads are simple through-streets)

### Quick Inspection

```bash
python3 -c "
import os
d = '$OUTPUT_DIR/line_graph'
fo = os.path.getsize(f'{d}/first_out') // 4
h  = os.path.getsize(f'{d}/head') // 4
print(f'Line graph nodes: {fo - 1:>10,}  (should == original arcs)')
print(f'Line graph arcs:  {h:>10,}')
print(f'Avg degree:       {h / (fo - 1):.2f}')
"
```

---

## 8. Phase 6 — Validate the Line Graph

```bash
"$VALIDATOR" "$GRAPH_DIR" --turn-expanded "$OUTPUT_DIR/line_graph"
```

### What It Checks

- **Node count** of line graph == arc count of original graph
- **No forbidden-turn transitions** exist in the line graph
- **No U-turn transitions** in the line graph
- **All transitions are valid**: each line-graph edge `(e1, e2)` satisfies `head[e1] == tail[e2]` in the original graph (i.e., consecutive arcs sharing a node)
- CSR integrity of the line graph itself

---

## 9. Phase 7 — CCH Node Ordering (Normal Graph)

The CCH (Customizable Contraction Hierarchy) requires a **nested dissection ordering** — a permutation of node IDs that groups spatially close nodes together, enabling efficient hierarchical contraction. This ordering is computed by **InertialFlowCutter (IFC)**, which finds balanced graph cuts using inertial flow.

### 9.1 Prerequisites — Build IFC

If you haven't already built the IFC console binary:

```bash
cd $REPO/rust_road_router/lib/InertialFlowCutter
mkdir -p build
/usr/bin/cmake -S . -B build -DCMAKE_BUILD_TYPE=Release -DGIT_SUBMODULE=OFF -DUSE_KAHIP=OFF
cmake --build build --target console -j"$(nproc)"
```

Verify:

```bash
ls -la $REPO/rust_road_router/lib/InertialFlowCutter/build/console
```

### 9.2 Set Up the IFC Variable

Add to your shell variables:

```bash
IFC_CONSOLE="$REPO/rust_road_router/lib/InertialFlowCutter/build/console"
```

### 9.3 Run IFC on the Normal Graph

The simplest way is to use the provided wrapper script:

```bash
cd $REPO/rust_road_router
./flow_cutter_cch_order.sh "$GRAPH_DIR"
```

This produces `$GRAPH_DIR/perms/cch_perm`.

> For a detailed reference on all three IFC scripts (arguments, internals, output format), see [IFC Scripts Reference](IFC Scripts Reference.md).

> **Smart directory resolution**: All three IFC wrapper scripts (`flow_cutter_cch_order.sh`, `flow_cutter_cch_cut_order.sh`, `flow_cutter_cch_cut_reorder.sh`) accept either a graph directory (containing `first_out` directly) or a profile directory (containing `graph/first_out`). If `$1/first_out` doesn't exist but `$1/graph/first_out` does, the script automatically resolves to the `graph/` subdirectory. So both of these are equivalent:
>
> ```bash
> ./flow_cutter_cch_order.sh "$GRAPH_DIR"          # direct graph dir
> ./flow_cutter_cch_order.sh "$OUTPUT_DIR"          # profile dir — auto-resolves to graph/
> ```

#### Manual invocation (equivalent)

If you want full control or need to tune parameters:

```bash
"$IFC_CONSOLE" \
  load_routingkit_unweighted_graph "$GRAPH_DIR/first_out" "$GRAPH_DIR/head" \
  load_routingkit_longitude "$GRAPH_DIR/longitude" \
  load_routingkit_latitude "$GRAPH_DIR/latitude" \
  remove_multi_arcs \
  remove_loops \
  add_back_arcs \
  sort_arcs \
  flow_cutter_set random_seed 5489 \
  reorder_nodes_at_random \
  reorder_nodes_in_preorder \
  flow_cutter_set thread_count ${2:-$(nproc)} \
  flow_cutter_set BulkDistance no \
  flow_cutter_set max_cut_size 100000000 \
  flow_cutter_set distance_ordering_cutter_count 0 \
  flow_cutter_set geo_pos_ordering_cutter_count 8 \
  flow_cutter_set bulk_assimilation_threshold 0.4 \
  flow_cutter_set bulk_assimilation_order_threshold 0.25 \
  flow_cutter_set bulk_step_fraction 0.05 \
  flow_cutter_set initial_assimilated_fraction 0.05 \
  flow_cutter_config \
  report_time \
  reorder_nodes_in_accelerated_flow_cutter_cch_order \
  do_not_report_time \
  examine_chordal_supergraph \
  save_routingkit_node_permutation_since_last_load "$GRAPH_DIR/perms/cch_perm"
```

> **Note on `thread_count`**: The `${2:-$(nproc)}` syntax is shell parameter expansion — it takes the script's second positional argument, defaulting to `$(nproc)` (all available cores) if not provided. IFC's `thread_count` must be `>= 1`; the previous default of `-1` was invalid and caused a runtime error. When running the script directly, you can pass a thread count as the second argument: `./flow_cutter_cch_order.sh "$GRAPH_DIR" 4`.

### 9.4 What It Does

1. **Loads the graph** as an unweighted structure (only topology matters — CCH ordering is metric-independent)
2. **Preprocesses**: removes multi-arcs and self-loops, adds back-arcs (makes the graph undirected), sorts arcs
3. **Computes nested dissection**: uses 8 geographic-position-based cutters (`geo_pos_ordering_cutter_count 8`) to find balanced separators recursively. Inertial flow uses node coordinates to find good initial cuts.
4. **Examines chordal supergraph**: reports quality metrics (fill-in edges, tree width)
5. **Saves** the node permutation as `cch_perm` — a `Vec<u32>` mapping rank → original node ID

### 9.5 Key Parameters

| Parameter | Default | Effect |
|-----------|---------|--------|
| `random_seed` | 5489 | Deterministic ordering. Change for different orderings. |
| `thread_count` | `$(nproc)` (all cores) | Parallelism. Must be `>= 1`. Pass as 2nd script arg (e.g., `./flow_cutter_cch_order.sh "$GRAPH_DIR" 4`), or set inline in manual invocation. |
| `geo_pos_ordering_cutter_count` | 8 | Number of geographic cutters. More = better quality but slower. |
| `max_cut_size` | 100000000 | Upper bound on separator size. |
| `bulk_assimilation_threshold` | 0.4 | Controls how aggressively small components are absorbed. |

### 9.6 What It Produces

```
$GRAPH_DIR/
├── ... (existing files) ...
└── perms/
    └── cch_perm                # Vec<u32> — node permutation for CCH
```

### 9.7 Quick Verification

```bash
python3 -c "
import os
perm_size = os.path.getsize('$GRAPH_DIR/perms/cch_perm') // 4
fo_size = os.path.getsize('$GRAPH_DIR/first_out') // 4 - 1
print(f'cch_perm entries: {perm_size:,}')
print(f'graph nodes:      {fo_size:,}')
assert perm_size == fo_size, 'MISMATCH!'
print('OK: cch_perm size matches node count')
"
```

### 9.8 Expected Runtime

For the Hanoi graph (~929K nodes uncompressed), expect **2–10 minutes** depending on CPU cores and clock speed. The `report_time` command in the IFC pipeline will print the elapsed time.

---

## 10. Phase 8 — CCH Node Ordering (Line Graph)

For turn-aware routing, you also need a CCH ordering for the **line graph**. Since the line graph is already materialized as a standard CSR graph (with its own `first_out`, `head`, `latitude`, `longitude`), you can run the same `flow_cutter_cch_order.sh` script on it — treating line graph nodes as regular nodes.

### 10.1 Run IFC on the Line Graph

Use the **same wrapper script** as Phase 7, just pointed at the line graph directory:

```bash
cd $REPO/rust_road_router
./flow_cutter_cch_order.sh "$OUTPUT_DIR/line_graph"
```

This produces `$OUTPUT_DIR/line_graph/perms/cch_perm` directly — no renaming needed. The line graph directory contains `first_out` directly (no nested `graph/` subdirectory), so the script's smart resolution picks it up as-is.

#### Manual invocation (equivalent)

```bash
"$IFC_CONSOLE" \
  load_routingkit_unweighted_graph "$OUTPUT_DIR/line_graph/first_out" "$OUTPUT_DIR/line_graph/head" \
  load_routingkit_longitude "$OUTPUT_DIR/line_graph/longitude" \
  load_routingkit_latitude "$OUTPUT_DIR/line_graph/latitude" \
  remove_multi_arcs \
  remove_loops \
  add_back_arcs \
  sort_arcs \
  flow_cutter_set random_seed 5489 \
  reorder_nodes_at_random \
  reorder_nodes_in_preorder \
  flow_cutter_set thread_count ${2:-$(nproc)} \
  flow_cutter_set BulkDistance no \
  flow_cutter_set max_cut_size 100000000 \
  flow_cutter_set distance_ordering_cutter_count 0 \
  flow_cutter_set geo_pos_ordering_cutter_count 8 \
  flow_cutter_set bulk_assimilation_threshold 0.4 \
  flow_cutter_set bulk_assimilation_order_threshold 0.25 \
  flow_cutter_set bulk_step_fraction 0.05 \
  flow_cutter_set initial_assimilated_fraction 0.05 \
  flow_cutter_config \
  report_time \
  reorder_nodes_in_accelerated_flow_cutter_cch_order \
  do_not_report_time \
  examine_chordal_supergraph \
  save_routingkit_node_permutation_since_last_load "$OUTPUT_DIR/line_graph/perms/cch_perm"
```

This is identical to the Phase 7 manual invocation — just with different input/output paths. The line graph is a standard CSR graph, so the same IFC pipeline applies.

### 10.2 Key Differences from Normal Graph Ordering

| Aspect | Normal Graph (Phase 7) | Line Graph (Phase 8) |
|--------|----------------------|---------------------|
| Input directory | `$GRAPH_DIR` | `$OUTPUT_DIR/line_graph` |
| Graph size | ~929K nodes, ~1.9M arcs | ~1.9M nodes, ~4M+ arcs |
| Script | `flow_cutter_cch_order.sh "$GRAPH_DIR"` | `flow_cutter_cch_order.sh "$OUTPUT_DIR/line_graph"` |
| Output file | `$GRAPH_DIR/perms/cch_perm` | `$OUTPUT_DIR/line_graph/perms/cch_perm` |
| Runtime | 2–10 minutes | 5–20 minutes (larger graph) |

### 10.3 What It Produces

```
$OUTPUT_DIR/line_graph/
├── ... (existing files) ...
└── perms/
    └── cch_perm                # Vec<u32> — line graph node permutation
```

### 10.4 Quick Verification

```bash
python3 -c "
import os
d = '$OUTPUT_DIR/line_graph'
perm_size = os.path.getsize(f'{d}/perms/cch_perm') // 4
fo_size = os.path.getsize(f'{d}/first_out') // 4 - 1
print(f'cch_perm entries:     {perm_size:,}')
print(f'line graph nodes:     {fo_size:,}')
assert perm_size == fo_size, 'MISMATCH!'
print('OK: cch_perm size matches line graph node count')
"
```

### 10.5 Alternative: Arc Permutation on Original Graph (without materialized line graph)

Instead of building the line graph first and running IFC on it, the `flow_cutter_cch_cut_order.sh` script can compute a line-graph-equivalent ordering directly from the **original** graph. It uses `reorder_arcs_in_accelerated_flow_cutter_cch_order normal` which internally constructs the line graph from the original graph's topology and produces an **arc permutation** — since line graph nodes = original arcs, this arc permutation is the line graph's node ordering.

```bash
cd $REPO/rust_road_router
./flow_cutter_cch_cut_order.sh "$GRAPH_DIR"
```

This produces `$GRAPH_DIR/perms/cch_perm_cuts`. To use it as the line graph's CCH ordering:

```bash
cp "$GRAPH_DIR/perms/cch_perm_cuts" "$OUTPUT_DIR/line_graph/perms/cch_perm"
```

> **Important**: `flow_cutter_cch_cut_order.sh` must be pointed at the **original** graph directory (or its parent profile directory — the smart resolution will find `graph/` automatically), not the line graph directory. It reads the original `first_out`/`head` and internally derives the line graph. Running it on the line graph directory would compute an arc ordering of the line graph, which is not useful.

#### Manual invocation — `flow_cutter_cch_cut_order.sh` (equivalent)

```bash
"$IFC_CONSOLE" \
  load_routingkit_unweighted_graph "$GRAPH_DIR/first_out" "$GRAPH_DIR/head" \
  load_routingkit_longitude "$GRAPH_DIR/longitude" \
  load_routingkit_latitude "$GRAPH_DIR/latitude" \
  flow_cutter_set random_seed 5489 \
  reorder_nodes_at_random \
  reorder_nodes_in_preorder \
  flow_cutter_set thread_count ${2:-$(nproc)} \
  flow_cutter_set BulkDistance no \
  flow_cutter_set max_cut_size 100000000 \
  flow_cutter_set distance_ordering_cutter_count 0 \
  flow_cutter_set geo_pos_ordering_cutter_count 8 \
  flow_cutter_set bulk_assimilation_threshold 0.4 \
  flow_cutter_set bulk_assimilation_order_threshold 0.25 \
  flow_cutter_set bulk_step_fraction 0.05 \
  flow_cutter_set initial_assimilated_fraction 0.05 \
  flow_cutter_config \
  report_time \
  reorder_arcs_in_accelerated_flow_cutter_cch_order normal \
  do_not_report_time \
  save_routingkit_arc_permutation_since_last_load "$GRAPH_DIR/perms/cch_perm_cuts"
```

Note the key differences from `flow_cutter_cch_order.sh`:
- **No graph preprocessing** (`remove_multi_arcs`, `remove_loops`, `add_back_arcs`, `sort_arcs` are absent) — the arc ordering operates on the raw directed graph
- **`reorder_arcs_in_accelerated_flow_cutter_cch_order normal`** instead of `reorder_nodes_in_accelerated_flow_cutter_cch_order` — computes an arc permutation, not a node permutation
- **`save_routingkit_arc_permutation_since_last_load`** instead of `save_routingkit_node_permutation_since_last_load`
- **No `examine_chordal_supergraph`** — quality metrics are not reported

The ordering quality may differ slightly from running `flow_cutter_cch_order.sh` on the materialized line graph, because the materialized approach includes the preprocessing steps.

#### Reorder variant — `flow_cutter_cch_cut_reorder.sh`

There is also a `reorder` variant that applies an additional reordering pass for potentially better CCH quality at the cost of slightly longer computation time:

```bash
cd $REPO/rust_road_router
./flow_cutter_cch_cut_reorder.sh "$GRAPH_DIR"
# Produces: $GRAPH_DIR/perms/cch_perm_cuts_reorder
```

The only difference from `flow_cutter_cch_cut_order.sh` is the mode argument: `reorder_arcs_in_accelerated_flow_cutter_cch_order reorder` instead of `normal`.

---

## 11. Inspecting the Output

### Full Inspection Script

Save this as `inspect_graph.py` and run with the profile directory as argument (for example, `Maps/data/hanoi_car`):

```python
#!/usr/bin/env python3
"""Inspect a profile directory with graph/, conditional_turns/, and line_graph/ subdirectories."""
import struct, sys, os

def read_vec(path, fmt='I'):
    size = {'I': 4, 'f': 4}[fmt]
    with open(path, 'rb') as f:
        data = f.read()
    return struct.unpack(f'<{len(data)//size}{fmt}', data)

def main():
    profile_dir = sys.argv[1]
    graph_dir = f'{profile_dir}/graph'

    first_out = read_vec(f'{graph_dir}/first_out')
    head = read_vec(f'{graph_dir}/head')
    tt = read_vec(f'{graph_dir}/travel_time')
    num_nodes = len(first_out) - 1
    num_arcs = len(head)

    print(f"=== Graph: {graph_dir} ===")
    print(f"Nodes: {num_nodes:,}")
    print(f"Arcs:  {num_arcs:,}")
    print(f"Avg degree: {num_arcs / num_nodes:.2f}")
    print()

    # Travel time stats
    times_sec = [t / 1000 for t in tt]
    print(f"Travel time: min={min(times_sec):.1f}s  max={max(times_sec):.1f}s  "
          f"avg={sum(times_sec)/len(times_sec):.1f}s")

    # Degree distribution
    degrees = [first_out[i+1] - first_out[i] for i in range(num_nodes)]
    from collections import Counter
    deg_dist = Counter(degrees)
    print(f"\nDegree distribution (top 5):")
    for deg, count in sorted(deg_dist.items(), key=lambda x: -x[1])[:5]:
        print(f"  degree {deg}: {count:,} nodes ({100*count/num_nodes:.1f}%)")

    # Forbidden turns
    ft_path = f'{graph_dir}/forbidden_turn_from_arc'
    if os.path.exists(ft_path):
        ft = read_vec(ft_path)
        print(f"\nForbidden turns: {len(ft)}")

    # Conditional turns
    ct_path = f'{profile_dir}/conditional_turns/conditional_turn_from_arc'
    if os.path.exists(ct_path):
        ct = read_vec(ct_path)
        print(f"Conditional turns: {len(ct)}")

    # Line graph
    lg_path = f'{profile_dir}/line_graph/first_out'
    if os.path.exists(lg_path):
        lg_fo = read_vec(lg_path)
        lg_head = read_vec(f'{profile_dir}/line_graph/head')
        print(f"\nLine graph: {len(lg_fo)-1:,} nodes, {len(lg_head):,} arcs "
              f"(avg degree: {len(lg_head)/(len(lg_fo)-1):.2f})")

if __name__ == '__main__':
    main()
```

### Verifying a Specific Turn Restriction

To check whether a specific forbidden turn was correctly excluded from the line graph:

```python
# Pick a forbidden turn pair
from_arc, to_arc = 42, 105  # example

# In the line graph, node `from_arc` should NOT have an edge to node `to_arc`
profile_dir = OUTPUT_DIR  # e.g. "$REPO/Maps/data/hanoi_car"
lg_first_out = read_vec(f'{profile_dir}/line_graph/first_out')
lg_head = read_vec(f'{profile_dir}/line_graph/head')

neighbors = lg_head[lg_first_out[from_arc]:lg_first_out[from_arc + 1]]
assert to_arc not in neighbors, "Forbidden turn found in line graph!"
print(f"Confirmed: turn ({from_arc} -> {to_arc}) correctly excluded")
```

---

## 12. Conceptual Deep Dive

### Why a Line Graph?

Standard shortest-path algorithms (Dijkstra, CH, CCH) operate on nodes. They can weight edges, but they cannot natively express **turn costs** or **turn restrictions** — because a "turn" involves two consecutive edges, not a single edge.

The **line graph transformation** solves this by changing the perspective:

| Original Graph | Line Graph |
|---|---|
| Node = intersection | Node = road segment (original edge) |
| Edge = road segment | Edge = turn (transition between road segments) |
| Edge weight = travel time | Edge weight = travel time of entering segment + turn cost |
| Turn restriction = ??? | Turn restriction = missing edge (simple!) |

After transformation, forbidden turns simply don't exist as edges. The routing algorithm doesn't need any special logic — it naturally avoids forbidden turns because there's no path through them.

### The Trade-off

The line graph is larger:
- **Nodes**: `num_original_arcs` (typically 2–3× the original node count)
- **Arcs**: `Σ (in_degree(v) × out_degree(v))` for all nodes `v` (minus forbidden turns and U-turns)

For Hanoi: ~276K nodes / ~655K arcs → ~655K nodes / ~1.3M arcs (roughly 2× in both dimensions).

This means routing queries take more memory and slightly more time, but the correctness gain from properly handling turn restrictions is essential for real-world navigation.

### How Conditional Turns Fit In

Conditional turns add a **time dimension**. A turn that's forbidden during rush hour is allowed at other times. The current pipeline extracts these restrictions but stores them separately — they are **not** baked into the line graph (which only handles unconditional forbidden turns).

The downstream routing server can use conditional turn data to:
1. Check the query time against time windows
2. Dynamically enable/disable turns during query execution
3. Support time-dependent CCH (TD-CCH) for more accurate routing

### The Peekable Iterator Pattern

The most elegant part of the codebase is how forbidden turns are checked during line graph construction. Both the `(from_arc, to_arc)` pairs and the `line_graph()` enumeration produce turns in the **same sorted order** — so a single peekable iterator can check membership in O(1) amortized time per turn:

```
Enumeration:  (0,1) (0,3) (0,5) (1,2) (1,4) (2,3) ...
Forbidden:           (0,3)            (1,4)
Iterator:     ^      ^     ^          ^      ^
              skip   match skip       match  skip
```

No hash set, no binary search — just a pointer that advances forward through a sorted list. This works because `line_graph()` iterates edges 0..N and for each edge iterates outgoing edges from its target in `first_out` order, which naturally produces lexicographically sorted `(e1, e2)` pairs.

---

## Quick Reference: Full Manual Pipeline

```bash
# Setup
REPO=~/VTS/Hanoi-Routing
INPUT_PBF="$REPO/Maps/vietnam-latest.osm.pbf"
OUTPUT_DIR="$REPO/Maps/data/hanoi_car"
GRAPH_DIR="$OUTPUT_DIR/graph"
PROFILE="car"

# Phase 1: Generate base graph
$REPO/CCH-Generator/build/cch_generator "$INPUT_PBF" "$GRAPH_DIR" --profile $PROFILE

# Phase 2: Validate base graph
$REPO/CCH-Generator/build/validate_graph "$GRAPH_DIR"

# Phase 3: Extract conditional turns
$REPO/RoutingKit/bin/conditional_turn_extract "$INPUT_PBF" "$GRAPH_DIR" "$OUTPUT_DIR" --profile $PROFILE

# Phase 4: Re-validate with conditional turns
$REPO/CCH-Generator/build/validate_graph "$GRAPH_DIR"

# Phase 5: Generate line graph
$REPO/CCH-Hanoi/target/release/generate_line_graph "$GRAPH_DIR" "$OUTPUT_DIR/line_graph"

# Phase 6: Validate line graph
$REPO/CCH-Generator/build/validate_graph "$GRAPH_DIR" --turn-expanded "$OUTPUT_DIR/line_graph"

# Phase 7: CCH ordering for normal graph
cd $REPO/rust_road_router
./flow_cutter_cch_order.sh "$GRAPH_DIR"

# Phase 8: CCH ordering for line graph
./flow_cutter_cch_order.sh "$OUTPUT_DIR/line_graph"
# Or via the arc-ordering variant on the original graph:
# ./flow_cutter_cch_cut_order.sh "$GRAPH_DIR"
# cp "$GRAPH_DIR/perms/cch_perm_cuts" "$OUTPUT_DIR/line_graph/perms/cch_perm"
```

For the **motorcycle** profile, change `OUTPUT_DIR` to `hanoi_motorcycle` and `PROFILE` to `motorcycle`.
