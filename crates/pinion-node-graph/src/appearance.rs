//! R1586 — what a node looks like, kept apart from what it means.
//!
//! The DCC keeps a node's collapsed-ness, its option panel, its preview, its
//! *selection* and its **mute** in one `flag` integer (`NODE_COLLAPSED`, `NODE_OPTIONS`, `NODE_PREVIEW`, `NODE_SELECT`, `NODE_MUTED`).
//! Nothing in that model says which of those bits the evaluator is allowed to
//! read, so the answer lives in whichever code happens to read them.
//!
//! Here the answer is a type. [`Appearance`] is everything about a node that a
//! renderer needs and evaluation must never see; [`Node::bypassed`] is the one
//! fact of this kind that *is* the graph's meaning, and it is a field of its
//! own. Selection is in neither, because a selection belongs to an editor and
//! not to the document — two people looking at one graph have two selections and
//! one document.
//!
//! [`Node::bypassed`]: crate::Node::bypassed

use serde::{Deserialize, Serialize};

use crate::model::{Document, NodeId, NodeKind, Side, TreeId, yes};

/// ★★★★★ R1921 — a colour a person authored, in sRGB.
///
/// Three channels and no alpha, which is a decision and not an omission: what
/// is authored here is *what colour this node is*, and how solidly any one of
/// its faces is painted is a fact about that face — see [`Faces`], where the
/// body tint gets its own translucency derived rather than stored. The DCC
/// authors three channels for the same reason; the engine's node colours are
/// four-channel and its own default implementations then vary only the alpha,
/// which is that derivation written four times.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
pub struct Tint {
    /// Red, sRGB.
    pub r: u8,
    /// Green, sRGB.
    pub g: u8,
    /// Blue, sRGB.
    pub b: u8,
}

impl Tint {
    /// Construct from raw sRGB channels.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// ★ Relative luminance, 0..=255, by the coefficients a person's eye
    /// actually weights the channels with.
    ///
    /// Integer arithmetic on purpose: this decides which of two text colours a
    /// title gets, and a value that answered differently on two machines would
    /// make [`Faces::title_text`] a fact about the renderer rather than about
    /// the colour.
    ///
    /// The narrowing cannot lose anything: the coefficients sum to exactly
    /// 1000, so the largest possible numerator is `255 * 1000` and the quotient
    /// is at most 255. That is an arithmetic fact rather than a hope, and if it
    /// were ever untrue the contrast floor in `dcc_node_copy_color` — which
    /// walks the whole colour cube — is what would go red.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub const fn luminance(self) -> u8 {
        // 0.2126 R + 0.7152 G + 0.0722 B, scaled by 1000 and divided back.
        let sum = (self.r as u32) * 213 + (self.g as u32) * 715 + (self.b as u32) * 72;
        (sum / 1000) as u8
    }

    /// The same colour scaled toward black by `numerator/denominator`.
    ///
    /// Every caller passes a numerator below the denominator, so each channel
    /// only ever shrinks and the narrowing cannot lose anything. `Faces::of`
    /// is the only caller and this is private, which is what makes that a
    /// statement about the code rather than about its callers.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    const fn scaled(self, numerator: u32, denominator: u32) -> Self {
        Self {
            r: ((self.r as u32) * numerator / denominator) as u8,
            g: ((self.g as u32) * numerator / denominator) as u8,
            b: ((self.b as u32) * numerator / denominator) as u8,
        }
    }
}

