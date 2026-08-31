//! R1602 — a census verdict is proven by a test.
//!
//! # Why this file exists
//!
//! R1601 made the reference census a *tool*: `tools/reference_census.py` enumerates what the DCC and
//! the engine register and `docs/reference-census.json` carries one verdict per operator. What that
//! tool proves is **completeness** — no reference operator is silently
//! absorbed into a percentage. What it does not prove, and cannot, is that any
//! verdict is **true**.
//!
//! The asymmetry is what makes that dangerous. A wrong `absent` leaves a fake
//! item on the gap list and the next round discovers it by trying to close it,
//! so it self-corrects. A wrong `have` **inflates the coverage number and
//! nobody trips over it** — and the error R1601 itself corrected was exactly
//! that direction, 93% down to 78%.
//!
//! So `covered_by` naming an API is a hand claim, and the ones most worth
//! doubting are the claims that a capability falls out of a *composition*
//! ("call `expose` again", "there is nothing to sync"). A composition is a
//! **hypothesis until it is run** — R1571 already learned that by building
//! R1555's written prescription and finding it harmful.
//!
//! This file is the answer: every `have` verdict this crate is responsible for
//! names a test here, the test exercises the capability **through the public
//! API only** (the position an application is in), and the pin and the proofs
//! are asserted to be in bijection. The coverage number is then something
//! `cargo test` re-derives rather than something a session remembered.
//!
//! # What a proof has to do
//!
//! Reach the capability the reference operator names, and assert an outcome
//! that would differ if the capability were missing. A proof that only calls a
//! method and unwraps it proves the method compiles.
//!
//! Three verdicts are **not** here and are not missing: `select_box`,
//! `select_circle` and `select_lasso` are region tests over
//! *drawn* geometry, which `select.rs` measured as belonging to the scene layer
//! rather than to a node model. Their proofs live where the capability does, in
//! `pinion-core`, and the pin says so — which is why a proof is addressed
//! `<crate>::<test>` rather than by a bare name.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use pinion_node_graph::{
    Act, Align, Appearance, Axis, Command, ConnectError, Conversion, Crossings, Definitions,
    Described, Direction, Distribute, Document, Edge, EditError, EditPath, Extent, Faces, Fragment,
    Grow, Hidden, Instance, InterfacePort, InterfaceSide, Item, ItemError, LinkId, Machine,
    Matched, Multiplicity, Node, NodeBody, NodeId, NodeKind, NodeSite, NotRecombinable,
    NotSplittable, Port, PortPath, PortRef, PortSite, PutAway, ROOT, Reach, RelinkError, SectionId,
    Session, Sharing, Side, Socket, Stack, Straighten, Stride, SwitchRefusal, Tint, TreeId,
    Variadic, WatchError, palette_of, type_palette,
};

// ---------------------------------------------------------------- taxonomy

/// Two socket types, so type disagreement is reachable.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Ty {
    Number,
    Text,
    /// R1912 — a COMPOSITE: two numbers under one port, the shape a split takes
    /// apart.
    Pair,
    /// R1912 — a container OF that composite. Its element splits and the
    /// reference refuses the container anyway, so this is what lets a proof
    /// tell "container" from "atom" instead of collapsing both into one no.
    Bag,
    /// ★ R1925 — this taxonomy's **two-state** type, declared through
    /// [`NodeKind::switch_type`]. Its own member and not a re-use of `Number`,
    /// because the whole point of the declaration is that only one type may
    /// switch a section: a fixture where the switchable type were also the
    /// commonest one could not tell the refusal from the acceptance.
    Flag,
}

/// The application's own taxonomy — the half this crate deliberately does not
/// own. Small, and every member is here because some proof needs its shape.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
enum Op {
    /// A constant. No inputs, so it is a source.
    Num(i64),
    /// A constant of the other type, so a refused wire is reachable.
    Word(String),
    /// `(Augend: Number = 0, Addend: Number = 1) -> Out: Number`. Both inputs
    /// carry a default, which is what makes a cleared authored value
    /// observable as a *value* rather than as an absence.
    Add,
    /// `(Augend: Number = 2, Factor: Number) -> Out: Number`. `Augend` matches `Add`'s by name and `Factor` does not, and `Factor` has **no
    /// default** — the shape the engine's "hide pins with no connection and no
    /// default" needs.
    Mul,
    /// `(Value: Number) -> Out: Number`. One in, one out: the shape a dissolve
    /// and a bypass are about.
    Double,
    /// `(Phrase: Text) -> Out: Text`.
    Shout,
    /// `(Result: Number) -> ()`. A sink, so a graph has an end.
    Sink,
    /// R1632 — `Execute ->| Then 0, Then 1, ..`. The engine's execution
    /// sequence: a variadic run of **control** outputs, floor two, no ceiling.
    Sequence,
    /// R1632 — `(Option 0, Option 1, .., Index) -> Out`. Its selector, whose
    /// `Index` input sits **after** the run, so the run's length decides that
    /// port's own address.
    Choose,
    /// R1632 — `(Base, Pose 0, Weight 0, Pose 1, Weight 1, .., Bias) -> Out`.
    /// Its blend list: **two** ports per item, from two parallel arrays, with a
    /// fixed port on each side of the run.
    Blend,
    /// R1632 — `(Member 0, Member 1, ..) -> Out: Text`. The DCC's socket
    /// items: each item carries its **own** name and socket type, which is what
    /// all but one of that reference's accessors declare and what the engine
    /// cannot express.
    Bundle,
    /// R1644 — `In ->| Then, Cost: Number`. A node that **runs and carries a
    /// value**, which no other member is: the debugging proofs need a
    /// breakpoint and a watch to be able to land on one node, and every control
    /// kind above is control-only while every valued kind above is pure.
    Stage(i64),
    /// R1912 — `(Go: control, Whole: Pair, Loose: Bag) -> Out: Pair`. The one
    /// kind that makes every arm of the split question reachable.
    Carry,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
enum Val {
    Number(i64),
    Text(String),
}

impl Val {
    fn number(&self) -> Option<i64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::Text(_) => None,
        }
    }
}

impl NodeKind for Op {
    type Type = Ty;
    type Value = Val;

    fn name(&self) -> String {
        match self {
            Self::Num(_) => "Num",
            Self::Word(_) => "Word",
            Self::Add => "Add",
            Self::Mul => "Mul",
            Self::Double => "Double",
            Self::Shout => "Shout",
            Self::Sink => "Sink",
            Self::Sequence => "Sequence",
            Self::Choose => "Choose",
            Self::Blend => "Blend",
            Self::Bundle => "Bundle",
            Self::Stage(_) => "Stage",
            Self::Carry => "Carry",
        }
        .to_owned()
    }

    /// R1912 — one composite type and one container of it, so the three arms
    /// are all reachable and a proof can tell the two refusals apart.
    fn composition(ty: &Ty) -> pinion_node_graph::Composition<Ty, Val> {
        use pinion_node_graph::Composition;
        match ty {
            Ty::Pair => Composition::Members(vec![
                Port::new("Left", Ty::Number).with_default(Val::Number(0)),
                Port::new("Right", Ty::Number).with_default(Val::Number(0)),
            ]),
            Ty::Bag => Composition::Container,
            Ty::Number | Ty::Text | Ty::Flag => Composition::Atom,
        }
    }

    /// ★ R1925 — the two-state type a section switch carries.
    fn switch_type() -> Option<Ty> {
        Some(Ty::Flag)
    }

    /// ★★★★★ R1927 — when a node of this taxonomy is in a questionable state.
    ///
    /// Shaped after the reference's own rule rather than invented: its one
    /// overrider that consults the graph answers from **whether a particular
    /// pin of its own is wired**, and reaches that fact by climbing out of
    /// itself because its signature hands it nothing. Here it is the argument.
    ///
    /// `Sink` is the only kind that warns, so a proof can tell a rule that
    /// fires from one that fires for everything.
    fn warning(&self, around: &pinion_node_graph::Surroundings) -> Option<String> {
        match self {
            Self::Sink if !around.is_wired(Side::Input, 0) => {
                Some("nothing reaches this sink, so it consumes nothing".to_owned())
            }
            _ => None,
        }
    }

    /// ★★★★★ R1926 — what colour each type is drawn in.
    ///
    /// `Text` says **nothing** on purpose, and it is an ATOM: a taxonomy that
    /// colours some of its types and not others is the ordinary case, and a
    /// fixture where every type spoke could not tell an undeclared colour from
    /// a declared one. `Bag` is a CONTAINER and *is* coloured, so a proof can
    /// tell "no member colours because the composition has no members" from
    /// "no member colours because nothing is coloured".
    fn type_colour(ty: &Ty) -> Option<Tint> {
        match ty {
            Ty::Number => Some(Tint::rgb(0x2D, 0x6C, 0xDF)),
            Ty::Pair => Some(Tint::rgb(0x7C, 0x4D, 0xEF)),
            Ty::Bag => Some(Tint::rgb(0xC7, 0x78, 0x00)),
            Ty::Flag => Some(Tint::rgb(0x1F, 0x8A, 0x4C)),
            Ty::Text => None,
        }
    }

    /// ★ R1926 — and the control plane's, which is a second declaration here
    /// because control is not a type (R1599).
    fn control_colour() -> Option<Tint> {
        Some(Tint::rgb(0xEC, 0x5A, 0xA0))
    }

    /// ★ R1916 — what a value of each type IS. `Bag` says nothing on purpose:
    /// a taxonomy that describes some of its types and not others is the
    /// ordinary case, and a fixture where every type spoke could not tell a
    /// missing half from a present one.
    fn type_description(ty: &Ty) -> Option<String> {
        match ty {
            Ty::Number => Some("a whole number".to_owned()),
            Ty::Text => Some("a line of text".to_owned()),
            Ty::Pair => Some("two numbers written `left|right`".to_owned()),
            Ty::Flag => Some("on, or off".to_owned()),
            Ty::Bag => None,
        }
    }

    /// R1913/R1914 — a pair is written `left|right`, so taking it apart is a
    /// split on the bar and putting it back is a join.
    ///
    /// Declared here as well as in the crate's own fixture because these are
    /// two different populations: this one exercises the reference census
    /// through the **public API only**, and a `have` verdict proved against an
    /// internal fixture is a verdict proved against something a consumer cannot
    /// reach.
    fn explode(ty: &Ty, value: &Val) -> Vec<Option<Val>> {
        match (ty, value) {
            (Ty::Pair, Val::Text(written)) => written
                .split('|')
                .map(|part| part.trim().parse::<i64>().ok().map(Val::Number))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn implode(ty: &Ty, members: &[Option<Val>]) -> Option<Val> {
        if *ty != Ty::Pair || members.len() != 2 {
            return None;
        }
        let part = |slot: &Option<Val>| match slot {
            Some(Val::Number(n)) => Some(n.to_string()),
            _ => None,
        };
        Some(Val::Text(format!(
            "{}|{}",
            part(&members[0])?,
            part(&members[1])?
        )))
    }

    fn inputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            // `Bundle` is here for the same reason the sources are: its members
            // are the NODE's, so its kind declares no fixed input at all.
            Self::Num(_) | Self::Word(_) | Self::Bundle => Vec::new(),
            Self::Add => vec![
                Port::new("Augend", Ty::Number).with_default(Val::Number(0)),
                Port::new("Addend", Ty::Number).with_default(Val::Number(1)),
            ],
            Self::Mul => vec![
                Port::new("Augend", Ty::Number).with_default(Val::Number(2)),
                Port::new("Factor", Ty::Number),
            ],
            Self::Double => vec![Port::new("Value", Ty::Number)],
            Self::Shout => vec![Port::new("Phrase", Ty::Text)],
            Self::Sink => vec![Port::new("Result", Ty::Number)],
            // R1632 — the FIXED half of a variadic kind. What repeats is
            // declared once, in `variadic`, so these two can never disagree
            // about where the run is.
            Self::Sequence | Self::Stage(_) => vec![Port::control("Execute")],
            Self::Choose => vec![Port::new("Index", Ty::Number).with_default(Val::Number(0))],
            Self::Blend => vec![
                Port::new("Base", Ty::Number).with_default(Val::Number(0)),
                Port::new("Bias", Ty::Number).with_default(Val::Number(0)),
            ],
            Self::Carry => vec![
                Port::control("Go"),
                // ★ R1914 — a resting value, so a split has something to share
                // out and a recombine has something to put back. Without one,
                // the act would be provable only in its shape.
                // ★ R1916 — and a sentence of its own, which is a different
                // fact from what its TYPE says: two ports of one type can be
                // for different things, and the reference has nowhere for the
                // distinction to live.
                Port::new("Whole", Ty::Pair)
                    .with_default(Val::Text("3|4".to_owned()))
                    .describing("the pair this node carries"),
                Port::new("Loose", Ty::Bag),
                // ★ R1914 — a port AFTER the composite one that can hold a
                // value, so "everything after the split moves, with what was
                // authored on it" is assertable. `Loose` cannot: no `Val`
                // classifies as `Bag`, which is deliberate elsewhere and
                // exactly why the fixture needed a fourth port rather than a
                // cleverer assertion about the third.
                Port::new("Tail", Ty::Number).with_default(Val::Number(0)),
            ],
        }
    }

    /// R1632 — which run of the ports above is the node's rather than the
    /// kind's.
    fn variadic(&self, side: Side) -> Option<Variadic<Ty, Val>> {
        match (self, side) {
            (Self::Sequence, Side::Output) => {
                Some(Variadic::at(0, vec![Port::control("Then")]).at_least(2))
            }
            (Self::Choose, Side::Input) => Some(
                Variadic::at(0, vec![Port::new("Option", Ty::Number)])
                    .at_least(2)
                    .at_most(4),
            ),
            (Self::Blend, Side::Input) => Some(
                Variadic::at(
                    1,
                    vec![
                        Port::new("Pose", Ty::Number),
                        Port::new("Weight", Ty::Number),
                    ],
                )
                .at_least(1),
            ),
            (Self::Bundle, Side::Input) => {
                Some(Variadic::at(0, vec![Port::new("Member", Ty::Number)]).at_least(1))
            }
            _ => None,
        }
    }

    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Num(_) | Self::Add | Self::Mul | Self::Double => {
                vec![Port::new("Out", Ty::Number)]
            }
            Self::Word(_) | Self::Shout | Self::Bundle => vec![Port::new("Out", Ty::Text)],
            Self::Choose | Self::Blend => vec![Port::new("Out", Ty::Number)],
            Self::Sink | Self::Sequence => Vec::new(),
            Self::Stage(_) => vec![Port::control("Then"), Port::new("Cost", Ty::Number)],
            Self::Carry => vec![Port::new("Out", Ty::Pair)],
        }
    }

    /// A **directed** relation, which is why it is here and not an equality: a
    /// number reads as text and text does not read back as a number. Both of
    /// the reference hooks this answers — the engine's `CreateAutomaticConversionNodeAndConnections` and the DCC's `the tree type::validate_link` —
    /// need exactly this asymmetry, and it was R1593's subject.
    fn conversion(from: &Ty, to: &Ty) -> Conversion<Val> {
        match (from, to) {
            // R1912 — the composite and its container join the identity arm:
            // each is DIRECT to itself and reaches nothing else.
            (Ty::Number, Ty::Number)
            | (Ty::Text, Ty::Text)
            | (Ty::Pair, Ty::Pair)
            | (Ty::Bag, Ty::Bag)
            | (Ty::Flag, Ty::Flag) => Conversion::Direct,
            (Ty::Number, Ty::Text) => Conversion::Converted(|value| match value {
                Val::Number(n) => Some(Val::Text(n.to_string())),
                text @ Val::Text(_) => Some(text),
            }),
            // ⚠ Written out rather than as `_`, so a type added next is a
            // compile error here — the rule this match already followed.
            (Ty::Text, Ty::Number)
            | (Ty::Pair | Ty::Bag | Ty::Flag, _)
            | (Ty::Number | Ty::Text, Ty::Pair | Ty::Bag | Ty::Flag) => Conversion::Refused,
        }
    }

    fn value_type(value: &Val) -> Option<Ty> {
        Some(match value {
            Val::Number(_) => Ty::Number,
            Val::Text(_) => Ty::Text,
        })
    }

    fn evaluate(&self, inputs: &[Option<Val>]) -> Vec<Option<Val>> {
        let number = |i: usize| inputs.get(i).and_then(Option::as_ref).and_then(Val::number);
        match self {
            Self::Num(n) => vec![Some(Val::Number(*n))],
            Self::Word(w) => vec![Some(Val::Text(w.clone()))],
            Self::Add => vec![number(0).zip(number(1)).map(|(a, b)| Val::Number(a + b))],
            Self::Mul => vec![number(0).zip(number(1)).map(|(a, b)| Val::Number(a * b))],
            Self::Double => vec![number(0).map(|v| Val::Number(v * 2))],
            Self::Shout => vec![inputs.first().and_then(Option::as_ref).map(|v| match v {
                Val::Text(t) => Val::Text(t.to_uppercase()),
                other @ Val::Number(_) => other.clone(),
            })],
            Self::Sink | Self::Sequence => Vec::new(),
            // Slot 0 is the control output and carries nothing: control is not
            // a value, which is why watching that port is refused.
            Self::Stage(cost) => vec![None, Some(Val::Number(*cost))],
            // R1912 — this kind exists for the split QUESTION, not for
            // evaluation; its composite input passes straight out.
            Self::Carry => vec![inputs.get(1).cloned().flatten()],
            // R1632 — `inputs` is as long as the NODE's resolved signature, so
            // a variadic kind reads the run it declared instead of a fixed
            // arity. That is the whole reason `evaluate` needed no new
            // parameter.
            Self::Choose => {
                let options = inputs.len() - 1;
                let index = number(options).unwrap_or(0);
                vec![
                    usize::try_from(index)
                        .ok()
                        .filter(|i| *i < options)
                        .and_then(number)
                        .map(Val::Number),
                ]
            }
            Self::Blend => {
                let last = inputs.len() - 1;
                let mut total = number(0).unwrap_or(0) + number(last).unwrap_or(0);
                let mut slot = 1;
                while slot + 1 < last {
                    total += number(slot).unwrap_or(0) * number(slot + 1).unwrap_or(0);
                    slot += 2;
                }
                vec![Some(Val::Number(total))]
            }
            Self::Bundle => vec![Some(Val::Text(
                inputs
                    .iter()
                    .map(|slot| match slot {
                        Some(Val::Number(n)) => n.to_string(),
                        Some(Val::Text(t)) => t.clone(),
                        None => "-".to_owned(),
                    })
                    .collect::<Vec<_>>()
                    .join("/"),
            ))],
        }
    }
}

// ----------------------------------------------------------------- helpers

fn num(document: &mut Document<Op>, n: i64) -> NodeId {
    document
        .add_node(ROOT, NodeBody::Kind(Op::Num(n)), 0, 0)
        .unwrap()
}

fn node(document: &mut Document<Op>, op: Op) -> NodeId {
    document.add_node(ROOT, NodeBody::Kind(op), 0, 0).unwrap()
}

fn wire(document: &mut Document<Op>, from: NodeId, out: u32, to: NodeId, input: u32) {
    document
        .connect(ROOT, Socket::new(from, out), Socket::new(to, input))
        .unwrap();
}

/// `two -> add.0`, `three -> add.1`, `add -> sink.0`. The one chain almost every
/// proof needs, so no proof spends its lines building it.
struct Chain {
    document: Document<Op>,
    two: NodeId,
    three: NodeId,
    add: NodeId,
    sink: NodeId,
}

fn chain() -> Chain {
    let mut document = Document::new("root");
    let two = num(&mut document, 2);
    let three = num(&mut document, 3);
    let add = node(&mut document, Op::Add);
    let sink = node(&mut document, Op::Sink);
    wire(&mut document, two, 0, add, 0);
    wire(&mut document, three, 0, add, 1);
    wire(&mut document, add, 0, sink, 0);
    Chain {
        document,
        two,
        three,
        add,
        sink,
    }
}

/// What actually arrives at a socket, evaluated end to end.
///
/// The chain ends in a sink, which has no outputs — so "the graph still
/// computes what it computed" is a statement about what reaches the sink's
/// input rather than about what it answers, and this is the accessor that makes
/// that assertable.
fn arrives(document: &Document<Op>, socket: Socket) -> Option<Val> {
    let mut evaluator = document.evaluator();
    evaluator.input(ROOT, socket)
}

/// Every link id touching `node`, either end. What a "break all links on this
/// node" gesture names, computed the way an application would.
fn links_touching(document: &Document<Op>, tree: TreeId, target: NodeId) -> Vec<LinkId> {
    document
        .tree(tree)
        .unwrap()
        .links()
        .iter()
        .filter(|link| link.from.node == target || link.to.node == target)
        .map(|link| link.id)
        .collect()
}

// -------------------------------------------------------------- the pin

/// The committed judgement, read at test time so the two cannot drift.
const PIN: &str = include_str!("../../../docs/reference-census.json");

/// Which crate's census this file is.
const CRATE: &str = "pinion-node-graph";

#[derive(Deserialize)]
struct Row {
    verdict: String,
    #[serde(default)]
    proven_by: String,
}

/// The proof for each `have` verdict this crate answers for.
///
/// The third element is the proof itself, so the compiler — not a grep — is
/// what checks it exists. Its *name* is derived from the operator by
/// [`proof_name`], so a row and its proof cannot end up describing different
/// capabilities without the bijection test saying so.
/// One `have` verdict and the proof that runs it.
struct Proof {
    tree: &'static str,
    operator: &'static str,
    /// The proof's own name, taken from the **compiler**.
    name: &'static str,
    run: Box<dyn Fn()>,
}

/// Bind a verdict to its proof, with the proof's name read off the function
/// itself rather than transcribed beside it.
///
/// ★ This shape was chosen by a counterfactual. The first draft stored a bare
/// `fn()` pointer, and renaming a proof was **not caught**: the bijection
/// compared operator names on both sides and the derived name against the pin,
/// so the function's own identifier was never in the comparison at all and
/// `proven_by` could name a test that no longer existed. `F` here is the
/// function's *item* type, so `type_name_of_val` answers with its path — the
/// identifier is written once, at the call site, and the string comes from the
/// compiler.
fn proof<F: Fn() + 'static>(tree: &'static str, operator: &'static str, run: F) -> Proof {
    let path = std::any::type_name_of_val(&run);
    Proof {
        tree,
        operator,
        name: path.rsplit("::").next().unwrap_or(path),
        run: Box::new(run),
    }
}

/// The proof for each `have` verdict this crate answers for.
///
/// Split by reference only because one list of sixty-one is past the length
/// this project lets a function have; the two are one table.
fn proofs() -> Vec<Proof> {
    let mut all = dcc_proofs();
    all.extend(dcc_hook_proofs());
    all.extend(engine_proofs());
    all.extend(engine_permission_proofs());
    all.extend(engine_relink_proofs());
    all.extend(colour_proofs());
    all.extend(admission_proofs());
    all.extend(description_proofs());
    all.extend(engine_wire_proofs());
    all.extend(engine_editor_proofs());
    all.extend(engine_hook_proofs());
    all.extend(engine_arrangement_proofs());
    all.extend(engine_schema_hook_proofs());
    all.extend(engine_variadic_proofs());
    all.extend(engine_debug_proofs());
    all.extend(engine_description_proofs());
    all.extend(dcc_item_proofs());
    all
}

/// R1632 — the VARIADIC-PORT cluster: the engine's ten commands for a node
/// whose port count is its own. Its own registry for the reason the
/// arrangement one has its own — the others are at the line ceiling, and these
/// are one capability.
fn engine_variadic_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "GraphEditor::AddExecutionPin",
            engine_graph_editor_add_execution_pin,
        ),
        proof(
            "engine",
            "GraphEditor::InsertExecutionPinBefore",
            engine_graph_editor_insert_execution_pin_before,
        ),
        proof(
            "engine",
            "GraphEditor::InsertExecutionPinAfter",
            engine_graph_editor_insert_execution_pin_after,
        ),
        proof(
            "engine",
            "GraphEditor::RemoveExecutionPin",
            engine_graph_editor_remove_execution_pin,
        ),
        proof(
            "engine",
            "GraphEditor::AddOptionPin",
            engine_graph_editor_add_option_pin,
        ),
        proof(
            "engine",
            "GraphEditor::RemoveOptionPin",
            engine_graph_editor_remove_option_pin,
        ),
        proof(
            "engine",
            "SoundCueGraph::DeleteInput",
            engine_sound_cue_graph_delete_input,
        ),
        proof(
            "engine",
            "AnimGraph::AddBlendListPin",
            engine_anim_graph_add_blend_list_pin,
        ),
        proof(
            "engine",
            "AnimGraph::RemoveBlendListPin",
            engine_anim_graph_remove_blend_list_pin,
        ),
        proof(
            "engine",
            "node::CreateUniquePinName",
            engine_node_create_unique_pin_name,
        ),
    ]
}

/// R1632 — and the DCC's half: its generic socket-item operators, which are a
/// template over per-node socket lists rather than a per-node-type routine.
fn dcc_item_proofs() -> Vec<Proof> {
    vec![
        proof(
            "dcc",
            "socket_items::make_add_item_operator",
            dcc_socket_items_make_add_item_operator,
        ),
        proof(
            "dcc",
            "socket_items::make_remove_item_by_index_operator",
            dcc_socket_items_make_remove_item_by_index_operator,
        ),
        proof(
            "dcc",
            "socket_items::make_move_item_operator",
            dcc_socket_items_make_move_item_operator,
        ),
    ]
}

fn dcc_proofs() -> Vec<Proof> {
    vec![
        proof("dcc", "add_empty_group", dcc_add_empty_group),
        proof("dcc", "hide_socket_toggle", dcc_hide_socket_toggle),
        proof("dcc", "add_group", dcc_add_group),
        proof("dcc", "add_group_input_node", dcc_add_group_input_node),
        proof("dcc", "attach", dcc_attach),
        proof("dcc", "clipboard_copy", dcc_clipboard_copy),
        proof("dcc", "clipboard_paste", dcc_clipboard_paste),
        proof(
            "dcc",
            "collapse_hide_unused_toggle",
            dcc_collapse_hide_unused_toggle,
        ),
        proof("dcc", "find_node", dcc_find_node),
        proof("dcc", "delete", dcc_delete),
        proof("dcc", "delete_reconnect", dcc_delete_reconnect),
        proof("dcc", "detach", dcc_detach),
        proof("dcc", "duplicate", dcc_duplicate),
        proof("dcc", "group_edit", dcc_group_edit),
        proof("dcc", "group_enter_exit", dcc_group_enter_exit),
        proof("dcc", "group_insert", dcc_group_insert),
        proof("dcc", "group_make", dcc_group_make),
        proof("dcc", "group_separate", dcc_group_separate),
        proof("dcc", "group_ungroup", dcc_group_ungroup),
        proof("dcc", "hide_toggle", dcc_hide_toggle),
        proof(
            "dcc",
            "interface_item_duplicate",
            dcc_interface_item_duplicate,
        ),
        proof("dcc", "interface_item_new", dcc_interface_item_new),
        proof(
            "dcc",
            "interface_item_new_panel_toggle",
            dcc_interface_item_new_panel_toggle,
        ),
        proof(
            "dcc",
            "interface_item_make_panel_toggle",
            dcc_interface_item_make_panel_toggle,
        ),
        proof("dcc", "interface_item_remove", dcc_interface_item_remove),
        proof("dcc", "join", dcc_join),
        proof("dcc", "join_nodes", dcc_join_nodes),
        proof("dcc", "link", dcc_link),
        proof("dcc", "link_make", dcc_link_make),
        proof("dcc", "links_cut", dcc_links_cut),
        proof("dcc", "links_detach", dcc_links_detach),
        proof("dcc", "links_mute", dcc_links_mute),
        proof("dcc", "mute_toggle", dcc_mute_toggle),
        proof("dcc", "new_node_tree", dcc_new_node_tree),
        proof("dcc", "options_toggle", dcc_options_toggle),
        proof("dcc", "parent_set", dcc_parent_set),
        proof("dcc", "preview_toggle", dcc_preview_toggle),
        proof("dcc", "resize", dcc_resize),
        proof("dcc", "select_grouped", dcc_select_grouped),
        proof("dcc", "select_linked_from", dcc_select_linked_from),
        proof("dcc", "select_linked_to", dcc_select_linked_to),
        proof("dcc", "select_same_type_step", dcc_select_same_type_step),
        proof("dcc", "sockets_sync", dcc_sockets_sync),
        proof("dcc", "swap_node", dcc_swap_node),
        proof("dcc", "tree_path_parent", dcc_tree_path_parent),
    ]
}

/// The HOOK surface (R1603): what the DCC asks a node type, a tree type
/// and a socket type to decide.
fn dcc_hook_proofs() -> Vec<Proof> {
    vec![
        proof("dcc", "node::can_sync_sockets", dcc_node_can_sync_sockets),
        proof("dcc", "node::copyfunc", dcc_node_copyfunc),
        proof("dcc", "node::initfunc", dcc_node_initfunc),
        proof("dcc", "node::labelfunc", dcc_node_labelfunc),
        proof("dcc", "node::updatefunc", dcc_node_updatefunc),
        proof("dcc", "node_tree::localize", dcc_node_tree_localize),
        proof("dcc", "node_tree::update", dcc_node_tree_update),
        proof(
            "dcc",
            "node_tree::validate_link",
            dcc_node_tree_validate_link,
        ),
        proof(
            "dcc",
            "node_socket::interface_from_socket",
            dcc_node_socket_interface_from_socket,
        ),
        proof(
            "dcc",
            "node_socket::interface_init_socket",
            dcc_node_socket_interface_init_socket,
        ),
    ]
}

/// The generic canvas's commands that act on **structure** — which nodes exist,
/// which tree they are in, and whether they take part.
/// ★★★★★ R1921 — a node's own COLOUR: one authored value, four derived faces.
///
/// Its own list for the reason the lists beside it have theirs — `engine_proofs`
/// is at the length this file splits on — and because these rows are one
/// mechanism the pin had already grouped: five of them carried the identical
/// `covered_by` sentence before anyone reached for them.
///
/// Only `node_copy_color` OWNS the proof; the engine's four face rows CITE it.
fn colour_proofs() -> Vec<Proof> {
    vec![
        proof("dcc", "node_copy_color", dcc_node_copy_color),
        // ★★★★★ R1926 — the three PORT colour rows. Three proofs and not one
        // citing two, because the three ask genuinely different questions and
        // the reference's own signatures are what separate them: one is asked
        // with a TYPE and no port (a legend can ask it), one with a PORT, and
        // one is the SECOND colour a composite type is drawn in.
        proof(
            "engine",
            "schema::GetPinTypeColor",
            engine_schema_get_pin_type_color,
        ),
        proof("engine", "schema::GetPinColor", engine_schema_get_pin_color),
        proof(
            "engine",
            "schema::GetSecondaryPinTypeColor",
            engine_schema_get_secondary_pin_type_color,
        ),
    ]
}

/// ★★★★★ R1922 — *would this tree accept this body?* Four rows, one mechanism;
/// only `node::poll` owns the proof and the other three cite it.
fn admission_proofs() -> Vec<Proof> {
    vec![proof("dcc", "node::poll", dcc_node_poll)]
}

