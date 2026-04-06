use rust_road_router::datastr::graph::{EdgeId, NodeId};
use serde::Serialize;

use crate::spatial::haversine_m;

const STRAIGHT_THRESHOLD_DEG: f64 = 15.0;
const SLIGHT_THRESHOLD_DEG: f64 = 40.0;
const SHARP_THRESHOLD_DEG: f64 = 110.0;
const U_TURN_THRESHOLD_DEG: f64 = 155.0;

/// Classification of a turn maneuver at an intersection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnDirection {
    Straight,
    SlightLeft,
    SlightRight,
    Left,
    Right,
    SharpLeft,
    SharpRight,
    UTurn,
    RoundaboutStraight,
    RoundaboutSlightLeft,
    RoundaboutSlightRight,
    RoundaboutLeft,
    RoundaboutRight,
    RoundaboutSharpLeft,
    RoundaboutSharpRight,
    RoundaboutUTurn,
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

fn with_roundabout_prefix(direction: TurnDirection, is_roundabout: bool) -> TurnDirection {
    if !is_roundabout {
        return direction;
    }

    match direction {
        TurnDirection::Straight => TurnDirection::RoundaboutStraight,
        TurnDirection::SlightLeft => TurnDirection::RoundaboutSlightLeft,
        TurnDirection::SlightRight => TurnDirection::RoundaboutSlightRight,
        TurnDirection::Left => TurnDirection::RoundaboutLeft,
        TurnDirection::Right => TurnDirection::RoundaboutRight,
        TurnDirection::SharpLeft => TurnDirection::RoundaboutSharpLeft,
        TurnDirection::SharpRight => TurnDirection::RoundaboutSharpRight,
        TurnDirection::UTurn => TurnDirection::RoundaboutUTurn,
        TurnDirection::RoundaboutStraight
        | TurnDirection::RoundaboutSlightLeft
        | TurnDirection::RoundaboutSlightRight
        | TurnDirection::RoundaboutLeft
        | TurnDirection::RoundaboutRight
        | TurnDirection::RoundaboutSharpLeft
        | TurnDirection::RoundaboutSharpRight
        | TurnDirection::RoundaboutUTurn => direction,
    }
}

fn is_roundabout_direction(direction: TurnDirection) -> bool {
    matches!(
        direction,
        TurnDirection::RoundaboutStraight
            | TurnDirection::RoundaboutSlightLeft
            | TurnDirection::RoundaboutSlightRight
            | TurnDirection::RoundaboutLeft
            | TurnDirection::RoundaboutRight
            | TurnDirection::RoundaboutSharpLeft
            | TurnDirection::RoundaboutSharpRight
            | TurnDirection::RoundaboutUTurn
    )
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

    let ax = (lng[head_a] as f64 - lng[tail_a] as f64) * cos_lat;
    let ay = lat[head_a] as f64 - lat[tail_a] as f64;

    let bx = (lng[head_b] as f64 - lng[head_a] as f64) * cos_lat;
    let by = lat[head_b] as f64 - lat[head_a] as f64;

    let dot = ax * bx + ay * by;
    let cross = ax * by - ay * bx;

    (cross.atan2(dot), cross)
}

