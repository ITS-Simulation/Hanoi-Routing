# CHANGELOGS.md

## 2026-04-15 — Cập nhật tài liệu hướng dẫn theo source hiện tại

- **`docs/Hướng dẫn sử dụng.md`**: Thêm mục 2 — Giao diện Route Viewer (UI):
  khởi động `--serve-ui`, luồng query (single/multi), so sánh route GeoJSON,
  đo khoảng cách, traffic/camera overlay, bản đồ nền, thông tin server & reset.
  Renumber toàn bộ mục lục và sub-sections.
- **`docs/Hướng dẫn đọc tài liệu và source.md`**: Cập nhật bảng module
  hanoi-core (thêm bounds, spatial, multi_route, cch_cache, via_way_restriction),
  bảng file hanoi-server (thêm handlers, engine, state, types, camera_overlay,
  traffic, route_eval, ui), thêm diagnose_turn vào hanoi-tools, cập nhật mô tả
  CCH_Data_Pipeline.
- **`docs/Hướng dẫn sử dụng.md`**: Thêm endpoints mới (evaluate_routes,
  reset_weights, traffic_overlay, camera_overlay, UI routes), thêm query params
  alternatives/stretch, thêm trường graph_type vào response, thêm binary
  diagnose_turn, sửa default ports (8080/9080), thêm mục 10 công cụ chẩn đoán.
- **`docs/Hướng dẫn cài đặt triển khai.md`**: Thêm diagnose_turn vào binary
  table, thêm --camera-config và --serve-ui vào server params.

## 2026-04-10 — implement proximity-first snapping and road-conforming connectors

- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`** (updated): Replaced
  composite snap-score ranking with the planned three-tier proximity-first
  selection flow for `query_coords()` and `multi_query_coords()`, moved
  projected snap points into the patched route coordinate chain, and
  recompute turn distances after prepending source-side connector geometry.
- **`CCH-Hanoi/crates/hanoi-core/src/cch.rs`** (updated): Applied the same
  tiered snap-pair selection and projected-point-in-coordinates patching to
  the normal-graph engine for parity, while keeping the existing public
  query signatures intact.
- **`CCH-Hanoi/crates/hanoi-core/src/geometry.rs`** (updated): Exposed
  `annotate_distances()` as `pub(crate)` so line-graph coordinate patching can
  recompute serialized `distance_to_next_m` values after coordinate-index
  shifts.
- **`CCH-Hanoi/crates/hanoi-core/src/spatial.rs`** (updated): Removed the
  obsolete composite snap-scoring helpers/constants and their test coverage,
  leaving snapping focused on candidate generation and connector geometry.
- **`CCH-Hanoi/crates/hanoi-server/src/engine.rs`** (updated): Simplified
  `connect_query_coordinates()` so responses only wrap the already-patched
  route polyline with user origin/destination points; projected snap points are
  no longer inserted separately.
- **`CCH-Hanoi/crates/hanoi-core/src/cch.rs`**,
  **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`**, and
  **`CCH-Hanoi/crates/hanoi-server/src/engine.rs`** (updated): Added
  regression tests covering tiered selection behavior, projected endpoint
  embedding/dedup, turn-distance recomputation after coordinate shifts, and
  the simplified coordinate wrapper.
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`** (updated): Replaced the
  earlier connector-drop-only Part C amendment with source/destination
  backtrack clipping near projected snap points on two-way roads, rebased turn
  indices after clipped geometry removal, and added regressions for clipping on
  both ends plus turn remapping through the clipped coordinate chain.
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`** (updated): Cleaned up the
  line-graph coordinate patching warnings by dropping an unused destination
  append count and compiling the reverse-end clipping helper only for its test
  coverage.
- **`CCH-Hanoi/crates/hanoi-server/static/index.html`**,
  **`CCH-Hanoi/crates/hanoi-server/static/app.js`**, and
  **`CCH-Hanoi/crates/hanoi-server/static/styles.css`** (updated): Added a
  frontend distance-measure tool with its own map interaction mode, live
  sidebar stats, measured-path overlays, and matching legend/map guidance so
  users can click out ad hoc distances without interfering with route queries.
- **`CCH-Hanoi/crates/hanoi-server/static/index.html`**,
  **`CCH-Hanoi/crates/hanoi-server/static/app.js`**, and
  **`CCH-Hanoi/crates/hanoi-server/static/styles.css`** (updated): Added a
  basemap flavor selector in the UI with a simpler light default plus balanced
  and classic OSM options, so the map can stay readable while still offering
  denser street context on demand.

## 2026-04-09 — fix exact snap-edge entry and exit costs for coordinate routing

- **`CCH-Hanoi/crates/hanoi-core/src/spatial.rs`** (updated): Added reusable
  helpers for snap-position-aware partial-edge costs and snap-to-node /
  snap-to-snap connector geometry so coordinate queries can use the exact
  projected position on each edge instead of falling back to endpoint
  heuristics.
- **`CCH-Hanoi/crates/hanoi-core/src/cch.rs`** (updated): Normal-graph
  coordinate queries now route from `src.head` to `dst.tail`, special-case
  forward travel on the same snapped edge, and add exact source/destination
  partial-edge costs before snap-pair ranking and response assembly.
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`** (updated): Line-graph
  coordinate queries now preserve the existing trimmed-path flow while
  replacing whole-edge endpoint costing with exact snap-edge suffix/prefix
  costs, including a non-trivial same-edge fallback when the destination lies
  behind the source on the same directed arc.
- **`CCH-Hanoi/crates/hanoi-core/src/cch.rs`** and
  **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`** (updated): Coordinate
  patching now uses cached snap projections directly and recomputes
  `distance_m` with snapped endpoints included, so the numeric route distance
  matches the visible geometry.
- **`CCH-Hanoi/crates/hanoi-cli/src/main.rs`** (updated): Adjusted the
  line-graph multi-query CLI path for the new mutable coordinate-query API.

## 2026-04-09 — finish snap-aware routing and shape-point geometry

- **`CCH-Generator/src/generate_graph.cpp`** (updated): Switched
  RoutingKit road-geometry extraction from `OSMRoadGeometry::none` to
  `OSMRoadGeometry::uncompressed` and now persist
  `first_modelling_node` / `modelling_node_latitude` /
  `modelling_node_longitude` alongside the existing graph vectors.
- **`CCH-Hanoi/crates/hanoi-core/src/graph.rs`** (updated): Added optional
  shape-point loading and validation to `GraphData`, keeping existing graphs
  backward-compatible while rejecting partial shape-point datasets.
