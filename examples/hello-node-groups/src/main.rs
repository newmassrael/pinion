//! R1577 §5.38 §5.52 — a DCC-class node system, composed.
//! R1578 — and its clipboard, which is a `Fragment` held in a signal.
//!
//! # What this example is for
//!
//! `hello-node-editor` is nine thousand lines and owns its own graph model,
//! its own edit machinery and its own evaluator. This one owns **none of
//! those**. It supplies a taxonomy — five material ops — and a way to draw a
//! box, and everything that makes it a *node system* comes from
//! [`pinion_node_graph`]: the typed model, the structural edits and their
//! invariants, re-usable group definitions with a **derived** interface, the
//! nesting rule, the edit path, and evaluation that descends into groups.
//!
//! So its length is the argument. What is absent here is what an application
//! no longer has to write.
//!
//! # What it demonstrates over RPC
//!
//! Every claim the substrate makes is readable as data: the definition library
//! and its instance counts, the derived interface of each definition, the edit
//! path, the value at any node, and — the one an editor is judged by — the
//! **text of the last refusal**, which names the wires that caused it.
//!
//! R1578 adds the clipboard to that list: what is held, which wires were
//! severed to hold it, how many bytes it serializes to, and what the last
//! insertion did — all readable without pasting.
//!
//! R1586 adds the other half of an editor's daily work: taking a stage OUT of
//! the pipeline. A node can be **bypassed** — it stops computing and the
//! values at its inputs pass through it — or **dissolved**, which does the
//! same thing to the structure and deletes it. Both read one derivation, so
//! the preview and the edit cannot disagree; `passthrough.<id>` publishes it, including the
//! outputs no input can feed, which is the value an author most needs told is
//! about to disappear. A **link** can be muted, which is the opposite
//! behaviour — the value stops — and so is a different word here than in the
//! DCC, where both are "mute".
//!
//! R1584 adds the two boundary moves, and with them the fact an editor is
//! obliged to show and the DCC does not: a group definition is *shared*, so
//! moving a node into one through this instance changes every other instance
//! too. `last_move` says which ports appeared, which disappeared, which links
//! died and where, and how many other instances came along — or `fork` first,
//! and none of them do.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    ArgCase, ArgForm, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner,
    SchemaArg, SchemaField, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{
    BoxNode, ContainerNode, PathCommand, PathNode, PathPoint, Rect, TextNode,
};
use pinion_core::style::{
    Border, BoxStyle, Color, Dash, LayoutStyle, PathStyle, Size, Stroke, TextStyle,
};
use std::collections::BTreeSet;

use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_node_graph::{
    Align, ArrangePass, ArrangeTail, Axis, Conversion, Crossings, Definitions, Distribute,
    Document, Edge, EditPath, Enframed, Extent, Fragment, Grow, Inserted, InterfaceSide, Item,
    ItemEdit, ItemEditTail, LinkId, Node, NodeBody, NodeId, NodeKind, Orphaned, Port, PortChange,
    PortRef, ROOT, Reach, Repartitioned, Rewired, Severed, Sharing, Side, Socket, Stack,
    Straighten, TreeId, Variadic,
};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloNodeGroupsRenderer, HelloNodeGroupsRendererError);

const THEME_TAG: &str = "app";
const VIEW_TAG: &str = "nodegroups";
const STATE_KEY: &str = "nodegroups-state";

// ---------------------------------------------------------------------------
// R1642 — the two conditional verbs declare their cases.
//
// `arrange` and `item` both read a trailing segment whose presence, meaning and
// vocabulary depend on the FIRST segment. R1638 declared `arrange`'s as one
// `{string, open, optional}` slot, which was not silence but a false statement:
// measured over the wire, it admitted `align:horizontal` (elided), and
// `align:horizontal:17` and `stack:horizontal:start` (wrong vocabulary), all
// three of which the dispatcher refuses — and `distribute:horizontal:start`,
// which it accepted and silently ignored. `item` said nothing at all, and could
// not have said anything useful: `add:in:1` and `move:in:2:0` are different
// arities, so no flat positional list covers both.
//
// The case tables below are BUILT from the model's own answers
// (`ArrangePass::tail()` / `ItemEdit::tail()` and each tail's `required()`), so
// a new pass or a new edit cannot arrive without one. What cannot be built from
// them is the `SchemaArg` itself — `pinion-node-graph` is pure data and must not
// depend on the framework to name one — so the argument's name, type and
// vocabulary are spelled here and a `const` assertion holds the pair to the
// model's facts. A changed `required()` is then a build failure rather than a
// schema that lies.
// ---------------------------------------------------------------------------

/// What `align` adds: which edge of the selection's own box to meet.
const ARRANGE_EDGE: &[SchemaArg] = &[SchemaArg::one_of("edge", "string", &Edge::WIRE_NAMES)];
/// What `stack` adds: the gap, which defaults to none when left out.
const ARRANGE_GAP: &[SchemaArg] = &[SchemaArg::open("gap", "int").optional()];

/// R1642 — which sides of the selected node `item` will answer for.
///
/// `side`'s vocabulary is `Side`'s two words, but the ANSWERABLE set is per
/// node: a kind declares a variadic run on one side, both, or neither, and
/// `item` refuses `NotVariadic` for the rest. Declaring the argument as the
/// closed pair would have been the same mistake this round repairs one segment
/// along — a schema admitting calls the surface refuses — so the argument points
/// at this read instead and the answer comes fresh, which is what
/// `ArgDomain::ValuesOf` is for.
///
/// Empty when the selection is not exactly one node, because `item` refuses then
/// anyway: an empty domain is the surface saying "no call is well formed right
/// now", which is true and useful, where the closed pair would have said the
/// opposite.
fn item_sides(document: &Document<Op>, tree: TreeId, selection: &[NodeId]) -> String {
    let [node] = *selection else {
        return String::new();
    };
    Side::ALL
        .iter()
        .filter(|side| document.variadic(tree, node, **side).is_some())
        .map(|side| side.name())
        .collect::<Vec<_>>()
        .join(",")
}

/// The arguments a pass adds, by what it reads after the axis.
///
/// Exhaustive on purpose: a new [`ArrangeTail`] arm fails to compile here, which
/// is the only place that would otherwise quietly keep publishing the old table.
const fn arrange_tail_args(tail: ArrangeTail) -> &'static [SchemaArg] {
    match tail {
        ArrangeTail::None => &[],
        ArrangeTail::Edge => ARRANGE_EDGE,
        ArrangeTail::Gap => ARRANGE_GAP,
    }
}

/// One case per pass, projected from [`ArrangePass::ALL`].
const ARRANGE_CASES: [ArgCase; ArrangePass::ARMS] = {
    let mut out = [ArgCase::EMPTY; ArrangePass::ARMS];
    let mut i = 0;
    while i < ArrangePass::ARMS {
        let pass = ArrangePass::ALL[i];
        out[i] = ArgCase::new(pass.name(), arrange_tail_args(pass.tail()));
        i += 1;
    }
    out
};

/// What `add` adds: an optional label for the new item.
const ITEM_LABEL: &[SchemaArg] = &[SchemaArg::open("label", "string").optional()];
/// What `move` adds: the position to carry the item to.
const ITEM_DESTINATION: &[SchemaArg] = &[SchemaArg::open("to", "int")];

/// The arguments an item edit adds, by what it reads after the position.
const fn item_tail_args(tail: ItemEditTail) -> &'static [SchemaArg] {
    match tail {
        ItemEditTail::None => &[],
        ItemEditTail::Label => ITEM_LABEL,
        ItemEditTail::Destination => ITEM_DESTINATION,
    }
}

/// One case per item edit, projected from [`ItemEdit::ALL`].
const ITEM_CASES: [ArgCase; ItemEdit::ARMS] = {
    let mut out = [ArgCase::EMPTY; ItemEdit::ARMS];
    let mut i = 0;
    while i < ItemEdit::ARMS {
        let edit = ItemEdit::ALL[i];
        out[i] = ArgCase::new(edit.name(), item_tail_args(edit.tail()));
        i += 1;
    }
    out
};

/// Every case adds what the model says its tail is, with the optionality the
/// model says — checked at compile time, since this is the one fact the schema
/// and the model each hold a copy of.
const _: () = {
    let mut i = 0;
    while i < ArrangePass::ARMS {
        let tail = ArrangePass::ALL[i].tail();
        let count = if matches!(tail, ArrangeTail::None) {
            0
        } else {
            1
        };
        assert!(
            ARRANGE_CASES[i].adds(count, !tail.required()),
            "an arrange case disagrees with ArrangePass::tail() / ArrangeTail::required()"
        );
        i += 1;
    }
    let mut j = 0;
    while j < ItemEdit::ARMS {
        let tail = ItemEdit::ALL[j].tail();
        let count = if matches!(tail, ItemEditTail::None) {
            0
        } else {
            1
        };
        assert!(
            ITEM_CASES[j].adds(count, !tail.required()),
            "an item case disagrees with ItemEdit::tail() / ItemEditTail::required()"
        );
        j += 1;
    }
};

const WIN_W: u32 = 900;
const WIN_H: u32 = 560;
const CARD_W: i32 = 150;
const ROW_H: i32 = 18;
const HEAD_H: i32 = 26;
/// How far a frame's fence stands off what it contains, per level of nesting.
const FRAME_PAD: i32 = 14;
const PORT: i32 = 9;
const CANVAS_TOP: i32 = 76;
const TITLE_FONT_PX: u32 = 16;
const LABEL_FONT_PX: u32 = 12;
const STATUS_FONT_PX: u32 = 12;

// --- The taxonomy: the whole of what this application supplies ----------------

/// The two socket types, and the **direction** between them.
///
/// R1593 — an amount broadcasts into a colour (the grey of that intensity) and
/// a colour never narrows back into an amount. That is the whole of what this
/// application declares about typing; everything else — that a link is judged
/// by it, that a value is converted by it, that a bypassed node routes by it,
/// that a saved file is re-checked against it — is the substrate's, and is one
/// declaration rather than three tables that can disagree
/// ([`NodeKind::conversion`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Ty {
    Colour,
    Amount,
}

impl Ty {
    const fn name(self) -> &'static str {
        match self {
            Self::Colour => "colour",
            Self::Amount => "amount",
        }
    }
}

/// A value on a wire. Integers throughout, so what an agent reads over RPC is
/// exact rather than a rounded float.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Val {
    /// sRGB bytes.
    Colour([i64; 3]),
    /// Percent, `0..=100`.
    Amount(i64),
}

impl Val {
    fn colour(&self) -> Option<[i64; 3]> {
        match self {
            Self::Colour(c) => Some(*c),
            Self::Amount(_) => None,
        }
    }

    fn amount(&self) -> Option<i64> {
        match self {
            Self::Amount(a) => Some(*a),
            Self::Colour(_) => None,
        }
    }

    /// R1594 — the wire form read back. The mirror of [`Val::wire`], so a value
    /// this application publishes is a value it accepts
    /// ([[wire-form-read-write-symmetry]]).
    fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if let Some((r, rest)) = raw.split_once(',') {
            let (g, b) = rest.split_once(',')?;
            return Some(Self::Colour([
                r.trim().parse().ok()?,
                g.trim().parse().ok()?,
                b.trim().parse().ok()?,
            ]));
        }
        Some(Self::Amount(raw.parse().ok()?))
    }

    /// The wire form: `"12,34,56"` for a colour, `"50"` for an amount.
    fn wire(&self) -> String {
        match self {
            Self::Colour([r, g, b]) => format!("{r},{g},{b}"),
            Self::Amount(a) => a.to_string(),
        }
    }
}

/// The material ops. Five arms, and not one of them knows what a group is.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Op {
    /// R1594 — a colour source. A UNIT variant: what it emits is a value
    /// authored on its own output port, not a payload in the taxonomy. Before
    /// R1594 the crate had nowhere to put a per-node value, so the constant had
    /// to live here — where an editor could never change it.
    Swatch,
    /// R1594 — a scalar source, the same shape.
    Level,
    /// `mix(a, b, t)` — component-wise, `t` in percent.
    Mix,
    /// Desaturate towards grey by `t` percent.
    Fade,
    /// Clamp an amount to a ceiling. R1587 — the one node here whose CONTROL
    /// input shares the data type it controls, so the bare identity rule would
    /// pass the ceiling through instead of the value. `Ceiling` declares itself
    /// off the bypass path, and so does `Clipped`, whose value only means
    /// anything while the node is computing.
    Cap,
    /// R1593 — an amount in, a colour out. The one shape here whose *bypass*
    /// route can only CONVERT: its single input reaches its single output
    /// through the lattice's broadcast and by no other path, so a relation that
    /// compared types with `==` would drop that output instead.
    ///
    /// What it computes is deliberately NOT the broadcast (it doubles, and
    /// clamps), so "the node ran" and "the node was bypassed" are two different
    /// numbers rather than one that could be either.
    Tint,
    /// R1593 — a strength and a colour in, a colour out. The shape that makes
    /// the bypass *preference* falsifiable: its output's OWN index holds an
    /// amount, which could reach it by converting, and index 1 holds a colour,
    /// which reaches it unchanged. A rule that ranked position above the value
    /// would pass the grey of the strength through instead of the colour.
    Glaze,
    /// R1632 — a LAYER STACK: `(Base: Colour, [Layer: Colour, Opacity: Amount] x n,
    /// Gain: Amount) -> Colour`.
    ///
    /// The one kind here whose port count belongs to the **node**. Its run has
    /// a fixed port on each side, and an item is a PAIR — which is the engine's
    /// blend-list shape (two parallel arrays) and the case that tells a correct
    /// re-index from one that shifts by a single port.
    Layers,
    /// The sink: its resolved input is the material's result.
    Output,
}

