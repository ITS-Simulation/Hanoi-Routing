use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde_json::Value;

use crate::api::dto::{
    CameraOverlayQuery, CameraOverlayResponse, EvaluateRoutesRequest, EvaluateRoutesResponse,
    TrafficOverlayQuery, TrafficOverlayResponse,
};
use crate::api::state::AppState;
use crate::ui::route_eval::MAX_ROUTE_EVALUATIONS;

/// POST /evaluate_routes — evaluate one or more imported GeoJSON routes using
/// the currently active server weight profile.
pub async fn handle_evaluate_routes(
    State(state): State<AppState>,
    Json(req): Json<EvaluateRoutesRequest>,
) -> Result<Json<EvaluateRoutesResponse>, (StatusCode, Json<Value>)> {
    if req.routes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "no_routes",
                "message": "At least one GeoJSON route must be provided for evaluation.",
            })),
        ));
    }

    if req.routes.len() > MAX_ROUTE_EVALUATIONS {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "too_many_routes",
                "message": format!(
                    "A maximum of {} routes can be compared at once.",
                    MAX_ROUTE_EVALUATIONS
                ),
            })),
        ));
    }

    let latest_weights = state
        .latest_weights
        .read()
        .expect("latest_weights lock poisoned");
    let using_customized_weights = latest_weights.is_some();
    let effective_weights = latest_weights
        .as_deref()
        .unwrap_or_else(|| state.baseline_weights.as_ref().as_ref());
    let routes = state
        .route_evaluator
        .evaluate_routes(&req.routes, effective_weights);

    Ok(Json(EvaluateRoutesResponse {
        using_customized_weights,
        graph_type: state.route_evaluator.graph_type(),
        routes,
    }))
}

/// GET /traffic_overlay — viewport-filtered traffic segments relative to the
/// baseline `travel_time` metric. In line-graph mode this is projected back to
/// original arcs as a pseudo-normal road overlay.
pub async fn handle_traffic_overlay(
    State(state): State<AppState>,
    Query(query): Query<TrafficOverlayQuery>,
) -> Result<Json<TrafficOverlayResponse>, (StatusCode, Json<Value>)> {
    if query.min_lat > query.max_lat || query.min_lng > query.max_lng {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_bbox",
                "message": "traffic overlay bbox is invalid: min values must be <= max values",
            })),
        ));
    }

    let latest_weights = state
        .latest_weights
        .read()
        .expect("latest_weights lock poisoned");
    let using_customized_weights = latest_weights.is_some();
    let response =
        state
            .traffic_overlay
            .render(&query, latest_weights.as_deref(), using_customized_weights);
    Ok(Json(response))
}

/// GET /camera_overlay — viewport-filtered camera markers.
pub async fn handle_camera_overlay(
    State(state): State<AppState>,
    Query(query): Query<CameraOverlayQuery>,
) -> Result<Json<CameraOverlayResponse>, (StatusCode, Json<Value>)> {
    if query.min_lat > query.max_lat || query.min_lng > query.max_lng {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_bbox",
                "message": "camera overlay bbox is invalid: min values must be <= max values",
            })),
        ));
    }

    Ok(Json(state.camera_overlay.render(&query)))
}
