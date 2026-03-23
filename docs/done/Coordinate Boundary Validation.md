# Coordinate Boundary Validation

## Context

The CCH-Hanoi routing stack has **zero validation** on coordinate inputs. When a
user sends coordinates outside the map's coverage area (e.g. Tokyo coordinates
to a Hanoi server), the KD-tree silently snaps to the nearest Hanoi node
(potentially thousands of km away) and returns a 200 OK with a valid-looking
route that has nothing to do with the input. This is a critical correctness
issue affecting the server, CLI, and gateway.

**Goal:** Reject invalid coordinates early, with clear error messages, at every
entry point — while keeping validation logic centralized in `hanoi-core` so all
consumers benefit automatically.

---

## Phase 1: Validation Types — `hanoi-core/src/bounds.rs`

### 1.1 New module: `hanoi-core/src/bounds.rs`

```rust
use std::fmt;

/// Axis-aligned geographic bounding box computed from graph node coordinates.
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    pub min_lat: f32,
    pub max_lat: f32,
    pub min_lng: f32,
    pub max_lng: f32,
}

impl BoundingBox {
    /// Compute from lat/lng slices. Panics on empty slices.
    pub fn from_coords(lat: &[f32], lng: &[f32]) -> Self {
        assert!(!lat.is_empty(), "cannot compute bounding box from empty coordinates");
        let (mut min_lat, mut max_lat) = (f32::MAX, f32::MIN);
        let (mut min_lng, mut max_lng) = (f32::MAX, f32::MIN);
        for (&la, &lo) in lat.iter().zip(lng.iter()) {
            min_lat = min_lat.min(la);
            max_lat = max_lat.max(la);
            min_lng = min_lng.min(lo);
            max_lng = max_lng.max(lo);
        }
        BoundingBox { min_lat, max_lat, min_lng, max_lng }
    }

    /// Check if a point is inside the box expanded by `padding_m` on all sides.
    pub fn contains_with_padding(&self, lat: f32, lng: f32, padding_m: f64) -> bool {
        let lat_pad = (padding_m / 111_320.0) as f32;
        let center_lat = ((self.min_lat + self.max_lat) / 2.0) as f64;
        let lng_pad = (padding_m / (111_320.0 * center_lat.to_radians().cos())) as f32;

        lat >= self.min_lat - lat_pad
            && lat <= self.max_lat + lat_pad
            && lng >= self.min_lng - lng_pad
            && lng <= self.max_lng + lng_pad
    }
}
```

### 1.2 Validation config and rejection type

```rust
/// Configurable parameters for coordinate validation.
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Padding in meters to expand the bounding box on all sides.
    /// Default: 1000.0 (1 km).
    pub bbox_padding_m: f64,
    /// Maximum snap distance in meters. Snaps farther than this are rejected.
    /// Default: 1000.0 (1 km).
    pub max_snap_distance_m: f64,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        ValidationConfig { bbox_padding_m: 1000.0, max_snap_distance_m: 1000.0 }
    }
}

/// Reason a coordinate was rejected.
#[derive(Debug, Clone)]
pub enum CoordRejection {
    /// Coordinate contains NaN or Infinity.
    NonFinite { label: &'static str, lat: f32, lng: f32 },
    /// Latitude outside [-90, 90] or longitude outside [-180, 180].
    InvalidRange { label: &'static str, lat: f32, lng: f32 },
    /// Coordinate is outside the graph's padded bounding box.
    OutOfBounds { label: &'static str, lat: f32, lng: f32, bbox: BoundingBox, padding_m: f64 },
    /// Snap distance exceeds the configured maximum.
    SnapTooFar { label: &'static str, lat: f32, lng: f32, snap_distance_m: f64, max_distance_m: f64 },
}

impl fmt::Display for CoordRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoordRejection::NonFinite { label, lat, lng } =>
                write!(f, "{label} coordinate ({lat}, {lng}) is not finite"),
            CoordRejection::InvalidRange { label, lat, lng } =>
                write!(f, "{label} coordinate ({lat}, {lng}) is outside valid geographic range"),
            CoordRejection::OutOfBounds { label, lat, lng, bbox, padding_m } =>
                write!(f, "{label} coordinate ({lat}, {lng}) is outside the map's coverage area \
                    (lat [{}, {}], lng [{}, {}], {padding_m}m padding)",
                    bbox.min_lat, bbox.max_lat, bbox.min_lng, bbox.max_lng),
            CoordRejection::SnapTooFar { label, lat, lng, snap_distance_m, max_distance_m } =>
                write!(f, "{label} coordinate ({lat}, {lng}) is {snap_distance_m:.0}m from the \
                    nearest road (maximum: {max_distance_m:.0}m)"),
        }
    }
}
```

