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

use crate::model::{Document, NodeId, NodeKind, TreeId, yes};

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
}

impl VisiblePorts {
    /// How many ports are hidden on both axes together.
    #[must_use]
    pub fn hidden_count(&self) -> usize {
        self.hidden_inputs.len() + self.hidden_outputs.len()
    }
}

impl<K: NodeKind> Document<K> {
    /// Which of `node`'s ports are drawn, and which its appearance hides.
    ///
    /// A port is hidden when the node asks for unwired ports to be hidden — by
    /// [`Appearance::hide_unused_ports`], or by being
    /// [`collapsed`](Appearance::collapsed), which is the same request with the
    /// node drawn small as well — **and** nothing is wired to it. A muted link
    /// still counts as wired: mutedness is about the value, and the wire is
    /// still on screen.
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
            if hide && !wired {
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
            if hide && !wired {
                visible.hidden_outputs.push(port);
            } else {
                visible.outputs.push(port);
            }
        }
        Some(visible)
    }
}
