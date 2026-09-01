//! R1644 — a run that can be stopped, stepped and watched.
//!
//! [`Document::run`](crate::Document::run) answers which nodes run and
//! [`Document::tick`](crate::Document::tick) advances what the graph remembers.
//! Both answer in one gulp: a run derives a whole execution order and a tick
//! moves every register at one instant, and **neither can be interrupted**. So
//! the two questions a person actually asks of a graph that misbehaves — *stop
//! before this node* and *what is this port holding* — had no mechanism at all.
//! The engine's visual-script debug utility class is fifteen commands over
//! exactly those two, and its behaviour-tree debugger adds five more for
//! stepping.
//!
//! # A breakpoint cannot change the run
//!
//! That is the whole design, and it is bought by a decision two rounds older.
//! Because state is a delay and nothing else (R1600), a run is a **pure
//! function** of the document and the registers — so the debugger never
//! suspends anything. It computes the entire run and then *moves about inside
//! it*: a [`Timeline`] is the run plus the watched values, and every debugger
//! command is arithmetic over that timeline. The breakpoints are not an
//! argument to the walk.
//!
//! Once **per command**, not once per session — [`Document::debug`] rebuilds
//! the timeline every time, because the document and the registers may have
//! moved between two commands and a cached run would be a claim that they had
//! not. That is the same cost the view already pays: a
//! [`Run`] has been recomputed per frame since R1599.
//!
//! Nothing weaker would do. A debugger that halts a running machine is a
//! debugger whose observations are of a *different* execution from the one the
//! program has without it, and every reverse-stepping implementation then has
//! to record frames to get back — which is what the engine's behaviour-tree
//! debugger does (`CurrentValues` versus `SavedValues` are two different
//! commands there, over recorded data). Here going backwards is the same
//! arithmetic as going forwards, on the same object, and "the value now" and
//! "the value at step 4" are one question.
//!
//! # Where a debugger may stop, and what it may watch
//!
//! A [`NodeSite`] is an address for stopping and a [`PortSite`] for watching,
//! and both carry an [`Occurrence`]: **every** instance of the definition the
//! node is in, or one named instance. The reference has no such axis and cannot
//! — a macro there is expanded by `CloneGraph` before anything runs, so its N
//! uses are N sets of nodes and a breakpoint on one is a breakpoint on one
//! copy. Here a definition's tree is shared, so "stop the second time through
//! this subgraph" is a thing to say.
//!
//! Both addresses are **checked against the document** rather than trusted,
//! for the reason [`Document::force`](crate::Document::force) is checked there:
//! only the document knows that the node exists, that a run can reach it at
//! all, and that the port carries a value. A breakpoint on a node with no
//! control port could never fire — it is *pulled*, not run — and a silently
//! inert breakpoint is worse than a refused one.
//!
//! # Six strides, from two words
//!
//! The reference names five stepping commands (`ForwardInto`, `ForwardOver`,
//! `BackInto`, `BackOver`, `StepOut`). They are a [`Direction`] and a
//! [`Stride`], which is 2 × 3 = **six**, and [`Command::STRIDES`] is that
//! product computed at compile time rather than a list to keep in step. The
//! sixth cell — stepping back *out* — is not a feature added on top; it is the
//! cell the reference's naming left unwritten.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::machine::{Machine, NESTING_LIMIT};
use crate::model::{
    Document, Instance, NodeBody, NodeId, NodeKind, Port, PortRef, ROOT, Side, TreeId,
};
use crate::run::{Run, RunError, Step, Stop};

// ------------------------------------------------------------------ addresses

/// Which occurrences of a node a breakpoint or a watch applies to (R1644).
///
/// A definition's tree is shared by every instance of it, so a node inside one
/// is *several* places a run can be. The reference has no equivalent: it
/// expands a macro into a copy per use before anything runs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Occurrence {
    /// Every instance, including the root reading.
    Any,
    /// One instance only.
    At(Instance),
}

impl Occurrence {
    /// Whether this occurrence covers `instance`.
    #[must_use]
    pub fn admits(&self, instance: &Instance) -> bool {
        match self {
            Self::Any => true,
            Self::At(only) => only == instance,
        }
    }

    /// Read one back from the form [`Display`](fmt::Display) writes, or `None`.
    ///
    /// `*` is every occurrence; anything else is an [`Instance`] path.
    #[must_use]
    pub fn from_wire(text: &str) -> Option<Self> {
        if text == Self::ANY {
            return Some(Self::Any);
        }
        Instance::from_wire(text).map(Self::At)
    }

    /// The wire spelling of [`Any`](Self::Any).
    ///
    /// Named because it is written on both sides of the wire and an instance
    /// path can never collide with it — every path starts with `/`.
    pub const ANY: &'static str = "*";
}

impl fmt::Display for Occurrence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => f.write_str(Self::ANY),
            Self::At(instance) => write!(f, "{instance}"),
        }
    }
}

/// One node, in some set of its occurrences: where a run may be stopped.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeSite {
    /// The tree the node is in.
    pub tree: TreeId,
    /// The node.
    pub node: NodeId,
    /// Which occurrences of it.
    pub occurrence: Occurrence,
}

impl NodeSite {
    /// The node, in every occurrence.
    #[must_use]
    pub const fn any(tree: TreeId, node: NodeId) -> Self {
        Self {
            tree,
            node,
            occurrence: Occurrence::Any,
        }
    }

    /// The node, in one instance only.
    #[must_use]
    pub const fn at(tree: TreeId, node: NodeId, instance: Instance) -> Self {
        Self {
            tree,
            node,
            occurrence: Occurrence::At(instance),
        }
    }