### 1.3 `to_details_json()` for API error responses

Requires adding `serde_json = "1"` to `hanoi-core/Cargo.toml`.

```rust
impl CoordRejection {
    pub fn to_details_json(&self) -> serde_json::Value {
        match self {
            CoordRejection::NonFinite { label, lat, lng } => serde_json::json!({
                "reason": "non_finite", "label": label, "lat": lat, "lng": lng,
            }),
            CoordRejection::InvalidRange { label, lat, lng } => serde_json::json!({
                "reason": "invalid_range", "label": label, "lat": lat, "lng": lng,
            }),
            CoordRejection::OutOfBounds { label, lat, lng, bbox, padding_m } => serde_json::json!({
                "reason": "out_of_bounds", "label": label, "lat": lat, "lng": lng,
                "bbox": { "min_lat": bbox.min_lat, "max_lat": bbox.max_lat,
                           "min_lng": bbox.min_lng, "max_lng": bbox.max_lng },
                "padding_m": padding_m,
            }),
            CoordRejection::SnapTooFar { label, lat, lng, snap_distance_m, max_distance_m } => serde_json::json!({
                "reason": "snap_too_far", "label": label, "lat": lat, "lng": lng,
                "snap_distance_m": snap_distance_m, "max_distance_m": max_distance_m,
            }),
        }
    }
}
```

### 1.4 Top-level validation function

```rust
/// Validate a single coordinate against geographic range and bounding box.
pub fn validate_coordinate(
    label: &'static str,
    lat: f32,
    lng: f32,
    bbox: &BoundingBox,
    config: &ValidationConfig,
) -> Result<(), CoordRejection> {
    if !lat.is_finite() || !lng.is_finite() {
        return Err(CoordRejection::NonFinite { label, lat, lng });
    }
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lng) {
        return Err(CoordRejection::InvalidRange { label, lat, lng });
    }
    if !bbox.contains_with_padding(lat, lng, config.bbox_padding_m) {
        return Err(CoordRejection::OutOfBounds { label, lat, lng, bbox: *bbox, padding_m: config.bbox_padding_m });
    }
    Ok(())
}
```

### 1.5 Register module in `hanoi-core/src/lib.rs`

Add `pub mod bounds;` and re-export key types:

```rust
pub mod bounds;
pub use bounds::{BoundingBox, CoordRejection, ValidationConfig};
```

### Files changed

- `CCH-Hanoi/crates/hanoi-core/src/bounds.rs` — **NEW**
- `CCH-Hanoi/crates/hanoi-core/src/lib.rs` — add `pub mod bounds` + re-exports
- `CCH-Hanoi/crates/hanoi-core/Cargo.toml` — add `serde_json = "1"`

---

## Phase 2: `SnapResult` with Distance + `SpatialIndex` Bounding Box

### 2.1 Add `snap_distance_m` to `SnapResult`

In `hanoi-core/src/spatial.rs`:

```rust
pub struct SnapResult {
    pub edge_id: EdgeId,
    pub tail: NodeId,
    pub head: NodeId,
    pub t: f64,
    /// Haversine distance in meters from the query point to the snapped point.
    pub snap_distance_m: f64,  // NEW
}
```

In `snap_to_edge()`, pass `best_dist` (already computed but currently discarded)
into the `SnapResult`:

```rust
SnapResult {
    edge_id: best_edge.expect("..."),
    tail: best_tail,
    head: best_head,
    t: best_t,
    snap_distance_m: best_dist,  // was already computed
}
```

### 2.2 Store `BoundingBox` in `SpatialIndex`

```rust
pub struct SpatialIndex {
    tree: ImmutableKdTree<f32, 2>,
    first_out: Vec<EdgeId>,
    head: Vec<NodeId>,
    lat: Vec<f32>,
    lng: Vec<f32>,
    bbox: BoundingBox,  // NEW
}
```

In `SpatialIndex::build()`, compute and log the bounding box:

```rust
let bbox = BoundingBox::from_coords(lat, lng);
tracing::info!(
    min_lat = bbox.min_lat, max_lat = bbox.max_lat,
    min_lng = bbox.min_lng, max_lng = bbox.max_lng,
    "bounding box computed"
);
```

Add accessor:

```rust
pub fn bbox(&self) -> &BoundingBox {
    &self.bbox
}
```

