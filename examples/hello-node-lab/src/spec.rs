//! R1651 — **the reference screen, written down as a value.**
//!
//! The standing instruction for this axis is to reproduce the reference tool's
//! screen A exactly. A round can claim that and be believed, or it can put the
//! screen in a table a machine reads and let a test compare the painted scene
//! against it — in *both* directions, so an element the screen is missing and
//! an element the screen invented are both failures.
//!
//! That is what this module is. Nothing here is paint: every constant is a fact
//! the reference states about screen A (which panes exist and how wide they
//! are, which roles the palette groups, which nodes the opening graph holds and
//! where, which fields the inspector shows and with what applies-scope), and
//! `main.rs` is written *against* it rather than beside it. A drift between the
//! two is therefore a test failure and not a matter of anybody's memory.
//!
//! The whole table is published on the wire as `spec`, so
//! `tools/demos/r1651_the_node_lab_matches_the_reference.py` reads it from the
//! running application rather than carrying a second copy — the second-copy
//! failure R1649's sweep exists to prevent, one level up.
//!
//! **Vocabulary is neutral by construction.** The reference's own node names,
//! configuration paths and protocol words are replaced with the role words the
//! tool class uses generally; the structure and the behaviour are what is being
//! reproduced, and those are what the table holds.

/// One pane of the shell, and the width the reference gives it.
pub struct PaneSpec {
    /// The paint tag.
    pub tag: &'static str,
    /// What a reader calls it.
    pub title: &'static str,
    /// Its width in logical pixels, or 0 for the pane that takes the rest.
    pub width: u32,
    /// The paint tag of its scrolling body, or `None` when the pane has none.
    ///
    /// ★ R1662 — a pane whose content can exceed it must scroll, or the
    /// overflow is painted outside the window where no gesture reaches it.
    /// Stated HERE, in the specification, because it is a property of the
    /// screen and not of one painter: the gate reads this column and checks the
    /// painted scene against it, so a pane that quietly stops scrolling fails
    /// rather than going silent.
    pub body: Option<&'static str>,
}

/// The four panes, left to right.
pub const PANES: &[PaneSpec] = &[
    PaneSpec {
        tag: "lab.rail",
        title: "",
        width: 54,
        // Fixed seats, one per destination: the rail's content is the
        // specification's own list and cannot outgrow the pane.
        body: None,
    },
    PaneSpec {
        tag: "lab.palette",
        title: "Node Palette",
        width: 230,
        body: Some("lab.palette.body"),
    },
    PaneSpec {
        tag: "lab.canvas",
        title: "",
        width: 0,
        // The canvas moves under a PAN, over a world surface it sizes itself,
        // rather than over a scrolled body — a different gesture with a
        // different offset, so it is not this column's business.
        body: None,
    },
    PaneSpec {
        tag: "lab.inspector",
        title: "Node Inspector",
        width: 312,
        body: Some("lab.inspector.body"),
    },
];

/// The application bar's height, and the canvas toolbar's.
pub const APP_BAR_H: u32 = 54;
/// The canvas toolbar's height.
pub const TOOLBAR_H: u32 = 46;

/// A palette entry: a role a node can be given.
pub struct RoleSpec {
    /// The role's name.
    pub name: &'static str,
    /// What it does, in the one line the palette has room for.
    pub gist: &'static str,
    /// Which group it sits in.
    pub group: &'static str,
    /// Whether it can accept an inbound link at all.
    pub accepts: bool,
}

/// The palette, grouped by what a node is *for*. Two groups, because that is
/// what the reference's first-release palette has — infrastructure that carries
/// traffic, and the traffic itself.
pub const ROLES: &[RoleSpec] = &[
    RoleSpec {
        name: "Router",
        gist: "listens, routes",
        group: "infrastructure",
        accepts: true,
    },
    RoleSpec {
        name: "Peer",
        gist: "joins the mesh",
        group: "infrastructure",
        accepts: true,
    },
    RoleSpec {
        name: "Client",
        gist: "one router only",
        group: "infrastructure",
        accepts: false,
    },
    RoleSpec {
        name: "Store",
        gist: "volume, key range",
        group: "infrastructure",
        accepts: true,
    },
    RoleSpec {
        name: "Publisher",
        gist: "sends, with a class",
        group: "traffic",
        accepts: false,
    },
    RoleSpec {
        name: "Subscriber",
        gist: "receives",
        group: "traffic",
        accepts: true,
    },
    RoleSpec {
        name: "Querier",
        gist: "asks, on a period",
        group: "traffic",
        accepts: false,
    },
    RoleSpec {
        name: "Responder",
        gist: "answers",
        group: "traffic",
        accepts: true,
    },
];

