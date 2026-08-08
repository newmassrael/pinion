//! R1599 §5.38 §5.52 — a **control** graph, composed from `pinion-node-graph`.
//!
//! Every other node-graph binding in this tree is a *dataflow* graph: a node's
//! value is a function of its inputs, every node has one, and the only order is
//! the one the dependencies force. This one is the other kind — the kind
//! Blueprint is — where an edge can say **when** instead of **what**, a node
//! can be skipped, and a cycle is a loop rather than a defect.
//!
//! What is absent here is the argument. This file declares a taxonomy and paints
//! it. It contains:
//!
//! * no link law — which end of a wire gives way is
//!   [`pinion_node_graph::Flow::multiplicity`], and it *inverts* between the
//!   planes: a value input takes one producer, a control output takes one
//!   successor;
//! * no acyclicity test — `Document::connect` refuses a value cycle and accepts
//!   a control one, and `Document::control_loops` names the loop's members;
//! * no scheduler — `Document::run` derives the order, and the sequence
//!   semantics (run one branch to completion, *then* the next) come from the
//!   framework's stack, not from anything here;
//! * no branch machinery — [`NodeKind::control`] is one method, and the
//!   taxonomy below overrides it exactly **once**, for `Branch`. A three-way
//!   `Sequence` takes the provided default and writes nothing, where Unreal
//!   5.8.1 needs a `UK2Node_ExecutionSequence` class and an
//!   `FKCHandler_ExecutionSequence` compile handler for the same behaviour.
//!
//! The screen is the Blueprint debugger's question — *which nodes ran, in what
//! order, and which never ran at all* — which is a question a dataflow graph
//! cannot be asked, because there every node has a value.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{Border, BoxStyle, Color, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_node_graph::{
    Document, Instance, Machine, NodeBody, NodeId, NodeKind, Port, PortRef, ROOT, Socket, Stop,
    TreeId,
};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloNodeFlowRenderer, HelloNodeFlowRendererError);

const VIEW_TAG: &str = "flow";
const THEME_TAG: &str = "flow.theme";
const TREE: TreeId = ROOT;
const WIN_W: u32 = 940;
const WIN_H: u32 = 620;
const ROW_H: u32 = 30;
const TITLE_FONT_PX: u32 = 17;
const BODY_FONT_PX: u32 = 13;

/// How many steps a run is allowed. A control **loop** is a legal graph here,
/// so a run need not terminate and the bound is the caller's decision — which
/// is why `Document::run` takes it rather than owning a constant.
const STEP_BUDGET: usize = 24;

/// How far ahead `ticks_to_finish` will look before answering. A scenario need
/// not converge at all, so the horizon is stated rather than implied.
const TICK_HORIZON: usize = 64;

// ---------------------------------------------------------------- taxonomy

/// The one socket type. A control port has none, which is the point.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Ty {
    Number,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
enum Val {
    Number(i64),
}

impl Val {
    const fn number(&self) -> i64 {
        match self {
            Self::Number(n) => *n,
        }
    }
}

/// A scenario step. `Reading` and `Budget` are **pure** — no control ports at
/// all — so they never appear in a trace; they are pulled when something reads
/// them, which is exactly Unreal's pure/impure split.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
enum Op {
    /// No control input, so it is an entry point — derivable rather than a
    /// class to know (Unreal reaches the same set by testing for
    /// `UK2Node_Event` / `UK2Node_FunctionEntry`).
    Begin,
    /// One control in, one out, plus a value out: the ordinary statement.
    Task(String),
    /// One in, N out. Overrides NOTHING — the provided `control` default hands
    /// control to every control output in port order, and the framework's stack
    /// is what makes that mean "each to completion, then the next".
    Fork(usize),
    /// One in, two out, choosing by what arrived at its value input.
    Branch,
    /// One in, none out.
    Finish,
    /// Pure: a constant reading.
    Reading(i64),
    /// Pure: is the reading over the budget?
    Over,
    /// Pure: one more than what arrived. The step a register advances by —
    /// pure, because the *remembering* is the delay's job and not a kind's.
    Bump,
}

impl Op {
    fn title(&self) -> String {
        match self {
            Self::Begin => "Begin".into(),
            Self::Task(what) => format!("Task {what}"),
            Self::Fork(n) => format!("Fork x{n}"),
            Self::Branch => "Branch".into(),
            Self::Finish => "Finish".into(),
            Self::Reading(n) => format!("Reading {n}"),
            Self::Over => "Over budget?".into(),
            Self::Bump => "Bump +1".into(),
        }
    }
}

impl NodeKind for Op {
    type Type = Ty;
    type Value = Val;

    fn name(&self) -> String {
        self.title()
    }

