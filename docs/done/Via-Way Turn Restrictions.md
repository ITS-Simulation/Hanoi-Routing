# Via-Way Turn Restrictions in Line Graph

## Context

The line graph currently enforces only **direct** (via-node) turn restrictions
from `forbidden_turn_from_arc` / `forbidden_turn_to_arc`. These are simple
`(from_arc, to_arc)` pairs at a single intersection.

**194 unconditional via-way restrictions** (and 19 conditional via-way
restrictions) are silently dropped. RoutingKit's unconditional decoder
(`osm_profile.cpp:973-976`) explicitly returns early for via-way members. The
conditional decoder captures them and the conditional resolver correctly resolves
them to arc pairs — but it **decomposes** multi-junction chains into independent
pairs, which is **semantically incorrect** for the line graph (explained below).
Furthermore, `generate_line_graph` never reads the conditional turn files.

| Chain length | Count | Typical restriction types   |
| ------------ | ----- | --------------------------- |
| 1 via way    | 177   | no_u_turn, no_left_turn     |
| 2 via ways   | 12    | no_u_turn, no_left_turn     |
| 3 via ways   | 3     | no_u_turn                   |
| 4 via ways   | 2     | no_u_turn                   |
| **Total**    | **194** |                           |

### Why independent pair decomposition is wrong for the line graph

The conditional resolver decomposes `from_way -> [via_ways] -> to_way` into one
`(from_arc, to_arc)` pair per junction. In penalty-based CCH customization, each
pair gets a large weight — this is conservative but safe (routes still exist,
just penalized). In the line graph, forbidden = **structurally absent**
(`None`). Decomposing into independent forbidden pairs **over-restricts**:

```
Restriction: from_arc -> via_arc -> to_arc  (ONLY this chain is forbidden)

Independent decomposition forbids:
  (from_arc, via_arc)  — blocks ALL entries from from_arc to via_arc
  (via_arc, to_arc)    — blocks ALL exits from via_arc to to_arc

Over-restriction examples:
  from_arc -> via_arc -> other_exit  WRONGLY BLOCKED
  other_entry -> via_arc -> to_arc   WRONGLY BLOCKED
```

---

## Approach: Node Splitting (Automaton Construction)

To forbid a specific **path** of 2+ edges through the line graph without
removing individual edges, we use **node splitting** — creating "tainted" copies
of intermediate nodes that track whether the vehicle has entered the forbidden
chain.

### Core algorithm

For a **prohibitive** restriction with forbidden LG path
`A -> V1 -> V2 -> ... -> VN -> T`:

1. For each intermediate node `Vi` (i = 1..N), create tainted copy `Vi'`
2. Redirect entry: the edge `A -> V1` becomes `A -> V1'`
3. For intermediate copies (i < N): `Vi'` gets all of `Vi`'s outgoing edges, but
   `Vi' -> V(i+1)` redirects to `V(i+1)'`
4. For the last copy `VN'`: gets all of `VN`'s outgoing edges **except**
   `VN -> T`
5. Weights for all copied edges are identical to originals

For a **mandatory** restriction (only_*):

Same structure, but tainted copies have only the chain edge (inverse of
prohibitive):

- Intermediate `Vi'`: outgoing = `{Vi' -> V(i+1)'}` only
- Last `VN'`: outgoing = `{VN' -> T}` only

### Edge cases (verified)

| Case | Handling |
|------|----------|
| Multiple restrictions sharing same intermediate node | Each creates its own tainted copy |
| Same from_arc, same intermediate, different to_arcs | Merge into single tainted copy that lacks all forbidden exits |
| Multi-segment via_way (entry != exit arc) | Walk the via_way's arc chain between junctions; taint each intermediate arc |
| Chain already broken by direct forbidden turn | Skip restriction (already enforced) |
| Tainted copy has 0 outgoing edges | Valid dead-end; CCH assigns INFINITY naturally |
| Weight encoding (shifted: `tt(dest) + turn_cost`) | Tainted copies inherit identical weights — same physical arcs |
| Source-edge correction in query | Split-map resolves tainted node → original arc for `tt(source)` |
| Coordinate snapping | Unaffected — spatial index only covers original nodes |
| CCH permutation | Node splitting happens before IFC ordering, so split nodes included naturally |

