# Structured Logging with `tracing`

## Overview

Replace all ad-hoc `eprintln!` calls across the CCH-Hanoi workspace with the
`tracing` framework — the industry-standard structured logging/diagnostics
system for Rust's tokio/axum ecosystem.

### Why `tracing`?

| Criteria                | `tracing`                             | `log4rs` (runner-up)              |
| ----------------------- | ------------------------------------- | --------------------------------- |
| Async span propagation  | Native (`.instrument()`)              | None                              |
| Axum/tower integration  | `tower-http::TraceLayer` (built-in)   | Manual                            |
| Structured fields       | `info!(latency_ms = 42, "done")`      | String interpolation only         |
| JSON output             | `fmt().json()` one-liner              | Encoder feature                   |
| Non-blocking file I/O   | `tracing-appender::non_blocking`      | Opt-in `background_rotation`      |
| OpenTelemetry path      | `tracing-opentelemetry`               | Not available                     |
| Per-module levels       | `EnvFilter` (`RUST_LOG=...`)          | YAML config                       |
| Community / maintenance | ~6k stars, tokio-rs maintained        | ~1k stars, community maintained   |

### Crate Stack

```
tracing                     — instrumentation macros (info!, warn!, debug!, #[instrument])
tracing-subscriber          — with "env-filter", "fmt", "json", "ansi" features
tracing-appender            — file output with daily rotation + non-blocking writer
tracing-tree                — hierarchical indented tree output for development
tower-http                  — TraceLayer for automatic per-request HTTP spans
tracing-log                 — bridge for dependencies using the `log` facade
```

### Human-Friendly Output Formats

A critical design goal: log output must be **readable by humans during
development** while remaining machine-parseable in production. The `tracing`
ecosystem offers several output formats, each suited to different contexts.

#### Format Comparison

| Format | Lines/Event | Readability | Concurrent-Safe | Best For |
| --- | --- | --- | --- | --- |
| **Full** (default) | 1 | Good | Yes | General purpose |
| **Compact** (`.compact()`) | 1 | Good | Yes | Narrow terminals |
| **Pretty** (`.pretty()`) | 2–4+ | Excellent | Yes | Quick local debugging |
| **Tree** (`tracing-tree`) | Multi | Excellent | No (interleaves) | Understanding call hierarchy |
| **JSON** (`.json()`) | 1 | Poor (raw) | Yes | Log aggregation / production |

#### Example Output — Same Event Across Formats

**Full** (default — single line, spans shown inline):
```
2026-03-18T12:00:00.123Z  INFO cch::query{from=100 to=500}: hanoi_core::cch: query completed distance_ms=42000 path_len=15
```

**Compact** (shorter lines, span fields appended):
```
2026-03-18T12:00:00.123Z  INFO cch::query: hanoi_core::cch: query completed distance_ms=42000 path_len=15 from=100 to=500
```

**Pretty** (multi-line, colorized, with source location):
```
  2026-03-18T12:00:00.123Z  INFO hanoi_core::cch: query completed, distance_ms: 42000, path_len: 15
    at crates/hanoi-core/src/cch.rs:185 on main
    in hanoi_core::cch::query with from: 100, to: 500
```

**Tree** (`tracing-tree` — indented hierarchy with box-drawing):
```
0ms  INFO hanoi_server starting
├─┐load_and_build graph_dir="/data/hanoi"
│ ├─ 12ms  INFO hanoi_core::cch building CCH, num_nodes: 185432, num_edges: 412876
│ ├─┐customize
│ │ ├─ 45ms  INFO hanoi_core::cch customization complete
│ ├─┘
├─┘
├─┐query from=100, to=500
│ ├─ 0ms  INFO hanoi_core::cch query completed, distance_ms: 42000, path_len: 15
├─┘
```

**JSON** (machine-parseable, one object per line):
```json
{"timestamp":"2026-03-18T12:00:00Z","level":"INFO","target":"hanoi_core::cch","span":{"name":"query","from":100,"to":500},"fields":{"message":"query completed","distance_ms":42000,"path_len":15}}
```

