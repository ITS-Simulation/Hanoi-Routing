# Performance Benchmarking Module

## Overview

Design a **modular, plug-and-play** benchmarking and performance analysis system
for the CCH-Hanoi routing server. The module must:

1. Benchmark both **core CCH logic** (build, customize, query) and **end-to-end
   server** performance (HTTP latency, throughput)
2. Be **completely modular** — enable/disable without breaking any part of the
   system
3. Support **result analysis and evaluation** with statistical reporting

### Design Principles

- **Zero coupling**: The benchmark module depends on the application; the
  application never depends on the benchmark module
- **Feature-gated**: Optional Cargo feature flags control compile-time inclusion
- **Separate crate**: Lives in its own crate (`hanoi-bench`) within the workspace
- **No runtime overhead**: When disabled, zero impact on production builds
- **Reproducible**: Deterministic query sets, configurable warm-up and iteration
  counts

---

## Architecture

```
CCH-Hanoi/
├── crates/
│   ├── hanoi-core/          ← Pure library, unchanged
│   ├── hanoi-server/        ← Unchanged (benchmarked externally via HTTP)
│   ├── hanoi-bench/         ← NEW: benchmark + analysis crate
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs       ← Benchmark framework types + utilities
│   │   │   ├── core.rs      ← Core CCH benchmarks (build, customize, query)
│   │   │   ├── server.rs    ← HTTP server benchmarks (latency, throughput)
│   │   │   ├── spatial.rs   ← Spatial indexing benchmarks (snap_to_edge)
│   │   │   ├── report.rs    ← Statistical analysis + report generation
│   │   │   └── dataset.rs   ← Query dataset generation + loading
│   │   ├── benches/
│   │   │   ├── cch_bench.rs         ← Criterion benchmarks for core CCH
│   │   │   └── spatial_bench.rs     ← Criterion benchmarks for spatial ops
│   │   └── src/bin/
│   │       ├── bench_core.rs        ← Standalone core benchmark runner
│   │       ├── bench_server.rs      ← Standalone server benchmark runner
│   │       └── bench_report.rs      ← Analysis + report from saved results
```

### Dependency Graph (One-Way Only)

```
hanoi-bench ──depends-on──► hanoi-core    (for in-process CCH benchmarks)
hanoi-bench ──depends-on──► reqwest       (for HTTP server benchmarks)
hanoi-bench ──depends-on──► criterion     (for micro-benchmarks)

hanoi-core  ──────────────► (no dependency on hanoi-bench)
hanoi-server ─────────────► (no dependency on hanoi-bench)
```

This ensures removing `hanoi-bench` from the workspace has **zero impact** on
the rest of the system.

---

## Phase 1: Crate Setup & Framework

### 1.1 Create `hanoi-bench` Crate

**CCH-Hanoi/Cargo.toml** — no changes needed. The workspace uses a glob pattern
`members = ["crates/*"]`, so any crate placed under `crates/` is
auto-discovered. Simply creating `crates/hanoi-bench/` is sufficient.

**crates/hanoi-bench/Cargo.toml**:

```toml
[package]
name = "hanoi-bench"
version = "0.1.0"
edition = "2024"

[dependencies]
hanoi-core = { path = "../hanoi-core" }
rust_road_router = { path = "../../../rust_road_router/engine" }
reqwest = { version = "0.12", features = ["json"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
rand = "0.9"
statrs = "0.18"            # Statistical functions (percentiles, std dev)
csv = "1"                  # CSV report output
chrono = "0.4"             # Timestamps for reports
futures = "0.3"            # join_all for concurrent benchmark tasks

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "cch_bench"
harness = false

[[bench]]
name = "spatial_bench"
harness = false

[[bin]]
name = "bench_core"
path = "src/bin/bench_core.rs"

[[bin]]
name = "bench_server"
path = "src/bin/bench_server.rs"

[[bin]]
name = "bench_report"
path = "src/bin/bench_report.rs"
```

### 1.2 Framework Types (`lib.rs`)

