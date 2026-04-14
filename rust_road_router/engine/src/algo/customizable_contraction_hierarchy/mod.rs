//! Implementation of Customizable Contraction Hierarchies.

use super::*;
use crate::{
    datastr::{
        graph::first_out_graph::BorrowedGraph,
        node_order::{NodeOrder, Rank},
    },
    io::*,
    report::{benchmark::*, block_reporting},
    util::{in_range_option::InRangeOption, *},
};
use std::{cmp::Ordering, ops::Range};

mod contraction;
use contraction::*;
pub mod customization;
pub use customization::ftd as ftd_cch;
pub use customization::{customize, customize_directed, customize_directed_perfect, customize_perfect};
pub mod separator_decomposition;
use separator_decomposition::*;
mod reorder;
pub use reorder::*;
pub mod query;

/// Execute first phase, that is metric independent preprocessing.
pub fn contract<Graph: LinkIterable<NodeIdT> + EdgeIdGraph>(graph: &Graph, node_order: NodeOrder) -> CCH {
    CCH::new(ContractionGraph::new(graph, node_order).contract())
}

/// A struct containing all metric independent preprocessing data of CCHs.
/// This includes on top of the chordal supergraph (the "contracted" graph),
/// several other structures like the elimination tree, a mapping from cch edge ids to original edge ids and the inverted graph.
pub struct CCH {
    first_out: Vec<EdgeId>,
    head: Vec<NodeId>,
    tail: Vec<NodeId>,
    node_order: NodeOrder,
    forward_cch_edge_to_orig_arc: Vecs<EdgeIdT>,
    backward_cch_edge_to_orig_arc: Vecs<EdgeIdT>,
    elimination_tree: Vec<InRangeOption<NodeId>>,
    inverted: ReversedGraphWithEdgeIds,
    separator_tree: SeparatorTree,
}

impl Deconstruct for CCH {
    fn save_each(&self, store: &dyn Fn(&str, &dyn Save) -> std::io::Result<()>) -> std::io::Result<()> {
        store("cch_first_out", &self.first_out)?;
        store("cch_head", &self.head)?;
        Ok(())
    }
}

pub struct CCHReconstrctor<'g, Graph>(pub &'g Graph);

impl<'g, Graph: EdgeIdGraph> ReconstructPrepared<CCH> for CCHReconstrctor<'g, Graph> {
    fn reconstruct_with(self, loader: Loader) -> std::io::Result<CCH> {
        let node_order = NodeOrder::reconstruct_from(&loader.path())?;
        let head: Vec<NodeId> = loader.load("cch_head")?;
        let cch_graph = UnweightedOwnedGraph::new(loader.load("cch_first_out")?, head);
        assert_eq!(cch_graph.num_nodes(), self.0.num_nodes());
        Ok(CCH::new_from(self.0, node_order, cch_graph))
    }
}

impl CCH {
    pub fn fix_order_and_build(graph: &(impl LinkIterable<NodeIdT> + EdgeIdGraph), order: NodeOrder) -> Self {
        let contracted = {
            let _blocked = block_reporting();
            ContractionGraph::new(graph, order.clone()).contract()
        };
        let order = reorder_for_seperator_based_customization(&order, SeparatorTree::new(&contracted.elimination_tree()));
        contract(graph, order)
    }

    fn new<Graph: EdgeIdGraph>(contracted_graph: ContractedGraph<Graph>) -> CCH {
        let (cch, order, orig) = contracted_graph.decompose();
        Self::new_from(orig, order, cch)
    }

    // this method creates all the other structures from the contracted graph
    fn new_from<Graph: EdgeIdGraph>(original_graph: &Graph, node_order: NodeOrder, contracted_graph: UnweightedOwnedGraph) -> Self {
        let elimination_tree = Self::build_elimination_tree(&contracted_graph);
        let n = contracted_graph.num_nodes() as NodeId;
        let m = contracted_graph.num_arcs();
        let mut tail = vec![0; m];

        let order = &node_order;
        let forward_cch_edge_to_orig_arc = Vecs::from_iters((0..n).flat_map(|node| {
            LinkIterable::<NodeIdT>::link_iter(&contracted_graph, node)
                .map(move |NodeIdT(neighbor)| original_graph.edge_indices(order.node(node), order.node(neighbor)))
        }));
        let backward_cch_edge_to_orig_arc = Vecs::from_iters((0..n).flat_map(|node| {
            LinkIterable::<NodeIdT>::link_iter(&contracted_graph, node)
                .map(move |NodeIdT(neighbor)| original_graph.edge_indices(order.node(neighbor), order.node(node)))
        }));

        for node in 0..n {
            tail[contracted_graph.neighbor_edge_indices_usize(node)]
                .iter_mut()
                .for_each(|tail| *tail = node);
        }

        let inverted = ReversedGraphWithEdgeIds::reversed(&contracted_graph);
        let (first_out, head) = contracted_graph.decompose();

        CCH {
            first_out,
            head,
            node_order,
            forward_cch_edge_to_orig_arc,
            backward_cch_edge_to_orig_arc,
            tail,
            inverted,
            separator_tree: SeparatorTree::new(&elimination_tree),
            elimination_tree,
        }
    }

