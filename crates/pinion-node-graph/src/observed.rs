//! R1645 — what a source **reported**, beside what a user drew.
//!
//! A graph editor that draws only what the user drew is a diagram. One that
//! also draws what is *actually out there* — links discovered at run time,
//! links that came up on their own, links that were drawn and never appeared —
//! is a diagnostic instrument, and the whole of its value is the **difference
//! between the two layers**.
//!
//! `hello-graph-diff` (R1575) established the shape and could not use this
//! crate for it, because there was nowhere in the model to put a reported link:
//! it kept two sets of name pairs of its own, 801 lines, deriving the kind of
//! each link from which sets contained it. That derivation is right and it is
//! lifted here whole. What an example could not have is everything below.
//!
//! # An observed link is not in the graph
//!
//! It is a **report about** the graph, so it is not in [`Tree::links`](crate::Tree::links) and no
//! derivation in this crate walks it. That is structural rather than
//! disciplined: [`Document::evaluate`], [`Document::run`],
//! [`Document::cycle_nodes`] and every boundary derivation read a tree's links,
//! and an observation is not one of those. Putting a layer *tag* on [`Link`]
//! instead would have placed reported edges inside the structure every
//! derivation walks, where only care keeps them out — the same argument R1644
//! made for keeping breakpoints out of the run.
//!
//! # Observation is admitted where authoring is refused
//!
//! [`Document::connect`] refuses a wire that closes a value cycle, that
//! over-subscribes an input, or that crosses incompatible types. **The world is
//! under no such obligation.** A tool that declined to *record* what it saw
//! because its model forbids it would be a tool lying about the capture, so
//! [`Document::observe`] checks only that the report is about this graph — the
//! sockets exist and point the right way — and nothing else.
//!
//! Where that shows up is [`Document::adopt`], which runs the authoring rules
//! on a reported link and **names** the refusal. "This exists in the world and
//! your model cannot represent it" is a finding; silently dropping it is not.
//!
//! # A drawing that is known to be incomplete says so
//!
//! Some questions are only answerable when the drawn links **are** the
//! topology. Field experience with a lab built this way is explicit about it:
//! a static rule said a path was blocked and the real system disagreed, because
//! links had appeared that nobody drew. So [`Document::reaches`] answers with a
//! [`Standing`], and it is [`Standing::Partial`] when auto-discovery is on **or
//! when anything has drifted** — the second is the stronger condition and is
//! derived rather than switched: one reported link nobody drew is proof the
//! drawing is not the whole topology, whatever the switch says.
//!
//! And because both graphs are there, the answer is about both: a
//! [`Reachability`] whose `drawn` and `observed` disagree is precisely the
//! diagnostic that experience describes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{ConnectError, Connected, Document, Link, LinkId, NodeId, Socket, TreeId};

/// Whether links may arrive without being drawn (R1645).
///
/// **Off by default, and that is a determinism switch rather than a
/// preference**: with discovery off, the drawn graph is the whole topology and
/// every static question about it has an answer. Turning it on trades that for
/// fidelity, and [`Standing`] is where the trade is reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Discovery {
    /// Nothing arrives unbidden: what is drawn is what there is.
    Off,
    /// A source may report links nobody drew.
    On,
}

impl Discovery {
    /// Both, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 2] = [Self::Off, Self::On];

    /// A stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
        }
    }

    /// Parse a wire name back, or `None` — the inverse of [`name`](Self::name).
    #[must_use]
    pub fn from_wire(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|one| one.name() == name)
    }

    /// The closed vocabulary, projected from [`ALL`](Self::ALL).
    pub const WIRE_NAMES: [&'static str; Self::ALL.len()] = {
        let mut out = [""; Self::ALL.len()];
        let mut at = 0;
        while at < Self::ALL.len() {
            out[at] = Self::ALL[at].name();
            at += 1;
        }
        out
    };
}

impl Default for Discovery {
    fn default() -> Self {
        Self::Off
    }
}

/// One link a source reported (R1645).
///
/// It carries no [`LinkId`] and no mute, because it is not a link — it is a
/// report that traffic goes from one socket to another. Its identity is the
/// pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Observation {
    /// The tree the report is about.
    pub tree: TreeId,
    /// The producing socket.
    pub from: Socket,
    /// The consuming socket.
    pub to: Socket,
}

/// Which layers a link is in (R1645).
///
/// **Derived, never stored.** There is no field anywhere saying which layer a
/// link belongs to, and no code maintaining one: a link is authored or not, it
/// is observed or not, and this is the product of those two facts. A stored
/// kind would be a second source for one truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LinkLayer {
    /// Drawn and reported: the drawing is right about this one.
    Matched,
    /// Drawn and **not** reported — you drew this and it is not there.
    Missing,
    /// Reported and **not** drawn — this exists and you did not draw it.
    Drift,
}