/// ★★★★★ R1923 — *what does this node say about itself, and who said it?* Two
/// rows; the engine's tooltip owns the proof and the DCC's hook cites it.
fn description_proofs() -> Vec<Proof> {
    vec![proof(
        "engine",
        "node::GetTooltipText",
        engine_node_get_tooltip_text,
    )]
}

/// ★★★★★ R1920 — the engine's PERMISSION rows: *may this edit be made?*, asked
/// before making it.
///
/// Its own list for the reason the wire/editor lists beside it are: adding to
/// `engine_proofs` put that function past the length this file splits on. And
/// these rows ask a different question from the commands they sat among —
/// those name an EDIT, these name whether one is allowed at all.
///
/// Only the delete row OWNS this proof. `node::GetCanRenameNode` and
/// `schema::CanCreateNewNodes` CITE it in the pin, which is how this file says
/// *one mechanism, several rows* — the same shape R1919's search rows have.
fn engine_permission_proofs() -> Vec<Proof> {
    vec![proof(
        "engine",
        "node::CanUserDeleteNode",
        engine_node_can_user_delete_node,
    )]
}

/// ★★★★★ R1924 — the RELINKING cluster: the engine's three, which are one
/// gesture cut into a start gate, a hover verdict and a commit.
///
/// Its own registry because the three are one capability and because reading
/// them apart is how R1924 found that only *two* of them were ever missing.
/// The commit has been [`Document::relink`] since R1681, so its row had been
/// wrong for 243 rounds — carried under a title that covered all three, which
/// is why nobody measured the clauses separately from it.
fn engine_relink_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "schema::IsConnectionRelinkingAllowed",
            engine_schema_is_connection_relinking_allowed,
        ),
        proof(
            "engine",
            "schema::CanRelinkConnectionToPin",
            engine_schema_can_relink_connection_to_pin,
        ),
        proof(
            "engine",
            "schema::TryRelinkConnectionTarget",
            engine_schema_try_relink_connection_target,
        ),
    ]
}

fn engine_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "GraphEditor::CollapseNodes",
            engine_graph_editor_collapse_nodes,
        ),
        // ★★★★★ R1912 — the three the census filed under struct-pin SPLITTING
        // and which are hiding, measured in the engine's own editor source.
        proof("engine", "node::CanSplitPin", engine_node_can_split_pin),
        // ★★★★★ R1914 — the ACT, which R1912 and R1913 built the question and
        // the value half of. Four rows and four proofs, because the reference
        // spells them as four commands and each says something the others do
        // not: the schema pair is the model's, the editor pair is what a
        // gesture reaches.
        proof("engine", "schema::SplitPin", engine_schema_split_pin),
        proof(
            "engine",
            "schema::RecombinePin",
            engine_schema_recombine_pin,
        ),
        proof(
            "engine",
            "GraphEditor::SplitStructPin",
            engine_graph_editor_split_struct_pin,
        ),
        proof(
            "engine",
            "GraphEditor::RecombineStructPin",
            engine_graph_editor_recombine_struct_pin,
        ),
        proof(
            "engine",
            "GraphEditor::RemoveThisStructVarPin",
            engine_graph_editor_remove_this_struct_var_pin,
        ),
        proof(
            "engine",
            "GraphEditor::RemoveOtherStructVarPins",
            engine_graph_editor_remove_other_struct_var_pins,
        ),
        proof(
            "engine",
            "GraphEditor::RestoreAllStructVarPins",
            engine_graph_editor_restore_all_struct_var_pins,
        ),
        proof(
            "engine",
            "GraphEditor::CollapseSelectionToFunction",
            engine_graph_editor_collapse_selection_to_function,
        ),
        proof(
            "engine",
            "GraphEditor::CollapseSelectionToMacro",
            engine_graph_editor_collapse_selection_to_macro,
        ),
        proof(
            "engine",
            "GraphEditor::CreateComment",
            engine_graph_editor_create_comment,
        ),
        proof(
            "engine",
            "GraphEditor::DeleteAndReconnectNodes",
            engine_graph_editor_delete_and_reconnect_nodes,
        ),
        proof(
            "engine",
            "GraphEditor::DisableNodes",
            engine_graph_editor_disable_nodes,
        ),
        proof(
            "engine",
            "GraphEditor::EnableNodes",
            engine_graph_editor_enable_nodes,
        ),
        proof(
            "engine",
            "GraphEditor::ExpandNodes",
            engine_graph_editor_expand_nodes,
        ),
        proof(
            "engine",
            "GraphEditor::PromoteSelectionToFunction",
            engine_graph_editor_promote_selection_to_function,
        ),
        proof(
            "engine",
            "GraphEditor::PromoteSelectionToMacro",
            engine_graph_editor_promote_selection_to_macro,
        ),
        proof(
            "engine",
            "GraphEditor::ReconstructNodes",
            engine_graph_editor_reconstruct_nodes,
        ),
        proof(
            "engine",
            "GraphEditor::SelectAllInputNodes",
            engine_graph_editor_select_all_input_nodes,
        ),
        proof(
            "engine",
            "GraphEditor::SelectAllOutputNodes",
            engine_graph_editor_select_all_output_nodes,
        ),
    ]
}

/// The generic canvas's commands that act on **ports and wires** — what a node
/// shows and what reaches it.
///
/// Split from the structural half because one list of twenty is past the length
/// this project lets a function have, and this is the seam it already had: a
/// wire is not a node.
fn engine_wire_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "GraphEditor::BreakNodeLinks",
            engine_graph_editor_break_node_links,
        ),
        proof(
            "engine",
            "GraphEditor::BreakPinLinks",
            engine_graph_editor_break_pin_links,
        ),
        proof(
            "engine",
            "GraphEditor::BreakThisLink",
            engine_graph_editor_break_this_link,
        ),
        proof(
            "engine",
            "GraphEditor::HideNoConnectionNoDefaultPins",
            engine_graph_editor_hide_no_connection_no_default_pins,
        ),
        proof(
            "engine",
            "GraphEditor::HideNoConnectionPins",
            engine_graph_editor_hide_no_connection_pins,
        ),
        proof(
            "engine",
            "GraphEditor::ResetPinToDefaultValue",
            engine_graph_editor_reset_pin_to_default_value,
        ),
        proof(
            "engine",
            "GraphEditor::ShowAllPins",
            engine_graph_editor_show_all_pins,
        ),
    ]
}

/// The per-editor command lists R1605 added — the ones whose `have` needs a
/// proof of its own rather than a citation of the generic canvas's.
///
/// There is one. That is the round's measurement rather than a small table:
/// eight editor-specific lists, 152 commands, and every capability among them
/// that this crate answers is answered by a mechanism the generic list already
/// named — so 25 rows CITE and one owns.
fn engine_editor_proofs() -> Vec<Proof> {
    vec![proof(
        "engine",
        "MaterialEditor::MatertialPasteHere",
        engine_material_editor_matertial_paste_here,
    )]
}

/// The engine pastes the clipboard **at a point** rather than back where it
/// came from, and the point has to mean the same thing for one node and for
/// five.
///
/// `Fragment` stores every node's position relative to the selection's centroid
/// (`Fragment::origin`), so `insert(.., at, ..)` puts the *fragment* there and
/// the relative layout is carried untouched. The distinction is invisible with
/// one node — `dcc_clipboard_paste` pastes one and cannot tell an anchor
/// from a per-node override — so this one pastes three at once and asserts both
/// halves: where the group landed, and that its shape survived.
///
/// ★ Past the engine 5.8: the anchor is a **value on the fragment**
/// (`Fragment::origin`), so a client can ask a copied graph where it considers
/// itself to be. The engine's clipboard is a text blob
/// (`graph utilities::ExportNodesToText`) holding absolute node positions, and
/// the averaging that turns it into a paste location lives inside
/// `visual script editor::PasteNodesHere` — so nothing can ask the payload
/// anything.
#[test]
fn engine_material_editor_matertial_paste_here() {
    let mut document: Document<Op> = Document::new("root");
    let left = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(2)), 100, 50)
        .unwrap();
    let middle = document
        .add_node(ROOT, NodeBody::Kind(Op::Double), 300, 50)
        .unwrap();
    let right = document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 500, 90)
        .unwrap();
    wire(&mut document, left, 0, middle, 0);
    wire(&mut document, middle, 0, right, 0);

    let fragment = document.extract(ROOT, &[left, middle, right]).unwrap();
    let pasted = document
        .insert(
            ROOT,
            &fragment,
            (1000, 1000),
            Crossings::Drop,
            Definitions::Share,
        )
        .unwrap();
    assert_eq!(pasted.nodes.len(), 3);

    let placed: Vec<(i32, i32)> = pasted
        .nodes
        .iter()
        .map(|&id| {
            let node = document.tree(ROOT).unwrap().node(id).unwrap();
            (node.x, node.y)
        })
        .collect();

    // Where it landed: the centroid is the point that was asked for.
    let centroid = (
        placed.iter().map(|p| p.0).sum::<i32>() / 3,
        placed.iter().map(|p| p.1).sum::<i32>() / 3,
    );
    assert_eq!(
        centroid,
        (1000, 1000),
        "the fragment is placed, not each node"
    );

    // And its shape: every pairwise offset is the one it had before.
    let before = [(100, 50), (300, 50), (500, 90)];
    let mut sorted = placed.clone();
    sorted.sort_unstable();
    for index in 1..3 {
        assert_eq!(
            (
                sorted[index].0 - sorted[index - 1].0,
                sorted[index].1 - sorted[index - 1].1
            ),
            (
                before[index].0 - before[index - 1].0,
                before[index].1 - before[index - 1].1
            ),
            "a paste at a point must not distort what was copied"
        );
    }
    assert_eq!(fragment.origin(), (300, 63), "and the anchor is readable");
}

/// R1631 — the ARRANGEMENT cluster: the engine's eleven align / distribute /
/// stack / straighten commands. Its own registry because the hook registry was
/// already at the line ceiling, and because these eleven are one capability.
fn engine_arrangement_proofs() -> Vec<Proof> {
    vec![
        // R1631 — the alignment cluster. Six aligns, two distributes, two
        // stacks and a straighten, all from the same three parameters.
        proof(
            "engine",
            "GraphEditor::AlignNodesLeft",
            engine_graph_editor_align_nodes_left,
        ),
        proof(
            "engine",
            "GraphEditor::AlignNodesRight",
            engine_graph_editor_align_nodes_right,
        ),
        proof(
            "engine",
            "GraphEditor::AlignNodesTop",
            engine_graph_editor_align_nodes_top,
        ),
        proof(
            "engine",
            "GraphEditor::AlignNodesBottom",
            engine_graph_editor_align_nodes_bottom,
        ),
        proof(
            "engine",
            "GraphEditor::AlignNodesCenter",
            engine_graph_editor_align_nodes_center,
        ),
        proof(
            "engine",
            "GraphEditor::AlignNodesMiddle",
            engine_graph_editor_align_nodes_middle,
        ),
        proof(
            "engine",
            "GraphEditor::DistributeNodesHorizontally",
            engine_graph_editor_distribute_nodes_horizontally,
        ),
        proof(
            "engine",
            "GraphEditor::DistributeNodesVertically",
            engine_graph_editor_distribute_nodes_vertically,
        ),
        proof(
            "engine",
            "GraphEditor::StackNodesHorizontally",
            engine_graph_editor_stack_nodes_horizontally,
        ),
        proof(
            "engine",
            "GraphEditor::StackNodesVertically",
            engine_graph_editor_stack_nodes_vertically,
        ),
        proof(
            "engine",
            "GraphEditor::StraightenConnections",
            engine_graph_editor_straighten_connections,
        ),
    ]
}

/// The HOOK surface (R1603): the virtuals of `the graph node` and
/// `the graph schema`.
fn engine_hook_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "node::AllocateDefaultPins",
            engine_node_allocate_default_pins,
        ),
        proof("engine", "node::DestroyNode", engine_node_destroy_node),
        proof(
            "engine",
            "node::GetPassThroughPin",
            engine_node_get_pass_through_pin,
        ),
        proof(
            "engine",
            "node::GetPinDisplayName",
            engine_node_get_pin_display_name,
        ),
        proof("engine", "node::GetSubGraphs", engine_node_get_sub_graphs),
        proof(
            "engine",
            "node::NodeConnectionListChanged",
            engine_node_node_connection_list_changed,
        ),
        proof("engine", "node::OnPinRemoved", engine_node_on_pin_removed),
        proof("engine", "node::OnRenameNode", engine_node_on_rename_node),
        proof(
            "engine",
            "node::OnUpdateCommentText",
            engine_node_on_update_comment_text,
        ),
        proof(
            "engine",
            "node::PinConnectionListChanged",
            engine_node_pin_connection_list_changed,
        ),
        proof(
            "engine",
            "node::PinDefaultValueChanged",
            engine_node_pin_default_value_changed,
        ),
        proof("engine", "node::PostPasteNode", engine_node_post_paste_node),
        proof(
            "engine",
            "node::PostPlacedNewNode",
            engine_node_post_placed_new_node,
        ),
        proof(
            "engine",
            "node::PrepareForCopying",
            engine_node_prepare_for_copying,
        ),
        proof("engine", "node::ResizeNode", engine_node_resize_node),
        // ★★★★★ R1927 — the WARNING pair. One proof, because they are one
        // answer here: the reference's two virtuals can come apart and in its
        // own source they do, so the tooltip row CITES this rather than owning
        // a second proof there is no second mechanism for.
        proof(
            "engine",
            "node::ShowVisualWarning",
            engine_node_show_visual_warning,
        ),
    ]
}

/// And the schema's half of it — what the engine asks a GRAPH to decide.
fn engine_schema_hook_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "schema::ArePinTypesEquivalent",
            engine_schema_are_pin_types_equivalent,
        ),
        proof(
            "engine",
            "schema::ArePinsCompatible",
            engine_schema_are_pins_compatible,
        ),
        proof(
            "engine",
            "schema::CanCreateConnection",
            engine_schema_can_create_connection,
        ),
        proof(
            "engine",
            "schema::CanEncapuslateNode",
            engine_schema_can_encapuslate_node,
        ),
        proof(
            "engine",
            "schema::CreateAutomaticConversionNodeAndConnections",
            engine_schema_create_automatic_conversion_node_and_connections,
        ),
        proof(
            "engine",
            "schema::DoesDefaultValueMatch",
            engine_schema_does_default_value_match,
        ),
        proof(
            "engine",
            "schema::GetGraphDisplayInformation",
            engine_schema_get_graph_display_information,
        ),
        proof(
            "engine",
            "schema::IsPinDefaultValid",
            engine_schema_is_pin_default_valid,
        ),
        proof(
            "engine",
            "schema::SetNodePosition",
            engine_schema_set_node_position,
        ),
        proof(
            "engine",
            "schema::TrySetDefaultValue",
            engine_schema_try_set_default_value,
        ),
    ]
}
/// The proof name a reference row must carry, so the two are one decision.
///
/// Two shapes, because a reference has two kinds of name. An **operator** is a
/// bare identifier — snake case under a fixed prefix in the DCC, Pascal case
/// in the engine. A **hook** is `Owner::member`, and its owner is stripped
/// down to what distinguishes it: a leading `b` or `U`, a leading graph and a
/// trailing `Type` are all the reference's own naming furniture, so
/// `the tree type` and `the graph node` become `node_tree` and `node`.
fn proof_name(tree: &str, operator: &str) -> String {
    if let Some((owner, member)) = operator.split_once("::") {
        // R1612 — the owner arrives already reduced to this stem, so the four
        // trims that used to strip a vendor's class prefix here are gone rather
        // than merely unreachable. They were also the last spelling of that
        // prefix left in a published file, and short enough that the name
        // census did not recognise them.
        return format!("{tree}_{}_{}", snake(owner), snake(member));
    }
    format!("{tree}_{}", snake(operator))
}

/// Pascal or snake in, snake out. A name already in snake case is unchanged.
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (index, character) in name.char_indices() {
        if character.is_ascii_uppercase() && index > 0 {
            out.push('_');
        }
        out.push(character.to_ascii_lowercase());
    }
    out
}

/// The pin and the proofs agree, in both directions.
///
/// This is the check that makes a verdict cost something. A `have` added to the
/// pin with no proof fails here; a proof deleted or renamed fails here; and a
/// proof whose name is not derived from the row that owns it fails here, because
/// the name comes from the **compiler** rather than from a string typed twice.
///
/// ★ It is not a bijection any more, and the reason is a finding rather than a
/// concession. One pinion mechanism often answers **several** reference rows —
/// the DCC's `the tree type::localize` and the engine's `the graph schema::DuplicateGraph` are one `fork_definition`; `delete` and `SafeDeleteNodeFromGraph` are one `remove_node` —
/// and saying so is exactly the "the reference writes it three times and this
/// derives it once" measurement R1589 recorded by hand. So a proof has one
/// **owner** (the row its name derives from) and may be **cited** by any
/// number of others, and the fan-out is reported.
#[test]
fn the_pin_and_the_proofs_agree() {
    let pin: BTreeMap<String, BTreeMap<String, Row>> =
        serde_json::from_str(PIN).expect("the census pin parses");

    let mut claimed: BTreeMap<(String, String), String> = BTreeMap::new();
    for (tree, rows) in &pin {
        for (operator, row) in rows {
            if row.verdict != "have" {
                assert!(
                    row.proven_by.is_empty(),
                    "{tree}/{operator} is {:?} and still names a proof — a verdict \
                     that is out of the numerator must not carry evidence for one",
                    row.verdict
                );
                continue;
            }
            let (crate_name, proof) = row
                .proven_by
                .split_once("::")
                .unwrap_or_else(|| panic!("{tree}/{operator}: proven_by is not <crate>::<test>"));
            if crate_name == CRATE {
                claimed.insert((tree.clone(), operator.clone()), proof.to_owned());
            }
        }
    }

    let table = proofs();
    let names: BTreeSet<&str> = table.iter().map(|entry| entry.name).collect();
    assert_eq!(names.len(), table.len(), "two proofs share a name");

    // Every row this crate is addressed by names a proof that is here.
    for ((tree, operator), proof) in &claimed {
        assert!(
            names.contains(proof.as_str()),
            "{tree}/{operator} names {proof}, which is not in this file"
        );
    }
    // And every proof here is owned by exactly one row, which is the row its
    // name derives from. A proof nothing owns is a test the pin never asks for.
    for entry in &table {
        let owner = (entry.tree.to_owned(), entry.operator.to_owned());
        assert_eq!(
            entry.name,
            proof_name(entry.tree, entry.operator),
            "{}/{} owns a proof whose name is not derived from it",
            entry.tree,
            entry.operator
        );
        assert_eq!(
            claimed.get(&owner).map(String::as_str),
            Some(entry.name),
            "{}/{} does not name the proof that says it owns it",
            entry.tree,
            entry.operator
        );
    }

    let cited = claimed.len() - table.len();
    assert!(
        cited > 0,
        "no row cites another row's proof, which would mean this crate answers \
         every reference row with its own mechanism — worth noticing if it ever \
         becomes true"
    );
}

/// Every proof in the table runs.
///
/// The individual `#[test]` attributes already run them one by one, which is
/// what names a failure. This asserts the *table* is live: an entry whose
/// function was quietly replaced by a stub still has to do the work.
#[test]
fn every_proof_in_the_table_runs() {
    for entry in proofs() {
        (entry.run)();
    }
}

// ==================================================== groups and definitions

/// The DCC adds an empty group and drops you inside it. The two halves here
/// are a definition with nothing in it and an instance standing for it — and
/// the instance's signature is **derived**, so an empty definition gives an
/// instance with no ports at all rather than a node with unresolved sockets.
#[test]
fn dcc_add_empty_group() {
    let mut document: Document<Op> = Document::new("root");
    let definition = document.add_definition("Empty");
    let instance = document.instantiate(ROOT, definition, 0, 0).unwrap();

    let signature = document.signature(ROOT, instance).unwrap();
    assert!(signature.inputs.is_empty() && signature.outputs.is_empty());
    assert_eq!(document.tree(definition).unwrap().node_count(), 0);
    assert!(document.evaluate(ROOT, instance).is_empty());
    assert!(document.validate().is_empty());
}

/// A second instance of a definition that already exists. What makes this more
/// than a copy is that the two instances share the definition and **not** the
/// value: fed differently they answer differently, which is the memo being
/// keyed by instance.
#[test]
fn dcc_add_group() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    assert_eq!(chain.document.instance_count(made.definition), 1);

    let again = chain
        .document
        .instantiate(ROOT, made.definition, 0, 300)
        .unwrap();
    assert_eq!(chain.document.instance_count(made.definition), 2);

    let ten = num(&mut chain.document, 10);
    wire(&mut chain.document, ten, 0, again, 0);
    wire(&mut chain.document, ten, 0, again, 1);
    assert_eq!(
        chain.document.evaluate(ROOT, again),
        vec![Some(Val::Number(20))]
    );
    assert_eq!(
        chain.document.evaluate(ROOT, made.node),
        vec![Some(Val::Number(5))],
        "the first instance is undisturbed by the second"
    );
}

/// The DCC's Group Input node. The interface is the definition's, and the node
/// is how the graph *inside* reaches it — so an `Input` interface node's ports
/// are OUTPUTS, which is the part that is easy to get backwards.
#[test]
fn dcc_add_group_input_node() {
    let mut document: Document<Op> = Document::new("root");
    let definition = document.add_definition("Def");
    let index = document
        .expose(
            definition,
            InterfaceSide::Input,
            Port::new("Seed", Ty::Number),
        )
        .unwrap();
    assert_eq!(index, 0);

    let inside = document
        .add_node(definition, NodeBody::Interface(InterfaceSide::Input), 0, 0)
        .unwrap();
    assert_eq!(
        document
            .tree(definition)
            .unwrap()
            .interface_node(InterfaceSide::Input)
            .map(|n| n.id),
        Some(inside)
    );
    let signature = document.signature(definition, inside).unwrap();
    assert!(signature.inputs.is_empty());
    assert_eq!(signature.outputs.len(), 1);
    assert_eq!(signature.outputs[0].name, "Seed");
}

/// Collapse a selection into a re-usable definition. The interface is not
/// authored: it is derived from what crossed the boundary, and the graph goes
/// on computing what it computed.
#[test]
fn dcc_group_make() {
    let mut chain = chain();
    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    assert_eq!(before, Some(Val::Number(5)));
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();

    let definition = chain.document.tree(made.definition).unwrap();
    assert_eq!(definition.interface().inputs().len(), 2);
    assert_eq!(definition.interface().outputs().len(), 1);
    assert!(chain.document.tree(ROOT).unwrap().node(chain.add).is_none());
    assert_eq!(arrives(&chain.document, Socket::new(chain.sink, 0)), before);
    assert!(chain.document.validate().is_empty());
}

/// Inline a group back. The nodes come out, the definition is reported as
/// having no instances left, and the value is unchanged.
#[test]
fn dcc_group_ungroup() {
    let mut chain = chain();
    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();

    let out = chain.document.ungroup(ROOT, made.node).unwrap();
    assert_eq!(out.nodes.len(), 1);
    assert!(out.definition_unused);
    assert_eq!(arrives(&chain.document, Socket::new(chain.sink, 0)), before);
}

/// Move a node from the host INTO the group through its instance. The DCC
/// leaves the interface alone; here it is re-derived, so the value that used
/// to cross keeps crossing and nothing is left describing a link that is gone.
#[test]
fn dcc_group_insert() {
    let mut chain = chain();
    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();

    let moved = chain
        .document
        .group_insert(ROOT, made.node, &[chain.two], Sharing::Shared)
        .unwrap();
    assert_eq!(
        moved.moved.len(),
        1,
        "the node is named by the id it has in the definition, which is another tree"
    );
    assert!(chain.document.tree(ROOT).unwrap().node(chain.two).is_none());
    assert_eq!(
        chain
            .document
            .tree(made.definition)
            .unwrap()
            .interface()
            .inputs()
            .len(),
        1,
        "the port `two` used to feed is gone; the one `three` feeds remains"
    );
    assert_eq!(arrives(&chain.document, Socket::new(chain.sink, 0)), before);
}

/// The other direction: move a node out of the definition into the host. The
/// value that used to cross the boundary is **reconnected**, which is where
/// the DCC loses it.
#[test]
fn dcc_group_separate() {
    let mut chain = chain();
    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    let made = chain
        .document
        .group(ROOT, &[chain.two, chain.add], "Sum")
        .unwrap();

    let inside: Vec<NodeId> = chain
        .document
        .tree(made.definition)
        .unwrap()
        .nodes()
        .filter(|n| matches!(&n.body, NodeBody::Kind(Op::Num(_))))
        .map(|n| n.id)
        .collect();
    assert_eq!(inside.len(), 1);

    let moved = chain
        .document
        .group_separate(ROOT, made.node, &inside, Sharing::Shared)
        .unwrap();
    assert_eq!(moved.moved.len(), 1);
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)),
        before,
        "the value that used to cross the boundary was reconnected"
    );
}

/// A definition is a tree of its own, addable without any node being collapsed
/// into it. The DCC's "New Node Tree".
#[test]
fn dcc_new_node_tree() {
    let mut document: Document<Op> = Document::new("root");
    let before = document.tree_count();
    let definition = document.add_definition("Fresh");

    assert_eq!(document.tree_count(), before + 1);
    assert!(document.definitions().any(|t| t.id == definition));
    assert_eq!(document.instance_count(definition), 0);
    assert!(document.tree(definition).unwrap().interface().is_empty());
}

/// Descend into a definition and come back. The path is a value, so "where am
/// I" is answerable without the editor keeping its own copy.
#[test]
fn dcc_group_edit() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();

    let mut path = EditPath::root();
    assert_eq!(path.current(), ROOT);
    let entered = path.enter(&chain.document, made.node).unwrap();
    assert_eq!(entered, made.definition);
    assert_eq!(path.current(), made.definition);
    assert_eq!(path.depth(), 1);
    assert_eq!(path.breadcrumb(&chain.document).len(), 2);
}

/// ★★★★★ R1923 — the engine's node tooltip and the DCC's node-type description
/// hook: **what a node says about itself, and which of its two sources said it.**
#[test]
fn engine_node_get_tooltip_text() {
    let mut chain = chain();

    // ⚠ The fixture's kinds say nothing about themselves, which is the honest
    // default: a kind is not obliged to describe itself, and this asserts that
    // the absence is an ANSWER rather than a hole to be filled with something
    // invented.
    assert_eq!(
        chain.document.description(ROOT, chain.add),
        None,
        "a kind with nothing to say, and a node nobody wrote on"
    );

    // ★ A person's note is carried, and says it was a person's.
    if let Some(slot) = chain
        .document
        .tree_mut(ROOT)
        .and_then(|host| host.node_mut(chain.add))
    {
        slot.description = Some("the one that adds the two readings".to_owned());
    }
    let said = chain
        .document
        .description(ROOT, chain.add)
        .expect("a note was written");
    assert_eq!(said.sentence, "the one that adds the two readings");
    // ★★★★★ THE HALF THE REFERENCE CANNOT EXPRESS. Its node tooltip hook hands
    // back a bare string and its own default returns the class's, so a caller
    // there is given one value and cannot tell *a person wrote this about this
    // node* from *this is what nodes of this sort are*. Those are different
    // facts: the first is editable and belongs to this node, the second is not
    // and belongs to every node of the kind.
    assert_eq!(said.source, Described::Authored);
    assert_eq!(said.source.wire_word(), "authored");

    // ★ Clearing the note is not the same as the kind having nothing to say —
    // and with this fixture's kinds silent, the answer returns to None rather
    // than to some invented sentence.
    if let Some(slot) = chain
        .document
        .tree_mut(ROOT)
        .and_then(|host| host.node_mut(chain.add))
    {
        slot.description = None;
    }
    assert_eq!(chain.document.description(ROOT, chain.add), None);

    // ★ A structural body says nothing of its own, deliberately: a frame and a
    // group instance are this CRATE's, so a sentence about them would be the
    // crate describing itself to an application's reader.
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    assert_eq!(
        chain.document.description(ROOT, made.node),
        None,
        "the crate does not put words in an application's mouth"
    );
    // ★ …but an application may still write one on it, which is what makes the
    // silence a default rather than a refusal.
    if let Some(slot) = chain
        .document
        .tree_mut(ROOT)
        .and_then(|host| host.node_mut(made.node))
    {
        slot.description = Some("the collapsed pair".to_owned());
    }
    assert_eq!(
        chain
            .document
            .description(ROOT, made.node)
            .map(|d| d.source),
        Some(Described::Authored)
    );

    // ★★★★★ THE NOTE TRAVELS WITH THE NODE, driven over the REAL public path —
    // extract a fragment and insert it — rather than by reaching into the
    // crate. `Node::adopt_from` destructures every field, so a new one has to
    // be answered for, and the answer here is `label`'s: a copy that arrived
    // without the note would have silently dropped what somebody said.
    let lifted = chain
        .document
        .extract(ROOT, &[made.node])
        .expect("the instance lifts out");
    let landed = chain
        .document
        .insert(
            ROOT,
            &lifted,
            (400, 400),
            Crossings::Drop,
            Definitions::Share,
        )
        .expect("and goes back in");
    let copy = landed.nodes.first().copied().expect("one node came back");
    assert_eq!(
        chain.document.description(ROOT, copy).map(|d| d.sentence),
        Some("the collapsed pair".to_owned()),
        "a pasted copy carries the note"
    );
    assert_eq!(
        chain.document.description(ROOT, copy).map(|d| d.source),
        Some(Described::Authored),
        "and still says a person wrote it"
    );
}

/// ★★★★★ R1922 — the DCC's `poll`/`poll_instance` and the engine's two
/// compatibility hooks: **would this tree accept this body?**
#[test]
fn dcc_node_poll() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();

    // An ordinary body goes anywhere a tree exists — the surface is not a
    // blanket refusal.
    assert!(
        chain
            .document
            .admits(made.definition, &NodeBody::Frame)
            .is_ok()
    );

    // ★★★★★ REFUSAL ONE, and the one that had NO diagnosis at all before this
    // round: ROOT is the tree nothing instantiates, so an interface end there
    // materialises a contract with no outside. Measured at R1922's open the
    // placement SUCCEEDED and `validate` said nothing whatsoever.
    assert_eq!(
        chain
            .document
            .admits(ROOT, &NodeBody::Interface(InterfaceSide::Input)),
        Err(EditError::RootHasNoOutside {
            tree: ROOT,
            side: InterfaceSide::Input,
        })
    );

    // ★ REFUSAL TWO: that side already has its inside end. `Tree::interface_node`
    // documents itself as answering *the sole node* materialising a side, and
    // it answers with the first — so a second was drawn and could not be found
    // by the accessor that is about it.
    let held = chain
        .document
        .tree(made.definition)
        .and_then(|host| host.interface_node(InterfaceSide::Input))
        .map(|node| node.id)
        .expect("the collapse built one");
    assert_eq!(
        chain
            .document
            .admits(made.definition, &NodeBody::Interface(InterfaceSide::Input)),
        Err(EditError::InterfaceEndTaken {
            tree: made.definition,
            side: InterfaceSide::Input,
            held_by: held,
        })
    );

    // ★★★★★ REFUSAL THREE, and the one PAST the reference: a tree cannot hold
    // an instance of itself, and the refusal NAMES THE CHAIN that would close.
    // The reference prints the same flat sentence for a direct self-nest and
    // for one four groups deep, so the definitions actually carrying the
    // recursion are never named there.
    let refused = chain
        .document
        .admits(made.definition, &NodeBody::Group(made.definition))
        .expect_err("a tree cannot hold itself");
    assert!(
        matches!(&refused, EditError::WouldContainItself { chain, .. } if !chain.is_empty()),
        "the chain is named: {refused:?}"
    );
    assert!(
        format!("{refused}").contains("chain"),
        "and the sentence says so: {refused}"
    );

    // ★★★★★ THE LAW: `may` asks this BEFORE an edit and `validate` asks it OF a
    // document, and they are the same predicate — so a placement the edit
    // refuses is a placement a document is reported for. Driven by building
    // the state the way a FILE does, which is the path `admits` cannot stand
    // in front of and the reason `validate` is not made redundant by it.
    assert_eq!(
        chain.document.may(
            ROOT,
            Act::Create(&NodeBody::Interface(InterfaceSide::Input))
        ),
        chain
            .document
            .admits(ROOT, &NodeBody::Interface(InterfaceSide::Input)),
        "may() is admits(), not a second opinion about it"
    );

    // ★ And the refusals are REACHABLE-BY-CONSTRUCTION rather than vacuous:
    // each of the three was accepted by this crate before this round, measured
    // at its open. Two of them `validate` already reported after the fact; the
    // ROOT one it did not report at all.
    assert!(
        chain.document.validate().is_empty(),
        "the document itself is well-formed, so none of the above is an artefact \
         of an already-broken fixture: {:?}",
        chain.document.validate()
    );
}