    /// Read one back from the form [`Display`](fmt::Display) writes, or `None`.
    #[must_use]
    pub fn from_wire(text: &str) -> Option<Self> {
        let (address, occurrence) = text.split_once('@')?;
        let (tree, node) = address.split_once(':')?;
        Some(Self {
            tree: TreeId(tree.parse().ok()?),
            node: NodeId(node.parse().ok()?),
            occurrence: Occurrence::from_wire(occurrence)?,
        })
    }
}

impl fmt::Display for NodeSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}@{}", self.tree, self.node, self.occurrence)
    }
}

/// One port of one node, in some set of its occurrences: what a watch reads.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PortSite {
    /// The tree the node is in.
    pub tree: TreeId,
    /// The node.
    pub node: NodeId,
    /// Which of its ports.
    pub port: PortRef,
    /// Which occurrences of it.
    pub occurrence: Occurrence,
}

impl PortSite {
    /// The port, in every occurrence.
    #[must_use]
    pub const fn any(tree: TreeId, node: NodeId, port: PortRef) -> Self {
        Self {
            tree,
            node,
            port,
            occurrence: Occurrence::Any,
        }
    }

    /// The port, in one instance only.
    #[must_use]
    pub const fn at(tree: TreeId, node: NodeId, port: PortRef, instance: Instance) -> Self {
        Self {
            tree,
            node,
            port,
            occurrence: Occurrence::At(instance),
        }
    }

    /// The node this port is on, in the same occurrences.
    #[must_use]
    pub fn node_site(&self) -> NodeSite {
        NodeSite {
            tree: self.tree,
            node: self.node,
            occurrence: self.occurrence.clone(),
        }
    }

    /// Read one back from the form [`Display`](fmt::Display) writes, or `None`.
    ///
    /// The side is found by trying each name as a prefix, which is unambiguous
    /// because neither [`Side`] name is a prefix of the other — a property of
    /// that vocabulary rather than of this parse, so it is asserted where the
    /// vocabulary lives.
    #[must_use]
    pub fn from_wire(text: &str) -> Option<Self> {
        let (address, occurrence) = text.split_once('@')?;
        let (owner, port) = address.rsplit_once('.')?;
        let (tree, node) = owner.split_once(':')?;
        let (side, index) = Side::ALL
            .into_iter()
            .find_map(|side| port.strip_prefix(side.name()).map(|rest| (side, rest)))?;
        Some(Self {
            tree: TreeId(tree.parse().ok()?),
            node: NodeId(node.parse().ok()?),
            port: PortRef {
                side,
                index: index.parse().ok()?,
            },
            occurrence: Occurrence::from_wire(occurrence)?,
        })
    }
}

impl fmt::Display for PortSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}.{}@{}",
            self.tree, self.node, self.port, self.occurrence
        )
    }
}

// ---------------------------------------------------------------- breakpoints

/// Where a run stops, and which of those places are live (R1644).
///
/// Holds no reference to a document, for the reason a [`Machine`] holds none:
/// it can be saved, restored, compared and sent. Whether a site is still one
/// the document supports is [`Document::stale_breakpoints`], asked rather than
/// assumed.
///
/// **Disabled is not removed.** A disabled breakpoint remembers its place,
/// which is why the reference has five commands here and not three, and why
/// this is a map to a flag rather than a set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breakpoints {
    #[serde(with = "armed_rows")]
    armed: BTreeMap<NodeSite, bool>,
}

/// Breakpoints travel as **rows**, for the reason a [`Machine`]'s registers do:
/// a [`NodeSite`] is not a string, and a map keyed by one is not expressible in
/// JSON at all.
mod armed_rows {
    use super::NodeSite;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize)]
    struct Row {
        site: NodeSite,
        enabled: bool,
    }

    pub(super) fn serialize<S>(
        armed: &BTreeMap<NodeSite, bool>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        armed
            .iter()
            .map(|(site, enabled)| Row {
                site: site.clone(),
                enabled: *enabled,
            })
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<NodeSite, bool>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Vec::<Row>::deserialize(deserializer)?
            .into_iter()
            .map(|row| (row.site, row.enabled))
            .collect())
    }
}

impl Breakpoints {
    /// No breakpoints.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            armed: BTreeMap::new(),
        }
    }

    /// How many places are armed, enabled or not.
    #[must_use]
    pub fn len(&self) -> usize {
        self.armed.len()
    }

    /// Whether nothing is armed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.armed.is_empty()
    }

    /// Every armed place and whether it is enabled, ascending.
    pub fn iter(&self) -> impl Iterator<Item = (&NodeSite, bool)> {
        self.armed.iter().map(|(site, enabled)| (site, *enabled))
    }

    /// Whether this exact site is armed.
    #[must_use]
    pub fn contains(&self, site: &NodeSite) -> bool {
        self.armed.contains_key(site)
    }

    /// Whether this exact site is armed **and** enabled.
    #[must_use]
    pub fn is_enabled(&self, site: &NodeSite) -> bool {
        self.armed.get(site) == Some(&true)
    }

    /// Remove a breakpoint, answering whether it was there.
    ///
    /// Needs no document: forgetting a place is not a claim about one.
    pub fn disarm(&mut self, site: &NodeSite) -> bool {
        self.armed.remove(site).is_some()
    }

    /// Enable or disable an armed breakpoint, answering what it was.
    ///
    /// `None` when nothing is armed there — which is a different fact from
    /// "it was already disabled", and the caller is the one who can tell them
    /// apart.
    pub fn set_enabled(&mut self, site: &NodeSite, enabled: bool) -> Option<bool> {
        self.armed
            .get_mut(site)
            .map(|held| std::mem::replace(held, enabled))
    }

    /// Enable every armed breakpoint, answering how many changed.
    pub fn enable_all(&mut self) -> usize {
        self.set_all(true)
    }

    /// Disable every armed breakpoint, answering how many changed.
    pub fn disable_all(&mut self) -> usize {
        self.set_all(false)
    }

    fn set_all(&mut self, enabled: bool) -> usize {
        let mut moved = 0;
        for held in self.armed.values_mut() {
            if *held != enabled {
                *held = enabled;
                moved += 1;
            }
        }
        moved
    }

    /// Forget every breakpoint, answering how many were forgotten.
    pub fn clear(&mut self) -> usize {
        let had = self.armed.len();
        self.armed.clear();
        had
    }

    /// The enabled breakpoint that stops a run arriving at this node in this
    /// occurrence, if any.
    ///
    /// The tree is part of the question because a [`NodeId`] is unique only
    /// within its tree.
    #[must_use]
    pub fn stops_at(&self, tree: TreeId, node: NodeId, instance: &Instance) -> Option<&NodeSite> {
        self.armed
            .iter()
            .find(|(site, enabled)| {
                **enabled
                    && site.tree == tree
                    && site.node == node
                    && site.occurrence.admits(instance)
            })
            .map(|(site, _)| site)
    }
}