#### Can Multiple Formats Run Simultaneously?

Yes — `tracing-subscriber`'s layer architecture allows stacking multiple
independent layers on one `Registry`. Each layer receives every event. This
is how the file-logging feature works: one human-readable layer writes to
stderr while a separate JSON layer writes to a log file.

However, stacking two stderr formats (e.g., `pretty` + `tree` both to stderr)
would produce **duplicate output** — every event printed twice. The practical
limit is **one format per output destination**. The useful combination is:

```
stderr  →  pretty / tree / compact / full  (human-readable)
file    →  json                             (machine-parseable)
```

This dual-layer setup is built into Phase 5 (file logging).

#### Recommended Strategy

Use the `--log-format` CLI argument to select the stderr presentation format.
Defaults to `pretty` for maximum readability out of the box:

| Value | Format | Use Case |
| --- | --- | --- |
| `pretty` (default) | Multi-line with source locations | Day-to-day development and operation |
| `full` | Single-line with inline spans | Compact single-line logs |
| `compact` | Abbreviated single-line | CI output, narrow terminals |
| `tree` | Indented hierarchy | Understanding span nesting during development |
| `json` | Newline-delimited JSON | Production log aggregation (ELK, Loki, Datadog) |

No recompilation needed — just pass the flag:

```bash
./hanoi_server --graph-dir ./data                              # pretty (default)
./hanoi_server --graph-dir ./data --log-format tree            # hierarchical tree
./hanoi_server --graph-dir ./data --log-format json            # JSON for production
RUST_LOG=debug ./hanoi_server --graph-dir ./data               # pretty + verbose
```

---

## Phase 1: Foundation — Add `tracing` and Initialize Subscribers

### 1.1 Add Dependencies

**hanoi-core/Cargo.toml** — add `tracing` (macros only, no subscriber):

```toml
[dependencies]
tracing = "0.1"
```

**hanoi-server/Cargo.toml** — add tracing crates (tower-http already has
`"trace"` feature enabled):

```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json", "ansi"] }
tracing-appender = "0.2"
tracing-tree = "0.4"
# tower-http already present with features = ["cors", "compression-gzip", "decompression-gzip", "trace"]
# No changes needed to tower-http.
```

**hanoi-cli/Cargo.toml**:

```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

**hanoi-gateway/Cargo.toml** — add tracing crates + tower-http (new dependency
for the gateway, currently not present):

```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tower-http = { version = "0.6", features = ["trace"] }    # NEW dependency for gateway
```

**hanoi-tools/Cargo.toml**:

```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

### 1.2 Add `--log-format` CLI Argument

All binaries already use `clap::Parser`. Add a shared `LogFormat` enum and
`--log-format` flag to each `Args` / `Cli` struct.

**Shared enum** (can live in `hanoi-core/src/lib.rs` or be duplicated in each
binary — duplicating is simpler since it's 10 lines and avoids coupling core to
`clap`):

```rust
use clap::ValueEnum;

#[derive(Clone, Default, ValueEnum)]
enum LogFormat {
    /// Multi-line, colorized, with source locations (most readable)
    #[default]
    Pretty,
    /// Single-line with inline span context
    Full,
    /// Abbreviated single-line
    Compact,
    /// Indented tree hierarchy (hanoi-server only)
    Tree,
    /// Newline-delimited JSON for log aggregation
    Json,
}
```

Add to each `Args` struct:

```rust
/// Log output format
#[arg(long, value_enum, default_value_t = LogFormat::Pretty)]
log_format: LogFormat,
```

### 1.3 Initialize Subscriber in Each Binary

**hanoi-server/src/main.rs** — format-switchable subscriber via `--log-format`:

```rust
use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use tracing_tree::HierarchicalLayer;

fn init_tracing(log_format: &LogFormat) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=debug"));

    // Each match arm calls .init() separately because different formats
    // produce different generic types that cannot be unified without boxing.
    match log_format {
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json())
                .init();
        }
        LogFormat::Pretty => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().pretty())
                .init();
        }
        LogFormat::Compact => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().compact().with_target(true))
                .init();
        }
        LogFormat::Tree => {
            tracing_subscriber::registry()
                .with(filter)
                .with(
                    HierarchicalLayer::new(2)
                        .with_targets(true)
                        .with_indent_lines(true)
                        .with_deferred_spans(true)
                        .with_span_retrace(true),
                )
                .init();
        }
        LogFormat::Full => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_target(true).with_thread_ids(true))
                .init();
        }
    }
}
```

Called early in `main()`:

```rust
let args = Args::parse();
init_tracing(&args.log_format);
```

The `RUST_LOG` env var still controls verbosity:

```bash
# Examples:
RUST_LOG=info                                    # default
RUST_LOG=hanoi_core::cch=debug,hanoi_server=info # debug CCH, info server
RUST_LOG=trace                                   # everything (very verbose)

./hanoi_server --graph-dir ./data                              # pretty (default)
./hanoi_server --graph-dir ./data --log-format tree            # hierarchical tree
RUST_LOG=debug ./hanoi_server --graph-dir ./data --log-format compact  # verbose + compact
```

**hanoi-cli/src/main.rs**, **hanoi-gateway/src/main.rs**,
**hanoi-tools/src/bin/generate_line_graph.rs** — same pattern but without tree
support:

```rust
use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

fn init_tracing(log_format: &LogFormat) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    match log_format {
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json())
                .init();
        }
        LogFormat::Pretty => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().pretty())
                .init();
        }
        LogFormat::Compact => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().compact())
                .init();
        }
        LogFormat::Full | LogFormat::Tree => {
            // Tree not available in CLI tools (no tracing-tree dep); fall back to full
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_target(true))
                .init();
        }
    }
}
```

Note: `tree` format falls back to `full` in CLI tools since they don't depend
on `tracing-tree`. These are short-lived processes where hierarchical output
adds less value.

---

## Phase 2: Replace `eprintln!` with Structured `tracing` Events

Migrate all 15+ `eprintln!` calls to structured tracing events. Each call maps
to an appropriate level:

### 2.1 hanoi-core/src/cch.rs

| Current                                                  | Replacement                                                                           |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| `eprintln!("Building CCH: {} nodes, {} edges", ...)`     | `tracing::info!(num_nodes = graph.num_nodes(), num_edges = graph.num_edges(), "building CCH")` |

### 2.2 hanoi-core/src/line_graph.rs

| Current                                                                  | Replacement                                                                                               |
| ------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------- |
| `eprintln!("Building DirectedCCH for line graph: {} nodes, {} edges")` | `tracing::info!(num_nodes, num_edges, "building DirectedCCH for line graph")`                             |

### 2.3 hanoi-server/src/engine.rs

| Current                                              | Replacement                                                                                  |
| ---------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `eprintln!("Re-customizing with {} weights...")`     | `tracing::info!(num_weights = weights.len(), "re-customizing")`                              |
| `eprintln!("Customization complete.")`               | `tracing::info!("customization complete")`                                                   |
| `eprintln!("Re-customizing line graph...")`          | `tracing::info!(num_weights = weights.len(), "re-customizing line graph")`                   |
| `eprintln!("Line graph customization complete.")`    | `tracing::info!("line graph customization complete")`                                        |

### 2.4 hanoi-server/src/main.rs

| Current                                                       | Replacement                                                                                   |
| ------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `eprintln!("Server ready: query=..., customize=..., mode=")` | `tracing::info!(%query_addr, %customize_addr, mode, "server ready")`                          |

### 2.5 hanoi-cli/src/main.rs

