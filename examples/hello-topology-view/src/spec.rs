//! ★★★★★ R1947 — **the reference's topology section, written down as a value.**
//!
//! The sibling of `hello-log-view::spec` and `hello-key-patterns::spec`, and
//! written for the same reason: a round can *claim* to have reproduced a screen
//! or it can put the screen in a table a machine reads and let a test compare
//! the painted scene against it in **both** directions, so an element the screen
//! is missing and an element the screen invented are both failures.
//!
//! # What is here and what is in the pin
//!
//! `docs/analyzer-topology-spec.json` holds the *surfaces* — which parts each
//! of the three panes has, in the reference's order, with the words a reader
//! reads. It is a separate artifact, reviewed as a claim about the reference
//! rather than as code, for the reason its own comment gives: a specification
//! written in the same file, in the same edit, by the same hand as the thing it
//! judges is a gate asking the subject for the answer.
//!
//! This module holds what the pin deliberately does not: the geometry the
//! screen lays out in, and the **population** — six peers around one router.
//! A specification that enumerated the population would be a copy of the data,
//! so the pin names `canvas` as one surface and this table says what goes in
//! it, with `painted.rs` asserting every declared node reaches the plot.
//!
//! # Vocabulary
//!
//! Neutral throughout, and not neutral *ad hoc*: the key patterns are the ones
//! `hello-key-patterns` already declares (`sensors/**`, `mesh/**`, `store/**`)
//! and the router is `R-01` as that section's rows already name it. Two
//! sections of one application that spelled one peer two ways would be two
//! captures wearing one name.

use pinion_core::conformance::SpecDocument;
use pinion_core::widgets::severity::SeverityScale;

// ── The window ──────────────────────────────────────────────────────────────

/// The width the section is specified at, and the width its standalone window
/// opens with. Matches the pin's `$at`, which is what the conformance sweep
/// measures at.
pub const WIN_W: u32 = 1220;
/// The height the section is specified at.
pub const WIN_H: u32 = 760;

/// The left column's width — the filter rail.
pub const FILTER_W: u32 = 238;
/// The right column's width — the inspector.
pub const INSPECTOR_W: u32 = 308;
/// The graph column's header height.
pub const HEADER_H: u32 = 46;

/// The padding inside a pane.
pub const PAD: u32 = 16;
/// The gap between two rows of one group.
pub const GAP: u32 = 13;

/// A group heading's height in the filter rail.
pub const GROUP_HEAD_H: u32 = 22;
/// A control row's height.
pub const ROW_H: u32 = 32;

// ── Type ────────────────────────────────────────────────────────────────────

/// The smallest text on this screen — a caption under a control.
pub const FONT_TINY: u32 = 10;
/// A group heading, and the monospace runs in the inspector's tiles.
pub const FONT_SMALL: u32 = 11;
/// Body text: a control's label, a node's subtitle.
pub const FONT_BODY: u32 = 13;
/// A pane's own title.
pub const FONT_TITLE: u32 = 14;
/// The inspector's headline — the picked node's identifier.
pub const FONT_HEADLINE: u32 = 22;

// ── The graph's plot space ──────────────────────────────────────────────────

/// ★★★★★ R1947 — **a node's place is stated in PER MILLE of the plot, not in
/// pixels.**
///
/// The reference lays its graph out in a 900x560 coordinate space and fits that
/// box into whatever the middle column is, so its numbers are meaningless
/// anywhere but at its own aspect ratio. Normalising once, here, means the
/// placement survives a resize, a zoom and a different window without anyone
/// re-deriving it — and it keeps the arithmetic in integers, which is what the
/// rest of this tree's geometry is in.
///
/// Per mille rather than per cent because the reference's positions do not land
/// on whole percents: `268/900` is 29.8%, and rounding it to 30% moves a node
/// two pixels at this width and more at a wider one.
pub const PLOT_SPAN: u32 = 1000;

/// A node's radius, in per mille of the plot's shorter side.
pub const NODE_R: u32 = 57;

