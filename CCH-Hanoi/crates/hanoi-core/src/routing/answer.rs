use rust_road_router::datastr::graph::{NodeId, Weight};
use crate::CoordRejection;
use crate::geo::spatial::haversine_m;
use crate::guidance::TurnAnnotation;

/// Answer from a shortest-path query.
pub struct QueryAnswer {
    /// Total travel time in milliseconds.
    pub distance_ms: Weight,
    /// Total route distance in meters (Haversine sum of coordinate path).
    pub distance_m: f64,
    /// Ordered sequence of original graph arc IDs traversed by the route.
    pub route_arc_ids: Vec<u32>,
    /// Ordered sequence of graph-specific path IDs used to replay this route
    /// under the active weight space. For normal graphs this is identical to
    /// `route_arc_ids`; for line graphs it carries the line-graph node path.
    pub weight_path_ids: Vec<u32>,
    /// Ordered sequence of node IDs along the shortest path.
    pub path: Vec<NodeId>,
    /// Expanded (lat, lng) route geometry. When shape points are present this
    /// includes modelling nodes, and coordinate queries may also prepend/append
    /// snap-edge connector points near the route endpoints.
    pub coordinates: Vec<(f32, f32)>,
    /// Turn annotations along the path.
    /// Empty for normal-graph queries (no turn info available).
    pub turns: Vec<TurnAnnotation>,
    /// User-supplied origin coordinate (set by coordinate queries).
    pub origin: Option<(f32, f32)>,
    /// User-supplied destination coordinate (set by coordinate queries).
    pub destination: Option<(f32, f32)>,
    /// Projected origin point on the snapped edge.
    pub snapped_origin: Option<(f32, f32)>,
    /// Projected destination point on the snapped edge.
    pub snapped_destination: Option<(f32, f32)>,
}

/// Sum Haversine distances between consecutive coordinates to get total route
/// distance in meters.
pub fn route_distance_m(coordinates: &[(f32, f32)]) -> f64 {
    coordinates
        .windows(2)
        .map(|w| haversine_m(w[0].0 as f64, w[0].1 as f64, w[1].0 as f64, w[1].1 as f64))
        .sum()
}

pub trait QueryRepository {
    fn run_query(
        &mut self,
        from: u32,
        to: u32
    ) -> Option<QueryAnswer>;

    fn run_query_coords(
        &mut self,
        from: (f32, f32),
        to: (f32, f32)
    ) -> Result<Option<QueryAnswer>, CoordRejection>;
}


