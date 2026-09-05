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
    Act, Admits, Admitted, AdvancedView, Align, Appearance, Arrival, AutowireError, Axis, Berth,
    Carrying, ClassSource, Classified, Classify, ClassifyError, Command, ConnectError, Container,
    Conversion, Crossings, Definitions, Described, Direction, Distribute, Document, Drawn, Edge,
    EditError, EditPath, Extent, Faces, Focus, Fragment, Grow, Hidden, Inspectable, Instance,
    InterfacePort, InterfaceSide, Item, ItemError, LandError, Landfall, LinkId, Machine, Matched,
    Multiplicity, Node, NodeBody, NodeId, NodeKind, NodeSite, NotRecombinable, NotSplittable,
    Objection, Passing, Port, PortClass, PortName, PortPath, PortRef, PortSite, PortValueError,
    PutAway, ROOT, Reach, Relatedness, RelinkError, RetypeError, SectionId, Session, Sharing, Side,
    Socket, Stack, Straighten, Stride, SwapError, SwitchRefusal, Tie, Tint, TreeId, Variadic,
    Violation, WatchError, Watches, palette_of, type_palette,
};
use pinion_node_graph::{Alone, Archive, BeaconError, Represented, StandInError};
use pinion_node_graph::{Carried, ZoneSwapError};
use pinion_node_graph::{
    Copying, DefinitionAct, DefinitionError, InZone, InsertError, PairError, Renamed, Substitution,
    Tree, Unlandable, Used,
};
use pinion_node_graph::{Fit, Margin, Unframed, ZoomRange};
use pinion_node_graph::{
    NoHome, NotPrunable, ParentError, Reception, RelocateError, Relocation, Seed,
};
use pinion_node_graph::{RoomError, SpliceError, Verdict, Widening};

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
    /// R1934 — `(In: Number) -> Out: Number`, and it declares itself a **point
    /// on a wire** ([`NodeKind::passing`]).
    ///
    /// The same port shape as `Double` on purpose: the two differ in exactly
    /// the one declaration under test, so an assertion that passes for one and
    /// not the other cannot be reading anything else. It exists because the
    /// engine's third overrider of the equivalent hook is an application node
    /// class rather than one of its two reroute classes, so a fixture with only
    /// the crate's own [`NodeBody::Reroute`] could not reach that case at all.
    Relay,
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
    /// R1980 — `(Seat 0, Seat 1, ..) -> Out: Text`, and it declares
    /// [`Berth::Fresh`]: an arriving end gets a **seat of its own**.
    ///
    /// The same port shape as `Bundle` on purpose — the two differ in exactly
    /// the one declaration under test, which is `Relay`'s relationship to
    /// `Double` and for the same reason: an assertion that passes for one and
    /// not the other cannot be reading anything else about them.
    Roster,
    /// R2001 — `(Value: Number, Trim: Number [advanced]) -> Out: Number`. A
    /// kind that declares one of its ports into the **advanced** class and
    /// keeps that classification to itself
    /// ([`NodeKind::advanced_ports_are_authored`] unanswered, which is the
    /// reference's own default and what all but two of its node classes do).
    Tuned,
    /// R2001 — the same ports as `Tuned`, differing in exactly the declaration
    /// under test: here a PERSON may move a port between the classes. The
    /// `Relay`/`Double` pairing again, and for the third time its reason — an
    /// assertion that holds for one and not the other cannot be reading
    /// anything else about them.
    Rig,
    /// ★★★★★ R2003 — the SECOND zone opener: `(Execute ->|, Times: Number = 1)`,
    /// closed by `Gather`.
    ///
    /// A fixture with one zone kind can prove a zone exists and cannot prove a
    /// zone's kind can CHANGE, which is what this round's row is about. Two are
    /// the floor, and these two are shaped so the swap is observably lossy in a
    /// DIFFERENT way each way round — `Sequence`'s variadic run has nowhere to
    /// go here, and `Times` has nowhere to go there, carrying its authored
    /// value with it.
    Span,
    /// ★★★★★ R2003 — what closes a `Span`: `(Result: Number) -> Out: Number`.
    ///
    /// It PRODUCES, where `Sink` does not, so a zone's closer can have a
    /// downstream wire — which is what makes the plain-node arm of a zone swap
    /// observable at all: the node's outgoing wires have to land somewhere, and
    /// the reference's own answer is that they land on the closer.
    Gather,
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
    type Graph = ();

    fn name(&self) -> String {
        match self {
            Self::Num(_) => "Num",
            Self::Word(_) => "Word",
            Self::Add => "Add",
            Self::Mul => "Mul",
            Self::Double => "Double",
            Self::Relay => "Relay",
            Self::Shout => "Shout",
            Self::Sink => "Sink",
            Self::Sequence => "Sequence",
            Self::Choose => "Choose",
            Self::Blend => "Blend",
            Self::Bundle => "Bundle",
            Self::Stage(_) => "Stage",
            Self::Carry => "Carry",
            Self::Roster => "Roster",
            Self::Tuned => "Tuned",
            Self::Rig => "Rig",
            Self::Span => "Span",
            Self::Gather => "Gather",
        }
        .to_owned()
    }

    /// ★★★★★ R2001 — only `Rig` hands its port classes to a person.
    ///
    /// `Tuned` declares the same advanced port and answers the SUPPLIED `false`
    /// — R1926's rule, that a fixture overriding everywhere leaves the default
    /// with no check on it — and the reference's own proportion, where two node
    /// classes in the whole tree answer yes.
    fn advanced_ports_are_authored(&self) -> bool {
        matches!(self, Self::Rig)
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

    /// ★★★★★ R1928 — what a node of this taxonomy calls its own ports.
    ///
    /// Shaped after the reference's own six overriders, which between them do
    /// only two things and one of them far more often:
    ///
    /// * `Carry` **suppresses**, the way its two reroute classes do — measured,
    ///   four of the six use the capability to make a pin show no name at all.
    /// * `Stage` **renames its control output**, which is the other thing they
    ///   do (two of them hand back a text of the node's own).
    /// * ⚠ **Every other kind says nothing**, so the SUPPLIED answer is under
    ///   test too. R1926's lesson: a fixture that overrides everywhere leaves
    ///   the default with no check on it at all.
    fn port_name(&self, at: PortRef, declared: &str) -> PortName {
        match self {
            Self::Carry => PortName::Silent,
            Self::Stage(_) if at == PortRef::output(0) => {
                PortName::Instead(format!("after {declared}"))
            }
            _ => PortName::Declared,
        }
    }

    /// ★★★★★ R1927 — when a node of this taxonomy is in a questionable state.
    ///
    /// Shaped after the reference's own rule rather than invented: its one
    /// overrider that consults the graph answers from **whether a particular
    /// pin of its own is wired**, and reaches that fact by climbing out of
    /// itself because its signature hands it nothing. Here it is the argument.
    ///
    /// R1941 — and each objection carries its WEIGHT. All three arms are
    /// reachable, and they are on DIFFERENT kinds so a proof cannot pass by
    /// condemning one:
    ///
    /// * an unfed `Sink` **blocks**: a graph whose end consumes nothing cannot
    ///   sensibly be run, which is what a blocking objection is for.
    /// * a `Relay` with nothing on its input **warns**: it will run and pass
    ///   nothing along.
    /// * a `Double` with no consumer **notes** it: worth knowing, and nothing
    ///   more.
    ///
    /// Everything else says nothing, so a proof can tell a rule that fires
    /// from one that fires for everything.
    fn warning(&self, around: &pinion_node_graph::Surroundings) -> Option<Objection> {
        match self {
            Self::Sink if !around.is_wired(Side::Input, 0) => Some(Objection::Blocks(
                "nothing reaches this sink, so it consumes nothing".to_owned(),
            )),
            Self::Relay if !around.any_wired(Side::Input) => Some(Objection::Warns(
                "this relay passes nothing along".to_owned(),
            )),
            Self::Double if !around.any_wired(Side::Output) => Some(Objection::Notes(
                "nothing reads what this doubles".to_owned(),
            )),
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

    /// ★ R1934 — the resting colour of a port nothing has decided yet. The
    /// engine's graph-editor settings carry one of these too, a dark grey
    /// beside the per-category colours.
    fn undecided_colour() -> Option<Tint> {
        Some(Tint::rgb(0x38, 0x32, 0x32))
    }

    /// ★★★★★ R1934 — `Relay` declares itself a point on a wire and nothing else
    /// does. One kind and not all of them, so an assertion about a passing kind
    /// and one about an ordinary kind are both reachable from this taxonomy.
    fn passing(&self) -> Option<Passing> {
        match self {
            Self::Relay => Some(Passing::ENDS),
            _ => None,
        }
    }

    /// R1938 — this taxonomy has exactly ONE container: a `Bag` is an ARRAY of
    /// `Pair`, and nothing else is a container of anything.
    ///
    /// Deliberately narrow in all three directions so a proof can tell a
    /// declaration from a blanket yes: one element type, one shape, and no
    /// nesting (a `Bag` of `Bag`s does not exist here). ★ That narrowness is
    /// what the reference cannot express — its equivalent defaults to *every
    /// type in every shape*, and its one overrider answers the same, so nothing
    /// in that tree ever refuses through it.
    fn contained(ty: &Ty, held: Container) -> Option<Ty> {
        match (ty, held) {
            (Ty::Pair, Container::Array) => Some(Ty::Bag),
            _ => None,
        }
    }

    /// R1939 — what a port takes as its RESTING value, narrowed past the type.
    ///
    /// Two declarations and no more, one of each shape, so a proof can tell a
    /// declaration from a blanket yes:
    ///
    /// * `Add`'s **Addend** takes a whole number from 0 to 100 — a rule, whose
    ///   repair is a clamp. Its sibling `Augend` declares nothing, so the same
    ///   node has one narrowed port and one open one and an assertion cannot
    ///   pass by condemning the kind.
    /// * `Shout`'s **Phrase** takes one of two words — a closed set, whose
    ///   repair is the first of them.
    ///
    /// ★ Neither is expressible in the reference, whose equivalent is a string
    /// keyed by a string: there a range is two unparsed strings under two keys
    /// no compiler relates, and the closed set is a *function name* the editor
    /// looks up at draw time.
    fn takes(&self, at: PortRef, ty: &Ty) -> Admits<Val> {
        match (self, at.side, at.index, ty) {
            (Self::Add, Side::Input, 1, Ty::Number) => Admits::Shaped {
                wants: "a whole number from 0 to 100".to_owned(),
                nearest: |value| value.number().map(|n| Val::Number(n.clamp(0, 100))),
            },
            (Self::Shout, Side::Input, 0, Ty::Text) => Admits::OneOf(vec![
                Val::Text("hello".to_owned()),
                Val::Text("goodbye".to_owned()),
            ]),
            _ => Admits::Anything,
        }
    }

    /// R1937 — the two CONSTANTS let their one output's type be chosen, and
    /// nothing else here does.
    ///
    /// Deliberately narrow, in three directions at once, so a proof can tell
    /// the declaration from a blanket yes: only these two kinds answer, only
    /// their OUTPUT answers, and only the two atom types are accepted — a
    /// composite is refused, which is what makes "the kind may decline a
    /// particular type" observable rather than assumed.
    fn retyped(&self, port: PortRef, ty: &Ty) -> Option<Self> {
        if !matches!(self, Self::Num(_) | Self::Word(_))
            || port.side != Side::Output
            || port.index != 0
        {
            return None;
        }
        match ty {
            Ty::Number => Some(Self::Num(0)),
            Ty::Text => Some(Self::Word(String::new())),
            _ => None,
        }
    }

    /// R1943 — which kinds OPEN a bracketed region, and what closes each.
    ///
    /// `Sequence` opens and `Sink` closes it — a pair chosen because the two
    /// are ALREADY in this fixture for other proofs, so the zone axis is
    /// carried by nodes whose other behaviour is independently pinned. Nothing
    /// else opens anything, so a proof can tell a declaration from a blanket
    /// yes, and `Sink` closing does not make `Sink` an opener: the closer is
    /// found through the opener's declaration, never by asking the closer.
    fn closed_by(&self) -> Option<Self> {
        match self {
            Self::Sequence => Some(Self::Sink),
            // ★★★★★ R2003 — the second pair, so a zone has another kind to
            // BECOME. Two openers out of nineteen kinds, which still lets a
            // proof tell a declaration from a blanket yes.
            Self::Span => Some(Self::Gather),
            _ => None,
        }
    }

    /// R1942 — which of this taxonomy's types have a value a person can LOOK
    /// AT while the graph runs.
    ///
    /// `Bag` cannot, and it is the only one: a container is a handle to however
    /// many things are in it, and showing "the value" of one is showing
    /// something the model does not have. That is the shape the reference's two
    /// overriders refuse — a pin that carries a live binding, and one that
    /// carries an evaluation-time pose — neither of which is an execution pin,
    /// which this crate already refuses on its own arm.
    ///
    /// ★ Exactly one type refuses, so a proof can tell a declaration from a
    /// blanket no, and the four that permit include the composite (`Pair`) —
    /// so "container" and "composite" are told apart here rather than collapsed
    /// into one refusal.
    fn inspectable(ty: &Ty) -> Inspectable {
        match ty {
            Ty::Bag => Inspectable::No(
                "a bag of pairs, which is a handle to however many things are \
                 in it rather than one value"
                    .to_owned(),
            ),
            _ => Inspectable::Yes,
        }
    }

    /// R1940 — what each node is drawn as, when nobody authored a colour.
    ///
    /// All three arms are reachable here, and the two that reach the SAME
    /// outcome by different statements are both present on purpose:
    ///
    /// * The two CONSTANTS are drawn like the type they produce — an answer
    ///   derived from the node's own state, so R1937's retype gesture recolours
    ///   the node it retypes. `Number` has a colour and `Text` deliberately
    ///   does not, so *the kind said nothing* and *the kind named a type nobody
    ///   coloured* are two statements a proof can tell apart at the hook while
    ///   they agree at the screen.
    /// * `Sink` names a colour of its own: an end-of-graph node is not about a
    ///   type, and a taxonomy must be able to say so without inventing one.
    /// * Everything else is unstated, so an assertion cannot pass by condemning
    ///   the whole taxonomy.
    fn drawn_as(&self) -> Drawn<Ty> {
        match self {
            Self::Num(_) => Drawn::LikeType(Ty::Number),
            Self::Word(_) => Drawn::LikeType(Ty::Text),
            Self::Sink => Drawn::In(Tint::rgb(0x33, 0x33, 0x3A)),
            _ => Drawn::Unstated,
        }
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
            Self::Num(_) | Self::Word(_) | Self::Bundle | Self::Roster => Vec::new(),
            Self::Add => vec![
                Port::new("Augend", Ty::Number).with_default(Val::Number(0)),
                Port::new("Addend", Ty::Number).with_default(Val::Number(1)),
            ],
            Self::Mul => vec![
                Port::new("Augend", Ty::Number).with_default(Val::Number(2)),
                Port::new("Factor", Ty::Number),
            ],
            Self::Double => vec![Port::new("Value", Ty::Number)],
            // ★★★★★ R2001 — one ordinary port and one declared into the
            // advanced class, so a fold has something to hide AND something to
            // leave alone. A kind whose every port were advanced could not tell
            // "folded" from "drew nothing".
            Self::Tuned | Self::Rig => vec![
                Port::new("Value", Ty::Number),
                Port::new("Trim", Ty::Number)
                    .with_default(Val::Number(0))
                    .describing("a correction most graphs leave alone")
                    .advanced(),
            ],
            Self::Relay => vec![Port::new("In", Ty::Number)],
            Self::Shout => vec![Port::new("Phrase", Ty::Text)],
            // ★ R2003 — `Gather` takes what `Sink` takes, under the same name,
            // so a zone swap between the two pairs has something that carries
            // and the report can be told from one that carried nothing.
            Self::Sink | Self::Gather => vec![Port::new("Result", Ty::Number)],
            // R1632 — the FIXED half of a variadic kind. What repeats is
            // declared once, in `variadic`, so these two can never disagree
            // about where the run is.
            Self::Sequence | Self::Stage(_) => vec![Port::control("Execute")],
            // ★★★★★ R2003 — the same control input under the same name, plus
            // one this pair has and the other does not. Swapping to it carries
            // `Execute`; swapping AWAY from it drops `Times` and discards the
            // value authored on it, which is the report a person needs.
            Self::Span => vec![
                Port::control("Execute"),
                Port::new("Times", Ty::Number).with_default(Val::Number(1)),
            ],
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
            (Self::Roster, Side::Input) => {
                Some(Variadic::at(0, vec![Port::new("Seat", Ty::Number)]).at_least(1))
            }
            _ => None,
        }
    }

    /// R1980 — the one declaration that separates `Roster` from `Bundle`.
    ///
    /// ★ It says `Fresh` on BOTH sides while only the input side has a run to
    /// grow, which is deliberate: a taxonomy is free to want a seat of its own
    /// wherever an end arrives, and the output side is then a kind asking for
    /// something its own ports cannot give. That is the case
    /// [`Berth::Fresh`]'s warning is about, and
    /// `a_preference_is_not_a_promise_of_a_seat` is what performs it.
    fn berth(&self, _side: Side) -> Berth {
        match self {
            Self::Roster => Berth::Fresh,
            _ => Berth::Earliest,
        }
    }

    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Num(_)
            | Self::Add
            | Self::Mul
            | Self::Double
            | Self::Relay
            | Self::Tuned
            | Self::Rig => {
                vec![Port::new("Out", Ty::Number)]
            }
            Self::Word(_) | Self::Shout | Self::Bundle | Self::Roster => {
                vec![Port::new("Out", Ty::Text)]
            }
            // ★ R2003 — `Gather` PRODUCES where `Sink` does not, so the closer
            // of a zone can have a downstream wire.
            Self::Choose | Self::Blend | Self::Gather => vec![Port::new("Out", Ty::Number)],
            Self::Sink | Self::Sequence | Self::Span => Vec::new(),
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
            // ★ R2001 — the advanced port is READ, so the class is a fact about
            // how a port is drawn and never about whether it is used: a fixture
            // whose advanced port fed nothing could not tell the two apart.
            Self::Tuned | Self::Rig => {
                vec![Some(Val::Number(
                    number(0).unwrap_or(0) + number(1).unwrap_or(0),
                ))]
            }
            // A kind that declares itself a point on a wire still has to say
            // what it computes, and what it computes is the identity.
            Self::Relay => vec![inputs.first().and_then(Clone::clone)],
            Self::Shout => vec![inputs.first().and_then(Option::as_ref).map(|v| match v {
                Val::Text(t) => Val::Text(t.to_uppercase()),
                other @ Val::Number(_) => other.clone(),
            })],
            Self::Sink | Self::Sequence | Self::Span => Vec::new(),
            // ★ R2003 — a zone's closer hands out what reached it, so the
            // region between the two ends has a result the graph can go on
            // with. `Sink` swallows and this does not, which is the difference
            // that lets a plain node BECOME a zone without its downstream
            // losing its supply.
            Self::Gather => vec![number(0).map(Val::Number)],
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
            Self::Bundle | Self::Roster => vec![Some(Val::Text(
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
    all.extend(hook_round_proofs());
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
    all.extend(dcc_r1933_proofs());
    all.extend(r1934_reroute_proofs());
    all.extend(r1935_named_reroute_proofs());
    all.extend(r1980_berth_proofs());
    all.extend(r1987_autowire_proofs());
    all.extend(r1988_focus_proofs());
    all.extend(r1991_frame_proofs());
    all.extend(r1993_relocate_proofs());
    all.extend(r1994_home_proofs());
    all
}

/// R1994 — the material editor's *Home* button.
fn r1994_home_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "MaterialEditor::CameraHome",
            engine_material_editor_camera_home,
        ),
        // R1995 — and its *Clean Unused Expressions* menu entry.
        proof(
            "engine",
            "MaterialEditor::CleanUnusedExpressions",
            engine_material_editor_clean_unused_expressions,
        ),
        // R1996 — the schema hook a drag asks on every hover.
        proof(
            "engine",
            "schema::CanMergeNodes",
            engine_schema_can_merge_nodes,
        ),
        // R1997 — the schema hook a new graph is born through.
        proof(
            "engine",
            "schema::CreateDefaultNodesForGraph",
            engine_schema_create_default_nodes_for_graph,
        ),
        // R1998 — the schema hook a paste asks for a replacement.
        proof(
            "engine",
            "schema::CreateSubstituteNode",
            engine_schema_create_substitute_node,
        ),
        // R1999 — the schema hook that says what kind of graph a graph is.
        proof(
            "engine",
            "schema::GetGraphType",
            engine_schema_get_graph_type,
        ),
        // R2000 — the animation editor's verb for a wire drawn the wrong way.
        proof(
            "engine",
            "AnimGraph::ReverseTransition",
            engine_anim_graph_reverse_transition,
        ),
        // R2001 — the node hook that says whose the advanced port class is.
        proof(
            "engine",
            "node::CanUserEditPinAdvancedViewFlag",
            engine_node_can_user_edit_pin_advanced_view_flag,
        ),
    ]
}

/// A taxonomy that declares an opening, so the seeded case is reachable at all.
///
/// Two nodes and a placement, which is the reference's own shape: its
/// custom-transition schema seeds a result at the origin and pose evaluators at
/// `x = ±300`, and a sound cue's root at `y = -58`. One node at the origin
/// would have made the position half of this untestable.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Opened {
    Result,
    Source,
}

impl NodeKind for Opened {
    type Type = Ty;
    type Value = Val;
    type Graph = ();

    fn name(&self) -> String {
        match self {
            Self::Result => "Result".into(),
            Self::Source => "Source".into(),
        }
    }

    fn opening() -> Vec<Seed<Self>> {
        vec![
            Seed::new(NodeBody::Kind(Self::Result)),
            Seed::new(NodeBody::Kind(Self::Source)).at(-300, 40),
        ]
    }

    fn inputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Result => vec![Port::new("In", Ty::Number)],
            Self::Source => Vec::new(),
        }
    }

    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Result => Vec::new(),
            Self::Source => vec![Port::new("Out", Ty::Number)],
        }
    }

    fn evaluate(&self, inputs: &[Option<Val>]) -> Vec<Option<Val>> {
        match self {
            Self::Result => Vec::new(),
            Self::Source => vec![inputs.first().cloned().flatten()],
        }
    }
}

/// ★★★★★ R1997 — the schema's `CreateDefaultNodesForGraph`: **a new graph is
/// born holding what its taxonomy says a graph holds**, and can say afterwards
/// whether anyone has touched it.
///
/// Measured at the reference: the base body is empty and seven schemas override
/// it, each creating the graph's result or root node, positioning it (a sound
/// cue's at `y = -58`, a custom transition's pose evaluators at `x = ±300`),
/// and marking it `FNodeMetadata::DefaultGraphNode`. The hook returns **void**,
/// so every overrider that needs the node afterwards writes it down a second
/// time on its own graph type. ★ The marker is READ, by two functions that
/// decide what a person is TOLD — an untouched graph is offered *Drag Off Pins
/// to Create/Connect New Nodes* and a touched one *Right-Click to Create New
/// Nodes*, and a hint fades on the first placement.
///
/// The assertions that would fail if the capability were missing:
///
/// 1. a definition opened through the taxonomy holds what it declared, in
///    order, **where it declared**;
/// 2. ★ the verb SAYS what it made, which the reference's `void` cannot;
/// 3. ★ the tree can answer *has anyone touched this yet* afterwards — and
///    ⚠ a wire between two born nodes counts, where the reference's own reader
///    asks only about nodes and would answer *untouched*;
/// 4. a taxonomy that declares no opening gets an empty tree, which is the
///    reference's base body, and the verbs that FILL a definition do not seed.
#[test]
fn engine_schema_create_default_nodes_for_graph() {
    let mut document: Document<Opened> = Document::new("root");
    let born = document.open_definition("Sub");

    // ★ (2) It says what it made.
    assert_eq!(born.nodes.len(), 2, "both declared nodes: {born:?}");
    let placed: Vec<(String, i32, i32)> = born
        .nodes
        .iter()
        .map(|id| {
            let held = document
                .tree(born.tree)
                .expect("the definition")
                .node(*id)
                .expect("a node it was born with");
            let name = match &held.body {
                NodeBody::Kind(kind) => kind.name(),
                other => panic!("a seeded node is a kind: {other:?}"),
            };
            (name, held.x, held.y)
        })
        .collect();
    // ★ (1) In order, and WHERE declared.
    assert_eq!(
        placed,
        vec![("Result".to_owned(), 0, 0), ("Source".to_owned(), -300, 40),],
        "★ the taxonomy's own order and its own placement — a seed that arrived \
         at the origin would stack a three-node opening on one spot"
    );

    // ★ (3) And the tree remembers.
    assert_eq!(
        document.opening_nodes(born.tree),
        {
            let mut ids = born.nodes.clone();
            ids.sort_unstable();
            ids
        },
        "the nodes it was born with are still the nodes it was born with"
    );
    assert!(
        document.untouched(born.tree),
        "★ nobody has done anything to it yet"
    );

    // ★ A NODE somebody added counts, with nothing wired at all. Asserted
    // apart from the wire below because they are two different ways of touching
    // a graph and a verdict that noticed only one of them would pass the other.
    {
        let mut filled = document.clone();
        let placed = filled
            .add_node(born.tree, NodeBody::Kind(Opened::Source), 40, 40)
            .expect("a card a person may place");
        assert!(
            !filled.untouched(born.tree),
            "★ somebody put {placed:?} here, and no wire was made"
        );
        assert_eq!(
            filled.opening_nodes(born.tree).len(),
            2,
            "★ while what it was BORN with is unchanged"
        );
    }

    // ★★★★★ A WIRE counts. The reference's `GraphHasUserPlacedNodes` walks
    // nodes only, so a graph whose born nodes somebody had wired together still
    // answers `untouched` there — the wrong answer to the question it is asked
    // on behalf of.
    let (result, source) = (born.nodes[0], born.nodes[1]);
    document
        .connect(born.tree, Socket::new(source, 0), Socket::new(result, 0))
        .expect("the opening wires up");
    assert!(
        !document.untouched(born.tree),
        "★★★★★ somebody has wired it, so it is not untouched — and no node was \
         added or removed to say so"
    );
    assert_eq!(
        document.opening_nodes(born.tree).len(),
        2,
        "★ while what it was BORN with is unchanged: the two questions are \
         different and both are answerable"
    );

    // And a born node taken out drops out of the list rather than dangling.
    document
        .remove_node(born.tree, source)
        .expect("a node a person may delete");
    assert_eq!(
        document.opening_nodes(born.tree),
        vec![result],
        "★ what it was born with, that is still there"
    );

    a_taxonomy_with_no_opening_gets_an_empty_tree();
}

/// ★ (4) The reference's base body is empty, and the verbs that FILL a
/// definition do not seed one.
fn a_taxonomy_with_no_opening_gets_an_empty_tree() {
    // `Op` declares no opening, which is the default.
    let mut document: Document<Op> = Document::new("root");
    let born = document.open_definition("Empty");
    assert!(
        born.nodes.is_empty() && document.opening_nodes(born.tree).is_empty(),
        "a taxonomy that declares nothing gets a tree born with nothing: {born:?}"
    );
    assert!(
        document.untouched(born.tree),
        "★ and an empty tree nobody has touched is untouched — `born with \
         nothing` and `emptied` are told apart by `opening_nodes`, not here"
    );

    // ★★★★★ And `group`, which FILLS a definition from a selection, does not
    // seed: a tree about to receive nodes must not also be given them.
    //
    // ⚠ Asserted on a taxonomy that HAS an opening, which is the whole point.
    // The first draft asserted it on `Op` — which declares none — so seeding
    // the grouped definition would have added nothing and the assertion was
    // vacuous. A counterfactual that swapped `group`'s `add_definition` for
    // `open_definition` was caught by nothing until this moved.
    let mut seeded: Document<Opened> = Document::new("root");
    let source = seeded
        .add_node(ROOT, NodeBody::Kind(Opened::Source), 0, 0)
        .expect("a source");
    let result = seeded
        .add_node(ROOT, NodeBody::Kind(Opened::Result), 200, 0)
        .expect("and what it feeds");
    seeded
        .connect(ROOT, Socket::new(source, 0), Socket::new(result, 0))
        .expect("a wire between them");
    let made = seeded
        .group(ROOT, &[result], "Part")
        .expect("a definition from a selection");
    assert!(
        seeded.opening_nodes(made.definition).is_empty(),
        "★★★★★ the grouped definition was NOT seeded, though this taxonomy \
         declares two opening nodes — the reference draws the same line, \
         calling its hook at the sites that CREATE a graph rather than at the \
         ones that fill one"
    );
    assert_eq!(
        seeded
            .tree(made.definition)
            .expect("the definition")
            .nodes()
            .count(),
        3,
        "★ it holds the card that was chosen and its two interface nodes, and \
         nothing the taxonomy would have added"
    );
}

/// ★★★★★ R1996 — the schema's `CanMergeNodes`: **would this node be taken
/// there?**, asked while a hand is still carrying it.
///
/// ⚠ **Its name is not what it does, measured at its one real overrider.** The
/// base schema returns `DISALLOW` with *Not implemented by this schema*, so it
/// is opt-in; it is called from exactly one place, `FDragNode::HoverTargetChanged`,
/// on every hover while nodes are dragged; and the only schema that implements
/// it is a behaviour tree's, deciding whether a **decorator or a service may be
/// ATTACHED to the node under the cursor** — a containment question, not a
/// merge. Its answer carries a verdict that picks an icon and a sentence shown
/// to the person.
///
/// The assertions that would fail if the capability were missing:
///
/// 1. the question is answerable without moving anything, and answers what the
///    verb would;
/// 2. ★ every refusal carries its reason — where the overrider's fall-through
///    returns `DISALLOW` with an EMPTY string, though its own declaration doc
///    says an empty string means legal;
/// 3. ★ a hand can be told *where else*, which the reference has no member for:
///    its hook answers one pair at a time and only for the node under the
///    cursor.
#[test]
fn engine_schema_can_merge_nodes() {
    let mut document: Document<Op> = Document::new("root");
    let outer = document
        .add_node(ROOT, NodeBody::Frame, 0, 0)
        .expect("a region");
    let inner = document
        .add_node(ROOT, NodeBody::Frame, 10, 10)
        .expect("another");
    let card = node(&mut document, Op::Double);
    document
        .set_parent(ROOT, inner, Some(outer))
        .expect("a frame may be inside a frame");

    // ★ (1) Asked, and it moved nothing — then the verb agrees.
    let before = document.clone();
    assert_eq!(document.may_hold(ROOT, card, Some(outer)), Ok(()));
    assert_eq!(document, before, "asking is not doing");
    assert_eq!(
        document
            .set_parent(ROOT, card, Some(outer))
            .expect("what the question admitted"),
        None,
        "and it was not inside anything before"
    );

    // ★★★★★ (2) Every refusal by name, and each one still refused by the verb.
    let loose = node(&mut document, Op::Double);
    for (why, node, parent) in [
        (ParentError::NotAFrame { node: loose }, card, Some(loose)),
        (ParentError::SelfParent(outer), outer, Some(outer)),
        (
            ParentError::NoSuchNode {
                tree: ROOT,
                node: NodeId(9_999),
            },
            card,
            Some(NodeId(9_999)),
        ),
    ] {
        assert_eq!(
            document.may_hold(ROOT, node, parent),
            Err(why.clone()),
            "★ the reason is NAMED — the reference's commonest refusal is an \
             empty string, though its own doc says empty means legal"
        );
        let mut trying = document.clone();
        assert_eq!(trying.set_parent(ROOT, node, parent), Err(why));
        assert_eq!(trying, document, "★ and a refusal changed nothing");
        assert!(
            !why_is_silent(&document, node, parent),
            "every refusal this pair reaches says something"
        );
    }

    // ★ The cycle, which carries the chain it would have closed rather than a
    // bare no.
    let cycle = document.may_hold(ROOT, outer, Some(inner));
    assert!(
        matches!(&cycle, Err(ParentError::Cycle { chain }) if chain.len() >= 2),
        "★★ putting a frame inside its own descendant names the chain: {cycle:?}"
    );

    // ★★★★★ (3) WHERE ELSE — the answer the reference has no member for.
    let where_else = document
        .holders(ROOT, card)
        .expect("the card is on the canvas");
    assert_eq!(
        where_else,
        vec![inner],
        "★★ every frame that would take it, minus the one it is already in: \
         {where_else:?}"
    );
    // ★ And it is DERIVED from the same vet: a frame the vet refuses is absent
    // from the list rather than listed and refused later.
    assert!(
        !document
            .holders(ROOT, outer)
            .expect("a frame too")
            .contains(&inner),
        "a frame cannot be offered its own descendant"
    );
    assert!(
        document
            .holders(ROOT, card)
            .expect("still there")
            .iter()
            .all(|frame| document.may_hold(ROOT, card, Some(*frame)).is_ok()),
        "★ the list and the rule are one derivation"
    );
}

/// Whether a refusal for this pair would reach a person with no words — the
/// reference's own failure, asserted against here rather than described.
fn why_is_silent(document: &Document<Op>, node: NodeId, parent: Option<NodeId>) -> bool {
    document
        .may_hold(ROOT, node, parent)
        .err()
        .is_none_or(|why| why.to_string().trim().is_empty())
}

/// ★★★★★ R1995 — the material editor's *Clean Unused Expressions*: **which
/// nodes no output reaches**, and taking them out.
///
/// Measured at the reference's own body: `CleanUnusedExpressions` asks
/// `GetUnusedExpressions` for a flat list and deletes it. That list is a
/// depth-first walk UPSTREAM from the graph's outputs — the material's root
/// node, or a function's output nodes — following input pins, skipping exec
/// pins, taking `LinkedTo[0]` on each; everything unmarked is unused. Before
/// deleting it asks yes/no about the FUNCTION INPUTS AND OUTPUTS among the
/// doomed, because consumers of the function lose their connections, and says
/// nothing about the rest.
///
/// The assertions that would fail if the capability were missing:
///
/// 1. a branch nothing downstream reaches is named, and a stray card with it,
///    while everything the output depends on is left alone;
/// 2. asking changes nothing, and doing it does exactly what asking said;
/// 3. ★ a graph with **nothing to anchor it** is REFUSED — the reference marks
///    every node unused there and hands that to a command that deletes them;
/// 4. ★ the doomed that are **structural** are named apart, which is the
///    reference's dialog turned into a fact per node;
/// 5. a frame is not rubbish, though no output can ever reach one.
#[test]
fn engine_material_editor_clean_unused_expressions() {
    // `two -> add.0`, `three -> add.1`, `add -> sink.0`, plus a dead branch and
    // a card nobody wired.
    let mut chain = chain();
    let stray = node(&mut chain.document, Op::Double);
    let dead_source = num(&mut chain.document, 9);
    let dead_step = node(&mut chain.document, Op::Double);
    wire(&mut chain.document, dead_source, 0, dead_step, 0);

    // ★★★★★ The outputs are NAMED, not derived, and this fixture is why. The
    // first draft derived them as *a step with something arriving and nothing
    // leaving* — and `dead_step` is exactly that shape, so the dead branch
    // anchored ITSELF and the operation found nothing. The reference gets to
    // derive because a material HAS a root; nothing here declares one.
    let asked = chain
        .document
        .unused(ROOT, &[chain.sink])
        .expect("the sink is the output");
    assert_eq!(
        asked.from,
        vec![chain.sink],
        "★ the premise is published — a person told which nodes are unused has \
         not been told what they were measured against: {asked:?}"
    );
    let doomed: Vec<NodeId> = asked.nodes.iter().map(|d| d.node).collect();
    let mut want = vec![stray, dead_source, dead_step];
    want.sort_unstable();
    assert_eq!(
        doomed, want,
        "★ (1) the dead branch AND the card nobody wired: {asked:?}"
    );
    for kept in [chain.two, chain.three, chain.add, chain.sink] {
        assert!(
            !doomed.contains(&kept),
            "everything the output depends on is left alone: {asked:?}"
        );
    }
    assert!(!asked.clean(), "there is something to take out");
    assert!(
        asked.structural().is_empty(),
        "★ (4) and none of these is felt outside this tree: {asked:?}"
    );

    // ★ (2) Asking changed nothing, and doing it does what asking said.
    let before = chain.document.clone();
    assert_eq!(
        chain.document.unused(ROOT, &[chain.sink]).ok(),
        Some(asked.clone()),
        "asking twice answers the same"
    );
    assert_eq!(chain.document, before, "and asking moved nothing");
    let done = chain
        .document
        .prune(ROOT, &[chain.sink])
        .expect("the sink is still the output");
    assert_eq!(
        done.unused, asked,
        "the verb acted on what the question said"
    );
    assert_eq!(done.gone, want, "and every one of them went");
    assert!(done.kept.is_empty(), "none refused: {done:?}");
    assert_eq!(done.links, 1, "the dead branch's one wire went with it");
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)),
        Some(Val::Number(5)),
        "★ and the graph still computes exactly what it did"
    );
    let again = chain
        .document
        .unused(ROOT, &[chain.sink])
        .expect("still the output");
    assert!(
        again.clean(),
        "★ pruning twice has nothing left to do: {again:?}"
    );

    a_graph_nothing_anchors_is_refused_rather_than_emptied();
    a_frame_is_not_rubbish();
    a_doomed_interface_is_named_as_structural();
}