- **`CCH-Hanoi/crates/hanoi-core/src/spatial.rs`** (updated): Reworked edge
  snapping to project onto the full arc polyline instead of the tail-head
  chord, cached the true projected point on `SnapResult`, added reusable route
  geometry expansion / snap-connector helpers, introduced composite
  snap-distance scoring (`distance_ms + snap penalties`), and added regression
  tests for polyline snapping, geometry expansion, same-edge connectors, and
  snap scoring.
- **`CCH-Hanoi/crates/hanoi-core/src/cch.rs`** (updated): Normal-graph
  queries now rank coordinate snap pairs by composite score instead of
  first-success / pure travel time, expand route coordinates through shape
  points, and splice snap-edge connector geometry into coordinate-query
  answers without changing the response schema.
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`** (updated): Line-graph
  coordinate queries now use the same composite snap ranking, expanded shape
  geometry, trimmed-edge connector handling, and remapped
  `TurnAnnotation.coordinate_index` values so maneuvers still point at the
  correct coordinate after modelling nodes are inserted.

## 2026-04-09 — implement snap projection and obvious maneuver promotion

- **`CCH-Hanoi/crates/hanoi-core/src/spatial.rs`** (updated): Added
  `SnapResult::projected_point()` so coordinate queries can recover the actual
  closest point on the snapped edge instead of only the nearest endpoint.
- **`CCH-Hanoi/crates/hanoi-core/src/cch.rs`** (updated): Extended
  `QueryAnswer` with projected snap metadata and threaded projected origin /
  destination points through normal-graph `query_coords()` and
  `multi_query_coords()` without changing the existing snap-candidate selection
  flow.
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`** (updated): Mirrored the
  projected snap metadata flow for line-graph coordinate queries so trimmed and
  multi-route answers carry the projected source / destination edge points.
- **`CCH-Hanoi/crates/hanoi-core/src/geometry.rs`** (updated): Added the
  `promote_obvious_maneuvers` pass at the end of `compute_turns()` so straight
  choices at degree-3+ forks are promoted to slight maneuvers when there is a
  competing straight-ish alternative, and added regression tests covering both
  promotion and non-promotion cases.
- **`CCH-Hanoi/crates/hanoi-server/src/engine.rs`** (updated): Rebuilt route
  polyline assembly for JSON and GeoJSON responses as
  `user -> projected snap -> graph path -> projected snap -> user`, with 1 m
  dedup against the first/last graph node to avoid double points when the snap
  already lands near an intersection, and added regression tests for the helper.

## 2026-04-09 — fix roundabout straight aggregation and compound U-turn detection

- **`CCH-Hanoi/crates/hanoi-core/src/geometry.rs`** (updated):
  - `merge_straights` now also merges consecutive `RoundaboutStraight` entries,
    collapsing noisy intermediate ring segments into a single instruction.
  - New `collapse_compound_uturns` pass: detects two consecutive same-direction
    turns (both >= 40°, sum >= 155°, <= 50m apart) and replaces them with a
    single `UTurn`. Fixes misidentification of median-break U-turns as two
    separate left turns.
  - Pipeline order: collapse_degree2 → merge_straights → annotate_distances →
    collapse_compound_uturns → strip_straights.

## 2026-04-09 — fix snap gap between user origin/destination and route polyline

- **`CCH-Hanoi/crates/hanoi-server/src/engine.rs`** (updated): Prepend user
  origin and append user destination to the LineString coordinate path in all
  three response builders (single GeoJSON, multi-route GeoJSON, plain JSON).
  Eliminates the visual gap between the marker and the route line on map UIs.

## 2026-04-09 — add CCH-Hanoi maintenance guide

- **`CCH-Hanoi/README_MAINTENANCE.md`** (new): Maintenance guide covering
  architecture, crate map, file-level risk classification, CCH lifecycle,
  HTTP API reference, common tasks, build commands, and troubleshooting.
- **`CCH-Hanoi/README_MAINTENANCE_VI.md`** (new): Vietnamese translation of
  the maintenance guide.

## 2026-04-09 — implement RAM-optimized CCH cache architecture

- **`rust_road_router/engine/src/util.rs`** (updated): Added `Vecs`
  serialization helpers and validated reconstruction so cached edge-mapping
  data can be round-tripped with explicit CSR checks instead of raw-layout
  assumptions, then extended the helper layer with shared `Storage<T>` backed
  by either owned `Arc<Vec<T>>` or read-only `memmap2::Mmap`.
- **`rust_road_router/engine/src/datastr/graph/first_out_graph.rs`** (updated):
  Added read-only accessors and `ReversedGraphWithEdgeIds::from_raw_validated()`
  so inverted directed-CCH graphs can be reconstructed from cache with CSR
  validation, and later switched those topology arrays to `Storage<T>` so warm
  cache loads can stay mmap-backed instead of materializing fresh `Vec`s.
- **`rust_road_router/engine/src/algo/customizable_contraction_hierarchy/mod.rs`**
  (updated): Implemented `DirectedCCH` deconstruction plus
  `DirectedCCHReconstructor` with structural validation for topology, mappings,
  node order, inverted graphs, and elimination tree data, then moved immutable
  `DirectedCCH` topology/mapping storage onto `Storage<T>` so cache hits reuse
  mmap-backed data directly.
- **`rust_road_router/engine/src/datastr/node_order.rs`** and
  **`rust_road_router/engine/src/datastr/graph.rs`** (updated): Switched
  `NodeOrder` to shared storage and added `#[repr(transparent)]` wrappers for
  graph/newtype serialization boundaries used by the cache and mmap path.
- **`CCH-Hanoi/crates/hanoi-core/src/cch_cache.rs`** (new): Added the
  cache-meta/checksum layer for CCH and DirectedCCH caches, including ABI/header
  validation and reconstructor dispatch.
