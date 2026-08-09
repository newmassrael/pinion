//! R1600 — what a running graph remembers.
//!
//! [`Document::evaluate`](crate::Document::evaluate) answers what a node's value
//! is and [`Document::run`](crate::Document::run) answers which nodes run; both
//! are pure reads of the document. This is the third question — *what does the
//! graph carry from one step to the next* — and it is the one that cannot be
//! answered by reading the document alone, because the answer is not in it.
//!
//! # Why a graph needs to remember at all
//!
//! R1599 made a control **loop** authorable, and left the reason it could not
//! yet mean anything: [`NodeKind::evaluate`] is a
//! pure function of the resolved inputs, so a node that ran twice computed the
//! same thing twice. Nothing accumulates, nothing counts, nothing converges. A
//! second pass differs only if some value is **one step behind**, and that is
//! [`NodeBody::Delay`] — Lustre's `pre`, Simulink's Unit Delay, SSA's φ at a
//! loop header, a hardware register.
//!
//! # State is a delay, and nothing else
//!
//! That is the deliberate limit and it is what buys everything below. A tick's
//! result is a function of the registers and the document; run the same
//! document from the same [`Machine`] and the same thing happens. The engine
//! takes the other road — state there is a visual script *variable*, arbitrary
//! mutable memory written by a variable set node — so which value a read sees
//! depends on where the execution wire happened to go, and the graph's meaning
//! is not a function of the graph.
//!
//! # Read, then commit
//!
//! Every register advances **together**: each delay's next value is computed
//! against the registers as they were, and only then are any written. Without
//! that, two delays feeding each other would both take whichever value the
//! walk happened to reach first, so the graph's meaning would depend on node
//! ordering; with it, they swap, which is what a pair of registers does.
//!
//! The separation is not a convention here — [`Document::tick`] holds the
//! machine by `&mut`, so the [`Evaluator`](crate::Evaluator) it reads through
//! (which borrows the machine immutably) **cannot** coexist with a write. The
//! borrow checker is what enforces the simultaneity.
//!
//! # The register's address
//!
//! An [`Instance`], not a [`NodeId`]. A definition's tree is shared by every
//! instance of it, so two instances of one counting group must count
//! separately — the same key the evaluator's memo has used since R1577, rather
//! than a second convention that could disagree with it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::model::{Document, Instance, NodeBody, NodeId, NodeKind, TreeId};

/// How deep a nesting of group instances [`Document::tick`] will walk.
///
/// A document this crate built cannot exceed it: [`Document::containment`] is
/// kept acyclic, so the nesting depth is bounded by the number of trees. This
/// is for the other kind — one that arrived from a file with a containment
/// cycle in it, which [`Document::validate`] reports and which would otherwise
/// walk forever inside the process doing the validating.
const NESTING_LIMIT: usize = 512;

/// The registers of a running document (R1600).
///
/// One value per [`NodeBody::Delay`] per [`Instance`], plus how many ticks have
/// been taken. It holds no reference to the document, so it can be saved,
/// restored, compared and sent — which is what makes a scenario reproducible
/// rather than merely repeatable.
///
/// A register with no entry is not an error: it means nothing has been
/// committed there yet, and the delay reads its authored initial value
/// ([`Node::values`](crate::Node::values), R1594 — Lustre's `->`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K::Value: Serialize + Clone",
    deserialize = "K::Value: Deserialize<'de>"
))]
pub struct Machine<K: NodeKind> {
    #[serde(with = "register_rows")]
    held: BTreeMap<Instance, BTreeMap<NodeId, K::Value>>,
    ticks: usize,
}

/// A machine's registers travel as **rows**, not as a map of maps.
///
/// The same argument [`Node`](crate::Node)'s own wire form makes one level up:
/// a row carries its whole address, so nothing can arrive with a key that
/// disagrees with what is under it. Here it is also forced — a register is
/// addressed by an [`Instance`] and a [`NodeId`], neither of which is a string,
/// and a map keyed by either is not expressible in JSON at all. Found by
/// round-tripping a machine in a test rather than by reading the derive.
mod register_rows {
    use super::{Instance, NodeId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize)]
    #[serde(bound(serialize = "V: Serialize", deserialize = "V: Deserialize<'de>"))]
    struct Row<V> {
        instance: Instance,
        node: NodeId,
        value: V,
    }

    pub(super) fn serialize<V, S>(
        held: &BTreeMap<Instance, BTreeMap<NodeId, V>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        V: Serialize + Clone,
        S: Serializer,
    {
        held.iter()
            .flat_map(|(instance, per_node)| {
                per_node.iter().map(move |(node, value)| Row {
                    instance: instance.clone(),
                    node: *node,
                    value: value.clone(),
                })
            })
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub(super) fn deserialize<'de, V, D>(
        deserializer: D,
    ) -> Result<BTreeMap<Instance, BTreeMap<NodeId, V>>, D::Error>
    where
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        let mut held: BTreeMap<Instance, BTreeMap<NodeId, V>> = BTreeMap::new();
        for row in Vec::<Row<V>>::deserialize(deserializer)? {
            held.entry(row.instance)
                .or_default()
                .insert(row.node, row.value);
        }
        Ok(held)
    }
}