/// Classify a turn angle into one of 16 directions (8 base + 8 roundabout).
pub fn classify_turn(angle_degrees: f64, cross: f64, is_roundabout: bool) -> TurnDirection {
    let abs_angle = angle_degrees.abs();

    let base = if abs_angle < STRAIGHT_THRESHOLD_DEG {
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
        TurnDirection::UTurn
    };

    with_roundabout_prefix(base, is_roundabout)
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
    is_arc_roundabout: &[u8],
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
        let is_roundabout = is_arc_roundabout[edge_a] != 0 || is_arc_roundabout[edge_b] != 0;
        let direction = classify_turn(angle_degrees, cross, is_roundabout);

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

fn collapse_degree2_subrun(subrun: &[TurnAnnotation], collapsed: &mut Vec<TurnAnnotation>) {
    if subrun.is_empty() {
        return;
    }

    let net_angle: f64 = subrun.iter().map(|turn| turn.angle_degrees).sum();
    let is_roundabout = subrun
        .iter()
        .any(|turn| is_roundabout_direction(turn.direction));

    if net_angle.abs() < STRAIGHT_THRESHOLD_DEG && !is_roundabout {
        return;
    }

    let last = subrun.last().expect("non-empty degree-2 subrun");
    let direction = classify_turn(net_angle, net_angle.signum(), is_roundabout);
    collapsed.push(TurnAnnotation {
        direction,
        angle_degrees: net_angle,
        coordinate_index: last.coordinate_index,
        distance_to_next_m: 0.0,
        intersection_degree: last.intersection_degree,
    });
}

/// Collapse degree-2 curvature noise using run-based analysis.
fn collapse_degree2_curves(turns: &mut Vec<TurnAnnotation>) {
    let input = std::mem::take(turns);
    let mut collapsed: Vec<TurnAnnotation> = Vec::with_capacity(input.len());
    let mut i = 0;

    while i < input.len() {
        if input[i].intersection_degree != 2 {
            collapsed.push(input[i].clone());
            i += 1;
            continue;
        }

        let run_start = i;
        while i < input.len() && input[i].intersection_degree == 2 {
            i += 1;
        }
        let run = &input[run_start..i];

        let mut run_idx = 0;
        while run_idx < run.len() {
            if run[run_idx].angle_degrees.abs() >= SLIGHT_THRESHOLD_DEG {
                collapsed.push(run[run_idx].clone());
                run_idx += 1;
                continue;
            }

            let subrun_start = run_idx;
            while run_idx < run.len() && run[run_idx].angle_degrees.abs() < SLIGHT_THRESHOLD_DEG {
                run_idx += 1;
            }
            collapse_degree2_subrun(&run[subrun_start..run_idx], &mut collapsed);
        }
    }

    *turns = collapsed;
}

/// Merge consecutive Straight entries into one.
fn merge_straights(turns: &mut Vec<TurnAnnotation>) {
    let input = std::mem::take(turns);
    let mut merged: Vec<TurnAnnotation> = Vec::with_capacity(input.len());
    let mut i = 0;

    while i < input.len() {
        if input[i].direction != TurnDirection::Straight {
            merged.push(input[i].clone());
            i += 1;
            continue;
        }

        let mut merged_straight = input[i].clone();
        let mut cumulative_angle = merged_straight.angle_degrees;
        i += 1;

        while i < input.len() && input[i].direction == TurnDirection::Straight {
            cumulative_angle += input[i].angle_degrees;
            i += 1;
        }

        merged_straight.angle_degrees = cumulative_angle;
        merged_straight.distance_to_next_m = 0.0;
        merged.push(merged_straight);
    }

    *turns = merged;
}

fn path_distance(coordinates: &[(f32, f32)], start: usize, end: usize) -> f64 {
    if coordinates.len() < 2 {
        return 0.0;
    }

    let last = coordinates.len() - 1;
    let start = start.min(last);
    let end = end.min(last);

    if start >= end {
        return 0.0;
    }

    let mut distance = 0.0;
    for idx in start..end {
        distance += haversine_m(
            coordinates[idx].0 as f64,
            coordinates[idx].1 as f64,
            coordinates[idx + 1].0 as f64,
            coordinates[idx + 1].1 as f64,
        );
    }

    distance
}

/// Populate distance_to_next_m via Haversine summation.
///
/// The first turn's distance spans from the route start (coordinate 0) to the
/// next turn's coordinate — this ensures the leading-straight distance covers
/// "head straight from trip start for X meters" rather than starting mid-route.
fn annotate_distances(turns: &mut Vec<TurnAnnotation>, coordinates: &[(f32, f32)]) {
    let route_end = coordinates.len().saturating_sub(1);

    for idx in 0..turns.len() {
        let start = if idx == 0 {
            0
        } else {
            turns[idx].coordinate_index as usize
        };
        let end = turns
            .get(idx + 1)
            .map(|turn| turn.coordinate_index as usize)
            .unwrap_or(route_end);
        turns[idx].distance_to_next_m = path_distance(coordinates, start, end);
    }
}

/// Strip interior Straights (not RoundaboutStraight); preserve leading Straight.
fn strip_straights(turns: &mut Vec<TurnAnnotation>) {
    let input = std::mem::take(turns);
    let mut stripped: Vec<TurnAnnotation> = Vec::with_capacity(input.len());
    let mut leading_straight: Option<TurnAnnotation> = None;

    for turn in input {
        if turn.direction == TurnDirection::Straight {
            if stripped.is_empty() {
                if let Some(existing) = leading_straight.as_mut() {
                    existing.angle_degrees += turn.angle_degrees;
                    existing.coordinate_index = turn.coordinate_index;
                    existing.distance_to_next_m += turn.distance_to_next_m;
                } else {
                    leading_straight = Some(turn);
                }
            } else if let Some(previous) = stripped.last_mut() {
                previous.distance_to_next_m += turn.distance_to_next_m;
            }
            continue;
        }

        if stripped.is_empty() {
            if let Some(leading) = leading_straight.take() {
                stripped.push(leading);
            }
        }

        stripped.push(turn);
    }

    if stripped.is_empty() {
        if let Some(leading) = leading_straight {
            stripped.push(leading);
        }
    }

    *turns = stripped;
}

/// Full post-processing pipeline (calls all four above in order).
pub fn refine_turns(turns: &mut Vec<TurnAnnotation>, coordinates: &[(f32, f32)]) {
    collapse_degree2_curves(turns);
    merge_straights(turns);
    annotate_distances(turns, coordinates);
    strip_straights(turns);

    if turns.is_empty() && !coordinates.is_empty() {
        turns.push(TurnAnnotation {
            direction: TurnDirection::Straight,
            angle_degrees: 0.0,
            coordinate_index: 0,
            distance_to_next_m: path_distance(coordinates, 0, coordinates.len().saturating_sub(1)),
            intersection_degree: 0,
        });
    }
}
