//! R1590 — a selection grows by asking the graph a question.
//!
//! # Why this module exists
//!
//! The selection itself is **not here**, and that is deliberate: R1586 put it
//! outside the document because two people looking at one graph have two
//! selections and one document. What *is* here is the half that is a property
//! of the graph — *given these nodes, which others are the ones you mean?* the
//! DCC spells that as six operators; four of them are questions about the
//! graph and are this module, and the other two are questions about the
//! **canvas** (see the end of these docs).
//!
//! Every one of them is a **pure query**. The DCC's are edits: they set the
//! `SELECT` bit on `bNode` and carry `OPTYPE_UNDO`, because there the selection lives in the
//! document — so "what would this select?" cannot be asked without selecting
//! it, and every answer costs an undo step. Here the answer is a value ([`Grown`])
//! and the document is untouched, which is what makes the question previewable
//! (§2 #3).
//!
//! # The reach is a parameter, not a keystroke count
//!
//! The single largest difference. `NODE_OT_select_linked_to` walks `output_socket->directly_linked_sockets()` — **one hop** — and so does
//! `NODE_OT_select_linked_from`. The question a person actually has is *what depends on this?*, which
//! is the transitive closure, and the DCC answers it by having you press the
//! key until the picture stops changing, with nothing telling you when that
//! has happened. [`Reach::Transitive`] is that question asked once, and [`Grown::added`] is what tells
//! you it has been answered: growing again by the same transitive question
//! adds nothing, which is a property this module's tests assert and the DCC's
//! mutating form cannot state.
//!
//! # Two relations, and the reach means the same thing in both
//!
//! Links are one relation and R1589's containment is the other, so the four
//! relational questions are the two directions of each, and [`Reach`] reads the
//! same way in all four. They cannot collide: a [`NodeBody::Frame`] has no
//! ports, so no link ever reaches one, and containment only ever relates a frame
//! to its members.
//!
//! # What is NOT here, and where it belongs
//!
//! `NODE_OT_select_circle` and `NODE_OT_select_lasso` test a region against
//! `node->runtime->draw_bounds` — the **drawn** rectangle. R1589 already
//! recorded that a node's extent is the application's and not this crate's,
//! because a model crate has no card geometry; measured here, those two
//! operators are that same fact and not a node-graph capability at all. Their
//! home is the layer that already knows what was painted where — a region test
//! over tagged scene rectangles, which would serve a timeline, a chart brush and
//! a diagram editor as well as this. Deliberately not smuggled in behind a
//! `Fn(NodeId) -> Rect` argument, which would put the caller's own loop inside a
//! crate that cannot check it.

use std::collections::{BTreeSet, VecDeque};
use std::fmt;

use crate::model::{Document, Node, NodeBody, NodeId, NodeKind, TreeId};

/// How far a relational question travels.
///
/// The same word in all four relational arms of [`Grow`], because it is a
/// property of the *question* rather than of the relation being asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// One step. The DCC's only mode for the link relation, and what its
    /// `NODE_OT_select_linked_to` / `..._from` do per keypress.
    Direct,
    /// Every step, to the end. The question "what depends on this" asked once.
    Transitive,
}