/// ★★★★★ (4) The doomed whose removal is felt OUTSIDE this tree.
///
/// ⚠ **This exists because a counterfactual PASSED.** Making `is_structural`
/// name the wrong body left every gate green: the fixtures above are all one
/// flat tree, where no node is half of a signature, so the flag was `false`
/// everywhere and a lie about it changed nothing. Unreachable rather than
/// unasserted — the repair is a fixture that reaches it (R1845), and reaching
/// it means a definition tree, which is the only place an interface node lives.
///
/// It is also exactly the case the reference's dialog is about: *any materials
/// which use this function will lose their connections to these once deleted*.
fn a_doomed_interface_is_named_as_structural() {
    let mut chain = chain();
    let made = chain
        .document
        .group(ROOT, &[chain.add], "Sum")
        .expect("a definition with an interface either side");
    let inside = made.definition;
    let faces = |document: &Document<Op>, want: InterfaceSide| -> Vec<NodeId> {
        document
            .tree(inside)
            .expect("the definition")
            .nodes()
            .filter(|node| node.body == NodeBody::Interface(want))
            .map(|node| node.id)
            .collect()
    };
    let output = *faces(&chain.document, InterfaceSide::Output)
        .first()
        .expect("what the definition is for");
    let asked = chain
        .document
        .unused(inside, &[output])
        .expect("the output interface anchors the definition");
    assert!(
        asked.clean(),
        "a freshly derived definition wastes nothing: {asked:?}"
    );

    // Cut the input interface loose from everything it fed, so it reaches
    // nothing. ⚠ EVERY wire: measured here, a definition has ONE input
    // interface node carrying a port per crossing — `in=0 out=2` for this
    // group — not one node per port, so cutting a single wire leaves it still
    // feeding through the other and still used.
    let inputs = faces(&chain.document, InterfaceSide::Input);
    let loose = *inputs.first().expect("the definition takes something in");
    let wires: Vec<LinkId> = chain
        .document
        .tree(inside)
        .expect("the definition")
        .links()
        .iter()
        .filter(|link| link.from.node == loose)
        .map(|link| link.id)
        .collect();
    assert!(!wires.is_empty(), "the interface feeds the grouped node");
    for wire in wires {
        chain
            .document
            .disconnect(inside, wire)
            .expect("a wire a person may take out");
    }

    let asked = chain
        .document
        .unused(inside, &[output])
        .expect("still anchored");
    assert_eq!(
        asked.nodes.iter().map(|d| d.node).collect::<Vec<_>>(),
        vec![loose],
        "the input nothing uses any more: {asked:?}"
    );
    assert_eq!(
        asked.structural(),
        vec![loose],
        "★★★★★ and it is named STRUCTURAL, because taking it out takes a port \
         off every instance of this definition — a consequence that does not \
         fit on this canvas. The reference asks yes/no about exactly this and \
         says nothing about the rest of its list: {asked:?}"
    );
}

/// ★★★★★ (3) A graph with no output at all.
///
/// The reference's walk starts from an empty stack, marks nothing, and returns
/// EVERY node — which its command then deletes. *Nobody has finished wiring
/// this* and *all of this is rubbish* are different facts.
fn a_graph_nothing_anchors_is_refused_rather_than_emptied() {
    let mut document: Document<Op> = Document::new("root");
    let one = num(&mut document, 1);
    let two = num(&mut document, 2);
    assert_eq!(
        document.unused(ROOT, &[]),
        Err(NotPrunable::Nothing),
        "★★★★★ nobody said what the graph is for: refused by name rather than \
         answered `all of it`, which is what the reference computes"
    );
    let untouched = document.clone();
    assert!(
        document.prune(ROOT, &[]).is_err(),
        "and the verb refuses too"
    );
    assert_eq!(document, untouched, "★ having changed nothing");
    // ★ A stale id is refused rather than skipped: skipping would quietly make
    // this a question about a SMALLER set of outputs, and a smaller set
    // condemns more nodes.
    let gone = NodeId(9_999);
    assert_eq!(
        document.unused(ROOT, &[one, gone]),
        Err(NotPrunable::NoSuchNode {
            tree: ROOT,
            node: gone
        })
    );
    // ★ The counterfactual for both refusals: the same graph with a real output
    // named answers, so they are about the naming and not about small graphs.
    let sink = node(&mut document, Op::Sink);
    wire(&mut document, one, 0, sink, 0);
    let asked = document.unused(ROOT, &[sink]).expect("now it is anchored");
    assert_eq!(
        asked.nodes.iter().map(|d| d.node).collect::<Vec<_>>(),
        vec![two],
        "and the source that feeds nothing is the unused one: {asked:?}"
    );
}

/// ★★★★★ (5) A frame is a region, not rubbish.
///
/// No output can ever reach one — a frame's signature is empty by construction
/// — so under the reference's own rule every frame on a canvas is unused. This
/// is the second consumer of `Document::steps`, and the first (R1994's `home`)
/// found the same class as a defect on the assembled screen.
fn a_frame_is_not_rubbish() {
    let mut chain = chain();
    let frame = chain
        .document
        .add_node(ROOT, NodeBody::Frame, 0, 0)
        .expect("a region on the canvas");
    let asked = chain
        .document
        .unused(ROOT, &[chain.sink])
        .expect("the sink is the output");
    assert!(
        !asked.nodes.iter().any(|doomed| doomed.node == frame),
        "★★★★★ the frame is not among the doomed: {asked:?}"
    );
    assert!(asked.clean(), "and nothing else is either: {asked:?}");
}

/// R1993 — the schema's two whole-port link operations, *move* and *copy*.
fn r1993_relocate_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "schema::MovePinLinks",
            engine_schema_move_pin_links,
        ),
        proof(
            "engine",
            "schema::CopyPinLinks",
            engine_schema_copy_pin_links,
        ),
    ]
}

/// R1991 — the script editor's two view operations, *zoom to window* and *zoom
/// to selection*.
fn r1991_frame_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "script_editor::ZoomToWindow",
            engine_script_editor_zoom_to_window,
        ),
        proof(
            "engine",
            "script_editor::ZoomToSelection",
            engine_script_editor_zoom_to_selection,
        ),
    ]
}

/// A spread graph: four cards at four places, so a fit over a subset and a fit
/// over the whole are different boxes rather than the same one.
fn spread_graph() -> (Document<Op>, [NodeId; 4]) {
    let mut document = Document::new("root");
    let at = |document: &mut Document<Op>, x, y| {
        document
            .add_node(ROOT, NodeBody::Kind(Op::Add), x, y)
            .unwrap()
    };
    let a = at(&mut document, 0, 0);
    let b = at(&mut document, 200, 100);
    let c = at(&mut document, 4_000, 0);
    let d = at(&mut document, 0, 4_000);
    (document, [a, b, c, d])
}

/// ★★★★★ R1991 — **frame the whole graph**, which this crate has had since
/// R1688 and the census did not know.
///
/// ⚠ This row was `absent` with the reason *the crate carries positions and no
/// viewport, and no binding derives one*. Measured at R1991: **the pin was true
/// the day it was written and went stale**, which is the ordinary case and
/// worth saying plainly — it was written at R1612.1 (2026-08-09) and
/// `view.rs` landed at R1688 (2026-08-14), so a capability arrived and the row
/// that says it is missing was never re-judged. `git merge-base --is-ancestor`
/// is the whole check.
///
/// Past the floor in the way the module header already claims and this asserts:
/// the fit reports whether it FITTED. The reference returns void, so an editor
/// built on it shows a corner of a too-large graph and reports success.
fn engine_script_editor_zoom_to_window() {
    let (document, [_, _, _, _]) = spread_graph();
    let fit = Fit {
        zoom: ZoomRange::new(0.25, 4.0).expect("a range"),
        margin: Margin::Canvas(0),
    };
    let whole = fit
        .run(&document, ROOT, (400, 300), |_| Some(Extent::new(100, 50)))
        .expect("four cards and a viewport");
    assert_eq!(
        whole.bounds,
        (0, 0, 4_100, 4_050),
        "every card is inside the framed box"
    );
    assert!(
        !whole.complete,
        "★ and it SAYS it could not hold this one — 4050 units into 300 pixels \
         is past the 0.25 floor. The reference has no value for this"
    );
    let roomy = Fit {
        zoom: ZoomRange::new(0.01, 4.0).expect("a range"),
        margin: Margin::Canvas(0),
    }
    .run(&document, ROOT, (400, 300), |_| Some(Extent::new(100, 50)))
    .expect("four cards");
    assert!(
        roomy.complete,
        "★ and reports true when the range does reach, so `complete` is not a \
         constant"
    );
}

/// ★★★★★ R1991 — **frame the selection**: the same fit over the cards a person
/// chose, and four separable answers when it cannot.
///
/// The floor reads its selected set and, where that set is empty or stale,
/// does nothing at all — indistinguishable by its caller from a fit that
/// worked. Every arm below is a value here, which is the claim.
fn engine_script_editor_zoom_to_selection() {
    let (document, [a, b, c, _d]) = spread_graph();
    let fit = Fit {
        zoom: ZoomRange::new(0.25, 4.0).expect("a range"),
        margin: Margin::Canvas(0),
    };
    let boxed = |node: &Node<Op>| Some(((node.x, node.y), Extent::new(100, 50)));

    let near = fit
        .selection(&document, ROOT, &[a, b], (400, 300), boxed)
        .expect("two chosen cards");
    assert_eq!(
        near.bounds,
        (0, 0, 300, 150),
        "★★★★★ the chosen pair, and not the two cards four thousand units away"
    );
    assert!(near.complete, "and that subset does fit");

    let far = fit
        .selection(&document, ROOT, &[a, c], (400, 300), boxed)
        .expect("two chosen cards");
    assert_ne!(
        far.bounds, near.bounds,
        "★ a different choice is a different frame"
    );

    // The four refusals, each its own value.
    assert_eq!(
        fit.selection(&document, ROOT, &[], (400, 300), boxed),
        Err(Unframed::Selection(
            pinion_node_graph::SelectError::NothingSelected(ROOT)
        )),
        "★★ choosing nothing is refused, not read as 'frame everything'"
    );
    assert!(matches!(
        fit.selection(&document, ROOT, &[a, NodeId(9_999)], (400, 300), boxed),
        Err(Unframed::Selection(
            pinion_node_graph::SelectError::NoSuchNode { .. }
        ))
    ));
    assert_eq!(
        fit.selection(&document, ROOT, &[a], (0, 300), boxed),
        Err(Unframed::NoViewport((0, 300))),
        "★ the window is the caller's problem, not the choice's"
    );
    assert_eq!(
        fit.selection(&document, ROOT, &[a, b], (400, 300), |_| None),
        Err(Unframed::NothingFramed { selected: 2 }),
        "★★★★★ a real choice with no boxes — the case that reads as a broken \
         button, and the count is the choice's own"
    );
}

/// R1988 — the two editors' *hide unrelated nodes*, which are two closures.
fn r1988_focus_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "script_editor::ToggleHideUnrelatedNodes",
            engine_script_editor_toggle_hide_unrelated_nodes,
        ),
        proof(
            "engine",
            "MaterialEditor::ToggleHideUnrelatedNodes",
            engine_material_editor_toggle_hide_unrelated_nodes,
        ),
    ]
}

/// ★★★★★ R1988 — **the script editor's shape**: a selection's ancestors and
/// descendants, and every node outside that pair of closures is faded.
///
/// Two rows and not one, because the two editors measured differently: this
/// one has no *whole chain* option at all, and its sibling proof below is the
/// one that carries it. What they share is that the outcome is **one bit per
/// node** — so the reason a node is lit is not recoverable there, which is what
/// [`Relatedness`] answers here.
fn engine_script_editor_toggle_hide_unrelated_nodes() {
    let c = chain();
    let answer = c.document.focus(ROOT, &[c.two], Focus::Lineage).unwrap();
    assert_eq!(answer.focus(), Focus::Lineage);
    assert_eq!(
        answer.relatedness(c.two).ties(),
        [Tie::Selected],
        "the selected card says so of itself"
    );
    assert_eq!(answer.relatedness(c.add).ties(), [Tie::Downstream]);
    assert_eq!(answer.relatedness(c.sink).ties(), [Tie::Downstream]);
    // ★ The fading half, which is what the reference's bit drives: the other
    // addend is reachable in NEITHER direction from this one.
    assert_eq!(
        answer.unrelated(),
        [c.three],
        "and it is the set a screen fades, published rather than written onto \
         the nodes"
    );
    assert_eq!(answer.relatedness(c.three), Relatedness::Unrelated);

    // ★★★★★ The reason a node is lit is recoverable, which one bit cannot do:
    // select both ends and the adder is upstream AND downstream at once.
    let both = c
        .document
        .focus(ROOT, &[c.two, c.sink], Focus::Lineage)
        .unwrap();
    assert_eq!(
        both.relatedness(c.add).ties(),
        [Tie::Upstream, Tie::Downstream],
        "★ two ties on one card — the fact a single bit per node destroys"
    );

    // ★ And the question is refused when it has no subject, rather than
    // answered with "everything is unrelated". The reference guards the same
    // case with a hidden bool and silently resets.
    assert!(c.document.focus(ROOT, &[], Focus::Lineage).is_err());
}

/// ★★★★★ R1988 — **the material editor's shape**: the same fade, plus the
/// *whole chain* option this crate spells [`Focus::Chain`].
///
/// Measured: that editor's downstream walk, with the option on, also collects
/// the upstream closure of every node it finds — so a **sibling** contributing
/// to the same result comes in, which the lineage above reaches in neither
/// direction. The option is a checkbox in a dropdown beside a bool on the
/// editor, so *on* and *on, whole chain* are two facts there and one value
/// here.
fn engine_material_editor_toggle_hide_unrelated_nodes() {
    let mut c = chain();
    // A frame holding the sibling, so the containment leg is exercised on the
    // one card the two closures disagree about — which is what makes
    // `Tie::Holding` falsifiable rather than always true.
    let holder = c.document.add_node(ROOT, NodeBody::Frame, 0, 0).unwrap();
    c.document.set_parent(ROOT, c.three, Some(holder)).unwrap();

    let lineage = c.document.focus(ROOT, &[c.two], Focus::Lineage).unwrap();
    let whole = c.document.focus(ROOT, &[c.two], Focus::Chain).unwrap();
    assert_eq!(
        lineage.relatedness(c.three),
        Relatedness::Unrelated,
        "lineage leaves the sibling out"
    );
    assert_eq!(
        lineage.relatedness(holder),
        Relatedness::Unrelated,
        "★ and so is the frame holding it — a frame is related by WHAT IT \
         HOLDS, which the reference decides from a comment rectangle and one \
         corner of a card"
    );
    assert_eq!(
        whole.relatedness(c.three).ties(),
        [Tie::Chain],
        "★ and the whole chain takes it in — under its OWN word, so a reader is \
         not told the sibling feeds the selection when it does not"
    );
    assert_eq!(
        whole.relatedness(holder).ties(),
        [Tie::Holding],
        "and the frame comes in with what it holds"
    );
    assert!(
        whole.unrelated().is_empty(),
        "this graph is one chain: {:?}",
        whole.unrelated()
    );
    // ★ The widening is a property of the pair: the chain relates everything
    // the lineage did.
    for node in lineage.related() {
        assert!(whole.relatedness(node).is_related(), "{node:?} fell out");
    }
    // ★ And the mode travels on the answer, so a screen publishing its reasons
    // cannot publish them under the wrong closure.
    assert_eq!(lineage.focus(), Focus::Lineage);
    assert_eq!(whole.focus(), Focus::Chain);
    assert_eq!(Focus::from_word("chain"), Some(Focus::Chain));
}

/// R1987 — the wire the arriving node was created by.
fn r1987_autowire_proofs() -> Vec<Proof> {
    vec![proof(
        "engine",
        "node::AutowireNewNode",
        engine_node_autowire_new_node,
    )]
}