// ==================================================== R1927 — node warnings

/// ★★★★★ R1927 — a node says whether it is in a questionable state, and why.
///
/// The reference's pair, and the three ways this passes it are each asserted
/// rather than claimed:
///
/// * **the warning carries its own sentence**, so the silent badge its own
///   overrider produces cannot be built here;
/// * **the situation is the argument**, so the rule changes answer when the
///   wiring changes and a test can drive it without a world around it;
/// * **the graph can be asked**, which the reference has no call for at all.
#[test]
fn engine_node_show_visual_warning() {
    let mut chain = chain();
    let lonely = chain
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 0, 0)
        .unwrap();

    // ★ Unwired, the rule fires — and it fires WITH a sentence. There is no
    // arrangement of this API in which it warns and says nothing.
    let said = chain
        .document
        .warning(ROOT, lonely)
        .expect("an unfed sink warns");
    assert_eq!(said.node, lonely);
    assert!(
        said.sentence.contains("nothing reaches this sink"),
        "the sentence is the application's own: {said:?}"
    );

    // ★★★★★ The situation is what it answers from: wire the sink and the same
    // node, the same kind, stops warning. Without this the rule could be a
    // constant and every other assertion here would still hold.
    chain
        .document
        .connect(ROOT, Socket::new(chain.add, 0), Socket::new(lonely, 0))
        .expect("a number reaches the sink");
    assert_eq!(chain.document.warning(ROOT, lonely), None);

    // And the argument is askable on its own, which is what makes a rule
    // testable without driving a document through it.
    let around = chain.document.surroundings(ROOT, lonely);
    assert!(around.is_wired(Side::Input, 0));
    assert!(around.any_wired(Side::Input));
    assert!(!around.any_wired(Side::Output));
    assert_eq!(around.wired(Side::Input).collect::<Vec<_>>(), vec![0]);

    // ★ A kind with no rule says nothing, which is the ordinary case: a
    // taxonomy where every kind warned could not tell a rule from a default.
    assert_eq!(chain.document.warning(ROOT, chain.add), None);

    // ★★★★★ The whole graph, in node order — the call the reference does not
    // have, because there the badge is decided inside the widget.
    let second = chain
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 40, 0)
        .unwrap();
    let listed = chain.document.warnings(ROOT);
    assert_eq!(
        listed.iter().map(|held| held.node).collect::<Vec<_>>(),
        vec![second],
        "only the unfed one is in the list, and it is addressable: {listed:?}"
    );
    assert!(listed.iter().all(|held| !held.sentence.is_empty()));

    // ⚠ A structural body has no application rule to ask, and answering `None`
    // for it is a different thing from a kind that declines to warn — this is
    // the arm that would panic if the walk asked a frame for a kind.
    let frame = chain
        .document
        .add_node(ROOT, NodeBody::Frame, 200, 200)
        .unwrap();
    assert_eq!(chain.document.warning(ROOT, frame), None);
    assert!(
        chain
            .document
            .warnings(ROOT)
            .iter()
            .all(|held| held.node != frame),
        "and it is not in the list"
    );

    // ⚠ And a warning is NOT a structural finding: this document is warning and
    // perfectly well formed, which is the line between the two.
    assert!(!chain.document.warnings(ROOT).is_empty());
    assert!(chain.document.validate().is_empty());
}

// ====================================================== R1926 — port colours

/// ★★★★★ R1926 — a socket **type** has a colour, asked where no port exists.
///
/// The reference's signature is what makes this its own row: it takes a pin
/// TYPE and no pin, so a legend or a type picker can ask it. R1921 gave a NODE
/// an authored colour and did not reach a type at all.
///
/// ★ The assertion the reference could not make: `Text` answers **nothing**.
/// Its base returns `FLinearColor::Black`, and its own K2 implementation writes
/// `// Type does not have a defined color!` before returning a settings
/// default — so there, *never coloured* and *coloured black* are one answer.
#[test]
fn engine_schema_get_pin_type_color() {
    assert_eq!(
        type_palette::<Op>(&Ty::Number).own(),
        Some(Tint::rgb(0x2D, 0x6C, 0xDF)),
        "a coloured type answers its colour"
    );
    assert_eq!(
        type_palette::<Op>(&Ty::Flag).own(),
        Some(Tint::rgb(0x1F, 0x8A, 0x4C))
    );
    assert_ne!(
        type_palette::<Op>(&Ty::Number).own(),
        type_palette::<Op>(&Ty::Flag).own(),
        "two types are two colours — without this the whole answer could be one \
         constant and every assertion above would still hold"
    );
    // ★★★★★ Absence, as a value.
    assert_eq!(type_palette::<Op>(&Ty::Text).own(), None);
    assert!(
        type_palette::<Op>(&Ty::Text).is_silent(),
        "an uncoloured atom is silent, which is the answer the reference \
         cannot give"
    );

    // ★ Control is not a type here (R1599), so it is a second declaration and
    // it is reachable — the reference reaches its execution pin through the
    // same hook because there an exec pin IS a pin type.
    let control: pinion_node_graph::Flow<Ty, Val> = Port::control("Then").flow;
    assert_eq!(
        palette_of::<Op>(&control).own(),
        Some(Tint::rgb(0xEC, 0x5A, 0xA0))
    );
    assert!(
        palette_of::<Op>(&control).members().is_empty(),
        "control carries nothing, so it is made of nothing"
    );
}

/// ★★★★★ R1926 — the SECOND colour a composite type is drawn in, and the two
/// ways this passes what was measured.
///
/// Read this round at the only implementation of substance, the reference
/// answers a real second colour **only when the type is a MAP**, and what it
/// answers is the map's VALUE half; an array or a set gets a settings constant.
/// So the census's own reason for this row — *a container whose element type
/// has a colour of its own* — was wrong twice: not containers, and not
/// elements.
///
/// Here it is one entry per member of a **composite**, derived from
/// `NodeKind::composition`, which the application already declares. The
/// reference's map is the two-member case; a three-member composite it cannot
/// speak about at all.
#[test]
fn engine_schema_get_secondary_pin_type_color() {
    let composite = type_palette::<Op>(&Ty::Pair);
    assert_eq!(
        composite.members().len(),
        2,
        "one entry per member of the composition the taxonomy already declares"
    );
    assert!(
        composite
            .members()
            .iter()
            .all(|held| *held == Some(Tint::rgb(0x2D, 0x6C, 0xDF))),
        "and each is the MEMBER's own type colour, not the composite's: {:?}",
        composite.members()
    );
    assert_ne!(
        composite.own(),
        composite.members()[0],
        "★ the composite's own colour is not its members' — a palette that \
         answered the same colour twice would satisfy the arity check above"
    );

    // ★★★★★ A CONTAINER is coloured and still has no member colours, so the
    // empty list is a fact about the COMPOSITION rather than about the colour.
    // Both halves are needed: the reference conflates them by answering a
    // constant for everything that is not a map.
    let container = type_palette::<Op>(&Ty::Bag);
    assert_eq!(container.own(), Some(Tint::rgb(0xC7, 0x78, 0x00)));
    assert!(container.members().is_empty());
    assert!(!container.is_silent(), "it has a colour of its own");

    // And an atom.
    assert!(type_palette::<Op>(&Ty::Number).members().is_empty());
}

/// ★★★★★ R1926 — a **port's** colour, over the resolved signature.
///
/// ★ Why this is a derivation and not a second authored value, measured across
/// the whole engine source this round: **twelve** schemas override the type
/// colour and **one** overrides the pin colour — and that one reads the pin's
/// type more precisely and then answers a TYPE colour, falling back to the type
/// hook otherwise. Nothing in the reference gives one pin a colour of its own,
/// so there is nothing here for a port's colour to disagree with its type's
/// about.
///
/// ★★ And the half a screen depends on: a port a SPLIT put there answers for
/// **its own** type. Without that, two halves of one address are drawn in one
/// colour, which is exactly the state the node lab was in before this round.
#[test]
fn engine_schema_get_pin_color() {
    let mut chain = chain();
    let carry = chain
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Carry), 0, 0)
        .unwrap();

    // `Whole` carries the composite type, so its port answers the composite's
    // palette — colour and members together.
    let whole = chain
        .document
        .port_palette(ROOT, carry, PortRef::input(1))
        .expect("`Whole` is a port");
    assert_eq!(whole.own(), type_palette::<Op>(&Ty::Pair).own());
    assert_eq!(whole.members().len(), 2);

    // ★ Control reaches the port-level answer too, which on this fixture is
    // input 0.
    assert_eq!(
        chain
            .document
            .port_palette(ROOT, carry, PortRef::input(0))
            .and_then(|held| held.own()),
        Op::control_colour(),
        "a control port answers the control declaration"
    );

    // ★ A port that is not there is `None`, where the reference answers Black
    // for a null pin — an answer a caller cannot tell from a black pin.
    assert_eq!(
        chain.document.port_palette(ROOT, carry, PortRef::input(99)),
        None
    );

    // ★★★★★ After a split, each member port answers for ITS OWN type.
    chain
        .document
        .split_port(ROOT, carry, Side::Input, &PortPath::root(1))
        .expect("`Whole` splits");
    let resolved = chain.document.resolved_ports(ROOT, carry, Side::Input);
    let members: Vec<(usize, Port<Ty, Val>)> = resolved
        .iter()
        .enumerate()
        .filter(|(_, (path, _))| path.depth() > 0)
        .map(|(index, (_, port))| (index, port.clone()))
        .collect();
    assert_eq!(members.len(), 2, "two halves are on the frame");
    for (index, port) in &members {
        let at = PortRef::input(u32::try_from(*index).unwrap_or(0));
        let answered = chain
            .document
            .port_palette(ROOT, carry, at)
            .expect("a member port is a port of the resolved signature");
        assert_eq!(
            answered,
            palette_of::<Op>(&port.flow),
            "the indexed answer and the port-in-hand answer are one derivation"
        );
        assert_eq!(
            answered.own(),
            type_palette::<Op>(&Ty::Number).own(),
            "and it is the MEMBER's type, not the composite it came out of"
        );
        assert_ne!(
            answered.own(),
            whole.own(),
            "★ so a half is NOT drawn in the colour the whole was — the defect \
             this round found on the node lab, stated as an assertion"
        );
    }
}

/// ★★★★★ R1921 — the DCC's `node_copy_color` and the engine's four node-face
/// colours: **one authored colour, four derived faces.**
#[test]
fn dcc_node_copy_color() {
    let mut chain = chain();

    // A node with no authored colour is drawn in whatever its kind gives, and
    // says so by carrying nothing rather than by carrying a colour it is not
    // using — which is the state the DCC's colour-plus-flag pair CAN hold.
    let bare = chain
        .document
        .tree(ROOT)
        .and_then(|host| host.node(chain.add))
        .map(|node| node.appearance.tint);
    assert_eq!(bare, Some(None), "an unauthored node carries no colour");

    // ★ COPYING is one assignment. The DCC has to move two facts and remember
    // to clear the bit when the source has none; here the source's whole answer
    // IS the value, so "copy the colour" and "copy the absence" are one line.
    let chosen = Tint::rgb(220, 40, 60);
    if let Some(slot) = chain
        .document
        .tree_mut(ROOT)
        .and_then(|host| host.node_mut(chain.add))
    {
        slot.appearance.tint = Some(chosen);
    }
    let source = chain
        .document
        .tree(ROOT)
        .and_then(|host| host.node(chain.add))
        .map(|node| node.appearance.tint)
        .expect("the node is there");
    if let Some(slot) = chain
        .document
        .tree_mut(ROOT)
        .and_then(|host| host.node_mut(chain.sink))
    {
        slot.appearance.tint = source;
    }
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .and_then(|host| host.node(chain.sink))
            .and_then(|node| node.appearance.tint),
        Some(chosen),
        "the colour copied"
    );

    // ★★★★★ THE FOUR FACES ARE DERIVED, so a node cannot answer with four
    // colours that do not go together. The engine asks four independent
    // virtuals; this asks one value.
    let faces = Faces::of(chosen);
    assert_eq!(faces.title, chosen, "the title band IS the authored colour");
    assert!(
        faces.body.luminance() < faces.title.luminance(),
        "the body reads as the same node, darker: {faces:?}"
    );
    assert!(
        faces.comment.luminance() < faces.body.luminance(),
        "and a frame sits further back still: {faces:?}"
    );

    // ★★★★★ THE LAW, OVER EVERY COLOUR THERE IS — not the ones somebody tried.
    // The engine's title colour and title TEXT colour are two unrelated
    // virtuals, so a subclass there can darken one and not the other and
    // produce a title nobody can read, with nothing able to notice. Here the
    // text is CHOSEN by contrast, which makes unreadable-title unreachable —
    // and that is a property, so it is held over the whole cube.
    let mut worst = 255i32;
    let mut worst_at = Tint::default();
    let mut checked = 0u32;
    for r in (0..=255).step_by(15) {
        for g in (0..=255).step_by(15) {
            for b in (0..=255).step_by(15) {
                let tint = Tint::rgb(r, g, b);
                let faces = Faces::of(tint);
                let ink = faces.title_text;
                assert!(
                    ink == Tint::rgb(0, 0, 0) || ink == Tint::rgb(255, 255, 255),
                    "the letters are one of two inks: {ink:?}"
                );
                let gap = i32::from(faces.title.luminance()) - i32::from(ink.luminance());
                let gap = gap.abs();
                if gap < worst {
                    worst = gap;
                    worst_at = tint;
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 4000, "the cube was really walked — {checked}");
    // ★ Printed, not only asserted: the MARGIN is the interesting number. A
    // floor that sits just under the worst case would be a tolerance the size
    // of the defect, which is R1862's lesson and which this round met again —
    // a screen-side mutation landed at a gap of 101 against this floor of 100
    // and passed. Anyone tightening this should read what the derivation
    // actually leaves rather than guess.
    println!("contrast: worst gap {worst} at {worst_at:?}, over {checked} colours");
    // ⚠ The floor is asserted as a NUMBER measured from the derivation rather
    // than a hope: whatever colour is authored, the letters differ from the
    // band they sit on by at least this much luminance.
    assert!(
        worst >= 100,
        "every authored colour leaves a readable title — worst {worst} at {worst_at:?}"
    );
}

/// ★★★★★ R1920 — the engine's three permission rows: **may this edit be made?**,
/// asked before making it.
#[test]
fn engine_node_can_user_delete_node() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();

    // ★ The graph-level question (`schema::CanCreateNewNodes`): a tree that is
    // there takes nodes, one that is not says so. R1922 gave the act a BODY,
    // because what a tree will accept depends on what is being put in it.
    assert!(
        chain
            .document
            .may(ROOT, Act::Create(&NodeBody::Frame))
            .is_ok()
    );
    assert_eq!(
        chain
            .document
            .may(TreeId(77), Act::Create(&NodeBody::Frame)),
        Err(EditError::NoSuchTree(TreeId(77)))
    );

    // ★★★★★ THE REFUSAL THAT MAKES THIS SURFACE NON-VACUOUS. Measured at
    // R1920's open, BEFORE it existed: this delete SUCCEEDED, the definition
    // kept the interface ports this node was the inside end of, and
    // `validate()` answered with an empty list. A group's contract could lose
    // the half a caller wires to, in one ordinary delete, silently.
    let iface = chain
        .document
        .tree(made.definition)
        .and_then(|host| host.interface_node(InterfaceSide::Input))
        .map(|node| node.id)
        .expect("collapsing into a definition builds its interface ends");
    assert_eq!(
        chain.document.may(made.definition, Act::Delete(iface)),
        Err(EditError::InterfaceEnd {
            tree: made.definition,
            node: iface,
            side: InterfaceSide::Input,
        }),
        "a tree cannot be asked to give up the end of its own contract"
    );
    assert!(
        format!(
            "{}",
            chain
                .document
                .may(made.definition, Act::Delete(iface))
                .unwrap_err()
        )
        .contains("no inside end"),
        "and the refusal says what would be LOST, not merely that it refused"
    );

    // ★ The rename question (`node::GetCanRenameNode`) carries the VALUE, so it
    // answers the whole of what it is asked. Both references split exactly
    // here — a permission predicate beside a separate name validator — and a
    // caller there has to consult two things and combine them itself.
    assert!(
        chain
            .document
            .may(ROOT, Act::Rename(made.node, Some("Fresh")))
            .is_ok()
    );
    assert!(matches!(
        chain
            .document
            .may(ROOT, Act::Rename(made.node, Some("   "))),
        Err(EditError::LabelEmpty { .. })
    ));

    // ★★★★★ THE LAW, AND THE WHOLE REASON THIS ANSWERS `Result<(), EditError>`
    // RATHER THAN A TYPE OF ITS OWN: the question and the edit are ONE
    // decision, so they cannot disagree. Driven over EVERY node of EVERY tree
    // against BOTH gated node verbs — a population derived from the document
    // rather than a list written here, so a node kind added later is covered
    // without this test being edited.
    let trees: Vec<TreeId> = std::iter::once(ROOT)
        .chain(chain.document.definitions().map(|tree| tree.id))
        .collect();
    let mut refusals = 0;
    let mut allowed = 0;
    for tree in trees {
        let nodes: Vec<NodeId> = chain
            .document
            .tree(tree)
            .map(|host| host.nodes().map(|node| node.id).collect())
            .unwrap_or_default();
        for node in nodes {
            // A fresh document each time, so an edit that LANDS cannot change
            // what the next question is asked about.
            for act in [Act::Delete(node), Act::Rename(node, Some("Probe"))] {
                let mut fresh = chain.document.clone();
                let asked = fresh.may(tree, act);
                let done = match act {
                    Act::Delete(id) => fresh.remove_node(tree, id).map(|_| ()),
                    Act::Rename(id, label) => fresh.relabel(tree, id, label).map(|_| ()),
                    Act::Create(_) => unreachable!("not in this population"),
                };
                assert_eq!(
                    asked, done,
                    "asking and doing are one decision: {tree:?} {act:?}"
                );
                if asked.is_ok() {
                    allowed += 1;
                } else {
                    refusals += 1;
                }
            }
        }
    }
    // ⚠ Both counts, because either alone can be satisfied by a surface that
    // answers one way for everything — and a permission question that always
    // says yes is exactly the vacuous gate this round exists to not build.
    assert!(allowed > 0, "some edits are allowed");
    assert!(
        refusals > 0,
        "and some are REFUSED — {refusals} of {} asked",
        allowed + refusals
    );
}

/// ★★★★★ R1924 — **may this wire's end be picked up at all**, asked before the
/// drag starts.
///
/// The engine asks this of a pin and answers one bit; its base class answers
/// `false`, so relinking is a thing a schema opts into. Here the answer is the
/// **list** of sockets that end may be re-aimed at, and the bit is
/// `!list.is_empty()` — one derivation rather than two rules, and the half a
/// hand actually needs: an editor can light what will take the wire instead of
/// letting the hand find out port by port.
///
/// The proof has to reach the empty list, or the surface says nothing: a
/// question that can only answer yes is a question nobody has to ask.
#[test]
fn engine_schema_is_connection_relinking_allowed() {
    let mut chain = chain();
    let into_add = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .find(|link| link.from.node == chain.two)
        .map(|link| link.id)
        .expect("the chain wires the first constant into the adder");

    // `two -> add.0`. Its consuming end may move to `add.1`, and to the sink's
    // input, and nowhere else in this graph.
    let targets = chain
        .document
        .relink_targets(ROOT, into_add, Side::Input)
        .expect("the link is there");
    assert_eq!(
        targets,
        vec![Socket::new(chain.add, 1), Socket::new(chain.sink, 0)],
        "every socket that would take it, and only those"
    );
    assert!(
        !targets.contains(&Socket::new(chain.add, 0)),
        "★ where it already is is not somewhere ELSE it may go — a list that \
         always held the current socket could never be empty"
    );

    // ★★★★★ THE EMPTY ANSWER, which is what the engine's `false` means and what
    // makes this surface non-vacuous. Take the other consumers away and the
    // producing end of that link has nowhere in this graph to come from.
    chain.document.remove_node(ROOT, chain.sink).unwrap();
    chain.document.remove_node(ROOT, chain.three).unwrap();
    assert_eq!(
        chain
            .document
            .relink_targets(ROOT, into_add, Side::Output)
            .expect("the link is still there"),
        Vec::new(),
        "★ the gesture would not start: this end is stuck"
    );
    // And the other end of the SAME link is not stuck, so the emptiness above
    // is about that end rather than about the graph having become too small for
    // the question to mean anything.
    assert_eq!(
        chain
            .document
            .relink_targets(ROOT, into_add, Side::Input)
            .unwrap(),
        vec![Socket::new(chain.add, 1)]
    );

    // The two ways there is nothing to ask about are named rather than folded
    // into a false.
    assert_eq!(
        chain
            .document
            .relink_targets(TreeId(77), into_add, Side::Input)
            .unwrap_err(),
        RelinkError::NoSuchTree(TreeId(77))
    );
}

/// ★★★★★ R1924 — **would this end be taken on that port?**, answered with the
/// reason, without moving anything.
///
/// The engine's question hands back a response object whose payload is a
/// sentence; its base class answers "not implemented by this schema". Here the
/// answer is the refusal itself, so a hand hovering a port that will not take
/// the wire is told the two types, or the port and its arity, or the path that
/// would close.
#[test]
fn engine_schema_can_relink_connection_to_pin() {
    let mut chain = chain();
    let word = chain
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Word("hi".into())), 0, 240)
        .unwrap();
    let shout = chain
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Shout), 200, 240)
        .unwrap();
    let text = chain
        .document
        .connect(ROOT, Socket::new(word, 0), Socket::new(shout, 0))
        .unwrap()
        .link;
    let before = chain.document.clone();

    // A yes, and it is a real one: the adder's number output reads as text
    // through this taxonomy's directed conversion, so this end really could be
    // moved there.
    assert_eq!(
        chain
            .document
            .may_relink(ROOT, text, Side::Output, Socket::new(chain.add, 0)),
        Ok(())
    );

    // ★ And the no, on the relation's other direction — text does not read back
    // as a number. The refusal names BOTH sockets and BOTH types, where the
    // reference hands back one sentence.
    assert_eq!(
        chain
            .document
            .may_relink(ROOT, text, Side::Input, Socket::new(chain.add, 0)),
        Err(RelinkError::Refused(ConnectError::TypeMismatch {
            from: Socket::new(word, 0),
            from_type: Ty::Text,
            to: Socket::new(chain.add, 0),
            to_type: Ty::Number,
        })),
    );

    // A port that is not there says its arity, so a caller knows how far the
    // node actually goes rather than only that it did not reach.
    match chain
        .document
        .may_relink(ROOT, text, Side::Input, Socket::new(shout, 4))
        .unwrap_err()
    {
        RelinkError::Refused(ConnectError::NoSuchPort { socket, arity }) => {
            assert_eq!(socket, Socket::new(shout, 4));
            assert_eq!(arity, 1);
        }
        other => panic!("expected the port refusal, not {other:?}"),
    }

    // ★★★★★ ASKING MOVES NOTHING — which is the whole difference from finding
    // out by trying, and what lets a screen ask on every pointer move.
    assert_eq!(
        chain.document, before,
        "the document is the one that went in"
    );

    // ★★★★★ AND THE ANSWER IS THE VERB'S OWN. Not a prediction agreeing with
    // it: `relink` calls this, so the pair below cannot come apart. Driven over
    // every link, every end and every socket the graph has, with the verb run
    // on a clone so both are answering the same state.
    let links: Vec<LinkId> = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .map(|link| link.id)
        .collect();
    let nodes: Vec<NodeId> = chain
        .document
        .tree(ROOT)
        .unwrap()
        .nodes()
        .map(|node| node.id)
        .collect();
    let (mut yes, mut no) = (0_usize, 0_usize);
    for link in links {
        for end in [Side::Input, Side::Output] {
            for node in &nodes {
                let signature = chain.document.signature(ROOT, *node).unwrap();
                let arity = match end {
                    Side::Input => signature.inputs.len(),
                    Side::Output => signature.outputs.len(),
                };
                for port in 0..u32::try_from(arity).unwrap() {
                    let socket = Socket::new(*node, port);
                    let asked = chain.document.may_relink(ROOT, link, end, socket);
                    let mut acting = chain.document.clone();
                    let done = acting.relink(ROOT, link, end, socket);
                    assert_eq!(
                        asked.is_ok(),
                        done.is_ok(),
                        "{link:?} {end:?} -> {socket:?}: asked and did disagree"
                    );
                    if let (Err(why), Err(did)) = (&asked, &done) {
                        assert_eq!(why, did, "and they refuse for the same reason");
                    }
                    if asked.is_ok() { yes += 1 } else { no += 1 }
                }
            }
        }
    }
    // Both counts: a surface that answered one way for everything would satisfy
    // the agreement above without saying anything.
    assert!(yes > 0 && no > 0, "{yes} allowed, {no} refused");
}

/// ★★★★★ R1924 — **the move itself**, which this crate has had since R1681 and
/// the census carried as absent until it was measured.
///
/// The engine's commit takes the wire's source pin, the pin it is on and the
/// pin it is going to, and answers a bool. What it cannot do is keep the wire's
/// identity: it has no move, so a relink there is a break and a re-make and
/// everything holding the old wire is holding a dangling name.
///
/// R1924's own finding, and why this proof exists at all: the row's title —
/// *moving an existing wire's end to another port* — covered three engine
/// members, and only two of them were ever missing. Four rounds carried the
/// whole group as absent because nobody measured its clauses apart. That is
/// R1827's rule and R1923's recurrence of it, met a third time.
#[test]
fn engine_schema_try_relink_connection_target() {
    let mut chain = chain();
    let held = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .find(|link| link.from.node == chain.two)
        .map(|link| link.id)
        .expect("the chain wires the first constant into the adder");
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)).and_then(|v| v.number()),
        Some(5)
    );

    let done = chain
        .document
        .relink(ROOT, held, Side::Input, Socket::new(chain.sink, 0))
        .expect("the sink takes a number");
    assert_eq!(
        done.link, held,
        "★ the wire is the SAME wire — its id is intact"
    );
    assert_eq!(done.was, Socket::new(chain.add, 0));
    assert_eq!(done.now, Socket::new(chain.sink, 0));
    assert!(done.moved());
    // What it displaced, named rather than silently dropped: the sink takes one
    // producer and already had `add`.
    assert_eq!(
        done.displaced.map(|link| link.from),
        Some(Socket::new(chain.add, 0)),
        "the wire it pushed off is answered, so an editor can undo the whole act"
    );
    // The graph really changed underneath: the sink now sees the constant.
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)).and_then(|v| v.number()),
        Some(2)
    );

    // Moving an end to where it already is is a SUCCESS that moved nothing —
    // the caller asked for a state and the state holds — and it says so.
    let same = chain
        .document
        .relink(ROOT, held, Side::Input, Socket::new(chain.sink, 0))
        .expect("asking for a state that holds is not a refusal");
    assert!(!same.moved());

    // ★★★★★ AND A REFUSAL MOVES NOTHING AT ALL. R1924 took the decision above
    // the lift, so the refusal path has no undo in it to get wrong — where
    // before it lifted the wire out and leaned on putting it back.
    let before = chain.document.clone();
    assert!(
        chain
            .document
            .relink(ROOT, held, Side::Output, Socket::new(chain.sink, 0))
            .is_err()
    );
    assert_eq!(
        chain.document, before,
        "the whole document, unchanged — every tree, every position, the link \
         order and the id counter"
    );
}

/// ★★★★★ R1919 — the DCC's `find_node`: a name is looked for **across** the
/// document rather than in one tree, and the answer says how to get there.
#[test]
fn dcc_find_node() {
    // ★★★★★ R1919 — the DCC's `find_node` and the engine's five per-editor
    // finds: a search across the tree AND the path to the hit. Six census rows,
    // one mechanism, and the pin's own `covered_by` had said so.
    let mut chain = chain();
    let inner = chain.document.group(ROOT, &[chain.add], "Inner").unwrap();
    let outer = chain.document.group(ROOT, &[inner.node], "Outer").unwrap();
    let buried = chain
        .document
        .tree(outer.definition)
        .and_then(|tree| tree.nodes().find(|n| n.label.is_none()).map(|n| n.id));
    assert!(buried.is_some(), "the outer group holds the inner one");
    chain
        .document
        .relabel(outer.definition, buried.unwrap(), Some("needle"))
        .expect("a fresh name is accepted");

    // The empty query is not a query: it contains-matches everything, and a
    // "result" that is the whole document answers nothing.
    assert!(chain.document.find(ROOT, "").is_empty());

    // ★ The hit is TWO trees down and the search started at the root, which is
    // what makes this a search across the tree rather than a lookup in one.
    let hits = chain.document.find(ROOT, "needle");
    assert_eq!(hits.len(), 1, "one node answers to it: {hits:?}");
    let hit = &hits[0];
    assert_eq!(hit.shown, "needle");
    assert_eq!(hit.because, Matched::Label, "a person called it that");
    // ★★★★★ The way IN is returned — neither reference publishes it, both only
    // perform the descent — and it is returned as this crate's OWN editing
    // position, so a caller hands it to its editor rather than replaying it.
    assert_eq!(
        hit.at.depth(),
        1,
        "one group away from where the search began"
    );
    assert_eq!(
        hit.at.current(),
        outer.definition,
        "and that is where it lives"
    );
    assert_eq!(
        hit.at.entries().last().and_then(|e| e.via),
        Some(outer.node),
        "the group descended is named"
    );
    // ★ The path a search builds and one a reader walks by hand are the SAME
    // value, which is what makes it handable to an editor.
    let mut walked = EditPath::root();
    walked.enter(&chain.document, outer.node).unwrap();
    assert_eq!(hit.at, walked);

    // ★ Case-insensitive containment over the name a reader SEES, so a node
    // nobody named is findable by the only word a reader has for it — and the
    // two answers stay apart in one list.
    let by_kind = chain.document.find(ROOT, "grou");
    assert!(
        by_kind.iter().any(|f| f.because == Matched::Kind),
        "an unnamed node answers by its body's own word: {by_kind:?}"
    );
    assert!(
        by_kind
            .iter()
            .all(|f| f.shown.to_lowercase().contains("grou")),
        "and every hit really carries the needle"
    );

    // ★ Shallowest first, so a reader meets the nodes nearest to where they
    // already are before the ones several groups down.
    let depths: Vec<usize> = by_kind.iter().map(|f| f.at.depth()).collect();
    let mut sorted = depths.clone();
    sorted.sort_unstable();
    assert_eq!(depths, sorted, "breadth first: {depths:?}");

    assert!(
        chain
            .document
            .find(ROOT, "nothing answers to this")
            .is_empty(),
        "and a needle nothing carries finds nothing"
    );
}

