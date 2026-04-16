use rust_road_router::algo::customizable_contraction_hierarchy::query::Server as CchQueryServer;
use rust_road_router::algo::customizable_contraction_hierarchy::{CustomizedBasic, DirectedCCH};
use rust_road_router::algo::{Query, QueryServer};
use rust_road_router::datastr::graph::{EdgeId, INFINITY, NodeId, Weight};

use crate::geo::bounds::{BoundingBox, CoordRejection, ValidationConfig};
use crate::geo::spatial::{SNAP_MAX_CANDIDATES, SnapResult, SpatialIndex};
use crate::guidance::{compute_turns, refine_turns};
use crate::routing::alternatives::{AlternativeServer, MultiQueryRepository, GEO_OVER_REQUEST, MAX_GEO_RATIO};
use crate::routing::line_graph::{
    LineGraphCchContext, clip_backtrack_protrusion_from_end, clip_backtrack_protrusion_from_start,
    update_turns_after_coordinate_patch,
};
use crate::routing::{QueryAnswer, route_distance_m, QueryRepository};
use crate::routing::normal::{
    append_destination_geometry, prepend_source_geometry,
    select_tiered_snap_pair,
};

/// Query engine for line graphs. Uses DirectedCCH and applies final-edge
/// correction automatically.
///
/// Coordinate queries snap in the **original graph's** intersection-node
/// coordinate space (unified snap), then use the snapped original edge ID
/// directly as a line-graph node ID. This ensures the line graph engine
/// selects the same physical road segment as the normal `QueryEngine`.
pub struct LineGraphQueryEngine<'a> {
    server: CchQueryServer<CustomizedBasic<'a, DirectedCCH>>,
    context: &'a LineGraphCchContext,
    /// Spatial index on the **original graph's** intersection-node coordinates.
    /// Used by `query_coords` for unified snapping.
    original_spatial: SpatialIndex,
    validation_config: ValidationConfig,
}

impl<'a> LineGraphQueryEngine<'a> {
    /// Create a line graph query engine. Performs initial customization
    /// with baseline weights and builds the original-graph spatial index
    /// for unified coordinate snapping.
    pub fn new(context: &'a LineGraphCchContext) -> Self {
        Self::with_validation_config(context, ValidationConfig::default())
    }

    pub fn with_validation_config(
        context: &'a LineGraphCchContext,
        validation_config: ValidationConfig,
    ) -> Self {
        let customized = context.customize();
        let server = CchQueryServer::new(customized);

        // Build spatial index on the ORIGINAL graph's intersection-node coordinates.
        // This ensures coordinate queries snap to the same physical road as the
        // normal QueryEngine. The snapped original edge ID is then used directly
        // as a line-graph node ID (line-graph node N = original edge N).
        let original_spatial = SpatialIndex::build_with_shape_points(
            &context.original_latitude,
            &context.original_longitude,
            &context.original_first_out,
            &context.original_head,
            context.original_first_modelling_node.as_deref(),
            context.original_modelling_node_latitude.as_deref(),
            context.original_modelling_node_longitude.as_deref(),
        );

        LineGraphQueryEngine {
            server,
            context,
            original_spatial,
            validation_config,
        }
    }

    /// Query by snapped coordinates in line-graph space while preserving the
    /// exact projected entry and exit positions on the snapped arcs.
    fn query_trimmed(&mut self, src: &SnapResult, dst: &SnapResult) -> Option<QueryAnswer> {
        self.query_coordinate_candidate(src, dst)
    }

    /// Query by line-graph node IDs (= original edge indices).
    /// Adds the source edge's travel_time (excluded from the shifted
    /// line-graph encoding where each LG edge carries `tt(next_edge)`).
    pub fn query(&mut self, source_edge: EdgeId, target_edge: EdgeId) -> Option<QueryAnswer> {
        let result = self.server.query(Query {
            from: source_edge as NodeId,
            to: target_edge as NodeId,
        });

        if let Some(mut connected) = result.found() {
            let cch_distance = connected.distance();
            if cch_distance >= INFINITY {
                return None;
            }

            // Source-edge correction: the shifted encoding includes tt(target_edge)
            // in cch_distance but excludes tt(source_edge). Since source_edge is
            // constant for all candidate paths, adding it post-hoc is correct.
            let source_edge_cost = self.context.lg_travel_time(source_edge);
            let lg_path = connected.node_path();
            self.build_answer_from_lg_path(cch_distance, &lg_path, source_edge_cost, false)
        } else {
            None
        }
    }

