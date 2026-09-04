//! ★★★★★ R1995 — **which nodes no output reaches**, and taking them out.
//!
//! A graph accumulates work that leads nowhere: a branch someone rewired past,
//! a card dropped and never joined up. The reference's material editor has one
//! menu entry for it, *Clean Unused Expressions*.
//!
//! # What the reference does, measured at its implementation rather than its
//! name
//!
//! `FMaterialEditor::CleanUnusedExpressions` asks `GetUnusedExpressions` for a
//! flat list and deletes it. That list is computed by a depth-first walk
//! **upstream from the graph's outputs** — the material's root node, or a
//! function's output nodes — following each node's **input** pins, skipping
//! exec pins, and taking `LinkedTo[0]` on each. A named reroute *usage* also
//! pushes its declaration, which is a dependency that is not a wire. Everything
//! the walk did not mark is unused.
//!
//! Before deleting it calls `CheckExpressionRemovalWarnings`, which builds a
//! string of the **function inputs and outputs** among the doomed and asks
//! yes/no — *any materials which use this function will lose their connections
//! to these once deleted*. Nothing is said about the rest of the list.
//!
//! # ★★★★★ Why the outputs are an ARGUMENT, and how that was found
//!
//! The reference can derive its answer because a material **has** a root node.
//! This crate's graphs do not: nothing in a [`NodeKind`] declares *the result
//! comes out of me*. The first draft here derived the outputs the way
//! [`Document::home`] derives an end — a step with something arriving and
//! nothing leaving — and the census fixture killed it immediately: **a dead
//! branch's own last node is exactly that shape, so every dead branch anchored
//! itself and the operation could never find one.** Deriving would have shipped
//! a verb that answers *nothing is unused* on the graphs it exists for.
//!
//! So the caller says what the graph is for. That is more general than the
//! reference, not less: a material editor would pass its root, and an editor
//! whose graph has several results passes them all. And the empty case is
//! refused rather than answered — see [`NotPrunable::Nothing`].
//!
//! # The five measured ways this passes it
//!
//! 1. ★★★★★ **Nothing to reach from is REFUSED, not answered.** With no root
//!    and no function outputs the reference's walk starts from an empty stack,
//!    marks nothing, and returns **every node in the graph** — and the command
//!    deletes them all. [`NotPrunable::Nothing`] says so instead, because
//!    *nobody said what this graph is for* and *everything here is rubbish* are
//!    different facts and only one of them is worth acting on.
//! 2. ★★★★★ **Asking is separable from doing.** `GetUnusedExpressions` is a
//!    helper on the graph; the editor exposes only the destructive command, so
//!    a person cannot ask *what would this take* except by taking it.
//!    [`Document::unused`] answers without touching the document.
//! 3. ★★★★★ **It says which of the doomed are STRUCTURAL** — the ones whose
//!    removal changes what this tree looks like from outside, so every instance
//!    of it loses a port. That is exactly what the reference's dialog is about,
//!    and here it is a field on each node rather than a sentence built for a
//!    message box, so a caller can act on it instead of reading it.
//! 4. ★★★★★ **A node may refuse to go, and the report says which did.** This
//!    crate has a per-node permission gate ([`Document::may`]); the reference
//!    deletes unconditionally. [`Pruned::kept`] carries each refusal with its
//!    reason.
//! 5. **Regions are not rubbish.** A frame has an empty signature, so no output
//!    can ever reach it — under the reference's own rule every frame on the
//!    canvas would be unused. [`Document::steps`] is what keeps them out, and
//!    it is the same derivation [`Document::home`] uses rather than a second
//!    spelling of it.
//!
//! # ⚠ One deliberate divergence, stated
//!
//! The reference skips **exec** pins when walking, because a material graph's
//! usage is a value dependency and its exec pins serve something else. This
//! crate follows every link, control included: a node that feeds another
//! through control is doing something, and deleting it would change what the
//! graph does. Following fewer edges can only ever delete MORE, which is the
//! dangerous direction.

use core::fmt;

use crate::model::{Document, EditError, Node, NodeBody, NodeId, NodeKind, TreeId};
use crate::select::{Grow, Reach};

/// A node no output reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Doomed {
    /// The node.
    pub node: NodeId,
    /// Whether taking it out changes what this tree looks like **from
    /// outside**.
    ///
    /// ★ An interface node is half of the tree's own signature, so removing one
    /// takes a port off every instance of this definition — a consequence that
    /// does not fit on this canvas. The reference warns about exactly this
    /// case, in a yes/no dialog naming its function inputs and outputs; here it
    /// is a fact per node, so a caller can refuse, confirm, or report it rather
    /// than having to parse a sentence.
    pub structural: bool,
}

/// What no output reaches, and what the reach was measured from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unused {
    /// Every node no output reaches, ascending. Empty is an answer: the graph
    /// is already clean.
    pub nodes: Vec<Doomed>,
    /// The outputs the walk started from, ascending — what the caller said the
    /// graph is for.
    ///
    /// ★ Published because it is the premise of the whole answer, and the
    /// reference's is invisible: a person told *these forty nodes are unused*
    /// has not been told *because only this one node counts as an output*,
    /// which is the sentence that would make them look again.
    pub from: Vec<NodeId>,
}

impl Unused {
    /// Whether there is nothing to take out.
    #[must_use]
    pub fn clean(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The doomed nodes whose removal is felt outside this tree, ascending.
    #[must_use]
    pub fn structural(&self) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|doomed| doomed.structural)
            .map(|doomed| doomed.node)
            .collect()
    }
}

