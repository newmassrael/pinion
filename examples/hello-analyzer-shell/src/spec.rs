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
//! [`Unavailable`]: a locked seat that
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

use pinion_core::availability::{Unavailable, UnavailableKind};
use pinion_core::conformance::Ledger;
/// R1697 — the operations table's shape, from the framework rather than from a
/// second copy of the sibling screen's (see [`OPERATIONS`]).
pub use pinion_core::operation::Operation as OperationSpec;
use pinion_core::widgets::chip_group::Choice;
use pinion_core::widgets::destination::{
    Destination, Destinations, Required, RosterSpec, SeatSpec,
};
pub use pinion_core::widgets::roving::{Activation, Axis, Ends, RovingSpec};

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

/// ★★★★★ R1695 — what this application does when a rail seat is pressed.
///
/// Before this arm existed the table said only whether a seat was *reserved*,
/// and the four that were neither reserved nor this screen fell through the gap
/// between those two words. Driven through the pointer router and measured:
/// pressing Stream, Decode, Catalog or Settings moved the string the rail
/// highlights itself from and left the painted scene at **193 tagged regions
/// before and 193 after** — the screen said *Stream* and showed the dashboard.
///
/// A seat is now one of three things, and the two that refuse refuse for
/// different reasons a reader must be able to tell apart.
pub enum Seat {
    /// This application shows it: pressing the seat arrives, and the page
    /// region changes.
    Page,
    /// Booked under a named requirement of a release that has not shipped.
    ///
    /// The reference locks rail seats the same way it locks palette items, and
    /// for the same stated reason: *"when the second release arrives the lock is
    /// lifted and the screen structure does not change"*.
    Reserved(&'static str),
    // ★★★★★ R1729 — **`Elsewhere` was here, and it is gone because nothing
    // constructs it any more.**
    //
    // It said *built and shipping, on a different surface of this product*, and
    // it existed because this tree assembled a one-application tool as three
    // executables: R1695 gave it three seats, R1724 took one (the node lab),
    // R1728 found two of the remaining three were seats the reference does not
    // have at all, and R1729 mounted the last real one (the capture viewer).
    // The compiler is what reported the arm was dead — an unconstructed variant
    // is a `dead_code` error under this workspace's lints — so the round did
    // not have to notice.
    //
    // Deleted rather than kept with an allow: an arm nobody builds is a claim
    // about this screen that is no longer true, and its return should be a
    // compile-checked event rather than a comment somebody remembers.
    // `pinion_core::availability::UnavailableKind::Elsewhere` is untouched and
    // still has consumers — the vocabulary is the framework's, and any product
    // with more than one surface needs it. What ended is this rail needing it.
    // ★★★★★ R1731 — **`Unbuilt` was here, and it is gone for the same reason
    // `Elsewhere` went at R1729: nothing constructs it.**
    //
    // It said *in the specification of the release being built, and not built*,
    // and R1728 spent an arm on it because the rail could not be made faithful
    // without one: the reference draws eight seats, this application drew
    // seven, and the two it lacked were absent for a reason none of the arms it
    // had could say. R1730 built the first of those two and R1731 the second,
    // so the arm has no constructor — and an unconstructed variant is a
    // `dead_code` error under this workspace's lints, so the compiler is what
    // reported it rather than a reader noticing.
    //
    // Deleted rather than kept behind an allow, which makes the claim the
    // strongest form available: **this rail cannot say "specified and not
    // built", because it has no way to spell it.** Its return would be a
    // compile-checked event rather than a comment somebody remembers.
    // `pinion_core::availability::UnavailableKind::Unbuilt` is untouched and
    // still has consumers — the vocabulary is the framework's, and any product
    // assembled against a written specification passes through that state.
}

/// One seat on the icon rail.
pub struct RailSpec {
    /// The seat's key, and the suffix of its paint tag.
    pub key: &'static str,
    /// What a reader calls it.
    pub title: &'static str,
    /// What pressing it does.
    pub seat: Seat,
}

impl RailSpec {
    /// The requirement this seat is booked under, when it is booked under one.
    #[must_use]
    pub const fn reserved_for(&self) -> Option<&'static str> {
        match self.seat {
            Seat::Reserved(why) => Some(why),
            Seat::Page => None,
        }
    }
}

/// The rail, top to bottom.
///
/// ★★★★★ R1728 — **the reference's eight seats, in the reference's order.**
/// Before this round it was seven, three of whose keys the reference does not
/// have, and it is worth recording how a divergence that large stayed
/// invisible: nothing compared the two. Measured at this round's open, by
/// pulling the seat list off each of the reference's three delivered screens —
/// they draw the *same* eight in the same order, and differ only in which one
/// is marked current. What this table had instead was one seat of the
/// reference's split into two under invented names, one seat missing outright,
/// and the node lab mounted at a seat drawn with a *different* seat's icon.
///
/// The specification is now a reviewed artifact of its own,
/// `docs/analyzer-rail-spec.json`, and [`canon`] loads it. This table stays
/// because a running seat needs what a specification does not fix — its icon,
/// its page, the wording of its refusal — and because a specification written
/// beside the thing it judges is not a specification.
pub const RAIL: &[RailSpec] = &[
    RailSpec {
        key: "dashboard",
        title: "Dashboard",
        seat: Seat::Page,
    },
    // ★★★★★ R1729 — **the second seat to stop saying *elsewhere*, and the
    // last one that ever could.** Its page is `hello-packet-view`, mounted
    // through `pinion_screen::Mount<PacketView>` — the same screen the
    // standalone binary runs, unedited. What remains closed on this rail is
    // closed for reasons no mount can fix: two sections nobody has built, and
    // two the reference itself defers.
    RailSpec {
        key: "packets",
        title: "Packets",
        seat: Seat::Page,
    },
    // ★★★★★ R1730 — **the first seat this rail ever opened by BUILDING one.**
    //
    // R1724 and R1729 each opened a seat by mounting a screen that already
    // existed somewhere else; this section existed nowhere. Its page is
    // `hello-key-patterns`, whose own three surfaces are checked against
    // `docs/analyzer-keys-spec.json` the way this rail is checked against the
    // pin beside it — so the seat being open is one claim and the section
    // behind it reproducing the reference is another, and both are gated.
    RailSpec {
        key: "keys",
        title: "Key Patterns",
        seat: Seat::Page,
    },
    // ★★★★★ R1731 — **the seat that closed the rail.** Its page is
    // `hello-log-view`, built for it the way `keys` was, and with it every
    // section the reference opens is a section this application opens. What
    // stays shut is shut because the reference itself defers it.
    RailSpec {
        key: "logs",
        title: "Logs",
        seat: Seat::Page,
    },
    // ★★★★★ R1724 — **the first seat to stop saying *elsewhere*.**
    //
    // Its page is `hello-node-lab`, mounted through
    // `pinion_screen::Mount<NodeLabView>` — the same binding the standalone
    // `hello-node-lab` binary runs, unedited. Which destinations have a screen
    // behind them is the `ScreenRoster`'s fact and is not written down a second
    // time here: a seat only says whether it is open.
    //
    // ★ R1728 renamed it. It was `catalog`, which is not a seat the reference
    // has, and it was drawn with the icon the reference gives *Logs* — a list
    // of bulleted lines — so the one seat this application had actually
    // finished was wearing another section's face.
    RailSpec {
        key: "lab",
        title: "Node Lab",
        seat: Seat::Page,
    },
    RailSpec {
        key: "topology",
        title: "Topology",
        seat: Seat::Reserved("requirement 12"),
    },
    // ★ R1728 — booked under requirement 18, not 14. The reference names the
    // requirement in the seat's own tooltip and 14 is not among the six it
    // defers; this said 14 from the round the seat was written.
    RailSpec {
        key: "sessions",
        title: "Sessions",
        seat: Seat::Reserved("requirement 18"),
    },
    RailSpec {
        key: "settings",
        title: "Settings",
        seat: Seat::Page,
    },
];

