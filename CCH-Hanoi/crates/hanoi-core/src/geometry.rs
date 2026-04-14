use rust_road_router::datastr::graph::{EdgeId, NodeId};
use serde::Serialize;

use crate::spatial::haversine_m;

const STRAIGHT_THRESHOLD_DEG: f64 = 20.0;
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
    /// Entering a roundabout.
    RoundaboutEnter,
    /// Exit roundabout going straight through.
    RoundaboutExitStraight,
    /// Exit roundabout to the right (first exit in right-hand traffic).
    RoundaboutExitRight,
    /// Exit roundabout to the left (go around most of the ring).
    RoundaboutExitLeft,
    /// Exit roundabout back the way you came (full loop).
    RoundaboutExitUturn,
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

    /// Whether this turn involves a roundabout arc (used for roundabout grouping).
    #[serde(skip)]
    pub is_roundabout: bool,
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

/// Compute raw turn annotations for a line-graph path.
///
/// Each entry corresponds to one transition from `lg_path[i]` to `lg_path[i+1]`.
/// `original_first_out` is the CSR offset array of the original graph, used to
/// compute the intersection node degree.
pub fn compute_turns(
    lg_path: &[NodeId],
    original_arc_id_of_lg_node: &[u32],
    original_tail: &[NodeId],
    original_head: &[NodeId],
    original_first_out: &[EdgeId],
    original_lat: &[f32],
    original_lng: &[f32],
    is_arc_roundabout: &[u8],
) -> Vec<TurnAnnotation> {
    let mut turns = Vec::new();

    for i in 0..lg_path.len().saturating_sub(1) {
        let edge_a = original_arc_id_of_lg_node[lg_path[i] as usize] as usize;
        let edge_b = original_arc_id_of_lg_node[lg_path[i + 1] as usize] as usize;

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
        let direction = classify_turn(angle_degrees, cross);

        let node = head_a as usize;
        let intersection_degree = original_first_out[node + 1] - original_first_out[node];

        turns.push(TurnAnnotation {
            direction,
            angle_degrees,
            coordinate_index: (i + 1) as u32,
            distance_to_next_m: 0.0,
            intersection_degree,
            is_roundabout,
        });
    }

    resolve_slight_turns(
        &mut turns,
        lg_path,
        original_arc_id_of_lg_node,
        original_tail,
        original_head,
        original_first_out,
        original_lat,
        original_lng,
    );

    turns
}

/// Maximum angular gap (degrees) between the route exit and the closest
/// alternative for the intersection to be considered ambiguous.  Below this
/// gap, the driver could easily take the wrong fork → emit slight turn.
const SLIGHT_FORK_GAP_DEG: f64 = 20.0;

/// Resolve slight turns: only emit SlightLeft/SlightRight when the intersection
/// has a competing fork close in angle (ambiguous fork).  Otherwise demote to
/// Straight.
fn resolve_slight_turns(
    turns: &mut [TurnAnnotation],
    lg_path: &[NodeId],
    original_arc_id_of_lg_node: &[u32],
    original_tail: &[NodeId],
    original_head: &[NodeId],
    original_first_out: &[EdgeId],
    original_lat: &[f32],
    original_lng: &[f32],
) {
    for turn in turns.iter_mut() {
        if turn.intersection_degree < 3 {
            continue;
        }

        if !matches!(
            turn.direction,
            TurnDirection::Straight | TurnDirection::SlightLeft | TurnDirection::SlightRight
        ) {
            continue;
        }

        let lg_idx = turn.coordinate_index.saturating_sub(1) as usize;
        if lg_idx + 1 >= lg_path.len() {
            continue;
        }

        let edge_a = original_arc_id_of_lg_node[lg_path[lg_idx] as usize] as usize;
        let edge_b = original_arc_id_of_lg_node[lg_path[lg_idx + 1] as usize] as usize;
        let intersection_node = original_head[edge_a];
        let start = original_first_out[intersection_node as usize] as usize;
        let end = original_first_out[intersection_node as usize + 1] as usize;
        let tail_a = original_tail[edge_a];

        // Find the nearest alternative exit by angular gap to our route exit
        let nearest_gap = (start..end)
            .filter(|&arc| arc != edge_b)
            .map(|arc| {
                let head_alt = original_head[arc];
                let (angle_radians, _) = compute_turn_angle(
                    tail_a,
                    intersection_node,
                    head_alt,
                    original_lat,
                    original_lng,
                );
                let alt_deg = angle_radians.to_degrees();
                (alt_deg, (turn.angle_degrees - alt_deg).abs())
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));

        if let Some((alt_angle, gap)) = nearest_gap {
            if gap < SLIGHT_FORK_GAP_DEG {
                let go_left = turn.angle_degrees > alt_angle;
                turn.direction = if go_left {
                    TurnDirection::SlightLeft
                } else {
                    TurnDirection::SlightRight
                };
                continue;
            }
        }

        turn.direction = TurnDirection::Straight;
    }
}

