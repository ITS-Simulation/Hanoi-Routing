use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde_json::Value;
use std::sync::atomic::Ordering;

use rust_road_router::datastr::graph::INFINITY;

use crate::state::{AppState, QueryMsg};
use crate::types::{
    CustomizeResponse, FormatParam, HealthResponse, InfoResponse, QueryRequest, QueryResponse, ReadyResponse,
};

/// POST /query — route query (coordinate-based or node-ID-based).
/// Response format is controlled by the `format` query parameter:
/// omit → GeoJSON Feature (default), `?format=json` → plain JSON.
pub async fn handle_query(
    State(state): State<AppState>,
    Query(params): Query<FormatParam>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let msg = QueryMsg {
        request: req,
        format: params.format,
        reply: tx,
    };

    if state.query_tx.send(msg).await.is_err() {
        return Ok(Json(serde_json::to_value(QueryResponse::empty()).unwrap()));
    }

    match rx.await {
        Ok(Ok(resp)) => {
            state.queries_processed.fetch_add(1, Ordering::Relaxed);
            Ok(Json(resp))
        }
        Ok(Err(rejection)) => {
            tracing::warn!(%rejection, "coordinate validation failed");
            Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "coordinate_validation_failed",
                    "message": rejection.to_string(),
                    "details": rejection.to_details_json(),
                })),
            ))
        }
        Err(_) => Ok(Json(serde_json::to_value(QueryResponse::empty()).unwrap())),
    }
}

/// POST /customize — accept raw binary weight vector.
/// Body: little-endian [u32; num_edges], optionally gzip-compressed.
pub async fn handle_customize(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<CustomizeResponse>, (StatusCode, Json<CustomizeResponse>)> {
    tracing::info!(
        body_bytes = body.len(),
        expected_edges = state.num_edges,
        "customize request received"
    );
    let expected = state.num_edges * 4;
    if body.len() != expected {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(CustomizeResponse {
                accepted: false,
                message: format!(
                    "expected {} bytes ({} edges x 4), got {}",
                    expected,
                    state.num_edges,
                    body.len()
                ),
            }),
        ));
    }
    // Copy bytes into a properly aligned Vec<u32>.
    // bytemuck::cast_slice requires 4-byte alignment which Bytes doesn't guarantee.
    let mut weights = vec![0u32; state.num_edges];
    bytemuck::cast_slice_mut::<u32, u8>(&mut weights).copy_from_slice(&body);

    // Reject weights >= INFINITY — CCH triangle relaxation uses plain addition,
    // so INFINITY + finite would produce a large finite value instead of
    // remaining INFINITY, leading to semantically incorrect shortest paths.
    if let Some(pos) = weights.iter().position(|&w| w >= INFINITY) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(CustomizeResponse {
                accepted: false,
                message: format!(
                    "weight[{}] = {} exceeds maximum allowed value ({})",
                    pos, weights[pos], INFINITY - 1
                ),
            }),
        ));
    }

    let _ = state.watch_tx.send(Some(weights));
    tracing::info!("customization weights accepted, queued for engine thread");
    Ok(Json(CustomizeResponse {
        accepted: true,
        message: "customization queued".into(),
    }))
}

/// GET /info — graph metadata and server status.
pub async fn handle_info(State(state): State<AppState>) -> Json<InfoResponse> {
    Json(InfoResponse {
        graph_type: if state.is_line_graph {
            "line_graph".into()
        } else {
            "normal".into()
        },
        num_nodes: state.num_nodes,
        num_edges: state.num_edges,
        customization_active: state.is_customization_active(),
        bbox: state.bbox.clone(),
    })
}

/// GET /health — operational metrics. Always 200 while the process is running.
pub async fn handle_health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        uptime_seconds: state.uptime_secs(),
        total_queries_processed: state.total_queries(),
        customization_active: state.is_customization_active(),
    })
}

/// GET /ready — readiness check. Returns 503 if the engine thread has died.
pub async fn handle_ready(
    State(state): State<AppState>,
) -> Result<Json<ReadyResponse>, (StatusCode, Json<ReadyResponse>)> {
    if state.is_engine_alive() {
        Ok(Json(ReadyResponse { ready: true }))
    } else {
        Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse { ready: false }),
        ))
    }
}