impl<K: NodeKind> Default for Machine<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: NodeKind> Machine<K> {
    /// A machine holding nothing, at tick zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            held: BTreeMap::new(),
            ticks: 0,
        }
    }

    /// How many ticks have been taken.
    #[must_use]
    pub const fn ticks(&self) -> usize {
        self.ticks
    }

    /// What this instance's `node` is holding, or `None` when nothing has been
    /// committed there.
    #[must_use]
    pub fn read(&self, instance: &Instance, node: NodeId) -> Option<&K::Value> {
        self.held.get(instance)?.get(&node)
    }

    /// How many registers are holding a value.
    #[must_use]
    pub fn len(&self) -> usize {
        self.held.values().map(BTreeMap::len).sum()
    }

    /// Whether nothing is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.held.values().all(BTreeMap::is_empty)
    }

    /// Every register holding a value, ascending by instance then node.
    ///
    /// The whole of the machine's memory, enumerable. The DCC's equivalent —
    /// the geometry-nodes simulation cache — is a private
    /// `Map<int, std::unique_ptr<SimulationNodeCache>>` on the modifier, keyed
    /// by a flattened nested-node id, with no accessor that walks it.
    pub fn iter(&self) -> impl Iterator<Item = (&Instance, NodeId, &K::Value)> {
        self.held.iter().flat_map(|(instance, per_node)| {
            per_node
                .iter()
                .map(move |(node, value)| (instance, *node, value))
        })
    }

    /// Write a register directly. **Crate-private** — see
    /// [`Document::force`], which is the checked way in.
    ///
    /// Unchecked on purpose and therefore not public: a [`Machine`] holds no
    /// reference to a document, so it cannot know that `node` is a delay or what
    /// type that delay holds. Only the document knows, so the check lives there.
    pub(crate) fn write(
        &mut self,
        instance: Instance,
        node: NodeId,
        value: K::Value,
    ) -> Option<K::Value> {
        self.held.entry(instance).or_default().insert(node, value)
    }

    /// Forget everything and go back to tick zero.
    ///
    /// Distinct from building a new machine only in that it says so at the call
    /// site: a scenario's *reset* is a thing an operator does, and a scenario
    /// that restarts by dropping its machine somewhere else is a scenario whose
    /// restart is not in the trace.
    pub fn reset(&mut self) {
        self.held.clear();
        self.ticks = 0;
    }
}

/// One register's advance (R1600).
#[derive(Debug, Clone, PartialEq)]
pub struct Committed<K: NodeKind> {
    /// Which occurrence of the delay.
    pub instance: Instance,
    /// The delay node.
    pub node: NodeId,
    /// What it was holding, if anything.
    pub before: Option<K::Value>,
    /// What it holds now — `None` when nothing arrived at its input, which
    /// clears the register rather than keeping a value the graph no longer
    /// produces.
    pub after: Option<K::Value>,
}

impl<K: NodeKind> Committed<K> {
    /// Whether this advance changed anything.
    ///
    /// A tick where nothing changed is a **fixed point**, which is the fact a
    /// converging scenario is waiting for.
    #[must_use]
    pub fn moved(&self) -> bool {
        self.before != self.after
    }
}

/// What one [`Document::tick`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick<K: NodeKind> {
    at: usize,
    committed: Vec<Committed<K>>,
    dropped: Vec<(Instance, NodeId)>,
    truncated: bool,
}

impl<K: NodeKind> Tick<K> {
    /// The tick number this produced: 1 for the first.
    #[must_use]
    pub const fn at(&self) -> usize {
        self.at
    }

    /// Every register that advanced, ascending by instance then node.
    #[must_use]
    pub fn committed(&self) -> &[Committed<K>] {
        &self.committed
    }

    /// How many registers actually changed value.
    ///
    /// Zero means the machine has reached a **fixed point**: ticking again
    /// changes nothing, so a scenario waiting for one is done. Answerable
    /// because every register advanced in the same tick — a per-node "has this
    /// settled" flag could not be compared across nodes that advanced at
    /// different moments.
    #[must_use]
    pub fn changed(&self) -> usize {
        self.committed.iter().filter(|one| one.moved()).count()
    }

