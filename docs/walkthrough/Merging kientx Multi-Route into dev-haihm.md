# Merging `kientx` Multi-Route into `dev-haihm`

> Goal: bring the K-alternative route implementation from `kientx` into the
> current `dev-haihm` branch without regressing the newer `dev-haihm`
> additions such as route export metadata, turn refinement, traffic overlay,
> camera overlay, route evaluation, and the bundled server UI.
>
> Shared merge base: `05bddb5e3178c7c005e4ee517e3e4e70e10c59c6`
>
> Feature commit on `kientx`: `8f021657a13d7595f0912a9fb0a9bd3487fb50d1`

## Recommendation

Do **not** merge the whole `kientx` branch and do **not** cherry-pick
`8f021657` as a single commit.

That commit mixes the real multi-route work with:

- unrelated docs,
- helper scripts,
- log and GeoJSON artifacts,
- a simplified older `hanoi-server` main/router shape,
- `Cargo.toml` changes that would drop newer `dev-haihm` features.

The safe path is:

1. copy the new core algorithm file as-is,
2. manually port the small integration points into the newer `dev-haihm`
   files,
3. optionally port CLI/docs/scripts after the backend is working.

---

## 1. Trace the source files from `kientx`

Use `git ls-tree` to find the relevant files, then `git show` to inspect the
actual source before editing anything.

### 1.1 File discovery

```bash
git ls-tree -r --name-only kientx | rg '^CCH-Hanoi/crates/hanoi-(core|server|cli)/src/(multi_route|cch|line_graph|engine|handlers|state|types|main)\.rs$|^docs/walkthrough/multi_route_algorithm_analysis\.md$'
```

Expected core hits:

```text
CCH-Hanoi/crates/hanoi-cli/src/main.rs
CCH-Hanoi/crates/hanoi-core/src/cch.rs
CCH-Hanoi/crates/hanoi-core/src/line_graph.rs
CCH-Hanoi/crates/hanoi-core/src/multi_route.rs
CCH-Hanoi/crates/hanoi-server/src/engine.rs
CCH-Hanoi/crates/hanoi-server/src/handlers.rs
CCH-Hanoi/crates/hanoi-server/src/main.rs
CCH-Hanoi/crates/hanoi-server/src/state.rs
CCH-Hanoi/crates/hanoi-server/src/types.rs
docs/walkthrough/multi_route_algorithm_analysis.md
```

### 1.2 Inspect the actual source from `kientx`

```bash
git show kientx:CCH-Hanoi/crates/hanoi-core/src/multi_route.rs | sed -n '1,220p'
git show kientx:CCH-Hanoi/crates/hanoi-core/src/cch.rs | sed -n '1,340p'
git show kientx:CCH-Hanoi/crates/hanoi-core/src/line_graph.rs | sed -n '1,720p'
git show kientx:CCH-Hanoi/crates/hanoi-server/src/engine.rs | sed -n '1,320p'
git show kientx:CCH-Hanoi/crates/hanoi-cli/src/main.rs | sed -n '1,520p'
git show kientx:docs/walkthrough/multi_route_algorithm_analysis.md | sed -n '1,220p'
```

### 1.3 Confirm the full branch delta

```bash
git log --oneline --decorate --no-merges dev-haihm..kientx
git diff --name-status dev-haihm..kientx
git show --stat --summary 8f021657a13d7595f0912a9fb0a9bd3487fb50d1
```

The important takeaway is that the multi-route implementation is only a small
subset of the files changed on `kientx`.

---

## 2. Decide what to port

### Required

| Path | Action | Notes |
|------|--------|-------|
| `CCH-Hanoi/crates/hanoi-core/src/multi_route.rs` | Copy as-is | Brand new file; no `dev-haihm` equivalent exists |
| `CCH-Hanoi/crates/hanoi-core/src/lib.rs` | Add module export | One-line additive change |
| `CCH-Hanoi/crates/hanoi-core/src/cch.rs` | Manual port | Must preserve `route_arc_ids` and `weight_path_ids` from `dev-haihm` |
| `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs` | Manual port | Must preserve `original_arc_id_of_lg_node`, roundabout-aware turns, and `refine_turns()` |
| `CCH-Hanoi/crates/hanoi-server/src/types.rs` | Manual port | Add query params without removing newer response fields |
| `CCH-Hanoi/crates/hanoi-server/src/state.rs` | Manual port | Add `alternatives` and `stretch` to `QueryMsg` only |
| `CCH-Hanoi/crates/hanoi-server/src/handlers.rs` | Manual port | Pass new params through to engine thread |
| `CCH-Hanoi/crates/hanoi-server/src/engine.rs` | Manual port | Add multi-route dispatch and multi-feature formatting without losing current export metadata |

### Recommended but optional

| Path | Action | Notes |
|------|--------|-------|
| `CCH-Hanoi/crates/hanoi-cli/src/main.rs` | Manual port | Adds `--alternatives` / `--stretch` support |
| `docs/walkthrough/multi_route_algorithm_analysis.md` | Copy as reference doc | Useful algorithm notes from `kientx` |

