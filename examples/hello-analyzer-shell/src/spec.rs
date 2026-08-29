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
pub use pinion_core::widgets::severity::SeverityScale;

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

/// One arrangement this application SHIPS: a name and the board it restores.
///
/// ★★★★★ R1894 — the behaviour canon offers FOUR of these before a person has
/// saved anything (`builtinPresets()`), each a different subject: everything at
/// once, latency, traffic, and the topology. This shell shipped ONE, so the
/// provenance axis R1893 built had a population of one built-in and could not
/// be exercised on a set.
pub struct ArrangementSpec {
    /// What the menu calls it.
    pub name: &'static str,
    /// The board it restores, in the same form as [`BOARD`].
    pub board: &'static [PlacedSpec],
}

/// The arrangements this application ships BESIDES the opening one.
///
/// Boards taken from the behaviour canon's own `builtinPresets()` verbatim —
/// the same kinds at the same cells with the same spans — because a layout
/// invented here would reproduce the canon's *idea* of subject views while
/// reproducing none of its arrangements.
///
/// ⚠ The names are English where the canon mixes one Korean word into
/// otherwise-English names. This screen's every other string is English and the
/// mixed form would be the only one; the SUBJECT is what the canon's name
/// carries and it survives the substitution.
///
/// ⚠⚠ The opening board is deliberately NOT the canon's `Overview`. This
/// application's catalogue is not the canon's, and `BOARD`'s own doc explains
/// why it opens with what it opens with. Reproducing the canon's other three
/// exactly while keeping this one is the honest split: those three are
/// arrangements OF widgets both applications have, and the opening board is a
/// statement about this catalogue.
pub const ARRANGEMENTS: &[ArrangementSpec] = &[
    ArrangementSpec {
        name: "Latency",
        board: &[
            PlacedSpec {
                kind: "latency",
                col: 0,
                row: 0,
                cols: 6,
                rows: 2,
            },
            PlacedSpec {
                kind: "loss",
                col: 6,
                row: 0,
                cols: 6,
                rows: 1,
            },
            PlacedSpec {
                kind: "throughput",
                col: 6,
                row: 1,
                cols: 6,
                rows: 2,
            },
            PlacedSpec {
                kind: "alarms",
                col: 0,
                row: 2,
                cols: 6,
                rows: 2,
            },
            PlacedSpec {
                kind: "health",
                col: 0,
                row: 4,
                cols: 12,
                rows: 1,
            },
        ],
    },
    ArrangementSpec {
        name: "Traffic",
        board: &[
            PlacedSpec {
                kind: "throughput",
                col: 0,
                row: 0,
                cols: 7,
                rows: 2,
            },
            PlacedSpec {
                kind: "share",
                col: 7,
                row: 0,
                cols: 5,
                rows: 2,
            },
            PlacedSpec {
                kind: "packet",
                col: 0,
                row: 2,
                cols: 7,
                rows: 2,
            },
            PlacedSpec {
                kind: "health",
                col: 7,
                row: 2,
                cols: 5,
                rows: 2,
            },
        ],
    },
    ArrangementSpec {
        name: "Topology focus",
        board: &[
            PlacedSpec {
                kind: "topology",
                col: 0,
                row: 0,
                cols: 8,
                rows: 3,
            },
            PlacedSpec {
                kind: "health",
                col: 8,
                row: 0,
                cols: 4,
                rows: 1,
            },
            PlacedSpec {
                kind: "alarms",
                col: 8,
                row: 1,
                cols: 4,
                rows: 2,
            },
            PlacedSpec {
                kind: "packet",
                col: 0,
                row: 3,
                cols: 8,
                rows: 2,
            },
        ],
    },
];

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
/// ★★★★★ R1797 — the tier column is **gone**, and that is the round's find.
///
/// It used to read `(key, title, Tier)`, and the gate next door asserted that
/// every entry in a section carried the same tier as the section did. That is
/// the same fact written twice, held in agreement by a test — which is exactly
/// what [`section_heading`]'s own comment argues against one paragraph below:
/// *"a marker written beside the tier is a second copy of it"*. The marker was
/// derived and the tier itself was not.
///
/// It surfaced because R1797 promoted one entry. The promotion was legal, the
/// screen was right, and a gate failed anyway — because a section could not
/// hold a promoted widget beside its unpromoted siblings without one of the two
/// copies becoming false. A section's release is now **read off its entries**
/// ([`section_tiers`]), so promoting one entry cannot make anything disagree
/// and a mixed section says so instead of being unrepresentable.
pub const SECTIONS: &[(&str, &str)] = &[
    ("capture", "CAPTURE & DECODE"),
    ("visual", "VISUALIZATION"),
    ("operate", "DIAGNOSE & OPERATE"),
];

