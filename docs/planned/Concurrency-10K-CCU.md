# Concurrency Architecture for 10,000 CCU

Production-grade concurrency rework for the CCH-Hanoi routing server.
Target: 10,000 concurrent users on a single instance.

## Status: DRAFT — Awaiting review (amended: flume MPMC channel)

---

## 1. Current Architecture & Bottleneck

```
HTTP Handlers (async, thousands)       Engine Thread (1, blocking)
  ┌─ handle_query() ──┐                ┌──────────────────────┐
  │  handle_query()    │  flume(256)    │  QueryEngine         │
  │  handle_query()    ├───────────────→│  (&mut self)         │
  │  handle_query()    │    oneshot     │  fw/bw_distances     │
  │  handle_query() ──┘←───response────│  fw/bw_parents       │
  └────────────────────┘                └──────────────────────┘
```

**Single engine thread** processes queries sequentially. At ~1ms per
single-query, theoretical max ~1000 QPS. Multi-route queries take 5-20ms,
dropping effective throughput to ~50-200 QPS under mixed load.

All non-query endpoints (`/evaluate_routes`, `/traffic_overlay`,
`/camera_overlay`, `/health`, `/info`) already operate concurrently — they
bypass the engine thread entirely.

### Why `&mut self` Is Fundamental

The CCH elimination-tree query mutates 4 workspace buffers per query:

| Buffer | Size (4.4M nodes) | Purpose |
|---|---|---|
| `fw_distances` | 17 MB | Forward search distances |
| `bw_distances` | 17 MB | Backward search distances |
| `fw_parents` | 35 MB | Forward parent pointers |
| `bw_parents` | 35 MB | Backward parent pointers |

Uses "lazy reset" — only touched entries are reset, giving O(search space)
instead of O(n). Two concurrent queries would clobber each other's workspace.
This is a correctness requirement, not a design choice.

---

## 2. Lifetime Constraints

`CustomizedBasic<'a, CCH>` borrows `&'a CCH` from the context:

```rust
pub struct CustomizedBasic<'a, CCH> {
    pub cch: &'a CCH,       // borrowed — NOT 'static
    upward: Vec<Weight>,
    downward: Vec<Weight>,
    // ...
}
```

**Consequences:**

| Approach | Compatible? | Why |
|---|---|---|
| `ArcSwap<CustomizedBasic>` | No | Requires `T: 'static` |
| `Arc<RwLock<CustomizedBasic>>` | No | Can't share borrowed ref across `thread::spawn` |
| `std::thread::scope` | **Yes** | Scoped threads can borrow `&context` safely |
| Per-worker owned `QueryEngine` | **Yes** | Each worker borrows context independently |

**Design choice:** `std::thread::scope` + per-worker `QueryEngine` instances,
all borrowing the same `CchContext`/`LineGraphCchContext`.

---

## 3. Implementation Plan

### R1: Worker Pool — Multiple Engine Threads

**Impact:** N x throughput (near-linear scaling to CPU cores)
**Memory cost:** ~104 MB per additional worker (workspace buffers)
**Files:** `hanoi-server/src/engine.rs`, `hanoi-server/src/main.rs`

#### Architecture

```
                              std::thread::scope
                              ┌─────────────────────────────────────┐
                              │  Worker 0: QueryEngine (&context)   │
  flume(4096)                 │  Worker 1: QueryEngine (&context)   │
HTTP ──────→ rx.clone()       │  Worker 2: QueryEngine (&context)   │
  ←── oneshot ────────────────│  Worker 3: QueryEngine (&context)   │
                              └─────────────────────────────────────┘
                              context dropped AFTER scope exits
```

Each worker:
1. Owns its own `QueryEngine` with independent workspace buffers
2. Borrows `&context` (read-only CCH topology) — safe via scoped threads
3. Holds a cloned `flume::Receiver` — native MPMC, no Mutex needed

#### Changes to `engine.rs`

Replace `run_normal` / `run_line_graph` with pool variants:

```rust
pub fn run_normal_pool(
    context: &CchContext,
    query_rx: flume::Receiver<QueryMsg>,
    watch_rx: watch::Receiver<Option<Vec<Weight>>>,
    customization_active: &Arc<AtomicUsize>,   // counter (R3)
    engine_alive: &Arc<AtomicBool>,
    rt: &tokio::runtime::Handle,
    num_workers: usize,
) {
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..num_workers)
            .map(|id| {
                let rx = query_rx.clone();        // flume::Receiver is Clone
                let mut watch = watch_rx.clone(); // watch::Receiver is Clone
                scope.spawn(move || {
                    let mut engine = QueryEngine::new(context);
                    worker_loop(id, &mut engine, &rx, &mut watch,
                                customization_active, rt);
                })
            })
            .collect();

        // Scoped threads auto-join when scope exits.
        // If any worker panics, scope propagates the panic.
    });

    engine_alive.store(false, Ordering::Relaxed);
}
```

