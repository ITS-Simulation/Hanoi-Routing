# CHANGELOGS.md

## 2026-03-29 — CCH-Hanoi: Reset turn-generation pipeline to raw output

- **`CCH-Hanoi/crates/hanoi-core/src/geometry.rs`**: Wiped all turn
  post-processing passes (`suppress_low_degree_turns`, `detect_roundabouts`,
  `merge_straights`, `refine_turns`, `suppress_close_turns`,
  `annotate_distances`, `path_distance`). Removed `RoundaboutStraight` and
  `RoundaboutLeft` variants from `TurnDirection`. Removed `edge_count` field
  from `TurnAnnotation`. Kept core: `compute_turn_angle`, `classify_turn`,
  `compute_turns`, `TurnDirection` (Straight/Left/Right/UTurn),
  `TurnAnnotation` with `direction`, `angle_degrees`, `coordinate_index`,
  `distance_to_next_m`, `intersection_degree`. `coordinate_index` is now
  assigned directly in `compute_turns` as `i + 1`.
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`**: Removed all
  post-processing calls from both `query()` and `query_trimmed()` (no more
  `refine_turns`, `retain`, `suppress_close_turns`, `annotate_distances`).
  Turns output is now raw `compute_turns` output. Fresh start for redesign.

## 2026-03-29 — CCH-Hanoi: Suppress short-distance turn noise

- **`CCH-Hanoi/crates/hanoi-core/src/geometry.rs`**: Added
  `suppress_close_turns()` with `MIN_TURN_SPACING_M = 20.0`. Walks the turn
  list front-to-back, dropping any turn whose path distance from the previous
  surviving turn (or route start) is < 20m. Eliminates road-curvature
  artifacts caused by OSM's dense node placement on curves — e.g., a
  Right(-72°) then Left(+75°) at 0.21m apart on a smooth road bend.
  Extracted `path_distance()` helper for haversine sum between coordinate
  indices.  Also `#[serde(skip)]` on `edge_count` and `coordinate_index`
  (internal pipeline fields, not meaningful to API consumers).
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`**: Updated both
  `query()` and `query_trimmed()` post-processing pipeline to:
  `retain(Straight)` → `suppress_close_turns()` → `annotate_distances()`.

## 2026-03-29 — CCH-Hanoi: Degree-aware roundabout detection + strip straights

- **`CCH-Hanoi/crates/hanoi-core/src/geometry.rs`**: Added
  `RoundaboutStraight` and `RoundaboutLeft` variants to `TurnDirection`.
  Added `intersection_degree: u32` field to `TurnAnnotation` (`#[serde(skip)]`
  — internal only, not serialized). Replaced `cancel_s_curves()` with
  `detect_roundabouts()`: adjacent opposite-sign turn pairs (both non-straight,
  individual < 60°) are now checked against node degree — pairs where **both**
  intersection nodes have outgoing degree ≥ 3 are classified as roundabouts by
  net angle (`RoundaboutStraight` |net| < 25°, `RoundaboutLeft` net > 0°,
  plain `Right` net < 0°); pairs at degree-2 nodes collapse to `Straight`
  (road wiggle). `compute_turns()` now accepts `original_first_out` to compute
  degree. Removed `S_CURVE_NET_THRESHOLD_DEG` constant, added
  `ROUNDABOUT_MIN_DEGREE = 3`.
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`**: Updated both
  `compute_turns()` callsites in `query()` and `query_trimmed()` to pass
  `original_first_out`. Added `turns.retain(|t| t.direction !=
  TurnDirection::Straight)` in both methods **before** `annotate_distances()`
  so that `distance_to_next_m` measures the gap between consecutive actionable
  maneuvers. Straights remain in the internal refinement pipeline but are no
  longer emitted. Roundabout variants survive the filter.

## 2026-03-29 — Audit fixes: gateway compile error + snap candidate sort determinism

- **`CCH-Hanoi/crates/hanoi-gateway/src/main.rs`**: Fixed borrow-after-move
  compile error: `profile_names` held `Vec<&str>` borrowing
  `config.profiles`, which was then moved into `GatewayState::new()` but
  still referenced by `tracing::info!`. Changed to `Vec<String>` with
  `.cloned()`.
- **`CCH-Hanoi/crates/hanoi-core/src/spatial.rs`**: Added `edge_id`
  tiebreaker to `snap_candidates()` sort to ensure deterministic candidate
  ordering when multiple edges have identical snap distances, conforming to
  the Progressive Snapping plan's invariant (Section 7, point 5).

## 2026-03-29 — CCH-Hanoi: Add distance-to-next on turn annotations

- **`CCH-Hanoi/crates/hanoi-core/src/geometry.rs`**: Extended
  `TurnAnnotation` with `distance_to_next_m: f64`. Added
  `annotate_distances()` to assign per-maneuver distances from
  `coordinate_index` to the next maneuver (or route end). Updated every
  `TurnAnnotation { ... }` constructor in the turn-refinement pipeline to seed
  the new field with `0.0` until post-processing runs.
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`**: Updated both `query()`
  and `query_trimmed()` to call `annotate_distances(&mut turns, &coordinates)`
  after coordinates are built, so line-graph turn output now includes the
  distance until the next maneuver without changing route geometry assembly.

## 2026-03-29 — CCH-Hanoi: Progressive multi-candidate snapping for coordinate queries

- **`CCH-Hanoi/crates/hanoi-core/src/spatial.rs`**: Added
  `SNAP_MAX_CANDIDATES`, `snap_candidates()`, and
  `validated_snap_candidates()`. `snap_to_edge()` and `validated_snap()` now
  delegate to the multi-candidate helpers. Candidate collection still uses the
  10 nearest KD-tree nodes, now deduplicates by `edge_id`, sorts by
  `snap_distance_m`, and preserves `SnapTooFar` rejection details using the
  best available snap distance when no filtered candidate remains.
- **`CCH-Hanoi/crates/hanoi-core/src/cch.rs`**: Rewrote
  `QueryEngine::query_coords()` to request up to 5 ranked snap candidates for
  origin and destination, then try ranked snap-pair routes via nearest
  endpoints until the first routable pair succeeds. Removed the old single-snap
  endpoint-combination fallback.
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`**: Rewrote
  `LineGraphQueryEngine::query_coords()` to use ranked original-edge snap
  candidates with `query_trimmed()`, preserving unified original-graph snap
  space while removing the old `collect_original_edge_candidates()` expansion
  path.

## 2026-03-29 — CCH-Hanoi: Snap edge trimming for coordinate-based line-graph queries

- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`**: Added private
  `query_trimmed()` method to `LineGraphQueryEngine`. Runs the same CCH query as
  `query()` but trims the first and last elements from the LG path (the snapped
  source and destination edges) before building turns, coordinates, and distance.
  This eliminates phantom first/last turns caused by coordinate snapping to full
  edge extents. `query_coords()` now calls `query_trimmed()` instead of `query()`
  for both the primary path and the fallback candidate loop. `query()` remains
  unchanged for direct node-ID callers. Fallback route ranking still uses
  `distance_ms` (full CCH travel time).

## 2026-03-29 — CCH-Hanoi: Purge test code

- **`CCH-Hanoi/crates/hanoi-core/src/geometry.rs`**: Removed `#[cfg(test)]`
  module (9 unit tests for turn direction classification and refinement pipeline).
- **`CCH-Hanoi/crates/hanoi-core/tests/turn_direction_integration.rs`**: Removed
  entirely (integration test for line-graph turn annotations).

## 2026-03-29 — CCH-Hanoi: Turn refinement pipeline (S-curve cancellation + straight merging)

- **`CCH-Hanoi/crates/hanoi-core/src/geometry.rs`**: Extended `TurnAnnotation`
  with `edge_count: u32` and `coordinate_index: u32` fields. Added two new
  constants (`S_CURVE_NET_THRESHOLD_DEG = 15.0`, `S_CURVE_MAX_THRESHOLD_DEG =
  60.0`). Implemented two-pass refinement pipeline: `cancel_s_curves()` replaces
  adjacent opposite-sign turn pairs (< 60° individual, < 15° net residual) with
  a single straight; `merge_straights()` collapses consecutive straight runs into
  one entry with cumulative angle, edge count, and coordinate index.
  `refine_turns()` convenience wrapper chains both passes. Updated
  `compute_turns()` to initialize new fields.
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`**: Updated import and
  replaced bare `compute_turns(...)` call with `refine_turns(compute_turns(...))`
  so line-graph queries return refined turn annotations.

## 2026-03-28 — Fix: Move query point coordinates from geometry to properties

- **`CCH-Hanoi/crates/hanoi-core/src/cch.rs`**: Added `origin` and `destination`
  fields (`Option<(f32, f32)>`) to `QueryAnswer`. Changed `patch_coordinates()`
  to store user-supplied coordinates as metadata instead of inserting them into
  the coordinate array. `distance_m` now reflects pure graph-node distance only.
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`**: Same `patch_coordinates()`
  change — origin/destination stored as metadata, coordinate array stays pure.
- **`CCH-Hanoi/crates/hanoi-server/src/engine.rs`**: Updated `answer_to_response`
  and `answer_to_geojson` to propagate origin/destination into output properties.
  GeoJSON embeds them as `[lat, lng]` in `properties.origin` / `properties.destination`.
- **`CCH-Hanoi/crates/hanoi-server/src/types.rs`**: Added `origin` and
  `destination` fields (`Option<[f32; 2]>`) to `QueryResponse`, both with
  `skip_serializing_if = "Option::is_none"`.

## 2026-03-28 — Audit: Turn direction post-implementation fixes

- **`CCH-Hanoi/crates/hanoi-core/src/geometry.rs`**: Added `debug_assert_eq!` in
  `compute_turns()` to verify the line-graph invariant `head_a == tail_b` (shared
  intersection) at each consecutive edge pair. Catches data corruption in debug
  builds. Promoted f32→f64 before subtraction in `compute_turn_angle()` to
  preserve precision on short urban segments (f32 subtraction at lng ~105° has
  ~12m rounding error per coordinate).
- **`CCH-Hanoi/crates/hanoi-server/src/types.rs`**: Added
  `#[serde(skip_serializing_if = "Vec::is_empty")]` to `QueryResponse::turns`.
  Now `?format=json` omits the `turns` key for normal-graph queries (no turn
  data), consistent with how GeoJSON conditionally includes it.

## 2026-03-28 — CCH-Hanoi: Add line-graph turn direction annotations

- **`CCH-Hanoi/crates/hanoi-core/src/geometry.rs`** (new) + **`CCH-Hanoi/crates/hanoi-core/Cargo.toml`**:
  Added the dedicated geometry module from the implementation plan, including
  `TurnDirection`, `TurnAnnotation`, threshold constants, signed-angle
  computation via dot/cross products in a local equirectangular projection, and
  unit tests covering straight/left/right/U-turn/degenerate cases. Added
  `serde` derive support to serialize turn annotations.
- **`CCH-Hanoi/crates/hanoi-core/src/lib.rs`**, **`CCH-Hanoi/crates/hanoi-core/src/cch.rs`**,
  **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`**,
  **`CCH-Hanoi/crates/hanoi-core/tests/turn_direction_integration.rs`**:
  Registered/re-exported the new geometry module, extended `QueryAnswer` with
  `turns`, kept normal-graph queries empty by construction, computed line-graph
  turn annotations before coordinate patching, and added a synthetic
  line-graph integration test that verifies `Straight -> Left -> Straight`
  annotations on an L-shaped route.
- **`CCH-Hanoi/crates/hanoi-server/src/types.rs`** +
  **`CCH-Hanoi/crates/hanoi-server/src/engine.rs`**: Extended JSON responses
  with a `turns` field and now embed non-empty turn annotations in GeoJSON
  feature `properties` while preserving existing response behavior for routes
  without turn data.

## 2026-03-28 — Gateway: Move profile selection from JSON body to query parameter

- **`CCH-Hanoi/crates/hanoi-gateway/src/types.rs`**: Removed `GatewayQueryRequest`
  and `BackendQueryRequest` structs. Replaced with `GatewayQueryParam` that
  combines `profile`, `format`, and `colors` into a single query-string extractor.
  Body is now forwarded as raw bytes — no deserialization or stripping needed.
- **`CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs`**: `handle_query` now extracts
  `profile` from `?profile=<name>` query parameter instead of JSON body. Body is
  accepted as raw `Bytes` and forwarded unchanged to the backend. Removed
  `serde::Serialize` dependency (no longer building `BackendQueryRequest`).
- **`CCH-Hanoi/README.md`** + **`docs/walkthrough/CCH-Hanoi Usage Guide.md`**:
  Updated §7.4 endpoint table, §7.5 architecture flowchart, §11.1 gateway request
  example, §13.5 curl examples, §13.7 error case, §13.9 validation checklist —
  all `POST /query` examples now use `?profile=car` query parameter with
  profile-free JSON body.

## 2026-03-28 — Gateway: Migrate from graph-type to profile-based routing with YAML config

- **`CCH-Hanoi/crates/hanoi-gateway/Cargo.toml`**: Added `serde_yaml` dependency.
- **`CCH-Hanoi/crates/hanoi-gateway/src/config.rs`** (new): YAML config schema
  (`GatewayConfig`, `ProfileConfig`, `LogFormat`), loader with validation
  (non-empty profiles, trailing-slash normalization).
- **`CCH-Hanoi/crates/hanoi-gateway/src/types.rs`**: Replaced `graph_type`
  field with `profile` in `GatewayQueryRequest` and `InfoQuery`.
- **`CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs`**: `GatewayState` now holds
  `HashMap<String, ProfileConfig>` instead of two hardcoded URLs. Backend
  selection uses profile name lookup. Added `GET /profiles` endpoint for client
  discovery. Error responses include `available_profiles` list.
- **`CCH-Hanoi/crates/hanoi-gateway/src/main.rs`**: Replaced CLI backend args
  (`--normal_backend`, `--line_graph_backend`) with `--config <path>` pointing
  to a YAML file. `--port` retained as optional override. `LogFormat` moved to
  `config.rs`. Registered `/profiles` route.
- **`CCH-Hanoi/crates/hanoi-gateway/gateway.yaml`** (new): Example config file.
- **`CCH-Hanoi/README.md`** + **`docs/walkthrough/CCH-Hanoi Usage Guide.md`**:
  Updated gateway sections (§7, §11, §13.5, §13.7, §14.4, §14.5) — all
  `graph_type` references replaced with `profile`, CLI examples updated to use
  `--config gateway.yaml`, added `/profiles` endpoint docs, flowcharts relabeled
  from normal/line_graph to car/motorcycle.

## 2026-03-27 — Server: Sync GeoJSON output format with CLI (FeatureCollection)

- **`CCH-Hanoi/crates/hanoi-server/src/engine.rs`**: Changed `answer_to_geojson`
  to wrap results in a `FeatureCollection` (with a single `Feature`) instead of
  a bare `Feature`. This matches the `hanoi-cli` output format and improves
  compatibility with GeoJSON consumers (geojson.io, Leaflet, QGIS). Also
  switched coordinate serialization from `serde_json::json!([lng, lat])` (Value)
  to `[f32; 2]` arrays for consistency with the CLI.
- **`CCH-Hanoi/README.md`** + **`docs/walkthrough/CCH-Hanoi Usage Guide.md`**:
  Updated GeoJSON response examples and verification checklists to reflect
  FeatureCollection wrapper.

## 2026-03-27 — Server: Add `?colors` query param for simplestyle-spec GeoJSON

- **`CCH-Hanoi/crates/hanoi-server/src/`**: Added `colors` query parameter.
  When present (`?colors`), GeoJSON responses include simplestyle-spec
  visualization properties (`stroke`, `stroke-width`, `fill`, `fill-opacity`).
  Ignored when `?format=json`.
  - `types.rs`: Added `colors: Option<String>` to `FormatParam`
  - `state.rs`: Added `colors: bool` to `QueryMsg`
  - `handlers.rs`: Passes `colors` presence from query params
  - `engine.rs`: `answer_to_geojson` injects simplestyle properties when
    `colors` is true
- **`CCH-Hanoi/crates/hanoi-gateway/src/`**: Gateway forwards `?colors` param
  to backend URLs alongside `?format`.
  - `types.rs`: Added `colors` to `GatewayFormatParam`
  - `proxy.rs`: Builds query string with both `format` and `colors` params
- **`CCH-Hanoi/README.md`** + **`docs/walkthrough/CCH-Hanoi Usage Guide.md`**:
  Updated API docs and test examples with `?colors` usage.

## 2026-03-26 — Docs: Clarify query walk termination in Stage 5

- **`docs/walkthrough/CCH Deep Dive.md`**: Rewrote Stage 5 "The idea" and
  "How the walk works" sections to make clear that both walks go **all the way
  to the root** — they do not stop at the first intersection. Added explanation
  of why early termination is incorrect (rank order ≠ distance order, unlike
  Dijkstra), a scenario diagram showing how the first intersection can be
  suboptimal, and expanded the concrete query meeting-point table with a
  "why not optimal?" column demonstrating the cost of early stopping.

## 2026-03-26 — Docs: Clarify triangle relaxation directional cross-pattern

- **`docs/walkthrough/CCH Deep Dive.md`**: Expanded the "Sub-phase B: Lower
  triangle relaxation" section with a detailed explanation of why the formulas
  use a cross-pattern (downward + upward, upward + downward). Added step-by-step
  directional traces showing how the two-hop detour through the lowest-ranked
  node always goes "down then up" in the elimination ordering, a summary table,
  and expanded the "Why it works" subsection with three numbered guarantees
  (triangle sufficiency, bottom-up correctness, single-pass property).

## 2026-03-26 — CCH-Generator: Downgrade travel-time >24h check to warning

- **`CCH-Generator/src/validate_graph.cpp`**: Changed the "Travel time sanity"
  check for arcs exceeding 24h travel time from a hard `[FAIL]` to a `[WARN]`.
  Country-scale maps (e.g., full Vietnam) can have legitimately long road
  segments on slow rural/mountain roads that exceed 24h, which is a data
  characteristic rather than structural corruption. Zero-travel-time arcs were
  already treated as warnings; this makes the behavior consistent.

## 2026-03-25 — Docs: Unified 8-node graph examples across CCH Deep Dive

- **`docs/walkthrough/CCH Deep Dive.md`**: Replaced all per-section toy examples
  (3-node, 5-node) with a single 8-node directed graph traced end-to-end through
  every CCH stage:
  - **Stage 1 (IFC)**: Corrected to symmetrize graph via `add_back_arcs` (8 added
    reverses → 26 arcs), fixed inter-arc capacity from ∞ to 1, rewrote
    Ford-Fulkerson trace on the expanded graph yielding separator {C,E} (was {E}),
    corrected to 8 geographic directions (was 4).
  - **Stage 2 (Contraction)**: Full FAST algorithm trace producing 4 shortcuts
    (B—G, G—C, H—C, H—E), 17-edge chordal supergraph.
  - **Stage 3 (Elimination Tree)**: Tree derived from contraction parent pointers,
    showing how tree shape mirrors the nested dissection structure.
  - **Stage 4 (Customization)**: Complete triangle relaxation trace for all 8
    nodes, showing how each shortcut gets its weight (e.g., C—E relaxed from 6 to
    2 via C→B(1)+B→E(1)).
  - **Stage 5 (Query)**: Bidirectional walk for query A→H, meeting at E with
    distance 12.
  - **Stage 6 (Unpacking)**: Recursive shortcut unpacking of A→H producing final
    path A→C→B→E→F→H = 12.

## 2026-03-24 — Server: GeoJSON default + format as query parameter

- **`CCH-Hanoi/crates/hanoi-server/src/`**: Moved `format` from the JSON
  request body to a URL query parameter (`?format=json`). GeoJSON is now the
  default response format (no query param needed); pass `?format=json` for the
  legacy flat JSON response.
  - `types.rs`: Removed `format` from `QueryRequest`, added `FormatParam` struct
  - `handlers.rs`: Added `Query<FormatParam>` extractor alongside `Json` body
  - `state.rs`: Added `format: Option<String>` to `QueryMsg`
  - `engine.rs`: Flipped default in `format_response` (GeoJSON first, JSON on
    explicit `?format=json`)
- **`CCH-Hanoi/crates/hanoi-gateway/src/`**: Updated gateway to accept `format`
  as a query parameter and forward it to backend URLs.
  - `types.rs`: Removed `format` from `GatewayQueryRequest`/`BackendQueryRequest`,
    added `GatewayFormatParam`
  - `proxy.rs`: Extracts `format` from query params, appends to backend URL
- **`CCH-Hanoi/README.md`** + **`docs/walkthrough/CCH-Hanoi Usage Guide.md`**:
  Updated API docs, curl examples, verification checklists, and flowcharts to
  reflect GeoJSON-default behavior and `?format=json` query parameter.

## 2026-03-24 — CCH Deep Dive Walkthrough

- **`docs/walkthrough/CCH Deep Dive.md`**: Conceptual deep-dive explaining
  the meaning behind CCH data structures, the elimination tree, and triangular
  relaxation. Covers the full transformation pipeline (CSR → node ordering →
  contraction → elimination tree → customization → query → path unpacking)
  with step-by-step worked examples, ASCII visualizations, and a complete
  data structure reference. Complements the existing CCH Walkthrough (which
  covers operational commands/code) with the "why" behind each transformation.
  - Updated Section 5 (Elimination Tree) with expanded clarification: what the
    tree is vs. is not (contraction history, not network representation), what
    the root represents (graph-theoretic bisector, not traffic importance),
    how layering maps to geographic scope (leaves = local side-streets,
    root = city bisector), and how tree depth determines query cost for
    close-by vs. cross-city routes.
  - Updated Section 4 (Contraction) with CH vs. CCH comparison: why CH
    contraction is weight-dependent (witness search) while CCH is weight-
    independent (unconditional shortcuts), how nested dissection ordering
    confines shortcuts within cells (no cross-partition leakage), the FAST
    algorithm's lowest-neighbor-merge optimization vs. all-pairs connection,
    and the core tradeoff table (more shortcuts but ~1s recustomization vs
    CH's fewer shortcuts but full rebuild on weight change).

## 2026-03-24 — CLI GeoJSON cleanup + --demo flag

- **`CCH-Hanoi/crates/hanoi-cli/src/main.rs`**:
  - Removed `path_nodes` array from GeoJSON `properties`. The LineString
    geometry already encodes the full path; raw node IDs are meaningless
    outside the graph and bloat the file. JSON output retains `path_nodes`.
  - Added `--demo` boolean flag to `Query` command. When active, injects
    simplestyle-spec properties (`stroke: #ff5500`, `stroke-width: 3`,
    `fill: #ffaa00`, `fill-opacity: 0.4`) into GeoJSON output for quick
    visualization in geojson.io / GitHub / QGIS.

