use std::path::Path;

use rust_road_router::datastr::graph::{INFINITY, Weight};

use hanoi_core::{CchContext, LineGraphCchContext};

use super::road_flags::load_major_road_flags_or_default;
use crate::api::dto::{TrafficOverlayBucket, TrafficOverlayQuery, TrafficOverlayResponse};

const TRAFFIC_GREEN_COLOR: &str = "#34c26b";
const TRAFFIC_YELLOW_COLOR: &str = "#f3c63f";
const TRAFFIC_RED_COLOR: &str = "#df5a43";

const GREEN_MAX_RATIO: f64 = 1.15;
const YELLOW_MAX_RATIO: f64 = 1.60;

#[derive(Clone, Copy)]
pub(crate) struct TrafficSegment {
    tail_lat: f32,
    tail_lng: f32,
    head_lat: f32,
    head_lng: f32,
    min_lat: f32,
    max_lat: f32,
    min_lng: f32,
    max_lng: f32,
    baseline_weight: Weight,
    is_tertiary_and_above: bool,
}

impl TrafficSegment {
    fn intersects(&self, query: &TrafficOverlayQuery) -> bool {
        !(self.max_lat < query.min_lat
            || self.min_lat > query.max_lat
            || self.max_lng < query.min_lng
            || self.min_lng > query.max_lng)
    }

    fn polyline(&self) -> [[f32; 2]; 2] {
        [
            [self.tail_lat, self.tail_lng],
            [self.head_lat, self.head_lng],
        ]
    }
}

pub(crate) enum TrafficOverlay {
    Normal {
        segments: Vec<TrafficSegment>,
        tertiary_filter_supported: bool,
    },
    LineGraphPseudoNormal {
        segments: Vec<TrafficSegment>,
        incoming_offsets: Vec<u32>,
        incoming_edges: Vec<u32>,
        tertiary_filter_supported: bool,
    },
}

impl TrafficOverlay {
    pub fn from_normal(context: &CchContext, manifest_path: &Path) -> Self {
        let first_out = &context.graph.first_out;
        let head = &context.graph.head;
        let latitude = &context.graph.latitude;
        let longitude = &context.graph.longitude;
        let (major_road_flags, tertiary_filter_supported) =
            load_major_road_flags_or_default(manifest_path, head.len());

        let mut segments = Vec::with_capacity(head.len());
        for tail_node in 0..first_out.len().saturating_sub(1) {
            let start = first_out[tail_node] as usize;
            let end = first_out[tail_node + 1] as usize;
            for edge_idx in start..end {
                let head_node = head[edge_idx] as usize;
                let tail_lat = latitude[tail_node];
                let tail_lng = longitude[tail_node];
                let head_lat = latitude[head_node];
                let head_lng = longitude[head_node];
                segments.push(TrafficSegment {
                    tail_lat,
                    tail_lng,
                    head_lat,
                    head_lng,
                    min_lat: tail_lat.min(head_lat),
                    max_lat: tail_lat.max(head_lat),
                    min_lng: tail_lng.min(head_lng),
                    max_lng: tail_lng.max(head_lng),
                    baseline_weight: context.baseline_weights[edge_idx],
                    is_tertiary_and_above: major_road_flags[edge_idx],
                });
            }
        }

        TrafficOverlay::Normal {
            segments,
            tertiary_filter_supported,
        }
    }

    pub fn from_line_graph(context: &LineGraphCchContext, manifest_path: &Path) -> Self {
        let original_edge_count = context.original_first_out.last().copied().unwrap_or(0) as usize;
        let (major_road_flags, tertiary_filter_supported) =
            load_major_road_flags_or_default(manifest_path, original_edge_count);
        let mut segments = Vec::with_capacity(original_edge_count);

        for arc_id in 0..original_edge_count {
            let tail_node = context.original_tail[arc_id] as usize;
            let head_node = context.original_head[arc_id] as usize;
            let tail_lat = context.original_latitude[tail_node];
            let tail_lng = context.original_longitude[tail_node];
            let head_lat = context.original_latitude[head_node];
            let head_lng = context.original_longitude[head_node];
            segments.push(TrafficSegment {
                tail_lat,
                tail_lng,
                head_lat,
                head_lng,
                min_lat: tail_lat.min(head_lat),
                max_lat: tail_lat.max(head_lat),
                min_lng: tail_lng.min(head_lng),
                max_lng: tail_lng.max(head_lng),
                baseline_weight: context.original_travel_time[arc_id],
                is_tertiary_and_above: major_road_flags[arc_id],
            });
        }

        let mut incoming_counts = vec![0u32; original_edge_count];
        for &target in context.graph.head.iter() {
            let target = target as usize;
            if target < original_edge_count {
                incoming_counts[target] += 1;
            }
        }

        let mut incoming_offsets = Vec::with_capacity(original_edge_count + 1);
        incoming_offsets.push(0);
        for count in &incoming_counts {
            let next = incoming_offsets.last().copied().unwrap_or(0) + count;
            incoming_offsets.push(next);
        }

        let mut incoming_edges = vec![0u32; incoming_offsets.last().copied().unwrap_or(0) as usize];
        let mut write_cursor = incoming_offsets[..original_edge_count].to_vec();
        for (edge_idx, &target) in context.graph.head.iter().enumerate() {
            let target = target as usize;
            if target >= original_edge_count {
                continue;
            }
            let cursor = &mut write_cursor[target];
            incoming_edges[*cursor as usize] = edge_idx as u32;
            *cursor += 1;
        }

        TrafficOverlay::LineGraphPseudoNormal {
            segments,
            incoming_offsets,
            incoming_edges,
            tertiary_filter_supported,
        }
    }

