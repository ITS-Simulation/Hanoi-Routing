mod config;
mod proxy;
mod types;

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::Router;
use axum::routing::{get, post};
use clap::Parser;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::{GatewayConfig, LogFormat};
use crate::proxy::GatewayState;

// ---------------------------------------------------------------------------
// CLI arguments
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "hanoi_gateway",
    about = "Profile-based API gateway for Hanoi routing servers",
    long_about = "\
Profile-based API gateway for Hanoi routing servers.\n\n\
All backend configuration is read from a YAML config file.\n\
See gateway.yaml for the expected format."
)]
struct Args {
    /// Path to the gateway YAML config file.
    #[arg(long, default_value = "gateway.yaml")]
    config: PathBuf,

    /// Override the port from the config file.
    #[arg(long)]
    port: Option<u16>,
}

// ---------------------------------------------------------------------------
// Tracing initialization
// ---------------------------------------------------------------------------

/// Initialize tracing. Returns an optional `WorkerGuard` that **must** be held
/// alive for the program's lifetime — dropping it closes the file writer.
fn init_tracing(log_format: &LogFormat, log_file: Option<&str>) -> Option<WorkerGuard> {
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

    let config = GatewayConfig::load(&args.config).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    let _guard = init_tracing(&config.log_format, config.log_file.as_deref());

    // CLI --port overrides config file
    let port = args.port.unwrap_or(config.port);

    let profile_names: Vec<String> = {
        let mut names: Vec<String> = config.profiles.keys().cloned().collect();
        names.sort_unstable();
        names
    };

    let state = GatewayState::new(config.profiles, config.backend_timeout_secs);

    let router = Router::new()
        .route("/query", post(proxy::handle_query))
        .route("/reset_weights", post(proxy::handle_reset_weights))
        .route("/info", get(proxy::handle_info))
        .route("/status", get(proxy::handle_info))
        .route("/health", get(proxy::handle_health))
        .route("/ready", get(proxy::handle_ready))
        .route("/profiles", get(proxy::handle_profiles))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr)
        .await
        .expect("failed to bind gateway port");

    tracing::info!(
        %addr,
        profiles = ?profile_names,
        config_file = %args.config.display(),
        "gateway ready"
    );

    axum::serve(listener, router).await.unwrap();
}
