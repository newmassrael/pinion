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
//! ([`NodeKind::evaluate`] takes `&self` and returns values), a node that runs
//! twice in a loop computes the same thing twice. R1600 supplies the value that
//! is one step behind — [`NodeBody::Delay`], read through a
//! [`Machine`] — and puts it deliberately on the **other** side
//! of the split this module draws: a run reads the registers, it does not
//! advance them. [`Document::tick`] advances them, all at once. So a control
//! loop spins within one instant and the world moves between instants, which is
//! Simulink's own division of labour between a step's sorted execution order and
//! its unit delays, and it is what keeps a tick's outcome a function of the
//! document and the registers rather than of the walk.
//!
//! # A run has a call stack
//!
//! Control entering a group instance is entering **that instance**, so the
//! walk carries a stack of levels, each holding the descent it is reading
//! through and the return address it came in by. A group instance is not a
//! computation and takes no turn of its own; entering one shows up in the trace
//! as the first [`Step`] *inside* it, whose [`Step::instance`] names the
//! instance node. Unreal has no equivalent because it has no instance: a macro
//! is expanded by `FEdGraphUtilities::CloneGraph`, so its N uses are N copies of
//! the nodes before anything runs.

use std::collections::BTreeMap;
use std::fmt;

use crate::eval::Descent;
use crate::machine::Machine;
use crate::model::{
    Control, Document, Instance, InterfaceSide, NodeBody, NodeId, NodeKind, Port, Socket, TreeId,
};

/// One node's turn: what it computed, and where it sent control.
// `PartialEq` and not `Eq`, because a node's outputs are the taxonomy's values
// and the commonest one a node graph carries is a float.
#[derive(Debug, Clone, PartialEq)]
pub struct Step<K: NodeKind> {
    /// **Which occurrence** of the node ran (R1600).
    ///
    /// A [`NodeId`] is unique within its tree and a definition's tree is shared
    /// by all its instances, so on a run that descends into groups the node
    /// alone does not say where a step happened. Two instances of one group
    /// produce steps with equal `node` and different `instance`.
    pub instance: Instance,
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
    /// whole point of asking on a graph that may loop. On a run that descended
    /// into groups this **flattens the instances**, so use [`Self::visited`]
    /// when two instances of one definition have to be told apart.
    #[must_use]
    pub fn trace(&self) -> Vec<NodeId> {
        self.steps.iter().map(|s| s.node).collect()
    }

    /// The trace with each step's occurrence, in order (R1600).
    #[must_use]
    pub fn visited(&self) -> Vec<(Instance, NodeId)> {
        self.steps
            .iter()
            .map(|s| (s.instance.clone(), s.node))
            .collect()
    }

    /// How many times `node` ran, across every instance it ran in.
    #[must_use]
    pub fn visits(&self, node: NodeId) -> usize {
        self.steps.iter().filter(|s| s.node == node).count()
    }

    /// How many times `node` ran **inside one instance** (R1600).
    #[must_use]
    pub fn visits_in(&self, instance: &Instance, node: NodeId) -> usize {
        self.steps
            .iter()
            .filter(|s| s.node == node && &s.instance == instance)
            .count()
    }

