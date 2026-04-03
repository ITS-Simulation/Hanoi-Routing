# Separator-Based Alternative Paths in CCH — Implementation Reference

**Source:** Bacherle, Bläsius, Zündorf (KIT) — ATMOS 2025
**Code:** https://github.com/mzuenni/Separator-Based-Alternative-Paths-in-CCHs

---

## 1. Problem Statement

Given a CCH-preprocessed road network, compute **k alternative paths** (typically k=1..3) between source `s` and destination `t` that satisfy three admissibility criteria:

1. **Limited Sharing (γ = 0.8):** Each alternative must share at most 80% of its cost with the shortest path *and* with the union of all other selected alternatives. Formally: `c(P ∩ (Ps,t ∪ other_alts)) ≤ γ · d(s,t)`.
2. **Bounded Stretch (ε = 0.25):** Every subpath deviating from the shortest path must not exceed 125% of the shortest distance between its endpoints. Formally: for every a-b-subpath P' of P, `c(P') ≤ (1+ε) · d(a,b)`.
3. **Local Optimality (α = 0.25):** Any subpath of length ≤ α·d(s,t) must itself be a shortest path. Checked via the **T-test** (see below).

These parameters (γ=0.8, ε=0.25, α=0.25) are the standard values used throughout the literature.

---

## 2. Why Existing Approaches Don't Work for CCH

The standard **plateau method** (used by CRP and bidirectional Dijkstra) discovers alternative via-vertex candidates by running the bidirectional search *longer than necessary* and finding shared subpaths in the forward/backward search trees.

**This doesn't apply to CCH** because the CCH query walks up the elimination tree from both `s` and `t` to the root — there is no stopping criterion to "relax." Once it reaches the root, there is nothing more to explore. The search space is structurally fixed by the elimination tree.

---

## 3. Core Insight: Separators as Via-Vertex Candidates

### The Key Observation

In a CCH, the elimination tree `T` encodes a **hierarchy of balanced separators**. For any two vertices `s` and `t`:

- Let `A` = set of **common ancestors** of `s` and `t` in `T`.
- `A` **separates** `s` from `t` — every s-t path must pass through some vertex in `A`.
- The standard CCH query **already computes** `d(s,v)` and `d(v,t)` for every `v ∈ A`.

Therefore, `A` is a natural set of **via-vertex candidates**, obtained with **zero additional cost** beyond the normal CCH query.

### Why This Works Well

- **Small separators** → few candidates → fast checking.
- **Balanced separators** → separator vertices lie roughly "in the middle" between s and t → intuitively good via-vertex candidates that produce genuinely different paths.

---

## 4. The Three Algorithm Variants

### 4.1 Basic Approach (SeArCCH)

**Input:** CCH with elimination tree `T`, source `s`, destination `t`.
**Candidate set:** `A` = common ancestors of `s` and `t` in `T` (the top-level separator).

**Algorithm:**

1. Run standard CCH query → get `d(s,v)` and `d(v,t)` for all `v ∈ A`, plus `d(s,t)`.
2. Sort candidates by **via-path length** `d(s,v) + d(v,t)` in increasing order.
3. Greedily process each candidate `v ∈ A`, applying four checks in order of increasing cost:
   - **Check 1 — Total Stretch Pruning:** If `d(s,v) + d(v,t) > (1+ε) · d(s,t)`, reject `v` AND stop processing (all remaining are longer).
   - **Check 2 — Bounded Stretch:** Find the deviation subpath a→v→b and check `d(a,v) + d(v,b) ≤ (1+ε) · (d(s,t) − d(s,a) − d(b,t))`.
   - **Check 3 — Limited Sharing:** Check sharing with shortest path and all previously selected alternatives.
   - **Check 4 — T-test (Local Optimality):** Run a separate CCH query for `d(a',b')` and verify it equals the path cost through `v`.
4. If all checks pass, add `v` to the selected alternatives and mark its edges.

**Performance:** 65% success rate for 1st alternative, 0.5ms runtime.

### 4.2 Two-Step Approach

**Motivation:** The basic approach fails when the top-level separator is too small (e.g., London↔Paris through the Eurotunnel — only a few separator vertices, all forced through the tunnel) or when the separator is too close to `s` or `t`.

**Key Idea:** If the separator vertex `v` used by the shortest path is unavoidable, **keep it fixed** and recursively look for alternatives in the two subproblems: `s → v_s` and `v_t → t`.

**Algorithm:**

1. Run the basic approach first.
2. If insufficient alternatives found, identify the vertex `v` on `P_{s,t}` that is **highest in the elimination tree** (closest to root).
3. Let `v_s` and `v_t` be the two neighbors of `v` on the shortest path.
4. Recursively call the basic algorithm for:
   - Alternative paths from `s` to `v_s`
   - Alternative paths from `v_t` to `t`
5. Combine all pairs of sub-alternatives into full s-t paths.
6. Check combined paths for admissibility (sharing, T-test at junction vertex `v`).

**Parameter Adjustments for Recursive Calls:**

- **Sharing threshold** (γ'): adjusted so that any subpath rejected in the recursive call would also fail in the combined path.
  ```
  γ' = (γ · d(s,t) − d(v_s, v_t)) / d(s, v_s)
  ```
  Rationale: the segment from v_s to v_t (through v) is shared by ALL combinations, so subtract it from the budget.

- **Local optimality** (α'): scaled to maintain the same absolute distance threshold.
  ```
  α' = α · d(s,t) / d(s, v_s)
  ```
  If α' ≥ 1, stop recursion and return only the shortest path.

**Performance:** 84% success rate for 1st alternative, 1.1ms runtime.

### 4.3 Recursive Approach

**Extension of the two-step:** Instead of using the basic approach for the subproblems, use the two-step approach itself — i.e., recurse deeper.

**Stopping condition:** Introduce parameter **µ** (recommended: **µ = 0.3**). Stop recursion when `d(s', t') < µ · d(s,t)` (subproblem distance is less than 30% of original distance).

**Performance:** 90% success rate for 1st alternative, 2.3ms runtime. This matches the state-of-the-art and outperforms CRP on both success rate for 2nd/3rd alternatives and query time.

---

## 5. Implementation Details

### 5.1 Finding the Deviation Points a and b

To check bounded stretch, you need to find where the via-path `P_{s,v,t}` diverges from and rejoins the shortest path `P_{s,t}`.

**Efficient approach using shortcut unpacking:**

1. From the CCH query, obtain `P⁺_{s,v}` in the augmented graph G⁺ (path with shortcuts).
2. Walk along `P⁺_{s,v}` and compare with `P⁺_{s,t}` (the shortest path in G⁺).
3. Find the **first edge** on `P⁺_{s,v}` that deviates from the shortest path.
4. **Recursively unpack only that deviating edge** — at each step, only one edge needs unpacking.
5. Once the deviating edge is an original graph edge (not a shortcut), its tail is vertex `a`.
6. Maintain running distance during unpacking → gives `d(s,a)` and `d(a,v)` for free.
7. Apply the same process backwards from `t` to find `b`.

This avoids fully unpacking the entire path just to find the deviation points.

### 5.2 Limited Sharing Check

**For sharing with shortest path:** From the bounded stretch check, you already know:
```
c(P_{s,v,t} ∩ P_{s,t}) = c(P_{s,v,t}) − c(P_{a,v,b})
```
So this is essentially free.

**For sharing with previously selected alternatives:**
1. Maintain a **set of marked edges** (all edges on the shortest path + all previously accepted alternatives).
2. Fully unpack `P_{s,v,t}` in the original graph G.
3. Sum the costs of all marked edges along the unpacked path.
4. Optimization: only iterate over the detour segment a→v→b and add `d(s,a) + d(b,t)` directly.

### 5.3 T-test Implementation

For a via vertex `v` with deviation points `a` and `b`:

1. On the fully unpacked path `P_{s,v,t}`, find vertices `a'` and `b'` at distance `α · d(a,b)` from `v` (walking towards `a` and towards `b` respectively).
2. Compute `c(P_{a',v,b'})` from the unpacked path (trivial — just sum edge costs).
3. Run a **separate CCH query** for `d(a', b')`.
4. Pass the T-test if and only if `d(a', b') = c(P_{a',v,b'})`.

The T-test guarantees local optimality with respect to α, but may falsely reject alternatives that are locally optimal with respect to 2α.

### 5.4 Total Stretch Pruning (Performance Optimization)

During the upward CCH search, **skip any vertex** whose tentative distance from `s` (or `t`) already exceeds `(1+ε) · d(s,t)`. This is the same pruning as proposed by Buchhold et al. for standard CCH, extended with the stretch factor.

Since candidates are processed in order of increasing via-path length, once one fails the total stretch, all remaining candidates also fail → early termination.

### 5.5 Greedy Selection Justification

The paper experimentally validates that greedy selection (by shortest via-path first) is near-optimal: almost no candidates are rejected due to sharing with *previously selected* alternatives (the 3rd vs 4th bars in Figure 6 are nearly identical). This means the selection order rarely matters — earlier selections don't block later viable candidates.

---

## 6. CCH Query Recap (for context)

The standard CCH query for `d(s,t)`:

1. **Forward search:** Walk from `s` upward through the elimination tree to the root, relaxing vertices in elimination-tree order. Uses the augmented graph G⁺ (with shortcuts). Computes `d(s, v)` for all ancestors of `s`.
2. **Backward search:** Walk from `t` upward similarly. Computes `d(v, t)` for all ancestors of `t`.
3. **Meeting point:** `d(s,t) = min over all common ancestors v of { d(s,v) + d(v,t) }`.

The common ancestors set `A` is exactly the separator-based via-vertex candidate set.

---

## 7. Comparison Summary (DIMACS Europe, 18M vertices)

| Variant | 1st alt (%) | Time (ms) | 2nd alt (%) | Time (ms) | 3rd alt (%) | Time (ms) |
|---------|-------------|-----------|-------------|-----------|-------------|-----------|
| Basic (SeArCCH) | 65.3 | 0.5 | 37.3 | 0.7 | 17.2 | 1.0 |
| Two-Step | 84.1 | 1.1 | 62.9 | 1.6 | 38.7 | 2.3 |
| Recursive (µ=0.3) | 90.0 | 2.3 | 68.6 | 4.2 | 44.7 | 6.0 |
| CRP (reference) | 90.9 | 5.8 | 65.4 | 3.3 | 39.2 | 3.4 |

CCH base query: ~0.245ms. Slowdowns vs base: Basic ×2, Two-Step ×4.5, Recursive ×9.4 (1st alt).

---

## 8. Practical Notes for Your Implementation

### What You Already Have (from standard CCH)

- Elimination tree `T` → parent pointers for every vertex.
- Augmented graph `G⁺` → shortcut edges with middle-vertex info for unpacking.
- Forward/backward upward search → distances `d(s,v)`, `d(v,t)` for all ancestors.
- Shortest path extraction via shortcut unpacking.

### What You Need to Add

1. **Common ancestor identification:** After the bidirectional upward search, identify the set `A` of common ancestors. These are vertices visited by both forward and backward searches.

2. **Via-path sorting:** Sort `A` by `d(s,v) + d(v,t)`.

3. **Four-check pipeline** for each candidate:
   - Total stretch filter (trivial, just compare distances)
   - Bounded stretch (partial unpacking to find a, b)
   - Limited sharing (edge marking + cost summation)
   - T-test (separate CCH query for d(a', b'))

4. **Edge marking infrastructure:** A bitset or similar structure over all original edges, reset per query, to track edges belonging to the shortest path and accepted alternatives.

5. **Recursive decomposition** (for two-step / recursive variants):
   - Find highest elimination-tree vertex `v` on the shortest path.
   - Identify neighbors `v_s`, `v_t` on the shortest path.
   - Recursively invoke with adjusted γ', α' parameters.
   - Combine and filter results.

### Mapping to Your Current "Meeting Nodes" Approach

Your current approach of selecting meeting nodes between source and target is conceptually similar to this paper's approach of using separator vertices as via-vertex candidates. The key enhancements this paper provides:

- **Structured candidate set:** Use the elimination tree's common ancestors rather than ad-hoc meeting points.
- **Admissibility guarantees:** The four-check pipeline ensures alternatives are high-quality.
- **Hierarchical deepening:** The two-step and recursive approaches handle cases where the top-level separator is too small or unbalanced.
- **Efficient distance reuse:** All candidate via-path lengths are already computed by the standard query — no extra shortest-path computations needed for initial filtering.

### Separator Hierarchy Choice

- **InertialFlowCutter** (recommended default): smaller separators → faster queries, slightly fewer alternative candidates.
- **InertialFlow** (RoutingKit): larger separators → more candidates → marginally better success rate for basic variant, but the recursive approach makes this difference negligible.

The paper recommends InertialFlowCutter + recursive approach (µ=0.3) as the best overall configuration.
