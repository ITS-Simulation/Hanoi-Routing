use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// YAML config schema
// ---------------------------------------------------------------------------

/// Top-level gateway configuration loaded from a YAML file.
///
/// # Format
///
/// ```yaml
/// # Gateway listen port (required)
/// port: 50051
///
/// # HTTP client settings for backend requests (optional section)
/// backend_timeout_secs: 30     # 0 to disable; default 30
///
/// # Logging (optional section)
/// log_format: pretty           # pretty | full | compact | tree | json
/// log_file: /var/log/gw.json   # omit to disable file logging
///
/// # Routing profiles — at least one required.
/// # Each key is the profile name used in API requests.
/// profiles:
///   car:
///     backend_url: "http://localhost:8080"
///   motorcycle:
///     backend_url: "http://localhost:8081"
/// ```
///
/// The `profiles` map is the single source of truth for which routing profiles
/// the gateway exposes. A request with an unknown profile is rejected with
/// HTTP 400.
#[derive(Deserialize)]
pub struct GatewayConfig {
    /// Gateway listen port.
    pub port: u16,

    /// Backend request timeout in seconds. `0` disables the timeout.
    /// Defaults to 30 if omitted.
    #[serde(default = "default_timeout")]
    pub backend_timeout_secs: u64,

    /// Log output format. Defaults to `pretty` if omitted.
    #[serde(default)]
    pub log_format: LogFormat,

    /// Optional path to also write logs in JSON format.
    pub log_file: Option<String>,

    /// Map of profile name → backend configuration. At least one entry required.
    pub profiles: HashMap<String, ProfileConfig>,
}

/// Backend configuration for a single routing profile.
#[derive(Deserialize, Clone)]
pub struct ProfileConfig {
    /// Base URL of the backend routing server (e.g. `http://localhost:8080`).
    /// Trailing slashes are stripped at load time.
    pub backend_url: String,
}

/// Log output format (mirrors the old CLI enum).
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    /// Multi-line, colorized, with source locations (most readable)
    #[default]
    Pretty,
    /// Single-line with inline span context
    Full,
    /// Abbreviated single-line
    Compact,
    /// Indented tree hierarchy (falls back to full)
    Tree,
    /// Newline-delimited JSON for log aggregation
    Json,
}

fn default_timeout() -> u64 {
    30
}

// ---------------------------------------------------------------------------
// Loading & validation
// ---------------------------------------------------------------------------

impl GatewayConfig {
    /// Load configuration from a YAML file, validate, and normalize.
    pub fn load(path: &Path) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read config file '{}': {}", path.display(), e))?;

        let mut config: GatewayConfig = serde_yaml::from_str(&contents)
            .map_err(|e| format!("invalid YAML in '{}': {}", path.display(), e))?;

        if config.profiles.is_empty() {
            return Err(format!(
                "config '{}': `profiles` must contain at least one entry",
                path.display()
            ));
        }

        // Normalize: strip trailing slashes from all backend URLs
        for profile in config.profiles.values_mut() {
            profile.backend_url = profile.backend_url.trim_end_matches('/').to_string();
        }

        Ok(config)
    }
}