/// The pin legend: three appearances, and what each one means.
///
/// Carried as data because the legend and the pins have to agree, and a legend
/// written beside the painter is a second copy of the rule.
pub const PIN_LEGEND: &[(&str, &str)] = &[
    ("dial", "a pin that can call out"),
    ("accept", "a pin that can be called, coloured by protocol"),
    ("closed", "nothing is listening, so nothing can call it"),
];

/// The transports a link can use, in the order the legend shows them.
pub const PROTOCOLS: &[&str] = &["tcp", "tls", "quic", "udp", "ws"];

/// A host frame: a box on the canvas that says which machine its nodes start
/// on.
pub struct FrameSpec {
    /// The frame's name.
    pub name: &'static str,
    /// What it is for, shown on its tab.
    pub gist: &'static str,
    /// Canvas-space rectangle: x, y, w, h.
    pub rect: (u32, u32, u32, u32),
}

/// The two hosts the opening graph is spread across.
pub const FRAMES: &[FrameSpec] = &[
    FrameSpec {
        name: "host-b",
        gist: "load",
        rect: (2, 30, 154, 430),
    },
    FrameSpec {
        name: "host-a",
        gist: "core",
        rect: (166, 18, 510, 462),
    },
];

/// One node of the opening graph.
pub struct NodeSpec {
    /// Its identifier, which is what the canvas card and the inspector show.
    pub id: &'static str,
    /// Its role, one of [`ROLES`].
    pub role: &'static str,
    /// The three-letter badge on the card.
    pub badge: &'static str,
    /// Canvas-space position and card width.
    pub rect: (u32, u32, u32),
    /// Which frame it starts on.
    pub frame: &'static str,
    /// The key/value lines the card shows, which are a digest of its form.
    pub rows: &'static [(&'static str, &'static str)],
}

/// The opening graph: eight nodes across two hosts, exactly as the reference
/// screen shows them.
pub const NODES: &[NodeSpec] = &[
    NodeSpec {
        id: "T-01",
        role: "Publisher",
        badge: "PUB",
        rect: (10, 60, 146),
        frame: "host-b",
        rows: &[
            ("mode", "client"),
            ("ke", "demo/units/1/pose"),
            ("rate", "100 msg/s"),
            ("class", "data, express"),
        ],
    },
    NodeSpec {
        id: "Q-01",
        role: "Querier",
        badge: "QRY",
        rect: (10, 326, 146),
        frame: "host-b",
        rows: &[
            ("select", "demo/units/**"),
            ("every", "500 ms"),
            ("fold", "latest"),
        ],
    },
    NodeSpec {
        id: "P-01",
        role: "Peer",
        badge: "PEER",
        rect: (172, 52, 150),
        frame: "host-a",
        rows: &[
            ("id", "b1"),
            ("listen", "tcp/0.0.0.0:7448"),
            ("discovery", "true"),
            ("routing", "peer_to_peer"),
        ],
    },
    NodeSpec {
        id: "S-01",
        role: "Store",
        badge: "STO",
        rect: (172, 322, 150),
        frame: "host-a",
        rows: &[
            ("id", "c1"),
            ("volume", "memory"),
            ("ke", "demo/units/**"),
            ("stamp", "true"),
        ],
    },
    NodeSpec {
        id: "R-01",
        role: "Router",
        badge: "RTR",
        rect: (344, 186, 152),
        frame: "host-a",
        rows: &[
            ("id", "a1"),
            ("listen", "tcp/0.0.0.0:7447"),
            ("control", "read, write"),
        ],
    },
    NodeSpec {
        id: "P-02",
        role: "Peer",
        badge: "PEER",
        rect: (520, 44, 146),
        frame: "host-a",
        rows: &[
            ("id", "b2"),
            ("listen", "tcp/0.0.0.0:7449"),
            ("discovery", "true"),
        ],
    },
    NodeSpec {
        id: "T-02",
        role: "Subscriber",
        badge: "SUB",
        rect: (520, 196, 146),
        frame: "host-a",
        rows: &[
            ("mode", "client"),
            ("ke", "demo/units/**"),
            ("expect", "1000 samples"),
        ],
    },
    NodeSpec {
        id: "P-03",
        role: "Peer",
        badge: "PEER",
        rect: (520, 348, 146),
        frame: "host-a",
        rows: &[("id", "b3"), ("listen", "tcp/0.0.0.0:7451")],
    },
];