    fn inputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Begin | Self::Reading(_) => Vec::new(),
            Self::Task(_) | Self::Fork(_) | Self::Finish => vec![Port::control("In")],
            Self::Branch => vec![Port::control("In"), Port::new("Condition", Ty::Number)],
            Self::Over => vec![
                Port::new("Reading", Ty::Number),
                Port::new("Limit", Ty::Number).with_default(Val::Number(40)),
            ],
            Self::Bump => vec![Port::new("In", Ty::Number)],
        }
    }

    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Begin => vec![Port::control("Then")],
            Self::Task(_) => vec![Port::control("Then"), Port::new("Cost", Ty::Number)],
            Self::Fork(n) => (0..*n)
                .map(|i| Port::control(format!("Then {i}")))
                .collect(),
            Self::Branch => vec![Port::control("True"), Port::control("False")],
            Self::Finish => Vec::new(),
            Self::Reading(_) | Self::Over | Self::Bump => vec![Port::new("Out", Ty::Number)],
        }
    }

    fn evaluate(&self, inputs: &[Option<Val>]) -> Vec<Option<Val>> {
        let number = |i: usize| inputs.get(i).and_then(Option::as_ref).map(Val::number);
        match self {
            Self::Begin | Self::Finish => Vec::new(),
            // Slot 0 is a control output and carries nothing — a control port
            // has no value, and the evaluator's own slot for it stays `None`.
            Self::Task(what) => vec![
                None,
                Some(Val::Number(i64::try_from(what.len()).unwrap_or(0))),
            ],
            Self::Fork(n) => vec![None; *n],
            Self::Branch => vec![None, None],
            Self::Reading(n) => vec![Some(Val::Number(*n))],
            Self::Over => vec![
                number(0)
                    .zip(number(1))
                    .map(|(reading, limit)| Val::Number(i64::from(reading > limit))),
            ],
            Self::Bump => vec![number(0).map(|n| Val::Number(n + 1))],
        }
    }

    /// The taxonomy's ONE override. Everything else — `Fork` included — takes
    /// the provided default.
    fn control(&self, inputs: &[Option<Val>]) -> pinion_node_graph::Control {
        match self {
            Self::Branch => {
                let truthy = inputs
                    .get(1)
                    .and_then(Option::as_ref)
                    .is_some_and(|v| v.number() != 0);
                pinion_node_graph::Control::to(u32::from(!truthy))
            }
            _ => pinion_node_graph::Control::FallThrough,
        }
    }
}

// ------------------------------------------------------------------- state

type Graph = Document<Op>;

struct FlowState {
    document: Signal<Graph>,
    /// The registers, R1600. Held beside the document rather than inside it,
    /// because a machine is what the graph *is doing* and the document is what
    /// it *is* — and only one of the two is saved with the file.
    machine: Signal<Machine<Op>>,
    budget: Signal<usize>,
    refusal: Signal<String>,
}

/// A scenario with **both** things a loop needs to mean something (R1600).
///
/// Control: `Begin -> Fork x2`. Arm 0 is `Task warm -> Branch`, whose False arm
/// loops back to `Task warm` and whose True arm settles. Arm 1 is a **group
/// instance** — control descends into it and comes back out — followed by
/// `Finish`. Arm 1 is only ever reached once arm 0 *completes*, because a fork
/// runs each arm to completion before the next.
///
/// Data: `elapsed(Delay) -> Bump +1 -> elapsed` is a value cycle closed through
/// a register, which is the only kind of value cycle there is. `elapsed` feeds
/// `Over budget?`, which feeds the branch — so **how many ticks the scenario
/// takes** is a question with an answer, and the answer needs the register.
///
/// Before R1600 this seed ran the loop until the step budget ran out, every
/// time, because nothing in the graph could change between iterations.
fn seed() -> (Graph, Vec<NodeId>) {
    let mut document = Document::new("scenario");
    let add = |document: &mut Graph, op: Op, x: i32, y: i32| {
        document.add_node(TREE, NodeBody::Kind(op), x, y).unwrap()
    };
    let begin = add(&mut document, Op::Begin, 40, 40);
    let fork = add(&mut document, Op::Fork(2), 40, 110);
    let warm = add(&mut document, Op::Task("warm".into()), 40, 190);
    let branch = add(&mut document, Op::Branch, 40, 260);
    let settle = add(&mut document, Op::Task("settle".into()), 40, 330);
    let drain = add(&mut document, Op::Task("drain".into()), 260, 400);
    let finish = add(&mut document, Op::Finish, 260, 470);
    let bump = add(&mut document, Op::Bump, 700, 190);
    let over = add(&mut document, Op::Over, 700, 330);
    // THE REGISTER. Its initial value is authored on its own output port, which
    // is R1594's mechanism rather than a second one -- Lustre's `->`.
    let elapsed = document
        .add_node(TREE, NodeBody::Delay(Ty::Number), 480, 260)
        .unwrap();
    document
        .set_port_value(TREE, elapsed, PortRef::output(0), Val::Number(0))
        .expect("a Number register starts at a Number");
    // And the limit is authored on the node that reads it, not on the kind.
    document
        .set_port_value(TREE, over, PortRef::input(1), Val::Number(LIMIT))
        .expect("the limit is this node's");

    let wire = |document: &mut Graph, from: (NodeId, u32), to: (NodeId, u32)| {
        document
            .connect(TREE, Socket::new(from.0, from.1), Socket::new(to.0, to.1))
            .expect("the seed wires are legal");
    };
    wire(&mut document, (begin, 0), (fork, 0));
    wire(&mut document, (fork, 0), (warm, 0));
    wire(&mut document, (warm, 0), (branch, 0));
    wire(&mut document, (branch, 0), (settle, 0));
    wire(&mut document, (fork, 1), (drain, 0));
    wire(&mut document, (drain, 0), (finish, 0));
    // The value plane. `elapsed -> bump -> elapsed` closes a cycle through the
    // register: legal, and refused for any other node.
    wire(&mut document, (elapsed, 0), (bump, 0));
    wire(&mut document, (bump, 0), (elapsed, 0));
    wire(&mut document, (elapsed, 0), (over, 0));
    wire(&mut document, (over, 0), (branch, 1));
    // THE CONTROL LOOP. `connect` accepts this because it closes a cycle through
    // CONTROL links, and a control cycle is a loop rather than a contradiction.
    wire(&mut document, (branch, 1), (warm, 0));

    // Arm 1's stage becomes a re-usable definition, so control has a boundary
    // to cross. The interface is DERIVED from the control links that crossed.
    let stage = document
        .group(TREE, &[drain], "Stage")
        .expect("one step collapses");

    (
        document,
        vec![
            begin, fork, warm, branch, settle, stage.node, finish, bump, over, elapsed,
        ],
    )
}

