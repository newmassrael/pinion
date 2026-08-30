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

use pinion_core::edge_panel::{EdgePlacement, EdgePolicy};
use pinion_core::style::ChromeEdge;

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
    /// ★★★★★ R1801 — **where this pane may live**, declared rather than implied.
    ///
    /// A reader asked three times why the palette and the inspector cannot be
    /// moved, and the measured answer came from the wire: `scene/drop_targets`
    /// answers `clauses: []` for this surface. Nothing had ever *said* they were
    /// movable, so there was nothing for a gesture to be checked against and
    /// nothing for a reader of the specification to see.
    ///
    /// Declared here because that is where every other property of a pane is
    /// declared, and because the specification is what the wire publishes: a
    /// panel that may move now says so where a client can read it, and a rail
    /// that may not says that too ([`EdgePolicy::fixed`]).
    pub policy: EdgePolicy,
    /// ★★★★★ R1902 — **where this pane opens**, declared rather than seeded by
    /// a constant nobody checks.
    ///
    /// [`Self::policy`] says where a pane MAY live; this says where it IS when
    /// a reader arrives, and until R1902 the two could contradict each other
    /// for the whole life of the program. Measured then: every *change* to a
    /// placement went through [`EdgePolicy::admit`] / `admit_fold` /
    /// `admit_extent`, and the placement the screen STARTED in went through
    /// nothing at all — it was two `const`s in the painter. A pane could
    /// declare it does not fold and open folded, and nothing would say a word.
    ///
    /// Declared here for the reason [`Self::policy`] is: this is where every
    /// property of a pane is declared, and the specification is what the wire
    /// publishes — so a client can tell *folded because it opens that way* from
    /// *folded because somebody folded it*, which are the same bit and
    /// different facts.
    ///
    /// The gate is `r1902_every_pane_opens_where_its_own_policy_admits`.
    pub opens: EdgePlacement,
    /// ★★★★★ R1909 — **which declared elements this pane HOLDS**, as tag
    /// prefixes, so a folded pane's contents can be told from the rest of the
    /// screen without asking the paint.
    ///
    /// # Why this had to be declared
    ///
    /// Because folding a pane makes its contents disappear, and until R1909 no
    /// pane of this screen opened folded, so the question "which of the
    /// declared elements just went away?" had never been asked. The tables
    /// elsewhere in this file — the voice census, the operation roster — list
    /// what this screen is MADE OF; they cannot also say what is showing right
    /// now, and a folded pane is precisely the case where those two answers
    /// differ. Measured at R1909, declaring one pane folded reported thirty-six
    /// declared elements as "the screen does not paint", every one of them a
    /// correct consequence and none of them a defect.
    ///
    /// # Why prefixes rather than a rectangle
    ///
    /// Because a rectangle would be derived from the paint, and this has to be
    /// readable when there is no paint to read: the whole point is to excuse an
    /// element that is ABSENT. A prefix is a claim about the screen's naming
    /// that a gate can check in both directions — see
    /// `r1909_a_folded_pane_hides_exactly_what_it_holds`, which asserts that
    /// everything under these prefixes really does vanish when the pane folds
    /// AND really does come back when it opens. An excuse list nothing checks
    /// is the escape hatch that disables its own gate.
    ///
    /// ⚠ Empty for a pane that holds no declared element of its own, which is a
    /// statement and not a gap — the rail's seats are declared under its own
    /// tag, so its prefix list is exactly that tag.
    pub holds: &'static [&'static str],
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
        // A rail is where it is. Declaring that is not a formality: an empty
        // allowed-set is a statement a client can read, and it is what makes
        // "this one does not move" different from "nobody has said".
        policy: EdgePolicy::fixed(),
        // A rail has no edge to open on. `Left` is the side it is drawn on and
        // its policy admits nothing, so this says "there is nowhere else" twice
        // — which is what makes the gate below non-vacuous for it too.
        opens: EdgePlacement::open(ChromeEdge::Left, 54),
        // Its seats are named under its own tag, and it does not fold.
        holds: &["lab.rail"],
    },
    PaneSpec {
        tag: "lab.palette",
        title: "Node Palette",
        width: 230,
        body: Some("lab.palette.body"),
        // ★★★★★ R1889 — and a reader may drag its width between these.
        //
        // The floor is what the widest chip row needs before its label starts
        // clipping; the ceiling is where a palette stops being chrome and
        // starts competing with the canvas it serves. Both are declared here
        // rather than clamped in the paint, so the gate can ask the
        // specification what this panel promises and then hold the screen to it.
        policy: EdgePolicy::movable(SIDES).resizable(180, 420),
        // ★★★★★ R1902 — **OPEN, and that is a re-measurement overturning this
        // campaign's own prescription.**
        //
        // The campaign's order step 3 says "hidden by default (the reference
        // editor's palette is)", and the reference editor does flag its tools
        // region hidden. But the reproduction target for THIS screen is the
        // behaviour canon, and the canon was extracted and read this round: its
        // palette state initialises to open, and it carries `togglePalette` /
        // `openPalette` so a reader puts it away and brings it back. Opening
        // folded would have been a second-pass change that UN-REPRODUCES the
        // canon — the standing order rule's named error, skipping reproduction
        // because our way looks better.
        //
        // ⇒ what this declaration is worth is not the value but the fact that
        // there IS one: it is now checked against this pane's own policy, which
        // nothing did before.
        opens: EdgePlacement::open(ChromeEdge::Left, 230),
        // Everything this panel draws is named under its own tag: the chips,
        // the pin legend, the discovery switch and its caption.
        holds: &["lab.palette"],
    },
    PaneSpec {
        tag: "lab.canvas",
        title: "",
        width: 0,
        // The canvas moves under a PAN, over a world surface it sizes itself,
        // rather than over a scrolled body — a different gesture with a
        // different offset, so it is not this column's business.
        body: None,
        // The canvas is what the side panels flank; it has no edge of its own.
        policy: EdgePolicy::fixed(),
        // Width 0 above: the canvas takes what the flanking panes leave, so its
        // opening extent is not a number anybody chose.
        opens: EdgePlacement::open(ChromeEdge::Left, 0),
        // ⚠ The graph's cards and wires are named `lab.node.*` / `lab.wire.*`,
        // not under this tag. Listed because they ARE what the canvas holds,
        // and a prefix list that named only `lab.canvas` would be describing
        // the pane's frame rather than its contents. The canvas does not fold,
        // so nothing here is ever excused — which is the point of writing it
        // down anyway: the gate can then check that the four lists PARTITION
        // the screen rather than merely covering the folding panes.
        holds: &["lab.canvas", "lab.node.", "lab.wire."],
    },
    PaneSpec {
        tag: "lab.inspector",
        title: "Node Inspector",
        width: 312,
        body: Some("lab.inspector.body"),
        // ★ R1889 — wider bounds than the palette's, and the reason is in the
        // content: this pane holds a form with labelled rows and a three-across
        // action strip, where the palette holds a single column of chips.
        policy: EdgePolicy::movable(SIDES).resizable(240, 520),
        // ★★★★★ R1909 — **FOLDED, on the right, as the reference editor's own
        // properties region is.** The campaign's order step 3, actually built.
        //
        // 🟥 The comment that stood here claimed an asymmetry with the palette
        // that did not exist — both panels opened showing, and had since R1902
        // reverted its folded palette. Now the asymmetry is REAL, and it is the
        // one the reference draws:
        //
        // * the PALETTE opens showing, because "what can I put on the canvas"
        //   is the question a reader arrives with, and the behaviour canon's own
        //   drawer initialises `paletteOpen: true`.
        // * the INSPECTOR opens folded, because "what are the properties of the
        //   selected node" has no subject yet — nothing is selected on a screen
        //   nobody has touched, so the pane would open onto its own empty state
        //   and take 312 px to say so.
        //
        // ⚠ Why this does NOT un-reproduce the behaviour canon, which is the
        // objection R1902 raised and answered: that canon has `togglePalette`
        // beside `state.widgets` — it is the DASHBOARD shell's drawer — and no
        // opinion whatever about this screen's panels. A second-pass improvement
        // is legitimate exactly where the canon is silent, and the standing rule
        // it would break is *skipping a reproduction because our way looks
        // better*, which is not this: there is nothing here to reproduce.
        //
        // 🟥🟥🟥★★★★★ R1908 declined this on a stronger-sounding claim — "the
        // canon has NO panel that opens folded, so opening one folded here
        // un-reproduces it" — and it was measured over **one line** of the
        // canon: the line its opening state is written on. Re-measured at R1909
        // over the WHOLE document, that canon opens with a node of its own graph
        // ALREADY COLLAPSED and another already muted, seeded in its opening
        // state and restored by its own reset. So *a thing this tool opens with
        // put away* is in the canon's vocabulary after all — it is a node rather
        // than a panel, which is exactly why the canon's silence about panels is
        // silence and not a prohibition.
        //
        // ⇒ ★★★★★ *the population a measurement covers is the population it was
        // taken from* — twice over here, once for the subject (a drawer, not
        // this panel) and once for the extent of the reading (one line, not the
        // document).
        //
        // ⚠ And it is a PLACEMENT, not an act somebody performed before the
        // reader arrived: `EdgePlacement::folded_at` keeps the extent, so the
        // strip opens to 312 px rather than to nothing. `EdgePolicy` admits it
        // because `movable` is foldable — the gate
        // `r1902_every_pane_opens_where_its_own_policy_admits` is what checks
        // that rather than this comment.
        opens: EdgePlacement::folded_at(ChromeEdge::Right, 312),
        // ★★★★★ R1909 — THREE prefixes, and that is the finding this field
        // exists to record: the inspector's contents are not all named after
        // it. The settings form is `lab.form.*` and the fault panel is
        // `lab.faults*`, both drawn inside this pane, so a reader asking "what
        // goes away when the inspector folds" could not have answered it from
        // the tag alone — which is why this is declared rather than derived
        // from a prefix match on `tag`.
        holds: &["lab.inspector", "lab.faults", "lab.form."],
    },
];

