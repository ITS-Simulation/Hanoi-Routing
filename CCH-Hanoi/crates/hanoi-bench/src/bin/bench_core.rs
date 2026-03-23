/// Standalone core benchmark runner.
///
/// Usage:
///   bench_core --graph-dir ./data --generate-queries 10000 --seed 42 --save-queries queries.json
///   bench_core --graph-dir ./data --queries queries.json --iterations 10 --output core_results.json
use std::path::PathBuf;

use clap::Parser;
use tracing;

use hanoi_bench::core::{bench_cch_build, bench_customize, bench_query, bench_query_coords};
use hanoi_bench::dataset::{generate_random_queries, load_queries, save_queries};
use hanoi_bench::log::init_bench_tracing;
use hanoi_bench::report::{ReportFormat, generate_report};
use hanoi_bench::spatial::{bench_kd_build, bench_snap};
use hanoi_bench::{BenchmarkConfig, BenchmarkRun};
use hanoi_core::{CchContext, QueryEngine};

#[derive(Parser)]
#[command(name = "bench_core", about = "Core CCH benchmark runner")]
struct Args {
    /// Path to graph directory
    #[arg(long)]
    graph_dir: PathBuf,

    /// Path to cch_perm file (default: <graph_dir>/perms/cch_perm)
    #[arg(long)]
    perm_path: Option<PathBuf>,

    /// Generate N random queries and optionally save them
    #[arg(long)]
    generate_queries: Option<usize>,

    /// Path to save generated queries
    #[arg(long)]
    save_queries: Option<PathBuf>,

    /// Path to load query dataset JSON file
    #[arg(long)]
    queries: Option<PathBuf>,

    /// Number of measured iterations
    #[arg(long, default_value_t = 10)]
    iterations: usize,

    /// Number of warmup iterations
    #[arg(long, default_value_t = 3)]
    warmup: usize,

    /// Number of queries per iteration
    #[arg(long, default_value_t = 1000)]
    query_count: usize,

    /// Output results file (JSON)
    #[arg(long, default_value = "core_results.json")]
    output: PathBuf,

    /// RNG seed
    #[arg(long, default_value_t = 42)]
    seed: u64,

    /// Custom log file name prefix (default: "bench_core")
    #[arg(long)]
    log_name: Option<String>,
}

fn main() {
    let args = Args::parse();
    let (_log_path, _guard) = init_bench_tracing(args.log_name.as_deref());

    let perm_path = args
        .perm_path
        .unwrap_or_else(|| args.graph_dir.join("perms/cch_perm"));

    let config = BenchmarkConfig {
        warmup_iterations: args.warmup,
        measured_iterations: args.iterations,
        query_count: args.query_count,
        seed: args.seed,
    };

    // Load graph for query generation
    tracing::info!(graph_dir = %args.graph_dir.display(), "loading graph");
    let context = CchContext::load_and_build(&args.graph_dir, &perm_path)
        .expect("failed to load and build CCH");

    // Generate or load queries
    let query_count = args.generate_queries.unwrap_or(args.query_count);
    let queries = if let Some(ref path) = args.queries {
        tracing::info!(path = %path.display(), "loading queries");
        load_queries(path)
    } else {
        tracing::info!(count = query_count, seed = args.seed, "generating random queries");
        generate_random_queries(
            context.graph.num_nodes() as u32,
            &context.graph.latitude,
            &context.graph.longitude,
            query_count,
            args.seed,
        )
    };

    // Optionally save generated queries
    if let Some(ref save_path) = args.save_queries {
        save_queries(&queries, save_path);
        tracing::info!(count = queries.len(), path = %save_path.display(), "saved queries");
        if args.queries.is_none() && args.generate_queries.is_some() {
            // If we were only asked to generate and save, exit
            return;
        }
    }

    let mut runs = Vec::new();

    // 1) CCH build benchmark
    tracing::info!("benchmarking CCH build");
    let build_measurements = bench_cch_build(&args.graph_dir, &perm_path, &config);
    runs.push(BenchmarkRun {
        name: "cch_build".to_string(),
        timestamp: chrono::Utc::now(),
        graph_dir: args.graph_dir.display().to_string(),
        measurements: build_measurements,
        config: config.clone(),
    });

    // 2) Customize benchmark
    tracing::info!("benchmarking customization");
    let customize_measurements = bench_customize(&context, &config);
    runs.push(BenchmarkRun {
        name: "customize".to_string(),
        timestamp: chrono::Utc::now(),
        graph_dir: args.graph_dir.display().to_string(),
        measurements: customize_measurements,
        config: config.clone(),
    });

    // 3) KD-tree build benchmark
    tracing::info!("benchmarking KD-tree build");
    let kd_measurements = bench_kd_build(
        &context.graph.latitude,
        &context.graph.longitude,
        &context.graph.first_out,
        &context.graph.head,
        &config,
    );
    runs.push(BenchmarkRun {
        name: "kd_tree_build".to_string(),
        timestamp: chrono::Utc::now(),
        graph_dir: args.graph_dir.display().to_string(),
        measurements: kd_measurements,
        config: config.clone(),
    });

    // 4) Query engine benchmarks
    let mut engine = QueryEngine::new(&context);

    tracing::info!("benchmarking node-ID queries");
    let query_measurements = bench_query(&mut engine, &queries, &config);
    runs.push(BenchmarkRun {
        name: "query_node_id".to_string(),
        timestamp: chrono::Utc::now(),
        graph_dir: args.graph_dir.display().to_string(),
        measurements: query_measurements,
        config: config.clone(),
    });

    tracing::info!("benchmarking coordinate queries");
    let coord_measurements = bench_query_coords(&mut engine, &queries, &config);
    runs.push(BenchmarkRun {
        name: "query_coords".to_string(),
        timestamp: chrono::Utc::now(),
        graph_dir: args.graph_dir.display().to_string(),
        measurements: coord_measurements,
        config: config.clone(),
    });

    // 5) Snap benchmark
    tracing::info!("benchmarking snap_to_edge");
    let snap_measurements = bench_snap(engine.spatial(), &queries, &config);
    runs.push(BenchmarkRun {
        name: "snap_to_edge".to_string(),
        timestamp: chrono::Utc::now(),
        graph_dir: args.graph_dir.display().to_string(),
        measurements: snap_measurements,
        config: config.clone(),
    });

    // Print summary table
    let mut stdout = Vec::new();
    generate_report(&runs, ReportFormat::Table, &mut stdout);
    print!("{}", String::from_utf8_lossy(&stdout));

    // Save JSON results
    let json = serde_json::to_string_pretty(&runs).expect("failed to serialize results");
    std::fs::write(&args.output, &json).expect("failed to write output file");
    tracing::info!(path = %args.output.display(), "results saved");
}
