# hanoi-server

CCH routing server for the Hanoi road network. Exposes a JSON query API and a
binary customization API on separate ports.

## Architecture

```
                    ┌──────────────────────────────────────────┐
  HTTP clients ───▶ │  query port (:8080)                      │
                    │    POST /query          route queries     │
                    │    POST /reset_weights  restore baseline  │
                    │    GET  /info           graph metadata    │
                    │    GET  /health         uptime + stats    │
                    │    GET  /ready          engine liveness   │
                    │  ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  │
                    │  UI-only (feature = "ui", --serve-ui):    │
                    │    POST /evaluate_routes                  │
                    │    GET  /traffic_overlay                  │
                    │    GET  /camera_overlay                   │
                    │    GET  /  /ui  /assets/*                 │
                    ├──────────────────────────────────────────┤
  Pipeline tools ─▶ │  customize port (:9080)                   │
                    │    POST /customize      upload weights    │
                    └──────────┬───────────────────────────────┘
                               │ mpsc (queries)
                               │ watch (weights)
                               ▼
                    ┌──────────────────────┐
                    │  Background engine   │
                    │  thread (1 thread)   │
                    │  - CCH customization │
                    │  - Dijkstra queries  │
                    └──────────────────────┘
```

The server runs two Axum listeners on separate TCP ports. Query handlers
forward requests to a single background engine thread via `mpsc`. Weight
updates go through a `watch` channel — latest-wins semantics, no queue.

## Graph modes

| Flag | Engine | Description |
| --- | --- | --- |
| *(default)* | `CchContext` / `QueryEngine` | Standard node-based CCH |
| `--line-graph` | `LineGraphCchContext` / `LineGraphQueryEngine` | Turn-expanded directed CCH (requires `--original-graph-dir`) |

## CLI

```
hanoi_server --graph-dir Maps/data/hanoi_car/graph [OPTIONS]
```

| Option | Default | Description |
| --- | --- | --- |
| `--graph-dir` | *(required)* | Path to RoutingKit-format graph directory |
| `--original-graph-dir` | *(none)* | Original graph dir (required with `--line-graph`) |
| `--query-port` | `8080` | Query API port |
| `--customize-port` | `9080` | Customization API port |
| `--line-graph` | `false` | Enable line-graph mode |
| `--serve-ui` | `false` | Serve bundled route-viewer UI (requires `ui` feature) |
| `--camera-config` | `CCH_Data_Pipeline/config/mvp_camera.yaml` | Camera YAML (requires `ui` feature) |
| `--log-format` | `pretty` | `pretty` / `full` / `compact` / `tree` / `json` |
| `--log-dir` | *(none)* | Enable daily-rotated JSON log files |

## API reference

### Query port (default 8080)

#### `POST /query`

Route between two points. Accepts coordinate-based or node-ID-based queries.

**Request body:**
```json
{
  "from_lat": 21.028, "from_lng": 105.854,
  "to_lat": 21.007,   "to_lng": 105.820
}
```

Or by node ID:
```json
{ "from_node": 12345, "to_node": 67890 }
```

**Query parameters:**

| Param | Effect |
| --- | --- |
| `format=json` | Plain JSON response (default: GeoJSON FeatureCollection) |
| `colors` | Add simplestyle-spec stroke/fill to GeoJSON output |
| `alternatives=N` | Number of alternative routes to return (0 = single shortest route) |
| `stretch=F` | Max geographic stretch factor for alternatives (default: `1.25` = 25% longer than shortest). A candidate route whose geographic length exceeds `shortest_distance * stretch` is rejected |

##### Response formats

The default response is a **GeoJSON FeatureCollection**. Use `?format=json`
for plain JSON.

> **Coordinate convention:** GeoJSON coordinates follow RFC 7946 `[lng, lat]`.
> Plain JSON uses `[lat, lng]` (matching the request format).

**GeoJSON response** (default):

