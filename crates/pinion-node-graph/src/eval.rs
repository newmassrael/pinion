//! R1577 — evaluation that descends into groups.
//!
//! The taxonomy computes; this module decides *what is asked of it, in what
//! order, and how often*. Three things make that more than a topological walk:
//!
//! * A group instance is not a computation. Evaluating one binds its resolved
//!   inputs to the definition's inside-input node and asks the definition's
//!   inside-output node what arrives — so one node's value is another whole
//!   graph's.
//! * The memo is keyed by **instance**, not by node. Two instances of one
//!   definition fed different values are two different results, and a memo keyed
//!   by `(tree, node)` would hand the second one the first one's answer. That is
//!   the single easiest thing to get wrong when adding groups to an evaluator
//!   that did not have them.
//! * Evaluating *inside* a definition is a real question — it is what a node
//!   editor shows while the user is in there — and it is answered with the
//!   interface's own port defaults standing in for the caller.

use std::collections::{BTreeMap, BTreeSet};

use crate::machine::Machine;
use crate::model::{
    Conversion, Document, Instance, InterfaceSide, NodeBody, NodeId, NodeKind, PortRef, Signature,
    Socket, TreeId, crossing,
};

/// How deep the walk may recurse before it gives up.
///
/// Reached only by a pathological document — a chain longer than any editor
/// draws — and the alternative to a cap is a stack overflow, which takes the
/// host process with it. Whether it was reached is published
/// ([`Evaluator::truncated`]) rather than folded into the `None`s it produces,
/// because "no value" and "we stopped looking" are different facts.
const DEPTH_LIMIT: usize = 512;

/// A memoised evaluation over one document.
///
/// Hold one across several reads — a sink, a hovered node, a debugger's
/// per-port readout — and the shared sub-graph is computed once. Drop it and the
/// memo goes; there is no cache to invalidate, which is the property that makes
/// this safe to call after an arbitrary edit.
pub struct Evaluator<'a, K: NodeKind> {
    document: &'a Document<K>,
    /// The registers every [`NodeBody::Delay`] reads (R1600), or `None` for a
    /// reading taken with no machine — where every delay carries its authored
    /// initial value, which is the reading at tick zero.
    state: Option<&'a Machine<K>>,
    memo: BTreeMap<(Instance, NodeId), Vec<Option<K::Value>>>,
    visiting: BTreeSet<(Instance, NodeId)>,
    truncated: bool,
}

impl<K: NodeKind> Document<K> {
    /// An evaluator sharing one memo across many reads.
    ///
    /// Every [`NodeBody::Delay`] reads its authored initial value, because
    /// there is no machine here to hold anything else — the reading at tick
    /// zero. Use [`Self::evaluator_on`] to read a document that is running.
    #[must_use]
    pub fn evaluator(&self) -> Evaluator<'_, K> {
        self.evaluator_with(None)
    }

    /// An evaluator that reads `state`'s registers (R1600).
    ///
    /// The same walk, with every [`NodeBody::Delay`] answering what the machine
    /// is holding for *that instance* rather than its initial value. Taking the
    /// machine by shared reference is the property that makes a reading
    /// side-effect free: only [`Self::tick`] advances anything.
    #[must_use]
    pub fn evaluator_on<'a>(&'a self, state: &'a Machine<K>) -> Evaluator<'a, K> {
        self.evaluator_with(Some(state))
    }

    /// The one constructor the two public forms are named arms of.
    pub(crate) fn evaluator_with<'a>(&'a self, state: Option<&'a Machine<K>>) -> Evaluator<'a, K> {
        Evaluator {
            document: self,
            state,
            memo: BTreeMap::new(),
            visiting: BTreeSet::new(),
            truncated: false,
        }
    }

    /// Every output of one node, from a fresh memo.
    ///
    /// The convenience form. Reading several nodes this way recomputes their
    /// shared upstream each time — use [`Self::evaluator`] for that.
    #[must_use]
    pub fn evaluate(&self, tree: TreeId, node: NodeId) -> Vec<Option<K::Value>> {
        self.evaluator().outputs(tree, node)
    }
}

