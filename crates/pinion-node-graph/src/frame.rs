//! R1589 — a node can belong to a frame, and belonging is a forest.
//!
//! # Why this module exists
//!
//! A node editor has two kinds of containment and they are not the same kind.
//! A **group** contains a graph: its members are in another tree, they compute,
//! and the boundary is a signature. A **frame** contains a *region of canvas*:
//! its members are right where they were, they compute exactly as before, and
//! the boundary means nothing to the evaluator at all. Groups arrived in R1577;
//! frames are the other half, and without them the largest thing a person
//! actually does to a big graph — put a fence round eight nodes and call it
//! "decode" — has nowhere to be recorded.
//!
//! The relation itself is one nullable field ([`Node::parent`]). What makes it
//! worth a module is that it is a **forest**, and a forest is a thing that can
//! be broken: a parent that is not a container, a parent that is not there, a
//! node that contains itself. Blender declares the same field and states both
//! of its rules as `BLI_assert` — `node_attach_node` asserts `parent.is_frame()`
//! and `!node_is_parent_and_child(parent, node)` — which are compiled out of
//! the build it ships. Worse, its own `NODE_OT_parent_set` **detaches before it
//! attaches**, so by the time that second assert runs the chain it would have
//! walked is already cleared: select a frame's own container along with it,
//! press <kbd>Ctrl</kbd>+<kbd>P</kbd>, and the two nodes contain each other in a
//! debug build too. Nothing in Blender then terminates —
//! `node_is_parent_and_child` and `get_sorted_node_parents` both walk `parent`
//! to `nullptr`.
//!
//! So here the two rules are checked, the refusal **names the chain**, and
//! [`Document::validate`] reports a forest that arrived broken from a file.
//!
//! # One derivation, not one per gesture
//!
//! Every gesture over the forest asks the same question: *which members of this
//! selection are not inside another member of it?* Framing attaches those;
//! unframing detaches those; a drag moves those. Blender writes it twice, as
//! `node_join_attach_recursive` and `node_detach_recursive` — two recursive
//! functions over two structs with identical fields (`NodeJoinState` and
//! `NodeDetachstate`, both `{bool done; bool descendent;}`) — and a third time
//! inside the transform code. Here it is [`Document::outermost`], and the
//! gestures are its call sites.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{Document, EditError, Node, NodeBody, NodeId, NodeKind, TreeId, centroid};

/// What a successful [`Document::enframe`] built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enframed {
    /// The frame node, in the tree the selection was in.
    pub frame: NodeId,
    /// The selected nodes now directly inside it, ascending. A selected node
    /// already inside another selected node is **not** here — it stays where it
    /// was, and the frame contains it through its own container.
    pub members: Vec<NodeId>,
}

/// A node that was taken out of a frame it is no longer with.
///
/// Reported wherever a selection travels without the frame that contained it:
/// out of a tree as a [`Fragment`](crate::Fragment), into a group definition, or
/// across a group boundary. Blender's copy path detaches in exactly these cases
/// (`node_copy_local` looks the parent up in the copy map and calls
/// `node_detach_node` when it is not there) and records nothing, so a user who
/// duplicates a node that was in a frame gets one that is not, with no
/// indication that anything happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Orphaned {
    /// The node, in the numbering of wherever it now is.
    pub node: NodeId,
    /// The frame it was in, in the numbering of the tree it came from.
    pub frame: NodeId,
}

impl fmt::Display for Orphaned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "node {} left frame {}", self.node.0, self.frame.0)
    }
}

/// Why a containment could not be established.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParentError {
    /// No such tree in this document.
    NoSuchTree(TreeId),
    /// No such node in that tree — either end.
    NoSuchNode {
        /// The tree that was searched.
        tree: TreeId,
        /// The node that is not in it.
        node: NodeId,
    },
    /// The proposed container is not a [`NodeBody::Frame`], so it is not a thing
    /// that contains. Blender states this as an assertion that ships disabled.
    NotAFrame {
        /// The node that was offered as a container.
        node: NodeId,
    },
    /// A node cannot be inside itself.
    SelfParent(NodeId),
    /// The containment would close a cycle; the chain runs from the node that
    /// would end up inside its own descendant to the frame it was offered to.
    Cycle {
        /// That chain, outermost first.
        chain: Vec<NodeId>,
    },
    /// Nothing was selected. A frame with no members is
    /// [`Document::add_node`] with [`NodeBody::Frame`]; this operation is
    /// "put *these* in a frame", so it has nothing to do.
    Empty,
}

