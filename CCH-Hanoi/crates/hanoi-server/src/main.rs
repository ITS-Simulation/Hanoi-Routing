mod camera_overlay;
mod engine;
mod handlers;
mod route_eval;
mod state;
mod traffic;
mod types;
mod ui;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, RwLock};

use axum::Router;
use axum::routing::{get, post};
use clap::{Parser, ValueEnum};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tower_http::cors::CorsLayer;
use tower_http::decompression::RequestDecompressionLayer;
use tower_http::trace::TraceLayer;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use tracing_tree::HierarchicalLayer;

use hanoi_core::BoundingBox;
use hanoi_core::cch::CchContext;
use hanoi_core::line_graph::LineGraphCchContext;
use rust_road_router::datastr::graph::Weight;

use crate::camera_overlay::CameraOverlay;
use crate::route_eval::RouteEvaluator;
use crate::state::{AppState, QueryMsg};
use crate::traffic::TrafficOverlay;
use crate::types::BboxInfo;

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
    /// Indented tree hierarchy
    Tree,
    /// Newline-delimited JSON for log aggregation
    Json,
}

#[derive(Parser)]
#[command(
    name = "hanoi_server",
    about = "CCH routing server for Hanoi road network"
)]
struct Args {
    /// Path to the graph directory (e.g. Maps/data/hanoi_car/graph)
    #[arg(long)]
    graph_dir: PathBuf,

    /// Path to the original graph directory (required for --line-graph mode)
    #[arg(long)]
    original_graph_dir: Option<PathBuf>,

    /// Camera YAML file used by the optional camera overlay.
    #[arg(long, default_value = "CCH_Data_Pipeline/config/mvp_camera.yaml")]
    camera_config: PathBuf,

    /// Port for the query API (REST/JSON)
    #[arg(long, default_value = "8080")]
    query_port: u16,

    /// Port for the customization API (REST/binary)
    #[arg(long, default_value = "9080")]
    customize_port: u16,

    /// Serve the bundled route-viewer UI on the query port.
    /// When omitted, the server exposes only the API endpoints.
    #[arg(long)]
    serve_ui: bool,

    /// Enable line-graph mode (uses DirectedCCH, final-edge correction)
    #[arg(long)]
    line_graph: bool,

    /// Log output format
    #[arg(long, value_enum, default_value_t = LogFormat::Pretty)]
    log_format: LogFormat,

