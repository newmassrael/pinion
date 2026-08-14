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

/// ★★ R1681 — links a source **reported** that nobody drew, source first.
///
/// The reference opens with exactly one, between the two peers that have
/// automatic discovery switched on, and it is drawn in the warning colour with
/// a dashed stroke. It is not in the graph: it is a claim about the world, and
/// the only thing that can be done to it is take it into the drawing.
///
/// One rather than none because an affordance with nothing to act on is an
/// affordance no test and no person can reach, and half of what this screen was
/// missing on the link axis hid behind exactly that.
pub const OBSERVED: &[(&str, &str)] = &[("P-01", "P-02")];

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

/// The icon rail, top to bottom, and the requirement each locked seat waits for.
///
/// The reference exposes the later scope as locked seats rather than hiding it,
/// so a reader can see what the tool is going to be.
///
/// ★ R1669 — `Some(requirement)` rather than a bare `true`. R1668 gave the
/// framework a channel for WHY a region is inert
/// ([`Unavailable`](pinion_core::availability::Unavailable)) and screen C
/// adopted it; this screen's rail kept the bool it had when there was nowhere
/// to put a reason, so its two locked seats were grey and mute — absent from
/// `scene/disabled`, and announced to a screen reader as ordinary destinations
/// that simply refuse. Two screens of one tool spelled one concept two ways.
pub const RAIL: &[(&str, Option<&'static str>)] = &[
    ("dashboard", None),
    ("packets", None),
    ("keys", None),
    ("logs", None),
    ("lab", None),
    ("topology", Some("requirement 12")),
    ("sessions", Some("requirement 14")),
];

/// Which rail seat is the one this screen is.
pub const RAIL_ACTIVE: &str = "lab";

/// ★★★ R1677 — **what the screen can be asked to DO**, which nothing in this
/// table said until now.
///
/// Everything above describes what the screen *has*: panes, roles, cards,
/// fields, gestures on a hint strip. `painted.rs` compares the painted scene
/// against all of it, in both directions, and that comparison is why several
/// rounds of drift were caught. It cannot catch a missing OPERATION, and the
/// reason is structural: an operation the screen does not answer paints
/// nothing, so there is no tag to look for and no run to measure. A census of
/// what is on screen is blind, by construction, to what the screen cannot do.
///
/// Measured, and the size of the blind spot is the argument: the reference
/// prototype publishes its own operation list and measures its own coverage
/// against it, so the population here is *its* declaration rather than anyone's
/// reading. Against those thirty, this screen answered seven outright, seven in
/// part, and **sixteen not at all** — and every check in this example was green
/// while sixteen thirtieths of the tool were missing. Three of the missing are
/// not scattered: there is no reset of any kind (five operations), and half of
/// a link's life is absent (delete, rewire, choose an endpoint, adopt an
/// observed one, reset).
///
/// So the operations become part of the specification, and the gate asks the
/// question the census cannot: **for every way this table says an operation can
/// be caused, causing it that way changes something observable.**
///
/// # Why two columns of cause and not one
///
/// Because the failure this screen keeps producing lives exactly between them.
/// Every defect a person reported while using this tool had the same shape: the
/// screen advertises an operation, an agent driving the wire gets it, and the
/// pointer does not. A wheel that zooms through `send` and not through the
/// wheel hook; a form row whose press resolves to a named target and then falls
/// through an unhandled arm; a frame that drags without selecting. One column
/// would have hidden every one of them, because the column that works is the
/// one a test naturally drives.
///
/// So [`verb`](OperationSpec::verb) is what an agent uses, [`gesture`] is
/// whether a person has a way in, and the gate drives BOTH — the second through
/// this screen's own pointer handlers, never by writing the state, because a
/// state a test invents can be one no mouse can reach.
///
/// # `Absent` is a row, not a gap
///
/// An operation this screen cannot do is written down with `verb: None`, which
/// is what lets it be counted, ratcheted and — the direction that matters more
/// — **falsified**: if a `None` row turns out to work, the declaration is stale
/// and the gate fails on that too. A table that only listed what works would
/// leave the sixteen exactly as invisible as they were.
///
/// [`gesture`]: OperationSpec::gesture
pub struct OperationSpec {
    /// What the operation is called, in the tool-class words this table uses
    /// throughout.
    pub name: &'static str,
    /// The action an agent invokes, and an argument that exercises it — or
    /// `None` when this screen has no path to the operation at all.
    pub verb: Option<(&'static str, &'static str)>,
    /// Whether a person can cause it with the pointer or the keyboard.
    ///
    /// A bool here and the driver in `painted.rs`, because the driver is a
    /// gesture — a press at a place, a drag between two — and a gesture is not
    /// a value this table can hold. The gate asserts the two agree in both
    /// directions: a `true` with no driver, and a driver for a `false`, are
    /// both failures.
    pub gesture: bool,
    /// The introspection slot whose value must DIFFER once the operation has
    /// run. Named per operation rather than derived, because "what changed" is
    /// the part of an operation a reader most needs and the part a test is
    /// most tempted to skip.
    pub witness: &'static str,
    /// ★★ R1678 — the operation that has to have run first, by name, or `None`
    /// when this one can be caused from the screen as it opens.
    ///
    /// A real property of the tool, not a convenience for the gate. Putting
    /// something back is only possible once it has been changed, which is why
    /// the reference wraps four of its five reset affordances in a conditional
    /// and shows them only when there is something to put back — the operation
    /// and its precondition are one design, and a table that recorded the first
    /// without the second would describe a screen where the buttons are always
    /// there.
    ///
    /// It names an operation rather than describing a state, so the gate
    /// reaches the precondition the way a person would — by doing the earlier
    /// thing — and a `needs` pointing at an operation this table does not hold
    /// is a failure.
    pub needs: Option<&'static str>,
}

/// The thirty operations, in the reference's own order.
///
/// The order is kept because it groups the way the tool does — the node's life,
/// then the frame's, then the form's, then the link's, then the view's, then
/// what leaves the screen — and a re-sorted list would lose the grouping that
/// makes the clusters of absence visible at a glance.
///
/// Every row below is MEASURED against this screen as it stands, not wished
/// for: the `verb` column holds an action the wire actually routes today, and
/// `gesture` says whether a pointer or key path actually reaches it. The gate
/// drives both and would fail on an optimistic entry.
pub const OPERATIONS: &[OperationSpec] = &[
    // ── a node's life ────────────────────────────────────────────
    // ★ The asymmetry runs the other way here: a person can add a node from
    // the palette and an AGENT cannot, because `add_node` is an internal
    // function with no arm on the wire. Nothing said so until this column
    // existed.
    OperationSpec {
        name: "add a node",
        verb: None,
        gesture: true,
        witness: "nodes",
        needs: None,
    },
    // ★★ R1682 — the node's own life, the cluster that was absent together in
    // the same way a link's was before R1681: this screen could make a node and
    // could do nothing else to one.
    //
    // ★ The argument names the node the way the canvas labels it, and the
    // deletion targets `P-03` because it is the one card the opening graph
    // gives no inbound link — a row that deleted a hub would be asserting
    // something about how much else goes with it.
    OperationSpec {
        name: "delete a node",
        verb: Some(("delete_node", "P-03")),
        gesture: true,
        witness: "nodes",
        needs: None,
    },
    // ★★★ R1683 — a gesture at last, and what it took was the axis rather than
    // a box: this screen had no text entry ANYWHERE, so every operation needing
    // a value typed was pointer-unreachable together. One field, the
    // framework's own, with a target — so the same field answers this row and
    // the "add a field by typing its key" row below it.
    OperationSpec {
        name: "rename a node",
        verb: Some(("rename", "P-03,edge-01")),
        gesture: true,
        witness: "nodes",
        needs: None,
    },
    OperationSpec {
        name: "reset the node set",
        verb: Some(("reset", "nodes")),
        gesture: true,
        witness: "nodes",
        needs: Some("add a node"),
    },
    OperationSpec {
        name: "move a node",
        verb: None,
        gesture: true,
        witness: "layout",
        needs: None,
    },
    OperationSpec {
        name: "reset the layout",
        verb: Some(("reset", "layout")),
        gesture: true,
        witness: "layout",
        needs: Some("move a node"),
    },
    // ★ Both witness `cards` rather than `layout` or `nodes`: neither changes
    // where a card sits or what it is called, and a row witnessing a slot its
    // operation does not move would be a row that can never pass. What they
    // move is the pair of switches a card carries, and that is the slot the
    // wire grew to hold them.
    //
    // ★★ They are TOGGLES, and the answer is the resulting state. The
    // affordance is one button whose label flips, so a verb that took the state
    // to set would be a second shape for the same gesture; a caller wanting a
    // particular state reads what it answered. The four verbs of a link's life
    // took arguments because they take *different* arguments — here there is
    // one argument and one act.
    OperationSpec {
        name: "collapse a node",
        verb: Some(("collapse", "P-03")),
        gesture: true,
        witness: "cards",
        needs: None,
    },
    OperationSpec {
        name: "disable a node",
        verb: Some(("disable", "P-03")),
        gesture: true,
        witness: "cards",
        needs: None,
    },
    // ── a frame's life ───────────────────────────────────────────
    OperationSpec {
        name: "re-parent a node between frames",
        verb: None,
        gesture: true,
        witness: "frames",
        needs: None,
    },
    OperationSpec {
        name: "move a frame and its members",
        verb: None,
        gesture: true,
        witness: "layout",
        needs: None,
    },
    // ── the form ─────────────────────────────────────────────────
    OperationSpec {
        name: "add a field from the catalogue",
        verb: Some(("add_field", "timestamping")),
        gesture: true,
        witness: "form",
        needs: None,
    },
    // ★★ R1683 — the second consumer of the one field, and the reason it was
    // built as a field with a TARGET rather than a rename box. The catalogue is
    // a list of the paths worth reaching for, not the boundary of what a
    // configuration has — which the reference says beside its own key box.
    // ★ The witness is the FORM, not the editor: what this operation leaves
    // behind is a row, and the editor it went through is shut again by the
    // time it has. A row witnessing the editor would pass only for as long as
    // the operation was unfinished.
    OperationSpec {
        name: "add a field by typing its key",
        verb: Some(("add_key", "transport.unicast.lowlatency")),
        gesture: true,
        witness: "form",
        needs: None,
    },
    OperationSpec {
        name: "edit a field",
        verb: Some(("set_field", "id=a9")),
        gesture: true,
        witness: "form",
        needs: None,
    },
    // ★★ R1686 closed the gap this row was written to publish: the wire could
    // remove a field and the screen offered no way to, which is as much a gap
    // as the reverse and is what this column exists to say out loud. The
    // affordance is the reference's own — a seat at the trailing edge of every
    // row's key line — and it is the FORM PAINTER's, so the property grid gets
    // it in the same act.
    OperationSpec {
        name: "remove a field",
        verb: Some(("remove_field", "control.permissions")),
        gesture: true,
        witness: "form",
        needs: None,
    },
    OperationSpec {
        name: "reset the fields",
        verb: Some(("reset", "fields")),
        gesture: true,
        witness: "form",
        needs: Some("edit a field"),
    },
    // ── a link's life ────────────────────────────────────────────
    OperationSpec {
        name: "author a link",
        verb: Some(("connect", "S-01,P-02")),
        gesture: true,
        witness: "links",
        needs: None,
    },
    // ★★ R1681 — a link's life after it is drawn. All four were absent, and
    // they were absent together: the screen could make a link and could do
    // nothing else to one.
    // ★ The argument names the link by its ENDS and not by an id, because an id
    // is minted in seeding order and a table asserting `3` would be asserting
    // something about the order this screen happens to author its opening graph
    // in — which is exactly the sort of coupling that survives until somebody
    // adds a link to the specification.
    OperationSpec {
        name: "delete a link",
        verb: Some(("delete_link", "Q-01>R-01")),
        gesture: true,
        witness: "links",
        needs: None,
    },
    OperationSpec {
        name: "rewire a link",
        verb: Some(("relink", "Q-01>R-01,P-03")),
        gesture: true,
        witness: "links",
        needs: None,
    },
    // ★ `needs` an edit, and that is the tool's own shape rather than a
    // convenience: an endpoint is a CHOICE, and there is no choice while the
    // target listens in one place. The reference draws the row of seats only
    // when there is more than one, so growing the list is how a person reaches
    // this — which is exactly what "edit a field" does here.
    OperationSpec {
        name: "select a link endpoint",
        verb: Some(("set_endpoint", "1")),
        gesture: true,
        witness: "links",
        needs: Some("edit a field"),
    },
    OperationSpec {
        name: "adopt an observed link",
        verb: Some(("adopt", "P-01,P-02")),
        gesture: true,
        witness: "links",
        needs: None,
    },
    OperationSpec {
        name: "reset the links",
        verb: Some(("reset", "links")),
        gesture: true,
        witness: "links",
        needs: Some("author a link"),
    },
    // ── the view ─────────────────────────────────────────────────
    OperationSpec {
        name: "pan",
        verb: None,
        gesture: true,
        witness: "pan",
        needs: None,
    },
    // ★ The zoom BUTTONS are the gesture; the wheel the hint strip advertises
    // is not, and `send WheelUp` moving the zoom is what made that look
    // answered. A verb is not a gesture.
    OperationSpec {
        name: "zoom",
        verb: Some(("zoom_by", "in")),
        gesture: true,
        witness: "zoom",
        needs: None,
    },
    // ★★★ R1688 — the arithmetic is the SUBSTRATE's
    // ([`pinion_node_graph::Fit`]) and not this screen's: the other node canvas
    // in this tree already framed its graph by hand, and a second copy of a fold
    // and an affine is how two editors come to disagree about what "everything"
    // means.
    //
    // ★ The witness is `zoom` and not `pan`, and both move: a fit that only
    // panned would be a fit that did not fit. `zoom` is the half that cannot be
    // reached by dragging, so it is the one that says the operation ran.
    OperationSpec {
        name: "fit the graph to the view",
        verb: Some(("fit", "")),
        gesture: true,
        witness: "zoom",
        // ★ Nothing: the opening graph does not fit the opening zoom, which is
        // a fact about this screen's own specification and is asserted where
        // that is checkable rather than encoded as a fake dependency here.
        needs: None,
    },
    // ★ The one reset whose affordance is UNCONDITIONAL — see
    // `ResetScope::gated`. It still `needs` a change, because a reset over an
    // unchanged view moves nothing and the gate would be asserting that a
    // no-op is an operation.
    OperationSpec {
        name: "reset the view",
        verb: Some(("reset", "view")),
        gesture: true,
        witness: "zoom",
        needs: Some("zoom"),
    },
    // ── what leaves the screen ───────────────────────────────────
    // ★★★ R1687 — the pair the reference writes as one group, and closing them
    // together is not tidiness: they are ONE derivation rendered twice (the
    // launch order, each node's document, and the rows that could not go into
    // it), so doing either alone would have meant building the derivation and
    // using half of it — and the unused half is the one that then drifts.
    //
    // ★ The witness is `produced` and not `document`. `document` answers the
    // SELECTED card's configuration, and an export is over the whole graph and
    // does not depend on what is selected — a row witnessing it would have been
    // asserting that exporting changes the card you were looking at.
    OperationSpec {
        name: "export the configuration",
        verb: Some(("export", "")),
        gesture: true,
        witness: "produced",
        needs: None,
    },
    OperationSpec {
        name: "produce the launch script",
        verb: Some(("script", "")),
        gesture: true,
        witness: "produced",
        needs: None,
    },
    // ★ The master discovery switch is written through `scene/intervene`, not
    // through an action — it is a published slot with a value, and an action
    // that set it would be a second path to one fact. `verb` is `None` because
    // this column is the ACTION column; that is a limit of the column and is
    // stated here rather than papered over with an action nobody needs.
    OperationSpec {
        name: "toggle discovery",
        verb: None,
        gesture: true,
        witness: "discovery",
        needs: None,
    },
    // ★★★ R1684 — the gesture arrived, and it arrived as a consequence rather
    // than as a feature. The verdict is derived from the form, so causing it
    // means making a value cross a bound; the integer stepper clamps at the
    // field's ceiling, correctly, and until this round the text rows had no
    // pointer path at all — so an agent could close the launch gate and a
    // person could not. Giving the form's rows the one text field to be typed
    // into answers this row without anything here being aimed at it.
    OperationSpec {
        name: "validate",
        verb: Some(("set_field", "transport.link.tx.batch_size=70000")),
        gesture: true,
        witness: "verdict",
        needs: None,
    },
    // ★★★ R1688 — and the affordance is the LAUNCH CHIP, which was already on
    // screen saying the verdict and doing nothing. That is the reference's own
    // design rather than a saving: the chip is the one thing that says a graph
    // will not start, so it is where a person looks and therefore where they
    // press. It also means this row cost the toolbar no width — which mattered,
    // because the toolbar is what sets this screen's minimum window size.
    //
    // ★ The witness is `selected`, and it moves on the screen as it opens
    // because the card the first finding is on is not the card the screen opens
    // with. A row whose witness could not move would be a row that can never
    // pass; that this one moves without a `needs` is a property of the opening
    // graph, asserted in the tests rather than assumed here.
    OperationSpec {
        name: "go to the first problem",
        verb: Some(("go_to_problem", "")),
        gesture: true,
        witness: "selected",
        needs: None,
    },
];