/// The DCC's "enter / exit a group".
#[test]
fn dcc_group_enter_exit() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();

    let mut path = EditPath::root();
    path.enter(&chain.document, made.node).unwrap();
    let back = path.exit().unwrap();
    assert_eq!(back, ROOT);
    assert_eq!(path.current(), ROOT);
    assert_eq!(path.depth(), 0);
    assert!(path.exit().is_err(), "there is nothing above the root");
}

/// The DCC's "go to parent tree" — the same exit, reached from a nesting two
/// deep so that "parent" is not a synonym for "root".
#[test]
fn dcc_tree_path_parent() {
    let mut chain = chain();
    let inner = chain.document.group(ROOT, &[chain.add], "Inner").unwrap();
    let outer = chain.document.group(ROOT, &[inner.node], "Outer").unwrap();

    let mut path = EditPath::root();
    path.enter(&chain.document, outer.node).unwrap();
    let inner_instance = chain
        .document
        .tree(outer.definition)
        .unwrap()
        .nodes()
        .find(|n| matches!(n.body, NodeBody::Group(_)))
        .map(|n| n.id)
        .unwrap();
    path.enter(&chain.document, inner_instance).unwrap();
    assert_eq!(path.depth(), 2);

    assert_eq!(path.exit().unwrap(), outer.definition);
    assert_eq!(path.depth(), 1);
}

// ============================================================== the interface

/// Add a port to a definition's interface. Every instance gains the socket at
/// once, because an instance's signature IS the interface.
#[test]
fn dcc_interface_item_new() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let before = chain
        .document
        .signature(ROOT, made.node)
        .unwrap()
        .inputs
        .len();

    let index = chain
        .document
        .expose(
            made.definition,
            InterfaceSide::Input,
            Port::new("Extra", Ty::Number),
        )
        .unwrap();
    assert_eq!(index as usize, before);
    assert_eq!(
        chain
            .document
            .signature(ROOT, made.node)
            .unwrap()
            .inputs
            .len(),
        before + 1
    );
    // ★ R1925 — the operator's other two clauses. It offers INPUT, OUTPUT and
    // PANEL, and until this round only the first was ever run.
    interface_item_new_output_and_panel_clauses();
}

/// Remove one. This is the direction that can invalidate indices, so the links
/// that had to go are **named with the tree they were in** — including the ones
/// at instances, which live in another tree entirely.
#[test]
fn dcc_interface_item_remove() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let arity = chain
        .document
        .signature(ROOT, made.node)
        .unwrap()
        .inputs
        .len();

    let dropped = chain
        .document
        .unexpose(made.definition, InterfaceSide::Input, 0)
        .unwrap();
    assert!(
        dropped.iter().any(|d| d.tree == ROOT),
        "a link at the instance is in the ROOT tree, not the definition's"
    );
    assert_eq!(
        chain
            .document
            .signature(ROOT, made.node)
            .unwrap()
            .inputs
            .len(),
        arity - 1
    );
    assert!(chain.document.validate().is_empty());
}

/// ★ A **composition claim**, which is why it is worth a test: the pin says
/// duplicating an interface item is `expose` called again with the same port.
/// Running it is what shows the claim holds — the copy is a distinct port with
/// its own index, the original's wiring is untouched, and every instance grows.
#[test]
fn dcc_interface_item_duplicate() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let original = chain
        .document
        .tree(made.definition)
        .unwrap()
        .interface()
        .inputs()[0]
        .clone();
    let wired = chain
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(made.node, 0))
        .is_some();
    assert!(wired);

    let copy = chain
        .document
        .expose(made.definition, InterfaceSide::Input, original.clone())
        .unwrap();

    let interface = chain.document.tree(made.definition).unwrap().interface();
    assert_ne!(copy, 0);
    assert_eq!(interface.inputs()[copy as usize].name, original.name);
    assert_eq!(
        interface.inputs()[copy as usize].value_type(),
        original.value_type()
    );
    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(made.node, 0))
            .is_some(),
        "duplicating a port does not disturb what the original one carries"
    );
    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(made.node, copy))
            .is_none(),
        "and the copy arrives unwired"
    );
}

// ================================================== R1925 — interface sections

/// ★★★★★ R1925 — the rest of `interface_item_new`, which
/// [`dcc_interface_item_new`] proves only the first clause of.
///
/// Called from that proof rather than registered beside it, because the row owns
/// one proof and this is the same row: the operator's `item_type` enum has three
/// members and the pin was carrying all three on a test that reached INPUT.
fn interface_item_new_output_and_panel_clauses() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();

    // The OUTPUT clause — true before this round and never run.
    let outputs = chain
        .document
        .tree(made.definition)
        .unwrap()
        .interface()
        .outputs()
        .len();
    let emitted = chain
        .document
        .expose(
            made.definition,
            InterfaceSide::Output,
            Port::new("Spare", Ty::Number),
        )
        .unwrap();
    assert_eq!(emitted as usize, outputs);
    assert_eq!(
        chain
            .document
            .signature(ROOT, made.node)
            .unwrap()
            .outputs
            .len(),
        outputs + 1,
        "an instance emits what its definition exposes"
    );

    // ★★★★★ The PANEL clause, which `expose` cannot answer at all: measured at
    // this round's open the pin read the whole operator as covered by
    // `Document::expose / unexpose`, so a third of its own enum was carried as
    // `have` by a proof that never reached it — the direction R1602 says costs a
    // test, because it inflates the number silently.
    let section = chain
        .document
        .add_section(made.definition, "Falloff")
        .unwrap();
    let held = chain
        .document
        .expose(
            made.definition,
            InterfaceSide::Input,
            Port::new("Radius", Ty::Number),
        )
        .unwrap();
    chain
        .document
        .assign_section(made.definition, InterfacePort::input(held), Some(section))
        .unwrap();

    let interface = chain.document.tree(made.definition).unwrap().interface();
    assert_eq!(interface.sections().len(), 1);
    assert_eq!(interface.section(section).unwrap().name(), "Falloff");
    assert_eq!(
        interface.section_of(InterfacePort::input(held)),
        Some(section),
        "the port the author put in the section reads back in it"
    );
    assert!(
        !interface.ungathered().contains(&InterfacePort::input(held)),
        "and is no longer part of the header-less run"
    );
    // A section is collapsible, which is what the covering sentence claimed and
    // nothing here could do: the state is the document's, so a definition that
    // is copied elsewhere arrives arranged the way it was left.
    assert!(!interface.section(section).unwrap().folded());
    assert!(
        !chain
            .document
            .set_section_folded(made.definition, section, true)
            .unwrap()
    );
    assert!(
        chain
            .document
            .tree(made.definition)
            .unwrap()
            .interface()
            .section(section)
            .unwrap()
            .folded()
    );
    assert!(chain.document.validate().is_empty());
}

/// ★★★★★ R1925 — `interface_item_new_panel_toggle`: a **new** switchable input
/// that stands for its section.
///
/// The reference makes a boolean socket, moves it to position 0 of the panel and
/// flags it. Here the type comes from the taxonomy's own declaration, so the
/// operation cannot invent a socket type this application does not have — and
/// the switch is listed first without the port's INDEX moving, so no link at any
/// instance is re-aimed.
#[test]
fn dcc_interface_item_new_panel_toggle() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let before = chain
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(made.node, 0))
        .map(|link| link.id);
    assert!(before.is_some(), "the instance arrives wired at input 0");

    let section = chain
        .document
        .add_section(made.definition, "Falloff")
        .unwrap();
    let ordinary = chain
        .document
        .expose(
            made.definition,
            InterfaceSide::Input,
            Port::new("Radius", Ty::Number),
        )
        .unwrap();
    chain
        .document
        .assign_section(
            made.definition,
            InterfacePort::input(ordinary),
            Some(section),
        )
        .unwrap();

    let switch = chain
        .document
        .new_section_switch(made.definition, section)
        .unwrap();

    let interface = chain.document.tree(made.definition).unwrap().interface();
    assert_eq!(interface.section(section).unwrap().switch(), Some(switch));
    assert_eq!(
        interface.inputs()[switch as usize].value_type(),
        Some(&Ty::Flag),
        "the new port carries the type the taxonomy declared, not a guess"
    );
    assert_eq!(
        interface.inputs()[switch as usize].name,
        "Falloff",
        "a port with no authored name takes the section's — the one place the \
         reference's naming is right"
    );
    assert_eq!(
        interface.section(section).unwrap().members(),
        [InterfacePort::input(switch), InterfacePort::input(ordinary)],
        "the switch is shown first"
    );
    assert!(
        switch > ordinary,
        "and it is shown first without being INDEXED first — the reference has \
         to move a socket to get this, and a move re-aims every instance's links"
    );
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(made.node, 0))
            .map(|link| link.id),
        before,
        "so the instance's wire at input 0 is the same link it was"
    );

    // ★ The refusal the reference spells *Panel already has a toggle*, as a
    // value, before anything is changed.
    assert_eq!(
        chain
            .document
            .new_section_switch(made.definition, section)
            .unwrap_err(),
        SwitchRefusal::SectionHasSwitch {
            section,
            port: switch
        }
    );
    // ★ And *Active item is not a panel*: a section id this interface never
    // handed out.
    assert_eq!(
        chain
            .document
            .new_section_switch(made.definition, SectionId(9))
            .unwrap_err()
            .wire_word(),
        "no-such-section"
    );
    assert!(chain.document.validate().is_empty());
}

/// ★★★★★ R1925 — `interface_item_make_panel_toggle` and
/// `interface_item_unlink_panel_toggle`, and the property the reference fails.
///
/// Promoting an existing port and demoting it again is the **identity** here.
/// The reference assigns the socket the panel's name on the way in and the
/// panel's name again on the way out, so an authored name does not survive the
/// round trip there; the section carries the header's label and the port keeps
/// its own.
///
/// Both refusals it spells are exercised as values, and both are asked through
/// [`Document::may_make_section_switch`] as well — the same rule at two moments,
/// so a screen that lights what it would accept cannot drift from the edit.
#[test]
fn dcc_interface_item_make_panel_toggle() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let section = chain
        .document
        .add_section(made.definition, "Falloff")
        .unwrap();

    // A number in the section: switchable by position, not by type.
    let number = chain
        .document
        .expose(
            made.definition,
            InterfaceSide::Input,
            Port::new("Radius", Ty::Number),
        )
        .unwrap();
    // A flag OUTSIDE any section: switchable by type, not by position.
    let loose = chain
        .document
        .expose(
            made.definition,
            InterfaceSide::Input,
            Port::new("Use falloff", Ty::Flag),
        )
        .unwrap();
    chain
        .document
        .assign_section(made.definition, InterfacePort::input(number), Some(section))
        .unwrap();

    // ★ *Only boolean input sockets are supported*.
    assert_eq!(
        chain
            .document
            .may_make_section_switch(made.definition, number)
            .unwrap_err(),
        SwitchRefusal::NotSwitchable { index: number }
    );
    // ★ *Socket must be in a panel*.
    assert_eq!(
        chain
            .document
            .may_make_section_switch(made.definition, loose)
            .unwrap_err(),
        SwitchRefusal::NotInASection { index: loose }
    );
    // The question and the act agree: what the question refuses, the edit
    // refuses, with the same value.
    assert_eq!(
        chain
            .document
            .make_section_switch(made.definition, loose)
            .unwrap_err(),
        SwitchRefusal::NotInASection { index: loose }
    );

    chain
        .document
        .assign_section(made.definition, InterfacePort::input(loose), Some(section))
        .unwrap();
    assert_eq!(
        chain
            .document
            .may_make_section_switch(made.definition, loose)
            .unwrap(),
        section,
        "in the section and of the switchable type, the question says yes"
    );
    assert_eq!(
        chain
            .document
            .make_section_switch(made.definition, loose)
            .unwrap(),
        section
    );

    let interface = chain.document.tree(made.definition).unwrap().interface();
    assert_eq!(interface.section(section).unwrap().switch(), Some(loose));
    assert_eq!(
        interface.section(section).unwrap().members().first(),
        Some(&InterfacePort::input(loose)),
        "and it moves to the front of what the section shows"
    );
    assert_eq!(
        interface.inputs()[loose as usize].name,
        "Use falloff",
        "★ the authored name is untouched — the reference overwrites it here"
    );

    interface_item_unlink_panel_toggle_leg(&mut chain, made.definition, section, loose);
}

/// ★★★★★ R1925 — the unlink half of the proof above, which
/// `interface_item_unlink_panel_toggle` cites.
///
/// Split out only because the pair is past this project's line budget for one
/// function; they are one proof, because *demote* is not a claim that can be
/// made without a promotion to undo.
fn interface_item_unlink_panel_toggle_leg(
    chain: &mut Chain,
    definition: TreeId,
    section: SectionId,
    loose: u32,
) {
    // Unlink: no longer the switch, still in the section, still itself.
    assert_eq!(
        chain
            .document
            .unlink_section_switch(definition, section)
            .unwrap(),
        loose
    );
    let interface = chain.document.tree(definition).unwrap().interface();
    assert_eq!(interface.section(section).unwrap().switch(), None);
    assert_eq!(
        interface.section_of(InterfacePort::input(loose)),
        Some(section),
        "*stand-alone* means no longer the switch, not removed from the section"
    );
    assert_eq!(
        interface.inputs()[loose as usize].name,
        "Use falloff",
        "★★★★★ the round trip is the identity — the reference's is not, because \
         its unlink writes the panel's name over the socket's a second time"
    );

    // ★ Its own refusal, which the reference reaches by returning false in
    // silence.
    assert_eq!(
        chain
            .document
            .unlink_section_switch(definition, section)
            .unwrap_err(),
        SwitchRefusal::SectionHasNoSwitch { section }
    );
    assert!(chain.document.validate().is_empty());
}

/// ★★★★★ R1925 — a removal keeps the sections true, and the crate says so
/// about a document that arrives already broken.
///
/// `unexpose` is the only operation here that can shorten a port list, so it is
/// the only place a member index can go stale. Removing a port below the switch
/// slides the switch down with it; removing the switch itself clears it.
#[test]
fn r1925_removing_a_port_keeps_its_section_true() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let section = chain
        .document
        .add_section(made.definition, "Falloff")
        .unwrap();
    let flag = chain
        .document
        .expose(
            made.definition,
            InterfaceSide::Input,
            Port::new("Use falloff", Ty::Flag),
        )
        .unwrap();
    chain
        .document
        .assign_section(made.definition, InterfacePort::input(flag), Some(section))
        .unwrap();
    chain
        .document
        .make_section_switch(made.definition, flag)
        .unwrap();

    // Remove an interface port BELOW the switch: every member slides down.
    chain
        .document
        .unexpose(made.definition, InterfaceSide::Input, 0)
        .unwrap();
    let interface = chain.document.tree(made.definition).unwrap().interface();
    assert_eq!(
        interface.section(section).unwrap().switch(),
        Some(flag - 1),
        "the switch follows the port it names"
    );
    assert_eq!(
        interface.inputs()[(flag - 1) as usize].name,
        "Use falloff",
        "and it still names that port"
    );
    assert!(chain.document.validate().is_empty());

    // Remove the switch itself: the section keeps its other members and has
    // none.
    chain
        .document
        .unexpose(made.definition, InterfaceSide::Input, flag - 1)
        .unwrap();
    let interface = chain.document.tree(made.definition).unwrap().interface();
    assert_eq!(interface.section(section).unwrap().switch(), None);
    assert!(interface.section(section).unwrap().members().is_empty());
    assert!(chain.document.validate().is_empty());
}

/// ★ The pin's reason for this one is that there is **nothing to sync** — the
/// signature is derived rather than stored. That is a claim about a mechanism
/// that does not exist, so the only honest proof is to change the interface and
/// observe the instance follow with no call in between.
#[test]
fn dcc_sockets_sync() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let inside = chain
        .document
        .tree(made.definition)
        .unwrap()
        .interface_node(InterfaceSide::Input)
        .map(|n| n.id)
        .unwrap();

    let before_instance = chain
        .document
        .signature(ROOT, made.node)
        .unwrap()
        .inputs
        .len();
    let before_inside = chain
        .document
        .signature(made.definition, inside)
        .unwrap()
        .outputs
        .len();

    chain
        .document
        .expose(
            made.definition,
            InterfaceSide::Input,
            Port::new("Late", Ty::Number),
        )
        .unwrap();

    assert_eq!(
        chain
            .document
            .signature(ROOT, made.node)
            .unwrap()
            .inputs
            .len(),
        before_instance + 1,
        "the instance answered with the new arity without being told"
    );
    assert_eq!(
        chain
            .document
            .signature(made.definition, inside)
            .unwrap()
            .outputs
            .len(),
        before_inside + 1,
        "and so did the interface node inside the definition"
    );
}

// ================================================================ structure

/// Wire two sockets. A link that could not carry a value is refused rather than
/// drawn, and the refusal names why.
#[test]
fn dcc_link() {
    let mut document: Document<Op> = Document::new("root");
    let two = num(&mut document, 2);
    let double = node(&mut document, Op::Double);
    document
        .connect(ROOT, Socket::new(two, 0), Socket::new(double, 0))
        .unwrap();
    assert_eq!(document.evaluate(ROOT, double), vec![Some(Val::Number(4))]);

    let word = node(&mut document, Op::Word("hi".into()));
    let refused = document.connect(ROOT, Socket::new(word, 0), Socket::new(double, 0));
    assert!(refused.is_err(), "Text does not cross into a Number socket");
}

/// The same verb read as the DCC's "Make Links": a value input takes exactly
/// one producer, so a second wire onto it **displaces** the first and says so,
/// rather than leaving the node with two feeds.
#[test]
fn dcc_link_make() {
    let mut chain = chain();
    let ten = num(&mut chain.document, 10);

    let second = chain
        .document
        .connect(ROOT, Socket::new(ten, 0), Socket::new(chain.add, 0))
        .unwrap();
    let displaced = second.displaced.expect("the first wire was named");
    assert_eq!(displaced.from.node, chain.two);
    assert_eq!(
        chain.document.evaluate(ROOT, chain.add),
        vec![Some(Val::Number(13))]
    );
}

/// ★ A **composition claim**: the DCC's link-cut drags a stroke across the
/// canvas and removes every wire it crossed. What an application has is the
/// set of ids that stroke named, so the proof is that cutting a *set* is a
/// loop over `disconnect` and that each cut hands back the link it removed — which is
/// what an undo needs and what the DCC's operator does not return.
#[test]
fn dcc_links_cut() {
    let mut chain = chain();
    let crossed: Vec<_> = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .filter(|link| link.to.node == chain.add)
        .map(|link| link.id)
        .collect();
    assert_eq!(crossed.len(), 2);

    let mut removed = Vec::new();
    for link in crossed {
        removed.push(chain.document.disconnect(ROOT, link).unwrap());
    }
    assert_eq!(removed.len(), 2);
    assert_eq!(chain.document.tree(ROOT).unwrap().links().len(), 1);

    for link in removed {
        chain.document.connect(ROOT, link.from, link.to).unwrap();
    }
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)),
        Some(Val::Number(5)),
        "each cut handed back what it removed, so the cut is undoable"
    );
}

/// The DCC's "Detach Links": the node stays, its wires do not, and what the
/// graph loses is reported rather than discovered.
#[test]
fn dcc_links_detach() {
    let mut chain = chain();
    let rewired = chain.document.detach(ROOT, chain.add).unwrap();

    assert!(chain.document.tree(ROOT).unwrap().node(chain.add).is_some());
    assert!(links_touching(&chain.document, ROOT, chain.add).is_empty());
    assert_eq!(rewired.removed.len() + rewired.severed.len(), 3);
}

/// A muted link stops the value and keeps the wire. It is a different word
/// from a bypassed node because it is the opposite behaviour, and the DCC
/// spells both "mute".
#[test]
fn dcc_links_mute() {
    let mut chain = chain();
    let link = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .find(|l| l.from.node == chain.two)
        .map(|l| l.id)
        .unwrap();

    assert!(!chain.document.set_link_muted(ROOT, link, true).unwrap());
    assert_eq!(
        chain.document.evaluate(ROOT, chain.add),
        vec![Some(Val::Number(3))],
        "the muted feed falls back to the port's declared default of 0"
    );
    assert_eq!(
        chain.document.tree(ROOT).unwrap().links().len(),
        3,
        "and the wire is still there to be drawn"
    );
}

/// Delete. The links that had to go with the node are named, so an editor never
/// has to scan for what it just broke.
#[test]
fn dcc_delete() {
    let mut chain = chain();
    let removed = chain.document.remove_node(ROOT, chain.add).unwrap();

    assert_eq!(removed.links.len(), 3);
    assert!(chain.document.tree(ROOT).unwrap().node(chain.add).is_none());
    assert!(chain.document.validate().is_empty());
}

/// Delete and reconnect. The DCC's own description is "remove nodes and
/// reconnect nodes **as if deletion was muted**", so the reconnection is the
/// bypass derivation applied to the structure — one rule, not two.
#[test]
fn dcc_delete_reconnect() {
    let mut document: Document<Op> = Document::new("root");
    let two = num(&mut document, 2);
    let double = node(&mut document, Op::Double);
    let sink = node(&mut document, Op::Sink);
    wire(&mut document, two, 0, double, 0);
    wire(&mut document, double, 0, sink, 0);

    let rewired = document.dissolve(ROOT, double).unwrap();
    assert_eq!(rewired.bridged.len(), 1);
    assert!(document.tree(ROOT).unwrap().node(double).is_none());
    assert_eq!(document.evaluate(ROOT, sink), Vec::new());
    assert_eq!(
        document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(sink, 0))
            .map(|l| l.from.node),
        Some(two),
        "the upstream took the deleted node's place"
    );
}

/// Bypass: the node stays and the values pass through it. The route is derived
/// from the signature alone, so unplugging a different port cannot change
/// which value leaves by which output — the property the DCC's
/// wiring-sensitive scoring does not have.
#[test]
fn dcc_mute_toggle() {
    let mut document: Document<Op> = Document::new("root");
    let two = num(&mut document, 2);
    let double = node(&mut document, Op::Double);
    let sink = node(&mut document, Op::Sink);
    wire(&mut document, two, 0, double, 0);
    wire(&mut document, double, 0, sink, 0);
    assert_eq!(document.evaluate(ROOT, double), vec![Some(Val::Number(4))]);

    assert!(!document.set_bypassed(ROOT, double, true).unwrap());
    let route = document.passthrough(ROOT, double).unwrap();
    assert!(route.is_identity());
    assert!(route.dropped_outputs().is_empty());
    assert_eq!(
        document.evaluate(ROOT, double),
        vec![Some(Val::Number(2))],
        "the input arrives at the output unchanged"
    );
    assert!(document.tree(ROOT).unwrap().node(double).is_some());
}

/// Change what a node IS without changing which node it is. The DCC creates a
/// new node and deletes the old one, so every reference to it dies; here the
/// id, the position and the frame membership all survive and what did not fit
/// is reported.
#[test]
fn dcc_swap_node() {
    let mut chain = chain();
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(chain.add))
        .unwrap()
        .label = Some("keep me".into());

    let swapped = chain.document.set_kind(ROOT, chain.add, Op::Mul).unwrap();

    let after = chain.document.tree(ROOT).unwrap().node(chain.add).unwrap();
    assert_eq!(after.id, chain.add);
    assert_eq!(after.label.as_deref(), Some("keep me"));
    assert!(
        swapped
            .carried
            .iter()
            .any(|c| c.by_name && c.from == PortRef::input(0)),
        "`Augend` is on both kinds, so the author's own name carried the wire"
    );
    assert_eq!(
        chain.document.evaluate(ROOT, chain.add),
        vec![Some(Val::Number(6))]
    );
}

// ================================================================ appearance

/// Hide the ports nothing is wired to. The answer is a *derivation* over the
/// declaration and the wiring together, which only the document can make.
#[test]
fn dcc_collapse_hide_unused_toggle() {
    let mut chain = chain();
    let ports = chain.document.visible_ports(ROOT, chain.add).unwrap();
    assert_eq!(ports.hidden_count(), 0);

    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(chain.add))
        .unwrap()
        .appearance
        .hide_unused_ports = true;

    let ports = chain.document.visible_ports(ROOT, chain.add).unwrap();
    assert_eq!(
        ports.inputs,
        vec![0, 1],
        "both inputs are wired, so both stay"
    );

    let lonely = node(&mut chain.document, Op::Add);
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(lonely))
        .unwrap()
        .appearance
        .hide_unused_ports = true;
    let ports = chain.document.visible_ports(ROOT, lonely).unwrap();
    assert!(ports.inputs.is_empty());
    assert_eq!(ports.hidden_inputs, vec![0, 1]);
}

/// ★★★★★ R1912 — the engine's *can this pin be split*, answered with a REASON.
///
/// Measured in the engine's node source, that predicate is a conjunction of
/// five conditions — the pin belongs to this node, it is connectable, **nothing
/// is linked to it**, and its type is a struct, with the base class answering
/// `false` so a kind opts in — and its schema adds a sixth at the moment of
/// splitting: the type must not be a container. One boolean comes back, so its
/// own editor can only grey a menu entry out.
///
/// Here each condition is a named arm, because each wants a different repair,
/// and the members come back with the answer so a caller that asked does not
/// derive it a second way to draw it.
#[test]
fn engine_node_can_split_pin() {
    let mut chain = chain();
    let carry = node(&mut chain.document, Op::Carry);

    let members = chain
        .document
        .splittable(ROOT, carry, Side::Input, 1)
        .expect("`Whole` carries the composite type");
    assert_eq!(members.len(), 2, "one port per member, in order");

    assert_eq!(
        chain
            .document
            .splittable(ROOT, carry, Side::Input, 0)
            .unwrap_err(),
        NotSplittable::Control,
    );
    assert_eq!(
        chain
            .document
            .splittable(ROOT, carry, Side::Input, 2)
            .unwrap_err(),
        NotSplittable::Container,
        "a container of a splittable element, told apart from an atom",
    );
    assert_eq!(
        chain
            .document
            .splittable(ROOT, chain.add, Side::Input, 0)
            .unwrap_err(),
        NotSplittable::Wired {
            side: Side::Input,
            index: 0
        },
        "the reference's own `LinkedTo.Num() == 0`",
    );
}

/// ★★★★★ R1916 — the DESCRIPTION cluster: the two hooks that make a port able
/// to SAY what it is for.
///
/// Its own list for the reason the others have one — `engine_proofs` is past
/// the length this project lets a function have — and the two belong together:
/// the reference splits them between the node and the schema, and each half is
/// a different claim. One is that the sentence exists at all; the other is that
/// it is COMPOSED with what the type says, which is the half the reference's
/// own base implementation does not do.
fn engine_description_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "node::GetPinHoverText",
            engine_node_get_pin_hover_text,
        ),
        proof(
            "engine",
            "schema::ConstructBasicPinTooltip",
            engine_schema_construct_basic_pin_tooltip,
        ),
    ]
}

/// ★★★★★ R1916 — the engine's `GetPinHoverText`: **a port carries a sentence
/// about what it is for.**
///
/// Its shape there is a **node-side hook taking a pin and filling in a string**
/// — so the sentence is the node's opinion of one of its pins, asked for on
/// demand and stored nowhere.
///
/// Here the sentence is the PORT's, which is what lets it travel with the thing
/// it describes: a member port a split makes carries its own, and a variadic
/// run's template carries one for every item it will produce. A node-side hook
/// would have had to be re-asked for each of those and given the same answer by
/// hand.
#[test]
fn engine_node_get_pin_hover_text() {
    let mut chain = chain();
    let carry = node(&mut chain.document, Op::Carry);

    let said = chain
        .document
        .port_tooltip(ROOT, carry, Side::Input, &PortPath::root(1))
        .expect("`Whole` is a port");
    assert_eq!(said.says.as_deref(), Some("the pair this node carries"));

    // ★ A port with nothing to add says `None` rather than an empty string.
    // Absent and blank are different answers and collapsing them is the escape
    // hatch this workspace refuses at the door.
    let go = chain
        .document
        .port_tooltip(ROOT, carry, Side::Input, &PortPath::root(0))
        .expect("`Go` is a port");
    assert_eq!(go.says, None);

    // ★★★★★ And a MEMBER a split made carries its own, which the reference
    // cannot reach: its sub-pins are pins, so the node would have to recognise
    // each one by name to answer differently for it.
    chain
        .document
        .split_port(ROOT, carry, Side::Input, &PortPath::root(1))
        .expect("`Whole` splits");
    let left = chain
        .document
        .port_tooltip(ROOT, carry, Side::Input, &PortPath::root(1).then(0))
        .expect("the left half is a port");
    assert_eq!(left.name, "Left");
    assert_eq!(
        left.member_of,
        Some(PortPath::root(1).then(0)),
        "★ and it says it is a half",
    );
}

/// ★★★★★ R1916 — the engine's `ConstructBasicPinTooltip`: **the sentence is
/// composed with what the TYPE says.**
///
/// ⚠ This is where the reference is wrong, measured this round rather than
/// assumed. It is a **schema-side hook taking a pin, a description GIVEN TO IT,
/// and a string to fill in** — so the description arrives from outside, with
/// nothing in the model saying where from — and its base implementation is one
/// line that hands that description straight back unchanged, while the comment
/// directly above it promises the hook "tacks on any other data important to
/// the schema (things like the pin's type, etc.)".
///
/// ⇒ the composition the documentation describes does not happen, and there is
/// no one place it could be checked from.
///
/// Here `Document::port_tooltip` IS that place, and this is the check.
#[test]
fn engine_schema_construct_basic_pin_tooltip() {
    let mut chain = chain();
    let carry = node(&mut chain.document, Op::Carry);

    let said = chain
        .document
        .port_tooltip(ROOT, carry, Side::Input, &PortPath::root(1))
        .expect("`Whole` is a port");
    assert_eq!(
        said.carries.as_deref(),
        Some("two numbers written `left|right`"),
        "★ the TYPE's half is present, which is the half the reference's base \
         implementation drops",
    );

    // ★★★★★ BOTH halves reach the rendering, and the rendering is DERIVED from
    // the pieces rather than stored beside them — so a consumer reading the
    // fields and a consumer reading the sentence cannot disagree.
    let sentence = said.sentence();
    assert!(sentence.contains("two numbers written"), "{sentence}");
    assert!(
        sentence.contains("the pair this node carries"),
        "{sentence}"
    );
    assert!(sentence.contains("Whole"), "{sentence}");

    // ★ And the facts the reference's single string loses: which way it faces,
    // how many links it may hold, and whether anything is on it.
    assert_eq!(said.side, Side::Input);
    assert_eq!(said.multiplicity, Multiplicity::One);
    assert!(!said.wired);
    assert!(sentence.contains("accepts one"), "{sentence}");

    // ★ A type that says nothing composes to a sentence that says nothing
    // about the type — not to one with a hole in it.
    let loose = chain
        .document
        .port_tooltip(ROOT, carry, Side::Input, &PortPath::root(2))
        .expect("`Loose` is a port");
    assert_eq!(loose.carries, None);
    assert!(!loose.sentence().contains("  "), "{:?}", loose.sentence());
}