---

## Where Node Splitting Sits in the CCH Pipeline

Node splitting is a **graph construction** operation — it modifies the line
graph's topology (CSR arrays) **before** any CCH phase begins. It is **not** a
CCH algorithm modification.

```
┌─────────────────────────────────────────────────────────┐
│  GRAPH CONSTRUCTION (pre-CCH, offline, run once)        │
│                                                         │
│  1. line_graph() — build base LG from original graph    │
│     • Direct forbidden turns baked in via None callback  │
│  2. apply_node_splits() — expand LG with tainted copies │
│     • Via-way restrictions baked into topology           │
│  3. Write expanded CSR to disk (first_out, head, tt)    │
│                                                         │
│  Output: a plain directed graph. CCH sees no difference │
│  between original LG nodes and split nodes.             │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│  CCH PHASE 1: CONTRACTION (metric-independent)          │
│                                                         │
│  • InertialFlowCutter reads first_out, head, lat, lng   │
│    FROM DISK — it sees the already-expanded graph.      │
│    Split nodes are indistinguishable from originals.    │
│    IFC computes cch_perm covering all N+K nodes.        │
│  • CCH::fix_order_and_build() contracts the graph       │
│  • to_directed_cch() prunes always-INFINITY edges       │
│                                                         │
│  No awareness of split nodes. They are just graph nodes │
│  with their own degree, contracted like any other.      │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│  CCH PHASE 2: CUSTOMIZATION (metric-dependent)          │
│                                                         │
│  • Assigns upward/downward weights to CCH edges         │
│  • Split nodes' edges carry correct weights (copied     │
│    from originals during node splitting)                 │
│  • Re-runs on /customize weight uploads                 │
│                                                         │
│  No awareness of split nodes.                           │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│  CCH PHASE 3: QUERY                                     │
│                                                         │
│  • Bidirectional Dijkstra on the customized CCH         │
│  • May route through split nodes transparently          │
│  • Path unpacking maps LG nodes → original arcs         │
│    (split_map extends this mapping for split nodes)     │
│                                                         │
│  No awareness of split nodes during search.             │
│  Path reconstruction uses split_map to resolve them.    │
└─────────────────────────────────────────────────────────┘
```

**Key point**: Node splitting produces a **structurally different graph** that
encodes the via-way restrictions in its topology. From CCH's perspective, it is
just a slightly larger graph (~200 extra nodes out of 1.87M). All three CCH
phases operate on it without modification. The split nodes participate in
contraction, customization, and query exactly like regular nodes.

This is the same principle used by direct forbidden turns: they are baked into
the line graph's CSR structure (missing edges) rather than handled during CCH
query. Node splitting extends this to multi-edge forbidden paths.

---

## Implementation Plan

### Phase 1: Extend `conditional_turn_extract` to output arc chains

**File**: `RoutingKit/src/conditional_turn_extract.cpp`

The conditional resolver already resolves via-way restrictions into junction
`(from_arc, to_arc)` pairs with full chain context
(`conditional_restriction_resolver.cpp:391-508`). Currently it only outputs the
decomposed pairs. We add a **second output mode** that writes the full arc
chains.

**Modification**: After `resolve_conditional_restrictions()` returns, also call a
new function `resolve_via_way_chains()` that:

1. Re-processes the `raw` restrictions vector (only via-way entries)
2. For each via-way restriction, resolves the full arc chain:
   `[from_arc, via_entry_1, ..., via_exit_1, via_entry_2, ..., to_arc]`
3. Writes output files **alongside the existing forbidden turn files** in the
   graph directory (not a subdirectory):
   - `via_way_chain_offsets` (u32): CSR offsets into arc array
   - `via_way_chain_arcs` (u32): packed `[from_arc, v1, v2, ..., vN, to_arc]`
   - `via_way_chain_mandatory` (u8): 0 = prohibitive, 1 = mandatory
   - If there are zero unconditional via-way restrictions, write **empty** files
     (0-length vectors + single-element offset `[0]`) so that
     `generate_line_graph` can unconditionally require them