## 2026-03-24 — Smoother Module Implementation Guide

- **`docs/walkthrough/Smoother Module.md`**: Step-by-step implementation guide
  for the Huber DES smoother module. Covers all 5 source files, build config,
  6 unit tests with test helpers, and verification checklist. Designed to be
  self-contained (no dependencies on other CCH_Data_Pipeline modules).

## 2026-03-24 — Live Weight Pipeline Plan Rev 4: Dual-Signal + I/O Contracts

### Summary

Major revision to live weight pipeline plan. Added dual-signal architecture
(speed + occupancy per camera), inter-module I/O data format specification,
temporal misalignment resolution, and aligned module layout with actual
`CCH_Data_Pipeline` Gradle project structure.

### Changes

- **`docs/planned/Live Weight Pipeline.md`** (Rev 4):
  - `SpeedPacket` → `CameraPacket` carrying both `speedKmh` and `occupancy`
  - Dual-lane aggregation: `DualAggregator` demuxes into `SpeedSummary` and
    `OccupancySummary` channels with shared window boundaries
  - Generic `Smoother<S>` interface with per-lane Huber DES parameters
    (speed: α=0.3/β=0.1/δ=15km/h; occupancy: α=0.2/β=0.05/δ=0.15)
  - New `EdgeJoiner` component with three-tier alignment strategy:
    co-temporal (< 30s), stale-gap (decayed confidence), dead-gap
    (fundamental-diagram interpolation)
  - `JoinedEdgeState` data class combining speed + occupancy + alignmentAge
  - Weight model updated to use occupancy scaling formula
  - New Section 5: full inter-module I/O data format specification with
    exact data classes, invariants, serialization formats, and boundary map
  - Project structure aligned to actual `CCH_Data_Pipeline/` modules
    (app, simulation, smoother, modeler)
  - Section numbering corrected (1–14)

---

## 2026-03-23 — Live Weight Pipeline Plan

### Summary

Created architecture plan for the live weight pipeline (`docs/planned/Live Weight Pipeline.md`).
This is the "horizontal T-bar" — ingesting real-time camera speed data into CCH weight vectors.

### Changes

- **`docs/planned/Live Weight Pipeline.md`** (Rev 3): Full plan covering:
  - Architecture critique of the original camera → worker → Kafka → smooth → model proposal
  - Tech stack: **Kotlin** (coroutines, fast iteration, JVM stability) in new
    `Live_Network_Routing` project; Rust was considered but Kotlin fits
    I/O-bound data pipeline work better
  - Camera-edge mapping backbone: `CameraMappingSource` interface with JSON
    config for simulation and database-backed `camera_edge_map` table for
    production (with schema and GIS snapping recommendation)
  - Weight model as explicit separate module: takes Huber DES output, produces
    `IntArray` weight vectors for CCH customization
  - Kotlin coroutine `Channel` replaces Kafka for simulation; `PacketSource`
    interface as extension point for DE team's future broker
  - Two-tier staleness TTL (stale at 5min, dead at 30min) with linear
    confidence decay for graceful degradation
  - Architecture risk (full-vector customization at high frequency) documented
    with three candidate future fixes: delta customization, batched partial
    updates, dual-buffer engine
  - Quantified graph analysis: 1,869,499 edges, 166K tertiary+ (8.9%)
  - Five-tier uncovered-edge strategy with neighbor congestion propagation
  - 6 implementation phases, 6 simulation scenarios

## 2026-03-20 — Validator Support for Node-Split Line Graphs

### Summary

Updated `validate_graph` to handle the expanded line graph produced by via-way
node splitting. The line graph now has `arc_count + split_nodes` nodes instead of
exactly `arc_count`, and transition checks must resolve split node IDs back to
their original arc IDs through the `via_way_split_map`. Also added validation for
the three via-way chain files (`via_way_chain_offsets`, `via_way_chain_arcs`,
`via_way_chain_mandatory`) produced by `conditional_turn_extract`.

### Changes

- **`CCH-Generator/src/validate_graph.cpp`**:
  - Extended `LineGraphData` with `split_map` field and
    `resolve_to_original_arc()` helper that maps split node IDs back through the
    split map for transition validation.
  - Added `load_optional_vector()` template for files that may not exist (backward
    compatibility with pre-split line graphs).
  - Updated `load_line_graph()` to optionally load `via_way_split_map`.
  - **Line graph node count check**: Changed from `node_count == arc_count` to
    `node_count == arc_count + split_map.size()`.
  - **Split map validity**: New check verifying all split_map entries reference
    valid base LG node IDs (`< arc_count`).
  - **Transition checks** (forbidden turn + connectivity): Resolve raw LG node IDs
    through `resolve_to_original_arc()` before checking against original graph, so
    edges involving split nodes are validated correctly.
  - **Via-way chain file validation** (new section): Checks file presence
    (all-or-nothing), CSR offset structure (monotonic, sentinel match), mandatory
    vector length consistency, arc ID bounds (`< arc_count`), and minimum chain
    length (>= 3).

---

## 2026-03-20 — Via-Way Turn Restriction Support (Node Splitting)

### Summary

Implemented via-way turn restriction enforcement in the line graph using node
splitting. Previously, 194 unconditional via-way restrictions from OSM were
silently dropped (RoutingKit's built-in decoder skips via-way members). The
conditional resolver decomposed them into independent `(from_arc, to_arc)` pairs,
which over-restricts in the line graph by structurally removing individual edges
rather than forbidding specific multi-edge paths. Node splitting creates "tainted"
copies of intermediate nodes that track chain entry, forbidding only the specific
forbidden path while preserving all other legal routes through the same edges.

### Changes

- **`RoutingKit/include/routingkit/conditional_restriction_resolver.h`**: Added
  `ViaWayChain` struct and `resolve_via_way_chains()` function declaration with
  inline default-profile overload.
- **`RoutingKit/src/conditional_restriction_resolver.cpp`**: Added
  `walk_way_arcs()` helper (walks multi-arc via-way segments between junctions)
  and `resolve_via_way_chains()` implementation. Filters to unconditional via-way
  restrictions, resolves full arc chains through junction/way resolution, handles
  multi-segment via-ways.
- **`RoutingKit/src/conditional_turn_extract.cpp`**: Added `save_uint8_vector()`
  helper. Added Step 2a after arc-pair resolution: calls
  `resolve_via_way_chains()` and writes `via_way_chain_offsets`,
  `via_way_chain_arcs`, `via_way_chain_mandatory` to `graph_dir/`. Empty files
  written when zero restrictions (mandatory input for `generate_line_graph`).
- **`CCH-Hanoi/crates/hanoi-core/src/via_way_restriction.rs`** (new): Types
  (`ViaWayChain`, `SplitResult`), I/O (`load_via_way_chains()`), and the
  node-splitting algorithm (`apply_node_splits()`). Supports both prohibitive
  (removes forbidden exit) and mandatory (keeps only chain-continuation edge)
  restrictions. Verifies chain connectivity before splitting.
- **`CCH-Hanoi/crates/hanoi-core/src/lib.rs`**: Added
  `pub mod via_way_restriction`.
- **`CCH-Hanoi/crates/hanoi-tools/src/bin/generate_line_graph.rs`**: Loads
  via-way chains (mandatory — errors if files missing), applies node splitting
  after `line_graph()`, extends coordinate arrays for split nodes, writes
  expanded CSR and `via_way_split_map` to output directory.
- **`CCH-Hanoi/crates/hanoi-tools/Cargo.toml`**: Added `hanoi-core` dependency.
- **`CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`**: Loads `via_way_split_map`
  (mandatory) in `load_and_build()`, extends `original_tail`, `original_head`,
  and `original_travel_time` arrays for split nodes, adds consistency check.

### Design

See `docs/planned/Via-Way Turn Restrictions.md` for the full plan including the
node-splitting algorithm, CCH interaction diagram, and edge case verification
matrix.

---

## 2026-03-20 — Validator Accepts U-turn Line-Graph Transitions

### Summary

Updated `validate_graph` so turn-expanded validation no longer fails when the
line graph contains U-turn transitions. This matches the current routing model,
which now allows U-turns.

### Changes

- **`CCH-Generator/src/validate_graph.cpp`**: Removed the dedicated
  "No U-turns in line graph" validation from the turn-expanded checks. The
  validator still checks line-graph node count, forbidden-turn transitions, and
  general transition consistency.

---

## 2026-03-19 — Unified Snap Space for Line Graph Coordinate Queries

### Summary

Changed `LineGraphQueryEngine::query_coords()` to snap coordinates in the
**original graph's** intersection-node coordinate space instead of the line
graph's tail-node coordinate space. This eliminates the coordinate-space
divergence that caused the line graph engine to select a different starting road
segment than the normal `QueryEngine` for the same input coordinates.

### Problem

The line graph's spatial index was built on tail-node coordinates (each line-graph
node gets the latitude/longitude of the original edge's source intersection).
This is a different coordinate space from the normal graph's intersection-node
coordinates, so the KD-tree nearest-neighbor search could select a different
physical road at the same input location — leading to artificially divergent
routes even when both engines should start from the same road.

### Fix

- **`LineGraphCchContext`**: Now stores `original_first_out` (previously loaded
  and discarded) so the engine can build a spatial index on the original graph's
  full CSR structure.
- **`LineGraphQueryEngine`**: Builds its spatial index on `original_latitude`,
  `original_longitude`, `original_first_out`, and `original_head` — the same
  coordinate space as the normal `QueryEngine`. The snapped original edge ID
  maps directly to a line-graph node ID (line-graph node N = original edge N).
- **Fallback candidates**: `collect_original_edge_candidates()` replaces
  `collect_candidate_nodes_prioritized()` — expands candidate set by
  enumerating outgoing original edges from both endpoints of the snapped edge.

### Changes

- **`hanoi-core/src/line_graph.rs`**: Added `original_first_out` field to
  `LineGraphCchContext`. Rewrote `LineGraphQueryEngine` spatial index
  construction, `query_coords()`, and candidate collection to use
  original-graph snap space.

### Verified

- Motorcycle profile: 99.1% node overlap between normal and line graph
  (diverges only near destination due to legitimate turn restrictions).
- Car profile: both engines now snap to the exact same starting node;
  remaining 12.6% travel-time delta is genuine turn-restriction routing cost.
- Full workspace builds clean (hanoi-core, hanoi-cli, hanoi-server, hanoi-bench).

---

## 2026-03-19 — CLI Query Results Always Written to File

### Summary

Changed `cch-hanoi query` to always write results to a file instead of dumping
potentially thousands of path nodes to stdout. When `--output-file` is omitted,
an auto-generated timestamped file is created (e.g., `query_2026-03-19T143052.geojson`).
A concise summary (distance, node count, output path) is logged to stderr via tracing.

### Changes

- **`hanoi-cli/src/main.rs`**: Removed `println!` stdout path for query results.
  Auto-generates output filename from timestamp + format extension when
  `--output-file` is not provided. Logs structured summary with `tracing::info!`.
- **`hanoi-cli/Cargo.toml`**: Added `chrono = "0.4"` for timestamp generation.
- **Docs**: Updated §8.1 in Usage Guide and README to describe always-to-file
  behavior with auto-generated filenames.

---

## 2026-03-19 — Always-on Tracing-based File Logging for hanoi-bench

### Summary

Added dual-output tracing (stderr + JSON file) to all three hanoi-bench
binaries: `bench_core`, `bench_server`, `bench_report`. Every bench run now
automatically creates a machine-readable JSON log file in the current directory.

### Changes

**New file: `hanoi-bench/src/log.rs`**
- Shared `init_bench_tracing(name: Option<&str>) -> (PathBuf, WorkerGuard)`
- Stderr: compact format (no target) for concise progress
- File: JSON format via `tracing_appender::non_blocking` — no ANSI leakage
  (uses `JsonFields` formatter, same proven pattern as hanoi-server)
- Auto-generated filename: `{name}_{timestamp}.log`
- `RUST_LOG` env var controls filter level (default: `info`)

**Updated binaries:**
- `bench_core.rs`: Replaced all `eprintln!` with structured `tracing::info!`,
  added `--log-name` CLI arg, wired up `init_bench_tracing`
- `bench_server.rs`: Same conversion — structured tracing with fields
  (`path`, `graph_dir`, `query_count`, `concurrency`), `--log-name`
- `bench_report.rs`: Converted to tracing, `load_runs()` returns `Result`
  instead of calling `process::exit(1)` directly (ensures `WorkerGuard` is
  dropped to flush logs before exit on error paths)

**Dependencies added to `hanoi-bench/Cargo.toml`:**
- `tracing = "0.1"`
- `tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }`
- `tracing-appender = "0.2"`

### Audit Fix

- `bench_report.rs`: Original `load_runs()` called `eprintln!` + `process::exit(1)`
  which skips destructors, silently discarding any buffered log events. Refactored
  to return `Result<Vec<BenchmarkRun>, String>`, with callers using
  `tracing::error!` + `drop(guard)` before `process::exit(1)`.

### Documentation

- Updated §10.1, §10.2, §10.3 CLI tables with `--log-name` argument
- Added new §10.4 "Benchmark Logging" section describing always-on file logging
- Renumbered §10.4→10.5 (Criterion), §10.5→10.6 (Reproducible Datasets)
- Updated §16.2 logging capabilities matrix: hanoi-bench now shows `RUST_LOG`
  support, always-on file logging, compact stderr, JSON file format
- Updated hanoi-bench note from "no tracing" to describe dual-output behavior

---

## 2026-03-19 — Audit: Fix 6 bugs in hanoi-cli/hanoi-gateway dual logging and GeoJSON output

### Context

Audit of the preceding "Dual Logging + GeoJSON Output" changes implemented by Haiku.
Found and fixed 6 bugs, ranging from critical (silent data loss) to cosmetic.

### Bugs Found and Fixed

**BUG 1 (CRITICAL): WorkerGuard dropped immediately — file logs silently lost**
- `init_tracing()` in both `hanoi-cli` and `hanoi-gateway` returned `()`, so the
  `WorkerGuard` from `tracing_appender::non_blocking()` was dropped at function exit.
  This closes the background writer thread, meaning **all subsequent log events to
  the file layer were silently discarded**. The log file would be empty or contain
  only events emitted during `init_tracing()` itself.
- Fix: Changed return type to `Option<WorkerGuard>`, caller holds `let _guard = ...`
  in `main()`, matching `hanoi-server`'s proven pattern.

**BUG 2 (MEDIUM): `--output-format` accepted any string — no validation**
- `output_format` was a free `String`. Running `--output-format foo` was accepted by
  clap and only caught at runtime by a recursive fallback in `format_result`.
- Fix: Replaced with `OutputFormat` enum deriving `ValueEnum`. Clap now rejects
  invalid values at parse time with `[possible values: geojson, json]`.
  Also eliminated the recursive `format_result` fallback (no longer needed), and
  changed the function to take `&[u32]`/`&[(f32, f32)]` slices instead of owned Vecs.

**BUG 3 (MEDIUM): ANSI escape codes leaked into log file**
- The file layer used `DefaultFields` format with `.with_ansi(false)`, but span
  field formatting is shared across layers in a `tracing_subscriber` registry.
  The stderr layer's ANSI formatting contaminated span fields (e.g.,
  `load_and_build{^[[3mgraph_dir^[[0m=...}`) in the file output.
- Fix: Changed file layer to use `.json()` format (with `JsonFields`), matching
  `hanoi-server`. JSON has its own field formatter immune to ANSI leakage.
  Added `"json"` feature to `tracing-subscriber` in both Cargo.toml files.

**BUG 4 (MEDIUM): JSON/Tree stderr formats silently fell back to Pretty**
- After adding the `"json"` feature to `tracing-subscriber`, the `LogFormat::Json`
  arm was still falling back to `.pretty()` despite JSON now being available.
- Fix: `Json` arm now correctly uses `.json()`. `Tree` remains a fallback (no
  `tracing-tree` dep in CLI/gateway).

**BUG 5 (LOW): Misleading `--log-file` help text**
- hanoi-cli said "Write logs to file instead of stderr" — but the actual behavior
  is dual-writer (both stderr and file simultaneously).
- Fix: Updated to "Also write logs to file in JSON format (logs go to both stderr
  and file)".

**BUG 6 (COSMETIC): No trailing newline in output file**
- `std::fs::write(&path, output_str)` wrote JSON without a trailing newline.
- Fix: `format!("{}\n", output_str)`.

### Documentation Corrections

Updated sections 7.2 and 8.1 in both Usage Guide and README:
- File logging description: "ANSI codes stripped" → "JSON format"
- Clarified `--log-format` only affects stderr; file is always JSON
- Added `jq` parsing examples
- Removed incorrect "Logs to file with pretty format" phrasing

### Files

- `CCH-Hanoi/crates/hanoi-cli/src/main.rs` (6 fixes)
- `CCH-Hanoi/crates/hanoi-cli/Cargo.toml` (added `json` feature to tracing-subscriber)
- `CCH-Hanoi/crates/hanoi-gateway/src/main.rs` (3 fixes: guard, ANSI, JSON format)
- `CCH-Hanoi/crates/hanoi-gateway/Cargo.toml` (added `json` feature to tracing-subscriber)
- `docs/walkthrough/CCH-Hanoi Usage Guide.md` (sections 7.2, 8.1 corrected)
- `CCH-Hanoi/README.md` (sections 7.2, 8.1 corrected)

## 2026-03-19 — Dual Logging for hanoi-cli and hanoi-gateway; GeoJSON Output for Queries

### Change

Enhanced logging infrastructure across CLI and server tools: added concurrent logging (stderr + file)
to both `hanoi-cli` and `hanoi-gateway`. Added file output and GeoJSON format support to `cch-hanoi` query command.
This resolves issues with terminal color codes in redirected output and provides machine-readable logs for parsing.

### Details

**hanoi-cli (`cch-hanoi`) query enhancements:**
- New `--output-file <PATH>` flag: write query results to file instead of stdout
- New `--output-format <FORMAT>` flag: `geojson` (default) or `json`
- GeoJSON follows RFC 7946: coordinates as [longitude, latitude] for mapping library compatibility
- JSON format preserves legacy [latitude, longitude] coordinate order

**hanoi-cli logging:**
- New top-level `--log-file <PATH>` flag: write logs to file without ANSI color codes
- Concurrent dual-writer: logs sent to **both** file and stderr simultaneously
- File output is machine-readable (no escape codes), suitable for log parsing and jq filtering
- Supports all 5 log formats (Pretty, Compact, Full, Tree, Json) with dual output