| Current                                            | Replacement                                                                                 |
| -------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `eprintln!("Loading graph from {:?}...")`          | `tracing::info!(?graph_dir, "loading graph")`                                               |
| `eprintln!("CCH built in {:.2?}")`                 | `tracing::info!(elapsed = ?t0.elapsed(), "CCH built")`                                      |
| `eprintln!("Initial customization + spatial...")`  | `tracing::info!(elapsed = ?t1.elapsed(), "initial customization + spatial index")`           |
| `eprintln!("Error: specify either...")`            | `tracing::error!("specify either --from-node/--to-node or coordinate flags")`                |
| `eprintln!("No path found.")`                      | `tracing::warn!("no path found")`                                                           |

### 2.6 hanoi-gateway/src/main.rs

| Current                                                       | Replacement                                                                            |
| ------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `eprintln!("Gateway ready on {}: normal=..., line_graph=")` | `tracing::info!(%addr, %normal_backend, %line_graph_backend, "gateway ready")`         |

### 2.7 hanoi-tools/src/bin/generate_line_graph.rs

| Current                                                                    | Replacement                                                                                    |
| -------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `eprintln!("Original graph: {} nodes, {} arcs, {} forbidden turns")`       | `tracing::info!(num_nodes, num_arcs, forbidden_turns, "original graph loaded")`                |
| `eprintln!("Line graph: {} nodes, {} arcs (avg degree: {:.2})")`          | `tracing::info!(num_nodes, num_arcs, avg_degree, "line graph constructed")`                    |
| `eprintln!("Output: {}")`                                                 | `tracing::info!(?output_dir, "line graph written")`                                            |

---

## Phase 3: Add Strategic Spans for Performance Visibility

Spans are the key differentiator of `tracing` — they capture **duration** and
**context** around operations. Use `#[instrument]` for function-level spans or
manual `tracing::info_span!()` for finer control.

### 3.1 CCH Build & Customization Spans (hanoi-core)

```rust
// cch.rs — load_and_build (signature: graph_dir: &Path, perm_path: &Path)
#[tracing::instrument(skip_all, fields(graph_dir = %graph_dir.display()))]
pub fn load_and_build(graph_dir: &Path, perm_path: &Path) -> std::io::Result<Self> { ... }

// line_graph.rs — load_and_build (3 params: line_graph_dir, original_graph_dir, perm_path)
#[tracing::instrument(skip_all, fields(
    line_graph_dir = %line_graph_dir.display(),
    original_graph_dir = %original_graph_dir.display()
))]
pub fn load_and_build(
    line_graph_dir: &Path,
    original_graph_dir: &Path,
    perm_path: &Path,
) -> std::io::Result<Self> { ... }

// cch.rs — customize / customize_with
#[tracing::instrument(skip_all)]
pub fn customize(&self) -> CustomizedBasic<'_, CCH> { ... }

#[tracing::instrument(skip_all, fields(num_weights = weights.len()))]
pub fn customize_with(&self, weights: &[Weight]) -> CustomizedBasic<'_, CCH> { ... }
```

### 3.2 Query Spans (hanoi-core)

```rust
// cch.rs — QueryEngine::query
#[tracing::instrument(skip(self), fields(from, to))]
pub fn query(&mut self, from: NodeId, to: NodeId) -> Option<QueryAnswer> { ... }

// cch.rs — QueryEngine::query_coords
#[tracing::instrument(skip(self), fields(
    from_lat = from.0, from_lng = from.1,
    to_lat = to.0, to_lng = to.1
))]
pub fn query_coords(&mut self, from: (f32, f32), to: (f32, f32)) -> Option<QueryAnswer> { ... }
```

### 3.3 Spatial Indexing Spans (hanoi-core)

```rust
// spatial.rs — SpatialIndex::build
#[tracing::instrument(skip_all, fields(num_nodes))]
pub fn build(...) -> Self { ... }

// spatial.rs — snap_to_edge (note: returns SnapResult, not Option<SnapResult>)
#[tracing::instrument(skip(self), fields(lat, lng))]
pub fn snap_to_edge(&self, lat: f32, lng: f32) -> SnapResult { ... }
```

### 3.4 HTTP Request Spans (hanoi-server, hanoi-gateway)