Same pattern for `run_line_graph_pool` with `LineGraphQueryEngine`.

#### Worker Loop

See R4 for the canonical loop body. Signature:

```rust
fn worker_loop<E: EngineDispatch>(
    id: usize,
    engine: &mut E,
    rx: &flume::Receiver<QueryMsg>,
    watch_rx: &mut watch::Receiver<Option<Vec<Weight>>>,
    customization_active: &Arc<AtomicUsize>,  // counter, not bool (R3)
    rt: &tokio::runtime::Handle,
)
```

Uses 5ms timeout (R4), `AtomicUsize` counter (R3), and `EngineDispatch`
trait for unified normal/line-graph dispatch.

#### `EngineDispatch` Trait (new, in engine.rs)

Unifies normal and line-graph dispatch. Takes owned `QueryMsg` because `reply`
(oneshot::Sender) must be moved out:

```rust
trait EngineDispatch {
    fn dispatch(&mut self, req: QueryRequest, format: Option<&str>,
                colors: bool, alternatives: u32, stretch: f64)
                -> Result<Value, CoordRejection>;
    fn apply_weights(&mut self, weights: &[Weight]);
}

impl<'a> EngineDispatch for QueryEngine<'a> {
    fn dispatch(&mut self, req: QueryRequest, format: Option<&str>,
                colors: bool, alternatives: u32, stretch: f64)
                -> Result<Value, CoordRejection> {
        dispatch_normal(self, req, format, colors, alternatives, stretch)
    }
    fn apply_weights(&mut self, weights: &[Weight]) {
        self.update_weights(weights);
    }
}
// Same impl for LineGraphQueryEngine → dispatch_line_graph
```

Worker dispatches by destructuring `QueryMsg`:

```rust
Ok(Some(qm)) => {
    let resp = engine.dispatch(
        qm.request, qm.format.as_deref(),
        qm.colors, qm.alternatives, qm.stretch,
    );
    let _ = qm.reply.send(resp);  // move reply out
}
```

#### Changes to `main.rs`

```rust
// Before:
let (query_tx, mut query_rx) = mpsc::channel::<QueryMsg>(256);
std::thread::spawn(move || {
    engine::run_normal(&context, &mut query_rx, ...);
});

// After:
let (query_tx, query_rx) = flume::bounded::<QueryMsg>(
    args.query_queue_size.unwrap_or(4096),
);
let num_workers = args.engine_workers
    .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()));
std::thread::spawn(move || {
    engine::run_normal_pool(&context, query_rx, watch_rx, ..., num_workers);
});
```

Add CLI arg:

```rust
/// Number of engine worker threads (default: CPU count)
#[arg(long, default_value = None)]
engine_workers: Option<usize>,
```

#### Audit: R1

| Check | Status | Notes |
|---|---|---|
| Algorithm correctness | Safe | Each worker owns independent workspace buffers |
| Lifetime safety | Safe | `std::thread::scope` guarantees join before context drops |
| No shared mutable state | Safe | Workers share only cloned `flume::Receiver` (lock-free MPMC) |
| Shutdown path | Safe | Scope blocks until all workers exit; `engine_alive` set after |
| Deadlock risk | None | No Mutex — flume uses internal lock-free queue |

---

### R2: Increase Channel Capacity

**Impact:** Burst absorption — prevents backpressure during traffic spikes
**Files:** `hanoi-server/src/main.rs`

```rust
// main.rs line 210
// Before:
let (query_tx, query_rx) = mpsc::channel::<QueryMsg>(256);

// After (flume — bounded MPMC):
let channel_capacity = args.query_queue_size.unwrap_or(4096);
let (query_tx, query_rx) = flume::bounded::<QueryMsg>(channel_capacity);
```

Add CLI arg:

```rust
/// Query queue capacity (default: 4096)
#[arg(long, default_value = None)]
query_queue_size: Option<usize>,
```

At 4096 capacity with 4 workers processing ~4000 QPS, the queue absorbs ~1
second of burst traffic before backpressure kicks in.

#### Audit: R2

| Check | Status |
|---|---|
| Correctness | No logic change — just capacity |
| Memory | ~4096 × size_of(QueryMsg) ≈ negligible |

