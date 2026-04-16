use std::path::Path;

use rust_road_router::algo::customizable_contraction_hierarchy::CustomizedBasic;
use rust_road_router::algo::customizable_contraction_hierarchy::{CCH, customize};
use rust_road_router::datastr::graph::{FirstOutGraph, NodeId, Weight};
use rust_road_router::datastr::node_order::NodeOrder;
use rust_road_router::io::Load;
use rust_road_router::util::Storage;

use crate::graph::cache::CchCache;
use crate::graph::data::GraphData;

/// Metric-independent CCH context. Owns the graph data, the CCH topology,
/// and the baseline weight vector. Reusable across customizations.
pub struct CchContext {
    pub graph: GraphData,
    pub cch: CCH,
    pub baseline_weights: Storage<Weight>,
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
            "preparing CCH"
        );

        let borrowed = graph.as_borrowed_graph();
        let cache = CchCache::new(graph_dir);
        let source_files = [
            graph_dir.join("first_out"),
            graph_dir.join("head"),
            perm_path.to_path_buf(),
        ];
        let source_refs: Vec<&Path> = source_files.iter().map(|path| path.as_path()).collect();
        let cch = 'build: {
            if cache.is_valid(&source_refs) {
                match cache.load_cch(&borrowed) {
                    Ok(loaded) => {
                        tracing::info!("loaded CCH from cache");
                        break 'build loaded;
                    }
                    Err(err) => {
                        tracing::warn!("cached CCH failed validation: {err}; rebuilding");
                    }
                }
            }

            tracing::info!("building CCH from scratch");
            let built = CCH::fix_order_and_build(&borrowed, order);
            if let Err(err) = cache.save_cch(&built, &source_refs) {
                tracing::warn!("failed to write CCH cache: {err}");
            }
            built
        };

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