**hanoi-gateway logging:**
- New `--log-file <PATH>` flag: write logs to file without ANSI color codes
- Concurrent dual-writer: logs sent to **both** file and stderr simultaneously
- All 5 log formats supported (Full is fallback for Tree)
- Useful for monitoring gateway behavior in production

**Logging architecture:**
- Both binaries use `tracing_appender::non_blocking()` for lock-free file I/O
- Stderr layer has ANSI colors enabled; file layer disables them for readability
- Dual-writer pattern matches existing hanoi-server implementation

**Dependencies:**
- Added `tracing-appender` crate to both hanoi-cli and hanoi-gateway

### Example Usage

```bash
# ========== cch-hanoi ==========

# Query with GeoJSON to file (default format)
cch-hanoi query --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843 \
  --output-file result.geojson

# Logs to both stderr and file (dual-writer)
cch-hanoi --log-file query.log query --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843

# ========== hanoi_gateway ==========

# Logs to both stderr and file
hanoi_gateway --port 50051 \
  --normal-backend http://localhost:8080 \
  --line-graph-backend http://localhost:8081 \
  --log-file gateway.log

# Combined with log format (Compact produces single-line, no-color for file)
hanoi_gateway --log-format compact --log-file gateway.log
```

### Files

- `CCH-Hanoi/crates/hanoi-cli/src/main.rs` (enhanced: output formatting, dual logging)
- `CCH-Hanoi/crates/hanoi-cli/Cargo.toml` (added: tracing-appender)
- `CCH-Hanoi/crates/hanoi-gateway/src/main.rs` (enhanced: dual logging)
- `CCH-Hanoi/crates/hanoi-gateway/Cargo.toml` (added: tracing-appender)
- `docs/walkthrough/CCH-Hanoi Usage Guide.md` (section 8.1: expanded with output/logging docs)
- `CCH-Hanoi/README.md` (section 8.1: mirrored changes)

## 2026-03-19 — Comprehensive Logging Setup Guide

### Change

Added Section 16 "Logging Setup Guide" to the CCH-Hanoi Usage Guide and README.
Expanded the existing stub section 6.6 to reference the new comprehensive guide.

### Details

- Full documentation of the tracing-based logging stack architecture
- Per-binary capability matrix (hanoi_server, cch-hanoi, hanoi_gateway,
  generate_line_graph, hanoi-bench) with feature availability
- Detailed reference for all 5 output formats: Pretty, Full, Compact, Tree, Json
  — with example output, characteristics, and best-use recommendations
- Format comparison table (lines/event, color, source location, thread ID, etc.)
- Complete RUST_LOG environment variable guide: default filters per binary,
  filter syntax, per-module granularity, and common recipes
- File-based logging documentation (hanoi-server --log-dir): daily rotation,
  JSON-only file format, non-blocking writer, WorkerGuard lifetime, jq recipes
- HTTP request tracing via tower-http TraceLayer (server and gateway)
- Full inventory of instrumented code points: info/warn/debug events and
  tracing::instrument spans with field names
- ANSI color control (NO_COLOR=1)
- Recommended configurations for development, production, CI, and debugging
- Note on cch-hanoi --log-format being a top-level flag (before subcommand)

### Files

- `docs/walkthrough/CCH-Hanoi Usage Guide.md` (updated: TOC, section 6.6, new section 16)
- `CCH-Hanoi/README.md` (updated: same changes mirrored)

## 2026-03-19 — IFC Parameter Calibration Recommendations for Hanoi

### Change

New planned document with calibration recommendations for the three
InertialFlowCutter shell scripts (`flow_cutter_cch_order.sh`,
`flow_cutter_cch_cut_order.sh`, `flow_cutter_cch_cut_reorder.sh`) to produce
better contraction permutations for Hanoi's dense urban mesh.

### Details

- Source-level analysis of how each IFC parameter controls the accelerated flow
  cutter: geo-position ordering, bulk piercing (equidistant vs adaptive),
  separator selection, pierce rating, and distance-aware cutters
- Diagnosis of 5 specific issues with current defaults on dense meshes:
  too few projection directions, early bulk saturation, small initial seed,
  conservative step fraction, no distance-based cutters
- Primary recommendations: `geo_pos_ordering_cutter_count` 8 -> 16,
  `bulk_assimilation_order_threshold` 0.25 -> 0.35,
  `initial_assimilated_fraction` 0.05 -> 0.10, `bulk_step_fraction` 0.05 -> 0.08
- Secondary recommendations: add 4 distance-ordering cutters, enable
  BulkDistance, relax max_imbalance to 0.25
- Three experimental variant configurations (aggressive, quality, BFS)
- Evaluation methodology using `examine_chordal_supergraph` output

### Files

- `docs/planned/IFC Parameter Calibration for Hanoi.md` (new)

---

## 2026-03-18 — Add comprehensive CCH-Hanoi Usage Guide

### Change

New walkthrough document covering the entire CCH-Hanoi system from build to
production operation: all 7 crates, every HTTP endpoint (request/response
format), CLI arguments, library API reference, weight customization, testing
guidance, operational flowcharts, and troubleshooting.

### Details

- **Workspace architecture**: crate dependency graph, build commands, binary
  outputs, edition/toolchain requirements
- **Data prerequisites**: directory layout for normal and line-graph modes,
  file format reference, CSR quick reference, dimension check scripts
- **hanoi-core API reference**: GraphData, CchContext, QueryEngine,
  LineGraphCchContext, LineGraphQueryEngine, SpatialIndex, BoundingBox,
  ValidationConfig, CoordRejection — all public types and methods documented
- **hanoi-server**: dual-port architecture, CLI arguments, engine loop,
  graceful shutdown, logging configuration
- **hanoi-gateway**: routing architecture, CLI arguments, error propagation
- **hanoi-cli**: query and info commands, directory conventions, exit codes
- **hanoi-tools**: generate_line_graph input/output specification
- **hanoi-bench**: core/server/report runners, Criterion harnesses,
  reproducible query datasets
- **HTTP API reference**: all 5 endpoints with request/response examples,
  status codes, validation errors, GeoJSON format
- **Weight customization guide**: how customization works, weight generation
  (Python and Rust), upload procedure, constraints, line graph considerations
- **Testing guide**: three-stage strategy (default weights → randomized
  fixed-seed → multiple weight sets), gateway testing, GeoJSON validation,
  error case testing, performance benchmarking, validation checklist
- **Operational flowcharts**: startup, query processing, customization, full
  deployment architecture with T-shape diagram (ASCII diagrams)
- **Integrated data pipeline (planned)**: full end-to-end architecture from
  traffic data sources through Huber-robust Double Exponential Smoothing, weight
  modeling (speed × distance → travel_time), to POST /customize upload — with
  pipeline stage descriptions and boundary contract table
- **Troubleshooting**: common issues, debug commands, performance expectations

### Files changed

- `docs/walkthrough/CCH-Hanoi Usage Guide.md` — **NEW**: comprehensive usage
  guide

---

## 2026-03-18 — Comprehensive CCH-Hanoi audit: gateway, server, and flow fixes

### Change

Full crate-by-crate, file-by-file audit of the entire CCH-Hanoi workspace (7
crates, 25 source files). Verified health monitoring & graceful shutdown plan
conformance, then audited all crates for logic loopholes, cross-crate
consistency, and end-to-end application flow correctness.

### Issues found and fixed

1. **Gateway `/query` missing 5xx propagation** — `handle_query` only checked
   `status.is_client_error()`, silently passing backend 500 errors through as
   200 OK. Added `|| status.is_server_error()` check to match `/info` handler.

2. **Gateway `/info` inconsistent error format** — `/info` errors returned
   plain `(StatusCode, String)` while `/query` returned `(StatusCode,
   Json<Value>)`. Unified `/info` to also return structured JSON error bodies
   for consistent API behavior behind the gateway.

3. **Server `/customize` missing weight value validation** — Accepted arbitrary
   `u32` values including `>= INFINITY` (u32::MAX / 2). CCH triangle relaxation
   uses plain addition, so `INFINITY + finite` produces a large finite value
   instead of remaining INFINITY, corrupting shortest-path results. Added
   per-value validation rejecting weights `>= INFINITY`, matching the legacy
   `rust_road_router` server's behavior.

4. **Shutdown timeout broadcast subscription ordering** — The `shutdown_timeout`
   future subscribed to the broadcast channel after the signal handler task was
   spawned. If a signal fired in the gap between spawn and subscribe, the
   timeout would miss it and never fire. Moved the subscription before the
   signal handler spawn so all three receivers (`customize_shutdown`,
   `query_shutdown`, `shutdown_timeout`) are registered before any sender task
   exists.

### Verified correct (no changes needed)

- **hanoi-core** (graph.rs, cch.rs, line_graph.rs, spatial.rs, bounds.rs): CSR
  validation, CCH build, directed CCH, query/customization, spatial indexing,
  haversine perpendicular projection, coordinate validation, final-edge
  correction, path mapping (including single-edge and same-source-target edge
  cases) all verified correct.
- **hanoi-server** (engine.rs, state.rs, types.rs): Health monitoring & graceful
  shutdown fully conforms to plan. Engine loop poll/customization cycle correct.
  Watch-channel last-writer-wins semantics correct and intentional. Query counter
  increments at the right layer.
- **hanoi-cli** (main.rs): Both normal and line-graph query/info paths correct.
  Error handling and exit codes appropriate.
- **hanoi-bench** (all files): Percentile helper deduplicated. Statistics,
  reporting, comparison, benchmark harnesses, criterion benchmarks all correct.
- **hanoi-tools** (generate_line_graph.rs): Turn-expansion closure correct.
  U-turn detection verified against engine `line_graph()` semantics. Forbidden-
  turn iterator scanning logic sound.
- **Cross-crate consistency**: Perm path convention (`<graph_dir>/perms/cch_perm`)
  consistent across all 4 binaries. Weight count flow (`num_edges` in AppState →
  `/customize` validation → `customize_with()` → `FirstOutGraph`) consistent in
  both normal and line-graph modes.

### Design observations (no fix required)

- Server `handle_query` returns empty `QueryResponse` (200) when engine thread
  dies, rather than 503. Deliberate — clients handle "no path found", and
  `/ready` exists for health checking.
- `biased` keyword in shutdown `tokio::select!` gives timeout priority over serve
  completion if both resolve simultaneously. Correct for force-kill semantics.
- Gateway does not proxy `/health` or `/ready` — intentional per design
  ("query/info only"); orchestration checks backends directly.

### Files changed

- `CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs` — added 5xx propagation to
  `/query`; unified `/info` error type to `Json<Value>`
- `CCH-Hanoi/crates/hanoi-server/src/handlers.rs` — added weight value
  validation rejecting values `>= INFINITY`
- `CCH-Hanoi/crates/hanoi-server/src/main.rs` — moved shutdown timeout broadcast
  subscription before signal handler spawn

---

## 2026-03-18 — Enhance health monitoring and graceful shutdown timeout

### Change

Implemented the enhancements from
`docs/planned/Enhanced Health Monitoring & Graceful Shutdown Timeout.md` in
`hanoi-server`, expanding `/health` telemetry and bounding graceful shutdown
time to 30 seconds.

### Details

- Added startup timestamp and cumulative successful query counting to the
  shared server state.
- Expanded `GET /health` to return uptime, total successful queries processed,
  and `customization_active` alongside `status: "ok"`.
- Incremented the query counter only for successful `/query` responses at the
  handler layer, matching the planned counting boundary.
- Wrapped the query listener graceful shutdown in a 30-second `tokio::select!`
  timeout that logs a warning and calls `std::process::exit(1)` if draining
  hangs.

### Files changed

- `CCH-Hanoi/crates/hanoi-server/src/state.rs` — added startup/query metrics
  state and accessors
- `CCH-Hanoi/crates/hanoi-server/src/types.rs` — expanded `HealthResponse`
- `CCH-Hanoi/crates/hanoi-server/src/handlers.rs` — counted successful queries
  and enriched `/health`
- `CCH-Hanoi/crates/hanoi-server/src/main.rs` — initialized metrics and added
  the planned graceful shutdown timeout

---

## 2026-03-18 — Implement planned CCH-Hanoi fixes and operational hardening

### Change

Implemented the fixes from `docs/planned/Fixes and Design Observations 2026-03-18.md`
across the CCH-Hanoi workspace, covering the confirmed issues plus the planned
server, gateway, CLI, and core hardening work.

### Details

- Deduplicated `hanoi-bench` percentile calculation into a single shared helper
  in `hanoi-bench/src/lib.rs`.
- Updated the gateway to propagate non-2xx `/info` responses and added a
  configurable backend request timeout via `--backend-timeout-secs`.
- Replaced panicking CSR validation asserts in `hanoi-core::GraphData::load`
  with `std::io::ErrorKind::InvalidData` results.
- Added CLI line-graph query/info mode with `--line-graph`, using
  `LineGraphCchContext` and `LineGraphQueryEngine` for offline turn-expanded
  queries.
- Added server engine liveness tracking, `/health` and `/ready` endpoints,
  graceful SIGINT/SIGTERM shutdown for both listeners, and new documentation
  for asynchronous `/customize` semantics and watch-channel behavior.

### Files changed

- `CCH-Hanoi/crates/hanoi-bench/src/lib.rs` — added shared `percentile_sorted`
- `CCH-Hanoi/crates/hanoi-bench/src/report.rs` — removed duplicated percentile helper
- `CCH-Hanoi/crates/hanoi-bench/src/server.rs` — removed duplicated percentile helper
- `CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs` — added `/info` status propagation and timeout-configured client creation
- `CCH-Hanoi/crates/hanoi-gateway/src/main.rs` — added `--backend-timeout-secs`
- `CCH-Hanoi/crates/hanoi-core/src/graph.rs` — converted CSR invariant checks to `Result`-based validation
- `CCH-Hanoi/crates/hanoi-cli/src/main.rs` — added `--line-graph` query/info mode
- `CCH-Hanoi/crates/hanoi-server/src/state.rs` — added engine liveness tracking
- `CCH-Hanoi/crates/hanoi-server/src/engine.rs` — tracked engine exit state
- `CCH-Hanoi/crates/hanoi-server/src/types.rs` — added health/readiness response types
- `CCH-Hanoi/crates/hanoi-server/src/handlers.rs` — added `/health` and `/ready`
- `CCH-Hanoi/crates/hanoi-server/src/main.rs` — added liveness state, routes, and graceful shutdown
- `CCH-Hanoi/crates/hanoi-server/README.md` — **NEW**: documented async `/customize` semantics and mitigation guidance

---

## 2026-03-18 — Add coordinate boundary validation across CCH-Hanoi

### Change

Added centralized coordinate validation so invalid coordinate queries are
rejected early instead of silently snapping to distant graph edges. The feature
now covers `hanoi-core`, the server, CLI, gateway, and benchmark callers.

### Details

- Added `hanoi-core::bounds` with `BoundingBox`, `ValidationConfig`,
  `CoordRejection`, structured JSON error details, and shared
  `validate_coordinate()` logic.
- Extended `SpatialIndex` and `SnapResult` with stored graph bounding boxes,
  snap distance in meters, and `validated_snap()` for combined bounds and
  snap-distance enforcement.
- Updated `QueryEngine` and `LineGraphQueryEngine` to carry validation config,
  expose bbox/config accessors, and return
  `Result<Option<QueryAnswer>, CoordRejection>` from `query_coords()`.
- Updated the server query pipeline to propagate `CoordRejection` through the
  engine channel, return HTTP 400 structured validation errors, and expose
  graph bbox metadata from `/info`.
- Updated the CLI to exit with code 2 on coordinate validation failure, updated
  the gateway query proxy to preserve backend client-error status for invalid
  coordinates, and updated benchmark callers for the new `Result` API.

### Files changed

- `CCH-Hanoi/crates/hanoi-core/Cargo.toml` — added `serde_json = "1"`
- `CCH-Hanoi/crates/hanoi-core/src/bounds.rs` — **NEW**: shared coordinate
  validation types and helpers
- `CCH-Hanoi/crates/hanoi-core/src/lib.rs` — registered `bounds` module and
  re-exported validation types
- `CCH-Hanoi/crates/hanoi-core/src/spatial.rs` — added bbox storage,
  `snap_distance_m`, `bbox()`, and `validated_snap()`
- `CCH-Hanoi/crates/hanoi-core/src/cch.rs` — added validation-configured
  `QueryEngine` and `Result`-returning `query_coords()`
- `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs` — mirrored validation changes
  for `LineGraphQueryEngine`
- `CCH-Hanoi/crates/hanoi-server/src/state.rs` — propagated
  `CoordRejection` through `QueryMsg` and added bbox to `AppState`
- `CCH-Hanoi/crates/hanoi-server/src/types.rs` — added `BboxInfo` and bbox to
  `InfoResponse`
- `CCH-Hanoi/crates/hanoi-server/src/handlers.rs` — returned structured HTTP
  400 validation errors and included bbox in `/info`
- `CCH-Hanoi/crates/hanoi-server/src/engine.rs` — propagated
  `CoordRejection` from both dispatch paths
- `CCH-Hanoi/crates/hanoi-server/src/main.rs` — computed bbox metadata at
  startup and stored it in app state
- `CCH-Hanoi/crates/hanoi-cli/src/main.rs` — handled coordinate validation
  failures with distinct exit code 2
- `CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs` — preserved backend client
  error status for invalid coordinate queries
- `CCH-Hanoi/crates/hanoi-bench/src/core.rs` — updated `query_coords()` bench
  calls for the new `Result` return type
- `CCH-Hanoi/crates/hanoi-bench/benches/cch_bench.rs` — ignored the bench
  `query_coords()` result explicitly to keep all-targets audit warning-free

---

## 2026-03-18 — Comprehensive CCH-Hanoi workspace audit (round 2)

### Change

Full audit of all 6 crates (33 source files) in the CCH-Hanoi workspace,
cross-referenced against upstream `rust_road_router` API signatures, all
documentation in `docs/`, the `scripts/pipeline` script, and actual on-disk
data in `Maps/data/`.

### Findings

**Confirmed issues (3):**

1. **Gateway swallows backend HTTP error codes** — `hanoi-gateway/src/proxy.rs`
   deserializes backend responses without checking status, returning 200 OK even
   when the backend sends 400/500. Breaks error propagation for the planned
   Coordinate Boundary Validation feature.
2. **CLI lacks line-graph query mode** — `hanoi-cli/src/main.rs` only supports
   `CchContext`/`QueryEngine` (normal graph). No `--line-graph` flag, no
   `--original-graph-dir` option, no `LineGraphCchContext` code path. Operators
   cannot test line-graph queries without the server.
3. **Duplicated `percentile_sorted` helper** — identically defined in both
   `hanoi-bench/src/report.rs` and `hanoi-bench/src/server.rs`.

**By-design behaviors documented (2):**

4. Server watch channel silently drops intermediate `/customize` weight vectors
   when multiple requests arrive during an ongoing customization. This is correct
   for live traffic (always want freshest data) but was undocumented.
5. `/customize` returns 200 before customization completes. No completion signal
   exists; `bench_query_after_customize` uses a 100ms heuristic sleep.

**Verified correct (19 checks):**

- All 11 upstream `rust_road_router` API usages match actual signatures
  (`CCH::fix_order_and_build`, `customize`, `customize_directed`,
  `Server::new/update`, `NodeOrder::from_node_order`, `FirstOutGraph::new`,
  `line_graph`, `Vec::load_from`)
- Line graph path mapping (tail sequence + final head, `saturating_add`
  final-edge correction)
- Line graph spatial snapping uses `nearest_node()` not `edge_id` (previous
  bug was correctly fixed)
- Haversine perpendicular distance with degenerate edge guard and clamped `t`
- Turn restriction merge-scan in generate_line_graph (unchanged since 03-12
  audit, still correct)
- Pipeline perm paths consistent with server expectations
  (`<graph_dir>/perms/cch_perm` for both normal and line graph)
- Line graph perm file has correct entry count (1,869,499 = original edge count)
- Server dual-port architecture, engine background thread, GeoJSON coordinate
  reversal

**Design observations (6, not blockers):**

- No coordinate validation yet (comprehensive plan exists in `docs/planned/`)
- No graceful shutdown signal handler on server
- No `/health` or `/ready` endpoint
- No request timeout on gateway proxy `reqwest::Client`
- `GraphData::load` uses panicking `assert_eq!` instead of returning `Err`
- Line graph ordering uses standard node permutation (valid but arc-based cut
  ordering may produce better separator quality)

### Files changed