/// The question a selection is grown by.
///
/// The four relational arms are the two directions of each of the graph's two
/// relations; the three predicate arms match a node against the selection
/// itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grow {
    /// Nodes the selection feeds. The DCC's `NODE_OT_select_linked_to`.
    ///
    /// A **muted** link is still followed: mutedness is about the value, and
    /// every structural derivation in this crate goes on seeing the wire
    /// (R1586). The DCC follows it too, so this is agreement rather than
    /// divergence — asserted, because it is the kind of agreement that is easy
    /// to break by accident.
    Downstream(Reach),
    /// Nodes that feed the selection. The DCC's `NODE_OT_select_linked_from`.
    Upstream(Reach),
    /// What the selected frames contain (R1589). A DCC analogue:
    /// selecting a frame there selects nothing inside it, though dragging one
    /// moves everything inside it.
    Contents(Reach),
    /// The frames containing the selection. The other direction of the same
    /// relation, which is the only reason it is here — see the module docs.
    Containers(Reach),
    /// Nodes that are the same kind as something selected. The DCC's
    /// `NODE_OT_select_grouped(TYPE)`.
    ///
    /// Keyed on the whole selection rather than on one **active** node, because
    /// this crate has no notion of an active node — a selection is the
    /// editor's, and "which of them is active" is the editor's too. With one
    /// node selected the two are the same question.
    SameKind,
    /// Nodes whose displayed name begins with the same prefix as something
    /// selected — the run up to the first `.`, `-` or `_`. The DCC's
    /// `NODE_OT_select_grouped(PREFIX)`.
    NamePrefix,
    /// Nodes whose displayed name ends with the same suffix as something
    /// selected — the run after the last `.`, `-` or `_`. The DCC's
    /// `NODE_OT_select_grouped(SUFFIX)`.
    NameSuffix,
}

/// What a growth produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grown {
    /// The whole selection afterwards, ascending and without repeats.
    pub selection: Vec<NodeId>,
    /// What this call added, ascending. Empty means the question had no answer
    /// the selection did not already hold — which is how a caller knows a
    /// [`Reach::Transitive`] walk has reached the end, and what the DCC's
    /// mutating form cannot report.
    pub added: Vec<NodeId>,
}

impl Grown {
    /// Whether the selection changed.
    #[must_use]
    pub fn changed(&self) -> bool {
        !self.added.is_empty()
    }
}

/// Why a selection question could not be answered.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SelectError {
    /// No such tree in this document.
    NoSuchTree(TreeId),
    /// The selection names a node that is not in that tree.
    ///
    /// Refused rather than skipped: a selection holding an id the tree does
    /// not have is a *stale* selection, and answering a question about it
    /// would quietly answer a different question. The DCC's operators skip.
    NoSuchNode {
        /// The tree that was searched.
        tree: TreeId,
        /// The node that is not in it.
        node: NodeId,
    },
}

impl fmt::Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode { tree, node } => {
                write!(f, "tree {} has no node {}", tree.0, node.0)
            }
        }
    }
}

impl std::error::Error for SelectError {}

/// The delimiters a displayed name is split on for [`Grow::NamePrefix`] and
/// [`Grow::NameSuffix`] — the DCC's own set.
const AFFIX_DELIMITERS: [char; 3] = ['.', '-', '_'];

/// Which end of a name an affix is taken from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Affix {
    Prefix,
    Suffix,
}

impl Affix {
    /// The affix of `name`, or `None` when it has no delimiter and so has no
    /// affix on this side.
    ///
    /// The DCC substitutes the WHOLE NAME for a missing suffix (`node_select_grouped_name`, the `from_right && !(sep && suf_act)`
    /// arm), which conflates "this node has no suffix" with "this node's
    /// suffix is its entire name" — so there, whether the operator groups
    /// anything at all depends on whether the node happened to be the first of
    /// its kind and thus escaped a `.001` disambiguator.
    fn of(self, name: &str) -> Option<&str> {
        match self {
            Self::Prefix => name.split_once(AFFIX_DELIMITERS).map(|(head, _)| head),
            Self::Suffix => name.rsplit_once(AFFIX_DELIMITERS).map(|(_, tail)| tail),
        }
    }
}

