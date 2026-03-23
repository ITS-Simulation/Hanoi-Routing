# Audit Findings — 2026-03-18

Comprehensive audit of the entire CCH-Hanoi workspace (6 crates, 33 source
files), cross-referenced against upstream `rust_road_router` API signatures, all
documentation in `docs/`, the pipeline script, and actual on-disk data in
`Maps/data/`.

## Scope

| Crate | Files audited | Status |
|-------|---------------|--------|
| hanoi-core | `lib.rs`, `graph.rs`, `cch.rs`, `line_graph.rs`, `spatial.rs` | Clean |
| hanoi-server | `main.rs`, `engine.rs`, `handlers.rs`, `state.rs`, `types.rs` | 2 issues |
| hanoi-gateway | `main.rs`, `proxy.rs`, `types.rs` | 2 issues |
| hanoi-cli | `main.rs` | 1 issue |
| hanoi-tools | `generate_line_graph.rs` | Clean |
| hanoi-bench | `lib.rs`, `core.rs`, `dataset.rs`, `report.rs`, `server.rs`, `spatial.rs`, 3 binaries, 2 criterion benches | 1 issue |

---

## Confirmed Issues

### 1. Gateway silently swallows backend HTTP error status codes

**Severity**: Medium — breaks error propagation for any backend 4xx/5xx.

