use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, RwLock};

use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};

use hanoi_core::{BoundingBox, CchContext, LineGraphCchContext};
use rust_road_router::datastr::graph::Weight;

use crate::api::dto::BboxInfo;
use crate::api::state::{AppState, QueryMsg};
use crate::app::args::Args;
#[cfg(feature = "ui")]
use crate::app::routes::mount_ui;
use crate::app::routes::{build_customize_router, build_query_router};
use crate::app::tracing::init_tracing;
#[cfg(feature = "ui")]
use crate::app::tracing::resolve_repo_relative_path;
use crate::runtime::worker;
#[cfg(feature = "ui")]
use crate::ui::camera::CameraOverlay;
#[cfg(feature = "ui")]
use crate::ui::route_eval::RouteEvaluator;
#[cfg(feature = "ui")]
use crate::ui::traffic::TrafficOverlay;

pub async fn run(args: Args) {
    let _guard = init_tracing(&args.log_format, args.log_dir.as_deref());
    #[cfg(feature = "ui")]
    let camera_config_path = resolve_repo_relative_path(&args.camera_config);

    let (query_tx, mut query_rx) = mpsc::channel::<QueryMsg>(256);
    let (watch_tx, mut watch_rx) = watch::channel::<Option<Vec<Weight>>>(None);
    let customization_active = Arc::new(AtomicBool::new(false));
    let engine_alive = Arc::new(AtomicBool::new(true));
    let startup_time = std::time::Instant::now();
    let queries_processed = Arc::new(AtomicU64::new(0));

    // Load graph and build CCH, then spawn background engine thread
    #[cfg(feature = "ui")]
    let (
        num_nodes,
        num_edges,
        bbox,
        baseline_weights,
        traffic_overlay,
        route_evaluator,
        camera_overlay,
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
            worker::run_line_graph(&context, &mut query_rx, &mut watch_rx, &ca, &ea, &rt);
        });

        (
            nn,
            ne,
            bbox,
            baseline_weights,
            traffic_overlay,
            route_evaluator,
            camera_overlay,
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
            worker::run_normal(&context, &mut query_rx, &mut watch_rx, &ca, &ea, &rt);
        });

        (
            nn,
            ne,
            bbox,
            baseline_weights,
            traffic_overlay,
            route_evaluator,
            camera_overlay,
        )
    };

    #[cfg(not(feature = "ui"))]
    let (num_nodes, num_edges, bbox, baseline_weights) = if args.line_graph {
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
        let baseline_weights = Arc::new(context.baseline_weights.clone());

        let ca = customization_active.clone();
        let ea = engine_alive.clone();
        let rt = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            worker::run_line_graph(&context, &mut query_rx, &mut watch_rx, &ca, &ea, &rt);
        });

        (nn, ne, bbox, baseline_weights)
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
        let baseline_weights = Arc::new(context.baseline_weights.clone());

        let ca = customization_active.clone();
        let ea = engine_alive.clone();
        let rt = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            worker::run_normal(&context, &mut query_rx, &mut watch_rx, &ca, &ea, &rt);
        });

        (nn, ne, bbox, baseline_weights)
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
        #[cfg(feature = "ui")]
        traffic_overlay,
        #[cfg(feature = "ui")]
        route_evaluator,
        #[cfg(feature = "ui")]
        camera_overlay,
    };

    // Query port — JSON API for external consumers
    let query_router = build_query_router(state.clone());
    #[cfg(feature = "ui")]
    let query_router = if args.serve_ui {
        mount_ui(query_router)
    } else {
        query_router
    };

    // Customize port — binary API with gzip decompression for internal pipeline
    let customize_router = build_customize_router(state.clone());

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
    #[cfg(feature = "ui")]
    let ui_enabled = args.serve_ui;
    #[cfg(not(feature = "ui"))]
    let ui_enabled = false;
    tracing::info!(
        %query_addr,
        %customize_addr,
        mode,
        ui_enabled,
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
