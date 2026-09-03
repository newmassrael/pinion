//! ★★★★★ R1988 — **which cards a selection is about, and why each one is.**
//!
//! # What the reference does, measured
//!
//! The engine spells this capability **twice**, in two editors, and the two do
//! not compute the same thing:
//!
//! * The script editor's collects, from every selected node, the **exec**
//!   closure upstream and downstream *and* the value closure upstream and
//!   downstream — except that a selected node which is itself a pure value node
//!   gets only the value halves, and the value walk stops descending at the
//!   first impure node it reaches (it takes that node and does not recurse
//!   through it).
//! * The material editor's has no such distinction — that graph has no
//!   execution pins — and instead carries an option, off by default, that turns
//!   the downstream walk into *the whole chain*: for every node found
//!   downstream, its own upstream closure is collected too. Siblings, in other
//!   words.
//!
//! Neither can express the other, and neither publishes which of them ran.
//!
//! # The three things this answers that a bool cannot
//!
//! Both editors record the outcome as **one bit per node**, which the canvas
//! reads as an opacity. So:
//!
//! 1. **Nobody can ask why a node is dim.** With two nodes selected and a chain
//!    between them the middle one is *both* downstream of one and upstream of
//!    the other, and one bit says neither. [`Relatedness`] carries the
//!    [`Tie`]s.
//! 2. **Nobody can ask which closure is in force.** There the mode is a bool on
//!    the editor plus a checkbox in a dropdown, so "on" and "on, whole chain"
//!    are two facts free to disagree; here they are one [`Focus`] with an arm
//!    each, and *on with no closure* is unrepresentable.
//! 3. **"Nothing ties it" and "I do not know that card" are the same answer.**
//!    Both editors iterate the graph's own node array, so the question cannot
//!    arise there — but a *client* holding an id from another tree can ask it,
//!    and R1983 measured what those ids do when carried across a threshold.
//!    [`Relatedness::Foreign`] is that third answer.
//!
//! # A frame is related by what it holds, not by where it is drawn
//!
//! The reference exempts comment nodes from the fade and then decides each one
//! separately, in the *widget* layer: a comment is related when some related
//! node's **top-left corner** falls inside the comment's drawn rectangle. That
//! is geometry, and it is the geometry of one corner — a card overlapping a
//! comment from above is not in it, and a card dragged clear of a comment it
//! still belongs to is.
//!
//! R1589 gave this crate a **declared** containment relation, so the question is
//! asked of membership instead: a [`NodeBody::Frame`] is related when something
//! it contains is ([`Tie::Holding`]). No card geometry is involved, which is
//! also why this can live in a model crate at all — see the [`select`] module
//! docs for the two DCC operators that genuinely cannot.
//!
//! # This is a pure query
//!
//! Nothing in the document changes, so a screen may ask what focusing *would*
//! show without showing it (§2 #3). The reference's is an edit on the nodes,
//! and its own undo does not carry it.
//!
//! [`select`]: crate::Grow

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{Document, NodeBody, NodeId, NodeKind, TreeId};
use crate::select::{Grow, Reach, SelectError};

/// How far relatedness travels from a selection.
///
/// Two arms and not a bool beside a flag: see the module docs for the
/// reference's pair, which is a bool on the editor plus a checkbox in a
/// dropdown.
/// Serialisable because a screen holds the mode in the framework's own state
/// channel, which round-trips a signal's value — not because a *document*
/// carries one: a focus is what a person is looking at now, and R1923's rule
/// says a derived view of a graph does not get frozen into its file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Focus {
    /// The selection's ancestors and descendants — what feeds it and what it
    /// feeds, however far.
    ///
    /// The reference's default in both editors.
    Lineage,
    /// [`Self::Lineage`], and then everything that feeds anything the selection
    /// feeds.
    ///
    /// The material editor's *focus whole chain* option: it answers "what
    /// contributes to the same results as this?", which takes in siblings that
    /// [`Self::Lineage`] cannot reach in either direction.
    Chain,
}