/// R1980 — the kind's say in **where** an arriving end berths.
fn r1980_berth_proofs() -> Vec<Proof> {
    vec![proof("dcc", "node::insert_link", dcc_node_insert_link)]
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

/// ★★★★★ R1944 — the proofs the per-pin and per-node capability rounds added,
/// kept apart from the operator list above.
///
/// ⚠ A SPLIT, not an `allow`: the workspace lints a function past a hundred
/// lines and it was right — one list had grown to hold two things. These are
/// the rounds that asked a KIND or a SCHEMA a question (what a port takes, what
/// a node is drawn as, whether a value can be looked at, what closes a zone,
/// what a removal took), where the list above is the editor's OPERATORS. The
/// boundary is what the proofs are about rather than where the line count fell.
fn hook_round_proofs() -> Vec<Proof> {
    vec![
        // R1937 — the per-port type pair. TWO mechanisms, not two spellings:
        // the editor's verb, and the node's chance to say what it becomes.
        proof(
            "engine",
            "GraphEditor::ChangePinType",
            engine_graph_editor_change_pin_type,
        ),
        proof(
            "engine",
            "node::PinTypeChanged",
            engine_node_pin_type_changed,
        ),
        // R1938 — the schema's half of the same selector: which container
        // shapes a type may be held in.
        proof(
            "engine",
            "schema::SupportsPinTypeContainer",
            engine_schema_supports_pin_type_container,
        ),
        // R1939 — what a port takes as its RESTING value: the capability the
        // reference spells as an open key-value bag hung on a pin.
        proof(
            "engine",
            "node::GetPinMetaData",
            engine_node_get_pin_meta_data,
        ),
        // R1940 — what a node is drawn as, answered per NODE and derived from
        // what that node currently is.
        proof("dcc", "node::ui_class", dcc_node_ui_class),
        // R1941 — a kind's objection carries its weight, and the heaviest one
        // stops the graph being run.
        proof(
            "engine",
            "node::ValidateNodeDuringCompilation",
            engine_node_validate_node_during_compilation,
        ),
        // R1942 — whether a type's value can be looked at while the graph runs,
        // and the refusal saying which type and why.
        proof(
            "engine",
            "schema::CanShowDataTooltipForPin",
            engine_schema_can_show_data_tooltip_for_pin,
        ),
        // R1943 — a zone is a PAIR, and the region between is derived.
        proof("dcc", "add_zone", dcc_add_zone),
        // R2003 — and a zone's KIND changes without either end being replaced.
        proof("dcc", "swap_zone", dcc_swap_zone),
        // R2004 — a node that stands in for several. The animation editor's
        // self-transition command is its ONE-ELEMENT case, which is what this
        // row's own sentence had wrong.
        proof(
            "engine",
            "AnimGraph::CreateSelfTransition",
            engine_anim_graph_create_self_transition,
        ),
        // R2006 — a taxonomy's own history, run one step at a time at load.
        proof(
            "engine",
            "schema::BackwardCompatibilityNodeConversion",
            engine_schema_backward_compatibility_node_conversion,
        ),
        // R1944 — a definition can be removed, and the removal says what went.
        proof(
            "engine",
            "schema::TryDeleteGraph",
            engine_schema_try_delete_graph,
        ),
        // R1986 — the three definition verbs and the one question that decides
        // them. Five rows, five proofs: the permission is a different capability
        // from the verb it gates, which is exactly why the reference publishes
        // both and why counting them as one would hide the half that is absent.
        proof(
            "engine",
            "schema::CanBeDeleted",
            engine_schema_can_be_deleted,
        ),
        proof(
            "engine",
            "schema::CanBeRenamed",
            engine_schema_can_be_renamed,
        ),
        proof(
            "engine",
            "schema::TryRenameGraph",
            engine_schema_try_rename_graph,
        ),
        proof(
            "engine",
            "schema::CanDuplicateGraph",
            engine_schema_can_duplicate_graph,
        ),
        proof(
            "engine",
            "schema::HandleGraphBeingDeleted",
            engine_schema_handle_graph_being_deleted,
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
        // R1936 — the two group swaps. One capability with the definition
        // arriving from two places: an existing one, and a fresh empty one.
        proof("dcc", "swap_group_asset", dcc_swap_group_asset),
        proof("dcc", "swap_empty_group", dcc_swap_empty_group),
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
    vec![
        proof(
            "engine",
            "node::CanUserDeleteNode",
            engine_node_can_user_delete_node,
        ),
        // ★★★★★ R1985 — the COPY half of the same surface, and one proof for
        // two rows for this registry's own stated reason: the reference asks
        // *may you be pasted here* of a node about a destination and *may you
        // be duplicated* of a node about where it already is, and one
        // declaration answers both ends. `node::CanPasteHere` CITES it.
        proof(
            "engine",
            "node::CanDuplicateNode",
            engine_node_can_duplicate_node,
        ),
    ]
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
        // ★★★★★ R1928 — the pin-NAMING pair. One proof, because they are one
        // answer here: the reference's `Should…` is a guard in front of its
        // `Get…` and a class that overrides only the guard suppresses by
        // accident, so the `Should…` row CITES this rather than owning a second
        // proof there is no second mechanism for.
        proof(
            "engine",
            "node::GetPinNameOverride",
            engine_node_get_pin_name_override,
        ),
        // ★★★★★ R1930 — the DROP pair. One proof, because they are one act
        // here: the question is the planner the verb runs, so a second proof
        // for the guard would be a second name for one mechanism.
        proof(
            "engine",
            "schema::DropPinOnNode",
            engine_schema_drop_pin_on_node,
        ),
        // ★★★★★ R1932 — the NAME-VALIDATION pair. Measured, they are two
        // capabilities on two subjects: the node's answers for that node and
        // has fourteen overriders; the schema's takes four arguments, is
        // overridden nowhere, and its consumers name a blueprint's variables
        // rather than a graph's nodes. Only the first is a node-graph
        // capability, and it is the one this proof is about.
        proof(
            "engine",
            "node::MakeNameValidator",
            engine_node_make_name_validator,
        ),
    ]
}

/// The DCC proofs this round adds, kept beside the engine's rather than folded
/// in: the census addresses a row by TREE and a proof list that mixed them would
/// make a mis-filed row look filed.
fn dcc_r1933_proofs() -> Vec<Proof> {
    vec![proof(
        "dcc",
        "node_tree::valid_socket_type",
        dcc_node_tree_valid_socket_type,
    )]
}

/// R1934 — the reroute pair. One row per tree, because the two are two
/// mechanisms and not two spellings: the DCC's is the **verb** that puts one on
/// a wire, and the engine's is the **question** an editor asks a node about
/// whether a wire runs through it.
fn r1934_reroute_proofs() -> Vec<Proof> {
    vec![
        proof("dcc", "add_reroute", dcc_add_reroute),
        proof("dcc", "insert_offset", dcc_insert_offset),
        proof(
            "engine",
            "node::ShouldDrawNodeAsControlPointOnly",
            engine_node_should_draw_node_as_control_point_only,
        ),
    ]
}

/// R1935 — the NAMED pair. Four rows, and reading all four is what showed they
/// are not four spellings of one thing: the two directions differ in the SHAPE
/// of their answer, and the two conversions are a fan-out and a fold.
fn r1935_named_reroute_proofs() -> Vec<Proof> {
    vec![
        proof(
            "engine",
            "MaterialEditor::SelectNamedRerouteDeclaration",
            engine_material_editor_select_named_reroute_declaration,
        ),
        proof(
            "engine",
            "MaterialEditor::SelectNamedRerouteUsages",
            engine_material_editor_select_named_reroute_usages,
        ),
        proof(
            "engine",
            "MaterialEditor::ConvertRerouteToNamedReroute",
            engine_material_editor_convert_reroute_to_named_reroute,
        ),
        // R2005 — and the fifth operator over the pair: one MORE far end of a
        // name that already exists.
        proof(
            "engine",
            "MaterialEditor::CreateRerouteUsageFromDeclaration",
            engine_material_editor_create_reroute_usage_from_declaration,
        ),
        proof(
            "engine",
            "MaterialEditor::ConvertNamedRerouteToReroute",
            engine_material_editor_convert_named_reroute_to_reroute,
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

/// ★★★★★ R1938 — **which container shapes a type may be held in**, answered
/// with the type each one produces.
///
/// The engine asks its schema this, and the measurement is what shaped the
/// answer here: the hook's default body is `return true` — every type in every
/// shape — and its ONE overrider in the whole tree answers `None || Array ||
/// Set || Map`, the same four. So the declaration exists and **nothing in that
/// tree ever refuses through it**, while its two consumers are both the pin
/// type selector filtering a menu that is never filtered.
///
/// Here the default is a refusal and the answer is the TYPE, so a chooser that
/// may offer a shape also knows what offering it produces — the permission and
/// the result are one value rather than two that can disagree.
#[test]
fn engine_schema_supports_pin_type_container() {
    let document: Document<Op> = Document::new("root");

    // ★ The taxonomy declares exactly one container, and the answer carries the
    // type it makes.
    assert_eq!(
        document.containers_of(&Ty::Pair),
        vec![(Container::Array, Ty::Bag)],
        "★ a bag is an ARRAY of pairs, and the answer says so"
    );
    // ★★★★★ And it REFUSES the rest, which is what the reference cannot do:
    // other shapes of the same type, and every other type.
    for ty in [Ty::Number, Ty::Text, Ty::Flag, Ty::Bag] {
        assert!(
            document.containers_of(&ty).is_empty(),
            "★ {ty:?} is in no container here — including `Bag`, so nesting is \
             refused rather than assumed"
        );
    }
    // ⚠ Per (type, SHAPE) rather than per type: the same type that has an array
    // has no set and no map, and a caller that asked only "is this
    // containerisable" would offer two shapes that do not exist.
    assert_eq!(Op::contained(&Ty::Pair, Container::Set), None);
    assert_eq!(Op::contained(&Ty::Pair, Container::Map), None);
    assert_eq!(Op::contained(&Ty::Pair, Container::Array), Some(Ty::Bag));

    // ★ The vocabulary is closed and derived: every shape has a word, and the
    // list is `Container::ALL` rather than a second list somewhere.
    assert_eq!(
        Container::ALL.map(Container::word),
        ["array", "set", "map"],
        "the words a wire uses, from the one list"
    );
}

/// ★★★★★ R1939 — **what a port will TAKE as its resting value**, said as a
/// declaration a screen can read and enforced by the edit that writes one.
///
/// # The measurement that changed this row's verdict
///
/// The reference's hook takes a pin name AND A KEY and answers a string, and
/// read from that signature alone it is an open bag of untyped metadata — which
/// is why this row's own recorded reason called the absence deliberate. Read
/// from its **consumers** it is not a bag: twenty-one call sites ask eighteen
/// distinct keys and every one asks the same question — *what may rest at this
/// port, and how should an editor offer it?* Four want a numeric range, nine a
/// filter on what may be picked, one a closed list, four how to present the
/// field.
///
/// All eleven overriders reach the SAME lookup: from the pin to the
/// DECLARATION it was generated from, falling back to its parent. ⚠ Not all by
/// CHAINING — nine call up, the tenth IS that lookup, and the eleventh chains
/// to nothing and runs the same lookup against its own model. Four add a case
/// of their own beside it — three a fixed string for one pin-and-key pair, one
/// built from the graph — and only TWO of those sit AHEAD of the lookup, the
/// other two being a fallback taken only when it answered empty. Not one of the
/// eleven reads a store hung on the port, so nobody authors that metadata on a
/// port, which is why the declaration lives on the kind here.
///
/// ★ Those two qualifications are R1939 correcting its OWN sentence before
/// publishing it: measured clause by clause, the first draft's "all eleven
/// CHAIN" and "four add a case ahead" were each half false — and each was
/// wrong in the direction that STRENGTHENS the conclusion, which is why
/// nothing would have prompted a reader to check. Absence is spelled as the empty string, so *no such key*
/// and *the key says nothing* are one value. And one shipped overrider ignores
/// the key it is asked for, answering one fixed key's value for every question
/// put to it — a defect nothing there can catch, because a string key is
/// checked against nothing.
///
/// ⇒ the bag is not built; the capability is, typed, with the refusal carrying
/// the value the same declaration would have taken.
#[test]
fn engine_node_get_pin_meta_data() {
    let mut document: Document<Op> = Document::new("root");
    let sum = document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 0, 0)
        .expect("root tree");

    // (A) The declaration is READABLE without being handed a value first —
    // which is the half a predicate cannot answer, and what an editor needs
    // before it can offer a field at all.
    assert_eq!(
        document
            .takes(ROOT, sum, PortRef::input(1))
            .as_ref()
            .map(Admits::wants),
        Some("a whole number from 0 to 100".to_owned()),
    );
    assert_eq!(
        document
            .takes(ROOT, sum, PortRef::input(0))
            .as_ref()
            .map(Admits::wants),
        Some("any value of this port's type".to_owned()),
        "★ the SIBLING port declares nothing, so this cannot pass by condemning \
         the whole kind"
    );

    // (B) The edit enforces it, and the refusal carries the value the same
    // declaration WOULD have taken — R1938's rule, that a permission and the
    // result of taking it are one answer.
    assert_eq!(
        document.set_port_value(ROOT, sum, PortRef::input(1), Val::Number(400)),
        Err(PortValueError::NotAdmitted {
            port: PortRef::input(1),
            wants: "a whole number from 0 to 100".to_owned(),
            instead: Some(Val::Number(100)),
        }),
    );
    // ★ And the repair the refusal offered is one the same declaration takes,
    // driven rather than asserted in prose.
    assert!(
        document
            .set_port_value(ROOT, sum, PortRef::input(1), Val::Number(100))
            .is_ok(),
    );
    // ★ The open sibling still takes the value its neighbour refused.
    assert!(
        document
            .set_port_value(ROOT, sum, PortRef::input(0), Val::Number(400))
            .is_ok(),
    );

    // (C) The TYPE is judged first, and reported as its own refusal: a
    // wrong-typed value is repaired by writing a value of another type, and
    // naming the narrower rule about it would send an author to fix the wrong
    // thing.
    assert!(matches!(
        document.set_port_value(ROOT, sum, PortRef::input(1), Val::Text("400".into())),
        Err(PortValueError::WrongType { .. }),
    ));

    // (D) A closed set is the other shape, and its repair is the first offer.
    let shout = document
        .add_node(ROOT, NodeBody::Kind(Op::Shout), 40, 0)
        .expect("root tree");
    assert_eq!(
        document.takes(ROOT, shout, PortRef::input(0)),
        Some(Admits::OneOf(vec![
            Val::Text("hello".to_owned()),
            Val::Text("goodbye".to_owned()),
        ])),
    );
    assert_eq!(
        document.set_port_value(ROOT, shout, PortRef::input(0), Val::Text("hi".into())),
        Err(PortValueError::NotAdmitted {
            port: PortRef::input(0),
            wants: "one of Text(\"hello\"), Text(\"goodbye\")".to_owned(),
            instead: Some(Val::Text("hello".to_owned())),
        }),
    );

    // (E) ★★★★★ And the rule can go out from under a value that was admitted
    // when it was written, which is why this is also a standing check rather
    // than only a gate on the edit. `Mul`'s second port declares nothing, so
    // 400 is authored legitimately; swapping the kind for one whose second port
    // is ranged carries the value across and leaves the document saying
    // something it would now refuse.
    let mul = document
        .add_node(ROOT, NodeBody::Kind(Op::Mul), 80, 0)
        .expect("root tree");
    document
        .set_port_value(ROOT, mul, PortRef::input(1), Val::Number(400))
        .expect("an open port takes it");
    assert!(
        document.validate().is_empty(),
        "and the document is clean while the rule still admits it"
    );
    document.set_kind(ROOT, mul, Op::Add).expect("a kind swap");
    assert_eq!(
        document.port_value(ROOT, mul, PortRef::input(1)),
        Some(&Val::Number(400)),
        "the swap carried the authored value across, which is what makes the \
         state reachable at all"
    );
    assert!(
        document
            .validate()
            .contains(&Violation::InadmissiblePortValue {
                tree: ROOT,
                node: mul,
                port: PortRef::input(1),
            }),
        "★★★★★ the standing check reports it: {:?}",
        document.validate(),
    );
}

/// ★★★★★ R1944 — **a definition can be removed from the document**, and the
/// removal says what went with it.
///
/// # The measurement
///
/// The reference's schema is asked to delete a graph, and the editor falls back
/// to its own procedure when the schema declines. Counted: **one declaration
/// (answering NO), ZERO overriders, one consumer** — so that extension point
/// has never once been taken and every deletion goes down the fallback. R1938's
/// shape: a hook whose refusal is never exercised is a hook nobody has had to
/// think about.
///
/// The fallback is the capability, and three things about it decided this:
///
/// * **It removes every node bound to that graph, unconditionally**, and
///   answers `void`. Here [`Used`] makes the caller choose, and the safe arm
///   NAMES the sites.
/// * **Whether a graph may go at all is a FLAG on the graph**, so *why not* has
///   no answer. Here the refusals are named.
/// * **It does not look for definitions its removal orphaned.**
///
/// ⚠ And this round found the structural reason the row was true: a tree's id
/// WAS its position in the document's list, so nothing could be removed without
/// every later id changing meaning. `Document::tree` searches now, and
/// `next_tree_id` counts from the highest ever handed out rather than from the
/// length — the two agreed only while removal was impossible.
#[test]
fn engine_schema_try_delete_graph() {
    let mut document: Document<Op> = Document::new("root");
    let outer = document
        .add_node(ROOT, NodeBody::Kind(Op::Double), 0, 0)
        .expect("root tree");
    let definition = document.add_definition("shared");
    let one = document
        .add_node(ROOT, NodeBody::Group(definition), 40, 0)
        .expect("root tree");
    let two = document
        .add_node(ROOT, NodeBody::Group(definition), 80, 0)
        .expect("root tree");

    // (A) ★★★★★ WHERE, not how many. `instance_count` has answered the count
    // since groups existed; a refusal a person cannot act on is a refusal they
    // work around.
    assert_eq!(document.instance_count(definition), 2);
    assert_eq!(
        document.instances_of(definition),
        vec![(ROOT, one), (ROOT, two)]
    );

    // (B) ★★★★★ The safe arm REFUSES and names the sites — the reference never
    // refuses at all, because its flag is on the graph and its fallback simply
    // removes what was bound.
    assert_eq!(
        document.remove_definition(definition, Used::Refuse),
        Err(DefinitionError::StillUsed {
            by: vec![(ROOT, one), (ROOT, two)]
        })
    );
    assert!(
        document.tree(definition).is_some(),
        "★ and a refused removal changed nothing"
    );

    // ★★★★★ A definition authored and NOT YET PLACED, standing here across the
    // removal below. It is the population that tells "this removal orphaned it"
    // from "it was already standing alone" — without it, a sweep of every
    // instance-less definition would look correct.
    let unplaced = document.add_definition("drafted, not placed");

    // (C) ★★★★★ The destructive arm REPORTS what it took, which the reference
    // answers `void` for.
    let went = document
        .remove_definition(definition, Used::TakeThemToo)
        .expect("the caller said so");
    assert_eq!(went.instances, vec![(ROOT, one), (ROOT, two)]);
    assert_eq!(went.definitions, vec![definition]);
    assert!(document.tree(definition).is_none());
    assert!(
        document
            .tree(ROOT)
            .is_some_and(|held| held.node(outer).is_some()),
        "★ and it took ONLY what stood for that definition"
    );
    assert!(
        document.tree(unplaced).is_some(),
        "★★★★★ and a definition that was ALREADY standing alone survives — the \
         removal sweeps up what IT orphaned, not every instance-less definition"
    );

    // (D) ★★★★★ AND THE ID IS NOT A POSITION ANY MORE, which is what made the
    // removal possible at all: the next definition gets a FRESH id, so a
    // `NodeBody::Group` naming the removed one cannot silently start naming it.
    let fresh = document.add_definition("later");
    assert_ne!(
        fresh, definition,
        "★★★★★ a removed id is never handed out again"
    );
    assert!(document.tree(fresh).is_some());
    assert!(
        document.tree(ROOT).is_some(),
        "★ and the root is still reachable by its own id after a gap opened \
         before it in the list"
    );
}

/// ★★★★★ R1986 — **may this definition be removed?**, asked before removing it.
///
/// # The measurement
///
/// The reference publishes *may this be deleted* on the **palette entry**, not
/// on the graph, and it answers `false` when nobody overrides it. Measured
/// across the whole tree, plugins included: **nobody overrides it** — the only
/// two sites are the declaration and one consumer, and that consumer reaches it
/// only as the last `else if` of a chain that has already handled every subject
/// that matters, a definition graph two branches earlier by reading a stored
/// bit on the graph. So for this subject the published permission hook is dead
/// code and the real answer is a flag, which can say *no* but never *why not*.
///
/// ★★★★★ What is asserted here is the property that makes the surface worth
/// having: the question and the edit are **one decision**. R1920 built that for
/// nodes; this is the definition tree's.
#[test]
fn engine_schema_can_be_deleted() {
    let mut document: Document<Op> = Document::new("root");
    let definition = document.add_definition("shared");
    let one = document
        .add_node(ROOT, NodeBody::Group(definition), 40, 0)
        .expect("root tree");

    // (A) ★ The root is not a definition, so no definition verb applies to it —
    // and the refusal says which of the two reasons it is.
    assert_eq!(
        document.may_definition(ROOT, DefinitionAct::Remove(Used::Refuse)),
        Err(DefinitionError::TheRoot)
    );
    assert_eq!(
        document.may_definition(TreeId(4242), DefinitionAct::Remove(Used::Refuse)),
        Err(DefinitionError::NoSuchTree(TreeId(4242)))
    );

    // (B) ★★★★★ The refusal NAMES the sites, before anything is attempted. The
    // reference's flag answers a bare no.
    assert_eq!(
        document.may_definition(definition, DefinitionAct::Remove(Used::Refuse)),
        Err(DefinitionError::StillUsed {
            by: vec![(ROOT, one)]
        })
    );
    assert!(
        document.tree(definition).is_some(),
        "★ and asking changed nothing"
    );

    // (C) ★★★★★ THE PROPERTY: asking and doing cannot disagree, because the
    // verb asks. Driven over both arms rather than asserted in prose.
    for used in [Used::Refuse, Used::TakeThemToo] {
        let mut scratch = document.clone();
        let asked = scratch.may_definition(definition, DefinitionAct::Remove(used));
        let done = scratch.remove_definition(definition, used);
        assert_eq!(
            asked.is_ok(),
            done.is_ok(),
            "★★★★★ {used:?}: the permission and the edit answered differently"
        );
        if let (Err(why), Err(refused)) = (asked, done) {
            assert_eq!(why, refused, "★ and for the same stated reason");
        }
    }

    // (D) ★ The destructive arm is a different question rather than a weaker
    // check: the caller has said what happens to the instances.
    assert_eq!(
        document.may_definition(definition, DefinitionAct::Remove(Used::TakeThemToo)),
        Ok(())
    );
}

/// ★★★★★ R1986 — **may this definition be renamed?**, asked before renaming it.
///
/// # The measurement
///
/// *May this be renamed* is published beside *may this be deleted*, on the
/// palette entry, and answers `true` by default. Measured across the whole
/// tree it has **four** overriders: three refuse (a placeholder entry, a
/// state-machine node, a visual-scripting event) and one answers **yes**,
/// restating the default. And the consumer that actually renames a graph does
/// not consult it at all: it gates on *may be deleted OR may be renamed*, read
/// off two stored bits. ⚠ **A rename permitted because deletion is** — which is
/// what one decision spelled in three places drifts into.
///
/// ⚠ The clause this proof does NOT assert is the interesting one: a name
/// another definition already holds is **admitted** here. See
/// `definition.rs`'s header — the fragment path adds a carried definition under
/// the name it arrives with, and the derivation that decides whether a carried
/// definition is one this document already has reads that name. A uniqueness
/// rule would make a paste grow a copy every time.
#[test]
fn engine_schema_can_be_renamed() {
    let mut document: Document<Op> = Document::new("root");
    let first = document.add_definition("Filter");
    let second = document.add_definition("Sink");

    assert_eq!(
        document.may_definition(ROOT, DefinitionAct::Rename("anything")),
        Err(DefinitionError::TheRoot),
        "★ the root's name is the document's, not a definition's"
    );
    assert_eq!(
        document.may_definition(first, DefinitionAct::Rename("   ")),
        Err(DefinitionError::NameEmpty { tree: first }),
        "★★★★★ and a name that is empty once trimmed is refused with a reason, \
         where the reference answers a bool meaning `I did not handle it`"
    );

    // ★★★★★ The measured divergence, asserted rather than assumed: a taken name
    // is ALLOWED, and the price is paid at the lookup.
    assert_eq!(
        document.may_definition(second, DefinitionAct::Rename("Filter")),
        Ok(())
    );
    document
        .rename_definition(second, "Filter")
        .expect("permitted above");
    assert_eq!(document.definitions_named("Filter"), vec![first, second]);
    assert_eq!(
        document.definition_named("Filter"),
        None,
        "★★★★★ a name two definitions hold addresses NEITHER — the discipline \
         `node_labelled` has, one level up"
    );

    // ★ And asking cannot disagree with doing, over every refusal above.
    for (subject, name) in [(ROOT, "x"), (first, "  "), (first, "Renamed")] {
        let mut scratch = document.clone();
        let asked = scratch.may_definition(subject, DefinitionAct::Rename(name));
        let done = scratch.rename_definition(subject, name);
        assert_eq!(asked.is_ok(), done.is_ok(), "★★★★★ {name:?} disagreed");
    }
}

/// ★★★★★ R1986 — **a definition can be given another name**, and the rename
/// says which name it replaced.
///
/// # The measurement
///
/// The reference publishes *rename this graph* on the schema, answering `bool`
/// meaning *I handled it*, so a refusal and *nothing to say* are the same
/// value and the caller learns neither what the name was nor why it could not
/// change.
///
/// ★★★★★ Measured across the whole tree it has exactly **one** overrider, in a
/// plugin, and that one is the argument: on a **root** graph it performs the
/// rename through its client host and then falls out to `return false`, so the
/// caller's fallback rename runs **on top of a rename that already happened**.
/// Only its non-root branch answers `true`. A bool that means *I handled it*
/// cannot be got right by the one implementation that exists.
///
/// ⚠ This round's first draft said *no overrider at all*, measured over the
/// engine source alone. The closing audit measured the whole tree and it was
/// false — see this module's `definition.rs` header note.
#[test]
fn engine_schema_try_rename_graph() {
    let mut document: Document<Op> = Document::new("root");
    let definition = document.add_definition("Filter");
    let instance = document
        .instantiate(ROOT, definition, 0, 0)
        .expect("a definition to stand for");

    // (A) ★★★★★ The previous name is the ANSWER, which is what makes the edit
    // undoable and reportable.
    assert_eq!(
        document.rename_definition(definition, "  Sifter  "),
        Ok("Filter".to_owned()),
        "★ and the wanted name is trimmed, as a node label is"
    );
    assert_eq!(
        document.tree(definition).map(|held| held.name.as_str()),
        Some("Sifter")
    );

    // (B) ★ The rename reaches what a person reads. `GetGraphDisplayInformation`
    // is answered from this same field, so a screen showing the old name would
    // be showing a second copy of it.
    let mut path = EditPath::root();
    path.enter(&document, instance)
        .expect("into the definition");
    assert_eq!(
        path.breadcrumb(&document),
        vec!["root".to_owned(), "Sifter".to_owned()],
        "★★★★★ the breadcrumb is derived, so it cannot lag the rename"
    );

    // (C) ★ A refused rename leaves the name exactly where it was — the
    // reference's routine has no such guarantee to make, because its refusal
    // path is the caller's.
    assert_eq!(
        document.rename_definition(definition, "\t\n "),
        Err(DefinitionError::NameEmpty { tree: definition })
    );
    assert_eq!(
        document.tree(definition).map(|held| held.name.as_str()),
        Some("Sifter"),
        "★★★★★ untouched by the refusal"
    );

    // (D) ★ And nothing else moved: the instance still stands for it, which is
    // the property a rename must not quietly cost.
    assert_eq!(document.instances_of(definition), vec![(ROOT, instance)]);
}

/// ★★★★★ R1986 — **a definition can be duplicated ON ITS OWN**, with no
/// instance to carry the copy, and the copy takes a name of its own.
///
/// # The measurement
///
/// *May this graph be duplicated* is the hook of this family with the most
/// overriders — **eight**, measured across the whole tree — and they answer
/// **by what the graph IS**: one reads the graph's type and admits two of them,
/// one refuses the root animation graph by name and by class, and **six**
/// answer a flat no. Its verb's supplied answer is a null pointer — a duplicate
/// that produced nothing, with no reason.
///
/// ⚠ `fork_definition` is NOT this. It needs an instance, and it rebinds that
/// instance to the copy; what the reference gates is duplicating a graph **as a
/// graph**, from a palette listing the document's definitions, where there is
/// no instance at all. That distinction is what the census row said was
/// missing, and it is asserted at (C).
#[test]
fn engine_schema_can_duplicate_graph() {
    let mut document: Document<Op> = Document::new("root");
    let definition = document.add_definition("Filter");
    let inner = document
        .add_node(definition, NodeBody::Kind(Op::Double), 0, 0)
        .expect("the definition");
    let standing = document
        .instantiate(ROOT, definition, 0, 0)
        .expect("a definition to stand for");

    // (A) ★ The permission, before the act. A copy of the root would BE a
    // definition rather than a copy of one, so it is refused — the reference
    // refuses its own root graph too, by comparing its name.
    assert_eq!(
        document.may_definition(ROOT, DefinitionAct::Duplicate),
        Err(DefinitionError::TheRoot)
    );
    assert_eq!(
        document.may_definition(TreeId(77), DefinitionAct::Duplicate),
        Err(DefinitionError::NoSuchTree(TreeId(77)))
    );
    assert_eq!(
        document.may_definition(definition, DefinitionAct::Duplicate),
        Ok(())
    );

    // (B) ★★★★★ The copy takes a NAME OF ITS OWN, numbered from the stem.
    let copy = document
        .duplicate_definition(definition)
        .expect("permitted above");
    assert_eq!(
        document.tree(copy).map(|held| held.name.as_str()),
        Some("Filter-01")
    );
    assert_eq!(
        document.definition_named("Filter"),
        Some(definition),
        "★★★★★ so the original is still addressable by name — a copy that kept \
         the name would make both of them address nothing"
    );

    // (C) ★★★★★ NOTHING STANDS FOR THE COPY. This is the whole difference from
    // `fork_definition`, which exists to rebind the instance it was given.
    assert_eq!(document.instances_of(copy), Vec::new());
    assert_eq!(
        document.instances_of(definition),
        vec![(ROOT, standing)],
        "★ and the original's instance did not move to the copy"
    );

    // (D) ★ The copy carries the contents, and the two are independent.
    assert_eq!(
        document.tree(copy).map(Tree::node_count),
        document.tree(definition).map(Tree::node_count)
    );
    document
        .add_node(copy, NodeBody::Kind(Op::Sink), 40, 0)
        .expect("the copy");
    assert_eq!(
        document.tree(definition).map(Tree::node_count),
        Some(1),
        "★★★★★ editing the copy did not reach the original"
    );
    assert!(
        document
            .tree(definition)
            .is_some_and(|held| held.node(inner).is_some())
    );

    // (E) ★ A copy of the copy numbers from the STEM rather than growing a
    // tail, which is R1985's rule on the node axis reused here.
    let again = document
        .duplicate_definition(copy)
        .expect("a copy may be copied");
    assert_eq!(
        document.tree(again).map(|held| held.name.as_str()),
        Some("Filter-02")
    );
}

/// ★★★★★ R1986 — **what a removal will take is answerable before it takes it**,
/// which is this crate's form of *the graph is going away*.
///
/// # The measurement
///
/// The reference publishes that notification on the schema and, measured across
/// the whole tree, it has **six** overriders. Reading **all six** is what said
/// what it is for: every one of them finds the node bound to the departing
/// graph and deletes it, and one does more — it also drops the graph from the
/// recently-edited list and clears the breakpoints inside it. ⇒ the capability
/// is *everything keyed to this definition is told, before it goes*.
///
/// Two things there make that half a capability:
///
/// * the listener is handed **one graph**, and the reference's own removal path
///   does not cascade, so it cannot be told about a chain it orphaned;
/// * it is handed the graph and left to walk it, so what was inside is
///   something each listener re-derives.
///
/// Here it is a question, answered before the act, and the removal is that same
/// answer applied.
#[test]
fn engine_schema_handle_graph_being_deleted() {
    let mut document: Document<Op> = Document::new("root");
    let outer = document.add_definition("Outer");
    let inner = document.add_definition("Inner");
    let shared = document.add_definition("Shared");
    // ★ The chain: the root stands for Outer, and Outer stands for Inner. So
    // removing Outer orphans Inner — the case the reference cannot report.
    let standing = document
        .instantiate(ROOT, outer, 0, 0)
        .expect("a definition to stand for");
    let nested = document
        .instantiate(outer, inner, 0, 0)
        .expect("a definition to stand for");
    // ★★★★★ AND THE CASE THAT SEPARATES *orphaned* FROM *touched*: `Shared` has
    // one instance inside the departing tree and one that survives it, so it
    // must NOT be swept. Added because a counterfactual that weakened the rule
    // from "every instance is going" to "any instance is going" was **caught by
    // nothing** on the fixture without it — the population, not the assertion,
    // was what could not tell them apart (R1845's class).
    let shared_here = document
        .instantiate(ROOT, shared, 80, 0)
        .expect("a definition to stand for");
    let shared_in = document
        .instantiate(outer, shared, 80, 0)
        .expect("a definition to stand for");
    let leaf = document
        .add_node(inner, NodeBody::Kind(Op::Double), 40, 0)
        .expect("the inner definition");
    let kept = document.add_definition("Unplaced");

    let before: BTreeSet<(TreeId, NodeId)> = (0..document.tree_count())
        .filter_map(|index| document.tree(TreeId(u32::try_from(index).unwrap_or(u32::MAX))))
        .flat_map(|held| held.nodes().map(move |node| (held.id, node.id)))
        .collect();

    // (A) ★★★★★ Asked BEFORE, and it names the chain.
    let coming = document
        .would_remove_definition(outer, Used::TakeThemToo)
        .expect("the caller named the destructive arm");
    assert_eq!(
        coming.definitions,
        vec![outer, inner],
        "★★★★★ the orphaned definition is in the report, which is what the \
         reference's one-graph notification cannot say"
    );
    assert!(
        !coming.definitions.contains(&kept),
        "★ and a definition that was ALREADY standing alone is not swept up"
    );
    assert!(
        !coming.definitions.contains(&shared),
        "★★★★★ nor one that merely HAS an instance inside the departing tree — \
         another instance survives it, so it is not orphaned"
    );

    // (B) ★★★★★ The departing trees can still be READ while the answer is held,
    // which is the whole reason the question comes before the act.
    assert_eq!(
        coming
            .definitions
            .iter()
            .filter_map(|id| document.tree(*id))
            .map(|held| held.name.clone())
            .collect::<Vec<_>>(),
        vec!["Outer".to_owned(), "Inner".to_owned()],
        "★ a report of bare ids after the fact could not answer this at all"
    );

    // (C) ★★★★★ And it names the nodes INSIDE them, not only the ones that
    // stood for them — the half a side table keyed by (tree, node) needs.
    assert_eq!(coming.instances, vec![(ROOT, standing), (outer, nested)]);
    assert_eq!(
        coming.nodes,
        vec![(outer, nested), (outer, shared_in), (inner, leaf)]
    );

    // (D) ★ The removal reports exactly what the question predicted, because it
    // IS that derivation applied.
    let went = document
        .remove_definition(outer, Used::TakeThemToo)
        .expect("the same answer");
    assert_eq!(went, coming);

    // (E) ★★★★★ THE COMPLETENESS ASSERTION: the report is the whole population.
    // Counted two ways — what the report says went, and what the document no
    // longer holds — and the two must agree. A notification that names some of
    // what left is worse than none, because a listener trusts it.
    let after: BTreeSet<(TreeId, NodeId)> = (0..document.tree_count())
        .filter_map(|index| document.tree(TreeId(u32::try_from(index).unwrap_or(u32::MAX))))
        .flat_map(|held| held.nodes().map(move |node| (held.id, node.id)))
        .collect();
    let reported: BTreeSet<(TreeId, NodeId)> = went
        .instances
        .iter()
        .chain(went.nodes.iter())
        .copied()
        .collect();
    assert_eq!(
        before.difference(&after).copied().collect::<BTreeSet<_>>(),
        reported,
        "★★★★★ every node that left is named, and nothing that stayed is"
    );
    assert!(document.tree(kept).is_some());
    assert!(
        document.tree(shared).is_some()
            && document.instances_of(shared) == vec![(ROOT, shared_here)],
        "★ and the surviving instance still stands for a definition that is here"
    );
    assert!(
        document.validate().is_empty(),
        "★★★★★ nothing is left naming a tree that is gone: {:?}",
        document.validate()
    );
}

/// ★★★★★ R1943 — **a zone is a PAIR of nodes**, and the region between them is
/// derived rather than stored.
///
/// # The measurement
///
/// The reference's add-a-zone operator does four things: it creates an INPUT
/// node and an OUTPUT node, pairs them, places them either side of the cursor,
/// and wires the one socket they share. So the census row's "a bracketed region
/// of ONE tree" describes what a person SEES; what the model holds is a pair.
/// Its four zones — a simulation across a time span, a dynamic repetition, a
/// per-element operation, and a closure evaluated elsewhere — are four such
/// pairs.
///
/// ★★★★★ TWO MEASURED DEFECTS, and each decides a piece of what is built here:
///
/// * **The pairing is a ONE-WAY id**, stored on the opener as the closer's
///   identifier with nothing on the closer. Asking a closer what it closes
///   means walking every opening node in the tree — its own pairing routine
///   performs exactly that walk to find out whether a closer is spoken for.
/// * **Its refusals are REPORTED, not returned**: the routine answers `bool`
///   and writes the reason into a report list, so *wrong kind of closer* and
///   *that closer is already paired* reach a caller as the same `false`.
///   R1942's class, on another axis.
///
/// ★ AND ONE MORE THIS ROUND FOUND: it checks only whether the CLOSER is
/// spoken for. An opener that is already in a zone simply has its stored id
/// overwritten, so re-pairing an opener silently abandons the zone it was in.
/// Both ends are checked here.
#[test]
fn dcc_add_zone() {
    let mut document: Document<Op> = Document::new("root");
    let opens = document
        .add_node(ROOT, NodeBody::Kind(Op::Sequence), 0, 0)
        .expect("root tree");
    let closes = document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 80, 0)
        .expect("root tree");

    // (A) ★ An opener declares its closer as a KIND, so what may close it is a
    // value rather than a rule a caller has to know. The reference reaches the
    // same fact through three registry hops (node type -> zone type -> that
    // zone's output type).
    assert_eq!(Op::Sequence.closed_by(), Some(Op::Sink));
    assert_eq!(Op::Double.closed_by(), None);

    // (B) ★★★★★ An opener with nothing closing it is a NAMED state, which the
    // reference reaches routinely (its operator creates both nodes before
    // pairing them) and cannot say: there an unpaired opener is one whose
    // stored id resolves to nothing.
    assert_eq!(document.in_zone(ROOT, opens), Some(InZone::OpensNothingYet));
    assert_eq!(
        document.in_zone(ROOT, closes),
        None,
        "★ and a node that merely COULD close one is not in a zone — the closer \
         is found through the opener, never by asking the closer"
    );

    // (C) The pair is made, and BOTH ends answer — the half the reference
    // cannot without scanning every opening node in the tree.
    document.pair(ROOT, opens, closes).expect("a zone");
    assert_eq!(document.in_zone(ROOT, opens), Some(InZone::Opens(closes)));
    assert_eq!(document.in_zone(ROOT, closes), Some(InZone::Closes(opens)));

    // (D) ★★★★★ EVERY REFUSAL IS ITS OWN ARM, which is the measured difference:
    // there they are one `bool` and a report list.
    let other = document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 160, 0)
        .expect("root tree");
    assert_eq!(
        document.pair(ROOT, opens, other),
        Err(PairError::AlreadyPaired {
            node: opens,
            with: closes
        }),
        "★ the OPENER being spoken for is caught — the reference checks only \
         the closer and overwrites the opener's id"
    );
    let plain = document
        .add_node(ROOT, NodeBody::Kind(Op::Double), 240, 0)
        .expect("root tree");
    assert_eq!(
        document.pair(ROOT, plain, other),
        Err(PairError::OpensNothing(plain))
    );
    let second = document
        .add_node(ROOT, NodeBody::Kind(Op::Sequence), 320, 0)
        .expect("root tree");
    assert_eq!(
        document.pair(ROOT, second, plain),
        Err(PairError::WrongCloser {
            opener: second,
            closer: plain
        }),
        "★ and the wrong KIND of closer is a different refusal from no closer \
         at all"
    );
    assert_eq!(
        document.pair(ROOT, second, second),
        Err(PairError::ItsOwnCloser(second))
    );

    // (E) ★ Taking it apart is addressed by EITHER end, because a person
    // clicking a node has whichever end they clicked.
    assert!(document.unpair(ROOT, closes), "addressed by the closer");
    assert_eq!(
        document.in_zone(ROOT, opens),
        Some(InZone::OpensNothingYet),
        "★ and the opener is back to waiting rather than to nothing"
    );
    assert_eq!(document.in_zone(ROOT, closes), None);
    // ★★★★★ And the freed closer can be used, which is what makes the refusal
    // in (D) about STATE rather than about identity.
    document
        .pair(ROOT, second, closes)
        .expect("the closer is free now");
    assert_eq!(document.in_zone(ROOT, closes), Some(InZone::Closes(second)));
}

/// The starting position that sections (A) to (D) of the `swap_zone` proof all
/// run over: a zone whose opener passes control on and whose closer is fed a
/// number, so a swap has something to carry, something to drop and a wire to
/// sever.
///
/// ★ Its own function because those four are four separate claims about the
/// same starting position — one test asserting all of them says only
/// *something in here broke*, and this crate's own rule is that an assertion
/// names what it is about.
struct SwappableZone {
    document: Document<Op>,
    opens: NodeId,
    closes: NodeId,
    ran: LinkId,
    fed_to_closer: LinkId,
}

fn a_zone_with_flow_through_both_ends() -> SwappableZone {
    let mut document: Document<Op> = Document::new("root");
    let opens = document
        .add_node(ROOT, NodeBody::Kind(Op::Sequence), 0, 0)
        .expect("root tree");
    let closes = document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 200, 0)
        .expect("root tree");
    let body = document
        .add_node(ROOT, NodeBody::Kind(Op::Stage(3)), 100, 100)
        .expect("root tree");
    let feed = num(&mut document, 7);
    document.pair(ROOT, opens, closes).expect("a zone");
    let ran = document
        .connect(ROOT, Socket::new(opens, 0), Socket::new(body, 0))
        .expect("control leaves the opener")
        .link;
    let fed_to_closer = document
        .connect(ROOT, Socket::new(feed, 0), Socket::new(closes, 0))
        .expect("a number reaches the closer")
        .link;
    SwappableZone {
        document,
        opens,
        closes,
        ran,
        fed_to_closer,
    }
}

/// ★★★★★ R2003 — **a zone's KIND changes, and the two nodes it is made of
/// survive the change.**
///
/// # What the reference's operator does, measured at it this round
///
/// It is offered from the *swap* menu with a pair of node-type strings, and it
/// has two arms: a node already in a zone has the whole zone re-made, and an
/// ordinary node is turned into one with its incoming wires going to the new
/// opener and its outgoing wires to the new closer.
///
/// Four measurements decided what is built here:
///
/// * **It destroys both ends.** Two nodes are created, the old pair's settings,
///   values and links are copied across, and the old pair is deleted — so every
///   id dies and with it every selection, saved layout, held reference and undo
///   record keyed by one. That is R1598's argument met on the zone axis, and it
///   is why this verb re-kinds the nodes in place.
/// * **The two ends' kinds are two independent arguments**, so a caller there
///   can ask for a zone whose opener never declared that closer. Here the
///   closer is [`NodeKind::closed_by`]'s own answer, so the mismatched request
///   is not refused — it cannot be written.
/// * **Its item transfer swallows what will not cross**: a loop over the old
///   zone's items matching by NAME only, with `except RuntimeError: pass`
///   around the one call that can fail and a silent early return when either
///   side has no item list at all. Here both ends go through the same
///   correspondence a swap uses, and everything that does not survive is NAMED.
/// * **It has no branch for an opener whose closer has been deleted.** Its pair
///   lookup answers *(this node, the thing it is paired with)* whenever the
///   node has a pairing field at all, and that field holds nothing in exactly
///   that state — which is a state this crate names
///   ([`InZone::OpensNothingYet`]) and answers for.
#[test]
fn dcc_swap_zone() {
    let SwappableZone {
        mut document,
        opens,
        closes,
        ran,
        fed_to_closer,
    } = a_zone_with_flow_through_both_ends();

    // (A) ★ The kind asked for has to be one that OPENS. The refusal is its own
    // arm and carries no payload: the kind is the caller's own argument, so
    // what a caller needs is which of the things it passed was wrong.
    assert_eq!(Op::Span.closed_by(), Some(Op::Gather));
    assert_eq!(Op::Double.closed_by(), None);

    assert_eq!(
        document.set_zone_kind(ROOT, opens, Op::Double, (0, 0)),
        Err(ZoneSwapError::KindOpensNothing),
        "★ a kind that opens no zone cannot be what a zone becomes"
    );

    // (B) ★★★★★ The zone changes kind, ADDRESSED FROM THE CLOSER, and both ends
    // keep their ids and their pairing. The reference has to be addressed from
    // either end too, and gets there by walking every node in the tree.
    let swapped = document
        .set_zone_kind(ROOT, closes, Op::Span, (0, 0))
        .expect("the zone becomes a Span");
    assert_eq!(
        swapped.was,
        Some(InZone::Closes(opens)),
        "★ the report says WHICH END the person addressed, in the vocabulary \
         `in_zone` already answers in — a screen puts focus back where the \
         gesture happened"
    );
    assert_eq!((swapped.opens, swapped.closes), (opens, Some(closes)));
    assert_eq!(
        swapped.made(),
        None,
        "★ nothing was made: the zone had two ends"
    );
    assert_eq!(
        document.in_zone(ROOT, opens),
        Some(InZone::Opens(closes)),
        "★★★★★ the PAIRING survived, which two `set_kind` calls cannot do"
    );
    assert_eq!(document.in_zone(ROOT, closes), Some(InZone::Closes(opens)));
    assert_eq!(
        swapped.opened.carried,
        vec![Carried {
            from: PortRef::input(0),
            to: PortRef::input(0),
            by_name: true,
        }],
        "★ the opener's control input carried BY NAME — the author's own \
         statement that these are the same port"
    );
    assert_eq!(
        swapped.opened.dropped,
        vec![PortRef::output(0), PortRef::output(1)],
        "★ and the run of control outputs had nowhere to go, so it is named"
    );
    assert_eq!(
        swapped
            .opened
            .severed
            .iter()
            .map(|l| l.id)
            .collect::<Vec<_>>(),
        vec![ran],
        "★★★★★ the wire that was on a dropped port is NAMED. The reference \
         drops what will not fit inside swallowed exceptions, so there a swap \
         and a swap that cost you a wire are the same outcome"
    );
    assert_eq!(
        swapped
            .closed
            .as_ref()
            .expect("the closer was re-kinded")
            .carried,
        vec![Carried {
            from: PortRef::input(0),
            to: PortRef::input(0),
            by_name: true,
        }],
        "★ and the CLOSER is reported separately, because an end can lose what \
         the other kept"
    );
    assert!(
        document
            .tree(ROOT)
            .expect("root")
            .link(fed_to_closer)
            .is_some(),
        "★ the wire the new closer still answers for is untouched"
    );
}

/// ★★★★★ R2003 — **the two ends of a zone cannot be asked to disagree.**
///
/// Section (C) of the `swap_zone` proof, its own test because it is its own
/// claim: the reference takes the two ends as two independent strings, so a
/// zone whose ends never declared each other is *sayable* there and is not
/// sayable here.
#[test]
fn dcc_swap_zone_gives_the_two_ends_kinds_that_agree() {
    let SwappableZone {
        mut document,
        closes,
        ..
    } = a_zone_with_flow_through_both_ends();
    document
        .set_zone_kind(ROOT, closes, Op::Span, (0, 0))
        .expect("the zone becomes a Span");

    // (C) ★★★★★ And the kinds of the two ends cannot disagree: only the opener
    // was asked for, and the closer is what that opener DECLARES closes it.
    assert!(
        matches!(
            document
                .tree(ROOT)
                .and_then(|t| t.node(closes))
                .map(|n| &n.body),
            Some(NodeBody::Kind(Op::Gather))
        ),
        "★★★★★ the reference takes the two ends as two independent strings, so \
         a zone whose ends never declared each other is sayable there"
    );
}

/// ★★★★★ R2003 — **swapping a zone BACK names every piece of what it cost.**
///
/// Section (D) of the `swap_zone` proof. Its own test because the loss here is
/// a *different* loss from the one the outward swap reports — an authored value
/// with nowhere to sit, and a wire on an output the old pair did not have — and
/// the reference drops both inside a swallowed exception.
#[test]
fn dcc_swap_zone_back_again_names_what_it_cost() {
    let SwappableZone {
        mut document,
        opens,
        closes,
        ..
    } = a_zone_with_flow_through_both_ends();
    document
        .set_zone_kind(ROOT, closes, Op::Span, (0, 0))
        .expect("the zone becomes a Span");

    // (D) ★★★★★ Swapping BACK is lossy in a different way, and every piece of
    // it is named: an authored value with nowhere to sit, and a wire on an
    // output the old pair did not have.
    document
        .set_port_value(ROOT, opens, PortRef::input(1), Val::Number(9))
        .expect("Times takes a number");
    let after = document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 320, 0)
        .expect("root tree");
    let onward = document
        .connect(ROOT, Socket::new(closes, 0), Socket::new(after, 0))
        .expect("the closer produces")
        .link;
    let back = document
        .set_zone_kind(ROOT, opens, Op::Sequence, (0, 0))
        .expect("the zone becomes a Sequence again");
    assert_eq!(
        back.opened.discarded,
        vec![(PortRef::input(1), Val::Number(9))],
        "★★★★★ the VALUE, not just its address — a report that named the port \
         alone leaves a caller nothing to show or to put back"
    );
    assert_eq!(
        back.closed
            .as_ref()
            .expect("the closer was re-kinded")
            .severed
            .iter()
            .map(|l| l.id)
            .collect::<Vec<_>>(),
        vec![onward],
        "★ and the closer losing its output is the closer's own report"
    );
    assert!(
        !back.lossless(),
        "★ which is what a swap that cost something says"
    );
    assert_eq!(document.in_zone(ROOT, opens), Some(InZone::Opens(closes)));
}

/// ★★★★★ R2003 — **an ordinary node BECOMES a zone, keeps its id, and its flow
/// is split across the two ends.**
///
/// Section (E) of the `swap_zone` proof, and the claim the reference cannot
/// make: it deletes the node and builds two, so every id dies and with it every
/// selection, saved layout, held reference and undo record keyed by one.
#[test]
fn dcc_swap_zone_makes_a_zone_of_an_ordinary_node() {
    // (E) ★★★★★ An ORDINARY node becomes a zone, keeps its id, and its flow is
    // SPLIT: what fed it feeds the opener, what it fed is fed by the closer.
    let mut plain: Document<Op> = Document::new("root");
    let source = num(&mut plain, 5);
    let node = plain
        .add_node(ROOT, NodeBody::Kind(Op::Double), 40, 60)
        .expect("root tree");
    let downstream = plain
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 240, 60)
        .expect("root tree");
    let upstream_link = plain
        .connect(ROOT, Socket::new(source, 0), Socket::new(node, 0))
        .expect("wired in")
        .link;
    let downstream_link = plain
        .connect(ROOT, Socket::new(node, 0), Socket::new(downstream, 0))
        .expect("wired on")
        .link;

    let became = plain
        .set_zone_kind(ROOT, node, Op::Span, (150, 20))
        .expect("the node becomes a zone");
    assert_eq!(
        became.was, None,
        "★ it was in no zone, which is what makes this the other arm"
    );
    let made = became.made().expect("a closer was made for it");
    assert_eq!(
        became.opens, node,
        "★★★★★ THE NODE KEPT ITS ID. The reference \
         deletes it and builds two, so every reference to it dies"
    );
    assert_eq!(became.closes, Some(made));
    let placed = plain
        .tree(ROOT)
        .and_then(|t| t.node(made))
        .expect("the made closer");
    assert_eq!(
        (placed.x, placed.y),
        (190, 80),
        "★ placed by the offset the CALLER gave — where a screen likes its \
         cards is the application's, and the reference's own offset is a \
         settable property of the operator"
    );
    assert_eq!(
        became.handed,
        vec![Carried {
            from: PortRef::output(0),
            to: PortRef::output(0),
            by_name: true,
        }],
        "★ the outgoing side was handed over, by the same correspondence a \
         swap uses rather than a second rule"
    );
    let wires = plain.tree(ROOT).expect("root");
    assert_eq!(
        wires.link(downstream_link).map(|l| l.from),
        Some(Socket::new(made, 0)),
        "★★★★★ the downstream wire KEPT ITS ID and now leaves the closer, so \
         the region between the two ends is what was inserted into the flow"
    );
    assert_eq!(
        wires.link(upstream_link).map(|l| l.to),
        Some(Socket::new(node, 1)),
        "★ and the incoming wire found the one port of the opener that would \
         take it — the control input could not, and the correspondence said so \
         rather than dropping the wire"
    );
    assert_eq!(plain.in_zone(ROOT, node), Some(InZone::Opens(made)));
    assert_eq!(plain.in_zone(ROOT, made), Some(InZone::Closes(node)));
    assert!(plain.validate().is_empty());
}