fn collapse_degree2_subrun(_subrun: &[TurnAnnotation], _collapsed: &mut Vec<TurnAnnotation>) {
    // Degree-2 nodes have no alternative exits — the driver has no choice.
    // All degree-2 curvature (road bends and roundabout ring segments) is
    // silently absorbed. Roundabout entry/exit happen at degree ≥ 3 nodes.
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
        let dir = input[i].direction;

        let mut merged_entry = input[i].clone();
        let mut cumulative_angle = merged_entry.angle_degrees;
        i += 1;

        while i < input.len() && input[i].direction == dir {
            cumulative_angle += input[i].angle_degrees;
            i += 1;
        }

        merged_entry.angle_degrees = cumulative_angle;
        merged_entry.distance_to_next_m = 0.0;
        merged.push(merged_entry);
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
pub(crate) fn annotate_distances(turns: &mut Vec<TurnAnnotation>, coordinates: &[(f32, f32)]) {
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

/// Strip interior Straights; preserve leading Straight.
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

/// Maximum distance (meters) between two consecutive turns for them to be
/// collapsed as a compound via-way turn. Median breaks, short connectors,
/// and service roads in Hanoi are typically < 30m; 40m provides margin.
const COMPOUND_TURN_MAX_DISTANCE_M: f64 = 50.0;

/// Collapse compound via-way turns: two consecutive same-direction turns
/// through a short connector segment.
///
/// In right-hand traffic:
/// - right + right → single Right/SharpRight (via-way right turn)
/// - left + left  → UTurn (median break; the only physical via-way left)
fn collapse_compound_turns(turns: &mut Vec<TurnAnnotation>, coordinates: &[(f32, f32)]) {
    if turns.len() < 2 {
        return;
    }

    let input = std::mem::take(turns);
    let mut result: Vec<TurnAnnotation> = Vec::with_capacity(input.len());
    let mut i = 0;

    while i < input.len() {
        if i + 1 < input.len() {
            let a = &input[i];
            let b = &input[i + 1];

            let both_left = a.angle_degrees > 0.0 && b.angle_degrees > 0.0;
            let both_right = a.angle_degrees < 0.0 && b.angle_degrees < 0.0;
            let both_substantial = a.angle_degrees.abs() >= STRAIGHT_THRESHOLD_DEG
                && b.angle_degrees.abs() >= STRAIGHT_THRESHOLD_DEG;
            // Use coordinate-based distance between the two turn nodes,
            // not distance_to_next_m which may be inflated by absorbed Straights.
            let connector_dist = path_distance(
                coordinates,
                a.coordinate_index as usize,
                b.coordinate_index as usize,
            );
            let close_enough = connector_dist <= COMPOUND_TURN_MAX_DISTANCE_M;

            if both_substantial && close_enough {
                let sum = a.angle_degrees + b.angle_degrees;

                // Via-way right turn: right + right → single right, but only
                // if the sum stays within one-turn range (≤ 110°). Two 90°
                // rights summing to 180° are two distinct turns, not one.
                if both_right && sum.abs() <= SHARP_THRESHOLD_DEG {
                    result.push(TurnAnnotation {
                        direction: classify_turn(sum, -1.0),
                        angle_degrees: sum,
                        coordinate_index: a.coordinate_index,
                        distance_to_next_m: a.distance_to_next_m + b.distance_to_next_m,
                        intersection_degree: a.intersection_degree,
                        is_roundabout: a.is_roundabout || b.is_roundabout,
                    });
                    i += 2;
                    continue;
                }

                if both_left && sum.abs() >= U_TURN_THRESHOLD_DEG {
                    result.push(TurnAnnotation {
                        direction: TurnDirection::UTurn,
                        angle_degrees: sum,
                        coordinate_index: a.coordinate_index,
                        distance_to_next_m: a.distance_to_next_m + b.distance_to_next_m,
                        intersection_degree: a.intersection_degree,
                        is_roundabout: a.is_roundabout || b.is_roundabout,
                    });
                    i += 2;
                    continue;
                }

            }
        }

        result.push(input[i].clone());
        i += 1;
    }

    *turns = result;
}

/// Thresholds for roundabout exit classification based on net traversal angle.
/// Roundabout exits are discrete (typically ~90° apart), so thresholds bisect
/// the gap between adjacent exit types.
const ROUNDABOUT_STRAIGHT_THRESHOLD_DEG: f64 = 30.0;
const ROUNDABOUT_UTURN_THRESHOLD_DEG: f64 = 135.0;

/// Classify a roundabout exit based on the net angle across the entire ring traversal.
fn classify_roundabout_exit(net_angle: f64) -> TurnDirection {
    let abs = net_angle.abs();
    if abs < ROUNDABOUT_STRAIGHT_THRESHOLD_DEG {
        TurnDirection::RoundaboutExitStraight
    } else if abs >= ROUNDABOUT_UTURN_THRESHOLD_DEG {
        TurnDirection::RoundaboutExitUturn
    } else if net_angle < 0.0 {
        // Negative = right (first exit in right-hand traffic)
        TurnDirection::RoundaboutExitRight
    } else {
        // Positive = left (go around most of the ring)
        TurnDirection::RoundaboutExitLeft
    }
}

/// Collapse consecutive roundabout turns into at most 2: enter + exit.
/// The exit direction is classified by the net angle across the full traversal.
fn collapse_roundabouts(turns: &mut Vec<TurnAnnotation>) {
    let input = std::mem::take(turns);
    let mut result: Vec<TurnAnnotation> = Vec::with_capacity(input.len());
    let mut i = 0;

    while i < input.len() {
        if !input[i].is_roundabout {
            result.push(input[i].clone());
            i += 1;
            continue;
        }

        // Collect the full roundabout run
        let run_start = i;
        while i < input.len() && input[i].is_roundabout {
            i += 1;
        }
        let run = &input[run_start..i];

        let net_angle: f64 = run.iter().map(|t| t.angle_degrees).sum();

        let mut entry = run[0].clone();
        entry.direction = TurnDirection::RoundaboutEnter;
        result.push(entry);

        // Exit uses the last turn's position but net angle for classification
        let mut exit = run[run.len() - 1].clone();
        exit.direction = classify_roundabout_exit(net_angle);
        exit.angle_degrees = net_angle;
        result.push(exit);
    }

    *turns = result;
}

/// Full post-processing pipeline.
pub fn refine_turns(turns: &mut Vec<TurnAnnotation>, coordinates: &[(f32, f32)]) {
    collapse_degree2_curves(turns);
    merge_straights(turns);
    collapse_roundabouts(turns);
    annotate_distances(turns, coordinates);
    strip_straights(turns);
    collapse_compound_turns(turns, coordinates);

    if turns.is_empty() && !coordinates.is_empty() {
        turns.push(TurnAnnotation {
            direction: TurnDirection::Straight,
            angle_degrees: 0.0,
            coordinate_index: 0,
            distance_to_next_m: path_distance(coordinates, 0, coordinates.len().saturating_sub(1)),
            intersection_degree: 0,
            is_roundabout: false,
        });
    }
}