**Location**: [hanoi-gateway/src/proxy.rs:54-67](CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs#L54-L67)

**Problem**: The `handle_query` function calls `resp.json().await` on the
backend response without checking `resp.status()`. If the backend returns
HTTP 400 (e.g., from a future coordinate validation rejection), the gateway
deserializes the error body as a success and returns it with 200 OK. The
same issue affects `handle_info`.

**Impact**: API consumers behind the gateway cannot distinguish between
successful routing results and backend errors. The planned Coordinate
Boundary Validation feature (documented in `docs/planned/Coordinate Boundary
Validation.md`, Phase 4.6) explicitly identifies this as a required fix.

**Fix**: Check `resp.status()` before deserializing; propagate the backend's
status code to the gateway's response.

---

### 2. Gateway `/info` defaults to `"normal"` without fallback indication

**Severity**: Low — silent default may confuse operators.

**Location**: [hanoi-gateway/src/proxy.rs:75](CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs#L75)

**Problem**: `GET /info` without a `graph_type` query parameter silently
defaults to `"normal"`. If the `"normal"` backend is down but the
`"line_graph"` backend is up, the request fails without indicating that
omitting `graph_type` caused the failure.

**Impact**: Operational confusion during partial outages. Not a correctness
bug, but an API ergonomics issue.

---

### 3. CLI `Query` subcommand lacks line-graph mode

**Severity**: Medium — CLI cannot query the line graph at all.

**Location**: [hanoi-cli/src/main.rs:36-64](CCH-Hanoi/crates/hanoi-cli/src/main.rs#L36-L64)

**Problem**: The CLI's `Query` subcommand only supports `CchContext` (normal
graph) and `QueryEngine`. It has no `--line-graph` flag, no
`--original-graph-dir` option, and no code path for `LineGraphCchContext` /
`LineGraphQueryEngine`. The server has full line-graph support, but the CLI
does not.

**Impact**: Operators cannot use the CLI to test line-graph queries
offline (without starting the server). The `Info` subcommand also only
loads `GraphData` without indicating whether the directory is a line graph.

---

### 4. Server customization channel can silently drop intermediate weight updates

**Severity**: Low — by-design behavior, but worth documenting.

**Location**: [hanoi-server/src/engine.rs:29-37](CCH-Hanoi/crates/hanoi-server/src/engine.rs#L29-L37)

**Problem**: The `watch::channel` semantics mean that if multiple
`/customize` requests arrive while a prior customization is in progress, only
the **latest** weight vector is applied. Intermediate vectors are silently
dropped. The `watch::Receiver::borrow_and_update()` call (line 30) always
sees the latest value.

**Impact**: This is actually correct behavior for live traffic updates (you
always want the freshest data), but it's not documented anywhere. If a
caller expects every `/customize` call to be applied sequentially, they will
be surprised.

**Status**: NOT A BUG — intentional watch channel semantics. Document in
server README.

---

### 5. Server `/customize` response returns 200 before customization completes

**Severity**: Low — potential race condition for test automation.

**Location**: [hanoi-server/src/handlers.rs:48-53](CCH-Hanoi/crates/hanoi-server/src/handlers.rs#L48-L53)

**Problem**: `handle_customize` sends the weights to the watch channel and
immediately returns `{"accepted": true, "message": "customization queued"}`.
The actual customization happens asynchronously in the background engine
thread. There is no way for the caller to know when the customization is
complete — the `/info` endpoint reports `customization_active` but this is a
point-in-time snapshot, not a completion signal.

**Impact**: Benchmarks and integration tests that call `/customize` followed
immediately by queries may get results from the old weights. The
`bench_query_after_customize` function in `hanoi-bench/src/server.rs:233`
mitigates this with a 100ms sleep, but this is a heuristic.

---

### 6. `hanoi-bench` `percentile_sorted` is duplicated

**Severity**: Very Low — code hygiene.

**Location**: [hanoi-bench/src/report.rs:279-285](CCH-Hanoi/crates/hanoi-bench/src/report.rs#L279-L285)
and [hanoi-bench/src/server.rs:240-246](CCH-Hanoi/crates/hanoi-bench/src/server.rs#L240-L246)

**Problem**: The `percentile_sorted` helper function is identically defined
in both `report.rs` and `server.rs`. This is a minor DRY violation.

**Fix**: Move to `lib.rs` as a crate-level utility.

---

## Verified Correct (Non-Issues)

### Upstream API Compatibility

All upstream `rust_road_router` API usages were verified against the actual
source code:

| API | Usage in CCH-Hanoi | Verified |
|-----|-------------------|----------|
| `CCH::fix_order_and_build(&graph, order)` | `cch.rs:62`, `line_graph.rs:87` | Signature matches |
| `CCH::to_directed_cch()` | `line_graph.rs:88` | Returns `DirectedCCH` |
| `customize(&cch, &metric)` | `cch.rs:77` | `CustomizedBasic<'_, CCH>` |
| `customize_directed(&directed_cch, &metric)` | `line_graph.rs:112` | `CustomizedBasic<'_, DirectedCCH>` |
| `CchQueryServer::new(customized)` | `cch.rs:105`, `line_graph.rs:143` | Accepts any `Customized` impl |
| `Server::update(new_customized)` | `cch.rs:199`, `line_graph.rs:267` | Takes owned `C`, swaps via `mem::swap` |
| `NodeOrder::from_node_order(perm)` | `cch.rs:57`, `line_graph.rs:61` | Takes `Vec<NodeId>` (owned) |
| `FirstOutGraph::new(slices)` | `cch.rs:72-76`, etc. | Generic over `AsRef<[T]>` — slices OK |
| `Query { from, to }` | `cch.rs:120`, `line_graph.rs:161-163` | Fields are `NodeId` |
| `line_graph(graph, callback)` | `generate_line_graph.rs:208` | `FnMut(EdgeId, EdgeId) -> Option<Weight>` → `OwnedGraph` |
| `Vec::load_from(path)` | All loading code | `Load` trait, returns `io::Result<Vec<T>>` |

### Graph Data Loading

`GraphData::load()` at [hanoi-core/src/graph.rs:20-43](CCH-Hanoi/crates/hanoi-core/src/graph.rs#L20-L43):
- CSR invariants are validated (sentinel check, array length consistency)
- `assert_eq!` is used — panics on invalid data rather than returning errors.
  This is acceptable for startup-time validation but could be softened to
  `Result` returns in the future.

### Line Graph Path Mapping

`LineGraphQueryEngine::query()` at [hanoi-core/src/line_graph.rs:160-208](CCH-Hanoi/crates/hanoi-core/src/line_graph.rs#L160-L208):
- Correctly maps line-graph node path → original tail nodes + final head
- Final-edge correction via `saturating_add` prevents overflow
- Coordinate mapping uses `original_latitude`/`original_longitude` (not line
  graph coordinates)
- Distance recomputation via `route_distance_m` is done on the intersection
  coordinates

### Line Graph Spatial Snapping

`LineGraphQueryEngine::query_coords()` at [hanoi-core/src/line_graph.rs:219-252](CCH-Hanoi/crates/hanoi-core/src/line_graph.rs#L219-L252):
- Uses `nearest_node()` (returns tail/head based on `t`) — NOT `edge_id`
- This was a previous bug (documented in `docs/walkthrough/Line Graph Spatial
  Indexing and Snapping.md`) and is now correctly fixed
- Fallback uses `collect_candidate_nodes_prioritized` which correctly collects
  line-graph node IDs (not CSR edge indices)

### Haversine Perpendicular Distance

[hanoi-core/src/spatial.rs:143-175](CCH-Hanoi/crates/hanoi-core/src/spatial.rs#L143-L175):
- Equirectangular projection around segment midpoint — accurate for short
  segments (road network edges are typically < 1km)
- Degenerate edge guard (`len_sq < 1e-20`) prevents division by zero
- Projection parameter `t` correctly clamped to `[0, 1]`
- Final distance computed via Haversine on geographic coordinates (not the
  projected plane)

### Turn Restriction Merge-Scan in generate_line_graph

[hanoi-tools/src/bin/generate_line_graph.rs:207-223](CCH-Hanoi/crates/hanoi-tools/src/bin/generate_line_graph.rs#L207-L223):
- Peekable iterator correctly advances past all lexicographically smaller
  pairs before checking for a match
- U-turn detection correctly checks `tail[edge1] == graph.head()[edge2]`
- This was verified correct in the 2026-03-12 audit and remains unchanged

### Pipeline CCH Perm Paths

The pipeline (`scripts/pipeline:99-104`) writes:
- Normal graph: `flow_cutter_cch_order.sh $GRAPH_DIR` → `$GRAPH_DIR/perms/cch_perm`
- Line graph: `flow_cutter_cch_order.sh $OUTPUT_DIR/line_graph` → `$OUTPUT_DIR/line_graph/perms/cch_perm`

The server (`hanoi-server/src/main.rs:182,195`) reads:
- Normal: `args.graph_dir.join("perms/cch_perm")`
- Line graph: `args.graph_dir.join("perms/cch_perm")` (where `--graph-dir` = line_graph directory)

**Path consistency verified.** Both produce and consume `<graph_dir>/perms/cch_perm`.

The line graph uses a standard node permutation generated directly on the line
graph (via `flow_cutter_cch_order.sh`), rather than the arc-based cut ordering
(`flow_cutter_cch_cut_order.sh` on the original graph) recommended by the CCH
Walkthrough. Both approaches produce valid nested dissection orderings. The
standard node ordering directly on the line graph is correct and produces the
right number of entries (verified: 1,869,499 entries matching 1,869,499 line
graph nodes = 1,869,499 original edges). The arc-based cut ordering may produce
a more optimal ordering in terms of separator quality, but the current approach
is not incorrect.

### Server Dual-Port Architecture

[hanoi-server/src/main.rs:220-248](CCH-Hanoi/crates/hanoi-server/src/main.rs#L220-L248):
- Query port (8080) and customize port (9080) run on separate listeners
- Customize port has a 64MB body limit and gzip decompression — appropriate for
  binary weight vectors
- The customize listener is spawned as a `tokio::spawn` task while the query
  listener runs on the main task — both remain active

### Engine Background Thread

[hanoi-server/src/engine.rs:18-91](CCH-Hanoi/crates/hanoi-server/src/engine.rs#L18-L91):
- Uses `std::thread::spawn` (not `tokio::spawn`) — correct because CCH
  operations are CPU-bound and should not block the async runtime
- 50ms timeout on `query_rx.recv()` ensures customization checks happen
  regularly even when no queries are incoming
- `customization_active` AtomicBool with `Ordering::Relaxed` is appropriate
  for a status flag (no ordering guarantees needed between threads)

### GeoJSON Output

[hanoi-server/src/engine.rs:174-206](CCH-Hanoi/crates/hanoi-server/src/engine.rs#L174-L206):
- Correctly reverses (lat, lng) → [lng, lat] per RFC 7946 GeoJSON spec
- Null geometry for no-path results is valid GeoJSON

---

## Design Observations (Not Bugs)

### A. No coordinate validation (planned)

The `docs/planned/Coordinate Boundary Validation.md` plan is comprehensive and
well-designed. It addresses all entry points (core, server, CLI, gateway, bench)
with a centralized validation in `hanoi-core`. Implementation would fix Issue #1
(gateway error propagation) as a side effect.

### B. No graceful shutdown for the server

The server has no signal handler for SIGTERM/SIGINT. The `_guard` for
`tracing-appender` is held on the stack, so it will flush on panic but not on
a signal kill (`kill -9`). For production use, a `tokio::signal::ctrl_c()`
handler with a shutdown channel would be appropriate.

### C. No health check endpoint

The server exposes `/query`, `/customize`, and `/info`, but no `/health` or
`/ready` endpoint. The `/info` endpoint serves as a de facto health check, but
it doesn't verify that the engine thread is alive (it only checks
`customization_active`).

### D. No request timeout on gateway proxy

The gateway's `reqwest::Client` uses default timeouts (no explicit
`timeout(Duration)` configuration). If a backend hangs, the gateway will hang
indefinitely.

### E. `GraphData::load` uses panicking asserts

[hanoi-core/src/graph.rs:33-39](CCH-Hanoi/crates/hanoi-core/src/graph.rs#L33-L39) uses
`assert_eq!` for CSR invariant checks. These will panic on invalid data rather
than returning a recoverable error. For a library crate, returning `Err` would
be more idiomatic and allow callers to handle failures gracefully.

### F. Line graph ordering strategy

The pipeline uses `flow_cutter_cch_order.sh` directly on the line graph,
producing a standard node permutation. The CCH Walkthrough recommends the
arc-based `flow_cutter_cch_cut_order.sh` on the original graph instead. Both
are valid, but the arc-based approach may produce better separator quality for
turn-expanded graphs. This is a potential optimization opportunity, not a
correctness issue.

---

## Summary

| Category | Count |
|----------|-------|
| Confirmed issues (to fix) | 3 (gateway error propagation, CLI missing line-graph, bench duplicate) |
| By-design behaviors to document | 2 (watch channel drop, async customize) |
| Verified correct | 11 API usages, 6 algorithmic checks, 2 path consistency checks |
| Design observations | 6 (future improvements, not blockers) |
