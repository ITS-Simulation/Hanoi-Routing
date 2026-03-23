# Multi-Via-Way Turn Restriction Support

## Problem Statement

OSM turn restrictions can have **multiple via-way members**, representing restrictions that span several intermediate road segments (e.g., a U-turn ban spanning two consecutive short links). Currently:

- **Unconditional pipeline** (`osm_profile.cpp`): Rejects any restriction with more than one via member (lines 949-952, 1101-1104), and silently drops all via-way restrictions entirely (lines 973-976, 1125-1128)
- **Conditional pipeline** (`conditional_restriction_resolver.cpp`): The decoder collects multiple via-ways into a vector, but the resolver explicitly drops multi-via-way restrictions (lines 369-374)

From the Hanoi PBF build log, **13 restrictions were dropped** due to "several via roles" — these are all multi-via-way restrictions that currently produce no forbidden turns.

## Current Architecture

### Unconditional Turn Restrictions (osm_profile.cpp)

```
OSM PBF → decode_osm_{car,motorcycle}_turn_restrictions()
         → Only accepts via-node (single OSM node)
         → Drops via-way (single or multi)
         → Output: (from_way, to_way, via_node, direction, category)
```

The unconditional pipeline has **no concept of via-ways at all**. It expects exactly one via member, and that member must be a node.

### Conditional Turn Restrictions (conditional_restriction_decoder.cpp → conditional_restriction_resolver.cpp)

```
OSM PBF → decode_conditional_restriction()
         → Collects via_ways into std::vector<uint64_t>  ← already supports multi
         → conditional_restriction_resolver:
              → via-node: resolve to (from_arc, to_arc) pair
              → single via-way: decompose into 2 turn pairs at junction A and B
              → multi via-way: DROPPED (line 369-374)
```

The decoder already stores multiple via-ways. Only the resolver needs extension.

### The `way` File

The `way` binary vector (arc → routing_way_id) is:
- **Produced by**: `osm_graph_builder.cpp` (line 169), `generate_graph.cpp` (line 222)
- **Consumed by**: `conditional_restriction_resolver.cpp` only (line 74)
- **NOT used by**: CCH ordering (IFC), CCH customization, server queries