/// ★★★★★ R1921 — the colours a node's faces are drawn in, all four DERIVED
/// from one authored [`Tint`].
///
/// # Why one authored value and four derived, rather than four authored
///
/// The engine asks a node four separate questions — its title colour, its body
/// tint, its comment colour and its title TEXT colour — and each is a virtual
/// its own subclass may answer independently. So a node there can answer with
/// four colours that do not go together, and nothing in that model can notice:
/// most sharply, the title colour and the title text colour are two unrelated
/// answers, and a subclass that darkens one without the other produces a title
/// nobody can read. The DCC does not have that problem because it does not ask
/// the question: it authors ONE colour per node and its own drawing code
/// derives the rest.
///
/// This takes the DCC's shape and states the consequence the engine leaves to
/// each subclass: [`title_text`](Self::title_text) is chosen by CONTRAST
/// against the title it will be drawn on, so *there is no authored colour for
/// which the title is unreadable*. That is a property a test can hold over
/// every colour, and `appearance::tests` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Faces {
    /// The header band — the authored colour itself.
    pub title: Tint,
    /// The body behind the ports: the same colour taken toward black, so a
    /// node reads as ONE thing with a lighter band rather than as two.
    pub body: Tint,
    /// A frame's fill. Frames are drawn behind what they contain, so this is
    /// taken further down than [`body`](Self::body).
    pub comment: Tint,
    /// ★ The title's LETTERS, chosen for contrast against
    /// [`title`](Self::title) rather than authored.
    pub title_text: Tint,
}

impl Faces {
    /// The faces an authored `tint` gives.
    #[must_use]
    pub const fn of(tint: Tint) -> Self {
        Self {
            title: tint,
            body: tint.scaled(45, 100),
            comment: tint.scaled(30, 100),
            // ★★★★★ The one face that is not a scaling of the authored colour:
            // it is a CHOICE BETWEEN two, made by luminance, and that is what
            // makes "the title is readable" true of every colour rather than of
            // the colours somebody happened to try.
            title_text: if tint.luminance() > 140 {
                Tint::rgb(0, 0, 0)
            } else {
                Tint::rgb(255, 255, 255)
            },
        }
    }
}

/// ★★★★★ R1940 — **what a node's KIND says the node is drawn as**, when
/// nobody has authored a colour for it.
///
/// See [`NodeKind::drawn_as`](crate::NodeKind::drawn_as) for the measurement
/// that shaped this. In short: the reference lets a node type override, per
/// INSTANCE, the class its header is drawn from, and all three of its
/// overriders DERIVE that class from the node's own authored state rather than
/// storing a colour on it.
///
/// Three arms and not an `Option<Tint>`, for R1928's reason — there are three
/// answers a taxonomy genuinely gives:
///
/// * [`Unstated`](Self::Unstated) — this kind says nothing, and the
///   application draws the node however it draws a node. A real answer, not a
///   hole: most kinds in most taxonomies have no opinion, and saying so is what
///   lets a screen show its own default without inferring one.
/// * [`In`](Self::In) — this colour, chosen by the kind.
/// * [`LikeType`](Self::LikeType) — ★ whatever colour THIS TYPE is drawn in.
///
/// # Why the third arm, and why it is the one that matters
///
/// The reference's class is a fixed enumeration, separate from its socket
/// types, and the correspondence between the two is a coincidence its model
/// cannot state: the node that answers "I am a vector operation" and the socket
/// that carries a vector reach two unrelated palettes. Here a kind can say *I
/// am drawn like the type I work on*, and the answer comes from the same
/// [`type_colour`](crate::NodeKind::type_colour) a port of that type is drawn
/// with — so a taxonomy that recolours a type recolours the nodes that work on
/// it, and the two cannot drift apart.
///
/// ⚠ `LikeType` can still resolve to nothing: a type whose colour is `None` is
/// a type nobody coloured, and a node drawn like it is drawn like it. That
/// collapses to the same outcome as `Unstated` and is deliberately NOT the same
/// STATEMENT — one says *this kind has no opinion*, the other *this kind's
/// opinion is that type's, which is itself unstated*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drawn<T> {
    /// This kind says nothing about how its nodes are drawn.
    Unstated,
    /// Drawn in this colour.
    In(Tint),
    /// Drawn in whatever colour this **type** is drawn in.
    LikeType(T),
}