/// ★★★★★ R1914 — the engine's `SplitPin`: **a composite value port becomes one
/// port per member, and the parent's value is shared out across them.**
///
/// Measured in its schema at R1913: it sets the parent `bHidden = true`, refuses
/// a container, makes one sub-pin per member with a name derived from the
/// parent's, and **parses the parent's authored value into per-member
/// defaults** — the half a split that knew only the members' types would leave
/// a reader to fill in again.
///
/// Past the reference in two ways this asserts:
///
/// * the parent's hiding has a **reason** ([`Hidden::Split`]), where the
///   reference sets a flag whose cause is recoverable only by noticing the pin
///   has sub-pins;
/// * the split says **what it did** — the member addresses, the ports that
///   moved, and where each piece of the value landed. Its command answers
///   `void`.
#[test]
fn engine_schema_split_pin() {
    let mut chain = chain();
    let carry = node(&mut chain.document, Op::Carry);
    let whole = PortPath::root(1);

    let apart = chain
        .document
        .split_port(ROOT, carry, Side::Input, &whole)
        .expect("`Whole` carries the composite type and nothing is wired to it");

    assert_eq!(
        apart.members,
        [PortPath::root(1).then(0), PortPath::root(1).then(1)],
        "one address per member, in declaration order",
    );
    let names: Vec<String> = chain
        .document
        .signature(ROOT, carry)
        .expect("the node is there")
        .inputs
        .into_iter()
        .map(|port| port.name)
        .collect();
    assert_eq!(
        names,
        ["Go", "Whole", "Left", "Right", "Loose", "Tail"],
        "★ the members take the parent's place and the parent keeps its own — \
         the reference's order, and everything after it moved by two",
    );
    assert_eq!(
        apart.moved,
        [
            (PortRef::input(2), PortRef::input(4)),
            (PortRef::input(3), PortRef::input(5)),
        ],
        "★ the ports with no visual cue: they had nothing to do with the split",
    );

    let seen = chain
        .document
        .visible_ports(ROOT, carry)
        .expect("the node is there");
    assert_eq!(
        seen.why_hidden(Side::Input, 1),
        Some(Hidden::Split),
        "★★★★★ hidden with a REASON, which the reference's flag has nowhere to \
         carry",
    );
    assert!(!seen.inputs.contains(&1));

    // ★★★★★ The value came apart. `Whole` rests at `3|4`, so the members rest
    // at 3 and 4 rather than at the type's declared zeroes.
    let resting: Vec<Option<Val>> = chain
        .document
        .resolved_ports(ROOT, carry, Side::Input)
        .into_iter()
        .map(|(_, port)| port.flow.default_value().cloned())
        .collect();
    assert_eq!(resting[2], Some(Val::Number(3)));
    assert_eq!(resting[3], Some(Val::Number(4)));

    assert_eq!(
        chain
            .document
            .split_port(ROOT, carry, Side::Input, &whole)
            .unwrap_err(),
        NotSplittable::AlreadySplit,
        "★ already done and cannot be done are opposite repairs; the reference \
         answers both with one `false`",
    );
}

/// ★★★★★ R1914 — the engine's `RecombinePin`: **the members go back into the
/// parent, and their value goes with them.**
///
/// ⚠ This is where the reference is **wrong**, measured at R1913 rather than
/// assumed: its recombine re-composes a parent's value with a hand-written
/// `if`-chain over four named struct types, one of which uses a *different
/// member order* from its own split's chain (there is a comment in the source
/// saying so), and every other composite type keeps whatever the parent had.
///
/// Here both directions are declared on the taxonomy that owns the type, so the
/// pair is one author's and [`round_trips`](pinion_node_graph::round_trips) is
/// a law any consumer can run. This asserts the consequence: an edit made on a
/// member arrives on the parent.
#[test]
fn engine_schema_recombine_pin() {
    let mut chain = chain();
    let carry = node(&mut chain.document, Op::Carry);
    let whole = PortPath::root(1);

    chain
        .document
        .split_port(ROOT, carry, Side::Input, &whole)
        .expect("`Whole` splits");
    chain
        .document
        .set_port_value(ROOT, carry, PortRef::input(3), Val::Number(9))
        .expect("`Right` is a value port");

    let folded = chain
        .document
        .recombine_port(ROOT, carry, Side::Input, &whole)
        .expect("`Whole` is split");
    assert_eq!(
        folded.composed,
        Some(Val::Text("3|9".to_owned())),
        "★★★★★ the edit on the member reached the parent — the half the \
         reference has for four named types and no others",
    );
    assert_eq!(
        chain.document.port_value(ROOT, carry, PortRef::input(1)),
        Some(&Val::Text("3|9".to_owned())),
    );
    assert!(
        folded.discarded.is_empty(),
        "★ a value folded INTO the parent is not a value lost",
    );
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, carry)
            .expect("the node is there")
            .why_hidden(Side::Input, 1),
        None,
        "and the parent is drawn again",
    );
}

/// ★★★★★ R1914 — the editor's `SplitStructPin`, which is the **gesture**: it
/// reaches a pin by the index the canvas drew, and everything after that pin
/// moves.
///
/// A separate proof from the schema's because it exercises a separate fact.
/// The reference makes sub-pins real pins, so a wire on a later port has to
/// travel; its own blend-list node ships the mirror-image defect as a `@TODO`
/// ("need to handle moving pins below up correctly"). Every edit here goes
/// through one correspondence, so the wires and the authored values cannot get
/// out of step with each other.
#[test]
fn engine_graph_editor_split_struct_pin() {
    let mut chain = chain();
    let carry = node(&mut chain.document, Op::Carry);
    let downstream = node(&mut chain.document, Op::Carry);

    // A value authored on the port that will move, and a wire on the side the
    // split does not touch. The second is the one with teeth: `remap_ports`
    // severs every link whose port it cannot find, so a split that forgot to
    // say the other side was unchanged would cut this.
    chain
        .document
        .set_port_value(ROOT, carry, PortRef::input(3), Val::Number(7))
        .expect("`Tail` is a value port");
    wire(&mut chain.document, carry, 0, downstream, 1);

    // The gesture arrives with the index the canvas drew, and the verb takes
    // an address — so this is the conversion an editor makes on the way in.
    let path = chain
        .document
        .path_of(ROOT, carry, Side::Input, 1)
        .expect("the canvas drew a pin at index 1");
    let apart = chain
        .document
        .split_port(ROOT, carry, Side::Input, &path)
        .expect("that pin splits");

    assert!(
        apart.severed.is_empty() && apart.discarded.is_empty(),
        "★ nothing was lost: {} wire(s) cut, {} value(s) dropped",
        apart.severed.len(),
        apart.discarded.len(),
    );
    assert_eq!(
        chain.document.port_value(ROOT, carry, PortRef::input(5)),
        Some(&Val::Number(7)),
        "★★★★★ the value followed `Tail` from index 3 to index 5 — the \
         re-indexing the reference's own blend-list node carries a `@TODO` for",
    );
    assert_eq!(
        chain.document.port_value(ROOT, carry, PortRef::input(3)),
        None,
        "and it is not ALSO where it was",
    );
    let leaving: Vec<u32> = chain
        .document
        .tree(ROOT)
        .expect("the tree is there")
        .links()
        .iter()
        .filter(|link| link.from.node == carry)
        .map(|link| link.from.port)
        .collect();
    assert_eq!(
        leaving,
        [0],
        "★ the untouched side kept its wire: a port left out of the \
         correspondence is a port severed",
    );
    assert_eq!(
        chain.document.index_of(ROOT, carry, Side::Input, &path),
        Some(1),
        "and the parent's own address still answers where it is",
    );
}

/// ★★★★★ R1914 — the editor's `RecombineStructPin`, which is **catchable at
/// either end** and folds a whole tree.
///
/// The reference delegates a parent's command to its first sub-pin and walks a
/// sub-pin's up to its parent, so both ends reach the same place. What it
/// cannot do is say which pin it folded or how many splits went with it — and
/// the second matters because the shape is a tree: a member that was itself
/// split stops being a port.
#[test]
fn engine_graph_editor_recombine_struct_pin() {
    let mut chain = chain();
    let carry = node(&mut chain.document, Op::Carry);
    let whole = PortPath::root(1);

    assert_eq!(
        chain
            .document
            .recombine_port(ROOT, carry, Side::Input, &whole)
            .unwrap_err(),
        NotRecombinable::NotSplit,
        "★ nothing to fold, which a greyed-out menu entry cannot say",
    );

    chain
        .document
        .split_port(ROOT, carry, Side::Input, &whole)
        .expect("`Whole` splits");

    // Asked at a MEMBER, not at the parent — the far end of the reference's
    // own parent-pin walk.
    let folded = chain
        .document
        .recombine_port(ROOT, carry, Side::Input, &whole.clone().then(1))
        .expect("`Right` is a member of a split port");
    assert_eq!(
        folded.parent, whole,
        "★★★★★ asking at a member folds the port that member belongs to",
    );
    assert_eq!(folded.folded, 1);
    let names: Vec<String> = chain
        .document
        .signature(ROOT, carry)
        .expect("the node is there")
        .inputs
        .into_iter()
        .map(|port| port.name)
        .collect();
    assert_eq!(
        names,
        ["Go", "Whole", "Loose", "Tail"],
        "and the node is as it was",
    );
}

/// ★★★★★ R1912 — the DCC's socket-hide toggle: a hand puts a node's unwired
/// sockets away **by name**, and pressing again brings them back.
///
/// The reference's operator sets a per-socket user-hidden flag over the unwired
/// sockets and clears every one of them when any is already set. Read whole,
/// its own visibility rule is `!is_user_hidden() && is_available() &&
/// inferred_visibility()` — three independent reasons a socket is not drawn.
/// This crate had the derivation and no way to be told, which is why the row
/// was `absent` while `hide_unused_ports` existed.
///
/// ⚠ The last assertion is the one the derivation cannot make, and it is what
/// makes this a different capability rather than a second spelling: the
/// reference re-derives its set from the wiring every press, so a socket it
/// hid comes back the moment something is wired to it. A declaration does not.
#[test]
fn dcc_hide_socket_toggle() {
    let mut chain = chain();
    let lonely = node(&mut chain.document, Op::Add);

    let done = chain
        .document
        .put_away_ports(ROOT, lonely, PutAway::Unwired)
        .unwrap();
    assert_eq!(
        done.len(),
        3,
        "an unwired Add has two inputs and one output, and the reference's own \
         operator takes all of them"
    );
    let ports = chain.document.visible_ports(ROOT, lonely).unwrap();
    assert!(ports.nothing_drawn(), "which the reference permits too");
    assert_eq!(ports.why_hidden(Side::Input, 0), Some(Hidden::PutAway));

    assert_eq!(chain.document.restore_ports(ROOT, lonely), Some(3));
    assert!(
        !chain
            .document
            .visible_ports(ROOT, lonely)
            .unwrap()
            .nothing_drawn()
    );

    // ★ And it STAYS away when wired, where the reference's derivation would
    // draw it again.
    chain
        .document
        .put_away_ports(ROOT, chain.add, PutAway::Port(Side::Input, 0))
        .unwrap();
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, chain.add)
            .unwrap()
            .why_hidden(Side::Input, 0),
        Some(Hidden::PutAway),
        "input 0 of `add` is wired in this fixture"
    );
}

/// ★★★★★ R1912 — the engine's *remove this struct var pin*: one named pin goes
/// away and the rest stay.
///
/// Measured in the engine's editor source, this and its two siblings call the
/// node's own remove-field-pins with a given-pin selector — they touch neither
/// sub-pins nor a parent pin, so they are HIDING and not splitting. The census
/// had all three filed under struct-pin splitting, which is the grouping this
/// round re-cut.
#[test]
fn engine_graph_editor_remove_this_struct_var_pin() {
    let mut chain = chain();
    chain
        .document
        .put_away_ports(ROOT, chain.add, PutAway::Port(Side::Input, 1))
        .unwrap();
    let ports = chain.document.visible_ports(ROOT, chain.add).unwrap();
    assert_eq!(ports.inputs, vec![0]);
    assert_eq!(ports.outputs, vec![0], "only the named pin went");
    assert_eq!(ports.why_hidden(Side::Input, 1), Some(Hidden::PutAway));
}

/// ★★★★★ R1912 — the engine's *remove all other pins*: everything but the named
/// one goes away, which is its own command's description verbatim.
#[test]
fn engine_graph_editor_remove_other_struct_var_pins() {
    let mut chain = chain();
    chain
        .document
        .put_away_ports(ROOT, chain.add, PutAway::AllOthers(Side::Input, 0))
        .unwrap();
    let ports = chain.document.visible_ports(ROOT, chain.add).unwrap();
    assert_eq!(ports.inputs, vec![0]);
    assert!(ports.outputs.is_empty(), "both sides, as the engine's does");
    assert_eq!(ports.hidden_count(), 2);
}

/// ★★★★★ R1912 — the engine's *restore all structure pins*, and the count its
/// own command is greyed out by.
///
/// The engine gates that command on "not all pins are shown"; here the same
/// fact is the return value, so a host does not need a second query to know
/// whether offering it would do anything.
#[test]
fn engine_graph_editor_restore_all_struct_var_pins() {
    let mut chain = chain();
    assert_eq!(
        chain.document.restore_ports(ROOT, chain.add),
        Some(0),
        "nothing is away yet, which is the greyed-out state"
    );
    chain
        .document
        .put_away_ports(ROOT, chain.add, PutAway::AllOthers(Side::Input, 0))
        .unwrap();
    assert_eq!(chain.document.restore_ports(ROOT, chain.add), Some(2));
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, chain.add)
            .unwrap()
            .hidden_count(),
        0
    );
}

/// Collapse: drawn small, and the same request about unused ports. Two
/// booleans rather than one state, so un-collapsing restores what the node was
/// already saying instead of a default.
#[test]
fn dcc_hide_toggle() {
    let mut chain = chain();
    let lonely = node(&mut chain.document, Op::Add);
    {
        let looks = &mut chain
            .document
            .tree_mut(ROOT)
            .and_then(|t| t.node_mut(lonely))
            .unwrap()
            .appearance;
        looks.hide_unused_ports = true;
        looks.collapsed = true;
    }
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, lonely)
            .unwrap()
            .hidden_count(),
        3
    );

    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(lonely))
        .unwrap()
        .appearance
        .collapsed = false;
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, lonely)
            .unwrap()
            .hidden_count(),
        3,
        "un-collapsing restored the node's own answer about unused ports"
    );
}

/// Whether the node's own controls are drawn. What a control *is* belongs to
/// the application; whether it is on screen travels with the node — and the
/// evaluator may not read it.
#[test]
fn dcc_options_toggle() {
    let mut chain = chain();
    let before = chain.document.evaluate(ROOT, chain.add);
    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .appearance
            .show_options
    );

    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(chain.add))
        .unwrap()
        .appearance
        .show_options = false;

    assert!(
        !chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .appearance
            .show_options
    );
    assert_eq!(chain.document.evaluate(ROOT, chain.add), before);
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, chain.add)
            .unwrap()
            .hidden_count(),
        0
    );
}

/// Whether the node's preview is drawn. Same axis, separate memory.
#[test]
fn dcc_preview_toggle() {
    let mut chain = chain();
    assert!(
        !chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .appearance
            .show_preview
    );
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(chain.add))
        .unwrap()
        .appearance
        .show_preview = true;

    let looks = &chain
        .document
        .tree(ROOT)
        .unwrap()
        .node(chain.add)
        .unwrap()
        .appearance;
    assert!(looks.show_preview);
    assert!(looks.show_options, "the two are independent");
}

/// An authored size. A width is `Option` because an application has its own
/// default; a **height** is `Option` for a different reason — an ordinary node's
/// height is a function of its ports and a frame's is not.
#[test]
fn dcc_resize() {
    let mut chain = chain();
    let frame = chain
        .document
        .enframe(ROOT, &[chain.add], None)
        .unwrap()
        .frame;
    {
        let looks = &mut chain
            .document
            .tree_mut(ROOT)
            .and_then(|t| t.node_mut(frame))
            .unwrap()
            .appearance;
        looks.width = Some(320);
        looks.height = Some(200);
    }
    let looks = &chain
        .document
        .tree(ROOT)
        .unwrap()
        .node(frame)
        .unwrap()
        .appearance;
    assert_eq!((looks.width, looks.height), (Some(320), Some(200)));
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .appearance,
        Appearance::default(),
        "an ordinary node derives its height and is left alone"
    );
}

// ==================================================================== frames

/// Fence a selection into a frame. The frame is a node, so it can be moved,
/// saved and asked about like any other — and it has no ports, so nothing links
/// to it and evaluation never reaches it.
#[test]
fn dcc_join() {
    let mut chain = chain();
    let enframed = chain
        .document
        .enframe(ROOT, &[chain.two, chain.add], Some("decode".into()))
        .unwrap();

    assert_eq!(enframed.members.len(), 2);
    assert_eq!(chain.document.members(ROOT, enframed.frame).len(), 2);
    let frame = chain
        .document
        .tree(ROOT)
        .unwrap()
        .node(enframed.frame)
        .unwrap();
    assert!(frame.is_frame());
    assert_eq!(frame.label.as_deref(), Some("decode"));
    let signature = chain.document.signature(ROOT, enframed.frame).unwrap();
    assert!(signature.inputs.is_empty() && signature.outputs.is_empty());
}

/// The same verb applied inside an existing frame. The new frame lands **inside
/// whatever already contained all of the selection**, so framing part of a
/// pipeline does not lift that part out of the pipeline — and the nodes that
/// were not selected stay where they were.
#[test]
fn dcc_join_nodes() {
    let mut chain = chain();
    let outer = chain
        .document
        .enframe(
            ROOT,
            &[chain.two, chain.three, chain.add],
            Some("pipeline".into()),
        )
        .unwrap();

    let inner = chain
        .document
        .enframe(ROOT, &[chain.two, chain.three], Some("sources".into()))
        .unwrap();

    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(inner.frame)
            .unwrap()
            .parent,
        Some(outer.frame),
        "the new frame joined the frame that already held all of the selection"
    );
    assert_eq!(
        chain.document.ancestry(ROOT, chain.two),
        vec![outer.frame, inner.frame],
        "the containment chain reads outermost first, the way a breadcrumb does"
    );
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .parent,
        Some(outer.frame),
        "and the member that was not selected did not move"
    );
}

/// Attach a node to a frame directly.
#[test]
fn dcc_attach() {
    let mut chain = chain();
    let frame = chain
        .document
        .enframe(ROOT, &[chain.add], None)
        .unwrap()
        .frame;

    let previous = chain
        .document
        .set_parent(ROOT, chain.two, Some(frame))
        .unwrap();
    assert_eq!(previous, None);
    assert!(chain.document.members(ROOT, frame).contains(&chain.two));
}

/// Detach one. The DCC's operator clears the parent outright, so only the
/// all-the-way form is reachable there; it is reachable here too, and the node
/// lands on the canvas rather than in limbo.
#[test]
fn dcc_detach() {
    let mut chain = chain();
    let frame = chain
        .document
        .enframe(ROOT, &[chain.add, chain.two], None)
        .unwrap()
        .frame;

    let left = chain.document.unframe(ROOT, &[chain.two]).unwrap();
    assert_eq!(left, vec![chain.two]);
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.two)
            .unwrap()
            .parent,
        None
    );
    assert_eq!(chain.document.members(ROOT, frame), vec![chain.add]);

    assert_eq!(
        chain.document.set_parent(ROOT, chain.add, None).unwrap(),
        Some(frame)
    );
    assert!(chain.document.members(ROOT, frame).is_empty());
}

/// Set a node's parent, and the two rules that makes it a forest: a container
/// must be a frame, and nothing may contain itself. The DCC states both as
/// assertions its shipped build compiles out, and its own operator detaches
/// before it attaches so the cycle guard cannot fire even in a debug build.
#[test]
fn dcc_parent_set() {
    let mut chain = chain();
    let outer = chain
        .document
        .enframe(ROOT, &[chain.add], None)
        .unwrap()
        .frame;
    let inner = chain
        .document
        .enframe(ROOT, &[chain.two], None)
        .unwrap()
        .frame;
    chain.document.set_parent(ROOT, inner, Some(outer)).unwrap();

    assert!(
        chain
            .document
            .set_parent(ROOT, chain.three, Some(chain.add))
            .is_err(),
        "an ordinary node is not a container"
    );
    let cycle = chain.document.set_parent(ROOT, outer, Some(inner));
    assert!(
        cycle.is_err(),
        "and a frame may not contain its own container"
    );
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(inner)
            .unwrap()
            .parent,
        Some(outer),
        "a refused parenting leaves the forest untouched"
    );
}

// ============================================================ fragments

/// Copy. A fragment is a **value**: it carries the nodes, the definitions they
/// depend on and the boundary it was cut from, and it serializes — which is what
/// a clipboard between two documents actually needs.
#[test]
fn dcc_clipboard_copy() {
    let chain = chain();
    let fragment = chain.document.extract(ROOT, &[chain.add]).unwrap();

    assert_eq!(fragment.node_count(), 1);
    assert_eq!(fragment.inbound().len(), 2, "the two feeds it was cut from");
    assert_eq!(fragment.outbound().len(), 1);
    assert_eq!(fragment.source_tree(), ROOT);

    let wire_form = serde_json::to_string(&fragment).unwrap();
    let back: Fragment<Op> = serde_json::from_str(&wire_form).unwrap();
    assert_eq!(back.node_count(), 1);
    assert!(back.validate().is_empty());
}

/// Paste. The fragment goes anywhere, re-using the definitions that are already
/// there rather than forking them.
#[test]
fn dcc_clipboard_paste() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let fragment = chain.document.extract(ROOT, &[made.node]).unwrap();

    let pasted = chain
        .document
        .insert(
            ROOT,
            &fragment,
            (900, 900),
            Crossings::Drop,
            Definitions::Share,
        )
        .unwrap();
    assert_eq!(pasted.nodes.len(), 1);
    assert!(pasted.definitions_added.is_empty());
    assert_eq!(pasted.definitions_reused, vec![made.definition]);
    assert_eq!(chain.document.instance_count(made.definition), 2);

    let landed = pasted.nodes[0];
    assert_ne!(landed, made.node);
    assert_eq!(
        (
            chain.document.tree(ROOT).unwrap().node(landed).unwrap().x,
            chain.document.tree(ROOT).unwrap().node(landed).unwrap().y
        ),
        (900, 900)
    );
}

/// Duplicate — extract and insert in one call, offset from the original.
#[test]
fn dcc_duplicate() {
    let mut chain = chain();
    let copy = chain
        .document
        .duplicate(
            ROOT,
            &[chain.two, chain.add],
            (40, 40),
            Crossings::KeepInbound,
            Definitions::Share,
        )
        .unwrap();

    assert_eq!(copy.nodes.len(), 2);
    assert!(
        copy.nodes
            .iter()
            .all(|n| *n != chain.two && *n != chain.add)
    );
    assert_eq!(
        copy.links.len(),
        1,
        "the wire inside the selection came along"
    );
    assert!(chain.document.validate().is_empty());
}

// ============================================================== selection

/// What feeds this. The DCC walks one hop per keypress with nothing telling
/// you when the picture has stopped changing; the reach is a parameter here,
/// and `added` is what says the transitive walk is done.
#[test]
fn dcc_select_linked_from() {
    let chain = chain();
    let direct = chain
        .document
        .grow(ROOT, &[chain.sink], Grow::Upstream(Reach::Direct))
        .unwrap();
    assert_eq!(direct.added, vec![chain.add]);

    let all = chain
        .document
        .grow(ROOT, &[chain.sink], Grow::Upstream(Reach::Transitive))
        .unwrap();
    assert_eq!(all.added, vec![chain.two, chain.three, chain.add]);
    let again = chain
        .document
        .grow(ROOT, &all.selection, Grow::Upstream(Reach::Transitive))
        .unwrap();
    assert!(
        !again.changed(),
        "the walk reports that it has reached the end"
    );
}

/// What this feeds — the other direction of the same relation, read the same
/// way.
#[test]
fn dcc_select_linked_to() {
    let chain = chain();
    let all = chain
        .document
        .grow(ROOT, &[chain.two], Grow::Downstream(Reach::Transitive))
        .unwrap();
    assert_eq!(all.added, vec![chain.add, chain.sink]);
    assert!(
        !chain
            .document
            .grow(ROOT, &[chain.two], Grow::Downstream(Reach::Direct))
            .unwrap()
            .added
            .contains(&chain.sink),
        "one hop stops at the adder"
    );
}

/// The DCC's "select grouped by type". Keyed on the whole selection rather
/// than on an active node, because a selection belongs to the editor and this
/// crate has no notion of which of them is active.
#[test]
fn dcc_select_grouped() {
    let chain = chain();
    let same = chain
        .document
        .grow(ROOT, &[chain.two], Grow::SameKind)
        .unwrap();
    assert_eq!(same.added, vec![chain.three]);
    assert!(!same.selection.contains(&chain.add));
}

/// The DCC steps the selection to the *next* node of the same kind. The run is
/// the answer that step walks, produced once.
#[test]
fn dcc_select_same_type_step() {
    let mut chain = chain();
    let four = num(&mut chain.document, 4);
    let run = chain.document.same_kind_run(ROOT, chain.two).unwrap();

    assert_eq!(run, vec![chain.two, chain.three, four]);
    assert_eq!(
        chain.document.same_kind_run(ROOT, chain.sink).unwrap(),
        vec![chain.sink],
        "a kind with one member is a run of one, not an absence"
    );
}

// ============================================================ the engine
// Engine

/// Break every link on a node, leaving the node. ★ A **composition claim**: the
/// crate's `disconnect` takes one link, so the proof is that an application can
/// name the set and that the node survives — which is the whole difference from
/// deleting it.
#[test]
fn engine_graph_editor_break_node_links() {
    let mut chain = chain();
    let touching = links_touching(&chain.document, ROOT, chain.add);
    assert_eq!(touching.len(), 3);

    for link in touching {
        chain.document.disconnect(ROOT, link).unwrap();
    }
    assert!(links_touching(&chain.document, ROOT, chain.add).is_empty());
    assert!(chain.document.tree(ROOT).unwrap().node(chain.add).is_some());
    assert!(chain.document.validate().is_empty());
}

/// Break every link on one **pin**. The narrower reading of the same
/// composition, and the one that shows what a pin is here: a [`Socket`] is a
/// port index on a node and says nothing about which side it is, because the
/// side is decided by **which end of the link it sits on**. So an input pin's
/// links are `link_into`, an output pin's are the links leaving it, and
/// `Socket::new(add, 0)` names both — which is why a filter that ignores the
/// end matches twice.
#[test]
fn engine_graph_editor_break_pin_links() {
    let mut chain = chain();
    let pin = Socket::new(chain.add, 0);
    let either_end = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .filter(|link| link.to == pin || link.from == pin)
        .count();
    assert_eq!(
        either_end, 2,
        "input 0 and output 0 of one node are the same Socket value"
    );

    let incoming = chain
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(pin)
        .unwrap()
        .id;
    chain.document.disconnect(ROOT, incoming).unwrap();
    assert!(chain.document.tree(ROOT).unwrap().link_into(pin).is_none());
    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(chain.add, 1))
            .is_some(),
        "the node's other input pin is untouched"
    );
    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .links()
            .iter()
            .any(|l| l.from == pin),
        "and so is the output pin that shares the index"
    );
}

/// Break one named link, and hand it back so it can be put back.
#[test]
fn engine_graph_editor_break_this_link() {
    let mut chain = chain();
    let id = chain.document.tree(ROOT).unwrap().links()[0].id;
    let removed = chain.document.disconnect(ROOT, id).unwrap();

    assert_eq!(removed.id, id);
    assert_eq!(chain.document.tree(ROOT).unwrap().links().len(), 2);
    chain
        .document
        .connect(ROOT, removed.from, removed.to)
        .unwrap();
    assert_eq!(chain.document.tree(ROOT).unwrap().links().len(), 3);
}

/// Collapse a selection into a subgraph node.
#[test]
fn engine_graph_editor_collapse_nodes() {
    let mut chain = chain();
    let made = chain
        .document
        .group(ROOT, &[chain.add], "Collapsed")
        .unwrap();

    assert!(matches!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(made.node)
            .unwrap()
            .body,
        NodeBody::Group(_)
    ));
    assert_eq!(chain.document.evaluate(ROOT, chain.sink), Vec::new());
    assert_eq!(
        chain.document.evaluate(ROOT, made.node),
        vec![Some(Val::Number(5))]
    );
}

/// The engine's "collapse to function": what separates a function from a
/// one-off subgraph is that it is **callable again**, so the proof
/// instantiates it a second time and checks the two occurrences do not share a
/// value.
#[test]
fn engine_graph_editor_collapse_selection_to_function() {
    let mut chain = chain();
    let made = chain
        .document
        .group(ROOT, &[chain.add], "Function")
        .unwrap();
    let second = chain
        .document
        .instantiate(ROOT, made.definition, 0, 400)
        .unwrap();

    let four = num(&mut chain.document, 4);
    wire(&mut chain.document, four, 0, second, 0);
    wire(&mut chain.document, four, 0, second, 1);
    assert_eq!(
        chain.document.evaluate(ROOT, second),
        vec![Some(Val::Number(8))]
    );
    assert_eq!(
        chain.document.evaluate(ROOT, made.node),
        vec![Some(Val::Number(5))]
    );
    assert_eq!(chain.document.instance_count(made.definition), 2);
}

/// The engine's "collapse to macro": a macro is the reading of the same
/// boundary that gets **expanded back into the caller**, so the proof is that
/// the collapse is reversible into the host with the value unchanged.
#[test]
fn engine_graph_editor_collapse_selection_to_macro() {
    let mut chain = chain();
    let before = chain.document.evaluate(ROOT, chain.sink);
    let made = chain.document.group(ROOT, &[chain.add], "Macro").unwrap();

    let expanded = chain.document.ungroup(ROOT, made.node).unwrap();
    assert_eq!(expanded.nodes.len(), 1);
    assert_eq!(chain.document.evaluate(ROOT, chain.sink), before);
    assert!(chain.document.tree(ROOT).unwrap().node(made.node).is_none());
}

/// The engine's comment box. Structurally a frame: it holds a region of
/// canvas, its members compute exactly as before, and the boundary means
/// nothing to the evaluator.
#[test]
fn engine_graph_editor_create_comment() {
    let mut chain = chain();
    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    let comment = chain
        .document
        .enframe(ROOT, &[chain.two, chain.three], Some("inputs".into()))
        .unwrap();

    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(comment.frame)
            .unwrap()
            .is_frame()
    );
    assert_eq!(arrives(&chain.document, Socket::new(chain.sink, 0)), before);

    let moved = chain
        .document
        .translate(ROOT, comment.frame, 100, 0)
        .unwrap();
    assert_eq!(
        moved,
        vec![comment.frame, chain.two, chain.three],
        "dragging a comment carries what it contains, the frame named first"
    );
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .x,
        0
    );
}

