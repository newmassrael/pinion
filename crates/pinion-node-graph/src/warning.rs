//! ★★★★★ R1927 — **a node says whether it is in a questionable state, and why,
//! computed from the situation it is actually in.**
//!
//! # What the reference does, measured at its own header and both overriders
//!
//! Its graph node publishes two overridable answers: *should this node show a
//! visual warning* (a bool) and *what does that warning say* (a text). Both
//! supplied answers are the empty ones — false, and no text. One consumer: the
//! node widget makes a badge visible when the first answers yes.
//!
//! Reading the two overriders in the whole engine source is what shaped this
//! module, and each finding contradicts a clause the census carried:
//!
//! 1. **It is not a STATE.** The census's covering sentence read *a node has no
//!    warning state*. It is a `const` method returning a computed bool — asked
//!    every time the widget lays out. The node does carry a stored flag for a
//!    compiler message, with its own clearing method, but that is a different
//!    pair of members and a different capability; the visual warning stores
//!    nothing.
//! 2. **It is not something a KIND attaches to itself.** The other half of that
//!    sentence read *a diagnostic a kind attaches to itself*, and it is
//!    backwards. Of the two overriders, one answers from **whether one of its
//!    own pins is wired** together with a setting on the object that CONTAINS
//!    it; the other answers from the **running** node it is debugging. The kind
//!    supplies the *rule*; the answer is per node and situational.
//! 3. ★★★★★ **The two answers can come apart, and in the reference they do.**
//!    They are independent virtuals: measured, one of the two overriders
//!    overrides only the bool and leaves the text at its empty supplied answer,
//!    so that node warns and says nothing. A person is shown a badge with no
//!    reason in it.
//!
//! # The three measured ways this passes it
//!
//! 1. **A warning cannot be silent.** One answer, not two: a kind returns
//!    `Option<String>`, so *warning* and *what it says* arrive together and
//!    the reference's silent badge is unrepresentable rather than merely
//!    discouraged.
//! 2. **The situation is HANDED to the kind, not reached for.** Both overriders
//!    climb out of themselves to find what they need — one walks its chain of
//!    containers in a loop looking for a particular ancestor, the other asks a
//!    global for the node being debugged — precisely because their signature
//!    gives them nothing. [`Surroundings`] passes what a rule needs, which is
//!    what makes the hook a function of its arguments and therefore testable
//!    without building a world around it.
//! 3. **A graph can be asked what is warning.** The reference has no such call:
//!    the badge is per node, decided inside the widget, so *what is wrong on
//!    this canvas* has to be assembled by whoever wants it.
//!    [`Document::warnings`] is that list, in node order, and it is what a
//!    "take me to the next problem" affordance needs.
//!
//! # What this is NOT, and the line is deliberate
//!
//! [`Document::validate`](crate::Document::validate) answers how a **document**
//! breaks its own structural rules — a dangling link, an overfed socket, a
//! cycle — and nothing an application says can add to it or take from it. A
//! warning is the other half: the **application's** judgement about one node,
//! in a graph that is perfectly well formed. Neither can be derived from the
//! other, and keeping them apart is what stops a kind from being able to
//! silence a structural finding.

use std::collections::BTreeSet;

use crate::model::{Document, NodeBody, NodeId, NodeKind, Side, TreeId};

/// What a kind may look at when deciding whether its node is warning.
///
/// Deliberately narrow: it says which of the node's own sockets have a link on
/// them, and nothing else. That is what the reference's own rule needed and
/// could not be given — and a hook handed the whole document could reach
/// anything, which makes *what does this rule depend on* unanswerable and the
/// rule impossible to re-run in a test.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Surroundings {
    inputs: BTreeSet<u32>,
    outputs: BTreeSet<u32>,
}

impl Surroundings {
    /// Whether a link lands on that socket of this node.
    #[must_use]
    pub fn is_wired(&self, side: Side, index: u32) -> bool {
        self.side(side).contains(&index)
    }

