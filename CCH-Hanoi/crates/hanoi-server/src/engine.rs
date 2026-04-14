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
                let resp = dispatch_normal(
                    &mut engine,
                    qm.request,
                    qm.format.as_deref(),
                    qm.colors,
                    qm.alternatives,
                    qm.stretch,
                );
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
                let resp = dispatch_line_graph(
                    &mut engine,
                    qm.request,
                    qm.format.as_deref(),
                    qm.colors,
                    qm.alternatives,
                    qm.stretch,
                );
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
    alternatives: u32,
    stretch: f64,
) -> Result<Value, CoordRejection> {
    if alternatives > 0 {
        let answers = if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
            (req.from_lat, req.from_lng, req.to_lat, req.to_lng)
        {
            engine.multi_query_coords((flat, flng), (tlat, tlng), alternatives as usize, stretch)?
        } else if let (Some(from), Some(to)) = (req.from_node, req.to_node) {
            engine.multi_query(from, to, alternatives as usize, stretch)
        } else {
            Vec::new()
        };

        tracing::info!(num_routes = answers.len(), "multi-route query completed");
        return Ok(format_multi_response(answers, format, colors, "normal"));
    }

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

    Ok(format_response(answer, format, colors, "normal"))
}

fn dispatch_line_graph(
    engine: &mut LineGraphQueryEngine<'_>,
    req: QueryRequest,
    format: Option<&str>,
    colors: bool,
    alternatives: u32,
    stretch: f64,
) -> Result<Value, CoordRejection> {
    if alternatives > 0 {
        let answers = if let (Some(flat), Some(flng), Some(tlat), Some(tlng)) =
            (req.from_lat, req.from_lng, req.to_lat, req.to_lng)
        {
            engine.multi_query_coords((flat, flng), (tlat, tlng), alternatives as usize, stretch)?
        } else if let (Some(from), Some(to)) = (req.from_node, req.to_node) {
            engine.multi_query(from, to, alternatives as usize, stretch)
        } else {
            Vec::new()
        };

        tracing::info!(num_routes = answers.len(), "multi-route query completed");
        return Ok(format_multi_response(answers, format, colors, "line_graph"));
    }

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

    Ok(format_response(answer, format, colors, "line_graph"))
}

/// Convert a query answer to the requested response format.
/// Default (None) → GeoJSON Feature; `"json"` → plain JSON.
/// When `colors` is true and format is GeoJSON, adds simplestyle-spec properties.
fn format_response(
    answer: Option<QueryAnswer>,
    format: Option<&str>,
    colors: bool,
    graph_type: &'static str,
) -> Value {
    match format {
        Some("json") => serde_json::to_value(answer_to_response(answer, graph_type)).unwrap(),
        _ => answer_to_geojson(answer, colors, graph_type),
    }
}

fn connect_query_coordinates(
    coordinates: Vec<(f32, f32)>,
    _origin: Option<(f32, f32)>,
    _destination: Option<(f32, f32)>,
) -> Vec<(f32, f32)> {
    // Note: origin and destination are NOT appended to coordinates.
    // The coordinates already include the clipped connector geometry
    // (projection point + road polylines) from patch_coordinates().
    // Appending raw user pins would create impossible paths across buildings.
    coordinates
}

fn answer_to_response(answer: Option<QueryAnswer>, graph_type: &'static str) -> QueryResponse {
    match answer {
        Some(a) => {
            let QueryAnswer {
                distance_ms,
                distance_m,
                route_arc_ids,
                weight_path_ids,
                path,
                coordinates,
                turns,
                origin,
                destination,
                snapped_origin: _,
                snapped_destination: _,
            } = a;

            let coord_pairs: Vec<[f32; 2]> =
                connect_query_coordinates(coordinates, origin, destination)
                    .into_iter()
                    .map(|(lat, lng)| [lat, lng])
                    .collect();

            QueryResponse {
                graph_type: Some(graph_type),
                distance_ms: Some(distance_ms),
                distance_m: Some(distance_m),
                path_nodes: path,
                route_arc_ids,
                weight_path_ids,
                coordinates: coord_pairs,
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
fn answer_to_geojson(answer: Option<QueryAnswer>, colors: bool, graph_type: &'static str) -> Value {
    match answer {
        Some(a) => {
            let QueryAnswer {
                distance_ms,
                distance_m,
                route_arc_ids,
                weight_path_ids,
                path,
                coordinates,
                turns,
                origin,
                destination,
                snapped_origin: _,
                snapped_destination: _,
            } = a;

            // Convert (lat, lng) → [lng, lat] per GeoJSON spec
            let coords: Vec<[f32; 2]> = connect_query_coordinates(coordinates, origin, destination)
                .into_iter()
                .map(|(lat, lng)| [lng, lat])
                .collect();

            let mut props = serde_json::json!({
                "source": "hanoi_server",
                "export_version": 1,
                "graph_type": graph_type,
                "distance_ms": distance_ms,
                "distance_m": distance_m,
                "path_nodes": path,
                "route_arc_ids": route_arc_ids,
                "weight_path_ids": weight_path_ids,
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

/// Color palette for multi-route visualization.
const ROUTE_COLORS: &[&str] = &[
    "#ff5500", "#0055ff", "#00aa44", "#aa00cc", "#cc8800", "#e6194b", "#3cb44b", "#4363d8",
    "#f58231", "#911eb4",
];

fn format_multi_response(
    answers: Vec<QueryAnswer>,
    format: Option<&str>,
    colors: bool,
    graph_type: &'static str,
) -> Value {
    match format {
        Some("json") => {
            let responses: Vec<QueryResponse> = answers
                .into_iter()
                .map(|a| answer_to_response(Some(a), graph_type))
                .collect();
            serde_json::to_value(responses).unwrap()
        }
        _ => answers_to_geojson(answers, colors, graph_type),
    }
}

fn answers_to_geojson(answers: Vec<QueryAnswer>, colors: bool, graph_type: &'static str) -> Value {
    if answers.is_empty() {
        return serde_json::json!({
            "type": "FeatureCollection",
            "features": []
        });
    }

    let features: Vec<Value> = answers
        .into_iter()
        .enumerate()
        .map(|(idx, a)| {
            let QueryAnswer {
                distance_ms,
                distance_m,
                route_arc_ids,
                weight_path_ids,
                path,
                coordinates,
                turns,
                origin,
                destination,
                snapped_origin: _,
                snapped_destination: _,
            } = a;

            let coords: Vec<[f32; 2]> = connect_query_coordinates(coordinates, origin, destination)
                .into_iter()
                .map(|(lat, lng)| [lng, lat])
                .collect();

            let mut props = serde_json::json!({
                "source": "hanoi_server",
                "export_version": 1,
                "graph_type": graph_type,
                "distance_ms": distance_ms,
                "distance_m": distance_m,
                "path_nodes": path,
                "route_arc_ids": route_arc_ids,
                "weight_path_ids": weight_path_ids,
                "route_index": idx,
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

