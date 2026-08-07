//! R1599 — the order the graph runs in.
//!
//! [`Document::evaluate`](crate::Document::evaluate) answers *what a node's
//! value is*. This answers *which nodes run, and when* — a different question
//! that a pure dataflow model cannot ask at all, because in one there is no
//! "when": every node's value exists, and the only order is the one the
//! dependencies force.
//!
//! # The two planes
//!
//! A value link and a control link are different edges obeying opposite laws
//! ([`Flow::multiplicity`](crate::Flow::multiplicity)), and the walk here
//! reads the second while the evaluator reads the first. So a node's **inputs**
//! come from wherever they always came from — pulled through the data plane, on
//! demand — and its **successor** comes from the control plane.
//!
//! Unreal splits the same way and says so on the predicate that does it:
//! `FKismetCompilerContext::PinIsImportantForDependancies` returns
//! `PinCategory != PC_Exec`, commented *"the execution wires do not form data
//! dependencies, they are only important for final scheduling and that is
//! handled thru gotos"*.
//!
//! # Why a stack
//!
//! A node may hand control to several outputs — that is a *sequence*, and it
//! means "run the first to completion, **then** the next", not "run them
//! concurrently". Completion is what needs remembering, so the pending work is
//! a stack and the successors of one step are pushed in reverse. Unreal's
//! sequence compiles to literally this: `push X1; goto A`, with `KCST_PushState`
//! onto a runtime state stack.
//!
//! # What a pure node is
//!
//! A node with no control ports never appears in a trace. It is not skipped —
//! it is *pulled*, by whoever reads its output, exactly as it always was. That
//! is the same split Unreal draws between an impure node (has exec pins, runs
//! when control reaches it, its results stored) and a pure one (no exec pins,
//! re-evaluated at each use).
//!
//! Because this crate's nodes are pure functions of their inputs
//! ([`NodeKind::evaluate`] takes `&self` and returns
//! values), a node that runs twice in a loop computes the same thing twice, and
//! the memo says so. Making the second pass *differ* needs a value that is one
//! iteration behind — SSA's φ, Lustre's `pre`, Simulink's unit delay — which is
//! a port that is allowed to close a data cycle. That is a value mechanism, not
//! a control one, and it is not in this module.

use std::collections::BTreeMap;
use std::fmt;

use crate::model::{Control, Document, NodeBody, NodeId, NodeKind, Port, Socket, TreeId};

/// One node's turn: what it computed, and where it sent control.
// `PartialEq` and not `Eq`, because a node's outputs are the taxonomy's values
// and the commonest one a node graph carries is a float.
#[derive(Debug, Clone, PartialEq)]
pub struct Step<K: NodeKind> {
    /// The node that ran.
    pub node: NodeId,
    /// Every output it produced, in port order — control ports included, where
    /// the slot is always `None` because control is not a value.
    pub outputs: Vec<Option<K::Value>>,
    /// The control output ports it handed control to, in the order it handed
    /// it. Empty when control stopped here.
    pub taken: Vec<u32>,
    /// Ports the kind named that are not control outputs of this node.
    ///
    /// Named rather than dropped: a taxonomy whose branch silently hands
    /// control to a *value* port would otherwise look exactly like one that
    /// deliberately halted, and those are opposite bugs.
    pub ignored: Vec<u32>,
}

/// Why a run stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// Nothing was left to run. The ordinary ending.
    Halted,
    /// The step budget ran out with work still pending — which is what an
    /// execution loop with no exit looks like from inside.
    ///
    /// Named rather than folded into [`Self::Halted`], because "it finished"
    /// and "we stopped watching" are different facts. Unreal reaches the same
    /// condition at run time, in a shipped build, by counting to
    /// `GMaximumScriptLoopIterations` and raising
    /// `EBlueprintExceptionType::InfiniteLoop`; the loop itself is nameable
    /// here before anything runs, by
    /// [`Document::control_loops`](crate::Document::control_loops).
    BudgetExhausted,
}