### 2.3 `validated_snap()` method

```rust
/// Validate coordinates and snap to edge, or return a rejection.
///
/// Validation order:
/// 1. Finite check, geographic range check, bounding box check
/// 2. Snap to nearest edge
/// 3. Snap distance check
#[tracing::instrument(skip(self, config), fields(label, lat, lng))]
pub fn validated_snap(
    &self,
    label: &'static str,
    lat: f32,
    lng: f32,
    config: &ValidationConfig,
) -> Result<SnapResult, CoordRejection> {
    crate::bounds::validate_coordinate(label, lat, lng, &self.bbox, config)?;

    let result = self.snap_to_edge(lat, lng);

    if result.snap_distance_m > config.max_snap_distance_m {
        return Err(CoordRejection::SnapTooFar {
            label, lat, lng,
            snap_distance_m: result.snap_distance_m,
            max_distance_m: config.max_snap_distance_m,
        });
    }

    Ok(result)
}
```

### 2.4 Update `lib.rs` re-exports

Add `BoundingBox` to the `SpatialIndex` re-export line or add a separate
re-export if needed. `BoundingBox` is already re-exported from `bounds`.

### Files changed

- `CCH-Hanoi/crates/hanoi-core/src/spatial.rs` — modify `SnapResult`, `SpatialIndex`, add `validated_snap`, add `bbox()`

---

## Phase 3: Integrate Validation into Query Engines

### 3.1 `QueryEngine` in `hanoi-core/src/cch.rs`

Add `validation_config` field:

```rust
pub struct QueryEngine<'a> {
    server: CchQueryServer<CustomizedBasic<'a, CCH>>,
    context: &'a CchContext,
    spatial: SpatialIndex,
    validation_config: ValidationConfig,  // NEW
}
```

Update constructors:

```rust
impl<'a> QueryEngine<'a> {
    pub fn new(context: &'a CchContext) -> Self {
        Self::with_validation_config(context, ValidationConfig::default())
    }

    pub fn with_validation_config(context: &'a CchContext, validation_config: ValidationConfig) -> Self {
        let customized = context.customize();
        let server = CchQueryServer::new(customized);
        let spatial = SpatialIndex::build(
            &context.graph.latitude, &context.graph.longitude,
            &context.graph.first_out, &context.graph.head,
        );
        QueryEngine { server, context, spatial, validation_config }
    }
}
```

**Change `query_coords` return type** (breaking change — all callers updated in
Phase 4):

```rust
#[tracing::instrument(skip(self), fields(
    from_lat = from.0, from_lng = from.1,
    to_lat = to.0, to_lng = to.1
))]
pub fn query_coords(
    &mut self,
    from: (f32, f32),
    to: (f32, f32),
) -> Result<Option<QueryAnswer>, CoordRejection> {
    let src = self.spatial.validated_snap("origin", from.0, from.1, &self.validation_config)?;
    let dst = self.spatial.validated_snap("destination", to.0, to.1, &self.validation_config)?;

    // ... existing routing logic unchanged, wrap final return in Ok(...)
}
```

Add accessors:

```rust
pub fn bbox(&self) -> &BoundingBox { self.spatial.bbox() }
pub fn validation_config(&self) -> &ValidationConfig { &self.validation_config }
```

### 3.2 `LineGraphQueryEngine` in `hanoi-core/src/line_graph.rs`

Identical treatment: add `validation_config` field, `with_validation_config`
constructor, change `query_coords` to `Result<Option<QueryAnswer>,
CoordRejection>`, add `bbox()` and `validation_config()` accessors.

### Files changed

- `CCH-Hanoi/crates/hanoi-core/src/cch.rs` — `QueryEngine` struct, constructors, `query_coords` return type
- `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs` — mirror changes for `LineGraphQueryEngine`

---

## Phase 4: Update All Consumers

### 4.1 Server engine dispatch — `hanoi-server/src/engine.rs`

Change both `dispatch_normal` and `dispatch_line_graph` to return
`Result<Value, CoordRejection>`:

```rust
fn dispatch_normal(engine: &mut QueryEngine<'_>, req: QueryRequest) -> Result<Value, CoordRejection> {
    let format = req.format.clone();
    let answer = if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
        (req.from_lat, req.from_lng, req.to_lat, req.to_lng)
    {
        engine.query_coords((flat, flng), (tlat, tlng))?  // propagate rejection
    } else if let (Some(from), Some(to)) = (req.from_node, req.to_node) {
        engine.query(from, to)
    } else {
        None
    };
    // ... existing logging ...
    Ok(format_response(answer, format.as_deref()))
}
```

