use hanoi_core::restrictions::via_way::{apply_node_splits, load_via_way_chains};
use rust_road_router::{datastr::graph::*, io::*};
use std::{
    error::Error,
    fs,
    io::{Error as IoError, ErrorKind},
    path::{Path, PathBuf},
};

use clap::{Parser, ValueEnum};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

// ---------------------------------------------------------------------------
// CLI arguments
// ---------------------------------------------------------------------------

#[derive(Clone, Default, ValueEnum)]
enum LogFormat {
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

#[derive(Parser)]
#[command(
    name = "generate_line_graph",
    about = "Generate a line graph from a road network graph"
)]
struct Args {
    /// Input graph directory
    graph_dir: PathBuf,

    /// Output directory (defaults to <graph_dir>/line_graph)
    output_dir: Option<PathBuf>,

    /// Log output format
    #[arg(long, value_enum, default_value_t = LogFormat::Pretty)]
    log_format: LogFormat,
}

// ---------------------------------------------------------------------------
// Tracing initialization
// ---------------------------------------------------------------------------

fn init_tracing(log_format: &LogFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    match log_format {
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json())
                .init();
        }
        LogFormat::Pretty => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().pretty())
                .init();
        }
        LogFormat::Compact => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().compact())
                .init();
        }
        LogFormat::Full | LogFormat::Tree => {
            // Tree not available in tools (no tracing-tree dep); fall back to full
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_target(true))
                .init();
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_graph_dir(input_dir: &Path) -> PathBuf {
    if input_dir.join("first_out").exists() {
        return input_dir.to_path_buf();
    }
    let nested_graph_dir = input_dir.join("graph");
    if nested_graph_dir.join("first_out").exists() {
        return nested_graph_dir;
    }
    input_dir.to_path_buf()
}

