use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use reqwest::Client;
use serde_json::Value;

use crate::types::{BackendQueryRequest, GatewayFormatParam, GatewayQueryRequest, InfoQuery};

/// Shared gateway state — holds HTTP clients and backend URLs.
#[derive(Clone)]
pub struct GatewayState {
    client: Client,
    normal_url: String,
    line_graph_url: String,
}

impl GatewayState {
    pub fn new(normal_url: &str, line_graph_url: &str, timeout_secs: u64) -> Self {
        let client = if timeout_secs > 0 {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(timeout_secs))
                .build()
                .expect("failed to build reqwest client")
        } else {
            Client::new()
        };

        GatewayState {
            client,
            normal_url: normal_url.trim_end_matches('/').to_string(),
            line_graph_url: line_graph_url.trim_end_matches('/').to_string(),
        }
    }

    fn backend_url(&self, graph_type: &str) -> Option<&str> {
        match graph_type {
            "normal" => Some(&self.normal_url),
            "line_graph" => Some(&self.line_graph_url),
            _ => None,
        }
    }
}

/// POST /query — forward to the appropriate backend based on graph_type.
/// Response format is controlled by the `format` query parameter (forwarded to backend).
pub async fn handle_query(
    State(state): State<GatewayState>,
    Query(params): Query<GatewayFormatParam>,
    Json(req): Json<GatewayQueryRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let backend = state.backend_url(&req.graph_type).ok_or((
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": format!("unknown graph_type: {}", req.graph_type)})),
    ))?;

    let backend_req = BackendQueryRequest {
        from_lat: req.from_lat,
        from_lng: req.from_lng,
        to_lat: req.to_lat,
        to_lng: req.to_lng,
        from_node: req.from_node,
        to_node: req.to_node,
    };

    // Forward format as a query parameter to the backend
    let url = match params.format.as_deref() {
        Some(fmt) => format!("{}/query?format={fmt}", backend),
        None => format!("{}/query", backend),
    };

    let resp = state
        .client
        .post(&url)
        .json(&backend_req)
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("backend error: {e}")})),
            )
        })?;

    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("invalid backend response: {e}")})),
        )
    })?;

    if status.is_client_error() || status.is_server_error() {
        Err((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(body),
        ))
    } else {
        Ok(Json(body))
    }
}

/// GET /info?graph_type=normal — forward to the appropriate backend.
pub async fn handle_info(
    State(state): State<GatewayState>,
    Query(params): Query<InfoQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let graph_type = params.graph_type.as_deref().unwrap_or("normal");

    let backend = state.backend_url(graph_type).ok_or((
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": format!("unknown graph_type: {}", graph_type)})),
    ))?;

    let resp = state
        .client
        .get(format!("{}/info", backend))
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("backend error: {e}")})),
            )
        })?;

    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("invalid backend response: {e}")})),
        )
    })?;

    if status.is_client_error() || status.is_server_error() {
        Err((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(body),
        ))
    } else {
        Ok(Json(body))
    }
}