    fn build_elimination_tree(graph: &UnweightedOwnedGraph) -> Vec<InRangeOption<NodeId>> {
        (0..graph.num_nodes())
            .map(|node_id| LinkIterable::<NodeIdT>::link_iter(graph, node_id as NodeId).map(|NodeIdT(n)| n).next())
            .map(InRangeOption::new)
            .collect()
    }

    /// Get the tail node for an edge id
    pub fn edge_id_to_tail(&self, edge_id: EdgeId) -> NodeId {
        self.tail[edge_id as usize]
    }

    /// Get chordal supergraph `first_out` as slice
    pub fn first_out(&self) -> &[EdgeId] {
        &self.first_out
    }

    /// Get chordal supergraph `head` as slice
    pub fn head(&self) -> &[NodeId] {
        &self.head
    }

    #[inline]
    pub fn neighbor_edge_indices(&self, node: NodeId) -> Range<EdgeId> {
        (self.first_out[node as usize])..(self.first_out[(node + 1) as usize])
    }

    #[inline]
    pub fn neighbor_edge_indices_usize(&self, node: NodeId) -> Range<usize> {
        let range = self.neighbor_edge_indices(node);
        Range {
            start: range.start as usize,
            end: range.end as usize,
        }
    }

    #[inline]
    pub fn neighbor_iter(&self, node: NodeId) -> std::iter::Cloned<std::slice::Iter<'_, NodeId>> {
        let range = self.neighbor_edge_indices_usize(node);
        self.head[range].iter().cloned()
    }

    /// Transform into a directed CCH which is more efficient
    /// for turn expanded graphs because many edges can be removed.
    pub fn to_directed_cch(&self) -> DirectedCCH {
        // identify arcs which are always infinity and can be removed
        let customized = customization::always_infinity(self);
        let forward = customized.forward_graph();
        let backward = customized.backward_graph();

        let mut forward_first_out = Vec::with_capacity(self.first_out.len());
        forward_first_out.push(0);
        let mut forward_head = Vec::with_capacity(self.head.len());
        let mut forward_tail = Vec::with_capacity(self.head.len());

        let mut backward_first_out = Vec::with_capacity(self.first_out.len());
        backward_first_out.push(0);
        let mut backward_head = Vec::with_capacity(self.head.len());
        let mut backward_tail = Vec::with_capacity(self.head.len());

        let forward_cch_edge_to_orig_arc = Vecs::from_iters(
            self.forward_cch_edge_to_orig_arc
                .iter()
                .zip(forward.weight().iter())
                .filter(|(_, w)| **w < INFINITY)
                .map(|(slc, _)| slc.iter().copied()),
        );
        let backward_cch_edge_to_orig_arc = Vecs::from_iters(
            self.backward_cch_edge_to_orig_arc
                .iter()
                .zip(backward.weight().iter())
                .filter(|(_, w)| **w < INFINITY)
                .map(|(slc, _)| slc.iter().copied()),
        );

        for node in 0..self.num_nodes() as NodeId {
            forward_head.extend(LinkIterable::<Link>::link_iter(&forward, node).filter(|l| l.weight < INFINITY).map(|l| l.node));
            backward_head.extend(LinkIterable::<Link>::link_iter(&backward, node).filter(|l| l.weight < INFINITY).map(|l| l.node));
            forward_tail.extend(LinkIterable::<Link>::link_iter(&forward, node).filter(|l| l.weight < INFINITY).map(|_| node));
            backward_tail.extend(LinkIterable::<Link>::link_iter(&backward, node).filter(|l| l.weight < INFINITY).map(|_| node));
            forward_first_out.push(forward_head.len() as EdgeId);
            backward_first_out.push(backward_head.len() as EdgeId);
        }

        let forward_inverted = ReversedGraphWithEdgeIds::reversed(&UnweightedFirstOutGraph::new(&forward_first_out[..], &forward_head[..]));
        let backward_inverted = ReversedGraphWithEdgeIds::reversed(&UnweightedFirstOutGraph::new(&backward_first_out[..], &backward_head[..]));

        DirectedCCH {
            forward_first_out: Storage::from_vec(forward_first_out),
            forward_head: Storage::from_vec(forward_head),
            forward_tail: Storage::from_vec(forward_tail),
            backward_first_out: Storage::from_vec(backward_first_out),
            backward_head: Storage::from_vec(backward_head),
            backward_tail: Storage::from_vec(backward_tail),
            node_order: self.node_order.clone(),
            forward_cch_edge_to_orig_arc,
            backward_cch_edge_to_orig_arc,
            elimination_tree: Storage::from_vec(self.elimination_tree.clone()),
            forward_inverted,
            backward_inverted,
            separator_tree: self.separator_tree.clone(),
        }
    }

    pub fn remove_always_infinity(&self) -> CCH {
        // identify arcs which are always infinity and can be removed
        let customized = customization::always_infinity(self);
        let forward = customized.forward_graph();
        let backward = customized.backward_graph();

        let mut first_out = Vec::with_capacity(self.first_out.len());
        first_out.push(0);
        let mut head = Vec::with_capacity(self.head.len());
        let mut tail = Vec::with_capacity(self.head.len());

        let forward_cch_edge_to_orig_arc = Vecs::from_iters(
            self.forward_cch_edge_to_orig_arc
                .iter()
                .zip(forward.weight().iter())
                .zip(backward.weight().iter())
                .filter(|((_, fw_w), bw_w)| **fw_w < INFINITY || **bw_w < INFINITY)
                .map(|((slc, _), _)| slc.iter().copied()),
        );
        let backward_cch_edge_to_orig_arc = Vecs::from_iters(
            self.backward_cch_edge_to_orig_arc
                .iter()
                .zip(forward.weight().iter())
                .zip(backward.weight().iter())
                .filter(|((_, fw_w), bw_w)| **fw_w < INFINITY || **bw_w < INFINITY)
                .map(|((slc, _), _)| slc.iter().copied()),
        );

        for node in 0..self.num_nodes() as NodeId {
            let range = self.neighbor_edge_indices_usize(node);
            head.extend(
                self.head[range.clone()]
                    .iter()
                    .zip(forward.weight()[range.clone()].iter())
                    .zip(backward.weight()[range].iter())
                    .filter(|((_, fw_w), bw_w)| **fw_w < INFINITY || **bw_w < INFINITY)
                    .map(|((head, _), _)| *head),
            );
            tail.extend(std::iter::repeat(node).take(head.len() - *first_out.last().unwrap() as usize));
            first_out.push(head.len() as EdgeId);
        }

        let inverted = ReversedGraphWithEdgeIds::reversed(&UnweightedFirstOutGraph::new(&first_out[..], &head[..]));

        CCH {
            first_out,
            head,
            tail,
            node_order: self.node_order.clone(),
            forward_cch_edge_to_orig_arc,
            backward_cch_edge_to_orig_arc,
            elimination_tree: self.elimination_tree.clone(),
            inverted,
            separator_tree: self.separator_tree.clone(),
        }
    }
}

