use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{mpsc, watch};

use hanoi_core::CoordRejection;
use hanoi_core::cch::{CchContext, QueryAnswer, QueryEngine};
use hanoi_core::line_graph::{LineGraphCchContext, LineGraphQueryEngine};
use rust_road_router::datastr::graph::Weight;
use serde_json::Value;

use crate::state::QueryMsg;
use crate::types::{QueryRequest, QueryResponse};

/// Background loop for the normal-graph engine.
///
/// Owns the `CchContext` and `QueryEngine` — processes queries from the mpsc
/// channel and applies customization updates from the watch channel.
pub fn run_normal(
    context: &CchContext,
    query_rx: &mut mpsc::Receiver<QueryMsg>,
    watch_rx: &mut watch::Receiver<Option<Vec<Weight>>>,
    customization_active: &Arc<AtomicBool>,
    engine_alive: &Arc<AtomicBool>,
    rt: &tokio::runtime::Handle,
) {
    let mut engine = QueryEngine::new(context);

    loop {
        // Non-blocking check for a pending customization
        if watch_rx.has_changed().unwrap_or(false) {
            if let Some(weights) = watch_rx.borrow_and_update().clone() {
                customization_active.store(true, Ordering::Relaxed);
                let _span =
                    tracing::info_span!("customization", num_weights = weights.len()).entered();
                tracing::info!(num_weights = weights.len(), "re-customizing");
                engine.update_weights(&weights);
                customization_active.store(false, Ordering::Relaxed);
                tracing::info!("customization complete");
            }
        }

        // Process one query (blocking with timeout to periodically check customization)
        let msg = rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(50), query_rx.recv()).await
        });

        match msg {
            Ok(Some(qm)) => {
                let resp =
                    dispatch_normal(&mut engine, qm.request, qm.format.as_deref(), qm.colors);
                let _ = qm.reply.send(resp);
            }
            Ok(None) => break, // Channel closed — shutdown
            Err(_) => {}       // Timeout — loop back
        }
    }

    engine_alive.store(false, Ordering::Relaxed);
}