/// A node's view state.
///
/// Held in the document rather than in a side table keyed by [`NodeId`], for the
/// same reason the node's position is: a group collapse, a fragment and a paste
/// all move nodes *between trees*, and an id is only unique within one — so a
/// side table would silently attach one node's looks to another's.
///
/// Four independent booleans and not a state machine, which is what
/// `clippy::struct_excessive_bools` would prefer: each is a *separate* gesture
/// with its own memory, and folding them together would lose the property that
/// makes them usable — un-collapsing a node restores whatever it was already
/// saying about its unused ports, rather than a default. The DCC keeps them as
/// separate bits for the same reason (and then keeps them in the same word as
/// `NODE_MUTED`, which is the part not copied here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct Appearance {
    /// ★★★★★ R1921 — **the colour a person gave this node**, or `None` for
    /// whatever its kind is drawn in.
    ///
    /// `Option` and not a colour beside a flag, and that is the whole decision.
    /// The DCC models this as TWO facts — a three-float `color` and a
    /// `NODE_CUSTOM_COLOR` bit — so *a node carrying a colour it is not using*
    /// is representable there, and its copy-colour operator has to move both
    /// (it does: it sets the bit, copies the channels, and clears the bit when
    /// the source has none). One `Option` makes that state unrepresentable
    /// rather than merely wrong, which is R1891's rule, and it is why copying a
    /// colour here is `dst.tint = src.tint` with nothing to keep in step.
    ///
    /// The faces this is drawn as are derived, never stored — see [`Faces`].
    #[serde(default)]
    pub tint: Option<Tint>,
    /// Drawn small, with only its wired ports showing. The DCC's
    /// `collapse_toggle`.
    #[serde(default)]
    pub collapsed: bool,
    /// Unwired ports are not drawn. The DCC's `hide_socket_toggle`,
    /// whose own description is "Toggle unused node socket display".
    #[serde(default)]
    pub hide_unused_ports: bool,
    /// Whether the node's own controls are shown. What a control *is* belongs
    /// to the application; whether it is on screen travels with the node. The
    /// DCC's `options_toggle`.
    #[serde(default = "yes")]
    pub show_options: bool,
    /// Whether the node's preview is shown. The DCC's
    /// `preview_toggle`.
    #[serde(default)]
    pub show_preview: bool,
    /// An authored width in the application's own units — the units `x` and
    /// `y` are already in — or `None` for whatever width the application gives
    /// a node of this kind. The DCC's `resize`.
    #[serde(default)]
    pub width: Option<u32>,
    /// An authored height, in the same units, or `None` for the height the
    /// application derives (R1595).
    ///
    /// `Option` rather than absent, and that is the whole of what it says: an
    /// ordinary node's height is a **function of how many ports it draws**, so
    /// `None` is the right answer there and authoring one would be a second
    /// statement free to disagree with the first. A [`NodeBody::Frame`] has no
    /// ports, so its height is a fact about the canvas and nothing derives it —
    /// which is exactly the case R1589 recorded as the reason this field was
    /// missing.
    ///
    /// The DCC carries both on every node (`node::width`, `node::height`) with
    /// no such distinction, and its `resize` is horizontal-only for ordinary
    /// nodes by convention rather than by anything in the model.
    ///
    /// [`NodeBody::Frame`]: crate::NodeBody::Frame
    #[serde(default)]
    pub height: Option<u32>,
    /// ★★★★★ R1912 — the input ports **a hand put away**, ascending.
    ///
    /// A second, independent reason a port is not drawn, and the one the
    /// derivation above cannot express: `hide_unused_ports` is a rule over the
    /// wiring, so it can only ever hide what nothing is wired to, and it
    /// re-decides on every read. This is a *declaration about a named port*,
    /// which survives that port becoming wired and survives the rule being
    /// turned off.
    ///
    /// # What forced it, measured at R1912
    ///
    /// Both references model this as a flag on the **socket**, not as a rule on
    /// the node. The DCC's socket carries a user-hidden bit and asks
    /// `!user_hidden && available && inferred` — **three** independent reasons
    /// a socket is not drawn, of which only the first is a person's — while its
    /// bulk "toggle unused sockets" operator merely *sets* that bit over the
    /// unwired ones. The engine hides a named pin, hides every pin but the
    /// named one, and restores them all, on a node whose ports are the fields
    /// of a struct.
    ///
    /// Before this field the crate had the bulk rule and no way to be *told*,
    /// so four census rows across two references — the DCC's socket-hide
    /// toggle and the engine's three — were one absent mechanism.
    ///
    /// Indices into [`Document::signature`](crate::Document::signature)'s
    /// inputs, which is where a variadic run has already been spliced in, so an
    /// index here means the same port the renderer draws and the wire names.
    #[serde(default)]
    pub put_away_inputs: Vec<u32>,
    /// The output ports a hand put away, ascending. See
    /// [`put_away_inputs`](Appearance::put_away_inputs).
    #[serde(default)]
    pub put_away_outputs: Vec<u32>,
    /// ★★★★★ R1914 — the input addresses a hand **split** into one port per
    /// member, ascending.
    ///
    /// A set of [`PortPath`]s and not a list of indices, and the two reasons
    /// are the reasons the whole split model is shaped this way:
    ///
    /// * **it nests** — a member that is itself composite splits again, which
    ///   the reference's own recombine walks recursively, so a flat list would
    ///   have modelled one level of a tree;
    /// * **it is stable** — a resolved index moves whenever a port before it
    ///   splits, so a declaration written in resolved indices would silently
    ///   re-point itself. [`Document::index_of`] converts when a resolved index
    ///   is what a caller needs.
    ///
    /// The invariant, held by [`Document::split_port`] and
    /// [`Document::recombine_port`]: every ancestor of a path here is also
    /// here. A member cannot be split before its parent is, because until then
    /// it is not a port.
    ///
    /// ⚠ Ports are hidden by this and not removed, which is the reference's own
    /// behaviour (`bHidden = true` at the top of its split) and the reason
    /// [`Hidden`] gained a third arm rather than this becoming a fourth
    /// mechanism.
    ///
    /// [`PortPath`]: crate::PortPath
    #[serde(default)]
    pub split_inputs: Vec<crate::PortPath>,
    /// The output addresses a hand split. See
    /// [`split_inputs`](Appearance::split_inputs).
    #[serde(default)]
    pub split_outputs: Vec<crate::PortPath>,
}

