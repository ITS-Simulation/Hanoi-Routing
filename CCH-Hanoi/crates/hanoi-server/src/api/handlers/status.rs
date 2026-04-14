use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::api::dto::{HealthResponse, InfoResponse, ReadyResponse};
use crate::api::state::AppState;

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