/// ★ R2003 — **an opener with nothing closing it yet is re-kinded and stays
/// exactly that.**
///
/// Section (F) of the `swap_zone` proof. Making a closer here would be
/// inventing a node to answer a request that did not ask for one, and this is
/// the state the reference's own pair lookup has no branch for.
#[test]
fn dcc_swap_zone_leaves_a_lone_opener_lone() {
    // (F) ★ An opener with nothing closing it yet is re-kinded and STAYS that
    // way. Making a closer here would be inventing a node to answer a request
    // that did not ask for one — and this is the state the reference's own pair
    // lookup has no branch for.
    let mut alone: Document<Op> = Document::new("root");
    let waiting = alone
        .add_node(ROOT, NodeBody::Kind(Op::Sequence), 0, 0)
        .expect("root tree");
    assert_eq!(alone.in_zone(ROOT, waiting), Some(InZone::OpensNothingYet));
    let held = alone
        .set_zone_kind(ROOT, waiting, Op::Span, (150, 0))
        .expect("re-kinded");
    assert_eq!(held.was, Some(InZone::OpensNothingYet));
    assert_eq!((held.closes, held.made()), (None, None));
    assert_eq!(alone.tree(ROOT).expect("root").nodes().count(), 1);
    assert_eq!(alone.in_zone(ROOT, waiting), Some(InZone::OpensNothingYet));
}

/// ★★★★★ R2003 — **a zone is TWO kinds, so it can be unopenable in a tree that
/// would have taken its opener perfectly happily**, and the refusal leaves the
/// document as it found it.
///
/// Section (G) of the `swap_zone` proof.
#[test]
fn dcc_swap_zone_refuses_when_the_closer_has_nowhere_to_go() {
    // (G) ★★★★★ A zone is TWO kinds, so it can be unopenable in a tree that
    // would have taken its opener perfectly happily — and the refusal names the
    // taxonomy's own reason rather than folding it into a general no.
    let mut planes: Document<Placed> = Document::new("root");
    let host = planes
        .add_node(ROOT, NodeBody::Kind(Placed::DataOnly), 0, 0)
        .expect("this tree is a data plane, so this kind is at home in it");
    assert_eq!(Placed::Anywhere.closed_by(), Some(Placed::Nowhere));
    assert_eq!(
        planes.in_zone(ROOT, host),
        None,
        "★ an ordinary node, so this is the arm that has to MAKE a closer"
    );
    let refused = planes.set_zone_kind(ROOT, host, Placed::Anywhere, (150, 0));
    assert!(
        matches!(refused, Err(ZoneSwapError::CloserRefused { .. })),
        "★ the node refused is not the one the caller named: {refused:?}"
    );
    assert_eq!(
        planes.tree(ROOT).expect("root").nodes().count(),
        1,
        "★★★★★ and a refused swap left the document as it found it — everything \
         that can refuse is asked before anything moves"
    );
}

/// ★★★★★ R1942 — **whether a type's value can be LOOKED AT while the graph
/// runs**, and a refusal that says which type and why.
///
/// # The measurement
///
/// The reference's schema is asked whether a pin may show its data, and the
/// answer gates whether a debugger lets a person inspect that pin at all.
/// Counted: **one supplied declaration** (answering NO — a bare schema knows
/// none of its types and cannot vouch for any), **two** overriders, **one**
/// consumer.
///
/// The two overriders decide the shape. One refuses **execution** pins and
/// **delegate** pins; the other refuses **pose** pins and defers to the first.
/// Execution is already answered here — control is not a value, and
/// [`WatchError::NotAValue`] has refused it since R1644. The other two are the
/// gap: a type that CARRIES a value and still has nothing a person can read.
///
/// ★★★★★ AND THE MEASURED DEFECT IS THAT THE ANSWER IS A BARE `bool`. Its one
/// consumer asks five separate questions — the pin is orphaned, the owning node
/// is disabled, the schema refuses, there is no debug context, the session is
/// not running — and folds every one into the same `false`. A person told *no*
/// cannot tell which of the five it was. Here the refusal carries its sentence
/// and its own arm, so the crate's refusal and the taxonomy's are two different
/// answers rather than one word.
#[test]
fn engine_schema_can_show_data_tooltip_for_pin() {
    let mut document: Document<Op> = Document::new("root");
    let carry = document
        .add_node(ROOT, NodeBody::Kind(Op::Carry), 0, 0)
        .expect("root tree");
    let mut watches = Watches::default();

    // (A) The declaration is READABLE without a port in hand — what a screen
    // needs before it offers a watch affordance at all.
    assert_eq!(<Op as NodeKind>::inspectable(&Ty::Number), Inspectable::Yes);
    assert!(matches!(
        <Op as NodeKind>::inspectable(&Ty::Bag),
        Inspectable::No(_)
    ));

    // (B) ★★★★★ THE WATCH ENFORCES IT, and the refusal carries the taxonomy's
    // own sentence — `Carry`'s inputs are (Go: control, Whole: Pair, Loose:
    // Bag), so one node reaches all three answers.
    assert_eq!(
        document.set_watch(
            &mut watches,
            PortSite::at(ROOT, carry, PortRef::input(1), Instance::root()),
        ),
        Ok(true),
        "★ the composite port is watchable, so this cannot pass by refusing \
         everything"
    );
    let refused = document
        .set_watch(
            &mut watches,
            PortSite::at(ROOT, carry, PortRef::input(2), Instance::root()),
        )
        .expect_err("a bag has no one value to read");
    match &refused {
        WatchError::NotInspectable { port, why, .. } => {
            assert_eq!(*port, PortRef::input(2));
            assert!(
                why.contains("handle to however many"),
                "★★★★★ the sentence is the TAXONOMY's, carried rather than \
                 re-derived: {why:?}"
            );
        }
        other => panic!("expected the taxonomy's refusal, got {other:?}"),
    }

    // (C) ★★★★★ AND IT IS A DIFFERENT ANSWER FROM THE CRATE'S. Control is
    // refused on its own arm, by a rule no taxonomy has a say in — the
    // distinction the reference cannot make, because both reach it through one
    // `bool` from one schema call.
    assert!(matches!(
        document.set_watch(
            &mut watches,
            PortSite::at(ROOT, carry, PortRef::input(0), Instance::root()),
        ),
        Err(WatchError::NotAValue { .. })
    ));

    // (D) ★ The refusal READS as a sentence, which is what a debugger shows.
    assert!(
        refused.to_string().contains("holds a bag of pairs"),
        "{refused}"
    );

    // (E) ★★★★★ And a watch armed while the type permitted it is REPORTED as
    // stale when the port's type changes under it — the half a gate on the
    // arming alone would miss. `Carry`'s second input is a Pair; retyping the
    // node is not available here, so this drives the equivalent reachable
    // state: a watch on a port that stops existing.
    let listed = document.stale_watches(&watches);
    assert!(
        listed.is_empty(),
        "nothing is stale while the document stands: {listed:?}"
    );
    document
        .set_kind(ROOT, carry, Op::Double)
        .expect("a kind swap");
    let listed = document.stale_watches(&watches);
    assert!(
        !listed.is_empty(),
        "★★★★★ the watch on a port the swap removed is REPORTED, with why — \
         the reference answers one watch at a time and only `bool`"
    );
}

/// ★★★★★ R1941 — **a kind's objection carries its WEIGHT, and the heaviest
/// one stops the graph being run.**
///
/// # The measurement
///
/// The reference asks each graph node to validate itself during compilation,
/// handing it the compiler's message log. Counted this round: **one supplied
/// (empty) declaration, 53 overriding declarations, 57 implementations, and 5
/// real call sites** — the rest of the matches are comments. The call sits at
/// the end of the structural well-formedness pass, and the pass's verdict is
/// the log's ERROR COUNT, so what a node says there can FAIL THE BUILD.
///
/// Across the editor's blueprint nodes those implementations record **27
/// errors, 31 warnings and 2 notes**: one hook, routinely, at three weights.
///
/// ⇒ the capability is not *a node may complain* — R1927 built that — it is
/// *a node may REFUSE*, and this round is the weight axis.
///
/// # The two measured defects this answers
///
/// * **The weight is in the CALL, not in the value.** Severity there is chosen
///   by which of three logging methods the implementation invoked; nothing
///   holds it afterwards, so a caller cannot ask *how bad is this node* — only
///   read a log. Here it is an [`Objection`], and
///   [`Document::may_run`] is a question anybody may ask without compiling.
/// * **The answer is a SIDE EFFECT.** The hook returns nothing and writes into
///   a log shared with everything else the compile said, so *is this node all
///   right?* is not a question that tree can put. Returning the objection makes
///   the per-node answer and the whole-graph verdict the same fact.
#[test]
fn engine_node_validate_node_during_compilation() {
    let mut chain = chain();

    // (A) ★★★★★ ALL THREE WEIGHTS, on three different kinds, so no assertion
    // here can pass by condemning one kind's rule.
    let sink = chain
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 0, 0)
        .expect("root tree");
    let relay = chain
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Relay), 40, 0)
        .expect("root tree");
    let double = chain
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Double), 80, 0)
        .expect("root tree");
    assert!(matches!(
        chain.document.warning(ROOT, sink).map(|w| w.objection),
        Some(Objection::Blocks(_))
    ));
    assert!(matches!(
        chain.document.warning(ROOT, relay).map(|w| w.objection),
        Some(Objection::Warns(_))
    ));
    assert!(matches!(
        chain.document.warning(ROOT, double).map(|w| w.objection),
        Some(Objection::Notes(_))
    ));

    // (B) ★★★★★ THE GATE: the blocking one stops the graph, and the other two
    // do not. Asserted as a VALUE, which is the half the reference cannot
    // answer without compiling and counting a log.
    assert!(
        !chain.document.may_run(ROOT),
        "a blocking objection stops it: {:?}",
        chain.document.objections(ROOT)
    );
    assert_eq!(
        chain
            .document
            .objections(ROOT)
            .into_iter()
            .map(|held| held.node)
            .collect::<Vec<_>>(),
        vec![sink],
        "★ and ONLY the blocking one is listed, so a refusal names what to fix"
    );

    // (C) ★ Every weight is still in the full list, so the blocking filter is
    // a view of one walk rather than a second opinion.
    assert_eq!(
        chain.document.warnings(ROOT).len(),
        3,
        "the three objections are all still reported: {:?}",
        chain.document.warnings(ROOT)
    );

    // (D) ★★★★★ AND THE GATE OPENS when the blocking objection is answered —
    // by changing the SITUATION, not the kind. Without this the verdict could
    // be a constant and everything above would still hold.
    chain
        .document
        .connect(ROOT, Socket::new(chain.add, 0), Socket::new(sink, 0))
        .expect("a number reaches the sink");
    assert!(
        chain.document.may_run(ROOT),
        "the sink is fed, so nothing blocks: {:?}",
        chain.document.objections(ROOT)
    );
    assert!(
        chain.document.warnings(ROOT).len() >= 2,
        "★ while the lighter objections are UNCHANGED — opening the gate is \
         not the same as silencing the list: {:?}",
        chain.document.warnings(ROOT)
    );

    // (E) ⚠ And it is NOT the structural check: a well-formed document that
    // blocks, and the two answers are independent.
    assert!(
        chain.document.validate().is_empty(),
        "the document was well formed throughout: {:?}",
        chain.document.validate()
    );
}

/// ★★★★★ R1940 — **what a node is drawn as, answered per NODE**, and derived
/// from what that node currently is.
///
/// # The measurement
///
/// The reference lets a node type supply an optional override of the CLASS its
/// header is drawn from, asked of the node rather than of the type. Three
/// implementations exist and **all three DERIVE**: one reads the colour tag of
/// the definition its group instance stands for, and two read the node's chosen
/// data type and answer *vector operation* or *colour operation* where they
/// would otherwise answer *converter*. None stores a colour — a person
/// authoring one is a separate, already-built axis here (`Appearance::tint`).
///
/// Two further measurements shaped what is built:
///
/// * **The fallback is a second declaration of the same fact.** A type
///   supplying the override ALSO declares a fixed class for when it is absent,
///   and both data-type implementations answer exactly that fixed class in
///   their own default branch — the same fact written twice, with **nothing in
///   that tree checking the two agree** (searched: no test, assertion or
///   validator relates them). Here a kind IS the node's state, so there is one
///   declaration and nothing to keep in step.
/// * **Both consumers carry their own copy of the choosing expression.** The
///   header-drawing code and the colour-tag query each spell *the override if
///   there is one, else the fixed class*, and the authored colour is a third
///   path again. Here [`Document::faces`] ranks all of it, once.
#[test]
fn dcc_node_ui_class() {
    let mut document: Document<Op> = Document::new("root");
    let constant = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(7)), 0, 0)
        .expect("root tree");
    let doubler = document
        .add_node(ROOT, NodeBody::Kind(Op::Double), 40, 0)
        .expect("root tree");
    let sink = document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 80, 0)
        .expect("root tree");

    // (A) A kind that says nothing is drawn as nothing — not as a black the
    // caller cannot tell from a chosen one.
    assert_eq!(Op::Double.drawn_as(), Drawn::Unstated);
    assert_eq!(
        document.faces(ROOT, doubler),
        None,
        "★ the neighbour declares nothing, so this proof cannot pass by \
         condemning the whole taxonomy"
    );

    // (B) A kind that names a TYPE is drawn in that type's colour — the same
    // colour a PORT of that type is drawn in, reached through one declaration.
    let number = <Op as NodeKind>::type_colour(&Ty::Number).expect("the fixture colours Number");
    assert_eq!(
        document.faces(ROOT, constant).map(|faces| faces.title),
        Some(number),
        "★★★★★ the node's header is the type's own colour, not a second \
         vocabulary that happens to agree"
    );
    // ★ And it IS the same declaration a port reads, driven rather than
    // asserted in prose: the constant's own output port answers the same.
    assert_eq!(
        document
            .port_palette(ROOT, constant, PortRef::output(0))
            .and_then(|palette| palette.own()),
        Some(number),
    );

    // (C) ★★★★★ THE CAPABILITY: the answer is per NODE, so R1937's retype
    // gesture recolours the node it retypes. Same kind family, different state,
    // different drawing — which is what "one appearance per kind" cannot do.
    document
        .set_kind(ROOT, constant, Op::Word("hi".to_owned()))
        .expect("a constant may be retyped");
    assert_eq!(
        document.faces(ROOT, constant),
        None,
        "★ now drawn like Text, which this taxonomy deliberately leaves \
         uncoloured — the kind SPOKE and the outcome is still nothing"
    );
    // ⚠ And that is a different STATEMENT from the silence in (A), which is
    // why the hook is asserted and not only the faces: a screen cannot tell
    // these apart by looking at the colour, and a model that collapsed them
    // would lose the difference entirely.
    assert_eq!(
        document
            .tree(ROOT)
            .and_then(|tree| tree.node(constant))
            .map(|held| match &held.body {
                NodeBody::Kind(kind) => kind.drawn_as(),
                _ => Drawn::Unstated,
            }),
        Some(Drawn::LikeType(Ty::Text)),
    );

    // (D) A kind may name a colour of its own, for a node that is not about a
    // type at all.
    assert_eq!(
        document.faces(ROOT, sink).map(|faces| faces.title),
        Some(Tint::rgb(0x33, 0x33, 0x3A)),
    );

    // (E) ★★★★★ AND WHAT A PERSON AUTHORED WINS, which is the ranking the
    // reference spreads across three code paths. Authored on the node whose
    // kind has the STRONGEST opinion, so this cannot pass by the kind having
    // said nothing.
    document
        .tree_mut(ROOT)
        .and_then(|tree| tree.node_mut(sink))
        .expect("the node is there")
        .appearance
        .tint = Some(Tint::rgb(0xE0, 0x10, 0x10));
    let faces = document.faces(ROOT, sink).expect("authored");
    assert_eq!(faces.title, Tint::rgb(0xE0, 0x10, 0x10));
    assert_eq!(
        faces.title_text,
        Tint::rgb(255, 255, 255),
        "★ and the other faces are still DERIVED from whichever colour won, so \
         one ranking feeds one derivation"
    );
    // ★ Clearing it hands the node back to its kind, rather than to nothing —
    // the half a one-way test would miss.
    document
        .tree_mut(ROOT)
        .and_then(|tree| tree.node_mut(sink))
        .expect("the node is there")
        .appearance
        .tint = None;
    assert_eq!(
        document.faces(ROOT, sink).map(|faces| faces.title),
        Some(Tint::rgb(0x33, 0x33, 0x3A)),
    );
}

/// ★★★★★ R1937 — **the VERB**: a person gives one port a type, and the node
/// becomes what its kind says it becomes.
///
/// The engine's editor command, whose own tooltip is *"Changes the type of this
/// pin (boolean, int, etc.)"* — a CHOICE on one pin, not a wildcard resolving
/// itself. What this crate adds is that the edit is the same edit as
/// `set_kind`, so what it costs is reported.
#[test]
fn engine_graph_editor_change_pin_type() {
    let mut chain = chain();
    let source = num(&mut chain.document, 7);
    let reader = node(&mut chain.document, Op::Double);
    wire(&mut chain.document, source, 0, reader, 0);
    assert_eq!(
        arrives(&chain.document, Socket::new(reader, 0)),
        Some(Val::Number(7))
    );

    // ★ The verb: that port, this type.
    let swapped = chain
        .document
        .set_port_type(ROOT, source, PortRef::output(0), &Ty::Text)
        .expect("a constant lets its output's type be chosen");

    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(source)
            .unwrap()
            .body,
        NodeBody::Kind(Op::Word(String::new())),
        "★ the node became what its kind said it becomes"
    );
    assert_eq!(
        chain.document.signature(ROOT, source).unwrap().outputs[0]
            .flow
            .value_type(),
        Some(&Ty::Text),
        "and the port carries the chosen type"
    );
    // ★★★★★ AND WHAT IT COST IS REPORTED. The wire into the reader could not
    // survive a Number->Text output, and it is NAMED — where the reference's
    // hook returns `void` and the node reconstructs, so nobody learns.
    assert_eq!(
        swapped.severed.len(),
        1,
        "the wire that could not cross is named: {swapped:?}"
    );
    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .links()
            .iter()
            .all(|link| link.from.node != source),
        "and the graph agrees with the report"
    );
    assert!(chain.document.validate().is_empty());

    // ⚠ The address is the node's OWN signature: an index past its ports is a
    // different refusal from a kind declining, because the repairs differ.
    assert_eq!(
        chain
            .document
            .set_port_type(ROOT, source, PortRef::output(9), &Ty::Number),
        Err(RetypeError::NoSuchPort {
            tree: ROOT,
            node: source,
            port: PortRef::output(9)
        })
    );
}

/// ★★★★★ R1937 — **the NODE's half**: the kind says what it becomes, and may
/// decline — before anything moves.
///
/// The engine's hook, and its shape is the finding. Measured across all seven
/// mentions: it is a `void` notification whose ONE external call site is the
/// pin's type-selector widget (`OwningNode->PinTypeChanged(GraphPinObj)`), the
/// rest being nodes calling it on themselves while reconstructing. Its own
/// comment says it is called when a pin's type "has had its' pin type changed
/// from an external source" — PAST TENSE, so a node hears about the change
/// after it happened and cannot refuse.
///
/// ★ Here the same declaration is asked FIRST, so declining is a refusal rather
/// than a reconstruction, and asking is separable from doing.
#[test]
fn engine_node_pin_type_changed() {
    let mut chain = chain();
    let source = num(&mut chain.document, 3);

    // ★ The DEFAULT is a refusal, and it is what every kind that has not opted
    // in says. Asserted on a kind that takes it, because a fixture where
    // everything opts in cannot see the default at all.
    assert!(
        !chain
            .document
            .may_set_port_type(ROOT, chain.add, PortRef::output(0), &Ty::Text),
        "★ `Add` never declared this hook, so its answer is the trait's default"
    );
    assert_eq!(
        chain
            .document
            .set_port_type(ROOT, chain.add, PortRef::output(0), &Ty::Text),
        Err(RetypeError::Refused {
            tree: ROOT,
            node: chain.add,
            port: PortRef::output(0)
        }),
        "and the verb refuses with the same declaration the question read"
    );

    // ★★★★★ CHOOSABLE IS NOT A PROPERTY OF THE PORT ALONE: the same port of the
    // same node accepts one type and declines another. The reference cannot ask
    // this at all — its hook is a notification, so the only way to find out is
    // to do it.
    assert!(
        chain
            .document
            .may_set_port_type(ROOT, source, PortRef::output(0), &Ty::Text)
    );
    assert!(
        !chain
            .document
            .may_set_port_type(ROOT, source, PortRef::output(0), &Ty::Pair),
        "★ a composite is declined by a kind that accepts the two atoms"
    );
    // ⚠ And the INPUT side of a kind that answers for its output does not: the
    // hook is asked about a port, not about a node.
    assert!(
        !chain
            .document
            .may_set_port_type(ROOT, chain.add, PortRef::input(0), &Ty::Number)
    );

    // ★ Asking and doing cannot disagree, because they read one declaration —
    // R1920's rule applied to a hook rather than to a permission.
    for ty in [Ty::Number, Ty::Text, Ty::Pair] {
        let asked = chain
            .document
            .may_set_port_type(ROOT, source, PortRef::output(0), &ty);
        let done = chain
            .document
            .set_port_type(ROOT, source, PortRef::output(0), &ty)
            .is_ok();
        assert_eq!(asked, done, "asking and doing agree for {ty:?}");
    }

    // ⚠ A body this crate owns has no kind to ask, and says so in its own word
    // rather than as a refusal by the kind — there is no kind.
    let bend = chain
        .document
        .add_node(ROOT, NodeBody::Reroute, 0, 0)
        .unwrap();
    assert_eq!(
        chain
            .document
            .set_port_type(ROOT, bend, PortRef::output(0), &Ty::Number),
        Err(RetypeError::NotAKind {
            tree: ROOT,
            node: bend
        })
    );
}

/// ★★★★★ R1936 — **make this node stand for that definition**, keeping the
/// wires it can, and KEEPING ITS IDENTITY.
///
/// The DCC's group-asset swap. ★ Measured on its operator rather than on its
/// name: it accepts any swappable node, not only a group instance, so becoming
/// a group and being re-pointed at another definition are one edit there — and
/// they are one verb here for the same reason.
///
/// ★★★★★ And re-pointing is NOT ungroup-then-nest, which is why it needs a verb
/// at all: that pair destroys the instance and makes another, so the `NodeId`
/// dies and with it every selection, layout, held reference and undo record
/// keyed by it. The identity assertion below is what makes this a swap.
#[test]
fn dcc_swap_group_asset() {
    let mut chain = chain();
    // Two definitions with the same face, so a re-point can carry every wire
    // and the assertion is about WHICH definition rather than about loss.
    let first = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let instance = first.node;
    let second = chain.document.group(ROOT, &[instance], "Again").unwrap();
    // Descend and take the inner instance, which stands for `first`.
    let inner = chain
        .document
        .tree(second.definition)
        .unwrap()
        .nodes()
        .find(|held| matches!(held.body, NodeBody::Group(inner) if inner == first.definition))
        .map(|held| held.id)
        .expect("the collapsed instance is inside the new definition");

    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    let third = chain.document.add_definition("Elsewhere");
    let swapped = chain
        .document
        .set_definition(second.definition, inner, third)
        .expect("an instance may be re-pointed");

    // ★★★★★ The identity survived: same node, different definition.
    let held = chain
        .document
        .tree(second.definition)
        .unwrap()
        .node(inner)
        .expect("★ the node is still there — this is a swap, not a replace");
    assert_eq!(held.body, NodeBody::Group(third));
    // ★ And what it cost is REPORTED rather than swallowed. The new definition
    // has an empty face, so every port went — and the report names each one,
    // every wire that touched it, and every authored value that was on it.
    assert!(
        !swapped.lossless(),
        "an empty face cannot answer the old ports: {swapped:?}"
    );
    assert_eq!(
        swapped.carried.len(),
        0,
        "nothing to carry across an empty face"
    );
    assert!(
        !swapped.dropped.is_empty(),
        "★ and the ports that went are NAMED — the reference drops them inside \
         three swallowed exceptions, so there 'the swap worked' and 'the swap \
         worked and cost you two wires' are the same outcome"
    );
    // ⚠ And the loss REACHES OUT: the outer graph fed from what that instance
    // produced, so emptying it empties the sink too. Asserted rather than
    // asserted away — the first draft of this test claimed the outer graph was
    // untouched, and it is not: a definition's face is the instance's
    // signature, so re-pointing one instance is visible everywhere its value
    // went. That is the fact a caller needs, and it is why `Swapped` names what
    // it cost instead of answering a bare "done".
    assert_eq!(before, Some(Val::Number(5)), "the fixture computed 2 + 3");
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)),
        None,
        "★ an empty definition produces nothing, and the graph says so"
    );

    // ★ The refusals are their own words. The root is the document, not a
    // definition; and a definition that would contain itself is the same guard
    // `nest` applies, asked here rather than re-derived.
    assert_eq!(
        chain
            .document
            .set_definition(second.definition, inner, ROOT),
        Err(SwapError::NotADefinition(ROOT))
    );
    let recursive = chain
        .document
        .set_definition(second.definition, inner, second.definition);
    assert!(
        matches!(recursive, Err(SwapError::Recursion { .. })),
        "a definition may not stand for the tree it is in: {recursive:?}"
    );
}

/// ★★★★★ R1936 — **make this node stand for a NEW, empty definition**, and
/// answer which one.
///
/// The DCC's empty-group swap, and measured it is exactly a composition: it
/// builds an empty group with an input end and an output end, calls its own
/// node swap, and then points the result at the group it made. Written as a
/// composition here too, so the two cannot disagree about what a swap is —
/// there they can, because the operator reaches back in and overwrites the
/// swapped node's tree afterwards.
#[test]
fn dcc_swap_empty_group() {
    let mut chain = chain();
    let before_trees = chain.document.tree_count();
    let wires_before = chain.document.tree(ROOT).unwrap().links().len();

    let (definition, swapped) = chain
        .document
        .set_new_definition(ROOT, chain.add, "Empty")
        .expect("a kind node becomes an empty group");

    assert_eq!(chain.document.tree_count(), before_trees + 1);
    assert!(
        chain
            .document
            .tree(definition)
            .unwrap()
            .interface()
            .is_empty(),
        "★ the definition it made is EMPTY, which is what the operator is named for"
    );
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .body,
        NodeBody::Group(definition),
        "★ and the node kept its id while becoming an instance of it"
    );
    // ★★★★★ Nothing could survive an empty face, and the report SAYS SO — three
    // wires gone, each named, where the reference loses them silently.
    assert!(swapped.carried.is_empty());
    assert_eq!(
        swapped.severed.len(),
        wires_before,
        "★ every wire on the node is named as severed: {swapped:?}"
    );
    assert_eq!(
        chain.document.tree(ROOT).unwrap().links().len(),
        0,
        "and the graph agrees with the report"
    );

    // ⚠ A REFUSED swap leaves no orphan definition behind. Measured by count,
    // because the tempting order — make the definition, then check — is one
    // early return away from littering the document with definitions nobody
    // asked for and nothing points at.
    let trees = chain.document.tree_count();
    let bend = chain
        .document
        .add_node(ROOT, NodeBody::Reroute, 0, 0)
        .unwrap();
    let refused = chain.document.set_new_definition(ROOT, bend, "Never");
    assert_eq!(
        refused,
        Err(SwapError::NotSwappable {
            tree: ROOT,
            node: bend
        }),
        "a body this crate owns is not the application's to overwrite"
    );
    assert_eq!(
        chain.document.tree_count(),
        trees,
        "★ and the refusal made no definition"
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

// ====================================== R1933 — what a tree will admit

/// ★★★★★ R1933 — a tree says which socket types it will admit, and the offer is
/// derived from the refusal.
///
/// The two references' two hooks are two READINGS of one fact — the DCC refuses
/// and offers, the engine only offers — so the three things asserted here are:
///
/// * a declared set REFUSES what it excludes, at the place a type arrives on an
///   interface;
/// * the OFFER is the same list, so a chooser cannot show what the edit would
///   turn away;
/// * and `Anything` is a value, distinct from a set that happens to be empty.
#[test]
fn dcc_node_tree_valid_socket_type() {
    let mut document: Document<Op> = Document::new("root");
    let definition = document.add_definition("typed");

    // ★ The supplied answer first: a tree that says nothing admits everything,
    // and it SAYS so rather than answering an empty offer.
    assert_eq!(document.admitted(definition), Admitted::Anything);
    assert!(document.admits_type(definition, &Ty::Text));
    assert_eq!(
        document.offered_types(definition),
        None,
        "a tree that restricts nothing has no list to offer — asking the \
         taxonomy is the caller's job, and an empty vector would be a lie"
    );
    document
        .expose(
            definition,
            InterfaceSide::Input,
            Port::new("free", Ty::Text),
        )
        .expect("anything goes before a restriction is declared");

    // ★★★★★ A DECLARED set refuses what it excludes, where the type arrives.
    document
        .set_admitted(definition, Admitted::These(vec![Ty::Number, Ty::Flag]))
        .expect("the tree is there");
    assert!(document.admits_type(definition, &Ty::Number));
    assert!(!document.admits_type(definition, &Ty::Text));
    let why = document
        .expose(
            definition,
            InterfaceSide::Input,
            Port::new("word", Ty::Text),
        )
        .expect_err("Text is not admitted");
    assert!(
        matches!(why, EditError::TypeNotAdmitted { tree, .. } if tree == definition),
        "and the refusal names the tree: {why}"
    );
    assert!(
        why.to_string().contains("Text"),
        "and says which type: {why}"
    );
    // ★ And an admitted one still goes through, so the rule is a filter rather
    // than a wall — without this the assertion above would hold for a tree that
    // refused everything.
    document
        .expose(
            definition,
            InterfaceSide::Input,
            Port::new("count", Ty::Number),
        )
        .expect("Number is admitted");

    // ★★★★★ THE OFFER IS THE SAME LIST. This is the whole of what the two
    // references have as two separate mechanisms.
    let offered = document
        .offered_types(definition)
        .expect("a restricted tree offers a list");
    assert_eq!(offered, vec![Ty::Number, Ty::Flag]);
    for ty in &offered {
        assert!(
            document.admits_type(definition, ty),
            "every offered type is one the edit would take: {ty:?}"
        );
    }
    assert!(
        !offered.contains(&Ty::Text),
        "and the one it refuses is not offered"
    );

    // ⚠ An EMPTY set is a real answer and a different one from `Anything`: this
    // tree admits nothing at all, and says so rather than reading as
    // unrestricted.
    let shut = document.add_definition("shut");
    document
        .set_admitted(shut, Admitted::These(Vec::new()))
        .expect("the tree is there");
    assert_eq!(document.offered_types(shut), Some(Vec::new()));
    assert!(!document.admits_type(shut, &Ty::Number));
    assert_ne!(document.admitted(shut), document.admitted(ROOT));

    // ★ A narrowing does not delete what is already there — it REPORTS it, so
    // whoever narrowed the set decides what to do about the links.
    let left = document.unadmitted_ports(definition);
    assert_eq!(
        left,
        vec![(InterfaceSide::Input, 0)],
        "the Text port exposed before the restriction is still there and named: {left:?}"
    );
    assert!(
        document.unadmitted_ports(ROOT).is_empty(),
        "and an unrestricted tree has none by construction"
    );
}

// ============================================== R1934 — a bend in a wire

/// ★★★★★ R1934 — **the DCC's `add_reroute`**: a gesture across a canvas puts a
/// reroute on every wire it crossed.
///
/// The operator's own four behaviours, measured from its source and asserted
/// here. The first is the one nobody would guess from the name:
///
/// * **one reroute per source socket, not per cut wire** — its comment says
///   "deduplicating new reroutes per output socket is useful because it allows
///   reusing reroutes for connected intersections";
/// * the cut links are **kept and re-pointed** (`link->fromnode = reroute`),
///   not deleted and remade;
/// * the feeding link is **muted exactly when every cut link was**;
/// * the reroute lands at the **average** of its own crossings.
///
/// And the property neither reference states because there it cannot be
/// stated — the graph still computes what it computed. The DCC has no
/// evaluator that could be asked in a test; the engine deletes the node before
/// compilation, so the question is answered by there being nothing left.
#[test]
fn dcc_add_reroute() {
    let mut chain = chain();
    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    assert_eq!(before, Some(Val::Number(5)), "the fixture computes 2 + 3");

    // A second reader of `add`, so the fan-out this deduplicates is real.
    let watch = node(&mut chain.document, Op::Double);
    wire(&mut chain.document, chain.add, 0, watch, 0);
    let cut: Vec<LinkId> = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .filter(|link| link.from == Socket::new(chain.add, 0))
        .map(|link| link.id)
        .collect();
    assert_eq!(cut.len(), 2, "two wires leave `add`");

    let made = chain
        .document
        .insert_reroutes(ROOT, &[(cut[0], 100, 40), (cut[1], 140, 60)])
        .expect("both wires were cut");

    // ★★★★★ ONE reroute, for two cut wires, because they share a source.
    assert_eq!(
        made.made.len(),
        1,
        "one reroute per SOURCE socket: {made:?}"
    );
    assert_eq!(made.feeds.len(), 1, "and one link feeding it");
    // ★ The cut links kept their identity.
    assert_eq!(made.rerouted, cut, "the cut links are the same links");

    let reroute = made.made[0];
    let held = chain.document.tree(ROOT).unwrap().node(reroute).unwrap();
    assert_eq!(held.body, NodeBody::Reroute);
    assert_eq!(
        (held.x, held.y),
        (120, 50),
        "★ it lands at the average of ITS OWN crossings"
    );

    // ★ Both readers now read the reroute, and `add` feeds only it.
    let links = chain.document.tree(ROOT).unwrap().links().to_vec();
    let from_add = links
        .iter()
        .filter(|link| link.from.node == chain.add)
        .count();
    let from_reroute = links
        .iter()
        .filter(|link| link.from.node == reroute)
        .count();
    assert_eq!(from_add, 1, "`add` feeds the reroute and nothing else");
    assert_eq!(from_reroute, 2, "and both readers hang off the reroute");

    // ★★★★★ AND THE GRAPH STILL COMPUTES WHAT IT COMPUTED.
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)),
        before,
        "a reroute is transparent to evaluation"
    );

    // ★ The reroute inherited what crosses it, rather than being authored.
    let signature = chain.document.signature(ROOT, reroute).unwrap();
    assert_eq!(signature.inputs.len(), 1);
    assert_eq!(signature.outputs.len(), 1);
    assert_eq!(
        signature.inputs[0].flow.value_type(),
        Some(&Ty::Number),
        "it carries what `add` gives"
    );

    // ⚠ A gesture that crossed nothing leaves nothing behind — including an
    // undo entry, which is why this is a refusal and not an empty answer.
    let nodes_before = chain.document.tree(ROOT).unwrap().node_count();
    assert!(chain.document.insert_reroutes(ROOT, &[]).is_err());
    assert_eq!(
        chain.document.tree(ROOT).unwrap().node_count(),
        nodes_before
    );
}

/// A row on one line: `src -> mid -> far -> end`, every card `WIDE` across,
/// with `mid` and `far` placed where the caller asks.
///
/// Positions are constructor arguments rather than a later mutation, so the
/// fixture reaches nothing but the public API — and the two the gaps are
/// measured from (`src`'s trailing edge, `far`'s leading edge) are the two that
/// vary.
fn row(mid_x: i32, far_x: i32) -> (Document<Op>, [NodeId; 4]) {
    let mut document: Document<Op> = Document::new("root");
    let src = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(4)), 0, 0)
        .expect("a source");
    let mid = document
        .add_node(ROOT, NodeBody::Kind(Op::Double), mid_x, 0)
        .expect("the card that arrived");
    let far = document
        .add_node(ROOT, NodeBody::Kind(Op::Double), far_x, 0)
        .expect("its consumer");
    let end = document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 900, 0)
        .expect("and the end of the row");
    wire(&mut document, src, 0, mid, 0);
    wire(&mut document, mid, 0, far, 0);
    wire(&mut document, far, 0, end, 0);
    (document, [src, mid, far, end])
}

