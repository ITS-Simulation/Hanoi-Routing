use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde_json::Value;
use std::sync::atomic::Ordering;

use rust_road_router::datastr::graph::INFINITY;

use crate::route_eval::MAX_ROUTE_EVALUATIONS;
use crate::state::{AppState, QueryMsg};
use crate::types::{
    CameraOverlayQuery, CameraOverlayResponse, CustomizeResponse, EvaluateRoutesRequest,
    EvaluateRoutesResponse, FormatParam, HealthResponse, InfoResponse, QueryRequest, QueryResponse,
    ReadyResponse, TrafficOverlayQuery, TrafficOverlayResponse,
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
        colors: params.colors.is_some(),
        alternatives: params.alternatives.unwrap_or(0),
        stretch: params.stretch.unwrap_or(hanoi_core::multi_route::DEFAULT_STRETCH),
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
        .unwrap_or(state.baseline_weights.as_ref());
    let routes = state
        .route_evaluator
        .evaluate_routes(&req.routes, effective_weights);

    Ok(Json(EvaluateRoutesResponse {
        using_customized_weights,
        graph_type: state.route_evaluator.graph_type(),
        routes,
    }))
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

    // Reject weights > INFINITY. INFINITY itself is allowed and means "road closed":
    // CCH triangle relaxation computes upward_weight + first_down_weight, and since
    // both operands are <= INFINITY, their sum is <= INFINITY + INFINITY = u32::MAX - 1,
    // which does not overflow u32. Any triangle involving an INFINITY leg produces a sum
    // >= INFINITY, so it never beats an existing finite shortcut weight — the closed edge
    // correctly stays unreachable throughout the hierarchy.
    if let Some(pos) = weights.iter().position(|&w| w > INFINITY) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(CustomizeResponse {
                accepted: false,
                message: format!(
                    "weight[{}] = {} exceeds maximum allowed value ({})",
                    pos, weights[pos], INFINITY
                ),
            }),
        ));
    }

    {
        let mut latest_weights = state
            .latest_weights
            .write()
            .expect("latest_weights lock poisoned");
        *latest_weights = Some(weights.clone());
    }

    let _ = state.watch_tx.send(Some(weights));
    tracing::info!("customization weights accepted, queued for engine thread");
    Ok(Json(CustomizeResponse {
        accepted: true,
        message: "customization queued".into(),
    }))
}

/// POST /reset_weights — restore the server's baseline metric.
pub async fn handle_reset_weights(
    State(state): State<AppState>,
) -> Result<Json<CustomizeResponse>, (StatusCode, Json<CustomizeResponse>)> {
    {
        let mut latest_weights = state
            .latest_weights
            .write()
            .expect("latest_weights lock poisoned");
        *latest_weights = None;
    }

    state
        .watch_tx
        .send(Some((*state.baseline_weights).clone()))
        .map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(CustomizeResponse {
                    accepted: false,
                    message: "engine is not available to accept a baseline reset".into(),
                }),
            )
        })?;

    tracing::info!("baseline weights queued");
    Ok(Json(CustomizeResponse {
        accepted: true,
        message: "baseline weights queued".into(),
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

/// GET /camera_overlay — viewport-filtered camera markers loaded from the
/// configured camera YAML file.
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