impl<K: NodeKind> Evaluator<'_, K> {
    /// Every output of `node`, in port order.
    ///
    /// When `tree` is a definition, the evaluation runs as if an instance had
    /// been fed the interface's own port defaults.
    pub fn outputs(&mut self, tree: TreeId, node: NodeId) -> Vec<Option<K::Value>> {
        let descent = self.root_descent(tree);
        self.node_outputs(&descent, node, 0)
    }

    /// Every output of `node` **inside one instance** (R1600).
    ///
    /// [`Self::outputs`] is this at the root. A caller that has descended —
    /// [`Document::run`](crate::Document::run) following control into a group,
    /// [`Document::tick`](crate::Document::tick) finding the registers — asks
    /// here, so there is one descent rather than one per walk.
    pub fn outputs_in(&mut self, descent: &Descent<K>, node: NodeId) -> Vec<Option<K::Value>> {
        self.node_outputs(descent, node, 0)
    }

    /// Every resolved input of `node` inside one instance (R1600).
    pub fn inputs_in(&mut self, descent: &Descent<K>, node: NodeId) -> Vec<Option<K::Value>> {
        self.node_inputs(descent, node, 0)
    }

    /// The descent for reading `tree` on its own — no group entered.
    #[must_use]
    pub fn root(&self, tree: TreeId) -> Descent<K> {
        self.root_descent(tree)
    }

    /// One level in: the descent inside `node`, a group instance of
    /// `definition` sitting in `descent`'s tree (R1600).
    ///
    /// The bindings are **this instance's** resolved inputs, which is the whole
    /// reason a group cannot be run by walking its definition: two instances of
    /// one definition are fed differently, so what a node inside sees is a
    /// property of the instance and not of the tree it lives in.
    pub fn enter(&mut self, descent: &Descent<K>, node: NodeId, definition: TreeId) -> Descent<K> {
        let bindings = self.node_inputs(descent, node, 0);
        Descent {
            instance: descent.instance.inside(descent.tree, node),
            tree: definition,
            bindings,
        }
    }

    /// Every resolved input of `node`, in port order: what actually arrives,
    /// which is the wired source's output when there is one and the port's own
    /// default when there is not.
    pub fn inputs(&mut self, tree: TreeId, node: NodeId) -> Vec<Option<K::Value>> {
        let descent = self.root_descent(tree);
        self.node_inputs(&descent, node, 0)
    }

    /// One resolved input.
    pub fn input(&mut self, tree: TreeId, socket: Socket) -> Option<K::Value> {
        self.inputs(tree, socket.node)
            .into_iter()
            .nth(socket.port as usize)
            .flatten()
    }

    /// How many `(instance, node)` results the memo is holding.
    #[must_use]
    pub fn cached(&self) -> usize {
        self.memo.len()
    }

    /// Whether any walk hit the recursion cap and stopped early.
    ///
    /// The cap is a fixed depth reached only by a pathological document; the
    /// alternative to having one is a stack overflow, which takes the host
    /// process with it. "No value" and "we stopped looking" are different
    /// facts, so this is published rather than folded into the `None`s.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    /// The starting descent level for a tree read on its own.
    fn root_descent(&self, tree: TreeId) -> Descent<K> {
        let bindings = self.document.tree(tree).map_or_else(Vec::new, |t| {
            t.interface()
                .inputs()
                .iter()
                .map(|port| port.default_value().cloned())
                .collect()
        });
        Descent {
            instance: Instance::root(),
            tree,
            bindings,
        }
    }

    fn node_outputs(
        &mut self,
        descent: &Descent<K>,
        node: NodeId,
        depth: usize,
    ) -> Vec<Option<K::Value>> {
        let Some(signature) = self.document.signature(descent.tree, node) else {
            return Vec::new();
        };
        let arity = signature.outputs.len();
        let key = (descent.instance.clone(), node);
        if let Some(cached) = self.memo.get(&key) {
            return cached.clone();
        }
        if depth >= DEPTH_LIMIT {
            self.truncated = true;
            return vec![None; arity];
        }
        // A document this crate built is acyclic, so this only fires on one that
        // arrived from elsewhere — where the honest answer is "no value", not a
        // hang.
        if !self.visiting.insert(key.clone()) {
            return vec![None; arity];
        }

        // The signature resolved above, so the node is there.
        let resolved = self
            .document
            .tree(descent.tree)
            .and_then(|t| t.node(node))
            .map(|n| (n.body.clone(), n.bypassed, n.disabled));
        let Some((body, bypassed, disabled)) = resolved else {
            self.visiting.remove(&key);
            return vec![None; arity];
        };
        // ★★ R1682 — a DISABLED node is not there. Before the bypass arm and
        // before the body, because the request is stronger than either: its
        // inputs are not resolved (nothing runs, so nothing reads them), its
        // outputs are empty, and — unlike every other path out of this function
        // — the authored-value fallback below is skipped too. A node switched
        // off that still handed out its own constant would be running.
        if disabled {
            self.visiting.remove(&key);
            let empty = vec![None; arity];
            self.memo.insert(key, empty.clone());
            return empty;
        }
        // R1586 — a bypassed node does not compute. Its inputs are still
        // resolved (something has to pass through), and the routing is derived
        // from its signature rather than authored. This arm comes before the
        // body match on purpose: it is the same answer for an application kind
        // and for a group instance, and a group instance must NOT be descended
        // into here — bypassing one is exactly the request not to run it.
        let mut outputs = if bypassed {
            self.passed_through(descent, node, depth, &signature)
        } else {
            self.body_outputs(descent, node, depth, &signature, body)
        };
        outputs.resize(arity, None);
        // R1594 — the other half of one rule: an authored value is what a port
        // carries when nothing else supplies one. For an input that means no
        // link; for an output it means the kind produced nothing there, which
        // is what makes a SOURCE node's constant this same mechanism instead
        // of a second one. The DCC's Value node reads its own output socket in
        // per-node C code, so there the fact is a node type's private
        // arrangement rather than a rule.
        if !bypassed
            && let Some(held) = self
                .document
                .tree(descent.tree)
                .and_then(|host| host.node(node))
        {
            for (index, slot) in outputs.iter_mut().enumerate() {
                if slot.is_some() {
                    continue;
                }
                let port = PortRef::output(u32::try_from(index).unwrap_or(u32::MAX));
                *slot = held.port_value(port).cloned().or_else(|| {
                    signature
                        .outputs
                        .get(index)
                        .and_then(|declared| declared.default_value().cloned())
                });
            }
        }

        self.visiting.remove(&key);
        self.memo.insert(key, outputs.clone());
        outputs
    }

    /// What a node that is neither disabled nor bypassed produces, **by its
    /// body**.
    ///
    /// Lifted out of [`Self::node_outputs`] at R2004, when adding a body tipped
    /// that function past the length lint — the repair R1999 made to
    /// [`Document::validate`], for the same reason and with the same choice of
    /// cut: this match is one derivation, everything above it in the caller is
    /// the guards (memo, depth, cycle, disabled, bypassed) that decide whether
    /// to reach it at all, and everything below is R1594's authored-value
    /// fallback, which applies to the answer whatever produced it.
    fn body_outputs(
        &mut self,
        descent: &Descent<K>,
        node: NodeId,
        depth: usize,
        signature: &Signature<K>,
        body: NodeBody<K>,
    ) -> Vec<Option<K::Value>> {
        {
            match body {
                NodeBody::Kind(kind) => {
                    let inputs = self.node_inputs(descent, node, depth);
                    kind.evaluate(&inputs)
                }
                // The inside end of this tree's interface inputs: it produces
                // what the instance above was fed.
                NodeBody::Interface(InterfaceSide::Input) => descent.bindings.clone(),
                // THREE bodies that produce nothing, for three different
                // reasons: the inside end of the OUTPUTS has none of its own; a
                // frame (R1589) is a fact about the canvas with no ports at all;
                // and ★R2004 a stand-in is not on a path a value takes, because
                // `Document::expanded_links` resolves it away before any wiring
                // is walked. A caller asking any of them directly gets the
                // honest empty answer. One arm because the answer is one value;
                // the compiler still enumerates a new body here.
                NodeBody::Interface(InterfaceSide::Output)
                | NodeBody::Frame
                | NodeBody::StandIn(_) => Vec::new(),
                // R1600 — the one node whose output is not a function of its
                // input. It does not read its input here at all, which is what
                // breaks the recursion a value cycle would otherwise be: the
                // register is a source. An empty register falls through to the
                // authored-value block below, so a delay's *initial* value is
                // R1594's mechanism rather than a second one — Lustre's `->`.
                NodeBody::Delay(_) => {
                    vec![
                        self.state
                            .and_then(|held| held.read(&descent.instance, node))
                            .cloned(),
                    ]
                }
                // ★★★★★ R1934 — a reroute computes nothing: what arrives at its
                // input leaves by its output, which is exactly what
                // `passed_through` derives for a bypassed node. Routing it
                // through the SAME derivation rather than writing a one-line
                // identity here is what stops the two from drifting — and the
                // derivation is already general enough, because a reroute's
                // signature is one port each side of the same flow.
                //
                // The engine reaches the same result by deleting the node
                // before compilation (`ExpandNode` splices its two pin nets
                // together); the DCC's evaluator likewise never sees one.
                // R1935 — a beacon is transparent in exactly the same way, and
                // shares the arm.
                NodeBody::Reroute | NodeBody::Beacon => {
                    self.passed_through(descent, node, depth, signature)
                }
                // ★★★★★ R1935 — an echo has no input to pass through, so it
                // reads the beacon's OUTPUT directly. That is the value
                // crossing the canvas with no edge, and this is the one place
                // in the evaluator where a step is taken along something other
                // than a link.
                //
                // A cycle through a name is a cycle: if the beacon's own input
                // depends on this echo, the `visiting` set above stops the walk
                // and the honest answer is no value — the same guard that
                // catches a cyclic document from elsewhere. `Document::connect`
                // refuses to BUILD one (see `cuts_dependency`), so reaching
                // this needs a document this crate did not make.
                //
                // A dangling echo answers no value rather than panicking: the
                // beacon it names is gone, and `Document::validate` is where
                // that is reported as the defect it is.
                NodeBody::Echo(beacon) => {
                    let mut from = self.node_outputs(descent, beacon, depth + 1);
                    vec![if from.is_empty() {
                        None
                    } else {
                        from.swap_remove(0)
                    }]
                }
                NodeBody::Group(definition) => {
                    let bindings = self.node_inputs(descent, node, depth);
                    let inner = Descent {
                        instance: descent.instance.inside(descent.tree, node),
                        tree: definition,
                        bindings,
                    };
                    match self
                        .document
                        .tree(definition)
                        .and_then(|t| t.interface_node(InterfaceSide::Output))
                        .map(|n| n.id)
                    {
                        Some(exit) => self.node_inputs(&inner, exit, depth + 1),
                        // A definition with no inside-output node exposes
                        // outputs that nothing inside produces. Legal, and
                        // empty.
                        None => Vec::new(),
                    }
                }
            }
        }
    }

    /// What a **bypassed** node's outputs carry (R1586).
    ///
    /// Its inputs are still resolved — something has to pass through — and the
    /// routing is derived from the signature rather than authored. Asked before
    /// the body is looked at on purpose: it is the same answer for an
    /// application kind and for a group instance, and a group instance must NOT
    /// be descended into, since bypassing one is exactly the request not to run
    /// it.
    fn passed_through(
        &mut self,
        descent: &Descent<K>,
        node: NodeId,
        depth: usize,
        signature: &Signature<K>,
    ) -> Vec<Option<K::Value>> {
        let arity = signature.outputs.len();
        let inputs = self.node_inputs(descent, node, depth);
        let routing = self
            .document
            .passthrough(descent.tree, node)
            .unwrap_or_default();
        let mut passed = vec![None; arity];
        for route in routing.routes() {
            // R1593 — a route may convert, and it converts by the taxonomy's one
            // relation, asked in the direction the value travels. Read off the
            // signature rather than carried on the route, because a `fn` pointer
            // on a `Route` would make the routing a value that could not be
            // compared.
            let crossing = signature
                .inputs
                .get(route.input as usize)
                .zip(signature.outputs.get(route.output as usize))
                .map_or(Conversion::Refused, |(input, out)| {
                    crossing::<K>(input, out)
                });
            if let Some(slot) = passed.get_mut(route.output as usize) {
                *slot = inputs
                    .get(route.input as usize)
                    .cloned()
                    .flatten()
                    .and_then(|value| crossing.apply(value));
            }
        }
        passed
    }

    fn node_inputs(
        &mut self,
        descent: &Descent<K>,
        node: NodeId,
        depth: usize,
    ) -> Vec<Option<K::Value>> {
        let Some(signature) = self.document.signature(descent.tree, node) else {
            return Vec::new();
        };
        let mut resolved = Vec::with_capacity(signature.inputs.len());
        for (index, port) in signature.inputs.iter().enumerate() {
            let socket = Socket::new(node, u32::try_from(index).unwrap_or(u32::MAX));
            // R1586 — a MUTED link is structurally present and semantically
            // absent, so the port falls back to its own default exactly as if
            // nothing were wired. Filtered here rather than in `link_into`,
            // because every structural derivation in this crate — the group
            // boundary, the partition, the fragment cut — must go on seeing it:
            // the wire is still there, it is the value that is not.
            let feeding = self
                .document
                .tree(descent.tree)
                .and_then(|t| t.link_into(socket))
                .filter(|link| !link.muted)
                .copied();
            resolved.push(match feeding {
                Some(link) => {
                    // R1593 — the value crosses the link under the same relation
                    // that let the link exist, so a wire `connect` accepted can
                    // always carry what travels along it. A `Refused` here is
                    // unreachable for a document this crate built and is the
                    // honest answer for one that arrived from elsewhere with a
                    // link `validate` would flag.
                    let crossing = self
                        .document
                        .conversion(descent.tree, link.from, socket)
                        .unwrap_or(Conversion::Refused);
                    self.node_outputs(descent, link.from.node, depth + 1)
                        .into_iter()
                        .nth(link.from.port as usize)
                        .flatten()
                        .and_then(|value| crossing.apply(value))
                }
                // R1594 — nothing is wired, so the port carries what was
                // authored ON THIS NODE, and only failing that the kind's own
                // resting value. Two `Swatch` nodes are two colours; the kind
                // can only say what a Swatch is.
                None => self
                    .document
                    .port_value(descent.tree, node, PortRef::input(socket.port))
                    .or(port.default_value())
                    .cloned(),
            });
        }
        resolved
    }
}