/// How much of the plot a node's selection ring adds to its radius.
pub const RING_R: u32 = 71;

/// The zoom the section opens at, and the one `Fit` returns to, as a percent.
pub const ZOOM_FIT: u32 = 100;
/// The closest the plot may be zoomed, as a percent.
pub const ZOOM_MAX: u32 = 200;
/// The furthest out the plot may be zoomed, as a percent.
pub const ZOOM_MIN: u32 = 50;
/// What one press of a zoom control changes the zoom by.
pub const ZOOM_STEP: u32 = 25;

// ── The population ──────────────────────────────────────────────────────────

/// How a node is doing, in this tree's own severity vocabulary.
///
/// ★ The reference has four states and this has four, but they do not line up
/// one for one and the difference is deliberate: its `ok` and `info` both read
/// *Active* on screen, so a reader cannot tell them apart and the distinction
/// exists only in the stroke colour. Here `Active` and `Serving` are different
/// WORDS, because a state a reader cannot name is a state they cannot filter
/// by — and the graded vocabulary the rest of this application uses
/// ([`SEVERITY`]) is what a severity has to be a word of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Carrying traffic.
    Active,
    /// Answering queries rather than streaming.
    Serving,
    /// Trying to come back.
    Reconnecting,
    /// Not reachable.
    Down,
}

impl Standing {
    /// What a reader calls it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Serving => "Serving",
            Self::Reconnecting => "Reconnecting",
            Self::Down => "Down",
        }
    }

    /// How bad it is — a word of [`SEVERITY`], which
    /// this crate's own tests check rather than assume.
    #[must_use]
    pub const fn severity(self) -> &'static str {
        match self {
            Self::Active | Self::Serving => "info",
            Self::Reconnecting => "warn",
            Self::Down => "error",
        }
    }
}

/// The graded words this section's standings are drawn from — the same scale
/// the shell's alarm feed grades itself by.
pub const SEVERITY: SeverityScale = SeverityScale::new(&["info", "warn", "error"]);

/// One node of the observed graph.
pub struct NodeSpec {
    /// What the capture calls it, and the suffix of its paint tag.
    pub id: &'static str,
    /// What it does.
    pub role: &'static str,
    /// The short word drawn under its circle in the plot.
    pub short: &'static str,
    /// How it is doing.
    pub standing: Standing,
    /// Its session identifier.
    pub zid: &'static str,
    /// How many links it holds.
    pub links: u32,
    /// What it is moving.
    pub rate: &'static str,
    /// How the link is secured.
    pub encryption: &'static str,
    /// The key patterns observed on it. Empty is a legitimate state — a node
    /// that is down declares nothing — and the inspector says so in words
    /// rather than drawing an empty box.
    pub keys: &'static [&'static str],
    /// Where it sits when the layout is [`Layout::Force`], in per mille of the
    /// plot.
    pub force: (u32, u32),
    /// Where it sits when the layout is [`Layout::Hierarchical`].
    pub hierarchical: (u32, u32),
}