/// The edges a side panel of this screen may occupy.
///
/// Left and right only, deliberately: the reference editor this screen is
/// measured against gives its node editor a left tools region and a right
/// sidebar, and puts the header on the horizontal edges. A top or bottom
/// palette is a different screen, not a placement of this one.
const SIDES: &[ChromeEdge] = &[ChromeEdge::Left, ChromeEdge::Right];

/// The application bar's height, and the canvas toolbar's.
pub const APP_BAR_H: u32 = 54;
/// The canvas toolbar's height.
pub const TOOLBAR_H: u32 = 46;

/// One traffic parameter a message carries, by the name the wire uses for it.
///
/// ★★★★★ R1848 — the census's `lab.t1.8` asks for *traffic nodes carrying rate,
/// payload, priority, congestion and reliability*, and its verdict is `app`:
/// the framework owns what a node IS ([`pinion_node_graph`]'s `NodeKind`) and
/// declines to own which parameters a domain's traffic has, because every
/// domain's are different. This is that taxonomy, declared where the domain
/// lives.
///
/// ⚠ What made it worth declaring rather than assuming: the opening graph
/// already PAINTS traffic parameters, as free-text key/value rows on a card
/// ([`NodeSpec::rows`]) with nothing saying which keys a role may use. So
/// nothing could ask whether a node states a parameter its role has, or states
/// one its role does not — the class this tree keeps repairing, one screen at a
/// time. A closed vocabulary is what turns that into a question with an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficParameter {
    /// How often it sends.
    Rate,
    /// How much each message carries.
    Payload,
    /// Which traffic it is served before.
    Priority,
    /// What it does when the path cannot keep up.
    Congestion,
    /// Whether delivery is guaranteed.
    Reliability,
}

