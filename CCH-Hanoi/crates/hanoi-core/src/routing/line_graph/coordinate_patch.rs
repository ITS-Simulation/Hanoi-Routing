use crate::geo::spatial::haversine_m;
use crate::guidance::{TurnAnnotation, annotate_distances};

pub(crate) fn update_turns_after_coordinate_patch(
    turns: &mut Vec<TurnAnnotation>,
    coordinates: &[(f32, f32)],
    prepended_count: usize,
    clipped_source_count: usize,
    clipped_destination_count: usize,
    original_route_len: usize,
) {
    if turns.is_empty() {
        return;
    }

    let retained_route_end = original_route_len.saturating_sub(clipped_destination_count);
    let mut remapped = Vec::with_capacity(turns.len());

    for mut turn in std::mem::take(turns) {
        if turn.coordinate_index == 0 {
            remapped.push(turn);
            continue;
        }

        let original_index = turn.coordinate_index as usize;
        if original_index < clipped_source_count || original_index >= retained_route_end {
            continue;
        }

        turn.coordinate_index = (prepended_count + original_index - clipped_source_count) as u32;
        remapped.push(turn);
    }

    *turns = remapped;
    annotate_distances(turns, coordinates);
}

fn point_distance_m(a: (f32, f32), b: (f32, f32)) -> f64 {
    haversine_m(a.0 as f64, a.1 as f64, b.0 as f64, b.1 as f64)
}

/// Clip backtrack protrusion at the start of a coordinate chain.
///
/// coordinates[0] = P2 (projected point on snapped edge).
/// The route may backtrack past P2 before heading toward the destination.
/// We find where the route polyline passes closest to P2 (by perpendicular
/// projection onto each segment) and replace everything before that point
/// with [P2, projection_on_segment].
pub(crate) fn clip_backtrack_protrusion_from_start(
    coordinates: &mut Vec<(f32, f32)>,
    anchor: (f32, f32),
) -> usize {
    if coordinates.len() < 3 {
        return 0;
    }

    // Skip coordinates[0] (= anchor) and coordinates[1] (first route point).
    // Search segments [1→2], [2→3], ... for the one closest to anchor.
    // The segment where the route "crosses back" past P2 will have the
    // smallest perpendicular distance.
    let mut best_dist = f64::MAX;
    let mut best_seg = 0usize; // index of segment start
    let mut best_t = 0.0f64;

    for seg in 1..(coordinates.len() - 1) {
        let a = coordinates[seg];
        let b = coordinates[seg + 1];
        let (dist, t) = perpendicular_distance_and_t(anchor, a, b);
        if dist < best_dist {
            best_dist = dist;
            best_seg = seg;
            best_t = t;
        }
        // Once we found a close segment and distance is growing, stop.
        if best_dist < 10.0 && dist > best_dist * 3.0 {
            break;
        }
    }

    // If the best segment is [0→1] or [1→2] with t=0, no real backtrack.
    if best_seg <= 1 && best_t < 0.01 {
        return 0;
    }

    // Check that this is actually closer than coordinates[1] to anchor.
    // If not, there's no backtrack to clip.
    let dist_to_first = point_distance_m(anchor, coordinates[1]);
    if best_dist >= dist_to_first {
        return 0;
    }

    // Interpolate the crossing point on the best segment.
    let a = coordinates[best_seg];
    let b = coordinates[best_seg + 1];
    let crossing = (
        a.0 + (b.0 - a.0) * best_t as f32,
        a.1 + (b.1 - a.1) * best_t as f32,
    );

    // Clip: [P2, crossing, seg+1, seg+2, ...]
    let clip_idx = best_seg + 1;
    let tail = coordinates.split_off(clip_idx);
    coordinates.truncate(1); // keep [P2]
    coordinates.push(crossing);
    coordinates.extend(tail);
    // We removed indices 1..clip_idx (= clip_idx-1 points) and added 1 (crossing)
    // Net removed = clip_idx - 1 - 1 = clip_idx - 2... but the return value is
    // "number of coordinates removed" for turn index adjustment.
    // Original had clip_idx points between [0] and [clip_idx].
    // We replaced them with 1 point (crossing). So removed = clip_idx - 1 - 1.
    // Actually: we had coordinates[1..clip_idx] removed = clip_idx-1 points,
    // and inserted 1 crossing point. Net shift = clip_idx - 2.
    if clip_idx >= 2 { clip_idx - 2 } else { 0 }
}

/// Perpendicular distance from point P to segment A→B, plus the projection
/// parameter t ∈ [0,1].
fn perpendicular_distance_and_t(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> (f64, f64) {
    let dx = (b.0 - a.0) as f64;
    let dy = (b.1 - a.1) as f64;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-14 {
        return (point_distance_m(p, a), 0.0);
    }
    let t = (((p.0 - a.0) as f64 * dx + (p.1 - a.1) as f64 * dy) / len_sq).clamp(0.0, 1.0);
    let proj = (a.0 as f64 + dx * t, a.1 as f64 + dy * t);
    let dist = point_distance_m(p, (proj.0 as f32, proj.1 as f32));
    (dist, t)
}

pub(crate) fn clip_backtrack_protrusion_from_end(
    coordinates: &mut Vec<(f32, f32)>,
    anchor: (f32, f32),
) -> usize {
    coordinates.reverse();
    let clipped = clip_backtrack_protrusion_from_start(coordinates, anchor);
    coordinates.reverse();
    clipped
}