/// One level of the descent: **where** a reading is being taken.
///
/// Named `Descent` and not `Frame`: R1589 gave this crate a [`NodeBody::Frame`], and a word that means
/// a stack level in one module and a canvas container in another is the exact
/// confusion R1586 spent a round untangling in the DCC's own vocabulary.
///
/// Public since R1600, because it is what
/// [`Document::run`](crate::Document::run) and
/// [`Document::tick`](crate::Document::tick) must **share** with the evaluator
/// rather than duplicate. R1599 recorded the cost of not sharing it: control
/// could not enter a group at all, because "a second copy would be free to
/// disagree about the value a node inside a group sees". Its fields stay
/// private — a descent is built by [`Evaluator::root`] and [`Evaluator::enter`]
/// or not at all, so one cannot be assembled that names an instance the
/// document does not have.
pub struct Descent<K: NodeKind> {
    instance: Instance,
    tree: TreeId,
    /// What the instance above was fed, standing in for this tree's
    /// inside-input node.
    bindings: Vec<Option<K::Value>>,
}

impl<K: NodeKind> Descent<K> {
    /// Which occurrence this is.
    #[must_use]
    pub const fn instance(&self) -> &Instance {
        &self.instance
    }

    /// The tree being read at this level.
    #[must_use]
    pub const fn tree(&self) -> TreeId {
        self.tree
    }
}

impl<K: NodeKind> Clone for Descent<K> {
    fn clone(&self) -> Self {
        Self {
            instance: self.instance.clone(),
            tree: self.tree,
            bindings: self.bindings.clone(),
        }
    }
}
