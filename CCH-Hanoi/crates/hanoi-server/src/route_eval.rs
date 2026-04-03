use rust_road_router::datastr::graph::{INFINITY, Weight};
use serde_json::{Map, Value};

use hanoi_core::cch::{CchContext, route_distance_m};
use hanoi_core::line_graph::LineGraphCchContext;

use crate::types::{EvaluateRouteInput, RouteEvaluationResult};

pub(crate) const MAX_ROUTE_EVALUATIONS: usize = 10;

pub(crate) enum RouteEvaluator {
    Normal(NormalRouteEvaluator),
    LineGraph(LineGraphRouteEvaluator),
}

impl RouteEvaluator {
    pub fn from_normal(context: &CchContext) -> Self {
        RouteEvaluator::Normal(NormalRouteEvaluator::new(context))
    }

    pub fn from_line_graph(context: &LineGraphCchContext) -> Self {
        RouteEvaluator::LineGraph(LineGraphRouteEvaluator::new(context))
    }

    pub fn graph_type(&self) -> &'static str {
        match self {
            RouteEvaluator::Normal { .. } => "normal",
            RouteEvaluator::LineGraph { .. } => "line_graph",
        }
    }

    pub fn evaluate_routes(
        &self,
        routes: &[EvaluateRouteInput],
        current_weights: &[Weight],
    ) -> Vec<RouteEvaluationResult> {
        routes
            .iter()
            .map(|route| match self {
                RouteEvaluator::Normal(evaluator) => evaluator.evaluate(route, current_weights),
                RouteEvaluator::LineGraph(evaluator) => evaluator.evaluate(route, current_weights),
            })
            .collect()
    }
}

pub(crate) struct NormalRouteEvaluator {
    first_out: Vec<u32>,
    head: Vec<u32>,
    arc_lengths_m: Vec<f64>,
}

impl NormalRouteEvaluator {
    fn new(context: &CchContext) -> Self {
        Self {
            first_out: context.graph.first_out.clone(),
            head: context.graph.head.clone(),
            arc_lengths_m: build_arc_lengths(
                &context.graph.first_out,
                &context.graph.head,
                &context.graph.latitude,
                &context.graph.longitude,
            ),
        }
    }

    fn evaluate(
        &self,
        input: &EvaluateRouteInput,
        current_weights: &[Weight],
    ) -> RouteEvaluationResult {
        let parsed = match ParsedRoute::from_geojson(input) {
            Ok(parsed) => parsed,
            Err(error) => return invalid_route_result(&input.name, error),
        };

        let mut issues = Vec::new();
        let (route_arc_ids, distance_mode) = self.resolve_arc_ids(&parsed, &mut issues);
        let distance_m = compute_distance(
            route_arc_ids.as_deref(),
            distance_mode,
            &parsed.geometry,
            &self.arc_lengths_m,
        );
        let travel_time_ms = route_arc_ids.as_deref().and_then(|arc_ids| {
            match sum_weighted_arcs(arc_ids, current_weights) {
                Ok(total) => Some(total),
                Err(error) => {
                    issues.push(error);
                    None
                }
            }
        });

        if travel_time_ms.is_none() {
            issues.push("Travel time could not be reconstructed from this GeoJSON on a normal graph server.".into());
        }

        RouteEvaluationResult {
            name: parsed.name,
            travel_time_ms,
            distance_m,
            geometry_point_count: parsed.geometry.len(),
            route_arc_count: route_arc_ids.as_ref().map_or(0, Vec::len),
            travel_time_mode: if travel_time_ms.is_some() {
                "normal_arc_sum"
            } else {
                "unavailable"
            },
            distance_mode,
            export_graph_type: parsed.export_graph_type,
            error: join_issues(issues),
        }
    }