impl Op {
    /// Every name the `add` verb takes, in palette order.
    ///
    /// One list, so the parser and the refusal that names the alternatives
    /// cannot drift — they did, before R1593: the message still read
    /// "swatch/level/mix/fade/output" long after `cap` was added.
    const PALETTE: [&'static str; 9] = [
        "swatch", "level", "mix", "fade", "cap", "tint", "glaze", "layers", "output",
    ];

    /// Parse the palette name a `add` verb takes.
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "swatch" => Self::Swatch,
            "level" => Self::Level,
            "mix" => Self::Mix,
            "fade" => Self::Fade,
            "cap" => Self::Cap,
            "tint" => Self::Tint,
            "glaze" => Self::Glaze,
            "layers" => Self::Layers,
            "output" => Self::Output,
            _ => return None,
        })
    }
}

impl NodeKind for Op {
    type Type = Ty;
    type Value = Val;
    type Graph = ();

    fn name(&self) -> String {
        match self {
            Self::Swatch => "Swatch",
            Self::Level => "Level",
            Self::Mix => "Mix",
            Self::Fade => "Fade",
            Self::Cap => "Cap",
            Self::Tint => "Tint",
            Self::Glaze => "Glaze",
            Self::Layers => "Layers",
            Self::Output => "Output",
        }
        .to_owned()
    }

    fn inputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Swatch | Self::Level => Vec::new(),
            Self::Mix => vec![
                Port::new("Base", Ty::Colour).with_default(Val::Colour([0, 0, 0])),
                Port::new("Blend", Ty::Colour).with_default(Val::Colour([255, 255, 255])),
                Port::new("Factor", Ty::Amount).with_default(Val::Amount(50)),
            ],
            Self::Fade => vec![
                Port::new("Colour", Ty::Colour).with_default(Val::Colour([0, 0, 0])),
                Port::new("Factor", Ty::Amount).with_default(Val::Amount(0)),
            ],
            Self::Cap => vec![
                Port::new("Ceiling", Ty::Amount)
                    .with_default(Val::Amount(100))
                    .no_passthrough(),
                Port::new("Amount", Ty::Amount).with_default(Val::Amount(0)),
            ],
            Self::Tint => vec![Port::new("Amount", Ty::Amount).with_default(Val::Amount(0))],
            Self::Glaze => vec![
                Port::new("Strength", Ty::Amount).with_default(Val::Amount(0)),
                Port::new("Colour", Ty::Colour).with_default(Val::Colour([0, 0, 0])),
            ],
            // R1632 — the FIXED half. What repeats is declared once, below.
            Self::Layers => vec![
                Port::new("Base", Ty::Colour).with_default(Val::Colour([0, 0, 0])),
                Port::new("Gain", Ty::Amount).with_default(Val::Amount(100)),
            ],
            Self::Output => vec![Port::new("Surface", Ty::Colour)],
        }
    }

    /// R1632 — which run of the ports above is the NODE's rather than the
    /// kind's. Every other kind here writes nothing, which is the default.
    fn variadic(&self, side: Side) -> Option<Variadic<Ty, Val>> {
        match (self, side) {
            (Self::Layers, Side::Input) => Some(
                Variadic::at(
                    1,
                    vec![
                        Port::new("Layer", Ty::Colour).with_default(Val::Colour([0, 0, 0])),
                        Port::new("Opacity", Ty::Amount).with_default(Val::Amount(100)),
                    ],
                )
                .at_least(1)
                .at_most(6),
            ),
            _ => None,
        }
    }

    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Mix | Self::Fade | Self::Tint | Self::Glaze | Self::Layers => {
                vec![Port::new("Colour", Ty::Colour)]
            }
            // R1594 — the resting value a fresh source emits is declared here,
            // by the KIND, and the value a particular node emits is authored on
            // that node's port. Type and name from the kind, value from the
            // node.
            Self::Swatch => {
                vec![Port::new("Colour", Ty::Colour).with_default(Val::Colour([128, 128, 128]))]
            }
            Self::Level => vec![Port::new("Amount", Ty::Amount).with_default(Val::Amount(50))],
            Self::Cap => vec![
                Port::new("Amount", Ty::Amount),
                Port::new("Clipped", Ty::Amount).no_passthrough(),
            ],
            Self::Output => Vec::new(),
        }
    }

    fn evaluate(&self, inputs: &[Option<Val>]) -> Vec<Option<Val>> {
        let colour = |i: usize| inputs.get(i).and_then(Option::as_ref).and_then(Val::colour);
        let amount = |i: usize| inputs.get(i).and_then(Option::as_ref).and_then(Val::amount);
        match self {
            // A source computes nothing: what it emits is what its port
            // carries, which is the node's business and not the kind's.
            Self::Swatch | Self::Level => vec![None],
            Self::Mix => {
                let (Some(a), Some(b), Some(t)) = (colour(0), colour(1), amount(2)) else {
                    return vec![None];
                };
                let t = t.clamp(0, 100);
                let mix = |x: i64, y: i64| (x * (100 - t) + y * t) / 100;
                vec![Some(Val::Colour([
                    mix(a[0], b[0]),
                    mix(a[1], b[1]),
                    mix(a[2], b[2]),
                ]))]
            }
            Self::Fade => {
                let (Some(c), Some(t)) = (colour(0), amount(1)) else {
                    return vec![None];
                };
                let t = t.clamp(0, 100);
                let grey = (c[0] + c[1] + c[2]) / 3;
                let towards = |x: i64| (x * (100 - t) + grey * t) / 100;
                vec![Some(Val::Colour([
                    towards(c[0]),
                    towards(c[1]),
                    towards(c[2]),
                ]))]
            }
            Self::Cap => {
                let (Some(ceiling), Some(amount)) = (amount(0), amount(1)) else {
                    return vec![None, None];
                };
                vec![
                    Some(Val::Amount(amount.min(ceiling))),
                    Some(Val::Amount((amount - ceiling).max(0))),
                ]
            }
            Self::Tint => {
                let level = amount(0).unwrap_or(0).clamp(0, 100) * 2;
                vec![Some(Val::Colour([level, level, level]))]
            }
            Self::Glaze => {
                let (Some(strength), Some(c)) = (amount(0), colour(1)) else {
                    return vec![None];
                };
                let keep = 100 - strength.clamp(0, 100);
                vec![Some(Val::Colour([
                    c[0] * keep / 100,
                    c[1] * keep / 100,
                    c[2] * keep / 100,
                ]))]
            }
            // R1632 — the payoff of a variadic kind: `inputs` is as long as
            // THIS NODE's resolved signature, so the loop reads the run the
            // node has rather than an arity compiled in here. Each layer is
            // composited over what is under it at its own opacity, and the
            // trailing `Gain` scales the result — a fixed port whose index
            // depends on how many layers there are.
            Self::Layers => {
                let last = inputs.len() - 1;
                let Some(mut under) = colour(0) else {
                    return vec![None];
                };
                let mut slot = 1;
                while slot + 1 < last {
                    if let (Some(over), Some(alpha)) = (colour(slot), amount(slot + 1)) {
                        let a = alpha.clamp(0, 100);
                        for channel in 0..3 {
                            under[channel] = (under[channel] * (100 - a) + over[channel] * a) / 100;
                        }
                    }
                    slot += 2;
                }
                let gain = amount(last).unwrap_or(100).clamp(0, 200);
                vec![Some(Val::Colour([
                    (under[0] * gain / 100).min(255),
                    (under[1] * gain / 100).min(255),
                    (under[2] * gain / 100).min(255),
                ]))]
            }
            Self::Output => Vec::new(),
        }
    }

    /// R1594 — which socket type a value is one of.
    ///
    /// This taxonomy's values carry their own type, so it can answer, and
    /// answering is what lets [`Document::set_port_value`] refuse a colour on an amount port. The DCC
    /// needs no equivalent because a socket's authored value is a different C
    /// struct per socket type.
    fn value_type(value: &Val) -> Option<Ty> {
        Some(match value {
            Val::Colour(_) => Ty::Colour,
            Val::Amount(_) => Ty::Amount,
        })
    }

    /// R1593 — the lattice. An amount broadcasts into a colour; a colour does
    /// not narrow back.
    ///
    /// An *associated* function, because a wire's legality is a property of the
    /// two types and of neither node: an editor asks it while the wire is being
    /// dragged, before there is a value and often before there is a node at the
    /// far end.
    ///
    /// Declaring the conversion here — rather than declaring a *predicate*
    /// here and the conversion somewhere else — is what makes "this wire is
    /// legal" and "this is what arrives along it" impossible to disagree. The
    /// DCC has three separate answers to that one question (`validate_link`, `DataTypeConversions`, `get_internal_link_type_priority`).
    fn conversion(from: &Ty, to: &Ty) -> Conversion<Val> {
        match (from, to) {
            (Ty::Colour, Ty::Colour) | (Ty::Amount, Ty::Amount) => Conversion::Direct,
            // A percentage becomes the grey of that intensity. Integer
            // throughout, so what an agent reads is exact.
            (Ty::Amount, Ty::Colour) => Conversion::Converted(|value| match value {
                Val::Amount(a) => {
                    let level = a.clamp(0, 100) * 255 / 100;
                    Some(Val::Colour([level, level, level]))
                }
                Val::Colour(_) => None,
            }),
            // No narrowing: a colour is three numbers and an amount is one, and
            // which one it would be is a question the lattice must not answer
            // by guessing.
            (Ty::Colour, Ty::Amount) => Conversion::Refused,
        }
    }
}

// --- The application state ----------------------------------------------------

/// Everything this example holds. The document is one signal, because the
/// substrate's document is one value.
struct GroupsState {
    document: Signal<Document<Op>>,
    path: Signal<EditPath>,
    selection: Signal<Vec<NodeId>>,
    /// The text of the last refusal, so the wire can be asked what went wrong.
    refusal: Signal<String>,
    /// R1578 — the clipboard. A `Fragment` is a value, so holding one is all a
    /// clipboard is; the substrate owns none of this because *where* a copied
    /// piece of graph is kept is the application's business (a signal here, a
    /// system clipboard elsewhere, a palette of snippets in a third place).
    clipboard: Signal<Option<Fragment<Op>>>,
    /// What the last insertion did, so the wire can read the outcome an editor
    /// has to show: which definitions were re-used, and which severed inputs
    /// did not come back.
    last_insert: Signal<String>,
    /// R1584 — what the last boundary move did to the interface, and what it
    /// cost at the definition's other instances. An editor has to show that
    /// second part: the user moved a node in one place and changed another.
    last_move: Signal<String>,
    /// R1586 — what the last dissolve or detach did: what it bridged, and what
    /// it could not. The second half is the one the DCC's `node_internal_relink` discards, and it
    /// is what tells an author a value has just gone.
    last_rewire: Signal<String>,
}

impl GroupsState {
    fn new() -> Self {
        Self {
            document: Signal::new(seed()),
            path: Signal::new(EditPath::root()),
            selection: Signal::new(Vec::new()),
            refusal: Signal::new(String::new()),
            clipboard: Signal::new(None),
            last_insert: Signal::new(String::new()),
            last_move: Signal::new(String::new()),
            last_rewire: Signal::new(String::new()),
        }
    }

    fn current(&self) -> TreeId {
        self.path.get().current()
    }

    /// Run an edit, recording its refusal rather than discarding it.
    fn edit<T, E: std::fmt::Display>(
        &self,
        run: impl FnOnce(&mut Document<Op>) -> Result<T, E>,
    ) -> Result<T, String> {
        let mut document = self.document.get();
        match run(&mut document) {
            Ok(value) => {
                self.document.set(document);
                self.refusal.set(String::new());
                Ok(value)
            }
            Err(error) => {
                let sentence = error.to_string();
                self.refusal.set(sentence.clone());
                Err(sentence)
            }
        }
    }
}

/// The starting material: two swatches and a level feeding a mix, into a sink.
/// Chosen so the very first `group [mix]` has three values crossing in and one
/// crossing out.
fn seed() -> Document<Op> {
    let mut document = Document::new("Material");
    let base = add(&mut document, Op::Swatch, 20, 0);
    let blend = add(&mut document, Op::Swatch, 20, 90);
    let level = add(&mut document, Op::Level, 20, 180);
    // R1594 — three nodes of two kinds, three different values, authored on the
    // nodes' own ports. Before this round the constant lived in the taxonomy,
    // so `Swatch` had to be a payload variant and nothing could edit it.
    for (node, value) in [
        (base, Val::Colour([200, 60, 60])),
        (blend, Val::Colour([40, 90, 220])),
        (level, Val::Amount(25)),
    ] {
        let _ = document.set_port_value(ROOT, node, PortRef::output(0), value);
    }
    let mix = add(&mut document, Op::Mix, 260, 60);
    let fade = add(&mut document, Op::Fade, 470, 60);
    let out = add(&mut document, Op::Output, 680, 60);
    for (from, to) in [
        (Socket::new(base, 0), Socket::new(mix, 0)),
        (Socket::new(blend, 0), Socket::new(mix, 1)),
        (Socket::new(level, 0), Socket::new(mix, 2)),
        (Socket::new(mix, 0), Socket::new(fade, 0)),
        (Socket::new(fade, 0), Socket::new(out, 0)),
    ] {
        let _ = document.connect(ROOT, from, to);
    }
    document
}

fn add(document: &mut Document<Op>, op: Op, x: i32, y: i32) -> NodeId {
    document
        .add_node(ROOT, NodeBody::Kind(op), x, y)
        .unwrap_or(NodeId(0))
}