impl fmt::Display for ParentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode { tree, node } => {
                write!(f, "tree {} has no node {}", tree.0, node.0)
            }
            Self::NotAFrame { node } => {
                write!(
                    f,
                    "node {} is not a frame, so nothing can be inside it",
                    node.0
                )
            }
            Self::SelfParent(node) => write!(f, "node {} cannot be inside itself", node.0),
            Self::Cycle { chain } => {
                f.write_str("that would put a frame inside itself: ")?;
                for (i, node) in chain.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" contains ")?;
                    }
                    write!(f, "{}", node.0)?;
                }
                f.write_str(", which would then contain the first")
            }
            Self::Empty => f.write_str("nothing is selected"),
        }
    }
}

impl std::error::Error for ParentError {}

impl<K: NodeKind> Document<K> {
    /// Put `node` inside `parent`, or take it out of everything (`None`),
    /// answering where it was.
    ///
    /// The one mutator of the forest. Blender's `NODE_OT_parent_set` and the
    /// model half of its `NODE_OT_attach`.
    ///
    /// # Errors
    ///
    /// See [`ParentError`]. A refusal changes nothing, and a cycle names the
    /// chain it would have closed rather than reporting a bare `false`.
    pub fn set_parent(
        &mut self,
        tree: TreeId,
        node: NodeId,
        parent: Option<NodeId>,
    ) -> Result<Option<NodeId>, ParentError> {
        let host = self.tree(tree).ok_or(ParentError::NoSuchTree(tree))?;
        if host.node(node).is_none() {
            return Err(ParentError::NoSuchNode { tree, node });
        }
        if let Some(frame) = parent {
            let container = host
                .node(frame)
                .ok_or(ParentError::NoSuchNode { tree, node: frame })?;
            if !container.is_frame() {
                return Err(ParentError::NotAFrame { node: frame });
            }
            if frame == node {
                return Err(ParentError::SelfParent(node));
            }
            // The chain from `node` down to `frame`, which putting `node` inside
            // `frame` would close. Read from the FRAME's ancestry, because that
            // is the half the new edge has to pass through.
            let ancestry = self.ancestry(tree, frame);
            if let Some(at) = ancestry.iter().position(|&a| a == node) {
                let mut chain = ancestry[at..].to_vec();
                chain.push(frame);
                return Err(ParentError::Cycle { chain });
            }
        }
        let slot = self
            .tree_mut(tree)
            .and_then(|t| t.node_mut(node))
            .ok_or(ParentError::NoSuchNode { tree, node })?;
        Ok(std::mem::replace(&mut slot.parent, parent))
    }