**Why modify RoutingKit?** The OSM way → graph arc resolution requires the PBF
file and `load_osm_id_mapping_from_pbf()` which already exists in RoutingKit.
The `way` file on disk uses local routing_way_ids (0-indexed), not OSM way IDs.
Reimplementing this mapping in Rust would require either:
- Re-parsing the PBF file with a Rust PBF library
- Saving the OSM-to-local mapping as a new file

Both are significantly more work than extending the existing C++ tool that
already has all the infrastructure.

**New function** `resolve_via_way_chains()` (add to
`conditional_restriction_resolver.cpp` or as a standalone helper):

```cpp
struct ViaWayChain {
    std::vector<unsigned> arcs;  // [from_arc, v1, ..., vN, to_arc]
    bool mandatory;
};

std::vector<ViaWayChain> resolve_via_way_chains(
    const std::string& graph_dir,
    const std::string& pbf_file,
    const std::vector<RawConditionalRestriction>& raw,
    std::function<bool(uint64_t, const TagMap&)> is_way_used,
    std::function<void(const std::string&)> log_message
);
```

This reuses `load_graph()`, `load_osm_id_mapping_from_pbf()`,
`find_junction_node()`, `find_incoming_arcs_of_way_at_node()`, and
`find_outgoing_arcs_of_way_at_node()` — all already exist in the resolver. The
key difference is that instead of decomposing into pairs, it **walks the arc
chain** between junctions and outputs the full sequence.

For multi-segment via_ways (where entry_arc != exit_arc), the function walks the
graph from entry_arc following arcs with the same `way` ID until reaching the
exit junction node.

### Phase 2: Load via-way chains in `generate_line_graph`

**File**: `CCH-Hanoi/crates/hanoi-tools/src/bin/generate_line_graph.rs`

Loading of via-way chain files is **mandatory**. The tool **errors out** if the
files are missing — this is the mechanism that guarantees every line graph query
produces a viable, law-abiding route. Without these files, the line graph would
silently ignore via-way restrictions, producing routes that violate real-world
turn rules. Making the files mandatory (even when empty) eliminates the
possibility of stale or incomplete line graphs entering production.

Empty files (zero restrictions) are valid and result in no node splitting.

```rust
// After: let exp_graph = line_graph(&graph, |edge1_idx, edge2_idx| { ... });
// Load via-way chains — REQUIRED (errors if files missing)
let chains = load_via_way_chains(&graph_path)?;
info!("loaded {} via-way restriction chains", chains.len());
// Apply node splitting (no-op if chains is empty)
let (expanded_graph, split_map) = apply_node_splits(exp_graph, &chains);
```

The validation step (already present for `forbidden_turn_*`) is extended to
also check for `via_way_chain_offsets`, `via_way_chain_arcs`, and
`via_way_chain_mandatory`. Missing files produce a hard error with a message
directing the user to run `conditional_turn_extract` first.

### Phase 3: Node splitting post-processor

**File**: `CCH-Hanoi/crates/hanoi-core/src/via_way_restriction.rs` (new)

Core types and the node-splitting algorithm:

```rust
pub struct ViaWayChain {
    pub arcs: Vec<u32>,      // [from_arc, v1, ..., vN, to_arc]
    pub mandatory: bool,
}

pub struct SplitResult {
    pub first_out: Vec<u32>,
    pub head: Vec<u32>,
    pub weight: Vec<u32>,
    pub split_map: Vec<u32>,  // split_node_i → original_node_id
}
```

Algorithm (operates on the LG CSR):

1. Convert CSR to adjacency list for easy mutation
2. For each restriction chain `[A, V1, ..., VN, T]`:
   a. Verify chain connectivity in LG (skip if any edge missing)
   b. Pre-check for merging: if another restriction already tainted `Vi` for the
      same incoming arc, reuse that tainted copy
   c. Create tainted copies, redirect edges, remove/restrict outgoing edges
3. Convert back to CSR with split nodes appended
4. Return `split_map` for path reconstruction

### Phase 4: Update path reconstruction

**File**: `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`

Load the `via_way_split_map` file from the line graph directory and extend
`original_tail`, `original_head`, and `original_travel_time` arrays. This file
is **mandatory** — if `generate_line_graph` performed node splitting, it must
be present. If no splitting occurred (zero via-way restrictions), the file is
empty (0-length vector) and no extension happens.

