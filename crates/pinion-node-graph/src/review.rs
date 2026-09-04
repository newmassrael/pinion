//! ★★★★★ R1945 — **one check answers for the whole document, and every finding
//! names the card a person must be taken to.**
//!
//! # What the reference does, measured at the command, its predicate, its log
//! and its status
//!
//! The script editor's compile command is one declaration mapped to one action
//! behind one predicate. What it produces is a **results log** — a flat array of
//! messages in emission order, two hand-kept counts beside it, and a separate
//! **unordered set** of the nodes any message annotated — plus a **status** on
//! the document, six-valued, of which three are outcomes of a check
//! (up to date / up to date with warnings / error) and three are lifecycle
//! (unknown / modified-since / being created).
//!
//! Five measurements shaped this module, each contradicting or completing a
//! clause the census row carried:
//!
//! 1. ★★★★★ **The row said we have no compile-time diagnostic list, and that is
//!    FALSE.** [`Document::validate`](crate::Document::validate) has answered
//!    the structural half for many rounds and
//!    [`Document::warnings`](crate::Document::warnings) the judgement half since
//!    R1927. What was missing is not a list — it is the **join**, and R1941 said
//!    so in its own words: *a caller wanting "can I start this" must pass both*.
//!    Nothing joined them, so every consumer joined them by hand or, as the node
//!    lab did, read one arm of one of them and dropped the rest.
//! 2. ★★★★★ **"Which node do I take you to" has TWO implementations there, and
//!    they read different sources.** One walks every graph asking each node
//!    about a **stored** compiler-message flag it carries; the other reads the
//!    log's annotated-node set. Two answers to one question, maintained apart,
//!    free to disagree. Here there is one derivation — [`Document::sites_of`] —
//!    and it is a function of the finding and the document.
//! 3. ★★★★★ **The status is a stored flag assigned by hand.** Counted in the
//!    reference tree: **ten** assignments of the modified-since value, across
//!    eight files in six modules, none of them the compile itself. A state that
//!    every mutator must remember to set is a state a new mutator forgets. Here
//!    a review is a **function of the document**, so there is no state to
//!    invalidate and no such flag exists to go stale — the same reason this
//!    crate's searches are derivations rather than kept result lists.
//! 4. **The counts are kept beside the list rather than derived from it.** The
//!    log's error and warning totals are incremented by its own logging methods,
//!    and it publishes two public methods that move a count **without adding a
//!    message** (measured: zero callers in that tree, so the divergence is
//!    possible and not yet taken). Here [`Review::counted`] counts the findings.
//! 5. **The predicate that gates the command is a bool with no reason.**
//!    Measured: one declaration, **zero** overriders, two consumers; it answers
//!    false for a library graph and while the editor is not in editing mode, and
//!    a person shown a disabled button is told neither. This module does not
//!    reproduce that: the *findings* are the reason, and [`Review::fitness`]
//!    reports the verdict rather than hiding it behind a greyed control.
//!
//! # What this deliberately does not reproduce
//!
//! A graph here is INTERPRETED ([`Document::run`](crate::Document::run)) and is
//! never lowered to a separate artifact, so there is no build product to be out
//! of date with and no compiled/modified-since state. That clause of the census
//! row is TRUE and stays true. What the compile command *gives the editor* — a
//! whole document checked in one act, every finding placed on a card, ordered so
//! the worst is first, with a verdict over the lot — is what is reproduced here.

use std::cmp::Reverse;

use crate::group::Violation;
use crate::model::{Document, InterfaceSide, LinkId, NodeBody, NodeId, NodeKind, Side, TreeId};
use crate::section::SectionBreach;
use crate::warning::Objection;

/// How heavily to take one finding.
///
/// The three words are [`Objection`]'s own, so a kind's answer and a structural
/// fault are weighed in one vocabulary rather than two that a consumer has to
/// reconcile. Ordered lightest first, so the **greatest** is the worst and
/// [`Review::worst`] is a maximum rather than a convention.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, pinion_derive::VariantCensus,
)]
#[variant_census(all)]
pub enum Weight {
    /// Worth knowing, and neither of the below.
    Notes,
    /// It will run, and something about it is suspect.
    Warns,
    /// It cannot run as it stands.
    Blocks,
}

impl Weight {
    /// Every arm, lightest first.
    pub const ALL: [Self; 3] = [Self::Notes, Self::Warns, Self::Blocks];

