# Line Graph Spatial Indexing and Snapping

How coordinate-based queries work on the turn-expanded (line) graph, where
"nodes" and "edges" have inverted semantics compared to a normal road graph.

## 1. Background: Normal Graph vs Line Graph

### Normal graph

| Concept     | Represents            |
| ----------- | --------------------- |
| **Node**    | Road intersection     |
| **Edge**    | Road segment (u → v)  |
| **Weight**  | Travel time of segment|

Querying: user gives coordinates → snap to nearest node → CCH query on node
IDs.

### Line graph (turn-expanded)

| Concept     | Represents                                          |
| ----------- | --------------------------------------------------- |
| **Node i**  | Original graph **edge i** (a road segment)          |
| **Edge i→j**| A legal **turn** from road segment i to segment j   |
| **Weight**  | `travel_time[i] + turn_cost(i, j)`                  |

The line graph is constructed by `generate_line_graph`:

```
For each original edge i (u → v):
    For each original edge j that starts from v:
        If turn i→j is not forbidden and not a u-turn:
            Create line-graph edge: node i → node j
            Weight = travel_time[i] + turn_cost(i, j)
```

**Fundamental identity: line graph node `i` ≡ original graph edge `i`.**

If the original graph has M edges, the line graph has M nodes.

### Coordinate assignment

Each line graph node gets the **tail intersection's coordinates** of the
original edge it represents:

```
line_graph.latitude[i]  = original.latitude[tail_of_edge_i]
line_graph.longitude[i] = original.longitude[tail_of_edge_i]
```

This means:

- Line graph nodes have geographic positions (the start of their road segment)
- But they don't represent intersections — they represent road segments
  anchored at their starting point

## 2. Spatial Index Construction

```rust
// line_graph.rs — LineGraphQueryEngine::new()
let spatial = SpatialIndex::build(
    &context.graph.latitude,      // line graph node coords
    &context.graph.longitude,
    &context.graph.first_out,     // line graph CSR
    &context.graph.head,
);
```

The spatial index is built on the **line graph's own CSR**. This means:

| SpatialIndex concept | In context of line graph      | In context of original graph  |
| -------------------- | ----------------------------- | ----------------------------- |
| KD-tree points       | Line graph node positions     | Original edge tail positions  |
| `first_out` / `head` | Line graph adjacency          | Turn connectivity             |
| "snap_to_edge" edge  | A line-graph edge (= a turn)  | A transition between segments |
| Edge tail/head       | Line graph node IDs           | Original edge indices         |

## 3. Snap-to-Edge Algorithm

`SpatialIndex::snap_to_edge(lat, lng)` runs a two-phase algorithm:

### Phase 1: KD-tree nearest-node lookup

```rust
let nearest = self.tree.nearest_n::<SquaredEuclidean>(&query_point, k); // k = 10
```

Finds the 10 nearest **line graph nodes** to the query point. Since node
coordinates are original edge tail positions, this finds road segments whose
starting intersections are close to the query.

### Phase 2: Haversine edge refinement

For each of the 10 nearest nodes, iterate all **outgoing edges** in the CSR
(= turns to subsequent road segments). For each candidate edge, compute the
Haversine perpendicular distance from the query point to the line segment
between the two endpoints:

```rust
for edge_idx in start..end {
    let tail_node = node;                    // line graph node = original edge
    let head_node = self.head[edge_idx];     // another line graph node

    let (dist, t) = haversine_perpendicular_distance_with_t(
        query_lat, query_lng,
        lat[tail_node], lng[tail_node],      // start of segment
        lat[head_node], lng[head_node],      // start of next segment
    );
}
```

The returned `SnapResult` contains:

```rust
SnapResult {
    edge_id: EdgeId,   // CSR edge index = line graph edge (a turn)
    tail: NodeId,      // line graph node = original road segment
    head: NodeId,      // line graph node = next road segment
    t: f64,            // 0.0 = at tail, 1.0 = at head
}
```

### What `t` means in the line graph context

The projection parameter `t` tells us where along the "line" between two road
segment starting points the query coordinate falls:

- `t < 0.5`: closer to `tail` (the starting road segment)
- `t ≥ 0.5`: closer to `head` (the next road segment after the turn)

This is used to prioritize which endpoint to try first in the CCH query.

## 4. The edge_id vs node_id Distinction (Bug Fix)

### The bug

