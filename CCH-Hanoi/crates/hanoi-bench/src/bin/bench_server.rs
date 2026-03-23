/// Server benchmark runner.
///
/// Expects a running hanoi_server instance.
///
/// Usage:
///   bench_server --url http://localhost:8080 --queries 1000 --concurrency 4
///   bench_server --url http://localhost:8080 --query-file queries.json --output results.json
use std::path::PathBuf;

use clap::Parser;
use tracing;

use hanoi_bench::dataset::{QueryPair, generate_random_queries, load_queries};
use hanoi_bench::log::init_bench_tracing;
use hanoi_bench::report::{ReportFormat, generate_report};
use hanoi_bench::server::{bench_http_concurrent, bench_http_info, bench_http_query};
use hanoi_bench::{BenchmarkConfig, BenchmarkRun};

#[derive(Parser)]
#[command(
    name = "bench_server",
    about = "Server benchmark runner for hanoi_server"
)]
struct Args {
    /// Base URL of the running server (query port)
    #[arg(long, default_value = "http://localhost:8080")]
    url: String,

    /// Number of queries to run
    #[arg(long, default_value_t = 1000)]
    queries: usize,

    /// Concurrency level for load tests
    #[arg(long, default_value_t = 1)]
    concurrency: usize,

    /// Path to query dataset JSON file. If absent, the server benchmarks
    /// will generate random coordinate-based queries by sampling lat/lng
    /// within the Hanoi bounding box (use --graph-dir to generate node-ID
    /// queries from graph files, then save with bench_core --save-queries).
    #[arg(long)]
    query_file: Option<PathBuf>,

    /// Path to graph directory (for generating node-ID-based queries when
    /// no query file is provided). Not required if --query-file is set.
    #[arg(long)]
    graph_dir: Option<PathBuf>,

    /// Output results file (JSON)
    #[arg(long, default_value = "bench_results.json")]
    output: PathBuf,

    /// RNG seed
    #[arg(long, default_value_t = 42)]
    seed: u64,

    /// Custom log file name prefix (default: "bench_server")
    #[arg(long)]
    log_name: Option<String>,
}

/// Hanoi bounding box for random coordinate generation.
const HANOI_LAT_MIN: f32 = 20.90;
const HANOI_LAT_MAX: f32 = 21.10;
const HANOI_LNG_MIN: f32 = 105.75;
const HANOI_LNG_MAX: f32 = 105.90;

fn generate_coord_queries(count: usize, seed: u64) -> Vec<QueryPair> {
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    let mut rng = StdRng::seed_from_u64(seed);
    (0..count)
        .map(|_| {
            let from_lat = rng.random_range(HANOI_LAT_MIN..HANOI_LAT_MAX);
            let from_lng = rng.random_range(HANOI_LNG_MIN..HANOI_LNG_MAX);
            let to_lat = rng.random_range(HANOI_LAT_MIN..HANOI_LAT_MAX);
            let to_lng = rng.random_range(HANOI_LNG_MIN..HANOI_LNG_MAX);
            QueryPair {
                from_node: 0,
                to_node: 0,
                from_coords: Some((from_lat, from_lng)),
                to_coords: Some((to_lat, to_lng)),
            }
        })
        .collect()
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let (_log_path, _guard) = init_bench_tracing(args.log_name.as_deref());

    let config = BenchmarkConfig {
        warmup_iterations: 3,
        measured_iterations: 10,
        query_count: args.queries,
        seed: args.seed,
    };

    // Load or generate queries
    let queries = if let Some(ref path) = args.query_file {
        tracing::info!(path = %path.display(), "loading queries");
        load_queries(path)
    } else if let Some(ref gd) = args.graph_dir {
        tracing::info!(graph_dir = %gd.display(), "generating queries from graph");
        let graph = hanoi_core::GraphData::load(gd).expect("failed to load graph");
        generate_random_queries(
            graph.num_nodes() as u32,
            &graph.latitude,
            &graph.longitude,
            args.queries,
            args.seed,
        )
    } else {
        tracing::info!(count = args.queries, seed = args.seed, "generating random coordinate queries within Hanoi bounding box");
        generate_coord_queries(args.queries, args.seed)
    };

    let mut runs = Vec::new();

    // 1) HTTP info benchmark
    tracing::info!("benchmarking GET /info");
    let info_measurements = bench_http_info(&args.url, &config).await;
    runs.push(BenchmarkRun {
        name: "http_info".to_string(),
        timestamp: chrono::Utc::now(),
        graph_dir: args
            .graph_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        measurements: info_measurements,
        config: config.clone(),
    });

    // 2) HTTP query (sequential)
    tracing::info!(query_count = args.queries, "benchmarking POST /query (sequential)");
    let query_measurements = bench_http_query(&args.url, &queries, &config).await;
    runs.push(BenchmarkRun {
        name: "http_query".to_string(),
        timestamp: chrono::Utc::now(),
        graph_dir: args
            .graph_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        measurements: query_measurements,
        config: config.clone(),
    });

    // 3) HTTP concurrent load test
    if args.concurrency > 1 {
        tracing::info!(concurrency = args.concurrency, query_count = args.queries, "benchmarking POST /query (concurrent)");
        let concurrent_measurements =
            bench_http_concurrent(&args.url, &queries, args.concurrency, &config).await;
        runs.push(BenchmarkRun {
            name: format!("http_concurrent_{}cli", args.concurrency),
            timestamp: chrono::Utc::now(),
            graph_dir: args
                .graph_dir
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            measurements: concurrent_measurements,
            config: config.clone(),
        });
    }

    // Print summary to stdout
    let mut stdout = Vec::new();
    generate_report(&runs, ReportFormat::Table, &mut stdout);
    print!("{}", String::from_utf8_lossy(&stdout));

    // Save JSON results
    let json = serde_json::to_string_pretty(&runs).expect("failed to serialize results");
    std::fs::write(&args.output, &json).expect("failed to write output file");
    tracing::info!(path = %args.output.display(), "results saved");
}
