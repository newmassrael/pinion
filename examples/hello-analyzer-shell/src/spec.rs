//! R1668 — **the reference screen C, written down as a value.**
//!
//! Screens A and B have held their reference this way since R1651 and R1663:
//! the reference states a screen, and a round can *claim* to have reproduced it
//! or it can put the screen in a table a machine reads and let a test compare
//! the painted scene against it in **both** directions, so an element the
//! screen is missing and an element the screen invented are both failures.
//! Screen C had neither the table nor the comparison — it was the one screen of
//! the three assembled against nothing but memory, and this module is the half
//! that was absent.
//!
//! Screen C is the dashboard: an application bar over an icon rail, a board of
//! placed widgets under a layout bar, and the palette the board is populated
//! from. What makes it the screen worth writing down is the palette: the first
//! release places **four** widgets and reserves **nine** more, and the reference
//! is emphatic that the nine are shown rather than hidden — *"a second-release
//! item is not a missing thing, it is a locked seat"* — so that the shape of the
//! finished tool is legible before it exists, and so the shell, the layout and
//! the routing do not have to be redesigned when the nine arrive.
//!
//! That is why this round gave the framework
//! [`Unavailable`](pinion_core::availability::Unavailable): a locked seat that
//! could only be drawn grey would be a screenshot of the reference rather than
//! a reproduction of it. Each reserved item states the requirement it is booked
//! under, the shell hands that to the disabled cascade, and it comes back out
//! on `scene/disabled` and in the accessibility tree.
//!
//! **Vocabulary is neutral by construction.** The reference's product name,
//! protocol words, interface names and resource paths are replaced with the
//! words the tool class uses generally, and its requirement identifiers with
//! plain ordinals. The structure and the behaviour are what is being
//! reproduced, and those are what this table holds.

/// The window the screen is specified at.
pub const WIN_W: u32 = 1440;
/// The window height the screen is specified at.
pub const WIN_H: u32 = 900;

/// The application bar's height.
pub const APP_BAR_H: u32 = 52;
/// The layout bar's height — the strip under the application bar carrying the
/// preset name, the placed count and the two board verbs.
pub const SUB_BAR_H: u32 = 46;
/// The icon rail's width.
pub const RAIL_W: u32 = 52;
/// The palette panel's width.
pub const PALETTE_W: u32 = 292;
/// The board's column count. The reference states it as a constant, and a
/// board's arrangement is only portable between two tools that agree what a
/// cell is.
pub const GRID_COLS: u32 = 12;

// --- The application bar -----------------------------------------------------

/// The capture the screen opens on, as the application bar states it: an
/// interface and an endpoint.
///
/// A documentation address (RFC 5737 TEST-NET-1) — no real address belongs in a
/// repository.
pub const SOURCE: &str = "eth0 \u{00B7} 192.0.2.10:7447";

/// The transport verb the application bar shows while a capture is running.
pub const TRANSPORT: &str = "Capturing";

/// The rate readout beside it, in the units a capture tool counts in.
pub const RATE: &str = "1,284 msg/s";

/// What the search field says when it is empty.
pub const SEARCH_HINT: &str = "name, address, value\u{2026}";

// --- The rail ----------------------------------------------------------------

/// One seat on the icon rail.
pub struct RailSpec {
    /// The seat's key, and the suffix of its paint tag.
    pub key: &'static str,
    /// What a reader calls it.
    pub title: &'static str,
    /// The requirement this seat is booked under when it is reserved, or `None`
    /// when the first release opens it.
    ///
    /// The reference locks two rail seats the same way it locks palette items,
    /// and for the same stated reason: *"when the second release arrives the
    /// lock is lifted and the screen structure does not change"*.
    pub reserved_for: Option<&'static str>,
}

/// The rail, top to bottom.
pub const RAIL: &[RailSpec] = &[
    RailSpec {
        key: "dashboard",
        title: "Dashboard",
        reserved_for: None,
    },
    RailSpec {
        key: "stream",
        title: "Stream",
        reserved_for: None,
    },
    RailSpec {
        key: "decode",
        title: "Decode",
        reserved_for: None,
    },
    RailSpec {
        key: "catalog",
        title: "Catalog",
        reserved_for: None,
    },
    RailSpec {
        key: "settings",
        title: "Settings",
        reserved_for: None,
    },
    RailSpec {
        key: "topology",
        title: "Topology",
        reserved_for: Some("requirement 12"),
    },
    RailSpec {
        key: "sessions",
        title: "Sessions",
        reserved_for: Some("requirement 14"),
    },
];