impl Focus {
    /// Every focus, in the order a control offers them — widening.
    pub const ALL: [Self; 2] = [Self::Lineage, Self::Chain];

    /// The word a wire carries for this focus.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Lineage => "lineage",
            Self::Chain => "chain",
        }
    }

    /// The focus that word names, or `None`.
    ///
    /// Paired with [`Self::word`] so a client's vocabulary is this type's and
    /// not a second list beside it.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|one| one.word() == word)
    }
}

impl fmt::Display for Focus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// One reason a node is related to a selection.
///
/// A node can carry several at once, which is the fact the reference's single
/// bit destroys — see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tie {
    /// It is in the selection.
    Selected,
    /// It feeds the selection, however far.
    Upstream,
    /// The selection feeds it, however far.
    Downstream,
    /// It feeds something the selection feeds, and neither reaches the other.
    ///
    /// Only [`Focus::Chain`] finds these.
    Chain,
    /// A frame, related because something it contains is.
    Holding,
}

impl Tie {
    /// The word a wire carries for this tie.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Upstream => "upstream",
            Self::Downstream => "downstream",
            Self::Chain => "chain",
            Self::Holding => "holding",
        }
    }
}

impl fmt::Display for Tie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// Where one node stands with respect to a focused selection.
///
/// **Three answers, so a type** rather than an `Option` of ties: `Foreign` and
/// `Unrelated` are different statements, and a caller handed one list of
/// related nodes cannot tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relatedness<'a> {
    /// Not a node of the tree this answer was derived over. Nothing is claimed
    /// about it — including that it is unrelated.
    Foreign,
    /// A node of that tree, with nothing tying it to the selection.
    Unrelated,
    /// Related, and these are the ties. **Never empty**, ascending, without
    /// repeats.
    Related(&'a [Tie]),
}

impl Relatedness<'_> {
    /// Whether this node is related.
    ///
    /// [`Self::Foreign`] is **not** related, and it is not unrelated either —
    /// which is why the predicate is spelled one way round and the other
    /// question is [`Self::is_unrelated`].
    #[must_use]
    pub const fn is_related(&self) -> bool {
        matches!(self, Self::Related(_))
    }

    /// Whether this is a node of the tree with nothing tying it to the
    /// selection — the ones a screen fades.
    #[must_use]
    pub const fn is_unrelated(&self) -> bool {
        matches!(self, Self::Unrelated)
    }

    /// The ties, empty for [`Self::Foreign`] and [`Self::Unrelated`].
    ///
    /// For rendering a reason; not for deciding relatedness, which is what
    /// [`Self::is_related`] and [`Self::is_unrelated`] are for.
    #[must_use]
    pub const fn ties(&self) -> &[Tie] {
        match self {
            Self::Foreign | Self::Unrelated => &[],
            Self::Related(ties) => ties,
        }
    }
}

/// What a focus question answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Focused {
    /// Which closure produced this.
    focus: Focus,
    /// Related nodes and their ties. Values are never empty.
    ties: BTreeMap<NodeId, Vec<Tie>>,
    /// Nodes of the tree that nothing ties to the selection, ascending.
    unrelated: Vec<NodeId>,
}

impl Focused {
    /// Which closure produced this answer.
    ///
    /// Carried on the answer rather than remembered by the caller, so a screen
    /// that publishes its reasons cannot publish them under the wrong mode.
    #[must_use]
    pub const fn focus(&self) -> Focus {
        self.focus
    }

    /// Where `node` stands.
    #[must_use]
    pub fn relatedness(&self, node: NodeId) -> Relatedness<'_> {
        if let Some(ties) = self.ties.get(&node) {
            Relatedness::Related(ties)
        } else if self.unrelated.binary_search(&node).is_ok() {
            Relatedness::Unrelated
        } else {
            Relatedness::Foreign
        }
    }

    /// Every related node, ascending.
    #[must_use]
    pub fn related(&self) -> Vec<NodeId> {
        self.ties.keys().copied().collect()
    }

    /// Every node nothing ties to the selection, ascending — what a screen
    /// fades.
    #[must_use]
    pub fn unrelated(&self) -> &[NodeId] {
        &self.unrelated
    }
}

