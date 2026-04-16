use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Clone, Default, ValueEnum)]
pub enum LogFormat {
    /// Multi-line, colorized, with source locations (most readable)
    #[default]
    Pretty,
    /// Single-line with inline span context
    Full,
    /// Abbreviated single-line
    Compact,
    /// Indented tree hierarchy
    Tree,
    /// Newline-delimited JSON for log aggregation
    Json,
}

#[derive(Parser)]
#[command(
    name = "hanoi_server",
    about = "CCH routing server for Hanoi road network"
)]
pub struct Args {
    /// Path to the graph directory (e.g. Maps/data/hanoi_car/graph)
    #[arg(long)]
    pub graph_dir: PathBuf,

    /// Path to the original graph directory (required for --line-graph mode)
    #[arg(long)]
    pub original_graph_dir: Option<PathBuf>,

    /// Camera YAML file used by the optional camera overlay.
    #[cfg(feature = "ui")]
    #[arg(long, default_value = "CCH_Data_Pipeline/config/mvp_camera.yaml")]
    pub camera_config: PathBuf,

    /// Port for the query API (REST/JSON)
    #[arg(long, default_value = "8080")]
    pub query_port: u16,

    /// Port for the customization API (REST/binary)
    #[arg(long, default_value = "9080")]
    pub customize_port: u16,

    /// Serve the bundled route-viewer UI on the query port.
    /// When omitted, the server exposes only the API endpoints.
    #[cfg(feature = "ui")]
    #[arg(long)]
    pub serve_ui: bool,

    /// Enable line-graph mode (uses DirectedCCH, final-edge correction)
    #[arg(long)]
    pub line_graph: bool,

    /// Log output format
    #[arg(long, value_enum, default_value_t = LogFormat::Pretty)]
    pub log_format: LogFormat,

    /// Directory for persistent log files (daily rotation, JSON format).
    /// Omit to log to stderr only.
    #[arg(long)]
    pub log_dir: Option<PathBuf>,
}
