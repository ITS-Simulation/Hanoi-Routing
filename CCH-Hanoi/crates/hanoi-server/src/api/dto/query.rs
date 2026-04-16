use hanoi_core::TurnAnnotation;
use serde::{Deserialize, Serialize};

use rust_road_router::datastr::graph::Weight;

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
    pub alternatives: Option<u32>,
    pub stretch: Option<f64>,
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
