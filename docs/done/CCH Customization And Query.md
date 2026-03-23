# Implementation Plan: CCH Customization & Query Integration

**Date**: 2026-03-17
**Scope**: Load graph data from `Maps/data/`, build CCH, customize with
weights, and perform shortest-path queries — for both the normal graph and the
line graph.

> **[AUDIT 2026-03-17]** — This plan has been audited against the actual
> codebase. All 22 engine API references (types, signatures, line numbers) are
> **verified correct**. Amendments are marked with `[AUDIT]` tags throughout.
> Summary: 2 factual errors fixed, 4 design gaps noted, 2 conformance issues
> with the CCH-Hanoi Hub walkthrough addressed.

---

## 1. Background & Motivation

We have fully prepared graph data for the Hanoi road network:

```
Maps/data/hanoi_car/
├── graph/
│   ├── first_out, head, travel_time       ← normal graph (CSR)
│   ├── latitude, longitude                ← node coordinates
│   ├── geo_distance, way                  ← edge metadata
│   ├── forbidden_turn_from_arc/to_arc     ← turn restrictions
│   └── perms/cch_perm                     ← CCH node ordering (from IFC)
├── line_graph/
│   ├── first_out, head, travel_time       ← turn-expanded graph (CSR)
│   ├── latitude, longitude                ← node coordinates (of edge tails)
│   └── perms/cch_perm                     ← CCH node ordering (from IFC)
└── conditional_turns/                     ← time-window turn restrictions
```

> **[AUDIT]** Added `line_graph/perms/cch_perm` to the tree — the line graph
> will also have its CCH ordering after the pipeline runs (see Section 5.1
> update).

The `rust_road_router` engine provides the complete CCH algorithm (contraction,
customization, query) as a library. The existing HTTP `server` crate wraps this
but is **tightly coupled to HERE data** (link IDs, direction booleans, rank
mappings), making it unsuitable for our OSM-pipeline data. We need a clean
integration layer that works directly with node IDs and edge indices.

---

## 2. Analysis of rust_road_router Engine API

### 2.1 Key types


| Type                        | Location                                            | Purpose                                                                       |
| --------------------------- | --------------------------------------------------- | ----------------------------------------------------------------------------- |
| `Weight` (`u32`)            | `engine/src/datastr/graph.rs`                       | Edge weight in milliseconds                                                   |
| `NodeId` (`u32`)            | `engine/src/datastr/graph.rs`                       | Node identifier                                                               |
| `EdgeId` (`u32`)            | `engine/src/datastr/graph.rs`                       | Edge identifier                                                               |
| `INFINITY` (`u32::MAX / 2`) | `engine/src/datastr/graph.rs:25`                    | Sentinel for unreachable; half of `u32::MAX` to prevent overflow when summing |
| `FirstOutGraph<F,H,W>`      | `engine/src/datastr/graph/first_out_graph.rs:22-30` | CSR graph — generic over container types                                      |
| `OwnedGraph`                | Same file                                           | `FirstOutGraph<Vec<EdgeId>, Vec<NodeId>, Vec<Weight>>`                        |
| `BorrowedGraph<'a>`         | Same file                                           | `FirstOutGraph<&'a [EdgeId], &'a [NodeId], &'a [Weight]>`                     |
| `NodeOrder`                 | `engine/src/datastr/node_order.rs`                  | Bidirectional mapping: node ID ↔ rank                                         |
| `Query`                     | `engine/src/algo/mod.rs:33-36`                      | `{ from: NodeId, to: NodeId }`                                                |
| `QueryResult<P,W>`          | `engine/src/algo/mod.rs:121-128`                    | Distance + lazy path server                                                   |
| `ConnectedQueryResult<P,W>` | `engine/src/algo/mod.rs:179`                        | Unwrapped result (asserts path exists)                                        |


### 2.2 Phase 1 — Contraction (metric-independent)

**Entry point**: `CCH::fix_order_and_build()` at
`engine/src/algo/customizable_contraction_hierarchy/mod.rs:64-71`

```rust
pub fn fix_order_and_build(
    graph: &(impl LinkIterable<NodeIdT> + EdgeIdGraph),
    order: NodeOrder
) -> Self
```

**What it does** (two-pass optimization):

1. Contracts graph with original order (silently) to extract elimination tree
2. Reorders nodes using separator decomposition for cache-friendly parallel
  customization (`reorder_for_seperator_based_customization`)
3. Contracts again with the optimized order

**Produces** `CCH` struct (`mod.rs:31-41`) containing:

- `first_out`, `head`, `tail` — chordal supergraph topology
- `node_order` — rank mapping
- `forward_cch_edge_to_orig_arc` / `backward_cch_edge_to_orig_arc` — `Vecs<EdgeIdT>`, maps each CCH edge back to the original graph edges it represents
- `elimination_tree` — parent pointers for the elimination tree walk
- `inverted` — reversed chordal graph with edge IDs (for triangle enumeration)
- `separator_tree` — nested dissection structure for parallel customization

**Key insight**: `CCH` stores **no weights at all**. It is purely topological.
This is what allows re-customization with different metrics without rebuilding.

**Alternative**: `contract()` at `mod.rs:24-26` — single-pass, no separator
reordering. Faster but yields slower customization.

### 2.3 Phase 2 — Customization (metric-dependent)

**Entry point**: `customize()` at `customization.rs:21-35`

```rust
pub fn customize<'c, Graph>(
    cch: &'c CCH,
    metric: &Graph
) -> CustomizedBasic<'c, CCH>
where
    Graph: LinkIterGraph + EdgeRandomAccessGraph<Link> + Sync,
```

**Two internal stages**:

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

**Produces** `CustomizedBasic<'a, CCH>` (`mod.rs:435-441`):

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

### 2.4 Phase 3 — Query (bidirectional elimination tree walk)

**Entry point**: `Server::query()` at `query.rs:202-208`

```rust
impl<C: Customized> QueryServer for Server<C> {
    fn query(&mut self, query: Query) -> QueryResult<Self::P<'_>, Weight>;
}
```

**Algorithm** (`Server::distance()` at `query.rs:44-129`):

1. Map original node IDs to CCH ranks
2. Create two `EliminationTreeWalk` instances (forward from source, backward
  from target)
3. Both walks proceed upward in the elimination tree simultaneously
4. When both reach the same node, check if `fw_dist[node] + bw_dist[node]`
  improves tentative distance
5. Track meeting node for path reconstruction

**Path reconstruction** (`Server::path()` at `query.rs:131-156`):

1. Trace parent pointers from meeting node back to source (forward parents)
2. Reverse the chain into backward parents
3. Unpack all shortcuts recursively via `unpack_path()` (`query.rs:158-176`)
4. Convert all ranks back to original node IDs

**Result API**:

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

**Performance stats**: Available via `PathServerWrapper`:

- `num_nodes_in_searchspace()` — nodes visited during the two walks
- `num_relaxed_edges()` — edges relaxed during the two walks

### 2.5 DirectedCCH — optimized variant for line graphs

`CCH::to_directed_cch()` at `mod.rs:161-219` creates a `DirectedCCH` by
identifying edges that are always INFINITY (unreachable in every metric) and
removing them. The comment at `mod.rs:159-160` states:

> Transform into a directed CCH which is more efficient for turn expanded
> graphs because many edges can be removed.

This is relevant because line graphs (turn-expanded) are highly directional —
many CCH edges can only be traversed in one direction. Using `DirectedCCH`
reduces the number of edges that need to be processed during customization.

**Flow for directed variant**:

```rust
let cch = CCH::fix_order_and_build(&line_graph, order);
let directed_cch = cch.to_directed_cch();
let customized = customize_directed(&directed_cch, &line_graph);
let mut server = Server::new(customized);
```

`customize_directed()` at `customization.rs:38-51` works like `customize()` but
with separate forward/backward edge arrays. The `customize_directed_basic()` at
`customization/directed.rs:3-172` performs the same triangle relaxation but
tracks upward and downward edges independently.

### 2.6 Re-customization (dynamic weight updates)

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

**Critical detail from the existing server** (`server/src/main.rs:309-330`):
Re-customization starts from a **clone** of the original weight vector. Updates
are NOT cumulative across calls — each customization starts fresh from the
baseline.

### 2.7 Data loading (I/O)

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

`NodeOrder` is constructed from the permutation vector:

```rust
let cch_order = NodeOrder::from_node_order(
    Vec::<NodeId>::load_from(path.join("perms/cch_perm"))?
);
```

---

## 3. Problems with the Existing Server

The HTTP server at `server/src/main.rs` has four API mismatches for our use case:

### 3.1 HERE-only customization

