//! Benchmark logging with always-on file output.
//!
//! Every bench binary gets dual logging (stderr + file) by default.
//! The file uses JSON format for machine-readable post-analysis.
//! Stderr uses compact format for human-readable progress.

use std::path::PathBuf;

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize tracing for benchmark binaries.
///
/// Always creates a log file in the current directory. The file name is
/// `{name}_{timestamp}.log` where `name` defaults to the binary name.
///
/// Returns the log file path and a `WorkerGuard` that **must** be held alive
/// for the program's lifetime.
pub fn init_bench_tracing(name: Option<&str>) -> (PathBuf, WorkerGuard) {
    let bin_name = name.unwrap_or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .as_deref()
            .unwrap_or("bench")
            // Leak is fine — this is called once at startup.
            .to_string()
            .leak()
    });

    let timestamp = chrono::Local::now().format("%Y-%m-%dT%H%M%S");
    let log_filename = format!("{}_{}.log", bin_name, timestamp);
    let log_path = PathBuf::from(&log_filename);

    let file = std::fs::File::create(&log_path).unwrap_or_else(|e| {
        eprintln!("failed to create bench log file {}: {}", log_filename, e);
        std::process::exit(1);
    });

    let (non_blocking, guard) = tracing_appender::non_blocking(file);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    /// JSON file layer — machine-readable, no ANSI codes.
    fn file_layer<S>(
        writer: NonBlocking,
    ) -> fmt::Layer<S, fmt::format::JsonFields, fmt::format::Format<fmt::format::Json>, NonBlocking>
    where
        S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    {
        fmt::layer().with_writer(writer).with_ansi(false).json()
    }

    // Stderr: compact for concise progress. File: JSON for analysis.
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().compact().with_target(false))
        .with(file_layer(non_blocking))
        .init();

    tracing::info!(log_file = %log_path.display(), "benchmark logging initialized");

    (log_path, guard)
}