/// Every card in [`row`] is this wide, so a gap is a number a reader of the
/// test can do in their head.
const WIDE: i32 = 100;

/// The clearance the rows below are judged against.
const CLEARANCE: i32 = 50;

/// ★★★★★ R1992 — the DCC's `insert_offset`: **the row moves apart to make room
/// for a card that arrived**, plus the splice that puts one there.
///
/// R1987 corrected this row's covering sentence after finding it equated with
/// the engine's autowire hook, which is a different gesture; what it left
/// absent is measured at the operator's own body — a `prev`, an `insert` and a
/// `next`, the gap either side against one margin, and a whole cone travelling
/// when the growing side is tight. Both halves are exercised here, because the
/// tree could previously splice only reroute bodies and a shove with nothing
/// inserted is not the capability.
///
/// The four assertions that would fail if the capability were missing:
///
/// 1. an arbitrary node is spliced onto a standing wire and **the graph still
///    computes**, through the node that arrived;
/// 2. the four verdicts are each **reachable**, at gaps chosen to select them;
/// 3. what moves on the growing side is the **whole cone**, not the one
///    neighbour;
/// 4. the report **names what moved and by how much** — the answer the
///    reference has no member for, which is why the census row could not be
///    closed by pointing at a bool.
///
/// A source feeding two consumers, so one output port holds **two** links —
/// which is what a whole-port operation needs and what a chain cannot give.
///
/// `two -> add.0`, `two -> mul.0`, `three -> add.1`. `add` is downstream of
/// `two`, which is what makes the mixed case below reachable: aiming `two`'s
/// links at `add`'s own output closes a loop for one of them and not the other.
struct FanOut {
    document: Document<Op>,
    two: NodeId,
    three: NodeId,
    add: NodeId,
    mul: NodeId,
}

fn fan_out() -> FanOut {
    let mut document = Document::new("root");
    let two = num(&mut document, 2);
    let three = num(&mut document, 3);
    let add = node(&mut document, Op::Add);
    let mul = node(&mut document, Op::Mul);
    wire(&mut document, two, 0, add, 0);
    wire(&mut document, two, 0, mul, 0);
    wire(&mut document, three, 0, add, 1);
    FanOut {
        document,
        two,
        three,
        add,
        mul,
    }
}

/// Which links each report says were taken, and which refused — read back as
/// ids so an assertion names links rather than counts alone.
fn split_report<E>(report: &Relocation<E>) -> (Vec<LinkId>, Vec<LinkId>) {
    let mut taken = Vec::new();
    let mut refused = Vec::new();
    for (id, how) in &report.links {
        if how.taken() {
            taken.push(*id);
        } else {
            refused.push(*id);
        }
    }
    (taken, refused)
}

/// ★★★★★ R1994 — the material editor's *Home*: **where the graph ends up**,
/// answered without moving a camera.
///
/// Measured at the reference's own body: the Home button calls `RecenterEditor`,
/// which takes the material graph's designated root node — or, for a material
/// function, walks its expressions backwards for output nodes, preferring the
/// one flagged last-previewed and otherwise the first found — and jumps the view
/// to it. ⚠ **With no such node it sets the view to the world origin** at the
/// current zoom, and it returns `void`.
///
/// The assertions that would fail if the capability were missing:
///
/// 1. the graph says where it ends up, and it is the node the flow arrives at;
/// 2. a graph that ends in more than one place **says so** rather than picking
///    silently — and says which the others are;
/// 3. a card nobody wired is an end in the trivial sense only, and the report
///    keeps that distinction instead of destroying it by filtering;
/// 4. each of the three ways there is no home is refused **by name**, where the
///    reference scrolls to the world origin and says nothing.
#[test]
fn engine_material_editor_camera_home() {
    // ── the ordinary graph ───────────────────────────────────────────
    let chain = chain();
    let home = chain.document.home(ROOT).expect("the chain ends somewhere");
    // ★ (1) `sink` is the one nothing leaves — the reference's root node is
    // exactly the node its material comes out of.
    assert_eq!(home.at, chain.sink, "the chain ends at its sink: {home:?}");
    assert!(home.sole(), "and it ends in exactly one place: {home:?}");
    assert!(
        home.others().is_empty(),
        "so there is nothing else to offer"
    );
    assert!(home.ends[0].fed, "the flow arrives there");

    // ── two ends, and a stray card ───────────────────────────────────
    let FanOut {
        mut document,
        add,
        mul,
        ..
    } = fan_out();
    let home = document.home(ROOT).expect("the fan-out ends somewhere");
    // ★ (2) TWO ends. The reference picks one by iteration order plus a preview
    // flag and never mentions the other.
    assert_eq!(
        home.ends.iter().map(|end| end.node).collect::<Vec<_>>(),
        vec![add.min(mul), add.max(mul)],
        "both consumers are ends, ascending: {home:?}"
    );
    assert!(!home.sole(), "so `at` was a CHOICE and the report says so");
    assert_eq!(
        home.others(),
        vec![add.max(mul)],
        "★ and the one it did not choose is offerable rather than lost: {home:?}"
    );

    // ★ (3) A card nobody wired. It IS an end — nothing leaves it — and saying
    // so is the point: a filtered list would hide the rule it was filtered by.
    let stray = node(&mut document, Op::Double);
    let home = document.home(ROOT).expect("still ends somewhere");
    let strayed = home
        .ends
        .iter()
        .find(|end| end.node == stray)
        .expect("the stray card is an end, trivially: {home:?}");
    assert!(!strayed.fed, "with nothing arriving at it");
    assert_ne!(
        home.at, stray,
        "★★★★★ and Home does NOT go there — a fed end is where the graph ends \
         UP, and a card someone dropped is not: {home:?}"
    );
    assert!(
        home.ends.iter().any(|end| end.fed),
        "the distinction is only worth keeping because both kinds are here"
    );

    // ── the three refusals ───────────────────────────────────────────
    // ★ (4a) A tree that is not here.
    assert!(matches!(
        document.home(TreeId(9_999)),
        Err(NoHome::NoSuchTree(_))
    ));
    // ★ (4b) Nothing to go to at all.
    let empty: Document<Op> = Document::new("root");
    assert!(matches!(empty.home(ROOT), Err(NoHome::Empty)));
    // ★★★★★ (4c) EVERY node feeds another, so nothing is an end. Reachable
    // only because this crate lets a cycle close through a delay — and it is
    // exactly where the reference scrolls a person to the world origin, which
    // is a place with nothing at it rather than an answer.
    let mut looped: Document<Op> = Document::new("root");
    let add = node(&mut looped, Op::Add);
    let delay = looped
        .add_node(ROOT, NodeBody::Delay(Ty::Number), 0, 0)
        .expect("a delay");
    wire(&mut looped, add, 0, delay, 0);
    looped
        .connect(ROOT, Socket::new(delay, 0), Socket::new(add, 0))
        .expect("a cycle broken by a delay is legal");
    assert_eq!(
        looped.home(ROOT),
        Err(NoHome::Endless { nodes: 2 }),
        "★★★★★ a graph that is all cycle has no end, and it is REFUSED by name \
         with the count — the reference goes to the origin and returns void"
    );
    // ★ The counterfactual for that refusal: the SAME graph with the closing
    // wire gone answers a home, so `Endless` is about the cycle and not about
    // delays or about two-node graphs.
    let mut cut = looped.clone();
    let closing = cut
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .find(|link| link.from.node == delay)
        .expect("the closing wire")
        .id;
    cut.disconnect(ROOT, closing).expect("take it out");
    assert_eq!(
        cut.home(ROOT).map(|home| home.at),
        Ok(delay),
        "with the loop open the delay is the end"
    );
}

/// ★★★★★ R1993 — a **moved** wire says what it replaced on arrival.
///
/// ⚠ **This exists because a counterfactual PASSED.** Blanking the move
/// report's `displaced` left every gate green, and the reason is the
/// population, not the assertion: a *producing* port holds many wires, so
/// moving onto one displaces nothing and the field is always `None` on that
/// side. An *accepting* port holds one. So the case was unreachable rather than
/// unasserted, and the repair is a fixture that reaches it — R1845's rule.
///
/// The engine's `MakeLinkTo` performs the same replacement and its single
/// response has no member that could name it.
fn a_moved_wire_names_what_it_displaced() {
    let FanOut {
        mut document,
        three,
        add,
        ..
    } = fan_out();
    // `add.0` is fed by `two` and `add.1` by `three`. Moving the accepting end
    // of `add.0`'s wire onto `add.1` therefore lands on a seat that is taken.
    let done = document
        .move_links(ROOT, Side::Input, Socket::new(add, 0), Socket::new(add, 1))
        .expect("both ports exist and differ");
    assert_eq!(done.taken(), 1, "one wire arrived at add.0: {done:?}");
    let Reception::Taken { displaced, .. } = &done.links[0].1 else {
        panic!("the seat takes it: {done:?}");
    };
    let gone = displaced.expect(
        "★★★★★ the accepting seat already held three's wire, and the report must \
         SAY the arriving wire replaced it -- an aggregate that dropped this \
         would be blinder than the single-wire verb it is built from",
    );
    assert_eq!(
        gone.from,
        Socket::new(three, 0),
        "and it names WHICH wire went: {done:?}"
    );
    assert_eq!(
        arrives(&document, Socket::new(add, 1)),
        Some(Val::Number(2)),
        "★ the graph agrees — add.1 is fed by two now"
    );
}

/// ★★★★★ R1993 — the engine's `MovePinLinks`: **every link on one port taken
/// to another**, and a refusal that does not cost a link.
///
/// Measured at the engine's implementation: it snapshots the from-pin's links,
/// **breaks every one of them**, and only then asks `CanCreateConnection` for
/// each against the target. Whatever the target refuses is therefore *already
/// gone*, and the single response it returns is overwritten by each failure so
/// it names only the last.
///
/// The assertions that would fail if the capability were missing:
///
/// 1. a port's links move as a set, and each moved link is **the same link** —
///    same id, so nothing keyed by it dangles;
/// 2. a link the target refuses **stays where it was**, which is the engine's
///    defect stated as a property;
/// 3. the report is **per link**, so a mixed outcome names which of them;
/// 4. asking is separable from doing, and answers the same thing.
#[test]
fn engine_schema_move_pin_links() {
    a_moved_wire_names_what_it_displaced();
    let FanOut {
        mut document,
        two,
        three,
        add,
        mul,
    } = fan_out();
    let out = |node: NodeId| Socket::new(node, 0);
    let held: Vec<LinkId> = document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .filter(|link| link.from == out(two))
        .map(|link| link.id)
        .collect();
    assert_eq!(held.len(), 2, "the fixture puts two links on one port");

    // ★ (4) Asked first, and it is the same answer.
    let asked = document
        .may_move_links(ROOT, Side::Output, out(two), out(add))
        .expect("both ports exist and differ");
    // ★ (3) MIXED, and mixed on purpose: `add` is downstream of `two`, so
    // aiming `two -> add.0` at `add`'s own output would close a loop, while
    // `two -> mul.0` becomes `add -> mul.0` and is fine.
    let (taken, refused) = split_report(&asked);
    assert_eq!(
        (taken.len(), refused.len()),
        (1, 1),
        "one of the two would be refused: {asked:?}"
    );
    assert!(!asked.complete(), "so the move is not complete");
    let doomed = refused[0];
    let survivor = taken[0];

    let before = document.clone();
    let done = document
        .move_links(ROOT, Side::Output, out(two), out(add))
        .expect("both ports exist and differ");
    assert_eq!(done, asked, "the verb did what the question said");
    assert_ne!(document, before, "and it did something");

    let links = document.tree(ROOT).unwrap().links().to_vec();
    let of = |id: LinkId| *links.iter().find(|l| l.id == id).expect("still a link");
    // ★ (1) The one that moved is the SAME link, on the new port.
    assert_eq!(
        of(survivor).from,
        out(add),
        "the moved link left the old port"
    );
    assert_eq!(
        of(survivor).to,
        Socket::new(mul, 0),
        "★ and it is the same link, under the id it always had -- the engine \
         breaks and re-makes, so its consumer would be holding a new one"
    );
    // ★★★★★ (2) THE ONE THE TARGET REFUSED IS STILL THERE, STILL ON `two`.
    assert_eq!(
        of(doomed).from,
        out(two),
        "★★★★★ a link the target would not take STAYS on the port it was on -- \
         the engine has already broken it by the time it finds out, and the \
         graph loses the edge"
    );
    assert_eq!(
        arrives(&document, Socket::new(add, 0)),
        Some(Val::Number(2)),
        "★ so the graph still computes through it"
    );

    // The caller's own errors, each reachable and each named.
    assert!(matches!(
        document.may_move_links(ROOT, Side::Output, out(two), out(two)),
        Err(RelocateError::SamePort(_))
    ));
    assert!(matches!(
        document.may_move_links(ROOT, Side::Output, out(two), Socket::new(three, 7)),
        Err(RelocateError::NoSuchPort { port: 7, .. })
    ));
    // ★ The counterfactual for the three refusals above: the SAME call shape,
    // on ports that are fine. Without it, "each is refused" would hold for a
    // vet that refused everything.
    assert!(
        document
            .may_move_links(ROOT, Side::Output, out(two), Socket::new(add, 0))
            .is_ok(),
        "an ordinary pair of ports is admitted"
    );
    // A port with nothing on it is an ANSWER, not a refusal -- and `complete`
    // is true for it, which is why `links` is published beside that flag.
    let empty = document
        .may_move_links(ROOT, Side::Output, Socket::new(mul, 0), out(three))
        .expect("mul has an output port");
    assert!(
        empty.links.is_empty() && empty.complete(),
        "nothing to move is complete and empty: {empty:?}"
    );
}

/// ★★★★★ R1993 — the engine's `CopyPinLinks`: **another port given a copy of
/// every link on this one**, with what each copy replaced named.
///
/// The engine walks the from-pin's links and calls `MakeLinkTo` on the target
/// for each the target admits, returning one response that the loop overwrites.
/// On a single-producer input `MakeLinkTo` breaks what was there, and nothing
/// in the response says so.
///
/// The assertions that would fail if the capability were missing:
///
/// 1. the source keeps every link it had — a copy is not a move;
/// 2. each copy is a **new** link, and the report names it;
/// 3. ★ each copy **names what it displaced**, which it must, because a value
///    input takes one producer and so a whole-port copy replaces on every
///    consumer.
#[test]
fn engine_schema_copy_pin_links() {
    let FanOut {
        mut document,
        two,
        three,
        add,
        mul,
    } = fan_out();
    let out = |node: NodeId| Socket::new(node, 0);
    let was: Vec<LinkId> = document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .filter(|link| link.from == out(two))
        .map(|link| link.id)
        .collect();

    let asked = document
        .may_copy_links(ROOT, Side::Output, out(two), out(three))
        .expect("both ports exist and differ");
    let untouched = document.clone();
    let done = document
        .copy_links(ROOT, Side::Output, out(two), out(three))
        .expect("both ports exist and differ");
    assert_eq!(done, asked, "the verb did what the question said");
    assert_ne!(document, untouched, "and it did something");
    assert!(
        done.complete(),
        "a number source feeds both consumers: {done:?}"
    );

    let (taken, _) = split_report(&done);
    assert_eq!(
        taken, was,
        "the report is keyed by the links that were there"
    );
    let made: Vec<LinkId> = done
        .links
        .iter()
        .map(|(_, how)| match how {
            // ★ (2)(3) The new link, and what it replaced.
            Reception::Taken { link, displaced } => {
                assert!(
                    displaced.is_some(),
                    "★★★★★ a value input takes ONE producer, so every copy \
                     REPLACED the original -- a report that did not say so \
                     would be describing an edit that silently deleted an edge"
                );
                *link
            }
            Reception::Refused(why) => panic!("nothing should refuse here: {why:?}"),
        })
        .collect();
    assert!(
        made.iter().all(|id| !was.contains(id)),
        "★ each copy is a NEW link: was {was:?}, made {made:?}"
    );

    // ★ (1) A copy is not a move, so what the source still feeds is the honest
    // question -- and here the answer is that the DISPLACEMENT took its links,
    // which is a property of the consuming side rather than of `copy_links`.
    let now = document.tree(ROOT).unwrap().links().to_vec();
    assert!(
        was.iter().all(|id| !now.iter().any(|l| l.id == *id)),
        "★ each original was displaced by its own copy, and the report SAID so \
         -- the engine's `MakeLinkTo` does the same and its response cannot"
    );
    assert_eq!(
        arrives(&document, Socket::new(add, 0)),
        Some(Val::Number(3)),
        "so both consumers are now fed by the port that was copied to"
    );
    assert_eq!(
        arrives(&document, Socket::new(mul, 0)),
        Some(Val::Number(3))
    );
}

/// The two halves are separate functions because they share no state — one
/// works on a computing chain, the other on a row of placed cards — and because
/// the pair together is over this workspace's hundred-line bound for one body.
#[test]
fn dcc_insert_offset() {
    a_card_is_spliced_onto_a_standing_wire();
    a_row_makes_room_for_what_arrived();
}

/// The first half: an arbitrary card goes onto a wire between two others, and
/// the value that crosses the canvas goes through it.
fn a_card_is_spliced_onto_a_standing_wire() {
    let mut chain = chain();
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)),
        Some(Val::Number(5)),
        "the fixture computes 2 + 3"
    );
    let standing = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .find(|link| link.from.node == chain.add && link.to.node == chain.sink)
        .expect("a wire from add to the sink")
        .id;
    let arriving = node(&mut chain.document, Op::Double);

    // Asked before anything moves, and the verb acts on this same answer.
    let asked = chain
        .document
        .may_insert_on_link(ROOT, standing, arriving)
        .expect("a doubler fits on a number wire");
    let done = chain
        .document
        .insert_on_link(ROOT, standing, arriving)
        .expect("and so the splice happens");
    assert_eq!(done.splice, asked, "the verb did what the question said");
    assert_eq!(
        done.kept, standing,
        "the standing link keeps its identity -- an undo has one thing to put back"
    );
    assert_ne!(done.fed, standing, "and the feeding link is a new one");
    assert_eq!(
        done.splice.between,
        (chain.add, chain.sink),
        "it says which two it went between"
    );
    // ★ (1) The capability, not the call: the value now arrives through the
    // card that was dropped on the wire.
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)),
        Some(Val::Number(10)),
        "2 + 3, doubled by the node that was spliced in"
    );

    // Refusals, each reachable, and each leaving the document untouched.
    assert!(matches!(
        chain
            .document
            .may_insert_on_link(ROOT, done.kept, chain.sink),
        Err(SpliceError::AlreadyAnEnd { .. })
    ));
    let source_only = node(&mut chain.document, Op::Num(1));
    let sink_only = node(&mut chain.document, Op::Sink);
    let spare = node(&mut chain.document, Op::Double);
    let baseline = chain.document.clone();

    let mut with_source = baseline.clone();
    assert!(
        matches!(
            with_source.insert_on_link(ROOT, done.kept, source_only),
            Err(SpliceError::NoIntake(_))
        ),
        "a node with no inputs has nothing to take the incoming wire"
    );
    assert_eq!(with_source, baseline, "a refused splice changed nothing");
    let mut with_sink = baseline.clone();
    assert!(
        matches!(
            with_sink.insert_on_link(ROOT, done.kept, sink_only),
            Err(SpliceError::NoOuttake(_))
        ),
        "and one with no outputs has nothing to give the outgoing one"
    );
    assert_eq!(with_sink, baseline, "either way round");
    // ★ The counterfactual for the two assertions above: the SAME comparison,
    // against a splice that was admitted. Without it "changed nothing" would
    // hold for a verb that did nothing at all.
    let mut admitted = baseline.clone();
    admitted
        .insert_on_link(ROOT, done.kept, spare)
        .expect("a doubler fits on a number wire");
    assert_ne!(
        admitted, baseline,
        "the same comparison does see a splice that happened"
    );
}

/// The second half: the row moves apart for a card that arrived in it, and the
/// report names what moved and why it did not when it did not.
fn a_row_makes_room_for_what_arrived() {
    // ★ (2) Each of the four verdicts, selected by the gaps alone. `mid` is
    // `WIDE` across, so `behind = mid_x - WIDE` and `ahead = far_x - mid_x -
    // WIDE`.
    // The canvas's own answer for where a card is drawn: here the model's own
    // position, `WIDE` across. A screen substitutes its painted box.
    let boxes = |node: &Node<Op>| Some(((node.x, node.y), Extent::new(WIDE, 40)));
    for (mid_x, far_x, behind, ahead, want) in [
        (160, 420, 60, 160, Verdict::Clear),
        (120, 400, 20, 180, Verdict::Shifted),
        (160, 290, 60, 30, Verdict::Shoved),
        (120, 240, 20, 20, Verdict::ShiftedAndShoved),
    ] {
        let (document, [_, mid, _, _]) = row(mid_x, far_x);
        let room = document
            .room_for(ROOT, mid, Widening::Rightward, CLEARANCE, boxes)
            .expect("the card is in a row");
        assert_eq!(
            (room.behind, room.ahead),
            (behind, ahead),
            "the gaps this case was chosen for"
        );
        assert_eq!(
            room.verdict, want,
            "gaps {behind}/{ahead} against {CLEARANCE}: {room:?}"
        );
        assert_eq!(
            room.verdict.moved(),
            room.shift != 0 || !room.shoved.is_empty(),
            "the arm and the numbers are one answer"
        );
    }

    // ★ (3)(4) The cone, and the report that names it. At 20/20 the inserted
    // card shifts by `CLEARANCE - 20` and the growing side by that again plus
    // its own shortfall.
    let (mut document, [src, mid, far, end]) = row(120, 240);
    let asked = document
        .room_for(ROOT, mid, Widening::Rightward, CLEARANCE, boxes)
        .expect("the card is in a row");
    let made = document
        .make_room_for(ROOT, mid, Widening::Rightward, CLEARANCE, boxes)
        .expect("and so the row makes room");
    assert_eq!(asked, made, "doing it reports what asking answered");
    assert_eq!(made.between, (src, far), "it names the two it measured");
    assert_eq!(made.shift, 30, "the card that arrived moves out of the gap");
    assert_eq!(
        made.shoved,
        vec![(far, 60), (end, 60)],
        "★ the WHOLE cone travels, not the one neighbour -- and the report \
         says which nodes and how far, which the reference answers as a bool"
    );
    assert_eq!(
        made.distance(mid),
        Some(30),
        "the inserted card is reachable through the same accessor"
    );
    assert_eq!(
        made.distance(src),
        None,
        "and the anchored side did not move"
    );
    let placed = |id: NodeId| document.tree(ROOT).unwrap().node(id).unwrap().x;
    assert_eq!(
        [placed(src), placed(mid), placed(far), placed(end)],
        [0, 150, 300, 960],
        "and the canvas holds exactly what was reported"
    );
    let after = document
        .room_for(ROOT, mid, Widening::Rightward, CLEARANCE, boxes)
        .expect("the card is still in a row");
    assert_eq!(
        after.verdict,
        Verdict::Clear,
        "★ once the room is made there is nothing left to do: {after:?}"
    );

    // The mirror reading moves the other cone, the other way.
    let (mut mirrored, [src, mid, _, _]) = row(120, 240);
    let left = mirrored
        .make_room_for(ROOT, mid, Widening::Leftward, CLEARANCE, boxes)
        .expect("a row may widen either way");
    assert_eq!(
        left.shoved,
        vec![(src, -60)],
        "leftward, the producing cone gives ground"
    );
    assert_eq!(left.shift, -30, "and the card moves with it");

    // A card with nothing on one side has no gap there to measure.
    let (document, [src, _, _, _]) = row(120, 240);
    assert!(matches!(
        document.room_for(ROOT, src, Widening::Rightward, CLEARANCE, boxes),
        Err(RoomError::NotInARow { producers: 0, .. })
    ));
}

/// ★★★★★ R1935 — **from a far end to the named endpoint it shows**, and the
/// value crossing the canvas with no edge between them.
///
/// The reference's operator navigates: it moves the selection to the one answer
/// and fits the view. Navigation is an affordance that only exists for a SINGLE
/// answer, which is why this reading is an `Option` — see its sibling below.
#[test]
fn engine_material_editor_select_named_reroute_declaration() {
    let mut chain = chain();
    let beacon = chain
        .document
        .add_node(ROOT, NodeBody::Beacon, 200, 0)
        .expect("a named endpoint");
    let echo = chain
        .document
        .add_node(ROOT, NodeBody::Echo(beacon), 600, 0)
        .expect("a far end of it");
    let wires_before = chain.document.tree(ROOT).unwrap().links().len();
    wire(&mut chain.document, chain.add, 0, beacon, 0);

    assert_eq!(
        chain.document.beacon_of(ROOT, echo),
        Some(beacon),
        "★ the far end names the endpoint it shows"
    );
    // ★★★★★ And the value got there over NO wire. The count is what makes this
    // the capability rather than an ordinary bend: one wire was added, the one
    // feeding the endpoint.
    assert_eq!(
        chain.document.tree(ROOT).unwrap().links().len(),
        wires_before + 1,
        "nothing joins the endpoint to its far end"
    );
    let mut evaluator = chain.document.evaluator();
    assert_eq!(
        evaluator.outputs(ROOT, echo),
        vec![Some(Val::Number(5))],
        "★ what `add` computed reaches the far end by NAME"
    );
    // The reading is honest about the two absences it can have: a node that is
    // not a far end, and one whose endpoint has gone.
    assert_eq!(chain.document.beacon_of(ROOT, chain.add), None);
    chain
        .document
        .remove_node(ROOT, beacon)
        .expect("an ordinary delete knows nothing of far ends");
    assert_eq!(chain.document.beacon_of(ROOT, echo), None);
    assert!(
        chain
            .document
            .validate()
            .iter()
            .any(|breach| breach.to_string().contains("endpoint is not there")),
        "★ and the standing check reports it, with no wire to have noticed it by"
    );
}

/// ★★★★★ R1935 — **the other direction, and NOT merely the same walk
/// reversed**: from the named endpoint to every far end of it.
///
/// The census row's own pre-R1935 reason — "the other direction of a named
/// reroute" — is what hid the finding. Measured on the reference, this operator
/// **clears** the selection and hands the list to the search-results panel,
/// because navigating is not a thing you can do to many nodes at once. So the
/// answer is a list where the other is at most one, and the shape IS the
/// capability.
#[test]
fn engine_material_editor_select_named_reroute_usages() {
    let mut chain = chain();
    let beacon = chain
        .document
        .add_node(ROOT, NodeBody::Beacon, 200, 0)
        .expect("a named endpoint");
    wire(&mut chain.document, chain.add, 0, beacon, 0);

    // A beacon nothing names yet answers an empty list — an ordinary state, not
    // a defect, since a name is useful the moment it exists.
    assert!(chain.document.echoes_of(ROOT, beacon).is_empty());
    assert!(chain.document.validate().is_empty());

    let mut made = Vec::new();
    for row in 0..3 {
        made.push(
            chain
                .document
                .add_node(ROOT, NodeBody::Echo(beacon), 600, row * 80)
                .expect("a far end"),
        );
    }
    made.sort_unstable();
    assert_eq!(
        chain.document.echoes_of(ROOT, beacon),
        made,
        "★ MANY, ascending — the list an editor hands to a search panel"
    );
    // ★ Each of the three carries the value, and none of them is wired to the
    // endpoint: three answers reached over zero edges.
    let joined = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .filter(|link| link.from.node == beacon)
        .count();
    assert_eq!(joined, 0, "nothing leaves the endpoint by wire");
    let mut evaluator = chain.document.evaluator();
    for &echo in &made {
        assert_eq!(evaluator.outputs(ROOT, echo), vec![Some(Val::Number(5))]);
    }
    // ⚠ And the question is asked of the right half: a far end is not an
    // endpoint, so it names none of its own.
    assert!(chain.document.echoes_of(ROOT, made[0]).is_empty());
}

/// ★★★★★ R1935 — **giving a bend a name is a FAN-OUT**, not a rename: one far
/// end per outgoing wire.
///
/// All four measured behaviours of the reference's operator: one far end per
/// wire, the wires **kept and re-pointed** rather than remade, the far ends
/// stacked by the drawn Y of the node each one feeds, and the endpoint placed
/// one offset to the other side of where the bend was.
#[test]
fn engine_material_editor_convert_reroute_to_named_reroute() {
    let mut chain = chain();
    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    // A second reader, so the fan-out has something to fan.
    let watch = node(&mut chain.document, Op::Double);
    // The second reader is drawn LOW and the sink stays HIGH, so an answer in
    // creation order and an answer in drawn order are distinguishable.
    chain
        .document
        .translate(ROOT, watch, 0, 900)
        .expect("the second reader moves down the canvas");
    let cut: Vec<LinkId> = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .filter(|link| link.from == Socket::new(chain.add, 0))
        .map(|link| link.id)
        .collect();
    wire(&mut chain.document, chain.add, 0, watch, 0);
    let made = chain
        .document
        .insert_reroutes(ROOT, &[(cut[0], 300, 100)])
        .expect("a wire was cut");
    let bend = made.made[0];
    wire(&mut chain.document, bend, 0, watch, 0);

    let wires_before: Vec<LinkId> = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .map(|link| link.id)
        .collect();
    let spread = chain
        .document
        .spread_reroute(ROOT, bend)
        .expect("a bend takes a name");

    assert_eq!(
        spread.echoes.len(),
        2,
        "★ one far end per outgoing wire: {spread:?}"
    );
    let wires_after: Vec<LinkId> = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .map(|link| link.id)
        .collect();
    assert_eq!(
        wires_after, wires_before,
        "★★★★★ every wire KEPT and re-pointed, none remade — so a caller \
         holding a link id still holds the same link and an undo has one thing \
         to put back"
    );
    // ★ The stack is ordered by the DOCUMENT — the drawn Y of each consumer —
    // and the sink was moved above the second reader, so it comes first.
    let feeds = |echo: NodeId| {
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .links()
            .iter()
            .find(|link| link.from.node == echo)
            .map(|link| link.to.node)
    };
    assert_eq!(
        feeds(spread.echoes[0]),
        Some(chain.sink),
        "★ the first far end feeds the node drawn highest"
    );
    assert_eq!(feeds(spread.echoes[1]), Some(watch));
    // ★★★★★ AND THE GRAPH STILL COMPUTES WHAT IT COMPUTED — over a canvas the
    // value now crosses by name.
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)),
        before,
        "a named pair is transparent to evaluation"
    );
    assert!(chain.document.validate().is_empty());
    // ⚠ And the refusal is its own word: a bend is what this takes, so handing
    // it the endpoint it just made says *convert the other way* rather than
    // saying nothing.
    assert!(chain.document.spread_reroute(ROOT, spread.beacon).is_err());
}

/// ★★★★★ R1935 — **taking the name away**: the endpoint and every far end fold
/// back into one bend, and the fold accepts EITHER half.
///
/// ★ Accepting a far end is worth reproducing rather than tidying away: the far
/// ends are the halves scattered across the canvas, and requiring the endpoint
/// would mean finding it first — the very thing the name exists to avoid.
#[test]
fn engine_material_editor_convert_named_reroute_to_reroute() {
    let mut chain = chain();
    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    let cut: Vec<LinkId> = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .filter(|link| link.from == Socket::new(chain.add, 0))
        .map(|link| link.id)
        .collect();
    let made = chain
        .document
        .insert_reroutes(ROOT, &[(cut[0], 300, 100)])
        .expect("a wire was cut");
    let bend = made.made[0];
    let placed = chain.document.tree(ROOT).unwrap().node(bend).unwrap();
    let (was_x, was_y) = (placed.x, placed.y);

    let spread = chain
        .document
        .spread_reroute(ROOT, bend)
        .expect("a bend takes a name");
    // ★ Folded from the FAR END, which the reference accepts and which is the
    // half a person is likely to be looking at.
    let gathered = chain
        .document
        .gather_beacon(ROOT, spread.echoes[0])
        .expect("a far end folds the pair it belongs to");

    let mut expected = vec![spread.beacon];
    expected.extend(spread.echoes.iter().copied());
    expected.sort_unstable();
    assert_eq!(
        gathered.gone, expected,
        "★ the endpoint and every far end of it"
    );
    let back = chain
        .document
        .tree(ROOT)
        .unwrap()
        .node(gathered.reroute)
        .unwrap();
    assert_eq!(
        (back.x, back.y),
        (was_x, was_y),
        "★★★★★ the round trip put the bend back where it began — the two \
         offsets are equal, which is what stops a conversion pair from walking \
         a node across the canvas"
    );
    assert_eq!(back.body, NodeBody::Reroute, "and it is a plain bend again");
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)),
        before,
        "★ and the graph still computes what it computed"
    );
    assert!(chain.document.validate().is_empty());
    // ⚠ The two refusals are separate words because the repairs are opposite:
    // this is now a bend, so folding it is the wrong direction.
    assert!(
        chain
            .document
            .gather_beacon(ROOT, gathered.reroute)
            .is_err()
    );
}

/// ★★★★★ R1934 — **the engine's `ShouldDrawNodeAsControlPointOnly`**: this node
/// is a point a wire passes through, and these are its two ends.
///
/// ★ The hook's name says *draw* and **not one of its seven call sites draws
/// anything**, measured across every one of them: three pick which END of the
/// point a drag should take, one spreads a hover along the chain, one keeps the
/// point's pins out of node alignment, and one asserts it as a precondition. So
/// what is reproduced here is the capability, not the name.
///
/// Two sources answer, and both are asserted, because the reference needs both:
/// of its three overriders, two are its reroute classes and the third is a
/// dataflow node class that answers by asking which node it is holding.
#[test]
fn engine_node_should_draw_node_as_control_point_only() {
    let mut chain = chain();

    // ★ An ordinary node is not one, and neither is a frame — the default is a
    // real answer rather than an absence of one.
    assert_eq!(chain.document.passing(ROOT, chain.add), None);
    let frame = chain
        .document
        .add_node(ROOT, NodeBody::Frame, 0, 0)
        .expect("a frame");
    assert_eq!(chain.document.passing(ROOT, frame), None);

    // ★★★★★ A reroute always is, and says which port is the way in.
    let cut = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .find(|link| link.from.node == chain.add)
        .unwrap()
        .id;
    let reroute = chain
        .document
        .insert_reroutes(ROOT, &[(cut, 10, 10)])
        .expect("the wire was cut")
        .made[0];
    let passing = chain
        .document
        .passing(ROOT, reroute)
        .expect("a reroute is a point on a wire");
    assert_eq!(passing.inbound, 0);
    assert_eq!(passing.outbound, 0);

    // ★ `end` is the question every one of the reference's drag sites asks by
    // hand, and it answers for both sides.
    assert_eq!(passing.end(Side::Input), passing.inbound);
    assert_eq!(passing.end(Side::Output), passing.outbound);

    // ★ And the two ends it names are real ports, which is what makes the
    // answer usable — the reference's own control-point widget asserts exactly
    // this in its constructor.
    let signature = chain.document.signature(ROOT, reroute).unwrap();
    assert!(signature.inputs.get(passing.inbound as usize).is_some());
    assert!(signature.outputs.get(passing.outbound as usize).is_some());

    // ★★★★★ An application KIND may also declare itself a point on a wire —
    // the third overrider's case, which no editor-side taxonomy could answer
    // for. `Op::Relay` is that kind in this fixture.
    let relay = node(&mut chain.document, Op::Relay);
    assert_eq!(
        chain.document.passing(ROOT, relay),
        Some(Passing::ENDS),
        "a kind that declares it is one"
    );
    assert_eq!(
        chain.document.passing(ROOT, chain.two),
        None,
        "and a kind of the same taxonomy that does not, is not"
    );
}

