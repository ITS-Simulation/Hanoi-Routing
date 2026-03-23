# rust_road_router Engine API Reference

## Purpose

This document is a developer reference for the `rust_road_router` engine crate's
public API surface — the types, functions, and patterns needed to use CCH
(Customizable Contraction Hierarchies) programmatically. It covers the three CCH
phases (Contraction, Customization, Query), the DirectedCCH optimization, data
loading, and re-customization.

> [!NOTE]
> For the high-level pipeline overview (OSM → graph → ordering → CCH → query),
> see [CCH Walkthrough.md](CCH%20Walkthrough.md). This document focuses on
> **code-level API usage** of the engine crate.

---

## 1. Key Types

| Type | Location | Purpose |
|------|----------|---------|
| `Weight` (`u32`) | `engine/src/datastr/graph.rs` | Edge weight in milliseconds |
| `NodeId` (`u32`) | `engine/src/datastr/graph.rs` | Node identifier |
| `EdgeId` (`u32`) | `engine/src/datastr/graph.rs` | Edge identifier |
| `INFINITY` (`u32::MAX / 2`) | `engine/src/datastr/graph.rs:25` | Sentinel for unreachable; half of `u32::MAX` to prevent overflow when summing |
| `FirstOutGraph<F,H,W>` | `engine/src/datastr/graph/first_out_graph.rs:22-30` | CSR graph — generic over container types |
| `OwnedGraph` | Same file | `FirstOutGraph<Vec<EdgeId>, Vec<NodeId>, Vec<Weight>>` |
| `BorrowedGraph<'a>` | Same file | `FirstOutGraph<&'a [EdgeId], &'a [NodeId], &'a [Weight]>` |
| `NodeOrder` | `engine/src/datastr/node_order.rs` | Bidirectional mapping: node ID ↔ rank |
| `Query` | `engine/src/algo/mod.rs:33-36` | `{ from: NodeId, to: NodeId }` |
| `QueryResult<P,W>` | `engine/src/algo/mod.rs:121-128` | Distance + lazy path server |
| `ConnectedQueryResult<P,W>` | `engine/src/algo/mod.rs:179` | Unwrapped result (asserts path exists) |

### Type aliases

`OwnedGraph` and `BorrowedGraph` are the most commonly used graph types:

```rust
// Owns all data — use when loading from disk
type OwnedGraph = FirstOutGraph<Vec<EdgeId>, Vec<NodeId>, Vec<Weight>>;

// Borrows slices — use for temporary views (e.g., passing to customize())
type BorrowedGraph<'a> = FirstOutGraph<&'a [EdgeId], &'a [NodeId], &'a [Weight]>;
```

`FirstOutGraph` implements the CSR (Compressed Sparse Row) representation. The
`first_out` array has length `num_nodes + 1`, where `first_out[v]` is the index
of the first outgoing edge of node `v` and `first_out[v+1] - first_out[v]` is
the out-degree.

---

## 2. Phase 1 — Contraction (Metric-Independent)

**Entry point**: `CCH::fix_order_and_build()` at
`engine/src/algo/customizable_contraction_hierarchy/mod.rs:64-71`

```rust
pub fn fix_order_and_build(
    graph: &(impl LinkIterable<NodeIdT> + EdgeIdGraph),
    order: NodeOrder
) -> Self
```

### What it does (two-pass optimization)

1. Contracts graph with original order (silently) to extract elimination tree
2. Reorders nodes using separator decomposition for cache-friendly parallel
   customization (`reorder_for_seperator_based_customization`)
3. Contracts again with the optimized order

### Produces

`CCH` struct (`mod.rs:31-41`) containing:

- `first_out`, `head`, `tail` — chordal supergraph topology
- `node_order` — rank mapping
- `forward_cch_edge_to_orig_arc` / `backward_cch_edge_to_orig_arc` —
  `Vec<EdgeIdT>`, maps each CCH edge back to the original graph edges it
  represents
- `elimination_tree` — parent pointers for the elimination tree walk
- `inverted` — reversed chordal graph with edge IDs (for triangle enumeration)
- `separator_tree` — nested dissection structure for parallel customization

**Key insight**: `CCH` stores **no weights at all**. It is purely topological.
This is what allows re-customization with different metrics without rebuilding.

### Alternative: `contract()`

`contract()` at `mod.rs:24-26` — single-pass, no separator reordering. Faster to
build but yields slower customization (no separator-based parallelism).

---

## 3. Phase 2 — Customization (Metric-Dependent)

**Entry point**: `customize()` at `customization.rs:21-35`

```rust
pub fn customize<'c, Graph>(
    cch: &'c CCH,
    metric: &Graph
) -> CustomizedBasic<'c, CCH>
where
    Graph: LinkIterGraph + EdgeRandomAccessGraph<Link> + Sync,
```

### Two internal stages

#### Stage A: Respecting (`prepare_weights` at `customization.rs:69-94`)

For each CCH edge, find all corresponding original arcs and take the minimum
weight:

```
for each CCH edge c:
    upward_weights[c]   = min { metric.weight[a] : a ∈ forward_cch_edge_to_orig_arc[c] }
    downward_weights[c] = min { metric.weight[a] : a ∈ backward_cch_edge_to_orig_arc[c] }
```

Parallelized via rayon (unless `cch-disable-par` feature is set).

#### Stage B: Basic customization (`customize_basic` at `customization.rs:119-250`)

Bottom-up triangle relaxation with separator-based parallelization:

```
for each node v in contraction order:
    for each lower triangle (v → low_node → neighbor):
        upward_weights[v→neighbor]   = min(current, downward[v→low] + upward[low→neighbor])
        downward_weights[v→neighbor] = min(current, upward[v→low]   + downward[low→neighbor])
```

Records unpacking information for path reconstruction.

### Produces

`CustomizedBasic<'a, CCH>` (`mod.rs:435-441`):

```rust
pub struct CustomizedBasic<'a, CCH> {
    pub cch: &'a CCH,
    upward: Vec<Weight>,
    downward: Vec<Weight>,
    up_unpacking: Vec<(InRangeOption<EdgeId>, InRangeOption<EdgeId>)>,
    down_unpacking: Vec<(InRangeOption<EdgeId>, InRangeOption<EdgeId>)>,
}
```

This implements the `Customized` trait (`mod.rs:419-432`) which provides:

- `forward_graph()` → `BorrowedGraph` (CCH topology + upward weights)
- `backward_graph()` → `BorrowedGraph` (CCH topology + downward weights)
- `cch()` → reference to the CCH
- `unpack_outgoing()` / `unpack_incoming()` → shortcut expansion

---

## 4. Phase 3 — Query (Bidirectional Elimination Tree Walk)

**Entry point**: `Server::query()` at `query.rs:202-208`

```rust
impl<C: Customized> QueryServer for Server<C> {
    fn query(&mut self, query: Query) -> QueryResult<Self::P<'_>, Weight>;
}
```

### Algorithm (`Server::distance()` at `query.rs:44-129`)

1. Map original node IDs to CCH ranks
2. Create two `EliminationTreeWalk` instances (forward from source, backward
   from target)
3. Both walks proceed upward in the elimination tree simultaneously
4. When both reach the same node, check if `fw_dist[node] + bw_dist[node]`
   improves tentative distance
5. Track meeting node for path reconstruction

### Path reconstruction (`Server::path()` at `query.rs:131-156`)

1. Trace parent pointers from meeting node back to source (forward parents)
2. Reverse the chain into backward parents
3. Unpack all shortcuts recursively via `unpack_path()` (`query.rs:158-176`)
4. Convert all ranks back to original node IDs

### Result API

```rust
let result = server.query(Query { from, to });

// Option 1: check if connected, then access
if let Some(mut connected) = result.found() {
    let distance: Weight = connected.distance();      // u32, milliseconds
    let path: Vec<NodeId> = connected.node_path();    // lazily computed
}

// Option 2: access directly (None if disconnected)
let distance: Option<Weight> = result.distance();
```

### Performance stats

Available via `PathServerWrapper`:

- `num_nodes_in_searchspace()` — nodes visited during the two walks
- `num_relaxed_edges()` — edges relaxed during the two walks

---

## 5. DirectedCCH — Optimized Variant for Line Graphs

`CCH::to_directed_cch()` at `mod.rs:161-219` creates a `DirectedCCH` by
identifying edges that are always INFINITY (unreachable in every metric) and
removing them. The comment at `mod.rs:159-160` states:

> Transform into a directed CCH which is more efficient for turn expanded
> graphs because many edges can be removed.

This is relevant because line graphs (turn-expanded) are highly directional —
many CCH edges can only be traversed in one direction. Using `DirectedCCH`
reduces the number of edges that need to be processed during customization.
**For line graphs, `DirectedCCH` is the recommended default**, not an optional
optimization.

### Flow for directed variant

```rust
let cch = CCH::fix_order_and_build(&line_graph, order);
let directed_cch = cch.to_directed_cch();   // prune unreachable edges
let customized = customize_directed(&directed_cch, &line_graph);
let mut server = Server::new(customized);
```

`customize_directed()` at `customization.rs:38-51` works like `customize()` but
with separate forward/backward edge arrays. The `customize_directed_basic()` at
`customization/directed.rs:3-172` performs the same triangle relaxation but
tracks upward and downward edges independently.

### Type system implication

`Server<CustomizedBasic<'a, CCH>>` and `Server<CustomizedBasic<'a, DirectedCCH>>`
are **different concrete types**. Code that needs to work with both variants must
use generics, an enum wrapper, or separate types — they cannot be held in a
single non-generic field.

---

## 6. Re-Customization (Dynamic Weight Updates)

The CCH is reusable across metrics. To update weights:

```rust
// Modify the weight vector
let mut new_weights = travel_time.clone();
new_weights[edge_idx] = new_weight;

// Create new graph and re-customize (reuses CCH topology)
let new_graph = FirstOutGraph::new(&first_out[..], &head[..], new_weights);
let new_customized = customize(&cch, &new_graph);
server.update(new_customized);  // atomic swap
```