/// The rail as the framework's own roster, which is what the screen navigates
/// with and publishes.
///
/// Built from [`RAIL`] rather than written twice: the seat table is what a
/// reader of this specification checks against the reference, and the roster is
/// what the application runs on. A second hand-written list is how the two
/// screens of this tool came to disagree about what the tool contains.
///
/// # Panics
///
/// If [`RAIL`] holds a duplicate or blank key, or opens nothing — all three
/// checked by [`Destinations::new`], and all three a defect in this file rather
/// than a state the running screen can reach.
#[must_use]
pub fn destinations() -> Destinations {
    Destinations::new(
        RAIL.iter()
            .map(|seat| match seat.seat {
                Seat::Page => Destination::open(seat.key, seat.title),
                Seat::Reserved(why) => {
                    Destination::closed(seat.key, seat.title, Unavailable::reserved(why))
                }
            })
            .collect(),
    )
    .expect("the rail declares a navigable roster")
}

/// Which rail seat this screen opens at.
pub const RAIL_ACTIVE: &str = "dashboard";

// --- The reference's navigation, and where this build does not reproduce it ---

/// The specification, as text, compiled in.
///
/// `include_str!` rather than a read at run time so the gate cannot pass by
/// finding no file: a specification that goes missing must break the build, not
/// silently stop judging. It is the same posture the census pins take.
const RAIL_SPEC_JSON: &str = include_str!("../../../docs/analyzer-rail-spec.json");

/// ★★★★★ R1728 — **the reference's rail, as `docs/analyzer-rail-spec.json`
/// states it.**
///
/// The specification half of this screen. [`RAIL`] above is what the
/// application runs on; this is what it is supposed to be, and the two are
/// separate artifacts on purpose — a specification written in the same file, in
/// the same edit, by the same hand as the thing it judges is a gate asking the
/// subject for the answer.
///
/// # Panics
///
/// If the specification is not readable as a roster — a malformed pin, a
/// duplicate key, a standing this vocabulary does not have. All of them are
/// defects in the pin rather than states the running screen can reach, and all
/// of them must stop the build rather than quietly weaken the comparison.
#[must_use]
pub fn canon() -> Destinations {
    let doc: serde_json::Value =
        serde_json::from_str(RAIL_SPEC_JSON).expect("the rail specification is readable JSON");
    let seats = doc["canon"]
        .as_array()
        .expect("the rail specification declares a canon array")
        .iter()
        .map(|seat| {
            let key = seat["key"].as_str().expect("a specified seat has a key");
            let title = seat["title"]
                .as_str()
                .expect("a specified seat has a title");
            match seat["standing"].as_str() {
                Some("open") => Destination::open(key.to_owned(), title.to_owned()),
                Some("closed") => {
                    let kind = seat["kind"]
                        .as_str()
                        .expect("a closed seat states its kind");
                    let kind = UnavailableKind::from_name(kind)
                        .expect("a closed seat's kind is one the framework has");
                    Destination::closed(
                        key.to_owned(),
                        title.to_owned(),
                        Unavailable::new(kind, "the behaviour reference"),
                    )
                }
                other => panic!("a specified seat's standing is open or closed, not {other:?}"),
            }
        })
        .collect();
    Destinations::new(seats).expect("the specification is a navigable roster")
}

/// The same specification as the framework's comparable form.
///
/// # Panics
///
/// As [`canon`] — a pin this cannot be built from is a defect in the pin.
#[must_use]
pub fn canon_spec() -> RosterSpec {
    RosterSpec::new(
        canon()
            .all()
            .iter()
            .map(|seat| SeatSpec {
                key: seat.key.clone(),
                title: seat.title.clone(),
                required: Required::of(&seat.standing),
            })
            .collect(),
    )
    .expect("the specification is a navigable roster")
}

/// The declared, reviewed remainder: where this build does not yet reproduce
/// the reference's navigation.
///
/// A list rather than a count, and a *closed* list rather than a floor: the
/// gate asserts the live difference is EXACTLY this, so a new divergence fails
/// and so does an accepted one that has quietly been paid off. The second
/// direction is what keeps the number from drifting up on its own.
///
/// ★★★ R1730 — **the framework's [`Ledger`] rather than a struct and a loop
/// here.** R1728 wrote both, plus the three per-entry conditions, inside this
/// application's own test — an entry whose sentence does not name its key, an
/// entry with no round, an entry with no reason. The key-pattern section needed
/// the same mechanism for three surfaces of its own, which is the third
/// consumer and the lift trigger, and the conditions now refuse a malformed pin
/// at load rather than being remembered per consumer.
///
/// # Panics
///
/// If the pin's `owed` entries are malformed, name no round, state no reason or
/// do not name their own seat — all defects in the pin.
#[must_use]
pub fn owed() -> Ledger {
    let doc: serde_json::Value =
        serde_json::from_str(RAIL_SPEC_JSON).expect("the rail specification is readable JSON");
    Ledger::from_json(&doc).expect("the rail's declared remainder is a readable ledger")
}

/// ★★★★★ R1733 — the BOARD's written specification: the palette row a widget
/// is picked up from, and what the canvas puts on screen while one is carried.
///
/// `include_str!` rather than a read at run time so the gate cannot pass by
/// finding no file: a specification that goes missing must break the build, not
/// silently stop judging.
const BOARD_SPEC_JSON: &str = include_str!("../../../docs/analyzer-board-spec.json");

/// The board's specification, parsed.
///
/// # Panics
///
/// If the document is not a specification — unreadable JSON, a surface with no
/// canon, a remainder entry naming no round or no reason. All defects in the
/// pin, and all of them must stop the build rather than weaken the gate.
#[must_use]
pub fn board_document() -> pinion_core::conformance::SpecDocument {
    pinion_core::conformance::SpecDocument::pinned(BOARD_SPEC_JSON, "docs/analyzer-board-spec.json")
}

/// ★★★★★ R1761 — the DASHBOARD SECTION's written specification: the surfaces a
/// reader sees on this page, as the reference draws them.
///
/// Separate from [`board_document`] by **evidence** rather than by screen — one
/// is read back out of a painted frame at any moment, the other needs a gesture
/// driven and a declaration asked. The pin's own header carries the reasoning
/// and `crate::judge` carries the reading.
const DASHBOARD_SPEC_JSON: &str = include_str!("../../../docs/analyzer-dashboard-spec.json");

/// ★★★★★ R1762 — the PREFERENCES SECTION's written specification: the ten rows
/// the reference draws, in four groups, and the words it heads them with.
const SETTINGS_SPEC_JSON: &str = include_str!("../../../docs/analyzer-settings-spec.json");

/// The preferences section's specification, parsed.
///
/// # Panics
///
/// If the document is not a specification — unreadable JSON, a surface with no
/// canon, a remainder entry naming no round or no reason. All defects in the
/// pin, and all of them must stop the build rather than weaken the gate.
#[must_use]
pub fn settings_document() -> pinion_core::conformance::SpecDocument {
    pinion_core::conformance::SpecDocument::pinned(
        SETTINGS_SPEC_JSON,
        "docs/analyzer-settings-spec.json",
    )
}