    /// The frames containing `node`, **outermost first**, excluding `node`.
    ///
    /// Outermost first because that is the order a breadcrumb reads and the
    /// order every chain this crate reports already uses
    /// ([`InsertError::Recursion`](crate::InsertError::Recursion)).
    ///
    /// Terminates on a document whose forest is broken — a cycle stops at the
    /// first repeat — because this is also what [`Self::validate`] uses to find
    /// one. There is deliberately no second depth cap beside that: two guards
    /// for one condition means neither is exercised alone.
    #[must_use]
    pub fn ancestry(&self, tree: TreeId, node: NodeId) -> Vec<NodeId> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let mut chain = Vec::new();
        let mut seen = BTreeSet::from([node]);
        let mut cursor = host.node(node).and_then(|n| n.parent);
        while let Some(current) = cursor {
            if !seen.insert(current) {
                break;
            }
            chain.push(current);
            cursor = host.node(current).and_then(|n| n.parent);
        }
        chain.reverse();
        chain
    }

    /// The nodes `frame` directly contains, ascending.
    #[must_use]
    pub fn members(&self, tree: TreeId, frame: NodeId) -> Vec<NodeId> {
        self.tree(tree).map_or_else(Vec::new, |host| {
            host.nodes()
                .filter(|n| n.parent == Some(frame))
                .map(|n| n.id)
                .collect()
        })
    }

    /// Every node `frame` contains, however deeply, ascending.
    ///
    /// This is what moves when the frame moves, and what a frame's own extent is
    /// derived from.
    #[must_use]
    pub fn contents(&self, tree: TreeId, frame: NodeId) -> Vec<NodeId> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let mut direct: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for node in host.nodes() {
            if let Some(parent) = node.parent {
                direct.entry(parent).or_default().push(node.id);
            }
        }
        let mut found: BTreeSet<NodeId> = BTreeSet::new();
        let mut stack = vec![frame];
        while let Some(current) = stack.pop() {
            for &child in direct.get(&current).map_or(&[][..], Vec::as_slice) {
                // A broken forest must terminate here too: a node already found
                // is not descended into a second time.
                if child != frame && found.insert(child) {
                    stack.push(child);
                }
            }
        }
        found.into_iter().collect()
    }

    /// The innermost frame containing every one of `nodes`, or `None` when they
    /// share only the canvas.
    ///
    /// Blender's `find_common_parent_node`, and the same derivation: the longest
    /// common **prefix** of the ancestries, which is what a lowest common
    /// ancestor is in a forest.
    ///
    /// A frame in `nodes` is judged by what contains *it*, not by itself — so
    /// asking about a frame and something inside it answers with the frame's own
    /// container, which is the only answer that is true of both.
    #[must_use]
    pub fn common_frame(&self, tree: TreeId, nodes: &[NodeId]) -> Option<NodeId> {
        let (first, rest) = nodes.split_first()?;
        let mut common = self.ancestry(tree, *first);
        for &node in rest {
            let theirs = self.ancestry(tree, node);
            let shared = common
                .iter()
                .zip(&theirs)
                .take_while(|(ours, theirs)| ours == theirs)
                .count();
            common.truncate(shared);
            if common.is_empty() {
                return None;
            }
        }
        common.last().copied()
    }

    /// Those of `selection` that no other member of `selection` contains,
    /// ascending.
    ///
    /// **The derivation every gesture over the forest is made of.** Attaching a
    /// selection to a new frame attaches these (the rest keep the containers
    /// they already have, which now sit inside the new frame); detaching a
    /// selection detaches these; dragging a selection moves these, and their
    /// contents come along because containment is what moving means.
    #[must_use]
    pub fn outermost(&self, tree: TreeId, selection: &[NodeId]) -> Vec<NodeId> {
        let chosen: BTreeSet<NodeId> = selection.iter().copied().collect();
        chosen
            .iter()
            .filter(|&&node| !self.ancestry(tree, node).iter().any(|a| chosen.contains(a)))
            .copied()
            .collect()
    }

    /// Put `selection` in a new frame and answer it.
    ///
    /// Blender's `NODE_OT_join`. The frame lands at the selection's centre, and
    /// **inside whatever already contained all of it** ([`Self::common_frame`]),
    /// so framing part of a pipeline does not lift it out of the pipeline.
    ///
    /// # Errors
    ///
    /// [`ParentError::Empty`] for an empty selection — an empty frame is
    /// [`Document::add_node`] with [`NodeBody::Frame`] and needs no derivation —
    /// or [`ParentError::NoSuchTree`] / [`ParentError::NoSuchNode`].
    pub fn enframe(
        &mut self,
        tree: TreeId,
        selection: &[NodeId],
        label: Option<String>,
    ) -> Result<Enframed, ParentError> {
        let host = self.tree(tree).ok_or(ParentError::NoSuchTree(tree))?;
        if selection.is_empty() {
            return Err(ParentError::Empty);
        }
        let mut positions = Vec::with_capacity(selection.len());
        for &id in selection {
            let node = host
                .node(id)
                .ok_or(ParentError::NoSuchNode { tree, node: id })?;
            positions.push((node.x, node.y));
        }
        let (x, y) = centroid(positions.into_iter());
        let container = self.common_frame(tree, selection);
        let members = self.outermost(tree, selection);

        let frame = self
            .add_node(tree, NodeBody::Frame, x, y)
            .map_err(|_| ParentError::NoSuchTree(tree))?;
        if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(frame)) {
            slot.label = label;
            slot.parent = container;
        }
        for &member in &members {
            if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(member)) {
                slot.parent = Some(frame);
            }
        }
        Ok(Enframed { frame, members })
    }

    /// Take `selection` out of the frames immediately containing it, answering
    /// the nodes that moved.
    ///
    /// **One level**, so a node in `Outer > Inner` lands in `Outer`. That is what
    /// "out of its frame" means, it composes (repeat to leave the next one), and
    /// the all-the-way form is [`Self::set_parent`] with `None`. Blender's
    /// `NODE_OT_detach` clears the parent outright, so only the second of those
    /// two behaviours is reachable there.
    ///
    /// Acts on [`Self::outermost`]: a selected node inside another selected node
    /// keeps its container, because that container is itself moving and taking
    /// it along.
    ///
    /// # Errors
    ///
    /// [`ParentError::NoSuchTree`] or [`ParentError::NoSuchNode`].
    pub fn unframe(
        &mut self,
        tree: TreeId,
        selection: &[NodeId],
    ) -> Result<Vec<NodeId>, ParentError> {
        let host = self.tree(tree).ok_or(ParentError::NoSuchTree(tree))?;
        for &id in selection {
            if host.node(id).is_none() {
                return Err(ParentError::NoSuchNode { tree, node: id });
            }
        }
        let mut moved = Vec::new();
        for node in self.outermost(tree, selection) {
            let grandparent = self
                .tree(tree)
                .and_then(|t| t.node(node))
                .and_then(|n| n.parent)
                .and_then(|parent| self.tree(tree).and_then(|t| t.node(parent)))
                .and_then(|parent| parent.parent);
            let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(node)) else {
                continue;
            };
            if slot.parent.is_none() {
                continue;
            }
            slot.parent = grandparent;
            moved.push(node);
        }
        Ok(moved)
    }

    /// Move `node` and everything it contains by `(dx, dy)`, answering every
    /// node that moved, `node` first.
    ///
    /// This is what the relation is *for*: a frame that did not carry its
    /// members when dragged would be a rectangle drawn behind them. Blender
    /// reaches the same behaviour from its transform system rather than from its
    /// node model, which is why `space_node` has a third copy of the
    /// selection-roots walk.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchNode`].
    pub fn translate(
        &mut self,
        tree: TreeId,
        node: NodeId,
        dx: i32,
        dy: i32,
    ) -> Result<Vec<NodeId>, EditError> {
        if self.tree(tree).is_none() {
            return Err(EditError::NoSuchTree(tree));
        }
        if self.tree(tree).and_then(|t| t.node(node)).is_none() {
            return Err(EditError::NoSuchNode { tree, node });
        }
        let mut moving = vec![node];
        moving.extend(self.contents(tree, node));
        for &id in &moving {
            if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(id)) {
                slot.x = slot.x.saturating_add(dx);
                slot.y = slot.y.saturating_add(dy);
            }
        }
        Ok(moving)
    }

    /// Re-point the `parent` of every node in `moved` into the destination's
    /// numbering, answering the ones whose frame did not come along.
    ///
    /// The one place the forest is reconciled after a set of nodes changes
    /// numbering, so a paste, an inline, a collapse and a boundary move cannot
    /// disagree about what happens to a container that stayed behind.
    ///
    /// `roots` is what a node whose parent was left behind becomes a member of
    /// instead: the container of whatever the selection was replaced by. That is
    /// how an inline lands inside the frame its instance was in.
    pub(crate) fn remap_parents(
        &mut self,
        tree: TreeId,
        moved: &BTreeMap<NodeId, NodeId>,
        source_parent: &BTreeMap<NodeId, Option<NodeId>>,
        roots: Option<NodeId>,
    ) -> Vec<Orphaned> {
        let mut orphaned = Vec::new();
        for (&old, &fresh) in moved {
            let was = source_parent.get(&old).copied().flatten();
            let now = match was {
                None => roots,
                Some(frame) if moved.contains_key(&frame) => moved.get(&frame).copied(),
                Some(frame) => {
                    orphaned.push(Orphaned { node: fresh, frame });
                    roots
                }
            };
            if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(fresh)) {
                slot.parent = now;
            }
        }
        orphaned.sort_unstable_by_key(|o| o.node);
        orphaned
    }
}

/// The `(node -> its parent)` map of a set of nodes, read before they move.
pub(crate) fn parents_of<'a, K: NodeKind + 'a>(
    nodes: impl Iterator<Item = &'a Node<K>>,
) -> BTreeMap<NodeId, Option<NodeId>> {
    nodes.map(|n| (n.id, n.parent)).collect()
}
