use hanoi_core::geo::spatial::haversine_m;
use hanoi_core::routing::route_distance_m;
use rust_road_router::datastr::graph::Weight;
use serde_json::{Map, Value};

use crate::api::dto::{EvaluateRouteInput, RouteEvaluationResult};

pub(super) struct ParsedRoute {
    pub name: String,
    pub export_graph_type: Option<String>,
    pub route_arc_ids: Option<Vec<u32>>,
    pub weight_path_ids: Option<Vec<u32>>,
    pub path_nodes: Option<Vec<u32>>,
    pub geometry: Vec<(f32, f32)>,
}

impl ParsedRoute {
    pub(super) fn from_geojson(input: &EvaluateRouteInput) -> Result<Self, String> {
        let (geometry, properties) = extract_route_feature(&input.geojson)?;

        Ok(Self {
            name: input.name.clone(),
            export_graph_type: string_property(properties, "graph_type"),
            route_arc_ids: u32_array_property(properties, "route_arc_ids")?,
            weight_path_ids: u32_array_property(properties, "weight_path_ids")?,
            path_nodes: u32_array_property(properties, "path_nodes")?,
            geometry,
        })
    }
}

pub(super) fn invalid_route_result(name: &str, error: String) -> RouteEvaluationResult {
    RouteEvaluationResult {
        name: name.to_string(),
        travel_time_ms: None,
        distance_m: None,
        geometry_point_count: 0,
        route_arc_count: 0,
        travel_time_mode: "unavailable",
        distance_mode: "unavailable",
        export_graph_type: None,
        error: Some(error),
    }
}

fn extract_route_feature(
    value: &Value,
) -> Result<(Vec<(f32, f32)>, Option<&Map<String, Value>>), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "GeoJSON payload must be a JSON object.".to_string())?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "GeoJSON payload is missing a string field 'type'.".to_string())?;

    match kind {
        "FeatureCollection" => {
            let features = object
                .get("features")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    "GeoJSON FeatureCollection is missing an array field 'features'.".to_string()
                })?;

            for feature in features {
                if let Ok(parsed) = extract_feature(feature) {
                    return Ok(parsed);
                }
            }

            Err("GeoJSON FeatureCollection does not contain a LineString feature.".into())
        }
        "Feature" => extract_feature(value),
        "LineString" => extract_linestring_geometry(value).map(|geometry| (geometry, None)),
        other => Err(format!(
            "Unsupported GeoJSON type '{}'. Expected FeatureCollection, Feature, or LineString.",
            other
        )),
    }
}

fn extract_feature(
    value: &Value,
) -> Result<(Vec<(f32, f32)>, Option<&Map<String, Value>>), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "GeoJSON feature must be a JSON object.".to_string())?;
    let geometry_value = object
        .get("geometry")
        .ok_or_else(|| "GeoJSON feature is missing 'geometry'.".to_string())?;
    let geometry = extract_linestring_geometry(geometry_value)?;
    let properties = object.get("properties").and_then(Value::as_object);
    Ok((geometry, properties))
}

fn extract_linestring_geometry(value: &Value) -> Result<Vec<(f32, f32)>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "GeoJSON geometry must be a JSON object.".to_string())?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "GeoJSON geometry is missing a string field 'type'.".to_string())?;
    if kind != "LineString" {
        return Err(format!(
            "GeoJSON geometry type '{}' is not supported. Expected LineString.",
            kind
        ));
    }

    let coordinates = object
        .get("coordinates")
        .and_then(Value::as_array)
        .ok_or_else(|| "GeoJSON LineString is missing an array field 'coordinates'.".to_string())?;

    let mut geometry = Vec::with_capacity(coordinates.len());
    for (index, coordinate) in coordinates.iter().enumerate() {
        let pair = coordinate.as_array().ok_or_else(|| {
            format!(
                "GeoJSON coordinate at index {} is not an array [lng, lat].",
                index
            )
        })?;
        if pair.len() < 2 {
            return Err(format!(
                "GeoJSON coordinate at index {} must contain at least [lng, lat].",
                index
            ));
        }

        let lng = pair[0]
            .as_f64()
            .ok_or_else(|| format!("GeoJSON longitude at index {} is not numeric.", index))?;
        let lat = pair[1]
            .as_f64()
            .ok_or_else(|| format!("GeoJSON latitude at index {} is not numeric.", index))?;
        geometry.push((lat as f32, lng as f32));
    }

    Ok(geometry)
}

