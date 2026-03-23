use criterion::{Criterion, criterion_group, criterion_main};
use std::path::PathBuf;

use hanoi_bench::dataset::generate_random_queries;
use hanoi_core::{GraphData, SpatialIndex};

fn graph_dir() -> PathBuf {
    PathBuf::from(std::env::var("GRAPH_DIR").expect("set GRAPH_DIR env var to the graph directory"))
}

fn spatial_build_benchmark(c: &mut Criterion) {
    let gd = graph_dir();
    let graph = GraphData::load(&gd).expect("failed to load graph");

    let mut group = c.benchmark_group("spatial");
    group.bench_function("kd_tree_build", |b| {
        b.iter(|| {
            let _ = SpatialIndex::build(
                &graph.latitude,
                &graph.longitude,
                &graph.first_out,
                &graph.head,
            );
        });
    });
    group.finish();
}

fn spatial_snap_benchmark(c: &mut Criterion) {
    let gd = graph_dir();
    let graph = GraphData::load(&gd).expect("failed to load graph");
    let spatial = SpatialIndex::build(
        &graph.latitude,
        &graph.longitude,
        &graph.first_out,
        &graph.head,
    );

    let queries = generate_random_queries(
        graph.num_nodes() as u32,
        &graph.latitude,
        &graph.longitude,
        100,
        42,
    );

    let mut group = c.benchmark_group("spatial");
    group.bench_function("snap_to_edge", |b| {
        let mut i = 0;
        b.iter(|| {
            let q = &queries[i % queries.len()];
            let (lat, lng) = q.from_coords.unwrap();
            spatial.snap_to_edge(lat, lng);
            i += 1;
        });
    });
    group.finish();
}

criterion_group!(benches, spatial_build_benchmark, spatial_snap_benchmark);
criterion_main!(benches);