/// Background loop for the line-graph engine.
pub fn run_line_graph(
    context: &LineGraphCchContext,
    query_rx: &mut mpsc::Receiver<QueryMsg>,
    watch_rx: &mut watch::Receiver<Option<Vec<Weight>>>,
    customization_active: &Arc<AtomicBool>,
    engine_alive: &Arc<AtomicBool>,
    rt: &tokio::runtime::Handle,
) {
    let mut engine = LineGraphQueryEngine::new(context);

    loop {
        if watch_rx.has_changed().unwrap_or(false) {
            if let Some(weights) = watch_rx.borrow_and_update().clone() {
                customization_active.store(true, Ordering::Relaxed);
                let _span =
                    tracing::info_span!("customization", num_weights = weights.len()).entered();
                tracing::info!(num_weights = weights.len(), "re-customizing line graph");
                engine.update_weights(&weights);
                customization_active.store(false, Ordering::Relaxed);
                tracing::info!("line graph customization complete");
            }
        }

        let msg = rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(50), query_rx.recv()).await
        });

        match msg {
            Ok(Some(qm)) => {
                let resp =
                    dispatch_line_graph(&mut engine, qm.request, qm.format.as_deref(), qm.colors);
                let _ = qm.reply.send(resp);
            }
            Ok(None) => break,
            Err(_) => {}
        }
    }

    engine_alive.store(false, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Query dispatch helpers
// ---------------------------------------------------------------------------

fn dispatch_normal(
    engine: &mut QueryEngine<'_>,
    req: QueryRequest,
    format: Option<&str>,
    colors: bool,
) -> Result<Value, CoordRejection> {
    let answer = if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
        (req.from_lat, req.from_lng, req.to_lat, req.to_lng)
    {
        engine.query_coords((flat, flng), (tlat, tlng))?
    } else if let (Some(from), Some(to)) = (req.from_node, req.to_node) {
        engine.query(from, to)
    } else {
        None
    };

    // Structured event with query result metadata
    match &answer {
        Some(a) => tracing::info!(
            distance_ms = a.distance_ms,
            distance_m = a.distance_m,
            path_len = a.path.len(),
            "query completed"
        ),
        None => tracing::info!("query returned no path"),
    }

    Ok(format_response(answer, format, colors))
}

fn dispatch_line_graph(
    engine: &mut LineGraphQueryEngine<'_>,
    req: QueryRequest,
    format: Option<&str>,
    colors: bool,
) -> Result<Value, CoordRejection> {
    let answer = if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
        (req.from_lat, req.from_lng, req.to_lat, req.to_lng)
    {
        engine.query_coords((flat, flng), (tlat, tlng))?
    } else if let (Some(from), Some(to)) = (req.from_node, req.to_node) {
        engine.query(from, to)
    } else {
        None
    };

    // Structured event with query result metadata
    match &answer {
        Some(a) => tracing::info!(
            distance_ms = a.distance_ms,
            distance_m = a.distance_m,
            path_len = a.path.len(),
            "query completed"
        ),
        None => tracing::info!("query returned no path"),
    }

    Ok(format_response(answer, format, colors))
}

/// Convert a query answer to the requested response format.
/// Default (None) → GeoJSON Feature; `"json"` → plain JSON.
/// When `colors` is true and format is GeoJSON, adds simplestyle-spec properties.
fn format_response(answer: Option<QueryAnswer>, format: Option<&str>, colors: bool) -> Value {
    match format {
        Some("json") => serde_json::to_value(answer_to_response(answer)).unwrap(),
        _ => answer_to_geojson(answer, colors),
    }
}

fn answer_to_response(answer: Option<QueryAnswer>) -> QueryResponse {
    match answer {
        Some(a) => {
            let QueryAnswer {
                distance_ms,
                distance_m,
                path,
                coordinates,
                turns,
                origin,
                destination,
            } = a;

            QueryResponse {
                distance_ms: Some(distance_ms),
                distance_m: Some(distance_m),
                path_nodes: path,
                coordinates: coordinates
                    .into_iter()
                    .map(|(lat, lng)| [lat, lng])
                    .collect(),
                turns,
                origin: origin.map(|(lat, lng)| [lat, lng]),
                destination: destination.map(|(lat, lng)| [lat, lng]),
            }
        }
        None => QueryResponse::empty(),
    }
}

/// Build a GeoJSON FeatureCollection with a LineString geometry from the query answer.
///
/// Per RFC 7946, coordinates are [longitude, latitude] (note: reversed from
/// our internal (lat, lng) convention).  Output is a FeatureCollection (matching
/// the CLI's output format) for maximum tool compatibility.
///
/// When `colors` is true, adds simplestyle-spec visualization properties
/// (stroke, stroke-width, fill, fill-opacity) to the Feature properties.
fn answer_to_geojson(answer: Option<QueryAnswer>, colors: bool) -> Value {
    match answer {
        Some(a) => {
            let QueryAnswer {
                distance_ms,
                distance_m,
                path: _,
                coordinates,
                turns,
                origin,
                destination,
            } = a;

            // Convert (lat, lng) → [lng, lat] per GeoJSON spec
            let coords: Vec<[f32; 2]> = coordinates.iter().map(|&(lat, lng)| [lng, lat]).collect();

            let mut props = serde_json::json!({
                "distance_ms": distance_ms,
                "distance_m": distance_m
            });
            let obj = props.as_object_mut().unwrap();
            if let Some((lat, lng)) = origin {
                obj.insert("origin".into(), serde_json::json!([lat, lng]));
            }
            if let Some((lat, lng)) = destination {
                obj.insert("destination".into(), serde_json::json!([lat, lng]));
            }
            if !turns.is_empty() {
                obj.insert("turns".into(), serde_json::to_value(turns).unwrap());
            }
            if colors {
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
        None => {
            serde_json::json!({
                "type": "FeatureCollection",
                "features": [{
                    "type": "Feature",
                    "geometry": null,
                    "properties": {
                        "distance_ms": null,
                        "distance_m": null
                    }
                }]
            })
        }
    }
}
