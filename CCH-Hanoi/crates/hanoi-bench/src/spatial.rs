use hanoi_core::SpatialIndex;
use rust_road_router::datastr::graph::{EdgeId, NodeId};

use crate::dataset::QueryPair;
use crate::{BenchmarkConfig, Measurement};

/// Benchmark `SpatialIndex::build()` — KD-tree construction.
pub fn bench_kd_build(
    lat: &[f32],
    lng: &[f32],
    first_out: &[EdgeId],
    head: &[NodeId],
    config: &BenchmarkConfig,
) -> Vec<Measurement> {
    // Warmup
    for _ in 0..config.warmup_iterations {
        let _ = SpatialIndex::build(lat, lng, first_out, head);
    }

    // Measured iterations
    let mut measurements = Vec::new();
    for i in 0..config.measured_iterations {
        let start = std::time::Instant::now();
        let _ = SpatialIndex::build(lat, lng, first_out, head);
        let elapsed = start.elapsed();
        measurements.push(Measurement {
            label: format!("kd_build_iter_{}", i),
            duration: elapsed,
            metadata: serde_json::json!({
                "ms": elapsed.as_secs_f64() * 1000.0,
                "num_nodes": lat.len(),
            }),
        });
    }
    measurements
}

/// Benchmark `SpatialIndex::snap_to_edge()` — point-to-edge snap.
pub fn bench_snap(
    spatial: &SpatialIndex,
    queries: &[QueryPair],
    config: &BenchmarkConfig,
) -> Vec<Measurement> {
    // Warmup
    for _ in 0..config.warmup_iterations {
        for q in queries.iter().take(config.query_count) {
            if let Some((lat, lng)) = q.from_coords {
                let _ = spatial.snap_to_edge(lat, lng);
            }
        }
    }

    // Measured iterations
    let mut measurements = Vec::new();
    for i in 0..config.measured_iterations {
        let start = std::time::Instant::now();
        let mut snapped = 0usize;
        for q in queries.iter().take(config.query_count) {
            if let Some((lat, lng)) = q.from_coords {
                let _ = spatial.snap_to_edge(lat, lng);
                snapped += 1;
            }
        }
        let elapsed = start.elapsed();
        measurements.push(Measurement {
            label: format!("snap_iter_{}", i),
            duration: elapsed,
            metadata: serde_json::json!({
                "snaps": snapped,
                "avg_us": if snapped > 0 { elapsed.as_micros() as f64 / snapped as f64 } else { 0.0 },
            }),
        });
    }
    measurements
}