impl TrafficParameter {
    /// The key a card's row uses for it, and the word the wire publishes.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            TrafficParameter::Rate => "rate",
            TrafficParameter::Payload => "payload",
            TrafficParameter::Priority => "priority",
            TrafficParameter::Congestion => "congestion",
            TrafficParameter::Reliability => "reliability",
        }
    }
}

/// The whole vocabulary, in the order the capability names it.
///
/// Closed on purpose: a parameter that is not here cannot be declared by a
/// role, which is what makes [`RoleSpec::carries`] a taxonomy rather than a
/// list of whatever anyone typed.
pub const TRAFFIC_PARAMETERS: &[TrafficParameter] = &[
    TrafficParameter::Rate,
    TrafficParameter::Payload,
    TrafficParameter::Priority,
    TrafficParameter::Congestion,
    TrafficParameter::Reliability,
];

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
    /// ★ R1848 — the traffic parameters a node in this role carries.
    ///
    /// Empty for every `infrastructure` role, and that emptiness is the
    /// taxonomy's content rather than an omission: a router carries other
    /// nodes' traffic and has none of its own, so a parameter here would be a
    /// claim about somebody else's messages.
    pub carries: &'static [TrafficParameter],
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
        carries: &[],
    },
    RoleSpec {
        name: "Peer",
        gist: "joins the mesh",
        group: "infrastructure",
        accepts: true,
        carries: &[],
    },
    RoleSpec {
        name: "Client",
        gist: "one router only",
        group: "infrastructure",
        accepts: false,
        carries: &[],
    },
    RoleSpec {
        name: "Store",
        gist: "volume, key range",
        group: "infrastructure",
        accepts: true,
        carries: &[],
    },
    // ★ R1848 — the four traffic roles, and the assignment is the DOMAIN's call,
    // which is what the census's `app` verdict means: the framework owns what a
    // node is and declines to own which parameters a domain's traffic has.
    // Each line below says why this role has what it has.
    RoleSpec {
        name: "Publisher",
        gist: "sends, with a class",
        group: "traffic",
        accepts: false,
        // It originates messages, so every parameter is its decision.
        carries: TRAFFIC_PARAMETERS,
    },
    RoleSpec {
        name: "Subscriber",
        gist: "receives",
        group: "traffic",
        accepts: true,
        // It chooses neither how often nor how large — those are the sender's.
        // What it does declare is how it wants to be served and whether it will
        // accept loss.
        carries: &[TrafficParameter::Priority, TrafficParameter::Reliability],
    },
    RoleSpec {
        name: "Querier",
        gist: "asks, on a period",
        group: "traffic",
        accepts: false,
        // A period IS a rate, and a query carries a payload; what it cannot
        // decide is what a congested path does to somebody else's answer.
        carries: &[
            TrafficParameter::Rate,
            TrafficParameter::Payload,
            TrafficParameter::Priority,
            TrafficParameter::Reliability,
        ],
    },
    RoleSpec {
        name: "Responder",
        gist: "answers",
        group: "traffic",
        accepts: true,
        // It answers when asked, so it has no rate of its own.
        carries: &[
            TrafficParameter::Payload,
            TrafficParameter::Priority,
            TrafficParameter::Reliability,
        ],
    },
];

/// The role a node has, if its name is one this palette offers.
#[must_use]
pub fn role_of(node: &NodeSpec) -> Option<&'static RoleSpec> {
    ROLES.iter().find(|role| role.name == node.role)
}

/// The traffic parameters a node's card actually STATES, of the ones its role
/// carries.
///
/// ★★★★★ R1848 — the join the screen did not have. A card's rows are free text
/// keyed by whatever the row was written with, and the role said nothing about
/// which keys belong to it, so "does this node state its priority?" had nowhere
/// to be asked. It does now, and the answer is derived from the two rather than
/// recorded a third time.
#[must_use]
pub fn stated_traffic(node: &NodeSpec) -> Vec<TrafficParameter> {
    role_of(node).map_or_else(Vec::new, |role| {
        role.carries
            .iter()
            .copied()
            .filter(|p| node.rows.iter().any(|(key, _)| *key == p.key()))
            .collect()
    })
}

