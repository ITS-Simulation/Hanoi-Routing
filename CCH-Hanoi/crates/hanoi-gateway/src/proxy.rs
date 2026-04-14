use std::collections::HashMap;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use reqwest::Client;
use serde_json::Value;

use crate::config::ProfileConfig;
use crate::types::{GatewayQueryParam, InfoQuery};

/// Shared gateway state — holds the HTTP client and per-profile backend config.
///
/// # API surface
///
/// | Method | Path | Description |
/// |--------|------------|---------------------------------------------------------|
/// | POST | `/query` | Route query — `?profile=<name>` selects the backend |
/// | GET | `/info` | Backend metadata — `?profile=<name>` (optional) |
/// | GET | `/profiles`| List all available routing profiles |
///
/// ## Profile selection
///
/// Every query **must** specify a `profile` query parameter that matches a key
/// in the gateway YAML config's `profiles` map. Unknown profiles are rejected
/// with HTTP 400 and a list of valid options.
///
/// ## Request format
///
/// ```text
/// POST /query?profile=car
/// Content-Type: application/json
///
/// {
///   "from_lat": 21.028,  "from_lng": 105.854,
///   "to_lat":   21.007,  "to_lng":   105.820
/// }
/// ```
///
/// The JSON body is forwarded to the backend unchanged.
/// Query parameters `format` and `colors` are forwarded unchanged.
#[derive(Clone)]
pub struct GatewayState {
    client: Client,
    /// Profile name → backend configuration. Populated from the YAML config.
    profiles: HashMap<String, ProfileConfig>,
}

impl GatewayState {
    pub fn new(profiles: HashMap<String, ProfileConfig>, timeout_secs: u64) -> Self {
        let client = if timeout_secs > 0 {
            Client::builder()
                .timeout(std::time::Duration::from_secs(timeout_secs))
                .build()
                .expect("failed to build reqwest client")
        } else {
            Client::new()
        };

        GatewayState { client, profiles }
    }

    fn backend_url(&self, profile: &str) -> Option<&str> {
        self.profiles.get(profile).map(|p| p.backend_url.as_str())
    }

    /// Return the sorted list of available profile names (for error messages
    /// and the /profiles endpoint).
    fn available_profiles(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.profiles.keys().map(|s| s.as_str()).collect();
        names.sort_unstable();
        names
    }
}

/// POST /query?profile=car — forward to the appropriate backend.
///
/// The `profile` query parameter selects the backend. The JSON body is forwarded
/// to the backend unchanged. Query parameters `format` and `colors` are also
/// forwarded to the backend.
///
/// # Errors
///
/// - **400 Bad Request** — unknown or missing profile (response includes `available_profiles`)
/// - **502 Bad Gateway** — backend unreachable or returned invalid JSON
pub async fn handle_query(
    State(state): State<GatewayState>,
    Query(params): Query<GatewayQueryParam>,
    body: Bytes,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let backend = state.backend_url(&params.profile).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("unknown profile: {}", params.profile),
                "available_profiles": state.available_profiles(),
            })),
        )
    })?;

    // Forward format and colors as query parameters to the backend
    let mut query_parts: Vec<String> = Vec::new();
    if let Some(ref fmt) = params.format {
        query_parts.push(format!("format={fmt}"));
    }
    if params.colors.is_some() {
        query_parts.push("colors".into());
    }
    let url = if query_parts.is_empty() {
        format!("{}/query", backend)
    } else {
        format!("{}/query?{}", backend, query_parts.join("&"))
    };

    let resp = state
        .client
        .post(&url)
        .header("content-type", "application/json")
        .body(body)
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

/// GET /info?profile=car — forward to the appropriate backend.
///
/// When `profile` is omitted, defaults to the first profile alphabetically.
///
/// # Errors
///
/// - **400 Bad Request** — unknown profile
/// - **502 Bad Gateway** — backend unreachable or returned invalid JSON
pub async fn handle_info(
    State(state): State<GatewayState>,
    Query(params): Query<InfoQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let default_profile;
    let profile = match params.profile.as_deref() {
        Some(p) => p,
        None => {
            default_profile = state
                .available_profiles()
                .into_iter()
                .next()
                .unwrap_or("car")
                .to_string();
            &default_profile
        }
    };

    let backend = state.backend_url(profile).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("unknown profile: {}", profile),
                "available_profiles": state.available_profiles(),
            })),
        )
    })?;

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

/// GET /profiles — list all available routing profiles.
///
/// Returns a JSON object with the sorted profile names. Useful for client
/// discovery (e.g., populating a dropdown).
///
/// ```json
/// { "profiles": ["car", "motorcycle"] }
/// ```
pub async fn handle_profiles(State(state): State<GatewayState>) -> Json<Value> {
    Json(serde_json::json!({
        "profiles": state.available_profiles(),
    }))
}