`/customize` accepts `Vec<(u64, bool, SerializedWeight)>` — HERE link IDs with
direction booleans. It requires `link_id_mapping` and `here_rank_to_link_id`
files (`main.rs:200-202`) that our OSM pipeline does not produce. There is no
way to customize by raw edge index.

> **[DECIDED]** Build a new server with a `**POST /customize` REST endpoint**
> (Axum, separate port) that accepts a raw binary weight vector
> (`application/octet-stream`, length = `num_edges × 4`). No HERE mapping,
> no JSON, no edge indices — the position in the vector *is* the edge index.
> See Section 4.5.3 for the endpoint definition and Section 4.5.5 for
> stale-update cancellation.
>
> **[UPDATED]** Changed from gRPC to **REST with raw binary body**. The only
> consumer is the internal live traffic module (same network,
> machine-to-machine). A plain HTTP POST with `application/octet-stream`
> achieves identical wire efficiency (~8 MB) without requiring protobuf
> tooling (tonic, prost, protoc). The full-vector format removes the
> server-side clone-and-merge step. Separate port ensures customization
> traffic cannot destabilize the query API.

### 3.2 HERE-only query endpoint

`/here_query` uses HERE link IDs for source/target. Only `/query` (lat/lng
based) works generically, but it only accepts coordinates — not node IDs.

> **[DECIDED]** New server exposes `POST /query` accepting both coordinate-based
> and node-ID-based queries. HERE endpoints are dropped entirely. See Section
> 4.5.

### 3.3 Single-graph design

The server loads one graph directory and builds one CCH. There is no concept of
simultaneously loading both normal and line graph, or routing on either.

> **[DECIDED]** Run **two separate server processes** — one for normal graph,
> one for line graph. Same binary, different flags. See Section 4.5.

### 3.4 No line-graph query correction

Line-graph queries require adding the final edge's travel time (because the line
graph only charges edge costs on transitions — the last edge has no outgoing
transition). The server has no logic for this.

> **[DECIDED]** Handled by `LineGraphQueryEngine::query()` which automatically
> adds `original_travel_time[target_edge]` to every result. See Section 4.2.3.

### 3.5 Architecture coupling

The server uses a message-passing pattern (`mpsc::channel`) between the Rocket
HTTP thread and a background processing thread. All queries and customizations
flow through this channel, with the CCH query `Server` behind
`Arc<Mutex<...>>`. This architecture is fine but is tightly wired to the HERE
data model.

> **[DECIDED]** The concurrency pattern (channel + background thread +
> `Arc<Mutex<>>`) is sound and will be reused. Only the data model changes:
> replace HERE types with our clean edge-index and coordinate types. See
> Section 4.5.

---

## 4. Proposed Implementation

### 4.1 Where to build

The `CCH-Hanoi` workspace is the right home:

- `hanoi-core` — reusable library API for CCH loading, customization, and query
- `hanoi-cli` — operator-facing CLI that exposes these operations as commands

Both crates already exist with stub code and depend on `rust_road_router`.

### 4.2 hanoi-core: Library API surface

#### 4.2.1 Graph loading module

New file: `crates/hanoi-core/src/graph.rs`

```rust
/// Loads a graph directory (normal or line graph) into memory.
/// Expects: first_out, head, travel_time, latitude, longitude
/// Optional: perms/cch_perm (or cch_perm at root)
pub struct GraphData {
    pub first_out: Vec<EdgeId>,
    pub head: Vec<NodeId>,
    pub travel_time: Vec<Weight>,
    pub latitude: Vec<f32>,
    pub longitude: Vec<f32>,
}

impl GraphData {
    pub fn load(dir: &Path) -> Result<Self>;
    pub fn num_nodes(&self) -> usize;      // first_out.len() - 1
    pub fn num_edges(&self) -> usize;      // head.len()
    pub fn as_borrowed_graph(&self) -> BorrowedGraph<'_>;
}
```

#### 4.2.2 CCH engine module

New file: `crates/hanoi-core/src/cch.rs`

```rust
/// Wraps the three CCH phases into a single ergonomic API.
pub struct CchEngine {
    graph: GraphData,
    cch: CCH,
    server: Server<CustomizedBasic<'static, CCH>>,  // self-referential — see note below
    baseline_weights: Vec<Weight>,
}
```

**Self-referential struct problem**: `CustomizedBasic<'a, CCH>` borrows `&'a CCH`, but we want both in the same struct. Solutions:

- **Option A**: Keep `CCH` in a `Box` or `Arc` and use unsafe to extend the
lifetime. The `ouroboros` or `self_cell` crates can do this safely.
- **Option B**: Separate the CCH and server into different structs, where the
caller manages the lifetimes.
- **Option C**: Use `Pin<Box<CCH>>` and transmute the lifetime. Common pattern
in the Rust routing community.

**Chosen approach: Option B** — avoid unsafe, keep it simple:

```rust
pub struct CchContext {
    pub graph: GraphData,
    pub cch: CCH,
    pub baseline_weights: Vec<Weight>,
}

impl CchContext {
    /// Load graph + CCH order, build CCH (Phase 1)
    pub fn load_and_build(graph_dir: &Path, perm_path: &Path) -> Result<Self>;

    /// Customize with baseline weights (Phase 2)
    pub fn customize(&self) -> CustomizedBasic<'_, CCH>;

    /// Customize with modified weights
    pub fn customize_with(&self, weights: Vec<Weight>) -> CustomizedBasic<'_, CCH>;
}

/// Query wrapper — borrows CchContext
pub struct QueryEngine<'a> {
    server: Server<CustomizedBasic<'a, CCH>>,
    context: &'a CchContext,
}

impl<'a> QueryEngine<'a> {
    pub fn new(context: &'a CchContext) -> Self;

    /// Query by node IDs
    pub fn query(&mut self, from: NodeId, to: NodeId) -> Option<QueryAnswer>;

    /// Query by lat/lng (snap-to-edge, then route from both endpoints of
    /// source/target edges — up to 4 CCH queries, return minimum)
    pub fn query_coords(&mut self, from: (f32, f32), to: (f32, f32)) -> Option<QueryAnswer>;

    /// Apply new weights and re-customize
    pub fn update_weights(&mut self, weights: Vec<Weight>);
}

pub struct QueryAnswer {
    pub distance_ms: Weight,
    pub path: Vec<NodeId>,
    pub coordinates: Vec<(f32, f32)>,  // (lat, lng) per path node — server layer flips to [lng, lat] for GeoJSON
}
```

#### 4.2.3 Line graph engine (separate type — Option C)

> **[DECIDED]** `LineGraphQueryEngine` is a separate concrete type from
> `QueryEngine`. Uses `DirectedCCH` exclusively. This keeps the two graph types
> modular and allows dedicated server processes per graph type.

```rust
/// Context for line graph CCH — uses DirectedCCH
pub struct LineGraphCchContext {
    pub graph: GraphData,                    // line graph data
    pub directed_cch: DirectedCCH,           // pruned CCH (no always-INFINITY edges)
    pub baseline_weights: Vec<Weight>,
    pub original_head: Vec<NodeId>,          // original graph's head array
    pub original_latitude: Vec<f32>,         // original graph's node coordinates
    pub original_longitude: Vec<f32>,
    pub original_travel_time: Vec<Weight>,   // for final-edge correction
}

impl LineGraphCchContext {
    /// Load line graph + original graph metadata, build DirectedCCH
    pub fn load_and_build(
        line_graph_dir: &Path,
        original_graph_dir: &Path,
        perm_path: &Path,
    ) -> Result<Self>;

    /// Customize with baseline weights (Phase 2, directed variant)
    pub fn customize(&self) -> CustomizedBasic<'_, DirectedCCH>;

    /// Customize with modified weights
    pub fn customize_with(&self, weights: Vec<Weight>) -> CustomizedBasic<'_, DirectedCCH>;
}

/// Query engine for line graph — uses DirectedCCH, applies final-edge correction
pub struct LineGraphQueryEngine<'a> {
    server: Server<CustomizedBasic<'a, DirectedCCH>>,
    context: &'a LineGraphCchContext,
    spatial: SpatialIndex,
}

impl<'a> LineGraphQueryEngine<'a> {
    pub fn new(context: &'a LineGraphCchContext) -> Self;

    /// Query by line-graph node IDs (= original edge indices).
    /// Automatically adds final edge's travel_time to the result.
    pub fn query(&mut self, source_edge: EdgeId, target_edge: EdgeId) -> Option<QueryAnswer>;

    /// Query by lat/lng — snap-to-edge gives the line-graph node ID directly
    /// (snapped edge ID = line-graph node). Multi-pair query for adjacent edges.
    pub fn query_coords(&mut self, from: (f32, f32), to: (f32, f32)) -> Option<QueryAnswer>;

    /// Apply new weights and re-customize (directed variant)
    pub fn update_weights(&mut self, weights: Vec<Weight>);
}
```