    fn resolve_arc_ids(
        &self,
        parsed: &ParsedRoute,
        issues: &mut Vec<String>,
    ) -> (Option<Vec<u32>>, &'static str) {
        if let Some(route_arc_ids) = parsed.route_arc_ids.as_ref() {
            return match validate_arc_ids(route_arc_ids, self.arc_lengths_m.len()) {
                Ok(validated) => (Some(validated), "route_arc_ids"),
                Err(error) => {
                    issues.push(error);
                    (None, geometry_distance_mode(&parsed.geometry))
                }
            };
        }

        if let Some(path_nodes) = parsed.path_nodes.as_ref() {
            return match reconstruct_arc_ids(&self.first_out, &self.head, path_nodes) {
                Ok(arc_ids) => (Some(arc_ids), "path_nodes"),
                Err(error) => {
                    issues.push(error);
                    (None, geometry_distance_mode(&parsed.geometry))
                }
            };
        }

        (None, geometry_distance_mode(&parsed.geometry))
    }
}

pub(crate) struct LineGraphRouteEvaluator {
    original_first_out: Vec<u32>,
    original_head: Vec<u32>,
    original_arc_lengths_m: Vec<f64>,
    original_arc_id_of_lg_node: Vec<u32>,
    original_travel_time_by_lg_node: Vec<Weight>,
    line_graph_first_out: Vec<u32>,
    line_graph_head: Vec<u32>,
    baseline_original_arc_weights: Vec<Weight>,
    incoming_offsets: Vec<u32>,
    incoming_edges: Vec<u32>,
}

impl LineGraphRouteEvaluator {
    fn new(context: &LineGraphCchContext) -> Self {
        let original_edge_count = context.original_first_out.last().copied().unwrap_or(0) as usize;
        let baseline_original_arc_weights =
            context.original_travel_time[..original_edge_count].to_vec();
        let (incoming_offsets, incoming_edges) =
            build_incoming_index(&context.graph.head, original_edge_count);

        Self {
            original_first_out: context.original_first_out.clone(),
            original_head: context.original_head.clone(),
            original_arc_lengths_m: build_arc_lengths(
                &context.original_first_out,
                &context.original_head,
                &context.original_latitude,
                &context.original_longitude,
            ),
            original_arc_id_of_lg_node: context.original_arc_id_of_lg_node.clone(),
            original_travel_time_by_lg_node: context.original_travel_time.clone(),
            line_graph_first_out: context.graph.first_out.clone(),
            line_graph_head: context.graph.head.clone(),
            baseline_original_arc_weights,
            incoming_offsets,
            incoming_edges,
        }
    }

    fn evaluate(
        &self,
        input: &EvaluateRouteInput,
        current_weights: &[Weight],
    ) -> RouteEvaluationResult {
        let parsed = match ParsedRoute::from_geojson(input) {
            Ok(parsed) => parsed,
            Err(error) => return invalid_route_result(&input.name, error),
        };

        let mut issues = Vec::new();
        let (route_arc_ids, distance_mode) = self.resolve_arc_ids(&parsed, &mut issues);
        let distance_m = compute_distance(
            route_arc_ids.as_deref(),
            distance_mode,
            &parsed.geometry,
            &self.original_arc_lengths_m,
        );

        let (travel_time_ms, travel_time_mode) = if parsed.export_graph_type.as_deref()
            == Some("line_graph")
        {
            if let Some(weight_path_ids) = parsed.weight_path_ids.as_ref() {
                match self.sum_exact_line_graph_path(weight_path_ids, current_weights) {
                    Ok(total) => (Some(total), "exact_weight_path"),
                    Err(error) => {
                        issues.push(error);
                        self.pseudo_normal_fallback(
                            route_arc_ids.as_deref(),
                            current_weights,
                            &mut issues,
                        )
                    }
                }
            } else {
                self.pseudo_normal_fallback(route_arc_ids.as_deref(), current_weights, &mut issues)
            }
        } else {
            self.pseudo_normal_fallback(route_arc_ids.as_deref(), current_weights, &mut issues)
        };

        if travel_time_ms.is_none() {
            issues.push(
                "Travel time could not be reconstructed from this GeoJSON on a line-graph server."
                    .into(),
            );
        }

        RouteEvaluationResult {
            name: parsed.name,
            travel_time_ms,
            distance_m,
            geometry_point_count: parsed.geometry.len(),
            route_arc_count: route_arc_ids.as_ref().map_or(0, Vec::len),
            travel_time_mode,
            distance_mode,
            export_graph_type: parsed.export_graph_type,
            error: join_issues(issues),
        }
    }

