# Graph Weight Format & Test Weight Generation Guide

This document explains the RoutingKit binary graph format used by both the **normal graph** and the **turn-expanded graph (line graph)**, and provides guidelines for generating fixed test weights.

---

## 1. Binary File Format

All three files are **headerless raw binary vectors** of `u32` (4 bytes each, little-endian on x86). No length prefix, no metadata — file size alone determines element count.


| File          | Element type | Length  | Description                                      |
| ------------- | ------------ | ------- | ------------------------------------------------ |
| `first_out`   | `u32`        | `n + 1` | CSR row-pointer array (`n` = number of nodes)    |
| `head`        | `u32`        | `m`     | Target node ID for each directed edge            |
| `travel_time` | `u32`        | `m`     | Weight for each directed edge (**milliseconds**) |


**Invariants** (enforced by `FirstOutGraph::new()` in `engine/src/datastr/graph/first_out_graph.rs:52-57`):

- `first_out[0] == 0`
- `first_out[n] == m` (last element equals total number of edges)
- `head.len() == travel_time.len() == m`
- `first_out` is monotonically non-decreasing

---

## 2. CSR (Compressed Sparse Row) Structure

### 2.1 The problem CSR solves

A road network with **n** nodes and **m** directed edges could be stored as an adjacency list (array of vectors), but that scatters memory across heap allocations. CSR packs everything into **three flat arrays**, giving you cache-friendly, O(1)-indexed access to any node's outgoing edges.

### 2.2 The three arrays


| Array         | Length  | Role                                                                                       |
| ------------- | ------- | ------------------------------------------------------------------------------------------ |
| `first_out`   | `n + 1` | **Index pointer** — tells you *where* in `head`/`travel_time` a node's edges begin and end |
| `head`        | `m`     | **Edge targets** — the destination node of each edge                                       |
| `travel_time` | `m`     | **Edge weights** — cost of each edge (milliseconds)                                        |


### 2.3 How it works

Think of `head` and `travel_time` as one long list of **all edges in the graph**, sorted by source node. Edges from node 0 come first, then edges from node 1, then node 2, etc.

`first_out` is the **table of contents** — it tells you where each node's section starts.

```
To read all outgoing edges of node v:

  start = first_out[v]       ← index of v's first edge
  end   = first_out[v + 1]   ← index of (v+1)'s first edge = one past v's last edge

  for i in start..end:
      target = head[i]
      cost   = travel_time[i]
      // edge: v → target, with weight cost
```

**Why `n + 1` entries?** The extra entry (`first_out[n]`) serves as the sentinel — it equals `m` (total edges), so the last node's range `first_out[n-1]..first_out[n]` works without special-casing.

### 2.4 Concrete walkthrough

Consider a 4-node graph:

```
    0 ──10s──▶ 1 ──5s──▶ 3
    │                     ▲
    └──20s──▶ 2 ──8s─────┘
```

**Edges (sorted by source):**


| Edge index | Source → Target | Weight    |
| ---------- | --------------- | --------- |
| 0          | 0 → 1           | 10,000 ms |
| 1          | 0 → 2           | 20,000 ms |
| 2          | 1 → 3           | 5,000 ms  |
| 3          | 2 → 3           | 8,000 ms  |


**The arrays:**

```
Index:        0    1    2    3    4
first_out = [ 0,   2,   3,   4,   4 ]
              ↑    ↑    ↑    ↑    ↑
            node0 node1 node2 node3 sentinel
              │    │    │    │
              │    │    │    └─ node 3's edges: [4..4) → empty (no outgoing)
              │    │    └────── node 2's edges: [3..4) → edge index 3
              │    └─────────── node 1's edges: [2..3) → edge index 2
              └──────────────── node 0's edges: [0..2) → edge indices 0, 1

Index:          0    1    2    3
head =        [ 1,   2,   3,   3 ]
travel_time = [ 10000, 20000, 5000, 8000 ]
```

**Reading node 0:**

- `first_out[0] = 0`, `first_out[1] = 2` → range `[0, 2)` → indices 0 and 1
- Edge 0: `head[0] = 1`, `travel_time[0] = 10000` → **0 → 1, 10 seconds**
- Edge 1: `head[1] = 2`, `travel_time[1] = 20000` → **0 → 2, 20 seconds**

