use serde::Serialize;

/// Response from the customize endpoint.
#[derive(Serialize)]
pub struct CustomizeResponse {
    pub accepted: bool,
    pub message: String,
}