/// The opening links, source first. Seven of them, which is what the toolbar
/// says and therefore what the canvas has to draw.
pub const LINKS: &[(&str, &str)] = &[
    ("T-01", "P-01"),
    ("Q-01", "R-01"),
    ("P-01", "R-01"),
    ("S-01", "R-01"),
    ("R-01", "P-02"),
    ("R-01", "T-02"),
    ("R-01", "P-03"),
];

/// The link the screen opens with selected, and therefore the only one whose
/// label is drawn. The reference is explicit that a label belongs to the
/// selected link alone: eight labels on seven wires is not a diagram.
pub const SELECTED_LINK: (&str, &str) = ("P-01", "R-01");

/// One row of the inspector, for the node the screen opens with selected.
pub struct FieldSpec {
    /// The configuration path, verbatim — this is the key, not a label.
    pub key: &'static str,
    /// The word the configuration calls this kind of value.
    pub ty: &'static str,
    /// `hot` or `restart`.
    pub applies: &'static str,
    /// What the field holds when the screen opens.
    pub value: &'static str,
}

/// The node the screen opens with selected.
pub const SELECTED_NODE: &str = "R-01";

/// The inspector's rows for [`SELECTED_NODE`].
///
/// **Exactly one is `hot`.** That is the reference's own point and the reason
/// the badge exists: a configuration of this kind has very few live-editable
/// keys, and a form that did not say which is which turns a correct tool into
/// an apparently broken one.
pub const FIELDS: &[FieldSpec] = &[
    FieldSpec {
        key: "id",
        ty: "text",
        applies: "restart",
        value: "a1",
    },
    FieldSpec {
        key: "listen.endpoints",
        ty: "locator[]",
        applies: "restart",
        value: "tcp/0.0.0.0:7447",
    },
    FieldSpec {
        key: "connect.endpoints",
        ty: "locator[]",
        applies: "hot",
        value: "tcp/10.0.0.21:7449",
    },
    FieldSpec {
        key: "control.permissions",
        ty: "perm",
        applies: "restart",
        value: "read, write",
    },
    FieldSpec {
        key: "transport.link.tx.batch_size",
        ty: "int",
        applies: "restart",
        value: "65535",
    },
];

/// The keys the inspector offers to add, as the reference's chips.
pub const ADDABLE: &[&str] = &[
    "discovery.multicast",
    "timestamping",
    "compression",
    "qos.priority",
    "routing.mode",
    "plugins",
];

/// The graph's name and the zoom the screen opens at.
pub const GRAPH_NAME: &str = "mesh-failover";
/// The zoom percentage the screen opens at.
pub const OPENING_ZOOM: u32 = 84;

/// The gestures the canvas advertises on its hint strip, and which this screen
/// therefore has to answer.
pub const GESTURES: &[(&str, &str)] = &[
    ("drag empty space", "pan"),
    ("wheel", "zoom"),
    ("drag a node", "place it, hold ctrl to snap"),
    ("drag a pin", "author a link"),
];

/// The icon rail, top to bottom. `locked` marks the destinations the first
/// release deliberately shows and does not open — the reference exposes the
/// later scope as locked seats rather than hiding it, so a reader can see what
/// the tool is going to be.
pub const RAIL: &[(&str, bool)] = &[
    ("dashboard", false),
    ("packets", false),
    ("keys", false),
    ("logs", false),
    ("lab", false),
    ("topology", true),
    ("sessions", true),
];

/// Which rail seat is the one this screen is.
pub const RAIL_ACTIVE: &str = "lab";