/// Why a breakpoint could not be armed (R1644).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BreakError {
    /// No such tree.
    NoSuchTree(TreeId),
    /// No such node in that tree.
    NoSuchNode {
        /// The tree.
        tree: TreeId,
        /// The node that is not in it.
        node: NodeId,
    },
    /// The occurrence names a chain of group instances this document does not
    /// have.
    NoSuchInstance,
    /// The occurrence is a real instance, of a *different* tree than the site
    /// names.
    ///
    /// Named apart from [`Self::NoSuchInstance`] because the two are opposite
    /// mistakes: one is an address that does not resolve, and this is one that
    /// resolves somewhere else.
    InstanceIsElsewhere {
        /// The tree the site named.
        site: TreeId,
        /// The tree the occurrence actually lands in.
        occurrence: TreeId,
    },
    /// The node has no control port at all, so no run can arrive at it: it is
    /// *pulled* by whoever reads its output, exactly as it always was.
    ///
    /// Refused rather than armed, because a breakpoint that can never fire
    /// reads as a breakpoint that never fired.
    NotOnTheControlPlane {
        /// The tree.
        tree: TreeId,
        /// The node that is off the control plane.
        node: NodeId,
    },
    /// The node is a group instance, which takes no turn of its own — entering
    /// one shows up as the first step *inside* it. Break there instead.
    IsAnInstance {
        /// The tree.
        tree: TreeId,
        /// The instance node.
        node: NodeId,
    },
}

impl fmt::Display for BreakError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {tree}"),
            Self::NoSuchNode { tree, node } => write!(f, "tree {tree} has no node {node}"),
            Self::NoSuchInstance => f.write_str("no such instance in this document"),
            Self::InstanceIsElsewhere { site, occurrence } => write!(
                f,
                "the site names tree {site} and the occurrence lands in tree {occurrence}"
            ),
            Self::NotOnTheControlPlane { tree, node } => write!(
                f,
                "node {node} in tree {tree} has no control port, so no run arrives at it"
            ),
            Self::IsAnInstance { tree, node } => write!(
                f,
                "node {node} in tree {tree} is a group instance and takes no turn of its own"
            ),
        }
    }
}

impl std::error::Error for BreakError {}

// -------------------------------------------------------------------- watches

/// The ports a run reports the value of (R1644).
///
/// A set rather than a map: a watch has nothing to be disabled — removing it is
/// the same act — and the reference agrees, keeping its watched pins in a flat
/// array beside its breakpoint objects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watches {
    on: BTreeSet<PortSite>,
}

impl Watches {
    /// Nothing watched.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            on: BTreeSet::new(),
        }
    }

    /// How many ports are watched.
    #[must_use]
    pub fn len(&self) -> usize {
        self.on.len()
    }

    /// Whether nothing is watched.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.on.is_empty()
    }

    /// Every watched port, ascending.
    pub fn iter(&self) -> impl Iterator<Item = &PortSite> {
        self.on.iter()
    }

    /// Whether this exact port is watched.
    #[must_use]
    pub fn contains(&self, site: &PortSite) -> bool {
        self.on.contains(site)
    }

    /// Stop watching a port, answering whether it was watched.
    pub fn unwatch(&mut self, site: &PortSite) -> bool {
        self.on.remove(site)
    }

    /// Stop watching everything, answering how many were dropped.
    pub fn clear(&mut self) -> usize {
        let had = self.on.len();
        self.on.clear();
        had
    }
}

/// Why a port could not be watched (R1644).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WatchError {
    /// No such tree.
    NoSuchTree(TreeId),
    /// No such node in that tree.
    NoSuchNode {
        /// The tree.
        tree: TreeId,
        /// The node that is not in it.
        node: NodeId,
    },
    /// The occurrence names a chain of group instances this document does not
    /// have.
    NoSuchInstance,
    /// The occurrence is a real instance of a different tree than the site
    /// names.
    InstanceIsElsewhere {
        /// The tree the site named.
        site: TreeId,
        /// The tree the occurrence actually lands in.
        occurrence: TreeId,
    },
    /// The node has no such port on that side.
    NoSuchPort {
        /// The tree.
        tree: TreeId,
        /// The node.
        node: NodeId,
        /// The port that is not on it.
        port: PortRef,
    },
    /// The port is a **control** port, and control is not a value: there is
    /// nothing to report. The reference refuses the same thing, by asking its
    /// schema whether the pin's category is an execution one.
    NotAValue {
        /// The tree.
        tree: TreeId,
        /// The node.
        node: NodeId,
        /// The control port.
        port: PortRef,
    },
    /// ★★★★★ R1942 — the port carries a value of a type the **taxonomy** says
    /// has nothing a person can read, and this says which type and why.
    ///
    /// Distinct from [`NotAValue`](Self::NotAValue), and the two are not two
    /// spellings of one refusal: that one is the CRATE's — control is not a
    /// value by construction — and this one is the taxonomy's, about a port
    /// that does carry a value. The reference reaches both through one `bool`
    /// from one schema call, which is why a person told *no* there cannot tell
    /// which of the two they met.
    ///
    /// The sentence is the taxonomy's own ([`NodeKind::inspectable`]), carried
    /// rather than re-derived, so what a debugger shows and what the taxonomy
    /// declared cannot differ.
    NotInspectable {
        /// The tree.
        tree: TreeId,
        /// The node.
        node: NodeId,
        /// The port.
        port: PortRef,
        /// Why this type has nothing to read, in the taxonomy's own words.
        why: String,
    },
}

