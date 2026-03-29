# Progressive Snapping

## Context

The current spatial snap algorithm in `SpatialIndex::snap_to_edge()` finds
exactly **one** nearest edge via a KD-tree lookup of 10 nearest nodes. If that
single edge is on a one-way street pointing the wrong direction, on a
disconnected fragment, or simply too far from the query point (inside a building
compound with no nearby intersections), the query fails — either with a
`SnapTooFar` rejection or a `None` route result.

The goal is to return **multiple ranked snap candidates** so that
`query_coords()` can iterate through them until it finds a routable pair. This
eliminates the fragile dependency on a single snap result while keeping the
happy-path performance identical.

---

## 1. Current Architecture

### Snap flow (spatial.rs)

```
validated_snap(label, lat, lng, config)
  ├─ validate_coordinate()           ← bbox + range checks (unchanged)
  ├─ snap_to_edge(lat, lng)          ← returns single SnapResult
  │    ├─ tree.nearest_n(k=10)       ← 10 nearest intersection nodes
  │    ├─ for each node → outgoing edges → perpendicular distance
  │    └─ track single best
  └─ check snap_distance_m ≤ max     ← SnapTooFar rejection
```

### Routing flow (cch.rs / line_graph.rs)

```
query_coords(from, to)
  ├─ validated_snap("origin", from)     → single SnapResult or Err
  ├─ validated_snap("destination", to)  → single SnapResult or Err
  ├─ primary: query(src, dst)
  └─ fallback: try endpoint/edge combos from the SAME snap result
```

**Problem**: The fallback loop only explores edges adjacent to the **one**
snapped edge. If that edge is unroutable, the entire query fails — even though a
perfectly good road might be 50m further away.

### Key types and constants

| Item | Location | Value |
|------|----------|-------|
| `K_NEAREST_NODES` | `spatial.rs:45` | 10 |
| `max_snap_distance_m` | `bounds.rs:56` | 2000.0 (default) |
| `bbox_padding_m` | `bounds.rs:53` | 1500.0 (default) |
| `SnapResult` | `spatial.rs:10-22` | `{ edge_id, tail, head, t, snap_distance_m }` |
| `NearestNeighbour` | kiddo v5.2.4 | `{ distance: f32, item: u64 }` |

### kiddo API available

`ImmutableKdTree<f32, 2>` provides (relevant methods):

- `nearest_n::<SquaredEuclidean>(query, k)` → `Vec<NearestNeighbour>` sorted by
  distance (current usage)
- `nearest_n_within::<SquaredEuclidean>(query, max_dist, k, sorted)` → same, but
  bounded by squared-Euclidean radius

Both return results **sorted by distance** when `sorted = true` (which
`nearest_n` always passes).

---

## 2. Design: Multi-Candidate Snap

### 2.1 New method: `snap_candidates()`

Add to `SpatialIndex`:

```rust
/// Return up to `max_results` snap candidates, sorted by ascending
/// haversine distance. Deduplicates by edge_id.
pub fn snap_candidates(
    &self,
    lat: f32,
    lng: f32,
    max_results: usize,
) -> Vec<SnapResult>
```

**Algorithm** (same structure as `snap_to_edge`, but collects all candidates):

1. KD-tree: `tree.nearest_n(k = K_NEAREST_NODES)` — same 10 nearest nodes
2. For each node → outgoing edges → `haversine_perpendicular_distance_with_t()`
3. Collect **all** `(edge_id, tail, head, t, dist)` into a `Vec`
4. **Deduplicate** by `edge_id` (keep the entry with smallest distance — an edge
   can be reached via its tail or head node, and both might be in the 10 nearest)
5. Sort by `snap_distance_m` ascending
6. Truncate to `max_results`
7. Return

### 2.2 New method: `validated_snap_candidates()`

Add to `SpatialIndex`:

```rust
/// Validate coordinates, then return up to `max_results` snap candidates
/// within `max_snap_distance_m`.
pub fn validated_snap_candidates(
    &self,
    label: &'static str,
    lat: f32,
    lng: f32,
    config: &ValidationConfig,
    max_results: usize,
) -> Result<Vec<SnapResult>, CoordRejection>
```