```json
{
  "type": "FeatureCollection",
  "features": [{
    "type": "Feature",
    "geometry": {
      "type": "LineString",
      "coordinates": [[105.854, 21.028], [105.840, 21.015], [105.820, 21.007]]
    },
    "properties": {
      "source": "hanoi_server",
      "export_version": 1,
      "graph_type": "normal",
      "distance_ms": 324000,
      "distance_m": 5400.0,
      "path_nodes": [1, 42, 99],
      "route_arc_ids": [10, 20],
      "weight_path_ids": [10, 20],
      "origin": [21.028, 105.854],
      "destination": [21.007, 105.820],
      "turns": [
        {
          "direction": "Left",
          "angle_degrees": 87.3,
          "distance_to_next_m": 420.5
        }
      ]
    }
  }]
}
```

With `?colors`, simplestyle-spec properties are added to each feature:

```json
{
  "stroke": "#ff5500",
  "stroke-width": 10,
  "fill": "#ffaa00",
  "fill-opacity": 0.4
}
```

**No-path response** (GeoJSON, when no route is found):

```json
{
  "type": "FeatureCollection",
  "features": [{
    "type": "Feature",
    "geometry": null,
    "properties": { "distance_ms": null, "distance_m": null }
  }]
}
```

**Plain JSON response** (`?format=json`):

```json
{
  "graph_type": "normal",
  "distance_ms": 324000,
  "distance_m": 5400.0,
  "path_nodes": [1, 42, 99],
  "route_arc_ids": [10, 20],
  "weight_path_ids": [10, 20],
  "coordinates": [[21.028, 105.854], [21.015, 105.840], [21.007, 105.820]],
  "turns": [
    {
      "direction": "Left",
      "angle_degrees": 87.3,
      "distance_to_next_m": 420.5
    }
  ],
  "origin": [21.028, 105.854],
  "destination": [21.007, 105.820]
}
```

Fields `route_arc_ids`, `weight_path_ids`, and `turns` are omitted when empty.
`origin` and `destination` are omitted for node-ID queries.

##### Multi-route response (`?alternatives=N`)

When `alternatives` > 0, the response contains multiple features (GeoJSON) or
an array of route objects (plain JSON).

**GeoJSON multi-route** — each feature has an additional `route_index` property
(0 = shortest). With `?colors`, each route gets a distinct stroke color from a
10-color palette (`#ff5500`, `#0055ff`, `#00aa44`, ...), and the primary route
gets `stroke-width: 10` vs `6` for alternatives.

```json
{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "geometry": {
        "type": "LineString",
        "coordinates": [[105.854, 21.028], [105.840, 21.015], [105.820, 21.007]]
      },
      "properties": {
        "source": "hanoi_server",
        "export_version": 1,
        "graph_type": "normal",
        "route_index": 0,
        "distance_ms": 324000,
        "distance_m": 5400.0,
        "path_nodes": [1, 42, 99],
        "route_arc_ids": [10, 20],
        "weight_path_ids": [10, 20],
        "origin": [21.028, 105.854],
        "destination": [21.007, 105.820],
        "stroke": "#ff5500",
        "stroke-width": 10,
        "fill": "#ff5500",
        "fill-opacity": 0.3
      }
    },
    {
      "type": "Feature",
      "geometry": {
        "type": "LineString",
        "coordinates": [[105.854, 21.028], [105.850, 21.020], [105.820, 21.007]]
      },
      "properties": {
        "source": "hanoi_server",
        "export_version": 1,
        "graph_type": "normal",
        "route_index": 1,
        "distance_ms": 340000,
        "distance_m": 5800.0,
        "path_nodes": [1, 55, 99],
        "route_arc_ids": [10, 30],
        "weight_path_ids": [10, 30],
        "origin": [21.028, 105.854],
        "destination": [21.007, 105.820],
        "stroke": "#0055ff",
        "stroke-width": 6,
        "fill": "#0055ff",
        "fill-opacity": 0.3
      }
    }
  ]
}
```

**Plain JSON multi-route** (`?format=json&alternatives=2`) — returns a JSON
array of route objects:

