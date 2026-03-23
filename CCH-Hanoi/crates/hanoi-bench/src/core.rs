use std::path::Path;

use hanoi_core::{CchContext, QueryEngine};
use rust_road_router::datastr::graph::Weight;

use crate::dataset::QueryPair;
use crate::{BenchmarkConfig, Measurement};

/// Benchmark `CchContext::load_and_build()` — Phase 1 contraction.
///
/// Returns one measurement per iteration (wall-clock time for the full
/// load-and-build cycle).
pub fn bench_cch_build(
    graph_dir: &Path,
    perm_path: &Path,
    config: &BenchmarkConfig,
) -> Vec<Measurement> {
    // Warmup
    for _ in 0..config.warmup_iterations {
        let _ = CchContext::load_and_build(graph_dir, perm_path);
    }

    // Measured iterations
    let mut measurements = Vec::new();
    for i in 0..config.measured_iterations {
        let start = std::time::Instant::now();
        let _ = CchContext::load_and_build(graph_dir, perm_path);
        let elapsed = start.elapsed();
        measurements.push(Measurement {
            label: format!("cch_build_iter_{}", i),
            duration: elapsed,
            metadata: serde_json::json!({
                "seconds": elapsed.as_secs_f64(),
            }),
        });
    }
    measurements
}

/// Benchmark `CchContext::customize()` — Phase 2 weight application.
pub fn bench_customize(context: &CchContext, config: &BenchmarkConfig) -> Vec<Measurement> {
    // Warmup
    for _ in 0..config.warmup_iterations {
        let _ = context.customize();
    }

    // Measured iterations
    let mut measurements = Vec::new();
    for i in 0..config.measured_iterations {
        let start = std::time::Instant::now();
        let _ = context.customize();
        let elapsed = start.elapsed();
        measurements.push(Measurement {
            label: format!("customize_iter_{}", i),
            duration: elapsed,
            metadata: serde_json::json!({
                "ms": elapsed.as_secs_f64() * 1000.0,
            }),
        });
    }
    measurements
}

/// Benchmark `QueryEngine::query()` — Phase 3 shortest path by node IDs.
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

/// Benchmark `QueryEngine::query_coords()` — snap + query by coordinates.
pub fn bench_query_coords(
    engine: &mut QueryEngine<'_>,
    queries: &[QueryPair],
    config: &BenchmarkConfig,
) -> Vec<Measurement> {
    // Warmup
    for _ in 0..config.warmup_iterations {
        for q in queries.iter().take(config.query_count) {
            if let (Some(from), Some(to)) = (q.from_coords, q.to_coords) {
                let _ = engine.query_coords(from, to);
            }
        }
    }

    // Measured iterations
    let mut measurements = Vec::new();
    for i in 0..config.measured_iterations {
        let start = std::time::Instant::now();
        let mut found = 0usize;
        let mut queried = 0usize;
        for q in queries.iter().take(config.query_count) {
            if let (Some(from), Some(to)) = (q.from_coords, q.to_coords) {
                queried += 1;
                if engine.query_coords(from, to).is_ok_and(|v| v.is_some()) {
                    found += 1;
                }
            }
        }
        let elapsed = start.elapsed();
        measurements.push(Measurement {
            label: format!("query_coords_iter_{}", i),
            duration: elapsed,
            metadata: serde_json::json!({
                "queries": queried,
                "found": found,
                "avg_us": if queried > 0 { elapsed.as_micros() as f64 / queried as f64 } else { 0.0 },
            }),
        });
    }
    measurements
}

/// Benchmark `QueryEngine::update_weights()` — re-customization with new weights.
pub fn bench_update_weights(
    engine: &mut QueryEngine<'_>,
    weights: &[Weight],
    config: &BenchmarkConfig,
) -> Vec<Measurement> {
    // Warmup
    for _ in 0..config.warmup_iterations {
        engine.update_weights(weights);
    }

    // Measured iterations
    let mut measurements = Vec::new();
    for i in 0..config.measured_iterations {
        let start = std::time::Instant::now();
        engine.update_weights(weights);
        let elapsed = start.elapsed();
        measurements.push(Measurement {
            label: format!("update_weights_iter_{}", i),
            duration: elapsed,
            metadata: serde_json::json!({
                "ms": elapsed.as_secs_f64() * 1000.0,
            }),
        });
    }
    measurements
}
