use rust_road_router::datastr::graph::{INFINITY, Weight};
use rust_road_router::util::Storage;

use hanoi_core::LineGraphCchContext;

use crate::api::dto::{EvaluateRouteInput, RouteEvaluationResult};

use super::parser::{
    ParsedRoute, build_arc_lengths, build_incoming_index, compute_distance, geometry_distance_mode,
    invalid_route_result, join_issues, reconstruct_arc_ids, validate_arc_ids,
};

pub(crate) struct LineGraphRouteEvaluator {
    original_first_out: Storage<u32>,
    original_head: Storage<u32>,
    original_arc_lengths_m: Vec<f64>,
    original_arc_id_of_lg_node: Storage<u32>,
    line_graph_first_out: Storage<u32>,
    line_graph_head: Storage<u32>,
    baseline_original_arc_weights: Storage<Weight>,
    incoming_offsets: Vec<u32>,
    incoming_edges: Vec<u32>,
}

impl LineGraphRouteEvaluator {
    pub(crate) fn new(context: &LineGraphCchContext) -> Self {
        let original_edge_count = context.original_first_out.last().copied().unwrap_or(0) as usize;
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
            line_graph_first_out: context.graph.first_out.clone(),
            line_graph_head: context.graph.head.clone(),
            baseline_original_arc_weights: context.original_travel_time.clone(),
            incoming_offsets,
            incoming_edges,
        }
    }

    pub(crate) fn evaluate(
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
        let first_arc = *self
            .original_arc_id_of_lg_node
            .get(first_node)
            .ok_or_else(|| {
                format!(
                    "weight_path_ids contains line-graph node {} outside the loaded dataset",
                    weight_path_ids[0]
                )
            })?;
        let mut total = *self
            .baseline_original_arc_weights
            .get(first_arc as usize)
            .ok_or_else(|| {
                format!(
                    "weight_path_ids first node {} maps to original arc {} outside the loaded dataset",
                    weight_path_ids[0], first_arc
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