impl Graph for CCH {
    fn num_arcs(&self) -> usize {
        self.head.len()
    }

    fn num_nodes(&self) -> usize {
        self.first_out.len() - 1
    }

    fn degree(&self, node: NodeId) -> usize {
        let node = node as usize;
        (self.first_out[node + 1] - self.first_out[node]) as usize
    }
}

/// Trait for directed and undirected CCHs
pub trait CCHT {
    fn num_cch_nodes(&self) -> usize {
        self.forward_first_out().len() - 1
    }
    fn forward_first_out(&self) -> &[EdgeId];
    fn backward_first_out(&self) -> &[EdgeId];
    fn forward_head(&self) -> &[NodeId];
    fn backward_head(&self) -> &[NodeId];
    fn forward_tail(&self) -> &[NodeId];
    fn backward_tail(&self) -> &[NodeId];
    fn forward_inverted(&self) -> &ReversedGraphWithEdgeIds;
    fn backward_inverted(&self) -> &ReversedGraphWithEdgeIds;
    fn forward_cch_edge_to_orig_arc(&self) -> &Vecs<EdgeIdT>;
    fn backward_cch_edge_to_orig_arc(&self) -> &Vecs<EdgeIdT>;

    /// Get elimination tree (actually forest).
    /// The tree is represented as a slice of length `n`.
    /// The entry with index `x` contains the parent node in the tree of node `x`.
    /// If there is no parent, `x` is a root node.
    fn elimination_tree(&self) -> &[InRangeOption<NodeId>];

    /// Borrow node order
    fn node_order(&self) -> &NodeOrder;

    /// Reconstruct the separators of the nested dissection order.
    fn separators(&self) -> &SeparatorTree;

    fn forward(&self) -> Slcs<'_, EdgeId, NodeId> {
        Slcs(self.forward_first_out(), self.forward_head())
    }

    fn backward(&self) -> Slcs<'_, EdgeId, NodeId> {
        Slcs(self.backward_first_out(), self.backward_head())
    }
}

pub fn unpack_arc(
    from: NodeId,
    to: NodeId,
    weight: Weight,
    upward: &[Weight],
    downward: &[Weight],
    forward_inverted: &ReversedGraphWithEdgeIds,
    backward_inverted: &ReversedGraphWithEdgeIds,
) -> Option<(NodeId, Weight, Weight)> {
    // `inverted` contains the downward neighbors sorted ascending.
    // We do a coordinated linear sweep over both neighborhoods.
    // Whenever we find a common neighbor, we have a lower triangle.
    let mut current_iter = backward_inverted.link_iter(from).peekable();
    let mut other_iter = forward_inverted.link_iter(to).peekable();

    debug_assert_eq!(upward.len(), forward_inverted.num_arcs());
    debug_assert_eq!(downward.len(), backward_inverted.num_arcs());

    while let (
        Some(&(NodeIdT(lower_from_first), Reversed(EdgeIdT(edge_from_first_id)))),
        Some(&(NodeIdT(lower_from_second), Reversed(EdgeIdT(edge_from_second_id)))),
    ) = (current_iter.peek(), other_iter.peek())
    {
        match lower_from_first.cmp(&lower_from_second) {
            Ordering::Less => current_iter.next(),
            Ordering::Greater => other_iter.next(),
            Ordering::Equal => {
                if downward[edge_from_first_id as usize] + upward[edge_from_second_id as usize] == weight {
                    return Some((lower_from_first, downward[edge_from_first_id as usize], upward[edge_from_second_id as usize]));
                }

                current_iter.next();
                other_iter.next()
            }
        };
    }

    None
}