/// The parameters a node's role carries that its card does NOT state.
///
/// ⚠ Not a defect list. The opening graph is the reference's screen and this
/// tree reproduces it; what this answers is how much of the declared taxonomy
/// that screen puts in front of a reader, which is a measurement the screen
/// could not previously produce about itself.
#[must_use]
pub fn unstated_traffic(node: &NodeSpec) -> Vec<TrafficParameter> {
    let stated = stated_traffic(node);
    role_of(node).map_or_else(Vec::new, |role| {
        role.carries
            .iter()
            .copied()
            .filter(|p| !stated.contains(p))
            .collect()
    })
}

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
    /// ★★★ R1716 — what the screen worked this row's value out from, or
    /// `None` when somebody wrote it.
    ///
    /// The specification's column rather than a fact discovered from the form,
    /// because it decides what the screen must PAINT: a row nobody wrote shows
    /// where its value came from, offers to be taken over rather than removed,
    /// and — when an edit could not have reached a running node anyway — does
    /// not say what an edit would cost.
    pub source: Option<&'static str>,
    /// ★★ R1716 — what this row is about instead of configuration, when it is
    /// not configuration at all. `None` means it ships in the document.
    pub aside: Option<&'static str>,
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
    // ★★★★★ R1716 — first, and worked out from the node's ROLE. A router that
    // came up in client mode would not be the node the canvas draws, so this is
    // not a value anybody types; it is a reading of the card, and the row says
    // so where a writable row says what an edit would cost.
    FieldSpec {
        key: "mode",
        ty: "mode",
        applies: "restart",
        value: "router",
        source: Some("role"),
        aside: None,
    },
    // ★★ R1690 — the type words changed with the shapes. `id` said `text`
    // while the target reads it with a parser, which is the defect the option
    // surface exposed: the word a row is labelled with and the shape it is
    // checked against were two independent claims, and this screen shipped
    // three rounds where they disagreed.
    FieldSpec {
        key: "id",
        ty: "id",
        applies: "restart",
        value: "a1",
        source: None,
        aside: None,
    },
    FieldSpec {
        key: "listen.endpoints",
        ty: "address[]",
        applies: "restart",
        value: "tcp/0.0.0.0:7447",
        source: None,
        aside: None,
    },
    // ★★★★★ R1842 — TWO rows, where this was one `perm` row until the option
    // surface stopped being written from memory. The target declares the two
    // permissions as separate boolean leaves and takes them as an object of
    // booleans; a single set-valued row composed an ARRAY at a path that is not
    // a leaf at all, so the exported configuration was a document the target
    // refuses — and refuses loudly, because a wrong TYPE stops it starting where
    // an unknown key only warns. The reference paints one control over both,
    // which this tree has no field shape for; that control is the widget-catalog
    // axis's, and it is recorded there rather than papered over here.
    FieldSpec {
        key: "admin.permissions.read",
        ty: "bool",
        applies: "restart",
        value: "true",
        source: None,
        aside: None,
    },
    FieldSpec {
        key: "admin.permissions.write",
        ty: "bool",
        applies: "restart",
        value: "true",
        source: None,
        aside: None,
    },
    FieldSpec {
        key: "transport.link.tx.batch_size",
        ty: "int",
        applies: "restart",
        value: "65535",
        source: None,
        aside: None,
    },
    // ★★★ R1716 — where this node runs. It appears because the graph is drawn
    // across two host frames; on a graph with one it would say nothing a reader
    // could act on, and the canon leaves it out for that reason. It is NOT
    // configuration — the plan starts the process there — so it goes aside and
    // never reaches the document.
    FieldSpec {
        key: "host",
        ty: "text",
        applies: "restart",
        value: "host-a",
        source: Some("frame"),
        aside: Some("placement"),
    },
    // ★★★★★ R1716 — worked out from the WIRES. Before this round the row held
    // an address typed beside the code while the canvas drew three links out of
    // this card, and the exported configuration shipped the typed one: a node
    // dialled where nothing listens and missed one it was drawn to reach.
    FieldSpec {
        key: "connect.endpoints",
        ty: "address[]",
        applies: "hot",
        value: "tcp/host-a:7449, tcp/host-a:7451",
        source: Some("wire"),
        aside: None,
    },
];

/// The keys the inspector offers to add, as the reference's chips.
///
/// ★★★ R1690 — **three of these named a section**, and the option surface is
/// what said so. A form row holds one value, so a chip keyed at a section
/// composes a string where the configuration wants a subtree: `timestamping`
/// and `compression` each have two leaves under them and `plugins` has two, and
/// all three offered the section itself. Adding any of them produced a document
/// the target would refuse, and no count could see it — the key IS on the
/// surface, so a reach figure read it as covered.
///
/// They name the leaf they meant now, and `Reach::unauthorable` is the gate
/// that fails if one goes back.
pub const ADDABLE: &[&str] = &[
    // ★★ R1716 — first because it is the one a person reaches for on a card
    // with no drawn links: the row is worked out from the wires when there are
    // any, and offered here when there are none.
    "connect.endpoints",
    // ★★★★★ R1842 — five of these seven changed spelling, and one was dropped
    // outright. The option surface is read from the target's own declaration
    // now, so a chip that names a key the target does not take is refusable
    // rather than merely wrong: `discovery.multicast` and `compression.enabled`
    // were sections of the real path, `routing.mode` was a paraphrase, and
    // `plugins.names` named nothing at all. `qos.priority` is the dropped one
    // and the sharpest of the six — it is not configuration in the first place,
    // it is a per-message traffic parameter, and the reference marks it as such
    // on its own rows.
    "discovery.multicast.enabled",
    "timestamping.enabled",
    "transport.unicast.compression.enabled",
    "namespace",
    "routing.peer.mode",
    "plugin_loading.search_dirs",
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
    // ★★ R1773 — `requirement 14` here until that round, where the reference
    // books this seat under 18. Nothing could see it: the shape gate accepts
    // any number and the census counts what is drawn, so the wrong number sat
    // in a locked seat's tooltip for as long as the seat existed.
    ("sessions", Some("requirement 18")),
    // ★★★★★ R1773 — the eighth seat, absent from this screen's copy while the
    // reference and the assembled shell both carry it. One application has one
    // navigation; this screen held a second copy of the roster and it had lost
    // a destination.
    ("settings", None),
];

/// Which rail seat is the one this screen is.
pub const RAIL_ACTIVE: &str = "lab";