```rust
let split_map = Vec::<u32>::load_from(lg_dir.join("via_way_split_map"))?;
for &original in &split_map {
    original_tail.push(original_tail[original as usize]);
    original_head.push(original_head[original as usize]);
    original_travel_time.push(original_travel_time[original as usize]);
}
```

Also extend latitude/longitude arrays (done during generation in Phase 2).

**Consistency check**: after loading, verify that `num_lg_nodes ==
original_travel_time.len()` (base LG nodes + split nodes). Error if mismatch.

### Phase 5: Pipeline integration

**File**: `scripts/pipeline` (and `CCH-Generator/scripts/run_pipeline`)

Updated pipeline:

```
cch_generator <pbf> <output_dir> --profile car
  → graph/forbidden_turn_from_arc, forbidden_turn_to_arc  (direct turns)

  → conditional_turn_extract <pbf> <graph_dir> --profile car
    → graph/via_way_chain_offsets, via_way_chain_arcs, via_way_chain_mandatory
      (full arc chains for unconditional via-way restrictions)
    → conditional_turns/  (time-dependent turn data, as before)

  → generate_line_graph <graph_dir>
    REQUIRES: forbidden_turn_* AND via_way_chain_* (hard error if missing)
    → line_graph/ CSR (with node splitting applied)
    → line_graph/via_way_split_map  (split node → original node mapping)

  → flow_cutter_cch_order.sh <graph_dir>
    (operates on the expanded line graph including split nodes)
```

No pipeline reordering needed — `conditional_turn_extract` already runs before
`generate_line_graph`. The new via-way chain files sit alongside
`forbidden_turn_*` in `graph/` and are a **mandatory** prerequisite.

---

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `RoutingKit/src/conditional_restriction_resolver.cpp` | Modify | Add `resolve_via_way_chains()` function |
| `RoutingKit/include/routingkit/conditional_restriction_resolver.h` | Modify | Declare `ViaWayChain` struct + function |
| `RoutingKit/src/conditional_turn_extract.cpp` | Modify | Call `resolve_via_way_chains()`, write chain files |
| `CCH-Hanoi/crates/hanoi-core/src/via_way_restriction.rs` | **Create** | Types, I/O, node-splitting algorithm |
| `CCH-Hanoi/crates/hanoi-core/src/lib.rs` | Modify | Add `pub mod via_way_restriction` |
| `CCH-Hanoi/crates/hanoi-tools/src/bin/generate_line_graph.rs` | Modify | Load chains, invoke node splitting, write split_map |
| `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs` | Modify | Load split_map, extend reconstruction arrays |
| `scripts/pipeline` | Verify | Ensure conditional_turn_extract runs before generate_line_graph (already true) |

### Not touched

- `rust_road_router/` — no modifications (engine `line_graph()` function
  unchanged; node splitting is a post-processing step)
- Graph CSR format — unchanged (split nodes just add more nodes/edges)
- Coordinate snapping — unchanged (spatial index covers original nodes only)

---

## Verification

1. **Synthetic test**: Create small graph with known via-way restriction. Verify
   node splitting produces correct topology (all edge cases from analysis above).

2. **Full pipeline**: Run on `hanoi_car` and `hanoi_motorcycle`:
   ```bash
   # Re-run conditional_turn_extract (now outputs via_way_restrictions/ too)
   RoutingKit/bin/conditional_turn_extract <hanoi.osm.pbf> Maps/data/hanoi_car/graph --profile car
   # Re-run line graph generation (now applies node splitting)
   CCH-Hanoi/target/release/generate_line_graph Maps/data/hanoi_car/graph
   # Re-run IFC ordering
   scripts/flow_cutter_cch_order.sh Maps/data/hanoi_car/line_graph
   ```

3. **Graph size**: Line graph should grow by ~200-300 nodes (one split node per
   intermediate arc per restriction). Negligible vs 1.87M base LG nodes.

4. **Query comparison**: Run same queries through normal and line graph engines.
   The line graph should now enforce additional restrictions, potentially
   producing different routes at affected intersections.

5. **Spot-check**: Pick a known via-way restriction, find its junction arcs,
   verify the LG lacks the forbidden path but preserves all other paths through
   the same intermediate arcs. Confirm weights are unchanged.
