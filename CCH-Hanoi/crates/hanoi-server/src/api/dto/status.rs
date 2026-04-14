use serde::Serialize;

/// Server metadata returned by GET /info.
#[derive(Serialize, Clone)]
pub struct BboxInfo {
    pub min_lat: f32,
    pub max_lat: f32,
    pub min_lng: f32,
    pub max_lng: f32,
}

/// Server metadata returned by GET /info.
#[derive(Serialize)]
pub struct InfoResponse {
    pub graph_type: String,
    pub num_nodes: usize,
    pub num_edges: usize,
    pub customization_active: bool,
    pub bbox: Option<BboxInfo>,
}

/// Response from GET /health — operational metrics.
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub uptime_seconds: u64,
    pub total_queries_processed: u64,
    pub customization_active: bool,
}

/// Response from GET /ready — 503 if the engine thread has died.
#[derive(Serialize)]
pub struct ReadyResponse {
    pub ready: bool,
}