/// The dashboard section's specification, parsed.
///
/// # Panics
///
/// If the document is not a specification — unreadable JSON, a surface with no
/// canon, a remainder entry naming no round or no reason. All defects in the
/// pin, and all of them must stop the build rather than weaken the gate.
#[must_use]
pub fn dashboard_document() -> pinion_core::conformance::SpecDocument {
    pinion_core::conformance::SpecDocument::pinned(
        DASHBOARD_SPEC_JSON,
        "docs/analyzer-dashboard-spec.json",
    )
}

// --- The Settings destination -------------------------------------------------
//
// ★★ R1695 — the second page, and the reason the region is worth building: a
// paged region with one page proves nothing. The reference's own Settings
// section is four switch rows in two groups, two rows whose affordance is booked
// for a later release, and a two-way appearance segment — small enough to
// reproduce faithfully in one round and shaped to exercise every part of this
// axis at once (a real control, a locked one, and a choice).

/// One switch row on the Settings page.
pub struct OptionSpec {
    /// The key the wire addresses it by, and the suffix of its paint tag.
    pub key: &'static str,
    /// The row's title.
    pub title: &'static str,
    /// The sentence under it.
    pub gist: &'static str,
    /// The group heading it sits under.
    pub group: &'static str,
    /// Whether the screen opens with it on.
    pub opens: bool,
}

/// The four switches, in the reference's order.
///
/// The opening values are the reference's and they **alternate**, which is worth
/// keeping: a page whose controls all open the same way lets a check that reads
/// the wrong one pass anyway.
pub const OPTIONS: &[OptionSpec] = &[
    OptionSpec {
        key: "reconnect",
        title: "Auto-reconnect",
        gist: "Resume capture on link recovery",
        group: "capture",
        opens: true,
    },
    OptionSpec {
        key: "on_launch",
        title: "Start capture on launch",
        gist: "Begin capturing when the app opens",
        group: "capture",
        opens: false,
    },
    OptionSpec {
        key: "resolve",
        title: "Resolve names from session start",
        gist: "Full mapping requires capturing from the handshake",
        group: "decode",
        opens: true,
    },
    OptionSpec {
        key: "numeric",
        title: "Show unresolved numeric ids",
        gist: "Display the raw id when the name is unknown",
        group: "decode",
        opens: false,
    },
];

/// ★★★★★ R1762 — one preferences row whose control is **a value and a chevron**.
///
/// The reference's first group opens with two of these before either of its
/// switches, and this build had neither: measured at R1761 against the
/// behaviour reference, the capture group draws four rows there and two here.
/// They are the rows that make the page a page rather than a switchboard — the
/// value is a word out of a roster, so the control is the collapsed chooser
/// R1732 built for a form and R1762 lifted out of it.
pub struct ValueRowSpec {
    /// The key the wire addresses it by, and the suffix of its paint tag.
    pub key: &'static str,
    /// The row's title.
    pub title: &'static str,
    /// The sentence under it.
    pub gist: &'static str,
}

/// ★★★★★ R1762 — the page's own heading, and the line under it.
///
/// The reference opens its preferences page with both and this build opened
/// with neither: measured at R1761, a reader arriving here was given four
/// unlabelled cards. A page that does not say what it is is a page whose groups
/// are the only thing naming it.
pub const SETTINGS_HEAD: (&str, &str) = (
    "Settings",
    "Capture, decode, decryption and appearance preferences.",
);

/// ★★★★★ R1762 — the decode group's third row: the payload formats this build
/// can take apart, as the chips the reference lists them in.
///
/// Not a switch and not a chooser — the reference draws a set of formats that
/// are *all* applied, which is a chip row. Its key, its title and the sentence
/// under it, in the reference's own order.
pub const PLUGIN_ROW: (&str, &str, &str) = (
    "plugins",
    "Payload sub-decoders",
    "Format plugins applied to payloads",
);

/// The payload formats the row lists, in the reference's order.
///
/// Neutral by construction: the reference names two third-party serialisations
/// and what is being reproduced is *that there are two named formats and both
/// are on*, so they are named for what they are.
pub const PLUGINS: [&str; 2] = ["schema", "records"];

/// ★★★★★ R1762 — the three facts the page closes with: what this tool is, what
/// wire format it reads, and which build a reader is looking at.
///
/// The reference's own footer, and the one place either screen says which build
/// it is — which is the fact a person filing a defect is asked for first.
pub const BUILD_STRIP: [&str; 3] = ["analysis tool", "wire format 0x09", "build R1762"];

/// The two value rows, in the reference's order — both above the switches.
pub const VALUE_ROWS: &[ValueRowSpec] = &[
    ValueRowSpec {
        key: "interface",
        title: "Interface",
        gist: "Capture source device",
    },
    ValueRowSpec {
        key: "retention",
        title: "Ring buffer size",
        gist: "In-memory capture retention",
    },
];

/// What the capture buffer may be sized to, smallest first.
///
/// A roster rather than a number field, because that is what the reference
/// draws: a word in a box with a chevron, not a spin button. The opening value
/// is the reference's own.
pub const RETENTIONS: [&str; 3] = ["256 MB", "512 MB", "1 GB"];

/// The buffer size the screen opens with — the middle of [`RETENTIONS`], which
/// is the reference's.
pub const RETENTION: &str = "512 MB";
///
/// The reference wires both of these to the same *arrives later* handler, so
/// they are the settings page's own locked seats — declared unavailable the way
/// the rail's are, rather than painted grey.
pub struct KeyRowSpec {
    /// The key the wire addresses it by, and the suffix of its paint tag.
    pub key: &'static str,
    /// The row's title.
    pub title: &'static str,
    /// The sentence under it.
    pub gist: &'static str,
    /// What the button says.
    pub verb: &'static str,
    /// The requirement it is booked under.
    pub reserved_for: &'static str,
}

/// The two key rows, in the reference's order.
pub const KEY_ROWS: &[KeyRowSpec] = &[
    KeyRowSpec {
        key: "keylog",
        title: "Transport key log",
        gist: "Import a key log to decrypt links",
        verb: "Import\u{2026}",
        reserved_for: "requirement 22",
    },
    KeyRowSpec {
        key: "e2e",
        title: "Application end-to-end key",
        gist: "Decode payloads the application encrypted end to end",
        verb: "Add key\u{2026}",
        reserved_for: "requirement 23",
    },
];

/// The appearance row, and the two-way segment on it.
pub const THEME_ROW: (&str, &str) = ("Theme", "Both themes are first-class");
/// The segment's choices, in the reference's order.
pub const THEMES: [&str; 2] = ["Dark", "Light"];

// --- The keyboard ring ---------------------------------------------------------