### Skip for this merge

| Path group | Why skip |
|------------|----------|
| `CCH-Hanoi/crates/hanoi-server/src/main.rs` | `dev-haihm` has a newer router/state model with UI, traffic, camera, and route evaluation |
| `CCH-Hanoi/crates/hanoi-server/Cargo.toml` | `kientx` version removes newer dependencies used on `dev-haihm` |
| `CCH-Hanoi/crates/hanoi-gateway/*` | Not needed for K-alternative backend support |
| `CCH-Hanoi/crates/hanoi-tools/src/bin/diagnose_turn.rs` | Useful tool, but independent from multi-route |
| `scripts/multi_query_ui.html` and other helper scripts | Optional follow-up; not required to land backend support |
| logs, `.geojson`, benchmark outputs | Artifacts only |
| `CCH-Generator`, `RoutingKit`, `CCH_Data_Pipeline` changes | Not part of the K-alternative core merge |

---

## 3. Create a merge branch

```bash
git checkout dev-haihm
git checkout -b feat/kientx-multi-route
```

Do the port on this branch, not directly on `dev-haihm`.

---

## 4. Port `hanoi-core`

### 4.1 Copy `multi_route.rs`

This file can be imported directly from `kientx`.

```bash
git show kientx:CCH-Hanoi/crates/hanoi-core/src/multi_route.rs > CCH-Hanoi/crates/hanoi-core/src/multi_route.rs
```

Why this file is safe to copy directly:

- it is new on `kientx`,
- it depends only on public `rust_road_router` CCH query APIs,
- it does not depend on the newer `dev-haihm` route export or turn metadata.

### 4.2 Register the module in `lib.rs`

Add one line:

```diff
 pub mod line_graph;
+pub mod multi_route;
 pub mod spatial;
 pub mod via_way_restriction;
```

### 4.3 Port normal-graph integration in `cch.rs`

Start from:

```bash
git show kientx:CCH-Hanoi/crates/hanoi-core/src/cch.rs | sed -n '1,340p'
```

Bring over the import:

```diff
+use crate::multi_route::{GEO_OVER_REQUEST, MAX_GEO_RATIO, MultiRouteServer};
```

Then add the two methods below to `impl<'a> QueryEngine<'a>`.

Important `dev-haihm` adaptation:

- on `kientx`, `multi_query()` is `&self`;
- on `dev-haihm`, make it `&mut self`, because the current branch reconstructs
  `route_arc_ids` through `self.reconstruct_arc_ids(&path)`.

#### Exact port position in the current file

In the current [cch.rs](/home/thomas/VTS/Hanoi-Routing/CCH-Hanoi/crates/hanoi-core/src/cch.rs),
insert the new methods inside the existing `impl<'a> QueryEngine<'a>` block,
directly **after** `fn reconstruct_arc_ids(...)` and **before**
`pub fn update_weights(...)`.

That local area should become:

```rust
impl<'a> QueryEngine<'a> {
    pub fn query(&mut self, ...) -> Option<QueryAnswer> { ... }
    pub fn query_coords(&mut self, ...) -> Result<Option<QueryAnswer>, CoordRejection> { ... }
    fn patch_coordinates(...) -> QueryAnswer { ... }
    fn reconstruct_arc_ids(&self, path: &[NodeId]) -> Option<Vec<u32>> { ... }

    // INSERT HERE:
    pub fn multi_query(&mut self, ...) -> Vec<QueryAnswer> { ... }
    pub fn multi_query_coords(&mut self, ...) -> Result<Vec<QueryAnswer>, CoordRejection> { ... }

    pub fn update_weights(&mut self, weights: &[Weight]) { ... }
}
```

If you accidentally paste the methods outside the impl block, Rust will fail to
parse the file. If you paste them into `CchContext`, they will compile in the
wrong place and still not be callable on `QueryEngine`.