    /// Whether a finding of this weight stops the graph being run.
    #[must_use]
    pub const fn blocks(self) -> bool {
        matches!(self, Self::Blocks)
    }

    /// The word this goes onto a wire as.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Notes => "notes",
            Self::Warns => "warns",
            Self::Blocks => "blocks",
        }
    }
}

/// What one finding is.
///
/// Two arms and not one flattened list of sentences, because the two halves
/// differ in **who may silence them**: a structural fault is the document's own
/// verdict and no application may add to it or take from it (R1941), while a
/// judgement is a kind's opinion about one node. A consumer that had only the
/// sentence could not tell which it was holding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The document breaks its own rules —
    /// [`Document::validate`](crate::Document::validate)'s verdict.
    Structure(Violation),
    /// A kind's judgement about one node —
    /// [`Document::warnings`](crate::Document::warnings)'s.
    Judgement(Objection),
}

impl Fault {
    /// How heavily to take it.
    ///
    /// ★ A structural fault always [`Weight::Blocks`], and that is R1941's own
    /// sentence rather than a choice made here: *a tree with a structural fault
    /// is not runnable whatever every kind says*. A judgement carries its weight
    /// inside itself.
    #[must_use]
    pub const fn weight(&self) -> Weight {
        match self {
            Self::Structure(_) => Weight::Blocks,
            Self::Judgement(objection) => match objection {
                Objection::Blocks(_) => Weight::Blocks,
                Objection::Warns(_) => Weight::Warns,
                Objection::Notes(_) => Weight::Notes,
            },
        }
    }

    /// What it says, in a sentence a screen can show.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Self::Structure(violation) => violation.to_string(),
            Self::Judgement(objection) => objection.sentence().to_owned(),
        }
    }
}

/// One thing wrong, and **the cards it is on**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The tree it is in.
    pub tree: TreeId,
    /// Every card the finding is about, **most-blamed first**.
    ///
    /// The first is where a person is taken; the rest are the others worth
    /// seeing, and a fault of a *pair* has two so neither end looks clean.
    /// Empty when nothing on the canvas answers for it, which is a legitimate
    /// answer for exactly the faults whose subject is not a node — and which
    /// arms those are is asserted, not left to a reader.
    pub sites: Vec<NodeId>,
    /// What is wrong.
    pub fault: Fault,
}

impl Finding {
    /// How heavily to take it — the fault's own weight, not a second copy.
    #[must_use]
    pub const fn weight(&self) -> Weight {
        self.fault.weight()
    }

    /// The card a person is taken to, if any card answers for this.
    #[must_use]
    pub fn site(&self) -> Option<NodeId> {
        self.sites.first().copied()
    }

    /// What it says.
    #[must_use]
    pub fn sentence(&self) -> String {
        self.fault.sentence()
    }
}

/// What a review says about running the document.
///
/// The three the reference's status carries as **outcomes** of a check. Its
/// other three arms are lifecycle — unknown, modified-since, being created —
/// and have no counterpart here for the reason stated in this module's header:
/// a review is a function of the document, so it is never out of date with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum Fitness {
    /// Nothing was found.
    Clean,
    /// Nothing stops it running, and something was said.
    Remarked,
    /// Something stops it running.
    Stopped,
}

impl Fitness {
    /// Every arm, in ascending severity.
    pub const ALL: [Self; 3] = [Self::Clean, Self::Remarked, Self::Stopped];

    /// Whether the document may be run as it stands.
    #[must_use]
    pub const fn may_run(self) -> bool {
        !matches!(self, Self::Stopped)
    }

    /// The word this goes onto a wire as.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Remarked => "remarked",
            Self::Stopped => "stopped",
        }
    }
}

/// Everything one check found, **worst first**.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Review {
    findings: Vec<Finding>,
}

impl Review {
    /// Every finding, heaviest first, then in tree and node order.
    #[must_use]
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// The one to take a person to.
    ///
    /// ★ The reference answers this with a scan for the most severe annotated
    /// node, run separately from the log it is derived from. Here the order IS
    /// the answer, so the two cannot come apart.
    #[must_use]
    pub fn worst(&self) -> Option<&Finding> {
        self.findings.first()
    }

    /// How many findings carry this weight — counted from the list rather than
    /// kept beside it.
    #[must_use]
    pub fn counted(&self, weight: Weight) -> usize {
        self.findings
            .iter()
            .filter(|found| found.weight() == weight)
            .count()
    }