**Reading node 3:**

- `first_out[3] = 4`, `first_out[4] = 4` → range `[4, 4)` → **empty** (no outgoing edges)

### 2.5 Key invariants

1. `**first_out[0] == 0`** — edges start at index 0
2. `**first_out[n] == m**` — sentinel equals total edge count
3. **Monotonically non-decreasing** — `first_out[v] <= first_out[v+1]` for all `v` (a node can have zero edges, meaning consecutive equal values, but never decreasing)
4. `**len(head) == len(travel_time) == m`** — every edge has exactly one target and one weight

### 2.6 The reverse lookup problem

CSR makes it fast to go **node → edges** but going **edge → source node** requires a reverse lookup. Given edge index `e`, you need to find which node `v` satisfies `first_out[v] <= e < first_out[v+1]`. This is a binary search on `first_out`, or more commonly pre-computed into a `tail` array:

```
tail[e] = v   such that   first_out[v] <= e < first_out[v+1]
```

The codebase builds this in `generate_line_graph.rs:126-131` because the line graph construction needs to know the source of every edge.

### 2.7 Why CSR for routing?

- **Memory**: Just `(n+1 + 2m)` integers — no pointers, no per-node allocations
- **Cache locality**: Scanning a node's neighbors reads a contiguous memory slice
- **Disk format**: The binary files are literally these arrays dumped raw — `mmap` or `read` them directly into memory with zero parsing
- **Dijkstra performance**: The priority queue pops a node, then iterates `first_out[v]..first_out[v+1]` — that's a tight, branch-free inner loop over contiguous memory

---

## 3. Weight Unit: Milliseconds

All persisted `travel_time` files use **milliseconds** (1 second = 1000 units).

**Authority**: the `tt_units_per_s` metadata file, which pipelines write as `[1000]`.

**From the OSM pipeline** (`CCH-Generator/src/generate_graph.cpp:201-203`):

```cpp
out.travel_time[a] *= 18000;  // geo_distance is in meters
out.travel_time[a] /= speed;  // speed is in km/h
out.travel_time[a] /= 5;
// Derivation: (meters / (km_h / 3.6)) * 1000 = (meters * 18000) / km_h / 5
```

**From the HERE pipeline** (`conversion/src/here/mod.rs:300`):

```rust
let weight = (1000.0 * length / speed_m_per_s).round() as Weight;
```

---

## 4. Normal Graph: Edge-to-Weight Mapping

The mapping is **positional** (1-to-1 by index):

```
Edge ID e  →  target = head[e],  weight = travel_time[e]
```

To find which node an edge originates from, you must search `first_out`:

```
source(e) = v  where  first_out[v] <= e < first_out[v+1]
```

This is typically precomputed into a `tail` array (see `generate_line_graph.rs:126-131`).

---

## 5. Line Graph (Turn-Expanded Graph): Structure and Weight Mapping

### 5.1 Node mapping

Each **original edge becomes a line-graph node**:

```
Line-graph node i  ↔  Original edge i
```

- `line_graph.num_nodes() == original_graph.num_arcs()`
- Line-graph node `i` has coordinates of the **tail** of original edge `i`

### 5.2 Edge creation (turns)

A line-graph edge from node `e1` to node `e2` exists if and only if:

