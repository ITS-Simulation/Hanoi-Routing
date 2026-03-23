use std::sync::Arc;

use crate::dataset::QueryPair;
use crate::{BenchmarkConfig, Measurement, percentile_sorted};

/// Send a single query to the server and return the parsed JSON response.
async fn send_query(
    client: &reqwest::Client,
    base_url: &str,
    q: &QueryPair,
) -> Result<serde_json::Value, reqwest::Error> {
    let body = if let (Some(from), Some(to)) = (q.from_coords, q.to_coords) {
        serde_json::json!({
            "from_lat": from.0,
            "from_lng": from.1,
            "to_lat": to.0,
            "to_lng": to.1,
        })
    } else {
        serde_json::json!({
            "from_node": q.from_node,
            "to_node": q.to_node,
        })
    };

    let resp = client
        .post(format!("{}/query", base_url))
        .json(&body)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    Ok(resp)
}

/// Benchmark POST /query round-trip latency (single client, sequential).
pub async fn bench_http_query(
    base_url: &str,
    queries: &[QueryPair],
    config: &BenchmarkConfig,
) -> Vec<Measurement> {
    let client = reqwest::Client::new();
    let mut measurements = Vec::new();

    // Warmup
    for q in queries.iter().take(config.warmup_iterations) {
        let _ = send_query(&client, base_url, q).await;
    }

    // Measured
    for (i, q) in queries.iter().take(config.query_count).enumerate() {
        let start = std::time::Instant::now();
        let resp = send_query(&client, base_url, q).await;
        let elapsed = start.elapsed();

        measurements.push(Measurement {
            label: format!("http_query_{}", i),
            duration: elapsed,
            metadata: serde_json::json!({
                "status": resp.is_ok(),
                "has_path": resp.ok().and_then(|r| r.get("distance_ms").and_then(|v| v.as_u64())).is_some(),
            }),
        });
    }
    measurements
}

/// Concurrent load test with N parallel clients.
pub async fn bench_http_concurrent(
    base_url: &str,
    queries: &[QueryPair],
    concurrency: usize,
    config: &BenchmarkConfig,
) -> Vec<Measurement> {
    let client = reqwest::Client::new();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));

    let start = std::time::Instant::now();
    let mut handles = Vec::new();

    for q in queries.iter().take(config.query_count) {
        let client = client.clone();
        let sem = semaphore.clone();
        let url = base_url.to_string();
        let query = q.clone();

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            let t = std::time::Instant::now();
            let resp = send_query(&client, &url, &query).await;
            let elapsed = t.elapsed();
            (elapsed, resp.is_ok())
        }));
    }

    let results: Vec<_> = futures::future::join_all(handles)
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();

    let total = start.elapsed();
    let succeeded = results.iter().filter(|(_, ok)| *ok).count();
    let latencies: Vec<f64> = results
        .iter()
        .map(|(d, _)| d.as_secs_f64() * 1000.0)
        .collect();

    let mut sorted = latencies.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let p50 = percentile_sorted(&sorted, 50.0);
    let p95 = percentile_sorted(&sorted, 95.0);
    let p99 = percentile_sorted(&sorted, 99.0);
    let qps = if total.as_secs_f64() > 0.0 {
        results.len() as f64 / total.as_secs_f64()
    } else {
        0.0
    };

    vec![Measurement {
        label: format!("http_concurrent_{}cli", concurrency),
        duration: total,
        metadata: serde_json::json!({
            "concurrency": concurrency,
            "total_queries": results.len(),
            "succeeded": succeeded,
            "throughput_qps": qps,
            "p50_ms": p50,
            "p95_ms": p95,
            "p99_ms": p99,
        }),
    }]
}

/// Benchmark GET /info round-trip latency.
pub async fn bench_http_info(base_url: &str, config: &BenchmarkConfig) -> Vec<Measurement> {
    let client = reqwest::Client::new();
    let mut measurements = Vec::new();

    // Warmup
    for _ in 0..config.warmup_iterations {
        let _ = client.get(format!("{}/info", base_url)).send().await;
    }

    // Measured
    for i in 0..config.measured_iterations {
        let start = std::time::Instant::now();
        let _ = client.get(format!("{}/info", base_url)).send().await;
        let elapsed = start.elapsed();
        measurements.push(Measurement {
            label: format!("http_info_{}", i),
            duration: elapsed,
            metadata: serde_json::json!({
                "ms": elapsed.as_secs_f64() * 1000.0,
            }),
        });
    }
    measurements
}

/// Benchmark POST /customize weight upload + application.
///
/// `customize_url` should be the base URL of the customize port
/// (default: http://localhost:9080). Sends the provided weights
/// as raw little-endian u32 bytes.
pub async fn bench_customize_upload(
    customize_url: &str,
    weights: &[u32],
    config: &BenchmarkConfig,
) -> Vec<Measurement> {
    let client = reqwest::Client::new();
    let body_bytes: Vec<u8> = weights.iter().flat_map(|w| w.to_le_bytes()).collect();
    let mut measurements = Vec::new();

    // Warmup
    for _ in 0..config.warmup_iterations {
        let _ = client
            .post(format!("{}/customize", customize_url))
            .header("content-type", "application/octet-stream")
            .body(body_bytes.clone())
            .send()
            .await;
    }

    // Measured
    for i in 0..config.measured_iterations {
        let start = std::time::Instant::now();
        let resp = client
            .post(format!("{}/customize", customize_url))
            .header("content-type", "application/octet-stream")
            .body(body_bytes.clone())
            .send()
            .await;
        let elapsed = start.elapsed();

        let ok = resp.is_ok();

        measurements.push(Measurement {
            label: format!("customize_upload_iter_{}", i),
            duration: elapsed,
            metadata: serde_json::json!({
                "seconds": elapsed.as_secs_f64(),
                "weight_count": weights.len(),
                "success": ok,
            }),
        });
    }
    measurements
}

/// Benchmark query latency immediately after customization.
pub async fn bench_query_after_customize(
    base_url: &str,
    customize_url: &str,
    weights: &[u32],
    queries: &[QueryPair],
    config: &BenchmarkConfig,
) -> Vec<Measurement> {
    let client = reqwest::Client::new();
    let body_bytes: Vec<u8> = weights.iter().flat_map(|w| w.to_le_bytes()).collect();

    // Upload weights first
    let _ = client
        .post(format!("{}/customize", customize_url))
        .header("content-type", "application/octet-stream")
        .body(body_bytes)
        .send()
        .await;

    // Brief pause to allow customization to apply
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Then benchmark queries
    bench_http_query(base_url, queries, config).await
}