impl fmt::Display for WatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {tree}"),
            Self::NoSuchNode { tree, node } => write!(f, "tree {tree} has no node {node}"),
            Self::NoSuchInstance => f.write_str("no such instance in this document"),
            Self::InstanceIsElsewhere { site, occurrence } => write!(
                f,
                "the site names tree {site} and the occurrence lands in tree {occurrence}"
            ),
            Self::NoSuchPort { tree, node, port } => {
                write!(f, "node {node} in tree {tree} has no port {port}")
            }
            Self::NotAValue { tree, node, port } => write!(
                f,
                "port {port} of node {node} in tree {tree} carries control, not a value"
            ),
            Self::NotInspectable {
                tree,
                node,
                port,
                why,
            } => write!(f, "port {port} of node {node} in tree {tree} holds {why}"),
        }
    }
}

impl std::error::Error for WatchError {}

/// ★★★★★ R1942 — **whether a value of a type can be LOOKED AT while the graph
/// runs.**
///
/// See [`NodeKind::inspectable`] for the measurement. Two arms rather than a
/// `bool`, and the sentence is the whole reason: the reference answers this
/// with a bare `bool` whose one consumer folds FIVE separate refusals into it,
/// so a person told *no* cannot tell which of the five they met.
///
/// ⚠ Not an `Option<String>`, where `None` would have to mean *yes*: a reader
/// meeting `None` has to be told which way it reads, and the two states here
/// are a permission and a refusal rather than a value and its absence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inspectable {
    /// A value of this type can be read.
    Yes,
    /// It cannot, and this says what it holds instead — in the taxonomy's own
    /// words, so a debugger quotes rather than invents.
    ///
    /// Reads into the refusal's sentence as *port … holds …*, so a taxonomy
    /// writes a noun phrase: "a live connection, which has no value to read".
    No(String),
}

/// What a watched port held, in one occurrence (R1644).
///
/// One per occurrence rather than one per step, and that is a *consequence*
/// rather than a simplification: a port's value is a pure function of the
/// document and the registers, so within one run it cannot differ between two
/// moments. What it does differ between is **instances** — which is the axis
/// the reference cannot have, and the reason its watch surface needs a
/// "currently selected debug object" to mean anything at all.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading<K: NodeKind> {
    /// The watch that asked. Its occurrence is as the client wrote it, which
    /// may be [`Occurrence::Any`].
    pub site: PortSite,
    /// The occurrence this reading is from.
    pub instance: Instance,
    /// What the port holds, or `None` when nothing arrives at it.
    pub value: Option<K::Value>,
    /// The first step at which the watched node ran in this occurrence.
    ///
    /// `None` when it never did — which for a node with no control port is not
    /// a failure but its nature: a pure node is pulled, never run. Reported
    /// rather than left to be inferred, because "a value that is not on the
    /// trace" and "a value the run never reached" look alike and are not.
    pub ran_at: Option<usize>,
}

// ------------------------------------------------------------------- commands

/// Which way a stride goes (R1644).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Direction {
    /// On through the run.
    Forward,
    /// Back the way it came.
    Back,
}

impl Direction {
    /// Both directions, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 2] = [Self::Forward, Self::Back];

    /// A stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Back => "back",
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

/// How far one stride goes (R1644).
///
/// The three are about **depth**: a group instance is a frame, and these are
/// the three things to do with one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Stride {
    /// One step, wherever it is — descending into a group instance if that is
    /// where control goes next.
    Into,
    /// One step at this depth or shallower: a group instance entered on the way
    /// runs to completion.
    Over,
    /// Until control is shallower than it is now: the frame runs to completion.
    Out,
}

impl Stride {
    /// All three, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 3] = [Self::Into, Self::Over, Self::Out];

    /// A stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Into => "into",
            Self::Over => "over",
            Self::Out => "out",
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

/// One debugger command (R1644).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// On to the next enabled breakpoint, or to the end. Always leaves where it
    /// is: a resume that re-stopped where it started could never get past a
    /// breakpoint.
    Resume,
    /// One stride, one way.
    Step {
        /// Which way.
        direction: Direction,
        /// How far.
        stride: Stride,
    },
    /// Back to the entry, having run nothing.
    Restart,
}

impl Command {
    /// Every stride, as the **product** it is (R1644).
    ///
    /// The reference names five stepping commands and this is 2 × 3. The array's
    /// length is that product rather than a literal, and the fill is positional,
    /// so adding an arm to either vocabulary and forgetting this grows the array
    /// and leaves a cell unfilled — which is a *compile* error, not a census
    /// somebody has to re-run. R1643 found the mirror mistake in a hand-written
    /// list checked only for length: an arm used twice passed, published one
    /// name twice, and lost the other.
    pub const STRIDES: [Self; Direction::ALL.len() * Stride::ALL.len()] = {
        let mut filled = [None; Direction::ALL.len() * Stride::ALL.len()];
        let mut way = 0;
        while way < Direction::ALL.len() {
            let mut far = 0;
            while far < Stride::ALL.len() {
                filled[way * Stride::ALL.len() + far] = Some(Self::Step {
                    direction: Direction::ALL[way],
                    stride: Stride::ALL[far],
                });
                far += 1;
            }
            way += 1;
        }
        let mut out = [Self::Resume; Direction::ALL.len() * Stride::ALL.len()];
        let mut at = 0;
        while at < out.len() {
            out[at] = match filled[at] {
                Some(one) => one,
                None => panic!("a stride cell was left unfilled"),
            };
            at += 1;
        }
        out
    };
}