```rust
/// Find up to `max_alternatives` alternative routes by node IDs.
pub fn multi_query(
    &mut self,
    from: NodeId,
    to: NodeId,
    max_alternatives: usize,
    stretch_factor: f64,
) -> Vec<QueryAnswer> {
    let customized = self.server.customized();
    let mut multi = MultiRouteServer::new(customized);
    let request_count = max_alternatives
        .saturating_mul(GEO_OVER_REQUEST)
        .max(max_alternatives + 10);
    let candidates = multi.multi_query(from, to, request_count, stretch_factor);

    let mut results: Vec<QueryAnswer> = Vec::with_capacity(max_alternatives);
    let mut shortest_geo_dist: Option<f64> = None;

    for alt in candidates {
        if results.len() >= max_alternatives {
            break;
        }
        if alt.path.is_empty() {
            continue;
        }

        let coordinates: Vec<(f32, f32)> = alt
            .path
            .iter()
            .map(|&node| {
                (
                    self.context.graph.latitude[node as usize],
                    self.context.graph.longitude[node as usize],
                )
            })
            .collect();
        let distance_m = route_distance_m(&coordinates);

        if let Some(base) = shortest_geo_dist {
            if distance_m > base * MAX_GEO_RATIO {
                continue;
            }
        } else {
            shortest_geo_dist = Some(distance_m);
        }

        let route_arc_ids = self.reconstruct_arc_ids(&alt.path).unwrap_or_default();
        let weight_path_ids = route_arc_ids.clone();

        results.push(QueryAnswer {
            distance_ms: alt.distance,
            distance_m,
            route_arc_ids,
            weight_path_ids,
            path: alt.path,
            coordinates,
            turns: vec![],
            origin: None,
            destination: None,
        });
    }

    results
}

/// Find up to `max_alternatives` alternative routes by coordinates.
pub fn multi_query_coords(
    &mut self,
    from: (f32, f32),
    to: (f32, f32),
    max_alternatives: usize,
    stretch_factor: f64,
) -> Result<Vec<QueryAnswer>, CoordRejection> {
    let src_snaps = self.spatial.validated_snap_candidates(
        "origin",
        from.0,
        from.1,
        &self.validation_config,
        SNAP_MAX_CANDIDATES,
    )?;
    let dst_snaps = self.spatial.validated_snap_candidates(
        "destination",
        to.0,
        to.1,
        &self.validation_config,
        SNAP_MAX_CANDIDATES,
    )?;

    for src in &src_snaps {
        for dst in &dst_snaps {
            let answers = self.multi_query(
                src.nearest_node(),
                dst.nearest_node(),
                max_alternatives,
                stretch_factor,
            );
            if !answers.is_empty() {
                let patched: Vec<QueryAnswer> = answers
                    .into_iter()
                    .map(|a| Self::patch_coordinates(a, from, to))
                    .collect();
                return Ok(patched);
            }
        }
    }

    Ok(Vec::new())
}
```

#### What this port is doing

- `MultiRouteServer::multi_query(...)` returns candidate node paths.
- `route_distance_m(...)` applies the same geographic-detour filter as
  `kientx`.
- `self.reconstruct_arc_ids(&alt.path)` is the important `dev-haihm`
  adaptation. It preserves the current export/replay metadata by filling:
  - `route_arc_ids`
  - `weight_path_ids`
- `multi_query_coords(...)` mirrors the existing `query_coords(...)` flow:
  snap first, then patch origin/destination metadata only after a route is
  accepted.

#### Minimal compile checklist for this section

If this section is ported correctly:

- `QueryEngine` now has `multi_query(...)`
- `QueryEngine` now has `multi_query_coords(...)`
- `cch.rs` has the `crate::multi_route` import
- `lib.rs` exports `pub mod multi_route;`

If it fails to compile, the usual causes are:

- missing `pub mod multi_route;` in `lib.rs`
- forgot the `use crate::multi_route::{...};` import
- used `&self` instead of `&mut self`
- pasted the methods outside `impl<'a> QueryEngine<'a>`

### 4.4 Port line-graph integration in `line_graph.rs`

Start from:

```bash
git show kientx:CCH-Hanoi/crates/hanoi-core/src/line_graph.rs | sed -n '1,720p'
```

Bring over the import:

```diff
+use crate::multi_route::{GEO_OVER_REQUEST, MAX_GEO_RATIO, MultiRouteServer};
```

Then port three pieces:

1. `build_answer_from_lg_path()`
2. `multi_query()`
3. `multi_query_coords()`

This file is the main manual adaptation point. Use the `kientx` logic, but
preserve these `dev-haihm` details:

- `route_arc_ids` must come from `self.context.original_arc_id_of_lg_node`
  instead of using raw line-graph node IDs directly.
- `weight_path_ids` must remain the full line-graph node sequence.
- `compute_turns()` must use the current branch signature, including
  `&self.context.is_arc_roundabout`.
- `refine_turns(&mut turns, &coordinates)` must still run.

#### Exact port position in the current file

In the current
[line_graph.rs](/home/thomas/VTS/Hanoi-Routing/CCH-Hanoi/crates/hanoi-core/src/line_graph.rs),
do **not** replace the existing `query_trimmed()`, `query()`, or
`query_coords()` implementations.

Insert the new block inside `impl<'a> LineGraphQueryEngine<'a>` directly
**after** `fn patch_coordinates(...)` and **before**
`pub fn update_weights(...)`.

That local area should become:

```rust
impl<'a> LineGraphQueryEngine<'a> {
    fn query_trimmed(&mut self, ...) -> Option<QueryAnswer> { ... }
    pub fn query(&mut self, ...) -> Option<QueryAnswer> { ... }
    pub fn query_coords(&mut self, ...) -> Result<Option<QueryAnswer>, CoordRejection> { ... }
    fn patch_coordinates(...) -> QueryAnswer { ... }

    // INSERT HERE:
    fn build_answer_from_lg_path(&self, ...) -> Option<QueryAnswer> { ... }
    pub fn multi_query(&self, ...) -> Vec<QueryAnswer> { ... }
    pub fn multi_query_coords(&self, ...) -> Result<Vec<QueryAnswer>, CoordRejection> { ... }

    pub fn update_weights(&mut self, weights: &[Weight]) { ... }
}
```