// =========================================== R1932 — what a name must be

/// ★★★★★ R1932 — a kind says where its name has to be unique, or that it need
/// not be.
///
/// The reference's node-side hook, and the three ways this passes it:
///
/// * **the off position is a value**, so a body whose name is a caption is free
///   without anybody building an accept-everything validator for it;
/// * **the scope is a type**, so two kinds wanting one reach state it once;
/// * **a refusal still NAMES the holder**, which the reference's bare enum
///   result cannot.
///
/// ⚠ And the supplied answer is under test, because most of this taxonomy does
/// not override — R1926's rule, which R1928 met again.
#[test]
fn engine_node_make_name_validator() {
    // ★ The census sentence said a label is free text. It is not, and has not
    // been since R1682 — this is that rule, still standing, and it is asserted
    // FIRST because everything below is a change to its reach.
    let mut document = Document::new("root");
    let one = node(&mut document, Op::Add);
    let two = node(&mut document, Op::Mul);
    document.relabel(ROOT, one, Some("adder")).expect("a name");
    assert!(
        matches!(
            document.relabel(ROOT, two, Some("adder")),
            Err(EditError::LabelTaken { held_by, .. }) if held_by == one
        ),
        "a taken name is refused, and the refusal NAMES the node holding it"
    );
    assert!(matches!(
        document.relabel(ROOT, two, Some("   ")),
        Err(EditError::LabelEmpty { .. })
    ));

    // ★★★★★ FREE — a frame's caption is not an address, so two frames may share
    // one. The reference spends a dummy validator per commenting class on this;
    // here it is what the body IS.
    let frame_a = document
        .add_node(ROOT, NodeBody::Frame, 0, 0)
        .expect("the root tree exists");
    let frame_b = document
        .add_node(ROOT, NodeBody::Frame, 40, 0)
        .expect("the root tree exists");
    assert_eq!(
        document.naming(ROOT, frame_a),
        pinion_node_graph::Naming::Free
    );
    document
        .relabel(ROOT, frame_a, Some("host"))
        .expect("a caption");
    document
        .relabel(ROOT, frame_b, Some("host"))
        .expect("★★★★★ and a second frame may share it");
    assert_eq!(
        document.nodes_labelled(ROOT, "host").len(),
        2,
        "both hold it, which is what Free means"
    );
    // ⚠ And the crate SAYS so rather than hiding it: a name two nodes answer to
    // does not identify one, and the by-name lookup reports that.
    assert_eq!(document.node_labelled(ROOT, "host"), None);

    // ★ A KIND keeps the supplied scope, so the ordinary case is under test.
    assert_eq!(
        document.naming(ROOT, one),
        pinion_node_graph::Naming::InTree
    );
    assert!(
        document.relabel(ROOT, two, Some("adder")).is_err(),
        "a node is still held to its tree"
    );

    // ★★★★★ IN DOCUMENT — the reference's commonest positive answer, and the
    // one this crate could not express. Asserted through the SEARCH rather than
    // through a taxonomy that declares it, because the fixture's kinds keep the
    // default: what has to exist is a reach that spans trees and can be asked.
    let inner = document.add_definition("inner");
    let deep = document
        .add_node(inner, NodeBody::Kind(Op::Double), 0, 0)
        .expect("the new tree exists");
    document
        .relabel(inner, deep, Some("adder"))
        .expect("another tree may hold it, because the kinds here are scoped to a tree");
    let everywhere = document.nodes_labelled_anywhere("adder");
    assert_eq!(
        everywhere.len(),
        2,
        "the document-wide search finds both: {everywhere:?}"
    );
    assert!(everywhere.contains(&(ROOT, one)) && everywhere.contains(&(inner, deep)));
    assert_eq!(
        document.nodes_labelled(ROOT, "adder"),
        vec![one],
        "while the tree-wide search finds one — the two scopes are different questions"
    );
}

// ============================================ R1930 — landing on a body

/// ★★★★★ R1930 — a wire released on a node's BODY: an existing port takes it, or
/// the node grows one and it lands there, in one act.
///
/// The reference's pair, and the three ways this passes it are asserted rather
/// than claimed:
///
/// * **one act, and a refusal changes nothing** — asserted over the WHOLE
///   document, not over a port count, because the reference's own consumer
///   leaves a pin behind when the connection it then attempts is refused;
/// * **the question answers what WOULD happen, as a type** — `Takes` and
///   `Grows` are different arms, where the reference has one bool and a string;
/// * **the question is the first half of the act** — what `may_land` says is
///   what `land` does, over every case here.
#[test]
fn engine_schema_drop_pin_on_node() {
    a_free_port_takes_the_end(&mut chain());
    a_full_node_grows_a_port_for_it(&mut chain());
    a_grown_port_lands_where_the_run_puts_it(&mut chain());
    a_refusal_leaves_the_document_as_it_was(&mut chain());
    a_node_that_cannot_grow_says_so(&mut chain());
}

/// ★ The ORDINARY case: a port that is already free takes the end, and no port
/// appears. Growing one here would litter a node every time a wire is re-aimed.
fn a_free_port_takes_the_end(chain: &mut Chain) {
    let choose = node(&mut chain.document, Op::Choose);
    let link = chain
        .document
        .tree(ROOT)
        .and_then(|tree| tree.links().last().map(|held| held.id))
        .expect("the chain has links");
    let was = chain
        .document
        .signature(ROOT, choose)
        .expect("a signature")
        .inputs
        .len();

    let fall = chain
        .document
        .may_land(ROOT, link, Side::Input, choose)
        .expect("a node with free ports takes it");
    assert_eq!(fall, Landfall::Takes(Socket::new(choose, 0)));
    assert!(!fall.is_new(), "nothing has to appear for this");

    let done = chain
        .document
        .land(ROOT, link, Side::Input, choose, Item::plain())
        .expect("and the act agrees with the question");
    assert_eq!(done.fall, fall, "the question IS the first half of the act");
    assert_eq!(done.relinked.now, Socket::new(choose, 0));
    assert_eq!(
        chain
            .document
            .signature(ROOT, choose)
            .expect("a signature")
            .inputs
            .len(),
        was,
        "and the node has exactly the ports it had"
    );
}

/// ★★★★★ Every port taken, so the run has to grow one — the capability the
/// census row names, reached through the public API only.
fn a_full_node_grows_a_port_for_it(chain: &mut Chain) {
    let choose = node(&mut chain.document, Op::Choose);
    // `Choose` opens with two option ports and one fixed `Index`. Fill every
    // one of them, so the only way to take another end is to grow.
    let filled = fill_every_input(&mut chain.document, choose);
    assert!(filled >= 3, "the fixture filled {filled} port(s)");
    let link = wire_from_a_fresh_source(&mut chain.document, chain.sink);

    let fall = chain
        .document
        .may_land(ROOT, link, Side::Input, choose)
        .expect("the run may grow");
    assert!(fall.is_new(), "a full node has to grow one: {fall:?}");
    let grown = fall.socket();

    let before = chain.document.clone();
    let done = chain
        .document
        .land(
            ROOT,
            link,
            Side::Input,
            choose,
            Item::plain().named("extra"),
        )
        .expect("and the act does it");
    assert_eq!(done.fall, fall, "the question IS the first half of the act");
    assert_eq!(done.relinked.now, grown, "the end is on the port that grew");
    assert_ne!(chain.document, before, "and the document did change");
    assert_eq!(
        chain
            .document
            .port_label(ROOT, choose, PortRef::input(grown.port)),
        Some(pinion_node_graph::Labelled {
            text: Some("extra".to_owned()),
            source: pinion_node_graph::NameSource::Item,
        }),
        "the item the caller described is the one that grew"
    );
}

/// ★★★★★ The grown port sits where the RUN puts it, not at the end of the list.
///
/// `Blend` declares two fixed inputs with its run spliced BETWEEN them and two
/// ports per item, so the socket is `start + ordinal * stride` and a landing
/// that dropped either half of that arithmetic would still be right on every
/// other kind in this taxonomy — R1928 measured exactly that trap on the naming
/// axis, and this is the same arithmetic reached by a different verb.
fn a_grown_port_lands_where_the_run_puts_it(chain: &mut Chain) {
    let blend = node(&mut chain.document, Op::Blend);
    fill_every_input(&mut chain.document, blend);
    let link = wire_from_a_fresh_source(&mut chain.document, chain.sink);

    let fall = chain
        .document
        .may_land(ROOT, link, Side::Input, blend)
        .expect("the run may grow");
    // start 1, one item already there, stride 2 -> the second item's first port.
    assert_eq!(
        fall,
        Landfall::Grows(Socket::new(blend, 3)),
        "the run starts at 1 and each item is two ports wide: {fall:?}"
    );
    let done = chain
        .document
        .land(ROOT, link, Side::Input, blend, Item::plain())
        .expect("and it lands");
    assert_eq!(done.relinked.now, Socket::new(blend, 3));
    // ⚠ And the fixed port the insert displaced kept its wire, which is what
    // makes this a splice rather than an append.
    assert!(
        chain
            .document
            .tree(ROOT)
            .expect("the root tree")
            .links()
            .iter()
            .any(|held| held.to == Socket::new(blend, 5)),
        "the port past the run moved and took its link with it"
    );
}

/// ★★★★★ A refusal leaves the document EQUAL to what it was — asserted whole,
/// because the reference's own consumer leaves the pin it made behind when the
/// connection it then attempts is refused.
fn a_refusal_leaves_the_document_as_it_was(chain: &mut Chain) {
    let shout = node(&mut chain.document, Op::Shout);
    let word = node(&mut chain.document, Op::Word("hi".to_owned()));
    wire(&mut chain.document, word, 0, shout, 0);
    let text = chain
        .document
        .tree(ROOT)
        .and_then(|tree| tree.links().last().map(|held| held.id))
        .expect("the text wire");
    let choose = node(&mut chain.document, Op::Choose);

    let before = chain.document.clone();
    let why = chain
        .document
        .land(ROOT, text, Side::Input, choose, Item::plain())
        .expect_err("a text wire cannot land on a number run");
    assert!(
        matches!(why, LandError::Refused(_)),
        "and the refusal is the wire's own, carried whole: {why}"
    );
    assert_eq!(
        chain.document, before,
        "★★★★★ the whole document is what it was — no port was grown and left"
    );
    // ⚠ And the question answered the same refusal, so nothing was learned by
    // trying that asking would not have said.
    assert!(
        chain
            .document
            .may_land(ROOT, text, Side::Input, choose)
            .is_err()
    );
}

/// ★ A node with no room and no run says so in its own arm, which is a different
/// problem from a refused wire and is fixed by a different action.
fn a_node_that_cannot_grow_says_so(chain: &mut Chain) {
    let double = node(&mut chain.document, Op::Double);
    fill_every_input(&mut chain.document, double);
    let link = wire_from_a_fresh_source(&mut chain.document, chain.sink);

    let why = chain
        .document
        .may_land(ROOT, link, Side::Input, double)
        .expect_err("a full node with no run has nowhere to put it");
    assert_eq!(
        why,
        LandError::NoRoom {
            node: double,
            side: Side::Input
        }
    );
    assert!(
        why.to_string().contains("cannot grow one"),
        "and it says so in words: {why}"
    );
    // ⚠ A node that is not there is a THIRD answer, not this one.
    assert!(matches!(
        chain
            .document
            .may_land(ROOT, link, Side::Input, NodeId(9999))
            .expect_err("no such node"),
        LandError::NoSuchNode { .. }
    ));
}

// ============================================ R1980 — which seat an end takes

/// ★★★★★ R1980 — **a kind says WHERE an arriving end berths**, while the
/// document keeps deciding what is legal.
///
/// # The census row, and why its covering sentence was false in both clauses
///
/// The row reads *a kind cannot intervene when a link is dropped on it; connect
/// decides centrally from the signature and the conversion relation*. Measured
/// at R1979.1 across the reference hook's header, its three consumers and all
/// 41 sites that install it — re-driven at R1980, where 41 = 15 sharing one
/// `return true`, 1 bridging to a script-defined function, and 25 doing the
/// work below:
///
/// * a kind CAN intervene — [`NodeKind::admits`] (R1885) refuses a pair, and
///   [`NodeKind::variadic`] + [`Document::land`] (R1930) grow a port for one;
/// * `connect` is not the whole of the decision either — `vet` asks admission,
///   crossing, cycles and multiplicity.
///
/// What those 25 per-node implementations actually do is one thing: an
/// end that touched this node's open end grows a port from the far end's type
/// and **moves onto it**. Every part of that was here except **which socket**,
/// which [`Document::free_port_for`] decided alone. This is that part.
///
/// # What is asserted
///
/// The first two are the SAME act on two kinds that differ in exactly one
/// declaration; the third holds them to that; and the last two hold the
/// division of labour — a preference does not make an illegal landing legal,
/// and a refused one leaves the document alone.
#[test]
fn dcc_node_insert_link() {
    a_kind_that_says_nothing_takes_the_free_seat(&mut seats());
    a_kind_that_wants_its_own_seat_grows_one(&mut seats());
    the_two_kinds_differ_in_one_declaration(&seats());
    a_preference_does_not_make_an_illegal_landing_legal(&mut seats());
    a_preference_is_not_a_promise_of_a_seat(&mut seats());
    a_structural_body_has_no_preference_to_state(&mut seats());
}

/// Two nodes whose ports are the same shape and whose kinds differ in one
/// declaration, plus a wire to re-aim at either of them.
struct Seats {
    document: Document<Op>,
    /// Declares nothing: [`Berth::Earliest`] by default.
    bundle: NodeId,
    /// Declares [`Berth::Fresh`].
    roster: NodeId,
    /// A drawn number wire, standing somewhere else.
    link: LinkId,
    /// A drawn TEXT wire, which no number seat may take.
    text: LinkId,
    /// A group instance — a body with no kind to ask.
    group: NodeId,
}

fn seats() -> Seats {
    let mut document = Document::new("root");
    let two = num(&mut document, 2);
    let sink = node(&mut document, Op::Sink);
    wire(&mut document, two, 0, sink, 0);
    let link = document
        .tree(ROOT)
        .and_then(|tree| tree.links().last().map(|held| held.id))
        .expect("the number wire");
    let word = node(&mut document, Op::Word("hi".to_owned()));
    let shout = node(&mut document, Op::Shout);
    wire(&mut document, word, 0, shout, 0);
    let text = document
        .tree(ROOT)
        .and_then(|tree| tree.links().last().map(|held| held.id))
        .expect("the text wire");
    let bundle = node(&mut document, Op::Bundle);
    let roster = node(&mut document, Op::Roster);
    let definition = document.add_definition("seatless");
    let group = document
        .add_node(ROOT, NodeBody::Group(definition), 0, 0)
        .expect("an instance of it");
    Seats {
        document,
        bundle,
        roster,
        link,
        text,
        group,
    }
}

fn seat_count(document: &Document<Op>, node: NodeId) -> usize {
    document
        .signature(ROOT, node)
        .map_or(0, |signature| signature.inputs.len())
}

/// ★ The kind that says nothing gets what every node got before this could be
/// asked: the free seat takes the end and no seat appears.
fn a_kind_that_says_nothing_takes_the_free_seat(seats: &mut Seats) {
    let was = seat_count(&seats.document, seats.bundle);
    let fall = seats
        .document
        .may_land(ROOT, seats.link, Side::Input, seats.bundle)
        .expect("a number wire lands on a number run");
    assert_eq!(
        fall,
        Landfall::Takes(Socket::new(seats.bundle, 0)),
        "the free seat takes it, which is the default and the answer a person expects"
    );
    let landed = seats
        .document
        .land(ROOT, seats.link, Side::Input, seats.bundle, Item::plain())
        .expect("and the act goes through");
    assert_eq!(
        landed.fall, fall,
        "the question IS the first half of the act"
    );
    assert_eq!(
        seat_count(&seats.document, seats.bundle),
        was,
        "and no seat appeared"
    );
}

/// ★★★★★ The capability the row names: the SAME wire on the SAME free seat, and
/// this kind gets a seat of its own instead.
fn a_kind_that_wants_its_own_seat_grows_one(seats: &mut Seats) {
    let was = seat_count(&seats.document, seats.roster);
    assert!(
        seats
            .document
            .occupants(ROOT, Socket::new(seats.roster, 0), Side::Input)
            .is_free(),
        "seat 0 is FREE — so what follows is the kind's doing and not the seat's"
    );
    let fall = seats
        .document
        .may_land(ROOT, seats.link, Side::Input, seats.roster)
        .expect("a number wire lands on a number run");
    assert_eq!(
        fall,
        Landfall::Grows(Socket::new(seats.roster, 1)),
        "★★★★★ the free seat is left alone and the node grows one"
    );
    let landed = seats
        .document
        .land(ROOT, seats.link, Side::Input, seats.roster, Item::plain())
        .expect("and the act goes through");
    assert_eq!(
        landed.fall, fall,
        "the question IS the first half of the act"
    );
    assert_eq!(
        seat_count(&seats.document, seats.roster),
        was + 1,
        "and the seat it named is the one that appeared"
    );
    assert_eq!(landed.relinked.now, Socket::new(seats.roster, 1));
}

/// ★★★★★ The two kinds differ in ONE declaration, so neither assertion above can
/// be reading anything else about them.
fn the_two_kinds_differ_in_one_declaration(seats: &Seats) {
    assert_eq!(
        seats.document.berth(ROOT, seats.bundle, Side::Input),
        Berth::Earliest,
        "the supplied answer, which is what a taxonomy that never thought about it gets"
    );
    assert_eq!(
        seats.document.berth(ROOT, seats.roster, Side::Input),
        Berth::Fresh
    );
    let shape = |node: NodeId| {
        seats
            .document
            .signature(ROOT, node)
            .expect("a signature")
            .inputs
            .iter()
            .map(|port| (port.flow.clone(), port.multiplicity(Side::Input)))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        shape(seats.bundle),
        shape(seats.roster),
        "★★★★★ same seats, same types, same multiplicity — one declaration apart"
    );
}

/// ★★★★★ The kind states a PREFERENCE, not a permission: a text wire is refused
/// on a number seat however the kind would like to seat it, and the refusal
/// leaves the document exactly as it was.
fn a_preference_does_not_make_an_illegal_landing_legal(seats: &mut Seats) {
    let before = seats.document.clone();
    let why = seats
        .document
        .land(ROOT, seats.text, Side::Input, seats.roster, Item::plain())
        .expect_err("a text wire cannot land on a number run, Fresh or not");
    assert!(
        matches!(why, LandError::Refused(_)),
        "and it is the wire's own refusal, carried whole: {why}"
    );
    assert_eq!(
        seats.document, before,
        "★★★★★ no seat was grown and left behind — the reference's consumer leaves one"
    );
    assert!(
        seats
            .document
            .may_land(ROOT, seats.text, Side::Input, seats.roster)
            .is_err(),
        "and the question said the same, so trying taught nothing asking would not have"
    );
}

/// ★★★★★ R1980 — **the declaration is a preference, not a promise of a seat.**
///
/// `Berth::Fresh` says *never take a seat that has room, grow one*. A kind that
/// declares it on a side with **no run to grow** has asked for something its
/// own ports cannot give, and the answer is the refusal that names exactly
/// that — not a port appearing from nowhere, and not the free seat it said it
/// did not want.
///
/// ⚠ This performs a sentence [`Berth::Fresh`]'s own documentation makes. This
/// repository has four consecutive rounds (R1853–R1856) in which a comment
/// promised a property and nothing carried it out, each time written by the
/// round that broke it, so a promise in prose now costs an assertion.
///
/// ⚠⚠ **The wire has to be one the free seat would legally have taken**, and the
/// first draft's did not. [`Document::free_port_for`] ends by asking
/// [`Document::may_relink`] of each candidate, so a seat the wire could not go
/// on is skipped and the landing answers `NoRoom` **for the type**, by a path
/// that has nothing to do with the declaration. Driven at R1980: with the
/// declaration removed the assertion still passed, because two faults were
/// travelling together (R1845). The repair is to the POPULATION — a text wire,
/// which that text seat would take — and not to the assertion.
fn a_preference_is_not_a_promise_of_a_seat(seats: &mut Seats) {
    let seat = Socket::new(seats.roster, 0);
    assert_eq!(
        seats
            .document
            .signature(ROOT, seats.roster)
            .expect("a signature")
            .outputs
            .len(),
        1,
        "the producing side has exactly one seat, and this kind grows none there"
    );
    assert!(
        seats.document.occupants(ROOT, seat, Side::Output).is_free(),
        "★ nothing is standing on it — so what follows is not the seat being full"
    );
    assert!(
        seats
            .document
            .may_relink(ROOT, seats.text, Side::Output, seat)
            .is_ok(),
        "★★★★★ and the wire WOULD be legal on it — this is what makes the refusal \
         below attributable to the declaration and to nothing else"
    );

    let why = seats
        .document
        .may_land(ROOT, seats.text, Side::Output, seats.roster)
        .expect_err("it wants a seat of its own and this side grows none");
    assert!(
        matches!(why, LandError::NoRoom { .. }),
        "★★★★★ so it says NO ROOM rather than taking the seat it declared it did \
         not want: {why}"
    );
    let before = seats.document.clone();
    assert!(
        seats
            .document
            .land(ROOT, seats.text, Side::Output, seats.roster, Item::plain())
            .is_err(),
        "the act says what the question said"
    );
    assert_eq!(
        seats.document, before,
        "★ and left the document alone — no seat appeared from nowhere"
    );
}

/// ★ A body with no kind has no preference to state, and is answered the
/// supplied one rather than having the question refused.
fn a_structural_body_has_no_preference_to_state(seats: &mut Seats) {
    assert_eq!(
        seats.document.berth(ROOT, seats.group, Side::Input),
        Berth::Earliest,
        "a group instance's ports are its definition's interface, so a seat of its own \
         per arrival is not a thing it could mean"
    );
    assert_eq!(
        seats.document.berth(ROOT, NodeId(9999), Side::Input),
        Berth::Earliest,
        "and so is a node that is not there — the question has no refusal arm because \
         every caller of it is already holding one"
    );
}

/// Wire a fresh number source into every free input of `node`, and answer how
/// many were filled.
fn fill_every_input(document: &mut Document<Op>, node: NodeId) -> usize {
    let count = document
        .signature(ROOT, node)
        .map_or(0, |signature| signature.inputs.len());
    let mut filled = 0;
    for index in 0..count {
        let source = num(document, i64::try_from(index).unwrap_or(0));
        if document
            .connect(
                ROOT,
                Socket::new(source, 0),
                Socket::new(node, u32::try_from(index).unwrap_or(0)),
            )
            .is_ok()
        {
            filled += 1;
        }
    }
    filled
}

/// A new link with a spare end, so a landing has something to move.
fn wire_from_a_fresh_source(document: &mut Document<Op>, into: NodeId) -> LinkId {
    let source = num(document, 41);
    let spare = node(document, Op::Double);
    let _ = into;
    document
        .connect(ROOT, Socket::new(source, 0), Socket::new(spare, 0))
        .expect("a number reaches a double")
        .link
}

// ================================================== R1928 — port naming

/// ★★★★★ R1928 — a node says what it calls its own ports, and the answer says
/// who chose it.
///
/// The reference's pair, and the three ways this passes it are each asserted
/// rather than claimed:
///
/// * **three answers, not two hooks** — keep, rename, and show nothing — so the
///   state its own source is in (opt in to overriding, then say nothing, and
///   suppress by accident) cannot be built here;
/// * **the answer names its source**, which a bare text cannot carry, and all
///   three sources are reached: the kind's declaration, an item's authored
///   label, and this node's own answer;
/// * **the supplied answer is under test**, because most of this taxonomy does
///   not override at all.
///
/// Each property is its own function below, because each is a **separate
/// claim** and a reader looking for "where is silence asserted" should land on
/// a name rather than on a line number. The split was asked for by a length
/// lint and is the right one on its own terms — which is why it was taken
/// rather than allowed away.
#[test]
fn engine_node_get_pin_name_override() {
    let mut document = Document::new("root");
    the_supplied_answer_keeps_the_declaration(&mut document);
    a_silent_port_has_no_name_rather_than_a_blank_one(&mut document);
    a_renamed_port_says_the_node_chose_it(&mut document);
    an_authored_item_label_is_the_authors(&mut document);
    an_items_ordinal_has_an_origin_and_a_stride(&mut document);

    // ★ A port that is not there has no name, which is a different answer from
    // a port with no name shown.
    let add = node(&mut document, Op::Add);
    assert_eq!(document.port_label(ROOT, add, PortRef::input(9)), None);
    assert!(document.port_label(ROOT, add, PortRef::input(0)).is_some());

    // ⚠ A body that is not a kind has no hook to ask, and answers the kind's —
    // this is the arm that would panic if the resolution assumed a kind.
    let frame = document
        .add_node(ROOT, NodeBody::Frame, 0, 0)
        .expect("the root tree exists");
    assert_eq!(document.port_labels(ROOT, frame, Side::Input), Vec::new());
}

/// ★ The ORDINARY case, and it is the SUPPLIED answer: a kind that says nothing
/// keeps its declaration, and the source says so.
///
/// First because without it every assertion below could hold with the hook
/// wired to a constant — R1926's lesson, that a fixture overriding everywhere
/// leaves the default with no check on it at all.
fn the_supplied_answer_keeps_the_declaration(document: &mut Document<Op>) {
    let add = node(document, Op::Add);
    let plain = document
        .port_label(ROOT, add, PortRef::input(0))
        .expect("Add has a first input");
    assert_eq!(
        plain,
        pinion_node_graph::Labelled {
            text: Some("Augend".to_owned()),
            source: pinion_node_graph::NameSource::Kind,
        },
        "a kind that does not override keeps its own declaration"
    );
}

/// ★★★★★ SILENT — the reference's commonest use of this capability, and the one
/// its own type cannot tell from an empty answer. Here it is a value.
fn a_silent_port_has_no_name_rather_than_a_blank_one(document: &mut Document<Op>) {
    let carry = node(document, Op::Carry);
    let hushed = document
        .port_label(ROOT, carry, PortRef::input(1))
        .expect("Carry has a second input");
    assert_eq!(hushed.text, None, "Carry shows no name for its ports");
    assert_eq!(hushed.source, pinion_node_graph::NameSource::Node);
    assert!(!hushed.is_shown());
    assert!(
        document
            .port_labels(ROOT, carry, Side::Input)
            .iter()
            .all(|held| !held.is_shown()),
        "and it is every port on the side, the way the reference's reroutes do it"
    );
    // ⚠ And the ports are still THERE — suppressing a name is not removing a
    // port, which is the confusion an empty string invites. Asserted as a
    // RELATION and not against a written-down count, because a count equal to
    // the defect would not see it (R1921).
    assert_eq!(
        document.port_labels(ROOT, carry, Side::Input).len(),
        document
            .signature(ROOT, carry)
            .expect("a signature")
            .inputs
            .len(),
        "one label per declared port, silent or not"
    );
    assert!(
        !document.port_labels(ROOT, carry, Side::Input).is_empty(),
        "and there are ports to be silent about"
    );
}

/// ★★★★★ INSTEAD — a name of the node's own, and it is derived from the declared
/// one, which is what the argument is for.
fn a_renamed_port_says_the_node_chose_it(document: &mut Document<Op>) {
    let stage = node(document, Op::Stage(7));
    let renamed = document
        .port_label(ROOT, stage, PortRef::output(0))
        .expect("Stage has a first output");
    assert_eq!(
        renamed,
        pinion_node_graph::Labelled {
            text: Some("after Then".to_owned()),
            source: pinion_node_graph::NameSource::Node,
        },
    );
    // ★ And only that port: a hook that answered for every port would pass the
    // line above and lose what makes this per-port.
    let untouched = document
        .port_label(ROOT, stage, PortRef::output(1))
        .expect("Stage has a second output");
    assert_eq!(untouched.source, pinion_node_graph::NameSource::Kind);
    assert_eq!(untouched.text.as_deref(), Some("Cost"));
}

/// ★★★★★ ITEM — the third source, which existed before this round and had no way
/// to be told apart from the kind's. An unlabelled item's name is derived from
/// its ordinal and is the KIND's; a labelled one is the author's, and now says
/// so.
fn an_authored_item_label_is_the_authors(document: &mut Document<Op>) {
    let sequence = node(document, Op::Sequence);
    let ordinal = document
        .port_label(ROOT, sequence, PortRef::output(0))
        .expect("the run tops up to its minimum");
    assert_eq!(
        ordinal.source,
        pinion_node_graph::NameSource::Kind,
        "an ordinal-derived name is the kind's: {ordinal:?}"
    );
    document
        .insert_item(
            ROOT,
            sequence,
            Side::Output,
            0,
            Item::plain().named("early"),
        )
        .expect("the run takes an item");
    let authored = document
        .port_label(ROOT, sequence, PortRef::output(0))
        .expect("the item contributes a port");
    assert_eq!(
        authored,
        pinion_node_graph::Labelled {
            text: Some("early".to_owned()),
            source: pinion_node_graph::NameSource::Item,
        },
        "and an authored one is the author's"
    );
    // ⚠ The item after it is still the kind's, so the source is a property of
    // the PORT and not of the node.
    assert_eq!(
        document
            .port_label(ROOT, sequence, PortRef::output(1))
            .expect("the topped-up item")
            .source,
        pinion_node_graph::NameSource::Kind,
    );
}

/// ★★★★★ A run that does NOT begin at index 0, and whose item contributes MORE
/// THAN ONE port. `Blend` declares two fixed inputs with its run spliced between
/// them, so the ordinal has an origin and a stride that are both different from
/// the trivial ones — and a resolution that measured either from the wrong place
/// would still be right on every kind above.
///
/// ⚠ This function exists because a counterfactual PASSED without it: dropping
/// the run's offset from the arithmetic left the whole suite green, since every
/// other variadic kind in this taxonomy starts its run at zero. The fixture's
/// POPULATION was wrong, not the assertion — R1926's class, and the repair is
/// the same one: widen what is asked, not what is claimed.
fn an_items_ordinal_has_an_origin_and_a_stride(document: &mut Document<Op>) {
    let blend = node(document, Op::Blend);
    document
        .insert_item(ROOT, blend, Side::Input, 0, Item::plain().named("hero"))
        .expect("the run takes an item");
    assert_eq!(
        document
            .port_label(ROOT, blend, PortRef::input(0))
            .expect("the fixed port before the run")
            .source,
        pinion_node_graph::NameSource::Kind,
        "the port BEFORE the run is the kind's, however the run's items are labelled"
    );
    for index in [1, 2] {
        assert_eq!(
            document
                .port_label(ROOT, blend, PortRef::input(index))
                .unwrap_or_else(|| panic!("input {index} is one of the item's two ports"))
                .source,
            pinion_node_graph::NameSource::Item,
            "both ports of a labelled item are the author's, not just the first"
        );
    }
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
        said.sentence().contains("nothing reaches this sink"),
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
    assert!(listed.iter().all(|held| !held.sentence().is_empty()));

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

/// ★★★★★ R1985 — **may a copy of this node be made, and may it land here.**
///
/// Two census rows, one mechanism, and the reference is what says they are one:
/// its paste-permission hook is asked of a NODE about a TARGET GRAPH, and one
/// of its TEN overriding classes answers by gathering every name the
/// destination already uses and declining if this one is among them. Its
/// duplicate hook is the same question with the destination being where the
/// node already is — seventeen classes override it and TEN refuse outright,
/// and those ten are the graph's fixed ends: a root, a result, an entry, a pin
/// base, each a node a graph has exactly one of.
///
/// ⚠ Both rows' pin reasons said the EDIT did not exist here — *no clipboard
/// and no paste verb*, *no verb that duplicates a node* — and both were false
/// when they were written at R1920: `extract`, `insert` and `duplicate` landed
/// at R1578, 342 rounds earlier. What was absent is what this proves.
#[test]
fn engine_node_can_duplicate_node() {
    /// A kind that is the graph's fixed end: there is one of these, under that
    /// name, and a second under another name is not what was asked for.
    #[derive(Debug, Clone, PartialEq)]
    struct Entry;
    impl NodeKind for Entry {
        type Type = ();
        type Value = ();
        type Graph = ();
        fn name(&self) -> String {
            "entry".to_owned()
        }
        fn inputs(&self) -> Vec<Port<(), ()>> {
            Vec::new()
        }
        fn outputs(&self) -> Vec<Port<(), ()>> {
            Vec::new()
        }
        fn evaluate(&self, _: &[Option<()>]) -> Vec<Option<()>> {
            Vec::new()
        }
        fn copying(&self) -> Copying {
            Copying::Refused
        }
    }

    let mut document: Document<Entry> = Document::new("root");
    let entry = document
        .add_node(ROOT, NodeBody::Kind(Entry), 0, 0)
        .unwrap();
    document.relabel(ROOT, entry, Some("Begin")).unwrap();

    // ★ `CanDuplicateNode` — refused, and the document is untouched, because
    // the decision is made in the plan and not part way through the writing.
    let before = document.tree(ROOT).unwrap().node_count();
    let why = document
        .duplicate(
            ROOT,
            &[entry],
            (0, 200),
            Crossings::Drop,
            Definitions::Share,
        )
        .unwrap_err();
    assert!(matches!(
        why,
        pinion_node_graph::DuplicateError::Place(InsertError::NameTaken { .. })
    ));
    assert_eq!(document.tree(ROOT).unwrap().node_count(), before);

    // ★★★★★ PAST THE REFERENCE: its hook answers a bare `bool`, so *I refuse*
    // and *nothing to say* are the same value, and at its own paste site a
    // refusal breaks that node's links and continues — a person is left with a
    // node missing its wires and nothing said. This names the node, the name,
    // and who here already answers to it.
    let said = why.to_string();
    assert!(
        said.contains("Begin"),
        "the name is in the sentence: {said}"
    );

    // ★ `CanPasteHere` is the same question asked of a destination: the very
    // same node lands wherever nothing answers to its name.
    let cut = document.extract(ROOT, &[entry]).unwrap();
    let mut elsewhere: Document<Entry> = Document::new("elsewhere");
    let landed = elsewhere
        .insert(ROOT, &cut, (0, 0), Crossings::Drop, Definitions::Share)
        .unwrap();
    assert_eq!(landed.nodes.len(), 1);
    assert!(landed.renamed.is_empty(), "nothing had to change");

    // ★★★★★ AND THE OTHER ANSWER IS A DECLARATION, NOT THE ONLY ONE. The DCC
    // renames the copy instead of refusing it — its copy helper calls its own
    // unique-name routine on the destination tree — and neither reference can
    // express the other. A kind says which, and either way the insertion
    // REPORTS what happened, which neither reference does.
    let mut plain: Document<Op> = Document::new("root");
    let node = plain.add_node(ROOT, NodeBody::Kind(Op::Add), 0, 0).unwrap();
    plain.relabel(ROOT, node, Some("Begin")).unwrap();
    let copied = plain
        .duplicate(ROOT, &[node], (0, 200), Crossings::Drop, Definitions::Share)
        .unwrap();
    assert_eq!(
        copied.renamed,
        vec![Renamed {
            node: copied.nodes[0],
            from: "Begin".to_owned(),
            to: "Begin-01".to_owned(),
        }]
    );

    // ★★★★★ THE LAW THIS ROUND EXISTS FOR: the copy path and the permission
    // surface are one decision. Before R1985 `duplicate` built the state
    // `may(Act::Rename)` refuses — two nodes answering to one name — and
    // `node_labelled` then addressed NEITHER.
    assert_eq!(plain.node_labelled(ROOT, "Begin"), Some(node));
    assert_eq!(plain.node_labelled(ROOT, "Begin-01"), Some(copied.nodes[0]));
    for id in [node, copied.nodes[0]] {
        let held = plain.tree(ROOT).unwrap().node(id).unwrap();
        let name = held.display_name();
        assert!(
            plain.may(ROOT, Act::Rename(id, Some(&name))).is_ok(),
            "every card holds a name it is ALLOWED to hold: {name}"
        );
    }
}

/// ★★★★★ R1998 — **what this taxonomy puts in place of a body the destination
/// will not take.**
///
/// The engine's schema publishes a hook handing back a node to use *in place
/// of* one being pasted; its base body answers `nullptr` and exactly one class
/// overrides it, turning a pasted **event** node into a **custom event** — it
/// declines unless the destination is the graph type that may hold events at
/// all, gathers the names already in use, and builds the replacement holding
/// the name the original arrived with. Its call site is the paste: for every
/// object it asks *may you be pasted here*, and where the answer is no it asks
/// the schema for a substitute, destroys the original when the two differ, and
/// spawns what is left.
///
/// # What this proves, and where it passes the reference
///
/// ★★★★★ **One value there is two facts.** `nullptr` is *this schema offers
/// nothing* and it is also *this node may not live in this graph at all*, and
/// the call site cannot tell them apart: both destroy the node and spawn
/// nothing. A person who pasted five nodes and got four is told nothing about
/// the fifth. Here they are three outcomes and all three are said — the
/// refusal the destination gave, [`InsertError::SubstituteUnlandable`], or a
/// paste whose [`Inserted::substituted`] names what arrived as one thing and
/// was placed as another.
///
/// ★★★★★ **And the hook is TOLD why.** The engine's is not: it re-decides for
/// itself whether the destination could have held the node, which is the
/// destination's answer computed a second time in a second place. Phase 2
/// turns on that — one kind answers one way for a name clash and another for
/// an interface end — so a hook that were not told could not pass it.
///
/// ★★★★★ **A stand-in owes the wires.** It is a different body with different
/// ports, and the fragment's wires were drawn against the old ones; the engine
/// re-matches its pins by name afterwards and quietly loses the ones that find
/// no partner. [`Document::insert`] documents a guarantee that would break, so
/// a stand-in that cannot carry what the original carried is refused with
/// [`InsertError::SubstituteCannotCarry`] before anything is written.
#[test]
fn engine_schema_create_substitute_node() {
    nothing_offered_keeps_the_refusal_the_destination_gave();
    a_stand_in_that_lands_is_named_in_the_landing();
    a_stand_in_that_cannot_land_is_a_third_outcome();
    a_stand_in_must_carry_the_wires_the_original_carried();
    the_other_per_node_refusal_reaches_the_hook_too();
    a_severed_value_is_judged_against_the_body_that_will_be_there();
}

/// A taxonomy of declarations, each member here because one arm of the
/// question needs its shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Decl {
    /// The graph's own declaration: there is one of these, under that
    /// name, so a copy is refused rather than renamed. The engine's event.
    Event,
    /// The one a person owns — same ports, named freely. What stands in
    /// for `Event`, as the engine's custom event stands in for its event.
    Custom,
    /// A declaration this taxonomy has nothing to offer for.
    Lone,
    /// A declaration whose stand-in is itself refused here.
    Stubborn,
    /// A declaration with two inputs.
    Wide,
    /// The stand-in offered for `Wide`, with one — so a wire into the
    /// second has nowhere to land.
    Narrow,
    /// A declaration whose input takes a number.
    Typed,
    /// The stand-in offered for `Typed`, whose input takes a word — so the
    /// port is there and what the wire carries cannot cross into it.
    Worded,
    /// A source of numbers, so a fragment can carry a wire at all.
    Feed,
    /// A declaration whose stand-in is a **point on a wire** — the crate's own
    /// [`NodeBody::Reroute`], not one of this taxonomy's kinds.
    ///
    /// The member that makes the *undecided* answer reachable at all: a
    /// reroute's ports carry whatever the chain hands them, so a body that
    /// declares its types could never reach that arm.
    Cabled,
    /// What stands in for an **input** interface end that arrived from
    /// elsewhere: the tree it materialised is not this one, so what is left
    /// is the value it passed on.
    Adrift,
}