    fn resolve_arc_ids(
        &self,
        parsed: &ParsedRoute,
        issues: &mut Vec<String>,
    ) -> (Option<Vec<u32>>, &'static str) {
        if let Some(route_arc_ids) = parsed.route_arc_ids.as_ref() {
            return match validate_arc_ids(route_arc_ids, self.original_arc_lengths_m.len()) {
                Ok(validated) => (Some(validated), "route_arc_ids"),
                Err(error) => {
                    issues.push(error);
                    (None, geometry_distance_mode(&parsed.geometry))
                }
            };
        }

        if let Some(path_nodes) = parsed.path_nodes.as_ref() {
            return match reconstruct_arc_ids(
                &self.original_first_out,
                &self.original_head,
                path_nodes,
            ) {
                Ok(arc_ids) => (Some(arc_ids), "path_nodes"),
                Err(error) => {
                    issues.push(error);
                    (None, geometry_distance_mode(&parsed.geometry))
                }
            };
        }

        if parsed.export_graph_type.as_deref() == Some("line_graph") {
            if let Some(weight_path_ids) = parsed.weight_path_ids.as_ref() {
                return match self.map_line_graph_nodes_to_original_arcs(weight_path_ids) {
                    Ok(arc_ids) => (Some(arc_ids), "weight_path_ids"),
                    Err(error) => {
                        issues.push(error);
                        (None, geometry_distance_mode(&parsed.geometry))
                    }
                };
            }
        }

        (None, geometry_distance_mode(&parsed.geometry))
    }

    fn pseudo_normal_fallback(
        &self,
        route_arc_ids: Option<&[u32]>,
        current_weights: &[Weight],
        issues: &mut Vec<String>,
    ) -> (Option<Weight>, &'static str) {
        match route_arc_ids {
            Some(arc_ids) => match self.sum_pseudo_normal_arcs(arc_ids, current_weights) {
                Ok(total) => (Some(total), "line_graph_pseudo_normal"),
                Err(error) => {
                    issues.push(error);
                    (None, "unavailable")
                }
            },
            None => (None, "unavailable"),
        }
    }

    fn sum_exact_line_graph_path(
        &self,
        weight_path_ids: &[u32],
        current_weights: &[Weight],
    ) -> Result<Weight, String> {
        if weight_path_ids.is_empty() {
            return Ok(0);
        }

        let first_node = weight_path_ids[0] as usize;
        let mut total = *self
            .original_travel_time_by_lg_node
            .get(first_node)
            .ok_or_else(|| {
                format!(
                    "weight_path_ids contains line-graph node {} outside the loaded dataset",
                    weight_path_ids[0]
                )
            })?;

        for window in weight_path_ids.windows(2) {
            let from = window[0] as usize;
            let to = window[1];
            let start = *self.line_graph_first_out.get(from).ok_or_else(|| {
                format!(
                    "weight_path_ids contains line-graph node {} outside the loaded dataset",
                    window[0]
                )
            })? as usize;
            let end = *self.line_graph_first_out.get(from + 1).ok_or_else(|| {
                format!(
                    "weight_path_ids contains line-graph node {} outside the loaded dataset",
                    window[0]
                )
            })? as usize;
            let edge_idx = (start..end)
                .find(|&edge_idx| self.line_graph_head[edge_idx] == to)
                .ok_or_else(|| {
                    format!(
                        "weight_path_ids contains an invalid transition from node {} to node {}",
                        window[0], window[1]
                    )
                })?;
            let weight = *current_weights.get(edge_idx).ok_or_else(|| {
                format!(
                    "active weight profile is missing transition edge {} required for imported route replay",
                    edge_idx
                )
            })?;
            total = total.saturating_add(weight);
        }

        Ok(total)
    }

