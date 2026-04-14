use std::path::Path;
#[cfg(feature = "ui")]
use std::path::PathBuf;

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use tracing_tree::HierarchicalLayer;

use crate::app::args::LogFormat;

/// Initialize tracing with format selection and optional file output.
/// Returns an optional WorkerGuard that MUST be held for the lifetime
/// of the program — dropping it flushes and closes the non-blocking
/// file writer.
pub fn init_tracing(log_format: &LogFormat, log_dir: Option<&Path>) -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=debug"));

    // Prepare the non-blocking file writer upfront (if requested).
    // The actual layer is created per-arm so its generic `S` parameter
    // matches the concrete subscriber it gets composed onto.
    let (writer, guard) = if let Some(dir) = log_dir {
        let file_appender = rolling::daily(dir, "hanoi-server.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        (Some(non_blocking), Some(guard))
    } else {
        (None, None)
    };

    /// Build an optional JSON file layer from a pre-allocated NonBlocking writer.
    fn file_layer<S>(
        writer: Option<NonBlocking>,
    ) -> Option<
        fmt::Layer<S, fmt::format::JsonFields, fmt::format::Format<fmt::format::Json>, NonBlocking>,
    >
    where
        S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    {
        writer.map(|w| fmt::layer().with_writer(w).with_ansi(false).json())
    }

    // Each match arm calls .init() separately because different stderr
    // formats produce different generic types that cannot be unified.
    match log_format {
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json())
                .with(file_layer(writer))
                .init();
        }
        LogFormat::Pretty => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().pretty())
                .with(file_layer(writer))
                .init();
        }
        LogFormat::Compact => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().compact().with_target(true))
                .with(file_layer(writer))
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
                .with(file_layer(writer))
                .init();
        }
        LogFormat::Full => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_target(true).with_thread_ids(true))
                .with(file_layer(writer))
                .init();
        }
    }

    guard
}

#[cfg(feature = "ui")]
pub fn resolve_repo_relative_path(path: &Path) -> PathBuf {
    if path.is_absolute() || path.exists() {
        return path.to_path_buf();
    }

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let repo_relative = repo_root.join(path);
    if repo_relative.exists() {
        return repo_relative;
    }

    path.to_path_buf()
}
