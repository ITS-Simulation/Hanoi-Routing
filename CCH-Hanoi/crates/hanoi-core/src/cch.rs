use std::path::Path;

use rust_road_router::algo::customizable_contraction_hierarchy::CustomizedBasic;
use rust_road_router::algo::customizable_contraction_hierarchy::query::Server as CchQueryServer;
use rust_road_router::algo::customizable_contraction_hierarchy::{CCH, customize};
use rust_road_router::algo::{Query, QueryServer};
use rust_road_router::datastr::graph::{FirstOutGraph, INFINITY, NodeId, Weight};
use rust_road_router::datastr::node_order::NodeOrder;
use rust_road_router::io::Load;

use crate::bounds::{BoundingBox, CoordRejection, ValidationConfig};
use crate::geometry::TurnAnnotation;
use crate::multi_route::{GEO_OVER_REQUEST, MAX_GEO_RATIO, MultiRouteServer};
use crate::graph::GraphData;
use crate::spatial::{SNAP_MAX_CANDIDATES, SpatialIndex};

/// Answer from a shortest-path query.
pub struct QueryAnswer {
    /// Total travel time in milliseconds.
    pub distance_ms: Weight,
    /// Total route distance in meters (Haversine sum of coordinate path).
    pub distance_m: f64,
    /// Ordered sequence of original graph arc IDs traversed by the route.
    pub route_arc_ids: Vec<u32>,
    /// Ordered sequence of graph-specific path IDs used to replay this route
    /// under the active weight space. For normal graphs this is identical to
    /// `route_arc_ids`; for line graphs it carries the line-graph node path.
    pub weight_path_ids: Vec<u32>,
    /// Ordered sequence of node IDs along the shortest path.
    pub path: Vec<NodeId>,
    /// Ordered (lat, lng) pairs for each path node (pure graph coordinates).
    pub coordinates: Vec<(f32, f32)>,
    /// Turn annotations along the path.
    /// Empty for normal-graph queries (no turn info available).
    pub turns: Vec<TurnAnnotation>,
    /// User-supplied origin coordinate (set by coordinate queries).
    pub origin: Option<(f32, f32)>,
    /// User-supplied destination coordinate (set by coordinate queries).
    pub destination: Option<(f32, f32)>,
}

/// Sum Haversine distances between consecutive coordinates to get total route
/// distance in meters.
pub fn route_distance_m(coordinates: &[(f32, f32)]) -> f64 {
    coordinates
        .windows(2)
        .map(|w| {
            crate::spatial::haversine_m(w[0].0 as f64, w[0].1 as f64, w[1].0 as f64, w[1].1 as f64)
        })
        .sum()
}

/// Metric-independent CCH context. Owns the graph data, the CCH topology,
/// and the baseline weight vector. Reusable across customizations.
pub struct CchContext {
    pub graph: GraphData,
    pub cch: CCH,
    pub baseline_weights: Vec<Weight>,
}

impl CchContext {
    /// Load graph data and CCH ordering, then build the CCH (Phase 1).
    ///
    /// `graph_dir` — directory with `first_out`, `head`, `travel_time`, `latitude`, `longitude`
    /// `perm_path` — path to the `cch_perm` file (typically `<graph_dir>/perms/cch_perm`)
    #[tracing::instrument(skip_all, fields(graph_dir = %graph_dir.display()))]
    pub fn load_and_build(graph_dir: &Path, perm_path: &Path) -> std::io::Result<Self> {
        let graph = GraphData::load(graph_dir)?;
        let perm: Vec<NodeId> = Vec::load_from(perm_path)?;
        let order = NodeOrder::from_node_order(perm);

        tracing::info!(
            num_nodes = graph.num_nodes(),
            num_edges = graph.num_edges(),
            "building CCH"
        );

        let borrowed = graph.as_borrowed_graph();
        let cch = CCH::fix_order_and_build(&borrowed, order);

        let baseline_weights = graph.travel_time.clone();

        Ok(CchContext {
            graph,
            cch,
            baseline_weights,
        })
    }