/// How many ticks the loop runs for. Authored onto the `Over budget?` node's
/// own Limit port, so the scenario's length is data rather than a constant the
/// taxonomy carries.
const LIMIT: i64 = 3;

fn use_flow_state() -> Rc<FlowState> {
    Owner::current()
        .expect("use_flow_state requires an active Owner scope")
        .cache("flow.state", || {
            let (document, _) = seed();
            FlowState {
                document: Signal::new(document),
                machine: Signal::new(Machine::new()),
                budget: Signal::new(STEP_BUDGET),
                refusal: Signal::new(String::new()),
            }
        })
}

/// The run as it stands **against the registers as they are**: the trace with
/// each step's instance, and why it stopped (R1600).
///
/// A run reads the machine; it does not advance it. So this is a pure function
/// of `(document, machine, budget)`, and asking it twice cannot answer twice.
fn current_run(
    document: &Graph,
    machine: &Machine<Op>,
    budget: usize,
) -> (Vec<(Instance, NodeId)>, Option<Stop>) {
    let Some(&entry) = document.entry_points(TREE).first() else {
        return (Vec::new(), None);
    };
    document.run_on(TREE, entry, budget, machine).map_or_else(
        |_| (Vec::new(), None),
        |run| (run.visited(), Some(run.stop())),
    )
}

/// A node's title, wherever in the instance tree it sits.
fn titled(document: &Graph, instance: &Instance, id: NodeId) -> String {
    let tree = instance.path().last().map_or(TREE, |(host, node)| {
        document
            .tree(*host)
            .and_then(|t| t.node(*node))
            .and_then(|held| match held.body {
                NodeBody::Group(definition) => Some(definition),
                _ => None,
            })
            .unwrap_or(TREE)
    });
    document
        .tree(tree)
        .and_then(|t| t.node(id))
        .map_or_else(String::new, pinion_node_graph::Node::display_name)
}

fn node_title(document: &Graph, id: NodeId) -> String {
    document
        .tree(TREE)
        .and_then(|t| t.node(id))
        .map_or_else(String::new, |node| match &node.body {
            NodeBody::Kind(op) => op.title(),
            // R1600 — a register says what it holds. Left as an explicit arm
            // rather than a `{other:?}` fall-through, because a body this crate
            // owns is one this view is expected to be able to name.
            NodeBody::Delay(ty) => format!("Delay <{ty:?}>"),
            NodeBody::Group(definition) => document
                .tree(*definition)
                .map_or_else(|| "Group".to_owned(), |t| format!("Group {}", t.name)),
            other => format!("{other:?}"),
        })
}

// -------------------------------------------------------------------- view