    fn sum_pseudo_normal_arcs(
        &self,
        route_arc_ids: &[u32],
        current_weights: &[Weight],
    ) -> Result<Weight, String> {
        let mut total = 0u32;
        for &arc_id in route_arc_ids {
            total = total.saturating_add(self.pseudo_normal_arc_weight(arc_id, current_weights)?);
        }
        Ok(total)
    }

    fn pseudo_normal_arc_weight(
        &self,
        arc_id: u32,
        current_weights: &[Weight],
    ) -> Result<Weight, String> {
        let arc_idx = arc_id as usize;
        let fallback_weight =
            *self
                .baseline_original_arc_weights
                .get(arc_idx)
                .ok_or_else(|| {
                    format!(
                        "route_arc_ids contains arc {} outside the loaded original graph",
                        arc_id
                    )
                })?;
        let start = self.incoming_offsets[arc_idx] as usize;
        let end = self.incoming_offsets[arc_idx + 1] as usize;

        if start >= end {
            return Ok(fallback_weight);
        }

        let mut best = INFINITY;
        for &incoming_edge in &self.incoming_edges[start..end] {
            let candidate = *current_weights.get(incoming_edge as usize).ok_or_else(|| {
                format!(
                    "active weight profile is missing transition edge {} required for pseudo-normal evaluation",
                    incoming_edge
                )
            })?;
            best = best.min(candidate);
        }

        Ok(if best == INFINITY { INFINITY } else { best })
    }

    fn map_line_graph_nodes_to_original_arcs(
        &self,
        weight_path_ids: &[u32],
    ) -> Result<Vec<u32>, String> {
        let mut route_arc_ids = Vec::with_capacity(weight_path_ids.len());
        for &lg_node in weight_path_ids {
            let original_arc_id = *self
                .original_arc_id_of_lg_node
                .get(lg_node as usize)
                .ok_or_else(|| {
                    format!(
                        "weight_path_ids contains line-graph node {} outside the loaded dataset",
                        lg_node
                    )
                })?;
            route_arc_ids.push(original_arc_id);
        }
        Ok(route_arc_ids)
    }
}

struct ParsedRoute {
    name: String,
    export_graph_type: Option<String>,
    route_arc_ids: Option<Vec<u32>>,
    weight_path_ids: Option<Vec<u32>>,
    path_nodes: Option<Vec<u32>>,
    geometry: Vec<(f32, f32)>,
}

impl ParsedRoute {
    fn from_geojson(input: &EvaluateRouteInput) -> Result<Self, String> {
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

fn invalid_route_result(name: &str, error: String) -> RouteEvaluationResult {
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

fn build_arc_lengths(
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
            lengths.push(hanoi_core::spatial::haversine_m(
                latitudes[tail_node] as f64,
                longitudes[tail_node] as f64,
                latitudes[head_node] as f64,
                longitudes[head_node] as f64,
            ));
        }
    }
    lengths
}

fn build_incoming_index(head: &[u32], original_edge_count: usize) -> (Vec<u32>, Vec<u32>) {
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

fn validate_arc_ids(route_arc_ids: &[u32], max_arc_count: usize) -> Result<Vec<u32>, String> {
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

fn reconstruct_arc_ids(
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

fn sum_weighted_arcs(route_arc_ids: &[u32], weights: &[Weight]) -> Result<Weight, String> {
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

fn compute_distance(
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

fn geometry_distance_mode(geometry: &[(f32, f32)]) -> &'static str {
    if geometry.is_empty() {
        "unavailable"
    } else {
        "geometry"
    }
}

fn join_issues(issues: Vec<String>) -> Option<String> {
    if issues.is_empty() {
        None
    } else {
        Some(issues.join(" "))
    }
}