This keeps the existing single-route logic intact and adds multi-route support
as a new tail section in the impl block.

#### Port step 1: add the helper

The helper below is the correct `dev-haihm`-compatible version. Paste it
exactly:

```rust
fn build_answer_from_lg_path(
    &self,
    cch_distance: Weight,
    lg_path: &[NodeId],
    source_edge_cost: Weight,
    trimmed: bool,
) -> Option<QueryAnswer> {
    if lg_path.is_empty() {
        return None;
    }

    let distance_ms = cch_distance.saturating_add(source_edge_cost);

    let effective_path: &[NodeId] = if trimmed && lg_path.len() > 2 {
        &lg_path[1..lg_path.len() - 1]
    } else if trimmed {
        &[]
    } else {
        lg_path
    };

    let route_arc_ids: Vec<u32> = effective_path
        .iter()
        .map(|&lg_node| self.context.original_arc_id_of_lg_node[lg_node as usize])
        .collect();
    let weight_path_ids: Vec<u32> = lg_path.iter().map(|&lg_node| lg_node as u32).collect();

    let mut path: Vec<NodeId> = effective_path
        .iter()
        .map(|&lg_node| self.context.original_tail[lg_node as usize])
        .collect();

    if trimmed {
        if let Some(&last_edge) = effective_path.last() {
            path.push(self.context.original_head[last_edge as usize]);
        } else if lg_path.len() >= 2 {
            path.push(self.context.original_head[lg_path[0] as usize]);
        }
    } else if let Some(&last_edge) = lg_path.last() {
        path.push(self.context.original_head[last_edge as usize]);
    }

    let coordinates: Vec<(f32, f32)> = path
        .iter()
        .map(|&node| {
            (
                self.context.original_latitude[node as usize],
                self.context.original_longitude[node as usize],
            )
        })
        .collect();

    let mut turns = compute_turns(
        effective_path,
        &self.context.original_tail,
        &self.context.original_head,
        &self.context.original_first_out,
        &self.context.original_latitude,
        &self.context.original_longitude,
        &self.context.is_arc_roundabout,
    );
    refine_turns(&mut turns, &coordinates);

    let distance_m = route_distance_m(&coordinates);

    Some(QueryAnswer {
        distance_ms,
        distance_m,
        route_arc_ids,
        weight_path_ids,
        path,
        coordinates,
        turns,
        origin: None,
        destination: None,
    })
}
```

#### Port step 2: add `multi_query()`

Paste this immediately below `build_answer_from_lg_path(...)`:

```rust
/// Find up to `max_alternatives` alternative routes by line-graph node IDs
/// (= original edge indices). Each route gets source-edge correction,
/// LG->original node mapping, and turn annotation.
pub fn multi_query(
    &self,
    source_edge: EdgeId,
    target_edge: EdgeId,
    max_alternatives: usize,
    stretch_factor: f64,
) -> Vec<QueryAnswer> {
    let customized = self.server.customized();
    let mut multi = MultiRouteServer::new(customized);
    let request_count = max_alternatives
        .saturating_mul(GEO_OVER_REQUEST)
        .max(max_alternatives + 10);
    let candidates = multi.multi_query(
        source_edge as NodeId,
        target_edge as NodeId,
        request_count,
        stretch_factor,
    );

    let source_edge_cost = self.context.original_travel_time[source_edge as usize];

    let mut results: Vec<QueryAnswer> = Vec::with_capacity(max_alternatives);
    let mut shortest_geo_dist: Option<f64> = None;

    for alt in candidates {
        if results.len() >= max_alternatives {
            break;
        }
        if let Some(answer) =
            self.build_answer_from_lg_path(alt.distance, &alt.path, source_edge_cost, false)
        {
            if let Some(base) = shortest_geo_dist {
                if answer.distance_m > base * MAX_GEO_RATIO {
                    continue;
                }
            } else {
                shortest_geo_dist = Some(answer.distance_m);
            }
            results.push(answer);
        }
    }

    results
}
```

#### Port step 3: add `multi_query_coords()`

Paste this immediately below `multi_query(...)`:

```rust
/// Find up to `max_alternatives` alternative routes by coordinates.
/// Snaps in the original graph coordinate space, then runs multi_query on
/// the snapped edge IDs with trimming.
pub fn multi_query_coords(
    &self,
    from: (f32, f32),
    to: (f32, f32),
    max_alternatives: usize,
    stretch_factor: f64,
) -> Result<Vec<QueryAnswer>, CoordRejection> {
    let src_snaps = self.original_spatial.validated_snap_candidates(
        "origin",
        from.0,
        from.1,
        &self.validation_config,
        SNAP_MAX_CANDIDATES,
    )?;
    let dst_snaps = self.original_spatial.validated_snap_candidates(
        "destination",
        to.0,
        to.1,
        &self.validation_config,
        SNAP_MAX_CANDIDATES,
    )?;

    for src in &src_snaps {
        for dst in &dst_snaps {
            let customized = self.server.customized();
            let mut multi = MultiRouteServer::new(customized);
            let request_count = max_alternatives
                .saturating_mul(GEO_OVER_REQUEST)
                .max(max_alternatives + 10);
            let candidates = multi.multi_query(
                src.edge_id as NodeId,
                dst.edge_id as NodeId,
                request_count,
                stretch_factor,
            );

            if candidates.is_empty() {
                continue;
            }

            let source_edge_cost = self.context.original_travel_time[src.edge_id as usize];

            let mut answers: Vec<QueryAnswer> = Vec::with_capacity(max_alternatives);
            let mut shortest_geo_dist: Option<f64> = None;

            for alt in candidates {
                if answers.len() >= max_alternatives {
                    break;
                }
                if let Some(answer) = self.build_answer_from_lg_path(
                    alt.distance,
                    &alt.path,
                    source_edge_cost,
                    true,
                ) {
                    let answer = Self::patch_coordinates(answer, from, to);
                    if let Some(base) = shortest_geo_dist {
                        if answer.distance_m > base * MAX_GEO_RATIO {
                            continue;
                        }
                    } else {
                        shortest_geo_dist = Some(answer.distance_m);
                    }
                    answers.push(answer);
                }
            }

            if !answers.is_empty() {
                return Ok(answers);
            }
        }
    }

    Ok(Vec::new())
}
```

#### Why this section must be ported manually

You cannot paste the `kientx` helper verbatim, because `dev-haihm` has already
extended the line-graph route model:

- `query()` and `query_trimmed()` already populate `route_arc_ids`
- `weight_path_ids` already carry exact line-graph replay IDs
- `compute_turns(...)` now takes `is_arc_roundabout`
- `refine_turns(...)` is part of the current turn pipeline

If you copy the old helper unchanged, you will either:

- lose route replay metadata,
- get the wrong `compute_turns(...)` argument count,
- or skip turn refinement.

#### Sanity-check against existing single-route code

Before building, verify your new helper mirrors the same data model already
used by the current single-route methods:

- `query_trimmed()` already shows how trimmed line-graph paths become
  `route_arc_ids` + `weight_path_ids`
- `query()` already shows how full line-graph paths become
  `route_arc_ids` + `weight_path_ids`
- the multi-route helper should follow the same mapping rules, just applied to
  candidate paths returned by `MultiRouteServer`

#### Immediate build check for this section

```bash
cargo build --manifest-path CCH-Hanoi/Cargo.toml -p hanoi-core
```

### 4.5 Immediate core verification

```bash
cargo fmt --manifest-path CCH-Hanoi/Cargo.toml --all
cargo build --manifest-path CCH-Hanoi/Cargo.toml -p hanoi-core
```

---

## 5. Wire the server API

### 5.1 `types.rs`

Inspect the `kientx` version:

```bash
git show kientx:CCH-Hanoi/crates/hanoi-server/src/types.rs | sed -n '1,220p'
```

#### Exact port position

In the current
[types.rs](/home/thomas/VTS/Hanoi-Routing/CCH-Hanoi/crates/hanoi-server/src/types.rs),
edit the existing `FormatParam` struct only. Insert the two new fields directly
after `colors`.

The final block should look like:

```diff
 pub struct FormatParam {
     pub format: Option<String>,
     pub colors: Option<String>,
+    pub alternatives: Option<u32>,
+    pub stretch: Option<f64>,
 }
```

Do **not** remove the current `QueryResponse` fields already present on
`dev-haihm` such as:

- `graph_type`,
- `route_arc_ids`,
- `weight_path_ids`.

### 5.2 `state.rs`

Inspect:

```bash
git show kientx:CCH-Hanoi/crates/hanoi-server/src/state.rs | sed -n '1,220p'
```

#### Exact port position

In the current
[state.rs](/home/thomas/VTS/Hanoi-Routing/CCH-Hanoi/crates/hanoi-server/src/state.rs),
edit `QueryMsg` only. Insert the new fields after `colors` and before `reply`.

The final `QueryMsg` should look like:

```diff
 pub struct QueryMsg {
     pub request: QueryRequest,
     pub format: Option<String>,
     pub colors: bool,
+    pub alternatives: u32,
+    pub stretch: f64,
     pub reply: tokio::sync::oneshot::Sender<Result<serde_json::Value, CoordRejection>>,
 }
```

Do not touch the newer `AppState` members for traffic, route evaluation, camera
overlay, baseline reset, and related UI support.

### 5.3 `handlers.rs`

Inspect:

```bash
git show kientx:CCH-Hanoi/crates/hanoi-server/src/handlers.rs | sed -n '1,220p'
```

#### Exact port position

In the current
[handlers.rs](/home/thomas/VTS/Hanoi-Routing/CCH-Hanoi/crates/hanoi-server/src/handlers.rs),
only edit the `let msg = QueryMsg { ... }` literal inside `handle_query()`.

The final literal should become:

```diff
 let msg = QueryMsg {
     request: req,
     format: params.format,
     colors: params.colors.is_some(),
+    alternatives: params.alternatives.unwrap_or(0),
+    stretch: params
+        .stretch
+        .unwrap_or(hanoi_core::multi_route::DEFAULT_STRETCH),
     reply: tx,
 };
```

