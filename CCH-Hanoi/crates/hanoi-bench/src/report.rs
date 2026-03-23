use std::io::Write;

use statrs::statistics::Statistics;

use crate::{BenchmarkRun, percentile_sorted};

/// Report output format.
pub enum ReportFormat {
    /// Human-readable table to stdout.
    Table,
    /// Machine-readable JSON (for CI/regression tracking).
    Json,
    /// CSV (for spreadsheet analysis).
    Csv,
}

/// Computed statistics for a benchmark category.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchmarkStats {
    pub name: String,
    pub min_us: f64,
    pub max_us: f64,
    pub mean_us: f64,
    pub median_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub std_dev_us: f64,
    pub throughput_qps: f64,
    pub success_rate: f64,
}

/// Compute statistics from a benchmark run's measurements.
pub fn compute_stats(run: &BenchmarkRun) -> BenchmarkStats {
    let durations_us: Vec<f64> = run
        .measurements
        .iter()
        .map(|m| m.duration.as_secs_f64() * 1_000_000.0)
        .collect();

    if durations_us.is_empty() {
        return BenchmarkStats {
            name: run.name.clone(),
            min_us: 0.0,
            max_us: 0.0,
            mean_us: 0.0,
            median_us: 0.0,
            p95_us: 0.0,
            p99_us: 0.0,
            std_dev_us: 0.0,
            throughput_qps: 0.0,
            success_rate: 0.0,
        };
    }

    let min_us = (&durations_us[..]).min();
    let max_us = (&durations_us[..]).max();
    let mean_us = (&durations_us[..]).mean();
    let std_dev_us = if durations_us.len() > 1 {
        (&durations_us[..]).std_dev()
    } else {
        0.0
    };

    let mut sorted = durations_us.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let median_us = percentile_sorted(&sorted, 50.0);
    let p95_us = percentile_sorted(&sorted, 95.0);
    let p99_us = percentile_sorted(&sorted, 99.0);

    // Compute throughput from metadata if available
    let total_queries: usize = run
        .measurements
        .iter()
        .filter_map(|m| m.metadata.get("queries").and_then(|v| v.as_u64()))
        .map(|q| q as usize)
        .sum();
    let total_time_s: f64 = run
        .measurements
        .iter()
        .map(|m| m.duration.as_secs_f64())
        .sum();
    let throughput_qps = if total_time_s > 0.0 && total_queries > 0 {
        total_queries as f64 / total_time_s
    } else {
        0.0
    };

    // Compute success rate from metadata if available
    let found: usize = run
        .measurements
        .iter()
        .filter_map(|m| m.metadata.get("found").and_then(|v| v.as_u64()))
        .map(|f| f as usize)
        .sum();
    let queried: usize = run
        .measurements
        .iter()
        .filter_map(|m| {
            m.metadata
                .get("queries")
                .and_then(|v| v.as_u64())
                .or_else(|| m.metadata.get("snaps").and_then(|v| v.as_u64()))
        })
        .map(|q| q as usize)
        .sum();
    let success_rate = if queried > 0 {
        found as f64 / queried as f64
    } else {
        0.0
    };

    BenchmarkStats {
        name: run.name.clone(),
        min_us,
        max_us,
        mean_us,
        median_us,
        p95_us,
        p99_us,
        std_dev_us,
        throughput_qps,
        success_rate,
    }
}

/// Generate a report from benchmark runs in the specified format.
pub fn generate_report(runs: &[BenchmarkRun], format: ReportFormat, output: &mut dyn Write) {
    let stats: Vec<BenchmarkStats> = runs.iter().map(compute_stats).collect();

    match format {
        ReportFormat::Table => write_table(&stats, output),
        ReportFormat::Json => write_json(&stats, output),
        ReportFormat::Csv => write_csv(&stats, output),
    }
}

/// Format a microsecond value into a human-readable string.
fn format_duration_us(us: f64) -> String {
    if us >= 1_000_000.0 {
        format!("{:.2}s", us / 1_000_000.0)
    } else if us >= 1_000.0 {
        format!("{:.1}ms", us / 1_000.0)
    } else {
        format!("{:.0}us", us)
    }
}