1. **Consecutive**: `head[e1] == tail[e2]` (the two original edges share an intermediate node)
2. **Not a forbidden turn**: `(e1, e2)` is not in `forbidden_turn_from_arc / forbidden_turn_to_arc`
3. **Not a U-turn**: `tail[e1] != head[e2]` (you don't go back where you came from)

### 5.3 Weight formula

From `engine/src/datastr/graph.rs:193`:

```rust
weight.push(link.weight + turn_cost);
```

Where `turn_cost` is the callback return value. In the standard pipeline (`generate_line_graph.rs:159`), the callback returns `Some(0)` for all allowed turns.

**Therefore**:

```
line_graph.travel_time[turn_edge] = original.travel_time[e1] + 0
                                  = travel_time of the FIRST original edge in the turn
```

### 5.4 Why this is correct for path costs

When routing finds a path through the line graph `[n_0, n_1, n_2, ..., n_k]`, the total cost is:

```
total = line_graph.travel_time[edge(n_0→n_1)]    ← = original.travel_time[n_0]
      + line_graph.travel_time[edge(n_1→n_2)]    ← = original.travel_time[n_1]
      + ...
      + line_graph.travel_time[edge(n_{k-1}→n_k)] ← = original.travel_time[n_{k-1}]
```

Every original edge's travel time is counted **exactly once** — it's charged when you "leave" that edge (transition to the next one). The only original edge **not charged** in the sum above is `n_k` (the final edge) — its cost must be added separately at the end of the query, or the routing algorithm must account for it.

> **Important nuance**: The last edge in a line-graph path (`n_k`) has its travel_time baked into any outgoing line-graph edge but if `n_k` is the destination, no such edge is traversed. Routing implementations typically handle this by adding `original.travel_time[n_k]` to the final result, or by treating the destination lookup accordingly.

---

## 6. CCH Customization: How Weights Flow from Disk to Query

### 6.1 The two-phase architecture

The CCH separates **structure** (metric-independent) from **weights** (metric-dependent):


| Phase                      | What it produces                                                                   | When it runs              | Contains weights? |
| -------------------------- | ---------------------------------------------------------------------------------- | ------------------------- | ----------------- |
| **Phase 1: Contraction**   | `CCH` struct — hierarchy topology, elimination tree, edge-to-original-arc mappings | Once at startup           | **No**            |
| **Phase 2: Customization** | `CustomizedBasic` — `upward_weights[]` + `downward_weights[]` on the CCH edges     | Every time weights change | **Yes**           |


The `CCH` struct stores **no weights at all**. It only stores structure: `first_out`, `head`, `tail`, `elimination_tree`, and critically, `forward_cch_edge_to_orig_arc` / `backward_cch_edge_to_orig_arc` — the mappings from each CCH edge back to the original graph edges it represents.

### 6.2 How customization paints weights onto the CCH

The `customize()` function (`engine/src/algo/customizable_contraction_hierarchy/customization.rs:21-35`) takes a CCH structure and a weighted graph, and produces a `CustomizedBasic`:

```
Step 1 — Initialize:
    upward_weights   = [INFINITY; num_cch_arcs]
    downward_weights = [INFINITY; num_cch_arcs]

Step 2 — Respecting phase (prepare_weights):
    For each CCH edge c:
        For each original arc a that maps to c:
            upward_weights[c]   = min(upward_weights[c],   metric.weight[a])
            downward_weights[c] = min(downward_weights[c], metric.weight[a])

Step 3 — Basic customization (customize_basic):
    Bottom-up triangle enumeration on the CCH:
        For each shortcut edge (u, w) via intermediate node v:
            upward_weights[u→w] = min(upward_weights[u→w],
                                      downward_weights[u→v] + upward_weights[v→w])
```

**Key detail**: `INFINITY = u32::MAX / 2 = 2,147,483,647` (defined at `engine/src/datastr/graph.rs:25`). This is intentionally half of `u32::MAX` so that adding two INFINITY values during shortcut creation doesn't overflow.

The `min()` in the respecting phase handles **parallel edges** — when multiple original arcs map to the same CCH edge, the cheapest one wins.

### 6.3 How the server applies custom weights

When the `/customize` endpoint receives updates (`server/src/main.rs:309-330`):

```
1. Clone the original OSM travel_time vector (never mutate the original)
2. For each (here_link_id, direction, new_weight) in the update:
       travel_time[local_edge_idx] = new_weight
3. Build a new graph: FirstOutGraph::new(first_out, head, modified_travel_time)
4. Run full CCH customization on this graph
5. Atomically swap the new CustomizedBasic into the query server
```

**Three critical implications**:

1. **The on-disk `travel_time` file is never modified.** The original in-memory vector is never mutated — it's always cloned first.
2. **Each `/customize` call starts fresh from the original OSM weights.** Updates are NOT cumulative across calls. If you call `/customize` with edge A updated, then call it again with edge B updated, the second call will have edge A back at its original OSM value.
3. **Edges not included in the update keep their OSM `travel_time` automatically.** Since the process starts by cloning the full original vector, any edge index you don't overwrite retains its original value.

### 6.4 Visual flow

```
OSM travel_time (on disk):  [1000, 2000, 3000, 5000, 8000]     never changes
                                  │
                                  │ loaded once at startup
                                  ▼
Original in-memory vector:  [1000, 2000, 3000, 5000, 8000]     never mutated
                                  │
                                  │ clone per /customize call
                                  ▼
Cloned vector:              [1000, 2000, 3000, 5000, 8000]
                                  │
                                  │ apply updates (e.g., edge 2 → 9999)
                                  ▼
Modified vector:            [1000, 2000, 9999, 5000, 8000]     edge 0,1,3,4 = OSM default
                                  │
                                  │ CCH customize()
                                  ▼
CustomizedBasic:            upward_weights[...]                 what queries actually use
                            downward_weights[...]
                                  │
                                  │ atomic swap
                                  ▼
Query server:               uses new CustomizedBasic for all subsequent queries
```

### 6.5 What queries actually read

Queries never touch the original `travel_time` array. They exclusively use the `CustomizedBasic`'s `upward_weights` and `downward_weights` via the elimination tree search:

```rust
// Bidirectional search up the elimination tree
let fw_graph = customized.forward_graph();   // → upward_weights
let bw_graph = customized.backward_graph();  // → downward_weights
// Edge relaxation reads from these weight vectors
```

The `CustomizedBasic` contains a complete, self-consistent set of shortcut weights that encodes shortest paths through the entire hierarchy. It doesn't matter whether the input was pure OSM weights or a mix of OSM + custom — after customization, the result is a unified weight structure.

### 6.6 Implications for test weight generation


| Scenario                                                       | What to do                                                                                            |
| -------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| **Test the full pipeline** (OSM → customize → query)           | Write test `travel_time` to the graph directory, then let the server load and customize it at startup |
| **Test live weight updates**                                   | Start the server with any `travel_time`, then POST to `/customize` with your test weights             |
| **Test customization correctness**                             | Call `customize()` directly with a `FirstOutGraph` containing your test weights                       |
| **Bypass customization** (test raw Dijkstra on the flat graph) | Use the `travel_time` array directly — it maps 1-to-1 to edges as described in Sections 2–5           |


**Important**: When testing via `/customize`, remember that updates use HERE link IDs (not raw edge indices) and require the `id_mapper` to translate. For direct programmatic testing, you can call `customize()` with raw edge-indexed weights, skipping the HERE ID mapping entirely.

---

## 7. Guidelines for Generating Fixed Test Weights

### 7.1 Normal graph test weights

To generate a valid `travel_time` file for a normal graph:

1. **Read `head`** to determine `m` (number of edges): `m = file_size(head) / 4`
2. **Allocate** a `Vec<u32>` of length `m`
3. **Assign weights** using your test strategy (see below)
4. **Write** as raw `u32` binary (no header)

**Strategies**:


| Strategy       | Formula                                                | Use case                                    |
| -------------- | ------------------------------------------------------ | ------------------------------------------- |
| Uniform        | `travel_time[e] = C` for all `e`                       | Verifying hop-count behavior                |
| Sequential     | `travel_time[e] = (e + 1) * 1000`                      | Every edge has unique weight; easy to trace |
| Distance-based | `travel_time[e] = haversine(tail[e], head[e]) * scale` | Realistic weights from coordinates          |
| Random bounded | `travel_time[e] = rand(1000..60000)`                   | Stress-testing with varied weights          |
| Known-path     | Set specific edges low, others high                    | Force a known shortest path for validation  |


**Constraints to respect**:

- **Type**: `u32`, so `0 ≤ weight ≤ 4,294,967,295`
- **Avoid zero weights** unless intentional — zero-weight edges can cause algorithmic edge cases (infinite loops in some Dijkstra variants if not guarded)
- **Avoid overflow**: For CCH/CH customization, shortcut weights are sums of two edge weights. Keep individual weights below `u32::MAX / 2` (~2.1 billion) to prevent overflow. In practice, realistic weights (< 10,000,000 ms = ~2.7 hours per edge) are well within safe range.
- **Unit consistency**: Always milliseconds. 1 second = `1000`, 1 minute = `60000`, 1 hour = `3600000`.

### 7.2 Line graph test weights

You have **two options**:

#### Option A: Generate the line graph from a normal graph with test weights

1. Write your test `travel_time` into the normal graph directory
2. Run `generate_line_graph` — it will produce the line graph with weights derived from your normal graph via the formula `line_travel_time = original_travel_time[e1]`
3. **Advantage**: Guarantees structural consistency between the two graphs

#### Option B: Write line graph weights directly

1. **Read the line graph's `head`** to determine `m_lg` (number of line-graph edges)
2. **Allocate** a `Vec<u32>` of length `m_lg`
3. **Assign weights** per your strategy
4. **Write** as raw `u32` binary

**Critical consistency rule for Option B**: If you plan to compare normal-graph and line-graph routing results, the line graph weight for a turn `(e1 → e2)` **must** equal `original.travel_time[e1]` (the first edge's weight). Breaking this invariant means the two graphs will compute different shortest paths.

### 7.3 Generating the binary files (Rust example)

```rust
use std::fs::File;
use std::io::Write;

fn write_u32_vec(path: &str, data: &[u32]) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    let bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            data.as_ptr() as *const u8,
            data.len() * std::mem::size_of::<u32>(),
        )
    };
    file.write_all(bytes)
}

// Example: uniform 10-second weights for a graph with m edges
fn generate_uniform_weights(graph_dir: &str, m: usize) {
    let weights: Vec<u32> = vec![10_000; m];  // 10 seconds per edge
    write_u32_vec(&format!("{}/travel_time", graph_dir), &weights).unwrap();
}
```

### 7.4 Generating the binary files (Python example)

```python
import numpy as np
import os

def read_u32_vec(path: str) -> np.ndarray:
    return np.fromfile(path, dtype=np.uint32)

def write_u32_vec(path: str, data: np.ndarray):
    data.astype(np.uint32).tofile(path)

def generate_test_weights(graph_dir: str, strategy: str = "uniform"):
    head = read_u32_vec(os.path.join(graph_dir, "head"))
    m = len(head)

    if strategy == "uniform":
        weights = np.full(m, 10_000, dtype=np.uint32)       # 10s per edge
    elif strategy == "sequential":
        weights = (np.arange(1, m + 1) * 1000).astype(np.uint32)
    elif strategy == "random":
        rng = np.random.default_rng(seed=42)
        weights = rng.integers(1000, 60_000, size=m, dtype=np.uint32)
    elif strategy == "known_path":
        weights = np.full(m, 100_000, dtype=np.uint32)       # default: high
        # Set your known shortest-path edges to low weight:
        # weights[edge_id_1] = 1000
        # weights[edge_id_2] = 1000
        # ...
    else:
        raise ValueError(f"Unknown strategy: {strategy}")

    write_u32_vec(os.path.join(graph_dir, "travel_time"), weights)
    print(f"Wrote {m} weights ({strategy}) to {graph_dir}/travel_time")

# Usage:
# generate_test_weights("/path/to/normal_graph", "uniform")
# generate_test_weights("/path/to/line_graph", "uniform")
```

### 7.5 Validation checklist

After generating test weights, verify:

- `file_size(travel_time) == file_size(head)` (same number of u32 elements)
- `file_size(travel_time) % 4 == 0` (valid u32 alignment)
- All weights > 0 (unless zero-weight is intentional)
- No individual weight > `u32::MAX / 2` (overflow safety for CH shortcuts)
- If testing both graphs: `line_graph_weight[turn] == normal_graph_weight[e1]` for consistency
- `tt_units_per_s` file exists and contains `[1000_u32]` (single-element binary vector)

### 7.6 Recommended test scenario: "known shortest path"

The most useful test strategy is to construct weights where you **know** the answer:

1. **Pick a source and target node** in the graph
2. **Find a path** between them (e.g., via BFS on the unweighted graph)
3. **Set all edges on that path** to a low weight (e.g., `1000` ms)
4. **Set all other edges** to a high weight (e.g., `1000000` ms)
5. **Expected result**: shortest path cost = `path_length * 1000` ms
6. **For the line graph**: after running `generate_line_graph`, the line graph will inherit these weights automatically. The expected line-graph shortest path cost = `(path_length - 1) * 1000` ms (the last edge is not counted in intermediate hops; add it separately if your query does so).

This lets you verify that the routing algorithm finds the correct path and computes the correct cost, for both the normal and turn-expanded graph.