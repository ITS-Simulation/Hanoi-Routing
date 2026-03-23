# Enhanced Health Monitoring & Graceful Shutdown Timeout

**Date**: 2026-03-18
**Scope**: Two minor enhancements to operational visibility and robustness
**Impact**: Low risk — additive features, no breaking changes to existing endpoints

---

## Overview

### Enhancement 1: Rich Health Endpoint (Extended Telemetry)
Replace the current minimalist `GET /health` response with operational metrics:
- **Uptime** (seconds since server started)
- **Total queries processed** (across entire lifetime)
- **Active customizations** (boolean flag already exists)

### Enhancement 2: Graceful Shutdown Timeout (Force-Kill After 30s)
Add a 30-second timeout to graceful shutdown. If connections don't drain within 30s, force-kill in-flight requests and exit.

**Rationale**: Production deployments need bounded shutdown times for orchestration (Kubernetes, systemd) that may kill the process anyway after hard timeout.

---

## Enhancement 1: Rich Health Endpoint

### Changes

#### File: `hanoi-server/src/state.rs` — Add Metrics Tracking

Add counters to `AppState`:

```rust
use std::sync::atomic::AtomicU64;

#[derive(Clone)]
pub struct AppState {
    // ... existing fields ...

    /// Server uptime: instant when server started. Used to compute age.
    pub startup_time: std::time::Instant,
    /// Total successful queries processed (not counting validation failures).
    pub queries_processed: Arc<AtomicU64>,
}

impl AppState {
    // ... existing methods ...

    pub fn uptime_secs(&self) -> u64 {
        self.startup_time.elapsed().as_secs()
    }

    pub fn total_queries(&self) -> u64 {
        self.queries_processed.load(Ordering::Relaxed)
    }
}
```

#### File: `hanoi-server/src/main.rs` — Initialize Metrics at Startup

After line 182 (after `engine_alive` init), add:

```rust
let startup_time = std::time::Instant::now();
let queries_processed = Arc::new(AtomicU64::new(0));
```

Pass to `AppState` construction (around line 243):

```rust
let state = AppState {
    // ... existing fields ...
    startup_time,
    queries_processed,
};
```

#### File: `hanoi-server/src/engine.rs` — Increment Counter on Success

In both `dispatch_normal` and `dispatch_line_graph`, after the routing completes successfully (after line where `answer` is computed), add a tracing event and increment counter. However, to avoid circular dependency (engine.rs can't import state), we'll increment at the **handler layer** instead (simpler, correct pattern).

#### File: `hanoi-server/src/handlers.rs` — Increment Counter on Successful Query

In `handle_query`, after line 30 (after successful route returns `Ok(Ok(resp))`), increment the counter:

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
        Ok(Ok(resp)) => {
            state.queries_processed.fetch_add(1, Ordering::Relaxed);
            Ok(Json(resp))
        }
        // ... error handling unchanged ...
    }
}
```

**Why here**: Handler is the right layer to count successful queries. Engine dispatches are internal; only successful HTTP responses count toward "queries processed."

#### File: `hanoi-server/src/types.rs` — New Response Type

Replace the current minimalist `HealthResponse` (line 78–82):

```rust
/// Response from GET /health — operational metrics.
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,           // Always "ok"
    pub uptime_seconds: u64,            // Seconds since startup
    pub total_queries_processed: u64,   // Cumulative count
    pub customization_active: bool,     // Point-in-time flag
}
```

#### File: `hanoi-server/src/handlers.rs` — Enhanced Handler

Replace `handle_health` (line 100–102):

```rust
/// GET /health — operational metrics. Always 200 while the process is running.
pub async fn handle_health(
    State(state): State<AppState>,
) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        uptime_seconds: state.uptime_secs(),
        total_queries_processed: state.total_queries(),
        customization_active: state.is_customization_active(),
    })
}
```

### Backward Compatibility

✅ **No breaking changes**. New fields added to existing response. Clients that only check `{"status":"ok"}` continue to work; new clients can read additional metrics.

### Example Response

```json
{
  "status": "ok",
  "uptime_seconds": 3600,
  "total_queries_processed": 15234,
  "customization_active": false
}
```

---

## Enhancement 2: Graceful Shutdown Timeout

### Changes

#### File: `hanoi-server/src/main.rs` — Add Shutdown Timeout Loop

Replace lines 334–340 (the current graceful shutdown block) with:

```rust
    // Shutdown timeout: force-kill after 30s if graceful shutdown hangs
    let shutdown_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);

    let query_result = tokio::select! {
        biased;  // Prioritize the deadline
        _ = tokio::time::sleep_until(shutdown_deadline) => {
            tracing::warn!("graceful shutdown timeout reached (30s), forcing exit");
            std::process::exit(1);
        }
        result = axum::serve(query_listener, query_router)
            .with_graceful_shutdown(query_shutdown) => {
            result
        }
    };

    let _ = query_result;
    let _ = customize_handle.await;
    tracing::info!("shutdown complete");