The original implementation passed `snap.edge_id` directly to the CCH query:

```rust
// WRONG: edge_id is a line-graph CSR edge index (a turn), not a node ID
self.query(src_snap.edge_id, dst_snap.edge_id)
```

But `query()` expects **line graph node IDs** (which are original edge indices):

```rust
pub fn query(&mut self, source_edge: EdgeId, target_edge: EdgeId) -> Option<QueryAnswer> {
    let result = self.server.query(Query {
        from: source_edge as NodeId,  // expects a line graph node ID
        to: target_edge as NodeId,
    });
```

A line-graph CSR edge index (position in `head[]`) is NOT the same as a line
graph node ID. The CSR edge index identifies a *turn* in the line graph,
while the node ID identifies a *road segment*.

**Example**: If the line graph has 200,000 nodes and 500,000 edges, a CSR edge
index could be anywhere in `0..500,000`, but node IDs only go up to `199,999`.
Passing an edge index ≥ 200,000 as a node ID would cause an out-of-bounds
access or query a completely wrong node.

Even for edge indices within node ID range, the semantics are wrong: edge index
42 in the CSR represents a specific turn, while node 42 represents a specific
road segment — these are unrelated entities.

### The fix

Use `tail` and `head` from the snap result — these are actual line graph
node IDs:

```rust
// CORRECT: use nearest_node() which returns tail or head based on t
let s = src_snap.nearest_node();
let d = dst_snap.nearest_node();
self.query(s, d)
```

The fallback path was also fixed. The old `collect_candidate_edges_prioritized`
collected CSR edge indices. The new `collect_candidate_nodes_prioritized`
collects line graph node IDs (the neighbors reachable via outgoing edges):

```rust
fn collect_candidate_nodes_prioritized(&self, snap: &SnapResult) -> Vec<NodeId> {
    let (first_node, second_node) = if snap.t < 0.5 {
        (snap.tail, snap.head)
    } else {
        (snap.head, snap.tail)
    };
    let mut nodes = vec![first_node, second_node];

    // Add neighbors reachable from each endpoint
    for (_, _, neighbor) in self.spatial.edges_incident_to(first_node) { ... }
    for (_, _, neighbor) in self.spatial.edges_incident_to(second_node) { ... }
    nodes
}
```

### Contrast with normal graph

The normal graph's `query_coords` was already correct:

```rust
// cch.rs — normal graph
let s = src.nearest_node();   // returns a graph node ID
let d = dst.nearest_node();
self.query(s, d)              // queries by node IDs ✓
```

The line graph code should have followed the same pattern, adjusted for
the fact that "nearest node" in the line graph is a road segment, not an
intersection — but the CCH still routes on node IDs either way.

## 5. Complete Query Flow

### Step-by-step for `query_coords(from, to)`

```
User coordinate: (21.028°N, 105.834°E)
        │
        ▼
┌─────────────────────────────────────┐
│ 1. KD-tree: find 10 nearest        │
│    line-graph nodes (road segments  │
│    whose tail intersections are     │
│    close to the query point)        │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│ 2. For each nearby node, check its  │
│    outgoing edges (turns). Compute  │
│    Haversine perpendicular distance │
│    to the line between tail/head    │
│    node coordinates.                │
│                                     │
│    Returns: SnapResult with         │
│    tail=lg_node_A, head=lg_node_B,  │
│    t=0.3 (closer to A)             │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│ 3. Primary query:                   │
│    nearest_node(t=0.3) → lg_node_A  │
│    CCH query: A → D                 │
│    (where D = dst snap's nearest)   │
└──────────────┬──────────────────────┘
               │ if no path found
               ▼
┌─────────────────────────────────────┐
│ 4. Fallback: collect candidates     │
│    from both endpoints of snap +    │
│    their neighbors. Try all pairs.  │
│    Keep shortest result.            │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│ 5. Raw CCH result: sequence of      │
│    line graph node IDs              │
│    [lg_node_7, lg_node_15, ...,     │
│     lg_node_42]                     │
│    Each = an original edge index    │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│ 6. Path mapping to intersections:   │
│    lg_node_i → original_tail[i]     │
│    (source intersection of edge i)  │
│                                     │
│    + append original_head[42]       │
│    (destination of final edge)      │
│                                     │
│    Result: intersection node IDs    │
│    [tail_7, tail_15, ..., head_42]  │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│ 6b. Coordinate mapping:            │
│     Each intersection node →        │
│     (original_lat, original_lng)    │
│                                     │
│     path and coordinates arrays     │
│     are aligned (same length)       │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│ 7. Final-edge correction:          │
│    distance += original_travel_time │
│    [target_edge]                    │
│                                     │
│    (CCH distance covers arriving   │
│    at the target road segment but   │
│    not traversing it)               │
└─────────────────────────────────────┘
```