impl Default for Appearance {
    /// An ordinary node: full size, every port drawn, controls shown, no
    /// preview, the application's own width, and the colour its kind gives.
    fn default() -> Self {
        Self {
            tint: None,
            collapsed: false,
            hide_unused_ports: false,
            show_options: true,
            show_preview: false,
            width: None,
            height: None,
            put_away_inputs: Vec::new(),
            put_away_outputs: Vec::new(),
            split_inputs: Vec::new(),
            split_outputs: Vec::new(),
        }
    }
}

/// Which of a node's ports a renderer draws.
///
/// Indices into the node's [`Signature`](crate::Signature), ascending. The
/// answer is a *derivation* over the declaration and the wiring together, which
/// is why it belongs here and not in the renderer: `hide_unused_ports` is not
/// answerable without knowing what is wired, and only the document knows that.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VisiblePorts {
    /// Input port indices to draw.
    pub inputs: Vec<u32>,
    /// Output port indices to draw.
    pub outputs: Vec<u32>,
    /// Input indices the appearance hides. Named rather than merely absent, so
    /// an editor can offer "show hidden ports" without recomputing the
    /// complement — and so a port that has vanished from the picture is a fact
    /// with a place to be reported from.
    pub hidden_inputs: Vec<u32>,
    /// Output indices the appearance hides.
    pub hidden_outputs: Vec<u32>,
    /// ★★★★★ R1912 — of [`hidden_inputs`](VisiblePorts::hidden_inputs), the
    /// ones a hand put away by name rather than the node's rule hiding.
    ///
    /// A **subset**, published beside the whole rather than instead of it, so a
    /// renderer that only wants to know what to draw reads one list and a
    /// client offering "bring it back" reads the other. See
    /// [`why_hidden`](VisiblePorts::why_hidden), which is the derived reading
    /// callers should prefer.
    pub put_away_inputs: Vec<u32>,
    /// Of [`hidden_outputs`](VisiblePorts::hidden_outputs), the ones a hand put
    /// away by name.
    pub put_away_outputs: Vec<u32>,
    /// ★★★★★ R1914 — of [`hidden_inputs`](VisiblePorts::hidden_inputs), the
    /// ones hidden because they are **split**: their member ports are drawn in
    /// their place, immediately after them.
    ///
    /// A third reason, published as its own subset for R1912's reason — the
    /// repair differs. A put-away port comes back with
    /// [`Document::restore_ports`]; a split one comes back with
    /// [`Document::recombine_port`], which also takes its member ports away
    /// again and puts their values back together.
    pub split_inputs: Vec<u32>,
    /// Of [`hidden_outputs`](VisiblePorts::hidden_outputs), the ones hidden
    /// because they are split.
    pub split_outputs: Vec<u32>,
}