    /// Query by coordinates using **unified snap space**.
    ///
    /// Snaps coordinates in the **original graph's** intersection-node coordinate
    /// space (identical to `QueryEngine::query_coords`), then uses the snapped
    /// original edge ID directly as a line-graph node ID. This eliminates the
    /// coordinate-space divergence that previously caused different edge selection
    /// between normal and line-graph engines.
    pub fn query_coords(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
    ) -> Result<Option<QueryAnswer>, CoordRejection> {
        let src_snaps = self.original_spatial.validated_snap_candidates(
            "origin",
            from.0,
            from.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;
        let dst_snaps = self.original_spatial.validated_snap_candidates(
            "destination",
            to.0,
            to.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;

        Ok(
            match select_tiered_snap_pair(&src_snaps, &dst_snaps, |src, dst| {
                tracing::debug!(
                    src_edge = src.edge_id,
                    dst_edge = dst.edge_id,
                    src_snap_dist_m = src.snap_distance_m,
                    dst_snap_dist_m = dst.snap_distance_m,
                    "unified snap candidate pair: original edge IDs -> line-graph node IDs"
                );
                self.query_trimmed(src, dst)
            }) {
                Some((src, dst, answer)) => {
                    Some(self.patch_coordinates(answer, from, to, &src, &dst))
                }
                _ => None,
            },
        )
    }

    /// Attach the user's origin/destination metadata and splice snap-edge
    /// connector geometry around the routed path when needed.
    fn direct_same_edge_coordinate_answer(
        &self,
        src: &SnapResult,
        dst: &SnapResult,
    ) -> Option<QueryAnswer> {
        let edge_cost = self.context.lg_travel_time(src.edge_id);
        let distance_ms = self
            .original_spatial
            .partial_edge_cost_between_snaps_ms(src, dst, edge_cost)?;

        Some(QueryAnswer {
            distance_ms,
            distance_m: 0.0,
            route_arc_ids: Vec::new(),
            weight_path_ids: vec![src.edge_id],
            path: Vec::new(),
            coordinates: Vec::new(),
            turns: Vec::new(),
            origin: None,
            destination: None,
            snapped_origin: None,
            snapped_destination: None,
        })
    }

    fn coordinate_distance_ms(
        &self,
        cch_distance: Weight,
        src: &SnapResult,
        dst: &SnapResult,
    ) -> Weight {
        let src_edge_cost = self.context.lg_travel_time(src.edge_id);
        let dst_edge_cost = self.context.lg_travel_time(dst.edge_id);

        cch_distance
            .saturating_sub(dst_edge_cost)
            .saturating_add(self.original_spatial.partial_edge_cost_to_node_ms(
                src,
                src.head,
                src_edge_cost,
            ))
            .saturating_add(self.original_spatial.partial_edge_cost_to_node_ms(
                dst,
                dst.tail,
                dst_edge_cost,
            ))
    }

    fn build_coordinate_answer_from_lg_path(
        &self,
        cch_distance: Weight,
        lg_path: &[NodeId],
        src: &SnapResult,
        dst: &SnapResult,
    ) -> Option<QueryAnswer> {
        let distance_ms = self.coordinate_distance_ms(cch_distance, src, dst);
        self.materialize_lg_path(distance_ms, lg_path, true)
    }

    fn query_same_edge_cycle_candidate(
        &mut self,
        src: &SnapResult,
        dst: &SnapResult,
    ) -> Option<QueryAnswer> {
        let start = self.context.graph.first_out[src.edge_id as usize] as usize;
        let end = self.context.graph.first_out[src.edge_id as usize + 1] as usize;
        let mut best: Option<QueryAnswer> = None;

        for edge_idx in start..end {
            let next = self.context.graph.head[edge_idx];
            let first_transition_cost = self.context.graph.travel_time[edge_idx];
            let result = self.server.query(Query {
                from: next,
                to: dst.edge_id as NodeId,
            });

            let Some(mut connected) = result.found() else {
                continue;
            };
            let rest_distance = connected.distance();
            if rest_distance >= INFINITY {
                continue;
            }

            let mut lg_path = Vec::with_capacity(connected.node_path().len() + 1);
            lg_path.push(src.edge_id as NodeId);
            lg_path.extend(connected.node_path());

            if let Some(answer) = self.build_coordinate_answer_from_lg_path(
                first_transition_cost.saturating_add(rest_distance),
                &lg_path,
                src,
                dst,
            ) {
                if best
                    .as_ref()
                    .map_or(true, |current| answer.distance_ms < current.distance_ms)
                {
                    best = Some(answer);
                }
            }
        }

        best
    }

    fn query_coordinate_candidate(
        &mut self,
        src: &SnapResult,
        dst: &SnapResult,
    ) -> Option<QueryAnswer> {
        if src.edge_id == dst.edge_id {
            return self
                .direct_same_edge_coordinate_answer(src, dst)
                .or_else(|| self.query_same_edge_cycle_candidate(src, dst));
        }

        let result = self.server.query(Query {
            from: src.edge_id as NodeId,
            to: dst.edge_id as NodeId,
        });

        if let Some(mut connected) = result.found() {
            let cch_distance = connected.distance();
            if cch_distance >= INFINITY {
                return None;
            }

            let lg_path = connected.node_path();
            self.build_coordinate_answer_from_lg_path(cch_distance, &lg_path, src, dst)
        } else {
            None
        }
    }

    fn multi_query_coordinate_candidates(
        &mut self,
        src: &SnapResult,
        dst: &SnapResult,
        max_alternatives: usize,
        stretch_factor: f64,
    ) -> Vec<QueryAnswer> {
        if src.edge_id == dst.edge_id {
            return self
                .direct_same_edge_coordinate_answer(src, dst)
                .or_else(|| self.query_same_edge_cycle_candidate(src, dst))
                .into_iter()
                .collect();
        }

        let customized = self.server.customized();
        let mut multi = AlternativeServer::new(customized);
        let request_count = max_alternatives
            .saturating_mul(GEO_OVER_REQUEST)
            .max(max_alternatives + 10);
        let geo_len = self.lg_path_geo_len();
        let candidates = multi.alternatives(
            src.edge_id as NodeId,
            dst.edge_id as NodeId,
            request_count,
            stretch_factor,
            geo_len,
        );

        let mut answers: Vec<QueryAnswer> = Vec::with_capacity(max_alternatives);
        let mut shortest_geo_dist: Option<f64> = None;

        for alt in candidates {
            if answers.len() >= max_alternatives {
                break;
            }
            if let Some(answer) =
                self.build_coordinate_answer_from_lg_path(alt.distance, &alt.path, src, dst)
            {
                if let Some(base) = shortest_geo_dist {
                    if answer.distance_m > base * MAX_GEO_RATIO {
                        continue;
                    }
                } else {
                    shortest_geo_dist = Some(answer.distance_m);
                }
                answers.push(answer);
            }
        }

        answers
    }

    fn has_reverse_edge(&self, edge_id: EdgeId) -> bool {
        let tail = self.context.original_tail[edge_id as usize];
        let head = self.context.original_head[edge_id as usize];
        let start = self.context.original_first_out[head as usize] as usize;
        let end = self.context.original_first_out[head as usize + 1] as usize;

        (start..end).any(|edge_idx| self.context.original_head[edge_idx] == tail)
    }

    fn patch_coordinates(
        &self,
        mut answer: QueryAnswer,
        from: (f32, f32),
        to: (f32, f32),
        src: &SnapResult,
        dst: &SnapResult,
    ) -> QueryAnswer {
        let src_projected = src.projected_point(
            &self.context.original_latitude,
            &self.context.original_longitude,
        );
        let dst_projected = dst.projected_point(
            &self.context.original_latitude,
            &self.context.original_longitude,
        );
        let original_route_len = answer.coordinates.len();
        let prepended_count = if answer.coordinates.is_empty() && src.edge_id == dst.edge_id {
            answer.coordinates = self.original_spatial.open_interval_between_snaps(src, dst);
            let prepended =
                prepend_source_geometry(&mut answer.coordinates, src_projected, Vec::new());
            let _ = append_destination_geometry(&mut answer.coordinates, Vec::new(), dst_projected);
            prepended
        } else {
            // Add connector geometry from snap point along the snapped edge to
            // the route entry/exit node.
            //
            // In line-graph trimmed paths:
            //   path[0]  = tail(first_interior_arc) = head(src_edge) = src.head
            //   path[-1] = head(last_interior_arc)  = tail(dst_edge) = dst.tail
            //
            // So connector_points_from_snap_to_node(src, src.head) returns the
            // source edge polyline from snap→head, and
            // connector_points_from_snap_to_node(dst, dst.tail) returns the
            // dest edge polyline from snap→tail (reversed for appending).

            let src_connector = if let Some(start_node) = answer.path.first().copied() {
                self.original_spatial
                    .connector_points_from_snap_to_node(src, start_node)
            } else {
                Vec::new()
            };
            let dst_connector = if let Some(end_node) = answer.path.last().copied() {
                let mut conn = self
                    .original_spatial
                    .connector_points_from_snap_to_node(dst, end_node);
                conn.reverse();
                conn
            } else {
                Vec::new()
            };

            let prepended =
                prepend_source_geometry(&mut answer.coordinates, src_projected, src_connector);
            let appended =
                append_destination_geometry(&mut answer.coordinates, dst_connector, dst_projected);

            // Clip source-side backtrack: when the route starts by going away
            // from the projected point (edge faces wrong direction), find where
            // the route crosses back and clip the protrusion.
            let clipped_source_count = if prepended > 0 && self.has_reverse_edge(src.edge_id) {
                clip_backtrack_protrusion_from_start(&mut answer.coordinates, src_projected)
            } else {
                0
            };

            // Clip destination-side backtrack: mirror of source clipping.
            let clipped_destination_count = if appended > 0 && self.has_reverse_edge(dst.edge_id) {
                clip_backtrack_protrusion_from_end(&mut answer.coordinates, dst_projected)
            } else {
                0
            };

            answer.origin = Some(from);
            answer.destination = Some(to);
            answer.snapped_origin = None;
            answer.snapped_destination = None;
            update_turns_after_coordinate_patch(
                &mut answer.turns,
                &answer.coordinates,
                prepended,
                clipped_source_count,
                clipped_destination_count,
                original_route_len,
            );
            answer.distance_m = route_distance_m(&answer.coordinates);
            return answer;
        };

        answer.origin = Some(from);
        answer.destination = Some(to);
        answer.snapped_origin = None;
        answer.snapped_destination = None;
        update_turns_after_coordinate_patch(
            &mut answer.turns,
            &answer.coordinates,
            prepended_count,
            0,
            0,
            original_route_len,
        );
        answer.distance_m = route_distance_m(&answer.coordinates);
        answer
    }

    fn build_answer_from_lg_path(
        &self,
        cch_distance: Weight,
        lg_path: &[NodeId],
        source_edge_cost: Weight,
        trimmed: bool,
    ) -> Option<QueryAnswer> {
        if lg_path.is_empty() {
            return None;
        }

        let distance_ms = cch_distance.saturating_add(source_edge_cost);
        self.materialize_lg_path(distance_ms, lg_path, trimmed)
    }

    fn materialize_lg_path(
        &self,
        distance_ms: Weight,
        lg_path: &[NodeId],
        trimmed: bool,
    ) -> Option<QueryAnswer> {
        if lg_path.is_empty() {
            return None;
        }

        let effective_path: &[NodeId] = if trimmed && lg_path.len() > 2 {
            &lg_path[1..lg_path.len() - 1]
        } else if trimmed {
            &[]
        } else {
            lg_path
        };

        let route_arc_ids: Vec<u32> = effective_path
            .iter()
            .map(|&lg_node| self.context.original_arc_id_of_lg_node[lg_node as usize])
            .collect();
        let weight_path_ids: Vec<u32> = lg_path.iter().map(|&lg_node| lg_node as u32).collect();

        let mut path: Vec<NodeId> = effective_path
            .iter()
            .map(|&lg_node| self.context.lg_tail_node(lg_node))
            .collect();

        if trimmed {
            if let Some(&last_edge) = effective_path.last() {
                path.push(self.context.lg_head_node(last_edge));
            } else if lg_path.len() >= 2 {
                path.push(self.context.lg_head_node(lg_path[0]));
            }
        } else if let Some(&last_edge) = lg_path.last() {
            path.push(self.context.lg_head_node(last_edge));
        }

        let (coordinates, arc_end_coordinate_indices) = if route_arc_ids.is_empty() {
            (
                path.iter()
                    .map(|&node| {
                        (
                            self.context.original_latitude[node as usize],
                            self.context.original_longitude[node as usize],
                        )
                    })
                    .collect(),
                Vec::new(),
            )
        } else {
            self.original_spatial
                .expand_route_arc_geometry_with_boundaries(&route_arc_ids)
        };

        let mut turns = compute_turns(
            effective_path,
            &self.context.original_arc_id_of_lg_node,
            &self.context.original_tail,
            &self.context.original_head,
            &self.context.original_first_out,
            &self.context.original_latitude,
            &self.context.original_longitude,
            &self.context.is_arc_roundabout,
        );
        for turn in &mut turns {
            let arc_idx = turn.coordinate_index.saturating_sub(1) as usize;
            if let Some(&coordinate_index) = arc_end_coordinate_indices.get(arc_idx) {
                turn.coordinate_index = coordinate_index;
            }
        }
        refine_turns(&mut turns, &coordinates);

        let distance_m = route_distance_m(&coordinates);

        Some(QueryAnswer {
            distance_ms,
            distance_m,
            route_arc_ids,
            weight_path_ids,
            path,
            coordinates,
            turns,
            origin: None,
            destination: None,
            snapped_origin: None,
            snapped_destination: None,
        })
    }

    /// Find up to `max_alternatives` alternative routes by line-graph node IDs
    /// (= original edge indices). Each route gets source-edge correction,
    /// LG->original node mapping, and turn annotation.
    #[tracing::instrument(
        skip(self),
        fields(source_edge, target_edge, max_alternatives, stretch_factor)
    )]
    pub fn multi_query(
        &self,
        source_edge: EdgeId,
        target_edge: EdgeId,
        max_alternatives: usize,
        stretch_factor: f64,
    ) -> Vec<QueryAnswer> {
        let customized = self.server.customized();
        let mut multi = AlternativeServer::new(customized);
        let request_count = max_alternatives
            .saturating_mul(GEO_OVER_REQUEST)
            .max(max_alternatives + 10);
        let geo_len = self.lg_path_geo_len();
        let candidates = multi.alternatives(
            source_edge as NodeId,
            target_edge as NodeId,
            request_count,
            stretch_factor,
            geo_len,
        );

        let source_edge_cost = self.context.lg_travel_time(source_edge);

        let mut results: Vec<QueryAnswer> = Vec::with_capacity(max_alternatives);
        let mut shortest_geo_dist: Option<f64> = None;

        for alt in candidates {
            if results.len() >= max_alternatives {
                break;
            }
            if let Some(answer) =
                self.build_answer_from_lg_path(alt.distance, &alt.path, source_edge_cost, false)
            {
                if let Some(base) = shortest_geo_dist {
                    if answer.distance_m > base * MAX_GEO_RATIO {
                        continue;
                    }
                } else {
                    shortest_geo_dist = Some(answer.distance_m);
                }
                results.push(answer);
            }
        }

        tracing::info!(
            requested = max_alternatives,
            returned = results.len(),
            shortest_ms = results.first().map(|route| route.distance_ms),
            "line_graph multi_query completed"
        );

        results
    }