/// What running a control plane did.
#[derive(Debug, Clone, PartialEq)]
pub struct Run<K: NodeKind> {
    steps: Vec<Step<K>>,
    stop: Stop,
    budget: usize,
}

impl<K: NodeKind> Run<K> {
    /// Every turn taken, in the order taken.
    #[must_use]
    pub fn steps(&self) -> &[Step<K>] {
        &self.steps
    }

    /// Why it stopped.
    #[must_use]
    pub const fn stop(&self) -> Stop {
        self.stop
    }

    /// The budget it was given.
    #[must_use]
    pub const fn budget(&self) -> usize {
        self.budget
    }

    /// The nodes that ran, in order — the trace, without the values.
    ///
    /// A node that ran more than once appears more than once, which is the
    /// whole point of asking on a graph that may loop.
    #[must_use]
    pub fn trace(&self) -> Vec<NodeId> {
        self.steps.iter().map(|s| s.node).collect()
    }

    /// How many times `node` ran.
    #[must_use]
    pub fn visits(&self, node: NodeId) -> usize {
        self.steps.iter().filter(|s| s.node == node).count()
    }
}

/// Why a run could not start, or could not be trusted to finish correctly.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RunError {
    /// No such tree.
    NoSuchTree(TreeId),
    /// No such node in that tree.
    NoSuchNode(NodeId),
    /// The node has no control output, so nothing can follow it and there is no
    /// run to make. A pure node is read, not run.
    NotOnTheControlPlane(NodeId),
    /// Control reaches a **group instance**, and this runner does not descend
    /// into one (R1599).
    ///
    /// Refused rather than run, and the cause is mechanical rather than a
    /// preference: descending correctly means resolving the definition's
    /// interior against *this instance's* inputs, which is
    /// [`Evaluator`](crate::Evaluator)'s per-instance descent — a private
    /// derivation this walk would have to share rather than duplicate, since a
    /// second copy would be free to disagree about the value a node inside a
    /// group sees. Falling through the instance instead would run the graph
    /// *around* the group and silently skip its body, which is the one outcome
    /// worse than a refusal.
    ControlEntersGroup {
        /// The instance control reached.
        node: NodeId,
        /// The definition it stands for.
        definition: TreeId,
    },
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode(node) => write!(f, "no node {}", node.0),
            Self::NotOnTheControlPlane(node) => {
                write!(f, "node {} has no control output to follow", node.0)
            }
            Self::ControlEntersGroup { node, definition } => write!(
                f,
                "control reaches group instance {} of definition {}, \
                 which this runner does not descend into",
                node.0, definition.0
            ),
        }
    }
}

impl std::error::Error for RunError {}

