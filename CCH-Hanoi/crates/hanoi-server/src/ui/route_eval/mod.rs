mod line_graph;
mod normal;
mod parser;

use rust_road_router::datastr::graph::Weight;

use hanoi_core::{CchContext, LineGraphCchContext};

use crate::api::dto::{EvaluateRouteInput, RouteEvaluationResult};

use self::line_graph::LineGraphRouteEvaluator;
use self::normal::NormalRouteEvaluator;

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