    /// Directory for persistent log files (daily rotation, JSON format).
    /// Omit to log to stderr only.
    #[arg(long)]
    log_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Tracing initialization
// ---------------------------------------------------------------------------

/// Initialize tracing with format selection and optional file output.
/// Returns an optional WorkerGuard that MUST be held for the lifetime
/// of the program — dropping it flushes and closes the non-blocking
/// file writer.
fn init_tracing(log_format: &LogFormat, log_dir: Option<&Path>) -> Option<WorkerGuard> {
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

fn resolve_repo_relative_path(path: &Path) -> PathBuf {
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

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let _guard = init_tracing(&args.log_format, args.log_dir.as_deref());
    let camera_config_path = resolve_repo_relative_path(&args.camera_config);

    let (query_tx, mut query_rx) = mpsc::channel::<QueryMsg>(256);
    let (watch_tx, mut watch_rx) = watch::channel::<Option<Vec<Weight>>>(None);
    let customization_active = Arc::new(AtomicBool::new(false));
    let engine_alive = Arc::new(AtomicBool::new(true));
    let startup_time = std::time::Instant::now();
    let queries_processed = Arc::new(AtomicU64::new(0));

    // Load graph and build CCH, then spawn background engine thread
    let (
        num_nodes,
        num_edges,
        bbox,
        traffic_overlay,
        route_evaluator,
        camera_overlay,
        baseline_weights,
    ) = if args.line_graph {
        let original_dir = args
            .original_graph_dir
            .as_ref()
            .expect("--original-graph-dir required for --line-graph mode");
        let perm_path = args.graph_dir.join("perms/cch_perm");

        let context =
            LineGraphCchContext::load_and_build(&args.graph_dir, original_dir, &perm_path)
                .expect("failed to load line graph");
        let nn = context.graph.num_nodes();
        let ne = context.graph.num_edges();
        let bbox = {
            let bb =
                BoundingBox::from_coords(&context.original_latitude, &context.original_longitude);
            Some(BboxInfo {
                min_lat: bb.min_lat,
                max_lat: bb.max_lat,
                min_lng: bb.min_lng,
                max_lng: bb.max_lng,
            })
        };

        let traffic_overlay = Arc::new(TrafficOverlay::from_line_graph(
            &context,
            &original_dir.join("road_arc_manifest.arrow"),
        ));
        let route_evaluator = Arc::new(RouteEvaluator::from_line_graph(&context));
        let camera_overlay = Arc::new(CameraOverlay::load(
            &original_dir.join("road_arc_manifest.arrow"),
            &camera_config_path,
        ));
        let baseline_weights = Arc::new(context.baseline_weights.clone());

        let ca = customization_active.clone();
        let ea = engine_alive.clone();
        let rt = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            engine::run_line_graph(&context, &mut query_rx, &mut watch_rx, &ca, &ea, &rt);
        });

        (
            nn,
            ne,
            bbox,
            traffic_overlay,
            route_evaluator,
            camera_overlay,
            baseline_weights,
        )
    } else {
        let perm_path = args.graph_dir.join("perms/cch_perm");

        let context =
            CchContext::load_and_build(&args.graph_dir, &perm_path).expect("failed to load graph");
        let nn = context.graph.num_nodes();
        let ne = context.graph.num_edges();
        let bbox = {
            let bb = BoundingBox::from_coords(&context.graph.latitude, &context.graph.longitude);
            Some(BboxInfo {
                min_lat: bb.min_lat,
                max_lat: bb.max_lat,
                min_lng: bb.min_lng,
                max_lng: bb.max_lng,
            })
        };

        let traffic_overlay = Arc::new(TrafficOverlay::from_normal(
            &context,
            &args.graph_dir.join("road_arc_manifest.arrow"),
        ));
        let route_evaluator = Arc::new(RouteEvaluator::from_normal(&context));
        let camera_overlay = Arc::new(CameraOverlay::load(
            &args.graph_dir.join("road_arc_manifest.arrow"),
            &camera_config_path,
        ));
        let baseline_weights = Arc::new(context.baseline_weights.clone());

        let ca = customization_active.clone();
        let ea = engine_alive.clone();
        let rt = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            engine::run_normal(&context, &mut query_rx, &mut watch_rx, &ca, &ea, &rt);
        });

        (
            nn,
            ne,
            bbox,
            traffic_overlay,
            route_evaluator,
            camera_overlay,
            baseline_weights,
        )
    };

    let state = AppState {
        query_tx,
        watch_tx,
        baseline_weights,
        latest_weights: Arc::new(RwLock::new(None)),
        num_edges,
        num_nodes,
        is_line_graph: args.line_graph,
        bbox,
        customization_active,
        engine_alive,
        startup_time,
        queries_processed,
        traffic_overlay,
        route_evaluator,
        camera_overlay,
    };

    // Query port — JSON API for external consumers
    let mut query_router = Router::new()
        .route("/query", post(handlers::handle_query))
        .route("/evaluate_routes", post(handlers::handle_evaluate_routes))
        .route("/reset_weights", post(handlers::handle_reset_weights))
        .route("/traffic_overlay", get(handlers::handle_traffic_overlay))
        .route("/camera_overlay", get(handlers::handle_camera_overlay))
        .route("/info", get(handlers::handle_info))
        .route("/health", get(handlers::handle_health))
        .route("/ready", get(handlers::handle_ready))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    if args.serve_ui {
        query_router = query_router
            .route("/", get(ui::handle_index))
            .route("/ui", get(ui::handle_index))
            .route("/assets/cch-query.css", get(ui::handle_styles))
            .route("/assets/cch-query.js", get(ui::handle_script));
    }

    // Customize port — binary API with gzip decompression for internal pipeline
    let customize_router = Router::new()
        .route("/customize", post(handlers::handle_customize))
        .layer(TraceLayer::new_for_http())
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024))
        .layer(RequestDecompressionLayer::new())
        .with_state(state.clone());

    let query_addr = SocketAddr::from(([0, 0, 0, 0], args.query_port));
    let customize_addr = SocketAddr::from(([0, 0, 0, 0], args.customize_port));

    let query_listener = TcpListener::bind(query_addr)
        .await
        .expect("failed to bind query port");
    let customize_listener = TcpListener::bind(customize_addr)
        .await
        .expect("failed to bind customize port");

    let mode = if args.line_graph {
        "line_graph"
    } else {
        "normal"
    };
    tracing::info!(
        %query_addr,
        %customize_addr,
        mode,
        ui_enabled = args.serve_ui,
        "server ready"
    );

    // Broadcast channel: one send notifies both listeners to begin graceful shutdown
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    let customize_shutdown = {
        let mut rx = shutdown_tx.subscribe();
        async move {
            let _ = rx.recv().await;
        }
    };
    let query_shutdown = {
        let mut rx = shutdown_tx.subscribe();
        async move {
            let _ = rx.recv().await;
        }
    };

    // Subscribe to the shutdown broadcast for the 30s force-kill timeout.
    // This MUST happen before spawning the signal handler to guarantee
    // the subscription exists when the broadcast fires.
    let shutdown_timeout = {
        let mut rx = shutdown_tx.subscribe();
        async move {
            let _ = rx.recv().await;
            let shutdown_deadline =
                tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);
            tokio::time::sleep_until(shutdown_deadline).await;
        }
    };

    let customize_handle = tokio::spawn(async move {
        axum::serve(customize_listener, customize_router)
            .with_graceful_shutdown(customize_shutdown)
            .await
            .unwrap();
    });

    let signal_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        let sigterm = async {
            let mut sig = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler");
            sig.recv().await;
        };
        #[cfg(not(unix))]
        let sigterm = std::future::pending::<()>();

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("SIGINT received, initiating graceful shutdown");
            }
            _ = sigterm => {
                tracing::info!("SIGTERM received, initiating graceful shutdown");
            }
        }

        let _ = signal_tx.send(());
    });

    let query_result = tokio::select! {
        biased;
        _ = shutdown_timeout => {
            tracing::warn!("graceful shutdown timeout reached (30s), forcing exit");
            std::process::exit(1);
        }
        result = axum::serve(query_listener, query_router)
            .with_graceful_shutdown(query_shutdown) => {
            result
        }
    };

    let _ = query_result;
    let _ = customize_handle.await;
    tracing::info!("shutdown complete");
}