```rust
/// A single timing measurement.
pub struct Measurement {
    pub label: String,
    pub duration: std::time::Duration,
    pub metadata: serde_json::Value,  // flexible key-value pairs
}

/// Collection of measurements for a benchmark run.
pub struct BenchmarkRun {
    pub name: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub graph_dir: String,
    pub measurements: Vec<Measurement>,
    pub config: BenchmarkConfig,
}

/// Benchmark configuration.
pub struct BenchmarkConfig {
    pub warmup_iterations: usize,
    pub measured_iterations: usize,
    pub query_count: usize,          // number of queries per iteration
    pub seed: u64,                   // RNG seed for reproducibility
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            warmup_iterations: 3,
            measured_iterations: 10,
            query_count: 1000,
            seed: 42,
        }
    }
}
```

---

## Phase 2: Query Dataset Generation (`dataset.rs`)

Generate reproducible query sets from graph metadata:

```rust
/// A query pair (source, destination) with optional coordinates.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct QueryPair {
    pub from_node: u32,
    pub to_node: u32,
    pub from_coords: Option<(f32, f32)>,
    pub to_coords: Option<(f32, f32)>,
}

/// Generate random query pairs from graph metadata.
pub fn generate_random_queries(
    num_nodes: u32,
    lat: &[f32],
    lng: &[f32],
    count: usize,
    seed: u64,
) -> Vec<QueryPair> { ... }

/// Load query pairs from a JSON file (for reproducible runs).
pub fn load_queries(path: &Path) -> Vec<QueryPair> { ... }

/// Save query pairs to a JSON file.
pub fn save_queries(queries: &[QueryPair], path: &Path) { ... }
```

Query datasets can be saved/loaded for reproducibility across runs and machines.

---

## Phase 3: Core CCH Benchmarks (`core.rs`)

In-process benchmarks that directly use `hanoi-core` APIs. No HTTP overhead.

### 3.1 Benchmark Categories

| Benchmark           | What It Measures                                      | Key Metric                  |
| ------------------- | ----------------------------------------------------- | --------------------------- |
| `bench_cch_build`   | `CchContext::load_and_build()` — Phase 1 contraction  | Wall-clock time (seconds)   |
| `bench_customize`   | `customize()` — Phase 2 weight application             | Wall-clock time (ms)        |
| `bench_query`       | `QueryEngine::query()` — Phase 3 shortest path         | Latency per query (μs)      |
| `bench_query_coords`| `QueryEngine::query_coords()` — snap + query           | Latency per query (μs)      |
| `bench_kd_build`    | `SpatialIndex::build()` — KD-tree construction          | Wall-clock time (ms)        |
| `bench_snap`        | `SpatialIndex::snap_to_edge()` — point-to-edge snap    | Latency per snap (μs)       |

### 3.2 Implementation Pattern

```rust
pub fn bench_query(
    engine: &mut QueryEngine<'_>,
    queries: &[QueryPair],
    config: &BenchmarkConfig,
) -> Vec<Measurement> {
    // Warmup
    for _ in 0..config.warmup_iterations {
        for q in queries.iter().take(config.query_count) {
            let _ = engine.query(q.from_node, q.to_node);
        }
    }

    // Measured iterations
    let mut measurements = Vec::new();
    for i in 0..config.measured_iterations {
        let start = std::time::Instant::now();
        let mut found = 0usize;
        for q in queries.iter().take(config.query_count) {
            if engine.query(q.from_node, q.to_node).is_some() {
                found += 1;
            }
        }
        let elapsed = start.elapsed();
        measurements.push(Measurement {
            label: format!("query_iter_{}", i),
            duration: elapsed,
            metadata: serde_json::json!({
                "queries": config.query_count,
                "found": found,
                "avg_us": elapsed.as_micros() as f64 / config.query_count as f64,
            }),
        });
    }
    measurements
}
```

### 3.3 Criterion Micro-Benchmarks (`benches/cch_bench.rs`)

For statistically rigorous micro-benchmarks with automatic outlier detection:

```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn cch_query_benchmark(c: &mut Criterion) {
    let context = CchContext::load_and_build(&graph_dir);
    let mut engine = QueryEngine::new(&context);
    let queries = generate_random_queries(context.num_nodes(), &lat, &lng, 100, 42);

    let mut group = c.benchmark_group("cch_query");
    group.bench_function("node_id_query", |b| {
        let mut i = 0;
        b.iter(|| {
            let q = &queries[i % queries.len()];
            engine.query(q.from_node, q.to_node);
            i += 1;
        });
    });
    group.bench_function("coord_query", |b| {
        let mut i = 0;
        b.iter(|| {
            let q = &queries[i % queries.len()];
            engine.query_coords(q.from_coords.unwrap(), q.to_coords.unwrap());
            i += 1;
        });
    });
    group.finish();
}

criterion_group!(benches, cch_query_benchmark);
criterion_main!(benches);
```

---

## Phase 4: Server Benchmarks (`server.rs`)

End-to-end HTTP benchmarks against a running `hanoi_server` instance.

### 4.1 Benchmark Categories

| Benchmark                | What It Measures                                     | Key Metric                  |
| ------------------------ | ---------------------------------------------------- | --------------------------- |
| `bench_http_query`       | POST /query round-trip latency (single client)        | p50/p95/p99 latency (ms)   |
| `bench_http_concurrent`  | POST /query under concurrent load (N clients)         | Throughput (queries/sec)    |
| `bench_http_info`        | GET /info round-trip latency                          | p50/p95 latency (ms)       |
| `bench_customize_upload` | POST /customize weight upload + application           | Wall-clock time (seconds)   |
| `bench_query_after_cust` | Query latency immediately after customization          | p50/p95 latency (ms)       |

### 4.2 Implementation Pattern

```rust
pub async fn bench_http_query(
    base_url: &str,
    queries: &[QueryPair],
    config: &BenchmarkConfig,
) -> Vec<Measurement> {
    let client = reqwest::Client::new();
    let mut measurements = Vec::new();

    // Warmup
    for q in queries.iter().take(config.warmup_iterations) {
        let _ = send_query(&client, base_url, q).await;
    }

    // Measured
    for (i, q) in queries.iter().take(config.query_count).enumerate() {
        let start = std::time::Instant::now();
        let resp = send_query(&client, base_url, q).await;
        let elapsed = start.elapsed();

        measurements.push(Measurement {
            label: format!("http_query_{}", i),
            duration: elapsed,
            metadata: serde_json::json!({
                "status": resp.is_ok(),
                "has_path": resp.ok().and_then(|r| r.get("distance_ms").and_then(|v| v.as_u64())).is_some(),
            }),
        });
    }
    measurements
}

/// Concurrent load test with N parallel clients.
pub async fn bench_http_concurrent(
    base_url: &str,
    queries: &[QueryPair],
    concurrency: usize,
    config: &BenchmarkConfig,
) -> Vec<Measurement> {
    let client = reqwest::Client::new();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));

    let start = std::time::Instant::now();
    let mut handles = Vec::new();

    for q in queries.iter().take(config.query_count) {
        let client = client.clone();
        let sem = semaphore.clone();
        let url = base_url.to_string();
        let query = q.clone();

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            let t = std::time::Instant::now();
            let _ = send_query(&client, &url, &query).await;
            t.elapsed()
        }));
    }

    let latencies: Vec<_> = futures::future::join_all(handles)
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();

    let total = start.elapsed();
    // ... compute statistics from latencies ...
}
```

### 4.3 CLI Runner (`bin/bench_server.rs`)

```rust
/// Server benchmark runner.
///
/// Expects a running hanoi_server instance.
///
/// Usage:
///   bench_server --url http://localhost:8080 --queries 1000 --concurrency 4
///   bench_server --url http://localhost:8080 --query-file queries.json --output results.json
#[derive(Parser)]
struct Args {
    /// Base URL of the running server
    #[arg(long, default_value = "http://localhost:8080")]
    url: String,

    /// Number of queries to run
    #[arg(long, default_value_t = 1000)]
    queries: usize,

    /// Concurrency level for load tests
    #[arg(long, default_value_t = 1)]
    concurrency: usize,

    /// Path to query dataset JSON file. If absent, the server benchmarks
    /// will generate random coordinate-based queries by sampling lat/lng
    /// within the Hanoi bounding box (use --graph-dir to generate node-ID
    /// queries from graph files, then save with bench_core --save-queries).
    #[arg(long)]
    query_file: Option<PathBuf>,

    /// Path to graph directory (for generating node-ID-based queries when
    /// no query file is provided). Not required if --query-file is set.
    #[arg(long)]
    graph_dir: Option<PathBuf>,

    /// Output results file (JSON)
    #[arg(long, default_value = "bench_results.json")]
    output: PathBuf,

    /// RNG seed
    #[arg(long, default_value_t = 42)]
    seed: u64,
}
```

