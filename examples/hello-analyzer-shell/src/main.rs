// R1412 §5.49 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]
// ★★★★★ R1819 — `spec_json` publishes this screen's whole written-down
// specification through one `serde_json::json!`, and that macro had reached the
// default recursion budget: adding a FOURTH key of any kind failed to build.
//
// Worth stating rather than silently raising, because the failure is
// misleading in a specific way — the compiler names whichever nested block
// happens to expand last, so the error pointed at `silences`, then at `locked`,
// while the actual cause was the outer object's depth. Extracting one block at
// a time just moves the finger.
//
// ⇒ a published table that cannot grow is a surface that quietly stops
// recording the screen, which is the opposite of what it is for.
#![recursion_limit = "256"]

//! `hello-analyzer-shell` — R1648/R1649 §5.21 §5.51 §2 #7 — the analysis-tool
//! **dashboard shell**, assembled as one application and operated by hand.
//!
//! ## Why an assembly is the deliverable
//!
//! `tools/analyzer_census.py` classifies every capability of this tool class
//! with one of five verdicts, and the largest bin is `app`: *the substrate is
//! here, the domain logic is the application's*. A `have` verdict is proven by
//! a test that exercises the capability through the public API (R1602); an
//! `app` verdict is a claim about composition, and the only thing that proves
//! composition is a composite. This is that composite, and the census names it.
//!
//! ## R1649 — the shell matches the reference tool's, screen and gesture
//!
//! R1648 assembled the *capabilities* and laid them out in the plainest
//! arrangement that could hold them. R1649 rebuilds the chrome and the
//! placement gestures against the tool this axis is judged by, because a shell
//! is exactly the part where "the substrate can express it" is not the same
//! claim as "the substrate can express it the way a professional tool does".
//! The differences that cost real work were all the second claim:
//!
//! * **A three-column shell**, not a board with a strip: an icon rail, a
//!   sub-header carrying the layout preset and the two board verbs, the canvas,
//!   and a **widget palette** the board is populated *from*. Twelve widget
//!   kinds exist as a catalogue; three are placed.
//! * **A card is added, not seeded.** The palette's `+` places one at the
//!   bottom of the board, so what is on the board is a decision somebody made
//!   and the count in two places has to agree.
//! * **A drag shows where it will land** — a snap-preview slot and a brighter
//!   grid — rather than displacing cards live. The reference commits on
//!   release; so does this.
//! * **Layout-edit mode** puts per-card size steppers on the cards, so a board
//!   is rearranged without a pointer drag at all.
//! * **Detaching REMOVES the card from the board** and opens a floating panel
//!   with a re-dock control; re-docking appends it at the bottom. R1648 kept
//!   the tile in place and argued for it — the reference does not, and on this
//!   axis the reference is the specification rather than a data point.
//!
//! ## The state vocabulary is a value, not paint
//!
//! Every card carries a [`Card`]: its header affordances as a set, and what its
//! body is showing as a [`CardState`] whose [`Remedy`] is **derived**. The pair
//! that justifies six arms rather than one `Error` is denied/opaque — both
//! render as "no content" and they are opposite in what a person can do, so a
//! shell with one arm offers "request access" on a link no permission can open.
//! Measured on the toolkit at 6.11: no content-state concept exists on any
//! panel or view class, and its item views have no placeholder at all.
//!
//! ## It is operated by hand, and by the wire, through one set of handlers
//!
//! ```text
//! cargo run -p hello-analyzer-shell --release
//! ```
//!
//! Add a widget from the palette, drag a card by its header, press the header
//! controls, toggle layout-edit mode and use the size steppers, detach a card
//! and re-dock it, switch a saved layout. The keyboard is [`KEYMAP`].
//!
//! A real press and a scripted one reach the same code: the framework's router
//! calls `pointer_move` and sends `PointerDown` / `PointerUp` through
//! `invoke("send", …)`, and the wire's `point` moves the same cursor. There is
//! no parallel automation surface that can drift from what a hand does.
//!
//! The geometry is likewise one thing — every rectangle comes from the helpers
//! above [`Hit::at`], read by both the painter and the hit test. The demo
//! sweeps the window in both directions to keep it so, and the second direction
//! (every painted control must be pressable at the centre of the rectangle it
//! was painted in) is what caught R1648 painting every card's contents at twice
//! their intended offset: children of an absolutely-positioned container are
//! placed relative to *it*.
//!
//! See `tools/demos/r1648_the_analyzer_shell_is_assembled.py`.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use pinion_a11y::{
    AccessFocus, AccessLive, AccessNode, AccessState, AccessValue, AriaRole, GridCell, GridColumn,
    GridExtent, GridRow, HasPopup, NavLink, SortDirection, WidgetA11y, grid_table_nodes_clamped,
    navigation_link_nodes, page_region_node,
};
use pinion_chart::{
    Bar, BarChart, BinEnds, Binned, ChartStyle, Mute, QuantileMethod, Quantiles, Sparkline,
};
use pinion_core::availability::Unavailable;
use pinion_core::crossing::{Crossing, CrossingPolicy, Passage, Rest, Side};
use pinion_core::detach::{Arrival, DetachHome, DetachPolicy, DetachedAffordance, HomeRequest};
use pinion_core::drop_target::{
    BOARD_WIDGET_DRAG_KIND, DropAccept, DropAction, DropActions, DropClause, DropContract,
    DropOffer, DropStanding, DropVerdict, standing_value,
};
use pinion_core::edge_panel::{EdgePlacement, PanelAffordance};
use pinion_core::external::{
    ArgForm, Backend, BackendFallback, BackendSupport, DragPayload, DragUpdate, External,
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    PointerTarget, ReadRefusal, RepaintOwner, SchemaArg, SchemaField, ThreadOwnership,
};
use pinion_core::focus_state;
use pinion_core::input::PointerReading;
use pinion_core::reactive::{Effect, Owner, Signal};
use pinion_core::scene::{ContainerNode, PathCommand, PathNode, PathPoint, Rect, TextNode};
use pinion_core::shrink::ShrinkPolicy;
use pinion_core::storage::Storage;
use pinion_core::style::{
    Border, BoxStyle, Chrome, ChromeEdge, ChromeRole, Color, LayoutStyle, PathStyle, Size, Stroke,
    TextAlign, TextOverflow, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, ThemeGap, ThemeMode, ThemeProvider, use_theme};
use pinion_core::utterance::{Announced, Tone, Utterance};
use pinion_core::voice::Silence;
use pinion_core::widgets::button::ButtonState;
use pinion_core::widgets::card::{Card, CardAffordance, CardChrome, CardState, Remedy};
use pinion_core::widgets::chip_group::{Chip, ChipGroup};
use pinion_core::widgets::destination::{Destinations, Detour, Journey};
use pinion_core::widgets::grid_sort::grid_sort_str;
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::roving::{Activation, Axis, Ends, Landing, Member, Roving, RovingSpec};
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::severity::SeverityScale;
use pinion_core::widgets::tile_grid::{
    Carried, Dropped, Maximized, Tile, TileDirection, TileDrag, TileGrid, TileId, TileNudge,
};
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::transport::{TransportClock, TransportStatus, use_transport_clock};
use pinion_core::widgets::view_order::{
    compute_order, cycle_sort, sort_dir_from_str, sort_dir_str,
};
use pinion_core::window_level::WindowLevel;
use pinion_core::workspace::Workspaces;
use pinion_core::{Frame, Scene, WidgetCore};
// ★★★★★ R1724 — the axis that makes this file an application rather than a
// screen: a destination's page can be another binding, mounted whole.
use pinion_core::chrome::{HostChrome, Part as ChromePart};
use pinion_core::widgets::picker::Picker;
use pinion_screen::{Mount, PageInset, Screen, ScreenRoster, ScreenState};
use pinion_shell::{SizeStrategy, WidgetView, WindowSpec, vello_renderer_impl};
use pinion_widget_paint::button::{self, ButtonColors, ButtonStyle};
use pinion_widget_paint::card_header;
use pinion_widget_paint::chooser;
// ★★★★★ R1951 — the marks a chrome control draws. The palette's fold button
// wore a face of its own until this round; now the same act reads the same way
// here and in the node lab's panels.
use pinion_widget_paint::control_mark;
use pinion_widget_paint::header_feed::{FeedColumn, HeaderFeed, HeaderFeedStyle};
use pinion_widget_paint::pages::{PagePointer, view_page_region};
use pinion_widget_paint::pane::{PanePointer, scroll_pane};
use pinion_widget_paint::run::text_run;
use pinion_widget_paint::stat_tile::StatTile;
use pinion_widget_paint::switch::{self, SwitchStyle};

mod judge;
mod spec;

// pinion-forge codegen output: `pub struct HelloAnalyzerShellRenderer` + its
// error type + async `new<...>` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloAnalyzerShellRenderer, HelloAnalyzerShellRendererError);

/// The size the specification's rectangles were measured against, and the
/// floor this screen declares to the shell.
const WIN_W: u32 = spec::WIN_W;
const WIN_H: u32 = spec::WIN_H;

/// R1712 — this screen's two floors, which are one size.
///
/// [`ShrinkPolicy::rigid`] says so as a declaration rather than as the absence
/// of one: the window stops where the layout stops, deliberately.
const SHRINK: ShrinkPolicy = ShrinkPolicy::rigid((WIN_W, WIN_H));

/// The live surface, or the design size where no shell has published one.
///
/// ★ R1671 — reported by a person maximising the window: the content stayed
/// where it was and the rest of the window was empty. Every rectangle on this
/// screen was authored against the CONSTANTS, so the screen painted 1440x900
/// in whatever window it was given. Screens A and B have read the live size
/// since R1654; this one never did, and its own painted sweep could not see it
/// because every check compared the screen to itself.
///
/// `use_viewport_size` is a tracked read, so the view re-runs on a resize; it
/// is strict about the owner scope and a bare unit call has none. The design
/// size is the honest fallback there — it is what the specification measured.
/// Below the floor it is also the answer: [`SHRINK`] concedes nothing, so a
/// smaller surface is not a state this screen can be dragged into.
fn window_size() -> (u32, u32) {
    // ★★★★★ R1700 — the framework's policy, not this screen's copy of it.
    //
    // R1671 fixed the two-sizes defect here by putting the size in this
    // screen's own state and reading it through a weak handle off a view scope.
    // That was correct and it was a SECOND spelling: the node lab read the
    // framework's record instead, and the capture viewer had a third that was
    // simply wrong. Three versions of one policy, one of them defective, is
    // what `layout_size` exists to end — and it also removes the reason
    // `ShellState::surface` had to be read from two routes.
    pinion_core::external::layout_size(VIEW_TAG, SHRINK.comfortable(), (WIN_W, WIN_H))
}

/// The live surface width, and height.
fn win_w() -> u32 {
    window_size().0
}

fn win_h() -> u32 {
    window_size().1
}

const VIEW_TAG: &str = "analyzer_shell";
const THEME_TAG: &str = "app";
const STATE_KEY: &str = "hello-analyzer-shell/state";
/// R1826 — the declared OS-window topology. See [`use_shell_windows`].
const WINDOWS_KEY: &str = "hello-analyzer-shell/windows";
/// R1826 — the subscription that keeps the topology equal to what is detached.
const WINDOWS_EFFECT_KEY: &str = "hello-analyzer-shell/windows-sync";
const TRANSPORT_KEY: &str = "hello-analyzer-shell/transport";
/// R1776 — the cache key the toast's clock registers under, so it registers once.
const TOAST_LIFE_KEY: &str = "hello-analyzer-shell/toast-life";

/// The replay window a scrub moves through, in seconds.
const REPLAY_SECS: f32 = 12.0;

// --- Shell metrics -----------------------------------------------------------
//
// The three the reference tool states as constants — twelve columns, a fixed
// row height and a gutter — are the same here, because a board's arrangement is
// only portable between two tools that agree on what a cell is.

const APP_BAR_H: u32 = spec::APP_BAR_H;
const SUB_BAR_H: u32 = spec::SUB_BAR_H;
const RAIL_W: u32 = spec::RAIL_W;
const PALETTE_W: u32 = spec::PALETTE_W;
const PALETTE_STRIP_W: u32 = spec::PALETTE_STRIP_W;
const GRID_COLS: u32 = spec::GRID_COLS;
const ROW_H: u32 = 174;
const GAP: u32 = 16;
/// A card's header strip.
const CARD_HDR: u32 = 34;
/// (R1668) The decode card's byte pane: the width it prefers, and the narrowest
/// it can be and still show an offset plus one byte. Below the floor the pane
/// is dropped rather than drawn outside the card.
const BYTES_W: u32 = 148;
const BYTES_FLOOR: u32 = 66;
/// The identifier map's two fixed columns and the narrowest its resource path
/// can be. ★ R2022 lifted them out of `map_body` so [`map_column_widths`] —
/// which the paint and the accessibility tree both ask now — states the rule
/// once.
const MAP_ID_W: u32 = 34;
const MAP_SEEN_W: u32 = 66;
const MAP_PATH_FLOOR: u32 = 40;
/// (R1668) A decode row's value column, and the narrowest its key can be. The
/// key is allocated first: a row that lost its name reads as a value belonging
/// to nothing.
const VALUE_W: u32 = 74;
const KEY_FLOOR: u32 = 30;
/// ★ R1876 — the height of one row of a decode card, tree side and byte side
/// alike.
///
/// Lifted because it was `const ROW_H: u32 = 19;` declared **twice**, once
/// inside `decode_body` and once inside `byte_pane` — two panes that are read
/// side by side and have to line up, agreeing by coincidence. The two were
/// equal, which is the only reason nobody noticed; a change to one would have
/// staggered the bytes against the tree they annotate.
const DECODE_ROW_H: u32 = 19;
/// ★★★★★ R2022 — the heading strip and one data row of the STREAM card, and of
/// the IDENTIFIER MAP card.
///
/// Lifted for [`DECODE_ROW_H`]'s reason one step further on: they were declared
/// inside each painter, so the only thing that could know how many rows a body
/// draws was that painter's own loop — and the function that tells a reader what
/// the card says counted the specification's table instead. Five bodies did, and
/// a reader was told about rows nobody drew (see
/// [`whole_rows_in`](pinion_core::containment::whole_rows_in)). Out here both
/// readers can ask.
const STREAM_HEAD_H: u32 = 20;
const STREAM_ROW_H: u32 = 20;
const MAP_HEAD_H: u32 = 18;
const MAP_ROW_H: u32 = 18;
/// The padding band a decode card's first tree row and first byte line sit
/// below.
const DECODE_TOP: u32 = 4;
/// (R1668) A filter stat tile: its height, and the narrowest it can be and
/// still hold a number. The three go or stay together.
const STAT_H: u32 = 46;
const STAT_FLOOR: u32 = 52;
/// The size-stepper strip layout-edit mode adds to the foot of every card.
const EDIT_BAR_H: u32 = 26;
/// A detached panel's opening size, and the cascade between successive ones.
const FLOAT_W: u32 = 520;
const FLOAT_H: u32 = 380;
const FLOAT_STEP: u32 = 30;
/// R1697 — the floor a resize clamps at, from the reference's own source.
///
/// Not a guess and not a fraction of the opening size: the reference writes
/// `Math.max(320, …)` and `Math.max(220, …)` literally, and a panel smaller
/// than that cannot show its header controls beside its title.
const FLOAT_MIN_W: u32 = 320;
const FLOAT_MIN_H: u32 = 220;
/// The corner a resize is grabbed by, square, inside the panel's bottom right.
const FLOAT_GRIP: u32 = 16;

/// R1662 — the input-router tag the board's scrolling body answers to.
const CANVAS_SCROLL: &str = "shell.canvas.body";

/// ★★★★★ R1762 — the preferences page's own scrolling viewport.
///
/// The reference's page scrolls (`overflow:auto` on its body) and this one did
/// not, which held while the page was four short cards. Measured the moment the
/// reference's own rows were built: the appearance group landed at y=872 in a
/// region ending at y=900 and its build strip at y=958, so the last group's
/// controls were painted where no press could reach them — the gate caught it
/// as a segment that would not change the theme.
const SETTINGS_SCROLL: &str = "shell.settings.body";

/// ★ R1851 — the words `sort_alarms` takes for a column.
///
/// A closed vocabulary, and tied to its definition by a gate rather than by
/// hand: `r1851_the_declared_vocabularies_are_their_definitions` asserts these
/// are exactly [`spec::ALARM_COLUMNS`]' headings, lowercased, so a column added
/// there and not here is a failing test instead of a verb a client cannot reach.
const ALARM_COLUMN_KEYS: &[&str] = &["severity", "time", "event"];

/// ★ R1851 — the words `sort_alarms` takes for a direction.
///
/// The framework's own sort vocabulary plus the word for *unsorted*. Held to
/// `sort_dir_str` / `sort_dir_from_str` by the same gate: a declaration spelled
/// differently from what the parser accepts is a client told the wrong thing.
const ALARM_DIRECTIONS: &[&str] = &["ascending", "descending", "none"];

/// ★ R1851 — the words `filter_alarms` takes.
///
/// ★★★★★ R2021 — the list itself moved to [`spec::ALARM_FLOORS`], because a
/// second surface reads it now: the card's own settings row offers exactly these
/// words to a person, and the wire declares exactly these words to a client. A
/// vocabulary two surfaces publish belongs with the specification rather than
/// beside one of them.
use spec::ALARM_FLOORS;

/// ★ R1851 — the alarm feed's own scrolling viewport.
///
/// A card's id carries its index on the board (`alarms#6`) and a `ScrollState`
/// tag is `&'static`, so this names the KIND rather than the placement. That is
/// exact rather than convenient: the board places one alarms card, and the gate
/// that asserts no kind is placed twice is what keeps it exact.
const ALARM_SCROLL: &str = "card.alarms.feed.scroll";

const FONT_TITLE: u32 = 13;
const FONT_BODY: u32 = 12;
const FONT_SMALL: u32 = 11;
const FONT_TINY: u32 = 10;

/// The clearance the status band keeps above and below its sentence.
const STATUS_PAD_Y: u32 = 5;
/// The clearance the status band keeps at each end.
const STATUS_PAD_X: u32 = 16;

/// ★★★★★ R1865 — **the face EVERY sentence in the status band is set in.**
///
/// One face, and it is what makes "the band can hold what the band says" a
/// sentence rather than a hope: the height comes from this face
/// ([`status_band_h`]), the slot comes from this face
/// ([`status_slot_rect`]), and both the gesture strip and the toast are set in
/// it. A band whose messages were set in two faces would need the taller one to
/// size it and would then have to be checked against every message it can ever
/// hold — the shape R1864 spent its round removing from this strip's height,
/// one axis over.
const STATUS_FACE: u32 = FONT_SMALL;

/// ★★★★★ R1864 — **the host's status band, and the mirror of the application
/// bar.**
///
/// The window's chrome is now symmetric: a full-width bar at the top, a
/// full-width band at the bottom, the rail between them on the left, and the
/// page in what is left. Everything this host says *about* the screen — the
/// gesture sentence and, since R1865, the toast — is said here, in host space,
/// so it can never be said on top of a guest.
///
/// # The defect it comes from
///
/// The gesture strip had no band to sit in and was placed by three numbers
/// that answered about a different screen: a hand-picked `+610`, the
/// **dashboard's** palette, and the window's bottom edge. Measured at R1864 by
/// painting all six open destinations and counting what the strip's rectangle
/// intersected: **seven text runs across three of them** — four in the capture
/// view's reassembly lanes, two in the node lab's validation panel, one in the
/// settings page — and at all six it lay *inside* [`page_rect`], which is the
/// guest's rectangle and not the host's to draw in. A reader reported it as
/// "that text keeps overlapping other UI elements", three times.
///
/// # Its height is derived, and that is the second half of the same report
///
/// The strip's own box was authored 14 pixels tall for an 11-pixel face, which
/// `pinion_core::containment::line_box` says needs 18 — so the sentence was
/// four pixels short of holding itself, and R1863's runtime warning named it on
/// the first frame anybody ran it against. A band sized from the face cannot be
/// short: the number the reservation needs is the number the band is built
/// from.
fn status_band_h() -> u32 {
    pinion_core::containment::line_box(STATUS_FACE) + STATUS_PAD_Y * 2
}

/// Where that band sits: the full width of the window, along its bottom edge.
fn status_band_rect() -> Rect {
    let h = status_band_h();
    Rect::new(0, win_h().saturating_sub(h), win_w(), h)
}

/// ★★★★★ R1865 — **the band's one message slot**, seated on its centre line.
///
/// One slot and not two, because that is what a status band IS: the reader
/// looks in one place, and what is there is whatever the application most
/// recently had to say. The gesture sentence lives here when nothing else does;
/// a toast takes it over for as long as it lives and hands it straight back.
///
/// # Why one place beats the previous arrangement, measured
///
/// R1861 made the floating toast AVOID what a screen had under it, which fixed
/// the covering and bought it with unpredictability — and a reader saw the bill
/// before anybody asked: *"it isn't covering anything, but the toast is in a
/// different place on the packet view."* Measured at R1865 across the six open
/// destinations, one sentence, one window size: the box landed at **three
/// different heights, 96 pixels apart** (838 / 804 / 742), and at NONE of them
/// was it where the behaviour reference puts it. A toast lives 2.6 seconds; the
/// property that makes one findable in that time is being in the same place
/// every time.
///
/// # What this rectangle inherited, and what each term of it cost
///
/// It was the gesture strip's, and every one of them has been paid for:
///
/// * ★★ R1701 — its WIDTH was a flat `470` at a flat offset, and adding one
///   gesture to the sentence pushed it past that number: the strip read
///   "… Esc restor…" in a window with room to spare. Third time this project met
///   a width chosen at the design size and required to keep a relation to
///   something that moves (R1687's launch floor, R1700's node-lab hint, this).
/// * ★★★★★ R1864 — **its PLACE was not derived, for 163 rounds.** The three
///   terms that put it on the window were `canvas_rect().x + 610`, the
///   **dashboard's** palette, and `win_h() - 47`: a hand-picked constant, a
///   panel five of the six destinations do not have, and the window's bottom
///   edge — which a mounted guest fills. R1861 registered this strip as a band
///   the toast must avoid and never asked whether the band was itself in the
///   right place; measuring that gave seven runs of other people's text under
///   it.
/// * ★ R1865 — and then the gesture strip stopped having a rectangle of its
///   own. A second name for one place is a second thing that can disagree about
///   where the application speaks.
fn status_slot_rect() -> Rect {
    let band = status_band_rect();
    pinion_core::containment::line_rect_in(
        band,
        band.x + STATUS_PAD_X,
        band.w.saturating_sub(STATUS_PAD_X * 2),
        STATUS_FACE,
    )
}

thread_local! {
    /// ★★★★★ R1903 — **an address book to the one state, not a second copy of
    /// anything.**
    ///
    /// This screen's geometry is a set of pure functions with no state in hand,
    /// and `canvas_rect` alone has a dozen callers — one of them `col_pitch`,
    /// which `cell_rect` uses, which everything uses. Threading a placement
    /// through that would turn a dozen pure helpers into state-takers, so the
    /// fact has to be readable from where they stand.
    ///
    /// ⚠ What this is NOT: a mirror of the placement. The sibling screen's
    /// `use_lab_state` warns that a second holder leaves the state's own
    /// untouched, and that warning is about holding the FACT twice. What is
    /// held here is a clone of the same `Rc` the owner's cache holds — two
    /// pointers to one value, so a write through either is a write to the one
    /// signal. There is still exactly one holder.
    ///
    /// ★ The first draft peeked the owner cache instead
    /// (`Owner::cache_get_by_str`), and it was wrong in the direction that
    /// hides: the geometry runs where `Owner::current()` is not the owner that
    /// cached the state — and for the wire, where there is no owner at all — so
    /// the peek answered `None` and every rectangle read the OPENING placement
    /// forever. The toast said the palette had been put away and the panel was
    /// still 292px wide. The in-process gate did not catch it because its owner
    /// is flat: the walk did.
    static SHELL_STATE: RefCell<Option<Rc<ShellState>>> = const { RefCell::new(None) };
}

/// **Where the palette is right now**, as a placement value.
///
/// Reads the address book above without constructing anything: a geometry
/// helper must not build state or register an animation, and this does neither.
/// Before the state exists — which is what a test asking for a rectangle first
/// sees — the answer is where the palette opens. Total, with no `unwrap` on the
/// path.
fn palette_placement() -> EdgePlacement {
    SHELL_STATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .map_or(spec::PALETTE_OPENS, |state| state.palette_at.get())
    })
}

/// ★★★★★ R1903 — **how much room the palette takes on the right**, and the ONE
/// derivation that answers it.
///
/// Measured this round: FIVE sites read `PALETTE_W` as chrome width — the
/// canvas rectangle, the sub bar's rectangle, the sub bar's chip layout, the
/// mounted roster's left inset, and the palette's own rectangle — and each
/// would have gone on subtracting the open width while the panel was folded.
/// That is R1887's lesson on this axis, stated one screen over: *a container
/// derived from a placement while the things around it are nailed to a constant
/// is half a derivation, and half a derivation is stable only until somebody
/// moves the other half.*
///
/// ⚠ The palette's own ROWS keep reading `PALETTE_W`, and that is correct
/// rather than missed: they are laid out in the panel's own space at its OPEN
/// width, and a folded panel does not build them at all.
fn palette_room() -> u32 {
    palette_placement().thickness(PALETTE_STRIP_W)
}

/// The canvas rectangle: everything between the rail and the palette, under
/// both bars and above the status band.
fn canvas_rect() -> Rect {
    Rect::new(
        RAIL_W,
        APP_BAR_H + SUB_BAR_H,
        win_w() - RAIL_W - palette_room(),
        win_h()
            .saturating_sub(APP_BAR_H + SUB_BAR_H)
            .saturating_sub(status_band_h()),
    )
}

/// ★★ R1695 — the rectangle the **paged region** occupies at a destination.
///
/// At the dashboard it is exactly [`canvas_rect`], because the dashboard also
/// paints a layout bar above it and a palette beside it; anywhere else those
/// are not there and the page gets the whole area the rail and application bar
/// leave. A destination-dependent rectangle is what a region is: the page is
/// what the window gives that destination, not a fixed hole in the chrome.
///
/// ★ R1864 — and what the **status band** leaves, at every destination. A band
/// the host draws in and the region also covers is not reserved space, it is a
/// collision waiting for the guest to paint something there — which is exactly
/// what three of the six destinations were doing.
fn page_rect(at: &str) -> Rect {
    // ★★★★★ R2045 — DERIVED from the roster's own declaration, where this
    // computed a second answer beside it. R1830 moved the grant into the roster
    // and left this function as the host's private arithmetic, so what held the
    // two together was one assertion, about one destination, in this host: the
    // same mistake anywhere else was green, and its counterfactual proved it by
    // being caught in exactly one place.
    //
    // Deriving is what this repository prefers over a wider cross-check — the
    // rule is not repaired, it is made unnecessary. What stays checkable is the
    // declaration against the chrome actually painted, which is a different
    // question and the one `r2045` now asks.
    // ⚠ NOT by asking the roster, and that is R1830's measurement rather than a
    // preference: the roster is built inside an `Owner::cache` factory, and a
    // factory closure may not call `Owner::cache` — which `win_w` does. So the
    // shared account is the INSET, which needs no window, and both readers take
    // it from `page_inset` below. The roster grants what this function insets
    // by; the host insets by what the roster grants; there is one number.
    let inset = page_inset(at);
    Rect::new(
        inset.left,
        inset.top,
        win_w().saturating_sub(inset.beside()),
        win_h()
            .saturating_sub(inset.top)
            .saturating_sub(inset.bottom),
    )
}

/// ★★★★★ (R2045) What this shell paints around one destination's page — the
/// one account of it, read by the roster's grant and by [`page_rect`].
///
/// R1830 put the grant in the roster and left the page rectangle here, so the
/// same fact had two accounts and what held them together was a single
/// assertion, about a single destination, in this application's own tests. Its
/// counterfactual is the evidence: collapsing the inset to one value for every
/// section was caught by that assertion and by nothing else, so the same
/// mistake at any other destination was green.
///
/// Deriving both from here is the repair this repository prefers over widening
/// the check — the two numbers become one, and there is no drift for a gate to
/// find. What is still worth asking, and what `r2045` asks, is whether this
/// declaration matches the chrome the host actually paints.
fn page_inset(at: &str) -> PageInset {
    if at == "dashboard" {
        // The dashboard paints a sub-bar above its page and a palette beside
        // it; no other destination paints either.
        return PageInset::new(
            RAIL_W,
            APP_BAR_H + SUB_BAR_H,
            palette_room(),
            status_band_h(),
        );
    }
    PageInset::new(RAIL_W, APP_BAR_H, 0, status_band_h())
}

/// The opening value of each Settings switch, from the specification.
fn opening_options() -> [bool; spec::OPTIONS.len()] {
    let mut opens = [false; spec::OPTIONS.len()];
    for (slot, option) in opens.iter_mut().zip(spec::OPTIONS) {
        *slot = option.opens;
    }
    opens
}

/// One grid column's pitch, gutters included.
fn col_pitch() -> u32 {
    (canvas_rect().w.saturating_sub(GAP)) / GRID_COLS
}

/// The narrowest span any card this board opens with occupies, in columns.
///
/// Read from [`spec::BOARD`] rather than written down: a placement moving from
/// five columns to four moves the floor below with it.
const fn narrowest_span() -> u32 {
    let mut narrowest = GRID_COLS;
    let mut i = 0;
    while i < spec::BOARD.len() {
        if spec::BOARD[i].cols < narrowest {
            narrowest = spec::BOARD[i].cols;
        }
        i += 1;
    }
    narrowest
}

/// ★★★★★ R1784 — **what the dashboard lays out in**, derived rather than
/// chosen.
///
/// The board's own geometry gives every term. A card spanning `n` columns is
/// `n * pitch - GAP` wide ([`cell_rect`]) and the pitch is
/// `(canvas - GAP) / GRID_COLS` ([`col_pitch`]), so the canvas the narrowest
/// card needs follows from one number: how narrow a card may be at all.
///
/// That number is not invented here either. [`FLOAT_MIN_W`] is what a card
/// clamps to once it is torn off, extracted from the reference's own source at
/// R1697 — and a card and its float hold the same content, so a card that is
/// legible detached and illegible in place would be this shell disagreeing with
/// itself about one thing.
const fn board_canvas_floor() -> u32 {
    // `n * pitch - GAP >= FLOAT_MIN_W`, rounded up so the floor is a width the
    // inequality actually holds at rather than one it is just short of.
    let pitch = (FLOAT_MIN_W + GAP).div_ceil(narrowest_span());
    pitch * GRID_COLS + GAP
}

/// The dashboard's floor, as a width of its PAGE REGION.
///
/// ★★★★★ R1784 — the first draft of this added [`RAIL_W`] and [`PALETTE_W`],
/// on R1761's measurement that a host paints a page's chrome beside the region
/// so the SECTION is wider than the region. That measurement is true and it is
/// not what this constant is: a [`ShrinkPolicy`] is what `page_scene` applies
/// to the region, so a number about the section would be compared with the
/// region's width and read as a shortfall of exactly the chrome. The gate's
/// first run said so — `("dashboard", 1176, 1096)`, and 1176 - 1096 is
/// `RAIL_W + PALETTE_W` less the rounding.
///
/// So the section's extra width is stated where it belongs, in the gate that
/// asserts the dashboard's region is narrower than a mounted screen's, and
/// this stays the region's own floor.
const DASHBOARD_MIN_W: u32 = board_canvas_floor();

/// The dashboard's height floor: the bars this host keeps, plus one board row.
///
/// One row rather than the four the opening layout places — the board scrolls,
/// and a floor that demanded the whole opening layout would be pinning the
/// layout rather than the page.
const DASHBOARD_MIN_H: u32 = APP_BAR_H + SUB_BAR_H + ROW_H + GAP;

// --- The widget catalogue ----------------------------------------------------
//
// The table itself is in `spec.rs` — the reference's own catalogue written down
// as a value, thirteen entries of which four the first release places and nine
// it reserves. What stays here is what is about PAINT rather than about the
// screen: the colour a kind is identified by, and the header controls a card
// carries. Everything else is read from the specification, so this file cannot
// drift from it without failing the gate in `painted.rs`.

use CardAffordance::{Close, Maximize, Settings, TearOff};

/// Every placed card carries the same header controls.
///
/// The specification names them and this maps the names onto the framework's
/// vocabulary, so a control added there arrives here rather than being
/// remembered: a uniform board is what makes a missing control legible, where a
/// board of individually-decided chrome is a board nobody can check.
fn chrome() -> Vec<CardAffordance> {
    spec::CARD_CHROME
        .iter()
        .map(|name| match *name {
            "settings" => Settings,
            "tear_off" => TearOff,
            "maximize" => Maximize,
            "close" => Close,
            other => panic!("the specification names a header control {other:?} this shell has no affordance for"),
        })
        .collect()
}

/// A colour from a packed `0xRRGGBB`, so the reference's token table can be
/// transcribed exactly as it is written rather than as three decimals each.
#[allow(
    clippy::cast_possible_truncation,
    reason = "each mask isolates one byte, which is what the cast takes"
)]
const fn rgb(hex: u32) -> Color {
    Color::rgb(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

/// The colour a kind is identified by — on its palette swatch and on its card.
///
/// Keyed by the specification's kind rather than stored beside it: a colour is
/// a decision about ink and the specification is a statement about the screen,
/// and this round's whole point is that the two are not the same document.
fn kind_color(kind: &str) -> Color {
    match kind {
        "packet" => rgb(0x2D_6C_DF),
        "decode" => rgb(0x8A_5C_F6),
        "keymap" => rgb(0xC7_78_00),
        "filter" => rgb(0x1F_8A_4C),
        "topology" => rgb(0x9A_00_4F),
        "overlay" => rgb(0xB0_33_5B),
        "throughput" => rgb(0x0E_9A_A7),
        "share" => rgb(0x35_C0_8B),
        "latency" => rgb(0xD3_3A_2C),
        "health" => rgb(0x4A_7A_9B),
        "loss" => rgb(0xA9_4F_10),
        "alarms" => rgb(0x7A_4B_C4),
        "admin" => rgb(0x2F_6B_6B),
        _ => rgb(0x6B_72_80),
    }
}

/// The cells a kind occupies when it is placed, from the opening board.
///
/// A reserved kind has no size because it is never placed, which is why this
/// answers `None` for one rather than inventing a default that no screen has
/// ever painted.
fn kind_span(kind: &str) -> Option<(u32, u32)> {
    spec::BOARD
        .iter()
        .find(|p| p.kind == kind)
        .map(|p| (p.cols, p.rows))
}

/// The catalogue entry for a kind.
fn def_of(kind: &str) -> Option<&'static spec::WidgetSpec> {
    spec::widget_of(kind)
}

/// The sources the application bar offers. The first is the one the
/// specification states the screen opens on; the others are what the control
/// cycles to, which the specification does not fix.
///
/// No real address belongs in a repository, so these are documentation
/// addresses (RFC 5737 TEST-NET-1).
const SOURCES: [&str; 3] = [
    spec::SOURCE,
    "lo \u{00B7} 127.0.0.1:7447",
    "file \u{00B7} session-2.capture",
];

/// The view tabs the application bar carries, by title.
///
/// ★★★★★ R1946 — derived from [`spec::VIEW_TABS`] rather than written here.
/// This used to be a two-element constant array of titles in this file, the
/// only navigable surface on this screen with no declaration behind it, and its
/// titles were spelled a second time inside the hit-test chip enumeration.
/// There is one list now, and that the constant no longer exists is the point:
/// a second list is what the two halves of this bar could disagree through.
fn tabs() -> Vec<&'static str> {
    spec::VIEW_TABS.iter().map(|tab| tab.title).collect()
}

/// A card's id is `<kind>#<n>` — the kind so the definition is recoverable
/// without a side table, the ordinal so a kind can be placed more than once.
fn kind_of(id: &str) -> &str {
    id.split_once('#').map_or(id, |(kind, _)| kind)
}

fn def_for_card(id: &str) -> Option<&'static spec::WidgetSpec> {
    def_of(kind_of(id))
}

fn label_of(id: &str) -> String {
    def_for_card(id).map_or_else(|| id.to_string(), |d| d.label.to_string())
}

/// The sparkline the filter card draws under its counts: how many messages
/// matched, over the recent past.
/// What the filter card's sparkline is a trend OF — the first of
/// [`spec::FILTER_STATS`]'s three counts, over the recent past.
///
/// ★ R1824 — this is the name a `Selection::Category` is matched against, so it
/// must be the measure's name and NOT a saved filter's: the whole point is that
/// under any saved filter this trend is a trend of something else, and dims to
/// context. Taken from the stat row it belongs to rather than written twice.
const MATCH_SERIES_OF: &str = spec::FILTER_STATS[0].1;

const MATCH_SERIES: [f64; 12] = [
    4.0, 6.0, 5.0, 9.0, 7.0, 12.0, 10.0, 14.0, 11.0, 15.0, 13.0, 17.0,
];

/// ★★★★★ R2019 — **the authored palette, as the design system emits it.**
///
/// Committed rather than read at run time, for the reason every other spec
/// document in this tree is (`analyzer-rail-spec`, `analyzer-keys-spec`,
/// `analyzer-config-surface`): a demo binary is launched from several
/// directories and a run-time read turns a missing file into *the colours look
/// wrong* rather than into a refusal.
///
/// It is the emitted document unchanged — colour VALUES, which are a result,
/// and no vocabulary from where they came.
const AUTHORED_LIGHT: &str = include_str!("../../../docs/analyzer-palette.light.json");
/// The dark half of [`AUTHORED_LIGHT`].
const AUTHORED_DARK: &str = include_str!("../../../docs/analyzer-palette.dark.json");

/// The authored palettes as this screen paints them, each with the roles its
/// document left to the framework.
///
/// ★★★★★ R2019 — **this is the crossing, and until now nobody had made it.**
/// A design system authored for this project has emitted these two documents
/// for a long time and every colour on this screen was a hand-transcribed
/// copy: measured at R2017 the copy and the source disagreed on TWENTY-THREE of
/// thirty-eight role-and-mode pairs, which is what a second copy always
/// eventually does. The document is now the source and the screen reads it.
///
/// ⚠ **Partly authored, and the gap says so rather than hiding.** Each
/// document binds nineteen of twenty-three roles — the four it leaves are ones
/// this vocabulary grew after the exporter was written — so those four keep the
/// framework's answer, and [`Theme::adopt`] returns their names. A silent
/// fallback would make *some of this screen is not authored* something a person
/// has to notice.
fn authored_palettes() -> ((Theme, ThemeGap), (Theme, ThemeGap)) {
    let light = Theme::light()
        .adopt(AUTHORED_LIGHT)
        .expect("the committed light palette is a palette document");
    let dark = Theme::dark()
        .adopt(AUTHORED_DARK)
        .expect("the committed dark palette is a palette document");
    (light, dark)
}

/// The palettes this screen binds, light then dark.
fn reference_palettes() -> (Theme, Theme) {
    let ((light, _), (dark, _)) = authored_palettes();
    (light, dark)
}

/// The canvas's dot-grid colour, which is not a theme role: it is a hairline
/// that must read as *below* the surface rather than on it.
fn grid_ink(dark: bool) -> Color {
    if dark {
        rgb(0x20_24_2C)
    } else {
        rgb(0xE4_E8_ED)
    }
}

// --- State -------------------------------------------------------------------

/// ★★★★★ R1826 — the OS window that carries the card detached as `id`.
///
/// ONE definition, read by three things that must agree exactly or the arc
/// breaks in a way no single one of them can see: the [`WindowSpec`] the
/// topology mints, [`ShellState::detached`]'s published answer, and — through
/// [`FLOAT_WINDOW_PREFIX`], which is the same spelling read backwards — the
/// `view_for_window` arm that paints it. A second spelling anywhere would open a
/// window nothing paints, or paint into a window nothing opened.
///
/// 🟥 This said FOUR, naming "the accessibility contribution for that window" as
/// the fourth. There is no such thing: `WidgetView` has `windows_signal` and
/// `view_for_window` and no per-window accessibility hook at all, so the count
/// was of an arrangement that does not exist. A window's accessibility here
/// comes from the nodes [`body_scene`] already attaches, which travel with the
/// scene and read no id.
fn float_window_id(card: &str) -> String {
    format!("{FLOAT_WINDOW_PREFIX}{card}")
}

/// The prefix that marks a window as a detached card's, and separates the
/// card's id from it. Chosen not to collide with `"main"`.
const FLOAT_WINDOW_PREFIX: &str = "torn-";

/// A detached panel: a card that has left the board and floats over it.
///
/// Serialisable because it lives in a `Signal`, and because a session saved
/// with a panel torn off must reopen with it torn off — the same reason the
/// arrangement is serialisable.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Float {
    id: String,
    x: u32,
    y: u32,
    /// ★★★★★ R1697 — the panel's own size, which it did not have.
    ///
    /// The width and height were module constants, so every detached panel was
    /// the same size for ever and the corner could not be pulled. The reference
    /// gives each float its own `w`/`h` and a resize that clamps at
    /// [`FLOAT_MIN_W`] x [`FLOAT_MIN_H`] — measured by extracting its own
    /// source rather than inferred from the mockup.
    w: u32,
    h: u32,
    /// Which panel is in front, when two overlap.
    ///
    /// It was the vector's order, read backwards at the hit test — which works
    /// exactly until something has to change it, and bringing a panel forward
    /// is the first thing a press on one must do. Explicit, from a monotonic
    /// counter, because the reference's `raiseFloat` is what its drag calls
    /// first and the two are one gesture.
    z: u32,
    /// ★★★★★ R1826 — whether this card's window stays above the application.
    ///
    /// The **option** half of the specification's multi-window clause — *tear
    /// off -> independent window, always-on-top option*. Off by default,
    /// because a window that arrives on top of everything is a decision a
    /// reader makes about one panel and not a behaviour they discover.
    ///
    /// Per FLOAT rather than per application: the whole point is watching ONE
    /// readout over other work, and an application-wide switch would put the
    /// packet stream on top to keep an eye on a latency chart.
    ///
    /// `#[serde(default)]` so a session saved before this field existed still
    /// loads — the same tolerance the arrangement's other additive fields take.
    #[serde(default)]
    on_top: bool,
    /// ★★★★★ R1891 — **where this panel lives**, and the reason it is a field
    /// on the float rather than a switch on the application.
    ///
    /// Measured at R1891 on the running assembled tool: tearing one card off
    /// left `windows: ["main", "torn-packet#0"]` AND five `float.packet#0…`
    /// regions painted in the main window. Two pictures of one card, from two
    /// models that did not track each other — the window topology keyed on
    /// which windows exist, the float carrying live geometry.
    ///
    /// Every reader of a float now derives from this: the window topology takes
    /// the [`DetachHome::Window`] ones, the canvas paint and the hit test take
    /// the [`DetachHome::Canvas`] ones, and `detached` reports the window ones
    /// because those are the only ones that HAVE a window. A card can no longer
    /// be in both places because the value cannot say both.
    ///
    /// Per float rather than per application, for [`on_top`](Self::on_top)'s
    /// reason: which panel a reader wants out on the desktop and which they
    /// want kept over the board is a decision about ONE panel.
    ///
    /// `#[serde(default = "default_detach_home")]` so a session saved before
    /// this field existed reopens with its panels where that build put them —
    /// which was a window, alongside a canvas float this round removes.
    #[serde(default = "default_detach_home")]
    home: DetachHome,
}

/// Where a float loaded from a session that predates the field goes.
///
/// Not [`DetachPolicy::preferred`], because that is a function of the host and
/// this is a function of HISTORY: builds before R1891 opened a window for every
/// detached card, so a session written by one is describing windows.
fn default_detach_home() -> DetachHome {
    DetachHome::Window
}

/// ★★★★★ R1897 §3 §5.15 — where a person's own arrangements are kept between
/// runs, and the ONE key they live under.
///
/// The behaviour canon persists two things in `localStorage`: the custom
/// presets, and the current layout with its name. This is the first half — the
/// arrangements — and it stores `Workspaces::saved()`, which is a person's own
/// and nothing else: the four this application ships come back by BEING
/// shipped, and storing them would let a later build find a previous version's
/// on disk and resurrect it.
///
/// One key, so `FileStorage`'s tempfile-and-rename covers the whole write; a
/// half-written set is a set that reopens missing arrangements.
const STORAGE_APP: &str = "pinion-analyzer-shell";
const STORAGE_CACHE_KEY: &str = "analyzer_shell.storage";
const ARRANGEMENTS_KEY: &str = "analyzer_shell.arrangements";

/// Bump on an incompatible stored shape. A mismatch starts from the shipped
/// set rather than misreading — the todomvc / node-editor precedent, and the
/// reason the blob carries a version at all.
const ARRANGEMENTS_VERSION: u32 = 1;

/// The stored form: a version and the person's own arrangements.
///
/// A struct rather than the bare set, because a bare set has nowhere to put the
/// version and "this file is older than this build" then reads as a corrupt
/// file. Named fields so a reader of the JSON can tell what it is looking at.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredArrangements {
    version: u32,
    arrangements: Workspaces<Preset>,
    /// ★★★★★ R1908 — **where this person left the chrome panels**, keyed by the
    /// panel's own tag.
    ///
    /// A map rather than a field per panel, so a second chrome panel needs no
    /// change of shape here and no version bump: an unknown key is a panel this
    /// build does not have, and a missing key is a panel nobody has moved. Both
    /// are ordinary, and neither is a corrupt file.
    ///
    /// `#[serde(default)]` because a file written before this round is not
    /// older-and-broken, it is older-and-fine — the version exists for shapes
    /// that cannot be read, and adding a field is not one. A bump here would
    /// have thrown away every saved arrangement on this build's first run.
    #[serde(default)]
    chrome: BTreeMap<String, EdgePlacement>,
}

/// The palette's key in [`StoredArrangements::chrome`].
///
/// The panel's own tag, so what is on disk can be matched to what is on screen
/// by a person reading both — the reason this file stores named JSON at all
/// rather than the opaque byte string the floor toolkit round-trips a docked
/// arrangement through.
const PALETTE_STORE_KEY: &str = "shell.palette";

/// What this host can do with a detached card.
///
/// `true` because this binding publishes a window topology — `use_shell_windows`
/// hands the shell a `Vec<WindowSpec>` and the shell opens what is in it. That
/// is not a claim this comment makes on its own: `r1891`'s walk tears a card
/// off and asserts a window actually appears, so the capability and the policy
/// are checked against each other in the running application rather than
/// agreeing here on paper.
///
/// A terminal backend would answer `false` and get [`DetachHome::Canvas`], which
/// is why the choice exists at all (§2 #6) — this file is the GUI half of a
/// dual-dispatch framework, not the only half.
fn detach_policy() -> DetachPolicy {
    DetachPolicy::for_host(true)
}

/// ★★★★★ R1905 — **the relation between the two spaces a detached card can
/// live in**, built from what this host can actually say about itself.
///
/// A window-homed card's rectangle is in the display's coordinate space and a
/// canvas-homed one's is in this window's. R1891 gave the card a home and left
/// the geometry alone, and the result was measurable on the running tool: the
/// identical `(120, 40)` served as a display coordinate while the card was a
/// window and as a host coordinate the moment it was sent to the canvas, so a
/// reader watched the panel jump by however far the window manager had placed
/// this window from the corner.
///
/// [`pinion_core::external::window_origin`] is the fact that was missing —
/// measured at R1905, `scene/windows` answered `position: None` for this
/// window, because the manager placed it and the shell never commanded one, and
/// nothing else published where it was.
///
/// ⚠ **`adrift` is not a fallback that hides anything.** A host that cannot say
/// where it is gets a transfer that says the crossing was unconverted, and the
/// wire publishes that word — which is strictly more than the silence this
/// replaces. The walk asserts this host is NOT adrift, so the honest arm cannot
/// quietly become the only one that ever runs.
/// ⚠ **The host space is the CANVAS's, not the window's** — found by this
/// round's own gate, which asked the running screen's hit test for the panel it
/// had just placed and did not get it. `Hit::at` folds the canvas origin out
/// before it reads the floats (`in_canvas(state, px - canvas.x, py - canvas.y)`),
/// so a float's stored pair is in the canvas's own frame and is short of the
/// window's by the rail, the palette and the two bars. Converting against the
/// window's origin would have landed every crossed panel that much off — the
/// [[debt-paint-and-gesture-read-two-facts]] class, one space up — written with
/// this file's own wiki-link spelling and not with rustdoc's, because a memory
/// slug is not a Rust path and `[` … `]` around one asks rustdoc to resolve it.
///
/// ⇒ ★ *The offset between two spaces is only right if you named the right two.*
fn shell_transfer() -> pinion_core::detach::Transfer {
    let canvas = canvas_rect();
    let host = (canvas.w, canvas.h);
    match pinion_core::external::window_origin(MAIN_WINDOW) {
        // The display's origin of the CANVAS: where the window is, plus where
        // the canvas starts inside it.
        Some((wx, wy)) => pinion_core::detach::Transfer::new(
            (
                wx.saturating_add(i32::try_from(canvas.x).unwrap_or(0)),
                wy.saturating_add(i32::try_from(canvas.y).unwrap_or(0)),
            ),
            host,
        ),
        None => pinion_core::detach::Transfer::adrift(host),
    }
}

/// The store a person's own arrangements are kept in.
///
/// ⚠ **Resolved OUTSIDE any `Owner::cache` factory and then held by
/// [`ShellState`].** `use_app_storage` is itself an `Owner::cache`, and this
/// framework refuses a cache factory that re-enters the cache — measured this
/// round, the first draft called it from inside the state's own factory and the
/// binding panicked at boot with `Re-entering on key="analyzer_shell.storage"`.
/// ⇒ ★ a hook is not a plain function: calling one from inside another's
/// factory is a different act from calling it beside one, and only the second
/// is allowed. Pre-resolving also means the state OWNS its dependency rather
/// than reaching back for it at every write.
fn arrangement_storage() -> Rc<pinion_platform_storage::AppStorage> {
    pinion_platform_storage::use_app_storage(STORAGE_CACHE_KEY, STORAGE_APP)
}

/// ★★★★★ R1897 — write a person's own arrangements, and only those.
///
/// Called after every operation that changes the SET (save, delete). Not after
/// applying one: applying changes which arrangement is current, which is a
/// different fact and is not this key's.
///
/// Best effort, as [`pinion_core::storage::Storage`] is: a failed write is a
/// silent no-op by that trait's contract, and this application treats
/// persistence as a capability rather than a guarantee.
fn persist_arrangements(state: &ShellState) {
    let stored = StoredArrangements {
        version: ARRANGEMENTS_VERSION,
        arrangements: state.presets.borrow().saved(),
        // ★★★★★ R1908 — and where the person left the chrome. Written on the
        // same key and in the same call, because a set of arrangements and the
        // panel beside them are one session: two keys would let a crash leave a
        // board from today next to a palette from yesterday.
        chrome: BTreeMap::from([(PALETTE_STORE_KEY.to_owned(), state.palette_at.get())]),
    };
    if let Ok(bytes) = serde_json::to_vec(&stored) {
        state.storage.save(ARRANGEMENTS_KEY, &bytes);
    }
}

/// Lay what a previous run saved over the arrangements this build ships.
///
/// ★ Every refusal is SAID, not dropped: a row whose name this build now ships
/// and a row claiming to be a built-in are both refused by
/// [`pinion_core::workspace::Workspaces::restore`], and a restore that
/// discarded them silently would be one nobody could debug. They reach the
/// person through the same utterance channel every other refusal here does.
///
/// A version mismatch starts from the shipped set rather than misreading —
/// stated rather than silent, for the same reason.
fn restore_arrangements(state: &ShellState) {
    let Some(bytes) = state.storage.load(ARRANGEMENTS_KEY) else {
        return;
    };
    let Ok(stored) = serde_json::from_slice::<StoredArrangements>(&bytes) else {
        state.say(Utterance::done(
            "saved layouts could not be read; starting from the ones this build ships",
        ));
        return;
    };
    if stored.version != ARRANGEMENTS_VERSION {
        state.say(Utterance::done(format!(
            "saved layouts are version {} and this build reads {ARRANGEMENTS_VERSION}; \
             starting from the ones it ships",
            stored.version
        )));
        return;
    }
    let mut presets = state.presets.borrow_mut();
    let refused = presets.restore(stored.arrangements);
    drop(presets);
    for refusal in &refused {
        state.say(Utterance::done(refusal.reason().to_string()));
    }
    // ★★★★★ R1908 — and the chrome the person left, JUDGED before it is used.
    //
    // This is where `EdgePlacement::folded_at` becomes reachable at all: no
    // specification in this tree opens a panel folded — the behaviour canon
    // opens its palette showing, and R1902 measured that opening folded would
    // un-reproduce it — so a folded panel is not something a build declares, it
    // is something a person did and came back to.
    //
    // Through the policy, because a stored placement is the one input here that
    // did not come from this build: an older version wrote it, this build may
    // have narrowed the panel's range since, and a person can edit the file.
    // `EdgePolicy::restore` asks exactly what an opening is asked and hands
    // back a place either way, so a boot cannot fail over a remembered width.
    if let Some(stored_at) = stored.chrome.get(PALETTE_STORE_KEY) {
        let restored = spec::PALETTE_POLICY.restore(*stored_at, spec::PALETTE_OPENS);
        state.palette_at.set(restored.at());
        state.palette_restored.set(restored.believed());
        if let Some(why) = restored.refused() {
            // Said, never dropped: a reader who folded this panel yesterday and
            // finds it open today is owed the sentence. A silent fallback is
            // R1902's defect one step on — the state is judged now, and the
            // judgement reaches nobody.
            state.say(Utterance::done(format!(
                "the palette could not open where you left it \u{2014} {}",
                why.reason()
            )));
        }
    }
}

/// A detached panel being moved or resized, in flight.
///
/// A separate type from the board's [`TileDrag`] rather than an arm added to
/// it, because the two live on **different planes**: a card is dragged between the board's
/// cells and previews where it would land, and a panel is dragged in pixels and
/// moves as the pointer moves — which is what the reference does, and what a
/// window does. Folding a pixel gesture into a type whose fields are a cell
/// coordinate and a snap target would give one of them a meaning it cannot
/// carry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct FloatGrab {
    /// Which panel.
    id: String,
    /// Whether the pointer is moving the panel or sizing it.
    edge: bool,
    /// Where the pointer was when the grab opened.
    from: (u32, u32),
    /// The panel's position or size at that moment — the origin every
    /// subsequent delta is added to, so a drag is exact rather than accumulated
    /// (an accumulating drag drifts by one pixel per event that is swallowed).
    origin: (u32, u32),
}

// ★★★★★ R1733 — the board drag in flight is the framework's
// `TileDrag` now, not a struct here.
//
// This file held `Drag { id, dx, dy, snap }`, which could carry only a card the
// board already had. Reproducing the reference's palette gesture meant carrying
// something that is NOT on the board, and the cheap way to do that is a second
// nullable field beside this one — which is precisely how the reference spells
// it, and why each of its handlers has to remember to check the other. Two
// nullable fields can be set at once and that state has no meaning.
//
// ★ Measured in that prototype rather than assumed, and it had already decayed:
// its held-card field is read by TWO guards and assigned a non-null value
// NOWHERE, because the reorder gesture moved onto another field and the guards
// were left behind. The cost of the shape shows up before the forgotten check
// does.
//
// The framework type is one value with two arms, so the check is a `match` and
// the compiler performs it; and its landing is read by both the preview and the
// release, so R1668's "one fact, two clamps" cannot come back on the new drag.

/// A saved layout: the arrangement AND which cards were on it.
///
/// Both, because a board is two facts — where the cells are and what is in
/// them — and a preset restoring only the first would put the previous board's
/// cards into the new layout's holes.
/// ★ R1897 — serialisable, because a person's own arrangements are kept
/// between runs and this is what one IS. Both halves have to persist for the
/// reason the doc above gives: a preset restoring only the cells would put the
/// previous board's cards into the new layout's holes, and a preset restored
/// from disk is no different.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Preset {
    board: TileGrid,
    cards: Vec<Card>,
}

struct ShellState {
    clock: Rc<TransportClock>,
    theme: Rc<ThemeProvider>,
    board: Signal<TileGrid>,
    cards: Signal<Vec<Card>>,
    /// `Some` exactly while a card is maximised. The token IS the way home.
    maximized: Signal<Option<Maximized>>,
    floats: Signal<Vec<Float>>,
    /// ★ R1893 — the framework's named-arrangement set, not a bare map.
    ///
    /// What the map could not carry: WHERE an arrangement came from. This
    /// application ships one and a person saves others, and those are the same
    /// kind of thing to a menu and different kinds of thing to a delete — so a
    /// delete built on the map would have taken both, which is why there was no
    /// delete. See [`pinion_core::workspace`].
    presets: RefCell<Workspaces<Preset>>,
    /// ★ R1897 — where a person's own arrangements are kept, held rather than
    /// looked up. See [`arrangement_storage`] for why it is resolved outside
    /// this state's own cache factory.
    storage: Rc<pinion_platform_storage::AppStorage>,
    preset: Signal<String>,
    preset_open: Signal<bool>,
    /// Layout-edit mode: the size steppers appear and the grid brightens.
    editing: Signal<bool>,
    /// Which card's settings strip is open, if any.
    config_open: Signal<Option<String>>,
    source: Signal<String>,
    capturing: Signal<bool>,
    search: Signal<String>,
    searching: Signal<bool>,
    tab: Signal<String>,
    /// ★★★★★ R1695 — **where the rail has taken you**, and the roster it is
    /// taken from.
    ///
    /// This was a bare `Signal<String>` the rail highlighted itself from, and
    /// nothing else read it. Four of the seven seats therefore "navigated" to a
    /// dashboard: measured through the router, the press moved this string and
    /// left the painted scene at 193 tagged regions before and 193 after.
    ///
    /// A [`Journey`] instead, because the string could hold a destination no
    /// press could reach and had no way to refuse one — and the region below
    /// now reads it, so *arriving* is a fact about the window rather than about
    /// a variable.
    journey: Signal<Journey>,
    /// ★★★★★ R1724 — **the roster, and the screens behind the destinations
    /// that have one.**
    ///
    /// This was a bare [`Destinations`] and the difference is what makes the
    /// analysis tool one application: a destination's page can now be another
    /// binding. Catalog is `hello-node-lab`, mounted whole and unedited, and
    /// its seat stopped saying *built, shipping, and not here*.
    ///
    /// Held rather than rebuilt at each read, for the reason the roster always
    /// was — the paint, the hit test and the wire must not be looking at three
    /// rosters — and now for a second one: a mounted screen's latched
    /// projection lives on the mount, so a roster rebuilt per frame would
    /// forget what the screen was showing every frame.
    screens: ScreenRoster,
    /// The Settings destination's four switches, in specification order.
    ///
    /// One array rather than four signals because the page renders them from a
    /// table and the wire publishes them from the same table; four fields would
    /// be four chances for the two to disagree about the order.
    options: Signal<[bool; spec::OPTIONS.len()]>,
    /// ★★★★★ R1762 — the capture buffer size in effect, from
    /// [`spec::RETENTIONS`].
    ///
    /// Its own signal rather than a second entry in `options`, because it is
    /// not a switch: the reference draws it as a word out of a roster, and a
    /// boolean array with a string in it would be the two encodings of one
    /// state this file has been bitten by before.
    retention: Signal<String>,
    /// ★★★★★ R1762 — which value row's roster is **open**, and where in it the
    /// reader is.
    ///
    /// `None` is closed, and there is deliberately no `open` flag beside a
    /// highlight: *closed and highlighting the fourth option* is a state that
    /// should not be spellable, which is the rule
    /// [`Picker`] itself is built on.
    picking: RefCell<Option<(String, pinion_core::widgets::picker::Picker)>>,
    /// ★★★★★ R1721 — which saved filter the filter card has applied.
    ///
    /// It had no state at all until this round, and that was the defect: the five
    /// chips were painted from a constant table, announced as five operable
    /// toggle buttons, and a pointer press over any of them changed nothing.
    /// Measured by driving the running screen — clicked three of them, read the
    /// tree back, and every `checked` was where it started.
    ///
    /// An `Option<usize>` rather than five booleans because the row declares
    /// at-most-one (`spec::FILTER_ROW`): five booleans would let this file hold a
    /// state the rule forbids, and the rule is what the chips are announced from.
    filter_chip: Signal<Option<usize>>,
    selected: Signal<Option<String>>,
    cursor: Signal<(u32, u32)>,
    /// ★★★★★ R1916 — whether the pointer is in the window at all, which is a
    /// different fact from where it last was.
    ///
    /// A second signal beside `cursor` and not an `Option` inside it, for the
    /// reason the node lab's is: every gesture reading `cursor` wants *the last
    /// place the pointer was* — a drag released outside the window commits
    /// where it left — and only the hover derivations want *is anybody
    /// pointing at anything*. Measured on the running shell at R1916: without
    /// it, a description shown under a resting pointer stayed on the frame
    /// after the pointer left.
    pointer_inside: Signal<bool>,
    pressed: RefCell<Option<Hit>>,
    drag: Signal<Option<TileDrag>>,
    /// ★★★★★ R1735 — **what the router says a release would do right now**, as
    /// the framework handed it over.
    ///
    /// Not derived here, and that is the point. The board's own preview lives on
    /// `drag` above; this is the same judgement travelling back from the router,
    /// so the sentence this screen publishes about a refusal is the framework's
    /// own words and a client asking the wire "can I let go here" reads the
    /// answer the release will act on rather than one this file re-computed.
    standing: RefCell<DropStanding>,
    /// ★★★★★ R1698 — **one keyboard cursor per composite**, keyed by the tag
    /// that owns the Tab stop.
    ///
    /// Held rather than rebuilt because a cursor is a position somebody put
    /// there: rebuilding it every frame would reset it whenever anything else
    /// on the screen changed. Re-seated (rather than replaced) each time it is
    /// read, so a roster that grows keeps the cursor on the member it was on.
    cursors: RefCell<BTreeMap<&'static str, Roving>>,
    /// R1697 — a detached panel being moved or sized, in flight.
    float_grab: Signal<Option<FloatGrab>>,
    /// ★★★★★ R1898 — **which side of the board's edge the gesture in flight
    /// would end on**, and whether ending there moves the card between them.
    ///
    /// The board and the loose space over it are two homes for one card, and
    /// before this field a drag could only end in the home it began in: a card
    /// carried off the board answered [`Dropped::Abandoned`] and a panel
    /// carried onto it slid across and came to rest on top. Both crossings
    /// existed as controls — the tear-off mark and the re-dock mark — and
    /// neither existed as a gesture.
    ///
    /// One value, read by the preview and by the release, so the destination a
    /// person is shown and the one the release performs are the same fact
    /// rather than two derivations. Beside [`drag`](Self::drag) rather than
    /// inside it for the reason [`FloatGrab`] is beside it: the drag owns the
    /// CELL and this owns the SIDE, and folding a side into a type whose fields
    /// are a cell coordinate would give one of them a meaning it cannot carry.
    ///
    /// Every gesture that can put the pointer on the other side declares one,
    /// including the two that must NOT cross — moving a panel and sizing it —
    /// because [`CrossingPolicy::Stays`] is what turns "this gesture does not
    /// dock" from a coincidence of what is painted under the cursor into a
    /// property with a sentence.
    crossing: Signal<Option<Crossing>>,
    /// ★★★★★ R1903 — **where the palette is**, as the placement value the
    /// framework's panel axis speaks.
    ///
    /// The canon keeps this as `paletteOpen` and puts `togglePalette` /
    /// `openPalette` on it; this screen had neither, so its palette was chrome
    /// a reader could not put away. A `bool` would have been the cheap
    /// spelling — and the wrong one, because R1902 made an opening placement a
    /// thing a policy JUDGES, and a bare flag has nothing to be judged.
    ///
    /// Seeded from `spec::PALETTE_OPENS`, which that judgement admits.
    palette_at: Signal<EdgePlacement>,
    /// ★★★★★ R1908 — whether [`Self::palette_at`] is where a PREVIOUS RUN left
    /// it, rather than where this build's specification opens it.
    ///
    /// Published beside the placement rather than folded into it, the argument
    /// `spec.palette_placement`'s own `at`/`opens` pair makes one source back: a
    /// folded palette is the same bit whether a person folded it a moment ago,
    /// this build declares it opens folded, or a stored session was believed —
    /// and a client restoring or explaining a session acts differently in each.
    ///
    /// `false` until a stored placement is actually used, which is also what it
    /// says when one was read and REFUSED.
    palette_restored: Signal<bool>,
    /// ★★★★★ R1905 — **how the last card that changed home got where it is**.
    ///
    /// Published beside `floats` rather than folded into it, because it is a
    /// fact about a CROSSING and not about a panel: a client reading only the
    /// new position cannot tell a converted place from an unconverted one, and
    /// that indistinguishability is the whole defect R1891 left open. The same
    /// argument `spec.palette_placement` makes with its `at`/`opens` pair one
    /// gesture over.
    ///
    /// Not persisted, and `Option` rather than a fourth arm: "nothing has
    /// crossed yet" is a different statement from any arrival, and a session
    /// reopened tomorrow has made no crossing.
    arrival: Signal<Option<Arrival>>,
    /// ★★★★★ R1900 — **the occupant a strip press picked up**, when the drag in
    /// flight began on a tab rather than on the grip.
    ///
    /// A cell shared by two cards is one place with two occupants, and its
    /// header carries both gestures: the grip drags the *place* and a tab drags
    /// the *occupant* out of it. Those are the same [`TileDrag`] — the same
    /// card, the same footprint, the same landing — and they differ only in
    /// what letting go means, so the difference cannot live in the drag.
    ///
    /// Beside [`drag`](Self::drag) for the reason [`crossing`](Self::crossing)
    /// is: the drag owns the CELL and this owns WHICH OCCUPANT OF IT, and a
    /// shared cell is the one case where those are different questions.
    ///
    /// ⚠ Two signals that must agree is a shape this file has paid for, so
    /// nothing reads this field directly — [`pulling_a_tab`] does, and it is
    /// **total**: a value left behind by an earlier gesture answers `false`,
    /// because it must name the card the drag is actually carrying *and* that
    /// card must still be sharing a cell. A stale value therefore cannot mean
    /// anything, rather than meaning the wrong thing.
    tab_carry: Signal<Option<String>>,
    /// R1697 — the next stacking number a raise hands out. Monotonic, as the
    /// reference's is: comparing two panels' `z` is only meaningful while the
    /// numbers are never reused.
    float_z: RefCell<u32>,
    /// The last thing that happened, shown as the reference's toast.
    ///
    /// ★★★★★ R1719 — an [`Utterance`], the same value the other two screens of
    /// this tool now hold, so "was that a refusal?" is a field and not a prefix
    /// this file used to write two ways.
    /// ★★★★★ R1778 — and its LIFETIME, in one holder the framework owns. Two
    /// fields and a screen-local ticker collapsed into this when the two sibling
    /// screens turned out to need the same thing.
    toast: Rc<pinion_core::utterance::Saying>,
    /// The ordinal the next placed card takes.
    next_id: RefCell<u32>,
    /// R1662 — the board's scroll offset. A board is a grid whose row count is
    /// the model's, not the window's, so past roughly four and a half rows the
    /// cards were painted below the window and no gesture reached them
    /// ([[debt-the-analyzer-canvas-does-not-scroll]]). Held on the state
    /// because the paint and the hit test both read it.
    canvas_scroll: Rc<ScrollState>,
    /// ★ R1762 — the preferences page's viewport, for the reason the reference
    /// gives its own page one: the groups are taller than the region.
    settings_scroll: Rc<ScrollState>,
    /// ★ R1851 — the alarm feed's viewport. On the state rather than inside the
    /// painter for `canvas_scroll`'s reason: the paint, the accessibility walk
    /// and the wire all read the same offset, and a scroll offset created per
    /// frame would reset itself on every repaint.
    alarm_scroll: Rc<ScrollState>,
    /// ★ R1851 — the order the alarm feed's rows are in, as
    /// `(column, ascending)`. The header's indicator is DERIVED from this, so
    /// there is no second place for the direction to live.
    alarm_sort: Signal<Option<(usize, bool)>>,
    /// ★ R1851 — the least severity the feed keeps, as a word of
    /// [`spec::SEVERITY`], or `None` for *all*.
    ///
    /// A word rather than a rank because that is what a client writes and what a
    /// reader is told; the rank is resolved through the scale, which REFUSES a
    /// word the vocabulary does not hold instead of quietly keeping nothing.
    alarm_floor: Signal<Option<String>>,
}

/// ★★★★★ R1724 — **the destinations of this application, and the screens
/// behind the ones that have one.**
///
/// A function rather than a literal inside [`ShellState::new`] because it has
/// two readers and they must not diverge: the running application, and the
/// censuses over this file's specification. Those censuses enumerate *this
/// screen's* regions, keyboard stops and voices, and they are complete for the
/// pages this screen paints — a page that is another screen is judged by that
/// screen's own specification and its own tests. Which is which is one fact,
/// and it lives here.
///
/// `Mount<NodeLabView>` needs nothing from the node lab beyond its binding, and
/// [`ScreenRoster::new`] refuses a mount at a destination the rail declares
/// closed, so a seat cannot say *built, shipping, and not here* while showing
/// the screen.
///
/// # Panics
///
/// If a screen is mounted at a key the rail does not hold or has closed — a
/// defect in this pairing rather than a state the running screen can reach.
/// ★★★★★ R1864 — the preferences page's two frames.
///
/// Pose 0 is the state a reader arrives in — the top of the page. Pose 1 is the
/// end of its scroll, which is where the last group lives. The offsets are the
/// pane's own: `scroll_to` clamps against the range the layout pass derived
/// from the content, so this asks for the end rather than naming a number that
/// would go stale the first time the page grows a row.
struct SettingsPoses;

impl pinion_screen::SectionPoser for SettingsPoses {
    fn poses(&self) -> usize {
        2
    }

    fn pose(&self, nth: usize) {
        let state = use_shell_state();
        let (_, max) = state.settings_scroll.max();
        // Clamped, so `max` is the end whatever the content is — and pose 0 is
        // the top for the same reason: a walk that left the page where the last
        // one put it would report a frame nobody opens.
        state
            .settings_scroll
            .scroll_to(0, if nth == 0 { 0 } else { max });
    }
}

#[must_use]
fn screen_roster() -> ScreenRoster {
    let mut roster = ScreenRoster::new(
        spec::destinations(),
        vec![
            // ★★★★★ R1729 — **the seat that used to say *elsewhere*.**
            //
            // The capture viewer was an executable of its own for as long as
            // this tool was three of them, and the rail said so honestly:
            // *built, shipping, and not here*. It is here now, mounted the way
            // the node lab was, with the screen unedited — only its package
            // gained a `[lib]` and its binding a `pub`.
            (
                "packets",
                Box::new(Mount::<hello_packet_view::PacketView>::new()) as Box<dyn Screen>,
            ),
            // ★★★★★ R1730 — **the first page this shell gained by building a
            // section rather than by placing one that already existed.**
            //
            // The key-pattern section is the reference's third seat and was in
            // this tree in no form at all. What it brings back is a screen
            // whose own surfaces are checked against a written specification —
            // `docs/analyzer-keys-spec.json` — so mounting it makes the rail's
            // claim and the section's claim two separate gated facts.
            (
                "keys",
                Box::new(Mount::<hello_key_patterns::KeyPatternView>::new()) as Box<dyn Screen>,
            ),
            // ★★★★★ R1731 — **the page that closed the rail.** With this every
            // section the reference opens is one this application opens, and
            // `docs/analyzer-rail-spec.json`'s declared remainder is empty.
            (
                "logs",
                Box::new(Mount::<hello_log_view::LogView>::new()) as Box<dyn Screen>,
            ),
            // ★ R1728 — `lab`, not `catalog`. The reference's fifth seat is its
            // node graph section and this is it; `catalog` was a key the
            // reference does not have.
            (
                "lab",
                Box::new(Mount::<hello_node_lab::NodeLabView>::new()) as Box<dyn Screen>,
            ),
            // ★★★★★ R1947 — **the first seat this build opens that the SCOPE
            // reference draws locked.** Every mount above closed a gap against
            // that mockup; this one opens a seat the mockup books under a later
            // requirement, because the BEHAVIOUR reference builds the section
            // and a person asked for it by name. `docs/analyzer-rail-spec.json`
            // carries the divergence in its `owed` list rather than pretending
            // the two references agree.
            (
                "topology",
                Box::new(Mount::<hello_topology_view::TopologyView>::new()) as Box<dyn Screen>,
            ),
            // ★★★★★ R1948 — **the eighth and last seat.** With this every
            // section the BEHAVIOUR reference builds is one this application
            // opens, and the rail has nothing left to refuse — which is what
            // emptied `spec::Seat` of its last arm.
            (
                "sessions",
                Box::new(Mount::<hello_sessions_view::SessionsView>::new()) as Box<dyn Screen>,
            ),
        ],
    )
    .expect("the mounted screens sit at open destinations of this rail")
    // ★★★★★ R1761 — **the page this shell paints itself, answering for
    // itself.** Not a mount: the dashboard's layout bar and its palette are
    // painted BESIDE the page region rather than in it, so a screen at this
    // destination would judge three quarters of its own section. A judge
    // answers the one question — how much of `docs/analyzer-dashboard-spec.json`
    // is on the frame — and gets nothing else. See `crate::judge`.
    .judging("dashboard", Box::new(judge::BoardJudge))
    .expect("`dashboard` is an open destination with no screen mounted at it")
    // ★★★★★ R1762 — and the other page this shell paints itself. With this the
    // application's `unjudged` count reaches ZERO: every section a reader can
    // arrive at is compared with a written specification, which is what R1738
    // opened the count for.
    .judging("settings", Box::new(judge::SettingsJudge))
    .expect("`settings` is an open destination with no screen mounted at it")
    // ★★★★★ R1784 — **and what those two pages lay out in.** A judge answers
    // whether a section is on the frame; this answers what the frame has to be
    // for it to fit, which R1781's check asked of the four mounted screens and
    // could not ask of these two at all. Not the judge's job and not a screen's:
    // see `ScreenRoster::laying_out`.
    //
    // `panning` rather than `rigid` for both, and the second number is what
    // says why: below the comfortable width these pages keep laying out and the
    // region pans, which is what they already do — the board scrolls and the
    // settings column narrows to its cap.
    .laying_out(
        "dashboard",
        pinion_core::shrink::ShrinkPolicy::panning(
            (DASHBOARD_MIN_W, DASHBOARD_MIN_H),
            // One column and its gutter: below the comfortable width the board
            // pans, and it stops being a board when a single column no longer
            // fits.
            (board_canvas_floor() / GRID_COLS + GAP, DASHBOARD_MIN_H),
        ),
    )
    .expect("`dashboard` is an open destination this host paints itself")
    .laying_out(
        "settings",
        pinion_core::shrink::ShrinkPolicy::panning(
            (SETTINGS_MAX_W, SETTINGS_MIN_H),
            (SETTINGS_MIN_W, SETTINGS_MIN_H),
        ),
    )
    .expect("`settings` is an open destination this host paints itself")
    // ★★★★★ R1864 — **and how many frames that page needs to show all of it.**
    //
    // The preferences page scrolls and its content is taller than the region it
    // is given: measured at R1864, 946 pixels of page in an 820-pixel viewport,
    // so its last group is below the fold. A walk of one frame per section
    // reported that group unreproduced — a verdict true of the frame and false
    // of the section, since a reader scrolls to it in one gesture.
    //
    // ⚠ It had been passing on a technicality. The same group STRADDLED the
    // fold before this host reserved its status band, and a node partly outside
    // a viewport is still painted; 28 pixels moved it from *partly visible* to
    // *outside* and a question that was never about one frame started answering
    // differently. Nothing changed about what a reader could see.
    .posing("settings", Box::new(SettingsPoses))
    .expect("`settings` is an open destination this host paints itself")
    // ★★★★★ R1911 — **and WHERE on the frame each of those two pages puts its
    // marks.** `Screen::tag` is this for the four mounted sections; these two
    // had no answer at all, so R1729's check — arriving paints a section,
    // leaving takes it away, the host's chrome survives — could not include
    // them. Measured at entry: it walked `mounted_keys` and covered four of the
    // six sections a reader can open, and **nothing anywhere asserted that
    // leaving the dashboard stops the dashboard being painted.**
    //
    // A SET rather than one stem, and that is this host's own geometry: R1761
    // measured that the layout bar at (52,52) and the palette at (1148,52) are
    // both OUTSIDE the page region at (52,98), because a host paints a page's
    // chrome beside its region rather than in it. One stem could only have
    // named the third of this page that sits in the region.
    //
    // ⚠ `shell.canvas` is deliberately not here and neither is `shell.appbar`
    // or `shell.rail`: those are painted at every destination, so they are the
    // host's chrome and not any section's marks. Claiming one would make this
    // section "still painted" everywhere, which is the overlap `painting`
    // refuses when two sections do it to each other.
    //
    // ⚠ `match.spark` is here and it is the one a reader would not derive: the
    // sparkline inside `card.{id}.sparkline` emits its own top-level family
    // (R1648), so a card's chart is addressed nowhere near its card. Measured
    // rather than assumed — it is exactly the kind of mark the unclaimed check
    // below exists to surface.
    .painting(
        "dashboard",
        &["shell.subbar", "shell.palette", "card", "match.spark"],
    )
    .expect("`dashboard` is open, has no screen, and claims nothing a guest paints")
    .painting("settings", &["shell.settings"])
    .expect("`settings` is open, has no screen, and claims nothing a guest paints")
    // ★★★★★ R1911 — **and what this host paints at EVERY destination**, which
    // is what makes "this mark belongs to nobody" an answerable question rather
    // than an unstated assumption. Without it a mark no section claims is
    // indistinguishable from the frame itself, and `paint_stems`'s default
    // would be an escape hatch: a screen that never declared its real family
    // would quietly pass a thinner check instead of turning up unclaimed.
    .painting_chrome(&[
        VIEW_TAG,
        "shell.appbar",
        "shell.rail",
        "shell.canvas",
        "shell.status",
        "shell.toast",
    ])
    .expect("the host's own chrome overlaps no section's claim")
    // ★★★★★ R1725 — **this application has a navigation, so its pages must not
    // each bring one.** Declared here, beside the roster it is a fact about:
    // the rail this shell paints IS `spec::RAIL`, and a screen shown inside it
    // is in a place that already answers "where can I go".
    //
    // 🟥🟥🟥 ★★★★★ R1822 — **`ApplicationBar` too, and the sentence that used
    // to stand here was wrong for 97 rounds while reading as settled.**
    //
    // It said: *not `ApplicationBar`: this shell's bar carries the capture
    // source, the capture state and the global search, and a mounted screen's
    // own bar carries that screen's subject. Those are different sentences.*
    //
    // The same round that wrote it had already measured the behaviour canon and
    // recorded the opposite, in the debt file it opened: the canon has ONE bar,
    // this one, identical on all three screens, and a graph's name and run
    // state are **not in it** — they are on the canvas toolbar. Which is where
    // the node lab already draws them. So the guest's bar was not carrying a
    // different sentence; it was carrying the same one twice, in a strip 54
    // pixels tall that the canon does not have.
    //
    // ⇒ ★★★★★ a design note that answers a NEARBY question closes the real one
    // as surely as a wrong implementation, and reads like a reason while it
    // does it. The measurement was in the tree the whole time.
    .providing(
        HostChrome::NONE
            .with(ChromePart::Navigation)
            .with(ChromePart::ApplicationBar),
    );

    // ★★★★★ R1830 — **and what each of those sections is GRANTED.**
    //
    // R1784 taught the roster what a section WANTS and left what it RECEIVES
    // here, in this host. The gate that compared the two therefore read
    // `page_rect` — this file's function. Another host has its own, so nothing
    // portable could ask whether a section's want and its grant were about the
    // same section, and the only thing holding the pair together was one
    // assertion inside this application's own test file.
    //
    // Now the roster holds both halves, so `sections_short_of_their_grant`
    // answers for any host and the assertion moves out of here.
    //
    // ★★★★★ WHAT IS DECLARED IS THE INSET, NOT THE WIDTH, and this round tried
    // the width first and could not build it: `page_rect` reads the window
    // through `Owner::cache`, this function runs inside such a factory, and
    // every screen-owning test died on `Owner::cache factory closures must not
    // call Owner::cache`. The width is per-frame; what this shell paints beside
    // a page is not. So the static half is declared and the roster derives the
    // rest — which is what the debt asked for in as many words, and the panic
    // is what made the difference impossible to ignore.
    //
    // These are `page_rect`'s own two arms, read as insets: every destination
    // loses the rail, and the dashboard loses its palette as well because that
    // palette sits BESIDE the page region rather than in it. Vertical chrome is
    // not declared because the question this answers is about width.
    //
    // Every open destination, from the roster's own list: a hand-written set
    // here would be a second population that could drift from the rail, and
    // `ungranted_keys` exists precisely so a missed one is named rather than
    // read as fitting.
    let open: Vec<String> = roster
        .destinations()
        .keys()
        .filter(|key| {
            roster
                .destinations()
                .get(key)
                .is_some_and(|d| d.standing.is_open())
        })
        .map(str::to_owned)
        .collect();
    for key in open {
        // ★★★★★ R2045 — all four sides, where this declared a width. The host
        // used to compute its page rectangle beside this declaration, so one
        // fact had two accounts and a single assertion about a single
        // destination held them together; `page_rect` DERIVES from here now,
        // and the two cannot disagree because there is only one of them.
        //
        // The dashboard is the arm that makes this per-key rather than one
        // constant: it paints a sub-bar above its page and a palette beside it,
        // and no other destination does either.
        roster = roster
            .granting(&key, page_inset(&key))
            .expect("every key came from this roster's own open destinations");
    }
    roster
}

/// Build one arrangement — a board and the cards on it — from a placement list.
///
/// ★★★★★ R1894 — **the opening board and every shipped arrangement come from
/// here.** This loop existed once, inside `ShellState::new`, and adding the
/// canon's other three would have made it four copies of "place these kinds at
/// these cells and mint a card for each". Four copies is how one of them comes
/// to mint a card the board does not hold, which the preset then restores into
/// a layout with a hole.
///
/// Ids are `<kind>#<n>` over the list's own order, so two arrangements holding
/// the same kind give it the same id — which is what lets a card keep its
/// identity across an `apply_preset`.
fn arrangement_of(placed: &[spec::PlacedSpec]) -> Preset {
    let mut board = TileGrid::new(GRID_COLS);
    let mut cards = Vec::new();
    for (n, spot) in placed.iter().enumerate() {
        let def = def_of(spot.kind).expect("an arrangement names catalogue kinds");
        let id = format!("{}#{n}", spot.kind);
        board
            .place(Tile::new(
                id.clone(),
                spot.col,
                spot.row,
                spot.cols,
                spot.rows,
            ))
            .expect("a specified arrangement is a legal one");
        cards.push(
            Card::new(id, def.label)
                .with_chrome(CardChrome::of(chrome()))
                .with_state(CardState::Ready),
        );
    }
    Preset { board, cards }
}

impl ShellState {
    fn new(
        clock: Rc<TransportClock>,
        theme: Rc<ThemeProvider>,
        toast: Rc<pinion_core::utterance::Saying>,
        storage: Rc<pinion_platform_storage::AppStorage>,
    ) -> Self {
        let (light, dark) = reference_palettes();
        theme.set_palettes(light, dark);
        theme.set_mode(ThemeMode::Dark);
        let opening = arrangement_of(spec::BOARD);
        let (board, cards) = (opening.board.clone(), opening.cards.clone());
        let roster = spec::destinations();
        // ★ R1893 — the arrangements this application SHIPS go in as built-ins,
        // and a person cannot delete them. Before that round the set was a bare
        // map and every row was the same kind of thing, which is why `delete`
        // could not be built at all.
        //
        // ★★★★★ R1894 — and there are FOUR of them now, from the same function
        // that built the opening board. The canon offers four subject views
        // before a person has saved anything; this shipped one, so the
        // provenance axis had a population of one and the menu had a single
        // row. `spec::ARRANGEMENTS` carries the canon's own boards verbatim.
        let mut presets = Workspaces::new().with_built_in(spec::PRESET, opening);
        for shipped in spec::ARRANGEMENTS {
            presets = presets.with_built_in(shipped.name, arrangement_of(shipped.board));
        }
        Self {
            clock,
            theme,
            board: Signal::new(board),
            cards: Signal::new(cards),
            maximized: Signal::new(None),
            floats: Signal::new(Vec::new()),
            presets: RefCell::new(presets),
            storage,
            preset: Signal::new(spec::PRESET.to_string()),
            preset_open: Signal::new(false),
            editing: Signal::new(false),
            config_open: Signal::new(None),
            source: Signal::new(SOURCES[0].to_string()),
            capturing: Signal::new(true),
            search: Signal::new(String::new()),
            searching: Signal::new(false),
            tab: Signal::new(spec::VIEW_TABS[0].title.to_string()),
            journey: Signal::new(
                Journey::begin(&roster, spec::RAIL_ACTIVE)
                    .expect("the screen opens at a destination it can reach"),
            ),
            screens: screen_roster(),
            options: Signal::new(opening_options()),
            retention: Signal::new(spec::RETENTION.to_string()),
            picking: RefCell::new(None),
            // The chip the specification opens with, read from the same table the
            // paint reads — so the screen opens where the reference's does.
            filter_chip: Signal::new(spec::FILTER_CHIPS.iter().position(|(_, on)| *on)),
            selected: Signal::new(None),
            cursor: Signal::new((0, 0)),
            // ★ False on the opening frame: a screen nobody has pointed at yet
            // has no pointer in it. The first `move_cursor` sets it.
            pointer_inside: Signal::new(false),
            pressed: RefCell::new(None),
            drag: Signal::new(None),
            standing: RefCell::new(DropStanding::Nowhere),
            cursors: RefCell::new(
                spec::FOCUS_RING
                    .iter()
                    // ★ R1910 — through `roster()`, which is the one arm that
                    // has a declaration to build a `Roving` from. The spatial
                    // arm has a cursor and no roster, so it correctly builds
                    // none here and answers its active descendant elsewhere.
                    .filter_map(|stop| {
                        stop.interior
                            .roster()
                            .map(|spec| (stop.tag, Roving::new(spec)))
                    })
                    .collect(),
            ),
            float_grab: Signal::new(None),
            crossing: Signal::new(None),
            tab_carry: Signal::new(None),
            palette_at: Signal::new(spec::PALETTE_OPENS),
            // R1908 — nothing has been restored yet; `restore_arrangements`
            // runs after construction and is what can set this.
            palette_restored: Signal::new(false),
            // R1905 — nothing has changed home yet, which is not an arrival.
            arrival: Signal::new(None),
            float_z: RefCell::new(0),
            // ★ R1719/R1778 — this screen is the one that opens having ALREADY
            // said something; the node lab and the packet viewer open silent.
            // That difference is now expressed by SAYING it at construction
            // rather than by holding a different type, so all three screens
            // hold the same thing. Its life starts full, so the opening
            // sentence behaves like every later one instead of being a special
            // case that never leaves.
            toast,
            next_id: RefCell::new(u(spec::BOARD.len())),
            canvas_scroll: Rc::new(ScrollState::with_tag(CANVAS_SCROLL)),
            settings_scroll: Rc::new(ScrollState::with_tag(SETTINGS_SCROLL)),
            alarm_scroll: Rc::new(ScrollState::with_tag(ALARM_SCROLL)),
            alarm_sort: Signal::new(Some(spec::ALARM_OPENING_SORT)),
            alarm_floor: Signal::new(None),
        }
    }

    fn card(&self, id: &str) -> Option<Card> {
        self.cards.get().into_iter().find(|c| c.id().as_str() == id)
    }

    /// Where the rail has taken this window.
    fn at(&self) -> String {
        self.journey.get().at().to_owned()
    }

    /// The destinations, which since R1724 are the screen roster's.
    ///
    /// A method rather than a field so there is one roster and not a copy of
    /// its destination half beside it.
    fn roster(&self) -> &Destinations {
        self.screens.destinations()
    }

    /// Go to a destination the way both a press and the wire do.
    ///
    /// One function rather than one per channel: R1673 measured this screen's
    /// two paths giving a reserved seat two different answers, and a shared
    /// verb is the only arrangement in which they cannot.
    fn go(&self, key: &str) -> Result<(), Detour> {
        let mut journey = self.journey.get();
        let arrival = journey.navigate(self.roster(), key)?;
        let title = journey.here(self.roster()).title.clone();
        self.journey.set(journey);
        match arrival {
            pinion_core::widgets::destination::Arrival::AlreadyHere => {
                // ★ R1719 — the arrival type already draws this distinction;
                // the toast now carries it too, instead of leaving a reader to
                // notice the word "already".
                self.say(Utterance::unchanged(format!("you are in {title}")));
            }
            pinion_core::widgets::destination::Arrival::Moved { .. } => {
                self.say(Utterance::done(format!("{title} section")));
            }
        }
        Ok(())
    }

    fn update_card(&self, id: &str, edit: impl FnOnce(&mut Card)) -> bool {
        let mut cards = self.cards.get();
        let Some(found) = cards.iter_mut().find(|c| c.id().as_str() == id) else {
            return false;
        };
        edit(found);
        self.cards.set(cards);
        true
    }

    fn card_ids(&self) -> String {
        self.cards
            .get()
            .iter()
            .map(|c| c.id().as_str().to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// The cards actually on the board — a detached one is not.
    fn placed(&self) -> Vec<Card> {
        let floating = self.floats.get();
        self.cards
            .get()
            .into_iter()
            .filter(|c| !floating.iter().any(|f| f.id == c.id().as_str()))
            .collect()
    }

    fn is_floating(&self, id: &str) -> bool {
        self.floats.get().iter().any(|f| f.id == id)
    }

    /// ★★★★★ R1826 — **what is detached, and where it went**, as one value.
    ///
    /// The board's own answer to the question a caller would otherwise have to
    /// track for itself: for every detached card, the id of the OS window that
    /// now carries it. Derived from [`floats`](Self::floats) — the one model —
    /// so it cannot report a window for a card that is not detached, nor miss
    /// one that is.
    ///
    /// This is the axis the reference toolkit at 6.11 does not have. A
    /// reference dock hands a floated panel a top-level container and the
    /// caller keeps the correspondence: there is no accessor that answers
    /// *which window is this panel in* — the nearest is walking up the parent
    /// chain to a top-level and comparing pointers, which answers only for a
    /// panel the caller already holds. Here it is a published slot, so an agent
    /// that never saw the gesture can ask.
    /// ★ R1891 — the WINDOW-homed floats only, because those are the only ones
    /// that have a window to name. A canvas-homed panel is detached and has no
    /// window id; reporting one would be a correspondence to something that
    /// does not exist, which is worse than the silence.
    fn detached(&self) -> Vec<(String, String)> {
        self.floats_at(DetachHome::Window)
            .into_iter()
            .map(|f| (f.id.clone(), float_window_id(&f.id)))
            .collect()
    }

    /// R1697 — the detached panels in stacking order, frontmost first.
    ///
    /// The hit test walks this and the paint walks its reverse, so the panel a
    /// press lands on is the panel drawn on top by construction rather than by
    /// two functions agreeing. Ties keep the roster's order, which only
    /// happens before anything has been raised.
    fn floats_front_to_back(&self) -> Vec<Float> {
        let mut floats = self.floats.get();
        floats.sort_by(|a, b| b.z.cmp(&a.z));
        floats
    }

    /// ★★★★★ R1891 — the detached panels living in `home`, frontmost first.
    ///
    /// **The one classifier.** Four readers used to walk every float: the window
    /// topology opened a window for each, the canvas painted a panel for each,
    /// the hit test looked for each over the canvas, and `detached` named a
    /// window for each. So a torn-off card was a window AND a canvas panel at
    /// once — measured on the running application at R1891, and not a
    /// theoretical worry: `windows: ["main", "torn-packet#0"]` alongside five
    /// `float.packet#0…` regions in the main window.
    ///
    /// Each of those four now asks this, so which of them a float appears in is
    /// one decision rather than four that happen to agree. A float has exactly
    /// one [`DetachHome`], so the partition is total and disjoint by
    /// construction — there is no float this returns twice and none it drops.
    fn floats_at(&self, home: DetachHome) -> Vec<Float> {
        self.floats_front_to_back()
            .into_iter()
            .filter(|f| f.home == home)
            .collect()
    }

    /// R1697 — hand out the next stacking number.
    ///
    /// The reference's `raiseFloat`, and its drag calls it first: bringing a
    /// panel forward is part of grabbing it, not a separate affordance.
    fn raise_float(&self, id: &str) {
        let next = {
            let mut counter = self.float_z.borrow_mut();
            *counter += 1;
            *counter
        };
        let floats = self
            .floats
            .get()
            .into_iter()
            .map(|f| {
                if f.id == id {
                    Float { z: next, ..f }
                } else {
                    f
                }
            })
            .collect();
        self.floats.set(floats);
    }

    /// R1697 — replace one panel, by id. Nothing if it is not detached.
    fn set_float(&self, id: &str, next: &Float) {
        let floats = self
            .floats
            .get()
            .into_iter()
            .map(|f| if f.id == id { next.clone() } else { f })
            .collect();
        self.floats.set(floats);
    }

    /// R1697 — one panel, by id.
    fn float(&self, id: &str) -> Option<Float> {
        self.floats.get().into_iter().find(|f| f.id == id)
    }

    /// ★★★★★ R1698 — **what a composite's arrows reach, in cursor order.**
    ///
    /// Derived from the same tables the paint and the accessibility tree read,
    /// so there is one roster rather than three. And deliberately NOT the
    /// accessibility children: the palette's children are three section groups
    /// and two status readouts while the thing a cursor walks is the thirteen
    /// catalogue entries inside them — the distinction the framework's `Roving`
    /// exists to keep, and the one the reference toolkit loses (its tab bar of
    /// three tabs reports five accessible children).
    ///
    /// A locked member stays in the roster. That is this screen's whole
    /// subject: a seat booked for a later release is SHOWN rather than hidden,
    /// so a reader must be able to put the cursor on it and be told what it is
    /// waiting for. The floor skips its disabled entries, which makes them
    /// undiscoverable from the keyboard.
    fn cursor_members(stop: &str) -> Vec<Member> {
        match stop {
            "shell.appbar" => vec![
                // ★★★★★ The two view tabs are a composite of their own, so the
                // bar reaches them as ONE member — WAI-ARIA's nesting, and the
                // reason a member is a tag rather than a control. R1698 stopped
                // there and R1699 measured what that cost: the tab list was
                // reachable and no key went into it, so from a keyboard the two
                // views could not be switched at all.
                Member::new(APP_BAR_TABS).containing(Self::view_tabs()),
                Member::new(BarChip::Source.tag()),
                Member::new(BarChip::Capture.tag()),
                Member::new(BarChip::Search.tag()),
            ],
            "shell.rail" => spec::RAIL
                .iter()
                .map(|seat| {
                    Member::maybe(
                        format!("shell.rail.{}", seat.key),
                        seat.reserved_for().is_none(),
                    )
                })
                .collect(),
            "shell.subbar" => SubChip::ALL
                .iter()
                .map(|chip| Member::new(chip.tag()))
                .collect(),
            "shell.palette" => spec::CATALOGUE
                .iter()
                .map(|def| {
                    Member::maybe(
                        format!("shell.palette.{}", def.kind),
                        def.tier == spec::Tier::Placeable,
                    )
                })
                .collect(),
            // ★★★★★ R1721 — the saved-filter bar's roster is the ROW's, taken from
            // the widget rather than rebuilt here. A second construction of "what
            // the chips are" is exactly the drift this round is repairing on the
            // other axis, and the cursor is the one reader that had no need of
            // which chip is on.
            _ if stop == filter_chips_tag() => {
                filter_row_of(FILTER_CARD, None, 0, spec::FILTER_CHIPS.len())
                    .cursor()
                    .map(|roving| roving.members().to_vec())
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    /// ★★★★★ R1699 — the composite the application bar's first member **is**.
    ///
    /// A ring of two peers, so it wraps; and `Explicit`, because arriving at a
    /// view tab must not switch the view a reader is trying to leave — the same
    /// argument the rail's cursor makes, and the one the floor's tab list has
    /// no way to express (measured at 6.11.1: its tab bar changes the current
    /// tab on every arrow and exposes no property that would let an author ask
    /// for anything else).
    fn view_tabs() -> Roving {
        let mut tabs = Roving::new(
            RovingSpec::new(Axis::Horizontal)
                .with_ends(Ends::Wrap)
                .with_activation(Activation::Explicit),
        );
        tabs.seat(
            (0..spec::VIEW_TABS.len())
                .map(|n| Member::new(BarChip::Tab(n).tag()))
                .collect(),
        );
        tabs
    }

    /// R1698 — the composite's cursor, re-seated from the live roster first.
    ///
    /// Re-seating rather than rebuilding is what keeps the cursor on the member
    /// somebody put it on when the roster changes underneath — the property
    /// `Roving::seat` exists for.
    fn with_cursor<R>(&self, stop: &str, f: impl FnOnce(&mut Roving) -> R) -> Option<R> {
        let members = Self::cursor_members(stop);
        let mut cursors = self.cursors.borrow_mut();
        let roving = cursors.get_mut(stop)?;
        roving.seat(members);
        Some(f(roving))
    }

    /// R1698 — a read-only snapshot of one composite's cursor, seated.
    fn cursor_of(&self, stop: &str) -> Option<Roving> {
        self.with_cursor(stop, |roving| roving.clone())
    }

    /// R1699 — a read-only snapshot of the composite one MEMBER is, seated.
    ///
    /// By member tag rather than "whatever the cursor rests on", because a
    /// nested composite publishes its roster whether or not anybody is standing
    /// on it: the point of publishing is to be askable before pressing a key.
    fn inner_cursor_of(&self, stop: &str, member: &str) -> Option<Roving> {
        self.with_cursor(stop, |roving| {
            roving
                .members()
                .iter()
                .find(|m| m.tag == member)
                .and_then(Member::inner)
                .cloned()
        })
        .flatten()
    }

    fn preset_names(&self) -> String {
        self.presets.borrow().names().join(",")
    }

    /// Say something to the person in front of the screen.
    ///
    /// ★★★★★ R1719 — takes an utterance. The `refusal_sentence` helper this
    /// file kept beside it is gone: it existed because four call sites had
    /// written `format!("refused: {why:?}")` and its own note said a screen
    /// that has to remember not to use `Debug` is a screen that will use
    /// `Debug`. The remembering now belongs to the constructor, which takes
    /// something that can say itself and names a `Debug` spelling as a fault.
    fn say(&self, what: Utterance) {
        self.toast.say(what);
    }
}

/// How long the toast stays, in seconds.
///
/// ★★★★★ R1776 — **the reference's own number**, read from it rather than
/// chosen: `setTimeout(() => this.setState({toast: null}), 2600)`. Before this
/// the toast never left at all, and a reader running the assembled tool saw two
/// of them stacked over a mounted screen's palette — this shell's and the
/// guest's, both permanent. The overlap was the symptom; the missing lifetime
/// was the defect.
const TOAST_SECONDS: f32 = 2.6;

/// The widest a toast may be, whatever it says — a long sentence elides rather
/// than growing a strip across the window.
const TOAST_W: u32 = 560;

/// Where the toast's sentence starts, past its tone bullet.
const TOAST_TEXT_X: u32 = 24;

/// The room kept to the right of the sentence, so the next thing in the band
/// does not sit on the last glyph.
const TOAST_PAD_RIGHT: u32 = 12;

/// ★★★★★ R1811 — the box a sentence needs, bounded.
///
/// The per-character estimate is the same family
/// `pinion_core::containment::line_rect` uses for the OTHER axis: a figure
/// derived from the face size rather than from shaping, because `view` cannot
/// shape (§6.3). What makes an estimate safe here is that it is bracketed by
/// two gates that fail in opposite directions — `escapes` if it is too narrow
/// and the sentence leaves the box, `slack` if it is too wide and the box holds
/// room its words never use. An estimate nobody bracketed is what the constant
/// 560 was.
///
/// ★ R1865 — measured in [`STATUS_FACE`], because that is the face every
/// sentence in the status band is set in. It used to say `FONT_BODY`, which was
/// the face the floating box used; a width estimated for one face and painted
/// in another is the two-accounts defect, one axis over from the one R1864
/// removed from this strip's height.
///
/// ★★★★★ R1865 — **and the MINIMUM went, because its reason went.** A
/// `TOAST_MIN_W` of 180 was there so that "the bullet and the rounded corners
/// still read as a strip rather than a pill" — a statement about a filled,
/// bordered, floating box. In the status band there is no fill, no border and
/// no corner: the sentence sits on the band's own ground beside its bullet, and
/// a box wider than its words is just room nothing uses. R1811's slack ratchet
/// named this floor as the whole reason its residue was not zero, and with the
/// floor gone the residue is gone with it.
/// ★★★★★ R1865 — **and the per-glyph figure is a FRACTION of the face, not the
/// face less six.**
///
/// `px - 6` gave 6 at a 12-pixel face and 5 at an 11-pixel one, which is not a
/// relation to the face at all — it is a subtraction that happens to land near
/// one at exactly the size it was written at. Measured on the assembled screen
/// at R1865: an 11-pixel face runs about **6 pixels a glyph**, so the estimate
/// was a fifth narrow and the sentence was ELIDED — `Node Lab section` painted
/// as `Node Lab sec…`, which is R1701's defect, in the same strip, twelve
/// rounds later.
///
/// ⚠ **And it had been narrow all along.** At 12 pixels the same subtraction
/// gives 6 against a real ~6.5, and nothing showed because `TOAST_MIN_W`
/// floored every short sentence at 180 — the constant R1865 removed. That is
/// the round's third *a constant's reason went away and something it was
/// hiding appeared*.
///
/// ⚠⚠ **The two gates R1811 says bracket this estimate do not both reach it.**
/// Its note reads: *too narrow and `escapes` reports the sentence leaving its
/// box, too wide and `slack` reports the room.* The first half is FALSE for a
/// run drawn with `TextOverflow::Ellipsis`, which is what this is: eliding is
/// how the renderer makes a too-narrow box fit, so nothing escapes and
/// `escapes` is silent by construction. `r1865_the_bands_sentences_are_not_
/// elided` is the gate that half was supposed to be.
const fn glyph_run(px: u32, glyphs: u32) -> u32 {
    // Two thirds of the face, which is above the widest per-glyph average this
    // screen's faces measure and below the width at which `slack` complains.
    glyphs.saturating_mul(px * 2 / 3)
}

fn toast_width(sentence: &str) -> u32 {
    let glyphs = u32::try_from(sentence.chars().count()).unwrap_or(u32::MAX);
    (TOAST_TEXT_X + glyph_run(STATUS_FACE, glyphs) + TOAST_PAD_RIGHT).min(TOAST_W)
}

// ★★★★★ R1778 — the clock and the holder that were HERE are gone, into
// `pinion_core::utterance::Saying`. R1776 built them for this screen and the round
// after it found the other two screens needed the same thing, one of them
// keeping its sentence in a `RefCell` where a lifetime could expire with nothing
// repainting. What went up is the lifetime and the holder; what stayed is where
// this screen paints the box, because that genuinely differs across the three
// and a single widget over it would fight the reference.
//
// The adoption REMOVED two signals and a screen-local `Tickable` from this file.
// A lift that only ever adds is usually cutting along the wrong axis.

// ★★★★★ R1700 — the scope-free `Weak` handle that used to live here is GONE,
// and the compiler is what found it. R1671 introduced it for one reason: the
// two halves of this screen run in different places (a view is inside an
// `Owner` scope, the `External` invoke path is not) and `window_size` had to
// answer in both. `pinion_core::external::layout_size` answers in both now, so
// the handle had no remaining reader and `-D dead-code` said so on the first
// build after the switch.
//
// That is the shape of the repayment: the workaround did not have to be
// argued away, it stopped compiling.

fn use_shell_state() -> Rc<ShellState> {
    let clock = use_transport_clock(TRANSPORT_KEY, REPLAY_SECS);
    let theme = use_theme(THEME_TAG);
    let owner = Owner::current().expect("use_shell_state requires an active Owner scope");
    // ★★★★★ R1776/R1778 — the toast's holder is REGISTERED rather than
    // constructed, because it is also the thing the paint loop ticks.
    // `register_animation_once` is gated on the cache for the reason its own
    // documentation gives: this hook re-runs on every view pass, and a second
    // registration would count the sentence's life down twice per frame.
    //
    // The opening sentence is said INSIDE the factory, so it runs once and
    // "this screen opens having already spoken" is an act rather than a
    // different type from its two siblings.
    let toast = owner.register_animation_once(TOAST_LIFE_KEY, || {
        let said = pinion_core::utterance::Saying::new(TOAST_SECONDS);
        said.say(Utterance::done(format!("{} loaded", spec::PRESET)));
        said
    });
    // ★★★★★ R1897 — a person's own arrangements are laid over the shipped ones
    // INSIDE the cache factory, so the restore runs exactly once per process
    // rather than on every view pass. `register_animation_once` above is gated
    // for the same reason and says so; this is that rule applied to the other
    // thing a boot does.
    // ⚠ Resolved HERE, beside the cache rather than inside its factory — see
    // `arrangement_storage`. `use_app_storage` is itself an `Owner::cache`, and
    // this framework refuses a factory that re-enters the cache.
    let storage = arrangement_storage();
    let state = owner.cache(STATE_KEY, move || {
        let state = ShellState::new(clock, theme, toast, storage);
        restore_arrangements(&state);
        state
    });
    // ★★★★★ R1903 — publish the ADDRESS of that state to this thread, so the
    // screen's pure geometry helpers can read the palette's placement from
    // where they stand. A clone of the same `Rc`, overwritten on every call, so
    // the newest state is the one the rectangles answer about — the staleness
    // the sibling screen's own note warns of, closed by never letting the book
    // hold an older address than the cache does.
    SHELL_STATE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&state)));
    state
}

/// ★★★★★ R1826 — **the OS windows this application wants**, derived from the
/// detached cards rather than written beside them.
///
/// The board already had ONE model of what is detached — `state.floats` — and
/// SEVERAL call sites write it (`raise_float`, `set_float`, `remove`, `detach`,
/// `redock`, `set_on_top`, the preset reset — count them with `grep -n
/// 'floats.set('` rather than trusting this list, which is why no number is
/// written here). Minting a [`WindowSpec`] at each would be that many writers of
/// a second model, which is the shape this repository keeps measuring the cost
/// of: they agree until the day one is added without the other, and then a card
/// is detached with no window or a window stands with no card. So the topology
/// SUBSCRIBES to the floats and recomputes, and the number of writers stays one.
///
/// 🟥 This paragraph said "four call sites", named four, and was wrong when it
/// was written — and then THIS ROUND ADDED A FIFTH (`set_on_top`) three thousand
/// lines away in the same commit without touching the sentence. The closing
/// audit caught it. ⇒ a hand count in prose rots from the moment it is written,
/// and the round that writes one is the round most likely to invalidate it.
///
/// The float's own `x`/`y` become the window's declared position, so a panel
/// that used to open at a 30-pixel stagger inside the canvas now opens at the
/// same stagger on the desktop — the arrangement is preserved rather than
/// re-invented, and `w`/`h` likewise become the window's size.
///
/// # 🟥🟥🟥 ★★★★★ A TOPOLOGY IS NOT LIVE GEOMETRY, and the first draft made it
/// one
///
/// The first version of this Effect rebuilt the whole spec list from the
/// floats and published it whenever the list differed — which meant on **every
/// frame of a resize drag**, because a float's `w`/`h` change under the
/// pointer. That made `r1697_a_torn_off_panel_can_be_moved` FLAKY rather than
/// broken: the same binary, driven through the same sequence twice, once
/// latched the grab and clamped at the floor and once latched nothing at all.
/// A demo that is green when the machine is quiet is what
/// [[zero-flake-policy]] refuses, and it was measured rather than suspected —
/// baseline green under `git stash`, red with this file, and the two outcomes
/// recorded from two runs.
///
/// The repair is not a damper on the Effect; it is saying the true thing.
/// [`WindowSpec::strategy`] is **create-time intent** by the framework's own
/// documentation, and it is the ONLY axis of the spec that is — `position`,
/// `title`, `decorations`, `display` and `level` are each documented as live and
/// reconcilable — so a topology that republished on every geometry change was
/// asking the list to carry something it does not carry, and paying a window
/// add/update reconcile per pointer frame for a value the shell then ignored.
///
/// 🟥 This said "only `position` and `title` are reconciled live", which the
/// closing audit measured false against `WindowSpec`'s own field docs — and the
/// sentence contradicted THIS ROUND'S OWN FEATURE, since `on_top` works
/// precisely because `level` is one of the live axes.
///
/// So the topology is keyed on **which windows exist**: a spec is minted when
/// a card's id appears, kept as it was while the id is present, and dropped
/// when the id goes. A detached card's window therefore opens at the size and
/// place the panel had when it was detached, which is the arrangement
/// `detach` assigns, and it stops racing the gesture.
///
/// ✅ **R1891 closed the fork this used to warn about.** The paragraph here
/// said resizing the in-canvas float no longer resized its window, and that one
/// card had two things claiming to be it. It does not any more: a float carries
/// a [`DetachHome`], this Effect takes the [`DetachHome::Window`] ones and the
/// canvas paints the [`DetachHome::Canvas`] ones, so the two pictures are two
/// disjoint sets. There is nothing to keep in sync because there is never a
/// second copy — and the "resize does not follow" complaint dissolves with it,
/// since the panel a reader sizes on the canvas is the only panel there is.
fn use_shell_windows() -> Rc<Signal<Vec<WindowSpec>>> {
    let owner = Owner::current().expect("use_shell_windows requires an active Owner scope");
    let windows: Rc<Signal<Vec<WindowSpec>>> =
        owner.cache(WINDOWS_KEY, || Signal::new(vec![main_window_spec()]));
    let state = use_shell_state();
    let owner_for_effect = owner.clone();
    let windows_e = Rc::clone(&windows);
    // The Effect is kept alive by its own cache slot: dropped here it would
    // unsubscribe at once and the topology would never move again. The
    // precedent is the dock editor's window-title sync, which holds it the
    // same way and for the same reason.
    owner.cache(WINDOWS_EFFECT_KEY, move || {
        // ★★★★★ What has been published, held HERE rather than read back off
        // the signal. Reading `windows_e.get()` inside the Effect subscribes
        // the Effect to its own output, so every publish re-triggers it — and
        // measured, that is not a theoretical loop: with it in place
        // `r1697_a_torn_off_panel_can_be_moved` failed at a DIFFERENT leg on
        // each run (D, then E, then F), which is what a self-feeding Effect
        // looks like from outside. `Signal` has no untracked read, and it
        // should not need one: a memo of what this Effect last said is the
        // Effect's own business.
        let published: RefCell<Vec<WindowSpec>> = RefCell::new(vec![main_window_spec()]);
        let effect = Effect::new(&owner_for_effect, move || {
            // The ONE subscription: what is detached. Everything below it is a
            // pure function of that and of what this Effect last published.
            // ★ R1891 — the WINDOW-homed ones. `floats_at` reads `floats`, so
            // the single subscription this Effect relies on is unchanged; what
            // changes is that a canvas-homed panel no longer mints a window it
            // is not in.
            let floats = state.floats_at(DetachHome::Window);
            let standing = published.borrow().clone();
            let mut specs = vec![main_window_spec()];
            for float in &floats {
                let id = float_window_id(&float.id);
                // ★ A window that is already open KEEPS the spec it was opened
                // with, except for the axes the shell reconciles live. Rebuilt
                // from the float's current geometry it would change under a
                // resize drag, and the topology would republish per pointer
                // frame — see this function's header for what that cost.
                let level = if float.on_top {
                    WindowLevel::AlwaysOnTop
                } else {
                    WindowLevel::Normal
                };
                match standing.iter().find(|spec| spec.id == id) {
                    Some(open) => specs.push(open.clone().with_level(level)),
                    None => specs.push(
                        WindowSpec::new(
                            Cow::Owned(id),
                            label_of(&float.id),
                            SizeStrategy::Fixed {
                                width: float.w,
                                height: float.h,
                            },
                        )
                        .with_position(
                            i32::try_from(float.x).unwrap_or(0),
                            i32::try_from(float.y).unwrap_or(0),
                        )
                        // R1826 — the specification's "always-on-top option",
                        // per panel. `WindowLevel` is the framework's existing
                        // declaration (R1610) and the shell applies it on a
                        // same-id change, so toggling it re-levels the window
                        // that is already open rather than needing a new one.
                        .with_level(level),
                    ),
                }
            }
            if standing != specs {
                published.borrow_mut().clone_from(&specs);
                windows_e.set(specs);
            }
        });
        WindowTopologySync { _effect: effect }
    });
    windows
}

/// Holds the topology's subscription for the owner's lifetime. See
/// [`use_shell_windows`].
struct WindowTopologySync {
    _effect: Effect,
}

/// The main window's spec — the one every topology starts from.
fn main_window_spec() -> WindowSpec {
    WindowSpec::new(
        Cow::Borrowed(MAIN_WINDOW),
        AnalyzerShellView::title(),
        SizeStrategy::shrinking(SHRINK, (WIN_W, WIN_H)),
    )
}

/// The canonical id of this application's primary window.
const MAIN_WINDOW: &str = "main";

// --- Geometry: ONE source, read by the paint and by the gesture --------------
//
// debt-paint-and-gesture-read-two-facts is open in this project because a
// surface whose painter and hit test compute their rectangles separately drifts
// into a control drawn where it cannot be clicked. Every rectangle below is
// computed once and used by BOTH the `*_scene` painters and `Hit::at`.

/// A container's own coordinate space: its size at the origin.
///
/// ★ Children of an absolutely-positioned container are placed RELATIVE TO IT,
/// so a child written in window coordinates lands at the parent's origin plus
/// its own. R1648 shipped exactly that bug and only a two-direction sweep of
/// the painted rectangles found it. The sub-rectangles below are **local**; the
/// painter passes `local(rect)` and the hit test subtracts the origin.
const fn local(rect: Rect) -> Rect {
    Rect::new(0, 0, rect.w, rect.h)
}

const fn contains(rect: Rect, px: u32, py: u32) -> bool {
    px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h
}

fn u(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Where a tile sits on the canvas, in the canvas's own space.
fn cell_rect(tile: &Tile) -> Rect {
    let pitch = col_pitch();
    Rect::new(
        GAP + tile.col * pitch,
        GAP + tile.row * ROW_H,
        (tile.w * pitch).saturating_sub(GAP).max(1),
        (tile.h * ROW_H).saturating_sub(GAP).max(1),
    )
}

/// ★★★★★ R1733 — whether a window point is **on the board at all**.
///
/// The half a card drag never needed: a card gripped on the board is always
/// over some cell, so "off the board" had no consequence and no name. A
/// footprint carried off a palette starts off the board and may be released
/// there, and the two are different answers — `cell_at_window` clamps, so
/// asking it alone turns a release over the palette into a placement at
/// whatever cell the clamp last produced.
///
/// Derived from the same rectangles [`Hit::at`] uses rather than a second
/// arithmetic: the board is the canvas, and the canvas is only the canvas at
/// the dashboard destination — at any other page that region belongs to the
/// page. A floating panel over it is not the board either; it is chrome, and
/// dropping a card onto one is not a placement.
/// ★★★★★ R1898 — and the gesture may be CARRYING one of the panels this rule
/// excludes.
///
/// A detached panel is chrome over the board, and dropping a card onto one is
/// not a placement (R1733). But a gesture that drags a panel — or one of that
/// panel's own marks — has that panel under the cursor for its whole life, so
/// asking the unqualified question would answer "not over the board" every
/// time and the crossing would be settled by *what happens to be painted*
/// rather than by where the board is.
///
/// That is the difference between a rule and a coincidence, and this tree has
/// paid for the coincidence before. One rule, stated once, with the carried
/// panel named: every OTHER float still blocks, because dropping a card under
/// somebody else's panel is the placement R1733 refused.
fn on_board_ignoring(state: &ShellState, px: u32, py: u32, carrying: Option<&str>) -> bool {
    if state.at() != "dashboard" || !contains(canvas_rect(), px, py) {
        return false;
    }
    match Hit::at(state, px, py) {
        // ★ R1907 — the send-home control is part of the panel, so a point over
        // it is over that panel. Left out of this list a card could be dropped
        // through it onto the board underneath, which is exactly the "somebody
        // else's panel" placement R1733 refused.
        Hit::Float(id)
        | Hit::FloatHome(id)
        | Hit::FloatRedock(id)
        | Hit::FloatClose(id)
        | Hit::FloatResize(id) => carrying == Some(id.as_str()),
        _ => true,
    }
}

/// ★★★★★ R1898 — where a release at this point would put what is being
/// carried, on whichever side of the board's edge that is.
///
/// The ONE classifier behind every gesture that can cross: a card gripped on
/// the board, a detached panel's re-dock mark carried onto it, and a panel
/// being moved or sized that must not dock. Written once because a second
/// spelling of "is the pointer over the board" is a second answer, which is the
/// shape [`preview_carry`](ShellOracle::preview_carry) exists to prevent one
/// layer down.
///
/// The cell is the raw one the pointer falls in. What a *joining* release
/// actually places is the [`TileDrag`]'s landing, which folds in the footprint
/// clamp — so this value classifies the side and the drag owns the cell, and
/// the two cannot disagree about where the preview was.
/// A window pixel as the framework's drag latch reads one.
///
/// One conversion, in one place: the latch measures a Euclidean distance and
/// this screen counts in whole pixels, and a cast written at each of the four
/// call sites is four chances to write it differently.
fn press_point(px: u32, py: u32) -> (f64, f64) {
    (f64::from(px), f64::from(py))
}

fn rest_at(state: &ShellState, px: u32, py: u32, carrying: Option<&str>) -> Rest {
    if on_board_ignoring(state, px, py, carrying) {
        let (col, row) = cell_at_window(state, px, py);
        Rest::cell(col, row)
    } else {
        Rest::point(px, py)
    }
}

/// ★ R1762 — a window coordinate folded into a scrolled surface's own frame.
///
/// One helper because three hit tests do it: the board's, the preferences
/// page's, and the cell query below. Two of them wrote it out and the third was
/// about to, which is this project's lift trigger — and the sharper reason is
/// that an offset folded three ways is an offset one of them folds the wrong
/// direction.
fn fold_by(v: u32, by: i32) -> u32 {
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "clamped into u32's range on the line above the cast"
    )]
    let folded = (i64::from(v) + i64::from(by)).clamp(0, i64::from(u32::MAX)) as u32;
    folded
}

/// ★★★★★ R1726 — a WINDOW point turned into the cell it falls in, with the
/// board's scroll folded in.
///
/// The board slides under the canvas, so a window point and a board point
/// differ by the scroll offset. `Hit::at` has folded it since R1662 — which is
/// why pressing a scrolled card still selects the right one — and the two
/// places that turned a press into a CELL did not. So a drag begun after
/// scrolling computed its grab and its destination in the unscrolled frame.
/// Reported from the running board: scroll down, press a widget, and the drop
/// position is not where the widget is.
///
/// One function, so the fold cannot be remembered at one call site and
/// forgotten at the other — which is exactly what had happened.
///
/// ⚠ R1898 — this paragraph had been sitting on `on_board` since R1733,
/// documenting a function that does not fold a scroll offset and never
/// mentioned this one. Found by removing that function: a doc comment attached
/// to the wrong item is a claim nothing checks, and the two had been read as
/// one block for 165 rounds.
fn cell_at_window(state: &ShellState, px: u32, py: u32) -> (u32, u32) {
    let canvas = canvas_rect();
    let (ox, oy) = state.canvas_scroll.offset();
    cell_at(
        fold_by(px.saturating_sub(canvas.x), ox),
        fold_by(py.saturating_sub(canvas.y), oy),
    )
}

/// ★★★★★ R1726 — the chip that rides the cursor while a card is carried,
/// saying WHAT is being carried.
///
/// Its shape is the behaviour reference's, read from that prototype's own
/// source: offset `+14, +10` from the pointer, above everything, transparent to
/// it, a bordered surface chip holding the widget's name. The reference does
/// **not** drag a copy of the widget — the widget stays on the board and this
/// names it — which is the answer to "shouldn't the widget show while
/// dragging": it does, in place, and this is what tells you which one you have.
fn carried_label(text: &str, cx: u32, cy: u32, palette: Palette) -> Scene {
    let w = 22 + u32::try_from(text.chars().count()).unwrap_or(8) * 7;
    Scene::Container(
        ContainerNode::new(vec![label(
            text,
            Rect::new(11, 7, w.saturating_sub(22), 15),
            12,
            palette.ink,
        )])
        // ★ R1733 — renamed from `shell.carried` into the surface stem the
        // carry's other parts share, so one specification can read all of them
        // back out of the paint. The chip is this build's answer to a fact the
        // reference gets from the browser's own drag image, and the
        // specification records it as such rather than deleting it.
        .with_tag("shell.carry.chip")
        .with_style(
            BoxStyle::filled(palette.raised)
                .with_corner_radius(9)
                .with_border(Border::new(palette.accent_fg, 1)),
        )
        // Transparent to the pointer: it rides the cursor, so a hit test that
        // could land on it would be testing the label instead of the board
        // underneath — the R1497 class this tree already paid for once.
        .with_layout(
            absolute(Rect::new(
                cx.saturating_add(14),
                cy.saturating_add(10),
                w,
                29,
            ))
            .with_pointer_transparent(true),
        ),
    )
}

/// The board cell a canvas-local pixel lands on — the inverse of [`cell_rect`],
/// and the only place the two directions meet.
fn cell_at(lx: u32, ly: u32) -> (u32, u32) {
    let pitch = col_pitch().max(1);
    let col = lx.saturating_sub(GAP) / pitch;
    let row = ly.saturating_sub(GAP) / ROW_H;
    (col.min(GRID_COLS - 1), row)
}

/// The outline a card strokes inside its rectangle at rest.
const CARD_OUTLINE: u32 = 1;

/// The accent ring it strokes instead when it is the keyboard's subject — the
/// wider of the two, because a ring has to read as a ring.
const CARD_RING: u32 = 2;

/// The band a card reserves for its own frame: the **widest** the frame ever
/// gets, whichever of the two it is drawing right now.
///
/// ★★ R1671 — every one of a card's three bands is inset by it, and that is a
/// rule rather than three decisions. Reported by a person looking at the
/// window, twice: the stream's `time / type / name / len` strip sat on the
/// card's outline, and after the body was inset the outline was still eaten —
/// by the size-stepper strip, which is the band nobody had thought about.
///
/// ★★ R1672 — and it was [`CARD_OUTLINE`], so the moment a card became the
/// selected one and swapped its 1px outline for the 2px ring, its header and
/// **every row of its body** stood on the ring's second pixel. Nine marks, in a
/// state this screen's sweep already ran, and nothing could see them: until this
/// round `scene/containment` compared a mark against the owner's BOX, and a
/// border is ink the box owns *inside* that box, so painting over it was by
/// that definition contained. The channel has a content box now
/// ([`pinion_core::containment::content_rect`]) and this is the placement half.
///
/// Derived from the two widths rather than picked, and **constant** rather than
/// a function of `selected`: a content rectangle that changed with the ring
/// would move every row in the card by a pixel when a person selects it.
const CARD_FRAME: u32 = if CARD_RING > CARD_OUTLINE {
    CARD_RING
} else {
    CARD_OUTLINE
};

/// The card's header band — the grip, the title and the header controls.
const fn header_rect(card: Rect) -> Rect {
    Rect::new(
        card.x + CARD_FRAME,
        card.y + CARD_FRAME,
        card.w.saturating_sub(CARD_FRAME * 2),
        CARD_HDR.saturating_sub(CARD_FRAME),
    )
}

/// The card's content band, between the header and the edit strip.
const fn body_rect(card: Rect, editing: bool) -> Rect {
    let foot = if editing { EDIT_BAR_H } else { 0 };
    Rect::new(
        card.x + CARD_FRAME,
        card.y + CARD_HDR,
        card.w.saturating_sub(CARD_FRAME * 2),
        card.h.saturating_sub(CARD_HDR + foot + CARD_FRAME),
    )
}

/// ★★★★★ R2022 — **the rectangle a card's body is painted in**, or `None` when
/// this card paints no body at all.
///
/// [`body_rect`] is the arithmetic; this is *which card rectangle to run it on*,
/// and until this round only the painter knew. `card_scene` takes the tile's
/// cell (or the float's frame), makes it card-local, and subtracts the edit
/// strip when the board is in layout-edit mode — three facts a describing
/// reader had no way to reach, so [`card_nodes`]'s bodies counted the
/// specification's tables instead and announced rows the card had no room for.
///
/// The `None` arms are the painter's own refusals rather than guesses at them:
/// [`body_scene`] paints a body only for a card whose state
/// [`is_ready`](CardState::is_ready) — a loading or failed card draws its
/// sentence and its remedy — and a card the board does not hold is not drawn.
///
/// ⚠ Card-LOCAL, which is what [`body_scene`] receives. Only the width and
/// height are read from it for the row and column derivations, but returning
/// the painter's own argument is what keeps this one derivation rather than a
/// second one that agrees today.
fn card_body_rect(state: &ShellState, card: &Card) -> Option<Rect> {
    if !card.state().is_ready() {
        return None;
    }
    let (frame, editing) = card_frame(state, card)?;
    Some(body_rect(frame, editing))
}

/// The card's own rectangle in its own space, and whether the board it is on is
/// in layout-edit mode — the two facts every part of a card is measured from.
///
/// A detached card's frame is its float's and it has no edit strip, which is why
/// the flag rides with the rectangle rather than being read separately.
fn card_frame(state: &ShellState, card: &Card) -> Option<(Rect, bool)> {
    let id = card.id().as_str();
    if let Some(float) = state.floats.get().into_iter().find(|f| f.id == id) {
        return Some((local(float_rect(&float)), false));
    }
    let board = state.board.get();
    let tile = board.tile(card.id())?;
    Some((local(cell_rect(tile)), state.editing.get()))
}

/// ★★★★★ R2022 — how a card's header band was laid out: which affordances
/// survived it, and whether the grip did.
///
/// [`header_scene`] asks `card_header::lay_out` and paints what it placed; until
/// this round [`card_nodes`] announced every affordance the chrome offers, so a
/// card narrow enough to give one away still told a reader it was there — the
/// same ghost the bodies had, on the strip above them.
///
/// `None` for a DETACHED card, and that is a statement rather than a gap: a
/// float's header is the float's own (`float.{id}.{wire}`), laid out by
/// [`float_affordance_rect`], so this layout does not describe it.
fn card_header_layout(state: &ShellState, card: &Card) -> Option<card_header::HeaderLayout> {
    if state.is_floating(card.id().as_str()) {
        return None;
    }
    let (frame, _) = card_frame(state, card)?;
    Some(card_header::lay_out(
        header_rect(frame),
        card.chrome().offered().len(),
        card.state().is_ready(),
        CARD_METRICS,
    ))
}

/// The size-stepper strip at the foot of a card in layout-edit mode.
const fn edit_bar_rect(card: Rect) -> Rect {
    Rect::new(
        card.x + CARD_FRAME,
        (card.y + card.h).saturating_sub(EDIT_BAR_H + CARD_FRAME),
        card.w.saturating_sub(CARD_FRAME * 2),
        EDIT_BAR_H,
    )
}

/// This screen's card-header measurements.
///
/// ★★★★★ R1816 — the numbers stayed, the arithmetic left. Every rectangle in a
/// card header used to be computed here, and the same arithmetic was written a
/// second time inside `Hit::at`; both now call
/// [`pinion_widget_paint::card_header`], so the paint and the gesture cannot
/// answer differently about where a slot is. The census row for this capability
/// read `app` rather than `have` precisely because the framework had the card's
/// MODEL and none of its layout.
const CARD_METRICS: card_header::CardMetrics = card_header::CardMetrics {
    band_h: CARD_HDR,
    slot_w: 28,
    slot_inset_y: 4,
    tail: 6,
    grip_w: 18,
    grip_inset: 4,
    title_gap: 20,
    min_title: 24,
    badge_w: 54,
    // ★ R1882 — the two faces live with the other measurements now, because a
    // face fixes the line box a run needs and the layout could not see them
    // where they were. They are the same two this screen always passed.
    title_px: FONT_BODY,
    badge_px: FONT_TINY,
};

/// One header control slot. Right-aligned, in declaration order, so the
/// rightmost is the last affordance the vocabulary declares.
fn affordance_rect(header: Rect, count: u32, n: u32) -> Rect {
    card_header::slot_rect(header, count, n, CARD_METRICS)
}

/// ★★★★★ R1907 — **what a detached panel's header offers here**, derived from
/// this host's own detach policy and asked by the paint and the hit test alike.
///
/// One derivation because a header control is drawn AND pressed, and this
/// screen's standing debt ([[debt-paint-and-gesture-read-two-facts]]) is
/// precisely what a second copy would be. Before this round the count was the
/// literal `2`, written twice in the painter and twice in [`Hit::in_canvas`],
/// with nothing comparing the four — so a third control could be drawn where
/// nothing could press it, or pressed where nothing was drawn.
fn float_affordances() -> &'static [DetachedAffordance] {
    detach_policy().detached_affordances()
}

/// The box of the `n`th control a detached panel's header offers.
///
/// Takes the index into [`float_affordances`] rather than a count and a slot,
/// so the two cannot be given inconsistently — the caller cannot say "the
/// second of two" while the roster has three.
fn float_affordance_rect(header: Rect, n: usize) -> Rect {
    let count = u32::try_from(float_affordances().len()).unwrap_or(1);
    let slot = u32::try_from(n).unwrap_or(0);
    affordance_rect(header, count, slot)
}

// ★ R1817 — `grip_rect` used to be declared here. `card_header::grip_scene`
// draws the grip now and nothing on this side asks where it is, so the
// delegating wrapper R1816 left became dead code. That is the compiler saying
// the lift finished rather than a comment claiming it did.

/// ★★★★★ R1900 — the tab strip a **shared** cell puts where its title would
/// have been, or `None` when the cell has one occupant.
///
/// The ONE derivation, asked by the paint and by [`Hit::at`] alike. A strip is
/// pressed as well as drawn, and this screen's standing debt
/// ([[debt-paint-and-gesture-read-two-facts]]) is exactly what a second copy of
/// this arithmetic would be — the framework already refuses to let the boxes
/// and the hit test disagree ([`card_header::Strip::at`]), and this keeps the
/// *inputs* to it single too: the title box a strip may take depends on how
/// many affordances the card offers and whether it earns a badge, and those are
/// the card's facts rather than the strip's.
fn card_strip(card: &Card, cell: &Tile, inside: Rect) -> Option<card_header::Strip> {
    if !cell.is_shared() {
        return None;
    }
    let title = card_header::lay_out(
        header_rect(inside),
        card.chrome().offered().len(),
        card.state().is_ready(),
        CARD_METRICS,
    )
    .title()?;
    Some(card_header::strip(
        title,
        cell.members().len(),
        cell.fore_index(),
        CARD_METRICS,
    ))
}

// ── R2021: a card's own settings ───────────────────────────────────────────
//
// ★★★★★ The measurements the behaviour prototype gives its per-card settings
// popover, read off its own markup rather than chosen here: a 250-wide panel
// inset from the card's right edge, hanging just under the header band, padded
// 14, with each field a small upper-case caption over a 32-high control.
//
// It is anchored to the CARD and painted after every card, so it hangs over its
// neighbours the way the prototype's does (it sets `overflow:visible` and lifts
// the card's stacking order for exactly this). What it must not do is live
// inside the card's own container: a popup drawn inside the thing that opens it
// is clipped by it, which is the R1672 lesson this screen has already paid for
// twice — once with its preset menu and once with the preferences roster.

/// The width the prototype's per-card settings panel has.
const CFG_W: u32 = 250;
/// Its inner padding.
const CFG_PAD: u32 = 14;
/// The gap between the header band and the panel's top edge.
const CFG_DROP: u32 = 6;
/// How far the panel is inset from the card's right edge.
const CFG_INSET: u32 = 10;
/// The panel's heading, and the line under it.
const CFG_HEAD_H: u32 = 18;
const CFG_GIST_H: u32 = 16;
/// One field: an upper-case caption over a control.
const CFG_CAPTION_H: u32 = 14;
const CFG_CTRL_H: u32 = 32;
/// The space under a field, before the next one.
const CFG_ROW_GAP: u32 = 12;

/// The narrowest panel that can still say what it is.
///
/// Below this it is not drawn at all — the all-or-nothing clamp the health
/// strip, the latency tiles and the alarm feed already make, and for their
/// reason: three clipped words are worse than an honest absence. A card too
/// small for the panel is a card whose settings are still reachable, over the
/// wire and from the roster the keyboard reaches.
const CFG_FLOOR: u32 = 170;

/// The settings panel of a card whose gear is pressed, in the CARD's own frame.
///
/// `None` when the card cannot hold one, which is a real state: the board
/// places a `4 x 1` alarm card and a person can narrow it further.
fn card_config_rect(card: Rect, rows: usize) -> Option<Rect> {
    let room = card.w.saturating_sub(CFG_INSET * 2);
    let w = CFG_W.min(room);
    if w < CFG_FLOOR {
        return None;
    }
    let h = CFG_PAD
        + CFG_HEAD_H
        + CFG_GIST_H
        + CFG_PAD
        + u32::try_from(rows).unwrap_or(0) * (CFG_CAPTION_H + CFG_CTRL_H + CFG_ROW_GAP)
        + CFG_PAD;
    Some(Rect::new(
        card.w.saturating_sub(CFG_INSET + w),
        header_rect(local(card)).h + CFG_DROP,
        w,
        h,
    ))
}

/// Where the `n`th field's caption and control sit, in the PANEL's own frame.
///
/// One derivation for the paint, the roster's anchor and the hit test — this
/// screen's standing rule, and the class it has now paid for four times.
fn card_config_row_rects(panel: Rect, n: usize) -> (Rect, Rect) {
    let top = CFG_PAD
        + CFG_HEAD_H
        + CFG_GIST_H
        + CFG_PAD
        + u32::try_from(n).unwrap_or(0) * (CFG_CAPTION_H + CFG_CTRL_H + CFG_ROW_GAP);
    let inner = panel.w.saturating_sub(CFG_PAD * 2);
    (
        Rect::new(CFG_PAD, top, inner, CFG_CAPTION_H),
        Rect::new(CFG_PAD, top + CFG_CAPTION_H, inner, CFG_CTRL_H),
    )
}

/// The card whose settings panel is up, and the panel's rectangle in the
/// BOARD's frame — the frame every other rectangle on the canvas is stated in.
///
/// `None` unless a card's gear is pressed, the card is still placed, and it has
/// room. All three are states the board reaches on its own: a card can be
/// closed, torn off or narrowed while its panel is open, and each of those has
/// to take the panel with it rather than leave one hanging over nothing.
fn card_config_panel(state: &ShellState) -> Option<(Card, Rect, Rect)> {
    // ★★★★★ The destination is part of the question. The board is painted at
    // one seat of the rail, and a panel that stayed hit-testable while a reader
    // was somewhere else is the class R1695 measured across this whole shell —
    // a page you left, still answering. Asked HERE rather than at each caller
    // because the paint, the hit test and the accessibility tree all come
    // through this function, and a guard at two of the three is the shape this
    // screen's standing debt is made of.
    if state.at() != "dashboard" {
        return None;
    }
    let open = state.config_open.get()?;
    let card = state
        .placed()
        .into_iter()
        .find(|c| c.id().as_str() == open)?;
    let board = state.board.get();
    let tile = board.tile(card.id())?;
    // A card sharing a place shows its panel only while it is the one in front,
    // for the reason the header does: the gear a person pressed is the front
    // card's, and the others are not painted at all (R1900).
    if &tile.id != card.id() {
        return None;
    }
    let cell = cell_rect(tile);
    let rows = spec::card_settings_of(kind_of(card.id().as_str())).len();
    let panel = card_config_rect(cell, rows)?;
    Some((
        card,
        cell,
        Rect::new(cell.x + panel.x, cell.y + panel.y, panel.w, panel.h),
    ))
}

/// A rectangle stated in the BOARD's frame, in the window's.
///
/// The canvas's origin, less the board's scroll — the inverse of the fold
/// [`Hit::in_canvas`] applies on the way in, written once so the two cannot
/// drift.
fn board_to_window(state: &ShellState, rect: Rect) -> Rect {
    let canvas = canvas_rect();
    let (ox, oy) = state.canvas_scroll.offset();
    Rect::new(
        canvas.x + fold_by(rect.x, -ox),
        canvas.y + fold_by(rect.y, -oy),
        rect.w,
        rect.h,
    )
}

/// Where a card setting's collapsed control sits, in the BOARD's frame.
///
/// `None` when the panel holding it is not up, which is what makes every caller
/// ask one question — *is this control on the screen* — instead of two.
fn card_setting_seat(state: &ShellState, valued: &Valued) -> Option<Rect> {
    let Valued::Card { card, setting } = valued else {
        return None;
    };
    let (open, _, panel) = card_config_panel(state)?;
    if open.id().as_str() != card {
        return None;
    }
    let n = spec::card_settings_of(kind_of(card))
        .into_iter()
        .position(|s| s.key == setting.key)?;
    let (_, seat) = card_config_row_rects(panel, n);
    Some(Rect::new(
        panel.x + seat.x,
        panel.y + seat.y,
        seat.w,
        seat.h,
    ))
}

/// ★★★★★ R2021 — the roster an open card setting lays over the board, in
/// **window** space and bounded by the canvas.
///
/// Window space for the reason the preferences page's is (R1762, R1672): a
/// roster drawn inside the panel that opens it is clipped by that panel, and a
/// roster is exactly the thing that must be allowed to hang past its opener.
/// The room it must stay inside is the CANVAS, handed to the framework's own
/// geometry rather than decided here — so a card near the bottom of the board
/// opens its roster upward instead of off the end.
fn card_roster_box(state: &ShellState) -> Option<(Valued, Picker, chooser::RosterBox)> {
    let picking = state.picking.borrow();
    let (key, picker) = picking.as_ref()?;
    let valued = Valued::from_key(key)?;
    if !matches!(valued, Valued::Card { .. }) {
        return None;
    }
    let seat = board_to_window(state, card_setting_seat(state, &valued)?);
    let roster = chooser::lay_roster(
        valued.roster_key(),
        seat,
        picker,
        canvas_rect(),
        SET_OPTION_H,
    );
    Some((valued, picker.clone(), roster))
}

/// ★★★★★ R1900 — what letting go of the carried card **here** does.
///
/// The board's inner boundary, one layer in from the edge R1898 named: a
/// release either takes a cell of the grid or joins whoever is already in one.
/// One value, read by the preview and by the release, for the reason every
/// other classifier on this screen is written once — a person is shown a
/// destination and the release must go to that destination, not to a second
/// derivation that agrees today.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Berth {
    /// A cell of its own — the placement [`TileDrag`] already computes.
    Own,
    /// The place this card's header belongs to, whose occupants it joins.
    With(String),
}

/// Which berth a release at `(px, py)` would use, for a card already on the
/// board.
///
/// A header band is the target rather than a card's middle, and that is the
/// gesture being **legible**: a title bar is where a person already believes a
/// window's identity lives, and the strip appears exactly there — so the place
/// a release is aimed at is the place that visibly changes. A card's middle is
/// a region with no mark on it, which is learnt by accident or from a manual.
fn berth_at(state: &ShellState, px: u32, py: u32, carried: &TileId) -> Berth {
    if state.at() != "dashboard" {
        return Berth::Own;
    }
    let Some(host) = Hit::at(state, px, py).header_card().map(str::to_owned) else {
        return Berth::Own;
    };
    let board = state.board.get();
    // ★★★★★ The carried card must be **on the board**, and this line was
    // missing from the first draft — the board's own conformance sweep caught
    // it, by carrying a PALETTE footprint over the middle of the board and
    // finding the join mark where the specification declares the cell mark.
    //
    // A join takes an occupant out of one place and puts it in another, so a
    // card that is not in a place yet has nothing to come from: joining from
    // the palette would be *place, then share*, two acts and a second cell that
    // exists for no frame. Answering `Own` is not a limitation swallowed — the
    // wire's `share` verb takes it from there, in two calls that each say what
    // they did.
    let Some(there) = board.tile(&TileId::new(host.clone())) else {
        return Berth::Own;
    };
    if board.tile(carried).is_none() || there.holds(carried) {
        // Not on the board; or its own header, or a header of the place it is
        // already in — for which there is no second place to come from, which
        // is what `TileGrid::share` refuses by name. Answering `Own` keeps the
        // *preview* honest: a mark promising a join the release would refuse is
        // worse than no mark.
        return Berth::Own;
    }
    Berth::With(host)
}

/// Whether the drag in flight is pulling `carried` **out of** a cell it shares,
/// rather than dragging that whole cell by its grip.
///
/// Total by construction — see [`ShellState::tab_carry`]. A value left over
/// from an earlier gesture names a different card, or a card that no longer
/// shares anything, and answers `false` either way.
fn pulling_a_tab(state: &ShellState, carried: &TileId) -> bool {
    state
        .tab_carry
        .get()
        .is_some_and(|member| member == carried.as_str())
        && state
            .board
            .get()
            .tile(carried)
            .is_some_and(pinion_core::widgets::tile_grid::Tile::is_shared)
}

/// Where a not-ready card's remedy control sits.
const fn remedy_rect(body: Rect) -> Rect {
    Rect::new(body.x + 10, body.y + 32, 150, 22)
}

/// The size steppers, left to right: `− W + − H +`.
const STEPPERS: [(&str, &str); 4] = [
    ("narrow", "\u{2212}"),
    ("widen", "+"),
    ("shorter", "\u{2212}"),
    ("taller", "+"),
];

/// Where the `n`th stepper button sits in a card's edit bar: two groups of two,
/// with the axis letter between them.
const fn stepper_rect(bar: Rect, n: u32) -> Rect {
    let group = n / 2;
    let within = n % 2;
    Rect::new(
        bar.x + 8 + group * 78 + within * 46,
        bar.y + 3,
        20,
        EDIT_BAR_H - 6,
    )
}

/// The app bar's pressable regions, left to right.
///
/// ★★★★★ R1946 — `Tab` carries WHICH tab, indexing [`spec::VIEW_TABS`], where
/// the previous `Tab0` / `Tab1` were two hand-written arms that spelled their
/// tags and rectangles a second time. A tab the specification declares and this
/// enumeration lacks is now unrepresentable: [`Self::all`] is built from the
/// table, so the chip set has the declaration's cardinality by construction
/// rather than by anyone remembering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarChip {
    /// The `n`th entry of [`spec::VIEW_TABS`].
    Tab(usize),
    Source,
    Capture,
    Search,
}

impl BarChip {
    /// Every pressable region of the bar, left to right — the declared tabs
    /// first, then the three chips that are this bar's own chrome.
    ///
    /// A `Vec` rather than the `const [Self; 5]` this was: a constant length is
    /// exactly the thing that could disagree with the specification, and it did
    /// for as long as the tabs were two named arms.
    fn all() -> Vec<Self> {
        (0..spec::VIEW_TABS.len())
            .map(Self::Tab)
            .chain([Self::Source, Self::Capture, Self::Search])
            .collect()
    }

    /// This chip's paint tag. A tab's is derived from its declared key, so the
    /// tag and the table cannot drift.
    fn tag(self) -> String {
        match self {
            Self::Tab(n) => format!("shell.appbar.tab.{}", spec::VIEW_TABS[n].key),
            Self::Source => "shell.appbar.source".to_owned(),
            Self::Capture => "shell.appbar.capture".to_owned(),
            Self::Search => "shell.appbar.search".to_owned(),
        }
    }

    fn rect(self) -> Rect {
        match self {
            Self::Tab(n) => spec::VIEW_TABS[n].rect,
            Self::Source => Rect::new(416, 10, 268, 32),
            Self::Capture => Rect::new(696, 10, 132, 32),
            Self::Search => Rect::new(win_w() - 300, 10, 288, 32),
        }
    }
}

/// The sub bar's pressable regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubChip {
    Preset,
    EditLayout,
    AddWidget,
}

impl SubChip {
    const ALL: [Self; 3] = [Self::Preset, Self::EditLayout, Self::AddWidget];

    const fn tag(self) -> &'static str {
        match self {
            Self::Preset => "shell.subbar.preset",
            Self::EditLayout => "shell.subbar.edit",
            Self::AddWidget => "shell.subbar.add",
        }
    }

    /// In the sub bar's own space, whose origin is `(RAIL_W, APP_BAR_H)`.
    fn rect(self) -> Rect {
        let bar_w = win_w() - RAIL_W - palette_room();
        match self {
            Self::Preset => Rect::new(16, 7, 178, 32),
            Self::EditLayout => Rect::new(bar_w - 330, 7, 140, 32),
            Self::AddWidget => Rect::new(bar_w - 180, 7, 164, 32),
        }
    }
}

/// One entry of the open preset menu, in **window** space.
///
/// ★★ R1672 — window rather than the sub bar's, because the menu left the sub
/// bar. A drop-down is anchored to the control that opens it and is not
/// *bounded* by it: this one hung 81 pixels below the bar it was a child of,
/// which the ink gate reports as an escape and which is exactly what it is —
/// the reference toolkit at 6.11 makes a menu a top-level popup for the same
/// reason. It is now a sibling of the bars, painted after them, at the window
/// coordinates its anchor derives.
/// The height of one preset-menu row. Named because the row's label is centred
/// in it and the paint reads both.
const PRESET_ROW_H: u32 = 30;

fn preset_item_rect(n: u32) -> Rect {
    let anchor = SubChip::Preset.rect();
    Rect::new(
        RAIL_W + anchor.x + 8,
        APP_BAR_H + anchor.y + 44 + n * 34,
        210,
        PRESET_ROW_H,
    )
}

/// Where the `n`th rail entry sits, in the rail container's own space.
const fn rail_rect(n: u32) -> Rect {
    Rect::new(8, 14 + n * 44, RAIL_W - 16, 36)
}

/// The rail PANEL: the strip between the application bar and the status band.
///
/// ★ R1864 — a rectangle rather than four numbers at the one site that built
/// it, because a second site now needs its height (the account chip is anchored
/// to the panel's bottom) and a chrome edge that two places compute is a chrome
/// edge two places can disagree about.
fn rail_panel_rect() -> Rect {
    Rect::new(
        0,
        APP_BAR_H,
        RAIL_W,
        win_h()
            .saturating_sub(APP_BAR_H)
            .saturating_sub(status_band_h()),
    )
}

/// One palette entry's height, at its most comfortable.
///
/// Sized so that the whole catalogue FITS the panel: the reference scrolls its
/// palette and this shell does not, so a row height that overflowed would put
/// the last widget kinds under the footer where nothing can reach them. That is
/// a real difference from the reference and it is spent here rather than
/// hidden — see the module docs' list of what is not matched.
///
/// ★★★★★ R1864 — **and "sized so that it fits" was an act somebody performed
/// once, by hand, at one window height.** Measured when the status band took 28
/// pixels off the panel: the last entry's bottom was at 816 and the footer's
/// top at 818, so the catalogue had cleared the counts by **two pixels** — and
/// with the panel shorter the two counts were drawn straight through the last
/// widget kind, which the caption gate reported as a run escaping its holder.
///
/// A number tuned to a layout is right until the layout moves, and it moves
/// silently. So this is the CEILING now and [`palette_row_h`] is what the panel
/// can actually afford; the fit is a derivation, and
/// `r1864_the_palette_catalogue_clears_its_own_footer` is what says so.
const PALETTE_ROW_H: u32 = 46;

/// Where the palette's rows begin, under its heading block.
const PALETTE_ROWS_TOP: u32 = 76;
/// The band at the panel's foot that its two counts sit in.
const PALETTE_FOOT_H: u32 = 30;
/// A section heading's own height, and the gap under it.
const PALETTE_SECTION_H: u32 = 20;
const PALETTE_SECTION_GAP: u32 = 6;
/// The gap under one entry, and the extra gap that closes a section.
const PALETTE_ENTRY_GAP: u32 = 4;
const PALETTE_SECTION_TAIL: u32 = 8;

/// The panel's body: everything between its heading block and its footer band.
fn palette_body_rect() -> Rect {
    let panel = palette_rect();
    Rect::new(
        0,
        PALETTE_ROWS_TOP,
        panel.w,
        panel
            .h
            .saturating_sub(PALETTE_ROWS_TOP)
            .saturating_sub(PALETTE_FOOT_H),
    )
}

/// The footer band the two counts are placed in.
fn palette_foot_rect() -> Rect {
    let panel = palette_rect();
    Rect::new(
        0,
        panel.h.saturating_sub(PALETTE_FOOT_H),
        panel.w,
        PALETTE_FOOT_H,
    )
}

/// ★★★★★ R1864 — what an entry can afford, which is what [`PALETTE_ROW_H`] was
/// asserting by hand.
///
/// The headings and their gaps are fixed per section; what is left divides
/// among the entries. Never taller than the comfortable height — a panel with
/// room to spare gets the reference's rhythm rather than rows stretched to fill
/// it — and never so tall that the catalogue runs into the counts below it.
fn palette_row_h() -> u32 {
    let sections = u(spec::SECTIONS.len());
    let entries = u(spec::CATALOGUE.len()).max(1);
    let fixed = sections * (PALETTE_SECTION_H + PALETTE_SECTION_GAP + PALETTE_SECTION_TAIL);
    let room = palette_body_rect().h.saturating_sub(fixed);
    (room / entries)
        .saturating_sub(PALETTE_ENTRY_GAP)
        .min(PALETTE_ROW_H)
}

/// The palette panel's rectangle.
/// ★ R1903 — the fold control, in the palette panel's own space.
///
/// One function because the paint draws it and [`Hit::at`] finds it, and this
/// screen's standing debt is exactly what a second copy of that rectangle would
/// be ([[debt-paint-and-gesture-read-two-facts]]).
///
/// ★ The canon's own measurements, read from its markup this round: a 26x26
/// button with a 7px radius, right-aligned in a header padded `14px 16px`
/// after a flex spacer, carrying `title="Collapse"`. Reproduced rather than
/// approximated — the size and the corner are the two things a reader's eye
/// actually compares.
/// ★★★★★ R1956 — **the band the palette's heading occupies**, so its title and
/// its fold control share a centre line by construction rather than by two
/// people picking `18` and `14`.
///
/// They had picked exactly that, and `containment::uncentred` reported the
/// pair: a 20-tall title at `y: 18` centres on 28, a 26-tall control at
/// `y: 14` centres on 27. Derived from one band, neither number is anybody's to
/// choose and a later change to either height carries the other's placement
/// with it.
const fn palette_head_band() -> Rect {
    Rect::new(0, 14, PALETTE_W, 26)
}

/// The title's box in that band, at the height its own face needs.
fn palette_head_title_rect() -> Rect {
    pinion_core::containment::line_rect_in(palette_head_band(), 16, 220, FONT_TITLE)
}

const fn palette_fold_rect() -> Rect {
    pinion_core::containment::band_in(palette_head_band(), PALETTE_W - 16 - 26, 26, 26)
}

/// ★★★★★ R1951 — **the face the palette's own control wears**, asked of the
/// policy that offers it rather than chosen here.
///
/// `spec::PALETTE_POLICY` admits no edge and folds, so
/// [`EdgePolicy::controls`](pinion_core::edge_panel::EdgePolicy::controls)
/// answers exactly one control — a fold toward the edge the palette is on — and
/// this screen paints what that answers. Two facts stop being this file's to
/// remember: *which* control the palette offers, and *which way* its mark
/// points.
///
/// # Panics
///
/// When the palette's placement offers no fold at all, which its own
/// specification forbids: `PALETTE_POLICY.foldable` is `true` and this is only
/// reached on the unfolded path, so a `None` here means the policy and the
/// screen have come apart and the screen would otherwise paint an empty box —
/// the defect this vocabulary exists to make impossible.
fn palette_fold_face() -> control_mark::ControlMark {
    let at = palette_placement();
    let control = spec::PALETTE_POLICY
        .control(PanelAffordance::Fold, at)
        .expect("the palette declares that it folds, and this path is the unfolded one");
    control_mark::ControlMark::of_panel(control)
}

/// The outline [`palette_scene`]'s chrome boxes draw inside themselves.
const PALETTE_CHROME_FRAME: u32 = 1;

/// A bordered chrome box's CONTENT rectangle in its own space.
///
/// ★ R1951 — the placement half of `containment::content_rect`, here for the
/// reason the node lab needed its own at R1950: a child's position is read from
/// the box's BORDER box, so a mark handed the box's own rectangle overhangs it
/// by the frame on all four sides. That is a defect a screen gate reports and a
/// reader sees, and it costs one function to make unreachable.
fn box_content(rect: Rect) -> Rect {
    pinion_core::containment::content_of(
        Rect::new(0, 0, rect.w, rect.h),
        Some(&Border::new(Color::rgba(0, 0, 0, 0), PALETTE_CHROME_FRAME)),
        &[],
    )
}

fn palette_rect() -> Rect {
    let room = palette_room();
    Rect::new(
        win_w() - room,
        APP_BAR_H,
        room,
        win_h()
            .saturating_sub(APP_BAR_H)
            .saturating_sub(status_band_h()),
    )
}

/// The palette's rows — section headers interleaved with entries, in the
/// panel's own space.
///
/// Returned rather than recomputed at each site: the painter walks it to draw
/// and the hit test walks it to resolve, which is the discipline the card
/// rectangles follow.
fn palette_rows() -> Vec<PaletteRow> {
    let mut out = Vec::new();
    let row_h = palette_row_h();
    let mut y = PALETTE_ROWS_TOP;
    for (key, title) in spec::SECTIONS {
        out.push(PaletteRow {
            def: None,
            section: key,
            // ★ R1761 — the heading says which release fills the group, which
            // is what the reference writes there. ★ R1797 — derived from the
            // group's own ENTRIES rather than from a tier column beside it, so
            // promoting one widget cannot leave the heading behind.
            title: spec::section_heading(key, title),
            rect: Rect::new(16, y, PALETTE_W - 32, PALETTE_SECTION_H),
        });
        y += PALETTE_SECTION_H + PALETTE_SECTION_GAP;
        for def in spec::CATALOGUE.iter().filter(|w| w.section == *key) {
            out.push(PaletteRow {
                def: Some(def),
                section: key,
                title: def.label.to_owned(),
                rect: Rect::new(10, y, PALETTE_W - 30, row_h),
            });
            y += row_h + PALETTE_ENTRY_GAP;
        }
        y += PALETTE_SECTION_TAIL;
    }
    out
}

/// One line of the palette panel: a section heading, or a catalogue entry.
struct PaletteRow {
    /// The catalogue entry, or `None` for the section heading above them.
    def: Option<&'static spec::WidgetSpec>,
    /// The section this line belongs to — the heading's own key, and the key of
    /// the section an entry sits under. Carried rather than recomputed because
    /// the heading is the group a reader descends through, and it needs a tag.
    section: &'static str,
    /// The words painted on the line.
    ///
    /// Owned since R1761: a section heading's words are composed from the
    /// group's name and its release, so they are not a `'static` string any
    /// more.
    title: String,
    /// Where, in the palette panel's own space.
    rect: Rect,
}

// --- What is under a point ---------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Hit {
    Chip(BarChip),
    Sub(SubChip),
    PresetItem(usize),
    Rail(&'static str),
    /// R1695 — a Settings switch, by its specification key.
    Option(&'static str),
    /// R1695 — a Settings key row's button, which is booked for a later release.
    KeyRow(&'static str),
    /// ★ R1762 — a Settings value row's collapsed control, by its specification
    /// key. Pressing it opens the roster; pressing it again dismisses.
    /// ★ R2021 — an owned key, because a roster is no longer only over a row
    /// the specification names statically: a card's own setting is addressed by
    /// the card showing it, and that id is built at run time.
    Choose(String),
    /// ★ R1762 — one option of an open roster, by its place in it. The key
    /// travels with it because a roster is over the whole page and the row it
    /// belongs to is not derivable from where the press landed.
    ChooseOption(String, usize),
    /// R1695 — a theme segment, by its index in [`spec::THEMES`].
    Theme(usize),
    Palette(&'static str),
    Grip(String),
    Affordance(String, CardAffordance),
    Stepper(String, &'static str),
    /// R1721 — a saved filter on the filter card, by its index in the row.
    FilterChip(String, usize),
    /// ★ R1851 — a heading of the alarm feed, by its column index. Pressing it
    /// cycles that column's order.
    AlarmColumn(String, usize),
    Remedy(String),
    /// ★★★★★ R1903 — the control that puts the palette away. On the panel's own
    /// header, where the canon's is.
    PaletteFold,
    /// ★★★★★ R1903 — the strip a folded palette leaves, and the WHOLE of it is
    /// the affordance.
    ///
    /// The canon agrees literally: its closed state draws a 44px band whose own
    /// element carries the toggle. Putting a small button inside a narrow strip
    /// would be a fold a reader cannot undo without aiming.
    PaletteStrip,
    /// ★★★★★ R1900 — one tab of a shared cell's strip, by the occupant it
    /// names.
    ///
    /// The occupant rather than an index, because an index is only meaningful
    /// beside the strip it was read from and this value outlives that read: the
    /// press stores it, the release acts on it, and in between the board can
    /// have changed. The strip's own hit test ([`card_header::Strip::at`])
    /// answers in indices and this arm is where that index stops travelling.
    Tab(String),
    Card(String),
    /// ★★★★★ R1907 — the control that sends a detached panel to the next home
    /// this host admits.
    ///
    /// Its own arm rather than a modifier on [`Self::FloatRedock`], because
    /// re-docking and changing home are opposite intentions: one gives the
    /// panel back to the board, the other keeps it detached and moves where it
    /// lives. The canon has no counterpart — its detached panel has one place
    /// to be — so this is the arm that carries the capability this tree has and
    /// the reference does not.
    FloatHome(String),
    FloatRedock(String),
    FloatClose(String),
    /// R1697 — the corner that sizes a detached panel. Its own arm rather than
    /// a modifier on [`Self::Float`], because the reference stops the event
    /// propagating there: a grab on the corner must not also start a move.
    FloatResize(String),
    Float(String),
    Nothing,
}

impl Hit {
    /// Front to back: the preset menu is over the sub bar, floats are over the
    /// canvas, and a card's own controls are over its body.
    fn at(state: &ShellState, px: u32, py: u32) -> Self {
        // ★ R1672 — the menu is a top-level popup now, so its rows are asked
        // for in WINDOW coordinates and there is no origin to subtract. The
        // paint reads the same function.
        if state.preset_open.get() {
            let rows = state.presets.borrow().len();
            for n in 0..=rows {
                if contains(preset_item_rect(u(n)), px, py) {
                    return Self::PresetItem(n);
                }
            }
        }
        if py < APP_BAR_H {
            for chip in BarChip::all() {
                if contains(chip.rect(), px, py) {
                    return Self::Chip(chip);
                }
            }
            return Self::Nothing;
        }
        // ★★ R1695 — the rail is chrome and is asked first, whatever the page.
        // Everything after it belongs to the destination showing, which is why
        // this branch moved above the palette's: the palette is the dashboard's
        // and is not painted anywhere else, so a press in its old column at
        // another destination must fall through to that page rather than reach a
        // row nobody can see. The paint and this test read one `page_rect`.
        if px < RAIL_W {
            for (n, seat) in spec::RAIL.iter().enumerate() {
                if contains(rail_rect(u(n)), px, py - APP_BAR_H) {
                    return Self::Rail(seat.key);
                }
            }
            return Self::Nothing;
        }
        let at = state.at();
        if at != "dashboard" {
            let region = page_rect(&at);
            return Self::in_settings(state, px - region.x, py - region.y);
        }
        if px >= palette_rect().x {
            // ★★★★★ R1903 — a FOLDED palette is a strip, and the whole strip is
            // the affordance. Asked before the rows, because a folded panel
            // builds none of them: the rows below would answer `Nothing` and a
            // reader's press on the band would do nothing at all.
            if palette_placement().folded {
                return Self::PaletteStrip;
            }
            let panel = palette_rect();
            let (lx, ly) = (px - panel.x, py - panel.y);
            if contains(palette_fold_rect(), lx, ly) {
                return Self::PaletteFold;
            }
            for row in palette_rows() {
                if let Some(def) = row.def
                    && contains(row.rect, lx, ly)
                {
                    return Self::Palette(def.kind);
                }
            }
            return Self::Nothing;
        }
        if py < APP_BAR_H + SUB_BAR_H {
            let (lx, ly) = (px - RAIL_W, py - APP_BAR_H);
            for chip in SubChip::ALL {
                if contains(chip.rect(), lx, ly) {
                    return Self::Sub(chip);
                }
            }
            return Self::Nothing;
        }
        // ★★★★★ R2021 — an OPEN card-setting roster is over everything on the
        // board, so it is asked before the canvas and in WINDOW space, which is
        // the frame it was laid in. Anywhere else falls through and closes it,
        // which is what a reader expects of a control that is collapsed until
        // you open it — and dismissing is not choosing, so the value is left
        // alone.
        if let Some((valued, _, roster)) = card_roster_box(state) {
            for (n, (_, seat)) in roster.options.iter().enumerate() {
                if contains(*seat, px, py) {
                    return Self::ChooseOption(valued.key(), n);
                }
            }
        }
        let canvas = canvas_rect();
        Self::in_canvas(state, px - canvas.x, py - canvas.y)
    }

    /// ★★★★★ R1699 — what a **key** press at `tag` addresses: the inverse of
    /// [`hit_word`].
    ///
    /// A keyboard activation is semantic, not geometric — the reader named a
    /// thing, not a pixel — so synthesising a press at the middle of the tag's
    /// rectangle would be wrong twice over: a member scrolled out of view has a
    /// rectangle and is covered, and a member with no rectangle at all (the
    /// tab list, which is anchored by the tabs it composes) has none to aim at.
    ///
    /// The risk of an inverse is that it drifts from the thing it inverts, so
    /// the round wrote the gate before the function:
    /// `r1699_every_cursor_member_resolves_to_the_hit_its_tag_names` requires,
    /// for every member of every composite, that `hit_word(of_tag(t)) == t`
    /// AND that `of_tag(t)` equals what `Hit::at` answers at the centre of that
    /// tag's painted rectangle. Two independent derivations of the same fact,
    /// with the paint as the arbiter.
    fn of_tag(state: &ShellState, tag: &str) -> Self {
        if let Some(chip) = BarChip::all().into_iter().find(|c| c.tag() == tag) {
            return Self::Chip(chip);
        }
        if let Some(chip) = SubChip::ALL.into_iter().find(|c| c.tag() == tag) {
            return Self::Sub(chip);
        }
        if let Some(key) = tag.strip_prefix("shell.rail.")
            && let Some(seat) = spec::RAIL.iter().find(|seat| seat.key == key)
        {
            return Self::Rail(seat.key);
        }
        if let Some(kind) = tag.strip_prefix("shell.palette.")
            && let Some(def) = spec::CATALOGUE.iter().find(|def| def.kind == kind)
        {
            return Self::Palette(def.kind);
        }
        if let Some(key) = tag.strip_prefix("shell.settings.option.")
            && let Some(option) = spec::OPTIONS.iter().find(|o| o.key == key)
        {
            return Self::Option(option.key);
        }
        if let Some(key) = tag.strip_prefix("shell.settings.key.")
            && let Some(row) = spec::KEY_ROWS.iter().find(|row| row.key == key)
        {
            return Self::KeyRow(row.key);
        }
        if let Some(n) = tag
            .strip_prefix("shell.settings.theme.")
            .and_then(|n| n.parse::<usize>().ok())
            && n < spec::THEMES.len()
        {
            return Self::Theme(n);
        }
        if let Some(n) = tag
            .strip_prefix("shell.preset.item.")
            .and_then(|n| n.parse::<usize>().ok())
            && n <= state.presets.borrow().len()
        {
            return Self::PresetItem(n);
        }
        // ★★★★★ R1721 — a saved filter, named. The keyboard reaches the bar and
        // chooses through the cursor, and `Enter` on a chip has to arrive at the
        // same action a press on it does — which is what the round-trip gate next
        // door requires of every member of every composite.
        if let Some(n) = tag
            .strip_prefix(&format!("card.{FILTER_CARD}.chip."))
            .and_then(|n| n.parse::<usize>().ok())
            && n < spec::FILTER_CHIPS.len()
        {
            return Self::FilterChip(FILTER_CARD.to_owned(), n);
        }
        Self::Nothing
    }

    /// ★ R1695 — what is under a point on the Settings page, in the region's own
    /// space.
    ///
    /// The rectangles come from the same `settings_*_rect` helpers the painter
    /// uses, which is the standing rule on this screen: what is drawn and what
    /// responds are derived from ONE fact.
    fn in_settings(state: &ShellState, cx: u32, cy: u32) -> Self {
        let region = page_rect(&state.at());
        // ★★★★★ R1762 — the page slides under its viewport, so the question is
        // folded into the query once and every rectangle below stays stated in
        // the page's own frame. The same shape the board's hit test has, for
        // the same reason: two places subtracting an offset is two places one
        // of them forgets to.
        let (_, oy) = state.settings_scroll.offset();
        let cy = fold_by(cy, oy);
        // ★★★★★ R1762 — an OPEN roster is over everything on this page, so it
        // is asked first. Anywhere else closes it, which is what a reader
        // expects of a control that is collapsed until you open it and is what
        // the reference does — and dismissing is not choosing, so the value is
        // left alone.
        {
            let picking = state.picking.borrow();
            // ★ R2021 — a row of THIS page, for the paint's reason: the seat
            // lookup answers `0` for a key it does not hold, so a card's roster
            // left open on the board would be pressable over the first
            // preferences row while nothing drew it there.
            if let Some((key, picker)) = picking
                .as_ref()
                .filter(|(key, _)| matches!(Valued::from_key(key), Some(Valued::Preference(_))))
            {
                let roster = chooser::lay_roster(
                    key,
                    settings_control_rect(region, key),
                    picker,
                    region,
                    SET_OPTION_H,
                );
                for (n, (_, seat)) in roster.options.iter().enumerate() {
                    if contains(*seat, region.x + cx, region.y + cy) {
                        return Self::ChooseOption(key.clone(), n);
                    }
                }
            }
        }
        for row in spec::VALUE_ROWS {
            let seat = settings_control_rect(region, row.key);
            if contains(
                Rect::new(seat.x - region.x, seat.y - region.y, seat.w, seat.h),
                cx,
                cy,
            ) {
                return Self::Choose(row.key.to_owned());
            }
        }
        for (n, option) in spec::OPTIONS.iter().enumerate() {
            let seat = Self::option_seat(region, n);
            if contains(seat, cx, cy) {
                return Self::Option(option.key);
            }
        }
        for (n, row) in spec::KEY_ROWS.iter().enumerate() {
            let card = settings_group_rect(region, "keys");
            let local_row = Rect::new(0, u(n) * SET_ROW_H, card.w, SET_ROW_H);
            let seat = settings_ctrl_rect(local_row, SET_CTRL_W);
            if contains(
                Rect::new(card.x + seat.x, card.y + seat.y, seat.w, seat.h),
                cx,
                cy,
            ) {
                return Self::KeyRow(row.key);
            }
        }
        for n in 0..spec::THEMES.len() {
            if contains(Self::theme_seat(region, u(n)), cx, cy) {
                return Self::Theme(n);
            }
        }
        Self::Nothing
    }

    /// The region-space rectangle of the `n`th switch on the Settings page.
    fn option_seat(region: Rect, n: usize) -> Rect {
        let option = &spec::OPTIONS[n];
        let card = settings_group_rect(region, option.group);
        // ★★★★★ R1762 — the switches start below whatever VALUE rows the group
        // opens with, and this offset is the same derivation the paint uses.
        // Found by the gate rather than by inspection: adding two rows to the
        // capture group moved the paint and left this reading row 0, so a press
        // at the centre of a painted switch answered `nothing` — the
        // paint-and-gesture-read-two-facts class, caught the round it appeared.
        let within = settings_value_count(option.group)
            + u(spec::OPTIONS[..n]
                .iter()
                .filter(|o| o.group == option.group)
                .count());
        let row = Rect::new(0, within * SET_ROW_H, card.w, SET_ROW_H);
        let seat = settings_ctrl_rect(row, 64);
        Rect::new(card.x + seat.x, card.y + seat.y, seat.w, seat.h)
    }

    /// The region-space rectangle of a theme segment.
    fn theme_seat(region: Rect, n: u32) -> Rect {
        let card = settings_group_rect(region, "appearance");
        let seat = settings_ctrl_rect(Rect::new(0, 0, card.w, SET_ROW_H), SEG_W);
        let w = seg_chip_w();
        Rect::new(
            card.x + seat.x + SEG_PAD + n * w,
            card.y + seat.y + 1 + SEG_PAD,
            w,
            SEG_CHIP_H,
        )
    }

    /// What is under a point in the canvas's own space.
    ///
    /// Split out of [`Self::at`] because the canvas is the one region with a
    /// stacking order of its own — floats over cards, a card's controls over
    /// its body — and reading that order should not mean scrolling past the
    /// four chrome regions first.
    /// ★★★★★ R2021 — what a press at a BOARD-frame point does to an open card
    /// settings panel, or `None` when it misses one.
    ///
    /// Its own function rather than a branch inside [`Self::in_canvas`],
    /// because that one is at its line budget and the compiler said so — the
    /// R1999 repair, which is to lift a piece out rather than to raise the
    /// number. `None` and not `Nothing`: *the panel did not answer* has to be
    /// distinguishable from *the panel swallowed it*, and only the caller can
    /// carry on to the cards.
    fn in_card_config(state: &ShellState, cx: u32, cy: u32) -> Option<Self> {
        let (card, _, panel) = card_config_panel(state)?;
        if !contains(panel, cx, cy) {
            return None;
        }
        let id = card.id().as_str().to_owned();
        for (n, setting) in spec::card_settings_of(kind_of(&id)).into_iter().enumerate() {
            let (_, seat) = card_config_row_rects(panel, n);
            if contains(
                Rect::new(panel.x + seat.x, panel.y + seat.y, seat.w, seat.h),
                cx,
                cy,
            ) {
                return Some(Self::Choose(Valued::Card { card: id, setting }.key()));
            }
        }
        // The panel's own body swallows the press rather than letting it reach
        // whatever card is under it: a person aiming at a panel and moving a
        // card behind it is the class R1726 measured on the drag preview — the
        // thing you can see is not the thing that answers.
        Some(Self::Nothing)
    }

    fn in_canvas(state: &ShellState, cx: u32, cy: u32) -> Self {
        // ★ R1697 — floats are over the canvas, FRONTMOST first. It was the
        // vector read backwards, which is the same answer only while nothing
        // reorders them; a press now raises the panel it lands on, so the
        // stacking order is state and the hit test reads that state.
        // ★ R1891 — only the CANVAS-homed panels are over the canvas. A
        // window-homed card is reached by pressing in its own window, and
        // hit-testing for it here would return a panel this scene does not
        // paint.
        for float in state.floats_at(DetachHome::Canvas) {
            let rect = float_rect(&float);
            if !contains(rect, cx, cy) {
                continue;
            }
            // The corner is tested before the body, because it is inside it.
            // ★ R1681.3's invariant, and this is a case of it: a new affordance
            // drawn over an existing one has to be reached before it, and the
            // paint draws the grip last for the same reason.
            if contains(float_grip_rect(&float), cx, cy) {
                return Self::FloatResize(float.id.clone());
            }
            let (lx, ly) = (cx - rect.x, cy - rect.y);
            let header = header_rect(local(rect));
            if contains(header, lx, ly) {
                // ★★★★★ R1907 — the roster is the ONE derivation the paint
                // reads too. It was two hand-written `2`s here and two more in
                // the painter, so adding a third control meant editing four
                // constants that nothing compares — the standing
                // [[debt-paint-and-gesture-read-two-facts]] shape, in the exact
                // place a new affordance had to go.
                for (n, offered) in float_affordances().iter().enumerate() {
                    if !contains(float_affordance_rect(header, n), lx, ly) {
                        continue;
                    }
                    let id = float.id.clone();
                    return match offered {
                        DetachedAffordance::SendHome => Self::FloatHome(id),
                        DetachedAffordance::Redock => Self::FloatRedock(id),
                        DetachedAffordance::Close => Self::FloatClose(id),
                    };
                }
            }
            return Self::Float(float.id.clone());
        }
        // ★ R1662 — past the floats the question is about the BOARD, which
        // slides under the canvas. Every rectangle below is stated in the
        // board's own frame, so the offset is folded into the query once and
        // the two cannot drift.
        let (ox, oy) = state.canvas_scroll.offset();
        let (cx, cy) = (fold_by(cx, ox), fold_by(cy, oy));
        // ★★★★★ R2021 — an open settings panel hangs OVER its neighbours, so it
        // is asked before the cards. Asking it inside the card loop would test
        // it only while the press was also inside the card that opened it, and
        // half of this panel is deliberately outside one.
        if let Some(hit) = Self::in_card_config(state, cx, cy) {
            return hit;
        }
        let board = state.board.get();
        let editing = state.editing.get();
        for card in &state.placed() {
            let Some(tile) = board.tile(card.id()) else {
                continue;
            };
            // ★★★★★ R1900 — a shared cell is ONE card to the pointer: the one
            // in front. Without this the other occupant answers for the same
            // rectangle, and which of the two replied would be decided by the
            // roster's order rather than by what a person can see.
            if &tile.id != card.id() {
                continue;
            }
            let rect = cell_rect(tile);
            if !contains(rect, cx, cy) {
                continue;
            }
            let (lx, ly) = (cx - rect.x, cy - rect.y);
            let inside = local(rect);
            let id = card.id().as_str().to_string();
            let header = header_rect(inside);
            if contains(header, lx, ly) {
                // ★ R1900 — the strip is over the title, so it is asked before
                // the grip's catch-all and after nothing: the framework laid
                // the tabs inside the title box, which does not overlap a slot.
                if let Some(at) = card_strip(card, tile, inside).and_then(|s| s.at(lx, ly)) {
                    if let Some(member) = tile.members().get(at) {
                        return Self::Tab(member.as_str().to_string());
                    }
                }
                let offered = card.chrome().offered();
                for (n, affordance) in offered.iter().enumerate() {
                    if contains(affordance_rect(header, u(offered.len()), u(n)), lx, ly) {
                        return Self::Affordance(id, *affordance);
                    }
                }
                // The whole header drags, as it does in the reference; the grip
                // is where it SAYS so.
                return Self::Grip(id);
            }
            if editing {
                let bar = edit_bar_rect(inside);
                if contains(bar, lx, ly) {
                    for (n, (verb, _)) in STEPPERS.iter().enumerate() {
                        if contains(stepper_rect(bar, u(n)), lx, ly) {
                            return Self::Stepper(id, verb);
                        }
                    }
                    return Self::Card(id);
                }
            }
            if card.remedy().is_some_and(Remedy::is_actionable)
                && contains(remedy_rect(body_rect(inside, editing)), lx, ly)
            {
                return Self::Remedy(id);
            }
            // ★★★★★ R1721 — a press on a saved filter reaches the saved filter.
            // Measured before this arm existed, by driving the running screen:
            // the five chips announced `checked` and clicking every one of them
            // left every `checked` where it was. The rectangles are the paint's
            // own, so a chip drawn where it cannot be pressed is not a state this
            // card can be in.
            if kind_of(&id) == "filter" {
                let body = body_rect(inside, editing);
                for (n, at) in filter_chip_rects(body) {
                    if contains(at, lx, ly) {
                        return Self::FilterChip(id, n);
                    }
                }
            }
            // ★★★★★ R1851 — a press on an alarm heading reaches the sort. The
            // rectangles are the FEED's own placements, asked of the same
            // builder the painter uses, so a heading drawn where it cannot be
            // pressed is not a state this card can be in — which is the
            // measured failure mode of every second copy of a layout on this
            // screen ([[debt-paint-and-gesture-read-two-facts]]).
            if kind_of(&id) == "alarms" {
                let body = body_rect(inside, editing);
                for (n, at) in alarm_head_rects(body) {
                    if contains(at, lx, ly) {
                        return Self::AlarmColumn(id, n);
                    }
                }
            }
            return Self::Card(id);
        }
        Self::Nothing
    }

    /// ★ R1900 — the card whose HEADER BAND this hit landed on, if any.
    ///
    /// One place, because "is the pointer on a header" is answered by
    /// [`Self::at`] as three different arms and needed by [`berth_at`] as one
    /// fact. Deriving it here is what keeps the two from parting company the
    /// next time a header grows a control.
    fn header_card(&self) -> Option<&str> {
        match self {
            Self::Grip(id) | Self::Affordance(id, _) | Self::Tab(id) => Some(id),
            _ => None,
        }
    }

    fn card_id(&self) -> Option<&str> {
        match self {
            Self::Affordance(id, _)
            | Self::Remedy(id)
            | Self::Card(id)
            | Self::Grip(id)
            | Self::FilterChip(id, _)
            | Self::AlarmColumn(id, _)
            | Self::Tab(id)
            | Self::Stepper(id, _) => Some(id),
            _ => None,
        }
    }
}

/// A detached panel's rectangle, in the canvas's own space.
///
/// ★ R1697 — the one function the paint AND the hit test read, kept that way
/// deliberately ([[debt-paint-and-gesture-read-two-facts]]): a resizable panel
/// whose corner is computed twice is a corner that can be drawn in one place
/// and grabbed in another.
const fn float_rect(float: &Float) -> Rect {
    Rect::new(float.x, float.y, float.w, float.h)
}

/// A float's position in the space its own home speaks, as the signed pair a
/// [`pinion_core::detach::Transfer`] crosses.
///
/// Signed because the display's space is: a monitor left of the primary has
/// negative coordinates in it. The stored pair is unsigned because a canvas
/// float's rectangle cannot be — see the note at the one call site that
/// converts back.
#[allow(
    clippy::cast_possible_wrap,
    reason = "a float's stored position is bounded by the window it was placed in"
)]
const fn at_of(float: &Float) -> (i32, i32) {
    (float.x as i32, float.y as i32)
}

/// The corner a resize is grabbed by, in the same space as [`float_rect`].
const fn float_grip_rect(float: &Float) -> Rect {
    let rect = float_rect(float);
    Rect::new(
        rect.x + rect.w.saturating_sub(FLOAT_GRIP),
        rect.y + rect.h.saturating_sub(FLOAT_GRIP),
        FLOAT_GRIP,
        FLOAT_GRIP,
    )
}

/// One hit, named by the **scene tag** of the thing that was hit.
///
/// R1614's lesson — a name that has to survive is an address, not a
/// description — and the demo enforces it by sweeping the window and requiring
/// every name this returns to be a tag the paint actually emitted.
/// ★ R1700 — the same word, in the shape the framework's pointer census reads:
/// `Nothing` where a press addresses nothing, so that "there is nothing here"
/// and "here is a thing called nothing" cannot be confused.
fn word_or_nothing(hit: &Hit) -> PointerTarget {
    match hit {
        Hit::Nothing => PointerTarget::Nothing,
        other => PointerTarget::Word(hit_word(other)),
    }
}

fn hit_word(hit: &Hit) -> String {
    match hit {
        Hit::Chip(chip) => chip.tag().to_string(),
        Hit::Sub(chip) => chip.tag().to_string(),
        Hit::PresetItem(n) => format!("shell.preset.item.{n}"),
        Hit::Rail(name) => format!("shell.rail.{name}"),
        Hit::Option(key) => format!("shell.settings.option.{key}"),
        Hit::KeyRow(key) => format!("shell.settings.key.{key}"),
        // ★ R2021 — both are the ROW's own tags now. They used to be built here
        // from the preferences page's spelling, which was right while every
        // roster was that page's; a card's setting is addressed by the card, so
        // asking the row is what keeps the driver pressing the name the paint
        // published rather than one this function invents.
        Hit::Choose(key) => Valued::from_key(key).map_or_else(String::new, |v| v.control_tag()),
        Hit::ChooseOption(key, n) => Valued::from_key(key).map_or_else(String::new, |valued| {
            valued.option_tag(valued.options().get(*n).map_or("", String::as_str))
        }),
        Hit::Theme(n) => format!("shell.settings.theme.{n}"),
        Hit::Palette(kind) => format!("shell.palette.{kind}"),
        Hit::Grip(id) => format!("card.{id}.grip"),
        Hit::Affordance(id, affordance) => format!("card.{id}.{}", affordance.wire()),
        Hit::Stepper(id, verb) => format!("card.{id}.{verb}"),
        Hit::Remedy(id) => format!("card.{id}.remedy"),
        Hit::PaletteFold => format!("{PALETTE_HEAD}fold"),
        Hit::PaletteStrip => "shell.palette.strip".to_owned(),
        // ★ R1900 — the tab names the OCCUPANT it selects, not the cell it is
        // drawn in. The cell's name is whichever occupant is in front, so a tag
        // built from it would change under a person's finger every time they
        // pressed a tab.
        Hit::Tab(id) => format!("card.{id}.tab"),
        Hit::FilterChip(id, n) => format!("card.{id}.chip.{n}"),
        Hit::AlarmColumn(id, n) => format!("card.{id}.feed.head.col#{n}"),
        Hit::Card(id) => format!("card.{id}"),
        // ★ R1907 — the tag is the affordance's own wire word, so what a
        // pointer answers and what the paint tagged are one name.
        Hit::FloatHome(id) => format!("float.{id}.{}", DetachedAffordance::SendHome.wire()),
        Hit::FloatRedock(id) => format!("float.{id}.redock"),
        Hit::FloatClose(id) => format!("float.{id}.close"),
        Hit::FloatResize(id) => format!("float.{id}.resize"),
        Hit::Float(id) => format!("float.{id}"),
        Hit::Nothing => "nothing".to_string(),
    }
}

/// The chords this shell claims, published as well as painted.
const KEYMAP: [(&str, &str); 12] = [
    ("/", "type into the global search; Enter or Escape leaves"),
    ("Arrow", "move the selection to the neighbouring card"),
    ("Shift+Arrow", "move the selected card one cell"),
    ("Alt+Arrow", "grow that side of the card"),
    ("Alt+Shift+Arrow", "shrink that side"),
    ("Enter", "maximise the selection, or restore"),
    ("Escape", "restore a maximised board"),
    ("Delete", "close the selected card"),
    ("e", "toggle layout edit mode"),
    ("o", "detach the selected card, or re-dock it"),
    ("r", "act on the selected card's remedy"),
    ("c / t / s", "capture / theme / source"),
];

/// ★ R1701 — "double it to max" is in this list because the gesture has NO
/// AFFORDANCE. Every other way to maximise a card is a thing a person can see:
/// a button in the header, a key named in the keymap panel. A double-click is
/// invisible, which is exactly why a screen that offers one has to say so — a
/// capability built and not announced is the mirror of one announced and not
/// built, and this screen has been on both sides of that.
///
/// ★★ And "Enter max" came OUT to pay for it, because the band this is painted
/// in is 470 logical pixels and the sentence had already filled it. Measured
/// rather than guessed: the run reported `ink_w: 468` and the renderer was
/// eliding at "Esc restor…". The rule that decided which item leaves is the
/// same one that decided the new item belongs — `Enter` duplicates a button
/// that is on screen, and the keymap panel names it in full.
const HELP_STRIP: &str = "drag a header \u{00B7} double it to max \u{00B7} e edit \u{00B7} \
     o detach \u{00B7} Esc restore \u{00B7} Del close \u{00B7} / search";

// --- The oracle (primary External) ------------------------------------------

/// ★★ R1714.1 — and it no longer keeps a size.
///
/// R1656 gave it one because `External::pointer_move` hands a FRACTION of the
/// widget and not the rectangle, so a consumer wanting pixels had to hold the
/// basis; R1684.4 made the framework answer that and left the field, because
/// the multiplication was still written here. `external::layout_point` carries
/// the whole expression now, so this became a field every resize wrote and
/// nothing read.
struct ShellOracle {
    state: Option<Rc<ShellState>>,
}

impl core::fmt::Debug for ShellOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ShellOracle")
            .field("attached", &self.state.is_some())
            .finish_non_exhaustive()
    }
}

impl ShellOracle {
    const NO_STATE: &str = "this shell surface is not bound to a model yet";

    const fn new() -> Self {
        Self { state: None }
    }

    fn attach_state(&mut self, state: Rc<ShellState>) {
        self.state = Some(state);
    }

    fn state(&self) -> Result<&Rc<ShellState>, InvokeError> {
        self.state
            .as_ref()
            .ok_or_else(|| InvokeError::rejected(Self::NO_STATE))
    }

    fn text(arg: &IntrospectValue) -> Result<String, InvokeError> {
        match arg {
            IntrospectValue::Text(s) => Ok(s.clone()),
            // ★★★★ R1720 — the KIND, not the `Debug` spelling; the node lab
            // carried the same line and R1720's gate read them both. See
            // [`IntrospectValue::kind`] for the measurement.
            other => Err(InvokeError::rejected(format!(
                "this action takes text and was given {}",
                other.kind()
            ))),
        }
    }

    fn card_of(state: &ShellState, id: &str) -> Result<Card, InvokeError> {
        state.card(id).ok_or_else(|| {
            InvokeError::rejected(format!(
                "no card {id:?} on this board; it holds {}",
                state.card_ids()
            ))
        })
    }

    /// The reads that take a card id, answered together.
    fn card_read(
        state: &Rc<ShellState>,
        path: &str,
        args: &IntrospectValue,
    ) -> Option<Result<IntrospectValue, InvokeError>> {
        let wanted = matches!(
            path,
            "title" | "chrome" | "section" | "state" | "detail" | "remedy" | "actionable" | "cell"
        );
        if !wanted {
            return None;
        }
        let card = match Self::text(args).and_then(|id| Self::card_of(state, id.trim())) {
            Ok(card) => card,
            Err(why) => return Some(Err(why)),
        };
        let id = card.id().as_str().to_string();
        let answer = match path {
            "title" => card.title().to_string(),
            "chrome" => offered_words(&card),
            "section" => def_for_card(&id).map_or_else(String::new, |d| d.section.to_string()),
            "state" => card.state().wire().to_string(),
            "detail" => card.state().detail().unwrap_or("").to_string(),
            "remedy" => remedy_word(card.remedy()).to_string(),
            // Where it is, or that it is nowhere on the board.
            "cell" => state.board.get().tile(card.id()).map_or_else(
                || "detached".to_string(),
                |t| format!("{},{},{},{}", t.col, t.row, t.w, t.h),
            ),
            _ => if card.remedy().is_some_and(Remedy::is_actionable) {
                "yes"
            } else {
                "no"
            }
            .to_string(),
        };
        Some(Ok(IntrospectValue::Text(answer)))
    }

    /// `sort_alarms` — put the alarm feed in an order, as `<column>:<direction>`.
    ///
    /// ★ The order is set here and NOWHERE else derives a direction: the
    /// header's indicator reads this same value through
    /// `HeaderFeed::with_sort`, so a client that sorts descending cannot get an
    /// ascending arrow.
    fn sort_alarms(state: &Rc<ShellState>, raw: &str) -> Result<IntrospectValue, InvokeError> {
        let (column, direction) = raw.split_once(':').ok_or_else(|| {
            InvokeError::rejected(format!(
                "{raw:?} is not <column>:<direction>; the columns are {} and the \
                 directions are {}",
                ALARM_COLUMN_KEYS.join(" / "),
                ALARM_DIRECTIONS.join(" / ")
            ))
        })?;
        let (column, direction) = (column.trim(), direction.trim());
        let n = ALARM_COLUMN_KEYS
            .iter()
            .position(|known| *known == column)
            .ok_or_else(|| {
                InvokeError::rejected(format!(
                    "{column:?} is not an alarm column; they are {}",
                    ALARM_COLUMN_KEYS.join(" / ")
                ))
            })?;
        // ★ `none` is the feed's own word and the other two are the framework's,
        // read through `sort_dir_from_str` rather than matched here — so the verb
        // accepts exactly what the wire grammar accepts.
        let sort = match direction {
            "none" => None,
            word => Some((
                n,
                sort_dir_from_str(word).ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "{word:?} is not a direction; they are {}",
                        ALARM_DIRECTIONS.join(" / ")
                    ))
                })?,
            )),
        };
        state.alarm_sort.set(sort);
        // A new order means the window starts again: keeping the offset would
        // leave a reader looking at row 12 of an order they have just replaced.
        state.alarm_scroll.scroll_to(0, 0);
        state.say(Utterance::done(format!(
            "alarms sorted by {}",
            grid_sort_str(sort)
        )));
        Ok(IntrospectValue::Text(grid_sort_str(sort)))
    }

    /// `filter_alarms` — keep only alarms at least this severe.
    ///
    /// ★★★★★ The REFUSAL is the point. Measured on the toolkit floor at 6.11.1,
    /// filtering rows spelled `err` by the word `error` answers *zero of six*
    /// and says nothing — and the behaviour prototype this build reproduces ships
    /// exactly that mismatch between its control's words and its rows'. Here the
    /// word goes through [`spec::SEVERITY`], which refuses one it does not hold
    /// and names the vocabulary in the refusal.
    fn filter_alarms(state: &Rc<ShellState>, raw: &str) -> Result<IntrospectValue, InvokeError> {
        if raw == "all" {
            state.alarm_floor.set(None);
            state.alarm_scroll.scroll_to(0, 0);
            state.say(Utterance::done("alarms show every severity"));
            return Ok(IntrospectValue::Text("all".to_owned()));
        }
        // ⚠ `require` and not `rank().is_some()`: the refusal carries the word
        // AND the vocabulary, which is what makes it actionable rather than a
        // second way of saying no.
        spec::SEVERITY
            .require(raw)
            .map_err(|why| InvokeError::rejected(why.to_string()))?;
        state.alarm_floor.set(Some(raw.to_owned()));
        state.alarm_scroll.scroll_to(0, 0);
        state.say(Utterance::done(format!("alarms show {raw} and above")));
        Ok(IntrospectValue::Text(raw.to_owned()))
    }

    /// `act` — perform one header affordance on one card.
    ///
    /// The whole point of the header being a *set*: an affordance the card does
    /// not offer is refused **by name**, before anything happens. A shell that
    /// only hides the button leaves the wire able to do what the screen says is
    /// impossible.
    fn act(state: &Rc<ShellState>, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let raw = Self::text(args)?;
        let (id, word) = raw
            .split_once(',')
            .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <card>,<affordance>")))?;
        let (id, word) = (id.trim(), word.trim());
        let card = Self::card_of(state, id)?;
        let affordance = CardAffordance::from_wire(word).ok_or_else(|| {
            InvokeError::rejected(format!(
                "{word:?} is not an affordance; they are {}",
                CardAffordance::ALL.map(CardAffordance::wire).join(" / ")
            ))
        })?;
        if !card.chrome().offers(affordance) {
            return Err(InvokeError::rejected(format!(
                "card {id:?} does not offer {word}; its header offers {}",
                offered_words(&card)
            )));
        }
        match affordance {
            CardAffordance::Settings => {
                let open = state.config_open.get().as_deref() == Some(id);
                state
                    .config_open
                    .set(if open { None } else { Some(id.to_string()) });
                state.say(Utterance::done(format!(
                    "{} settings {}",
                    label_of(id),
                    if open { "closed" } else { "opened" }
                )));
            }
            CardAffordance::TearOff => return Self::detach(state, id),
            // ★★★★★ R1697 — the header control **toggles**, and it did not.
            //
            // Found by this round's own operations gate on its first run: the
            // card was maximised with a press and the same press then called
            // `maximize` again, which refuses with "a card is already
            // maximised; restore first". So a person who maximised a card with
            // the mouse had no way back with the mouse — only `Escape`, or the
            // wire. Every window control that grows a thing shrinks it again;
            // nothing had ever asked this one to.
            //
            // The two WIRE verbs stay separate and precise (`maximize` and
            // `restore` each refuse when they do not apply), because an agent
            // asking for a specific outcome should not get a toggle. This is
            // the affordance, and an affordance is a press.
            CardAffordance::Maximize => {
                return if state.maximized.get().is_some_and(|m| m.id().as_str() == id) {
                    Self::restore(state)
                } else {
                    Self::maximize(state, id)
                };
            }
            CardAffordance::Close => {
                Self::remove(state, id);
                state.say(Utterance::done(format!("{} removed", label_of(id))));
            }
        }
        Ok(IntrospectValue::Text(format!("{id} {word}")))
    }

    /// Take a card off the board and out of the deck.
    fn remove(state: &Rc<ShellState>, id: &str) {
        let mut board = state.board.get();
        board.remove(&TileId::new(id)).ok();
        state.board.set(board);
        state.cards.set(
            state
                .cards
                .get()
                .into_iter()
                .filter(|c| c.id().as_str() != id)
                .collect(),
        );
        state.floats.set(
            state
                .floats
                .get()
                .into_iter()
                .filter(|f| f.id != id)
                .collect(),
        );
        if state.selected.get().as_deref() == Some(id) {
            state.selected.set(None);
        }
    }

    /// ★ Detach REMOVES the card from the board and opens a floating panel.
    ///
    /// R1648 kept the tile in place and argued that a temporary gesture should
    /// not reflow a layout. The reference tool does remove it, and re-docking
    /// appends at the bottom — so a detach IS a layout edit there, and on this
    /// axis the reference is the specification rather than a data point.
    fn detach(state: &Rc<ShellState>, id: &str) -> Result<IntrospectValue, InvokeError> {
        if state.is_floating(id) {
            return Err(InvokeError::rejected(format!(
                "card {id:?} is already detached"
            )));
        }
        Self::card_of(state, id)?;
        let mut board = state.board.get();
        board.remove(&TileId::new(id)).ok();
        state.board.set(board);
        let n = u(state.floats.get().len());
        let z = {
            let mut counter = state.float_z.borrow_mut();
            *counter += 1;
            *counter
        };
        let mut floats = state.floats.get();
        floats.push(Float {
            id: id.to_string(),
            x: 120 + n * FLOAT_STEP,
            y: 40 + n * FLOAT_STEP,
            // R1697 — the reference's opening size, per panel from here on
            // rather than for all of them at once.
            w: FLOAT_W,
            h: FLOAT_H,
            // A panel arrives in front, which is also what its `detachWidget`
            // does — it takes `floatZ + 1` in the same breath as its position.
            z,
            // R1826 — ordinary stacking until a reader asks otherwise.
            on_top: false,
            // ★ R1891 — where it goes is the HOST's answer, not this
            // function's. `preferred()` on a windowing host is a window, which
            // is what R1826 built; what changes is that the canvas float is no
            // longer painted alongside it.
            home: detach_policy().preferred(),
        });
        state.floats.set(floats);
        state.say(Utterance::done(format!(
            "{} \u{2192} detached window",
            label_of(id)
        )));
        Ok(IntrospectValue::Text(format!("{id} tear_off")))
    }

    /// Put a detached card back — at the bottom of the board, as the reference
    /// does. The card lost its cell when it left, and inventing one back would
    /// be a third placement rule nobody asked for.
    fn redock(state: &Rc<ShellState>, id: &str) -> Result<IntrospectValue, InvokeError> {
        if !state.is_floating(id) {
            return Err(InvokeError::rejected(format!(
                "card {id:?} is not detached"
            )));
        }
        let (cols, rows) = kind_span(kind_of(id))
            .ok_or_else(|| InvokeError::rejected(format!("no specified cell size for {id:?}")))?;
        let mut board = state.board.get();
        let row = board.rows();
        board
            .place(Tile::new(id, 0, row, cols, rows))
            .map_err(|why| InvokeError::rejected(why.to_string()))?;
        state.board.set(board);
        state.floats.set(
            state
                .floats
                .get()
                .into_iter()
                .filter(|f| f.id != id)
                .collect(),
        );
        state.say(Utterance::done(format!("{} re-docked", label_of(id))));
        Ok(IntrospectValue::Text(format!("{id} redock")))
    }

    /// ★★★★★ R1826 — keep a detached card's window above the application, or
    /// stop.
    ///
    /// The specification's *always-on-top option*, and a TOGGLE for the reason
    /// R1697 gave the maximise control one: every window control that does a
    /// thing undoes it, and a reader who put a panel on top with the wire and
    /// could not take it off again would have a switch with one position.
    ///
    /// Refuses for a card that is not detached. A card on the board has no
    /// window to level, and answering `ok` would be a claim about a window that
    /// does not exist — the shape this round was opened by, one layer down.
    fn set_on_top(state: &Rc<ShellState>, id: &str) -> Result<IntrospectValue, InvokeError> {
        if !state.is_floating(id) {
            return Err(InvokeError::rejected(format!(
                "card {id:?} is not detached, so it has no window to keep on top"
            )));
        }
        let mut floats = state.floats.get();
        let mut now = false;
        for float in &mut floats {
            if float.id == id {
                float.on_top = !float.on_top;
                now = float.on_top;
            }
        }
        state.floats.set(floats);
        state.say(Utterance::done(format!(
            "{} {}",
            label_of(id),
            if now {
                "kept on top"
            } else {
                "no longer on top"
            }
        )));
        Ok(IntrospectValue::Bool(now))
    }

    /// ★★★★★ R1891 — **move a detached card between the places this host can
    /// put it**, or refuse naming what it can.
    ///
    /// `detach_home <card>,<window|canvas>`. Keyed rather than a bare word, for
    /// R1889's reason one verb over: the grammar must not depend on whether an
    /// argument happens to parse as something else.
    ///
    /// Both refusals are the framework's, not this file's: the host's
    /// [`DetachPolicy`] admits or refuses, and the refusal carries the homes
    /// that WOULD have worked. A terminal-hosted build of this same binding
    /// refuses `window` here and says so.
    /// ★★★★★ R1900 — `share <card>,<with>`: put `card` into the place `with`
    /// occupies.
    fn share_place(
        state: &Rc<ShellState>,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let text = Self::text(args)?;
        let (member, host) = text
            .split_once(',')
            .ok_or_else(|| InvokeError::rejected("expected <card>,<with>"))?;
        let (member, host) = (member.trim(), host.trim());
        let mut board = state.board.get();
        let shared = board
            .share(&TileId::new(member), &TileId::new(host))
            .map_err(|why| InvokeError::rejected(why.to_string()))?;
        state.board.set(board);
        let names: Vec<String> = shared
            .members
            .iter()
            .map(|m| label_of(m.as_str()))
            .collect();
        state.say(Utterance::done(format!(
            "{} shares a place with {}",
            label_of(member),
            names.join(", ")
        )));
        Ok(IntrospectValue::Text(names.join(",")))
    }

    /// ★★★★★ R1900 — `unshare <card>,<col>,<row>`: give `card` a cell of its
    /// own, the size of the one it was sharing.
    fn unshare_place(
        state: &Rc<ShellState>,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let text = Self::text(args)?;
        let mut parts = text.split(',');
        let (Some(member), Some(col), Some(row), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(InvokeError::rejected("expected <card>,<col>,<row>"));
        };
        let member = member.trim();
        let cell = |what: &str, raw: &str| {
            raw.trim().parse::<u32>().map_err(|_| {
                InvokeError::rejected(format!("{what} must be a whole number, not {raw:?}"))
            })
        };
        let (col, row) = (cell("col", col)?, cell("row", row)?);
        let mut board = state.board.get();
        board
            .unshare(&TileId::new(member), col, row)
            .map_err(|why| InvokeError::rejected(why.to_string()))?;
        state.board.set(board);
        state.say(Utterance::done(format!(
            "{} has a place of its own at ({col},{row})",
            label_of(member)
        )));
        Ok(IntrospectValue::Text(format!("{member},{col},{row}")))
    }

    /// ★★★★★ R1903 — `palette <fold|unfold|toggle>`: put the palette away, or
    /// bring it back.
    ///
    /// **The one place the placement changes.** The header control, the strip,
    /// the sub bar's add button and a client all arrive here, so the screen and
    /// an agent cannot come to mean different things by the same act — the rule
    /// R1887 established for the sibling screen's panels.
    ///
    /// The policy is asked rather than assumed: `admit_fold` is what refuses a
    /// fold on a panel that declared it does not fold, and its refusal carries
    /// the sentence a person is shown. A screen that folded without asking
    /// would be the habit this axis exists to end — a declared constraint
    /// quietly losing to an imperative call.
    fn place_palette(state: &Rc<ShellState>, verb: &str) -> Result<EdgePlacement, InvokeError> {
        let at = state.palette_at.get();
        let want = match verb.trim() {
            "fold" => true,
            "unfold" => false,
            "toggle" => !at.folded,
            other => {
                return Err(InvokeError::rejected(format!(
                    "the palette folds and unfolds; {other:?} is neither"
                )));
            }
        };
        let placed = spec::PALETTE_POLICY
            .admit_fold(at, want)
            .map_err(|why| InvokeError::rejected(why.reason().to_string()))?;
        state.palette_at.set(placed);
        // ★★★★★ R1908 — and it OUTLIVES THE RUN. A person who puts the palette
        // away and comes back tomorrow finds it away; before this round the
        // placement was re-seeded from the specification at every boot, so the
        // gesture R1903 built was undone by closing the application.
        //
        // Written here rather than at a shutdown hook because there is no
        // moment this application is told it is ending — the same argument
        // `persist_arrangements` makes for writing after each change to the set.
        // The fold is now a person's arrangement, so it is stored where a
        // person's arrangements are.
        state.palette_restored.set(false);
        persist_arrangements(state);
        state.say(Utterance::done(if placed.folded {
            "palette put away \u{2014} the strip brings it back".to_owned()
        } else {
            // The canon's own sentence for `openPalette`, which is what makes
            // the re-opening tell a reader what the panel is FOR.
            "palette open \u{2014} drag a widget onto the canvas".to_owned()
        }));
        Ok(placed)
    }

    /// ★★★★★ R1900 — `reveal <card>`: bring an occupant of a shared place to
    /// the front, which is what a press on its tab does.
    ///
    /// Revealing what is already in front is a legal, uninteresting outcome
    /// rather than an error — the framework models it as `was == now` — so this
    /// answers with both halves and lets the caller see which it got.
    fn reveal_tab(state: &Rc<ShellState>, id: &str) -> Result<IntrospectValue, InvokeError> {
        let mut board = state.board.get();
        let moved = board
            .reveal(&TileId::new(id))
            .map_err(|why| InvokeError::rejected(why.to_string()))?;
        state.board.set(board);
        state.say(Utterance::done(format!("{} is in front", label_of(id))));
        Ok(IntrospectValue::Text(format!(
            "{},{}",
            moved.was, moved.now
        )))
    }

    /// The PARSING half: a wire string becomes a card and a [`HomeRequest`].
    ///
    /// ★ R1907 — split from [`Self::send_home`], which is the verb. Two
    /// channels reach that verb now — this one and the header control — and a
    /// verb that also parses is a verb only one of them can use, which is how
    /// this screen came to have a home nobody could change by hand.
    fn set_detach_home(
        state: &Rc<ShellState>,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let text = Self::text(args)?;
        let (id, want) = text
            .split_once(',')
            .ok_or_else(|| InvokeError::rejected("expected <card>,<window|canvas|next>"))?;
        let (id, want) = (id.trim(), want.trim());
        let want = want.strip_prefix("home=").unwrap_or(want);
        // ★★★★★ R1907 — `next` is a request this channel can make too, so the
        // wire is not weaker than the hand. A client driving this tool
        // headlessly can say "somewhere else" without first reading the policy
        // and computing the answer — which is the computation that would be a
        // second spelling of it.
        let asked = HomeRequest::from_wire(want).ok_or_else(|| {
            InvokeError::rejected(format!(
                "a detached panel lives in a window or on the canvas, or goes to \
                 the next one; not {want:?}"
            ))
        })?;
        Self::send_home(state, id, asked)
    }

    /// ★★★★★ R1907 — the ONE verb that changes where a detached panel lives.
    ///
    /// Both channels arrive here: the wire through [`Self::set_detach_home`],
    /// and a person through the control on the panel's own header. R1903 built
    /// this shape one gesture over and the reason is the same — two paths that
    /// do the same thing differently are two behaviours, and only one of them
    /// gets tested.
    ///
    /// The request is resolved by the POLICY ([`HomeRequest::resolve`]), never
    /// here: this function does not know that the home after a window is the
    /// canvas, and a painter that did would be the policy's second spelling.
    fn send_home(
        state: &Rc<ShellState>,
        id: &str,
        asked: HomeRequest,
    ) -> Result<IntrospectValue, InvokeError> {
        if !state.is_floating(id) {
            return Err(InvokeError::rejected(format!(
                "card {id:?} is not detached, so it has no home to move"
            )));
        }
        let from = state
            .float(id)
            .map(|f| f.home)
            .ok_or_else(|| InvokeError::rejected(format!("card {id:?} is not detached")))?;
        let home = asked
            .resolve(detach_policy(), from)
            .map_err(|refusal| InvokeError::rejected(refusal.reason().to_string()))?;
        // ★★★★★ R1905 — the numbers CROSS, they are not merely relabelled.
        //
        // `Float { home, ..f }` was this line, and it changed which space the
        // rectangle is read against while leaving the rectangle alone. See
        // [`shell_transfer`] for what that measured.
        let transfer = shell_transfer();
        let mut arrival = None;
        let floats = state
            .floats
            .get()
            .into_iter()
            .map(|f| {
                if f.id != id {
                    return f;
                }
                let arrived = transfer.cross(f.home, home, at_of(&f), (f.w, f.h));
                arrival = Some(arrived.how());
                let (x, y) = arrived.at();
                Float {
                    home,
                    // A host origin is at or past the display's corner and a
                    // host coordinate is non-negative, so a crossing INTO the
                    // display's space cannot go below zero from here. The one
                    // arrangement that would — this window on a monitor left of
                    // or above the primary — needs the display topology, which
                    // `Transfer` deliberately does not hold (its header says
                    // why), and is this round's stated residue.
                    x: u32::try_from(x).unwrap_or(0),
                    y: u32::try_from(y).unwrap_or(0),
                    ..f
                }
            })
            .collect();
        state.floats.set(floats);
        // The card was checked to be floating above, so the map arm above ran
        // for it and this is `Some`. `expect` rather than a default: a default
        // would be this round's own escape hatch, one type down from the one
        // `Arrival` refuses.
        let arrival = arrival.expect("the card was found floating, so its crossing ran");
        state.arrival.set(Some(arrival));
        state.say(Utterance::done(format!(
            "{} \u{2192} {} ({})",
            label_of(id),
            home.as_str(),
            arrival.as_str()
        )));
        Ok(IntrospectValue::Text(format!(
            "{id} {} {}",
            home.as_str(),
            arrival.as_str()
        )))
    }

    /// ★★★★★ R1733 — what the palette OFFERS of a kind: its catalogue entry
    /// and the footprint the specification gives it.
    ///
    /// Lifted out of [`Self::add`] because the drag needs the same three
    /// refusals *at pick-up*, and a second copy of them would be a second
    /// wording of "this row does not place a card" — which is the shape the
    /// R1668 comment right below warns about, one gesture over.
    fn offered(kind: &str) -> Result<(&'static spec::WidgetSpec, (u32, u32)), InvokeError> {
        // ★★★★★ R1734 — the board's PUBLISHED declaration is asked first.
        //
        // Not because this path could otherwise take the wrong thing — every
        // caller here is already inside the dashboard — but because the
        // declaration is what `$drop` and `scene/drop_targets` answer with, and
        // a declaration nothing consults is a claim rather than a contract. The
        // list that tells an agent "yes, a widget footprint lands here" is now
        // the list this refuses against, so the two cannot say different things
        // about the same build. The refusal is the framework's own sentence,
        // for the same reason: a second wording of one rule is a second rule.
        if let Err(refusal) = Self::declared_drop_contract().admits(
            BOARD_WIDGET_DRAG_KIND,
            DropActions::one(DropAction::Copy),
            None,
        ) {
            return Err(InvokeError::rejected(refusal.sentence()));
        }
        let def = def_of(kind.trim()).ok_or_else(|| {
            InvokeError::rejected(format!(
                "{kind:?} is not a widget kind; the palette offers {}",
                spec::CATALOGUE
                    .iter()
                    .filter(|w| w.tier == spec::Tier::Placeable)
                    .map(|w| w.kind)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
        // R1668 — a reserved kind refuses, and the refusal carries the reason
        // rather than a bare "no". The palette paints the same fact (the row is
        // declared unavailable, so it is inert, faded and announced with its
        // reason), and this is that fact on the invoke path — the two cannot
        // drift because both read `reserved_for` from the specification.
        if def.tier == spec::Tier::Reserved {
            return Err(InvokeError::rejected(format!(
                "{:?} is reserved for {} and this release does not place it",
                def.kind, def.reserved_for
            )));
        }
        let span = kind_span(def.kind).ok_or_else(|| {
            InvokeError::rejected(format!("{:?} has no specified cell size", def.kind))
        })?;
        Ok((def, span))
    }

    /// R1733 — the id the next card of a kind would take, **without consuming
    /// the counter**.
    ///
    /// A drag that is picked up and then abandoned must leave nothing behind,
    /// and bumping an ordinal is a change ("what is being carried is not yet a
    /// value" — the rule R1732 built the picker on). The counter moves in
    /// [`Self::add_at`], where a card is actually placed.
    fn prospective_id(state: &Rc<ShellState>, def: &spec::WidgetSpec) -> String {
        format!("{}#{}", def.kind, *state.next_id.borrow())
    }

    /// ★★★★★ R1734 — **what this screen accepts from a drag**, declared once.
    ///
    /// One clause, and the two actions are both real: a palette row is
    /// **copied** (the row stays where it is and the board gains a card) and a
    /// card already on the board is **moved**. Naming them separately is what
    /// lets a client ask the narrower question — *can I move something here* —
    /// and be answered without dragging anything.
    ///
    /// A whole-surface region rather than a part list, and the reason is
    /// measured rather than a preference: this screen hit-tests itself, so it
    /// paints one scene tag and a drop point over it carries no `#sub` half for
    /// a part clause to match. Declaring parts the wire cannot resolve would
    /// publish a promise the router could never keep. The board's own region
    /// test stays where it is, inside the gesture — see `on_board_ignoring`.
    pub(crate) const fn declared_drop_contract() -> DropContract {
        DropContract::new(
            const {
                &[DropClause::surface(
                    BOARD_WIDGET_DRAG_KIND,
                    DropActions::one(DropAction::Copy).with(DropAction::Move),
                )]
            },
        )
    }

    /// ★★★★★ R1733 — pick a palette row's widget up: the drag the pointer will
    /// carry until it releases.
    ///
    /// Refused **here**, while the person is still holding it, rather than at
    /// the drop — and by the same three sentences the action gives.
    fn pick_up(state: &Rc<ShellState>, kind: &str) -> Result<TileDrag, InvokeError> {
        let (def, (cols, rows)) = Self::offered(kind)?;
        let id = Self::prospective_id(state, def);
        TileDrag::pick(&state.board.get(), id, cols, rows)
            .map_err(|why| InvokeError::rejected(why.to_string()))
    }

    /// Place a new card of that kind at `at`, or at the bottom of the board
    /// when the caller names no cell.
    ///
    /// ★★★★★ R1733 — the placement goes through the same [`TileDrag`] the
    /// pointer gesture does, so a drop, a palette click and a wire call are one
    /// arithmetic. A cell the caller invented (`add` now takes one) is clamped
    /// by the board's own rule instead of trusted, and the preview a drag drew
    /// and the cell this places at cannot differ, because `hover` is the
    /// function that resolved both.
    fn add_at(
        state: &Rc<ShellState>,
        kind: &str,
        at: Option<(u32, u32)>,
    ) -> Result<IntrospectValue, InvokeError> {
        let board = state.board.get();
        let mut drag = Self::pick_up(state, kind)?;
        let (col, row) = at.unwrap_or((0, board.rows()));
        drag.hover(&board, col, row);
        Self::place_carried(state, drag)
    }

    /// ★★★★★ R1733 — put a carried footprint down: the board takes the tile,
    /// and a card, a selection and a sentence follow it.
    ///
    /// The pointer gesture hands its **live** drag here, so the object that
    /// resolved the preview is the object that commits — not a second one built
    /// from a remembered cell. [`Self::add_at`] builds one and hands it over
    /// too, so a palette click, a wire call and a drop are one path with one
    /// arithmetic and one set of refusals.
    fn place_carried(
        state: &Rc<ShellState>,
        drag: TileDrag,
    ) -> Result<IntrospectValue, InvokeError> {
        let Carried::Fresh { id, .. } = drag.carried() else {
            return Err(InvokeError::rejected(
                "a card already on the board is moved, not added",
            ));
        };
        let id = id.as_str().to_string();
        // The prospective id names the kind before its `#ordinal`, so the
        // catalogue entry is derived from what is carried rather than passed
        // beside it — one fact, not two that can disagree.
        let (def, _) = Self::offered(id.split('#').next().unwrap_or_default())?;
        let mut board = state.board.get();
        match drag.drop_on(&mut board) {
            Ok(Dropped::Landed { .. }) => {}
            Ok(other) => {
                return Err(InvokeError::rejected(format!(
                    "a carry with no landing places nothing ({other:?})"
                )));
            }
            Err(why) => return Err(InvokeError::rejected(why.to_string())),
        }
        *state.next_id.borrow_mut() += 1;
        state.board.set(board);
        let mut cards = state.cards.get();
        cards.push(
            Card::new(id.clone(), def.label)
                .with_chrome(CardChrome::of(chrome()))
                .with_state(CardState::Ready),
        );
        state.cards.set(cards);
        state.selected.set(Some(id.clone()));
        state.say(Utterance::done(format!("{} added", def.label)));
        Ok(IntrospectValue::Text(id))
    }

    /// The wire's `add`: a kind, and optionally the cell to place it in.
    ///
    /// R1733 — the cell is what the pointer gesture resolves, so an agent can
    /// reach the same placement a person's drag reaches. Without it the wire
    /// could only ever append at the bottom, and the new gesture would be one
    /// no agent could perform — the §2 #2 rule that the headless path is the
    /// primary one, not a subset.
    fn add(state: &Rc<ShellState>, call: &str) -> Result<IntrospectValue, InvokeError> {
        let mut parts = call.split(',');
        let kind = parts.next().unwrap_or_default();
        let at = match (parts.next(), parts.next()) {
            (None, None) => None,
            (Some(col), Some(row)) => Some((Self::cell_word(col)?, Self::cell_word(row)?)),
            _ => {
                return Err(InvokeError::rejected(
                    "a placement names both a column and a row, or neither",
                ));
            }
        };
        if parts.next().is_some() {
            return Err(InvokeError::rejected(
                "add takes a kind, and optionally a column and a row",
            ));
        }
        Self::add_at(state, kind, at)
    }

    /// One cell coordinate off the wire.
    fn cell_word(word: &str) -> Result<u32, InvokeError> {
        word.trim()
            .parse::<u32>()
            .map_err(|_| InvokeError::rejected(format!("{word:?} is not a cell coordinate")))
    }

    fn maximize(state: &Rc<ShellState>, id: &str) -> Result<IntrospectValue, InvokeError> {
        if state.maximized.get().is_some() {
            return Err(InvokeError::rejected(
                "a card is already maximised; restore first",
            ));
        }
        let mut board = state.board.get();
        let token = board
            .maximize(&TileId::new(id))
            .map_err(|why| InvokeError::rejected(why.to_string()))?;
        state.board.set(board);
        state.maximized.set(Some(token));
        state.say(Utterance::done(format!("{} maximised", label_of(id))));
        Ok(IntrospectValue::Text(format!("{id} maximize")))
    }

    fn restore(state: &Rc<ShellState>) -> Result<IntrospectValue, InvokeError> {
        let token = state
            .maximized
            .get()
            .ok_or_else(|| InvokeError::rejected("no card is maximised"))?;
        let id = token.id().as_str().to_string();
        state.board.set(token.restore());
        state.maximized.set(None);
        state.say(Utterance::done(format!("{} restored", label_of(&id))));
        Ok(IntrospectValue::Text(id))
    }

    /// A size stepper: one cell wider, narrower, taller or shorter.
    fn step(state: &Rc<ShellState>, id: &str, verb: &str) -> Result<IntrospectValue, InvokeError> {
        let tile_id = TileId::new(id);
        let mut board = state.board.get();
        let tile = board
            .tile(&tile_id)
            .ok_or_else(|| InvokeError::rejected(format!("card {id:?} is not on the board")))?;
        let (w, h) = match verb {
            "widen" => (tile.w + 1, tile.h),
            "narrow" => (tile.w.saturating_sub(1).max(1), tile.h),
            "taller" => (tile.w, tile.h + 1),
            "shorter" => (tile.w, tile.h.saturating_sub(1).max(1)),
            other => {
                return Err(InvokeError::rejected(format!(
                    "{other:?} is not a size step; they are {}",
                    STEPPERS.map(|(verb, _)| verb).join(" / ")
                )));
            }
        };
        board
            .resize(&tile_id, w, h)
            .map_err(|why| InvokeError::rejected(why.to_string()))?;
        state.board.set(board);
        // ★★★★★ R1952 — "is now", where this said `U+2192`. The face this tree
        // ships has no glyph for that arrow, so every resize a person made put
        // a `.notdef` box in the middle of the sentence the shell said back to
        // them. A mark inside a sentence cannot become a drawn path — it has to
        // flow with the words — so a sentence gets words.
        state.say(Utterance::done(format!(
            "{} is now {w}\u{00D7}{h}",
            label_of(id)
        )));
        Ok(IntrospectValue::Text(format!("{w}x{h}")))
    }

    fn set_state(
        state: &Rc<ShellState>,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let raw = Self::text(args)?;
        let mut parts = raw.splitn(3, ',');
        let id = parts.next().unwrap_or_default().trim();
        let word = parts
            .next()
            .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <card>,<state>[,<why>]")))?
            .trim();
        let detail = parts.next().map(str::trim).filter(|d| !d.is_empty());
        Self::card_of(state, id)?;
        let next = parse_state(word, detail).map_err(InvokeError::rejected)?;
        let remedy = next.remedy();
        state.update_card(id, |card| card.set_state(next.clone()));
        state.say(Utterance::done(format!(
            "{} is {}",
            label_of(id),
            next.wire()
        )));
        Ok(IntrospectValue::Text(format!(
            "{id} {} {}",
            next.wire(),
            remedy_word(remedy)
        )))
    }

    /// The app bar's own writable slots — source, capture, theme.
    ///
    /// `None` when `path` is not one of them, so the dispatcher stays a list.
    /// They are grouped because they are one region of the screen and each one
    /// refuses the same way: by naming what it does accept.
    fn write_bar(
        state: &Rc<ShellState>,
        path: &str,
        value: &IntrospectValue,
    ) -> Option<Result<(), InterveneError>> {
        let word = |value: &IntrospectValue| match value {
            IntrospectValue::Text(s) => Ok(s.trim().to_string()),
            _ => Err(InterveneError::TypeMismatch),
        };
        Some(match path {
            "source" => word(value).and_then(|name| {
                let chosen = SOURCES.iter().find(|s| **s == name).ok_or_else(|| {
                    InterveneError::out_of_range(format!(
                        "{name:?} is not a source; they are {}",
                        SOURCES.join(", ")
                    ))
                })?;
                state.source.set((*chosen).to_string());
                state.say(Utterance::done(format!("source {chosen}")));
                Ok(())
            }),
            "capturing" => match value {
                IntrospectValue::Bool(on) => {
                    state.capturing.set(*on);
                    state.say(Utterance::done(format!(
                        "capture {}",
                        if *on { "on" } else { "off" }
                    )));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "theme" => word(value).and_then(|name| {
                let mode = match name.as_str() {
                    "light" => ThemeMode::Light,
                    "dark" => ThemeMode::Dark,
                    "system" => ThemeMode::System,
                    other => {
                        return Err(InterveneError::out_of_range(format!(
                            "{other:?} is not a theme; they are light / dark / system"
                        )));
                    }
                };
                state.theme.set_mode(mode);
                state.say(Utterance::done(format!("theme {name}")));
                Ok(())
            }),
            _ => return None,
        })
    }

    fn apply_preset(state: &Rc<ShellState>, name: &str) -> Result<(), InterveneError> {
        // ★ R1893 — the refusal is the framework's, so the sentence a person
        // reads here and the one an agent reads from a dock editor are the same
        // rule rather than two wordings of it.
        let preset = state
            .presets
            .borrow()
            .apply(name)
            .cloned()
            .map_err(|refusal| InterveneError::out_of_range(refusal.reason().to_string()))?;
        // A preset restores BOTH facts. Restoring only the arrangement would
        // put the previous board's cards into the new layout's holes.
        state.maximized.set(None);
        state.floats.set(Vec::new());
        state.board.set(preset.board);
        state.cards.set(preset.cards);
        state.preset.set(name.to_string());
        state.preset_open.set(false);
        state.say(Utterance::done(format!("layout \u{201C}{name}\u{201D}")));
        Ok(())
    }

    fn save_preset(state: &Rc<ShellState>, name: &str) -> Result<IntrospectValue, InvokeError> {
        // ★ R1893 — both refusals are the framework's now: an unnamed
        // arrangement, and one that would overwrite what this application
        // ships. The second did not exist before, and its absence was not a
        // missing check — it was a set that could not tell the two apart.
        state
            .presets
            .borrow_mut()
            .save(
                name,
                Preset {
                    board: state.board.get(),
                    cards: state.cards.get(),
                },
            )
            .map_err(|refusal| InvokeError::rejected(refusal.reason().to_string()))?;
        state.preset.set(name.trim().to_string());
        // ★ R1897 — the set changed, so it is written. After the signal, so a
        // reader of the file and a reader of the wire see the same set.
        persist_arrangements(state);
        state.say(Utterance::done(format!(
            "layout saved \u{00B7} {}",
            name.trim()
        )));
        Ok(IntrospectValue::Text(state.preset_names()))
    }

    /// ★★★★★ R1893 — **delete a saved arrangement**, which the behaviour canon
    /// has (`deleteCustomPreset`) and this shell did not.
    ///
    /// It could not have it before: the set was a bare map in which the
    /// arrangement this application opens on looked exactly like one a person
    /// saved, so a delete would have taken the opening layout with no way back.
    /// The refusal that makes it safe is the framework's
    /// ([`pinion_core::workspace`]), and it names what to do instead.
    ///
    /// Deleting the arrangement that is CURRENT leaves the board alone — the
    /// cards on screen are not the preset, they are what the preset produced,
    /// and clearing the board because a menu row went away would be a delete
    /// doing something no reader asked for. The name shown falls back to the
    /// application's own, because a screen labelled with an arrangement that no
    /// longer exists is a screen lying about where its board came from.
    fn delete_preset(state: &Rc<ShellState>, name: &str) -> Result<IntrospectValue, InvokeError> {
        state
            .presets
            .borrow_mut()
            .delete(name)
            .map_err(|refusal| InvokeError::rejected(refusal.reason().to_string()))?;
        if state.preset.get() == name {
            state.preset.set(spec::PRESET.to_string());
        }
        // ★ R1897 — a delete changes the set too, so a deleted arrangement
        // stays deleted across a restart. Without this the file would bring it
        // back and the delete would look like it had not happened.
        persist_arrangements(state);
        state.say(Utterance::done(format!("layout deleted \u{00B7} {name}")));
        Ok(IntrospectValue::Text(state.preset_names()))
    }
}

fn remedy_word(remedy: Option<Remedy>) -> &'static str {
    remedy.map_or("none", Remedy::wire)
}

fn offered_words(card: &Card) -> String {
    card.chrome()
        .offered()
        .into_iter()
        .map(CardAffordance::wire)
        .collect::<Vec<_>>()
        .join(",")
}

/// Parse `<state>` plus its optional detail into a [`CardState`].
///
/// The arity check is a fact about the vocabulary: two arms take a reason and
/// four do not.
fn parse_state(word: &str, detail: Option<&str>) -> Result<CardState, String> {
    let carried = |what: &str| -> Result<std::borrow::Cow<'static, str>, String> {
        detail
            .map(|d| std::borrow::Cow::Owned(d.to_string()))
            .ok_or_else(|| format!("{what} carries a reason: send \"<card>,{what},<why>\""))
    };
    let plain = |made: CardState| -> Result<CardState, String> {
        if detail.is_some() {
            return Err(format!(
                "{} carries no reason; the same explanation every time is not one",
                made.wire()
            ));
        }
        Ok(made)
    };
    match word {
        "ready" => plain(CardState::Ready),
        "loading" => plain(CardState::Loading),
        "empty" => plain(CardState::Empty),
        "opaque" => plain(CardState::Opaque),
        "failed" => Ok(CardState::Failed(carried("failed")?)),
        "denied" => Ok(CardState::Denied(carried("denied")?)),
        other => Err(format!(
            "{other:?} is not a card state; they are {}",
            CardState::ALL
                .iter()
                .map(CardState::wire)
                .collect::<Vec<_>>()
                .join(" / ")
        )),
    }
}

/// Everything this surface publishes.
const FIELDS: &[SchemaField] = const {
    &[
        // the app bar
        SchemaField::new("source", "string"),
        SchemaField::new("sources", "string"),
        // ★ R1762 — the preferences page's value rows.
        SchemaField::new("retention", "string"),
        SchemaField::new("retentions", "string"),
        SchemaField::new("picking", "string"),
        SchemaField::new("capturing", "bool"),
        SchemaField::new("search", "string"),
        SchemaField::new("theme", "string"),
        SchemaField::new("tab", "string"),
        SchemaField::new("tabs", "string"),
        // R1668 — the reference screen this shell claims to reproduce, as the
        // table a gate reads. Published so the demo compares the running
        // application against the specification rather than against a second
        // copy of it (the failure R1649's sweep exists to prevent, one level up).
        SchemaField::new("spec", "json"),
        // the rail and the sub bar
        SchemaField::new("rail", "string"),
        SchemaField::new("reserved_rail", "json"),
        SchemaField::new("nav", "string"),
        // ★★ R1695 — where the rail can take you, what each seat's standing is,
        // and which one is showing. `nav` above says only the last of those.
        SchemaField::new("destinations", "json"),
        // ★★ R1728 — how much of the reference's navigation this build
        // reproduces, and every place it does not, with the reason each
        // difference was accepted. `destinations` reports what IS on the rail;
        // this reports the rail against the rail it is supposed to be.
        SchemaField::new("conformance", "json"),
        // ★★★★★ R1738 — how much of ITS OWN specification each section of this
        // application reproduces, one row per destination.
        //
        // `conformance` above is about the rail: eight seats specified, eight
        // reproduced. That sentence was the only conformance this application
        // published, and read as a statement about the tool it was wrong —
        // measured over this wire before this slot existed, four of the six
        // open sections had never been compared with anything and nothing said
        // so. This slot is the population, so a section is missing from it only
        // by not being in the application.
        SchemaField::new("sections", "json"),
        // ★★★★★ R1918 — what the marks on this frame say about themselves, in
        // two populations: the chrome around every page, and the page this host
        // paints at the destination it is at. Plus the rectangle that page
        // occupies, which is what lets a reader tell the two apart on the FRAME
        // rather than on this host's word.
        SchemaField::new("described", "json"),
        // ★★★★★ R1767 — the same population, judged over the WALK a reader is
        // taking rather than over the frame in front of them.
        //
        // `sections` above cannot ever say this application reproduces its
        // specification, and that is not a defect in it: one frame paints one
        // section, so every other section is away and an away surface
        // reconciles nothing. Measured over this wire before this slot existed,
        // walking all six open sections and returning left the headline at
        // `26 of 133` — the boot number, honestly. This slot is where the walk
        // itself is the unit, and each credited verdict names the step it was
        // read at so nothing is credited to a frame nobody saw.
        SchemaField::new("journey", "json"),
        // The Settings destination's switches.
        SchemaField::new("options", "json"),
        SchemaField::new("editing", "bool"),
        SchemaField::new("config_open", "string"),
        // ★★★★★ R1851 — the alarm feed, whole. Not `alarm_sort` and
        // `alarm_floor` as two string slots: the order, the threshold and the
        // rows are one reading, and a client that took them separately could
        // see a threshold from one frame beside rows from another.
        SchemaField::new("alarms", "json"),
        // ★★★★★ R1851 — and the two verbs that move it. Both declare a CLOSED
        // vocabulary, which is the half the reference cannot state: probed at
        // 6.11.1, its row filtering is a predicate over a string, so the set of
        // words that mean anything is whatever the rows happen to be spelled
        // and a client discovers it by getting zero rows back.
        SchemaField::action_with(
            "sort_alarms",
            "string",
            ArgForm::Scalar,
            const {
                &[
                    SchemaArg::one_of("column", "string", ALARM_COLUMN_KEYS),
                    SchemaArg::one_of("direction", "string", ALARM_DIRECTIONS),
                ]
            },
        ),
        SchemaField::action_with(
            "filter_alarms",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::one_of("severity", "string", ALARM_FLOORS)] },
        ),
        // the catalogue and the board
        SchemaField::new("catalogue", "string"),
        // ★★ R1735 — what a release would do RIGHT NOW, as the router judged
        // it. `drag` below says where the board would put the carry; this says
        // whether the drop happens at all, and when it does not, why.
        SchemaField::new("drop_standing", "json"),
        SchemaField::new("cards", "string"),
        SchemaField::new("card_count", "int"),
        SchemaField::new("placed_count", "int"),
        SchemaField::new("layout", "string"),
        SchemaField::new("maximized", "string"),
        SchemaField::new("restore_to", "string"),
        SchemaField::new("floating", "string"),
        SchemaField::new("floats", "json"),
        // R1891 — the homes this host can put a detached card in.
        SchemaField::new("detach_policy", "json"),
        // R1905 — how the last card that changed home got where it is.
        SchemaField::new("arrival", "json"),
        // R1826 — which OS window carries each detached card.
        SchemaField::new("detached", "json"),
        SchemaField::new("float_grab", "string"),
        // ★★★★★ R1898 — which side of the board's edge the gesture in flight
        // would end on, and whether ending there moves the card between them.
        // The floor keeps this bit private; here it is a slot, so an agent that
        // never saw the pointer can ask what letting go would do.
        SchemaField::new("crossing", "json"),
        // named layouts
        SchemaField::new("preset", "string"),
        SchemaField::new("presets", "string"),
        // R1893 — the same set as rows: name, provenance, and whether the row
        // offers a delete.
        SchemaField::new("arrangements", "json"),
        SchemaField::new("preset_open", "bool"),
        // the transport
        SchemaField::new("transport", "string"),
        SchemaField::new("playhead", "int"),
        // the published vocabularies
        SchemaField::new("affordances", "string"),
        SchemaField::new("states", "string"),
        SchemaField::new("remedies", "string"),
        SchemaField::new("steppers", "string"),
        SchemaField::new("toast", "string"),
        SchemaField::new("said", "object"),
        // ★★★★★ R1790 — how long what is being said has left, so a gate advances
        // time by asking rather than by pinning a number this screen owns.
        SchemaField::new("saying", "json"),
        // direct manipulation
        SchemaField::new("cursor", "string"),
        SchemaField::new("selected", "string"),
        SchemaField::new("hit", "string"),
        SchemaField::new("keymap", "string"),
        SchemaField::new("drag", "string"),
        SchemaField::new("carrying", "string"),
        // per-card reads
        SchemaField::action_with(
            "title",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "chrome",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "section",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "state",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "detail",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "remedy",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "actionable",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "cell",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        // the verbs
        SchemaField::action_with(
            "act",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("card", "string", "cards"),
                    SchemaArg::key("affordance", "string", "affordances"),
                ]
            },
        ),
        SchemaField::action_with(
            "set_state",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("card", "string", "cards"),
                    SchemaArg::key("state", "string", "states"),
                    SchemaArg::key("why", "string", "states").optional(),
                ]
            },
        ),
        SchemaField::action_with(
            "resize",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("card", "string", "cards"),
                    SchemaArg::key("step", "string", "steppers"),
                ]
            },
        ),
        // R1733 — a kind, and optionally the cell a drop would put it in. The
        // two cell arguments are declared optional together; `add` refuses one
        // without the other rather than guessing the missing half.
        SchemaField::action_with(
            "add",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("kind", "string", "catalogue"),
                    SchemaArg::key("col", "int", "layout").optional(),
                    SchemaArg::key("row", "int", "layout").optional(),
                ]
            },
        ),
        SchemaField::action("maximize", "string"),
        SchemaField::action("restore", "string"),
        SchemaField::action("redock", "string"),
        // R1826 — the specification's always-on-top option, per detached card.
        SchemaField::action("on_top", "string"),
        // R1891 — `<card>,<window|canvas>`: where a detached card lives.
        SchemaField::action("detach_home", "string"),
        // ★★★★★ R1903 — `fold` / `unfold` / `toggle`: the canon's own two verbs
        // plus the toggle its control performs, as one action with a closed
        // vocabulary that refuses anything else by name.
        SchemaField::action("palette", "string"),
        // ★★★★★ R1900 — a place two cards share. Three verbs rather than one
        // with a mode word, because each takes a different argument and a
        // grammar that says so is a grammar a client can be refused against.
        SchemaField::action_with(
            "share",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("card", "string", "the card that joins"),
                    SchemaArg::key("with", "string", "a card of the place it joins"),
                ]
            },
        ),
        SchemaField::action_with(
            "unshare",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("card", "string", "the card that leaves"),
                    SchemaArg::key("col", "int", "column of its own cell"),
                    SchemaArg::key("row", "int", "row of its own cell"),
                ]
            },
        ),
        SchemaField::action("reveal", "string"),
        SchemaField::action("save_preset", "string"),
        // R1893 — the canon's delete, and the refusal that makes it safe.
        SchemaField::action("delete_preset", "string"),
        SchemaField::action("seek", "string"),
        SchemaField::action_with(
            "point",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("x", "int", "cursor"),
                    SchemaArg::key("y", "int", "cursor"),
                ]
            },
        ),
        SchemaField::action("send", "string"),
        SchemaField::action("key", "string"),
    ]
};

/// R1697 — every detached panel's geometry, front to back.
///
/// Its own function so the reply is one place and so the read arm stays inside
/// the length the lints allow; the order IS the stacking order, so a client
/// never has to sort for it.
/// R1826 — which OS window carries each detached card, front to back.
///
/// Beside [`floats_json`] and for its reason: the reply is one place, and the
/// read arm stays inside the length the lints allow — which this round proved
/// is not a formality, having pushed `query` to 104 lines by writing the arm
/// inline.
fn detached_json(state: &ShellState) -> serde_json::Value {
    serde_json::Value::Array(
        state
            .detached()
            .into_iter()
            .map(|(card, window)| serde_json::json!({"card": card, "window": window}))
            .collect(),
    )
}

fn floats_json(state: &ShellState) -> serde_json::Value {
    serde_json::Value::Array(
        state
            .floats_front_to_back()
            .iter()
            .map(|f| {
                serde_json::json!({
                    "id": f.id, "x": f.x, "y": f.y, "w": f.w, "h": f.h, "z": f.z,
                    // ★★★★★ R1891 — WHERE it is. Without this a client reading
                    // a float saw a rectangle and had no way to know which
                    // coordinate space it is in: the display's, or this
                    // window's canvas. Two different questions were answerable
                    // only by cross-referencing `detached`, and a card missing
                    // from there was indistinguishable from one this slot had
                    // not caught up with.
                    "home": f.home.as_str(),
                    // ★★★★★ R1905 — and WHICH SPACE those four numbers are in,
                    // said rather than derivable.
                    //
                    // ⚠ The paragraph above claims `home` already answers this.
                    // It does not: it answers which HOME, and a client still has
                    // to hold the mapping from home to space — which is a fact
                    // about this framework that nothing on the wire stated. The
                    // two homes' spaces are what a crossing converts between, so
                    // the name of the space is what a client needs to reason
                    // about a position at all.
                    "space": f.home.space().as_str(),
                })
            })
            .collect(),
    )
}

/// ★★★★★ R1905 — how the last card that changed home got where it is.
///
/// `null` until one has, which is not [`Arrival::Adrift`]: an unasked question
/// and an unconvertible one are different answers and a client branching on
/// them acts differently.
fn arrival_json(state: &ShellState) -> serde_json::Value {
    match state.arrival.get() {
        Some(arrival) => serde_json::json!({
            "how": arrival.as_str(),
            "exact": arrival.is_exact(),
            // The offset a crossing WOULD use, so a client can tell "this host
            // cannot convert" from "it converted and nothing moved".
            "knows_offset": shell_transfer().knows_offset(),
        }),
        None => serde_json::Value::Null,
    }
}

/// ★ R1898 — everything a caller can ask about what a POINTER is doing.
///
/// Split out of `query` for the reason [`query_arrangements`] below was, and by
/// the same rule: this `match` reached the length this workspace lints for when
/// the crossing slot was added, and the honest split is by subject. A reader
/// asking "what is the hand doing" finds seven slots together — where the
/// cursor is, what is under it, what is selected, what is being carried and in
/// what form, which panel is being moved, and what letting go would do.
///
/// Returns `UnknownPath` for anything else, so the caller's `match` and this
/// function cannot disagree about which paths belong here.
fn query_gesture(state: &ShellState, path: &str) -> Result<IntrospectValue, ReadRefusal> {
    match path {
        "cursor" => {
            let (x, y) = state.cursor.get();
            Ok(IntrospectValue::Text(format!("{x},{y}")))
        }
        "selected" => Ok(IntrospectValue::Text(
            state.selected.get().unwrap_or_default(),
        )),
        "hit" => {
            let (x, y) = state.cursor.get();
            Ok(IntrospectValue::Text(hit_word(&Hit::at(state, x, y))))
        }
        "drag" | "carrying" => Ok(carry_slot(state, path)),
        // What the pointer is doing to a panel right now, or empty. The peer of
        // `drag`, and separate for the reason the types are separate: one
        // gesture lands in a cell and the other in a pixel.
        "float_grab" => Ok(IntrospectValue::Text(
            state.float_grab.get().map_or_else(String::new, |grab| {
                format!("{},{}", grab.id, if grab.edge { "resize" } else { "move" })
            }),
        )),
        // ★★★★★ R1898 — what letting go RIGHT NOW would do, refusal and all.
        // The same value the release reads, so a client cannot be told one
        // thing and shown another.
        "crossing" => Ok(IntrospectValue::Json(crossing_json(state))),
        _ => Err(ReadRefusal::UnknownPath),
    }
}

/// ★ R1893 — everything a caller can ask about the SET of named arrangements.
///
/// Split out of `query` because that `match` reached the length this workspace
/// lints for, and the split is by subject: a reader looking for "what does this
/// application know about saved layouts" finds four slots together rather than
/// four arms scattered through sixty.
///
/// Returns `UnknownPath` for anything else, so the caller's `match` and this
/// function cannot disagree about which paths belong here — adding a path in
/// one place and not the other fails closed rather than silently answering
/// nothing.
fn query_arrangements(state: &ShellState, path: &str) -> Result<IntrospectValue, ReadRefusal> {
    match path {
        "preset" => Ok(IntrospectValue::Text(state.preset.get())),
        "presets" => Ok(IntrospectValue::Text(state.preset_names())),
        // ★★★★★ The same set, WITH each row's provenance and whether it offers
        // a delete. `presets` answers the names a menu shows; this answers what
        // each row can DO, so a client draws the delete control on the rows
        // that have one instead of offering it everywhere and finding out by
        // being refused (§2 #2).
        "arrangements" => Ok(IntrospectValue::Json(arrangements_json(state))),
        "preset_open" => Ok(IntrospectValue::Bool(state.preset_open.get())),
        _ => Err(ReadRefusal::UnknownPath),
    }
}

/// ★★★★★ R1893 §2 #2 — **the arrangements as ROWS**, each saying where it came
/// from and whether it can be removed.
///
/// `deletable` is read from the framework's own rule
/// ([`pinion_core::workspace::Provenance::deletable`]) rather than compared
/// against the word here, so what this publishes and what `delete_preset`
/// enforces cannot come apart. Publishing the word alone would leave a client
/// to re-derive the rule, which is how two readers come to disagree.
fn arrangements_json(state: &ShellState) -> serde_json::Value {
    serde_json::Value::Array(
        state
            .presets
            .borrow()
            .iter()
            .map(|(name, arrangement)| {
                serde_json::json!({
                    "name": name,
                    "provenance": arrangement.provenance.as_str(),
                    "deletable": arrangement.provenance.deletable(),
                })
            })
            .collect(),
    )
}

/// ★★★★★ R1891 §2 #2 — **what this host can do with a detached card**, so an
/// agent asks before it is refused rather than after.
///
/// The floor toolkit at 6.11 has no such surface, and does not need one: a
/// panel it detaches is always a top-level window, so there is nothing to
/// choose and nothing to publish. Here the answer varies with the backend
/// (§2 #6), which makes it a fact a caller has to be able to obtain.
fn detach_policy_json() -> serde_json::Value {
    let policy = detach_policy();
    serde_json::json!({
        "homes": policy.homes().iter().map(|h| h.as_str()).collect::<Vec<_>>(),
        "preferred": policy.preferred().as_str(),
        // ★★★★★ R1907 — what a detached panel's HEADER offers, and where "the
        // next home" leads from each home.
        //
        // Both are derived from `homes` above, and both are published anyway,
        // for the reason R1642 made a declaration a precondition of dispatch: a
        // client that has to re-derive them is holding a second copy of this
        // host's policy, free to disagree the day a third home appears. The
        // roster also says whether the control EXISTS — a host with one home
        // draws no send-home control, and a client that only saw `homes` would
        // have to know that rule to predict it.
        "affordances": policy
            .detached_affordances()
            .iter()
            .map(|a| a.wire())
            .collect::<Vec<_>>(),
        // A map rather than a bare "next": which home follows depends on where
        // the panel is, and publishing one answer would be right for one panel
        // and wrong for the rest. Absent entirely when this host has one home,
        // which is the honest report of "there is no next one" — an entry
        // mapping a home to itself would read as a move that changes nothing.
        "next_from": policy
            .homes()
            .iter()
            .filter_map(|from| {
                let to = policy.next_home(*from).ok()?;
                Some((from.as_str().to_owned(), serde_json::Value::from(to.as_str())))
            })
            .collect::<serde_json::Map<String, serde_json::Value>>(),
    })
}

impl ExternalIntrospect for ShellOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(FIELDS)
    }

    /// ★★★★★ R1734 §5.51 §2 #2 — **what may be handed to this screen**,
    /// answerable before anything is picked up.
    ///
    /// The reference prototype's board takes its widgets by drag and drop, and
    /// R1733 reproduced the gesture. What no prototype and no mature toolkit
    /// can offer is this: the accept set as *data*, so an agent asks where a
    /// thing can land instead of dragging it somewhere to find out. Measured on
    /// the toolkit floor at 6.11.1, acceptance there is one boolean per widget
    /// whose real decision lives inside an event handler that has to run — see
    /// [`pinion_core::drop_target`] for the probe.
    ///
    /// [`ShellOracle::declared_drop_contract`] is the single name this and
    /// [`ShellOracle::offered`] both read, so the answer published on the wire
    /// is the rule the screen actually enforces.
    fn drop_contract(&self) -> DropContract {
        ShellOracle::declared_drop_contract()
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| ReadRefusal::unavailable("no capture is loaded"))?;
        let text = |s: String| Ok(IntrospectValue::Text(s));
        let clock = &state.clock;
        match path {
            "source" => text(state.source.get()),
            "sources" => text(SOURCES.join(",")),
            // ★ R1762 — the preferences page's value rows answer through one
            // helper rather than three arms here, because this match is at the
            // line limit and a slot added to a page should not have to argue
            // with the size of a function about a different one.
            "retention" | "retentions" | "picking" => text(settings_slot(state, path)),
            "capturing" => Ok(IntrospectValue::Bool(state.capturing.get())),
            "search" => text(state.search.get()),
            "theme" => text(theme_word(&state.theme)),
            "tab" => text(state.tab.get()),
            "tabs" => text(tabs().join(",")),
            "spec" | "rail" | "reserved_rail" | "catalogue" | "conformance" => {
                read_specification(path)
            }
            "nav" => text(state.at()),
            // ★★ R1695 — the roster and the position, in one published value
            // built by the framework so two screens of one product cannot
            // publish the same fact in two shapes.
            "destinations" => Ok(IntrospectValue::Json(
                // ★ R1724 — the roster's wire, plus which destinations are
                // whole screens. An agent that had to infer that from tag
                // prefixes would be inferring a rule nobody wrote down.
                state.screens.wire(&state.journey.get()),
            )),
            // ★★★★★ R1918 — **what the marks on this frame say about
            // themselves**, split into the two populations that are two
            // different claims: the chrome around every page, and the page this
            // host paints at the destination it is at.
            //
            // Published rather than left to a gate to re-derive, because a gate
            // that spelled the register a second time would be comparing this
            // screen against a copy of itself. The region tag travels with it so
            // a reader knows which mark on the frame to look at, which is the
            // one thing a client cannot guess.
            "described" => Ok(IntrospectValue::Json(described_wire(state))),
            "sections" => Ok(IntrospectValue::Json(sections_json(state))),
            "journey" => Ok(IntrospectValue::Json(journey_json(state))),
            "options" => Ok(IntrospectValue::Json(options_json(state))),
            "editing" => Ok(IntrospectValue::Bool(state.editing.get())),
            "config_open" => text(state.config_open.get().unwrap_or_default()),
            // ★★★★★ R1735 — **what letting go right now would do**, in the
            // framework's own words, for a client that is holding something.
            //
            // The peer of `scene/drop_targets`, which answers the same question
            // with nothing in hand. This one is the LIVE answer, and it is the
            // value the router handed this surface rather than a re-derivation:
            // a reader that sees `accepted` here and then releases gets that
            // landing, because the release commits the same acceptance.
            "drop_standing" => Ok(IntrospectValue::Json(standing_value(
                &state.standing.borrow(),
            ))),
            // ★★★★★ R1851 — the alarm feed as data: its vocabulary, its
            // threshold, its order, the rows it kept and the rows it BUILT.
            "alarms" => Ok(IntrospectValue::Json(alarms_wire(state))),
            "cards" => text(state.card_ids()),
            "card_count" => Ok(IntrospectValue::Int(i64::from(u(state.cards.get().len())))),
            "placed_count" => Ok(IntrospectValue::Int(i64::from(u(state.placed().len())))),
            "layout" => text(
                serde_json::to_string(&state.board.get()).unwrap_or_else(|why| why.to_string()),
            ),
            "maximized" => text(
                state
                    .maximized
                    .get()
                    .map_or_else(String::new, |m| m.id().as_str().to_string()),
            ),
            "restore_to" => text(state.maximized.get().map_or_else(String::new, |m| {
                serde_json::to_string(m.peek()).unwrap_or_else(|why| why.to_string())
            })),
            "floating" => text(floating_ids(state)),
            // ★★★★★ R1697 — **where each detached panel is, how big, and which
            // is in front**, which nothing published.
            //
            // `floating` above is the roster and answers "which cards have
            // left the board"; this is their geometry, and without it a moved
            // panel is invisible to every reader that is not a screenshot.
            // That is the whole reason the defect survived: the operation had
            // no witness a test could name, so no test could name it.
            //
            // Front to back, so the order IS the stacking order rather than
            // something a reader has to sort for.
            "floats" => Ok(IntrospectValue::Json(floats_json(state))),
            // R1891 — the homes this host offers, and which it picks by
            // default. A client reads this before asking for one.
            "detach_policy" => Ok(IntrospectValue::Json(detach_policy_json())),
            // ★★★★★ R1905 — and how the last crossing between two homes went.
            // A client reading only the new position cannot tell a converted
            // place from an unconverted one.
            "arrival" => Ok(IntrospectValue::Json(arrival_json(state))),
            // ★★★★★ R1826 — **what is detached, and WHERE IT WENT.**
            //
            // `floating` says which cards left the board and `floats` says
            // where their panels sit inside the canvas; neither says which OS
            // WINDOW now carries one, because until this round none did. A
            // caller that wanted to snapshot a torn-off card had to know this
            // application's window-naming convention and rebuild the id.
            //
            // This is the axis the reference toolkit at 6.11 has no answer for:
            // a floated dock widget there gets a top-level container and the
            // correspondence lives in whatever the caller wrote down — the
            // nearest available signal is walking the parent chain of a widget
            // the caller already holds, which cannot answer for a panel it does
            // not. Published as a slot, it is answerable by an agent that never
            // saw the gesture: `scene/query .../detached` then
            // `scene/snapshot {window: <that id>}`.
            "detached" => Ok(IntrospectValue::Json(detached_json(state))),
            // ★ R1893 — the arrangement questions are answered one function
            // over. Not a tidy-up: this `match` had reached the length limit
            // this workspace lints for, and the honest split is by SUBJECT
            // rather than by whichever arm happened to be last. See
            // `query_arrangements`.
            "preset" | "presets" | "arrangements" | "preset_open" => {
                query_arrangements(state, path)
            }
            "transport" => text(transport_word(clock.status(), state.capturing.get())),
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a playhead fraction is 0.0..=1.0, so per-mille is 0..=1000"
            )]
            "playhead" => Ok(IntrospectValue::Int(i64::from(
                (clock.position() * 1000.0).round() as i32,
            ))),
            "affordances" => text(CardAffordance::ALL.map(CardAffordance::wire).join(",")),
            "states" => text(
                CardState::ALL
                    .iter()
                    .map(CardState::wire)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "remedies" => text(Remedy::ALL.map(Remedy::wire).join(",")),
            "steppers" => text(STEPPERS.map(|(verb, _)| verb).join(",")),
            "toast" => text(state.toast.sentence()),
            // ★★★★★ R1719 — the same fact with its KIND on it, spelled `said`
            // on all three screens of this tool. `toast` stays the sentence a
            // person reads, because that is what its readers ask for.
            //
            // ★ R1778 — an OPTION on the wire now, because a sentence whose
            // time is up is not being said. A client that read the old shape
            // could not tell "nothing is showing" from "the last thing is still
            // showing", which is the fact the lifetime introduced.
            "said" => Ok(IntrospectValue::Json(
                serde_json::to_value(state.toast.showing())
                    .map_err(|_| ReadRefusal::UnknownPath)?,
            )),
            // ★★★★★ R1790 — the sentence AND how long it has. `said` answers
            // WHAT is showing; this answers how long it will be, which is the
            // fact a gate needs to advance time deliberately instead of
            // guessing it. A guessed duration is a check whose verdict depends
            // on machine speed, and R1787's CI run failed exactly that way.
            "saying" => Ok(IntrospectValue::Json(state.toast.to_wire())),
            // ★ R1898 — the questions about what a POINTER is doing are
            // answered one function over, for the reason R1893 split the
            // arrangement ones out: this `match` had reached the length this
            // workspace lints for, and the honest split is by SUBJECT rather
            // than by whichever arm happened to arrive last. See
            // `query_gesture`.
            "cursor" | "selected" | "hit" | "drag" | "carrying" | "float_grab" | "crossing" => {
                query_gesture(state, path)
            }
            "keymap" => text(
                KEYMAP
                    .iter()
                    .map(|(chord, what)| format!("{chord}={what}"))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        let state = self
            .state
            .as_ref()
            .ok_or(InterveneError::UnknownPath)?
            .clone();
        let word = |value: &IntrospectValue| match value {
            IntrospectValue::Text(s) => Ok(s.trim().to_string()),
            _ => Err(InterveneError::TypeMismatch),
        };
        if let Some(done) = Self::write_bar(&state, path, &value) {
            return done;
        }
        match path {
            "editing" => {
                let IntrospectValue::Bool(on) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.editing.set(on);
                state.say(Utterance::done(if on {
                    "layout edit mode"
                } else {
                    "layout locked"
                }));
                Ok(())
            }
            "preset_open" => {
                let IntrospectValue::Bool(on) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.preset_open.set(on);
                Ok(())
            }
            "search" => {
                let IntrospectValue::Text(needle) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.search.set(needle.clone());
                state.say(Utterance::done(format!("search {needle:?}")));
                Ok(())
            }
            "tab" => {
                let name = word(&value)?;
                let all = tabs();
                let chosen = all.iter().find(|t| **t == name).ok_or_else(|| {
                    InterveneError::out_of_range(format!(
                        "{name:?} is not a tab; they are {}",
                        all.join(", ")
                    ))
                })?;
                state.tab.set((*chosen).to_string());
                state.say(Utterance::done(format!("view {chosen}")));
                Ok(())
            }
            // ★ R1695 — the wire drives the SAME verb the pointer does, so the
            // two channels cannot answer a destination differently. What the
            // wire adds is the closed set in its refusal, which a person gets
            // from the rail itself.
            "nav" => {
                let name = word(&value)?;
                state.go(&name).map_err(|detour| match detour {
                    Detour::NoSuchDestination { .. } => InterveneError::out_of_range(format!(
                        "{name:?} is not a rail section; they are {}",
                        state.roster().keys().collect::<Vec<_>>().join(", ")
                    )),
                    Detour::Closed { .. } => InterveneError::out_of_range(format!(
                        "the {name:?} section is {}",
                        state
                            .roster()
                            .get(&name)
                            .and_then(|d| d.standing.why())
                            .map_or_else(
                                String::new,
                                pinion_core::availability::Unavailable::sentence
                            )
                    )),
                })
            }
            "preset" => ShellOracle::apply_preset(&state, &word(&value)?),
            "sources" | "cards" | "card_count" | "placed_count" | "layout" | "maximized"
            | "restore_to" | "floating" | "floats" | "detached" | "detach_policy" | "arrival"
            | "float_grab" | "crossing" | "presets" | "arrangements" | "transport" | "playhead"
            | "affordances" | "states" | "remedies" | "steppers" | "toast" | "cursor"
            | "selected" | "hit" | "keymap" | "rail" | "tabs" | "catalogue" | "config_open"
            | "drag" | "carrying" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    /// ★★★★★ R1720 — the refusal an agent was handed, put in front of the
    /// person watching this screen.
    ///
    /// Measured before this round: **0 of this screen's 20 refusing verbs**
    /// reached the toast. Every place this screen announced a refusal was on a
    /// PRESS path — the palette, an affordance, a stepper, a float's redock —
    /// so the refusals a person saw were exactly the ones they had caused
    /// themselves, and an agent driving the same board was silent.
    fn announce(&mut self, refused: &Utterance) -> Announced {
        let Some(state) = self.state.as_ref() else {
            return Announced::nowhere("no capture is loaded, so there is no board to say it on");
        };
        state.say(refused.clone());
        Announced::at("shell.toast")
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.state()?.clone();
        if let Some(answer) = Self::card_read(&state, path, &args) {
            return answer;
        }
        match path {
            "act" => Self::act(&state, &args),
            "sort_alarms" => Self::sort_alarms(&state, Self::text(&args)?.trim()),
            "filter_alarms" => Self::filter_alarms(&state, Self::text(&args)?.trim()),
            "set_state" => Self::set_state(&state, &args),
            "add" => Self::add(&state, &Self::text(&args)?),
            "maximize" => Self::maximize(&state, Self::text(&args)?.trim()),
            "restore" => Self::restore(&state),
            "redock" => Self::redock(&state, Self::text(&args)?.trim()),
            // ★★★★★ R1826 — the specification's "always-on-top option", as a
            // verb rather than a setting nobody can reach. Refuses for a card
            // that is not detached, because a card on the board has no window
            // to level and answering `ok` would be a claim about a window that
            // does not exist.
            "on_top" => Self::set_on_top(&state, Self::text(&args)?.trim()),
            // ★★★★★ R1891 — where a detached card lives, as a verb. The homes
            // it may take are the host's, so a build with no window server
            // refuses `window` here with a sentence naming the canvas.
            "detach_home" => Self::set_detach_home(&state, &args),
            // ★★★★★ R1900 — the three strip verbs. §2 #2 makes the agent the
            // primary path, so a gesture a hand can perform and a client cannot
            // is half a capability — and these go through the SAME board calls
            // the pointer reaches, so the two cannot come to mean different
            // things.
            // ★★★★★ R1903 — the canon's `togglePalette` / `openPalette`, on the
            // wire. §2 #2 makes the agent the primary path, so a panel a hand
            // can put away and a client cannot is half a capability.
            "palette" => Self::place_palette(&state, &Self::text(&args)?)
                .map(|at| IntrospectValue::Text(if at.folded { "folded" } else { "open" }.into())),
            "share" => Self::share_place(&state, &args),
            "unshare" => Self::unshare_place(&state, &args),
            "reveal" => Self::reveal_tab(&state, Self::text(&args)?.trim()),
            "resize" => {
                let raw = Self::text(&args)?;
                let (id, verb) = raw.split_once(',').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <card>,<step>"))
                })?;
                Self::step(&state, id.trim(), verb.trim())
            }
            "save_preset" => Self::save_preset(&state, Self::text(&args)?.trim()),
            // ★ R1893 — the canon's `deleteCustomPreset`. Refuses a built-in
            // with a sentence naming what to do instead.
            "delete_preset" => Self::delete_preset(&state, Self::text(&args)?.trim()),
            "seek" => {
                let raw = Self::text(&args)?;
                let per_mille: i32 = raw
                    .trim()
                    .parse()
                    .map_err(|_| InvokeError::rejected(format!("{raw:?} is not 0..=1000")))?;
                if !(0..=1000).contains(&per_mille) {
                    return Err(InvokeError::rejected(format!(
                        "a playhead is 0..=1000 per mille, got {per_mille}"
                    )));
                }
                state.clock.pause();
                state
                    .clock
                    .seek(f32::from(i16::try_from(per_mille).unwrap_or(0)) / 1000.0);
                state.capturing.set(false);
                state.say(Utterance::done(format!("seek {per_mille}")));
                Ok(IntrospectValue::Int(i64::from(per_mille)))
            }
            "point" => {
                let raw = Self::text(&args)?;
                let (x, y) = raw
                    .split_once(',')
                    .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <x>,<y>")))?;
                let parse = |what: &str, s: &str| -> Result<u32, InvokeError> {
                    s.trim()
                        .parse::<u32>()
                        .map_err(|_| InvokeError::rejected(format!("{what} is a pixel, got {s:?}")))
                };
                let (x, y) = (parse("x", x)?, parse("y", y)?);
                // ★ R1671 — from the STATE, not through the Owner-scoped read:
                // this path has no scope, so `window_size()` here answered with
                // the design fallback and refused presses the paint had put on
                // screen.
                let (w, h) = window_size();
                if x >= w || y >= h {
                    return Err(InvokeError::rejected(format!(
                        "({x},{y}) is outside the {w}x{h} shell"
                    )));
                }
                Self::move_cursor(&state, x, y);
                Ok(IntrospectValue::Text(hit_word(&Hit::at(&state, x, y))))
            }
            "send" => {
                let event = Self::text(&args)?;
                match event.trim() {
                    "PointerDown" => Self::press(&state),
                    "PointerUp" => Self::release(&state),
                    // ★★★★★ R1701 — the desktop convention a person reported
                    // missing: two clicks on a window's title bar toggle it
                    // between its size and its maximum. The router has
                    // synthesised this event since R664 and this screen refused
                    // it as "not a pointer event", so a card header could be
                    // double-clicked all day and nothing happened.
                    "DoubleClick" => Self::double_click(&state),
                    // A cancel drops the latch WITHOUT performing it — the
                    // difference between letting go and being interrupted.
                    "PointerLeave" | "PointerCancel" => {
                        state.pressed.borrow_mut().take();
                        state.drag.set(None);
                        // ★ R1916 — and the pointer is GONE, which is what
                        // takes a resting description off the frame. A leave is
                        // not a move to somewhere else.
                        state.pointer_inside.set(false);
                    }
                    other => {
                        return Err(InvokeError::rejected(format!(
                            "{other:?} is not a pointer event; they are PointerDown / \
                             PointerUp / PointerLeave / PointerCancel"
                        )));
                    }
                }
                Ok(IntrospectValue::Text(state.toast.sentence()))
            }
            "key" => {
                let chord = Self::text(&args)?;
                Ok(IntrospectValue::Bool(Self::key(&state, chord.trim())))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// --- Direct manipulation -----------------------------------------------------

impl ShellOracle {
    /// Move the cursor, and update the snap preview if a drag is in flight.
    /// ★★★★★ R1735 — where the cursor is, out of a live drag update, in this
    /// screen's own frame.
    ///
    /// The drop point's `x_rel` / `y_rel` are the cursor normalised over the tag
    /// it is on, which is exactly the fraction
    /// [`pointer_move`](External::pointer_move) is handed — so passing it to the
    /// same [`layout_point`](pinion_core::external::layout_point) makes this the
    /// SAME derivation rather than a second one that could disagree by a pan or
    /// a resize. The fallback is for a cursor that resolved onto some other
    /// surface, where the absolute window position is all there is; this screen
    /// fills its window, so it is a guard rather than a live path.
    fn drag_cursor(update: &DragUpdate) -> (u32, u32) {
        match update.over.as_ref() {
            Some(point) if pinion_core::composite_tag::split_subindex(&point.tag).0 == VIEW_TAG => {
                pinion_core::external::layout_point(VIEW_TAG, (point.x_rel, point.y_rel))
            }
            _ => {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a window-logical cursor is a pixel inside the window"
                )]
                let at = (
                    update.cursor.0.max(0.0) as u32,
                    update.cursor.1.max(0.0) as u32,
                );
                pinion_core::external::into_layout(VIEW_TAG, at)
            }
        }
    }

    fn move_cursor(state: &Rc<ShellState>, px: u32, py: u32) {
        state.cursor.set((px, py));
        // ★ R1916 — a move is what says the pointer is here again after a leave.
        state.pointer_inside.set(true);
        Self::carry_crossing(state, px, py);
        if let Some(grab) = state.float_grab.get() {
            Self::carry_float_grab(state, &grab, px, py);
            return;
        }
        // ★★★★★ R1735 — through the ONE preview body. The router's
        // `drop_offered` runs the same call, so a carry driven by the router and
        // a carry driven by this screen's own cursor cannot land in different
        // cells — which is the class this screen has now paid for twice.
        let _ = Self::preview_carry(state, px, py);
    }

    /// ★★★★★ R1735 — **preview the live carry at a window point, and say what a
    /// release there would do.**
    ///
    /// The single body behind two callers: this screen's own cursor path (a card
    /// gripped on the board, whose gesture never leaves the surface) and the
    /// router's [`External::drop_offered`] (a palette carry, whose gesture is a
    /// real drag session). Written once because it is one question — R1668's
    /// finding, which R1733 answered by giving the carry one landing and this
    /// round answers again one layer up, where a second body had just appeared.
    ///
    /// `Ok` is the cell a release would use. `Err` is why it would not place
    /// anything, in words a person reads — the reason the router forwards to the
    /// source as [`DropStanding::Refused`] and the wire publishes at
    /// `drop_standing`.
    ///
    /// ★ The `leave` half is the one a card drag never needed: a footprint
    /// carried out over the palette has no landing, so releasing there is an
    /// abandon rather than a placement at whatever cell the clamp last produced.
    fn preview_carry(state: &Rc<ShellState>, px: u32, py: u32) -> Result<(u32, u32), &'static str> {
        let Some(mut drag) = state.drag.get() else {
            return Err("nothing is being carried");
        };
        let before = drag.landing();
        // The grip offset and the column clamp both live in the framework type,
        // so this file does no arithmetic that could differ from what the
        // release does.
        // ★★★★★ R1898 — through `rest_at`, the ONE classifier, so a carry that
        // is a detached panel on its way back to the board is judged by the
        // same rule as a footprint off the palette. Before this the question
        // was spelled twice — here and at the edge — and the two disagreed
        // about a panel the cursor was inside of.
        let carried = Self::carried_float(state);
        // ★★★★★ R1898 — a gesture that has not become a drag previews nothing.
        //
        // The crossing owns the click-vs-drag latch, and a preview drawn for a
        // press that is still a click would be a rectangle the release does not
        // honour — the lie this round's whole module exists to make unspellable.
        // A palette carry has no crossing and is unaffected: the router opens
        // its drag session, which has already made that determination.
        if state
            .crossing
            .get()
            .is_some_and(|crossing| !crossing.is_drag())
        {
            drag.leave();
            if drag.landing() != before {
                state.drag.set(Some(drag));
            }
            return Err("the press has not become a drag yet");
        }
        let outcome = match rest_at(state, px, py, carried.as_deref()) {
            Rest::Inside { col, row } => {
                drag.hover(&state.board.get(), col, row);
                drag.landing()
                    .ok_or("this footprint does not fit at that cell")
            }
            Rest::Outside { .. } => {
                drag.leave();
                Err(if state.at() == "dashboard" {
                    "the board is the canvas, and the cursor is not over it"
                } else {
                    "a widget lands on the dashboard's board, and that is not the page showing"
                })
            }
        };
        if drag.landing() != before {
            state.drag.set(Some(drag));
        }
        outcome
    }

    /// R1698 — what a composite's cursor landing somewhere does besides move.
    ///
    /// Every stop here declares
    /// [`Activation::Explicit`],
    /// so arriving never chooses — and the `choose` bit is read rather than
    /// ignored, because a declaration nothing reads is the "declared and thrown
    /// away" arm R1684 named. If a stop is ever declared `Follows`, this is
    /// where that becomes an action rather than a comment.
    ///
    /// The toast is what makes the cursor perceivable at all to somebody
    /// driving the wire, and the ring is what makes it perceivable on screen:
    /// the framework frames the active descendant, which this screen publishes
    /// through `access_focus_target`.
    fn landed(state: &Rc<ShellState>, stop: &str, landing: Landing) {
        let Some(tag) = state
            .cursor_of(stop)
            .and_then(|r| r.active_descendant().map(str::to_owned))
        else {
            return;
        };
        let what = match landing {
            // ★ Every stop on THIS screen declares `Explicit`, and a unit test
            // asserts that, so a `choose: true` reaching here would mean the
            // specification changed without this arm being written. The other
            // arm's real consumer is the capture viewer's message list, where
            // the cursor IS the selection.
            Landing::Moved { choose: true, .. } => "cursor moved and chose",
            Landing::Moved { .. } => "cursor moved",
            Landing::Held(_) => "cursor held at the end",
            Landing::Nowhere => return,
            // ★★★★★ R1699 — the half `Explicit` had always promised and never
            // delivered. Measured before this arm existed, by driving the
            // running screen: `Enter` and `Space` at all five stops, twelve
            // presses, nothing painted changed.
            //
            // Both arms reach `act_on_hit`, which is the ONE place that knows
            // what a tag does and why it might refuse — a second refusal
            // sentence written here would be the two-spellings defect this
            // screen has already paid for twice (R1695's rail, R1668's seats).
            // They stay distinct on the wire, where `enabled` tells a client
            // that choosing this member refuses *before* it presses anything.
            Landing::Chosen(_) | Landing::Refused(_) => {
                Self::act_on_hit(state, Hit::of_tag(state, &tag));
                return;
            }
            Landing::Entered(_) => "entered",
            Landing::Exited(_) => "left",
        };
        state.say(Utterance::done(format!("{tag} \u{00B7} {what}")));
    }

    /// R1697 — bring a detached panel forward and start moving or sizing it.
    ///
    /// The raise comes first because the reference's `startFloatDrag` calls
    /// `raiseFloat` before it reads the panel's origin, and the order is load
    /// bearing: grabbing a panel that is behind another must bring it out from
    /// under before the pointer starts moving it, or the panel being dragged is
    /// the one the person cannot see.
    fn open_float_grab(state: &Rc<ShellState>, id: &str, edge: bool, at: (u32, u32)) {
        state.raise_float(id);
        let Some(float) = state.float(id) else {
            return;
        };
        state.float_grab.set(Some(FloatGrab {
            id: id.to_string(),
            edge,
            from: at,
            origin: if edge {
                (float.w, float.h)
            } else {
                (float.x, float.y)
            },
        }));
    }

    /// R1697 — carry a float grab to the cursor.
    ///
    /// Live rather than previewed, which is the opposite of what a card drag
    /// does on this same screen and is correct for both: a card lands in a cell
    /// and the board would reflow under the finger, and a panel lands where it
    /// is put. The reference draws the same line.
    fn carry_float_grab(state: &Rc<ShellState>, grab: &FloatGrab, px: u32, py: u32) {
        let Some(float) = state.float(&grab.id) else {
            return;
        };
        // Signed, because a panel can be dragged left and up as readily as
        // right and down, and unsigned arithmetic would clamp a leftward drag
        // to zero movement rather than to the window edge.
        let dx = i64::from(px) - i64::from(grab.from.0);
        let dy = i64::from(py) - i64::from(grab.from.1);
        let shift = |origin: u32, delta: i64, floor: u32| -> u32 {
            #[allow(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "clamped into u32's range on the line above the cast"
            )]
            let moved =
                (i64::from(origin) + delta).clamp(i64::from(floor), i64::from(u32::MAX)) as u32;
            moved
        };
        let next = if grab.edge {
            Float {
                w: shift(grab.origin.0, dx, FLOAT_MIN_W),
                h: shift(grab.origin.1, dy, FLOAT_MIN_H),
                ..float
            }
        } else {
            Float {
                x: shift(grab.origin.0, dx, 0),
                y: shift(grab.origin.1, dy, 0),
                ..float
            }
        };
        state.set_float(&grab.id, &next);
    }

    /// A press latches what is under the cursor; a press on a card header opens
    /// a drag that **previews** rather than moving, because the reference
    /// commits on release and a board reflowing under the finger would make the
    /// preview a lie.
    fn press(state: &Rc<ShellState>) {
        let (px, py) = state.cursor.get();
        let hit = Hit::at(state, px, py);
        if let Some(id) = hit.card_id() {
            state.selected.set(Some(id.to_string()));
        }
        // ★★★★★ R1900 — **a press on a tab brings that occupant to the front,
        // and the drag it opens takes it back out of the shared cell.**
        //
        // Both on the press, and both the floor's own behaviour: pressing a tab
        // there activates it immediately rather than on release, and dragging
        // one out of a tabbed dock is how a panel leaves it. The reveal happens
        // first so the drag carries what the person is now looking at — a drag
        // that carried the *previous* front would be the screen acting on a
        // card the press just replaced.
        if let Hit::Tab(member) = &hit {
            let id = TileId::new(member.clone());
            let mut board = state.board.get();
            match board.reveal(&id) {
                Ok(_) => {
                    state.board.set(board.clone());
                    state.tab_carry.set(Some(member.clone()));
                    let (col, row) = cell_at_window(state, px, py);
                    if let Ok(drag) = TileDrag::grip(&board, &id, col, row) {
                        state.drag.set(Some(drag));
                    }
                    // R1898's rule, kept: a gesture that cannot cross the
                    // board's edge SAYS so, so "it did not leave the board" is
                    // a declaration rather than an accident of what is painted
                    // under the cursor.
                    state.crossing.set(Some(Crossing::open(
                        label_of(member),
                        CrossingPolicy::stays(
                            "a card sharing a cell leaves the board from a cell of its own; \
                             drag it out onto the board first",
                        ),
                        Side::Inside,
                        press_point(px, py),
                        Rest::cell(col, row),
                    )));
                }
                Err(why) => state.say(Utterance::refused(&InvokeError::rejected(why.to_string()))),
            }
        }
        if let Hit::Grip(id) = &hit {
            let board = state.board.get();
            let (col, row) = cell_at_window(state, px, py);
            // ★ R1900 — a grip drags the PLACE, so a gesture that began on a
            // tab is over: clearing it here rather than only at release is what
            // keeps `pulling_a_tab` answering about the drag in flight.
            state.tab_carry.set(None);
            if let Ok(drag) = TileDrag::grip(&board, &TileId::new(id.clone()), col, row) {
                state.drag.set(Some(drag));
                // ★★★★★ R1898 — **and the edge, so carrying it off the board
                // takes it off the board.** The floor's own gesture: its
                // detachable panel is dragged out by the strip this grip is.
                //
                // A maximised card declares the other arm, and that is a
                // correctness requirement rather than a preference. Maximising
                // stores the arrangement to restore into
                // ([`Maximized`]), and that stored board still holds this
                // card — so letting the gesture take it out would leave the
                // restore pointing at a card that is no longer there, which is
                // the two-pictures defect R1891 closed one axis over.
                state.crossing.set(Some(Crossing::open(
                    label_of(id),
                    if state.maximized.get().is_some() {
                        CrossingPolicy::stays(
                            "a maximised card is in the arrangement waiting to be restored, so \
                             restore it before taking it off the board",
                        )
                    } else {
                        CrossingPolicy::Crosses
                    },
                    Side::Inside,
                    press_point(px, py),
                    // Where the card ALREADY is: the cell it occupies. A press
                    // moves nothing, so the opening rest has to say so.
                    Rest::cell(col, row),
                )));
            }
        }
        // ★★★★★ R1898 — **a press on a detached panel's re-dock mark picks the
        // panel up for the board.**
        //
        // The palette's shape, one gesture over (R1733): the ACTION is not
        // replaced — a press and release on the mark still re-docks at the
        // bottom of the board, which is what the behaviour canon does — and a
        // drag off the mark onto a cell docks it THERE. One control, two
        // gestures, the same verb underneath.
        //
        // Why this mark and not the panel's body: dragging the body moves the
        // panel, which the canon has and this build must not take away. The
        // standing rule that a floor is a floor rather than a ceiling cuts both
        // ways — it says build what the floor has and does not say delete what
        // it lacks. The floor resolves this same collision with a held modifier
        // key whose flag is private to its drag state; a second affordance is
        // reachable without knowing a keystroke, and it says what it does.
        if let Hit::FloatRedock(id) = &hit {
            if let Some((cols, rows)) = kind_span(kind_of(id)) {
                if let Ok(drag) = TileDrag::pick(&state.board.get(), id.clone(), cols, rows) {
                    state.drag.set(Some(drag));
                }
            }
            state.crossing.set(Some(Crossing::open(
                label_of(id),
                CrossingPolicy::Crosses,
                Side::Outside,
                press_point(px, py),
                // ★★★★★ Where the PANEL already is, not what is under the
                // pointer. The mark is painted over the board, so reading the
                // pointer's side here made a plain press on it a placement —
                // measured on the running application, which docked the panel
                // at column 6 and displaced five cards.
                Rest::point(px, py),
            )));
        }
        // ★★★★★ R1733 — **a press on a palette row picks the widget up.**
        //
        // The reference's palette tile is draggable and its board takes the
        // drop; this row was reachable only as an action, which is the last
        // first-pass gap its GUI census had open.
        //
        // The action is NOT replaced. The press latches the hit as it always
        // has, so a press and release on the same row still adds at the bottom
        // — see `release`, where an abandoned carry falls through to the latch.
        // That matters more here than fidelity does: the reference has no
        // keyboard bindings at all, so moving its pointer-only gesture over
        // *instead* of the action would take the palette away from a reader who
        // cannot drag.
        if let Hit::Palette(kind) = &hit {
            match Self::pick_up(state, kind) {
                Ok(drag) => state.drag.set(Some(drag)),
                // A reserved row, or a kind with no declared footprint. The
                // press still latches, so the release says the same thing the
                // action would — one refusal, not two spellings of it.
                Err(_) => state.drag.set(None),
            }
        }
        // ★★★★★ R1697 — **a press on a detached panel grabs it.**
        //
        // This is the arm a person reported: it read
        // `Hit::Float(_) | Hit::Nothing => {}` on the release path and nothing
        // opened a gesture on the press path, so a torn-off panel was nailed
        // where it landed. Every gate on this screen was green and each was
        // correct — the panel is painted, hit-testable, named and announced.
        // None of them asks whether grabbing it moves it.
        if let Hit::Float(id) | Hit::FloatResize(id) = &hit {
            let sizing = matches!(hit, Hit::FloatResize(_));
            Self::open_float_grab(state, id, sizing, (px, py));
            // ★★★★★ R1898 — **these two gestures declare that they do not
            // dock**, and the declaration is what makes that a property.
            //
            // A panel lives over the board, so both of these spend their whole
            // life with the pointer inside the board's rectangle. Leaving them
            // undeclared would make "it does not dock" true only because the
            // panel itself is painted under the cursor — a coincidence of
            // stacking, not a rule — and `on_board_ignoring` deliberately looks
            // THROUGH the carried panel so that coincidence cannot be what
            // answers. What answers instead is this sentence, and it names the
            // gesture that does dock rather than only saying no.
            state.crossing.set(Some(Crossing::open(
                label_of(id),
                CrossingPolicy::stays(if sizing {
                    "this drag sizes the panel; drag its re-dock mark onto a cell to put it back \
                     on the board"
                } else {
                    "this drag moves the panel; drag its re-dock mark onto a cell to put it back \
                     on the board"
                }),
                Side::Outside,
                press_point(px, py),
                Rest::point(px, py),
            )));
        }
        *state.pressed.borrow_mut() = Some(hit);
    }

    /// ★★★★★ R1701 — two clicks on a card's header toggle it between its size
    /// on the board and the whole board.
    ///
    /// Reported by a person: "shouldn't double-clicking a window toggle
    /// maximise?" — and the behaviour reference cannot settle it, because it is
    /// a browser prototype with no window chrome at all (measured: zero
    /// double-click handlers in its 194,828 bytes of application script). So the
    /// FLOOR settles it, which the standing directive says it may: built and run
    /// offscreen at 6.11, an in-application sub-window's title-bar double-click
    /// takes it from 300x200 to its parent's full 900x600, and a docking panel's
    /// takes it from docked to floating. A card on this board is the first of
    /// those two shapes.
    ///
    /// It goes through [`Self::act`] — the SAME entry the header button's press
    /// takes, refusal and all — so the two gestures cannot come to mean
    /// different things, and a card whose header does not OFFER maximise
    /// refuses the double-click by name rather than silently ignoring it. That
    /// is R1697's rule, which made that button a toggle in the first place.
    ///
    /// A double-click anywhere that is not a header does nothing, which is also
    /// the floor's answer: a sub-window's body double-click is the content's
    /// business.
    fn double_click(state: &Rc<ShellState>) {
        let (px, py) = state.cursor.get();
        let Hit::Grip(id) = Hit::at(state, px, py) else {
            return;
        };
        // ★★★★★ The second click's gesture is SUPERSEDED, not carried out
        // alongside. A grip press opens a board drag (R1697), so without this a
        // double-click maximised the card and then let the trailing release
        // commit a move aimed at the board that existed before it grew.
        // Measured, before this line: double-clicking a header reported "Decode
        // Inspector moved, displacing Message Stream, Identifier Map, Search &
        // Filter" and the board never came back to the arrangement it opened
        // with. The debt this round repays named that risk before it was built
        // — a move gesture and a double-click share one place, so one of them
        // has to yield — and driving it is what showed which.
        state.drag.set(None);
        state.pressed.borrow_mut().take();
        let call = IntrospectValue::Text(format!("{id},{}", CardAffordance::Maximize.wire()));
        if let Err(why) = Self::act(state, &call) {
            state.say(Utterance::refused(&why));
        }
    }

    /// A release performs the latched control if the cursor is still on it, and
    /// commits a drag wherever the preview ended up.
    fn release(state: &Rc<ShellState>) {
        let latched = state.pressed.borrow_mut().take();
        // ★ R1697 — a float grab has already done its work by the time the
        // button comes up (it moves live), so releasing only ends it. Returning
        // here is what stops the release ALSO acting on the latched hit: a drag
        // that finished over the panel's body must not read as a press on the
        // body.
        if let Some(grab) = state.float_grab.get() {
            state.float_grab.set(None);
            // ★★★★★ R1898 — this release honours whatever the crossing says,
            // and what these two gestures say is `Stays`.
            //
            // The arm is written even though the declaration beside them means
            // it never runs, and that is the point: a declaration nothing would
            // act on is a label, and a label cannot be broken by a test. With
            // the arm here, changing either gesture's policy to `Crosses` docks
            // the panel mid-move — which is a defect a walk can see, and does
            // (`r1898`, section C). The sentence explaining the refusal stays
            // readable on the `crossing` slot right up to the release rather
            // than arriving as a toast for something nobody asked for.
            let passage = state.crossing.get().map(|crossing| crossing.passage());
            state.crossing.set(None);
            if let Some(Ok(Passage::Joined { col, row })) = passage {
                Self::dock_where(state, &grab.id, col, row);
                return;
            }
            // Only speak if something actually changed. A press and release
            // without movement is a click on the panel, and announcing "moved"
            // for it would be the same class of lie the rail told before R1695:
            // a message describing an arrival that did not happen.
            let now = state
                .float(&grab.id)
                .map(|f| if grab.edge { (f.w, f.h) } else { (f.x, f.y) });
            if now.is_some_and(|now| now != grab.origin) {
                state.say(Utterance::done(format!(
                    "{} {}",
                    label_of(&grab.id),
                    if grab.edge { "resized" } else { "moved" }
                )));
            }
            return;
        }
        if let Some(drag) = state.drag.get() {
            state.drag.set(None);
            // ★★★★★ R1898 — **what side of the board's edge this release is
            // on, asked once, before the board's own commit.**
            //
            // The `match` is total over `Passage`, so a gesture that crossed
            // cannot fall through to the move path by omission — which is
            // exactly how the two crossings were missing before this round:
            // there was no value to match on, so "off the board" reached
            // `Dropped::Abandoned` and meant nothing.
            let passage = state.crossing.get().map(|crossing| crossing.passage());
            state.crossing.set(None);
            // ★★★★★ R1733 — an ABANDONED carry falls through to the latch.
            //
            // That is what keeps the palette's action alive now that pressing a
            // row also picks the widget up: press and release on the same row
            // carries it nowhere, so the latched hit acts and the card is added
            // at the bottom exactly as before. Fidelity to a pointer-only
            // reference must not cost a reader the only path they have.
            //
            // ★★★★★ R1898 — and it is what keeps the re-dock mark's ACTION
            // alive for the same reason, one gesture over. All three answers
            // below are "may the release go on to perform what the press
            // latched", so the rule is one rule with three bodies rather than
            // three conventions.
            let go_on = match passage {
                Some(Ok(Passage::Left { x, y })) => {
                    Self::leave_board(state, drag.carried().id().as_str(), (x, y));
                    false
                }
                Some(Ok(Passage::Joined { .. })) => Self::join_board(state, drag),
                // A move within the board, a drift outside it, a refused
                // crossing, or a gesture that declared no crossing at all: the
                // board's own commit answers, exactly as it did before.
                Some(Ok(Passage::Moved { .. } | Passage::Drifted { .. }) | Err(_)) | None => {
                    Self::commit_drag(state, drag)
                }
            };
            if !go_on {
                return;
            }
        }
        let (px, py) = state.cursor.get();
        let Some(latched) = latched else { return };
        if Hit::at(state, px, py) != latched {
            return;
        }
        Self::act_on_hit(state, latched);
    }

    /// ★★★★★ R1898 — which detached panel a gesture is carrying, if any.
    ///
    /// Derived from the two things already in flight rather than stored a third
    /// time: a panel being moved or sized is the [`FloatGrab`]'s, and a panel
    /// being carried back to the board is the fresh [`TileDrag`]'s — *if that
    /// carry names a card that is detached*, which is what separates it from a
    /// footprint carried off the palette. A field would be a third fact three
    /// places would have to keep in step.
    fn carried_float(state: &ShellState) -> Option<String> {
        if let Some(grab) = state.float_grab.get() {
            return Some(grab.id);
        }
        let drag = state.drag.get()?;
        let id = drag.carried().id().as_str().to_owned();
        (!drag.carried().is_placed() && state.is_floating(&id)).then_some(id)
    }

    /// ★★★★★ R1898 — carry the crossing to the cursor.
    ///
    /// Runs for EVERY gesture that declared one, including the two that do not
    /// cross: `crossing` is what a client reads to know what letting go would
    /// do, and a slot that stopped updating while a panel was being moved would
    /// answer for a pointer position that is minutes old.
    fn carry_crossing(state: &Rc<ShellState>, px: u32, py: u32) {
        let Some(mut crossing) = state.crossing.get() else {
            return;
        };
        let carried = Self::carried_float(state);
        let rest = rest_at(state, px, py, carried.as_deref());
        let before = (crossing.rest(), crossing.is_drag());
        crossing.hover(press_point(px, py), rest);
        if (crossing.rest(), crossing.is_drag()) != before {
            state.crossing.set(Some(crossing));
        }
    }

    /// ★★★★★ R1898 — **the card leaves the board, at the point it was let go
    /// of**, and takes its rectangle with it.
    ///
    /// A card that changes which side of the edge it lives on has to arrive
    /// somewhere, and the two sides do not share a coordinate system. The
    /// transfer is stated here because it is the host that owns both spaces —
    /// the cell is the board's, the point is the window's, and the panel's
    /// rectangle is the canvas's.
    ///
    /// ⚠ This is NOT the transfer R1891 left open, and the two are easy to run
    /// together. That one is between the two DETACHED homes — a window's
    /// rectangle is in the display's space and a canvas float's is in the
    /// host's, so `detach_home` still moves a panel between them without its
    /// geometry following. This one is between docked and loose. Both are
    /// coordinate-system crossings; only this one is closed.
    ///
    /// # Why the canvas and not a window
    ///
    /// The tear-off *control* takes [`DetachPolicy::preferred`], which on this
    /// host is a real window (R1826). A DRAG ends at a point on this canvas, and
    /// a panel whose position is the point the pointer let go of is a panel on
    /// the canvas — handing that point to a window server would be handing it a
    /// coordinate in a space it does not use. So the gesture asks for
    /// [`DetachHome::Canvas`] by name, through the policy, and a host that
    /// could not provide it refuses with a sentence rather than silently
    /// putting the panel somewhere else.
    ///
    /// # The size it arrives at
    ///
    /// The size it had on the board, not the opening float size: a panel that
    /// jumped to another size under the finger would be a different object from
    /// the one that was picked up. Clamped up to the panel minimums, which is
    /// what a card narrower than 320 logical pixels needs to keep its own
    /// header controls beside its title (R1697's measurement).
    fn leave_board(state: &Rc<ShellState>, id: &str, at: (u32, u32)) {
        let canvas = canvas_rect();
        let Some(tile) = state.board.get().tile(&TileId::new(id)).cloned() else {
            return;
        };
        let was = cell_rect(&tile);
        let (width, height) = (was.w.max(FLOAT_MIN_W), was.h.max(FLOAT_MIN_H));
        // Into the canvas's own space, and kept inside it: a panel dropped over
        // the palette would otherwise be painted outside the region that draws
        // it, and a panel nobody can see is a panel nobody can put back.
        let local = |point: u32, origin: u32, extent: u32, span: u32| {
            point
                .saturating_sub(origin)
                .min(extent.saturating_sub(span))
        };
        let home = match detach_policy().admit(DetachHome::Canvas) {
            Ok(home) => home,
            Err(refusal) => {
                state.say(Utterance::refused(&refusal.reason().to_string()));
                return;
            }
        };
        let mut board = state.board.get();
        board.remove(&TileId::new(id)).ok();
        state.board.set(board);
        let stacking = {
            let mut counter = state.float_z.borrow_mut();
            *counter += 1;
            *counter
        };
        let mut floats = state.floats.get();
        floats.push(Float {
            id: id.to_owned(),
            x: local(at.0, canvas.x, canvas.w, width),
            y: local(at.1, canvas.y, canvas.h, height),
            w: width,
            h: height,
            z: stacking,
            on_top: false,
            home,
        });
        state.floats.set(floats);
        state.say(Utterance::done(format!(
            "{} \u{2192} detached panel",
            label_of(id)
        )));
    }

    /// ★★★★★ R1898 — dock `id` at a cell a passage named, through the one
    /// commit body.
    ///
    /// For a gesture that carries no [`TileDrag`] of its own — a panel being
    /// moved, if its policy ever said it crosses. The drag is built here and
    /// hovered with the cell the crossing reported, so the grid's own footprint
    /// clamp is applied exactly once and the placement goes through
    /// [`join_board`](Self::join_board) like every other.
    fn dock_where(state: &Rc<ShellState>, id: &str, col: u32, row: u32) -> bool {
        let Some((w, h)) = kind_span(kind_of(id)) else {
            return true;
        };
        let board = state.board.get();
        let Ok(mut drag) = TileDrag::pick(&board, id.to_owned(), w, h) else {
            return true;
        };
        drag.hover(&board, col, row);
        Self::join_board(state, drag)
    }

    /// ★★★★★ R1898 — **the panel joins the board, in the cell the preview was
    /// drawing.**
    ///
    /// The other direction, and it commits through the same [`TileDrag`] the
    /// preview read — so the rectangle a person watched and the cell the card
    /// takes are one value, which is R1668's rule and the reason this gesture
    /// carries a tile drag at all rather than computing a cell here.
    ///
    /// The `redock` verb is NOT replaced: it puts a panel back at the bottom of
    /// the board, which is what the behaviour canon does and what a press on
    /// the same mark still does. This is the chosen-cell gesture beside it —
    /// the palette's shape (R1733), one control with two gestures.
    ///
    /// Answers whether the release may go on to perform what the press latched,
    /// exactly as [`commit_drag`](Self::commit_drag) does. ★ A press and a
    /// release on the mark without moving never aimed at a cell, so the carry
    /// has no landing and this docks nothing and answers `true` — which is what
    /// keeps that press the plain `redock` it has always been. The alternative
    /// — treating a press on a panel that happens to lie over the board as a
    /// placement — was the first draft, and this gate caught it: `floating` did
    /// not move, because a drop with no landing is `Abandoned`.
    fn join_board(state: &Rc<ShellState>, drag: TileDrag) -> bool {
        let id = drag.carried().id().as_str().to_owned();
        let label = label_of(&id);
        let mut board = state.board.get();
        match drag.drop_on(&mut board) {
            Ok(Dropped::Landed { at, reflow }) => {
                state.board.set(board);
                state.floats.set(
                    state
                        .floats
                        .get()
                        .into_iter()
                        .filter(|f| f.id != id)
                        .collect(),
                );
                state.say(Utterance::done(if reflow.is_clean() {
                    format!("{label} re-docked at column {}, row {}", at.0, at.1)
                } else {
                    format!(
                        "{label} re-docked at column {}, row {}, displacing {}",
                        at.0,
                        at.1,
                        reflow
                            .displaced()
                            .iter()
                            .map(|d| label_of(d.id.as_str()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }));
                false
            }
            // Nothing was aimed at: `Unmoved` cannot happen for a carry that is
            // not on the board, and `Abandoned` is the press that never moved.
            // The release goes on to the latch, where the mark's own action
            // puts the panel back at the bottom.
            Ok(_) => true,
            // The board would not take it there. The panel stays detached and
            // exactly where it is, and the sentence is the grid's own — a
            // second wording here would be a second rule. The latch is NOT
            // performed: a refused drop must not quietly become the default
            // placement the press would have done.
            Err(why) => {
                state.say(Utterance::refused(&why.to_string()));
                false
            }
        }
    }

    /// Put down what the board was carrying. Answers whether the release should
    /// go on to perform the latched control.
    ///
    /// ★★★★★ R1733 — the three outcomes are the framework's
    /// [`Dropped`] arms rather than a remembered comparison. R1701 measured
    /// what the comparison costs when somebody forgets it: a click that carried
    /// nothing reflowed the board and announced a move that had not happened.
    /// Here the middle case has a name and the `match` is what demands it.
    fn commit_drag(state: &Rc<ShellState>, drag: TileDrag) -> bool {
        if !drag.carried().is_placed() {
            // ★★★★★ R1735 — a FRESH carry does not reach here any more, and the
            // change is a behaviour the framework already owned.
            //
            // A palette press opens a ROUTER drag session
            // ([`External::begin_drag`]), so its release is committed by
            // `drop_commit` and cleared by `drag_release_at` before this runs.
            // What R1733 wrote here — an abandoned carry falling through to the
            // latch, so a drag that wandered off and came back still added a
            // card — was this screen re-deriving click-vs-drag, and it decided
            // the opposite of the framework's own rule (a real drag suppresses
            // the trailing click; R794). Measured on the floor at 6.11.1: a
            // source that ran a drag receives **zero** mouse releases for that
            // gesture, so a dragged row's click does not fire there either.
            //
            // Total rather than a panic: answering "the latch may act" is the
            // arm that cannot be wrong about a case this build has never seen,
            // and the claim that it is unreachable is a test rather than a
            // comment — `r1735_a_fresh_carry_is_not_the_shells_to_commit`.
            return true;
        }
        let mut board = state.board.get();
        let label = label_of(drag.carried().id().as_str());
        let carried = drag.carried().id().clone();
        let (px, py) = state.cursor.get();
        // ★★★★★ R1900 — the board's INNER boundary, decided before the cell is,
        // because joining a place and taking one are different acts on the same
        // release and only one of them is a landing.
        match berth_at(state, px, py, &carried) {
            Berth::With(host) => {
                return Self::let_go_onto(state, &mut board, &carried, &host, &label);
            }
            Berth::Own if pulling_a_tab(state, &carried) => {
                return Self::let_go_out_of(state, &mut board, &drag, &carried, &label);
            }
            Berth::Own => {}
        }
        match drag.drop_on(&mut board) {
            Ok(Dropped::Landed { reflow, .. }) => {
                state.board.set(board);
                state.say(Utterance::done(if reflow.is_clean() {
                    format!("{label} moved")
                } else {
                    format!(
                        "{label} moved, displacing {}",
                        reflow
                            .displaced()
                            .iter()
                            .map(|d| label_of(d.id.as_str()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }));
                false
            }
            // Everything that is NOT a landing leaves the board alone and says
            // nothing, and the two ways to get here are worth naming even
            // though they share an answer:
            //
            // * `Unmoved` — a press and release that carried the card nowhere.
            //   The latch is not performed either, because a header's latched
            //   hit is the card itself and acting on it would re-announce the
            //   selection the press already made.
            // * `Abandoned` — released off the board, so the card stays where
            //   it is. The reference has no answer here at all: its board drag
            //   listens on the whole document, so a release over its palette
            //   commits.
            //
            // `Dropped` is non-exhaustive, so a later arm lands here too —
            // leaving the board untouched is the answer that cannot be wrong
            // about a case this build has never seen.
            Ok(_) => false,
            Err(why) => {
                state.say(Utterance::refused(&InvokeError::rejected(why.to_string())));
                false
            }
        }
    }

    /// ★★★★★ R1900 — a card let go over another card's header joins its place.
    ///
    /// The whole outcome is announced, occupants and all, because the visible
    /// change is that one card *disappeared* — it is behind a tab now, and a
    /// sentence saying only "moved" would describe a board a person cannot see.
    fn let_go_onto(
        state: &Rc<ShellState>,
        board: &mut TileGrid,
        carried: &TileId,
        host: &str,
        label: &str,
    ) -> bool {
        match board.share(carried, &TileId::new(host.to_owned())) {
            Ok(shared) => {
                let names: Vec<String> = shared
                    .members
                    .iter()
                    .map(|m| label_of(m.as_str()))
                    .collect();
                state.board.set(board.clone());
                state.say(Utterance::done(format!(
                    "{label} shares a place with {}",
                    names.join(", ")
                )));
                false
            }
            Err(why) => {
                state.say(Utterance::refused(&InvokeError::rejected(why.to_string())));
                false
            }
        }
    }

    /// ★★★★★ R1900 — a tab dragged out of a shared place onto the board gets a
    /// cell of its own there.
    ///
    /// The inverse of [`Self::let_go_onto`], and the reason a strip is not a
    /// trap: R1802's standing lesson on this very campaign is that a state a
    /// person can be *put into* and cannot get *out of* by hand is a capability
    /// only on paper.
    fn let_go_out_of(
        state: &Rc<ShellState>,
        board: &mut TileGrid,
        drag: &TileDrag,
        carried: &TileId,
        label: &str,
    ) -> bool {
        let Some((col, row)) = drag.landing() else {
            // Released off the board. The place keeps its occupants, which is
            // what the crossing this gesture declared already says in words.
            return false;
        };
        match board.unshare(carried, col, row) {
            Ok(_) => {
                state.board.set(board.clone());
                state.say(Utterance::done(format!(
                    "{label} has a place of its own at ({col},{row})"
                )));
                false
            }
            Err(why) => {
                state.say(Utterance::refused(&InvokeError::rejected(why.to_string())));
                false
            }
        }
    }

    /// What a completed press on one hit target does.
    ///
    /// Lifted out of [`Self::release`] in R1668 so a test can put a hit to the
    /// shell directly. It was reachable only by placing a cursor and driving a
    /// press-release pair, which meant "does a reserved rail seat refuse" could
    /// be asked only through the geometry that already refuses to reach it --
    /// the shape R1649.1 named: a capability verified only by the path that
    /// bypasses it is a capability nobody verified.
    fn act_on_hit(state: &Rc<ShellState>, latched: Hit) {
        match latched {
            Hit::Chip(chip) => Self::press_chip(state, chip),
            Hit::Sub(chip) => Self::press_sub(state, chip),
            Hit::PresetItem(n) => Self::press_preset_item(state, n),
            // ★ R1695 — one verb, and its refusal is the roster's sentence.
            // Before this the press path authored its own message and told a
            // reserved seat the wrong thing on the sibling screen; a refusal
            // nobody derives is a refusal two channels will spell two ways.
            Hit::Rail(key) => {
                if let Err(detour) = state.go(key) {
                    // ★ R1719 — a detour is the rail declining to take you
                    // there, so it is a refusal and now reads as one.
                    state.say(Utterance::refused(&detour.sentence(state.roster())));
                }
            }
            Hit::Option(key) => Self::toggle_option(state, key),
            // ★★★★★ R1762 — the collapsed control TOGGLES its roster. Opening
            // it is not a write: the value stays where it is until a word is
            // chosen, which is the rule `Picker` is built on and the one the
            // floor's own collapsed control breaks (it commits on every arrow).
            Hit::Choose(key) => Self::toggle_roster(state, &key),
            Hit::ChooseOption(key, n) => Self::choose_value(state, &key, n),
            // Painted inert, so a pointer never reaches it; this is the
            // keyboard and wire path saying the same thing the seat declares.
            Hit::KeyRow(key) => {
                let row = spec::KEY_ROWS.iter().find(|row| row.key == key);
                if let Some(row) = row {
                    // ★ R1719 — "you cannot use this, and here is why" is a
                    // refusal; it was reaching a reader in the voice of an
                    // acknowledgement.
                    state.say(Utterance::new(
                        Tone::Refused,
                        format!(
                            "{} is {}",
                            row.title,
                            // R2042 — a row no requirement books says so, in
                            // place of a number that would book somebody else's
                            // capability.
                            Unavailable::reserved(row.reserved_for.unwrap_or(spec::UNBOOKED))
                                .sentence()
                        ),
                    ));
                }
            }
            Hit::Theme(n) => Self::choose_theme(state, n),
            Hit::Palette(kind) => {
                if let Err(why) = Self::add(state, kind) {
                    state.say(Utterance::refused(&why));
                }
            }
            Hit::Affordance(id, affordance) => {
                let call = IntrospectValue::Text(format!("{id},{}", affordance.wire()));
                if let Err(why) = Self::act(state, &call) {
                    // A refusal a person triggered has to be visible to that
                    // person, not only to the wire that would have read it.
                    state.say(Utterance::refused(&why));
                }
            }
            Hit::Stepper(id, verb) => {
                if let Err(why) = Self::step(state, &id, verb) {
                    state.say(Utterance::refused(&why));
                }
            }
            Hit::Remedy(id) => Self::apply_remedy(state, &id),
            // ★★★★★ R1903 — both palette gestures go through the ONE verb, so
            // the pointer and a client cannot mean different things by them.
            Hit::PaletteFold | Hit::PaletteStrip => {
                if let Err(why) = Self::place_palette(state, "toggle") {
                    state.say(Utterance::refused(&why));
                }
            }
            // ★★★★★ R1900 — the press already brought this tab to the front,
            // so the latch says which one is there rather than doing it again.
            //
            // Not a second `reveal`: a press that reveals on the way down and
            // again on release would be two mutations for one gesture, and the
            // second would undo a drag that had meanwhile taken the card
            // somewhere else. The sentence is the whole act on this path, and
            // it is the pointer's half of what `reveal` answers on the wire.
            Hit::Tab(id) => {
                state.say(Utterance::done(format!("{} is in front", label_of(&id))));
                state.selected.set(Some(id));
            }
            // ★★★★★ R1721 — the rule applies the choice and says what happened;
            // this arm only stores it. `Utterance` either way, so a refusal
            // (there is one: a rule that keeps one on) reaches the person by the
            // same path a success does.
            Hit::FilterChip(id, n) => Self::choose_filter(state, &id, n),
            // ★ R1851 — the pointer and the wire arrive at ONE function. A
            // heading press cycles this column's order through
            // `cycle_sort` — the framework's own three-state cycle
            // (ascending -> descending -> unsorted) — and then goes through the
            // same verb a client calls, so the two paths cannot drift.
            Hit::AlarmColumn(_, n) => {
                let next = match state.alarm_sort.get() {
                    Some((col, ascending)) if col == n => cycle_sort(Some(ascending)),
                    // A press on a different column starts that column's cycle,
                    // rather than inheriting the direction of the one before it.
                    _ => Some(true),
                };
                let word = ALARM_COLUMN_KEYS.get(n).copied().unwrap_or_default();
                let order = format!("{word}:{}", sort_dir_str(next));
                if let Err(why) = Self::sort_alarms(state, &order) {
                    state.say(Utterance::refused(&why.to_string()));
                }
            }
            // ★★★★★ R1907 — a person can now say WHERE a detached panel lives.
            //
            // Through the same verb the wire uses, and asking the POLICY where
            // "next" is rather than deciding here: this arm does not know that
            // the home after a window is the canvas, so a third home would need
            // nothing of it. R1902's finding one gesture over is why — a
            // painter that re-spells a policy is a second answer waiting to
            // disagree with the first.
            Hit::FloatHome(id) => {
                if let Err(why) = Self::send_home(state, &id, HomeRequest::Next) {
                    state.say(Utterance::refused(&why.to_string()));
                }
            }
            Hit::FloatRedock(id) => {
                if let Err(why) = Self::redock(state, &id) {
                    state.say(Utterance::refused(&why));
                }
            }
            Hit::FloatClose(id) => {
                Self::remove(state, &id);
                state.say(Utterance::done(format!("{} closed", label_of(&id))));
            }
            Hit::Card(id) | Hit::Grip(id) => {
                state.say(Utterance::done(format!("{} selected", label_of(&id))));
            }
            // A press on a panel raised and grabbed it (`open_float_grab`);
            // there is nothing left for the release to do, and the raise is
            // why this is no longer the same arm as hitting nothing.
            Hit::Float(_) | Hit::FloatResize(_) | Hit::Nothing => {}
        }
    }

    fn press_chip(state: &Rc<ShellState>, chip: BarChip) {
        match chip {
            // R1946 — one arm for every declared tab, so a tab added to the
            // specification presses without a second edit here.
            BarChip::Tab(n) => {
                let name = spec::VIEW_TABS[n].title;
                state.tab.set(name.to_string());
                state.say(Utterance::done(format!("view {name}")));
            }
            BarChip::Source => {
                let now = state.source.get();
                let at = SOURCES.iter().position(|s| *s == now).unwrap_or(0);
                let next = SOURCES[(at + 1) % SOURCES.len()];
                state.source.set(next.to_string());
                state.say(Utterance::done(format!("source {next}")));
            }
            BarChip::Capture => {
                let on = !state.capturing.get();
                state.capturing.set(on);
                state.say(Utterance::done(format!(
                    "capture {}",
                    if on { "on" } else { "off" }
                )));
            }
            BarChip::Search => {
                state.searching.set(true);
                state.say(Utterance::done("searching (Enter or Escape leaves)"));
            }
        }
    }

    fn press_sub(state: &Rc<ShellState>, chip: SubChip) {
        match chip {
            SubChip::Preset => {
                let open = !state.preset_open.get();
                state.preset_open.set(open);
            }
            SubChip::EditLayout => {
                let on = !state.editing.get();
                state.editing.set(on);
                state.say(Utterance::done(if on {
                    "layout edit mode"
                } else {
                    "layout locked"
                }));
            }
            SubChip::AddWidget => {
                // ★★★★★ R1903 — this is the canon's `openPalette`, at last.
                //
                // The comment that stood here said "the palette is always open
                // in this shell, so the button is what SAYS where widgets come
                // from" — true when written, and it was the gap written down:
                // the canon's button OPENS the drawer if it is closed and then
                // says its sentence, and ours could only ever say the sentence.
                //
                // Opening rather than toggling, deliberately and as the canon
                // does: a reader who asks to add a widget wants the palette
                // there, and a button that closed it on the second press would
                // take the thing away from somebody reaching for it.
                //
                // ★ R1695 — it used to move the rail's highlight to `catalog`
                // as well, which was that round's defect in miniature: the rail
                // said you were at a destination the window had not taken you
                // to. The pointer is what the button aims, and it aims at the
                // palette on this page.
                if state.palette_at.get().folded {
                    if let Err(why) = Self::place_palette(state, "unfold") {
                        state.say(Utterance::refused(&why));
                    }
                } else {
                    state.say(Utterance::done("pick a widget from the palette \u{2192}"));
                }
            }
        }
    }

    /// ★ R1695 — flip one Settings switch.
    ///
    /// Keyed by the specification's own order rather than by a field per option,
    /// so a fifth switch is a row in the table and nothing here.
    fn toggle_option(state: &Rc<ShellState>, key: &str) {
        let Some(n) = spec::OPTIONS.iter().position(|o| o.key == key) else {
            return;
        };
        let mut on = state.options.get();
        on[n] = !on[n];
        state.options.set(on);
        state.say(Utterance::done(format!(
            "{} {}",
            spec::OPTIONS[n].title,
            if on[n] { "on" } else { "off" }
        )));
    }

    /// ★★★★★ R1762 — open a value row's roster, or dismiss the one that is
    /// open.
    ///
    /// Pressing a second row while one is open moves to that row rather than
    /// closing everything, because two rosters open at once is a state a reader
    /// never asked for and `picking` cannot hold.
    fn toggle_roster(state: &Rc<ShellState>, key: &str) {
        let holding = state
            .picking
            .borrow()
            .as_ref()
            .map(|(open, _)| open.clone());
        if holding.as_deref() == Some(key) {
            *state.picking.borrow_mut() = None;
            state.say(Utterance::new(Tone::Unchanged, "closed".to_owned()));
            return;
        }
        // ★ R2021 — a key nothing declares is a REFUSAL naming the key, not a
        // crash. The keys reaching here are built by the hit test from what is
        // painted, so an unknown one means the paint and this file disagree —
        // which is a thing to be told about rather than to die on.
        let Some(valued) = Valued::from_key(key) else {
            state.say(Utterance::refused(&InvokeError::rejected(format!(
                "{key:?} is not a row this screen offers a roster over"
            ))));
            return;
        };
        let options = valued.options();
        let chosen = valued.read(state);
        match Picker::over(options, &chosen) {
            Ok(picker) => {
                let title = valued.title();
                *state.picking.borrow_mut() = Some((key.to_owned(), picker));
                state.say(Utterance::done(format!("{title} open, {chosen}")));
            }
            // A roster with nothing in it is a defect in this file's tables
            // rather than a state a reader can reach, and it is said out loud
            // rather than swallowed: a control that opened onto nothing would
            // read as a control that does not work.
            Err(why) => state.say(Utterance::refused(&format!("{why:?}"))),
        }
    }

    /// ★★★★★ R1762 — take the word at `n` of the open roster, write it where
    /// the value lives, and close.
    fn choose_value(state: &Rc<ShellState>, key: &str, n: usize) {
        let word = {
            let mut picking = state.picking.borrow_mut();
            let Some((open, picker)) = picking.as_mut() else {
                return;
            };
            if open != key || !picker.point_at(n) {
                return;
            }
            picker.highlighted().to_owned()
        };
        *state.picking.borrow_mut() = None;
        // ★★★★★ R2021 — the write goes through the row itself, which for a
        // card's setting means through the verb a client calls. The sentence a
        // reader hears is the writer's too, so the two channels do not spell
        // one outcome two ways — `filter_alarms` already says *alarms show warn
        // and above* and this used to say it a second time in its own words.
        if let Some(valued) = Valued::from_key(key) {
            valued.write(state, &word);
        }
    }

    /// ★ R1695 — choose a theme from the Settings page's segment.
    ///
    /// The same `ThemeProvider` the application bar's toggle and the `t` chord
    /// write, so three affordances over one fact rather than three facts.
    fn choose_theme(state: &Rc<ShellState>, n: usize) {
        let dark = n == 0;
        state.theme.set_mode(if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        });
        state.say(Utterance::done(format!(
            "theme {}",
            spec::THEMES[n].to_lowercase()
        )));
    }

    /// The preset menu's rows: every saved layout, then "Save current layout".
    fn press_preset_item(state: &Rc<ShellState>, n: usize) {
        let names: Vec<String> = state.presets.borrow().names();
        if let Some(name) = names.get(n) {
            ShellOracle::apply_preset(state, name).ok();
            return;
        }
        // The last row saves. The name is derived rather than typed, because a
        // text prompt is a modal this shell has no business inventing.
        let name = format!("Layout {}", names.len() + 1);
        Self::save_preset(state, &name).ok();
        state.preset_open.set(false);
    }

    /// What pressing an actionable remedy does.
    ///
    /// The framework decides WHICH remedy a state has; what a remedy MEANS for
    /// this data is the application's, which is why this lives here and
    /// `Remedy` has no `perform`.
    /// ★★★★★ R1721 — choose a saved filter on the filter card.
    ///
    /// The rule ([`spec::FILTER_ROW`]) decides what the choice does and writes the
    /// sentence; this stores the result. The `Option<usize>` the state holds is
    /// the rule's shape rather than a copy of it — five booleans would let this
    /// screen hold two chips on, which the row it announces says cannot happen.
    fn choose_filter(state: &Rc<ShellState>, id: &str, n: usize) {
        let mut row = filter_row(state, id);
        let said = row.choose(n);
        if said.tone() == Tone::Done {
            state.filter_chip.set(row.chosen());
            // A pointer press moves the cursor too, so the keyboard picks up
            // where the mouse left off rather than starting from the seat
            // somebody walked to five presses ago.
            let tag = row.chips()[n].tag.clone();
            state.with_cursor(&filter_chips_tag(), |roving| roving.point_at(&tag));
        }
        state.say(said);
    }

    fn apply_remedy(state: &Rc<ShellState>, id: &str) {
        let Some(card) = state.card(id) else { return };
        let Some(remedy) = card.remedy().filter(|r| r.is_actionable()) else {
            // ★ R1719 — a card whose trouble has no remedy is not refusing the
            // person; there is nothing here to do, which is the third arm.
            state.say(Utterance::unchanged(format!(
                "nothing to do about {}",
                label_of(id)
            )));
            return;
        };
        let next = match remedy {
            Remedy::Retry => CardState::Loading,
            Remedy::Widen => {
                state.search.set(String::new());
                CardState::Ready
            }
            Remedy::Authorize => CardState::Ready,
            Remedy::Wait | Remedy::Nothing => return,
        };
        state.update_card(id, |card| card.set_state(next.clone()));
        state.say(Utterance::done(format!(
            "{}: {} \u{2192} {}",
            label_of(id),
            remedy.wire(),
            next.wire()
        )));
    }

    /// The keymap, as one function so the wire and a real keyboard drive the
    /// same one rather than two that drift.
    fn key(state: &Rc<ShellState>, chord: &str) -> bool {
        Self::key_at(state, focus_state::focused().as_deref(), chord)
    }

    /// ★★★★★ R1698 — **the keymap, told where the reader is standing.**
    ///
    /// Before this round the screen's keyboard was global: an arrow meant "move
    /// the board's selection" wherever focus was, so Tabbing to the rail and
    /// pressing Down moved a card on a board the reader had left. That is half
    /// of the WAI-ARIA composite pattern missing — R1696 gave each composite one
    /// Tab stop and nothing gave it a cursor inside.
    ///
    /// So the composite that owns the focus is asked FIRST, and only a key it
    /// does not navigate by falls through to the screen. A composite declaring
    /// [`Axis::Horizontal`]
    /// returns `None` for `ArrowUp`, which is what lets a vertical enclosing
    /// gesture still work while the reader is inside a horizontal bar.
    fn key_at(state: &Rc<ShellState>, focused: Option<&str>, chord: &str) -> bool {
        if let Some(stop) = focused
            && let Some(landing) = state
                .with_cursor(stop, |roving| roving.key(chord))
                .flatten()
        {
            Self::landed(state, stop, landing);
            return true;
        }
        if state.searching.get() {
            return Self::search_key(state, chord);
        }
        if chord == "/" {
            state.searching.set(true);
            state.say(Utterance::done("searching (Enter or Escape leaves)"));
            return true;
        }
        let selected = state.selected.get();
        let (modifiers, base) = chord.rsplit_once('+').map_or(("", chord), |(m, b)| (m, b));
        let shift = modifiers.contains("Shift");
        let alt = modifiers.contains("Alt");
        let direction = match base {
            "ArrowLeft" => Some(TileDirection::Left),
            "ArrowRight" => Some(TileDirection::Right),
            "ArrowUp" => Some(TileDirection::Up),
            "ArrowDown" => Some(TileDirection::Down),
            _ => None,
        };
        if let Some(direction) = direction {
            // ★★★★★ R1698 — **the board's arrows belong to the board.**
            //
            // A key a composite does not navigate by falls through, which is
            // right; what is not right is where it used to land. Measured after
            // the cursors went in: standing on the application bar and pressing
            // Down moved a CARD on the board the reader had left, because this
            // handler had no idea where anybody was standing.
            //
            // `None` still reaches it, and deliberately: that is the wire's own
            // channel (`invoke("key", …)` with nothing focused) and an agent
            // asking the board to move its selection is asking for exactly
            // that. What is refused is an arrow arriving from inside another
            // composite.
            if focused.is_some_and(|tag| tag != "shell.canvas") {
                return false;
            }
            return Self::arrow(state, direction, shift, alt);
        }
        match base {
            "Enter" => selected.is_some_and(|id| {
                if state.maximized.get().is_some() {
                    Self::restore(state).is_ok()
                } else {
                    Self::maximize(state, &id).is_ok()
                }
            }),
            "Escape" => {
                if state.preset_open.get() {
                    state.preset_open.set(false);
                    return true;
                }
                Self::restore(state).is_ok()
            }
            "Delete" | "Backspace" => selected.is_some_and(|id| {
                Self::act(state, &IntrospectValue::Text(format!("{id},close"))).is_ok()
            }),
            "e" | "E" => {
                Self::press_sub(state, SubChip::EditLayout);
                true
            }
            "o" | "O" => selected.is_some_and(|id| {
                if state.is_floating(&id) {
                    Self::redock(state, &id).is_ok()
                } else {
                    Self::act(state, &IntrospectValue::Text(format!("{id},tear_off"))).is_ok()
                }
            }),
            "r" | "R" => selected.is_some_and(|id| {
                Self::apply_remedy(state, &id);
                true
            }),
            "c" | "C" => {
                Self::press_chip(state, BarChip::Capture);
                true
            }
            "t" | "T" => {
                let dark = theme_word(&state.theme) != "dark";
                state.theme.set_mode(if dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                });
                state.say(Utterance::done(format!(
                    "theme {}",
                    if dark { "dark" } else { "light" }
                )));
                true
            }
            "s" | "S" => {
                Self::press_chip(state, BarChip::Source);
                true
            }
            _ => false,
        }
    }

    /// Keystrokes while the search box has them. Deliberately narrow, and
    /// everything else is REFUSED rather than falling through to the board,
    /// because a chord that quietly did something else while the caret was in a
    /// text box is the worst thing a mode can do.
    fn search_key(state: &Rc<ShellState>, chord: &str) -> bool {
        match chord {
            "Enter" | "Escape" => {
                state.searching.set(false);
                state.say(Utterance::done(format!("search {:?}", state.search.get())));
                true
            }
            "Backspace" => {
                let mut text = state.search.get();
                let had = text.pop().is_some();
                state.search.set(text);
                had
            }
            "Space" => {
                state.search.set(format!("{} ", state.search.get()));
                true
            }
            one if one.chars().count() == 1 && !one.chars().any(char::is_control) => {
                state.search.set(format!("{}{one}", state.search.get()));
                true
            }
            _ => false,
        }
    }

    /// The four arrows, and the three things a modifier makes them mean.
    fn arrow(state: &Rc<ShellState>, direction: TileDirection, shift: bool, alt: bool) -> bool {
        let Some(id) = state.selected.get() else {
            // Nothing selected: the first arrow picks a card, so the keyboard
            // has a way in that does not require a click.
            let first = state.placed().first().map(|c| c.id().as_str().to_string());
            let had = first.is_some();
            state.selected.set(first);
            return had;
        };
        let tile = TileId::new(id.clone());
        if !shift && !alt {
            let board = state.board.get();
            let Some(next) = board.neighbour(&tile, direction).map(|t| t.id.clone()) else {
                return false;
            };
            state.selected.set(Some(next.as_str().to_string()));
            state.say(Utterance::done(format!(
                "{} selected",
                label_of(next.as_str())
            )));
            return true;
        }
        let nudge = match (shift, alt) {
            (true, false) => TileNudge::Move(direction),
            (false, true) => TileNudge::Grow(direction),
            _ => TileNudge::Shrink(direction),
        };
        let mut board = state.board.get();
        match board.nudge(&tile, nudge) {
            Ok(_) => {
                state.board.set(board);
                state.say(Utterance::done(format!("{} {nudge:?}", label_of(&id))));
                true
            }
            Err(why) => {
                state.say(Utterance::refused(&why));
                false
            }
        }
    }
}

impl External for ShellOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Keep the cursor while a press is held, so a drag that strays off a card
    /// keeps previewing rather than being cancelled by a stray pixel.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// Hover positions too: the shell tracks the cursor so a press knows where
    /// it landed, and the press event itself carries no coordinates.
    fn wants_hover_move(&self) -> bool {
        true
    }

    /// ★★★★★ R1700 §5.35 — what a press here addresses, for the framework to
    /// hold against what this screen painted here.
    fn target_at(&self, x: u32, y: u32) -> PointerTarget {
        // ★ R1737 — through the framework's own frame conversion, which R1714
        // wrote precisely so a caller could put it on every such point without
        // first asking whether this screen pans. It is the identity here today;
        // it was missing on four of the five self-hit-testing screens in this
        // tree, so each of them was a screen whose hit test would be right at
        // one offset and wrong at every other — the defect R1714 measured on
        // the node lab, where a 400-pixel pan took `scene/pointer_target` from
        // 57 deliverable rectangles to 1.
        let (x, y) = pinion_core::external::into_layout(VIEW_TAG, (x, y));
        self.state.as_ref().map_or(PointerTarget::Unanswered, |s| {
            word_or_nothing(&Hit::at(s, x, y))
        })
    }

    /// ★★★★★ R1700 §5.35 — the same question by name, over [`Hit::of_tag`].
    ///
    /// R1699 built that inverse for keyboard activation and gated it against
    /// the paint, so it is not derived from the geometry this is checked
    /// against — which is what makes the pair two derivations rather than one
    /// read twice.
    fn target_of_tag(&self, tag: &str) -> PointerTarget {
        self.state.as_ref().map_or(PointerTarget::Unanswered, |s| {
            word_or_nothing(&Hit::of_tag(s, tag))
        })
    }

    fn pointer_move(&mut self, at: PointerReading) {
        let Some(state) = self.state.clone() else {
            return;
        };
        // ★ R1656 — the LIVE surface, told by `External::on_resize`. It was the
        // design constant, which is right at the size the app opens in and
        // wrong by opening-size-over-current-size at every other size: a person
        // reported nodes that stop clicking after a maximise, and the
        // coordinates were measured arriving at 0.5775x.
        // ★★ R1714.1 — through the framework's own expression. R1656 fixed the
        // BASIS here; R1714 moved the clamp and the multiplication with it, so
        // every self-hit-testing screen resolves a pointer the same way and a
        // screen that later declares a pan gets that term for free.
        let (px, py) = pinion_core::external::layout_point(VIEW_TAG, at.at);
        Self::move_cursor(&state, px, py);
    }

    /// ★★★★★ R1735 §5.51 — **a palette press opens a real drag session.**
    ///
    /// R1733 built the palette→board carry inside this screen: the press picked
    /// a footprint up, `pointer_move` previewed it and `release` placed it. That
    /// worked because one surface owned both ends, and it is exactly the shape
    /// R1734's target contract was built to replace. This is the screen joining
    /// that contract: from here the ROUTER drives the gesture, asks this
    /// surface's own published declaration whether the drop is admissible, and
    /// hands the acceptance back as the commit's witness.
    ///
    /// Only a palette carry. A card gripped on the board and a detached panel
    /// being moved are this screen's own capture gestures — they never leave
    /// the surface and have no destination to ask — so returning `None` keeps
    /// them on the pointer path they already run on.
    ///
    /// `Copy`, and it is measured rather than chosen: the behaviour reference's
    /// palette declares its drag a copy at drag start (the row stays, the board
    /// gains a card).
    fn begin_drag(&self) -> Option<DragPayload> {
        let state = self.state.as_ref()?;
        let Some(Hit::Palette(kind)) = *state.pressed.borrow() else {
            return None;
        };
        // `press` has already run `pick_up`, whose three refusals are the
        // palette's own. No carry means it refused, and a refused pick-up must
        // not open a session — otherwise the router would drive a drag with
        // nothing in hand.
        let drag = state.drag.get()?;
        if drag.carried().is_placed() {
            return None;
        }
        Some(
            DragPayload::new(
                BOARD_WIDGET_DRAG_KIND,
                IntrospectValue::Text(kind.to_owned()),
            )
            .with_actions(DropActions::one(DropAction::Copy)),
        )
    }

    /// ★★★★★ R1735 §5.51 — **the cursor keeps arriving while the drag runs.**
    ///
    /// This is the half that made the move a debt rather than a refactor. A
    /// router with a session open stops calling
    /// [`pointer_move`](External::pointer_move), and this screen hit-tests
    /// itself from `state.cursor` — so every gesture that reads the cursor (a
    /// card grip, a floating panel's move, its resize) would freeze the moment
    /// a palette drag started. The floor has the same shape and no way out of
    /// it: measured at 6.11.1, a source's pointer handler runs **zero** times
    /// while its own drag is in flight and no member of the drag object carries
    /// a point, so a self-hit-testing screen there simply has no live cursor.
    ///
    /// Through [`layout_point`](pinion_core::external::layout_point) and the
    /// drop point's own fractions, which is the SAME expression
    /// [`pointer_move`](External::pointer_move) resolves a pointer with — R1714.1's
    /// rule, so a screen that later declares a pan gets that term here for free.
    fn drag_to_at(&mut self, _payload: &DragPayload, update: &DragUpdate) {
        let Some(state) = self.state.clone() else {
            return;
        };
        *state.standing.borrow_mut() = update.standing.clone();
        let (px, py) = Self::drag_cursor(update);
        state.cursor.set((px, py));
    }

    /// R1735 §5.51 — the gesture is over: nothing is in hand and no latch is
    /// left behind.
    ///
    /// The commit (or the refusal) has already happened in
    /// [`drop_commit`](External::drop_commit) — the router runs the target half
    /// first — so this is only the source's own tidying. It clears the latch
    /// when the press became a real drag, because the router then synthesises no
    /// trailing press-release and `release` never runs to clear it.
    ///
    /// ★ And when the press did NOT become a drag, the latch is left alone on
    /// purpose: the router synthesises the release, `release` acts on the latch,
    /// and a click on a palette row still adds a card. That is R1733's rule kept
    /// by the framework's own click-vs-drag verdict instead of by a second one
    /// written here.
    fn drag_release_at(&mut self, _payload: &DragPayload, update: &DragUpdate) {
        let Some(state) = self.state.clone() else {
            return;
        };
        let (px, py) = Self::drag_cursor(update);
        state.cursor.set((px, py));
        state.drag.set(None);
        // ★★★★★ R1735 — and a refused release SAYS SO, in the framework's own
        // words rather than in a second wording written here.
        //
        // R1720's rule is that a refusal reaches the person, and this gesture
        // had no way to keep it: before this round an abandoned carry fell
        // through to the latch and announced whatever the latch did, which is
        // an announcement about a different act. A release the board would not
        // take now says why the board would not take it — and the sentence is
        // the one the standing carried, which is the one the wire published,
        // which is the one `drop_offered` produced. One refusal, three readers.
        //
        // Only for a real drag: a click that never moved is not a refused drop,
        // it is the palette's action, and it announces its own outcome.
        if update.became_drag {
            state.pressed.borrow_mut().take();
            if let Some(refusal) = update.standing.refusal() {
                state.say(Utterance::refused(&refusal.sentence()));
            }
        }
        *state.standing.borrow_mut() = DropStanding::Nowhere;
    }

    /// ★★★★★ R1735 §5.51 — **the board answers for itself.**
    ///
    /// The router has already checked this screen's published declaration
    /// ([`ShellOracle::declared_drop_contract`]) before calling here, so the
    /// three structural refusals never reach this body. What is left is what
    /// only live state knows: whether the dashboard is the page showing, and
    /// whether the cursor is over the canvas rather than over a floating panel.
    ///
    /// The acceptance carries the cell as its landing, and that cell comes from
    /// the ONE [`TileDrag`] this screen carries — the same object
    /// [`ShellOracle::place_carried`] later drops. So the preview, the published
    /// standing and the commit are one arithmetic, which is R1733's property
    /// restated at the input contract.
    fn drop_offered(&mut self, offer: &DropOffer) -> DropVerdict {
        let Some(state) = self.state.clone() else {
            return DropVerdict::decline("no capture is loaded");
        };
        let (px, py) =
            pinion_core::external::layout_point(VIEW_TAG, (offer.at.x_rel, offer.at.y_rel));
        match Self::preview_carry(&state, px, py) {
            Ok((col, row)) => DropVerdict::accept(
                offer.actions.first(),
                IntrospectValue::Json(serde_json::json!({ "col": col, "row": row })),
            ),
            // The refusal is the preview's own sentence, so what a person is
            // told and what the board did are one string.
            Err(why) => DropVerdict::decline(why),
        }
    }

    /// R1735 §5.51 — the cursor left, so the preview goes.
    ///
    /// Only the preview. What is in hand stays in hand: a carry that wanders off
    /// the board and back must still be carrying something, which is what
    /// [`TileDrag::leave`] expresses and a `None` here would destroy.
    fn drop_left(&mut self) {
        let Some(state) = self.state.clone() else {
            return;
        };
        if let Some(mut drag) = state.drag.get() {
            drag.leave();
            state.drag.set(Some(drag));
        }
    }

    /// ★★★★★ R1735 §5.51 — **put it down where the preview showed.**
    ///
    /// The witness is asserted against the live carry rather than re-read from
    /// the cursor: `place_carried` drops the very [`TileDrag`] whose `landing`
    /// produced `accept.landing`, so the two cannot describe different cells.
    /// The check is here because the framework's guarantee is that the commit
    /// RECEIVES what the preview produced — a screen still has to be the one
    /// that applies it, and this is that application saying so out loud.
    fn drop_commit(&mut self, _offer: &DropOffer, accept: &DropAccept) {
        let Some(state) = self.state.clone() else {
            return;
        };
        let Some(drag) = state.drag.get() else {
            return;
        };
        debug_assert_eq!(
            drag.landing()
                .map(|(col, row)| serde_json::json!({"col": col, "row": row})),
            match &accept.landing {
                IntrospectValue::Json(v) => Some(v.clone()),
                _ => None,
            },
            "the acceptance the router hands back is the carry's own landing",
        );
        state.drag.set(None);
        if let Err(why) = Self::place_carried(&state, drag) {
            state.say(Utterance::refused(&why));
        }
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

fn theme_word(theme: &ThemeProvider) -> String {
    match theme.mode() {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::System => "system",
        // `ThemeMode` is `#[non_exhaustive]`: a mode this shell has not been
        // taught is reported as itself rather than folded into one of the
        // three, which would make the app bar claim a setting nobody chose.
        other => return format!("{other:?}").to_lowercase(),
    }
    .to_string()
}

/// The three transport words, derived rather than stored: "live" is not a
/// fourth clock state, it is the absence of a replay while capture is on.
fn transport_word(status: TransportStatus, capturing: bool) -> String {
    match status {
        TransportStatus::Playing => "replaying",
        TransportStatus::Stopped if capturing => "live",
        TransportStatus::Paused | TransportStatus::Stopped => "paused",
    }
    .to_string()
}

// --- The view ----------------------------------------------------------------

/// The colours every painter here reads.
#[derive(Debug, Clone, Copy)]
struct Palette {
    ink: Color,
    muted: Color,
    accent: Color,
    on_accent: Color,
    accent_fg: Color,
    canvas: Color,
    panel: Color,
    raised: Color,
    high: Color,
    outline: Color,
    grid: Color,
    /// R1668 — the ink an identifier the capture cannot resolve is drawn in.
    /// A role rather than a literal because a warning has to hold its contrast
    /// in both themes, which a hand-picked amber does in exactly one.
    warn: Color,
    /// R1719 — the ink the toast's bullet takes when what it says is a refusal.
    refused: Color,
    /// ★ R1806 — the ink a row **outside the active cross-filter** is drawn in.
    ///
    /// Fainter than [`muted`](Self::muted), which this screen already spends on
    /// "present but not the point", because a filtered-out row is a third
    /// thing: still there, deliberately not current. A role rather than a
    /// hand-picked grey, for `warn` and `refused`'s reason — a literal holds
    /// its contrast in exactly one of the two themes.
    faded: Color,
}

/// A text run at an exact place in its container.
///
/// ★ The `with_layout` is not optional. A `TextNode` carries a rect, but
/// without a layout style it is laid out **in flow** by the parent — so a set
/// of labels written at deliberate coordinates stacks vertically instead, and
/// the screen reads as a list of everything the container holds. This shell's
/// first draft omitted it and every card painted its title, badge and
/// description down the left edge.
/// ★★ R1672 — and the run **elides**, which this screen alone did not say.
///
/// A run placed at an exact rectangle has a fixed width, so what happens when
/// the string is longer than that width is a policy somebody has to choose.
/// Screens A and B choose `Ellipsis` for every run they place; this screen left
/// the default, so a label too long for its box was painted *past* it — over
/// whatever was next — rather than shortened. Measured the round the ink gate
/// reached this screen: **32 marks**, including four of the app bar's own.
///
/// `Ellipsis` is the honest contract for a fixed box: it says the box wins and
/// the string gives way. A box that is genuinely too small for its string is
/// still a defect, and it is the shaped measurement at boot
/// (`scene/containment`) that reports that one — this policy is about what the
/// paint does when it happens, not about pretending it did not.
fn label(text: &str, rect: Rect, px: u32, fg: Color) -> Scene {
    clipped(text, rect, px, fg, TextOverflow::Ellipsis)
}

/// A label seated in the chrome box that holds it — centred on that box's line,
/// at the height its own face needs.
///
/// ★★★★★ R1880 — the caller passes the **chrome**, not a rectangle for the
/// text, and that is the whole point: a height it cannot state is a height it
/// cannot get wrong. Measured at this round's entry, every one of the
/// application bar's forty-eight runs — eight of them, on each of the six
/// destinations the bar is painted on — sat in a box too short for its own
/// face, from six separately written seats that had each picked a plausible
/// number: `16` for a 12px face wanting 20, `18` for a 13px face wanting 21,
/// `16` again for an 11px face wanting 18.
///
/// ⇒ **not one judgement made six times, but one rule nobody had written**,
/// which is the mechanical-duplication case `code_chip` next door records.
/// [`pinion_core::containment::line_rect_in`] is that rule, and this helper is
/// how a call site reaches it without also re-deriving where the box goes.
///
/// `chrome` is the box the label sits in, as its own container was laid out —
/// so the rectangle comes back in the child coordinates the container's
/// children use, and a caller does not convert anything.
fn chrome_label(chrome: Rect, x: u32, w: u32, text: &str, px: u32, fg: Color) -> Scene {
    label(
        text,
        pinion_core::containment::line_rect_in(chrome_seat(chrome), x, w, px),
        px,
        fg,
    )
}

/// ★★★★★ R1956 — **the seat a chrome box's children are placed in**: its own
/// extent, at the origin those children are laid out from.
///
/// Split out of [`chrome_label`] when a second caller needed it — a preset
/// chip's fold arrow, which is a MARK rather than a run and so wants
/// [`pinion_core::containment::band_in`] instead of `line_rect_in`, but wants
/// the same seat. Written twice the two would be free to disagree about what a
/// chrome box's seat is, and the defect that opened this round is exactly a
/// rule with two authors.
const fn chrome_seat(chrome: Rect) -> Rect {
    Rect::new(0, 0, chrome.w, chrome.h)
}

/// A kind's three-letter code in its coloured chip, the word seated by the
/// chip.
///
/// ★ R1672 — one helper because two call sites drew this and **both put the
/// word's box past the chip's right edge**, by the same three pixels, from two
/// separately picked insets (`(5, 9, 30, 14)` in a 32-wide chip and
/// `(9, 9, 34, 14)` in a 40-wide one). Neither is a judgement anybody made
/// twice; it is one rule written twice and got wrong twice, which is the
/// mechanical-duplication case.
fn code_chip(code: &str, chip: Rect, skin: BoxStyle, ink: Color, tag: Option<String>) -> Scene {
    /// The clearance a code chip keeps between its edge and its word.
    const CODE_PAD: u32 = 5;
    let line = pinion_core::containment::line_box(FONT_TINY);
    let word = Rect::new(
        CODE_PAD,
        chip.h.saturating_sub(line) / 2,
        chip.w.saturating_sub(CODE_PAD * 2),
        line,
    );
    let node = ContainerNode::new(vec![label(code, word, FONT_TINY, ink)]);
    let node = match tag {
        Some(tag) => node.with_tag(tag),
        None => node,
    };
    Scene::Container(node.with_style(skin).with_layout(absolute(chip)))
}

/// Place a node at an exact rectangle inside its container, and make it
/// **pointer-transparent**.
///
/// ★★ The transparency is load-bearing and it is the bug this shell shipped.
/// The §5.35 input router resolves the hit target by hit-testing the paint
/// scene for the DEEPEST TAGGED node under the cursor, then looks up an
/// `External` carrying that tag. Every tag here is an ADDRESS — the thing the
/// wire and the demo compare (R1613: a tag is an address, not a claim of
/// clickability) — and there is exactly one `External`, the root. So a tagged
/// child that is not transparent becomes the target, the lookup finds no
/// `External` with that tag, and the router silently forwards NOTHING: a real
/// mouse move never arrives and every control is dead to a hand, while the
/// wire's own `point` / `send` keep working because they bypass the router.
///
/// That asymmetry is why the first version passed its whole demo while being
/// unusable, and it is exactly the "parallel automation surface" the module
/// docs claim does not exist. The root container is the one node that keeps
/// its own layout, so it stays the target.
fn absolute(rect: Rect) -> LayoutStyle {
    LayoutStyle::decoration(rect)
}

// --- The Settings destination ------------------------------------------------
//
// ★★ R1695 — the reference's own Settings section, reproduced: four switch rows
// in two groups, two rows whose affordance is booked for a later release, and a
// two-way appearance segment. The switch is
// [`pinion_widget_paint::switch`](switch::view_switch) rather than a hand-drawn
// track — the class R1673 measured on screen A, where a track was painted with
// no knob at all because the screen drew a switch instead of using one.

/// The Settings page's metrics, region-local.
const SET_PAD: u32 = 24;
const SET_MAX_W: u32 = 720;
const SET_HEAD_H: u32 = 14;
const SET_HEAD_GAP: u32 = 10;
const SET_GROUP_GAP: u32 = 22;
const SET_ROW_H: u32 = 64;
const SET_ROW_PAD: u32 = 18;
const SET_CTRL_W: u32 = 96;
/// ★ R1762 — the seat a value row's collapsed chooser sits in.
///
/// Wider than a button's, because what it holds is a value rather than a verb:
/// the reference's capture-source row shows a device and its address, and a
/// seat sized for a word would elide the half that identifies it.
const SET_VALUE_W: u32 = 208;
/// The height of one option in an open value roster.
const SET_OPTION_H: u32 = 30;
/// ★ R1762 — the block the page's own heading and its one line occupy, above
/// the first group's own heading.
const SET_PAGE_HEAD_H: u32 = 58;
/// The seat the payload-format row's chips sit in.
const SET_PLUGIN_W: u32 = 148;
/// The appearance segment: its overall width, its inner pad and a chip's height.
///
/// Named rather than written into the painter and the hit test separately —
/// this screen's standing rule, and the class it has paid for three times.
const SEG_W: u32 = 148;
const SEG_PAD: u32 = 3;
const SEG_CHIP_H: u32 = 30;

/// One appearance chip's width, derived so the pair fills the segment.
const fn seg_chip_w() -> u32 {
    (SEG_W - SEG_PAD * 2) / 2
}

/// ★★★★★ R1784 — **what the settings page lays out in**, both numbers derived
/// from the metrics above it.
///
/// The comfortable width is the one this page already refuses to exceed:
/// [`settings_col`] caps its content at [`SET_MAX_W`], so anything beyond that
/// plus the inset is width the page declines to use.
///
/// The floor is where a row stops being a row. Every row is a title and a
/// control on one line, and the widest seat a control takes is
/// [`SET_VALUE_W`] — sized at R1762 to hold a device and its address rather
/// than a word. Below the inset, the row's own padding and that seat, the two
/// halves overlap.
const SETTINGS_MAX_W: u32 = SET_MAX_W + SET_PAD * 2;
const SETTINGS_MIN_W: u32 = SET_PAD * 2 + SET_ROW_PAD * 2 + SET_VALUE_W;
/// The settings page's height floor: the bar this host keeps, the page's own
/// heading block, and one group heading with one row under it.
const SETTINGS_MIN_H: u32 = APP_BAR_H + SET_PAD * 2 + SET_PAGE_HEAD_H + SET_HEAD_H + SET_ROW_H;

/// The content column: the page inset, bounded so the rows do not stretch to a
/// maximised window's width and leave their controls a screen away from their
/// titles.
fn settings_col(region: Rect) -> Rect {
    let w = region.w.saturating_sub(SET_PAD * 2).min(SET_MAX_W);
    Rect::new(SET_PAD, SET_PAD, w, region.h.saturating_sub(SET_PAD * 2))
}

/// How many rows a group holds — the switch groups from [`spec::OPTIONS`], the
/// key group from [`spec::KEY_ROWS`], and appearance is the single theme row.
fn settings_group_rows(group: &str) -> u32 {
    match group {
        "keys" => u(spec::KEY_ROWS.len()),
        "appearance" => 1,
        // ★ R1762 — the capture group's switches, and the two VALUE rows the
        // reference opens it with above them. Counted here rather than at the
        // paint, because the card's height and the row a control lands on are
        // one arithmetic and this file has drawn a card too short for its own
        // rows before.
        other => {
            u(spec::OPTIONS.iter().filter(|o| o.group == other).count())
                + settings_value_count(other)
                + settings_plugin_count(other)
        }
    }
}

/// How many plugin rows a group closes with. Only the decode group has one,
/// which is the reference's arrangement rather than a rule.
fn settings_plugin_count(group: &str) -> u32 {
    u32::from(group == "decode")
}

/// How many value rows a group opens with. Only the capture group has any, and
/// that is the reference's arrangement rather than a rule.
fn settings_value_count(group: &str) -> u32 {
    if group == "capture" {
        u(spec::VALUE_ROWS.len())
    } else {
        0
    }
}

/// The card rectangle a group occupies, region-local.
fn settings_group_rect(region: Rect, group: &str) -> Rect {
    let col = settings_col(region);
    // ★ R1762 — below the page's own heading, which the reference opens with.
    // One constant read by the paint, this arithmetic and the hit test, which
    // is this screen's standing rule about a number three things need.
    let mut y = col.y + SET_PAGE_HEAD_H;
    for (key, _) in spec::OPTION_GROUPS {
        let rows = settings_group_rows(key);
        y += SET_HEAD_H + SET_HEAD_GAP;
        if key == group {
            return Rect::new(col.x, y, col.w, rows * SET_ROW_H);
        }
        y += rows * SET_ROW_H + SET_GROUP_GAP;
    }
    Rect::new(col.x, y, col.w, 0)
}

/// A row's control seat, at the trailing end.
fn settings_ctrl_rect(row: Rect, w: u32) -> Rect {
    let h = 32;
    Rect::new(
        row.x + row.w.saturating_sub(SET_ROW_PAD + w),
        row.y + (SET_ROW_H.saturating_sub(h)) / 2,
        w,
        h,
    )
}

/// A small square dot — a grid pip, a status light, a grip dot.
fn dot(x: u32, y: u32, size: u32, fill: Color) -> Scene {
    disc(None, x, y, size, fill)
}

/// A [`dot`] a reader — or a walk — can address by name.
///
/// ★ R2012 — most dots here are strokes of a glyph and have nothing to be asked
/// about individually. The status bullet is not one of those: it is the whole
/// of what a sighted reader learns the tone from, so *what colour is it* has to
/// be a question something can ask, and a question aimed at a shape with no
/// address is answered by counting children.
fn named_dot(tag: &'static str, x: u32, y: u32, size: u32, fill: Color) -> Scene {
    disc(Some(tag), x, y, size, fill)
}

fn disc(tag: Option<&'static str>, x: u32, y: u32, size: u32, fill: Color) -> Scene {
    let mut node = ContainerNode::new(Vec::new())
        .with_style(BoxStyle::filled(fill).with_corner_radius(size / 2))
        .with_layout(absolute(Rect::new(x, y, size, size)));
    if let Some(tag) = tag {
        node = node.with_tag(tag);
    }
    Scene::Container(node)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "shell coordinates are < 2^13, exactly representable in f32"
)]
fn ppt(x: u32, y: u32) -> PathPoint {
    PathPoint::new(x as f32, y as f32)
}

/// A stroked polyline set, in `rect`-local coordinates.
fn strokes(rect: Rect, runs: &[Vec<(u32, u32)>], ink: Color, width: u32) -> Scene {
    let mut commands = Vec::new();
    for run in runs {
        for (n, (x, y)) in run.iter().enumerate() {
            let point = ppt(*x, *y);
            commands.push(if n == 0 {
                PathCommand::MoveTo(point)
            } else {
                PathCommand::LineTo(point)
            });
        }
    }
    Scene::Path(
        PathNode::new(rect, commands, PathStyle::stroked(Stroke::new(ink, width)))
            .with_layout(absolute(rect)),
    )
}

fn close_mark(rect: Rect, ink: Color) -> Scene {
    let (w, h) = (rect.w, rect.h);
    let (x0, y0, x1, y1) = (w / 2 - 4, h / 2 - 4, w / 2 + 4, h / 2 + 4);
    strokes(
        rect,
        &[vec![(x0, y0), (x1, y1)], vec![(x1, y0), (x0, y1)]],
        ink,
        1,
    )
}

// ★ R1817 — the detach mark moved to the card header's glyphs, and R1950 moved
// those on again to `pinion_widget_paint::control_mark`, where a panel's chrome
// draws from the same vocabulary.

/// R1697 — the resize mark: two diagonals climbing out of the bottom-right
/// corner, the form every window manager and every reference toolkit uses for
/// a size grip.
fn resize_mark(rect: Rect, ink: Color) -> Scene {
    let (w, h) = (rect.w, rect.h);
    strokes(
        rect,
        &[
            vec![(w - 11, h - 3), (w - 3, h - 11)],
            vec![(w - 6, h - 3), (w - 3, h - 6)],
        ],
        ink,
        1,
    )
}

/// The re-dock mark: a box with a bar along its foot.
fn redock_mark(rect: Rect, ink: Color) -> Scene {
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    strokes(
        rect,
        &[
            vec![
                (cx - 5, cy - 5),
                (cx + 5, cy - 5),
                (cx + 5, cy + 5),
                (cx - 5, cy + 5),
                (cx - 5, cy - 5),
            ],
            vec![(cx - 5, cy + 2), (cx + 5, cy + 2)],
        ],
        ink,
        1,
    )
}

/// ★★★★★ R1907 — **send this panel to the next home**: a frame with an arrow
/// leaving it to the right.
///
/// It must not read as [`redock_mark`], which is the control beside it and
/// means the opposite — back to the board. So the frame is drawn OPEN on the
/// side the arrow leaves by: re-dock closes a box, send-home opens one. R1697's
/// lesson, one control over: a control that looks like its neighbour tells a
/// person the same thing about two different acts.
fn send_home_mark(rect: Rect, ink: Color) -> Scene {
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    strokes(
        rect,
        &[
            // The frame, open on the right.
            vec![
                (cx + 1, cy - 5),
                (cx - 5, cy - 5),
                (cx - 5, cy + 5),
                (cx + 1, cy + 5),
            ],
            // The arrow leaving through the opening.
            vec![(cx - 2, cy), (cx + 6, cy)],
            vec![(cx + 3, cy - 3), (cx + 6, cy), (cx + 3, cy + 3)],
        ],
        ink,
        1,
    )
}

// ★ R1817 — the affordance marks moved to `card_header`, and R1697's lesson
// went with them: `restore` is the maximise control's OTHER face, because a
// control that toggles without changing its mark tells a person the same thing
// in both states. A lesson left behind when its code moves is a lesson nobody
// re-reads. ★ R1950 moved both again, to
// `pinion_widget_paint::control_mark::ControlMark`, where that face is a VALUE
// rather than a `bool` a caller has to remember to pass.

/// A framed pane, split once across and once down: a list of messages beside
/// what one of them contains.
fn pane_mark(rect: Rect, ink: Color) -> Scene {
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    strokes(
        rect,
        &[
            vec![
                (cx - 7, cy - 6),
                (cx + 7, cy - 6),
                (cx + 7, cy + 6),
                (cx - 7, cy + 6),
                (cx - 7, cy - 6),
            ],
            vec![(cx - 7, cy - 2), (cx + 7, cy - 2)],
            vec![(cx - 2, cy - 2), (cx - 2, cy + 6)],
        ],
        ink,
        1,
    )
}

/// Two nodes and a wire from the first's output to the second's input — the
/// graph the section authors, at icon size.
fn graph_mark(rect: Rect, ink: Color) -> Scene {
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    strokes(
        rect,
        &[
            vec![
                (cx - 7, cy - 7),
                (cx - 2, cy - 7),
                (cx - 2, cy - 3),
                (cx - 7, cy - 3),
                (cx - 7, cy - 7),
            ],
            vec![
                (cx + 2, cy + 3),
                (cx + 7, cy + 3),
                (cx + 7, cy + 7),
                (cx + 2, cy + 7),
                (cx + 2, cy + 3),
            ],
            vec![(cx - 2, cy - 5), (cx + 4, cy - 5), (cx + 4, cy + 3)],
        ],
        ink,
        1,
    )
}

/// Two sliders with their handles at different offsets.
fn slider_mark(rect: Rect, ink: Color) -> Vec<Scene> {
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    vec![
        strokes(
            rect,
            &[
                vec![(cx - 6, cy - 3), (cx + 6, cy - 3)],
                vec![(cx - 6, cy + 3), (cx + 6, cy + 3)],
            ],
            ink,
            1,
        ),
        dot(cx - 3, cy - 5, 4, ink),
        dot(cx + 1, cy + 1, 4, ink),
    ]
}

/// The rail's icon for one section, drawn rather than set in a font — a glyph
/// this project does not ship is a box, and a box is not an icon.
///
/// ★★ R1728 — the fallback arm below is the reason two adjacent seats were
/// drawn identically for as long as this function has existed. It stays,
/// because a rail key with no arm should still paint *something*; what changed
/// is that a gate now compares every seat's drawing with every other seat's, so
/// a second seat falling through fails instead of shipping.
fn rail_mark(key: &str, rect: Rect, ink: Color) -> Vec<Scene> {
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    match key {
        "dashboard" => vec![
            dot(cx - 6, cy - 6, 5, ink),
            dot(cx + 1, cy - 6, 5, ink),
            dot(cx - 6, cy + 1, 5, ink),
            dot(cx + 1, cy + 1, 5, ink),
        ],
        // Three nodes and the links between them — the reference's own mark for
        // this section, which this had never been. ★ R1728 redrew it: what was
        // here was a bracket under a dot, which is not a topology and is not
        // what the reference draws.
        "topology" => vec![
            dot(cx - 7, cy - 7, 5, ink),
            dot(cx + 3, cy - 6, 5, ink),
            dot(cx - 2, cy + 3, 5, ink),
            strokes(
                rect,
                &[
                    vec![(cx - 3, cy - 5), (cx + 3, cy - 4)],
                    vec![(cx - 4, cy - 3), (cx - 1, cy + 3)],
                    vec![(cx + 4, cy - 2), (cx + 1, cy + 3)],
                ],
                ink,
                1,
            ),
        ],
        // ★★★★★ R1728 — these two had **no arm at all** and fell through to the
        // fallback below, so the rail drew them identically: two adjacent seats
        // a reader could not tell apart, on a screen whose whole subject is
        // telling things apart. Nothing caught it because nothing had ever
        // compared one seat's drawing with another's.
        //
        // A list of session rows, the last one short.
        "sessions" => vec![strokes(
            rect,
            &[
                vec![(cx - 6, cy - 4), (cx + 6, cy - 4)],
                vec![(cx - 6, cy), (cx + 6, cy)],
                vec![(cx - 6, cy + 4), (cx + 2, cy + 4)],
            ],
            ink,
            1,
        )],
        "settings" => slider_mark(rect, ink),
        // ★★★★★ R1728 — the seat each of these belongs to is not the seat it
        // was under. Measured against the reference's own rail markup: the
        // chevrons are its **key-pattern** section and were keyed `decode`, and
        // the bulleted list is its **log** section and was keyed `catalog` —
        // which is where the node graph lab had been mounted, so the one
        // section this application had finished was wearing another's face.
        // A seat and its icon are one fact and nothing had ever compared them.
        //
        "packets" => vec![pane_mark(rect, ink)],
        // Two chevrons facing outward — a pattern standing in for what it
        // matches.
        "keys" => vec![strokes(
            rect,
            &[
                vec![(cx - 2, cy - 5), (cx - 6, cy), (cx - 2, cy + 5)],
                vec![(cx + 2, cy - 5), (cx + 6, cy), (cx + 2, cy + 5)],
            ],
            ink,
            1,
        )],
        // Bulleted lines: entries, each with a marker, the last one short.
        "logs" => vec![
            dot(cx - 7, cy - 5, 3, ink),
            dot(cx - 7, cy - 1, 3, ink),
            dot(cx - 7, cy + 3, 3, ink),
            strokes(
                rect,
                &[
                    vec![(cx - 2, cy - 4), (cx + 7, cy - 4)],
                    vec![(cx - 2, cy), (cx + 7, cy)],
                    vec![(cx - 2, cy + 4), (cx + 5, cy + 4)],
                ],
                ink,
                1,
            ),
        ],
        "lab" => vec![graph_mark(rect, ink)],
        _ => vec![
            strokes(
                rect,
                &[
                    vec![(cx - 7, cy - 4), (cx + 7, cy - 4)],
                    vec![(cx - 7, cy + 3), (cx + 7, cy + 3)],
                ],
                ink,
                1,
            ),
            dot(cx - 2, cy - 6, 4, ink),
            dot(cx + 2, cy + 1, 4, ink),
        ],
    }
}

/// A pill: a rounded box with a status light and a label.
fn pill(rect: Rect, tag: &str, light: Color, text: &str, palette: Palette) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![
            dot(12, rect.h / 2 - 3, 7, light),
            chrome_label(
                rect,
                28,
                rect.w.saturating_sub(38),
                text,
                FONT_BODY,
                palette.ink,
            ),
        ])
        .with_tag(tag.to_string())
        .with_style(
            BoxStyle::filled(palette.raised)
                .with_corner_radius(8)
                .with_border(Border::new(palette.outline, 1)),
        )
        .with_layout(absolute(rect)),
    )
}

/// A button: filled when it is the affirmative one, outlined otherwise.
fn button(rect: Rect, tag: &str, text: &str, filled: bool, palette: Palette) -> Scene {
    let (fill, ink, border) = if filled {
        (palette.accent, palette.on_accent, palette.accent)
    } else {
        (palette.panel, palette.ink, palette.outline)
    };
    Scene::Container(
        ContainerNode::new(vec![chrome_label(
            rect,
            14,
            rect.w.saturating_sub(24),
            text,
            FONT_BODY,
            ink,
        )])
        .with_tag(tag.to_string())
        .with_style(
            BoxStyle::filled(fill)
                .with_corner_radius(8)
                .with_border(Border::new(border, 1)),
        )
        .with_layout(absolute(rect)),
    )
}

fn app_bar_scene(state: &ShellState, palette: Palette) -> Scene {
    let bar = Rect::new(0, 0, win_w(), APP_BAR_H);
    let mut children = vec![
        dot(16, 18, 16, palette.accent),
        chrome_label(bar, 42, 118, "Analyzer", FONT_TITLE, palette.ink),
    ];
    for (n, tab) in spec::VIEW_TABS.iter().enumerate() {
        let name = tab.title;
        let chip = BarChip::Tab(n);
        let on = state.tab.get() == name;
        children.push(Scene::Container(
            ContainerNode::new(vec![chrome_label(
                chip.rect(),
                14,
                chip.rect().w.saturating_sub(20),
                name,
                FONT_BODY,
                if on { palette.ink } else { palette.muted },
            )])
            .with_tag(chip.tag())
            .with_style(
                BoxStyle::filled(if on { palette.high } else { palette.panel })
                    .with_corner_radius(8),
            )
            .with_layout(absolute(chip.rect())),
        ));
    }
    children.push(pill(
        BarChip::Source.rect(),
        &BarChip::Source.tag(),
        rgb(0x35_C0_8B),
        &state.source.get(),
        palette,
    ));
    let capturing = state.capturing.get();
    children.push(pill(
        BarChip::Capture.rect(),
        &BarChip::Capture.tag(),
        if capturing {
            palette.accent_fg
        } else {
            palette.muted
        },
        if capturing { spec::TRANSPORT } else { "Paused" },
        palette,
    ));
    children.push(chrome_label(
        bar,
        842,
        92,
        &transport_word(state.clock.status(), capturing),
        FONT_SMALL,
        palette.muted,
    ));
    // The rate readout: what a capture tool is counting while it runs.
    children.push(chrome_label(
        bar,
        938,
        96,
        spec::RATE,
        FONT_SMALL,
        palette.muted,
    ));
    // The search box says which of its two states it is in; a box that looked
    // the same either way would leave a person typing into the board.
    let searching = state.searching.get();
    children.push(search_chip(state, palette, searching));
    keyboard_stop(
        Scene::Container(
            ContainerNode::new(children)
                .with_tag("shell.appbar")
                .with_style(BoxStyle::filled(palette.panel))
                .with_layout(absolute(Rect::new(0, 0, win_w(), APP_BAR_H))),
        ),
        "shell.appbar",
        &state.at(),
    )
}

/// The application bar's search field: the hint when empty, the query when not,
/// and a caret while it has the keys.
///
/// Split out at R1696 because the bar grew a line past the hundred this project
/// allows a function, and the search field is the part of it with its own
/// three-way state — which is the one worth having a name.
fn search_chip(state: &ShellState, palette: Palette, searching: bool) -> Scene {
    let search = state.search.get();
    Scene::Container(
        ContainerNode::new(vec![chrome_label(
            BarChip::Search.rect(),
            12,
            BarChip::Search.rect().w.saturating_sub(20),
            &if searching {
                format!("{search}|")
            } else if search.is_empty() {
                spec::SEARCH_HINT.to_string()
            } else {
                search
            },
            FONT_BODY,
            if searching {
                palette.ink
            } else {
                palette.muted
            },
        )])
        .with_tag(BarChip::Search.tag())
        .with_style(
            BoxStyle::filled(palette.raised)
                .with_corner_radius(8)
                .with_border(Border::new(
                    if searching {
                        palette.accent_fg
                    } else {
                        palette.outline
                    },
                    1,
                )),
        )
        .with_layout(absolute(BarChip::Search.rect())),
    )
}

fn sub_bar_scene(state: &ShellState, palette: Palette) -> Scene {
    let placed = state.placed().len();
    let preset = SubChip::Preset.rect();
    let children = vec![
        Scene::Container(
            ContainerNode::new(vec![
                chrome_label(
                    preset,
                    12,
                    preset.w.saturating_sub(38),
                    &state.preset.get(),
                    FONT_TITLE,
                    palette.ink,
                ),
                // The fold arrow shares the chip's line with the name beside
                // it. Both were hand-placed (`y: 8` against `y: 13`) and landed
                // a pixel apart, which is the pair `containment::uncentred`
                // reported here.
                strokes(
                    pinion_core::containment::band_in(chrome_seat(preset), preset.w - 26, 12, 8),
                    &[vec![(0, 0), (5, 5), (10, 0)]],
                    palette.muted,
                    1,
                ),
            ])
            .with_tag(SubChip::Preset.tag())
            .with_style(BoxStyle::filled(palette.panel).with_corner_radius(8))
            .with_layout(absolute(preset)),
        ),
        // ★★★★★ R1761 — ADDRESSABLE. The reference's layout bar states how many
        // widgets are placed beside the preset that placed them, and this build
        // painted it as loose ink: measured over the wire before this round,
        // `shell.subbar.` held three parts where the reference draws four, and
        // the missing one was the only one that is not a control. A part a
        // reader can see and nothing can name is a part no specification
        // reaches.
        cell(
            "shell.subbar.count".to_owned(),
            &format!("{placed} widgets placed"),
            Rect::new(preset.x + preset.w + 14, preset.y + 8, 220, 16),
            FONT_BODY,
            palette.muted,
            TextOverflow::Ellipsis,
        )
        // ★ R1761 — addressable and NOT a second voice: the bar announces this
        // count as its own readout, so the mark declares itself part of that
        // announcement. Being tagged is what lets a specification name it;
        // being `part_of` is what stops a reader hearing the same number twice
        // and what keeps it out of the population of things a press must
        // reach, since it is not a control.
        .silenced(Silence::part_of("shell.subbar")),
        button(
            SubChip::EditLayout.rect(),
            SubChip::EditLayout.tag(),
            if state.editing.get() {
                "Done"
            } else {
                spec::BOARD_VERBS[0]
            },
            state.editing.get(),
            palette,
        ),
        button(
            SubChip::AddWidget.rect(),
            SubChip::AddWidget.tag(),
            spec::BOARD_VERBS[1],
            true,
            palette,
        ),
    ];
    keyboard_stop(
        Scene::Container(
            ContainerNode::new(children)
                .with_tag("shell.subbar")
                .with_style(BoxStyle::filled(palette.canvas))
                .with_layout(absolute(Rect::new(
                    RAIL_W,
                    APP_BAR_H,
                    win_w() - RAIL_W - palette_room(),
                    SUB_BAR_H,
                ))),
        ),
        "shell.subbar",
        &state.at(),
    )
}

/// The saved-layout menu: a **top-level popup**, painted in window space — the
/// same space [`preset_item_rect`] gives the hit test.
/// ★★★★★ R2021 — **the panel a card's settings control opens.**
///
/// The defect this repays, measured before the round: pressing the gear on a
/// card header toggled `config_open` and **nothing anywhere drew it**. Seven
/// places touched that signal and not one was a painter, so the press moved
/// state, a message said the settings had opened, and the screen did not
/// change. Every gate on this board stayed green — the flag is on the wire, so
/// it was observable — which is the shape this tree keeps meeting: the
/// declaration is there and the pixels are not.
///
/// Its rows are the card's DECLARED settings ([`spec::CARD_SETTINGS`]), and each
/// of those names the verb it drives, so a control that does nothing cannot be
/// written down. A card with none says so in a sentence rather than opening
/// onto an empty box, because *this card has no settings yet* and *this control
/// is broken* are different things and a reader is owed the difference.
fn card_config_scene(state: &ShellState, palette: Palette) -> Vec<Scene> {
    let Some((card, _, panel)) = card_config_panel(state) else {
        return Vec::new();
    };
    let theme = use_theme(THEME_TAG).theme_animated();
    let id = card.id().as_str();
    let settings = spec::card_settings_of(kind_of(id));
    let inner = panel.w.saturating_sub(CFG_PAD * 2);
    let head_h = pinion_core::containment::line_box(FONT_BODY);
    let gist_h = pinion_core::containment::line_box(FONT_TINY);
    let mut children = vec![
        label(
            "Widget settings",
            Rect::new(CFG_PAD, CFG_PAD, inner, head_h),
            FONT_BODY,
            palette.ink,
        ),
        label(
            // The prototype's own line: it names the scope of what is being
            // changed, which is the fact a person needs before touching it —
            // this is THIS card's copy of the setting, not the tool's.
            &format!("Per-instance \u{b7} {}", card.title()),
            Rect::new(CFG_PAD, CFG_PAD + CFG_HEAD_H, inner, gist_h),
            FONT_TINY,
            palette.muted,
        ),
    ];
    if settings.is_empty() {
        children.push(label(
            "No settings yet for this widget",
            Rect::new(
                CFG_PAD,
                CFG_PAD + CFG_HEAD_H + CFG_GIST_H + CFG_PAD,
                inner,
                gist_h,
            ),
            FONT_TINY,
            palette.muted,
        ));
    }
    for (n, setting) in settings.iter().enumerate() {
        let valued = Valued::Card {
            card: id.to_owned(),
            setting,
        };
        let (caption, seat) = card_config_row_rects(panel, n);
        children.push(label(
            &setting.label.to_uppercase(),
            Rect::new(
                caption.x,
                caption.y,
                caption.w,
                gist_h.min(caption.h.max(gist_h)),
            ),
            FONT_TINY,
            palette.muted,
        ));
        // The framework's collapsed chooser, not a box with a word in it. The
        // preferences page's rows are the first consumer and this is the
        // second; hand-rolling it here is the class R1673 measured on a sibling
        // screen, where a switch was drawn as a track with no knob.
        children.push(chooser::view_collapsed(
            &chooser::ChooserTags {
                control: valued.control_tag(),
                shown: format!("{}.shown.{}", valued.tag_prefix(), valued.roster_key()),
                arrow: format!("{}.arrow.{}", valued.tag_prefix(), valued.roster_key()),
            },
            &valued.read(state),
            seat,
            (0, 0),
            BoxStyle::filled(palette.canvas)
                .with_corner_radius(8)
                .with_border(Border::new(palette.outline, 1)),
            &theme,
        ));
    }
    vec![Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("card.{id}.config"))
            .with_style(
                BoxStyle::filled(palette.panel)
                    .with_corner_radius(11)
                    .with_border(Border::new(palette.outline, 1)),
            )
            .with_layout(absolute(panel)),
    )]
}

fn preset_menu_scene(state: &ShellState, palette: Palette) -> Scene {
    let names: Vec<String> = state.presets.borrow().names();
    let rows = u(names.len()) + 1;
    let first = preset_item_rect(0);
    let panel = Rect::new(first.x - 8, first.y - 30, first.w + 16, rows * 34 + 38);
    let mut children = vec![label(
        "SAVED LAYOUTS",
        Rect::new(14, 10, panel.w - 24, 14),
        FONT_TINY,
        palette.muted,
    )];
    let row_local = |row: Rect| Rect::new(row.x - panel.x, row.y - panel.y, row.w, row.h);
    // ★★★★★ R1894 — the label's box is what a 12px face NEEDS, asked of the
    // framework rather than written down.
    //
    // It was `y: 7, h: 16`, and 16 is three short of `line_box(12)`. Nothing
    // noticed while the menu had one row; adding the canon's other three made
    // the short-box gate go 79 -> 82, naming exactly the three new rows. ⇒ ★a
    // per-row defect is invisible at one row, and the round that adds rows is
    // the round that owes the repair — raising the budget instead would have
    // banked the defect three times over.
    let text_h = pinion_core::containment::line_box(FONT_BODY);
    let text_y = (PRESET_ROW_H.saturating_sub(text_h)) / 2;
    for (n, name) in names.iter().enumerate() {
        let row = preset_item_rect(u(n));
        let on = &state.preset.get() == name;
        children.push(Scene::Container(
            ContainerNode::new(vec![label(
                name,
                Rect::new(12, text_y, row.w.saturating_sub(20), text_h),
                FONT_BODY,
                if on { palette.accent_fg } else { palette.ink },
            )])
            .with_tag(format!("shell.preset.item.{n}"))
            .with_style(
                BoxStyle::filled(if on { palette.high } else { palette.raised })
                    .with_corner_radius(6),
            )
            .with_layout(absolute(row_local(row))),
        ));
    }
    let save = preset_item_rect(u(names.len()));
    children.push(Scene::Container(
        ContainerNode::new(vec![label(
            "+  Save current layout",
            Rect::new(12, text_y, save.w.saturating_sub(20), text_h),
            FONT_BODY,
            palette.accent_fg,
        )])
        .with_tag(format!("shell.preset.item.{}", names.len()))
        .with_style(BoxStyle::filled(palette.raised).with_corner_radius(6))
        .with_layout(absolute(row_local(save))),
    ));
    Scene::Container(
        ContainerNode::new(children)
            .with_tag("shell.preset.menu")
            .with_style(
                BoxStyle::filled(palette.panel)
                    .with_corner_radius(10)
                    .with_border(Border::new(palette.outline, 1)),
            )
            .with_layout(absolute(panel)),
    )
}

fn rail_scene(state: &ShellState, palette: Palette) -> Scene {
    let mut entries = Vec::new();
    let nav = state.at();
    for (n, seat) in spec::RAIL.iter().enumerate() {
        let key = seat.key;
        let rect = rail_rect(u(n));
        let on = nav == key;
        let ink = if on { palette.accent_fg } else { palette.muted };
        // R1668 — a seat this application cannot take you to is DECLARED
        // unavailable rather than painted grey by hand. The declaration is what
        // makes it inert to the pointer, fades its ink, announces it to a screen
        // reader and puts the reason on `scene/disabled`; a hand-picked grey
        // would do only the last of those, in a way nothing can check.
        //
        // ★ R1695 — the reason comes from the ROSTER, so the seat's paint, its
        // refusal and its accessibility node are one fact. Three of these were
        // painted live and refused nothing until this round.
        let layout = state
            .roster()
            .get(key)
            .and_then(|d| d.standing.why())
            .map_or_else(
                || absolute(rect),
                |why| absolute(rect).with_unavailable(why.clone()),
            );
        entries.push(Scene::Container(
            ContainerNode::new(rail_mark(key, local(rect), ink))
                .with_tag(format!("shell.rail.{key}"))
                .with_style(
                    BoxStyle::filled(if on { palette.high } else { palette.panel })
                        .with_corner_radius(8),
                )
                .with_layout(layout),
        ));
    }
    entries.push(Scene::Container(
        ContainerNode::new(vec![label(
            ACCOUNT_INITIALS,
            Rect::new(8, 9, 24, 14),
            FONT_TINY,
            palette.on_accent,
        )])
        .with_tag("shell.rail.account")
        .with_style(BoxStyle::filled(palette.accent).with_corner_radius(16))
        // ★ R1864 — anchored to the RAIL's own bottom rather than to the
        // window's. The two were the same number until the status band took a
        // strip of the window away from the rail; a chip placed from the window
        // would now hang below the panel that holds it.
        .with_layout(absolute(Rect::new(
            10,
            rail_panel_rect().h.saturating_sub(46),
            32,
            32,
        ))),
    ));
    keyboard_stop(
        Scene::Container(
            ContainerNode::new(entries)
                .with_tag("shell.rail")
                .with_style(BoxStyle::filled(palette.panel))
                .with_layout(absolute(rail_panel_rect())),
        ),
        "shell.rail",
        &nav,
    )
}

/// The Settings destination's page, region-local.
///
/// ★★ R1695 — the second page, and what makes the region worth building: a
/// paged region with one page proves nothing about paging.
fn settings_scene(state: &ShellState, palette: Palette, region: Rect) -> Vec<Scene> {
    let col = settings_col(region);
    // ★★★★★ R1762 — the page says what it is, which the reference's does and
    // this one did not: a reader arriving here was given four unlabelled cards.
    let mut out = vec![
        cell(
            "shell.settings.head.title".to_owned(),
            spec::SETTINGS_HEAD.0,
            Rect::new(col.x, col.y, col.w, 24),
            FONT_TITLE,
            palette.ink,
            TextOverflow::Ellipsis,
        )
        // These words ARE the page region's accessible name; saying them again
        // would be one fact in two voices.
        .silenced(Silence::name_of("shell.canvas")),
        cell(
            "shell.settings.head.gist".to_owned(),
            spec::SETTINGS_HEAD.1,
            Rect::new(col.x, col.y + 26, col.w, 16),
            FONT_BODY,
            palette.muted,
            TextOverflow::Ellipsis,
        )
        .silenced(Silence::part_of("shell.canvas")),
    ];
    // ★★★★★ R1762 — the three facts the page closes with. The reference's own
    // footer, and the one place either screen says which build a reader is
    // looking at — the fact a person filing a defect is asked for first.
    let last = settings_group_rect(region, spec::OPTION_GROUPS[spec::OPTION_GROUPS.len() - 1].0);
    out.push(cell(
        "shell.settings.build".to_owned(),
        &spec::BUILD_STRIP.join(" \u{00b7} "),
        Rect::new(col.x, last.y + last.h + SET_GROUP_GAP, col.w, 16),
        FONT_SMALL,
        palette.muted,
        TextOverflow::Ellipsis,
    ));
    // ★ R1762 — and it is ANNOUNCED rather than silent: which build a reader is
    // looking at is not decoration, it is the first thing a person filing a
    // defect is asked for, and a strip only a sighted reader can get it from is
    // a strip half the readers do not have.
    for (key, heading) in spec::OPTION_GROUPS {
        let card = settings_group_rect(region, key);
        // ★★★★★ R1762 — ADDRESSABLE. Four group headings a reader reads and no
        // specification could name: measured at R1761, the settings page's
        // paint held rows, switches and buttons, and the four words that say
        // what each card is were loose ink.
        out.push(
            cell(
                format!("shell.settings.head.{key}"),
                heading,
                Rect::new(col.x, card.y - SET_HEAD_H - SET_HEAD_GAP, col.w, SET_HEAD_H),
                FONT_SMALL,
                palette.muted,
                TextOverflow::Ellipsis,
            )
            // The heading IS the group's accessible name, so it is addressable
            // for a specification and silent for a reader.
            .silenced(Silence::name_of(format!("shell.settings.group.{key}"))),
        );
        let rows = match key {
            "keys" => settings_key_rows(palette, region),
            "appearance" => settings_theme_row(state, palette, region),
            // ★★★★★ R1762 — the capture group opens with the reference's two
            // VALUE rows and then its switches, in that order. They are laid
            // first so the switches below them start where the reference's do.
            "capture" => {
                let mut rows = settings_value_rows(state, palette, region);
                rows.extend(settings_option_rows(state, palette, region, key));
                rows
            }
            // ★★★★★ R1762 — the decode group closes with the reference's
            // payload-format row, below its two switches.
            "decode" => {
                let mut rows = settings_option_rows(state, palette, region, key);
                rows.extend(settings_plugin_row(palette, region));
                rows
            }
            group => settings_option_rows(state, palette, region, group),
        };
        out.push(Scene::Container(
            ContainerNode::new(rows)
                .with_tag(format!("shell.settings.group.{key}"))
                .with_style(
                    BoxStyle::filled(palette.panel)
                        .with_corner_radius(12)
                        .with_border(Border::new(palette.outline, 1)),
                )
                .with_layout(absolute(card)),
        ));
    }
    out
}

/// A row's title and the sentence under it, in the card's own space.
///
/// Silenced as `part_of` the control it names: the switch beside it takes its
/// accessible name from this text, so a reader who heard both would hear the
/// row twice. `named` is that control's **tag** — a silence that points at
/// prose is a silence pointing at nothing, which is what `dangling` counts and
/// what the first draft of this page produced seven of.
fn settings_text(
    key: &str,
    title: &str,
    gist: &str,
    row: Rect,
    palette: Palette,
    named: String,
) -> Scene {
    let inner = Rect::new(
        SET_ROW_PAD,
        row.y,
        row.w.saturating_sub(SET_ROW_PAD * 2),
        row.h,
    );
    Scene::Container(
        ContainerNode::new(vec![
            label(
                title,
                Rect::new(0, 15, inner.w.saturating_sub(SET_CTRL_W), 16),
                FONT_TITLE,
                palette.ink,
            ),
            label(
                gist,
                Rect::new(0, 34, inner.w.saturating_sub(SET_CTRL_W), 15),
                FONT_BODY,
                palette.muted,
            ),
        ])
        .with_tag(format!("shell.settings.row.{key}"))
        .with_layout(absolute(inner)),
    )
    .silenced(Silence::part_of(named))
}

/// The switch rows of one group.
fn settings_option_rows(
    state: &ShellState,
    palette: Palette,
    region: Rect,
    group: &str,
) -> Vec<Scene> {
    let on = state.options.get();
    let theme = use_theme(THEME_TAG).theme_animated();
    let card = settings_group_rect(region, group);
    let mut out = Vec::new();
    // Two indices, and both are load-bearing: `n` is the row's place in this
    // group's card and `index` its place in the specification, which is what the
    // value array is keyed by. Collapsing them would work only while every group
    // held every option.
    // ★ R1762 — the switches start below whatever value rows the group opens
    // with, and the offset is derived from the same count the card's height is.
    let first = settings_value_count(group);
    for (n, (index, option)) in spec::OPTIONS
        .iter()
        .enumerate()
        .filter(|(_, option)| option.group == group)
        .enumerate()
    {
        let row = Rect::new(0, (first + u(n)) * SET_ROW_H, card.w, SET_ROW_H);
        out.push(settings_text(
            option.key,
            option.title,
            option.gist,
            row,
            palette,
            format!("shell.settings.option.{}", option.key),
        ));
        let seat = settings_ctrl_rect(row, 64);
        out.push(Scene::Container(
            ContainerNode::new(vec![switch::view_switch(
                format!("shell.settings.option.{}", option.key),
                ToggleState::Idle,
                on[index],
                &theme,
                &SwitchStyle::m3(),
                option.title,
                // R1837 — it announces itself: this switch IS the control on
                // the settings row, named by the option it turns on.
                None,
            )])
            .with_layout(absolute(seat)),
        ));
    }
    out
}

/// ★★★★★ R1762 — the capture group's **value rows**: a word out of a roster,
/// and the chevron that opens it.
///
/// The control is `pinion_widget_paint::chooser`, not a box with a word in it.
/// Hand-rolling it is the class R1673 measured on a sibling screen — a switch
/// drawn as a track with no knob — and this shell already carries one instance
/// of it in its own preset menu, so a second would have made it a habit rather
/// than an oversight. The lift that made the control reachable from here is
/// this round's framework half.
///
/// The roster is **not** painted here: it belongs over everything, in window
/// space, which is where [`settings_roster_scene`] puts it. A popup drawn
/// inside the card that opens it is clipped by that card — the R1672 lesson
/// this screen already paid for once with its preset menu.
fn settings_value_rows(state: &ShellState, palette: Palette, region: Rect) -> Vec<Scene> {
    let theme = use_theme(THEME_TAG).theme_animated();
    let card = settings_group_rect(region, "capture");
    let mut out = Vec::new();
    for (n, value_row) in spec::VALUE_ROWS.iter().enumerate() {
        let row = Rect::new(0, u(n) * SET_ROW_H, card.w, SET_ROW_H);
        out.push(settings_text(
            value_row.key,
            value_row.title,
            value_row.gist,
            row,
            palette,
            settings_choose_tag(value_row.key),
        ));
        let seat = settings_ctrl_rect(row, SET_VALUE_W);
        out.push(chooser::view_collapsed(
            &chooser::ChooserTags {
                control: settings_choose_tag(value_row.key),
                shown: format!("shell.settings.shown.{}", value_row.key),
                arrow: format!("shell.settings.arrow.{}", value_row.key),
            },
            &settings_value_of(state, value_row.key),
            seat,
            (0, 0),
            BoxStyle::filled(palette.canvas)
                .with_corner_radius(8)
                .with_border(Border::new(palette.outline, 1)),
            &theme,
        ));
    }
    out
}

/// ★★★★★ R1762 — the decode group's payload-format row: the formats this build
/// applies, as the chips the reference lists them in.
///
/// Every chip is ON and none of them is pressable, which is the reference's own
/// row: it states what the decoder does rather than offering a choice. They are
/// addressed all the same, because a reader sees them and a specification that
/// cannot name them cannot check that both are there.
fn settings_plugin_row(palette: Palette, region: Rect) -> Vec<Scene> {
    let (key, title, gist) = spec::PLUGIN_ROW;
    let card = settings_group_rect(region, "decode");
    let n = u(spec::OPTIONS.iter().filter(|o| o.group == "decode").count());
    let row = Rect::new(0, n * SET_ROW_H, card.w, SET_ROW_H);
    let seat = settings_ctrl_rect(row, SET_PLUGIN_W);
    // The announced thing on this row is the CONTROL, exactly as on a switch
    // row: the title and the sentence take their voice from it, so a reader
    // hears the row once. Here the control is the chip seat, which is the only
    // node that can carry what the formats are.
    let seat_tag = format!("shell.settings.row.{key}.chips");
    let mut out = vec![settings_text(key, title, gist, row, palette, seat_tag)];
    let mut chips = Vec::new();
    for (i, word) in spec::PLUGINS.iter().enumerate() {
        let w = (SET_PLUGIN_W.saturating_sub(SEG_PAD * 2) - SEG_PAD) / u(spec::PLUGINS.len());
        chips.push(Scene::Container(
            ContainerNode::new(vec![label(
                word,
                Rect::new(8, 6, w.saturating_sub(16), 14),
                FONT_SMALL,
                palette.accent_fg,
            )])
            .with_tag(format!("shell.settings.plugin.{word}"))
            .with_style(
                BoxStyle::filled(palette.high)
                    .with_corner_radius(6)
                    .with_border(Border::new(palette.accent_fg, 1)),
            )
            .with_layout(
                absolute(Rect::new(
                    SEG_PAD + u(i) * (w + SEG_PAD),
                    SEG_PAD,
                    w,
                    SEG_CHIP_H,
                ))
                // Each chip's word is in the row's announced value: the row
                // states what the decoder applies, and a reader hearing every
                // chip separately would hear the same sentence twice.
                .with_silence(Silence::part_of(format!("shell.settings.row.{key}.chips"))),
            ),
        ));
    }
    out.push(Scene::Container(
        ContainerNode::new(chips)
            .with_tag(format!("shell.settings.row.{key}.chips"))
            .with_layout(absolute(Rect::new(
                seat.x,
                row.y + (SET_ROW_H.saturating_sub(SEG_CHIP_H + SEG_PAD * 2)) / 2,
                seat.w,
                SEG_CHIP_H + SEG_PAD * 2,
            ))),
    ));
    out
}

/// ★★★★★ R2021 — **what a roster is a roster OVER.**
///
/// One `picking` signal holds the open roster and until this round the four
/// things a roster needs — its title, its words, what it is holding, and where
/// to write a chosen word — were four functions keyed by a `&str`, each ending
/// in a `panic!` for a key it did not know. That is a partial function per
/// question, which is three chances for a new consumer to be forgotten in one
/// of them and answered in the others; and this round IS that new consumer,
/// because a card's own settings row is a value row on a different page.
///
/// A value instead. The questions become methods, the compiler is what says a
/// new arm answered all of them, and the key a roster is remembered by is
/// derived from the value rather than being a string the four functions each
/// re-interpret. The three `panic!`s are gone — not by being caught, but by
/// there being no unparsed key left to reach one.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Valued {
    /// A row in the preferences page's capture group.
    Preference(&'static spec::ValueRowSpec),
    /// ★ A setting a card offers under its own header: which card is showing
    /// it, and which of that kind's declared settings it is.
    ///
    /// The card travels as an id rather than as a reference, for the reason
    /// [`Hit::Tab`] carries one: the press stores it, the release acts on it,
    /// and in between the board can have changed.
    Card {
        card: String,
        setting: &'static spec::CardSettingSpec,
    },
}

impl Valued {
    /// The key a roster is remembered by while it is open.
    ///
    /// A card's is scoped by the card, because two cards of one kind offer the
    /// same setting and a bare `severity` could not say whose roster is up.
    fn key(&self) -> String {
        match self {
            Self::Preference(row) => row.key.to_owned(),
            Self::Card { card, setting } => format!("card.{card}.{}", setting.key),
        }
    }

    /// Read a key back into the thing it names.
    ///
    /// `None` for a key nothing declares, which is what makes every caller's
    /// handling of an unknown key a decision rather than a crash.
    fn from_key(key: &str) -> Option<Self> {
        if let Some(row) = spec::VALUE_ROWS.iter().find(|row| row.key == key) {
            return Some(Self::Preference(row));
        }
        let rest = key.strip_prefix("card.")?;
        // A card id carries no dot (`alarms#6`), so the LAST dot separates the
        // card from the setting — stated rather than assumed, because the
        // setting keys are single words and the split has to survive a card id
        // that grows one.
        let (card, field) = rest.rsplit_once('.')?;
        let kind = kind_of(card);
        let setting = spec::card_settings_of(kind)
            .into_iter()
            .find(|s| s.key == field)?;
        Some(Self::Card {
            card: card.to_owned(),
            setting,
        })
    }

    /// What a reader calls it — the words they hear when it moves.
    fn title(&self) -> &'static str {
        match self {
            Self::Preference(row) => row.title,
            Self::Card { setting, .. } => setting.label,
        }
    }

    /// Which of the tool's settings this row is a control over.
    const fn drives(&self) -> spec::Drives {
        match self {
            Self::Preference(row) => row.drives,
            Self::Card { setting, .. } => setting.drives,
        }
    }

    /// What it may be set to, in the order the roster lists them.
    ///
    /// ⚠ Matched on [`spec::Drives`] with **no wildcard arm**: a fallback here
    /// would give a setting added tomorrow the capture sources, and a roster
    /// offering the wrong words is a control that works and does the wrong
    /// thing — which is worse than one that refuses.
    fn options(&self) -> Vec<String> {
        match self.drives() {
            spec::Drives::CaptureSource => SOURCES.iter().map(|s| (*s).to_owned()).collect(),
            spec::Drives::Retention => spec::RETENTIONS.iter().map(|s| (*s).to_owned()).collect(),
            spec::Drives::AlarmFloor => ALARM_FLOORS.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    /// What it is holding right now.
    ///
    /// Read from the state the value actually drives rather than from a store
    /// beside it, so the row cannot show a word the tool is not using — which
    /// is precisely the defect the behaviour prototype ships, where the chosen
    /// severity is written into a per-card map nothing reads.
    fn read(&self, state: &ShellState) -> String {
        match self.drives() {
            spec::Drives::CaptureSource => state.source.get(),
            spec::Drives::Retention => state.retention.get(),
            // `all` is the absence of a floor, which is the same statement the
            // verb's own argument makes — one word, not a blank.
            spec::Drives::AlarmFloor => state
                .alarm_floor
                .get()
                .unwrap_or_else(|| ALARM_FLOORS[0].to_owned()),
        }
    }

    /// Take `word` and put it where this row's value lives.
    ///
    /// ★★★★★ R2021 — **through the same function the wire calls**, never
    /// beside it. The pointer and the client therefore cannot come to mean
    /// different things by one setting, which is the shape R1851 gave the
    /// alarm feed's column headers (`Hit::AlarmColumn` reaches `sort_alarms`)
    /// and the shape this row now has: a chosen severity goes through
    /// [`ShellOracle::filter_alarms`], refusal and announcement included.
    ///
    /// The two preferences write their own signals because they have no verb —
    /// the wire reaches them by writing the slot instead, which is the split
    /// [`spec::OPERATIONS`]' own documentation draws.
    fn write(&self, state: &Rc<ShellState>, word: &str) {
        match self.drives() {
            spec::Drives::CaptureSource => {
                state.source.set(word.to_owned());
                state.say(Utterance::done(format!("{} {word}", self.title())));
            }
            spec::Drives::Retention => {
                state.retention.set(word.to_owned());
                state.say(Utterance::done(format!("{} {word}", self.title())));
            }
            spec::Drives::AlarmFloor => {
                if let Err(why) = ShellOracle::filter_alarms(state, word) {
                    state.say(Utterance::refused(&why));
                }
            }
        }
    }

    /// Which destination this row lives on, so a roster cannot survive
    /// navigating away from the page that opened it.
    fn page(&self) -> &'static str {
        match self {
            Self::Preference(_) => "settings",
            Self::Card { .. } => "dashboard",
        }
    }

    /// What the row's parts are addressed UNDER.
    ///
    /// The chooser lays its own suffixes beneath this, so the three tags below
    /// are the framework's spelling with this in front rather than three
    /// strings this file composes — which is what lets a driver press the name
    /// the paint published without either side holding a table.
    /// ⚠ `config` and not `settings` for a card's: `card.<id>.settings` is
    /// already taken — it is the header control that OPENS this panel, named by
    /// [`spec::CARD_CHROME`]. Two things under one tag is the defect
    /// [[debt-two-holders-of-one-name-are-refused-by-every-verb-and-reported-by-nothing]]
    /// records, and the state signal these rows are shown by is `config_open`,
    /// so this is the name the rest of the file already uses for them.
    fn tag_prefix(&self) -> String {
        match self {
            Self::Preference(_) => "shell.settings".to_owned(),
            Self::Card { card, .. } => format!("card.{card}.config"),
        }
    }

    /// The key the chooser lays its parts under, within the prefix.
    const fn roster_key(&self) -> &'static str {
        match self {
            Self::Preference(row) => row.key,
            Self::Card { setting, .. } => setting.key,
        }
    }

    /// The tag the collapsed control is addressed by.
    fn control_tag(&self) -> String {
        format!("{}.choose.{}", self.tag_prefix(), self.roster_key())
    }

    /// The tag the open roster is addressed by — the framework's own spelling.
    fn roster_tag(&self) -> String {
        format!("{}.roster.{}", self.tag_prefix(), self.roster_key())
    }

    /// The tag one word of the open roster is addressed by.
    fn option_tag(&self, word: &str) -> String {
        format!("{}.option.{}.{word}", self.tag_prefix(), self.roster_key())
    }
}

/// The tag a value row's collapsed control is addressed by.
fn settings_choose_tag(key: &str) -> String {
    format!("shell.settings.choose.{key}")
}

/// ★★★★★ R1762 — the open roster, in **window** space.
///
/// Empty unless a roster is open and the reader is on the page it belongs to:
/// a popup that survived navigating away would be the class R1695 measured
/// across this whole shell — a page you left still on screen.
///
/// The room it must stay inside is the PAGE REGION, handed to the framework's
/// own geometry rather than decided here. That is the rule
/// `pinion_widget_paint::chooser` keeps from R1732: a surface laid into a
/// region it cannot see the bottom of would open a roster off the end of it.
/// ★★★★★ R2026 — `None` when there is no roster, not an empty container.
///
/// An absent surface is one that is NOT IN THE SCENE, which is the answer this
/// screen already gave for a mark it chose not to paint (R1776: *absent rather
/// than transparent — a mark that is painted invisibly is still in the scene,
/// still in the accessibility tree*). Spelling it as an empty container puts a
/// node in the tree whose only content is its own absence, and every reader
/// then has to INFER what it means: measured at R1971, the reach walk had to
/// grow *a container that holds nothing draws nothing* to keep from reporting
/// four of these per frame as defects, and that inference was wrong on its
/// first try — narrowed to "empty" alone it also excused empty containers that
/// DO have a box, taking three gates red.
fn settings_roster_scene(state: &ShellState, at: &str) -> Option<Scene> {
    if at != "settings" {
        return None;
    }
    let picking = state.picking.borrow();
    let (key, picker) = picking.as_ref()?;
    // ★★★★★ R2021 — and it must be a row of THIS page. One signal now holds the
    // open roster of either page, so a card's roster left open on the board and
    // then navigated away from would otherwise be drawn here — anchored at the
    // first preferences row, because the lookup that finds a row's seat answers
    // `0` for a key it does not hold. A roster over a control that is not on the
    // screen is the class R1695 measured across this whole shell.
    if !matches!(Valued::from_key(key), Some(Valued::Preference(_))) {
        return None;
    }
    let region = page_rect("settings");
    let theme = use_theme(THEME_TAG).theme_animated();
    let roster = chooser::lay_roster(
        key,
        settings_control_rect(region, key),
        picker,
        region,
        SET_OPTION_H,
    );
    Some(chooser::view_roster(
        "shell.settings",
        &roster,
        picker,
        &settings_value_of(state, key),
        (0, 0),
        &theme,
    ))
}

/// ★★★★★ R2021 — the open roster of a **card's** setting, in window space.
///
/// Empty unless the reader is on the board that holds the card, which is the
/// same guard the preferences roster carries: a popup that survived navigating
/// away is a page you left still on the screen.
fn card_roster_scene(state: &ShellState, at: &str) -> Option<Scene> {
    let (valued, picker, roster) = card_roster_box(state)?;
    if at != valued.page() {
        return None;
    }
    let theme = use_theme(THEME_TAG).theme_animated();
    Some(chooser::view_roster(
        &valued.tag_prefix(),
        &roster,
        &picker,
        &valued.read(state),
        (0, 0),
        &theme,
    ))
}

/// Where a value row's collapsed control is, in **window** space.
///
/// One derivation for the paint, the roster's anchor and the hit test — the
/// standing rule on this screen, and the class it has paid for three times.
fn settings_control_rect(region: Rect, key: &str) -> Rect {
    let card = settings_group_rect(region, "capture");
    let n = spec::VALUE_ROWS
        .iter()
        .position(|row| row.key == key)
        .map_or(0, u);
    let row = Rect::new(0, n * SET_ROW_H, card.w, SET_ROW_H);
    let seat = settings_ctrl_rect(row, SET_VALUE_W);
    Rect::new(
        region.x + card.x + seat.x,
        region.y + card.y + seat.y,
        seat.w,
        seat.h,
    )
}

/// What a value row is holding right now.
///
/// Read from the shell's own state rather than from a table of defaults, so the
/// row says what the tool is actually doing — the capture source it shows is
/// the one the application bar shows, which is the whole reason the reference
/// puts it on this page.
///
/// ★ R2021 — a thin wrapper over [`Valued::read`] now. A key nothing declares
/// answers the empty string rather than crashing: this is called from the paint
/// and from the wire, and a roster key that outlived its card is a state the
/// board can genuinely reach.
fn settings_value_of(state: &ShellState, key: &str) -> String {
    Valued::from_key(key).map_or_else(String::new, |valued| valued.read(state))
}

/// ★★★★★ R1762 — what the preferences page's value rows publish: what each
/// holds, what each may hold, and which one's roster is open.
///
/// A client that has to press a rectangle to find out what a control holds is a
/// client reading pixels, which is the reason every other row on that page is
/// on the wire too.
///
/// # Panics
///
/// If asked for a slot the schema does not declare, which is a defect in this
/// pairing rather than a path a client can reach: the census asserts the two
/// agree.
fn settings_slot(state: &ShellState, path: &str) -> String {
    match path {
        "retention" => state.retention.get(),
        "retentions" => spec::RETENTIONS.join(","),
        "picking" => state
            .picking
            .borrow()
            .as_ref()
            .map_or_else(String::new, |(key, _)| key.clone()),
        other => panic!("the schema declares no preferences slot {other:?}"),
    }
}

/// What a value row is called — the words a reader hears when it moves.
fn settings_value_title(key: &str) -> &'static str {
    Valued::from_key(key).map_or("", |valued| valued.title())
}

/// The two key rows, whose affordance is booked for a later release.
fn settings_key_rows(palette: Palette, region: Rect) -> Vec<Scene> {
    let theme = use_theme(THEME_TAG).theme_animated();
    let mut out = Vec::new();
    for (n, key_row) in spec::KEY_ROWS.iter().enumerate() {
        let card = settings_group_rect(region, "keys");
        let row = Rect::new(0, u(n) * SET_ROW_H, card.w, SET_ROW_H);
        out.push(settings_text(
            key_row.key,
            key_row.title,
            key_row.gist,
            row,
            palette,
            format!("shell.settings.key.{}", key_row.key),
        ));
        let seat = settings_ctrl_rect(row, SET_CTRL_W);
        // ★★ R1695 — the framework's BUTTON, not a box with a word in it.
        // Hand-rolling it is the class R1673 measured on the sibling screen,
        // where a switch was drawn as a track with no knob at all; here the
        // hand-rolled version put its label flush against the left border,
        // because centring a label is what a button does and a box does not.
        // Measured on the first draft by ink span: 683 in a box from 682.
        out.push(Scene::Container(
            ContainerNode::new(vec![button::view_button(
                key_row.verb,
                ButtonState::Disabled,
                0.0,
                &ButtonColors::filled_tonal(&theme),
                &ButtonStyle::m3_default(format!("shell.settings.key.{}", key_row.key))
                    .with_corner_radius(8)
                    .with_size(Size::px(seat.w, seat.h))
                    .with_label_font_size_px(FONT_BODY)
                    // ★ R1696 — booked for a later release, so not a Tab stop.
                    // The flag is NOT what makes that true and a counterfactual
                    // proved it: flipping this to `true` changed nothing,
                    // because the row below declares `Unavailable` and R1554's
                    // enumeration returns at a disabled region before it reads
                    // any child's flag (`r1554_a_disabled_region_contributes_
                    // no_tab_stops`). It stays `false` so the declaration and
                    // the guarantee agree, and this comment says which one is
                    // load-bearing — a widget default of `true` under a
                    // structural `no` reads as a control that is one edit away
                    // from being reachable.
                    .with_focusable(false),
            )])
            .with_layout(absolute(seat).with_unavailable(Unavailable::reserved(
                // R2042 — the row that no requirement books says so here too,
                // so the paint, the spoken refusal and the description all
                // carry one sentence rather than three readings of a field.
                key_row.reserved_for.unwrap_or(spec::UNBOOKED),
            ))),
        ));
    }
    out
}

/// The appearance row: one two-way segment over the theme.
fn settings_theme_row(state: &ShellState, palette: Palette, region: Rect) -> Vec<Scene> {
    let card = settings_group_rect(region, "appearance");
    let row = Rect::new(0, 0, card.w, SET_ROW_H);
    let dark = theme_word(&state.theme) == "dark";
    let theme = use_theme(THEME_TAG).theme_animated();
    let seg_w = SEG_W;
    let seat = settings_ctrl_rect(row, seg_w);
    let mut segs = Vec::new();
    for (n, name) in spec::THEMES.iter().enumerate() {
        let on = (n == 0) == dark;
        let w = seg_chip_w();
        // The chosen half is the accent surface and the other is the tonal
        // one — the reference's own two-way appearance segment, painted with
        // the catalogue's button rather than a box and a word.
        let colors = if on {
            ButtonColors::accent(&theme)
        } else {
            ButtonColors::filled_tonal(&theme)
        };
        segs.push(Scene::Container(
            ContainerNode::new(vec![button::view_button(
                name,
                ButtonState::Idle,
                0.0,
                &colors,
                &ButtonStyle::m3_default(format!("shell.settings.theme.{n}"))
                    .with_corner_radius(6)
                    .with_size(Size::px(w, SEG_CHIP_H))
                    .with_label_font_size_px(FONT_BODY),
            )])
            .with_layout(absolute(Rect::new(
                SEG_PAD + u(n) * w,
                SEG_PAD,
                w,
                SEG_CHIP_H,
            ))),
        ));
    }
    vec![
        settings_text(
            "theme",
            spec::THEME_ROW.0,
            spec::THEME_ROW.1,
            row,
            palette,
            // The row names the pair, not either button, so it folds into the
            // group that owns them.
            "shell.settings.group.appearance".to_owned(),
        ),
        Scene::Container(
            ContainerNode::new(segs)
                .with_tag("shell.settings.theme")
                .with_style(BoxStyle::filled(palette.canvas).with_corner_radius(8))
                .with_layout(absolute(Rect::new(
                    seat.x,
                    seat.y + 1,
                    seg_w,
                    SEG_CHIP_H + SEG_PAD * 2,
                ))),
        ),
    ]
}

/// The canvas's dot grid: one pip per cell corner.
///
/// Painted always, faintly — an empty board that looked like an empty panel
/// would not say that things can be placed on it — and brighter while a drag or
/// layout-edit is in flight, which is the reference's aligned overlay.
fn grid_scene(rows: u32, palette: Palette, bright: bool) -> Vec<Scene> {
    let pitch = col_pitch();
    let ink = if bright {
        palette.outline
    } else {
        palette.grid
    };
    let size = if bright { 3 } else { 2 };
    let rows = rows.max(1);
    let mut out = Vec::new();
    for row in 0..=rows {
        for col in 0..=GRID_COLS {
            out.push(dot(
                GAP + col * pitch - size / 2,
                GAP + row * ROW_H - size / 2,
                size,
                ink,
            ));
        }
    }
    if !bright {
        return out;
    }
    // ★★★★★ R1733 — while something is being placed the grid is a PART of the
    // screen, with a name, rather than texture.
    //
    // The reference draws a dashed rectangle per cell only while a placement is
    // in flight (`showOverlay`); this board brightens the dots it always has.
    // Same fact, drawn differently — and it had no tag, so a specification of
    // what a carry puts on screen could not read it back out of the paint. The
    // wrapper exists exactly when the overlay is on, so the part is present
    // when the fact is and absent when it is not.
    // ★ The box is the marks' own extent, not the grid's arithmetic: a mark is
    // centred on its intersection, so the last one in each direction reaches
    // half its size past the last line. The containment gate measured the
    // difference — two pixels, five marks over — on the wrapper's first run.
    vec![Scene::Container(
        ContainerNode::new(out)
            .with_tag("shell.carry.grid")
            .with_layout(absolute(Rect::new(
                0,
                0,
                GAP + GRID_COLS * pitch + size.div_ceil(2),
                GAP + rows * ROW_H + size.div_ceil(2),
            ))),
    )]
}

/// A card's header: grip, status light, title (or a shared place's tab strip),
/// LIVE badge, controls.
fn header_scene(
    card: &Card,
    rect: Rect,
    palette: Palette,
    maximized: bool,
    sharing: &[TileId],
    fore: usize,
) -> Vec<Scene> {
    // ★ R1816 — `HDR_TAIL`, `MIN_TITLE` and `BADGE_W` used to be declared here.
    // They are `CARD_METRICS` now, and the compiler is what said so: after the
    // arithmetic moved to the framework all three became dead code, which is
    // this tree's proof that a lift LEFT NO COPY rather than adding a second
    // one beside the first.
    let id = card.id().as_str();
    // ★★★★★ R1817 — the whole header is the framework's now, skin included.
    //
    // R1816 lifted the arithmetic and left the glyphs here, on the reasoning
    // that they had been chosen once. That deferral was wrong under this work's
    // standing instruction — the framework builds the capability whether or not
    // a second consumer exists, and the deliverable is a crate — and it left
    // the census row saying `app` while the framework already owned everything
    // hard about the thing.
    //
    // What this function keeps is what is genuinely THIS SCREEN'S: which card
    // it is, what its kind colour means, and the words. The give-way order, the
    // rectangles and the marks are `card_header`'s, and `Hit::at` asks the same
    // module where a slot is — so what is drawn is what is pressed without
    // either side holding a second copy.
    let offered = card.chrome().offered();
    // ★ R1900 — the tags and the words a strip needs, built here because both
    // are this screen's: the tag is the occupant's own (see
    // [`card_header::HeaderTab::tag`]) and the label is what a person calls the
    // card, which `label_of` owns.
    let addresses: Vec<String> = sharing
        .iter()
        .map(|member| format!("card.{member}.tab"))
        .collect();
    let words: Vec<String> = sharing
        .iter()
        .map(|member| label_of(member.as_str()))
        .collect();
    let tabs: Vec<card_header::HeaderTab<'_>> = addresses
        .iter()
        .zip(&words)
        .map(|(tag, label)| card_header::HeaderTab {
            tag: tag.as_str(),
            label: label.as_str(),
        })
        .collect();
    card_header::header_scene(
        &format!("card.{id}"),
        rect,
        &card_header::HeaderSpec {
            tabs: &tabs,
            fore,
            offered: &offered,
            ready: card.state().is_ready(),
            restore: maximized,
            title: card.title(),
            badge: "LIVE",
            ink: card_header::HeaderInk {
                title: palette.ink,
                muted: palette.muted,
                accent: palette.accent_fg,
                kind: kind_color(kind_of(id)),
            },
        },
        CARD_METRICS,
    )
}

/// What a card's body paints: its content, or — for **every** not-ready
/// state — the same two things, the sentence and its derived remedy, so the
/// twelve kinds cannot disagree about what an encrypted link offers.
fn body_scene(state: &ShellState, card: &Card, rect: Rect, palette: Palette) -> Vec<Scene> {
    if card.state().is_ready() {
        return ready_body(state, card, rect, palette);
    }
    let mut out = vec![label(
        &state_sentence(card.state()),
        Rect::new(rect.x + 12, rect.y + 10, rect.w.saturating_sub(24), 16),
        FONT_BODY,
        palette.muted,
    )];
    if let Some(remedy) = card.remedy() {
        // A remedy is painted as a control only when the person is the one
        // expected to act. `Wait` is the card's own job and `Nothing` is
        // nobody's, and neither gets a button — the derivation doing the
        // deciding rather than this function.
        let slot = remedy_rect(rect);
        let actionable = remedy.is_actionable();
        out.push(Scene::Container(
            ContainerNode::new(vec![label(
                remedy_label(remedy),
                Rect::new(12, 3, slot.w.saturating_sub(20), 16),
                FONT_BODY,
                if actionable {
                    palette.on_accent
                } else {
                    palette.muted
                },
            )])
            .with_tag(format!("card.{}.remedy", card.id().as_str()))
            .with_style(
                BoxStyle::filled(if actionable {
                    palette.accent
                } else {
                    palette.raised
                })
                .with_corner_radius(6),
            )
            .with_layout(absolute(slot)),
        ));
    }
    out
}

/// A ready card's content — the body the specification gives that kind.
///
/// R1668 replaced a placeholder here. The four placeable widgets are what
/// screen C *is*, and a board of four coloured swatches reproduces the
/// arrangement while reproducing none of the screen: the gate next door
/// compares painted rows against `spec`, and a placeholder has no rows to
/// compare. Each body is drawn from the specification's own table, so a row
/// added there appears here and nowhere is there a second copy to disagree.
/// ★★★★★ R1806 — **the cross-filter the board is under, as the framework
/// reports it.**
///
/// The saved-filter chip has had state since R1721 and exactly one reader: its
/// own chip row. Clicking it lit a chip and changed nothing else on the board,
/// which is the census row `dashboard.t2.4` — *click to cross-filter every
/// linked view* — with **every** meaning one.
///
/// This publishes the lit chip as a
/// [`Selection`](pinion_chart::Selection) into the board's declared
/// [`LinkGroup`](pinion_chart::LinkGroup) and hands back the
/// [`Reach`](pinion_chart::Reach). A card asks the reach what it must apply
/// rather than being handed a window, so "was I part of this?" and "what do I
/// narrow to?" are one question — and a card that is *not* part of it can say
/// why, which is what the refusals carry.
///
/// `None` when no chip is lit: the crossfilter convention that no selection is
/// not the same as an empty one, and every card renders full.
fn cross_filter(state: &ShellState) -> Option<pinion_chart::Reach> {
    let n = state.filter_chip.get()?;
    let (name, _) = spec::FILTER_CHIPS.get(n)?;
    Some(spec::dashboard_links().publish(&pinion_chart::Selection::Category((*name).to_string())))
}

fn ready_body(state: &ShellState, card: &Card, rect: Rect, palette: Palette) -> Vec<Scene> {
    let id = card.id().as_str();
    match kind_of(id) {
        // ★ R1806 — the stream is the card a saved filter actually narrows, and
        // it is a DIFFERENT card from the one clicked. That is the whole of the
        // census sentence: the chip lives on the filter card, and the rows that
        // stop being current live here.
        "packet" => stream_body(state, id, rect, palette),
        "decode" => decode_body(id, rect, palette),
        "keymap" => map_body(id, rect, palette),
        // ★★★★★ R1721 — the state is threaded rather than fetched. The first draft
        // reached for `use_shell_state()` inside the body painter and the RUNNING
        // screen said no: the shell's press path has no Owner scope, so a click on
        // a chip panicked in `use_transport_clock`. The demo found it; no unit test
        // could have, because every one of them runs inside an Owner.
        "filter" => filter_body(state, id, rect, palette),
        // ★ R1797 — the fifth. Everything it draws is DERIVED from one capture
        // record: the bar heights, the three tiles above them and which bars
        // are emphasised all come out of the same samples, so the card cannot
        // state a percentile its own bars contradict. The reference's can, and
        // measurably does.
        "latency" => latency_body(id, rect, palette),
        // ★ R1843 — the sixth, and the first whose tile comes from a crate.
        "health" => health_body(id, rect, palette),
        // ★★★★★ R1851 — the seventh, and the first COMPOSITION that comes from a
        // crate: a sortable column header over a virtualised body, which nothing
        // in this tree had put together before. Every part is the framework's;
        // the row is this screen's, which is the half a data grid cannot draw.
        "alarms" => alarms_body(state, id, rect, palette),
        // A kind with no body painter of its own still reads as content rather
        // than as a gap. Reachable only if the catalogue grows a placeable kind
        // before its body does, which is the moment a placeholder is honest.
        other => placeholder_body(other, id, rect, palette),
    }
}

/// The ink a message type is drawn in.
///
/// Looked up by the type's position in the specification's legend, so a row
/// carrying a type the legend does not list is drawn muted rather than
/// silently taking a colour that means something else. The alternative -- a
/// match on the words -- lets the two lists drift and says nothing when they do.
const TYPE_INKS: [Color; 5] = [
    rgb(0x2D_6C_DF),
    rgb(0xC7_78_00),
    rgb(0x1F_8A_4C),
    rgb(0x8A_5C_F6),
    rgb(0x0E_9A_A7),
];

fn type_ink(word: &str, palette: Palette) -> Color {
    spec::STREAM_TYPES
        .iter()
        .position(|known| *known == word)
        .and_then(|n| TYPE_INKS.get(n).copied())
        .unwrap_or(palette.muted)
}

/// The x offset and **text width** of each stream column inside a body of that
/// width, for the columns that fit.
///
/// Derived from the specification's column table: the one column whose width is
/// `0` takes what the others leave, so a body narrower than the fixed columns
/// gives it nothing rather than wrapping into the next card.
///
/// ★ The gutter is subtracted HERE, and a column with nothing left after it is
/// dropped rather than returned at zero. The first draft returned the cell
/// width and left each caller to write `w - 6`, which underflows the moment a
/// card is one cell wide: a debug panic, and in release a column four billion
/// pixels across. A counterfactual found it -- no swept state had ever been
/// small enough -- and the repair is to make the subtraction impossible to get
/// wrong rather than to write it correctly in four places.
fn stream_columns(width: u32) -> Vec<(&'static str, u32, u32)> {
    const GUTTER: u32 = 6;
    /// The narrowest a column can be and still say anything.
    const FLOOR: u32 = 18;
    let fixed: u32 = spec::STREAM_COLUMNS.iter().map(|(_, w)| *w).sum();
    let flexible = width.saturating_sub(fixed + 24);
    let mut out = Vec::new();
    let mut x = 12;
    let mut left = width.saturating_sub(12);
    for (name, w) in spec::STREAM_COLUMNS {
        let wanted = if *w == 0 { flexible } else { *w };
        // A column takes what it wants or what is left, whichever is smaller,
        // and the ones with nothing left are dropped from the right. The label
        // elides rather than overflowing, so a narrowed column says it was cut
        // instead of painting over its neighbour.
        let cell = wanted.min(left);
        let text = cell.saturating_sub(GUTTER);
        if text < FLOOR {
            break;
        }
        out.push((*name, x, text));
        x += cell;
        left -= cell;
    }
    out
}

/// A label that says so when it does not fit, rather than painting past its
/// box. `Ellipsis` for a value read from the left, `EllipsisStart` for a path
/// whose leaf is what identifies it (R1654's distinction).
fn clipped(text: &str, rect: Rect, px: u32, fg: Color, overflow: TextOverflow) -> Scene {
    Scene::Text(
        TextNode::styled(
            text,
            rect,
            TextStyle::new()
                .with_size_px(px)
                .with_fg(fg)
                .with_overflow(overflow),
        )
        .with_layout(absolute(rect)),
    )
}

/// A label centred **inside the box it was handed**, by declaring the
/// alignment rather than by arithmetic on the caller's side.
///
/// ★★★★★ R1904 — this helper replaces a comment that said it could not exist,
/// and the correction is worth keeping because the comment outlived the reason
/// for it by 209 rounds.
///
/// R1695 measured three chip labels inking at their node's left edge with
/// `TextAlign::Center` asked for, concluded the property was inert on an
/// absolutely placed run, and wrote that down here beside a link to
/// `debt-a-declared-text-alignment-does-nothing-on-an-absolutely-placed-run`.
/// R1780 closed that debt by measuring the discriminator: alignment is applied
/// after `break_all_lines(max_width)`, and `max_width` is the run's OWN
/// rectangle — so a label handed a box its own size is centred in a box its own
/// size, which is where it already was. All three of R1695's cases were that.
/// Two live tests perform the corrected rule:
/// `pinion_text::cache::r1780_an_alignment_moves_a_run_within_the_width_it_was_given`
/// and `pinion_rpc::text_blocks::r1780_the_wire_shows_whether_an_alignment_had_room`.
///
/// ⇒ **the property works wherever the box is wider than the text**, and a
/// person reading the running window found the place where that matters: a
/// byte in an 18-wide band inked 10 wide and flush left, 3 against 9. The
/// comment that stood here is what a reader of that code met first.
///
/// ⚠ **A caller with no room gets nothing**, silently, exactly as R1695
/// measured — which is why the gate for this is
/// `r1904_a_byte_is_centred_in_its_cell_by_ink` and it asserts the room before
/// it asserts the centring. `pinion_widget_paint::button::view_button` still
/// centres with `JustifyContent::Center` on a flex row, and that remains the
/// idiom where a flex row is what the caller has; this is the idiom for a run
/// placed at an exact rectangle, which is what this screen's dense panes are
/// built from.
fn centred(text: &str, rect: Rect, px: u32, fg: Color) -> Scene {
    Scene::Text(
        TextNode::styled(
            text,
            rect,
            TextStyle::new()
                .with_size_px(px)
                .with_fg(fg)
                .with_overflow(TextOverflow::Ellipsis)
                .with_align(TextAlign::Center),
        )
        .with_layout(absolute(rect)),
    )
}

/// ★★★★★ R1694 — [`clipped`], **addressable**.
///
/// A table cell painted without a tag is a value a reader can see and cannot
/// ask about: the row is one box and its cells are its siblings, so the whole
/// row collapses to one run of words. Measured at 6.11.1, a model-driven item
/// view answers a cell query with the cell's own name, its row, its column and
/// its column header — the strong case, and the reason both of this screen's
/// tables are announced cell by cell rather than row by row.
fn cell(tag: String, text: &str, rect: Rect, px: u32, fg: Color, overflow: TextOverflow) -> Scene {
    text_run(
        tag,
        text,
        rect,
        TextStyle::new()
            .with_size_px(px)
            .with_fg(fg)
            .with_overflow(overflow),
    )
}

/// The stem every cell of table card `id` is addressed under.
///
/// ★ R1873 — a stem rather than a `format!` inside the builder, because the
/// gate that judges this family has to name it too, and a family named twice is
/// a family that drifts. The builder below and
/// `painted::r1873_no_grid_run_of_a_table_card_sits_in_a_box_too_short_for_its_face`
/// now share one definition, and a test asserts each builder still starts with
/// its stem.
fn cell_stem(id: &str) -> String {
    format!("card.{id}.cell.")
}

/// The stem every column heading of table card `id` is addressed under.
fn head_cell_stem(id: &str) -> String {
    format!("card.{id}.head.")
}

/// The tag a table card's cell is addressed by.
fn cell_tag(id: &str, row: usize, column: usize) -> String {
    format!("{}{row}_{column}", cell_stem(id))
}

/// The tag a table card's column header is addressed by.
fn head_cell_tag(id: &str, column: usize) -> String {
    format!("{}{column}", head_cell_stem(id))
}

/// One run of a table card's grid: a column heading or a cell, in a box that is
/// a band tall enough for the face it is set in, centred in the **seat** the
/// run owns.
///
/// ★★★★★ R1873 — **the height and the vertical placement are not parameters,
/// and that is the repair rather than a bigger number.** Every grid run on this
/// screen was authored `Rect::new(x, <y>, w, 13)` beside `FONT_TINY`, with a
/// hand-picked `y` that was **2 at three sites, 3 at one and 4 at one** — five
/// sites, three offsets, one height, and nothing relating any of them to the
/// face. [`pinion_core::containment::line_box`] of 10 is **17**, so the box was
/// four pixels short of the reservation everywhere.
///
/// ⚠ Short of the *reservation*, which is deliberately conservative: this is
/// not a claim that a descender was destroyed in every one of them. What the
/// screen-wide shortfall gate reported for this destination alongside the
/// count is that no run was short by more than its slack over the em — the
/// glyph bodies fit and the descenders were the part at risk. The defect being
/// repaired is that **nothing here consulted the face at all**, which is the
/// condition under which the next face change decides it silently.
///
/// Writing `17` at each site would repair the runs and leave the height a
/// number somebody types, which is exactly how it became 13.
///
/// The caller hands the seat — the whole rectangle the run owns in its strip —
/// and gets the band inside it. So a face change moves every grid box on this
/// screen, a row-height change moves them too, and neither can be forgotten;
/// and `px` is named ONCE, where before a box's height and its run's face were
/// two independent arguments that nothing related.
fn grid_cell(
    tag: String,
    text: &str,
    seat: Rect,
    px: u32,
    fg: Color,
    overflow: TextOverflow,
) -> Scene {
    cell(
        tag,
        text,
        pinion_core::containment::line_rect_in(seat, seat.x, seat.w, px),
        px,
        fg,
        overflow,
    )
}

/// ★★★★★ R1876 — one run's box in a decode card's body: a band tall enough for
/// the card's face, centred in the row that holds it.
///
/// A sibling of [`grid_cell`] rather than a second spelling of it. `grid_cell`
/// serves a run that carries a TAG and so returns a whole `Scene`; a decode
/// card's key, value, offset and byte cells are untagged, and what they need is
/// the RECTANGLE. Splitting at that seam is what lets both come from one
/// derivation instead of one of them re-deriving it.
///
/// ⚠ The seat's height and the face are the point, and they are why this is a
/// function and not four calls to
/// [`line_rect_in`](pinion_core::containment::line_rect_in): all four sites
/// were `Rect::new(x, 3, w, 13)` beside `FONT_TINY`, whose `line_box` is
/// **17**, in a 19-pixel row — four pixels short, four times, each site naming
/// the face and the height again. Here the row and the face are named ONCE.
fn decode_band(x: u32, w: u32) -> Rect {
    let seat = Rect::new(x, 0, w, DECODE_ROW_H);
    pinion_core::containment::line_rect_in(seat, x, w, FONT_TINY)
}

/// The message stream: a header row of columns over the opening rows.
fn stream_body(state: &ShellState, id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    const HEAD_H: u32 = STREAM_HEAD_H;
    const ROW_H: u32 = STREAM_ROW_H;
    // ★★★★★ R1806 — the selection this card was reached by, or `None`.
    //
    // Asked of the reach by NAME rather than read off the chip signal, and the
    // difference is the point: this card does not know whether it is linked, it
    // asks. Were its declaration removed from `spec::dashboard_links` the rows
    // would go back to full strength AND
    // `r1806_the_link_declaration_covers_the_placed_board` would name the card
    // — where before this round removing a `.select_x_range` call failed
    // nothing at all.
    let selected = cross_filter(state)
        .as_ref()
        .and_then(|reach| reach.selection_for("packet").cloned());
    let rule = selected.as_ref().and_then(|selection| {
        let name = selection.category()?;
        spec::FILTER_CHIPS
            .iter()
            .position(|(chip, _)| *chip == name)
            .and_then(spec::chip_rule)
    });
    let columns = stream_columns(rect.w);
    let mut out = vec![Scene::Container(
        ContainerNode::new(
            columns
                .iter()
                .enumerate()
                .map(|(c, (name, x, w))| {
                    grid_cell(
                        head_cell_tag(id, c),
                        name,
                        Rect::new(*x, 0, *w, HEAD_H),
                        FONT_TINY,
                        palette.muted,
                        TextOverflow::Ellipsis,
                    )
                })
                .collect(),
        )
        .with_tag(format!("card.{id}.head"))
        .with_style(BoxStyle::filled(palette.raised))
        .with_layout(absolute(Rect::new(rect.x, rect.y, rect.w, HEAD_H))),
    )];
    // ★★★★★ R2022 — the seats, from the one derivation that also tells the
    // accessibility tree how many rows this body has. A row whose bottom would
    // leave the card gets no seat and is not painted at all: the alternative —
    // painting it and letting it land on the card below — is the defect R1656
    // measured on twenty-five surfaces.
    for (n, seat) in stream_seats(rect).iter() {
        let (time, kind, name, len) = spec::STREAM_ROWS[n];
        // A cell per column that fits, in the specification's order, with the
        // row's own values. Zipped rather than indexed: a narrow card drops
        // columns from the right, and indexing would reach past the end.
        let values = [time, kind, name, len];
        // ★ R1806 — a row the active saved filter does not select mutes; it is
        // not dropped. The crossfilter convention this crate documents: the
        // reader must be able to see what fell outside the filter, or narrowing
        // and having no data look the same.
        let outside = rule.is_some_and(|rule| !rule.selects(kind, name));
        let cells = columns
            .iter()
            .zip(values)
            .enumerate()
            .map(|(c, ((column, x, w), value))| {
                let ink = if outside {
                    palette.faded
                } else if *column == "type" {
                    type_ink(value, palette)
                } else if *column == "name" {
                    palette.ink
                } else {
                    palette.muted
                };
                // A resource path is identified by its leaf, so it gives up
                // its head; everything else reads from the left.
                let overflow = if *column == "name" {
                    TextOverflow::EllipsisStart
                } else {
                    TextOverflow::Ellipsis
                };
                grid_cell(
                    cell_tag(id, n, c),
                    value,
                    Rect::new(*x, 0, *w, ROW_H),
                    FONT_TINY,
                    ink,
                    overflow,
                )
            })
            .collect();
        out.push(Scene::Container(
            ContainerNode::new(cells)
                .with_tag(format!("card.{id}.row.{n}"))
                .with_layout(absolute(seat)),
        ));
    }
    out
}

/// ★★★★★ R2022 — where the stream card's rows sit, and therefore how many of
/// them there are.
///
/// The painter draws in these and [`stream_nodes`] counts them, which is what
/// stops the two disagreeing: a row this says has no seat is one the painter
/// cannot place and the accessibility tree cannot mention.
fn stream_seats(body: Rect) -> pinion_core::containment::RowSeats {
    pinion_core::containment::whole_rows_in(
        body,
        STREAM_HEAD_H,
        STREAM_ROW_H,
        spec::STREAM_ROWS.len(),
    )
}

/// The same, for the identifier map card.
fn map_seats(body: Rect) -> pinion_core::containment::RowSeats {
    pinion_core::containment::whole_rows_in(body, MAP_HEAD_H, MAP_ROW_H, spec::MAP_ROWS.len())
}

/// The decode card's TREE rows, which start under a padding band rather than
/// under a heading strip.
fn decode_seats(body: Rect) -> pinion_core::containment::RowSeats {
    pinion_core::containment::whole_rows_in(body, DECODE_TOP, DECODE_ROW_H, spec::DECODE_ROWS.len())
}

/// The decode card's BYTE lines, which share the tree's rhythm and its band —
/// they are read side by side, so a second derivation here would stagger them.
fn byte_line_seats(body: Rect) -> pinion_core::containment::RowSeats {
    pinion_core::containment::whole_rows_in(
        body,
        DECODE_TOP,
        DECODE_ROW_H,
        spec::DECODE_BYTES.len(),
    )
}

/// The decode inspector: the layer tree beside the bytes it decoded.
fn decode_body(id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    // The tree keeps at least half the card; the bytes pane takes what is left,
    // and is dropped entirely when that is less than one byte's worth. A fixed
    // pane on a card narrower than the pane paints outside the card, which the
    // gate reports and a reader sees as one card's bytes on the next one.
    let bytes_w = BYTES_W.min(rect.w / 2);
    let tree_w = match byte_pane_w(rect.w) {
        Some(pane) => rect.w.saturating_sub(pane + 12),
        None => rect.w,
    };
    let mut out = Vec::new();
    for (n, seat) in decode_seats(rect).iter() {
        let (depth, key, value) = spec::DECODE_ROWS[n];
        let indent = (10 + depth * 12).min(tree_w);
        let heading = depth == 0;
        let selected = n == spec::DECODE_SELECTED;
        // ★ The key is the identifying half, so it is allocated FIRST and the
        // value gets what is left. The first draft gave the value a fixed
        // seventy-four pixels at a fixed offset and let the key take the
        // remainder: on a narrowed card the remainder was nothing, the key
        // vanished, and the row read as a value with no name. A positional
        // comparison against the specification is what caught it -- a check
        // that asked only "are these the row's words" saw the same set.
        let room = tree_w.saturating_sub(indent + 6);
        let with_value = !value.is_empty() && room >= KEY_FLOOR + VALUE_W;
        let key_w = if with_value { room - VALUE_W } else { room };
        let mut cells = Vec::new();
        // ★ R1876 — the key and its value are two runs of ONE row, and both
        // were `y = 3` with a height of 13 for a face wanting 17. Derived from
        // the row, they share its centre by construction.
        if key_w > 0 {
            cells.push(clipped(
                key,
                decode_band(indent, key_w),
                FONT_TINY,
                if heading { palette.ink } else { palette.muted },
                TextOverflow::Ellipsis,
            ));
        }
        if with_value {
            cells.push(clipped(
                value,
                decode_band(indent + key_w, VALUE_W),
                FONT_TINY,
                palette.ink,
                TextOverflow::EllipsisStart,
            ));
        }
        out.push(Scene::Container(
            ContainerNode::new(cells)
                .with_tag(format!("card.{id}.tree.{n}"))
                .with_style(if selected {
                    BoxStyle::filled(palette.high).with_corner_radius(4)
                } else {
                    BoxStyle::default()
                })
                .with_layout(absolute(Rect::new(seat.x, seat.y, tree_w, seat.h))),
        ));
    }
    out.extend(byte_pane(
        id,
        Rect::new(rect.x + tree_w + 12, rect.y, bytes_w, rect.h),
        rect,
        palette,
    ));
    out
}

/// The bytes the decode card shows beside its tree, four per line, with the
/// selected field's own bytes lit.
///
/// The law screen B is built on (R1663): what is drawn lit is exactly what the
/// map says the selection occupies, not a resemblance of it.
fn byte_pane(id: &str, pane: Rect, card: Rect, palette: Palette) -> Vec<Scene> {
    const ROW_H: u32 = DECODE_ROW_H;
    let mut out = Vec::new();
    if pane.w < BYTES_FLOOR {
        return out;
    }
    let (start, end) = spec::DECODE_SELECTED_SPAN;
    // ★★★★★ R2022 — the lines the CARD has room for, from the derivation
    // [`byte_nodes`] also counts. The seats are the card's rather than the
    // pane's because the pane is as tall as the card and its lines share the
    // tree's rhythm.
    for (line, seat) in byte_line_seats(card).iter() {
        let quad = &spec::DECODE_BYTES[line];
        let mut cells = vec![label(
            &format!("{:04x}", line * 4),
            decode_band(6, 30),
            FONT_TINY,
            palette.muted,
        )];
        for (col, byte) in quad.iter().enumerate().take(byte_columns(pane.w)) {
            let index = line * 4 + col;
            let lit = index >= start && index < end;
            cells.push(Scene::Container(
                // ★★★★★ R1904 — CENTRED in the band, which is what a person
                // reading the running window said it was not. The band is
                // centred in the 22-wide cell and the cell is centred on its
                // column, and neither of those puts the GLYPHS anywhere: a run
                // with no declared alignment inks at its box's left edge, so a
                // 10-wide byte sat 3 from one side and 9 from the other inside
                // a chain of boxes that were each exactly centred.
                ContainerNode::new(vec![centred(
                    &format!("{byte:02x}"),
                    decode_band(2, 18),
                    FONT_TINY,
                    if lit { palette.on_accent } else { palette.ink },
                )])
                .with_tag(format!("card.{id}.byte.{index}"))
                .with_style(if lit {
                    BoxStyle::filled(palette.accent).with_corner_radius(3)
                } else {
                    BoxStyle::default()
                })
                .with_layout(absolute(Rect::new(40 + u(col) * 24, 0, 22, ROW_H))),
            ));
        }
        out.push(Scene::Container(
            ContainerNode::new(cells)
                .with_tag(format!("card.{id}.bytes.{line}"))
                .with_layout(absolute(Rect::new(pane.x, seat.y, pane.w, seat.h))),
        ));
    }
    out
}

/// ★★★★★ R2022 — how many of a byte line's four columns a pane that wide shows.
///
/// A cell that would leave the pane is not painted — its bytes are not lost, the
/// pane is simply narrower than four columns, and a cell drawn past the edge
/// lands on whatever is beside the card. This was the painter's `break`, so
/// [`byte_nodes`] announced all four whatever the pane's width.
fn byte_columns(pane_w: u32) -> usize {
    (0..spec::DECODE_BYTES.first().map_or(0, |quad| quad.len()))
        .take_while(|col| 40 + u(*col) * 24 + 22 <= pane_w)
        .count()
}

/// Whether the decode card's byte pane is drawn at all, and how wide it is.
///
/// The pane keeps at most half the card and is dropped entirely below
/// [`BYTES_FLOOR`], because a fixed pane on a card narrower than the pane paints
/// outside the card. Asked by the painter and by [`byte_nodes`].
fn byte_pane_w(body_w: u32) -> Option<u32> {
    let bytes_w = BYTES_W.min(body_w / 2);
    (bytes_w >= BYTES_FLOOR).then_some(bytes_w)
}

/// ★★★★★ R2022 — the identifier map's resource-path width, and whether the
/// timestamp column is there at all.
///
/// The columns are allocated left to right and a column with nothing left is
/// dropped, so a narrowed card shows the id and the resource rather than the id
/// and the timestamp. Same discipline as the stream's columns and the decode
/// tree's key: the identifying half is allocated first.
fn map_column_widths(body_w: u32) -> (u32, bool) {
    let room = body_w.saturating_sub(12 + MAP_ID_W + 6);
    let with_seen = room >= MAP_PATH_FLOOR + MAP_SEEN_W;
    (if with_seen { room - MAP_SEEN_W } else { room }, with_seen)
}

/// How many of the identifier map's three columns a body that wide paints.
fn map_columns_shown(body_w: u32) -> usize {
    let (path_w, with_seen) = map_column_widths(body_w);
    1 + usize::from(path_w > 0) + usize::from(with_seen)
}

/// The identifier map: numeric id to resource path, and when it was declared.
fn map_body(id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    const HEAD_H: u32 = MAP_HEAD_H;
    const ROW_H: u32 = MAP_ROW_H;
    let (path_w, with_seen) = map_column_widths(rect.w);
    // `row` is `None` for the header strip and the row index otherwise, which is
    // what the cell tags are built from.
    let cells = |ink: Color, cols: [&str; 3], warn: bool, row: Option<usize>| {
        let tag = |column: usize| match row {
            None => head_cell_tag(id, column),
            Some(r) => cell_tag(id, r, column),
        };
        // ★ R1873 — the strip the run sits in, named where it is used rather
        // than assumed: the heading strip and a data row are different seats
        // even where they happen to be the same height, and a band derived from
        // the wrong one would be centred in a strip the run is not in.
        let seat_h = if row.is_none() { HEAD_H } else { ROW_H };
        let seat = |x: u32, w: u32| Rect::new(x, 0, w, seat_h);
        let mut out = vec![grid_cell(
            tag(0),
            cols[0],
            seat(12, MAP_ID_W),
            FONT_TINY,
            if warn { palette.warn } else { ink },
            TextOverflow::Ellipsis,
        )];
        if path_w > 0 {
            out.push(grid_cell(
                tag(1),
                cols[1],
                seat(12 + MAP_ID_W + 6, path_w),
                FONT_TINY,
                ink,
                TextOverflow::EllipsisStart,
            ));
        }
        if with_seen {
            out.push(grid_cell(
                tag(2),
                cols[2],
                seat(12 + MAP_ID_W + 6 + path_w, MAP_SEEN_W),
                FONT_TINY,
                palette.muted,
                TextOverflow::Ellipsis,
            ));
        }
        out
    };
    let mut out = vec![Scene::Container(
        ContainerNode::new(cells(
            palette.muted,
            [
                spec::MAP_COLUMNS[0],
                spec::MAP_COLUMNS[1],
                spec::MAP_COLUMNS[2],
            ],
            false,
            None,
        ))
        .with_tag(format!("card.{id}.head"))
        .with_style(BoxStyle::filled(palette.raised))
        .with_layout(absolute(Rect::new(rect.x, rect.y, rect.w, HEAD_H))),
    )];
    for (n, seat) in map_seats(rect).iter() {
        let (key, path, seen) = spec::MAP_ROWS[n];
        let unresolved = n == spec::MAP_UNRESOLVED;
        out.push(Scene::Container(
            ContainerNode::new(cells(
                if unresolved {
                    palette.warn
                } else {
                    palette.ink
                },
                [key, path, seen],
                unresolved,
                Some(n),
            ))
            .with_tag(format!("card.{id}.map.{n}"))
            .with_layout(absolute(seat)),
        ));
    }
    out
}

/// The search and filter card: the query, the saved chips, and the three counts
/// whose relation is the point of the card.
/// The height the latency card keeps for its caption.
const LATENCY_CAPTION_H: u32 = 24;

/// ★★★★★ R2022 — how wide each tile of a strip of `n` stat tiles is, or `None`
/// when the body is too narrow for them.
///
/// The tiles go or stay TOGETHER — a strip showing two of three numbers whose
/// point is their relation says something a reader would misread — so this
/// answers about the strip and not about a tile. It was written twice, once in
/// the latency card's painter and once in the filter card's, and neither
/// describing function could ask either: both announced their tiles whatever the
/// width, which is a reader being told about tiles nobody drew.
fn stat_strip_w(body_w: u32, tiles: usize) -> Option<u32> {
    if tiles == 0 {
        return None;
    }
    let each = body_w.saturating_sub(2 * 8) / u(tiles);
    (each >= STAT_FLOOR).then_some(each)
}

/// ★★★★★ R2022 — the y the latency card's bars start at: under the tile strip
/// when the strip is drawn, and at the body's own top when it is not.
fn latency_bars_top(body: Rect, tiles: usize) -> u32 {
    if stat_strip_w(body.w, tiles).is_some() {
        body.y + STAT_H + 10
    } else {
        body.y
    }
}

/// The box the latency card's distribution is plotted in, or `None` when the
/// body has no room for one. Asked by the painter and by [`latency_nodes`].
fn latency_plot_rect(body: Rect, tiles: usize, bins: usize) -> Option<Rect> {
    distribution_box(
        body,
        latency_bars_top(body, tiles),
        bins,
        &ChartStyle::default(),
    )
}

/// Whether the latency card's caption band is drawn — it needs its own height
/// clear of wherever the bars begin.
fn latency_caption_shown(body: Rect, tiles: usize) -> bool {
    (body.y + body.h).saturating_sub(LATENCY_CAPTION_H) > latency_bars_top(body, tiles)
}

/// The shortest plot the latency card will draw a distribution into.
///
/// Under this the bars are shorter than the caption beside them and the shape
/// is not a distribution any more, so the card shows its three tiles and says
/// nothing it cannot say honestly.
const MIN_PLOT_H: u32 = 24;

/// ★★★★★ Where the latency card draws its distribution, or `None` when the card
/// is too small to draw one honestly.
///
/// The floor is DERIVED from the chart's own style, not guessed. The first
/// draft wrote `bars_h >= 40`, and the containment gate found what a number
/// picked by eye misses: a chart spends its margins before it draws anything,
/// and a rect narrower than those gutters puts the axis — and every mark
/// aligned to it — outside the box. `plot_area`'s margins say how much is
/// spent; a plot narrower than one pixel per bucket is not a distribution a
/// reader can see.
///
/// ★ One predicate, shared with the gate that sweeps it. A test carrying its
/// own copy of "is there room" would be checking a rule the painter does not
/// use, which is the failure that keeps a guard green while the screen is
/// wrong.
fn distribution_box(rect: Rect, top: u32, bars: usize, style: &ChartStyle) -> Option<Rect> {
    let h = (rect.y + rect.h).saturating_sub(top + LATENCY_CAPTION_H);
    let spent_w = style.margin.left + style.margin.right;
    let spent_h = style.margin.top + style.margin.bottom;
    let count = u32::try_from(bars).unwrap_or(u32::MAX);
    (rect.w > spent_w.saturating_add(count) && h > spent_h + MIN_PLOT_H)
        .then(|| Rect::new(rect.x, top, rect.w, h))
}

/// ★★★★★ R1797 — the latency card's binned distribution, derived once.
///
/// Everything the card draws comes out of this: the bar heights, the three stat
/// tiles, and which bars are the tail. That is the whole point of the card — see
/// [`spec::LATENCY_LADDER`] for what the reference publishes instead, and why
/// its two halves cannot both be true.
///
/// Returns `None` only if the specification's own record stops being binnable,
/// which the gate next door asserts it is not. A card that unwrapped here would
/// take the screen down over a constant.
fn latency_binned() -> Option<(Binned, Quantiles)> {
    let binned = Binned::over(
        spec::LATENCY_SAMPLES,
        spec::LATENCY_LADDER,
        // ★ Open, not closed. The record's slowest reply is 72 ms and the
        // ladder stops at 64, so a closed ladder would DROP it — and the
        // maximum tile would then report a sample no bar accounts for. An
        // unbounded top bin is what a latency distribution actually has.
        BinEnds::Open,
    )
    .ok()?;
    // `Linear` — Hyndman & Fan type 7, R's and NumPy's default — because the
    // card needs p95 and Tukey's hinges do not define one. Naming it is the
    // point: `Quantiles::at` would REFUSE p95 under the default method rather
    // than quietly interpolating a number Tukey never defined.
    let quantiles = Quantiles::of(spec::LATENCY_SAMPLES, QuantileMethod::Linear).ok()?;
    Some((binned, quantiles))
}

/// ★ R1797 — the palette's groups, each naming the releases its entries occupy.
///
/// Both, when a group holds both. The tier column this replaced could not say
/// that: a section carried one tier of its own and every entry had to match it,
/// which is one fact stored twice — and promoting a single widget out of a
/// group made one of the two copies false.
///
/// `palette_groups_json` and not `sections_json`, which this file already uses
/// for the RAIL's sections. Two unrelated things are called a section on this
/// screen — a rail destination and a palette group — and the compiler is what
/// said so.
fn palette_groups_json() -> Vec<serde_json::Value> {
    spec::SECTIONS
        .iter()
        .map(|(key, title)| {
            let (placeable, reserved) = spec::section_tiers(key);
            // Built before the macro rather than inside it: `json!` matches an
            // array literal as a JSON array and stops there, so a method chain
            // after the `]` is not a token it expects.
            let tiers: Vec<&str> = [
                placeable.then(|| tier_word(spec::Tier::Placeable)),
                reserved.then(|| tier_word(spec::Tier::Reserved)),
            ]
            .into_iter()
            .flatten()
            .collect();
            serde_json::json!({
                "key": key,
                "title": title,
                "tiers": tiers,
                "heading": spec::section_heading(key, title),
            })
        })
        .collect()
}

/// ★★★★★ R1797 — what the latency card derived, for a reader who never sees it.
///
/// The debt that opened this round asked for exactly this and named the reason:
/// *the wire should answer the rule and the basis, because that is where this
/// module goes past the floor* — it is what separates a surprising shape that
/// is in the data from one the binning made. The floor publishes neither; its
/// bar surface has no name for a rule or a basis at all, measured this round.
///
/// It is also what makes the card's consistency checkable from OUTSIDE: the
/// tiles and the bucket counts come from one derivation and are published
/// together, so an agent can do the arithmetic the reference's own card fails —
/// walk the counts to the 95th percentile and ask whether the bucket it lands
/// in contains the published cut. `tools/demos/r1649_…` does exactly that.
///
/// Every field is DERIVED. Nothing here is the reference's published figure:
/// those live under `#[cfg(test)]` because they are the gate's oracle, and
/// shipping them beside these would put two accounts of one number on the wire.
fn latency_wire() -> serde_json::Value {
    let Some((binned, quantiles)) = latency_binned() else {
        // The specification's own record stopped being binnable, which the gate
        // says cannot happen. Reported rather than unwrapped: a wire read is
        // not a place to take the process down.
        return serde_json::json!({ "binned": false });
    };
    let cut = quantiles.at(0.95).ok();
    let basis = binned.basis();
    serde_json::json!({
        "binned": true,
        "rule": binned.rule().name(),
        "ends": if binned.ends() == BinEnds::Open { "open" } else { "closed" },
        "unit": spec::LATENCY_UNIT,
        "boundaries": spec::LATENCY_LADDER,
        "buckets": (0..binned.bins()).map(|k| serde_json::json!({
            "label": binned.label(k),
            "count": binned.counts()[k],
            "lo": binned.extent(k).and_then(|(lo, _)| lo),
            "hi": binned.extent(k).and_then(|(_, hi)| hi),
            "tail": binned.tail_from(cut.unwrap_or(f64::INFINITY)).contains(&k),
        })).collect::<Vec<_>>(),
        "outside": { "below": binned.outside().0, "above": binned.outside().1 },
        "basis": {
            "n": basis.n,
            "min": basis.min,
            "max": basis.max,
            "sigma": basis.sigma,
            "iqr": basis.iqr,
            "quantile_method": basis.quantile_method.name(),
        },
        "tiles": latency_stats(&quantiles).into_iter().map(|(key, value)| {
            serde_json::json!({ "key": key, "value": value })
        }).collect::<Vec<_>>(),
        "tail_cut": cut,
        "caption": spec::LATENCY_CAPTION,
    })
}

/// The card's three stat tiles, as `(key, rendered value)`.
///
/// Derived rather than stated, so a change to the record moves the tiles. The
/// gate asserts these land on the reference's three published figures.
fn latency_stats(quantiles: &Quantiles) -> Vec<(&'static str, String)> {
    let fmt = |ms: f64| format!("{ms:.1} {}", spec::LATENCY_UNIT);
    let read = |p: f64| quantiles.at(p).map_or_else(|_| "\u{2014}".to_owned(), fmt);
    // ★ The KEYS come from the specification and the VALUES are derived here.
    // Writing the three words again beside the three derivations would be the
    // second copy that drifts: the specification is what the paint gate reads
    // to know how many tiles to expect and what each is called.
    let values = [read(0.50), read(0.95), fmt(quantiles.max())];
    spec::LATENCY_STAT_KEYS
        .iter()
        .copied()
        .zip(values)
        .collect()
}

/// ★★★★★ R1955 — **a stat tile's two lines, stacked so they cannot collide**,
/// and R1956 — **through the framework's own element rather than a fourth
/// hand-rolled one.**
///
/// Both stat cards paint the same shape: a bigger line and a smaller one, one
/// above the other inside a [`STAT_H`] tile. Each wrote FOUR literals for it —
/// two heights and two `y`s — and the heights were four and five pixels short
/// of the faces they hold, which is what a reader meets as a cut descender.
///
/// ⚠ Deriving only the heights is what made a helper necessary rather than
/// optional: with the boxes grown and the `y`s left as literals, the second line
/// started one pixel inside the first and `r1649`'s smear gate reported six
/// pairs painted on top of each other. **A height and the offset of what sits
/// under it are one fact.**
///
/// ⚠⚠ AND THE FIRST DRAFT OF THIS FUNCTION WAS ITSELF THE DUPLICATION IT WAS
/// REPAIRING. R1955 wrote its own stacking arithmetic — first line at a given
/// top, second at `top + line_box(first)` — without asking whether the
/// framework already owned it. It does:
/// [`pinion_core::containment::stacked_line_rects`], built at R1874 for exactly
/// this shape (*a name over its gist, a title over its subtitle*), and it is
/// STRICTLY better than what R1955 wrote — the block is centred in the seat by
/// `band_in`'s rule, rounding once, so a one-line stack lands exactly where
/// `line_rect_in` would put it and the tile's own top offset stops being a
/// number anybody picks.
fn stat_lines(first: (&str, u32, Color), second: (&str, u32, Color), seat: Rect) -> Vec<Scene> {
    let (first_text, first_face, first_ink) = first;
    let (second_text, second_face, second_ink) = second;
    let [top, bottom] = pinion_core::containment::stacked_line_rects(
        seat,
        10,
        seat.w.saturating_sub(20),
        [first_face, second_face],
    );
    vec![
        label(first_text, top, first_face, first_ink),
        label(second_text, bottom, second_face, second_ink),
    ]
}

/// The latency card's content: three derived tiles, the binned distribution
/// with its tail emphasised, and the caption that says what the emphasis means.
fn latency_body(id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    let Some((binned, quantiles)) = latency_binned() else {
        return placeholder_body("latency", id, rect, palette);
    };
    let mut out = Vec::new();

    // --- the three tiles ---------------------------------------------------
    let stats = latency_stats(&quantiles);
    if let Some(stat_w) = stat_strip_w(rect.w, stats.len()) {
        for (n, (key, value)) in stats.iter().enumerate() {
            out.push(Scene::Container(
                ContainerNode::new(stat_lines(
                    (key, FONT_TINY, palette.muted),
                    (value, FONT_TITLE, palette.ink),
                    Rect::new(0, 0, stat_w, STAT_H),
                ))
                .with_tag(format!("card.{id}.stat.{n}"))
                .with_style(
                    BoxStyle::filled(palette.raised)
                        .with_corner_radius(8)
                        .with_border(Border::new(palette.outline, 1)),
                )
                .with_layout(absolute(Rect::new(
                    rect.x + u(n) * (stat_w + 8),
                    rect.y,
                    stat_w,
                    STAT_H,
                ))),
            ));
        }
    }

    // --- the bars ----------------------------------------------------------
    //
    // ★ The tail comes from the SAMPLES, through `tail_from`, and not from an
    // index. The reference paints its last two bars amber by writing `i >= 6`;
    // this asks which bins lie entirely at or above the 95th percentile, so it
    // moves when the capture does and a reader can check the claim against the
    // tile beside it.
    let cut = quantiles.at(0.95).unwrap_or(f64::INFINITY);
    let tail = binned.tail_from(cut);
    let bars: Vec<Bar> = binned
        .bars()
        .into_iter()
        .enumerate()
        .map(|(k, mut bar)| {
            // The interval form, which is what a bucket column with no numeric
            // axis is read in. `bars()` labels a bounded bin with its lower
            // edge, for a reader matching an axis tick; this card has no tick
            // to match, and its unbounded bins have no edge to print.
            bar.label = binned.label(k);
            if tail.contains(&k) {
                bar.color = Some(palette.warn);
            }
            bar
        })
        .collect();

    let style = ChartStyle::default();
    if let Some(box_) = latency_plot_rect(rect, stats.len(), bars.len()) {
        out.push(Scene::Container(
            ContainerNode::new(vec![
                // ★★★★★ The chart's prefix must be neither EQUAL TO nor a
                // PREFIX OF the container's tag, and this round hit both walls
                // in turn:
                //
                // * `card.{id}.bin` against a container `card.{id}.bins` —
                //   one letter, and it reads fine. Every chart node then
                //   declared itself part of a tag that is not its ancestor.
                // * `card.{id}.bins` for both — the chart emits a root node
                //   carrying the bare prefix, so TWO regions answered to one
                //   address and the voice census counted 257 tags for 258
                //   regions.
                //
                // `.dist` collides with neither. The sibling sparkline has had
                // this arrangement since R1648 (`match.spark` inside
                // `card.{id}.sparkline`) and nothing had written down why.
                //
                // ⚠ And neither wall was what broke twenty-seven demos. A
                // `Silence::part_of(X)` is a promise that X SPEAKS for these
                // marks; tagging X does not make that promise. What was missing
                // is `latency_nodes`, and until it existed the chart was silent
                // while claiming to be covered.
                BarChart::new(bars)
                    .with_tag_prefix(format!("card.{id}.dist"))
                    .build(Rect::new(0, 0, box_.w, box_.h), &style)
                    .silenced(Silence::part_of(format!("card.{id}.bins"))),
            ])
            .with_tag(format!("card.{id}.bins"))
            .with_layout(absolute(box_)),
        ));
    }

    // --- the caption -------------------------------------------------------
    let caption_y = (rect.y + rect.h).saturating_sub(LATENCY_CAPTION_H);
    if latency_caption_shown(rect, stats.len()) {
        out.push(Scene::Container(
            // ★ The run is INSET. Written flush at `x: 0` first, and the ink
            // gate caught it: a glyph's ink can start a pixel left of its
            // run's origin (a `q`'s tail, an italic's overhang), so a run at
            // its container's own edge paints outside the box that owns it.
            // Every other body in this file pads for the same reason.
            ContainerNode::new(vec![clipped(
                spec::LATENCY_CAPTION,
                Rect::new(10, 6, rect.w.saturating_sub(20), 13),
                FONT_TINY,
                palette.muted,
                TextOverflow::Ellipsis,
            )])
            .with_tag(format!("card.{id}.caption"))
            .with_style(
                BoxStyle::filled(palette.panel).with_border(Border::new(palette.outline, 1)),
            )
            .with_layout(absolute(Rect::new(
                rect.x,
                caption_y,
                rect.w,
                LATENCY_CAPTION_H,
            ))),
        ));
    }
    out
}

fn filter_body(state: &ShellState, id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    let mut out = vec![Scene::Container(
        ContainerNode::new(vec![label(
            spec::FILTER_QUERY,
            Rect::new(10, 7, rect.w.saturating_sub(36), 14),
            FONT_SMALL,
            palette.ink,
        )])
        .with_tag(format!("card.{id}.query"))
        .with_style(
            BoxStyle::filled(palette.raised)
                .with_corner_radius(7)
                .with_border(Border::new(palette.outline, 1)),
        )
        .with_layout(absolute(Rect::new(rect.x, rect.y, rect.w, 28))),
    )];
    // ★★★★★ R1721 — the bar is one widget, and the chips now come off it: which
    // are on, which is a keyboard stop, and what each is called. Before this the
    // five were painted from a constant table and a press over any of them did
    // nothing, while the tree announced five operable toggle buttons.
    let row = filter_row(state, id);
    let placed = filter_chip_rects(rect);
    let bar = filter_bar_rect(rect);
    let mut pills = Vec::with_capacity(placed.len());
    for (n, at) in &placed {
        let chip = &row.chips()[*n];
        pills.push(
            Scene::Container(
                ContainerNode::new(vec![clipped(
                    &chip.label,
                    // ★★★★★ R1955 — the label's box is DERIVED from the face it
                    // is set in. It was the literal `13` while
                    // `line_box(FONT_TINY)` is 17, so every chip on this card
                    // was four pixels short of the line it holds — the shape a
                    // person sees as a cut descender, and six of the first six
                    // rows `r1800`'s gate names when its budget is lowered.
                    Rect::new(
                        9,
                        4,
                        at.w.saturating_sub(18),
                        pinion_core::containment::line_box(FONT_TINY),
                    ),
                    FONT_TINY,
                    if chip.on {
                        palette.on_accent
                    } else {
                        palette.muted
                    },
                    TextOverflow::Ellipsis,
                )])
                .with_tag(chip.tag.clone())
                .with_style(
                    BoxStyle::filled(if chip.on {
                        palette.accent
                    } else {
                        palette.raised
                    })
                    .with_corner_radius(10)
                    .with_border(Border::new(palette.outline, 1)),
                )
                .with_layout(absolute(Rect::new(
                    at.x - bar.x,
                    at.y - bar.y,
                    at.w,
                    at.h,
                ))),
            )
            .with_focusable(row.is_a_stop(&chip.tag)),
        );
    }
    out.push(
        Scene::Container(
            ContainerNode::new(pills)
                .with_tag(row.tag().to_owned())
                // ★ A tagged node that is not pointer-transparent becomes the
                // router's hit target and swallows the press —
                // `debt-a-tagged-node-can-swallow-a-real-press-anywhere`. The bar
                // is a keyboard fact; a press still reaches the chip under it, or
                // falls through to the card between chips.
                .with_layout(absolute(bar).with_pointer_transparent(true)),
        )
        .with_focusable(row.is_a_stop(row.tag())),
    );
    out.extend(filter_counts(
        state,
        id,
        filter_counts_area(rect),
        rect,
        palette,
    ));
    out
}

/// ★★★★★ R2022 — the band the filter card's three counts and its trend go in,
/// which starts below whatever line the chips wrapped to.
///
/// It was spelled inside the painter, so [`filter_nodes`] could not know whether
/// there was room for the tiles at all — and announced all three whatever the
/// card's size.
fn filter_counts_area(body: Rect) -> Rect {
    let last_line = filter_chip_rects(body)
        .last()
        .map_or(body.y + 34, |(_, at)| at.y);
    Rect::new(
        body.x,
        last_line + 30,
        body.w,
        body.y + body.h - (last_line + 30).min(body.y + body.h),
    )
}

/// Whether the filter card's three counts are painted, and whether the trend
/// under them is. The counts go or stay together (see [`filter_counts`]), and
/// the trend needs its own thirty pixels below them.
fn filter_counts_shown(body: Rect) -> (bool, bool) {
    let area = filter_counts_area(body);
    let counts = stat_strip_w(area.w, spec::FILTER_STATS.len()).is_some()
        && area.y + STAT_H <= body.y + body.h;
    (counts, counts && area.y + 52 + 30 <= body.y + body.h)
}

/// ★★★★★ R1721 — the filter card's saved-filter bar, as the widget it is.
///
/// The rule is [`spec::FILTER_ROW`] and this is the only place that reads the
/// screen's state through it, so the roles, the selection attribute, the stop
/// and the arrows cannot be decided anywhere else.
fn filter_row(state: &ShellState, id: &str) -> ChipGroup {
    // ★★★★★ R1721 — the seat comes from the shell's ONE cursor map, not from a
    // signal beside it. The first draft of this round added a `filter_cursor`
    // signal and that was the two-copies shape this tree has paid for
    // repeatedly: the map is where every other composite's cursor lives, and a
    // second copy would be a second thing for the arrows and the paint to
    // disagree about. `cursor_members` reads the roster WITHOUT a seat, so this
    // read does not recurse.
    let seat = state
        .cursor_of(&filter_chips_tag())
        .and_then(|roving| roving.cursor())
        .unwrap_or(0);
    filter_row_of(id, state.filter_chip.get(), seat, spec::FILTER_CHIPS.len())
}

/// [`filter_row`] holding only the chips a body that size has room to paint —
/// what a describing reader is walked through (R2022).
fn filter_row_shown(state: &ShellState, id: &str, body: Rect) -> ChipGroup {
    let seat = state
        .cursor_of(&filter_chips_tag())
        .and_then(|roving| roving.cursor())
        .unwrap_or(0);
    filter_row_of(
        id,
        state.filter_chip.get(),
        seat,
        filter_chip_rects(body).len(),
    )
}

/// The bar built from a choice and a cursor, so the roster and the rule are
/// readable without a running screen — the ring's roster is asked for from
/// `cursor_members`, which has no state to read and must not grow a second copy
/// of what the chips are.
/// ★★★★★ R2022 — `shown` is how many of the chips the card has ROOM for.
///
/// The bar wraps and a chip that would wrap past the card's bottom is not
/// painted ([`filter_chip_rects`]), so a roster of five on a one-cell card is a
/// roster of toggles two of which are not there. Every caller that is not
/// describing the paint passes the whole set; [`filter_nodes`] passes what the
/// geometry placed.
fn filter_row_of(id: &str, chosen: Option<usize>, cursor: usize, shown: usize) -> ChipGroup {
    ChipGroup::new(
        format!("card.{id}.chips"),
        "Saved filters",
        spec::FILTER_CHIPS
            .iter()
            .enumerate()
            .take(shown)
            .map(|(n, (name, _))| {
                Chip::new(format!("card.{id}.chip.{n}"), *name, chosen == Some(n))
            })
            .collect(),
        spec::FILTER_ROW,
    )
    .with_cursor(cursor)
}

/// The filter card the board opens with, and therefore the card whose
/// saved-filter bar the declared focus ring names.
const FILTER_CARD: &str = "filter#3";

/// The tag that bar carries, derived from [`FILTER_CARD`] so the ring's entry and
/// the widget's own tag are one string rather than two that agree today.
fn filter_chips_tag() -> String {
    format!("card.{FILTER_CARD}.chips")
}

/// Where the filter card's chips land, and which of them the card is tall enough
/// to show: `(index, rect)` in the body's own coordinates.
///
/// ★ ONE geometry, read by the paint and by the hit test —
/// `debt-paint-and-gesture-read-two-facts` is open in this project because a
/// control drawn where it cannot be pressed is what happens when those are two
/// functions, and until this round the chips were drawn by a loop the hit test
/// did not have.
///
/// A chip is as wide as its word, and a row of them wraps rather than running off
/// the card — the policy R1651 measured on the reference's own form. It is also
/// CLAMPED to the card: a word longer than the card cannot be made to fit by
/// wrapping, and the first draft let it run off the right edge on a one-cell
/// card.
/// R1721 — the box the saved-filter chips sit in, in the body's own coordinates.
///
/// The bar is a region the specification names, so it has to be painted and it
/// has to have bounds an assistive technology can be given. Derived from the
/// chips rather than written down, and it falls back to the first line's strip
/// when the card is too short to show any: a bar that vanished would take the
/// region with it, and "the card is too short for the chips" is a different fact
/// from "there is no saved-filter bar".
fn filter_bar_rect(rect: Rect) -> Rect {
    let placed = filter_chip_rects(rect);
    let Some((_, first)) = placed.first() else {
        return Rect::new(rect.x, rect.y + 34, rect.w, 22);
    };
    let right = placed.iter().map(|(_, at)| at.x + at.w).max().unwrap_or(0);
    let bottom = placed.iter().map(|(_, at)| at.y + at.h).max().unwrap_or(0);
    Rect::new(
        rect.x,
        first.y,
        right.saturating_sub(rect.x),
        bottom.saturating_sub(first.y),
    )
}

fn filter_chip_rects(rect: Rect) -> Vec<(usize, Rect)> {
    let mut out = Vec::with_capacity(spec::FILTER_CHIPS.len());
    let mut x = rect.x;
    let mut y = rect.y + 34;
    for (n, (name, _)) in spec::FILTER_CHIPS.iter().enumerate() {
        let w = (18 + u(name.chars().count()) * 6).min(rect.w);
        if x + w > rect.x + rect.w {
            x = rect.x;
            y += 26;
        }
        if y + 22 > rect.y + rect.h {
            break;
        }
        out.push((n, Rect::new(x, y, w, 22)));
        x += w + 6;
    }
    out
}

/// The filter card's three counts, and the recent past of the first.
///
/// Three rather than one because the reference's point is the RELATION -- a
/// reader is looking at a subset of a subset, and a single number cannot say
/// which subset it is. The tiles go or stay together: a card too short for them
/// shows the query and the chips, which are the parts a reader can still act on.
fn filter_counts(
    state: &ShellState,
    id: &str,
    area: Rect,
    card: Rect,
    palette: Palette,
) -> Vec<Scene> {
    let mut out = Vec::new();
    let Some(stat_w) = stat_strip_w(area.w, spec::FILTER_STATS.len()) else {
        return out;
    };
    if area.y + STAT_H > card.y + card.h {
        return out;
    }
    for (n, (value, what)) in spec::FILTER_STATS.iter().enumerate() {
        out.push(Scene::Container(
            ContainerNode::new(stat_lines(
                (value, FONT_TITLE, palette.ink),
                (what, FONT_TINY, palette.muted),
                Rect::new(0, 0, stat_w, STAT_H),
            ))
            .with_tag(format!("card.{id}.stat.{n}"))
            .with_style(
                BoxStyle::filled(palette.raised)
                    .with_corner_radius(8)
                    .with_border(Border::new(palette.outline, 1)),
            )
            .with_layout(absolute(Rect::new(
                area.x + u(n) * (stat_w + 8),
                area.y,
                stat_w,
                STAT_H,
            ))),
        ));
    }
    // The recent past of the first count, so a reader can see whether the
    // matched share is moving before reading three numbers off the tiles.
    let spark_y = area.y + 52;
    if spark_y + 30 <= card.y + card.h {
        out.push(Scene::Container(
            ContainerNode::new(vec![
                // The plot area and its stroke are HOW the region is drawn; the
                // region itself states the series, so these two are declared
                // quiet rather than left undecided.
                // ★★★★★ R1824 — the trend PARTICIPATES in the board's
                // cross-filter, through the one API every chart kind answers
                // (`pinion_chart::Mute`). Until this round it was the last
                // thing on this screen that a saved filter reached and that
                // went on looking exactly the same: `MATCH_SERIES` is the
                // matched count of the WHOLE capture, so under a saved filter
                // it is a trend of something other than what the reader is
                // looking at, and it kept claiming otherwise at full strength.
                //
                // Named rather than tagged: `with_tag_prefix` addresses the
                // chart, `labelled` says what the trend is OF, and it is the
                // latter a `Selection::Category` is matched against.
                Sparkline::new(MATCH_SERIES.to_vec())
                    .labelled(MATCH_SERIES_OF)
                    .with_color(kind_color("filter"))
                    .with_tag_prefix("match.spark")
                    .muted_by_reach(cross_filter(state).as_ref(), kind_of(id))
                    .build(
                        Rect::new(0, 0, area.w, card.y + card.h - spark_y),
                        &ChartStyle::default(),
                    )
                    .silenced(Silence::part_of(format!("card.{id}.sparkline"))),
            ])
            .with_tag(format!("card.{id}.sparkline"))
            .with_layout(absolute(Rect::new(
                area.x,
                spark_y,
                area.w,
                card.y + card.h - spark_y,
            ))),
        ));
    }
    out
}

/// ★★★★★ R2002 — **the words a health tile paints at its label face**, and the
/// only place they are spelled.
///
/// [`StatTile`] declares [`Silence::name_of`] on that face: *my ink is the
/// tile's name*. WAI-ARIA calls the resulting obligation label-in-name, so the
/// tile's accessible name has to carry these words — and until this round the
/// two were built separately and disagreed. Measured by the census arm added
/// the same round: tile 2 painted `Rate /s` and was announced `Rate`, so a
/// person reading the tile's heading aloud reached nothing, while the `/s` was
/// filed away in the tile's VALUE where a sighted reader never sees it.
///
/// ⚠ The unit rides with the label rather than with the value, and the reason
/// is in [`tile_metrics`]: it is a property of the quantity, not of one reading
/// of it. That decision was already made for the ink; this is it being made
/// once instead of twice.
fn tile_heading(tile: &spec::HealthTile) -> String {
    if tile.unit.is_empty() {
        tile.label.to_owned()
    } else {
        format!("{} {}", tile.label, tile.unit)
    }
}

/// One health tile's specification — its words and its skin, with no placement.
///
/// ★ R1843 — one definition, because the strip asks it TWICE: once to find how
/// many tiles fit ([`StatTile::min_width`]) and once to paint them. Two
/// spellings of a tile would let the fitting rule and the painted tile drift,
/// and the drift would show as a card that fits four tiles and paints five.
///
/// ★ The UNIT rides with the label, not with the value, and the ink gate
/// decided it: `"6.4k msg/s"` at the value face hung 42px past its box. The
/// label draws at the tiny face and the value at the title face, so the same
/// words cost far less beside the label — and a unit belongs to the quantity
/// rather than to one reading of it.
fn tile_metrics(tile: &spec::HealthTile) -> StatTile {
    StatTile::new(tile_heading(tile), tile.value)
        .with_delta(tile.delta)
        .with_label_style(TextStyle::new().with_size_px(FONT_TINY))
        .with_value_style(TextStyle::new().with_size_px(FONT_TITLE))
        .with_delta_style(TextStyle::new().with_size_px(FONT_TINY))
}

/// The same tile, wearing the board's colours.
///
/// ⚠ The split is not decoration. [`StatTile::min_width`] reads font sizes and
/// never colours, and the accessibility tree has to ask that question WITHOUT a
/// [`Palette`] — building one needs `use_theme`, which needs an `Owner` scope
/// the shell's non-paint paths do not have (the R1721 lesson, learned when a
/// body painter reached for `use_shell_state` and the running screen panicked).
/// So the words and the faces live in [`tile_metrics`], which both readers
/// share, and only the ink is added here.
fn tile_spec(tile: &spec::HealthTile, palette: Palette) -> StatTile {
    tile_metrics(tile)
        .with_label_style(
            TextStyle::new()
                .with_size_px(FONT_TINY)
                .with_fg(palette.muted),
        )
        .with_value_style(
            TextStyle::new()
                .with_size_px(FONT_TITLE)
                .with_fg(palette.ink),
        )
        .with_delta_style(
            TextStyle::new()
                .with_size_px(FONT_TINY)
                .with_fg(palette.muted),
        )
        .with_box_style(
            BoxStyle::filled(palette.raised)
                .with_corner_radius(8)
                .with_border(Border::new(palette.outline, 1)),
        )
}

/// Room between one tile of the health strip and the next.
const TILE_GAP: u32 = 8;
/// A tile narrower than this cannot hold a label and a value, so the strip
/// draws nothing rather than a row of clipped words.
const TILE_FLOOR: u32 = 84;
/// The height the health strip draws a trend series in.
const TREND_H: u32 = 16;

/// How many tiles a health strip `width` px wide can show, or `None` for none.
///
/// ★★★★★ R1843 — ONE rule with TWO readers, and the demo is what forced it.
///
/// The strip narrows by dropping whole tiles, so how many it shows is a
/// function of its width. That rule lived inside the painter, and the
/// accessibility tree announced all five unconditionally — so at the opening
/// size the card PAINTED three tiles and ANNOUNCED five. The demo measured
/// exactly that (`3 tile(s) painted, 5 announced`), which is a reader being
/// told about two tiles nobody drew.
///
/// ⚠ Worse than a miscount: the round had written that announcing all five was
/// a VIRTUE — "a reader asking the card what it knows gets every quantity
/// whatever the width let it draw". It is not a virtue, it is a ghost region,
/// and the sentence was repaired in the same commit that found it.
///
/// The floor is what the first `n` tiles THEMSELVES need, asked of the widget
/// through [`StatTile::min_width`] rather than picked here: a number chosen for
/// one set of words says nothing about another set.
fn health_tile_count(width: u32) -> Option<u32> {
    let most = u32::try_from(spec::HEALTH_TILES.len()).unwrap_or(1);
    let fits = |n: u32| {
        n > 0 && {
            let each = width.saturating_sub(TILE_GAP * (n - 1)) / n;
            each >= TILE_FLOOR
                && spec::HEALTH_TILES[..n as usize]
                    .iter()
                    .all(|t| tile_metrics(t).min_width() <= each)
        }
    };
    (1..=most).rev().find(|n| fits(*n))
}

/// The health card: a strip of KPI tiles, each with its own trend sparkline.
///
/// ★★★★★ R1843 — the census's `dashboard.t1.8`, and the point of it is that the
/// tile is no longer assembled here. `pinion_widget_paint::stat_tile` builds the
/// box and PLACES the words through `caption`; this function decides only what
/// the tiles say and how wide they are.
///
/// ★ The sparkline is passed IN rather than named by the tile, because
/// `pinion-widget-paint` does not depend on `pinion-chart` and a tile that
/// embedded a chart would make it. The seam is a closure taking the rectangle
/// the tile reserved, so a figure cannot be built for a tile with no room.
///
/// ⚠⚠ **Every rectangle below is in its PARENT's space, and the first draft got
/// this wrong twice in one function.** A child laid out absolutely resolves
/// against its container, so passing a caller-space rectangle into a container
/// that is itself absolutely positioned applies the offset twice. The ink gate
/// reported `card.health#2.stat.0` at y=530 inside a card at y=462 — the body's
/// own origin, added a second time. The tiles take strip-local coordinates and
/// the sparkline takes trail-local ones for that reason.
fn health_body(id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    // ★★★★★ R1843 — how many tiles fit, rather than all of them or none.
    //
    // The first draft painted five or nothing, and nothing is what a
    // four-column placement got. That is the wrong shape twice over: a card
    // that paints nothing reads as a placeholder, and a strip that insists on
    // five would clip every one of them. So the count comes DOWN from the
    // width — the widest `n` whose tiles each clear `TILE_FLOOR` — and the
    // reference does the same thing by a different means, letting its strip
    // scroll sideways when its own presets place this seat narrow.
    //
    // ⚠ Fewer tiles is fewer FACTS on screen, so it is not free. What makes it
    // honest here is that the tiles are ordered and the ones dropped are the
    // last of them.
    //
    // ⚠⚠ R2022 — this used to end *"and the a11y tree still announces all five:
    // a reader asking the card what it knows gets every quantity, whatever the
    // width let it draw"*, and that is the sentence R1843 recorded as its own
    // finding and R1846 repaired. It was repaired in `health_nodes`, thirty
    // lines away, and left standing HERE — so the round that named the defect
    // fixed one of the sentence's two copies. A reader is told about the tiles
    // this strip DRAWS; announcing the others is the ghost region this board
    // carries a gate for.
    let Some(count) = health_tile_count(rect.w) else {
        return Vec::new();
    };
    let tiles = &spec::HEALTH_TILES[..count as usize];
    let tile_w = rect.w.saturating_sub(TILE_GAP * (count - 1)) / count;

    let style = ChartStyle::default();
    let mut out = Vec::new();
    for (n, tile) in tiles.iter().enumerate() {
        // ★ R1843 — the UNIT rides with the label, not with the value, and the
        // ink gate is what decided it: `"6.4k msg/s"` at the value face hung
        // 42px past its own box. The label is drawn at the tiny face and the
        // value at the title face, so the same words cost far less beside the
        // label — and a unit belongs to what is being measured rather than to
        // this particular reading of it.
        let built = tile_spec(tile, palette).with_trail(TREND_H).build_with(
            &format!("card.{id}.stat.{n}"),
            // Strip-local: the container below is what carries `rect`.
            Rect::new(u(n) * (tile_w + TILE_GAP), 0, tile_w, rect.h),
            |trail| {
                // ★ The prefix is neither equal to nor a prefix of the
                // container's tag (`…stat.{n}.trail`) — the collision R1797
                // hit twice in one round, because a chart emits a root node
                // carrying the bare prefix.
                // ★ R1843 — the prefix sits UNDER the trail's tag, not
                // beside it. R1797's rule is that a chart's prefix must be
                // neither equal to nor a prefix of its container's tag, and
                // `…stat.{n}.spark` satisfies that — but it also failed to
                // say the chart is INSIDE `…stat.{n}.trail`, so the two
                // regions shared one rectangle and the disjointness gate
                // read them as painted over each other. Naming the chart as
                // the trail's child expresses the nesting the scene already
                // has, and still collides with nothing.
                Sparkline::new(tile.trend.to_vec())
                    .with_tag_prefix(format!("card.{id}.stat.{n}.trail.spark"))
                    .with_color(palette.accent)
                    // Trail-local, for the same reason as the tile above.
                    .build(Rect::new(0, 0, trail.w, trail.h), &style)
            },
        );
        out.push(built.into_scene());
    }
    vec![Scene::Container(
        ContainerNode::new(out)
            .with_tag(format!("card.{id}.tiles"))
            .with_layout(absolute(rect)),
    )]
}

// --- The alarm feed (R1851) --------------------------------------------------

/// The severity swatch's width — the stripe the behaviour prototype draws down
/// a row's leading edge, which is how a reader who is scanning finds the severe
/// rows without reading a word.
const ALARM_BAR_W: u32 = 3;
/// The clearance between the swatch and the first word.
const ALARM_INSET: u32 = 11;

/// The ink a severity is drawn in.
///
/// ★ Role-derived, never a literal, for the reason [`Palette::warn`]'s own
/// comment gives: a hand-picked amber or red holds its contrast in exactly one
/// of the two themes.
///
/// ⚠ The LEAST severe level recedes rather than taking a saturated ink. The
/// behaviour prototype paints its `info` rows a strong blue, which competes with
/// its warnings for a reader's attention; a row that is not an alert should be
/// present and not the point. This is not this round's invention: measured at
/// R1851, `hello-log-view` — the same product's other severity surface, written
/// against the same reference — already resolves its `Info` to its muted text
/// ink. Two screens, one mapping.
///
/// ⚠⚠ Two consumers is not three. The standing rule lifts a MECHANICAL
/// duplicate at two and defers an OPINIONATED one to the third identical copy,
/// and a severity-to-ink mapping is a design choice. So this stays here and the
/// measurement is recorded rather than acted on.
fn severity_ink(rank: usize, palette: Palette) -> Color {
    match rank {
        0 => palette.muted,
        n if n + 1 >= spec::SEVERITY.len() => palette.refused,
        _ => palette.warn,
    }
}

/// The severity ranks of every alarm, or the refusal naming the first word the
/// vocabulary does not hold.
///
/// ★ This cannot fail today — every word in [`spec::ALARMS`] is in
/// [`spec::SEVERITY`], and a gate asserts it. It returns a `Result` anyway
/// because the FLOOR comes off the wire, where a caller can write anything, and
/// one resolution path for both means a bad word is refused in the same words
/// wherever it came from.
fn alarm_ranks() -> Result<Vec<usize>, pinion_core::widgets::severity::UnknownLevel> {
    spec::SEVERITY.ranks(spec::ALARMS.iter().map(|a| a.severity))
}

/// The alarms the feed shows, in the order it shows them.
///
/// The permutation is [`compute_order`]'s — the framework's 1-D sort/filter SSOT
/// since R747 — and everything this function adds is the two KEYS and the
/// threshold. Nothing here sorts.
fn alarm_order(state: &ShellState) -> Vec<usize> {
    let Ok(ranks) = alarm_ranks() else {
        return Vec::new();
    };
    // A floor already refused at the boundary cannot be stored, so a word here
    // that the scale does not hold is impossible rather than merely unlikely —
    // and if one ever were, `rank` answers `None` and the feed keeps everything,
    // which is the reading that hides nothing from a person.
    let floor = state
        .alarm_floor
        .get()
        .and_then(|word| spec::SEVERITY.rank(&word));
    let Some((column, ascending)) = state.alarm_sort.get() else {
        // Unsorted: the table's own order, narrowed. Kept as a case rather than
        // folded into `compute_order` with `None`, because a caller reading this
        // wants to see that an unsorted feed is still FILTERED.
        return (0..spec::ALARMS.len())
            .filter(|&i| SeverityScale::passes(floor, ranks[i]))
            .collect();
    };
    // One key type for three columns, and the unused components are held at a
    // constant rather than left to vary: a tuple whose earlier component moves
    // when it should not is a sort by a column nobody asked for.
    let key = |i: usize| -> (usize, u32, &'static str) {
        let alarm = &spec::ALARMS[i];
        match column {
            // ★ The severity column's key is the RANK and not the word: an
            // alphabetical order over `error / info / warn` is not an order
            // anybody means, and it is exactly what a string filter or a string
            // sort gives you. Ties fall back to the instant, so equal
            // severities read newest-first inside their band.
            0 => (ranks[i], alarm.seconds(), ""),
            // The instant, which is the STORED fact — not the rendered clock
            // string, whose lexical order is chronological only by luck.
            1 => (0, alarm.seconds(), ""),
            // The reading. `compute_order`'s own documented idiom for a 1-D
            // list is the row's display label as a `&str`.
            _ => (0, 0, alarm.message),
        }
    };
    compute_order(spec::ALARMS.len(), Some(ascending), key, |i| {
        SeverityScale::passes(floor, ranks[i])
    })
}

/// The feed assembly for a card body of `rect`, configured but not yet built.
///
/// ★ ONE constructor, called by the painter, by the accessibility walk and by
/// the gates. The window a reader is told about has to be the window that was
/// built, and three call sites configuring their own builder is three chances
/// for those to differ — which is the defect class this screen has measured most
/// often.
fn alarm_feed<'a>(
    tag: &'a str,
    rect: Rect,
    columns: &'a [FeedColumn<'a>],
    rows: usize,
) -> HeaderFeed<'a> {
    HeaderFeed::new(
        tag,
        rect,
        columns,
        HeaderFeedStyle::new(spec::ALARM_ROW_H)
            .with_header_text_px(FONT_SMALL)
            .with_header_height(spec::ALARM_HEAD_H)
            // ★ Whole rows, and NO overscan — the two go together. A feed in a
            // fixed box read as a table must not show a half row (its words are
            // clipped, so it looks present and cannot be read), and with the
            // body already an exact multiple of the pitch an overscan row would
            // be a constructed row with nothing visible in it — which every word
            // census over the paint would correctly report as a row painting
            // nothing.
            .with_whole_rows()
            .with_overscan(0),
        rows,
    )
}

/// The feed's columns at a body width, or `None` when the body cannot hold them.
///
/// ★ The stated widths, with the one declared `0` taking what the others leave —
/// and an ALL-OR-NOTHING floor under that remainder. Below
/// [`spec::ALARM_EVENT_FLOOR`] the feed draws nothing rather than three clipped
/// words, the same clamp the health strip and the latency tiles make: an alarm
/// row's severity, instant and reading are one statement, and two of the three
/// is a sentence a reader completes wrongly.
///
/// ⚠ Measured, not chosen: without the floor the `4 x 1` card shrunk to one cell
/// laid its columns out past its own body, and the ink gate reported seven marks
/// outside the box that owns them — a heading overhanging by 21px and five clock
/// readings by 12px each.
fn alarm_columns(width: u32) -> Option<Vec<FeedColumn<'static>>> {
    let stated: u32 = spec::ALARM_COLUMNS.iter().map(|(_, w)| *w).sum();
    let rest = width.saturating_sub(stated);
    if rest < spec::ALARM_EVENT_FLOOR {
        return None;
    }
    Some(
        spec::ALARM_COLUMNS
            .iter()
            .map(|(label, w)| FeedColumn::new(label, if *w == 0 { rest } else { *w }))
            .collect(),
    )
}

/// Where each alarm heading is, in the card's own space.
///
/// ★★★★★ The PAINT's rectangles, not a second set: the placements come from the
/// same [`alarm_feed`] builder the painter hands to
/// `pinion_widget_paint::header_feed`, and the strip's height from the same
/// style. This screen's most expensive recurring defect is a rectangle computed
/// twice ([[debt-paint-and-gesture-read-two-facts]]), and a header is exactly
/// where it bites: a heading drawn at one x and hit-tested at another is a
/// control that looks pressable and is not.
fn alarm_head_rects(body: Rect) -> Vec<(usize, Rect)> {
    // A body too narrow for the columns paints no header, so there is nothing
    // there to press — the gesture and the paint agree about that too.
    let Some(columns) = alarm_columns(body.w) else {
        return Vec::new();
    };
    let feed = alarm_feed("card.alarms.feed", body, &columns, 0);
    feed.placements()
        .into_iter()
        .map(|place| {
            (
                place.visual,
                Rect::new(body.x + place.x, body.y, place.size, spec::ALARM_HEAD_H),
            )
        })
        .collect()
}

/// The alarm card's body: a sortable severity header over a virtualised feed.
///
/// ★★★★★ R1851 — the composition, and every part of it comes from a crate.
/// `pinion_widget_paint::header_feed` assembles the strip and the window,
/// `pinion_core::widgets::severity` grades the rows and `compute_order` puts them
/// in order. What this function contributes is the ROW — a swatch, a level, a
/// clock reading and a message — which is the half a data grid cannot draw and
/// therefore the half that makes this a feed.
fn alarms_body(state: &ShellState, id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    let Ok(ranks) = alarm_ranks() else {
        // Unreachable while the gate below holds, and honest if it ever is not:
        // a feed that cannot grade its own rows says so instead of drawing an
        // ungraded list.
        return placeholder_body("alarms", id, rect, palette);
    };
    // ★ All or nothing: a body too narrow for the reading column draws no feed
    // rather than three clipped words. The strip's own words would fit; what
    // would not is the alarm, and an alarm card showing headings over nothing is
    // worse than an empty one.
    let Some(columns) = alarm_columns(rect.w) else {
        return Vec::new();
    };
    let order = alarm_order(state);
    let tag = format!("card.{id}.feed");
    let feed = alarm_feed(&tag, rect, &columns, order.len()).with_sort(state.alarm_sort.get());
    let window = feed.window(state.alarm_scroll.offset_y());
    vec![feed.build(
        &state.alarm_scroll,
        &use_theme(THEME_TAG).theme_animated(),
        |index, row, places| {
            let alarm = order
                .get(index)
                .map_or(&spec::ALARMS[0], |&n| &spec::ALARMS[n]);
            let rank = order.get(index).map_or(0, |&n| ranks[n]);
            let ink = severity_ink(rank, palette);
            let level = spec::SEVERITY.name(rank).unwrap_or("");
            let slot = index.saturating_sub(window.first);
            let line = pinion_core::containment::line_box(FONT_TINY);
            let top = (row.h.saturating_sub(line)) / 2;
            // ⚠ Each word sits in ITS OWN column, at the placement the header
            // used. That is the whole reason `build_row` is handed `places`: a
            // heading over a word the row put somewhere else is a heading that
            // names nothing.
            let cell = |n: usize| {
                places.get(n).map_or((0, 0), |p| {
                    (p.x + ALARM_INSET, p.size.saturating_sub(ALARM_INSET * 2))
                })
            };
            // ★★★★★ Each word is a tagged CELL of the row, because that is what
            // it is: this feed announces as a TABLE, and WAI-ARIA's structural
            // rule is that a `row` owns members of a cell role. The first draft
            // put the whole reading on the row and left the words anonymous, and
            // the structure gate refused it by name — eleven nodes, `row` empty
            // and `columnheader` stray. Tagged rather than anonymous so the
            // announcement has something to point AT: an announced tag nothing
            // paints is a name a reader can be sent to and not find.
            let words: [(String, Color); 3] = [
                (level.to_uppercase(), ink),
                (alarm.clock(), palette.muted),
                (alarm.message.to_owned(), palette.ink),
            ];
            let mut parts = vec![
                // The prototype's own stripe: three pixels down the row's
                // leading edge, in the severity's ink. Kept verbatim — it is
                // what a reader scanning for trouble actually uses, and it is
                // decoration beside a cell that says the same thing in words.
                Scene::Container(
                    ContainerNode::new(Vec::new())
                        .with_style(BoxStyle::filled(ink).with_corner_radius(2))
                        .with_layout(absolute(Rect::new(
                            0,
                            3,
                            ALARM_BAR_W,
                            row.h.saturating_sub(6),
                        ))),
                ),
            ];
            for (k, (text, fg)) in words.into_iter().enumerate() {
                let (x, w) = cell(k);
                parts.push(Scene::Container(
                    ContainerNode::new(vec![label(&text, Rect::new(0, 0, w, line), FONT_TINY, fg)])
                        .with_tag(format!("{tag}.row.{slot}.cell.{k}"))
                        .with_layout(absolute(Rect::new(x, top, w, line))),
                ));
            }
            Scene::Container(
                ContainerNode::new(parts)
                    // ★ `row.{slot}` and not `row#{slot}`: the `#` spelling is the
                    // router's composite-subindex convention (which is why the
                    // header's cells carry it — the crate emits them), and a feed
                    // row is not a router target. The dotted spelling is what this
                    // screen's body-row families are named in, so the gates that
                    // walk every card's rows walk this card's too.
                    .with_tag(format!("{tag}.row.{slot}"))
                    .with_layout(absolute(Rect::new(0, 0, row.w, row.h))),
            )
        },
    )]
}

/// The alarm feed's live state, as one value a client can read in a round trip.
///
/// ★★★★★ §2 #2 — the whole feed as data, which is the axis the reference has no
/// answer for. Probed on the toolkit floor at 6.11.1: a virtualised tabular view
/// over ten thousand rows reports ten thousand through its public surface and
/// publishes NO count of the rows it actually built — asking it which those are
/// does not compile. Here `built` is the window, `order` is the permutation, and
/// `vocabulary` is the closed list a client picks a threshold from instead of
/// guessing at spelling.
fn alarms_wire(state: &ShellState) -> serde_json::Value {
    let order = alarm_order(state);
    let columns = alarm_columns(alarm_body_width(state)).unwrap_or_default();
    let feed = alarm_feed(
        "card.alarms.feed",
        alarm_body_rect(state),
        &columns,
        order.len(),
    )
    .with_sort(state.alarm_sort.get());
    let window = feed.window(state.alarm_scroll.offset_y());
    serde_json::json!({
        "vocabulary": spec::SEVERITY.levels(),
        "floor": state.alarm_floor.get(),
        "sort": grid_sort_str(state.alarm_sort.get()),
        "total": spec::ALARMS.len(),
        "shown": order.len(),
        "in_reference": spec::ALARMS_IN_REFERENCE,
        // ⚠ The rows the feed CONSTRUCTED, by their place in `order` — which is
        // the fact the probe above could not get out of the reference at all.
        "built": (window.first..window.first + window.count).collect::<Vec<_>>(),
        "rows": order.iter().map(|&n| serde_json::json!({
            "at": spec::ALARMS[n].clock(),
            "seconds": spec::ALARMS[n].seconds(),
            "severity": spec::ALARMS[n].severity,
            "message": spec::ALARMS[n].message,
        })).collect::<Vec<_>>(),
    })
}

/// The rectangle the alarm card's body occupies, or an empty one when the board
/// does not hold that card.
///
/// One helper because the readers that need it should not re-derive it: the
/// wire and this card's own gates.
///
/// ⚠ R2022 — this said *"three readers … the wire, the accessibility walk and
/// the gates"*, and the walk stopped being one: [`alarms_nodes`] is handed the
/// rectangle the PAINTER was given ([`card_body_rect`]), which is card-local
/// where this is canvas-space. They agree in width and height on the board, so
/// nothing moved — but a sentence naming its readers goes stale the moment one
/// of them leaves, which is why the count is gone from it.
fn alarm_body_rect(state: &ShellState) -> Rect {
    let board = state.board.get();
    spec::card_of("alarms")
        .and_then(|id| state.card(&id))
        .and_then(|card| board.tile(card.id()).map(cell_rect))
        .map_or(Rect::default(), |cell| body_rect(cell, state.editing.get()))
}

/// The width of that rectangle.
fn alarm_body_width(state: &ShellState) -> u32 {
    alarm_body_rect(state).w
}

/// The alarm feed, as things an assistive reader can be told.
///
/// ⚠ The window is asked of the SAME builder the painter uses
/// ([`alarm_feed`]), so a reader cannot be told about a row nobody constructed —
/// which is the exact defect R1843 shipped on the health strip (three tiles
/// painted, five announced) and R1846 had to repair.
fn alarms_nodes(state: &ShellState, card: &Card, rect: Rect) -> Vec<AccessNode> {
    let id = card.id().as_str();
    if state.board.get().tile(card.id()).is_none() {
        return Vec::new();
    }
    let Ok(ranks) = alarm_ranks() else {
        return Vec::new();
    };
    // ★ R2022 — the rectangle the PAINTER was handed, rather than
    // [`alarm_body_rect`]'s own re-derivation from the tile. The two agree in
    // width and height on the board and disagree in ORIGIN (the wire's is
    // canvas-space, the painter's is card-local), and this one is the painter's,
    // which is what makes the pair drivable at a size of a test's choosing.
    // ⚠ The SAME refusal the painter makes. A card too narrow for the feed paints
    // nothing, so it announces nothing — a reader told about rows nobody drew is
    // exactly the defect R1846 had to repair on the health strip.
    let Some(columns) = alarm_columns(rect.w) else {
        return Vec::new();
    };
    let order = alarm_order(state);
    let tag = format!("card.{id}.feed");
    let sort = state.alarm_sort.get();
    let feed = alarm_feed(&tag, rect, &columns, order.len()).with_sort(sort);
    let window = feed.window(state.alarm_scroll.offset_y());

    let mut nodes = Vec::new();
    // ★★★★★ A TABLE, and the row counts are the WHOLE feed rather than the
    // window.
    //
    // This is the half of the composition a virtualised list cannot state on its
    // own and the reference toolkit has no answer for at all: `aria-rowcount` is
    // eighteen alarms plus the heading row, and each row carries its
    // `aria-rowindex` inside that, so a reader is told *row 5 of 19* while four
    // rows exist in the tree. Probed at 6.11.1, a virtualised tabular view there
    // reports its MODEL's count and publishes nothing about the rows it built.
    //
    // ⚠ The rows hang off the TABLE and not off a node for `{tag}.body`. That
    // container is the scrolling frame and it is declared quiet: a clip is not a
    // thing on the screen, and announcing it would put a step between a reader
    // and the rows for no fact gained.
    //
    // ★★★★★ R1856 — this comment used to say "*this screen* declares it quiet",
    // and no screen did. R1851 left the declaration an opt-in on the assembly
    // and this caller never took it, so the frame and the clip inside it went
    // out UNDECIDED — a region a reader is never told exists and no author
    // chose. The declaration now lives in `HeaderFeed::build`, which is the only
    // place that can state a reason true of every feed, and cannot be omitted.
    // The comment names where it lives because a reader here will look for it.
    let mut table = AccessNode::new(tag.clone(), AriaRole::Table)
        .with_name(format!(
            "Alarms, {} of {} shown, {}",
            order.len(),
            spec::ALARMS.len(),
            match state.alarm_floor.get() {
                Some(word) => format!("{word} and above"),
                None => "every severity".to_string(),
            }
        ))
        .with_row_count(u(order.len()) + 1)
        .with_column_count(u(columns.len()));
    let head_tag = format!("{tag}.head");
    table = table.with_child(head_tag.clone());
    // The heading strip IS a row — WAI-ARIA's rule, not a stylistic choice: a
    // `columnheader` is a member of a `row`, and a heading attached anywhere
    // else is a heading of nothing.
    let mut head = AccessNode::new(head_tag, AriaRole::Row)
        .with_name("Alarm columns")
        .with_row(0);
    // ★ Each heading says which way it is sorted, which is the fact a reader
    // scanning a feed most needs and the one a coloured arrow alone withholds.
    let mut heads = Vec::new();
    for (n, column) in columns.iter().enumerate() {
        let col_tag = format!("{tag}.head.col#{n}");
        head = head.with_child(col_tag.clone());
        let node = AccessNode::new(col_tag, AriaRole::ColumnHeader)
            .with_name(column.label)
            .with_column(n);
        heads.push(match sort.filter(|(col, _)| *col == n) {
            Some((_, ascending)) => node.with_sort(SortDirection::from_ascending(ascending)),
            None => node,
        });
    }
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    for slot in 0..window.count {
        let Some(&n) = order.get(window.first + slot) else {
            continue;
        };
        let alarm = &spec::ALARMS[n];
        let level = spec::SEVERITY.name(ranks[n]).unwrap_or("");
        let row_tag = format!("{tag}.row.{slot}");
        table = table.with_child(row_tag.clone());
        let mut row = AccessNode::new(row_tag.clone(), AriaRole::Row)
            .with_name(format!("{level} at {}", alarm.clock()))
            // ★ The whole reading on the row as well as in its cells, so a
            // reader who cannot see the stripe is told the severity in words and
            // gets the row in one value rather than having to assemble it from
            // three leaves.
            .with_value(AccessValue::Text(format!(
                "{}, {level}, {}",
                alarm.clock(),
                alarm.message
            )))
            // Where this row is in the WHOLE feed, not in the window — which is
            // the fact a window otherwise withholds.
            .with_row(window.first + slot + 1);
        for (k, word) in [level, &alarm.clock(), alarm.message]
            .into_iter()
            .enumerate()
        {
            let cell_tag = format!("{row_tag}.cell.{k}");
            row = row.with_child(cell_tag.clone());
            cells.push(
                AccessNode::new(cell_tag, AriaRole::Cell)
                    .with_name(columns.get(k).map_or("", |c| c.label))
                    .with_value(AccessValue::Text(word.to_owned()))
                    .with_column(k),
            );
        }
        rows.push(row);
    }
    nodes.push(table);
    nodes.push(head);
    nodes.extend(heads);
    nodes.extend(rows);
    nodes.extend(cells);
    nodes
}

/// The health card's tiles, as things an assistive reader can be told.
///
/// One [`AriaRole::Status`] per tile carrying its reading, under a group — the
/// shape the latency card's strip already uses, so the two read alike. A series
/// nobody can be read out is announced as the value it ends at, which is the
/// fact rather than the picture.
fn health_nodes(card: &Card, body: Rect) -> Vec<AccessNode> {
    let id = card.id().as_str();
    // ⚠ The SAME rule the painter runs, over the same rectangle. ★ R2022 — the
    // rectangle now comes from [`card_body_rect`] rather than being re-derived
    // here from the tile: this function's own copy read the board's cell even
    // for a card that had been maximised or torn off, and answered about a size
    // the card no longer had.
    let Some(count) = health_tile_count(body.w) else {
        return Vec::new();
    };
    let mut nodes = Vec::new();
    let mut group =
        AccessNode::new(format!("card.{id}.tiles"), AriaRole::Group).with_name("Health");
    for (n, tile) in spec::HEALTH_TILES[..count as usize].iter().enumerate() {
        let tag = format!("card.{id}.stat.{n}");
        group = group.with_child(tag.clone());
        // ★★★★★ R2002 — the name is the HEADING the tile paints, through the
        // one derivation both readers share. It was `tile.label` with the unit
        // moved into the value, which meant the ink said `Rate /s` and the name
        // said `Rate`: the tile's own label face declares itself that name, so
        // this is label-in-name and it was broken. The unit leaves the value in
        // the same move — it is stated once now, on the quantity, which is the
        // arrangement the painted tile already argued for.
        nodes.push(
            AccessNode::new(tag, AriaRole::Status)
                .with_name(tile_heading(tile))
                .with_value(AccessValue::Text(format!(
                    "{}, {} since the previous window",
                    tile.value, tile.delta
                ))),
        );
    }
    nodes.insert(0, group);
    nodes
}

/// A kind with no body painter: its code and its one line, which is what the
/// palette already told the person who placed it.
fn placeholder_body(kind: &str, id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    let def = def_of(kind);
    vec![
        code_chip(
            def.map_or("", |d| d.code),
            Rect::new(rect.x + 12, rect.y + 10, 40, 32),
            BoxStyle::filled(kind_color(kind)).with_corner_radius(6),
            palette.on_accent,
            Some(format!("card.{id}.code")),
        ),
        label(
            def.map_or("", |d| d.gist),
            Rect::new(rect.x + 62, rect.y + 18, rect.w.saturating_sub(74), 16),
            FONT_BODY,
            palette.muted,
        ),
    ]
}

fn state_sentence(state: &CardState) -> String {
    match state {
        CardState::Ready => "showing content".to_string(),
        CardState::Loading => "loading\u{2026}".to_string(),
        CardState::Empty => "nothing matched this filter".to_string(),
        CardState::Failed(why) => format!("could not load: {why}"),
        CardState::Denied(what) => format!("not permitted: {what}"),
        CardState::Opaque => "link is encrypted; content unavailable".to_string(),
    }
}

const fn remedy_label(remedy: Remedy) -> &'static str {
    match remedy {
        Remedy::Wait => "waiting",
        Remedy::Retry => "Retry",
        Remedy::Widen => "Widen filter",
        Remedy::Authorize => "Request access",
        Remedy::Nothing => "nothing can be done",
    }
}

/// The size-stepper strip layout-edit mode puts on every card.
fn edit_bar_scene(card_id: &str, bar: Rect, cell: (u32, u32), palette: Palette) -> Scene {
    // ★★★★★ R1851 — every run in this strip is placed in a box that holds its own
    // FACE, and before this round not one of them was.
    //
    // Measured by a per-card zero gate written for the alarm card: the four
    // stepper glyphs, the two axis letters and the size reading were all authored
    // into 14px boxes for faces needing 18 and 20 — seven short runs per card,
    // in every editing state, at every size. They were invisible because the
    // screen-wide gate is a RATCHET over a population measured before it existed,
    // where seven runs of one band sit under the noise; what made them visible
    // was asking the question of ONE card, where the answer can be zero.
    //
    // ⚠ This is what a seventh card would otherwise have COST: seven more short
    // runs, pushing a ratchet over its budget for a reason that has nothing to do
    // with the card. The repair takes the count down for all of them.
    // ⚠ And the stepper's glyph is set one step SMALLER, which is a measurement
    // rather than a taste. The button is 20px tall with a 1px border, so the box
    // that owns its ink is 18px (R1672 made containment read the CONTENT box,
    // because a border is ink the box owns inside itself); a 12px face needs a
    // 20px line and overhung that content box by a pixel at each end. Either the
    // button grows or the face gives way, and the button's size is the strip's
    // own geometry — so the face gives way.
    let small = pinion_core::containment::line_box(FONT_SMALL);
    let mut children = Vec::new();
    for (n, (verb, glyph)) in STEPPERS.iter().enumerate() {
        let slot = stepper_rect(bar, u(n));
        children.push(Scene::Container(
            ContainerNode::new(vec![label(
                glyph,
                Rect::new(6, slot.h.saturating_sub(small) / 2, 14, small),
                FONT_SMALL,
                palette.ink,
            )])
            .with_tag(format!("card.{card_id}.{verb}"))
            .with_style(
                BoxStyle::filled(palette.high)
                    .with_corner_radius(4)
                    .with_border(Border::new(palette.outline, 1)),
            )
            .with_layout(absolute(Rect::new(
                slot.x - bar.x,
                slot.y - bar.y,
                slot.w,
                slot.h,
            ))),
        ));
    }
    let axis_y = (bar.h.saturating_sub(small)) / 2;
    children.push(label(
        "W",
        Rect::new(58, axis_y, 12, small),
        FONT_SMALL,
        palette.muted,
    ));
    children.push(label(
        "H",
        Rect::new(136, axis_y, 12, small),
        FONT_SMALL,
        palette.muted,
    ));
    children.push(label(
        &format!("{}\u{00D7}{}", cell.0, cell.1),
        Rect::new(bar.w.saturating_sub(48), axis_y, 40, small),
        FONT_SMALL,
        palette.accent_fg,
    ));
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("card.{card_id}.editbar"))
            .with_style(BoxStyle::filled(palette.raised))
            .with_layout(absolute(bar)),
    )
}

/// Declare that `node` occupies its parent's chrome band of `role`. R1674.
///
/// One place, because the claim has to reach every mark in a band and a band is
/// several siblings — a grip, a title, a status pill, two buttons. A per-call
/// `.with_layout(...)` at each of those sites is five chances to miss one, and
/// a missed one reports as a mark that landed on the header rather than as a
/// mark that IS the header.
fn in_chrome(node: Scene, role: ChromeRole) -> Scene {
    let mut node = node;
    if let Some(layout) = node.layout_style_mut() {
        *layout = layout.clone().with_chrome_slot(role);
    }
    node
}

/// ★★ R1696 — a region is a keyboard stop **because the specification says so**.
///
/// The declaration is read rather than restated: `spec::FOCUS_RING` is the ring,
/// and this is the only place the paint consults it, so a stop added to the
/// table appears on screen and a stop removed from it disappears. Writing
/// `.with_focusable(true)` at each painter would be five independent decisions
/// and the table would become a description of what somebody remembered.
fn keyboard_stop(node: Scene, tag: &str, at: &str) -> Scene {
    let declared = spec::FOCUS_RING
        .iter()
        .any(|stop| stop.tag == tag && stop.at.shows_at(at));
    node.with_focusable(declared)
}

/// ★ R1721 — the three booleans that say how a card is being SHOWN, as one
/// value.
///
/// They arrived at `card_scene` as three separate arguments and the round that
/// threaded the state through the paint made that eight, which clippy's
/// `too_many_arguments` caught. Grouping them is the right repair rather than an
/// allow: "selected, being edited, maximised" is one question about this card at
/// this frame, and a caller cannot now pass two of the three and forget the
/// third.
/// ★ R1900 — no longer `Copy`: a shared cell's strip is a list, and the
/// compiler saying so is the type admitting that "what this card looks like"
/// stopped being four machine words.
#[derive(Clone)]
struct CardFace {
    /// The board's selection rests on this card.
    selected: bool,
    /// The board is in layout-editing mode, so the card shows its edit bar.
    editing: bool,
    /// This card is the one wearing the restore face of the maximise control.
    maximized: bool,
    /// ★ R1900 — everyone sharing this card's cell, in strip order, when it is
    /// shared with anybody. Empty otherwise, and the header then draws a title
    /// as it always did.
    ///
    /// On the FACE rather than looked up inside, because a face is what a card
    /// looks like right now and this is exactly that — and because the caller
    /// already has the cell in hand, so looking it up again would be a second
    /// read of a value that can change between them.
    sharing: Vec<TileId>,
    /// Which of [`Self::sharing`] is in front. `0` when nothing is shared.
    fore: usize,
}

fn card_scene(
    state: &ShellState,
    card: &Card,
    rect: Rect,
    cell: (u32, u32),
    palette: Palette,
    face: CardFace,
) -> Scene {
    let CardFace {
        selected,
        editing,
        maximized,
        sharing,
        fore,
    } = face;
    let inside = local(rect);
    // ★★ R1674 — the header's marks say they are IN the header band, so the
    // containment check judges them against that band and not against the
    // card's content rectangle. Both halves are needed and neither works
    // alone: without the card's `Chrome::header` declaration the band does not
    // exist to be judged against, and without these claims declaring it would
    // report the card's own title as an escape.
    //
    // What this buys, beyond silence: a mark that lands on the title strip
    // WITHOUT claiming it now arrives as `trespass: ["chrome:header"]` instead
    // of as an undifferentiated overhang, so a reader is told which band was
    // invaded. The floor has no form for that — its whole reservation is four
    // integers with the reason discarded.
    let mut children: Vec<Scene> = header_scene(
        card,
        header_rect(inside),
        palette,
        maximized,
        &sharing,
        fore,
    )
    .into_iter()
    .map(|node| in_chrome(node, ChromeRole::Header))
    .collect();
    children.extend(body_scene(state, card, body_rect(inside, editing), palette));
    if editing {
        children.push(in_chrome(
            edit_bar_scene(card.id().as_str(), edit_bar_rect(inside), cell, palette),
            ChromeRole::Footer,
        ));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("card.{}", card.id().as_str()))
            .with_style(card_style(palette, selected, editing))
            .with_layout(absolute(rect)),
    )
}

/// A card's skin: its fill, its outline, and **the bands of itself it keeps**.
///
/// ★★ R1674 — the header strip and the edit bar are now DECLARED chrome
/// ([`pinion_core::style::Chrome`]) rather than two rectangles this file works
/// out privately. The card's content rectangle is therefore the region its rows
/// actually get, and `scene/containment` can say `chrome:header` about a mark
/// that landed on the title strip instead of the undifferentiated "out of
/// bounds" that was the only available answer.
///
/// This is where the floor stops: probed at 6.11, a custom-painted widget can
/// publish the same INSET through a four-integer content-margin setter, and
/// reading it back gives four integers with the reason gone — a caption band
/// and a three pixel border are the same four numbers there. A card header is not one of
/// the ~20 complex controls with their own sub-control enum, so there is no
/// second way to ask either.
fn card_style(palette: Palette, selected: bool, editing: bool) -> BoxStyle {
    // The selection ring: one card is the keyboard's subject and a person has
    // to see which. Accent on the border rather than a different fill, so a
    // selected card that is also failing still reads as failing.
    let border = if selected {
        Border::new(palette.accent_fg, CARD_RING)
    } else {
        Border::new(palette.outline, CARD_OUTLINE)
    };
    // ★★ The band extents are measured from the card's EDGE and `content_of`
    // subtracts the border before them, so each declaration is the distance to
    // the band's far side LESS the border currently drawn.
    //
    // The subtlety is [`CARD_FRAME`], which is deliberately the WIDER of the
    // two borders and constant, so a card's rows do not shift by a pixel the
    // moment a person selects it. The card therefore reserves more than it
    // draws when it is not selected, and a declaration that ignored that would
    // be a pixel out — measured, exactly that: three edit bars reported one
    // pixel above their own footer band, by a check that had just been told
    // where the band was. Deriving both sides from the same two constants is
    // what makes the placement functions and this one incapable of disagreeing,
    // which `r1674_the_declared_bands_are_the_placed_bands` then asserts.
    let mut style = BoxStyle::filled(palette.panel)
        .with_corner_radius(10)
        .with_border(border)
        .with_chrome(Chrome::header(CARD_HDR.saturating_sub(border.width)));
    if editing {
        // The size-stepper strip only exists in layout-edit mode, so the
        // content rectangle is smaller in that mode — a fact about this card
        // that no reader could previously obtain.
        style = style.with_chrome(Chrome::new(
            ChromeEdge::Bottom,
            EDIT_BAR_H + CARD_FRAME - border.width,
            ChromeRole::Footer,
        ));
    }
    style
}

/// A detached panel, floating over the canvas.
fn float_scene(state: &ShellState, float: &Float, palette: Palette) -> Option<Scene> {
    let card = state.card(&float.id)?;
    let rect = float_rect(float);
    let inside = local(rect);
    let header = header_rect(inside);
    let colour = kind_color(kind_of(&float.id));
    let mut children = vec![
        dot(14, header.y + CARD_HDR / 2 - 4, 9, colour),
        label(
            card.title(),
            Rect::new(30, header.y + 9, header.w.saturating_sub(200), 16),
            FONT_BODY,
            palette.ink,
        ),
        // The badge that says this panel is not on the board.
        Scene::Container(
            ContainerNode::new(vec![label(
                "DETACHED",
                Rect::new(9, 4, 66, 12),
                FONT_TINY,
                palette.muted,
            )])
            .with_tag(format!("float.{}.badge", float.id))
            .with_style(
                BoxStyle::filled(palette.raised)
                    .with_corner_radius(4)
                    .with_border(Border::new(palette.outline, 1)),
            )
            .with_layout(absolute(Rect::new(
                header.w.saturating_sub(160),
                header.y + 8,
                84,
                20,
            ))),
        ),
    ];
    // ★★★★★ R1907 — the header's controls, from the SAME roster the hit test
    // walks. The marks are chosen by the affordance rather than by position, so
    // a host whose policy drops `send_home` draws two and answers for two
    // without either side being told a number.
    children.extend(float_affordances().iter().enumerate().map(|(n, offered)| {
        let slot = float_affordance_rect(header, n);
        let mark = match offered {
            DetachedAffordance::SendHome => send_home_mark(local(slot), palette.muted),
            DetachedAffordance::Redock => redock_mark(local(slot), palette.muted),
            DetachedAffordance::Close => close_mark(local(slot), palette.muted),
        };
        Scene::Container(
            ContainerNode::new(vec![mark])
                .with_tag(format!("float.{}.{}", float.id, offered.wire()))
                .with_layout(absolute(slot)),
        )
    }));
    children.extend(body_scene(state, &card, body_rect(inside, false), palette));
    // ★★ R1697 — the corner, painted last so it is over the body it sits in,
    // and from the SAME function the hit test reads. An affordance a person is
    // meant to grab has to be visible: R1690's lesson is that a capability
    // built and not drawn is one no gate can see and nobody can use.
    let grip = Rect::new(
        inside.w.saturating_sub(FLOAT_GRIP),
        inside.h.saturating_sub(FLOAT_GRIP),
        FLOAT_GRIP,
        FLOAT_GRIP,
    );
    children.push(Scene::Container(
        ContainerNode::new(vec![resize_mark(local(grip), palette.muted)])
            .with_tag(format!("float.{}.resize", float.id))
            .with_layout(absolute(grip)),
    ));
    Some(Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("float.{}", float.id))
            .with_style(
                BoxStyle::filled(palette.panel)
                    .with_corner_radius(10)
                    .with_border(Border::new(palette.accent_fg, 1)),
            )
            .with_layout(absolute(rect)),
    ))
}

/// ★★★★★ R1826 — what a detached card's OWN WINDOW paints: that card, filling
/// it.
///
/// The body comes from [`body_scene`], the same function the board and the
/// in-canvas float both call, so a card does not become a different card by
/// leaving the board — which is the property a tear-off is FOR. What it does
/// not carry is the float's chrome: a redock mark, a close mark and a resize
/// grip are how a panel inside a canvas is moved, closed and sized, and a real
/// window has an operating system for all three. Drawing them again would be
/// the two-of-everything defect this application already refuses one level up
/// (`pinion_core::chrome`).
///
/// The frame between a redock and the topology catching up paints this window's
/// GROUND and nothing else — no badge, no title, no body. The window is about to
/// be dropped, and a frame that drew the card as though nothing had happened
/// would be the one frame that lies.
///
/// 🟥 That sentence was written here before it was true, and this round's
/// closing audit is what caught it. The first draft keyed the content on
/// `ShellState::card`, which answers for every card ON THE BOARD whether or not
/// it is detached — so a re-docked card went on being painted, at the default
/// float size, by a window whose whole subject had left. It is keyed on the
/// FLOAT now, which is the fact the topology itself is keyed on, so a window and
/// its content cannot disagree about whether this card is detached.
fn torn_window_scene(card_id: &str) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_shell_state();
    let dark = theme_word(&state.theme) == "dark";
    let palette = palette_of(&theme, dark);
    // The float's own declared size is the window's, so the scene is authored
    // at the size the topology asked the operating system for. And its ABSENCE
    // is the whole answer — see the header.
    //
    // ★★★★★ R1838 — this comment used to end "rather than at whatever
    // `window_size()` reports for the MAIN window", and that clause was a
    // FRAMEWORK DEFECT being worked around in prose. It was true: every
    // window's `layout_size` answered the primary's extent, so a binding that
    // did not hand-author its own size laid a torn-off window out at the main
    // window's — and maximising the main window spread it further out of its
    // own edges every time. This screen dodged it and wrote the dodge down
    // instead of reporting it. `pinion_core::external::with_window_extent` is
    // the repair, and `window_size()` would now answer this window; the float's
    // declared size is still read here because it is the size the TOPOLOGY
    // asked for, which is the fact this scene is about.
    let Some((w, h)) = state.float(card_id).map(|f| (f.w.max(1), f.h.max(1))) else {
        return Scene::Container(
            ContainerNode::new(Vec::new())
                .with_tag(format!("torn.{card_id}"))
                .with_style(BoxStyle::filled(palette.canvas))
                .with_layout(absolute(Rect::new(0, 0, FLOAT_W, FLOAT_H))),
        );
    };
    let rect = Rect::new(0, 0, w, h);
    let mut children = vec![Scene::Container(
        ContainerNode::new(vec![label(
            "DETACHED",
            Rect::new(9, 4, 66, 12),
            FONT_TINY,
            palette.muted,
        )])
        .with_tag(format!("torn.{card_id}.badge"))
        .with_style(
            BoxStyle::filled(palette.raised)
                .with_corner_radius(4)
                .with_border(Border::new(palette.outline, 1)),
        )
        .with_layout(absolute(Rect::new(10, 10, 84, 20))),
    )];
    if let Some(card) = state.card(card_id) {
        children.push(label(
            card.title(),
            Rect::new(104, 13, w.saturating_sub(114), 16),
            FONT_BODY,
            palette.ink,
        ));
        let body = Rect::new(10, 40, w.saturating_sub(20), h.saturating_sub(50));
        children.extend(body_scene(&state, &card, body, palette));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("torn.{card_id}"))
            .with_style(BoxStyle::filled(palette.canvas))
            .with_layout(absolute(rect)),
    )
}

/// R1668 — the reads that answer from the SPECIFICATION rather than from the
/// shell's state.
///
/// Grouped because that is the distinction: nothing here can change while the
/// application runs, so a client that caches one of these is right to.
fn read_specification(path: &str) -> Result<IntrospectValue, ReadRefusal> {
    match path {
        "spec" => Ok(IntrospectValue::Json(spec_json())),
        "rail" => Ok(IntrospectValue::Text(
            spec::RAIL
                .iter()
                .map(|seat| seat.key)
                .collect::<Vec<_>>()
                .join(","),
        )),
        // The seats a later release opens, and what each is booked under. A
        // locked seat that could only be SEEN is a screenshot; one that says
        // what it is waiting for is a specification.
        "reserved_rail" => Ok(IntrospectValue::Json(serde_json::Value::Array(
            spec::RAIL
                .iter()
                .filter_map(|seat| {
                    seat.reserved_for()
                        .map(|why| serde_json::json!({ "key": seat.key, "reserved_for": why }))
                })
                .collect(),
        ))),
        // The thirteen kinds, so a client picks from what is offered.
        "catalogue" => Ok(IntrospectValue::Text(
            spec::CATALOGUE
                .iter()
                .map(|w| w.kind)
                .collect::<Vec<_>>()
                .join(","),
        )),
        // ★★★★★ R1728 — **the application says how much of its own
        // specification it reproduces**, and where it does not.
        //
        // Not a test fixture: this is the sentence an agent driving the tool
        // needs before it plans anything. "Open the log section" is a
        // reasonable next move and the answer is *no, and here is why, and it
        // is not the same why as the two seats that are waiting for a later
        // release*. Nothing on the wire could say that — `destinations` reports
        // the standing of what IS on the rail, and a seat missing from the rail
        // reports nothing at all, which is why three invented keys and two
        // absent ones went unremarked for several hundred rounds.
        //
        // The comparison runs against `docs/analyzer-rail-spec.json`, which is
        // a reviewed artifact rather than a second copy of the table above.
        "conformance" => {
            // ★ R1953 — the live difference is `spec::rail_divergence()`, the
            // one derivation the two gates read as well. This computed it a
            // third time, and a fact spelled three times is a fact that moves
            // in two of them.
            let found = spec::rail_divergence();
            let divergences: Vec<serde_json::Value> = found
                .iter()
                .map(|d| serde_json::json!({ "key": d.key(), "says": d.sentence() }))
                .collect();
            // ★★★★★ R1953 — **the wire mirrors the specification's TWO lists.**
            //
            // This published one, named `owed`, meaning *the reference has this
            // seat and this build has not written it*. R1947 and R1948 wrote
            // entries meaning the reverse into that array, and when R1953 split
            // them by meaning this field became honestly empty — and by itself
            // it then told an agent the tool differs from the reference nowhere
            // by declaration, while two declared differences existed. A field
            // that goes empty because its meaning was narrowed is a field that
            // has to gain its counterpart in the same edit.
            let entries = |ledger: &pinion_core::conformance::Ledger| -> Vec<serde_json::Value> {
                ledger
                    .owed()
                    .iter()
                    .map(|o| {
                        serde_json::json!({
                            "key": o.key,
                            "says": o.sentence,
                            "since": o.since,
                            "why": o.why,
                        })
                    })
                    .collect()
            };
            let owed = entries(&spec::owed());
            let ahead = entries(&spec::ahead());
            let specified = spec::canon_spec().len();
            // ★★★★★ R1946 — **the distance from the OTHER reference, on the
            // same surface.**
            //
            // Everything above answers for the scope mockup, and its answer is
            // full marks: eight specified, eight reproduced, nothing owed. It
            // was that answer, with no second one beside it, that let this
            // build report conformance while two sections a person went looking
            // for did not exist. They are not divergences from the mockup — it
            // locks both seats and so does this build — they are seats the
            // working reference BUILDS and this one has not.
            //
            // An agent asking this path how far the tool is from the reference
            // now gets both numbers, and the smaller one is the honest one.
            let behind: Vec<serde_json::Value> = spec::second_phase_owed_declared()
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "key": entry.key,
                        "since": entry.round,
                        "why": entry.reason,
                    })
                })
                .collect();
            let builds = spec::behaviour_built().len();
            Ok(IntrospectValue::Json(serde_json::json!({
                "specified": specified,
                "reproduced": specified - divergences.len(),
                "divergences": divergences,
                "owed": owed,
                "ahead": ahead,
                // ★★ R1953 — whether the difference this rail HAS is the
                // difference it DECLARES, answered by the application rather
                // than left for a reader to work out by comparing the arrays
                // above. Two lists side by side are two lists; an agent asking
                // *can I trust the declared remainder* was being handed the
                // comparison instead of its result.
                "reconciles": spec::divergences().reconciles(&found),
                "behaviour": {
                    "builds": builds,
                    "reproduced": builds - behind.len(),
                    "owed": behind,
                },
            })))
        }
        // Unreachable through the caller's match, which routes exactly the
        // five paths above. Stated rather than unwrapped: a sixth path added to
        // that arm and not to this one refuses in the schema's own vocabulary
        // instead of panicking a shell.
        _ => Err(ReadRefusal::UnknownPath),
    }
}

/// R1697 — the operations table, on the wire.
///
/// Each row carries the introspection slot that must move once the operation
/// has run, which is what turns the table from a list of names into something
/// a client can check for itself: read the slot, cause the operation the way
/// the row says it can be caused, read the slot again.
fn operations_json() -> serde_json::Value {
    serde_json::Value::Array(
        spec::OPERATIONS
            .iter()
            .map(|op| {
                serde_json::json!({
                    "name": op.name,
                    "verb": op.verb.map(|(action, arg)| serde_json::json!([action, arg])),
                    "gesture": op.gesture,
                    "witness": op.witness,
                    "needs": op.needs,
                })
            })
            .collect(),
    )
}

/// R1668 — the reference screen C, as the wire hands it to a client.
///
/// Every field is read straight out of `spec`, so the running application and
/// the gate compare against one table rather than two spellings of it.
/// ★★★★★ R1733 — the two slots that describe a board drag in flight.
///
/// * `drag` — where a release would put what is carried. Empty when nothing is
///   being carried, which is a different answer from a carry hovering over cell
///   0,0 — **and empty ALSO when something is carried off the board**, because
///   then there is no cell a release would use.
/// * `carrying` — WHAT is being carried, and whether it is already on the
///   board.
///
/// Two names rather than one string with a sentinel in it: a reader asking
/// *where would it land* and a reader asking *is anything held* are asking
/// different questions, and one answer for both is how a client ends up parsing
/// for emptiness. The second is the half a wire reader could not see at all — a
/// palette footprint in flight is not a tile, not a card and not in `layout`,
/// so before this round every question about it had the same answer as
/// "nothing is happening". The floor cannot answer it either: a drag there
/// lives inside the source widget's own event loop.
fn carry_slot(state: &ShellState, path: &str) -> IntrospectValue {
    let held = state.drag.get();
    IntrospectValue::Text(match (path, held) {
        (_, None) => String::new(),
        ("carrying", Some(drag)) => format!(
            "{}:{}",
            if drag.carried().is_placed() {
                "placed"
            } else {
                "fresh"
            },
            drag.carried().id(),
        ),
        (_, Some(drag)) => drag
            .landing()
            .map(|(col, row)| format!("{},{col},{row}", drag.carried().id()))
            .unwrap_or_default(),
    })
}

/// ★★★★★ R1898 — **what letting go right now would do**, as a value.
///
/// `null` when no gesture is in flight, which is a different answer from a
/// gesture that would do nothing: the second says so with `passage` and a
/// reason.
///
/// The whole crossing is published — which side it began on, which side the
/// pointer rests on, and the verdict — because a client that could read only
/// the verdict could not tell "this drag does not dock" from "the pointer is
/// not over the board yet", and those call for different next moves.
fn crossing_json(state: &ShellState) -> serde_json::Value {
    let Some(crossing) = state.crossing.get() else {
        return serde_json::Value::Null;
    };
    let rest = match crossing.rest() {
        Rest::Inside { col, row } => serde_json::json!({"side": "inside", "col": col, "row": row}),
        Rest::Outside { x, y } => serde_json::json!({"side": "outside", "x": x, "y": y}),
    };
    let verdict = match crossing.passage() {
        Ok(passage) => serde_json::json!({
            "passage": passage.as_str(),
            "crosses": passage.crosses(),
        }),
        Err(refusal) => serde_json::json!({
            "passage": serde_json::Value::Null,
            "crosses": false,
            "refused": refusal.wire_word(),
            "because": refusal.reason().as_str(),
        }),
    };
    serde_json::json!({
        "unit": crossing.unit(),
        "began": crossing.began().as_str(),
        "policy": crossing.policy().as_str(),
        "rest": rest,
        "verdict": verdict,
    })
}

/// Every widget kind the palette offers, and what each row says about itself.
fn catalogue_json() -> serde_json::Value {
    serde_json::Value::Array(
        spec::CATALOGUE
            .iter()
            .map(|w| {
                serde_json::json!({
                    "kind": w.kind,
                    "code": w.code,
                    "label": w.label,
                    "gist": w.gist,
                    "section": w.section,
                    "tier": tier_word(w.tier),
                    "reserved_for": w.reserved_for,
                })
            })
            .collect(),
    )
}

/// ★★ R1733 — the board specification's surfaces, as a value a client reads.
///
/// Each surface names its parts in the specified order and the differences this
/// build has declared against them, each with the round that accepted it and
/// why. Read out of `docs/analyzer-board-spec.json` — the reviewed artifact —
/// rather than restated here, so the thing an agent is told and the thing the
/// gate judges against are one file.
fn carry_surfaces_json() -> serde_json::Value {
    let doc = spec::board_document();
    serde_json::Value::Array(
        doc.surfaces()
            .map(|surface| {
                let parts = doc.canon(surface).map(|canon| {
                    canon
                        .parts()
                        .iter()
                        .map(|part| serde_json::json!({ "key": part.key, "title": part.title }))
                        .collect::<Vec<_>>()
                });
                let owed = doc.ledger(surface).map(|ledger| {
                    ledger
                        .owed()
                        .iter()
                        .map(|entry| {
                            serde_json::json!({
                                "key": entry.key,
                                "says": entry.sentence,
                                "since": entry.since,
                                "why": entry.why,
                            })
                        })
                        .collect::<Vec<_>>()
                });
                serde_json::json!({
                    "surface": surface,
                    "parts": parts.unwrap_or_default(),
                    "owed": owed.unwrap_or_default(),
                })
            })
            .collect(),
    )
}

/// The declared silences, expanded per member.
///
/// ★ Extracted by R1819 for the same reason as the function below it, and the
/// two together are what made room: `spec_json`'s outer `json!` was already at
/// the macro's recursion budget, so ANY new key would have failed to build. A
/// published table that cannot grow is a surface that stops recording the
/// screen, and the limit says nothing about which key is at fault — it names
/// whichever nested block happens to be last.
fn silences_json() -> Vec<serde_json::Value> {
    spec::SILENCES
        .iter()
        .flat_map(|(tag, population, kind, at)| {
            population.members().into_iter().map(move |member| {
                serde_json::json!({
                    "tag": tag.replace("{}", &member),
                    "kind": kind,
                    "at": where_word(*at),
                })
            })
        })
        .collect()
}

/// What this screen says the pointer does, as the wire carries it.
///
/// ★ A function rather than an inline `map` for the reason `operations_json`
/// is one, and R1819 met that reason head on: adding four lines inline pushed
/// `serde_json::json!` past its recursion limit and the build refused. The
/// macro's depth is a real budget, and a published table is what spends it.
fn gestures_json() -> Vec<serde_json::Value> {
    spec::GESTURES
        .iter()
        .map(|(gesture, effect)| serde_json::json!({ "gesture": gesture, "effect": effect }))
        .collect()
}

fn spec_json() -> serde_json::Value {
    serde_json::json!({
        "window": { "w": spec::WIN_W, "h": spec::WIN_H },
        "metrics": {
            "app_bar_h": spec::APP_BAR_H,
            "sub_bar_h": spec::SUB_BAR_H,
            "rail_w": spec::RAIL_W,
            "palette_w": spec::PALETTE_W,
            "grid_cols": spec::GRID_COLS,
        },
        // ★★★★★ R1903 — the palette's placement, and what it may do. `at` is
        // where it IS, `opens` where it arrives — two facts, for the reason
        // R1900 gave one screen over: the same bit cannot say whether a panel
        // was put away by a person or came that way. `foldable` is the policy,
        // published so a client is told what the `palette` verb will accept
        // rather than having to be refused to find out.
        // ⚠ `palette_placement`, not `palette`: this document already publishes
        // a `palette` — the catalogue roster — and a second meaning under one
        // key is the shape this tree keeps paying for. Lifted into its own
        // function for the reason the lint gives, which is the same reason
        // `floating_ids` was lifted at R1738: adding one slot took this
        // builder a line over its budget.
        "palette_placement": palette_placement_json(),
        "source": spec::SOURCE,
        "transport": spec::TRANSPORT,
        "rate": spec::RATE,
        "preset": spec::PRESET,
        "board_verbs": spec::BOARD_VERBS,
        // ★ R1695 — `open` joined `reserved_for`, because a seat that is neither
        // this page nor booked for a release is a third thing and the old two
        // columns could not say it.
        "rail": spec::RAIL.iter().map(|seat| serde_json::json!({
            "key": seat.key,
            "title": seat.title,
            "reserved_for": seat.reserved_for(),
            "open": seat.reserved_for().is_none(),
        })).collect::<Vec<_>>(),
        "rail_active": spec::RAIL_ACTIVE,
        // ★ R1695 — the Settings destination.
        "options": spec::OPTIONS.iter().map(|o| serde_json::json!({
            "key": o.key, "title": o.title, "gist": o.gist,
            "group": o.group, "opens": o.opens,
        })).collect::<Vec<_>>(),
        "key_rows": spec::KEY_ROWS.iter().map(|r| serde_json::json!({
            "key": r.key, "title": r.title, "gist": r.gist,
            "verb": r.verb,
            // ★★★★★ R2042 — the SENTENCE the seat carries, which is what the
            // paint and the spoken refusal carry too. Publishing the raw
            // `Option` here would make the wire and the paint two readings of
            // one field — the class this round repaired one layer down — and a
            // walk comparing them would have had to know the fallback.
            "reserved_for": r.reserved_for.unwrap_or(spec::UNBOOKED),
            // And the fact itself, so a reader can tell "booked under a
            // requirement" from "booked under nothing" without parsing prose.
            "booked": r.reserved_for.is_some(),
        })).collect::<Vec<_>>(),
        "option_groups": spec::OPTION_GROUPS.iter().map(|(key, title)| serde_json::json!({
            "key": key, "title": title,
        })).collect::<Vec<_>>(),
        "themes": spec::THEMES,
        "theme_row": { "title": spec::THEME_ROW.0, "gist": spec::THEME_ROW.1 },
        // ★★ R1696 — where the Tab key stops, in the order it stops there, and
        // WHAT each stop holds. The last column is the part a tag cannot carry:
        // an agent reading this learns that landing on `shell.rail` puts it
        // among the tool's destinations rather than on one of them.
        // ★★★★★ R1910 — and `interior`, which is WHAT THE ARROWS DO here.
        //
        // Its absence is what cost three published rounds of red CI. A client
        // walking this ring could see that a stop published no roster and had
        // no way to learn whether that meant *the cursor here is spatial, ask
        // the active descendant* or *this stop is a single control and owes
        // nothing*. The sweep's `r1698` demo guessed the first, correctly for
        // the one stop that existed, and R1903 added a stop of the second kind.
        //
        // ⇒ a self-describing surface that leaves a client GUESSING is not
        // self-describing; the guess is a rule, and rules break silently when
        // the population grows. `owes_cursor` is published beside the word
        // rather than left for a client to derive from it, so the two cursor
        // arms answer one question the same way without every client
        // re-deriving which arms those are.
        "focus_ring": spec::FOCUS_RING.iter().map(|stop| serde_json::json!({
            "tag": stop.tag, "holds": stop.holds, "at": where_word(stop.at),
            "interior": stop.interior.wire(),
            "owes_cursor": stop.interior.owes_an_active_descendant(),
        })).collect::<Vec<_>>(),
        // ★★ R1697 — what this screen can be ASKED to do, published so an
        // agent reads the operations rather than discovering them by trying.
        "operations": operations_json(),
        // ★★★★★ R1819 — what this screen SAYS THE POINTER DOES, which it never
        // said at all until now. The last of the tool's three screens to
        // publish one, and the reason it mattered: the gate over this
        // population ran here over the EMPTY SET, which passes and reads
        // exactly like a screen that keeps every promise.
        //
        // ⚠ Not `operations` under another name. That is what the screen can be
        // asked to DO; this is what it TELLS A PERSON a drag does, and only the
        // second can be a lie to somebody looking at the screen. They overlap
        // deliberately and neither is derived from the other.
        "gestures": gestures_json(),
        // ★ R1797 — a section publishes the releases its ENTRIES occupy rather
        // than a single tier of its own. Both, when it holds both: the column
        // that used to be here could not say that, and this round is when a
        // section first does.
        "sections": palette_groups_json(),
        "catalogue": catalogue_json(),
        // ★ R1733 — what the palette panel SAYS, published beside what it
        // holds. Its line is now the reference's own — that a row is dragged
        // onto the canvas — and an agent that can only read the catalogue
        // learns the kinds and not that there are two ways to place one.
        "palette": { "title": spec::PALETTE_TITLE, "hint": spec::PALETTE_HINT },
        // ★★ R1733 — the board gesture's written specification, published.
        //
        // §2 #7: an agent driving this screen needs to know what a carry puts
        // on the board *before* it starts one, and which of those parts this
        // build has that the reference does not. The rail publishes the same
        // two halves for the same reason (R1728), and the reason is that a
        // specification nothing reads at run time is a document rather than a
        // contract.
        "carry_surfaces": carry_surfaces_json(),
        "placeable_count": spec::placeable_count(),
        "reserved_count": spec::reserved_count(),
        "board": spec::BOARD.iter().map(|p| serde_json::json!({
            "kind": p.kind, "col": p.col, "row": p.row, "cols": p.cols, "rows": p.rows,
        })).collect::<Vec<_>>(),
        "card_chrome": spec::CARD_CHROME,
        "stream_columns": spec::STREAM_COLUMNS.iter().map(|(name, w)| serde_json::json!({
            "name": name, "width": w,
        })).collect::<Vec<_>>(),
        "stream_rows": spec::STREAM_ROWS.iter().map(|(time, kind, name, len)| serde_json::json!({
            "time": time, "type": kind, "name": name, "len": len,
        })).collect::<Vec<_>>(),
        "decode_rows": spec::DECODE_ROWS.iter().map(|(depth, key, value)| serde_json::json!({
            "depth": depth, "key": key, "value": value,
        })).collect::<Vec<_>>(),
        "decode_selected": spec::DECODE_SELECTED,
        "decode_span": [spec::DECODE_SELECTED_SPAN.0, spec::DECODE_SELECTED_SPAN.1],
        // ★★★★★ R1797 — the latency card's DERIVATION; see `latency_wire`.
        "latency": latency_wire(),
        "map_rows": spec::MAP_ROWS.iter().map(|(id, path, seen)| serde_json::json!({
            "id": id, "resource": path, "first_seen": seen,
        })).collect::<Vec<_>>(),
        "map_unresolved": spec::MAP_UNRESOLVED,
        "filter_query": spec::FILTER_QUERY,
        "filter_chips": spec::FILTER_CHIPS.iter().map(|(name, on)| serde_json::json!({
            "name": name, "on": on,
        })).collect::<Vec<_>>(),
        "filter_stats": spec::FILTER_STATS.iter().map(|(value, what)| serde_json::json!({
            "value": value, "of": what,
        })).collect::<Vec<_>>(),
        // ★ R1694 — what reaches a reader who never sees the drawing, EXPANDED
        // here rather than listed, so a family that grows a member cannot be
        // satisfied by the members that were there when the table was written.
        // ★ R1695 — each row now says WHICH destination it belongs to, so a
        // client can ask the census about a page it is not looking at.
        "voices": spec::VOICES.iter().flat_map(|voice| {
            voice.population.members().into_iter().map(move |member| serde_json::json!({
                "tag": voice.tag.replace("{}", &member),
                "role": voice.role,
                "at": where_word(voice.at),
            }))
        }).collect::<Vec<_>>(),
        "silences": silences_json(),
        "locked": spec::LOCKED.iter().flat_map(|(tag, population, at)| {
            population.members().into_iter().map(move |member| serde_json::json!({
                "tag": tag.replace("{}", &member),
                "at": where_word(*at),
            }))
        }).collect::<Vec<_>>(),
    })
}

/// ★★★★★ R1903 — the palette's placement, and what it may do.
///
/// `at` is where it IS and `opens` where it arrives — two facts, for the reason
/// R1900 gave one screen over: the same bit cannot say whether a panel was put
/// away by a person or came that way, and a client restoring a session needs
/// the second. `foldable` is the policy, published so a client is told what the
/// `palette` verb accepts rather than having to be refused to find out.
fn palette_placement_json() -> serde_json::Value {
    let at = palette_placement();
    serde_json::json!({
        "at": { "edge": "right", "extent": at.extent, "folded": at.folded },
        "opens": {
            "edge": "right",
            "extent": spec::PALETTE_OPENS.extent,
            "folded": spec::PALETTE_OPENS.folded,
        },
        "foldable": spec::PALETTE_POLICY.foldable,
        "strip_w": spec::PALETTE_STRIP_W,
        // ★★★★★ R1908 — whether `at` came from a PREVIOUS RUN.
        //
        // A third fact beside `at` and `opens`, and not derivable from them: a
        // palette folded at the specification's own extent is the same two
        // fields whether this build opened it that way, a person folded it a
        // moment ago, or a stored session was read and believed. A client
        // explaining a session, or deciding whether to offer "reset to
        // default", needs the one this cannot be inferred from.
        "restored": SHELL_STATE.with(|slot| {
            slot.borrow().as_ref().is_some_and(|state| state.palette_restored.get())
        }),
    })
}

/// The ids of the cards that are currently torn off, in the order they float.
///
/// Lifted out of the read arm in R1738 for the reason the lint gives: adding one
/// slot took `query` one line over its budget, and a nine-line expression inline
/// beside forty one-line arms was the cheapest thing in it to name.
fn floating_ids(state: &ShellState) -> String {
    state
        .floats
        .get()
        .iter()
        .map(|f| f.id.clone())
        .collect::<Vec<_>>()
        .join(",")
}

/// ★★★★★ R1738 — the framework's own count of how much of this application has
/// been judged, section by section.
///
/// Built by `ScreenRoster` from the roster rather than assembled here, for the
/// reason the slot exists at all: a list written on this side is a list a
/// section can fall off, and that is exactly what the round measured — four of
/// six open sections had never been compared with anything and nothing said so.
fn sections_json(state: &ShellState) -> serde_json::Value {
    state.screens.conformance(&state.journey.get()).to_json()
}

/// ★★★★★ R1767 — how much of its specification each section reproduced
/// somewhere along the walk this session has taken.
///
/// The peer of [`sections_json`], which answers about the frame in front of the
/// reader. Both are published, because they are two questions and reading
/// either as the other is the mistake both exist to prevent: this one can say
/// *this application reproduces what it is specified to be* and cannot say
/// *right now*, and the other is the exact opposite.
fn journey_json(state: &ShellState) -> serde_json::Value {
    state
        .screens
        .journey_conformance(&state.journey.get())
        .to_json()
}

/// ★ R1695 — the Settings switches, as the wire reads them.
fn options_json(state: &ShellState) -> serde_json::Value {
    let on = state.options.get();
    serde_json::Value::Array(
        spec::OPTIONS
            .iter()
            .zip(on)
            .map(|(option, on)| {
                serde_json::json!({
                    "key": option.key,
                    "title": option.title,
                    "gist": option.gist,
                    "group": option.group,
                    "on": on,
                })
            })
            .collect(),
    )
}

/// The wire spelling of where a region belongs — `"*"` for chrome, which is on
/// screen at every destination, and the key otherwise.
const fn where_word(at: spec::Where) -> &'static str {
    match at {
        spec::Where::Chrome => "*",
        spec::Where::At(key) => key,
    }
}

/// The wire spelling of a tier.
const fn tier_word(tier: spec::Tier) -> &'static str {
    match tier {
        spec::Tier::Placeable => "placeable",
        spec::Tier::Reserved => "reserved",
    }
}

/// ★★★★★ R1733 — the tag one PART of a palette row is addressed by, when the
/// row is one a widget can be picked up from.
///
/// Family first — `part.<what>.<kind>` — because the kind is the address and
/// the part is what is drawn of it, which is the convention
/// `painted_surface_of` reads. The row itself keeps `shell.palette.<kind>`, so
/// the row's own tag and its parts' cannot be confused for each other by a
/// dot count.
///
/// ★★ [`None`] for a RESERVED row, and both halves of that are deliberate:
///
/// * The specification is the reference's palette row, and the reference has
///   no reserved rows at all — its twelve are all draggable. A reserved row's
///   trailing seat says *later*; calling it `verb` ("the seat that places one
///   without a drag") would be a specification claiming something untrue of the
///   thing it was read off.
/// * A reserved row DECLARES itself unavailable, and the disabled cascade
///   reaches every tagged node under it. So tagging four parts inside one turns
///   one announced inert region into five, four of which are ink — and a
///   region stating a reason that reaches no reader is a defect this tree
///   already gates for. It caught this on the first run: thirty-six of them.
fn part_tag_of(part: &str, def: &'static spec::WidgetSpec) -> Option<String> {
    (def.tier == spec::Tier::Placeable).then(|| part_tag(part, def.kind))
}

/// The tag one part of a palette row is addressed by, unconditionally — the
/// spelling, for a caller that has already decided the row has parts.
pub(crate) fn part_tag(part: &str, kind: &str) -> String {
    format!("{PALETTE_PART}{part}.{kind}")
}

/// The stem every palette-row part is tagged under.
pub(crate) const PALETTE_PART: &str = "shell.palette.part.";

/// The stem the palette panel's own heading is tagged under.
///
/// Its own stem rather than a suffix of the panel's, because
/// [`PaintedRegions::parts_under`](pinion_core::painted::PaintedRegions::parts_under)
/// takes the tags whose remainder holds no further dot — so the heading sits
/// beside the entries in one flat family unless it is given a stem of its own,
/// and a specification of the catalogue would then have to name two lines that
/// are not catalogue entries.
pub(crate) const PALETTE_HEAD: &str = "shell.palette.head.";

/// What the panel calls itself.
pub(crate) const PALETTE_HEAD_TITLE: &str = "shell.palette.head.title";

/// The one line under it saying how a widget gets onto the board.
pub(crate) const PALETTE_HEAD_HINT: &str = "shell.palette.head.hint";

/// One palette row: the swatch, the name, the one line, and what the row offers
/// -- the verb for a placeable entry, the booking for a reserved one.
fn palette_row(
    state: &ShellState,
    def: &'static spec::WidgetSpec,
    rect: Rect,
    palette: Palette,
) -> Scene {
    let placed = state
        .cards
        .get()
        .iter()
        .filter(|c| kind_of(c.id().as_str()) == def.kind)
        .count();
    let reserved = def.tier == spec::Tier::Reserved;
    // The reference shows the reservation in the slot the add control would
    // occupy, so a reader's eye finds "what does this row offer me" in one
    // place down the whole list.
    //
    // ★ Its width is what the text beside it gives up. The first draft sized
    // the text against the ADD control and then put a wider word in the slot,
    // and every one of the nine reserved rows painted its name under the
    // badge -- eighteen overlapping pairs, which the gate reported before
    // anybody looked at the window.
    // ★ R1733 — a row's parts are TAGGED when the row is one a widget can be
    // picked up from, so the specification of what such a row is made of can be
    // read back out of the paint rather than off a table this file also writes.
    // A reserved row's are not, for the two reasons on `part_tag_of`.
    //
    // ★★ Each tagged part DECLARES its quiet. The row is the control — pressing
    // anywhere on it adds, and it is the thing a reader arrives at — so its
    // parts owe a reader silence with a reason: the name IS the row's name, and
    // the line and the seat are folded into its announcement. A painted,
    // addressable region nobody decided anything about is `unvoiced`, and the
    // demo harness's census counted sixteen of them on this row's first run.
    let row_tag = format!("shell.palette.{}", def.kind);
    let part = |what: &str, text: &str, at: Rect, px: u32, fg: Color| -> Scene {
        match part_tag_of(what, def) {
            Some(tag) => {
                cell(tag, text, at, px, fg, TextOverflow::Ellipsis).silenced(if what == "name" {
                    Silence::name_of(row_tag.clone())
                } else {
                    Silence::part_of(row_tag.clone())
                })
            }
            None => clipped(text, at, px, fg, TextOverflow::Ellipsis),
        }
    };
    let (trailing_w, trailing) = if reserved {
        (
            52,
            part(
                "verb",
                "later",
                Rect::new(rect.w.saturating_sub(52), 15, 46, 16),
                FONT_SMALL,
                palette.muted,
            ),
        )
    } else {
        (
            34,
            part(
                "verb",
                &if placed == 0 {
                    "+".to_string()
                } else {
                    format!("+ {placed}")
                },
                Rect::new(rect.w.saturating_sub(30), 15, 26, 16),
                FONT_BODY,
                palette.accent_fg,
            ),
        )
    };
    let text_w = rect.w.saturating_sub(50 + trailing_w + 6);
    // R1668 — a reserved row is DECLARED unavailable and states what it is
    // booked under. Everything a reader and an agent get from that -- inert to
    // the pointer, faded ink, `aria-disabled` with a spoken reason, a row on
    // `scene/disabled` naming the requirement -- follows from this one
    // declaration rather than from four hand-kept copies of it.
    let layout = if reserved {
        absolute(rect).with_unavailable(Unavailable::reserved(def.reserved_for))
    } else {
        absolute(rect)
    };
    Scene::Container(
        ContainerNode::new(vec![
            // The swatch is the kind's colour and its short code — decoration
            // all the way down, so the silence covers the run inside it too.
            code_chip(
                def.code,
                Rect::new(8, 7, 32, 32),
                BoxStyle::filled(kind_color(def.kind)).with_corner_radius(8),
                palette.on_accent,
                part_tag_of("swatch", def),
            )
            .silenced(Silence::decorative(
                "the kind's colour tile and its short code",
            )),
            // Both elide. A palette is a list of names of varying length in a
            // fixed column, so "the longest one happens to fit" is not a
            // property anybody can keep -- and the boot gate measured this one
            // at ten pixels outside its row.
            part(
                "name",
                def.label,
                Rect::new(50, 8, text_w, 16),
                FONT_BODY,
                palette.ink,
            ),
            part(
                "gist",
                def.gist,
                Rect::new(50, 26, text_w, 14),
                FONT_SMALL,
                palette.muted,
            ),
            trailing,
        ])
        .with_tag(format!("shell.palette.{}", def.kind))
        .with_style(
            BoxStyle::filled(palette.raised)
                .with_corner_radius(10)
                .with_border(Border::new(palette.outline, 1)),
        )
        .with_layout(layout),
    )
}

/// ★★★★★ R1726/R1733 — **the mark at the cell a release would use.**
///
/// R1726: the preview is a MARK, not a surface. It was an opaque fill, and an
/// opaque fill has no correct layer — under the cards it hides behind whatever
/// occupies the destination, and over them it hides the widget standing there.
/// Both were driven with a real pointer and both read the same way to the
/// person: "the widget goes grey". Translucent, it can sit above the board and
/// cover nothing, so the destination is legible AND so is what is currently in
/// it — which is exactly what somebody deciding where to drop needs at once.
///
/// R1733: and it SAYS WHICH CELL, beside the same grip glyph the row it came
/// from is dragged by. Both read from the reference's own mark — a six-dot grip
/// and the coordinate in parentheses, in the accent, centred. This board drew an
/// empty rectangle, so "where exactly" was something you counted grid lines for,
/// and on a twelve-column board nobody does.
fn carry_slot_scene(ghost: &Tile, palette: Palette) -> Scene {
    let tint = palette.accent_fg;
    let slot = cell_rect(ghost);
    let mid = slot.h.saturating_sub(16) / 2;
    Scene::Container(
        ContainerNode::new(vec![
            Scene::Container(
                ContainerNode::new(
                    (0..3)
                        .flat_map(|r| (0..2).map(move |c| dot(c * 6, r * 6, 3, tint)))
                        .collect(),
                )
                .with_tag("shell.carry.slot.grip")
                .with_layout(absolute(Rect::new(slot.w / 2 - 34, mid + 2, 9, 15))),
            ),
            cell(
                "shell.carry.slot.cell".to_owned(),
                &format!("({},{})", ghost.col, ghost.row),
                Rect::new(slot.w / 2 - 18, mid, 60, 16),
                FONT_BODY,
                tint,
                TextOverflow::Clip,
            ),
        ])
        .with_tag("shell.carry.slot")
        .with_style(
            BoxStyle::filled(Color::rgba(tint.r, tint.g, tint.b, 0x24))
                .with_corner_radius(10)
                .with_border(Border::new(palette.accent_fg, 2)),
        )
        .with_layout(absolute(slot)),
    )
}

/// ★★★★★ R1733/R1900 — **what a release would do, drawn.**
///
/// One function because there is one classifier: [`berth_at`] answers whether
/// the release takes a cell or joins a place, and each answer has exactly one
/// mark. Written as a `match` over the whole vocabulary rather than an `if`
/// beside the old code, so a third berth cannot be added without a painter
/// being chosen for it.
fn berth_preview_scene(
    state: &ShellState,
    board: &TileGrid,
    drag: &TileDrag,
    palette: Palette,
) -> Option<Scene> {
    let (px, py) = state.cursor.get();
    match berth_at(state, px, py, drag.carried().id()) {
        Berth::With(host) => board
            .tile(&TileId::new(host))
            .map(|host| join_mark_scene(host, palette)),
        // ★★★★★ R1733 — the preview is the DRAG's, so the rectangle drawn here
        // and the cell a release commits to are the same value read twice
        // rather than two derivations that agree today.
        Berth::Own => drag
            .preview(board)
            .map(|ghost| carry_slot_scene(&ghost, palette)),
    }
}

/// ★★★★★ R1900 — **the mark on the place a release would join.**
///
/// The header band rather than the whole cell, because the header is what the
/// release is aimed at and the strip is what will change: a mark over the whole
/// card would say "this card is going away", which is the opposite of what a
/// join does.
///
/// Its accent and translucency are [`carry_slot_scene`]'s, for R1726's reason —
/// a preview that covers what is under it hides the very thing a person is
/// deciding about.
fn join_mark_scene(host: &Tile, palette: Palette) -> Scene {
    let tint = palette.accent_fg;
    let band = header_rect(cell_rect(host));
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag("shell.carry.join")
            .with_style(
                BoxStyle::filled(Color::rgba(tint.r, tint.g, tint.b, 0x24))
                    .with_corner_radius(8)
                    .with_border(Border::new(tint, 2)),
            )
            .with_layout(absolute(band)),
    )
}

/// ★★★★★ R1903 — a folded palette: a strip, and the WHOLE strip is what a
/// reader presses to bring it back.
///
/// The canon's closed state literally: a narrow band on the same edge, carrying
/// the toggle on its own element. Not a button inside a band — a fold a reader
/// has to aim at to undo is a fold that traps them.
///
/// Its rows are not built at all, which is what makes the strip the only way
/// back rather than merely the visible one — the R1695 rule about a page that
/// is not current, applied to a panel that is not open.
fn palette_strip_scene(palette: Palette) -> Scene {
    let panel = palette_rect();
    let mid = panel.h / 2;
    Scene::Container(
        ContainerNode::new(vec![
            // Three dots down the middle: the same grip vocabulary this screen
            // uses everywhere else for "a thing a hand takes hold of".
            Scene::Container(
                ContainerNode::new(
                    (0..3)
                        .map(|n| dot(0, n * 8, 4, palette.muted))
                        .collect::<Vec<_>>(),
                )
                .with_tag("shell.palette.strip.grip")
                .with_layout(absolute(Rect::new(panel.w / 2 - 2, mid - 12, 4, 20))),
            ),
        ])
        .with_tag("shell.palette.strip")
        .with_style(BoxStyle::filled(palette.raised).with_border(Border::new(palette.outline, 1)))
        .with_layout(absolute(panel).with_focusable(true)),
    )
}

/// The palette panel: the catalogue, grouped, with a count at the foot.
fn palette_scene(state: &ShellState, palette: Palette) -> Scene {
    if palette_placement().folded {
        return palette_strip_scene(palette);
    }
    let panel = palette_rect();
    // ★★★★★ R1761 — the panel's own heading, ADDRESSABLE. It was two loose
    // labels, so the two lines a reader reads first were the two lines nothing
    // could ask about: measured over the wire before this round, the paint under
    // `shell.palette.` held thirteen entries and two counts and neither of
    // these. A specification cannot fix words that have no address.
    let mut children = vec![
        cell(
            PALETTE_HEAD_TITLE.to_owned(),
            spec::PALETTE_TITLE,
            palette_head_title_rect(),
            FONT_TITLE,
            palette.ink,
            TextOverflow::Ellipsis,
        )
        // ★ R1761 — these words ARE the panel's accessible name, so saying them
        // again would be one fact in two voices. Addressable for the
        // specification, silent for the reader.
        .silenced(Silence::name_of("shell.palette")),
        // ★★★★★ R1903 — the control that puts it away, at the rectangle
        // `Hit::at` asks the same function for.
        //
        // ★★★★★ R1951 — and its MARK is now the shared chrome vocabulary's,
        // asked of the palette's own policy rather than drawn here. Two things
        // were wrong with the twelve-by-two bar it replaces, and only one of
        // them was the one R1950 wrote down: the same act wore a different face
        // from the node lab's panels, AND **the behaviour reference draws a
        // chevron**, measured this round — its collapse control is a 20-unit box
        // carrying `8,5 13,10 8,15`. So the bar was not a reproduction of
        // anything; it was this screen's own invention, and a bar says
        // *minimise* where the reference says *push it back to its edge*.
        Scene::Container(
            ContainerNode::new(control_mark::scenes(
                palette_fold_face(),
                box_content(palette_fold_rect()),
                palette.muted,
            ))
            .with_tag(format!("{PALETTE_HEAD}fold"))
            .with_style(
                BoxStyle::filled(palette.raised)
                    .with_corner_radius(7)
                    .with_border(Border::new(palette.outline, 1)),
            )
            .with_layout(absolute(palette_fold_rect()).with_focusable(true)),
        ),
        cell(
            PALETTE_HEAD_HINT.to_owned(),
            spec::PALETTE_HINT,
            Rect::new(16, 42, 250, 16),
            FONT_SMALL,
            palette.muted,
            TextOverflow::Ellipsis,
        )
        // And the line under it is the panel's own description, which the list
        // announces as its value.
        .silenced(Silence::part_of("shell.palette")),
    ];
    for row in palette_rows() {
        match row.def {
            // The heading is the group a reader descends through, so it is
            // addressable rather than loose ink between the entries.
            None => children.push(cell(
                format!("shell.palette.section.{}", row.section),
                &row.title,
                row.rect,
                FONT_TINY,
                palette.muted,
                TextOverflow::Ellipsis,
            )),
            Some(def) => children.push(palette_row(state, def, row.rect, palette)),
        }
    }
    // Both counts, because the screen's whole claim is the relation between
    // them: this release places four, and holds nine seats open.
    //
    // ★ R1864 — seated in the panel's own FOOTER BAND rather than at a hand
    // offset from its bottom edge. The band is what `palette_row_h` divides the
    // rest of the panel around, so the counts and the catalogue cannot disagree
    // about where one ends and the other begins — which they had, by two pixels
    // at the design height and by a whole row once the panel moved.
    let foot = palette_foot_rect();
    children.push(cell(
        "shell.palette.placed".to_owned(),
        &format!(
            "{} placed of {}",
            state.placed().len(),
            spec::placeable_count()
        ),
        pinion_core::containment::line_rect_in(foot, 16, 130, FONT_SMALL),
        FONT_SMALL,
        palette.muted,
        TextOverflow::Ellipsis,
    ));
    children.push(cell(
        "shell.palette.reserved".to_owned(),
        &format!("{} reserved", spec::reserved_count()),
        pinion_core::containment::line_rect_in(foot, panel.w.saturating_sub(110), 94, FONT_SMALL),
        FONT_SMALL,
        palette.muted,
        TextOverflow::Ellipsis,
    ));
    keyboard_stop(
        Scene::Container(
            ContainerNode::new(children)
                .with_tag("shell.palette")
                .with_style(BoxStyle::filled(palette.panel))
                .with_layout(absolute(panel)),
        ),
        "shell.palette",
        &state.at(),
    )
}

/// The toast: what just happened, floating at the foot of the WINDOW.
///
/// ★★★★★ R1776 — of the window, and centred, because that is where the
/// reference puts it: `position: fixed; bottom: 22px; left: 50%;
/// transform: translateX(-50%)`. It used to be `canvas.x + 24, win_h() - 58`,
/// and the x is the part that mattered — `canvas_rect()` is the rectangle the
/// DASHBOARD uses for its canvas, and once screens were mounted that stopped
/// being the region a destination receives (`page_rect` is the function that
/// knows the difference). A reader saw the result sitting on a mounted screen's
/// palette.
///
/// The width is still a constant and the reference sizes to content; that
/// remains open, because this shell has no text measurement of its own and
/// inventing a third approximation of one would be adding the duplication a
/// later round has to remove.
///
/// ★★ R1778 — returns `None` once the sentence's time is up, so ONE place knows
/// whether there is a toast. The call site used to hold that condition too, and
/// two places knowing when a thing is drawn is how one of them comes to be
/// wrong.
/// ★★★★★ R1865 — **what the band's one slot is saying**, in the band's own
/// space.
///
/// `None` once the sentence's time is up, so ONE place knows whether there is a
/// toast (R1778's rule, kept). The gesture strip is what the slot holds
/// otherwise, and it is the CALLER that says so — this returns the toast's
/// occupancy or nothing, and the two possibilities meet in exactly one place.
fn toast_in_slot(state: &ShellState, palette: Palette, theme: &Theme, slot: Rect) -> Option<Scene> {
    let said = state.toast.showing()?;
    let sentence = said.sentence();
    // ★★★★★ R1811 — **the box is a claim about its sentence, so it is measured
    // from it.**
    //
    // It was `TOAST_W`, a constant 560, and a reader looking at the running
    // window said the box was strangely wide for the words in it. Measured
    // through `containment::slack` on the assembled screen, the box held over
    // 400 pixels its content never used.
    //
    // ⚠ Measured here rather than laid out, and that is forced: `view` is SYNC
    // AND PURE by §6.3 so `dry_run` holds, and shaping lives in the render
    // layer — the same wall R1778 met when a toast needed a LIFETIME and got
    // `Owner::register_animation`. So this is the estimate the containment gate
    // is judged against, and the gate is what keeps the estimate honest: too
    // narrow and `escapes` reports the sentence leaving its box, too wide and
    // `slack` reports the room.
    //
    // ★ R1865 — and it is bounded by the SLOT now, not only by `TOAST_W`: the
    // band is a fixed width and a sentence longer than it elides rather than
    // running off the end of the window.
    let width = toast_width(&sentence).min(slot.w);
    debug_assert!(width > 0, "a sentence with a zero-width box is not said");
    Some(Scene::Container(
        ContainerNode::new(vec![
            // ★★★★★ R1719 — the bullet was `accent_fg` whatever had been said.
            // A refusal and a confirmation were one picture, on the screen and
            // in a reader's ear both.
            //
            // ★ R1865 — and it is the ONLY thing that distinguishes a toast
            // from the gesture strip now, so it is centred on the slot's line
            // rather than placed at a hand-picked offset into a box that no
            // longer exists.
            named_dot(
                TOAST_DOT_TAG,
                0,
                slot.h.saturating_sub(TOAST_DOT) / 2,
                TOAST_DOT,
                toast_dot(said.tone(), theme),
            )
            // ★ R2012 — addressable and deliberately NOT a second voice. The
            // bullet is the seeing half of a pair whose hearing half is already
            // said: `Tone::frame` puts the tone in words at the front of the
            // sentence this toast announces. A shape with no words of its own,
            // read as its own stop, is the same fact a third time.
            .silenced(Silence::part_of("shell.toast")),
            label(
                &sentence,
                Rect::new(
                    TOAST_TEXT_X,
                    0,
                    width.saturating_sub(TOAST_TEXT_X + TOAST_PAD_RIGHT),
                    slot.h,
                ),
                STATUS_FACE,
                palette.ink,
            ),
        ])
        .with_tag("shell.toast")
        .with_layout(absolute(Rect::new(0, 0, width, slot.h))),
    ))
}

/// The tone bullet's size.
const TOAST_DOT: u32 = 8;

/// The tone bullet's address. Inside `shell.toast`'s family, because the
/// bullet is part of what the toast says rather than a mark of its own.
const TOAST_DOT_TAG: &str = "shell.toast.tone";

/// ★★★★★ R1864/R1865 — **the host's status band, drawn, and everything this
/// application says about itself is in it.**
///
/// A filled strip in [`status_band_rect`] carrying ONE message slot. It is
/// painted rather than merely reserved for two reasons a reader can see: a band
/// nobody fills reads as the page running off the bottom of the window, and a
/// sentence needs a ground of its own — before R1864 the gesture strip was set
/// in whatever the guest happened to have painted underneath it.
///
/// # The slot holds the toast when there is one, and the gesture strip when
/// there is not
///
/// That is the status-bar pattern, and R1865 is the round that finished it. The
/// floating box this replaces was moved out of the way of whatever the screen
/// had under it (R1861), which fixed the covering and bought it with
/// unpredictability — measured across the six open destinations, one sentence,
/// one window: **three different heights, 96 pixels apart**, and never the one
/// the behaviour reference specifies. A reader reported that cost without being
/// asked. Here the answer to *where does it appear* is one rectangle for every
/// destination, every sentence and every screen, and the answer to *what does it
/// cover* is **nothing**, structurally: the band is outside every
/// [`page_rect`].
///
/// # ⚠ What is spent, stated rather than hidden
///
/// The gesture sentence is not readable while a toast is up. That is the trade a
/// one-slot band makes and it is the right way round — the strip is a reminder a
/// reader can wait 2.6 seconds for, and a toast is the answer to something they
/// just did. The alternative, two slots side by side, halves the room for both
/// and makes the toast's position depend on the length of a sentence beside it,
/// which is the property this round exists to remove.
/// ★★★★★ R1916 — the description of the mark a reader is resting on, drawn
/// beside it.
///
/// The canon puts a `title` on 25 of its controls and this tool drew none. What
/// was missing was not the widget — R695 built one — but a way to say *that
/// mark over there has a sentence*, which its own module docs named as a future
/// axis. `pinion_core::describe` is that, and this is its second consumer.
///
/// ★ R2026 — `None` when nothing is being rested on, which is what makes the
/// frame CHANGE under a resting cursor — the canon surface the census calls
/// `affordance.hover`, whose probe compares two painted frames. Absent rather
/// than empty, for the reason [`settings_roster_scene`] carries.
fn shell_tip_scene(state: &Rc<ShellState>, palette: Palette) -> Option<Scene> {
    let (tag, sentence) =
        shell_description_shown(state, pinion_core::focus_state::focused().as_deref())?;
    let anchor =
        pinion_core::painted::painted_regions(VIEW_TAG).and_then(|marks| marks.rect_of(&tag))?;
    // ★★★★★ R1918 — WHERE it goes and WHAT IT LOOKS LIKE are both the
    // substrate's. What stays here is the palette a description has to sit
    // legibly on, and the window it is clamped inside.
    Some(Scene::Container(
        ContainerNode::new(vec![pinion_widget_paint::described::view_description(
            SHELL_TIP,
            &sentence,
            anchor,
            Rect::new(0, 0, win_w(), win_h()),
            (0, 0),
            pinion_widget_paint::described::DescriptionStyle {
                face: STATUS_FACE,
                ..pinion_widget_paint::described::DescriptionStyle::COMPACT
            },
            pinion_widget_paint::described::DescriptionInk {
                surface: palette.raised,
                outline: None,
                ink: palette.ink,
            },
        )])
        .with_layout(absolute(Rect::new(0, 0, win_w(), win_h()))),
    ))
}

fn status_band_scene(state: &ShellState, palette: Palette, theme: &Theme) -> Scene {
    let band = status_band_rect();
    let slot = status_slot_rect();
    // The slot in the BAND's space, which is what the band's own node is laid
    // out in.
    let local = Rect::new(slot.x - band.x, slot.y - band.y, slot.w, slot.h);
    let saying = toast_in_slot(state, palette, theme, slot).unwrap_or_else(|| {
        // ★★★★★ R1867 — the idle occupant is a REGION now, not a bare run.
        //
        // The slot below declares that whatever is in it speaks, and that
        // promise has to be kept in both of its states. A toast keeps it
        // (`shell.toast` is announced); this sentence did not, because a run
        // with no address is invisible to the accessibility tree and to the
        // census both. So the host's gesture help — the one line telling a
        // reader what the pointer does here — reached nobody who does not see
        // the drawing, for the 163 rounds it was a floating strip and the three
        // it has been in this band.
        Scene::Container(
            ContainerNode::new(vec![label(
                HELP_STRIP,
                Rect::new(0, 0, local.w, local.h),
                STATUS_FACE,
                palette.muted,
            )])
            .with_tag(STATUS_GESTURE)
            .with_layout(absolute(Rect::new(0, 0, local.w, local.h))),
        )
    });
    // ★★★★★ R1867 — **one slot, and its occupant is what speaks.**
    //
    // `layout` rather than `part_of(X)`: the slot has two possible occupants
    // and neither is the other's part, so naming one of them would be a
    // declaration that is true half the time. What is true in both states is
    // that this box arranges and does not announce — and the census checks
    // exactly that, by refusing a `layout` whose subtree nobody speaks for
    // (`Voice::Hollow`). That refusal is what forced the gesture sentence above
    // to become a region; a weaker declaration would have bought this gate's
    // silence with a reader's.
    let slot_node = Scene::Container(
        ContainerNode::new(vec![saying])
            .with_tag(STATUS_SLOT)
            .with_layout(absolute(local)),
    )
    .silenced(Silence::layout("the status band's one message slot"));
    Scene::Container(
        ContainerNode::new(vec![slot_node])
            .with_tag(STATUS_BAND)
            // Exactly the application bar's own style, and deliberately: the two
            // are the same piece of furniture at opposite edges, and a band that
            // painted itself differently would read as a panel that had slipped
            // down there.
            .with_style(BoxStyle::filled(palette.panel))
            .with_layout(absolute(band)),
    )
    // ★ R1867 — the band is the slot's ground and nothing else. Its own words
    // are the slot's, so it arranges and stays quiet.
    .silenced(Silence::layout("the status band's ground"))
}

/// The status band's tag.
const STATUS_BAND: &str = "shell.status";

/// ★ R1865 — the band's one message slot, addressable.
///
/// A tag rather than an untagged container, because *where the application
/// speaks* is a fact a specification should be able to name and a gate should be
/// able to ask about — and because R1864 measured what an untagged mark costs:
/// the gesture strip reached four mounted screens for 163 rounds without ever
/// entering the ratchet that counts host marks over a guest, since that
/// predicate counts TAGGED nodes.
const STATUS_SLOT: &str = "shell.status.slot";

/// ★★★★★ R1867 — the slot's IDLE occupant: the host's gesture sentence.
///
/// It has an address because it has a voice, and it has a voice because the
/// slot's declaration promises one. Before this round the sentence was an
/// untagged text run inside a tagged box, which is the one shape the voice
/// census cannot see: a run with no address is not an addressable region, so
/// the box read as a container over nothing and the words reached no reader who
/// does not see them.
const STATUS_GESTURE: &str = "shell.status.gesture";

/// Every colour the painters here read, resolved from one theme.
///
/// Extracted at its second consumer (R1674): the geometry tests need a palette
/// to build a card's style with, and the alternative — a second literal struct
/// in the test — is a copy that stops resembling the real one the first time a
/// role is added.
fn palette_of(theme: &Theme, dark: bool) -> Palette {
    Palette {
        ink: theme.resolve(ColorRole::OnSurface),
        muted: theme.resolve(ColorRole::OnSurfaceMuted),
        accent: theme.resolve(ColorRole::Accent),
        on_accent: theme.resolve(ColorRole::OnAccent),
        accent_fg: theme.resolve(ColorRole::InversePrimary),
        canvas: theme.resolve(ColorRole::Surface),
        panel: theme.resolve(ColorRole::SurfaceContainerLow),
        raised: theme.resolve(ColorRole::SurfaceContainer),
        high: theme.resolve(ColorRole::SurfaceContainerHigh),
        outline: theme.resolve(ColorRole::Outline),
        grid: grid_ink(dark),
        warn: theme.resolve(ColorRole::Warning),
        // ★ R1719 — the toast's bullet needs a colour for a refusal, and it is
        // a role rather than a literal for `warn`'s reason: a red picked by
        // hand holds its contrast in exactly one of the two themes.
        refused: theme.resolve(ColorRole::Error),
        faded: theme.resolve(ColorRole::Outline),
    }
}

/// The toast bullet's colour, which is what a sighted reader learns the tone
/// from — the seeing half of the pair whose hearing half is the live region's
/// urgency, both off one [`Tone`].
///
/// ★★★★★ R2012 — this used to be a `match` HERE, and the match reached for a
/// role whose ground is somewhere else. `Tone::Done` took `accent_fg`
/// ([`ColorRole::InversePrimary`]), declared for
/// [`ColorRole::InverseSurface`] and not for the status band this dot is drawn
/// on. It was not carelessness — there was no role for *it happened*, so the
/// nearest tone that was not the error red got used.
///
/// ⚠ WHAT SAVED THIS SCREEN WAS AN ACCIDENT, AND A COUNTERFACTUAL IS WHAT
/// FOUND THAT OUT. [`reference_palettes`] binds a magenta for
/// `inverse_primary`, so the bullet measured **7.88** light and **5.97** dark
/// here and was perfectly findable. Against the framework's own
/// `Theme::light` / `Theme::dark` the same mapping reads **1.70** and
/// **2.17** — under the 3.0 a non-text mark is held to. So the defect was in
/// the answer, not in this palette, and any application inheriting the
/// defaults would have carried it.
///
/// The mapping lives on [`Tone::role`] now, where the vocabulary states it
/// once for every consumer, and `r2012_the_status_bullet_is_findable_in_both_palettes`
/// holds it to the floor in the canonical palettes as well as in these.
fn toast_dot(tone: Tone, theme: &Theme) -> Color {
    theme.resolve(tone.role())
}

fn view(_state: ScreenState, frame: Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_shell_state();
    let dark = theme_word(&state.theme) == "dark";
    let palette = palette_of(&theme, dark);

    let journey = state.journey.get();
    let here = journey.here(state.roster()).clone();
    let region = page_rect(here.key.as_ref());
    // ★★★★★ R1695 — the page a destination gets, built by the framework's
    // region so that the pages it is NOT at are never constructed. Before this
    // the rail moved a string and the window did not change: measured through
    // the router, four of seven seats left the screen at 193 painted regions
    // before and 193 after.
    let page = keyboard_stop(
        view_page_region(
            "shell.canvas",
            region,
            palette.canvas,
            &here,
            // ★★★★★ R1724 — a mounted screen resolves its own presses, and the
            // region must not be pointer-transparent in front of it. Measured
            // the day the lab was first mounted: transparent, the whole screen
            // was dead to a mouse while every wire path kept working.
            if state.screens.is_mounted(here.key.as_ref()) {
                PagePointer::PageResolves
            } else {
                PagePointer::HostResolves
            },
            // ★★★★★ R1724 — **the page may now be a whole other binding.**
            //
            // `page_scene` hands back the mounted screen's own scene, built in
            // the extent this region was placed at — so the guest's paint and
            // its hit test resolve against one rectangle. A destination with no
            // screen behind it is one of this application's own pages, exactly
            // as before, and the match below is what says which.
            |here| {
                state
                    .screens
                    .page_scene(&journey, (region.w, region.h), &frame)
                    .map_or_else(
                        || match here.key.as_ref() {
                            // ★★★★★ R1762 — the page SCROLLS, which the
                            // reference's does and this one did not. The pane
                            // derives its range from the groups themselves, so
                            // a page that grows a row cannot outrun a number
                            // written anywhere.
                            "settings" => vec![
                                scroll_pane(
                                    &state.settings_scroll,
                                    Rect::new(0, 0, region.w, region.h),
                                    (0, SET_PAD),
                                    // Every press goes to the one root
                                    // `External` that runs this screen's own
                                    // hit test, so the pane is invisible to
                                    // the router (R1655).
                                    PanePointer::PassesThrough,
                                    settings_scene(&state, palette, region),
                                )
                                .silenced(Silence::layout(
                                    "the preferences page's scrolling viewport",
                                )),
                            ],
                            _ => dashboard_scene(&state, palette),
                        },
                        |mounted| vec![mounted],
                    )
            },
        ),
        // ★ R1696 — a stop at the DASHBOARD only, where this region is the
        // board and the arrows already move a selection among its cards. At
        // Settings the same rectangle is a page whose controls are their own
        // stops, and a landmark is not a Tab stop.
        "shell.canvas",
        here.key.as_ref(),
    );

    let children = std::iter::once(page)
        .chain([app_bar_scene(&state, palette), rail_scene(&state, palette)])
        // ★★★★★ R1776 — the toast is on the frame only while it has life left.
        // Absent rather than transparent: a mark that is painted invisibly is
        // still in the scene, still in the accessibility tree, and still over
        // the guest as far as `pinion_screen::layering` is concerned — and the
        // reader who reported this saw exactly what a permanent one does.
        //
        // ★★★★★ R1865 — and it is not a sibling here any more: it is painted by
        // `status_band_scene`, in the band's one message slot, below. A floating
        // box that had to be MOVED off whatever was under it is a box whose
        // position depends on the screen; a slot in host furniture is one
        // rectangle for every destination.
        // ★ R1695 — the layout bar and the palette are the DASHBOARD's. They sit
        // outside the region because they sit outside its rectangle, so the
        // substrate's guarantee does not reach them; the specification's
        // destination column is what checks them, in both directions.
        .chain(if spec::shows_board_chrome(here.key.as_ref()) {
            vec![
                sub_bar_scene(&state, palette),
                palette_scene(&state, palette),
            ]
        } else {
            Vec::new()
        })
        // ★★★★★ R2026 — **an absent surface is not in this list**, rather than
        // in it as an empty container.
        //
        // Four of these were built per frame and every one of them meant *this
        // surface is not open right now*. Measured over the whole walk before
        // the repair: 32 boxless empty containers, 4 per destination across all
        // 8, every one a top-level child of this node — and nothing else in the
        // assembled scene spelled absence that way. They cost a reader of the
        // scene an INFERENCE: R1971's reach walk had to grow *a container that
        // holds nothing draws nothing* to keep from reporting them as defects,
        // and that inference was wrong on its first attempt (narrowed to
        // "empty" alone it also excused empty containers that DO have a box,
        // taking three gates red).
        //
        // `Option<Scene>` and `.flatten()` is the same shape `hello-node-lab`
        // has used for its toast and its pin tip since R1688 — this screen was
        // the one spelling it the other way.
        .chain(
            [
                // ★★ R1672 — the preset menu is a POPUP: anchored to the sub
                // bar's chip, bounded by the window. It used to be a child of
                // the bar and hung 81 pixels below it, which is an escape and
                // was invisible until the ink gate reached this screen. A
                // sibling here also puts it over everything it opens across,
                // which a child of one bar can never be.
                (state.preset_open.get() && spec::shows_board_chrome(here.key.as_ref()))
                    .then(|| preset_menu_scene(&state, palette)),
                // ★★★★★ R1762 — an open value roster, for the same reason and in
                // the same place: over everything, in window space, bounded by the
                // page it must not leave. Painted after the page so a press on it
                // resolves to the roster rather than to whatever row it covers.
                settings_roster_scene(&state, here.key.as_ref()),
                // ★★★★★ R2021 — and the roster a CARD's setting opens, in the same
                // place for the same reason. Two calls rather than one branch
                // because the two are anchored in different frames — one to a row
                // on a page, one to a panel on a board that scrolls — and folding
                // them together would mean one of the two anchors being computed
                // where it cannot see what it needs.
                card_roster_scene(&state, here.key.as_ref()),
                // ★ The band is always there, so it says `Some` rather than
                // being chained separately: the ORDER of these five is what
                // decides what covers what, and splitting the always-present
                // one out would put that order in two places.
                Some(status_band_scene(&state, palette, &theme)),
                // ★★★★★ R1916 — the description a reader is resting on, over
                // everything and last, because it is content ABOUT what is
                // under it. The canon's own `title` tooltips draw the same way.
                shell_tip_scene(&state, palette),
            ]
            .into_iter()
            .flatten(),
        )
        .collect::<Vec<_>>();

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(VIEW_TAG)
            .with_style(BoxStyle::filled(palette.canvas))
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(win_w(), win_h()))
                    // ★★★★★ R1735 — **this screen is the drop region**, so the
                    // router resolves a drag over it to the surface that
                    // declared, not to whichever inner label happened to be the
                    // deepest tag under the cursor.
                    //
                    // R1734 declared what the board accepts and nothing could
                    // route to it: the fallback resolution names the deepest
                    // painted tag, and there is no `External` behind
                    // `shell.canvas` or `shell.palette.<kind>` to ask. This is
                    // the opt-in the dock panels have had since R1080, said by
                    // the one node that IS the surface — which is also why the
                    // declaration can only be `DropRegion::Surface`: the point
                    // that arrives is normalised over this whole rectangle.
                    .with_drop_target(true),
            ),
    )
}

/// The dashboard destination's page, region-local: the board, and the cards
/// torn off it.
fn dashboard_scene(state: &ShellState, palette: Palette) -> Vec<Scene> {
    let board = state.board.get();
    let selected = state.selected.get();
    let editing = state.editing.get();
    let drag = state.drag.get();
    // R1697 — which card, if any, wears the restore face of the maximise
    // control. Read once here rather than per card so the paint and the toggle
    // read one fact.
    let maximized = state.maximized.get();

    let canvas = canvas_rect();
    // ★★★★★ R1733 — the board grows while something is being carried.
    //
    // Read from the behaviour reference's own arithmetic: its overlay row count
    // is the tallest tile's bottom plus **three** rows while a drag is in
    // flight and plus one otherwise. Without it there is nowhere to drop a card
    // *below* everything already placed — the grid simply stops, and the last
    // row of the board is the last row a drop can reach.
    let carrying_rows = if drag.is_some() { 3 } else { 1 };
    let mut canvas_children = grid_scene(
        board.rows() + carrying_rows,
        palette,
        editing || drag.is_some(),
    );
    for card in &state.placed() {
        let Some(tile) = board.tile(card.id()) else {
            continue;
        };
        // ★★★★★ R1900 — a shared cell draws the occupant in FRONT and nobody
        // else. The others are not hidden by being painted over: they are not
        // built, which is the R1695 rule about a page that is not current, and
        // it is what makes the strip the only way to reach them.
        if &tile.id != card.id() {
            continue;
        }
        canvas_children.push(card_scene(
            state,
            card,
            cell_rect(tile),
            (tile.w, tile.h),
            palette,
            CardFace {
                selected: selected.as_deref() == Some(card.id().as_str()),
                editing,
                maximized: maximized.as_ref().is_some_and(|m| m.id() == card.id()),
                sharing: if tile.is_shared() {
                    tile.members().to_vec()
                } else {
                    Vec::new()
                },
                fore: tile.fore_index(),
            },
        ));
    }
    // ★★★★★ R2021 — a card's settings panel, over the cards and under the
    // carry. A sibling of the cards rather than a child of the one that opened
    // it, so it hangs over its neighbours the way the prototype's does — and so
    // that the card's own container does not clip it, which is what a popup
    // drawn inside its opener always suffers.
    canvas_children.extend(card_config_scene(state, palette));
    // ★★★★★ R1726 — the snap preview, and then the card being HELD above it.
    //
    // The order here is three deep and every layer was measured, because two of
    // the three arrangements are wrong and each is wrong in a way the other
    // hides:
    //
    // * preview LAST (what shipped): it is opaque, so it covered the card being
    //   dragged whole — reported as "while dragging the widget's interior is
    //   just grey". The widget had lost nothing; it was underneath.
    // * preview FIRST (this round's first repair): the card is visible again,
    //   but the preview now hides behind whatever already occupies the target
    //   cell — which is every drag ONTO another widget, i.e. the normal one. A
    //   drag with no visible destination reads as the thing you are holding
    //   having vanished.
    // * preview after the resting cards, the held card after the preview: the
    //   destination is visible over the board AND the card you are holding is
    //   on top of both. That is this.
    //
    // The lift is what makes the third arrangement expressible at all: without
    // it there is no layer between "over the cards" and "under the dragged
    // one", because the dragged card IS one of the cards.
    if let Some(drag) = &drag {
        // ★★★★★ R1900 — WHICH preview is the classifier's, and the classifier
        // is the release's. A join has no cell to mark: the card is going into
        // a place that already exists, so marking a cell would promise a
        // placement that is not going to happen.
        canvas_children.extend(berth_preview_scene(state, &board, drag, palette));
        // Third consumer of the held derivation, and the one that made it a
        // function rather than only a `ContainerNode` builder — this board
        // never holds a container of its own, it hands its cards to a pane
        // helper.
        //
        // ★ R1733 — only a card the board already holds can be raised. A
        // palette footprint has no card on the canvas to lift, and asking for
        // one by name would quietly raise nothing; the `match` is what says so.
        if drag.carried().is_placed() {
            let held = [format!("card.{}", drag.carried().id())];
            pinion_core::held::raise_to_front(
                &mut canvas_children,
                &held,
                pinion_core::held::HELD_SHADOW,
            );
        }
    }
    // ★★★★★ R1726 — **the label that follows the cursor**, which the behaviour
    // reference has and this board did not.
    //
    // Read from the reference's own source rather than guessed: it keeps THREE
    // things during a board drag — the widget stays where it is, a snap mark
    // shows the destination cell, and a small chip rides the cursor carrying
    // the widget's NAME (`dragGhost`, offset +14/+10 from the pointer, above
    // everything, transparent to it). We had the first two. Without the third
    // the gesture never says WHAT is being carried, which is what "shouldn't
    // the widget show while dragging" is asking for — and the answer the
    // reference gives is a name, not a copy of the widget.
    //
    // It is NOT inside the scrolling board: it follows the pointer in window
    // coordinates, so it must not slide when the board does.
    //
    // ★ R1733 — and it is what a palette drag has INSTEAD of a card standing
    // still on the board: a footprint being carried has nothing painted at its
    // source, so this chip is the only thing that says what is in hand. The
    // reference gets that for free from the browser's own drag image; a
    // framework that draws its own scene has to draw it.
    let drag_label = drag.as_ref().map(|drag| {
        let (cx, cy) = state.cursor.get();
        carried_label(&label_of(drag.carried().id().as_str()), cx, cy, palette)
    });
    let canvas_children = canvas_children;
    // ★ R1662 — the BOARD scrolls and the floats do not. A torn-off card is
    // chrome over the canvas, so it keeps its place when the board slides;
    // that is the same split the node lab makes between its world surface and
    // the gate panel floating over it. The scroll range is derived from the
    // cards themselves by the pane, so a board that grows a row cannot outrun
    // a number written here.
    let mut canvas_children = vec![
        scroll_pane(
            &state.canvas_scroll,
            Rect::new(0, 0, canvas.w, canvas.h),
            (0, GAP),
            // Every press goes to the one root `External` that runs this
            // screen's own hit test, so the pane must be invisible to the
            // router (R1655).
            PanePointer::PassesThrough,
            canvas_children,
        )
        // The viewport is a clip rather than a thing on the screen: what a
        // reader walks is the board inside it.
        .silenced(Silence::layout("the board's scrolling viewport")),
    ];
    // ★★★★★ R1733 — **the invitation**, which the reference draws over the
    // whole canvas the moment a palette footprint is carried onto it.
    //
    // Read from its own markup: while its board is being dragged onto, a dashed
    // accent frame inset eight pixels covers the canvas and says, in words, that
    // letting go places the widget. It is not decoration — it is the only thing
    // that distinguishes "carrying something onto this" from "the grid happens
    // to be brighter", and a person who has picked a row up and is unsure what
    // happens next is exactly the reader it answers.
    //
    // Only for a FRESH carry. Moving a card the board already holds is not an
    // add, and the reference agrees: it is the palette handlers that set the
    // flag this reads, and the reorder path leaves it alone.
    if drag
        .as_ref()
        .is_some_and(|d| !d.carried().is_placed() && d.landing().is_some())
    {
        canvas_children.push(Scene::Container(
            ContainerNode::new(vec![label(
                spec::DROP_INVITATION,
                Rect::new(0, canvas.h / 2 - 24, canvas.w - 16, 20),
                FONT_BODY,
                palette.accent_fg,
            )])
            .with_tag("shell.carry.banner")
            .with_style(
                BoxStyle::filled(Color::rgba(0, 0, 0, 0))
                    .with_corner_radius(16)
                    .with_border(Border::new(palette.accent_fg, 2)),
            )
            .with_layout(absolute(Rect::new(
                8,
                8,
                canvas.w.saturating_sub(16),
                canvas.h.saturating_sub(16),
            ))),
        ));
    }
    // ★ R1697 — back to front, which is the REVERSE of the order the hit test
    // walks. Both read `floats_front_to_back`, so painting the frontmost panel
    // last and hitting it first are one decision rather than two that agree
    // until somebody changes one of them.
    // ★ R1891 — and only the CANVAS-homed ones are painted here. A window-homed
    // card is painted by its own window, which is why this used to draw a
    // second copy of it.
    for float in state.floats_at(DetachHome::Canvas).iter().rev() {
        if let Some(scene) = float_scene(state, float, palette) {
            canvas_children.push(scene);
        }
    }
    // ★★★★★ R1726 — the carried label rides the cursor, so it is LAST: above
    // the board, above the torn-off panels, above everything. It is the one
    // thing on this screen whose position is the pointer's rather than the
    // layout's.
    canvas_children.extend(drag_label);
    canvas_children
}

struct AnalyzerShellView;

impl WidgetCore for AnalyzerShellView {
    /// ★★★★★ R1724 — **where the application is, and how far the screen it is
    /// showing has moved.**
    ///
    /// This was `()`, which was true while every page was this file's own: the
    /// board, the cards and the switches are signals, and a signal repaints on
    /// its own. It stops being true the moment a page is another binding whose
    /// projection comes out of the state scene — a mounted screen's text field
    /// would paint its first frame and no other, because nothing this shell
    /// declares would ever differ.
    type State = ScreenState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = ShellOracle::new();
        oracle.attach_state(use_shell_state());
        Box::new(oracle)
    }

    /// ★★★★★ R1724 — the surfaces of whichever screen is showing, and nobody
    /// else's.
    ///
    /// This is the whole of "a section you are not in cannot be pressed": the
    /// externals of a screen the journey is not at are not in the state scene,
    /// so the §5.35 router has no target and the wire has no slot. Measured at
    /// 6.11.1, a page of the reference toolkit's paged container that is not
    /// showing counted a press, a key and a wheel.
    fn create_extra_externals() -> Vec<pinion_core::widget_core::ExtraExternal> {
        let state = use_shell_state();
        state.screens.externals(&state.journey.get())
    }

    /// R1724 — the surface set IS the current screen's, so it changes whenever
    /// the rail does.
    fn external_set_is_dynamic() -> bool {
        true
    }

    fn tag() -> &'static str {
        VIEW_TAG
    }

    /// R1724 — read the current screen's projection out of the state scene and
    /// park it on its mount, reporting where we are as the `Copy` value the
    /// framework compares frame to frame.
    fn read_state(scene: &Scene) -> ScreenState {
        let state = use_shell_state();
        state.screens.latch(&state.journey.get(), scene)
    }

    fn view(state: ScreenState, frame: &Frame) -> Scene {
        view(state, *frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-analyzer-shell (R1649 §5.21 analysis-tool dashboard shell)"
    }

    /// ★★★★★ R1698 — **the hook this screen did not have, so its keyboard was
    /// not reachable from a keyboard.**
    ///
    /// Measured before it existed, by driving the running application: the wire
    /// `invoke("key", "ArrowRight")` moved the board's selection and a REAL
    /// `scene/key` press moved nothing at all. The screen publishes a keymap of
    /// twelve chords through `KEYMAP` and not one of them was reachable by
    /// pressing a key. Every test that drove the keyboard passed because every
    /// test drove it through the wire — R1693's lesson, on the sibling screen,
    /// recurring here: *the test and the defect were the same mistake.*
    ///
    /// The census across all 225 examples: 172 bindings implement this hook and
    /// 135 of them read `focused`. This screen implemented it zero times.
    ///
    /// `focused` is threaded through rather than dropped, which is what makes
    /// the composite cursors work at all: the arrows have to mean something
    /// different depending on which composite the reader is standing in, and
    /// three of this tree's bindings forward a key to one External without ever
    /// asking where focus is.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        chord: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        let state = use_shell_state();
        // ★★★★★ R1724 — the showing screen answers first, and the shell's own
        // chrome second. Both are on screen at once, so both have to be
        // reachable; the guest goes first because the chord a person types
        // while looking at a section belongs to that section.
        if state.screens.with_current(&state.journey.get(), |screen| {
            screen.apply_key(scene, focused, chord, modifiers)
        }) == Some(true)
        {
            return true;
        }
        ShellOracle::key_at(&state, focused, chord)
    }
}

impl WidgetA11y for AnalyzerShellView {
    /// ★★★★★ R1698 — **where the cursor rests inside the focused composite.**
    ///
    /// WAI-ARIA's composite model in two lines: the AT focus stays on the
    /// composite and `aria-activedescendant` names the member the arrows are
    /// on. This screen returned nothing at all before, so an assistive
    /// technology landing on the rail was told "Destinations" and never which
    /// destination the cursor was on — and the framework's focus ring framed
    /// the whole bar rather than the member.
    ///
    /// The ring comes free with it: `resolve_focus_ring_tag` reads exactly this
    /// hook, so publishing the descendant is also what makes the cursor visible
    /// on screen.
    /// ★ R1724 — a stop that is not one of this shell's own belongs to the
    /// screen showing in the page region.
    ///
    /// Decided by **ownership** (`spec::FOCUS_RING` is this shell's stop
    /// table) rather than by the shape of the guest's answer: the trait's
    /// default returns an atomic focus for whatever it is handed, so "the
    /// guest said something" cannot be told from "the guest has nothing to
    /// say".
    fn access_focus_target(_state: &ScreenState, focused: Option<&str>) -> Option<AccessFocus> {
        let stop = focused?;
        let state = use_shell_state();
        if !spec::FOCUS_RING.iter().any(|own| own.tag == stop)
            && let Some(target) = state.screens.with_current(&state.journey.get(), |screen| {
                screen.access_focus_target(Some(stop))
            })
        {
            return target;
        }
        Some(AccessFocus::addressing(
            stop,
            active_descendant(&state, stop),
        ))
    }

    /// ★★★★★ R1694 — **the screen a reader can walk, locked seats included.**
    ///
    /// Before this round the dashboard painted 128 addressable regions and
    /// announced five: a group for the window and one per card, each holding
    /// nothing. The rail, both bars, two tables, the decode tree, seventy-two
    /// bytes and the whole palette were not in the tree at all — and with them
    /// went the screen's own claim, which is that **nine seats are locked and
    /// each says what it is booked under**. The framework has computed that
    /// reason since R1668 and published it on `scene/disabled`; none of the
    /// eleven locked regions had a node to carry it.
    ///
    /// Measured at 6.11.1 by building and running the same shape: a locked entry
    /// in an item view and a locked destination in a tab bar answer
    /// `focusable, selectable` and carry **no unavailable state at all** — the
    /// bit survives only on a plain widget — so a reader there is invited to
    /// activate exactly the seats the screen has closed. Here every locked seat
    /// is announced unavailable, keeps its place in the set, and carries the
    /// kind, the detail and the recourse the bit cannot hold.
    /// ★★ R1695 — and the tree now follows the rail. A destination's nodes are
    /// emitted only where that destination is showing, which is the same
    /// property the paint has and for the same reason: a reader offered a
    /// control that is not on screen is offered a control nobody can reach.
    /// ★★★★★ R1724 — and a page that is a whole screen brings **its own**
    /// tree, under this region, only while it is showing.
    ///
    /// Measured at 6.11.1: a page of the paged container that is NOT showing is
    /// reachable as an accessible child with its text field under it, marked
    /// `invisible` and nothing more. Here the screen that is not showing was
    /// never built, so there is nothing to mark.
    fn access_node(_state: &ScreenState, focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_shell_state();
        let journey = state.journey.get();
        let here = journey.here(state.roster());
        let dashboard = spec::shows_board_chrome(here.key.as_ref());
        let mut root = AccessNode::new(VIEW_TAG, AriaRole::Group)
            .with_name("Analyzer dashboard")
            .with_value(AccessValue::Text(format!(
                "{} of {} widgets placed on layout \"{}\", {} reserved, source {}",
                state.placed().len(),
                spec::placeable_count(),
                state.preset.get(),
                spec::reserved_count(),
                state.source.get(),
            )))
            .with_child("shell.appbar")
            .with_child("shell.rail");
        if dashboard {
            root = root.with_child("shell.subbar");
        }
        root = root.with_child("shell.canvas");
        if dashboard {
            // ★ R1903 — the child is whichever of the two the panel currently
            // IS, read from the same placement the paint reads. A tree naming
            // `shell.palette` while the screen draws a strip would be the
            // announce-what-is-not-painted class this tree already has a name
            // for.
            root = root.with_child(if palette_placement().folded {
                "shell.palette.strip"
            } else {
                "shell.palette"
            });
        }
        // ★ R1867 — the status band's slot has two occupants and the tree
        // carries whichever is PAINTED. `shell.toast` is a live region and
        // stays whether or not it holds a sentence (R1778); the gesture
        // sentence is not live and is only there when no toast has taken its
        // place, so announcing it unconditionally would be a name with no
        // region behind it — `Voice::Ghost`, which is a defect the census
        // reports and this round has no reason to author.
        let mut root = root.with_child("shell.toast");
        if state.toast.showing().is_none() {
            root = root.with_child(STATUS_GESTURE);
        }
        let mut nodes = vec![root];
        nodes.extend(app_bar_nodes(&state));
        nodes.extend(rail_nodes(&state));
        // The region says which destination arrived — the fact the reference
        // toolkit's paged container leaves empty on its own accessible value.
        let mut region = page_region_node("shell.canvas", here);
        if dashboard {
            let (value, children, cards) = board_nodes(&state);
            region = region.with_value(AccessValue::Text(value));
            for child in children {
                region = region.with_child(child);
            }
            nodes.push(region);
            nodes.extend(sub_bar_nodes(&state));
            nodes.extend(cards);
            nodes.extend(palette_nodes(&state));
        } else if let Some(mounted) = state
            .screens
            .with_current(&journey, |screen| screen.access_node(focused))
        {
            // ★★★★★ R1724 — the mounted screen's tree, parented to the region
            // it is painted in. The screen's own root is the region's child, so
            // a reader walking the rail arrives at the section rather than at a
            // rectangle with nothing in it.
            if let Some(root) = mounted.first() {
                region = region.with_child(root.tag.clone());
            }
            nodes.push(region);
            nodes.extend(mounted);
        } else {
            // ★ R1762 — the page's own heading is this region's NAME and the
            // line under it is its value, which is why both painted marks
            // declare themselves silent: a reader who heard them and the region
            // would hear the page twice.
            region = region
                .with_name(spec::SETTINGS_HEAD.0)
                .with_value(AccessValue::Text(spec::SETTINGS_HEAD.1.to_owned()));
            let (children, rows) = settings_nodes(&state);
            for child in children {
                region = region.with_child(child);
            }
            nodes.push(region);
            nodes.extend(rows);
        }
        nodes.push(
            // ★★★★★ R1719 — the urgency is derived. `Polite`, flat, meant a
            // refused widget placement waited for a pause while every theme
            // change got the same treatment.
            AccessNode::new("shell.toast", AriaRole::Status)
                .with_name("Activity")
                .with_value(AccessValue::Text(state.toast.sentence()))
                // ★★ R1778 — the urgency of what is CURRENTLY said, and the
                // polite one when nothing is. A live region stays in the tree
                // whether or not it holds a sentence, so this needs an answer
                // for silence; `Done`'s is the right one, because an empty
                // region has nothing to interrupt a reader about.
                .with_live(AccessLive::for_urgency(
                    state
                        .toast
                        .showing()
                        .map_or(Tone::Done, |said| said.tone())
                        .urgency(),
                )),
        );
        if state.toast.showing().is_none() {
            // ★★★★★ R1867 — the host's gesture help, heard.
            //
            // `Status` because that is what this shell already calls a strip
            // that states a fact rather than takes a press — `shell.settings
            // .build` is the same shape and the same role — and NOT live: a
            // sentence that has been there since the window opened has nothing
            // to interrupt a reader about.
            nodes.push(
                AccessNode::new(STATUS_GESTURE, AriaRole::Status)
                    .with_name("Gestures")
                    .with_value(AccessValue::Text(HELP_STRIP.to_owned())),
            );
        }
        // ★★★★★ R1916 — and the description a reader is resting on, wired to
        // the mark it belongs to. The second consumer of
        // `pinion_core::describe`, and the one that closes the canon's bare
        // hover: this shell's own chrome carries icons with no room to print
        // what they do, which is the reason the canon puts a `title` on them.
        //
        // ★★★★★ R1918 — the four steps are the substrate's now, shared with the
        // five other screens of this application that draw one.
        if let Some((tag, sentence)) = shell_description_shown(&state, focused) {
            pinion_widget_paint::described::announce_description(
                &mut nodes, &tag, SHELL_TIP, &sentence,
            );
        }
        nodes
    }
}

/// The tag the shell's description region is painted and announced under.
const SHELL_TIP: &str = "shell.tip";

/// ★★★★★ R1918 — the sentences this shell's CHROME carries, by paint tag.
///
/// Chrome is what the host paints at **every** destination, so what is
/// described here is described everywhere — which is exactly why it is a
/// separate register from [`page_descriptions`]. A gate asking *does this page
/// say anything about its own marks* must not be satisfied by a mark that
/// belongs to the frame around the page, and the only way to keep that honest
/// is for the two populations to be two values rather than one filtered later.
///
/// ★ The canon's own choice, measured this round out of its markup: nine of its
/// twenty-five `title` attributes are chrome — its eight rail seats and its
/// appearance toggle — and the rest are spread over its pages. It titles
/// **every** rail seat, which is what an icon rail with no text is: eight marks
/// with no room to print what they do.
///
/// ⚠ Only the rail is here, and that is not a shortfall: this build has no
/// appearance toggle in its application bar. Its theme control is a segment on
/// the settings PAGE, so it is described by [`page_descriptions`] — the canon's
/// ninth chrome title is a page mark here because this build put the control
/// somewhere else.
///
/// The sentence is derived from the seat's own declaration rather than authored
/// per seat, so a seat added to [`spec::RAIL`] arrives described.
fn chrome_descriptions() -> pinion_core::describe::Descriptions {
    let mut described = pinion_core::describe::Descriptions::new();
    // ★★★★★ R1946 — a seat this build is behind the BEHAVIOUR reference on
    // says so, and the set is derived from the pin rather than named here.
    //
    // A person opened the window and asked why two of these sections are not
    // there at all. What the seat could answer was the requirement that books
    // it, which is the SCOPE mockup's fact and true — and silent about the one
    // thing the question was actually about: the working reference has both
    // sections, and this build has not built them yet. That silence was not a
    // wording choice; nothing in this tree held the fact until this round.
    let behind = spec::second_phase_owed();
    for seat in spec::RAIL {
        let sentence = match seat.reserved_for() {
            // ★ The reserved seats already SAY their requirement — the rail
            // announces it and the refusal repeats it. What they could not do
            // is say it to a reader who is only looking, which is the debt this
            // register closes on this axis.
            Some(why) if behind.iter().any(|key| key == seat.key) => format!(
                "{} is not in this release - booked under {why}; the reference draws it \
                 and this build does not yet",
                seat.title
            ),
            Some(why) => format!("{} is not in this release - booked under {why}", seat.title),
            None => format!("Go to {}", seat.title),
        };
        described.describe(format!("shell.rail.{}", seat.key), sentence);
    }
    described
}

/// ★★★★★ R1918 — the sentences the page THIS HOST PAINTS at `at` carries.
///
/// Empty at a destination whose page is a mounted screen: that screen keeps its
/// own register and publishes it on its own surface. A host that answered for a
/// page it does not paint would be answering for a register it cannot see.
///
/// ★★ R1916 built the first of these (the board's card chrome) and this round
/// added the settings page's, which is the other page this host paints itself.
/// The rule for WHICH marks is the canon's: the ones with no room to print what
/// they do. Every switch row on the settings page prints its own gist under it
/// and is deliberately **not** here; the locked rows' buttons say `Import…` and
/// nothing about why they refuse, and the appearance segment's two words say
/// nothing at all.
fn page_descriptions(state: &ShellState, at: &str) -> pinion_core::describe::Descriptions {
    let mut described = pinion_core::describe::Descriptions::new();
    match at {
        "dashboard" => {
            for card in state.placed() {
                let id = card.id();
                let title = card.title();
                described.describe(
                    format!("card.{id}.grip"),
                    format!("Drag to move {title} to another place on the board"),
                );
                described.describe(
                    format!("card.{id}.widen"),
                    format!("Give {title} more of the board's width"),
                );
            }
        }
        "settings" => {
            for row in spec::KEY_ROWS {
                described.describe(
                    format!("shell.settings.key.{}", row.key),
                    format!(
                        "{} is not in this release - booked under {}",
                        row.verb.trim_end_matches('\u{2026}').trim(),
                        row.reserved_for.unwrap_or(spec::UNBOOKED)
                    ),
                );
            }
            for (n, name) in spec::THEMES.iter().enumerate() {
                described.describe(
                    format!("shell.settings.theme.{n}"),
                    format!("Set the whole application to the {name} appearance"),
                );
            }
        }
        _ => {}
    }
    described
}

/// ★★★★★ R1918 — the two registers as data, with the mark they are drawn and
/// announced under.
///
/// `chrome` and `page` are separate keys and not one map, because the claim
/// *this page says something about its own marks* is the one the debt closed
/// here is about, and a joined map cannot be asked it.
fn described_wire(state: &ShellState) -> serde_json::Value {
    let rows = |described: &pinion_core::describe::Descriptions| {
        described
            .tags()
            .map(|tag| {
                serde_json::json!({
                    "tag": tag,
                    "sentence": described.of(tag).unwrap_or_default(),
                })
            })
            .collect::<Vec<_>>()
    };
    let at = state.at();
    let page = page_rect(&at);
    serde_json::json!({
        "region": SHELL_TIP,
        "at": at,
        // ★★★★★ R1918 — **the rectangle the page occupies at this
        // destination**, published beside the two registers because it is what
        // tells the two apart on the FRAME rather than on this host's word. A
        // reader can now check that a page's described marks are inside its own
        // region, which is the difference between *this page says something*
        // and *something on this window does*.
        "page_at": [page.x, page.y, page.w, page.h],
        "chrome": rows(&chrome_descriptions()),
        "page": rows(&page_descriptions(state, &at)),
    })
}

/// Both registers as one, which is what the drawn surface reads.
///
/// The surface does not care which population a sentence came from — a reader
/// resting on a rail seat and a reader resting on a card grip are shown one
/// thing the same way. The split exists for the gate, not for the drawing.
fn shell_descriptions(state: &ShellState) -> pinion_core::describe::Descriptions {
    let mut described = chrome_descriptions();
    described.merge(&page_descriptions(state, &state.at()));
    described
}

/// ★★★★★ R1916 — the shell description a reader is being shown, as
/// `(tag, sentence)`.
///
/// The same shape the node lab's is, and deliberately so: the screen hands the
/// substrate where the reader's attention is and is handed back what to show.
/// Neither screen carries `hovered == tag`, which is what the debt this closes
/// named as the thing to avoid.
fn shell_description_shown(
    state: &Rc<ShellState>,
    focused: Option<&str>,
) -> Option<(String, String)> {
    let described = shell_descriptions(state);
    let cursor = state.cursor.get();
    // ★ `hit_word` is the shell's OWN tag for what the pointer is over — the
    // same function the pointer-target census reads — so the register's keys
    // and the hit's answer are one spelling rather than two.
    let hovered = state
        .pointer_inside
        .get()
        .then(|| hit_word(&Hit::at(state, cursor.0, cursor.1)));
    // ★★★★★ R1918 — the keyboard reader's attention is the INNERMOST thing
    // inside the Tab stop, and this is the same derivation the accessibility
    // tree frames with. Passing the raw stop answered nothing for every
    // described mark on this screen, because none of them IS a stop — a rail
    // seat, a card grip and a settings row all live inside one.
    let attention = focused.and_then(|stop| active_descendant(state, stop));
    let shown = described.shown(&pinion_core::describe::Resting {
        hovered: hovered.as_deref(),
        focused: attention.as_deref().or(focused),
        dismissed: false,
    })?;
    Some((shown.tag.to_owned(), shown.sentence.to_owned()))
}

/// The application bar: which view is open, what capture is being read, how fast
/// it is arriving, and the search field.
fn app_bar_nodes(state: &Rc<ShellState>) -> Vec<AccessNode> {
    let current = state.tab.get();
    let searching = state.searching.get();
    let search = state.search.get();
    let mut tabs = AccessNode::new(APP_BAR_TABS, AriaRole::TabList).with_name("View");
    // ★★★★★ R1699 — a nested composite publishes its OWN axis and roster, so a
    // client learns what the inner arrows reach without descending first. The
    // roving is the live one the bar holds, not a fresh copy, or the cursor the
    // wire reports and the cursor the arrows move would be two objects.
    if let Some(inner) = state.inner_cursor_of("shell.appbar", APP_BAR_TABS) {
        tabs = tabs.with_navigation(&inner);
    }
    let mut nodes = Vec::new();
    for (n, tab) in spec::VIEW_TABS.iter().enumerate() {
        let name = tab.title;
        let tag = BarChip::Tab(n).tag();
        tabs = tabs.with_child(&tag);
        nodes.push(
            AccessNode::new(&tag, AriaRole::Tab)
                .with_name(name)
                .with_selected(current == name)
                .with_set_position(n, spec::VIEW_TABS.len()),
        );
    }
    nodes.insert(0, tabs);
    nodes.insert(
        0,
        with_cursor_declared(
            AccessNode::new("shell.appbar", AriaRole::Toolbar)
                .with_name("Application bar")
                .with_child(APP_BAR_TABS)
                .with_child(BarChip::Source.tag())
                .with_child(BarChip::Capture.tag())
                .with_child(BarChip::Search.tag()),
            state,
        ),
    );
    nodes.push(
        AccessNode::new(BarChip::Source.tag(), AriaRole::Button)
            .with_name("Capture source")
            .with_value(AccessValue::Text(state.source.get()))
            .with_has_popup(HasPopup::Menu),
    );
    // The rate readout moves while nobody touches it, which is what a live
    // region is for — and the only region on this bar that is one.
    nodes.push(
        AccessNode::new(BarChip::Capture.tag(), AriaRole::Status)
            .with_name("Capture")
            .with_value(AccessValue::Text(format!(
                "{}, {}",
                if state.capturing.get() {
                    spec::TRANSPORT
                } else {
                    "Paused"
                },
                spec::RATE,
            )))
            .with_live(AccessLive::Polite),
    );
    nodes.push(
        AccessNode::new(BarChip::Search.tag(), AriaRole::TextInput)
            .with_name("Search")
            // An empty field announces what it is FOR rather than the hint text
            // painted in it: the hint is a placeholder, and reading it as the
            // value would tell a reader the field already holds those words.
            .with_value(AccessValue::Text(search))
            .with_focused(searching),
    );
    nodes
}

/// ★★★★★ R1698 — publish the cursor this composite owns, if the specification
/// gave it one.
///
/// One place, read from the state's own seated `Roving`, so what the wire says
/// the arrows reach and what the arrows actually reach are the same object
/// rather than two lists that agree today. A stop the ring declares with no
/// cursor (the board, whose cursor is spatial) passes through unchanged.
/// ★★★★★ R1918 — **where a keyboard reader's attention actually is**, inside
/// the Tab stop that holds it.
///
/// Lifted out of `AnalyzerShellView::access_focus_target` because R1918 needed a
/// SECOND reader of it and the two must not disagree: the accessibility tree
/// frames the innermost thing, and a description has to be about the same
/// thing. Measured this round on the running shell — a keyboard reader on the
/// rail is focused on `shell.rail`, and every mark this application describes
/// there is a `shell.rail.<seat>` INSIDE it, so a description keyed on the raw
/// focus answered nothing for a keyboard reader on any page. The walk found it:
/// `focus/set` on a described mark was refused `tag_not_focusable`, which is
/// the honest report that a described mark is not itself a stop.
///
/// ⚠ A stop with no interior answers `None` and the caller falls back to the
/// stop itself, which is right: for a plain control the stop IS the thing.
fn active_descendant(state: &Rc<ShellState>, stop: &str) -> Option<String> {
    state
        .cursor_of(stop)
        // ★★★★★ R1699 — the INNERMOST tag, not the member at this level.
        // ARIA's `aria-activedescendant` addresses any descendant of the
        // element owning the Tab stop, and the framework's focus ring reads
        // this same hook, so a cursor that has gone into the tab list frames
        // the tab rather than the list.
        .and_then(|roving| roving.active_descendant().map(str::to_owned))
        // The board's cursor is its selection: it is spatial rather than a
        // linear roster, so it declares no `Roving` and reports the card it
        // is on. It has had that cursor since R1662 and published it to
        // nobody.
        .or_else(|| {
            (stop == "shell.canvas")
                .then(|| state.selected.get().map(|id| format!("card.{id}")))
                .flatten()
        })
}

fn with_cursor_declared(node: AccessNode, state: &Rc<ShellState>) -> AccessNode {
    match state.cursor_of(&node.tag) {
        Some(roving) => node.with_navigation(&roving),
        None => node,
    }
}

/// R1699 — the initials the account chip shows, painted and announced from one
/// place so a reader hears what is drawn.
const ACCOUNT_INITIALS: &str = "NE";

// ★★★★★ R1719 — `refusal_sentence` used to live here. R1699 wrote it because
// four call sites in this file (and four in the node lab) had written
// `format!("refused: {why:?}")`, which put `Rejected(RefusalReason("…"))` on
// screen — Rust syntax and escaped quotes in front of somebody who asked to
// place a widget — and its own note said that a screen which has to remember
// not to use `Debug` is a screen that will use `Debug`.
//
// It was right, and a screen-local helper is the wrong place for it: the other
// two screens of this tool never got one, so one of them was still writing the
// frame by hand this morning. The rule is `Utterance::refused` now, and the
// `Debug` spelling it was protecting against is a fault the constructor names
// rather than a habit each screen has to keep.

/// The tag the two view tabs are announced under. Nothing paints it — the tabs
/// are painted individually and the list is what a reader descends through — so
/// it is anchored by the members it composes.
const APP_BAR_TABS: &str = "shell.appbar.tabs";

/// The rail: seven destinations and the account seat, two of them **locked**.
///
/// Built through [`navigation_link_nodes`] rather than by hand, and the reason
/// this screen is that builder's forcing consumer: a destination that is not
/// available is exactly what its `unavailable` slot exists for. The reason
/// itself is NOT restated here — the seat declares it once on its layout style
/// and the accessibility assembler relays what the disabled cascade resolved, so
/// there is one declaration and no second spelling to drift from it.
fn rail_nodes(state: &Rc<ShellState>) -> Vec<AccessNode> {
    let here = state.at();
    let tags: Vec<String> = spec::RAIL
        .iter()
        .map(|seat| format!("shell.rail.{}", seat.key))
        .collect();
    let links: Vec<NavLink<'_>> = spec::RAIL
        .iter()
        .zip(&tags)
        .map(|(seat, tag)| NavLink {
            tag: tag.as_str(),
            label: seat.title,
            state: RadioState::Idle,
            current: seat.key == here,
            focused: false,
            unavailable: None,
        })
        .collect();
    let mut nodes = navigation_link_nodes("shell.rail", "Destinations", &links);
    if let Some(rail) = nodes.first_mut() {
        rail.children.push("shell.rail.account".to_owned());
        *rail = with_cursor_declared(rail.clone(), state);
    }
    // ★★★★★ R1699 — a `group`, not a `button`, and NOT a member of the rail's
    // cursor.
    //
    // Both of those were false claims and the round's own gate is what asked.
    // Nothing presses this seat: no arm of `Hit::at` reaches its rectangle and
    // none of `Hit::of_tag` names it, so it was a control by announcement only —
    // the shape R1694 kept finding from the other side. And the canon settles
    // which direction to repair it in: read out of the reference mockup's own
    // source, the avatar is a plain styled element with no handler, no link and
    // no menu. It says whose session this is. Making it press something would
    // have been inventing a product decision; making it honest costs nothing
    // and removes two lies.
    nodes.push(
        AccessNode::new("shell.rail.account", AriaRole::Group)
            .with_name("Account")
            .with_value(AccessValue::Text(ACCOUNT_INITIALS.to_owned())),
    );
    nodes
}

/// The layout bar: which layout is open, and the two verbs that change the
/// board.
fn sub_bar_nodes(state: &Rc<ShellState>) -> Vec<AccessNode> {
    vec![
        with_cursor_declared(
            AccessNode::new("shell.subbar", AriaRole::Toolbar)
                .with_name("Layout bar")
                // ★ R1761 — and how many widgets it is holding, which the bar
                // paints beside the preset and had announced nowhere in this
                // bar. Carried by the toolbar rather than published as a fourth
                // child, because it is the bar's own readout and not a stop a
                // cursor should land on — which is also what lets the painted
                // mark declare itself part of this announcement instead of
                // being a second voice for one fact.
                .with_value(AccessValue::Text(format!(
                    "{} widgets placed",
                    state.placed().len()
                )))
                .with_child(SubChip::Preset.tag())
                .with_child(SubChip::EditLayout.tag())
                .with_child(SubChip::AddWidget.tag()),
            state,
        ),
        AccessNode::new(SubChip::Preset.tag(), AriaRole::Button)
            .with_name("Layout preset")
            .with_value(AccessValue::Text(state.preset.get()))
            .with_has_popup(HasPopup::Menu)
            .with_expanded(state.preset_open.get()),
        AccessNode::new(SubChip::EditLayout.tag(), AriaRole::Button)
            .with_name(if state.editing.get() {
                "Done"
            } else {
                spec::BOARD_VERBS[0]
            })
            .with_state(AccessState {
                checked: Some(state.editing.get()),
                ..AccessState::default()
            }),
        AccessNode::new(SubChip::AddWidget.tag(), AriaRole::Button).with_name(spec::BOARD_VERBS[1]),
    ]
}

/// Every card placed on the board, and the tags the page region owns them by.
///
/// ★ R1695 — the board's own node is gone from here: the rectangle it described
/// is the **page region** now, and a region that is also a board would be two
/// nodes at one tag. What the board contributes is what it is — a value saying
/// how full it is, and the children.
fn board_nodes(state: &Rc<ShellState>) -> (String, Vec<String>, Vec<AccessNode>) {
    let cards = state.cards.get();
    let value = format!(
        "{} widget(s) placed on a {}-column grid",
        state.placed().len(),
        spec::GRID_COLS,
    );
    let mut children = Vec::new();
    let mut nodes = Vec::new();
    let board = state.board.get();
    for card in &cards {
        // ★★★★★ R1900 — a card BEHIND a tab is not announced, for the same
        // reason it is not painted: it is not on the screen. The tab that
        // selects it is, and it carries the card's name.
        //
        // This is the closing audit of this round finding its own hole: the
        // paint stopped building the occupants a strip covers, and this tree
        // went on publishing their rows — which is `announces a row it does not
        // paint`, the defect this project already has a name for. The two
        // decisions are one line each and they read the same fact, so they
        // cannot answer differently.
        if board
            .tile(card.id())
            .is_some_and(|cell| &cell.id != card.id())
        {
            continue;
        }
        children.push(format!("card.{}", card.id().as_str()));
        nodes.extend(card_nodes(state, card));
    }
    (value, children, nodes)
}

/// ★★ R1695 — the Settings destination, as a reader walks it: four groups, each
/// owning its rows.
///
/// Returns the region's children and the nodes, the same split the board makes,
/// so the page region owns whichever destination is showing.
///
/// The two key rows carry **no reason of their own here**: they declare it once
/// on their layout style and the assembler relays what the disabled cascade
/// resolved. A second spelling is a second thing to drift.
fn settings_nodes(state: &Rc<ShellState>) -> (Vec<String>, Vec<AccessNode>) {
    let mut children = Vec::new();
    let mut nodes = Vec::new();
    // ★ R1762 — the build strip, which the page closes with and which a reader
    // filing a defect is asked for first.
    children.push("shell.settings.build".to_owned());
    nodes.push(
        AccessNode::new("shell.settings.build", AriaRole::Status)
            .with_name("Build")
            .with_value(AccessValue::Text(spec::BUILD_STRIP.join(", "))),
    );
    for (key, heading) in spec::OPTION_GROUPS {
        let tag = format!("shell.settings.group.{key}");
        children.push(tag.clone());
        let rows = match key {
            "keys" => settings_key_nodes(),
            "appearance" => settings_theme_nodes(state),
            // ★ R1762 — the value rows come first here for the reason they come
            // first on screen: a tree whose order is not the paint's order is a
            // reader being walked through a page that is not in front of them.
            "capture" => {
                let mut rows = settings_value_nodes(state);
                rows.extend(settings_option_nodes(state, key));
                rows
            }
            // ★ R1762 — and the decode group's payload-format row, whose seat
            // is what carries the formats: the chips themselves are its parts.
            "decode" => {
                let mut rows = settings_option_nodes(state, key);
                rows.push(
                    AccessNode::new(
                        format!("shell.settings.row.{}.chips", spec::PLUGIN_ROW.0),
                        AriaRole::Status,
                    )
                    .with_name(spec::PLUGIN_ROW.1)
                    .with_value(AccessValue::Text(spec::PLUGINS.join(", "))),
                );
                rows
            }
            group_key => settings_option_nodes(state, group_key),
        };
        let mut group = AccessNode::new(&tag, AriaRole::Group).with_name(heading);
        for row in &rows {
            group = group.with_child(row.tag.clone());
        }
        nodes.push(group);
        nodes.extend(rows);
    }
    (children, nodes)
}

/// ★★★★★ R1762 — the value rows: a collapsed chooser each, and the roster of
/// the one that is open.
///
/// `ComboBox` with `expanded`, which is the pair a reader needs: the role says
/// there is a roster behind it and the state says whether it is in front of
/// them. Measured at 6.11.1 on the floor's own collapsed control — its
/// accessible object reports the value and the item count and has **no
/// expanded state at all** unless the platform layer adds one, so a reader is
/// told what is chosen and never told whether the list is open.
fn settings_value_nodes(state: &Rc<ShellState>) -> Vec<AccessNode> {
    let picking = state.picking.borrow();
    // ★ R2021 — this page's own row, and not whatever roster happens to be
    // open. The signal is shared with the board's card settings now, and
    // announcing a card's options under a preferences tag would tell a reader
    // about a roster this page does not draw — the same guard the paint above
    // carries, for the same reason.
    let open = picking
        .as_ref()
        .filter(|(key, _)| matches!(Valued::from_key(key), Some(Valued::Preference(_))));
    let mut nodes = Vec::new();
    for row in spec::VALUE_ROWS {
        let showing = open.is_some_and(|(key, _)| key == row.key);
        let mut node = AccessNode::new(settings_choose_tag(row.key), AriaRole::ComboBox)
            .with_name(row.title)
            .with_value(AccessValue::Text(settings_value_of(state, row.key)))
            .with_expanded(showing);
        if showing {
            node = node.with_child(format!("shell.settings.roster.{}", row.key));
        }
        nodes.push(node);
    }
    // The open roster, and one node per word in it. Emitted only while it is
    // open, which is the same property the paint has: a reader offered options
    // that are not on screen is offered options nobody can reach.
    if let Some((key, picker)) = open {
        let roster_tag = format!("shell.settings.roster.{key}");
        let mut roster = AccessNode::new(&roster_tag, AriaRole::Listbox)
            .with_name(format!("{} options", settings_value_title(key)));
        let chosen = settings_value_of(state, key);
        let mut options = Vec::new();
        for (n, word) in picker.options().iter().enumerate() {
            let tag = format!("shell.settings.option.{key}.{word}");
            roster = roster.with_child(tag.clone());
            options.push(
                AccessNode::new(tag, AriaRole::ListBoxOption)
                    .with_name(word.as_ref())
                    .with_set_position(n, picker.len())
                    .with_selected(word.as_ref() == chosen),
            );
        }
        nodes.push(roster);
        nodes.extend(options);
    }
    nodes
}

/// The switch rows of one Settings group.
fn settings_option_nodes(state: &Rc<ShellState>, group: &str) -> Vec<AccessNode> {
    let on = state.options.get();
    spec::OPTIONS
        .iter()
        .enumerate()
        .filter(|(_, option)| option.group == group)
        .map(|(index, option)| {
            AccessNode::new(
                format!("shell.settings.option.{}", option.key),
                AriaRole::Switch,
            )
            .with_name(option.title)
            .with_value(AccessValue::Text(option.gist.to_owned()))
            .with_state(AccessState {
                checked: Some(on[index]),
                ..AccessState::default()
            })
        })
        .collect()
}

/// The two rows whose affordance is booked for a later release.
fn settings_key_nodes() -> Vec<AccessNode> {
    spec::KEY_ROWS
        .iter()
        .map(|row| {
            AccessNode::new(format!("shell.settings.key.{}", row.key), AriaRole::Button)
                .with_name(format!("{} \u{2014} {}", row.title, row.verb))
        })
        .collect()
}

/// The appearance segment: an exclusive choice, so a `radiogroup` of `radio`s.
///
/// ★ R1695 — not two buttons carrying a checked flag, which is what the first
/// draft emitted. A reader told "Dark, button, checked" learns that something is
/// on; told "Dark, radio button, selected, 1 of 2" they learn that choosing the
/// other one turns this off. WAI-ARIA binds `radio` to an owning `radiogroup`
/// and `pinion_a11y::structure` enforces exactly that, so the group is a node
/// rather than a convention.
fn settings_theme_nodes(state: &Rc<ShellState>) -> Vec<AccessNode> {
    let dark = theme_word(&state.theme) == "dark";
    let mut group =
        AccessNode::new("shell.settings.theme", AriaRole::RadioGroup).with_name(spec::THEME_ROW.0);
    let mut nodes = Vec::new();
    for (n, name) in spec::THEMES.iter().enumerate() {
        let tag = format!("shell.settings.theme.{n}");
        group = group.with_child(&tag);
        nodes.push(
            AccessNode::new(&tag, AriaRole::RadioButton)
                .with_name(*name)
                .with_state(AccessState {
                    checked: Some((n == 0) == dark),
                    ..AccessState::default()
                }),
        );
    }
    nodes.insert(0, group);
    nodes
}

/// One placed card: the region, its header controls, and its body.
fn card_nodes(state: &Rc<ShellState>, card: &Card) -> Vec<AccessNode> {
    let id = card.id().as_str();
    let where_it_is = if state.is_floating(id) {
        "detached; "
    } else {
        ""
    };
    let announce = match card.remedy() {
        None => format!("{where_it_is}{}", state_sentence(card.state())),
        Some(remedy) => format!(
            "{where_it_is}{}; {}",
            state_sentence(card.state()),
            remedy_label(remedy)
        ),
    };
    // ★★★★★ R2022 — how the header actually came out, so what is announced is
    // what was drawn. `None` is a detached card, whose header is its float's;
    // there the offered set is what the tree states, as it did for every card
    // before this round.
    let header = card_header_layout(state, card);
    let mut region = AccessNode::new(format!("card.{id}"), AriaRole::Group)
        .with_name(card.title())
        .with_value(AccessValue::Text(announce))
        .with_state(AccessState::default());
    let mut nodes = Vec::new();
    // The grip gives way LAST rather than never (`HeaderLayout::grip`), so a
    // card dragged to nothing has none and a reader is not offered one.
    if header.as_ref().is_none_or(|h| h.grip().is_some()) {
        region = region.with_child(format!("card.{id}.grip"));
        nodes.push(
            AccessNode::new(format!("card.{id}.grip"), AriaRole::Button)
                .with_name(format!("Move {}", card.title())),
        );
    }
    // ★★★★★ R1900 — a shared place's strip, as a reader meets it: a `TabList`
    // whose tabs say which one is selected.
    //
    // On the card in FRONT, because that is the one whose header carries the
    // strip — the same rule the paint and the hit test follow, so a reader is
    // walked through the header that is actually there. A card behind a tab
    // keeps its own region, as a detached one does: it exists, and where it is
    // is what changed.
    if let Some(tile) = state.board.get().tile(card.id()) {
        if tile.is_shared() && &tile.id == card.id() {
            let list = format!("card.{id}.tabs");
            region = region.with_child(list.clone());
            let mut strip = AccessNode::new(list, AriaRole::TabList).with_name(format!(
                "{} shares a place with {} other card(s)",
                card.title(),
                tile.members().len() - 1
            ));
            for member in tile.members() {
                let tag = format!("card.{member}.tab");
                strip = strip.with_child(tag.clone());
                nodes.push(
                    AccessNode::new(tag, AriaRole::Tab)
                        .with_name(label_of(member.as_str()))
                        .with_selected(member == &tile.id),
                );
            }
            nodes.push(strip);
        }
    }
    for control in card_chrome_nodes(state, card, header.as_ref()) {
        region = region.with_child(control.tag.clone());
        nodes.push(control);
    }
    // ★★★★★ R2022 — the body's rows come from the rectangle the PAINTER draws
    // them in. A body that cannot be painted at all — a card the board does not
    // hold, or one whose state puts a sentence there instead — has no rows to
    // announce, which is the same refusal `body_scene` makes.
    let body = card_body_rect(state, card)
        .map_or_else(Vec::new, |rect| card_body_nodes(state, card, rect));
    for node in &body {
        // The body's own containers are the card's children; everything under
        // them is reached through them.
        if BODY_ROOTS
            .iter()
            .any(|suffix| node.tag == format!("card.{id}.{suffix}"))
        {
            region = region.with_child(node.tag.clone());
        }
    }
    nodes.extend(body);
    // ★★★★★ R2021 — the settings panel, and ONLY while it is painted.
    //
    // `card_config_panel` is the same derivation the painter and the hit test
    // ask, so a reader is offered the rows that are on the screen and no
    // others: announcing a panel a narrow card has no room for is the ghost
    // class this board already carries a ratchet for
    // ([[debt-a-card-announces-a-row-it-does-not-paint]]), and the cheapest way
    // not to join it is to ask the painter's own question.
    if card_config_panel(state).is_some_and(|(open, _, _)| open.id() == card.id()) {
        let panel_tag = format!("card.{id}.config");
        region = region.with_child(panel_tag.clone());
        let mut group = AccessNode::new(&panel_tag, AriaRole::Group)
            .with_name(format!("{} settings", card.title()));
        for setting in spec::card_settings_of(kind_of(id)) {
            let valued = Valued::Card {
                card: id.to_owned(),
                setting,
            };
            group = group.with_child(valued.control_tag());
            nodes.extend(valued_nodes(state, &valued));
        }
        nodes.push(group);
    }
    nodes.insert(0, region);
    nodes
}

/// ★★★★★ R2021 — a value row as a reader meets it: a `ComboBox` saying what it
/// holds and whether its roster is in front of them, plus that roster's own
/// words while it is.
///
/// Lifted out of the preferences page's [`settings_value_nodes`] rather than
/// written a second time — a second copy is what this round is repaying one
/// layer down, and the two would have differed at once: this row's tags are
/// scoped by the card showing it and that page's are not.
fn valued_nodes(state: &Rc<ShellState>, valued: &Valued) -> Vec<AccessNode> {
    let picking = state.picking.borrow();
    let open = picking
        .as_ref()
        .filter(|(key, _)| *key == valued.key())
        .map(|(_, picker)| picker);
    let chosen = valued.read(state);
    let mut node = AccessNode::new(valued.control_tag(), AriaRole::ComboBox)
        .with_name(valued.title())
        .with_value(AccessValue::Text(chosen.clone()))
        .with_expanded(open.is_some());
    let Some(picker) = open else {
        return vec![node];
    };
    node = node.with_child(valued.roster_tag());
    let mut roster = AccessNode::new(valued.roster_tag(), AriaRole::Listbox)
        .with_name(format!("{} options", valued.title()));
    let mut options = Vec::new();
    for (n, word) in picker.options().iter().enumerate() {
        let tag = valued.option_tag(word.as_ref());
        roster = roster.with_child(tag.clone());
        options.push(
            AccessNode::new(tag, AriaRole::ListBoxOption)
                .with_name(word.as_ref())
                .with_set_position(n, picker.len())
                .with_selected(word.as_ref() == chosen),
        );
    }
    let mut out = vec![node, roster];
    out.extend(options);
    out
}

/// The tag suffixes a card body's own top-level containers use — the nodes that
/// become the card region's children.
const BODY_ROOTS: &[&str] = &[
    "grid",
    "tree",
    "bytegrid",
    "query",
    "chips",
    "counts",
    "sparkline",
    // ★ R1797 — the latency card's two: the tile strip and the distribution.
    "tiles",
    "bins",
];

/// ★★★★★ R2022 — the header affordances a card offers a reader: the ones its
/// strip had ROOM for, in the order it gives way (from the left, so what
/// survives is a suffix).
///
/// The population is the card's own chrome rather than [`spec::CARD_CHROME`],
/// because that is what the painter asked — a card that stopped offering one
/// would otherwise still be announced as having it, and the two vocabularies are
/// held equal by `r1668_every_named_header_control_is_one_the_shell_has`.
///
/// `None` for `header` is a DETACHED card, whose strip is its float's; there the
/// offered set is what the tree states, as it did for every card before R2022.
fn card_chrome_nodes(
    state: &ShellState,
    card: &Card,
    header: Option<&card_header::HeaderLayout>,
) -> Vec<AccessNode> {
    let id = card.id().as_str();
    let offered = card.chrome().offered();
    let surviving: Vec<&'static str> = match header {
        Some(layout) => layout
            .slots()
            .iter()
            .filter_map(|(n, _)| offered.get(*n).map(|a| a.wire()))
            .collect(),
        None => offered.iter().map(|a| a.wire()).collect(),
    };
    surviving
        .into_iter()
        .map(|control| {
            AccessNode::new(format!("card.{id}.{control}"), AriaRole::Button).with_name(
                match control {
                    "settings" => "Configure".to_owned(),
                    "tear_off" => {
                        if state.is_floating(id) {
                            "Redock".to_owned()
                        } else {
                            "Detach".to_owned()
                        }
                    }
                    "maximize" => "Maximize".to_owned(),
                    _ => format!("Remove {}", card.title()),
                },
            )
        })
        .collect()
}

/// ★★★★★ R2022 — **what a card's body says, given the rectangle it is drawn
/// in** — the describing twin of [`ready_body`], which dispatches the same kinds
/// on the same rectangle.
///
/// It is a function rather than an arm inside [`card_nodes`] so that a test can
/// hold the pair to each other AT A SIZE OF ITS OWN CHOOSING, and that is a
/// repair rather than a tidy-up. Two counterfactuals PASSED at R2022 — the byte
/// pane's line clamp and the latency caption's — because the sweep narrows and
/// shortens every card in one step, so no swept case is ever WIDE AND SHORT, and
/// a body whose width refusal fires first can hide a height rule behind it. The
/// two faults always travelled together (R1845's class), and the repair belongs
/// to the population rather than to an assertion:
/// `r2022_a_body_announces_only_what_it_paints_at_every_size` varies the two
/// independently, which nothing that goes through the board can do.
fn card_body_nodes(state: &ShellState, card: &Card, body: Rect) -> Vec<AccessNode> {
    let id = card.id().as_str();
    match def_for_card(id).map(|def| def.kind) {
        Some("packet") => stream_nodes(id, body),
        Some("decode") => decode_nodes(id, body),
        Some("keymap") => map_nodes(id, body),
        Some("filter") => filter_nodes(state, id, body),
        Some("latency") => latency_nodes(id, body),
        Some("health") => health_nodes(card, body),
        Some("alarms") => alarms_nodes(state, card, body),
        _ => Vec::new(),
    }
}

/// The message stream, as a **grid**: a header row of column headers, then one
/// row per message holding one cell per column.
///
/// This is the shape a model-driven item view builds for itself at the floor —
/// measured at 6.11.1, its cell query answers the cell's name, its row, its
/// column and its column header — and the shape a hand-painted table has to
/// build or it has none at all.
///
/// ★★★★★ R2022 — the rows and the columns are the ones the PAINTER has room
/// for, asked of [`stream_seats`] and [`stream_columns`] rather than counted off
/// the specification. The extent still states what the model holds, so a reader
/// is told *five rows of eight* rather than either lie.
fn stream_nodes(id: &str, body: Rect) -> Vec<AccessNode> {
    let columns = stream_columns(body.w).len();
    let rows: Vec<Vec<String>> = stream_seats(body)
        .iter()
        .map(|(n, _)| {
            let (time, kind, name, len) = spec::STREAM_ROWS[n];
            [time, kind, name, len]
                .into_iter()
                .take(columns)
                .map(str::to_owned)
                .collect()
        })
        .collect();
    table_nodes(
        id,
        "Message stream",
        columns,
        &rows,
        GridExtent {
            rows: spec::STREAM_ROWS.len(),
            columns: spec::STREAM_COLUMNS.len(),
        },
    )
}

/// The identifier map, as a grid on the same shape as the stream.
///
/// ★ The unresolved row's timestamp is painted as an em dash, which is the
/// typographic stand-in for a value that is not knowable — and to somebody
/// reading rather than looking it is a punctuation mark. The cell announces the
/// meaning instead, which the voice census is what asked for: a name with no
/// word in it is a hole.
///
/// ★★★★★ R2022 — the rows and columns the painter has room for, for
/// [`stream_nodes`]'s reason and by the same means.
fn map_nodes(id: &str, body: Rect) -> Vec<AccessNode> {
    let columns = map_columns_shown(body.w);
    let rows: Vec<Vec<String>> = map_seats(body)
        .iter()
        .map(|(n, _)| {
            let (key, path, seen) = spec::MAP_ROWS[n];
            let when = if seen.chars().all(|c| !c.is_alphanumeric()) {
                "not known"
            } else {
                seen
            };
            [key, path, when]
                .into_iter()
                .take(columns)
                .map(str::to_owned)
                .collect()
        })
        .collect();
    table_nodes(
        id,
        "Identifier map",
        columns,
        &rows,
        GridExtent {
            rows: spec::MAP_ROWS.len(),
            columns: spec::MAP_COLUMNS.len(),
        },
    )
}

/// A card body that is a table.
///
/// ★★★★★ Built by [`grid_table_nodes_clamped`] rather than by hand. The first draft of
/// this screen hand-rolled the shape — as the sibling capture screen already
/// did — and the two disagreed about where the header row sits: WAI-ARIA counts
/// it in `aria-rowcount`, so it has to be counted in `aria-rowindex` too, and a
/// tree that counts it in one and not the other leaves its header unplaced and
/// its last row unreachable. The rule now lives once, in the builder, where a
/// third table cannot re-derive it differently.
///
/// The column headers are deliberately left unnamed here: they are painted with
/// their own tags, so the name comes from the paint and the two cannot drift.
///
/// ★★★★★ R2022 — `columns` and `rows` are what the card has ROOM for and
/// `extent` is what its model holds. They were the same thing until this round,
/// and that is why both table cards announced rows and columns nobody drew.
fn table_nodes(
    id: &str,
    name: &str,
    columns: usize,
    rows: &[Vec<String>],
    extent: GridExtent,
) -> Vec<AccessNode> {
    let grid_columns: Vec<GridColumn> = (0..columns)
        .map(|c| GridColumn {
            tag: head_cell_tag(id, c),
            sort: None,
        })
        .collect();
    let grid_rows: Vec<GridRow> = rows
        .iter()
        .enumerate()
        .map(|(r, values)| GridRow {
            tag: format!("card.{id}.{}.{r}", row_suffix(id)),
            selected: false,
            state: RadioState::Idle,
            cells: values
                .iter()
                .enumerate()
                .map(|(c, value)| GridCell {
                    tag: cell_tag(id, r, c),
                    name: value.clone(),
                    focused: false,
                    selected: None,
                })
                .collect(),
        })
        .collect();
    grid_table_nodes_clamped(
        &format!("card.{id}.grid"),
        name,
        false,
        &format!("card.{id}.head"),
        &grid_columns,
        &grid_rows,
        extent,
    )
}

/// The tag segment a table card's data rows are painted under.
fn row_suffix(id: &str) -> &'static str {
    if def_for_card(id).map(|def| def.kind) == Some("keymap") {
        "map"
    } else {
        "row"
    }
}

/// The decode inspector: the layer tree, and the bytes it was decoded from.
///
/// ★ The tree is where this beats the floor rather than matching it. Built and
/// run at 6.11.1, a two-column tree announces a row as **two sibling items** —
/// the field and its value are peers, the value reports that it can expand, and
/// the hierarchy is gone: every item is a direct child whatever its depth. Here
/// a field is one item, its value is its value, and the level carries the depth
/// the paint indents by.
///
/// ★★★★★ R2022 — the rows the card has ROOM for, from [`decode_seats`]. The
/// tree's own `aria-setsize` stays the whole layer set, and each item keeps its
/// place among its real siblings: a reader is told *this card shows six of
/// eight*, which is a fact, rather than being walked through two rows nobody
/// drew.
fn decode_nodes(id: &str, body: Rect) -> Vec<AccessNode> {
    let mut tree = AccessNode::new(format!("card.{id}.tree"), AriaRole::Tree)
        .with_name("Decoded layers")
        .with_size_of_set(u32::try_from(spec::DECODE_ROWS.len()).unwrap_or(u32::MAX));
    let mut nodes = Vec::new();
    for (n, _) in decode_seats(body).iter() {
        let (depth, key, value) = spec::DECODE_ROWS[n];
        let tag = format!("card.{id}.tree.{n}");
        tree = tree.with_child(tag.clone());
        let (place, siblings) = sibling_place(n);
        let mut item = AccessNode::new(tag, AriaRole::TreeItem)
            .with_name(key)
            .with_level(depth + 1)
            .with_set_position(place, siblings)
            .with_selected(n == spec::DECODE_SELECTED);
        if !value.is_empty() {
            item = item.with_value(AccessValue::Text(value.to_owned()));
        }
        // A layer heading is what folds; a field under it does not.
        if depth == 0 {
            item = item.with_expanded(true);
        }
        nodes.push(item);
    }
    nodes.insert(0, tree);
    nodes.extend(byte_nodes(id, body));
    nodes
}

/// Where one decode row sits **among its own siblings** — the pair a flat index
/// cannot give, and the one a reader is told.
fn sibling_place(n: usize) -> (usize, usize) {
    let depth = spec::DECODE_ROWS[n].0;
    let siblings: Vec<usize> = spec::DECODE_ROWS
        .iter()
        .enumerate()
        .filter(|(m, row)| row.0 == depth && same_parent(*m, n))
        .map(|(m, _)| m)
        .collect();
    let place = siblings.iter().position(|m| *m == n).unwrap_or(0);
    (place, siblings.len())
}

/// Whether two decode rows of the same depth hang under the same heading.
fn same_parent(a: usize, b: usize) -> bool {
    let parent = |n: usize| {
        let depth = spec::DECODE_ROWS[n].0;
        (0..n).rev().find(|m| spec::DECODE_ROWS[*m].0 < depth)
    };
    parent(a) == parent(b)
}

/// The captured frame as a grid: one row per painted line, one cell per byte,
/// and the bytes the selected field was read from announced as selected.
fn byte_nodes(id: &str, body: Rect) -> Vec<AccessNode> {
    let per_line = 4;
    let lines = spec::DECODE_BYTES.len();
    // ★★★★★ R2022 — the pane's own refusal, first: below its floor the painter
    // drops the pane entirely rather than draw it outside the card, so there is
    // no grid to be walked and announcing one is a region a reader is sent to
    // and finds nothing in.
    let Some(pane_w) = byte_pane_w(body.w) else {
        return Vec::new();
    };
    let columns = byte_columns(pane_w);
    let mut grid = AccessNode::new(format!("card.{id}.bytegrid"), AriaRole::Grid)
        .with_name("Captured bytes")
        .with_row_count(u32::try_from(lines).unwrap_or(u32::MAX))
        .with_column_count(u32::try_from(per_line).unwrap_or(u32::MAX));
    let mut nodes = Vec::new();
    let (from, to) = spec::DECODE_SELECTED_SPAN;
    for (line, _) in byte_line_seats(body).iter() {
        let bytes = &spec::DECODE_BYTES[line];
        let row_tag = format!("card.{id}.bytes.{line}");
        grid = grid.with_child(row_tag.clone());
        // The row is named by the offset it starts at, which is what the strip
        // paints in its left column and what a reader counts from.
        let mut row = AccessNode::new(row_tag, AriaRole::Row)
            .with_name(format!("{:04x}", line * per_line))
            .with_row(line);
        for (column, byte) in bytes.iter().enumerate().take(columns) {
            let index = line * per_line + column;
            let tag = format!("card.{id}.byte.{index}");
            row = row.with_child(tag.clone());
            nodes.push(
                AccessNode::new(tag, AriaRole::GridCell)
                    .with_name(format!("{byte:02x}"))
                    .with_row(line)
                    .with_column(column)
                    .with_selected(index >= from && index < to),
            );
        }
        nodes.push(row);
    }
    nodes.insert(0, grid);
    nodes
}

/// The search and filter card: the query, the saved chips, and the three counts
/// whose **relation** is the point of the card.
fn filter_nodes(state: &ShellState, id: &str, body: Rect) -> Vec<AccessNode> {
    let mut nodes = vec![
        AccessNode::new(format!("card.{id}.query"), AriaRole::TextInput)
            .with_name("Query")
            .with_value(AccessValue::Text(spec::FILTER_QUERY.to_owned())),
    ];
    // ★★★★★ R1721 — the bar's whole subtree is its rule's. It used to be five
    // `button`s with `aria-pressed`, hand-written here, over a set that can never
    // have two on — and nothing at all could change one of them. `spec::FILTER_ROW`
    // is the declaration; this call is the only thing that reads it into a tree.
    // ★★★★★ R2022 — the chips the card has ROOM for. `filter_chip_rects` is the
    // geometry the paint and the hit test already shared; the tree is its third
    // reader, so a chip that wrapped off the bottom of a shrunken card stops
    // being a toggle a reader is offered and cannot reach.
    let chips = pinion_a11y::chip_group_nodes(
        &filter_row_shown(state, id, body),
        focus_state::focused().as_deref(),
    );
    let (with_counts, with_trend) = filter_counts_shown(body);
    let mut counts =
        AccessNode::new(format!("card.{id}.counts"), AriaRole::Group).with_name("Match counts");
    if with_counts {
        for (n, (value, what)) in spec::FILTER_STATS.iter().enumerate() {
            let tag = format!("card.{id}.stat.{n}");
            counts = counts.with_child(tag.clone());
            // The word is the name and the number is the value: a reader told
            // only "12,418" has been told which of three numbers it is by
            // position, and position is exactly what somebody not looking at the
            // card does not have.
            nodes.push(
                AccessNode::new(tag, AriaRole::Status)
                    .with_name(*what)
                    .with_value(AccessValue::Text((*value).to_owned())),
            );
        }
    }
    if with_trend {
        nodes.push(
            AccessNode::new(format!("card.{id}.sparkline"), AriaRole::Group)
                .with_name("Matched over time")
                .with_value(AccessValue::Text(series_reading(&MATCH_SERIES))),
        );
    }
    nodes.extend(chips);
    nodes.push(counts);
    nodes
}

/// ★★★★★ R1797 — what the latency card says to somebody who cannot see it.
///
/// A chart is the case where "announce the picture" is not enough: the shape IS
/// the content, so the reading has to carry the distribution. This one says the
/// three landmarks, then every bucket with its count, then which buckets are
/// the tail and WHY — the same derivation the wire publishes and the paint
/// draws, from the same call, so the three cannot disagree.
///
/// ★ Written because twenty-seven demos failed without it, all with one
/// message: every mark the chart painted was `dangling`, its silence naming a
/// container that no accessibility node answered for. A `Silence::part_of(X)`
/// is a promise that X speaks for these marks. Tagging X is not that promise —
/// the sparkline next door has had both since R1648 and this card had only the
/// tag, so the chart was silent AND claiming to be covered.
///
/// ★★★★★ R2022 — the tile strip, the plot and the caption are announced only
/// where the painter has room to draw them, from the painter's own derivations
/// ([`stat_strip_w`], [`distribution_box`], [`latency_caption_shown`]). On a
/// one-cell card the strip is dropped whole, and three tiles were announced into
/// a card that had drawn none of them.
fn latency_nodes(id: &str, body: Rect) -> Vec<AccessNode> {
    let Some((binned, quantiles)) = latency_binned() else {
        return Vec::new();
    };
    let mut nodes = Vec::new();
    let tiles = latency_stats(&quantiles);
    let strip = format!("card.{id}.tiles");
    let mut group = AccessNode::new(strip.clone(), AriaRole::Group).with_name("Round trip");
    if stat_strip_w(body.w, tiles.len()).is_some() {
        for (n, (key, value)) in tiles.iter().enumerate() {
            let tag = format!("card.{id}.stat.{n}");
            group = group.with_child(tag.clone());
            nodes.push(
                AccessNode::new(tag, AriaRole::Status)
                    .with_name(*key)
                    .with_value(AccessValue::Text(value.clone())),
            );
        }
    }
    nodes.insert(0, group);
    let cut = quantiles.at(0.95).unwrap_or(f64::INFINITY);
    let tail = binned.tail_from(cut);
    let buckets = (0..binned.bins())
        .map(|k| format!("{} {}", binned.label(k), binned.counts()[k]))
        .collect::<Vec<_>>()
        .join(", ");
    let emphasised = if tail.is_empty() {
        "no bucket is entirely above it".to_owned()
    } else {
        format!(
            "{} above it",
            tail.clone()
                .map(|k| binned.label(k))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    if latency_plot_rect(body, tiles.len(), binned.bins()).is_some() {
        nodes.push(
            AccessNode::new(format!("card.{id}.bins"), AriaRole::Group)
                .with_name("Round trip distribution")
                .with_value(AccessValue::Text(format!(
                    "{} samples in {} buckets, {} — {}: {buckets}; {emphasised}",
                    binned.basis().n,
                    binned.bins(),
                    spec::LATENCY_UNIT,
                    binned.rule().name(),
                ))),
        );
    }
    if latency_caption_shown(body, tiles.len()) {
        nodes.push(
            AccessNode::new(format!("card.{id}.caption"), AriaRole::Status)
                .with_name("About this chart")
                .with_value(AccessValue::Text(spec::LATENCY_CAPTION.to_owned())),
        );
    }
    nodes
}

/// What a sparkline says to somebody who cannot see it: how many samples, the
/// range they cover, and where it ended.
fn series_reading(series: &[f64]) -> String {
    let low = series.iter().copied().fold(f64::INFINITY, f64::min);
    let high = series.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let last = series.last().copied().unwrap_or(0.0);
    format!(
        "{} samples, {low:.0} to {high:.0}, latest {last:.0}",
        series.len()
    )
}

/// The palette: three sections of catalogue entries, **nine of them locked**.
///
/// Every entry is a `listitem` whatever its tier, and its place in the set is
/// counted over the whole catalogue — because a locked seat is a seat. Dropping
/// the locked ones would make the palette announce four entries while the panel
/// shows thirteen and its own footer says nine are reserved.
///
/// The reason a locked entry is locked is **not restated here**: the row
/// declares it once on its layout style, and the accessibility assembler relays
/// what the disabled cascade resolved. One declaration, and the wire, the
/// accessibility tree and the ink cannot disagree.
fn palette_nodes(state: &Rc<ShellState>) -> Vec<AccessNode> {
    // ★★★★★ R1903 — a folded palette announces its STRIP and nothing else, for
    // the same reason it paints nothing else: a reader told about thirteen
    // catalogue rows that are not on screen is a reader sent looking for them.
    // The strip carries the panel's name plus what pressing it does, so the way
    // back is the thing that is announced.
    if palette_placement().folded {
        return vec![
            AccessNode::new("shell.palette.strip", AriaRole::Button)
                .with_name(format!("{}, put away", spec::PALETTE_TITLE))
                .with_value(AccessValue::Text("open the palette".to_owned())),
        ];
    }
    let mut list = AccessNode::new("shell.palette", AriaRole::List)
        .with_name(spec::PALETTE_TITLE)
        // ★ R1761 — and the line the panel paints under that name, which is
        // how a widget gets onto the board. It was on screen and in no
        // announcement; the painted mark now declares itself part of this one
        // rather than speaking for itself.
        .with_value(AccessValue::Text(spec::PALETTE_HINT.to_owned()))
        .with_size_of_set(u32::try_from(spec::CATALOGUE.len()).unwrap_or(u32::MAX));
    let mut nodes = Vec::new();
    for (key, title) in spec::SECTIONS {
        let section_tag = format!("shell.palette.section.{key}");
        list = list.with_child(section_tag.clone());
        let mut section = AccessNode::new(section_tag, AriaRole::Group).with_name(*title);
        for (n, def) in spec::CATALOGUE.iter().enumerate() {
            if def.section != *key {
                continue;
            }
            let tag = format!("shell.palette.{}", def.kind);
            section = section.with_child(tag.clone());
            nodes.push(
                AccessNode::new(tag, AriaRole::ListItem)
                    .with_name(def.label)
                    .with_value(AccessValue::Text(def.gist.to_owned()))
                    .with_set_position(n, spec::CATALOGUE.len()),
            );
        }
        nodes.push(section);
    }
    list = with_cursor_declared(
        list.with_child("shell.palette.placed")
            .with_child("shell.palette.reserved"),
        state,
    );
    nodes.push(
        AccessNode::new("shell.palette.placed", AriaRole::Status)
            .with_name("Placed")
            .with_value(AccessValue::Text(format!(
                "{} of {}",
                state.placed().len(),
                spec::placeable_count()
            ))),
    );
    nodes.push(
        AccessNode::new("shell.palette.reserved", AriaRole::Status)
            .with_name("Reserved")
            .with_value(AccessValue::Text(spec::reserved_count().to_string())),
    );
    // ★★★★★ R1903 — the fold control is a keyboard stop, so it must be a node
    // the tree can name: a reader who lands on something the tree cannot name
    // has landed nowhere, which is what the ring gate says in those words.
    //
    // A sibling of the list rather than a child of it, because the catalogue's
    // roving cursor enumerates the thirteen entries and this is not one of them.
    nodes.push(
        AccessNode::new(format!("{PALETTE_HEAD}fold"), AriaRole::Button)
            .with_name(format!("Put {} away", spec::PALETTE_TITLE)),
    );
    nodes.insert(0, list);
    nodes
}

impl WidgetView for AnalyzerShellView {
    type Renderer = HelloAnalyzerShellRenderer;

    /// ★ R1712 — [`SHRINK`] is the whole declaration, and it is rigid: this
    /// window stops exactly where its layout does. Behaviour is unchanged (a
    /// floor pinned at the open size is what `SizeStrategy::Fixed` meant); what
    /// changed is that the floor is now a *decision on the wire* rather than a
    /// default nobody examined, and `scene/size_floor` can tell those apart.
    ///
    /// Measured, this screen could take 49 pixels narrower and 29 shorter with
    /// everything still reachable. Left unmade for the reason its sibling
    /// records: the band clips the rail, the canvas and the palette together,
    /// so its honest declaration is most of the screen.
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::shrinking(SHRINK, (WIN_W, WIN_H))
    }

    fn shrink_policy() -> Option<ShrinkPolicy> {
        Some(SHRINK)
    }

    /// ★★★★★ R1826 — **the tear-off opens a real window.**
    ///
    /// Measured before this existed, by driving the running application:
    /// `scene/windows` declared exactly one window, `main`, at boot AND after
    /// `act packet#0,tear_off`. The card left the board and appeared as
    /// `float.packet#0` — a panel painted inside the canvas — while the screen
    /// said "packet#0 -> detached window" in a sentence a reader could see and
    /// no window existed to match.
    ///
    /// The specification this shell reproduces asks for both halves — *widget =
    /// independent card (… tear off …) · multi-window (tear off -> independent
    /// window, always-on-top option)* — and the assembly had the first.
    fn windows_signal() -> Option<Rc<Signal<Vec<WindowSpec>>>> {
        Some(use_shell_windows())
    }

    /// ★★★★★ R1826 — what each window paints.
    ///
    /// The main window paints the application; a `torn-<card>` window paints
    /// THAT card and nothing else. The card id is recovered from the window id
    /// through the same prefix [`float_window_id`] built it with, so a window
    /// that exists and a window that is painted cannot disagree about which
    /// card they are about.
    ///
    /// A window whose card is no longer detached — the frame between a redock
    /// and the topology catching up — still goes to [`torn_window_scene`],
    /// which paints its ground and nothing else. 🟥 This paragraph said it
    /// painted the APPLICATION, which the arm below has never done and which
    /// contradicted `torn_window_scene`'s own header three thousand lines away;
    /// the closing audit caught the pair. Two docstrings describing one branch
    /// is how they come to disagree, so the answer lives at the function that
    /// decides it and this one points there.
    fn view_for_window(
        window_id: &str,
        state: <Self as pinion_core::WidgetCore>::State,
        frame: &Frame,
    ) -> Scene {
        match window_id.strip_prefix(FLOAT_WINDOW_PREFIX) {
            Some(card) => torn_window_scene(card),
            None => Self::view(state, frame),
        }
    }

    /// ★★★ R1724 — a text box inside the showing screen is that screen's, and
    /// so is the press that puts a caret in it.
    ///
    /// This shell has no text box of its own that takes a caret from a
    /// pointer, so both hooks are pure delegation — which is the point: a
    /// mounted screen keeps every hook it overrode, and the two the node lab
    /// overrode are these.
    fn position_caret_for_point(
        _state: &ScreenState,
        scene: &Scene,
        focused: Option<&str>,
        hit_tag: Option<&str>,
        x: f32,
        y: f32,
        extend: bool,
    ) -> Option<usize> {
        let state = use_shell_state();
        state
            .screens
            .with_current(&state.journey.get(), |screen| {
                screen.position_caret_for_point(scene, focused, hit_tag, x, y, extend)
            })
            .flatten()
    }

    fn select_drag_to_point(
        _state: &ScreenState,
        scene: &Scene,
        focused: Option<&str>,
        anchor: usize,
        x: f32,
        y: f32,
    ) -> bool {
        let state = use_shell_state();
        state
            .screens
            .with_current(&state.journey.get(), |screen| {
                screen.select_drag_to_point(scene, focused, anchor, x, y)
            })
            .unwrap_or(false)
    }
}

fn main() {
    pinion_shell::run::<AnalyzerShellView>();
}

#[cfg(test)]
mod painted;
#[cfg(test)]
mod tests;