- **`CCH-Hanoi/crates/hanoi-core/src/cch.rs`** and
  **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`** (updated): Switched both
  `load_and_build()` paths to cache-or-build flow, added DirectedCCH post-load
  edge-mapping validation, preserved rebuild fallback on cache miss or
  validation error, and then converted baseline/original-graph storage in both
  normal and line-graph contexts to shared `Storage<T>` so runtime weight and
  topology arrays are reused instead of cloned.
- **`CCH-Hanoi/crates/hanoi-core/src/graph.rs`** and
  **`CCH-Hanoi/crates/hanoi-core/src/geometry.rs`** (updated): Reworked
  `GraphData::load()` to mmap RoutingKit vectors with explicit structural
  validation, and adjusted turn computation to resolve line-graph nodes through
  `original_arc_id_of_lg_node` so original-graph arrays no longer need
  line-graph-length duplication.
- **`CCH-Hanoi/crates/hanoi-server/src/route_eval.rs`**,
  **`CCH-Hanoi/crates/hanoi-server/src/state.rs`**, and
  **`CCH-Hanoi/crates/hanoi-server/src/handlers.rs`** (updated): Propagated the
  shared-storage model into server state and exact-route evaluation so baseline
  weights and original-graph arrays are reused without extra heap copies on
  startup.
- **`CCH-Hanoi/crates/hanoi-core/src/lib.rs`** and
  **`CCH-Hanoi/crates/hanoi-core/Cargo.toml`** (updated): Wired in the new
  internal cache module and added `chrono` for RFC3339 cache metadata
  timestamps plus `memmap2` for the shared read-only graph/CCH storage path.
- **`rust_road_router/engine/Cargo.toml`** and
  **`CCH-Hanoi/crates/hanoi-server/Cargo.toml`** (updated): Added the runtime
  dependency support needed for mmap-backed storage through the engine and
  server stack.

## 2026-04-08 — Concurrency plan: replace tokio::mpsc + TokioMutex with flume MPMC

- **`docs/planned/Concurrency-10K-CCU.md`** (amended): Replaced
  `tokio::mpsc` + `Arc<TokioMutex<Receiver>>` worker-pool design with
  `flume::bounded` MPMC channel. Receiver is Clone — each worker gets its own
  handle, eliminating the Mutex synchronization layer. Updated R1 (pool
  architecture, code snippets, audit), R2 (channel creation), R4 (worker loop
  `recv_async`, match arms for `RecvError`), R6a (`flume::TrySendError`), R6c
  (health endpoint uses `capacity()` + `len()`), risk table, and files-changed
  table. New dependency: `flume` in `hanoi-server/Cargo.toml`.

## 2026-04-08 — plan amendment round 6: 3 residual issues in RAM-optimized CCH plan

- **`docs/planned/RAM-Optimized-CCH-Architecture.md`** (amended): Fixed 3 residual
  issues: (1) validate_edge_mappings needs CCHT + EdgeIdT imports not currently in
  line_graph.rs — added import note with exact use statements; (2) Step 5
  original_tail / original_arc_id_of_lg_node decision was deferred — resolved as
  Option 2 (recompute at startup, ~15-25 MB heap, O(n) fast); (3) normal-graph
  CCHReconstrctor hardening gap acknowledged as intentional weaker path —
  no change, already documented.

## 2026-04-08 — plan amendment round 5: 2 issues + nit in RAM-optimized CCH plan

- **`docs/planned/RAM-Optimized-CCH-Architecture.md`** (amended): Fixed 2 issues
  + 1 nit: (1) Step 4 load path aborted on corrupt cache instead of rebuilding —
  replaced `cache.load()?` with match/fallback that catches io::Error and falls
  through to build-from-scratch; (2) edge mapping data values (fw/bw
  edge_to_orig_data) not range-checked against source graph — added post-load
  `validate_edge_mappings()` in hanoi-core that checks all EdgeIdT values <
  num_metric_edges before customization can use them; also added design rationale
  for two-layer validation (structural in rust_road_router, semantic in
  hanoi-core). Nit: P9 snippet updated to pass source_files to cache.save().
  Risk table expanded with 2 new entries.

## 2026-04-08 — plan amendment round 4: 4 issues + nits in RAM-optimized CCH plan

- **`docs/planned/RAM-Optimized-CCH-Architecture.md`** (amended): Fixed 4 issues
  + minor nits: (1) NodeOrder serialization round-trip broken — save_each writes
  "ranks" but reconstructor loaded "cch_order"; unified on ranks-based path with
  from_ranks(); (2) first_out monotonicity missing — added windows(2) checks on
  all directed topology + inverted first_out arrays, also added to
  from_raw_validated; (3) elimination tree cycle → infinite loop — added
  pre-construction parent_rank > child_rank check to block cycles before
  SeparatorTree::new() can hang; (4) normal-graph CCHReconstrctor still uses
  assert! — documented as known gap, scoped strong guarantees to DirectedCCH only.
  Nits: Phase B reconstruct_with(Loader) → reconstruct_from, Step 4 example now
  passes source_files to is_valid/save. Risk table expanded with 5 new entries.

## 2026-04-08 — plan amendment round 3: 3 more issues in RAM-optimized CCH plan

- **`docs/planned/RAM-Optimized-CCH-Architecture.md`** (amended): Fixed 3 more
  issues: (1) Value-range checks missing — added per-element bounds validation for
  all head/tail (< num_nodes) and inverted edge_ids (< num_fw/bw_edges) arrays,
  since customization/directed.rs uses get_unchecked_mut with this assumption;
  (2) NodeOrder permutation not truly validated — replaced NodeOrder::reconstruct_from
  (backed by debug_assert!, silent in release) with manual bijection check
  (seen[] array, returns InvalidData on duplicate/out-of-range); (3) Step 5 scope
  table overstated mmap coverage — original_tail (reconstructed via CSR loop) and
  original_arc_id_of_lg_node (synthesized + split-extended) cannot be directly
  mmap'd; table corrected with two implementation options (cache flat file vs recompute).

## 2026-04-08 — plan amendment round 2: 5 more issues in RAM-optimized CCH plan

- **`docs/planned/RAM-Optimized-CCH-Architecture.md`** (amended): Fixed 5 more
  issues: (1) Cross-structure size validation — reconstructor now checks
  mapping/inverted/tree/order sizes against directed topology (prevents silent
  zip truncation in prepare_weights); (2) assert!→io::Error — all from_raw and
  reconstructor validation returns InvalidData, not panic (matches graph.rs
  pattern); (3) Loader::new API doesn't exist — fixed to reconstruct_from();
  (4) Step 5 mmap scope under-specified — added explicit table of all Vec
  owners that must be covered for 400–600 MB target; (5) Normal-graph caching
  clarified — reuses existing CCHReconstrctor, no new rust_road_router code.

## 2026-04-08 — plan amendment: 5 soundness issues in RAM-optimized CCH plan

- **`docs/planned/RAM-Optimized-CCH-Architecture.md`** (amended): Fixed 5
  confirmed issues: (1) InRangeOption/EdgeIdT lack #[repr(transparent)] — now
  serialize through inner u32, not raw bytes; (2) no cache schema/ABI marker —
  added cache_meta header with version, endianness, pointer_width; (3) DirectedCCH
  reconstruction impossible from outside crate — added ReconstructPrepared impl
  inside rust_road_router (approved exception); (4) from_raw constructors
  bypassed CSR validation — added structural asserts; (5) travel_time in
  checksum rationale was wrong (always_infinity uses prepare_zero_weights, not
  travel_time) — removed from checksum. Also: withdrew Vecs first_idx usize→u32
  in-memory conversion (serialize on-the-fly instead), corrected P8 access
  pattern claim, renumbered steps (5→4 steps + future mmap).

## 2026-04-08 — walkthrough: RAM-optimized CCH architecture plan

- **`docs/walkthrough/RAM-Optimized-CCH-Architecture.md`** (new): Full
  serialization + mmap architectural plan to reduce steady-state RAM from ~2.2 GB
  to ~400–600 MB. Covers data structure audit, two-phase startup (build+cache vs
  mmap-load), 6-step implementation order, 10 identified potential problems.
  All decisions confirmed: u32 for Vecs indices (P4), SHA-256 content hash (P6),
  explicit CCH drop for first-run cleanup (P9), profile-scoped cache dirs.

## 2026-04-08 �� documentation update: multi-route, Vietnamese translations

- **`CCH-Hanoi/README.md`** (updated): Added K-alternative routes to system
  overview, library API (§5.6), CLI usage (§8.1 --alternatives/--stretch),
  HTTP API (§11.1 ?alternatives=N&stretch=F with multi-route response example),
  testing guide (§13.6), performance table, and validation checklist.
  Renumbered subsequent sections accordingly.
- **`CCH-Hanoi/README_VI.md`** (new): Full Vietnamese translation of README.md
  with natural phrasing and complete diacritics.
- **`CCH-Hanoi/README_ALTERNATIVE.md`** (new): Copy of the updated
  K-Alternative Routes walkthrough for the CCH-Hanoi workspace.
- **`CCH-Hanoi/README_ALTERNATIVE_VI.md`** (new): Full Vietnamese translation
  of the alternative routes walkthrough.
- **`docs/walkthrough/K-Alternative Routes Implementation.md`** (updated):
  Reworked to match actual implementation — renamed MultiRouteServer →
  AlternativeServer, updated constants (BOUNDED_STRETCH_EPS 0.25→0.4,
  LOCAL_OPT_T_FRACTION 0.25→0.4, LOCAL_OPT_EPSILON 0→0.1), added Phase 4
  recursive decomposition, bounded stretch at deviation points, cost-based
  sharing, and three new design decision entries.

## 2026-04-07 — implement planned multi-route improvements

- **`rust_road_router/engine/src/algo/customizable_contraction_hierarchy/query/alternative.rs`** (new):
  Added the alternative-route query core from the reviewed plan, including
  per-edge-cost path unpacking, travel-time prefiltering, bounded-stretch and
  T-test checks, cost-based sharing, recursive subproblem stitching, and
  debug/trace rejection logging.
- **`rust_road_router/engine/src/algo/customizable_contraction_hierarchy/query.rs`**
  (updated): Exported the new additive `alternative` query module.
- **`rust_road_router/engine/Cargo.toml`** (updated): Added `tracing` for the
  new alternative-query instrumentation.
- **`CCH-Hanoi/crates/hanoi-core/src/multi_route.rs`** (updated): Reduced the
  Hanoi-side module to the planned constants-and-re-exports shim.
- **`CCH-Hanoi/crates/hanoi-core/src/cch.rs`** and
  **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`** (updated): Switched both
  engines to `AlternativeServer::alternatives()`, removed the old ambiguous
  `edge_cost` closure plumbing, and added info-level multi-route query
  instrumentation on normal and line-graph entry points.
