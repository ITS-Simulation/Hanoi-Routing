use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;

use rust_road_router::datastr::graph::INFINITY;

use crate::api::dto::CustomizeResponse;
use crate::api::state::AppState;

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
        .send(Some(state.baseline_weights.as_ref().to_vec()))
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
