use hanoi_core::TurnAnnotation;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
}

#[derive(Deserialize)]
pub struct TrafficOverlayQuery {
    pub min_lat: f32,
    pub max_lat: f32,
    pub min_lng: f32,
    pub max_lng: f32,
    #[serde(default)]
    pub tertiary_and_above_only: bool,
}

/// Query result returned as JSON.
#[derive(Serialize)]
pub struct QueryResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_type: Option<&'static str>,
    pub distance_ms: Option<Weight>,
    pub distance_m: Option<f64>,
    pub path_nodes: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub route_arc_ids: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub weight_path_ids: Vec<u32>,
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
            graph_type: None,
            distance_ms: None,
            distance_m: None,
            path_nodes: vec![],
            route_arc_ids: vec![],
            weight_path_ids: vec![],
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

#[derive(Serialize)]
pub struct TrafficOverlayBucket {
    pub status: &'static str,
    pub color: &'static str,
    pub segments: Vec<[[f32; 2]; 2]>,
}

#[derive(Serialize)]
pub struct TrafficOverlayResponse {
    pub using_customized_weights: bool,
    pub mapping_mode: &'static str,
    pub tertiary_filter_supported: bool,
    pub tertiary_and_above_only: bool,
    pub visible_segment_count: usize,
    pub buckets: Vec<TrafficOverlayBucket>,
}

#[derive(Deserialize)]
pub struct CameraOverlayQuery {
    pub min_lat: f32,
    pub max_lat: f32,
    pub min_lng: f32,
    pub max_lng: f32,
}

#[derive(Serialize)]
pub struct CameraOverlayCamera {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arc_id: Option<u32>,
    pub lat: f32,
    pub lng: f32,
}

#[derive(Serialize)]
pub struct CameraOverlayResponse {
    pub available: bool,
    pub visible_camera_count: usize,
    pub total_camera_count: usize,
    pub cameras: Vec<CameraOverlayCamera>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Deserialize)]
pub struct EvaluateRoutesRequest {
    pub routes: Vec<EvaluateRouteInput>,
}

#[derive(Deserialize)]
pub struct EvaluateRouteInput {
    pub name: String,
    pub geojson: Value,
}

#[derive(Serialize)]
pub struct EvaluateRoutesResponse {
    pub using_customized_weights: bool,
    pub graph_type: &'static str,
    pub routes: Vec<RouteEvaluationResult>,
}

#[derive(Serialize)]
pub struct RouteEvaluationResult {
    pub name: String,
    pub travel_time_ms: Option<Weight>,
    pub distance_m: Option<f64>,
    pub geometry_point_count: usize,
    pub route_arc_count: usize,
    pub travel_time_mode: &'static str,
    pub distance_mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export_graph_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
