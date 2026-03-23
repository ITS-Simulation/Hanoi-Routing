use std::path::Path;

use rust_road_router::algo::customizable_contraction_hierarchy::CustomizedBasic;
use rust_road_router::algo::customizable_contraction_hierarchy::query::Server as CchQueryServer;
use rust_road_router::algo::customizable_contraction_hierarchy::{CCH, customize};
use rust_road_router::algo::{Query, QueryServer};
use rust_road_router::datastr::graph::{FirstOutGraph, INFINITY, NodeId, Weight};
use rust_road_router::datastr::node_order::NodeOrder;
use rust_road_router::io::Load;

use crate::bounds::{BoundingBox, CoordRejection, ValidationConfig};
use crate::graph::GraphData;
use crate::spatial::SpatialIndex;

/// Answer from a shortest-path query.
pub struct QueryAnswer {
    /// Total travel time in milliseconds.
    pub distance_ms: Weight,
    /// Total route distance in meters (Haversine sum of coordinate path).
    pub distance_m: f64,
    /// Ordered sequence of node IDs along the shortest path.
    pub path: Vec<NodeId>,
    /// Ordered (lat, lng) pairs for each path node.
    pub coordinates: Vec<(f32, f32)>,
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
                path,
                coordinates,
            })
        } else {
            None
        }
    }

    /// Query by coordinates. Snaps to nearest edge, uses the projection parameter
    /// `t` to pick the nearest endpoint, then runs a single CCH query.
    /// Falls back to the other endpoint pair if the primary query finds no path.
    #[tracing::instrument(skip(self), fields(
        from_lat = from.0, from_lng = from.1,
        to_lat = to.0, to_lng = to.1
    ))]
    pub fn query_coords(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
    ) -> Result<Option<QueryAnswer>, CoordRejection> {
        let src = self
            .spatial
            .validated_snap("origin", from.0, from.1, &self.validation_config)?;
        let dst =
            self.spatial
                .validated_snap("destination", to.0, to.1, &self.validation_config)?;

        // Primary: route from the nearest endpoint of each snapped edge
        let s = src.nearest_node();
        let d = dst.nearest_node();
        if let Some(answer) = self.query(s, d) {
            return Ok(Some(Self::patch_coordinates(answer, from, to)));
        }

        // Fallback: try all 4 endpoint combinations (handles disconnected components,
        // one-way streets where the nearest node is unreachable, etc.)
        let src_nodes = [src.tail, src.head];
        let dst_nodes = [dst.tail, dst.head];
        let mut best: Option<QueryAnswer> = None;

        for &sn in &src_nodes {
            for &dn in &dst_nodes {
                if sn == s && dn == d {
                    continue; // Already tried this pair
                }
                if let Some(answer) = self.query(sn, dn) {
                    let is_better = best
                        .as_ref()
                        .map_or(true, |b| answer.distance_ms < b.distance_ms);
                    if is_better {
                        best = Some(answer);
                    }
                }
            }
        }

        Ok(best.map(|answer| Self::patch_coordinates(answer, from, to)))
    }

    /// Prepend the user's origin coordinate and append the destination coordinate
    /// to the query result's coordinate path, so map visualizations show the full
    /// journey from the user's actual position.
    fn patch_coordinates(mut answer: QueryAnswer, from: (f32, f32), to: (f32, f32)) -> QueryAnswer {
        answer.coordinates.insert(0, from);
        answer.coordinates.push(to);
        answer.distance_m = route_distance_m(&answer.coordinates);
        answer
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