### Why final-edge correction?

Line graph edge weights encode `travel_time[i] + turn_cost(i, j)` — the cost
to traverse road segment `i` and then make the turn onto segment `j`. This
means the CCH distance from source to target covers:

- Traversing the source segment
- All intermediate turns and segments
- The turn *onto* the target segment

But it does **not** include traversing the target segment itself. So we add
`original_travel_time[target_edge]` to get the true end-to-end travel time.

## 6. Path Result Mapping

The CCH query returns a path of **line graph node IDs** (original edge
indices). To produce an API-friendly result, these must be mapped back to
**original intersection node IDs**.

### The `original_tail` array

The original graph's `first_out` CSR array is loaded at startup and used to
reconstruct a `tail` array:

```rust
// tail[edge_i] = the node whose adjacency list contains edge i
let mut original_tail = Vec::with_capacity(num_original_edges);
for node in 0..(original_first_out.len() - 1) {
    let degree = (original_first_out[node + 1] - original_first_out[node]) as usize;
    for _ in 0..degree {
        original_tail.push(node as NodeId);
    }
}
```

Combined with the already-loaded `original_head`, we now have both endpoints of
every original edge:

```
original edge i:  original_tail[i] ──────→ original_head[i]
                   (source node)            (target node)
```

### Mapping algorithm

```
CCH path (line graph nodes):  [lg_7, lg_15, lg_23, lg_42]

Map each → tail of corresponding original edge:
  original_tail[7]  = node 103
  original_tail[15] = node 107
  original_tail[23] = node 112
  original_tail[42] = node 118

Append head of final original edge:
  original_head[42] = node 125

Result path (intersection IDs): [103, 107, 112, 118, 125]
Coordinates:                     [(lat,lng) for each intersection]
```

Both `path` and `coordinates` arrays have the same length (`n+1` for an
`n`-node CCH path). This matches the normal graph's output format exactly.

### API consistency

The server's `answer_to_response` function produces the same JSON structure
regardless of graph type:

```json
{
  "distance_ms": 12345,
  "path_nodes": [103, 107, 112, 118, 125],
  "coordinates": [[21.02, 105.83], [21.03, 105.84], ...]
}
```

API consumers cannot distinguish a line graph result from a normal graph result
— both return intersection node IDs and aligned coordinate arrays.

## 7. Comparison: Normal vs Line Graph Snapping

| Aspect                   | Normal graph                        | Line graph                             |
| ------------------------ | ----------------------------------- | -------------------------------------- |
| KD-tree built on         | Intersection coordinates            | Road segment start coordinates         |
| `snap_to_edge` finds     | Nearest road segment                | Nearest turn (line-graph edge)         |
| `tail`/`head` are        | Intersection node IDs               | Line-graph node IDs (= orig edge IDs) |
| CCH query uses           | `nearest_node()` → intersection ID  | `nearest_node()` → orig edge ID       |
| Fallback candidates      | Both endpoints of snapped edge      | Both endpoints + their LG neighbors    |
| Path output              | Intersection IDs → coords directly  | LG nodes → original_tail + final head |
| Distance correction      | None needed                         | Add target edge's travel_time          |

## 7. Edge Cases

### One-way streets

If the nearest road segment is one-way in the wrong direction, the primary
query may fail. The fallback tries neighboring road segments (via
`collect_candidate_nodes_prioritized`), which includes segments reachable from
the other endpoint of the snapped turn.

### Zero-length edges

If an original edge has zero length (tail == head coordinates), the Haversine
perpendicular distance calculation handles this via the `len_sq < 1e-20` guard,
falling back to point-to-point distance with `t = 0.0`.

### Query point far from any road

The KD-tree always returns results (the k nearest nodes), so there's no
"too far" check. The snap will find the geographically closest road segment
even if it's kilometers away. This is a trade-off: no query ever fails due to
distance, but results may be nonsensical for coordinates far from the road
network.
