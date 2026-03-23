use serde::{Deserialize, Serialize};

use rust_road_router::datastr::graph::Weight;

// ---------------------------------------------------------------------------
// Query API types (JSON)
// ---------------------------------------------------------------------------

/// Incoming query — supports both coordinate-based and node-ID-based queries.
/// The server detects the variant by which fields are present.
#[derive(Deserialize)]
pub struct QueryRequest {
    pub from_lat: Option<f32>,
    pub from_lng: Option<f32>,
    pub to_lat: Option<f32>,
    pub to_lng: Option<f32>,
    pub from_node: Option<u32>,
    pub to_node: Option<u32>,
    /// Response format: omit or "default" for the standard response,
    /// "geojson" for a GeoJSON Feature with LineString geometry.
    pub format: Option<String>,
}

/// Query result returned as JSON.
#[derive(Serialize)]
pub struct QueryResponse {
    pub distance_ms: Option<Weight>,
    pub distance_m: Option<f64>,
    pub path_nodes: Vec<u32>,
    pub coordinates: Vec<[f32; 2]>,
}

impl QueryResponse {
    pub fn empty() -> Self {
        QueryResponse {
            distance_ms: None,
            distance_m: None,
            path_nodes: vec![],
            coordinates: vec![],
        }
    }
}

// ---------------------------------------------------------------------------
// Customize API types
// ---------------------------------------------------------------------------

/// Response from the customize endpoint.
#[derive(Serialize)]
pub struct CustomizeResponse {
    pub accepted: bool,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Info API types
// ---------------------------------------------------------------------------

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