impl VisiblePorts {
    /// How many ports are hidden on both axes together.
    #[must_use]
    pub fn hidden_count(&self) -> usize {
        self.hidden_inputs.len() + self.hidden_outputs.len()
    }

    /// ★★★★★ R1912 — **why** the port at `index` on `side` is not drawn, or
    /// `None` when it is drawn.
    ///
    /// The question neither reference can be asked. The DCC computes socket
    /// visibility as a conjunction of three independent facts and publishes
    /// only the conjunction, so a caller that finds a socket missing cannot
    /// tell a person's decision from the node kind's; the engine simply removes
    /// the pin. Here the two the crate has are separate arms, because the
    /// repairs differ: one is undone by [`Document::restore_ports`], the other
    /// by wiring the port or by turning the node's rule off.
    #[must_use]
    pub fn why_hidden(&self, side: Side, index: u32) -> Option<Hidden> {
        let (hidden, put_away, split) = match side {
            Side::Input => (
                &self.hidden_inputs,
                &self.put_away_inputs,
                &self.split_inputs,
            ),
            Side::Output => (
                &self.hidden_outputs,
                &self.put_away_outputs,
                &self.split_outputs,
            ),
        };
        if !hidden.contains(&index) {
            return None;
        }
        // ★ R1914 — split is asked FIRST, and the order is a decision rather
        // than a convenience: a split port whose parent a hand had also put
        // away is still, to a person looking at the picture, split — its
        // members are on the frame. Restoring it would put a port back that is
        // already there twice over, so the repair a caller must be told about
        // is the recombine.
        Some(if split.contains(&index) {
            Hidden::Split
        } else if put_away.contains(&index) {
            Hidden::PutAway
        } else {
            Hidden::Unused
        })
    }

    /// ★★★★★ R1912 — whether this node has **no port on the frame at all**.
    ///
    /// Published rather than refused, and the difference is a measurement: the
    /// DCC's bulk socket-hide operator hides *every* unwired socket, so on a
    /// node nothing is wired to it reaches exactly this state and the reference
    /// permits it. A first draft of this crate's verb refused it; refusing what
    /// a reference does is not superiority, it is a divergence.
    ///
    /// What superiority looks like instead is that the state is **sayable**: a
    /// host can warn, offer "bring them back", or draw the node's edge
    /// differently, none of which either reference can do because neither
    /// publishes the fact. The ports are still named in
    /// [`hidden_inputs`](VisiblePorts::hidden_inputs) and
    /// [`hidden_outputs`](VisiblePorts::hidden_outputs), so nothing is lost —
    /// only the wiring handles are off the frame, and
    /// [`Document::restore_ports`] brings them back.
    #[must_use]
    pub fn nothing_drawn(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty() && self.hidden_count() > 0
    }
}

/// ★★★★★ R1912 — why a port is not drawn.
///
/// Two arms because the crate has two independent reasons, and an editor's
/// repair is different for each: a port a hand put away comes back with
/// [`Document::restore_ports`], while a port the node's own rule hid comes back
/// by being wired or by the rule being turned off. A single "hidden" boolean
/// would send a reader to the wrong one half the time.
///
/// ⚠ The DCC has a **third** — a socket its node kind declares does not apply
/// at all, which no gesture restores. This crate has no such declaration, so
/// there is no arm for it: an arm nothing can produce is an arm a reader would
/// have to be told to ignore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Hidden {
    /// A hand put this port away, by name. Restored by
    /// [`Document::restore_ports`].
    PutAway,
    /// The node asks for unwired ports to be hidden and nothing is wired to
    /// this one. Restored by wiring it, or by turning the node's rule off.
    Unused,
    /// ★★★★★ R1914 — this port is **split**: one port per member of its type
    /// is drawn immediately after it, carrying its share of its value.
    /// Restored by [`Document::recombine_port`].
    ///
    /// The reference sets exactly this state (`bHidden = true` on the parent at
    /// the top of its split) and has no way to report it: its pin is hidden,
    /// and *why* is recoverable only by noticing the pin has sub-pins.
    Split,
}

