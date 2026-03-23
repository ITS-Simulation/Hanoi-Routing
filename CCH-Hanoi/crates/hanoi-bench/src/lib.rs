pub mod core;
pub mod dataset;
pub mod log;
pub mod report;
pub mod server;
pub mod spatial;

/// A single timing measurement.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Measurement {
    pub label: String,
    pub duration: std::time::Duration,
    pub metadata: serde_json::Value,
}

/// Collection of measurements for a benchmark run.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchmarkRun {
    pub name: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub graph_dir: String,
    pub measurements: Vec<Measurement>,
    pub config: BenchmarkConfig,
}

/// Benchmark configuration.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchmarkConfig {
    pub warmup_iterations: usize,
    pub measured_iterations: usize,
    pub query_count: usize,
    pub seed: u64,
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

/// Compute percentile from a pre-sorted slice. `p` is 0–100.
pub fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
