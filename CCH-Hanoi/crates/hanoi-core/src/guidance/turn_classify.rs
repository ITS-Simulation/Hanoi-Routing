use rust_road_router::datastr::graph::NodeId;

use crate::guidance::TurnDirection;

pub(crate) const STRAIGHT_THRESHOLD_DEG: f64 = 20.0;
pub(crate) const SLIGHT_THRESHOLD_DEG: f64 = 40.0;
pub(crate) const SHARP_THRESHOLD_DEG: f64 = 110.0;
pub(crate) const U_TURN_THRESHOLD_DEG: f64 = 155.0;

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

    let ax = (lng[head_a] as f64 - lng[tail_a] as f64) * cos_lat;
    let ay = lat[head_a] as f64 - lat[tail_a] as f64;

    let bx = (lng[head_b] as f64 - lng[head_a] as f64) * cos_lat;
    let by = lat[head_b] as f64 - lat[head_a] as f64;

    let dot = ax * bx + ay * by;
    let cross = ax * by - ay * bx;

    (cross.atan2(dot), cross)
}

/// Classify a turn angle into a direction.
pub fn classify_turn(angle_degrees: f64, cross: f64) -> TurnDirection {
    let abs_angle = angle_degrees.abs();

    if abs_angle < STRAIGHT_THRESHOLD_DEG {
        TurnDirection::Straight
    } else if abs_angle < SLIGHT_THRESHOLD_DEG {
        if cross > 0.0 {
            TurnDirection::SlightLeft
        } else if cross < 0.0 {
            TurnDirection::SlightRight
        } else {
            TurnDirection::Straight
        }
    } else if abs_angle < SHARP_THRESHOLD_DEG {
        if cross > 0.0 {
            TurnDirection::Left
        } else if cross < 0.0 {
            TurnDirection::Right
        } else {
            TurnDirection::Straight
        }
    } else if abs_angle < U_TURN_THRESHOLD_DEG {
        if cross > 0.0 {
            TurnDirection::SharpLeft
        } else if cross < 0.0 {
            TurnDirection::SharpRight
        } else {
            TurnDirection::Straight
        }
    } else {
        // In right-hand traffic, U-turns are always left turns (crossing
        // oncoming lanes). A ≥155° right deviation is just a sharp right.
        if cross >= 0.0 {
            TurnDirection::UTurn
        } else {
            TurnDirection::SharpRight
        }
    }
}