impl Hidden {
    /// The word this reason is published under, for a client reading the wire.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::PutAway => "put_away",
            Self::Unused => "unused",
            Self::Split => "split",
        }
    }
}

/// ★★★★★ R1912 — which ports a [`Document::put_away_ports`] call is about.
///
/// One verb with a named scope rather than three verbs, and the scope names are
/// the references' own: the engine offers *remove this pin* and *remove all
/// other pins* as two commands over one node, and the DCC's bulk operator is
/// *hide every unwired socket*. Three commands, one question — **which ports** —
/// so it is one parameter.
///
/// ⚠ [`Unwired`](Self::Unwired) is here because it is what the DCC's operator
/// does, and it is NOT the same as the node's `hide_unused_ports` rule even
/// though it selects the same ports today: this puts them away *by name*, so
/// they stay away when one of them is later wired. The rule re-decides; this
/// remembers. That difference is the whole reason a declaration was needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutAway {
    /// Exactly the port at this index on this side.
    Port(Side, u32),
    /// Every port on both sides except the one named — the engine's *remove all
    /// other pins*.
    AllOthers(Side, u32),
    /// Every port nothing is wired to, on both sides — the DCC's bulk operator.
    Unwired,
}

/// ★★★★★ R1912 — why a request to put a port away was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PutAwayRefusal {
    /// No such node in that tree.
    NoSuchNode {
        /// The tree asked about.
        tree: TreeId,
        /// The node asked about.
        node: NodeId,
    },
    /// The node has no port at that index on that side.
    NoSuchPort {
        /// The side asked about.
        side: Side,
        /// The index asked about.
        index: u32,
        /// How many ports that side actually has.
        of: u32,
    },
    /// ★★★★★ This node's ports **are** the node, so putting one away would
    /// leave nothing to grab.
    ///
    /// The DCC's own refusal, and it is written in its source as a special case
    /// on one node type — *the reroute node is the socket itself, do not hide
    /// this*. Here it is a declaration on the kind rather than a name test, so
    /// a second such kind is covered the day it is written rather than the day
    /// somebody remembers this branch.
    PortsAreTheNode {
        /// What the node is called, so a refusal a person reads names it.
        kind: String,
    },
}

impl core::fmt::Display for PutAwayRefusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoSuchNode { tree, node } => {
                write!(f, "no node {node:?} in tree {tree:?}")
            }
            Self::NoSuchPort { side, index, of } => {
                write!(f, "no {side:?} port at {index}; this node has {of} of them")
            }
            Self::PortsAreTheNode { kind } => write!(
                f,
                "`{kind}`'s ports are the node itself, so putting one away \
                 would leave nothing to grab"
            ),
        }
    }
}

impl std::error::Error for PutAwayRefusal {}