/// What taking them out did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pruned {
    /// The nodes that went, ascending.
    pub gone: Vec<NodeId>,
    /// The nodes that were unused and **refused to go**, each with the reason
    /// the per-node gate gave. ★ The reference has no such gate and no such
    /// report.
    pub kept: Vec<(NodeId, EditError)>,
    /// How many wires went with them.
    pub links: usize,
    /// The answer this acted on, unchanged — so the record of the edit carries
    /// its own premise.
    pub unused: Unused,
}

/// Why a graph could not be asked what it is not using.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotPrunable {
    /// The tree is not in this document.
    NoSuchTree(TreeId),
    /// One of the named outputs is not in that tree.
    ///
    /// Refused rather than skipped, which is [`SelectError`](crate::SelectError)'s
    /// own rule and for its reason: a stale id quietly makes this a question
    /// about a smaller set of outputs, and a smaller set condemns more nodes.
    NoSuchNode { tree: TreeId, node: NodeId },
    /// No outputs were named, so nothing anchors the graph and the honest
    /// answer would be *every node here is unused*.
    ///
    /// ★★★★★ Refused rather than answered, because the reference computes
    /// exactly that answer and hands it to a command that empties the canvas. A
    /// graph nobody has said the purpose of is not a graph full of rubbish.
    Nothing,
}

impl fmt::Display for NotPrunable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode { tree, node } => {
                write!(f, "no node {} in tree {}", node.0, tree.0)
            }
            Self::Nothing => write!(f, "no output was named, so nothing anchors the graph"),
        }
    }
}

impl std::error::Error for NotPrunable {}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1995 — **which nodes none of these outputs reaches.**
    ///
    /// `outputs` is what the graph is FOR — the caller's answer, because
    /// nothing in this crate declares it. The walk runs upstream from them and
    /// everything it does not reach is unused. Asking changes nothing.
    ///
    /// # Errors
    ///
    /// [`NotPrunable`] — the tree is not there, an output is not in it, or no
    /// output was named at all.
    pub fn unused(&self, tree: TreeId, outputs: &[NodeId]) -> Result<Unused, NotPrunable> {
        if self.tree(tree).is_none() {
            return Err(NotPrunable::NoSuchTree(tree));
        }
        for &node in outputs {
            if self.tree(tree).and_then(|host| host.node(node)).is_none() {
                return Err(NotPrunable::NoSuchNode { tree, node });
            }
        }
        if outputs.is_empty() {
            return Err(NotPrunable::Nothing);
        }
        let steps = self.steps(tree);
        let mut from: Vec<NodeId> = outputs.to_vec();
        from.sort_unstable();
        from.dedup();
        // `grow` includes the seeds, so what comes back is *everything the
        // outputs depend on*, outputs included.
        // ⚠ The fallback is a stated limit rather than a case that happens: the
        // outputs were vetted against this tree a few lines up, which is every
        // refusal `grow` has. It answers the outputs alone — the smallest
        // truthful answer — rather than an empty set, which would condemn them.
        let used = self
            .grow(tree, &from, Grow::Upstream(Reach::Transitive))
            .map_or_else(|_| from.clone(), |grown| grown.selection);
        let nodes = steps
            .into_iter()
            .filter(|node| !used.contains(node))
            .map(|node| Doomed {
                node,
                structural: self
                    .tree(tree)
                    .and_then(|host| host.node(node))
                    .is_some_and(is_structural),
            })
            .collect();
        Ok(Unused { nodes, from })
    }

    /// ★★★★★ R1995 — **take out every node none of these outputs reaches**, and
    /// say what went and what would not.
    ///
    /// Exactly what [`unused`](Self::unused) answered is attempted, one node at
    /// a time through [`remove_node`](Self::remove_node) so each node's own
    /// permission gate is asked. A node that refuses stays, with its reason in
    /// [`Pruned::kept`].
    ///
    /// ★ The set does not have to be recomputed as it shrinks: a node reachable
    /// from an output is reachable along a path whose every node is also
    /// reachable, so the used set is closed and taking unused nodes out cannot
    /// make a used one unused.
    ///
    /// # Errors
    ///
    /// [`NotPrunable`] — exactly what [`unused`](Self::unused) answers.
    pub fn prune(&mut self, tree: TreeId, outputs: &[NodeId]) -> Result<Pruned, NotPrunable> {
        let unused = self.unused(tree, outputs)?;
        let mut gone = Vec::new();
        let mut kept = Vec::new();
        let mut links = 0usize;
        for doomed in &unused.nodes {
            match self.remove_node(tree, doomed.node) {
                Ok(removed) => {
                    links += removed.links.len();
                    gone.push(doomed.node);
                }
                Err(why) => kept.push((doomed.node, why)),
            }
        }
        Ok(Pruned {
            gone,
            kept,
            links,
            unused,
        })
    }
}

/// Whether removing this node changes what its tree looks like from outside.
///
/// One arm today, and a function rather than an inline `matches!` because the
/// question is *what is felt beyond this canvas* and the answer is a property
/// of a body — a second body that publishes a port would belong here, and a
/// caller comparing against a literal would not find out.
fn is_structural<K: NodeKind>(node: &Node<K>) -> bool {
    matches!(node.body, NodeBody::Interface(_))
}