    /// Customize with baseline weights (Phase 2).
    #[tracing::instrument(skip_all)]
    pub fn customize(&self) -> CustomizedBasic<'_, CCH> {
        let metric = FirstOutGraph::new(
            &self.graph.first_out[..],
            &self.graph.head[..],
            &self.baseline_weights[..],
        );
        customize(&self.cch, &metric)
    }

    /// Customize with caller-provided weights (Phase 2).
    #[tracing::instrument(skip_all, fields(num_weights = weights.len()))]
    pub fn customize_with(&self, weights: &[Weight]) -> CustomizedBasic<'_, CCH> {
        let metric = FirstOutGraph::new(&self.graph.first_out[..], &self.graph.head[..], weights);
        customize(&self.cch, &metric)
    }
}

/// Query engine wrapping a CCH query server. Borrows a `CchContext` for its
/// lifetime so the CCH topology is guaranteed to outlive the customized data.
pub struct QueryEngine<'a> {
    server: CchQueryServer<CustomizedBasic<'a, CCH>>,
    context: &'a CchContext,
    spatial: SpatialIndex,
    validation_config: ValidationConfig,
}

impl<'a> QueryEngine<'a> {
    /// Create a query engine from a CCH context. Performs initial customization
    /// with baseline weights and builds the spatial index.
    pub fn new(context: &'a CchContext) -> Self {
        Self::with_validation_config(context, ValidationConfig::default())
    }

    pub fn with_validation_config(
        context: &'a CchContext,
        validation_config: ValidationConfig,
    ) -> Self {
        let customized = context.customize();
        let server = CchQueryServer::new(customized);

        let spatial = SpatialIndex::build(
            &context.graph.latitude,
            &context.graph.longitude,
            &context.graph.first_out,
            &context.graph.head,
        );

        QueryEngine {
            server,
            context,
            spatial,
            validation_config,
        }
    }

    /// Query by node IDs. Returns None if no path exists.
    #[tracing::instrument(skip(self), fields(from, to))]
    pub fn query(&mut self, from: NodeId, to: NodeId) -> Option<QueryAnswer> {
        let result = self.server.query(Query { from, to });

        if let Some(mut connected) = result.found() {
            let distance_ms = connected.distance();
            if distance_ms >= INFINITY {
                return None;
            }
            let path = connected.node_path();
            let route_arc_ids = self.reconstruct_arc_ids(&path)?;
            let weight_path_ids = route_arc_ids.clone();
            let coordinates: Vec<(f32, f32)> = path
                .iter()
                .map(|&node| {
                    (
                        self.context.graph.latitude[node as usize],
                        self.context.graph.longitude[node as usize],
                    )
                })
                .collect();
            let distance_m = route_distance_m(&coordinates);
            Some(QueryAnswer {
                distance_ms,
                distance_m,
                route_arc_ids,
                weight_path_ids,
                path,
                coordinates,
                turns: vec![],
                origin: None,
                destination: None,
            })
        } else {
            None
        }
    }