fn use_groups_state() -> Rc<GroupsState> {
    Owner::current()
        .expect("use_groups_state requires an active Owner scope")
        .cache(STATE_KEY, GroupsState::new)
}

// --- Painting -----------------------------------------------------------------

/// Clamp a signed canvas coordinate into the unsigned pixel space a `Rect` uses.
fn upx(v: i32) -> u32 {
    u32::try_from(v).unwrap_or(0)
}

/// A path point from integer canvas coordinates.
#[allow(
    clippy::cast_precision_loss,
    reason = "canvas coordinates are < 2^13, exactly representable in f32"
)]
fn ppt(x: i32, y: i32) -> PathPoint {
    PathPoint::new(x as f32, y as f32)
}

/// A port index as a canvas offset. A signature is short by construction; a
/// count that would not fit is clamped rather than wrapped.
fn rows(count: usize) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

/// How tall a node's card is: a head plus one row per port, whichever side has
/// more. Derived from the signature, so a group node resizes when its
/// definition's interface changes and nothing here has to be told.
fn card_height(ports: (usize, usize)) -> i32 {
    HEAD_H + rows(ports.0.max(ports.1).max(1)) * ROW_H + 6
}

/// R1589 — a frame's extent: the union of what it contains, plus a margin that
/// grows with how deeply it is nested, so an inner frame sits inside its
/// container rather than on top of it.
///
/// Derived by the **application**, because it needs card geometry the crate
/// deliberately does not have: the crate answers *what does this contain*,
/// this answers *how big is that*. The DCC computes the same union in `node_draw.cc` into
/// `runtime->draw_bounds`, a cache with a pass to keep it fresh.
///
/// `None` for a frame containing nothing — an empty fence has no derived size,
/// and drawing a default-sized one would be inventing a fact.
fn frame_extent(
    document: &Document<Op>,
    tree: TreeId,
    frame: NodeId,
) -> Option<(i32, i32, i32, i32)> {
    let host = document.tree(tree)?;
    let contents = document.contents(tree, frame);
    let depth_here = document.ancestry(tree, frame).len();
    let mut extent: Option<(i32, i32, i32, i32)> = None;
    let mut below = 1;
    for &member in &contents {
        below = below.max(
            document
                .ancestry(tree, member)
                .len()
                .saturating_sub(depth_here),
        );
        let node = host.node(member)?;
        if node.is_frame() {
            continue;
        }
        let shown = document.visible_ports(tree, member).unwrap_or_default();
        let height = card_height((shown.inputs.len(), shown.outputs.len()));
        let card = (node.x, node.y, node.x + CARD_W, node.y + height);
        extent = Some(match extent {
            None => card,
            Some(had) => (
                had.0.min(card.0),
                had.1.min(card.1),
                had.2.max(card.2),
                had.3.max(card.3),
            ),
        });
    }
    let pad = FRAME_PAD * i32::try_from(below).unwrap_or(1);
    extent.map(|(x0, y0, x1, y1)| (x0 - pad, y0 - pad, x1 + pad, y1 + pad))
}

/// The fence itself, plus its title. Tagged, so the derived extent is readable
/// over the wire without a screenshot (§2 #7).
fn frame_scene(
    document: &Document<Op>,
    tree: TreeId,
    node: &Node<Op>,
    selected: bool,
    theme: &pinion_core::theme::Theme,
) -> Vec<Scene> {
    let Some((x0, y0, x1, y1)) = frame_extent(document, tree, node.id) else {
        return Vec::new();
    };
    let rect = Rect::new(upx(x0), upx(y0 + CANVAS_TOP), upx(x1 - x0), upx(y1 - y0));
    let outline = theme.resolve(if selected {
        ColorRole::Accent
    } else {
        ColorRole::Outline
    });
    vec![
        Scene::Box(
            BoxNode::new(
                rect,
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerLow))
                    .with_border(Border::new(outline, if selected { 2 } else { 1 })),
            )
            .with_tag(format!("{VIEW_TAG}.frame.{}", node.id.0))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(rect.x, rect.y)
                    .with_size(Size::px(rect.w, rect.h)),
            ),
        ),
        Scene::Text(
            TextNode::styled(
                node.display_name(),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(STATUS_FONT_PX)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            )
            .with_tag(format!("{VIEW_TAG}.frame.{}.title", node.id.0))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(rect.x + 6, rect.y + 4)
                    .with_size(Size::px(rect.w.saturating_sub(12), STATUS_FONT_PX + 4)),
            ),
        ),
    ]
}

/// One port: its pin and its name.
///
/// The tag names the port's INDEX IN THE SIGNATURE and not the row it landed
/// on, because a tag is an address: hiding an unused port moves every later row
/// up, and a tag derived from the row would then name a different port than it
/// did a frame ago.
fn port_scenes(
    declared: &Port<Ty, Val>,
    at: (NodeId, u32, bool),
    (px, py): (i32, i32),
    (theme, label): (&pinion_core::theme::Theme, Color),
) -> Vec<Scene> {
    let (node, port, output) = at;
    vec![
        Scene::Box(
            BoxNode::new(
                Rect::new(upx(px - PORT / 2), upx(py - PORT / 2), upx(PORT), upx(PORT)),
                BoxStyle::filled(theme.resolve(match declared.value_type() {
                    Some(Ty::Colour) => ColorRole::Accent,
                    Some(Ty::Amount) => ColorRole::OnSurfaceMuted,
                    // R1599 — this taxonomy declares no control port; the arm
                    // exists because the model now admits one.
                    None => ColorRole::Outline,
                })),
            )
            .with_tag(format!(
                "{VIEW_TAG}.pin.{}.{}.{port}",
                node.0,
                if output { "out" } else { "in" }
            ))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(upx(px - PORT / 2), upx(py - PORT / 2))
                    .with_size(Size::px(upx(PORT), upx(PORT))),
            ),
        ),
        Scene::Text(
            TextNode::styled(
                &declared.name,
                Rect::default(),
                TextStyle::new().with_size_px(LABEL_FONT_PX).with_fg(label),
            )
            // R1632 — the name a variadic port shows is DERIVED from its item's
            // ordinal, so it is worth being able to read the painted one rather
            // than the model's. Addressed like the pin beside it.
            .with_tag(format!(
                "{VIEW_TAG}.pinlabel.{}.{}.{port}",
                node.0,
                if output { "out" } else { "in" }
            ))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(
                        upx(if output { px - 76 } else { px + 10 }),
                        upx(py - i32::try_from(LABEL_FONT_PX).unwrap_or(12) / 2 - 1),
                    )
                    .with_size(Size::px(66, LABEL_FONT_PX + 4)),
            ),
        ),
    ]
}

/// The routing a bypassed node passes through, drawn across its card.
///
/// An output with no route simply has no line reaching it — which is the fact
/// an author most needs to see, and the one the DCC's derivation discards.
///
/// R1593 — a route that CONVERTS is drawn dotted, by the same
/// [`WireLook`] a link is, because it is the same fact about the same value:
/// what leaves is not what arrived. That the two agree is structural rather
/// than remembered — one enum, one stroke table.
fn passthrough_scenes(
    document: &Document<Op>,
    tree: TreeId,
    node: NodeId,
    at: (i32, i32),
    shown: &pinion_node_graph::VisiblePorts,
    theme: &pinion_core::theme::Theme,
) -> Vec<Scene> {
    let Some(through) = document.passthrough(tree, node) else {
        return Vec::new();
    };
    through
        .routes()
        .iter()
        .map(|route| {
            let muted = theme.resolve(ColorRole::OnSurfaceMuted);
            wire_scene(
                WireLook::of(false, route.converts),
                pin_at(at.0, at.1, position(&shown.inputs, route.input), false),
                pin_at(at.0, at.1, position(&shown.outputs, route.output), true),
                muted,
                muted,
                format!("{VIEW_TAG}.through.{}.{}", node.0, route.output),
            )
        })
        .collect()
}

/// Which drawn row a port index landed on, once hidden ports were taken out.
///
/// A port's identity is its index in the signature; its *place on screen* is its
/// place among the ports still drawn. Keeping the two apart is what lets a
/// collapsed node hide a port without renumbering anything.
fn position(shown: &[u32], port: u32) -> usize {
    shown.iter().position(|&p| p == port).unwrap_or(0)
}

/// Where a socket's pin sits, in canvas coordinates.
fn pin_at(x: i32, y: i32, index: usize, output: bool) -> (i32, i32) {
    let px = if output {
        x + CARD_W - PORT / 2
    } else {
        x + PORT / 2
    };
    (px, y + HEAD_H + rows(index) * ROW_H + ROW_H / 2)
}

/// What a node's BODY is, named on the wire.
///
/// A free function rather than an arm, because every structural body this crate
/// owns has to be named here and the match is exhaustive — so a body added
/// upstream fails to compile until this says what to call it (R1600 added the
/// fifth).
fn body_name(body: &NodeBody<Op>) -> String {
    match body {
        NodeBody::Kind(op) => op.name(),
        NodeBody::Group(inner) => format!("group:{}", inner.0),
        NodeBody::Interface(InterfaceSide::Input) => "interface:input".to_owned(),
        NodeBody::Interface(InterfaceSide::Output) => "interface:output".to_owned(),
        NodeBody::Frame => "frame".to_owned(),
        NodeBody::Delay(ty) => format!("delay:{ty:?}"),
        NodeBody::Reroute => "reroute".to_owned(),
        // R1935 — the two halves of a NAME. Told apart here because they are
        // two shapes and not two spellings: one takes a wire in, the other has
        // no way in at all.
        NodeBody::Beacon => "named".to_owned(),
        NodeBody::Echo(end) => format!("far:{}", end.0),
    }
}

fn node_scene(
    document: &Document<Op>,
    tree: TreeId,
    node: &pinion_node_graph::Node<Op>,
    selected: bool,
    theme: &pinion_core::theme::Theme,
) -> Vec<Scene> {
    let signature = document.signature(tree, node.id);
    let (ins, outs) = signature.as_ref().map_or((Vec::new(), Vec::new()), |s| {
        (s.inputs.clone(), s.outputs.clone())
    });
    // R1586 — which ports are DRAWN is the document's derivation, not this
    // painter's: `hide_unused_ports` is unanswerable without knowing what is
    // wired, and only the document knows that.
    let shown = document.visible_ports(tree, node.id).unwrap_or_default();
    let height = card_height((shown.inputs.len(), shown.outputs.len()));
    let (x, y) = (node.x, node.y + CANVAS_TOP);

    // A group instance is drawn in the container role, so "this node is another
    // graph" is visible without reading its title.
    let fill = theme.resolve(match node.body {
        // R1600 — a register joins the group instance in the container role:
        // both are structural bodies this crate owns, and what each one is is a
        // fact about the graph rather than about this taxonomy.
        // R1935 — a NAMED endpoint joins them: its name is the address a value
        // crosses the canvas to, so it is a card with something to read.
        NodeBody::Group(_) | NodeBody::Delay(_) | NodeBody::Beacon => {
            ColorRole::SurfaceContainerHighest
        }
        // A frame is not a card: it is drawn by `frame_scene`, behind
        // everything, at an extent DERIVED from what it contains (R1589), so
        // the canvas loop never reaches this arm for one.
        // R1934 — a reroute joins these two: it is a point on a wire and not a
        // card, so it is drawn in the recessive role for the reason a frame is.
        // This demo has no gesture that makes one, so the arm is reached only
        // by a document loaded from elsewhere.
        // R1935 — a FAR END is drawn recessively with the bend, because what it
        // shows is the endpoint's name rather than its own.
        NodeBody::Interface(_) | NodeBody::Frame | NodeBody::Reroute | NodeBody::Echo(_) => {
            ColorRole::SurfaceContainerLow
        }
        NodeBody::Kind(_) => ColorRole::SurfaceContainerHigh,
    });
    let label = theme.resolve(ColorRole::OnSurface);
    let outline = theme.resolve(if selected {
        ColorRole::Accent
    } else if node.bypassed {
        ColorRole::OnSurfaceMuted
    } else {
        ColorRole::Outline
    });
    let rect = Rect::new(upx(x), upx(y), upx(CARD_W), upx(height));
    let mut scenes = vec![
        Scene::Box(
            BoxNode::new(
                rect,
                BoxStyle::filled(fill)
                    .with_border(Border::new(outline, if selected { 2 } else { 1 })),
            )
            .with_tag(format!("{VIEW_TAG}.node.{}", node.id.0))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(rect.x, rect.y)
                    .with_size(Size::px(rect.w, rect.h)),
            ),
        ),
        Scene::Text(
            TextNode::styled(
                node.display_name(),
                Rect::default(),
                TextStyle::new().with_size_px(LABEL_FONT_PX).with_fg(label),
            )
            .with_tag(format!("{VIEW_TAG}.title.{}", node.id.0))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(rect.x + 8, rect.y + 6)
                    .with_size(Size::px(upx(CARD_W - 12), LABEL_FONT_PX + 6)),
            ),
        ),
    ];
    // A bypassed node draws what passes through it: the routing the substrate
    // derived, as wires straight across the card. An author sees which value
    // comes out where without reading `passthrough` over the wire — and an
    // output with no route simply has no line reaching it, which is the fact
    // most worth seeing.
    if node.bypassed {
        scenes.extend(passthrough_scenes(
            document,
            tree,
            node.id,
            (x, y),
            &shown,
            theme,
        ));
    }
    for (side, ports) in [(false, &shown.inputs), (true, &shown.outputs)] {
        for (row, &port) in ports.iter().enumerate() {
            let all = if side { &outs } else { &ins };
            let Some(declared) = all.get(port as usize) else {
                continue;
            };
            scenes.extend(port_scenes(
                declared,
                (node.id, port, side),
                pin_at(x, y, row, side),
                (theme, label),
            ));
        }
    }
    scenes
}

