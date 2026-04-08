# CHANGELOGS.md

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