impl LinkLayer {
    /// All three, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 3] = [Self::Matched, Self::Missing, Self::Drift];

    /// A stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::Missing => "missing",
            Self::Drift => "drift",
        }
    }

    /// Parse a wire name back, or `None` — the inverse of [`name`](Self::name).
    #[must_use]
    pub fn from_wire(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|one| one.name() == name)
    }

    /// The closed vocabulary, projected from [`ALL`](Self::ALL).
    pub const WIRE_NAMES: [&'static str; Self::ALL.len()] = {
        let mut out = [""; Self::ALL.len()];
        let mut at = 0;
        while at < Self::ALL.len() {
            out[at] = Self::ALL[at].name();
            at += 1;
        }
        out
    };
}

/// The difference between the two layers of one tree (R1645).
///
/// The product this whole axis exists for. Every member is a *derivation* of
/// the authored links and the observations; nothing here is maintained.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layers {
    matched: Vec<LinkId>,
    missing: Vec<LinkId>,
    drift: Vec<Observation>,
}

impl Layers {
    /// Links that are drawn and reported, ascending.
    #[must_use]
    pub fn matched(&self) -> &[LinkId] {
        &self.matched
    }

    /// Links that are drawn and were **not** reported, ascending.
    #[must_use]
    pub fn missing(&self) -> &[LinkId] {
        &self.missing
    }

    /// Reports that nothing drawn accounts for, ascending.
    #[must_use]
    pub fn drift(&self) -> &[Observation] {
        &self.drift
    }

    /// Whether the two layers agree completely.
    #[must_use]
    pub fn agrees(&self) -> bool {
        self.missing.is_empty() && self.drift.is_empty()
    }

    /// How many links are in each layer, by name — the census a header line
    /// shows.
    #[must_use]
    pub fn counts(&self) -> BTreeMap<&'static str, usize> {
        [
            (LinkLayer::Matched.name(), self.matched.len()),
            (LinkLayer::Missing.name(), self.missing.len()),
            (LinkLayer::Drift.name(), self.drift.len()),
        ]
        .into_iter()
        .collect()
    }
}

/// Whether an answer about the drawn graph can be trusted as an answer about
/// the world (R1645).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Standing {
    /// The drawn links **are** the topology: discovery is off and nothing has
    /// drifted, so a static answer is an answer about the world.
    Certain,
    /// The drawing is known to be a *partial* specification, so an answer about
    /// it is an answer about the drawing only.
    Partial {
        /// How many reported links nothing drawn accounts for. Non-zero is on
        /// its own enough to make an answer partial, whatever the switch says.
        drift: usize,
        /// Whether links are allowed to arrive undrawn at all.
        discovery: Discovery,
    },
}

impl Standing {
    /// Whether the drawn graph can be read as the topology.
    #[must_use]
    pub const fn is_certain(&self) -> bool {
        matches!(self, Self::Certain)
    }

    /// A stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Certain => "certain",
            Self::Partial { .. } => "partial",
        }
    }
}

impl fmt::Display for Standing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Certain => f.write_str("certain"),
            Self::Partial { drift, discovery } => write!(
                f,
                "partial (discovery {}, {drift} undrawn link(s) reported)",
                discovery.name()
            ),
        }
    }
}

/// An answer, and whether the drawing it was computed from is the topology
/// (R1645).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Judgement<T> {
    answer: T,
    standing: Standing,
}

impl<T> Judgement<T> {
    /// The answer.
    pub const fn answer(&self) -> &T {
        &self.answer
    }

    /// Whether it can be read as an answer about the world.
    pub const fn standing(&self) -> Standing {
        self.standing
    }

    /// The answer, only if it can be trusted as one about the world.
    ///
    /// The shape that makes the distinction hard to ignore: a caller who wants
    /// a bare `bool` has to say what it should mean when the drawing is known
    /// to be partial.
    pub const fn certain(&self) -> Option<&T> {
        match self.standing {
            Standing::Certain => Some(&self.answer),
            Standing::Partial { .. } => None,
        }
    }
}

/// Whether one node reaches another, on each layer (R1645).
///
/// Both, because when the drawing is partial they are different questions —
/// and the case that matters is the one field experience names: a static rule
/// said a path was blocked and the real system disagreed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reachability {
    /// Along the links someone drew.
    pub drawn: bool,
    /// Along the links a source reported.
    pub observed: bool,
}

impl Reachability {
    /// Whether the two layers disagree — the diagnostic this pair exists for.
    #[must_use]
    pub const fn disagrees(&self) -> bool {
        self.drawn != self.observed
    }
}