/// One absolutely-positioned line of text. Every string this view paints goes
/// through here, so a reader looking for "what does the screen say" has one
/// place to look.
fn text(body: String, tag: String, x: u32, y: u32, w: u32, px: u32, fg: Color) -> Scene {
    Scene::Text(
        TextNode::styled(
            body,
            Rect::default(),
            TextStyle::new().with_size_px(px).with_fg(fg),
        )
        .with_tag(tag)
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(x, y)
                .with_size(Size::px(w, px + 6)),
        ),
    )
}

/// The roster: every node, and how many times the run reached it. A node that
/// never ran is the fact a dataflow view cannot show, because there every node
/// has a value.
fn roster(
    document: &Graph,
    theme: &pinion_core::theme::Theme,
    machine: &Machine<Op>,
    trace: &[(Instance, NodeId)],
    loops: &[NodeId],
) -> Vec<Scene> {
    let ink = theme.resolve(ColorRole::OnSurface);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let accent = theme.resolve(ColorRole::Accent);
    let mut ids: Vec<NodeId> = document
        .tree(TREE)
        .map(|t| t.nodes().map(|n| n.id).collect())
        .unwrap_or_default();
    ids.sort_unstable();
    let mut out = Vec::new();
    for (row, id) in ids.iter().enumerate() {
        let y = 100 + u32::try_from(row).unwrap_or(0) * ROW_H;
        let ran = trace
            .iter()
            .filter(|(instance, node)| instance.is_root() && node == id)
            .count();
        let register = machine
            .read(&Instance::root(), *id)
            .map(|value| format!("holds {}", value.number()));
        let on_loop = loops.contains(id);
        let is_pure = document.signature(TREE, *id).is_some_and(|s| {
            !s.inputs.iter().any(Port::is_control) && !s.outputs.iter().any(Port::is_control)
        });
        out.push(Scene::Box(
            BoxNode::new(
                Rect::default(),
                BoxStyle::filled(if ran > 0 {
                    theme.resolve(ColorRole::SurfaceContainerHighest)
                } else {
                    theme.resolve(ColorRole::Surface)
                })
                .with_border(Border::new(
                    if on_loop {
                        accent
                    } else {
                        theme.resolve(ColorRole::Outline)
                    },
                    1,
                )),
            )
            .with_tag(format!("{VIEW_TAG}.node.{id}"))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(20, y)
                    .with_size(Size::px(420, ROW_H - 4)),
            ),
        ));
        out.push(text(
            format!(
                "{}  ·  {}",
                node_title(document, *id),
                if let Some(register) = register {
                    register
                } else if is_pure {
                    "pure — pulled, never in the trace".to_owned()
                } else if ran == 0 {
                    "did not run".to_owned()
                } else {
                    format!("ran {ran}x")
                }
            ),
            format!("{VIEW_TAG}.node.{id}.label"),
            30,
            y + 5,
            400,
            BODY_FONT_PX,
            if ran > 0 { ink } else { muted },
        ));
    }
    out
}