/// ★★★★★ R1696 — **where the Tab key stops**, in the order it stops there.
///
/// This screen had NONE. Measured on CI the round after it gained an
/// accessibility tree: it announces a `navigation` landmark of links, two
/// `toolbar`s, a `list` of items, four `tab`s, a `textbox` and thirty-nine
/// buttons, and `focus/next` from cold answered nothing at all — announced as
/// operable, unreachable by keyboard. Two gates refused it (`r1518`,
/// `r1570.1`), which is the same pair that refused the sibling screen at R1693
/// for the same reason.
///
/// # The shape is the WAI-ARIA composite pattern
///
/// **One stop per composite**, not one per control: a toolbar, a navigation
/// rail and a list are each a single stop whose members a cursor moves among,
/// which is what every floor does and what the sibling screen already holds.
/// A control that is not inside a composite is its own stop — which on this
/// screen is the Settings page's switches and its appearance segment, and those
/// declare it through the catalogue widgets rather than here.
///
/// # The order is the paint order, and that is RATIFIED rather than missing
///
/// The §5.39 enumeration is depth-first over the painted scene, so this table's
/// order must BE the child order in `view` — and it is asserted to be. The page
/// region is painted first because the chrome is painted over it, so a reader
/// Tabs into the page before the application bar, which is not the order a
/// reader would choose.
///
/// It is worth being exact about why it stays that way. §5.39's own alternatives
/// record **rejects a manual tab index** — the reason given is that imitating
/// HTML's `tabindex` violates the declarative spirit of a view function, and
/// that automatic traversal is the canonical form. So this is not a gap to fill
/// from a consumer; changing it is a spec round, and the argument that round
/// would have to weigh is recorded in
/// `debt-a-screens-tab-order-is-its-z-order`: paint order is not a free choice
/// for a screen whose chrome is painted OVER its page, which is a case the
/// original rejection did not address.
pub struct StopSpec {
    /// The painted tag the stop lands on. It must also be a node in the
    /// accessibility tree, or a reader is told nothing about what they landed
    /// on — the `missing bearer` arm `r1518` reports.
    pub tag: &'static str,
    /// What a cursor does once it is here, in one phrase, for a reader of this
    /// table. Not painted: this is the specification saying what the stop is
    /// FOR, which is the part a tag cannot carry.
    pub holds: &'static str,
    /// Which destination it is on.
    pub at: Where,
    /// ★★★★★ R1698 — **what the arrows do once a reader is here.**
    ///
    /// A stop with no cursor is a room with a door and no floor: R1696 gave
    /// this screen one Tab stop per composite and measuring it the day this
    /// round opened found eleven stops, four arrow keys each, forty-four
    /// presses that moved nothing, and an active descendant that was `None`
    /// everywhere. That is half of WAI-ARIA's composite pattern.
    ///
    /// `None` for a stop that is a single control rather than a composite —
    /// there is nothing inside it for a cursor to move between.
    pub cursor: Option<RovingSpec>,
}

/// The ring, in Tab order — which is paint order.
pub const FOCUS_RING: &[StopSpec] = &[
    StopSpec {
        tag: "shell.canvas",
        holds: "the board; the arrows move the selection among its cards",
        at: Where::At("dashboard"),
        // ★ The board's cursor is SPATIAL — the arrows move to the neighbouring
        // card in that direction, not to the next one in a list — so it is not
        // a linear roster and declares none. It has had that cursor since
        // R1662 and published it to nobody; this round makes it the active
        // descendant, which is the half it was missing.
        cursor: None,
    },
    // ★★★★★ R1721 — the filter card's saved-filter bar, and the first stop this
    // screen has ever had **inside a card**. Measured before this round by driving
    // the running screen: the card's five chips were announced as operable and a
    // keyboard reached none of them — the whole board's interior was pointer-only.
    //
    // It sits here because the ring is enumerated depth-first over the paint and
    // the cards are painted inside the canvas. Its cursor is not written out:
    // `FILTER_ROW` derives it, and this entry reads that derivation so the ring
    // census and the widget cannot disagree about the arrows. The rule is also why
    // the bar is ONE stop rather than five.
    StopSpec {
        tag: "card.filter#3.chips",
        holds: "the filter card's saved filters",
        at: Where::At("dashboard"),
        cursor: FILTER_ROW.cursor(),
    },
    StopSpec {
        tag: "shell.appbar",
        holds: "the application bar's views, source, capture and search",
        at: Where::Chrome,
        // A bar of peers with no meaningful last one, so it wraps.
        cursor: Some(
            RovingSpec::new(Axis::Horizontal)
                .with_ends(Ends::Wrap)
                .with_activation(Activation::Explicit),
        ),
    },
    StopSpec {
        tag: "shell.rail",
        holds: "the tool's destinations",
        at: Where::Chrome,
        // ★★ Vertical, wrapping, and EXPLICIT — the last of those is the one
        // that matters. A rail whose selection followed its cursor would
        // navigate away from the page a reader is trying to leave, once per
        // arrow press, on the way to the destination they want. The floor's
        // tab list has no way to say this: measured, it changes the current
        // tab on every arrow and exposes no property that would let an author
        // ask for anything else.
        cursor: Some(
            RovingSpec::new(Axis::Vertical)
                .with_ends(Ends::Wrap)
                .with_activation(Activation::Explicit),
        ),
    },
    StopSpec {
        tag: "shell.subbar",
        holds: "the layout preset and the two board verbs",
        at: Where::At("dashboard"),
        cursor: Some(
            RovingSpec::new(Axis::Horizontal)
                .with_ends(Ends::Wrap)
                .with_activation(Activation::Explicit),
        ),
    },
    StopSpec {
        tag: "shell.palette",
        holds: "the widget catalogue the board is populated from",
        at: Where::At("dashboard"),
        // ★★ A catalogue read top to bottom, and it STOPS at its ends: the
        // list has a first and a last entry a reader is meant to feel, which
        // wrapping would take away. Its members are the thirteen entries and
        // NOT its accessibility children, which are three section groups and
        // two status readouts — the distinction `Roving` exists to keep.
        cursor: Some(RovingSpec::new(Axis::Vertical).with_ends(Ends::Stop)),
    },
];

/// The Settings page's groups, top to bottom: the key its tag carries and the
/// heading a reader sees.
pub const OPTION_GROUPS: [(&str, &str); 4] = [
    ("capture", "Capture"),
    ("decode", "Decode"),
    ("keys", "Keys"),
    ("appearance", "Appearance"),
];

// --- The layout bar ----------------------------------------------------------

/// The name of the layout the screen opens with.
pub const PRESET: &str = "Overview";

/// The two verbs the layout bar offers, left to right.
///
/// The second is the one the palette exists for, and the reference gives it the
/// primary emphasis: a board is *populated by a person*, so how many widgets are
/// on it is a decision somebody made rather than a seed.
pub const BOARD_VERBS: &[&str] = &["Edit Layout", "Add Widget"];

/// ★★★★★ R1761 — the layout bar's parts, in the order the reference draws
/// them, and what each one is.
///
/// The words beside the keys are this screen's own, not the words on screen:
/// two of the four are *values* (which layout is in effect, how many widgets it
/// holds) and one of the verbs changes its own label while editing, so a
/// specification pinning the painted words would report a working screen as
/// wrong the moment a reader used it. What the pin fixes here is which parts the
/// bar has and in what order — see `crate::judge` for the rule and for which
/// surfaces are judged by their words instead.
pub const LAYOUT_BAR: &[(&str, &str)] = &[
    (
        "preset",
        "the layout in effect, and what opens the list of them",
    ),
    ("count", "how many widgets the board is holding"),
    ("edit", "the verb that turns layout editing on"),
    ("add", "the verb that opens a placement"),
];

/// The two counts along the foot of the palette panel, left to right.
///
/// Both are values, so they are titled here rather than read — and both are
/// derived from [`CATALOGUE`] on screen, which is what makes the pair worth
/// stating: the reference's palette footer says how many this release places
/// *and* how many a later one brings, and the second number is the screen's
/// whole argument about scope.
pub const PALETTE_FOOT: &[(&str, &str)] = &[
    (
        "placed",
        "how many of this release's kinds are on the board",
    ),
    ("reserved", "how many kinds a later release brings"),
];

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