/// ★ The router is the first entry rather than a field apart.
///
/// The reference draws it as a special case — a rounded rectangle rather than a
/// circle, with its own click handler and its own selection ring, written out
/// beside the loop that draws every other node. Measured: that is 14 lines of
/// markup that differ from the loop's in the SHAPE and in nothing else. Here it
/// is a node like any other with [`NodeSpec::is_router`] deciding the shape, so
/// the population is one list, the inspector needs no second path, and a
/// capture that observed two routers would draw both.
pub const NODES: &[NodeSpec] = &[
    NodeSpec {
        id: "R-01",
        role: "Router",
        short: "ROUTER",
        standing: Standing::Active,
        zid: "router-01",
        links: 5,
        rate: "6.4k msg/s",
        encryption: "TLS 1.3",
        keys: &["@/router/**", "@/*/session/**"],
        force: (500, 500),
        hierarchical: (500, 232),
    },
    NodeSpec {
        id: "P-01",
        role: "Publisher",
        short: "PUB",
        standing: Standing::Active,
        zid: "a3f1c8d2",
        links: 1,
        rate: "3.1k msg/s",
        encryption: "TLS 1.3",
        keys: &["sensors/unit-1/pose", "sensors/unit-1/vel"],
        force: (500, 143),
        hierarchical: (144, 625),
    },
    NodeSpec {
        id: "P-02",
        role: "Subscriber",
        short: "SUB",
        standing: Standing::Active,
        zid: "7c02ff10",
        links: 1,
        rate: "864 msg/s",
        encryption: "TLS 1.3",
        keys: &["sensors/unit-*/pose"],
        force: (702, 321),
        hierarchical: (287, 625),
    },
    NodeSpec {
        id: "P-03",
        role: "Reconnecting peer",
        short: "RECONN",
        standing: Standing::Reconnecting,
        zid: "b5519a3e",
        links: 1,
        rate: "-",
        encryption: "TLS 1.3",
        keys: &["sensors/telemetry/**"],
        force: (702, 679),
        hierarchical: (429, 625),
    },
    NodeSpec {
        id: "P-04",
        role: "Queryable",
        short: "QUERY",
        standing: Standing::Serving,
        zid: "d9b8e720",
        links: 1,
        rate: "212 q/s",
        encryption: "none (tcp)",
        keys: &["store/**"],
        force: (500, 857),
        hierarchical: (571, 625),
    },
    NodeSpec {
        id: "P-05",
        role: "Peer",
        short: "PEER",
        standing: Standing::Active,
        zid: "5e10c4aa",
        links: 2,
        rate: "1.4k msg/s",
        encryption: "QUIC",
        keys: &["mesh/telemetry", "mesh/ctrl"],
        force: (298, 679),
        hierarchical: (713, 625),
    },
    NodeSpec {
        id: "P-06",
        role: "Unreachable peer",
        short: "DOWN",
        standing: Standing::Down,
        zid: "0f22ab91",
        links: 0,
        rate: "offline",
        encryption: "-",
        keys: &[],
        force: (298, 321),
        hierarchical: (856, 625),
    },
];

impl NodeSpec {
    /// Whether this node routes for others, which is what decides its shape in
    /// the plot. Derived from the role rather than stored, so a second router
    /// cannot be drawn as a peer by anyone forgetting a flag.
    #[must_use]
    pub fn is_router(&self) -> bool {
        self.role == "Router"
    }

    /// Where this node sits under `layout`, in per mille of the plot.
    #[must_use]
    pub const fn at(&self, layout: Layout) -> (u32, u32) {
        match layout {
            Layout::Force => self.force,
            Layout::Hierarchical => self.hierarchical,
        }
    }
}

/// What kind of link the capture observed between two nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// A node's traffic to the router.
    Data,
    /// The same, at a lower rate.
    Slow,
    /// A link that is degrading.
    Strained,
    /// A link between two peers rather than through the router.
    Mesh,
    /// A link the capture last saw fail.
    Down,
}

impl LinkKind {
    /// The graded word this kind carries, or `None` where a link is simply
    /// carrying traffic. A `None` here is not "unclassified" — it is the
    /// statement that a healthy link has no severity, which is different from
    /// a link whose severity nobody wrote down.
    #[must_use]
    pub const fn severity(self) -> Option<&'static str> {
        match self {
            Self::Data | Self::Slow | Self::Mesh => None,
            Self::Strained => Some("warn"),
            Self::Down => Some("error"),
        }
    }

    /// Whether this kind is drawn only while the peer-mesh toggle is on.
    #[must_use]
    pub const fn is_mesh(self) -> bool {
        matches!(self, Self::Mesh)
    }

    /// Whether this kind is drawn only while the down-links toggle is on.
    #[must_use]
    pub const fn is_down(self) -> bool {
        matches!(self, Self::Down)
    }
}