/// Why a debugger is where it is (R1644).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Halt {
    /// At the entry, having run nothing.
    Entry,
    /// A stride finished here.
    Stepped,
    /// The node about to run has an enabled breakpoint on it — and has **not**
    /// run. The reference stops in the same place, before the node it marks.
    AtBreakpoint {
        /// The breakpoint that stopped it.
        site: NodeSite,
        /// The occurrence it stopped in.
        instance: Instance,
        /// The node about to run.
        node: NodeId,
    },
    /// The run is over. [`Stop`] says which way it ended.
    Ended(Stop),
}

impl Halt {
    /// A stable name, for a caption or a wire form.
    ///
    /// The two [`Stop`] arms keep the spelling
    /// [`Document::run`](crate::Document::run)'s consumers already publish, so a
    /// client reading "why did it stop" gets one vocabulary and not two.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::Stepped => "stepped",
            Self::AtBreakpoint { .. } => "breakpoint",
            Self::Ended(Stop::Halted) => "halted",
            Self::Ended(Stop::BudgetExhausted) => "budget_exhausted",
        }
    }
}

/// Where a command would land, without applying it (R1644).
///
/// Published so a caller can *plan* a move — the same reason
/// `dry_run` exists one layer up (§2 #3): asking costs nothing and changes
/// nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Landing {
    /// How many steps will have been taken.
    pub at: usize,
    /// Why it is there.
    pub halt: Halt,
}

// ------------------------------------------------------------------- timeline

/// A whole run, with what a debugger needs to move about in it (R1644).
///
/// The run itself, the tree each step happened in, and every watched port's
/// value per occurrence. Computed **without reference to any breakpoint**,
/// which is what makes "a breakpoint cannot change the run" structural rather
/// than a promise.
#[derive(Debug, Clone, PartialEq)]
pub struct Timeline<K: NodeKind> {
    run: Run<K>,
    hosts: Vec<TreeId>,
    readings: Vec<Reading<K>>,
}

impl<K: NodeKind> Timeline<K> {
    /// The run.
    #[must_use]
    pub const fn run(&self) -> &Run<K> {
        &self.run
    }

    /// Every watched port's value, per occurrence, ascending by site then
    /// occurrence.
    #[must_use]
    pub fn readings(&self) -> &[Reading<K>] {
        &self.readings
    }

    /// How many steps the run took.
    #[must_use]
    pub fn len(&self) -> usize {
        self.run.steps().len()
    }

    /// Whether nothing ran.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.run.steps().is_empty()
    }

    /// The step at `at`, or `None` past the end.
    #[must_use]
    pub fn step(&self, at: usize) -> Option<&Step<K>> {
        self.run.steps().get(at)
    }

    /// The tree the step at `at` happened in, or `None` past the end.
    #[must_use]
    pub fn host(&self, at: usize) -> Option<TreeId> {
        self.hosts.get(at).copied()
    }

    /// How deep the step at `at` was — how many group instances control had
    /// descended through.
    #[must_use]
    pub fn depth(&self, at: usize) -> Option<usize> {
        self.step(at).map(|step| step.instance.depth())
    }

    /// Where `command` lands, from `at`, without applying it.
    #[must_use]
    pub fn seek(&self, at: usize, command: Command, breakpoints: &Breakpoints) -> Landing {
        let end = self.len();
        let from = at.min(end);
        let landed = match command {
            Command::Restart => 0,
            Command::Step {
                direction: Direction::Forward,
                stride,
            } => self.forward(from, stride),
            Command::Step {
                direction: Direction::Back,
                stride,
            } => self.back(from, stride),
            Command::Resume => {
                let first = usize::from(self.breakpoint_at(from, breakpoints).is_some()) + from;
                (first..end)
                    .find(|ahead| self.breakpoint_at(*ahead, breakpoints).is_some())
                    .unwrap_or(end)
            }
        };
        Landing {
            at: landed,
            halt: self.halt_at(landed, breakpoints),
        }
    }

    /// The depth of the frame control is in at `at`.
    ///
    /// Past the end that is the last step's depth: the run is over, and the
    /// frame it ended in is the one a backwards stride is leaving.
    fn frame_depth(&self, at: usize) -> usize {
        self.depth(at)
            .or_else(|| self.depth(self.len().checked_sub(1)?))
            .unwrap_or(0)
    }

    fn forward(&self, at: usize, stride: Stride) -> usize {
        let end = self.len();
        if at >= end {
            return end;
        }
        let here = self.depth(at).unwrap_or(0);
        let past = |deeper_than: usize| {
            (at + 1..end)
                .find(|ahead| self.depth(*ahead).unwrap_or(0) <= deeper_than)
                .unwrap_or(end)
        };
        match stride {
            Stride::Into => at + 1,
            Stride::Over => past(here),
            // One shallower than `here` is what "out of this frame" means, and
            // at the outermost frame there is nothing shallower — so stepping
            // out of the top runs to the end, which is what every debugger
            // does.
            Stride::Out => here.checked_sub(1).map_or(end, past),
        }
    }

    fn back(&self, at: usize, stride: Stride) -> usize {
        if at == 0 {
            return 0;
        }
        let here = self.frame_depth(at);
        let before = |deeper_than: usize| {
            (0..at)
                .rev()
                .find(|behind| self.depth(*behind).unwrap_or(0) <= deeper_than)
                .unwrap_or(0)
        };
        match stride {
            Stride::Into => at - 1,
            Stride::Over => before(here),
            Stride::Out => here.checked_sub(1).map_or(0, before),
        }
    }

    fn breakpoint_at<'a>(&self, at: usize, breakpoints: &'a Breakpoints) -> Option<&'a NodeSite> {
        let step = self.step(at)?;
        breakpoints.stops_at(self.host(at)?, step.node, &step.instance)
    }

    fn halt_at(&self, at: usize, breakpoints: &Breakpoints) -> Halt {
        if let Some(site) = self.breakpoint_at(at, breakpoints) {
            let step = &self.run.steps()[at];
            return Halt::AtBreakpoint {
                site: site.clone(),
                instance: step.instance.clone(),
                node: step.node,
            };
        }
        if at >= self.len() {
            return Halt::Ended(self.run.stop());
        }
        if at == 0 {
            return Halt::Entry;
        }
        Halt::Stepped
    }
}

