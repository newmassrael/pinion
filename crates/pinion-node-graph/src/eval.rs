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

use crate::model::{Document, InterfaceSide, NodeBody, NodeId, NodeKind, Socket, TreeId};

/// How deep the walk may recurse before it gives up.
///
/// Reached only by a pathological document — a chain longer than any editor
/// draws — and the alternative to a cap is a stack overflow, which takes the
/// host process with it. Whether it was reached is published
/// ([`Evaluator::truncated`]) rather than folded into the `None`s it produces,
/// because "no value" and "we stopped looking" are different facts.
const DEPTH_LIMIT: usize = 512;

/// Which instance of which definition a value belongs to.
///
/// The chain of group nodes descended through, outermost first. Empty at the
/// tree the evaluation started in.
type Instance = Vec<(TreeId, NodeId)>;

/// A memoised evaluation over one document.
///
/// Hold one across several reads — a sink, a hovered node, a debugger's
/// per-port readout — and the shared sub-graph is computed once. Drop it and the
/// memo goes; there is no cache to invalidate, which is the property that makes
/// this safe to call after an arbitrary edit.
pub struct Evaluator<'a, K: NodeKind> {
    document: &'a Document<K>,
    memo: BTreeMap<(Instance, NodeId), Vec<Option<K::Value>>>,
    visiting: BTreeSet<(Instance, NodeId)>,
    truncated: bool,
}

impl<K: NodeKind> Document<K> {
    /// An evaluator sharing one memo across many reads.
    #[must_use]
    pub fn evaluator(&self) -> Evaluator<'_, K> {
        Evaluator {
            document: self,
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
        let frame = self.top_frame(tree);
        self.node_outputs(&frame, node, 0)
    }

    /// Every resolved input of `node`, in port order: what actually arrives,
    /// which is the wired source's output when there is one and the port's own
    /// default when there is not.
    pub fn inputs(&mut self, tree: TreeId, node: NodeId) -> Vec<Option<K::Value>> {
        let frame = self.top_frame(tree);
        self.node_inputs(&frame, node, 0)
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

    /// The starting frame for a tree read on its own.
    fn top_frame(&self, tree: TreeId) -> Frame<K> {
        let bindings = self.document.tree(tree).map_or_else(Vec::new, |t| {
            t.interface()
                .inputs()
                .iter()
                .map(|port| port.default.clone())
                .collect()
        });
        Frame {
            instance: Instance::new(),
            tree,
            bindings,
        }
    }

    fn node_outputs(
        &mut self,
        frame: &Frame<K>,
        node: NodeId,
        depth: usize,
    ) -> Vec<Option<K::Value>> {
        let Some(signature) = self.document.signature(frame.tree, node) else {
            return Vec::new();
        };
        let arity = signature.outputs.len();
        let key = (frame.instance.clone(), node);
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
        let body = self
            .document
            .tree(frame.tree)
            .and_then(|t| t.node(node))
            .map(|n| n.body.clone());
        let Some(body) = body else {
            self.visiting.remove(&key);
            return vec![None; arity];
        };
        let mut outputs = match body {
            NodeBody::Kind(kind) => {
                let inputs = self.node_inputs(frame, node, depth);
                kind.evaluate(&inputs)
            }
            // The inside end of this tree's interface inputs: it produces what
            // the instance above was fed.
            NodeBody::Interface(InterfaceSide::Input) => frame.bindings.clone(),
            // The inside end of the OUTPUTS has none of its own.
            NodeBody::Interface(InterfaceSide::Output) => Vec::new(),
            NodeBody::Group(definition) => {
                let bindings = self.node_inputs(frame, node, depth);
                let mut instance = frame.instance.clone();
                instance.push((frame.tree, node));
                let inner = Frame {
                    instance,
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
                    // A definition with no inside-output node exposes outputs
                    // that nothing inside produces. Legal, and empty.
                    None => Vec::new(),
                }
            }
        };
        outputs.resize(arity, None);

        self.visiting.remove(&key);
        self.memo.insert(key, outputs.clone());
        outputs
    }

    fn node_inputs(
        &mut self,
        frame: &Frame<K>,
        node: NodeId,
        depth: usize,
    ) -> Vec<Option<K::Value>> {
        let Some(signature) = self.document.signature(frame.tree, node) else {
            return Vec::new();
        };
        let mut resolved = Vec::with_capacity(signature.inputs.len());
        for (index, port) in signature.inputs.iter().enumerate() {
            let socket = Socket::new(node, u32::try_from(index).unwrap_or(u32::MAX));
            let feeding = self
                .document
                .tree(frame.tree)
                .and_then(|t| t.link_into(socket))
                .copied();
            resolved.push(match feeding {
                Some(link) => self
                    .node_outputs(frame, link.from.node, depth + 1)
                    .into_iter()
                    .nth(link.from.port as usize)
                    .flatten(),
                None => port.default.clone(),
            });
        }
        resolved
    }
}

/// One level of the descent.
struct Frame<K: NodeKind> {
    instance: Instance,
    tree: TreeId,
    /// What the instance above was fed, standing in for this tree's
    /// inside-input node.
    bindings: Vec<Option<K::Value>>,
}
