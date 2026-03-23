use criterion::{Criterion, criterion_group, criterion_main};
use std::path::PathBuf;

use hanoi_bench::dataset::generate_random_queries;
use hanoi_core::{CchContext, QueryEngine};

fn graph_dir() -> PathBuf {
    PathBuf::from(std::env::var("GRAPH_DIR").expect("set GRAPH_DIR env var to the graph directory"))
}

fn perm_path(graph_dir: &PathBuf) -> PathBuf {
    let explicit = std::env::var("PERM_PATH").ok().map(PathBuf::from);
    explicit.unwrap_or_else(|| graph_dir.join("perms/cch_perm"))
}

fn cch_query_benchmark(c: &mut Criterion) {
    let gd = graph_dir();
    let pp = perm_path(&gd);
    let context = CchContext::load_and_build(&gd, &pp).expect("failed to load graph");
    let mut engine = QueryEngine::new(&context);

    let queries = generate_random_queries(
        context.graph.num_nodes() as u32,
        &context.graph.latitude,
        &context.graph.longitude,
        100,
        42,
    );

    let mut group = c.benchmark_group("cch_query");
    group.bench_function("node_id_query", |b| {
        let mut i = 0;
        b.iter(|| {
            let q = &queries[i % queries.len()];
            engine.query(q.from_node, q.to_node);
            i += 1;
        });
    });
    group.bench_function("coord_query", |b| {
        let mut i = 0;
        b.iter(|| {
            let q = &queries[i % queries.len()];
            let _ = engine.query_coords(q.from_coords.unwrap(), q.to_coords.unwrap());
            i += 1;
        });
    });
    group.finish();
}

fn cch_customize_benchmark(c: &mut Criterion) {
    let gd = graph_dir();
    let pp = perm_path(&gd);
    let context = CchContext::load_and_build(&gd, &pp).expect("failed to load graph");

    let mut group = c.benchmark_group("cch_customize");
    group.bench_function("baseline_customize", |b| {
        b.iter(|| {
            let _ = context.customize();
        });
    });
    group.finish();
}

criterion_group!(benches, cch_query_benchmark, cch_customize_benchmark);
criterion_main!(benches);