```json
[
  {
    "graph_type": "normal",
    "distance_ms": 324000,
    "distance_m": 5400.0,
    "path_nodes": [1, 42, 99],
    "route_arc_ids": [10, 20],
    "weight_path_ids": [10, 20],
    "coordinates": [[21.028, 105.854], [21.015, 105.840], [21.007, 105.820]],
    "origin": [21.028, 105.854],
    "destination": [21.007, 105.820]
  },
  {
    "graph_type": "normal",
    "distance_ms": 340000,
    "distance_m": 5800.0,
    "path_nodes": [1, 55, 99],
    "route_arc_ids": [10, 30],
    "weight_path_ids": [10, 30],
    "coordinates": [[21.028, 105.854], [21.020, 105.850], [21.007, 105.820]],
    "origin": [21.028, 105.854],
    "destination": [21.007, 105.820]
  }
]
```

##### Turn annotations

Each turn annotation in the `turns` array contains:

| Field | Type | Description |
| --- | --- | --- |
| `direction` | string | `Straight`, `SlightLeft`, `SlightRight`, `Left`, `Right`, `SharpLeft`, `SharpRight`, `UTurn`, `RoundaboutEnter`, `RoundaboutExitStraight`, `RoundaboutExitRight`, `RoundaboutExitLeft` |
| `angle_degrees` | f64 | Signed turn angle [-180, 180]. Positive = left, negative = right |
| `distance_to_next_m` | f64 | Meters to next maneuver (or route end) |

##### Error responses

**400 Bad Request** — coordinate validation failed:

```json
{
  "error": "coordinate_validation_failed",
  "message": "origin is outside the graph bounding box",
  "details": {
    "reason": "out_of_bounds",
    "label": "origin",
    "lat": 22.5,
    "lng": 105.8,
    "bbox": { "min_lat": 20.9, "max_lat": 21.1, "min_lng": 105.7, "max_lng": 105.9 },
    "padding_m": 5000.0
  }
}
```

Rejection reasons: `non_finite`, `invalid_range`, `out_of_bounds`, `snap_too_far`.

#### `POST /reset_weights`

Restore the server's baseline travel-time metric. No request body.

#### `GET /info`

```json
{
  "graph_type": "normal",
  "num_nodes": 123456,
  "num_edges": 456789,
  "customization_active": false,
  "bbox": { "min_lat": 20.9, "max_lat": 21.1, "min_lng": 105.7, "max_lng": 105.9 }
}
```

#### `GET /health`

```json
{
  "status": "ok",
  "uptime_seconds": 3600,
  "total_queries_processed": 42,
  "customization_active": false
}
```

#### `GET /ready`

Returns `200 {"ready": true}` or `503 {"ready": false}` if the engine thread
has died.

### Customize port (default 9080)

#### `POST /customize`

Upload a new weight vector. Body: raw little-endian `[u32; num_edges]`,
optionally gzip-compressed (handled by `RequestDecompressionLayer`).
Max body size: 64 MiB.

**Validation:**
- Byte count must equal `num_edges * 4`
- All weights must be <= `INFINITY` (2^31 - 1)

**Response:** `200 {"accepted": true, "message": "customization queued"}`

Returns **before** customization completes. Poll `GET /info` and watch
`customization_active` for completion.

### UI-only endpoints (feature = "ui", --serve-ui)

| Endpoint | Method | Description |
| --- | --- | --- |
| `/evaluate_routes` | POST | Evaluate imported GeoJSON routes against current weights |
| `/traffic_overlay` | GET | Viewport-filtered traffic heatmap segments |
| `/camera_overlay` | GET | Viewport-filtered camera markers |
| `/` `/ui` `/assets/*` | GET | Bundled route-viewer web UI |

## Watch-channel semantics

The engine loop uses `borrow_and_update()` on the watch receiver, so it always
sees the latest weight vector. If `/customize` is called twice while a
customization is running, the earlier vector is silently replaced. This is
intentional — the engine should use the freshest weights, not replay stale
intermediate states.

## Graceful shutdown

SIGINT or SIGTERM triggers graceful shutdown via a broadcast channel. Both
listeners drain in-flight requests. A 30-second hard deadline force-kills the
process if shutdown stalls.

## Build

```bash
# API-only (default)
cargo build --release -p hanoi-server

# With bundled UI
cargo build --release -p hanoi-server --features ui
```
