//! ★★★★★ R1932 — **what a kind requires of its own name**: where it has to be
//! unique, or that it need not be.
//!
//! # What the reference does, measured at its header, its consumers and its
//! fourteen overriders
//!
//! Its graph node publishes *make me a name validator*, supplied `NULL`, and its
//! schema publishes a second call of the same shape that is **not the same
//! mechanism**: the node's takes no arguments and answers for THAT node, the
//! schema's takes four (a blueprint, the original name, a validation scope and
//! an action type) and its one implementation is not overridden anywhere — its
//! consumers are the palette and the details panel, naming a blueprint's
//! variables and actions rather than a graph's nodes. Two names for two
//! capabilities on two subjects, and only the first is about a node.
//!
//! Reading all fourteen overriders of the first is what shaped this module, and
//! they do exactly two things:
//!
//! 1. ★★★★★ **Four of them SUPPRESS.** A comment and both reroute classes answer
//!    a dummy validator that says `Ok` to everything, carrying the same
//!    copy-pasted comment — *comments can be duplicated, etc...* That is the
//!    commonest single use, and it is the same shape R1928 measured on the pin
//!    naming hook: the capability's ordinary job is to take a rule AWAY.
//! 2. **The rest choose a SCOPE.** A composite, a timeline, a custom event, a
//!    function entry, a state machine and a cached pose each build a validator
//!    over the whole **blueprint** — not over the graph the node sits in — so
//!    what the override actually settles is *how far this name has to reach to
//!    be unique*.
//!
//! ⇒ so the axis is a scope with an off position, which is what [`Naming`] is.
//!
//! # ⚠ And the census's covering sentence was false
//!
//! It read *no name-validation surface: a label is free text*. A label has not
//! been free text since R1682:
//! [`Document::may`](Document::may)`(Act::Rename)` already refuses an empty
//! name ([`LabelEmpty`](crate::EditError::LabelEmpty)) and a name another node
//! in the tree holds ([`LabelTaken`](crate::EditError::LabelTaken), which NAMES
//! that node — the reference's
//! `AlreadyInUse` is a bare enum constant and cannot). Three of the reference's
//! seven validator results were already here.
//!
//! What was absent is that the rule was the CRATE's alone: an application could
//! neither widen the scope nor turn it off, and a frame — this crate's comment —
//! was held to the same uniqueness as a node the graph is addressed by.
//!
//! # The three measured ways this passes it
//!
//! 1. **The off position is a value, not an object you have to remember to
//!    build.** Two classes there hand back a dummy validator with an identical
//!    comment; here a kind answers [`Naming::Free`] and a frame *is* free
//!    without anybody writing anything.
//! 2. **The scope is a type rather than a constructor argument.** There the
//!    reach is whatever object was passed to the validator, so two classes
//!    wanting the same reach state it twice and can disagree; here there is one
//!    enum and [`Document::may`] reads it.
//! 3. **A refusal still names the holder.** Widening the scope did not cost the
//!    thing this crate has and the reference does not — `LabelTaken` says which
//!    node answers to the name, so a person is told what to rename rather than
//!    that they may not.

use crate::model::{Document, NodeBody, NodeId, NodeKind, TreeId};

/// What a kind requires of the name a person gives one of its nodes.
///
/// Three arms, measured off the reference's fourteen overriders rather than
/// invented: they either turn the rule off or choose how far the name has to
/// reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Naming {
    /// Unique among the authored names in the node's own tree. The supplied
    /// answer, and what this crate has enforced since R1682.
    #[default]
    InTree,
    /// Unique among the authored names in the whole document — every tree,
    /// including definitions this node is not in.
    ///
    /// The reference's commonest positive answer: six of its overriders build a
    /// validator over the whole blueprint rather than over the graph the node
    /// sits in.
    InDocument,
    /// Nothing is required. Two nodes of this kind may answer to one name.
    ///
    /// ⚠ A name that does not identify cannot be looked up, and
    /// [`Document::node_labelled`] says so by answering `None` when more than
    /// one node holds it. That is the trade this arm makes, and a kind takes it
    /// deliberately — the reference's comment and reroute classes do, because
    /// their name is a caption rather than an address.
    Free,
}

impl Naming {
    /// Whether a name of this kind has to identify one node.
    #[must_use]
    pub const fn is_unique(self) -> bool {
        !matches!(self, Self::Free)
    }
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1932 — **what `node` requires of its own name.**
    ///
    /// A kind answers for itself. Every other body answers what the crate knows
    /// about it, and one of those is not the default: a **frame** is
    /// [`Naming::Free`], for the reference's own reason — a frame takes no part
    /// in the graph, nothing is addressed by its caption, and holding two
    /// frames apart by name is a rule with nothing behind it. There the same
    /// decision is a dummy validator each commenting class has to remember to
    /// build.
    #[must_use]
    pub fn naming(&self, tree: TreeId, node: NodeId) -> Naming {
        match self.tree(tree).and_then(|host| host.node(node)) {
            Some(held) => match &held.body {
                NodeBody::Kind(_) => K::naming(),
                // A caption, not an address.
                NodeBody::Frame => Naming::Free,
                // An interface end, a group instance and a delay are all
                // addressed by name somewhere — the tree they belong to is the
                // scope, which is the supplied answer.
                NodeBody::Group(_) | NodeBody::Interface(_) | NodeBody::Delay(_) => Naming::InTree,
            },
            None => Naming::InTree,
        }
    }

    /// Every node in the WHOLE document that has authored the name `label`.
    ///
    /// The document-wide peer of
    /// [`nodes_labelled`](Document::nodes_labelled), which searches one tree.
    /// Needed because [`Naming::InDocument`] is a real answer and a scope that
    /// could not be searched would be a scope nothing enforced.
    #[must_use]
    pub fn nodes_labelled_anywhere(&self, label: &str) -> Vec<(TreeId, NodeId)> {
        (0..self.tree_count())
            .map(|index| TreeId(u32::try_from(index).unwrap_or(u32::MAX)))
            .flat_map(|tree| {
                self.nodes_labelled(tree, label)
                    .into_iter()
                    .map(move |node| (tree, node))
            })
            .collect()
    }
}