    /// Every group instance control entered, ascending — the run's own record
    /// of where it went (R1600).
    #[must_use]
    pub fn entered(&self) -> Vec<Instance> {
        let mut found: Vec<Instance> = self
            .steps
            .iter()
            .filter(|s| !s.instance.is_root())
            .map(|s| s.instance.clone())
            .collect();
        found.sort();
        found.dedup();
        found
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
    /// A run was asked to *begin* at a group instance (R1600).
    ///
    /// Control enters a group through one of the instance's control inputs, and
    /// which one decides where inside it lands. A run starting there arrived by
    /// no port, so there is no answer — and a definition with several entry
    /// points of its own gives the question several, which is why this is
    /// refused rather than guessed. Begin at a node that hands control *to* the
    /// instance, or run the definition on its own.
    EntryIsAnInstance(NodeId),
    /// Control entered a group instance whose definition has no inside-input
    /// node, so there is nothing on the other side of the tunnel (R1600).
    ///
    /// Named rather than treated as a halt: a run that quietly stopped at the
    /// edge of a group looks exactly like one that finished, and those are
    /// opposite facts. The definition is malformed only in this respect —
    /// evaluating it is fine, since its interface inputs simply reach nothing.
    GroupHasNoEntry {
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
            Self::EntryIsAnInstance(node) => write!(
                f,
                "node {} is a group instance, and control enters one by a port \
                 rather than by starting there",
                node.0
            ),
            Self::GroupHasNoEntry { node, definition } => write!(
                f,
                "control entered group instance {}, whose definition {} has no \
                 inside-input node to enter at",
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
        self.walk(tree, entry, budget, None)
    }

    /// The same run, reading a [`Machine`]'s registers (R1600).
    ///
    /// Every [`NodeBody::Delay`] the data plane reaches answers what the machine
    /// is holding rather than its initial value, so the trace can *change*
    /// between ticks — a branch whose condition comes from a delay takes a
    /// different arm once the world has moved. The machine is taken by shared
    /// reference because a run does not advance it: see this module's header.
    ///
    /// # Errors
    ///
    /// See [`RunError`].
    pub fn run_on(
        &self,
        tree: TreeId,
        entry: NodeId,
        budget: usize,
        state: &Machine<K>,
    ) -> Result<Run<K>, RunError> {
        self.walk(tree, entry, budget, Some(state))
    }

    fn walk(
        &self,
        tree: TreeId,
        entry: NodeId,
        budget: usize,
        state: Option<&Machine<K>>,
    ) -> Result<Run<K>, RunError> {
        self.check_entry(tree, entry)?;
        let mut evaluator = self.evaluator_with(state);
        let mut levels: Vec<Level<K>> = vec![Level {
            descent: evaluator.root(tree),
            returns_to: None,
        }];
        // One control-edge map per tree, built the first time control is in it.
        // A control output holds at most one link (`Multiplicity::One`), so each
        // is a function; the *target socket* is the value rather than the target
        // node, because which control input control arrived at is what a tunnel
        // and a branch each need to know.
        let mut successors: BTreeMap<TreeId, BTreeMap<Socket, Socket>> = BTreeMap::new();

        let mut steps: Vec<Step<K>> = Vec::new();
        let mut pending: Vec<Arrival> = vec![Arrival {
            level: 0,
            node: entry,
            port: None,
        }];
        let stop = loop {
            let Some(arrival) = pending.pop() else {
                break Stop::Halted;
            };
            if steps.len() >= budget {
                break Stop::BudgetExhausted;
            }
            let here = levels[arrival.level].descent.clone();
            let host = here.tree();
            let Some(signature) = self.signature(host, arrival.node) else {
                continue;
            };
            let Some(body) = self
                .tree(host)
                .and_then(|t| t.node(arrival.node))
                .map(|held| held.body.clone())
            else {
                continue;
            };

            // Entering a group: control tunnels to the definition's inside-input
            // node, at the same port index, inside a descent bound to THIS
            // instance's inputs. The instance itself takes no turn — it is not a
            // computation, which is the same reason the evaluator descends
            // through one instead of evaluating it.
            if let NodeBody::Group(definition) = body {
                let inside = self.tunnel_into(definition, &arrival)?;
                let deeper = evaluator.enter(&here, arrival.node, definition);
                levels.push(Level {
                    descent: deeper,
                    returns_to: Some((arrival.level, arrival.node)),
                });
                pending.push(Arrival {
                    level: levels.len() - 1,
                    node: inside,
                    port: arrival.port,
                });
                continue;
            }

            let inputs = evaluator.inputs_in(&here, arrival.node);
            let outputs = evaluator.outputs_in(&here, arrival.node);
            let answer = match (&body, arrival.port) {
                (NodeBody::Kind(kind), _) => kind.control(&inputs),
                // The inside-input node is a TUNNEL: control leaves it by the
                // one port it came in by, because a group's interface port `i`
                // outside and inside are the same port. Falling through every
                // control output would run every entry of the definition on a
                // single arrival, which is a different graph.
                //
                // Arriving by no port at all is a definition being run on its
                // own, where there is no outside port to correspond to — so
                // there it does fall through, and every control entry of the
                // definition fires in port order.
                (NodeBody::Interface(InterfaceSide::Input), Some(port)) => Control::to(port),
                // A frame, and the inside-OUTPUT node, whose signature has no
                // outputs at all — so falling through takes nothing and control
                // leaves by the tunnel below.
                _ => Control::FallThrough,
            };

            let (taken, ignored) = resolve_control(&answer, &signature);

            // Leaving a group: control reaching the inside-OUTPUT node at port
            // `i` resumes in the caller at the instance's output port `i`. The
            // return address is on the level, which is what makes this a call
            // stack rather than a lookup — the same instance may be on it more
            // than once.
            if let (NodeBody::Interface(InterfaceSide::Output), Some(port)) = (&body, arrival.port)
                && let Some((caller, instance_node)) = levels[arrival.level].returns_to
            {
                let outer = levels[caller].descent.tree();
                let edges = Self::control_edges(self, outer, &mut successors);
                if let Some(next) = edges.get(&Socket::new(instance_node, port)).copied() {
                    pending.push(Arrival {
                        level: caller,
                        node: next.node,
                        port: Some(next.port),
                    });
                }
            }

            // Reverse, because the pending work is a stack and the first port
            // named must be the first to run — with everything its own branch
            // reaches running before the second port is reached at all.
            let edges = Self::control_edges(self, host, &mut successors);
            let outgoing: Vec<Option<Socket>> = taken
                .iter()
                .map(|port| edges.get(&Socket::new(arrival.node, *port)).copied())
                .collect();
            for next in outgoing.into_iter().rev().flatten() {
                pending.push(Arrival {
                    level: arrival.level,
                    node: next.node,
                    port: Some(next.port),
                });
            }
            steps.push(Step {
                instance: here.instance().clone(),
                node: arrival.node,
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

    /// Whether a run can begin at `entry` at all.
    ///
    /// # Errors
    ///
    /// [`RunError::NoSuchTree`], [`RunError::NoSuchNode`],
    /// [`RunError::NotOnTheControlPlane`] or [`RunError::EntryIsAnInstance`].
    fn check_entry(&self, tree: TreeId, entry: NodeId) -> Result<(), RunError> {
        if self.tree(tree).is_none() {
            return Err(RunError::NoSuchTree(tree));
        }
        let signature = self
            .signature(tree, entry)
            .ok_or(RunError::NoSuchNode(entry))?;
        if !signature.outputs.iter().any(Port::is_control) {
            return Err(RunError::NotOnTheControlPlane(entry));
        }
        if matches!(
            self.tree(tree).and_then(|t| t.node(entry)).map(|n| &n.body),
            Some(NodeBody::Group(_))
        ) {
            return Err(RunError::EntryIsAnInstance(entry));
        }
        Ok(())
    }

    /// The node inside `definition` that control entering an instance lands on.
    ///
    /// Both refusals live here rather than at the call site, because both are
    /// about the tunnel and neither is about the walk.
    fn tunnel_into(&self, definition: TreeId, arrival: &Arrival) -> Result<NodeId, RunError> {
        if arrival.port.is_none() {
            return Err(RunError::EntryIsAnInstance(arrival.node));
        }
        self.tree(definition)
            .and_then(|t| t.interface_node(InterfaceSide::Input))
            .map(|n| n.id)
            .ok_or(RunError::GroupHasNoEntry {
                node: arrival.node,
                definition,
            })
    }

    /// Where each control output of `tree` goes, memoised across the walk.
    fn control_edges<'a>(
        &self,
        tree: TreeId,
        built: &'a mut BTreeMap<TreeId, BTreeMap<Socket, Socket>>,
    ) -> &'a BTreeMap<Socket, Socket> {
        built.entry(tree).or_insert_with(|| {
            let mut edges = BTreeMap::new();
            if let Some(host) = self.tree(tree) {
                for link in host.links() {
                    if self
                        .signature(tree, link.from.node)
                        .and_then(|s| s.outputs.get(link.from.port as usize).map(Port::is_control))
                        .unwrap_or(false)
                    {
                        edges.insert(link.from, link.to);
                    }
                }
            }
            edges
        })
    }
}

/// One level of a run's call stack (R1600).
struct Level<K: NodeKind> {
    descent: Descent<K>,
    /// Where control goes when it leaves this group: the level that entered it,
    /// and the instance node there. `None` at the tree the run started in.
    returns_to: Option<(usize, NodeId)>,
}

/// Control arriving somewhere: which level, which node, and by which of that
/// node's control inputs — `None` only for the node the run began at.
struct Arrival {
    level: usize,
    node: NodeId,
    port: Option<u32>,
}

/// Which control outputs a node hands control to, and which of the ports it
/// named are not control outputs of its own.
///
/// The second half is why this is a partition rather than a filter: a taxonomy
/// whose branch quietly hands control to a *value* port would otherwise look
/// exactly like one that deliberately halted, and those are opposite bugs.
fn resolve_control<K: NodeKind>(
    answer: &Control,
    signature: &crate::model::Signature<K>,
) -> (Vec<u32>, Vec<u32>) {
    let control_outputs: Vec<u32> = signature
        .outputs
        .iter()
        .enumerate()
        .filter(|(_, port)| port.is_control())
        .map(|(at, _)| u32::try_from(at).unwrap_or(u32::MAX))
        .collect();
    match answer {
        Control::FallThrough => (control_outputs, Vec::new()),
        Control::Take(asked) => asked
            .iter()
            .copied()
            .partition(|port| control_outputs.contains(port)),
    }
}
