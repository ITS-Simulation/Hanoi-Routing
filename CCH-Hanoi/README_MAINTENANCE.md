# CCH-Hanoi Maintenance Guide

Practical guide for maintaining the CCH-Hanoi workspace without needing to
understand the routing algorithm core (`rust_road_router`).

---

## 1. Architecture at a Glance

```
                           +------------------+
                           | rust_road_router |  <-- DO NOT TOUCH
                           |  (engine crate)  |
                           +--------+---------+
                                    |
              Cargo path dependency |  (types + algorithm API)
                                    |
+-------------------------------+   |   +-------------------+
|         hanoi-core            |<--+   |   hanoi-gateway   |
| CCH build, query, spatial,   |       | Profile-aware     |
| geometry, traffic, cache     |       | reverse proxy     |
+------+-------+-------+------+       | (no engine dep)   |
       |       |       |              +-------------------+
       v       v       v
  hanoi-    hanoi-   hanoi-     hanoi-
  server     cli     tools      bench
```

**Key rule:** `rust_road_router` is an opaque dependency. You call its public
API; you never modify its source.

---

## 2. Crate Responsibilities

| Crate | Lines | Engine dep? | What it does |
|-------|------:|:-----------:|--------------|
| `hanoi-core` | ~2800 | **Direct** | CCH build/query lifecycle, spatial index, geometry, via-way restrictions, CCH cache, multi-route |
| `hanoi-server` | ~3000 | Shallow | Axum HTTP server, handlers, traffic overlay, camera overlay, route evaluation, UI |
| `hanoi-cli` | ~600 | None | CLI wrapper over `hanoi-core` (query, multi-query, info) |
| `hanoi-gateway` | ~520 | None | Profile-routing reverse proxy (car/motorcycle dispatch) |
| `hanoi-tools` | ~800 | Moderate | Offline utilities: `generate_line_graph`, `diagnose_turn` |
| `hanoi-bench` | ~1500 | Shallow | Benchmarking: CCH build, customize, query, spatial, HTTP |

### What "Shallow" engine dependency means

`hanoi-server`, `hanoi-bench` only import **type aliases** from the engine:

```rust
use rust_road_router::datastr::graph::Weight;    // = u32
use rust_road_router::datastr::graph::INFINITY;  // = u32::MAX
use rust_road_router::datastr::graph::NodeId;    // = u32
use rust_road_router::datastr::graph::EdgeId;    // = u32
```

These are all `u32`. No algorithm knowledge required.

---

## 3. Dependency Graph (Cargo)

```
hanoi-server  ──depends──>  hanoi-core  ──depends──>  rust_road_router
hanoi-cli     ──depends──>  hanoi-core
hanoi-bench   ──depends──>  hanoi-core
hanoi-tools   ──depends──>  rust_road_router  (direct, for I/O + graph types)
hanoi-gateway ──depends──>  (nothing in-workspace — pure HTTP proxy)
```

Third-party dependencies of note:

| Dependency | Used by | Purpose |
|-----------|---------|---------|
| `axum` | server, gateway | HTTP framework |
| `tokio` | server, gateway | Async runtime |
| `kiddo` | core | KD-tree for spatial snapping |
| `rayon` | core | Parallel CCH customization |
| `arrow` | server | Road arc manifest (Arrow IPC) |
| `clap` | server, cli, gateway | CLI argument parsing |
| `sha2` | core | CCH cache checksums |
| `memmap2` | core | Memory-mapped file I/O |

---

## 4. File-Level Risk Map

### Safe Zone (no engine knowledge needed)

These files can be modified freely. They handle HTTP, configuration, I/O
formatting, and business logic:

| File | Purpose |
|------|---------|
| `hanoi-server/src/handlers.rs` | HTTP request handlers |
| `hanoi-server/src/types.rs` | Request/response JSON structs |
| `hanoi-server/src/state.rs` | Shared server state, mpsc messages |
| `hanoi-server/src/traffic.rs` | Traffic weight overlay logic |
| `hanoi-server/src/camera_overlay.rs` | Speed camera overlay |
| `hanoi-server/src/route_eval.rs` | Route evaluation (GeoJSON replay) |
| `hanoi-server/src/ui.rs` | Embedded web UI |
| `hanoi-server/src/engine.rs` | Query dispatch (mpsc consumer) |
| `hanoi-server/src/main.rs` | Server startup and wiring |
| `hanoi-cli/src/main.rs` | CLI entry point |
| `hanoi-gateway/src/*` | Entire gateway crate |
| `hanoi-core/src/bounds.rs` | Bounding box validation |
| `hanoi-core/src/geometry.rs` | Turn angle computation |
| `hanoi-core/src/spatial.rs` | KD-tree spatial index |
| `hanoi-core/src/multi_route.rs` | Multi-route constants |
| `hanoi-core/src/graph.rs` | Graph data loading wrapper |
| `hanoi-bench/src/*` | All benchmarking infra |

