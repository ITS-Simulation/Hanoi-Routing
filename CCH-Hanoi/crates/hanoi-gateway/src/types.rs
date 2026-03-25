use serde::{Deserialize, Serialize};

/// Incoming gateway query — extends the backend QueryRequest with graph_type routing.
#[derive(Deserialize, Serialize)]
pub struct GatewayQueryRequest {
    /// Which graph to route on: "normal" or "line_graph"
    pub graph_type: String,

    // Coordinate-based
    pub from_lat: Option<f32>,
    pub from_lng: Option<f32>,
    pub to_lat: Option<f32>,
    pub to_lng: Option<f32>,

    // Node-ID-based
    pub from_node: Option<u32>,
    pub to_node: Option<u32>,
}

/// URL query-string parameters for the gateway query endpoint.
/// `format` is forwarded as a query param to the backend.
#[derive(Deserialize)]
pub struct GatewayFormatParam {
    pub format: Option<String>,
}

/// Backend query request (forwarded to the routing server — sans graph_type).
#[derive(Serialize)]
pub struct BackendQueryRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_lat: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_lng: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_lat: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_lng: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_node: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_node: Option<u32>,
}

/// Gateway info request — selects which backend to query.
#[derive(Deserialize)]
pub struct InfoQuery {
    pub graph_type: Option<String>,
}