fn validate_turn_arrays(
    forbidden_from: &[EdgeId],
    forbidden_to: &[EdgeId],
    arc_count: usize,
) -> Result<(), Box<dyn Error>> {
    if forbidden_from.len() != forbidden_to.len() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            format!(
                "forbidden_turn_from_arc length ({}) does not match forbidden_turn_to_arc length ({})",
                forbidden_from.len(),
                forbidden_to.len()
            ),
        )
        .into());
    }

    for idx in 0..forbidden_from.len() {
        if forbidden_from[idx] as usize >= arc_count || forbidden_to[idx] as usize >= arc_count {
            return Err(IoError::new(
                ErrorKind::InvalidData,
                format!(
                    "forbidden turn pair at index {idx} is out of bounds: ({}, {}) with arc_count {arc_count}",
                    forbidden_from[idx], forbidden_to[idx]
                ),
            )
            .into());
        }
    }

    for idx in 1..forbidden_from.len() {
        let prev = (forbidden_from[idx - 1], forbidden_to[idx - 1]);
        let current = (forbidden_from[idx], forbidden_to[idx]);
        if current < prev {
            return Err(IoError::new(
                ErrorKind::InvalidData,
                format!(
                    "forbidden turns must be sorted lexicographically by (from_arc, to_arc); \
                     first inversion at index {idx}: ({}, {}) then ({}, {})",
                    prev.0, prev.1, current.0, current.1
                ),
            )
            .into());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    init_tracing(&args.log_format);

    let graph_path = resolve_graph_dir(&args.graph_dir);
    let output_dir = args.output_dir.unwrap_or_else(|| {
        if graph_path != args.graph_dir
            && graph_path.file_name().and_then(|name| name.to_str()) == Some("graph")
        {
            args.graph_dir.join("line_graph")
        } else {
            graph_path.join("line_graph")
        }
    });

    fs::create_dir_all(&output_dir)?;

    let graph = WeightedGraphReconstructor("travel_time").reconstruct_from(&graph_path)?;
    let lat = Vec::<f32>::load_from(graph_path.join("latitude"))?;
    let lng = Vec::<f32>::load_from(graph_path.join("longitude"))?;
    let is_arc_roundabout: Vec<u8> = Vec::load_from(graph_path.join("is_arc_roundabout"))?;

    if lat.len() != graph.num_nodes() || lng.len() != graph.num_nodes() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            format!(
                "coordinate vector lengths must match node_count {}; got latitude={}, longitude={}",
                graph.num_nodes(),
                lat.len(),
                lng.len()
            ),
        )
        .into());
    }
    assert_eq!(is_arc_roundabout.len(), graph.num_arcs());

    let forbidden_from = Vec::<EdgeId>::load_from(graph_path.join("forbidden_turn_from_arc"))?;
    let forbidden_to = Vec::<EdgeId>::load_from(graph_path.join("forbidden_turn_to_arc"))?;
    validate_turn_arrays(&forbidden_from, &forbidden_to, graph.num_arcs())?;

    tracing::info!(
        num_nodes = graph.num_nodes(),
        num_arcs = graph.num_arcs(),
        forbidden_turns = forbidden_from.len(),
        "original graph loaded"
    );

    let mut tail = Vec::with_capacity(graph.num_arcs());
    for node in 0..graph.num_nodes() {
        for _ in 0..graph.degree(node as NodeId) {
            tail.push(node as NodeId);
        }
    }
    if tail.len() != graph.num_arcs() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            format!(
                "tail array length {} does not match arc_count {}",
                tail.len(),
                graph.num_arcs()
            ),
        )
        .into());
    }

    let mut iter = forbidden_from.iter().zip(forbidden_to.iter()).peekable();
    let exp_graph = line_graph(&graph, |edge1_idx, edge2_idx| {
        while let Some(&(&from_arc, &to_arc)) = iter.peek() {
            if from_arc < edge1_idx || (from_arc == edge1_idx && to_arc < edge2_idx) {
                iter.next();
            } else {
                break;
            }
        }
        if iter.peek() == Some(&(&edge1_idx, &edge2_idx)) {
            return None;
        }
        if tail[edge1_idx as usize] == graph.head()[edge2_idx as usize] {
            return Some(20_000); // U-turn penalty: 20 seconds (in milliseconds)
        }
        Some(0)
    });

    if exp_graph.num_nodes() != graph.num_arcs() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            format!(
                "line graph node_count {} does not match original arc_count {}",
                exp_graph.num_nodes(),
                graph.num_arcs()
            ),
        )
        .into());
    }

    let base_lg_nodes = exp_graph.num_nodes();
    let avg_degree = exp_graph.num_arcs() as f64 / base_lg_nodes.max(1) as f64;
    tracing::info!(
        num_nodes = base_lg_nodes,
        num_arcs = exp_graph.num_arcs(),
        avg_degree = format!("{:.2}", avg_degree).as_str(),
        "base line graph constructed"
    );

    // Load via-way restriction chains — REQUIRED (errors if files missing)
    let chains = load_via_way_chains(&graph_path)?;
    tracing::info!(
        num_chains = chains.len(),
        "via-way restriction chains loaded"
    );

    // Apply node splitting (no-op if chains is empty)
    let split_result = apply_node_splits(exp_graph, &chains);
    let split_count = split_result.split_map.len();

    if split_count > 0 {
        tracing::info!(
            base_nodes = base_lg_nodes,
            split_nodes = split_count,
            expanded_nodes = split_result.graph.num_nodes(),
            expanded_arcs = split_result.graph.num_arcs(),
            "node splitting applied"
        );
    }

    // Build coordinate arrays: base nodes use tail[idx] mapping,
    // split nodes inherit coordinates from their original LG node.
    let expanded_n = split_result.graph.num_nodes();
    let mut new_lat: Vec<f32> = Vec::with_capacity(expanded_n);
    let mut new_lng: Vec<f32> = Vec::with_capacity(expanded_n);
    let mut lg_is_roundabout: Vec<u8> = is_arc_roundabout.clone();

    for idx in 0..base_lg_nodes {
        new_lat.push(lat[tail[idx] as usize]);
        new_lng.push(lng[tail[idx] as usize]);
    }
    for &original in &split_result.split_map {
        // Split node inherits coordinates from its original LG node
        new_lat.push(lat[tail[original as usize] as usize]);
        new_lng.push(lng[tail[original as usize] as usize]);
        lg_is_roundabout.push(is_arc_roundabout[original as usize]);
    }

    // Write expanded line graph
    split_result
        .graph
        .first_out()
        .write_to(&output_dir.join("first_out"))?;
    split_result
        .graph
        .head()
        .write_to(&output_dir.join("head"))?;
    split_result
        .graph
        .weight()
        .write_to(&output_dir.join("travel_time"))?;
    new_lat.write_to(&output_dir.join("latitude"))?;
    new_lng.write_to(&output_dir.join("longitude"))?;
    lg_is_roundabout.write_to(&output_dir.join("is_arc_roundabout"))?;

    // Write split map (mandatory for path reconstruction at runtime)
    split_result
        .split_map
        .write_to(&output_dir.join("via_way_split_map"))?;

    tracing::info!(
        ?output_dir,
        total_nodes = expanded_n,
        split_nodes = split_count,
        "line graph written"
    );
    Ok(())
}