- `docs/done/Audit Findings 2026-03-18.md` — **NEW**: full audit report with
  detailed analysis, code references, and verification tables

---

## 2026-03-18 — Derive pipeline output directory from map filename

### Change

The `scripts/pipeline` helper no longer hardcodes `hanoi_$PROFILE` for output
data. It now derives the map name from the input `.osm.pbf` filename and writes
to `Maps/data/${MAP_NAME}_$PROFILE` instead.

### Details

- Extracts the basename from the provided `map_source` argument.
- Removes the trailing `.osm.pbf` suffix to get `MAP_NAME`.
- Builds output paths like `Maps/data/hanoi_car` or `Maps/data/hochiminh_motorcycle`
  based on the actual source filename.

### Files changed

- `scripts/pipeline` — derive `OUTPUT_DIR` from the input map filename

---

## 2026-03-18 — Structured logging with `tracing` framework

### Change

Replace all ad-hoc `eprintln!` calls across the CCH-Hanoi workspace with the
`tracing` framework. Adds structured, level-aware logging with configurable
output formats (`--log-format` CLI flag), `RUST_LOG` env-var filtering, optional
daily-rotated JSON file logging (`--log-dir` on the server), `#[instrument]`
spans on key operations, and `TraceLayer` for automatic HTTP request tracing.

### Details

- **Phase 1 — Dependencies**: Added `tracing`, `tracing-subscriber` (with
  `env-filter`, `json`, `ansi` features), `tracing-appender`, `tracing-tree`,
  and `tower-http` (with `trace` feature) across all five crates. Added `clap`
  to `hanoi-tools` to support the `--log-format` flag.
- **Phase 2 — eprintln! → tracing macros**: Replaced all 15+ `eprintln!` calls
  with structured `tracing::info!`, `tracing::warn!`, `tracing::error!`, and
  `tracing::debug!` events with named fields (e.g., `num_nodes`, `num_edges`,
  `distance_ms`, `path_len`).
- **Phase 3 — Spans**: Added `#[tracing::instrument]` to `CchContext::load_and_build`,
  `customize`, `customize_with`, `QueryEngine::query`, `query_coords`,
  `LineGraphCchContext::load_and_build`, `customize`, `customize_with`,
  `SpatialIndex::build`, `snap_to_edge`. Added `TraceLayer::new_for_http()` to
  both hanoi-server routers and the hanoi-gateway router. Added
  `tracing::info_span!("customization")` in engine background loops.
- **Phase 4 — Structured fields**: Added query result metadata logging in
  `dispatch_normal` and `dispatch_line_graph`. Added `tracing::info!` to
  `handle_customize` for request receipt and acceptance. Added `tracing::debug!`
  to `GraphData::load` for disk I/O visibility.
- **Phase 5 — File logging**: hanoi-server supports `--log-dir <PATH>` for
  daily-rotated JSON file output via `tracing-appender`. The `WorkerGuard` is
  held for program lifetime to ensure flush on shutdown.
- **LogFormat enum**: All binaries support `--log-format` with values: `pretty`
  (default, multi-line with source locations), `full` (single-line), `compact`
  (abbreviated), `tree` (hierarchical, server only), `json` (machine-parseable).
  CLI tools and gateway fall back to `full` when `tree` is selected (no
  `tracing-tree` dependency).
- **`generate_line_graph` migrated to clap**: Converted from manual
  `env::args()` parsing to `clap::Parser` to support the `--log-format` flag
  consistently across all binaries.

### Files changed

- `CCH-Hanoi/crates/hanoi-core/Cargo.toml` — added `tracing = "0.1"`
- `CCH-Hanoi/crates/hanoi-core/src/cch.rs` — `#[instrument]` spans, `tracing::info!` for CCH build
- `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs` — `#[instrument]` spans, `tracing::info!` for DirectedCCH build
- `CCH-Hanoi/crates/hanoi-core/src/spatial.rs` — `#[instrument]` on `build` and `snap_to_edge`
- `CCH-Hanoi/crates/hanoi-core/src/graph.rs` — `tracing::debug!` for graph loading
- `CCH-Hanoi/crates/hanoi-server/Cargo.toml` — added tracing crates
- `CCH-Hanoi/crates/hanoi-server/src/main.rs` — `LogFormat` enum, `init_tracing` with file logging, `TraceLayer` on routers
- `CCH-Hanoi/crates/hanoi-server/src/engine.rs` — `tracing::info_span!`, structured query result events
- `CCH-Hanoi/crates/hanoi-server/src/handlers.rs` — `tracing::info!` for customize requests
- `CCH-Hanoi/crates/hanoi-cli/Cargo.toml` — added tracing crates
- `CCH-Hanoi/crates/hanoi-cli/src/main.rs` — `LogFormat` enum, `init_tracing`, all `eprintln!` replaced
- `CCH-Hanoi/crates/hanoi-gateway/Cargo.toml` — added tracing crates and `tower-http`
- `CCH-Hanoi/crates/hanoi-gateway/src/main.rs` — `LogFormat` enum, `init_tracing`, `TraceLayer`, `eprintln!` replaced
- `CCH-Hanoi/crates/hanoi-tools/Cargo.toml` — added `clap`, tracing crates
- `CCH-Hanoi/crates/hanoi-tools/src/bin/generate_line_graph.rs` — migrated to clap, `LogFormat` enum, `init_tracing`, all `eprintln!` replaced

---

## 2026-03-18 — Add performance benchmarking module (hanoi-bench)

### Change

New `hanoi-bench` crate providing a modular, zero-coupling performance
benchmarking and analysis system for the CCH-Hanoi routing stack. Benchmarks
both core CCH logic (build, customize, query) and end-to-end HTTP server
performance (latency, throughput, concurrent load).

### Details

- **Framework types** (`lib.rs`): `Measurement`, `BenchmarkRun`,
  `BenchmarkConfig` with serde support and sensible defaults.
- **Query datasets** (`dataset.rs`): Reproducible query generation from graph
  metadata with JSON save/load for cross-run consistency.
- **Core CCH benchmarks** (`core.rs`): In-process benchmarks for
  `CchContext::load_and_build()`, `customize()`, `QueryEngine::query()`,
  `query_coords()`, and `update_weights()` with warmup/iteration control.
- **Spatial benchmarks** (`spatial.rs`): `SpatialIndex::build()` and
  `snap_to_edge()` benchmarks.
- **Server benchmarks** (`server.rs`): HTTP benchmarks for `POST /query`
  (sequential + concurrent with semaphore-based load control), `GET /info`,
  `POST /customize` upload, and post-customization query latency.
- **Statistical reporting** (`report.rs`): Computes min/max/mean/p50/p95/p99/
  std_dev/throughput/success_rate. Outputs as human-readable table, JSON (for
  CI regression tracking), or CSV. Comparison mode with configurable regression
  threshold and exit code 1 on regression.
- **Criterion harnesses** (`benches/`): `cch_bench.rs` for query and customize
  micro-benchmarks; `spatial_bench.rs` for KD-tree build and snap_to_edge.
- **CLI runners**: `bench_core` (all in-process benchmarks), `bench_server`
  (HTTP benchmarks against running server), `bench_report` (analysis and
  comparison from saved JSON results).
- **Zero coupling**: `hanoi-bench` depends on `hanoi-core` and
  `rust_road_router` but nothing depends on `hanoi-bench`. Can be excluded
  or deleted with zero impact on the workspace.

### Files changed

- `CCH-Hanoi/crates/hanoi-bench/Cargo.toml` — new crate manifest
- `CCH-Hanoi/crates/hanoi-bench/src/lib.rs` — framework types and module declarations
- `CCH-Hanoi/crates/hanoi-bench/src/dataset.rs` — query dataset generation and I/O
- `CCH-Hanoi/crates/hanoi-bench/src/core.rs` — core CCH benchmark functions
- `CCH-Hanoi/crates/hanoi-bench/src/spatial.rs` — spatial indexing benchmark functions
- `CCH-Hanoi/crates/hanoi-bench/src/server.rs` — HTTP server benchmark functions
- `CCH-Hanoi/crates/hanoi-bench/src/report.rs` — statistical analysis and report generation
- `CCH-Hanoi/crates/hanoi-bench/src/bin/bench_core.rs` — standalone core benchmark CLI
- `CCH-Hanoi/crates/hanoi-bench/src/bin/bench_server.rs` — standalone server benchmark CLI
- `CCH-Hanoi/crates/hanoi-bench/src/bin/bench_report.rs` — analysis and comparison CLI
- `CCH-Hanoi/crates/hanoi-bench/benches/cch_bench.rs` — Criterion CCH micro-benchmarks
- `CCH-Hanoi/crates/hanoi-bench/benches/spatial_bench.rs` — Criterion spatial micro-benchmarks

---

## 2026-03-18 — Add GeoJSON response format to /query endpoint

### Change

The `/query` endpoint now accepts an optional `format` request parameter.
When `format` is `"geojson"`, the response is a GeoJSON Feature with a
LineString geometry (RFC 7946) instead of the standard JSON response.

### Request example

```json
{
  "from_lat": 21.028, "from_lng": 105.834,
  "to_lat": 21.006, "to_lng": 105.843,
  "format": "geojson"
}
```

### GeoJSON response example

```json
{
  "type": "Feature",
  "geometry": {
    "type": "LineString",
    "coordinates": [[105.834, 21.028], [105.836, 21.025], ...]
  },
  "properties": {
    "distance_ms": 12345,
    "distance_m": 3842.7
  }
}
```

### Details

- Coordinates in GeoJSON follow RFC 7946: `[longitude, latitude]` (reversed
  from our internal `(lat, lng)` convention). The standard format is unchanged.
- When no path is found, returns `"geometry": null` (valid per RFC 7946 §3.2).
- `format` field defaults to standard response when omitted or set to any
  value other than `"geojson"` — fully backward-compatible.
- Gateway forwards the `format` field to the backend server transparently.
- Handler return type changed from `Json<QueryResponse>` to `Json<Value>` to
  accommodate both response shapes.
- Channel type changed from `oneshot::Sender<QueryResponse>` to
  `oneshot::Sender<Value>`.

### Files changed

- `CCH-Hanoi/crates/hanoi-server/src/types.rs` — added `format` to `QueryRequest`
- `CCH-Hanoi/crates/hanoi-server/src/state.rs` — channel type `QueryResponse` → `Value`
- `CCH-Hanoi/crates/hanoi-server/src/handlers.rs` — handler returns `Json<Value>`
- `CCH-Hanoi/crates/hanoi-server/src/engine.rs` — added `format_response()`,
  `answer_to_geojson()`; dispatch functions return `Value`
- `CCH-Hanoi/crates/hanoi-gateway/src/types.rs` — added `format` to gateway types
- `CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs` — forwards `format` to backend

---

## 2026-03-18 — Add distance_m (route distance in meters) to query results

### Change

Query responses now include `distance_m`: the total geographic route distance
in meters, computed as the Haversine sum of consecutive coordinate pairs.

### Details

- Added `distance_m: f64` field to `QueryAnswer` (hanoi-core).
- Added `distance_m: Option<f64>` field to `QueryResponse` (hanoi-server).
- Added `route_distance_m()` helper in `cch.rs` — sums Haversine distances
  over `coordinates.windows(2)`.
- Made `haversine_m()` in `spatial.rs` public so it can be called from
  `cch.rs` and `line_graph.rs`.
- `distance_m` is computed in `query()` from intersection coordinates, then
  **recomputed** in `patch_coordinates()` to include the user origin/destination
  segments.
- CLI and gateway also output `distance_m`.

### Files changed

- `CCH-Hanoi/crates/hanoi-core/src/cch.rs`
- `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`
- `CCH-Hanoi/crates/hanoi-core/src/spatial.rs`
- `CCH-Hanoi/crates/hanoi-server/src/types.rs`
- `CCH-Hanoi/crates/hanoi-server/src/engine.rs`
- `CCH-Hanoi/crates/hanoi-cli/src/main.rs`

---

## 2026-03-18 — Patch user origin/destination coordinates into query result path

### Change

For coordinate-based queries (`query_coords`), the result's `coordinates`
array now includes the user's original query coordinates: prepended at the
start and appended at the end. This gives map visualization a complete
polyline from the user's actual position to the destination, rather than
starting/ending at the nearest snapped intersection.

### Details

- Added `patch_coordinates()` to both `QueryEngine` (normal graph) and
  `LineGraphQueryEngine` (line graph).
- Applied in both the primary query path and the fallback path.
- Only affects `coordinates`, not `path_nodes` (user positions are not graph
  nodes). `coordinates.len() == path_nodes.len() + 2` for coordinate queries.
- Node-ID queries (`from_node`/`to_node`) are unaffected.

### Files changed

- `CCH-Hanoi/crates/hanoi-core/src/cch.rs`
- `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`

---

## 2026-03-17 — Map line graph query results to original intersection node IDs

### Problem

`LineGraphQueryEngine::query()` returned raw line-graph node IDs in `path`
(= original edge indices), but API consumers expect intersection node IDs.
The `path_nodes` and `coordinates` arrays also had mismatched lengths
(`n` vs `n+1`).

### Changes

- **`LineGraphCchContext`**: Added `original_tail: Vec<NodeId>` field,
  reconstructed from the original graph's `first_out` at load time. Also
  loads `original_first_out` during `load_and_build()`.
- **`query()`**: Maps CCH path from line-graph node IDs to original
  intersection IDs via `original_tail[lg_node]`, appends
  `original_head[last_edge]` for the destination. Coordinates now derived
  from the same mapped intersection path. Both arrays have length `n+1`.
- API output is now identical in structure for both normal and line graph
  queries: `path_nodes` = intersection IDs, `coordinates` = aligned positions.

### Files changed

- `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`

### Docs updated

- `docs/walkthrough/Line Graph Spatial Indexing and Snapping.md` — added
  Section 6 (Path Result Mapping) covering `original_tail` reconstruction,
  mapping algorithm, and API consistency.

---

## 2026-03-17 — Fix line graph query_coords: edge_id vs node_id semantic bug

### Bug

`LineGraphQueryEngine::query_coords()` passed `snap.edge_id` (a line-graph CSR
edge index = a turn) to `query()`, which expected line-graph **node** IDs
(= original edge indices). A CSR edge index and a node ID are unrelated
entities — this could produce wrong routes or out-of-bounds panics.

The normal graph's `query_coords()` was already correct (used
`snap.nearest_node()` → actual node IDs).

### Fix

- **`query_coords()`**: Now uses `src_snap.nearest_node()` / `dst_snap.nearest_node()`
  (returns `tail` or `head` based on projection `t`), matching the normal graph
  pattern.
- **`collect_candidate_edges_prioritized`** → renamed to
  **`collect_candidate_nodes_prioritized`**: Returns line-graph **node** IDs
  (both endpoints of the snapped edge + their outgoing neighbors) instead of
  CSR edge indices.

### Files changed

- `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`

### New docs

- `docs/walkthrough/Line Graph Spatial Indexing and Snapping.md` — full
  walkthrough of how coordinate snapping works on line graphs, the node/edge
  semantic inversion, and how the bug arose.

---

## 2026-03-17 — Implement CCH Customization & Query (full plan execution)

Implemented the complete CCH Customization & Query system as specified in
`docs/planned/CCH Customization And Query.md`. This adds 6 new crates to the
CCH-Hanoi workspace covering graph loading, CCH routing, HTTP server, CLI, and
API gateway.

### New crates

- **hanoi-core** — Reusable library API for CCH loading, customization, query
  - `graph.rs`: `GraphData` struct — loads RoutingKit binary format (CSR),
    validates invariants, provides zero-copy `BorrowedGraph` views
  - `spatial.rs`: `SpatialIndex` with `kiddo::ImmutableKdTree<f32, 2>` — hybrid
    snap-to-edge (KD-tree on nodes + Haversine perpendicular distance to edges).
    Returns `SnapResult` with projection parameter `t` for nearest-endpoint
    selection
  - `cch.rs`: `CchContext` (owns graph + CCH topology, metric-independent) and
    `QueryEngine` (borrows context, handles queries + re-customization).
    `query_coords()` uses `t`-based nearest endpoint with fallback to all 4
    endpoint combinations
  - `line_graph.rs`: `LineGraphCchContext` (uses `DirectedCCH` for pruned
    turn-expanded graphs) and `LineGraphQueryEngine` (final-edge correction,
    coordinate mapping with original graph head/lat/lng, t-prioritized
    candidate edge collection)
  - Dependencies: `kiddo 5`, `rayon 1`

- **hanoi-server** — Modular dual-port Axum HTTP server
  - `types.rs`: Request/response types (QueryRequest, QueryResponse,
    CustomizeResponse, InfoResponse)
  - `state.rs`: `AppState` with mpsc query channel, watch customization channel,
    AtomicBool for customization status
  - `handlers.rs`: `handle_query` (POST /query, JSON), `handle_customize`
    (POST /customize, binary body with alignment-safe copy), `handle_info`
    (GET /info)
  - `engine.rs`: Background loops for normal-graph and line-graph engines —
    processes queries via mpsc, applies weight updates via watch channel with
    stale-update cancellation
  - `main.rs`: CLI args (--graph-dir, --query-port, --customize-port,
    --line-graph, --original-graph-dir), dual Axum listener setup with
    `RequestDecompressionLayer` for gzip, `DefaultBodyLimit::max(64MB)`
  - Dependencies: `axum 0.8`, `tokio 1`, `tower-http 0.6`, `clap 4`,
    `bytemuck 1`, `serde 1`

- **hanoi-gateway** — API gateway (query/info only, T-shape architecture)
  - `types.rs`: `GatewayQueryRequest` (adds `graph_type` field for routing),
    `BackendQueryRequest`, `InfoQuery`
  - `proxy.rs`: `GatewayState` with reqwest client, routes queries to normal
    or line_graph backend by `graph_type` field
  - `main.rs`: CLI args (--port, --normal-backend, --line-graph-backend)
  - Does NOT proxy /customize (direct pipeline→server path)
  - Dependencies: `axum 0.8`, `reqwest 0.12`, `clap 4`, `serde 1`

### Updated crates

- **hanoi-cli** — Operator-facing CLI (no server required)
  - `query` subcommand: loads graph from disk, builds CCH, customizes with
    baseline travel_time, runs query, outputs JSON
  - `info` subcommand: displays graph metadata (num_nodes, num_edges)
  - Only imports `hanoi-core` (never `rust_road_router` directly)
  - Added deps: `clap 4`, `serde 1`, `serde_json 1`

### Audit findings and fixes

1. **bytemuck alignment** (`handlers.rs`): `bytemuck::cast_slice(&body)` requires
   4-byte alignment which `axum::body::Bytes` doesn't guarantee. Fixed by
   copying into aligned `Vec<u32>` via `cast_slice_mut` + `copy_from_slice`.

2. **Snap-to-edge `t` parameter** (`spatial.rs`): Extended `SnapResult` with
   projection parameter `t` (0.0 = at tail, 1.0 = at head). `cch.rs` uses `t`
   for primary nearest-endpoint query with fallback. `line_graph.rs` uses `t`
   to prioritize candidate edges from the nearer intersection.

3. **Haversine distance** (`spatial.rs`): Uses Haversine formula for geographic
   accuracy instead of Euclidean approximation. Equirectangular local projection
   for the perpendicular projection step, then Haversine for final distance.

4. **Line graph `original_travel_time` staleness**: Known design limitation —
   final-edge correction uses baseline `original_travel_time` from disk. If
   live traffic updates original graph weights, the correction may be stale.
   Documented; requires both servers to be updated in sync.

### Architecture notes

- Workspace `Cargo.toml` uses `members = ["crates/*"]` glob — new crates
  auto-discovered
- `hanoi-tools/Cargo.toml` unchanged (keeps `generate_line_graph` independent)
- Server binary: `hanoi_server` (in `hanoi-server` crate)
- Gateway binary: `hanoi_gateway` (in `hanoi-gateway` crate)
- CLI binary: `cch-hanoi` (in `hanoi-cli` crate)
- All compile clean (zero warnings, zero errors) in release mode

## 2026-03-17 — CCH plan: add gzip decompression for customize endpoint

Added `RequestDecompressionLayer` from `tower-http` to the customize port's
Axum router. The pipeline client can optionally compress the ~8 MB weight
vector with gzip (~2-3 MB on wire) by setting `Content-Encoding: gzip`.
Server decompresses transparently before the handler. Gzip is optional —
uncompressed requests still work. Updated:
- **Section 4.5.1**: Added `decompression` to `tower-http` purpose
- **Section 4.5.3**: Added `RequestDecompressionLayer` to customize router setup
- **Section 5.3**: Added `decompression-gzip` feature to `tower-http` in Cargo.toml
- **Section 9**: Added gzip mechanism explanation with client/server flow diagram

