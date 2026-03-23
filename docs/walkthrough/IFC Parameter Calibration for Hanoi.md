# IFC Parameter Calibration for Dense Urban Networks (Hanoi)

Recommendations for tuning InertialFlowCutter parameters in the three
`flow_cutter_cch_*.sh` scripts, based on the empirical analysis in
[IFC Ordering Effectiveness Analysis](../walkthrough/IFC%20Ordering%20Effectiveness%20Analysis.md)
and a source-level reading of the IFC accelerated flow cutter implementation.

**Date:** 2026-03-19

---

## Table of Contents

1. [Current Parameter Baseline](#1-current-parameter-baseline)
2. [How the Parameters Work (Source-Level)](#2-how-the-parameters-work-source-level)
3. [Diagnosis: Why Current Settings Under-Perform on Dense Meshes](#3-diagnosis-why-current-settings-under-perform-on-dense-meshes)
4. [Recommended Parameter Changes](#4-recommended-parameter-changes)
5. [Parameters to Leave Unchanged](#5-parameters-to-leave-unchanged)
6. [Experimental Variants Worth Testing](#6-experimental-variants-worth-testing)
7. [Expected Impact](#7-expected-impact)
8. [How to Evaluate](#8-how-to-evaluate)

---

## 1. Current Parameter Baseline

All three scripts (`flow_cutter_cch_order.sh`, `flow_cutter_cch_cut_order.sh`,
`flow_cutter_cch_cut_reorder.sh`) use identical IFC parameters:

| Parameter | Current Value | Description |
|-----------|--------------|-------------|
| `random_seed` | 5489 | Deterministic seed |
| `thread_count` | `$2` or `$(nproc)` | Parallelism |
| `BulkDistance` | no | Disable hop-distance-based terminal sets |
| `max_cut_size` | 100000000 | Effectively unlimited |
| `distance_ordering_cutter_count` | 0 | No random-source-target cutters |
| `geo_pos_ordering_cutter_count` | 8 | 8 angular projection directions |
| `bulk_assimilation_threshold` | 0.4 | Max fraction of nodes bulk-pierced per side |
| `bulk_assimilation_order_threshold` | 0.25 | Fraction of geo-order usable for bulk piercing |
| `bulk_step_fraction` | 0.05 | Chunk size per adaptive bulk step |
| `initial_assimilated_fraction` | 0.05 | Initial seed size (equidistant mode) |

Additionally, the following defaults from `flow_cutter_config.h` are **not
overridden** and therefore active:

| Parameter | Default Value | Description |
|-----------|--------------|-------------|
| `separator_selection` | `node_min_expansion` | Optimize cut/smaller-side ratio |
| `max_imbalance` | 0.2 | Minimum smaller-side fraction |
| `pierce_rating` | `max_target_minus_source_hop_dist` | Pierce node selection heuristic |
| `avoid_augmenting_path` | `avoid_and_pick_best` | Prefer non-augmenting pierce nodes |
| `skip_non_maximum_sides` | `skip` | Skip non-maximum cuts |
| `graph_search_algorithm` | `pseudo_depth_first_search` | Reachability search strategy |
| `cutter_count` | 3 | (Unused in accelerated path) |
| `branch_factor` | 5 | (Unused in accelerated path) |

---

## 2. How the Parameters Work (Source-Level)

### 2.1 Geo-Position Ordering (`geo_pos_ordering_cutter_count`)

In `flow_cutter_accelerated.h:1853`, `compute_inertial_flow_orders()` creates
`cutter_count` projection directions evenly spaced from 0 to pi:

```
phi_i = i * pi / cutter_count    (for i = 0, 1, ..., cutter_count-1)
```

For each direction, all nodes are sorted by their dot product with the unit
vector `(cos(phi), sin(phi))` applied to `(lat, lon)`. The sorted order is then
partially sorted: only the first and last `bulk_assimilation_order_threshold *
node_count` positions are fully sorted (via `nth_element` + `sort`), since
only those extremes are used for bulk piercing.

With `cutter_count = 8`, the projections are at 0, 22.5, 45, 67.5, 90, 112.5,
135, and 157.5 degrees. Each projection produces a separate flow cutter instance
that competes to find the best separator.

**Key insight:** On Hanoi's nearly-isotropic mesh, more projection angles help
because no single direction dominates. The graph has no strong axis-aligned
structure (unlike a grid city like Manhattan).

### 2.2 Bulk Piercing

The accelerated cutter uses **bulk piercing** instead of the classic one-node-
at-a-time pierce strategy. This works as follows:

1. **Initialization** (`equidistant_bulk_piercing`): Seeds each side with
   `initial_assimilated_fraction * node_count` nodes from the extremes of the
   geo-order. For 930K nodes at 0.05, this is ~46,500 nodes per side.

2. **Subsequent steps** (`adaptive_bulk_piercing`): Each step assimilates a
   chunk of size:
   ```
   nodes_to_assimilate = bulk_step_fraction * ((1 - bulk_step_fraction) * node_count/2 - current_side_size)
   ```
   This is a **diminishing fraction** of the remaining gap to the half-graph.

3. **Stopping conditions:**
   - `bulk_assimilation_threshold` (0.4): Stop bulk piercing when a side exceeds
     40% of graph size
   - `bulk_assimilation_order_threshold` (0.25): Stop when the geo-order pointer
     has consumed 25% of positions from either end

The bulk-pierced nodes are added to the assimilated set, and only those with
neighbors outside the set become "extra nodes" (frontier nodes for flow
computation).

### 2.3 Separator Selection (`node_min_expansion`)

The `ComputeSeparator` in `separator.h` uses the **expanded graph**
representation (each node split into in/out) to find **node separators** (not
edge cuts). The scoring function is:

```
score = cut_size / smaller_side_size
```

With a penalty of +1,000,000 if `smaller_side_size < max_imbalance *
node_count`. Lower score wins. This is the **node expansion ratio** — the
separator size relative to the smaller partition.

### 2.4 Pierce Rating (`max_target_minus_source_hop_dist`)

When selecting which frontier node to pierce next, the cutter scores each
candidate by `target_hop_distance - source_hop_distance`. This prefers nodes
that are "far from source, close to target" — i.e., nodes near the opposite
side's boundary, which tend to maximize the smaller side's growth and produce
balanced cuts faster.

### 2.5 BulkDistance

When `BulkDistance = yes`, the hop-distance computation uses a **multi-source
BFS** from the first/last `bulk_distance_factor * node_order.size()` nodes in
the geo-order, rather than a single-source BFS from the source/target node. This
gives distance estimates that reflect the entire terminal set rather than a
single point.

---

## 3. Diagnosis: Why Current Settings Under-Perform on Dense Meshes

### 3.1 Too Few Projection Directions

With 8 directions, the algorithm tries only 8 geo-based bisection planes. On
Hanoi's isotropic mesh, the quality variance between adjacent directions is
small, but with only 8 samples, the algorithm may miss the optimal bisection
angle entirely. Since the mesh lacks any dominant axis, having more directions
increases the probability of finding the best cut.

**Evidence:** The analysis shows that at 10% node removal, the balance ratio is
only 8-14%, meaning the algorithm struggles to find balanced bisections. More
angular samples could help find angles where the mesh has slightly more
structure (e.g., along rivers, highways, or district boundaries).

### 3.2 Bulk Piercing Saturates Too Early

`bulk_assimilation_order_threshold = 0.25` means only the extreme 25% of each
geo-order is available for bulk piercing. On a homogeneous mesh, the "middle
50%" of the geo-order contains nodes that are **not clearly** on either side of
any natural separator. By stopping at 25%, the algorithm forces the remaining
15% (between 25% and the ~40% threshold) to be pierced one-by-one via the cut
front, which is slow and doesn't benefit from the geo-ordering heuristic.

### 3.3 Initial Seed Is Too Small

`initial_assimilated_fraction = 0.05` seeds each side with ~46K nodes (5%).
On a dense mesh where 10% removal barely fragments the graph, starting with only
5% means the initial flow computation spans a vast unassigned middle region.
The initial max-flow is expensive (the graph is large and well-connected) but
produces a poor cut because the "source" and "target" regions are tiny relative
to the graph.

### 3.4 Bulk Step Fraction Is Conservative

`bulk_step_fraction = 0.05` means each adaptive step adds only 5% of the
remaining gap. On a 930K-node graph where each side needs ~370K nodes
(40% threshold), this means:
- Step 1: ~16K nodes
- Step 2: ~15K nodes
- Step 3: ~14K nodes
- ...

This creates many small flow augmentation rounds, each with marginal benefit on
a mesh where the flow value increases slowly. Larger steps would reduce the
number of expensive flow computations.

### 3.5 No Distance-Based Cutters

`distance_ordering_cutter_count = 0` disables all random-source-target cutters.
These cutters use **hop distance** to build the node order (instead of geo
position), which can capture graph-metric structure that geographic coordinates
miss. On a mesh with alleys and narrow passages, hop distance may reveal
bottlenecks that are invisible in the lat/lon projection.

---

## 4. Recommended Parameter Changes

### 4.1 Primary Recommendations (High Confidence)

These changes address the diagnosed issues and have strong theoretical backing:

| Parameter | Current | Recommended | Rationale |
|-----------|---------|-------------|-----------|
| `geo_pos_ordering_cutter_count` | 8 | **16** | Double the angular resolution. Cost: ~2x memory for cutter instances, but runtime is parallelized across threads. On a mesh with no dominant axis, sampling every 11.25 degrees instead of 22.5 degrees significantly improves coverage. |
| `bulk_assimilation_order_threshold` | 0.25 | **0.35** | Allow bulk piercing to consume 35% from each end of the geo-order. This bridges more of the gap to the 40% threshold, reducing the number of expensive single-pierce flow augmentations. |
| `initial_assimilated_fraction` | 0.05 | **0.10** | Double the initial seed to 10%. On Hanoi's mesh, the first 5% barely dents the graph; 10% provides a more meaningful initial partition and better initial flow computation. |
| `bulk_step_fraction` | 0.05 | **0.08** | Increase adaptive step size by 60%. Fewer, larger steps mean fewer flow recomputations. The mesh is so uniform that fine-grained stepping provides diminishing returns. |

### 4.2 Secondary Recommendations (Medium Confidence)

These are worth testing but may have trade-offs:

| Parameter | Current | Recommended | Rationale |
|-----------|---------|-------------|-----------|
| `distance_ordering_cutter_count` | 0 | **4** | Add 4 random-source-target cutters that use hop-distance ordering. These complement the geo-based cutters by capturing graph-metric bottlenecks (narrow alleys, bridges) invisible in geographic projection. Total cutters: 16 + 4 = 20. |
| `BulkDistance` | no | **yes** | Enable multi-source BFS for distance computation. When distance-ordering cutters are active, using the terminal set (rather than a single source) gives better distance estimates on a mesh where a single node's BFS covers the entire graph uniformly. |
| `max_imbalance` | 0.2 (default) | **0.25** | Relax the balance constraint from 20% to 25%. On Hanoi's mesh, insisting on 20% balance forces the algorithm to accept larger separators. Allowing 25% imbalance lets it find smaller separators at the cost of slightly unbalanced partitions, which CCH tolerates well. |

### 4.3 Proposed Script Parameters

For `flow_cutter_cch_order.sh` (and identically for the other two):

```bash
"${CONSOLE_BIN}" \
  load_routingkit_unweighted_graph "${GRAPH_DIR}/first_out" "${GRAPH_DIR}/head" \
  load_routingkit_longitude "${GRAPH_DIR}/longitude" \
  load_routingkit_latitude "${GRAPH_DIR}/latitude" \
  remove_multi_arcs \
  remove_loops \
  add_back_arcs \
  sort_arcs \
  flow_cutter_set random_seed $seed \
  reorder_nodes_at_random \
  reorder_nodes_in_preorder \
  flow_cutter_set thread_count ${2:-$(nproc)} \
  flow_cutter_set max_cut_size 100000000 \
  flow_cutter_set geo_pos_ordering_cutter_count 16 \
  flow_cutter_set distance_ordering_cutter_count 4 \
  flow_cutter_set BulkDistance yes \
  flow_cutter_set bulk_assimilation_threshold 0.4 \
  flow_cutter_set bulk_assimilation_order_threshold 0.35 \
  flow_cutter_set bulk_step_fraction 0.08 \
  flow_cutter_set initial_assimilated_fraction 0.10 \
  flow_cutter_config \
  report_time \
  reorder_nodes_in_accelerated_flow_cutter_cch_order \
  do_not_report_time \
  examine_chordal_supergraph \
  save_routingkit_node_permutation_since_last_load "${GRAPH_DIR}/perms/cch_perm"
```

---

## 5. Parameters to Leave Unchanged

| Parameter | Value | Why |
|-----------|-------|-----|
| `separator_selection` | `node_min_expansion` | This is the correct choice for CCH (node separators, not edge cuts). The expanded graph approach finds true vertex separators. |
| `pierce_rating` | `max_target_minus_source_hop_dist` | Best general-purpose heuristic. Alternatives like `circular_hop` or `random` perform worse in experiments on various graph types. |
| `avoid_augmenting_path` | `avoid_and_pick_best` | Avoiding augmenting paths reduces flow value growth, producing more balanced cuts. This is especially important on dense meshes where flow values grow quickly. |
| `skip_non_maximum_sides` | `skip` | Skipping non-maximum sides is a standard optimization that avoids wasting time on cuts that won't improve the current best. |
| `graph_search_algorithm` | `pseudo_depth_first_search` | PseudoDFS is the default for good reason: it explores deeply (good for finding augmenting paths) while being cache-friendly (stack instead of queue). BFS would explore level-by-level, which is slower on dense graphs. |
| `bulk_assimilation_threshold` | 0.4 | 40% is close to the theoretical maximum (50%) while leaving room for the separator. Increasing beyond 0.4 risks the two sides overlapping before a good separator is found. |
| `max_cut_size` | 100000000 | Already effectively unlimited. No reason to reduce it — the mesh topology ensures that cuts won't grow beyond a few thousand even at deep recursion levels. |
| `random_seed` | 5489 | Any fixed seed works. The seed is only for reproducibility. |

---

## 6. Experimental Variants Worth Testing

Beyond the primary recommendations, these are more speculative configurations
that could yield further improvements but require empirical validation:

### 6.1 Aggressive Variant (fewer, larger steps)

Optimized for speed at the potential cost of slight quality loss:

```
geo_pos_ordering_cutter_count  = 12
distance_ordering_cutter_count = 0
bulk_assimilation_order_threshold = 0.40
bulk_step_fraction             = 0.12
initial_assimilated_fraction   = 0.15
```

**Rationale:** Maximizes bulk piercing speed. The large initial seed and step
size means fewer flow computations. Good if runtime is a priority and the mesh
is truly uniform (no hidden structure to find with distance-based cutters).

### 6.2 Quality Variant (more cutters, finer steps)

Optimized for separator quality at the cost of longer runtime:

```
geo_pos_ordering_cutter_count  = 24
distance_ordering_cutter_count = 8
BulkDistance                    = yes
bulk_assimilation_order_threshold = 0.30
bulk_step_fraction             = 0.05
initial_assimilated_fraction   = 0.08
max_imbalance                  = 0.25
```

**Rationale:** 32 total cutters sample the space densely. The 8 distance-based
cutters provide diverse graph-metric perspectives. Smaller steps allow finer
separator selection. Higher imbalance tolerance allows finding smaller
separators. Runtime will be ~3-5x longer (mostly parallelizable), but the
ordering only needs to be computed once.

### 6.3 BFS Search Variant

```
graph_search_algorithm = breadth_first_search
```

On dense meshes, BFS explores the graph layer-by-layer from the frontier, which
may produce more uniform reachability sets. PseudoDFS can get "stuck" exploring
one direction deeply before visiting nearby nodes. This is speculative — the
IFC authors chose PseudoDFS as the default after extensive testing on highway
networks, but Hanoi's topology is different enough that BFS might perform better.

---

## 7. Expected Impact

### 7.1 What Will Improve

- **Separator size at each recursion level**: More cutters and better bulk
  piercing should find smaller separators on average, reducing CCH fill-in.
- **Fill-in (chordal supergraph size)**: Smaller separators at each level
  compound — even a 5-10% reduction in average separator size can yield
  20-30% less fill-in due to the recursive structure.
- **CCH query search space**: Smaller fill-in means fewer nodes visited per
  query.
- **CCH customization time**: Fewer edges in the chordal supergraph means
  less work during weight customization.

### 7.2 What Will Not Change

- **The fundamental topological limitation**: Hanoi's mesh has Theta(sqrt(N))
  minimum balanced separators. No parameter tuning can overcome this — the
  ordering will still be "weaker" than on highway-dominated networks.
- **The 10% fragmentation threshold**: The mesh requires ~90K node removals
  for meaningful fragmentation. This is intrinsic to the city's morphology.
- **The arc ordering quality**: The analysis showed `cch_perm_cuts` and
  `cch_perm_cuts_reorder` produce nearly identical locality metrics. This
  is unlikely to change with node-ordering parameter adjustments.

### 7.3 Estimated Improvement Range

Based on the analysis data and parameter interaction model:

| Metric | Current (estimated) | After tuning (estimated) | Improvement |
|--------|-------------------|------------------------|-------------|
| IFC runtime | ~3 sec | ~5-8 sec | Slower (more cutters) |
| Average separator size | Baseline | 5-15% smaller | Moderate |
| CCH fill-in | Baseline | 10-25% less | Significant |
| CCH query time | Baseline | 5-15% faster | Moderate |
| CCH customization | Baseline | 10-20% faster | Moderate |

The IFC runtime increase is acceptable because it's a one-time preprocessing
cost (~3 seconds currently), and the pipeline's total runtime is dominated by
other stages (graph generation, line graph construction, etc.).

---

## 8. How to Evaluate

### 8.1 Direct Comparison

Run `flow_cutter_cch_order.sh` with both old and new parameters on the same
graph, comparing the `examine_chordal_supergraph` output:

- **Tree width** (lower is better)
- **Search space sizes** (lower is better)
- **Number of edges in chordal supergraph** (lower is better)

### 8.2 Separator Quality

Reproduce the separator hierarchy analysis from the effectiveness analysis
document using the new `cch_perm`:

- Remove top 0.1%, 0.5%, 1%, 2%, 5%, 10% of nodes
- Measure largest component percentage and balance ratio
- Better ordering = faster fragmentation and higher balance ratios

### 8.3 End-to-End CCH Performance

After generating the new ordering:

1. Run CCH customization and measure time
2. Run sample queries and measure average/median/P95 query time
3. Compare against the current ordering's performance

### 8.4 A/B Testing Multiple Configurations

Since IFC runs in ~3-8 seconds, it's practical to test all variants from Section
6 and compare their `examine_chordal_supergraph` output. This takes under a
minute total and gives definitive answers about which configuration is best for
this specific graph.
