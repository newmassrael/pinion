//! R1584 — the one place a tree's nodes are mapped onto `0..n`.
//!
//! Every algorithm [`pinion_graph`] offers states its graph as an `order` plus
//! links over `0..order`, and every operation here that reaches for one — the
//! collapse (R1577), the cut (R1578), the two boundary moves (R1584) — needs
//! the same translation out and the same translation back. It had been written
//! out twice before this module existed and was about to be written out twice
//! more.
//!
//! Lifting it found a defect the two copies shared: both indexed the node map
//! directly, so a document holding a link that names a missing node — which
//! [`Document::validate`](crate::Document::validate) exists to report, and which
//! nothing stops a deserialized document from holding — **panicked** instead of
//! being refused. The translation now fails, and its callers name the link.

use pinion_graph::group as boundary;
use std::collections::BTreeMap;

use crate::model::{LinkId, NodeId, NodeKind, Socket, Tree};

/// One tree's nodes and links, renumbered for [`pinion_graph`].
///
/// The link order is preserved exactly, so an index answered by a
/// [`boundary::Boundary`] indexes the tree's own link slice.
pub(crate) struct Numbering {
    node_of: Vec<NodeId>,
    vertex_of: BTreeMap<NodeId, usize>,
    links: Vec<boundary::Link>,
}

impl Numbering {
    /// Number `tree`'s nodes in ascending id order.
    ///
    /// # Errors
    ///
    /// The id of the first link naming a node the tree does not hold. Such a
    /// tree has no graph to speak of, so there is nothing to derive from it.
    pub(crate) fn of<K: NodeKind>(tree: &Tree<K>) -> Result<Self, LinkId> {
        let node_of: Vec<NodeId> = tree.nodes().map(|n| n.id).collect();
        let vertex_of: BTreeMap<NodeId, usize> =
            node_of.iter().enumerate().map(|(i, &n)| (n, i)).collect();
        let mut links = Vec::with_capacity(tree.links().len());
        for link in tree.links() {
            let (Some(&from), Some(&to)) =
                (vertex_of.get(&link.from.node), vertex_of.get(&link.to.node))
            else {
                return Err(link.id);
            };
            links.push(boundary::Link::new(
                boundary::Socket::new(from, link.from.port),
                boundary::Socket::new(to, link.to.port),
            ));
        }
        Ok(Self {
            node_of,
            vertex_of,
            links,
        })
    }

    /// How many vertices the graph has.
    pub(crate) fn order(&self) -> usize {
        self.node_of.len()
    }

    /// The links, in the tree's own order.
    pub(crate) fn links(&self) -> &[boundary::Link] {
        &self.links
    }

    /// `nodes` as vertices, or `None` when one of them is not in the tree.
    pub(crate) fn vertices(&self, nodes: &[NodeId]) -> Option<Vec<usize>> {
        nodes
            .iter()
            .map(|n| self.vertex_of.get(n).copied())
            .collect()
    }

    /// The node a vertex stands for.
    pub(crate) fn node(&self, vertex: usize) -> NodeId {
        self.node_of
            .get(vertex)
            .copied()
            .unwrap_or(NodeId(u32::MAX))
    }

    /// A vertex path back in the caller's own ids.
    pub(crate) fn path(&self, vertices: Vec<usize>) -> Vec<NodeId> {
        vertices.into_iter().map(|v| self.node(v)).collect()
    }

    /// A boundary socket back in the caller's own ids.
    pub(crate) fn socket(&self, socket: boundary::Socket) -> Socket {
        Socket::new(self.node(socket.vertex), socket.port)
    }
}