### Caution Zone (touches engine API)

These files call the CCH build/query API. Changes here require understanding the
three-phase CCH lifecycle (see section 5):

| File | What it does with the engine |
|------|------------------------------|
| `hanoi-core/src/cch.rs` | Normal-graph CCH: build, customize, query |
| `hanoi-core/src/line_graph.rs` | Line-graph CCH: build, customize, query + path unpacking |
| `hanoi-core/src/cch_cache.rs` | Serialize/deserialize DirectedCCH to disk cache |
| `hanoi-core/src/via_way_restriction.rs` | Via-way turn restriction loading |
| `hanoi-tools/src/bin/generate_line_graph.rs` | Line graph construction from normal graph |
| `hanoi-tools/src/bin/diagnose_turn.rs` | Turn restriction diagnostics |

---

## 5. The CCH Lifecycle (What You Need to Know)

The engine API follows a rigid three-phase lifecycle. This is the only algorithm
concept you need to understand:

```
Phase 1: BUILD            Phase 2: CUSTOMIZE         Phase 3: QUERY
─────────────────         ──────────────────         ──────────────
Graph + ordering          CCH + weights              Customized CCH
       │                        │                          │
       v                        v                          v
CCH::fix_order_and_build  customize(&cch, &metric)   server.query(Query { from, to })
       │                        │                          │
       v                        v                          v
  CCH / DirectedCCH       CustomizedBasic             QueryResult { distance, path }
  (topology, immutable)   (up/down weights)           (shortest path answer)
```

- **Phase 1** is expensive (~30-60s). Done once at startup or loaded from cache.
- **Phase 2** is fast (~1-3s). Re-run when weights change (traffic updates).
- **Phase 3** is instant (~0.1-1ms per query).

The two wrapper structs in `hanoi-core`:

| Struct | Engine type inside | Phase 1 | Phase 2 | Phase 3 |
|--------|-------------------|---------|---------|---------|
| `CchContext` | `CCH` (undirected) | `load_and_build()` | `customize()` | `QueryEngine::query()` |
| `LineGraphCchContext` | `DirectedCCH` | `load_and_build()` | `customize()` | `LineGraphQueryEngine::query()` |

---

## 6. HTTP API Endpoints

All routes are defined in `hanoi-server/src/main.rs:341-363`.

### Query Router (concurrent with queries)

| Method | Path | Handler | Purpose |
|--------|------|---------|---------|
| POST | `/query` | `handle_query` | Shortest-path query (coordinates or node IDs) |
| POST | `/evaluate_routes` | `handle_evaluate_routes` | Evaluate imported GeoJSON routes |
| POST | `/reset_weights` | `handle_reset_weights` | Reset weights to baseline |
| GET | `/traffic_overlay` | `handle_traffic_overlay` | Current traffic weight state |
| GET | `/camera_overlay` | `handle_camera_overlay` | Speed camera data |
| GET | `/info` | `handle_info` | Server metadata (nodes, edges, mode) |
| GET | `/health` | `handle_health` | Health check |
| GET | `/ready` | `handle_ready` | Readiness probe |

### Customize Router (serialized, blocks queries during customization)

| Method | Path | Handler | Purpose |
|--------|------|---------|---------|
| POST | `/customize` | `handle_customize` | Apply new weight vector |

### UI Router (static assets)

| Method | Path | Handler |
|--------|------|---------|
| GET | `/` , `/ui` | `handle_index` |
| GET | `/assets/cch-query.css` | `handle_styles` |
| GET | `/assets/cch-query.js` | `handle_script` |

### Gateway API (hanoi-gateway)

| Method | Path | Purpose |
|--------|------|---------|
| POST | `/query?profile=<name>` | Dispatches to per-profile backend |
| GET | `/info?profile=<name>` | Backend metadata |
| GET | `/profiles` | List available profiles |

---

## 7. Common Maintenance Tasks

### Adding a new HTTP endpoint

1. Define request/response types in `hanoi-server/src/types.rs`
2. Add handler function in `hanoi-server/src/handlers.rs`
3. Register route in `hanoi-server/src/main.rs` (query_router or customize_router)
4. No engine knowledge needed.

### Modifying query response format

1. Edit `QueryAnswer` in `hanoi-core/src/cch.rs:18-40`
2. Update the JSON serialization in `hanoi-server/src/handlers.rs:handle_query`
3. The `QueryAnswer` struct is pure data — no engine types in it.

### Adding a new gateway profile

1. Add entry to the gateway YAML config file
2. Start a new `hanoi_server` instance pointing at the profile's graph directory
3. No code changes needed.

### Updating traffic overlay logic

1. Edit `hanoi-server/src/traffic.rs`
2. `TrafficOverlay` works with `Vec<Weight>` (just `Vec<u32>`) — no engine types.
3. The overlay is applied via `/customize` endpoint which triggers Phase 2.

### Adding a new CLI subcommand