**Final-edge correction**: In the line graph, each node represents an original
edge. The path cost only includes the first edge's weight for each transition.
The final "destination edge" has no outgoing transition, so its weight is
missing from the sum. `LineGraphQueryEngine::query()` adds
`original_travel_time[target_edge]` automatically. See Section 5.4 of the
[Graph Weight Format Guide](../walkthrough/Graph%20Weight%20Format%20and%20Test%20Weight%20Generation%20Guide.md).

**Coordinate mapping for line graph paths**: The path returned is a sequence of
line-graph node IDs (= original edge indices). To produce coordinates:

1. Each path node's coordinate = `(lg_latitude[node], lg_longitude[node])`
  (the tail intersection of the original edge)
2. Append the **head node** of the final edge:
  `(original_latitude[original_head[last_edge]], original_longitude[...])`
   to close the path at the destination intersection

This requires `LineGraphCchContext` to hold references to the original graph's
`head`, `latitude`, and `longitude` arrays — which is why they are included in
the struct above.

#### 4.2.4 Nearest-edge lookup (snap-to-edge)

For coordinate-based queries, a KD-tree finds nearby nodes, then a
perpendicular-distance check selects the closest **edge** (road segment).

> **[UPDATED]** The existing server uses `fux_kdtree ^0.2.0`, which is
> **abandoned** (last release: January 2017, 49 downloads/month). We use
> `**kiddo`** instead — the current best-in-class KD-tree crate:
>
>
> | Crate        | Version   | Last Updated | Downloads/month | Status    |
> | ------------ | --------- | ------------ | --------------- | --------- |
> | `fux_kdtree` | 0.2.0     | Jan 2017     | 49              | Abandoned |
> | `**kiddo**`  | **5.2.4** | **Jan 2026** | **631,000**     | Active    |
>
>
> `kiddo` offers const-generic dimensions (compile-time safety for 2D),
> `ImmutableKdTree` (optimal for static point sets like graph coordinates),
> and SIMD optimizations. MIT/Apache-2.0 licensed.

**Why snap-to-edge instead of snap-to-node**: Snap-to-node finds the nearest
intersection, which can be wrong when the user is midway along a long road
segment — the nearest intersection might be on a parallel street. Snap-to-edge
answers "which road am I on?" by finding the edge whose geometric segment
(tail→head) is closest to the query point.

**Hybrid approach** (KD-tree on nodes + edge post-filter):

```
1. KD-tree query → find k nearest nodes (k ≈ 5–10)
2. For each nearby node, collect all incident edges
3. For each candidate edge (tail→head):
     project query point onto the edge segment
     compute perpendicular distance to the projection
4. Return the edge with the smallest distance
5. Route from both endpoints of that edge, take the shorter result
```

This avoids needing a separate spatial index (R-tree) on edge segments — the
node KD-tree is reused, and the edge filtering is a small local computation.

```rust
use kiddo::ImmutableKdTree;

pub struct SpatialIndex {
    tree: ImmutableKdTree<f32, 2>,   // 2D (lat, lng), immutable after build
    first_out: Vec<EdgeId>,           // CSR adjacency — to find incident edges
    head: Vec<NodeId>,                // edge targets — to get edge endpoints
    lat: Vec<f32>,
    lng: Vec<f32>,
}

/// Result of snapping a coordinate to the nearest edge
pub struct SnapResult {
    pub edge_id: EdgeId,              // the closest edge
    pub tail: NodeId,                 // edge's source node
    pub head: NodeId,                 // edge's target node
}

impl SpatialIndex {
    pub fn build(
        lat: &[f32], lng: &[f32],
        first_out: &[EdgeId], head: &[NodeId],
    ) -> Self;

    /// Snap a coordinate to the nearest edge in the graph.
    /// Returns the edge and its two endpoints.
    pub fn snap_to_edge(&self, lat: f32, lng: f32) -> SnapResult;
}
```

For **normal graph** coordinate queries, the engine queries from both endpoints
of the snapped source edge and both endpoints of the snapped target edge
(up to 2×2 = 4 queries), taking the minimum. This handles the ambiguity of
which direction along the snapped edge the user intends to travel.

For **line graph** coordinate queries, the snapped edge ID directly gives the
line-graph node ID (since line-graph node `i` = original edge `i`). The
multi-pair query from Section 8 still applies for finding all candidate edges
adjacent to the snapped intersection.

### 4.3 hanoi-cli: Command-line interface

New commands for `cch-hanoi`:

```
cch-hanoi query <graph_dir> --from-node <id> --to-node <id>
cch-hanoi query <graph_dir> --from-lat <f> --from-lng <f> --to-lat <f> --to-lng <f>
cch-hanoi customize <graph_dir> --weights <file>
cch-hanoi info <graph_dir>
```

> **[AUDIT — Conformance]** Per the CCH-Hanoi Hub walkthrough: `hanoi-cli`
> wraps `hanoi-core` and must **only** call `hanoi-core` public APIs — it must
> NOT import `rust_road_router` types directly. All CLI subcommands should use
> `CchContext`, `QueryEngine`, `GraphData`, etc. from `hanoi-core`. The Hub
> explicitly states: *"Skeleton until core has APIs"* and *"The CLI
> (`hanoi-cli`) will wrap `hanoi-core` once it has APIs to expose."*
>
> Additionally, `<graph_dir>` should be clarified: it is the **parent**
> directory containing `graph/` and `line_graph/` (e.g.,
> `Maps/data/hanoi_car/`), not the `graph/` subdirectory itself.

### 4.4 DirectedCCH for line graph

> **[DECIDED]** `DirectedCCH` is the **mandatory default** for line graphs, not
> an optional optimization. The type separation is **Option C** — separate
> `QueryEngine` (undirected CCH) and `LineGraphQueryEngine` (DirectedCCH).

