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
    GridRow, HasPopup, NavLink, WidgetA11y, grid_table_nodes, navigation_link_nodes,
    page_region_node,
};
use pinion_chart::{
    Bar, BarChart, BinEnds, Binned, ChartStyle, Mute, QuantileMethod, Quantiles, Sparkline,
};
use pinion_core::availability::Unavailable;
use pinion_core::drop_target::{
    BOARD_WIDGET_DRAG_KIND, DropAccept, DropAction, DropActions, DropClause, DropContract,
    DropOffer, DropStanding, DropVerdict, standing_value,
};
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
use pinion_core::style::{
    Border, BoxStyle, Chrome, ChromeEdge, ChromeRole, Color, LayoutStyle, PathStyle, Size, Stroke,
    TextOverflow, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, ThemeMode, ThemeProvider, use_theme};
use pinion_core::utterance::{Announced, Tone, Utterance};
use pinion_core::voice::Silence;
use pinion_core::widgets::button::ButtonState;
use pinion_core::widgets::card::{Card, CardAffordance, CardChrome, CardState, Remedy};
use pinion_core::widgets::chip_group::{Chip, ChipGroup};
use pinion_core::widgets::destination::{Destinations, Detour, Journey};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::roving::{Activation, Axis, Ends, Landing, Member, Roving, RovingSpec};
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::tile_grid::{
    Carried, Dropped, Maximized, Tile, TileDirection, TileDrag, TileGrid, TileId, TileNudge,
};
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::transport::{TransportClock, TransportStatus, use_transport_clock};
use pinion_core::window_level::WindowLevel;
use pinion_core::{Frame, Scene, WidgetCore};
// ★★★★★ R1724 — the axis that makes this file an application rather than a
// screen: a destination's page can be another binding, mounted whole.
use pinion_core::chrome::{HostChrome, Part as ChromePart};
use pinion_core::widgets::picker::Picker;
use pinion_screen::{Mount, Screen, ScreenRoster, ScreenState};
use pinion_shell::{SizeStrategy, WidgetView, WindowSpec, vello_renderer_impl};
use pinion_widget_paint::button::{self, ButtonColors, ButtonStyle};
use pinion_widget_paint::card_header;
use pinion_widget_paint::chooser;
use pinion_widget_paint::pages::{PagePointer, view_page_region};
use pinion_widget_paint::pane::{PanePointer, scroll_pane};
use pinion_widget_paint::run::text_run;
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
/// (R1668) A decode row's value column, and the narrowest its key can be. The
/// key is allocated first: a row that lost its name reads as a value belonging
/// to nothing.
const VALUE_W: u32 = 74;
const KEY_FLOOR: u32 = 30;
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

const FONT_TITLE: u32 = 13;
const FONT_BODY: u32 = 12;
const FONT_SMALL: u32 = 11;
const FONT_TINY: u32 = 10;

/// The canvas rectangle: everything between the rail and the palette, under
/// both bars.
fn canvas_rect() -> Rect {
    Rect::new(
        RAIL_W,
        APP_BAR_H + SUB_BAR_H,
        win_w() - RAIL_W - PALETTE_W,
        win_h() - APP_BAR_H - SUB_BAR_H,
    )
}