    /// Registers that were being held for a delay this walk did not reach —
    /// dropped by this tick, and named rather than dropped silently.
    ///
    /// A document is editable while it runs, so a delay can be deleted, or a
    /// group instance removed, while a value is being held for it. The walk is
    /// the authority on which registers exist; anything it did not reach is
    /// memory for a node that is not there, and keeping it would make a machine
    /// grow forever across an editing session.
    ///
    /// Always empty when [`Self::truncated`] is set: an incomplete walk is not
    /// evidence of absence.
    #[must_use]
    pub fn dropped(&self) -> &[(Instance, NodeId)] {
        &self.dropped
    }

    /// Whether the nesting walk hit its depth cap and stopped early, so
    /// some instances were not advanced.
    ///
    /// Reachable only by a document [`Document::validate`] already condemns.
    /// Published rather than folded into a short list, because "nothing else to
    /// advance" and "we stopped looking" are different facts.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

impl<K: NodeKind> Document<K> {
    /// Advance every register once, and answer what moved (R1600).
    ///
    /// Each [`NodeBody::Delay`] reachable from `tree` — inside every instance of
    /// every group, at every depth — takes the value now arriving at its input,
    /// and the whole set is written **after** the whole set is read. See this
    /// module's header for why that is the only defensible order and for how the
    /// borrow checker is what holds it.
    ///
    /// Ticking a document with no delays in it is legal and does nothing but
    /// advance the count, which is the honest answer rather than an error: a
    /// scenario is allowed to have no state yet.
    pub fn tick(&self, tree: TreeId, state: &mut Machine<K>) -> Tick<K> {
        let mut arriving: Vec<(Instance, NodeId, Option<K::Value>)> = Vec::new();
        let mut reached: BTreeSet<(Instance, NodeId)> = BTreeSet::new();
        let mut truncated = false;

        // The read half. Scoped, because the evaluator borrows `state` — so
        // the compiler will not let a single register be written while another
        // is still being read.
        {
            let mut evaluator = self.evaluator_on(state);
            let mut pending = vec![evaluator.root(tree)];
            while let Some(descent) = pending.pop() {
                if descent.instance().depth() > NESTING_LIMIT {
                    truncated = true;
                    continue;
                }
                let Some(host) = self.tree(descent.tree()) else {
                    continue;
                };
                let mut delays: Vec<NodeId> = Vec::new();
                let mut groups: Vec<(NodeId, TreeId)> = Vec::new();
                for held in host.nodes() {
                    match &held.body {
                        NodeBody::Delay(_) => delays.push(held.id),
                        NodeBody::Group(definition) => groups.push((held.id, *definition)),
                        _ => {}
                    }
                }
                delays.sort_unstable();
                groups.sort_unstable();
                for node in delays {
                    // Port 0 — a delay's signature is derived, so there is
                    // exactly one input and it is the value being remembered.
                    let next = evaluator
                        .inputs_in(&descent, node)
                        .into_iter()
                        .next()
                        .flatten();
                    reached.insert((descent.instance().clone(), node));
                    arriving.push((descent.instance().clone(), node, next));
                }
                for (node, definition) in groups {
                    pending.push(evaluator.enter(&descent, node, definition));
                }
            }
        }

        // The write half.
        state.ticks += 1;
        let mut committed: Vec<Committed<K>> = Vec::new();
        for (instance, node, after) in arriving {
            let per_node = state.held.entry(instance.clone()).or_default();
            let before = match after.clone() {
                Some(value) => per_node.insert(node, value),
                None => per_node.remove(&node),
            };
            committed.push(Committed {
                instance,
                node,
                before,
                after,
            });
        }
        committed.sort_by(|a, b| (&a.instance, a.node).cmp(&(&b.instance, b.node)));

        let mut dropped: Vec<(Instance, NodeId)> = Vec::new();
        if !truncated {
            for (instance, per_node) in &state.held {
                for node in per_node.keys() {
                    if !reached.contains(&(instance.clone(), *node)) {
                        dropped.push((instance.clone(), *node));
                    }
                }
            }
            for (instance, node) in &dropped {
                if let Some(per_node) = state.held.get_mut(instance) {
                    per_node.remove(node);
                }
            }
        }
        state.held.retain(|_, per_node| !per_node.is_empty());

        Tick {
            at: state.ticks,
            committed,
            dropped,
            truncated,
        }
    }