1. Add variant to `Command` enum in `hanoi-cli/src/main.rs`
2. Use `hanoi-core` public API (`CchContext`, `LineGraphCchContext`, etc.)
3. No direct engine imports needed — CLI goes through `hanoi-core`.

### Updating spatial snapping logic

1. Edit `hanoi-core/src/spatial.rs`
2. Uses `kiddo` KD-tree — standard spatial indexing, no engine dependency.
3. Only engine types used: `NodeId`, `EdgeId` (both `u32`).

### Modifying camera overlay

1. Edit `hanoi-server/src/camera_overlay.rs`
2. Pure data processing (Arrow IPC + YAML config). No engine dependency.

---

## 8. Build & Test

```bash
# Build everything
cd CCH-Hanoi
cargo build --release --workspace

# Build specific binary
cargo build --release -p hanoi-server --bin hanoi_server
cargo build --release -p hanoi-cli --bin cch-hanoi
cargo build --release -p hanoi-gateway --bin hanoi_gateway

# Run tests (if any)
cargo test --workspace

# Run the server
./target/release/hanoi_server \
    --graph-dir Maps/data/hanoi_motorcycle/line_graph \
    --original-graph-dir Maps/data/hanoi_motorcycle/graph \
    --line-graph

# Run the gateway
./target/release/hanoi_gateway --config gateway.yaml
```

**Requires nightly Rust** — the workspace uses `edition = "2024"`.

---

## 9. What Triggers an Engine Rebuild?

You do NOT need to rebuild `rust_road_router` unless:

- The Rust toolchain version changes (nightly updates)
- You run `cargo clean`
- Someone modifies files in `rust_road_router/engine/` (forbidden by policy)

Normal CCH-Hanoi development only recompiles the CCH-Hanoi crates. The engine
is compiled once and cached by Cargo.

---

## 10. Troubleshooting

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| Server startup takes 30-60s | CCH build (Phase 1) running from scratch | Check if `cch_cache/` exists. First run is always slow. |
| "failed to load graph" panic | Wrong `--graph-dir` path or missing binary files | Verify `first_out`, `head`, `travel_time` exist in the directory |
| "via_way_split_map" missing | Line graph not generated | Re-run `generate_line_graph` on the graph directory |
| Query returns INFINITY weight | No path exists, or source/target snapped to disconnected component | Check coordinate validity, bounding box, spatial snap candidates |
| `/customize` blocks queries | By design — customization is serialized | Keep weight vectors small; customization takes ~1-3s |
| Gateway returns 400 | Unknown profile name | Check gateway YAML config for available profile keys |
| Cache rebuild on every startup | Source files changed (checksum mismatch) | Expected after re-running the data pipeline |

---

## 11. Files You Should Never Need to Touch

| Path | Reason |
|------|--------|
| `rust_road_router/` | Algorithm core — off-limits |
| `RoutingKit/` | C++ dependency — off-limits |
| `InertialFlowCutter/` | CCH ordering tool — off-limits |
| `hanoi-core/src/cch_cache.rs` | Serialization of engine internals — only changes if engine struct changes |
| `hanoi-core/src/cch.rs:67-88` | The `load_and_build` function — stable, build-customize-query pattern |
| `hanoi-core/src/line_graph.rs:69-198` | Same as above for line graph variant |

---

## 12. Engine Type Quick Reference

When reading CCH-Hanoi code, these engine types appear frequently. They are all
simple wrappers around integers:

| Engine type | Actual type | Meaning |
|-------------|-------------|---------|
| `Weight` | `u32` | Travel time in milliseconds |
| `NodeId` | `u32` | Node index in graph |
| `EdgeId` | `u32` | Edge index in graph |
| `EdgeIdT` | `u32` (newtype) | Type-safe edge ID |
| `INFINITY` | `u32::MAX` | No-path sentinel value |
| `NodeOrder` | `Arc<[u32]>` x 2 | CCH node permutation (ranks + order) |
| `CCH` | struct | Undirected CCH topology |
| `DirectedCCH` | struct | Directed CCH topology (for line graphs) |
| `CustomizedBasic` | struct | CCH + weights (result of Phase 2) |
| `FirstOutGraph` | struct | CSR graph (first_out + head + weight slices) |

---

## 13. Decision Log

| Decision | Rationale |
|----------|-----------|
| `rust_road_router` is read-only | Team lacks deep Rust algorithm expertise; engine is battle-tested |
| `hanoi-gateway` has zero engine dependency | Pure HTTP proxy; can be maintained/rewritten independently |
| `hanoi-cli` imports only from `hanoi-core` | Decouples CLI from engine; all algorithm access goes through core |
| CCH cache lives in `hanoi-core`, not engine | Cache logic is deployment-specific, not algorithm-specific |
| Type aliases (`Weight`, `NodeId`) imported from engine | Avoids divergence; cost is a Cargo dependency, not cognitive load |