/// Where a debug session stands, and what it is watching (R1644).
///
/// A **value**: the breakpoints, the watches and the position, serializable
/// together and holding no reference to a document. So a debugging setup is a
/// thing to save, hand to a colleague, or attach to a bug report — where the
/// reference keeps its breakpoints in the asset and its position nowhere at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    tree: TreeId,
    entry: NodeId,
    budget: usize,
    at: usize,
    breakpoints: Breakpoints,
    watches: Watches,
}

impl Session {
    /// A session at the entry, with nothing armed and nothing watched.
    ///
    /// `budget` bounds the run for the reason
    /// [`Document::run`](crate::Document::run) takes one: a control loop is a
    /// legal graph, so a run need not terminate.
    #[must_use]
    pub const fn new(tree: TreeId, entry: NodeId, budget: usize) -> Self {
        Self {
            tree,
            entry,
            budget,
            at: 0,
            breakpoints: Breakpoints::new(),
            watches: Watches::new(),
        }
    }

    /// The tree being run.
    #[must_use]
    pub const fn tree(&self) -> TreeId {
        self.tree
    }

    /// The node the run begins at.
    #[must_use]
    pub const fn entry(&self) -> NodeId {
        self.entry
    }

    /// How many steps the run is allowed.
    #[must_use]
    pub const fn budget(&self) -> usize {
        self.budget
    }

    /// Change how many steps the run is allowed.
    pub const fn set_budget(&mut self, budget: usize) {
        self.budget = budget;
    }

    /// How many steps have been taken.
    #[must_use]
    pub const fn at(&self) -> usize {
        self.at
    }

    /// The breakpoints.
    #[must_use]
    pub const fn breakpoints(&self) -> &Breakpoints {
        &self.breakpoints
    }

    /// The breakpoints, to disarm or to enable. Arming one is
    /// [`Document::set_breakpoint`], which is the checked way in.
    pub const fn breakpoints_mut(&mut self) -> &mut Breakpoints {
        &mut self.breakpoints
    }

    /// The watches.
    #[must_use]
    pub const fn watches(&self) -> &Watches {
        &self.watches
    }

    /// The watches, to drop. Adding one is [`Document::set_watch`].
    pub const fn watches_mut(&mut self) -> &mut Watches {
        &mut self.watches
    }
}

/// A run, stopped somewhere (R1644).
#[derive(Debug, Clone, PartialEq)]
pub struct Paused<K: NodeKind> {
    timeline: Timeline<K>,
    at: usize,
    halt: Halt,
}

impl<K: NodeKind> Paused<K> {
    /// How many steps have been taken.
    #[must_use]
    pub const fn at(&self) -> usize {
        self.at
    }

    /// Why it is here.
    #[must_use]
    pub const fn halt(&self) -> &Halt {
        &self.halt
    }

    /// The whole run this is a position in.
    #[must_use]
    pub const fn timeline(&self) -> &Timeline<K> {
        &self.timeline
    }

    /// The steps actually taken, in order.
    #[must_use]
    pub fn taken(&self) -> &[Step<K>] {
        &self.timeline.run.steps()[..self.at.min(self.timeline.len())]
    }

    /// The step **about to** run, or `None` at the end.
    ///
    /// A prediction, and an exact one: a run is a pure function of the document
    /// and the registers, so what this node will compute is not a guess. No
    /// debugger over a mutable-state graph can answer this at all.
    #[must_use]
    pub fn next(&self) -> Option<&Step<K>> {
        self.timeline.step(self.at)
    }

    /// The call stack here, outermost first: the group instances control has
    /// descended through and not yet left.
    #[must_use]
    pub fn stack(&self) -> &[(TreeId, NodeId)] {
        self.next()
            .or_else(|| self.timeline.step(self.timeline.len().checked_sub(1)?))
            .map_or(&[], |step| step.instance.path())
    }

    /// Every watched port's value, per occurrence.
    #[must_use]
    pub fn readings(&self) -> &[Reading<K>] {
        self.timeline.readings()
    }
}

// ------------------------------------------------------------------- document

impl<K: NodeKind> Document<K> {
    /// Every occurrence of `tree` — each chain of group instances that lands in
    /// it, ascending (R1644).
    ///
    /// The root tree has exactly one, the empty chain. A definition nothing
    /// instantiates has **none**, which is the honest answer and the reason a
    /// watch in it reports nothing.
    ///
    /// Bounded by the same nesting cap [`Self::tick`] uses, for the same
    /// reason: a document that arrived from a file may have a containment cycle
    /// in it, and walking one forever inside the process doing the validating is
    /// not a diagnosis.
    #[must_use]
    pub fn occurrences(&self, tree: TreeId) -> Vec<Instance> {
        let mut found = Vec::new();
        let mut pending = vec![(ROOT, Instance::root())];
        while let Some((host, instance)) = pending.pop() {
            if instance.depth() > NESTING_LIMIT {
                continue;
            }
            if host == tree {
                found.push(instance.clone());
            }
            let Some(held) = self.tree(host) else {
                continue;
            };
            let mut deeper: Vec<(NodeId, TreeId)> = held
                .nodes()
                .filter_map(|node| match node.body {
                    NodeBody::Group(definition) => Some((node.id, definition)),
                    _ => None,
                })
                .collect();
            deeper.sort_unstable();
            for (node, definition) in deeper {
                pending.push((definition, instance.inside(host, node)));
            }
        }
        found.sort();
        found
    }