/// The two header lines: what the run did, and what the world holds.
///
/// Lifted out of `view` because the round gave the header a second subject —
/// the machine — and a view function that paints the whole screen in one body
/// stops being readable at exactly the point it starts having sections.
fn header(
    document: &Graph,
    machine: &Machine<Op>,
    trace: &[(Instance, NodeId)],
    stop: Option<Stop>,
    budget: usize,
    loops: &[NodeId],
    theme: &pinion_core::theme::Theme,
) -> Vec<Scene> {
    let ink = theme.resolve(ColorRole::OnSurface);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    vec![
        text(
            "Control plane — which nodes run, when, and what the graph remembers".into(),
            format!("{VIEW_TAG}.title"),
            20,
            16,
            WIN_W - 40,
            TITLE_FONT_PX,
            ink,
        ),
        text(
            format!(
                "{} steps, stopped: {}   ·   loop members: {}   ·   entries: {}",
                trace.len(),
                match stop {
                    Some(Stop::Halted) => "halted".to_owned(),
                    Some(Stop::BudgetExhausted) => format!("budget ({budget}) exhausted"),
                    None => "no entry".to_owned(),
                },
                if loops.is_empty() {
                    "none".to_owned()
                } else {
                    loops
                        .iter()
                        .map(|id| node_title(document, *id))
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                document.entry_points(TREE).len(),
            ),
            format!("{VIEW_TAG}.status"),
            20,
            42,
            WIN_W - 40,
            BODY_FONT_PX,
            muted,
        ),
        text(
            format!(
                "tick {}   ·   {} register(s) held   ·   {}",
                machine.ticks(),
                machine.len(),
                machine
                    .iter()
                    .map(|(instance, node, value)| format!(
                        "{instance}@{node} = {}",
                        value.number()
                    ))
                    .collect::<Vec<_>>()
                    .join("  ")
            ),
            format!("{VIEW_TAG}.machine"),
            20,
            60,
            WIN_W - 40,
            BODY_FONT_PX,
            muted,
        ),
    ]
}

fn view() -> Scene {
    let state = use_flow_state();
    let theme = use_theme(THEME_TAG).theme_animated();
    let document = state.document.get();
    let machine = state.machine.get();
    let budget = state.budget.get();
    let ink = theme.resolve(ColorRole::OnSurface);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let (trace, stop) = current_run(&document, &machine, budget);
    let loops = document.control_loops(TREE);

    let mut children = header(&document, &machine, &trace, stop, budget, &loops, &theme);

    children.extend(roster(&document, &theme, &machine, &trace, &loops));

    // Right column: the trace itself, in order.
    children.push(text(
        "Trace".into(),
        format!("{VIEW_TAG}.trace.title"),
        480,
        100,
        420,
        BODY_FONT_PX,
        ink,
    ));
    for (step, (instance, id)) in trace.iter().enumerate().take(16) {
        let y = 124 + u32::try_from(step).unwrap_or(0) * 22;
        children.push(text(
            format!(
                "{step}. {}{}",
                titled(&document, instance, *id),
                if instance.is_root() {
                    String::new()
                } else {
                    format!("   in {instance}")
                }
            ),
            format!("{VIEW_TAG}.trace.{step}"),
            480,
            y,
            420,
            BODY_FONT_PX,
            muted,
        ));
    }

    let refusal = state.refusal.get();
    if !refusal.is_empty() {
        children.push(text(
            format!("refused: {refusal}"),
            format!("{VIEW_TAG}.refusal"),
            20,
            WIN_H - 34,
            WIN_W - 40,
            BODY_FONT_PX,
            theme.resolve(ColorRole::Error),
        ));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(VIEW_TAG)
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

// --------------------------------------------------------------------- rpc

struct FlowOracle {
    state: Option<Rc<FlowState>>,
}

impl core::fmt::Debug for FlowOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FlowOracle")
            .field("attached", &self.state.is_some())
            .finish()
    }
}

impl FlowOracle {
    const fn new() -> Self {
        Self { state: None }
    }

    fn attach(&mut self, state: Rc<FlowState>) {
        self.state = Some(state);
    }

    fn bound(&self) -> Result<&Rc<FlowState>, InvokeError> {
        self.state.as_ref().ok_or_else(|| {
            InvokeError::Rejected("this control-flow surface is not bound to a document yet".into())
        })
    }

    fn number(args: &IntrospectValue) -> Result<u32, InvokeError> {
        match args {
            IntrospectValue::Int(n) => u32::try_from(*n)
                .map_err(|_| InvokeError::Rejected(format!("{n} is not a node id").into())),
            other => Err(InvokeError::Rejected(
                format!("expected an int, got {other:?}").into(),
            )),
        }
    }

    fn pair(args: &IntrospectValue) -> Result<Vec<u32>, InvokeError> {
        let IntrospectValue::Text(spec) = args else {
            return Err(InvokeError::Rejected(
                "expected \"<from node>.<port>,<to node>.<port>\"".into(),
            ));
        };
        let parsed: Option<Vec<u32>> = spec
            .split([',', '.'])
            .map(|part| part.trim().parse::<u32>().ok())
            .collect();
        parsed
            .filter(|v| v.len() == 4)
            .ok_or_else(|| InvokeError::Rejected(format!("{spec:?} is not <n>.<p>,<n>.<p>").into()))
    }
}

impl ExternalIntrospect for FlowOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("nodes", "int"),
                    SchemaField::new("links", "int"),
                    SchemaField::new("valid", "string"),
                    // The control plane's own questions -- none of which a
                    // dataflow document can be asked.
                    SchemaField::new("entries", "string"),
                    SchemaField::new("trace", "string"),
                    SchemaField::new("steps", "int"),
                    SchemaField::new("stop", "string"),
                    SchemaField::new("budget", "int"),
                    SchemaField::new("control_loops", "string"),
                    SchemaField::new("cycle_nodes", "string"),
                    SchemaField::new("pure_nodes", "string"),
                    SchemaField::new("never_ran", "string"),
                    SchemaField::new("port_flows", "string"),
                    SchemaField::new("last_refusal", "string"),
                    // R1600 -- what the graph REMEMBERS, and where control
                    // went. Neither is readable from the document alone.
                    SchemaField::new("ticks", "int"),
                    SchemaField::new("registers", "string"),
                    SchemaField::new("delays", "string"),
                    SchemaField::new("trace_instances", "string"),
                    SchemaField::new("entered", "string"),
                    SchemaField::new("at_fixed_point", "string"),
                    SchemaField::new("ticks_to_finish", "int"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let state = self.state.as_ref()?;
        let document = state.document.get();
        let machine = state.machine.get();
        let budget = state.budget.get();
        let (trace, stop) = current_run(&document, &machine, budget);
        let int = |v: usize| Some(IntrospectValue::Int(i64::try_from(v).unwrap_or(i64::MAX)));
        let ids = |list: &[NodeId]| {
            IntrospectValue::Text(
                list.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            )
        };
        match path {
            "nodes" => int(document
                .tree(TREE)
                .map_or(0, pinion_node_graph::Tree::node_count)),
            "links" => int(document.tree(TREE).map_or(0, |t| t.links().len())),
            "valid" => Some(IntrospectValue::Text(if document.validate().is_empty() {
                "ok".to_owned()
            } else {
                format!("{:?}", document.validate())
            })),
            "entries" => Some(ids(&document.entry_points(TREE))),
            "trace" => Some(ids(&trace.iter().map(|(_, id)| *id).collect::<Vec<_>>())),
            "steps" => int(trace.len()),
            "stop" => Some(IntrospectValue::Text(
                match stop {
                    Some(Stop::Halted) => "halted",
                    Some(Stop::BudgetExhausted) => "budget_exhausted",
                    None => "no_entry",
                }
                .to_owned(),
            )),
            "budget" => int(budget),
            "control_loops" => Some(ids(&document.control_loops(TREE))),
            "cycle_nodes" => Some(ids(&document.cycle_nodes(TREE))),
            "pure_nodes" => {
                let mut pure: Vec<NodeId> = document
                    .tree(TREE)?
                    .nodes()
                    .filter(|node| {
                        document.signature(TREE, node.id).is_some_and(|s| {
                            !s.inputs.iter().any(Port::is_control)
                                && !s.outputs.iter().any(Port::is_control)
                        })
                    })
                    .map(|node| node.id)
                    .collect();
                pure.sort_unstable();
                Some(ids(&pure))
            }
            "never_ran" => {
                let mut cold: Vec<NodeId> = document
                    .tree(TREE)?
                    .nodes()
                    .map(|node| node.id)
                    .filter(|id| !trace.iter().any(|(_, ran)| ran == id))
                    .collect();
                cold.sort_unstable();
                Some(ids(&cold))
            }
            "port_flows" => {
                let mut rows = Vec::new();
                let mut all: Vec<NodeId> =
                    document.tree(TREE)?.nodes().map(|node| node.id).collect();
                all.sort_unstable();
                for id in all {
                    let signature = document.signature(TREE, id)?;
                    let side = |ports: &[Port<Ty, Val>]| {
                        ports
                            .iter()
                            .map(|p| if p.is_control() { "c" } else { "v" })
                            .collect::<String>()
                    };
                    rows.push(format!(
                        "{id}:{}>{}",
                        side(&signature.inputs),
                        side(&signature.outputs)
                    ));
                }
                Some(IntrospectValue::Text(rows.join(",")))
            }
            "last_refusal" => Some(IntrospectValue::Text(state.refusal.get())),
            // R1600 -- the machine, read in its own function so this one stays
            // about the document.
            _ => Self::machine_read(&document, &machine, budget, path),
        }
    }

    /// R1566 — a refusal names the CHANNEL: a path this surface publishes as a
    /// read is reported read-only, and only a name it does not publish at all
    /// is reported unknown.
    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "nodes" | "links" | "valid" | "entries" | "trace" | "steps" | "stop" | "budget"
            | "control_loops" | "cycle_nodes" | "pure_nodes" | "never_ran" | "port_flows"
            | "last_refusal" | "ticks" | "registers" | "delays" | "trace_instances" | "entered"
            | "at_fixed_point" | "ticks_to_finish" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let outcome = self.act(path, &args);
        if let (Some(state), Err(InvokeError::Rejected(reason))) = (self.state.as_ref(), &outcome) {
            state.refusal.set(reason.to_string());
        }
        outcome
    }
}

