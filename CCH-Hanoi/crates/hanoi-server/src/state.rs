use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use tokio::sync::{mpsc, watch};

use hanoi_core::CoordRejection;
use rust_road_router::datastr::graph::Weight;
use rust_road_router::util::Storage;

use crate::camera_overlay::CameraOverlay;
use crate::route_eval::RouteEvaluator;
use crate::traffic::TrafficOverlay;
use crate::types::{BboxInfo, QueryRequest};

/// Message sent from the HTTP query handler to the background engine thread.
pub struct QueryMsg {
    pub request: QueryRequest,
    /// Response format from the URL query string (`?format=json`).
    /// `None` → GeoJSON (default), `Some("json")` → plain JSON.
    pub format: Option<String>,
    /// Whether to include simplestyle-spec color properties in GeoJSON output.
    pub colors: bool,
    pub alternatives: u32,
    pub stretch: f64,
    pub reply: tokio::sync::oneshot::Sender<Result<serde_json::Value, CoordRejection>>,
}

/// Shared application state injected into Axum handlers via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    /// Channel to send query requests to the background engine thread.
    pub query_tx: mpsc::Sender<QueryMsg>,
    /// Watch channel to send weight-update vectors to the customization loop.
    pub watch_tx: watch::Sender<Option<Vec<Weight>>>,
    /// Baseline weight vector for the currently loaded graph.
    pub baseline_weights: Arc<Storage<Weight>>,
    /// Latest accepted customization weights for read-only HTTP inspection.
    pub latest_weights: Arc<RwLock<Option<Vec<Weight>>>>,
    /// Number of edges in the loaded graph (for body-size validation).
    pub num_edges: usize,
    /// Number of nodes in the loaded graph (for /info).
    pub num_nodes: usize,
    /// Whether this server is in line-graph mode.
    pub is_line_graph: bool,
    /// Bounding box of the graph's geographic coverage.
    pub bbox: Option<BboxInfo>,
    /// Whether a background customization is currently running.
    pub customization_active: Arc<AtomicBool>,
    /// Whether the background engine thread is still alive.
    /// Set to false by the engine thread before it exits.
    pub engine_alive: Arc<AtomicBool>,
    /// Server uptime: instant when server started. Used to compute age.
    pub startup_time: std::time::Instant,
    /// Total successful queries processed (not counting validation failures).
    pub queries_processed: Arc<AtomicU64>,
    /// Precomputed geometry + baseline data for the traffic overlay endpoint.
    pub traffic_overlay: Arc<TrafficOverlay>,
    /// Route replay/evaluation support for imported GeoJSON routes.
    pub route_evaluator: Arc<RouteEvaluator>,
    /// Precomputed camera overlay data loaded from the configured YAML file.
    pub camera_overlay: Arc<CameraOverlay>,
}

impl AppState {
    pub fn is_customization_active(&self) -> bool {
        self.customization_active.load(Ordering::Relaxed)
    }

    pub fn is_engine_alive(&self) -> bool {
        self.engine_alive.load(Ordering::Relaxed)
    }

    pub fn uptime_secs(&self) -> u64 {
        self.startup_time.elapsed().as_secs()
    }

    pub fn total_queries(&self) -> u64 {
        self.queries_processed.load(Ordering::Relaxed)
    }
}