impl<K: NodeKind> Document<K> {
    /// Which of `node`'s ports are drawn, and which its appearance hides.
    ///
    /// A port is hidden when a hand **put it away** by name
    /// ([`Appearance::put_away_inputs`], R1912), or when the node asks for
    /// unwired ports to be hidden — by [`Appearance::hide_unused_ports`], or by
    /// being [`collapsed`](Appearance::collapsed), which is the same request
    /// with the node drawn small as well — **and** nothing is wired to it. A
    /// muted link still counts as wired: mutedness is about the value, and the
    /// wire is still on screen.
    ///
    /// ★★★★★ The two reasons are **independent**, and that is R1912's finding:
    /// a put-away port stays away when it is later wired, which the rule alone
    /// could never express because the rule re-decides on every read. Which of
    /// the two hid a given port is [`VisiblePorts::why_hidden`].
    ///
    /// `None` when the node is not there.
    #[must_use]
    pub fn visible_ports(&self, tree: TreeId, node: NodeId) -> Option<VisiblePorts> {
        let host = self.tree(tree)?;
        let appearance = &host.node(node)?.appearance;
        let signature = self.signature(tree, node)?;
        let hide = appearance.collapsed || appearance.hide_unused_ports;

        // ★ R1914 — the resolved indices of the ports a split hid, derived from
        // the same expansion the signature above was spliced by. Asking the
        // splice rather than re-deriving from the declaration is what keeps a
        // hidden index and a drawn index from being two different answers.
        let split_in = self.split_parents(tree, node, Side::Input);
        let split_out = self.split_parents(tree, node, Side::Output);

        let mut visible = VisiblePorts::default();
        for index in 0..signature.inputs.len() {
            let port = u32::try_from(index).unwrap_or(u32::MAX);
            let wired = host.link_into(crate::Socket::new(node, port)).is_some();
            let put_away = appearance.put_away_inputs.contains(&port);
            let split = split_in.contains(&port);
            if put_away {
                visible.put_away_inputs.push(port);
            }
            if split {
                visible.split_inputs.push(port);
            }
            if split || put_away || (hide && !wired) {
                visible.hidden_inputs.push(port);
            } else {
                visible.inputs.push(port);
            }
        }
        for index in 0..signature.outputs.len() {
            let port = u32::try_from(index).unwrap_or(u32::MAX);
            let wired = host
                .links()
                .iter()
                .any(|l| l.from == crate::Socket::new(node, port));
            let put_away = appearance.put_away_outputs.contains(&port);
            let split = split_out.contains(&port);
            if put_away {
                visible.put_away_outputs.push(port);
            }
            if split {
                visible.split_outputs.push(port);
            }
            if split || put_away || (hide && !wired) {
                visible.hidden_outputs.push(port);
            } else {
                visible.outputs.push(port);
            }
        }
        Some(visible)
    }

    /// ★★★★★ R1912 — **put a node's ports away by name**, which is the gesture
    /// four census rows across two references were one absent mechanism for.
    ///
    /// The DCC's socket-hide toggle and the engine's *remove this pin*, *remove
    /// all other pins* — one verb, because those are three answers to one
    /// question (*which ports*), which is what [`PutAway`] is.
    ///
    /// Returns the ports this call newly put away, in order, so a caller can
    /// undo exactly what happened rather than restoring everything: a port
    /// already away is not reported, because reporting it would make an undo
    /// that reads this list show a port the person had already hidden.
    ///
    /// # Superior to both references, and this is the reason it is a
    /// declaration
    ///
    /// The DCC's operator *sets* its socket flags from the wiring, so it
    /// silently rewrites what a person chose the last time they used it; and
    /// once set, nothing says whether a socket is away because a person said so
    /// or because the node's kind does not offer it. Here the request is
    /// remembered by name, the derived rule is a separate reason, and
    /// [`VisiblePorts::why_hidden`] tells them apart.
    ///
    /// # Errors
    ///
    /// [`PutAwayRefusal`] — an absent node or port, or a kind whose ports
    /// **are** the node ([`NodeKind::ports_are_the_node`]).
    ///
    /// ⚠ Putting away *every* port is NOT refused, and that is measured rather
    /// than permitted by omission: the DCC's own bulk operator reaches exactly
    /// that state on a node nothing is wired to. It is published instead —
    /// [`VisiblePorts::nothing_drawn`].
    pub fn put_away_ports(
        &mut self,
        tree: TreeId,
        node: NodeId,
        which: PutAway,
    ) -> Result<Vec<(Side, u32)>, PutAwayRefusal> {
        let signature = self
            .signature(tree, node)
            .ok_or(PutAwayRefusal::NoSuchNode { tree, node })?;
        let counts = |side: Side| -> u32 {
            let n = match side {
                Side::Input => signature.inputs.len(),
                Side::Output => signature.outputs.len(),
            };
            u32::try_from(n).unwrap_or(u32::MAX)
        };
        // ★ The kind's own refusal first, because it is about the node rather
        // than about the request: a kind that cannot put ports away answers the
        // same way whichever port was named, and answering `NoSuchPort` first
        // would send a caller to check an index that was never the problem.
        if let Some(kind) = self.kind_that_is_its_ports(tree, node) {
            return Err(PutAwayRefusal::PortsAreTheNode { kind });
        }
        if let PutAway::Port(side, index) | PutAway::AllOthers(side, index) = which {
            let of = counts(side);
            if index >= of {
                return Err(PutAwayRefusal::NoSuchPort { side, index, of });
            }
        }

        let wanted = self.ports_selected_by(tree, node, which, &counts);
        let host = self
            .tree_mut(tree)
            .ok_or(PutAwayRefusal::NoSuchNode { tree, node })?;
        let target = host
            .node_mut(node)
            .ok_or(PutAwayRefusal::NoSuchNode { tree, node })?;

        let mut done = Vec::new();
        for (side, index) in wanted {
            let into = match side {
                Side::Input => &mut target.appearance.put_away_inputs,
                Side::Output => &mut target.appearance.put_away_outputs,
            };
            if !into.contains(&index) {
                into.push(index);
                into.sort_unstable();
                done.push((side, index));
            }
        }
        Ok(done)
    }

