# Fixes and Design Observations — Implementation Plan (2026-03-18)

## Context

The 2026-03-18 audit found **3 confirmed issues**, **2 by-design behaviors** needing
documentation, and **6 design observations**. The user wants:

- Recommended fix added for **Finding 5** (`/customize` race condition)
- Fixes implemented for **Design Observations B, C, D, E**
- All confirmed issues addressed

Observation **A** (coordinate boundary validation) is already planned separately in
`docs/planned/Coordinate Boundary Validation.md` — out of scope here.
Observation **F** (arc-based line-graph ordering) is a performance optimization with no
correctness impact — out of scope here.

---

## Fix 1 — Bench: Deduplicate `percentile_sorted`

**Severity**: Very Low — DRY violation, no correctness impact
**Audit reference**: Confirmed Issue #6

### Problem

`percentile_sorted` is defined identically in two files:

- [hanoi-bench/src/report.rs:302-307](CCH-Hanoi/crates/hanoi-bench/src/report.rs#L302-L307)
  — `fn percentile_sorted(sorted: &[f64], p: f64) -> f64`, called at lines 66, 67, 68
- [hanoi-bench/src/server.rs:177-183](CCH-Hanoi/crates/hanoi-bench/src/server.rs#L177-L183)
  — identical definition, called at lines 92, 93

### Changes

**[hanoi-bench/src/lib.rs](CCH-Hanoi/crates/hanoi-bench/src/lib.rs)** — add after line 43:

```rust
/// Compute percentile from a pre-sorted slice. `p` is 0–100.
pub fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
```

**[hanoi-bench/src/report.rs](CCH-Hanoi/crates/hanoi-bench/src/report.rs)** — delete lines 302–307
(the local `fn percentile_sorted`). Call sites at lines 66–68 remain unchanged; they resolve
to `crate::percentile_sorted` automatically.

**[hanoi-bench/src/server.rs](CCH-Hanoi/crates/hanoi-bench/src/server.rs)** — delete lines 177–183
(the local `fn percentile_sorted`). Call sites at lines 92–93 remain unchanged for the same
reason.

No `use` imports needed — both files are in the same crate as `lib.rs`.

### Verification

```bash
cargo build -p hanoi-bench   # must compile with no unused-function warnings
```

---

## Fix 2 — Gateway: Propagate HTTP error status codes from `/info`

**Severity**: Medium — API consumers cannot detect backend errors
**Audit reference**: Confirmed Issue #1

### Problem

**`handle_query`** at [hanoi-gateway/src/proxy.rs:55-83](CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs#L55-L83)
already checks `resp.status()` (line 68 `let status = resp.status();`) and propagates client
errors via the `if status.is_client_error()` branch at line 76. This part is correct.

**`handle_info`** at [hanoi-gateway/src/proxy.rs:98-112](CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs#L98-L112)
does **not** check status at all:

```rust
// CURRENT — no status check, always returns 200
let resp = state.client.get(format!("{}/info", backend)).send().await
    .map_err(|e| (StatusCode::BAD_GATEWAY, format!("backend error: {e}")))?;
let body: Value = resp.json().await
    .map_err(|e| (StatusCode::BAD_GATEWAY, format!("invalid backend response: {e}")))?;
Ok(Json(body))   // ← returns 200 even if backend returned 500
```

### Changes

**[hanoi-gateway/src/proxy.rs](CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs)** — update
`handle_info` (lines 98–112) to add status propagation, mirroring `handle_query`:

```rust
pub async fn handle_info(
    State(state): State<GatewayState>,
    Query(params): Query<InfoQuery>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let graph_type = params.graph_type.as_deref().unwrap_or("normal");

    let backend = state.backend_url(graph_type).ok_or((
        StatusCode::BAD_REQUEST,
        format!("unknown graph_type: {}", graph_type),
    ))?;

    let resp = state
        .client
        .get(format!("{}/info", backend))
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("backend error: {e}")))?;

    // NEW: capture status before consuming body
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| {
        (StatusCode::BAD_GATEWAY, format!("invalid backend response: {e}"))
    })?;

    // NEW: propagate non-2xx responses
    if status.is_client_error() || status.is_server_error() {
        Err((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            format!("backend returned {status}"),
        ))
    } else {
        Ok(Json(body))
    }
}
```

The return type `Result<Json<Value>, (StatusCode, String)>` is unchanged.

### Verification

- Point gateway at a stub backend that returns `500 Internal Server Error` for `/info`
- `GET http://gateway:50051/info?graph_type=normal` must return 500, not 200
- `handle_query` must remain unchanged and still propagate errors correctly

---

## Fix 3 — CLI: Add line-graph query mode

**Severity**: Medium — CLI cannot test line-graph queries offline
**Audit reference**: Confirmed Issue #3

### Problem

[hanoi-cli/src/main.rs](CCH-Hanoi/crates/hanoi-cli/src/main.rs) line 7 only imports:

```rust
use hanoi_core::{CchContext, GraphData, QueryEngine};
```

`Command::Query` (lines 40–64) has no `--line-graph` flag. The match arm (lines 120–165)
unconditionally calls `CchContext::load_and_build`. There is no path for
`LineGraphCchContext` or `LineGraphQueryEngine`.

`Command::Info` (lines 67–71) shows only `num_nodes`/`num_edges` with no graph type
indication.

### Changes

**[hanoi-cli/src/main.rs](CCH-Hanoi/crates/hanoi-cli/src/main.rs)**

**a) Imports** — replace line 7:

```rust
use hanoi_core::{CchContext, GraphData, LineGraphCchContext, LineGraphQueryEngine, QueryEngine};
```

**b) `Command::Query` variant** — add `--line-graph` flag after the `data_dir` field (line 43):

```rust
Query {
    /// Parent data directory (contains graph/ and optionally line_graph/ subdirectories)
    #[arg(long)]
    data_dir: PathBuf,

    /// Query the turn-expanded line graph instead of the normal graph.
    /// Expects data_dir/line_graph/ and data_dir/graph/ to both exist.
    #[arg(long, default_value_t = false)]
    line_graph: bool,

    // from_node, to_node, from_lat, from_lng, to_lat, to_lng — unchanged
    ...
},
```

**c) `Command::Query` match arm** — replace lines 119–165 with the conditional dispatch below.
The output JSON block (`match answer { Some(a) => ... }`) is identical for both modes and is
kept as-is at the end:

```rust
Command::Query {
    data_dir, line_graph,
    from_node, to_node, from_lat, from_lng, to_lat, to_lng,
} => {
    let answer = if line_graph {
        let lg_dir = data_dir.join("line_graph");
        let original_dir = data_dir.join("graph");
        let perm_path = lg_dir.join("perms/cch_perm");

        tracing::info!(?lg_dir, ?original_dir, "loading line graph");
        let t0 = Instant::now();
        let context = LineGraphCchContext::load_and_build(&lg_dir, &original_dir, &perm_path)
            .expect("failed to load line graph");
        tracing::info!(elapsed = ?t0.elapsed(), "DirectedCCH built");

        let t1 = Instant::now();
        let mut engine = LineGraphQueryEngine::new(&context);
        tracing::info!(elapsed = ?t1.elapsed(), "initial customization + spatial index");

        if let (Some(from), Some(to)) = (from_node, to_node) {
            engine.query(from, to)
        } else if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
            (from_lat, from_lng, to_lat, to_lng)
        {
            match engine.query_coords((flat, flng), (tlat, tlng)) {
                Ok(answer) => answer,
                Err(rejection) => {
                    tracing::error!(%rejection, "coordinate validation failed");
                    std::process::exit(2);
                }
            }
        } else {
            tracing::error!("specify either --from-node/--to-node or coordinate flags");
            std::process::exit(1);
        }
    } else {
        // Existing normal-graph path — unchanged from current lines 120–149
        let graph_dir = data_dir.join("graph");
        let perm_path = graph_dir.join("perms/cch_perm");
        tracing::info!(?graph_dir, "loading graph");
        let t0 = Instant::now();
        let context = CchContext::load_and_build(&graph_dir, &perm_path)
            .expect("failed to load graph");
        tracing::info!(elapsed = ?t0.elapsed(), "CCH built");
        let t1 = Instant::now();
        let mut engine = QueryEngine::new(&context);
        tracing::info!(elapsed = ?t1.elapsed(), "initial customization + spatial index");
        if let (Some(from), Some(to)) = (from_node, to_node) {
            engine.query(from, to)
        } else if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
            (from_lat, from_lng, to_lat, to_lng)
        {
            match engine.query_coords((flat, flng), (tlat, tlng)) {
                Ok(answer) => answer,
                Err(rejection) => {
                    tracing::error!(%rejection, "coordinate validation failed");
                    std::process::exit(2);
                }
            }
        } else {
            tracing::error!("specify either --from-node/--to-node or coordinate flags");
            std::process::exit(1);
        }
    };

    // Output — same structure for both modes
    match answer {
        Some(a) => {
            let output = serde_json::json!({
                "distance_ms": a.distance_ms,
                "distance_m": a.distance_m,
                "path_nodes": a.path,
                "coordinates": a.coordinates.iter().map(|&(lat, lng)| [lat, lng]).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        None => {
            tracing::warn!("no path found");
            std::process::exit(1);
        }
    }
}
```

**d) `Command::Info` variant** — add `--line-graph` flag and show graph type in output:

```rust
Info {
    #[arg(long)]
    data_dir: PathBuf,

    /// Show info for the line graph instead of the normal graph.
    #[arg(long, default_value_t = false)]
    line_graph: bool,
},
```

Match arm body (replaces current lines 168–177):

```rust
Command::Info { data_dir, line_graph } => {
    let (graph_dir, graph_type) = if line_graph {
        (data_dir.join("line_graph"), "line_graph")
    } else {
        (data_dir.join("graph"), "normal")
    };
    let graph = GraphData::load(&graph_dir).expect("failed to load graph");
    let output = serde_json::json!({
        "graph_type": graph_type,
        "graph_dir": graph_dir.display().to_string(),
        "num_nodes": graph.num_nodes(),
        "num_edges": graph.num_edges(),
    });
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
```

**[hanoi-cli/Cargo.toml](CCH-Hanoi/crates/hanoi-cli/Cargo.toml)** — no changes needed.
`hanoi-core` already re-exports `LineGraphCchContext` and `LineGraphQueryEngine` (confirmed
in `hanoi-core/src/lib.rs`).

### Verification

```bash
# Normal mode — regression test
cch-hanoi query --data-dir Maps/data/hanoi_car --from-node 0 --to-node 100

# Line-graph mode
cch-hanoi query --data-dir Maps/data/hanoi_car --line-graph --from-node 0 --to-node 100

# Info with graph type shown
cch-hanoi info --data-dir Maps/data/hanoi_car --line-graph
# expected output includes: "graph_type": "line_graph"
```