impl<K: NodeKind> Document<K> {
    /// Grow `selection` in `tree` by one question, answering the new selection
    /// and what it added.
    ///
    /// A **pure query**: nothing in the document changes. The DCC's
    /// equivalents set the `SELECT` bit and carry `OPTYPE_UNDO`.
    ///
    /// # Errors
    ///
    /// See [`SelectError`].
    pub fn grow(&self, tree: TreeId, selection: &[NodeId], by: Grow) -> Result<Grown, SelectError> {
        let host = self.tree(tree).ok_or(SelectError::NoSuchTree(tree))?;
        let mut held: BTreeSet<NodeId> = BTreeSet::new();
        for &id in selection {
            if host.node(id).is_none() {
                return Err(SelectError::NoSuchNode { tree, node: id });
            }
            held.insert(id);
        }

        let found = match by {
            Grow::Downstream(reach) => self.reachable(tree, &held, reach, Direction::Down),
            Grow::Upstream(reach) => self.reachable(tree, &held, reach, Direction::Up),
            Grow::Contents(reach) => self.through_frames(tree, &held, reach, Direction::Down),
            Grow::Containers(reach) => self.through_frames(tree, &held, reach, Direction::Up),
            Grow::SameKind => self.matching(tree, &held, Match::Kind),
            Grow::NamePrefix => self.matching(tree, &held, Match::Affix(Affix::Prefix)),
            Grow::NameSuffix => self.matching(tree, &held, Match::Affix(Affix::Suffix)),
        };

        let added: Vec<NodeId> = found.difference(&held).copied().collect();
        let selection: Vec<NodeId> = held.union(&found).copied().collect();
        Ok(Grown { selection, added })
    }

    /// Every node of the same kind as `like`, in **evaluation order**, `like`
    /// included.
    ///
    /// The substrate under the DCC's `NODE_OT_select_same_type_step`, which
    /// walks `toposort_left_to_right()` one position at a time. Publishing the
    /// run rather than the step is what lets a caller say *where* in it the
    /// cursor is — "3 of 7" — which that operator cannot, since it answers by
    /// changing the active node and reports only whether it moved.
    ///
    /// Ordered by dependency and then by id, so the answer is the same every
    /// time for one document. A node no link reaches keeps its place by id among
    /// its peers.
    ///
    /// `None` when the node is not there.
    #[must_use]
    pub fn same_kind_run(&self, tree: TreeId, like: NodeId) -> Option<Vec<NodeId>> {
        let host = self.tree(tree)?;
        let subject = host.node(like)?;
        let order = self.evaluation_order(tree);
        Some(
            order
                .into_iter()
                .filter(|&id| host.node(id).is_some_and(|node| same_kind(node, subject)))
                .collect(),
        )
    }

