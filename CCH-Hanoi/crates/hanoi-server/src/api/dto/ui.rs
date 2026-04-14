use serde::{Deserialize, Serialize};
use serde_json::Value;

use rust_road_router::datastr::graph::Weight;

#[derive(Deserialize)]
pub struct TrafficOverlayQuery {
    pub min_lat: f32,
    pub max_lat: f32,
    pub min_lng: f32,
    pub max_lng: f32,
    #[serde(default)]
    pub tertiary_and_above_only: bool,
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