/// The engine's delete-and-reconnect, over a selection rather than one node —
/// the reading that shows the derivation composes.
#[test]
fn engine_graph_editor_delete_and_reconnect_nodes() {
    let mut document: Document<Op> = Document::new("root");
    let two = num(&mut document, 2);
    let first = node(&mut document, Op::Double);
    let second = node(&mut document, Op::Double);
    let sink = node(&mut document, Op::Sink);
    wire(&mut document, two, 0, first, 0);
    wire(&mut document, first, 0, second, 0);
    wire(&mut document, second, 0, sink, 0);

    for target in [first, second] {
        document.dissolve(ROOT, target).unwrap();
    }
    assert_eq!(
        document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(sink, 0))
            .map(|l| l.from.node),
        Some(two)
    );
    assert_eq!(document.tree(ROOT).unwrap().node_count(), 2);
}

/// The engine's Disable Nodes. Its own semantics are the pass-through this
/// crate derives, and the outputs no input can feed are **named** rather than
/// being discovered as a missing wire.
#[test]
fn engine_graph_editor_disable_nodes() {
    let mut chain = chain();
    assert!(!chain.document.set_bypassed(ROOT, chain.add, true).unwrap());
    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .bypassed
    );

    let route = chain.document.passthrough(ROOT, chain.add).unwrap();
    assert_eq!(route.source_of(0), Some(0));
    assert_eq!(
        chain.document.evaluate(ROOT, chain.add),
        vec![Some(Val::Number(2))],
        "the first feed passes straight through"
    );
}

/// And back on. The flag is the graph's *meaning* rather than its looks, so it
/// is a field of the node and not of `Appearance`.
#[test]
fn engine_graph_editor_enable_nodes() {
    let mut chain = chain();
    chain.document.set_bypassed(ROOT, chain.add, true).unwrap();
    assert!(chain.document.set_bypassed(ROOT, chain.add, false).unwrap());

    assert!(
        !chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .bypassed
    );
    assert_eq!(
        chain.document.evaluate(ROOT, chain.add),
        vec![Some(Val::Number(5))]
    );
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .appearance,
        Appearance::default(),
        "enabling a node changed nothing about how it is drawn"
    );
}

/// The engine's Expand Node — the inverse of a collapse, back into the caller.
#[test]
fn engine_graph_editor_expand_nodes() {
    let mut chain = chain();
    let made = chain
        .document
        .group(ROOT, &[chain.two, chain.add], "Sub")
        .unwrap();
    let out = chain.document.ungroup(ROOT, made.node).unwrap();

    assert_eq!(out.nodes.len(), 2);
    assert!(out.definition_unused);
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)),
        Some(Val::Number(5))
    );
    assert_eq!(chain.document.tree(ROOT).unwrap().node_count(), 4);
}

/// The engine hides unconnected pins.
#[test]
fn engine_graph_editor_hide_no_connection_pins() {
    let mut chain = chain();
    let mul = node(&mut chain.document, Op::Mul);
    wire(&mut chain.document, chain.two, 0, mul, 1);
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(mul))
        .unwrap()
        .appearance
        .hide_unused_ports = true;

    let ports = chain.document.visible_ports(ROOT, mul).unwrap();
    assert_eq!(ports.inputs, vec![1]);
    assert_eq!(ports.hidden_inputs, vec![0]);
}

/// ★ the engine's *other* hide command keeps a pin that has a **default**,
/// because a defaulted pin still carries a value the reader wants to see. A
/// **composition claim**: this crate publishes the hidden set and the
/// signature says which ports have defaults, so the narrower rule is a filter
/// an application writes — proven here by writing it.
#[test]
fn engine_graph_editor_hide_no_connection_no_default_pins() {
    let mut chain = chain();
    let mul = node(&mut chain.document, Op::Mul);
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(mul))
        .unwrap()
        .appearance
        .hide_unused_ports = true;

    let ports = chain.document.visible_ports(ROOT, mul).unwrap();
    let signature = chain.document.signature(ROOT, mul).unwrap();
    assert_eq!(ports.hidden_inputs, vec![0, 1], "neither is wired");

    let no_default: Vec<u32> = ports
        .hidden_inputs
        .iter()
        .copied()
        .filter(|index| signature.inputs[*index as usize].default_value().is_none())
        .collect();
    assert_eq!(
        no_default,
        vec![1],
        "`Augend` has a default and survives the narrower rule; `Factor` does not"
    );
}

/// The engine's Show All Pins, which is the same declaration read the other
/// way.
#[test]
fn engine_graph_editor_show_all_pins() {
    let mut chain = chain();
    let lonely = node(&mut chain.document, Op::Add);
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(lonely))
        .unwrap()
        .appearance
        .hide_unused_ports = true;
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, lonely)
            .unwrap()
            .hidden_count(),
        3
    );

    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(lonely))
        .unwrap()
        .appearance
        .hide_unused_ports = false;
    let ports = chain.document.visible_ports(ROOT, lonely).unwrap();
    assert_eq!(ports.hidden_count(), 0);
    assert_eq!(ports.inputs, vec![0, 1]);
    assert_eq!(ports.outputs, vec![0]);
}

/// The engine promotes a selection to a re-usable function on the visual
/// script. Same boundary, read as a definition that outlives the place it came
/// from: the proof deletes the original instance and instantiates the
/// definition again.
#[test]
fn engine_graph_editor_promote_selection_to_function() {
    let mut chain = chain();
    let made = chain
        .document
        .group(ROOT, &[chain.add], "Promoted")
        .unwrap();
    chain.document.remove_node(ROOT, made.node).unwrap();
    assert_eq!(chain.document.instance_count(made.definition), 0);

    let fresh = chain
        .document
        .instantiate(ROOT, made.definition, 10, 10)
        .unwrap();
    let two = num(&mut chain.document, 6);
    wire(&mut chain.document, two, 0, fresh, 0);
    wire(&mut chain.document, two, 0, fresh, 1);
    assert_eq!(
        chain.document.evaluate(ROOT, fresh),
        vec![Some(Val::Number(12))]
    );
}

/// And to a macro, whose reading is expansion — proven by round-tripping the
/// boundary and asserting the graph means the same thing afterwards.
#[test]
fn engine_graph_editor_promote_selection_to_macro() {
    let mut chain = chain();
    let before = chain.document.evaluate(ROOT, chain.sink);
    let made = chain
        .document
        .group(ROOT, &[chain.two, chain.add], "Promoted")
        .unwrap();
    let expanded = chain.document.ungroup(ROOT, made.node).unwrap();

    assert_eq!(expanded.nodes.len(), 2);
    assert_eq!(chain.document.evaluate(ROOT, chain.sink), before);
    assert!(chain.document.validate().is_empty());
}

/// ★ the engine's Reconstruct Node re-reads a node against a signature that
/// has changed underneath it. The pin's reason is that a signature here is
/// **derived**, so there is nothing to reconstruct — a claim about an absence,
/// proven by changing the interface and observing the instance follow with the
/// links that no longer fit **named**.
#[test]
fn engine_graph_editor_reconstruct_nodes() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let before = chain
        .document
        .signature(ROOT, made.node)
        .unwrap()
        .inputs
        .len();

    let dropped = chain
        .document
        .unexpose(made.definition, InterfaceSide::Input, 0)
        .unwrap();

    assert_eq!(
        chain
            .document
            .signature(ROOT, made.node)
            .unwrap()
            .inputs
            .len(),
        before - 1,
        "the instance answers with the new arity, with no reconstruct step"
    );
    assert_eq!(
        dropped.len(),
        2,
        "the wire inside and the wire at the instance"
    );
    assert!(chain.document.validate().is_empty());
}

/// The engine resets a pin to the value its declaration gives it. The authored
/// value is what a node carries when nothing else supplies one, so clearing it
/// has to leave **no** authored value — asserted apart from the value that
/// then arrives, because "removed" and "overwritten with the default" look
/// identical if only the second is checked.
#[test]
fn engine_graph_editor_reset_pin_to_default_value() {
    let mut chain = chain();
    let mul = node(&mut chain.document, Op::Mul);
    wire(&mut chain.document, chain.two, 0, mul, 1);
    assert_eq!(
        chain.document.evaluate(ROOT, mul),
        vec![Some(Val::Number(4))]
    );

    chain
        .document
        .set_port_value(ROOT, mul, PortRef::input(0), Val::Number(10))
        .unwrap();
    assert_eq!(
        chain.document.port_value(ROOT, mul, PortRef::input(0)),
        Some(&Val::Number(10))
    );
    assert_eq!(
        chain.document.evaluate(ROOT, mul),
        vec![Some(Val::Number(20))]
    );

    let cleared = chain
        .document
        .clear_port_value(ROOT, mul, PortRef::input(0))
        .unwrap();
    assert_eq!(
        cleared,
        Some(Val::Number(10)),
        "and it says what it removed"
    );
    assert_eq!(
        chain.document.port_value(ROOT, mul, PortRef::input(0)),
        None,
        "the node carries no authored value at all, rather than a copy of the default"
    );
    assert_eq!(
        chain.document.evaluate(ROOT, mul),
        vec![Some(Val::Number(4))]
    );
}

/// The engine selects everything feeding the selection — the transitive
/// question, which is the one a person actually has.
#[test]
fn engine_graph_editor_select_all_input_nodes() {
    let chain = chain();
    let grown = chain
        .document
        .grow(ROOT, &[chain.sink], Grow::Upstream(Reach::Transitive))
        .unwrap();
    assert_eq!(
        grown.selection,
        vec![chain.two, chain.three, chain.add, chain.sink]
    );
}

/// And everything the selection feeds.
#[test]
fn engine_graph_editor_select_all_output_nodes() {
    let chain = chain();
    let grown = chain
        .document
        .grow(
            ROOT,
            &[chain.two, chain.three],
            Grow::Downstream(Reach::Transitive),
        )
        .unwrap();
    assert_eq!(grown.added, vec![chain.add, chain.sink]);
}

// ============================================ the HOOK surface (R1603)

// What follows answers the second surface: not "what can a user invoke" but
// "what does the editor ask the node system to decide". R1593 and R1594 both
// live here and on no operator list anywhere, which is why an operator census
// read them as zero and the coverage judged on top of it was overstated.

/// The DCC calls a node type when the tree changed so it can bring itself up
/// to date. Nothing here is ever told: every derived fact is recomputed on
/// read, so a node cannot be stale.
#[test]
fn dcc_node_updatefunc() {
    let mut chain = chain();
    let lonely = node(&mut chain.document, Op::Add);
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(lonely))
        .unwrap()
        .appearance
        .hide_unused_ports = true;
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, lonely)
            .unwrap()
            .hidden_inputs,
        vec![0, 1]
    );

    wire(&mut chain.document, chain.two, 0, lonely, 1);
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, lonely)
            .unwrap()
            .hidden_inputs,
        vec![0],
        "the node answered the new wiring with nothing having notified it"
    );
}

/// The DCC asks a node type whether its sockets may be re-synchronised with
/// its declaration. There is nothing to synchronise: a node's signature IS its
/// kind's, so changing the kind changes the signature in the same instant.
#[test]
fn dcc_node_can_sync_sockets() {
    let mut chain = chain();
    let before = chain.document.signature(ROOT, chain.add).unwrap();
    assert_eq!(before.inputs[1].name, "Addend");

    chain.document.set_kind(ROOT, chain.add, Op::Mul).unwrap();
    let after = chain.document.signature(ROOT, chain.add).unwrap();
    assert_eq!(after.inputs[1].name, "Factor");
    assert_eq!(
        after.inputs[1].default_value(),
        None,
        "and its declared default came with it"
    );
}

/// The DCC calls a node type to copy its per-node storage. `adopt_from`
/// destructures its source, so a field added to a node fails to compile until
/// someone says whether a copy carries it — where a hand-written copy silently
/// drops it (the defect R1589 found in this crate's own `move_nodes`).
#[test]
fn dcc_node_copyfunc() {
    let mut chain = chain();
    let frame = chain
        .document
        .enframe(ROOT, &[chain.add], None)
        .unwrap()
        .frame;
    chain.document.set_bypassed(ROOT, chain.add, true).unwrap();
    chain
        .document
        .set_port_value(ROOT, chain.add, PortRef::input(0), Val::Number(7))
        .unwrap();
    {
        let held = chain
            .document
            .tree_mut(ROOT)
            .and_then(|t| t.node_mut(chain.add))
            .unwrap();
        held.label = Some("stage".into());
        held.appearance.collapsed = true;
    }

    let copy = chain
        .document
        .duplicate(
            ROOT,
            &[chain.add],
            (10, 10),
            Crossings::Drop,
            Definitions::Share,
        )
        .unwrap();
    let made = chain
        .document
        .tree(ROOT)
        .unwrap()
        .node(copy.nodes[0])
        .unwrap();
    assert_eq!(made.label.as_deref(), Some("stage"));
    assert!(made.bypassed);
    assert!(made.appearance.collapsed);
    assert_eq!(made.values.get(&PortRef::input(0)), Some(&Val::Number(7)));
    assert_eq!(
        made.parent,
        Some(frame),
        "and it lands back inside its fence"
    );
}

/// The DCC calls a node type to initialise a new node. Here a node is born as
/// its kind: `add_node` takes the body, and the ports and their declared
/// defaults are there in the same call.
#[test]
fn dcc_node_initfunc() {
    let mut document: Document<Op> = Document::new("root");
    let add = node(&mut document, Op::Add);

    let signature = document.signature(ROOT, add).unwrap();
    assert_eq!(signature.inputs.len(), 2);
    assert_eq!(signature.inputs[0].default_value(), Some(&Val::Number(0)));
    assert_eq!(signature.inputs[1].default_value(), Some(&Val::Number(1)));
    assert_eq!(
        document.evaluate(ROOT, add),
        vec![Some(Val::Number(1))],
        "and it computes from those defaults with nothing else supplied"
    );
}

/// The DCC calls a node type for the node's displayed label. Here the kind
/// answers and an authored label overrides it, which is one derivation
/// (`display_name`) rather than a callback each type has to remember.
#[test]
fn dcc_node_labelfunc() {
    let mut chain = chain();
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .display_name(),
        "Add"
    );

    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(chain.add))
        .unwrap()
        .label = Some("Total".into());
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .display_name(),
        "Total"
    );
}

/// The DCC's tree type decides whether a wire may exist. Here that is `NodeKind::conversion`,
/// declared once **as the conversion**, so "may this wire exist" and "what
/// arrives along it" are one answer — the DCC keeps three.
#[test]
fn dcc_node_tree_validate_link() {
    let mut document: Document<Op> = Document::new("root");
    let two = num(&mut document, 2);
    let word = node(&mut document, Op::Word("hi".into()));
    let shout = node(&mut document, Op::Shout);
    let double = node(&mut document, Op::Double);

    assert!(
        document
            .connect(ROOT, Socket::new(word, 0), Socket::new(double, 0))
            .is_err()
    );
    document
        .connect(ROOT, Socket::new(two, 0), Socket::new(shout, 0))
        .unwrap();
    assert_eq!(
        document.evaluate(ROOT, shout),
        vec![Some(Val::Text("2".into()))],
        "a number crossed into a Text port through the declared conversion"
    );
}

/// The DCC's tree type is called to update after a change. Here the standing
/// check is `validate`, which is a question rather than a pass an edit has to
/// remember to run — and it answers about a document that arrived from a file
/// just as well as about one this process built.
#[test]
fn dcc_node_tree_update() {
    let mut chain = chain();
    assert!(chain.document.validate().is_empty());
    chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    assert!(chain.document.validate().is_empty());

    let wire_form = serde_json::to_string(&chain.document).unwrap();
    let round_trip: Document<Op> = serde_json::from_str(&wire_form).unwrap();
    assert!(round_trip.validate().is_empty());
}

/// The DCC's tree type makes a local copy of the tree to evaluate. Here a
/// definition is forked, and the fork is independent: an edit through one
/// instance does not reach the other.
#[test]
fn dcc_node_tree_localize() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let second = chain
        .document
        .instantiate(ROOT, made.definition, 0, 300)
        .unwrap();

    let forked = chain.document.fork_definition(ROOT, second).unwrap();
    assert_ne!(forked, made.definition);
    assert_eq!(chain.document.instance_count(made.definition), 1);
    assert_eq!(chain.document.instance_count(forked), 1);

    chain
        .document
        .expose(forked, InterfaceSide::Input, Port::new("Extra", Ty::Number))
        .unwrap();
    assert_eq!(
        chain.document.signature(ROOT, second).unwrap().inputs.len(),
        3
    );
    assert_eq!(
        chain
            .document
            .signature(ROOT, made.node)
            .unwrap()
            .inputs
            .len(),
        2,
        "the original definition is untouched"
    );
}

/// The DCC's socket type builds an interface item from a socket. Here `expose`
/// takes the **port itself**, so an interface port is a port — name, type and
/// declared default together — rather than a second description of one.
#[test]
fn dcc_node_socket_interface_from_socket() {
    let mut document: Document<Op> = Document::new("root");
    let definition = document.add_definition("Def");
    let port = Port::new("Seed", Ty::Number).with_default(Val::Number(4));
    document
        .expose(definition, InterfaceSide::Input, port)
        .unwrap();

    let interface = document.tree(definition).unwrap().interface();
    assert_eq!(interface.inputs()[0].name, "Seed");
    assert_eq!(interface.inputs()[0].value_type(), Some(&Ty::Number));
    assert_eq!(interface.inputs()[0].default_value(), Some(&Val::Number(4)));
}

/// And the other direction: the DCC's socket type initialises a node socket
/// from an interface item. Here an instance's socket **is** the interface
/// port, derived, so the two cannot describe different things.
#[test]
fn dcc_node_socket_interface_init_socket() {
    let mut document: Document<Op> = Document::new("root");
    let definition = document.add_definition("Def");
    document
        .expose(
            definition,
            InterfaceSide::Input,
            Port::new("Seed", Ty::Number).with_default(Val::Number(4)),
        )
        .unwrap();
    let instance = document.instantiate(ROOT, definition, 0, 0).unwrap();

    let signature = document.signature(ROOT, instance).unwrap();
    let interface = document.tree(definition).unwrap().interface();
    assert_eq!(signature.inputs.len(), 1);
    assert_eq!(signature.inputs[0].name, interface.inputs()[0].name);
    assert_eq!(signature.inputs[0].default_value(), Some(&Val::Number(4)));
}

// ---------------------------------------------------------- the engine's node

/// The engine's node allocates its default pins. Here a kind **declares** its
/// ports, so a node's sockets are derived from the kind rather than built by a
/// call the node has to make and could forget.
#[test]
fn engine_node_allocate_default_pins() {
    let mut document: Document<Op> = Document::new("root");
    let mul = node(&mut document, Op::Mul);
    let signature = document.signature(ROOT, mul).unwrap();

    assert_eq!(
        signature
            .inputs
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        Op::Mul
            .inputs()
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(signature.outputs.len(), 1);
}

/// The engine's node is told to destroy itself. Here removal is the
/// document's, and it **names** what went — the links, and the members a
/// deleted frame handed to the frame above rather than stranding on the
/// canvas.
#[test]
fn engine_node_destroy_node() {
    let mut chain = chain();
    let inner = chain
        .document
        .enframe(ROOT, &[chain.add], None)
        .unwrap()
        .frame;
    let outer = chain
        .document
        .enframe(ROOT, &[chain.two], None)
        .unwrap()
        .frame;
    chain.document.set_parent(ROOT, inner, Some(outer)).unwrap();

    let removed = chain.document.remove_node(ROOT, inner).unwrap();
    assert_eq!(removed.adopted, vec![chain.add]);
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .parent,
        Some(outer),
        "the member went to the frame ABOVE, not to the canvas"
    );
}

/// The engine asks a node which pin a value passes through when the node is
/// disabled. Here the answer is derived from the **signature alone**, so
/// unplugging a different port cannot change it — where the engine's own
/// equivalent ranks against a static type table and breaks ties on what
/// happens to be wired.
#[test]
fn engine_node_get_pass_through_pin() {
    let mut chain = chain();
    let before = chain.document.passthrough(ROOT, chain.add).unwrap();
    assert_eq!(before.source_of(0), Some(0));

    let second = chain
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(chain.add, 1))
        .map(|l| l.id)
        .unwrap();
    chain.document.disconnect(ROOT, second).unwrap();
    assert_eq!(
        chain
            .document
            .passthrough(ROOT, chain.add)
            .unwrap()
            .source_of(0),
        Some(0),
        "unplugging the other input did not move the route"
    );
}

/// The engine asks a node for a pin's displayed name. Here a port carries its
/// name and the signature answers it, on an instance as well as on a kind.
#[test]
fn engine_node_get_pin_display_name() {
    let mut chain = chain();
    let signature = chain.document.signature(ROOT, chain.add).unwrap();
    assert_eq!(signature.inputs[0].name, "Augend");
    assert_eq!(signature.outputs[0].name, "Out");

    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let derived = chain.document.signature(ROOT, made.node).unwrap();
    assert_eq!(derived.inputs.len(), 2);
    assert!(derived.inputs.iter().all(|p| !p.name.is_empty()));
}

/// The engine asks a node for the graphs it contains. Here containment is a
/// document-level relation, so the nesting is readable in one call rather than
/// one pointer at a time.
#[test]
fn engine_node_get_sub_graphs() {
    let mut chain = chain();
    let inner = chain.document.group(ROOT, &[chain.add], "Inner").unwrap();
    let outer = chain.document.group(ROOT, &[inner.node], "Outer").unwrap();

    let containment = chain.document.containment();
    assert!(containment.contains(&(outer.definition.0 as usize, inner.definition.0 as usize)));
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(outer.node)
            .unwrap()
            .body,
        NodeBody::Group(outer.definition)
    );
}

/// The engine tells a node its connections changed. Here nothing is told,
/// because nothing is stored: what the node computes is a function of the
/// graph as it is when the question is asked.
#[test]
fn engine_node_node_connection_list_changed() {
    let mut chain = chain();
    assert_eq!(
        chain.document.evaluate(ROOT, chain.add),
        vec![Some(Val::Number(5))]
    );

    let ten = num(&mut chain.document, 10);
    wire(&mut chain.document, ten, 0, chain.add, 0);
    assert_eq!(
        chain.document.evaluate(ROOT, chain.add),
        vec![Some(Val::Number(13))],
        "the new answer needed no notification"
    );
}

/// The engine tells a node one of its pins was removed. Here removing an
/// interface port names every link that had to go **with the tree it was in**
/// — which is the point, since the ones that matter are at instances, in other
/// trees.
#[test]
fn engine_node_on_pin_removed() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let dropped = chain
        .document
        .unexpose(made.definition, InterfaceSide::Input, 0)
        .unwrap();

    assert!(dropped.iter().any(|d| d.tree == ROOT));
    assert!(dropped.iter().any(|d| d.tree == made.definition));
}

/// The engine's node is told it was renamed. Here a label is a field of the
/// node, so a rename is an assignment — and it travels with a copy, which is
/// what makes it a property of the node rather than of the editor.
#[test]
fn engine_node_on_rename_node() {
    let mut chain = chain();
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(chain.add))
        .unwrap()
        .label = Some("Total".into());

    let copy = chain
        .document
        .duplicate(
            ROOT,
            &[chain.add],
            (10, 0),
            Crossings::Drop,
            Definitions::Share,
        )
        .unwrap();
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(copy.nodes[0])
            .unwrap()
            .label
            .as_deref(),
        Some("Total")
    );
}

/// The engine's comment node is told its text changed. A frame's label is the
/// same field an ordinary node's is, so nothing here has a second text model.
#[test]
fn engine_node_on_update_comment_text() {
    let mut chain = chain();
    let frame = chain
        .document
        .enframe(ROOT, &[chain.two], Some("inputs".into()))
        .unwrap()
        .frame;
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(frame)
            .unwrap()
            .display_name(),
        "inputs"
    );

    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(frame))
        .unwrap()
        .label = Some("sources".into());
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(frame)
            .unwrap()
            .display_name(),
        "sources"
    );
}

/// The engine tells one **pin** its connections changed. Here a port's
/// visibility is a derivation over the declaration and the wiring together,
/// per port.
#[test]
fn engine_node_pin_connection_list_changed() {
    let mut chain = chain();
    let mul = node(&mut chain.document, Op::Mul);
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(mul))
        .unwrap()
        .appearance
        .hide_unused_ports = true;
    assert_eq!(
        chain.document.visible_ports(ROOT, mul).unwrap().inputs,
        Vec::<u32>::new()
    );

    wire(&mut chain.document, chain.two, 0, mul, 0);
    assert_eq!(
        chain.document.visible_ports(ROOT, mul).unwrap().inputs,
        vec![0]
    );
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, mul)
            .unwrap()
            .hidden_inputs,
        vec![1]
    );
}

/// The engine tells a node a pin's default value changed. Here the authored
/// value is what the port carries when nothing else supplies one, so writing
/// it changes what the node computes and nothing has to be notified.
#[test]
fn engine_node_pin_default_value_changed() {
    let mut chain = chain();
    let mul = node(&mut chain.document, Op::Mul);
    wire(&mut chain.document, chain.two, 0, mul, 1);
    assert_eq!(
        chain.document.evaluate(ROOT, mul),
        vec![Some(Val::Number(4))]
    );

    let previous = chain
        .document
        .set_port_value(ROOT, mul, PortRef::input(0), Val::Number(5))
        .unwrap();
    assert_eq!(previous, None, "and it says what it replaced");
    assert_eq!(
        chain.document.evaluate(ROOT, mul),
        vec![Some(Val::Number(10))]
    );
}

/// The engine's node is told it was pasted. Here the paste **reports** what it
/// did, so a caller never has to scan for what arrived attached and what did
/// not.
#[test]
fn engine_node_post_paste_node() {
    let mut chain = chain();
    let frame = chain
        .document
        .enframe(ROOT, &[chain.add], None)
        .unwrap()
        .frame;
    let fragment = chain.document.extract(ROOT, &[chain.add]).unwrap();

    let pasted = chain
        .document
        .insert(
            ROOT,
            &fragment,
            (500, 500),
            Crossings::KeepInbound,
            Definitions::Share,
        )
        .unwrap();
    assert_eq!(pasted.nodes.len(), 1);
    assert_eq!(
        pasted.reframed, pasted.nodes,
        "the copy landed back inside the fence its original is in"
    );
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(pasted.nodes[0])
            .unwrap()
            .parent,
        Some(frame)
    );
    assert_eq!(
        pasted.reattached.len(),
        2,
        "and the feeds it was cut from were restored and NAMED"
    );
    assert!(pasted.unattached.is_empty());
}

/// The engine's node is told it was just placed, so it can finish itself. Here
/// a placed node is complete by construction: it answers its signature, its
/// declared defaults and its value in the same breath as its id.
#[test]
fn engine_node_post_placed_new_node() {
    let mut document: Document<Op> = Document::new("root");
    let add = document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 40, 90)
        .unwrap();

    let placed = document.tree(ROOT).unwrap().node(add).unwrap();
    assert_eq!((placed.x, placed.y), (40, 90));
    assert_eq!(placed.appearance, Appearance::default());
    assert_eq!(document.signature(ROOT, add).unwrap().inputs.len(), 2);
    assert_eq!(document.evaluate(ROOT, add), vec![Some(Val::Number(1))]);
    assert!(document.validate().is_empty());
}

/// The engine prepares a node for copying. Here the copy is a **value** that
/// carries the definitions it depends on, so it can be written to a file or
/// sent to another process rather than living inside one editor's clipboard.
#[test]
fn engine_node_prepare_for_copying() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let fragment = chain.document.extract(ROOT, &[made.node]).unwrap();

    assert_eq!(fragment.definitions().count(), 1);
    let wire_form = serde_json::to_string(&fragment).unwrap();
    let back: Fragment<Op> = serde_json::from_str(&wire_form).unwrap();
    assert_eq!(back.definitions().count(), 1);

    let mut elsewhere: Document<Op> = Document::new("other");
    let landed = elsewhere
        .insert(ROOT, &back, (0, 0), Crossings::Drop, Definitions::Share)
        .unwrap();
    assert_eq!(
        landed.definitions_added.len(),
        1,
        "the definition travelled with it"
    );
}

/// The engine resizes a node. A width is authored on any node; a **height** is
/// authored only where nothing derives it, which is what tells a frame apart
/// from a node whose height is a function of its ports.
#[test]
fn engine_node_resize_node() {
    let mut chain = chain();
    let frame = chain
        .document
        .enframe(ROOT, &[chain.add], None)
        .unwrap()
        .frame;
    {
        let looks = &mut chain
            .document
            .tree_mut(ROOT)
            .and_then(|t| t.node_mut(frame))
            .unwrap()
            .appearance;
        looks.width = Some(400);
        looks.height = Some(240);
    }
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(chain.add))
        .unwrap()
        .appearance
        .width = Some(180);

    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(frame)
            .unwrap()
            .appearance
            .height,
        Some(240)
    );
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .appearance
            .height,
        None,
        "an ordinary node's height stays derived even after its width is authored"
    );
}

// -------------------------------------------------------- the engine's schema

/// The engine asks the schema whether two pin types are equivalent. Here that
/// is `NodeKind::conversion` answering `Direct`, which is the same declaration that decides what
/// arrives.
#[test]
fn engine_schema_are_pin_types_equivalent() {
    assert!(matches!(
        Op::conversion(&Ty::Number, &Ty::Number),
        Conversion::Direct
    ));
    assert!(matches!(
        Op::conversion(&Ty::Text, &Ty::Text),
        Conversion::Direct
    ));
    assert!(matches!(
        Op::conversion(&Ty::Number, &Ty::Text),
        Conversion::Converted(_)
    ));
    assert!(Op::conversion(&Ty::Text, &Ty::Number).is_refused());
}

/// The engine asks whether two **pins** are compatible, which is a different
/// question from whether their types are: a port also has a side and a flow.
/// `crossing` is the one question every derivation in this crate asks.
#[test]
fn engine_schema_are_pins_compatible() {
    let number = Port::new("A", Ty::Number);
    let text = Port::new("B", Ty::Text);
    assert!(pinion_node_graph::crossing::<Op>(&number, &text).is_allowed());
    assert!(pinion_node_graph::crossing::<Op>(&number, &text).converts());
    assert!(pinion_node_graph::crossing::<Op>(&text, &number).is_refused());

    let control: Port<Ty, Val> = Port::control("Then");
    assert!(
        pinion_node_graph::crossing::<Op>(&number, &control).is_refused(),
        "a value port and a control port never pair, whatever their types"
    );
}

/// The engine asks the schema whether a connection may be made. Here `connect`
/// answers it and **names** whichever of the four things failed — including
/// the path a refused wire would have closed.
#[test]
fn engine_schema_can_create_connection() {
    let mut document: Document<Op> = Document::new("root");
    let first = node(&mut document, Op::Double);
    let second = node(&mut document, Op::Double);
    wire(&mut document, first, 0, second, 0);

    let cycle = document.connect(ROOT, Socket::new(second, 0), Socket::new(first, 0));
    let error = cycle.unwrap_err();
    let named = format!("{error}");
    assert!(named.contains("cycle") || named.contains("path"), "{named}");

    let word = node(&mut document, Op::Word("x".into()));
    assert!(
        document
            .connect(ROOT, Socket::new(word, 0), Socket::new(first, 0))
            .is_err()
    );
}

