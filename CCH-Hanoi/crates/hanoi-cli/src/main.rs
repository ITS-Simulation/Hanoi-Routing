use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand, ValueEnum};
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use hanoi_core::{CchContext, GraphData, LineGraphCchContext, LineGraphQueryEngine, QueryEngine};

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
#[command(name = "cch-hanoi", about = "Hanoi CCH routing CLI")]
struct Cli {
    /// Log output format
    #[arg(long, value_enum, default_value_t = LogFormat::Pretty)]
    log_format: LogFormat,

    /// Also write logs to file in JSON format (logs go to both stderr and file)
    #[arg(long, value_name = "PATH")]
    log_file: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Debug, Default, ValueEnum)]
enum OutputFormat {
    /// GeoJSON FeatureCollection (RFC 7946, [lng, lat] coordinates)
    #[default]
    Geojson,
    /// Flat JSON with [lat, lng] coordinates (legacy format)
    Json,
}

#[derive(Subcommand)]
enum Command {
    /// Run a shortest-path query on the normal graph.
    /// Loads graph from disk, builds CCH, customizes with baseline travel_time,
    /// and runs the query — all in-process, no server required.
    Query {
        /// Parent data directory (contains graph/ and optionally line_graph/ subdirectories)
        #[arg(long)]
        data_dir: PathBuf,

        /// Query the turn-expanded line graph instead of the normal graph.
        /// Expects data_dir/line_graph/ and data_dir/graph/ to both exist.
        #[arg(long, default_value_t = false)]
        line_graph: bool,

        /// Source node ID
        #[arg(long)]
        from_node: Option<u32>,
        /// Target node ID
        #[arg(long)]
        to_node: Option<u32>,

        /// Source latitude
        #[arg(long)]
        from_lat: Option<f32>,
        /// Source longitude
        #[arg(long, requires = "from_lat")]
        from_lng: Option<f32>,
        /// Target latitude
        #[arg(long)]
        to_lat: Option<f32>,
        /// Target longitude
        #[arg(long, requires = "to_lat")]
        to_lng: Option<f32>,

        /// Output file path (auto-generated if omitted: query_<timestamp>.geojson/.json)
        #[arg(long)]
        output_file: Option<PathBuf>,

        /// Output format
        #[arg(long, value_enum, default_value_t = OutputFormat::Geojson)]
        output_format: OutputFormat,

        /// Add simplestyle-spec visualization properties to GeoJSON output
        /// (stroke, stroke-width, fill, fill-opacity)
        #[arg(long, default_value_t = false)]
        demo: bool,

        /// Number of alternative routes to find (0 = single shortest path)
        #[arg(long, default_value_t = 0)]
        alternatives: u32,

        /// Stretch factor for alternative routes (candidates up to this factor x optimal distance)
        #[arg(long, default_value_t = hanoi_core::multi_route::DEFAULT_STRETCH)]
        stretch: f64,
    },

    /// Display graph metadata (num nodes, num edges) without building the CCH.
    Info {
        /// Parent data directory (contains graph/ subdirectory)
        #[arg(long)]
        data_dir: PathBuf,

        /// Show info for the line graph instead of the normal graph.
        #[arg(long, default_value_t = false)]
        line_graph: bool,
    },
}

/// Format query result as JSON or GeoJSON (RFC 7946).
/// GeoJSON uses [longitude, latitude] coordinate order per spec.
fn format_result(
    distance_ms: u32,
    distance_m: f64,
    path: &[u32],
    coordinates: &[(f32, f32)],
    format: &OutputFormat,
    demo: bool,
) -> serde_json::Value {
    match format {
        OutputFormat::Geojson => {
            let coords: Vec<[f32; 2]> = coordinates
                .iter()
                .map(|&(lat, lng)| [lng, lat]) // GeoJSON: [longitude, latitude]
                .collect();
            let mut props = serde_json::json!({
                "distance_ms": distance_ms,
                "distance_m": distance_m
            });
            if demo {
                let obj = props.as_object_mut().unwrap();
                obj.insert("stroke".into(), serde_json::json!("#ff5500"));
                obj.insert("stroke-width".into(), serde_json::json!(10));
                obj.insert("fill".into(), serde_json::json!("#ffaa00"));
                obj.insert("fill-opacity".into(), serde_json::json!(0.4));
            }
            serde_json::json!({
                "type": "FeatureCollection",
                "features": [{
                    "type": "Feature",
                    "geometry": {
                        "type": "LineString",
                        "coordinates": coords
                    },
                    "properties": props
                }]
            })
        }
        OutputFormat::Json => {
            serde_json::json!({
                "distance_ms": distance_ms,
                "distance_m": distance_m,
                "path_nodes": path,
                "coordinates": coordinates.iter().map(|&(lat, lng)| [lat, lng]).collect::<Vec<_>>(),
            })
        }
    }
}

