use rust_road_router::datastr::graph::{EdgeId, NodeId};

/// Result of snapping a coordinate to the nearest edge in the graph.
#[derive(Clone, Copy, Debug)]
pub struct SnapResult {
    /// The closest edge index.
    pub edge_id: EdgeId,
    /// Source node of the closest edge.
    pub tail: NodeId,
    /// Target node of the closest edge.
    pub head: NodeId,
    /// Projection parameter along the full edge geometry: 0.0 = at tail,
    /// 1.0 = at head.
    pub t: f64,
    /// Haversine distance in meters from the query point to the snapped point.
    pub snap_distance_m: f64,
    /// Latitude of the projected snap point.
    pub projected_lat: f32,
    /// Longitude of the projected snap point.
    pub projected_lng: f32,
}

impl SnapResult {
    /// The nearest endpoint node based on the projection parameter.
    /// t < 0.5 → closer to tail, t >= 0.5 → closer to head.
    pub fn nearest_node(&self) -> NodeId {
        if self.t < 0.5 { self.tail } else { self.head }
    }

    /// The geographic point on the edge closest to the query point.
    pub fn projected_point(&self, _lat: &[f32], _lng: &[f32]) -> (f32, f32) {
        (self.projected_lat, self.projected_lng)
    }
}
