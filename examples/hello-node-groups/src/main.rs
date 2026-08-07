//! R1577 §5.38 §5.52 — a Blender-class node system, composed.
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
//! the pipeline. A node can be **bypassed** — it stops computing and the values
//! at its inputs pass through it — or **dissolved**, which does the same thing
//! to the structure and deletes it. Both read one derivation, so the preview
//! and the edit cannot disagree; `passthrough.<id>` publishes it, including the
//! outputs no input can feed, which is the value an author most needs told is
//! about to disappear. A **link** can be muted, which is the opposite
//! behaviour — the value stops — and so is a different word here than in
//! Blender, where both are "mute".
//!
//! R1584 adds the two boundary moves, and with them the fact an editor is
//! obliged to show and Blender does not: a group definition is *shared*, so
//! moving a node into one through this instance changes every other instance
//! too. `last_move` says which ports appeared, which disappeared, which links
//! died and where, and how many other instances came along — or `fork` first,
//! and none of them do.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{
    BoxNode, ContainerNode, PathCommand, PathNode, PathPoint, Rect, TextNode,
};
use pinion_core::style::{
    Border, BoxStyle, Color, Dash, LayoutStyle, PathStyle, Size, Stroke, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_node_graph::{
    Crossings, Definitions, Document, EditPath, Fragment, Inserted, InterfaceSide, LinkId,
    NodeBody, NodeId, NodeKind, Port, PortChange, ROOT, Repartitioned, Rewired, Severed, Sharing,
    Socket, TreeId,
};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloNodeGroupsRenderer, HelloNodeGroupsRendererError);

const THEME_TAG: &str = "app";
const VIEW_TAG: &str = "nodegroups";
const STATE_KEY: &str = "nodegroups-state";

const WIN_W: u32 = 900;
const WIN_H: u32 = 560;
const CARD_W: i32 = 150;
const ROW_H: i32 = 18;
const HEAD_H: i32 = 26;
const PORT: i32 = 9;
const CANVAS_TOP: i32 = 76;
const TITLE_FONT_PX: u32 = 16;
const LABEL_FONT_PX: u32 = 12;
const STATUS_FONT_PX: u32 = 12;

// --- The taxonomy: the whole of what this application supplies ----------------

/// The two socket types. Everything else about typing — that a link needs
/// agreement, that a refusal names both ends — is the substrate's.
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
    Swatch([i64; 3]),
    Level(i64),
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
    /// The sink: its resolved input is the material's result.
    Output,
}

impl Op {
    /// Parse the palette name a `add` verb takes.
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "swatch" => Self::Swatch([128, 128, 128]),
            "level" => Self::Level(50),
            "mix" => Self::Mix,
            "fade" => Self::Fade,
            "cap" => Self::Cap,
            "output" => Self::Output,
            _ => return None,
        })
    }
}

impl NodeKind for Op {
    type Type = Ty;
    type Value = Val;

    fn name(&self) -> String {
        match self {
            Self::Swatch(_) => "Swatch",
            Self::Level(_) => "Level",
            Self::Mix => "Mix",
            Self::Fade => "Fade",
            Self::Cap => "Cap",
            Self::Output => "Output",
        }
        .to_owned()
    }

    fn inputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Swatch(_) | Self::Level(_) => Vec::new(),
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
            Self::Output => vec![Port::new("Surface", Ty::Colour)],
        }
    }

    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Swatch(_) | Self::Mix | Self::Fade => vec![Port::new("Colour", Ty::Colour)],
            Self::Level(_) => vec![Port::new("Amount", Ty::Amount)],
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
            Self::Swatch(rgb) => vec![Some(Val::Colour(*rgb))],
            Self::Level(a) => vec![Some(Val::Amount(*a))],
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
            Self::Output => Vec::new(),
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
    /// it could not. The second half is the one Blender's `node_internal_relink`
    /// discards, and it is what tells an author a value has just gone.
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
    let base = add(&mut document, Op::Swatch([200, 60, 60]), 20, 0);
    let blend = add(&mut document, Op::Swatch([40, 90, 220]), 20, 90);
    let level = add(&mut document, Op::Level(25), 20, 180);
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

