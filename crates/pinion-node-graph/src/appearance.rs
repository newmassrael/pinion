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
}

impl Default for Appearance {
    /// An ordinary node: full size, every port drawn, controls shown, no
    /// preview, the application's own width.
    fn default() -> Self {
        Self {
            collapsed: false,
            hide_unused_ports: false,
            show_options: true,
            show_preview: false,
            width: None,
            height: None,
            put_away_inputs: Vec::new(),
            put_away_outputs: Vec::new(),
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
        let (hidden, put_away) = match side {
            Side::Input => (&self.hidden_inputs, &self.put_away_inputs),
            Side::Output => (&self.hidden_outputs, &self.put_away_outputs),
        };
        if !hidden.contains(&index) {
            return None;
        }
        Some(if put_away.contains(&index) {
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
}

impl Hidden {
    /// The word this reason is published under, for a client reading the wire.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::PutAway => "put_away",
            Self::Unused => "unused",
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

        let mut visible = VisiblePorts::default();
        for index in 0..signature.inputs.len() {
            let port = u32::try_from(index).unwrap_or(u32::MAX);
            let wired = host.link_into(crate::Socket::new(node, port)).is_some();
            let put_away = appearance.put_away_inputs.contains(&port);
            if put_away {
                visible.put_away_inputs.push(port);
            }
            if put_away || (hide && !wired) {
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
            if put_away {
                visible.put_away_outputs.push(port);
            }
            if put_away || (hide && !wired) {
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