/// ★★★★★ R1773 — the reference's rail, as `docs/analyzer-rail-spec.json`
/// states it, compiled in so the gate cannot pass by finding no file.
///
/// [`RAIL`] above is what this screen draws; this is what it is supposed to
/// be, and they are two artifacts on purpose — a specification written by the
/// same hand in the same edit as the thing it judges is a gate asking the
/// subject for the answer. The sibling screen has had both halves since R1728;
/// this one had only the first, and measured at R1773 its hand-written copy had
/// drifted **twice** without anything noticing.
///
/// ⚠ Second parser of this pin in the tree (the assembled shell has the other).
/// A third should lift the parsing rather than copy it again.
///
/// ⚠ `cfg(test)` because only the gate reads it: this crate is also built as a
/// library by the assembled shell, where an unread constant is a dead-code
/// error. The sibling screen's copy is not gated because its running screen
/// derives its destinations from the pin, and this one does not — it keeps its
/// own roster and is CHECKED against the pin, which is the comparison that can
/// fail for the right reason.
#[cfg(test)]
pub const RAIL_SPEC_JSON: &str = include_str!("../../../docs/analyzer-rail-spec.json");

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
///
/// ★★ R1697 — **the shape is the framework's now**, and this is a re-export
/// rather than a second copy of it. The sibling screen produced the identical
/// defect three rounds later (a detached panel that could be torn off and not
/// moved, with every gate on that screen green), which is the second consumer
/// this table's shape needed — so
/// [`Operation`](pinion_core::operation::Operation) holds the columns and the
/// consistency checks a reader of the table alone can run, and each screen
/// keeps only its own rows and its own driver.
pub use pinion_core::operation::Operation as OperationSpec;

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
    // ★★★ R1706 — this row gained its verb, and it is the same act on both
    // channels because the reference's is: its frame-drag handler SELECTS the
    // host's cards on its first line and then carries them, so a
    // `select_frame` beside a `move_frame` would be two spellings of a gesture
    // a person cannot perform in halves.
    //
    // ★ It also stopped being one of the rows a person could cause and an
    // agent could not — the asymmetry this table's two cause columns exist to
    // surface, on a screen whose premise is that an agent drives it.
    OperationSpec {
        name: "move a frame and its members",
        verb: Some(("move_frame", "host-b,40,30")),
        gesture: true,
        witness: "layout",
        needs: None,
    },
    // ── the form ─────────────────────────────────────────────────
    OperationSpec {
        name: "add a field from the catalogue",
        verb: Some(("add_field", "timestamping.enabled")),
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
        verb: Some(("remove_field", "admin.permissions.write")),
        gesture: true,
        witness: "form",
        needs: None,
    },
    // ★★★ R1716 — the act that exists because some rows are NOT somebody's to
    // write: taking one over. `mode` is worked out from the role on every card,
    // so it is the row this is always available on.
    OperationSpec {
        name: "take a derived field over",
        verb: Some(("author_field", "mode")),
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

/// ★★★★★ R1691 — **what a reader is told this screen has**, which nothing in
/// this table said until now.
///
/// The tables above describe what is painted and what can be done. Neither says
/// what reaches somebody who never sees the drawing, and the gap that opened
/// under that silence was measured the day this was written: the screen painted
/// **166** addressable regions and announced **30** of them (its tree held 35
/// nodes; five are virtual description regions). The palette, the icon rail,
/// the canvas's frames and wires and pins, the launch gate, the gesture hint and
/// the inspector's own chrome had no voice at all — and every check in this
/// example was green, because a region with no accessibility node paints
/// perfectly and answers every question about its rectangle.
///
/// [`pinion_core::voice`] is what makes that measurable at all: it classifies
/// every addressable region as announced, deliberately silent, or a hole. But a
/// *total* census can be satisfied by declaring everything silent, and that is
/// what this table exists to prevent. It pins the **split**: these regions owe a
/// voice and these owe a silence, and a round that moves one across the line
/// fails here rather than in somebody's screen reader.
///
/// # Why a population and not a list of tags
///
/// Because most of the screen is one shape repeated per item — a button per
/// role, a link per rail seat, a card per node, a wire per link, a control per
/// field — and a list of the expanded tags would be a second copy of the tables
/// above. [`Population`] names the table the family comes from, so the gate
/// expands it and a family that grows a member cannot be satisfied by the
/// members that were there when this was written.
pub struct VoiceSpec {
    /// The tag, verbatim for [`Population::One`] and with `{}` where the family
    /// substitutes its member's name otherwise.
    pub tag: &'static str,
    /// The role a reader is told this region is — the WAI-ARIA word
    /// `scene/access` publishes, so the two surfaces join on it.
    pub role: &'static str,
    /// Which table the family's members come from.
    pub population: Population,
}

/// Where a [`VoiceSpec`]'s members come from.
///
/// Naming the source table rather than the count is what makes the gate hold as
/// the screen grows: a ninth role or a second observed link expands the
/// population automatically, and a member that stopped being announced fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Population {
    /// Exactly one region, at [`VoiceSpec::tag`] verbatim.
    One,
    /// One per [`ROLES`] entry.
    Roles,
    /// One per [`RAIL`] seat.
    Rail,
    /// One per [`NODES`] card.
    Nodes,
    /// One per [`LINKS`] wire, keyed by the link's mint order.
    Links,
    /// One per [`FIELDS`] row of the inspector.
    Fields,
    /// ★★★ R1716 — one per [`FIELDS`] row **somebody wrote**: the rows that
    /// can be taken away, and the rows that say what an edit would cost.
    AuthoredFields,
    /// One per [`FIELDS`] row the screen **worked out**: the rows that name a
    /// source and offer to be taken over.
    DerivedFields,
    /// One per [`FIELDS`] row that is not configuration at all.
    AsideFields,
    /// One per [`FIELDS`] row that carries an applies badge — every authored
    /// row, and a derived row whose value still reaches a running node when its
    /// source moves.
    BadgedFields,
    /// One per [`PROTOCOLS`] chip.
    Protocols,
    /// One per [`PIN_LEGEND`] entry.
    PinKinds,
}