/// A struct containing all metric independent preprocessing data of CCHs.
/// This includes on top of the chordal supergraph (the "contracted" graph),
/// several other structures like the elimination tree, a mapping from cch edge ids to original edge ids and the inverted graph.
impl CCHT for CCH {
    fn forward_first_out(&self) -> &[EdgeId] {
        &self.first_out[..]
    }
    fn backward_first_out(&self) -> &[EdgeId] {
        &self.first_out[..]
    }
    fn forward_head(&self) -> &[NodeId] {
        &self.head[..]
    }
    fn backward_head(&self) -> &[NodeId] {
        &self.head[..]
    }
    fn forward_tail(&self) -> &[NodeId] {
        &self.tail[..]
    }
    fn backward_tail(&self) -> &[NodeId] {
        &self.tail[..]
    }
    fn forward_inverted(&self) -> &ReversedGraphWithEdgeIds {
        &self.inverted
    }
    fn backward_inverted(&self) -> &ReversedGraphWithEdgeIds {
        &self.inverted
    }
    fn forward_cch_edge_to_orig_arc(&self) -> &Vecs<EdgeIdT> {
        &self.forward_cch_edge_to_orig_arc
    }
    fn backward_cch_edge_to_orig_arc(&self) -> &Vecs<EdgeIdT> {
        &self.backward_cch_edge_to_orig_arc
    }

    fn node_order(&self) -> &NodeOrder {
        &self.node_order
    }

    fn elimination_tree(&self) -> &[InRangeOption<NodeId>] {
        &self.elimination_tree[..]
    }

    fn separators(&self) -> &SeparatorTree {
        &self.separator_tree
    }
}

pub trait Customized {
    type CCH: CCHT;
    fn forward_graph(&self) -> BorrowedGraph<'_>;
    fn backward_graph(&self) -> BorrowedGraph<'_>;
    fn cch(&self) -> &Self::CCH;

    fn unpack_outgoing(&self, edge: EdgeIdT) -> Option<(EdgeIdT, EdgeIdT, NodeIdT)>;
    fn unpack_incoming(&self, edge: EdgeIdT) -> Option<(EdgeIdT, EdgeIdT, NodeIdT)>;

    fn forward_tail(&self) -> &[NodeId];
    fn backward_tail(&self) -> &[NodeId];
    fn forward_unpacking(&self) -> &[(InRangeOption<EdgeId>, InRangeOption<EdgeId>)];
    fn backward_unpacking(&self) -> &[(InRangeOption<EdgeId>, InRangeOption<EdgeId>)];
}

/// A struct containing the results of the second preprocessing phase.
pub struct CustomizedBasic<'a, CCH> {
    pub cch: &'a CCH,
    upward: Vec<Weight>,
    downward: Vec<Weight>,
    up_unpacking: Vec<(InRangeOption<EdgeId>, InRangeOption<EdgeId>)>,
    down_unpacking: Vec<(InRangeOption<EdgeId>, InRangeOption<EdgeId>)>,
}

impl<'a, C: CCHT> CustomizedBasic<'a, C> {
    fn new(
        cch: &'a C,
        upward: Vec<Weight>,
        downward: Vec<Weight>,
        up_unpacking: Vec<(InRangeOption<EdgeId>, InRangeOption<EdgeId>)>,
        down_unpacking: Vec<(InRangeOption<EdgeId>, InRangeOption<EdgeId>)>,
    ) -> Self {
        Self {
            cch,
            upward,
            downward,
            up_unpacking,
            down_unpacking,
        }
    }