/// One observed link.
pub struct LinkSpec {
    /// The node the link is drawn from.
    pub from: &'static str,
    /// The node the link is drawn to.
    pub to: &'static str,
    /// What kind of link it is.
    pub kind: LinkKind,
    /// The label drawn beside it, where the reference labels one.
    pub label: Option<&'static str>,
}

/// The links the capture observed — eight, as the filter rail's heading states.
pub const LINKS: &[LinkSpec] = &[
    LinkSpec {
        from: "P-01",
        to: "R-01",
        kind: LinkKind::Data,
        label: Some("3.1k"),
    },
    LinkSpec {
        from: "P-02",
        to: "R-01",
        kind: LinkKind::Slow,
        label: Some("864"),
    },
    LinkSpec {
        from: "P-03",
        to: "R-01",
        kind: LinkKind::Strained,
        label: None,
    },
    LinkSpec {
        from: "P-04",
        to: "R-01",
        kind: LinkKind::Slow,
        label: Some("212"),
    },
    LinkSpec {
        from: "P-05",
        to: "R-01",
        kind: LinkKind::Data,
        label: None,
    },
    LinkSpec {
        from: "P-05",
        to: "P-02",
        kind: LinkKind::Mesh,
        label: None,
    },
    LinkSpec {
        from: "P-05",
        to: "P-01",
        kind: LinkKind::Mesh,
        label: None,
    },
    LinkSpec {
        from: "P-06",
        to: "R-01",
        kind: LinkKind::Down,
        label: None,
    },
];

// ── The filter rail's controls ──────────────────────────────────────────────

/// Which way the plot arranges its nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Around the router, as the capture's link structure suggests.
    Force,
    /// The router over a row of peers.
    Hierarchical,
}

impl Layout {
    /// What a reader calls it, and what the segmented control's button says.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Force => "Force",
            Self::Hierarchical => "Hierarchical",
        }
    }

    /// The word the graph header uses when it states which layout is in force.
    #[must_use]
    pub const fn in_force(self) -> &'static str {
        match self {
            Self::Force => "force",
            Self::Hierarchical => "hierarchical",
        }
    }
}

/// The two layouts, in the order the segmented control draws them.
pub const LAYOUTS: [Layout; 2] = [Layout::Force, Layout::Hierarchical];

/// One switch in the filter rail.
pub struct ToggleSpec {
    /// The suffix of its paint tag.
    pub key: &'static str,
    /// What a reader calls it.
    pub title: &'static str,
    /// Which group of the rail it sits in.
    pub group: &'static str,
    /// Whether the section opens with it on.
    pub opens_on: bool,
}

/// The rail's switches, in the order it draws them.
pub const TOGGLES: &[ToggleSpec] = &[
    ToggleSpec {
        key: "mesh",
        title: "Peer mesh",
        group: "links",
        opens_on: true,
    },
    ToggleSpec {
        key: "down",
        title: "Down links",
        group: "links",
        opens_on: true,
    },
    ToggleSpec {
        key: "live",
        title: "Live capture",
        group: "streaming",
        opens_on: true,
    },
];

/// The key pattern the rail's highlight control opens on.
pub const HIGHLIGHT: &str = "sensors/**";

/// The patterns the highlight control offers beside it, as chips.
pub const HIGHLIGHT_CHIPS: &[&str] = &["mesh/**", "store/**"];

// ── The inspector's actions ─────────────────────────────────────────────────

/// One action the inspector offers on the picked node.
pub struct ActionSpec {
    /// The suffix of its paint tag.
    pub key: &'static str,
    /// What the button says.
    pub title: &'static str,
    /// The requirement that books it, which is why it refuses.
    ///
    /// ★ Not an `Option`. Both of this section's actions lead somewhere nothing
    /// has built, and a field that could be `None` would be a place for a third
    /// action to arrive with no statement at all — the escape hatch this tree
    /// keeps refusing. An action that works is a different type: it is a
    /// handler, not a row in this table.
    pub reserved_for: &'static str,
}

