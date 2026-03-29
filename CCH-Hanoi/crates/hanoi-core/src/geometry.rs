use rust_road_router::datastr::graph::{EdgeId, NodeId};
use serde::Serialize;

const STRAIGHT_THRESHOLD_DEG: f64 = 25.0;
const U_TURN_THRESHOLD_DEG: f64 = 155.0;

/// Classification of a turn maneuver at an intersection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnDirection {
    Straight,
    Left,
    Right,
    UTurn,
}

/// A single turn annotation along a route.
#[derive(Debug, Clone, Serialize)]
pub struct TurnAnnotation {
    /// Classified turn direction.
    pub direction: TurnDirection,

    /// Signed turn angle in degrees. Positive = left, negative = right.
    /// Range: [-180, 180].
    pub angle_degrees: f64,

    /// Index into the output `coordinates[]` array where this maneuver occurs.
    #[serde(skip)]
    pub coordinate_index: u32,

    /// Distance in meters from this maneuver to the next maneuver (or to the
    /// route end for the last entry).
    pub distance_to_next_m: f64,

    /// Degree of the intersection node in the original graph (outgoing edges).
    #[serde(skip)]
    pub intersection_degree: u32,
}

/// Compute the signed turn angle between segment A (`tail_a -> head_a`) and
/// segment B (`head_a -> head_b`) in a local equirectangular projection.
///
/// Returns `(angle_radians, cross_product)`.
pub fn compute_turn_angle(
    tail_a: NodeId,
    head_a: NodeId,
    head_b: NodeId,
    lat: &[f32],
    lng: &[f32],
) -> (f64, f64) {
    let tail_a = tail_a as usize;
    let head_a = head_a as usize;
    let head_b = head_b as usize;

    let turn_lat = lat[head_a] as f64;
    let cos_lat = turn_lat.to_radians().cos();

    // Promote to f64 before subtraction to preserve precision.
    // f32 subtraction at lng ~105° loses ~12m of resolution per coordinate,
    // which can produce multi-degree angle errors on short urban segments.
    let ax = (lng[head_a] as f64 - lng[tail_a] as f64) * cos_lat;
    let ay = lat[head_a] as f64 - lat[tail_a] as f64;

    let bx = (lng[head_b] as f64 - lng[head_a] as f64) * cos_lat;
    let by = lat[head_b] as f64 - lat[head_a] as f64;

    let dot = ax * bx + ay * by;
    let cross = ax * by - ay * bx;

    (cross.atan2(dot), cross)
}

/// Classify a signed turn angle using OSRM/Valhalla-style thresholds.
pub fn classify_turn(angle_degrees: f64, cross: f64) -> TurnDirection {
    let abs_angle = angle_degrees.abs();

    if abs_angle < STRAIGHT_THRESHOLD_DEG {
        TurnDirection::Straight
    } else if abs_angle >= U_TURN_THRESHOLD_DEG {
        TurnDirection::UTurn
    } else if cross > 0.0 {
        TurnDirection::Left
    } else if cross < 0.0 {
        TurnDirection::Right
    } else {
        TurnDirection::Straight
    }
}

/// Compute raw turn annotations for a line-graph path.
///
/// Each entry corresponds to one transition from `lg_path[i]` to `lg_path[i+1]`.
/// `original_first_out` is the CSR offset array of the original graph, used to
/// compute the intersection node degree.
pub fn compute_turns(
    lg_path: &[NodeId],
    original_tail: &[NodeId],
    original_head: &[NodeId],
    original_first_out: &[EdgeId],
    original_lat: &[f32],
    original_lng: &[f32],
) -> Vec<TurnAnnotation> {
    let mut turns = Vec::new();

    for i in 0..lg_path.len().saturating_sub(1) {
        let edge_a = lg_path[i] as usize;
        let edge_b = lg_path[i + 1] as usize;

        let tail_a = original_tail[edge_a];
        let head_a = original_head[edge_a];
        let head_b = original_head[edge_b];

        debug_assert_eq!(
            head_a, original_tail[edge_b],
            "line graph invariant violated: edge {} head ({}) != edge {} tail ({})",
            edge_a, head_a, edge_b, original_tail[edge_b]
        );

        let (angle_radians, cross) =
            compute_turn_angle(tail_a, head_a, head_b, original_lat, original_lng);
        let angle_degrees = angle_radians.to_degrees();
        let direction = classify_turn(angle_degrees, cross);

        // Outgoing degree of the shared intersection node (head_a).
        let node = head_a as usize;
        let intersection_degree = original_first_out[node + 1] - original_first_out[node];

        turns.push(TurnAnnotation {
            direction,
            angle_degrees,
            coordinate_index: (i + 1) as u32,
            distance_to_next_m: 0.0,
            intersection_degree,
        });
    }

    turns
}
