use hanoi_core::TurnAnnotation;
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
}

/// URL query-string parameters controlling response format.
///
/// - `format`: omit → GeoJSON (default); `format=json` → plain JSON.
/// - `colors`: presence (`?colors`) adds simplestyle-spec properties to GeoJSON.
///   Ignored when `format=json`.
#[derive(Deserialize)]
pub struct FormatParam {
    pub format: Option<String>,
    /// When present (any value or empty), adds simplestyle-spec visualization
    /// properties (stroke, stroke-width, fill, fill-opacity) to GeoJSON output.
    pub colors: Option<String>,
    /// Number of alternative routes to return (0 or absent = single best route).
    pub alternatives: Option<u32>,
    /// Maximum stretch factor for alternative routes (e.g. 1.3 = 30% longer).
    /// Defaults to 1.3 if omitted.
    pub stretch: Option<f64>,
}

/// Query result returned as JSON.
#[derive(Serialize)]
pub struct QueryResponse {
    pub distance_ms: Option<Weight>,
    pub distance_m: Option<f64>,
    pub path_nodes: Vec<u32>,
    pub coordinates: Vec<[f32; 2]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub turns: Vec<TurnAnnotation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<[f32; 2]>,
}

impl QueryResponse {
    pub fn empty() -> Self {
        QueryResponse {
            distance_ms: None,
            distance_m: None,
            path_nodes: vec![],
            coordinates: vec![],
            turns: vec![],
            origin: None,
            destination: None,
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
