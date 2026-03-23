/// Analysis + report generation from saved benchmark results.
///
/// Usage:
///   bench_report --baseline results_v1.json --current results_v2.json
///   bench_report --baseline old.json --current new.json --threshold 10
///   bench_report --input results.json --format csv
use std::path::PathBuf;

use clap::Parser;
use tracing;

use hanoi_bench::BenchmarkRun;
use hanoi_bench::log::init_bench_tracing;
use hanoi_bench::report::{ReportFormat, compare_runs, generate_report, write_comparison_table};

#[derive(Parser)]
#[command(
    name = "bench_report",
    about = "Benchmark result analysis and comparison"
)]
struct Args {
    /// Input results file for single-run report
    #[arg(long)]
    input: Option<PathBuf>,

    /// Baseline results file for comparison
    #[arg(long)]
    baseline: Option<PathBuf>,

    /// Current results file for comparison
    #[arg(long)]
    current: Option<PathBuf>,

    /// Output format: table, json, csv
    #[arg(long, default_value = "table")]
    format: String,

    /// Regression threshold percentage (exit code 1 if exceeded)
    #[arg(long, default_value_t = 10.0)]
    threshold: f64,

    /// Custom log file name prefix (default: "bench_report")
    #[arg(long)]
    log_name: Option<String>,
}

fn parse_format(s: &str) -> ReportFormat {
    match s.to_lowercase().as_str() {
        "json" => ReportFormat::Json,
        "csv" => ReportFormat::Csv,
        _ => ReportFormat::Table,
    }
}

fn load_runs(path: &PathBuf) -> Result<Vec<BenchmarkRun>, String> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
    serde_json::from_str(&data)
        .map_err(|e| format!("failed to parse {}: {}", path.display(), e))
}

fn main() {
    let args = Args::parse();
    let (_log_path, _guard) = init_bench_tracing(args.log_name.as_deref());

    // Mode 1: Compare baseline vs current
    if let (Some(baseline_path), Some(current_path)) = (&args.baseline, &args.current) {
        let baseline = match load_runs(baseline_path) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("{}", e);
                drop(_guard);
                std::process::exit(1);
            }
        };
        let current = match load_runs(current_path) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("{}", e);
                drop(_guard);
                std::process::exit(1);
            }
        };

        let (results, any_regressed) = compare_runs(&baseline, &current, args.threshold);

        let mut stdout = std::io::stdout();
        write_comparison_table(&results, &mut stdout);

        if any_regressed {
            tracing::warn!(threshold = args.threshold, "REGRESSION DETECTED: one or more benchmarks regressed by more than threshold");
            // Explicitly drop guard to flush logs before exit
            drop(_guard);
            std::process::exit(1);
        }
        return;
    }

    // Mode 2: Single-run report
    if let Some(ref input_path) = args.input {
        let runs = match load_runs(input_path) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("{}", e);
                drop(_guard);
                std::process::exit(1);
            }
        };
        let format = parse_format(&args.format);
        let mut stdout = std::io::stdout();
        generate_report(&runs, format, &mut stdout);
        return;
    }

    tracing::error!("provide either --input for single report, or --baseline + --current for comparison");
    drop(_guard);
    std::process::exit(1);
}