/// What a wire is doing with the value it was given, which is what decides how
/// it is drawn.
///
/// Three facts, ORDERED rather than merged, and this is the one place that
/// ordering is stated: a muted wire carries no value, so there is nothing for
/// it to convert, and drawing it as "converting" would say something false.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WireLook {
    /// Carries its value unchanged.
    Direct,
    /// Carries its value, and **changes** it on the way (R1593).
    Converting,
    /// Drawn, and carrying nothing (R1586).
    Muted,
}

impl WireLook {
    /// The look a link with these two facts has.
    const fn of(muted: bool, converts: bool) -> Self {
        if muted {
            Self::Muted
        } else if converts {
            Self::Converting
        } else {
            Self::Direct
        }
    }

    /// The stroke that says it.
    ///
    /// Solid, dashed and dotted are three arms of one vocabulary R1575 opened
    /// — so a reader tells them apart without a legend, and a colour-blind
    /// reader tells them apart at all. The DCC shows an implicit conversion
    /// only by materialising a whole `implicit_conversion` node into the tree, so seeing that
    /// fact there costs a change to the graph you are looking at.
    fn stroke(self, direct: Color, muted: Color) -> Stroke {
        match self {
            Self::Direct => Stroke::new(direct, 2),
            Self::Converting => Stroke::new(direct, 2).with_dash(Dash::DOTTED),
            Self::Muted => Stroke::new(muted, 2).with_dash(Dash::DASHED),
        }
    }
}

/// A wire: a cubic whose handles are horizontal, the shape every node editor
/// draws.
fn wire_scene(
    look: WireLook,
    from: (i32, i32),
    to: (i32, i32),
    direct: Color,
    muted: Color,
    tag: String,
) -> Scene {
    wire_with(from, to, look.stroke(direct, muted), tag)
}

fn wire_with(from: (i32, i32), to: (i32, i32), stroke: Stroke, tag: String) -> Scene {
    let reach = ((to.0 - from.0).abs() / 2).clamp(30, 120);
    let left = from.0.min(to.0) - 4;
    let top = from.1.min(to.1) - 4;
    let point = |p: (i32, i32)| ppt(p.0 - left, p.1 - top);
    Scene::Path(
        PathNode::new(
            Rect::new(upx(left), upx(top), 1, 1),
            vec![
                PathCommand::MoveTo(point(from)),
                PathCommand::CurveTo {
                    c1: point((from.0 + reach, from.1)),
                    c2: point((to.0 - reach, to.1)),
                    end: point(to),
                },
            ],
            PathStyle::stroked(stroke),
        )
        .with_tag(tag)
        .with_layout(LayoutStyle::new().with_absolute_position(upx(left), upx(top))),
    )
}