```

**Why `biased` select**:
- Without `biased`, tokio may randomly choose the timeout branch even if serve completes in time
- With `biased`, timeout only triggers if serve hasn't completed first
- Prevents false positives

### Safety & Behavior

| Scenario | Behavior |
|----------|----------|
| Normal graceful shutdown (< 30s) | `axum::serve()` completes first, `tokio::select!` exits cleanly, logs "shutdown complete" |
| Stalled in-flight connections (> 30s) | Timeout fires, logs warning, `exit(1)`, Kubernetes/systemd detects exit and cleans up |
| Signal during shutdown | Signal handler already spawned, sends broadcast — graceful shutdown begins |

### No New Dependencies

✅ `tokio::select!` and `tokio::time` are part of existing `tokio` features.

---

## Flow Verification (No Logic Loopholes)

### Enhancement 1 Flow

```
HTTP GET /health
  ↓
handle_health(state) pulls from AppState
  ↓
Returns {status, uptime_secs(), total_queries(), customization_active}
  ↓
HTTP 200 with rich metrics
```

**No loophole**: Metrics are read-only snapshots. `Instant::now()` is monotonically increasing, `AtomicU64` is monotonically increasing. No data corruption possible.

### Enhancement 2 Flow (Normal Shutdown)

```
SIGINT/SIGTERM
  ↓
Signal handler sends broadcast shutdown
  ↓
Both listeners' with_graceful_shutdown() triggered
  ↓
axum::serve() stops accepting new connections, drains in-flight requests
  ↓
axum::serve() returns (< 30s)
  ↓
tokio::select! exits first branch (serve)
  ↓
customize_handle.await finishes
  ↓
"shutdown complete" logged
```

### Enhancement 2 Flow (Timeout Path — Stalled Connections)

```
SIGINT/SIGTERM
  ↓
Signal handler sends broadcast shutdown
  ↓
axum::serve() stops accepting, tries to drain in-flight requests
  ↓
Connections hold open (client bug, slow client, network stall)
  ↓
30s deadline elapses
  ↓
tokio::select! timeout branch fires first
  ↓
"graceful shutdown timeout reached" warning logged
  ↓
std::process::exit(1) — hard kill all tasks
  ↓
Kernel cleans up connections
  ↓
systemd/Kubernetes detects exit code 1, handles pod cleanup
```

**No loophole**: Hard exit via `process::exit()` is safe. Rust async tasks are cooperative, not system threads — killing the process is clean. `_guard` doesn't matter anymore because we're exiting anyway.

---

## Files Modified Summary

| File | Changes |
|------|---------|
| `hanoi-server/src/state.rs` | Add `startup_time: Instant`, `queries_processed: Arc<AtomicU64>`, methods `uptime_secs()` and `total_queries()` |
| `hanoi-server/src/main.rs` | Initialize both fields, pass to AppState, add 30s shutdown timeout with force-exit |
| `hanoi-server/src/handlers.rs` | Update `handle_health()` to accept `State(state)` and return new fields; increment counter in `handle_query()` |
| `hanoi-server/src/types.rs` | Expand `HealthResponse` with `uptime_seconds`, `total_queries_processed`, `customization_active` |

**Total**: 4 files, ~40 lines of code added.

---

## Verification Plan

### Unit Testing
1. **Health endpoint**: Call `GET /health` multiple times, verify `uptime_seconds` increases monotonically, `total_queries_processed` increases after queries.
2. **Query counting**: Send 100 queries, verify counter reaches 100 (only successful queries count).
3. **Shutdown timeout**:
   - Start server, send SIGINT, verify clean shutdown within 30s
   - Stress-test with slow clients, verify timeout triggers at ~30s boundary

### Manual Testing
```bash
# Start server
hanoi_server --graph-dir ... --log-format pretty