    /// Whether nothing was found at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    /// How many findings there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.findings.len()
    }

    /// The verdict over the lot.
    #[must_use]
    pub fn fitness(&self) -> Fitness {
        match self.worst() {
            None => Fitness::Clean,
            Some(found) if found.weight().blocks() => Fitness::Stopped,
            Some(_) => Fitness::Remarked,
        }
    }
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1945 — **check the whole document, once**, and hand back
    /// everything wrong with it, worst first, each finding naming the cards it
    /// is on.
    ///
    /// The join R1941 named and left to the caller: the structural verdict
    /// ([`validate`](Self::validate)) and every kind's judgement
    /// ([`warnings`](Self::warnings)) are one ordered list here, so *can I start
    /// this* and *what do I fix first* are one question with one answer.
    ///
    /// ⚠ Neither half is re-derived: this calls both, so a consumer of the
    /// review and a consumer of either list cannot come to disagree about what
    /// the document said — the shape R1940 found the reference getting wrong on
    /// another axis, where two consumers each carried their own copy of one
    /// decision.
    #[must_use]
    pub fn review(&self) -> Review {
        let mut findings: Vec<Finding> = Vec::new();
        for violation in self.validate() {
            findings.push(Finding {
                tree: violation_tree(&violation),
                sites: self.sites_of(&violation),
                fault: Fault::Structure(violation),
            });
        }
        let trees: Vec<TreeId> = self.trees().map(|held| held.id).collect();
        for tree in trees {
            for held in self.warnings(tree) {
                findings.push(Finding {
                    tree,
                    sites: vec![held.node],
                    fault: Fault::Judgement(held.objection),
                });
            }
        }
        // Stable, so findings of equal weight, tree and card keep the order the
        // two halves produced them in — which is already tree order and then
        // node order, because that is the order both halves walk in.
        findings.sort_by_key(|found| (Reverse(found.weight()), found.tree.0, found.site()));
        Review { findings }
    }

    /// ★★★★★ R1945 — **which cards answer for a structural fault**, most-blamed
    /// first.
    ///
    /// One derivation, matched over every arm with no wildcard, so an arm added
    /// later is a build failure rather than a fault that silently lands on no
    /// card. The reference has two functions answering this question from two
    /// different sources; this is the one.
    ///
    /// The order within a pair is the blame: the end an author must change comes
    /// first and the other follows, so a screen may take a person to the first
    /// and still show that the other is involved.
    #[must_use]
    pub fn sites_of(&self, violation: &Violation) -> Vec<NodeId> {
        match violation {
            // The link names a socket that is not there. Whichever end lost the
            // port is the blamed one; if a whole node went, the surviving end is
            // all there is to show.
            Violation::DanglingLink { tree, link } => {
                let blame = self.lost_port_end(*tree, *link);
                self.link_sites(*tree, *link, blame)
            }
            // The sink's requirement is the one that was not met, so it is
            // blamed; the source is shown because an author may change either.
            Violation::TypeMismatch { tree, link } => {
                self.link_sites(*tree, *link, Some(Side::Input))
            }
            // The refusal already says which end to change (R1885).
            Violation::Incompatible {
                tree,
                link,
                refusal,
            } => self.link_sites(*tree, *link, Some(refusal.end)),
            Violation::Overlinked { socket, .. } => vec![socket.node],
            Violation::DanglingEcho { node, .. }
            | Violation::DanglingInstance { node, .. }
            | Violation::ContainmentCycle { node, .. }
            | Violation::StrayPortValue { node, .. }
            | Violation::MistypedPortValue { node, .. }
            | Violation::InadmissiblePortValue { node, .. }
            | Violation::TooManyItems { node, .. }
            // R1999 — the card whose kind this graph does not admit. One card,
            // and it is the one to act on: the repairs are moving it out or
            // re-classifying the tree, and both start from looking at it.
            | Violation::NotAtHome { node, .. }
            // The container it names is not there, so there is one card.
            | Violation::DanglingParent { node, .. } => vec![*node],
            // Both are there and both are the fault: the contained node is
            // blamed because moving it is the repair, and the container is shown
            // because making it a frame is the other one.
            Violation::ParentNotAFrame { node, parent, .. } => vec![*node, *parent],
            // Every node on the knot, ascending — the set R1596 says is the only
            // one that can be acted on.
            Violation::Cycle { nodes, .. } => nodes.clone(),
            // A section is a grouping of one half of a tree's interface, and the
            // card that materialises that half is the one to go to.
            Violation::SectionBroken { tree, breach, .. } => self
                .interface_sites(*tree, breach_side(breach))
                .into_iter()
                .take(1)
                .collect(),
            // The whole point of the arm is that there is more than one, so all
            // of them: a person has to delete one and cannot be shown only the
            // survivor.
            Violation::DuplicateInterfaceNode { tree, side } => self.interface_sites(*tree, *side),
            // The chain is between DEFINITIONS. Its direct case has cards — the
            // instances of the definition standing inside the definition itself
            // — and an indirect chain has none in any one tree, which is why
            // this can legitimately answer nothing.
            Violation::Recursion { definition } => self
                .instances_of(*definition)
                .into_iter()
                .filter(|(tree, _)| tree == definition)
                .map(|(_, node)| node)
                .collect(),
        }
    }

    /// The ends of a link that still exist, the blamed one first.
    fn link_sites(&self, tree: TreeId, link: LinkId, blame: Option<Side>) -> Vec<NodeId> {
        let Some(held) = self.tree(tree).and_then(|held| held.link(link)) else {
            return Vec::new();
        };
        let (first, second) = match blame {
            Some(Side::Output) => (held.from.node, held.to.node),
            _ => (held.to.node, held.from.node),
        };
        [first, second]
            .into_iter()
            .filter(|node| self.signature(tree, *node).is_some())
            .collect()
    }

    /// Which end of a dangling link lost the port, when exactly one did.
    ///
    /// `None` when a whole node went — there is then no port to have lost, and
    /// no end is more to blame than the other.
    fn lost_port_end(&self, tree: TreeId, link: LinkId) -> Option<Side> {
        let held = self.tree(tree)?.link(link)?;
        let source = self.signature(tree, held.from.node)?;
        let sink = self.signature(tree, held.to.node)?;
        match (
            source.outputs.len() <= held.from.port as usize,
            sink.inputs.len() <= held.to.port as usize,
        ) {
            (true, false) => Some(Side::Output),
            (false, true) => Some(Side::Input),
            _ => None,
        }
    }

    /// Every node in a tree materialising one half of its interface, ascending.
    ///
    /// [`Tree::interface_node`](crate::Tree::interface_node) answers *the sole*
    /// one, which is the wrong question for the fault that says there are two.
    fn interface_sites(&self, tree: TreeId, side: InterfaceSide) -> Vec<NodeId> {
        let Some(held) = self.tree(tree) else {
            return Vec::new();
        };
        let mut found: Vec<NodeId> = held
            .nodes()
            .filter(|node| node.body == NodeBody::Interface(side))
            .map(|node| node.id)
            .collect();
        found.sort_unstable();
        found
    }
}