---

## Phase 5: Statistical Analysis & Reporting (`report.rs`)

### 5.1 Statistics Computed

For each benchmark category, compute:

| Statistic        | Description                                           |
| ---------------- | ----------------------------------------------------- |
| `min`            | Minimum latency                                       |
| `max`            | Maximum latency                                       |
| `mean`           | Arithmetic mean                                       |
| `median` (p50)   | 50th percentile                                       |
| `p95`            | 95th percentile                                       |
| `p99`            | 99th percentile                                       |
| `std_dev`        | Standard deviation                                    |
| `throughput`     | Queries per second                                    |
| `success_rate`   | Fraction of queries that returned a path              |

### 5.2 Report Formats

```rust
pub enum ReportFormat {
    /// Human-readable table to stdout
    Table,
    /// Machine-readable JSON (for CI/regression tracking)
    Json,
    /// CSV (for spreadsheet analysis)
    Csv,
}

pub fn generate_report(
    runs: &[BenchmarkRun],
    format: ReportFormat,
    output: &mut dyn Write,
) { ... }
```

### 5.3 Example Table Output

```
┌─────────────────────┬────────┬────────┬────────┬────────┬────────┬────────┐
│ Benchmark           │    Min │   Mean │    p50 │    p95 │    p99 │    Max │
├─────────────────────┼────────┼────────┼────────┼────────┼────────┼────────┤
│ cch_build           │  2.31s │  2.45s │  2.43s │  2.58s │  2.61s │  2.63s │
│ customize           │  145ms │  152ms │  150ms │  162ms │  168ms │  170ms │
│ query (node_id)     │  12μs  │  18μs  │  16μs  │  28μs  │  45μs  │  82μs  │
│ query (coords)      │  35μs  │  48μs  │  44μs  │  72μs  │  95μs  │ 120μs  │
│ snap_to_edge        │  8μs   │  12μs  │  11μs  │  18μs  │  25μs  │  30μs  │
│ http_query (1 cli)  │  0.8ms │  1.2ms │  1.1ms │  1.8ms │  2.5ms │  3.2ms │
│ http_query (4 cli)  │  1.2ms │  2.1ms │  1.9ms │  3.5ms │  5.0ms │  6.8ms │
│ kd_tree_build       │  85ms  │  92ms  │  90ms  │  98ms  │ 102ms  │ 105ms  │
└─────────────────────┴────────┴────────┴────────┴────────┴────────┴────────┘
Throughput: 850 queries/sec (4 concurrent clients)
Success rate: 94.2% (942/1000 queries found a path)
```

### 5.4 JSON Output (for CI Regression Tracking)

```json
{
  "timestamp": "2026-03-18T12:00:00Z",
  "graph_dir": "./data/hanoi",
  "config": { "warmup": 3, "iterations": 10, "queries": 1000, "seed": 42 },
  "results": {
    "query_node_id": {
      "min_us": 12, "mean_us": 18, "p50_us": 16, "p95_us": 28, "p99_us": 45,
      "max_us": 82, "std_dev_us": 8.3, "throughput_qps": 55555
    },
    "query_coords": { ... },
    "http_query": { ... }
  }
}
```

---

## Phase 6: Comparison & Regression Detection

### 6.1 Compare Two Runs

The `bench_report` binary can compare two result files:

```bash
bench_report --baseline results_v1.json --current results_v2.json
```

Output:

```
┌─────────────────────┬──────────┬──────────┬──────────┐
│ Benchmark           │ Baseline │  Current │   Change │
├─────────────────────┼──────────┼──────────┼──────────┤
│ query (node_id) p50 │    16μs  │    14μs  │  -12.5%  │
│ query (coords)  p50 │    44μs  │    46μs  │   +4.5%  │
│ http_query      p50 │   1.1ms  │   1.0ms  │   -9.1%  │
│ customize           │   150ms  │   148ms  │   -1.3%  │
└─────────────────────┴──────────┴──────────┴──────────┘
```

### 6.2 Regression Threshold

Configurable regression threshold (default: 10% slowdown):

```bash
bench_report --baseline old.json --current new.json --threshold 10
# Exit code 1 if any benchmark regressed by >10%
```

This enables CI integration:

```yaml
# In CI pipeline
- run: cargo run -p hanoi-bench --bin bench_core -- --graph-dir ./data --output current.json
- run: cargo run -p hanoi-bench --bin bench_report -- --baseline baseline.json --current current.json --threshold 10
```

---

## Modularity Guarantees

### How to Enable

```bash
# Add to workspace (already a member)
# Build benchmarks:
cargo build --release -p hanoi-bench

# Run core benchmarks:
cargo run --release -p hanoi-bench --bin bench_core -- --graph-dir ./data

# Run Criterion micro-benchmarks:
cargo bench -p hanoi-bench

# Run server benchmarks (requires running server):
cargo run --release -p hanoi-bench --bin bench_server -- --url http://localhost:8080
```

### How to Disable

**Option A — Exclude from workspace** (the glob `crates/*` auto-includes, so
use `exclude` to opt out):

```toml
# CCH-Hanoi/Cargo.toml
[workspace]
members = ["crates/*"]
resolver = "2"
exclude = ["crates/hanoi-bench"]   # ← add this line
```

No other file changes needed. The rest of the workspace compiles and runs
identically.

**Option B — Delete the directory** (zero trace):

```bash
rm -rf crates/hanoi-bench
# The glob pattern stops matching, workspace is unchanged.
```

**Option C — Don't build it** (simplest):

```bash
# Only build what you need:
cargo build --release -p hanoi-server
# hanoi-bench is never compiled
```

### Why This Is Safe

1. **No reverse dependency**: `hanoi-core`, `hanoi-server`, `hanoi-cli`,
   `hanoi-gateway`, and `hanoi-tools` have **zero imports** from `hanoi-bench`
2. **No feature flags in other crates**: No `#[cfg(feature = "bench")]` in
   production code
3. **No shared state**: Benchmarks create their own `CchContext` and
   `QueryEngine` instances
4. **No build-script coupling**: No `build.rs` dependencies between crates
5. **Server benchmarks are external**: They hit the HTTP API like any client —
   the server doesn't know it's being benchmarked

---

## Implementation Order

| Phase | Description                             | Deliverable                                  |
| ----- | --------------------------------------- | -------------------------------------------- |
| 1     | Crate setup, framework types            | `hanoi-bench` compiles with basic types       |
| 2     | Query dataset generation                | Reproducible query sets (save/load JSON)      |
| 3     | Core CCH benchmarks + Criterion         | `bench_core` binary + `cargo bench` support   |
| 4     | Server HTTP benchmarks                  | `bench_server` binary with concurrency        |
| 5     | Statistical reporting                   | Table + JSON + CSV output formats             |
| 6     | Comparison + regression detection       | `bench_report` binary with threshold alerts   |

### CLI Usage Summary

```bash
# Generate a query dataset
bench_core --graph-dir ./data --generate-queries 10000 --seed 42 --save-queries queries.json

# Run core benchmarks
bench_core --graph-dir ./data --queries queries.json --iterations 10 --output core_results.json

# Run Criterion micro-benchmarks (auto-detected by Criterion)
GRAPH_DIR=./data cargo bench -p hanoi-bench

# Run server benchmarks
bench_server --url http://localhost:8080 --queries queries.json --concurrency 4 --output server_results.json

# Generate comparison report
bench_report --baseline baseline.json --current server_results.json --format table

# CI regression check
bench_report --baseline baseline.json --current server_results.json --threshold 10
```
