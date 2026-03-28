use serde::Deserialize;

/// URL query-string parameters for the gateway query endpoint.
/// `profile` selects the backend; `format` and `colors` are forwarded to the backend.
#[derive(Deserialize)]
pub struct GatewayQueryParam {
    /// Routing profile name (e.g. "car", "motorcycle"). Must match a profile
    /// key defined in the gateway YAML config.
    pub profile: String,
    pub format: Option<String>,
    pub colors: Option<String>,
}

/// Gateway info request — selects which backend to query by profile.
#[derive(Deserialize)]
pub struct InfoQuery {
    pub profile: Option<String>,
}