/// The engine asks whether a node may be encapsulated into a subgraph. Here
/// the refusal is by **reachability** and it names the walk, where the
/// engine's own `CanEncapuslateNode` answers a bare bool.
#[test]
fn engine_schema_can_encapuslate_node() {
    let mut document: Document<Op> = Document::new("root");
    let source = num(&mut document, 2);
    let outside = node(&mut document, Op::Double);
    let sink = node(&mut document, Op::Double);
    wire(&mut document, source, 0, outside, 0);
    wire(&mut document, outside, 0, sink, 0);

    let refused = document.group(ROOT, &[source, sink], "Bad");
    assert!(
        refused.is_err(),
        "an unselected node both fed by and feeding the selection would make the tree cyclic"
    );
    assert!(
        document
            .group(ROOT, &[source, outside, sink], "Good")
            .is_ok()
    );
}

/// ★ the engine's schema materialises a whole conversion **node** into the
/// graph when a wire needs one, so the graph the user sees is not the graph
/// they drew. Here the conversion is a property of the link and costs no node
/// at all.
#[test]
fn engine_schema_create_automatic_conversion_node_and_connections() {
    let mut document: Document<Op> = Document::new("root");
    let two = num(&mut document, 2);
    let shout = node(&mut document, Op::Shout);
    let before = document.tree(ROOT).unwrap().node_count();

    let made = document
        .connect(ROOT, Socket::new(two, 0), Socket::new(shout, 0))
        .unwrap();
    assert_eq!(document.tree(ROOT).unwrap().node_count(), before);

    let conversion = document.link_conversion(ROOT, made.link).unwrap();
    assert!(conversion.converts(), "and the wire SAYS it converts");
    assert_eq!(
        document.evaluate(ROOT, shout),
        vec![Some(Val::Text("2".into()))]
    );
}

/// The engine asks whether a pin still holds its default value. Here that is
/// the authored value beside the declared one, and the two are separate
/// questions with separate answers.
#[test]
fn engine_schema_does_default_value_match() {
    let mut chain = chain();
    let mul = node(&mut chain.document, Op::Mul);
    let declared = chain.document.signature(ROOT, mul).unwrap().inputs[0]
        .default_value()
        .cloned();
    assert_eq!(declared, Some(Val::Number(2)));
    assert_eq!(
        chain.document.port_value(ROOT, mul, PortRef::input(0)),
        None
    );

    chain
        .document
        .set_port_value(ROOT, mul, PortRef::input(0), Val::Number(2))
        .unwrap();
    assert_eq!(
        chain.document.port_value(ROOT, mul, PortRef::input(0)),
        Some(&Val::Number(2)),
        "authored to the SAME value as the declaration, and still distinguishable from unset"
    );
}

/// The engine asks the schema how to display a graph. Here a tree carries its
/// name and the edit path reads the chain of them, so "where am I" is one
/// call.
#[test]
fn engine_schema_get_graph_display_information() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let mut path = EditPath::root();
    path.enter(&chain.document, made.node).unwrap();

    let crumbs = chain.document.tree(made.definition).map(|t| t.name.clone());
    assert_eq!(crumbs.as_deref(), Some("Sum"));
    assert_eq!(path.breadcrumb(&chain.document).len(), 2);
    assert!(
        path.breadcrumb(&chain.document)
            .iter()
            .any(|c| c.contains("Sum"))
    );
}

/// The engine asks whether a pin's default value is valid. Here the write is
/// **type-checked** through `NodeKind::value_type` and refused by name.
#[test]
fn engine_schema_is_pin_default_valid() {
    let mut chain = chain();
    let mul = node(&mut chain.document, Op::Mul);
    let refused =
        chain
            .document
            .set_port_value(ROOT, mul, PortRef::input(0), Val::Text("nope".into()));
    assert!(refused.is_err());
    assert_eq!(
        chain.document.port_value(ROOT, mul, PortRef::input(0)),
        None
    );

    chain
        .document
        .set_port_value(ROOT, mul, PortRef::input(0), Val::Number(3))
        .unwrap();
    assert_eq!(
        chain.document.port_value(ROOT, mul, PortRef::input(0)),
        Some(&Val::Number(3))
    );
}

/// The engine asks the schema to place a node. Here position is a field, and
/// moving a frame carries what it contains — which is what the containment
/// relation is for.
#[test]
fn engine_schema_set_node_position() {
    let mut chain = chain();
    let frame = chain
        .document
        .enframe(ROOT, &[chain.two, chain.three], None)
        .unwrap()
        .frame;
    let before = chain
        .document
        .tree(ROOT)
        .unwrap()
        .node(chain.two)
        .unwrap()
        .x;

    let moved = chain.document.translate(ROOT, frame, 60, -20).unwrap();
    assert_eq!(moved.len(), 3);
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.two)
            .unwrap()
            .x,
        before + 60
    );
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .x,
        0
    );
}

/// The engine's schema has one setter per value type (`TrySetDefaultValue`, `TrySetDefaultText`, `TrySetDefaultObject`). Here the
/// value is the taxonomy's own, so there is one setter — and it is gated by
/// the **signature**, refusing a port the node does not have and naming the
/// arity.
#[test]
fn engine_schema_try_set_default_value() {
    let mut chain = chain();
    let mul = node(&mut chain.document, Op::Mul);
    let refused = chain
        .document
        .set_port_value(ROOT, mul, PortRef::input(9), Val::Number(1));
    assert!(refused.is_err());
    let named = format!("{}", refused.unwrap_err());
    assert!(named.contains('9') || named.contains('2'), "{named}");

    chain
        .document
        .set_port_value(ROOT, mul, PortRef::output(0), Val::Number(11))
        .unwrap();
    assert_eq!(
        chain.document.port_value(ROOT, mul, PortRef::output(0)),
        Some(&Val::Number(11)),
        "an OUTPUT may be authored too: one sentence covers both sides"
    );
}

// ---------------------------------------------------- R1631 arrangement

/// A card, in the application's own units.
const R1631_CARD: Extent = Extent::new(150, 40);

/// Four nodes scattered on the canvas, in the order they were added.
fn r1631_scattered() -> (Document<Op>, Vec<NodeId>) {
    let mut document = Document::new("root");
    let ids: Vec<NodeId> = [
        (Op::Num(1), 10, 5),
        (Op::Num(2), 40, 90),
        (Op::Add, 200, 30),
        (Op::Sink, 260, 140),
    ]
    .into_iter()
    .map(|(op, x, y)| {
        document
            .add_node(ROOT, NodeBody::Kind(op), x, y)
            .expect("root tree")
    })
    .collect();
    (document, ids)
}

/// The engine's `AlignNodesLeft`: the selection's leading x edge, and NOTHING
/// moves on the other axis.
#[test]
fn engine_graph_editor_align_nodes_left() {
    let (document, ids) = r1631_scattered();
    let picked: BTreeSet<NodeId> = ids.iter().copied().collect();
    let placed =
        Align::to(Axis::Horizontal, Edge::Start).run(&document, ROOT, &picked, |_| R1631_CARD);
    assert!(ids.iter().all(|id| placed.positions()[id].0 == 10));
    for &id in &ids {
        let was = document.tree(ROOT).unwrap().node(id).unwrap();
        assert_eq!(
            placed.positions()[&id].1,
            was.y,
            "a horizontal align keeps y"
        );
    }
}

/// The engine's `AlignNodesRight`: trailing edges meet, so the CARD's width is
/// part of the answer.
#[test]
fn engine_graph_editor_align_nodes_right() {
    let (document, ids) = r1631_scattered();
    let picked: BTreeSet<NodeId> = ids.iter().copied().collect();
    let placed =
        Align::to(Axis::Horizontal, Edge::End).run(&document, ROOT, &picked, |_| R1631_CARD);
    assert!(
        ids.iter()
            .all(|id| placed.positions()[id].0 + R1631_CARD.width == 260 + R1631_CARD.width)
    );
    assert_ne!(
        placed.positions()[&ids[0]].0,
        10,
        "the leading node really moved"
    );
}

/// The engine's `AlignNodesTop`.
#[test]
fn engine_graph_editor_align_nodes_top() {
    let (document, ids) = r1631_scattered();
    let picked: BTreeSet<NodeId> = ids.iter().copied().collect();
    let placed =
        Align::to(Axis::Vertical, Edge::Start).run(&document, ROOT, &picked, |_| R1631_CARD);
    assert!(ids.iter().all(|id| placed.positions()[id].1 == 5));
    for &id in &ids {
        let was = document.tree(ROOT).unwrap().node(id).unwrap();
        assert_eq!(placed.positions()[&id].0, was.x, "a vertical align keeps x");
    }
}

/// The engine's `AlignNodesBottom`.
#[test]
fn engine_graph_editor_align_nodes_bottom() {
    let (document, ids) = r1631_scattered();
    let picked: BTreeSet<NodeId> = ids.iter().copied().collect();
    let placed = Align::to(Axis::Vertical, Edge::End).run(&document, ROOT, &picked, |_| R1631_CARD);
    assert!(
        ids.iter()
            .all(|id| placed.positions()[id].1 + R1631_CARD.height == 140 + R1631_CARD.height)
    );
    assert_ne!(
        placed.positions()[&ids[0]].1,
        5,
        "the top node really moved"
    );
}

/// The engine's `AlignNodesCenter` — the x midline, and the arm that DRIFTS if
/// the rounding goes outward, so it is applied twice against uneven cards.
#[test]
fn engine_graph_editor_align_nodes_center() {
    let (mut document, ids) = r1631_scattered();
    let odd = ids[1];
    let extent = |node: &Node<Op>| {
        if node.id == odd {
            Extent::new(45, 55)
        } else {
            R1631_CARD
        }
    };
    let picked: BTreeSet<NodeId> = ids.iter().copied().collect();
    let once = Align::to(Axis::Horizontal, Edge::Center).run(&document, ROOT, &picked, extent);
    assert!(document.apply(ROOT, &once) > 0, "the first press moves");
    let twice = Align::to(Axis::Horizontal, Edge::Center).run(&document, ROOT, &picked, extent);
    assert_eq!(document.apply(ROOT, &twice), 0, "and the second does not");
}

/// The engine's `AlignNodesMiddle` — the y midline, same idempotence.
#[test]
fn engine_graph_editor_align_nodes_middle() {
    let (mut document, ids) = r1631_scattered();
    let odd = ids[1];
    let extent = |node: &Node<Op>| {
        if node.id == odd {
            Extent::new(45, 55)
        } else {
            R1631_CARD
        }
    };
    let picked: BTreeSet<NodeId> = ids.iter().copied().collect();
    let once = Align::to(Axis::Vertical, Edge::Center).run(&document, ROOT, &picked, extent);
    assert!(document.apply(ROOT, &once) > 0, "the first press moves");
    let twice = Align::to(Axis::Vertical, Edge::Center).run(&document, ROOT, &picked, extent);
    assert_eq!(document.apply(ROOT, &twice), 0, "and the second does not");
}

/// The engine's `DistributeNodesHorizontally`: equal GAPS — not equal pitches —
/// with the extremes pinned. The cards differ in width so the two rules give
/// different answers.
#[test]
fn engine_graph_editor_distribute_nodes_horizontally() {
    let (document, ids) = r1631_scattered();
    let picked: BTreeSet<NodeId> = ids.iter().copied().collect();
    let widths = [40, 100, 20, 60];
    let extent = |node: &Node<Op>| {
        let i = ids.iter().position(|&id| id == node.id).unwrap_or(0);
        Extent::new(widths[i], 40)
    };
    let placed = Distribute::along(Axis::Horizontal).run(&document, ROOT, &picked, extent);
    let mut spans: Vec<(i32, i32)> = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let x = placed.positions()[&id].0;
            (x, x + widths[i])
        })
        .collect();
    spans.sort_unstable();
    assert_eq!(spans[0].0, 10, "the leading extreme is pinned");
    assert_eq!(spans[3].1, 260 + 60, "and the trailing one");
    let gaps: Vec<i32> = spans.windows(2).map(|p| p[1].0 - p[0].1).collect();
    let lo = *gaps.iter().min().expect("gaps");
    let hi = *gaps.iter().max().expect("gaps");
    assert!(
        hi - lo <= 1,
        "equal but for the integer remainder: {gaps:?}"
    );
}

/// The engine's `DistributeNodesVertically` — the same rule on the other axis,
/// which is what makes the axis a parameter rather than two commands.
#[test]
fn engine_graph_editor_distribute_nodes_vertically() {
    let (document, ids) = r1631_scattered();
    let picked: BTreeSet<NodeId> = ids.iter().copied().collect();
    let placed = Distribute::along(Axis::Vertical).run(&document, ROOT, &picked, |_| R1631_CARD);
    let mut ys: Vec<i32> = ids.iter().map(|id| placed.positions()[id].1).collect();
    ys.sort_unstable();
    assert_eq!(ys[0], 5, "pinned");
    assert_eq!(ys[3], 140, "pinned");
    let gaps: Vec<i32> = ys
        .windows(2)
        .map(|p| p[1] - p[0] - R1631_CARD.height)
        .collect();
    let lo = *gaps.iter().min().expect("gaps");
    let hi = *gaps.iter().max().expect("gaps");
    assert!(hi - lo <= 1, "{gaps:?}");
}

/// The engine's `StackNodesHorizontally` — and the gap is a PARAMETER, which is
/// the constant the reference compiles into its editor.
#[test]
fn engine_graph_editor_stack_nodes_horizontally() {
    let (document, ids) = r1631_scattered();
    let picked: BTreeSet<NodeId> = ids.iter().copied().collect();
    for gap in [0, 12, 50] {
        let placed =
            Stack::along(Axis::Horizontal, gap).run(&document, ROOT, &picked, |_| R1631_CARD);
        let mut xs: Vec<i32> = ids.iter().map(|id| placed.positions()[id].0).collect();
        xs.sort_unstable();
        assert_eq!(xs[0], 10, "the run starts where its leading node was");
        assert!(
            xs.windows(2).all(|p| p[1] - p[0] == R1631_CARD.width + gap),
            "gap {gap}: {xs:?}"
        );
    }
}

/// The engine's `StackNodesVertically`, plus the clamp: a negative gap is an
/// overlap, and an overlap is not a stack.
#[test]
fn engine_graph_editor_stack_nodes_vertically() {
    let (document, ids) = r1631_scattered();
    let picked: BTreeSet<NodeId> = ids.iter().copied().collect();
    let placed = Stack::along(Axis::Vertical, 12).run(&document, ROOT, &picked, |_| R1631_CARD);
    let mut ys: Vec<i32> = ids.iter().map(|id| placed.positions()[id].1).collect();
    ys.sort_unstable();
    assert_eq!(ys[0], 5);
    assert!(ys.windows(2).all(|p| p[1] - p[0] == R1631_CARD.height + 12));

    let clamped = Stack::along(Axis::Vertical, -30).run(&document, ROOT, &picked, |_| R1631_CARD);
    let mut tight: Vec<i32> = ids.iter().map(|id| clamped.positions()[id].1).collect();
    tight.sort_unstable();
    assert!(
        tight.windows(2).all(|p| p[1] - p[0] == R1631_CARD.height),
        "clamped to touching: {tight:?}"
    );
}

/// The engine's `StraightenConnections`, and the answer it does not give: a
/// fan-in cannot be straightened, and the leftover link is NAMED.
#[test]
fn engine_graph_editor_straighten_connections() {
    let (mut document, ids) = r1631_scattered();
    wire(&mut document, ids[0], 0, ids[2], 0);
    wire(&mut document, ids[1], 0, ids[2], 1);
    let picked: BTreeSet<NodeId> = ids.iter().copied().collect();

    let done = Straighten::along(Axis::Horizontal).run(&document, ROOT, &picked);
    assert_eq!(
        done.straight().len(),
        1,
        "one producer claimed the consumer"
    );
    assert_eq!(
        done.bent().len(),
        1,
        "and the second is reported, not hidden"
    );
    assert!(!done.is_complete());
    assert_eq!(
        done.placement().positions().get(&ids[2]).map(|p| p.1),
        Some(5),
        "the consumer met the producer that claimed it first"
    );
}

// ======================================================= R1632 — variadic ports

/// A `Sequence` with `count` branches, each wired to its own sink, plus the
/// upstream that fires it.
///
/// Every branch reaches a *different* node, so "the wires moved correctly" is a
/// statement about which sink each branch reaches rather than about how many
/// wires survived — the distinction a re-index bug lives in.
fn sequenced(count: u32) -> (Document<Op>, NodeId, Vec<NodeId>) {
    let mut document: Document<Op> = Document::new("root");
    let sequence = node(&mut document, Op::Sequence);
    for extra in 2..count {
        document
            .insert_item(ROOT, sequence, Side::Output, extra, Item::plain())
            .unwrap();
    }
    let branches = (0..count)
        .map(|branch| {
            let sink = node(&mut document, Op::Sequence);
            document
                .connect(ROOT, Socket::new(sequence, branch), Socket::new(sink, 0))
                .unwrap();
            sink
        })
        .collect();
    (document, sequence, branches)
}

/// Which node each control output of `sequence` reaches.
fn branches_of(document: &Document<Op>, sequence: NodeId) -> Vec<Option<NodeId>> {
    let arity = document.signature(ROOT, sequence).unwrap().outputs.len();
    (0..arity)
        .map(|port| {
            let port = u32::try_from(port).unwrap();
            document
                .tree(ROOT)
                .unwrap()
                .links()
                .iter()
                .find(|link| link.from == Socket::new(sequence, port))
                .map(|link| link.to.node)
        })
        .collect()
}

/// The port names of one side, which is what an editor draws.
fn port_names(document: &Document<Op>, target: NodeId, side: Side) -> Vec<String> {
    let signature = document.signature(ROOT, target).unwrap();
    match side {
        Side::Input => signature.inputs,
        Side::Output => signature.outputs,
    }
    .iter()
    .map(|port| port.name.clone())
    .collect()
}

/// The engine's "adds another execution output pin to an execution sequence or
/// switch node". Appending is `insert_item` at the run's own length.
///
/// What it discriminates: the branch really is a **control** port and not a
/// value one, so the arm added is the one the reference's exec pin is, and the
/// engine's own floor of two (`CanRemoveExecutionPin`: `NumOutPins > 2`) is
/// declared rather than checked at a menu.
#[test]
fn engine_graph_editor_add_execution_pin() {
    let (mut document, sequence, branches) = sequenced(2);
    let before = branches_of(&document, sequence);

    let count = document.items(ROOT, sequence, Side::Output).unwrap().len();
    let change = document
        .insert_item(
            ROOT,
            sequence,
            Side::Output,
            u32::try_from(count).unwrap(),
            Item::plain(),
        )
        .unwrap();

    assert_eq!(change.items, 3);
    assert_eq!(change.added, vec![PortRef::output(2)]);
    assert!(change.is_lossless(), "appending cuts nothing");
    assert_eq!(
        port_names(&document, sequence, Side::Output),
        ["Then 0", "Then 1", "Then 2"]
    );
    let signature = document.signature(ROOT, sequence).unwrap();
    assert!(
        signature.outputs.iter().all(Port::is_control),
        "the pin added is an EXECUTION pin, which is a different plane from a value"
    );
    assert_eq!(
        branches_of(&document, sequence)[..2],
        before[..2],
        "and the branches that were there still reach what they reached"
    );
    assert_eq!(branches_of(&document, sequence)[2], None);
    assert_eq!(branches.len(), 2);
    assert!(document.validate().is_empty());
}

/// The engine's "adds another execution output pin **before** this one".
///
/// What it discriminates against its `After` sibling: the branch that was at
/// `i` must end up at `i + 1`, still reaching the same node. An implementation
/// that inserted after would leave it at `i` and this would fail — which is why
/// the two are separate proofs rather than one with a parameter.
#[test]
fn engine_graph_editor_insert_execution_pin_before() {
    let (mut document, sequence, branches) = sequenced(3);

    let change = document
        .insert_item(ROOT, sequence, Side::Output, 1, Item::plain())
        .unwrap();
    assert!(change.is_lossless());
    assert_eq!(
        branches_of(&document, sequence),
        vec![
            Some(branches[0]),
            None,
            Some(branches[1]),
            Some(branches[2])
        ],
        "the new pin took index 1 and pushed the branch that was there along"
    );
    assert!(
        change
            .moved
            .contains(&(PortRef::output(1), PortRef::output(2))),
        "and the change says so: {:?}",
        change.moved
    );
}

/// The engine's "adds another execution output pin **after** this one".
///
/// The mirror assertion: the branch at `i` stays at `i` and the gap opens
/// below it. Together with the `Before` proof this is what makes "one method
/// and a number" a real answer to the reference's two commands rather than a
/// claim.
#[test]
fn engine_graph_editor_insert_execution_pin_after() {
    let (mut document, sequence, branches) = sequenced(3);

    document
        .insert_item(ROOT, sequence, Side::Output, 1 + 1, Item::plain())
        .unwrap();
    assert_eq!(
        branches_of(&document, sequence),
        vec![
            Some(branches[0]),
            Some(branches[1]),
            None,
            Some(branches[2])
        ],
        "the branch at 1 stayed at 1 and the gap opened below it"
    );
}

/// The engine's `RemoveExecutionPin`, and the half it does not do.
///
/// Its execution-sequence node's `RemovePinFromExecutionNode` calls
/// `MarkAsGarbage`, which reaches the pin's own destructor and
/// `BreakAllPinLinks()`; the command answers `void`, so what it disconnected is
/// gone. Here the cut wire is handed back — and re-making it from the report is
/// what proves the report is enough for an undo.
#[test]
fn engine_graph_editor_remove_execution_pin() {
    let (mut document, sequence, branches) = sequenced(4);

    let change = document
        .remove_item(ROOT, sequence, Side::Output, 1)
        .unwrap();

    assert_eq!(change.items, 3);
    assert_eq!(change.severed.len(), 1);
    assert_eq!(
        change.severed[0].to.node, branches[1],
        "the wire it cut is NAMED, where the reference's returns void"
    );
    assert_eq!(
        branches_of(&document, sequence),
        vec![Some(branches[0]), Some(branches[2]), Some(branches[3])],
        "and the branches below moved up, still reaching what they reached"
    );

    // The report is enough to put it back, which is the test of "enough".
    document
        .connect(ROOT, Socket::new(sequence, 2), Socket::new(branches[1], 0))
        .unwrap();
    assert_eq!(branches_of(&document, sequence)[2], Some(branches[1]));

    // And the floor is the operation's, not a menu's.
    document
        .remove_item(ROOT, sequence, Side::Output, 0)
        .unwrap();
    assert_eq!(
        document.remove_item(ROOT, sequence, Side::Output, 0),
        Err(ItemError::AtMinimum {
            side: Side::Output,
            min: 2
        })
    );
}

/// The engine's `AddOptionPin` — the same operation on the INPUT side, and the
/// case that makes re-indexing observable at all.
///
/// The selector's `Index` input sits **after** the options, so adding one moves
/// a port that has nothing to do with the edit. Nothing on the node says so,
/// and an implementation that only re-indexed within the run would silently
/// re-point whatever was wired to `Index`.
#[test]
fn engine_graph_editor_add_option_pin() {
    let mut document: Document<Op> = Document::new("root");
    let choose = node(&mut document, Op::Choose);
    let picker = num(&mut document, 1);
    let first = num(&mut document, 10);
    wire(&mut document, first, 0, choose, 0);
    wire(&mut document, picker, 0, choose, 2);
    assert_eq!(
        arrives(&document, Socket::new(choose, 2)),
        Some(Val::Number(1)),
        "the Index is the third input while there are two options"
    );

    let change = document
        .insert_item(ROOT, choose, Side::Input, 2, Item::plain())
        .unwrap();

    assert!(change.is_lossless());
    assert_eq!(
        port_names(&document, choose, Side::Input),
        ["Option 0", "Option 1", "Option 2", "Index"]
    );
    assert!(
        change
            .moved
            .contains(&(PortRef::input(2), PortRef::input(3))),
        "★ the fixed port PAST the run moved: {:?}",
        change.moved
    );
    assert_eq!(
        document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(choose, 3))
            .map(|link| link.from.node),
        Some(picker),
        "and the wire that was on it came along"
    );
    assert_eq!(
        arrives(&document, Socket::new(choose, 0)),
        Some(Val::Number(10)),
        "while the option that was already wired is where it was"
    );
}

/// The engine's `RemoveOptionPin`, whose own description is "removes the
/// **last** option input pin from the node" — so it is `remove_item` at one
/// below the count, and the *last* is what has to go.
///
/// What it discriminates: a fixture wired on every option would pass an
/// implementation that removed the first, so this asserts which wire was cut
/// and which survived.
#[test]
fn engine_graph_editor_remove_option_pin() {
    let mut document: Document<Op> = Document::new("root");
    let choose = node(&mut document, Op::Choose);
    document
        .insert_item(ROOT, choose, Side::Input, 2, Item::plain())
        .unwrap();
    let feeds: Vec<NodeId> = (0..3).map(|n| num(&mut document, 10 + n)).collect();
    for (option, feed) in feeds.iter().enumerate() {
        wire(
            &mut document,
            *feed,
            0,
            choose,
            u32::try_from(option).unwrap(),
        );
    }

    let count = u32::try_from(document.items(ROOT, choose, Side::Input).unwrap().len()).unwrap();
    let change = document
        .remove_item(ROOT, choose, Side::Input, count - 1)
        .unwrap();

    assert_eq!(change.items, 2);
    assert_eq!(change.severed.len(), 1);
    assert_eq!(
        change.severed[0].from.node, feeds[2],
        "the LAST option's wire is the one cut"
    );
    assert_eq!(
        arrives(&document, Socket::new(choose, 0)),
        Some(Val::Number(10))
    );
    assert_eq!(
        arrives(&document, Socket::new(choose, 1)),
        Some(Val::Number(11)),
        "and the two before it are untouched at their own indices"
    );
}

/// The sound-cue editor's `DeleteInput` — removing an item from the middle of
/// an INPUT run, and handing back the value authored on it.
///
/// Distinct from the execution-pin removal in the fact it asserts: an authored
/// value is a thing only a *value* port has, so this is the arm that shows the
/// two halves of `remap_ports` move together. A link kept while the value on
/// the same port vanished is the corruption the shared map exists to rule out.
#[test]
fn engine_sound_cue_graph_delete_input() {
    let mut document: Document<Op> = Document::new("root");
    let choose = node(&mut document, Op::Choose);
    document
        .insert_item(ROOT, choose, Side::Input, 2, Item::plain())
        .unwrap();
    for option in 0..3u32 {
        document
            .set_port_value(
                ROOT,
                choose,
                PortRef::input(option),
                Val::Number(i64::from(70 + option)),
            )
            .unwrap();
    }
    document
        .set_port_value(ROOT, choose, PortRef::input(3), Val::Number(2))
        .unwrap();
    assert_eq!(
        document.evaluate(ROOT, choose),
        vec![Some(Val::Number(72))],
        "index 2 selects the third option"
    );

    let change = document.remove_item(ROOT, choose, Side::Input, 0).unwrap();

    assert_eq!(
        change.discarded,
        vec![(PortRef::input(0), Val::Number(70))],
        "the value authored on the removed port is HANDED BACK"
    );
    assert_eq!(
        document
            .tree(ROOT)
            .unwrap()
            .node(choose)
            .unwrap()
            .port_value(PortRef::input(2)),
        Some(&Val::Number(2)),
        "and the Index's own authored value followed the Index"
    );
    assert_eq!(
        document.evaluate(ROOT, choose),
        vec![None],
        "which now points past the two options that are left, rather than at a \
         value the author never put there"
    );
}

/// The animation editor's `AddBlendListPin` — one item, **two** ports.
///
/// Its blend-list runtime node's `AddPose` appends to two parallel arrays
/// (`BlendTime` and `BlendPose`), so an item there is a pair. This is the case
/// that tells a correct re-index from one that shifts by a single port: with
/// one port per item the two arithmetics agree and nothing can distinguish
/// them.
#[test]
fn engine_anim_graph_add_blend_list_pin() {
    let mut document: Document<Op> = Document::new("root");
    let blend = node(&mut document, Op::Blend);
    let bias = num(&mut document, 5);
    wire(&mut document, bias, 0, blend, 3);
    assert_eq!(
        port_names(&document, blend, Side::Input),
        ["Base", "Pose 0", "Weight 0", "Bias"]
    );

    let change = document
        .insert_item(ROOT, blend, Side::Input, 1, Item::plain())
        .unwrap();

    assert_eq!(
        change.added,
        vec![PortRef::input(3), PortRef::input(4)],
        "★ ONE item, TWO ports"
    );
    assert_eq!(
        port_names(&document, blend, Side::Input),
        ["Base", "Pose 0", "Weight 0", "Pose 1", "Weight 1", "Bias"]
    );
    assert!(
        change
            .moved
            .contains(&(PortRef::input(3), PortRef::input(5))),
        "★ and `Bias` moved by TWO, not by one: {:?}",
        change.moved
    );
    assert_eq!(
        document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(blend, 5))
            .map(|link| link.from.node),
        Some(bias)
    );
}

/// The animation editor's `RemoveBlendListPin` — the case its own source says
/// it does not handle.
///
/// Its blend-list editor node's `RemovePinFromBlendList` at 5.8.1 carries
/// `//@TODO: ANIMREFACTOR: Need to handle moving pins below up correctly`. This
/// is that: a middle item removed out of three, with every remaining port
/// wired, so "the pins below moved up correctly" is asserted per port rather
/// than as a count.
#[test]
fn engine_anim_graph_remove_blend_list_pin() {
    let mut document: Document<Op> = Document::new("root");
    let blend = node(&mut document, Op::Blend);
    for extra in 1..3 {
        document
            .insert_item(ROOT, blend, Side::Input, extra, Item::plain())
            .unwrap();
    }
    let arity = document.signature(ROOT, blend).unwrap().inputs.len();
    assert_eq!(arity, 8);
    let feeds: Vec<NodeId> = (0..arity)
        .map(|port| {
            let feed = num(&mut document, i64::try_from(port).unwrap());
            wire(&mut document, feed, 0, blend, u32::try_from(port).unwrap());
            feed
        })
        .collect();

    let change = document.remove_item(ROOT, blend, Side::Input, 1).unwrap();

    assert_eq!(change.severed.len(), 2, "the pair went together");
    let reached: Vec<Option<NodeId>> = (0..6)
        .map(|port| {
            document
                .tree(ROOT)
                .unwrap()
                .link_into(Socket::new(blend, port))
                .map(|link| link.from.node)
        })
        .collect();
    assert_eq!(
        reached,
        vec![
            Some(feeds[0]), // Base
            Some(feeds[1]), // Pose 0
            Some(feeds[2]), // Weight 0
            Some(feeds[5]), // Pose 2 came up two
            Some(feeds[6]), // Weight 2 with it
            Some(feeds[7]), // ★ Bias, which the reference's TODO is about
        ],
        "every pin below the removed item moved up by a whole item"
    );
}

