use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde_json::Value;
use std::sync::atomic::Ordering;

use crate::api::dto::{FormatParam, QueryRequest, QueryResponse};
use crate::api::state::{AppState, QueryMsg};

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
        colors: params.colors.is_some(),
        alternatives: params.alternatives.unwrap_or(0),
        stretch: params
            .stretch
            .unwrap_or(hanoi_core::routing::alternatives::DEFAULT_STRETCH),
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