fn string_property(properties: Option<&Map<String, Value>>, key: &str) -> Option<String> {
    properties
        .and_then(|props| props.get(key))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn u32_array_property(
    properties: Option<&Map<String, Value>>,
    key: &str,
) -> Result<Option<Vec<u32>>, String> {
    let Some(value) = properties.and_then(|props| props.get(key)) else {
        return Ok(None);
    };
    let array = value.as_array().ok_or_else(|| {
        format!(
            "GeoJSON property '{}' must be an array of unsigned integers.",
            key
        )
    })?;

    let mut values = Vec::with_capacity(array.len());
    for (index, item) in array.iter().enumerate() {
        let raw = item.as_u64().ok_or_else(|| {
            format!(
                "GeoJSON property '{}' contains a non-integer value at index {}.",
                key, index
            )
        })?;
        let value = u32::try_from(raw).map_err(|_| {
            format!(
                "GeoJSON property '{}' contains value {} at index {} which exceeds u32.",
                key, raw, index
            )
        })?;
        values.push(value);
    }

    Ok(Some(values))
}

pub(super) fn build_arc_lengths(
    first_out: &[u32],
    head: &[u32],
    latitudes: &[f32],
    longitudes: &[f32],
) -> Vec<f64> {
    let mut lengths = Vec::with_capacity(head.len());
    for tail_node in 0..first_out.len().saturating_sub(1) {
        let start = first_out[tail_node] as usize;
        let end = first_out[tail_node + 1] as usize;
        for edge_idx in start..end {
            let head_node = head[edge_idx] as usize;
            lengths.push(haversine_m(
                latitudes[tail_node] as f64,
                longitudes[tail_node] as f64,
                latitudes[head_node] as f64,
                longitudes[head_node] as f64,
            ));
        }
    }
    lengths
}

pub(super) fn build_incoming_index(
    head: &[u32],
    original_edge_count: usize,
) -> (Vec<u32>, Vec<u32>) {
    let mut incoming_counts = vec![0u32; original_edge_count];
    for &target in head {
        let target = target as usize;
        if target < original_edge_count {
            incoming_counts[target] += 1;
        }
    }

    let mut incoming_offsets = Vec::with_capacity(original_edge_count + 1);
    incoming_offsets.push(0);
    for count in &incoming_counts {
        let next = incoming_offsets.last().copied().unwrap_or(0) + count;
        incoming_offsets.push(next);
    }

    let mut incoming_edges = vec![0u32; incoming_offsets.last().copied().unwrap_or(0) as usize];
    let mut write_cursor = incoming_offsets[..original_edge_count].to_vec();
    for (edge_idx, &target) in head.iter().enumerate() {
        let target = target as usize;
        if target >= original_edge_count {
            continue;
        }
        let cursor = &mut write_cursor[target];
        incoming_edges[*cursor as usize] = edge_idx as u32;
        *cursor += 1;
    }

    (incoming_offsets, incoming_edges)
}

pub(super) fn validate_arc_ids(
    route_arc_ids: &[u32],
    max_arc_count: usize,
) -> Result<Vec<u32>, String> {
    for &arc_id in route_arc_ids {
        if arc_id as usize >= max_arc_count {
            return Err(format!(
                "route_arc_ids contains arc {} outside the loaded graph",
                arc_id
            ));
        }
    }
    Ok(route_arc_ids.to_vec())
}

pub(super) fn reconstruct_arc_ids(
    first_out: &[u32],
    head: &[u32],
    path_nodes: &[u32],
) -> Result<Vec<u32>, String> {
    if path_nodes.len() < 2 {
        return Ok(Vec::new());
    }

    let mut arc_ids = Vec::with_capacity(path_nodes.len() - 1);
    for window in path_nodes.windows(2) {
        let tail = window[0] as usize;
        let target = window[1];
        let start = *first_out.get(tail).ok_or_else(|| {
            format!(
                "path_nodes contains node {} outside the loaded graph",
                window[0]
            )
        })? as usize;
        let end = *first_out.get(tail + 1).ok_or_else(|| {
            format!(
                "path_nodes contains node {} outside the loaded graph",
                window[0]
            )
        })? as usize;
        let edge_idx = (start..end)
            .find(|&edge_idx| head[edge_idx] == target)
            .ok_or_else(|| {
                format!(
                    "path_nodes contains an invalid transition from node {} to node {}",
                    window[0], window[1]
                )
            })?;
        arc_ids.push(edge_idx as u32);
    }

    Ok(arc_ids)
}

pub(super) fn sum_weighted_arcs(
    route_arc_ids: &[u32],
    weights: &[Weight],
) -> Result<Weight, String> {
    let mut total = 0u32;
    for &arc_id in route_arc_ids {
        let weight = *weights.get(arc_id as usize).ok_or_else(|| {
            format!(
                "route_arc_ids contains arc {} outside the active weight vector",
                arc_id
            )
        })?;
        total = total.saturating_add(weight);
    }
    Ok(total)
}

pub(super) fn compute_distance(
    route_arc_ids: Option<&[u32]>,
    distance_mode: &'static str,
    geometry: &[(f32, f32)],
    arc_lengths_m: &[f64],
) -> Option<f64> {
    match route_arc_ids {
        Some(route_arc_ids)
            if matches!(
                distance_mode,
                "route_arc_ids" | "path_nodes" | "weight_path_ids"
            ) =>
        {
            let mut total = 0.0;
            for &arc_id in route_arc_ids {
                total += arc_lengths_m[arc_id as usize];
            }
            Some(total)
        }
        _ if !geometry.is_empty() => Some(route_distance_m(geometry)),
        _ => None,
    }
}

pub(super) fn geometry_distance_mode(geometry: &[(f32, f32)]) -> &'static str {
    if geometry.is_empty() {
        "unavailable"
    } else {
        "geometry"
    }
}

pub(super) fn join_issues(issues: Vec<String>) -> Option<String> {
    if issues.is_empty() {
        None
    } else {
        Some(issues.join(" "))
    }
}