/// Where a socket's pin sits, in canvas coordinates.
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
                BoxStyle::filled(theme.resolve(match declared.ty {
                    Ty::Colour => ColorRole::Accent,
                    Ty::Amount => ColorRole::OnSurfaceMuted,
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
/// an author most needs to see, and the one Blender's derivation discards.
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
            wire_scene(
                pin_at(at.0, at.1, position(&shown.inputs, route.input), false),
                pin_at(at.0, at.1, position(&shown.outputs, route.output), true),
                theme.resolve(ColorRole::OnSurfaceMuted),
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

fn pin_at(x: i32, y: i32, index: usize, output: bool) -> (i32, i32) {
    let px = if output {
        x + CARD_W - PORT / 2
    } else {
        x + PORT / 2
    };
    (px, y + HEAD_H + rows(index) * ROW_H + ROW_H / 2)
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
        NodeBody::Group(_) => ColorRole::SurfaceContainerHighest,
        NodeBody::Interface(_) => ColorRole::SurfaceContainerLow,
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

/// A wire: a cubic whose handles are horizontal, the shape every node editor
/// draws.
fn wire_scene(from: (i32, i32), to: (i32, i32), colour: Color, tag: String) -> Scene {
    wire_with(from, to, Stroke::new(colour, 2), tag)
}

/// The same wire, dashed: drawn, and carrying nothing (R1586).
fn muted_wire_scene(from: (i32, i32), to: (i32, i32), colour: Color, tag: String) -> Scene {
    wire_with(
        from,
        to,
        Stroke::new(colour, 2).with_dash(Dash::DASHED),
        tag,
    )
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
            children.push(if link.muted {
                // R1586 — a muted wire is still a wire. Dashed, because what
                // changed is whether it carries a value and not whether it is
                // there, and R1575 gave the stroke that vocabulary.
                muted_wire_scene(
                    from,
                    to,
                    theme.resolve(ColorRole::OnSurfaceMuted),
                    format!("{VIEW_TAG}.wire.{}", link.id.0),
                )
            } else {
                wire_scene(
                    from,
                    to,
                    wire_colour,
                    format!("{VIEW_TAG}.wire.{}", link.id.0),
                )
            });
        }
        for node in host.nodes() {
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
        "def:{}|forked_from:{}|moved:{}|exposed:{}|unexposed:{}|severed:{}|others:{}",
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
        out.other_instances
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
        "nodes:{}|links:{}|added:{}|reused:{}|reattached:{}|unattached:{}",
        out.nodes.len(),
        out.links.len(),
        ids(&out.definitions_added),
        ids(&out.definitions_reused),
        out.reattached.len(),
        describe_severed(&out.unattached)
    )
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
                    // judged by, and the one Blender has no analogue for.
                    SchemaField::new("last_refusal", "string"),
                    // R1578 — the clipboard, as data. Blender's is a .blend
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
                    SchemaField::new("last_rewire", "string"),
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
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let state = self.state.as_ref()?;
        let document = state.document.get();
        let tree = state.current();
        let int = |v: usize| Some(IntrospectValue::Int(i64::try_from(v).unwrap_or(i64::MAX)));
        match path {
            "trees" => int(document.tree_count()),
            "definitions" => Some(IntrospectValue::Text(
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
            "valid" => Some(IntrospectValue::Text(if document.validate().is_empty() {
                "ok".to_owned()
            } else {
                format!("{:?}", document.validate())
            })),
            "path" => Some(IntrospectValue::Text(
                state.path.get().breadcrumb(&document).join("/"),
            )),
            "depth" => int(state.path.get().depth()),
            "current_tree" => Some(IntrospectValue::Int(i64::from(tree.0))),
            "selection" => Some(IntrospectValue::Text(
                state
                    .selection
                    .get()
                    .iter()
                    .map(|n| n.0.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            )),
            "last_refusal" => Some(IntrospectValue::Text(state.refusal.get())),
            "clipboard" => {
                Some(IntrospectValue::Text(state.clipboard.get().map_or_else(
                    || "empty".to_owned(),
                    |f| describe_fragment(&f),
                )))
            }
            "clipboard_severed" => Some(IntrospectValue::Text(state.clipboard.get().map_or_else(
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
            "last_insert" => Some(IntrospectValue::Text(state.last_insert.get())),
            "last_move" => Some(IntrospectValue::Text(state.last_move.get())),
            "last_rewire" => Some(IntrospectValue::Text(state.last_rewire.get())),
            "bypassed" => Some(IntrospectValue::Text(join_ids(
                document
                    .tree(tree)
                    .into_iter()
                    .flat_map(pinion_node_graph::Tree::nodes)
                    .filter(|n| n.bypassed)
                    .map(|n| n.id.0),
            ))),
            "muted_links" => Some(IntrospectValue::Text(join_ids(
                document
                    .tree(tree)
                    .map(pinion_node_graph::Tree::links)
                    .unwrap_or_default()
                    .iter()
                    .filter(|l| l.muted)
                    .map(|l| l.id.0),
            ))),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "trees" | "definitions" | "nodes" | "links" | "valid" | "path" | "depth"
            | "current_tree" | "selection" | "last_refusal" | "clipboard" | "clipboard_severed"
            | "clipboard_bytes" | "last_insert" | "last_move" | "bypassed" | "muted_links"
            | "last_rewire" => Err(InterveneError::ReadOnly),
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
            | "passthrough" | "visible_ports" | "looks" => self.read(path, &args),
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
                Ok(IntrospectValue::Text(match &node.body {
                    NodeBody::Kind(op) => op.name(),
                    NodeBody::Group(inner) => format!("group:{}", inner.0),
                    NodeBody::Interface(InterfaceSide::Input) => "interface:input".to_owned(),
                    NodeBody::Interface(InterfaceSide::Output) => "interface:output".to_owned(),
                }))
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
                Ok(IntrospectValue::Text(format!(
                    "routes:{routes}|dropped:{}|unreached:{}|identity:{}",
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

    /// The verbs. Each one is a call into the substrate plus the bookkeeping
    /// this application actually owns: what is selected, and where the user is.
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
            "group" => {
                let name = Self::text(args)?;
                let selection = state.selection.get();
                let made = state
                    .edit(|document| document.group(tree, &selection, name))
                    .map_err(InvokeError::rejected)?;
                state.selection.set(vec![made.node]);
                ok(&format!("{}:{}", made.definition.0, made.node.0))
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
            "copy" | "paste" | "duplicate" => self.clipboard(path, args),
            "group_insert" | "group_separate" | "fork" => self.boundary(path, args),
            _ => Err(InvokeError::UnknownPath),
        }
    }

    /// R1584 — the two directions of a boundary move, and the fork that makes
    /// either of them local.
    ///
    /// The whole of what this application supplies is *where the boundary is*.
    /// Inward it is named by the argument, because the user is looking at the
    /// host tree and pointing at a group. Outward it is the edit path's own last
    /// step — the user is inside the group, so the group they are inside IS the
    /// boundary — which is the same place Blender reads it from
    /// (`snode->edittree` against `ED_node_tree_get(snode, 1)`), and refusing at
    /// the root is its "Not inside node group".
    /// R1586 — the verbs that change how a node or a wire takes part.
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
    /// The two arms are *stated at the call*. Blender's `linked` arm defaults
    /// from a user preference (`U.dupflag & USER_DUP_NTREE`), so whether an edit
    /// to the copy also changes the original depends on a setting the gesture
    /// does not mention.
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
                        "{:?} is not one of swatch/level/mix/fade/output",
                        Self::text(args).unwrap_or_default()
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
/// The second half is the part Blender's `node_internal_relink` removes and
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
            format!("{}:{}{off}", p.name, p.ty.name())
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