## 2026-03-17 — CCH plan: drop gRPC, use REST with binary body everywhere

Replaced gRPC (`tonic`/`prost`/`protoc`) with plain REST binary body for
customization. Architecture clarification: the API gateway only handles external
query/info traffic; customization is a direct internal path from the data
processing pipeline to each server's customize port (T-shape architecture).

Key changes across `docs/planned/CCH Customization And Query.md`:
- **Section 3.1**: `[DECIDED]` updated — REST `POST /customize` with
  `application/octet-stream` body replaces gRPC `RoutingCustomizer` service
- **Section 4.5.1**: Removed `tonic`, `prost`, `tonic-build` from deps;
  retained `bytemuck` for zero-copy deserialization
- **Section 4.5.2**: `--grpc-port` → `--customize-port`; port table updated
  from "gRPC" to "REST, binary"
- **Section 4.5.3**: Replaced protobuf service definition + tonic handler with
  Axum handler (`async fn handle_customize(body: Bytes)`); added dual-port Axum
  setup snippet; removed proto file reference
- **Section 4.5.4**: Concurrency diagram: Tonic → Axum on customize port
- **Section 4.5.5**: gRPC references → REST/HTTP references
- **Section 4.5.6**: Gateway is now query/info only (no `/customize` proxy);
  customization goes directly from pipeline → server customize port; removed
  all gRPC client references from gateway
- **Section 4.5.7**: Simplified to "REST everywhere"; gRPC noted as future
  option if structured RPC contracts become valuable
- **Section 5.3**: Removed `tonic`, `prost`, `tonic-build` from Cargo.toml
- **Section 6**: Removed checklist items 9a (proto file) and 9b (build.rs);
  updated items 9 and 10 to reflect Axum-only stack
- **Section 9**: Weight update flow diagram updated (gRPC → REST binary)
- **Section 10**: Replaced 3 gRPC risks with 2 REST-specific risks (body
  size limit, endianness); removed protoc build dependency risk
- **Section 11**: Design decisions #6 and #10 updated to reflect REST-only
  protocol choice

Rationale: single unary call with a binary blob doesn't warrant gRPC tooling.
Plain HTTP POST achieves identical wire efficiency (~8 MB). Same Axum framework
on both ports — fewer dependencies, simpler build, easier debugging (curl).

## 2026-03-17 — CCH plan: simplify to unary-only gRPC (remove streaming variant)

Removed `CustomizeStream` streaming RPC and `WeightChunk` message from protobuf
definition. Unary `Customize` RPC with raised max message size (16 MB) is
sufficient for ~8 MB weight vectors. Simpler: one RPC, one handler, no
reassembly logic. Updated risk table to reflect max message size config on both
client and server. Added server setup snippet with `max_decoding_message_size`.

## 2026-03-17 — CCH plan: gRPC customization, full-vector binary, separate ports, gRPC gateway

Updated `docs/planned/CCH Customization And Query.md` — replaced REST
`POST /customize` with gRPC `RoutingCustomizer` service:
- **Section 3.1**: Updated `[DECIDED]` marker — gRPC binary weight vector
  replaces JSON per-edge objects
- **Section 4.5.1**: Promoted `tonic 0.14` to required; added `prost 0.13`,
  `tonic-build 0.14`, `bytemuck 1` to tech stack
- **Section 4.5.2**: Each server now binds two ports — query (REST/Axum) and
  customize (gRPC/tonic). Port table: normal (8080/9080), line graph (8081/9081)
- **Section 4.5.3**: Full protobuf service definition for `RoutingCustomizer`
  (`Customize` + `CustomizeStream` RPCs); tonic handler with `bytemuck::cast_slice`
  zero-copy deserialization; moved `/customize` out of REST endpoint table
- **Section 4.5.4**: Updated concurrency diagram — shows two ports converging
  on background thread via `mpsc` (queries) and `watch` (weights)
- **Section 4.5.5**: Updated stale-update cancellation to reference gRPC
- **Section 4.5.6**: Gateway changed from REST proxy (Axum) to gRPC gateway
  (tonic); full `RoutingGateway` protobuf definition with `graph_type`-based
  routing to backend servers
- **Section 4.5.7**: Updated future integration to reflect dual REST/gRPC access
- **Section 5.3**: `hanoi-tools/Cargo.toml` updated with `tonic`, `prost`,
  `bytemuck` deps + `tonic-build` build-dep
- **Section 6**: Added checklist items 9a (proto file) and 9b (build.rs);
  updated items 9, 10 with gRPC-specific requirements
- **Section 9**: Complete rewrite — full-vector replacement flow via gRPC,
  eliminated clone-and-merge step, added wire cost analysis (8 MB raw, ~2–3 MB
  gzipped for 2M edges)
- **Section 10**: Added 3 risk entries (protobuf message size limit, endianness,
  protoc build dependency)
- **Section 11**: Added decision #10 (customization protocol); updated #6
  (protocol split: REST for queries, gRPC for internal)

## 2026-03-17 — CCH plan: Axum server, crate research, API gateway, response format

Updated `docs/planned/CCH Customization And Query.md` with technology decisions
based on crate research:
- **Section 4.2.4**: Replaced `fux_kdtree ^0.2.0` (abandoned, 2017) with
  `kiddo 5.x` (631K downloads/month, `ImmutableKdTree`, SIMD, active)
- **Section 4.5**: Complete rewrite — now **Axum 0.8** with full technology
  stack table (axum, tokio, tower, tower-http, serde, clap, tonic)
- **Section 4.5.3**: Detailed request/response format for all 3 endpoints
  (`/query`, `/customize`, `/info`) with field-by-field documentation
- **Section 4.5.4**: `crossbeam_utils::thread::scope` replaced with
  `std::thread::scope` (crossbeam is soft-deprecated since Rust 1.63)
- **Section 4.5.5**: `tokio::sync::watch` for stale-update cancellation
- **Section 4.5.6**: New API gateway design — path-based routing to both
  servers, REST+gRPC multiplexing via tonic 0.14 `axum` feature flag
- **Section 5.3**: Updated all Cargo.toml dependency lists for hanoi-core,
  hanoi-cli, and hanoi-tools
- **Section 6**: Updated checklist items 5, 6, 9, 10 with specific crate names
- **Section 10**: Updated KD-tree risk (migration, not health)
- **Section 11**: Resolved decisions #5 (Axum), #8 (kiddo), #9 (std::thread)

## 2026-03-17 — Major update to CCH Customization & Query plan (design decisions)

Applied owner decisions and new sections to
`docs/planned/CCH Customization And Query.md`:
- **Sections 3.1/3.2/3.5**: Marked with `[DECIDED]` — build new custom server
  wrapping `hanoi-core`, replacing HERE-coupled endpoints
- **Section 3.3**: Marked with `[DECIDED]` — two separate server processes
  (normal graph + line graph)
- **Section 4.2.2**: Confirmed Option B (separate structs, explicit lifetimes)
- **Section 4.2.3**: Replaced `query_line_graph()` method with full
  `LineGraphCchContext` + `LineGraphQueryEngine` types (Option C), including
  original graph metadata for coordinate mapping
- **Section 4.4**: DirectedCCH confirmed as mandatory default for line graphs;
  Option C confirmed for type separation
- **NEW Section 4.5**: Full server design — two-process architecture, API
  endpoints (`/query`, `/customize`, `/info`), concurrency pattern, background
  customization with stale-update cancellation for live traffic, HTTP framework
  choice (deferred), future app integration notes
- **Section 5.3**: All dependencies listed explicitly (`fux_kdtree`, `rayon`,
  `clap`, `serde`, `serde_json`)
- **Section 6**: Expanded checklist to 4 phases (A: Core, B: CLI+Server,
  C: Line graph, D: Visualization); graph-loading duplication confirmed as
  Option A
- **Section 9**: Replacing updates confirmed for live traffic; integration
  pattern with external traffic module documented
- **Section 10**: Added 4 new risks (line-graph coordinate mapping, live traffic
  update frequency, coordinate system confusion, road geometry fidelity)
- **Section 11**: All 4 original decisions resolved; 3 new deferred decisions
  added (HTTP framework, app protocol, road geometry)
- **NEW Section 13**: Path visualization and coordinate mapping — normal graph
  coordinate extraction, line graph edge-to-coordinate translation, coordinate
  system boundary rule (`hanoi-core` returns `(lat,lng)`, server flips for
  GeoJSON), road-following geometry options (Mapbox Map Matching vs OSM way
  storage)
- `QueryAnswer` struct now includes `coordinates: Vec<(f32, f32)>` field

## 2026-03-17 — Add Rust fundamentals walkthrough for experienced programmers

Created `docs/walkthrough/Rust Fundamentals for Experienced Programmers.md` as a
large onboarding/reference guide for learning Rust in the context of this
repository. Covers:
- Rust's design goals and tradeoffs relative to Kotlin, Python, Java, C, and
  C++
- syntax, expressions, mutability, types, structs, enums, `Option`, and
  `Result`
- ownership, moves, borrowing, lifetimes, strings, collections, and standard
  pointer/container types
- traits, generics, static vs dynamic dispatch, methods, modules, macros, and
  iterators/closures
- error handling, concurrency, async, unsafe Rust, and performance model
- idiomatic Rust design advice and common beginner failure modes
- a suggested staged learning path and repo-specific next reading order

Also added `socs/walkthrough/Rust Fundamentals for Experienced Programmers.md`
as a lightweight mirror entry point that forwards readers to the canonical
`docs/walkthrough` copy.

## 2026-03-17 — Add rust_road_router algorithm families walkthrough

Created `docs/walkthrough/rust_road_router Algorithm Families.md` to map the
workspace and answer which algorithm families exist beyond the core CH/CCH
tooling. Covers:
- current workspace members and note that `rust_road_router/README.md` is stale
  relative to `Cargo.toml`
- non-CH/CCH engine modules: Dijkstra, A*, ALT, Hub Labels, TopoCore
- CH-derived but distinct modules: RPHAST, CATCHUp, TD A*, TD-S, traffic-aware
  routing, metric merging, and path-repair utilities
- role of the `catchup`, `chpot`, `tdpot`, and `cchpp` crates as mostly binary
  experiment/workbench crates rather than separate reusable library toolkits
- recommended reading order based on whether the goal is baseline routing,
  heuristic acceleration, time-dependent routing, or experiment entry points

## 2026-03-17 — Extract engine API reference walkthrough

Copied Section 2 of `docs/planned/CCH Customization And Query.md` into a
standalone walkthrough at
`docs/walkthrough/rust_road_router Engine API Reference.md`. Covers:
- Key types (Weight, NodeId, FirstOutGraph, NodeOrder, Query, QueryResult)
- Phase 1 (Contraction): `CCH::fix_order_and_build()`, CCH struct contents
- Phase 2 (Customization): `customize()`, prepare_weights, customize_basic,
  CustomizedBasic struct
- Phase 3 (Query): `Server::query()`, elimination tree walk, path unpacking,
  result API
- DirectedCCH variant for line graphs with type system implications
- Re-customization pattern with baseline cloning
- Data loading via `Load` trait and `NodeOrder` construction
- Full working examples for both normal graph and line graph with DirectedCCH

## 2026-03-17 — Audit CCH Customization & Query plan

Audited `docs/planned/CCH Customization And Query.md` against actual codebase.
All 22 engine API references verified correct. Amendments applied inline with
`[AUDIT]` markers:
- Fixed 2 factual errors: corrected `flow_cutter_cch_order.sh` path, added
  `line_graph/perms/cch_perm` to directory tree
- Added 4 design-gap notes: `DirectedCCH`/`CCH` type parameter constraint,
  `FirstOutGraph` construction step, `update_weights` lifetime safety,
  KD-tree crate naming (`fux_kdtree`)
- Added 2 conformance notes with CCH-Hanoi Hub walkthrough: CLI must only call
  `hanoi-core` APIs, graph-loading duplication strategy
- Expanded risk table with 3 new entries
- Resolved Open Decision #1 (already decided), added guidance on #2
- Added Section 12 (implementation notes)

## 2026-03-17 — Make pipeline runtime output omit zero units

Refined `scripts/pipeline` duration formatting so zero-value units are hidden:
- `65s` now prints as `1m 5s` (not `00h:01m:05s`)
- `3600s` now prints as `1h` (not `01h:00m:00s`)
- `0s` remains `0s` to avoid empty output

## 2026-03-17 — Add total runtime monitoring to pipeline script

Updated `scripts/pipeline` to report end-to-end elapsed runtime for every execution:
- Added `format_duration()` helper for compact `h/m/s` output.
- Added `on_exit()` + `trap 'on_exit $?' EXIT` so runtime is printed on both success and failure.
- Replaced the fixed success line with runtime-aware completion output.

## 2026-03-17 — Expand CSR section in Graph Weight Format Guide

Expanded Section 2 (CSR Structure) in `docs/walkthrough/Graph Weight Format and Test Weight Generation Guide.md` from a brief overview to 7 detailed subsections: problem CSR solves, the three arrays, how lookups work, concrete walkthrough with annotated diagrams, key invariants, reverse lookup problem and `tail` array, and why CSR is the right choice for routing.

## 2026-03-17 — Add CCH Customization & Query implementation plan

Created `docs/planned/cch-customization-and-query.md` — comprehensive plan for integrating CCH phases 2 (customization) and 3 (query) into the `CCH-Hanoi` workspace. Covers:
- Full analysis of `rust_road_router` engine API: types, contraction, customization (respecting + triangle relaxation), query (elimination tree walk), path unpacking
- Problems with existing HTTP server (HERE-only data model, single graph, no line-graph correction)
- Proposed `hanoi-core` library API: `GraphData`, `CchContext`, `QueryEngine`, `SpatialIndex`
- Line graph support via `DirectedCCH` (optimized for turn-expanded graphs)
- Self-referential struct problem analysis with recommended solution (separate structs)
- Weight update flow (clone-modify-customize-swap, non-cumulative)
- Data flow diagrams for both normal and line graph paths
- Prerequisites: line graph needs its own `cch_perm` from IFC
- File-level implementation checklist (10 items across 3 phases)

## 2026-03-16 — Add Graph Weight Format & Test Weight Generation Guide

Created `docs/walkthrough/Graph Weight Format and Test Weight Generation Guide.md` — comprehensive reference for the RoutingKit binary graph format, CCH customization weight flow, and how to generate fixed test weights for both normal and turn-expanded (line) graphs. Covers:
- Binary file format: headerless raw `u32` vectors for `first_out`, `head`, `travel_time`
- CSR (Compressed Sparse Row) structure with worked examples
- Weight unit convention: milliseconds (`tt_units_per_s = 1000`)
- Normal graph: positional 1-to-1 edge-to-weight mapping
- Line graph: node↔edge identity mapping, turn creation rules (consecutive + not forbidden + not U-turn), weight formula (`travel_time[e1] + turn_cost`, where turn_cost = 0)
- Path cost correctness proof for the line graph
- CCH customization weight flow: two-phase architecture (metric-independent structure vs metric-dependent weights), `INFINITY = u32::MAX / 2` initialization, `prepare_weights` respecting phase with `min()` for parallel edges, bottom-up shortcut triangle enumeration
- Server `/customize` endpoint: clone-modify-customize-swap pattern, non-cumulative updates, unspecified edges retain OSM defaults, atomic swap for query consistency
- Test weight generation strategies: uniform, sequential, distance-based, random, known-path
- Rust and Python code examples for reading/writing binary vectors
- Validation checklist and recommended "known shortest path" test scenario
- Overflow safety guidelines for CH/CCH shortcut weights

## 2026-03-13 — Add IFC Ordering Effectiveness Analysis walkthrough

Created `docs/walkthrough/IFC Ordering Effectiveness Analysis.md` — comprehensive empirical analysis of IFC nested dissection orderings on the Hanoi road network, serving as proof of algorithmic effectiveness. Covers:
- Graph topology characterization (both car and motorcycle profiles)
- Degree distribution analysis: 88% junction nodes, 12% chain nodes — radically different from typical Western road networks
- Urban morphology explanation: Hanoi's dense alley mesh (80%+ service/residential roads) creates a uniform mesh with no natural road hierarchy
- Separator hierarchy quality: removal-threshold analysis at 0.1%–10% showing topology-bounded fragmentation
- Arc ordering comparison: `cch_perm_cuts` vs `cch_perm_cuts_reorder` (91% identical, no meaningful quality difference)
- Runtime performance: ~3.1 seconds on 28 threads for ~930K nodes (musec = microseconds)
- Cross-profile consistency: car and motorcycle profiles produce nearly identical structural metrics
- Recommendations: `normal` arc ordering sufficient; ordering is production-ready

## 2026-03-13 — Add `mkdir -p` for `perms/` directory in IFC scripts

All three IFC scripts now save output to `${GRAPH_DIR}/perms/` subdirectories. Added `mkdir -p "${GRAPH_DIR}/perms"` before invoking the console binary, since IFC's `save_vector` (via `std::ofstream`) does not create parent directories and would fail with `"Can not open ... for writing"`.

### Changes
- `flow_cutter_cch_order.sh`: added `mkdir -p "${GRAPH_DIR}/perms"`
- `flow_cutter_cch_cut_order.sh`: added `mkdir -p "${GRAPH_DIR}/perms"`
- `flow_cutter_cch_cut_reorder.sh`: added `mkdir -p "${GRAPH_DIR}/perms"`
- Updated all `cch_perm` path references in Manual Pipeline Guide to use `perms/` subdirectory

## 2026-03-13 — Add IFC Scripts Reference walkthrough

Created `docs/walkthrough/IFC Scripts Reference.md` — detailed reference for all three IFC wrapper scripts covering:
- Script usage, arguments (`$1` input dir, `$2` thread count), and smart directory resolution
- Script 1 (`flow_cutter_cch_order.sh`): node permutation, graph preprocessing, quality metrics
- Script 2 (`flow_cutter_cch_cut_order.sh`): arc permutation via internal line graph construction (`normal` mode)
- Script 3 (`flow_cutter_cch_cut_reorder.sh`): arc permutation with extra reordering pass (`reorder` mode)
- Permutation format and semantics (`perm[rank] = original_id`)
- Output log interpretation (config dump, timing, chordal supergraph metrics)
- Decision matrix for choosing which script to use
- Source-level references to `console.cpp` for internal behavior

## 2026-03-13 — Fix `thread_count` bug in IFC wrapper scripts

All three IFC scripts (`flow_cutter_cch_order.sh`, `flow_cutter_cch_cut_order.sh`, `flow_cutter_cch_cut_reorder.sh`) defaulted `thread_count` to `-1` via `${2:--1}`, but IFC's `flow_cutter_config` validates `thread_count >= 1` — causing a runtime crash when no second argument was provided. Fixed default to `$(nproc)`.

### Changes
- `flow_cutter_cch_order.sh`: `${2:--1}` → `${2:-$(nproc)}`
- `flow_cutter_cch_cut_order.sh`: `${2:--1}` → `${2:-$(nproc)}`
- `flow_cutter_cch_cut_reorder.sh`: `${2:--1}` → `${2:-$(nproc)}`
- Updated Manual Pipeline Guide to reflect the fix and document the `>= 1` constraint

## 2026-03-13 — Update IFC script documentation in Manual Pipeline Guide

Updated Phases 7, 8, and 10.5 in `docs/walkthrough/Manual Pipeline Guide.md` to match the actual IFC wrapper scripts.