    /// Arm a breakpoint, answering whether it was newly armed (R1644).
    ///
    /// Hangs off the document, and not off [`Breakpoints`], for the reason
    /// [`Self::force`] does: every check worth making is a fact only the
    /// document has. Arming one that is already there leaves its enabled flag
    /// alone — re-arming is not a way to re-enable, because those are separate
    /// commands in every debugger there is.
    ///
    /// # Errors
    ///
    /// [`BreakError`].
    pub fn set_breakpoint(
        &self,
        points: &mut Breakpoints,
        site: NodeSite,
    ) -> Result<bool, BreakError> {
        self.check_node_site(&site)?;
        if points.armed.contains_key(&site) {
            return Ok(false);
        }
        points.armed.insert(site, true);
        Ok(true)
    }

    /// Arm a breakpoint if there is none there, disarm it if there is —
    /// answering whether one is there **now** (R1644).
    ///
    /// Presence, not the enabled flag. The reference draws the same line, and
    /// the two are easy to conflate: its `ToggleBreakpoint` creates or removes,
    /// while its enable and disable commands move `bEnabled` on one that stays.
    ///
    /// # Errors
    ///
    /// [`BreakError`] — and only when there is nothing there to remove, since
    /// removing needs no claim about the document.
    pub fn toggle_breakpoint(
        &self,
        points: &mut Breakpoints,
        site: NodeSite,
    ) -> Result<bool, BreakError> {
        if points.disarm(&site) {
            return Ok(false);
        }
        self.set_breakpoint(points, site)
    }

    /// Watch a port, answering whether it was newly watched (R1644).
    ///
    /// # Errors
    ///
    /// [`WatchError`].
    pub fn set_watch(&self, on: &mut Watches, site: PortSite) -> Result<bool, WatchError> {
        self.check_port_site(&site)?;
        Ok(on.on.insert(site))
    }

    /// Watch a port if it is not watched, drop it if it is — answering whether
    /// it is watched **now** (R1644).
    ///
    /// # Errors
    ///
    /// [`WatchError`], and only when there is nothing there to drop.
    pub fn toggle_watch(&self, on: &mut Watches, site: PortSite) -> Result<bool, WatchError> {
        if on.unwatch(&site) {
            return Ok(false);
        }
        self.set_watch(on, site)
    }

    /// Every armed breakpoint this document no longer supports, and **why**
    /// (R1644).
    ///
    /// A document is editable while it is being debugged, so a node under a
    /// breakpoint can be deleted, or have its kind changed to one with no
    /// control port. Reported rather than dropped: a debugger that silently
    /// forgets the place a person marked is worse than one that says the mark
    /// no longer holds. The reference asks the same question one breakpoint at a
    /// time and answers `bool`.
    #[must_use]
    pub fn stale_breakpoints(&self, points: &Breakpoints) -> Vec<(NodeSite, BreakError)> {
        points
            .iter()
            .filter_map(|(site, _)| {
                self.check_node_site(site)
                    .err()
                    .map(|why| (site.clone(), why))
            })
            .collect()
    }

    /// Every watched port this document no longer supports, and why (R1644).
    #[must_use]
    pub fn stale_watches(&self, on: &Watches) -> Vec<(PortSite, WatchError)> {
        on.iter()
            .filter_map(|site| {
                self.check_port_site(site)
                    .err()
                    .map(|why| (site.clone(), why))
            })
            .collect()
    }

    /// The whole run, with the watched values (R1644).
    ///
    /// The breakpoints are **not** an input. That is the design: see this
    /// module's header.
    ///
    /// # Errors
    ///
    /// [`RunError`], from the run itself.
    pub fn timeline(&self, session: &Session, state: &Machine<K>) -> Result<Timeline<K>, RunError> {
        let run = self.run_on(session.tree, session.entry, session.budget, state)?;
        let hosts = run
            .steps()
            .iter()
            .map(|step| self.tree_of(&step.instance).unwrap_or(session.tree))
            .collect();
        let readings = self.read_watches(&session.watches, state, &run);
        Ok(Timeline {
            run,
            hosts,
            readings,
        })
    }

    /// Where the session stands, taking no step (R1644).
    ///
    /// # Errors
    ///
    /// [`RunError`].
    pub fn paused(&self, session: &Session, state: &Machine<K>) -> Result<Paused<K>, RunError> {
        let timeline = self.timeline(session, state)?;
        let at = session.at.min(timeline.len());
        let halt = timeline.halt_at(at, &session.breakpoints);
        Ok(Paused { timeline, at, halt })
    }

    /// Apply one debugger command, and answer where it stopped (R1644).
    ///
    /// The session's position is **clamped** to the run: a document edited mid
    /// session can shorten its own run, and a position past the end is a
    /// position the next command corrects rather than an error, because the
    /// document is allowed to change and the debugger is not the authority on
    /// it.
    ///
    /// # Errors
    ///
    /// [`RunError`].
    pub fn debug(
        &self,
        session: &mut Session,
        state: &Machine<K>,
        command: Command,
    ) -> Result<Paused<K>, RunError> {
        let timeline = self.timeline(session, state)?;
        let landing = timeline.seek(session.at, command, &session.breakpoints);
        session.at = landing.at;
        Ok(Paused {
            timeline,
            at: landing.at,
            halt: landing.halt,
        })
    }

