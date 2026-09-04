//! ★★★★★ R1994 — **where a graph ends up**, so a canvas can be sent there.
//!
//! A person who has panned away from everything wants one press that puts them
//! back at the thing the graph is *for*. That is not the same question as
//! framing the whole graph ([`Fit::run`](crate::Fit)) or framing what they
//! chose ([`Fit::selection`](crate::Fit)): it asks the **graph** where its face
//! is, and the answer is a node.
//!
//! # What the reference does, measured at its implementation rather than its
//! name
//!
//! Its material editor binds a *Home* button to `RecenterEditor`:
//!
//! * For an ordinary material graph, home is the graph's designated **root
//!   node** — the one the result comes out of.
//! * For a material *function*, it walks the expressions backwards looking for
//!   function-output nodes, prefers the one flagged as last previewed, and
//!   otherwise takes the first it found.
//! * With a node in hand it jumps the view to it.
//! * ⚠ **With no node in hand it sets the view location to the world origin**,
//!   keeping the current zoom.
//! * It returns `void`.
//!
//! # The four measured ways this passes it
//!
//! 1. ★★★★★ **It says where home IS.** `RecenterEditor` returns nothing, so
//!    *where would Home take me* can be asked only by pressing it and looking.
//!    [`Document::home`] answers a node without touching any camera.
//! 2. ★★★★★ **It refuses, by name, when there is no home.** The reference's
//!    fallback is the world origin — a place with nothing at it, which reaches
//!    a person as a button that scrolled them into empty space. [`NoHome`]
//!    separates *this tree is not here*, *it has no nodes at all* and *every
//!    node feeds another, so nothing is an end*.
//! 3. ★★★★★ **It does not hide the ambiguity.** A graph can end in more than
//!    one place. The reference picks one by iteration order plus a preview
//!    flag and says nothing about the others; [`Home::ends`] is all of them.
//! 4. **It keeps the distinction its own list would otherwise destroy.** A node
//!    with nothing arriving *and* nothing leaving is an end only in the trivial
//!    sense — a card someone dropped on the canvas. [`End::fed`] says which,
//!    so a caller does not have to recompute the rule to tell a stray card from
//!    the place the graph actually ends up.
//!
//! # ⚠ Why the answer is derived and not declared
//!
//! The reference's material graph *owns* a root node object, so it never has to
//! ask. This crate's [`NodeKind`] is a consumer's own type and nothing in it
//! declares *I am this graph's face*. Rather than invent a declaration no
//! consumer asked for, home is read off the shape the graph already has — a
//! node no link leaves — which needs nothing added to any taxonomy and is
//! exactly right for the reference's own case, where the material root node is
//! the one nothing leaves.
//!
//! ⚠ With one refinement that the assembled screen forced rather than the
//! design anticipating it: only a node that **could be linked at all** counts.
//! A [`NodeBody::Frame`](crate::NodeBody)'s signature is empty by construction,
//! so no link can ever leave one and every frame qualified as an end forever —
//! measured on screen A, where two host frames were reported alongside the
//! three real ends. The filter asks the signature rather than naming the body,
//! so a body added later with the same emptiness is covered on the same commit.

use core::fmt;

use crate::model::{Document, NodeId, NodeKind, TreeId};

/// One of the places a graph ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct End {
    /// The node no link leaves.
    pub node: NodeId,
    /// Whether anything arrives at it.
    ///
    /// ★ `false` is a node with nothing on either side — a card dropped on the
    /// canvas and not yet wired. It is an end in the trivial sense and is
    /// reported as one, because leaving it out would make [`Home::ends`] a
    /// filtered list whose rule a caller could not see; saying so instead lets
    /// a screen tell *the graph ends here* from *nothing is wired to this*.
    pub fed: bool,
}

/// Where a graph ends up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    /// The one this crate would send a canvas to.
    ///
    /// The first **fed** end, or — when nothing the graph draws is wired at
    /// all — the first end outright. Ascending by [`NodeId`] either way, which
    /// is this crate's tiebreak everywhere else. Stated rather than left
    /// implicit because the reference's equivalent choice is iteration order
    /// over an array, which is a different answer on a day the array is built
    /// differently.
    pub at: NodeId,
    /// Every end, ascending. One entry means [`at`](Self::at) was not a choice.
    pub ends: Vec<End>,
}

impl Home {
    /// Whether the graph ends in exactly one place, so `at` is the answer
    /// rather than *an* answer.
    #[must_use]
    pub fn sole(&self) -> bool {
        self.ends.len() == 1
    }