    pub fn into_weights(self) -> (Vec<Weight>, Vec<Weight>) {
        (self.upward, self.downward)
    }
}
impl<'a, C: CCHT> Customized for CustomizedBasic<'a, C> {
    type CCH = C;
    fn forward_graph(&self) -> BorrowedGraph<'_> {
        FirstOutGraph::new(self.cch.forward_first_out(), self.cch.forward_head(), &self.upward)
    }
    fn backward_graph(&self) -> BorrowedGraph<'_> {
        FirstOutGraph::new(self.cch.backward_first_out(), self.cch.backward_head(), &self.downward)
    }
    fn cch(&self) -> &C {
        self.cch
    }
    fn forward_tail(&self) -> &[NodeId] {
        self.cch().forward_tail()
    }
    fn backward_tail(&self) -> &[NodeId] {
        self.cch().backward_tail()
    }
    fn unpack_outgoing(&self, EdgeIdT(edge): EdgeIdT) -> Option<(EdgeIdT, EdgeIdT, NodeIdT)> {
        let (down, up) = self.up_unpacking[edge as usize];
        down.value()
            .map(|down| (EdgeIdT(down), EdgeIdT(up.value().unwrap()), NodeIdT(self.backward_tail()[down as usize])))
    }
    fn unpack_incoming(&self, EdgeIdT(edge): EdgeIdT) -> Option<(EdgeIdT, EdgeIdT, NodeIdT)> {
        let (down, up) = self.down_unpacking[edge as usize];
        down.value()
            .map(|down| (EdgeIdT(down), EdgeIdT(up.value().unwrap()), NodeIdT(self.backward_tail()[down as usize])))
    }
    fn forward_unpacking(&self) -> &[(InRangeOption<EdgeId>, InRangeOption<EdgeId>)] {
        &self.up_unpacking
    }
    fn backward_unpacking(&self) -> &[(InRangeOption<EdgeId>, InRangeOption<EdgeId>)] {
        &self.down_unpacking
    }
}

pub struct CustomizedPerfect<'a, CCH> {
    pub cch: &'a CCH,
    upward: OwnedGraph,
    downward: OwnedGraph,
    up_unpacking: Vec<(InRangeOption<EdgeId>, InRangeOption<EdgeId>)>,
    down_unpacking: Vec<(InRangeOption<EdgeId>, InRangeOption<EdgeId>)>,
    forward_tail: Vec<NodeId>,
    backward_tail: Vec<NodeId>,
}

impl<'a, C: CCHT> CustomizedPerfect<'a, C> {
    fn new(
        cch: &'a C,
        upward: OwnedGraph,
        downward: OwnedGraph,
        up_unpacking: Vec<(InRangeOption<EdgeId>, InRangeOption<EdgeId>)>,
        down_unpacking: Vec<(InRangeOption<EdgeId>, InRangeOption<EdgeId>)>,
        forward_tail: Vec<NodeId>,
        backward_tail: Vec<NodeId>,
    ) -> Self {
        Self {
            cch,
            upward,
            downward,
            up_unpacking,
            down_unpacking,
            forward_tail,
            backward_tail,
        }
    }
}
impl<'a, C: CCHT> Customized for CustomizedPerfect<'a, C> {
    type CCH = C;
    fn forward_graph(&self) -> BorrowedGraph<'_> {
        self.upward.borrowed()
    }
    fn backward_graph(&self) -> BorrowedGraph<'_> {
        self.downward.borrowed()
    }
    fn cch(&self) -> &C {
        self.cch
    }
    fn forward_tail(&self) -> &[NodeId] {
        &self.forward_tail
    }
    fn backward_tail(&self) -> &[NodeId] {
        &self.backward_tail
    }
    fn unpack_outgoing(&self, EdgeIdT(edge): EdgeIdT) -> Option<(EdgeIdT, EdgeIdT, NodeIdT)> {
        let (down, up) = self.up_unpacking[edge as usize];
        down.value()
            .map(|down| (EdgeIdT(down), EdgeIdT(up.value().unwrap()), NodeIdT(self.backward_tail()[down as usize])))
    }
    fn unpack_incoming(&self, EdgeIdT(edge): EdgeIdT) -> Option<(EdgeIdT, EdgeIdT, NodeIdT)> {
        let (down, up) = self.down_unpacking[edge as usize];
        down.value()
            .map(|down| (EdgeIdT(down), EdgeIdT(up.value().unwrap()), NodeIdT(self.backward_tail()[down as usize])))
    }
    fn forward_unpacking(&self) -> &[(InRangeOption<EdgeId>, InRangeOption<EdgeId>)] {
        &self.up_unpacking
    }
    fn backward_unpacking(&self) -> &[(InRangeOption<EdgeId>, InRangeOption<EdgeId>)] {
        &self.down_unpacking
    }
}

impl<C> crate::io::Deconstruct for CustomizedPerfect<'_, C> {
    fn save_each(&self, store: &dyn Fn(&str, &dyn crate::io::Save) -> std::io::Result<()>) -> std::io::Result<()> {
        store("fw_graph", &Sub(&self.upward))?;
        store("bw_graph", &Sub(&self.downward))?;
        store("up_unpacking", &self.up_unpacking)?;
        store("down_unpacking", &self.down_unpacking)?;
        Ok(())
    }
}

impl<'a, C: CCHT> crate::io::ReconstructPrepared<CustomizedPerfect<'a, C>> for &'a C {
    fn reconstruct_with(self, loader: Loader) -> std::io::Result<CustomizedPerfect<'a, C>> {
        let upward: OwnedGraph = loader.reconstruct("fw_graph")?;
        let downward: OwnedGraph = loader.reconstruct("bw_graph")?;

        let forward_tail = upward
            .first_out()
            .array_windows::<2>()
            .enumerate()
            .flat_map(|(tail, [idx_from, idx_to])| std::iter::repeat(tail as EdgeId).take((idx_to - idx_from) as usize))
            .collect();

        let backward_tail = downward
            .first_out()
            .array_windows::<2>()
            .enumerate()
            .flat_map(|(tail, [idx_from, idx_to])| std::iter::repeat(tail as EdgeId).take((idx_to - idx_from) as usize))
            .collect();

        Ok(CustomizedPerfect {
            cch: self,
            upward,
            downward,
            up_unpacking: loader.load("up_unpacking")?,
            down_unpacking: loader.load("down_unpacking")?,
            forward_tail,
            backward_tail,
        })
    }
}