---

### R3: Coordinated Weight Updates

**Impact:** Correct multi-worker customization without redundant work
**Files:** `hanoi-server/src/engine.rs`

#### The Problem

With N workers each holding a cloned `watch::Receiver`, all N will detect a
weight change and call `context.customize_with(weights)`. Since `customize_with`
uses rayon internally, N simultaneous calls would thrash the global thread pool.

#### Why ArcSwap / Shared CustomizedBasic Won't Work

`CustomizedBasic<'a, CCH>` borrows `&'a CCH` — it is NOT `'static`. Cannot be
wrapped in `Arc`, `ArcSwap`, or sent between threads. Each worker **must**
construct its own `CustomizedBasic` from its own `&context` borrow.

#### Solution: Independent Watch + Natural Staggering

Each worker owns a cloned `watch::Receiver` and checks independently:

```rust
// Each worker, at the top of its loop:
if watch_rx.has_changed().unwrap_or(false) {
    if let Some(w) = watch_rx.borrow_and_update().clone() {
        engine.update_weights(&w);
    }
}
```

Workers naturally stagger because they check at different points in their
loop (between queries). The first worker to enter `customize_with` saturates
rayon; subsequent workers serialize on the rayon pool — no thrashing.

**`customization_active` flag:** Use an `AtomicUsize` counter instead of
`AtomicBool`. Workers increment on enter, decrement on exit. The handler
checks `count > 0`:

```rust
// Shared: Arc<AtomicUsize>
customization_active.fetch_add(1, Ordering::Relaxed);
engine.update_weights(&w);
customization_active.fetch_sub(1, Ordering::Relaxed);
```

This avoids the race where one worker clears the flag while another is still
customizing. `AppState::is_customization_active()` becomes `count > 0`.

#### Consistency Model

During the transition window (~200-500ms × N workers), some workers serve
old weights, others new. This is acceptable:

- Customization is already async from the HTTP handler's perspective (handler
  returns "customization queued" immediately)
- The current single-engine design also has a window where the HTTP response
  is sent before the engine applies weights
- No user-visible correctness issue — routes are valid under either weight set

#### Audit: R3

| Check | Status | Notes |
|---|---|---|
| Correctness | Safe | Each worker gets independent `CustomizedBasic` |
| Rayon contention | Acceptable | Workers stagger naturally; rayon serializes internally |
| Stale queries | Acceptable | Brief window — same as current async model |
| Deadlock risk | None | No barriers or cross-worker locks |
| Flag race | Fixed | AtomicUsize counter instead of AtomicBool |

---

### R4: Reduce Poll Interval (50ms → 5ms)

**Impact:** Faster customization detection, lower tail latency
**Files:** `hanoi-server/src/engine.rs`

#### Why Not `tokio::select!`

Using `select!` on both `watch_rx` and `query_rx` in the same branch is
fragile — `watch` changes are rare vs. high-frequency queries. The simpler
and correct approach: check watch (non-blocking), then recv with timeout.

#### Final Worker Loop (canonical — used by R1)

```rust
loop {
    // 1. Check customization first (non-blocking)
    if watch_rx.has_changed().unwrap_or(false) {
        if let Some(w) = watch_rx.borrow_and_update().clone() {
            customization_active.fetch_add(1, Ordering::Relaxed);
            engine.apply_weights(&w);
            customization_active.fetch_sub(1, Ordering::Relaxed);
        }
    }

    // 2. Compete for one query — flume MPMC, no Mutex needed
    let msg = rt.block_on(async {
        tokio::time::timeout(Duration::from_millis(5), rx.recv_async()).await
    });

    match msg {
        Ok(Ok(qm)) => {
            let resp = engine.dispatch(
                qm.request, qm.format.as_deref(),
                qm.colors, qm.alternatives, qm.stretch,
            );
            let _ = qm.reply.send(resp);
        }
        Ok(Err(_)) => break, // channel closed (all senders dropped) — shutdown
        Err(_) => {}         // timeout — loop back to check customization
    }
}
```

Reduced from 50ms to 5ms. The timeout only gates customization detection —
when queries are flowing, `recv_async()` returns instantly.

> **Note:** `flume::recv_async()` returns `Result<T, RecvError>` (not
> `Option<T>`), so the match arms differ from `tokio::mpsc`: `Ok(Ok(qm))`
> for a message, `Ok(Err(_))` for channel closed.

#### Audit: R4