### 5.4 `engine.rs`

Inspect:

```bash
git show kientx:CCH-Hanoi/crates/hanoi-server/src/engine.rs | sed -n '1,320p'
```

This is the second manual-adaptation file after `line_graph.rs`.

Do **not** replace the whole file with the `kientx` version. The current branch
already contains newer response metadata and helper endpoints.

#### Port step 1: update the two engine-thread call sites

In the current
[engine.rs](/home/thomas/VTS/Hanoi-Routing/CCH-Hanoi/crates/hanoi-server/src/engine.rs),
edit only the `dispatch_*` calls inside `run_normal()` and `run_line_graph()`.

Change:

```rust
dispatch_normal(&mut engine, qm.request, qm.format.as_deref(), qm.colors);
```

to:

```rust
dispatch_normal(
    &mut engine,
    qm.request,
    qm.format.as_deref(),
    qm.colors,
    qm.alternatives,
    qm.stretch,
);
```

Change:

```rust
dispatch_line_graph(&mut engine, qm.request, qm.format.as_deref(), qm.colors);
```

to:

```rust
dispatch_line_graph(
    &mut engine,
    qm.request,
    qm.format.as_deref(),
    qm.colors,
    qm.alternatives,
    qm.stretch,
);
```

#### Port step 2: extend the dispatch function signatures

Change the signatures to:

```rust
fn dispatch_normal(
    engine: &mut QueryEngine<'_>,
    req: QueryRequest,
    format: Option<&str>,
    colors: bool,
    alternatives: u32,
    stretch: f64,
) -> Result<Value, CoordRejection>
```

and:

```rust
fn dispatch_line_graph(
    engine: &mut LineGraphQueryEngine<'_>,
    req: QueryRequest,
    format: Option<&str>,
    colors: bool,
    alternatives: u32,
    stretch: f64,
) -> Result<Value, CoordRejection>
```

#### Port step 3: add the early multi-route branch to each dispatch helper

Add this at the top of `dispatch_normal(...)`, before the existing single-route
`let answer = ...` logic:

```rust
if alternatives > 0 {
    let answers = if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
        (req.from_lat, req.from_lng, req.to_lat, req.to_lng)
    {
        engine.multi_query_coords((flat, flng), (tlat, tlng), alternatives as usize, stretch)?
    } else if let (Some(from), Some(to)) = (req.from_node, req.to_node) {
        engine.multi_query(from, to, alternatives as usize, stretch)
    } else {
        Vec::new()
    };

    tracing::info!(num_routes = answers.len(), "multi-route query completed");
    return Ok(format_multi_response(answers, format, colors, "normal"));
}
```

Add the line-graph equivalent at the top of `dispatch_line_graph(...)`:

```rust
if alternatives > 0 {
    let answers = if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
        (req.from_lat, req.from_lng, req.to_lat, req.to_lng)
    {
        engine.multi_query_coords((flat, flng), (tlat, tlng), alternatives as usize, stretch)?
    } else if let (Some(from), Some(to)) = (req.from_node, req.to_node) {
        engine.multi_query(from, to, alternatives as usize, stretch)
    } else {
        Vec::new()
    };

    tracing::info!(num_routes = answers.len(), "multi-route query completed");
    return Ok(format_multi_response(answers, format, colors, "line_graph"));
}
```

#### Port step 4: add the multi-route formatting helpers

Leave the existing single-route helpers alone:

- `format_response(...)`
- `answer_to_response(...)`
- `answer_to_geojson(...)`

Insert the new block directly **after** `answer_to_geojson(...)` and **before**
the end of the file:

```rust
/// Color palette for multi-route visualization.
const ROUTE_COLORS: &[&str] = &[
    "#ff5500", "#0055ff", "#00aa44", "#aa00cc", "#cc8800",
    "#e6194b", "#3cb44b", "#4363d8", "#f58231", "#911eb4",
];

fn format_multi_response(
    answers: Vec<QueryAnswer>,
    format: Option<&str>,
    colors: bool,
    graph_type: &'static str,
) -> Value {
    match format {
        Some("json") => {
            let responses: Vec<QueryResponse> = answers
                .into_iter()
                .map(|a| answer_to_response(Some(a), graph_type))
                .collect();
            serde_json::to_value(responses).unwrap()
        }
        _ => answers_to_geojson(answers, colors, graph_type),
    }
}

fn answers_to_geojson(
    answers: Vec<QueryAnswer>,
    colors: bool,
    graph_type: &'static str,
) -> Value {
    if answers.is_empty() {
        return serde_json::json!({
            "type": "FeatureCollection",
            "features": []
        });
    }

    let features: Vec<Value> = answers
        .into_iter()
        .enumerate()
        .map(|(idx, a)| {
            let QueryAnswer {
                distance_ms,
                distance_m,
                route_arc_ids,
                weight_path_ids,
                path,
                coordinates,
                turns,
                origin,
                destination,
            } = a;

            let coords: Vec<[f32; 2]> = coordinates
                .iter()
                .map(|&(lat, lng)| [lng, lat])
                .collect();

            let mut props = serde_json::json!({
                "source": "hanoi_server",
                "export_version": 1,
                "graph_type": graph_type,
                "distance_ms": distance_ms,
                "distance_m": distance_m,
                "path_nodes": path,
                "route_arc_ids": route_arc_ids,
                "weight_path_ids": weight_path_ids,
                "route_index": idx,
            });

            let obj = props.as_object_mut().unwrap();
            if let Some((lat, lng)) = origin {
                obj.insert("origin".into(), serde_json::json!([lat, lng]));
            }
            if let Some((lat, lng)) = destination {
                obj.insert("destination".into(), serde_json::json!([lat, lng]));
            }
            if !turns.is_empty() {
                obj.insert("turns".into(), serde_json::to_value(turns).unwrap());
            }
            if colors {
                let color = ROUTE_COLORS[idx % ROUTE_COLORS.len()];
                obj.insert("stroke".into(), serde_json::json!(color));
                obj.insert("stroke-width".into(), serde_json::json!(if idx == 0 { 10 } else { 6 }));
                obj.insert("fill".into(), serde_json::json!(color));
                obj.insert("fill-opacity".into(), serde_json::json!(0.3));
            }

            serde_json::json!({
                "type": "Feature",
                "geometry": {
                    "type": "LineString",
                    "coordinates": coords
                },
                "properties": props
            })
        })
        .collect();

    serde_json::json!({
        "type": "FeatureCollection",
        "features": features
    })
}
```

#### Why this helper must be adapted from `kientx`

The raw `kientx` helper is too old for the current branch because the current
single-route exporter already includes:

- `graph_type`
- `source`
- `export_version`
- `path_nodes`
- `route_arc_ids`
- `weight_path_ids`

Your multi-route output should include the same metadata so exported routes stay
compatible with the rest of `dev-haihm`.

#### Build check for the server layer

```bash
cargo build --manifest-path CCH-Hanoi/Cargo.toml -p hanoi-server
```

Common failure patterns:

- `no field alternatives on type QueryMsg`
  you forgot `state.rs`
- `no field alternatives on type FormatParam`
  you forgot `types.rs`
- `no method named multi_query_coords`
  you have not finished section 4
- wrong argument count for `dispatch_normal(...)`
  only one call site or one signature was updated

### 5.5 Server-side verification

```bash
cargo build --manifest-path CCH-Hanoi/Cargo.toml -p hanoi-server
```

---

## 6. Optional CLI port

If you want CLI access, inspect:

```bash
git show kientx:CCH-Hanoi/crates/hanoi-cli/src/main.rs | sed -n '1,520p'
```

This step is optional, but if you want the CLI to expose alternative routing,
port it the same way: targeted edits, not a whole-file replacement.

#### Port step 1: add the new flags to `Command::Query`

In the current
[main.rs](/home/thomas/VTS/Hanoi-Routing/CCH-Hanoi/crates/hanoi-cli/src/main.rs),
inside `Command::Query`, insert these fields directly after `demo`:

```rust
/// Number of alternative routes to find (0 = single shortest path)
#[arg(long, default_value_t = 0)]
alternatives: u32,

/// Stretch factor for alternative routes (candidates up to this factor x optimal distance)
#[arg(long, default_value_t = hanoi_core::multi_route::DEFAULT_STRETCH)]
stretch: f64,
```

#### Port step 2: add the helper block before `main()`

Insert the following block directly **after** `init_tracing(...)` and **before**
`fn main()`:

- `ROUTE_COLORS`
- `format_multi_result(...)`
- `run_multi_query(...)`

The easiest way is to copy the helper block from `kientx`, then make one
important `dev-haihm` fix:

- in the normal-graph branch of `run_multi_query(...)`, use
  `let mut engine = QueryEngine::new(&context);`
  because on the current branch `QueryEngine::multi_query(...)` should be
  `&mut self`

If you want the exact source block to paste, use:

```bash
git show kientx:CCH-Hanoi/crates/hanoi-cli/src/main.rs | sed -n '240,420p'
```

#### Port step 3: branch in `main()`

Inside the `Command::Query { ... } => { ... }` match arm:

1. add `alternatives` and `stretch` to the destructuring pattern
2. wrap the existing single-route logic with:

```rust
if alternatives > 0 {
    run_multi_query(
        &data_dir,
        line_graph,
        from_node,
        to_node,
        from_lat,
        from_lng,
        to_lat,
        to_lng,
        output_file,
        &output_format,
        demo,
        alternatives,
        stretch,
    );
} else {
    // existing single-route body
}
```

That is the same structural move used in `kientx`.

#### Optional `dev-haihm` CLI polish

If you want CLI-export parity with the current server response contract, extend
`format_multi_result(...)` to also emit:

- `path_nodes`
- `route_arc_ids`
- `weight_path_ids`

This is not required for the algorithm to work, but it keeps CLI exports closer
to the richer `dev-haihm` response format.

