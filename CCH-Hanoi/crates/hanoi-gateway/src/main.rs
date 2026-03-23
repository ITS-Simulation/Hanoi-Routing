mod proxy;
mod types;

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::Router;
use axum::routing::{get, post};
use clap::{Parser, ValueEnum};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::proxy::GatewayState;

// ---------------------------------------------------------------------------
// CLI arguments
// ---------------------------------------------------------------------------

#[derive(Clone, Default, ValueEnum)]
enum LogFormat {
    /// Multi-line, colorized, with source locations (most readable)
    #[default]
    Pretty,
    /// Single-line with inline span context
    Full,
    /// Abbreviated single-line
    Compact,
    /// Indented tree hierarchy (falls back to full)
    Tree,
    /// Newline-delimited JSON for log aggregation
    Json,
}

#[derive(Parser)]
#[command(
    name = "hanoi_gateway",
    about = "API gateway for Hanoi routing servers (query/info only)"
)]
struct Args {
    /// Gateway port
    #[arg(long, default_value = "50051")]
    port: u16,

    /// Normal graph server query endpoint (e.g. http://localhost:8080)
    #[arg(long, default_value = "http://localhost:8080")]
    normal_backend: String,

    /// Line graph server query endpoint (e.g. http://localhost:8081)
    #[arg(long, default_value = "http://localhost:8081")]
    line_graph_backend: String,

    /// Backend request timeout in seconds. Set to 0 to disable.
    #[arg(long, default_value = "30")]
    backend_timeout_secs: u64,

    /// Log output format
    #[arg(long, value_enum, default_value_t = LogFormat::Pretty)]
    log_format: LogFormat,

    /// Also write logs to file in JSON format (logs go to both stderr and file)
    #[arg(long, value_name = "PATH")]
    log_file: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Tracing initialization
// ---------------------------------------------------------------------------

/// Initialize tracing. Returns an optional `WorkerGuard` that **must** be held
/// alive for the program's lifetime — dropping it closes the file writer.
fn init_tracing(log_format: &LogFormat, log_file: Option<&PathBuf>) -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Optional file writer (non-blocking with no ANSI codes)
    let (file_writer, guard) = if let Some(path) = log_file {
        match std::fs::File::create(path) {
            Ok(file) => {
                let (non_blocking, guard) = tracing_appender::non_blocking(file);
                (Some(non_blocking), Some(guard))
            }
            Err(e) => {
                eprintln!("failed to create log file: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        (None, None)
    };

    /// Build an optional JSON file layer from a pre-allocated NonBlocking writer.
    /// Uses JSON format to avoid ANSI code leakage from the stderr layer's span fields.
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

    // Compose stderr + optional file logging
    match log_format {
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json())
                .with(file_layer(file_writer))
                .init();
        }
        LogFormat::Pretty => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().pretty())
                .with(file_layer(file_writer))
                .init();
        }
        LogFormat::Compact => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().compact())
                .with(file_layer(file_writer))
                .init();
        }
        LogFormat::Full | LogFormat::Tree => {
            // Tree not available in gateway (no tracing-tree dep); fall back to full
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_target(true))
                .with(file_layer(file_writer))
                .init();
        }
    }

    guard
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let _guard = init_tracing(&args.log_format, args.log_file.as_ref());

    let state = GatewayState::new(
        &args.normal_backend,
        &args.line_graph_backend,
        args.backend_timeout_secs,
    );

    let router = Router::new()
        .route("/query", post(proxy::handle_query))
        .route("/info", get(proxy::handle_info))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    let listener = TcpListener::bind(addr)
        .await
        .expect("failed to bind gateway port");

    tracing::info!(%addr, %args.normal_backend, %args.line_graph_backend, "gateway ready");

    axum::serve(listener, router).await.unwrap();
}
