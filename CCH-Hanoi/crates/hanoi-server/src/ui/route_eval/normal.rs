use rust_road_router::datastr::graph::Weight;
use rust_road_router::util::Storage;

use hanoi_core::CchContext;

use crate::api::dto::{EvaluateRouteInput, RouteEvaluationResult};

use super::parser::{
    ParsedRoute, build_arc_lengths, compute_distance, geometry_distance_mode, invalid_route_result,
    join_issues, reconstruct_arc_ids, sum_weighted_arcs, validate_arc_ids,
};

pub(crate) struct NormalRouteEvaluator {
    first_out: Storage<u32>,
    head: Storage<u32>,
    arc_lengths_m: Vec<f64>,
}

impl NormalRouteEvaluator {
    pub(crate) fn new(context: &CchContext) -> Self {
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