impl<K: NodeKind> Document<K> {
    /// Which nodes of `tree` the `selection` is about, and why each one is.
    ///
    /// A **pure query**: see the module docs for what the reference's two
    /// spellings of this do instead, and for the three questions this answers
    /// that their one bit per node cannot.
    ///
    /// # Errors
    ///
    /// [`SelectError::NoSuchTree`] and [`SelectError::NoSuchNode`] as for
    /// [`Document::grow`], which vets the same pair — a stale selection is
    /// refused rather than skipped.
    ///
    /// [`SelectError::NothingSelected`] when `selection` is empty. Refused, and
    /// that is a decision: the honest set-theoretic answer is *every node is
    /// unrelated*, which a screen applying it would paint as a graph faded to
    /// nothing. The reference guards the same case with a hidden bool and
    /// silently resets; here the caller is told, so a screen that shows
    /// everything does it because the question was refused rather than because
    /// it guessed.
    pub fn focus(
        &self,
        tree: TreeId,
        selection: &[NodeId],
        focus: Focus,
    ) -> Result<Focused, SelectError> {
        let host = self.tree(tree).ok_or(SelectError::NoSuchTree(tree))?;
        if selection.is_empty() {
            return Err(SelectError::NothingSelected(tree));
        }
        for &id in selection {
            if host.node(id).is_none() {
                return Err(SelectError::NoSuchNode { tree, node: id });
            }
        }

        let mut ties: BTreeMap<NodeId, BTreeSet<Tie>> = BTreeMap::new();
        for &id in selection {
            ties.entry(id).or_default().insert(Tie::Selected);
        }

        // Two calls and not one, because the two directions are two facts about
        // each node found and a union would lose which was which.
        let down = self.grow(tree, selection, Grow::Downstream(Reach::Transitive))?;
        for &id in &down.added {
            ties.entry(id).or_default().insert(Tie::Downstream);
        }
        let up = self.grow(tree, selection, Grow::Upstream(Reach::Transitive))?;
        for &id in &up.added {
            ties.entry(id).or_default().insert(Tie::Upstream);
        }

        if focus == Focus::Chain && !down.added.is_empty() {
            // ★ Seeded from what the selection FEEDS, which is the reference's
            // own shape: its whole-chain option collects the upstream closure
            // of each node found downstream. Asked once over all of them rather
            // than once per node — the closure of a union is the union of the
            // closures, so this is the same set for one walk instead of n.
            let chain = self.grow(tree, &down.added, Grow::Upstream(Reach::Transitive))?;
            for id in chain.added {
                if !ties.contains_key(&id) {
                    ties.entry(id).or_default().insert(Tie::Chain);
                }
            }
        }

        // ★ Last, and over the answer so far: a frame is related by what it
        // holds, so it cannot be decided before the things it holds have been.
        let frames: Vec<NodeId> = host
            .nodes()
            .filter(|node| matches!(node.body, NodeBody::Frame))
            .map(|node| node.id)
            .collect();
        for frame in frames {
            if self
                .contents(tree, frame)
                .iter()
                .any(|held| ties.contains_key(held))
            {
                ties.entry(frame).or_default().insert(Tie::Holding);
            }
        }

        // ★ Sorted, because `standing` asks this by bisection and a caller
        // reading it gets one order rather than the tree's iteration order.
        let mut unrelated: Vec<NodeId> = host
            .nodes()
            .map(|node| node.id)
            .filter(|id| !ties.contains_key(id))
            .collect();
        unrelated.sort_unstable();
        Ok(Focused {
            focus,
            ties: ties
                .into_iter()
                .map(|(id, set)| (id, set.into_iter().collect()))
                .collect(),
            unrelated,
        })
    }
}