fn cache_check(cond: bool, msg: impl Into<String>) -> std::io::Result<()> {
    if cond {
        Ok(())
    } else {
        Err(std::io::Error::new(std::io::ErrorKind::InvalidData, msg.into()))
    }
}

impl crate::io::Deconstruct for DirectedCCH {
    fn save_each(&self, store: &dyn Fn(&str, &dyn crate::io::Save) -> std::io::Result<()>) -> std::io::Result<()> {
        store("directed_fw_first_out", &&self.forward_first_out[..])?;
        store("directed_fw_head", &&self.forward_head[..])?;
        store("directed_fw_tail", &&self.forward_tail[..])?;
        store("directed_bw_first_out", &&self.backward_first_out[..])?;
        store("directed_bw_head", &&self.backward_head[..])?;
        store("directed_bw_tail", &&self.backward_tail[..])?;

        let fw_edge_to_orig_idx = self.forward_cch_edge_to_orig_arc.first_idx_as_u32();
        store("fw_edge_to_orig_idx", &fw_edge_to_orig_idx)?;
        let fw_edge_to_orig_data: Vec<u32> = self.forward_cch_edge_to_orig_arc.data_as_slice().iter().map(|&EdgeIdT(x)| x).collect();
        store("fw_edge_to_orig_data", &fw_edge_to_orig_data)?;

        let bw_edge_to_orig_idx = self.backward_cch_edge_to_orig_arc.first_idx_as_u32();
        store("bw_edge_to_orig_idx", &bw_edge_to_orig_idx)?;
        let bw_edge_to_orig_data: Vec<u32> = self.backward_cch_edge_to_orig_arc.data_as_slice().iter().map(|&EdgeIdT(x)| x).collect();
        store("bw_edge_to_orig_data", &bw_edge_to_orig_data)?;

        store("fw_inverted_first_out", &self.forward_inverted.first_out())?;
        store("fw_inverted_head", &self.forward_inverted.head_slice())?;
        store("fw_inverted_edge_ids", &self.forward_inverted.edge_ids())?;
        store("bw_inverted_first_out", &self.backward_inverted.first_out())?;
        store("bw_inverted_head", &self.backward_inverted.head_slice())?;
        store("bw_inverted_edge_ids", &self.backward_inverted.edge_ids())?;

        let elimination_tree: Vec<u32> = self.elimination_tree.iter().map(|opt| opt.value().unwrap_or(u32::MAX)).collect();
        store("elimination_tree", &elimination_tree)?;
        self.node_order.save_each(&|name, data| store(name, data))?;

        Ok(())
    }
}

pub struct DirectedCCHReconstructor;