/// Every region of the opening screen that owes a reader a voice, and what it
/// announces as.
///
/// The dynamic regions are deliberately absent — the launch gate's findings, the
/// picked link's chrome, the reset seats — because their population is the
/// state's rather than this table's. They are not unchecked: the census asks the
/// stronger question of them, which is that nothing is left unclassified at all.
pub const VOICES: &[VoiceSpec] = &[
    // The shell.
    VoiceSpec {
        tag: "lab.appbar",
        role: "group",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.appbar.state",
        role: "status",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.rail",
        role: "navigation",
        population: Population::One,
    },
    // ★ A LINK per seat, including the two that are locked — the reference
    // exposes later scope rather than hiding it, and a destination a reader
    // cannot hear about is hidden however visible it is.
    VoiceSpec {
        tag: "lab.rail.{}",
        role: "link",
        population: Population::Rail,
    },
    // The palette.
    VoiceSpec {
        tag: "lab.palette",
        role: "group",
        population: Population::One,
    },
    // ★★★★★ R1887 — the two controls that PLACE this panel. A button each,
    // because each one does a thing to the panel rather than reporting a state
    // of it; what they do is in the name the accessibility tree gives them,
    // which is derived from the placement so it says where pressing would go.
    VoiceSpec {
        tag: "lab.palette.flip",
        role: "button",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.palette.fold",
        role: "button",
        population: Population::One,
    },
    // ⚠ R1887.1 — the strip a folded panel leaves is NOT here, and the reason is
    // this table's own: it lists the regions of the OPENING screen, and the
    // dynamic ones are deliberately absent because their population is the
    // state's rather than this table's. A `VoiceSpec` for the strip made the
    // gate demand a region the opening screen correctly does not paint. The
    // strip is not unchecked — the census asks it the stronger question, that
    // nothing painted is left unclassified in ANY swept state, and the folded
    // state is swept now.
    VoiceSpec {
        tag: "lab.palette.role.{}",
        role: "button",
        population: Population::Roles,
    },
    VoiceSpec {
        tag: "lab.palette.pin.{}",
        role: "group",
        population: Population::PinKinds,
    },
    // ★★ A SWITCH, not a button and not a checkbox. R1681.3 reported this
    // control as unreadable on screen; what a reader is TOLD it is has to be
    // right whatever the ink does.
    VoiceSpec {
        tag: "lab.palette.discovery",
        role: "switch",
        population: Population::One,
    },
    // ★ R1813 — `.caption`, not `.state`: the switch's read-out is the switch
    // box's own caption child now, and that suffix is the framework's statement
    // of whose caption it is. The ROLE is unchanged — a caption that is also a
    // live status region is both, and the two facts are on different axes.
    VoiceSpec {
        tag: "lab.palette.discovery.caption",
        role: "status",
        population: Population::One,
    },
    // The canvas.
    VoiceSpec {
        tag: "lab.canvas",
        role: "group",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.node.{}",
        role: "group",
        population: Population::Nodes,
    },
    VoiceSpec {
        tag: "lab.link.{}",
        role: "group",
        population: Population::Links,
    },
    // The toolbar and the two floating panels.
    VoiceSpec {
        tag: "lab.toolbar",
        role: "toolbar",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.toolbar.meta",
        role: "status",
        population: Population::One,
    },
    // ★ The zoom read-out is painted INSIDE this seat, and declares itself its
    // name — so this row is what makes that redirect true rather than
    // well-formed.
    VoiceSpec {
        tag: "lab.reset.view",
        role: "button",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.gate",
        role: "list",
        population: Population::One,
    },
    // ★★★ The gesture strip. This screen's ONLY statement of what a pointer can
    // do, and it was inaudible — a reader was left to discover panning, zooming
    // and link authoring by trying.
    VoiceSpec {
        tag: "lab.hint",
        role: "status",
        population: Population::One,
    },
    // The inspector.
    VoiceSpec {
        tag: "lab.inspector",
        role: "group",
        population: Population::One,
    },
    // ★★★★★ R1887 — the inspector's own pair, the palette's twins.
    VoiceSpec {
        tag: "lab.inspector.flip",
        role: "button",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.inspector.fold",
        role: "button",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.inspector.id",
        role: "heading",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.inspector.role",
        role: "status",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.inspector.degree",
        role: "status",
        population: Population::One,
    },
    // ★★★ R1706 — how many cards are picked and which one the panel is
    // showing. A `status`, like the two pills beside it: it is a fact the
    // screen keeps up to date rather than something a reader operates.
    VoiceSpec {
        tag: "lab.inspector.selcount",
        role: "status",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.inspector.reach",
        role: "status",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.inspector.note",
        role: "status",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.inspector.rename",
        role: "button",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.inspector.addkey",
        role: "button",
        population: Population::One,
    },
    VoiceSpec {
        tag: "lab.inspector.name",
        role: "textbox",
        population: Population::One,
    },
    // The form's rows, from the framework's painter. The ROLE is the shape's,
    // which is checked against [`FIELDS`]' own type words rather than listed
    // here — see the gate.
    VoiceSpec {
        tag: "lab.form.control.{}",
        role: "",
        population: Population::Fields,
    },
    VoiceSpec {
        tag: "lab.form.remove.{}",
        role: "button",
        population: Population::AuthoredFields,
    },
    // ★★★ R1716 — the other seat, on the other population. Two rows here
    // rather than one with a wildcard, because they are two acts and a reader
    // is told which one they are on.
    VoiceSpec {
        tag: "lab.form.author.{}",
        role: "button",
        population: Population::DerivedFields,
    },
];

/// Every region of the opening screen that deliberately has **no** voice, and
/// the class of silence it declares.
///
/// The other half of the split, and the half that makes the first half mean
/// something: a census with nothing in this column would be satisfiable by
/// naming every rectangle, and one with everything in it by naming none.
///
/// The `kind` words are [`pinion_core::voice::SilenceKind`]'s own wire
/// spellings, so what this table says is what `scene/voice` publishes.
pub const SILENCES: &[(&str, Population, &str)] = &[
    // The graph's name, painted three times and announced once.
    ("lab.appbar.graph", Population::One, "name_of"),
    ("lab.toolbar.title", Population::One, "name_of"),
    // Colour keys. A reader who never sees the colours loses the membership of
    // the transport set, which the palette announces as its value — so the
    // chips are part of that rather than five stops saying one word each.
    ("lab.palette.swatch.{}", Population::Roles, "decorative"),
    ("lab.palette.protocol.{}", Population::Protocols, "part_of"),
    ("lab.palette.discovery.track", Population::One, "decorative"),
    // The scrolling bodies. Their panes are what a reader lands on.
    ("lab.palette.body", Population::One, "layout"),
    ("lab.inspector.body", Population::One, "layout"),
    // A card's identifier and its role chip: the card says both.
    ("lab.node.{}.id", Population::Nodes, "name_of"),
    ("lab.node.{}.badge", Population::Nodes, "part_of"),
    // Captions inside the seat they name.
    ("lab.toolbar.run.label", Population::One, "name_of"),
    ("lab.toolbar.zoom", Population::One, "name_of"),
    ("lab.gate.head", Population::One, "name_of"),
    ("lab.gate.verdict", Population::One, "part_of"),
    ("lab.hint.text", Population::One, "name_of"),
    ("lab.inspector.degree.text", Population::One, "name_of"),
    ("lab.inspector.selcount.text", Population::One, "name_of"),
    ("lab.inspector.reach.text", Population::One, "name_of"),
    ("lab.inspector.note.text", Population::One, "name_of"),
    // The applies badge: its words are already the row's description.
    ("lab.form.applies.{}", Population::BadgedFields, "name_of"),
    // ★★★ R1716 — and so are these two. A derived row's description says
    // "worked out from the role" and a row that goes aside says what it is
    // instead, so the badges are the same words a second time: part of the row
    // rather than stops of their own.
    ("lab.form.source.{}", Population::DerivedFields, "name_of"),
    ("lab.form.aside.{}", Population::AsideFields, "name_of"),
];