fn write_table(stats: &[BenchmarkStats], output: &mut dyn Write) {
    writeln!(
        output,
        "{:<25} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "Benchmark", "Min", "Mean", "p50", "p95", "p99", "Max"
    )
    .unwrap();
    writeln!(output, "{}", "-".repeat(87)).unwrap();

    for s in stats {
        writeln!(
            output,
            "{:<25} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
            s.name,
            format_duration_us(s.min_us),
            format_duration_us(s.mean_us),
            format_duration_us(s.median_us),
            format_duration_us(s.p95_us),
            format_duration_us(s.p99_us),
            format_duration_us(s.max_us),
        )
        .unwrap();
    }

    // Print throughput and success rate summaries if available
    for s in stats {
        if s.throughput_qps > 0.0 {
            writeln!(
                output,
                "Throughput ({}): {:.0} queries/sec",
                s.name, s.throughput_qps
            )
            .unwrap();
        }
        if s.success_rate > 0.0 {
            writeln!(
                output,
                "Success rate ({}): {:.1}%",
                s.name,
                s.success_rate * 100.0
            )
            .unwrap();
        }
    }
}

fn write_json(stats: &[BenchmarkStats], output: &mut dyn Write) {
    let json = serde_json::to_string_pretty(stats).expect("failed to serialize stats");
    write!(output, "{}", json).unwrap();
}

fn write_csv(stats: &[BenchmarkStats], output: &mut dyn Write) {
    let mut wtr = csv::Writer::from_writer(output);
    wtr.write_record([
        "name",
        "min_us",
        "max_us",
        "mean_us",
        "median_us",
        "p95_us",
        "p99_us",
        "std_dev_us",
        "throughput_qps",
        "success_rate",
    ])
    .unwrap();
    for s in stats {
        wtr.write_record(&[
            s.name.clone(),
            format!("{:.2}", s.min_us),
            format!("{:.2}", s.max_us),
            format!("{:.2}", s.mean_us),
            format!("{:.2}", s.median_us),
            format!("{:.2}", s.p95_us),
            format!("{:.2}", s.p99_us),
            format!("{:.2}", s.std_dev_us),
            format!("{:.2}", s.throughput_qps),
            format!("{:.4}", s.success_rate),
        ])
        .unwrap();
    }
    wtr.flush().unwrap();
}

/// Comparison result between baseline and current benchmark.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ComparisonResult {
    pub name: String,
    pub baseline_p50_us: f64,
    pub current_p50_us: f64,
    pub change_percent: f64,
    pub regressed: bool,
}

/// Compare two sets of benchmark results.
///
/// Returns comparison results and whether any benchmark regressed beyond
/// the threshold (percentage, e.g. 10.0 = 10%).
pub fn compare_runs(
    baseline: &[BenchmarkRun],
    current: &[BenchmarkRun],
    threshold: f64,
) -> (Vec<ComparisonResult>, bool) {
    let baseline_stats: Vec<BenchmarkStats> = baseline.iter().map(compute_stats).collect();
    let current_stats: Vec<BenchmarkStats> = current.iter().map(compute_stats).collect();

    let mut results = Vec::new();
    let mut any_regressed = false;

    for cs in &current_stats {
        if let Some(bs) = baseline_stats.iter().find(|b| b.name == cs.name) {
            let change_percent = if bs.median_us > 0.0 {
                ((cs.median_us - bs.median_us) / bs.median_us) * 100.0
            } else {
                0.0
            };
            let regressed = change_percent > threshold;
            if regressed {
                any_regressed = true;
            }
            results.push(ComparisonResult {
                name: cs.name.clone(),
                baseline_p50_us: bs.median_us,
                current_p50_us: cs.median_us,
                change_percent,
                regressed,
            });
        }
    }

    (results, any_regressed)
}

/// Write comparison results as a table.
pub fn write_comparison_table(results: &[ComparisonResult], output: &mut dyn Write) {
    writeln!(
        output,
        "{:<25} {:>12} {:>12} {:>10}",
        "Benchmark", "Baseline", "Current", "Change"
    )
    .unwrap();
    writeln!(output, "{}", "-".repeat(61)).unwrap();

    for r in results {
        let change_str = if r.change_percent >= 0.0 {
            format!("+{:.1}%", r.change_percent)
        } else {
            format!("{:.1}%", r.change_percent)
        };
        let marker = if r.regressed { " REGRESSION" } else { "" };
        writeln!(
            output,
            "{:<25} {:>12} {:>12} {:>10}{}",
            r.name,
            format_duration_us(r.baseline_p50_us),
            format_duration_us(r.current_p50_us),
            change_str,
            marker,
        )
        .unwrap();
    }
}