/// Which rail seat this screen is.
pub const RAIL_ACTIVE: &str = "dashboard";

// --- The layout bar ----------------------------------------------------------

/// The name of the layout the screen opens with.
pub const PRESET: &str = "Overview";

/// The two verbs the layout bar offers, left to right.
///
/// The second is the one the palette exists for, and the reference gives it the
/// primary emphasis: a board is *populated by a person*, so how many widgets are
/// on it is a decision somebody made rather than a seed.
pub const BOARD_VERBS: &[&str] = &["Edit Layout", "Add Widget"];

// --- The catalogue -----------------------------------------------------------

/// Which release a catalogue entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// The first release places it. The palette offers a control that adds it
    /// to the board.
    Placeable,
    /// A later release brings it. The palette shows it, names it, describes it
    /// and states the requirement it is booked under — and does not add it.
    Reserved,
}

/// One entry in the widget palette.
pub struct WidgetSpec {
    /// The kind key, and the suffix of its paint tags.
    pub kind: &'static str,
    /// The three-letter code the palette's swatch carries.
    pub code: &'static str,
    /// What a reader calls it.
    pub label: &'static str,
    /// What it does, in the one line the palette row has room for.
    pub gist: &'static str,
    /// Which palette section it sits in.
    pub section: &'static str,
    /// Which release brings it.
    pub tier: Tier,
    /// The requirement a [`Tier::Reserved`] entry is booked under, and `""` for
    /// a placeable one.
    ///
    /// This is the string the shell hands to
    /// [`Unavailable::reserved`](pinion_core::availability::Unavailable::reserved),
    /// so what the palette row says and what `scene/disabled` reports are the
    /// same value rather than two spellings of one intention.
    pub reserved_for: &'static str,
}

/// The palette's sections, in the order the panel lists them, with the release
/// each section's entries belong to.
///
/// The reference's own grouping: the sections are homogeneous in tier, because
/// the tier is part of the heading a reader scans.
pub const SECTIONS: &[(&str, &str, Tier)] = &[
    ("capture", "CAPTURE & DECODE", Tier::Placeable),
    ("visual", "VISUALIZATION", Tier::Reserved),
    ("operate", "DIAGNOSE & OPERATE", Tier::Reserved),
];

/// The thirteen catalogue entries: four the first release places, nine it
/// reserves.
///
/// The counts are the point of the screen and the palette's footer states both,
/// so a kind added here without a section, or moved between tiers, makes two
/// numbers on the screen disagree — which is what the gate next door checks.
pub const CATALOGUE: &[WidgetSpec] = &[
    WidgetSpec {
        kind: "packet",
        code: "PKT",
        label: "Message Stream",
        gist: "colour-coded live list",
        section: "capture",
        tier: Tier::Placeable,
        reserved_for: "",
    },
    WidgetSpec {
        kind: "decode",
        code: "DEC",
        label: "Decode Inspector",
        gist: "layer tree beside the bytes",
        section: "capture",
        tier: Tier::Placeable,
        reserved_for: "",
    },
    WidgetSpec {
        kind: "keymap",
        code: "IDM",
        label: "Identifier Map",
        gist: "numeric id to resource path",
        section: "capture",
        tier: Tier::Placeable,
        reserved_for: "",
    },
    WidgetSpec {
        kind: "filter",
        code: "FLT",
        label: "Search & Filter",
        gist: "query bar, saved chips, counts",
        section: "capture",
        tier: Tier::Placeable,
        reserved_for: "",
    },
    WidgetSpec {
        kind: "topology",
        code: "TOP",
        label: "Topology Map",
        gist: "live connection structure",
        section: "visual",
        tier: Tier::Reserved,
        reserved_for: "requirement 12",
    },
    WidgetSpec {
        kind: "overlay",
        code: "OVL",
        label: "Topology Overlay",
        gist: "observed structure over declared",
        section: "visual",
        tier: Tier::Reserved,
        reserved_for: "requirement 13",
    },
    WidgetSpec {
        kind: "throughput",
        code: "THR",
        label: "Throughput",
        gist: "time series per resource",
        section: "visual",
        tier: Tier::Reserved,
        reserved_for: "requirement 16",
    },
    WidgetSpec {
        kind: "share",
        code: "SHR",
        label: "Traffic Share",
        gist: "share of volume by endpoint",
        section: "visual",
        tier: Tier::Reserved,
        reserved_for: "requirement 17",
    },
    WidgetSpec {
        kind: "latency",
        code: "LAT",
        label: "Latency",
        gist: "request to reply round trip",
        section: "visual",
        tier: Tier::Reserved,
        reserved_for: "requirement 19",
    },
    WidgetSpec {
        kind: "health",
        code: "HLT",
        label: "Health Tiles",
        gist: "session and error summary",
        section: "operate",
        tier: Tier::Reserved,
        reserved_for: "requirement 18",
    },
    WidgetSpec {
        kind: "loss",
        code: "LOS",
        label: "Loss Tracker",
        gist: "sequence-gap lanes",
        section: "operate",
        tier: Tier::Reserved,
        reserved_for: "requirement 20",
    },
    WidgetSpec {
        kind: "alarms",
        code: "ALM",
        label: "Alarms",
        gist: "highlight rules by severity",
        section: "operate",
        tier: Tier::Reserved,
        reserved_for: "requirement 21",
    },
    WidgetSpec {
        kind: "admin",
        code: "ADM",
        label: "Admin Query",
        gist: "ask a node about itself",
        section: "operate",
        tier: Tier::Reserved,
        reserved_for: "requirement 14",
    },
];