    /// What every watch holds, in every occurrence the document has.
    ///
    /// The walk is [`Self::tick`]'s: descend every group instance from the root,
    /// at every depth. A reading is taken through the same
    /// [`Evaluator`](crate::Evaluator) the run reads through, so a watched value
    /// and the value the graph acted on cannot disagree.
    fn read_watches(&self, watches: &Watches, state: &Machine<K>, run: &Run<K>) -> Vec<Reading<K>> {
        let mut out: Vec<Reading<K>> = Vec::new();
        if watches.is_empty() {
            return out;
        }
        let mut evaluator = self.evaluator_on(state);
        let mut pending = vec![evaluator.root(ROOT)];
        while let Some(descent) = pending.pop() {
            if descent.instance().depth() > NESTING_LIMIT {
                continue;
            }
            let here = descent.tree();
            for site in watches
                .iter()
                .filter(|site| site.tree == here && site.occurrence.admits(descent.instance()))
            {
                let at = site.port.index as usize;
                let value = match site.port.side {
                    Side::Input => evaluator.inputs_in(&descent, site.node),
                    Side::Output => evaluator.outputs_in(&descent, site.node),
                }
                .into_iter()
                .nth(at)
                .flatten();
                out.push(Reading {
                    site: site.clone(),
                    instance: descent.instance().clone(),
                    value,
                    ran_at: run.steps().iter().position(|step| {
                        step.node == site.node && &step.instance == descent.instance()
                    }),
                });
            }
            let Some(held) = self.tree(here) else {
                continue;
            };
            let mut deeper: Vec<(NodeId, TreeId)> = held
                .nodes()
                .filter_map(|node| match node.body {
                    NodeBody::Group(definition) => Some((node.id, definition)),
                    _ => None,
                })
                .collect();
            deeper.sort_unstable();
            for (node, definition) in deeper {
                pending.push(evaluator.enter(&descent, node, definition));
            }
        }
        out.sort_by(|a, b| (&a.site, &a.instance).cmp(&(&b.site, &b.instance)));
        out
    }

    /// Whether a run could ever arrive at this site.
    fn check_node_site(&self, site: &NodeSite) -> Result<(), BreakError> {
        self.check_occurrence(site.tree, &site.occurrence)
            .map_err(|why| match why {
                SiteError::NoSuchTree => BreakError::NoSuchTree(site.tree),
                SiteError::NoSuchInstance => BreakError::NoSuchInstance,
                SiteError::Elsewhere(occurrence) => BreakError::InstanceIsElsewhere {
                    site: site.tree,
                    occurrence,
                },
            })?;
        let signature = self
            .signature(site.tree, site.node)
            .ok_or(BreakError::NoSuchNode {
                tree: site.tree,
                node: site.node,
            })?;
        if matches!(
            self.tree(site.tree)
                .and_then(|held| held.node(site.node))
                .map(|held| &held.body),
            Some(NodeBody::Group(_))
        ) {
            return Err(BreakError::IsAnInstance {
                tree: site.tree,
                node: site.node,
            });
        }
        if !signature.inputs.iter().any(Port::is_control)
            && !signature.outputs.iter().any(Port::is_control)
        {
            return Err(BreakError::NotOnTheControlPlane {
                tree: site.tree,
                node: site.node,
            });
        }
        Ok(())
    }

    /// Whether this port exists and carries a value.
    fn check_port_site(&self, site: &PortSite) -> Result<(), WatchError> {
        self.check_occurrence(site.tree, &site.occurrence)
            .map_err(|why| match why {
                SiteError::NoSuchTree => WatchError::NoSuchTree(site.tree),
                SiteError::NoSuchInstance => WatchError::NoSuchInstance,
                SiteError::Elsewhere(occurrence) => WatchError::InstanceIsElsewhere {
                    site: site.tree,
                    occurrence,
                },
            })?;
        let signature = self
            .signature(site.tree, site.node)
            .ok_or(WatchError::NoSuchNode {
                tree: site.tree,
                node: site.node,
            })?;
        let ports = match site.port.side {
            Side::Input => &signature.inputs,
            Side::Output => &signature.outputs,
        };
        let port = ports
            .get(site.port.index as usize)
            .ok_or(WatchError::NoSuchPort {
                tree: site.tree,
                node: site.node,
                port: site.port,
            })?;
        if port.is_control() {
            return Err(WatchError::NotAValue {
                tree: site.tree,
                node: site.node,
                port: site.port,
            });
        }
        // ★★★★★ R1942 — and then what the TAXONOMY says about the type. Asked
        // after control, because control is not a value by construction and a
        // taxonomy has no say in that: reporting the narrower refusal about it
        // would name a rule that did not apply.
        if let Some(ty) = port.value_type() {
            if let crate::Inspectable::No(why) = K::inspectable(ty) {
                return Err(WatchError::NotInspectable {
                    tree: site.tree,
                    node: site.node,
                    port: site.port,
                    why,
                });
            }
        }
        Ok(())
    }

    /// The half of the check both addresses share: the tree is here, and the
    /// occurrence — if one was named — resolves to that tree.
    fn check_occurrence(&self, tree: TreeId, occurrence: &Occurrence) -> Result<(), SiteError> {
        if self.tree(tree).is_none() {
            return Err(SiteError::NoSuchTree);
        }
        if let Occurrence::At(instance) = occurrence {
            match self.tree_of(instance) {
                None => return Err(SiteError::NoSuchInstance),
                Some(lands) if lands != tree => return Err(SiteError::Elsewhere(lands)),
                Some(_) => {}
            }
        }
        Ok(())
    }
}

/// The shared half of the two address checks, before each names it in its own
/// vocabulary.
///
/// Private: a caller sees [`BreakError`] or [`WatchError`], never this. It
/// exists so the two checks cannot drift on the part that is the same question,
/// which is the shape R1643 found five copies of.
enum SiteError {
    NoSuchTree,
    NoSuchInstance,
    Elsewhere(TreeId),
}
