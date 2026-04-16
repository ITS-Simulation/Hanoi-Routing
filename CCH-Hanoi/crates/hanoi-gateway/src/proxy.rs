use std::collections::HashMap;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use reqwest::Client;
use serde_json::Value;

use crate::config::ProfileConfig;
use crate::types::{GatewayQueryParam, InfoQuery, RequiredProfileQuery};

type ProxyResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

/// Shared gateway state — holds the HTTP client and per-profile backend config.
///
/// # API surface
///
/// | Method | Path | Description |
/// |--------|------------|---------------------------------------------------------|
/// | POST | `/query` | Route query — `?profile=<name>` selects the backend |
/// | POST | `/reset_weights` | Reset the selected backend to baseline weights |
/// | GET | `/info` | Backend metadata — `?profile=<name>` (optional) |
/// | GET | `/status` | Gateway alias for `/info` |
/// | GET | `/health` | Backend health metrics — `?profile=<name>` (optional) |
/// | GET | `/ready` | Backend readiness — `?profile=<name>` (optional) |
/// | GET | `/profiles`| List all available routing profiles |
///
/// ## Profile selection
///
/// Mutating and query endpoints such as `/query` and `/reset_weights`
/// **must** specify a `profile` query parameter that matches a key in the
/// gateway YAML config's `profiles` map. Status-style endpoints (`/info`,
/// `/status`, `/health`, `/ready`) also accept `profile`; when omitted they
/// default to the first configured profile alphabetically. Unknown profiles are
/// rejected with HTTP 400 and a list of valid options.
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
/// Query parameters supported by the backend are forwarded unchanged.
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

fn unknown_profile_response(state: &GatewayState, profile: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "error": format!("unknown profile: {profile}"),
            "available_profiles": state.available_profiles(),
        })),
    )
}

fn resolve_backend<'a>(
    state: &'a GatewayState,
    profile: &str,
) -> Result<&'a str, (StatusCode, Json<Value>)> {
    state
        .backend_url(profile)
        .ok_or_else(|| unknown_profile_response(state, profile))
}

fn default_profile(state: &GatewayState) -> String {
    state
        .available_profiles()
        .into_iter()
        .next()
        .unwrap_or("car")
        .to_string()
}

fn query_pairs(params: &GatewayQueryParam) -> Vec<(&'static str, String)> {
    let mut pairs = Vec::new();

    if let Some(ref format) = params.format {
        pairs.push(("format", format.clone()));
    }
    if let Some(ref colors) = params.colors {
        pairs.push(("colors", colors.clone()));
    }
    if let Some(alternatives) = params.alternatives {
        pairs.push(("alternatives", alternatives.to_string()));
    }
    if let Some(stretch) = params.stretch {
        pairs.push(("stretch", stretch.to_string()));
    }

    pairs
}

async fn send_backend_request(request: reqwest::RequestBuilder) -> ProxyResult {
    let resp = request.send().await.map_err(|e| {
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

async fn proxy_profile_get(state: GatewayState, params: InfoQuery, path: &str) -> ProxyResult {
    let profile = params.profile.unwrap_or_else(|| default_profile(&state));
    let backend = resolve_backend(&state, &profile)?;
    send_backend_request(state.client.get(format!("{backend}{path}"))).await
}

/// POST /query?profile=car — forward to the appropriate backend.
///
/// The `profile` query parameter selects the backend. The JSON body is forwarded
/// to the backend unchanged. Query parameters accepted by `hanoi-server`
/// (`format`, `colors`, `alternatives`, `stretch`) are also forwarded.
///
/// # Errors
///
/// - **400 Bad Request** — unknown or missing profile (response includes `available_profiles`)
/// - **502 Bad Gateway** — backend unreachable or returned invalid JSON
pub async fn handle_query(
    State(state): State<GatewayState>,
    Query(params): Query<GatewayQueryParam>,
    body: Bytes,
) -> ProxyResult {
    let backend = resolve_backend(&state, &params.profile)?;
    let mut request = state
        .client
        .post(format!("{backend}/query"))
        .header("content-type", "application/json");

    for (key, value) in query_pairs(&params) {
        request = request.query(&[(key, value)]);
    }

    send_backend_request(request.body(body)).await
}

/// POST /reset_weights?profile=car — reset the selected backend to baseline weights.
pub async fn handle_reset_weights(
    State(state): State<GatewayState>,
    Query(params): Query<RequiredProfileQuery>,
    body: Bytes,
) -> ProxyResult {
    let backend = resolve_backend(&state, &params.profile)?;
    let request = state.client.post(format!("{backend}/reset_weights"));
    let request = if body.is_empty() {
        request
    } else {
        request.body(body)
    };

    send_backend_request(request).await
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
) -> ProxyResult {
    proxy_profile_get(state, params, "/info").await
}

/// GET /health?profile=car — forward the selected backend's health response.
pub async fn handle_health(
    State(state): State<GatewayState>,
    Query(params): Query<InfoQuery>,
) -> ProxyResult {
    proxy_profile_get(state, params, "/health").await
}

/// GET /ready?profile=car — forward the selected backend's readiness response.
pub async fn handle_ready(
    State(state): State<GatewayState>,
    Query(params): Query<InfoQuery>,
) -> ProxyResult {
    proxy_profile_get(state, params, "/ready").await
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

#[cfg(test)]
mod tests {
    use super::query_pairs;
    use crate::types::GatewayQueryParam;

    #[test]
    fn query_pairs_forward_all_supported_query_controls() {
        let params = GatewayQueryParam {
            profile: "car".into(),
            format: Some("json".into()),
            colors: Some(String::new()),
            alternatives: Some(2),
            stretch: Some(1.5),
        };

        assert_eq!(
            query_pairs(&params),
            vec![
                ("format", "json".into()),
                ("colors", String::new()),
                ("alternatives", "2".into()),
                ("stretch", "1.5".into()),
            ]
        );
    }

    #[test]
    fn query_pairs_skip_profile_and_absent_controls() {
        let params = GatewayQueryParam {
            profile: "motorcycle".into(),
            format: None,
            colors: None,
            alternatives: None,
            stretch: None,
        };

        assert!(query_pairs(&params).is_empty());
    }
}
