use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tokio::sync::{mpsc, watch};

use hanoi_core::CoordRejection;
use rust_road_router::datastr::graph::Weight;

use crate::types::{BboxInfo, QueryRequest};

/// Message sent from the HTTP query handler to the background engine thread.
pub struct QueryMsg {
    pub request: QueryRequest,
    pub reply: tokio::sync::oneshot::Sender<Result<serde_json::Value, CoordRejection>>,
}

/// Shared application state injected into Axum handlers via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    /// Channel to send query requests to the background engine thread.
    pub query_tx: mpsc::Sender<QueryMsg>,
    /// Watch channel to send weight-update vectors to the customization loop.
    pub watch_tx: watch::Sender<Option<Vec<Weight>>>,
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