/// Why a report could not be recorded (R1645).
///
/// Deliberately short. A report is refused only when it is not *about this
/// graph*; every rule that governs authoring is left to [`Document::adopt`],
/// because the world does not obey them and a capture that dropped what it saw
/// would be the tool lying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ObserveError {
    /// No such tree.
    NoSuchTree(TreeId),
    /// No such node in that tree.
    NoSuchNode {
        /// The tree.
        tree: TreeId,
        /// The node that is not in it.
        node: NodeId,
    },
    /// A socket the report names is not a port of its node.
    NoSuchPort {
        /// The tree.
        tree: TreeId,
        /// The socket that does not resolve.
        socket: Socket,
    },
    /// The report is backwards: `from` must be an output and `to` an input.
    ///
    /// Refused rather than silently swapped, because which way traffic went is
    /// the content of the report.
    Backwards {
        /// The tree.
        tree: TreeId,
        /// The socket that is on the wrong side.
        socket: Socket,
    },
}

impl fmt::Display for ObserveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {tree}"),
            Self::NoSuchNode { tree, node } => write!(f, "tree {tree} has no node {node}"),
            Self::NoSuchPort { tree, socket } => {
                write!(f, "tree {tree} has no port for {socket}")
            }
            Self::Backwards { tree, socket } => write!(
                f,
                "in tree {tree}, {socket} is on the wrong side for the direction reported"
            ),
        }
    }
}

impl std::error::Error for ObserveError {}

/// Why a reported link could not be drawn (R1645).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AdoptError<T> {
    /// Nothing was reported there, so there is nothing to adopt.
    NotObserved(Observation),
    /// It was reported and the model cannot hold it — the finding this verb
    /// exists to produce, rather than a failure to be swallowed.
    CannotAuthor(ConnectError<T>),
}

impl<T: fmt::Debug> fmt::Display for AdoptError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotObserved(what) => write!(
                f,
                "nothing reported from {} to {} in tree {}",
                what.from, what.to, what.tree
            ),
            Self::CannotAuthor(why) => {
                write!(f, "reported, and this graph cannot hold it: {why:?}")
            }
        }
    }
}

impl<T: fmt::Debug> std::error::Error for AdoptError<T> {}

impl<K: crate::model::NodeKind> Document<K> {
    /// Whether links are allowed to arrive undrawn (R1645).
    #[must_use]
    pub fn discovery(&self) -> Discovery {
        self.discovery_setting()
    }

    /// Turn auto-discovery on or off, answering what it was.
    ///
    /// The default is [`Discovery::Off`], and it is off because the drawn graph
    /// being the topology is what makes every static question answerable. See
    /// [`Standing`].
    pub fn set_discovery(&mut self, discovery: Discovery) -> Discovery {
        self.set_discovery_setting(discovery)
    }

    /// Record that a source reported traffic from `from` to `to` (R1645).
    ///
    /// Answers whether this is new. Checks only that the report is **about this
    /// graph** — see this module's header for why nothing else is checked.
    ///
    /// # Errors
    ///
    /// [`ObserveError`].
    pub fn observe(
        &mut self,
        tree: TreeId,
        from: Socket,
        to: Socket,
    ) -> Result<bool, ObserveError> {
        self.check_report(tree, from, to)?;
        Ok(self.record(Observation { tree, from, to }))
    }

    /// Forget one report, answering whether it was there.
    pub fn unobserve(&mut self, tree: TreeId, from: Socket, to: Socket) -> bool {
        self.forget(&Observation { tree, from, to })
    }

    /// Forget every report about `tree`, answering how many.
    pub fn clear_observations(&mut self, tree: TreeId) -> usize {
        self.forget_tree(tree)
    }

    /// Every report about `tree`, ascending.
    #[must_use]
    pub fn observations(&self, tree: TreeId) -> Vec<Observation> {
        self.reports()
            .filter(|one| one.tree == tree)
            .copied()
            .collect()
    }

    /// The difference between the two layers of `tree` (R1645).
    #[must_use]
    pub fn layers(&self, tree: TreeId) -> Layers {
        let reported: BTreeSet<(Socket, Socket)> = self
            .reports()
            .filter(|one| one.tree == tree)
            .map(|one| (one.from, one.to))
            .collect();
        let drawn: BTreeSet<(Socket, Socket)> = self
            .tree(tree)
            .map(|host| host.links().iter().map(|l| (l.from, l.to)).collect())
            .unwrap_or_default();
        let mut found = Layers::default();
        for link in self.tree(tree).map(Self::links_of).unwrap_or_default() {
            if reported.contains(&(link.from, link.to)) {
                found.matched.push(link.id);
            } else {
                found.missing.push(link.id);
            }
        }
        found.matched.sort_unstable();
        found.missing.sort_unstable();
        found.drift = self
            .reports()
            .filter(|one| one.tree == tree && !drawn.contains(&(one.from, one.to)))
            .copied()
            .collect();
        found
    }