/// The engine's `CreateUniquePinName`, and the ceiling it implies.
///
/// Its add-pin interface's `GetMaxInputPinsNum` is `'Z' - 'A'` — "the node is
/// limited by the number of letters in the alphabet for display purposes" — and
/// its execution-sequence node re-runs a renaming loop after every insert and
/// every remove to keep `Then_0`… compact. Here the ordinal is applied when the
/// signature is resolved, so the names are compact by construction and 26 is
/// not a number this crate knows.
#[test]
fn engine_node_create_unique_pin_name() {
    let (mut document, sequence, _) = sequenced(2);
    for _ in 0..30 {
        let count =
            u32::try_from(document.items(ROOT, sequence, Side::Output).unwrap().len()).unwrap();
        document
            .insert_item(ROOT, sequence, Side::Output, count, Item::plain())
            .unwrap();
    }
    let names = port_names(&document, sequence, Side::Output);
    assert_eq!(
        names.len(),
        32,
        "★ past the alphabet, with no ceiling to hit"
    );
    assert_eq!(names.last().map(String::as_str), Some("Then 31"));
    assert_eq!(
        names.iter().collect::<BTreeSet<_>>().len(),
        names.len(),
        "every name is unique, which is what the reference's loop is for"
    );

    document
        .remove_item(ROOT, sequence, Side::Output, 0)
        .unwrap();
    let after = port_names(&document, sequence, Side::Output);
    assert_eq!(
        after,
        (0..31).map(|n| format!("Then {n}")).collect::<Vec<_>>(),
        "and after a removal they are still compact, with no renaming pass — \
         they were never stored"
    );
}

/// The DCC's `socket_items::make_add_item_operator`: an item carries its own
/// **name** and its own **socket type**.
///
/// Measured at `8cf50599`: every socket-item accessor but the index switch's
/// declares `has_type` or `has_name`, so an authored item is that reference's
/// normal case. The type is not decoration — a wire refused against the
/// template's type is accepted against the item's, which is what shows
/// `connect` consults the resolved signature and not the kind.
#[test]
fn dcc_socket_items_make_add_item_operator() {
    let mut document: Document<Op> = Document::new("root");
    let bundle = node(&mut document, Op::Bundle);
    let word = node(&mut document, Op::Word("hi".into()));
    assert!(
        document
            .connect(ROOT, Socket::new(word, 0), Socket::new(bundle, 0))
            .is_err(),
        "the template's member is a Number and Text does not cross into one"
    );

    document
        .insert_item(
            ROOT,
            bundle,
            Side::Input,
            1,
            Item::plain().named("Caption").typed(0, Ty::Text),
        )
        .unwrap();

    assert_eq!(
        port_names(&document, bundle, Side::Input),
        ["Member 0", "Caption"],
        "the authored name IS the socket's name; the unnamed one keeps its ordinal"
    );
    document
        .connect(ROOT, Socket::new(word, 0), Socket::new(bundle, 1))
        .unwrap();
    assert_eq!(
        document.evaluate(ROOT, bundle),
        vec![Some(Val::Text("-/hi".into()))],
        "and the value that the authored type let through arrives"
    );
    assert!(document.validate().is_empty());
}

/// The DCC's `socket_items::make_remove_item_by_index_operator`, and what makes
/// it more than the port re-index: **the remaining items keep their own names
/// and types** at their new positions.
///
/// An implementation that re-indexed links and values but rebuilt the item list
/// from the template would pass every wire assertion and quietly rename the
/// author's sockets. Its `make_remove_active_item_operator` is the same call
/// with the index the editor is holding, which is why that row cites this
/// proof rather than owning one.
#[test]
fn dcc_socket_items_make_remove_item_by_index_operator() {
    let mut document: Document<Op> = Document::new("root");
    let bundle = node(&mut document, Op::Bundle);
    for (at, name) in [(1, "Body"), (2, "Tail")] {
        document
            .insert_item(
                ROOT,
                bundle,
                Side::Input,
                at,
                Item::plain().named(name).typed(0, Ty::Text),
            )
            .unwrap();
    }
    let word = node(&mut document, Op::Word("keep".into()));
    wire(&mut document, word, 0, bundle, 2);
    assert_eq!(
        port_names(&document, bundle, Side::Input),
        ["Member 0", "Body", "Tail"]
    );

    let change = document.remove_item(ROOT, bundle, Side::Input, 0).unwrap();

    assert!(change.is_lossless(), "nothing was wired to Member 0");
    assert_eq!(
        port_names(&document, bundle, Side::Input),
        ["Body", "Tail"],
        "★ the surviving items kept their AUTHORED names at their new indices"
    );
    assert_eq!(
        document.signature(ROOT, bundle).unwrap().inputs[1].value_type(),
        Some(&Ty::Text),
        "and their authored types"
    );
    assert_eq!(
        document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(bundle, 1))
            .map(|link| link.from.node),
        Some(word),
        "with the wire that was on Tail"
    );
}

/// The DCC's `socket_items::make_move_item_operator` — reordering a per-node
/// socket list, which the engine has **no** command for at all.
///
/// A permutation is the sharpest test the correspondence has: every address
/// must arrive somewhere, so `severed` and `discarded` both being empty is a
/// claim about the arithmetic rather than about the fixture.
#[test]
fn dcc_socket_items_make_move_item_operator() {
    let mut document: Document<Op> = Document::new("root");
    let bundle = node(&mut document, Op::Bundle);
    for (at, name) in [(1, "Body"), (2, "Tail")] {
        document
            .insert_item(ROOT, bundle, Side::Input, at, Item::plain().named(name))
            .unwrap();
    }
    let feeds: Vec<NodeId> = (0..3).map(|n| num(&mut document, 40 + n)).collect();
    for (port, feed) in feeds.iter().enumerate() {
        wire(
            &mut document,
            *feed,
            0,
            bundle,
            u32::try_from(port).unwrap(),
        );
    }
    let before = document.evaluate(ROOT, bundle);
    assert_eq!(before, vec![Some(Val::Text("40/41/42".into()))]);

    let change = document.move_item(ROOT, bundle, Side::Input, 2, 0).unwrap();

    assert!(
        change.is_lossless(),
        "a permutation loses nothing: {change:?}"
    );
    assert_eq!(
        port_names(&document, bundle, Side::Input),
        ["Tail", "Member 1", "Body"],
        "the named items travelled and the unnamed one took the ordinal it now has"
    );
    assert_eq!(
        document.evaluate(ROOT, bundle),
        vec![Some(Val::Text("42/40/41".into()))],
        "and every wire arrived at the port its item went to"
    );
}

// ================================================= stopping, stepping, watching

/// R1644 — the DEBUGGING cluster: the engine's twenty-three commands for
/// stopping a graph, stepping it and reading what it holds.
///
/// Its own registry for the reason the arrangement and variadic ones have
/// theirs: the others are at the line ceiling, and these are one capability.
/// Split in two for that same ceiling — stopping, then stepping and watching.
fn engine_debug_proofs() -> Vec<Proof> {
    let mut all = engine_breakpoint_proofs();
    all.extend(engine_watch_and_stride_proofs());
    all
}

/// The breakpoint half: where a run stops, and which of those places are live.
fn engine_breakpoint_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "GraphEditor::AddBreakpoint",
            engine_graph_editor_add_breakpoint,
        ),
        proof(
            "engine",
            "GraphEditor::RemoveBreakpoint",
            engine_graph_editor_remove_breakpoint,
        ),
        proof(
            "engine",
            "GraphEditor::EnableBreakpoint",
            engine_graph_editor_enable_breakpoint,
        ),
        proof(
            "engine",
            "GraphEditor::DisableBreakpoint",
            engine_graph_editor_disable_breakpoint,
        ),
        proof(
            "engine",
            "GraphEditor::ToggleBreakpoint",
            engine_graph_editor_toggle_breakpoint,
        ),
        proof(
            "engine",
            "script_editor::ClearAllBreakpoints",
            engine_script_editor_clear_all_breakpoints,
        ),
        proof(
            "engine",
            "script_editor::EnableAllBreakpoints",
            engine_script_editor_enable_all_breakpoints,
        ),
        proof(
            "engine",
            "script_editor::DisableAllBreakpoints",
            engine_script_editor_disable_all_breakpoints,
        ),
    ]
}

/// The watch and stride half: what a stopped run holds, and how it moves.
fn engine_watch_and_stride_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "GraphEditor::StartWatchingPin",
            engine_graph_editor_start_watching_pin,
        ),
        proof(
            "engine",
            "GraphEditor::StopWatchingPin",
            engine_graph_editor_stop_watching_pin,
        ),
        proof(
            "engine",
            "schema::IsPinBeingWatched",
            engine_schema_is_pin_being_watched,
        ),
        proof(
            "engine",
            "schema::DoesSupportPinWatching",
            engine_schema_does_support_pin_watching,
        ),
        proof(
            "engine",
            "script_editor::ClearAllWatches",
            engine_script_editor_clear_all_watches,
        ),
        proof(
            "engine",
            "AnimGraph::TogglePoseWatch",
            engine_anim_graph_toggle_pose_watch,
        ),
        proof(
            "engine",
            "BehaviorTreeDebugger::ForwardInto",
            engine_behavior_tree_debugger_forward_into,
        ),
        proof(
            "engine",
            "BehaviorTreeDebugger::ForwardOver",
            engine_behavior_tree_debugger_forward_over,
        ),
        proof(
            "engine",
            "BehaviorTreeDebugger::StepOut",
            engine_behavior_tree_debugger_step_out,
        ),
        proof(
            "engine",
            "BehaviorTreeDebugger::BackInto",
            engine_behavior_tree_debugger_back_into,
        ),
        proof(
            "engine",
            "BehaviorTreeDebugger::BackOver",
            engine_behavior_tree_debugger_back_over,
        ),
        proof(
            "engine",
            "BehaviorTreeDebugger::CurrentValues",
            engine_behavior_tree_debugger_current_values,
        ),
        proof(
            "engine",
            "BehaviorTreeDebugger::SavedValues",
            engine_behavior_tree_debugger_saved_values,
        ),
        proof(
            "engine",
            "script_editor::OpenBlueprintDebugger",
            engine_script_editor_open_blueprint_debugger,
        ),
    ]
}

/// What a debug session is built on: `head -> [Stage] -> tail`, with the middle
/// stage collapsed so the run crosses a boundary.
///
/// Its trace is five steps at depths `0,1,1,1,0` — the entry, the definition's
/// inside-input node, the stage, the inside-output node, and the tail. A run
/// that never left the root would let `into`, `over` and `out` be one function
/// with no proof noticing.
struct Debugging {
    document: Document<Op>,
    session: Session,
    head: NodeId,
    tail: NodeId,
    inside: NodeId,
    definition: TreeId,
    instance: NodeId,
}

fn debugging() -> Debugging {
    let mut document: Document<Op> = Document::new("root");
    let head = node(&mut document, Op::Stage(1));
    let mid = node(&mut document, Op::Stage(2));
    let tail = node(&mut document, Op::Stage(3));
    wire_control(&mut document, head, mid);
    wire_control(&mut document, mid, tail);
    let grouped = document.group(ROOT, &[mid], "Stage").expect("it collapses");
    let inside = document
        .tree(grouped.definition)
        .and_then(|held| held.nodes().find(|n| matches!(n.body, NodeBody::Kind(_))))
        .map(|n| n.id)
        .expect("the stage moved in");
    let session = Session::new(ROOT, head, 32);
    Debugging {
        document,
        session,
        head,
        tail,
        inside,
        definition: grouped.definition,
        instance: grouped.node,
    }
}

fn wire_control(document: &mut Document<Op>, from: NodeId, to: NodeId) {
    document
        .connect(ROOT, Socket::new(from, 0), Socket::new(to, 0))
        .expect("control wires");
}

/// How far a session gets on one resume, and why it stopped.
fn resume(document: &Document<Op>, session: &mut Session) -> (usize, &'static str) {
    let paused = document
        .debug(session, &Machine::new(), Command::Resume)
        .expect("the run starts");
    (paused.at(), paused.halt().name())
}

/// Put a session at step `at`, through the commands a client has: back to the
/// entry, then one step at a time. There is deliberately no setter for the
/// position — a debugger moves by stepping, and a proof that reached its
/// fixture state some other way would not be exercising the surface.
fn at_step(document: &Document<Op>, session: &mut Session, at: usize) {
    document
        .debug(session, &Machine::new(), Command::Restart)
        .expect("the run is there");
    for _ in 0..at {
        document
            .debug(
                session,
                &Machine::new(),
                Command::Step {
                    direction: Direction::Forward,
                    stride: Stride::Into,
                },
            )
            .expect("the run is there");
    }
    assert_eq!(session.at(), at, "the fixture is where it says it is");
}

#[test]
fn engine_graph_editor_add_breakpoint() {
    let Debugging {
        document,
        mut session,
        tail,
        ..
    } = debugging();
    assert_eq!(resume(&document, &mut session), (5, "halted"));

    at_step(&document, &mut session, 0);
    assert!(
        document
            .set_breakpoint(session.breakpoints_mut(), NodeSite::any(ROOT, tail))
            .unwrap()
    );
    let paused = document
        .debug(&mut session, &Machine::new(), Command::Resume)
        .unwrap();
    assert_eq!(paused.at(), 4, "it stopped where nothing stopped it before");
    assert_eq!(paused.next().map(|step| step.node), Some(tail));
    assert!(
        paused
            .taken()
            .iter()
            .all(|step| !(step.instance.is_root() && step.node == tail)),
        "and BEFORE the marked node ran, which is where the reference stops too"
    );
    // ★ The occurrence is in that comparison because a `NodeId` is unique only
    // within its tree: the definition's own nodes carry ids that collide with
    // the root's, and a check by node alone reports the tail as having run
    // inside the instance. Written the naive way first, and this fixture caught
    // it.
    assert!(
        paused.taken().iter().any(|step| step.node == tail),
        "★ flattened, the ids collide across trees"
    );
}

#[test]
fn engine_graph_editor_remove_breakpoint() {
    let Debugging {
        document,
        mut session,
        tail,
        ..
    } = debugging();
    let site = NodeSite::any(ROOT, tail);
    document
        .set_breakpoint(session.breakpoints_mut(), site.clone())
        .unwrap();
    assert_eq!(resume(&document, &mut session), (4, "breakpoint"));

    at_step(&document, &mut session, 0);
    assert!(session.breakpoints_mut().disarm(&site));
    assert!(!session.breakpoints_mut().disarm(&site), "and once only");
    assert_eq!(
        resume(&document, &mut session),
        (5, "halted"),
        "with the mark gone the run is not stopped"
    );
}

#[test]
fn engine_graph_editor_enable_breakpoint() {
    let Debugging {
        document,
        mut session,
        tail,
        ..
    } = debugging();
    let site = NodeSite::any(ROOT, tail);
    document
        .set_breakpoint(session.breakpoints_mut(), site.clone())
        .unwrap();
    assert_eq!(
        session.breakpoints_mut().set_enabled(&site, false),
        Some(true)
    );
    assert_eq!(resume(&document, &mut session), (5, "halted"));

    at_step(&document, &mut session, 0);
    assert_eq!(
        session.breakpoints_mut().set_enabled(&site, true),
        Some(false),
        "and it answers the flag it replaced"
    );
    assert_eq!(resume(&document, &mut session), (4, "breakpoint"));
}

#[test]
fn engine_graph_editor_disable_breakpoint() {
    let Debugging {
        document,
        mut session,
        tail,
        ..
    } = debugging();
    let site = NodeSite::any(ROOT, tail);
    document
        .set_breakpoint(session.breakpoints_mut(), site.clone())
        .unwrap();
    session.breakpoints_mut().set_enabled(&site, false);

    // Disabled is NOT removed: the place is remembered, which is why the
    // reference has five commands here rather than three.
    assert!(session.breakpoints().contains(&site));
    assert!(!session.breakpoints().is_enabled(&site));
    assert_eq!(session.breakpoints().len(), 1);
    assert_eq!(resume(&document, &mut session), (5, "halted"));
}

#[test]
fn engine_graph_editor_toggle_breakpoint() {
    let Debugging {
        document,
        mut session,
        tail,
        ..
    } = debugging();
    let site = NodeSite::any(ROOT, tail);
    assert!(
        document
            .toggle_breakpoint(session.breakpoints_mut(), site.clone())
            .unwrap()
    );
    assert!(session.breakpoints().is_enabled(&site));
    session.breakpoints_mut().set_enabled(&site, false);

    // A disabled one toggles AWAY, not back to enabled: toggling is about
    // presence, and the reference draws the same line — its toggle creates or
    // removes, and `bEnabled` moves only under its own two commands.
    assert!(
        !document
            .toggle_breakpoint(session.breakpoints_mut(), site.clone())
            .unwrap()
    );
    assert!(session.breakpoints().is_empty());
    assert_eq!(resume(&document, &mut session), (5, "halted"));
}

#[test]
fn engine_script_editor_clear_all_breakpoints() {
    let Debugging {
        document,
        mut session,
        head,
        tail,
        inside,
        definition,
        ..
    } = debugging();
    for site in [
        NodeSite::any(ROOT, head),
        NodeSite::any(ROOT, tail),
        NodeSite::any(definition, inside),
    ] {
        document
            .set_breakpoint(session.breakpoints_mut(), site)
            .unwrap();
    }
    assert_eq!(session.breakpoints().len(), 3);
    assert_eq!(session.breakpoints_mut().clear(), 3, "and it says how many");
    assert_eq!(session.breakpoints_mut().clear(), 0);
    assert_eq!(resume(&document, &mut session), (5, "halted"));
}

#[test]
fn engine_script_editor_enable_all_breakpoints() {
    let Debugging {
        document,
        mut session,
        head,
        tail,
        ..
    } = debugging();
    for site in [NodeSite::any(ROOT, head), NodeSite::any(ROOT, tail)] {
        document
            .set_breakpoint(session.breakpoints_mut(), site)
            .unwrap();
    }
    assert_eq!(session.breakpoints_mut().disable_all(), 2);
    assert_eq!(resume(&document, &mut session), (5, "halted"));

    at_step(&document, &mut session, 0);
    assert_eq!(session.breakpoints_mut().enable_all(), 2);
    assert_eq!(
        session.breakpoints_mut().enable_all(),
        0,
        "and it counts what CHANGED, not what is armed"
    );
    // Back at the entry, whose own mark is live again — so the session is
    // already stopped at one, and resuming carries on to the next.
    assert_eq!(
        document
            .paused(&session, &Machine::new())
            .unwrap()
            .halt()
            .name(),
        "breakpoint"
    );
    assert_eq!(resume(&document, &mut session), (4, "breakpoint"));
}

#[test]
fn engine_script_editor_disable_all_breakpoints() {
    let Debugging {
        document,
        mut session,
        head,
        tail,
        ..
    } = debugging();
    for site in [NodeSite::any(ROOT, head), NodeSite::any(ROOT, tail)] {
        document
            .set_breakpoint(session.breakpoints_mut(), site)
            .unwrap();
    }
    assert_eq!(session.breakpoints_mut().disable_all(), 2);
    assert_eq!(
        session.breakpoints().len(),
        2,
        "every place is remembered, and none of them stops anything"
    );
    assert!(session.breakpoints().iter().all(|(_, live)| !live));
    assert_eq!(resume(&document, &mut session), (5, "halted"));
}

#[test]
fn engine_graph_editor_start_watching_pin() {
    let Debugging {
        document,
        mut session,
        inside,
        definition,
        instance,
        ..
    } = debugging();
    let cost = PortSite::any(definition, inside, PortRef::output(1));
    assert!(
        document
            .set_watch(session.watches_mut(), cost.clone())
            .unwrap()
    );
    assert!(
        !document
            .set_watch(session.watches_mut(), cost.clone())
            .unwrap(),
        "watching twice is watching once"
    );

    let paused = document.paused(&session, &Machine::new()).unwrap();
    assert_eq!(paused.readings().len(), 1);
    let reading = &paused.readings()[0];
    assert_eq!(reading.value, Some(Val::Number(2)), "the stage's own cost");
    assert_eq!(reading.instance, Instance::root().inside(ROOT, instance));
    assert_eq!(reading.ran_at, Some(2), "and it ran, at step 2");
}

#[test]
fn engine_graph_editor_stop_watching_pin() {
    let Debugging {
        document,
        mut session,
        inside,
        definition,
        ..
    } = debugging();
    let cost = PortSite::any(definition, inside, PortRef::output(1));
    document
        .set_watch(session.watches_mut(), cost.clone())
        .unwrap();
    assert_eq!(
        document
            .paused(&session, &Machine::new())
            .unwrap()
            .readings()
            .len(),
        1
    );

    assert!(session.watches_mut().unwatch(&cost));
    assert!(!session.watches_mut().unwatch(&cost), "and once only");
    assert!(
        document
            .paused(&session, &Machine::new())
            .unwrap()
            .readings()
            .is_empty(),
        "nothing is reported for a port nobody is watching"
    );
}

#[test]
fn engine_schema_is_pin_being_watched() {
    let Debugging {
        document,
        mut session,
        inside,
        definition,
        instance,
        ..
    } = debugging();
    let every = PortSite::any(definition, inside, PortRef::output(1));
    let only = PortSite::at(
        definition,
        inside,
        PortRef::output(1),
        Instance::root().inside(ROOT, instance),
    );
    assert!(!session.watches().contains(&every));

    document
        .set_watch(session.watches_mut(), every.clone())
        .unwrap();
    assert!(session.watches().contains(&every));
    // ★ The occurrence is part of the address: watching every occurrence is not
    // watching one of them, and asking about the wrong one answers no. The
    // reference has no such axis — a macro there is expanded per use before
    // anything runs, so its watched pin is one copy's pin.
    assert!(!session.watches().contains(&only));
    document
        .set_watch(session.watches_mut(), only.clone())
        .unwrap();
    assert_eq!(session.watches().len(), 2);
    assert!(session.watches().contains(&only));
}

#[test]
fn engine_schema_does_support_pin_watching() {
    let Debugging {
        document,
        mut session,
        inside,
        definition,
        ..
    } = debugging();
    // Output 0 of a stage is its control output. Control is not a value, so
    // there is nothing to report and the watch is refused rather than armed to
    // report nothing. The reference refuses the same thing, by asking its
    // schema whether the pin's category is an execution one.
    assert_eq!(
        document.set_watch(
            session.watches_mut(),
            PortSite::any(definition, inside, PortRef::output(0)),
        ),
        Err(WatchError::NotAValue {
            tree: definition,
            node: inside,
            port: PortRef::output(0),
        })
    );
    assert!(session.watches().is_empty());
    assert!(
        document
            .set_watch(
                session.watches_mut(),
                PortSite::any(definition, inside, PortRef::output(1)),
            )
            .is_ok(),
        "and the value port beside it is admitted, so the check is not \
         refusing everything"
    );
}

#[test]
fn engine_script_editor_clear_all_watches() {
    let Debugging {
        document,
        mut session,
        inside,
        definition,
        instance,
        ..
    } = debugging();
    for site in [
        PortSite::any(definition, inside, PortRef::output(1)),
        PortSite::at(
            definition,
            inside,
            PortRef::output(1),
            Instance::root().inside(ROOT, instance),
        ),
    ] {
        document.set_watch(session.watches_mut(), site).unwrap();
    }
    assert_eq!(
        document
            .paused(&session, &Machine::new())
            .unwrap()
            .readings()
            .len(),
        2
    );
    assert_eq!(session.watches_mut().clear(), 2, "and it says how many");
    assert_eq!(session.watches_mut().clear(), 0);
    assert!(
        document
            .paused(&session, &Machine::new())
            .unwrap()
            .readings()
            .is_empty()
    );
}

#[test]
fn engine_anim_graph_toggle_pose_watch() {
    let Debugging {
        document,
        mut session,
        inside,
        definition,
        ..
    } = debugging();
    let cost = PortSite::any(definition, inside, PortRef::output(1));
    assert!(
        document
            .toggle_watch(session.watches_mut(), cost.clone())
            .unwrap()
    );
    assert_eq!(
        document
            .paused(&session, &Machine::new())
            .unwrap()
            .readings()
            .len(),
        1
    );
    assert!(
        !document
            .toggle_watch(session.watches_mut(), cost.clone())
            .unwrap()
    );
    assert!(
        document
            .paused(&session, &Machine::new())
            .unwrap()
            .readings()
            .is_empty(),
        "one verb, both ways"
    );
}

/// Where a stride lands, from `at`, on the fixture's `0,1,1,1,0` run.
fn stride_to(
    document: &Document<Op>,
    session: &Session,
    at: usize,
    direction: Direction,
    stride: Stride,
) -> usize {
    let mut moved = session.clone();
    at_step(document, &mut moved, at);
    document
        .debug(
            &mut moved,
            &Machine::new(),
            Command::Step { direction, stride },
        )
        .expect("the run is there")
        .at()
}

#[test]
fn engine_behavior_tree_debugger_forward_into() {
    let Debugging {
        document, session, ..
    } = debugging();
    for at in 0..5 {
        assert_eq!(
            stride_to(&document, &session, at, Direction::Forward, Stride::Into),
            at + 1,
            "one step is one step, boundary or not"
        );
    }
    // Including INTO the instance: step 0 is at the root and step 1 is inside.
    let timeline = document.timeline(&session, &Machine::new()).unwrap();
    assert_eq!(timeline.depth(0), Some(0));
    assert_eq!(timeline.depth(1), Some(1));
}

#[test]
fn engine_behavior_tree_debugger_forward_over() {
    let Debugging {
        document, session, ..
    } = debugging();
    // From the root, the whole instance is skipped: three steps inside pass in
    // one command, and the next thing at depth 0 is step 4.
    assert_eq!(
        stride_to(&document, &session, 0, Direction::Forward, Stride::Over),
        4
    );
    assert_ne!(
        stride_to(&document, &session, 0, Direction::Forward, Stride::Over),
        stride_to(&document, &session, 0, Direction::Forward, Stride::Into),
        "which is what makes it a different command from `into`"
    );
    // Inside the frame there is nothing deeper to skip, so it is one step.
    assert_eq!(
        stride_to(&document, &session, 1, Direction::Forward, Stride::Over),
        2
    );
}

#[test]
fn engine_behavior_tree_debugger_step_out() {
    let Debugging {
        document, session, ..
    } = debugging();
    // From inside the instance, control leaves the frame: from step 1 or step
    // 2, out lands at 4, which is the first step back at depth 0.
    for at in 1..=2 {
        assert_eq!(
            stride_to(&document, &session, at, Direction::Forward, Stride::Out),
            4
        );
    }
    // And out of the OUTERMOST frame runs to the end, because there is nothing
    // shallower to arrive at — which is what every debugger does.
    assert_eq!(
        stride_to(&document, &session, 0, Direction::Forward, Stride::Out),
        5
    );
}

#[test]
fn engine_behavior_tree_debugger_back_into() {
    let Debugging {
        document, session, ..
    } = debugging();
    for at in 1..=5 {
        assert_eq!(
            stride_to(&document, &session, at, Direction::Back, Stride::Into),
            at - 1
        );
    }
    assert_eq!(
        stride_to(&document, &session, 0, Direction::Back, Stride::Into),
        0,
        "the entry is the floor"
    );
    // ★ Backwards is the SAME arithmetic as forwards, on the same object —
    // not a replay of recorded frames. The reference's backward commands read
    // its recorded execution history, which is why "current" and "saved" values
    // are two separate commands there.
    let there = stride_to(&document, &session, 2, Direction::Forward, Stride::Into);
    assert_eq!(
        stride_to(&document, &session, there, Direction::Back, Stride::Into),
        2
    );
}

#[test]
fn engine_behavior_tree_debugger_back_over() {
    let Debugging {
        document, session, ..
    } = debugging();
    // Back across the whole instance: from step 4 at the root, the previous
    // thing at depth 0 is step 0.
    assert_eq!(
        stride_to(&document, &session, 4, Direction::Back, Stride::Over),
        0
    );
    assert_ne!(
        stride_to(&document, &session, 4, Direction::Back, Stride::Over),
        stride_to(&document, &session, 4, Direction::Back, Stride::Into),
        "which is what makes it a different command from `back into`"
    );
    // And within the frame it is one step, for the same reason `over` is.
    assert_eq!(
        stride_to(&document, &session, 2, Direction::Back, Stride::Over),
        1
    );
}

#[test]
fn engine_behavior_tree_debugger_current_values() {
    let Debugging {
        document,
        mut session,
        inside,
        definition,
        ..
    } = debugging();
    document
        .set_watch(
            session.watches_mut(),
            PortSite::any(definition, inside, PortRef::output(1)),
        )
        .unwrap();
    let paused = document
        .debug(
            &mut session,
            &Machine::new(),
            Command::Step {
                direction: Direction::Forward,
                stride: Stride::Into,
            },
        )
        .unwrap();
    assert_eq!(paused.at(), 1);
    assert_eq!(
        paused.readings().first().and_then(|one| one.value.clone()),
        Some(Val::Number(2)),
        "the value at the place the run is stopped, from the same evaluator \
         the run itself reads through"
    );
}

#[test]
fn engine_behavior_tree_debugger_saved_values() {
    let Debugging {
        document,
        mut session,
        inside,
        definition,
        ..
    } = debugging();
    document
        .set_watch(
            session.watches_mut(),
            PortSite::any(definition, inside, PortRef::output(1)),
        )
        .unwrap();
    let state = Machine::new();

    // ★ "Live" and "recorded" are ONE object here, and this is the assertion
    // that says so: the readings are identical at two different positions while
    // the trace prefix is not. In the reference those are two commands over two
    // data sources — the running debug object, and a recorded frame history —
    // and they can disagree. Here a run is a pure function of the document and
    // the registers, so there is nothing to record.
    at_step(&document, &mut session, 1);
    let early = document.paused(&session, &state).unwrap();
    at_step(&document, &mut session, 4);
    let late = document.paused(&session, &state).unwrap();
    assert_eq!(early.readings(), late.readings());
    assert_eq!(early.taken().len(), 1);
    assert_eq!(late.taken().len(), 4, "and the trace DID grow between them");
    assert_eq!(
        late.taken()[..1],
        *early.taken(),
        "the earlier prefix is the later one's beginning"
    );
}

#[test]
fn engine_script_editor_open_blueprint_debugger() {
    let Debugging {
        document,
        mut session,
        inside,
        definition,
        instance,
        ..
    } = debugging();
    document
        .set_breakpoint(session.breakpoints_mut(), NodeSite::any(definition, inside))
        .unwrap();
    document
        .set_watch(
            session.watches_mut(),
            PortSite::any(definition, inside, PortRef::output(1)),
        )
        .unwrap();

    // The surface, in one read: where it is, why, what is about to run, the
    // call stack it is in, what it has run, and what the watched ports hold.
    // R1599 and R1600 gave `run` and `tick`, and nothing observed them from
    // outside.
    let paused = document
        .debug(&mut session, &Machine::new(), Command::Resume)
        .unwrap();
    assert_eq!(paused.halt().name(), "breakpoint");
    assert_eq!(paused.at(), 2);
    assert_eq!(paused.next().map(|step| step.node), Some(inside));
    assert_eq!(paused.stack(), [(ROOT, instance)], "one frame deep");
    assert_eq!(paused.taken().len(), 2);
    assert_eq!(paused.readings().len(), 1);
    assert!(document.stale_breakpoints(session.breakpoints()).is_empty());

    // And the session is a VALUE: the marks, the watches and the position
    // survive the wire together, so a debugging setup can be saved or handed
    // on. The reference keeps its breakpoints in the asset and its position
    // nowhere at all.
    let json = serde_json::to_string(&session).expect("a session serialises");
    let back: Session = serde_json::from_str(&json).expect("and comes back");
    assert_eq!(back, session);
    assert_eq!(back.at(), 2);
}
