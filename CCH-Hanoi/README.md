# CCH-Hanoi Usage Guide

Comprehensive reference for operating, querying, and testing the CCH-Hanoi
routing system — from building the workspace to running production servers,
issuing queries, uploading custom weights, and validating results.

---

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Workspace Architecture](#2-workspace-architecture)
3. [Building the Workspace](#3-building-the-workspace)
4. [Data Prerequisites](#4-data-prerequisites)
5. [hanoi-core — Library API Reference](#5-hanoi-core--library-api-reference)
6. [hanoi-server — HTTP Routing Server](#6-hanoi-server--http-routing-server)
7. [hanoi-gateway — API Gateway](#7-hanoi-gateway--api-gateway)
8. [hanoi-cli — Command-Line Interface](#8-hanoi-cli--command-line-interface)
9. [hanoi-tools — Pipeline Utilities](#9-hanoi-tools--pipeline-utilities)
10. [hanoi-bench — Performance Benchmarking](#10-hanoi-bench--performance-benchmarking)
11. [HTTP API Reference](#11-http-api-reference)
12. [Weight Customization Guide](#12-weight-customization-guide)
13. [Testing Guide](#13-testing-guide)
14. [Operational Flowcharts](#14-operational-flowcharts)
15. [Troubleshooting](#15-troubleshooting)
16. [Logging Setup Guide](#16-logging-setup-guide)

---

## 1. System Overview

CCH-Hanoi is the Hanoi-specific integration layer for Customizable Contraction
Hierarchies (CCH) routing. It sits on top of `rust_road_router` (generic
algorithms) and provides:

- **Graph loading** from RoutingKit binary format
- **CCH construction** (metric-independent contraction hierarchy)
- **Customization** (painting weights onto the CCH, re-customizable at runtime)
- **Shortest-path queries** by node ID or GPS coordinates
- **Turn-aware routing** via line graph (turn-expanded) mode
- **HTTP server** with dual-port architecture (query + customization)
- **API gateway** for unified access to both graph modes
- **Performance benchmarking** with statistical analysis and regression detection

### Two Routing Modes


| Mode           | Graph Type          | CCH Type               | Turn Restrictions            | Use Case                                |
| -------------- | ------------------- | ---------------------- | ---------------------------- | --------------------------------------- |
| **Normal**     | Standard road graph | `CCH` (undirected)     | Not enforced                 | Fast routing without turn modeling      |
| **Line Graph** | Turn-expanded graph | `DirectedCCH` (pruned) | Enforced via graph structure | Accurate routing with turn restrictions |


**Normal mode**: Nodes = intersections, edges = road segments. Simpler, faster,
but ignores turn restrictions.

**Line graph mode**: Nodes = original road segments (edges), edges = legal turns
between consecutive segments. Forbidden turns are structurally absent. Requires
both the line graph data *and* the original graph metadata (for path mapping and
final-edge correction).

### Key Algorithmic Concepts

**Three-phase CCH pipeline**:

```
Phase 1: Contraction (once at startup)
  Input:  graph topology + node ordering (cch_perm)
  Output: CCH hierarchy structure (metric-independent)

Phase 2: Customization (on every weight change)
  Input:  CCH structure + edge weight vector (travel_time)
  Output: CustomizedBasic (upward/downward shortcut weights)

Phase 3: Query (per request)
  Input:  source node + target node + CustomizedBasic
  Output: shortest path distance + node sequence
```

**INFINITY sentinel**: `u32::MAX / 2 = 2,147,483,647`. Used as "no edge" in CCH
shortcuts. Triangle relaxation uses plain addition (`a + b`, not
`saturating_add`), so any input weight >= INFINITY would corrupt results. The
server rejects such weights at the `/customize` endpoint.

---

## 2. Workspace Architecture

```
CCH-Hanoi/
├── Cargo.toml                     # Workspace root: members = ["crates/*"]
├── rust-toolchain.toml            # Pins to nightly (required by rust_road_router)
└── crates/
    ├── hanoi-core/                # Library — graph loading, CCH, spatial indexing, queries
    ├── hanoi-server/              # Binary — dual-port HTTP server (Axum)
    ├── hanoi-gateway/             # Binary — API gateway proxy
    ├── hanoi-cli/                 # Binary — offline CLI for queries and info
    ├── hanoi-tools/               # Binaries — pipeline utilities (generate_line_graph)
    └── hanoi-bench/               # Library + Binaries — benchmarking and analysis
```

### Crate Dependency Graph

```
rust_road_router/engine     (upstream — generic algorithms, NEVER modified)
        ↑
   hanoi-core               (Hanoi-specific CCH implementation)
        ↑
   hanoi-cli                (CLI skin)
   hanoi-server             (HTTP server)
   hanoi-bench              (benchmarks)

   hanoi-tools              (independent — depends on rust_road_router directly)
   hanoi-gateway            (independent — HTTP proxy, no core dependency)
```

### Edition and Toolchain

All crates use **Rust edition 2024** on **nightly** toolchain. Nightly is
required because `rust_road_router/engine` uses
`#![feature(impl_trait_in_assoc_type)]`.

---

## 3. Building the Workspace

### Build Everything

```bash
cd CCH-Hanoi
cargo build --release --workspace
```

### Build Individual Crates

```bash
# Server
cargo build --release -p hanoi-server

# Gateway
cargo build --release -p hanoi-gateway

# CLI
cargo build --release -p hanoi-cli

# Line graph generator tool
cargo build --release -p hanoi-tools --bin generate_line_graph

# Benchmarks (all runners)
cargo build --release -p hanoi-bench
```

### Build Outputs


| Binary                | Crate         | Path                                 |
| --------------------- | ------------- | ------------------------------------ |
| `hanoi_server`        | hanoi-server  | `target/release/hanoi_server`        |
| `hanoi_gateway`       | hanoi-gateway | `target/release/hanoi_gateway`       |
| `cch-hanoi`           | hanoi-cli     | `target/release/cch-hanoi`           |
| `generate_line_graph` | hanoi-tools   | `target/release/generate_line_graph` |
| `bench_core`          | hanoi-bench   | `target/release/bench_core`          |
| `bench_server`        | hanoi-bench   | `target/release/bench_server`        |
| `bench_report`        | hanoi-bench   | `target/release/bench_report`        |


### Run Tests

```bash
cargo test --workspace
```

---

## 4. Data Prerequisites

Before running any CCH-Hanoi binary, you need graph data produced by the
upstream pipeline. See `docs/walkthrough/Manual Pipeline Guide.md` for the
full PBF-to-graph pipeline.

### Required Directory Layout

**Normal mode** — needs a single graph directory:

```
Maps/data/hanoi_car/
└── graph/
    ├── first_out                  # CSR offsets (Vec<u32>, n+1 elements)
    ├── head                       # CSR targets (Vec<u32>, m elements)
    ├── travel_time                # Edge weights in milliseconds (Vec<u32>, m elements)
    ├── latitude                   # Node latitudes (Vec<f32>, n elements)
    ├── longitude                  # Node longitudes (Vec<f32>, n elements)
    └── perms/
        └── cch_perm               # Node ordering for CCH (Vec<u32>, n elements)
```

**Line graph mode** — needs *both* the line graph and the original graph:

```
Maps/data/hanoi_car/
├── graph/                         # Original graph (for path mapping + final-edge correction)
│   ├── first_out
│   ├── head
│   ├── travel_time
│   ├── latitude
│   └── longitude
└── line_graph/                    # Turn-expanded graph
    ├── first_out                  # Line graph CSR (LG nodes = original edges)
    ├── head
    ├── travel_time
    ├── latitude                   # LG node coordinates (= original edge tail coords)
    ├── longitude
    └── perms/
        └── cch_perm               # Line graph node ordering
```

### File Format

All files are **headerless raw binary vectors** — no magic numbers, no length
prefix. Element count is inferred from `file_size / element_size`:


| File                                           | Element Type | Element Size |
| ---------------------------------------------- | ------------ | ------------ |
| `first_out`, `head`, `travel_time`, `cch_perm` | `u32`        | 4 bytes      |
| `latitude`, `longitude`                        | `f32`        | 4 bytes      |


### CSR (Compressed Sparse Row) Quick Reference

```
Node v's outgoing edges:  head[first_out[v] .. first_out[v+1]]
Edge e's weight:          travel_time[e]
Edge e's target:          head[e]
Number of nodes:          first_out.len() - 1
Number of edges:          head.len()
```

Invariants: `first_out[0] == 0`, `first_out[n] == m`,
`head.len() == travel_time.len()`.

### Quick Dimension Check

```python
import os

def graph_dims(graph_dir):
    n = os.path.getsize(f'{graph_dir}/first_out') // 4 - 1
    m = os.path.getsize(f'{graph_dir}/head') // 4
    print(f"Nodes: {n:,}  Edges: {m:,}")

graph_dims("Maps/data/hanoi_car/graph")        # ~276K nodes, ~655K edges
graph_dims("Maps/data/hanoi_car/line_graph")   # ~655K nodes, ~1.3M edges
```

---

## 5. hanoi-core — Library API Reference

The core library provides all routing logic. Other crates (server, CLI, bench)
are consumers of this API.

### 5.1 GraphData — Graph Loading

```rust
use hanoi_core::GraphData;

// Load graph from RoutingKit binary files
let graph = GraphData::load(Path::new("Maps/data/hanoi_car/graph"))?;

println!("Nodes: {}", graph.num_nodes());
println!("Edges: {}", graph.num_edges());

// Get a zero-copy CSR view for the CCH builder
let borrowed = graph.as_borrowed_graph();

// Get a CSR view with custom weights (for re-customization)
let custom = graph.as_borrowed_graph_with_weights(&custom_weights);
```

**Validation on load**: Checks `first_out[0] == 0`, monotonicity,
`head.len() == travel_time.len()`, all values in range. Returns
`std::io::Error` with `ErrorKind::InvalidData` on failure.

### 5.2 CchContext — Normal Graph CCH

```rust
use hanoi_core::CchContext;

// Phase 1: Load graph + build CCH topology (metric-independent)
let context = CchContext::load_and_build(
    Path::new("Maps/data/hanoi_car/graph"),
    Path::new("Maps/data/hanoi_car/graph/perms/cch_perm"),
)?;

// Phase 2: Customize with baseline weights
let customized = context.customize();

// Phase 2 (alternative): Customize with caller-provided weights
let customized = context.customize_with(&my_weights);
```

### 5.3 QueryEngine — Normal Graph Queries

```rust
use hanoi_core::QueryEngine;

// Create engine (initial customization + spatial index build)
let mut engine = QueryEngine::new(&context);

// Phase 3: Query by node IDs
if let Some(answer) = engine.query(source_node, target_node) {
    println!("Distance: {} ms ({:.1} m)", answer.distance_ms, answer.distance_m);
    println!("Path: {:?}", answer.path);         // Intersection node IDs
    println!("Coords: {:?}", answer.coordinates); // (lat, lng) pairs
}

// Query by GPS coordinates (snap-to-edge + fallback)
match engine.query_coords((21.028, 105.834), (21.006, 105.843)) {
    Ok(Some(answer)) => { /* route found */ }
    Ok(None)         => { /* no path between these points */ }
    Err(rejection)   => { /* coordinate validation failed */ }
}

// Live weight update (re-customizes the CCH)
engine.update_weights(&new_weights);
```

**Coordinate query flow**:

1. Snap source/destination to nearest graph edge (KD-tree + Haversine)
2. Select nearest endpoint based on projection parameter `t`
3. Run CCH query
4. If no path: try all 4 endpoint combinations (tail/head of both snaps)
5. Patch result coordinates with user's original query coordinates

### 5.4 LineGraphCchContext — Line Graph CCH

```rust
use hanoi_core::LineGraphCchContext;

// Load line graph + original graph metadata, build DirectedCCH
let context = LineGraphCchContext::load_and_build(
    Path::new("Maps/data/hanoi_car/line_graph"),     // line graph CSR
    Path::new("Maps/data/hanoi_car/graph"),           // original graph (for path mapping)
    Path::new("Maps/data/hanoi_car/line_graph/perms/cch_perm"),
)?;
```

### 5.5 LineGraphQueryEngine — Line Graph Queries

```rust
use hanoi_core::LineGraphQueryEngine;

let mut engine = LineGraphQueryEngine::new(&context);

// Query by line-graph node IDs (= original edge indices)
if let Some(answer) = engine.query(source_edge_id, target_edge_id) {
    // answer.path contains original intersection node IDs (not LG nodes)
    // answer.distance_ms includes final-edge correction
}

// Query by GPS coordinates (same interface as normal mode)
let result = engine.query_coords((21.028, 105.834), (21.006, 105.843))?;
```

**Line graph query internals**:

- CCH query returns line-graph node IDs (= original edge indices)
- Path mapping: `original_tail[lg_node]` for each node, plus
`original_head[last_edge]` for destination
- **Final-edge correction**: adds `original_travel_time[target_edge]` to
distance (the CCH distance covers arriving at the target segment but not
traversing it)
- Output format is identical to normal mode: intersection node IDs + coordinates

### 5.6 SpatialIndex — Coordinate Snapping

```rust
use hanoi_core::SpatialIndex;

let spatial = SpatialIndex::build(&lat, &lng, &first_out, &head);

// Snap a GPS coordinate to the nearest graph edge
let snap = spatial.snap_to_edge(21.028, 105.834);
// snap.edge_id  — CSR edge index of nearest edge
// snap.tail     — source node ID
// snap.head     — target node ID
// snap.t        — projection parameter [0, 1]: <0.5 closer to tail, >=0.5 closer to head
// snap.snap_distance_m — Haversine distance from query point to snapped point

// With validation (checks bbox + snap distance)
let validated = spatial.validated_snap("source", 21.028, 105.834, &config)?;
```

**Algorithm**: Hybrid KD-tree (k=10 nearest nodes) + Haversine perpendicular
distance to all outgoing edges of those nodes.

### 5.7 BoundingBox and Coordinate Validation

```rust
use hanoi_core::{BoundingBox, ValidationConfig, CoordRejection};
use hanoi_core::bounds::validate_coordinate;

let bbox = BoundingBox::from_coords(&lat, &lng);

let config = ValidationConfig {
    bbox_padding_m: 1000.0,       // 1 km padding around graph bbox
    max_snap_distance_m: 1000.0,  // reject snaps further than 1 km
};

// Validates: finite, in geographic range, within padded bbox
validate_coordinate("source", 21.028, 105.834, &bbox, &config)?;
```

**Rejection reasons** (`CoordRejection` enum):

- `NonFinite` — NaN or Infinity
- `InvalidRange` — lat outside [-90, 90] or lng outside [-180, 180]
- `OutOfBounds` — outside padded graph bounding box
- `SnapTooFar` — nearest edge is further than `max_snap_distance_m`

### 5.8 QueryAnswer — Result Type

```rust
pub struct QueryAnswer {
    pub distance_ms: u32,              // Total travel time in milliseconds
    pub distance_m: f64,               // Route distance via Haversine sum (meters)
    pub path: Vec<u32>,                // Ordered intersection node IDs
    pub coordinates: Vec<(f32, f32)>,  // (lat, lng) for each path node
}
```

For coordinate queries, `coordinates` includes the user's original query
coordinates prepended/appended, so `coordinates.len() == path.len() + 2`.

---

## 6. hanoi-server — HTTP Routing Server

### 6.1 Starting the Server

**Normal mode**:

```bash
hanoi_server \
  --graph-dir Maps/data/hanoi_car/graph \
  --query-port 8080 \
  --customize-port 9080
```

**Line graph mode**:

```bash
hanoi_server \
  --graph-dir Maps/data/hanoi_car/line_graph \
  --original-graph-dir Maps/data/hanoi_car/graph \
  --query-port 8081 \
  --customize-port 9081 \
  --line-graph
```

### 6.2 CLI Arguments


| Argument               | Default    | Description                                             |
| ---------------------- | ---------- | ------------------------------------------------------- |
| `--graph-dir`          | (required) | Path to graph directory                                 |
| `--original-graph-dir` | (none)     | Required for `--line-graph` mode                        |
| `--query-port`         | `8080`     | Port for query/info/health/ready API                    |
| `--customize-port`     | `9080`     | Port for weight upload API                              |
| `--line-graph`         | `false`    | Enable turn-expanded routing                            |
| `--log-format`         | `pretty`   | Log format: `pretty`, `full`, `compact`, `tree`, `json` |
| `--log-dir`            | (none)     | Directory for daily-rotated JSON log files              |


### 6.3 Dual-Port Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                       hanoi_server                          │
│                                                             │
│  Query Port (:8080)          Customize Port (:9080)         │
│  ┌──────────────────┐        ┌──────────────────────┐       │
│  │ POST /query      │        │ POST /customize      │       │
│  │ GET  /info       │        │   (binary body,      │       │
│  │ GET  /health     │        │    optional gzip)    │       │
│  │ GET  /ready      │        └──────────┬───────────┘       │
│  └───────┬──────────┘                   │                   │
│          │ mpsc channel                 │ watch channel      │
│          ▼                              ▼                   │
│  ┌─────────────────────────────────────────────────┐        │
│  │            Background Engine Thread              │        │
│  │  ┌─────────────────────────────────────────┐    │        │
│  │  │ QueryEngine / LineGraphQueryEngine       │    │        │
│  │  │   → CCH customization                   │    │        │
│  │  │   → shortest-path queries               │    │        │
│  │  └─────────────────────────────────────────┘    │        │
│  └─────────────────────────────────────────────────┘        │
└─────────────────────────────────────────────────────────────┘
```

**Why two ports**: The query port serves external JSON API consumers. The
customize port accepts raw binary weight vectors from the internal data pipeline.
Separating them allows independent access control and body size limits (64 MB on
customize, standard limits on query).

### 6.4 Engine Background Thread

The engine runs in a dedicated OS thread (not a Tokio task) to avoid blocking
the async runtime during CPU-intensive CCH operations.

**Engine loop**:

```
loop:
  1. Check watch channel (non-blocking):
     → if new weights: set customization_active=true, re-customize, set false
  2. Wait for query message (50ms timeout):
     → on message: dispatch query, send result via oneshot
     → on timeout: loop back (allows periodic customization checks)
     → on channel close: exit loop, set engine_alive=false
```

**Watch channel semantics**: Last-writer-wins. If multiple `/customize` requests
arrive during an ongoing customization, only the latest weight vector is applied.
This is intentional for live-traffic updates.

### 6.5 Graceful Shutdown

The server handles SIGINT and SIGTERM:

1. Broadcasts shutdown to both listeners
2. Both ports stop accepting new connections
3. In-flight requests drain gracefully
4. 30-second timeout — if draining hangs, force-exits with code 1

### 6.6 Logging

The server has the most comprehensive logging capabilities of any CCH-Hanoi
binary. It supports all five output formats, file-based log persistence with
daily rotation, and HTTP request tracing via `tower-http`.

For full details on all logging options, formats, environment variables, and
per-binary differences, see [Section 16: Logging Setup Guide](#16-logging-setup-guide).

**Quick start examples**:

```bash
# Default: info level, HTTP debug, pretty output to stderr
hanoi_server --graph-dir Maps/data/hanoi_car/graph

# JSON logs to stderr + daily-rotated file logs
hanoi_server --graph-dir Maps/data/hanoi_car/graph --log-format json --log-dir /var/log/hanoi/

# Debug everything
RUST_LOG=debug hanoi_server --graph-dir Maps/data/hanoi_car/graph

# Tree-structured hierarchical output (server only)
hanoi_server --graph-dir Maps/data/hanoi_car/graph --log-format tree
```

---

## 7. hanoi-gateway — API Gateway

The gateway provides a unified entry point that routes queries to the
appropriate backend based on the request's **routing profile** (e.g. `car`,
`motorcycle`). All backend configuration is defined in a YAML config file.

### 7.1 Starting the Gateway

```bash
hanoi_gateway --config gateway.yaml
hanoi_gateway --config gateway.yaml --port 9000   # override port
```

### 7.2 CLI Arguments

| Argument   | Default          | Description                                  |
| ---------- | ---------------- | -------------------------------------------- |
| `--config` | `gateway.yaml`   | Path to the YAML config file                 |
| `--port`   | (from config)    | Override the port defined in the config file  |

### 7.3 YAML Configuration

The YAML config file is the **single source of truth** for the gateway. It
controls the listen port, backend timeout, logging, and — most importantly —
the set of routing profiles and their backend URLs.

```yaml
port: 50051
backend_timeout_secs: 30       # 0 to disable; default 30
log_format: pretty             # pretty | full | compact | tree | json
# log_file: /var/log/gw.json  # omit to disable file logging

profiles:
  car:
    backend_url: "http://localhost:8080"
  motorcycle:
    backend_url: "http://localhost:8081"
```

**Config fields:**

| Field                 | Required | Default  | Description                                         |
| --------------------- | -------- | -------- | --------------------------------------------------- |
| `port`                | yes      | —        | Gateway listen port                                 |
| `backend_timeout_secs`| no       | `30`     | HTTP client timeout for backend requests (0 = none) |
| `log_format`          | no       | `pretty` | Log output format: pretty, full, compact, tree, json|
| `log_file`            | no       | (none)   | Also write logs to file in JSON format              |
| `profiles`            | yes      | —        | Map of profile name → backend config (≥ 1 entry)   |
| `profiles.<name>.backend_url` | yes | —   | Base URL of the backend routing server              |

The gateway does not care whether a backend uses a normal graph or line graph —
that is the backend's concern. This decouples the gateway API from graph
topology details. Adding a new profile (e.g. `truck`, `bicycle`) only requires
a new entry in the YAML and a running backend server.

**Logging to file** — when `log_file` is set, logs are written to **both**
stderr and the file simultaneously. Stderr uses `log_format`; the file is
always newline-delimited JSON:

```bash
# Parse the JSON log file with jq
jq 'select(.fields.message | contains("ready"))' /var/log/gw.json
```

### 7.4 Gateway Endpoints

| Method | Path        | Description                                                 |
| ------ | ----------- | ----------------------------------------------------------- |
| POST   | `/query`    | Route query — `?profile=<name>` selects the backend         |
| GET    | `/info`     | Backend metadata — `?profile=<name>` (optional)             |
| GET    | `/profiles` | List all available routing profiles                         |

`GET /profiles` response:

```json
{ "profiles": ["car", "motorcycle"] }
```

### 7.5 Routing Architecture

```
             Client
               │
       POST /query?profile=car  {...}
               │
               ▼
        ┌─────────────┐
        │   Gateway    │ :50051
        │   (:50051)   │
        └──┬───────┬───┘
           │       │
   profile         profile
   ="car"          ="motorcycle"
           │       │
           ▼       ▼
     ┌─────────┐ ┌─────────┐
     │ Server  │ │ Server  │
     │ :8080   │ │ :8081   │
     │  (car)  │ │ (moto)  │
     └─────────┘ └─────────┘
```

**What the gateway proxies**: `/query` (POST) and `/info` (GET) only.

**What it does NOT proxy**: `/customize`, `/health`, `/ready`. Customization
goes directly from the pipeline to each server's customize port. Health/ready
checks are done by orchestration tools directly against each backend.

### 7.6 Error Propagation

The gateway preserves backend HTTP status codes. If the backend returns 400
(e.g., coordinate validation failure) or 500, the gateway forwards that exact
status and JSON body to the client.

Unknown profiles return HTTP 400 with the list of valid options:

```json
{
  "error": "unknown profile: truck",
  "available_profiles": ["car", "motorcycle"]
}
```

---

## 8. hanoi-cli — Command-Line Interface

The CLI runs queries and info lookups entirely in-process — no server required.
Useful for one-off queries, validation, and scripting.

### 8.1 Query Command

**Basic usage — By node IDs**:

```bash
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-node 1000 \
  --to-node 5000
```

**By coordinates**:

```bash
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843
```

**Line graph mode**:

```bash
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --line-graph \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843
```

**Output formatting and file options**:

The `--output-format` flag controls output format (default: `geojson`):

```bash
# GeoJSON format (RFC 7946) — default, suitable for mapping libraries
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843 \
  --output-format geojson

# JSON format — coordinates as [lat, lng]
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843 \
  --output-format json
```

Results are always written to a file. If `--output-file` is omitted, a
timestamped file is auto-generated in the current directory (e.g.,
`query_2026-03-19T143052.geojson`). The extension matches the output format.
A summary (distance, node count, output path) is logged to stderr.

```bash
# Explicit output file
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843 \
  --output-file result.geojson

# Auto-generated file (creates query_<timestamp>.geojson)
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843
```

**GeoJSON output example** (default format):

```json
{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "geometry": {
        "type": "LineString",
        "coordinates": [[105.834, 21.028], [105.836, 21.025], ...]
      },
      "properties": {
        "distance_ms": 142300,
        "distance_m": 3842.7,
        "path_nodes": [1523, 1524, 1530, 1547, 1563]
      }
    }
  ]
}
```

**JSON output example** (legacy format):

```json
{
  "distance_ms": 142300,
  "distance_m": 3842.7,
  "path_nodes": [1523, 1524, 1530, 1547, 1563],
  "coordinates": [[21.028, 105.834], [21.025, 105.836], ...]
}
```

**Logging to file** — concurrent output to stderr and file:

By default, logs go to stderr with color formatting. When `--log-file` is specified,
logs are written to **both** stderr and the file simultaneously. The file always uses
**JSON** format (newline-delimited), which is machine-readable and immune to ANSI
escape codes. The `--log-format` flag only affects stderr output.

The `--log-file` flag is a top-level flag — it must come **before** the subcommand:

```bash
# Logs to both stderr (pretty, colored) and query.log (JSON)
cch-hanoi --log-file query.log query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843

# --log-format only affects stderr; file is always JSON
cch-hanoi --log-format compact --log-file query.log query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843

# Parse the JSON log file with jq
jq '.fields.message' query.log
```

**Exit codes**: 0 = success, 1 = no path found, 2 = coordinate validation failure.

### 8.2 Info Command

```bash
cch-hanoi info --data-dir Maps/data/hanoi_car
cch-hanoi info --data-dir Maps/data/hanoi_car --line-graph
```

**Output**:

```json
{
  "graph_type": "normal",
  "graph_dir": "Maps/data/hanoi_car/graph",
  "num_nodes": 276372,
  "num_edges": 654787
}
```

### 8.3 Directory Convention

The `--data-dir` flag points to the **parent** data directory. The CLI
automatically appends `/graph` for normal mode, `/line_graph` and `/graph` for
line graph mode:

```
--data-dir Maps/data/hanoi_car
  Normal:     loads Maps/data/hanoi_car/graph/
  Line graph: loads Maps/data/hanoi_car/line_graph/ + Maps/data/hanoi_car/graph/
```

---

## 9. hanoi-tools — Pipeline Utilities

### 9.1 generate_line_graph

Converts a base road graph into a turn-expanded line graph.

```bash
# Output to <graph_dir>/line_graph/ (default)
generate_line_graph Maps/data/hanoi_car/graph

# Output to explicit directory
generate_line_graph Maps/data/hanoi_car/graph Maps/data/hanoi_car/line_graph
```

**CLI Arguments**:


| Argument       | Required | Description                                          |
| -------------- | -------- | ---------------------------------------------------- |
| `<graph_dir>`  | Yes      | Input graph directory (positional)                   |
| `<output_dir>` | No       | Output directory (default: `<graph_dir>/line_graph`) |
| `--log-format` | No       | Log format (default: `pretty`)                       |


**Input files** (from `<graph_dir>/`):


| File                      | Required | Description                   |
| ------------------------- | -------- | ----------------------------- |
| `first_out`               | Yes      | CSR offsets                   |
| `head`                    | Yes      | CSR targets                   |
| `travel_time`             | Yes      | Edge weights (milliseconds)   |
| `latitude`                | Yes      | Node latitudes                |
| `longitude`               | Yes      | Node longitudes               |
| `forbidden_turn_from_arc` | Yes      | Sorted forbidden turn sources |
| `forbidden_turn_to_arc`   | Yes      | Sorted forbidden turn targets |


**Output files** (to output directory):


| File          | Description                                                        |
| ------------- | ------------------------------------------------------------------ |
| `first_out`   | Line graph CSR offsets                                             |
| `head`        | Line graph edge targets                                            |
| `travel_time` | Line graph weights: `original_travel_time[e1] + turn_cost(e1, e2)` |
| `latitude`    | Line graph node coordinates (= tail node of original edge)         |
| `longitude`   | Line graph node coordinates                                        |


**What it does**:

1. Loads the base graph and forbidden turns
2. Builds a tail array (edge → source node reverse lookup)
3. Enumerates all possible turns at each intersection
4. Filters forbidden turns (sorted merge-scan, O(1) amortized) and U-turns
5. Writes the expanded graph

**Expected dimensions** (Hanoi car graph):

- Input: ~276K nodes, ~655K edges, ~403 forbidden turns
- Output: ~655K nodes (= original edges), ~1.3M edges (valid turns)

---

## 10. hanoi-bench — Performance Benchmarking

### 10.1 Core Benchmarks (No Server Required)

```bash
bench_core \
  --graph-dir Maps/data/hanoi_car/graph \
  --query-count 1000 \
  --iterations 10 \
  --output core_results.json
```

**What it benchmarks** (in sequence):

1. CCH build (`CchContext::load_and_build`)
2. Customization (`customize()`)
3. KD-tree build (`SpatialIndex::build`)
4. Node-ID queries (`query()`)
5. Coordinate queries (`query_coords()`)
6. Snap-to-edge (`snap_to_edge()`)

**CLI Arguments**:


| Argument             | Default                      | Description                  |
| -------------------- | ---------------------------- | ---------------------------- |
| `--graph-dir`        | (required)                   | Graph directory path         |
| `--perm-path`        | `<graph_dir>/perms/cch_perm` | CCH ordering file            |
| `--query-count`      | `1000`                       | Queries per iteration        |
| `--iterations`       | `10`                         | Measured iterations          |
| `--warmup`           | `3`                          | Warmup iterations            |
| `--seed`             | `42`                         | RNG seed for reproducibility |
| `--generate-queries` | (none)                       | Generate N random queries    |
| `--save-queries`     | (none)                       | Save queries to JSON file    |
| `--queries`          | (none)                       | Load queries from JSON file  |
| `--output`           | `core_results.json`          | Results output file          |
| `--log-name`         | `bench_core`                 | Custom log file name prefix  |


### 10.2 Server Benchmarks (Requires Running Server)

```bash
# Sequential queries
bench_server \
  --url http://localhost:8080 \
  --queries 1000 \
  --graph-dir Maps/data/hanoi_car/graph

# Concurrent load test
bench_server \
  --url http://localhost:8080 \
  --queries 1000 \
  --concurrency 10 \
  --graph-dir Maps/data/hanoi_car/graph
```

**What it benchmarks**:

1. `GET /info` latency
2. `POST /query` sequential latency
3. `POST /query` concurrent throughput (with N clients)

**CLI Arguments**:


| Argument        | Default                 | Description                    |
| --------------- | ----------------------- | ------------------------------ |
| `--url`         | `http://localhost:8080` | Server URL                     |
| `--queries`     | `1000`                  | Number of queries              |
| `--concurrency` | `1`                     | Concurrent clients             |
| `--query-file`  | (none)                  | Load query dataset from JSON   |
| `--graph-dir`   | (none)                  | Graph dir for query generation |
| `--seed`        | `42`                    | RNG seed                       |
| `--output`      | `bench_results.json`    | Results file                   |
| `--log-name`    | `bench_server`          | Custom log file name prefix    |


### 10.3 Report Generation and Comparison

```bash
# Generate report from results
bench_report --input core_results.json --format table

# Compare two runs for regression detection
bench_report \
  --baseline previous_results.json \
  --current current_results.json \
  --threshold 10
```

Exit code 1 if any benchmark regressed by more than `--threshold` percent.
All three binaries also accept `--log-name <PREFIX>` to customize the log file
name prefix (default: the binary name).

**Statistics computed**: min, max, mean, median (p50), p95, p99, std_dev,
throughput (QPS), success rate.

**Output formats**: `table` (human-readable), `json` (CI integration), `csv`
(spreadsheet).

### 10.4 Benchmark Logging

All bench binaries automatically create a log file in the current directory on
every run. No CLI flag is required to enable this — it is always on.

- **Stderr**: Compact format for human-readable progress
- **File**: JSON format for machine-readable post-analysis
- **Filename**: `{binary_name}_{timestamp}.log` (e.g., `bench_core_2026-03-19T143052.log`)
- **Custom prefix**: `--log-name my_run` → `my_run_2026-03-19T143052.log`
- **Filter**: Controlled via `RUST_LOG` env var (default: `info`)

Parse log files with `jq`:

```bash
# View all events
cat bench_core_2026-03-19T143052.log | jq .

# Extract benchmark phase timings
cat bench_core_2026-03-19T143052.log | jq 'select(.fields.message != null) | .fields.message'
```

### 10.5 Criterion Micro-Benchmarks

```bash
# CCH benchmarks
cargo bench --bench cch_bench -p hanoi-bench

# Spatial benchmarks
cargo bench --bench spatial_bench -p hanoi-bench
```

### 10.6 Reproducible Query Datasets

```bash
# Generate and save a query dataset
bench_core --graph-dir ... --generate-queries 5000 --save-queries queries.json

# Reuse it across runs
bench_core --graph-dir ... --queries queries.json --output run1.json
bench_core --graph-dir ... --queries queries.json --output run2.json
bench_report --baseline run1.json --current run2.json
```

---

## 11. HTTP API Reference

### 11.1 POST /query — Shortest-Path Query

**Port**: Query port (default 8080, or gateway 50051)

**Request (coordinate-based)**:

`POST /query` (default: GeoJSON) | `POST /query?format=json` (plain JSON) | `POST /query?colors` (GeoJSON with simplestyle-spec colors).

```json
{
  "from_lat": 21.028,
  "from_lng": 105.834,
  "to_lat": 21.006,
  "to_lng": 105.843
}
```

**Request (node-ID-based)**:

```json
{
  "from_node": 1000,
  "to_node": 5000
}
```

**Request via gateway** (profile in query param):

```
POST /query?profile=car
Content-Type: application/json

{
  "from_lat": 21.028,
  "from_lng": 105.834,
  "to_lat": 21.006,
  "to_lng": 105.843
}
```

**Response (default — GeoJSON)** — 200 OK:

```json
{
  "type": "FeatureCollection",
  "features": [{
    "type": "Feature",
    "geometry": {
      "type": "LineString",
      "coordinates": [[105.834, 21.028], [105.836, 21.025], ...]
    },
    "properties": {
      "distance_ms": 142300,
      "distance_m": 3842.7
    }
  }]
}
```

Note: GeoJSON coordinates are `[longitude, latitude]` per RFC 7946 (reversed
from internal convention). When no path is found, `"geometry": null`.

**Response (JSON format)** — `POST /query?format=json`:

```json
{
  "distance_ms": 142300,
  "distance_m": 3842.7,
  "path_nodes": [1523, 1524, 1530, 1547, 1563],
  "coordinates": [[21.028, 105.834], [21.025, 105.836], ...]
}
```

When no path is found: `distance_ms`/`distance_m` are `null`, arrays are empty.

**Error Response** — 400 Bad Request (coordinate validation):

```json
{
  "error": "coordinate_validation_failed",
  "message": "source coordinate (91.0, 105.8) is outside valid geographic range",
  "details": {
    "label": "source",
    "lat": 91.0,
    "lng": 105.8,
    "reason": "InvalidRange"
  }
}
```

### 11.2 POST /customize — Upload Weight Vector

**Port**: Customize port (default 9080)

**Request**: Raw binary body — little-endian `[u32; num_edges]`

```bash
# Upload weights with curl
curl -X POST http://localhost:9080/customize \
  --data-binary @travel_time \
  -H "Content-Type: application/octet-stream"

# With gzip compression
gzip -c travel_time | curl -X POST http://localhost:9080/customize \
  --data-binary @- \
  -H "Content-Type: application/octet-stream" \
  -H "Content-Encoding: gzip"
```

**Validation**:

- Body size must equal `num_edges * 4` bytes
- All weight values must be `< INFINITY` (< 2,147,483,647)
- Max body size: 64 MB

**Success Response** — 200 OK:

```json
{
  "accepted": true,
  "message": "customization queued"
}
```

**Error Response** — 400 Bad Request:

```json
{
  "accepted": false,
  "message": "expected 2619148 bytes (654787 edges x 4), got 1000"
}
```

```json
{
  "accepted": false,
  "message": "weight[42] = 2147483647 exceeds maximum allowed value (2147483646)"
}
```

**Important**: `/customize` returns 200 before customization completes. The
handler validates and queues; the engine thread applies asynchronously. To
confirm completion, poll `GET /info` and watch `customization_active` transition
from `true` to `false`.

### 11.3 GET /info — Graph Metadata

**Port**: Query port (default 8080)

```json
{
  "graph_type": "normal",
  "num_nodes": 276372,
  "num_edges": 654787,
  "customization_active": false,
  "bbox": {
    "min_lat": 20.899,
    "max_lat": 21.098,
    "min_lng": 105.701,
    "max_lng": 105.952
  }
}
```

Via gateway: `GET /info?profile=car`

### 11.4 GET /health — Operational Metrics

**Port**: Query port (default 8080). Always returns 200.

```json
{
  "status": "ok",
  "uptime_seconds": 3600,
  "total_queries_processed": 15230,
  "customization_active": false
}
```

### 11.5 GET /ready — Readiness Check

**Port**: Query port (default 8080).

- **200 OK**: `{"ready": true}` — engine thread alive
- **503 Service Unavailable**: `{"ready": false}` — engine thread has died

### 11.6 Endpoint Summary Table


| Endpoint     | Method | Port      | Purpose             | Status Codes |
| ------------ | ------ | --------- | ------------------- | ------------ |
| `/query`     | POST   | Query     | Route query         | 200, 400     |
| `/info`      | GET    | Query     | Graph metadata      | 200          |
| `/health`    | GET    | Query     | Operational metrics | 200          |
| `/ready`     | GET    | Query     | Readiness probe     | 200, 503     |
| `/customize` | POST   | Customize | Weight upload       | 200, 400     |


---

## 12. Weight Customization Guide

### 12.1 How Customization Works

```
On-disk travel_time: [1000, 2000, 3000, 5000, 8000]    ← never changes
                          │
                          │ loaded once at startup (baseline)
                          ▼
Baseline weights:    [1000, 2000, 3000, 5000, 8000]    ← never mutated
                          │
                          │ POST /customize with new vector
                          ▼
Replacement vector:  [1000, 9999, 3000, 5000, 8000]    ← full replacement
                          │
                          │ CCH customize() (Phase 2)
                          ▼
CustomizedBasic:     upward_weights[...]                 ← what queries use
                     downward_weights[...]
```

**Key points**:

- Each `/customize` call sends a **complete** replacement weight vector
- Updates are NOT cumulative — each call starts fresh
- The baseline (on-disk) `travel_time` is never modified
- Weight vector length must exactly equal `num_edges` (check via `GET /info`)

### 12.2 Generating Test Weights

**Python — quick generation**:

```python
import numpy as np

def read_u32(path):
    return np.fromfile(path, dtype=np.uint32)

def write_u32(path, data):
    data.astype(np.uint32).tofile(path)

# Read graph dimensions
graph_dir = "Maps/data/hanoi_car/graph"
head = read_u32(f"{graph_dir}/head")
m = len(head)
print(f"Graph has {m:,} edges")

# Strategy 1: Uniform weights (10 seconds per edge)
weights = np.full(m, 10_000, dtype=np.uint32)

# Strategy 2: Random bounded (1s–60s per edge, seed=42)
rng = np.random.default_rng(seed=42)
weights = rng.integers(1_000, 60_000, size=m, dtype=np.uint32)

# Strategy 3: Distance-based (from coordinates)
lat = np.fromfile(f"{graph_dir}/latitude", dtype=np.float32)
lng = np.fromfile(f"{graph_dir}/longitude", dtype=np.float32)
first_out = read_u32(f"{graph_dir}/first_out")
# (compute haversine per edge, scale to milliseconds)

# Write to file
write_u32(f"{graph_dir}/travel_time", weights)
```

**Rust — programmatic generation**:

```rust
use std::fs::File;
use std::io::Write;

fn write_u32_vec(path: &str, data: &[u32]) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    let bytes = bytemuck::cast_slice(data);
    file.write_all(bytes)
}

// Uniform 10-second weights
let weights = vec![10_000u32; num_edges];
write_u32_vec("travel_time", &weights)?;
```

### 12.3 Uploading Weights to a Running Server

```bash
# Generate random weights
python3 -c "
import numpy as np
m = $(python3 -c "import os; print(os.path.getsize('Maps/data/hanoi_car/graph/head') // 4)")
rng = np.random.default_rng(seed=42)
weights = rng.integers(1_000, 60_000, size=m, dtype=np.uint32)
weights.tofile('test_weights.bin')
print(f'Wrote {m} weights to test_weights.bin')
"

# Upload to server
curl -X POST http://localhost:9080/customize \
  --data-binary @test_weights.bin \
  -H "Content-Type: application/octet-stream"

# Wait for customization to complete
sleep 0.2
curl -s http://localhost:8080/info | python3 -m json.tool
```

### 12.4 Weight Constraints


| Constraint        | Value                        | Reason                                             |
| ----------------- | ---------------------------- | -------------------------------------------------- |
| Type              | `u32` (little-endian)        | RoutingKit binary format                           |
| Min value         | 0 (use with caution)         | Zero-weight edges can cause algorithmic edge cases |
| Max value         | 2,147,483,646 (INFINITY - 1) | Server rejects >= INFINITY                         |
| Recommended range | 1,000 – 10,000,000           | 1 second – 2.7 hours per edge                      |
| Unit              | Milliseconds                 | `tt_units_per_s = 1000`                            |
| Vector length     | Exactly `num_edges`          | Must match graph topology                          |


### 12.5 Line Graph Weight Considerations

For line graph weights, there are two approaches:

**Option A: Generate from normal graph** (recommended for consistency)

1. Write test `travel_time` to the normal graph directory
2. Run `generate_line_graph` — it derives line graph weights automatically
3. Line graph weight formula: `travel_time[turn_edge] = original_travel_time[e1]`

**Option B: Direct line graph weights**

1. Write `travel_time` directly to the line graph directory
2. Must have exactly `num_line_graph_edges` elements
3. **Consistency rule**: `line_weight(e1 → e2) = original_travel_time[e1]` if
  you want normal and line graph results to agree

---

## 13. Testing Guide

### 13.1 Testing Strategy Overview

The testing workflow progresses through three stages:

```
Stage 1: Default Weights
  → Use the on-disk travel_time (from OSM pipeline)
  → Validates the full stack works end-to-end
  → Baseline behavior verification

Stage 2: Randomized Fixed-Seed Weights
  → Generate deterministic random weights (seed=42)
  → Same input across runs = reproducible results
  → Stress-tests weight diversity

Stage 3: Multiple Weight Sets
  → Generate several fixed-seed weight sets (seed=1, 2, 3, ...)
  → Compare results across customization cycles
  → Validates re-customization correctness
```

### 13.2 Stage 1: Test with Default Weights

**CLI (no server)**:

```bash
# Normal mode — query by coordinates
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843

# Line graph mode
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --line-graph \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843

# Compare: both modes should produce similar distances
# (line graph may differ due to turn restriction enforcement)
```

**Server**:

```bash
# Start server with default weights
hanoi_server --graph-dir Maps/data/hanoi_car/graph

# Query
curl -s -X POST http://localhost:8080/query \
  -H "Content-Type: application/json" \
  -d '{"from_lat": 21.028, "from_lng": 105.834, "to_lat": 21.006, "to_lng": 105.843}' \
  | python3 -m json.tool

# Check info
curl -s http://localhost:8080/info | python3 -m json.tool

# Check health
curl -s http://localhost:8080/health | python3 -m json.tool
```

**What to verify**:

- Response is a GeoJSON FeatureCollection with a single Feature
- `features[0].properties.distance_ms` > 0 (route exists)
- `features[0].geometry.coordinates` is non-empty with `[lng, lat]` pairs (RFC 7946 order)
- All coordinates are within the Hanoi bounding box
- `GET /info` returns correct `num_nodes` and `num_edges`
- `GET /ready` returns `{"ready": true}`

### 13.3 Stage 2: Test with Randomized Fixed-Seed Weights

**Generate a single reproducible weight set**:

```python
import numpy as np
import os

graph_dir = "Maps/data/hanoi_car/graph"
m = os.path.getsize(f"{graph_dir}/head") // 4

# Fixed seed = reproducible across runs
rng = np.random.default_rng(seed=42)
weights = rng.integers(1_000, 60_000, size=m, dtype=np.uint32)
weights.tofile("test_weights_seed42.bin")
print(f"Generated {m:,} weights, range [{weights.min():,}, {weights.max():,}]")
```

**Upload and test**:

```bash
# Upload randomized weights
curl -X POST http://localhost:9080/customize \
  --data-binary @test_weights_seed42.bin \
  -H "Content-Type: application/octet-stream"

# Wait for customization
sleep 0.3

# Run same queries — distances should differ from Stage 1
curl -s -X POST http://localhost:8080/query \
  -H "Content-Type: application/json" \
  -d '{"from_lat": 21.028, "from_lng": 105.834, "to_lat": 21.006, "to_lng": 105.843}' \
  | python3 -m json.tool
```

**What to verify**:

- Routes still found (no INFINITY contamination)
- Distances differ from baseline (customization was applied)
- Path may differ (different weights → different shortest paths)
- `GET /info` shows `customization_active: false` after settling

### 13.4 Stage 3: Multiple Weight Sets (Re-Customization Validation)

```python
import numpy as np
import requests
import time

graph_dir = "Maps/data/hanoi_car/graph"
m = os.path.getsize(f"{graph_dir}/head") // 4

# Test query
query = {
    "from_lat": 21.028, "from_lng": 105.834,
    "to_lat": 21.006, "to_lng": 105.843
}

results = {}

for seed in [1, 2, 3, 42, 100]:
    # Generate weights
    rng = np.random.default_rng(seed=seed)
    weights = rng.integers(1_000, 60_000, size=m, dtype=np.uint32)

    # Upload
    resp = requests.post(
        "http://localhost:9080/customize",
        data=weights.tobytes(),
        headers={"Content-Type": "application/octet-stream"}
    )
    assert resp.json()["accepted"]

    # Wait for customization
    time.sleep(0.3)

    # Query
    resp = requests.post("http://localhost:8080/query", json=query)
    result = resp.json()
    results[seed] = result["distance_ms"]
    print(f"Seed {seed:>3}: distance_ms = {result['distance_ms']}")

# Verify: different seeds should produce different distances
assert len(set(results.values())) > 1, "All seeds produced same distance!"
print("Re-customization validation passed: different weights → different routes")
```

**What to verify**:

- Each seed produces a different distance (customization actually takes effect)
- No stale results (watch channel updates are applied)
- No crashes or panics during rapid re-customization
- Server health remains OK throughout

### 13.5 Testing the Gateway

```bash
# Start both servers
hanoi_server --graph-dir Maps/data/hanoi_car/graph \
  --query-port 8080 --customize-port 9080 &

hanoi_server --graph-dir Maps/data/hanoi_motorcycle/graph \
  --query-port 8081 --customize-port 9081 &

# Start gateway with config
hanoi_gateway --config gateway.yaml &

# List available profiles
curl -s http://localhost:50051/profiles | python3 -m json.tool

# Query via gateway — car profile
curl -s -X POST "http://localhost:50051/query?profile=car" \
  -H "Content-Type: application/json" \
  -d '{
    "from_lat": 21.028, "from_lng": 105.834,
    "to_lat": 21.006, "to_lng": 105.843
  }' | python3 -m json.tool

# Query via gateway — motorcycle profile
curl -s -X POST "http://localhost:50051/query?profile=motorcycle" \
  -H "Content-Type: application/json" \
  -d '{
    "from_lat": 21.028, "from_lng": 105.834,
    "to_lat": 21.006, "to_lng": 105.843
  }' | python3 -m json.tool

# Info via gateway
curl -s "http://localhost:50051/info?profile=car" | python3 -m json.tool
curl -s "http://localhost:50051/info?profile=motorcycle" | python3 -m json.tool
```

### 13.6 Testing Response Formats

```bash
# Default response is GeoJSON (no query param needed)
curl -s -X POST http://localhost:8080/query \
  -H "Content-Type: application/json" \
  -d '{
    "from_lat": 21.028, "from_lng": 105.834,
    "to_lat": 21.006, "to_lng": 105.843
  }' | python3 -m json.tool

# Explicit JSON format via query parameter
curl -s -X POST "http://localhost:8080/query?format=json" \
  -H "Content-Type: application/json" \
  -d '{
    "from_lat": 21.028, "from_lng": 105.834,
    "to_lat": 21.006, "to_lng": 105.843
  }' | python3 -m json.tool

# GeoJSON with simplestyle-spec color properties
curl -s -X POST "http://localhost:8080/query?colors" \
  -H "Content-Type: application/json" \
  -d '{
    "from_lat": 21.028, "from_lng": 105.834,
    "to_lat": 21.006, "to_lng": 105.843
  }' | python3 -m json.tool
```

**Verify**: Default response has `geometry.coordinates` with `[longitude, latitude]` order
(RFC 7946). `?format=json` returns flat `distance_ms`/`coordinates` fields.
`?colors` adds `stroke`, `stroke-width`, `fill`, `fill-opacity` to GeoJSON properties.

### 13.7 Testing Error Cases

```bash
# Invalid coordinates (out of range)
curl -s -X POST http://localhost:8080/query \
  -H "Content-Type: application/json" \
  -d '{"from_lat": 91.0, "from_lng": 105.834, "to_lat": 21.006, "to_lng": 105.843}'
# → 400 with coordinate_validation_failed

# Coordinates far from graph
curl -s -X POST http://localhost:8080/query \
  -H "Content-Type: application/json" \
  -d '{"from_lat": 10.0, "from_lng": 100.0, "to_lat": 21.006, "to_lng": 105.843}'
# → 400 with OutOfBounds rejection

# Wrong weight vector size
echo "invalid" | curl -X POST http://localhost:9080/customize \
  --data-binary @- -H "Content-Type: application/octet-stream"
# → 400 with size mismatch error

# Unknown profile via gateway
curl -s -X POST "http://localhost:50051/query?profile=unknown" \
  -H "Content-Type: application/json" \
  -d '{"from_lat": 21.028, "from_lng": 105.834, "to_lat": 21.006, "to_lng": 105.843}'
# → 400 with unknown profile error and available_profiles list
```

### 13.8 Performance Benchmarks During Testing

```bash
# Core benchmarks (no server)
bench_core \
  --graph-dir Maps/data/hanoi_car/graph \
  --query-count 1000 \
  --output baseline.json

# Server benchmarks
bench_server \
  --url http://localhost:8080 \
  --queries 1000 \
  --concurrency 10 \
  --graph-dir Maps/data/hanoi_car/graph \
  --output server_baseline.json

# After changes, compare for regressions
bench_core --graph-dir Maps/data/hanoi_car/graph --output current.json
bench_report --baseline baseline.json --current current.json --threshold 10
```

### 13.9 Validation Checklist


| Check                                 | Command / Method                                                          | Expected                   |
| ------------------------------------- | ------------------------------------------------------------------------- | -------------------------- |
| Graph loads without error             | `cch-hanoi info --data-dir ...`                                           | JSON with node/edge counts |
| Normal query returns route            | `cch-hanoi query --data-dir ... --from-lat ... --to-lat ...`              | Exit 0, distance > 0       |
| Line graph query returns route        | `cch-hanoi query --data-dir ... --line-graph --from-lat ... --to-lat ...` | Exit 0, distance > 0       |
| Server starts and serves              | `curl /health`                                                            | `{"status": "ok"}`         |
| Server ready check                    | `curl /ready`                                                             | `{"ready": true}` (200)    |
| Customization accepted                | `curl POST /customize`                                                    | `{"accepted": true}`       |
| Customization changes routing         | Compare distances before/after                                            | Different `distance_ms`    |
| Re-customization works                | Upload multiple weight sets                                               | Different results per set  |
| Coordinate validation rejects invalid | `curl` with bad coords                                                    | 400 error                  |
| Weight validation rejects INFINITY    | Upload weights with >= 2^31/2                                             | 400 error                  |
| Gateway routes correctly              | `curl POST /query?profile=car`                                            | Routes to correct backend  |
| GeoJSON format correct (default)      | `curl POST /query` (no query param)                                       | Valid GeoJSON Feature      |
| JSON format via query param           | `curl POST /query?format=json`                                            | Flat JSON response         |
| Graceful shutdown                     | Send SIGTERM to server                                                    | Clean exit within 30s      |


---

## 14. Operational Flowcharts

### 14.1 Server Startup Flow

```
CLI arguments parsed
        │
        ▼
  ┌─────────────────┐
  │ Load graph data  │  GraphData::load() — validates CSR invariants
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Build CCH        │  Phase 1 contraction (metric-independent)
  │ (or DirectedCCH) │  One-time cost: seconds to minutes
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Spawn engine     │  Background OS thread with query + customization channels
  │ thread           │  Initial customization with baseline weights
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Bind TCP ports   │  Query port + Customize port
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Install signal   │  SIGINT / SIGTERM → graceful shutdown
  │ handlers         │  30-second force-kill timeout
  └────────┬────────┘
           │
           ▼
     Server ready
     (accepting requests)
```

### 14.2 Query Processing Flow

```
POST /query  {"from_lat": 21.028, "from_lng": 105.834, ...}
        │
        ▼
  ┌─────────────────┐
  │ Parse JSON       │  Deserialize QueryRequest
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Send to engine   │  Via mpsc channel (256 buffer)
  │ thread           │  Oneshot reply channel for response
  └────────┬────────┘
           │
           ▼   (in engine thread)
  ┌─────────────────┐
  │ Detect variant   │  Coordinates → query_coords()
  │                  │  Node IDs → query()
  └────────┬────────┘
           │ (if coordinate query)
           ▼
  ┌─────────────────┐
  │ Validate coords  │  Geographic range, bbox, snap distance
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Snap to edge     │  KD-tree (k=10) + Haversine perpendicular
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ CCH query        │  Bidirectional elimination-tree search
  │ (Phase 3)        │  on CustomizedBasic
  └────────┬────────┘
           │ if no path
           ▼
  ┌─────────────────┐
  │ Fallback: try    │  All 4 endpoint combinations
  │ alternate nodes  │  (tail/head × source/dest)
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Format response  │  Default GeoJSON or ?format=json
  └────────┬────────┘
           │
           ▼
     200 OK  {"distance_ms": ..., "path_nodes": [...], ...}
```

### 14.3 Customization Flow

```
POST /customize  (binary body: [u32; num_edges])
        │
        ▼
  ┌─────────────────┐
  │ Validate size    │  body.len() == num_edges * 4
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Copy to aligned  │  bytemuck: Bytes → Vec<u32>
  │ Vec<u32>         │  (Bytes doesn't guarantee 4-byte alignment)
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Validate values  │  All weights < INFINITY (2,147,483,647)
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Queue via watch  │  watch_tx.send(Some(weights))
  │ channel          │  Last-writer-wins if multiple pending
  └────────┬────────┘
           │
           ▼
     200 OK  {"accepted": true, "message": "customization queued"}
           │
           │  (asynchronously, in engine thread)
           ▼
  ┌─────────────────┐
  │ Re-customize     │  CCH Phase 2 with new weights
  │ CCH              │  customization_active = true → false
  └─────────────────┘
```

### 14.4 Full Deployment Architecture (T-Shape)

The system follows a **T-shape architecture** — the vertical bar is external
query traffic flowing through the gateway, and the horizontal bar is the
internal data pipeline pushing weights directly to each server's customize port.

```
                    External Clients
                    (apps, dashboards)
                         │
                    POST /query
                    GET  /info
                         │
                         ▼
              ┌─────────────────────┐
              │   API Gateway       │  :50051         ─┐
              │   (hanoi_gateway)   │                   │
              └──────┬─────┬────────┘                   │ Vertical bar:
                     │     │                            │ external query
        profile      │     │  profile                   │ traffic
        ="car"       │     │  ="motorcycle"             │
                     │     │                            │
                     ▼     ▼                           ─┘
           ┌──────────┐  ┌──────────┐
           │ Server A  │  │ Server B  │
           │  (car)    │  │  (moto)   │
           │ :8080     │  │ :8081     │
           │ :9080     │  │ :9081     │
           └─────┬─────┘  └─────┬─────┘
                 ▲              ▲
                 │              │
       POST /customize   POST /customize              ── Horizontal bar:
                 │              │                         internal weight
                 └──────┬───────┘                         pipeline
                        │
        ┌───────────────┴────────────────┐
        │   Data Processing Pipeline      │
        │   (generates weight vectors     │
        │    from live traffic data)      │
        └────────────────────────────────┘
```

**Key design property**: The gateway **never** proxies `/customize`. Weight
updates flow directly from the pipeline to each server's customize port. This
keeps the gateway stateless and purely consumer-facing, while weight upload
traffic (up to 8 MB per update) stays on the internal network.

### 14.5 Integrated Data Pipeline (Planned)

The full end-to-end system extends beyond the CCH routing servers. The data
processing pipeline — responsible for transforming raw traffic data into usable
`travel_time` weight vectors — sits upstream of the `/customize` endpoint. This
section describes the intended integrated architecture.

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        DATA PIPELINE                                    │
│                                                                         │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────────────┐    │
│  │ Traffic Data  │     │ Data Ingest  │     │   Data Processing    │    │
│  │ Sources       │────▶│              │────▶│                      │    │
│  │               │     │ Collection,  │     │ Huber-robust Double  │    │
│  │ • Probe data  │     │ validation,  │     │ Exponential          │    │
│  │ • Loop detectors    │ deduplication│     │ Smoothing (DES)      │    │
│  │ • Floating car│     │              │     │                      │    │
│  │ • API feeds   │     │              │     │ Filters noise,       │    │
│  └──────────────┘     └──────────────┘     │ handles outliers,    │    │
│                                             │ produces smoothed    │    │
│                                             │ speed estimates      │    │
│                                             └──────────┬───────────┘    │
│                                                        │               │
│                                                        ▼               │
│                        ┌──────────────────────────────────────────┐    │
│                        │          Weight Modeling                  │    │
│                        │                                          │    │
│                        │  Smoothed speed + route distance          │    │
│                        │        → travel_time (milliseconds)      │    │
│                        │                                          │    │
│                        │  Custom model: maps traffic speeds to    │    │
│                        │  per-edge travel times, accounting for   │    │
│                        │  edge geographic distance (haversine)    │    │
│                        │                                          │    │
│                        │  Formula concept:                        │    │
│                        │    travel_time[e] = f(speed[e],          │    │
│                        │                      distance[e])        │    │
│                        │                                          │    │
│                        │  Output: Vec<u32> [num_edges]            │    │
│                        │          (milliseconds, < INFINITY)      │    │
│                        └──────────────────┬───────────────────────┘    │
│                                           │                            │
└───────────────────────────────────────────┼────────────────────────────┘
                                            │
                              Usable weight vector
                              (raw binary, little-endian u32)
                                            │
                         ┌──────────────────┴──────────────────┐
                         │                                     │
                         ▼                                     ▼
           POST /customize :9080                 POST /customize :9081
           ┌──────────────────┐                  ┌──────────────────┐
           │   Server A       │                  │   Server B       │
           │   (car)          │                  │   (motorcycle)   │
           │                  │                  │                  │
           │   CCH Phase 2:   │                  │   CCH Phase 2:   │
           │   re-customize   │                  │   re-customize   │
           │   with new       │                  │   with new       │
           │   weights        │                  │   weights        │
           └──────────────────┘                  └──────────────────┘
                  │                                     │
                  │         ┌─────────────┐             │
                  └────────▶│ API Gateway │◀────────────┘
                            │  (:50051)   │
                            └──────┬──────┘
                                   │
                                   ▼
                            External Clients
                            POST /query
                            GET  /info
```

#### Pipeline Stage Descriptions

**Stage 1 — Traffic Data Sources**: Raw traffic observations from any
combination of probe vehicles, loop detectors, floating car data, third-party
API feeds, or historical datasets. Format and source are the pipeline's own
concern — the routing servers are agnostic.

**Stage 2 — Data Ingest**: Collection, validation, deduplication, and
normalization of raw traffic observations. Ensures data quality before
statistical processing. Handles missing data, timestamp alignment, and
source-specific quirks.

**Stage 3 — Data Processing (Huber-robust Double Exponential Smoothing)**:
Statistical smoothing of raw speed observations into stable speed estimates.

- **Double Exponential Smoothing (DES)**: Captures both level and trend in
time-series speed data, adapting to gradual traffic pattern changes (e.g.,
morning rush onset, evening clearing)
- **Huber loss robustification**: Replaces the standard squared-error loss
with the Huber loss function, which is quadratic for small residuals but
linear for large ones. This makes the smoothing **resistant to outliers**
(e.g., a GPS probe reporting 200 km/h on a residential street, or a
momentary zero-speed reading from a stopped vehicle being sampled)
- **Output**: Smoothed per-segment speed estimates (km/h or m/s) that track
real traffic conditions without being jerked around by noisy individual
observations

**Stage 4 — Weight Modeling**: Converts smoothed speed estimates into
`travel_time` values (milliseconds) suitable for the CCH routing engine.

- **Input**: Smoothed speed per road segment + segment geographic distance
- **Model**: A custom mapping function that accounts for edge distance
(Haversine between endpoint coordinates) and the smoothed speed. The
baseline formula is `travel_time[e] = distance_m[e] / speed_m_per_s[e] * 1000`,
but the model may incorporate corrections for intersection delays, road
class adjustments, or congestion nonlinearities
- **Constraints**: Output must be `u32`, in range `[1, INFINITY)` where
`INFINITY = 2,147,483,647`. Zero weights are avoided (algorithmic edge
cases). Values represent milliseconds
- **Output**: A complete `Vec<u32>` of length `num_edges` — one weight per
directed edge in the graph, ready for upload

**Stage 5 — POST /customize**: The usable weight vector is uploaded as raw
binary (`application/octet-stream`, little-endian `[u32; num_edges]`) to each
server's customize port. The server validates, queues, and the engine thread
re-customizes the CCH asynchronously (Phase 2).

#### Update Cadence

The pipeline is designed for **periodic snapshot updates**: every X seconds
(configurable), the pipeline produces a fresh complete weight vector reflecting
current traffic conditions. Each upload is a **full replacement** — not
cumulative, not sparse. This is correct for snapshot-based traffic data where
each observation window produces a complete picture.

#### Boundary Contract

The interface between the data pipeline and the routing servers is deliberately
narrow:


| Concern            | Pipeline's responsibility                        | Server's responsibility                          |
| ------------------ | ------------------------------------------------ | ------------------------------------------------ |
| Data sources       | Collect, validate, normalize                     | Agnostic                                         |
| Speed estimation   | Huber-robust DES smoothing                       | Agnostic                                         |
| Weight computation | Model: speed × distance → ms                     | Agnostic                                         |
| Weight format      | `Vec<u32>`, length = `num_edges`, all < INFINITY | Validate size + values                           |
| Transport          | HTTP POST raw binary                             | Accept, decompress (gzip optional)               |
| Scheduling         | Decides when to push                             | Accepts any time; watch-channel last-writer-wins |
| CCH customization  | Agnostic                                         | Phase 2 re-customization                         |
| Query serving      | Agnostic                                         | Phase 3 queries on latest weights                |


This separation means either side can be replaced, upgraded, or scaled
independently. The pipeline doesn't need to know about CCH internals, and the
servers don't need to know about traffic data formats or smoothing algorithms.

---

## 15. Troubleshooting

### 15.1 Common Issues

**"failed to load graph"**

- Check that all required files exist in the graph directory
- Verify file sizes: `first_out` should have `(n+1) * 4` bytes, `head` and
`travel_time` should have `m * 4` bytes
- Check CSR invariants with: `python3 -c "import struct; fo = struct.unpack(...); assert fo[0] == 0"`

**"failed to bind query port"**

- Port already in use: `lsof -i :8080`
- Use different ports: `--query-port 8082 --customize-port 9082`

**"--original-graph-dir required for --line-graph mode"**

- Line graph mode needs access to the original graph for path mapping and
final-edge correction

**Customization appears to have no effect**

- `/customize` is asynchronous — wait for completion
- Poll `GET /info` until `customization_active: false`
- Verify weight vector length matches `num_edges` from `/info`

**Queries return empty results (no path)**

- Verify coordinates are within the graph's bounding box (check `GET /info`)
- Try node-ID queries to rule out snapping issues
- Check if source and target are in the same connected component

**Gateway returns 502 Bad Gateway**

- Backend server is not running or unreachable
- Check backend URL in gateway arguments
- Verify backend health: `curl http://localhost:8080/health`

### 15.2 Useful Debug Commands

```bash
# Check graph dimensions
python3 -c "
import os
d = 'Maps/data/hanoi_car/graph'
print(f'Nodes: {os.path.getsize(f\"{d}/first_out\") // 4 - 1:,}')
print(f'Edges: {os.path.getsize(f\"{d}/head\") // 4:,}')
print(f'Perm:  {os.path.getsize(f\"{d}/perms/cch_perm\") // 4:,}')
"

# Verify perm size matches node count
python3 -c "
import os
d = 'Maps/data/hanoi_car/graph'
n = os.path.getsize(f'{d}/first_out') // 4 - 1
p = os.path.getsize(f'{d}/perms/cch_perm') // 4
assert n == p, f'MISMATCH: {n} nodes vs {p} perm entries'
print(f'OK: {n:,} nodes = {p:,} perm entries')
"

# Enable debug logging
RUST_LOG=debug hanoi_server --graph-dir ...

# Check server state
curl -s http://localhost:8080/health | python3 -m json.tool
curl -s http://localhost:8080/ready  | python3 -m json.tool
curl -s http://localhost:8080/info   | python3 -m json.tool
```

### 15.3 Performance Expectations


| Operation               | Normal Graph | Line Graph | Notes               |
| ----------------------- | ------------ | ---------- | ------------------- |
| Graph load              | < 1s         | < 2s       | Disk I/O bound      |
| CCH build (Phase 1)     | 2–10s        | 5–20s      | CPU bound, one-time |
| Customization (Phase 2) | 50–200ms     | 100–500ms  | Per weight update   |
| Query (Phase 3)         | < 1ms        | < 2ms      | Per query           |
| KD-tree build           | < 1s         | < 2s       | One-time            |
| Snap-to-edge            | < 0.1ms      | < 0.1ms    | Per coordinate      |


Measured on Hanoi car graph (~276K nodes / ~655K edges for normal, ~655K nodes /
~1.3M edges for line graph). Actual performance varies with hardware.

---

## 16. Logging Setup Guide

All CCH-Hanoi binaries use the [tracing](https://docs.rs/tracing) ecosystem for
structured logging. This section covers every logging option, format, and
configuration available across the workspace.

### 16.1 Logging Stack

```
┌─────────────────────────────────────────────────────────────────────┐
│                     Application Code                                │
│  tracing::info!(), tracing::debug!(), #[tracing::instrument]       │
└──────────────────────────────┬──────────────────────────────────────┘
                               │ tracing events & spans
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                  tracing-subscriber registry                        │
│                                                                     │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐  │
│  │  EnvFilter   │  │  fmt layer   │  │  file layer (optional)   │  │
│  │  (RUST_LOG)  │  │  (stderr)    │  │  (JSON, daily rotation)  │  │
│  └──────────────┘  └──────────────┘  └──────────────────────────┘  │
│                                                                     │
│  ┌──────────────────────────────────┐                               │
│  │  tower-http TraceLayer          │  (hanoi-server & gateway)      │
│  │  (automatic HTTP request spans) │                                │
│  └──────────────────────────────────┘                               │
└─────────────────────────────────────────────────────────────────────┘
```

**Key components**:

- **EnvFilter**: Parses `RUST_LOG` environment variable for per-module log level
control. Falls back to a binary-specific default if `RUST_LOG` is not set.
- **fmt layer**: Formats tracing events to stderr in one of five selectable
output formats.
- **file layer** (hanoi-server only): Writes JSON-formatted logs to
daily-rotated files via a non-blocking background writer.
- **TraceLayer** (hanoi-server and hanoi-gateway): Automatically creates spans
for HTTP requests, logging method, path, status code, and latency.

### 16.2 Per-Binary Logging Capabilities

| Capability                  | `hanoi_server` | `cch-hanoi` | `hanoi_gateway` | `generate_line_graph` | `hanoi-bench` |
| --------------------------- | -------------- | ----------- | --------------- | --------------------- | ------------- |
| `--log-format` flag         | Yes            | Yes         | Yes             | Yes                   | No            |
| `RUST_LOG` env override     | Yes            | Yes         | Yes             | Yes                   | Yes           |
| `--log-dir` file output     | Yes            | No          | No              | No                    | No            |
| Always-on file logging      | No             | No          | No              | No                    | Yes           |
| Pretty format               | Yes            | Yes         | Yes             | Yes                   | —             |
| Full format                 | Yes            | Yes         | Yes             | Yes                   | —             |
| Compact format              | Yes            | Yes         | Yes             | Yes                   | Stderr        |
| Tree format                 | Yes (native)   | Fallback¹   | Fallback¹       | Fallback¹             | —             |
| Json format                 | Yes            | Yes         | Yes             | Yes                   | File          |
| HTTP request tracing        | Yes            | No          | Yes             | No                    | No            |
| Default filter              | `info,tower_http=debug` | `info` | `info` | `info`           | `info`        |

¹ Falls back to Full format because `tracing-tree` is only a dependency of
`hanoi-server`.

**Note on `hanoi-bench`**: All three bench binaries (`bench_core`,
`bench_server`, `bench_report`) use a dual-output tracing subscriber. Stderr
gets compact format for progress; a JSON log file is always created in the
current directory (see §10.4). Use `--log-name <PREFIX>` to customize the
filename prefix.

### 16.3 The `--log-format` Flag

All binaries (except hanoi-bench) accept `--log-format <FORMAT>` to select the
stderr output format. The default is `pretty`.

**Important**: For `cch-hanoi`, `--log-format` is a **top-level** flag that must
come before the subcommand:

```bash
# Correct:
cch-hanoi --log-format json query --data-dir Maps/data/hanoi_car ...

# Wrong (will error):
cch-hanoi query --log-format json --data-dir Maps/data/hanoi_car ...
```

For all other binaries, `--log-format` appears alongside other flags:

```bash
hanoi_server --log-format compact --graph-dir Maps/data/hanoi_car/graph
hanoi_gateway --log-format json --port 50051
generate_line_graph --log-format full Maps/data/hanoi_car/graph
```

### 16.4 Output Format Reference

#### Pretty (default)

Multi-line, colorized output with source file locations. Most human-readable
format for development and interactive use.

```bash
hanoi_server --log-format pretty --graph-dir Maps/data/hanoi_car/graph
```

```
  2026-03-19T10:30:00.123456Z  INFO hanoi_core::cch: building CCH
    at crates/hanoi-core/src/cch.rs:58
    in hanoi_core::cch::load_and_build with graph_dir: Maps/data/hanoi_car/graph

  2026-03-19T10:30:05.456789Z  INFO hanoi_server: server ready
    query_addr: 0.0.0.0:8080
    customize_addr: 0.0.0.0:9080
    mode: normal
```

**Characteristics**:
- Multi-line with indented fields
- ANSI color codes (disable with `NO_COLOR=1`)
- Full source file + line number shown
- Span context displayed inline
- Best for: development, debugging, interactive terminals

#### Full

Single-line output with full span context and thread IDs. Good balance between
readability and density.

```bash
hanoi_server --log-format full --graph-dir Maps/data/hanoi_car/graph
```

```
2026-03-19T10:30:00.123Z  INFO hanoi_core::cch: building CCH num_nodes=276372 num_edges=654787
2026-03-19T10:30:05.456Z  INFO ThreadId(01) hanoi_server: server ready query_addr=0.0.0.0:8080
```

**Characteristics**:
- Single-line per event
- Shows target module (`hanoi_core::cch`)
- Shows thread IDs
- Compact field display (`key=value`)
- Best for: production terminals, log tailing

#### Compact

Abbreviated single-line format with target module. Most concise text format.

```bash
hanoi_server --log-format compact --graph-dir Maps/data/hanoi_car/graph
```

```
2026-03-19T10:30:00.123Z  INFO hanoi_core::cch: building CCH
2026-03-19T10:30:05.456Z  INFO hanoi_server: server ready
```

**Characteristics**:
- Shortest single-line format
- Shows target module
- Fields may be abbreviated or omitted
- Best for: high-volume logs where brevity matters

#### Tree (hanoi-server only)

Indented hierarchical output that visually nests spans and their events. Uses
`tracing-tree` with deferred spans and span retrace.

```bash
hanoi_server --log-format tree --graph-dir Maps/data/hanoi_car/graph
```

```
hanoi_core::cch::load_and_build
  graph_dir: Maps/data/hanoi_car/graph
  0ms  INFO building CCH
  ┌ hanoi_core::cch::customize
  │ 150ms  INFO customization complete
  └
5012ms  INFO hanoi_server: server ready
```

**Characteristics**:
- Indented tree structure with indent lines (`│`, `┌`, `└`)
- Shows target modules
- Deferred spans (rendered when first child event occurs)
- Span retrace (re-renders parent context when needed)
- Indent width: 2 spaces
- Best for: understanding execution flow, debugging nested operations

**Availability**: Only `hanoi-server` has the `tracing-tree` dependency. Other
binaries (`cch-hanoi`, `hanoi_gateway`, `generate_line_graph`) accept `--log-format tree`
but silently fall back to the `full` format.

#### Json

Newline-delimited JSON. Each event is a single JSON object. No ANSI color codes.

```bash
hanoi_server --log-format json --graph-dir Maps/data/hanoi_car/graph
```

```json
{"timestamp":"2026-03-19T10:30:00.123456Z","level":"INFO","target":"hanoi_core::cch","fields":{"message":"building CCH","num_nodes":276372,"num_edges":654787},"spans":[{"name":"load_and_build","graph_dir":"Maps/data/hanoi_car/graph"}]}
{"timestamp":"2026-03-19T10:30:05.456789Z","level":"INFO","target":"hanoi_server","fields":{"message":"server ready","query_addr":"0.0.0.0:8080","customize_addr":"0.0.0.0:9080","mode":"normal"}}
```

**Characteristics**:
- One JSON object per line (NDJSON / JSON Lines)
- No ANSI escape codes
- Structured fields preserved as JSON keys
- Span context included in `spans` array
- Machine-parseable
- Best for: log aggregation (ELK, Loki, Splunk, Datadog), CI pipelines,
programmatic analysis

### 16.5 Format Comparison Table

| Format    | Lines/Event | Color | Source Location | Thread ID | Span Context | Machine-Readable |
| --------- | ----------- | ----- | --------------- | --------- | ------------ | ---------------- |
| `pretty`  | Multi       | Yes   | Yes             | No        | Inline       | No               |
| `full`    | 1           | Yes   | No              | Yes       | Inline       | No               |
| `compact` | 1           | Yes   | No              | No        | Abbreviated  | No               |
| `tree`    | Multi       | Yes   | No              | No        | Hierarchical | No               |
| `json`    | 1           | No    | No              | No        | JSON array   | Yes              |

### 16.6 The `RUST_LOG` Environment Variable

`RUST_LOG` controls which log events pass through the filter. It overrides the
binary's built-in default. The syntax follows the
[`tracing-subscriber` EnvFilter](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html)
format.

#### Default Filters (when `RUST_LOG` is not set)

| Binary                | Default Filter              | Rationale                                          |
| --------------------- | --------------------------- | -------------------------------------------------- |
| `hanoi_server`        | `info,tower_http=debug`     | Shows HTTP request details for debugging            |
| `cch-hanoi`           | `info`                      | One-shot tool; minimal noise                        |
| `hanoi_gateway`       | `info`                      | Proxy; HTTP tracing via tower-http at info is enough |
| `generate_line_graph` | `info`                      | Pipeline tool; progress-level logging               |

#### Filter Syntax

```bash
# Global level
RUST_LOG=debug                    # Everything at debug or above
RUST_LOG=warn                     # Only warnings and errors
RUST_LOG=trace                    # Maximum verbosity (very noisy)

# Per-module levels (comma-separated)
RUST_LOG=info,hanoi_core=debug    # Info globally, debug for hanoi_core
RUST_LOG=warn,hanoi_server=info   # Warn globally, info for server crate
RUST_LOG=info,tower_http=trace    # Info globally, trace for HTTP layer

# Target-specific granularity
RUST_LOG=hanoi_core::cch=trace              # Trace just the CCH module
RUST_LOG=hanoi_core::spatial=debug          # Debug spatial indexing
RUST_LOG=hanoi_server::engine=trace         # Trace engine thread
RUST_LOG=hanoi_server::handlers=debug       # Debug HTTP handlers

# Combining multiple targets
RUST_LOG="info,hanoi_core::cch=debug,hanoi_core::spatial=debug,tower_http=debug"
```

#### Common Recipes

```bash
# Development: see everything including CCH internals
RUST_LOG=debug hanoi_server --graph-dir Maps/data/hanoi_car/graph

# Production: info only, no HTTP noise
RUST_LOG=info hanoi_server --graph-dir Maps/data/hanoi_car/graph

# Debug coordinate snapping issues
RUST_LOG="info,hanoi_core::spatial=debug" cch-hanoi --log-format full \
  query --data-dir Maps/data/hanoi_car --from-lat 21.028 --from-lng 105.834 ...

# Debug CCH customization performance
RUST_LOG="info,hanoi_core::cch=trace,hanoi_server::engine=trace" \
  hanoi_server --log-format full --graph-dir Maps/data/hanoi_car/graph

# Debug gateway proxy behavior
RUST_LOG="debug,hyper=info" hanoi_gateway --log-format full

# Silence everything except errors
RUST_LOG=error hanoi_server --graph-dir Maps/data/hanoi_car/graph

# Debug graph loading (data file I/O)
RUST_LOG="info,hanoi_core::graph=debug" cch-hanoi query --data-dir Maps/data/hanoi_car ...
```

### 16.7 File-Based Logging (hanoi-server only)

The `--log-dir` flag enables persistent log files alongside stderr output. This
is only available for `hanoi_server`.

#### How It Works

```
hanoi_server --log-dir /var/log/hanoi/ --log-format pretty --graph-dir ...
                  │                            │
                  │                            └─ controls stderr format
                  │
                  ▼
         /var/log/hanoi/
         └── hanoi-server.log.2026-03-19   ← always JSON, regardless of --log-format
         └── hanoi-server.log.2026-03-20   ← new file at midnight
         └── hanoi-server.log.2026-03-21
```

**Key behaviors**:

- **File format is always JSON** — regardless of `--log-format`. This ensures
log files are machine-parseable for aggregation tools, even when the operator
prefers pretty or tree output on stderr.
- **Daily rotation** — a new file is created at midnight (local time). The
filename is `hanoi-server.log.YYYY-MM-DD`.
- **Non-blocking writes** — log events are written to a background thread via
`tracing-appender::non_blocking`. The main application never blocks on file I/O.
- **WorkerGuard lifetime** — the non-blocking writer returns a `WorkerGuard`
that must be held for the lifetime of the program. Dropping it flushes and
closes the writer. The server holds this guard in `main()`.
- **Dual output** — stderr and file layers run simultaneously. Both receive the
same events (filtered by the same `RUST_LOG` / default filter).

#### Usage

```bash
# Pretty stderr + JSON file logs
hanoi_server --graph-dir Maps/data/hanoi_car/graph \
  --log-format pretty --log-dir /var/log/hanoi/

# JSON everywhere (stderr + file)
hanoi_server --graph-dir Maps/data/hanoi_car/graph \
  --log-format json --log-dir /var/log/hanoi/

# Compact stderr for monitoring + JSON file for aggregation
hanoi_server --graph-dir Maps/data/hanoi_car/graph \
  --log-format compact --log-dir /var/log/hanoi/
```

#### Parsing File Logs

```bash
# Tail the current day's log
tail -f /var/log/hanoi/hanoi-server.log.$(date +%Y-%m-%d)

# Filter for errors with jq
cat /var/log/hanoi/hanoi-server.log.2026-03-19 | jq 'select(.level == "ERROR")'

# Extract customization events
cat /var/log/hanoi/hanoi-server.log.2026-03-19 | jq 'select(.fields.message | contains("customiz"))'

# Count queries per hour
cat /var/log/hanoi/hanoi-server.log.2026-03-19 \
  | jq -r 'select(.fields.message == "query") | .timestamp[:13]' \
  | sort | uniq -c
```

### 16.8 HTTP Request Tracing

`hanoi-server` and `hanoi-gateway` include `tower-http`'s `TraceLayer`, which
automatically creates tracing spans for every HTTP request.

**What it logs** (at `tower_http=debug` level):

- Request: method, URI, version, headers
- Response: status code, latency
- Body: size (if known)

**Control via RUST_LOG**:

```bash
# Include HTTP request details (server default)
RUST_LOG="info,tower_http=debug" hanoi_server --graph-dir ...

# Full HTTP tracing (very verbose — includes headers, body info)
RUST_LOG="info,tower_http=trace" hanoi_server --graph-dir ...

# Suppress HTTP tracing (only application logs)
RUST_LOG="info,tower_http=warn" hanoi_server --graph-dir ...
```

### 16.9 Instrumented Code Points

The `hanoi-core` library and server crate emit structured tracing events at key
points in the routing pipeline. These are the log messages you'll see at various
levels:

#### Info-Level Events (visible by default)

| Source Module              | Message                              | Fields                              | When                                |
| -------------------------- | ------------------------------------ | ----------------------------------- | ----------------------------------- |
| `hanoi_core::cch`          | building CCH                         | `num_nodes`, `num_edges`            | Phase 1 contraction start           |
| `hanoi_core::line_graph`   | building DirectedCCH for line graph  | `num_nodes`, `num_edges`            | Line graph Phase 1 start            |
| `hanoi_core::spatial`      | spatial index built                  | `bbox` (min/max lat/lng)            | KD-tree construction complete       |
| `hanoi_server::engine`     | re-customizing                       | `num_weights`                       | Phase 2 customization start         |
| `hanoi_server::engine`     | customization complete               | —                                   | Phase 2 customization end           |
| `hanoi_server::engine`     | re-customizing line graph            | `num_weights`                       | Line graph Phase 2 start            |
| `hanoi_server::engine`     | line graph customization complete    | —                                   | Line graph Phase 2 end              |
| `hanoi_server::handlers`   | customization weights accepted       | —                                   | `/customize` validation passed      |
| `hanoi_server`             | server ready                         | `query_addr`, `customize_addr`      | Both ports bound and serving        |

#### Warning-Level Events

| Source Module              | Message                              | Fields                              | When                                |
| -------------------------- | ------------------------------------ | ----------------------------------- | ----------------------------------- |
| `hanoi_server::handlers`   | coordinate validation failed         | `rejection`                         | Invalid coordinates in `/query`     |

#### Debug-Level Events (require `RUST_LOG=debug` or module-specific)

| Source Module              | Message                              | Fields                              | When                                |
| -------------------------- | ------------------------------------ | ----------------------------------- | ----------------------------------- |
| `hanoi_core::graph`        | loading graph data from disk         | `dir`                               | GraphData::load() start             |
| `hanoi_core::graph`        | graph data loaded                    | `num_nodes`, `num_edges`            | GraphData::load() complete          |

#### Instrumented Spans (for timing and nesting)

| Source Module              | Span Name            | Fields                              | What it wraps                       |
| -------------------------- | -------------------- | ----------------------------------- | ----------------------------------- |
| `hanoi_core::cch`          | `load_and_build`     | `graph_dir`                         | Full Phase 1 pipeline               |
| `hanoi_core::cch`          | `customize`          | —                                   | Phase 2 customization               |
| `hanoi_core::cch`          | `customize_with`     | `num_weights`                       | Phase 2 with custom weights         |
| `hanoi_core::cch`          | `query`              | `from`, `to`                        | Single CCH query                    |
| `hanoi_core::cch`          | `query_coords`       | `from`, `to`                        | Coordinate-based query              |
| `hanoi_core::line_graph`   | `load_and_build`     | `line_graph_dir`, `original_graph_dir` | Line graph Phase 1                |
| `hanoi_core::line_graph`   | `customize`          | —                                   | Line graph Phase 2                  |
| `hanoi_core::line_graph`   | `customize_with`     | `num_weights`                       | Line graph Phase 2 with custom weights |
| `hanoi_core::spatial`      | `build`              | `num_nodes`                         | KD-tree construction                |
| `hanoi_core::spatial`      | `snap_to_edge`       | `lat`, `lng`                        | Single snap-to-edge operation       |
| `hanoi_core::spatial`      | `validated_snap`     | `label`, `lat`, `lng`               | Snap with bbox/distance validation  |
| `hanoi_server::engine`     | `customization`      | `num_weights`                       | Engine thread customization block   |

### 16.10 Disabling ANSI Colors

Text formats (`pretty`, `full`, `compact`, `tree`) emit ANSI color escape codes
by default. To disable colors (useful for piping to files or non-terminal
environments):

```bash
# Standard environment variable (respected by tracing-subscriber)
NO_COLOR=1 hanoi_server --graph-dir Maps/data/hanoi_car/graph

# Or use json format (never emits ANSI codes)
hanoi_server --log-format json --graph-dir Maps/data/hanoi_car/graph
```

### 16.11 Recommended Configurations

#### Local Development

```bash
# Pretty output, debug level for your area of interest
RUST_LOG="info,hanoi_core::cch=debug" hanoi_server \
  --log-format pretty \
  --graph-dir Maps/data/hanoi_car/graph
```

#### Production Deployment

```bash
# Compact stderr for monitoring dashboards + JSON file for aggregation
RUST_LOG="info,tower_http=info" hanoi_server \
  --log-format compact \
  --log-dir /var/log/hanoi/ \
  --graph-dir Maps/data/hanoi_car/graph
```

#### CI / Automated Testing

```bash
# JSON to stderr for structured parsing, warnings only
RUST_LOG=warn hanoi_server \
  --log-format json \
  --graph-dir Maps/data/hanoi_car/graph
```

#### Quick CLI Debugging

```bash
# Full format with debug to see query internals
RUST_LOG="debug" cch-hanoi --log-format full \
  query --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843
```

#### Pipeline Tool Debugging

```bash
# Trace line graph generation
RUST_LOG="debug" generate_line_graph --log-format full Maps/data/hanoi_car/graph
```