/// The palette panel's heading and the line under it.
pub const PALETTE_TITLE: &str = "Widget Palette";
/// The palette panel's sub-heading.
pub const PALETTE_HINT: &str = "Add one to the board";

/// How many entries the first release places.
#[must_use]
pub fn placeable_count() -> usize {
    CATALOGUE
        .iter()
        .filter(|w| w.tier == Tier::Placeable)
        .count()
}

/// How many entries it reserves.
#[must_use]
pub fn reserved_count() -> usize {
    CATALOGUE
        .iter()
        .filter(|w| w.tier == Tier::Reserved)
        .count()
}

/// The catalogue entry for a kind.
#[must_use]
pub fn widget_of(kind: &str) -> Option<&'static WidgetSpec> {
    CATALOGUE.iter().find(|w| w.kind == kind)
}

// --- The opening board -------------------------------------------------------

/// One placed widget: its kind, its top-left cell, and the cells it spans.
pub struct PlacedSpec {
    /// The catalogue kind.
    pub kind: &'static str,
    /// Column of its left edge, zero-based.
    pub col: u32,
    /// Row of its top edge, zero-based.
    pub row: u32,
    /// Columns spanned.
    pub cols: u32,
    /// Rows spanned.
    pub rows: u32,
}

/// The board the "Overview" layout opens with: **all four** of the first
/// release's widgets, placed.
///
/// The reference opens with every placeable widget on the board and the palette
/// showing that it has nothing left to offer for this release — which is a
/// stronger statement than an arbitrary subset, and the one the footer's
/// "4 placed" is counting.
pub const BOARD: &[PlacedSpec] = &[
    PlacedSpec {
        kind: "packet",
        col: 0,
        row: 0,
        cols: 7,
        rows: 2,
    },
    PlacedSpec {
        kind: "decode",
        col: 7,
        row: 0,
        cols: 5,
        rows: 2,
    },
    PlacedSpec {
        kind: "keymap",
        col: 0,
        row: 2,
        cols: 6,
        rows: 2,
    },
    PlacedSpec {
        kind: "filter",
        col: 6,
        row: 2,
        cols: 6,
        rows: 2,
    },
];

// ★ R1669 — the bottom two cards are TWO rows tall, and that is a measurement
// rather than a preference. At one row the identifier map's body is 140px and
// its specified header plus seven rows need 144, so the card could never show
// what this table says it holds -- at any window size, in every state. The
// painter clamped correctly and the screen was quietly a row short, which the
// clamp-coverage gate found by noticing that the map's rows were never once
// observed complete. A card that cannot hold its own specification is a
// specification that has not been reproduced.

// --- What each placed widget's body holds ------------------------------------

/// The columns of the message-stream table, left to right, with the width the
/// reference gives each. `0` takes what the others leave.
pub const STREAM_COLUMNS: &[(&str, u32)] = &[("time", 92), ("type", 78), ("name", 0), ("len", 46)];