impl<K: NodeKind> Document<K> {
    /// Which nodes of `tree` control can *begin* at, ascending (R1599).
    ///
    /// A node with at least one control output and **no control input at all**:
    /// nothing can hand control to it, so if it runs, it runs first. That is a
    /// property of the node's signature, which is what makes it derivable —
    /// Unreal reaches the same set by node *class* (`UK2Node_Event`,
    /// `UK2Node_FunctionEntry`), so there it is a list of types to know rather
    /// than a question to ask.
    ///
    /// A node whose control input is merely *unwired* is not an entry: it is
    /// unreachable, which is a different fact and one an editor reports
    /// differently.
    #[must_use]
    pub fn entry_points(&self, tree: TreeId) -> Vec<NodeId> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let mut found: Vec<NodeId> = host
            .nodes()
            .filter(|node| {
                self.signature(tree, node.id).is_some_and(|signature| {
                    signature.outputs.iter().any(Port::is_control)
                        && !signature.inputs.iter().any(Port::is_control)
                })
            })
            .map(|node| node.id)
            .collect();
        found.sort_unstable();
        found
    }

    /// Run the control plane of `tree` from `entry`, for at most `budget` steps.
    ///
    /// The order is derived, never stored: each node's kind says where it hands
    /// control ([`NodeKind::control`]), the control links say what is at the
    /// other end, and a node's data inputs are pulled through the value plane
    /// exactly as they always are.
    ///
    /// `budget` bounds the trace because a control **loop** is a legal graph
    /// here (see [`Document::control_loops`](crate::Document::control_loops)),
    /// so a run need not terminate. It is a parameter rather than a constant
    /// because how many steps are too many is the caller's question, and the
    /// answer is reported on the [`Run`] beside the reason it stopped.
    ///
    /// # Errors
    ///
    /// See [`RunError`].
    pub fn run(&self, tree: TreeId, entry: NodeId, budget: usize) -> Result<Run<K>, RunError> {
        if self.tree(tree).is_none() {
            return Err(RunError::NoSuchTree(tree));
        }
        let signature = self
            .signature(tree, entry)
            .ok_or(RunError::NoSuchNode(entry))?;
        if !signature.outputs.iter().any(Port::is_control) {
            return Err(RunError::NotOnTheControlPlane(entry));
        }

        // Where each control output goes. Built once: a control output holds at
        // most one link (`Multiplicity::One`), so this is a function, and
        // building it up front is also what lets the run be a pure read.
        let mut successor: BTreeMap<Socket, NodeId> = BTreeMap::new();
        if let Some(host) = self.tree(tree) {
            for link in host.links() {
                if self
                    .signature(tree, link.from.node)
                    .and_then(|s| s.outputs.get(link.from.port as usize).map(Port::is_control))
                    .unwrap_or(false)
                {
                    successor.insert(link.from, link.to.node);
                }
            }
        }

        let mut evaluator = self.evaluator();
        let mut steps: Vec<Step<K>> = Vec::new();
        let mut pending: Vec<NodeId> = vec![entry];
        let stop = loop {
            let Some(node) = pending.pop() else {
                break Stop::Halted;
            };
            if steps.len() >= budget {
                break Stop::BudgetExhausted;
            }
            let Some(signature) = self.signature(tree, node) else {
                continue;
            };
            // A group instance on the control plane is refused rather than
            // passed through — see `RunError::ControlEntersGroup`.
            if let Some(NodeBody::Group(definition)) =
                self.tree(tree).and_then(|t| t.node(node)).map(|n| &n.body)
            {
                return Err(RunError::ControlEntersGroup {
                    node,
                    definition: *definition,
                });
            }

            let inputs = evaluator.inputs(tree, node);
            let outputs = evaluator.outputs(tree, node);
            let answer =
                self.tree(tree)
                    .and_then(|t| t.node(node))
                    .map_or(Control::FallThrough, |held| match &held.body {
                        NodeBody::Kind(kind) => kind.control(&inputs),
                        // The structural bodies are this crate's, so their control
                        // answer is too: an interface node and a frame have no
                        // opinion, and fall through whatever they declare.
                        _ => Control::FallThrough,
                    });

            let control_outputs: Vec<u32> = signature
                .outputs
                .iter()
                .enumerate()
                .filter(|(_, p)| p.is_control())
                .map(|(at, _)| u32::try_from(at).unwrap_or(u32::MAX))
                .collect();
            let (taken, ignored) = match answer {
                Control::FallThrough => (control_outputs, Vec::new()),
                Control::Take(asked) => asked
                    .into_iter()
                    .partition(|port| control_outputs.contains(port)),
            };

            // Reverse, because the pending work is a stack and the first port
            // named must be the first to run — with everything its own branch
            // reaches running before the second port is reached at all.
            for port in taken.iter().rev() {
                if let Some(next) = successor.get(&Socket::new(node, *port)) {
                    pending.push(*next);
                }
            }
            steps.push(Step {
                node,
                outputs,
                taken,
                ignored,
            });
        };
        Ok(Run {
            steps,
            stop,
            budget,
        })
    }
}