    /// Every node in `tree`, producers before consumers, ties broken by id.
    ///
    /// A tree this crate built is acyclic, so this is a topological order. One
    /// that arrived broken still gets every node exactly once — the nodes a
    /// cycle keeps from ever becoming ready are appended by id, so the answer
    /// stays a permutation rather than losing them.
    #[must_use]
    pub fn evaluation_order(&self, tree: TreeId) -> Vec<NodeId> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let mut waiting: std::collections::BTreeMap<NodeId, usize> =
            host.nodes().map(|n| (n.id, 0)).collect();
        for link in host.links() {
            if let Some(count) = waiting.get_mut(&link.to.node) {
                *count += 1;
            }
        }
        let mut ready: BTreeSet<NodeId> = waiting
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut order = Vec::with_capacity(waiting.len());
        while let Some(&next) = ready.iter().next() {
            ready.remove(&next);
            waiting.remove(&next);
            order.push(next);
            for link in host.links().iter().filter(|l| l.from.node == next) {
                if let Some(count) = waiting.get_mut(&link.to.node) {
                    *count -= 1;
                    if *count == 0 {
                        ready.insert(link.to.node);
                    }
                }
            }
        }
        // Whatever a cycle left unready, by id, so this is always a permutation.
        order.extend(waiting.into_keys());
        order
    }

    /// The nodes `seeds` reaches by following links in `direction`.
    fn reachable(
        &self,
        tree: TreeId,
        seeds: &BTreeSet<NodeId>,
        reach: Reach,
        direction: Direction,
    ) -> BTreeSet<NodeId> {
        let Some(host) = self.tree(tree) else {
            return BTreeSet::new();
        };
        let mut found = BTreeSet::new();
        let mut queue: VecDeque<NodeId> = seeds.iter().copied().collect();
        let mut walked: BTreeSet<NodeId> = seeds.clone();
        while let Some(current) = queue.pop_front() {
            for link in host.links() {
                let (near, far) = match direction {
                    Direction::Down => (link.from.node, link.to.node),
                    Direction::Up => (link.to.node, link.from.node),
                };
                if near != current || host.node(far).is_none() {
                    continue;
                }
                found.insert(far);
                // The seen-set is the only guard, so a malformed document with a
                // link cycle terminates here without a second bound to disagree
                // with it.
                if reach == Reach::Transitive && walked.insert(far) {
                    queue.push_back(far);
                }
            }
        }
        found
    }

    /// The nodes `seeds` reaches through R1589's containment.
    fn through_frames(
        &self,
        tree: TreeId,
        seeds: &BTreeSet<NodeId>,
        reach: Reach,
        direction: Direction,
    ) -> BTreeSet<NodeId> {
        let mut found = BTreeSet::new();
        for &seed in seeds {
            match (direction, reach) {
                (Direction::Down, Reach::Direct) => found.extend(self.members(tree, seed)),
                (Direction::Down, Reach::Transitive) => found.extend(self.contents(tree, seed)),
                (Direction::Up, Reach::Direct) => {
                    found.extend(self.ancestry(tree, seed).last().copied());
                }
                (Direction::Up, Reach::Transitive) => found.extend(self.ancestry(tree, seed)),
            }
        }
        found
    }

    /// The nodes matching the selection under a per-node predicate.
    fn matching(&self, tree: TreeId, seeds: &BTreeSet<NodeId>, by: Match) -> BTreeSet<NodeId> {
        let Some(host) = self.tree(tree) else {
            return BTreeSet::new();
        };
        let chosen: Vec<&Node<K>> = seeds.iter().filter_map(|&id| host.node(id)).collect();
        match by {
            Match::Kind => host
                .nodes()
                .filter(|node| chosen.iter().any(|subject| same_kind(node, subject)))
                .map(|node| node.id)
                .collect(),
            Match::Affix(side) => {
                // A selected node with no affix on this side offers no
                // criterion, so it contributes nothing rather than contributing
                // its whole name — see `Affix::of`.
                let wanted: BTreeSet<String> = chosen
                    .iter()
                    .filter_map(|node| side.of(&node.display_name()).map(ToOwned::to_owned))
                    .collect();
                host.nodes()
                    .filter(|node| {
                        side.of(&node.display_name())
                            .is_some_and(|affix| wanted.contains(affix))
                    })
                    .map(|node| node.id)
                    .collect()
            }
        }
    }
}

/// Which way a relation is followed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Down,
    Up,
}

/// Which per-node predicate a match uses.
#[derive(Clone, Copy)]
enum Match {
    Kind,
    Affix(Affix),
}

/// Whether two nodes are the same *kind* — what they do, never what they are
/// called or what they are set to.
///
/// [`NodeKind::name`]'s own contract: a stable identity token, never derived
/// from a label. The DCC compares `type_legacy`, which is the same idea.
///
/// **Two group instances are the same kind only when they instance the same
/// definition**, where the DCC gives every group node the one type `NODE_GROUP`. An
/// instance's signature *is* its definition's interface, so two instances of
/// different definitions are not alike in any respect this model can see, and
/// calling them one kind would grow a selection into nodes that have nothing
/// to do with it.
///
/// **Two delays are the same kind only when they hold the same type** (R1600),
/// which is the same argument one step down: a delay's whole signature is
/// derived from the type it holds, so two that hold different types have no
/// port in common.
fn same_kind<K: NodeKind>(a: &Node<K>, b: &Node<K>) -> bool {
    match (&a.body, &b.body) {
        (NodeBody::Kind(a), NodeBody::Kind(b)) => a.name() == b.name(),
        (NodeBody::Group(a), NodeBody::Group(b)) => a == b,
        (NodeBody::Interface(a), NodeBody::Interface(b)) => a == b,
        (NodeBody::Frame, NodeBody::Frame) => true,
        (NodeBody::Delay(a), NodeBody::Delay(b)) => a == b,
        _ => false,
    }
}