/// The message-stream rows the screen opens with: time, type, name, length.
///
/// Eight, which is what the reference's card height holds — a body that scrolls
/// past its own card is the defect screen B was rebuilt around in R1662.
pub const STREAM_ROWS: &[(&str, &str, &str, &str)] = &[
    ("12:04:38.221", "Data", "units/1/pose", "48"),
    ("12:04:38.198", "Query", "store/**", "64"),
    ("12:04:38.140", "Reply", "store/config", "212"),
    ("12:04:37.960", "Declare", "id 4 = units/*/pose", "32"),
    ("12:04:37.902", "Data", "fragment 1 of 3", "1,280"),
    ("12:04:37.771", "Control", "keep-alive P-05", "16"),
    ("12:04:37.660", "Data", "units/0/frame", "40"),
    ("12:04:37.540", "Data", "id 7 = units/2/pose", "48"),
];

/// The distinct message types the stream colours, in the order the legend would
/// list them. A type on a row that is not in this list is a row the painter
/// cannot colour.
pub const STREAM_TYPES: &[&str] = &["Data", "Query", "Reply", "Declare", "Control"];

/// The decode inspector's tree: indent level, key, value. Level 0 is a layer
/// heading and carries no value.
pub const DECODE_ROWS: &[(u32, &str, &str)] = &[
    (0, "L1 \u{00B7} Transport", ""),
    (1, "reliability", "reliable"),
    (1, "sequence", "3419"),
    (1, "fragment", "3 of 3"),
    (0, "L3 \u{00B7} Application", ""),
    (1, "message", "Put"),
    (1, "id", "4 + /1/depth"),
    (1, "resolved", "units/1/depth"),
];

/// Which decode row the card opens with selected, as an index into
/// [`DECODE_ROWS`].
pub const DECODE_SELECTED: usize = 2;

/// The bytes the decode inspector shows beside the tree, four per line.
pub const DECODE_BYTES: &[[u8; 4]] = &[
    [0x16, 0x03, 0x04, 0xa0],
    [0x01, 0x5b, 0x00, 0x00],
    [0x0c, 0x5b, 0x00, 0x00],
    [0x0d, 0x5b, 0x04, 0x2f],
    [0x31, 0x2f, 0x64, 0x65],
    [0x70, 0x74, 0x68, 0x00],
];

/// The byte range the selected decode row occupies, as `[start, end)` into the
/// flattened [`DECODE_BYTES`].
///
/// The reference lights exactly the bytes the selected field came from, which
/// is the law screen B was built around in R1663 — stated here so screen C is
/// held to it too rather than merely resembling it.
pub const DECODE_SELECTED_SPAN: (usize, usize) = (12, 14);

/// The identifier map's columns.
pub const MAP_COLUMNS: &[&str] = &["id", "resource", "first seen"];

/// The identifier map's rows: id, resource path, when it was first declared.
///
/// The last row is the reference's **unresolved** one — an identifier that was
/// declared before the capture started, so the path is not knowable and the row
/// says so instead of guessing. It is the row that justifies the card having a
/// warning ink at all.
pub const MAP_ROWS: &[(&str, &str, &str)] = &[
    ("4", "units/*/pose", "37.960"),
    ("6", "store/**", "36.904"),
    ("7", "units/2/pose", "36.512"),
    ("9", "mesh/telemetry", "35.880"),
    ("11", "units/0/frame", "35.204"),
    ("14", "units/1/depth", "34.771"),
    ("23", "declared before the capture", "\u{2014}"),
];

/// Which map row is the unresolved one, as an index into [`MAP_ROWS`].
pub const MAP_UNRESOLVED: usize = 6;

/// The query the filter card opens with.
pub const FILTER_QUERY: &str = "name ~= \"units/**\"";

/// The saved filter chips, and whether each is on when the screen opens.
pub const FILTER_CHIPS: &[(&str, bool)] = &[
    ("units only", true),
    ("shared memory", false),
    ("reassembly failed", false),
    ("exclude P-03", false),
    ("declares only", false),
];

/// The filter card's three counts: value, then what it counts.
///
/// Three rather than one because the reference's point is the *relation* — a
/// reader is looking at a subset of a subset, and a single number cannot say
/// which subset it is.
pub const FILTER_STATS: &[(&str, &str)] = &[
    ("12,418", "matched"),
    ("184,392", "captured"),
    ("37", "shown"),
];

/// The header controls a placed card carries, in the order they are painted.
///
/// Every placed card carries the same four in the reference: the board is
/// uniform, and a card missing a control it should have is exactly the kind of
/// drift a hand-maintained screen accumulates.
pub const CARD_CHROME: &[&str] = &["settings", "tear_off", "maximize", "close"];