impl FlowOracle {
    /// The machine's own half of the read surface (R1600).
    ///
    /// Split out because it answers about a DIFFERENT thing: `query` above
    /// reads the document, and everything here reads what the graph is
    /// currently holding — which is the same separation `Machine` itself draws
    /// by holding no reference to a document.
    fn machine_read(
        document: &Graph,
        machine: &Machine<Op>,
        budget: usize,
        path: &str,
    ) -> Option<IntrospectValue> {
        let int = |v: usize| Some(IntrospectValue::Int(i64::try_from(v).unwrap_or(i64::MAX)));
        let ids = |list: &[NodeId]| {
            IntrospectValue::Text(
                list.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            )
        };
        let (trace, _) = current_run(document, machine, budget);
        match path {
            // R1600 -- the machine.
            "ticks" => int(machine.ticks()),
            "registers" => Some(IntrospectValue::Text(
                machine
                    .iter()
                    .map(|(instance, node, value)| format!("{instance}@{node}={}", value.number()))
                    .collect::<Vec<_>>()
                    .join(","),
            )),
            "delays" => Some(ids(&document.delays(TREE))),
            // The trace WITH its instances: two runs through one definition are
            // two sets of steps, and flattening them loses which is which.
            //
            // `@` separates the instance from the node ON PURPOSE, and the
            // reason was found by reading this wire rather than this code: an
            // instance prints as `/0:10` and a node as `6`, so concatenating
            // them gives `/0:106`, which is also what instance `/0:106` at the
            // root would print. A composite address needs its own separator.
            "trace_instances" => Some(IntrospectValue::Text(
                trace
                    .iter()
                    .map(|(instance, node)| format!("{instance}@{node}"))
                    .collect::<Vec<_>>()
                    .join(","),
            )),
            "entered" => Some(IntrospectValue::Text(
                document
                    .run_on(TREE, *document.entry_points(TREE).first()?, budget, machine)
                    .map(|run| {
                        run.entered()
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default(),
            )),
            // Whether ticking again would change anything -- asked without
            // advancing, by ticking a COPY. The machine is a value, so this
            // costs a clone and no side effect.
            "at_fixed_point" => {
                let mut probe = machine.clone();
                Some(IntrospectValue::Text(
                    if document.tick(TREE, &mut probe).changed() == 0 {
                        "yes".to_owned()
                    } else {
                        "no".to_owned()
                    },
                ))
            }
            // How many more ticks until the scenario HALTS: the question the
            // whole round exists to make answerable, and it is answered on a
            // copy so asking it does not move the world.
            "ticks_to_finish" => {
                let mut probe = machine.clone();
                let mut taken = 0_usize;
                while taken <= TICK_HORIZON {
                    if current_run(document, &probe, budget).1 == Some(Stop::Halted) {
                        break;
                    }
                    document.tick(TREE, &mut probe);
                    taken += 1;
                }
                int(taken)
            }
            _ => None,
        }
    }

    /// The machine's own verbs (R1600).
    fn act_on_machine(
        state: &Rc<FlowState>,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // R1600 -- advance every register once, simultaneously.
            "tick" => {
                let document = state.document.get();
                let mut machine = state.machine.get();
                let tick = document.tick(TREE, &mut machine);
                let moved = tick.changed();
                let at = tick.at();
                let dropped = tick.dropped().len();
                state.machine.set(machine);
                state.refusal.set(String::new());
                Ok(IntrospectValue::Text(format!(
                    "tick {at}: {moved} moved, {dropped} dropped"
                )))
            }
            // Tick until nothing changes, or the budget runs out. The last tick
            // still moving IS "did not converge".
            "settle" => {
                let budget = Self::number(args)? as usize;
                let document = state.document.get();
                let mut machine = state.machine.get();
                let taken = document.settle(TREE, &mut machine, budget);
                let converged = taken.last().is_some_and(|last| last.changed() == 0);
                let count = taken.len();
                state.machine.set(machine);
                state.refusal.set(String::new());
                Ok(IntrospectValue::Text(format!(
                    "{count} tick(s), converged: {converged}"
                )))
            }
            // Write a register directly -- the debugger's verb, and the one
            // thing that moves state without a tick.
            "force" => {
                let IntrospectValue::Text(spec) = args else {
                    return Err(InvokeError::Rejected("expected \"<node>,<value>\"".into()));
                };
                let mut parts = spec.split(',');
                let id = parts
                    .next()
                    .and_then(|p| p.trim().parse::<u32>().ok())
                    .ok_or_else(|| InvokeError::Rejected(format!("{spec:?}: no node id").into()))?;
                let value = parts
                    .next()
                    .and_then(|p| p.trim().parse::<i64>().ok())
                    .ok_or_else(|| InvokeError::Rejected(format!("{spec:?}: no value").into()))?;
                // R1601.1 — the check is the framework's, not a second copy
                // here: only the document knows the node is a register and what
                // that register can hold.
                let document = state.document.get();
                let mut machine = state.machine.get();
                let was = document
                    .force(
                        &mut machine,
                        &Instance::root(),
                        NodeId(id),
                        Val::Number(value),
                    )
                    .map_err(|e| InvokeError::Rejected(e.to_string().into()))?;
                state.machine.set(machine);
                state.refusal.set(String::new());
                Ok(IntrospectValue::Text(match was {
                    Some(before) => format!("was {}", before.number()),
                    None => "was unset".to_owned(),
                }))
            }
            // Back to tick zero, keeping the document. The scenario's RESTART,
            // which is a different verb from rebuilding the graph.
            "rewind" => {
                let mut machine = state.machine.get();
                machine.reset();
                state.machine.set(machine);
                state.refusal.set(String::new());
                Ok(IntrospectValue::Text("rewound".to_owned()))
            }
            other => Err(InvokeError::Rejected(format!("no verb {other:?}").into())),
        }
    }

    fn act(&mut self, path: &str, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?.clone();
        match path {
            "wire" => {
                let parts = Self::pair(args)?;
                let mut document = state.document.get();
                let made = document
                    .connect(
                        TREE,
                        Socket::new(NodeId(parts[0]), parts[1]),
                        Socket::new(NodeId(parts[2]), parts[3]),
                    )
                    .map_err(|e| InvokeError::Rejected(e.to_string().into()))?;
                state.document.set(document);
                state.refusal.set(String::new());
                Ok(IntrospectValue::Text(match made.displaced {
                    // Which end gave way is the round's whole asymmetry, so it
                    // is reported rather than left to be inferred.
                    Some(link) => format!("linked, displacing {}->{}", link.from, link.to),
                    None => "linked".to_owned(),
                }))
            }
            "unwire" => {
                let id = Self::number(args)?;
                let mut document = state.document.get();
                let link = document
                    .disconnect(TREE, pinion_node_graph::LinkId(id))
                    .map_err(|e| InvokeError::Rejected(e.to_string().into()))?;
                state.document.set(document);
                state.refusal.set(String::new());
                Ok(IntrospectValue::Text(format!("{}->{}", link.from, link.to)))
            }
            "set_budget" => {
                let budget = Self::number(args)?;
                state.budget.set(budget as usize);
                Ok(IntrospectValue::Int(i64::from(budget)))
            }
            "set_reading" => {
                let IntrospectValue::Text(spec) = args else {
                    return Err(InvokeError::Rejected("expected \"<node>,<value>\"".into()));
                };
                let mut parts = spec.split(',');
                let id = parts
                    .next()
                    .and_then(|p| p.trim().parse::<u32>().ok())
                    .ok_or_else(|| InvokeError::Rejected(format!("{spec:?}: no node id").into()))?;
                let value = parts
                    .next()
                    .and_then(|p| p.trim().parse::<i64>().ok())
                    .ok_or_else(|| InvokeError::Rejected(format!("{spec:?}: no value").into()))?;
                let mut document = state.document.get();
                document
                    .set_kind(TREE, NodeId(id), Op::Reading(value))
                    .map_err(|e| InvokeError::Rejected(e.to_string().into()))?;
                state.document.set(document);
                state.refusal.set(String::new());
                Ok(IntrospectValue::Int(value))
            }
            "bypass" => {
                let id = Self::number(args)?;
                let mut document = state.document.get();
                let was = document
                    .set_bypassed(TREE, NodeId(id), true)
                    .map_err(|e| InvokeError::Rejected(e.to_string().into()))?;
                state.document.set(document);
                state.refusal.set(String::new());
                Ok(IntrospectValue::Text(format!("was {was}")))
            }
            // R1600 -- the machine's own verbs, in their own function for the
            // same reason its reads are: they act on what the graph is DOING
            // rather than on what it is.
            "tick" | "settle" | "force" | "rewind" => Self::act_on_machine(&state, path, args),
            "reset" => {
                let (document, _) = seed();
                state.document.set(document);
                state.machine.set(Machine::new());
                state.budget.set(STEP_BUDGET);
                state.refusal.set(String::new());
                Ok(IntrospectValue::Text("reset".to_owned()))
            }
            other => Err(InvokeError::Rejected(format!("no verb {other:?}").into())),
        }
    }
}

impl External for FlowOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

// ------------------------------------------------------------------ widget

struct NodeFlowView;

impl WidgetCore for NodeFlowView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = FlowOracle::new();
        oracle.attach(use_flow_state());
        Box::new(oracle)
    }

    fn tag() -> &'static str {
        VIEW_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(_state: (), _frame: &Frame) -> Scene {
        view()
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-node-flow (R1599 §5.38 §5.52)"
    }
}

impl WidgetA11y for NodeFlowView {
    /// The execution order is the whole subject, and it is the state a user who
    /// cannot see the screen has no other way to get.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_flow_state();
        let document = state.document.get();
        let machine = state.machine.get();
        let (trace, stop) = current_run(&document, &machine, state.budget.get());
        vec![
            AccessNode::new(VIEW_TAG, AriaRole::Group)
                .with_name("Scenario control graph")
                .with_value(AccessValue::Text(format!(
                    "tick {}, {} steps, {}, {} node(s) on a control loop, \
                     {} register(s) held",
                    machine.ticks(),
                    trace.len(),
                    match stop {
                        Some(Stop::Halted) => "halted",
                        Some(Stop::BudgetExhausted) => "budget exhausted",
                        None => "no entry point",
                    },
                    document.control_loops(TREE).len(),
                    machine.len(),
                ))),
        ]
    }
}

impl WidgetView for NodeFlowView {
    type Renderer = HelloNodeFlowRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<NodeFlowView>();
}

#[cfg(test)]
mod tests;
