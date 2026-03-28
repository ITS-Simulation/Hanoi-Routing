# CCH Deep Dive: Data Structures, Elimination Tree & Triangular Relaxation

> **Audience**: Someone who wants to _understand_ the algorithm, not just use it.
> This document explains _what_ each data structure means geometrically, _why_
> each transformation happens, and how the pieces connect from raw map data to
> sub-millisecond shortest-path queries.
>
> For the operational pipeline reference (commands, code snippets, build steps),
> see [CCH Walkthrough.md](CCH%20Walkthrough.md).

---

## Table of Contents

1. [The Big Picture](#1-the-big-picture)
2. [Stage 0 — The Road Network as a CSR Graph](#2-stage-0--the-road-network-as-a-csr-graph)
3. [Stage 1 — Node Ordering: Why It Matters](#3-stage-1--node-ordering-why-it-matters)
4. [Stage 2 — Contraction: Building the Chordal Supergraph](#4-stage-2--contraction-building-the-chordal-supergraph)
5. [Stage 3 — The Elimination Tree](#5-stage-3--the-elimination-tree)
6. [Stage 4 — Customization: Triangular Relaxation](#6-stage-4--customization-triangular-relaxation)
7. [Stage 5 — Query: Bidirectional Elimination Tree Walk](#7-stage-5--query-bidirectional-elimination-tree-walk)
8. [Stage 6 — Path Unpacking](#8-stage-6--path-unpacking)
9. [Complete Data Structure Reference](#9-complete-data-structure-reference)
10. [Visual Summary: The Full Transformation Pipeline](#10-visual-summary-the-full-transformation-pipeline)

---

## 1. The Big Picture

CCH solves one problem: **answer shortest-path queries on a road network in
sub-millisecond time, even when edge weights change frequently**.

The insight is to split the work into three phases with vastly different costs:

```
Phase           Depends on        Cost        Frequency
─────────────── ────────────────  ──────────  ──────────────────
1. Contraction  Topology only     ~5-10 min   Once per graph
2. Customization Edge weights     ~1 second   Every weight update
3. Query        Source & target   <1 ms       Every user request
```

The key question: **how do you preprocess a graph so that weight changes only
cost ~1 second, not minutes?**

Answer: separate _which shortcuts exist_ (topology, fixed) from _what they
weigh_ (metric, changeable). Contraction determines the structure. Customization
fills in the numbers. The query exploits both.

---

## 2. Stage 0 — The Road Network as a CSR Graph

### What the map looks like in memory

A road network with **n** nodes and **m** directed edges is stored as three
arrays in **Compressed Sparse Row (CSR)** format:

```
first_out:  [0, 2, 5, 7, 9, ...]     length = n + 1
head:       [3, 7, 0, 4, 5, 1, 6, ...] length = m
weight:     [4200, 1800, 4200, ...]   length = m  (milliseconds)
```

**How to read it**: Node `v`'s outgoing edges occupy indices
`first_out[v]..first_out[v+1]` in the `head` and `weight` arrays.

```
Example: 5 nodes, 8 edges

Node 0 ──→ Node 1 (3000ms)     first_out = [0, 2, 4, 6, 7, 8]
       ──→ Node 3 (5000ms)     head      = [1, 3, 2, 4, 0, 3, 4, 2]
Node 1 ──→ Node 2 (2000ms)     weight    = [3000, 5000, 2000, 4000,
       ──→ Node 4 (4000ms)                  3000, 1000, 2000, 2000]
Node 2 ──→ Node 0 (3000ms)
       ──→ Node 3 (1000ms)
Node 3 ──→ Node 4 (2000ms)
Node 4 ──→ Node 2 (2000ms)

Reading node 1's edges:
  first_out[1] = 2, first_out[2] = 4
  → edges at indices [2, 3]
  → head[2]=2, head[3]=4  (node 1 connects to nodes 2 and 4)
  → weight[2]=2000, weight[3]=4000
```

### Why CSR?

- **Cache-friendly**: All edges from one node are contiguous in memory
- **Compact**: Only 3 arrays, no pointers or linked lists
- **O(1) degree**: `degree(v) = first_out[v+1] - first_out[v]`
- **On-disk format**: RoutingKit stores each array as a raw binary file
  (no headers, just packed values)

### The Rust type

```rust
// engine/src/datastr/graph/first_out_graph.rs
pub struct FirstOutGraph<FirstOutContainer, HeadContainer, WeightContainer> {
    first_out: FirstOutContainer,  // &[u32] or Vec<u32>
    head:      HeadContainer,      // &[u32] or Vec<u32>
    weight:    WeightContainer,    // &[u32] or Vec<u32>
}
```

### What the files on disk look like

```
graph_dir/
  first_out     ← Vec<u32>, n+1 entries, little-endian
  head          ← Vec<u32>, m entries
  travel_time   ← Vec<u32>, m entries (milliseconds)
  latitude      ← Vec<f32>, n entries
  longitude     ← Vec<f32>, n entries
```

Each file is a raw memory dump. File size = element_count × 4 bytes. No headers,
no delimiters. This makes loading trivial: `mmap` or `read_exact` into a typed
buffer.

---

## 3. Stage 1 — Node Ordering: Why It Matters

### The problem

Contraction Hierarchies work by **eliminating nodes one at a time**, from least
important to most important. The order in which you eliminate them determines:

- How many shortcut edges get created (fewer = better)
- How tall the elimination tree is (shorter = faster queries)
- Whether the structure admits parallelism (nested dissection = yes)

### What the ordering is

A **node order** is a permutation that assigns each node a **rank**:

```
NodeOrder {
    node_order: [4, 2, 0, 1, 3]   // node_order[rank] = node_id
    ranks:      [2, 3, 1, 4, 0]   // ranks[node_id]   = rank
}
```

- Rank 0 = least important, contracted first
- Rank n-1 = most important, contracted last
- The permutation and its inverse are stored together for O(1) lookup

### Nested dissection ordering

The ordering used by CCH comes from **nested dissection** — a recursive
partitioning strategy:

```
Step 1: Find a small "separator" S that splits the graph into two halves A and B
        (no edges between A and B except through S)

Step 2: Recursively partition A and B

Step 3: Assign ranks: A's nodes first, then B's nodes, then S's nodes

                    ┌─────────────────────────────────┐
                    │         Separator S              │  ← highest ranks
                    │        (rank 8–9)                │
                    ├────────────────┬────────────────┤
                    │    Cell A      │    Cell B       │
                    │   (rank 0–3)   │   (rank 4–7)   │
                    │                │                 │
                    │  ┌──────────┐  │  ┌───────────┐ │
                    │  │ sep a    │  │  │  sep b    │ │
                    │  │(rank 2-3)│  │  │ (rank 6-7)│ │
                    │  ├────┬─────┤  │  ├─────┬─────┤ │
                    │  │A.a │A.b  │  │  │B.a  │B.b  │ │
                    │  │0-1 │     │  │  │4-5  │     │ │
                    │  └────┴─────┘  │  └─────┴─────┘ │
                    └────────────────┴────────────────┘
```

**Why nested dissection?**

1. **Few shortcuts**: Separator nodes are high-rank, so contracting low-rank
   cell nodes only creates shortcuts within the cell — never across cells.
2. **Parallel customization**: Cells A and B share no edges (except through S),
   so they can be customized independently on separate CPU cores.
3. **Logarithmic tree depth**: The elimination tree has O(log n) depth for
   planar graphs, giving O(log n) query time.

### How it's computed

[InertialFlowCutter](https://github.com/kit-algo/InertialFlowCutter) uses
geography-aware min-cuts to find separators. It reads the graph topology +
coordinates and outputs a permutation file `cch_perm`.

```
cch_perm: Vec<u32>  —  cch_perm[rank] = original_node_id
```

### A Concrete Example: Nested Dissection on an 8-Node Graph

To build intuition for what IFC produces, we trace its nested dissection on a
small directed graph, showing how it symmetrizes, finds node separators via
max-flow, and assigns ranks.

#### The Directed Input Graph

```
18 directed edges:
A→B(4)   B→E(1)   D→G(6)   E→F(5)   F→H(3)   H→G(7)
A→C(2)   B→D(5)   E→D(2)   E→G(4)   G→H(7)
C→E(6)   C→F(8)   E→C(6)   F→C(2)   G→E(8)
C→B(1)   D→B(3)
```

#### Step 0: Symmetrization via `add_back_arcs`

IFC requires a symmetric (undirected) graph. The pipeline script
`flow_cutter_cch_order.sh` runs `add_back_arcs` before IFC starts, ensuring every
arc u→v has a matching v→u. This adds 8 missing reverse arcs:

```
Already symmetric pairs (no change):
  B↔D, C↔E, C↔F, E↔G, G↔H

Missing reverses added:
  B→A (for A→B)    C→A (for A→C)    B→C (for C→B)    E→B (for B→E)
  D→E (for E→D)    F→E (for E→F)    G→D (for D→G)    H→F (for F→H)
```

**Result: 18 + 8 = 26 directed arcs (13 undirected edges × 2)**

```
Undirected adjacency (what IFC sees):
A: {B, C}              degree 2
B: {A, C, D, E}        degree 4
C: {A, B, E, F}        degree 4
D: {B, E, G}           degree 3
E: {B, C, D, F, G}     degree 5  ← still the hub
F: {C, E, H}           degree 3
G: {D, E, H}           degree 3
H: {F, G}              degree 2
```

> **Why is symmetrization safe?** IFC only outputs a node ordering (`cch_perm`).
> The actual CCH contraction and customization later operate on the original
> directed graph with real weights. The symmetric copy is discarded after ordering.

#### Step 1: Node-Splitting for Min-Node-Cut

IFC needs to find a minimum **node** separator (not edge cut). It converts the
problem into an edge-cut problem by splitting each node into two halves connected
by a capacity-1 bottleneck:

```
Original node v  →  v_in ──[cap 1]──→ v_out     (the bottleneck)
                    v_out ──[cap 0]──→ v_in      (reverse for residual graph)

Original edge u─v  →  u_out ──[cap 1]──→ v_in
                       v_in  ──[cap 0]──→ u_out   (reverse)
                       v_out ──[cap 1]──→ u_in
                       u_in  ──[cap 0]──→ v_out   (reverse)

Expanded graph: 16 nodes (8 × 2), 68 arcs (26×2 inter + 8×2 intra)
```

Any flow passing through node v must cross the intra-arc `v_in→v_out` (capacity
1). A saturated intra-arc means "this node is fully used." A min-edge-cut in the
expanded graph directly corresponds to a min-node-separator in the original.

#### Step 2: Geographic Partitioning (8 Directions)

IFC creates `geo_pos_ordering_cutter_count` cutters (default **8** in the
pipeline). Each cutter projects node coordinates onto a different axis:

```
projection(v) = lat(v) × cos(φ) + lon(v) × sin(φ)

For 8 cutters: φ = 0°, 22.5°, 45°, 67.5°, 90°, 112.5°, 135°, 157.5°

Roughly:  ─ horizontal,  ╲ shallow diagonal, ╲ diagonal,
          │ vertical,     ╱ anti-diagonal, etc.
```

For each direction, IFC sorts nodes by projection value and selects extremes as
initial source/target candidates. Each cutter then **incrementally grows** its
source and target sets by "piercing" one node at a time from the cut frontier.

All 8 cutters race in parallel. After each pierce step, the cutter evaluates its
current cut quality using the score `cut_size / smaller_side_size`. The best
cut seen across all cutters (lowest score, subject to balance constraints) wins.

#### Step 3: Ford-Fulkerson on the Expanded Graph (Latitude Axis)

Using the spatial layout from the original drawing:

```
        A(0,0)          B(2,0)
                                     D(4,1)
             C(1,2)     E(2,2)

        F(0,4)          G(2,4)       H(4,4)
```

To trace one cutter in detail, we pick the **latitude axis** (φ = 0°,
projection = latitude). Sorted top-to-bottom: A(0), B(0), D(1), C(2), E(2),
F(4), G(4), H(4).

After initial piercing and growth, suppose the source set is {A, B} and the
target set is {F, G, H}. Ford-Fulkerson now finds augmenting paths on the
**expanded** graph (16 nodes). We trace this at the original-node level, noting
which intra-arcs are saturated:

**Iteration 1 — Find augmenting path via DFS from source set**:

```
DFS from source {A,B}:
  A_out → C_in  (inter-arc, cap 1, unsaturated ✓)
  C_in  → C_out (intra-arc, cap 1, unsaturated ✓)  ← node C consumed
  C_out → F_in  (inter-arc, cap 1, unsaturated ✓)
  F_in  → F_out (intra-arc, cap 1, unsaturated ✓)  ← node F consumed, F is target

Path found: A → C → F
Mark saturated: A_out→C_in, C_in→C_out, C_out→F_in, F_in→F_out
Flow intensity: 1
```

**Iteration 2 — Find another augmenting path**:

```
DFS from source {A,B}:
  A_out → C_in  (SATURATED, skip)
  B_out → E_in  (inter-arc, cap 1, unsaturated ✓)
  E_in  → E_out (intra-arc, cap 1, unsaturated ✓)  ← node E consumed
  E_out → G_in  (inter-arc, cap 1, unsaturated ✓)
  G_in  → G_out (intra-arc, cap 1, unsaturated ✓)  ← node G consumed, G is target

Path found: B → E → G
Mark saturated: B_out→E_in, E_in→E_out, E_out→G_in, G_in→G_out
Flow intensity: 2
```

**Iteration 3 — Search for another augmenting path**:

```
DFS from source {A,B}:
  From A_out: A_out→C_in (SATURATED), A_out→B_in (B is source, skip)
  From B_out: B_out→A_in (A is source, skip),
              B_out→C_in (SATURATED via residual? no, B_out→C_in is a different
                          inter-arc from A_out→C_in — check: is B_out→C_in
                          saturated? No! B—C edge is separate from A—C edge)
  B_out → C_in  (inter-arc for edge B-C, cap 1, unsaturated ✓)
  C_in  → C_out (SATURATED from iteration 1)

  Dead end. Try residual arc: C_out → C_in has cap 1 (residual of saturated
  C_in→C_out). But C_out→C_in leads backward — no forward progress toward target.

  Try other neighbors:
  B_out → D_in  (inter-arc, cap 1, unsaturated ✓)
  D_in  → D_out (intra-arc, cap 1, unsaturated ✓)  ← node D consumed
  D_out → E_in  (inter-arc for edge D-E, cap 1, unsaturated ✓)
  E_in  → E_out (SATURATED from iteration 2)

  Dead end at E. Try D_out→G_in:
  D_out → G_in  (inter-arc for edge D-G, cap 1, unsaturated ✓)
  G_in  → G_out (SATURATED from iteration 2)

  Dead end at G. Try D_out→B_in (back to source, skip).

  All paths from {A,B} are exhausted. No augmenting path exists.
  Flow intensity remains: 2
```

**Max flow = 2. Ford-Fulkerson terminates.**

**Separator extraction from the cut**:

The saturated arcs that cross the cut boundary are the min-cut. IFC inspects each
saturated arc in the expanded graph:

```
Saturated intra-arcs (node cuts):
  C_in → C_out  →  original node C  →  separator member
  E_in → E_out  →  original node E  →  separator member

Saturated inter-arcs: A_out→C_in, C_out→F_in, B_out→E_in, E_out→G_in
  (these cross the cut but are inter-arcs, not intra-arcs — the intra-arcs
   are the actual bottlenecks that define the node separator)
```

**Result**:
```
Source side:     {A, B, D}     (reachable from source in residual graph)
Separator:       {C, E}        (min-node-cut, flow = 2)
Target side:     {F, G, H}     (reachable from target in residual graph)
```

D ends up on the source side because D_in→D_out is unsaturated — D was explored
but never consumed as a bottleneck. All of D's forward paths to the target were
blocked by the already-saturated E and G, so D remains reachable from source
without crossing any cut arcs.

#### Step 4: Rank Assignment (Level 0)

```
Left component:    {A, B, D}    → ranks 0–2    (contract first)
Separator:         {C, E}       → ranks 6–7    (contract last, highest ranks)
Right component:   {F, G, H}    → ranks 3–5
```

#### Step 5: Recursion on Left {A, B, D}

Undirected subgraph edges: A—B, B—D (a path graph).

**Min-node-cut**: {B} separates {A} from {D} — removing B disconnects the path.

```
{A}  → rank 0     (interior)
{D}  → rank 1     (interior)
{B}  → rank 2     (separator)
```

#### Step 6: Recursion on Right {F, G, H}

Undirected subgraph edges: F—H, G—H (a star centered on H).

**Min-node-cut**: {H} separates {F} from {G} — removing H disconnects them.

```
{F}  → rank 3     (interior)
{G}  → rank 4     (interior)
{H}  → rank 5     (separator)
```

#### Final Node Order

```
Rank  Node  Role                                         Recursion level
────  ────  ───────────────────────────────────────────  ───────────────
0     A     Interior of left                             Level 2
1     D     Interior of left                             Level 2
2     B     Separator of left (bridges A ↔ D)            Level 1
3     F     Interior of right                            Level 2
4     G     Interior of right                            Level 2
5     H     Separator of right (bridges F ↔ G)           Level 1
6     C     Top-level separator (bridges upper ↔ lower)  Level 0
7     E     Top-level separator (global hub)             Level 0
```

**`cch_perm`**: `[A, D, B, F, G, H, C, E]`

The recursion tree that produced this:

```
                        {A,B,C,D,E,F,G,H}
                       /       |         \
                  {A,B,D}    {C,E}      {F,G,H}
                  /  |  \   sep(6-7)    /  |  \
                {A} {B} {D}           {F} {H} {G}
                r0  r2  r1            r3  r5  r4
                   sep                   sep
```

#### Key Takeaways

1. **Symmetrization precedes everything**: The directed graph gains 8 reverse arcs
   via `add_back_arcs`. IFC never sees the directed version.

2. **Node-splitting converts node-cuts to edge-cuts**: The capacity-1 intra-arc
   `v_in→v_out` is the mechanism that lets standard max-flow find node separators.

3. **Separators get the highest ranks**: {C, E} are rank 6–7 because they are the
   top-level separator. They contract last, which is when cross-cluster shortcuts
   are created.

4. **Recursion preserves locality**: Within {A,B,D}, contracting A and D creates
   shortcuts only within this cluster. Cross-cluster shortcuts to {F,G,H} are
   deferred until the separator {C,E} contracts.

5. **Parallelism follows the tree**: {A,B,D} and {F,G,H} share no edges (only
   connected through {C,E}), so their customization can run on separate cores.

---

## 4. Stage 2 — Contraction: Building the Chordal Supergraph

### CH vs. CCH contraction — the fundamental difference

If you already understand classic **Contraction Hierarchies (CH)**, the natural
question is: "CCH also contracts nodes and creates shortcuts — so what's
different?" The difference is profound and explains everything about why CCH
exists.

#### CH contraction (weight-dependent)

In classic CH, contraction is **metric-dependent**. When contracting node `v`:

1. For every pair of neighbors `(u, w)`, run a **witness search** (a local
   Dijkstra) to check whether the shortest `u→w` path goes through `v`
2. Only add shortcut `u→w` if no witness (shorter alternative) exists
3. The order in which nodes are contracted is also chosen based on weights
   (heuristic: contract "unimportant" nodes that create few shortcuts first)

```
CH contracting node v:

    u ───── v ───── w        Does a shorter u→w path exist
            │                that does NOT use v?
            x
                             YES → no shortcut needed (witness found)
                             NO  → add shortcut u→w with weight = w(u,v) + w(v,w)
```

**Problem**: When weights change (traffic update, different vehicle profile),
the witness searches give different answers. Some shortcuts that were needed
before are now unnecessary. Some new ones are needed. **The entire contraction
must be redone from scratch** — a process that takes minutes.

#### CCH contraction (weight-independent)

CCH takes a radically different approach: **never look at weights during
contraction**. Instead:

1. The contraction order is fixed upfront by **nested dissection** (graph
   partitioning based on topology + geometry, not weights)
2. When contracting node `v`, **unconditionally connect all neighbor pairs** —
   no witness search, no weight checking
3. This creates MORE shortcuts than CH (some are unnecessary for the current
   metric), but the structure works for ALL metrics

```
CCH contracting node v:

    u ───── v ───── w        No witness search.
            │                ALWAYS add shortcut u→w.
            x                Weight = "to be determined later"
                             (filled in during customization)
```

**Tradeoff**:

| | CH | CCH |
|---|---|---|
| Shortcut count | Fewer (only needed ones) | More (all possible ones) |
| Contraction cost | ~5 min | ~5 min |
| Weight change cost | **~5 min (redo everything)** | **~1 sec (re-customize only)** |
| Query speed | Slightly faster (sparser graph) | Slightly slower (denser graph) |
| Order depends on | Weights + heuristics | Topology only |

For live traffic routing where weights change every few minutes, the CCH
tradeoff is overwhelmingly worth it.

#### How the nested dissection ordering shapes contraction

The ordering from InertialFlowCutter is not random — it's a **nested
dissection** ordering where cell-interior nodes have low ranks and separator
nodes have high ranks. This has a critical consequence for contraction:

```
Graph partitioned by nested dissection:

    ┌──── Cell A ────┐    S    ┌──── Cell B ────┐
    │                │  e   e  │                │
    │  a1  a2  a3    │  p   p  │  b1  b2  b3    │
    │                │  a   a  │                │
    │  a4  a5  a6    │  r   r  │  b4  b5  b6    │
    │                │  a   a  │                │
    └────────────────┘  t   t  └────────────────┘
                        o   o
                        r   r

    Ranks:  a1..a6 = 0..5     (low — contracted first)
            b1..b6 = 6..11    (low — contracted first)
            separators = 12+  (high — contracted last)
```

When contracting cell-interior nodes (low rank):
- Their neighbors are **other cell-interior nodes** or **separator nodes**
- All separator nodes have higher rank → they stay in the graph
- Shortcuts are created **within the cell or to separator nodes**
- **Crucially: no shortcuts are ever created between Cell A and Cell B**

This means contraction within Cell A is completely independent of Cell B.
The separator acts as a firewall: shortcuts cannot "leak" across the partition.

```
Contracting a1 (Cell A interior, low rank):

    Neighbors of a1: {a2, s1}  (a2 is a cell neighbor, s1 is a separator node)
    Lowest-ranked neighbor: a2
    Merge {s1} into a2's adjacency → shortcut a2──s1

    This shortcut stays WITHIN Cell A's boundary.
    Cell B is completely unaffected.
```

This locality property is what makes CCH customization parallelizable —
cells can be processed on different CPU cores with no synchronization until
you reach the separator level.

### What contraction does (the mechanism)

Starting from the original graph, contraction processes nodes in rank order
(lowest first). When a node is "contracted" (eliminated), its neighbors must
remain connected — so we add **shortcut edges** between them.

But CCH uses a specific optimization called the **FAST algorithm** (FAst and
Simple Triangulation) that avoids connecting ALL neighbor pairs. Instead, it
only merges the contracted node's neighbors into its **lowest-ranked neighbor**:

```
Standard contraction of v (connect all pairs):

    u₁ ─── v ─── u₂      →    u₁ ── u₂    (shortcut)
            │                  u₁ ── u₃    (shortcut)
            u₃                 u₂ ── u₃    (shortcut)
                               = 3 new edges for 3 neighbors

FAST algorithm contraction of v:

    u₁ ─── v ─── u₂      →    u₁ gets {u₂, u₃} merged into its adjacency
            │                  (u₁ is lowest-ranked neighbor)
            u₃
                               Only u₁──u₂ and u₁──u₃ are added
                               u₂──u₃ is NOT directly added (yet)
                               BUT: when u₁ is later contracted,
                               u₂ and u₃ will be connected then.
```

The FAST algorithm produces the same final result (a chordal supergraph) with
fewer intermediate operations. The key insight: connecting everything to the
**lowest neighbor** is sufficient because that neighbor will itself be
contracted later, propagating the connections upward.

The result is called the **chordal supergraph**: the original graph plus all
shortcuts that _any_ metric might need.

### Step-by-step example (using our 8-node graph)

Using the IFC ordering from Stage 1: `cch_perm = [A, D, B, F, G, H, C, E]`
(rank: A=0, D=1, B=2, F=3, G=4, H=5, C=6, E=7).

Contraction operates on the **original directed graph** (18 edges), but the CCH
stores edges as undirected pairs with separate up/down weights. For now we only
care about the topology — weights are filled in during customization (Stage 4).

```
Initial adjacency (sorted by rank within each neighbor list):

A(0): {B(2), C(6)}
D(1): {B(2), G(4), E(7)}
B(2): {D(1), G(4), C(6), E(7)}    ← D(1) is lower rank, rest are higher
F(3): {H(5), C(6), E(7)}
G(4): {D(1), B(2), H(5), E(7)}
H(5): {F(3), G(4)}
C(6): {A(0), B(2), F(3), E(7)}
E(7): {D(1), B(2), F(3), G(4), C(6)}
```

**Contract A (rank 0)**: neighbors = {B(2), C(6)}
- Lowest neighbor: B(2)
- Merge {C} into B → B──C already exists. No new edge.
- **parent(A) = B**

**Contract D (rank 1)**: neighbors = {B(2), G(4), E(7)}
- Lowest neighbor: B(2)
- Merge {G, E} into B → B──E exists, **B──G is NEW**
- **parent(D) = B**

**Contract B (rank 2)**: neighbors (excluding contracted A,D) = {G(4), C(6), E(7)}
- Lowest neighbor: G(4)
- Merge {C, E} into G → G──E exists, **G──C is NEW**
- **parent(B) = G**

**Contract F (rank 3)**: neighbors = {H(5), C(6), E(7)}
- Lowest neighbor: H(5)
- Merge {C, E} into H → **H──C is NEW**, **H──E is NEW**
- **parent(F) = H**

**Contract G (rank 4)**: neighbors (excluding B) = {H(5), C(6), E(7)}
- Lowest neighbor: H(5)
- Merge {C, E} into H → H──C, H──E already exist. No new edges.
- **parent(G) = H**

**Contract H (rank 5)**: neighbors (excluding F,G) = {C(6), E(7)}
- Lowest neighbor: C(6)
- Merge {E} into C → C──E already exists. No new edge.
- **parent(H) = C**

**Contract C (rank 6)**: neighbors (excluding A,B,F,H) = {E(7)}
- Only one neighbor. Nothing to merge.
- **parent(C) = E**

**E (rank 7)**: root — not contracted.

**Chordal supergraph**: 13 original + 4 shortcuts = **17 undirected edges**

```
Shortcuts created:
  B──G  (from contracting D)
  G──C  (from contracting B)
  H──C  (from contracting F)
  H──E  (from contracting F)
```

### The key insight: topology vs. weights

**Contraction never looks at edge weights**. It only asks: "are these nodes
connected?" The shortcuts it creates are _structural_ — they encode _possible_
paths, not their costs. The actual cost of each shortcut is determined later
during customization.

This is what makes CCH "customizable" — the expensive contraction step runs
once, and weight changes only require re-running the cheap customization.

### The algorithm in code

```rust
// contraction.rs — the core loop (simplified)
while let Some((node, mut subgraph)) = graph.remove_lowest() {
    if let Some((&lowest_neighbor, other_neighbors)) = node.edges.split_first() {
        // Merge all OTHER neighbors into the LOWEST neighbor's adjacency
        subgraph[lowest_neighbor].merge_neighbors(other_neighbors);
    }
}
```

This is the **FAST algorithm** (FAst and Simple Triangulation). The
`merge_neighbors` operation is a sorted merge — O(degree) per node. Practically
very fast, though theoretically not guaranteed linear.

### What "chordal" means

A graph is **chordal** if every cycle of length ≥ 4 has a **chord** (an edge
connecting two non-adjacent cycle vertices). The contraction process guarantees
this property.

Why we need chordality: it ensures that during customization, every shortest
path through the hierarchy can be decomposed into **triangles** — which is
exactly what triangular relaxation exploits.

---

## 5. Stage 3 — The Elimination Tree

### What it is — and what it is NOT

The **elimination tree** is a forest (usually a single tree for connected
graphs) that records the parent-child relationship created during contraction.

> **Common misconception**: The elimination tree is _not_ a representation of the
> road network. Two nodes connected by a road might sit in distant branches of
> the tree. Two nodes that are parent-child in the tree might not share a direct
> road at all.

Think of the elimination tree as a **contraction history** — a merge dendrogram
that records which node was "absorbed into" which other node during the
elimination process.

| Structure | Represents |
|---|---|
| CSR graph (`first_out`/`head`) | The actual road network (who connects to who) |
| Chordal supergraph | The road network **plus** all shortcut edges |
| **Elimination tree** | The **contraction history** — a parent-child hierarchy over the same node set, but with _completely different edges_ than the graph |

```rust
// mod.rs
elimination_tree: Vec<InRangeOption<NodeId>>
// elimination_tree[node] = Some(parent)  or  None (root)
```

### How it's built

When node `v` is contracted, its **lowest-ranked remaining neighbor** becomes
its **parent** in the elimination tree:

```rust
// mod.rs:117-122
fn build_elimination_tree(graph: &UnweightedOwnedGraph) -> Vec<InRangeOption<NodeId>> {
    (0..graph.num_nodes())
        .map(|node_id|
            graph.link_iter(node_id as NodeId)
                .map(|NodeIdT(n)| n)
                .next()  // first neighbor = lowest rank = parent
        )
        .map(InRangeOption::new)
        .collect()
}
```

Since edges in the chordal supergraph are stored sorted by rank, the **first
neighbor** of each node is always its lowest-ranked neighbor — which becomes its
parent.

### Visualizing the elimination tree

Using our 8-node graph (contraction order: A, D, B, F, G, H, C, E):

```
Parent assignments (from contraction in Stage 2):
  parent(A) = B     (A's lowest neighbor was B)
  parent(D) = B     (D's lowest neighbor was B)
  parent(B) = G     (B's lowest uncontracted neighbor was G)
  parent(F) = H     (F's lowest neighbor was H)
  parent(G) = H     (G's lowest uncontracted neighbor was H)
  parent(H) = C     (H's lowest uncontracted neighbor was C)
  parent(C) = E     (C's only uncontracted neighbor was E)
  parent(E) = None  (root — highest rank, last contracted)
```

```
Elimination tree:

     rank 7:      E       ← root (top-level separator, last contracted)
                  │
     rank 6:      C       ← top-level separator
                  │
     rank 5:      H       ← right-component separator
                 ╱ ╲
     rank 4,3:  G   F     ← right-component interiors
                │
     rank 2:    B         ← left-component separator
               ╱ ╲
     rank 0,1: A   D     ← left-component interiors (leaves)
```

Notice how the tree shape mirrors the nested dissection from Stage 1:
- Left cluster {A,D} are leaves under their separator B
- Right cluster {F,G} are leaves (G under H, F under H)
- Both clusters connect through separator C, then root E

### What is the root?

The root is the **last node contracted** — the node with the **highest rank**.
In a nested dissection ordering, this is a node on the **top-level separator**:
the small set of nodes whose removal splits the entire graph into two
roughly equal halves.

For the Hanoi road network, the root would be some intersection along a major
separator road — but not necessarily the busiest or most "important" road.
The root is chosen for its **graph-theoretic bisection quality**, not its traffic
significance. It's the node whose removal most evenly splits the remaining
graph.

If the graph has multiple disconnected components, you get a **forest** (multiple
roots) instead of a single tree.

### How the tree gets layered

The layering follows directly from the **contraction order** and the **nested
dissection** structure:

```
How the tree forms (bottom-up, during contraction):

  When node v is contracted:
    parent(v) = v's lowest-ranked remaining neighbor

  This means:
    - Nodes contracted EARLY (low rank) → deep in the tree (leaves)
    - Nodes contracted LATE (high rank) → near the root
    - The root = last node contracted = highest rank
```

The nested dissection ordering creates a natural correspondence between
**tree depth** and **geographic scope**:

```
    Tree level         What lives here            Geographic scope
    ─────────────────  ─────────────────────────  ──────────────────────
    Layer 0 (root)     Top separator              ~10 nodes — city bisector
    Layer 1            Sub-separators             ~40 nodes — district borders
    Layer 2            Sub-sub-separators         ~160 nodes
    ...                ...                        ...
    Layer k (leaves)   Interior cell nodes        ~100,000s — local side-streets
```

In real graphs, the tree is wide and shallow (not a chain like our toy example):

```
Real-world elimination tree (schematic, Hanoi ~500k nodes):

                           [root]                          ← top separator node
                          ╱      ╲
                    [sep_L]      [sep_R]                   ← splits Hanoi E/W
                   ╱    ╲        ╱    ╲
                 ...    ...    ...    ...                   ← district-level cells
                ╱╲  ╱╲            ╱╲  ╱╲
              ... ... ...       ... ... ...                ← neighborhood cells
              │││ │││ │││       │││ │││ │││               ← thousands of leaves
                                                              (alley intersections)

    Depth ≈ O(log n) ≈ 18-20 levels for 500k nodes
    Width at bottom ≈ thousands of independent cells
```

> **Key insight**: The tree shape is entirely determined by the **node ordering**
> (from InertialFlowCutter), not by edge weights. A different ordering produces a
> different tree with different depth and width — which is why good orderings
> matter so much for query performance.

### What the elimination tree means for routing

The elimination tree does _not_ encode road connections. It encodes **which
level of geographic granularity** each node belongs to:

- **Leaves** are hyper-local (alley intersections in a single neighborhood)
- **The root** is the global separator (a city-scale bisection point)
- **Walking upward** = zooming out from local to global scope

This directly determines query behavior:

```
source walks up:  side-street → neighborhood → district → city-half → root
target walks up:  side-street → neighborhood → district → city-half → root
                                                              ↑
                                                  They meet at some level.
                                              This level determines query cost.

Close-by queries:  meet at a low level → very fast (few nodes visited)
Cross-city queries: meet near the root → still fast (only ~20 levels exist)
```

The formal properties that make this work:

- **Ancestor relationship**: If node `u` is an ancestor of node `v` in the
  elimination tree, then `u` might appear on a shortest path involving `v`.
- **Independent subtrees**: If nodes `v` and `w` are in different subtrees with
  no common ancestor below the root, then no shortest path between them goes
  through each other's subtree.
- **Query path**: To find the shortest path between two nodes, you only need to
  walk from each node **upward** to their **lowest common ancestor** — nodes
  outside these two paths are irrelevant.

This is why queries are fast: instead of searching the entire graph (Dijkstra),
you walk up two branches of a tree.

### The separator tree (for parallelization)

The elimination tree is also converted to a **separator tree** for parallel
customization:

```rust
// separator_decomposition.rs
pub struct SeparatorTree {
    pub nodes: SeparatorNodes,         // The separator node IDs
    pub children: Vec<SeparatorTree>,  // Child components
    pub num_nodes: usize,              // Total nodes in this subtree
}
```

Long chains in the elimination tree (single-child sequences) get collapsed into
"separator" groups. The resulting tree matches the nested dissection structure:
leaf cells can be customized in parallel, synchronized only at separator
boundaries.

---

## 6. Stage 4 — Customization: Triangular Relaxation

This is the phase that runs every time weights change (~1 second). It fills
the chordal supergraph's shortcut edges with actual weights.

### The two sub-phases

#### Sub-phase A: Respecting (copy original weights)

For each CCH edge, look up which original graph edge(s) it corresponds to and
take the minimum weight:

```
For CCH edge e:
    upward_weight[e]   = min { original_weight[a] : a ∈ forward_orig_arcs(e) }
    downward_weight[e] = min { original_weight[a] : a ∈ backward_orig_arcs(e) }

If e has no original arc mapping: weight stays at INFINITY
(this means e is a pure shortcut, not an original edge)
```

After this step, original edges have correct weights, but shortcuts still have
INFINITY weights. Sub-phase B fixes the shortcuts.

#### Sub-phase B: Lower triangle relaxation

**The core insight**: Every shortcut edge was created because a lower-ranked
node was contracted. If there's a shortcut `u──v` created when node `x` (rank
lower than both `u` and `v`) was contracted, then the path `u→x→v` is a
candidate for the weight of that shortcut.

```
Triangle relaxation for shortcut u──v via lower node x:

       u ─────────── v        ← the shortcut we're computing
        ╲           ╱
         ╲         ╱
          ╲       ╱
           ╲     ╱
            ╲   ╱
              x                ← lower-ranked node (already contracted)

    upward_weight(u→v) = min( upward_weight(u→v),
                              downward_weight(u→x) + upward_weight(x→v) )

    downward_weight(u→v) = min( downward_weight(u→v),
                                upward_weight(u→x) + downward_weight(x→v) )
```

**"Upward"** and **"downward"** refer to direction relative to rank:
- **Upward** weight on edge `u──v` (where rank(u) < rank(v)): the cost of
  traveling from `u` toward `v` in the _original_ graph direction
- **Downward** weight: the cost of traveling from `v` toward `u`

Since the original graph is directed, the forward and backward costs of an
undirected CCH edge can differ.

#### Why the cross-pattern: downward + upward, upward + downward

The formulas mix upward and downward weights, which can be confusing at first.
The key is that the two-hop detour through `x` always **goes down then comes
back up** in the elimination ordering — and the two legs necessarily use
opposite weight arrays.

Consider the three nodes with `rank(x) < rank(u) < rank(v)`. The CCH stores
edges pointing low→high, so the stored edges are `x→u` and `x→v`. Each stored
edge carries two weights: upward (the real-world cost from low to high) and
downward (the real-world cost from high to low).

**Relaxing upward_weight(u→v)** — the real-world path from u to v through x:

```
Real-world path:  u ──→ x ──→ v
                    ↘       ↗
              (go DOWN    (go UP
              from u to x)  from x to v)

    Step 1: u → x in the real world
            u is higher than x, so this is going DOWN the ordering
            → read downward_weight on edge x──u, written as downward_weight(u→x)

    Step 2: x → v in the real world
            x is lower than v, so this is going UP the ordering
            → read upward_weight on edge x──v, written as upward_weight(x→v)

    Total = downward_weight(u→x) + upward_weight(x→v)
```

**Relaxing downward_weight(u→v)** — the real-world path from v to u through x:

```
Real-world path:  v ──→ x ──→ u
                    ↘       ↗
              (go DOWN    (go UP
              from v to x)  from x to u)

    Step 1: v → x in the real world
            v is higher than x, so this is going DOWN the ordering
            → read downward_weight on edge x──v, written as downward_weight(x→v)

    Step 2: x → u in the real world
            x is lower than u, so this is going UP the ordering
            → read upward_weight on edge x──u, written as upward_weight(u→x)

    Total = downward_weight(x→v) + upward_weight(u→x)
```

The pattern is always: **down to the intermediate, then up to the destination**.
This is not a coincidence — it's structural. The intermediate node `x` has the
lowest rank in the triangle, so any path through it must descend to reach `x`
and ascend to leave it.

Summary table for the edge `u──v` (where `rank(x) < rank(u) < rank(v)`):

```
Weight being      Real-world  Leg 1        Leg 2         Formula
relaxed           path        (down to x)  (up from x)
────────────────  ──────────  ───────────  ────────────  ──────────────────────
upward(u→v)       u → x → v  down(u→x)    up(x→v)      down(u→x) + up(x→v)
downward(u→v)     v → x → u  down(x→v)    up(u→x)      up(u→x)   + down(x→v)
```

### Why it works: the triangle inequality guarantee

The chordal supergraph has a key property: for every pair of adjacent nodes `u`
and `v`, **all** lower-ranked nodes that could be on a shortest path between
them are connected to both `u` and `v` in the supergraph. This means:

1. **We only need to check triangles (not longer paths).** Suppose the true
   shortest path from `u` to `v` goes through nodes `x₁, x₂, ..., xₖ`, all
   with rank lower than both `u` and `v`. The chordal completion guarantees
   that edges exist between every consecutive pair of these intermediate nodes
   _and_ between each of them and `u`/`v`. This means the multi-hop path
   `u → x₁ → x₂ → ... → xₖ → v` can be decomposed into a chain of triangles,
   each of which is relaxed independently. The bottom-up processing order
   ensures these compose correctly.

2. **Bottom-up processing guarantees correctness.** When we process node `v`
   (examine all triangles where `v` is the highest node), every edge between
   nodes of rank < rank(v) already has its final weight. This is because those
   edges were updated when their own highest-ranked endpoint was processed
   earlier. So the weights we read from the two lower legs of the triangle are
   already correct shortest-path distances — not intermediate approximations.

3. **A single pass suffices.** Unlike Bellman-Ford (which needs multiple rounds),
   CCH customization needs exactly one bottom-up pass. The elimination ordering
   provides a topological guarantee: no triangle's lower edges depend on any
   edge that hasn't been finalized yet.

After processing all nodes, every shortcut edge has a weight equal to the
shortest path it represents.

### Processing order and workspace

Nodes are processed in **increasing rank order**. For each node `v`:

```
1. Load direct weights into workspace:
   For each upward neighbor w of v:
       workspace_out[w] = upward_weight(v→w)
       workspace_in[w]  = downward_weight(v→w)

2. Enumerate lower triangles:
   For each lower neighbor u of v (via inverted graph):
       For each upward neighbor w of u (where w has rank > v):
           // Triangle: v ← u → w  (u is the low node)
           //
           //      v ─────────── w
           //       ╲           ╱
           //         u
           //
           relax workspace_out[w] with: upward(u→w) + downward(v→u)
           relax workspace_in[w]  with: downward(u→w) + upward(v→u)

3. Write back relaxed weights to CCH edges
```

### Concrete example (using our 8-node graph)

Continuing from Stage 2, the chordal supergraph has 17 undirected edges (13
original + 4 shortcuts). Each edge stores two weights: **upward** (lower rank →
higher rank in the original directed graph) and **downward** (higher → lower).

**Sub-phase A (Respecting)** — copy original directed weights:

```
Edge       Lower Higher  Up              Down             Source
─────────  ───── ──────  ──────────────  ───────────────  ────────
A(0)──B(2)   A     B     4 (A→B)        ∞                original
A(0)──C(6)   A     C     2 (A→C)        ∞                original
D(1)──B(2)   D     B     3 (D→B)        5 (B→D)          original
D(1)──G(4)   D     G     6 (D→G)        ∞                original
D(1)──E(7)   D     E     ∞              2 (E→D)          original
B(2)──G(4)   B     G     ∞              ∞                shortcut
B(2)──C(6)   B     C     ∞              1 (C→B)          original
B(2)──E(7)   B     E     1 (B→E)        ∞                original
F(3)──C(6)   F     C     2 (F→C)        8 (C→F)          original
F(3)──H(5)   F     H     3 (F→H)        ∞                original
F(3)──E(7)   F     E     ∞              5 (E→F)          original
G(4)──C(6)   G     C     ∞              ∞                shortcut
G(4)──H(5)   G     H     7 (G→H)        7 (H→G)          original
G(4)──E(7)   G     E     8 (G→E)        4 (E→G)          original
H(5)──C(6)   H     C     ∞              ∞                shortcut
H(5)──E(7)   H     E     ∞              ∞                shortcut
C(6)──E(7)   C     E     6 (C→E)        6 (E→C)          original
```

All 4 shortcuts start at (∞, ∞). Sub-phase B fills them in.

**Sub-phase B (Triangle relaxation)** — process nodes in rank order. For each
node v, find each lower neighbor u. For each of u's upward neighbors w with
rank(w) > rank(v), relax the v──w edge via the triangle v←u→w:

```
up(v→w)   = min( up(v→w),   up(u→w) + down(v→u) )
down(v→w) = min( down(v→w), down(u→w) + up(v→u) )
```

**Process A (rank 0), D (rank 1)**: No lower neighbors. Skip.

**Process B (rank 2)**: lower neighbors = {A(0), D(1)}

```
  via A(0) → A's upward neighbors above B: {C(6)}
    Triangle B←A→C:
      up(B→C)   = min(∞, up(A→C) + down(B→A))   = min(∞, 2+∞)  = ∞
      down(B→C) = min(1, down(A→C) + up(B→A))   = min(1, ∞+4)  = 1

  via D(1) → D's upward neighbors above B: {G(4), E(7)}
    Triangle B←D→G:
      up(B→G)   = min(∞, up(D→G) + down(B→D))   = min(∞, 6+5)  = 11  ★
      down(B→G) = min(∞, down(D→G) + up(B→D))   = min(∞, ∞+∞)  = ∞
    Triangle B←D→E:
      up(B→E)   = min(1, up(D→E) + down(B→D))   = min(1, ∞+5)  = 1
      down(B→E) = min(∞, down(D→E) + up(B→D))   = min(∞, 2+∞)  = ∞
```

  B──G gets its first real weight: **up = 11** (path B→D(5)→G(6) = 11).

**Process F (rank 3)**: lower neighbors = none (all neighbors rank > 3). Skip.

**Process G (rank 4)**: lower neighbors = {D(1), B(2)}

```
  via D(1) → D's upward neighbors above G: {E(7)}
    Triangle G←D→E:
      up(G→E)   = min(8, up(D→E) + down(G→D))   = min(8, ∞+∞)  = 8
      down(G→E) = min(4, down(D→E) + up(G→D))   = min(4, 2+∞)  = 4

  via B(2) → B's upward neighbors above G: {C(6), E(7)}
    Triangle G←B→C:
      up(G→C)   = min(∞, up(B→C) + down(G→B))   = min(∞, ∞+∞)  = ∞
      down(G→C) = min(∞, down(B→C) + up(G→B))   = min(∞, 1+11) = 12  ★
    Triangle G←B→E:
      up(G→E)   = min(8, up(B→E) + down(G→B))   = min(8, 1+∞)  = 8
      down(G→E) = min(4, down(B→E) + up(G→B))   = min(4, ∞+11) = 4
```

  G──C gets: **down = 12** (path C→B(1)→G(11) = 12).

**Process H (rank 5)**: lower neighbors = {F(3), G(4)}

```
  via F(3) → F's upward neighbors above H: {C(6), E(7)}
    Triangle H←F→C:
      up(H→C)   = min(∞, up(F→C) + down(H→F))   = min(∞, 2+∞)  = ∞
      down(H→C) = min(∞, down(F→C) + up(H→F))   = min(∞, 8+3)  = 11  ★
    Triangle H←F→E:
      up(H→E)   = min(∞, up(F→E) + down(H→F))   = min(∞, ∞+∞)  = ∞
      down(H→E) = min(∞, down(F→E) + up(H→F))   = min(∞, 5+3)  = 8   ★

  via G(4) → G's upward neighbors above H: {C(6), E(7)}
    Triangle H←G→C:
      up(H→C)   = min(∞, up(G→C) + down(H→G))   = min(∞, ∞+7)  = ∞
      down(H→C) = min(11, down(G→C) + up(H→G))   = min(11, 12+7) = 11
    Triangle H←G→E:
      up(H→E)   = min(∞, up(G→E) + down(H→G))   = min(∞, 8+7)  = 15  ★
      down(H→E) = min(8, down(G→E) + up(H→G))   = min(8, 4+7)  = 8
```

  H──C gets: **down = 11** (path C→F(8)+F→H(3) = 11).
  H──E gets: **up = 15** (G→E(8)+H→G(7)), **down = 8** (E→F(5)+F→H(3)).

**Process C (rank 6)**: lower neighbors = {A(0), B(2), F(3), G(4), H(5)}

```
  via B(2) → B's upward neighbors above C: {E(7)}
    Triangle C←B→E:
      up(C→E)   = min(6, up(B→E) + down(C→B))   = min(6, 1+1)  = 2   ★
      down(C→E) = min(6, down(B→E) + up(C→B))   = min(6, ∞+∞)  = 6

  via F(3), G(4), H(5): all triangles to E, but none improve on 2 or 6.
```

  C──E relaxed: **up = 2** (path C→B(1)+B→E(1) = 2, beating direct C→E = 6).

**Final customized weights**:

```
Edge        Up   Down  Shortcut path (if changed from original)
──────────  ───  ────  ─────────────────────────────────────────
A──B          4    ∞
A──C          2    ∞
D──B          3    5
D──G          6    ∞
D──E          ∞    2
B──G         11    ∞   B→D(5) + D→G(6)
B──C          ∞    1
B──E          1    ∞
F──C          2    8
F──H          3    ∞
F──E          ∞    5
G──C          ∞   12   C→B(1) + B→G(11)
G──H          7    7
G──E          8    4
H──C          ∞   11   C→F(8) + F→H(3)
H──E         15    8   up: G→E(8)+H→G(7), down: E→F(5)+F→H(3)
C──E          2    6   up: C→B(1)+B→E(1) (was 6)
```

### Parallelization via separator tree

The separator tree enables parallel customization:

```
                    Separator S  ← must be single-threaded
                   ╱             ╲
              Cell A              Cell B  ← can run in parallel!
             ╱      ╲           ╱      ╲
          Cell A.1  Cell A.2  Cell B.1  Cell B.2  ← 4-way parallel
```

Cells at the same level share no edges, so their triangle relaxations are
independent. The algorithm processes the tree bottom-up: parallelize cells
at each level, then synchronize at the separator above.

---

## 7. Stage 5 — Query: Bidirectional Elimination Tree Walk

### The idea

To find the shortest path from `source` to `target`:

1. Start at `source`, walk **up** the elimination tree to the root
2. Start at `target`, walk **up** the elimination tree to the root
3. **Both walks go all the way to the root** — they do NOT stop when they
   first intersect
4. After both walks finish, scan all nodes reached by both and pick the one
   that minimizes `fw_dist[node] + bw_dist[node]`

### Why not stop at the first intersection?

Unlike Dijkstra's bidirectional search (which processes nodes in distance order
and can stop at intersection), the CCH walk processes nodes in **rank order** —
which has no relationship to distance. The first shared node is not necessarily
on the shortest path.

Consider this scenario:

```
              root
             ╱    ╲
           ...    ...
           │        │
    fw →  [m₁]     [m₂]  ← bw     m₁ is the first intersection,
           │        │               but m₂ gives a shorter total!
          ...      ...
           │        │
       [source]  [target]
```

The forward walk might reach `m₁` via a long detour (fw_dist[m₁] = 50), while
a shorter route goes through `m₂` higher up (fw_dist[m₂] = 3, bw_dist[m₂] = 5,
total = 8). If we stopped at `m₁`, we'd miss the optimal path through `m₂`.

Walking to the root guarantees we consider **every** candidate meeting point.

### How the walk works

```
                      root         ← both walks end here
                     ╱    ╲
                   ...    ...
                   │        │
  forward walk →  ...      ...  ←  backward walk
                   │        │
               [source]  [target]

Walk rules:
  - Each walk follows parent pointers upward to the root
  - At each node visited, relax edges to ALL upward neighbors
    (not just the parent — all neighbors with higher rank)
  - Track tentative best: whenever a node has been reached by
    BOTH walks, check if fw_dist[node] + bw_dist[node] < best
  - The walk does NOT stop at the first meeting — continue to root
  - After both walks finish, the best tentative distance is the answer
```

### Edge relaxation during the walk

At each node the walk visits, it relaxes edges to **all** upward neighbors in
the chordal supergraph — not just the tree parent. The tree guides the order of
nodes visited, but the relaxation uses the full chordal supergraph adjacency:

```
Forward walk at node v:
  For each upward neighbor w in the chordal supergraph:
      fw_dist[w] = min(fw_dist[w], fw_dist[v] + upward_weight(v→w))

Backward walk at node v:
  For each upward neighbor w:
      bw_dist[w] = min(bw_dist[w], bw_dist[v] + downward_weight(v→w))
```

Note: the backward walk reads **downward** weights. This is because walking
"up" the tree from the target means traversing edges in reverse — what costs
`downward_weight` in the real-world direction (high → low) is what the backward
search needs.

### Why this is fast

- The elimination tree has depth O(log n) for planar graphs
- Each walk visits O(log n) nodes
- At each node, it relaxes O(degree) edges in the chordal supergraph
- Total: O(degree × log n) work per query
- In practice: a few hundred edge relaxations → sub-millisecond
- No priority queue needed — just follow parent pointers upward

### Concrete query: A → H (using our 8-node graph)

```
Elimination tree (for reference):

        E(7)         fw_dist  bw_dist
        │
        C(6)
        │
        H(5)  ← target
       ╱ ╲
      G(4) F(3)
      │
      B(2)
     ╱ ╲
    A(0) D(1)
    ↑
  source
```

**Forward walk** (from A, upward):

```
  Visit A(0):  fw[A]=0
    relax A→B(up=4):  fw[B] = 4
    relax A→C(up=2):  fw[C] = 2

  Visit B(2):  fw[B]=4     (parent of A)
    relax B→G(up=11): fw[G] = 4+11 = 15
    relax B→C(up=∞):  no improvement
    relax B→E(up=1):  fw[E] = 4+1 = 5

  Visit G(4):  fw[G]=15    (parent of B)
    relax G→H(up=7):  fw[H] = 15+7 = 22
    relax G→C(up=∞):  no improvement
    relax G→E(up=8):  fw[E] = min(5, 15+8) = 5

  Visit H(5):  fw[H]=22    (parent of G — also the target!)
    relax H→C(up=∞):  no improvement
    relax H→E(up=15): fw[E] = min(5, 22+15) = 5

  Visit C(6):  fw[C]=2     (parent of H)
    relax C→E(up=2):  fw[E] = min(5, 2+2) = 4  ★ improved!

  Visit E(7):  root. Done.
```

**Backward walk** (from H, upward, using downward weights):

```
  Visit H(5):  bw[H]=0
    relax H→C(down=11): bw[C] = 11
    relax H→E(down=8):  bw[E] = 8

  Visit C(6):  bw[C]=11   (parent of H)
    relax C→E(down=6):  bw[E] = min(8, 11+6) = 8

  Visit E(7):  root. Done.
```

**Both walks reached the root E(7).** Now scan every node visited by both:

```
  Node  fw_dist  bw_dist  Total  Why not optimal?
  ────  ───────  ───────  ─────  ────────────────────────────────
  H       22       0       22   Forward took a long detour via G
  C        2      11       13   Backward path C→H is expensive
  E        4       8       12   ★ best meeting point
```

If we had stopped at H (the first node reached by both walks), we would have
reported distance 22 — nearly double the true shortest path of 12.

**Shortest distance A→H = 12.**

The actual path (to be unpacked in Stage 6):
  fw path to E: A→C(2) → C→E(up=2, which is the shortcut C→B→E)
  bw path from E: E→H(down=8, which is the shortcut E→F→H)
  Full: A → C → B → E → F → H = 2 + 1 + 1 + 5 + 3 = **12** ✓

### Pruning

If `fw_dist[node] ≥ best_so_far`, the forward walk skips that node's edge
relaxation (`skip_next` in the code). This avoids wasting time on clearly
suboptimal branches.

---

## 8. Stage 6 — Path Unpacking

### The problem

The query finds a path through the **chordal supergraph**, which includes
shortcuts. These shortcuts don't correspond to real roads — they need to be
**unpacked** back into original edges.

### How unpacking works

During customization, when a triangle relaxation succeeds, we record which
two edges were used:

```
If shortcut u→v was relaxed via triangle u→x→v:
    unpacking_info[u→v] = (edge u→x, edge x→v)
```

To unpack a path, we recursively replace each shortcut with its two sub-edges
until only original edges remain.

**Unpacking our A→H query result:**

The query found the shortest path meets at E(7) with distance 12. The CCH path
through the chordal supergraph is:

```
Forward:  A ──up(2)──→ C ──up(2)──→ E       (fw_dist[E] = 4)
Backward: E ──down(8)──→ H                   (bw_dist[E] = 8)

CCH path: A → C → E → H    (3 edges, using chordal supergraph weights)
```

**Step 1**: Is A→C (up=2) a shortcut? No — A→C is an original edge with weight 2. Done.

**Step 2**: Is C→E (up=2) a shortcut? Yes — the original C→E has weight 6, but
customization relaxed it to 2 via triangle C←B→E. Unpack:

```
  C→E was relaxed via B:  C──B (down=1) + B──E (up=1)
  Replace C→E with: C → B → E

  Is C→B (down=1) original? Yes — C→B exists with weight 1. ✓
  Is B→E (up=1) original? Yes — B→E exists with weight 1. ✓
```

**Step 3**: Is E→H (down=8) a shortcut? Yes — H──E was created during contraction
of F. Customization set down=8 via triangle H←F→E. Unpack:

```
  E→H was relaxed via F:  E──F (down=5) + F──H (up=3)
  Replace E→H with: E → F → H

  Is E→F (down=5) original? Yes — E→F exists with weight 5. ✓
  Is F→H (up=3) original? Yes — F→H exists with weight 3. ✓
```

**Final unpacked path:**

```
A ──2──→ C ──1──→ B ──1──→ E ──5──→ F ──3──→ H

Total: 2 + 1 + 1 + 5 + 3 = 12 ✓
```

All edges are original directed edges from the input graph. The path
A→C→B→E→F→H is the true shortest path from A to H.

### The coordinated linear sweep

The actual unpacking in `mod.rs` uses a clever technique. To find the
intermediate node `x` of shortcut `u→v`:

```
Goal: find node x such that weight(u→v) = weight(u→x) + weight(x→v)

Method: iterate the INVERTED adjacency lists of u and v simultaneously
        (both sorted by node ID), looking for a common neighbor x
        whose edge weights sum to the shortcut weight.

This is O(degree(u) + degree(v)), not O(degree²).
```

---

## 9. Complete Data Structure Reference

### CCH (the main structure)

```
CCH
├── first_out: Vec<u32>          CSR offsets for the chordal supergraph
│                                 (nodes indexed by RANK, not original ID)
│
├── head: Vec<u32>               Target nodes (by rank) for each edge
│                                 Sorted ascending within each node's range
│
├── tail: Vec<u32>               Source node for each edge (reverse of CSR)
│                                 tail[edge_id] = which node this edge comes from
│
├── node_order: NodeOrder        The bidirectional rank ↔ node_id mapping
│   ├── node_order[rank] = id    "Who has this rank?"
│   └── ranks[id] = rank         "What rank does this node have?"
│
├── forward_cch_edge_to_orig_arc: Vecs<EdgeIdT>
│   └── For each CCH edge: which original FORWARD edges it represents
│       (used in sub-phase A of customization)
│
├── backward_cch_edge_to_orig_arc: Vecs<EdgeIdT>
│   └── Same but for BACKWARD (reverse-direction) original edges
│
├── elimination_tree: Vec<InRangeOption<u32>>
│   └── Parent pointer array. elimination_tree[v] = parent of v
│       None = root node. Always: rank(parent) > rank(child)
│
├── inverted: ReversedGraphWithEdgeIds
│   └── The chordal supergraph transposed (edges reversed)
│       Used during customization to find lower neighbors efficiently
│       Carries original edge IDs for weight lookup
│
└── separator_tree: SeparatorTree
    └── Nested dissection decomposition reconstructed from elimination tree
        Used for parallel customization
        Tree of separators, each containing a range of consecutive node IDs
```

### CustomizedBasic (after customization)

```
CustomizedBasic
├── cch: &CCH                    Reference to the CCH structure
│
├── upward: Vec<u32>             Weight for each CCH edge in the forward direction
│                                 (from lower rank to higher rank = "upward")
│
├── downward: Vec<u32>           Weight for each CCH edge in the backward direction
│                                 (from higher rank to lower rank = "downward")
│
├── up_unpacking: Vec<(Option<EdgeId>, Option<EdgeId>)>
│   └── For each upward edge: the two sub-edges it decomposes into
│       None = this is an original edge (no further unpacking)
│
└── down_unpacking: Vec<(Option<EdgeId>, Option<EdgeId>)>
    └── Same for downward direction
```

### DirectedCCH (for turn-expanded graphs)

```
DirectedCCH
├── forward_first_out, forward_head, forward_tail
│   └── Separate CSR for edges that carry finite FORWARD weight
│
├── backward_first_out, backward_head, backward_tail
│   └── Separate CSR for edges that carry finite BACKWARD weight
│
├── forward_inverted, backward_inverted
│   └── Separate reversed graphs for each direction
│
└── (everything else same as CCH)

Motivation: In turn-expanded (line) graphs, many edges are one-directional
(a turn is valid in only one direction). Storing separate forward/backward
graphs prunes 30-50% of dead edges, speeding up customization and queries.
```

### NodeOrder

```
NodeOrder
├── node_order: Arc<[u32]>    node_order[rank] = original_node_id
└── ranks: Arc<[u32]>         ranks[original_node_id] = rank

These are inverse permutations of each other:
  ranks[node_order[r]] = r   for all r
  node_order[ranks[n]] = n   for all n
```

### SeparatorTree

```
SeparatorTree
├── nodes: SeparatorNodes
│   ├── Consecutive(Range<u32>)   Separator is a contiguous range of ranks
│   └── Random(Vec<u32>)          Separator has non-contiguous ranks
│
├── children: Vec<SeparatorTree>  Recursive child components
│
└── num_nodes: usize              Total nodes in this subtree

The tree mirrors the nested dissection:
  - Leaf cells have no children
  - Internal separators have 2+ children
  - Processing is bottom-up: leaves in parallel, then parent separator
```

---

## 10. Visual Summary: The Full Transformation Pipeline

```
 ① THE REAL WORLD                        ② IN MEMORY (CSR)
 ┌──────────────────────┐                ┌───────────────────────────────────┐
 │                      │                │  first_out = [0, 2, 4, 6, 7, 8]  │
 │   Roads, junctions,  │  ──OSM PBF──→ │  head      = [1, 3, 2, 4, 0, ...]│
 │   speed limits       │  RoutingKit    │  weight    = [3000, 5000, ...]    │
 │                      │                │  lat, lng  = [21.03, ...], [...]  │
 │                      │                │                                   │
 └──────────────────────┘                │  n nodes, m directed edges       │
                                         └──────────────┬────────────────────┘
                                                        │
                                                        ▼
 ③ NODE ORDERING                         ④ CONTRACTION
 ┌──────────────────────┐                ┌───────────────────────────────────┐
 │                      │                │                                   │
 │  InertialFlowCutter  │                │  Process nodes in rank order:     │
 │  finds separators    │  ──cch_perm──→ │  Contract each, add shortcuts     │
 │  via min-cuts        │                │                                   │
 │                      │                │  Result: chordal supergraph       │
 │  Output: rank for    │                │    + elimination tree             │
 │  each node           │                │    + edge mappings                │
 │                      │                │    + separator tree               │
 └──────────────────────┘                │                                   │
                                         │  ★ TOPOLOGY ONLY — NO WEIGHTS    │
                                         └──────────────┬────────────────────┘
                                                        │
                                    ┌───────────────────┘
                                    │
                                    ▼
 ⑤ CUSTOMIZATION                         ⑥ QUERY
 ┌──────────────────────────┐            ┌──────────────────────────────────┐
 │                          │            │                                  │
 │  A. Copy original edge   │            │  Bidirectional walk up the       │
 │     weights to CCH edges │            │  elimination tree:               │
 │                          │            │                                  │
 │  B. Triangle relaxation: │            │  source ↗ ↗ ↗ meeting ↖ ↖ target│
 │     For each shortcut,   │  ────────→ │                  ↓               │
 │     find best path       │            │  Unpack shortcuts → real path    │
 │     through triangles    │            │  Map ranks → node IDs → coords  │
 │                          │            │                                  │
 │  ★ ~1 SECOND             │            │  ★ <1 MILLISECOND               │
 │  ★ RE-RUN ON WEIGHT      │            │                                  │
 │    CHANGE ONLY           │            │                                  │
 └──────────────────────────┘            └──────────────────────────────────┘
```

### The transformation at each stage

| Stage | Input | Output | What changes | What stays |
|-------|-------|--------|-------------|-----------|
| 0. Load | OSM PBF | CSR graph | — | — |
| 1. Order | Graph + coords | Node ranking | — | Graph topology |
| 2. Contract | Graph + ranking | Chordal supergraph + elim tree | Topology grows (shortcuts added) | No weights involved |
| 3. Customize | Chordal supergraph + weights | Weighted shortcuts | Weights assigned to all edges | Topology unchanged |
| 4. Query | Weighted CCH + (source, target) | Distance + path | Nothing changes | Everything reused |

### The fundamental invariant

At every point in the pipeline, this holds:

> **The shortest path between any two nodes in the original graph equals the
> shortest path between those same nodes in the weighted chordal supergraph,
> using only "upward" edges (toward higher rank) from each endpoint.**

This is what makes the elimination tree walk correct: by walking upward from
both source and target, you explore exactly the set of nodes that could be on a
shortest path — and the triangle relaxation in customization ensures their
weights are correct.