/// ★★★★★ R1761 — **which release a section's entries belong to, in the words
/// the heading paints.**
///
/// The reference writes it into the heading itself — *«group» · «release»* —
/// and this build painted the group and dropped the release, so a reader
/// scanning the panel could see three headings and not which of them the first
/// release fills. Measured against the reference screen at R1761, while writing
/// the palette's surfaces down as a specification: the heading is where the
/// reference answers *when does this arrive*, and the entries beneath it repeat
/// only the requirement, one row at a time.
///
/// **Derived from the tier rather than typed into [`SECTIONS`]**, because a
/// marker written beside the tier is a second copy of it: a section retiered
/// with its heading left alone would say one thing and offer another, and
/// nothing would notice. This way the heading cannot disagree with what the
/// rows do.
#[must_use]
pub fn section_heading(title: &str, tier: Tier) -> String {
    let release = match tier {
        Tier::Placeable => "RELEASE 1",
        Tier::Reserved => "RELEASE 2",
    };
    format!("{title} \u{b7} {release}")
}

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
///
/// ★ R1733 — the reference's own line, reachable at last: it says a row is
/// dragged onto the canvas, and until this round that was an instruction to do
/// something this build could not do. It said "add one to the board" instead,
/// which is true of the click and silent about the gesture.
pub const PALETTE_HINT: &str = "Drag onto the canvas, or click to add";

/// ★ R1733 — what the canvas says while a palette footprint is being carried
/// over it. The reference's whole-canvas invitation, in this tool's words.
pub const DROP_INVITATION: &str = "Drop to add widget";

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

// --- What this screen can be asked to DO -------------------------------------