    /// Write one register directly, answering what was there (R1600, checked
    /// R1601.1).
    ///
    /// **Forcing**, in the hardware and Simulink sense: not something the graph
    /// does, but something an operator or a debugger does *to* it — restore a
    /// saved scenario mid-run, drive a branch down the arm you want to see,
    /// reproduce a state a capture caught. Named for stepping outside the tick
    /// discipline rather than for setting a value, because that is the fact
    /// worth seeing at the call site: [`Self::tick`] advances every register at
    /// one instant, and this one does not.
    ///
    /// It hangs off the **document** rather than off the machine because the
    /// three things worth checking are all facts the machine cannot know: that
    /// the instance names a tree, that the node there is a
    /// [`NodeBody::Delay`], and that the value is one that delay can hold.
    /// R1601.1 moved it here after finding the alternative's doc claiming
    /// [`Self::validate`] would report a mistyped forced value — it cannot,
    /// because `validate` reads a document and a forced value is not in one.
    ///
    /// # Errors
    ///
    /// [`ForceError`].
    pub fn force(
        &self,
        state: &mut Machine<K>,
        instance: &Instance,
        node: NodeId,
        value: K::Value,
    ) -> Result<Option<K::Value>, ForceError> {
        let tree = self.tree_of(instance).ok_or(ForceError::NoSuchInstance)?;
        let held = self
            .tree(tree)
            .and_then(|host| host.node(node))
            .ok_or(ForceError::NoSuchNode { tree, node })?;
        let NodeBody::Delay(ty) = &held.body else {
            return Err(ForceError::NotARegister { tree, node });
        };
        // The same gate `set_port_value` applies to an authored value, and for
        // the same reason: a taxonomy that declines to classify its values
        // (`value_type` answers `None`) is not held to a rule it cannot state.
        if let Some(found) = K::value_type(&value)
            && &found != ty
        {
            return Err(ForceError::WrongType { tree, node });
        }
        Ok(state.write(instance.clone(), node, value))
    }

    /// Which tree an instance path lands in, or `None` if it names a node or a
    /// definition this document does not have.
    ///
    /// The one place an [`Instance`] built by hand is checked against the
    /// document. [`Descent`](crate::Descent) instances are built by the walk and
    /// cannot be wrong; one handed in from outside can be.
    fn tree_of(&self, instance: &Instance) -> Option<TreeId> {
        let mut tree = crate::model::ROOT;
        for (host, node) in instance.path() {
            if *host != tree {
                return None;
            }
            match self.tree(tree)?.node(*node)?.body {
                NodeBody::Group(definition) => tree = definition,
                _ => return None,
            }
        }
        Some(tree)
    }

    /// Tick until nothing changes, or `budget` ticks have been taken (R1600).
    ///
    /// The **fixed point**: the reading a converging scenario is waiting for,
    /// and the shape a dataflow-with-state graph settles into. Answers every
    /// tick taken, so a caller that did not converge can see how far it got and
    /// what was still moving on the last one — `settle(..).last()` having a
    /// non-zero [`Tick::changed`] is exactly "did not converge".
    ///
    /// A budget rather than a loop, for the same reason
    /// [`Document::run`](crate::Document::run) takes one: a graph with a delay
    /// in a feedback loop need not converge at all, and how long is too long is
    /// the caller's question.
    pub fn settle(&self, tree: TreeId, state: &mut Machine<K>, budget: usize) -> Vec<Tick<K>> {
        let mut taken = Vec::new();
        for _ in 0..budget {
            let tick = self.tick(tree, state);
            let done = tick.changed() == 0;
            taken.push(tick);
            if done {
                break;
            }
        }
        taken
    }
}

/// Why a register could not be forced (R1601.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ForceError {
    /// The instance path does not name a chain of group instances in this
    /// document.
    NoSuchInstance,
    /// No such node in the tree that instance lands in.
    NoSuchNode {
        /// The tree the instance resolved to.
        tree: TreeId,
        /// The node that is not in it.
        node: NodeId,
    },
    /// The node is there and is not a [`NodeBody::Delay`], so it holds nothing
    /// between ticks and there is no register to write.
    NotARegister {
        /// The tree it is in.
        tree: TreeId,
        /// The node that is not a register.
        node: NodeId,
    },
    /// The taxonomy classifies its values and this one is not the type the
    /// delay holds.
    WrongType {
        /// The tree it is in.
        tree: TreeId,
        /// The register.
        node: NodeId,
    },
}

impl std::fmt::Display for ForceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchInstance => f.write_str("no such instance in this document"),
            Self::NoSuchNode { tree, node } => {
                write!(f, "tree {tree} has no node {node}")
            }
            Self::NotARegister { tree, node } => {
                write!(f, "node {node} in tree {tree} is not a register")
            }
            Self::WrongType { tree, node } => write!(
                f,
                "register {node} in tree {tree} cannot hold a value of that type"
            ),
        }
    }
}

impl std::error::Error for ForceError {}