/// Whether a save carries what an operation moved.
///
/// ★★★ R1689 — the reference has a meter for exactly this question and it is
/// the fourth of the four self-censuses it publishes. Its shape is worth
/// copying and its strength is not: it asks whether every piece of state is
/// **classified** — carried, or explicitly volatile — and reports whatever is
/// neither. That catches the failure it was written for (somebody adds a
/// setting and forgets to list it, so it is silently dropped from every save)
/// and it does not catch a key that is classified as carried and still does not
/// come back.
///
/// So [`KEPT`] is that partition, and the gate over it asks the stronger
/// question: it drives each operation, saves, puts the screen back, opens the
/// save, and asserts the slot reads what it read **after** the operation for a
/// [`Keeps::Saved`] row — and what it read **before** for a [`Keeps::Volatile`]
/// one. A deliberate omission that is only written down is a claim; one that is
/// checked in the same run as its opposite is a property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keeps {
    /// A save carries it, and opening the save brings it back.
    Saved,
    /// A save deliberately does not carry it.
    Volatile,
}

/// One introspection slot, and whether a save carries it.
pub struct KeptSpec {
    /// The slot, named the way [`OperationSpec::witness`] names it.
    pub witness: &'static str,
    /// Which half of the partition it is in.
    pub keeps: Keeps,
    /// Where it lives, or why it is left behind — the half a bare partition
    /// cannot say, and the half a reader deciding where to put a new piece of
    /// state actually needs.
    pub why: &'static str,
}

/// Every slot an operation can move, partitioned.
///
/// The population is [`OPERATIONS`]' own `witness` column, and the gate asserts
/// that in **both directions**: a slot an operation moves and nobody classified
/// is a save with a hole in it, and a classification for a slot nothing moves is
/// a rule about a fact that does not exist. Counting one way would let the first
/// hide behind the second.
pub const KEPT: &[KeptSpec] = &[
    KeptSpec {
        witness: "nodes",
        keeps: Keeps::Saved,
        why: "the cards are the document's",
    },
    KeptSpec {
        witness: "layout",
        keeps: Keeps::Saved,
        why: "a card's position and its containing frame are both the document's",
    },
    KeptSpec {
        witness: "cards",
        keeps: Keeps::Saved,
        why: "collapsed and switched-off are node state, so the document carries them",
    },
    KeptSpec {
        witness: "frames",
        keeps: Keeps::Saved,
        why: "the containment is the document's and the host NAMES are the screen's",
    },
    KeptSpec {
        witness: "form",
        keeps: Keeps::Saved,
        why: "each card's settings form, whole, in the screen's own companion",
    },
    KeptSpec {
        witness: "links",
        keeps: Keeps::Saved,
        why: "the authored links are the document's",
    },
    KeptSpec {
        witness: "verdict",
        keeps: Keeps::Saved,
        why: "derived from the forms, so it comes back with them rather than being stored",
    },
    KeptSpec {
        witness: "discovery",
        keeps: Keeps::Saved,
        why: "the master switch decides what the graph MEANS, so a file without it is ambiguous",
    },
    KeptSpec {
        witness: "selected",
        keeps: Keeps::Saved,
        why: "by NAME, because that is how every other part of this screen addresses a card",
    },
    // ★★ The view. The reference does NOT keep it — its own volatile list names
    // the pan and the zoom — and this screen does, which is a deliberate
    // divergence rather than an oversight. The reference is a page that is
    // reloaded; this writes a file somebody opens later, and a document that
    // comes back looking nothing like it was left is a document you have to
    // find your place in again. It is registered as a divergence and is the
    // owner's to rule on.
    KeptSpec {
        witness: "pan",
        keeps: Keeps::Saved,
        why: "where the canvas was pointed, as the archive's camera",
    },
    KeptSpec {
        witness: "zoom",
        keeps: Keeps::Saved,
        why: "the other half of the camera; the two are one fact and travel together",
    },
    // ★★★ The one that is deliberately left behind, and the reason is what
    // makes the partition mean something. An exported configuration is a thing
    // somebody PRODUCED at a moment, from a graph that has since been edited;
    // restoring it beside a changed graph would put an artifact on screen that
    // no longer describes what is next to it.
    KeptSpec {
        witness: "produced",
        keeps: Keeps::Volatile,
        why: "an artifact belongs to the moment it was taken, not to the graph",
    },
];

// ── The inspector, as the behaviour reference draws it (R1732) ──────────────

/// The pinned specification of the node inspector's rows.
///
/// `include_str!` rather than a read at run time so the gate cannot pass by
/// finding no file: a specification that goes missing must break the build, not
/// silently stop judging. The same decision the two sibling sections' pins
/// carry, for the same reason.
const INSPECTOR_SPEC_JSON: &str = include_str!("../../../docs/analyzer-inspector-spec.json");

/// The inspector specification, as the framework's own document.
///
/// # Panics
///
/// If the pin is not a specification — unreadable JSON, no surfaces, a
/// duplicate part key, a remainder entry naming no round. All are defects in
/// the pin rather than states the running screen can reach.
#[must_use]
pub fn inspector_document() -> pinion_core::conformance::SpecDocument {
    pinion_core::conformance::SpecDocument::pinned(
        INSPECTOR_SPEC_JSON,
        "docs/analyzer-inspector-spec.json",
    )
}