| Check | Status | Notes |
|---|---|---|
| Latency | Improved | 5ms max customization detection vs 50ms |
| CPU usage | Comparable | Workers block on recv, not spinning |
| Distribution | Fair | flume wakes one waiter per message (FIFO) |
| Starvation | None | No lock — flume's internal queue handles contention |

---

### R5: Efficient Allocator for AlternativeServer Workspace

**Impact:** Eliminates ~208 MB alloc/dealloc overhead per multi_query call
**Files:** `hanoi-server/src/main.rs`, `hanoi-server/Cargo.toml`

#### The Problem

`multi_query` allocates `AlternativeServer::new(customized)` per call — ~208 MB
of workspace buffers (fw/bw distances, parents, t-test vectors). With N workers
under multi-route load, this means repeated 208 MB × N transient allocations.

#### Why We Can't Pre-allocate

`AlternativeServer<'a, C>` borrows `&'a CustomizedBasic`, which changes on
every `update_weights()`. Can't persist the struct across customization
boundaries. And `AlternativeServer` lives in rust_road_router (read-only).

#### Solution: mimalloc Global Allocator

`mimalloc` reuses recently-freed same-size blocks. After the first
`multi_query` call per worker, subsequent alloc/dealloc cycles become
near-instant (thread-local free lists, no OS interaction).

```rust
// main.rs
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
```

```toml
# Cargo.toml
mimalloc = { version = "0.1", default-features = false }
```

#### Audit: R5

| Check | Status | Notes |
|---|---|---|
| rust_road_router untouched | Safe | Only change allocator in hanoi-server |
| Correctness | Safe | No logic changes — drop-in allocator replacement |
| Memory | Improved | Thread-local free lists prevent OS round-trips |
| Build | Safe | mimalloc is widely used, no unsafe in our code |

---

### R6: Backpressure & Load Shedding

**Impact:** Graceful degradation under overload instead of unbounded queueing
**Files:** `hanoi-server/src/main.rs`, `hanoi-server/src/handlers.rs`

#### 6a: Queue Timeout in Handler

```rust
// handlers.rs — handle_query
pub async fn handle_query(...) -> ... {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let msg = QueryMsg { ..., reply: tx };

    // Backpressure: fail fast if channel is full
    match state.query_tx.try_send(msg) {
        Ok(()) => {}
        Err(flume::TrySendError::Full(_)) => {
            return Err((StatusCode::SERVICE_UNAVAILABLE, Json(json!({
                "error": "server overloaded, query queue full"
            }))));
        }
        Err(flume::TrySendError::Disconnected(_)) => {
            return Err((StatusCode::SERVICE_UNAVAILABLE, Json(json!({
                "error": "engine unavailable"
            }))));
        }
    }

    // Response timeout: don't wait forever
    match tokio::time::timeout(Duration::from_secs(5), rx).await {
        Ok(Ok(Ok(resp))) => {
            state.queries_processed.fetch_add(1, Ordering::Relaxed);
            Ok(Json(resp))
        }
        Ok(Ok(Err(rejection))) => { /* coordinate rejection */ }
        _ => Err((StatusCode::GATEWAY_TIMEOUT, Json(json!({
            "error": "query timed out"
        })))),
    }
}
```

Key changes:
- `try_send` instead of `send().await` — returns 503 immediately when queue is
  full instead of blocking the HTTP handler
- 5-second response timeout — prevents handler from waiting forever if engine
  is slow or stuck

#### 6b: Tower Concurrency Limit

```rust
// main.rs — query router
let query_router = Router::new()
    .route("/query", post(handlers::handle_query))
    // ... other routes ...
    .layer(tower::limit::ConcurrencyLimitLayer::new(
        args.max_concurrent_queries.unwrap_or(1000),
    ))
    .layer(TraceLayer::new_for_http())
    .with_state(state.clone());
```

Limits in-flight HTTP handler tasks. When the limit is hit, new requests get
503 immediately — protects Tokio runtime from task exhaustion.

#### 6c: Health Endpoint Queue Depth

```rust
// handlers.rs — handle_health
pub async fn handle_health(State(state): State<AppState>) -> Json<Value> {
    let cap = state.query_tx.capacity().unwrap_or(0);
    let used = state.query_tx.len();
    Json(json!({
        "status": if state.is_engine_alive() { "ok" } else { "degraded" },
        "uptime_secs": state.uptime_secs(),
        "queries_processed": state.total_queries(),
        "customization_active": state.is_customization_active(),
        "queue_capacity": cap,
        "queue_used": used,
        "queue_available": cap.saturating_sub(used),
    }))
}
```