    /// Which layers one link is in, by its sockets (R1645).
    #[must_use]
    pub fn link_layer(&self, tree: TreeId, from: Socket, to: Socket) -> Option<LinkLayer> {
        let drawn = self
            .tree(tree)
            .is_some_and(|host| host.links().iter().any(|l| l.from == from && l.to == to));
        let reported = self
            .reports()
            .any(|one| one.tree == tree && one.from == from && one.to == to);
        match (drawn, reported) {
            (true, true) => Some(LinkLayer::Matched),
            (true, false) => Some(LinkLayer::Missing),
            (false, true) => Some(LinkLayer::Drift),
            (false, false) => None,
        }
    }

    /// Draw a link that was reported, so the drawing says what is there (R1645).
    ///
    /// Runs the **authoring** rules — it is [`Document::connect`] underneath —
    /// so a reported link this model cannot hold is *named* rather than
    /// dropped. That refusal is the finding: the world is doing something the
    /// drawing cannot express.
    ///
    /// # Errors
    ///
    /// [`AdoptError`].
    pub fn adopt(
        &mut self,
        tree: TreeId,
        from: Socket,
        to: Socket,
    ) -> Result<Connected, AdoptError<K::Type>> {
        let what = Observation { tree, from, to };
        if !self.reports().any(|one| *one == what) {
            return Err(AdoptError::NotObserved(what));
        }
        self.connect(tree, from, to)
            .map_err(AdoptError::CannotAuthor)
    }

    /// Whether an answer computed from `tree`'s drawn links is an answer about
    /// the world (R1645).
    #[must_use]
    pub fn standing(&self, tree: TreeId) -> Standing {
        let drift = self.layers(tree).drift.len();
        let discovery = self.discovery();
        if drift == 0 && discovery == Discovery::Off {
            Standing::Certain
        } else {
            Standing::Partial { drift, discovery }
        }
    }

    /// Whether `start` reaches `goal` on each layer, and whether that answer
    /// can be read as one about the world (R1645).
    ///
    /// The drawn answer is [`Document::data_path_between`], so it obeys the
    /// same causality rule every other data-plane derivation does. The observed
    /// answer is plain reachability over the reported pairs: a report says
    /// traffic went somewhere, and nothing in it claims to be a data-plane
    /// dependency.
    #[must_use]
    pub fn reaches(&self, tree: TreeId, start: NodeId, goal: NodeId) -> Judgement<Reachability> {
        Judgement {
            answer: Reachability {
                drawn: self.data_path_between(tree, start, goal).is_some(),
                observed: self.reported_reaches(tree, start, goal),
            },
            standing: self.standing(tree),
        }
    }

    /// Plain reachability over the reported pairs.
    fn reported_reaches(&self, tree: TreeId, start: NodeId, goal: NodeId) -> bool {
        if start == goal {
            return true;
        }
        let mut seen: BTreeSet<NodeId> = BTreeSet::from([start]);
        let mut pending = vec![start];
        while let Some(here) = pending.pop() {
            for step in self
                .reports()
                .filter(|one| one.tree == tree && one.from.node == here)
            {
                if step.to.node == goal {
                    return true;
                }
                if seen.insert(step.to.node) {
                    pending.push(step.to.node);
                }
            }
        }
        false
    }

    /// A report is about this graph: both sockets resolve, and they point the
    /// way the report says traffic went.
    fn check_report(&self, tree: TreeId, from: Socket, to: Socket) -> Result<(), ObserveError> {
        if self.tree(tree).is_none() {
            return Err(ObserveError::NoSuchTree(tree));
        }
        for (socket, producing) in [(from, true), (to, false)] {
            let signature = self
                .signature(tree, socket.node)
                .ok_or(ObserveError::NoSuchNode {
                    tree,
                    node: socket.node,
                })?;
            let side = if producing {
                &signature.outputs
            } else {
                &signature.inputs
            };
            if side.get(socket.port as usize).is_none() {
                // Named apart: a port that exists on the OTHER side is a report
                // written backwards, and one that exists on neither is a report
                // about a graph this is not.
                let other = if producing {
                    &signature.inputs
                } else {
                    &signature.outputs
                };
                return Err(if other.get(socket.port as usize).is_some() {
                    ObserveError::Backwards { tree, socket }
                } else {
                    ObserveError::NoSuchPort { tree, socket }
                });
            }
        }
        Ok(())
    }

    fn links_of(host: &crate::model::Tree<K>) -> Vec<Link> {
        host.links().to_vec()
    }
}