/// Initialize tracing. Returns an optional `WorkerGuard` that **must** be held
/// alive for the program's lifetime — dropping it closes the file writer.
fn init_tracing(log_format: &LogFormat, log_file: Option<&PathBuf>) -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Optional file writer (non-blocking with no ANSI codes)
    let (file_writer, guard) = if let Some(path) = log_file {
        match std::fs::File::create(path) {
            Ok(file) => {
                let (non_blocking, guard) = tracing_appender::non_blocking(file);
                (Some(non_blocking), Some(guard))
            }
            Err(e) => {
                eprintln!("failed to create log file: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        (None, None)
    };

    /// Build an optional JSON file layer from a pre-allocated NonBlocking writer.
    /// Uses JSON format to avoid ANSI code leakage from the stderr layer's span fields.
    fn file_layer<S>(
        writer: Option<NonBlocking>,
    ) -> Option<
        fmt::Layer<S, fmt::format::JsonFields, fmt::format::Format<fmt::format::Json>, NonBlocking>,
    >
    where
        S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    {
        writer.map(|w| fmt::layer().with_writer(w).with_ansi(false).json())
    }

    // Compose stderr + optional file logging
    match log_format {
        LogFormat::Pretty => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().pretty())
                .with(file_layer(file_writer))
                .init();
        }
        LogFormat::Compact => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().compact())
                .with(file_layer(file_writer))
                .init();
        }
        LogFormat::Full => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_target(true))
                .with(file_layer(file_writer))
                .init();
        }
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json())
                .with(file_layer(file_writer))
                .init();
        }
        LogFormat::Tree => {
            // Tree not available in CLI (no tracing-tree dep); fall back to pretty
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().pretty())
                .with(file_layer(file_writer))
                .init();
        }
    }

    guard
}

/// Color palette for multi-route GeoJSON visualization.
const ROUTE_COLORS: &[&str] = &[
    "#ff5500", "#0055ff", "#00aa44", "#aa00cc", "#cc8800", "#e6194b", "#3cb44b", "#4363d8",
    "#f58231", "#911eb4",
];