    /// Whether anything at all is wired on that side.
    #[must_use]
    pub fn any_wired(&self, side: Side) -> bool {
        !self.side(side).is_empty()
    }

    /// Every wired socket index on that side, ascending.
    pub fn wired(&self, side: Side) -> impl Iterator<Item = u32> + '_ {
        self.side(side).iter().copied()
    }

    fn side(&self, side: Side) -> &BTreeSet<u32> {
        match side {
            Side::Input => &self.inputs,
            Side::Output => &self.outputs,
        }
    }
}

/// ★★★★★ R1941 — **what a kind says is wrong with its node, and how heavily.**
///
/// R1927 made *warning* and *what it says* one answer so a badge could not be
/// silent. This adds the axis that answer did not carry: **whether the
/// objection stops anything.**
///
/// # What forced it, measured in the reference this round
///
/// Its graph node is asked, during compilation, to validate itself, and what it
/// says goes into the compiler's own message log — the same log whose ERROR
/// COUNT decides whether the compile succeeded. Measured across the editor's
/// blueprint nodes, those implementations record **27 errors, 31 warnings and
/// 2 notes**: the same hook, routinely, at three different weights, and the
/// heaviest of them STOPS THE BUILD. So the capability is not *a node may
/// complain*; it is *a node may REFUSE*.
///
/// Three arms, each carrying its own sentence, so the states the reference can
/// reach are unrepresentable here:
///
/// * **The weight lives in the TYPE, not in which method was called.** There,
///   severity is chosen by calling one of three log methods, and a node that
///   forgets which one it meant is indistinguishable from one that meant it.
///   A value cannot forget.
/// * **A sentence cannot come apart from its weight**, which is R1927's rule
///   applied to the new axis: the sentence is inside the arm.
/// * **And nothing is unclassified.** There is no fourth *said something,
///   weight unknown* state, because there is nowhere to put it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Objection {
    /// The node cannot be run as it stands, and this says why.
    ///
    /// The weight that STOPS things: [`Document::may_run`] is false while any
    /// node answers this.
    Blocks(String),
    /// The node will run and something about it is suspect.
    Warns(String),
    /// Worth knowing, and neither of the above.
    ///
    /// Its own arm rather than folded into [`Warns`](Self::Warns) because the
    /// reference distinguishes them and a screen showing every note as a
    /// warning is how a list of warnings stops being read.
    Notes(String),
}

impl Objection {
    /// What it says, whatever its weight.
    #[must_use]
    pub fn sentence(&self) -> &str {
        match self {
            Self::Blocks(said) | Self::Warns(said) | Self::Notes(said) => said,
        }
    }

    /// Whether this objection stops the graph being run.
    #[must_use]
    pub const fn blocks(&self) -> bool {
        matches!(self, Self::Blocks(_))
    }
}

/// What a node says is wrong with it, with the node it is about.
///
/// Carries the node so a list of these is addressable — the half the
/// reference's per-node badge does not have to solve and its absent
/// graph-level list would have to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// The node it is about.
    pub node: NodeId,
    /// What is wrong and how heavily, in the application's own words.
    ///
    /// Never empty by construction: a kind answers `None` or an objection, and
    /// an objection carries its sentence inside its weight — so there is no
    /// state in which a node objects without saying why, and none in which it
    /// says something at no weight at all.
    pub objection: Objection,
}

impl Warning {
    /// What is wrong, in the application's own words.
    #[must_use]
    pub fn sentence(&self) -> &str {
        self.objection.sentence()
    }
}