    /// Query by coordinates using ranked snap candidates in the original graph.
    #[tracing::instrument(skip(self), fields(
        from_lat = from.0, from_lng = from.1,
        to_lat = to.0, to_lng = to.1
    ))]
    pub fn query_coords(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
    ) -> Result<Option<QueryAnswer>, CoordRejection> {
        let src_snaps = self.spatial.validated_snap_candidates(
            "origin",
            from.0,
            from.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;
        let dst_snaps = self.spatial.validated_snap_candidates(
            "destination",
            to.0,
            to.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;

        let mut best: Option<QueryAnswer> = None;

        for src in &src_snaps {
            for dst in &dst_snaps {
                let s = src.nearest_node();
                let d = dst.nearest_node();

                if let Some(answer) = self.query(s, d) {
                    let is_better = best
                        .as_ref()
                        .map_or(true, |b| answer.distance_ms < b.distance_ms);
                    if is_better {
                        best = Some(answer);
                    }
                    break;
                }
            }

            if best.is_some() {
                break;
            }
        }

        Ok(best.map(|answer| Self::patch_coordinates(answer, from, to)))
    }

    /// Find up to `max_alternatives` alternative routes by node IDs.
    /// TODO: Consider add tracing for logging
    pub fn multi_query(
        &mut self,
        from: NodeId,
        to: NodeId,
        max_alternatives: usize,
        stretch_factor: f64,
    ) -> Vec<QueryAnswer> {
        let customized = self.server.customized();
        let mut multi = MultiRouteServer::new(customized);
        let request_count = max_alternatives
            .saturating_mul(GEO_OVER_REQUEST)
            .max(max_alternatives + 10);
        let candidates = multi.multi_query(from, to, request_count, stretch_factor);

        let mut results: Vec<QueryAnswer> = Vec::with_capacity(max_alternatives);
        let mut shortest_geo_dist: Option<f64> = None;

        for alt in candidates {
            if results.len() >= max_alternatives {
                break;
            }
            if alt.path.is_empty() {
                continue;
            }

            let coordinates: Vec<(f32, f32)> = alt
                .path
                .iter()
                .map(|&node| {
                    (
                        self.context.graph.latitude[node as usize],
                        self.context.graph.longitude[node as usize],
                    )
                })
                .collect();
            let distance_m = route_distance_m(&coordinates);

            if let Some(base) = shortest_geo_dist {
                if distance_m > base * MAX_GEO_RATIO {
                    continue;
                }
            } else {
                shortest_geo_dist = Some(distance_m);
            }

            let route_arc_ids = self.reconstruct_arc_ids(&alt.path).unwrap_or_default();
            let weight_path_ids = route_arc_ids.clone();

            results.push(QueryAnswer {
                distance_ms: alt.distance,
                distance_m,
                route_arc_ids,
                weight_path_ids,
                path: alt.path,
                coordinates,
                turns: vec![],
                origin: None,
                destination: None,
            });
        }

        results
    }

    /// Find up to `max_alternatives` alternative routes by coordinates.
    /// TODO: Consider add tracing for logging
    pub fn multi_query_coords(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
        max_alternatives: usize,
        stretch_factor: f64,
    ) -> Result<Vec<QueryAnswer>, CoordRejection> {
        let src_snaps = self.spatial.validated_snap_candidates(
            "origin",
            from.0,
            from.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;
        let dst_snaps = self.spatial.validated_snap_candidates(
            "destination",
            to.0,
            to.1,
            &self.validation_config,
            SNAP_MAX_CANDIDATES,
        )?;

        for src in &src_snaps {
            for dst in &dst_snaps {
                let answers = self.multi_query(
                    src.nearest_node(),
                    dst.nearest_node(),
                    max_alternatives,
                    stretch_factor,
                );
                if !answers.is_empty() {
                    let patched: Vec<QueryAnswer> = answers
                        .into_iter()
                        .map(|a| Self::patch_coordinates(a, from, to))
                        .collect();
                    return Ok(patched);
                }
            }
        }

        Ok(Vec::new())
    }

    /// Attach the user's origin and destination coordinates to the query answer
    /// as metadata — the coordinate path itself stays pure (graph nodes only).
    fn patch_coordinates(mut answer: QueryAnswer, from: (f32, f32), to: (f32, f32)) -> QueryAnswer {
        answer.origin = Some(from);
        answer.destination = Some(to);
        answer
    }

    fn reconstruct_arc_ids(&self, path: &[NodeId]) -> Option<Vec<u32>> {
        if path.len() < 2 {
            return Some(Vec::new());
        }

        let mut arc_ids = Vec::with_capacity(path.len() - 1);
        for window in path.windows(2) {
            let tail = window[0] as usize;
            let head = window[1];
            let start = self.context.graph.first_out[tail] as usize;
            let end = self.context.graph.first_out[tail + 1] as usize;
            let edge_idx =
                (start..end).find(|&edge_idx| self.context.graph.head[edge_idx] == head)?;
            arc_ids.push(edge_idx as u32);
        }
        Some(arc_ids)
    }

    /// Apply new weights and re-customize. The CCH topology is reused.
    /// The old customized data is dropped and replaced atomically via swap.
    pub fn update_weights(&mut self, weights: &[Weight]) {
        let new_customized = self.context.customize_with(weights);
        self.server.update(new_customized);
    }

    pub fn bbox(&self) -> &BoundingBox {
        self.spatial.bbox()
    }

    pub fn validation_config(&self) -> &ValidationConfig {
        &self.validation_config
    }

    /// Access the snap-to-edge spatial index.
    pub fn spatial(&self) -> &SpatialIndex {
        &self.spatial
    }

    /// Access the underlying CCH context.
    pub fn context(&self) -> &'a CchContext {
        self.context
    }
}