/// The configuration path whose roster the conformance gate drives.
///
/// One of the keys the palette offers, so the gate reaches it the way a session
/// does — press the chip, and the row is there. Named here rather than spelled
/// in the gate so the two cannot part.
pub const ENUM_KEY: &str = "routing.peer.mode";

/// ★★★★★ R1834 — **the reference has NO level of detail, and this records it as
/// a fact rather than as an absence somebody remembers.**
///
/// The reference applies one transform to the whole graph, so a card and the
/// diagram around it shrink together by construction. Measured over its 195 KB
/// of application logic: **zero** conditionals on zoom, anywhere. It never
/// collapses a card's contents, at any zoom it can reach — and its zoom clamp
/// bottoms at 25%, which is this screen's floor too, so the divergence was one
/// of DETAIL and not one of range.
///
/// This screen used to. `card_shape_at` carried `scaled(FONT_TINY) >= 6`, which
/// hid every row below 67% — across 25%..=66%, a range the reference draws in
/// full. The cause was a face floor of 6px: it stopped text shrinking while the
/// diagram kept shrinking, so a card grew relative to its neighbours until they
/// overlapped, and the collapse was a repair aimed at that symptom. R1834
/// lowered the floor to 1 and removed the collapse.
///
/// Kept as a `const` so
/// `r1834_a_cards_rows_do_not_depend_on_the_zoom` reads it rather than
/// restating it, which is the R1651 shape: a reference fact belongs in the
/// specification, and the gate cross-checks the screen against it.
pub const REFERENCE_COLLAPSES_CARD_DETAIL_AT_LOW_ZOOM: bool = false;

/// The face a digest row is authored at, so a gate can ask what it scales to
/// without reaching for a private constant of the screen.
pub const FONT_TINY_PX: u32 = 9;

// ── The fault-injection panel (R1857) ───────────────────────────────────────

/// ★★★★★ R1857 — **the fault-injection panel, declared where this screen is
/// declared.**
///
/// R1853 built the panel and gave it five gates of its own, and every one of
/// them asks about the panel's *contents*. Nothing said the panel EXISTS. So
/// the screen painted twenty-eight elements this table did not name, and the
/// backward check — the one whose whole job is *the screen invented nothing* —
/// reported them the first time anything looked at the whole screen.
///
/// **What is declared here is the SHAPE, and deliberately not the contents.**
/// A row is one fault the selected card's own declaration admits, derived by
/// `pinion_core::widgets::fault_injection::injectable`; writing those out here
/// would be a hand-maintained copy of a derivation, which is exactly what R1853
/// removed and what would rot the next time a field is added. The shape is the
/// half that is a fact about the SCREEN: a headed box, one row per offer, three
/// parts to a row, and one run per scope the panel cannot reach.
///
/// ⚠ **This panel is a second-pass addition.** The reference screen carries a
/// pre-launch verdict and no fault injection at all, so nothing about it can be
/// read off the reference — which is precisely why it has to be written down
/// somewhere, and why "the reference does not have it" is not a reason to leave
/// it undeclared.
pub struct FaultPanelSpec {
    /// The panel's own box.
    pub tag: &'static str,
    /// Its heading, which states how many offers the derivation produced.
    pub head: &'static str,
    /// One box per offer, addressed `<row_stem><n>` in painted order.
    pub row_stem: &'static str,
    /// The parts every row paints, in the order the row lays them out.
    pub row_parts: &'static [&'static str],
    /// One run per scope the panel cannot offer, addressed `<scope_stem><wire>`.
    pub scope_stem: &'static str,
}

/// The panel, as this screen paints it.
pub const FAULT_PANEL: FaultPanelSpec = FaultPanelSpec {
    tag: "lab.faults",
    head: "lab.faults.head",
    row_stem: "lab.faults.row.",
    row_parts: &["what", "badge", "why"],
    scope_stem: "lab.faults.scope.",
};

impl FaultPanelSpec {
    /// The box for the `n`th offer.
    #[must_use]
    pub fn row(&self, n: usize) -> String {
        format!("{}{n}", self.row_stem)
    }

    /// One part of the `n`th offer's row.
    #[must_use]
    pub fn part(&self, n: usize, part: &str) -> String {
        format!("{}{n}.{part}", self.row_stem)
    }

    /// The run naming the offer's key and arm.
    ///
    /// The three parts are addressed by their DECLARED POSITION rather than by
    /// a literal spelled again at the painter, so renaming one in
    /// [`Self::row_parts`] moves the paint with it instead of leaving the
    /// declaration and the screen to drift.
    #[must_use]
    pub fn what(&self, n: usize) -> String {
        self.part(n, self.row_parts[0])
    }

    /// The run carrying the applies-scope badge the field declares.
    #[must_use]
    pub fn badge(&self, n: usize) -> String {
        self.part(n, self.row_parts[1])
    }

    /// The run carrying the clause that admits the offer.
    #[must_use]
    pub fn why(&self, n: usize) -> String {
        self.part(n, self.row_parts[2])
    }

    /// The run that says why `wire`'s faults are out of reach.
    #[must_use]
    pub fn scope(&self, wire: &str) -> String {
        format!("{}{wire}", self.scope_stem)
    }

    /// **Every element the panel paints**, given how many offers the
    /// declaration admits and which scopes it cannot reach.
    ///
    /// The one derivation, so the screen's painter, the crate's gates and the
    /// wire demo cannot come to disagree about what the panel is made of. The
    /// two arguments are the parts that are *not* facts about the screen: the
    /// offers come from the selected card's declaration and the scopes from
    /// [`pinion_core::widgets::fault_injection::Scope`], and both are asked of
    /// the running application rather than pinned here.
    #[must_use]
    pub fn roster(&self, offers: usize, out_of_reach: &[&str]) -> Vec<String> {
        let mut out = vec![self.tag.to_owned(), self.head.to_owned()];
        for n in 0..offers {
            out.push(self.row(n));
            for part in self.row_parts {
                out.push(self.part(n, part));
            }
        }
        for wire in out_of_reach {
            out.push(self.scope(wire));
        }
        out
    }
}