---

## Fix 4 — Server: Document async `/customize` and watch channel semantics

**Severity**: Low — undocumented behavior causes test automation surprises
**Audit reference**: Confirmed Issues #4 (by-design) and #5 (by-design, race condition)

### Changes

**Create new file: [hanoi-server/README.md](CCH-Hanoi/crates/hanoi-server/README.md)**

Document the following:

- Dual-port architecture table (8080 query, 9080 customize)
- `/customize` returns 200 **before** customization completes — it only queues the weight
  vector into the `watch::channel` (`handlers.rs:74`: `let _ = state.watch_tx.send(Some(weights))`)
- The background engine thread reads the watch channel at the top of its loop
  (`engine.rs:30-39`) and sets `customization_active` to `true` during re-customization
- Watch-channel semantics: `borrow_and_update()` always returns the latest value; if
  `/customize` is called twice in rapid succession, the first weight vector may be silently
  dropped — this is intentional for live-traffic updates
- **Race condition mitigation strategies**:
  1. **Poll `/info`** — after calling `/customize`, poll `GET /info` checking
     `customization_active`. Loop with exponential backoff (start 10ms, max 500ms).
     Transition `true → false` signals completion. Example:
     ```
     POST /customize  →  {"accepted": true}
     GET  /info       →  {"customization_active": true}   (wait 10ms)
     GET  /info       →  {"customization_active": false}  (done)
     ```
  2. **Fixed sleep** — wait 100–200ms after `/customize` returns. The
     `bench_query_after_customize` function in `hanoi-bench/src/server.rs:172` uses
     `tokio::time::sleep(Duration::from_millis(100))`. Acceptable for benchmarks; not
     reliable for production under load.
  3. **Future enhancement** — a dedicated `GET /customize/status` endpoint or WebSocket
     push could provide a proper completion signal without polling.

---

## Fix 5 — Server: Graceful Shutdown (Design Observation B)

**Severity**: Low — production servers must respond to SIGTERM/SIGINT
**Audit reference**: Design Observation B

### Problem