### Changes
- Fixed `thread_count` in manual invocations and docs: `${2:-$(nproc)}` (accepts script's 2nd positional arg, defaults to all cores) instead of hardcoded `-1`
- Documented smart directory resolution: all IFC scripts auto-resolve `$1/graph/` if `$1/first_out` doesn't exist
- Added note about passing thread count as 2nd script argument
- Added full manual invocation for `flow_cutter_cch_cut_order.sh` (was previously missing — only the wrapper command was shown)
- Listed key differences between cut-order and node-order scripts (no preprocessing, arc vs node permutation, no chordal supergraph)
- Clarified `normal` mode argument in `reorder_arcs_in_accelerated_flow_cutter_cch_order normal`
- Added note about line graph directory structure for `flow_cutter_cch_order.sh`

## 2026-03-13 — Make `compare_profiles` comparison-only with graph mode selection

Simplified `CCH-Generator/scripts/compare_profiles` so it no longer generates graphs or extracts conditional turns.

### Changes
- Updated script usage from:
  - `<input.osm.pbf> <output_root_dir>`
  - to `<output_root_dir> <mode>`
- Added explicit mode selection:
  - `normal` (alias: `graph`) compares `hanoi_{car,motorcycle}/graph`
  - `line_graph` (alias: `line`) compares `hanoi_{car,motorcycle}/line_graph`
- Removed all generation dependencies and steps:
  - no `cch_generator` invocation
  - no `conditional_turn_extract` invocation
- Added directory existence checks for both profiles and selected graph mode directories.
- Kept summary metrics for both modes:
  - always: node/arc counts and travel time min/max/mean
  - `normal` only: forbidden turn count and conditional turn count

## 2026-03-13 — Store `conditional_turns/` at profile root (implemented)

Adjusted the profile layout so conditional turn outputs live beside `graph/` instead of inside it:

- target layout is now `Maps/data/hanoi_<profile>/conditional_turns/*`
- base graph vectors remain in `Maps/data/hanoi_<profile>/graph/*`
- line graph remains in `Maps/data/hanoi_<profile>/line_graph/*`

### Changes
- Updated pipeline scripts to pass explicit output roots to `conditional_turn_extract`:
  - `scripts/pipeline`
  - `CCH-Generator/scripts/run_pipeline`
  - `CCH-Generator/scripts/compare_profiles`
- Updated comparison script conditional-turn read path:
  - `CCH-Generator/scripts/compare_profiles` now reads conditional files from `<profile_dir>/conditional_turns/*` while graph stats still read from `<profile_dir>/graph/*`
- Updated validator conditional-turn lookup compatibility:
  - `CCH-Generator/src/validate_graph.cpp` now checks:
    - `<graph_dir>/conditional_turns/*` (legacy/flat use), then
    - `<graph_dir>/../conditional_turns/*` when `graph_dir` is named `graph`
- Updated manual walkthrough commands and examples:
  - `docs/walkthrough/Manual Pipeline Guide.md`

### Signature audit
- Verified extractor invocation matches current RoutingKit CLI signature:
  - `conditional_turn_extract <pbf_file> <graph_dir> [<output_dir>] [--profile car|motorcycle]`
- Verified validator CLI signature remains unchanged:
  - `validate_graph <graph_dir> [--turn-expanded <line_graph_dir>] ...`
- Verified script call sites pass positional arguments in the expected order for both profiles.

### Audit scope
- Searched updated scripts/docs for stale `graph/conditional_turns` references and replaced active ones with profile-root layout references.
- Kept validator fallback support for previously generated nested layouts to avoid breaking older datasets.
- No builds or tests were run.

## 2026-03-13 — Move profile graph vectors under `graph/` and keep loader compatibility (implemented)

Reorganized `Maps/data/hanoi_<profile>/` so base graph vectors live under `graph/`, while keeping `line_graph/` at profile root. Updated tooling and wrappers so graph loading continues to work with either `<profile>` or `<profile>/graph` inputs.

### Changes
- Updated pipeline scripts to generate and consume base graph data from `<profile>/graph`:
  - `scripts/pipeline`
  - `CCH-Generator/scripts/run_pipeline`
  - `CCH-Generator/scripts/compare_profiles`
- Updated helper tooling:
  - `scripts/graph_binary_viewer` usage example now points to `.../hanoi_<profile>/graph`
  - `CCH-Hanoi/crates/hanoi-tools/src/bin/generate_line_graph.rs` now resolves graph input from either:
    - direct graph dir (`first_out` exists), or
    - nested graph dir (`<input>/graph/first_out` exists)
  - default output behavior for nested input root remains profile-friendly (`<profile>/line_graph`)
- Updated InertialFlowCutter wrappers in `rust_road_router` to auto-resolve nested graph directories:
  - `rust_road_router/flow_cutter_cch_order.sh`
  - `rust_road_router/flow_cutter_cch_cut_order.sh`
  - `rust_road_router/flow_cutter_cch_cut_reorder.sh`
  - wrappers now save permutations into the resolved graph directory (`.../graph/cch_perm*`)
- Updated active walkthrough commands/paths:
  - `docs/walkthrough/Manual Pipeline Guide.md`
- Migrated existing dataset files:
  - moved `Maps/data/hanoi_motorcycle/{first_out,head,travel_time,geo_distance,way,latitude,longitude,forbidden_turn_*}` to `Maps/data/hanoi_motorcycle/graph/`
  - moved `Maps/data/hanoi_motorcycle/conditional_turns/` to `Maps/data/hanoi_motorcycle/graph/conditional_turns/`
  - preserved `Maps/data/hanoi_motorcycle/line_graph/` at profile root

### Signature audit
- Verified unchanged external signatures used by updated graph-loading paths:
  - `WeightedGraphReconstructor("travel_time").reconstruct_from(&path)`
  - `Load::load_from<P: AsRef<Path>>(...)`
  - `Store::write_to(&dyn AsRef<Path>)`
  - `line_graph(&impl EdgeRandomAccessGraph<Link>, FnMut(EdgeId, EdgeId) -> Option<Weight>)`
- Verified updated helper function signatures in line-graph tool:
  - `parse_args() -> Result<(PathBuf, Option<PathBuf>), Box<dyn Error>>`
  - `resolve_graph_dir(input_dir: &Path) -> PathBuf`
- Verified wrapper script interface compatibility:
  - existing single positional graph argument still accepted
  - nested `graph/` resolution is additive (non-breaking for flat layouts)

### Audit scope
- Confirmed filesystem layout now matches target for available profile data:
  - `Maps/data/hanoi_motorcycle/graph/*` for base graph + conditional turns
  - `Maps/data/hanoi_motorcycle/line_graph/*` unchanged
- Confirmed script path flow consistency:
  - generation, validation, conditional extraction, and line-graph expansion now use `GRAPH_DIR` consistently
  - line-graph output remains `<profile>/line_graph`
- No builds or tests were run.

## 2026-03-13 — Fix CCH-Hanoi architecture: tools are independent, core is empty stub

Corrected the CCH-Hanoi crate structure to match the intended architecture. The previous refactor incorrectly placed line-graph generation logic in `hanoi-core` and made tools/CLI into thin wrappers. The correct design:

- **`hanoi-core`** — empty stub; reserved for future CCH implementation and API exposure
- **`hanoi-cli`** — skeleton; no subcommands until core has APIs
- **`hanoi-tools`** — independent, self-contained utilities; each tool owns its own logic

### Changes
- `CCH-Hanoi/crates/hanoi-tools/src/bin/generate_line_graph.rs`:
  - restored to self-contained tool with all logic inline (validation, graph loading, line-graph generation, output)
  - removed `hanoi_core` import — tool depends on `rust_road_router` directly
- `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`:
  - **deleted** — line-graph logic does not belong in core
- `CCH-Hanoi/crates/hanoi-core/src/lib.rs`:
  - reverted to empty stub (no module declarations)
- `CCH-Hanoi/crates/hanoi-tools/Cargo.toml`:
  - removed `hanoi-core` dependency (not needed for `generate_line_graph`)
- `CCH-Hanoi/crates/hanoi-cli/src/main.rs`:
  - reverted to skeleton (prints "no commands available" — core has no APIs yet)
- `docs/planned/CCH-Hanoi Structure Rework.md`:
  - rewritten to match corrected three-crate architecture
  - removed placeholder crates, removed line-graph-in-core references
  - clarified dependency direction: tools are independent, core never depends on tools
- `docs/walkthrough/CCH-Hanoi Hub.md`:
  - rewritten to reflect corrected architecture, vision, and dependency rules

---

## 2026-03-13 — Refactor CCH-Hanoi to clarified three-crate architecture (implemented)

Refactored `CCH-Hanoi` to match the clarified architecture:
- `hanoi-core` is the primary reusable implementation/API layer
- `hanoi-cli` exposes `hanoi-core` via CLI
- `hanoi-tools` provides independent utility binaries and depends on both `hanoi-core` and `rust_road_router`

### Changes
- Updated `CCH-Hanoi/crates/hanoi-cli/src/main.rs`:
  - implemented CLI wrapper command:
    - `cch-hanoi line-graph generate <graph_dir> [--output-dir <dir>]`
  - CLI now performs argument validation and calls `hanoi_core::line_graph::generate_line_graph(...)`
- Updated `CCH-Hanoi/crates/hanoi-tools/Cargo.toml`:
  - retained `hanoi-core` dependency
  - added explicit `rust_road_router = { path = "../../../rust_road_router/engine" }` dependency to preserve independent tool-layer capability
- Removed placeholder-crate artifact:
  - deleted `CCH-Hanoi/crates/hanoi-graph-loader/Cargo.toml`
  - deleted `CCH-Hanoi/crates/hanoi-graph-loader/src/lib.rs`
  - removed empty `hanoi-graph-loader/` directories
- Updated docs to align with the three-crate model:
  - `AGENTS.md`
  - `CLAUDE.md`
  - `docs/walkthrough/CCH-Hanoi Hub.md`

### Signature audit
- Verified `hanoi-core` API signatures:
  - `default_line_graph_output_dir(graph_dir: &Path) -> PathBuf`
  - `generate_line_graph(graph_dir: &Path, output_dir: &Path) -> HanoiResult<()>`
- Verified CLI/tool wrapper call signatures:
  - `parse_args() -> HanoiResult<(PathBuf, PathBuf)>` in both CLI/tool entry points
  - CLI `main() -> HanoiResult<()>` and tool `main() -> HanoiResult<()>` both call `generate_line_graph(&Path, &Path)`
- Verified all external `rust_road_router` call sites against upstream definitions:
  - `line_graph(graph: &impl EdgeRandomAccessGraph<Link>, turn_costs: impl FnMut(EdgeId, EdgeId) -> Option<Weight>) -> OwnedGraph`
  - `WeightedGraphReconstructor(pub &'static str)` and `reconstruct_from<D: AsRef<OsStr>>(&D)`
  - `Load::load_from<P: AsRef<Path>>(path: P) -> Result<Self>`
  - `Store::write_to(&self, path: &dyn AsRef<Path>) -> Result<()>`
  - graph accessors and traits used by core logic: `num_nodes()`, `num_arcs()`, `degree(NodeId)`, `head()`, `first_out()`, `weight()`
- Verified manifest/dependency signatures:
  - workspace root remains `[workspace] members = ["crates/*"]`
  - `hanoi-tools` now includes both required dependency roots (`hanoi-core`, `rust_road_router`)

### Audit scope
- Confirmed `CCH-Hanoi/crates/` now contains only the three primary crates:
  - `hanoi-core`
  - `hanoi-cli`
  - `hanoi-tools`
- Confirmed `CCH-Generator/scripts/run_pipeline` still invokes standalone tool path via:
  - `cargo run --release -p hanoi-tools --bin generate_line_graph -- <graph_dir>`
- Reviewed active guidance/walkthrough docs for placeholder-crate drift and updated to the current architecture.
- No builds or tests were run, per request.

## 2026-03-13 — CCH-Hanoi Hub walkthrough document

Added walkthrough documentation for CCH-Hanoi describing its purpose, vision, workspace structure, current tools, pipeline role, and future roadmap. Written to reflect the planned post-rework state (per `docs/planned/CCH-Hanoi Structure Rework.md`).

### Changes
- Created `docs/walkthrough/CCH-Hanoi Hub.md`:
  - Purpose and boundary rule (generic algorithms → `rust_road_router`, Hanoi-specific → `CCH-Hanoi`)
  - Dual-surface vision (API-first library + thin CLI/tools)
  - Full workspace layout with crate responsibilities table
  - Dependency direction diagram
  - `generate_line_graph` tool documentation (input/output files, usage, CLI interface)
  - Pipeline position and script invocation
  - Build commands
  - Future roadmap (library extraction, graph loading, customization, query, stable API)

## 2026-03-13 — Hanoi speed calibration defaults for car and motorcycle profiles (implemented)

Implemented the planned Hanoi-focused default speed calibration in RoutingKit OSM profiles, with unchanged `maxspeed` override behavior and unchanged motorcycle way-filter semantics.

### Changes
- Updated `RoutingKit/src/osm_profile.cpp`:
  - `get_osm_way_speed(...)` car defaults:
    - `motorway` `90 -> 100`
    - `motorway_link` `45 -> 40`
    - `trunk` `85 -> 70`
    - `trunk_link` `40 -> 35`
    - `primary` `65 -> 50`
    - `secondary` `55 -> 40`
    - `tertiary` `40 -> 30`
    - `unclassified` `25 -> 20`
    - `residential` `25 -> 20`
    - `service` `8 -> 4`
    - `track` `8 -> 4`
    - junction fallback `20 -> 15`
  - `get_osm_motorcycle_way_speed(...)` motorcycle defaults:
    - `motorway` `80 -> 65`
    - `trunk` `70 -> 60`
    - `trunk_link` `35 -> 30`
    - `primary` `60 -> 55`
    - `primary_link` `30 -> 35`
    - `secondary` `50 -> 45`

### Signature audit
- Verified declaration/definition signature parity for:
  - `get_osm_way_speed(uint64_t, const TagMap&, std::function<void(const std::string&)>)`
  - `get_osm_motorcycle_way_speed(uint64_t, const TagMap&, std::function<void(const std::string&)>)`
  - `parse_osm_speed_tag(uint64_t, const char*, std::function<void(const std::string&)>)`
- Reconfirmed that `is_osm_way_used_by_motorcycles(...)` is unchanged and still enforces per-way expressway exclusion via `motorcycle=no`.

### Audit scope
- Reviewed both edited speed tables line-by-line against `docs/planned/Hanoi Speed Calibration Plan.md`.
- Applied the currently edited plan-table values directly (including motorcycle `motorway = 65` and `primary_link = 35`).
- Confirmed no changes were made to `parse_maxspeed_value(...)`, turn restriction logic, or Rust consumers.
- No builds or runtime executions were attempted, per request.

## 2026-03-13 — GPU compatibility assessment for CCH pipeline and VDF data processing (docs)

Assessed GPU viability across the entire CCH routing pipeline and a planned data processing pipeline (Huber-robust Double ES smoothing + custom VDF model).

### Changes
- Added `docs/walkthrough/GPU Compatibility Assessment.md`:
  - Audited all pipeline stages (contraction, customization, query, InertialFlowCutter) for GPU suitability
  - Documented current CPU parallelism tech (rayon, OpenMP, TBB) per component
  - Assessed graph algorithm stages as not viable for GPU due to irregular memory access patterns
  - Identified Huber-robust DES and VDF application as excellent GPU candidates (embarrassingly parallel, per-edge independence)
  - Outlined recommended GPU technology choices (wgpu, cudarc, CuPy/PyTorch)
  - Mapped integration point to server `/customize` endpoint and `travel_time` binary format
  - Added implementation trade-offs section: development complexity, deployment/portability, performance nuance, and decision criteria for GPU vs CPU
  - Recommended staged migration approach (CPU rayon baseline first, GPU only if latency requires it)
  - Added hardware-specific analysis for 2x NVIDIA A30 (Ampere GA100): benefits (HBM2 bandwidth, MIG partitioning, NVLink), drawbacks (CUDA lock-in, driver maintenance, overprovisioning risk), and revised staged recommendation using MIG slices
  - Added scale-aware analysis with actual Hanoi network metadata (~900k nodes/1.2M edges loaded, ~2.7M/3.6M line graph), revised memory estimates, CPU vs GPU timing at real scale, MIG slice sizing per scenario, and updated recommendations showing GPU is strongly recommended for line graph real-time use cases

## 2026-03-13 — Extend CCH-Hanoi structure rework plan with placeholder crates (docs)

Extended the `CCH-Hanoi` rework plan so the initial workspace bootstrap explicitly creates empty placeholder crates for future module areas, instead of leaving those package boundaries implicit.

### Changes
- Updated `docs/planned/CCH-Hanoi Structure Rework.md`:
  - added explicit placeholder crates for future graph-loading, customization, and query modules
  - added bootstrap guidance that placeholder crates should compile cleanly with stub `lib.rs` files
  - clarified that these crates establish workspace/package boundaries only and do not count as implemented functionality
  - added placeholder-crate compilation to the workspace verification expectations

## 2026-03-13 — Refine CCH-Hanoi structure rework plan for standalone utility builds (docs)

Adjusted the planned `CCH-Hanoi` hub rework so utility binaries such as `generate_line_graph` remain independently buildable, rather than being treated only as transitional CLI aliases.

### Changes
- Updated `docs/planned/CCH-Hanoi Structure Rework.md`:
  - added a dedicated `hanoi-tools` workspace member for standalone utility binaries
  - changed `generate_line_graph` from a temporary compatibility command to a first-class standalone utility surface
  - clarified that the main `cch-hanoi` CLI and standalone tools both wrap the same `hanoi-core` API
  - added explicit independent-build requirements and test coverage for separately built utilities
  - kept the API-first direction unchanged while making separate utility builds an explicit requirement

## 2026-03-13 — Audit round 6: stale directory references + parse-failure fallback (implemented)

Audited all changes from 2026-03-13. Found and fixed stale `CCH-Advanced-Generator` directory references (actual directory is `CCH-Hanoi`) and a silent parse-failure loophole in conditional turn extraction.

### Bug 1 — Stale `CCH-Advanced-Generator` references across scripts and guidance files
The Rust line graph generator directory was renamed to `CCH-Hanoi` but multiple execution-critical paths still referenced the old name `CCH-Advanced-Generator`, causing runtime failures.

#### Changes
- Updated `scripts/pipeline`:
  - line 35: `CCH-Advanced-Generator` → `CCH-Hanoi` in `LINE_GRAPH_GEN` path
- Updated `CCH-Generator/scripts/run_pipeline`:
  - line 26: `CCH-Advanced-Generator` → `CCH-Hanoi` in `LINE_GRAPH_GENERATOR_DIR` path
- Updated `CLAUDE.md`:
  - component name, build command, and pipeline diagram updated from `CCH-Advanced-Generator` to `CCH-Hanoi`
- Updated `AGENTS.md`:
  - project structure, build command, and test command updated from `CCH-Advanced-Generator` to `CCH-Hanoi`
- Updated `docs/walkthrough/Manual Pipeline Guide.md`:
  - all four references updated from `CCH-Advanced-Generator` to `CCH-Hanoi`

### Bug 2 — Parse-failure conditional turns silently never enforced
`RoutingKit/src/conditional_turn_extract.cpp` Step 3: when `parse_conditional_value` returned empty for a conditional entry (unparsable condition string), `parsed_windows[i]` stayed empty. The arc pair was still written to output with zero time windows, so `is_time_window_active()` always returned `false` — the restriction was silently dropped at query time.

#### Changes
- Updated `RoutingKit/src/conditional_turn_extract.cpp`:
  - parse-failure path now assigns `{0x7F, 0, 1440}` (all-day, always-active) as a conservative fallback, ensuring unparsable restrictions are enforced rather than silently ignored

### Audit scope (2026-03-13)
Audited all changes from the two new changelog entries:
- **Conditional turns reorganization**: producer write paths, validator read paths, `compare_profiles` read path, `scripts/pipeline`, `run_pipeline` — all correctly use `conditional_turns/` subdirectory. No remaining flat-path consumers found in repo-wide search.
- **Unconditional via-way time window fix**: `{0x7F, 0, 1440}` correctly evaluates to always-active in `is_time_window_active()` (takes `end > start` branch: `minutes >= 0 && minutes < 1440`, always true for valid inputs) — **no issues**
- **Parse-failure fallback**: verified `save_time_windows` correctly serializes the fallback window — **no issues**
- **`CCH-Hanoi` directory name**: verified `Cargo.toml` package name is `cch-hanoi`, directory is `CCH-Hanoi`. All execution-critical references now corrected.
- **`compare_profiles` script**: hardcoded `hanoi_` prefix in output dirs is a minor limitation but consistent with `run_pipeline` — **noted, not fixed**

## 2026-03-13 — Reorganize conditional turn outputs under `conditional_turns/` (implemented)

Implemented the planned `Maps/data` layout change so only conditional turn files move into a dedicated `conditional_turns/` subdirectory, while graph, node, fixed-turn, and line-graph paths remain unchanged.

### Changes
- Updated `RoutingKit/src/conditional_turn_extract.cpp`:
  - conditional outputs now always write to `<output_root>/conditional_turns/`
  - added explicit subdirectory creation before writing outputs
  - updated CLI usage text to describe the new output layout
  - replaced direct `save_vector(...)` writes for conditional arc vectors with an empty-safe local writer so empty-output cases do not rely on `vec[0]`
- Updated `CCH-Generator/src/validate_graph.cpp`:
  - conditional validation now reads:
    - `graph_dir/conditional_turns/conditional_turn_from_arc`
    - `graph_dir/conditional_turns/conditional_turn_to_arc`
    - `graph_dir/conditional_turns/conditional_turn_time_windows`
- Updated `CCH-Generator/scripts/compare_profiles`:
  - conditional-turn counts now load from `conditional_turns/conditional_turn_from_arc`

### Signature audit
- Verified `RoutingKit::resolve_conditional_restrictions(...)` against `RoutingKit/include/routingkit/conditional_restriction_resolver.h`
- Verified `RoutingKit::load_vector<T>(const std::string&)` against `RoutingKit/include/routingkit/vector_io.h`
- Verified `scan_conditional_restrictions_from_pbf(...)`, `car_conditional_tag_priority()`, and `motorcycle_conditional_tag_priority()` against `RoutingKit/include/routingkit/conditional_restriction_decoder.h`
- Verified `is_osm_way_used_by_cars(...)` and `is_osm_way_used_by_motorcycles(...)` against `RoutingKit/include/routingkit/osm_profile.h`
- Verified `parse_conditional_value(const char*)` against `RoutingKit/include/routingkit/osm_condition_parser.h`
- Verified `get_micro_time()` against `RoutingKit/include/routingkit/timer.h`
- Verified the touched validator path handling only uses current `std::filesystem` APIs already present in `validate_graph.cpp`
- Verified `CCH-Generator/scripts/run_pipeline` requires no path changes because it delegates conditional extraction and validation to the updated binaries

### Audit scope
- Repository-wide executable/source search for `conditional_turn_from_arc`, `conditional_turn_to_arc`, and `conditional_turn_time_windows`: no remaining live code paths expect the old flat profile-root location
- Reviewed extractor write flow for both normal and empty-output cases: `conditional_turns/` is created before writes, and empty conditional vectors are now written safely
- Reviewed validator conditional checks end-to-end with the new nested paths: presence, pair-size consistency, arc bounds, lexicographic sort, packed time-window integrity, and overlap checks remain unchanged apart from the directory prefix
- Reviewed `compare_profiles` summary path so reported conditional-turn counts remain accurate after the reorganization
- No builds or runtime executions were attempted, per request

## 2026-03-13 — Sync AGENTS guidance to Codex project memory (docs)

Captured the latest repository guidance from `AGENTS.md` into persistent Codex project memory for this workspace.

### Changes
- Added `/home/thomas/.codex/memories/home__thomas__VTS__Hanoi-Routing.md` with synced project memory covering:
  - repository structure
  - build and validation commands
  - coding conventions
  - canonical time-unit rules
  - process rules, including changelog discipline and Plan mode output location

## 2026-03-13 — Audit round 5: fix unconditional via-way time windows (implemented)

Fixed unconditional via-way turn restrictions being silently ineffective due to empty time windows in the conditional turn output.

### Bug found
- `RoutingKit/src/conditional_turn_extract.cpp:220-224`: Unconditional via-way restrictions (resolved with `condition_string = ""`) were assigned empty `parsed_windows`, making `is_time_window_active()` always return `false`. These restrictions were never enforced at query time.

### Changes
- Updated `RoutingKit/src/conditional_turn_extract.cpp`:
  - added `{0x7F, 0, 1440}` (all days, 00:00–24:00) time window assignment for unconditional entries in Step 3, so they are always active during query evaluation

### Audit scope (2026-03-13)
Audited all changes from 2026-03-12 and 2026-03-13:
- Travel-time anomaly tracking (validate_graph.cpp): speed formula, div-by-zero guards, sample diagnostics — **no issues**
- 24:MM time validation fix (osm_condition_parser.cpp): **verified fixed**
- Uninitialized inversion_index fix (validate_graph.cpp): **verified fixed** in both forbidden and conditional sort checks
- Multi-via-way chain resolution (conditional_restriction_resolver.cpp): junction dedup, mandatory decomposition, unique-candidate fallback — **no issues**
- Forbidden-turn merge-scan in line graph generator (generate_line_graph.rs): lexicographic ordering assumption valid — **no issues**
- Overlap filter timing/logging (conditional_turn_extract.cpp): early-return paths — **no issues**
- Pipeline scripts (scripts/pipeline, CCH-Generator/scripts/run_pipeline): fail-fast, pre-flight checks — **no issues**

## 2026-03-13 — Document canonical travel-time units across guidance files (docs)

Aligned repository guidance docs with verified code behavior for time-unit handling across RoutingKit and rust_road_router.

### Changes
- Updated `AGENTS.md`:
  - added `Time Unit Conventions` section documenting:
    - persisted `travel_time` in milliseconds
    - canonical OSM formula (`*18000 / speed / 5`) as millisecond conversion
    - integer TD millisecond timeline (`86_400_000` per day)
    - role of `tt_units_per_s` metadata (`1000` in current pipelines)
    - floating TD/CATCHUp internal seconds with explicit ms<->s conversion
    - note on stale legacy "seconds" comments
- Updated `CLAUDE.md`:
  - added `Time Unit Conventions` subsection under `Data Format` with the same canonical rules and caveats

## 2026-03-13 — Travel-time anomaly tracking in graph validator (implemented)

Extended travel-time validation diagnostics so abnormal values are easier to investigate directly from validator output.

### Changes
- Updated `CCH-Generator/src/validate_graph.cpp`:
  - added sample arc IDs to `Travel time sanity` for zero travel times and `>24h` travel times
  - added `Travel time anomaly tracking` check that estimates implied speed from `geo_distance` and `travel_time`
  - reports warning-level outliers for unusually fast (`>180 km/h`) or slow (`<1 km/h`) arcs when distance is at least 100 m
  - includes per-sample diagnostics (`tail->head`, `tt`, `dist`, implied `speed`) to support targeted debugging

## 2026-03-12 — Reject invalid `24:MM` times in condition parser (implemented)

Fixed time validation in `try_parse_time` to reject semantically invalid times where hour is 24 but minutes are non-zero (e.g. `24:30`, `24:59`). Only `24:00` is valid as the end-of-day boundary per OSM spec.

### Changes
- Updated `RoutingKit/src/osm_condition_parser.cpp`:
  - line 36: added `(h == 24 && m != 0)` guard to the existing `h > 24 || m > 59` check

## 2026-03-12 — Multi-via-way audit round 4 fixes (implemented)

Strengthened the forbidden-turn sorting validation in the graph validator to match the conditional-turn check upgraded in round 2.

### Changes
- Updated `CCH-Generator/src/validate_graph.cpp`:
  - replaced `std::is_sorted` on `forbidden_turn_from_arc` alone with full lexicographic `(from_arc, to_arc)` order validation
  - added inversion-index diagnostics for failed forbidden-turn sort checks
  - now consistent with the conditional-turn sorting check pattern
- Noted: `osm_profile.cpp` `motorcar` variable shadow inside `bicycle_road` block is original RoutingKit code and intentionally left unchanged

## 2026-03-12 — Multi-via-way audit round 3 fixes (implemented)

Implemented the concrete resolver/extractor hardening changes from `docs/planned/Multi-Via-Way Audit 2.md`: empty-graph safety in resolver graph metadata, explicit graph vector consistency checks, and Step 2b overlap-filter timing/logging fixes for early-return paths.

### Changes
- Updated `RoutingKit/src/conditional_restriction_resolver.cpp`:
  - changed `GraphData::node_count()` to guard empty `first_out` (`0` instead of unsigned underflow)
  - added upfront consistency checks in `load_graph(...)`:
    - `way.size() == arc_count`
    - `latitude.size() == node_count`
    - `longitude.size() == node_count`
  - added descriptive `std::runtime_error` messages on mismatch
- Updated `RoutingKit/src/conditional_turn_extract.cpp`:
  - refactored Step 2b overlap-filter timing/logging so completion timing is always reported (including skipped paths)
  - added explicit skip-reason logs for forbidden-turn load failures and vector length mismatches
  - preserved existing overlap filtering behavior for successful paths
- Signature audit:
  - verified touched API signatures against current headers/traits:
    - `RoutingKit::load_vector(...)`, `RoutingKit::save_vector(...)`, `RoutingKit::get_micro_time(...)`
    - `RoutingKit::load_osm_id_mapping_from_pbf(...)`
    - `RoutingKit::scan_conditional_restrictions_from_pbf(...)`
    - `RoutingKit::resolve_conditional_restrictions(...)`

## 2026-03-12 — Multi-via-way audit round 2 fixes (implemented)

Implemented the concrete logic/safety fixes from `docs/planned/Multi-Via-Way Audit.md`: removed dead line-graph input handling, hardened way-index bounds in junction lookup, strengthened conditional turn sorting validation, cleaned redundant day-mask writes, and made the interactive pipeline script fail-fast.

### Changes
- Updated `CCH-Advanced-Generator/src/generate_line_graph.rs`:
  - removed dead `geo_distance` load and arc-count validation in the line-graph generator (input was unused and never written to output)
- Updated `RoutingKit/src/conditional_restriction_resolver.cpp`:
  - added defensive bounds checks in `find_junction_node(...)` before indexing `first_index_of_way`
- Updated `CCH-Generator/src/validate_graph.cpp`:
  - replaced conditional-turn sorting check on `conditional_turn_from_arc` alone with full lexicographic `(from_arc, to_arc)` order validation
  - added inversion-index diagnostics for failed conditional sort checks
- Updated `RoutingKit/src/osm_condition_parser.cpp`:
  - removed redundant `mask |= (1 << last_day)` assignments after inclusive day-range loops in `parse_day_spec(...)`
- Updated `scripts/pipeline`:
  - added `set -euo pipefail` for fail-fast shell behavior
- Signature audit:
  - verified all touched external API call signatures against current declarations: `WeightedGraphReconstructor::reconstruct_from(...)`, `line_graph(...)`, `Load::load_from(...)`, `Store::write_to(...)`, `load_osm_id_mapping_from_pbf(...)`, `scan_conditional_restrictions_from_pbf(...)`, and `resolve_conditional_restrictions(...)`

## 2026-03-12 — Multi-via-way turn restrictions in conditional resolver (implemented)

Implemented full via-way chain resolution in the conditional restriction resolver so restrictions with multiple `via` ways are no longer dropped. Single via-way restrictions now run through the same generalized path.

### Changes
- Updated `RoutingKit/src/conditional_restriction_resolver.cpp`:
  - replaced the `multi-via-way not supported` drop path with generalized chain handling for `from_way -> via_way[0..N-1] -> to_way`
  - maps and validates every via-way member against routing-way IDs before resolution
  - computes one unique junction per consecutive way pair and rejects degenerate chains that reuse a junction node
  - resolves one `(from_arc, to_arc)` pair per chain junction, with direction-based disambiguation applied only at the chain entry junction
  - emits decomposed turn pairs for both prohibitive and mandatory restrictions across all chain junctions
  - preserved existing via-node behavior and single-via-way behavior as a special case of the generalized chain logic
  - audited integration call signatures used by this flow (`load_osm_id_mapping_from_pbf`, `scan_conditional_restrictions_from_pbf`, `resolve_conditional_restrictions`) against their public headers

## 2026-03-11 — Add IFC ordering phases to Manual Pipeline Guide (docs)

Added Phase 7 (CCH node ordering for normal graph) and Phase 8 (CCH node ordering for line graph) to `docs/Manual Pipeline Guide.md`. Covers IFC build prerequisites, wrapper scripts, manual invocation with full parameter breakdown, key differences between normal/line graph ordering, the arc-ordering variant, and verification steps.

### Changes
- Updated `docs/Manual Pipeline Guide.md`:
  - Added sections 9 (Phase 7) and 10 (Phase 8) with detailed IFC instructions
  - Updated table of contents (now 12 sections)
  - Updated Quick Reference block with Phase 7 and Phase 8 commands
  - Renumbered existing sections 9–10 to 11–12

## 2026-03-11 — Filter conditional turns that overlap with forbidden turns (implemented)

Added Step 2b to `conditional_turn_extract` that loads existing `forbidden_turn_from_arc`/`forbidden_turn_to_arc` and removes any conditional turn pairs that duplicate an unconditional forbidden turn. Unconditional bans supersede conditional restrictions, so the overlap is redundant.

### Changes
- Updated `RoutingKit/src/conditional_turn_extract.cpp`:
  - Added `#include <unordered_set>`
  - Added Step 2b between resolution (Step 2) and parsing (Step 3): loads forbidden turns, builds a hash set of `(from_arc, to_arc)` pairs, filters out matching conditional turns

## 2026-03-11 — Disable degree-2 chain compression in CCH Generator (implemented)

Changed `all_modelling_nodes_are_routing_nodes` from `false` to `true` in the CCH Generator's OSM graph loading, so all nodes on routable ways are preserved in the routing graph (no degree-2 chain compression).

### Changes
- Updated `CCH-Generator/src/generate_graph.cpp`:
  - line 156: `load_osm_id_mapping_from_pbf` parameter `false` → `true`
  - line 177: `load_osm_routing_graph_from_pbf` parameter `false` → `true`

## 2026-03-11 — Rust 2024 edition + lifetime syntax + dead code fixes (implemented)

Fixed the Rust 2024 edition pattern-matching error in the line graph generator, plus ~80 `mismatched_lifetime_syntaxes` warnings and 3 dead-code warnings across the engine crate.

### Changes — error
- Updated `CCH-Advanced-Generator/src/generate_line_graph.rs`:
  - line 130: `Some((&from_arc, &to_arc))` → `Some(&(&from_arc, &to_arc))` (Rust 2024 requires explicit `&` for implicit-borrow patterns in `Peekable::peek()`)

### Changes — dead code warnings
- Updated `rust_road_router/engine/src/report.rs`:
  - line 309: added `#[allow(dead_code)]` on `CollectionItemContextGuard`'s RAII field
- Updated `rust_road_router/engine/src/datastr/graph/floating_time_dependent/piecewise_linear_function/cursor.rs`:
  - line 15: added `#[allow(dead_code)]` on unused trait method `ipps`
- Updated `rust_road_router/engine/src/datastr/graph/time_dependent/geometry/point.rs`:
  - line 30: added `#[allow(dead_code)]` on `Point` struct (fields `x`, `y` only used via `Debug`)

### Changes — mismatched lifetime syntax warnings (~78 instances across 20 files)
Added explicit `'_` lifetime annotations (or named lifetimes where appropriate) to return types that hide an elided lifetime, per the new `mismatched_lifetime_syntaxes` lint:

- `algo/alt.rs`: `ALTPotential` → `ALTPotential<'_>` (2 sites)
- `algo/ch_potentials.rs`: `BorrowedCCHPot`, `CHPotential<BorrowedGraph, ...>`, `CCHPotentialWithPathUnpacking`, `BorrowedGraph`, `BucketCHPotential` (13 sites)
- `algo/ch_potentials/penalty.rs`: `BorrowedCCHPot` in `PenaltyPot` (1 site)
- `algo/ch_potentials/query.rs`: `BiconnectedPathServerWrapper`, `BiDirCorePathServerWrapper`, `MultiThreadedBiDirCorePathServerWrapper` (4 sites)
- `algo/ch_potentials/td_query.rs`: `BiconnectedPathServerWrapper` (2 sites)
- `algo/contraction_hierarchy/mod.rs`: `PartialContractionGraph` (1 site)
- `algo/customizable_contraction_hierarchy/mod.rs`: `Iter`, `Slcs`, `BorrowedGraph` in trait + 2 impls (9 sites)
- `algo/customizable_contraction_hierarchy/contraction.rs`: `PartialContractionGraph` (1 site)
- `algo/customizable_contraction_hierarchy/customization.rs`: `CustomizedBasic` (2 sites)
- `algo/customizable_contraction_hierarchy/customization/directed.rs`: `CustomizedBasic` (1 site)
- `algo/customizable_contraction_hierarchy/query/nearest_neighbor.rs`: `'_` → `'s` (1 site)
- `algo/dijkstra/query/dijkstra.rs`: `ServerWrapper` (1 site)
- `algo/rphast.rs`: `RPHASTResult`, `SSERPHASTResult` (2 sites)
- `report.rs`: `CollectionItemContextGuard` (1 site)
- `datastr/graph/floating_time_dependent/mod.rs`: `MutTopPLF` (1 site)
- `datastr/graph/floating_time_dependent/graph.rs`: `PeriodicPiecewiseLinearFunction`, `UpdatedPiecewiseLinearFunction` (2 sites)
- `datastr/graph/floating_time_dependent/shortcut.rs`: `SourcesIter` (2 sites)
- `datastr/graph/floating_time_dependent/shortcut_source.rs`: `WrappingSourceIter` (2 sites)
- `datastr/graph/floating_time_dependent/shortcut_graph.rs`: `PeriodicATTF`, `PartialATTF`, `BorrowedGraph`, `ReconstructedGraph` (17 sites)
- `datastr/graph/time_dependent/graph.rs`: `PiecewiseLinearFunction` (1 site)
- `link_speed_estimates/link_speed_estimator.rs`: `dyn State + 'a` → `dyn State<'a> + 'a` (14 sites)

---

## 2026-03-11 — Engine crate nightly compatibility fixes (implemented)

Fixed compilation errors and warnings in `rust_road_router/engine` caused by nightly compiler evolution: renamed/stabilized feature gates, a renamed slice method, ambiguous glob imports, unused-mut bindings, and missing `check-cfg` declarations.

### Changes — errors
- Updated `rust_road_router/engine/src/lib.rs`:
  - replaced `#![feature(type_alias_impl_trait)]` with `#![feature(impl_trait_in_assoc_type)]` (feature gate was renamed)
  - removed `#![feature(array_windows)]` (stable since 1.94.0)
  - removed `#![feature(slice_group_by)]` (stable since 1.77.0)
  - removed `#![feature(binary_heap_retain)]` (stable since 1.70.0)
- Updated `rust_road_router/engine/src/algo/hl.rs`:
  - renamed `group_by_mut` to `chunk_by_mut` (method was renamed during `slice_group_by` stabilization)

### Changes — warnings
- Updated `rust_road_router/engine/build.rs`:
  - added `cargo::rustc-check-cfg` declarations for `override_tdcch_approx_threshold`, `override_tdcch_approx`, and `override_traffic_max_query_time` to silence `unexpected_cfgs` warnings
- Updated `rust_road_router/engine/src/datastr/graph/floating_time_dependent/shortcut_graph.rs`:
  - added explicit `use super::shortcut_source::Sources as _;` to disambiguate the `Sources` trait from the `Sources` enum imported via `use super::*;` (future hard error per rust-lang/rust#147992)
  - removed unnecessary `mut` from two `let mut waiting_state` bindings (lines ~1048 and ~1755) — the variable holds a `&mut` reference but is never reassigned

---

## 2026-03-11 — Manual Pipeline Guide (documentation)

Added `docs/Manual Pipeline Guide.md` — a step-by-step walkthrough for manually running the graph generation pipeline (PBF → base graph → conditional turns → line graph), with data format explanations, inspection scripts, and conceptual deep dives.

### Changes
- Added `docs/Manual Pipeline Guide.md`

---

## 2026-03-11 — CCH-Advanced-Generator edition set to 2024 (implemented)

Adjusted the Rust edition for `CCH-Advanced-Generator` to match your requested edition.

### Changes
- Updated `CCH-Advanced-Generator/Cargo.toml`:
  - changed `edition` from `2021` to `2024`

---

## 2026-03-11 — Line graph generator implementation + pipeline integration (implemented)

Implemented the `docs/Line Graph Generator Plan.md` deliverables by adding a dedicated Rust line-graph generator crate binary and wiring it into the CCH pipeline with line-graph validation steps.

### Changes
- Updated `CCH-Advanced-Generator/Cargo.toml`:
  - renamed package to `cch-advanced-generator`
  - switched to Rust 2021 edition
  - added engine path dependency: `rust_road_router = { path = "../rust_road_router/engine" }`
  - declared explicit binary target `generate_line_graph` at `src/generate_line_graph.rs`
- Added `CCH-Advanced-Generator/rust-toolchain.toml`:
  - pinned toolchain channel to `nightly` to match `rust_road_router` engine requirements
- Added `CCH-Advanced-Generator/src/generate_line_graph.rs`:
  - implemented CLI: `generate_line_graph <graph_dir> [<output_dir>]`
  - loads base graph via `WeightedGraphReconstructor("travel_time").reconstruct_from(...)`
  - loads `latitude`, `longitude`, `forbidden_turn_from_arc`, `forbidden_turn_to_arc`
  - validates forbidden-turn arrays (length equality, arc bounds, lexicographic sorting) before merge-scan
  - builds turn-expanded line graph via engine `line_graph(...)` with:
    - forbidden-turn filter (peekable merge-scan)
    - U-turn filter (`tail[e1] == head[e2]`)
    - zero turn penalty (`Some(0)`)
  - maps line-graph node coordinates from original `tail` node coordinates
  - writes RoutingKit-format outputs: `first_out`, `head`, `travel_time`, `latitude`, `longitude`
  - prints basic generation stats and output path
- Removed placeholder `CCH-Advanced-Generator/src/main.rs` ("Hello, world!")
- Updated `CCH-Generator/scripts/run_pipeline`:
  - integrated Rust line-graph generation into both profiles
  - expanded flow to 12 steps:
    - generate + validate base graph (car/motorcycle)
    - extract conditionals + validate (car/motorcycle)
    - generate line graph + validate with `--turn-expanded` (car/motorcycle)
  - added manifest/cargo preflight checks for `CCH-Advanced-Generator`
  - removed IFC permutation generation from this script so it remains a separate/manual step per plan

### Signature and existence checks performed during implementation
- Confirmed engine API: `line_graph(graph, turn_costs)` in `rust_road_router/engine/src/datastr/graph.rs`
- Confirmed graph loader: `WeightedGraphReconstructor("travel_time").reconstruct_from(...)` in `rust_road_router/engine/src/datastr/graph/first_out_graph.rs`
- Confirmed validator CLI supports `--turn-expanded` in `CCH-Generator/src/validate_graph.cpp`

---

## 2026-03-11 — InertialFlowCutter build fixes for modern CMake and oneTBB (implemented)

Fixed build compatibility issues preventing InertialFlowCutter from compiling with CMake 4.x and oneTBB 2021+.

### Change
- Updated `rust_road_router/lib/InertialFlowCutter/CMakeLists.txt`:
  - bumped `cmake_minimum_required` from `3.1` to `3.5...3.31` for CMake 4.x compat
  - removed custom `FindTBB.cmake` module path; now uses system-provided `TBBConfig.cmake`
  - replaced `${TBB_LIBRARIES}` link with modern imported target `TBB::tbb`
- Updated `rust_road_router/lib/InertialFlowCutter/extern/KaHIP/CMakeLists.txt`:
  - bumped `cmake_minimum_required` from `3.10` to `3.10...3.31` for CMake 4.x compat
- Updated `rust_road_router/lib/InertialFlowCutter/src/console.cpp`:
  - replaced `#include <tbb/task_scheduler_init.h>` with `#include <tbb/global_control.h>`
  - replaced all 15 occurrences of `tbb::task_scheduler_init scheduler(...)` with `tbb::global_control gc(tbb::global_control::max_allowed_parallelism, ...)`

### Build command
```bash
cd rust_road_router/lib/InertialFlowCutter
mkdir -p build
/usr/bin/cmake -S . -B build -DCMAKE_BUILD_TYPE=Release -DGIT_SUBMODULE=OFF -DUSE_KAHIP=OFF
cmake --build build --target console -j"$(nproc)"
```

---

## 2026-03-11 — CCH-Advanced-Generator plan: Rust-based line graph generation (docs only)

### Changes
- Updated `docs/CCH Walkthrough.md`:
  - Section 1.1: Added `way` vector to directory layout; corrected `forbidden_turn_*` annotation from "turn-aware only" to "always" (they're always produced by RoutingKit's OSM loader)
  - Section 1.2: Added documentation for the two-pass Builder API (`osm_graph_builder.h` + `osm_profile.h`) used by CCH-Generator, alongside the existing simple API section
  - Section 1.3: Added documentation for the standalone `turn_expand_osm.rs` Rust binary that exports the line graph to disk
- Rewrote `docs/Line Graph Generator Plan.md`: Focused Rust project (`CCH-Advanced-Generator`) as a discrete line graph generator — reads pre-generated base graph from CCH-Generator (C++), calls `engine::line_graph()` directly, outputs to `line_graph/` subdirectory for InertialFlowCutter

## 2026-03-11 — Pipeline outputs moved to `Maps/data` and root-PWD script execution (implemented)

Adjusted pipeline/script execution paths so profile outputs are written under `Maps/data` and script invocations are robust when launched from repository root.

### Change
- Updated `CCH-Generator/scripts/run_pipeline`:
  - usage now takes `<input.osm.pbf>` plus optional deprecated `output_root_dir` (ignored)
  - output root is now fixed to `Maps/data` under the repository root
  - profile directories are now:
    - `Maps/data/car`
    - `Maps/data/motorcycle`
  - script now `cd`s to repository root before running pipeline steps
  - IFC helper now invokes flow-cutter scripts from repository root
- Updated flow-cutter scripts to be CWD-independent by resolving console path from script location:
  - `rust_road_router/flow_cutter_cch_order.sh`
  - `rust_road_router/flow_cutter_cch_cut_order.sh`
  - `rust_road_router/flow_cutter_cch_cut_reorder.sh`

---

## 2026-03-11 — `is_unsigned_integer` modernized with `std::all_of` (implemented)

Refactored digit validation in `CCH-Generator/src/validate_graph.cpp` to use the STL algorithm style suggested by CLion while preserving behavior and safety.

### Change
- Replaced the manual character loop in `is_unsigned_integer(...)` with `std::all_of(...)`.
- Kept the explicit empty-string guard (`!value.empty()`) so empty input still returns `false`.
- Kept `static_cast<unsigned char>(c)` before `std::isdigit(...)` to avoid signed-char undefined behavior.

---

## 2026-03-11 — Conditional Extractor Cleanup (implemented)

Removed redundant API-signature verification scaffolding from `RoutingKit/src/conditional_turn_extract.cpp`.

### Change
- Deleted `verify_profile_aware_api_signatures()` and its call in `main(...)`.
- Rationale: direct calls to the profile-aware APIs already provide compile-time type checking; helper added no runtime behavior and reduced readability.

---

## 2026-03-11 — Profile-Aware Conditional Resolver (implemented)

Implemented `docs/Profile-Aware Conditional Resolver Proposal.md` across RoutingKit and CCH-Generator scripts so conditional restriction extraction and resolution are profile-aware (car/motorcycle), while preserving backward compatibility.

### Change A — Decoder tag-priority callbacks
- Updated `RoutingKit/include/routingkit/conditional_restriction_decoder.h`:
  - added `ConditionalTagPriority`
  - added profile factories:
    - `car_conditional_tag_priority()`
    - `motorcycle_conditional_tag_priority()`
  - added new primary API:
    - `scan_conditional_restrictions_from_pbf(..., ConditionalTagPriority, ...)`
  - kept old call shape via backward-compatible inline overload (defaults to car tags)
- Updated `RoutingKit/src/conditional_restriction_decoder.cpp`:
  - replaced hardcoded `restriction(:motorcar)` lookups with `tag_priority` lookups
  - added safety fallback to car priority when an incomplete tag-priority struct is passed

### Change B — Resolver way-filter callback
- Updated `RoutingKit/include/routingkit/conditional_restriction_resolver.h`:
  - added new primary API:
    - `resolve_conditional_restrictions(..., std::function<bool(uint64_t, const TagMap&)>, ...)`
  - kept old call shape via backward-compatible inline overload (defaults to car way filter)
- Updated `RoutingKit/src/conditional_restriction_resolver.cpp`:
  - replaced hardcoded `is_osm_way_used_by_cars(...)` mapping callback with caller-provided `is_way_used(...)`
  - added defensive fallback to car filter if an empty `std::function` is provided

### Change C — `conditional_turn_extract` profile flag
- Updated `RoutingKit/src/conditional_turn_extract.cpp`:
  - CLI now supports:
    - `conditional_turn_extract <pbf_file> <graph_dir> [<output_dir>] [--profile car|motorcycle]`
  - defaults to `car` when profile is not supplied
  - parses both `--profile <value>` and `--profile=<value>`
  - wires profile into both stages:
    - decoder tag-priority selection
    - resolver way-filter selection
  - added explicit compile-time signature/existence checks via function-pointer bindings for:
    - `scan_conditional_restrictions_from_pbf` (new overload)
    - `resolve_conditional_restrictions` (new overload)
    - `is_osm_way_used_by_cars`
    - `is_osm_way_used_by_motorcycles`

### Change D — Pipeline/profile script updates
- Updated `CCH-Generator/scripts/run_pipeline.sh`:
  - car extraction now passes `--profile car`
  - motorcycle extraction now passes `--profile motorcycle`
  - removed obsolete car-only resolver warning
- Updated `CCH-Generator/scripts/compare_profiles.sh`:
  - car extraction now passes `--profile car`
  - motorcycle extraction now passes `--profile motorcycle`
  - removed obsolete car-only resolver warning

### Post-implementation audit notes
- Verified no remaining hardcoded car-way mapping in resolver code path used by `conditional_turn_extract`.
- Verified decoder tag selection now follows profile-first then generic fallback order.
- Verified backward-compatible overloads still allow legacy call sites without profile/callback arguments.
- Added defensive runtime fallback behavior for malformed callback/tag-priority inputs to avoid null dereference / empty-function invocation failures.

---

## 2026-03-11 — Conditional Turn Integration (implemented)

Implemented `docs/Conditional Turn Integration Plan.md` across CCH-Generator so conditional turn restrictions are generated, persisted, validated, and surfaced in pipeline/profile tooling.

### Change 1 — Save `way` vector in generator
- Updated `CCH-Generator/src/generate_graph.cpp` to bypass `simple_load_osm_*` and call:
  - `load_osm_id_mapping_from_pbf(...)`
  - `load_osm_routing_graph_from_pbf(...)`
- Replicated travel-time computation logic from `osm_simple.cpp` and added `way` persistence:
  - writes new output file: `way` (arc -> routing way ID)
- Added profile-aware callback wiring for both car and motorcycle paths.

### Change 2 + 4 — Pipeline/script integration
- Updated `CCH-Generator/scripts/run_pipeline.sh`:
  - adds `RoutingKit/bin/conditional_turn_extract` as a required executable
  - adds conditional extraction + validation pass for both profiles
  - expands flow to `[1/10]..[10/10]` and keeps IFC permutation generation/validation for both profiles
- Updated `CCH-Generator/scripts/compare_profiles.sh`:
  - runs conditional extraction when available
  - reports `Conditional turns` count/delta in profile comparison output

### Change 3 — Conditional output validation
- Updated `CCH-Generator/src/validate_graph.cpp` with auto-detected conditional checks (when conditional files exist):
  - `Conditional turn vector consistency`
  - `Conditional turn arc bounds`
  - `Conditional turn sorting`
  - `Time window file integrity` (offset prefix, monotonic offsets, packed-size check with 5-byte packed TimeWindow entries)
  - `No overlap with forbidden turns`
- Added explicit presence check for the conditional triplet:
  - `conditional_turn_from_arc`, `conditional_turn_to_arc`, `conditional_turn_time_windows`

### Signature/existence checks
- Verified intended RoutingKit methods exist in headers and bound them with explicit function-pointer signatures in `generate_graph.cpp`:
  - `load_osm_id_mapping_from_pbf`
  - `load_osm_routing_graph_from_pbf`
  - `is_osm_way_used_by_cars` / `is_osm_way_used_by_motorcycles`
  - `get_osm_way_speed` / `get_osm_motorcycle_way_speed`
  - `get_osm_car_direction_category` / `get_osm_motorcycle_direction_category`
  - `decode_osm_car_turn_restrictions` / `decode_osm_motorcycle_turn_restrictions`

### Post-implementation audit notes
- Added safety guards in travel-time calculation:
  - fail fast on out-of-range `routing_way_id`
  - fail fast on zero way speed (prevents divide-by-zero)
- Conditional validator now fails fast for partial/fragmented conditional outputs instead of silently skipping checks.
- Added explicit runtime warning in scripts for the known resolver caveat:
  - `conditional_restriction_resolver.cpp` currently rebuilds mappings with `is_osm_way_used_by_cars` (line 295), so motorcycle conditional extraction remains Phase-2 profile-awareness work.

---

## 2026-03-11 — CCH-Generator Phase 1 (implemented)

Implemented Parts 1-3 of `docs/CCH-Generator Plan.md` in `CCH-Generator/` as a RoutingKit-linked C++17 project with graph generation and structural validation tooling, plus pipeline helper scripts.

### Build/config updates
- Updated `CCH-Generator/CMakeLists.txt`:
  - switched to CMake 3.16 and C++17
  - added `cch_generator` and `validate_graph` executables
  - linked local RoutingKit (`../RoutingKit/include`, `../RoutingKit/lib`, `routingkit z pthread m`)
- Added `CCH-Generator/.gitignore` (`build/`, `cmake-build-*/`, `data/`)
- Removed stub `CCH-Generator/main.cpp`

### New generator and validator
- Added `CCH-Generator/src/graph_utils.h` with intended shared helpers:
  - `ensure_directory(...)`
  - `build_tail(...)`
  - `print_graph_stats(...)`
- Added `CCH-Generator/src/generate_graph.cpp`:
  - CLI: `cch_generator <input.osm.pbf> <output_dir> [--profile car|motorcycle]`
  - loads car/motorcycle RoutingKit graph from PBF
  - saves `first_out`, `head`, `travel_time`, `geo_distance`, `latitude`, `longitude`, `forbidden_turn_from_arc`, `forbidden_turn_to_arc`
  - prints graph statistics
- Added `CCH-Generator/src/validate_graph.cpp`:
  - CLI: `validate_graph <graph_dir> [--turn-expanded <line_graph_dir>] [--check-perm <perm_file> [expected_size]]`
  - implements standard checks (CSR/head bounds/self-loops/vector consistency/coord sanity/travel-time sanity/turn sorting+bounds/isolated nodes/connectivity)
  - implements turn-expanded checks (line node count, forbidden-turn transitions, U-turn transitions, transition consistency, optional `cch_exp_perm` validation)
  - adds permutation validity checks for external `--check-perm` files

### New scripts
- Added `CCH-Generator/scripts/run_pipeline.sh`:
  - generate + validate (car and motorcycle)
  - run IFC ordering scripts for both profiles
  - normalize `cch_perm_cuts` → `cch_exp_perm` for both profiles
  - validate both `cch_perm` and `cch_exp_perm` for both profiles
- Added `CCH-Generator/scripts/compare_profiles.sh`:
  - generate car/motorcycle outputs and report node/arc/turn deltas plus travel-time distribution summary

### Signature/existence checks and audit notes
- Verified intended RoutingKit APIs exist in `RoutingKit/include/routingkit/osm_simple.h`:
  - `simple_load_osm_car_routing_graph_from_pbf(...)`
  - `simple_load_osm_motorcycle_routing_graph_from_pbf(...)`
- Added compile-time signature bindings in `generate_graph.cpp` (`CarLoaderSignature`, `MotorcycleLoaderSignature`) to enforce expected loader signatures.
- Post-implementation logic audit fixes:
  - validator now marks self-loop, isolated-node, and connectivity checks as warnings/skipped when CSR/head-bound prerequisites fail (prevents misleading follow-up failures)
  - added line-graph transition consistency check to catch invalid non-consecutive arc transitions

---

## 2026-03-10 — CCH-Generator Plan (planned)

Created `docs/CCH-Generator Plan.md` — a 5-part plan to turn `CCH-Generator/` into a C++ graph generation + validation tool linked against RoutingKit.

### Scope
- **Part 0**: Build RoutingKit (human only)
- **Parts 1-3**: CMakeLists.txt, `generate_graph.cpp` (OSM PBF → binary), `validate_graph.cpp` (10+ structural checks)
- **Part 4**: Build InertialFlowCutter + run `flow_cutter_cch_order.sh` / `flow_cutter_cch_cut_order.sh` (human only)
- **Part 5**: End-to-end test with `hanoi.osm.pbf` for both car and motorcycle profiles

### Key decisions
- Link RoutingKit statically (`libroutingkit.a`) with `-lz -pthread -lm` (no external protobuf — RoutingKit bundles its own)
- C++17 (not C++20) to avoid `std::is_pod` deprecation warnings from RoutingKit headers
- Turn-expanded graph validation as secondary goal (line graph built by Rust, not CCH-Generator)

---

## 2026-03-10 — CCH Walkthrough Reorganized

Restructured `docs/CCH Walkthrough.md` from 8 linear steps into 3 CCH phases: **Contraction** (Load & Build), **Customization** (Load Weights), **Query**.

### Ordering fixes
- **Turn expansion** moved before CCH build (was Step 4, after Step 3 — but the line graph must be built *before* `CCH::fix_order_and_build`)
- **DirectedCCH** moved from Customization to Contraction phase (it's a structural transformation, not weight-dependent)
- **Node ordering** now follows turn expansion (you need the line graph before ordering its nodes)
- **Live traffic updates** and **result extraction** placed under Customization and Query phases respectively

### Factual corrections
- **Section 1.5** (old): claimed "RoutingKit's loader does NOT extract [turn restrictions]" — corrected: `SimpleOSMCarRoutingGraph` includes `forbidden_turn_from_arc`/`forbidden_turn_to_arc` and calls `decode_osm_car_turn_restrictions`. Only via-way restrictions are dropped.
- **Cut order script**: clarified that `flow_cutter_cch_cut_order.sh` produces an **arc** permutation (`cch_perm_cuts`) which becomes the line graph's node ordering when renamed to `cch_exp_perm`

### Structural improvements
- Architecture diagram updated to show correct dependency flow (line graph → InertialFlowCutter → CCH build → DirectedCCH)
- Complete pipeline example annotated with phase labels
- Removed broken `file:///` links from external machine

---

## 2026-03-10 — Motorcycle Routing Profile (implemented)

Implemented the profile from `docs/Motorcycle Profile Implementation.md` by extending RoutingKit's OSM profile and simple loader APIs. Added the 4 motorcycle callbacks in `osm_profile.{h,cpp}` and added `SimpleOSMMotorcycleRoutingGraph` plus `simple_load_osm_motorcycle_routing_graph_from_pbf` in `osm_simple.{h,cpp}`.

### Added profile methods
- `is_osm_way_used_by_motorcycles`
- `get_osm_motorcycle_direction_category`
- `get_osm_motorcycle_way_speed`
- `decode_osm_motorcycle_turn_restrictions`

### Post-implementation audit notes
- Added explicit `access` override handling (`motorcycle=yes` / `motor_vehicle=yes`) to avoid rejecting mode-specific overrides when generic `access` is restrictive.
- Added `except` parsing for fallback `restriction` relations so motorcycle and motor_vehicle exemptions are not incorrectly applied as forbidden turns.
- Signature and existence check completed for all intended methods and simple-loader API symbols.

---

## 2026-03-10 — Conditional Turn Restrictions (Phase 1)

Implemented the conditional turn restriction extraction tool as described in `docs/Conditional Turns Implementation.md`. Added 7 new files to RoutingKit (3 headers, 3 library sources, 1 tool source) — zero modifications to existing files.

### New files
- `include/routingkit/osm_condition_parser.h` / `src/osm_condition_parser.cpp` — Parses OSM opening-hours-style time conditions (`Mo-Fr 07:00-09:00`) into `TimeWindow` structs
- `include/routingkit/conditional_restriction_decoder.h` / `src/conditional_restriction_decoder.cpp` — Scans PBF for `restriction:conditional` tags and unconditional via-way restrictions
- `include/routingkit/conditional_restriction_resolver.h` / `src/conditional_restriction_resolver.cpp` — Resolves raw restrictions to `(from_arc, to_arc)` pairs using the existing graph
- `src/conditional_turn_extract.cpp` — CLI tool (`bin/conditional_turn_extract`) orchestrating the full pipeline

### Audit findings fixed
- **Via-way direction disambiguation**: Fixed incorrect use of overall restriction direction at junction B; now uses unique-candidate resolution for via-way junctions, with direction fallback only at junction A
- **Asymmetric via-node/via-way handling**: Fixed member parser to consistently reject restrictions with both via-node and via-way members

### Post-implementation audit fixes (2026-03-10)
- **Day-range wrapping bug**: `parse_day_spec` silently produced empty mask for wrapping ranges like `Fr-Mo` (loop `d=4; d<=0` never executed). Fixed with modular arithmetic `(d+1)%7`.
- **Dedup sort instability**: `std::sort` is unstable — unconditional entries could land after conditionals with the same `(from,to)` pair, leaving stale conditionals un-subsumed. Fixed by adding condition-emptiness as a tertiary sort key.
- **Dead cleanup in `find_junction_node`**: Removed unnecessary bit-reset loop on a function-local `vector<bool>` that is destroyed at return.

### Known limitations (by design, per Phase 1 scope)
- Multi-via-way chains deferred to Phase 2
- Public holiday (`PH`) conditions logged as unsupported
- Midnight-wrapping time windows only check the queried day (not previous day)
- `find_junction_node` allocates O(n) per call — acceptable for Phase 1 volumes