`flume::Sender::capacity()` returns `Option<usize>` (None for unbounded),
`len()` returns current queue depth. Difference = available slots for
health-aware routing.

#### Audit: R6

| Check | Status | Notes |
|---|---|---|
| No data loss | Safe | Rejected queries get explicit 503 with reason |
| Handler blocking | Eliminated | `try_send` is non-blocking |
| Timeout cleanup | Safe | Dropped oneshot Sender signals Err to receiver |

---

## 4. Implementation Order

```
R2: Increase channel capacity               [~5 lines, main.rs]
 ↓
R6: Backpressure + load shedding             [~40 lines, handlers.rs + main.rs]
 ↓
R1+R3+R4: Worker pool with coordinated       [~120 lines, engine.rs + main.rs + state.rs]
           customization and 5ms poll
 ↓
R5: mimalloc global allocator                [3 lines, main.rs + Cargo.toml]
```

R2, R6, R5 are independent. R1 subsumes R3 (watch per worker) and R4 (5ms
poll) — they are part of the worker loop design.

---

## 5. Memory Budget (4 Workers, Hanoi Line Graph)

| Component | Current (1 worker) | After (4 workers) |
|---|---|---|
| CCH topology (shared, read-only) | ~1.4 GB | ~1.4 GB (no change) |
| Server workspace × N | 104 MB | 416 MB |
| AlternativeServer (transient) | 208 MB | 208 MB × concurrent multi_queries |
| CustomizedBasic × N | 200 MB | 800 MB |
| SpatialIndex × N | 40 MB | 40 MB (shared via &context) |
| **Total steady-state** | **~1.7 GB** | **~2.7 GB** |

With mmap (from RAM-Optimized plan), topology drops to ~400 MB file-backed:

| After mmap + 4 workers | ~1.7 GB |
|---|---|

---

## 6. Expected Performance

| Metric | Current (1 worker) | After (4 workers) | After (8 workers) |
|---|---|---|---|
| Single-query QPS | ~1000 | ~4000 | ~8000 |
| Multi-route QPS | ~50-200 | ~200-800 | ~400-1600 |
| p50 latency | ~1 ms | ~1 ms | ~1 ms |
| p99 latency | ~3 ms | ~3 ms | ~5 ms |
| Burst absorption | 256 queued | 4096 queued | 4096 queued |
| Customization stall | ~500ms all | ~500ms per worker (staggered) | ~500ms per worker |
| Max CCU (mixed) | ~500-1000 | ~2000-5000 | ~5000-10000 |

For 10K CCU target: **8 workers on an 8-core machine** is the minimum
recommendation. Query patterns matter — if most users are idle (browsing map,
not actively routing), 4 workers may suffice.

---

## 7. Risk Assessment

| Risk | Severity | Mitigation |
|---|---|---|
| Worker panic takes down scope | High | Catch panics via `std::panic::catch_unwind` in worker; log and restart |
| Rayon contention during parallel customization | Medium | Workers stagger naturally; rayon serializes internally |
| Memory growth with N workers | Medium | Cap workers via CLI arg; budget per deployment |
| Stale weights during transition | Low | Same as current — customization is async; eventual consistency acceptable |
| flume dependency upgrade | Low | Stable crate (~30M downloads), no unsafe in our code, zero transitive deps |

---

## 8. CLI Args Summary

| Arg | Default | Description |
|---|---|---|
| `--engine-workers` | CPU count | Number of engine worker threads |
| `--query-queue-size` | 4096 | Query channel capacity |
| `--max-concurrent-queries` | 1000 | Tower concurrency limit on `/query` |
| `--query-timeout-secs` | 5 | Max wait time for query response |

---

## 9. Files Changed

| File | Changes |
|---|---|
| `hanoi-server/src/engine.rs` | Worker pool functions, `EngineDispatch` trait, worker loop (flume `recv_async`) |
| `hanoi-server/src/main.rs` | CLI args, `flume::bounded` channel, worker pool spawn, mimalloc, concurrency limit |
| `hanoi-server/src/handlers.rs` | `try_send` + timeout in `handle_query`, queue depth in `handle_health` |
| `hanoi-server/src/state.rs` | `query_tx: flume::Sender<QueryMsg>`; change `customization_active` from `AtomicBool` to `AtomicUsize` |
| `hanoi-server/Cargo.toml` | Add `flume` + `mimalloc` dependencies |
| `rust_road_router/*` | **NO CHANGES** |
| `hanoi-core/*` | **NO CHANGES** |