    /// Find up to `max_alternatives` alternative routes by coordinates.
    /// Snaps in the original graph coordinate space, then runs multi_query on
    /// the snapped edge IDs with trimming.
    #[tracing::instrument(skip(self), fields(
        from_lat = from.0,
        from_lng = from.1,
        to_lat = to.0,
        to_lng = to.1,
        max_alternatives,
        stretch_factor
    ))]
    pub fn multi_query_coords(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
        max_alternatives: usize,
        stretch_factor: f64,
    ) -> Result<Vec<QueryAnswer>, CoordRejection> {
        let src_snaps = self.original_spatial.validated_snap_candidates(
            "origin",
            from.0,
            from.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;
        let dst_snaps = self.original_spatial.validated_snap_candidates(
            "destination",
            to.0,
            to.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;

        if let Some((src, dst, answers)) =
            select_tiered_snap_pair(&src_snaps, &dst_snaps, |src, dst| {
                let answers = self.multi_query_coordinate_candidates(
                    src,
                    dst,
                    max_alternatives,
                    stretch_factor,
                );
                (!answers.is_empty()).then_some(answers)
            })
        {
            let answers: Vec<QueryAnswer> = answers
                .into_iter()
                .map(|answer| self.patch_coordinates(answer, from, to, &src, &dst))
                .collect();
            tracing::info!(
                requested = max_alternatives,
                returned = answers.len(),
                shortest_ms = answers.first().map(|route| route.distance_ms),
                "line_graph multi_query_coords completed"
            );
            Ok(answers)
        } else {
            tracing::info!(
                requested = max_alternatives,
                returned = 0usize,
                "line_graph multi_query_coords completed"
            );
            Ok(Vec::new())
        }
    }

    /// Apply new weights and re-customize (directed variant).
    pub fn update_weights(&mut self, weights: &[Weight]) {
        let new_customized = self.context.customize_with(weights);
        self.server.update(new_customized);
    }

    pub fn bbox(&self) -> &BoundingBox {
        self.original_spatial.bbox()
    }

    pub fn validation_config(&self) -> &ValidationConfig {
        &self.validation_config
    }

    /// Access the original-graph spatial index (used for unified snapping).
    pub fn spatial(&self) -> &SpatialIndex {
        &self.original_spatial
    }

    /// Access the underlying line graph CCH context.
    pub fn context(&self) -> &'a LineGraphCchContext {
        self.context
    }

    fn lg_path_geo_len(&self) -> impl Fn(&[NodeId]) -> f64 + '_ {
        let lg_to_orig = &self.context.original_arc_id_of_lg_node;
        let orig_tail = &self.context.original_tail;
        let orig_head = &self.context.original_head;
        let orig_lat = &self.context.original_latitude;
        let orig_lng = &self.context.original_longitude;

        move |lg_path: &[NodeId]| -> f64 {
            let mut coords: Vec<(f32, f32)> = lg_path
                .iter()
                .map(|&lg_node| {
                    let node = orig_tail[lg_to_orig[lg_node as usize] as usize];
                    (orig_lat[node as usize], orig_lng[node as usize])
                })
                .collect();
            if let Some(&last) = lg_path.last() {
                let node = orig_head[lg_to_orig[last as usize] as usize];
                coords.push((orig_lat[node as usize], orig_lng[node as usize]));
            }
            route_distance_m(&coords)
        }
    }
}

impl<'a> QueryRepository for LineGraphQueryEngine<'a> {
    fn run_query(&mut self, from: u32, to: u32) -> Option<QueryAnswer> {
        self.query(from, to)
    }

    fn run_query_coords(&mut self, from: (f32, f32), to: (f32, f32)) -> Result<Option<QueryAnswer>, CoordRejection> {
        self.query_coords(from, to)
    }
}

impl<'a> MultiQueryRepository for LineGraphQueryEngine<'a> {
    fn run_multi_query(
        &mut self,
        from: u32,
        to: u32,
        alternatives: usize,
        stretch: f64,
    ) -> Vec<QueryAnswer> {
        self.multi_query(from, to, alternatives, stretch)
    }

    fn run_multi_query_coords(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
        alternatives: usize,
        stretch: f64,
    ) -> Result<Vec<QueryAnswer>, CoordRejection> {
        self.multi_query_coords(from, to, alternatives, stretch)
    }
}