impl NodeKind for Decl {
    type Type = Ty;
    type Value = Val;
    type Graph = ();

    fn name(&self) -> String {
        format!("{self:?}")
    }

    /// What each takes, which is the half a stand-in's ports are judged on.
    fn inputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Event | Self::Custom | Self::Lone | Self::Stubborn => {
                vec![Port::new("In", Ty::Number)]
            }
            Self::Wide => vec![Port::new("A", Ty::Number), Port::new("B", Ty::Number)],
            Self::Narrow | Self::Typed | Self::Cabled => vec![Port::new("A", Ty::Number)],
            Self::Worded => vec![Port::new("A", Ty::Text)],
            Self::Feed | Self::Adrift => Vec::new(),
        }
    }

    /// Every member hands on one number, so a fragment can always be wired.
    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        vec![Port::new("Out", Ty::Number)]
    }

    fn evaluate(&self, _: &[Option<Val>]) -> Vec<Option<Val>> {
        vec![Some(Val::Number(0))]
    }

    /// The ones whose name is the thing they ARE.
    fn copying(&self) -> Copying {
        match self {
            Self::Event | Self::Lone | Self::Stubborn | Self::Wide | Self::Typed | Self::Cabled => {
                Copying::Refused
            }
            Self::Custom | Self::Narrow | Self::Worded | Self::Feed | Self::Adrift => {
                Copying::Renamed
            }
        }
    }

    /// ★★★★★ Matched EXHAUSTIVELY on the reason, which is the declaration
    /// this round exists for. A hook that were handed no reason could not
    /// answer `Event` two different ways, and a `why` that were
    /// `non_exhaustive` would let a taxonomy answer a refusal it has never
    /// considered from a wildcard arm.
    fn substitute(body: &NodeBody<Self>, why: &Unlandable) -> Option<NodeBody<Self>> {
        let kind = match (body, why) {
            (NodeBody::Kind(Self::Event), Unlandable::NameTaken { .. }) => Self::Custom,
            (NodeBody::Kind(Self::Stubborn), Unlandable::NameTaken { .. }) => Self::Event,
            (NodeBody::Kind(Self::Wide), Unlandable::NameTaken { .. }) => Self::Narrow,
            (NodeBody::Kind(Self::Typed), Unlandable::NameTaken { .. }) => Self::Worded,
            (NodeBody::Interface(_), Unlandable::InterfaceEnd(InterfaceSide::Input)) => {
                Self::Adrift
            }
            // ★ A stand-in need not be one of this taxonomy's kinds: the bodies
            // the crate itself owns are offerable too, and a point on a wire is
            // the one whose ports have no type of their own.
            (NodeBody::Kind(Self::Cabled), Unlandable::NameTaken { .. }) => {
                return Some(NodeBody::Reroute);
            }
            // `Lone` has no stand-in, and neither has anything else — the
            // honest answer, and the base implementation's.
            _ => return None,
        };
        Some(NodeBody::Kind(kind))
    }
}

/// A document holding one `kind` named `held`, and a fragment carrying a
/// second one of the same name — the state a paste of a declaration into a
/// graph that already has it produces.
fn clash(kind: &Decl, held: &str) -> (Document<Decl>, Fragment<Decl>) {
    let mut document: Document<Decl> = Document::new("root");
    let there = document
        .add_node(ROOT, NodeBody::Kind(kind.clone()), 0, 0)
        .unwrap();
    document.relabel(ROOT, there, Some(held)).unwrap();
    // What a person wrote about this node, and what its KIND holds — the
    // two sides of the line a stand-in is copied across.
    document
        .set_port_value(ROOT, there, PortRef::input(0), Val::Number(7))
        .unwrap();
    if let Some(slot) = document
        .tree_mut(ROOT)
        .and_then(|host| host.node_mut(there))
    {
        slot.description = Some("the one everything dials".to_owned());
        slot.appearance.tint = Some(Tint::rgb(220, 40, 60));
    }
    let cut = document.extract(ROOT, &[there]).unwrap();
    (document, cut)
}

/// Phase 1 — **the taxonomy offers nothing: the refusal the destination gave.**
///
/// `Lone` reaches the hook and the hook declines, so the paste keeps the
/// `NameTaken` it already had and the document is untouched. This is the arm
/// the engine's `nullptr` shares with phase 3, and telling them apart is the
/// whole of this round.
fn nothing_offered_keeps_the_refusal_the_destination_gave() {
    let (mut document, cut) = clash(&Decl::Lone, "Only");
    let before = document.clone();
    let why = document
        .insert(ROOT, &cut, (0, 200), Crossings::Drop, Definitions::Share)
        .unwrap_err();
    assert!(
        matches!(&why, InsertError::NameTaken { label, .. } if label == "Only"),
        "nothing stood in, so the refusal is the one the destination gave: {why}"
    );
    assert_eq!(document, before, "and a refusal leaves the document alone");
}

/// Phase 2 — **the taxonomy offers a body that lands**, and the landing names
/// what arrived as one thing and was placed as another.
fn a_stand_in_that_lands_is_named_in_the_landing() {
    let (mut document, cut) = clash(&Decl::Event, "Begin");
    let landed = document
        .insert(ROOT, &cut, (0, 200), Crossings::Drop, Definitions::Share)
        .unwrap();
    assert_eq!(landed.nodes.len(), 1, "the paste happened");
    let arrived = cut.nodes().next().unwrap().id;
    assert_eq!(
        landed.substituted,
        vec![Substitution {
            node: arrived,
            became: landed.nodes[0],
            why: Unlandable::NameTaken {
                label: "Begin".to_owned(),
                held_by: (ROOT, NodeId(0)),
            },
        }],
        "★★★★★ and it SAYS what arrived as one thing was placed as another — \
         the engine reports this nowhere, so a substituted node and a dropped \
         one leave the same trace there, which is none"
    );
    let placed = document.tree(ROOT).unwrap().node(landed.nodes[0]).unwrap();
    assert_eq!(
        placed.body,
        NodeBody::Kind(Decl::Custom),
        "the stand-in is what is in the document, not what was copied"
    );
    // ⚠ The incumbent keeps its name and the stand-in takes one of its own.
    // The engine's overrider renames the CLASHING OBJECT out of the way
    // instead — a paste that edits what was already there, which is not what
    // somebody who pressed paste asked for.
    assert_eq!(document.node_labelled(ROOT, "Begin"), Some(NodeId(0)));
    assert_eq!(
        document.node_labelled(ROOT, "Begin-01"),
        Some(landed.nodes[0])
    );

    // ★★★★★ What a stand-in inherits: **what a person wrote, and nothing the
    // kind holds.** A note means the same thing whatever body sits under it; a
    // tint and a held port value describe the body that arrived, and this is
    // not that body. The engine's one overrider draws the same line — it builds
    // a fresh node and carries only the name across.
    let stood = landed.nodes[0];
    let note = document
        .description(ROOT, stood)
        .expect("a person's sentence about the card travels with it");
    assert_eq!(note.sentence, "the one everything dials");
    assert_eq!(
        note.source,
        Described::Authored,
        "and it arrives still marked as a PERSON's, not as the stand-in kind's \
         own sentence about itself"
    );
    let held = document.tree(ROOT).unwrap().node(stood).unwrap();
    assert_eq!(
        held.appearance.tint, None,
        "★ but the colour does not: it described a declaration that is not here \
         any more"
    );
    assert_eq!(
        document.port_value(ROOT, stood, PortRef::input(0)),
        None,
        "★ nor does a value held on a port, which is addressed into the \
         signature of the body that arrived"
    );
    assert_eq!(
        document
            .tree(ROOT)
            .unwrap()
            .node(NodeId(0))
            .unwrap()
            .appearance
            .tint,
        Some(Tint::rgb(220, 40, 60)),
        "and the ORIGINAL still has both — this is about the stand-in, not \
         about losing them"
    );
}

/// Phase 3 — **the taxonomy offers a body that cannot land either.**
///
/// `Stubborn` stands `Event` in, and `Event` is refused here for the very same
/// reason. Asked once and not again: a taxonomy answering a refusal with
/// another refused body is describing a hole in itself, and looping would only
/// find it later.
fn a_stand_in_that_cannot_land_is_a_third_outcome() {
    let (mut document, cut) = clash(&Decl::Stubborn, "Again");
    let before = document.clone();
    let why = document
        .insert(ROOT, &cut, (0, 200), Crossings::Drop, Definitions::Share)
        .unwrap_err();
    assert!(
        matches!(
            &why,
            InsertError::SubstituteUnlandable {
                why: Unlandable::NameTaken { label, .. },
                ..
            } if label == "Again"
        ),
        "★★★★★ a THIRD outcome, which the engine's one null cannot express: {why}"
    );
    assert!(
        why.to_string().contains("Again"),
        "and the sentence names what could not land: {why}"
    );
    assert_eq!(document, before);
}

/// Phase 4 — **a stand-in that cannot carry the wires the original carried.**
///
/// Two shapes of *cannot carry*, proven apart: a port that is not there, and a
/// port that is there and will not take what the wire carries.
fn a_stand_in_must_carry_the_wires_the_original_carried() {
    for (kind, held, port, side) in [
        (Decl::Wide, "Fan", 1, Side::Input),
        (Decl::Typed, "Count", 0, Side::Input),
    ] {
        let mut document: Document<Decl> = Document::new("root");
        let there = document
            .add_node(ROOT, NodeBody::Kind(kind.clone()), 0, 0)
            .unwrap();
        document.relabel(ROOT, there, Some(held)).unwrap();
        let feed = document
            .add_node(ROOT, NodeBody::Kind(Decl::Feed), -100, 0)
            .unwrap();
        document
            .connect(ROOT, Socket::new(feed, 0), Socket::new(there, port))
            .unwrap();
        // BOTH ends travel, so the wire is inside the fragment and is what the
        // stand-in is asked to carry.
        let cut = document.extract(ROOT, &[there, feed]).unwrap();
        let before = document.clone();
        let refused = document
            .insert(ROOT, &cut, (0, 200), Crossings::Drop, Definitions::Share)
            .unwrap_err();
        assert!(
            matches!(
                refused,
                InsertError::SubstituteCannotCarry { port: p, side: s, .. }
                    if p == port && s == side
            ),
            "★★★★★ refused BEFORE the first mutation, naming the port the \
             wiring needs — the engine loses such a wire silently: {refused}"
        );
        assert_eq!(document, before, "so the document is untouched: {held}");
    }
}