/// The inspector's two actions, both drawn and both refused.
pub const ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        key: "isolate",
        title: "Isolate",
        reserved_for: "requirement 13",
    },
    ActionSpec {
        key: "detach",
        title: "Detach",
        reserved_for: "requirement 23",
    },
];

// ── The pin ─────────────────────────────────────────────────────────────────

/// The written specification, compiled in.
///
/// `include_str!` rather than a read at run time so the gate cannot pass by
/// finding no file: a specification that goes missing must break the build, not
/// silently stop judging.
const TOPOLOGY_SPEC_JSON: &str = include_str!("../../../docs/analyzer-topology-spec.json");

/// This section's three surfaces, as `docs/analyzer-topology-spec.json` states
/// them.
///
/// # Panics
///
/// If the document is not a specification — unreadable JSON, a surface with no
/// canon, a remainder entry naming no round or no reason. All of them are
/// defects in the pin, and all of them must stop the build rather than weaken
/// the gate.
#[must_use]
pub fn document() -> SpecDocument {
    SpecDocument::pinned(TOPOLOGY_SPEC_JSON, "docs/analyzer-topology-spec.json")
}

/// One part of a pane, as this build draws it.
pub struct PartSpec {
    /// The suffix of the part's paint tag.
    pub key: &'static str,
    /// What a reader calls it — held against the pin's own title.
    pub title: &'static str,
}

/// The filter rail's parts, in the order they are drawn.
pub const FILTERS: &[PartSpec] = &[
    PartSpec {
        key: "title",
        title: "Filters and layers",
    },
    PartSpec {
        key: "observed",
        title: "Observed population",
    },
    PartSpec {
        key: "layout",
        title: "Layout",
    },
    PartSpec {
        key: "links",
        title: "Show links",
    },
    PartSpec {
        key: "highlight",
        title: "Highlight key pattern",
    },
    PartSpec {
        key: "streaming",
        title: "Streaming",
    },
];

/// The graph pane's parts.
pub const GRAPH: &[PartSpec] = &[
    PartSpec {
        key: "title",
        title: "Network topology",
    },
    PartSpec {
        key: "layout_label",
        title: "Layout in force",
    },
    PartSpec {
        key: "live",
        title: "Capture state",
    },
    PartSpec {
        key: "fit",
        title: "Fit",
    },
    PartSpec {
        key: "canvas",
        title: "The node-link plot",
    },
    PartSpec {
        key: "zoom_in",
        title: "Zoom in",
    },
    PartSpec {
        key: "zoom_out",
        title: "Zoom out",
    },
    PartSpec {
        key: "hint",
        title: "Selection hint",
    },
];

/// The inspector's parts.
pub const INSPECTOR: &[PartSpec] = &[
    PartSpec {
        key: "title",
        title: "Inspector",
    },
    PartSpec {
        key: "badge",
        title: "Selected node",
    },
    PartSpec {
        key: "id",
        title: "Node identifier",
    },
    PartSpec {
        key: "status",
        title: "Status",
    },
    PartSpec {
        key: "role",
        title: "Role",
    },
    PartSpec {
        key: "zid",
        title: "Session identifier",
    },
    PartSpec {
        key: "links",
        title: "Links",
    },
    PartSpec {
        key: "rate",
        title: "Message rate",
    },
    PartSpec {
        key: "encryption",
        title: "Encryption",
    },
    PartSpec {
        key: "state",
        title: "Status tile",
    },
    PartSpec {
        key: "keys",
        title: "Key patterns",
    },
    PartSpec {
        key: "isolate",
        title: "Isolate",
    },
    PartSpec {
        key: "detach",
        title: "Detach",
    },
];

/// The node this section opens with picked.
///
/// The router, because it is the one node every other one is linked to, so the
/// inspector opens saying something about the graph as a whole rather than
/// about whichever peer happened to be first in the table.
pub const OPENS_ON: &str = "R-01";

/// The node this table names, or `None`.
#[must_use]
pub fn node(id: &str) -> Option<&'static NodeSpec> {
    NODES.iter().find(|n| n.id == id)
}