/// ★★ R1695 — the rectangle the **paged region** occupies at a destination.
///
/// At the dashboard it is exactly [`canvas_rect`], because the dashboard also
/// paints a layout bar above it and a palette beside it; anywhere else those
/// are not there and the page gets the whole area the rail and application bar
/// leave. A destination-dependent rectangle is what a region is: the page is
/// what the window gives that destination, not a fixed hole in the chrome.
fn page_rect(at: &str) -> Rect {
    if at == "dashboard" {
        return canvas_rect();
    }
    Rect::new(RAIL_W, APP_BAR_H, win_w() - RAIL_W, win_h() - APP_BAR_H)
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

/// The two view tabs the application bar carries.
const TABS: [&str; 2] = ["Dashboard", "Design System"];

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

/// The reference tool's own dark and light token sets, mapped onto this
/// framework's roles.
///
/// A tool of this class is looked at for hours in a dim room, so its palette is
/// a decision rather than a default, and matching it is most of what makes two
/// screens look like the same product. The mapping is one-way and explicit:
/// the canvas is the darkest ground, panels sit on it, the raised chrome is one
/// step lighter again, and one accent serves every affirmative control.
fn reference_palettes() -> (Theme, Theme) {
    let dark = Theme {
        surface: rgb(0x0E_0F_12),
        on_surface: rgb(0xE8_EB_EF),
        on_surface_muted: rgb(0x98_A2_AD),
        accent: rgb(0x9A_00_4F),
        on_accent: rgb(0xFF_FF_FF),
        outline: rgb(0x2A_2E_36),
        surface_container_low: rgb(0x16_18_1D),
        surface_container: rgb(0x1E_21_27),
        surface_container_high: rgb(0x25_2A_33),
        surface_container_highest: rgb(0x3A_40_4B),
        error: rgb(0xF0_70_5E),
        inverse_primary: rgb(0xEC_5A_A0),
        ..Theme::dark()
    };
    let light = Theme {
        surface: rgb(0xF6_F7_F9),
        on_surface: rgb(0x14_17_1C),
        on_surface_muted: rgb(0x5B_65_70),
        accent: rgb(0x9A_00_4F),
        on_accent: rgb(0xFF_FF_FF),
        outline: rgb(0xE1_E5_EA),
        surface_container_low: rgb(0xFF_FF_FF),
        surface_container: rgb(0xEE_F0_F3),
        surface_container_high: rgb(0xE7_EA_EF),
        surface_container_highest: rgb(0xCD_D3_DB),
        error: rgb(0xD3_3A_2C),
        inverse_primary: rgb(0x9A_00_4F),
        ..Theme::light()
    };
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
#[derive(Debug, Clone)]
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
    presets: RefCell<BTreeMap<String, Preset>>,
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
#[must_use]
fn screen_roster() -> ScreenRoster {
    ScreenRoster::new(
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
    )
}

impl ShellState {
    fn new(
        clock: Rc<TransportClock>,
        theme: Rc<ThemeProvider>,
        toast: Rc<pinion_core::utterance::Saying>,
    ) -> Self {
        let (light, dark) = reference_palettes();
        theme.set_palettes(light, dark);
        theme.set_mode(ThemeMode::Dark);
        let mut board = TileGrid::new(GRID_COLS);
        let mut cards = Vec::new();
        for (n, placed) in spec::BOARD.iter().enumerate() {
            let def = def_of(placed.kind).expect("the board names catalogue kinds");
            let id = format!("{}#{n}", placed.kind);
            board
                .place(Tile::new(
                    id.clone(),
                    placed.col,
                    placed.row,
                    placed.cols,
                    placed.rows,
                ))
                .expect("the specified board is a legal arrangement");
            cards.push(
                Card::new(id, def.label)
                    .with_chrome(CardChrome::of(chrome()))
                    .with_state(CardState::Ready),
            );
        }
        let roster = spec::destinations();
        let mut presets = BTreeMap::new();
        presets.insert(
            spec::PRESET.to_string(),
            Preset {
                board: board.clone(),
                cards: cards.clone(),
            },
        );
        Self {
            clock,
            theme,
            board: Signal::new(board),
            cards: Signal::new(cards),
            maximized: Signal::new(None),
            floats: Signal::new(Vec::new()),
            presets: RefCell::new(presets),
            preset: Signal::new(spec::PRESET.to_string()),
            preset_open: Signal::new(false),
            editing: Signal::new(false),
            config_open: Signal::new(None),
            source: Signal::new(SOURCES[0].to_string()),
            capturing: Signal::new(true),
            search: Signal::new(String::new()),
            searching: Signal::new(false),
            tab: Signal::new(TABS[0].to_string()),
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
            pressed: RefCell::new(None),
            drag: Signal::new(None),
            standing: RefCell::new(DropStanding::Nowhere),
            cursors: RefCell::new(
                spec::FOCUS_RING
                    .iter()
                    .filter_map(|stop| stop.cursor.map(|spec| (stop.tag, Roving::new(spec))))
                    .collect(),
            ),
            float_grab: Signal::new(None),
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
    fn detached(&self) -> Vec<(String, String)> {
        self.floats_front_to_back()
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
                        matches!(seat.seat, spec::Seat::Page),
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
            _ if stop == filter_chips_tag() => filter_row_of(FILTER_CARD, None, 0)
                .cursor()
                .map(|roving| roving.members().to_vec())
                .unwrap_or_default(),
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
        tabs.seat(vec![
            Member::new(BarChip::Tab0.tag()),
            Member::new(BarChip::Tab1.tag()),
        ]);
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
        self.presets
            .borrow()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(",")
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

/// The toast's box. The width is a constant and the reference sizes to content
/// — see [`toast_scene`] for why that half is deliberately still open.
/// The widest a toast may be, whatever it says — a long sentence elides rather
/// than growing a strip across the window.
const TOAST_W: u32 = 560;

/// Where the toast's sentence starts, past its tone bullet.
const TOAST_TEXT_X: u32 = 32;

/// The room kept to the right of the sentence, so the border does not sit on
/// the last glyph.
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
fn toast_width(sentence: &str) -> u32 {
    let glyphs = u32::try_from(sentence.chars().count()).unwrap_or(u32::MAX);
    let run = glyphs.saturating_mul(FONT_BODY.saturating_sub(6));
    (TOAST_TEXT_X + run + TOAST_PAD_RIGHT).clamp(TOAST_MIN_W, TOAST_W)
}

/// Narrow enough for a two-word sentence, wide enough that the bullet and the
/// rounded corners still read as a strip rather than a pill.
const TOAST_MIN_W: u32 = 180;
/// The toast's height.
const TOAST_H: u32 = 34;

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
    owner.cache(STATE_KEY, move || ShellState::new(clock, theme, toast))
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
/// ⚠ What that costs, stated rather than hidden: **resizing the in-canvas
/// float no longer resizes its window.** That is not a gap this round can
/// close by patching, because it is the fork the debt itself flagged — one
/// card now has two things claiming to be it, a panel on the canvas and a
/// window on the desktop, and deciding which one a person manipulates is a
/// design decision rather than an arithmetic one. Registered as its own debt.
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
            let floats = state.floats.get();
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
fn on_board(state: &ShellState, px: u32, py: u32) -> bool {
    state.at() == "dashboard"
        && contains(canvas_rect(), px, py)
        && !matches!(
            Hit::at(state, px, py),
            Hit::Float(_) | Hit::FloatRedock(_) | Hit::FloatClose(_) | Hit::FloatResize(_)
        )
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
};

/// One header control slot. Right-aligned, in declaration order, so the
/// rightmost is the last affordance the vocabulary declares.
fn affordance_rect(header: Rect, count: u32, n: u32) -> Rect {
    card_header::slot_rect(header, count, n, CARD_METRICS)
}

// ★ R1817 — `grip_rect` used to be declared here. `card_header::grip_scene`
// draws the grip now and nothing on this side asks where it is, so the
// delegating wrapper R1816 left became dead code. That is the compiler saying
// the lift finished rather than a comment claiming it did.

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarChip {
    Tab0,
    Tab1,
    Source,
    Capture,
    Search,
}

impl BarChip {
    const ALL: [Self; 5] = [
        Self::Tab0,
        Self::Tab1,
        Self::Source,
        Self::Capture,
        Self::Search,
    ];

    const fn tag(self) -> &'static str {
        match self {
            Self::Tab0 => "shell.appbar.tab.dashboard",
            Self::Tab1 => "shell.appbar.tab.design",
            Self::Source => "shell.appbar.source",
            Self::Capture => "shell.appbar.capture",
            Self::Search => "shell.appbar.search",
        }
    }

    fn rect(self) -> Rect {
        match self {
            Self::Tab0 => Rect::new(168, 10, 108, 32),
            Self::Tab1 => Rect::new(280, 10, 118, 32),
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
        let bar_w = win_w() - RAIL_W - PALETTE_W;
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
fn preset_item_rect(n: u32) -> Rect {
    let anchor = SubChip::Preset.rect();
    Rect::new(
        RAIL_W + anchor.x + 8,
        APP_BAR_H + anchor.y + 44 + n * 34,
        210,
        30,
    )
}

/// Where the `n`th rail entry sits, in the rail container's own space.
const fn rail_rect(n: u32) -> Rect {
    Rect::new(8, 14 + n * 44, RAIL_W - 16, 36)
}

/// One palette entry's height.
///
/// Sized so that the whole catalogue FITS the panel: the reference scrolls its
/// palette and this shell does not, so a row height that overflowed would put
/// the last widget kinds under the footer where nothing can reach them. That is
/// a real difference from the reference and it is spent here rather than
/// hidden — see the module docs' list of what is not matched.
const PALETTE_ROW_H: u32 = 46;

/// The palette panel's rectangle.
fn palette_rect() -> Rect {
    Rect::new(
        win_w() - PALETTE_W,
        APP_BAR_H,
        PALETTE_W,
        win_h() - APP_BAR_H,
    )
}

/// Where the strip that names this screen's gestures sits — the band between
/// the toast and the palette.
///
/// ★★ R1701 — it was a flat `470` at a flat offset, and adding one gesture to
/// the sentence pushed it past that number: the strip read "… Esc restor…" in a
/// window with room to spare. That is the third time this project has met a
/// width chosen at the design size and required to keep a relation to something
/// that moves (R1687's launch floor, R1700's node-lab hint, this), so it is
/// derived: the room is what lies between where the strip starts and the panel
/// on the right, less the gap that keeps them from touching.
///
/// Deriving it rather than widening the number is also what keeps the text-smear
/// gate satisfied — a strip that simply took the whole window would paint over
/// the palette, which is the failure R1701's node-lab sibling walked into on its
/// first attempt.
fn help_strip_rect() -> Rect {
    let x = canvas_rect().x + 610;
    let room = palette_rect().x.saturating_sub(x + 16);
    Rect::new(x, win_h() - 47, room, 14)
}

/// The palette's rows — section headers interleaved with entries, in the
/// panel's own space.
///
/// Returned rather than recomputed at each site: the painter walks it to draw
/// and the hit test walks it to resolve, which is the discipline the card
/// rectangles follow.
fn palette_rows() -> Vec<PaletteRow> {
    let mut out = Vec::new();
    let mut y = 76_u32;
    for (key, title) in spec::SECTIONS {
        out.push(PaletteRow {
            def: None,
            section: key,
            // ★ R1761 — the heading says which release fills the group, which
            // is what the reference writes there. ★ R1797 — derived from the
            // group's own ENTRIES rather than from a tier column beside it, so
            // promoting one widget cannot leave the heading behind.
            title: spec::section_heading(key, title),
            rect: Rect::new(16, y, PALETTE_W - 32, 20),
        });
        y += 26;
        for def in spec::CATALOGUE.iter().filter(|w| w.section == *key) {
            out.push(PaletteRow {
                def: Some(def),
                section: key,
                title: def.label.to_owned(),
                rect: Rect::new(10, y, PALETTE_W - 30, PALETTE_ROW_H),
            });
            y += PALETTE_ROW_H + 4;
        }
        y += 8;
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
    Choose(&'static str),
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
    Remedy(String),
    Card(String),
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
            for chip in BarChip::ALL {
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
            let panel = palette_rect();
            let (lx, ly) = (px - panel.x, py - panel.y);
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
        if let Some(chip) = BarChip::ALL.into_iter().find(|c| c.tag() == tag) {
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
            if let Some((key, picker)) = picking.as_ref() {
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
                return Self::Choose(row.key);
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
    fn in_canvas(state: &ShellState, cx: u32, cy: u32) -> Self {
        // ★ R1697 — floats are over the canvas, FRONTMOST first. It was the
        // vector read backwards, which is the same answer only while nothing
        // reorders them; a press now raises the panel it lands on, so the
        // stacking order is state and the hit test reads that state.
        for float in state.floats_front_to_back() {
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
                if contains(affordance_rect(header, 2, 1), lx, ly) {
                    return Self::FloatClose(float.id.clone());
                }
                if contains(affordance_rect(header, 2, 0), lx, ly) {
                    return Self::FloatRedock(float.id.clone());
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
        let board = state.board.get();
        let editing = state.editing.get();
        for card in &state.placed() {
            let Some(tile) = board.tile(card.id()) else {
                continue;
            };
            let rect = cell_rect(tile);
            if !contains(rect, cx, cy) {
                continue;
            }
            let (lx, ly) = (cx - rect.x, cy - rect.y);
            let inside = local(rect);
            let id = card.id().as_str().to_string();
            let header = header_rect(inside);
            if contains(header, lx, ly) {
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
            return Self::Card(id);
        }
        Self::Nothing
    }

    fn card_id(&self) -> Option<&str> {
        match self {
            Self::Affordance(id, _)
            | Self::Remedy(id)
            | Self::Card(id)
            | Self::Grip(id)
            | Self::FilterChip(id, _)
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
        Hit::Choose(key) => settings_choose_tag(key),
        // The suffix vocabulary the framework's roster lays its options under,
        // so a driver presses the name the paint published.
        Hit::ChooseOption(key, n) => format!(
            "shell.settings.option.{key}.{}",
            settings_options_of(key)
                .get(*n)
                .cloned()
                .unwrap_or_default()
        ),
        Hit::Theme(n) => format!("shell.settings.theme.{n}"),
        Hit::Palette(kind) => format!("shell.palette.{kind}"),
        Hit::Grip(id) => format!("card.{id}.grip"),
        Hit::Affordance(id, affordance) => format!("card.{id}.{}", affordance.wire()),
        Hit::Stepper(id, verb) => format!("card.{id}.{verb}"),
        Hit::Remedy(id) => format!("card.{id}.remedy"),
        Hit::FilterChip(id, n) => format!("card.{id}.chip.{n}"),
        Hit::Card(id) => format!("card.{id}"),
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
    /// test stays where it is, inside the gesture — see `on_board`.
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
        state.say(Utterance::done(format!(
            "{} \u{2192} {w}\u{00D7}{h}",
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
        let stored = state.presets.borrow().get(name).cloned();
        let preset = stored.ok_or_else(|| {
            InterveneError::out_of_range(format!(
                "{name:?} is not a saved layout; they are {}",
                state.preset_names()
            ))
        })?;
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
        if name.is_empty() {
            return Err(InvokeError::rejected("a layout preset needs a name"));
        }
        state.presets.borrow_mut().insert(
            name.to_string(),
            Preset {
                board: state.board.get(),
                cards: state.cards.get(),
            },
        );
        state.preset.set(name.to_string());
        state.say(Utterance::done(format!("layout saved \u{00B7} {name}")));
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
        // R1826 — which OS window carries each detached card.
        SchemaField::new("detached", "json"),
        SchemaField::new("float_grab", "string"),
        // named layouts
        SchemaField::new("preset", "string"),
        SchemaField::new("presets", "string"),
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
        SchemaField::action("save_preset", "string"),
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
                })
            })
            .collect(),
    )
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
            "tabs" => text(TABS.join(",")),
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
            // What the pointer is doing to a panel right now, or empty. The
            // peer of `drag` below, and separate for the reason the types are
            // separate: one gesture lands in a cell and the other in a pixel.
            "float_grab" => text(state.float_grab.get().map_or_else(String::new, |g| {
                format!("{},{}", g.id, if g.edge { "resize" } else { "move" })
            })),
            "preset" => text(state.preset.get()),
            "presets" => text(state.preset_names()),
            "preset_open" => Ok(IntrospectValue::Bool(state.preset_open.get())),
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
            "cursor" => {
                let (x, y) = state.cursor.get();
                text(format!("{x},{y}"))
            }
            "selected" => text(state.selected.get().unwrap_or_default()),
            "hit" => {
                let (x, y) = state.cursor.get();
                text(hit_word(&Hit::at(state, x, y)))
            }
            "drag" | "carrying" => Ok(carry_slot(state, path)),
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
                let chosen = TABS.iter().find(|t| **t == name).ok_or_else(|| {
                    InterveneError::out_of_range(format!(
                        "{name:?} is not a tab; they are {}",
                        TABS.join(", ")
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
            | "restore_to" | "floating" | "floats" | "detached" | "float_grab" | "presets"
            | "transport" | "playhead" | "affordances" | "states" | "remedies" | "steppers"
            | "toast" | "cursor" | "selected" | "hit" | "keymap" | "rail" | "tabs"
            | "catalogue" | "config_open" | "drag" | "carrying" => Err(InterveneError::ReadOnly),
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
            "resize" => {
                let raw = Self::text(&args)?;
                let (id, verb) = raw.split_once(',').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <card>,<step>"))
                })?;
                Self::step(&state, id.trim(), verb.trim())
            }
            "save_preset" => Self::save_preset(&state, Self::text(&args)?.trim()),
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
        let outcome = if on_board(state, px, py) {
            let (col, row) = cell_at_window(state, px, py);
            drag.hover(&state.board.get(), col, row);
            drag.landing()
                .ok_or("this footprint does not fit at that cell")
        } else if state.at() == "dashboard" {
            drag.leave();
            Err("the board is the canvas, and the cursor is not over it")
        } else {
            drag.leave();
            Err("a widget lands on the dashboard's board, and that is not the page showing")
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
        if let Hit::Grip(id) = &hit {
            let board = state.board.get();
            let (col, row) = cell_at_window(state, px, py);
            if let Ok(drag) = TileDrag::grip(&board, &TileId::new(id.clone()), col, row) {
                state.drag.set(Some(drag));
            }
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
            Self::open_float_grab(state, id, matches!(hit, Hit::FloatResize(_)), (px, py));
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
            // ★★★★★ R1733 — an ABANDONED carry falls through to the latch.
            //
            // That is what keeps the palette's action alive now that pressing a
            // row also picks the widget up: press and release on the same row
            // carries it nowhere, so the latched hit acts and the card is added
            // at the bottom exactly as before. Fidelity to a pointer-only
            // reference must not cost a reader the only path they have.
            if !Self::commit_drag(state, drag) {
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
            Hit::Choose(key) => Self::toggle_roster(state, key),
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
                            Unavailable::reserved(row.reserved_for).sentence()
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
            // ★★★★★ R1721 — the rule applies the choice and says what happened;
            // this arm only stores it. `Utterance` either way, so a refusal
            // (there is one: a rule that keeps one on) reaches the person by the
            // same path a success does.
            Hit::FilterChip(id, n) => Self::choose_filter(state, &id, n),
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
            BarChip::Tab0 | BarChip::Tab1 => {
                let name = if chip == BarChip::Tab0 {
                    TABS[0]
                } else {
                    TABS[1]
                };
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
                // The palette is always open in this shell, so the button is
                // what SAYS where widgets come from rather than opening a second
                // chooser.
                //
                // ★ R1695 — it used to move the rail's highlight to `catalog`
                // as well, which is this round's own defect in miniature: the
                // rail said you were at a destination the window had not taken
                // you to. The pointer is what the button aims, and it aims at
                // the palette on this page.
                state.say(Utterance::done("pick a widget from the palette \u{2192}"));
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
        let options = settings_options_of(key);
        let chosen = settings_value_of(state, key);
        match Picker::over(options, &chosen) {
            Ok(picker) => {
                let title = settings_value_title(key);
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
        match key {
            "interface" => state.source.set(word.clone()),
            "retention" => state.retention.set(word.clone()),
            other => {
                panic!("the specification names a value row {other:?} this shell cannot answer")
            }
        }
        state.say(Utterance::done(format!(
            "{} {word}",
            settings_value_title(key)
        )));
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
        let names: Vec<String> = state.presets.borrow().keys().cloned().collect();
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
    LayoutStyle::new()
        .with_absolute_position(rect.x, rect.y)
        .with_size(Size::px(rect.w, rect.h))
        .with_pointer_transparent(true)
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
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_style(BoxStyle::filled(fill).with_corner_radius(size / 2))
            .with_layout(absolute(Rect::new(x, y, size, size))),
    )
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

// ★ R1817 — the detach mark moved to `card_header::affordance_mark` with the
// rest of the header's glyphs.

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

// ★ R1817 — `affordance_mark` moved to `card_header`, and R1697's lesson went
// with it: `restore` is the maximise control's OTHER face, because a control
// that toggles without changing its mark tells a person the same thing in both
// states. A lesson left behind when its code moves is a lesson nobody re-reads.

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
            label(
                text,
                Rect::new(28, rect.h / 2 - 8, rect.w.saturating_sub(38), 16),
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
        ContainerNode::new(vec![label(
            text,
            Rect::new(14, rect.h / 2 - 8, rect.w.saturating_sub(24), 16),
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
    let mut children = vec![
        dot(16, 18, 16, palette.accent),
        label(
            "Analyzer",
            Rect::new(42, 17, 118, 18),
            FONT_TITLE,
            palette.ink,
        ),
    ];
    for (n, name) in TABS.iter().enumerate() {
        let chip = if n == 0 { BarChip::Tab0 } else { BarChip::Tab1 };
        let on = state.tab.get() == *name;
        children.push(Scene::Container(
            ContainerNode::new(vec![label(
                name,
                Rect::new(14, 8, chip.rect().w.saturating_sub(20), 16),
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
        BarChip::Source.tag(),
        rgb(0x35_C0_8B),
        &state.source.get(),
        palette,
    ));
    let capturing = state.capturing.get();
    children.push(pill(
        BarChip::Capture.rect(),
        BarChip::Capture.tag(),
        if capturing {
            palette.accent_fg
        } else {
            palette.muted
        },
        if capturing { spec::TRANSPORT } else { "Paused" },
        palette,
    ));
    children.push(label(
        &transport_word(state.clock.status(), capturing),
        Rect::new(842, 19, 92, 16),
        FONT_SMALL,
        palette.muted,
    ));
    // The rate readout: what a capture tool is counting while it runs.
    children.push(label(
        spec::RATE,
        Rect::new(938, 19, 96, 16),
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
        ContainerNode::new(vec![label(
            &if searching {
                format!("{search}|")
            } else if search.is_empty() {
                spec::SEARCH_HINT.to_string()
            } else {
                search
            },
            Rect::new(12, 8, BarChip::Search.rect().w.saturating_sub(20), 16),
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
                label(
                    &state.preset.get(),
                    Rect::new(12, 8, preset.w.saturating_sub(38), 16),
                    FONT_TITLE,
                    palette.ink,
                ),
                strokes(
                    Rect::new(preset.w - 26, 13, 12, 8),
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
                    win_w() - RAIL_W - PALETTE_W,
                    SUB_BAR_H,
                ))),
        ),
        "shell.subbar",
        &state.at(),
    )
}

/// The saved-layout menu: a **top-level popup**, painted in window space — the
/// same space [`preset_item_rect`] gives the hit test.
fn preset_menu_scene(state: &ShellState, palette: Palette) -> Scene {
    let names: Vec<String> = state.presets.borrow().keys().cloned().collect();
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
    for (n, name) in names.iter().enumerate() {
        let row = preset_item_rect(u(n));
        let on = &state.preset.get() == name;
        children.push(Scene::Container(
            ContainerNode::new(vec![label(
                name,
                Rect::new(12, 7, row.w.saturating_sub(20), 16),
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
            Rect::new(12, 7, save.w.saturating_sub(20), 16),
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
        .with_layout(absolute(Rect::new(10, win_h() - APP_BAR_H - 46, 32, 32))),
    ));
    keyboard_stop(
        Scene::Container(
            ContainerNode::new(entries)
                .with_tag("shell.rail")
                .with_style(BoxStyle::filled(palette.panel))
                .with_layout(absolute(Rect::new(
                    0,
                    APP_BAR_H,
                    RAIL_W,
                    win_h().saturating_sub(APP_BAR_H),
                ))),
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
fn settings_roster_scene(state: &ShellState, at: &str) -> Scene {
    let empty = Scene::Container(ContainerNode::new(Vec::new()));
    if at != "settings" {
        return empty;
    }
    let picking = state.picking.borrow();
    let Some((key, picker)) = picking.as_ref() else {
        return empty;
    };
    let region = page_rect("settings");
    let theme = use_theme(THEME_TAG).theme_animated();
    let roster = chooser::lay_roster(
        key,
        settings_control_rect(region, key),
        picker,
        region,
        SET_OPTION_H,
    );
    chooser::view_roster(
        "shell.settings",
        &roster,
        picker,
        &settings_value_of(state, key),
        (0, 0),
        &theme,
    )
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
fn settings_value_of(state: &ShellState, key: &str) -> String {
    match key {
        "interface" => state.source.get(),
        "retention" => state.retention.get(),
        other => panic!("the specification names a value row {other:?} this shell cannot answer"),
    }
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
///
/// # Panics
///
/// If asked about a key the specification does not name, which is a defect in
/// this file rather than a state the screen can reach.
fn settings_value_title(key: &str) -> &'static str {
    spec::VALUE_ROWS
        .iter()
        .find(|row| row.key == key)
        .map(|row| row.title)
        .expect("the specification names every value row this shell draws")
}

/// What a value row may be set to, in the order the roster lists them.
fn settings_options_of(key: &str) -> Vec<String> {
    match key {
        "interface" => SOURCES.iter().map(|s| (*s).to_owned()).collect(),
        "retention" => spec::RETENTIONS.iter().map(|s| (*s).to_owned()).collect(),
        other => panic!("the specification names a value row {other:?} this shell cannot answer"),
    }
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
            .with_layout(
                absolute(seat).with_unavailable(Unavailable::reserved(key_row.reserved_for)),
            ),
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

/// A card's header: grip, status light, title, LIVE badge, controls.
fn header_scene(card: &Card, rect: Rect, palette: Palette, maximized: bool) -> Vec<Scene> {
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
    card_header::header_scene(
        &format!("card.{id}"),
        rect,
        &card_header::HeaderSpec {
            offered: &offered,
            ready: card.state().is_ready(),
            restore: maximized,
            title: card.title(),
            badge: "LIVE",
            title_px: FONT_BODY,
            badge_px: FONT_TINY,
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

// ★★★ R1695 — there is no `centred` helper here, and the reason is a
// measurement worth keeping.
//
// The first repair for the left-flush button labels set
// `TextStyle::with_align(TextAlign::Center)`. Measured by ink span on the
// rendered page it did **nothing**: the segment's `Dark` inked 634..659 inside a
// chip starting at 635, and `Import…` inked 683..729 in a box from 682 — the
// glyphs at the node's left edge in every case. The framework's own button does
// not use that property either; `pinion_widget_paint::button::view_button`
// centres with `JustifyContent::Center` on a flex row, which is the idiom this
// screen now uses. Filed as `debt-a-declared-text-alignment-does-nothing-on-an-
// absolutely-placed-run` rather than worked around, because the property is
// published, accepted and reported back on `scene/snapshot`.

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

/// The tag a table card's cell is addressed by.
fn cell_tag(id: &str, row: usize, column: usize) -> String {
    format!("card.{id}.cell.{row}_{column}")
}

/// The tag a table card's column header is addressed by.
fn head_cell_tag(id: &str, column: usize) -> String {
    format!("card.{id}.head.{column}")
}

/// The message stream: a header row of columns over the opening rows.
fn stream_body(state: &ShellState, id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    const HEAD_H: u32 = 20;
    const ROW_H: u32 = 20;
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
                    cell(
                        head_cell_tag(id, c),
                        name,
                        Rect::new(*x, 4, *w, 13),
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
    for (n, (time, kind, name, len)) in spec::STREAM_ROWS.iter().enumerate() {
        let top = rect.y + HEAD_H + u(n) * ROW_H;
        // A row whose bottom would leave the card is not painted at all. The
        // alternative -- painting it and letting it land on the card below --
        // is the defect R1656 measured on twenty-five surfaces.
        if top + ROW_H > rect.y + rect.h {
            break;
        }
        // A cell per column that fits, in the specification's order, with the
        // row's own values. Zipped rather than indexed: a narrow card drops
        // columns from the right, and indexing would reach past the end.
        let values = [*time, *kind, *name, *len];
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
                cell(
                    cell_tag(id, n, c),
                    value,
                    Rect::new(*x, 3, *w, 13),
                    FONT_TINY,
                    ink,
                    overflow,
                )
            })
            .collect();
        out.push(Scene::Container(
            ContainerNode::new(cells)
                .with_tag(format!("card.{id}.row.{n}"))
                .with_layout(absolute(Rect::new(rect.x, top, rect.w, ROW_H))),
        ));
    }
    out
}

/// The decode inspector: the layer tree beside the bytes it decoded.
fn decode_body(id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    const ROW_H: u32 = 19;
    // The tree keeps at least half the card; the bytes pane takes what is left,
    // and is dropped entirely when that is less than one byte's worth. A fixed
    // pane on a card narrower than the pane paints outside the card, which the
    // gate reports and a reader sees as one card's bytes on the next one.
    let bytes_w = BYTES_W.min(rect.w / 2);
    let tree_w = if bytes_w >= BYTES_FLOOR {
        rect.w.saturating_sub(bytes_w + 12)
    } else {
        rect.w
    };
    let mut out = Vec::new();
    for (n, (depth, key, value)) in spec::DECODE_ROWS.iter().enumerate() {
        let top = rect.y + 4 + u(n) * ROW_H;
        if top + ROW_H > rect.y + rect.h {
            break;
        }
        let indent = (10 + depth * 12).min(tree_w);
        let heading = *depth == 0;
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
        if key_w > 0 {
            cells.push(clipped(
                key,
                Rect::new(indent, 3, key_w, 13),
                FONT_TINY,
                if heading { palette.ink } else { palette.muted },
                TextOverflow::Ellipsis,
            ));
        }
        if with_value {
            cells.push(clipped(
                value,
                Rect::new(indent + key_w, 3, VALUE_W, 13),
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
                .with_layout(absolute(Rect::new(rect.x, top, tree_w, ROW_H))),
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
    const ROW_H: u32 = 19;
    let mut out = Vec::new();
    if pane.w < BYTES_FLOOR {
        return out;
    }
    let (start, end) = spec::DECODE_SELECTED_SPAN;
    for (line, quad) in spec::DECODE_BYTES.iter().enumerate() {
        let top = card.y + 4 + u(line) * ROW_H;
        if top + ROW_H > card.y + card.h {
            break;
        }
        let mut cells = vec![label(
            &format!("{:04x}", line * 4),
            Rect::new(6, 3, 30, 13),
            FONT_TINY,
            palette.muted,
        )];
        for (col, byte) in quad.iter().enumerate() {
            let index = line * 4 + col;
            // A cell that would leave the pane is not painted. Its bytes are
            // not lost -- the pane is simply narrower than four columns, and a
            // cell drawn past the edge lands on whatever is beside the card.
            if 40 + u(col) * 24 + 22 > pane.w {
                break;
            }
            let lit = index >= start && index < end;
            cells.push(Scene::Container(
                ContainerNode::new(vec![label(
                    &format!("{byte:02x}"),
                    Rect::new(2, 3, 18, 13),
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
                .with_layout(absolute(Rect::new(pane.x, top, pane.w, ROW_H))),
        ));
    }
    out
}

/// The identifier map: numeric id to resource path, and when it was declared.
fn map_body(id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    const HEAD_H: u32 = 18;
    const ROW_H: u32 = 18;
    // The columns are allocated left to right and a column with nothing left is
    // dropped, so a narrowed card shows the id and the resource rather than the
    // id and the timestamp. Same discipline as the stream's columns and the
    // decode tree's key: the identifying half is allocated first.
    const ID_W: u32 = 34;
    const SEEN_W: u32 = 66;
    const PATH_FLOOR: u32 = 40;
    let room = rect.w.saturating_sub(12 + ID_W + 6);
    let with_seen = room >= PATH_FLOOR + SEEN_W;
    let path_w = if with_seen { room - SEEN_W } else { room };
    // `row` is `None` for the header strip and the row index otherwise, which is
    // what the cell tags are built from.
    let cells = |ink: Color, cols: [&str; 3], warn: bool, row: Option<usize>| {
        let tag = |column: usize| match row {
            None => head_cell_tag(id, column),
            Some(r) => cell_tag(id, r, column),
        };
        let mut out = vec![cell(
            tag(0),
            cols[0],
            Rect::new(12, 2, ID_W, 13),
            FONT_TINY,
            if warn { palette.warn } else { ink },
            TextOverflow::Ellipsis,
        )];
        if path_w > 0 {
            out.push(cell(
                tag(1),
                cols[1],
                Rect::new(12 + ID_W + 6, 2, path_w, 13),
                FONT_TINY,
                ink,
                TextOverflow::EllipsisStart,
            ));
        }
        if with_seen {
            out.push(cell(
                tag(2),
                cols[2],
                Rect::new(12 + ID_W + 6 + path_w, 2, SEEN_W, 13),
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
    for (n, (key, path, seen)) in spec::MAP_ROWS.iter().enumerate() {
        let top = rect.y + HEAD_H + u(n) * ROW_H;
        if top + ROW_H > rect.y + rect.h {
            break;
        }
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
            .with_layout(absolute(Rect::new(rect.x, top, rect.w, ROW_H))),
        ));
    }
    out
}

/// The search and filter card: the query, the saved chips, and the three counts
/// whose relation is the point of the card.
/// The height the latency card keeps for its caption.
const LATENCY_CAPTION_H: u32 = 24;

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

/// The latency card's content: three derived tiles, the binned distribution
/// with its tail emphasised, and the caption that says what the emphasis means.
fn latency_body(id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    let Some((binned, quantiles)) = latency_binned() else {
        return placeholder_body("latency", id, rect, palette);
    };
    let mut out = Vec::new();

    // --- the three tiles ---------------------------------------------------
    let stats = latency_stats(&quantiles);
    let stat_w = rect.w.saturating_sub(2 * 8) / u(stats.len());
    let bars_top = if stat_w < STAT_FLOOR {
        rect.y
    } else {
        for (n, (key, value)) in stats.iter().enumerate() {
            out.push(Scene::Container(
                ContainerNode::new(vec![
                    label(
                        key,
                        Rect::new(10, 6, stat_w.saturating_sub(20), 12),
                        FONT_TINY,
                        palette.muted,
                    ),
                    label(
                        value,
                        Rect::new(10, 22, stat_w.saturating_sub(20), 17),
                        FONT_TITLE,
                        palette.ink,
                    ),
                ])
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
        rect.y + STAT_H + 10
    };

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
    if let Some(box_) = distribution_box(rect, bars_top, bars.len(), &style) {
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
    if caption_y > bars_top {
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
                    Rect::new(9, 4, at.w.saturating_sub(18), 13),
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
    let last_line = placed.last().map_or(rect.y + 34, |(_, at)| at.y);
    out.extend(filter_counts(
        state,
        id,
        Rect::new(
            rect.x,
            last_line + 30,
            rect.w,
            rect.y + rect.h - (last_line + 30).min(rect.y + rect.h),
        ),
        rect,
        palette,
    ));
    out
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
    filter_row_of(id, state.filter_chip.get(), seat)
}

/// The bar built from a choice and a cursor, so the roster and the rule are
/// readable without a running screen — the ring's roster is asked for from
/// `cursor_members`, which has no state to read and must not grow a second copy
/// of what the chips are.
fn filter_row_of(id: &str, chosen: Option<usize>, cursor: usize) -> ChipGroup {
    ChipGroup::new(
        format!("card.{id}.chips"),
        "Saved filters",
        spec::FILTER_CHIPS
            .iter()
            .enumerate()
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
    let stat_w = area.w.saturating_sub(2 * 8) / u(spec::FILTER_STATS.len());
    if stat_w < STAT_FLOOR || area.y + STAT_H > card.y + card.h {
        return out;
    }
    for (n, (value, what)) in spec::FILTER_STATS.iter().enumerate() {
        out.push(Scene::Container(
            ContainerNode::new(vec![
                label(
                    value,
                    Rect::new(10, 7, stat_w.saturating_sub(20), 17),
                    FONT_TITLE,
                    palette.ink,
                ),
                label(
                    what,
                    Rect::new(10, 27, stat_w.saturating_sub(20), 13),
                    FONT_TINY,
                    palette.muted,
                ),
            ])
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
    let mut children = Vec::new();
    for (n, (verb, glyph)) in STEPPERS.iter().enumerate() {
        let slot = stepper_rect(bar, u(n));
        children.push(Scene::Container(
            ContainerNode::new(vec![label(
                glyph,
                Rect::new(6, 2, 14, 14),
                FONT_BODY,
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
    children.push(label(
        "W",
        Rect::new(58, 6, 12, 14),
        FONT_SMALL,
        palette.muted,
    ));
    children.push(label(
        "H",
        Rect::new(136, 6, 12, 14),
        FONT_SMALL,
        palette.muted,
    ));
    children.push(label(
        &format!("{}\u{00D7}{}", cell.0, cell.1),
        Rect::new(bar.w.saturating_sub(48), 6, 40, 14),
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
#[derive(Clone, Copy)]
struct CardFace {
    /// The board's selection rests on this card.
    selected: bool,
    /// The board is in layout-editing mode, so the card shows its edit bar.
    editing: bool,
    /// This card is the one wearing the restore face of the maximise control.
    maximized: bool,
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
    let mut children: Vec<Scene> = header_scene(card, header_rect(inside), palette, maximized)
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
        Scene::Container(
            ContainerNode::new(vec![redock_mark(
                local(affordance_rect(header, 2, 0)),
                palette.muted,
            )])
            .with_tag(format!("float.{}.redock", float.id))
            .with_layout(absolute(affordance_rect(header, 2, 0))),
        ),
        Scene::Container(
            ContainerNode::new(vec![close_mark(
                local(affordance_rect(header, 2, 1)),
                palette.muted,
            )])
            .with_tag(format!("float.{}.close", float.id))
            .with_layout(absolute(affordance_rect(header, 2, 1))),
        ),
    ];
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
    // at the size the topology asked the operating system for rather than at
    // whatever `window_size()` reports for the MAIN window. And its ABSENCE is
    // the whole answer — see the header.
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
            let built = spec::destinations();
            let divergences: Vec<serde_json::Value> = spec::canon_spec()
                .diff(&built)
                .iter()
                .map(|d| serde_json::json!({ "key": d.key(), "says": d.sentence() }))
                .collect();
            let ledger = spec::owed();
            let owed: Vec<serde_json::Value> = ledger
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
                .collect();
            let specified = spec::canon_spec().len();
            Ok(IntrospectValue::Json(serde_json::json!({
                "specified": specified,
                "reproduced": specified - divergences.len(),
                "divergences": divergences,
                "owed": owed,
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
            "open": matches!(seat.seat, spec::Seat::Page),
        })).collect::<Vec<_>>(),
        "rail_active": spec::RAIL_ACTIVE,
        // ★ R1695 — the Settings destination.
        "options": spec::OPTIONS.iter().map(|o| serde_json::json!({
            "key": o.key, "title": o.title, "gist": o.gist,
            "group": o.group, "opens": o.opens,
        })).collect::<Vec<_>>(),
        "key_rows": spec::KEY_ROWS.iter().map(|r| serde_json::json!({
            "key": r.key, "title": r.title, "gist": r.gist,
            "verb": r.verb, "reserved_for": r.reserved_for,
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
        "focus_ring": spec::FOCUS_RING.iter().map(|stop| serde_json::json!({
            "tag": stop.tag, "holds": stop.holds, "at": where_word(stop.at),
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

/// The palette panel: the catalogue, grouped, with a count at the foot.
fn palette_scene(state: &ShellState, palette: Palette) -> Scene {
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
            Rect::new(16, 18, 220, 20),
            FONT_TITLE,
            palette.ink,
            TextOverflow::Ellipsis,
        )
        // ★ R1761 — these words ARE the panel's accessible name, so saying them
        // again would be one fact in two voices. Addressable for the
        // specification, silent for the reader.
        .silenced(Silence::name_of("shell.palette")),
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
    children.push(cell(
        "shell.palette.placed".to_owned(),
        &format!(
            "{} placed of {}",
            state.placed().len(),
            spec::placeable_count()
        ),
        Rect::new(16, panel.h.saturating_sub(30), 130, 16),
        FONT_SMALL,
        palette.muted,
        TextOverflow::Ellipsis,
    ));
    children.push(cell(
        "shell.palette.reserved".to_owned(),
        &format!("{} reserved", spec::reserved_count()),
        Rect::new(
            panel.w.saturating_sub(110),
            panel.h.saturating_sub(30),
            94,
            16,
        ),
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
fn toast_scene(state: &ShellState, palette: Palette) -> Option<Scene> {
    let said = state.toast.showing()?;
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
    // `slack` reports the room. Between the two, an estimate cannot rot
    // silently in either direction.
    let width = toast_width(&said.sentence());
    let rect = Rect::new(
        (win_w().saturating_sub(width)) / 2,
        win_h().saturating_sub(22 + TOAST_H),
        width,
        TOAST_H,
    );
    Some(Scene::Container(
        ContainerNode::new(vec![
            // ★★★★★ R1719 — the bullet was `accent_fg` whatever had been said.
            // A refusal and a confirmation were one picture, on the screen and
            // in a reader's ear both.
            dot(14, 13, 8, toast_dot(said.tone(), palette)),
            label(
                &said.sentence(),
                Rect::new(
                    TOAST_TEXT_X,
                    9,
                    rect.w.saturating_sub(TOAST_TEXT_X + TOAST_PAD_RIGHT),
                    16,
                ),
                FONT_BODY,
                palette.ink,
            ),
        ])
        .with_tag("shell.toast")
        .with_style(
            BoxStyle::filled(palette.raised)
                .with_corner_radius(10)
                .with_border(Border::new(palette.outline, 1)),
        )
        .with_layout(absolute(rect)),
    ))
}

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
const fn toast_dot(tone: Tone, palette: Palette) -> Color {
    match tone {
        Tone::Done => palette.accent_fg,
        Tone::Refused => palette.refused,
        // Nothing happened. The bullet says "heard you" rather than "did it",
        // in the ink this screen already uses for present-but-not-the-point.
        Tone::Unchanged => palette.muted,
    }
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
        .chain(toast_scene(&state, palette))
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
        .chain([
            // ★★ R1672 — the preset menu is a POPUP: anchored to the sub bar's
            // chip, bounded by the window. It used to be a child of the bar and
            // hung 81 pixels below it, which is an escape and was invisible
            // until the ink gate reached this screen. A sibling here also puts
            // it over everything it opens across, which a child of one bar can
            // never be.
            if state.preset_open.get() && spec::shows_board_chrome(here.key.as_ref()) {
                preset_menu_scene(&state, palette)
            } else {
                Scene::Container(ContainerNode::new(Vec::new()))
            },
            // ★★★★★ R1762 — an open value roster, for the same reason and in
            // the same place: over everything, in window space, bounded by the
            // page it must not leave. Painted after the page so a press on it
            // resolves to the roster rather than to whatever row it covers.
            settings_roster_scene(&state, here.key.as_ref()),
            label(HELP_STRIP, help_strip_rect(), FONT_SMALL, palette.muted),
        ])
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
            },
        ));
    }
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
        // ★★★★★ R1733 — the preview is the DRAG's, so the rectangle drawn here
        // and the cell a release commits to are the same value read twice
        // rather than two derivations that agree today. And it answers for a
        // palette footprint the same way it answers for a card, which is what
        // made the new gesture two lines here instead of a second painter.
        if let Some(ghost) = drag.preview(&board) {
            canvas_children.push(carry_slot_scene(&ghost, palette));
        }
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
    for float in state.floats_front_to_back().iter().rev() {
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
        let cursor = state
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
            });
        Some(AccessFocus::addressing(stop, cursor))
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
            root = root.with_child("shell.palette");
        }
        let mut nodes = vec![root.with_child("shell.toast")];
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
        nodes
    }
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
    for (n, name) in TABS.iter().enumerate() {
        let tag = if n == 0 {
            BarChip::Tab0.tag()
        } else {
            BarChip::Tab1.tag()
        };
        tabs = tabs.with_child(tag);
        nodes.push(
            AccessNode::new(tag, AriaRole::Tab)
                .with_name(*name)
                .with_selected(current == *name)
                .with_set_position(n, TABS.len()),
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
    for card in &cards {
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
    let open = picking.as_ref();
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
    let mut region = AccessNode::new(format!("card.{id}"), AriaRole::Group)
        .with_name(card.title())
        .with_value(AccessValue::Text(announce))
        .with_state(AccessState::default())
        .with_child(format!("card.{id}.grip"));
    let mut nodes = vec![
        AccessNode::new(format!("card.{id}.grip"), AriaRole::Button)
            .with_name(format!("Move {}", card.title())),
    ];
    for control in spec::CARD_CHROME {
        let tag = format!("card.{id}.{control}");
        region = region.with_child(tag.clone());
        nodes.push(
            AccessNode::new(tag, AriaRole::Button).with_name(match *control {
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
            }),
        );
    }
    let body = match def_for_card(id).map(|def| def.kind) {
        Some("packet") => stream_nodes(id),
        Some("decode") => decode_nodes(id),
        Some("keymap") => map_nodes(id),
        Some("filter") => filter_nodes(state, id),
        Some("latency") => latency_nodes(id),
        _ => Vec::new(),
    };
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
    nodes.insert(0, region);
    nodes
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

/// The message stream, as a **grid**: a header row of column headers, then one
/// row per message holding one cell per column.
///
/// This is the shape a model-driven item view builds for itself at the floor —
/// measured at 6.11.1, its cell query answers the cell's name, its row, its
/// column and its column header — and the shape a hand-painted table has to
/// build or it has none at all.
fn stream_nodes(id: &str) -> Vec<AccessNode> {
    let rows: Vec<Vec<String>> = spec::STREAM_ROWS
        .iter()
        .map(|(time, kind, name, len)| {
            vec![
                (*time).to_owned(),
                (*kind).to_owned(),
                (*name).to_owned(),
                (*len).to_owned(),
            ]
        })
        .collect();
    table_nodes(id, "Message stream", spec::STREAM_COLUMNS.len(), &rows)
}

/// The identifier map, as a grid on the same shape as the stream.
///
/// ★ The unresolved row's timestamp is painted as an em dash, which is the
/// typographic stand-in for a value that is not knowable — and to somebody
/// reading rather than looking it is a punctuation mark. The cell announces the
/// meaning instead, which the voice census is what asked for: a name with no
/// word in it is a hole.
fn map_nodes(id: &str) -> Vec<AccessNode> {
    let rows: Vec<Vec<String>> = spec::MAP_ROWS
        .iter()
        .map(|(key, path, seen)| {
            let when = if seen.chars().all(|c| !c.is_alphanumeric()) {
                "not known".to_owned()
            } else {
                (*seen).to_owned()
            };
            vec![(*key).to_owned(), (*path).to_owned(), when]
        })
        .collect();
    table_nodes(id, "Identifier map", spec::MAP_COLUMNS.len(), &rows)
}

/// A card body that is a table.
///
/// ★★★★★ Built by [`grid_table_nodes`] rather than by hand. The first draft of
/// this screen hand-rolled the shape — as the sibling capture screen already
/// did — and the two disagreed about where the header row sits: WAI-ARIA counts
/// it in `aria-rowcount`, so it has to be counted in `aria-rowindex` too, and a
/// tree that counts it in one and not the other leaves its header unplaced and
/// its last row unreachable. The rule now lives once, in the builder, where a
/// third table cannot re-derive it differently.
///
/// The column headers are deliberately left unnamed here: they are painted with
/// their own tags, so the name comes from the paint and the two cannot drift.
fn table_nodes(id: &str, name: &str, columns: usize, rows: &[Vec<String>]) -> Vec<AccessNode> {
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
    grid_table_nodes(
        &format!("card.{id}.grid"),
        name,
        false,
        &format!("card.{id}.head"),
        &grid_columns,
        &grid_rows,
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
fn decode_nodes(id: &str) -> Vec<AccessNode> {
    let mut tree = AccessNode::new(format!("card.{id}.tree"), AriaRole::Tree)
        .with_name("Decoded layers")
        .with_size_of_set(u32::try_from(spec::DECODE_ROWS.len()).unwrap_or(u32::MAX));
    let mut nodes = Vec::new();
    for (n, (depth, key, value)) in spec::DECODE_ROWS.iter().enumerate() {
        let tag = format!("card.{id}.tree.{n}");
        tree = tree.with_child(tag.clone());
        let (place, siblings) = sibling_place(n);
        let mut item = AccessNode::new(tag, AriaRole::TreeItem)
            .with_name(*key)
            .with_level(*depth + 1)
            .with_set_position(place, siblings)
            .with_selected(n == spec::DECODE_SELECTED);
        if !value.is_empty() {
            item = item.with_value(AccessValue::Text((*value).to_owned()));
        }
        // A layer heading is what folds; a field under it does not.
        if *depth == 0 {
            item = item.with_expanded(true);
        }
        nodes.push(item);
    }
    nodes.insert(0, tree);
    nodes.extend(byte_nodes(id));
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
fn byte_nodes(id: &str) -> Vec<AccessNode> {
    let per_line = 4;
    let lines = spec::DECODE_BYTES.len();
    let mut grid = AccessNode::new(format!("card.{id}.bytegrid"), AriaRole::Grid)
        .with_name("Captured bytes")
        .with_row_count(u32::try_from(lines).unwrap_or(u32::MAX))
        .with_column_count(u32::try_from(per_line).unwrap_or(u32::MAX));
    let mut nodes = Vec::new();
    let (from, to) = spec::DECODE_SELECTED_SPAN;
    for (line, bytes) in spec::DECODE_BYTES.iter().enumerate() {
        let row_tag = format!("card.{id}.bytes.{line}");
        grid = grid.with_child(row_tag.clone());
        // The row is named by the offset it starts at, which is what the strip
        // paints in its left column and what a reader counts from.
        let mut row = AccessNode::new(row_tag, AriaRole::Row)
            .with_name(format!("{:04x}", line * per_line))
            .with_row(line);
        for (column, byte) in bytes.iter().enumerate() {
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
fn filter_nodes(state: &ShellState, id: &str) -> Vec<AccessNode> {
    let mut nodes = vec![
        AccessNode::new(format!("card.{id}.query"), AriaRole::TextInput)
            .with_name("Query")
            .with_value(AccessValue::Text(spec::FILTER_QUERY.to_owned())),
    ];
    // ★★★★★ R1721 — the bar's whole subtree is its rule's. It used to be five
    // `button`s with `aria-pressed`, hand-written here, over a set that can never
    // have two on — and nothing at all could change one of them. `spec::FILTER_ROW`
    // is the declaration; this call is the only thing that reads it into a tree.
    let chips =
        pinion_a11y::chip_group_nodes(&filter_row(state, id), focus_state::focused().as_deref());
    let mut counts =
        AccessNode::new(format!("card.{id}.counts"), AriaRole::Group).with_name("Match counts");
    for (n, (value, what)) in spec::FILTER_STATS.iter().enumerate() {
        let tag = format!("card.{id}.stat.{n}");
        counts = counts.with_child(tag.clone());
        // The word is the name and the number is the value: a reader told only
        // "12,418" has been told which of three numbers it is by position, and
        // position is exactly what somebody not looking at the card does not
        // have.
        nodes.push(
            AccessNode::new(tag, AriaRole::Status)
                .with_name(*what)
                .with_value(AccessValue::Text((*value).to_owned())),
        );
    }
    nodes.push(
        AccessNode::new(format!("card.{id}.sparkline"), AriaRole::Group)
            .with_name("Matched over time")
            .with_value(AccessValue::Text(series_reading(&MATCH_SERIES))),
    );
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
fn latency_nodes(id: &str) -> Vec<AccessNode> {
    let Some((binned, quantiles)) = latency_binned() else {
        return Vec::new();
    };
    let mut nodes = Vec::new();
    let tiles = latency_stats(&quantiles);
    let strip = format!("card.{id}.tiles");
    let mut group = AccessNode::new(strip.clone(), AriaRole::Group).with_name("Round trip");
    for (n, (key, value)) in tiles.iter().enumerate() {
        let tag = format!("card.{id}.stat.{n}");
        group = group.with_child(tag.clone());
        nodes.push(
            AccessNode::new(tag, AriaRole::Status)
                .with_name(*key)
                .with_value(AccessValue::Text(value.clone())),
        );
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
    nodes.push(
        AccessNode::new(format!("card.{id}.caption"), AriaRole::Status)
            .with_name("About this chart")
            .with_value(AccessValue::Text(spec::LATENCY_CAPTION.to_owned())),
    );
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