For the line graph specifically, the `DirectedCCH` variant is essential. The
undirected `CCH` treats every edge as potentially traversable in both
directions, but line graphs are highly directional (a turn from edge A to edge B
doesn't imply a turn from B to A). The `DirectedCCH` prunes edges that are
always INFINITY in one direction, yielding substantial edge-count reduction.

`to_directed_cch()` requires an initial customization with a zero metric to
identify always-INFINITY edges, then rebuilds the CCH structure. This is a
one-time preprocessing cost at startup.

For the line graph path, the flow is:

```rust
let cch = CCH::fix_order_and_build(&line_graph, order);
let directed_cch = cch.to_directed_cch();   // prune unreachable edges (one-time)
let customized = customize_directed(&directed_cch, &line_graph);
let mut server = Server::new(customized);
```

**Type system note**: `Server<CustomizedBasic<'a, CCH>>` and
`Server<CustomizedBasic<'a, DirectedCCH>>` are different concrete types. This is
resolved by having two separate engine types (Section 4.2.2 and 4.2.3), each
running in its own server process (Section 4.5).

### 4.5 Server design — custom HTTP API (Axum)

> **[DECIDED]** Build a new HTTP server using **Axum** that replaces the
> existing `rust_road_router/server`. The server wraps `hanoi-core` APIs
> exclusively — no direct `rust_road_router` imports.

#### 4.5.1 Technology stack


| Crate        | Version                     | Purpose                                                                |
| ------------ | --------------------------- | ---------------------------------------------------------------------- |
| `axum`       | `0.8`                       | HTTP framework — macro-free, handlers are plain async functions        |
| `tokio`      | `1`                         | Async runtime (multi-threaded work-stealing scheduler)                 |
| `tower`      | `0.5`                       | Middleware layer (timeout, rate limiting)                              |
| `tower-http` | `0.6`                       | HTTP middleware (CORS, compression, decompression, tracing)            |
| `serde`      | `1` (with `derive` feature) | Request/response serialization (query API)                             |
| `serde_json` | `1`                         | JSON parsing (query API)                                               |
| `clap`       | `4` (with `derive` feature) | CLI argument parsing for server binary                                 |
| `bytemuck`   | `1`                         | Zero-copy cast of `&[u8]` → `&[u32]` for weight vector deserialization |


> **[UPDATED]** Removed `tonic`, `prost`, and `tonic-build` — gRPC replaced
> with plain REST (`application/octet-stream`) for the customize endpoint.
> `bytemuck` is retained for zero-copy binary deserialization. Both ports
> (query + customize) use Axum — single HTTP framework, no protobuf tooling.

**Why Axum over Rocket**: Axum is the most popular Rust web framework (2023
Rust Developer Survey), built on the tokio/tower/hyper ecosystem. It works on
**stable Rust** (MSRV 1.80) — though we require nightly for
`rust_road_router`, not depending on nightly for the HTTP layer reduces
fragility. Handlers are plain `async fn`s with extractor-based dependency
injection. All components are `tower::Service`, making them testable without a
running server.

**Why not Rocket**: Rocket historically required nightly. While recent versions
support stable, its ecosystem is smaller, and axum's tower-based composability
is better suited for our gateway pattern (Section 4.5.7).

#### 4.5.2 Two-process architecture

Run **two separate server processes**, same binary, different graph directories.
Each process binds **two HTTP ports** (both Axum): one for queries, one for
customization:

```bash
# Normal graph server (query: 8080, customize: 9080)
hanoi-server --graph-dir Maps/data/hanoi_car/graph \
             --query-port 8080 --customize-port 9080

# Line graph server (query: 8081, customize: 9081)
hanoi-server --graph-dir Maps/data/hanoi_car/line_graph \
             --original-graph-dir Maps/data/hanoi_car/graph \
             --query-port 8081 --customize-port 9081 --line-graph
```

**Port assignment**:


| Server       | Query port (REST) | Customize port (REST, binary) |
| ------------ | ----------------- | ----------------------------- |
| Normal graph | `8080`            | `9080`                        |
| Line graph   | `8081`            | `9081`                        |


> **[UPDATED]** Each server binds two Axum HTTP ports. The query port serves
> `/query` and `/info` (JSON). The customize port serves `POST /customize`
> (raw binary body, `application/octet-stream`). Both are plain HTTP — no
> gRPC, no protobuf. Separate ports ensure: (1) a large ~8 MB customization
> upload cannot block query traffic, (2) each port can be firewalled
> independently (customize port internal-only, query port exposed).

The `--line-graph` flag switches behavior:

- Uses `LineGraphCchContext` + `LineGraphQueryEngine` (DirectedCCH)
- Applies final-edge correction on queries
- Uses edge-based coordinate snapping
- Requires `--original-graph-dir` for coordinate mapping and final-edge weights

Each server is fully self-contained — it can run independently without the
other. This is important: in development or testing you may only need one graph
type. Benefits: memory isolation, independent scaling, independent updates,
clean failure domain.

#### 4.5.3 API endpoints

**REST endpoints** (Axum, query port):


| Method | Endpoint | Input                 | Purpose                                         |
| ------ | -------- | --------------------- | ----------------------------------------------- |
| `POST` | `/query` | `QueryRequest` (JSON) | Route query — coordinate-based or node-ID-based |
| `GET`  | `/info`  | —                     | Graph metadata and server status                |


**Customize endpoint** (Axum, customize port — separate from query port):


| Method | Endpoint     | Content-Type               | Purpose                                           |
| ------ | ------------ | -------------------------- | ------------------------------------------------- |
| `POST` | `/customize` | `application/octet-stream` | Re-customize with full weight vector (raw binary) |


> **[UPDATED]** Customization uses plain REST with raw binary body on a
> **separate port**. No JSON, no gRPC, no protobuf — just raw `[u32; num_edges]` bytes over HTTP. Rationale: internal-only consumer (live
> traffic module), binary-heavy payload (2M+ edges), port isolation for
> fault tolerance. A plain HTTP POST achieves identical wire efficiency to
> gRPC for this single-endpoint, unary use case.

##### `POST /query` — Request

```json
// Option A: coordinate-based (most common — any GPS point)
{
  "from_lat": 21.0285,
  "from_lng": 105.8542,
  "to_lat": 21.0355,
  "to_lng": 105.8480
}

// Option B: node-ID-based (for internal/debugging use)
{
  "from_node": 1234,
  "to_node": 5678
}
```

Both options are accepted by the same endpoint. The server detects which
variant based on which fields are present.

##### `POST /query` — Processing pipeline (normal graph mode)

```
Coordinate request                         Node-ID request
{ from_lat, from_lng, ... }                { from_node, to_node }
         │                                          │
         ▼                                          │
  SpatialIndex::snap_to_edge()                      │
  (KD-tree → nearby nodes → edge post-filter)       │
         │                                          │
         ▼                                          │
  SnapResult { edge, tail, head }  (×2: src+dst)    │
         │                                          │
         ▼                                          │
  Query from both endpoints of src edge             │
  to both endpoints of dst edge (up to 4 queries)   │
         │                                          │
         ▼                                          ▼
  Take minimum-distance result   ◄──────────────────┘
         │
         ▼
  Map path nodes → coordinates     ← latitude[node], longitude[node]
         │
         ▼
  QueryAnswer { distance_ms, path_nodes, coordinates }
```

##### `POST /query` — Processing pipeline (line graph mode)

```
Coordinate request                         Edge-ID request
{ from_lat, from_lng, ... }                { from_node, to_node }
         │                                          │
         ▼                                          │
  SpatialIndex::snap_to_edge()                      │
  (snapped edge ID = line-graph node ID)            │
         │                                          │
         ▼                                          ▼
  LineGraphQueryEngine::query(src_edge, dst_edge)   │
  + multi-pair query for adjacent edges ◄───────────┘
  + final-edge correction (adds target edge weight)
         │
         ▼
  Map path → coordinates (tail-node coords + final head-node)
         │
         ▼
  QueryAnswer { distance_ms, path_nodes, coordinates }
```

See Section 4.2.4 for the snap-to-edge algorithm and Section 4.2.3 for the
line graph coordinate mapping.

##### `POST /query` — Response

```json
{
  "distance_ms": 42000,
  "path_nodes": [1234, 2001, 3050, 5678],
  "coordinates": [[21.0285, 105.8542], [21.0290, 105.8550], [21.0320, 105.8510], [21.0355, 105.8480]]
}
```


| Field         | Type           | Description                                                                                                                                                                                                                                                                                                                                                                |
| ------------- | -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `distance_ms` | `u32`          | Total travel time in **milliseconds**. This is the shortest-path cost from the CCH query. For line graph mode, includes the final-edge correction.                                                                                                                                                                                                                         |
| `path_nodes`  | `[u32]`        | Ordered sequence of node IDs along the shortest path. For **normal graph**: intersection node IDs. For **line graph**: line-graph node IDs (= original edge indices).                                                                                                                                                                                                      |
| `coordinates` | `[[f32, f32]]` | Ordered `[lat, lng]` pairs for each point in the path. For **normal graph**: one coordinate per intersection. For **line graph**: tail-node coordinates of each original edge, plus the head-node coordinate of the final edge (see Section 13.3). The **server layer** flips these to `[lng, lat]` when the `Accept` header or a query parameter requests GeoJSON format. |


If no path exists (disconnected nodes), the response is:

```json
{
  "distance_ms": null,
  "path_nodes": [],
  "coordinates": []
}
```

##### `POST /customize` — Customize endpoint (binary body)

**Request**: Raw binary body containing the full weight vector.


| Header         | Value                                                                                                                           |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `Content-Type` | `application/octet-stream`                                                                                                      |
| Body           | Raw little-endian `[u32; num_edges]` — position = edge index, value = travel time in ms. Must be exactly `num_edges × 4` bytes. |


**Response** (JSON):

```json
// 200 OK
{ "accepted": true, "message": "customization queued" }

// 400 Bad Request
{ "accepted": false, "message": "<error message here>" }
```

**Server-side handler** (Axum):

```rust
use axum::{body::Bytes, extract::State, http::StatusCode, Json};

async fn handle_customize(
    State(app): State<AppState>,
    body: Bytes,
) -> Result<Json<CustomizeResponse>, (StatusCode, Json<CustomizeResponse>)> {
    let expected = app.num_edges * 4;
    if body.len() != expected {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(CustomizeResponse {
                accepted: false,
                message: format!(
                    "expected {} bytes ({} edges × 4), got {}",
                    expected, app.num_edges, body.len()
                ),
            }),
        ));
    }
    let weights: &[u32] = bytemuck::cast_slice(&body);
    // Send to customization thread via watch channel (Section 4.5.5)
    app.watch_tx.send(weights.to_vec()).unwrap();
    Ok(Json(CustomizeResponse {
        accepted: true,
        message: "customization queued".into(),
    }))
}
```

**Customize port setup** (two Axum instances in one binary):

```rust
// Query port — JSON API for external consumers
let query_router = Router::new()
    .route("/query", post(handle_query))
    .route("/info", get(handle_info))
    .with_state(app_state.clone());

// Customize port — binary API for internal pipeline
// RequestDecompressionLayer transparently decompresses gzip request bodies
// (Content-Encoding: gzip). The handler receives decompressed bytes.
let customize_router = Router::new()
    .route("/customize", post(handle_customize))
    .layer(DefaultBodyLimit::max(16 * 1024 * 1024))  // 16 MB (post-decompression)
    .layer(RequestDecompressionLayer::new())          // gzip request decompression
    .with_state(app_state.clone());

let query_listener = TcpListener::bind(query_addr).await?;
let customize_listener = TcpListener::bind(customize_addr).await?;

tokio::spawn(axum::serve(query_listener, query_router).into_future());
axum::serve(customize_listener, customize_router).await?;
```

The response returns **immediately** — customization runs in a background
thread. Queries continue being served with the previous weights until the new
customization completes and is atomically swapped in.

> **[UPDATED]** Replaced gRPC with plain REST binary body. Key advantages for
> the 2M-edge Hanoi graph:
>
> - **Wire size**: ~8 MB raw (vs ~90 MB JSON for full vector). ~2–3 MB with
>   gzip (`RequestDecompressionLayer` on server, client sends
>   `Content-Encoding: gzip`).
> - **Parse time**: `bytemuck::cast_slice` is zero-copy reinterpretation — no
> deserialization. JSON parsing a 2M-element array takes measurable time.
> - **No merge step**: full vector replaces the baseline directly — no need to
> clone baseline weights and apply sparse overrides.
> - **Port isolation**: customize runs on a dedicated Axum port. A large upload
> cannot starve the query API.
> - **Simplicity**: no protobuf tooling, no codegen, no `protoc` dependency.
> Same Axum framework on both ports — one less dependency to maintain.

##### `GET /info` — Response

```json
{
  "graph_type": "normal",
  "num_nodes": 245000,
  "num_edges": 612000,
  "server_mode": "normal",
  "customization_active": false
}
```


| Field                  | Type     | Description                                                   |
| ---------------------- | -------- | ------------------------------------------------------------- |
| `graph_type`           | `string` | `"normal"` or `"line_graph"`                                  |
| `num_nodes`            | `u32`    | Number of nodes in the loaded graph                           |
| `num_edges`            | `u32`    | Number of edges in the loaded graph                           |
| `server_mode`          | `string` | `"normal"` or `"line_graph"` — echoes the `--line-graph` flag |
| `customization_active` | `bool`   | Whether a background customization is currently running       |


#### 4.5.4 Concurrency pattern

Adapted from the existing server's proven pattern, updated to use standard
library primitives:

```
┌────────────────────────────────┐  ┌──────────────────────────────────┐
│  Axum HTTP (query port, e.g.  │  │  Axum HTTP (customize port,     │
│  8080/8081)                   │  │  e.g. 9080/9081)                │
│  /query, /info                │  │  POST /customize (binary body)  │
└──────────────┬─────────────────┘  └──────────────┬───────────────────┘
               │ tokio::sync::mpsc                  │ tokio::sync::watch
               │ (query requests)                   │ (latest weight vector)
               ▼                                    ▼
┌──────────────────────────────────────────────────────────────────────┐
│  Background thread (owns CchContext + QueryEngine)                  │
│                                                                     │
│  std::thread::scope                                                 │
│    ├── query loop: recv from mpsc → lock Server → query → respond   │
│    └── customization watcher: watch_rx.changed() → re-customize     │
│           → Arc<Mutex<Server>> lock → swap → unlock                 │
└──────────────────────────────────────────────────────────────────────┘
```

> **[UPDATED]** Both ports are Axum HTTP — no Tonic/gRPC. The background
> thread receives queries via `mpsc` and weight updates via `watch` — two
> independent channels, two independent ports, converging on the same
> `Server` instance behind `Arc<Mutex<>>`.

> **[UPDATED]** `crossbeam_utils::thread::scope` is **soft-deprecated** since
> Rust 1.63 — the standard library now provides `std::thread::scope` with
> better performance and proper panic propagation. Use `std::thread::scope`
> for our new server. The existing `rust_road_router/server` still uses
> crossbeam but we do not modify it.

Key properties:

- Queries are served during re-customization (lock held only for the O(1) swap)
- The swap is atomic — no query ever sees a partially-customized state
- `std::thread::scope` propagates panics from child threads (crossbeam
silently ignores them — this is safer)

#### 4.5.5 Background customization with stale-update cancellation

For live traffic integration, a separate module handles live traffic data
processing and packaging. This module produces a full weight vector and sends it
to the server via `POST /customize` (raw binary). The server must handle the
case where a new update arrives while a previous customization is still running:

```
Live traffic module → POST /customize (raw binary) (every X seconds)
                              │  (customize port, e.g. 9080)
                              ▼
                    ┌─────────────────────────────┐
                    │   Customization scheduler    │
                    │                             │
                    │   If customization running: │
                    │     → discard in-progress   │
                    │     → start new one with    │
                    │       latest weights        │
                    │                             │
                    │   If idle:                  │
                    │     → start customization   │
                    │       immediately           │
                    └─────────────────────────────┘
```

**Design**: Use `tokio::sync::watch` — a single-value channel where the
receiver always sees the **latest** value. The HTTP handler writes the received
weight vector into the watch channel. The customization thread calls
`watch_rx.changed()` to detect new data, reads the latest weight set, and
begins customization. If new data arrives mid-customization, the thread detects
the change after completing (or at cancellation checkpoints) and starts over
with the newest weights.

**Important**: The live traffic module is an **external concern** — it is NOT
part of `hanoi-core` or the server. It is a separate module/process that
produces `Vec<u32>` (full weight vector, `num_edges` elements) and sends it via
`POST /customize` (raw binary). This separation means:

- The server doesn't need to know about traffic data formats or sources
- The traffic module doesn't need to know about CCH internals
- Either can be replaced independently
- They can run on different machines (same network) — plain HTTP handles the
transport

#### 4.5.6 API gateway (optional — for unified external access)

Each routing server (normal graph, line graph) runs independently with its own
query + customize ports. For production deployment, an **API gateway** provides
a single entry point for **external consumers** (apps, dashboards) to access
both servers.

The gateway exposes **query and info endpoints only** — it does NOT proxy
customization. Customization is a direct internal link from the data processing
pipeline to each server's customize port (T-shape architecture):

```
External clients (apps, dashboards, curl)
    │
    │  REST: POST /query         { graph_type, from_lat, ... }
    │  REST: GET  /info?graph_type=normal
    ▼
┌──────────────────────────────────────────────────┐
│     API Gateway (Axum, port 50051)               │
│  ┌────────────────────────────────────────────┐  │
│  │  Routes by graph_type field:               │  │
│  │                                            │  │
│  │  "normal"     → localhost:8080             │  │
│  │  "line_graph" → localhost:8081             │  │
│  └────────────────────────────────────────────┘  │
│                                                  │
│  REST proxy (reqwest or Axum reverse proxy)      │
└──────────────────────────────────────────────────┘
    │  HTTP: /query, /info       │  HTTP: /query, /info
    ▼                            ▼
Normal graph server       Line graph server
(query: 8080)             (query: 8081)


Data Processing Pipeline (internal) ─── POST /customize ──► Normal server  :9080
                                    └── POST /customize ──► Line graph srv :9081
```

> **[UPDATED]** Gateway is now **query/info only** — no customization proxy.
> Customization is a direct internal path from the data processing pipeline
> to each server's customize port. The gateway is purely for external app
> access. All communication is REST/HTTP — no gRPC anywhere in the stack.

**Gateway REST endpoints** (Axum):


| Method | Path     | Description                                  |
| ------ | -------- | -------------------------------------------- |
| POST   | `/query` | Route query — forwards to backend query port |
| GET    | `/info`  | Server info (num nodes/edges, status)        |


**Query request/response** (JSON):

```json
// POST /query
{
  "graph_type": "normal",          // "normal" or "line_graph"
  "from_lat": 21.028511,
  "from_lng": 105.804817,
  "to_lat": 21.007324,
  "to_lng": 105.847130
}

// Response
{
  "distance_ms": 542300,
  "path": [[21.028, 105.804], [21.025, 105.810], ...]
}
```

**Info request** (JSON):

```json
// GET /info?graph_type=normal
// Response
{
  "graph_type": "normal",
  "num_nodes": 245000,
  "num_edges": 2100000,
  "customization_active": false
}
```

The gateway is a thin Axum binary — it holds HTTP clients (reqwest or Axum
reverse proxy) to each backend's query port. Routes are selected by the
`graph_type` field. No routing algorithm logic, no CCH knowledge.

The gateway is **optional** — each server is independently addressable by its
query port. The gateway exists for operational convenience (single entry point,
centralized auth/rate-limiting via tower middleware).

#### 4.5.7 Future app integration

The system uses **REST/HTTP everywhere**:

- **External** (gateway or direct server query port): For apps, dashboards,
browser clients, curl debugging. JSON over HTTP.
- **Internal** (direct server customize port): For the data processing
pipeline. Raw binary over HTTP (`application/octet-stream`).

Both are plain HTTP — no gRPC, no protobuf, no special tooling. Any HTTP
client (reqwest, curl, fetch) can interact with any endpoint.

Future considerations:

- Mapbox integration for route visualization (see Section 13)
- WebSocket or SSE streaming from gateway for live route updates to connected
clients
- Authentication/rate limiting via `tower` middleware layers on Axum
- If structured RPC contracts become valuable (many internal services, cross-
language clients), gRPC can be introduced at that point

These are deferred to when the app development team is involved.

---

## 5. Prerequisites & Blockers

### 5.1 Line graph needs its own cch_perm

The line graph at `Maps/data/hanoi_car/line_graph/` currently has no `cch_perm`
file. CCH requires a node ordering computed by InertialFlowCutter (IFC).

**To generate**:

```bash
cd Maps/data/hanoi_car
../../../rust_road_router/flow_cutter_cch_order.sh line_graph/
# Produces: line_graph/perms/cch_perm
```

> **[AUDIT]** Fixed script path: was `../../scripts/flow_cutter_cch_order.sh`
> — the actual location is `rust_road_router/flow_cutter_cch_order.sh` (there
> is no `scripts/` directory at the repo root containing this script).
> Note: the pipeline script (`scripts/pipeline`) handles this automatically.

This must be done before the line graph can be used with CCH. The normal graph
already has `graph/perms/cch_perm`.

### 5.2 Nightly Rust

The `rust_road_router` engine uses nightly features:

- `#![feature(array_windows)]` (used in `customize_perfect` reconstruction)
- `#![feature(impl_trait_in_assoc_type)]` — the critical nightly feature
- GATs (`type P<'s>: PathServer where Self: 's`)

> **[AUDIT]** The Hub walkthrough identifies `impl_trait_in_assoc_type` as the
> key nightly requirement. Ensure `rust-toolchain.toml` stays on nightly.

The `CCH-Hanoi` workspace already specifies nightly via `rust-toolchain.toml`.

### 5.3 Cargo dependency wiring

> **[DECIDED]** All dependencies are listed explicitly — do not rely on
> transitive deps.

`hanoi-core/Cargo.toml`:

```toml
[dependencies]
rust_road_router = { path = "../../../rust_road_router/engine" }  # already exists
kiddo = "5"               # KD-tree spatial index (replaces abandoned fux_kdtree)
rayon = "1"               # parallel customization (explicit, not transitive)
```

`hanoi-cli/Cargo.toml`:

```toml
[dependencies]
hanoi-core = { path = "../hanoi-core" }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

`hanoi-tools/Cargo.toml` (for `hanoi_server` binary):

```toml
[dependencies]
hanoi-core = { path = "../hanoi-core" }
axum = "0.8"                                                        # REST query API
tokio = { version = "1", features = ["full"] }                      # async runtime
tower = "0.5"                                                       # middleware
tower-http = { version = "0.6", features = ["cors", "compression-gzip", "decompression-gzip", "trace"] }
serde = { version = "1", features = ["derive"] }                    # JSON serialization (query API)
serde_json = "1"
clap = { version = "4", features = ["derive"] }                     # CLI args
bytemuck = { version = "1", features = ["derive"] }                 # zero-copy &[u8] → &[u32]
```

> **[UPDATED]** Removed `tonic`, `prost`, and `tonic-build` — no gRPC or
> protobuf in the stack. Customization uses plain REST with binary body on a
> separate Axum port. `bytemuck` retained for zero-copy `&[u8]` → `&[u32]`
> cast of the incoming weight vector.

---

## 6. File-Level Implementation Checklist

### Phase A: Core library (hanoi-core)


| #   | File                                  | What to implement                                                                                                                                                                                                                  |
| --- | ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `crates/hanoi-core/src/lib.rs`        | Module declarations: `pub mod graph; pub mod cch; pub mod line_graph; pub mod spatial;`                                                                                                                                            |
| 2   | `crates/hanoi-core/src/graph.rs`      | `GraphData` struct, `load()`, `as_borrowed_graph()`, validation                                                                                                                                                                    |
| 3   | `crates/hanoi-core/src/cch.rs`        | `CchContext`, `QueryEngine`, `QueryAnswer`, customization wrappers. `query_coords()` must implement multi-pair query: snap-to-edge → route from both endpoints of source/target edges (up to 2×2 = 4 CCH queries) → return minimum |
| 4   | `crates/hanoi-core/src/line_graph.rs` | `LineGraphCchContext`, `LineGraphQueryEngine`, DirectedCCH wrappers, final-edge correction, coordinate mapping from original graph                                                                                                 |
| 5   | `crates/hanoi-core/src/spatial.rs`    | `SpatialIndex` with `kiddo::ImmutableKdTree` + snap-to-edge (KD-tree → nearby nodes → perpendicular edge filter)                                                                                                                   |
| 6   | `crates/hanoi-core/Cargo.toml`        | Add `kiddo = "5"`, `rayon = "1"` dependencies                                                                                                                                                                                      |


### Phase B: CLI + Server (hanoi-cli)


| #   | File                                         | What to implement                                                                                                                                                                                                                                                                            |
| --- | -------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 7   | `crates/hanoi-cli/src/main.rs`               | Argument parsing, `query` and `info` subcommands (interactive CLI)                                                                                                                                                                                                                           |
| 8   | `crates/hanoi-cli/Cargo.toml`                | Add `clap`, `serde`, `serde_json` dependencies                                                                                                                                                                                                                                               |
| 9   | `crates/hanoi-tools/src/bin/hanoi_server.rs` | Dual-port Axum server: query port (`/query`, `/info`, JSON) + customize port (`POST /customize`, binary body); `--line-graph` flag; `--query-port` / `--customize-port` args; `tokio::sync::watch` for customization scheduling; `std::thread::scope` for background thread; see Section 4.5 |
| 10  | `crates/hanoi-tools/Cargo.toml`              | Add `axum`, `tokio`, `tower`, `tower-http`, `serde`, `serde_json`, `clap`, `bytemuck` deps                                                                                                                                                                                                   |


### Phase C: Line graph support


| #   | Task                               | Detail                                                                                                                                                                                                 |
| --- | ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 11  | Generate `cch_perm` for line graph | Run IFC on `line_graph/` directory (pipeline handles this; verify after `hanoi_car` generation completes)                                                                                              |
| 12  | Implement `LineGraphCchContext`    | `to_directed_cch()` + `customize_directed()`, load original graph metadata                                                                                                                             |
| 13  | Implement `LineGraphQueryEngine`   | Final-edge correction, coordinate mapping, snap-to-edge giving line-graph node ID directly, multi-pair query across all edges incident to snapped intersection (d_s × d_t CCH queries), return minimum |


### Phase D: Path visualization support


| #   | Task                             | Detail                                                                                                                         |
| --- | -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| 14  | `QueryAnswer` coordinate output  | Both engines populate `coordinates: Vec<(f32, f32)>` in `(lat, lng)` order                                                     |
| 15  | Server GeoJSON conversion        | Server layer converts `(lat, lng)` → `[lng, lat]` for Mapbox/GeoJSON consumers                                                 |
| 16  | Road-following geometry (future) | Integrate Mapbox Map Matching API as optional server-side post-processing; or store OSM way geometries during graph generation |


> **[DECIDED — Graph-loading duplication]** `generate_line_graph.rs` in
> `hanoi-tools` keeps its own graph-loading logic (Option A). This preserves
> tool independence: if `hanoi-core` has issues during development, the pipeline
> tool remains functional as a bypass. Revisit once `GraphData` is stable.

---

## 7. Full Code Flow — Normal Graph

```
                    ┌──────────────────────────────────────────┐
                    │        On-disk files (Maps/data/)        │
                    │  first_out  head  travel_time  cch_perm  │
                    │  latitude   longitude                    │
                    └─────────────────┬────────────────────────┘
                                      │ Vec::load_from()
                                      ▼
                    ┌──────────────────────────────────────────┐
                    │           GraphData (in memory)          │
                    │  first_out: Vec<EdgeId>                  │
                    │  head: Vec<NodeId>                       │
                    │  travel_time: Vec<Weight>                │
                    └─────────────────┬────────────────────────┘
                                      │ FirstOutGraph::new()
                                      ▼
                    ┌──────────────────────────────────────────┐
                    │     BorrowedGraph (CSR view, no copy)    │
                    └─────────────────┬────────────────────────┘
                                      │
                     ┌────────────────┴──────────────────┐
                     │ Phase 1                           │
                     │ CCH::fix_order_and_build(         │
                     │   &graph, order                   │
                     │ )                                 │
                     └────────────────┬──────────────────┘
                                      │
                                      ▼
                    ┌──────────────────────────────────────────┐
                    │   CCH struct (topology only, no weights) │
                    │   first_out, head, tail (chordal graph)  │
                    │   elimination_tree                       │
                    │   cch_edge → orig_arc mappings           │
                    │   separator_tree                         │
                    └─────────────────┬────────────────────────┘
                                      │
                     ┌────────────────┴──────────────────┐
                     │ Phase 2                           │
                     │ customize(&cch, &graph)           │
                     │   → prepare_weights (respecting)  │
                     │   → customize_basic (triangles)   │
                     └────────────────┬──────────────────┘
                                      │
                                      ▼
                    ┌──────────────────────────────────────────┐
                    │   CustomizedBasic (weights on CCH edges) │
                    │   upward: Vec<Weight>                    │
                    │   downward: Vec<Weight>                  │
                    │   up_unpacking, down_unpacking           │
                    └─────────────────┬────────────────────────┘
                                      │ Server::new(customized)
                                      ▼
                    ┌──────────────────────────────────────────┐
                    │     Server<CustomizedBasic>              │
                    │   fw_distances, bw_distances (workspace) │
                    │   fw_parents, bw_parents                 │
                    └─────────────────┬────────────────────────┘
                                      │
                     ┌────────────────┴──────────────────┐
                     │ Phase 3                           │
                     │ server.query(Query { from, to })  │
                     │   → elimination tree walk (bidir) │
                     │   → path unpacking                │
                     └────────────────┬──────────────────┘
                                      │
                                      ▼
                    ┌──────────────────────────────────────────┐
                    │     QueryAnswer                          │
                    │   distance_ms: u32 (milliseconds)        │
                    │   path: Vec<NodeId>                      │
                    │   coordinates: Vec<(f32, f32)> (lat,lng) │
                    └──────────────────────────────────────────┘
```

---

## 8. Full Code Flow — Line Graph

```
line_graph/first_out, head, travel_time, cch_perm
                    │
                    ▼
            GraphData::load()
                    │
                    ▼
    CCH::fix_order_and_build(&lg, order)
                    │
                    ▼
            cch.to_directed_cch()        ← prune always-INFINITY edges
                    │
                    ▼
    customize_directed(&dir_cch, &lg)
                    │
                    ▼
            Server::new(customized)
                    │
                    ▼
    server.query(Query {
        from: source_edge_id,            ← line-graph node = original edge
        to: target_edge_id,
    })
                    │
                    ▼
    result.distance + original_travel_time[target_edge_id]
                    ↑
        Final edge correction ───────────────────────────
```

**Mapping between graphs**:

- Line-graph node `i` = Original graph edge `i`
- To query "route from intersection A to intersection B via turn-aware routing":
  1. Find original edges adjacent to A → these are candidate source line-graph
    nodes
  2. Find original edges adjacent to B → these are candidate target line-graph
    nodes
  3. Query all (source, target) pairs and take the minimum
  4. Add `original_travel_time[target_edge]` to the result

---

## 9. Weight Update Flow

> **[DECIDED]** Use **full-vector replacement** via REST binary POST. The live
> traffic module sends a complete `[u32; num_edges]` weight vector. NOT
> cumulative, NOT sparse. This is correct for live traffic data that arrives
> as periodic snapshots.

```
Live traffic module (external process, possibly different machine)
        │
        │ POST /customize (application/octet-stream)
        │ Body: raw [u32; num_edges] (customize port, e.g. 9080)
        ▼
HTTP handler: bytemuck::cast_slice(&body) → &[u32]
        │
        │ watch_tx.send(weights.to_vec())
        │ (tokio::sync::watch — latest-value semantics)
        ▼
Customization thread detects change:
        │
        │ received_weights: Vec<u32>   ← complete snapshot, ready to use
        │                                 (no baseline clone + merge needed)
        ▼
new_graph = FirstOutGraph::new(&first_out, &head, received_weights)
        │
        │ re-customize (reuses CCH topology, background thread)
        ▼
new_customized = customize(&cch, &new_graph)
        │
        │ atomic swap (O(1), lock held only for swap)
        ▼
server.update(new_customized)
        │
        (queries served with previous customization during re-customization)
```

> **[UPDATED]** Simplified from the previous sparse-update flow (clone baseline
> → apply per-edge overrides → re-customize). The full-vector approach
> eliminates the merge step entirely — the received weight vector is used
> directly as the metric for re-customization. The server no longer needs to
> store a `baseline_weights` vector for merging purposes (though it may
> retain one for `/info` or diagnostics).

**Full-vector replacement is correct for live traffic** because:

- Traffic feeds are **snapshots**, not deltas — each update represents the
current state of every road segment
- Replacing is **idempotent** — apply the same snapshot twice, get the same
result; no drift from lost or reordered updates
- CCH re-customization cost is **fixed** regardless of how many edges changed
(full bottom-up triangle relaxation always runs)
- The traffic module already computes the full weight vector internally — sending
it directly avoids duplicating merge logic on both sides

**Wire cost for Hanoi** (~2M edges):

- Raw: `2M × 4 bytes = 8 MB` per update
- With gzip: ~2–3 MB (travel time vectors compress well due to low entropy —
  most values are similar magnitudes)
- At one update every 30 seconds: ~4–6 MB/min bandwidth — negligible on a LAN

**Gzip mechanism**: The server's customize port uses `tower-http`'s
`RequestDecompressionLayer`, which transparently handles `Content-Encoding:
gzip` request bodies. The client (pipeline module) compresses the raw weight
vector with gzip before sending:

```
Pipeline client:
  raw_weights: [u32; 2M] → 8 MB
  gzip(raw_weights) → ~2–3 MB
  POST /customize  Content-Encoding: gzip  body: <compressed>

Server (customize port):
  RequestDecompressionLayer detects Content-Encoding: gzip
  → decompresses body transparently
  → handler receives decompressed Bytes (8 MB)
  → bytemuck::cast_slice → &[u32]
```

If the client sends without `Content-Encoding: gzip`, the middleware passes the
body through unchanged — gzip is optional, not required. This allows simple
debugging with `curl --data-binary @weights.bin` without compression.

**Integration with live traffic module**: A separate, external module/process
handles traffic data processing and produces `Vec<u32>` (length = `num_edges`,
values in milliseconds) that it sends via `POST /customize` (raw binary). The
server's
customization scheduler (Section 4.5.5) handles cancellation of stale
in-progress customizations when newer data arrives.

---

## 10. Risk Assessment


| Risk                                                 | Impact                                                                                                             | Mitigation                                                                                                                                           |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| Self-referential struct (`CCH` + `CustomizedBasic`)  | Compile error                                                                                                      | **Resolved**: Option B (separate structs with explicit lifetimes)                                                                                    |
| Missing `cch_perm` for line graph                    | Line graph CCH won't build                                                                                         | Generate with IFC after `hanoi_car` pipeline completes                                                                                               |
| Large graph memory usage                             | Hanoi graph may be large; two server processes doubles it                                                          | Profile; CCH adds ~2-3x edge count overhead. Separate processes allow independent memory management                                                  |
| Nightly Rust breakage                                | Build failure                                                                                                      | Pin nightly version in `rust-toolchain.toml`                                                                                                         |
| `DirectedCCH` vs `CCH` type parameter                | Different concrete types — can't share a field                                                                     | **Resolved**: Option C — separate `QueryEngine` / `LineGraphQueryEngine` types                                                                       |
| KD-tree crate migration                              | Using `kiddo` instead of `fux_kdtree` — API differences                                                            | `kiddo` is well-documented with 631K downloads/month; `ImmutableKdTree` is a clean fit for our static-coordinate use case                            |
| Line-graph coordinate query performance              | For intersection with degree `d_s` × `d_t`, requires `d_s * d_t` CCH queries                                       | Acceptable for CLI; consider multi-target optimization for future server use                                                                         |
| Line-graph coordinate mapping                        | `LineGraphQueryEngine` needs original graph's `head`/`lat`/`lng` arrays — additional memory and loading complexity | Included in `LineGraphCchContext` struct (Section 4.2.3); loaded from `--original-graph-dir`                                                         |
| Live traffic update frequency vs customization time  | If updates arrive faster than customization completes, work is wasted                                              | Stale-update cancellation: always customize with latest snapshot, discard in-progress work (Section 4.5.5)                                           |
| Coordinate system confusion (`lat,lng` vs `lng,lat`) | Mapbox/GeoJSON uses `[lng, lat]`; internal format is `(lat, lng)`                                                  | `hanoi-core` returns `(lat, lng)`; server layer flips for GeoJSON. Clear boundary.                                                                   |
| Road geometry fidelity                               | Straight-line polylines between intersections look jagged on curved roads                                          | Phase D: Mapbox Map Matching as post-processing, or OSM way geometry storage (deferred)                                                              |
| Binary weight vector size                            | Full `[u32; 2M]` = 8 MB per update; exceeds Axum default body limit (2 MB)                                         | Configure `DefaultBodyLimit::max(16 * 1024 * 1024)` on the customize router (see Section 4.5.3)                                                      |
| Endianness mismatch                                  | `bytemuck::cast_slice` assumes sender and receiver share endianness                                                | Both sides are x86-64 Linux — little-endian is guaranteed. Document this assumption. If cross-arch deployment arises, add explicit endian conversion |


---

## 11. Design Decisions (Resolved)

1. ~~**Should `QueryEngine` own or borrow the `CchContext`?**~~ **[RESOLVED]**
  — Option B: `QueryEngine` **borrows** `&'a CchContext`. See Section 4.2.2.
2. ~~**Should we support both normal and line graph in a single
  `QueryEngine`?**~~ **[RESOLVED]** — **No.** Separate types: `QueryEngine`
   (undirected CCH) and `LineGraphQueryEngine` (DirectedCCH). Each runs in its
   own server process. See Sections 4.2.3, 4.4, 4.5.
3. ~~**Should coordinate-based queries snap to nearest node or nearest
  edge?**~~ **[RESOLVED]** — **Snap-to-edge** for both graph types. Hybrid
   approach: KD-tree finds nearby nodes, then perpendicular-distance check
   selects the closest edge segment. For normal graph: route from both
   endpoints (up to 4 queries). For line graph: snapped edge ID = line-graph
   node ID directly. See Section 4.2.4.
4. ~~**Should re-customization happen in a background thread?**~~
  **[RESOLVED]** — **Always yes**, for both CLI and server. Background thread
   with atomic swap. For live traffic: stale-update cancellation via
   `watch`-style channel (Section 4.5.4). Customization must never block
   queries.
5. ~~**HTTP framework choice**~~ **[RESOLVED]** — **Axum 0.8**. See Section
  4.5.1 for rationale. Replaces Rocket.
6. ~~**Protocol for app ↔ server communication**~~ **[RESOLVED]** — **REST
  everywhere**. Query endpoints (`/query`, `/info`) use JSON. Customize
   endpoint uses raw binary (`application/octet-stream`). No gRPC — single
   framework (Axum) on all ports. Gateway exposes query/info only; customize
   is a direct internal path from the data pipeline. See Section 4.5.6–4.5.7.
7. **Road-following geometry** — Whether to use Mapbox Map Matching or store
  OSM way geometries. Deferred to Phase D. See Section 13.
8. ~~**KD-tree crate**~~ **[RESOLVED]** — `**kiddo` 5.x** replaces abandoned
  `fux_kdtree`. See Section 4.2.4.
9. ~~**Scoped threads**~~ **[RESOLVED]** — `**std::thread::scope`** replaces
  soft-deprecated `crossbeam_utils::thread::scope`. See Section 4.5.4.
10. ~~**Customization protocol and payload format**~~ **[RESOLVED]** — **REST
  with raw binary body** (`application/octet-stream`) on a **separate Axum
    port**. Rationale: (a) the only consumer is the internal live traffic
    module — single unary call with a binary blob doesn't warrant gRPC's
    tooling overhead (tonic, prost, protoc); (b) full vector (8 MB raw,
    ~2–3 MB gzipped for 2M edges) eliminates the server-side clone-and-merge
    step; (c) separate port isolates customization traffic from query traffic
    for fault tolerance; (d) same Axum framework on both ports — one less
    dependency. See Sections 4.5.3, 4.5.5, 9.

---

## 12. Audit Implementation Notes [AUDIT: added section]

### 12.1 `CchContext::customize()` must build a temporary `FirstOutGraph`

The engine's `customize()` requires a `&Graph` where `Graph: LinkIterGraph + EdgeRandomAccessGraph<Link> + Sync`. The implementation of
`CchContext::customize()` must construct a temporary `FirstOutGraph`:

```rust
impl CchContext {
    pub fn customize(&self) -> CustomizedBasic<'_, CCH> {
        let metric = FirstOutGraph::new(
            &self.graph.first_out[..],
            &self.graph.head[..],
            &self.baseline_weights[..],
        );
        rust_road_router::algo::customizable_contraction_hierarchy::customize(&self.cch, &metric)
    }
}
```

The temporary `FirstOutGraph` (metric) is only borrowed during `customize()`
execution — the returned `CustomizedBasic` borrows `&CCH`, NOT the metric. So
the temporary can be dropped safely after the call.

### 12.2 `update_weights` lifetime safety

`QueryEngine::update_weights(&mut self, weights)` creates a new
`CustomizedBasic<'a, CCH>` that borrows `self.context.cch`. Then
`Server::update()` uses `std::mem::swap` to replace the old customized data.
The old `CustomizedBasic` is dropped, and the new one takes its place — both
borrow the same `&'a CCH`. This is lifetime-safe but non-obvious; comment it
in the implementation.

---

## 13. Path Visualization & Coordinate Mapping

### 13.1 The problem

Query results from `rust_road_router` are `Vec<NodeId>` (normal graph) or
`Vec<NodeId>` where each node represents an original edge (line graph). Map
services like Mapbox need coordinate sequences — `[(lat, lng), ...]` — to draw
polylines.

### 13.2 Normal graph — intersection coordinates

Straightforward: each node has `latitude[node]` and `longitude[node]`. The
`QueryEngine` populates `QueryAnswer.coordinates` by mapping each path node to
its coordinates:

```rust
let coordinates: Vec<(f32, f32)> = path.iter()
    .map(|&node| (latitude[node as usize], longitude[node as usize]))
    .collect();
```

This produces intersection-to-intersection straight lines. The resulting
polyline is **geometrically approximate** — real roads curve between
intersections, but only intersection coordinates are stored in the graph.

### 13.3 Line graph — edge-to-coordinate translation

The line graph path is a sequence of original edge indices. Each line-graph
node's coordinate is the tail node of the corresponding original edge.
`LineGraphQueryEngine` populates coordinates as:

```rust
// Each path node → tail-node coordinate of the original edge
let mut coordinates: Vec<(f32, f32)> = path.iter()
    .map(|&lg_node| (lg_latitude[lg_node as usize], lg_longitude[lg_node as usize]))
    .collect();

// Append the HEAD node of the final edge (destination intersection)
if let Some(&last_edge) = path.last() {
    let dest_node = original_head[last_edge as usize];
    coordinates.push((original_latitude[dest_node as usize],
                      original_longitude[dest_node as usize]));
}
```

This requires `LineGraphCchContext` to hold the original graph's `head`,
`latitude`, and `longitude` arrays (already included in the struct definition
in Section 4.2.3).

### 13.4 Coordinate system boundary

`hanoi-core` returns all coordinates in **(lat, lng)** order — matching the
internal storage format (`latitude[i]`, `longitude[i]`).

The **server layer** is responsible for converting to the format required by
downstream consumers:

- **GeoJSON / Mapbox**: `[lng, lat]` order (note: reversed)
- **Leaflet**: `[lat, lng]` order (same as internal)
- **Google Maps**: `{ lat, lng }` object

This boundary is enforced by keeping `hanoi-core` coordinate-system-agnostic.
Each module handles only its own concerns.

### 13.5 Road-following geometry (future — Phase D)

The current graph stores only intersection coordinates. Real roads have
curvature between intersections. Two approaches for smoother visualization:

**Option 1: Mapbox Map Matching API** (recommended first step)

- Send the intersection coordinate sequence to Mapbox's Map Matching endpoint
- Mapbox returns the actual road geometry (intermediate shape points)
- Pros: No graph pipeline changes; works with existing data
- Cons: External API dependency; adds latency; requires Mapbox API key

**Option 2: Store OSM way geometries during graph generation**

- Modify `cch_generator` to preserve intermediate coordinates from OSM ways
- Store as an additional per-edge array (e.g., `geometry: Vec<Vec<(f32, f32)>>`)
- Pros: No external dependency; instant lookup
- Cons: Significant pipeline changes; increases graph size substantially

**Recommendation**: Start with Mapbox Map Matching (Option 1) for the initial
app integration. Evaluate Option 2 if latency or API cost becomes a concern.
This decision is deferred to when the app development team is involved.