/// Which tiers a section's entries actually occupy, as `(has_placeable,
/// has_reserved)`.
///
/// A section with no entries at all answers `(false, false)`, which
/// [`section_heading`] renders as no release clause rather than as a wrong one.
#[must_use]
pub fn section_tiers(section: &str) -> (bool, bool) {
    CATALOGUE.iter().filter(|w| w.section == section).fold(
        (false, false),
        |(placeable, reserved), w| match w.tier {
            Tier::Placeable => (true, reserved),
            Tier::Reserved => (placeable, true),
        },
    )
}

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
/// **Derived from the entries rather than typed into [`SECTIONS`]**, because a
/// marker written beside the tier is a second copy of it: a section retiered
/// with its heading left alone would say one thing and offer another, and
/// nothing would notice. This way the heading cannot disagree with what the
/// rows do.
///
/// ★ R1797 — it now derives from the ENTRIES rather than from a tier column on
/// the section, which was itself the second copy this comment warns about. A
/// section holding both releases says so: promoting one widget out of a group
/// is a thing the release plan does, and a heading that could only name one
/// release made it unrepresentable.
#[must_use]
pub fn section_heading(section: &str, title: &str) -> String {
    let release = match section_tiers(section) {
        (true, true) => "RELEASE 1 + 2",
        (true, false) => "RELEASE 1",
        (false, true) => "RELEASE 2",
        // A section with no entries has no release to name. It cannot arise
        // from `CATALOGUE` as it stands and the gate next door says so; the
        // heading is answered rather than unwrapped because a panel that
        // panicked over an empty group would be a worse failure than a plain
        // title.
        (false, false) => return title.to_owned(),
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
    // ★ R1797 — promoted from `Tier::Reserved`. The reference books this seat
    // under its second release, and it stayed booked here for as long as the
    // framework could not draw what sits in it: a latency distribution needs
    // geometric buckets with an unbounded tail, and until this round the
    // histogram had neither. Promoting a seat is a change to the RELEASE
    // structure the reference defines, so it was asked rather than assumed.
    WidgetSpec {
        kind: "latency",
        code: "LAT",
        label: "Latency",
        gist: "request to reply round trip",
        section: "visual",
        tier: Tier::Placeable,
        reserved_for: "",
    },
    // ★ R1843 — promoted from `Tier::Reserved`, on the R1797 pattern. The
    // reference books this seat under its second release and it stayed booked
    // here for as long as the framework could not draw what sits in it: a KPI
    // tile is a box, a label, a value and a sparkline, and until
    // `pinion_widget_paint::stat_tile` no crate had a tile at all — only two
    // examples that had each hand-rolled one. What promotes it is the
    // reference's own board, which places this seat by default and in four of
    // its presets, and its catalogue entry naming the sparkline no other widget
    // of that catalogue has. Promoting a seat changes the RELEASE structure the
    // reference defines, so it is declared in the deferred register's `built`
    // list rather than made to look like a seat that was never locked.
    WidgetSpec {
        kind: "health",
        code: "HLT",
        label: "Health Tiles",
        gist: "session and error summary",
        section: "operate",
        tier: Tier::Placeable,
        reserved_for: "",
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
    // ★ R1851 — promoted from `Tier::Reserved`, on the R1797 / R1843 pattern,
    // and the evidence is the reference's OWN board rather than a preference.
    // Measured in the behaviour prototype: its opening board places this seat
    // (at column 4 of the fifth row, four columns by two rows), three of its
    // four built-in presets place it, and its catalogue entry declares the
    // footprint `4 x 2` this board now gives it. What it was waiting for was a
    // framework that could draw the thing: a feed is a virtualised list under a
    // sortable column header, and until `pinion_widget_paint::header_feed`
    // nothing in this tree composed those two — measured the same round, the
    // only surface holding both was the data grid, where every row is a row of
    // cells and a severity swatch beside two lines of text is not.
    //
    // Promoting a seat changes the RELEASE structure the reference defines, so
    // it is declared in the deferred register's `built` list rather than made
    // to look like a seat that was never locked.
    WidgetSpec {
        kind: "alarms",
        code: "ALM",
        label: "Alarms",
        gist: "highlight rules by severity",
        section: "operate",
        tier: Tier::Placeable,
        reserved_for: "",
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
        cols: 4,
        rows: 2,
    },
    PlacedSpec {
        kind: "decode",
        col: 4,
        row: 0,
        cols: 4,
        rows: 2,
    },
    // ★★★★★ R1797 — the second row's three, and the width change is MEASURED
    // rather than preferred.
    //
    // The reference's opening board puts latency at column 0 of the FIFTH row,
    // four columns wide and two rows tall, and that is where this round put it
    // first. Then the paint census refused it, correctly: `cell_rect` places
    // row 4 at `GAP + 4 * ROW_H` = 712, the canvas is `WIN_H - APP_BAR_H -
    // SUB_BAR_H` = 802 tall, and the card is 332 deep — so its header and its
    // three tiles were on screen and its DISTRIBUTION, the whole point of the
    // card, began exactly at the fold.
    //
    // The reference's first release fills its viewport exactly: four cards,
    // four rows, 16 + 4 × 174 = 712 in 802. A fifth card does not fit by
    // placement, only by reflow. So the two cards of the second row give up two
    // columns each and the promoted card takes the third slot beside them —
    // the reference's ROW assignment for all three is preserved, and every card
    // stays whole and visible. Recorded as a remainder in
    // `analyzer-dashboard-spec.json` rather than passed off as the reference's.
    // ★★★★★ R1843 — the second band re-cut so a SIXTH card fits, and which
    // card gave up its second row was MEASURED rather than chosen.
    //
    // The board is 12 columns by 4 rows and was exactly full: 7x2 + 5x2 + three
    // 4x2 is 48 of 48 cells. A fifth row does not exist — `cell_rect` puts row
    // `n`'s bottom at `GAP + n * ROW_H`, so four rows end at 712 in an 802-tall
    // canvas and five would end at 886. A sixth card can therefore only come
    // from cells another card gives up, and there are only 48.
    //
    // The first cut here took the row from `filter`, and `r1824` refused it by
    // name — *the filter card paints its trend*, which it cannot do in one row.
    // `latency` was already known to need two (R1797 measured its distribution
    // at 332px). So the row comes from `keymap`, which is the one card of the
    // band that no gate objects to at a single row.
    //
    // ⚠ That leaves the strip FOUR columns, not the six the reference's default
    // board gives it — and this is reference-faithful rather than a compromise
    // against it: the reference draws this same seat at `w:4` in one of its own
    // presets. What four columns costs is tiles, and the body says so by
    // painting the ones that fit instead of pretending to five.
    // ⚠⚠ ORDER IS IDENTITY HERE. A card's id is `kind#n` where `n` is its INDEX
    // in this array, so inserting a placement in the middle RENAMES every card
    // after it. R1843's first cut put `health` third and renumbered `keymap#2`
    // to `#3` and `filter#3` to `#4` — which is what six of that run's failures
    // were, every one of them reading as a card that vanished: a cursor resting
    // where nothing paints, a region announced and unpainted, a specification
    // naming a part the surface no longer had. A new placement goes LAST.
    // ★★★★★ R1851 — a SEVENTH card fits, and its footprint is not a preference:
    // it is what is left after COUNTING, and the count is entirely made of
    // measurements other rounds already paid for.
    //
    // The board is 12 columns by 4 rows = 48 cells and six `4 x 2` cards filled
    // it exactly. Three ways out, and two are closed by existing gates:
    //
    //   * a fifth row — `cell_rect` ends row `n` at `GAP + n * ROW_H`, so four
    //     rows end at 712 in an 802-tall canvas and a card on row 4 begins at
    //     the fold. R1797 TRIED that placement and the paint census refused it
    //     by name, correctly: the card's whole point began where nobody could
    //     see it.
    //   * a narrower card — three columns is `3 * 90 - 16 = 254px` against the
    //     320px a card clamps to when torn off, and `r1843` refuses it by name.
    //   * cells another card gives up. Which leaves counting WHICH cards can.
    //
    // Every one of the six has already been measured on exactly that question:
    //   packet / decode  — eight stream rows and a decode tree; neither fits
    //   keymap           — R1669: at one row its body is 140px and its header
    //                      plus seven rows need 144. Measured, named, refused
    //   filter           — `r1824` refuses it by name: *the filter card paints
    //                      its trend*, which it cannot do in one row
    //   latency          — R1797 measured its distribution at 332px
    //   health           — the behaviour prototype's own catalogue declares this
    //                      seat `6 x 1`, so ONE row is its reference footprint
    //                      rather than a concession
    //
    // So five cards need two rows (40 cells), `health` needs one (4), and what
    // is left is FOUR CELLS. The alarm card is `4 x 1` because that is the only
    // thing it can be — and the feed's row is shaped to that in `ALARM_ROW_H`,
    // which is where the consequence is stated rather than hidden.
    //
    // ⚠ The prototype's catalogue declares this seat `4 x 2` and its own board
    // is six rows deep. We reproduce the seat, not its depth, because the depth
    // is what R1797's refusal is about. Stated, not glossed.
    //
    // ⚠⚠ ORDER IS IDENTITY HERE. A card's id is `kind#n` where `n` is its INDEX
    // in this array, so inserting a placement in the middle RENAMES every card
    // after it. R1843's first cut put `health` third and renumbered two cards,
    // which surfaced as six gates reporting cards that had vanished. A new
    // placement goes LAST — which is why `alarms` is last here even though it
    // sits under `health` on the board.
    PlacedSpec {
        kind: "keymap",
        col: 8,
        row: 0,
        cols: 4,
        rows: 2,
    },
    PlacedSpec {
        kind: "filter",
        col: 0,
        row: 2,
        cols: 4,
        rows: 2,
    },
    PlacedSpec {
        kind: "latency",
        col: 4,
        row: 2,
        cols: 4,
        rows: 2,
    },
    PlacedSpec {
        kind: "health",
        col: 8,
        row: 2,
        cols: 4,
        rows: 1,
    },
    PlacedSpec {
        kind: "alarms",
        col: 8,
        row: 3,
        cols: 4,
        rows: 1,
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
/// ★★★★★ R1819 — **what this screen tells a person the POINTER does**, which
/// until now it never said at all.
///
/// The third and last of the tool's three screens to get one. Screen A prints a
/// hint strip, screen B publishes a list, and this screen published nothing —
/// so the gate over that population ran here over the EMPTY SET and passed,
/// which is indistinguishable from a screen that keeps every promise. That is
/// the debt this closes, and it is the same shape as the defect that opened it:
/// screen A advertised `wheel -> zoom` for its whole life with the wheel dead,
/// invisible because the operation table is a DIFFERENT population.
///
/// ⚠ This is not [`OPERATIONS`]. That table is what the screen can DO and how
/// each is caused; this is what the screen SAYS a drag does. The two overlap
/// and are not the same claim — a screen can perform an operation it never
/// advertises, and advertise one it cannot perform, and only the second is a
/// lie to a person.
///
/// Taken from the behaviour prototype's own board section rather than invented:
/// it binds a drag on a floating panel's chrome, a drag on its corner, a drag
/// on a card's grip, and a drag out of the palette onto the canvas.
/// ★★★★★ R1898 — the last two rows are the board's EDGE, which until this
/// round no gesture crossed in either direction.
///
/// Both are the floor's own gestures rather than this build's invention: its
/// detachable panel is dragged out of its host by the strip a card's grip is,
/// and dragged back in the same way. Where this build differs is the way in —
/// the floor collides "move the loose panel" with "put it back" and separates
/// them by a held modifier key whose flag is private to its drag state, so a
/// reader cannot ask what letting go would do. Here the two gestures are two
/// affordances, and the one that does not dock says so
/// ([`pinion_core::crossing::CrossingPolicy::Stays`]) in a sentence naming the
/// one that does.
pub const GESTURES: &[(&str, &str)] = &[
    ("drag a card by its grip", "moves it on the board"),
    ("drag a detached panel", "moves the panel"),
    ("drag a detached panel's corner", "resizes the panel"),
    ("drag a palette entry to the board", "places that widget"),
    ("drag a card off the board", "detaches it where you let go"),
    (
        "drag a detached panel's re-dock mark onto the board",
        "docks it in the cell under the cursor",
    ),
];

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
    // ★★★★★ R1898 — **the board's edge, crossed by a drag.** The two rows this
    // table could not have held before, because the value that says which side
    // a release lands on did not exist: a card carried off the board answered
    // `Dropped::Abandoned` and nothing happened, and a panel carried onto it
    // slid across and came to rest on top.
    //
    // `verb: None` on both, and that is the asymmetry the column exists to
    // show — the same one "move a card on the board" has carried since R1697.
    // The verbs beside them (`act …,tear_off` and `redock`) are the DEFAULT
    // placements: one takes the host's preferred home, the other appends at the
    // bottom of the board, and neither takes a position. Choosing where is the
    // gesture's, and giving it a verb would mean inventing a wire spelling for
    // a pixel and a cell that no client has asked for.
    OperationSpec {
        name: "drag a card off the board",
        verb: None,
        gesture: true,
        witness: "floats",
        needs: None,
    },
    OperationSpec {
        name: "dock a detached panel where it is dropped",
        verb: None,
        gesture: true,
        witness: "layout",
        needs: Some("drag a card off the board"),
    },
    // ★★★★★ R1891 — **where a detached card lives is an operation**, and the
    // five rows below need it because they are the CANVAS home's gestures.
    //
    // The behaviour canon detaches into a panel drawn over its own page — it is
    // a web prototype and cannot open a window. The development specification
    // asks for the other thing: *tear off -> independent window*. Both are
    // real, so this build offers both and a card carries which one it is in
    // (`pinion_core::detach::DetachHome`). Tearing off takes the host's
    // preferred home, which here is the window; this row is how a reader — or
    // an agent — asks for the canon's form instead.
    //
    // Before this row existed the two were painted AT ONCE, which is the defect
    // it closes: one card, a window and a canvas panel, neither tracking the
    // other.
    OperationSpec {
        name: "put a detached panel on the canvas",
        verb: Some(("detach_home", "packet#0,canvas")),
        gesture: false,
        witness: "floats",
        needs: Some("detach a card"),
    },
    // ★★★★★ The three rows R1697 exists for. All three were absent, all
    // three are one gesture in the reference's own source — its float drag
    // calls its raise before reading the panel's origin — and the person who
    // found the first one found it by opening the window and pulling.
    //
    // ★ R1891 — they need the CANVAS home, not merely a detached card: a
    // window-homed card is dragged and sized by the window manager, and there
    // is no `float.<id>` on this canvas to grab.
    OperationSpec {
        name: "move a detached panel",
        verb: None,
        gesture: true,
        witness: "floats",
        needs: Some("put a detached panel on the canvas"),
    },
    OperationSpec {
        name: "size a detached panel",
        verb: None,
        gesture: true,
        witness: "floats",
        needs: Some("put a detached panel on the canvas"),
    },
    OperationSpec {
        name: "bring a detached panel forward",
        verb: None,
        gesture: true,
        witness: "floats",
        needs: Some("put a detached panel on the canvas"),
    },
    OperationSpec {
        name: "re-dock a detached panel",
        verb: Some(("redock", "packet#0")),
        gesture: true,
        witness: "floating",
        needs: Some("put a detached panel on the canvas"),
    },
    OperationSpec {
        name: "close a detached panel",
        verb: None,
        gesture: true,
        witness: "floating",
        needs: Some("put a detached panel on the canvas"),
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

/// ★★★★★ R1806 — **what each saved filter selects**, as a rule rather than as
/// a name.
///
/// [`FILTER_CHIPS`] holds five names and, until this round, nothing else: a
/// chip could be lit, and being lit meant nothing to any other card on the
/// board. A name a machine cannot evaluate is a caption, so the cross-filter
/// had nowhere to start. One rule per chip, in [`FILTER_CHIPS`] order, and the
/// bijection is asserted rather than trusted.
///
/// These are the demonstration capture's stated semantics, in the same sense
/// [`STREAM_ROWS`] is a stated capture: content this file authors, evaluated
/// against the rows this file also authors. A rule that matches nothing in
/// this capture (`shared memory`) is kept deliberately — an empty result is a
/// real outcome of a filter, and a board that can only be shown narrowing to a
/// non-empty set has not demonstrated the case a reader most needs to trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipRule {
    /// Rows whose resource name mentions the word.
    NameMentions(&'static str),
    /// Rows of exactly this message type (a [`STREAM_TYPES`] word).
    TypeIs(&'static str),
    /// Every row whose resource name does **not** mention the word — the
    /// exclusion form, which is not the negation of the others in general
    /// because it is the only rule that can grow the shown set.
    NameOmits(&'static str),
}

impl ChipRule {
    /// Whether a [`STREAM_ROWS`] row is selected by this rule.
    #[must_use]
    pub fn selects(self, kind: &str, name: &str) -> bool {
        match self {
            Self::NameMentions(word) => name.contains(word),
            Self::TypeIs(want) => kind == want,
            Self::NameOmits(word) => !name.contains(word),
        }
    }
}

/// One rule per [`FILTER_CHIPS`] entry, in that order.
pub const FILTER_CHIP_RULES: &[ChipRule] = &[
    ChipRule::NameMentions("units"),
    ChipRule::NameMentions("shm"),
    ChipRule::NameMentions("fragment"),
    ChipRule::NameOmits("P-03"),
    ChipRule::TypeIs("Declare"),
];

/// The rule the `n`th saved filter applies.
#[must_use]
pub fn chip_rule(n: usize) -> Option<ChipRule> {
    FILTER_CHIP_RULES.get(n).copied()
}

/// ★★★★★ R1806 — **the dashboard's linked views, declared.**
///
/// The census sentence this answers is *click to cross-filter every linked
/// view*, and the word that had no referent anywhere in this tree was
/// **every**. A cross-filter was an imperative call written once per chart, so
/// "every linked view" was whatever set of calls somebody had remembered to
/// write; a card added to the board and forgotten would render unfiltered and
/// nothing would say so.
///
/// This is that set as a value. Each placed card declares which
/// [`Domain`](pinion_chart::Domain)s of selection it can accept, or declares
/// itself inert with the reason it accepts none — and
/// [`LinkGroup::audit`](pinion_chart::LinkGroup::audit) compares the
/// declaration against the cards the board actually painted, in both
/// directions.
///
/// The two inert cards are the reason the *reason* matters. Neither will ever
/// narrow, but they will not narrow for different causes: a decode inspector
/// shows one selected message rather than a view over the population, and a key
/// legend is not capture data at all. Left out of the group they would read as
/// oversights; given an empty domain list they would read as mismatches. Stated
/// inert, each says the true thing about itself.
///
/// The latency card is the interesting refusal: it is a genuine view over the
/// capture and it **does** cross-filter — by
/// [`XRange`](pinion_chart::Domain::XRange), the millisecond window its
/// distribution is drawn in. A saved filter selects by category, and this card
/// has no per-category breakdown of its samples to narrow by, so it is refused
/// with both sides named. That is a fact about the capture's shape, and before
/// this round the only way it could be expressed was for the card to quietly
/// not change.
///
/// # Panics
///
/// Never in a shipped build: the declaration is a literal here and its only
/// failure modes are a duplicate name or an unexplained mute, both of which the
/// crate refuses at construction and both of which this module's tests cover.
#[must_use]
pub fn dashboard_links() -> pinion_chart::LinkGroup {
    use pinion_chart::{Domain, Link, LinkGroup};
    LinkGroup::new([
        Link::new("packet", [Domain::Category, Domain::XRange]),
        Link::new("filter", [Domain::Category]),
        Link::new("latency", [Domain::XRange]),
        Link::inert(
            "decode",
            "a decode of one selected message, not a view over the population",
        ),
        Link::inert("keymap", "a key legend, not capture data"),
        // ★ R1843 — inert, and the reason is the card's own honest limit rather
        // than a claim about what a health summary must be. Its tiles are read
        // over the whole capture window, and their series is a FIXTURE: nothing
        // in this tree accumulates a per-window reading for these quantities,
        // so there is no narrowed population for a cross-filter to recompute
        // them against. `Inert` says "cannot answer this question", which is
        // exactly the state — and it is distinct from "not part of this
        // population", which a silence here would have meant.
        Link::inert(
            "health",
            "a summary over the whole capture window, not a view over the current selection",
        ),
        // ★ R1851 — inert, and the reason is a MEASURED absence rather than a
        // claim about what an alarm feed must be. This feed narrows: it has an
        // ordered severity threshold, which is a stronger narrowing than any
        // category selection on this board. What it cannot do is answer the
        // CROSS-FILTER's question, because the endpoint an alarm concerns lives
        // inside its reading rather than in a field of it, so there is nothing
        // here to match a selection against. That is the same shape R1848 repaired
        // for traffic roles — declare the taxonomy instead of parsing the prose —
        // and it is a round of its own rather than a sentence to gloss.
        Link::inert(
            "alarms",
            "graded by its own severity threshold; the endpoint an alarm concerns is \
             inside its reading rather than a field of it, so a selection has nothing \
             to match",
        ),
    ])
    .expect("the dashboard's link declaration is well formed")
}

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

// --- The latency card (R1797) ------------------------------------------------

/// ★★★★★ R1797 — the reference's latency card, and the reason its numbers are
/// **derived here rather than copied**.
///
/// The reference draws round-trip time as eight buckets under three stat tiles,
/// and it publishes both: the bar counts `120, 340, 520, 410, 180, 70, 24, 8`
/// and the tiles `p50 3.2 ms`, `p95 11.4 ms`, `max 72 ms`. Measured before this
/// card was drawn, **those two halves describe different distributions**. Its
/// own counts total 1,672 samples, of which 1,570 — 93.9% — are at or below 16
/// milliseconds, so the 95th percentile of the bars it draws falls in the
/// `16-32` bucket. Its tile says 11.4, which is in `8-16`.
///
/// That is not a defect worth reproducing, and it is the exact failure the
/// framework work behind this card removes: a mockup states four numbers
/// independently and nothing can notice they disagree, while a card that
/// *derives* all four from one record cannot state a percentile its own bars
/// contradict. So the reference's **structure** is reproduced — the ladder, the
/// three tiles, the caption, the emphasised tail — and its **figures** become
/// the oracle the derivation is checked against.
///
/// [`LATENCY_SAMPLES`] is a capture record chosen so that the derivation lands
/// on the reference's three published landmarks exactly.
pub const LATENCY_LADDER: &[f64] = &[1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0];

/// The round-trip times the card bins, in milliseconds.
///
/// One hundred of them, so that an index is a percent and the profile can be
/// read off the list. Chosen by stating the quantile function at the landmarks
/// the reference names and reading a hundred samples off it — which is why
/// `LATENCY_P50`, `LATENCY_P95` and the maximum come out exactly, and why
/// the modal bucket is the reference's `2-4`.
///
/// ★ The card computes its bins from THESE. It does not start from counts —
/// that is what `hello-histogram-brush` does, and a card that followed it would
/// not exercise the binning at all.
pub const LATENCY_SAMPLES: &[f64] = &[
    0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.1, //
    1.2, 1.2, 1.2, 1.3, 1.3, 1.4, 1.4, 1.5, 1.5, 1.6, //
    1.6, 1.7, 1.7, 1.8, 1.8, 1.9, 1.9, 2.0, 2.0, 2.1, //
    2.1, 2.2, 2.3, 2.3, 2.4, 2.4, 2.5, 2.5, 2.6, 2.6, //
    2.7, 2.7, 2.8, 2.8, 2.9, 3.0, 3.0, 3.1, 3.1, 3.2, //
    3.2, 3.3, 3.4, 3.5, 3.6, 3.7, 3.8, 3.9, 4.0, 4.2, //
    4.3, 4.5, 4.7, 4.8, 5.0, 5.2, 5.3, 5.5, 5.6, 5.8, //
    6.0, 6.1, 6.3, 6.5, 6.6, 6.8, 7.0, 7.1, 7.3, 7.5, //
    7.6, 7.8, 8.0, 8.2, 8.5, 8.8, 9.1, 9.4, 9.7, 9.9, //
    10.2, 10.5, 10.8, 11.1, 11.4, 11.4, 19.8, 27.8, 47.7, 72.0,
];

/// The reference's median tile, in milliseconds — the oracle the derived p50 is
/// checked against.
///
/// ★★ The oracle is the GATE's, not the application's, and `#[cfg(test)]` says
/// so rather than a comment. These four items and `latency_tile` are the
/// reference's published figures; the card never reads them, and shipping them
/// in the binary would put the reference's numbers where the application's own
/// derivation belongs — the exact conflation this card exists to remove. The
/// compiler found it: outside the gates they are dead, and `dead_code` said so.
#[cfg(test)]
pub const LATENCY_P50: f64 = 3.2;

/// The reference's 95th-percentile tile, in milliseconds.
///
/// ★ It is also the **tail cut** the gate checks against: the bars at or above
/// it are the ones the card emphasises. The reference hard-codes which bars are
/// amber by index; the card derives its cut from the samples, so it moves when
/// the capture does, which is a claim a reader can check.
#[cfg(test)]
pub const LATENCY_P95: f64 = 11.4;

/// The reference's maximum tile, in milliseconds.
#[cfg(test)]
pub const LATENCY_MAX: f64 = 72.0;

/// The three stat tiles' keys, in the order the reference lays them out.
pub const LATENCY_STAT_KEYS: &[&str] = &["p50", "p95", "max"];

/// The caption under the bars.
///
/// The reference's three clauses — what is measured, what the buckets are, and
/// what the emphasis means — with the third saying *why* a bar is emphasised
/// rather than only that it is. That clause is the one thing here the reference
/// could not have written: its tail is an index.
pub const LATENCY_CAPTION: &str =
    "request-reply round trip \u{00B7} ms buckets \u{00B7} tail above p95";

/// The unit the tiles are read in.
pub const LATENCY_UNIT: &str = "ms";

/// One KPI tile of the health card: a label, a value in a unit, the change
/// since the previous window, and the series that change is the end of.
///
/// ★ R1843 — the `trend` is what makes this card the census's *KPI stat tiles
/// with sparklines* rather than a row of numbers. The reference gives this
/// widget a sparkline-window setting and gives no other widget one, so the
/// series is the seat's defining property and not decoration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HealthTile {
    /// What is being counted.
    pub label: &'static str,
    /// The latest reading.
    pub value: &'static str,
    /// The unit the reading is in, or `""` for a bare count.
    pub unit: &'static str,
    /// The change since the previous window, signed.
    pub delta: &'static str,
    /// The readings the value is the last of — the sparkline's series.
    pub trend: &'static [f64],
}

/// The health card's five tiles.
///
/// ⚠ These are a FIXTURE, not a derivation. The latency card next door derives
/// every number it draws from one capture record, and this card cannot yet:
/// nothing in this tree accumulates a per-window series for these five
/// quantities. Said here rather than letting a reader infer a measurement from
/// a table that looks like one.
///
/// ★ The labels are SHORT, and that is a measurement rather than a style
/// choice. The strip divides its card between five tiles, so a tile's inner
/// width is about a fifth of the card less its padding; a label wider than that
/// is placed by [`pinion_widget_paint::caption`] with `Fit::Overflows` and the
/// ink gate reports it as a mark outside its box. R1843's first draft wrote
/// "Active sessions" and the gate caught it hanging 37px past its own label
/// box.
/// How many of [`HEALTH_TILES`] the strip actually draws on the opening board.
///
/// ★★★★★ R1846 — **the health strip is the first body whose row count is a
/// function of its WIDTH, and the census had no way to say so.** Every other
/// family here expands from a table: the latency card has three tiles and draws
/// three. This card declares five and draws as many as clear
/// `StatTile::min_width` in the space the board gives it — three at [`WIN_W`] —
/// so a family over the whole table would demand two regions nobody paints, and
/// a family over what is painted would be the paint answering the census.
///
/// ⚠ So this is a NUMBER, and it is here rather than derived because the rule
/// that produces it needs the card's pixel width, which lives in the painter
/// and is not reachable from a `const` table. What makes it honest is the gate:
/// `r1846_the_strip_draws_what_the_census_declares` fails the moment the paint
/// and this disagree, at every size the sweep runs. That is the same shape as
/// this screen's other pinned numbers — a written value whose drift is what a
/// test exists to catch.
///
/// ⇒ Registered as a debt in its own right: a width-dependent family cannot be
/// DERIVED here, and until it can, every narrowing body will need a pin like
/// this one.
pub const HEALTH_TILES_SHOWN: usize = 3;

pub const HEALTH_TILES: &[HealthTile] = &[
    HealthTile {
        label: "Sessions",
        value: "5",
        unit: "",
        delta: "+1",
        trend: &[3.0, 4.0, 4.0, 5.0, 4.0, 5.0, 5.0],
    },
    HealthTile {
        label: "Errors",
        value: "12",
        unit: "",
        delta: "+3",
        trend: &[2.0, 4.0, 3.0, 6.0, 8.0, 9.0, 12.0],
    },
    HealthTile {
        label: "Rate",
        value: "6.4k",
        // ★ Terse because the heading carries it: at the tile widths this card
        // is placed at, "Rate msg/s" hung 12px past its own box and the ink
        // gate said so. The a11y value spells the unit out.
        unit: "/s",
        delta: "+8%",
        trend: &[5.0, 5.5, 6.0, 5.8, 6.2, 6.1, 6.4],
    },
    HealthTile {
        label: "Loss",
        value: "0.04",
        unit: "%",
        delta: "-0.01",
        trend: &[0.08, 0.07, 0.06, 0.05, 0.05, 0.04, 0.04],
    },
    HealthTile {
        // ★ R1843 — the abbreviation is load-bearing, not cosmetic. A tile's
        // floor is its widest heading, so the LONGEST label caps how many tiles
        // the strip can show at ANY width: with "Round trip ms" the fifth tile
        // never appeared, even maximised. `r1669` refuses that by name — a
        // clamp whose unclamped side no swept size reaches is a guard nothing
        // exercises, and deleting it would change nothing. The a11y value
        // spells the quantity out for a reader who needs it.
        label: "RTT",
        value: "3.2",
        unit: "ms",
        delta: "+0.4",
        trend: &[2.6, 2.8, 3.0, 2.9, 3.1, 3.0, 3.2],
    },
];

// ── The alarm feed (R1851) ──────────────────────────────────────────────────

/// The severity vocabulary this tool's alarms are graded on, **least severe
/// first**.
///
/// ★★★★★ One ordered vocabulary rather than three words compared by hand, and
/// the reason is measured on both the reference and the behaviour prototype.
///
/// The prototype's alarm card offers a *minimum severity* control whose options
/// are spelled `info / warn / error` over a feed whose rows are spelled
/// `info / warn / err` — so its most severe setting could never have matched a
/// row, and nothing in it could say so. ⚠ And the control is never READ: the key
/// occurs exactly once in the prototype's whole script, in the declaration that
/// offers it (`grep -c minSev` over the extracted app logic answers 1), so the
/// mismatch had no way of ever surfacing. Probed on the toolkit floor at 6.11.1,
/// the same shape is what that toolkit's row filtering IS: a predicate over a
/// string, which answers *zero of six* for the word `error` over rows spelled
/// `err` and says nothing. `at least this severe` has to be written there as a
/// pattern enumerating the words by hand, and one word misspelled inside it
/// silently drops rows.
///
/// [`SeverityScale`] refuses a word it does not hold, by name, and carries the
/// vocabulary in the refusal. That is the whole difference: an unspellable
/// threshold is an error here and an empty feed there.
pub const SEVERITY: SeverityScale = SeverityScale::new(&["info", "warn", "error"]);

/// One alarm.
///
/// ★ The INSTANT is the stored fact and the clock reading is derived from it
/// ([`AlarmSpec::clock`]). The prototype stores only the rendered string, which
/// means its feed cannot be ordered by time without trusting that a lexical
/// comparison of `HH:MM:SS` happens to be chronological — true for its six rows
/// and luck rather than a guarantee. One fact, so the reading and the order
/// cannot disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlarmSpec {
    /// When it was seen, as `(hour, minute, second)` of the capture's day.
    pub at: (u32, u32, u32),
    /// How bad it is — a word of [`SEVERITY`], which is checked.
    pub severity: &'static str,
    /// The one-line reading.
    pub message: &'static str,
}

impl AlarmSpec {
    /// The instant, as seconds into the day — the sort key.
    #[must_use]
    pub const fn seconds(&self) -> u32 {
        let (h, m, s) = self.at;
        h * 3600 + m * 60 + s
    }

    /// The clock reading a reader sees, derived from [`seconds`](Self::seconds)'
    /// own inputs.
    #[must_use]
    pub fn clock(&self) -> String {
        let (h, m, s) = self.at;
        format!("{h:02}:{m:02}:{s:02}")
    }
}

/// How many of [`ALARMS`] the behaviour prototype's own feed publishes.
///
/// The first this many rows of the table below ARE that feed, in its order,
/// neutrally worded. The rest are this build's, and the reason they exist is
/// stated at [`ALARMS`].
pub const ALARMS_IN_REFERENCE: usize = 6;

/// The alarm feed's rows, newest first as the prototype writes them.
///
/// ⚠ A FIXTURE, not a derivation — like [`HEALTH_TILES`] and unlike the latency
/// card, which derives every number it draws from one capture record. Nothing in
/// this tree yet raises an alarm from the traffic it holds. Said here rather
/// than letting a reader infer a measurement from a table that looks like one.
///
/// ⚠⚠ **Longer than the prototype's six, deliberately.** A feed that fits
/// inside its own viewport is not a feed, and *the invisible row was not
/// constructed* is not a claim anyone can make about six rows in a body six rows
/// tall. The prototype is a first-prototype mockup and its six are a sample; the
/// first [`ALARMS_IN_REFERENCE`] rows here reproduce them exactly, in their
/// order, and the rest continue the same session backwards so the virtualisation
/// and the ordering are things a gate can actually observe.
///
/// ★ The first two rows are OUT of time order — `12:04:34` then `12:04:36` — and
/// that is the prototype's own quirk, kept. It is the cheapest possible proof
/// that sorting this feed does something: unsorted, the newest alarm is second.
pub const ALARMS: &[AlarmSpec] = &[
    AlarmSpec {
        at: (12, 4, 34),
        severity: "error",
        message: "Session closed - P-06 (link fault)",
    },
    AlarmSpec {
        at: (12, 4, 36),
        severity: "warn",
        message: "Keep-alive timed out - P-03",
    },
    AlarmSpec {
        at: (12, 2, 11),
        severity: "warn",
        message: "Endpoint unreachable - P-06",
    },
    AlarmSpec {
        at: (11, 58, 3),
        severity: "info",
        message: "Protocol revision 0x09 agreed",
    },
    AlarmSpec {
        at: (11, 55, 47),
        severity: "error",
        message: "Decode fault - malformed frame - P-04",
    },
    AlarmSpec {
        at: (11, 52, 20),
        severity: "info",
        message: "Subscription declared - units/*/pose",
    },
    // ── this build's continuation; see the note above ──
    AlarmSpec {
        at: (11, 49, 58),
        severity: "info",
        message: "Session opened - P-04",
    },
    AlarmSpec {
        at: (11, 47, 12),
        severity: "warn",
        message: "Retransmit budget at 80% - P-02",
    },
    AlarmSpec {
        at: (11, 44, 30),
        severity: "info",
        message: "Identifier 7 bound - units/2/pose",
    },
    AlarmSpec {
        at: (11, 41, 7),
        severity: "error",
        message: "Reply timed out - store/config",
    },
    AlarmSpec {
        at: (11, 38, 44),
        severity: "info",
        message: "Endpoint joined - P-06",
    },
    AlarmSpec {
        at: (11, 35, 19),
        severity: "warn",
        message: "Fragment reassembly deferred - P-05",
    },
    AlarmSpec {
        at: (11, 32, 2),
        severity: "info",
        message: "Identifier 4 bound - units/*/pose",
    },
    AlarmSpec {
        at: (11, 28, 51),
        severity: "warn",
        message: "Queue depth above watermark - P-01",
    },
    AlarmSpec {
        at: (11, 25, 36),
        severity: "info",
        message: "Session opened - P-06",
    },
    AlarmSpec {
        at: (11, 22, 10),
        severity: "error",
        message: "Frame discarded - checksum mismatch - P-03",
    },
    AlarmSpec {
        at: (11, 19, 47),
        severity: "info",
        message: "Retention window reset - store/**",
    },
    AlarmSpec {
        at: (11, 16, 22),
        severity: "warn",
        message: "Keep-alive late - P-05",
    },
];

/// The alarm feed's three sortable columns, left to right.
///
/// The column whose width is `0` takes what the others leave — the same
/// convention [`STREAM_COLUMNS`] uses on this screen, and it is not a shortcut:
/// a card's body width is a function of the window's, so fixed numbers could
/// equal the body at exactly one window size and would be wrong at every other.
pub const ALARM_COLUMNS: &[(&str, u32)] = &[("Severity", 96), ("Time", 84), ("Event", 0)];

/// One alarm row's vertical slot.
///
/// ★★★★★ ONE LINE, and this is the one place the prototype's row shape is not
/// reproduced. The reason is [`BOARD`]'s count: the prototype draws this seat two
/// board rows tall and gives its row two lines — a severity word beside a clock
/// reading, then the message under both — and the count above leaves this board
/// exactly ONE row for the card. In the body a single row gives, a two-line row
/// at the prototype's own padding shows **two** alarms and a one-line row shows
/// four; a feed showing two of eighteen is not a feed.
///
/// ⚠ And the divergence pays for itself twice, which is why it is this way round
/// rather than dropping a column: a message on its own full-width second line
/// can be headed by nothing, so the prototype's row admits at most TWO columns.
/// One line puts all three parts side by side, which is what makes the message
/// sortable and the header honest about what sits under it.
///
/// The swatch stays. It is the prototype's own three-pixel severity stripe and
/// it is the thing a reader scanning for trouble actually uses.
pub const ALARM_ROW_H: u32 = 24;

/// The alarm feed's header strip.
///
/// Shorter than the column header's 40px default, because this strip sits inside
/// a one-row card body and every pixel it takes is an alarm the feed cannot show.
///
/// ★ 26 and not 24, and the two pixels are load-bearing: the body a `4 x 1` card
/// gives is 122px, the feed shows whole rows only, and `122 - 26 = 96` is exactly
/// four rows where `122 - 24 = 98` is four rows and a two-pixel stripe. The
/// remainder is better spent on the header than left as a gap.
/// `r1851_the_feed_builds_only_the_window_it_shows` prints all four numbers.
pub const ALARM_HEAD_H: u32 = 26;

/// The narrowest the feed's reading column can be and still say anything.
///
/// Below it the feed draws NOTHING rather than three clipped words — the
/// all-or-nothing clamp the health strip and the latency tiles already make, and
/// for the same reason: an alarm row's three parts are one statement.
pub const ALARM_EVENT_FLOOR: u32 = 120;

/// How many alarm rows the feed CONSTRUCTS on the opening board.
///
/// The window that fits the body a `4 x 2` card gives, plus the overscan row at
/// each end. Pinned rather than derived for [`HEALTH_TILES_SHOWN`]'s reason — the
/// rule needs the card's pixel height, which lives in the painter — and honest
/// for the same one: `r1851_the_feed_builds_only_the_window_it_shows` fails the
/// moment the paint and this disagree, and PRINTS both.
///
/// ⚠ It is a count of SLOTS, not of alarms. [`ALARMS`] is longer, which is the
/// whole point: the rows outside this window are never constructed.
pub const ALARM_ROWS_SHOWN: usize = 4;

/// Which column the feed opens sorted on, and whether ascending.
///
/// Time, descending — newest first, which is the order the prototype writes its
/// own rows in and therefore the order it means. Unlike the prototype this build
/// says so, and having said so cannot then paint them in a different one.
pub const ALARM_OPENING_SORT: (usize, bool) = (1, false);

/// How many value-axis ticks the distribution draws.
///
/// `ChartStyle::default().y_ticks`, which is what the painter actually asks
/// for. Stated here because the silence census needs a population and a number
/// written twice is a number that drifts — the gate below asserts these two are
/// the same.
pub const LATENCY_Y_TICKS: usize = 5;

/// Tile `n` as the **reference** publishes it: its key and its rendered value.
///
/// The oracle, not the output. The card derives these three numbers from
/// [`LATENCY_SAMPLES`] and never reads this function; the paint gate compares
/// what the card drew against what this says, which is an assertion that can
/// fail. A gate that read the card's own helper would be comparing a value
/// with itself.
///
/// Out of range answers an empty pair rather than panicking: this is read by a
/// sweep that walks whatever the card painted, and a card that grew a fourth
/// tile should fail the comparison rather than take the process down.
#[cfg(test)]
#[must_use]
pub fn latency_tile(n: usize) -> (&'static str, String) {
    let ms = match n {
        0 => LATENCY_P50,
        1 => LATENCY_P95,
        2 => LATENCY_MAX,
        _ => return ("", String::new()),
    };
    (LATENCY_STAT_KEYS[n], format!("{ms:.1} {LATENCY_UNIT}"))
}

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
    /// ★ R1797 — one per [`LATENCY_STAT_KEYS`] landmark, keyed by index.
    ///
    /// A second stat population rather than a reuse of [`Self::Stats`]: the two cards
    /// have three tiles each TODAY, and a population that answered "three" for
    /// both would keep agreeing after one of them changed. The census is meant
    /// to break when a surface does.
    LatencyTiles,
    /// ★ R1797 — one per latency bucket: the stated ladder's interior bins plus
    /// the two unbounded ends.
    ///
    /// Derived from [`LATENCY_LADDER`] rather than written as eight, so a
    /// boundary added there moves the census with it.
    LatencyBins,
    /// ★ R1797 — one per value-axis tick the distribution draws.
    ///
    /// The chart's own default, which is the honest source: this is how many
    /// grid lines and y labels the painter emits, and a number here that
    /// disagreed would demand regions nothing paints.
    LatencyTicks,
    /// ★ R1846 — one per health tile the strip actually DRAWS, which is
    /// [`HEALTH_TILES_SHOWN`] and not the whole of [`HEALTH_TILES`].
    ///
    /// The first family here whose size is a function of the card's width. See
    /// that constant for why the number is pinned rather than derived, and for
    /// the gate that makes the pin safe.
    HealthTiles,
    /// ★ R1846 — the health strip's own container, one per placed health card.
    ///
    /// It gets a row where `card.latency#4.tiles` deliberately does not,
    /// because this one PAINTS: it is a real box in the scene and therefore an
    /// addressable region, while the latency card's is a grouping node with no
    /// rectangle. The comment beside that absence in [`VOICES`] is what made
    /// the difference checkable rather than a judgement call.
    HealthStrip,
    /// ★ R1846 — the parts of each drawn health tile whose text IS the tile's
    /// NAME: the label's box and the caption inside it.
    HealthTileNames,
    /// ★ R1846 — the parts of each drawn health tile that are folded into the
    /// tile's announcement: the value, the delta, and the trailing figure with
    /// the sparkline the caller hands it.
    ///
    /// ⚠ These two families are the SHAPE `pinion_widget_paint::stat_tile`
    /// builds, written where this screen's census can see it. They are not a
    /// second opinion about it: the crate declares the silence, and these rows
    /// say the screen expects exactly those regions to be quiet. When the crate
    /// changes its shape both must move, and the demo is what says so.
    HealthTileParts,
    /// ★ R1851 — the alarm feed's assembly root: the TABLE itself.
    AlarmFeed,
    /// ★ R1851 — the feed's heading strip, which announces as a `row`.
    ///
    /// Its own family rather than part of [`AlarmFeed`](Self::AlarmFeed) because
    /// the two announce as different KINDS, and the census pairs a family with
    /// one role. WAI-ARIA is what decides it: a `columnheader` is a member of a
    /// `row`, so a heading strip attached to anything else is a heading of
    /// nothing — asserted by the structure gate, which refused the first draft
    /// by name.
    AlarmHead,
    /// ★ R1851 — the container that frames the feed's rows.
    ///
    /// Separate from [`AlarmFeed`](Self::AlarmFeed) because this one is SILENT
    /// and those two speak; a family spanning both would have to be split at the
    /// gate instead of here.
    AlarmBody,
    /// ★ R1851 — one per [`ALARM_COLUMNS`] heading, keyed by its visual index.
    AlarmColumns,
    /// ★ R1851 — the part of each heading that carries the heading's own word,
    /// which the section it sits in already announces.
    AlarmColumnLabels,
    /// ★★★★★ R1856 — the sort ARROW, which exists on exactly one heading.
    ///
    /// The first family here whose membership is a function of a value a reader
    /// can CHANGE, and it is derived from [`ALARM_OPENING_SORT`] for that
    /// reason: this table describes the destination as it opens, so the family
    /// names the heading the feed opens sorted on and says so rather than
    /// listing all three and being wrong about two.
    ///
    /// ⚠ The residue, stated: after a reader re-sorts, the painted arrow moves
    /// and this row describes where it WAS. That is the same bargain
    /// [`AlarmRows`](Self::AlarmRows) makes with the scroll offset, and it is
    /// the honest one for an opening-state census — the alternative, a family
    /// that reads live state, would make the specification a mirror of the paint
    /// instead of a claim against it.
    ///
    /// Empty when the opening sort names a column [`ALARM_COLUMNS`] does not
    /// have. Sortability is NOT consulted, because this table does not declare
    /// it — every alarm heading is built sortable — and a claim about a
    /// property the specification does not hold would be prose the code cannot
    /// keep.
    AlarmSortIndicator,
    /// ★★★★★ R1851 — one per alarm row the feed **CONSTRUCTS**, keyed by its
    /// slot in the window rather than by the alarm it holds.
    ///
    /// The first family here whose size is a function of the card's HEIGHT and
    /// whose membership changes with the SCROLL. Keyed by slot for exactly that
    /// reason: the set of rows built is a window over [`ALARMS`], so a family
    /// keyed by alarm would name eighteen regions of which the feed paints
    /// [`ALARM_ROWS_SHOWN`] — and which eighteen depends on where a reader has
    /// scrolled to. A slot is a place in the feed and there are always that
    /// many of them; WHICH alarm is in one is the row's announcement, not its
    /// address.
    ///
    /// ⚠ Like [`HEALTH_TILES_SHOWN`] this is a PINNED number rather than a
    /// derived one, for that constant's reason: the rule that produces it needs
    /// the card's pixel height, which lives in the painter and is not reachable
    /// from a `const` table. The gate is what makes the pin honest.
    AlarmRows,
    /// ★★★★★ R1851 — one per CELL of each constructed alarm row.
    ///
    /// A `row` owns members of a cell role or it is an empty collection, which
    /// the structure gate refuses; and a word painted with no tag is a word the
    /// announcement cannot point at. So the row's three parts are cells, and
    /// this is their family — the product of [`AlarmRows`](Self::AlarmRows) and
    /// [`ALARM_COLUMNS`], written as a product rather than a number so a column
    /// added there moves the census with it.
    AlarmCells,
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
            Population::Sections => SECTIONS.iter().map(|(key, _)| (*key).to_owned()).collect(),
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
            Population::LatencyTiles => indexes(LATENCY_STAT_KEYS.len()),
            Population::HealthTiles => indexes(HEALTH_TILES_SHOWN),
            Population::HealthStrip => {
                card_of("health").map_or_else(Vec::new, |id| vec![format!("{id}.tiles")])
            }
            Population::HealthTileNames => health_tile_parts(&["label", "label.caption"]),
            Population::HealthTileParts => health_tile_parts(&[
                "value",
                "value.caption",
                "delta",
                "delta.caption",
                "trail",
                "trail.spark",
                "trail.spark.line",
            ]),
            // ★ R1851 — the alarm card's six families answer through one helper.
            // Not for tidiness: `members` is at this crate's line limit, and a
            // family added to one card should not have to argue with the size of
            // a function about six others.
            Population::AlarmFeed
            | Population::AlarmHead
            | Population::AlarmBody
            | Population::AlarmColumns
            | Population::AlarmColumnLabels
            | Population::AlarmSortIndicator
            | Population::AlarmRows
            | Population::AlarmCells => alarm_members(self),
            // The interior bins the ladder's boundaries describe, plus the two
            // unbounded ends `BinEnds::Open` adds.
            Population::LatencyBins => indexes(LATENCY_LADDER.len() + 1),
            Population::LatencyTicks => indexes(LATENCY_Y_TICKS),
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

/// `{health card}.stat.{n}.{part}` for every part of every tile the strip draws.
///
/// ★ R1846 — the product of [`HEALTH_TILES_SHOWN`] and the suffixes the crate's
/// tile builds. Written once rather than per suffix, because the suffixes are
/// one fact — the shape of `pinion_widget_paint::stat_tile` — and nine rows of
/// it would be nine places to update when that shape moves.
fn health_tile_parts(parts: &[&str]) -> Vec<String> {
    card_of("health").map_or_else(Vec::new, |id| {
        (0..HEALTH_TILES_SHOWN)
            .flat_map(|n| {
                let id = id.clone();
                parts
                    .iter()
                    .map(move |part| format!("{id}.stat.{n}.{part}"))
                    .collect::<Vec<_>>()
            })
            .collect()
    })
}

/// The members of one of the alarm card's seven families.
///
/// ★ Every one of them is a PRODUCT or a range over a declared table — the
/// headings over [`ALARM_COLUMNS`], the rows over [`ALARM_ROWS_SHOWN`], the cells
/// over both — written that way rather than as numbers so a column added to the
/// feed moves the census with it.
///
/// # Panics
///
/// On a family that is not the alarm card's. Unreachable: the caller is
/// [`Population::members`]'s own match arm, and adding a family to this card
/// without adding it here is a non-exhaustive match rather than a run-time
/// surprise.
fn alarm_members(family: Population) -> Vec<String> {
    let cols = ALARM_COLUMNS.len();
    let named =
        |parts: Vec<String>| alarm_parts(&parts.iter().map(String::as_str).collect::<Vec<_>>());
    match family {
        Population::AlarmFeed => alarm_parts(&["feed"]),
        Population::AlarmHead => alarm_parts(&["feed.head"]),
        Population::AlarmBody => alarm_parts(&["feed.body"]),
        Population::AlarmColumns => {
            named((0..cols).map(|n| format!("feed.head.col#{n}")).collect())
        }
        Population::AlarmColumnLabels => named(
            (0..cols)
                .map(|n| format!("feed.head.col_label#{n}"))
                .collect(),
        ),
        Population::AlarmSortIndicator => {
            let (col, _) = ALARM_OPENING_SORT;
            // A column this table does not have paints no arrow, so the family
            // is empty rather than naming a region the screen would then owe an
            // explanation for.
            named(
                (col < cols)
                    .then(|| format!("feed.head.col_sort#{col}"))
                    .into_iter()
                    .collect(),
            )
        }
        Population::AlarmRows => named(
            (0..ALARM_ROWS_SHOWN)
                .map(|n| format!("feed.row.{n}"))
                .collect(),
        ),
        Population::AlarmCells => named(
            (0..ALARM_ROWS_SHOWN)
                .flat_map(|n| (0..cols).map(move |k| format!("feed.row.{n}.cell.{k}")))
                .collect(),
        ),
        other => unreachable!("{other:?} is not one of the alarm card's families"),
    }
}

/// `{card}.{part}` for every named part of the placed alarms card.
///
/// The alarm card's peer of [`health_tile_parts`], and separate from it for the
/// same reason that function is not a general helper: the SHAPE differs. A
/// health tile's parts hang off a per-tile index; the feed's hang off the
/// assembly root, and one function taking both would need a parameter saying
/// which shape it was building.
fn alarm_parts(parts: &[&str]) -> Vec<String> {
    card_of("alarms").map_or_else(Vec::new, |id| {
        parts.iter().map(|part| format!("{id}.{part}")).collect()
    })
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
    // --- the latency card (R1797) ---------------------------------------
    //
    // ★ Three, and the distribution is the one that matters: a chart is where
    // "announce the picture" is not enough, because the SHAPE is the content.
    // Its reading carries the sample count, the rule that binned them, every
    // bucket with its count and which buckets are the tail — the same
    // derivation the wire publishes and the paint draws.
    // ⚠ No entry for `card.latency#4.tiles`. It is an accessibility GROUPING
    // node — it gathers the three tiles under one name — and it paints nothing
    // of its own, so a voice declaration for it names a region the census
    // cannot find. The sibling `card.filter#3.counts` is the same shape and has
    // the same absence; this comment is here because that absence read as an
    // oversight until the census refused the entry.
    VoiceSpec {
        tag: "card.latency#4.stat.{}",
        role: "status",
        population: Population::LatencyTiles,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.latency#4.bins",
        role: "group",
        population: Population::One,
        at: Where::At("dashboard"),
    },
    // --- the health card (R1843, declared R1846) --------------------------
    //
    // ★★★★★ R1846 — these four rows were MISSING, and the round that built the
    // card did not notice because the demo that asks is not run by
    // `cargo test`. Measured before they were written: the running screen
    // announced `card.health#5.tiles` and three `card.health#5.stat.{n}` that
    // this table had never named — which is `r1694`'s "nothing speaks that the
    // specification did not say would", failing in the direction that matters
    // most, since an undeclared voice is one nobody reviewed.
    //
    // ⚠ Unlike `card.latency#4.tiles` (see the note above it) this strip's
    // container PAINTS, so it is an addressable region and owes a row.
    VoiceSpec {
        tag: "card.{}",
        role: "group",
        population: Population::HealthStrip,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.health#5.stat.{}",
        role: "status",
        population: Population::HealthTiles,
        at: Where::At("dashboard"),
    },
    // ★ The caption is VOICED, not silenced. It says what is measured, what the
    // buckets are, and why some bars are emphasised — the third clause being
    // the one the reference could not write, since its tail is an index. A
    // reader who cannot see the chart needs that sentence more than one who can.
    VoiceSpec {
        tag: "card.latency#4.caption",
        role: "status",
        population: Population::One,
        at: Where::At("dashboard"),
    },
    // --- the alarm feed (R1851) -------------------------------------------
    //
    // ★★★★★ The feed and its header strip are GROUPS a reader descends through,
    // each heading is a `columnheader` that says what its column holds AND
    // which way it is sorted, and each constructed row announces the whole
    // alarm. Written here BEFORE the card was drawn rather than after, which is
    // R1846's lesson: that round built a card and found four voices the
    // specification had never named, invisible to `cargo test` because a voice
    // census needs a running screen.
    // ★★★★★ A TABLE, whose heading strip is a `row` of `columnheader`s and whose
    // data rows are `row`s of `cell`s. That shape is WAI-ARIA's structural rule
    // rather than a preference, and the first draft — a `group` holding rows
    // that held nothing — was refused by the structure gate at eleven nodes:
    // every `columnheader` stray, every `row` empty. What the rule buys is the
    // thing this card is FOR: `aria-sort` is a property of a column heading, and
    // a heading not in a row is a heading of nothing.
    VoiceSpec {
        tag: "card.{}",
        role: "table",
        population: Population::AlarmFeed,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.{}",
        role: "row",
        population: Population::AlarmHead,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.{}",
        role: "columnheader",
        population: Population::AlarmColumns,
        at: Where::At("dashboard"),
    },
    // ★ A row, not a status: this is tabular content a reader moves through,
    // and the severity is part of what the row SAYS rather than an urgency the
    // screen asserts. A live region here would interrupt on every scroll.
    VoiceSpec {
        tag: "card.{}",
        role: "row",
        population: Population::AlarmRows,
        at: Where::At("dashboard"),
    },
    VoiceSpec {
        tag: "card.{}",
        role: "cell",
        population: Population::AlarmCells,
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
    // ★★★★★ R1868 — the settings page's two SPEAKING regions, which the table
    // had never named either. Both are `status`, and both were found the same
    // way as the twelve silences beside them: by a reconciliation that finally
    // ran somewhere other than the destination the application opens at.
    //
    // The build strip is the one place either screen says which build a reader
    // is looking at — the fact a person filing a defect is asked for first — and
    // the plugin row's seat carries the formats this build can take apart. A
    // region that speaks and is published nowhere is a client reading the
    // specification and being told the window is quieter than it is.
    VoiceSpec {
        tag: "shell.settings.build",
        role: "status",
        population: Population::One,
        at: Where::At("settings"),
    },
    VoiceSpec {
        tag: "shell.settings.row.plugins.chips",
        role: "status",
        population: Population::One,
        at: Where::At("settings"),
    },
    // --- what just happened ---------------------------------------------
    VoiceSpec {
        tag: "shell.toast",
        role: "status",
        population: Population::One,
        at: Where::Chrome,
    },
    // ★★★★★ R1867 — and what is always true: the gesture sentence, which the
    // status band's slot shows whenever no toast has taken it. Announced for
    // the reason the slot's silence is a `layout` — the promise that the slot's
    // occupant speaks has to hold in BOTH of its states, and this is the state
    // nothing was speaking in.
    //
    // ⚠ It is `Where::Chrome` and it is nonetheless absent while a toast is up,
    // which is not a contradiction: `Where` says which DESTINATIONS paint a
    // region, and this one is painted at all six. The other axis — the slot's
    // occupancy — is the toast's lifetime, and the accessibility tree follows
    // it so the region is announced exactly when it is painted.
    VoiceSpec {
        tag: "shell.status.gesture",
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
    // ★★★★★ R1846 — every part of every health tile the strip draws, quiet
    // because the tile itself speaks. These are the first silences in this
    // table the SCREEN does not declare: `pinion_widget_paint::stat_tile` does,
    // at the site that paints them, and this pair of rows is the screen saying
    // it expects exactly that. Before R1846 the crate declared nothing and the
    // running dashboard reported 27 undecided regions — one third of a card —
    // while every `cargo test` gate stayed green, because a voice census needs
    // a running screen and a demo is not run by `cargo test`.
    (
        "card.{}",
        Population::HealthTileNames,
        "name_of",
        Where::At("dashboard"),
    ),
    (
        "card.{}",
        Population::HealthTileParts,
        "part_of",
        Where::At("dashboard"),
    ),
    // ★ R1851 — a heading's own word, quiet because the section it sits in
    // announces it. `pinion_widget_paint::column_header` paints the label as a
    // pointer-transparent leaf inside its section for exactly that reason, and
    // this row is the screen saying it expects that region and no other.
    (
        "card.{}",
        Population::AlarmColumnLabels,
        "name_of",
        Where::At("dashboard"),
    ),
    // ★★★★★ R1856 — the arrow that says which way the feed is sorted, quiet
    // because the heading it sits in announces the direction in words. A
    // reader who never reaches this region loses nothing, which is what
    // `decorative` claims; the heading's `aria-sort` is what makes the claim
    // true, and `pinion_widget_paint::header_feed`'s own gate is what keeps the
    // pairing.
    (
        "card.{}",
        Population::AlarmSortIndicator,
        "decorative",
        Where::At("dashboard"),
    ),
    // The feed's scrolling viewport is a clip, not a thing on the screen — the
    // same declaration the board's own viewport carries, and made at the site
    // that paints it (`HeaderFeed::build`) rather than here.
    // Two rows because the two regions are addressed differently: the frame is
    // under the card's id, and the clip answers to a `ScrollState` tag, which is
    // `&'static` and therefore names the KIND.
    //
    // ⚠ R1856 — these three rows were written at R1851 and were the SCREEN's
    // half of a declaration the PAINT never made: the assembly left the silence
    // an opt-in and this screen never took it, so every one of these regions
    // shipped undecided while this table said otherwise. The gate that would
    // have caught it is the `^` below, and it never ran, because the
    // `unvoiced == 0` assertion in front of it failed first. A specification is
    // a claim against the paint, and a claim nothing executes is prose.
    (
        "card.{}",
        Population::AlarmBody,
        "layout",
        Where::At("dashboard"),
    ),
    (
        "card.alarms.feed.scroll",
        Population::One,
        "layout",
        Where::At("dashboard"),
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
    // ★★★★★ R1797 — the latency distribution's marks. The REGION is what a
    // reader is told about and its announcement carries the whole shape —
    // sample count, the rule that binned them, every bucket with its count and
    // which are the tail — so each mark here is how that is drawn.
    //
    // Declared per FAMILY rather than as one wildcard, because the four
    // families owe different kinds of quiet and saying so is the point of this
    // table: a bar is part of the distribution, an axis line and a grid line
    // are decoration, and a tick label's words are already in the region's
    // reading. A single blanket entry would satisfy the census while making no
    // claim at all.
    (
        "card.latency#4.dist",
        Population::One,
        "part_of",
        Where::At("dashboard"),
    ),
    (
        "card.latency#4.dist.bar.{}",
        Population::LatencyBins,
        "part_of",
        Where::At("dashboard"),
    ),
    (
        "card.latency#4.dist.xlabel.{}",
        Population::LatencyBins,
        "part_of",
        Where::At("dashboard"),
    ),
    (
        "card.latency#4.dist.axis.x",
        Population::One,
        "decorative",
        Where::At("dashboard"),
    ),
    (
        "card.latency#4.dist.axis.y",
        Population::One,
        "decorative",
        Where::At("dashboard"),
    ),
    (
        "card.latency#4.dist.grid.y.{}",
        Population::LatencyTicks,
        "decorative",
        Where::At("dashboard"),
    ),
    (
        "card.latency#4.dist.label.y.{}",
        Population::LatencyTicks,
        "part_of",
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
    // ★★★★★ R1868 — the settings page's other twelve quiet regions, and the
    // reason they were missing for so long is the finding rather than the rows:
    // the reconciliation that holds this table to what the screen paints ran
    // ONLY at the destination the application opens at. `r1694` compares both
    // records at the dashboard; `r1695` visits every destination and asks only
    // whether anything is *undecided*. So a page that is not the opening one
    // could paint whatever it liked and publish none of it, and this page did —
    // fourteen regions, twelve quiet and two speaking.
    //
    // Each word here is the PAINTER's, read off the census rather than chosen:
    // a row published with a different reason than the scene declares is the
    // `KindDiffers` arm, which is the half a comparison over tags cannot see.
    (
        "shell.settings.body",
        Population::One,
        "layout",
        Where::At("settings"),
    ),
    (
        "shell.settings.head.title",
        Population::One,
        "name_of",
        Where::At("settings"),
    ),
    (
        "shell.settings.head.gist",
        Population::One,
        "part_of",
        Where::At("settings"),
    ),
    (
        "shell.settings.head.{}",
        Population::OptionGroups,
        "name_of",
        Where::At("settings"),
    ),
    (
        "shell.settings.plugin.records",
        Population::One,
        "part_of",
        Where::At("settings"),
    ),
    (
        "shell.settings.plugin.schema",
        Population::One,
        "part_of",
        Where::At("settings"),
    ),
    (
        "shell.settings.row.{}",
        Population::ValueRows,
        "part_of",
        Where::At("settings"),
    ),
    (
        "shell.settings.row.plugins",
        Population::One,
        "part_of",
        Where::At("settings"),
    ),
    // ★★★★★ R1867 — the host's status band and its one message slot. Both are
    // furniture: the band is the slot's ground and the slot arranges whichever
    // occupant is there, so `layout` is the only honest kind for either — and
    // `layout` is the one kind the census will not let a screen hide behind,
    // because it promises the subtree speaks and reports `hollow` when it does
    // not.
    //
    // ⚠ These two rows are the reason this round exists. `shell.status` entered
    // at R1864 and `shell.status.slot` at R1865, both without a declaration and
    // neither round aware of the other — which is a structure and not a slip.
    // What caught them was the demo sweep, which runs AFTER a push; what stops
    // the next one is `r1867_no_destination_paints_a_region_with_no_declared_
    // voice` in this crate's own tests, which runs before one.
    ("shell.status", Population::One, "layout", Where::Chrome),
    (
        "shell.status.slot",
        Population::One,
        "layout",
        Where::Chrome,
    ),
];