impl<K: NodeKind> Document<K> {
    /// What one node says is wrong with it, or `None` when it says nothing.
    ///
    /// `None` for a node that is not there, and `None` for a structural body —
    /// a frame, a group instance, an interface end and a delay are this crate's
    /// and have no application rule to ask.
    #[must_use]
    pub fn warning(&self, tree: TreeId, node: NodeId) -> Option<Warning> {
        let held = self.tree(tree)?.node(node)?;
        let NodeBody::Kind(kind) = &held.body else {
            return None;
        };
        let objection = kind.warning(&self.surroundings(tree, node))?;
        Some(Warning { node, objection })
    }

    /// Every warning in a tree, in node order.
    ///
    /// Node order for [`Document::validate`]'s reason and for the reference's:
    /// a person asking for *the first problem* means the first one they would
    /// read, and an order that came out of a hash would answer differently on
    /// two runs.
    #[must_use]
    pub fn warnings(&self, tree: TreeId) -> Vec<Warning> {
        let Some(held) = self.tree(tree) else {
            return Vec::new();
        };
        let mut found: Vec<NodeId> = held.nodes().map(|node| node.id).collect();
        found.sort_unstable();
        found
            .into_iter()
            .filter_map(|node| self.warning(tree, node))
            .collect()
    }

    /// ★★★★★ R1941 — **may this tree be run?**
    ///
    /// False while any node answers [`Objection::Blocks`]. The question a
    /// person asks before starting anything, answered as a VALUE — which is
    /// the difference from the reference that matters most here.
    ///
    /// # Why this is the shape, measured
    ///
    /// There, a node's validation writes into a compiler message log and
    /// returns nothing, so *is this node all right?* has no answer except by
    /// compiling and counting what came out. The weight is chosen by which
    /// logging method the implementation happened to call, and the verdict
    /// exists only as a count of errors accumulated in a log that also holds
    /// everything else the compile said.
    ///
    /// Here a kind returns its objection, so this is a question anybody may
    /// ask, at any time, without running anything and without a log — and
    /// [`Document::objections`] hands back exactly the ones that stop it, in
    /// node order, so "why not?" is answerable in the same breath.
    ///
    /// ⚠ It says nothing about whether the document is WELL FORMED — that is
    /// [`Document::validate`](crate::Document::validate), which no application
    /// may add to or silence. A tree can be perfectly well formed and still
    /// refuse to run because a kind objects to one node's configuration, and a
    /// tree with a structural fault is not runnable whatever every kind says.
    /// The two gates are separate on purpose and a caller wanting *can I start
    /// this* must pass both.
    #[must_use]
    pub fn may_run(&self, tree: TreeId) -> bool {
        self.objections(tree).is_empty()
    }

    /// ★★★★★ R1941 — every objection in this tree that STOPS it being run, in
    /// node order.
    ///
    /// The other half of [`may_run`](Self::may_run): a refusal a person cannot
    /// act on is a refusal they will work around. Node order for
    /// [`warnings`](Self::warnings)'s reason.
    ///
    /// ⚠ A filter over [`warnings`](Self::warnings) rather than a second walk
    /// of the tree, so the blocking list and the full list cannot disagree
    /// about what any one node said — the shape R1940 found the reference
    /// getting wrong on another axis, where two consumers each carried their
    /// own copy of one decision.
    #[must_use]
    pub fn objections(&self, tree: TreeId) -> Vec<Warning> {
        self.warnings(tree)
            .into_iter()
            .filter(|held| held.objection.blocks())
            .collect()
    }

    /// What is wired to one node, as a kind's rule sees it.
    ///
    /// Public because a rule is worth testing on its own, and a caller that
    /// could not build the argument would have to drive the whole document to
    /// exercise one sentence.
    #[must_use]
    pub fn surroundings(&self, tree: TreeId, node: NodeId) -> Surroundings {
        let Some(held) = self.tree(tree) else {
            return Surroundings::default();
        };
        let mut around = Surroundings::default();
        for link in held.links() {
            if link.to.node == node {
                around.inputs.insert(link.to.port);
            }
            if link.from.node == node {
                around.outputs.insert(link.from.port);
            }
        }
        around
    }
}