/// Format multiple route answers as a GeoJSON FeatureCollection or JSON array.
fn format_multi_result(
    answers: &[hanoi_core::cch::QueryAnswer],
    format: &OutputFormat,
    demo: bool,
) -> serde_json::Value {
    match format {
        OutputFormat::Geojson => {
            let features: Vec<serde_json::Value> = answers
                .iter()
                .enumerate()
                .map(|(idx, a)| {
                    let coords: Vec<[f32; 2]> =
                        a.coordinates.iter().map(|&(lat, lng)| [lng, lat]).collect();
                    let mut props = serde_json::json!({
                        "distance_ms": a.distance_ms,
                        "distance_m": a.distance_m,
                        "route_index": idx
                    });
                    if demo {
                        let obj = props.as_object_mut().unwrap();
                        let color = ROUTE_COLORS[idx % ROUTE_COLORS.len()];
                        obj.insert("stroke".into(), serde_json::json!(color));
                        obj.insert(
                            "stroke-width".into(),
                            serde_json::json!(if idx == 0 { 10 } else { 6 }),
                        );
                        obj.insert("fill".into(), serde_json::json!(color));
                        obj.insert("fill-opacity".into(), serde_json::json!(0.3));
                    }
                    serde_json::json!({
                        "type": "Feature",
                        "geometry": {
                            "type": "LineString",
                            "coordinates": coords
                        },
                        "properties": props
                    })
                })
                .collect();
            serde_json::json!({
                "type": "FeatureCollection",
                "features": features
            })
        }
        OutputFormat::Json => {
            let routes: Vec<serde_json::Value> = answers
                .iter()
                .map(|a| {
                    serde_json::json!({
                        "distance_ms": a.distance_ms,
                        "distance_m": a.distance_m,
                        "path_nodes": a.path,
                        "coordinates": a.coordinates.iter().map(|&(lat, lng)| [lat, lng]).collect::<Vec<_>>(),
                    })
                })
                .collect();
            serde_json::to_value(routes).unwrap()
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_multi_query(
    data_dir: &PathBuf,
    line_graph: bool,
    from_node: Option<u32>,
    to_node: Option<u32>,
    from_lat: Option<f32>,
    from_lng: Option<f32>,
    to_lat: Option<f32>,
    to_lng: Option<f32>,
    output_file: Option<PathBuf>,
    output_format: &OutputFormat,
    demo: bool,
    alternatives: u32,
    stretch: f64,
) {
    let answers = if line_graph {
        let lg_dir = data_dir.join("line_graph");
        let original_dir = data_dir.join("graph");
        let perm_path = lg_dir.join("perms/cch_perm");

        tracing::info!(?lg_dir, ?original_dir, "loading line graph");
        let t0 = Instant::now();
        let context = LineGraphCchContext::load_and_build(&lg_dir, &original_dir, &perm_path)
            .expect("failed to load line graph");
        tracing::info!(elapsed = ?t0.elapsed(), "DirectedCCH built");

        let t1 = Instant::now();
        let mut engine = LineGraphQueryEngine::new(&context);
        tracing::info!(elapsed = ?t1.elapsed(), "initial customization + spatial index");

        if let (Some(from), Some(to)) = (from_node, to_node) {
            engine.multi_query(from, to, alternatives as usize, stretch)
        } else if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
            (from_lat, from_lng, to_lat, to_lng)
        {
            match engine.multi_query_coords(
                (flat, flng),
                (tlat, tlng),
                alternatives as usize,
                stretch,
            ) {
                Ok(answers) => answers,
                Err(rejection) => {
                    tracing::error!(%rejection, "coordinate validation failed");
                    std::process::exit(2);
                }
            }
        } else {
            tracing::error!("specify either --from-node/--to-node or coordinate flags");
            std::process::exit(1);
        }
    } else {
        let graph_dir = data_dir.join("graph");
        let perm_path = graph_dir.join("perms/cch_perm");

        tracing::info!(?graph_dir, "loading graph");
        let t0 = Instant::now();
        let context =
            CchContext::load_and_build(&graph_dir, &perm_path).expect("failed to load graph");
        tracing::info!(elapsed = ?t0.elapsed(), "CCH built");

        let t1 = Instant::now();
        let mut engine = QueryEngine::new(&context);
        tracing::info!(elapsed = ?t1.elapsed(), "initial customization + spatial index");

        if let (Some(from), Some(to)) = (from_node, to_node) {
            engine.multi_query(from, to, alternatives as usize, stretch)
        } else if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
            (from_lat, from_lng, to_lat, to_lng)
        {
            match engine.multi_query_coords(
                (flat, flng),
                (tlat, tlng),
                alternatives as usize,
                stretch,
            ) {
                Ok(answers) => answers,
                Err(rejection) => {
                    tracing::error!(%rejection, "coordinate validation failed");
                    std::process::exit(2);
                }
            }
        } else {
            tracing::error!("specify either --from-node/--to-node or coordinate flags");
            std::process::exit(1);
        }
    };

    if answers.is_empty() {
        tracing::warn!("no routes found");
        std::process::exit(1);
    }

    let output = format_multi_result(&answers, output_format, demo);
    let output_str = serde_json::to_string_pretty(&output).unwrap();

    let path = output_file.unwrap_or_else(|| {
        let ts = chrono::Local::now().format("%Y-%m-%dT%H%M%S");
        let ext = match output_format {
            OutputFormat::Geojson => "geojson",
            OutputFormat::Json => "json",
        };
        PathBuf::from(format!("query_{}.{}", ts, ext))
    });

    match std::fs::write(&path, format!("{}\n", output_str)) {
        Ok(_) => {
            tracing::info!(
                num_routes = answers.len(),
                output = %path.display(),
                "multi-route query result"
            );
        }
        Err(e) => {
            tracing::error!(?path, error = %e, "failed to write output file");
            std::process::exit(1);
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let _guard = init_tracing(&cli.log_format, cli.log_file.as_ref());

    match cli.command {
        Command::Query {
            data_dir,
            line_graph,
            from_node,
            to_node,
            from_lat,
            from_lng,
            to_lat,
            to_lng,
            output_file,
            output_format,
            demo,
            alternatives,
            stretch,
        } => {
            if alternatives > 0 {
                run_multi_query(
                    &data_dir,
                    line_graph,
                    from_node,
                    to_node,
                    from_lat,
                    from_lng,
                    to_lat,
                    to_lng,
                    output_file,
                    &output_format,
                    demo,
                    alternatives,
                    stretch,
                );
            } else {
                let answer = if line_graph {
                    let lg_dir = data_dir.join("line_graph");
                    let original_dir = data_dir.join("graph");
                    let perm_path = lg_dir.join("perms/cch_perm");

                    tracing::info!(?lg_dir, ?original_dir, "loading line graph");
                    let t0 = Instant::now();
                    let context =
                        LineGraphCchContext::load_and_build(&lg_dir, &original_dir, &perm_path)
                            .expect("failed to load line graph");
                    tracing::info!(elapsed = ?t0.elapsed(), "DirectedCCH built");

                    let t1 = Instant::now();
                    let mut engine = LineGraphQueryEngine::new(&context);
                    tracing::info!(elapsed = ?t1.elapsed(), "initial customization + spatial index");

                    if let (Some(from), Some(to)) = (from_node, to_node) {
                        engine.query(from, to)
                    } else if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
                        (from_lat, from_lng, to_lat, to_lng)
                    {
                        match engine.query_coords((flat, flng), (tlat, tlng)) {
                            Ok(answer) => answer,
                            Err(rejection) => {
                                tracing::error!(%rejection, "coordinate validation failed");
                                std::process::exit(2);
                            }
                        }
                    } else {
                        tracing::error!("specify either --from-node/--to-node or coordinate flags");
                        std::process::exit(1);
                    }
                } else {
                    let graph_dir = data_dir.join("graph");
                    let perm_path = graph_dir.join("perms/cch_perm");

                    tracing::info!(?graph_dir, "loading graph");
                    let t0 = Instant::now();
                    let context = CchContext::load_and_build(&graph_dir, &perm_path)
                        .expect("failed to load graph");
                    tracing::info!(elapsed = ?t0.elapsed(), "CCH built");

                    let t1 = Instant::now();
                    let mut engine = QueryEngine::new(&context);
                    tracing::info!(elapsed = ?t1.elapsed(), "initial customization + spatial index");

                    if let (Some(from), Some(to)) = (from_node, to_node) {
                        engine.query(from, to)
                    } else if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
                        (from_lat, from_lng, to_lat, to_lng)
                    {
                        match engine.query_coords((flat, flng), (tlat, tlng)) {
                            Ok(answer) => answer,
                            Err(rejection) => {
                                tracing::error!(%rejection, "coordinate validation failed");
                                std::process::exit(2);
                            }
                        }
                    } else {
                        tracing::error!("specify either --from-node/--to-node or coordinate flags");
                        std::process::exit(1);
                    }
                };

                match answer {
                    Some(a) => {
                        let output = format_result(
                            a.distance_ms,
                            a.distance_m,
                            &a.path,
                            &a.coordinates,
                            &output_format,
                            demo,
                        );
                        let output_str = serde_json::to_string_pretty(&output).unwrap();

                        let path = output_file.unwrap_or_else(|| {
                            let ts = chrono::Local::now().format("%Y-%m-%dT%H%M%S");
                            let ext = match output_format {
                                OutputFormat::Geojson => "geojson",
                                OutputFormat::Json => "json",
                            };
                            PathBuf::from(format!("query_{}.{}", ts, ext))
                        });

                        match std::fs::write(&path, format!("{}\n", output_str)) {
                            Ok(_) => {
                                tracing::info!(
                                    distance_ms = a.distance_ms,
                                    distance_m = format!("{:.1}", a.distance_m).as_str(),
                                    path_nodes = a.path.len(),
                                    output = %path.display(),
                                    "query result"
                                );
                            }
                            Err(e) => {
                                tracing::error!(?path, error = %e, "failed to write output file");
                                std::process::exit(1);
                            }
                        }
                    }
                    None => {
                        tracing::warn!("no path found");
                        std::process::exit(1);
                    }
                }
            }
        }

        Command::Info {
            data_dir,
            line_graph,
        } => {
            let (graph_dir, graph_type) = if line_graph {
                (data_dir.join("line_graph"), "line_graph")
            } else {
                (data_dir.join("graph"), "normal")
            };
            let graph = GraphData::load(&graph_dir).expect("failed to load graph");
            let output = serde_json::json!({
                "graph_type": graph_type,
                "graph_dir": graph_dir.display().to_string(),
                "num_nodes": graph.num_nodes(),
                "num_edges": graph.num_edges(),
            });
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
    }
}