In `run_normal` and `run_line_graph`, update the dispatch call sites:

```rust
match msg {
    Ok(Some(qm)) => {
        let resp = dispatch_normal(&mut engine, qm.request);
        let _ = qm.reply.send(resp);  // now sends Result<Value, CoordRejection>
    }
    // ...
}
```

### 4.2 Server channel type — `hanoi-server/src/state.rs`

Change `QueryMsg.reply` to carry the `Result`:

```rust
use hanoi_core::CoordRejection;

pub struct QueryMsg {
    pub request: QueryRequest,
    pub reply: tokio::sync::oneshot::Sender<Result<serde_json::Value, CoordRejection>>,
}
```

### 4.3 Server handler — `hanoi-server/src/handlers.rs`

Change `handle_query` return type to `Result` for proper HTTP 400 responses:

```rust
pub async fn handle_query(
    State(state): State<AppState>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let msg = QueryMsg { request: req, reply: tx };

    if state.query_tx.send(msg).await.is_err() {
        return Ok(Json(serde_json::to_value(QueryResponse::empty()).unwrap()));
    }

    match rx.await {
        Ok(Ok(resp)) => Ok(Json(resp)),
        Ok(Err(rejection)) => {
            tracing::warn!(%rejection, "coordinate validation failed");
            Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "coordinate_validation_failed",
                    "message": rejection.to_string(),
                    "details": rejection.to_details_json(),
                })),
            ))
        }
        Err(_) => Ok(Json(serde_json::to_value(QueryResponse::empty()).unwrap())),
    }
}
```

### 4.4 Server `/info` — bounding box in response

**`hanoi-server/src/types.rs`** — add `BboxInfo` and include in `InfoResponse`:

```rust
#[derive(Serialize, Clone)]
pub struct BboxInfo {
    pub min_lat: f32,
    pub max_lat: f32,
    pub min_lng: f32,
    pub max_lng: f32,
}

#[derive(Serialize)]
pub struct InfoResponse {
    pub graph_type: String,
    pub num_nodes: usize,
    pub num_edges: usize,
    pub customization_active: bool,
    pub bbox: Option<BboxInfo>,  // NEW
}
```

**`hanoi-server/src/state.rs`** — add `bbox` to `AppState`:

```rust
pub struct AppState {
    // ... existing fields ...
    pub bbox: Option<BboxInfo>,  // NEW
}
```

**`hanoi-server/src/main.rs`** — compute bbox at startup from graph data
(lines ~175-210, after loading context but before `std::thread::spawn`):

For normal mode use `context.graph.latitude` / `context.graph.longitude`.
For line-graph mode use `context.original_latitude` /
`context.original_longitude` (the actual geographic coverage).

```rust
let bbox_info = {
    let bb = BoundingBox::from_coords(&lat_slice, &lng_slice);
    Some(BboxInfo { min_lat: bb.min_lat, max_lat: bb.max_lat,
                    min_lng: bb.min_lng, max_lng: bb.max_lng })
};
```

Pass `bbox` into the `AppState` struct literal.

**`hanoi-server/src/handlers.rs`** — update `handle_info`:

```rust
pub async fn handle_info(State(state): State<AppState>) -> Json<InfoResponse> {
    Json(InfoResponse {
        graph_type: if state.is_line_graph { "line_graph".into() } else { "normal".into() },
        num_nodes: state.num_nodes,
        num_edges: state.num_edges,
        customization_active: state.is_customization_active(),
        bbox: state.bbox.clone(),
    })
}
```

### 4.5 CLI — `hanoi-cli/src/main.rs`

Update the `query_coords` call (line ~137) to handle `Result`:

```rust
} else if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) = (from_lat, from_lng, to_lat, to_lng) {
    match engine.query_coords((flat, flng), (tlat, tlng)) {
        Ok(answer) => answer,
        Err(rejection) => {
            tracing::error!(%rejection, "coordinate validation failed");
            std::process::exit(2);  // distinct from exit 1 (no path)
        }
    }
}
```

### 4.6 Gateway — `hanoi-gateway/src/proxy.rs`

The gateway proxies requests. The backend server now returns HTTP 400 for
invalid coordinates, but the current gateway code silently converts all backend
responses to 200. Fix `handle_query` to propagate backend error status:

```rust
pub async fn handle_query(
    State(state): State<GatewayState>,
    Json(req): Json<GatewayQueryRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let backend = state
        .backend_url(&req.graph_type)
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("unknown graph_type: {}", req.graph_type)}))))?;

    // ... build and send backend_req (unchanged) ...

    let resp = state.client.post(format!("{}/query", backend))
        .json(&backend_req).send().await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": format!("backend error: {e}")}))))?;

    let status = resp.status();
    let body: Value = resp.json().await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": format!("invalid backend response: {e}")}))))?;

    if status.is_client_error() {
        Err((StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_REQUEST), Json(body)))
    } else {
        Ok(Json(body))
    }
}
```

### 4.7 Bench — `hanoi-bench/src/core.rs`

Update `bench_query_coords` (line ~128) for the new `Result` return:

```rust
if engine.query_coords(from, to).is_ok_and(|v| v.is_some()) {
    found += 1;
}
```

And in the warmup loop (line ~114):

```rust
let _ = engine.query_coords(from, to);
```

This is already a wildcard discard so it works with `Result`.

### Files changed

- `CCH-Hanoi/crates/hanoi-server/src/state.rs` — `QueryMsg.reply` type, `bbox` in `AppState`
- `CCH-Hanoi/crates/hanoi-server/src/types.rs` — add `BboxInfo`, update `InfoResponse`
- `CCH-Hanoi/crates/hanoi-server/src/handlers.rs` — `handle_query` returns `Result`, `handle_info` includes bbox
- `CCH-Hanoi/crates/hanoi-server/src/engine.rs` — `dispatch_*` return `Result`, propagate `CoordRejection`
- `CCH-Hanoi/crates/hanoi-server/src/main.rs` — compute bbox at startup, add to `AppState`
- `CCH-Hanoi/crates/hanoi-cli/src/main.rs` — handle `Result` from `query_coords`
- `CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs` — propagate backend HTTP status codes
- `CCH-Hanoi/crates/hanoi-bench/src/core.rs` — update `query_coords` calls

---

## Edge Cases

| Case | Layer | Behavior |
|---|---|---|
| NaN lat/lng | `validate_coordinate` | `NonFinite` rejection |
| Infinity / -Infinity | `validate_coordinate` | `NonFinite` rejection |
| Lat = 91.0 (impossible) | `validate_coordinate` | `InvalidRange` rejection |
| Tokyo coords to Hanoi server | `validate_coordinate` | `OutOfBounds` rejection |
| Coords in Hanoi river (no road nearby) | `validated_snap` | `SnapTooFar` rejection |
| Coords exactly on bbox edge | `contains_with_padding` | Accepted (within 1km padding) |
| Coords 500m outside bbox edge | `contains_with_padding` | Accepted (within padding) |
| Coords 2km outside bbox edge | `contains_with_padding` | `OutOfBounds` rejection |
| Node-ID queries (not coords) | Not validated | Pass through unchanged — internal IDs |
| -0.0 latitude | `is_finite()` true | Accepted (valid IEEE 754) |

---

## API Error Response Format

HTTP 400 response body:

```json
{
    "error": "coordinate_validation_failed",
    "message": "origin coordinate (35.6762, 139.6503) is outside the map's coverage area (lat [20.85, 21.15], lng [105.65, 106.05], 1000m padding)",
    "details": {
        "reason": "out_of_bounds",
        "label": "origin",
        "lat": 35.6762,
        "lng": 139.6503,
        "bbox": { "min_lat": 20.85, "max_lat": 21.15, "min_lng": 105.65, "max_lng": 106.05 },
        "padding_m": 1000.0
    }
}
```

---

## Implementation Sequence

Phases 1-2 are additive (no breaking changes). Phases 3-4 are **breaking**
(change `query_coords` return type) and must be done together.

| Phase | Files | Nature |
|---|---|---|
| 1 | `bounds.rs` (new), `lib.rs`, `Cargo.toml` | Additive — new module |
| 2 | `spatial.rs` | Additive — new field + method |
| 3 | `cch.rs`, `line_graph.rs` | **Breaking** — `query_coords` signature |
| 4 | `state.rs`, `types.rs`, `handlers.rs`, `engine.rs`, `main.rs`, `cli/main.rs`, `proxy.rs`, `core.rs` | Consumer updates |

---

## Verification

After implementation:

1. `cargo check --workspace` must pass with zero errors and zero warnings
2. Verify `/info` response includes `bbox` field
3. Test manually (or via curl) that sending Tokyo coordinates to the server
   returns HTTP 400 with the structured error JSON
4. Verify that valid Hanoi coordinates still return 200 with route data
5. Verify that coordinates in a Hanoi river/lake return `snap_too_far`