/// Phase 5 — **the other per-node refusal: an interface end.**
///
/// [`Document::extract`] refuses to build one, so this is a fragment that
/// ARRIVED FROM ELSEWHERE — off a wire or out of a file, which is what a
/// fragment is for. The hook is asked here too, which is what makes the
/// substitution reach the WHOLE population of per-node refusals rather than
/// whichever one was convenient.
fn the_other_per_node_refusal_reaches_the_hook_too() {
    let mut document: Document<Decl> = Document::new("root");
    let lone = document
        .add_node(ROOT, NodeBody::Kind(Decl::Lone), 0, 0)
        .unwrap();
    let cut = document.extract(ROOT, &[lone]).unwrap();
    let elsewhere = serde_json::to_string(&cut)
        .unwrap()
        .replace(r#"{"Kind":"Lone"}"#, r#"{"Interface":"Input"}"#);
    let arrived: Fragment<Decl> = serde_json::from_str(&elsewhere).unwrap();
    assert!(
        arrived
            .nodes()
            .any(|node| matches!(node.body, NodeBody::Interface(_))),
        "the fragment really does carry an interface end"
    );
    let landed = document
        .insert(
            ROOT,
            &arrived,
            (0, 200),
            Crossings::Drop,
            Definitions::Share,
        )
        .unwrap();
    assert_eq!(
        landed.substituted.first().map(|it| &it.why),
        Some(&Unlandable::InterfaceEnd(InterfaceSide::Input)),
        "★ and the hook was told WHICH refusal this was"
    );
    assert_eq!(
        document
            .tree(ROOT)
            .unwrap()
            .node(landed.nodes[0])
            .unwrap()
            .body,
        NodeBody::Kind(Decl::Adrift)
    );

    // ---- 6. and the same fragment, into a taxonomy that offers nothing -----
    //
    // ⚠ The counterfactual for phase 5: `Op` declares no substitute at all, so
    // it takes the supplied `None` — and an interface end that arrived from
    // elsewhere is refused exactly as it was before this hook existed. Without
    // this, phase 5 could be passing because interface ends are simply allowed.
    let mut plain: Document<Op> = Document::new("root");
    let adder = plain.add_node(ROOT, NodeBody::Kind(Op::Add), 0, 0).unwrap();
    let cut = plain.extract(ROOT, &[adder]).unwrap();
    let elsewhere = serde_json::to_string(&cut)
        .unwrap()
        .replace(r#"{"Kind":"Add"}"#, r#"{"Interface":"Input"}"#);
    let arrived: Fragment<Op> = serde_json::from_str(&elsewhere).unwrap();
    assert!(
        matches!(
            plain.insert(ROOT, &arrived, (0, 0), Crossings::Drop, Definitions::Share),
            Err(InsertError::InterfaceNodeInFragment(_))
        ),
        "a taxonomy that offers nothing keeps the refusal it always had"
    );
}

/// Phase 6 — **a severed value is judged against the body that WILL be there,
/// not the one that arrived.**
///
/// A paste re-feeds a copy from the socket that fed its original where that
/// socket is still here ([`Crossings::KeepInbound`]), and the port it would
/// land on belongs to the stand-in. Judging it against the body that is not
/// going to be there is how a crossing gets restored onto a port that no longer
/// exists — which is why R1998 moved the naming decision, where the second
/// substitution is made, **ahead** of the crossings.
///
/// ⚠ This phase exists because a counterfactual PASSED: breaking *a port whose
/// type is undecided accepts what reaches it* caught nothing, since every
/// stand-in the other phases offer is a body that DECLARES its types. The
/// repair is the population, not the assertion — the eighth time this campaign
/// has been caught by that.
fn a_severed_value_is_judged_against_the_body_that_will_be_there() {
    // (a) A stand-in that is a point on a wire declares no type of its own, so
    //     the severed value reaches it.
    let (mut document, cut) = severed_into(&Decl::Cabled, "Line");
    let landed = document
        .insert(
            ROOT,
            &cut,
            (0, 200),
            Crossings::KeepInbound,
            Definitions::Share,
        )
        .unwrap();
    assert_eq!(landed.substituted.len(), 1, "the stand-in was made");
    assert_eq!(
        document
            .tree(ROOT)
            .unwrap()
            .node(landed.nodes[0])
            .unwrap()
            .body,
        NodeBody::Reroute,
        "★ and it is one of the crate's own bodies, not one of this taxonomy's \
         kinds — a stand-in is a BODY, so the whole vocabulary is offerable"
    );
    assert!(
        landed.unattached.is_empty(),
        "★★★★★ the severed value REACHED it: a point on a wire takes the type \
         of whatever feeds it, so a crossing into one is allowed by \
         construction rather than being a hole the paste has to report"
    );

    // (b) And a stand-in that DOES declare its ports is judged on them: this
    //     one takes a word where the severed value is a number, so the paste
    //     lands and says the input could not be re-fed.
    let (mut document, cut) = severed_into(&Decl::Typed, "Count");
    let landed = document
        .insert(
            ROOT,
            &cut,
            (0, 200),
            Crossings::KeepInbound,
            Definitions::Share,
        )
        .unwrap();
    assert_eq!(landed.substituted.len(), 1);
    assert_eq!(
        landed.unattached.len(),
        1,
        "★★★★★ judged against the STAND-IN's port and not the arrived one — \
         re-feeding a number onto the word this body actually has would be a \
         wire the document's own rules refuse"
    );
}

/// A document whose `kind` is fed from a source, and a fragment carrying only
/// the consumer — so what arrives is a copy with one input cut, which is the
/// state [`Crossings::KeepInbound`] is about.
fn severed_into(kind: &Decl, held: &str) -> (Document<Decl>, Fragment<Decl>) {
    let mut document: Document<Decl> = Document::new("root");
    let feed = document
        .add_node(ROOT, NodeBody::Kind(Decl::Feed), -100, 0)
        .unwrap();
    let there = document
        .add_node(ROOT, NodeBody::Kind(kind.clone()), 0, 0)
        .unwrap();
    document.relabel(ROOT, there, Some(held)).unwrap();
    document
        .connect(ROOT, Socket::new(feed, 0), Socket::new(there, 0))
        .unwrap();
    let cut = document.extract(ROOT, &[there]).unwrap();
    assert_eq!(
        cut.inbound().len(),
        1,
        "the copy arrives with its input cut"
    );
    (document, cut)
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
        said.carries,
        Carrying::Value {
            described: Some("two numbers written `left|right`".to_owned()),
        },
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
    assert_eq!(loose.carries, Carrying::Value { described: None });
    assert!(!loose.sentence().contains("  "), "{:?}", loose.sentence());
    // ★★★★★ R1934 — and it does not say something ELSE about the type either.
    // This line is what was missing: the assertion above checked the sentence
    // had no hole in it, so a sentence that filled the hole with the wrong
    // fact — "accepts control", which is what an undescribed value port said
    // from R1916 to R1934 — passed it.
    assert!(
        !loose.sentence().contains("control"),
        "a value port claimed to carry control: {:?}",
        loose.sentence(),
    );
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
///
/// ⚠ R1985 — the LABEL is the one field a copy does not carry verbatim, and
/// the DCC is where that rule comes from: its copy helper gives the copy a name
/// unique in the destination tree. Everything else here is unchanged, which is
/// the point of the distinction — the label is an ADDRESS in this tree and the
/// rest is what the node is.
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
    assert_eq!(
        made.label.as_deref(),
        Some("stage-01"),
        "the address is the one field that cannot be carried verbatim"
    );
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
///
/// ⚠★★★★★ R1985 — *travels with a copy* used to be asserted as *travels
/// UNCHANGED*, and that was asserting a defect: a copy landing in the tree it
/// came from under its original's name leaves two nodes answering to one, which
/// `may(Act::Rename)` refuses to create and `node_labelled` then cannot address.
/// What is a property of the node is that the copy arrives NAMED — carrying its
/// original's name where nothing here answers to it, and a name derived from it
/// where something does. Both are asserted now.
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
        Some("Total-01")
    );
    // ★ and the copy SAYS where its name came from, which the reference's own
    // rename notification cannot: it is handed the node and nothing else.
    assert_eq!(
        copy.renamed,
        vec![Renamed {
            node: copy.nodes[0],
            from: "Total".to_owned(),
            to: "Total-01".to_owned(),
        }]
    );
    // ★★★★★ The original still answers to its own name, so the rename that
    // travelled did not cost the graph an address.
    assert_eq!(chain.document.node_labelled(ROOT, "Total"), Some(chain.add));

    // ★ Into a tree that has never heard of it, the name comes across whole.
    let cut = chain.document.extract(ROOT, &[chain.add]).unwrap();
    let mut elsewhere: Document<Op> = Document::new("elsewhere");
    let landed = elsewhere
        .insert(ROOT, &cut, (0, 0), Crossings::Drop, Definitions::Share)
        .unwrap();
    assert_eq!(
        elsewhere
            .tree(ROOT)
            .unwrap()
            .node(landed.nodes[0])
            .unwrap()
            .label
            .as_deref(),
        Some("Total")
    );
    assert!(landed.renamed.is_empty());
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

/// ★★★★★ R2000 — the animation editor's `ReverseTransition`: **a link runs the
/// other way without being redrawn.**
///
/// # What the reference does, and where its verb is cheap
///
/// A transition there sits between two state nodes that have exactly one
/// inbound and one outbound pin apiece, so *which ports* never comes up and the
/// verb can be a bare command. A node here has as many ports as its kind
/// declares, so the reversal is a **landing** — [`Berth`]'s policy over a pair,
/// the same rule a drop on a card already follows — and this proves the three
/// outcomes that policy has: an existing pair takes it, a port appears for it,
/// or nothing can hold it.
///
/// # What would differ if the capability were missing
///
/// The link's [`LinkId`] and its mute. Delete-and-redraw reaches the same
/// picture and mints a new id, so everything holding the old name — a picked
/// wire, a breakpoint, an undo entry — is left pointing at nothing. Both are
/// asserted, and the mute is asserted because a wiring being A/B-tested is
/// still being A/B-tested after somebody notices it points the wrong way.
#[test]
fn engine_anim_graph_reverse_transition() {
    a_transition_turns_round_under_its_own_name();
    a_value_link_turns_round_and_the_walk_ignores_it();
    two_ports_would_take_it_and_the_earliest_does();
    a_port_appears_when_none_has_room();
    a_far_node_that_produces_nothing_is_its_own_refusal();
}

/// The reference's own case: one pin per side, and the link keeps its name.
fn a_transition_turns_round_under_its_own_name() {
    let mut document: Document<Op> = Document::new("root");
    let first = node(&mut document, Op::Stage(1));
    let second = node(&mut document, Op::Stage(2));
    let made = document
        .connect(ROOT, Socket::new(first, 0), Socket::new(second, 0))
        .expect("a control link between two stages");
    document
        .set_link_muted(ROOT, made.link, true)
        .expect("the transition is being A/B-tested");

    assert_eq!(
        document.may_turn(ROOT, made.link),
        Ok((
            Landfall::Takes(Socket::new(second, 0)),
            Landfall::Takes(Socket::new(first, 0)),
        )),
        "★ both pins are there, so nothing has to appear — and the question \
         says WHICH pins rather than merely yes"
    );
    let turned = document
        .turn(ROOT, made.link, Item::plain())
        .expect("the transition turns round");
    assert_eq!(
        turned.retargeted.link, made.link,
        "★★★★★ THE POINT: the same link, not a new one"
    );
    assert!(turned.reversed(), "and it runs between the same two stages");
    assert_eq!(
        turned.retargeted.was,
        (Socket::new(first, 0), Socket::new(second, 0))
    );
    assert_eq!(
        document
            .tree(ROOT)
            .and_then(|host| host.link(made.link))
            .map(|link| (link.from, link.to, link.muted)),
        Some((Socket::new(second, 0), Socket::new(first, 0), true)),
        "★ and the mute travelled with it"
    );
    // ★ A stage's second output carries a NUMBER, so the search had to reject
    // it and take the control pin. Asserted because port order is what makes
    // that luck rather than a rule: `Then` happens to be first here.
    assert_eq!(
        port_names(&document, second, Side::Output),
        ["Then", "Cost"],
        "★ the pair the landing chose is the one the flows admit, not port 0 by \
         position"
    );
}

/// The data plane, where the acyclicity walk must not see the moving link.
fn a_value_link_turns_round_and_the_walk_ignores_it() {
    let mut document: Document<Op> = Document::new("root");
    let doubling = node(&mut document, Op::Double);
    let relay = node(&mut document, Op::Relay);
    let made = document
        .connect(ROOT, Socket::new(doubling, 0), Socket::new(relay, 0))
        .expect("a value link");
    assert!(
        document
            .turn(ROOT, made.link, Item::plain())
            .is_ok_and(|turned| turned.reversed()),
        "★★★★★ a VALUE link turns round too — and this is the assertion that \
         fails when the acyclicity walk is allowed to see the link being \
         moved, because the path it finds from the far node back is that link"
    );
}

/// Two ports would take it: the EARLIEST does, and that is a policy.
fn two_ports_would_take_it_and_the_earliest_does() {
    let mut document: Document<Op> = Document::new("root");
    let sum = node(&mut document, Op::Add);
    let doubling = node(&mut document, Op::Double);
    let made = document
        .connect(ROOT, Socket::new(sum, 0), Socket::new(doubling, 0))
        .expect("a value link");
    assert_eq!(
        document.may_turn(ROOT, made.link),
        Ok((
            Landfall::Takes(Socket::new(doubling, 0)),
            Landfall::Takes(Socket::new(sum, 0)),
        )),
        "★ an augend AND an addend would take it; the earliest does, which is \
         the rule a drop on this node already follows. The first draft of this \
         verb refused here instead, and the node lab produced that refusal on \
         its second gesture"
    );
    assert!(
        document
            .turn(ROOT, made.link, Item::plain())
            .is_ok_and(|turned| turned.reversed()),
    );
}

/// A port appears for it: every existing one is taken.
fn a_port_appears_when_none_has_room() {
    let mut document: Document<Op> = Document::new("root");
    let blend = node(&mut document, Op::Blend);
    let doubling = node(&mut document, Op::Double);
    let made = document
        .connect(ROOT, Socket::new(blend, 0), Socket::new(doubling, 0))
        .expect("a value link");
    for port in 0..4 {
        let source = num(&mut document, i64::from(port));
        wire(&mut document, source, 0, blend, port);
    }
    assert_eq!(
        port_names(&document, blend, Side::Input),
        ["Base", "Pose 0", "Weight 0", "Bias"],
        "every input the blend has is now fed"
    );
    let turned = document
        .turn(ROOT, made.link, Item::plain())
        .expect("the run grows a seat for it");
    assert!(
        matches!(turned.falls.1, Landfall::Grows(_)),
        "★ no existing port had room, so one appeared: {:?}",
        turned.falls
    );
    assert_eq!(
        port_names(&document, blend, Side::Input),
        ["Base", "Pose 0", "Weight 0", "Pose 1", "Weight 1", "Bias"],
        "★ ONE item and therefore TWO ports (R1632), and the fixed port past the \
         run moved by two rather than by one"
    );
    assert_eq!(
        document
            .tree(ROOT)
            .and_then(|host| host.link(made.link))
            .map(|link| (link.from, link.to)),
        Some((Socket::new(doubling, 0), Socket::new(blend, 3))),
        "★ and the end is on the port that appeared, which is the first of the \
         item's two"
    );
}

/// Nothing can hold it: a far node that produces nothing, named apart from a
/// wire the graph refuses.
fn a_far_node_that_produces_nothing_is_its_own_refusal() {
    let mut document: Document<Op> = Document::new("root");
    let doubling = node(&mut document, Op::Double);
    let sink = node(&mut document, Op::Sink);
    let made = document
        .connect(ROOT, Socket::new(doubling, 0), Socket::new(sink, 0))
        .expect("a value link");
    assert_eq!(
        document.turn(ROOT, made.link, Item::plain()),
        Err(LandError::NoRoom {
            node: sink,
            side: Side::Output,
        }),
        "★ a sink declares no output and no run to grow one — named apart from \
         `Refused`, because this is fixed by giving the node a port and that by \
         changing the wire"
    );
    assert_eq!(
        document.may_turn(ROOT, made.link).map(|_| ()),
        Err(LandError::NoRoom {
            node: sink,
            side: Side::Output,
        }),
        "★ and asking is the same call as doing, so a greyed control and a \
         refused press cannot disagree"
    );
    assert_eq!(
        document
            .tree(ROOT)
            .and_then(|host| host.link(made.link))
            .map(|link| (link.from, link.to)),
        Some((Socket::new(doubling, 0), Socket::new(sink, 0))),
        "★ and a refused turn moved nothing"
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

// ============================================ R1987 — the arriving node's wire

/// R1987 — the graph node's `autowire a newly created node`.
///
/// # What the reference is, measured at its own header
///
/// One hook on the base graph node, whose parameter is documented *the source
/// pin that caused the new node to be created (typically a drag-release context
/// menu creation)*. Its return is `void` and its supplied body is empty.
///
/// Counted over the whole tree this round, by definitions and by `override`
/// declarations, which agree: **31 overriders**, 17 in the editor source and 14
/// in plugin modules; **25 call sites**, two of which sit inside the base
/// schema's own node-from-menu action. Of the 31, only **8** ask the schema's
/// `CanCreateConnection` — **26 call `TryCreateConnection`**, which attempts the
/// wire and answers a bool. So the common shape is not choose-then-wire; it is
/// try-until-one-sticks, and what it "preferred" is observable only afterwards
/// by looking at the graph.
///
/// # What is asserted, and why each is a claim about the *reference*
///
/// Each sub-assertion below is something a caller there cannot do. The hook
/// returns nothing, so 1 and 2 have no answer to read; the empty base body is
/// indistinguishable from a scan that found nothing, so 3 has no distinction to
/// draw; 26 of the 31 never form a preference, so 4 has no rule to state; and a
/// bool cannot name what it broke, so 6 has nothing to report.
#[test]
fn engine_node_autowire_new_node() {
    a_node_that_arrives_comes_in_already_wired();
    every_port_that_refused_says_why();
    no_port_at_all_is_not_the_same_answer_as_every_port_refusing();
    the_preference_outranks_declaration_order();
    asking_and_doing_are_one_decision();
    the_link_that_gives_way_is_reported();
    a_drag_off_a_consuming_pin_is_offered_the_outputs();
}

/// A wire the hand dragged off a producing pin and has not landed yet.
fn dangling() -> (Document<Op>, NodeId) {
    let mut document = Document::new("root");
    let two = num(&mut document, 2);
    (document, two)
}

/// 1. The node arrives already wired, and the answer **names the port**.
fn a_node_that_arrives_comes_in_already_wired() {
    let (mut document, two) = dangling();
    let sink = node(&mut document, Op::Sink);
    assert!(
        document.tree(ROOT).expect("root").links().is_empty(),
        "the wire is still in the hand"
    );

    let done = document
        .autowire(ROOT, Socket::new(two, 0), Side::Output, sink)
        .expect("Sink takes a number");
    assert_eq!(done.took.port, 0);
    assert_eq!(done.took.arrival, Arrival::Unchanged);
    assert_eq!(done.displaced, None);

    let links = document.tree(ROOT).expect("root").links().to_vec();
    assert_eq!(links.len(), 1, "one wire, and it is the one that was held");
    assert_eq!(
        (links[0].from, links[0].to),
        (Socket::new(two, 0), Socket::new(sink, 0))
    );
    assert_eq!(links[0].id, done.link, "and the answer names it");
    assert!(document.validate().is_empty());
}

/// 2. When nothing takes it, **every** candidate is named with its own reason.
///
/// `Carry` presents four inputs of four different flavours and a TEXT wire is
/// refused by all of them — a control port, two types that do not cross, and a
/// number port text does not read back into. The reference's answer to this
/// whole situation is to do nothing and say nothing.
fn every_port_that_refused_says_why() {
    let mut document = Document::new("root");
    let word = node(&mut document, Op::Word("hi".to_owned()));
    let carry = node(&mut document, Op::Carry);

    let why = document
        .autowire(ROOT, Socket::new(word, 0), Side::Output, carry)
        .expect_err("no pin of Carry takes text");
    let AutowireError::NoneTakes { declined } = why else {
        panic!("Carry HAS pins, so this is not NoPorts: {why:?}");
    };
    assert_eq!(
        declined.len(),
        4,
        "one entry per port that was offered the wire, in declaration order"
    );
    assert_eq!(
        declined.iter().map(|one| one.port).collect::<Vec<_>>(),
        [0, 1, 2, 3],
        "declaration order, so a person can match them to the node they see"
    );
    for one in &declined {
        assert!(
            !one.why.to_string().is_empty(),
            "port {} declined without saying why",
            one.port
        );
    }

    // ★ And the refusals are DIFFERENT refusals, which is the whole point of
    // carrying the authoring error rather than a bit: a wire refused for a
    // control port and one refused for a type are repaired by different acts.
    let reasons: BTreeSet<String> = declined.iter().map(|one| one.why.to_string()).collect();
    assert!(
        reasons.len() > 1,
        "four ports of four flavours collapsed to one sentence: {reasons:?}"
    );
    assert!(
        document.tree(ROOT).expect("root").links().is_empty(),
        "and a refused autowire leaves the document alone"
    );
}

/// 3. *This kind never listens* and *these pins all refused* are two facts.
///
/// The reference cannot tell them apart: both are its empty hook body.
fn no_port_at_all_is_not_the_same_answer_as_every_port_refusing() {
    let mut document = Document::new("root");
    let word = node(&mut document, Op::Word("hi".to_owned()));
    let source = num(&mut document, 7);
    let carry = node(&mut document, Op::Carry);

    // A source has no inputs at all, so there is nothing to offer the wire.
    let none = document
        .autowire(ROOT, Socket::new(word, 0), Side::Output, source)
        .expect_err("Num presents no input");
    assert!(
        matches!(none, AutowireError::NoPorts { node, side } if node == source && side == Side::Input),
        "the arm names the node and the side it has none on: {none:?}"
    );
    assert!(
        none.to_string().contains(Side::Input.noun()),
        "and says so to a person, in the prose word rather than the wire \
         token: {none}"
    );

    // Same wire, same gesture, a node that HAS pins — a different arm.
    let refused = document
        .autowire(ROOT, Socket::new(word, 0), Side::Output, carry)
        .expect_err("Carry's pins all refuse text");
    assert!(matches!(refused, AutowireError::NoneTakes { .. }));
    assert_ne!(
        std::mem::discriminant(&none),
        std::mem::discriminant(&refused),
        "★ the two facts must not be one arm"
    );
}

/// 4. The preference is one derivation, and it **outranks declaration order**.
///
/// A definition exposing a TEXT input first and a NUMBER input second, so the
/// better candidate is the *later* one: a number crosses into the number port
/// unchanged and into the text port only through the taxonomy's declared
/// conversion. Ordering by declaration — which is what the reference's
/// try-until-one-sticks shape effectively does — would take the text port.
fn the_preference_outranks_declaration_order() {
    let mut document = Document::new("root");
    let two = num(&mut document, 2);
    let definition = document.add_definition("Two ways in");
    document
        .expose(
            definition,
            InterfaceSide::Input,
            Port::new("word", Ty::Text),
        )
        .expect("a text way in");
    document
        .expose(
            definition,
            InterfaceSide::Input,
            Port::new("count", Ty::Number),
        )
        .expect("and a number one");
    let arriving = document
        .instantiate(ROOT, definition, 300, 0)
        .expect("an instance of it");

    let offered = document
        .autowire_uptakes(ROOT, Socket::new(two, 0), Side::Output, arriving)
        .expect("both ways in take a number, one of them by conversion");
    assert_eq!(
        offered.len(),
        2,
        "every port that would take it, not just one"
    );
    assert_eq!(
        (offered[0].port, offered[0].arrival),
        (1, Arrival::Unchanged),
        "★ the number port is SECOND in declaration order and FIRST in preference"
    );
    assert_eq!(
        (offered[1].port, offered[1].arrival),
        (0, Arrival::Converted),
        "and the text port takes it through the declared conversion"
    );
    assert!(offered[0].preference() < offered[1].preference());

    // The verb agrees with the ranking rather than with the declaration.
    let done = document
        .autowire(ROOT, Socket::new(two, 0), Side::Output, arriving)
        .expect("it wires");
    assert_eq!(done.took.port, 1);
}

/// 5. Asking and doing are **one** decision, not two that have to agree.
fn asking_and_doing_are_one_decision() {
    let (mut document, two) = dangling();
    let sink = node(&mut document, Op::Sink);
    let socket = Socket::new(two, 0);

    let asked = document
        .may_autowire(ROOT, socket, Side::Output, sink)
        .expect("a port would take it");
    assert!(
        document.tree(ROOT).expect("root").links().is_empty(),
        "★ asking changed nothing"
    );
    let done = document
        .autowire(ROOT, socket, Side::Output, sink)
        .expect("and doing it works");
    assert_eq!(asked, done.took, "★ the question and the verb are one call");

    // The refusing direction too: what `may_autowire` refuses, `autowire`
    // refuses with the same words. Two implementations would be free to drift.
    let mut other = Document::new("root");
    let word = node(&mut other, Op::Word("hi".to_owned()));
    let carry = node(&mut other, Op::Carry);
    let from = Socket::new(word, 0);
    let asked = other
        .may_autowire(ROOT, from, Side::Output, carry)
        .expect_err("nothing takes text");
    let done = other
        .autowire(ROOT, from, Side::Output, carry)
        .expect_err("and the verb refuses too");
    assert_eq!(asked.to_string(), done.to_string());
}

/// 6. The link that **gives way** is named, which is what makes it undoable.
///
/// A value input holds one link, so wiring a second displaces the first. The
/// reference's connection attempt answers a bare bool, so what it broke is
/// simply gone.
fn the_link_that_gives_way_is_reported() {
    let mut document = Document::new("root");
    let two = num(&mut document, 2);
    let three = num(&mut document, 3);
    let sink = node(&mut document, Op::Sink);
    wire(&mut document, two, 0, sink, 0);
    let standing = document
        .tree(ROOT)
        .and_then(|tree| tree.links().last().copied())
        .expect("the standing wire");

    // Asked BEFORE anything moves, which is the point: a screen can warn.
    let asked = document
        .may_autowire(ROOT, Socket::new(three, 0), Side::Output, sink)
        .expect("Sink's only input takes it");
    assert_eq!(
        asked.displaces,
        Some(standing),
        "★ and says which wire would go, before it goes"
    );

    let done = document
        .autowire(ROOT, Socket::new(three, 0), Side::Output, sink)
        .expect("it wires");
    assert_eq!(done.displaced, Some(standing));
    let links = document.tree(ROOT).expect("root").links().to_vec();
    assert_eq!(links.len(), 1, "the input still holds exactly one");
    assert_eq!(links[0].from, Socket::new(three, 0));
    assert!(document.validate().is_empty());
}

/// 7. The gesture works in **both** directions.
///
/// A wire dragged off a *consuming* pin is offered the arriving node's outputs.
/// The reference reads the direction off the one pin pointer it is handed; here
/// the caller says which list the index belongs to, so the two ends cannot be
/// confused for one another.
fn a_drag_off_a_consuming_pin_is_offered_the_outputs() {
    let mut document = Document::new("root");
    let sink = node(&mut document, Op::Sink);
    let add = node(&mut document, Op::Add);

    let done = document
        .autowire(ROOT, Socket::new(sink, 0), Side::Input, add)
        .expect("Add's output takes it");
    assert_eq!(done.took.port, 0, "Add's Out");
    let links = document.tree(ROOT).expect("root").links().to_vec();
    assert_eq!(
        (links[0].from, links[0].to),
        (Socket::new(add, 0), Socket::new(sink, 0)),
        "★ and the wire is oriented producer-to-consumer, not mirrored"
    );
    assert!(document.validate().is_empty());
}

/// ★★★★★ R1999 — a taxonomy whose graphs come in **kinds**, and whose node
/// kinds declare where they are at home.
///
/// Three answers and not two, because [`Admitted`] is wider than the graph-kind
/// vocabulary: naming graph kinds produces `Anything` and a non-empty `These`,
/// and the third shape — a kind at home in **no** graph — cannot be reached by
/// naming any of them. That is R1998's carry (R1845's eighth) applied before
/// the fixture is written rather than after a counterfactual passes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
enum Plane {
    /// The ordinary graph, and the one an unclassified tree is.
    #[default]
    Data,
    /// A graph that is instantiated, so anything holding a unique name is wrong
    /// in it.
    Template,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Placed {
    /// At home anywhere: the supplied answer, and what a kind that has not
    /// thought about it says.
    Anywhere,
    /// At home in one kind of graph only.
    DataOnly,
    /// At home in none — declarable, and unreachable by naming graph kinds.
    Nowhere,
}

impl NodeKind for Placed {
    type Type = Ty;
    type Value = Val;
    type Graph = Plane;

    fn at_home(&self) -> Admitted<Plane> {
        match self {
            Self::Anywhere => Admitted::Anything,
            Self::DataOnly => Admitted::These(vec![Plane::Data]),
            Self::Nowhere => Admitted::These(Vec::new()),
        }
    }

    /// ★★★★★ R2003 — a zone whose opener is welcome anywhere and whose closer
    /// is welcome nowhere.
    ///
    /// Declared HERE rather than in a taxonomy of its own because this is the
    /// fixture that already has a kind at home in no graph, and the point being
    /// made needs exactly that: a zone is TWO kinds, so where a zone may be
    /// opened is not answered by asking about its opener. Nothing in R1999's
    /// proofs reads this hook, so adding it cannot move them.
    fn closed_by(&self) -> Option<Self> {
        match self {
            Self::Anywhere => Some(Self::Nowhere),
            _ => None,
        }
    }

    fn name(&self) -> String {
        match self {
            Self::Anywhere => "Anywhere".into(),
            Self::DataOnly => "DataOnly".into(),
            Self::Nowhere => "Nowhere".into(),
        }
    }

    fn inputs(&self) -> Vec<Port<Ty, Val>> {
        vec![Port::new("In", Ty::Number)]
    }

    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        vec![Port::new("Out", Ty::Number)]
    }

    fn evaluate(&self, inputs: &[Option<Val>]) -> Vec<Option<Val>> {
        vec![inputs.first().cloned().flatten()]
    }
}

/// ★★★★★ R1999 — the schema's `GetGraphType`: **a tree says what kind of graph
/// it is**, in the taxonomy's own vocabulary, and one declaration per node kind
/// says which of those kinds it is at home in.
///
/// Measured at the reference, three things that each changed what is built:
/// its vocabulary is a **fixed five-member enumeration** written for one editor
/// and the comment directly above the hook says so in its own words; the
/// supplied body **ignores the graph it is handed** and answers the first
/// member, so *this is a function graph* and *I could not classify this* — and
/// *there is no graph* — are one value; and the largest group of its 53
/// consumers is the per-node-type *are you compatible with this graph* test —
/// sixteen calls in fifteen node classes, four times the next largest group,
/// each re-writing the same comparison.
///
/// The assertions that would fail if the capability were missing:
///
/// 1. a tree answers its kind, and a tree that is not there answers **nothing**
///    rather than a kind;
/// 2. ★ a definition is born of a stated kind, and the unstated one is the
///    TAXONOMY's default rather than this crate's;
/// 3. ★ the placement is refused, by the kind's own declaration, and the
///    refusal names both the kind and the graph;
/// 4. ★ the offer is the SAME predicate as the refusal, so a chooser cannot
///    offer what an edit would refuse;
/// 5. ★ a re-classification says what it left behind, and `validate` reports
///    it — a pass the reference does not make at all;
/// 6. the kind survives a save and a re-open, and a document written before the
///    field existed reads back as the taxonomy's default.
#[test]
fn engine_schema_get_graph_type() {
    let mut document: Document<Placed> = Document::new("root");

    // ★ (1) The root says what it is, and nothing is not a kind.
    assert_eq!(
        document.graph_kind(ROOT),
        Some(&Plane::Data),
        "the root is the taxonomy's unchosen kind"
    );
    assert_eq!(
        document.graph_kind(TreeId(77)),
        None,
        "★ and a tree that is not there answers NOTHING — the reference's own \
         hook ignores its argument and answers the first member of its \
         enumeration for a graph that does not exist"
    );

    // ★ (2) Born of a stated kind; the unstated one is the taxonomy's.
    let template = document.add_definition_of("Pattern", Plane::Template);
    let plain = document.add_definition("Plain");
    assert_eq!(document.graph_kind(template), Some(&Plane::Template));
    assert_eq!(
        document.graph_kind(plain),
        Some(&Plane::Data),
        "★ the TAXONOMY's unchosen kind and not a member this crate picked"
    );

    // ★ (3) The refusal, and it names both halves.
    let refused = document
        .add_node(template, NodeBody::Kind(Placed::DataOnly), 0, 0)
        .expect_err("a data-only kind is not at home in a template");
    assert_eq!(
        refused,
        EditError::KindNotAdmitted {
            tree: template,
            kind: "DataOnly".to_owned(),
            graph: "Template".to_owned(),
        },
        "★ which kind, and which graph — the reference's per-node hook answers \
         one bool and names neither"
    );
    let said = refused.to_string();
    assert!(
        said.contains("DataOnly") && said.contains("Template"),
        "and the sentence carries both: {said}"
    );

    // The same kind, in the graph it IS at home in.
    let welcome = document
        .add_node(plain, NodeBody::Kind(Placed::DataOnly), 0, 0)
        .expect("★ the rule is the TEMPLATE's, not a blanket refusal");
    assert!(document.tree(plain).expect("plain").node(welcome).is_some());

    // A kind that declares nothing is at home in both.
    for tree in [template, plain] {
        document
            .add_node(tree, NodeBody::Kind(Placed::Anywhere), 40, 0)
            .expect("the supplied answer lets a kind through everywhere");
    }
    // And the third shape: at home in none, which naming graph kinds cannot
    // produce and so nothing else in this test would have reached.
    for tree in [ROOT, template, plain] {
        assert!(
            document
                .add_node(tree, NodeBody::Kind(Placed::Nowhere), 80, 0)
                .is_err(),
            "★ an empty declaration is a real answer and not the same as \
             `Anything` — tree {tree} took a kind at home nowhere"
        );
    }

    // ★ (4) The offer is the refusal, asked as a question.
    for (tree, kind, want) in [
        (template, Placed::DataOnly, false),
        (plain, Placed::DataOnly, true),
        (template, Placed::Anywhere, true),
        (template, Placed::Nowhere, false),
        (TreeId(77), Placed::Nowhere, true),
    ] {
        assert_eq!(
            document.at_home(tree, &kind),
            want,
            "★ what a palette filters with is what `add_node` refuses on: \
             {kind:?} in tree {tree}"
        );
    }

    a_re_classification_says_what_it_left_behind(&mut document, plain, welcome);
    the_kind_survives_the_file(&document, template, plain);
}

/// Claim (5) — **a re-classification says what it left behind**, and the
/// document reports it.
///
/// `add_node` cannot produce this state; `set_graph_kind` can, and deleting the
/// person's work instead is what it deliberately does not do. The reference
/// has no in-place re-classification AT ALL — measured at R1999 over every site
/// that mutates one of the three lists a type is read off: the one place that
/// picks a list from a type is choosing for a freshly DUPLICATED graph, one
/// removes the graph from every list at once (deletion), and the rest reorder
/// inside a single list. So *what did that leave behind* is not a question
/// answered badly there; it is a question that cannot be asked.
fn a_re_classification_says_what_it_left_behind(
    document: &mut Document<Placed>,
    plain: TreeId,
    welcome: NodeId,
) {
    assert!(
        document.not_at_home(plain).is_empty(),
        "nothing is out of place yet"
    );
    assert!(document.validate().is_empty(), "and the document is clean");
    document
        .set_graph_kind(plain, Plane::Template)
        .expect("a tree that is there");
    assert_eq!(
        document.not_at_home(plain),
        vec![welcome],
        "★ the card the new kind does not admit, still there"
    );
    assert!(
        document.tree(plain).expect("plain").node(welcome).is_some(),
        "★ and nothing was deleted, which would have taken its links with it"
    );
    assert!(
        document.validate().contains(&Violation::NotAtHome {
            tree: plain,
            node: welcome,
        }),
        "★ one predicate, asked of an edit and of a whole document: {:?}",
        document.validate()
    );
    assert_eq!(
        document.set_graph_kind(TreeId(77), Plane::Data),
        Err(EditError::NoSuchTree(TreeId(77))),
    );
}

/// Claim (6) — **the kind survives the file**, and a document written before
/// the field existed reads back as the taxonomy's default rather than failing
/// to load.
fn the_kind_survives_the_file(document: &Document<Placed>, template: TreeId, plain: TreeId) {
    let text = serde_json::to_string(document).expect("a document is writable");
    let read: Document<Placed> = serde_json::from_str(&text).expect("and readable");
    assert_eq!(read.graph_kind(template), Some(&Plane::Template));
    assert_eq!(
        read.graph_kind(plain),
        Some(&Plane::Template),
        "the re-classification above, round-tripped"
    );

    let mut older = serde_json::from_str::<serde_json::Value>(&text).expect("json");
    for tree in older["trees"].as_array_mut().expect("trees") {
        tree.as_object_mut().expect("a tree").remove("kind");
    }
    let older: Document<Placed> =
        serde_json::from_value(older).expect("★ a document written before the field still loads");
    assert_eq!(
        older.graph_kind(template),
        Some(&Plane::Data),
        "★ as the taxonomy's unchosen kind"
    );
}

/// ★★★★★ R2001 — the graph node's `CanUserEditPinAdvancedViewFlag`: **a class
/// of port folded away behind one control, and who may say which ports are in
/// it.**
///
/// # What the reference does, measured at its own source
///
/// Three things carry it there and the census row names the third: a bit on
/// each pin; a stored tri-state on the node — *no advanced pins* / *shown* /
/// *hidden* — that the chevron writes; and this virtual on the node class,
/// whose base answers no and which **two** classes in the whole tree override.
///
/// Read at its one consumer, that virtual is not about a menu. It sits in the
/// routine that carries a pin's persistent data across a rebuild of the node's
/// pins and decides whether the old pin's advanced bit is copied forward, with
/// the comment *"Otherwise we don't want to copy this, or we'd be ignoring new
/// metadata that tries to hide old pins."* The flag exists because there a
/// declaration and a person's choice are **the same storage**.
///
/// # What would differ if the capability were missing
///
/// Four things, and each is asserted below:
///
/// * a declared advanced port would be on the frame like any other, so
///   declaring one would mean nothing;
/// * a folded class would hide a socket a wire ends on — the reference's own
///   *not connected* guard, and the one rule here that is a reproduction
///   rather than an improvement;
/// * a person's classification and the kind's declaration would share one slot,
///   so *put it back the way the kind declares it* would have nothing to put
///   back;
/// * a kind that keeps its classes would find out it had been overruled only
///   afterwards.
#[test]
fn engine_node_can_user_edit_pin_advanced_view_flag() {
    let mut document: Document<Op> = Document::new("root");
    let rig = document
        .add_node(ROOT, NodeBody::Kind(Op::Rig), 0, 0)
        .unwrap();
    let tuned = document
        .add_node(ROOT, NodeBody::Kind(Op::Tuned), 0, 100)
        .unwrap();
    let feed = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(7)), 0, 200)
        .unwrap();

    // 1. The declaration folds a port away, and the node says it has a control.
    let folded = document.visible_ports(ROOT, rig).unwrap();
    assert_eq!(folded.inputs, vec![0], "`Value` is on the frame");
    assert_eq!(folded.advanced_inputs, vec![1], "`Trim` is folded away");
    assert_eq!(
        folded.why_hidden(Side::Input, 1),
        Some(Hidden::Advanced),
        "★ and it says WHICH reason, which the reference publishes for none of \
         its three: its pin widget answers one conjunction",
    );
    assert_eq!(
        document.advanced_view(ROOT, rig),
        Some(AdvancedView::Folded)
    );
    assert_eq!(
        document.advanced_ports(ROOT, rig),
        Some(vec![PortRef::input(1)]),
    );

    // 2. ★★★★★ THE REFERENCE'S OWN RULE, reproduced: a wire ending on an
    //    advanced port keeps it on the frame however the group is folded.
    document
        .connect(ROOT, Socket::new(feed, 0), Socket::new(rig, 1))
        .expect("Out: Number reaches Trim: Number");
    assert_eq!(
        document.visible_ports(ROOT, rig).unwrap().inputs,
        vec![0, 1],
        "★★★★★ folding a class must not hide a socket a wire ends on",
    );
    let link = document.tree(ROOT).unwrap().links()[0].id;
    document.disconnect(ROOT, link).expect("the only link");

    // 3. ★★★★★ WHAT THE REFERENCE CANNOT DO. A person's classification lives
    //    apart from the kind's declaration, so *say nothing again* is a real
    //    third answer and the declaration is still there to go back to.
    assert_eq!(
        document.classified(ROOT, rig, PortRef::input(1)),
        Some(Classified {
            class: PortClass::Advanced,
            source: ClassSource::Kind,
        }),
        "★ the answer says WHO, so an editor knows whether there is anything \
         to put back",
    );
    assert_eq!(
        document
            .classify_port(ROOT, rig, PortRef::input(1), Classify::Plain)
            .expect("`Rig` hands its classes to a person"),
        Classified {
            class: PortClass::Plain,
            source: ClassSource::Person,
        },
    );
    assert_eq!(
        document.advanced_view(ROOT, rig),
        Some(AdvancedView::Nothing),
        "★★★★★ and *this node has nothing advanced* is DERIVED: the reference \
         stores it, twenty sites promote it by hand and five demote it, so a \
         node that stops having advanced pins keeps drawing the control",
    );
    document
        .classify_port(ROOT, rig, PortRef::input(1), Classify::Declared)
        .expect("and back");
    assert_eq!(
        document.classified(ROOT, rig, PortRef::input(1)),
        Some(Classified {
            class: PortClass::Advanced,
            source: ClassSource::Kind,
        }),
        "★★★★★ the kind's declaration survived a person disagreeing with it, \
         because the two were never one slot",
    );

    // 4. A kind that keeps its classes refuses, and says so before the act —
    //    the same call a screen greys the gesture with.
    let refusal = ClassifyError::KindDecides {
        kind: "Tuned".to_owned(),
    };
    assert_eq!(
        document.may_classify_port(ROOT, tuned, PortRef::input(1), Classify::Plain),
        Err(refusal.clone()),
    );
    assert_eq!(
        document.classify_port(ROOT, tuned, PortRef::input(1), Classify::Plain),
        Err(refusal),
        "★ the edit is a call site of the question, not a second copy of it",
    );
    assert_eq!(
        document.advanced_view(ROOT, tuned),
        Some(AdvancedView::Folded),
        "★ what is refused is a person's disagreement, not the class itself",
    );
}

/// ★★★★★ R2004 — the animation editor's **self-transition** command, and the
/// row's own sentence measured wrong.
///
/// The pin read *a link whose source and sink are the same node*, and named two
/// obstacles: `connect` refuses a self-link, and the control plane has no
/// self-edge constructor. Both are true. **Neither is what the reference's
/// operator does.** Read from `FAnimationBlueprintEditor::OnCreateSelfTransition`,
/// it never makes a self-edge: it creates an **alias node**, runs a name
/// validator to make `Self` unique, places it at `+200, -100` from the state,
/// puts that state into the alias's aliased-state **set**, and links alias to
/// state. The self-loop is what that link *expands to*.
///
/// So the capability the row names is a node that stands in for a SET, and this
/// command is its one-element case. Its own baker says the mechanism in a
/// comment — *"Alias's are simply decompiled into multiple connections."*
///
/// ★★★★★ And the aliased states are a `TSet`, with a *global* flag beside it
/// and a `GetAliasedState()` documented *Returns null if aliasing more than one
/// state* — so the general mechanism was there all along and the command is a
/// canned use of it. Building the command alone would have reproduced the
/// canned case and missed the capability.
#[test]
fn engine_anim_graph_create_self_transition() {
    let mut document: Document<Op> = Document::new("root");
    let stage = document
        .add_node(ROOT, NodeBody::Kind(Op::Stage(3)), 30, 70)
        .expect("root tree");

    // (A) The obstacle the row named is REAL and stays. A node feeding itself
    // with nothing declaring that it was meant is a mistake.
    assert_eq!(
        document
            .connect(ROOT, Socket::new(stage, 0), Socket::new(stage, 0))
            .unwrap_err(),
        ConnectError::SelfLink(stage),
    );

    // (B) The reference's four steps, in one verb.
    let stood = document
        .stand_in_for(ROOT, stage)
        .expect("a stage has a control port each side");
    let card = document.tree(ROOT).unwrap().node(stood.stand_in).unwrap();
    assert_eq!(
        (card.x, card.y),
        (230, -30),
        "★ placed at the reference's own offset from the card it stands for"
    );
    assert_eq!(
        document.stands_alone(ROOT, stood.stand_in),
        Some(Alone::Yes(stage)),
        "★ standing for exactly the one node — the command's one-element case"
    );

    // (C) ★★★★★ And the link MEANS the self-loop, which is the row.
    let expanded = document.expanded_links(ROOT);
    assert_eq!(expanded.len(), 1);
    assert_eq!(
        (expanded[0].from.node, expanded[0].to.node),
        (stage, stage),
        "★★★★★ the loop `connect` refuses to author is what the stand-in \
         declares was meant"
    );
    assert_eq!(
        document.control_loops(ROOT),
        vec![stage],
        "★ and the cycle derivation walks the EXPANSION, so the loop is visible \
         to every reader without any of them knowing about stand-ins"
    );
    assert!(document.validate().is_empty(), "★ and it is not a fault");

    // (D) ★ The command's alias is a transition SOURCE, and widening it stays
    // legal — measured rather than assumed, and the first draft of this proof
    // asserted the opposite: the expansion piles onto a control INPUT, which
    // R1599 derives as holding many predecessors. So the very command this row
    // names lands on the half its own validator permits, and building only the
    // command would never have reached the other half.
    let second = document
        .add_node(ROOT, NodeBody::Kind(Op::Stage(5)), 30, 200)
        .expect("root tree");
    document
        .represent(ROOT, stood.stand_in, second)
        .expect("★ a control input takes many predecessors");
    assert_eq!(
        document
            .expanded_links(ROOT)
            .into_iter()
            .filter(|held| held.to.node == stage)
            .count(),
        2,
        "★★★★★ one wire drawn, two meant — the reference's own sentence for its \
         alias, as a reading rather than as a step inside a compile"
    );
    assert_eq!(
        document.stands_alone(ROOT, stood.stand_in),
        Some(Alone::Several(2)),
        "★ and *there is no single one* is an answer with a reason, where the \
         reference returns the same null it returns for a deleted state"
    );

    // (E) ★★★★★ The other half, which IS the reference's hand-written
    // validator: *an alias used as a transition's TARGET must alias a single
    // state*. Nothing here says that. The links pile onto the socket at the far
    // end, and there that socket is a control OUTPUT, which holds one
    // successor — so `Flow::multiplicity` refuses the edit, and the rule is a
    // theorem rather than a message discovered at compile time.
    let target = document
        .add_node(
            ROOT,
            NodeBody::StandIn(Represented::Named(BTreeSet::from([stage]))),
            400,
            0,
        )
        .expect("root tree");
    document
        .connect(ROOT, Socket::new(second, 0), Socket::new(target, 0))
        .expect("one member, so the control output feeds one successor");
    assert_eq!(
        document.represent(ROOT, target, second).unwrap_err(),
        StandInError::WouldCrowd {
            tree: ROOT,
            stand_in: target,
            socket: Socket::new(second, 0),
            side: Side::Output,
            would_be: 2,
        },
        "★★★★★ derived from `Multiplicity`, not written down — and it names the \
         SOCKET, which the reference's message does not carry"
    );
}

/// ★★★★★ R2005 — the material editor's **Create Reroute Usage** command: one
/// more far end of a name that already exists.
///
/// The fifth operator over R1935's named pair, and the one that GROWS the far
/// end set — where the other four read the two directions and convert both
/// ways. The model was already here; what was absent was the verb.
///
/// ★★★★★ THREE THINGS MEASURED ABOUT ITS COMMAND, each deciding a piece:
///
/// * **It has no activation condition.** `OnCreateRerouteUsageFromDeclaration`
///   is bound TWICE as a bare `FExecuteAction` with no `FCanExecuteAction`, and
///   the only thing standing between it and a node it does nothing for is an
///   `IsA(...Declaration)` test in the context-menu builder. So *may I* is a
///   question that exists only where the menu is drawn.
/// * **It stacks cards on one point.** The new usage goes to
///   `NodePosX + 150, NodePosY` unconditionally, so a second call lands it
///   exactly on the first and nothing reports it.
/// * **It clears the selection and selects nothing.** `ClearSelectionSet()`
///   runs first and the node it makes is never selected, so a person is left
///   with a new card and no indication which it is.
///
/// ★ And it writes TWO addresses for one referent — a `Declaration` pointer and
/// a copy of that declaration's guid — which R1935 already does not.
#[test]
fn engine_material_editor_create_reroute_usage_from_declaration() {
    let mut document: Document<Op> = Document::new("root");
    let source = node(&mut document, Op::Num(5));
    let beacon = document
        .add_node(ROOT, NodeBody::Beacon, 40, 70)
        .expect("root tree");
    wire(&mut document, source, 0, beacon, 0);

    // (A) The question the reference does not have, and the verb ASKS it rather
    // than repeating it.
    assert_eq!(document.may_echo_beacon(ROOT, beacon), Ok(()));

    // (B) The command itself, at the reference's own offset.
    let first = document
        .echo_beacon(ROOT, beacon)
        .expect("a named endpoint echoes");
    assert_eq!(first.at, (190, 70), "★ its own +150, same Y");
    assert!(first.past.is_empty());
    assert_eq!(
        document.beacon_of(ROOT, first.echo),
        Some(beacon),
        "★ bound to the endpoint by ONE address, a `NodeId` — not a pointer and \
         a guid that can come apart"
    );
    assert_eq!(
        document.evaluate(ROOT, first.echo),
        vec![Some(Val::Number(5))],
        "★★★★★ and the value crosses the canvas to it over NO wire, which is \
         what the pair is for"
    );

    // (C) ★★★★★ Twice, and the second does not land on the first.
    let second = document.echo_beacon(ROOT, beacon).expect("and another");
    assert_ne!(
        second.at, first.at,
        "★★★★★ the reference puts both on the same point"
    );
    assert_eq!(
        second.past,
        vec![first.echo],
        "★ and what it stepped past is NAMED, which a fixed offset cannot say"
    );
    assert_eq!(
        document.echoes_of(ROOT, beacon),
        vec![first.echo, second.echo],
        "★ both belong to the name, which is the reading the verb grew"
    );

    // (D) ★ Asked of the OTHER half, the refusal carries the repair — where the
    // reference simply does not offer the menu entry.
    assert_eq!(
        document.may_echo_beacon(ROOT, first.echo),
        Err(BeaconError::NotTheEndpoint {
            node: first.echo,
            endpoint: Some(beacon),
        }),
    );
    assert_eq!(
        document.may_echo_beacon(ROOT, source),
        Err(BeaconError::NotNamed(source)),
        "★ and a node that is neither half is a DIFFERENT refusal, because \
         there is nothing to redirect to"
    );
    assert!(document.validate().is_empty());
}

/// A taxonomy with a HISTORY, for the migration proof below (R2006).
///
/// Its own and not `Op`'s, because `Op` takes `NodeKind::version`'s default of
/// zero — which is what a taxonomy that has never changed should say, and which
/// the proof asserts stays true for it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
enum Era {
    /// What documents held before step 1.
    Before,
    /// What step 1 makes — and what step 3 exists to repair.
    Between,
    /// What step 3 makes.
    After,
}

impl NodeKind for Era {
    type Type = Ty;
    type Value = Val;
    type Graph = ();

    fn name(&self) -> String {
        match self {
            Self::Before => "Before".to_owned(),
            Self::Between => "Between".to_owned(),
            Self::After => "After".to_owned(),
        }
    }

    fn inputs(&self) -> Vec<Port<Ty, Val>> {
        vec![Port::new("In", Ty::Number)]
    }

    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        vec![Port::new("Out", Ty::Number)]
    }

    fn evaluate(&self, inputs: &[Option<Val>]) -> Vec<Option<Val>> {
        vec![inputs.first().cloned().flatten()]
    }

    fn version() -> u32 {
        3
    }

    fn at_step(&self, step: u32) -> Option<Self> {
        match (step, self) {
            (1, Self::Before) => Some(Self::Between),
            (3, Self::Between) => Some(Self::After),
            _ => None,
        }
    }
}

/// ★★★★★ R2006 — the engine's **backward-compatibility node conversion**: a
/// document's node kinds brought up to date at load, one version step at a
/// time.
///
/// ★★★★★ FOUR THINGS MEASURED ABOUT ITS HOOK, and the first two decided the
/// whole shape:
///
/// 1. **It carries no version.** The virtual takes a graph and a bool, nothing
///    more, so every implementor has to fetch a version for itself out of the
///    serialisation linker. Of the **two** that implement it, only ONE does —
///    the other runs its conversions unconditionally on every load, forever,
///    for every document however new. A version that each implementor decides
///    to consult is a version half of them will not.
/// 2. **The one that does branches instead of composing.** It is written
///    `if (v < 21) { four conversions } else if (v < 24) { one }`, and the
///    declaration of step 24 carries a comment saying documents brought up to
///    date by step 21 may end up with a wrong default for one parameter. So
///    step 24 exists to repair what step 21 produces — and a document at
///    version 10 takes step 21 and is then excluded by the `else` from the
///    repair it has just earned. **The document that most needs it is exactly
///    the one skipped.**
/// 3. **It answers `void`** and writes its failures to a warning log, so
///    *nothing was needed* and *four things happened* reach a caller the same.
/// 4. **Its `bOnlySafeChanges` parameter is `true` at its only call site**, so
///    the unsafe mode is unreachable — a knob with one position.
///
/// Here the version belongs to the MECHANISM: `NodeKind::version` is stamped on
/// every archive beside the format's own revision, `Document::migrate` runs
/// every step between the two in ascending order, and the report is a value.
#[test]
fn engine_schema_backward_compatibility_node_conversion() {
    // (A) A document written three versions ago, and a kind no step names.
    let mut document: Document<Era> = Document::new("root");
    let aged = document
        .add_node(ROOT, NodeBody::Kind(Era::Before), 0, 0)
        .expect("root tree");
    let untouched = document
        .add_node(ROOT, NodeBody::Kind(Era::After), 0, 100)
        .expect("root tree");

    let ran = document.migrate(0);

    // (B) ★★★★★ THE COMPOSITION the reference's `else if` breaks.
    assert_eq!(
        document.tree(ROOT).unwrap().node(aged).unwrap().body,
        NodeBody::Kind(Era::After),
        "★★★★★ step 1 made it Between and step 3 then repaired THAT — the \
         reference's branch stops after the first and the repair never runs"
    );
    assert_eq!(
        document.tree(ROOT).unwrap().node(untouched).unwrap().body,
        NodeBody::Kind(Era::After),
        "★ and a node no step names is left where it was"
    );

    // (C) ★★★★★ THE REPORT IS A VALUE, where the reference answers void.
    assert_eq!((ran.from, ran.to), (0, 3));
    assert_eq!(
        ran.steps.iter().map(|s| s.step).collect::<Vec<_>>(),
        vec![1, 3],
        "★ the steps that DID work, ascending — step 2 was offered and changed \
         nothing, so it is not listed"
    );
    assert_eq!(
        ran.touched(),
        vec![aged],
        "★ two steps, one node, counted once"
    );

    // (D) ★ A document already current is left alone AND SAYS SO, which the
    // reference's unversioned implementor cannot: it re-converts every load.
    let quiet = document.migrate(3);
    assert!(quiet.is_empty());
    assert_eq!(quiet.touched(), []);

    // (E) ★★ The version is the MECHANISM's, stamped beside the format's own,
    // rather than something each implementor fetches for itself.
    let text = Archive::<Era>::of(document).write().expect("representable");
    assert!(text.contains("\"taxonomy\": 3") && text.contains("\"revision\": 1"));
    assert_eq!(
        Archive::<Era>::read(&text).taxonomy_version(),
        Some(3),
        "★ and a reader gets back what it must migrate FROM"
    );

    // (F) ★ A taxonomy that has never changed says so, and migrating it is a
    // no-op rather than a pass that rewrites what it walks.
    assert_eq!(<Op as NodeKind>::version(), 0);
    let mut plain: Document<Op> = Document::new("root");
    let add = node(&mut plain, Op::Add);
    assert!(plain.migrate(0).is_empty());
    assert_eq!(
        plain.tree(ROOT).unwrap().node(add).unwrap().body,
        NodeBody::Kind(Op::Add)
    );
}