Build check:

```bash
cargo build --manifest-path CCH-Hanoi/Cargo.toml -p hanoi-cli
```

---

## 7. Optional docs and UI follow-up

### 7.1 Copy the algorithm walkthrough

This file is a good reference and can be brought over directly:

```bash
git show kientx:docs/walkthrough/multi_route_algorithm_analysis.md > docs/walkthrough/multi_route_algorithm_analysis.md
```

### 7.2 Treat UI work as a separate task

`kientx` includes standalone helper scripts such as `scripts/multi_query_ui.html`,
but the current `dev-haihm` bundled server UI does **not** yet expose
`alternatives` / `stretch` controls.

That means:

- backend merge can land first,
- CLI can land second,
- bundled UI integration should be a deliberate follow-up, not part of the
  backend merge unless you want to design the UX now.

---

## 8. What not to copy from `kientx`

Do not transplant these files during the multi-route merge:

- `CCH-Hanoi/crates/hanoi-server/src/main.rs`
  `dev-haihm` already has the richer query/reset/UI/traffic/camera/evaluate
  router setup.
- `CCH-Hanoi/crates/hanoi-server/Cargo.toml`
  the `kientx` version removes dependencies used by `dev-haihm`.
- `CCH-Hanoi/crates/hanoi-gateway/*`
  unrelated to the core K-alternative merge.
- `CCH-Hanoi/crates/hanoi-core/src/geometry.rs`
  `dev-haihm` has the newer turn refinement pipeline.
- `CCH-Hanoi/crates/hanoi-tools/src/bin/diagnose_turn.rs`
  useful, but independent.
- logs and artifacts:
  `bench_results.json`, `bench_core_*.log`, `bench_server_*.log`,
  `multi_route_result.geojson`, `query_*.geojson`
- unrelated helper scripts unless you explicitly want them.

Also do **not** import any older `kientx` turn-cost or turn-generation logic
over the current `dev-haihm` line-graph behavior.

---

## 9. End-to-end verification

### 9.1 Format and build

```bash
cargo fmt --manifest-path CCH-Hanoi/Cargo.toml --all
cargo build --release --manifest-path CCH-Hanoi/Cargo.toml --workspace
```

### 9.2 Test

```bash
cargo test --manifest-path CCH-Hanoi/Cargo.toml --workspace
```

### 9.3 CLI check

Single-route sanity check:

```bash
cargo run --release --manifest-path CCH-Hanoi/Cargo.toml -p hanoi-cli -- \
  query \
  --data-dir <dataset-root> \
  --line-graph \
  --from-lat 21.0300 \
  --from-lng 105.8400 \
  --to-lat 21.0000 \
  --to-lng 105.8200
```

Multi-route check:

```bash
cargo run --release --manifest-path CCH-Hanoi/Cargo.toml -p hanoi-cli -- \
  query \
  --data-dir <dataset-root> \
  --line-graph \
  --from-lat 21.0300 \
  --from-lng 105.8400 \
  --to-lat 21.0000 \
  --to-lng 105.8200 \
  --alternatives 3 \
  --stretch 1.30 \
  --output-format geojson
```

Expected result:

- a GeoJSON feature collection with up to 3 features,
- route 0 is the shortest path,
- each feature has distinct styling if demo coloring is enabled.

### 9.4 Server check

Normal graph:

```bash
cargo run --release --manifest-path CCH-Hanoi/Cargo.toml -p hanoi-server -- \
  --graph-dir <dataset-root>/graph \
  --query-port 8080 \
  --customize-port 9080
```

Line graph:

```bash
cargo run --release --manifest-path CCH-Hanoi/Cargo.toml -p hanoi-server -- \
  --graph-dir <dataset-root>/line_graph \
  --original-graph-dir <dataset-root>/graph \
  --line-graph \
  --serve-ui \
  --query-port 8080 \
  --customize-port 9080
```

Multi-route API request:

```bash
curl -sS -X POST \
  'http://localhost:8080/query?alternatives=3&stretch=1.30&colors' \
  -H 'content-type: application/json' \
  -d '{
    "from_lat": 21.0300,
    "from_lng": 105.8400,
    "to_lat": 21.0000,
    "to_lng": 105.8200
  }'
```

Expected result:

- HTTP 200,
- GeoJSON `FeatureCollection`,
- up to `alternatives` features,
- each feature contains route metadata used by the current branch export flow.

---

## 10. Merge back

Once the port builds and tests cleanly:

```bash
git checkout dev-haihm
git merge feat/kientx-multi-route
```

If you prefer a clean history, squash the feature branch first. Do not squash
until after you have completed the verification steps above.

---

## 11. Suggested execution order

If you want the least risky sequence, do it in this order:

1. `multi_route.rs`
2. `lib.rs`
3. `cch.rs`
4. `line_graph.rs`
5. server `types.rs`, `state.rs`, `handlers.rs`, `engine.rs`
6. build and test backend
7. optional CLI port
8. optional docs copy
9. optional UI follow-up

That keeps the hard merge surface small and lets you verify each layer before
adding the next one.