/// Which half of an interface a breach is about.
///
/// The switch arms name an input index because a section's switch is an input by
/// construction, so the side follows from the arm rather than being carried.
const fn breach_side(breach: &SectionBreach) -> InterfaceSide {
    match breach {
        SectionBreach::NoSuchMember(port) | SectionBreach::MemberShared(port) => port.side,
        SectionBreach::SwitchNotAMember(_) | SectionBreach::SwitchNotSwitchable(_) => {
            InterfaceSide::Input
        }
    }
}

/// Which tree a violation is about.
///
/// ★ Every arm carries its tree except the recursion, whose subject IS a
/// definition — so the definition is its tree, and this is the one place that
/// has to know it.
const fn violation_tree(violation: &Violation) -> TreeId {
    match violation {
        Violation::DanglingLink { tree, .. }
        | Violation::DanglingEcho { tree, .. }
        | Violation::Overlinked { tree, .. }
        | Violation::TypeMismatch { tree, .. }
        | Violation::Incompatible { tree, .. }
        | Violation::Cycle { tree, .. }
        | Violation::SectionBroken { tree, .. }
        | Violation::DanglingInstance { tree, .. }
        | Violation::DuplicateInterfaceNode { tree, .. }
        | Violation::DanglingParent { tree, .. }
        | Violation::ParentNotAFrame { tree, .. }
        | Violation::ContainmentCycle { tree, .. }
        | Violation::StrayPortValue { tree, .. }
        | Violation::MistypedPortValue { tree, .. }
        | Violation::InadmissiblePortValue { tree, .. }
        | Violation::TooManyItems { tree, .. }
        | Violation::NotAtHome { tree, .. } => *tree,
        Violation::Recursion { definition } => *definition,
    }
}