**Verdict**: The `way` file is needed for turn restriction resolution and should be kept. However, it can be excluded from the line graph output since the line graph has no turn restrictions (they're already baked into the topology).

## Implementation Plan

### Phase 1: Extend the Conditional Resolver for Multi-Via-Way

**File**: `RoutingKit/src/conditional_restriction_resolver.cpp`

**Current single via-way logic** (lines 367-456):
1. Find junction A (from_way ∩ via_way)
2. Find junction B (via_way ∩ to_way)
3. Resolve arc pairs at both junctions
4. Emit 2 forbidden turn pairs

**Extended multi-via-way logic**:
1. Build an ordered chain of junctions: from_way → via_way[0] → via_way[1] → ... → via_way[N-1] → to_way
2. For each consecutive pair (way_i, way_{i+1}), find the shared junction node
3. Validate the chain: all junctions must be distinct, all ways must share exactly one junction with their neighbor
4. At each junction, resolve the (incoming_arc, outgoing_arc) pair
5. Emit N+1 forbidden turn pairs (one per junction in the chain)

**Pseudocode**:
```cpp
// Build way chain: [from_way, via_way_0, via_way_1, ..., via_way_N, to_way]
std::vector<unsigned> way_chain;
way_chain.push_back(local_from_way);
for(auto& vw : r.via_ways)
    way_chain.push_back(routing_way.to_local(vw, invalid_id));
way_chain.push_back(local_to_way);

// Find junction between each consecutive pair
std::vector<unsigned> junctions;  // size = way_chain.size() - 1
for(size_t i = 0; i + 1 < way_chain.size(); ++i) {
    unsigned junction = find_junction_node(g, way_chain[i], way_chain[i+1]);
    if(junction == invalid_id) { drop; break; }
    junctions.push_back(junction);
}

// Validate: no duplicate junctions (would indicate degenerate chain)
// Resolve arc pairs and emit forbidden turns at each junction
for(size_t i = 0; i < junctions.size(); ++i) {
    auto incoming = find_incoming_arcs_of_way_at_node(g, way_chain[i], junctions[i]);
    auto outgoing = find_outgoing_arcs_of_way_at_node(g, way_chain[i+1], junctions[i]);
    auto pair = resolve_unique_arc_pair(incoming, outgoing);
    if(!pair.valid && i == 0)
        pair = disambiguate_arc_pair(g, incoming, outgoing, junctions[i], r.direction);
    if(!pair.valid) { drop; break; }
    emit_forbidden_turn(pair);
}
```

**Key design decisions**:
- **Direction disambiguation**: Only applicable at junction 0 (entry from from_way). At interior and exit junctions, the overall restriction direction is meaningless — rely on unique-candidate resolution only
- **Decomposition strategy**: Same as single via-way (Strategy B — conservative penalty). Each junction gets its own forbidden turn. This over-penalizes slightly (blocks individual junction turns even when only the full chain should be forbidden), but is safe and consistent with the existing approach
- **Error handling**: If any junction in the chain cannot be found or resolved, drop the entire restriction with a log message

### Phase 2: Extend the Unconditional Pipeline (osm_profile.cpp)

**Approach: Route via-way restrictions through the conditional pipeline** with an "always active" condition. This avoids duplicating via-way resolution logic in `osm_profile.cpp`. The conditional resolver already has all the machinery — unconditional via-way restrictions are simply conditional restrictions with no time condition, which the existing pipeline handles naturally.

### Phase 3: Validate

- Re-run the pipeline on `hanoi.osm.pbf`
- Verify the 13 previously-dropped restrictions now produce forbidden turns
- Run the graph validator to confirm no new failures
- Check that the total forbidden turn count increases appropriately

## Regarding the `way` File

### Keep or remove?

**Keep it.** The `way` file is the mechanism that makes via-way resolution possible — it maps arc IDs back to OSM way IDs. Without it, the resolver cannot determine which arcs belong to which way.

However:
- The `way` file is **not needed** in the CCH algorithm pipeline (ordering, customization, queries)
- The `way` file is **not needed** in the line graph output (turn restrictions are already baked into topology)
- It only needs to exist in the original graph directory for the `conditional_turn_extract` tool to use

**No action needed** — current behavior is correct. The `way` file is produced during graph generation and consumed during turn restriction extraction. It's never loaded by the server or CCH algorithms.

## Output Format Compatibility

Multi-via-way restrictions are fully compatible with the existing RoutingKit binary format. The resolver decomposes each multi-via-way restriction into multiple **(from_arc, to_arc) pairs** — the same representation used by via-node and single via-way restrictions. These pairs are appended to the same flat sorted `u32` vectors (`forbidden_turn_from_arc`, `forbidden_turn_to_arc`, or the conditional equivalents).

A restriction with N via-ways produces N+1 junction pairs instead of 1. Downstream consumers (CCH ordering, customization, server, line graph generator) cannot distinguish the origin of a forbidden turn — it's all just arc pairs. **No format changes are needed.**

## Files to Modify

| File | Change |
|------|--------|
| `RoutingKit/src/conditional_restriction_resolver.cpp` | Replace multi-via-way drop (lines 369-374) with chain resolution logic |
| `RoutingKit/src/osm_profile.cpp` | (Phase 2) Route unconditional via-way restrictions to the conditional pipeline with "always active" condition |

## Risk Assessment

- **Low risk**: The conditional resolver already handles single via-way correctly. Multi-via-way is a natural extension of the same pattern
- **Data dependency**: Requires the `way` file, which is already produced by both RoutingKit and CCH-Generator
- **Over-penalization**: The decomposition strategy (one forbidden turn per junction) is conservative but may block valid paths that only partially overlap with the restricted chain. This is the same trade-off already accepted for single via-way restrictions