[hanoi-server/src/main.rs:282-289](CCH-Hanoi/crates/hanoi-server/src/main.rs#L282-L289)
runs both listeners with no shutdown signal:

```rust
tokio::spawn(async move {
    axum::serve(customize_listener, customize_router).await.unwrap();
});
axum::serve(query_listener, query_router).await.unwrap();
```

`_guard` (the `tracing-appender` `WorkerGuard`) is held on the stack in `main`. It is only
dropped when `main` returns. Since `axum::serve(...)` never returns unless interrupted,
`_guard` is never dropped, meaning file log flushing never happens on a graceful kill.

### Changes

**[hanoi-server/src/main.rs](CCH-Hanoi/crates/hanoi-server/src/main.rs)**

Axum's `axum::serve(listener, router)` returns a `Serve` builder with
`.with_graceful_shutdown(signal)`. The signal is any `Future<Output = ()>`.

Replace lines 282–289 with:

```rust
// Broadcast channel: one send notifies both listeners to begin graceful shutdown
let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

let customize_shutdown = {
    let mut rx = shutdown_tx.subscribe();
    async move { let _ = rx.recv().await; }
};
let query_shutdown = {
    let mut rx = shutdown_tx.subscribe();
    async move { let _ = rx.recv().await; }
};

// Customize listener in background with its own shutdown receiver
tokio::spawn(async move {
    axum::serve(customize_listener, customize_router)
        .with_graceful_shutdown(customize_shutdown)
        .await
        .unwrap();
});

// Wait for SIGINT or SIGTERM, then broadcast shutdown
tokio::select! {
    _ = tokio::signal::ctrl_c() => {
        tracing::info!("SIGINT received, initiating graceful shutdown");
    }
    #[cfg(unix)]
    _ = async {
        let mut sig = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate()
        ).expect("failed to install SIGTERM handler");
        sig.recv().await
    } => {
        tracing::info!("SIGTERM received, initiating graceful shutdown");
    }
}

let _ = shutdown_tx.send(());

axum::serve(query_listener, query_router)
    .with_graceful_shutdown(query_shutdown)
    .await
    .unwrap();

tracing::info!("shutdown complete");
// _guard drops here — flushes tracing-appender file writer
```

**Dependencies**: `tokio::signal` and `tokio::sync::broadcast` are both part of
`tokio = { features = ["full"] }` already in [hanoi-server/Cargo.toml](CCH-Hanoi/crates/hanoi-server/Cargo.toml#L10).
No new dependencies required.

### Verification

```bash
# Start server, then:
kill -SIGTERM <pid>    # logs "SIGTERM received" then "shutdown complete", exits 0
kill -INT <pid>        # same for SIGINT / Ctrl-C
```

---

## Fix 6 — Server: Health and Readiness Endpoints (Design Observation C)

**Severity**: Low — load balancers and orchestrators need liveness/readiness signals
**Audit reference**: Design Observation C

### Problem

There is no `/health` or `/ready` endpoint. `GET /info` is used as a de-facto health check,
but `InfoResponse` only contains `customization_active` — it does not indicate whether the
background engine thread is still running. If the engine thread panics, the server appears
healthy while being unable to serve queries.

### Changes

**[hanoi-server/src/state.rs](CCH-Hanoi/crates/hanoi-server/src/state.rs)**

Add a second `Arc<AtomicBool>` to track engine thread liveness. Add field after line 33:

```rust
/// Whether the background engine thread is still alive.
/// Set to false by the engine thread before it exits.
pub engine_alive: Arc<AtomicBool>,
```

Add helper after `is_customization_active` (line 38):

```rust
pub fn is_engine_alive(&self) -> bool {
    self.engine_alive.load(Ordering::Relaxed)
}
```

**[hanoi-server/src/engine.rs](CCH-Hanoi/crates/hanoi-server/src/engine.rs)**

Add `engine_alive: &Arc<AtomicBool>` parameter to both `run_normal` (line 19) and
`run_line_graph` (line 59). After each `loop { }` block, set it to `false`:

```rust
pub fn run_normal(
    context: &CchContext,
    query_rx: &mut mpsc::Receiver<QueryMsg>,
    watch_rx: &mut watch::Receiver<Option<Vec<Weight>>>,
    customization_active: &Arc<AtomicBool>,
    engine_alive: &Arc<AtomicBool>,   // NEW parameter
    rt: &tokio::runtime::Handle,
) {
    let mut engine = QueryEngine::new(context);
    loop {
        // ... unchanged ...
        Ok(None) => break, // Channel closed — shutdown
    }
    engine_alive.store(false, Ordering::Relaxed);  // NEW — mark dead before returning
}
```

Same pattern for `run_line_graph`.

**[hanoi-server/src/main.rs](CCH-Hanoi/crates/hanoi-server/src/main.rs)**

After line 181 (`let customization_active = Arc::new(AtomicBool::new(false));`), add:

```rust
let engine_alive = Arc::new(AtomicBool::new(true));
```

Pass `engine_alive.clone()` into both `std::thread::spawn` closures alongside
`customization_active`. Include in the `AppState` struct construction at line 240:

```rust
let state = AppState {
    query_tx,
    watch_tx,
    num_edges,
    num_nodes,
    is_line_graph: args.line_graph,
    bbox,
    customization_active,
    engine_alive,   // NEW
};
```

Add routes to the query router (lines 251–255):

```rust
let query_router = Router::new()
    .route("/query", post(handlers::handle_query))
    .route("/info", get(handlers::handle_info))
    .route("/health", get(handlers::handle_health))   // NEW
    .route("/ready", get(handlers::handle_ready))     // NEW
    .layer(TraceLayer::new_for_http())
    .with_state(state.clone());
```

**[hanoi-server/src/types.rs](CCH-Hanoi/crates/hanoi-server/src/types.rs)**

Add after `InfoResponse` (after line 76):

```rust
/// Response from GET /health — always 200 while the process is alive.
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}

/// Response from GET /ready — 503 if the engine thread has died.
#[derive(Serialize)]
pub struct ReadyResponse {
    pub ready: bool,
}
```

**[hanoi-server/src/handlers.rs](CCH-Hanoi/crates/hanoi-server/src/handlers.rs)**

Add `HealthResponse` and `ReadyResponse` to the import on line 8:

```rust
use crate::types::{
    CustomizeResponse, HealthResponse, InfoResponse, QueryRequest, QueryResponse, ReadyResponse,
};
```

Add two new handlers after `handle_info`:

```rust
/// GET /health — liveness check. Always 200 while the process is running.
pub async fn handle_health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// GET /ready — readiness check. Returns 503 if the engine thread has died.
pub async fn handle_ready(
    State(state): State<AppState>,
) -> Result<Json<ReadyResponse>, (StatusCode, Json<ReadyResponse>)> {
    if state.is_engine_alive() {
        Ok(Json(ReadyResponse { ready: true }))
    } else {
        Err((StatusCode::SERVICE_UNAVAILABLE, Json(ReadyResponse { ready: false })))
    }
}
```

### Verification

```bash
curl http://localhost:8080/health
# {"status":"ok"}  → HTTP 200

curl http://localhost:8080/ready
# {"ready":true}   → HTTP 200 (engine alive)

# After simulating engine thread death (e.g. crash or channel close):
# {"ready":false}  → HTTP 503
```

---

## Fix 7 — Gateway: Request Timeout (Design Observation D)

**Severity**: Low — gateway hangs indefinitely if a backend stalls
**Audit reference**: Design Observation D

### Problem

[hanoi-gateway/src/proxy.rs:18-24](CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs#L18-L24):

```rust
impl GatewayState {
    pub fn new(normal_url: &str, line_graph_url: &str) -> Self {
        GatewayState {
            client: Client::new(),   // ← default timeout = none
            ...
        }
    }
}
```

`reqwest::Client::new()` uses library defaults — no `connect_timeout`, no `timeout`. A
stalled backend causes the gateway to hold the connection forever.

### Changes

**[hanoi-gateway/src/proxy.rs](CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs)**

Update `GatewayState::new` signature to accept a timeout and build the client via `builder()`:

```rust
impl GatewayState {
    pub fn new(normal_url: &str, line_graph_url: &str, timeout_secs: u64) -> Self {
        let client = if timeout_secs > 0 {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(timeout_secs))
                .build()
                .expect("failed to build reqwest client")
        } else {
            reqwest::Client::new()   // 0 = no timeout (opt-out)
        };
        GatewayState {
            client,
            normal_url: normal_url.trim_end_matches('/').to_string(),
            line_graph_url: line_graph_url.trim_end_matches('/').to_string(),
        }
    }
    ...
}
```

**[hanoi-gateway/src/main.rs](CCH-Hanoi/crates/hanoi-gateway/src/main.rs)**

Add argument to `Args` struct (after `line_graph_backend`, around line 50):

```rust
/// Backend request timeout in seconds. Set to 0 to disable.
#[arg(long, default_value = "30")]
backend_timeout_secs: u64,
```

Update `GatewayState::new` call (line 102):

```rust
let state = GatewayState::new(
    &args.normal_backend,
    &args.line_graph_backend,
    args.backend_timeout_secs,
);
```

No new Cargo.toml dependencies — `reqwest::Client::builder()` is part of the existing
`reqwest = { version = "0.12", features = ["json"] }` in
[hanoi-gateway/Cargo.toml](CCH-Hanoi/crates/hanoi-gateway/Cargo.toml#L12).

### Verification

```bash
# Start a stub backend that never responds, then:
hanoi_gateway --backend-timeout-secs 2 --normal-backend http://localhost:9999

curl -X POST http://localhost:50051/query \
  -H 'Content-Type: application/json' \
  -d '{"graph_type":"normal","from_node":0,"to_node":1}'
# Must return 502 Bad Gateway within ~2 seconds, not hang indefinitely
```

---

## Fix 8 — Core: Replace panicking asserts with `Result` (Design Observation E)

**Severity**: Low — library crate should not panic on invalid input
**Audit reference**: Design Observation E

### Problem

[hanoi-core/src/graph.rs:32-51](CCH-Hanoi/crates/hanoi-core/src/graph.rs#L32-L51) has four
`assert_eq!` calls validating CSR invariants:

```rust
assert_eq!(*first_out.last().expect("first_out must be non-empty") as usize, num_edges, "...");
assert_eq!(latitude.len(), num_nodes, "...");
assert_eq!(longitude.len(), num_nodes, "...");
assert_eq!(travel_time.len(), num_edges, "...");
```

A truncated or corrupt file triggers `panic!` and a backtrace — not an actionable error
message. `GraphData::load` already returns `std::io::Result<Self>`, so the fix is purely
internal.

### Changes

**[hanoi-core/src/graph.rs](CCH-Hanoi/crates/hanoi-core/src/graph.rs)**

Add a local helper function before `GraphData::load` (or inline after the loads):

```rust
use std::io::{Error, ErrorKind};

fn graph_check(cond: bool, msg: impl Into<String>) -> std::io::Result<()> {
    if cond {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::InvalidData, msg.into()))
    }
}
```

Replace the four `assert_eq!` blocks (lines 32–51) with:

```rust
let sentinel = first_out
    .last()
    .ok_or_else(|| Error::new(ErrorKind::InvalidData, "first_out is empty — no sentinel"))?;
graph_check(
    *sentinel as usize == num_edges,
    format!("first_out sentinel ({}) != num_edges ({})", sentinel, num_edges),
)?;
graph_check(
    latitude.len() == num_nodes,
    format!("latitude.len() ({}) != num_nodes ({})", latitude.len(), num_nodes),
)?;
graph_check(
    longitude.len() == num_nodes,
    format!("longitude.len() ({}) != num_nodes ({})", longitude.len(), num_nodes),
)?;
graph_check(
    travel_time.len() == num_edges,
    format!("travel_time.len() ({}) != num_edges ({})", travel_time.len(), num_edges),
)?;
```

**Call-site impact** — all callers already handle `std::io::Result`:

| File | Call | Current pattern | Action needed |
|------|------|-----------------|---------------|
| [hanoi-core/src/cch.rs:51](CCH-Hanoi/crates/hanoi-core/src/cch.rs#L51) | `GraphData::load(graph_dir)?` | `?` propagates | None |
| [hanoi-core/src/line_graph.rs:48](CCH-Hanoi/crates/hanoi-core/src/line_graph.rs#L48) | `GraphData::load(line_graph_dir)?` | `?` propagates | None |
| [hanoi-server/src/main.rs:192](CCH-Hanoi/crates/hanoi-server/src/main.rs#L192) | `.expect("failed to load line graph")` | panic with message | Acceptable at startup |
| [hanoi-server/src/main.rs:217](CCH-Hanoi/crates/hanoi-server/src/main.rs#L217) | `.expect("failed to load graph")` | panic with message | Acceptable at startup |
| [hanoi-cli/src/main.rs:126](CCH-Hanoi/crates/hanoi-cli/src/main.rs#L126) | `.expect("failed to load graph")` | panic with message | Acceptable at startup |
| [hanoi-cli/src/main.rs:170](CCH-Hanoi/crates/hanoi-cli/src/main.rs#L170) | `.expect("failed to load graph")` | panic with message | Acceptable at startup |

No call-site changes required.

### Verification

```bash
# Truncate travel_time to an invalid size
truncate -s 0 Maps/data/hanoi_car/graph/travel_time

# Server startup must print a clear error and exit — not a backtrace
hanoi_server --graph-dir Maps/data/hanoi_car/graph ...
# Expected: "failed to load graph: travel_time.len() (0) != num_edges (456789)"
```

---

## Implementation Order

| # | Fix | Files touched | Rationale |
|---|-----|---------------|-----------|
| 1 | Deduplicate `percentile_sorted` | `hanoi-bench/src/lib.rs`, `report.rs`, `server.rs` | Trivial, zero dependencies |
| 2 | Gateway: `handle_info` status check | `hanoi-gateway/src/proxy.rs` | Mirrors existing `handle_query` pattern |
| 3 | Gateway: request timeout | `hanoi-gateway/src/proxy.rs`, `main.rs` | Companion to Fix 2, same file |
| 4 | Core: `graph.rs` error returns | `hanoi-core/src/graph.rs` | No call-site changes, contained |
| 5 | CLI: line-graph mode | `hanoi-cli/src/main.rs` | Depends on `hanoi-core` (no changes there) |
| 6 | Server: `engine_alive` AtomicBool | `state.rs`, `engine.rs`, `main.rs` | Prerequisite for Fix 7 |
| 7 | Server: `/health` and `/ready` | `handlers.rs`, `types.rs`, `main.rs` | Depends on Fix 6 |
| 8 | Server: graceful shutdown | `main.rs` | Touches same file as Fix 7; do last in server |
| 9 | Server README | `hanoi-server/README.md` | New file, no code dependencies |

---

## File Change Summary

| File | Fixes | Nature of change |
|------|-------|-----------------|
| `CCH-Hanoi/crates/hanoi-bench/src/lib.rs` | 1 | Add `pub fn percentile_sorted` |
| `CCH-Hanoi/crates/hanoi-bench/src/report.rs` | 1 | Delete local `fn percentile_sorted` (lines 302–307) |
| `CCH-Hanoi/crates/hanoi-bench/src/server.rs` | 1 | Delete local `fn percentile_sorted` (lines 177–183) |
| `CCH-Hanoi/crates/hanoi-gateway/src/proxy.rs` | 2, 7 | Add status check in `handle_info`; add timeout to `new()` |
| `CCH-Hanoi/crates/hanoi-gateway/src/main.rs` | 7 | Add `--backend-timeout-secs`; pass to `GatewayState::new` |
| `CCH-Hanoi/crates/hanoi-core/src/graph.rs` | 8 | Replace 4× `assert_eq!` with `io::Error` returns |
| `CCH-Hanoi/crates/hanoi-cli/src/main.rs` | 5 | Add `--line-graph` to `Query`+`Info`; add `LineGraphCchContext` branch |
| `CCH-Hanoi/crates/hanoi-server/src/state.rs` | 6 | Add `engine_alive: Arc<AtomicBool>` + `is_engine_alive()` |
| `CCH-Hanoi/crates/hanoi-server/src/engine.rs` | 6 | Accept `engine_alive` param; `store(false)` on loop exit |
| `CCH-Hanoi/crates/hanoi-server/src/types.rs` | 7 | Add `HealthResponse`, `ReadyResponse` |
| `CCH-Hanoi/crates/hanoi-server/src/handlers.rs` | 7 | Add `handle_health`, `handle_ready` |
| `CCH-Hanoi/crates/hanoi-server/src/main.rs` | 6, 7, 8 | Add `engine_alive`; new routes; graceful shutdown block |
| *(new)* `CCH-Hanoi/crates/hanoi-server/README.md` | 9 | Document async `/customize`, watch semantics, race mitigations |

---

## Notes

- **No changes to `rust_road_router` or `RoutingKit`** — strictly off-limits per CLAUDE.md
- **No new Cargo dependencies** for any fix — all required crates are already present
- **All changes logged** to `docs/CHANGELOGS.md` per project convention