impl crate::io::ReconstructPrepared<DirectedCCH> for DirectedCCHReconstructor {
    fn reconstruct_with(self, loader: Loader) -> std::io::Result<DirectedCCH> {
        let forward_first_out = Storage::<EdgeId>::mmap(loader.path().join("directed_fw_first_out"))?;
        let forward_head = Storage::<NodeId>::mmap(loader.path().join("directed_fw_head"))?;
        let forward_tail = Storage::<NodeId>::mmap(loader.path().join("directed_fw_tail"))?;
        let backward_first_out = Storage::<EdgeId>::mmap(loader.path().join("directed_bw_first_out"))?;
        let backward_head = Storage::<NodeId>::mmap(loader.path().join("directed_bw_head"))?;
        let backward_tail = Storage::<NodeId>::mmap(loader.path().join("directed_bw_tail"))?;

        cache_check(!forward_first_out.is_empty(), "fw first_out empty")?;
        cache_check(*forward_first_out.first().unwrap() == 0, "fw first_out[0] != 0")?;
        cache_check(
            *forward_first_out.last().unwrap() as usize == forward_head.len(),
            format!(
                "fw first_out sentinel ({}) != head.len ({})",
                forward_first_out.last().unwrap(),
                forward_head.len()
            ),
        )?;
        for window in forward_first_out.windows(2) {
            cache_check(window[0] <= window[1], "fw first_out not monotonically non-decreasing")?;
        }
        cache_check(forward_head.len() == forward_tail.len(), "fw head.len != tail.len")?;

        cache_check(!backward_first_out.is_empty(), "bw first_out empty")?;
        cache_check(*backward_first_out.first().unwrap() == 0, "bw first_out[0] != 0")?;
        cache_check(
            *backward_first_out.last().unwrap() as usize == backward_head.len(),
            format!(
                "bw first_out sentinel ({}) != head.len ({})",
                backward_first_out.last().unwrap(),
                backward_head.len()
            ),
        )?;
        for window in backward_first_out.windows(2) {
            cache_check(window[0] <= window[1], "bw first_out not monotonically non-decreasing")?;
        }
        cache_check(backward_head.len() == backward_tail.len(), "bw head.len != tail.len")?;
        cache_check(
            forward_first_out.len() == backward_first_out.len(),
            "fw/bw first_out length mismatch (different num_nodes)",
        )?;

        let num_nodes = forward_first_out.len() - 1;
        let num_fw_edges = forward_head.len();
        let num_bw_edges = backward_head.len();

        for &head in forward_head.iter() {
            cache_check((head as usize) < num_nodes, format!("fw head value {} >= num_nodes {}", head, num_nodes))?;
        }
        for &tail in forward_tail.iter() {
            cache_check((tail as usize) < num_nodes, format!("fw tail value {} >= num_nodes {}", tail, num_nodes))?;
        }
        for &head in backward_head.iter() {
            cache_check((head as usize) < num_nodes, format!("bw head value {} >= num_nodes {}", head, num_nodes))?;
        }
        for &tail in backward_tail.iter() {
            cache_check((tail as usize) < num_nodes, format!("bw tail value {} >= num_nodes {}", tail, num_nodes))?;
        }

        let fw_idx = Storage::<u32>::mmap(loader.path().join("fw_edge_to_orig_idx"))?;
        let fw_data = Storage::<EdgeIdT>::mmap(loader.path().join("fw_edge_to_orig_data"))?;
        cache_check(
            fw_idx.len() == num_fw_edges + 1,
            format!("fw_edge_to_orig_idx.len ({}) != num_fw_edges+1 ({})", fw_idx.len(), num_fw_edges + 1),
        )?;
        let forward_cch_edge_to_orig_arc = Vecs::from_storage(fw_idx, fw_data)?;

        let bw_idx = Storage::<u32>::mmap(loader.path().join("bw_edge_to_orig_idx"))?;
        let bw_data = Storage::<EdgeIdT>::mmap(loader.path().join("bw_edge_to_orig_data"))?;
        cache_check(
            bw_idx.len() == num_bw_edges + 1,
            format!("bw_edge_to_orig_idx.len ({}) != num_bw_edges+1 ({})", bw_idx.len(), num_bw_edges + 1),
        )?;
        let backward_cch_edge_to_orig_arc = Vecs::from_storage(bw_idx, bw_data)?;

        let fw_inv_first_out = Storage::<EdgeId>::mmap(loader.path().join("fw_inverted_first_out"))?;
        let fw_inv_head = Storage::<NodeId>::mmap(loader.path().join("fw_inverted_head"))?;
        let fw_inv_edge_ids = Storage::<EdgeId>::mmap(loader.path().join("fw_inverted_edge_ids"))?;
        cache_check(
            fw_inv_first_out.len() == num_nodes + 1,
            format!("fw_inverted first_out.len ({}) != num_nodes+1 ({})", fw_inv_first_out.len(), num_nodes + 1),
        )?;
        cache_check(
            fw_inv_head.len() == num_fw_edges,
            format!("fw_inverted head.len ({}) != num_fw_edges ({})", fw_inv_head.len(), num_fw_edges),
        )?;
        for &head in fw_inv_head.iter() {
            cache_check(
                (head as usize) < num_nodes,
                format!("fw_inverted head value {} >= num_nodes {}", head, num_nodes),
            )?;
        }
        cache_check(
            fw_inv_edge_ids.len() == num_fw_edges,
            format!("fw_inverted edge_ids.len ({}) != num_fw_edges ({})", fw_inv_edge_ids.len(), num_fw_edges),
        )?;
        for &edge_id in fw_inv_edge_ids.iter() {
            cache_check(
                (edge_id as usize) < num_fw_edges,
                format!("fw_inverted edge_id {} >= num_fw_edges {}", edge_id, num_fw_edges),
            )?;
        }
        let forward_inverted = ReversedGraphWithEdgeIds::from_storage_validated(fw_inv_first_out, fw_inv_head, fw_inv_edge_ids)?;

        let bw_inv_first_out = Storage::<EdgeId>::mmap(loader.path().join("bw_inverted_first_out"))?;
        let bw_inv_head = Storage::<NodeId>::mmap(loader.path().join("bw_inverted_head"))?;
        let bw_inv_edge_ids = Storage::<EdgeId>::mmap(loader.path().join("bw_inverted_edge_ids"))?;
        cache_check(
            bw_inv_first_out.len() == num_nodes + 1,
            format!("bw_inverted first_out.len ({}) != num_nodes+1 ({})", bw_inv_first_out.len(), num_nodes + 1),
        )?;
        cache_check(
            bw_inv_head.len() == num_bw_edges,
            format!("bw_inverted head.len ({}) != num_bw_edges ({})", bw_inv_head.len(), num_bw_edges),
        )?;
        for &head in bw_inv_head.iter() {
            cache_check(
                (head as usize) < num_nodes,
                format!("bw_inverted head value {} >= num_nodes {}", head, num_nodes),
            )?;
        }
        cache_check(
            bw_inv_edge_ids.len() == num_bw_edges,
            format!("bw_inverted edge_ids.len ({}) != num_bw_edges ({})", bw_inv_edge_ids.len(), num_bw_edges),
        )?;
        for &edge_id in bw_inv_edge_ids.iter() {
            cache_check(
                (edge_id as usize) < num_bw_edges,
                format!("bw_inverted edge_id {} >= num_bw_edges {}", edge_id, num_bw_edges),
            )?;
        }
        let backward_inverted = ReversedGraphWithEdgeIds::from_storage_validated(bw_inv_first_out, bw_inv_head, bw_inv_edge_ids)?;

        let elimination_tree = Storage::<InRangeOption<NodeId>>::mmap(loader.path().join("elimination_tree"))?;
        cache_check(
            elimination_tree.len() == num_nodes,
            format!("elimination_tree.len ({}) != num_nodes ({})", elimination_tree.len(), num_nodes),
        )?;
        for parent in elimination_tree.iter() {
            if let Some(parent) = parent.value() {
                cache_check(
                    (parent as usize) < num_nodes,
                    format!("elimination_tree parent {} >= num_nodes {}", parent, num_nodes),
                )?;
            }
        }

        let raw_ranks = Storage::<Rank>::mmap(loader.path().join("ranks"))?;
        cache_check(
            raw_ranks.len() == num_nodes,
            format!("ranks.len ({}) != num_nodes ({})", raw_ranks.len(), num_nodes),
        )?;
        let mut seen = vec![false; num_nodes];
        for &rank in raw_ranks.iter() {
            cache_check((rank as usize) < num_nodes, format!("ranks contains out-of-range value {}", rank))?;
            cache_check(!seen[rank as usize], format!("ranks contains duplicate value {}", rank))?;
            seen[rank as usize] = true;
        }
        let node_order = NodeOrder::from_ranks_storage(raw_ranks);

        for (node, parent) in elimination_tree.iter().enumerate() {
            if let Some(parent) = parent.value() {
                cache_check(
                    (parent as usize) > node,
                    format!("elimination_tree[{}] = {} — parent must have higher rank than child", node, parent),
                )?;
            }
        }
        let separator_tree = SeparatorTree::new(&elimination_tree);

        Ok(DirectedCCH {
            forward_first_out,
            forward_head,
            forward_tail,
            backward_first_out,
            backward_head,
            backward_tail,
            node_order,
            forward_cch_edge_to_orig_arc,
            backward_cch_edge_to_orig_arc,
            elimination_tree,
            forward_inverted,
            backward_inverted,
            separator_tree,
        })
    }
}

