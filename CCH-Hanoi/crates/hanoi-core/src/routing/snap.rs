use crate::geo::spatial::{SnapResult, haversine_m};

pub(crate) const TIER2_SNAP_CANDIDATES: usize = 4;
const PROJECTED_POINT_DEDUP_DISTANCE_M: f64 = 1.0;

fn snap_distance_sum(src: &SnapResult, dst: &SnapResult) -> f64 {
    src.snap_distance_m + dst.snap_distance_m
}

fn search_snap_pairs<T, F, I>(
    src_snaps: &[SnapResult],
    dst_snaps: &[SnapResult],
    mut include_pair: I,
    evaluate: &mut F,
) -> Option<(SnapResult, SnapResult, T)>
where
    F: FnMut(&SnapResult, &SnapResult) -> Option<T>,
    I: FnMut(usize, usize) -> bool,
{
    let mut best: Option<(SnapResult, SnapResult, T, f64)> = None;

    for (src_idx, src) in src_snaps.iter().enumerate() {
        for (dst_idx, dst) in dst_snaps.iter().enumerate() {
            if !include_pair(src_idx, dst_idx) {
                continue;
            }

            let Some(result) = evaluate(src, dst) else {
                continue;
            };
            let pair_distance = snap_distance_sum(src, dst);

            if best.as_ref().map_or(true, |(_, _, _, best_distance)| {
                pair_distance < *best_distance
            }) {
                best = Some((*src, *dst, result, pair_distance));
            }
        }
    }

    best.map(|(src, dst, result, _)| (src, dst, result))
}

pub(crate) fn select_tiered_snap_pair<T, F>(
    src_snaps: &[SnapResult],
    dst_snaps: &[SnapResult],
    mut evaluate: F,
) -> Option<(SnapResult, SnapResult, T)>
where
    F: FnMut(&SnapResult, &SnapResult) -> Option<T>,
{
    let (Some(src), Some(dst)) = (src_snaps.first(), dst_snaps.first()) else {
        return None;
    };

    if let Some(result) = evaluate(src, dst) {
        return Some((*src, *dst, result));
    }

    let tier2_src_limit = src_snaps.len().min(TIER2_SNAP_CANDIDATES);
    let tier2_dst_limit = dst_snaps.len().min(TIER2_SNAP_CANDIDATES);

    if let Some(result) = search_snap_pairs(
        src_snaps,
        dst_snaps,
        |src_idx, dst_idx| {
            src_idx < tier2_src_limit
                && dst_idx < tier2_dst_limit
                && !(src_idx == 0 && dst_idx == 0)
        },
        &mut evaluate,
    ) {
        return Some(result);
    }

    search_snap_pairs(
        src_snaps,
        dst_snaps,
        |src_idx, dst_idx| src_idx >= tier2_src_limit || dst_idx >= tier2_dst_limit,
        &mut evaluate,
    )
}

fn coords_within_projected_dedup_threshold(a: (f32, f32), b: (f32, f32)) -> bool {
    haversine_m(a.0 as f64, a.1 as f64, b.0 as f64, b.1 as f64) < PROJECTED_POINT_DEDUP_DISTANCE_M
}

pub(crate) fn prepend_source_geometry(
    coordinates: &mut Vec<(f32, f32)>,
    projected_point: (f32, f32),
    connector_points: Vec<(f32, f32)>,
) -> usize {
    let mut prefix = connector_points;
    let first_existing = prefix
        .first()
        .copied()
        .or_else(|| coordinates.first().copied());

    if first_existing.map_or(true, |point| {
        !coords_within_projected_dedup_threshold(projected_point, point)
    }) {
        prefix.insert(0, projected_point);
    }

    let prepended_count = prefix.len();
    if prepended_count > 0 {
        prefix.extend(std::mem::take(coordinates));
        *coordinates = prefix;
    }

    prepended_count
}

pub(crate) fn append_destination_geometry(
    coordinates: &mut Vec<(f32, f32)>,
    connector_points: Vec<(f32, f32)>,
    projected_point: (f32, f32),
) -> usize {
    let mut suffix = connector_points;
    let last_existing = suffix
        .last()
        .copied()
        .or_else(|| coordinates.last().copied());

    if last_existing.map_or(true, |point| {
        !coords_within_projected_dedup_threshold(projected_point, point)
    }) {
        suffix.push(projected_point);
    }

    let appended_count = suffix.len();
    coordinates.extend(suffix);
    appended_count
}