    /// ★★★★★ R1912 — **bring every put-away port back**, and say how many came
    /// back.
    ///
    /// The engine's *restore all structure pins*, whose own command is gated on
    /// "not all pins are shown" — the same fact this returns, so a host can
    /// grey the control out rather than offering one that does nothing.
    ///
    /// It restores only what a hand put away. A port the node's own rule hides
    /// is not this verb's business, and clearing it here would be a second
    /// spelling of turning that rule off.
    ///
    /// `None` when the node is not there — an absent node is not a node with
    /// nothing to restore.
    pub fn restore_ports(&mut self, tree: TreeId, node: NodeId) -> Option<usize> {
        let host = self.tree_mut(tree)?;
        let target = host.node_mut(node)?;
        let count =
            target.appearance.put_away_inputs.len() + target.appearance.put_away_outputs.len();
        target.appearance.put_away_inputs.clear();
        target.appearance.put_away_outputs.clear();
        Some(count)
    }

    /// The name of `node`'s kind when that kind's ports **are** the node, else
    /// `None`.
    fn kind_that_is_its_ports(&self, tree: TreeId, node: NodeId) -> Option<String> {
        let target = self.tree(tree)?.node(node)?;
        match &target.body {
            crate::NodeBody::Kind(kind) if kind.ports_are_the_node() => Some(kind.name()),
            _ => None,
        }
    }

    /// The ports a [`PutAway`] scope names, in side-then-index order.
    ///
    /// Derived from the signature and the wiring rather than spelled at each
    /// call site, so the three scopes cannot drift into three rules.
    fn ports_selected_by(
        &self,
        tree: TreeId,
        node: NodeId,
        which: PutAway,
        counts: &impl Fn(Side) -> u32,
    ) -> Vec<(Side, u32)> {
        let mut wanted = Vec::new();
        match which {
            PutAway::Port(side, index) => wanted.push((side, index)),
            PutAway::AllOthers(side, index) => {
                for other in Side::ALL {
                    for port in 0..counts(other) {
                        if !(other == side && port == index) {
                            wanted.push((other, port));
                        }
                    }
                }
            }
            PutAway::Unwired => {
                let Some(host) = self.tree(tree) else {
                    return wanted;
                };
                for port in 0..counts(Side::Input) {
                    if host.link_into(crate::Socket::new(node, port)).is_none() {
                        wanted.push((Side::Input, port));
                    }
                }
                for port in 0..counts(Side::Output) {
                    let from = crate::Socket::new(node, port);
                    if !host.links().iter().any(|l| l.from == from) {
                        wanted.push((Side::Output, port));
                    }
                }
            }
        }
        wanted
    }
}