pub struct DirectedCCH {
    forward_first_out: Storage<EdgeId>,
    forward_head: Storage<NodeId>,
    forward_tail: Storage<NodeId>,
    backward_first_out: Storage<EdgeId>,
    backward_head: Storage<NodeId>,
    backward_tail: Storage<NodeId>,
    node_order: NodeOrder,
    forward_cch_edge_to_orig_arc: Vecs<EdgeIdT>,
    backward_cch_edge_to_orig_arc: Vecs<EdgeIdT>,
    elimination_tree: Storage<InRangeOption<NodeId>>,
    forward_inverted: ReversedGraphWithEdgeIds,
    backward_inverted: ReversedGraphWithEdgeIds,
    separator_tree: SeparatorTree,
}

impl DirectedCCH {
    fn num_nodes(&self) -> usize {
        self.forward_first_out.len() - 1
    }
}

impl CCHT for DirectedCCH {
    fn forward_first_out(&self) -> &[EdgeId] {
        self.forward_first_out.as_slice()
    }
    fn backward_first_out(&self) -> &[EdgeId] {
        self.backward_first_out.as_slice()
    }
    fn forward_head(&self) -> &[NodeId] {
        self.forward_head.as_slice()
    }
    fn backward_head(&self) -> &[NodeId] {
        self.backward_head.as_slice()
    }
    fn forward_tail(&self) -> &[NodeId] {
        self.forward_tail.as_slice()
    }
    fn backward_tail(&self) -> &[NodeId] {
        self.backward_tail.as_slice()
    }
    fn forward_inverted(&self) -> &ReversedGraphWithEdgeIds {
        &self.forward_inverted
    }
    fn backward_inverted(&self) -> &ReversedGraphWithEdgeIds {
        &self.backward_inverted
    }
    fn forward_cch_edge_to_orig_arc(&self) -> &Vecs<EdgeIdT> {
        &self.forward_cch_edge_to_orig_arc
    }
    fn backward_cch_edge_to_orig_arc(&self) -> &Vecs<EdgeIdT> {
        &self.backward_cch_edge_to_orig_arc
    }

    fn node_order(&self) -> &NodeOrder {
        &self.node_order
    }

    fn elimination_tree(&self) -> &[InRangeOption<NodeId>] {
        self.elimination_tree.as_slice()
    }

    fn separators(&self) -> &SeparatorTree {
        &self.separator_tree
    }
}