fn view() -> Scene {
    let state = use_groups_state();
    let theme = use_theme(THEME_TAG).theme_animated();
    let document = state.document.get();
    let path = state.path.get();
    let tree = path.current();
    let selection = state.selection.get();
    let ink = theme.resolve(ColorRole::OnSurface);

    let mut children = vec![
        Scene::Text(
            TextNode::styled(
                path.breadcrumb(&document).join("  >  "),
                Rect::default(),
                TextStyle::new().with_size_px(TITLE_FONT_PX).with_fg(ink),
            )
            .with_tag(format!("{VIEW_TAG}.breadcrumb"))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(20, 18)
                    .with_size(Size::px(WIN_W - 40, TITLE_FONT_PX + 8)),
            ),
        ),
        Scene::Text(
            TextNode::styled(
                status_line(&document, &state),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(STATUS_FONT_PX)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            )
            .with_tag(format!("{VIEW_TAG}.status"))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(20, 46)
                    .with_size(Size::px(WIN_W - 40, STATUS_FONT_PX + 8)),
            ),
        ),
    ];

    if let Some(host) = document.tree(tree) {
        // R1589 — frames first, outermost first, so a nested fence draws on top
        // of the one containing it and both draw behind every wire and card.
        let mut fences: Vec<&Node<Op>> = host.nodes().filter(|n| n.is_frame()).collect();
        fences.sort_by_key(|n| document.ancestry(tree, n.id).len());
        for fence in fences {
            children.extend(frame_scene(
                &document,
                tree,
                fence,
                selection.contains(&fence.id),
                &theme,
            ));
        }
        let wire_colour = theme.resolve(ColorRole::Outline);
        for link in host.links() {
            let (Some(source), Some(sink)) = (
                document.tree(tree).and_then(|t| t.node(link.from.node)),
                document.tree(tree).and_then(|t| t.node(link.to.node)),
            ) else {
                continue;
            };
            let from = pin_at(
                source.x,
                source.y + CANVAS_TOP,
                link.from.port as usize,
                true,
            );
            let to = pin_at(sink.x, sink.y + CANVAS_TOP, link.to.port as usize, false);
            let look = WireLook::of(
                link.muted,
                document
                    .link_conversion(tree, link.id)
                    .is_some_and(|c| c.converts()),
            );
            children.push(wire_scene(
                look,
                from,
                to,
                wire_colour,
                theme.resolve(ColorRole::OnSurfaceMuted),
                format!("{VIEW_TAG}.wire.{}", link.id.0),
            ));
        }
        for node in host.nodes().filter(|n| !n.is_frame()) {
            children.extend(node_scene(
                &document,
                tree,
                node,
                selection.contains(&node.id),
                &theme,
            ));
        }
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(VIEW_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

fn status_line(document: &Document<Op>, state: &GroupsState) -> String {
    let refusal = state.refusal.get();
    if !refusal.is_empty() {
        return format!("refused: {refusal}");
    }
    let tree = state.current();
    let nodes = document
        .tree(tree)
        .map_or(0, pinion_node_graph::Tree::node_count);
    let held = state.clipboard.get().map_or_else(String::new, |fragment| {
        format!(", clipboard {}", describe_fragment(&fragment))
    });
    format!(
        "{nodes} nodes, {} definitions, {} selected{held}",
        document.tree_count() - 1,
        state.selection.get().len()
    )
}

/// A boundary move in one line: which definition, what happened to its
/// interface, and — the part only this framework answers — who else was changed.
fn describe_move(out: &Repartitioned<Op>) -> String {
    let ports = |changes: &[PortChange<Op>]| {
        changes
            .iter()
            .map(|change| {
                let side = match change.side {
                    InterfaceSide::Input => "in",
                    InterfaceSide::Output => "out",
                };
                format!("{side}{}:{}", change.index, change.port.name)
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!(
        "def:{}|forked_from:{}|moved:{}|exposed:{}|unexposed:{}|severed:{}|others:{}|unframed:{}",
        out.definition.0,
        out.forked_from
            .map_or_else(|| "-".to_owned(), |t| t.0.to_string()),
        out.moved.len(),
        ports(&out.exposed),
        ports(&out.unexposed),
        out.severed
            .iter()
            .map(|dropped| format!("t{}:{}", dropped.tree.0, dropped.link.to))
            .collect::<Vec<_>>()
            .join(" "),
        out.other_instances,
        describe_orphans(&out.orphaned)
    )
}

/// A fragment in one line: what it holds and what was cut away to get it.
fn describe_fragment(fragment: &Fragment<Op>) -> String {
    format!(
        "{}n/{}d in:{} out:{}",
        fragment.node_count(),
        fragment.definitions().count(),
        fragment.inbound().len(),
        fragment.outbound().len()
    )
}

/// The crossings in the wire's own vocabulary: `"3.0>5.1,5.2"`, producer first.
fn describe_severed(severed: &[Severed]) -> String {
    severed
        .iter()
        .map(|one| {
            let consumers: Vec<String> = one
                .consumers()
                .iter()
                .map(|c| format!("{}.{}", c.node.0, c.port))
                .collect();
            format!(
                "{}.{}>{}",
                one.producer().node.0,
                one.producer().port,
                consumers.join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// What an insertion did, as one readable sentence.
fn describe_insert(out: &Inserted) -> String {
    let ids = |trees: &[TreeId]| {
        trees
            .iter()
            .map(|t| t.0.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "nodes:{}|links:{}|added:{}|reused:{}|reattached:{}|unattached:{}|reframed:{}|unframed:{}",
        out.nodes.len(),
        out.links.len(),
        ids(&out.definitions_added),
        ids(&out.definitions_reused),
        out.reattached.len(),
        describe_severed(&out.unattached),
        out.reframed
            .iter()
            .map(|n| n.0.to_string())
            .collect::<Vec<_>>()
            .join(","),
        describe_orphans(&out.unframed)
    )
}

/// R1589 — the containments an edit could not carry: `"3<7"`, the node and the
/// frame it is no longer in. Published rather than left to be noticed, because
/// a copy that quietly left its fence behind is exactly what the DCC does.
fn describe_orphans(orphaned: &[Orphaned]) -> String {
    orphaned
        .iter()
        .map(|one| format!("{}<{}", one.node.0, one.frame.0))
        .collect::<Vec<_>>()
        .join(",")
}

/// Where a fragment goes and under which two policies.
struct Placement {
    point: (i32, i32),
    crossings: Crossings,
    definitions: Definitions,
}

// --- The RPC surface ----------------------------------------------------------

struct GroupsOracle {
    state: Option<Rc<GroupsState>>,
}

impl core::fmt::Debug for GroupsOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GroupsOracle")
            .field("attached", &self.state.is_some())
            .finish()
    }
}

impl GroupsOracle {
    const NO_STATE: &str = "this node-group surface is not bound to a document yet";

    const fn new() -> Self {
        Self { state: None }
    }

    fn attach(&mut self, state: Rc<GroupsState>) {
        self.state = Some(state);
    }

    fn bound(&self) -> Result<Rc<GroupsState>, InvokeError> {
        self.state
            .clone()
            .ok_or_else(|| InvokeError::rejected(Self::NO_STATE))
    }

    fn text(arg: &IntrospectValue) -> Result<String, InvokeError> {
        match arg {
            IntrospectValue::Text(s) => Ok(s.trim().to_owned()),
            _ => Err(InvokeError::TypeMismatch),
        }
    }

    fn number(arg: &IntrospectValue) -> Result<u32, InvokeError> {
        let raw = Self::text(arg)?;
        raw.parse()
            .map_err(|_| InvokeError::rejected(format!("{raw:?} is not a number")))
    }

    /// R1642 — read one segment against a **closed vocabulary**, with the
    /// vocabulary itself in the refusal.
    ///
    /// `parse` is the crate's own `from_wire`, so the set this admits is the set
    /// `names` publishes by construction — the property the round's conformance
    /// demo checks in both directions over the wire. Before this, the three
    /// vocabularies `arrange` and `item` read were spelled as match arms here
    /// while `$schema` published the crate's `WIRE_NAMES`, which is two
    /// definitions of one set: the declaration could go on advertising a value
    /// this parser had stopped accepting and nothing would notice.
    fn word<T>(
        what: &str,
        segment: Option<&str>,
        names: &[&'static str],
        parse: impl Fn(&str) -> Option<T>,
    ) -> Result<T, InvokeError> {
        let Some(segment) = segment else {
            return Err(InvokeError::rejected(format!(
                "{what} is missing; expected one of {names:?}"
            )));
        };
        parse(segment).ok_or_else(|| {
            InvokeError::rejected(format!("{what} {segment:?} is not one of {names:?}"))
        })
    }

    /// R1642 — refuse a segment past the ones the declaration accounts for.
    ///
    /// A delimited payload can always carry more words, and dropping them is the
    /// failure mode this round is about seen from the other side: the client is
    /// told nothing while the surface does something other than what was asked.
    fn no_more<'a>(
        what: &str,
        mut parts: impl Iterator<Item = &'a str>,
    ) -> Result<(), InvokeError> {
        match parts.next() {
            None => Ok(()),
            Some(extra) => Err(InvokeError::rejected(format!(
                "{what} takes no further segment, got {extra:?}"
            ))),
        }
    }

    /// `"3"` or `"3,5,8"` — a node id list.
    fn ids(arg: &IntrospectValue) -> Result<Vec<NodeId>, InvokeError> {
        let raw = Self::text(arg)?;
        if raw.is_empty() {
            return Ok(Vec::new());
        }
        raw.split(',')
            .map(|piece| {
                piece
                    .trim()
                    .parse()
                    .map(NodeId)
                    .map_err(|_| InvokeError::rejected(format!("{piece:?} is not a node id")))
            })
            .collect()
    }

    /// `"2.0>4.1"` — a socket pair.
    fn pair(arg: &IntrospectValue) -> Result<(Socket, Socket), InvokeError> {
        let raw = Self::text(arg)?;
        let (from, to) = raw.split_once('>').ok_or_else(|| {
            InvokeError::rejected(format!(
                "malformed argument {raw:?} (expected \"<node>.<port>><node>.<port>\")"
            ))
        })?;
        Ok((Self::socket(from)?, Self::socket(to)?))
    }

    fn socket(raw: &str) -> Result<Socket, InvokeError> {
        let (node, port) = raw.trim().split_once('.').ok_or_else(|| {
            InvokeError::rejected(format!(
                "malformed socket {raw:?} (expected \"<node>.<port>\")"
            ))
        })?;
        let node = node
            .parse()
            .map_err(|_| InvokeError::rejected(format!("{node:?} is not a node id")))?;
        let port = port
            .parse()
            .map_err(|_| InvokeError::rejected(format!("{port:?} is not a port index")))?;
        Ok(Socket::new(NodeId(node), port))
    }

    /// `"6"` — a node in the tree being edited — or `"0.6"`, a node in a named
    /// tree. The second form is what lets an AGENT act on a tree the user is
    /// not currently inside, which is the situation an edit path has to survive
    /// (§2 #2: the RPC surface addresses the document, not the view).
    fn addressed(arg: &IntrospectValue, default: TreeId) -> Result<(TreeId, NodeId), InvokeError> {
        let raw = Self::text(arg)?;
        let (tree, node) =
            match raw.split_once('.') {
                Some((tree, node)) => (
                    TreeId(tree.trim().parse().map_err(|_| {
                        InvokeError::rejected(format!("{tree:?} is not a tree id"))
                    })?),
                    node,
                ),
                None => (default, raw.as_str()),
            };
        let node = NodeId(
            node.trim()
                .parse()
                .map_err(|_| InvokeError::rejected(format!("{node:?} is not a node id")))?,
        );
        Ok((tree, node))
    }

    fn tree_arg(&self, arg: &IntrospectValue) -> Result<(Rc<GroupsState>, TreeId), InvokeError> {
        let state = self.bound()?;
        let tree = TreeId(Self::number(arg)?);
        if state.document.get().tree(tree).is_none() {
            return Err(InvokeError::rejected(format!("no tree {}", tree.0)));
        }
        Ok((state, tree))
    }
}

impl ExternalIntrospect for GroupsOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // The document as it stands.
                    SchemaField::new("trees", "int"),
                    SchemaField::new("definitions", "string"),
                    SchemaField::new("nodes", "int"),
                    SchemaField::new("links", "int"),
                    SchemaField::new("valid", "string"),
                    // Where the user is.
                    SchemaField::new("path", "string"),
                    SchemaField::new("depth", "int"),
                    SchemaField::new("current_tree", "int"),
                    SchemaField::new("selection", "string"),
                    // Why the last edit did not happen. The field an editor is
                    // judged by, and the one the DCC has no analogue for.
                    SchemaField::new("last_refusal", "string"),
                    // R1578 — the clipboard, as data. The DCC's is a .blend
                    // file in the temp directory, so none of this is askable
                    // there without pasting first.
                    SchemaField::new("clipboard", "string"),
                    SchemaField::new("clipboard_severed", "string"),
                    SchemaField::new("clipboard_bytes", "int"),
                    SchemaField::new("last_insert", "string"),
                    SchemaField::new("last_move", "string"),
                    // R1586 — how each node and wire takes part.
                    SchemaField::new("bypassed", "string"),
                    SchemaField::new("muted_links", "string"),
                    SchemaField::new("link_conversions", "string"),
                    SchemaField::new("last_rewire", "string"),
                    // R1589 — the containment forest, whole. Neither the DCC
                    // nor the toolkit has an accessor for "what contains what
                    // right now": `node::parent` is one pointer per node, so
                    // the relation exists only as something you reassemble.
                    SchemaField::new("frames", "string"),
                    // R1642 — the live bound `item`'s `side` argument points at.
                    SchemaField::new("item_sides", "string"),
                    // Argument-taking reads.
                    SchemaField::action("node_kind", "string"),
                    SchemaField::action("node_value", "string"),
                    SchemaField::action("node_ports", "string"),
                    SchemaField::action("interface", "string"),
                    SchemaField::action("instances", "string"),
                    SchemaField::action("tree_name", "string"),
                    SchemaField::action("passthrough", "string"),
                    SchemaField::action("visible_ports", "string"),
                    SchemaField::action("looks", "string"),
                    SchemaField::action("containment", "string"),
                    SchemaField::action("same_kind", "string"),
                    SchemaField::action("conversion", "string"),
                    SchemaField::action("port_values", "string"),
                    // The verbs.
                    SchemaField::action("select", "string"),
                    SchemaField::action("group", "string"),
                    SchemaField::action("ungroup", "string"),
                    SchemaField::action("instantiate", "string"),
                    SchemaField::action("group_insert", "string"),
                    SchemaField::action("group_separate", "string"),
                    SchemaField::action("fork", "string"),
                    SchemaField::action("copy", "string"),
                    SchemaField::action("paste", "string"),
                    SchemaField::action("duplicate", "string"),
                    SchemaField::action("enter", "string"),
                    SchemaField::action("exit", "string"),
                    SchemaField::action("add", "string"),
                    SchemaField::action("connect", "string"),
                    SchemaField::action("expose", "string"),
                    SchemaField::action("unexpose", "string"),
                    SchemaField::action("reset", "string"),
                    SchemaField::action("bypass", "string"),
                    SchemaField::action("mute_link", "string"),
                    SchemaField::action("dissolve", "string"),
                    SchemaField::action("detach", "string"),
                    SchemaField::action("collapse", "string"),
                    SchemaField::action("hide_ports", "string"),
                    SchemaField::action("frame", "string"),
                    SchemaField::action("unframe", "string"),
                    SchemaField::action("reparent", "string"),
                    SchemaField::action("set_value", "string"),
                    SchemaField::action("clear_value", "string"),
                    SchemaField::action("nudge", "string"),
                    SchemaField::action("grow", "string"),
                    // R1631 / R1632 — the two verbs whose whole point is that a
                    // family of reference commands is parameters here. Declared
                    // like every other, so a client discovers them the same way
                    // (`arrange` was added in R1631 and this list was not, which
                    // left it callable and undiscoverable).
                    // R1638 — and what it takes. The reference spells these
                    // arguments into eleven command NAMES (R1631), so its
                    // vocabulary cannot be enumerated at all; here the pass, the
                    // axis and the edge-or-gap are three declared segments whose
                    // vocabularies are projected from the crate's own enums.
                    // R1642 — and the third segment is no longer one `open`
                    // slot: the pass CHOOSES it, so the pass declares the cases
                    // and each says what it brings. `ARRANGE_CASES` for why.
                    SchemaField::action_with(
                        "arrange",
                        "string",
                        ArgForm::Delimited(':'),
                        const {
                            &[
                                SchemaArg::one_of_with("pass", "string", &ARRANGE_CASES),
                                SchemaArg::one_of("axis", "string", &Axis::WIRE_NAMES),
                            ]
                        },
                    ),
                    // R1642 — the second conditional verb, and the one a flat
                    // argument list cannot describe at all: `add:in:1` and
                    // `move:in:2:0` are different arities.
                    SchemaField::action_with(
                        "item",
                        "string",
                        ArgForm::Delimited(':'),
                        const {
                            &[
                                SchemaArg::one_of_with("verb", "string", &ITEM_CASES),
                                // Not the closed pair: which side answers is a
                                // property of the selected node's kind, so the
                                // argument names the live path that lists them.
                                SchemaArg::key("side", "string", "item_sides"),
                                // Open, and honestly so: the bound is the item
                                // count on the side named by the PREVIOUS
                                // argument, and `IndexOf` addresses one fixed
                                // path. A domain that depends on a sibling
                                // argument is the residue this round leaves —
                                // cases make an argument's PRESENCE depend on a
                                // sibling, not another argument's bound.
                                SchemaArg::open("index", "int"),
                            ]
                        },
                    ),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| ReadRefusal::unavailable("the editor holds no document yet"))?;
        let document = state.document.get();
        let tree = state.current();
        let int = |v: usize| Ok(IntrospectValue::Int(i64::try_from(v).unwrap_or(i64::MAX)));
        match path {
            "trees" => int(document.tree_count()),
            "definitions" => Ok(IntrospectValue::Text(
                document
                    .definitions()
                    .map(|t| format!("{}:{}", t.id.0, t.name))
                    .collect::<Vec<_>>()
                    .join(","),
            )),
            "nodes" => int(document
                .tree(tree)
                .map_or(0, pinion_node_graph::Tree::node_count)),
            "links" => int(document.tree(tree).map_or(0, |t| t.links().len())),
            "valid" => Ok(IntrospectValue::Text(if document.validate().is_empty() {
                "ok".to_owned()
            } else {
                format!("{:?}", document.validate())
            })),
            "path" => Ok(IntrospectValue::Text(
                state.path.get().breadcrumb(&document).join("/"),
            )),
            "depth" => int(state.path.get().depth()),
            "current_tree" => Ok(IntrospectValue::Int(i64::from(tree.0))),
            "selection" => Ok(IntrospectValue::Text(
                state
                    .selection
                    .get()
                    .iter()
                    .map(|n| n.0.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            )),
            "item_sides" => Ok(IntrospectValue::Text(item_sides(
                &document,
                tree,
                &state.selection.get(),
            ))),
            "last_refusal" => Ok(IntrospectValue::Text(state.refusal.get())),
            "clipboard" => {
                Ok(IntrospectValue::Text(state.clipboard.get().map_or_else(
                    || "empty".to_owned(),
                    |f| describe_fragment(&f),
                )))
            }
            "clipboard_severed" => Ok(IntrospectValue::Text(state.clipboard.get().map_or_else(
                String::new,
                |f| {
                    format!(
                        "in:{}|out:{}",
                        describe_severed(f.inbound()),
                        describe_severed(f.outbound())
                    )
                },
            ))),
            // A fragment is serializable, which is what makes it a clipboard
            // and not a handle into this process. Publishing the byte count is
            // the cheapest proof of that over the wire.
            "clipboard_bytes" => int(state
                .clipboard
                .get()
                .and_then(|f| serde_json::to_string(&f).ok())
                .map_or(0, |json| json.len())),
            "last_insert" => Ok(IntrospectValue::Text(state.last_insert.get())),
            "last_move" => Ok(IntrospectValue::Text(state.last_move.get())),
            "last_rewire" => Ok(IntrospectValue::Text(state.last_rewire.get())),
            "bypassed" => Ok(IntrospectValue::Text(join_ids(
                document
                    .tree(tree)
                    .into_iter()
                    .flat_map(pinion_node_graph::Tree::nodes)
                    .filter(|n| n.bypassed)
                    .map(|n| n.id.0),
            ))),
            "frames" => Ok(IntrospectValue::Text(
                document
                    .tree(tree)
                    .into_iter()
                    .flat_map(pinion_node_graph::Tree::nodes)
                    .filter(|n| n.is_frame())
                    .map(|n| {
                        format!(
                            "{}={}",
                            n.id.0,
                            join_ids(document.members(tree, n.id).into_iter().map(|m| m.0))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(";"),
            )),
            "muted_links" | "link_conversions" => {
                Self::link_query(&document, tree, path).ok_or(ReadRefusal::UnknownPath)
            }
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "trees" | "definitions" | "nodes" | "links" | "valid" | "path" | "depth"
            | "current_tree" | "selection" | "last_refusal" | "clipboard" | "clipboard_severed"
            | "clipboard_bytes" | "last_insert" | "last_move" | "bypassed" | "muted_links"
            | "link_conversions" | "last_rewire" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let outcome = match path {
            "node_kind" | "node_value" | "node_ports" | "interface" | "instances" | "tree_name"
            | "passthrough" | "visible_ports" | "looks" | "containment" | "same_kind"
            | "conversion" | "port_values" => self.read(path, &args),
            _ => self.verb(path, &args),
        };
        // R1584 — every refusal reaches the readout, whoever made it. The
        // substrate's arrive through `edit`, which records them; the
        // application's own — a malformed argument, a boundary that is not
        // there — reached the error frame and nothing else, so `last_refusal`
        // could show one kind of refusal and not the other. Recording it at the
        // one dispatch site is what makes "every refusal is showable" a
        // property of the surface rather than of each verb remembering.
        if let (Some(state), Err(InvokeError::Rejected(reason))) = (self.state.as_ref(), &outcome) {
            state.refusal.set(reason.to_string());
        }
        outcome
    }
}

impl GroupsOracle {
    /// The argument-taking reads.
    fn read(&self, path: &str, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let document = state.document.get();
        let tree = state.current();
        match path {
            "node_kind" => {
                let id = NodeId(Self::number(args)?);
                let node = document
                    .tree(tree)
                    .and_then(|t| t.node(id))
                    .ok_or_else(|| InvokeError::rejected(format!("no node {}", id.0)))?;
                Ok(IntrospectValue::Text(body_name(&node.body)))
            }
            "node_value" => {
                let id = NodeId(Self::number(args)?);
                let values = document.evaluate(tree, id);
                Ok(IntrospectValue::Text(
                    values
                        .iter()
                        .map(|v| v.as_ref().map_or_else(|| "null".to_owned(), Val::wire))
                        .collect::<Vec<_>>()
                        .join("|"),
                ))
            }
            "node_ports" => {
                let id = NodeId(Self::number(args)?);
                let signature = document
                    .signature(tree, id)
                    .ok_or_else(|| InvokeError::rejected(format!("no node {}", id.0)))?;
                Ok(IntrospectValue::Text(format!(
                    "in:{}|out:{}",
                    describe(&signature.inputs),
                    describe(&signature.outputs)
                )))
            }
            "interface" => {
                let (_, tree) = self.tree_arg(args)?;
                let document = state.document.get();
                let interface = document
                    .tree(tree)
                    .map(pinion_node_graph::Tree::interface)
                    .ok_or_else(|| InvokeError::rejected(format!("no tree {}", tree.0)))?;
                Ok(IntrospectValue::Text(format!(
                    "in:{}|out:{}",
                    describe(interface.inputs()),
                    describe(interface.outputs())
                )))
            }
            "instances" => {
                let (_, tree) = self.tree_arg(args)?;
                Ok(IntrospectValue::Int(
                    i64::try_from(state.document.get().instance_count(tree)).unwrap_or(i64::MAX),
                ))
            }
            "passthrough" | "visible_ports" | "looks" => self.participation_read(path, args),
            "conversion" => self.conversion_read(args),
            "port_values" => self.port_values_read(args),
            // R1594 — what has been authored on this node's own ports, and what
            // each of them therefore CARRIES. Two different facts: the first is
            // the node's, the second is the answer after the kind's declared
            // resting value and any link have had their say.
            "same_kind" => self.same_kind_read(args),
            "containment" => {
                let document = state.document.get();
                let tree = state.current();
                let id = NodeId(Self::number(args)?);
                let node = document
                    .tree(tree)
                    .and_then(|t| t.node(id))
                    .ok_or_else(|| InvokeError::rejected(format!("no node {}", id.0)))?;
                let list = |ids: &[NodeId]| {
                    ids.iter()
                        .map(|n| n.0.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                };
                Ok(IntrospectValue::Text(format!(
                    "frame:{} parent:{} ancestry:{} members:{} contents:{}",
                    node.is_frame(),
                    node.parent
                        .map_or_else(|| "-".to_owned(), |p| p.0.to_string()),
                    list(&document.ancestry(tree, id)),
                    list(&document.members(tree, id)),
                    list(&document.contents(tree, id)),
                )))
            }
            "tree_name" => {
                let (_, tree) = self.tree_arg(args)?;
                Ok(IntrospectValue::Text(
                    state
                        .document
                        .get()
                        .tree(tree)
                        .map(|t| t.name.clone())
                        .unwrap_or_default(),
                ))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }

    /// R1586 — the three reads that say how a node takes part: what would flow
    /// through it, which of its ports are drawn, and what it looks like. Split
    /// out of [`read`](Self::read) because they are one subject, the way the
    /// boundary and clipboard verbs already are.
    fn participation_read(
        &self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let document = state.document.get();
        let tree = state.current();
        match path {
            "passthrough" => {
                let id = NodeId(Self::number(args)?);
                let through = document
                    .passthrough(tree, id)
                    .ok_or_else(|| InvokeError::rejected(format!("no node {}", id.0)))?;
                let routes = through
                    .routes()
                    .iter()
                    .map(|r| format!("{}<-{}", r.output, r.input))
                    .collect::<Vec<_>>()
                    .join(",");
                // R1593 — which of those routes CHANGES the value on the way.
                // A separate field rather than a mark inside `routes`, so a
                // reader that predates the question still parses the same
                // string it always did.
                let converting = join_ids(
                    through
                        .routes()
                        .iter()
                        .filter(|r| r.converts)
                        .map(|r| r.output),
                );
                Ok(IntrospectValue::Text(format!(
                    "routes:{routes}|dropped:{}|unreached:{}|identity:{}|converting:{converting}",
                    join_ids(through.dropped_outputs().iter().copied()),
                    join_ids(through.unreached_inputs().iter().copied()),
                    through.is_identity(),
                )))
            }
            "visible_ports" => {
                let id = NodeId(Self::number(args)?);
                let shown = document
                    .visible_ports(tree, id)
                    .ok_or_else(|| InvokeError::rejected(format!("no node {}", id.0)))?;
                Ok(IntrospectValue::Text(format!(
                    "in:{}|out:{}|hidden_in:{}|hidden_out:{}",
                    join_ids(shown.inputs.iter().copied()),
                    join_ids(shown.outputs.iter().copied()),
                    join_ids(shown.hidden_inputs.iter().copied()),
                    join_ids(shown.hidden_outputs.iter().copied()),
                )))
            }
            "looks" => {
                let id = NodeId(Self::number(args)?);
                let node = document
                    .tree(tree)
                    .and_then(|t| t.node(id))
                    .ok_or_else(|| InvokeError::rejected(format!("no node {}", id.0)))?;
                let look = &node.appearance;
                Ok(IntrospectValue::Text(format!(
                    "bypassed:{}|collapsed:{}|hide_unused_ports:{}|options:{}|preview:{}|width:{}",
                    node.bypassed,
                    look.collapsed,
                    look.hide_unused_ports,
                    look.show_options,
                    look.show_preview,
                    look.width
                        .map_or_else(|| "auto".to_owned(), |w| w.to_string()),
                )))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }

    /// The two per-link facts, kept together because they are one subject —
    /// what each wire in this tree is doing with the value it was given.
    ///
    /// Split out of [`query`](Self::query) at clippy's own line limit, which is
    /// the same signal that produced [`participation_read`](Self::participation_read).
    fn link_query(document: &Document<Op>, tree: TreeId, path: &str) -> Option<IntrospectValue> {
        let links = document
            .tree(tree)
            .map(pinion_node_graph::Tree::links)
            .unwrap_or_default();
        match path {
            "muted_links" => Some(IntrospectValue::Text(join_ids(
                links.iter().filter(|l| l.muted).map(|l| l.id.0),
            ))),
            // R1593 — which of this tree's links carry their value through a
            // conversion. The DCC makes the same fact visible by materialising
            // a whole `implicit_conversion` node into the tree; here it is
            // derived from the link's two ends, so nothing can go stale and
            // nothing has to be drawn to be asked.
            "link_conversions" => Some(IntrospectValue::Text(
                links
                    .iter()
                    .map(|l| {
                        format!(
                            "{}={}",
                            l.id.0,
                            document
                                .link_conversion(tree, l.id)
                                .map_or("gone", |c| c.name())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(","),
            )),
            _ => None,
        }
    }

    /// R1593 — what would happen to a value going from one socket to another,
    /// asked BEFORE any wire exists.
    ///
    /// Takes the same argument spelling the `connect` verb does, so "may I?"
    /// and "do it" name the wire the same way — and answers with the two types
    /// as well as the verdict, because "refused" is only actionable if you are
    /// told what was refused.
    fn conversion_read(&self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let document = state.document.get();
        let tree = state.current();

        let (from, to) = Self::pair(args)?;
        let describe = |socket: Socket, side: InterfaceSide| {
            document
                .signature(tree, socket.node)
                .and_then(|sig| {
                    let ports = match side {
                        InterfaceSide::Input => sig.inputs,
                        InterfaceSide::Output => sig.outputs,
                    };
                    ports
                        .get(socket.port as usize)
                        .map(|p| p.value_type().map_or("control", |t| t.name()))
                })
                .unwrap_or("?")
        };
        let conversion = document
            .conversion(tree, from, to)
            .ok_or_else(|| InvokeError::rejected(format!("no socket {from} or {to}")))?;
        Ok(IntrospectValue::Text(format!(
            "{}->{} {}",
            describe(from, InterfaceSide::Output),
            describe(to, InterfaceSide::Input),
            conversion.name(),
        )))
    }

    /// R1594 — what has been authored on this node's own ports, and what each
    /// of its ports therefore CARRIES.
    ///
    /// Two different facts, and keeping them apart is the point: the first is
    /// the node's own, the second is the answer after a link, the node's value
    /// and the kind's declared resting value have each had their say in that
    /// order.
    fn port_values_read(&self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let document = state.document.get();
        let tree = state.current();
        let id = NodeId(Self::number(args)?);
        let node = document
            .tree(tree)
            .and_then(|t| t.node(id))
            .ok_or_else(|| InvokeError::rejected(format!("no node {}", id.0)))?;
        let authored = node
            .values
            .iter()
            .map(|(port, value)| format!("{port}={}", value.wire()))
            .collect::<Vec<_>>()
            .join(",");
        let show = |side: &str, values: Vec<Option<Val>>| {
            values
                .into_iter()
                .enumerate()
                .map(|(i, v)| {
                    format!(
                        "{side}{i}={}",
                        v.map_or_else(|| "null".to_owned(), |v| v.wire())
                    )
                })
                .collect::<Vec<_>>()
        };
        let mut evaluator = document.evaluator();
        let inputs = evaluator.inputs(tree, id);
        let outputs = evaluator.outputs(tree, id);
        let carries = show("in", inputs)
            .into_iter()
            .chain(show("out", outputs))
            .collect::<Vec<_>>()
            .join(",");
        Ok(IntrospectValue::Text(format!(
            "authored:{authored}|carries:{carries}"
        )))
    }

    /// The verbs. Each one is a call into the substrate plus the bookkeeping
    /// this application actually owns: what is selected, and where the user is.
    /// R1631 — run one arrangement pass over the current selection and apply
    /// it, answering how many nodes moved.
    ///
    /// The answer carries the count rather than a bare "ok" because a
    /// placement that changes nothing is a real outcome — an author pressing
    /// align twice should see `0`, and an undo stack keyed off this should
    /// record nothing. `straighten` additionally reports the links it could
    /// not straighten, which is the fact the reference's command does not
    /// publish at all.
    ///
    /// R1642 — the parse walks the schema's own shape: the discriminant, then the
    /// arguments every case shares, then the ones this case declared. A segment
    /// past that is **refused** rather than dropped, because the declaration says
    /// `distribute` takes none and a surface that quietly swallows one is how an
    /// author comes to believe a tool is broken (the field lesson behind plane
    /// B's `HOT` / `RESTART` badges: a setting that is accepted and ignored is
    /// worse than one that is rejected).
    fn arrange(&mut self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let tree = state.current();
        let spec = Self::text(args)?;
        let mut parts = spec.split(':');
        let pass = Self::word(
            "arrange pass",
            parts.next(),
            &ArrangePass::WIRE_NAMES,
            ArrangePass::from_wire,
        )?;
        let axis = Self::word(
            "arrange axis",
            parts.next(),
            &Axis::WIRE_NAMES,
            Axis::from_wire,
        )?;
        let tail = parts.next();
        Self::no_more("arrange", parts)?;
        let selection: BTreeSet<NodeId> = state.selection.get().into_iter().collect();
        let document = state.document.get();
        // The card's own geometry, which is the application's to know: the
        // crate asks for an extent precisely so it never has to guess one.
        let extent = |node: &Node<Op>| {
            let shown = document.visible_ports(tree, node.id).unwrap_or_default();
            Extent::new(
                CARD_W,
                card_height((shown.inputs.len(), shown.outputs.len())),
            )
        };
        // A pass that declared no tail is handed none: `ArrangeTail::None` is a
        // claim the dispatcher has to honour, not a permission to ignore.
        if matches!(pass.tail(), ArrangeTail::None) && tail.is_some() {
            return Err(InvokeError::rejected(format!(
                "arrange {} reads no third segment, got {tail:?}",
                pass.name()
            )));
        }
        let edge = |word: Option<&str>| {
            Self::word("arrange edge", word, &Edge::WIRE_NAMES, Edge::from_wire)
        };
        let (placement, report) = match pass {
            ArrangePass::Align => (
                Align::to(axis, edge(tail)?).run(&document, tree, &selection, extent),
                String::new(),
            ),
            ArrangePass::Distribute => (
                Distribute::along(axis).run(&document, tree, &selection, extent),
                String::new(),
            ),
            ArrangePass::Stack => {
                let gap: i32 = tail
                    .unwrap_or("0")
                    .parse()
                    .map_err(|_| InvokeError::rejected("arrange stack gap is not an integer"))?;
                (
                    Stack::along(axis, gap).run(&document, tree, &selection, extent),
                    String::new(),
                )
            }
            ArrangePass::Straighten => {
                let done = Straighten::along(axis).run(&document, tree, &selection);
                let bent = done
                    .bent()
                    .iter()
                    .map(|l| l.0.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                (
                    done.placement().clone(),
                    format!("|straight:{}|bent:{bent}", done.straight().len()),
                )
            }
        };
        drop(document);
        let moved = state
            .edit(|document| Ok::<usize, String>(document.apply(tree, &placement)))
            .map_err(InvokeError::rejected)?;
        Ok(IntrospectValue::Text(format!("moved:{moved}{report}")))
    }

    /// R1632 — add, remove or reorder one item of a node's variadic run.
    ///
    /// `add:<side>:<at>[:<label>]` / `remove:<side>:<at>` / `move:<side>:<from>:<to>`,
    /// over the single selected node. The answer carries what the edit cost —
    /// the ports it moved, the wires it cut and the authored values it handed
    /// back — because that is the half the reference's own commands do not
    /// publish: `RemoveExecutionPin` answers `void` after breaking the links.
    fn item(&mut self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let tree = state.current();
        let spec = Self::text(args)?;
        let mut parts = spec.split(':');
        let verb = Self::word(
            "item verb",
            parts.next(),
            &ItemEdit::WIRE_NAMES,
            ItemEdit::from_wire,
        )?;
        let side = Self::word(
            "item side",
            parts.next(),
            &Side::WIRE_NAMES,
            Side::from_wire,
        )?;
        let index = |what: &str, word: Option<&str>| -> Result<u32, InvokeError> {
            word.and_then(|w| w.parse().ok())
                .ok_or_else(|| InvokeError::rejected(format!("{what} {word:?} is not a number")))
        };
        let first = index("item index", parts.next())?;
        let tail = parts.next().map(str::to_owned);
        Self::no_more("item", parts)?;
        // Same rule as `arrange`: `remove` declared no fourth segment, so one
        // handed to it is a mistake the surface states rather than absorbs.
        if matches!(verb.tail(), ItemEditTail::None) && tail.is_some() {
            return Err(InvokeError::rejected(format!(
                "item {} reads no fourth segment, got {tail:?}",
                verb.name()
            )));
        }
        let selection = state.selection.get();
        let [node] = selection[..] else {
            return Err(InvokeError::rejected(format!(
                "item needs exactly one selected node, not {}",
                selection.len()
            )));
        };

        let change = match verb {
            ItemEdit::Add => {
                let mut item = Item::plain();
                if let Some(label) = tail {
                    item = item.named(label);
                }
                state.edit(|document| {
                    document
                        .insert_item(tree, node, side, first, item)
                        .map_err(|error| format!("{error:?}"))
                })
            }
            ItemEdit::Remove => state.edit(|document| {
                document
                    .remove_item(tree, node, side, first)
                    .map_err(|error| format!("{error:?}"))
            }),
            ItemEdit::Move => {
                let to = index("item destination", tail.as_deref())?;
                state.edit(|document| {
                    document
                        .move_item(tree, node, side, first, to)
                        .map_err(|error| format!("{error:?}"))
                })
            }
        }
        .map_err(InvokeError::rejected)?;

        let addresses = |list: &[PortRef]| {
            list.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        Ok(IntrospectValue::Text(format!(
            "items:{}|added:{}|moved:{}|severed:{}|discarded:{}",
            change.items,
            addresses(&change.added),
            change
                .moved
                .iter()
                .map(|(from, to)| format!("{from}>{to}"))
                .collect::<Vec<_>>()
                .join(","),
            change
                .severed
                .iter()
                .map(|link| format!("{}>{}", link.from, link.to))
                .collect::<Vec<_>>()
                .join(","),
            change
                .discarded
                .iter()
                .map(|(port, value)| format!("{port}={}", value.wire()))
                .collect::<Vec<_>>()
                .join(","),
        )))
    }

    fn verb(&mut self, path: &str, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let tree = state.current();
        match path {
            "enter" | "exit" | "add" | "connect" | "expose" | "unexpose" | "reset" => {
                return self.navigate(path, args);
            }
            _ => {}
        }
        let ok = |text: &str| Ok(IntrospectValue::Text(text.to_owned()));
        match path {
            "select" => {
                let ids = Self::ids(args)?;
                state.selection.set(ids.clone());
                ok(&ids.len().to_string())
            }
            "bypass" | "collapse" | "hide_ports" | "mute_link" | "dissolve" | "detach" => {
                self.participation(path, args)
            }
            // R1631 — the engine reference's eleven align / distribute / stack /
            // straighten commands, driven as ONE verb over the current
            // selection: `align:horizontal:start`, `distribute:vertical`,
            // `stack:horizontal:24`, `straighten:horizontal`. One verb because
            // the crate's vocabulary is parameters rather than eleven names,
            // and a surface that re-spelled them as eleven paths would throw
            // that away at the boundary.
            "arrange" => self.arrange(args),
            // R1632 — the engine reference's ten variadic-pin commands and the
            // DCC's four socket-item operators, driven as ONE verb:
            // `item add:in:2`, `item add:in:2:Overlay`, `item remove:in:0`,
            // `item move:in:2:0`. One verb for the same reason `arrange` is
            // one: the crate's vocabulary is two operations and a position,
            // and re-spelling them as fourteen paths would throw that away at
            // the boundary.
            "item" => self.item(args),
            "group" => {
                let name = Self::text(args)?;
                let selection = state.selection.get();
                let made = state
                    .edit(|document| document.group(tree, &selection, name))
                    .map_err(InvokeError::rejected)?;
                state.selection.set(vec![made.node]);
                ok(&format!(
                    "{}:{}|unframed:{}",
                    made.definition.0,
                    made.node.0,
                    describe_orphans(&made.orphaned)
                ))
            }
            "ungroup" => {
                let (host, id) = Self::addressed(args, tree)?;
                let out = state
                    .edit(|document| document.ungroup(host, id))
                    .map_err(InvokeError::rejected)?;
                state.selection.set(out.nodes.clone());
                // The path may have been inside what just went away.
                let mut path = state.path.get();
                path.prune(&state.document.get());
                state.path.set(path);
                ok(&format!(
                    "{} nodes, definition {}",
                    out.nodes.len(),
                    if out.definition_unused {
                        "unused"
                    } else {
                        "in use"
                    }
                ))
            }
            "instantiate" => {
                let definition = TreeId(Self::number(args)?);
                let node = state
                    .edit(|document| document.instantiate(tree, definition, 320, 300))
                    .map_err(InvokeError::rejected)?;
                state.selection.set(vec![node]);
                ok(&node.0.to_string())
            }
            "set_value" | "clear_value" => self.port_value(path, args),
            "frame" | "unframe" | "reparent" | "nudge" => self.containment(path, args),
            "grow" => self.grow(args),
            "copy" | "paste" | "duplicate" => self.clipboard(path, args),
            "group_insert" | "group_separate" | "fork" => self.boundary(path, args),
            _ => Err(InvokeError::UnknownPath),
        }
    }

    /// R1594 — author a value on one of a node's own ports, or take it back.
    ///
    /// `set_value "3.out0=200,60,60"` and `clear_value "3.out0"`. The port is
    /// spelled the way [`PortRef`] prints itself, so the argument and the
    /// readout use one vocabulary.
    fn port_value(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let tree = state.current();
        let raw = Self::text(args)?;
        let (target, value) = match raw.split_once('=') {
            Some((target, value)) => (target, Some(value)),
            None => (raw.as_str(), None),
        };
        let (node, port) = target.trim().split_once('.').ok_or_else(|| {
            InvokeError::rejected(format!(
                "malformed argument {target:?} (expected \"<node>.<in|out><port>\")"
            ))
        })?;
        let node = NodeId(
            node.trim()
                .parse()
                .map_err(|_| InvokeError::rejected(format!("{node:?} is not a node id")))?,
        );
        let port = Self::port_ref(port)?;
        if path == "set_value" {
            {
                let value = value.ok_or_else(|| {
                    InvokeError::rejected("set_value needs \"<node>.<port>=<value>\"".to_owned())
                })?;
                let parsed = Val::parse(value).ok_or_else(|| {
                    InvokeError::rejected(format!("{value:?} is not a colour or an amount"))
                })?;
                let replaced = state
                    .edit(|document| document.set_port_value(tree, node, port, parsed.clone()))
                    .map_err(InvokeError::rejected)?;
                Ok(IntrospectValue::Text(replaced.map_or_else(
                    || "authored".to_owned(),
                    |old| format!("authored, was {}", old.wire()),
                )))
            }
        } else {
            let cleared = state
                .edit(|document| document.clear_port_value(tree, node, port))
                .map_err(InvokeError::rejected)?;
            Ok(IntrospectValue::Text(cleared.map_or_else(
                || "nothing was authored".to_owned(),
                |old| format!("cleared {}", old.wire()),
            )))
        }
    }

    /// `"in0"` / `"out2"` — the spelling [`PortRef`] prints.
    fn port_ref(raw: &str) -> Result<PortRef, InvokeError> {
        let raw = raw.trim();
        let malformed = || {
            InvokeError::rejected(format!(
                "malformed port {raw:?} (expected \"in<N>\" or \"out<N>\")"
            ))
        };
        if let Some(index) = raw.strip_prefix("out") {
            return Ok(PortRef::output(index.parse().map_err(|_| malformed())?));
        }
        let index = raw.strip_prefix("in").ok_or_else(malformed)?;
        Ok(PortRef::input(index.parse().map_err(|_| malformed())?))
    }

    /// R1590 — the run of nodes that do what this one does, in evaluation
    /// order, with the subject's place in it.
    ///
    /// "3 of 7" is the fact `select_same_type_step` cannot answer: it
    /// reports by moving the active node and says only whether it moved.
    fn same_kind_read(&self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let document = state.document.get();
        let tree = state.current();
        let id = NodeId(Self::number(args)?);
        let run = document
            .same_kind_run(tree, id)
            .ok_or_else(|| InvokeError::rejected(format!("no node {}", id.0)))?;
        let at = run.iter().position(|&n| n == id).unwrap_or_default();
        Ok(IntrospectValue::Text(format!(
            "at:{} of:{} run:{}",
            at + 1,
            run.len(),
            join_ids(run.iter().map(|n| n.0))
        )))
    }

    /// R1590 — grow the selection by one question about the graph.
    ///
    /// The whole of what this application supplies is the **word**: the
    /// derivation, the reach and the refusal are the substrate's, and the
    /// selection this grows is the editor's because a selection belongs to
    /// whoever is looking rather than to the document.
    fn grow(&mut self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let tree = state.current();
        let raw = Self::text(args)?;
        let (word, reach) = raw.split_once(':').unwrap_or((raw.as_str(), "direct"));
        let reach = match reach.trim() {
            "direct" => Reach::Direct,
            "transitive" => Reach::Transitive,
            other => {
                return Err(InvokeError::rejected(format!(
                    "{other:?} is not a reach (expected \"direct\" or \"transitive\")"
                )));
            }
        };
        let by = match word.trim() {
            "downstream" => Grow::Downstream(reach),
            "upstream" => Grow::Upstream(reach),
            "contents" => Grow::Contents(reach),
            "containers" => Grow::Containers(reach),
            "same_kind" => Grow::SameKind,
            "prefix" => Grow::NamePrefix,
            "suffix" => Grow::NameSuffix,
            other => {
                return Err(InvokeError::rejected(format!(
                    "{other:?} is not a way to grow a selection"
                )));
            }
        };
        // A pure query: the document is not touched, so this does not go
        // through `edit` and nothing is marked dirty by asking.
        let grown = state
            .document
            .get()
            .grow(tree, &state.selection.get(), by)
            .map_err(|error| InvokeError::rejected(error.to_string()))?;
        state.selection.set(grown.selection.clone());
        Ok(IntrospectValue::Text(format!(
            "added:{}|now:{}",
            join_ids(grown.added.iter().map(|n| n.0)),
            grown.selection.len()
        )))
    }

    /// R1589 — the containment gestures. Every one of them is one substrate
    /// call: the derivation that decides *which* nodes each acts on lives in
    /// the crate ([`Document::outermost`]), so this application supplies only
    /// the selection and the argument.
    fn containment(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let tree = state.current();
        let ok = |text: String| Ok(IntrospectValue::Text(text));
        match path {
            "frame" => {
                let label = Self::text(args)?;
                let selection = state.selection.get();
                let made: Enframed = state
                    .edit(|document| {
                        document.enframe(
                            tree,
                            &selection,
                            (!label.is_empty()).then(|| label.clone()),
                        )
                    })
                    .map_err(InvokeError::rejected)?;
                state.selection.set(vec![made.frame]);
                ok(format!(
                    "{}:{}",
                    made.frame.0,
                    made.members
                        .iter()
                        .map(|n| n.0.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                ))
            }
            "unframe" => {
                let selection = state.selection.get();
                let moved = state
                    .edit(|document| document.unframe(tree, &selection))
                    .map_err(InvokeError::rejected)?;
                ok(moved
                    .iter()
                    .map(|n| n.0.to_string())
                    .collect::<Vec<_>>()
                    .join(","))
            }
            "reparent" => {
                let (node, into) = Self::containment_arg(args)?;
                let was = state
                    .edit(|document| document.set_parent(tree, node, into))
                    .map_err(InvokeError::rejected)?;
                ok(was.map_or_else(|| "-".to_owned(), |p| p.0.to_string()))
            }
            "nudge" => {
                let (node, dx, dy) = Self::nudge_arg(args)?;
                let moved = state
                    .edit(|document| document.translate(tree, node, dx, dy))
                    .map_err(InvokeError::rejected)?;
                ok(moved
                    .iter()
                    .map(|n| n.0.to_string())
                    .collect::<Vec<_>>()
                    .join(","))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }

    /// `"3>7"` puts node 3 inside frame 7; `"3>-"` takes it out of everything.
    fn containment_arg(arg: &IntrospectValue) -> Result<(NodeId, Option<NodeId>), InvokeError> {
        let raw = Self::text(arg)?;
        let (node, into) = raw.split_once('>').ok_or_else(|| {
            InvokeError::rejected(format!(
                "malformed argument {raw:?} (expected \"<node>><frame|->\")"
            ))
        })?;
        let node = NodeId(
            node.trim()
                .parse()
                .map_err(|_| InvokeError::rejected(format!("{node:?} is not a node id")))?,
        );
        let into = match into.trim() {
            "-" => None,
            frame => Some(NodeId(frame.parse().map_err(|_| {
                InvokeError::rejected(format!("{frame:?} is not a node id"))
            })?)),
        };
        Ok((node, into))
    }

    /// `"7:40:-10"` — move node 7 right 40 and up 10.
    fn nudge_arg(arg: &IntrospectValue) -> Result<(NodeId, i32, i32), InvokeError> {
        let raw = Self::text(arg)?;
        let parts: Vec<&str> = raw.split(':').collect();
        let [node, dx, dy] = parts.as_slice() else {
            return Err(InvokeError::rejected(format!(
                "malformed argument {raw:?} (expected \"<node>:<dx>:<dy>\")"
            )));
        };
        let number = |raw: &str| {
            raw.trim()
                .parse::<i32>()
                .map_err(|_| InvokeError::rejected(format!("{raw:?} is not a number")))
        };
        Ok((
            NodeId(
                node.trim()
                    .parse()
                    .map_err(|_| InvokeError::rejected(format!("{node:?} is not a node id")))?,
            ),
            number(dx)?,
            number(dy)?,
        ))
    }

    /// R1584 — the two directions of a boundary move, and the fork that makes
    /// either of them local.
    ///
    /// The whole of what this application supplies is *where the boundary is*.
    /// Inward it is named by the argument, because the user is looking at the
    /// host tree and pointing at a group. Outward it is the edit path's own
    /// last step — the user is inside the group, so the group they are inside
    /// IS the boundary — which is the same place the DCC reads it from
    /// (`snode->edittree` against `tree_get(snode, 1)`), and refusing at the
    /// root is its "Not inside node group". R1586 — the verbs that change how
    /// a node or a wire takes part.
    ///
    /// `bypass` and `mute_link` change what the graph *means*; `collapse` and
    /// `hide_ports` change only what it looks like; `dissolve` and `detach`
    /// apply the bypass derivation to the structure. Grouped here because the
    /// difference between those three kinds is the round's whole subject.
    fn participation(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let tree = state.current();
        let ok = |text: &str| Ok(IntrospectValue::Text(text.to_owned()));
        match path {
            // R1586 — a node says how it takes part. `bypass` changes the
            // meaning; `collapse` and `hide_ports` change only the picture, and
            // the demo asserts that difference over the wire.
            "bypass" | "collapse" | "hide_ports" => {
                let ids = Self::ids(args)?;
                let mut document = state.document.get();
                let mut changed = Vec::new();
                for id in ids {
                    let node = document
                        .tree_mut(tree)
                        .and_then(|t| t.node_mut(id))
                        .ok_or_else(|| InvokeError::rejected(format!("no node {}", id.0)))?;
                    let slot = match path {
                        "bypass" => &mut node.bypassed,
                        "collapse" => &mut node.appearance.collapsed,
                        _ => &mut node.appearance.hide_unused_ports,
                    };
                    *slot = !*slot;
                    changed.push(format!("{}={}", id.0, *slot));
                }
                state.document.set(document);
                state.refusal.set(String::new());
                ok(&changed.join(","))
            }
            "mute_link" => {
                let id = LinkId(Self::number(args)?);
                let mut document = state.document.get();
                let was = document
                    .tree(tree)
                    .and_then(|t| t.link(id))
                    .map(|l| l.muted)
                    .ok_or_else(|| InvokeError::rejected(format!("no link {}", id.0)))?;
                document
                    .set_link_muted(tree, id, !was)
                    .map_err(|e| InvokeError::rejected(e.to_string()))?;
                state.document.set(document);
                state.refusal.set(String::new());
                ok(if was { "unmuted" } else { "muted" })
            }
            "dissolve" | "detach" => {
                let id = NodeId(Self::number(args)?);
                let keep = path == "detach";
                let out = state
                    .edit(|document| {
                        if keep {
                            document.detach(tree, id)
                        } else {
                            document.dissolve(tree, id)
                        }
                    })
                    .map_err(InvokeError::rejected)?;
                state.last_rewire.set(describe_rewire(&out));
                if !keep {
                    state.selection.set(
                        state
                            .selection
                            .get()
                            .into_iter()
                            .filter(|n| *n != id)
                            .collect(),
                    );
                }
                ok(&describe_rewire(&out))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }

    fn boundary(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let raw = Self::text(args)?;
        // `"<instance>"`, or with the sharing arm named: `"<instance>,fork"`.
        // Stated at the call, like R1578's `fork`/`share`, because "does this
        // also change the group's other users" is not a preference.
        let mut sharing = Sharing::Shared;
        let mut address = IntrospectValue::Text(String::new());
        for piece in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match piece {
                "fork" => sharing = Sharing::Fork,
                "shared" => sharing = Sharing::Shared,
                other => address = IntrospectValue::Text(other.to_owned()),
            }
        }
        if path == "fork" {
            let (host, instance) = Self::addressed(&address, state.current())?;
            let copy = state
                .edit(|document| document.fork_definition(host, instance))
                .map_err(InvokeError::rejected)?;
            return Ok(IntrospectValue::Text(copy.0.to_string()));
        }

        let selection = state.selection.get();
        let out = if path == "group_insert" {
            let host = state.current();
            let instance = Self::addressed(&address, host)?.1;
            state
                .edit(|document| document.group_insert(host, instance, &selection, sharing))
                .map_err(InvokeError::rejected)?
        } else {
            let path_now = state.path.get();
            let entries = path_now.entries();
            let (Some(step), Some(above)) = (
                entries.last().and_then(|entry| entry.via),
                entries.len().checked_sub(2).and_then(|at| entries.get(at)),
            ) else {
                return Err(InvokeError::rejected(
                    "not inside a group: separate moves nodes out to the tree above",
                ));
            };
            let host = above.tree;
            let out = state
                .edit(|document| document.group_separate(host, step, &selection, sharing))
                .map_err(InvokeError::rejected)?;
            // The nodes are in the tree above now, so that is where the user is.
            let mut walked = state.path.get();
            let _ = walked.exit();
            state.path.set(walked);
            out
        };
        state.selection.set(out.moved.clone());
        let described = describe_move(&out);
        state.last_move.set(described.clone());
        Ok(IntrospectValue::Text(described))
    }

    /// R1578 — copy, paste and duplicate, which are three call sites of one
    /// substrate operation: lift a piece of the graph out as a value, and put a
    /// value back in.
    ///
    /// The whole of what this application supplies is where the value is *kept*
    /// and how the two policy arms are spelled on the wire.
    fn clipboard(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let tree = state.current();
        let ok = |text: &str| Ok(IntrospectValue::Text(text.to_owned()));
        match path {
            "copy" => {
                let selection = state.selection.get();
                let fragment = state
                    .document
                    .get()
                    .extract(tree, &selection)
                    .map_err(|error| {
                        state.refusal.set(error.to_string());
                        InvokeError::rejected(error.to_string())
                    })?;
                let described = describe_fragment(&fragment);
                state.clipboard.set(Some(fragment));
                state.refusal.set(String::new());
                ok(&described)
            }
            "paste" => {
                let Placement {
                    point,
                    crossings,
                    definitions,
                } = Self::placement(args)?;
                let fragment = state
                    .clipboard
                    .get()
                    .ok_or_else(|| InvokeError::rejected("the clipboard is empty"))?;
                let out = state
                    .edit(|document| {
                        document.insert(tree, &fragment, point, crossings, definitions)
                    })
                    .map_err(InvokeError::rejected)?;
                state.selection.set(out.nodes.clone());
                let described = describe_insert(&out);
                state.last_insert.set(described.clone());
                ok(&described)
            }
            "duplicate" => {
                let Placement {
                    point,
                    crossings,
                    definitions,
                } = Self::placement(args)?;
                let selection = state.selection.get();
                let out = state
                    .edit(|document| {
                        document.duplicate(tree, &selection, point, crossings, definitions)
                    })
                    .map_err(InvokeError::rejected)?;
                state.selection.set(out.nodes.clone());
                let described = describe_insert(&out);
                state.last_insert.set(described.clone());
                ok(&described)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }

    /// `"600,300"`, or with either policy named: `"600,300,keep,fork"`.
    ///
    /// The two arms are *stated at the call*. The DCC's `linked` arm defaults from
    /// a user preference (`U.dupflag & USER_DUP_NTREE`), so whether an edit to the copy also changes
    /// the original depends on a setting the gesture does not mention.
    fn placement(arg: &IntrospectValue) -> Result<Placement, InvokeError> {
        let raw = Self::text(arg)?;
        let mut point = (0_i32, 0_i32);
        let mut crossings = Crossings::Drop;
        let mut definitions = Definitions::Share;
        let mut coordinates = 0;
        for piece in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match piece {
                "keep" => crossings = Crossings::KeepInbound,
                "drop" => crossings = Crossings::Drop,
                "fork" => definitions = Definitions::Fork,
                "share" => definitions = Definitions::Share,
                number => {
                    let value: i32 = number.parse().map_err(|_| {
                        InvokeError::rejected(format!(
                            "{number:?} is neither a coordinate nor one of \
                             keep/drop/fork/share"
                        ))
                    })?;
                    match coordinates {
                        0 => point.0 = value,
                        1 => point.1 = value,
                        _ => {
                            return Err(InvokeError::rejected("a placement takes two coordinates"));
                        }
                    }
                    coordinates += 1;
                }
            }
        }
        if coordinates != 2 {
            return Err(InvokeError::rejected(
                "a placement needs \"<x>,<y>\", optionally with keep/drop and fork/share",
            ));
        }
        Ok(Placement {
            point,
            crossings,
            definitions,
        })
    }

    /// Navigation, the palette, and the interface edits: the verbs that do not
    /// change which nodes exist in the current tree.
    fn navigate(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.bound()?;
        let tree = state.current();
        let ok = |text: &str| Ok(IntrospectValue::Text(text.to_owned()));
        match path {
            "enter" => {
                let id = NodeId(Self::number(args)?);
                let document = state.document.get();
                let mut path = state.path.get();
                let inner = path.enter(&document, id).map_err(|error| {
                    state.refusal.set(error.to_string());
                    InvokeError::rejected(error.to_string())
                })?;
                state.refusal.set(String::new());
                state.path.set(path);
                state.selection.set(Vec::new());
                ok(&inner.0.to_string())
            }
            "exit" => {
                let mut path = state.path.get();
                let outer = path.exit().map_err(|error| {
                    state.refusal.set(error.to_string());
                    InvokeError::rejected(error.to_string())
                })?;
                state.refusal.set(String::new());
                state.path.set(path);
                state.selection.set(Vec::new());
                ok(&outer.0.to_string())
            }
            "add" => {
                let op = Op::parse(&Self::text(args)?).ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "{:?} is not one of {}",
                        Self::text(args).unwrap_or_default(),
                        Op::PALETTE.join("/")
                    ))
                })?;
                let placed = state
                    .edit(|document| document.add_node(tree, NodeBody::Kind(op), 320, 380))
                    .map_err(InvokeError::rejected)?;
                ok(&placed.0.to_string())
            }
            "connect" => {
                let (from, to) = Self::pair(args)?;
                let outcome = state
                    .edit(|document| document.connect(tree, from, to))
                    .map_err(InvokeError::rejected)?;
                ok(&outcome.displaced.map_or_else(
                    || "linked".to_owned(),
                    |link| format!("linked, displaced {}", link.id.0),
                ))
            }
            "expose" => {
                let (_, target) = self.tree_arg(args)?;
                let index = state
                    .edit(|document| {
                        document.expose(
                            target,
                            InterfaceSide::Input,
                            Port::new("Extra", Ty::Amount).with_default(Val::Amount(0)),
                        )
                    })
                    .map_err(InvokeError::rejected)?;
                ok(&index.to_string())
            }
            "unexpose" => {
                let raw = Self::text(args)?;
                let (tree_raw, index_raw) = raw.split_once('.').ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "malformed argument {raw:?} (expected \"<tree>.<index>\")"
                    ))
                })?;
                let target =
                    TreeId(tree_raw.parse().map_err(|_| {
                        InvokeError::rejected(format!("{tree_raw:?} is not a tree"))
                    })?);
                let index: u32 = index_raw.parse().map_err(|_| {
                    InvokeError::rejected(format!("{index_raw:?} is not a port index"))
                })?;
                let dropped = state
                    .edit(|document| document.unexpose(target, InterfaceSide::Input, index))
                    .map_err(InvokeError::rejected)?;
                ok(&dropped.len().to_string())
            }
            "reset" => {
                state.document.set(seed());
                state.path.set(EditPath::root());
                state.selection.set(Vec::new());
                state.refusal.set(String::new());
                // The clipboard deliberately SURVIVES: a fragment is a value,
                // not a view into the document it came from, and outliving the
                // document is the whole of what makes it a clipboard. The
                // insertion report does not — it describes edits to a document
                // that is gone.
                state.last_insert.set(String::new());
                ok("reset")
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// `"Base:colour,Blend:colour"` — a port list as the wire sees it.
/// `1,4,7` — the wire form for a list of small numbers, empty when there are
/// none. One spelling, so an agent parses every id list the same way.
fn join_ids(ids: impl Iterator<Item = u32>) -> String {
    ids.map(|id| id.to_string()).collect::<Vec<_>>().join(",")
}

/// R1586 — a rewire in one line: what was bridged, and what nothing reached.
///
/// The second half is the part the DCC's `node_internal_relink` removes and
/// never mentions, so it is the reason this read exists at all.
fn describe_rewire(out: &Rewired) -> String {
    let bridges = out
        .bridged
        .iter()
        .map(|b| {
            format!(
                "{}.{}->{}.{}{}",
                b.from.node.0,
                b.from.port,
                b.to.node.0,
                b.to.port,
                if b.muted { " (muted)" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let severed = out
        .severed
        .iter()
        .map(|l| format!("{}.{}", l.to.node.0, l.to.port))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "bridged:{bridges}|severed:{severed}|removed:{}|lossless:{}",
        out.removed.len(),
        out.lossless()
    )
}

/// `Ceiling:amount(off)` — a port declared off the bypass path is marked, so an
/// agent reads the DECLARATION beside the derivation `passthrough` answers.
fn describe(ports: &[Port<Ty, Val>]) -> String {
    ports
        .iter()
        .map(|p| {
            let off = if p.passthrough { "" } else { "(off)" };
            format!(
                "{}:{}{off}",
                p.name,
                p.value_type().map_or("control", |t| t.name())
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

impl External for GroupsOracle {
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

// --- The widget ---------------------------------------------------------------

struct NodeGroupsView;

impl WidgetCore for NodeGroupsView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = GroupsOracle::new();
        oracle.attach(use_groups_state());
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
        "pinion hello-node-groups (R1577 §5.38 §5.52)"
    }
}

impl WidgetA11y for NodeGroupsView {
    /// Where the user is and what is around them. A nested definition is
    /// exactly the state an AT user cannot infer from a drawing they cannot
    /// see, so the breadcrumb is in the value text rather than only on screen.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_groups_state();
        let document = state.document.get();
        let path = state.path.get();
        let tree = path.current();
        let nodes = document
            .tree(tree)
            .map_or(0, pinion_node_graph::Tree::node_count);
        vec![
            AccessNode::new(VIEW_TAG, AriaRole::Group)
                .with_name("Material node graph")
                .with_value(AccessValue::Text(format!(
                    "editing {}, {nodes} nodes, {} definitions, depth {}",
                    path.breadcrumb(&document).join(" in "),
                    document.tree_count() - 1,
                    path.depth()
                ))),
        ]
    }
}

impl WidgetView for NodeGroupsView {
    type Renderer = HelloNodeGroupsRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<NodeGroupsView>();
}

#[cfg(test)]
mod tests;