**Steps:**

1. `validate_coordinate(label, lat, lng, bbox, config)?` — same pre-checks
2. `let all = self.snap_candidates(lat, lng, max_results + buffer)`
   - Use a slightly larger internal `max_results` to account for distance
     filtering
3. Filter to only candidates where `snap_distance_m <= config.max_snap_distance_m`
4. If result is empty → return `Err(CoordRejection::SnapTooFar { ... })` using
   the best candidate's distance (or `f64::MAX` if truly no edges found)
5. Return `Ok(filtered)`

### 2.3 Constant

```rust
const SNAP_MAX_CANDIDATES: usize = 5;
```

5 candidates is sufficient — in practice, the 2nd or 3rd candidate almost always
works. This bounds the worst-case CCH queries to 5 × 5 = 25 per request (but
early-exit means the typical case is 1-3 queries).

### 2.4 Rewrite `snap_to_edge()` atop `snap_candidates()`

To avoid duplication, rewrite the existing method as:

```rust
pub fn snap_to_edge(&self, lat: f32, lng: f32) -> SnapResult {
    self.snap_candidates(lat, lng, 1)
        .into_iter()
        .next()
        .expect("graph must have at least one edge near the query point")
}
```

This preserves the existing API for any callers that only need a single result.

Similarly, `validated_snap()` delegates to `validated_snap_candidates(..., 1)`:

```rust
pub fn validated_snap(...) -> Result<SnapResult, CoordRejection> {
    self.validated_snap_candidates(label, lat, lng, config, 1)
        .map(|mut v| v.remove(0))
}
```

---

## 3. Updated `query_coords()` — Both Engines

### 3.1 Normal graph engine (`cch.rs`)

```rust
pub fn query_coords(&mut self, from, to) -> Result<Option<QueryAnswer>, CoordRejection> {
    // Phase 1: multi-snap
    let src_snaps = self.spatial.validated_snap_candidates(
        "origin", from.0, from.1, &self.validation_config, SNAP_MAX_CANDIDATES,
    )?;
    let dst_snaps = self.spatial.validated_snap_candidates(
        "destination", to.0, to.1, &self.validation_config, SNAP_MAX_CANDIDATES,
    )?;

    // Phase 2: route with ranked candidates (early exit on first success)
    let mut best: Option<QueryAnswer> = None;

    for src in &src_snaps {
        for dst in &dst_snaps {
            let s = src.nearest_node();
            let d = dst.nearest_node();
            if let Some(answer) = self.query(s, d) {
                let is_better = best.as_ref()
                    .map_or(true, |b| answer.distance_ms < b.distance_ms);
                if is_better {
                    best = Some(answer);
                }
                break;  // found a route from this src, try next src
            }
        }
        if best.is_some() { break; }  // early exit
    }

    Ok(best.map(|a| Self::patch_coordinates(a, from, to)))
}
```

**Key change**: Instead of snapping once then trying endpoint permutations of
that one edge, we now try **different edges entirely** — each representing a
different physical road near the query point.

The endpoint permutation fallback (`src.tail`/`src.head` combinations) is
**removed** because:
- Multi-snap already covers nearby edges from different directions
- The old fallback was a workaround for having only one snap result
- Fewer CCH queries overall (5×5 worst case vs 1 + 4 endpoint combos + N
  expanded candidates)

### 3.2 Line-graph engine (`line_graph.rs`)

Same structure, but uses `query_trimmed()` and edge IDs directly:

```rust
pub fn query_coords(&mut self, from, to) -> Result<Option<QueryAnswer>, CoordRejection> {
    let src_snaps = self.original_spatial.validated_snap_candidates(
        "origin", from.0, from.1, &self.validation_config, SNAP_MAX_CANDIDATES,
    )?;
    let dst_snaps = self.original_spatial.validated_snap_candidates(
        "destination", to.0, to.1, &self.validation_config, SNAP_MAX_CANDIDATES,
    )?;

    let mut best: Option<QueryAnswer> = None;

    for src in &src_snaps {
        for dst in &dst_snaps {
            if let Some(answer) = self.query_trimmed(src.edge_id, dst.edge_id) {
                let is_better = best.as_ref()
                    .map_or(true, |b| answer.distance_ms < b.distance_ms);
                if is_better {
                    best = Some(answer);
                }
                break;
            }
        }
        if best.is_some() { break; }
    }

    Ok(best.map(|a| Self::patch_coordinates(a, from, to)))
}
```

`collect_original_edge_candidates()` is **removed** — it served the same purpose
(expanding from a single snap) that multi-snap now handles more effectively.

---

## 4. Files to Modify

| File | Change |
|------|--------|
| `hanoi-core/src/spatial.rs` | Add `snap_candidates()`, `validated_snap_candidates()`, `SNAP_MAX_CANDIDATES`. Rewrite `snap_to_edge()` and `validated_snap()` as thin wrappers. |
| `hanoi-core/src/cch.rs` | Rewrite `query_coords()` to use multi-snap loop. Remove endpoint permutation fallback. |
| `hanoi-core/src/line_graph.rs` | Rewrite `query_coords()` to use multi-snap loop. Remove `collect_original_edge_candidates()`. |
| `docs/CHANGELOGS.md` | Log changes. |

**Not modified:**
- `bounds.rs` — `ValidationConfig`, `CoordRejection` unchanged
- `hanoi-server/*` — callers of `query_coords()` are unchanged (same signature)
- `hanoi-cli/*` — same
- `hanoi-bench/*` — same
- Normal graph `query()` and line-graph `query()` / `query_trimmed()` — unchanged

---

## 5. Edge Cases

### 5.1 All candidates fail routing

If no `(src, dst)` pair produces a route, `query_coords()` returns `Ok(None)` —
same behavior as today, but reached after trying more candidates.

### 5.2 Only one candidate within distance threshold

Degenerates to current behavior: one snap result, one routing attempt. No
performance regression.

### 5.3 Duplicate edges across nodes

Two of the 10 nearest KD-tree nodes may share an edge (node A's outgoing edge
A→B is also reachable via node B). `snap_candidates()` deduplicates by
`edge_id`, keeping the entry with the smaller `snap_distance_m`.

### 5.4 Very sparse graph regions

If `K_NEAREST_NODES = 10` yields fewer than `SNAP_MAX_CANDIDATES` distinct
edges, we simply return fewer candidates. The algorithm degrades gracefully.

### 5.5 Dense Hanoi urban core

Typical Hanoi intersection has 3-4 outgoing edges. 10 nearest nodes → ~30-40
candidate edges before dedup → plenty of diversity for 5 candidates.

---

## 6. Performance

### Happy path (no change)

- 1 KD-tree query (same as today — just collect more from the same result)
- 1 CCH query (first candidate succeeds)
- Total: ~identical latency

### Worst case (all 5×5 fail)

- 1 KD-tree query per endpoint (2 total)
- Up to 25 CCH queries
- But: early-exit means typical failing queries try 3-5 CCH queries before
  finding a route, not 25
- Still sub-100ms for Hanoi's graph size

### Comparison with current fallback

Current worst case: 1 snap + 4 endpoint combos + N expanded edge candidates
(line graph). This can already be 10+ CCH queries. Multi-snap replaces this with
a more principled approach that's no worse in query count and much better in
coverage.

---

## 7. Invariants

1. **`snap_to_edge()` and `validated_snap()` are preserved** as public methods
   with identical signatures and behavior. No downstream breakage.
2. **`query()` methods are never modified** — only `query_coords()` changes.
3. **`CoordRejection` variants are unchanged** — error responses stay the same.
4. **`SnapResult` struct is unchanged** — all existing fields preserved.
5. **Candidate ordering is deterministic** — sorted by `snap_distance_m`, with
   `edge_id` as tiebreaker for equal distances.
