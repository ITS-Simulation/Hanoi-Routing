use rust_road_router::datastr::graph::NodeId;
use serde::Serialize;

const STRAIGHT_THRESHOLD_DEG: f64 = 25.0;
const U_TURN_THRESHOLD_DEG: f64 = 155.0;
const S_CURVE_NET_THRESHOLD_DEG: f64 = 15.0;
const S_CURVE_MAX_THRESHOLD_DEG: f64 = 60.0;

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
    /// Range: [-180, 180]. For merged straights, this is the cumulative sum.
    pub angle_degrees: f64,

    /// Number of original line-graph transitions this entry spans.
    pub edge_count: u32,

    /// Index into the output `coordinates[]` array where this maneuver occurs.
    /// For a turn, this is the intersection node. For a merged straight, this
    /// is the last intersection in the straight run.
    pub coordinate_index: u32,
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

/// Compute turn annotations for a line-graph path.
///
/// Each entry corresponds to one transition from `lg_path[i]` to `lg_path[i+1]`.
pub fn compute_turns(
    lg_path: &[NodeId],
    original_tail: &[NodeId],
    original_head: &[NodeId],
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
            head_a,
            original_tail[edge_b],
            "line graph invariant violated: edge {} head ({}) != edge {} tail ({})",
            edge_a,
            head_a,
            edge_b,
            original_tail[edge_b]
        );

        let (angle_radians, cross) =
            compute_turn_angle(tail_a, head_a, head_b, original_lat, original_lng);
        let angle_degrees = angle_radians.to_degrees();
        let direction = classify_turn(angle_degrees, cross);

        turns.push(TurnAnnotation {
            direction,
            angle_degrees,
            edge_count: 1,
            coordinate_index: 0, // placeholder — assigned by merge_straights
        });
    }

    turns
}

/// Pass 1: Replace adjacent opposite-sign turn pairs that are geometric
/// artifacts with a single straight carrying the residual angle.
///
/// A pair is cancelled when:
/// - Both entries are non-straight
/// - They have opposite signs (one left, one right)
/// - The net residual angle is below `S_CURVE_NET_THRESHOLD_DEG` (15°)
/// - Neither individual angle exceeds `S_CURVE_MAX_THRESHOLD_DEG` (60°)
pub fn cancel_s_curves(turns: Vec<TurnAnnotation>) -> Vec<TurnAnnotation> {
    if turns.len() < 2 {
        return turns;
    }

    let mut result = Vec::with_capacity(turns.len());
    let mut i = 0;

    while i < turns.len() - 1 {
        let a = &turns[i];
        let b = &turns[i + 1];

        let both_non_straight = a.direction != TurnDirection::Straight && b.direction != TurnDirection::Straight;
        let opposite_signs = a.angle_degrees * b.angle_degrees < 0.0;
        let net = a.angle_degrees + b.angle_degrees;
        let max_individual = a.angle_degrees.abs().max(b.angle_degrees.abs());

        if both_non_straight
            && opposite_signs
            && net.abs() < S_CURVE_NET_THRESHOLD_DEG
            && max_individual < S_CURVE_MAX_THRESHOLD_DEG
        {
            result.push(TurnAnnotation {
                direction: TurnDirection::Straight,
                angle_degrees: net,
                edge_count: 2,
                coordinate_index: 0, // placeholder — assigned by merge_straights
            });
            i += 2; // skip the consumed pair
        } else {
            result.push(turns[i].clone());
            i += 1;
        }
    }

    // Emit final element if not consumed by a pair cancellation
    if i < turns.len() {
        result.push(turns[i].clone());
    }

    result
}

/// Pass 2: Collapse consecutive straight entries into a single entry with
/// cumulative angle and edge count. Assigns `coordinate_index` to every entry.
pub fn merge_straights(turns: Vec<TurnAnnotation>) -> Vec<TurnAnnotation> {
    let mut result = Vec::new();
    let mut i = 0;
    let mut raw_index: u32 = 0;

    while i < turns.len() {
        if turns[i].direction == TurnDirection::Straight {
            // Start a merge run
            let mut cumulative_angle = 0.0_f64;
            let mut total_edges = 0_u32;

            while i < turns.len() && turns[i].direction == TurnDirection::Straight {
                cumulative_angle += turns[i].angle_degrees;
                total_edges += turns[i].edge_count;
                i += 1;
            }

            raw_index += total_edges;
            result.push(TurnAnnotation {
                direction: TurnDirection::Straight,
                angle_degrees: cumulative_angle,
                edge_count: total_edges,
                coordinate_index: raw_index,
            });
        } else {
            // Non-straight turn — emit as-is with its coordinate_index
            raw_index += turns[i].edge_count;
            result.push(TurnAnnotation {
                direction: turns[i].direction,
                angle_degrees: turns[i].angle_degrees,
                edge_count: turns[i].edge_count,
                coordinate_index: raw_index,
            });
            i += 1;
        }
    }

    result
}

/// Convenience wrapper: chains S-curve cancellation then straight merging.
pub fn refine_turns(turns: Vec<TurnAnnotation>) -> Vec<TurnAnnotation> {
    merge_straights(cancel_s_curves(turns))
}
