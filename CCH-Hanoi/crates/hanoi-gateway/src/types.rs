use serde::Deserialize;

/// URL query-string parameters for the gateway query endpoint.
/// `profile` selects the backend; the remaining fields are forwarded to the backend.
#[derive(Deserialize)]
pub struct GatewayQueryParam {
    /// Routing profile name (e.g. "car", "motorcycle"). Must match a profile
    /// key defined in the gateway YAML config.
    pub profile: String,
    pub format: Option<String>,
    pub colors: Option<String>,
    pub alternatives: Option<u32>,
    pub stretch: Option<f64>,
}

/// Optional-profile gateway request for status-style endpoints.
#[derive(Deserialize)]
pub struct InfoQuery {
    pub profile: Option<String>,
}

/// Required-profile gateway request for backend operations.
#[derive(Deserialize)]
pub struct RequiredProfileQuery {
    pub profile: String,
}