    /// The ends that are not the one this would go to, ascending — what a
    /// screen offering *the other ends* would list.
    #[must_use]
    pub fn others(&self) -> Vec<NodeId> {
        self.ends
            .iter()
            .map(|end| end.node)
            .filter(|node| *node != self.at)
            .collect()
    }
}

/// Why a graph has nowhere to call home.
///
/// ★ Three cases the reference answers with one silent jump to the world
/// origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoHome {
    /// The tree is not in this document.
    NoSuchTree(TreeId),
    /// Nothing here is a step in a graph: either the tree holds no nodes at
    /// all, or everything in it is a region rather than something a link could
    /// reach — a canvas of bare frames.
    Empty,
    /// Every node feeds another, so nothing is an end.
    ///
    /// ⚠ Reachable, and only because this crate lets a cycle close through a
    /// delay: a graph that is all cycle has no terminus at all. The count is
    /// reported because *nothing is an end* is a very different fact about two
    /// nodes than about two hundred.
    Endless { nodes: usize },
}

impl fmt::Display for NoHome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::Empty => write!(f, "nothing here is a step in a graph"),
            Self::Endless { nodes } => write!(
                f,
                "every one of the {nodes} node(s) feeds another, so the graph has no end"
            ),
        }
    }
}

impl std::error::Error for NoHome {}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1994 — the nodes of a tree that are **steps in a graph**, rather
    /// than regions on a canvas, ascending.
    ///
    /// A [`NodeBody::Frame`](crate::NodeBody)'s signature is empty by
    /// construction, so no link can ever reach or leave one. Every question of
    /// the form *what is this graph's shape* has to leave those out, and asking
    /// the SIGNATURE rather than naming the body means a body added later with
    /// the same emptiness is covered without another edit.
    ///
    /// ★ R1995 — lifted to `pub(crate)` when [`Document::unused`] became the
    /// second caller. It was found by the assembled screen once already
    /// (frames answered as ends); a second spelling of it would have been a
    /// second chance to make the same mistake, and there the cost would have
    /// been *deleting the host frames*.
    pub(crate) fn steps(&self, tree: TreeId) -> Vec<NodeId> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let mut steps: Vec<NodeId> = host
            .nodes()
            .map(|node| node.id)
            .filter(|node| {
                self.signature(tree, *node)
                    .is_some_and(|face| !face.inputs.is_empty() || !face.outputs.is_empty())
            })
            .collect();
        steps.sort_unstable();
        steps
    }

    /// ★★★★★ R1994 — **where this graph ends up**, asked of the graph rather
    /// than of a camera.
    ///
    /// An end is a node no link leaves. Every one is reported, each saying
    /// whether anything arrives at it, and [`Home::at`] is the one this crate
    /// would point a canvas at.
    ///
    /// Nothing about a viewport is decided here: a caller pans to
    /// [`Home::at`]'s box with [`Camera::reveal`](crate::Camera::reveal), which
    /// keeps the zoom — the same thing the reference does once it has a node.
    /// What the reference cannot do is answer this without moving.
    ///
    /// # Errors
    ///
    /// [`NoHome`] — the tree is not there, nothing in it is a step, or every
    /// step feeds another.
    pub fn home(&self, tree: TreeId) -> Result<Home, NoHome> {
        let host = self.tree(tree).ok_or(NoHome::NoSuchTree(tree))?;
        // ★★★★★ A node that CANNOT be linked is not a place the graph ends — it
        // is a region on the canvas. Measured on the assembled screen: without
        // this the two host frames were reported as ends, because a frame's
        // signature is empty BY CONSTRUCTION so no link can ever leave one and
        // it qualified forever. See [`Document::steps`].
        let steps = self.steps(tree);
        if steps.is_empty() {
            return Err(NoHome::Empty);
        }
        let ends: Vec<End> = steps
            .iter()
            .copied()
            .filter(|node| !host.links().iter().any(|link| link.from.node == *node))
            .map(|node| End {
                node,
                fed: host.links().iter().any(|link| link.to.node == node),
            })
            .collect();
        // ★ The whole graph is a cycle. The reference goes to the world origin
        // here, which is a place rather than an answer.
        let first = ends.first().ok_or(NoHome::Endless { nodes: steps.len() })?;
        // A fed end is where the graph ENDS UP; an unfed one is a card nobody
        // wired. Prefer the former, and fall back to the first end rather than
        // refusing, because a graph with no links at all still has somewhere to
        // put a person.
        let at = ends.iter().find(|end| end.fed).unwrap_or(first).node;
        Ok(Home { at, ends })
    }
}