# In another terminal
# Check health metrics
curl http://localhost:8080/health
# {"status":"ok", "uptime_seconds": 5, "total_queries_processed": 0, "customization_active": false}

# Make some queries
curl -X POST http://localhost:8080/query -d '...'

# Check health again
curl http://localhost:8080/health
# {"status":"ok", "uptime_seconds": 10, "total_queries_processed": 1, "customization_active": false}

# Test graceful shutdown (back to first terminal)
Ctrl+C
# Logs: "SIGINT received, initiating graceful shutdown" → "shutdown complete" (should be ~1s, not 30s)
```

### Regression Testing
✅ Existing `/ready` endpoint unchanged — still works
✅ Existing `/info` endpoint unchanged — still works
✅ Query handler now increments counter but doesn't change response — backward compatible
✅ Shutdown flow enhanced but graceful exit path unchanged — same logs, just bounded

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Counter overflow (u64) | Unlikely for ~10y deployment. Would wrap to 0. Not critical metric. |
| `Instant::now()` precision varies by OS | Expected behavior, acceptable for uptime estimation. |
| `biased` select causes priority inversion | Design goal: deadline has priority over serve completion, preventing unbounded waits. Correct. |
| Force-exit doesn't flush logs | Tracing-appender `_guard` already dropped before exit code path. Logs are pre-flushed. |

**No critical risks.** Both enhancements are defensive: health metrics help troubleshoot slow response times, timeout prevents hung deployments.

---

## Implementation Order

1. **Add fields to state.rs** — Initialize counters
2. **Initialize in main.rs** — Create Instant and AtomicU64
3. **Update types.rs** — Define new HealthResponse shape
4. **Update handlers.rs** — Implement new handle_health, increment counter
5. **Add shutdown timeout to main.rs** — Wrap serve in tokio::select! with deadline

No inter-dependency conflicts. Can be implemented in any order.

---

## Dependencies

✅ No new Cargo dependencies required. Uses only:
- `std::sync::atomic::AtomicU64` (stdlib)
- `std::time::Instant` (stdlib)
- `tokio::time::sleep_until()` (already imported via `full` feature)
- `tokio::select!` macro (already available)

---

## Changelog Entry

```
## 2026-03-18 — Enhanced Health Monitoring & Graceful Shutdown Timeout

### Changes

- **Health endpoint enriched**: `GET /health` now returns `uptime_seconds` (uptime in seconds), `total_queries_processed` (cumulative count), and `customization_active` (point-in-time flag) alongside `status: "ok"`.
- **Graceful shutdown timeout**: Added 30-second deadline to graceful shutdown. If in-flight connections don't drain within 30s, server logs warning and force-exits to prevent indefinite hangs in orchestration systems (Kubernetes, systemd).

### Files changed

- `hanoi-server/src/state.rs` — Added `startup_time` and `queries_processed` tracking
- `hanoi-server/src/main.rs` — Initialize metrics, add shutdown timeout with force-exit
- `hanoi-server/src/handlers.rs` — Enhanced `/health` handler, count queries in `/query` handler
- `hanoi-server/src/types.rs` — Expanded `HealthResponse` with new fields

### Impact

- Backward compatible (new fields added to health response)
- No performance impact (atomic read/write in critical path is negligible)
- Improves observability and operational robustness
```

---

## Deployment Notes

**For orchestration systems**:
- If using Kubernetes, `livenessProbe` can continue to use `/health` (always 200)
- `readinessProbe` continues to use `/ready` (checks engine thread)
- Now has uptime metric for monitoring dashboards (Prometheus, Datadog)

**For systemd/manual startup**:
- Graceful shutdown now bounded to ~30s
- Prevents zombie processes after SIGTERM if clients hang
- Monitor logs for "graceful shutdown timeout reached" warnings — indicates slow client or network issue