/// ★★★★★ R1697 — **the operations this screen offers, and the evidence that
/// each one happens.**
///
/// Everything above describes what the dashboard *has*. This is what it *does*,
/// and its absence is why the defect that opened this round survived: a person
/// tore a panel off, tried to drag it, and it was nailed where it landed. The
/// press arm that would have started the gesture read
/// `Hit::Float(_) | Hit::Nothing => {}` — a panel folded in with hitting
/// nothing at all — and every gate on this screen stayed green, each of them
/// correctly. The panel is painted, hit-testable, contained, named and
/// announced. Not one of them asks whether **grabbing it moves it**, and none
/// of them could, because there was no table saying it should.
///
/// The sibling screen has had this table since R1677 and it caught exactly this
/// class three times. The shape now lives in the framework
/// ([`Operation`](pinion_core::operation::Operation)) rather than being copied
/// here, which is the repair for [[debt-a-shape-two-screens-hand-roll-is-a-substrate-hole-nobody-censuses]].
///
/// # Scope: direct manipulation of the board and its panels
///
/// This table is the pointer's half of the dashboard — what a person grabs, and
/// what moves when they do. Deliberately not the whole surface: the rail, the
/// settings switches and the transport are driven by *writing a slot*, and a
/// row whose `verb` column had to mean "intervene" for some rows and "invoke"
/// for others would be a column with two meanings. Those live under their own
/// gates already ([`RAIL`], [`OPTIONS`]).
///
/// # Every row is measured
///
/// The `verb` column holds an action the wire actually routes today; `gesture`
/// says whether a pointer path actually reaches it. The gate drives BOTH and
/// fails on an optimistic entry — and on a `verb: None` row that turns out to
/// work, which is what keeps the absences honest as the screen grows.
pub const OPERATIONS: &[OperationSpec] = &[
    // ── a card on the board ──────────────────────────────────────
    OperationSpec {
        name: "place a widget on the board",
        verb: Some(("add", "packet")),
        gesture: true,
        witness: "layout",
        needs: None,
    },
    // ★ The asymmetry this column exists to show: a person drags a card by its
    // header and an agent has no verb for it at all. Nothing said so until the
    // table did.
    OperationSpec {
        name: "move a card on the board",
        verb: None,
        gesture: true,
        witness: "layout",
        needs: None,
    },
    OperationSpec {
        name: "resize a card",
        verb: Some(("resize", "packet#0,widen")),
        gesture: true,
        witness: "layout",
        needs: None,
    },
    OperationSpec {
        name: "maximise a card",
        verb: Some(("maximize", "packet#0")),
        gesture: true,
        witness: "maximized",
        needs: None,
    },
    OperationSpec {
        name: "restore a maximised card",
        verb: Some(("restore", "")),
        gesture: true,
        witness: "maximized",
        needs: Some("maximise a card"),
    },
    OperationSpec {
        name: "close a card",
        verb: Some(("act", "packet#0,close")),
        gesture: true,
        witness: "layout",
        needs: None,
    },
    // ── a card that has left the board ───────────────────────────
    OperationSpec {
        name: "detach a card",
        verb: Some(("act", "packet#0,tear_off")),
        gesture: true,
        witness: "floating",
        needs: None,
    },
    // ★★★★★ The three rows this round exists for. All three were absent, all
    // three are one gesture in the reference's own source — its float drag
    // calls its raise before reading the panel's origin — and the person who
    // found the first one found it by opening the window and pulling.
    OperationSpec {
        name: "move a detached panel",
        verb: None,
        gesture: true,
        witness: "floats",
        needs: Some("detach a card"),
    },
    OperationSpec {
        name: "size a detached panel",
        verb: None,
        gesture: true,
        witness: "floats",
        needs: Some("detach a card"),
    },
    OperationSpec {
        name: "bring a detached panel forward",
        verb: None,
        gesture: true,
        witness: "floats",
        needs: Some("detach a card"),
    },
    OperationSpec {
        name: "re-dock a detached panel",
        verb: Some(("redock", "packet#0")),
        gesture: true,
        witness: "floating",
        needs: Some("detach a card"),
    },
    OperationSpec {
        name: "close a detached panel",
        verb: None,
        gesture: true,
        witness: "floating",
        needs: Some("detach a card"),
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

/// ★★★★★ R1721 — how many saved filters the card may have applied at once.
///
/// Measured on the reference's own mockup: both places it draws this bar draw
/// exactly one chip lit out of three and out of five, which is the shape of a
/// saved filter — applying one replaces the last, and applying the current one
/// again means "stop filtering". The capture viewer's bar already behaved that
/// way and announced something else; this card announced the same wrong thing
/// and did not behave at all.
///
/// One word, and the `listbox` the bar is announced as, the `option` each chip
/// is, the `aria-selected` that carries its on-ness, the single keyboard stop,
/// the arrows and the `Enter` all follow.
pub const FILTER_ROW: Choice = Choice::AtMostOne;

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

// --- What reaches somebody who never sees the drawing ------------------------

/// The identifier a placed card's tags are built from — `packet#0` and so on,
/// which is [`BOARD`]'s order applied to its kinds.
#[must_use]
pub fn card_ids() -> Vec<String> {
    BOARD
        .iter()
        .enumerate()
        .map(|(n, placed)| format!("{}#{n}", placed.kind))
        .collect()
}

/// One region of the opening screen that owes a reader a voice, and what it
/// announces as.
///
/// Screens A and B have held this table since R1691 and R1693. Screen C had
/// none, and the day this was written it painted **128** addressable regions and
/// announced **five** — a group for the window and one per card, holding
/// nothing. The rail, the bars, both tables, the decode tree, the bytes and the
/// whole palette reached a reader as four names and a summary sentence.
///
/// ★★★★★ **What makes screen C the one worth writing down is the nine locked
/// seats.** They are the screen's entire claim: a second-release item is shown
/// rather than hidden, so the shape of the finished tool is legible before it
/// exists. The framework has stated *why* each is locked since R1668 — a kind, a
/// detail and a derived recourse, on `scene/disabled` — and none of the eleven
/// locked regions was in the accessibility tree at all, so the reason reached
/// nobody it was built for.
///
/// Measured at 6.11.1 by building and running the same shape rather than reading
/// about it: a locked entry in an item view and a locked destination in a tab
/// bar answer **`focusable, selectable` and carry no unavailable state at all** —
/// the bit survives only on a plain widget. So there a reader is invited to
/// activate exactly the seats the screen has closed.
pub struct VoiceSpec {
    /// The tag, verbatim for [`Population::One`] and with `{}` where the family
    /// substitutes its member's name otherwise.
    pub tag: &'static str,
    /// The role a reader is told this region is — the WAI-ARIA word
    /// `scene/access` publishes, so the two surfaces join on it.
    pub role: &'static str,
    /// Which family the members come from.
    pub population: Population,
    /// ★★★★★ R1695 — **which destination this region belongs to.**
    ///
    /// The table used to describe one screen, because the application had one:
    /// [[debt-the-voice-gate-judges-only-the-opening-screen]] records exactly
    /// that limit, and records it as unclosable because nothing enumerated the
    /// screens an application has. The rail's roster is that enumeration, so
    /// the gate can now walk every open destination and judge what each one
    /// paints — and a page added without a row here fails rather than escaping.
    pub at: Where,
}

/// Which destination a region belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    /// Painted whatever the rail has chosen — the application bar, the rail
    /// itself, the toast.
    Chrome,
    /// Painted only while the journey is at this destination.
    At(&'static str),
}

impl Where {
    /// Whether a region with this standing is on screen at `destination`.
    #[must_use]
    pub fn shows_at(self, destination: &str) -> bool {
        match self {
            Where::Chrome => true,
            Where::At(key) => key == destination,
        }
    }
}

/// Whether a destination shows the **dashboard's own chrome** — the layout bar
/// and the palette, which name a board's preset and populate a board and are
/// meaningless anywhere else.
///
/// One predicate for the painter, the accessibility tree and the census, so the
/// screen cannot build a bar the census says is not there. Both of those used to
/// spell it as a key written into an `if`.
#[must_use]
pub fn shows_board_chrome(destination: &str) -> bool {
    Where::At("dashboard").shows_at(destination)
}

/// Where a [`VoiceSpec`]'s members come from.
///
/// Per-screen and closed on purpose (the R1693 finding): a screen naming a
/// family it does not have should not compile, and the thing that has to
/// generalise is the *expander*, which is a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Population {
    /// Exactly one region, at [`VoiceSpec::tag`] verbatim.
    One,
    /// One per [`RAIL`] seat, keyed by its key.
    Rail,
    /// One per [`CATALOGUE`] entry, keyed by its kind.
    Catalogue,
    /// One per [`SECTIONS`] heading, keyed by its key.
    Sections,
    /// One per placed card, keyed by its [`card_ids`] identifier.
    Cards,
    /// ★ One per (card, control) **pair** — the product of [`card_ids`] and
    /// [`CARD_CHROME`], substituted as `{id}.{control}`.
    CardChrome,
    /// One per card whose body is a table: the header strip each of them paints.
    TableHeads,
    /// One per [`STREAM_COLUMNS`] entry.
    StreamColumns,
    /// One per [`STREAM_ROWS`] message.
    StreamRows,
    /// ★ One per (message, column) pair.
    StreamCells,
    /// One per [`DECODE_ROWS`] entry.
    DecodeRows,
    /// One per line of the byte pane: [`DECODE_BYTES`] rows.
    ByteRows,
    /// ★ One per **byte of the frame** — a range whose length is four times the
    /// line count rather than an index into any table of rows.
    Bytes,
    /// One per [`MAP_COLUMNS`] entry.
    MapColumns,
    /// One per [`MAP_ROWS`] row.
    MapRows,
    /// ★ One per (identifier, column) pair.
    MapCells,
    /// One per [`FILTER_CHIPS`] saved filter, keyed by index.
    Chips,
    /// One per [`FILTER_STATS`] count, keyed by index.
    Stats,
    /// ★ One per catalogue entry the first release **reserves** — a predicate
    /// over [`CATALOGUE`] rather than the whole of it, so the gate demands
    /// exactly the nine locked seats and not thirteen.
    Reserved,
    /// ★ One per rail seat this application **cannot take you to** — booked for
    /// a later release or built on another surface. A predicate over [`RAIL`],
    /// so a seat that opens leaves this family by opening.
    ReservedRail,
    /// R1695 — one per [`OPTION_GROUPS`] heading on the Settings page.
    OptionGroups,
    /// R1695 — one per [`OPTIONS`] switch, keyed by its key.
    Options,
    /// ★ R1762 — one per [`VALUE_ROWS`] row, keyed by its key.
    ValueRows,
    /// R1695 — one per [`KEY_ROWS`] row, keyed by its key.
    KeyRows,
    /// R1695 — one per [`THEMES`] choice, keyed by index.
    Themes,
    /// ★ R1733 — one per catalogue entry the first release **places**: the
    /// mirror of [`Reserved`](Self::Reserved), and the rows whose parts are
    /// addressable. A reserved row's parts are not tagged at all, for the two
    /// reasons the painter states, so a family over the whole catalogue would
    /// demand nine regions that are not there.
    Placeable,
}

impl Population {
    /// The members this family expands to, as the strings a tag substitutes.
    ///
    /// The expander lives beside the arms rather than in the gate, because two
    /// of this screen's families are **computed** — a product and a range — and
    /// a gate holding the rule would be the place a screen's own shape got
    /// re-derived by whoever wrote the gate.
    #[must_use]
    pub fn members(self) -> Vec<String> {
        let indexes = |n: usize| (0..n).map(|i| i.to_string()).collect::<Vec<_>>();
        let under = |kind: &str, suffix: &str, n: usize| {
            card_of(kind).map_or_else(Vec::new, |id| {
                (0..n).map(|i| format!("{id}.{suffix}.{i}")).collect()
            })
        };
        match self {
            Population::One => vec![String::new()],
            Population::Rail => RAIL.iter().map(|seat| seat.key.to_owned()).collect(),
            Population::Catalogue => CATALOGUE.iter().map(|w| w.kind.to_owned()).collect(),
            Population::Sections => SECTIONS
                .iter()
                .map(|(key, _, _)| (*key).to_owned())
                .collect(),
            Population::Cards => card_ids(),
            Population::CardChrome => card_ids()
                .into_iter()
                .flat_map(|id| CARD_CHROME.iter().map(move |c| format!("{id}.{c}")))
                .collect(),
            Population::TableHeads => TABLE_CARDS
                .iter()
                .filter_map(|kind| card_of(kind))
                .map(|id| format!("{id}.head"))
                .collect(),
            Population::StreamColumns => under("packet", "head", STREAM_COLUMNS.len()),
            Population::StreamRows => under("packet", "row", STREAM_ROWS.len()),
            Population::StreamCells => {
                cell_members("packet", STREAM_ROWS.len(), STREAM_COLUMNS.len())
            }
            Population::DecodeRows => under("decode", "tree", DECODE_ROWS.len()),
            Population::ByteRows => under("decode", "bytes", DECODE_BYTES.len()),
            Population::Bytes => under("decode", "byte", DECODE_BYTES.len() * 4),
            Population::MapColumns => under("keymap", "head", MAP_COLUMNS.len()),
            Population::MapRows => under("keymap", "map", MAP_ROWS.len()),
            Population::MapCells => cell_members("keymap", MAP_ROWS.len(), MAP_COLUMNS.len()),
            Population::Chips => indexes(FILTER_CHIPS.len()),
            Population::Stats => indexes(FILTER_STATS.len()),
            Population::Reserved => CATALOGUE
                .iter()
                .filter(|w| w.tier == Tier::Reserved)
                .map(|w| w.kind.to_owned())
                .collect(),
            Population::ReservedRail => RAIL
                .iter()
                .filter(|seat| !matches!(seat.seat, Seat::Page))
                .map(|seat| seat.key.to_owned())
                .collect(),
            Population::OptionGroups => OPTION_GROUPS
                .iter()
                .map(|(key, _)| (*key).to_owned())
                .collect(),
            Population::Options => OPTIONS.iter().map(|o| o.key.to_owned()).collect(),
            Population::ValueRows => VALUE_ROWS.iter().map(|r| r.key.to_owned()).collect(),
            Population::KeyRows => KEY_ROWS.iter().map(|r| r.key.to_owned()).collect(),
            Population::Themes => indexes(THEMES.len()),
            Population::Placeable => CATALOGUE
                .iter()
                .filter(|w| w.tier == Tier::Placeable)
                .map(|w| w.kind.to_owned())
                .collect(),
        }
    }
}

/// ★★★★★ Every region the opening screen declares **unavailable**, and where a
/// reader finds it.
///
/// Eleven of them: nine catalogue entries booked for a later release, and the
/// two rail destinations booked the same way. This is the screen's entire
/// claim — *"a second-release item is not a missing thing, it is a locked
/// seat"* — and until R1694 not one of the eleven was in the accessibility tree
/// at all, so the kind, the detail and the recourse the framework computes for
/// each of them reached nobody.
///
/// Derived from the tier and the reservation rather than listed, so a seat that
/// is unlocked leaves this table by being unlocked.
/// ★ R1695 — sixteen now, not eleven, and the five that joined are the point of
/// the round: three rail seats this application cannot take you to were painted
/// as ordinary destinations, and the Settings page has two affordances of its
/// own that are booked for a later release.
pub const LOCKED: &[(&str, Population, Where)] = &[
    ("shell.rail.{}", Population::ReservedRail, Where::Chrome),
    (
        "shell.palette.{}",
        Population::Reserved,
        Where::At("dashboard"),
    ),
    (
        "shell.settings.key.{}",
        Population::KeyRows,
        Where::At("settings"),
    ),
];

/// The catalogue kinds whose card body is a table with a header strip.
pub const TABLE_CARDS: &[&str] = &["packet", "keymap"];

/// The card identifier a kind is placed under, or `None` when this layout does
/// not place it.
#[must_use]
pub fn card_of(kind: &str) -> Option<String> {
    BOARD
        .iter()
        .position(|placed| placed.kind == kind)
        .map(|n| format!("{kind}#{n}"))
}

/// `{card}.cell.{row}_{column}` for every cell of a placed table card.
fn cell_members(kind: &str, rows: usize, columns: usize) -> Vec<String> {
    card_of(kind).map_or_else(Vec::new, |id| {
        (0..rows)
            .flat_map(|r| {
                let id = id.clone();
                (0..columns).map(move |c| format!("{id}.cell.{r}_{c}"))
            })
            .collect()
    })
}

/// Every region of the opening screen that owes a reader a voice, and what it
/// announces as.
pub const VOICES: &[VoiceSpec] = &[
    VoiceSpec {
        tag: "analyzer_shell",
        role: "group",
        population: Population::One,
        at: Where::Chrome,
    },
    // --- the application bar --------------------------------------------
    VoiceSpec {
        tag: "shell.appbar",
        role: "toolbar",
        population: Population::One,
        at: Where::Chrome,
    },
    VoiceSpec {
        tag: "shell.appbar.tab.dashboard",
        role: "tab",
        population: Population::One,
        at: Where::Chrome,
    },
    VoiceSpec {
        tag: "shell.appbar.tab.design",
        role: "tab",
        population: Population::One,
        at: Where::Chrome,
    },
    VoiceSpec {
        tag: "shell.appbar.source",
        role: "button",
        population: Population::One,
        at: Where::Chrome,
    },
    // The rate readout changes while nobody touches it, which is what a live
    // region is for — and the only one of the bar's regions that is.
    VoiceSpec {
        tag: "shell.appbar.capture",
        role: "status",
        population: Population::One,
        at: Where::Chrome,
    },
    VoiceSpec {
        tag: "shell.appbar.search",
        role: "textbox",
        population: Population::One,
        at: Where::Chrome,
    },
    // --- the rail -------------------------------------------------------
    VoiceSpec {
        tag: "shell.rail",
        role: "navigation",
        population: Population::One,
        at: Where::Chrome,
    },
    VoiceSpec {
        tag: "shell.rail.{}",
        role: "link",
        population: Population::Rail,
        at: Where::Chrome,
    },
    // ★ R1699 — a `group`, not a `button`. Nothing presses this seat from either
    // channel and the reference's avatar has no handler at all, so announcing an
    // action it does not have was a claim the round's own gate refused.
    VoiceSpec {
        tag: "shell.rail.account",
        role: "group",
        population: Population::One,
        at: Where::Chrome,
    },
    // --- the layout bar -------------------------------------------------
    //
    // ★ R1695 — the layout bar and the palette below are the DASHBOARD's, not
    // the window's: they name a board's preset and populate a board, and both
    // are meaningless at any other destination. They are built outside
    // [`view_page_region`](pinion_widget_paint::pages::view_page_region), so
    // the substrate's "only the current page is built" guarantee does not cover
    // them — this column is what does, in both directions.
    VoiceSpec {
        tag: "shell.subbar",
        role: "toolbar",
        population: Population::One,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "shell.subbar.preset",
        role: "button",
        population: Population::One,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "shell.subbar.edit",
        role: "button",
        population: Population::One,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "shell.subbar.add",
        role: "button",
        population: Population::One,
        at: Where::At("dashboard"),
    },
    // --- the page region ------------------------------------------------
    //
    // ★★ R1695 — a `region` landmark rather than a group, named for the
    // destination inside it, so a reader who jumps to it is told which one
    // arrived. It is chrome because the rectangle is always there; what is in
    // it is not.
    VoiceSpec {
        tag: "shell.canvas",
        role: "region",
        population: Population::One,
        at: Where::Chrome,
    },
    VoiceSpec {
        tag: "card.{}",
        role: "group",
        population: Population::Cards,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.{}",
        role: "button",
        population: Population::CardChrome,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.{}.grip",
        role: "button",
        population: Population::Cards,
        at: Where::At("dashboard"),
    },
    // --- the message stream ---------------------------------------------
    VoiceSpec {
        tag: "card.{}",
        role: "row",
        population: Population::TableHeads,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.{}",
        role: "columnheader",
        population: Population::StreamColumns,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.{}",
        role: "row",
        population: Population::StreamRows,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.{}",
        role: "gridcell",
        population: Population::StreamCells,
        at: Where::At("dashboard"),
    },
    // --- the decode inspector -------------------------------------------
    VoiceSpec {
        tag: "card.{}",
        role: "treeitem",
        population: Population::DecodeRows,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.{}",
        role: "row",
        population: Population::ByteRows,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.{}",
        role: "gridcell",
        population: Population::Bytes,
        at: Where::At("dashboard"),
    },
    // --- the identifier map ---------------------------------------------
    VoiceSpec {
        tag: "card.{}",
        role: "columnheader",
        population: Population::MapColumns,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.{}",
        role: "row",
        population: Population::MapRows,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.{}",
        role: "gridcell",
        population: Population::MapCells,
        at: Where::At("dashboard"),
    },
    // --- search and filter ----------------------------------------------
    VoiceSpec {
        tag: "card.filter#3.query",
        role: "textbox",
        population: Population::One,
        at: Where::At("dashboard"),
    },
    // ★★★★★ R1721 — the bar and its chips announce what the RULE says they are.
    // Both were the word `button`, typed here, over a set that can never have two
    // on; now they are what `chip_group_nodes` actually builds, read from the same
    // declaration, so this census cannot check one person's typing against
    // another's.
    VoiceSpec {
        tag: "card.filter#3.chips",
        role: pinion_a11y::group_role(FILTER_ROW).aria_name(),
        population: Population::One,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.filter#3.chip.{}",
        role: pinion_a11y::member_role(FILTER_ROW).aria_name(),
        population: Population::Chips,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.filter#3.stat.{}",
        role: "status",
        population: Population::Stats,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.filter#3.sparkline",
        role: "group",
        population: Population::One,
        at: Where::At("dashboard"),
    },
    // --- the palette ----------------------------------------------------
    VoiceSpec {
        tag: "shell.palette",
        role: "list",
        population: Population::One,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "shell.palette.section.{}",
        role: "group",
        population: Population::Sections,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "shell.palette.{}",
        role: "listitem",
        population: Population::Catalogue,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "shell.palette.placed",
        role: "status",
        population: Population::One,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "shell.palette.reserved",
        role: "status",
        population: Population::One,
        at: Where::At("dashboard"),
    },
    // --- the settings destination ----------------------------------------
    //
    // ★★ R1695 — the second page, and the rows this table would never have
    // held before it had a destination column: they are not on the opening
    // screen, so every census this screen has ever run was blind to them.
    VoiceSpec {
        tag: "shell.settings.group.{}",
        role: "group",
        population: Population::OptionGroups,
        at: Where::At("settings"),
    },
    VoiceSpec {
        tag: "shell.settings.option.{}",
        role: "switch",
        population: Population::Options,
        at: Where::At("settings"),
    },
    VoiceSpec {
        tag: "shell.settings.key.{}",
        role: "button",
        population: Population::KeyRows,
        at: Where::At("settings"),
    },
    // ★★★★★ R1762 — the capture group's two value rows. A `combobox` is the
    // role that says *there is a roster behind this*, and the screen carries
    // `expanded` beside it so a reader is told whether the roster is in front
    // of them — the pair the floor's own collapsed control cannot publish
    // without a platform layer adding one.
    VoiceSpec {
        tag: "shell.settings.choose.{}",
        role: "combobox",
        population: Population::ValueRows,
        at: Where::At("settings"),
    },
    VoiceSpec {
        tag: "shell.settings.theme",
        role: "radiogroup",
        population: Population::One,
        at: Where::At("settings"),
    },
    VoiceSpec {
        tag: "shell.settings.theme.{}",
        role: "radio",
        population: Population::Themes,
        at: Where::At("settings"),
    },
    // --- what just happened ---------------------------------------------
    VoiceSpec {
        tag: "shell.toast",
        role: "status",
        population: Population::One,
        at: Where::Chrome,
    },
];

/// Every region of the opening screen that owes a reader **silence**, and why.
///
/// A total census is satisfied by declaring everything silent, so this table is
/// the other half of the split and is what makes the first one a claim.
pub const SILENCES: &[(&str, Population, &str, Where)] = &[
    // ★★★★★ R1761 — three marks that became ADDRESSABLE so a specification
    // could name them, and are silent because each one's words are already in
    // an announcement a reader gets. Being tagged and being announced are two
    // decisions, and this table is where the second one is made: a mark that
    // gained an address without one would be a second voice for a fact the
    // screen already says, which is exactly what a reader experiences as
    // repetition.
    (
        "shell.subbar.count",
        Population::One,
        "part_of",
        Where::At("dashboard"),
    ),
    // ★★★★★ R1762 — a collapsed chooser's two inner parts. Both are painted,
    // addressable and pressable, so the census asks about them — and the honest
    // answer for both is that their content is already in the chooser's own
    // announcement: the word IS its value, and the arrow draws the same
    // open/closed state `expanded` carries.
    (
        "shell.settings.shown.{}",
        Population::ValueRows,
        "part_of",
        Where::At("settings"),
    ),
    (
        "shell.settings.arrow.{}",
        Population::ValueRows,
        "part_of",
        Where::At("settings"),
    ),
    (
        "shell.palette.head.title",
        Population::One,
        "name_of",
        Where::At("dashboard"),
    ),
    (
        "shell.palette.head.hint",
        Population::One,
        "part_of",
        Where::At("dashboard"),
    ),
    // The scrolling viewport is a clip, not a thing on the screen: what a
    // reader walks is the board inside it.
    (
        "shell.canvas.body",
        Population::One,
        "layout",
        Where::At("dashboard"),
    ),
    // The plot area and its stroke. The card's sparkline region is the thing a
    // reader is told about and it states the series; these are how it is drawn.
    (
        "match.spark",
        Population::One,
        "part_of",
        Where::At("dashboard"),
    ),
    (
        "match.spark.line",
        Population::One,
        "decorative",
        Where::At("dashboard"),
    ),
    // ★★ R1733 — a palette row's four parts. The ROW is the control: pressing
    // anywhere on it adds, and it is the thing a reader arrives at. Its parts
    // are how it is drawn, so each says which of the four kinds of quiet it
    // owes — the swatch is decoration, the name IS the row's name, and the
    // line and the seat belong to it.
    //
    // Only the rows a widget can be picked up FROM: a reserved row's parts
    // carry no tag at all (`part_tag_of`), so a family over the whole
    // catalogue would demand nine regions that are not painted.
    (
        "shell.palette.part.swatch.{}",
        Population::Placeable,
        "decorative",
        Where::At("dashboard"),
    ),
    (
        "shell.palette.part.name.{}",
        Population::Placeable,
        "name_of",
        Where::At("dashboard"),
    ),
    (
        "shell.palette.part.gist.{}",
        Population::Placeable,
        "part_of",
        Where::At("dashboard"),
    ),
    (
        "shell.palette.part.verb.{}",
        Population::Placeable,
        "part_of",
        Where::At("dashboard"),
    ),
    // R1695 — a settings row's title and its sentence under it are what the
    // control beside them is NAMED by, so announcing them again would say
    // everything twice. The silence names that control's **tag**: a `part_of`
    // holding prose points at nothing, and the census counts that as
    // `dangling` — which is how the first draft of this page was caught, with
    // seven of them.
    (
        "shell.settings.row.{}",
        Population::Options,
        "part_of",
        Where::At("settings"),
    ),
    (
        "shell.settings.row.{}",
        Population::KeyRows,
        "part_of",
        Where::At("settings"),
    ),
    (
        "shell.settings.row.theme",
        Population::One,
        "part_of",
        Where::At("settings"),
    ),
];