    pub fn render(
        &self,
        query: &TrafficOverlayQuery,
        current_weights: Option<&[Weight]>,
        using_customized_weights: bool,
    ) -> TrafficOverlayResponse {
        let mut green_segments = Vec::new();
        let mut yellow_segments = Vec::new();
        let mut red_segments = Vec::new();
        let tertiary_filter_supported = self.tertiary_filter_supported();
        let tertiary_and_above_only = query.tertiary_and_above_only && tertiary_filter_supported;

        match self {
            TrafficOverlay::Normal { segments, .. } => {
                for (edge_idx, segment) in segments.iter().enumerate() {
                    if !segment.intersects(query)
                        || (tertiary_and_above_only && !segment.is_tertiary_and_above)
                    {
                        continue;
                    }
                    let current_weight = current_weights
                        .and_then(|weights| weights.get(edge_idx).copied())
                        .unwrap_or(segment.baseline_weight);
                    push_segment_by_status(
                        &mut green_segments,
                        &mut yellow_segments,
                        &mut red_segments,
                        *segment,
                        current_weight,
                    );
                }
            }
            TrafficOverlay::LineGraphPseudoNormal {
                segments,
                incoming_offsets,
                incoming_edges,
                ..
            } => {
                for (arc_idx, segment) in segments.iter().enumerate() {
                    if !segment.intersects(query)
                        || (tertiary_and_above_only && !segment.is_tertiary_and_above)
                    {
                        continue;
                    }
                    let current_weight =
                        current_weights.map_or(segment.baseline_weight, |weights| {
                            pseudo_normal_arc_weight(
                                arc_idx,
                                segment.baseline_weight,
                                weights,
                                incoming_offsets,
                                incoming_edges,
                            )
                        });
                    push_segment_by_status(
                        &mut green_segments,
                        &mut yellow_segments,
                        &mut red_segments,
                        *segment,
                        current_weight,
                    );
                }
            }
        }

        let visible_segment_count =
            green_segments.len() + yellow_segments.len() + red_segments.len();
        let mapping_mode = match self {
            TrafficOverlay::Normal { .. } => "normal",
            TrafficOverlay::LineGraphPseudoNormal { .. } => "line_graph_pseudo_normal",
        };

        TrafficOverlayResponse {
            using_customized_weights,
            mapping_mode,
            tertiary_filter_supported,
            tertiary_and_above_only,
            visible_segment_count,
            buckets: vec![
                TrafficOverlayBucket {
                    status: "green",
                    color: TRAFFIC_GREEN_COLOR,
                    segments: green_segments,
                },
                TrafficOverlayBucket {
                    status: "yellow",
                    color: TRAFFIC_YELLOW_COLOR,
                    segments: yellow_segments,
                },
                TrafficOverlayBucket {
                    status: "red",
                    color: TRAFFIC_RED_COLOR,
                    segments: red_segments,
                },
            ],
        }
    }

    fn tertiary_filter_supported(&self) -> bool {
        match self {
            TrafficOverlay::Normal {
                tertiary_filter_supported,
                ..
            } => *tertiary_filter_supported,
            TrafficOverlay::LineGraphPseudoNormal {
                tertiary_filter_supported,
                ..
            } => *tertiary_filter_supported,
        }
    }
}

fn pseudo_normal_arc_weight(
    arc_idx: usize,
    fallback_weight: Weight,
    weights: &[Weight],
    incoming_offsets: &[u32],
    incoming_edges: &[u32],
) -> Weight {
    let start = incoming_offsets[arc_idx] as usize;
    let end = incoming_offsets[arc_idx + 1] as usize;
    if start >= end {
        return fallback_weight;
    }

    let mut best = INFINITY;
    for &incoming_edge in &incoming_edges[start..end] {
        if let Some(&candidate) = weights.get(incoming_edge as usize) {
            best = best.min(candidate);
        }
    }

    if best == INFINITY { INFINITY } else { best }
}

fn push_segment_by_status(
    green_segments: &mut Vec<[[f32; 2]; 2]>,
    yellow_segments: &mut Vec<[[f32; 2]; 2]>,
    red_segments: &mut Vec<[[f32; 2]; 2]>,
    segment: TrafficSegment,
    current_weight: Weight,
) {
    let bucket = classify_status(segment.baseline_weight, current_weight);
    match bucket {
        "green" => green_segments.push(segment.polyline()),
        "yellow" => yellow_segments.push(segment.polyline()),
        _ => red_segments.push(segment.polyline()),
    }
}

fn classify_status(baseline_weight: Weight, current_weight: Weight) -> &'static str {
    if current_weight >= INFINITY {
        return "red";
    }
    if baseline_weight == 0 {
        return "green";
    }

    let ratio = current_weight as f64 / baseline_weight as f64;
    if ratio <= GREEN_MAX_RATIO {
        "green"
    } else if ratio <= YELLOW_MAX_RATIO {
        "yellow"
    } else {
        "red"
    }
}
