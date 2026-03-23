# IFC Scripts Reference

A detailed reference for the three InertialFlowCutter (IFC) wrapper scripts in `rust_road_router/`. These scripts compute nested dissection orderings for CCH preprocessing — the output of **CCH Phase 1 (Contraction)**.

---

## Table of Contents

1. [Overview](#1-overview)
2. [Common Behavior](#2-common-behavior)
3. [Script 1: `flow_cutter_cch_order.sh` — Node Ordering](#3-script-1-flow_cutter_cch_ordersh--node-ordering)
4. [Script 2: `flow_cutter_cch_cut_order.sh` — Arc Ordering (normal)](#4-script-2-flow_cutter_cch_cut_ordersh--arc-ordering-normal)
5. [Script 3: `flow_cutter_cch_cut_reorder.sh` — Arc Ordering (reorder)](#5-script-3-flow_cutter_cch_cut_reordersh--arc-ordering-reorder)
6. [Output Summary](#6-output-summary)
7. [Understanding the Output Log](#7-understanding-the-output-log)
8. [Choosing Which Script to Use](#8-choosing-which-script-to-use)

---

## 1. Overview

| Script | Output | Type | Applies to |
|--------|--------|------|-----------|
| `flow_cutter_cch_order.sh` | `perms/cch_perm` | Node permutation | Any CSR graph (normal or line graph) |
| `flow_cutter_cch_cut_order.sh` | `perms/cch_perm_cuts` | Arc permutation | Normal graph → produces line graph ordering |
| `flow_cutter_cch_cut_reorder.sh` | `perms/cch_perm_cuts_reorder` | Arc permutation | Normal graph → produces line graph ordering (better quality) |

All three scripts invoke the IFC `console` binary, which lives at `rust_road_router/lib/InertialFlowCutter/build/console`.

---

## 2. Common Behavior

### 2.1 Usage

```bash
./script.sh <INPUT_DIR> [THREAD_COUNT]
#              $1            $2
```

- **`$1`** — Input directory (required). The graph directory to process.
- **`$2`** — Thread count (optional). Defaults to `$(nproc)` (all available cores). Must be `>= 1`.

### 2.2 Smart Directory Resolution

All three scripts resolve the graph directory identically:

```bash
INPUT_DIR="$1"
GRAPH_DIR="$INPUT_DIR"

if [ ! -f "${GRAPH_DIR}/first_out" ] && [ -f "${INPUT_DIR}/graph/first_out" ]; then
  GRAPH_DIR="${INPUT_DIR}/graph"
fi
```

This means `$1` can be either:
- A **graph directory** directly (e.g., `Maps/data/hanoi_car/graph`) — contains `first_out`, used as-is
- A **profile directory** (e.g., `Maps/data/hanoi_car`) — auto-resolves to `graph/` subdirectory

**Output always goes to the resolved `$GRAPH_DIR`**, not necessarily `$1`. If you pass a profile directory, the permutation files land inside `graph/perms/`. Each script creates the `perms/` subdirectory via `mkdir -p` before invoking the console binary, since IFC's `save_vector` (which uses `std::ofstream`) does not create parent directories.

### 2.3 Console Binary Location

Each script locates the IFC binary relative to its own filesystem location:

```bash
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
CONSOLE_BIN="${SCRIPT_DIR}/lib/InertialFlowCutter/build/console"
```

The `CDPATH=` prefix neutralizes any user-configured `CDPATH` to ensure deterministic `cd` behavior.

### 2.4 Thread Count

```bash
flow_cutter_set thread_count ${2:-$(nproc)}
```

The `${2:-$(nproc)}` syntax is shell parameter expansion: use `$2` if provided and non-empty, otherwise use `$(nproc)`. IFC validates `thread_count >= 1` at parse time — an invalid value (like the old default `-1`) causes an immediate runtime error:

```
Exception : Value for "thread_count" must fullfill "x>=1"
```

The thread count controls TBB (Threading Building Blocks) parallelism via `tbb::global_control::max_allowed_parallelism`.

### 2.5 Shared IFC Parameters

All three scripts configure the same flow cutter settings:

| Parameter | Value | Purpose |
|-----------|-------|---------|
| `random_seed` | 5489 | Deterministic ordering. Change for different orderings. |
| `BulkDistance` | no | Disables bulk distance computation — uses geo-position cutters instead |
| `max_cut_size` | 100000000 | Upper bound on separator size (effectively unlimited) |
| `distance_ordering_cutter_count` | 0 | No distance-based cutters |
| `geo_pos_ordering_cutter_count` | 8 | 8 geographic cutters for finding balanced separators |
| `bulk_assimilation_threshold` | 0.4 | Small components < 40% of parent get absorbed |
| `bulk_assimilation_order_threshold` | 0.25 | Order threshold for bulk assimilation |
| `bulk_step_fraction` | 0.05 | Step size for bulk operations |
| `initial_assimilated_fraction` | 0.05 | Initial fraction of assimilated nodes |

### 2.6 Pre-ordering Steps

All three scripts perform these two randomization steps before computing the nested dissection:

```
reorder_nodes_at_random        # Shuffle nodes using the configured random_seed
reorder_nodes_in_preorder      # Reorder into a DFS preorder traversal
```

These improve the quality of the initial layout before the inertial flow cutter runs.

---

## 3. Script 1: `flow_cutter_cch_order.sh` — Node Ordering

### 3.1 Purpose

Computes a **node permutation** for CCH on whatever CSR graph you point it at. This is the general-purpose script.

### 3.2 Applies To

**Any CSR graph directory** — either the normal graph or the line graph:

```bash
# Normal graph
./flow_cutter_cch_order.sh "$GRAPH_DIR"

# Line graph (treats line graph nodes as regular nodes)
./flow_cutter_cch_order.sh "$OUTPUT_DIR/line_graph"
```

### 3.3 Output

`$GRAPH_DIR/perms/cch_perm` — a `Vec<u32>` of length `num_nodes`.

**Semantics:** `cch_perm[rank] = original_node_id`

- Rank 0 → least important node (contracted first)
- Rank N-1 → most important node (contracted last — typically a separator node at the top of the nested dissection tree)

### 3.4 How It Works

#### Graph Preprocessing (unique to this script)

Before computing the ordering, this script preprocesses the loaded graph:

```
remove_multi_arcs    # Remove duplicate edges between the same node pair
remove_loops         # Remove self-loops
add_back_arcs        # Add reverse edges (make the graph undirected/symmetric)
sort_arcs            # Sort adjacency lists
```

These are **required preconditions** for `reorder_nodes_in_accelerated_flow_cutter_cch_order`, which enforces (in `console.cpp`):
- `is_symmetric(tail, head)` — graph must be undirected
- `!has_multi_arcs(tail, head)` — no duplicate edges
- `is_loop_free(tail, head)` — no self-loops

#### Nested Dissection Computation

The core command:

```
reorder_nodes_in_accelerated_flow_cutter_cch_order
```

Calls `cch_order::compute_cch_graph_order(tail, head, arc_weight, ComputeSeparator(...))` which:

1. Recursively partitions the graph using inertial flow to find balanced separators
2. The 8 geographic cutters project node coordinates onto different directions and use flow computations to find small cuts
3. Returns a node permutation encoding the nested dissection ordering

The result is applied via `permutate_nodes(order)`, which updates the internal `node_original_position` tracker by composition.

#### Quality Metrics

After the ordering, this script runs:

```
examine_chordal_supergraph
```

This reports CCH quality metrics (see [Section 7](#7-understanding-the-output-log)).

#### Save

```
save_routingkit_node_permutation_since_last_load "$GRAPH_DIR/perms/cch_perm"
```

This iterates `node_original_position[i]` for all current nodes and writes the mapping as a raw `Vec<u32>`. Because `node_original_position` started as the identity at load time and was composed with every reordering operation, it maps **current position (rank) → original node ID**.

---

## 4. Script 2: `flow_cutter_cch_cut_order.sh` — Arc Ordering (normal)

### 4.1 Purpose

Computes an **arc permutation** on the normal graph that serves as a **line graph node ordering** — without needing to materialize the line graph on disk.

### 4.2 Applies To

**The normal graph only.** Must be pointed at the original graph directory:

```bash
./flow_cutter_cch_cut_order.sh "$GRAPH_DIR"
```

> **Warning**: Do NOT point this at a line graph directory. It would compute an arc ordering of the line graph, which is meaningless — you'd get an ordering for a "line graph of the line graph."

### 4.3 Output

`$GRAPH_DIR/perms/cch_perm_cuts` — a `Vec<u32>` of length `num_arcs`.

**Semantics:** `cch_perm_cuts[rank] = original_arc_id`

Since line graph nodes = original arcs, this is directly usable as a line graph CCH node ordering:

```bash
cp "$GRAPH_DIR/perms/cch_perm_cuts" "$OUTPUT_DIR/line_graph/perms/cch_perm"
```

### 4.4 How It Works

#### No Shell-Level Preprocessing

Unlike script 1, there are **no** `remove_multi_arcs`, `remove_loops`, `add_back_arcs`, `sort_arcs` commands in the script. All preprocessing is handled internally by the C++ command.

#### Internal Line Graph Construction

The `reorder_arcs_in_accelerated_flow_cutter_cch_order normal` command (`console.cpp:2970-3068`) performs these steps internally:

**Step 1 — Synthetic back-arc addition** (lines 2978-3006):

Creates `extended_tail`/`extended_head` with `2 × num_arcs` entries by mirroring every arc:
- Original arcs `[0, num_arcs)`: `extended_tail[i] = tail(i)`, `extended_head[i] = head(i)`
- Back-arcs `[num_arcs, 2×num_arcs)`: `extended_tail[i] = head(i-N)`, `extended_head[i] = tail(i-N)`

**Step 2 — Sort, deduplicate, remove loops** (lines 3008-3034):

- `sort_arcs_first_by_tail_second_by_head` — creates a sorted ordering
- `identify_non_multi_arcs` — flags duplicates for removal
- Self-loops (`tail == head`) are also flagged for removal
- Result: `simple_tail`/`simple_head` — a clean undirected graph

**Step 3 — Nested dissection on the expanded graph** (lines 3042-3047):

```cpp
tbb::global_control gc(tbb::global_control::max_allowed_parallelism, flow_cutter_config.thread_count);
order = cch_order::compute_nested_dissection_expanded_graph_order(
    simple_tail, simple_head, simple_arc_weight,
    flow_cutter::ComputeCut<...>(node_geo_pos, flow_cutter_config, reorder_arcs)
);
```

The `reorder_arcs` parameter is `false` for this script (`normal` mode).

`compute_nested_dissection_expanded_graph_order` operates on the **expanded graph** — the line graph derived from the original graph's topology. It computes a nested dissection ordering where the "nodes" being ordered are the arcs of the simplified graph.

**Step 4 — Map back to original arc IDs** (lines 3050-3066):

The expanded-graph ordering is translated back to original arc positions:

1. Start with the identity permutation over original arcs
2. Sort by the rank each arc received in the expanded graph ordering
3. Self-loops (which weren't in the simplified graph) are pushed to the end
4. The result is reversed

**Step 5 — Apply and save** (lines 3068, 2962-2965):

`permutate_arcs(final_order)` updates `arc_original_position`, which is then written to disk as the output.

#### No Quality Metrics

This script does **not** call `examine_chordal_supergraph` — that command only works for node orderings.

### 4.5 Key Difference from Script 1 on Line Graph

Running `flow_cutter_cch_order.sh` on a materialized line graph and running `flow_cutter_cch_cut_order.sh` on the original graph both produce a CCH ordering for turn-aware routing, but they differ:

| Aspect | Script 1 on line graph | Script 2 on normal graph |
|--------|----------------------|--------------------------|
| Preprocessing | Shell-level: `remove_multi_arcs`, `remove_loops`, `add_back_arcs`, `sort_arcs` | Internal: same ops but on a synthetic expanded graph |
| Quality metrics | Yes (`examine_chordal_supergraph`) | No |
| Disk requirements | Materialized line graph must exist | Works directly from normal graph |
| Output location | `line_graph/perms/cch_perm` | `graph/perms/cch_perm_cuts` (copy to line graph dir) |

---

## 5. Script 3: `flow_cutter_cch_cut_reorder.sh` — Arc Ordering (reorder)

### 5.1 Purpose

Identical to script 2, but with an additional reordering pass during separator computation for potentially better CCH quality.

### 5.2 The Only Difference

At `console.cpp:2976`:

```cpp
bool reorder_arcs = arg[0] == "reorder";  // true for script 3, false for script 2
```

This boolean is passed to `ComputeCut`:

```cpp
flow_cutter::ComputeCut<...>(node_geo_pos, flow_cutter_config, reorder_arcs)
```

When `reorder_arcs = true`, the cutter re-sorts arcs within each recursive subproblem for better geographic locality in the cuts, potentially finding tighter separators.

### 5.3 Output

`$GRAPH_DIR/perms/cch_perm_cuts_reorder` — same format and semantics as script 2.

### 5.4 Trade-off

- **Better ordering quality** (potentially tighter tree width)
- **Longer computation time** (~20–50% more than `normal` mode)

---

## 6. Output Summary

### Permutation Format

All output files are **headerless raw binary** `Vec<u32>` — one `u32` per entry, little-endian, no length prefix. This is the standard RoutingKit binary format.

### Permutation Semantics

Both node and arc permutations use the same convention:

```
perm[rank] = original_id
```

Where:
- `rank` = position in the contraction ordering (0 = contracted first, N-1 = contracted last)
- `original_id` = the node/arc ID as it appears in the on-disk graph files

### File Locations

| Script | Output path |
|--------|------------|
| `flow_cutter_cch_order.sh` | `$GRAPH_DIR/perms/cch_perm` |
| `flow_cutter_cch_cut_order.sh` | `$GRAPH_DIR/perms/cch_perm_cuts` |
| `flow_cutter_cch_cut_reorder.sh` | `$GRAPH_DIR/perms/cch_perm_cuts_reorder` |

Where `$GRAPH_DIR` is the **resolved** directory (after smart resolution), not necessarily `$1`.

### Quick Verification

```python
import struct, os

def read_u32(path):
    with open(path, 'rb') as f:
        data = f.read()
    return struct.unpack(f'<{len(data)//4}I', data)

# For node permutation (script 1):
perm = read_u32('graph/perms/cch_perm')
first_out = read_u32('graph/first_out')
assert len(perm) == len(first_out) - 1, "cch_perm length must equal node count"
assert sorted(perm) == list(range(len(perm))), "must be a valid permutation"

# For arc permutation (scripts 2/3):
arc_perm = read_u32('graph/perms/cch_perm_cuts')
head = read_u32('graph/head')
assert len(arc_perm) == len(head), "cch_perm_cuts length must equal arc count"
assert sorted(arc_perm) == list(range(len(arc_perm))), "must be a valid permutation"
```

---

## 7. Understanding the Output Log

All three scripts produce a configuration dump and timing output. Script 1 additionally produces quality metrics.

### 7.1 Configuration Dump (`flow_cutter_config`)

Printed before computation begins. Example:

```
                  BulkDistance : no
            SeparatorSelection : node_min_expansion
           AvoidAugmentingPath : avoid_and_pick_best
           SkipNonMaximumSides : skip
          GraphSearchAlgorithm : pseudo_depth_first_search
                     DumpState : no
                    ReportCuts : yes
                  PierceRating : max_target_minus_source_hop_dist
                  cutter_count : 3
                   random_seed : 5489
                        source : -1
                        target : -1
                  thread_count : 28
                  max_cut_size : 100000000
                 max_imbalance : 0.200000
                 branch_factor : 5
                    chunk_size : 0.100000
          bulk_distance_factor : 0.050000
   bulk_assimilation_threshold : 0.400000
bulk_assimilation_order_threshold : 0.250000
  initial_assimilated_fraction : 0.050000
            bulk_step_fraction : 0.050000
 geo_pos_ordering_cutter_count : 8
distance_ordering_cutter_count : 0
```

Most of these are the configured values from the script. The others (`SeparatorSelection`, `AvoidAugmentingPath`, `SkipNonMaximumSides`, `GraphSearchAlgorithm`, `PierceRating`, `cutter_count`, `max_imbalance`, `branch_factor`, `chunk_size`, `bulk_distance_factor`) are IFC internal defaults that are not set by the scripts.

Note: `source: -1` and `target: -1` are sentinel values for IFC's source/target node selection (unset), not related to the `thread_count >= 1` constraint.

### 7.2 Timing (`report_time` / `do_not_report_time`)

```
running time : 826578musec
```

Reports the wall-clock time in **microseconds** for the computation between `report_time` and `do_not_report_time`. This covers only the nested dissection computation, not the graph loading or saving.

### 7.3 Quality Metrics (`examine_chordal_supergraph`) — Script 1 Only

```
super_graph_upward_arc_count : 2076861
       upper tree width bound : 79
      elimination tree height : 207
average elimination tree depth : 125.116
 maximum arcs in search space : 8746
 average arcs in search space : 3432.97
number of triangles in super graph : 3134751
```

| Metric | Meaning | What "good" looks like |
|--------|---------|----------------------|
| `super_graph_upward_arc_count` | Total edges in the CCH after contraction (fill-in). | Lower = better ordering. |
| `upper tree width bound` | Maximum separator size encountered. | For road networks: O(√n). For Hanoi (~276K nodes), < 100 is excellent. |
| `elimination tree height` | Depth of the elimination tree. Determines sequential dependency levels during customization. | Lower = more parallelizable customization. |
| `average elimination tree depth` | Average depth across all nodes. | Reflects typical query cost. |
| `maximum arcs in search space` | Worst-case query search space size. | Determines worst-case query time. |
| `average arcs in search space` | Average query search space. | The primary indicator of average query performance. |
| `number of triangles in super graph` | Triangle count in the fill-in graph. | Affects customization work per level. |

Scripts 2 and 3 do **not** produce these metrics — `examine_chordal_supergraph` only works on node orderings, not arc orderings.

---

## 8. Choosing Which Script to Use

### For the Normal Graph

Use **script 1** (`flow_cutter_cch_order.sh`):

```bash
./flow_cutter_cch_order.sh "$GRAPH_DIR"
# → $GRAPH_DIR/perms/cch_perm
```

This is the only option — scripts 2 and 3 produce arc orderings, not node orderings.

### For the Line Graph

You have two approaches:

#### Approach A: Materialized Line Graph + Script 1

Requires the line graph to be built first (`generate_line_graph`), then:

```bash
./flow_cutter_cch_order.sh "$OUTPUT_DIR/line_graph"
# → $OUTPUT_DIR/line_graph/perms/cch_perm
```

**Pros:** Full preprocessing (remove multi-arcs, loops, add back-arcs). Quality metrics via `examine_chordal_supergraph`. The line graph is treated as a first-class CSR graph.

**Cons:** Requires materializing the line graph on disk first (~2× the original graph size).

#### Approach B: Script 2 or 3 on the Original Graph

No line graph materialization needed:

```bash
./flow_cutter_cch_cut_order.sh "$GRAPH_DIR"
# → $GRAPH_DIR/perms/cch_perm_cuts

# Then copy to the line graph directory:
cp "$GRAPH_DIR/perms/cch_perm_cuts" "$OUTPUT_DIR/line_graph/perms/cch_perm"
```

Or with the reorder variant for better quality:

```bash
./flow_cutter_cch_cut_reorder.sh "$GRAPH_DIR"
cp "$GRAPH_DIR/perms/cch_perm_cuts_reorder" "$OUTPUT_DIR/line_graph/perms/cch_perm"
```

**Pros:** No need to materialize the line graph. Handles preprocessing internally.

**Cons:** No quality metrics. The internal preprocessing may differ slightly from the shell-level preprocessing in Approach A, potentially producing a different (slightly better or worse) ordering.

### Decision Matrix

| Scenario | Script | Why |
|----------|--------|-----|
| Normal graph CCH | Script 1 | Only option |
| Line graph CCH, want quality metrics | Script 1 on line graph dir | `examine_chordal_supergraph` output |
| Line graph CCH, no line graph on disk | Script 2 or 3 | Avoids materializing line graph |
| Line graph CCH, want best quality | Script 3, or Script 1 on line graph | Reorder pass or full preprocessing |
| Quick line graph CCH | Script 2 | Fastest of the arc-ordering variants |