### Critical detail: baseline cloning

Re-customization starts from a **clone** of the original weight vector. Updates
are NOT cumulative across calls — each customization starts fresh from the
baseline. This pattern is used in the existing server (`server/src/main.rs:309-330`).

If you want cumulative updates, maintain a mutable weight vector externally and
pass it each time.

### `Server::update()` swap pattern

`Server::update()` at `query.rs:36-38` uses `std::mem::swap` to replace the
internal `CustomizedBasic`. The old customized data is dropped, and the new one
takes its place — both borrow the same `&CCH`, so this is lifetime-safe.

---

## 7. Data Loading (I/O)

All graph files are headerless raw binary vectors loaded via the `Load` trait
(`engine/src/io.rs:104-122`):

```rust
use rust_road_router::io::*;

let first_out: Vec<EdgeId> = Vec::load_from(path.join("first_out"))?;
let head: Vec<NodeId>      = Vec::load_from(path.join("head"))?;
let travel_time: Vec<Weight> = Vec::load_from(path.join("travel_time"))?;
```

The `Load` trait works by:

1. Reading file metadata to get byte count
2. Allocating `Vec<T>` of size `num_bytes / size_of::<T>()`
3. Reading raw bytes directly into the vector

### Loading a NodeOrder

`NodeOrder` is constructed from the permutation vector:

```rust
let cch_order = NodeOrder::from_node_order(
    Vec::<NodeId>::load_from(path.join("perms/cch_perm"))?
);
```

### Constructing a graph from loaded data

```rust
// BorrowedGraph — borrows existing slices, zero-copy
let graph = FirstOutGraph::new(&first_out[..], &head[..], &travel_time[..]);

// OwnedGraph — takes ownership of vectors
let graph = FirstOutGraph::new(first_out, head, travel_time);
```

`FirstOutGraph::new()` accepts any combination of owned vectors and borrowed
slices thanks to its generic type parameters `<F, H, W>`.

---

## 8. Putting It All Together

### Minimal example: load → build → customize → query

```rust
use rust_road_router::algo::customizable_contraction_hierarchy::*;
use rust_road_router::algo::{Query, QueryServer};
use rust_road_router::datastr::graph::*;
use rust_road_router::datastr::node_order::NodeOrder;
use rust_road_router::io::*;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new("Maps/data/hanoi_car/graph");

    // Load graph data
    let first_out: Vec<EdgeId> = Vec::load_from(dir.join("first_out"))?;
    let head: Vec<NodeId> = Vec::load_from(dir.join("head"))?;
    let travel_time: Vec<Weight> = Vec::load_from(dir.join("travel_time"))?;

    // Build graph view
    let graph = FirstOutGraph::new(&first_out[..], &head[..], &travel_time[..]);

    // Load node ordering and build CCH (Phase 1)
    let order = NodeOrder::from_node_order(
        Vec::<NodeId>::load_from(dir.join("perms/cch_perm"))?
    );
    let cch = CCH::fix_order_and_build(&graph, order);

    // Customize with travel_time metric (Phase 2)
    let customized = customize(&cch, &graph);

    // Create query server and run queries (Phase 3)
    let mut server = Server::new(customized);
    let result = server.query(Query { from: 0, to: 100 });

    if let Some(mut connected) = result.found() {
        println!("Distance: {} ms", connected.distance());
        println!("Path: {:?}", connected.node_path());
    } else {
        println!("No path found");
    }

    Ok(())
}
```

### Line graph with DirectedCCH

```rust
let lg_dir = Path::new("Maps/data/hanoi_car/line_graph");

let first_out: Vec<EdgeId> = Vec::load_from(lg_dir.join("first_out"))?;
let head: Vec<NodeId> = Vec::load_from(lg_dir.join("head"))?;
let travel_time: Vec<Weight> = Vec::load_from(lg_dir.join("travel_time"))?;

let lg = FirstOutGraph::new(&first_out[..], &head[..], &travel_time[..]);

let order = NodeOrder::from_node_order(
    Vec::<NodeId>::load_from(lg_dir.join("perms/cch_perm"))?
);

// Build CCH, then convert to DirectedCCH for efficiency
let cch = CCH::fix_order_and_build(&lg, order);
let directed_cch = cch.to_directed_cch();
let customized = customize_directed(&directed_cch, &lg);
let mut server = Server::new(customized);

// Line graph nodes = original graph edges
// Query: "route from original edge 42 to original edge 99"
let result = server.query(Query { from: 42, to: 99 });

if let Some(mut connected) = result.found() {
    // Add final edge cost (line graph doesn't include it)
    let original_travel_time: Vec<Weight> =
        Vec::load_from(Path::new("Maps/data/hanoi_car/graph/travel_time"))?;
    let total = connected.distance() + original_travel_time[99];
    println!("Total distance: {} ms", total);
}
```