Add `tower-http::trace::TraceLayer` to Axum routers. This automatically creates
a span per request with method, URI, version, and emits events for response
status + latency.

**hanoi-server** has two separate routers — add `TraceLayer` to both:

```rust
use tower_http::trace::TraceLayer;

// Query router (port 8080)
let query_router = Router::new()
    .route("/query", post(handlers::handle_query))
    .route("/info", get(handlers::handle_info))
    .layer(TraceLayer::new_for_http())   // <-- add
    .with_state(state.clone());

// Customize router (port 9080)
let customize_router = Router::new()
    .route("/customize", post(handlers::handle_customize))
    .layer(TraceLayer::new_for_http())   // <-- add
    .layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024))
    .layer(RequestDecompressionLayer::new())
    .with_state(state.clone());
```

**hanoi-gateway** has one router — add `TraceLayer`:

```rust
let router = Router::new()
    .route("/query", post(proxy::handle_query))
    .route("/info", get(proxy::handle_info))
    .layer(TraceLayer::new_for_http())   // <-- add
    .with_state(state);
```

### 3.5 Engine Loop Spans (hanoi-server)

```rust
// engine.rs — wrap the customization operation
let _span = tracing::info_span!("customization", num_weights = weights.len()).entered();
engine.update_weights(&weights);
// span drops here, recording duration
```

---

## Phase 4: Add Structured Fields to Query Responses

Enrich query dispatch with result fields for observability:

```rust
fn dispatch_normal(engine: &mut QueryEngine<'_>, req: QueryRequest) -> Value {
    let format = req.format.clone();
    let answer = /* ... existing dispatch ... */;

    // Structured event with query result metadata
    match &answer {
        Some(a) => tracing::info!(
            distance_ms = a.distance_ms,
            distance_m = a.distance_m,
            path_len = a.path.len(),
            "query completed"
        ),
        None => tracing::info!("query returned no path"),
    }

    format_response(answer, format.as_deref())
}
```

### 4.1 Add Logging to handle_customize (hanoi-server/src/handlers.rs)

The `/customize` endpoint receives raw weight vectors — a significant
operational event that should be logged:

```rust
pub async fn handle_customize(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<CustomizeResponse>, (StatusCode, Json<CustomizeResponse>)> {
    tracing::info!(body_bytes = body.len(), expected_edges = state.num_edges, "customize request received");
    // ... existing validation ...
    tracing::info!("customization weights accepted, queued for engine thread");
    // ...
}
```

### 4.2 Add Debug Logging to GraphData::load (hanoi-core/src/graph.rs)

```rust
pub fn load(graph_dir: &Path) -> std::io::Result<Self> {
    tracing::debug!(?graph_dir, "loading graph data from disk");
    // ... existing loading ...
    tracing::debug!(num_nodes = first_out.len() - 1, num_edges = head.len(), "graph data loaded");
    // ...
}
```

---

## Phase 5: Optional File Logging

For production deployments that need persistent log files alongside stderr.
The file layer always uses JSON (machine-parseable), while stderr uses
whatever `--log-format` is set to (human-readable `pretty` by default).

Add a `--log-dir <PATH>` flag to the `Args` struct:

```rust
/// Directory for persistent log files (daily rotation, JSON format).
/// Omit to log to stderr only.
#[arg(long)]
log_dir: Option<PathBuf>,
```

Update `init_tracing` to accept both arguments:

```rust
use tracing_appender::rolling;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use tracing_tree::HierarchicalLayer;

/// Initialize tracing with format selection and optional file output.
/// Returns an optional WorkerGuard that MUST be held for the lifetime
/// of the program — dropping it flushes and closes the non-blocking
/// file writer.
fn init_tracing(log_format: &LogFormat, log_dir: Option<&Path>) -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=debug"));

    // File layer: always JSON, no ANSI colors
    let (file_layer, guard) = if let Some(dir) = log_dir {
        let file_appender = rolling::daily(dir, "hanoi-server.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        let layer = fmt::layer()
            .with_writer(non_blocking)
            .with_ansi(false)
            .json();
        (Some(layer), Some(guard))
    } else {
        (None, None)
    };

    // Stderr layer: format depends on --log-format
    match log_format {
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json())
                .with(file_layer)
                .init();
        }
        LogFormat::Pretty => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().pretty())
                .with(file_layer)
                .init();
        }
        LogFormat::Compact => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().compact().with_target(true))
                .with(file_layer)
                .init();
        }
        LogFormat::Tree => {
            tracing_subscriber::registry()
                .with(filter)
                .with(
                    HierarchicalLayer::new(2)
                        .with_targets(true)
                        .with_indent_lines(true)
                        .with_deferred_spans(true)
                        .with_span_retrace(true),
                )
                .with(file_layer)
                .init();
        }
        LogFormat::Full => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_target(true).with_thread_ids(true))
                .with(file_layer)
                .init();
        }
    }

    guard
}
```

Called in `main()` — the guard **must** be stored to keep the file writer alive:

```rust
let args = Args::parse();
let _guard = init_tracing(&args.log_format, args.log_dir.as_deref());
```

Usage:

```bash
# Stderr only (pretty by default), no file logging
./hanoi_server --graph-dir ./data

# Stderr (tree) + JSON file logging for aggregation
./hanoi_server --graph-dir ./data --log-format tree --log-dir /var/log/hanoi

# Stderr (compact) + JSON file logging
./hanoi_server --graph-dir ./data --log-format compact --log-dir /var/log/hanoi

# Full JSON everywhere (both stderr and file)
./hanoi_server --graph-dir ./data --log-format json --log-dir /var/log/hanoi
```

---

## Implementation Order & File Change Summary

| Phase | Files Modified                                          | Effort |
| ----- | ------------------------------------------------------- | ------ |
| 1     | 5 Cargo.toml + 4 main.rs (subscriber init + format selection) | Medium |
| 2     | cch.rs, line_graph.rs, engine.rs, main.rs (×3), generate_line_graph.rs | Small  |
| 3     | cch.rs, spatial.rs, line_graph.rs, engine.rs, main.rs (×2) | Medium |
| 4     | engine.rs, handlers.rs, graph.rs                         | Small  |
| 5     | hanoi-server/main.rs (merge init_tracing with file layer + format selection) | Medium |

### Log Level Guidelines

| Level   | Use For                                                          |
| ------- | ---------------------------------------------------------------- |
| `error` | Unrecoverable failures (channel closed, invalid graph data)      |
| `warn`  | Recoverable issues (no path found, bad request format)           |
| `info`  | Lifecycle events (startup, ready, customization, query complete) |
| `debug` | Detailed operation data (snap candidates, path lengths, timing)  |
| `trace` | Hot-path internals (CCH traversal steps — use sparingly)         |

### Runtime Control

No recompilation needed to change log levels or format:

| Control | Mechanism | Default |
| --- | --- | --- |
| Verbosity | `RUST_LOG` env var | `info,tower_http=debug` (server), `info` (others) |
| Stderr format | `--log-format` CLI arg | `pretty` |
| File logging | `--log-dir` CLI arg | disabled (stderr only) |

```bash
# Default (pretty format, info level, stderr only)
./hanoi_server --graph-dir ./data

# Debug spatial snapping with tree view
RUST_LOG=hanoi_core::spatial=debug ./hanoi_server --graph-dir ./data --log-format tree

# Compact output for CI or narrow terminals
./hanoi_server --graph-dir ./data --log-format compact

# Silence tower-http request logs
RUST_LOG=info,tower_http=warn ./hanoi_server --graph-dir ./data

# Full trace for development
RUST_LOG=trace ./hanoi_server --graph-dir ./data

# JSON for production log aggregation (stderr + file)
RUST_LOG=info ./hanoi_server --graph-dir ./data --log-format json --log-dir /var/log/hanoi

# Pretty stderr + JSON file logging (best of both worlds)
./hanoi_server --graph-dir ./data --log-dir /var/log/hanoi
```