- **Audit / fix note:** During the implementation audit, the recursive combine
  phase was corrected to sort stitched left/right candidates by total distance
  before admissibility checks so longer combinations cannot block shorter valid
  ones by iteration order.

## 2026-04-07 — multi-route improvements plan: 15 bug fixes (six rounds)

- **`docs/planned/multi-route-improvements.md`** (updated): Resolved 15 bugs
  across six review rounds. Round 1 (#1–5): P1 self-referential struct,
  Q4/P2 geo-stretch gap, Q1 backward scan off-by-one, Q3 stitch duplicate
  v_s, edge_cost parallel arc ambiguity. Round 2 (#6–9): P1 sibling borrow
  unworkable, retain() geo-stretch too late, AlternativeRoute.edge_costs
  undeclared, R1 stale header + signatures. Round 3 (#10–11): Q3 edge-cost
  stitch incorrectly mirrored node dedup onto edges, run_basic_selection
  undefined + accepted[0] unguarded. Round 4 (#12–13): Q1
  find_deviation_points missing len<2 guard + unused reference_costs param,
  R1 AlternativeRoute geo_distance_m omission clarified. Round 5 (#14): R1
  adapter contradictorily re-exported AlternativeRoute while claiming
  hanoi-core keeps its own version — resolved by dropping geo_distance_m
  from both types. Round 6 (#15): R1 adapter described as "thin wrapper"
  providing closures but was actually just constants — clarified as
  constants+re-exports shim with callers calling AlternativeServer directly.

## 2026-04-07 — retire standalone query UI scripts

- **`scripts/query_ui.html`** (removed): Dropped the old standalone
  single-route HTML tool now that the bundled `hanoi-server` frontend covers
  the supported query workflow.
- **`scripts/multi_query_ui.html`** (removed): Dropped the duplicate standalone
  multi-route UI after merging that functionality into the current
  `hanoi-server` frontend stack.
- **`scripts/serve_query_ui.py`** (removed): Removed the local proxy/dev server
  helper that only existed to support the retired standalone HTML UIs.

## 2026-04-07 — hanoi-server map-first layout refinement

- **`CCH-Hanoi/crates/hanoi-server/static/index.html`** (updated): Reworked
  the bundled viewer into a more compact query workspace with Build / Routes /
  Turns subviews, moved overlay controls into the sidebar, and added a
  collapsible main panel toggle so the map can take over when needed.
- **`CCH-Hanoi/crates/hanoi-server/static/app.js`** (updated): Added
  persistent sidebar collapse state and query-subview state, wired the new
  layout controls into the existing multi-route flow, and automatically switch
  successful queries into the Routes view while keeping route logic unchanged.
- **`CCH-Hanoi/crates/hanoi-server/static/styles.css`** (updated): Tightened
  the sidebar and overlay layout for less scrolling, added compact map-first
  responsive behavior, and constrained long route lists to scroll within their
  own cards instead of stretching the whole panel.

## 2026-04-07 — hanoi-server bundled multi-route query UI

- **`CCH-Hanoi/crates/hanoi-server/static/index.html`** (updated): Added a
  query-mode switch to the bundled viewer, alternatives/stretch controls, a
  route-stack section, and legend updates so the bundled UI can drive both
  single-route and multi-route queries without leaving the current frontend
  stack.
- **`CCH-Hanoi/crates/hanoi-server/static/app.js`** (updated): Merged the
  standalone multi-query behavior into the bundled viewer state flow, including
  multi-route request params, full FeatureCollection preservation, selectable
  query-route cards, route-aware map styling, and selected-route summary /
  maneuver rendering.
- **`CCH-Hanoi/crates/hanoi-server/static/styles.css`** (updated): Added
  styling for the new bundled multi-route controls, route-list cards, and
  query-route map treatment using the existing hanoi-server visual language
  rather than the older standalone script UI palette.

## 2026-04-06 — latest origin-kientx merge walkthrough

- **`docs/walkthrough/Merging latest origin-kientx updates into dev-haihm.md`**
  (new): Reworked the latest `kientx` merge guide into a shorter,
  step-focused walkthrough for the refactored `dev-haihm` branch. The doc keeps
  the `git ls-tree` / `git show` audit trail, narrows the required merge to
  `multi_route.rs`, `cch.rs`, and `line_graph.rs`, and shows the exact snippets
  and paste targets to migrate while explicitly skipping refactored
  `hanoi-server` and CLI files. Added a parity note for `cch.rs` so the guide
  now calls out the one remaining behavioral choice versus `kientx`:
  `reconstruct_arc_ids()` fallback vs skipping invalid candidates.

## 2026-04-06 — kientx cross-reference & merge guide

- **`docs/walkthrough/Alternative Route Quality Investigation.md`** (updated):
  Added §8b cross-referencing kientx branch multi-route code against the 3
  identified problems (all confirmed), structural diff tables for both branches,
  and portability assessment. Also added §9–§12 covering concurrency
  architecture, penalty-based k-paths flow, SeArCCH feasibility, and solution
  comparison.
- **`docs/walkthrough/Merging kientx Multi-Route into dev-haihm.md`** (new):
  Step-by-step merge guide grounded in `git ls-tree` / `git show` inspection of
  the `kientx` branch, including a required/adapt/skip file map, current-branch
  code-port instructions, and corrected build/API verification commands for
  `dev-haihm`.

## 2026-04-06 — kientx multi-route merge walkthrough expansion

- **`docs/walkthrough/Merging kientx Multi-Route into dev-haihm.md`** (new):
  Expanded the merge guide with exact insertion points in `cch.rs`,
  `line_graph.rs`, `types.rs`, `state.rs`, `handlers.rs`, `engine.rs`, and the
  optional CLI port, plus adapted code snippets showing what to paste and what
  to preserve from the current `dev-haihm` branch.

## 2026-04-03 — hanoi-server collapsible legend panel

- **`CCH-Hanoi/crates/hanoi-server/static/index.html`** (updated): Reworked the
  map legend card into a header/body layout with a dedicated collapse button.
- **`CCH-Hanoi/crates/hanoi-server/static/app.js`** (updated): Added a
  persistent collapsed-state toggle for the legend card so the UI remembers the
  user's preferred legend visibility across reloads.
- **`CCH-Hanoi/crates/hanoi-server/static/styles.css`** (updated): Added the
  header/body collapse styling for the legend overlay.

## 2026-04-03 — hanoi-server camera overlay path resolution

- **`CCH-Hanoi/crates/hanoi-server/src/main.rs`** (updated): The default
  `--camera-config CCH_Data_Pipeline/config/mvp_camera.yaml` path is now
  resolved relative to the repository root when needed, so the camera overlay
  still loads correctly if `hanoi_server` is launched from `CCH-Hanoi/` instead
  of the repo root.

## 2026-04-03 — hanoi-server camera distribution overlay

- **`CCH-Hanoi/crates/hanoi-server/src/camera_overlay.rs`** (new): Added a
  manifest-backed camera overlay model that loads configured camera locations
  from `CCH_Data_Pipeline/config/mvp_camera.yaml`, resolving `arc_id` entries to
  midpoint coordinates from `road_arc_manifest.arrow` and falling back
  gracefully when the manifest or YAML is unavailable.
- **`CCH-Hanoi/crates/hanoi-server/src/types.rs`** (updated): Added typed
  viewport query and response payloads for the camera overlay endpoint.
- **`CCH-Hanoi/crates/hanoi-server/src/state.rs`** (updated): Stored the loaded
  camera-overlay model in shared app state so the UI can request filtered camera
  markers without touching the routing engine.
- **`CCH-Hanoi/crates/hanoi-server/src/handlers.rs`** (updated): Added
  `GET /camera_overlay` to return viewport-filtered camera markers from the
  configured YAML file.
- **`CCH-Hanoi/crates/hanoi-server/src/main.rs`** (updated): Added a
  `--camera-config` CLI flag, built the camera overlay model during startup for
  both normal and line-graph modes, and exposed the new endpoint on the query
  server.
- **`CCH-Hanoi/crates/hanoi-server/static/index.html`** (updated): Added a
  legend-side camera overlay toggle alongside the traffic overlay controls.
- **`CCH-Hanoi/crates/hanoi-server/static/app.js`** (updated): Added a
  toggleable Leaflet camera layer that fetches viewport-scoped camera markers,
  renders them below the recommended route, and shows camera details in marker
  popups.
- **`CCH-Hanoi/crates/hanoi-server/static/styles.css`** (updated): Added legend
  and map marker styling for the new camera overlay.
- **`CCH-Hanoi/crates/hanoi-server/Cargo.toml`** (updated): Added YAML parsing
  support for loading the camera configuration file in the server crate.

## 2026-04-03 — hanoi-server tertiary-class traffic overlay filter

- **`CCH-Hanoi/crates/hanoi-server/src/traffic.rs`** (updated): Traffic overlay
  segments now load OSM `highway` classes from `road_arc_manifest.arrow` via the
  official Apache Arrow Rust crate and can be filtered server-side to show only
  roads classified as tertiary or above.
- **`CCH-Hanoi/crates/hanoi-server/src/types.rs`** (updated): Extended the
  traffic-overlay query/response contract with a `tertiary_and_above_only`
  request flag and support metadata so the UI can request and reflect the
  major-road filter state.
- **`CCH-Hanoi/crates/hanoi-server/src/main.rs`** (updated): Traffic overlay
  startup now passes the graph or original-graph manifest path into the overlay
  model so road-class filtering works in both normal and line-graph modes.
- **`CCH-Hanoi/crates/hanoi-server/static/index.html`** (updated): Added a
  legend checkbox to limit traffic overlay rendering to roads down to tertiary.
- **`CCH-Hanoi/crates/hanoi-server/static/app.js`** (updated): Wired the new
  traffic filter toggle into persisted UI state and `/traffic_overlay` requests,
  with disabled/status handling when a dataset does not support the road-class
  filter.
- **`CCH-Hanoi/crates/hanoi-server/static/styles.css`** (updated): Styled the
  legend-side traffic filter checkbox row.
- **`CCH-Hanoi/crates/hanoi-server/Cargo.toml`** (updated): Added the official
  Apache Arrow Rust crate dependency for manifest-backed traffic overlay
  classification loading.

## 2026-04-03 — hanoi-server legend moved to top right

- **`CCH-Hanoi/crates/hanoi-server/static/index.html`** (updated): Re-anchored
  the floating map legend card from the bottom-right corner to the top-right
  corner of the bundled route viewer.
- **`CCH-Hanoi/crates/hanoi-server/static/styles.css`** (updated): Added the new
  `top-right` overlay position helper for desktop and mobile layout.

## 2026-04-03 — hanoi-server thinner traffic overlay strokes

- **`CCH-Hanoi/crates/hanoi-server/static/app.js`** (updated): Reduced the
  traffic overlay line width in the bundled UI from `5` to `2` so congestion
  coloring stays visible without overpowering the route geometry.

## 2026-04-03 — hanoi-server GeoJSON route export and equal-level comparison

- **`CCH-Hanoi/crates/hanoi-core/src/cch.rs`** (updated): Extended query answers
  with exported route replay metadata so normal-graph routes can be re-evaluated
  later from GeoJSON without recomputing a shortest path.
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`** (updated): Extended
  line-graph query answers with original-arc export metadata plus exact
  line-graph replay IDs so imported GeoJSON routes can be re-evaluated against
  the current active weight profile while preserving pseudo-normal display
  geometry.
- **`CCH-Hanoi/crates/hanoi-server/src/route_eval.rs`** (new): Added imported
  route evaluation for up to 10 GeoJSON routes, including exact replay for
  exported line-graph routes and pseudo-normal fallback evaluation when only
  original-arc metadata is available.
- **`CCH-Hanoi/crates/hanoi-server/src/engine.rs`** (updated): Query responses
  and exported GeoJSON now include `graph_type`, `path_nodes`, `route_arc_ids`,
  and `weight_path_ids` so the UI can export self-contained route results for
  later comparison.
- **`CCH-Hanoi/crates/hanoi-server/src/handlers.rs`** (updated): Added
  `POST /evaluate_routes` to evaluate imported GeoJSON routes under the
  currently active baseline or customized weights.
- **`CCH-Hanoi/crates/hanoi-server/src/main.rs`** (updated): Built the new
  route-evaluator model during startup for both normal and line-graph server
  modes and exposed the evaluation endpoint on the query port.
- **`CCH-Hanoi/crates/hanoi-server/src/state.rs`** (updated): Stored the
  precomputed route evaluator in shared server state alongside the traffic
  overlay model.
- **`CCH-Hanoi/crates/hanoi-server/src/types.rs`** (updated): Added typed
  request/response payloads for imported-route evaluation and extended the query
  JSON response shape with route export metadata.
- **`CCH-Hanoi/crates/hanoi-server/static/index.html`** (updated): Added a
  dedicated `Compare` tab, GeoJSON import controls, imported-route result list,
  an optional `Focus 1-1` comparison mode, and a GeoJSON export action for live
  query results.
- **`CCH-Hanoi/crates/hanoi-server/static/app.js`** (updated): Added client
  GeoJSON export, imported-route loading/evaluation, multi-route overlay
  rendering, server-driven route comparison under active weights, and equal-
  level route cards without treating any imported route as the comparison
  reference. Added a focused pairwise mode that isolates any two imported routes
  on the map and summarizes their direct time/distance gap.
- **`CCH-Hanoi/crates/hanoi-server/static/styles.css`** (updated): Styled the
  new workspace tabs, imported-route comparison cards, comparison route map
  strokes, focus-mode controls/summary, and related controls.

## 2026-04-03 — hanoi-server traffic overlay and baseline reset UI

- **`CCH-Hanoi/crates/hanoi-server/src/traffic.rs`** (new): Added
  viewport-filtered traffic overlay generation for the bundled route viewer.
  Normal-graph mode colors road segments directly by customized-vs-baseline
  weight ratio; line-graph mode projects the metric back onto original arcs as a
  pseudo-normal road overlay using incoming-transition aggregation.
- **`CCH-Hanoi/crates/hanoi-server/src/state.rs`** (updated): Added retained
  baseline weights, latest accepted customization weights, and precomputed
  traffic-overlay geometry to shared server state.
- **`CCH-Hanoi/crates/hanoi-server/src/handlers.rs`** (updated): Added
  `GET /traffic_overlay` for viewport-scoped overlay data and
  `POST /reset_weights` to re-queue the server baseline metric and clear the
  live customization snapshot used by the UI.
- **`CCH-Hanoi/crates/hanoi-server/src/main.rs`** (updated): Built the traffic
  overlay model during startup for both normal and line-graph modes, stored the
  baseline metric in app state, and exposed the new traffic/reset routes on the
  query port.
- **`CCH-Hanoi/crates/hanoi-server/src/types.rs`** (updated): Added typed
  traffic-overlay request/response payloads.
- **`CCH-Hanoi/crates/hanoi-server/static/index.html`** (updated): Added a
  toggleable traffic-overlay control/legend to the map UI and a `Reset Weights`
  button in the server-context toolbar.
- **`CCH-Hanoi/crates/hanoi-server/static/app.js`** (updated): Added Leaflet
  traffic-layer rendering below the recommended route, viewport/poll-based
  overlay refresh, line-graph pseudo-normal status messaging, and the UI reset
  action that posts baseline restoration back to the server.
- **`CCH-Hanoi/crates/hanoi-server/static/styles.css`** (updated): Styled the
  traffic legend/toggle, server-context action row, and dedicated traffic map
  strokes.

## 2026-04-03 — CCH_Data_Pipeline graph layout compatibility

- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/GraphLoader.kt`**
  (updated): The live-weight loader now accepts the repo's current dataset
  layout where `graph/` and `line_graph/` are sibling directories under a
  dataset root, in addition to the older nested `graph/line_graph/` layout.
  Passing either the dataset root or the `graph/` directory now resolves
  correctly.
- **`CCH_Data_Pipeline/app/src/test/kotlin/com/thomas/cch_app/GraphLoaderTest.kt`**
  (new): Added regression coverage for sibling `graph/` + `line_graph/`
  resolution and the unresolved-line-graph failure path.

## 2026-04-03 — hanoi-server UI distance precision

- **`CCH-Hanoi/crates/hanoi-server/static/app.js`** (updated): Route-viewer
  distance formatting now keeps `0.01` precision in the bundled UI. Route
  summary stats, success banners, maneuver distances, and snap-distance error
  messages now show two decimal places instead of coarse integer/one-decimal
  rounding.

## 2026-04-03 — Camera config web distance precision

- **`camera-config-web/static/app.js`** (updated): Nearby arc candidate
  distances in the Camera editor now render with `0.01 m` precision instead of
  `0.1 m`, matching the backend's exported `distance_m` precision.

## 2026-04-03 — Camera config web editor delete action

- **`camera-config-web/static/index.html`** (updated): Added a `Delete` button
  to the Camera editor toolbar alongside `Reset` and `Save Camera`.
- **`camera-config-web/static/app.js`** (updated): Wired the editor delete
  button to remove the currently edited saved camera through the same deletion
  path used by the `Saved` tab list, and keep the button disabled unless the
  editor is actually in edit mode.
- **`camera-config-web/static/styles.css`** (updated): Added a shared disabled
  button treatment so the inactive editor delete action is visually distinct.

## 2026-04-03 — hanoi-server bundled CCH route viewer

- **`CCH-Hanoi/crates/hanoi-server/src/main.rs`** (updated): Added an opt-in
  `--serve-ui` flag so the query server can expose a bundled map frontend on the
  query port without changing the existing API-only default behavior.
- **`CCH-Hanoi/crates/hanoi-server/src/ui.rs`** (new): Added small static-file
  handlers for the bundled route-viewer assets.
- **`CCH-Hanoi/crates/hanoi-server/static/index.html`** (new): Added a map-first
  CCH query UI for picking source/destination points by map click or manual
  coordinates, querying `/query`, and presenting route stats.
- **`CCH-Hanoi/crates/hanoi-server/static/styles.css`** (new): Added the
  glass-inspired responsive UI styling, floating map cards, animated route
  treatment, and custom source/destination markers.
- **`CCH-Hanoi/crates/hanoi-server/static/app.js`** (new): Added the client
  logic for server status loading, coordinate editing, map click selection,
  GeoJSON route rendering, maneuver rendering, and query error handling.
- **`CCH-Hanoi/crates/hanoi-server/static/styles.css`** (updated): Allowed the
  bundled sidebar to scroll vertically on desktop so lower summary/maneuver
  content is reachable, and shifted the route treatment away from source-blue
  into a distinct green palette.
- **`CCH-Hanoi/crates/hanoi-server/static/app.js`** (updated): Switched the
  rendered GeoJSON route line and halo to the new green route palette so the
  recommended path is visually separate from the blue source marker.
- **`CCH-Hanoi/crates/hanoi-server/README.md`** (updated): Documented how to
  enable the bundled UI and clarified that API-only operation remains the
  default.

## 2026-04-03 — Camera config web map click edit shortcut

- **`camera-config-web/static/app.js`** (updated): Map camera markers now act as
  a direct `Zoom + Edit` shortcut for saved cameras. Clicking a saved camera on
  the map switches the sidebar to the `Camera` tab, loads that camera into the
  editor draft, restores its selected arc / propagation preview, and flies the
  map back to the camera location without also triggering the generic map-click
  candidate lookup.

## 2026-04-02 — CCH_Data_Pipeline: add slight deterministic free-flow variance

- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/WeightGenerator.kt`**
  (updated): Free-flow camera profiles no longer stay perfectly flat away from
  peaks. The generator now adds a small deterministic speed/occupancy wobble
  during free-flow periods, while fading that variance out near configured peak
  points so exact peak behavior remains intact.
- **`CCH_Data_Pipeline/app/src/test/kotlin/com/thomas/cch_app/WeightGeneratorTest.kt`**
  (updated): Adjusted the turn-cost preservation test to compute its expected
  free-flow weight through `profileSpeed()`, and added coverage asserting that
  the new free-flow variance is deterministic, bounded, and inactive at exact
  peak anchors.

## 2026-04-02 — Camera config web UI tightening

- **`camera-config-web/static/index.html`** (updated): Reworked the editor
  sidebar into compact workflow tabs (`Search`, `Camera`, `Saved`, `Profiles`,
  `Export`) so long search/candidate/preview lists no longer push the full
  editor below the viewport.
- **`camera-config-web/static/styles.css`** (updated): Added the new tabbed
  layout, capped the long in-panel lists with internal scrolling, and styled a
  reusable Leaflet direction-arrow marker for clearer directed-arc previews.
- **`camera-config-web/static/app.js`** (updated): Wired tab switching into the
  editor flow, replaced the selected-arc endpoint dot with a heading-aware arrow
  marker, and thickened both selected-arc and propagated-way map strokes so
  direction and coverage are easier to read during placement. The camera
  workbench now lives in floating map-side panels that only appear for the
  `Camera` tab, and those panels are individually collapsible to free up map
  space while editing. Follow-up UI tightening aligned the arrow with the actual
  rendered segment direction and clamped the floating workbench to the map pane
  so it no longer causes page-level overflow/scrollbars.
- **`camera-config-web/server.py`** (updated): Added YAML import validation for
  the web editor:
  - rejects duplicate YAML keys and schema mismatches,
  - validates imported cameras/profiles against the same MVP shape expected by
    `CCH_Data_Pipeline`,
  - resolves imported cameras back onto the current manifest so they remain
    editable in the UI,
  - rejects imported propagation overlap conflicts that would violate the
    editor's one-camera-per-represented-arc rule.
- **`camera-config-web/static/app.js`** (updated): Added a `Load YAML` flow that
  replaces the current local editor state with a validated imported config,
  preserving profiles/cameras so the user can keep editing and append new
  entries from there.
- **`camera-config-web/static/index.html`** (updated): Added a top-level
  `Load YAML` action to the web editor header.
- **`camera-config-web/static/styles.css`** (updated): Added small header action
  spacing for the YAML import control.

## 2026-04-02 — Camera Way Propagation plan

- **`docs/planned/Camera Way Propagation.md`** (new): Drafted implementation
  plan for propagating camera speed profiles to all same-way, same-direction
  sibling arcs. Currently a camera covers only one arc; this plan introduces a
  `CameraProfileExpander` that uses `RoadIndex` (way → arcs CSR) and
  `ArcManifest.isAntiparallelToWay` as the sole hard directional filter. Bearing
  difference is warning-only (not a hard rejection) to avoid excluding valid
  arcs on curved roads. Includes note clarifying that `routingWayId` and
  `osmWayId` are 1:1 bijective within the loaded graph. Changes scoped to 1 new
  file + ~10 lines in `Main.kt`; `WeightGenerator`, `CameraResolver`, and
  `GraphLoader` remain untouched.

## 2026-04-02 — Camera way propagation implementation

- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/CameraProfileExpander.kt`**
  (new): Implemented way-direction propagation from anchor arcs to sibling arcs
  sharing the same `routingWayId` and `isAntiparallelToWay`, with non-blocking
  bearing warnings and first-camera-wins overlap handling.
- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/Main.kt`**
  (updated): Inserted the propagation expansion step between camera resolution
  and weight generation, and logged anchor-vs-expanded coverage counts.
- **`CCH_Data_Pipeline/app/src/test/kotlin/com/thomas/cch_app/CameraProfileExpanderTest.kt`**
  (new): Added unit coverage for two-way grouping, backward one-way expansion,
  non-blocking large bearing differences, and first-camera-wins overlap.

## 2026-04-02 — Local camera config web app

- **`camera-config-web/`** (new): Added an isolated local web app for building
  `cameras.yaml` files against `road_arc_manifest.arrow`:
  - Python backend loads the manifest, builds a lightweight spatial index, and
    exposes nearby-arc, road-search, and YAML-export APIs.
  - Road search is accent-insensitive, so plain ASCII queries still find
    Vietnamese street names such as `Trang Tien` -> `Phố Tràng Tiền`.
  - Candidate arc selection now keeps the graph's directed semantics visible:
    the UI shows each arc's bearing plus `with/against OSM way`, sorts
    coordinate-mode candidates by flow-bearing match, and avoids auto-picking a
    directionless nearest arc when no flow bearing is available.
  - The export panel now explicitly documents the YAML contract: `arc` mode
    writes the exact directed `arc_id`, while `coordinate` mode writes only
    `lat` / `lon` / `flow_bearing_deg`.
  - Camera ownership is now enforced per directed arc: the UI flags candidate
    arcs already claimed by another camera, and saving a second camera on the
    same `selectedArc.arc_id` is rejected.
  - The editor now previews the propagated way-direction group behind the
    selected anchor arc, highlights that represented group on the map, and
    blocks saving when the propagated coverage would overlap a previously saved
    camera in the local editor state.
  - Leaflet/OpenStreetMap frontend lets the user click roads on a map, inspect
    nearby directed arcs, manage profiles, assign profiles to cameras, and
    export YAML in the same shape expected by `CCH_Data_Pipeline`.
  - Added a small README, `requirements.txt`, and local Python cache ignore
    rules for repo hygiene.

## 2026-04-01 — CCH_Data_Pipeline sample camera config

- **`CCH_Data_Pipeline/examples/cameras.sample.yaml`** (new): Added a checked-in
  sample `cameras.yaml` showing the current MVP schema:
  - profile definitions with `free_flow_kmh`, `free_flow_occupancy`, and
    optional Gaussian `peaks`,
  - both supported camera placement modes: explicit `arc_id` and
    `lat`/`lon`/`flow_bearing_deg`,
  - comments clarifying that `flow_bearing_deg` is traffic-flow direction, not
    camera-facing direction.

## 2026-04-01 — Road closure support via INFINITY weight

- **`CCH-Hanoi/crates/hanoi-server/src/handlers.rs`** (updated): Corrected the
  weight validation guard in `handle_customize` from `>= INFINITY` to
  `> INFINITY`. The original comment was misleading: INFINITY is safe to pass to
  CCH because `INFINITY + x` for any `x <= INFINITY` does not overflow `u32` (by
  design of `INFINITY = u32::MAX / 2`), and any triangle sum involving an
  INFINITY leg is `>= INFINITY`, so it never wins the `min` relaxation — closed
  edges correctly propagate as unreachable through the hierarchy.
- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/WeightGenerator.kt`**
  (updated): Added road-closure support:
  - New `closedEdges: Set<Int>` constructor parameter (arc IDs, same namespace
    as `cameraProfiles`); validated against `originalEdgeCount` at construction
    time.
  - `generateWeights()` emits `ROAD_CLOSED` for all incoming line-graph edges of
    a closed arc and skips the normal weight computation for that arc.
  - New `ROAD_CLOSED = 2_147_483_647` companion constant (= CCH `INFINITY`).
  - `logSummary()` now reports `closedOriginalEdges` and `closedLgEdges` and
    excludes `ROAD_CLOSED` entries from `minWeight`/`maxWeight` stats.

## 2026-03-31 — CCH_Data_Pipeline: harden Live Weight MVP wiring

- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/CameraConfig.kt`**
  (updated): Tightened YAML validation for the MVP camera loader:
  - rejects duplicate YAML mapping keys up front via SnakeYAML loader options,
  - rejects duplicate camera IDs,
  - rejects blank profile names and blank camera labels/profile references.
- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/Main.kt`**
  (updated): Fixed resolved-camera ownership handling:
  - duplicate resolved `arc_id` collisions now fail explicitly during runtime
    wiring instead of being silently overwritten by `associate`.
- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/CameraResolver.kt`**
  (updated): Kept the resolver focused on resolution only:
  - validates arc-manifest size against original edge count,
  - leaves duplicate arc conflict handling to the runtime assembly layer,
  - normalizes numeric formatting with a stable locale for clearer logs/errors.
- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/LineGraphFanOut.kt`**
  (updated): Added stricter reverse-index validation:
  - validates `via_way_split_map` targets even when `build()` is called
    directly,
  - verifies the reverse-index fill cursor lands exactly on each CSR sentinel.
- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/GraphLoader.kt`**
  (updated): Hardened manifest/file loading:
  - requires real files rather than just existing paths,
  - rejects null `name` / `highway` values in `road_arc_manifest.arrow` instead
    of silently converting them to empty strings.
- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/WeightGenerator.kt`**
  (updated): Added constructor-time consistency checks so mismatched graph,
  fan-out, or camera-profile wiring fails fast before weight generation.
- **`CCH_Data_Pipeline/gradle/libs.versions.toml`** (updated): Corrected the
  Arrow memory dependency to the concrete `arrow-memory-core` artifact used by
  the current Arrow Java release.
- **`CCH_Data_Pipeline/app/build.gradle.kts`** (updated): Fixed the Shadow JAR
  manifest to write the concrete main-class value instead of the Gradle property
  object.

## 2026-03-31 — CCH_Data_Pipeline: implement Live Weight MVP app

- **`CCH_Data_Pipeline/app/build.gradle.kts`** (updated): Added the MVP app
  runtime dependencies and packaging/runtime flags needed for Clikt, SnakeYAML,
  Arrow Java, Ktor, coroutines, and Arrow's JVM `--add-opens` requirement.
- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/GraphLoader.kt`**
  (new): Implemented graph and manifest loading for the MVP:
  - reads the original RoutingKit vectors needed for weighting (`travel_time`,
    `geo_distance`, `way`),
  - reads line-graph vectors (`first_out`, `head`, `travel_time`) and optional
    `via_way_split_map`,
  - reads `road_arc_manifest.arrow` via Arrow Java,
  - derives a road-level index and flat per-edge `highway` lookup from the arc
    manifest.
- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/LineGraphFanOut.kt`**
  (new): Implemented the reverse index from original/base arcs to line-graph
  edges, including normalization of split LG target nodes back to their source
  original arc IDs and preservation of per-turn baseline turn costs.
- **`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/CameraConfig.kt`**
  (new): Added the YAML camera/profile model and loader for:
  - named speed profiles,
  - explicit `arc_id` camera placement,
  - coordinate-based placement with `flow_bearing_deg`.
