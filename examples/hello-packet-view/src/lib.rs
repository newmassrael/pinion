// R1412 §5.49 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-packet-view` — R1663 §5.27 §5.40 §5.41 — the analysis tool's
//! **capture viewer**, screen B, assembled as one application against a
//! written-down specification of the reference screen.
//!
//! ## What forced this example
//!
//! `docs/analyzer-census.json` carried `capture.t0.3` — *bidirectional
//! highlight between a field and its bytes* — as a `have`, covered by the hex
//! dump plus `scene/marks`. Driven on the wire, neither surface holds the
//! relation: the dissection External publishes thirteen paths and not one names
//! a byte; the hex External publishes seventeen and not one names a field. The
//! verdict was covering the byte↔*cell* pair, which is a different pair.
//!
//! [`pinion_core::widgets::field_bytes`] is the relation that was missing, and
//! this screen is the consumer that proves it composes: the decode tree, the
//! byte pane's highlight and the layer chain a reader sees are all derived from
//! **one** [`ByteMap`], so no two of them can disagree.
//!
//! ## The screen
//!
//! ```text
//! cargo run -p hello-packet-view --release
//! ```
//!
//! A filter bar over the always-visible session context, a three-pane body —
//! the message list, the layered decode, the bytes — and a reassembly strip
//! along the bottom. Click a message to decode it, click a decode row to light
//! its bytes, click a byte to select the field that owns it, click a layer
//! heading to fold it, click a saved filter to apply it.
//!
//! Every rectangle comes from the helpers above `Hit::at`, read by both the
//! painter and the hit test, and the sweep in `painted.rs` presses the centre
//! of every painted control to keep it that way.
//!
//! See `tools/demos/r1663_a_field_says_which_bytes.py`.

mod judge;
mod spec;

#[cfg(test)]
mod painted;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::rc::Rc;

use pinion_a11y::{
    AccessFocus, AccessLive, AccessNode, AccessValue, AriaRole, GridCell, GridColumn, GridRow,
    SortDirection, WidgetA11y, grid_table_nodes,
};
use pinion_core::containment::line_rect_in;
use pinion_core::describe::{Descriptions, Resting};
use pinion_core::external::{
    ArgForm, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, ObjectArgs, PointerTarget,
    ReadRefusal, RepaintOwner, SchemaArg, SchemaField, ThreadOwnership, one_of_phrase,
};
use pinion_core::focus_state;
use pinion_core::input::PointerReading;
use pinion_core::pane_row::{Pane, PaneRow};
use pinion_core::reactive::Signal;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::shrink::ShrinkPolicy;
use pinion_core::style::{Border, BoxStyle, Color, LayoutStyle, Size, TextOverflow, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::utterance::{Announced, Tone, Utterance};
use pinion_core::voice::Silence;
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::chip_group::{Chip, ChipGroup};
use pinion_core::widgets::field_bytes::{
    ByteExtent, ByteMap, ByteMapExternal, ByteMapState, ByteSource, Coverage, FieldSpan, SourceId,
    use_byte_map,
};
use pinion_core::widgets::grid_sort::{Admission, col_sort_dir, grid_sort_parse, grid_sort_str};
use pinion_core::widgets::hex_dump::{ByteSelection, HexLayout};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::roving::{Activation, Axis, Ends, Landing, Member, Roving, RovingSpec};
use pinion_core::widgets::row_query::RowQuery;
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::table::{cell_cmp, cycle_col_sort, grid_order_by};
use pinion_core::widgets::table_export;
use pinion_core::widgets::text_edit::{TextEditState, use_text_edit_state};
use pinion_core::widgets::text_field::TextFieldState;
use pinion_core::{CellKind, Frame, Scene, WidgetCore, edit_field_keymap};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::pane::{PanePointer, scroll_pane};
use pinion_widget_paint::run::text_run;
use pinion_widget_paint::text_field as tf_paint;

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloPacketViewRenderer, HelloPacketViewRendererError);

// ── Geometry ────────────────────────────────────────────────────────────────

const WIN_W: u32 = 1440;
const WIN_H: u32 = 900;
const VIEW_TAG: &str = "packet_view";
const MAP_TAG: &str = "pv.map";
/// R1707 — the query box: the tag the field's own external is addressed by, the
/// tag its buffer is keyed on, and the tag it is painted under. One name.
const QUERY_TAG: &str = "pv.filter.query";
const THEME_TAG: &str = "app";

const APP_BAR_H: u32 = spec::APP_BAR_H;
const FILTER_H: u32 = spec::FILTER_H;
const CONTEXT_H: u32 = spec::CONTEXT_H;
const REASSEMBLY_H: u32 = spec::REASSEMBLY_H;
/// The decode pane's **design** width — what the specification draws it at, and
/// what it keeps whenever the window has room for it.
///
/// ⚠ Not a floor. [`TREE_FLOOR`] is, and the two were the same number until
/// R1860 measured what that cost.
const TREE_W: u32 = spec::PANES[1].width;
/// The byte pane's **design** width, on the same terms as [`TREE_W`].
const BYTES_W: u32 = spec::PANES[2].width;

const PAD: u32 = 12;

/// R1778 — how long this screen's message stays, in seconds. The reference's
/// own number, and the same one its two sibling screens use.
const TOAST_SECONDS: f32 = 2.6;

/// R1707 — how wide the query box is. Wide enough for the reference's own
/// three-clause query at this face, which is the longest thing the screen ever
/// puts in it.
const QUERY_W: u32 = 460;
/// R1707 — how tall it is, inside the bar's 46.
const QUERY_H: u32 = 26;
const ROW_H: u32 = 22;
const HEAD_H: u32 = 24;
const FONT_TITLE: u32 = 14;
const FONT_SMALL: u32 = 11;
const FONT_MONO: u32 = 11;

/// The byte grid's cell size. Every column of the dump is a multiple of it, so
/// the painter and the hit test index the same lattice.
const CELL_W: u32 = 8;
const CELL_H: u32 = 18;

/// The byte grid's column arithmetic. `const`, so [`BYTES_FLOOR`] is derived
/// from the same lattice the painter and the hit test index.
const HEX: HexLayout = HexLayout::new(spec::SOURCES[0].1).with_bytes_per_row(spec::BYTES_PER_ROW);

/// The width the specification's fixed columns need, summed from the
/// specification rather than copied out of it.
const fn fixed_columns() -> u32 {
    let mut total = 0;
    let mut i = 0;
    while i < spec::COLUMNS.len() {
        total += spec::COLUMNS[i].width;
        i += 1;
    }
    total
}

/// How many characters `text` has, at compile time.
///
/// UTF-8 continuation bytes are the ones matching `0b10xxxxxx`; every other byte
/// starts a character. Written out because `str::chars` is not `const`, and the
/// floor below has to be **derived** from the strings rather than measured once
/// by hand — which is the whole lesson [`NAME_FLOOR`] records.
const fn char_count(text: &str) -> u32 {
    let bytes = text.as_bytes();
    let mut count = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] & 0b1100_0000 != 0b1000_0000 {
            count += 1;
        }
        i += 1;
    }
    count
}

/// A grid column index as a `u32`, in a `const` context.
///
/// `u32::try_from` is not `const`, so the bound is *asserted* — and in the
/// `const` context a floor is declared in, a failed assertion is a **compile
/// error**, which is the same refusal `try_from` would give at run time. The
/// cast below cannot truncate because the line above it says so.
#[allow(clippy::cast_possible_truncation)]
const fn columns(n: usize) -> u32 {
    assert!(
        n <= 0xffff_ffff,
        "a byte grid with more columns than a u32 can index has no pixel width"
    );
    n as u32
}

/// The width a small run of `text` occupies. The `const` half of [`run_box`],
/// so a floor derived from it and the box actually painted cannot disagree.
const fn run_width(text: &str) -> u32 {
    char_count(text) * (FONT_SMALL - 4) + 10
}

/// The width one message's annotations take out of the name column: the note,
/// and the fragment marker with the gap that separates it.
const fn annotations_width(rows: &[spec::RowSpec], n: usize) -> u32 {
    let row = &rows[n];
    // ★★★★★ R1827 — the link annotation is counted per ROW and not as an upper
    // bound over the capture, which is what makes it free here: the two rows in
    // an exchange carry no note and no fragment, so the widest annotated row is
    // still the reassembled one and [`NAME_FLOOR`] does not move. A bound
    // instead — the widest sequence number, charged to every row — would add it
    // to that row too and push this screen's minimum past the size it opens in.
    let mut width = link_width(rows, n);
    if !row.note.is_empty() {
        width += run_width(row.note);
    }
    if let Some(fragment) = &row.fragment {
        // The marker is painted as `{marker} {piece}`, and the 8 is the gap the
        // painter subtracts before placing it.
        width += char_count(fragment.marker) * (FONT_SMALL - 4)
            + char_count(fragment.piece) * (FONT_SMALL - 4)
            + (FONT_SMALL - 4)
            + 10
            + 8;
    }
    width
}

/// The widest annotation load any message in the capture puts on the name
/// column.
const fn widest_annotations() -> u32 {
    let mut widest = 0;
    let mut i = 0;
    while i < spec::ROWS.len() {
        let width = annotations_width(spec::ROWS, i);
        if width > widest {
            widest = width;
        }
        i += 1;
    }
    widest
}

/// The narrowest the resource name itself may be painted — about seven
/// characters and an ellipsis, which is what the design size gives it today.
const NAME_RUN_FLOOR: u32 = 60;

/// The narrowest the flexible `name` column may be and still show a resource
/// path rather than an ellipsis alone.
///
/// ★★★★★ R1693 — this was `180`, a number somebody picked, and the per-cell
/// tagging the accessibility round added is what finally failed on it: at the
/// declared floor the annotations of the reassembled message
/// (`Last 3/3` + `reassembled 3,144 B`) are wider than the whole column, so the
/// resource name was painted at **zero width** and simply was not there. A
/// sighted reader saw a message with no name; nothing failed, because nothing
/// had ever asked whether that cell was painted.
///
/// It is the same defect the comment on [`LIST_FLOOR`] already recorded one
/// level up — a floor somebody picks can be wrong about the thing it is a floor
/// FOR — and the same repair: derive it from what the column has to hold.
const NAME_FLOOR: u32 = widest_annotations() + NAME_RUN_FLOOR;

/// The list pane's floor, **derived** from the columns it has to show.
///
/// ★ This was a round number (420) and the sweep's first run failed on it: the
/// fixed columns alone need more than that, so at the declared floor the
/// flexible column collapsed to zero width and its header stopped being
/// painted at all. A floor somebody picks is a floor that can be wrong about
/// the thing it is a floor FOR — the same shape as R1662's `MIN_H`, one level
/// down.
const LIST_FLOOR: u32 = fixed_columns() + NAME_FLOOR + PAD * 2;

/// The narrowest a decode row's value may be painted and still be a value
/// rather than an ellipsis — the decode pane's [`NAME_RUN_FLOOR`].
const VALUE_RUN_FLOOR: u32 = 60;

/// How far one level of the decode tree indents a row.
const TREE_INDENT_STEP: u32 = 14;
/// How deep a decode row can be indented.
///
/// **Derived, not measured**: `visible_fields` computes a row's depth as
/// `usize::from(path.contains('.'))`, so no row can be deeper than one however
/// the capture is decoded. `r1860_no_decode_row_is_deeper_than_the_floor_
/// assumes` is what holds that, because the premise of a derivation is a claim
/// like any other.
const TREE_MAX_DEPTH: u32 = 1;
/// Where a decode row's value begins, measured from the row's indent.
const TREE_VALUE_X: u32 = 140;
/// The gap a decode row's value keeps between itself and whatever is placed
/// from the right edge.
const TREE_VALUE_TAIL: u32 = 8;

/// The decode pane's floor, **derived** from the widest row it has to lay out:
/// the deepest indent, the value's own floor, and the badge that is placed from
/// the right edge before the value takes what is left.
///
/// ★ R1827's rule, applied one pane over: the badge is charged because a row
/// that carries one is a row this pane lays out, not because some upper bound
/// over the capture says it might be.
const TREE_FLOOR: u32 = PAD
    + TREE_INDENT_STEP * TREE_MAX_DEPTH
    + TREE_VALUE_X
    + TREE_VALUE_TAIL
    + VALUE_RUN_FLOOR
    + run_width("derived")
    + PAD;

/// The byte pane's floor, **derived** from the lattice it draws: the last hex
/// pair of a row has to end inside the pane.
///
/// Everything else in the pane is elastic — the title, the offset headers and
/// the span readout all sit left of this or take what is left — so the grid is
/// what the floor is a floor FOR.
const BYTES_FLOOR: u32 =
    PAD + columns(HEX.hex_col(spec::BYTES_PER_ROW - 1)) * CELL_W + CELL_W * 2 + PAD;

/// The two fixed panes, each declaring the width it is drawn at and the width
/// below which it cannot draw what it holds.
const SIDE_PANES: &[Pane] = &[
    Pane::new(TREE_W, TREE_FLOOR),
    Pane::new(BYTES_W, BYTES_FLOOR),
];

/// The three panes as a row: the message list is the flexible one and takes
/// what is left.
///
/// Every width this screen has an opinion about comes out of here — the design
/// arrangement, the declared minimum, and the widths of the panes themselves —
/// so the width the screen tells the window it lays out in and the widths its
/// panes can actually draw in cannot become two different claims.
const PANES: PaneRow = PaneRow::new(LIST_FLOOR, SIDE_PANES);

/// The narrowest window this screen will lay out in: **every pane at its own
/// floor**.
///
/// ★★★★★ R1860 — this was `LIST_FLOOR + TREE_W + BYTES_W`, which is one derived
/// floor plus two *design widths standing in for floors*. The two panes had no
/// floor at all; the width the specification draws them at was doing that job,
/// and a design width is not a floor for the same reason [`NAME_FLOOR`] and
/// [`LIST_FLOOR`] each record one level down — **a floor somebody picks can be
/// wrong about the thing it is a floor FOR**. Here it was wrong by 73 pixels,
/// and 37 of them were the ones a reader saw: the application this screen is a
/// page of grants it 1388, the screen declared it lays out at 1425, so it laid
/// out at 1425 inside a window 1440 wide and the third reassembly lane's right
/// outline was painted at 1455 and cut.
///
/// ★ R1662's lesson still holds and is why this is a sum of the *panes* rather
/// than a number somebody measured on a screen that happened to fit.
const MIN_W: u32 = PANES.floor();
/// The shortest window this screen will lay out in — the chrome, plus room for
/// four message rows, plus the reassembly strip.
const MIN_H: u32 = APP_BAR_H + FILTER_H + CONTEXT_H + HEAD_H + ROW_H * 4 + REASSEMBLY_H;

/// R1712 — this screen's two floors, which are one size.
///
/// [`ShrinkPolicy::rigid`] is the honest spelling of "the window stops where
/// the layout stops": a declaration somebody made, not a default nobody
/// examined. Both readers below take their number from here.
const SHRINK: ShrinkPolicy = ShrinkPolicy::rigid((MIN_W, MIN_H));

/// The live surface, or the design size where no shell has published one.
///
/// ★★★★★ R1700 — **the framework's answer, and the same one on both halves of
/// this screen.** This fell back to `(WIN_W, WIN_H)` off an owner scope, and
/// every pointer handler and every wire action on this screen runs off one: the
/// paint reflowed to the live window while the hit test went on resolving
/// against 1440x900. Measured before the repair, through a real shell at
/// 2494x1011 — of the 166 painted rectangles that moved, **166** had stopped
/// being pressable where they were drawn. Reported by a person twice, and green
/// in every gate both times, because an in-process fixture paints and hit-tests
/// inside one owner scope where the two halves cannot disagree.
///
/// The sibling screens had already met this and answered it two different ways
/// — one read the framework's record, one kept a `Signal` of its own — so the
/// policy was lifted into [`pinion_core::external::layout_size`] rather than
/// spelled here a third time.
fn window_size() -> (u32, u32) {
    pinion_core::external::layout_size(VIEW_TAG, SHRINK.comfortable(), (WIN_W, WIN_H))
}

fn body_rect() -> Rect {
    let (w, h) = window_size();
    let top = APP_BAR_H + FILTER_H + CONTEXT_H;
    Rect::new(0, top, w, h.saturating_sub(top + REASSEMBLY_H))
}

/// How wide the three panes are in the window this screen was given: the list,
/// the decode tree, the byte grid.
///
/// ★★★★★ R1860 — **the two side panes flex, and they did not before.** The list
/// was the one flexible pane and the other two were their design widths at every
/// size, so a window narrower than the design arrangement could only be served
/// by painting past its edge.
///
/// The rule itself is the framework's ([`pinion_core::pane_row`]) rather than
/// this screen's, because "a row of panes, each declaring the width below which
/// it cannot draw what it holds" is not a fact about capture viewers — two more
/// screens in this workspace put a fixed detail pane beside a flexible one and
/// declare their minimum the same way.
fn pane_widths() -> (u32, u32, u32) {
    let (list, fixed) = PANES.share(body_rect().w);
    (list, fixed[0], fixed[1])
}

/// The width below which the `n`-th pane of [`spec::PANES`] cannot draw what it
/// holds.
///
/// The message list is the flexible pane and index 0 of both tables; the two
/// after it are [`SIDE_PANES`] in the same order, so a consumer reads the number
/// the layout itself uses rather than one written down beside it.
fn pane_floor(n: usize) -> u32 {
    match n.checked_sub(1) {
        None => LIST_FLOOR,
        Some(fixed) => SIDE_PANES[fixed].floor(),
    }
}

fn list_rect() -> Rect {
    let body = body_rect();
    Rect::new(body.x, body.y, pane_widths().0, body.h)
}

fn tree_rect() -> Rect {
    let body = body_rect();
    let (list, tree, _) = pane_widths();
    Rect::new(list, body.y, tree, body.h)
}

fn bytes_rect() -> Rect {
    let body = body_rect();
    let (list, tree, bytes) = pane_widths();
    Rect::new(list + tree, body.y, bytes, body.h)
}

fn filter_rect() -> Rect {
    Rect::new(0, APP_BAR_H, window_size().0, FILTER_H)
}

fn context_rect() -> Rect {
    Rect::new(0, APP_BAR_H + FILTER_H, window_size().0, CONTEXT_H)
}

fn reassembly_rect() -> Rect {
    let (w, h) = window_size();
    Rect::new(0, h - REASSEMBLY_H, w, REASSEMBLY_H)
}

// ── Ink ─────────────────────────────────────────────────────────────────────

const fn rgb(hex: u32) -> Color {
    Color::rgb(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

/// The inks this screen paints with, resolved from the theme once per frame.
#[derive(Clone, Copy)]
struct Ink {
    bg: Color,
    surface: Color,
    outline: Color,
    text: Color,
    text_2: Color,
    text_3: Color,
    accent: Color,
    warn: Color,
    err: Color,
    /// R2012 — the tone for a fact that is neither right nor wrong.
    ///
    /// ⚠ Taken from the theme's DARK palette, and the reason is measured
    /// rather than stylistic: this screen paints its own near-black chrome
    /// (`bg` below) whatever the theme is, so a light-palette state tone lands
    /// on a ground it was never chosen against. Measured on `bg`, the light
    /// palette's `warning` reads **2.80** and its `error` **2.93**, both under
    /// the 3.0 non-text floor, against **11.17** and **11.22** for the dark
    /// palette's — so `warn` and `err` beside this field are legible only when
    /// the reader happens to have chosen the dark theme. This field does not
    /// join them in that ⇒
    /// [[debt-a-screen-with-its-own-chrome-reads-state-tones-from-the-readers-palette]].
    info: Color,
    ok: Color,
    lit: Color,
}

fn ink(theme: &Theme) -> Ink {
    Ink {
        bg: rgb(0x0E_0F12),
        surface: rgb(0x16_181D),
        outline: rgb(0x2A_2E36),
        text: rgb(0xE8_EBEF),
        text_2: rgb(0x98_A2AD),
        text_3: rgb(0x69_7180),
        accent: rgb(0xEC_5AA0),
        warn: theme.resolve(ColorRole::Warning),
        err: theme.resolve(ColorRole::Error),
        info: Theme::dark().resolve(ColorRole::Info),
        ok: rgb(0x35_C08B),
        lit: Color::rgba(0x9A, 0x00, 0x4F, 0x60),
    }
}

/// The ink a message class is drawn in — the reader's index into a list going
/// past, which is why the reference gives five classes five colours.
fn kind_ink(kind: &str) -> Color {
    match kind {
        "Data" => rgb(0x3d_8b_fd),
        "Query" => rgb(0xb0_69_d8),
        "Response" => rgb(0x2e_a0_67),
        "Declare" => rgb(0xd1_8b_1f),
        _ => rgb(0x77_82_8c),
    }
}

// ── State ───────────────────────────────────────────────────────────────────

/// Everything the screen holds, and nothing it can derive.
struct ViewState {
    /// Which message of [`spec::ROWS`] is decoded.
    row: Signal<usize>,
    /// Which decode field is selected, by path.
    field: Signal<String>,
    /// ★★★★★ R1698 — which byte the hex grid's keyboard cursor rests on.
    ///
    /// **Not** derivable from [`field`](Self::field), and the round found that
    /// out by trying: what the grid LIGHTS is the selected field's extent, and
    /// a cursor computed from it cannot move within a field at all — every
    /// arrow snapped back to the field's first byte. Selection and cursor are
    /// two facts in every grid that has both (WAI-ARIA gives them
    /// `aria-selected` and `aria-activedescendant` for the same reason), and
    /// collapsing them is what made the byte grid's arrows a no-op.
    ///
    /// A press writes it too, so the pointer and the keyboard never disagree
    /// about where the cursor is.
    byte: Signal<usize>,
    /// ★★★★★ R1699 — which **cell** of the selected message row the reader has
    /// gone into, or `None` while the cursor is still on the row itself.
    ///
    /// This screen announces its message list as a `grid`, and WAI-ARIA's grid
    /// pattern is two axes: the vertical one moves between rows, the horizontal
    /// one between the cells of the row you are on. R1698 built the first and
    /// nothing measured the second. R1699 did, by driving the running screen:
    /// every row reported one cell per column to the accessibility tree, and
    /// `ArrowRight` standing on a row moved nothing — so the columns existed
    /// for a reader and were unreachable by one.
    ///
    /// ⚠ R1827 took two numbers out of that sentence ("the sixteen rows each
    /// report seven cells"). **Both are still true, and that is the reason
    /// rather than the objection**: this round built an eighth column, ran it
    /// into the paint gates, and abandoned it, so for the length of an
    /// afternoon this sentence asserted seven about a screen with eight and
    /// nothing here could have said so. A number that survives by having the
    /// change reverted is not a number that held. The arity lives in
    /// `spec::COLUMNS.len()` and is asserted by
    /// `r1693_the_name_cell_announces_the_annotations_painted_beside_it`.
    ///
    /// `Option` rather than a plain index because the descent is a fact and not
    /// a default: a reader who has not gone into a row should hear the row, and
    /// a grid that always addressed a cell could not say "the whole row", which
    /// is exactly what this list's selection means.
    cell: Signal<Option<usize>>,
    /// Which saved filters are on.
    ///
    /// ★★★★★ R1721 — **at most one of them ever is**, and until that round only
    /// [`toggle_saved`] knew: the vector was cleared by hand there while the
    /// accessibility tree announced three independent toggle buttons and this
    /// screen's own test file called them "independent switches". The rule is
    /// now `spec::SAVED_ROW`, declared once and read by [`saved_row`], and the
    /// roles, the Tab stops and the arrows are derived from it.
    saved: Signal<Vec<bool>>,
    /// ★★★★★ R1721 — where the saved-filter row's keyboard cursor rests.
    ///
    /// A second fact from [`saved`](Self::saved), for the reason the byte grid's
    /// cursor is a second fact from its selection: the row declares
    /// [`Activation::Explicit`], so a reader walks the chips *without* applying
    /// them and presses `Enter` on the one they want. Collapsing the two would
    /// make every arrow apply a filter, which is the behaviour
    /// [`Activation::Follows`] is for and the opposite of what a saved-filter
    /// bar should do.
    saved_cursor: Signal<usize>,
    /// ★★★★★ R1707 — the query the list is running, as the person wrote it.
    ///
    /// **This is the field's own buffer, not a copy of it.** The alternative —
    /// a `Signal<String>` beside the text field — is the two-copies shape this
    /// tree has paid for repeatedly, and here it would fail in the most visible
    /// way possible: the list would filter on the last committed query while
    /// the box showed the one being typed. Holding the buffer makes the filter
    /// live as the reference's is, and makes "what the bar shows" and "what the
    /// list runs" the same read.
    ///
    /// Empty means *keep everything*, which is what the screen opens with. The
    /// TEXT is what is held rather than a compiled predicate — the thing
    /// measured missing from the reference floor at 6.11.1, where a wildcard
    /// handed to the row-filtering proxy reads back as the compiled regular
    /// expression and the pattern a person typed is gone.
    query: Rc<TextEditState>,
    /// ★★★★★ R1829 — **which column the list is ordered by, and which way.**
    ///
    /// `None` is the capture's own order, which this capture writes NEWEST
    /// FIRST — so "unsorted" here is not "no opinion", it is the arrival order
    /// an analyser opens in. Following one exchange wants the opposite, and
    /// that difference is the whole capability (`capture.t1.11`, *follow one
    /// session, in time order*): a filter alone leaves the reply above the
    /// request it answers.
    ///
    /// ★ The shape is the framework's `(column, ascending)` and not a bespoke
    /// enum, because everything that consumes it is the framework's too —
    /// [`col_sort_dir`] for the header glyph and `aria-sort`, [`grid_sort_str`]
    /// / [`grid_sort_parse`] for the wire, [`cycle_col_sort`] for the header
    /// press, [`grid_order_by`] for the permutation. A screen that invented its
    /// own spelling would have to translate at four boundaries.
    ///
    /// ⚠ **What it deliberately does NOT do is adopt `GridSortState`.** That
    /// type owns the cells, the filter AND the sort; this screen's filter is
    /// [`RowQuery`], which parses column NAMES and keeps each clause's source
    /// text, and is strictly richer than the `GridFilter` `GridSortState`
    /// carries. Taking the whole object would put two filter models on one
    /// list — the defect this tree already carries in two other places — so
    /// what is taken is the ORDERING, which is published separately for
    /// exactly this reason.
    sort: Signal<Option<(usize, bool)>>,
    /// Which layers are folded, by index into [`spec::LAYERS`].
    folded: Signal<Vec<bool>>,
    /// The dissection of the selected message. **One** value: the tree, the
    /// byte highlight and the layer chain all read it.
    map: Rc<ByteMapState>,
    /// The scroll offsets of the three pane bodies.
    list_scroll: Rc<ScrollState>,
    tree_scroll: Rc<ScrollState>,
    bytes_scroll: Rc<ScrollState>,
    /// Where the cursor last was, because a press carries no coordinates.
    cursor: Signal<(u32, u32)>,
    /// ★★★★★ R1918 — whether anybody is pointing at this screen at all.
    ///
    /// A second signal beside [`cursor`](Self::cursor) and not a sentinel
    /// inside it: every gesture reading `cursor` wants *the last place the
    /// pointer was*, and only the hover derivations want *is anybody
    /// pointing*. A leave is not a move to somewhere else, so without this a
    /// description stays on the frame over a window nobody is pointing at.
    pointer_inside: Signal<bool>,
    /// The last thing the screen did, for the status line and the wire.
    ///
    /// ★★★★★ R1719 — an [`Utterance`], so the one fact downstream needs (was
    /// this a refusal?) is a value rather than a `"query refused: "` prefix.
    /// `None` is "has not said anything yet", the only spelling of it now.
    ///
    /// ★★★★★ R1778 — and a `Saying` rather than a `RefCell`, which is the half of
    /// this lift that only this screen needed. **A `RefCell` is not reactive.**
    /// Of the three screens that keep a status message, this was the one whose
    /// sentence nothing could observe changing, so a lifetime added here alone
    /// would have expired with nothing repainting — the screen would have
    /// looked unfixed while its code said otherwise.
    said: Rc<pinion_core::utterance::Saying>,
}

impl ViewState {
    /// Say something to the person in front of the screen.
    ///
    /// ★★★★★ R1719 — takes an utterance, so the live region's urgency comes
    /// off the tone. This screen was measured announcing everything politely,
    /// including a query it refused to run.
    fn say(&self, what: Utterance) {
        self.said.say(what);
    }

    /// What a person reads, or an empty string when nothing has been said.
    ///
    /// ★ R1778 — the holder's own method now. Three screens had written this
    /// same chain, which is what made it a thing to lift rather than a helper
    /// to copy a fourth time.
    fn said_sentence(&self) -> String {
        self.said.sentence()
    }

    /// ★★★ R1707 — the running query, parsed.
    ///
    /// A malformed query keeps everything rather than nothing. The choice is
    /// not arbitrary: a half-typed query is malformed on nearly every
    /// keystroke, and a screen that emptied its list while a person typed would
    /// flash the capture away and back. The refusal is not swallowed — it is
    /// what [`query_fault`](Self::query_fault) answers and what the bar paints.
    fn query(&self) -> RowQuery {
        RowQuery::parse(&self.query.text(), spec::QUERY_COLUMNS).unwrap_or_default()
    }

    /// Why the running query could not be understood, or `None` when it could.
    fn query_fault(&self) -> Option<String> {
        RowQuery::parse(&self.query.text(), spec::QUERY_COLUMNS)
            .err()
            .map(|e| e.to_string())
    }

    /// ★★★ R1707 — the source indices the query keeps, in capture order.
    ///
    /// The ONE derivation. The painter, the hit test, the keyboard, the
    /// accessibility tree and the wire all read this, so the list a person sees
    /// and the list a press lands in cannot be two lists — the failure this
    /// tree has paid for under several names.
    /// ★★★★★ R1829 — **the rows the query kept, in the order the reader asked
    /// for**, and it is ONE function because a screen with two of these has two
    /// answers to *which message is at the top*.
    ///
    /// Filter-then-sort, through [`grid_order_by`] — the framework's ordering
    /// SSOT, the same one the virtualised data grid runs on. Three things come
    /// with taking it rather than writing a `sort_by` here:
    ///
    /// * `sort == None` returns the survivors in **source order**, so the
    ///   screen's opening behaviour is unchanged by construction rather than by
    ///   a branch somebody has to keep correct;
    /// * the sort is **stable**, so equal keys hold their capture order in both
    ///   directions — which is what makes two messages sharing a timestamp
    ///   deterministic rather than merely usually-fine;
    /// * the comparison is [`cell_cmp`], which is numeric-aware.
    ///
    /// ⚠ **That last one is a trap worth naming, because it chooses silently.**
    /// `cell_cmp` sorts numerically when BOTH cells parse as `f64` and
    /// lexically otherwise, so `len` and `sn` sort as numbers (12 before 100)
    /// while `time` sorts as text. Text is chronological here only because
    /// every timestamp is fixed-width `HH:MM:SS.mmm` — which is not an
    /// assumption this screen is entitled to make quietly, and is why
    /// `r1827_a_timestamp_sorts_as_text_because_every_one_is_the_same_shape`
    /// exists. `r1829_ordering_by_time_is_chronological_and_by_length_numeric`
    /// asserts both branches on the real capture, because a comparator picked
    /// by a `parse` is one a reader cannot see.
    ///
    /// The cells compared are [`cell_texts`] — what the row PAINTS — so the
    /// order a reader sees is the order of what they are looking at, not of
    /// some parallel value the screen does not show.
    fn kept(&self) -> Vec<usize> {
        let query = self.query();
        let everything = query.is_everything();
        grid_order_by(
            spec::ROWS.len(),
            self.sort.get(),
            |col, a, b| {
                let (ca, cb) = (cell_texts(a), cell_texts(b));
                match (ca.get(col), cb.get(col)) {
                    (Some(x), Some(y)) => cell_cmp(x, y),
                    _ => core::cmp::Ordering::Equal,
                }
            },
            |n| {
                everything || {
                    let cells = spec::ROWS[n].attributes();
                    query.admit(|c| cells.get(c).map_or("", String::as_str)) == Admission::Admitted
                }
            },
        )
    }

    /// Which message the list's cursor is on: the selected one when the query
    /// kept it, else the first it did keep, else the selected one again (an
    /// empty result has no row to stand on and the roster is empty too).
    fn cursor_row(&self) -> usize {
        let selected = self.row.get();
        let kept = self.kept();
        if kept.contains(&selected) {
            return selected;
        }
        kept.first().copied().unwrap_or(selected)
    }

    /// Which clause hid source row `n`, or `None` when the row is shown.
    ///
    /// The question the reference floor answers with an invalid index and
    /// nothing else — the same answer for every reason a row can be absent.
    fn why_hidden(&self, n: usize) -> Option<String> {
        let query = self.query();
        let cells = spec::ROWS.get(n)?.attributes();
        query
            .rejecting_clause(|c| cells.get(c).map_or("", String::as_str))
            .map(|clause| clause.text.clone())
    }

    /// The bytes of the frame this screen shows. Deterministic and derived from
    /// the selected row, so a decode and its dump cannot come from different
    /// messages.
    fn frame_bytes(&self) -> Vec<u8> {
        frame_bytes(self.row.get())
    }

    /// The selected field's highlight over the frame, when it has one there.
    fn lit_selection(&self) -> Option<ByteSelection> {
        let map = self.map.map();
        match map.selection_for(&self.field.get()) {
            Ok((source, sel)) if source == SourceId::new(0) => Some(sel),
            _ => None,
        }
    }
}

/// The frame bytes of message `row` — a stable pseudo-capture, so the dump is
/// the same on every host and in every run.
///
/// ★★★★★ R1814 — and for the message the specification describes in full, the
/// bytes under a field that **declares its encoding** are that encoding, not
/// the filler. Before this the whole frame was a hash of the row index, so the
/// two bytes `sn` lit read `18 1c` — 6172 — while the tree beside them said
/// 3419. The relation between a field and its bytes was true and tested; the
/// *content* of those bytes was decoration, on the one screen whose subject is
/// field-to-byte fidelity.
///
/// Only leaves are written, and only where [`spec::Wire`] states an encoding.
/// A layer heading spans its children, so writing its extent would overwrite
/// them — `r1814_no_declared_field_spans_another` refuses that arrangement
/// rather than relying on the order of this loop.
fn frame_bytes(row: usize) -> Vec<u8> {
    let mut bytes = capture_filler(row, spec::SOURCES[0].1);
    if row != spec::OPENING_ROW {
        // Every other message's decode is the stand-in in `decode`, whose rows
        // state an extent rather than a value — so there is nothing to encode,
        // and pretending otherwise would be the same defect with a new face.
        return bytes;
    }
    for field in spec::FIELDS {
        if field.source != Some(0) {
            continue;
        }
        let Some(encoded) = field.wire.encode(field.len) else {
            continue;
        };
        if let Some(room) = bytes.get_mut(field.at..field.at + field.len) {
            room.copy_from_slice(&encoded);
        }
    }
    bytes
}

/// The capture a message's frame sits in: deterministic, host-independent, and
/// not a claim about any value.
///
/// ★ R1814 split this out of [`frame_bytes`] so the two facts are separable.
/// This is what a byte is when nothing has said what it holds; everything the
/// specification *can* say is written over it.
fn capture_filler(row: usize, len: usize) -> Vec<u8> {
    let seed = u32::try_from(row).unwrap_or(0).wrapping_mul(0x9e37) ^ 0x5a5a;
    (0..len)
        .map(|i| {
            let n = u32::try_from(i).unwrap_or(0);
            ((seed.wrapping_add(n.wrapping_mul(31)) >> 3) & 0xff) as u8
        })
        .collect()
}

fn use_view_state() -> Rc<ViewState> {
    // ★ [[owner-cache-no-nested-factory]] — every cached slot this one holds is
    // resolved BEFORE the factory runs, because `Owner::cache` cannot re-enter
    // itself and a factory that calls another `use_*` hook does exactly that.
    let map = use_byte_map("packet_view.map", || decode(spec::OPENING_ROW));
    let list_scroll = pinion_core::widgets::scroll::use_scroll_state("pv.list.body");
    let tree_scroll = pinion_core::widgets::scroll::use_scroll_state("pv.tree.body");
    let bytes_scroll = pinion_core::widgets::scroll::use_scroll_state("pv.bytes.body");
    // R1707 — the query field's own buffer, resolved out here for the same
    // reason as the four above. The first draft called the hook inside the
    // factory and every test in this example failed identically, which is the
    // rule doing its job: `Owner::cache` refuses to re-enter rather than
    // handing back a second buffer for the same tag.
    let query = use_text_edit_state(QUERY_TAG);
    let owner = pinion_core::reactive::Owner::current()
        .expect("use_view_state requires an active Owner scope");
    // ★★★★★ R1778 — the status holder is REGISTERED, because it is also what
    // the paint loop ticks, and resolved out here for the same reason as the
    // five above: a factory that calls another `use_*` hook re-enters
    // `Owner::cache`, which refuses.
    let said = owner.register_animation_once("packet_view.said", || {
        pinion_core::utterance::Saying::new(TOAST_SECONDS)
    });
    owner.cache("packet_view.state", || ViewState {
        row: Signal::new(spec::OPENING_ROW),
        field: Signal::new(spec::OPENING_FIELD.to_owned()),
        // The cursor opens on the first byte of the field the screen opens
        // with, so the two agree at boot and diverge only once somebody moves
        // one of them. Derived rather than written down, because a second
        // literal would be a second thing to keep in step with the field.
        byte: Signal::new(
            map.map()
                .extent_of(spec::OPENING_FIELD)
                .map_or(0, |(_, extent)| extent.at()),
        ),
        // R1699 — the screen opens on the row, not inside it. A grid that
        // started with a cell addressed would announce a column nobody chose.
        cell: Signal::new(None),
        saved: Signal::new(vec![false; spec::SAVED_FILTERS.len()]),
        saved_cursor: Signal::new(0),
        // R1707 — the field's own buffer, resolved above. The screen opens
        // unfiltered; see `spec::EXAMPLE_QUERY` for why the reference's own
        // query is a saved filter rather than the opening state.
        query,
        // R1829 — the screen opens in the capture's own order, which is the
        // arrival order the reference opens in. Ordering is something a reader
        // asks for.
        sort: Signal::new(None),
        folded: Signal::new(vec![false; spec::LAYERS.len()]),
        map,
        list_scroll,
        tree_scroll,
        bytes_scroll,
        cursor: Signal::new((0, 0)),
        pointer_inside: Signal::new(false),
        said,
    })
}

/// The example's decoder: the dissection of message `row`.
///
/// The reference states the decode of one message in full, and that table is
/// [`spec::FIELDS`] — reproduced verbatim for that row. Every other message
/// gets the same three layers built from the facts the row itself carries, so
/// selecting a different message produces a *real* dissection of a real length
/// rather than a screen that goes blank.
///
/// A row is reassembled when the specification says it is, and only then does
/// the map have a second source — which is the case the crate's source id
/// exists for.
fn decode(row: usize) -> ByteMap {
    if row == spec::OPENING_ROW {
        return spec_map();
    }
    let message = &spec::ROWS[row];
    let frame = SourceId::new(0);
    let frame_len = spec::SOURCES[0].1;
    // The message's own length decides how much of the frame the message layer
    // covers; the framing, transport and network headers are fixed.
    let message_layer = described_extent("l3");
    let body = (message.len as usize).clamp(4, frame_len - message_layer.at());
    // ★★★★★ R1747 — the network layer, which this stand-in decode did not have.
    //
    // Found by the conformance verdict rather than by reading the code: the
    // capture viewer's own specification says the tree names four layers, and
    // the context strip beside it says `low latency off · 4 layers` — and this
    // decode drew THREE for fifteen of the sixteen messages. The screen was
    // contradicting its own session context, and every check in this example
    // was green because the only decode anything ever asserted about was the
    // one message `spec::FIELDS` describes.
    //
    // ★★★★★ R2011 — the extents are now READ from the described decode instead
    // of written a second time, and the reason is that the second copy had
    // already drifted.
    //
    // The paragraph above said *the extent mirrors the described decode's*,
    // which is what made it a stand-in rather than an invention. Measured while
    // moving `l0.link`: it did not. `l0.stream` was six bytes here and four
    // there, so selecting the stream offset lit `0x06..0x0c` on fifteen
    // messages and `0x06..0x0a` on the sixteenth — one row, two extents,
    // depending only on which message a reader had open. Nothing could see it,
    // because every check in this crate asks about the described decode and
    // this list is the one place the described decode is not what answers.
    //
    // A sentence claiming two lists agree is not a gate. Reading one out of the
    // other is, and it also means the shift `l0.link` just caused arrives here
    // by construction rather than by somebody remembering.
    //
    // ★ The path list stays this function's own: a stand-in is deliberately
    // COARSER than the described decode (no `l0.batch`, no transport leaves
    // beyond the sequence number), and which rows it draws is a statement about
    // what can be known from a row's own facts. What it must not have is its
    // own idea of where those rows are.
    let spans = vec![
        FieldSpan::bytes("l0", frame, described_extent("l0")),
        FieldSpan::bytes("l0.link", frame, described_extent("l0.link")),
        FieldSpan::bytes("l0.stream", frame, described_extent("l0.stream")),
        FieldSpan::bytes("l1", frame, described_extent("l1")),
        FieldSpan::bytes("l1.sn", frame, described_extent("l1.sn")),
        FieldSpan::bytes("l2", frame, described_extent("l2")),
        FieldSpan::bytes("l3", frame, ByteExtent::new(message_layer.at(), body)),
        FieldSpan::bytes("l3.name_id", frame, described_extent("l3.name_id")),
        FieldSpan::derived("l3.resolved"),
    ];
    ByteMap::build(vec![ByteSource::new(spec::SOURCES[0].0, frame_len)], spans)
        .expect("the example's decoder produces a well-formed dissection")
}

/// Where the described decode puts `path`.
///
/// ★ Panics for a path the specification does not have, which is the point: the
/// stand-in decode names rows it expects the described one to define, and a
/// typo or a row removed from [`spec::FIELDS`] must stop this screen rather
/// than quietly produce a dissection with a hole in it.
fn described_extent(path: &str) -> ByteExtent {
    let field = spec::FIELDS
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("the described decode has no `{path}` to place"));
    ByteExtent::new(field.at, field.len)
}

/// The reference's own decode table, as a [`ByteMap`].
fn spec_map() -> ByteMap {
    let sources = spec::SOURCES
        .iter()
        .map(|(name, len)| ByteSource::new(*name, *len))
        .collect();
    let spans = spec::FIELDS
        .iter()
        .map(|f| match f.source {
            Some(s) => FieldSpan::bytes(f.path, SourceId::new(s), ByteExtent::new(f.at, f.len)),
            None => FieldSpan::derived(f.path),
        })
        .collect();
    ByteMap::build(sources, spans)
        .expect("the reference's decode table is a well-formed dissection")
}

/// The decode rows on screen: the specification's rows for the reference
/// message, and the decoder's own paths for every other one, minus whatever a
/// folded layer hides.
fn visible_fields(state: &ViewState) -> Vec<(String, String, String, usize)> {
    let map = state.map.map();
    let folded = state.folded.get();
    let described = state.row.get() == spec::OPENING_ROW;
    map.fields()
        .iter()
        .filter_map(|span| {
            let path = span.path().to_owned();
            let layer = spec::LAYERS.iter().position(|(id, _)| path.starts_with(id));
            if let Some(n) = layer
                && folded.get(n).copied().unwrap_or(false)
                && path.contains('.')
            {
                return None;
            }
            let (name, value) = if let Some(n) = layer.filter(|_| !path.contains('.')) {
                // ★★★★★ R1747 — a layer heading names its LAYER, whatever
                // message is open. Which layers a decode has is a fact about
                // the capture; what each one is called is a fact about the
                // protocol, and this line used to conflate them: for every
                // message but the one `spec::FIELDS` describes, the heading
                // fell through to the leaf of the path and a reader saw `l0`
                // where the reference draws the layer's name. Found by the
                // conformance verdict, which reads the words the frame drew.
                let value = if described {
                    spec::FIELDS
                        .iter()
                        .find(|f| f.path == path)
                        .map_or_else(String::new, spec::FieldSpecRow::shown_value)
                } else {
                    String::new()
                };
                (spec::LAYERS[n].1.to_owned(), value)
            } else if described {
                spec::FIELDS.iter().find(|f| f.path == path).map_or(
                    (path.clone(), String::new()),
                    |f| {
                        // ★★★★★ R2011 — a field whose DECLARATION determines
                        // its printed form is painted from the declaration.
                        // See `spec::FieldSpecRow::shown_value`: for an address
                        // the reader now sees the octets the byte pane lights,
                        // so the row and the highlight cannot say different
                        // things — and the External publishes the same call, so
                        // neither can the two channels.
                        (f.name.to_owned(), f.shown_value())
                    },
                )
            } else {
                let leaf = path.rsplit('.').next().unwrap_or(&path).to_owned();
                let value = map.extent_of(&path).map_or_else(
                    || "derived".to_owned(),
                    |(_, e)| format!("{} B at 0x{:02x}", e.len(), e.at()),
                );
                (leaf, value)
            };
            let depth = usize::from(path.contains('.'));
            Some((path, name, value, depth))
        })
        .collect()
}

// ── Paint helpers ───────────────────────────────────────────────────────────

/// ★★★★★ R1664 — and the pointer declaration is load-bearing, exactly as it is
/// on this screen's two siblings.
///
/// This screen routes every press to one root `External` and addresses its parts
/// by tag. Those two facts fight: the §5.35 router resolves the **deepest tagged
/// node** under the cursor and looks its primary half up as an `External`, so
/// every tag painted for addressing shadows the root and the press is dropped
/// without a word. `scene/click {path}`, `scene/invoke` and `send` all keep
/// working, because they call the handler by name and never ask the router —
/// which is why R1663 shipped this screen with 11 integration tests, a
/// 160-assertion demo and a boot gate all green, and a person opening the window
/// found that pressing anything did nothing at all.
///
/// `hello-node-lab` and `hello-analyzer-shell` both carry this line, and
/// `hello-analyzer-shell` carries R1649.1's account of learning it. This file
/// was written after both and without it.
///
/// The root container is the one node that keeps its own layout, so it stays the
/// target; everything built through this helper is an address.
fn absolute(rect: Rect) -> LayoutStyle {
    LayoutStyle::decoration(rect)
}

/// The style every run on this screen carries.
///
/// ★ R1654 — including an overflow policy. A resource name, a hop and a
/// timestamp are all captured data, so a run wider than the box it was given
/// would wrap onto the row below. `Ellipsis` rather than `Clip`, because a hard
/// cut leaves no evidence that anything was removed.
fn run_style(px: u32, fg: Color) -> TextStyle {
    TextStyle::new()
        .with_size_px(px)
        .with_fg(fg)
        .with_overflow(TextOverflow::Ellipsis)
}

fn label(text: impl Into<String>, rect: Rect, px: u32, fg: Color) -> Scene {
    Scene::Text(TextNode::styled(text.into(), rect, run_style(px, fg)).with_layout(absolute(rect)))
}

/// A run that can be addressed — the lifted
/// [`text_run`], which is where the
/// rectangle-used-twice and pointer-transparency decisions now live (R1694,
/// the third identical copy).
fn tagged_label(tag: &str, text: impl Into<String>, rect: Rect, px: u32, fg: Color) -> Scene {
    text_run(tag, text, rect, run_style(px, fg))
}

/// A bordered panel's CONTENT rectangle in its own space: its box less the 1px
/// frame [`panel`] draws inside it.
///
/// ★ R1672 — the placement half of
/// [`pinion_core::containment::content_rect`], which is the check half. A pane
/// that handed its scrolling body `(0, 0, rect.w, rect.h)` put the body over
/// its own outline, and the channel could not say so until it learned the
/// border-box / content-box distinction. Named here so the two halves cannot
/// drift: change the frame's width and both follow.
/// The width of the outline [`panel`] strokes INSIDE its box.
const PANEL_FRAME: u32 = 1;

fn panel_content(rect: Rect) -> Rect {
    pinion_core::containment::content_of(
        Rect::new(0, 0, rect.w, rect.h),
        Some(&Border::new(Color::rgba(0, 0, 0, 0), PANEL_FRAME)),
        // A plain panel reserves no band of itself: it draws a frame and gives
        // everything inside it away. R1674 made this an argument rather than a
        // default so a panel that GROWS a header has to come back here.
        &[],
    )
}

fn panel(tag: &str, rect: Rect, fill: Color, border: Option<Color>, children: Vec<Scene>) -> Scene {
    let mut style = BoxStyle::filled(fill);
    if let Some(colour) = border {
        style = style.with_border(Border::new(colour, PANEL_FRAME));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(tag.to_owned())
            .with_style(style)
            .with_layout(absolute(rect)),
    )
}

// ★★★★★ R1693 — this screen had NO keyboard stop. It announced three `button`
// chips and three composite panes and a keyboard user could reach none of them,
// which is the same defect as announcing a `table` with no rows, one axis over:
// a role that promises something the screen cannot do.
//
// The ring is the WAI-ARIA composite pattern: **one stop per composite** (the
// grid, the tree, the byte grid — arrows move *within* them, which is what this
// screen's `ArrowUp`/`ArrowDown` already do) plus one per plain button.
//
// ★ R1696 — the four-line helper that declared it is gone: the sibling screen
// needed the same thing and a verbatim copy is how a mechanism becomes two, so
// it is `Scene::with_focusable` now. Same lift, same evidence and the same
// second-consumer trigger as `Scene::silenced` at R1693.

fn box_at(tag: &str, rect: Rect, fill: Color, border: Option<Color>, radius: u32) -> Scene {
    let mut style = BoxStyle::filled(fill).with_corner_radius(radius);
    if let Some(colour) = border {
        style = style.with_border(Border::new(colour, PANEL_FRAME));
    }
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(tag.to_owned())
            .with_style(style)
            .with_layout(absolute(rect)),
    )
}

const fn contains(rect: Rect, px: u32, py: u32) -> bool {
    px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h
}

/// The centre of a rectangle — where the sweep presses, because a control that
/// does not answer at the middle of its own paint is not reachable.
#[cfg(test)]
const fn centre(rect: Rect) -> (u32, u32) {
    (rect.x + rect.w / 2, rect.y + rect.h / 2)
}

/// A window point in a scrolling pane's own coordinates.
///
/// ★ R1662 — the pane's origin and its scroll offset are folded in ONE place,
/// because a screen that folds them per call site ends up with a painter that
/// moved and a hit test that did not.
fn in_pane(scroll: &ScrollState, pane: Rect, px: u32, py: u32) -> (u32, u32) {
    let (ox, oy) = scroll.offset();
    let fold = |v: u32, origin: u32, by: i32| -> u32 {
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "clamped into u32's range on the line above the cast"
        )]
        let folded =
            (i64::from(v) - i64::from(origin) + i64::from(by)).clamp(0, i64::from(u32::MAX)) as u32;
        folded
    };
    (fold(px, pane.x, ox), fold(py, pane.y, oy))
}

// ── The rectangles both the painter and the hit test read ───────────────────

/// The n-th saved-filter chip, in window coordinates.
fn saved_chip(n: usize) -> Rect {
    let width = 116;
    let x = filter_rect().w.saturating_sub(
        320 + u32::try_from(spec::SAVED_FILTERS.len() - n).unwrap_or(1) * (width + 8),
    );
    Rect::new(x, APP_BAR_H + 12, width, 22)
}

/// ★★★★★ R1721 — the saved-filter **bar**: the box the chips sit in, in the
/// filter strip's own coordinates.
///
/// It exists because the row is one widget: an assistive technology asking where
/// "Saved filters" is should be told where the chips are, and a group whose box
/// was the whole strip would answer with the query field's ground too. Derived
/// from the chips rather than written down, so a fourth saved filter moves it.
fn saved_bar(strip: Rect) -> Rect {
    let first = saved_chip(0);
    let last = saved_chip(spec::SAVED_FILTERS.len().saturating_sub(1));
    Rect::new(
        first.x,
        first.y - strip.y,
        (last.x + last.w).saturating_sub(first.x),
        first.h,
    )
}

/// The n-th message row, in the list pane's own coordinates.
/// ★ R1829 — the n-th column HEADER's pressable rectangle, in the list pane's
/// own unscrolled coordinates: the column's width, the header band's height.
///
/// Deliberately taller than the 12px text run painted inside it. A header is
/// pressed at the word, and a target the exact height of its glyphs is one a
/// reader misses by two pixels — the run stays the thing that is DRAWN and this
/// is the thing that is HIT, which is the same split every chip on this screen
/// already has.
fn list_head(n: usize) -> Rect {
    let col = list_col(n);
    Rect::new(col.x, 0, col.w, HEAD_H)
}

fn list_row(n: usize) -> Rect {
    Rect::new(
        0,
        HEAD_H + u32::try_from(n).unwrap_or(0) * ROW_H,
        list_rect().w,
        ROW_H,
    )
}

/// The n-th column of the message list, in the list pane's own coordinates.
fn list_col(n: usize) -> Rect {
    let flex = list_rect()
        .w
        .saturating_sub(spec::COLUMNS.iter().map(|c| c.width).sum::<u32>() + PAD * 2);
    let mut x = PAD;
    for (i, column) in spec::COLUMNS.iter().enumerate() {
        let w = if column.width == 0 {
            flex
        } else {
            column.width
        };
        if i == n {
            return Rect::new(x, 0, w, ROW_H);
        }
        x += w;
    }
    Rect::new(x, 0, 0, ROW_H)
}

/// The n-th decode row, in the tree pane's own coordinates.
///
/// ★ R1860 — the row is as wide as the pane IS, not as wide as the
/// specification draws it. The two were the same number while the pane could
/// not flex.
fn tree_row(n: usize) -> Rect {
    Rect::new(
        0,
        HEAD_H + u32::try_from(n).unwrap_or(0) * ROW_H,
        tree_rect().w,
        ROW_H,
    )
}

/// The fold chevron of the `n`-th visible decode row, in the tree pane's own
/// space.
///
/// ★★★★★ R1815 — read by the painter AND by the hit test, which is the whole
/// point of it being a function. Before this the rectangle was a literal inside
/// `tree_row_paint` and the hit test did not know the chevron existed: pressing
/// anywhere on a layer row folded it, INCLUDING the name, so the row could not
/// be selected by a pointer at all while the same row opened over the wire.
///
/// Only a layer row draws one. The rectangle is returned for any `n` because it
/// is pure arithmetic; who draws it is [`tree_row_paint`]'s question and who
/// answers for it is [`Hit::at`]'s, and both ask [`spec::LAYERS`].
///
/// ★★★★★ R1875 — a band of the row, like every other run in it. It was
/// `y + 5, h = 12` for a face wanting 18, so the glyph it paints was in a box
/// that could not hold it.
///
/// ⚠ The hit target grows with the box, and that is CORRECT rather than a side
/// effect: R1815 made this one rectangle precisely so the thing a reader sees
/// and the thing a press resolves to cannot differ. A chevron whose box is the
/// size of its glyph is also a chevron whose press target is, and both were
/// six pixels short of the face.
fn tree_chevron(n: usize) -> Rect {
    run_band(tree_row(n), PAD - 6, 10)
}

/// The byte grid's column arithmetic — the crate's, so the painter and the hit
/// test cannot each derive their own.
fn hex_layout() -> HexLayout {
    HEX
}

/// The n-th byte's hex cell, in the byte pane's own coordinates.
fn byte_cell(byte: usize) -> Option<Rect> {
    let layout = hex_layout();
    let cell = layout.hex_cell(byte)?;
    Some(Rect::new(
        PAD + u32::try_from(cell.col).unwrap_or(0) * CELL_W,
        HEAD_H + u32::try_from(cell.row).unwrap_or(0) * CELL_H,
        CELL_W * 2,
        CELL_H,
    ))
}

/// The n-th reassembly lane.
fn lane_rect(n: usize) -> Rect {
    let strip = reassembly_rect();
    let w =
        (strip.w.saturating_sub(PAD * 2)) / u32::try_from(spec::LANES.len().max(1)).unwrap_or(1);
    Rect::new(
        strip.x + PAD + u32::try_from(n).unwrap_or(0) * w,
        strip.y + 34,
        w.saturating_sub(10),
        44,
    )
}

// ── The hit test ────────────────────────────────────────────────────────────

/// What is under a point. One enum, resolved from the same rectangles the
/// painter uses.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hit {
    /// A message row.
    Message(usize),
    /// A decode row, by its path.
    Field(String),
    /// A byte of the frame.
    Byte(usize),
    /// A saved-filter chip.
    Saved(usize),
    /// A layer heading, which folds it.
    Layer(usize),
    /// ★★★★★ R1829 — a column header, which cycles the list's order.
    ///
    /// The header band is the one part of the list pane that does NOT scroll
    /// with the rows, so it is addressed in the pane's unscrolled coordinates —
    /// see [`Hit::at`], where getting that wrong would make the header
    /// pressable only while the list is at the top.
    Header(usize),
    /// Nothing that answers.
    None,
}

impl Hit {
    /// ★★★★★ R1699 — what a **key** press at `tag` addresses.
    ///
    /// A keyboard activation names a thing, not a pixel, so synthesising a
    /// press at the middle of the tag's rectangle would be wrong twice over: a
    /// row scrolled out of the pane has a rectangle and is clipped, and a cell
    /// inside a scrolled row would be aimed at through a viewport nobody moved.
    ///
    /// The danger of a second address space is drift from the first, so the
    /// gate came before the function:
    /// `r1699_every_cursor_member_resolves_to_the_hit_its_tag_names` requires,
    /// for every member of every composite, that this answers exactly what
    /// [`Hit::at`] answers at the centre of that tag's **painted** rectangle —
    /// two derivations of one fact, with the paint as the arbiter.
    fn of_tag(state: &ViewState, tag: &str) -> Self {
        // R1829 — before the row arm, because `pv.list.head.` and
        // `pv.list.row.` share a prefix up to the family and only diverge
        // after it.
        if let Some(n) = tag
            .strip_prefix("pv.list.head.")
            .and_then(|n| n.parse::<usize>().ok())
            && n < spec::COLUMNS.len()
        {
            return Self::Header(n);
        }
        if let Some(n) = tag
            .strip_prefix("pv.filter.saved.")
            .and_then(|n| n.parse::<usize>().ok())
            && n < spec::SAVED_FILTERS.len()
        {
            return Self::Saved(n);
        }
        if let Some(n) = tag
            .strip_prefix("pv.list.row.")
            .and_then(|n| n.parse::<usize>().ok())
            && n < spec::ROWS.len()
        {
            return Self::Message(n);
        }
        // ★★★★★ R1699 — **a cell's press is its row's press**, and that is the
        // paint's answer rather than a convenience. The cell labels are text
        // runs, deliberately transparent to the pointer since R1692 (a container
        // per cell would sit above the row in paint order and take the press the
        // row needs), so a press anywhere on a cell already reaches the row —
        // which is what the gate below checks at the centre of every cell's
        // painted rectangle.
        //
        // What this does NOT give is the mirror of the keyboard's new power: a
        // pointer cannot put the CELL cursor on a particular column, which the
        // floor's item view does (measured at 6.11.1 — clicking a cell makes it
        // current). Registered rather than improvised, because this screen has
        // no behaviour canon for cell selection and inventing one would diverge
        // from the reference instead of matching it.
        if let Some((row, _column)) = tag
            .strip_prefix("pv.list.cell.")
            .and_then(|rest| rest.split_once('_'))
            && let Ok(row) = row.parse::<usize>()
            && row < spec::ROWS.len()
        {
            return Self::Message(row);
        }
        if let Some(b) = tag
            .strip_prefix("pv.bytes.cell.")
            .and_then(|b| b.parse::<usize>().ok())
            && b < state.frame_bytes().len()
        {
            return Self::Byte(b);
        }
        // ★★★★★ R1815 — the chevron has its own tag and now its own arm. It has
        // carried `pv.tree.layer.{id}` since R1693 and nothing here matched it,
        // so the tag was addressable in the paint and inert to every press.
        if let Some(id) = tag.strip_prefix("pv.tree.layer.")
            && let Some(layer) = spec::LAYERS.iter().position(|(lid, _)| *lid == id)
        {
            return Self::Layer(layer);
        }
        if let Some(path) = tag.strip_prefix("pv.tree.field.")
            && state.map.map().field(path).is_some()
        {
            // ★ A layer heading falls here too, and that is the repair: the row
            // SELECTS, exactly as every other row of the tree does and as the
            // behaviour canon does for all of them. Folding answers to the
            // chevron's tag above.
            return Self::Field(path.to_owned());
        }
        Self::None
    }

    /// ★★★★★ R1700 — the word the wire answers a press with, and therefore the
    /// word [`External::target_at`] answers with too.
    ///
    /// Lifted out of `query("hit.<x>.<y>")`, where it was spelled inline. One
    /// function rather than two, because the framework's check holds a press's
    /// word against a tag's word and a second spelling would make it compare
    /// this screen with a copy of itself.
    fn word(&self) -> Option<String> {
        Some(match self {
            Self::Message(n) => format!("message.{n}"),
            Self::Field(p) => format!("field.{p}"),
            Self::Byte(b) => format!("byte.{b}"),
            Self::Saved(n) => format!("saved.{n}"),
            Self::Layer(n) => format!("layer.{n}"),
            Self::Header(n) => format!("header.{n}"),
            Self::None => return None,
        })
    }

    /// What answers at the window point `(px, py)`.
    fn at(state: &ViewState, px: u32, py: u32) -> Self {
        // ★★★★★ R1707 — **there is deliberately no stand-aside arm for the
        // query box here, and that is a measurement rather than an oversight.**
        //
        // The sibling screen needs one: its field opens ON TOP of the form, so
        // without an arm the press resolves to the form row underneath and the
        // caret never lands. This bar is laid out so that nothing else answers
        // where the box is — the chips start at x=748 and the box ends at 472 —
        // so an arm here would return `None` in a case that already returns
        // `None`.
        //
        // A counterfactual proved it: neutering the arm this round first wrote
        // changed no answer anywhere, and the nine-point test written to guard
        // it could not fail, because `Hit::None` is what this screen says both
        // for "standing aside" and for "nothing is there". What IS load-bearing
        // is `query_byte_at`, and that is where the gate went.
        for (n, _) in spec::SAVED_FILTERS.iter().enumerate() {
            if contains(saved_chip(n), px, py) {
                return Self::Saved(n);
            }
        }
        let list = list_rect();
        if contains(list, px, py) {
            // ★★★★★ R1829 — the header band FIRST, and in UNSCROLLED pane
            // coordinates. The rows below it are inside the scroll node and the
            // header is not, so running the header through `in_pane` would
            // shift it by the scroll offset and make it answer only while the
            // list is at the top — a defect that hides completely in a test
            // that never scrolls.
            let (hx, hy) = (px.saturating_sub(list.x), py.saturating_sub(list.y));
            if hy < HEAD_H {
                for n in 0..spec::COLUMNS.len() {
                    if contains(list_head(n), hx, hy) {
                        return Self::Header(n);
                    }
                }
                return Self::None;
            }
            let (lx, ly) = in_pane(&state.list_scroll, list, px, py);
            // R1707 — walk what is DRAWN. A hit test over the source rows would
            // answer a hidden message under a filtered list, which is the exact
            // shape of "what is drawn is what is pressed" this tree closed for
            // the sibling screens.
            for (visual, &n) in state.kept().iter().enumerate() {
                if contains(list_row(visual), lx, ly) {
                    return Self::Message(n);
                }
            }
            return Self::None;
        }
        let tree = tree_rect();
        if contains(tree, px, py) {
            let (tx, ty) = in_pane(&state.tree_scroll, tree, px, py);
            for (n, (path, ..)) in visible_fields(state).into_iter().enumerate() {
                if contains(tree_row(n), tx, ty) {
                    // ★★★★★ R1815 — **the fold is the CHEVRON's, and the rest of
                    // the row selects.** This arm used to answer `Layer` for the
                    // whole row, so a layer heading could not be opened by a
                    // pointer at all — while `invoke select_field` opened it,
                    // which is a screen whose two channels disagree about what
                    // the same row does.
                    //
                    // The behaviour canon puts selection on EVERY row of the
                    // decode tree and has no fold whatever; folding is this
                    // screen's own addition, and it had quietly taken the
                    // canon's gesture to pay for itself. The standing rule is
                    // that what the canon has and we lack gets built, and what
                    // we have and it lacks is kept — so the fold moves onto the
                    // affordance that draws it rather than being removed.
                    //
                    // The two rectangles are disjoint by construction: the
                    // chevron is 10px at `PAD - 6` and the name starts at
                    // `indent + 6`, so no press is ambiguous.
                    if spec::LAYERS.iter().any(|(id, _)| *id == path.as_str())
                        && contains(tree_chevron(n), tx, ty)
                    {
                        let layer = spec::LAYERS
                            .iter()
                            .position(|(id, _)| *id == path.as_str())
                            .unwrap_or(0);
                        return Self::Layer(layer);
                    }
                    return Self::Field(path);
                }
            }
            return Self::None;
        }
        let bytes = bytes_rect();
        if contains(bytes, px, py) {
            let (bx, by) = in_pane(&state.bytes_scroll, bytes, px, py);
            for byte in 0..spec::SOURCES[0].1 {
                if byte_cell(byte).is_some_and(|r| contains(r, bx, by)) {
                    return Self::Byte(byte);
                }
            }
        }
        Self::None
    }
}

// ── The handlers a press and the wire both reach ────────────────────────────

fn select_message(state: &Rc<ViewState>, row: usize) {
    if row >= spec::ROWS.len() {
        return;
    }
    state.row.set(row);
    state.map.set(decode(row));
    // The previously selected field may not exist in the new decode. Fall back
    // to the outermost layer rather than to nothing, because a decode pane with
    // no selection has no highlight to show and the screen would look broken.
    let map = state.map.map();
    if map.field(&state.field.get()).is_none() {
        state.field.set(spec::LAYERS[0].0.to_owned());
    }
    state.say(Utterance::done(format!("message {row}")));
}

fn select_field(state: &Rc<ViewState>, path: &str) {
    if state.map.map().field(path).is_none() {
        return;
    }
    state.field.set(path.to_owned());
    state.say(Utterance::done(format!("field {path}")));
}

/// The inverse direction as a gesture: a press on a byte selects the field that
/// owns it. The screen does not keep its own byte→field table — the map is
/// asked, so the highlight a press produces and the highlight a field selection
/// produces are the same derivation.
fn select_byte(state: &Rc<ViewState>, byte: usize) {
    // ★ R1698 — the cursor goes where the byte is, whether a press or an arrow
    // brought us here, so the pointer and the keyboard never disagree about
    // where the grid's cursor rests.
    state.byte.set(byte);
    let map = state.map.map();
    match map.coverage_at(SourceId::new(0), byte) {
        Coverage::Field(span) => {
            let path = span.path().to_owned();
            state.field.set(path.clone());
            state.say(Utterance::done(format!("byte {byte} is {path}")));
        }
        // ★ R1719 — both of these are answers, not failures: the byte is
        // genuinely outside anything the dissector claimed. A person asked and
        // is being told, so they say it in the tone of a thing that happened.
        Coverage::Unmapped => state.say(Utterance::done(format!(
            "byte {byte} is claimed by no field"
        ))),
        Coverage::OutOfBuffer => {
            state.say(Utterance::done(format!("byte {byte} is past the frame")));
        }
    }
}

/// ★★★ R1707 — set the running query and say what it did.
///
/// The one write path. The pointer, the keyboard, the saved chips and the wire
/// all come through here, so "what the bar shows" and "what the list runs"
/// cannot become two answers.
fn set_query(state: &Rc<ViewState>, text: &str) {
    state.query.set_text(text.to_owned());
    announce_query(state);
}

/// ★★★ R1707 — §2 #2: an agent runs the same query a person types, through the
/// same slot, and gets the same list.
///
/// A malformed query is REFUSED here rather than kept, which is the opposite of
/// what the painted bar does with one — and both are right. A person types a
/// query one character at a time and is malformed on nearly every keystroke; an
/// agent sends a whole query, and a silent "that kept everything" would be
/// indistinguishable from success.
fn run_filter(state: &Rc<ViewState>, text: &str) -> Result<IntrospectValue, InvokeError> {
    let query = RowQuery::parse(text, spec::QUERY_COLUMNS)
        .map_err(|why| InvokeError::rejected(why.to_string()))?;
    set_query(state, text);
    Ok(IntrospectValue::Json(serde_json::json!({
        "kept": state.kept().len(),
        "of": spec::ROWS.len(),
        "clauses": query.clauses().len(),
    })))
}

/// ★★★★★ R1829 — **order the list**, from the wire form the `sort` read answers.
///
/// # Why it refuses rather than clamps
///
/// `grid_sort_parse` returns `None` for a string it cannot read, and a column
/// past the end is a different failure from a malformed one — so both are
/// answered, by name, with the range the caller could have used. The
/// alternative every grid in this tree offers instead is a SILENT clamp to
/// unsorted, which is right for *restoring* a saved order (a stale column must
/// not point the glyph at a phantom) and wrong for a command a client just
/// issued: it would report success and leave the list in the order it was
/// already in, which reads as "the sort did nothing" rather than "you asked for
/// a column that is not there".
///
/// # What it answers
///
/// The order it ended in and the row now at the top, because that is the fact a
/// caller is actually after: *which message am I looking at first*. A bare
/// acknowledgement would make the caller ask again.
fn run_sort(state: &Rc<ViewState>, text: &str) -> Result<IntrospectValue, InvokeError> {
    let columns = spec::COLUMNS.len();
    let sort = grid_sort_parse(text).ok_or_else(|| {
        InvokeError::rejected(format!(
            "{text:?} is not an order — use \"none\", or \"<column>:ascending\" \
             / \"<column>:descending\" with a column in 0..{columns}"
        ))
    })?;
    if let Some((col, _)) = sort
        && col >= columns
    {
        return Err(InvokeError::rejected(format!(
            "no column {col} — this list has {columns}, numbered 0..{}",
            columns - 1
        )));
    }
    state.sort.set(sort);
    let kept = state.kept();
    announce_sort(state);
    Ok(IntrospectValue::Json(serde_json::json!({
        "sort": grid_sort_str(state.sort.get()),
        "kept": kept.len(),
        "top": kept.first(),
    })))
}

/// ★ R1829 — say what the order is now, naming the COLUMN rather than its
/// index: the status line is read by a person, and "ordered by time, oldest
/// first" is the sentence they would say. The index stays on the wire, where
/// the reader is a client.
fn announce_sort(state: &Rc<ViewState>) {
    match state.sort.get() {
        None => state.say(Utterance::done("capture order")),
        Some((col, ascending)) => {
            let title = spec::COLUMNS.get(col).map_or("?", |column| column.title);
            state.say(Utterance::done(format!(
                "ordered by {title}, {}",
                if ascending { "ascending" } else { "descending" }
            )));
        }
    }
}

/// R1707 — say what the running query did, in the words the bar prints.
fn announce_query(state: &Rc<ViewState>) {
    match state.query_fault() {
        // ★★★★★ R1719 — the frame this line used to write by hand is the
        // tone's now, so the word "refused" is written once in the workspace
        // and this screen's refusal reaches a reader interrupting rather than
        // waiting, like every other screen's.
        Some(why) => state.say(Utterance::refused(&why)),
        None if state.query.text().trim().is_empty() => {
            state.say(Utterance::done("filter cleared"));
        }
        None => state.say(Utterance::done(format!(
            "{} of {} shown",
            state.kept().len(),
            spec::ROWS.len()
        ))),
    }
}

/// ★★ R1707 — a saved chip applies its own query.
///
/// Until this round this flipped a boolean and announced "applied units only"
/// while the list did not move. The chips are exclusive because the queries are
/// whole queries rather than clauses: turning two on would mean composing them,
/// and the reference offers no such composition — pressing a second saved
/// filter there replaces the first.
/// ★★★★★ R1721 — **the saved-filter bar, as the one widget it is.**
///
/// The row's rule is declared here and nowhere else, and six things follow from
/// it: the `listbox` the group is announced as, the `option` each chip is, the
/// `aria-selected` that carries a chip's on-ness, the single Tab stop the bar
/// costs, the arrows that walk it, and the `Enter` that applies one. Before this
/// existed each of those was a separate decision and two of them were wrong —
/// measured by driving the running screen, the bar announced three independent
/// toggle buttons over a set that can never have two on.
///
/// [`SAVED_ROW`](spec::SAVED_ROW) is the rule; this projects the screen's own
/// state through it, so there is no second copy of what is on.
fn saved_row(state: &Rc<ViewState>) -> ChipGroup {
    saved_row_of(&state.saved.get(), state.saved_cursor.get())
}

/// The bar built from an on-set and a cursor, which is what makes the roster and
/// the rule readable without a running screen — the censuses ask for the stops,
/// and the stops do not depend on which chip is on.
fn saved_row_of(on: &[bool], cursor: usize) -> ChipGroup {
    ChipGroup::new(
        SAVED_TAG,
        "Saved filters",
        spec::SAVED_FILTERS
            .iter()
            .enumerate()
            .map(|(n, saved)| {
                Chip::new(
                    format!("{SAVED_TAG}.{n}"),
                    saved.name,
                    on.get(n).copied().unwrap_or(false),
                )
            })
            .collect(),
        spec::SAVED_ROW,
    )
    .with_cursor(cursor)
}

/// The keyboard stops the saved-filter bar costs, derived from its rule.
///
/// Read by the two ring censuses only: the production ring is *enumerated off the
/// painted scene*, which is the arbiter — a list this file also asserted against
/// would be the screen grading its own homework.
#[cfg(test)]
fn saved_stops() -> Vec<String> {
    saved_row_of(&[], 0)
        .stops()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The bar's own tag — the group node, and the Tab stop the rule derives.
const SAVED_TAG: &str = "pv.filter.saved";

fn toggle_saved(state: &Rc<ViewState>, n: usize) {
    let mut row = saved_row(state);
    let said = row.choose(n);
    // ★★★★★ R1721 — the rule applied the change; this screen only stores it.
    // The `vec![false; N]` that used to live here was the rule, written out at
    // one site while three others announced a different one.
    state.saved.set(row.on());
    if said.tone() == Tone::Done {
        state.saved_cursor.set(n);
        let on = row.chips()[n].on;
        set_query(state, if on { spec::SAVED_FILTERS[n].query } else { "" });
        state.say(Utterance::done(format!(
            "{} {} — {}",
            if on { "applied" } else { "cleared" },
            spec::SAVED_FILTERS[n].name,
            count_line(state),
        )));
    } else {
        state.say(said);
    }
}

fn toggle_layer(state: &Rc<ViewState>, n: usize) {
    let mut folded = state.folded.get();
    if let Some(slot) = folded.get_mut(n) {
        *slot = !*slot;
        state.folded.set(folded);
        state.say(Utterance::done(format!("layer {}", spec::LAYERS[n].0)));
    }
}

/// ★★★★★ R1829 — pressing a column header cycles this list's order the way
/// every other grid in this tree cycles: unsorted -> ascending -> descending ->
/// unsorted, and a DIFFERENT column jumps straight to it ascending.
///
/// The transition is [`cycle_col_sort`], taken rather than re-matched. It is
/// three lines to write and the reason not to write them is that this is a
/// *controller wiring*: a screen whose header cycle disagreed with the rest of
/// the tree would be a bug wearing the costume of a style choice, and nothing
/// would catch it — the states are all legal, just not the ones a reader who
/// has used another grid here expects.
fn cycle_sort(state: &Rc<ViewState>, col: usize) {
    state
        .sort
        .set(cycle_col_sort(state.sort.get(), col, spec::COLUMNS.len()));
    announce_sort(state);
}

fn move_cursor(state: &Rc<ViewState>, px: u32, py: u32) {
    state.cursor.set((px, py));
    // ★ R1918 — a move is what says the pointer is here again after a leave.
    state.pointer_inside.set(true);
}

fn press(state: &Rc<ViewState>) {
    let (px, py) = state.cursor.get();
    act_on_hit(state, Hit::at(state, px, py));
}

/// ★★★★★ R1699 — what a completed press on one hit target does, whichever
/// channel produced the target.
///
/// Lifted out of [`press`] so a **key** reaches the same arms a pointer does.
/// Before this the two channels were structurally incapable of agreeing: the
/// pointer's actions lived inside a function that started by reading a cursor
/// position, so a keyboard could only have got at them by inventing coordinates
/// or by writing the arms a second time.
///
/// Returns whether the hit did anything, which is what lets the keymap fall
/// through when a stop names nothing pressable.
fn act_on_hit(state: &Rc<ViewState>, hit: Hit) -> bool {
    match hit {
        Hit::Message(n) => select_message(state, n),
        Hit::Field(path) => select_field(state, &path),
        Hit::Byte(b) => select_byte(state, b),
        Hit::Saved(n) => toggle_saved(state, n),
        Hit::Layer(n) => toggle_layer(state, n),
        Hit::Header(n) => cycle_sort(state, n),
        Hit::None => return false,
    }
    true
}

/// R1699 — choose what a nested cursor is resting on: the innermost tag of the
/// path, which is the thing the reader actually named.
fn activate_tag(state: &Rc<ViewState>, path: &[&str]) {
    if let Some(tag) = path.last() {
        act_on_hit(state, Hit::of_tag(state, tag));
    }
}

/// ★★★★★ R1698 — **the cursor each pane already has, said in the framework's
/// vocabulary.**
///
/// Not new state: the message list's cursor IS `row`, the decode tree's IS
/// `field`, and the byte grid's is the byte the map has selected. A second copy
/// of any of them would be a second thing to keep in step. So this projects
/// what the screen holds into a [`Roving`] — which is what makes the arrows
/// scopable, the active descendant publishable, and the policy askable, without
/// the screen owning a cursor twice.
///
/// All three declare [`Activation::Follows`], and that is the substantive
/// difference from the sibling screen: here the cursor **is** the selection —
/// moving down a message list means reading the next message — while a
/// navigation rail whose selection followed its cursor would navigate away from
/// the page a reader is trying to leave.
fn pane_cursor(state: &Rc<ViewState>, stop: &str) -> Option<Roving> {
    let (spec, members, at) = match stop {
        "pv.list" => (
            RovingSpec::new(Axis::Vertical).with_activation(Activation::Follows),
            // ★★★ R1707 — the roster is what the query KEPT. A cursor that
            // walked the hidden rows would step onto messages the list does not
            // draw, which is the keyboard half of the defect the hit test just
            // stopped having.
            state
                .kept()
                .into_iter()
                // ★★★★★ R1699 — a row is a composite of its cells, which is
                // what `grid` MEANS. Every row carries its inner roster, not
                // only the selected one: a client is entitled to ask what is
                // inside a row before moving the selection onto it, and a
                // roster that appeared when the selection arrived would make
                // the answer depend on where somebody is standing.
                .map(|n| Member::new(format!("pv.list.row.{n}")).containing(row_cells_cursor(n)))
                .collect::<Vec<_>>(),
            // ★★★ R1707 — the cursor rests on the selected message when the
            // query kept it, and otherwise on the first message it did keep.
            //
            // DERIVED rather than repaired: a filter that moved the selection
            // would have to do it from inside the text field's own keystroke
            // path, and a view is not allowed to mutate (§6.3). The reference
            // prototype leaves its selection alone too — what would be wrong is
            // a cursor pointing at a row that is not in its own roster.
            format!("pv.list.row.{}", state.cursor_row()),
        ),
        "pv.tree" => (
            RovingSpec::new(Axis::Vertical).with_activation(Activation::Follows),
            visible_fields(state)
                .iter()
                .map(|(path, ..)| Member::new(format!("pv.tree.field.{path}")))
                .collect(),
            format!("pv.tree.field.{}", state.field.get()),
        ),
        "pv.bytes" => (
            // Both axes, because the grid wraps: a byte's neighbour to the
            // right is the next byte and the one below is sixteen further on,
            // and both are steps along the SAME linear buffer. `Both` is the
            // arm ARIA leaves undefined rather than calling horizontal.
            RovingSpec::new(Axis::Both).with_activation(Activation::Follows),
            (0..state.frame_bytes().len())
                .map(|b| Member::new(format!("pv.bytes.cell.{b}")))
                .collect(),
            format!("pv.bytes.cell.{}", state.byte.get()),
        ),
        // ★★★★★ R1721 — the saved-filter bar's cursor is not written out here at
        // all: the row's own rule builds it, seats it, and picks the policy. The
        // three panes below still project by hand because their rosters are the
        // screen's subject matter; a chip row's roster IS the widget.
        SAVED_TAG => return saved_row(state).cursor(),
        _ => return None,
    };
    let mut roving = Roving::new(spec);
    roving.seat(members);
    roving.point_at(&at);
    // ★★★★★ R1699 — the descent is projected too, from the one fact that holds
    // it. `Some(column)` means the reader went into the selected row, so the
    // composite is entered and its inner cursor points at that cell.
    if stop == "pv.list"
        && let Some(column) = state.cell.get()
    {
        roving.enter();
        if let Some(inner) = roving.inner_at_cursor_mut() {
            inner.point_at(&list_cell_tag(state.row.get(), column));
        }
    }
    Some(roving)
}

/// ★★★★★ R1699 — the cells of one message row, as the composite that row **is**.
///
/// `Stop` at the ends, unlike the tab list beside it: a row has a first and a
/// last column a reader is meant to feel, and wrapping from the length back to
/// the timestamp would read as a jump to another row. `Follows`, because
/// arriving at a cell IS reading it — the same argument the enclosing list
/// makes about its rows.
fn row_cells_cursor(row: usize) -> Roving {
    let mut cells = Roving::new(
        RovingSpec::new(Axis::Horizontal)
            .with_ends(Ends::Stop)
            .with_activation(Activation::Follows),
    );
    cells.seat(
        (0..spec::COLUMNS.len())
            .map(|c| Member::new(list_cell_tag(row, c)))
            .collect(),
    );
    cells
}

/// R1698 — put a pane's cursor where a [`Roving`] left it.
///
/// The write half of [`pane_cursor`]'s projection: one place, so a cursor that
/// moved and a selection that did not is not a state this screen can be in.
fn seat_pane_cursor(state: &Rc<ViewState>, stop: &str, roving: &Roving) {
    let Some(index) = roving.cursor() else { return };
    match stop {
        "pv.list" => {
            select_message(state, index);
            // ★★★★★ R1699 — the descent is written back with the row, in the
            // same place, so a cursor that went into a cell and a selection
            // that did not is not a state this screen can be in.
            let column = roving
                .entered()
                .then(|| roving.inner_at_cursor().and_then(Roving::cursor))
                .flatten();
            state.cell.set(column);
            if let Some(column) = column {
                state.say(Utterance::done(format!(
                    "{} of message {index}",
                    spec::COLUMNS[column].title
                )));
            }
        }
        "pv.tree" => {
            if let Some((path, ..)) = visible_fields(state).get(index) {
                select_field(state, &path.clone());
            }
        }
        "pv.bytes" => select_byte(state, index),
        // ★★★★★ R1721 — walking the saved-filter bar moves the CURSOR and applies
        // nothing, because the row declared `Explicit`. A bar whose arrows applied
        // filters would run four queries on the way to the fifth chip, and that
        // is the distinction `Activation` exists to make.
        SAVED_TAG => {
            state.saved_cursor.set(index);
            if let Some(chip) = saved_row(state).chips().get(index) {
                state.say(Utterance::unchanged(chip.label.clone()));
            }
        }
        _ => {}
    }
}

fn key(state: &Rc<ViewState>, chord: &str) -> bool {
    key_at(state, focus_state::focused().as_deref(), chord)
}

/// ★★★★★ R1698 — **the keymap, told where the reader is standing.**
///
/// Measured before this existed, by driving the running application: at all
/// SIX of this screen's Tab stops — three filter chips, the decode tree and the
/// byte grid included — pressing `ArrowDown` moved the **message list**, and
/// the active descendant was `None` everywhere. An arrow meant one thing no
/// matter where anybody was standing, which is the other half of the composite
/// pattern R1693 left open when it gave the panes their Tab stops.
///
/// A plain button is its own stop and owns no cursor, so a chord arriving there
/// falls through to the screen — which is why `Escape` still works from
/// anywhere and why the pane arrows no longer do.
fn key_at(state: &Rc<ViewState>, focused: Option<&str>, chord: &str) -> bool {
    if let Some(stop) = focused
        && let Some(mut roving) = pane_cursor(state, stop)
        && let Some(landing) = roving.key(chord)
    {
        match landing {
            // ★★★★★ R1699 — entering and leaving move the projection too, and
            // they are the same write as an arrow: `seat_pane_cursor` reads the
            // whole path, so there is one place that turns a cursor into state
            // whatever key moved it.
            // ★★★★★ R1721 — `choose: false` seats the cursor too, and until this
            // round it did not: every stop here declared `Follows`, so the arm
            // was dead and the saved-filter bar — the screen's first `Explicit`
            // composite — would have walked its cursor and thrown the move away.
            // A dead arm is a defect waiting for its first caller.
            Landing::Moved { .. } | Landing::Entered(_) | Landing::Exited(_) => {
                seat_pane_cursor(state, stop, &roving);
            }
            Landing::Chosen(_) | Landing::Refused(_) => {
                activate_tag(state, &roving.tag_path());
            }
            Landing::Held(_) | Landing::Nowhere => {}
        }
        return true;
    }
    // ★★★★★ R1699 — **a stop that owns no cursor can still be acted on.**
    //
    // Measured before this existed, by driving the running screen: the three
    // saved-filter chips announce `role=button`, a keyboard reaches all three,
    // and `Enter` and `Space` at every one of them changed nothing painted. A
    // button a keyboard cannot press is below the floor rather than above it —
    // measured at 6.11.1, a push button activates on both keys, always.
    if let Some(stop) = focused
        && matches!(chord, "Enter" | "Space")
        && act_on_hit(state, Hit::of_tag(state, stop))
    {
        return true;
    }
    match chord {
        // ★★★★★ R1693 — the chords a real keyboard sends. They were `Down` and
        // `Up`, which **no key press produces**: the shell spells a named key
        // the way the web platform does (`ArrowDown`), and this screen is the
        // only place in the tree that spelled them short. Measured through the
        // wire, `ArrowDown` moved nothing and `Down` moved the selection — so
        // this screen's keyboard navigation had never worked from a keyboard,
        // and every test that drove it passed because every test used the
        // screen's own spelling.
        //
        // The round that found it is the round that announced this list as a
        // `grid`, which is a composite widget whose contract IS that the arrows
        // move the selection. Announcing that while the arrows are dead is the
        // same class of defect as announcing a table with no rows.
        // ★★★★★ R1698 — **the message list's arrows belong to the message
        // list.** A chord a composite does not navigate by falls through, and
        // this is where it used to land: measured on the running screen,
        // standing on any of the three saved-filter chips and pressing Down
        // moved a row in a pane the reader was not in.
        //
        // `None` still reaches it, deliberately — that is the wire's own
        // channel (`invoke("key", …)` with nothing focused), where an agent
        // asking the list to move its selection is asking for exactly that.
        "ArrowDown" | "ArrowUp" if focused.is_none_or(|tag| tag == "pv.list") => {
            let row = state.row.get();
            let next = if chord == "ArrowDown" {
                (row + 1).min(spec::ROWS.len() - 1)
            } else {
                row.saturating_sub(1)
            };
            select_message(state, next);
            true
        }
        // ★★★★★ R1815 — **expand and collapse, which is what a tree item
        // announcing `aria-expanded` is contracting to accept.**
        //
        // This screen announces `aria-expanded` on every layer heading, and
        // until this round NO KEY COULD CHANGE IT. The tree's cursor declares
        // `Activation::Follows`, so an arrow moves the cursor and selects; there
        // is nothing for `Enter` to activate, the roving consumes no such chord,
        // and the fallback arm asks `Hit::of_tag` about the focused stop — the
        // PANE tag, never the row's. Folding was reachable from the pointer
        // alone.
        //
        // ★★★★★ That fact is MEASURED, in
        // `r1815_the_arrows_expand_and_collapse_what_the_item_announces`, and it
        // is measured because reading this code gave the wrong answer twice, in
        // opposite directions — first that `Enter` folded a row, then that it
        // did. Running it is what settled which. A claim about a keymap made by
        // tracing control flow through a roving cursor, a landing enum and two
        // fallback arms is a guess wearing a citation.
        //
        // The chevron cannot be the keyboard's answer either: it is declared
        // part of its tree item rather than a stop of its own, which is the
        // correct ARIA shape — a tree item owns its expansion — and is exactly
        // why the *arrows* are where expansion belongs. The tree's roving is
        // `Axis::Vertical`, so neither arrow is consumed and both fall through
        // to here, where until now nothing claimed them.
        //
        // ⚠ This is the EXPANSION half of the ARIA tree pattern, not all of it:
        // there, `ArrowRight` on an already-open node moves to its first child
        // and `ArrowLeft` on a leaf moves to its parent. Those are navigation,
        // they need a cursor that can address a parent, and they are not built
        // — a chord that would only navigate returns `false` and falls through
        // rather than silently doing nothing.
        "ArrowRight" | "ArrowLeft" if focused == Some("pv.tree") => {
            let path = state.field.get();
            spec::LAYERS
                .iter()
                .position(|(id, _)| *id == path)
                .is_some_and(|index| {
                    let want_folded = chord == "ArrowLeft";
                    if state.folded.get().get(index).copied().unwrap_or(false) == want_folded {
                        return false;
                    }
                    toggle_layer(state, index);
                    true
                })
        }
        "Escape" => {
            state.field.set(spec::LAYERS[0].0.to_owned());
            true
        }
        _ => false,
    }
}

// ── The view ────────────────────────────────────────────────────────────────

fn view(field: (TextFieldState, u32), _frame: Frame) -> Scene {
    let state = use_view_state();
    let theme = use_theme(THEME_TAG).theme_animated();
    let ink = ink(&theme);
    let (w, h) = window_size();
    let mut children = vec![
        app_bar(&state, ink),
        filter_bar(&state, field, &theme, ink),
        context_strip(&state, ink),
        list_pane(&state, ink),
        tree_pane(&state, ink),
        bytes_pane(&state, ink),
        reassembly_strip(ink),
    ];
    // ★ R1918 — LAST, so a description paints over the panes it hangs off.
    children.extend(description_scene(&state, ink));
    Scene::Container(
        ContainerNode::new(vec![
            panel("pv.root", Rect::new(0, 0, w, h), ink.bg, None, children).silenced(
                Silence::layout("places the two bars, the three panes and the reassembly strip"),
            ),
        ])
        // ★ R1664 — the root carries the tag the widget is REGISTERED under, so
        // the router has something to resolve a press to. `pv.root` above is an
        // ADDRESS, for `scene/snapshot` and the sweep; this is the RECEIVER.
        // They were two string literals in two functions with nothing checking
        // that either of them named anything, and the screen was dead at every
        // point in the window. `scene/pointer_reach`.externals is the read that
        // now holds both sides of that join.
        .with_tag(VIEW_TAG)
        .with_layout(
            LayoutStyle::new()
                .with_size(Size::px(w, h))
                // The receiver is an address for presses, not a region a reader
                // travels to: everything it holds is `pv.root`, which says so
                // for itself.
                .with_silence(Silence::layout(
                    "the window's receiver; it holds the screen",
                )),
        ),
    )
}

fn app_bar(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let (w, _) = window_size();
    panel(
        "pv.appbar",
        Rect::new(0, 0, w, APP_BAR_H),
        ink.surface,
        Some(ink.outline),
        vec![
            // ★ R1800 — the run the reader reported, twice, eleven days apart:
            // the descender of `p` was cut off. The box was `h = 16` beside a
            // `FONT_TITLE` of 14, two numbers chosen independently, and the
            // face needs 23. Derived now, and centred in the bar rather than
            // placed at a `+19` that centred the OLD height.
            label(
                "packet view",
                line_rect_in(Rect::new(0, 0, w, APP_BAR_H), 16, 96, FONT_TITLE),
                FONT_TITLE,
                ink.text,
            ),
            tagged_label(
                "pv.appbar.interface",
                spec::INTERFACE,
                Rect::new(124, 20, 190, 14),
                FONT_SMALL,
                ink.text_2,
            ),
            tagged_label(
                "pv.appbar.rate",
                spec::RATE,
                Rect::new(324, 20, 120, 14),
                FONT_SMALL,
                ink.ok,
            ),
            tagged_label(
                "pv.appbar.said",
                state.said_sentence(),
                Rect::new(w.saturating_sub(360), 20, 340, 14),
                FONT_SMALL,
                ink.text_3,
            ),
        ],
    )
}

/// Which byte of the query box a window point is on, or `None` when the box is
/// unfocused or the point is outside it.
///
/// The one hit-test funnel the press hook and the drag hook share: two of them
/// would let a drag select to a different byte than the press caret landed on.
/// The rectangle comes from the painted scene, so it is the box a person sees.
fn query_byte_at(
    interaction: TextFieldState,
    scene: &Scene,
    focused: Option<&str>,
    x: f32,
    y: f32,
) -> Option<usize> {
    if focused != Some(QUERY_TAG) {
        return None;
    }
    let rect = pinion_shell::rect_for_tag(scene, QUERY_TAG)?;
    // Compared in the pointer's own units rather than by casting it to the
    // rectangle's: a cast would round a point just outside the left edge INTO
    // the box, and a press half a pixel above it out of one it is in.
    if !rect.contains_point(x, y) {
        return None;
    }
    tf_paint::byte_for_scene_point(
        QUERY_TAG,
        interaction,
        scene,
        x,
        y,
        &use_theme(THEME_TAG).theme_animated(),
        &query_field_style(),
    )
}

/// R1707 — how the query box is drawn, and the SSOT the click-to-caret hit test
/// resolves against. Two styles here would put the caret on a different letter
/// from the one under the cursor.
fn query_field_style() -> tf_paint::TextFieldStyle {
    tf_paint::TextFieldStyle {
        field_w: QUERY_W,
        field_h: QUERY_H,
        field_pad: 8,
        font_size_px: FONT_SMALL,
        ..tf_paint::TextFieldStyle::m3_filled()
    }
}

fn filter_bar(
    state: &Rc<ViewState>,
    field: (TextFieldState, u32),
    theme: &Theme,
    ink: Ink,
) -> Scene {
    let rect = filter_rect();
    let mut children = Vec::new();
    // ★★★ R1707 — the query, as a box a person types in.
    //
    // Until this round the bar painted three constant strings here and the list
    // ignored them. What it showed was a filter; what it did was nothing.
    let fault = state.query_fault();
    children.push(Scene::Container(
        ContainerNode::new(vec![tf_paint::view_field(
            QUERY_TAG,
            field.0,
            field.1,
            theme,
            &query_field_style(),
            "Filter query",
        )])
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(PAD, 10)
                .with_size(Size::px(QUERY_W, QUERY_H)),
        ),
    ));
    // The reason, where the reader is looking, and only when there is one. A
    // query bar that answered a malformed query with an empty list would be
    // indistinguishable from one that answered a correct query with no matches.
    if let Some(why) = &fault {
        children.push(tagged_label(
            "pv.filter.fault",
            why.clone(),
            Rect::new(PAD + QUERY_W + 12, 17, 300, 13),
            FONT_SMALL,
            ink.err,
        ));
    }
    // ★★★★★ R1721 — the bar is ONE widget, and it says so in the paint: a group
    // container over the chips, focusable exactly when the rule makes the row
    // the stop. The pill keeps this screen's own tones (the reference's, not
    // Material's) — what came from the framework is the one thing a chip cannot
    // work out by looking at itself, and the `with_focusable(true)` that used to
    // sit here was that answer, guessed.
    let row_group = saved_row(state);
    let bar = saved_bar(rect);
    let mut pills = Vec::with_capacity(row_group.len() * 2);
    for (n, chip) in row_group.chips().iter().enumerate() {
        let at = saved_chip(n);
        let (cx, cy) = (at.x - bar.x, at.y - rect.y - bar.y);
        pills.push(
            box_at(
                &chip.tag,
                Rect::new(cx, cy, at.w, at.h),
                if chip.on { ink.lit } else { ink.surface },
                Some(if chip.on { ink.accent } else { ink.outline }),
                11,
            )
            .with_focusable(row_group.is_a_stop(&chip.tag)),
        );
        pills.push(label(
            chip.label.clone(),
            Rect::new(cx + 10, cy + 5, at.w - 20, 12),
            FONT_SMALL,
            if chip.on { ink.accent } else { ink.text_2 },
        ));
    }
    children.push(
        Scene::Container(
            ContainerNode::new(pills)
                .with_tag(SAVED_TAG.to_owned())
                // ★ A tagged node that is not pointer-transparent becomes the
                // router's hit target and swallows the press, because nothing
                // resolves the BAR to an action — the class
                // `debt-a-tagged-node-can-swallow-a-real-press-anywhere` names.
                // The bar is a keyboard fact; the pointer still reaches whichever
                // chip it is over.
                .with_layout(absolute(bar).with_pointer_transparent(true)),
        )
        .with_focusable(row_group.is_a_stop(SAVED_TAG)),
    );
    children.push(tagged_label(
        "pv.filter.count",
        count_line(state),
        Rect::new(rect.w.saturating_sub(196), 16, 180, 14),
        FONT_SMALL,
        ink.text_2,
    ));
    panel("pv.filter", rect, ink.surface, Some(ink.outline), children)
}

/// ★★★ R1707 — what the bar's right end says.
///
/// Unfiltered it is the capture's own scale, which is the fact a reader wants
/// when nothing is narrowing the list. Filtered it is **derived** — how many of
/// the messages this screen holds the query kept — because a filter that
/// reported a constant while the list changed under it is the defect this
/// round exists to remove, and a number nothing derives is how that survives.
fn count_line(state: &ViewState) -> String {
    if state.query().is_everything() {
        return format!("{} / {}", comma(spec::MATCHED), comma(spec::CAPTURED));
    }
    format!("{} of {} shown", state.kept().len(), spec::ROWS.len())
}

/// A count with thousands separators, the way the reference prints one.
fn comma(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The box a small run of `text` needs, derived from its own length.
///
/// ★ The context strip's first layout gave every part a fixed slot and put the
/// consequence note at a fixed offset inside it; the sweep's first run caught
/// `off` and `4 layers` painted on top of each other. A cursor that advances by
/// what it just placed cannot produce that, whatever the content turns out to
/// be — the fixed slot was a number that had to stay true about strings nobody
/// had measured.
fn run_box(text: &str, x: u32, y: u32) -> Rect {
    Rect::new(x, y, run_width(text), 14)
}

fn context_strip(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let rect = context_rect();
    // ★★★★★ R1852 — the premise now says HOW FAR IT REACHES.
    //
    // The reference keeps this strip on screen at all times because the decode
    // below it is not interpretable without these values, and it names the
    // session they were negotiated for. What it does not say — and what this
    // capture's own hops answer — is that the session it names is ONE of the
    // conversations the table shows, so most rows below are being read against
    // a premise that is not theirs.
    //
    // Derived, never stored: `spec::rows_in_session` counts the hops, so a
    // capture that gains a row moves this sentence with it.
    //
    // ★★★★★ ONE RUN, ONE SENTENCE, AND TWO INKS — and every part of that is a
    // gate's answer rather than a preference. Two attempts were refused by name:
    //
    //  * a SECOND run saying whether the premise covers the selected row. The
    //    ink gate refused it — the strip is a fixed band that the session plus
    //    six negotiated values already fill, so the extra run pushed the last
    //    value 14px past the panel that owns it.
    //  * the SAME run with a different sentence on such a row. The conformance
    //    gate refused that — a declared remainder is one sentence the run must
    //    read, and a run that reads two cannot be declared once.
    //
    // ⇒ the WORDS are the reach, which is true of the capture whatever row is
    // selected, and whether the premise covers THIS row is carried by the ink.
    // Colour alone would be the wrong way to say it, which is why the
    // accessibility value below says it in words: the pair is the rule, not the
    // colour.
    let covered = spec::row_in_session(state.row.get());
    let session = format!(
        "negotiated · session {} · {} of {} rows",
        spec::SESSION,
        spec::rows_in_session(),
        spec::ROWS.len(),
    );
    let session_box = run_box(&session, PAD, 12);
    let mut children = vec![tagged_label(
        "pv.context.session",
        session.clone(),
        session_box,
        FONT_SMALL,
        // `warn` is this screen's role for *present and not to be trusted as it
        // stands*, which is exactly what a decode read against another session's
        // premises is.
        if covered { ink.text_3 } else { ink.warn },
    )];
    let mut x = session_box.x + session_box.w + 24;
    for value in spec::CONTEXT {
        let slug = value.key.replace(' ', "_");
        let key_box = run_box(value.key, x, 12);
        children.push(label(value.key, key_box, FONT_SMALL, ink.text_3));
        x = key_box.x + key_box.w;
        let value_box = run_box(value.value, x, 12);
        children.push(tagged_label(
            &format!("pv.context.{slug}"),
            value.value,
            value_box,
            FONT_SMALL,
            ink.text,
        ));
        x = value_box.x + value_box.w;
        if !value.note.is_empty() {
            let note_box = run_box(value.note, x, 12);
            children.push(label(value.note, note_box, FONT_SMALL, ink.text_3));
            x = note_box.x + note_box.w;
        }
        x += 18;
    }
    panel("pv.context", rect, ink.bg, Some(ink.outline), children)
}

/// The tag the description region is painted and announced under.
const TOOLTIP_TAG: &str = "pv.tip";

/// ★★★★★ R1918 — the sentences this screen's marks carry, by paint tag.
///
/// The canon's rule for WHICH decides the list: **the ones with no room to
/// print what they do**. Five of the seven column headers are abbreviations or
/// symbols the band cannot expand (`sn`, `len`, `from -> to`), and a layer
/// heading is a level number a reader outside this protocol cannot expand at
/// all.
///
/// Every sentence is DERIVED from a declaration the mark is already built from
/// — a column carries its own `description`, a layer carries its own name — so
/// a column added to the specification arrives described, and none of these
/// sentences can drift from the thing it describes.
///
/// ★ The column header's sentence also says the press it takes, which is this
/// screen's own addition over the other two list sections: here a header
/// **cycles the order**, and that is exactly the kind of fact a three-letter
/// title has no room for.
fn descriptions() -> Descriptions {
    let mut described = Descriptions::new();
    for (n, column) in spec::COLUMNS.iter().enumerate() {
        described.describe(
            format!("pv.list.head.{n}"),
            format!("{} - press to sort by it", column.description),
        );
    }
    for (key, title) in spec::LAYERS {
        described.describe(
            format!("pv.tree.layer.{key}"),
            format!("{title} - press to fold this layer away"),
        );
    }
    described
}

/// ★★★★★ R1918 — the description a reader is being shown, as `(tag, sentence)`.
///
/// Resolved from the PAINT REGISTER rather than from [`Hit`]. Both populations
/// here happen to be pressable, so this screen could have gone through the hit
/// test — and deliberately does not, because then the three list sections of
/// one application would resolve the same question two ways, and the two that
/// describe unpressable headers would be the odd ones out.
fn description_shown(state: &Rc<ViewState>, focused: Option<&str>) -> Option<(String, String)> {
    let described = descriptions();
    let (px, py) = state.cursor.get();
    let marks = pinion_core::painted::painted_regions(VIEW_TAG)?;
    let hovered = state
        .pointer_inside
        .get()
        .then(|| described.under(&marks, px, py))
        .flatten();
    let shown = described.shown(&Resting {
        hovered,
        focused,
        dismissed: false,
    })?;
    Some((shown.tag.to_owned(), shown.sentence.to_owned()))
}

/// ★★★★★ R1918 — the register as data, with the mark it is drawn under.
fn described_wire() -> serde_json::Value {
    let described = descriptions();
    serde_json::json!({
        "region": TOOLTIP_TAG,
        "marks": described
            .tags()
            .map(|tag| serde_json::json!({
                "tag": tag,
                "sentence": described.of(tag).unwrap_or_default(),
            }))
            .collect::<Vec<_>>(),
    })
}

/// ★★★★★ R1918 — the description drawn beside the mark it belongs to.
fn description_scene(state: &Rc<ViewState>, ink: Ink) -> Vec<Scene> {
    let Some((tag, sentence)) =
        description_shown(state, pinion_core::focus_state::focused().as_deref())
    else {
        return Vec::new();
    };
    let Some(anchor) =
        pinion_core::painted::painted_regions(VIEW_TAG).and_then(|marks| marks.rect_of(&tag))
    else {
        return Vec::new();
    };
    let (w, h) = window_size();
    vec![pinion_widget_paint::described::view_description(
        TOOLTIP_TAG,
        &sentence,
        anchor,
        Rect::new(0, 0, w, h),
        (0, 0),
        pinion_widget_paint::described::DescriptionStyle::COMPACT,
        pinion_widget_paint::described::DescriptionInk {
            surface: ink.surface,
            outline: Some(ink.outline),
            ink: ink.text,
        },
    )]
}

fn list_pane(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let rect = list_rect();
    let selected = state.row.get();
    let mut children = Vec::new();
    for (n, column) in spec::COLUMNS.iter().enumerate() {
        let col = list_col(n);
        children.push(tagged_label(
            &format!("pv.list.head.{n}"),
            column.title,
            run_band(list_head_seat(), col.x, col.w.saturating_sub(8)),
            FONT_SMALL,
            ink.text_3,
        ));
    }
    // ★★★ R1707 — the rows the query KEPT, laid out by their visual position
    // and tagged by their source index. The tag is the row's identity and the
    // position is where it currently sits; conflating them is how a filtered
    // list starts answering a press with the wrong row.
    for (visual, &n) in state.kept().iter().enumerate() {
        children.extend(list_row_paint(n, visual, selected, ink));
    }
    // ★ One stop for the whole grid — the WAI-ARIA composite pattern, and the
    // one this screen already behaves like: the arrows move the selection
    // *inside* it rather than off it.
    panel(
        "pv.list",
        rect,
        ink.bg,
        Some(ink.outline),
        vec![
            scroll_pane(
                &state.list_scroll,
                panel_content(rect),
                (0, PAD),
                PanePointer::PassesThrough,
                children,
            )
            .silenced(Silence::layout(
                "scrolls the message grid; the rows inside it are what a reader lands on",
            )),
        ],
    )
    .with_focusable(true)
}

/// One message row, in the list pane's own coordinates.
///
/// `n` is the row's index in [`spec::ROWS`] — its identity, which is what every
/// tag carries. `visual` is where it sits in the list right now, which the
/// query decides.
fn list_row_paint(n: usize, visual: usize, selected: usize, ink: Ink) -> Vec<Scene> {
    let message = &spec::ROWS[n];
    let mut children = Vec::new();
    {
        let row = list_row(visual);
        if n == selected {
            children.push(
                box_at("pv.list.selected", row, ink.lit, Some(ink.accent), 0).silenced(
                    Silence::decorative(
                        "the band behind the open message; the row says it is selected",
                    ),
                ),
            );
        }
        children.push(box_at(
            &format!("pv.list.row.{n}"),
            Rect::new(row.x, row.y, row.w, row.h),
            Color::rgba(0, 0, 0, 0),
            None,
            0,
        ));
        let cell = |i: usize| {
            let c = list_col(i);
            run_band(row, c.x, c.w.saturating_sub(8))
        };
        // ★★★★★ R1693 — every cell carries its own tag, which is what makes the
        // list a grid a reader can traverse rather than sixteen paragraphs. The
        // floor's item view answers `cellAt(row, col)` from its model, so a
        // hand-painted table that only tagged its rows would be strictly less
        // navigable than the thing it is meant to beat — measured at 6.11.1,
        // `cellAt(3, 4)` names the cell and reports its row, column and column
        // header, while a custom-painted pane answers one node with no children
        // at all. Tagged on the TEXT rather than in a new box: a container per
        // cell would sit above the row in paint order and take the press the row
        // needs (`scene/pointer_reach` refused exactly that shape in R1692).
        let text = ink.text;
        let texts = cell_texts(n);
        for (c, ink_for) in [
            ink.text_2,
            text,
            ink.text_2,
            ink.text_2,
            kind_ink(message.kind),
        ]
        .into_iter()
        .enumerate()
        {
            children.push(cell_label(n, c, texts[c].clone(), cell(c), ink_for));
        }
        children.extend(name_column_paint(
            n,
            cell(NAME_COLUMN),
            row,
            &texts[NAME_COLUMN],
            ink,
        ));
        children.push(cell_label(n, 6, texts[6].clone(), cell(6), ink.text_2));
    }
    children
}

/// The name column: the row's annotations, then the resource name in what is
/// left of the column after them.
///
/// ★ The name column is shared with the row's annotations, so the annotations
/// are placed FIRST, from the right edge inward, and the name takes what is
/// left. The first draft gave the name the whole column and put the annotations
/// at fixed offsets inside it; the sweep's first run found `piece 1 of 3`
/// painted underneath `First 1/3`. Subtracting what is already placed cannot
/// produce that, whatever the strings turn out to be.
///
/// ★ R1827 — split out of [`list_row_paint`] when a third annotation took that
/// function past the hundred-line bound, which is the same seam and the same
/// reason `tree_row_paint` was split out of `tree_pane` at R1693. Worth having
/// on its own terms: "what shares the name column, and in what order" is one
/// decision, and it was the only part of the row's paint that had to be read as
/// a sequence rather than as a list.
fn name_column_paint(n: usize, name_col: Rect, row: Rect, name: &str, ink: Ink) -> Vec<Scene> {
    let message = &spec::ROWS[n];
    let mut runs = Vec::new();
    let mut right = name_col.x + name_col.w;
    let mut annotation = |runs: &mut Vec<Scene>, suffix: &str, text: String, gap: u32, fg| {
        let width = run_box(&text, 0, 0).w;
        right = right.saturating_sub(width + gap);
        runs.push(
            tagged_label(
                &format!("pv.list.row.{n}.{suffix}"),
                text,
                // ★ R1872 — the ROW is the seat, not one of its edges. This took
                // `y: u32` and could only add a hand-picked offset to it; a band
                // needs the seat's height as well, which is why the parameter
                // changed rather than the arithmetic.
                run_band(row, right, width),
                FONT_SMALL,
                fg,
            )
            // Painted inside the name column and announced as part of that
            // cell: an annotation read as its own stop would tell a reader
            // "out of band" with nothing to attach it to.
            .silenced(Silence::part_of(list_cell_tag(n, NAME_COLUMN))),
        );
    };
    // ★★★★★ R1827 — the exchange this message is half of, in the accent ink so a
    // pair reads as a pair at a glance, and placed FIRST so links line up at the
    // column's right edge across rows — which is how a reader scans a list for
    // them.
    //
    // No row of this capture carries a link AND a note or a fragment, so the
    // order among the three is a choice this round made and not a fact it
    // measured. It is written down as a choice for that reason.
    //
    // A row in no pair paints NOTHING here, rather than a dash or an empty run.
    // That is not a style call either: an empty run is not a painted mark, and
    // the first draft of this round — which made the link a column and pushed an
    // empty label for the unpaired rows — was refused by TWO gates for that
    // alone, re-measured by rebuilding the draft and running the suite against
    // it:
    //
    //   r1663_every_declared_element_of_the_screen_is_painted
    //     `the specification declares 11 element(s) the screen does not paint
    //      and no scroll reaches: ["pv.list.cell.4_7", … "pv.list.cell.15_7"]`
    //   r1693_the_screen_speaks_and_is_quiet_exactly_where_the_specification_says
    //     `the specification declares pv.list.row.2.linked quiet and nothing
    //      paints it`
    //
    // A fact that is absent is absent. ⚠ Eleven, not the fourteen rows that are
    // in no exchange — the gate runs at the DECLARED FLOOR, where four message
    // rows fit, so the three unpaired rows above the fold are a different case
    // from the eleven below it. The count is quoted because it was measured;
    // which of the two cases each row fell into was not, and is not claimed.
    if let Some(link) = spec::link_text(n) {
        annotation(&mut runs, "linked", link, 8, ink.accent);
    }
    if !message.note.is_empty() {
        annotation(&mut runs, "note", message.note.to_owned(), 0, ink.warn);
    }
    if let Some(fragment) = &message.fragment {
        // ★★★★★ R2012 — the non-`Drop` arm was `ink.warn`, and it said the
        // wrong thing. A message that is one piece of a larger one is a FACT
        // about the capture, not a caution about it: the reference paints
        // `First 1/3` and `More 2/3` in an informational tone and keeps the
        // caution tone for what is actually off. Painting both in amber gave a
        // reader two colours for three situations, so the one row that IS a
        // fault (`Drop`) was the only one distinguishable — by being red, not
        // by the other two being calm.
        //
        // The theme had no informational role until this round; that absence
        // is why the arm reached for the nearest tone that was not the error.
        let ink_for = if fragment.marker == "Drop" {
            ink.err
        } else {
            ink.info
        };
        let marker = format!("{} {}", fragment.marker, fragment.piece);
        annotation(&mut runs, "fragment", marker, 8, ink_for);
    }
    runs.push(cell_label(
        n,
        NAME_COLUMN,
        name.to_owned(),
        run_band(row, name_col.x, right.saturating_sub(name_col.x + 8)),
        ink.text,
    ));
    runs
}

/// ★★★★★ R1827 — **the exchange, as a relation an agent can follow.**
///
/// The name cell's annotation says "answers 1182", which is what a person
/// reading a list needs and is not enough for a client: a sequence number is
/// unique per channel, not per capture, so resolving it back to a row would mean
/// re-deriving the pairing the screen already knows. This answers with the ROW,
/// so following the exchange is a lookup rather than a second derivation — and
/// with the row's own role, so a client can tell which end it is holding without
/// re-reading the type column.
///
/// Keyed by row index like `why_hidden`, and for its reason: a map says *these
/// rows and no others*, where a per-row read would make a client ask sixteen
/// times to discover that fourteen answers are empty.
///
/// A function rather than an arm because the arm took `query` past the
/// hundred-line bound. Which is the honest place for it anyway: an arm that
/// builds a structure is a structure with a `match` in front of it.
fn correlation_json() -> serde_json::Value {
    serde_json::Value::Object(
        (0..spec::ROWS.len())
            .filter_map(|n| {
                spec::correlation(n).map(|other| {
                    (
                        n.to_string(),
                        serde_json::json!({
                            "row": other,
                            "sn": spec::ROWS[other].sn,
                            "role": match spec::ROWS[n].kind {
                                "Response" => "reply",
                                _ => "request",
                            },
                        }),
                    )
                })
            })
            .collect(),
    )
}

/// One run's box on this screen: a band tall enough for the screen's face,
/// centred in the seat that holds it.
///
/// ★★★★★ R1875 — **renamed from `cell_band` because it has a second pane
/// now.** The decode tree carried the identical mistake at five more sites, and
/// the whole repair there was to call this. A helper named for the first place
/// it was needed is a helper the next reader does not recognise as theirs —
/// which is how one convention comes to be written out twice.
///
/// ★★★★★ R1872 — **derived, never hand-picked, and that is the repair rather
/// than a bigger number.** Every box in this table was authored
/// `Rect::new(x, seat.y + 5, w, 12)`, and [`pinion_core::containment::line_box`]
/// of `FONT_SMALL` is **18**: the face needs six pixels the box never had, so
/// every descender in the message list was cut. Measured through the integrated
/// shell, that one mistake is 128 runs — the seven column headings, the 112
/// cells, and the nine row annotations — and it was the largest single site in
/// the whole application.
///
/// Writing `18` in place of `12` would have repaired 128 runs and left the
/// height a number somebody types, which is how it became 12 in the first
/// place. The seat's own centre and the face are what the band is now made of,
/// so a face change moves every box in this table and a row-height change moves
/// them too — neither can be forgotten.
///
/// ⚠ The centre is preserved exactly: `seat.y + 5` with height 12 centres at
/// `seat.y + 11`, and `ROW_H` is 22, so this is the same centre with the right
/// height. The header's old `y = 6, h = 12` centres at 12 in a 24-pixel head,
/// likewise. The decode tree R1875 moved onto this had `y + 5` and `12` too,
/// in the same `ROW_H`, so the same sentence covers it — which is the evidence
/// that it was one convention rather than two.
fn run_band(seat: Rect, x: u32, w: u32) -> Rect {
    pinion_core::containment::line_rect_in(seat, x, w, FONT_SMALL)
}

/// The seat the list's column headings sit in — the head strip, in the list
/// pane's own coordinates.
fn list_head_seat() -> Rect {
    Rect::new(0, 0, list_rect().w, HEAD_H)
}

/// The seat the decode tree's title sits in — its head strip, in the tree
/// pane's own coordinates.
///
/// ★ R1875 — named beside its sibling rather than spelled inline, because the
/// two panes' head strips are the same fact and the pair is what makes
/// `HEAD_H` a shared constant rather than a coincidence.
fn tree_head_seat() -> Rect {
    Rect::new(0, 0, tree_rect().w, HEAD_H)
}

/// The tag one message cell is addressed by: the row and the column it is in.
///
/// A function rather than a `format!` at each of the seven sites, because the
/// spelling is a join — [`spec::ROWS`] crossed with [`spec::COLUMNS`] — and both
/// the paint and [`spec::VOICES`] have to produce it from the same rule or the
/// census would be comparing two conventions.
#[must_use]
fn list_cell_tag(row: usize, column: usize) -> String {
    format!("pv.list.cell.{row}_{column}")
}

/// One message cell, tagged so a reader can traverse the grid a column at a
/// time. Its accessible name is the text painted here, which is why nothing
/// re-states the value in the accessibility layer.
/// ★★★★★ R2015 — the figures setting comes from the COLUMN, here, and no
/// caller passes it.
///
/// A cell of `sn` or `len` or `time` is a number a reader compares down the
/// column, and until this round the framework had no way to ask a proportional
/// face for figures of one width — so the sequence numbers did not line up and
/// a gap in them was harder to see than it should be. The declaration lives on
/// [`spec::ColumnSpec::numeric`], one field per column, and this function is
/// the only place it becomes a style: every caller already passes the column
/// index, so none of them can pass the wrong answer or forget to pass one.
fn cell_label(row: usize, column: usize, text: impl Into<String>, rect: Rect, fg: Color) -> Scene {
    let style = run_style(FONT_SMALL, fg).with_numeric(spec::COLUMNS[column].numeric_style());
    text_run(list_cell_tag(row, column), text, rect, style)
}

/// One decode row: its selection band, its fold chevron, its name, its derived
/// badge and its value.
///
/// Split out of [`tree_pane`] at R1693, when the per-run silence declarations
/// took that function past the hundred-line bound. The seam is the one the
/// message list already uses (`list_row_paint`), so the two panes read alike.
fn tree_row_paint(
    state: &Rc<ViewState>,
    n: usize,
    (path, name, value, depth): &(String, String, String, usize),
    selected: &str,
    ink: Ink,
) -> Vec<Scene> {
    let rect = tree_rect();
    let map = state.map.map();
    let folded = state.folded.get();
    let row = tree_row(n);
    let layer = spec::LAYERS.iter().position(|(id, _)| *id == path.as_str());
    let mut children = Vec::new();
    if path == selected {
        children.push(
            box_at("pv.tree.selected", row, ink.lit, Some(ink.accent), 0).silenced(
                Silence::decorative("the band behind the open field; the item says it is selected"),
            ),
        );
    }
    let indent = PAD + u32::try_from(*depth).unwrap_or(0) * TREE_INDENT_STEP;
    if let Some(index) = layer {
        children.push(
            tagged_label(
                &format!("pv.tree.layer.{}", spec::LAYERS[index].0),
                if folded.get(index).copied().unwrap_or(false) {
                    ">"
                } else {
                    "v"
                },
                tree_chevron(n),
                FONT_SMALL,
                ink.text_3,
            )
            // ★ The fold chevron. `v` and `>` are a picture of a state ARIA has
            // a word for, and the item carries it as `aria-expanded` —
            // announcing the glyph too would read a punctuation mark aloud
            // beside the thing it already said.
            .silenced(Silence::part_of(format!("pv.tree.field.{path}"))),
        );
    }
    // ★★★★★ R1875 — the field's name, the badge beside it and the value are
    // three runs of ONE row, and before this each was authored `y + 5` with a
    // height of 12 for a face wanting 18. Three heights and three offsets that
    // nothing related to the face or to each other; now all three are bands of
    // the row, so they share its centre by construction and a face change moves
    // every one of them.
    children.push(tagged_label(
        &format!("pv.tree.field.{path}"),
        name.clone(),
        run_band(row, indent + 6, 128),
        FONT_SMALL,
        if layer.is_some() {
            ink.text
        } else {
            ink.text_2
        },
    ));
    // The badge is placed from the right edge first and the value takes what is
    // left — the same rule the message rows use, for the same reason the sweep
    // found there.
    let mut right = rect.w.saturating_sub(PAD);
    // The derived arm, shown rather than folded into "no bytes": a reader must
    // be able to tell a computed value from one nobody mapped.
    if map.extent_of(path).is_none() && map.field(path).is_some() {
        let width = run_box("derived", 0, 0).w;
        right = right.saturating_sub(width);
        children.push(
            tagged_label(
                &format!("pv.tree.derived.{path}"),
                "derived",
                run_band(row, right, width),
                FONT_SMALL,
                ink.text_3,
            )
            // The badge says this value came from no bytes. That is a fact about
            // the field, announced with it rather than as a separate stop that
            // says only "derived".
            .silenced(Silence::part_of(format!("pv.tree.field.{path}"))),
        );
    }
    children.push(label(
        value.clone(),
        run_band(
            row,
            indent + TREE_VALUE_X,
            right.saturating_sub(indent + TREE_VALUE_X + TREE_VALUE_TAIL),
        ),
        FONT_SMALL,
        ink.text,
    ));
    children
}

fn tree_pane(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let rect = tree_rect();
    let selected = state.field.get();
    let mut children = vec![
        tagged_label(
            "pv.tree.title",
            format!("{}  ·  L0 -> L3", spec::PANES[1].title),
            // ★ R1875 — the head strip is the seat, the same way the message
            // list's headings take `list_head_seat`. `y = 6, h = 12` centred at
            // 12 in a 24px head; so does this, with the height the face needs.
            run_band(tree_head_seat(), PAD, 200),
            FONT_SMALL,
            ink.text_3,
        )
        .silenced(Silence::name_of("pv.tree")),
    ];
    for (n, field) in visible_fields(state).into_iter().enumerate() {
        children.extend(tree_row_paint(state, n, &field, selected.as_str(), ink));
    }
    // One stop for the tree, like the grid beside it.
    panel(
        "pv.tree",
        rect,
        ink.surface,
        Some(ink.outline),
        vec![
            scroll_pane(
                &state.tree_scroll,
                panel_content(rect),
                (0, PAD),
                PanePointer::PassesThrough,
                children,
            )
            .silenced(Silence::layout(
                "scrolls the decode tree; the items inside it are what a reader lands on",
            )),
        ],
    )
    .with_focusable(true)
}

fn bytes_pane(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let rect = bytes_rect();
    let buffer = state.frame_bytes();
    let layout = hex_layout();
    let lit = state.lit_selection();
    let map = state.map.map();
    let marks = map.marks(SourceId::new(0));
    let selected = state.field.get();
    let mut children = vec![
        tagged_label(
            "pv.bytes.title",
            spec::PANES[2].title,
            Rect::new(PAD, 6, 80, 12),
            FONT_SMALL,
            ink.text_3,
        )
        .silenced(Silence::name_of("pv.bytes")),
        tagged_label(
            "pv.bytes.span",
            lit.map_or_else(
                || format!("{selected} · {}", spec::NO_BYTES),
                |sel| {
                    format!(
                        "{selected} · 0x{:02x}..0x{:02x}",
                        sel.start(),
                        sel.end() - 1
                    )
                },
            ),
            Rect::new(PAD + 84, 6, rect.w.saturating_sub(PAD * 2 + 84), 12),
            FONT_SMALL,
            ink.accent,
        ),
    ];
    for row in 0..layout.rows() {
        let y = HEAD_H + u32::try_from(row).unwrap_or(0) * CELL_H;
        // ★★★ R1693 — the offset is this row's HEADER, tagged so it is one.
        // Measured at 6.11.1, a cell in an item view answers `rowHeaderCells`,
        // so a byte pane whose rows had no header would be less locatable than
        // the floor — while the same pane painted by hand there answers a single
        // node with no children at all.
        children.push(tagged_label(
            &bytes_offset_tag(row),
            format!("{:04x}", row * spec::BYTES_PER_ROW),
            Rect::new(PAD, y + 3, 34, 12),
            FONT_MONO,
            ink.text_3,
        ));
    }
    for (byte, value) in buffer.iter().enumerate() {
        let Some(cell) = byte_cell(byte) else {
            continue;
        };
        let inside = lit.is_some_and(|sel| sel.contains(byte));
        if inside {
            children.push(
                box_at(
                    &format!("pv.bytes.lit.{byte}"),
                    Rect::new(cell.x - 1, cell.y + 1, cell.w + 2, cell.h - 2),
                    ink.lit,
                    Some(ink.accent),
                    2,
                )
                .silenced(Silence::decorative(
                    "the highlight behind a byte the open field was read from; the \
                     pane's readout says which bytes those are",
                )),
            );
        }
        children.push(tagged_label(
            &format!("pv.bytes.cell.{byte}"),
            format!("{value:02x}"),
            Rect::new(cell.x, cell.y + 3, cell.w, 12),
            FONT_MONO,
            if inside {
                ink.accent
            } else if marks.top_at(byte).is_some() {
                ink.text
            } else {
                ink.text_3
            },
        ));
    }
    // And one for the byte grid, so the three composites are three stops.
    panel(
        "pv.bytes",
        rect,
        ink.surface,
        Some(ink.outline),
        vec![
            scroll_pane(
                &state.bytes_scroll,
                panel_content(rect),
                (0, PAD),
                PanePointer::PassesThrough,
                children,
            )
            .silenced(Silence::layout(
                "scrolls the byte grid; the rows inside it are what a reader lands on",
            )),
        ],
    )
    .with_focusable(true)
}

/// The tag one row of the byte grid addresses its offset by.
///
/// Named here for the same reason [`list_cell_tag`] is: the paint and
/// [`spec::VOICES`] both produce this spelling, and a second convention would
/// make the census compare two things.
#[must_use]
fn bytes_offset_tag(row: usize) -> String {
    format!("pv.bytes.offset.{row}")
}

/// The tag one row of the byte grid is announced under.
///
/// Nothing paints this: a byte row is the eight cells and the offset beside
/// them, and the row is what a reader descends *through*. It is anchored in the
/// census by the members it composes, which is the exemption the census can
/// check for itself rather than one a screen declares.
#[must_use]
fn bytes_row_tag(row: usize) -> String {
    format!("pv.bytes.row.{row}")
}

/// The strip's totals sentence, in one place because it has two readers.
///
/// ★★★★★ R1845 — the *carrying* number is the CAPTURE's, not the lane roster's.
/// It used to be `LANES.len()` written inline in the painter, so the header
/// announced however many lanes happened to be drawn and called that "carrying".
/// The reference's own strip draws three lanes beside the number four, which is
/// what makes them demonstrably separate facts rather than one fact written
/// twice — and this screen had the smaller one in both places.
///
/// ⚠ Lifted out of the painter because a sentence painted and nowhere else is a
/// claim no test can reach: the label carried this text and the accessibility
/// node beside it carried none, so a counterfactual swapping the two numbers
/// back would have been caught by nothing.
///
/// ★★★★★ **AND THE REFERENCE WAS NOT CONTRADICTING ITSELF.** R1747 recorded
/// this readout as a deliberate divergence, on the finding that the reference
/// announces *four of eight channels* above *three* drawn lanes — "a reader who
/// counts is right and the readout is wrong" — and derived the number from the
/// lanes so "the two cannot part". They are two facts, and this capture is the
/// proof: it carries four channels and the strip draws three lanes, exactly as
/// the reference does. What made them look like one number was the absence of
/// the join this round added — a lane naming no channel could not be compared
/// to a row, so the only count in reach was the roster's. The divergence entry
/// is retired with this sentence, which now reproduces the reference's.
fn reassembly_counts() -> String {
    let (done, running, dropped) = spec::REASSEMBLY;
    format!(
        "priority × delivery · {} of {} channels · done {} · in progress {} · abandoned {}",
        spec::channels().len(),
        spec::CHANNELS,
        comma(done),
        running,
        dropped
    )
}

fn reassembly_strip(ink: Ink) -> Scene {
    let rect = reassembly_rect();
    let mut children = vec![
        tagged_label(
            "pv.reassembly.title",
            "reassembly · sequence continuity per channel",
            Rect::new(PAD, 10, 300, 12),
            FONT_SMALL,
            ink.text_3,
        )
        .silenced(Silence::name_of("pv.reassembly")),
        tagged_label(
            "pv.reassembly.counts",
            reassembly_counts(),
            Rect::new(rect.w.saturating_sub(516), 10, 500, 12),
            FONT_SMALL,
            ink.text_2,
        ),
    ];
    for (n, lane) in spec::LANES.iter().enumerate() {
        let seat = lane_rect(n);
        let local = Rect::new(seat.x, seat.y - rect.y, seat.w, seat.h);
        children.push(box_at(
            &format!("pv.reassembly.lane.{n}"),
            local,
            ink.surface,
            // ★ R1845 — the box is lit by what the lane has to REPORT, not by
            // continuity alone: an abandoned reassembly is a fault a reader must
            // see, and it is not a break in the sequence.
            Some(if lane.faults().is_empty() {
                ink.outline
            } else {
                ink.err
            }),
            4,
        ));
        children.push(label(
            lane.name,
            Rect::new(local.x + 10, local.y + 8, local.w - 20, 12),
            FONT_SMALL,
            ink.text,
        ));
        children.push(label(
            lane_reading(lane),
            Rect::new(local.x + 10, local.y + 24, local.w - 20, 12),
            FONT_SMALL,
            if lane.faults().is_empty() {
                ink.text_2
            } else {
                ink.err
            },
        ));
    }
    panel("pv.reassembly", rect, ink.bg, Some(ink.outline), children)
}

// ── The External ────────────────────────────────────────────────────────────

/// The screen's own oracle: the one `External` every press is delivered to, and
/// the surface the wire drives the screen through.
/// ★★ R1714.1 — and it no longer keeps a size.
///
/// R1656 gave it one because `External::pointer_move` hands a FRACTION and not
/// the rectangle, so a consumer wanting pixels had to hold the basis; R1684.4
/// made the framework answer that and left the field, because the
/// multiplication was still written here. `external::layout_point` carries the
/// whole expression now, and a field written by every resize and read by nobody
/// is what a close audit deletes.
struct ViewOracle {
    state: Option<Rc<ViewState>>,
}

impl core::fmt::Debug for ViewOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ViewOracle")
            .field("attached", &self.state.is_some())
            .finish_non_exhaustive()
    }
}

impl ViewOracle {
    const fn new() -> Self {
        Self { state: None }
    }

    fn attach(&mut self, state: Rc<ViewState>) {
        self.state = Some(state);
    }

    fn state(&self) -> Result<&Rc<ViewState>, InvokeError> {
        self.state
            .as_ref()
            .ok_or_else(|| InvokeError::rejected("the screen is not attached yet"))
    }

    fn text(args: &IntrospectValue) -> Result<String, InvokeError> {
        args.as_str()
            .map(str::to_owned)
            .ok_or_else(|| InvokeError::rejected("expected a string argument"))
    }

    /// ★★★★★ R1787 — **export the range**, through the framework's own
    /// derivation rather than a `join` written here.
    ///
    /// The rows are what each one ANNOUNCES ([`row_cells`]), not what its
    /// columns paint: the note and the fragment marker live inside the name
    /// column rather than in columns of their own, and an export that dropped
    /// them would omit exactly the facts this screen exists to surface. That
    /// choice is also what makes the losses non-hypothetical — the note
    /// `"unknown · 12 B · shown, not decoded"` holds a comma.
    ///
    /// The header line is always written: a capture export lands in a file
    /// somebody opens later, and seven unlabelled columns is the state the
    /// reference floor's binary payload leaves a reader in.
    fn export(
        state: &Rc<ViewState>,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        // R1789 — the shared reader, lifted beside `ArgForm::Object`.
        let obj = ObjectArgs::of(args, "export")?;
        let dialect_name = obj.word("dialect");
        let Some(dialect) = dialect_name.and_then(table_export::Dialect::by_name) else {
            return Err(InvokeError::rejected(format!(
                "no dialect named {:?}; this capture writes {}",
                dialect_name.unwrap_or(""),
                one_of_phrase(table_export::Dialect::NAMED.iter().map(|d| d.name))
            )));
        };
        let scope = obj.word("scope");
        let rows: Vec<usize> = match scope {
            Some("shown") => state.kept(),
            Some("all") => (0..spec::ROWS.len()).collect(),
            other => {
                return Err(InvokeError::rejected(format!(
                    "no scope named {:?}; this capture exports {}",
                    other.unwrap_or(""),
                    one_of_phrase(spec::EXPORT_SCOPES.iter().copied())
                )));
            }
        };
        let headers: Vec<String> = spec::COLUMNS.iter().map(|c| c.title.to_owned()).collect();
        let cells: Vec<Vec<String>> = rows.iter().map(|&n| row_cells(n)).collect();
        let export = table_export::write(dialect, Some(&headers), &cells);
        let mut wire = export.to_wire();
        if let Some(obj) = wire.as_object_mut() {
            obj.insert(
                "dialect".to_owned(),
                serde_json::Value::from(dialect_name.unwrap_or("")),
            );
            obj.insert(
                "scope".to_owned(),
                serde_json::Value::from(scope.unwrap_or("")),
            );
            obj.insert("rows".to_owned(), serde_json::json!(rows));
        }
        Ok(IntrospectValue::Json(wire))
    }
}

impl External for ViewOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn wants_hover_move(&self) -> bool {
        true
    }

    fn pointer_move(&mut self, at: PointerReading) {
        let Some(state) = self.state.clone() else {
            return;
        };
        // ★★ R1714.1 — the framework's expression, not a third copy of it.
        //
        // R1656 gave this the LIVE surface rather than the design constants,
        // which is the half that was wrong then; R1714 moved the whole
        // conversion into `external::layout_point`, so the basis, the clamp and
        // the multiplication are one fact for every self-hit-testing screen.
        // This screen does not pan, and the pan term is the identity for a
        // screen that does not — which is why adopting it is free here and why
        // it will keep being right if this screen ever declares one.
        let (px, py) = pinion_core::external::layout_point(VIEW_TAG, at.at);
        move_cursor(&state, px, py);
    }

    /// ★★★★★ R1700 §5.35 — what a press here addresses, for the framework to
    /// hold against what this screen painted here.
    fn target_at(&self, x: u32, y: u32) -> PointerTarget {
        // ★ R1737 — the framework's own frame conversion, put here for the
        // reason R1714 wrote it: a caller applies it without first asking
        // whether this screen pans, and a screen that omits it has a hit test
        // that is right at one offset and wrong at every other.
        let (x, y) = pinion_core::external::into_layout(VIEW_TAG, (x, y));
        self.state.as_ref().map_or(PointerTarget::Unanswered, |s| {
            Hit::at(s, x, y)
                .word()
                .map_or(PointerTarget::Nothing, PointerTarget::Word)
        })
    }

    /// ★★★★★ R1700 §5.35 — the same question by name. One line over
    /// [`Hit::of_tag`], which R1699 built for keyboard activation and which is
    /// therefore not derived from the geometry this is checked against.
    fn target_of_tag(&self, tag: &str) -> PointerTarget {
        self.state.as_ref().map_or(PointerTarget::Unanswered, |s| {
            Hit::of_tag(s, tag)
                .word()
                .map_or(PointerTarget::Nothing, PointerTarget::Word)
        })
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for ViewOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("spec", "json"),
                    // ★★★★★ R1918 — what the marks on this frame say about
                    // themselves, with the region they are drawn under.
                    SchemaField::new("described", "json"),
                    // ★★★★★ R1845 — the protocol violations this capture
                    // contains, derived. `violation_kinds` publishes the closed
                    // vocabulary beside them so a client enumerates what can be
                    // reported instead of discovering the words from a sample
                    // that happens to contain them.
                    SchemaField::new("violations", "json"),
                    SchemaField::new("violation_kinds", "string"),
                    // ★★★★★ R1852 — the topology built from the hops, and the
                    // two closed vocabularies its answers are drawn from. The
                    // vocabularies are their own slot rather than repeated
                    // inside every row: a client enumerates what an answer can
                    // BE once, instead of discovering the words from whichever
                    // sample this capture happens to be.
                    SchemaField::new("topology", "json"),
                    SchemaField::new("topology_vocabulary", "json"),
                    // ★★★★★ R1747 — how much of the capture viewer's written
                    // specification this build is showing, published beside the
                    // screen's own table. `json` rather than the `string` some
                    // of its neighbours use because it is the framework's own
                    // shape: an agent asking two sections how much of
                    // themselves they are must not have to parse two answers.
                    SchemaField::new("conformance", "json"),
                    SchemaField::new("row_count", "int"),
                    SchemaField::new("selected_row", "int"),
                    SchemaField::new("selected_field", "string"),
                    SchemaField::new("selected_span", "json"),
                    SchemaField::new("visible_fields", "json"),
                    SchemaField::new("saved", "json"),
                    SchemaField::new("folded", "json"),
                    SchemaField::new("said", "object"),
                    // ★★★★★ R1790 — how long what is being said has left.
                    SchemaField::new("saying", "json"),
                    SchemaField::new("cursor", "json"),
                    // ★★★★★ R1772 — where each of the three panes has been
                    // scrolled to. Found by writing this screen's operation
                    // table: the behaviour reference's capture view offers
                    // three scrollable regions, and `Operation::witness` is
                    // mandatory — a row cannot be written for an operation
                    // whose effect nothing publishes. This build held all three
                    // offsets and used them to hit-test, and published none of
                    // them, so an agent could neither cause a scroll nor SEE
                    // that one had happened. Publishing them is what lets those
                    // three rows say `verb: None, gesture: true` honestly
                    // rather than being left out of the table altogether.
                    SchemaField::new("scroll", "json"),
                    // ★★★ R1707 — the filter's whole surface, declared. The
                    // declaration is a PRECONDITION of dispatch (R1637), so an
                    // arm added to `query` and left out here answers
                    // `UnknownIntrospectPath` — which is exactly what this
                    // round's demo hit on its first run.
                    SchemaField::new("query", "string"),
                    SchemaField::new("query_clauses", "json"),
                    SchemaField::new("query_fault", "string"),
                    SchemaField::new("kept_rows", "json"),
                    SchemaField::new("why_hidden", "json"),
                    // ★★★★★ R1827 — **request-response correlation**, the
                    // capability the analysis-tool census carries as
                    // `capture.t1.9`. Declared beside `why_hidden` because it is
                    // the same shape of answer — a map from row index to a fact
                    // about that row, holding only the rows the fact is true of.
                    SchemaField::new("correlation", "json"),
                    // ★★★★★ R1829 — the list's order, in the framework's own
                    // wire vocabulary (`none` / `<col>:ascending` /
                    // `<col>:descending`) rather than a spelling of this
                    // screen's own. A client that can read the order of any
                    // grid in this tree can read this one.
                    //
                    // ★★★★★ **A SLOT NAMES THE FACT AND A VERB NAMES THE ACT,
                    // and finding out that this screen already had that rule is
                    // what settled the shape.** The first draft declared `sort`
                    // twice — as this read AND as an `invoke` action at the same
                    // path — and the wire refused it: `PathIsAReadSlot`, a path
                    // is one or the other. Two ways out were measured.
                    //
                    // The framework's own `GridSortExternal` publishes `sort` as
                    // a read and takes its writes through `intervene`, at the
                    // same path. That was built, and then dropped, for two
                    // reasons that only appear once it is running: an
                    // `intervene` returns unit, so the order, the kept count and
                    // the row now at the top — all three of which `run_sort`
                    // already knows — would cost a client a second round-trip
                    // that its peer `filter` does not; and this screen does not
                    // follow that convention anywhere else. Its filter is an
                    // `invoke` action called `filter` whose FACT is read at
                    // `query`, so the established rule here is a distinct path
                    // per direction with ONE value vocabulary between them.
                    // `sort` + `order` is that rule applied, and switching
                    // idioms halfway down one surface is worse than differing
                    // from a sibling crate.
                    SchemaField::new("sort", "string"),
                    SchemaField::parametric(
                        "hit.<x>.<y>",
                        "string",
                        const { &[SchemaArg::open("x", "int"), SchemaArg::open("y", "int")] },
                    ),
                    // ★★★★★ R1787 — **exporting a range**, the capability the
                    // analysis-tool census carries as `capture.t1.12`. The
                    // dialects are readable BEFORE exporting, each saying
                    // whether it can carry any cell unchanged, and the answer
                    // names every cell its dialect could not — which on this
                    // screen is not hypothetical: a cell of the message list
                    // holds a comma (`reassembled 3,144 B`), so a naive
                    // comma-separated export of this very screen would split a
                    // column.
                    //
                    // ⚠ R1827 removed the number this comment carried. It said
                    // "seven cells", which is what a grep of the fixture
                    // answers — most of those literals belong to the decode
                    // tree, which the message list does not export. R1787's own
                    // demo measured it on the wire, got a different number, and
                    // retracted "seven" in its docstring; this copy of the
                    // sentence, three thousand lines away, kept it. It is
                    // written here with no number at all —
                    // `tools/demos/r1787_an_export_says_what_it_could_not_carry.py`
                    // section B counts it on every run.
                    SchemaField::new("export_dialects", "json"),
                    SchemaField::action_with(
                        "export",
                        "json",
                        ArgForm::Object,
                        const {
                            &[
                                SchemaArg::key("dialect", "string", "export_dialects"),
                                SchemaArg::one_of("scope", "string", spec::EXPORT_SCOPES),
                            ]
                        },
                    ),
                    SchemaField::action("select_message", "int"),
                    SchemaField::action("select_field", "string"),
                    SchemaField::action("select_byte", "int"),
                    SchemaField::action("toggle_saved", "int"),
                    SchemaField::action("toggle_layer", "int"),
                    SchemaField::action("filter", "string"),
                    // R1829 — the act. Takes exactly what the `sort` slot
                    // answers, so a client saves an order and restores it
                    // without translating; see that slot for why the two carry
                    // different paths.
                    SchemaField::action("order", "string"),
                    SchemaField::action("point", "string"),
                    SchemaField::action("press", "string"),
                    // ★ R1664 — declared, not merely handled. The router's press
                    // verb was the one action this screen could be driven by and
                    // the one it did not publish, so `rpc/schema` described a
                    // widget that answered a verb it never mentioned and did not
                    // answer the verb it is actually pressed with.
                    SchemaField::action("send", "string"),
                    SchemaField::action("key", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| ReadRefusal::unavailable("no capture is loaded"))?;
        if let Some(rest) = path.strip_prefix("hit.") {
            let (x, y) = rest.split_once('.').ok_or(ReadRefusal::QueryTypeMismatch)?;
            let (px, py) = (
                x.parse().map_err(|_| ReadRefusal::QueryTypeMismatch)?,
                y.parse().map_err(|_| ReadRefusal::QueryTypeMismatch)?,
            );
            return Ok(IntrospectValue::Text(
                Hit::at(state, px, py)
                    .word()
                    .unwrap_or_else(|| "none".to_owned()),
            ));
        }
        match path {
            "spec" => Ok(IntrospectValue::Json(spec_json())),
            // ★★★★★ R1918 — what the marks on this frame say about themselves.
            "described" => Ok(IntrospectValue::Json(described_wire())),
            // ★ R1747 — the SAME value the host publishes for this section, so
            // "one build, two placements" is a fact a client can check rather
            // than a claim this file makes.
            "conformance" => Ok(IntrospectValue::Json(pinion_shell::conformance_json::<
                PacketView,
            >())),
            "row_count" => Ok(IntrospectValue::Int(
                i64::try_from(spec::ROWS.len()).unwrap_or(i64::MAX),
            )),
            // ★★★★★ R1845 — the census's `capture.t2.18`. DERIVED from the
            // capture rather than stored beside it, so a violation cannot drift
            // from the rows it is about; every ingredient was already in
            // `RowSpec` and nothing had asked.
            "violations" => Ok(IntrospectValue::Json(violations_json())),
            "violation_kinds" => Ok(IntrospectValue::Text(spec::VIOLATION_KINDS.join(","))),
            // ★★★★★ R1852 — the topology this capture SHOWS, with no management
            // API asked. `pinion_node_graph::SightedTopology` builds it from the
            // hops and the whole answer is on the wire rather than in a picture,
            // because §2 #7 is that a scene is queryable as text and because a
            // topology map is a later release's widget while this capability is
            // not.
            //
            // ⚠ Both arms answer through one helper because clippy's line limit
            // named a real thing: this `query` is at the limit, and a slot added
            // here should not have to argue with the size of a function about
            // every other slot. The lint's repair is the split, not an allow.
            "topology" | "topology_vocabulary" => Ok(IntrospectValue::Json(topology_slot(path))),
            "selected_row" => Ok(IntrospectValue::Int(
                i64::try_from(state.row.get()).unwrap_or(i64::MAX),
            )),
            "selected_field" => Ok(IntrospectValue::Text(state.field.get())),
            "selected_span" => Ok(state.lit_selection().map_or(IntrospectValue::Null, |sel| {
                IntrospectValue::Json(serde_json::json!({
                    "start": sel.start(),
                    "end": sel.end(),
                }))
            })),
            "visible_fields" => Ok(IntrospectValue::Json(serde_json::Value::Array(
                visible_fields(state)
                    .into_iter()
                    .map(|(p, ..)| serde_json::Value::String(p))
                    .collect(),
            ))),
            "saved" => Ok(IntrospectValue::Json(serde_json::json!(state.saved.get()))),
            // ★★★ R1707 — the query as the person wrote it, which the reference
            // floor cannot give back: measured at 6.11.1, handing its
            // row-filtering proxy `sensors/unit/*` and reading the filter
            // returns `(?s:sensors/unit/[^/]*)`.
            "query" => Ok(IntrospectValue::Text(state.query.text())),
            "query_clauses" => Ok(IntrospectValue::Json(serde_json::Value::Array(
                state
                    .query()
                    .clauses()
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "column": c.column,
                            "op": c.op.wire_token(),
                            "operand": c.operand,
                            "text": c.text,
                        })
                    })
                    .collect(),
            ))),
            "query_fault" => Ok(state
                .query_fault()
                .map_or(IntrospectValue::Null, IntrospectValue::Text)),
            "kept_rows" => Ok(IntrospectValue::Json(serde_json::json!(state.kept()))),
            // ★ R1829 — `grid_sort_str` and not a `format!` here. The wire form
            // is the framework's, and the only way it stays the framework's is
            // for this arm to ask the framework what it is.
            "sort" => Ok(IntrospectValue::Text(grid_sort_str(state.sort.get()))),
            // ★ R1787 — the dialects `export` writes, each saying whether it
            // can carry any cell unchanged. Derived from the framework's one
            // roster, so this screen cannot offer a dialect the writer lacks.
            "export_dialects" => Ok(IntrospectValue::Json(table_export::dialects_to_wire())),
            // ★★★★★ R1707 — **why a message is not in the list.**
            //
            // The question a capture viewer's reader actually has, and the one
            // the reference floor answers with an invalid model index — the
            // same answer for every reason a row can be absent. Across its
            // row-filtering proxy's 12 properties and 101 methods, measured,
            // not one names a reason.
            "why_hidden" => Ok(IntrospectValue::Json(serde_json::Value::Object(
                (0..spec::ROWS.len())
                    .filter_map(|n| {
                        state
                            .why_hidden(n)
                            .map(|clause| (n.to_string(), serde_json::json!(clause)))
                    })
                    .collect(),
            ))),
            // ★★★★★ R1827 — **the exchange, as a relation an agent can follow.**
            //
            // The `linked` column shows one sequence number, which is what a
            // person reading a list needs and is not enough for a client: a
            // sequence number is unique per channel, not per capture, so
            // resolving it back to a row means re-deriving the pairing the
            // screen already knows. This answers with the ROW, so following the
            // exchange is a lookup rather than a second derivation — and with
            // the row's own role, so a client can tell which end it is holding
            // without re-reading the type column.
            //
            // Keyed by row index like `why_hidden`, and for its reason: a map
            // says *these rows and no others*, where a per-row read would make
            // a client ask sixteen times to discover that fourteen answers are
            // empty.
            "correlation" => Ok(IntrospectValue::Json(correlation_json())),
            "folded" => Ok(IntrospectValue::Json(serde_json::json!(state.folded.get()))),
            // ★★★★★ R1719 — `said` answers the VALUE now, not the sentence, and
            // it is spelled `said` on all three screens of this tool. It used
            // to be a string, and the one reader of it outside this file asked
            // for `["sentence"]` instead — cheaper than three screens spelling
            // one concept three ways, which is the defect one level up from the
            // one this round is about.
            "said" => Ok(IntrospectValue::Json(match state.said.showing() {
                Some(said) => serde_json::to_value(&said).map_err(|_| ReadRefusal::UnknownPath)?,
                None => serde_json::Value::Null,
            })),
            // ★★★★★ R1790 — the sentence AND how long it has, so a gate advances
            // time by asking rather than by guessing a number this screen owns.
            "saying" => Ok(IntrospectValue::Json(state.said.to_wire())),
            "cursor" => {
                let (x, y) = state.cursor.get();
                Ok(IntrospectValue::Json(serde_json::json!({"x": x, "y": y})))
            }
            // ★ R1772 — all three at once rather than three paths, because the
            // question a reader has is *where is this screen scrolled to*, and
            // three reads that could be taken a frame apart answer it worse.
            "scroll" => {
                let pane = |scroll: &ScrollState| {
                    let (x, y) = scroll.offset();
                    serde_json::json!({ "x": x, "y": y })
                };
                Ok(IntrospectValue::Json(serde_json::json!({
                    "list": pane(&state.list_scroll),
                    "tree": pane(&state.tree_scroll),
                    "bytes": pane(&state.bytes_scroll),
                })))
            }
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        if self.query(path).is_ok() {
            Err(InterveneError::ReadOnly)
        } else {
            Err(InterveneError::UnknownPath)
        }
    }

    /// ★★★★★ R1720 — the refusal an agent was handed, put in front of the
    /// person watching this screen.
    ///
    /// Measured before this round: **0 of this screen's 9 refusing verbs**
    /// changed anything a person could see. Its only refusal a person ever met
    /// was the one the filter box announces as they type, so an agent that sent
    /// a malformed query got a sentence and the person beside it got nothing —
    /// while the list they were looking at stayed exactly as it was.
    fn announce(&mut self, refused: &Utterance) -> Announced {
        let Some(state) = self.state.as_ref() else {
            return Announced::nowhere("no capture is loaded, so there is no app bar to say it in");
        };
        state.say(refused.clone());
        Announced::at("pv.appbar.said")
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.state()?.clone();
        match path {
            // ★★★★★ R1787 — export the range a person is looking at.
            "export" => Self::export(&state, &args),
            "select_message" => {
                let n = args
                    .as_usize()
                    .ok_or_else(|| InvokeError::rejected("expected a row index"))?;
                if n >= spec::ROWS.len() {
                    return Err(InvokeError::rejected(format!(
                        "no message {n} was captured"
                    )));
                }
                select_message(&state, n);
                Ok(IntrospectValue::Int(i64::try_from(n).unwrap_or(i64::MAX)))
            }
            "select_field" => {
                let want = Self::text(&args)?;
                let want = want.trim();
                if state.map.map().field(want).is_none() {
                    return Err(InvokeError::rejected(format!(
                        "this decode has no field at {want:?}"
                    )));
                }
                select_field(&state, want);
                Ok(IntrospectValue::Text(want.to_owned()))
            }
            "select_byte" => {
                let b = args
                    .as_usize()
                    .ok_or_else(|| InvokeError::rejected("expected a byte offset"))?;
                select_byte(&state, b);
                Ok(IntrospectValue::Text(state.field.get()))
            }
            "toggle_saved" => {
                let n = args
                    .as_usize()
                    .ok_or_else(|| InvokeError::rejected("expected a filter index"))?;
                if n >= spec::SAVED_FILTERS.len() {
                    return Err(InvokeError::rejected(format!("no saved filter {n}")));
                }
                toggle_saved(&state, n);
                Ok(IntrospectValue::Json(serde_json::json!(state.saved.get())))
            }
            "filter" => run_filter(&state, &Self::text(&args)?),
            "order" => run_sort(&state, &Self::text(&args)?),
            "toggle_layer" => {
                let n = args
                    .as_usize()
                    .ok_or_else(|| InvokeError::rejected("expected a layer index"))?;
                if n >= spec::LAYERS.len() {
                    return Err(InvokeError::rejected(format!("no layer {n}")));
                }
                toggle_layer(&state, n);
                Ok(IntrospectValue::Json(serde_json::json!(state.folded.get())))
            }
            "point" => {
                let raw = Self::text(&args)?;
                let (x, y) = raw
                    .trim()
                    .split_once(',')
                    .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <x>,<y>")))?;
                let px = x
                    .trim()
                    .parse()
                    .map_err(|_| InvokeError::rejected(format!("{x:?} is not a coordinate")))?;
                let py = y
                    .trim()
                    .parse()
                    .map_err(|_| InvokeError::rejected(format!("{y:?} is not a coordinate")))?;
                move_cursor(&state, px, py);
                Ok(IntrospectValue::Text(format!("{px},{py}")))
            }
            "press" => {
                press(&state);
                Ok(IntrospectValue::Text(state.said_sentence()))
            }
            // ★★★★ R1664 — `send` is the verb the §5.35 ROUTER presses with,
            // and it is the second half of this screen's deafness.
            //
            // Routing a press is two joins, and R1663 got neither: the painted
            // root has to carry the tag the widget is registered under (the
            // repair in `view` above), and the widget has to answer the verb
            // `dispatch_send` dispatches — `invoke("send", "PointerDown")`. This
            // screen spelled its own action `press`, so once the first join was
            // repaired the router resolved the widget, called `send`, got
            // `UnknownPath` back, and **discarded it** (`let _ = intro.invoke(…)`
            // in `pinion_runtime::input::dispatch_send`): a press that arrives,
            // is refused, and leaves no trace anywhere.
            //
            // Measured before and after this arm existed, driving `scene/click
            // {at}` — the wire entry point that goes through the router — rather
            // than `invoke("press")`, which is the oracle and was never broken.
            "send" => {
                let event = Self::text(&args)?;
                match event.trim() {
                    "PointerDown" => press(&state),
                    // A release commits nothing on this screen: selection is
                    // decided at press, and there is no drag. Accepted rather
                    // than rejected, because the router sends the pair and a
                    // widget that refuses half of it is the shape above.
                    "PointerUp" | "PointerEnter" => {}
                    "PointerLeave" | "PointerCancel" => {
                        // ★ R1918 — the pointer is GONE, which is a different
                        // fact from where it last was, and it is what takes a
                        // resting description off the frame.
                        state.pointer_inside.set(false);
                    }
                    other => {
                        return Err(InvokeError::rejected(format!(
                            "{other:?} is not a pointer event; they are PointerDown / \
                             PointerUp / PointerEnter / PointerLeave / PointerCancel"
                        )));
                    }
                }
                Ok(IntrospectValue::Text(state.said_sentence()))
            }
            "key" => {
                let chord = Self::text(&args)?;
                Ok(IntrospectValue::Bool(key(&state, chord.trim())))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// The derived violation table in its wire form.
///
/// ★ R1845 — lifted out of `query`'s arm rather than shaped inside it. `query`
/// is a DISPATCH table: an arm that also builds a document reads as if the shape
/// were part of the routing, and the arm's twelve lines took the method past
/// `clippy::too_many_lines`. The lint named a real thing — the two jobs had run
/// together — so the repair is the split and not an allow.
/// Either topology slot's answer, by path.
///
/// The vocabularies are their OWN slot rather than repeated inside every row: a
/// client enumerates what an answer can BE once, instead of discovering the words
/// from whichever sample this capture happens to be.
fn topology_slot(path: &str) -> serde_json::Value {
    if path == "topology_vocabulary" {
        return serde_json::json!({
            "vantage": pinion_node_graph::Vantage::WIRE_NAMES,
            "sighted": pinion_node_graph::Sighted::WIRE_NAMES,
        });
    }
    topology_json()
}

/// The topology this capture shows, as one value a client reads in a round trip.
///
/// ★★★★★ R1852 — §2 #7, on the axis the reference has no answer for. Every
/// endpoint carries its VANTAGE and every direction its SIGHTED verdict, and the
/// second is three-valued on purpose: `seen`, `not_seen` between two endpoints
/// this capture knows, and `unknown` for a name it never mentioned. Probed
/// against the toolkit floor at 6.11.1, a tabular model's cell accessor answers
/// one empty value for *nothing here* and for *nothing known here*, so a client
/// there cannot tell the two apart at all.
///
/// `standing` is `partial` by construction and says so with its reason: nothing
/// drawn accounts for any of these directions, because there is no drawing.
fn topology_json() -> serde_json::Value {
    let seen = spec::sighted_topology();
    let (a, b) = spec::session_endpoints();
    serde_json::json!({
        "endpoints": seen.endpoints().iter().map(|name| serde_json::json!({
            "name": name,
            "vantage": seen.vantage(name).map(pinion_node_graph::Vantage::name),
            "sends_to": seen.degree(name).map(|(out, _)| out),
            "hears_from": seen.degree(name).map(|(_, into)| into),
            "peers": seen.peers(name),
        })).collect::<Vec<_>>(),
        "edges": seen.edges().map(|(from, to, times)| serde_json::json!({
            "from": from, "to": to, "times": times,
        })).collect::<Vec<_>>(),
        "conversations": seen.conversations().iter().map(|(x, y)| serde_json::json!({
            "a": x, "b": y,
        })).collect::<Vec<_>>(),
        "sightings": seen.sightings(),
        "standing": seen.standing().name(),
        "why_partial": seen.standing().to_string(),
        // ★ THE PREMISE AND ITS REACH, which is what made this worth building:
        // the strip states a negotiated context and this says how much of the
        // table that context is about.
        "negotiated_session": { "a": a, "b": b },
        "negotiated_is_one_of": seen.conversations().len(),
        "rows_in_session": spec::rows_in_session(),
        "rows": spec::ROWS.len(),
        // ⚠ And the three-valued answer, exercised on the wire rather than only
        // described: the reverse of a seen direction, and a name no hop holds.
        "probe": {
            "seen": seen.sighted(a, b).name(),
            "reverse": seen.sighted(b, a).name(),
            "stranger": seen.sighted("no-such-endpoint", b).name(),
        },
    })
}

fn violations_json() -> serde_json::Value {
    serde_json::Value::Array(
        spec::violations()
            .into_iter()
            .map(|v| {
                serde_json::json!({
                    "kind": v.kind,
                    "row": v.row,
                    "why": v.why,
                })
            })
            .collect(),
    )
}

/// The whole specification, as the wire sees it — so the demo reads the table
/// from the running application rather than keeping a second copy of it.
fn spec_json() -> serde_json::Value {
    // ★★★★★ R1814 — the described message's frame, read once so each field can
    // be asked whether the bytes actually there decode as it declares. This is
    // the screen checking its OWN claim and publishing the answer, rather than
    // publishing the claim and leaving a reader to trust it.
    let described = frame_bytes(spec::OPENING_ROW);
    serde_json::json!({
        // ★★★★★ R1747 — the specification this screen is JUDGED against,
        // published beside the tables it is BUILT from, and they are two
        // different documents on purpose.
        //
        // Everything else in here is `crate::spec` — the screen's own table,
        // written in the same edit as the painter it feeds. This is
        // `docs/analyzer-packets-spec.json`, extracted from the behaviour
        // reference by another hand, and it is what `conformance` compares the
        // paint with. Published because a report of counts cannot answer *what
        // is your verdict about*: a surface's standing says how many parts were
        // specified and not which.
        //
        // Nested under a name of its own rather than merged, for a reason this
        // round measured: R1738's gate looks up a surface by name in whatever a
        // section publishes, and the pin's `context` surface and this screen's
        // own `context` table would have collided at the top level — two
        // different documents answering to one word.
        "packets": spec::packets_document().to_json(),
        // ★★★★★ R1860 — and each pane's FLOOR beside the width it is drawn at,
        // because the two stopped being one number. A consumer that reads only
        // `width` can ask "is this pane the width it declares?", which was the
        // right question while the panes could not flex and is the wrong one
        // now; with the floor published it can ask the question that is still
        // true — a pane is painted somewhere between the width it can draw in
        // and the width it is drawn at. Published rather than derived, because
        // the shortfall a window is short by is not a fact a client can compute.
        "panes": spec::PANES.iter().enumerate().map(|(n, p)| serde_json::json!({
            "tag": p.tag, "title": p.title, "width": p.width, "body": p.body,
            "floor": pane_floor(n),
        })).collect::<Vec<_>>(),
        "columns": spec::COLUMNS.iter().map(|c| serde_json::json!({
            "title": c.title, "width": c.width,
        })).collect::<Vec<_>>(),
        "kinds": spec::KINDS,
        "context": spec::CONTEXT.iter().map(|c| serde_json::json!({
            "key": c.key, "value": c.value, "note": c.note,
        })).collect::<Vec<_>>(),
        "saved_filters": spec::SAVED_FILTERS.iter().map(|f| serde_json::json!({
            "name": f.name, "query": f.query,
        })).collect::<Vec<_>>(),
        // ★★★ R1707 — what this screen tells a person the mouse and keyboard
        // do. Published rather than painted: the sibling screen prints a hint
        // strip because the reference's node canvas does, and the reference's
        // capture section does not — but a promise nobody can enumerate is one
        // no gate can hold the screen to, which is the state this screen was in.
        "gestures": spec::GESTURES.iter().map(|(g, effect)| serde_json::json!({
            "gesture": g, "effect": effect,
        })).collect::<Vec<_>>(),
        // ★★★★★ R1772 — the operations the behaviour reference's capture view
        // offers, and by which of the two causes THIS build reaches each.
        // Published rather than kept in the source for the reason its two
        // siblings are: a demo carrying its own copy of the list would be
        // checking the list against itself. `absent` is DERIVED so an operation
        // cannot be declared missing and reachable at once.
        "operations": spec::OPERATIONS.iter().map(|op| serde_json::json!({
            "name": op.name,
            "verb": op.verb.map(|(verb, arg)| serde_json::json!([verb, arg])),
            "gesture": op.gesture,
            "witness": op.witness,
            "needs": op.needs,
            "absent": !op.reachable(),
        })).collect::<Vec<_>>(),
        "query_columns": spec::QUERY_COLUMNS,
        "query_placeholder": spec::QUERY_PLACEHOLDER,
        "example_query": spec::EXAMPLE_QUERY,
        "layers": spec::LAYERS.iter().map(|(id, title)| serde_json::json!({
            "id": id, "title": title,
        })).collect::<Vec<_>>(),
        // ★★★★★ R1814 — and each field now says whether its bytes ENCODE the
        // value beside them, which is the question this screen exists to answer
        // and could not. `encodes` is the fact; `undeclared_because` is the
        // reason when it is false, because a reader — a person or an agent —
        // seeing a hex pane reads it as the encoding of the value above it, and
        // most of this table is a capture the specification cannot decode.
        // Publishing the reason rather than only the bit is what makes the
        // difference between a limitation and a silence. (How many is answered
        // by `cargo test -p hello-packet-view r1814 -- --nocapture`; this
        // round's audit caught a hand-written count here that was wrong.)
        "fields": spec::FIELDS.iter().map(|f| serde_json::json!({
            // ★★★★★ R2011 — `value` is what the ROW SHOWS, which is the tree's
            // own call and not this table's string. They differ wherever a
            // declaration renders itself, and an agent that read one while a
            // person read the other would have no way to notice.
            "path": f.path, "name": f.name, "value": f.shown_value(),
            "source": f.source, "at": f.at, "len": f.len,
            "encodes": f.wire.is_declared(),
            "undeclared_because": match f.wire {
                spec::Wire::Undeclared(why) => Some(why),
                _ => None,
            },
            // ★★★★★ The THIRD direction, on the wire: bytes to value. `encodes`
            // is what the table CLAIMS; this is the painted frame decoded and
            // compared against that claim, so an agent can tell a declaration
            // from a decoration without rendering anything.
            "reads_back": f.source == Some(0)
                && described
                    .get(f.at..f.at + f.len)
                    .is_some_and(|bytes| f.wire.reads(bytes)),
        })).collect::<Vec<_>>(),
        "rows": spec::ROWS.iter().map(|r| serde_json::json!({
            "time": r.time, "hop": r.hop, "channel": r.channel, "sn": r.sn,
            "kind": r.kind, "name": r.name, "len": r.len, "note": r.note,
            "fragment": r.fragment.as_ref().map(|f| serde_json::json!({
                "marker": f.marker, "piece": f.piece,
            })),
        })).collect::<Vec<_>>(),
        // ★★★★★ R1845 — a lane publishes the channel it is about and the
        // reading DERIVED from that channel's rows, so a client holding both
        // halves of this document can check the strip against the capture. It
        // could not before: the lane named no channel.
        "lanes": spec::LANES.iter().map(|l| serde_json::json!({
            "name": l.name, "channel": l.channel, "sn": l.sn(),
            "continuous": l.continuous(), "dropped": l.dropped(),
            "skipped": l.skipped(), "out_of_order": l.out_of_order(),
        })).collect::<Vec<_>>(),
        "channels_carrying": spec::channels(),
        // ★ R1845 — the premise the third violation kind is judged against. A
        // client told an extension was "unnegotiated" and not told what WAS
        // negotiated has been handed a verdict it cannot check.
        "negotiated_extensions": spec::NEGOTIATED_EXTENSIONS,
        "sources": spec::SOURCES.iter().map(|(n, l)| serde_json::json!({
            "name": n, "len": l,
        })).collect::<Vec<_>>(),
        "matched": spec::MATCHED,
        "captured": spec::CAPTURED,
        "opening_row": spec::OPENING_ROW,
        "opening_field": spec::OPENING_FIELD,
        // ★★★★★ R1693 — the declared split, **already expanded**. A client
        // reading this gets the tags and the roles rather than a table plus the
        // rule for reading it, so a demo checking what a reader is told does not
        // carry a second copy of the populations — and a family that grows a
        // member grows here, where the demo is looking.
        "voices": spec::VOICES.iter().flat_map(|v| {
            v.population.members().into_iter().map(|member| serde_json::json!({
                "tag": v.tag.replace("{}", &member), "role": v.role,
            }))
        }).collect::<Vec<_>>(),
        "silences": spec::SILENCES.iter().flat_map(|(tag, population, kind)| {
            population.members().into_iter().map(move |member| serde_json::json!({
                "tag": tag.replace("{}", &member), "kind": kind,
            }))
        }).collect::<Vec<_>>(),
    })
}

// ── The binding ─────────────────────────────────────────────────────────────

/// ★ R1729 — public, because this screen is now both a window of its own and a
/// **page** of the analysis-tool shell (`pinion_screen::Mount<PacketView>`).
/// The binding is unchanged either way; only who builds it differs.
pub struct PacketView;

impl WidgetCore for PacketView {
    /// ★★★ R1707 — the query box's posture and caret, which the shell reads out
    /// of the painted scene and hands back to the view.
    ///
    /// The same contract screen A and the node editor use, so the field's own
    /// external stays the authority on what it holds and this screen never
    /// guesses. It was `()` while this screen had no text entry anywhere, and
    /// that is exactly how long its filter bar was a painted constant.
    type State = (TextFieldState, u32);
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = ViewOracle::new();
        oracle.attach(use_view_state());
        Box::new(oracle)
    }

    /// The byte map is published as its own External, because the relation is
    /// the crate's and a consumer that re-published it here would be the second
    /// copy this round removed.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(
                MAP_TAG,
                Box::new(ByteMapExternal::new(Rc::clone(&use_view_state().map))),
            ),
            // ★★★ R1707 — the thing that HOLDS the query text, owns focus and
            // takes a keystroke. Measured on the sibling screen while it was
            // wired: without this the box paints, the screen reports itself
            // editing, and every keystroke is refused, because the keymap
            // forwards to an external that is not there.
            pinion_core::widgets::text_field::blur_committing_field_extra(QUERY_TAG),
        ]
    }

    fn tag() -> &'static str {
        VIEW_TAG
    }

    /// ★★★★★ R1911 — this screen's marks are addressed under `pv.`, not under
    /// its root tag: measured over the wire at 1440x900, ONE node carries
    /// `packet_view` and 292 carry a `pv.` address. A host asking where this
    /// section is from `tag()` alone was asking about a marker node.
    fn paint_stems() -> Vec<&'static str> {
        vec![VIEW_TAG, "pv"]
    }

    fn read_state(scene: &Scene) -> (TextFieldState, u32) {
        tf_paint::read_text_field_state(scene, QUERY_TAG)
    }

    fn view(state: (TextFieldState, u32), frame: &Frame) -> Scene {
        view(state, *frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-packet-view (R1663 §5.41 field-to-bytes capture viewer)"
    }

    /// ★★ R1698 — `focused` is threaded through rather than dropped.
    ///
    /// It was `_focused`, and the census across the tree says that is the
    /// minority position: 135 of 172 `apply_key` implementations read it. What
    /// dropping it cost here was measured on the running screen — at all six
    /// Tab stops, `ArrowDown` moved the message list, including from the decode
    /// tree and the byte grid, which have cursors of their own.
    ///
    /// It goes over the wire as a second argument rather than through a second
    /// channel, so the RPC `invoke("key", …)` path and a real key press reach
    /// the same function with the same information.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        chord: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        // ★★★ R1707 — while the query box has focus every key is the box's,
        // through the framework's own keymap rather than a fifth copy of one.
        // The screen's own chords are deliberately unreachable here: a person
        // typing `type in (Data, Query)` has to be able to type a space, and
        // this screen binds Space.
        if focused == Some(QUERY_TAG) {
            let state = use_view_state();
            return edit_field_keymap(
                scene,
                QUERY_TAG,
                chord,
                modifiers,
                CellKind::Text,
                // The filter is already live — the list re-derives from this
                // very buffer on every keystroke — so Enter has nothing to
                // apply. What it does is SAY where the query got to, which is
                // the one thing typing does not announce.
                || announce_query(&state),
                || {},
            );
        }
        key_at(&use_view_state(), focused, chord)
    }
}

impl WidgetA11y for PacketView {
    /// ★★★★★ R1698 — where the cursor rests inside the focused pane.
    ///
    /// The panes announce themselves as a `grid`, a `tree` and a `grid`, and
    /// every one of those roles' contract is that the arrows move a cursor an
    /// assistive technology can follow. Measured before this existed: the
    /// active descendant was `None` at every stop, so a reader was told the
    /// list had focus and never which row.
    fn access_focus_target(
        _state: &(TextFieldState, u32),
        focused: Option<&str>,
    ) -> Option<AccessFocus> {
        let stop = focused?;
        let state = use_view_state();
        // ★★★★★ R1699 — the INNERMOST tag. ARIA's `aria-activedescendant`
        // addresses any descendant of the element owning the Tab stop, and the
        // framework's focus ring reads this same hook, so a reader who has gone
        // into a row is framed on the cell rather than on the row.
        let cursor =
            pane_cursor(&state, stop).and_then(|r| r.active_descendant().map(str::to_owned));
        Some(AccessFocus::addressing(stop, cursor))
    }

    /// ★★★★★ R1693 — **the screen, announced.**
    ///
    /// It announced three nodes until this round: a `table` with no row, a
    /// `tree` with no item, and a `group`. The 186 painted regions behind them
    /// — sixteen messages of seven columns each, twenty-one decoded fields over
    /// four layers, seventy-two bytes, the filter, the negotiated context and
    /// the reassembly lanes — were not in the accessibility tree at all, and
    /// every check in this example was green because a region with no node
    /// paints perfectly and answers every question about its rectangle.
    ///
    /// The two collection roles are the sharper half: announcing `table` and
    /// holding nothing tells a reader a table is there and gives them nothing to
    /// enter. `scene/conform` is what refuses that now.
    ///
    /// Built pane by pane below, and each pane's population comes from the same
    /// `spec` table the painter reads — so what a reader hears and what is drawn
    /// cannot drift, and `spec::VOICES` can expand the families and check both
    /// directions.
    fn access_node(_state: &(TextFieldState, u32), focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_view_state();
        let mut nodes = app_bar_nodes(&state);
        nodes.extend(filter_nodes(&state));
        nodes.extend(context_nodes(&state));
        nodes.extend(list_nodes(&state));
        nodes.extend(tree_nodes(&state));
        nodes.extend(bytes_nodes(&state));
        nodes.extend(reassembly_nodes());
        // ★★★★★ R1698 — each pane publishes the cursor its arrows move, in one
        // place rather than at three builders. What it publishes is NOT the
        // node's children: the tree's children are its visible fields (which
        // happens to match) while the byte grid's are its ROWS and the cursor
        // walks its cells. A client reading `children` to learn what the arrows
        // reach would be told rows and given cells.
        //
        // ★★★★★ R1699 — and a member that is ITSELF a composite publishes its
        // own roster, so a client can ask what is inside a message row without
        // first moving the selection onto it. Built once and indexed rather
        // than asked per node: `pane_cursor` seats sixteen rows of seven cells,
        // and asking it for each of ~290 nodes would rebuild that roster ~870
        // times a frame.
        let cursors: Vec<(&str, Roving)> = PANE_STOPS
            .iter()
            .filter_map(|stop| pane_cursor(&state, stop).map(|roving| (*stop, roving)))
            .collect();
        let nested: BTreeMap<&str, &Roving> = cursors
            .iter()
            .flat_map(|(_, roving)| {
                roving
                    .members()
                    .iter()
                    .filter_map(|m| m.inner().map(|inner| (m.tag.as_str(), inner)))
            })
            .collect();
        for node in &mut nodes {
            if let Some((_, roving)) = cursors.iter().find(|(stop, _)| *stop == node.tag) {
                *node = node.clone().with_navigation(roving);
            } else if let Some(inner) = nested.get(node.tag.as_str()) {
                *node = node.clone().with_navigation(inner);
            }
        }
        // ★★★★★ R1918 — and the description a reader is resting on, tied to the
        // mark it belongs to through `aria-describedby`. After the navigation
        // pass, because that pass rewrites nodes by tag and the region is not
        // one of them.
        // ★ R1918 — the parameter lost its underscore here, which is the honest
        // report of a change: this screen ignored where the keyboard was, and
        // now it does not.
        if let Some((tag, sentence)) = description_shown(&state, focused) {
            pinion_widget_paint::described::announce_description(
                &mut nodes,
                &tag,
                TOOLTIP_TAG,
                &sentence,
            );
        }
        nodes
    }
}

/// The three panes that own a keyboard cursor.
///
/// A list rather than three literals because R1699 needed to walk them twice —
/// once to publish each pane's roster and once to publish the rosters of the
/// members that are composites — and a second spelling of "which panes have
/// cursors" is a second thing to keep in step with [`pane_cursor`].
const PANE_STOPS: [&str; 3] = ["pv.list", "pv.tree", "pv.bytes"];

/// The application bar: what capture is open, how fast it is arriving, and the
/// running commentary.
fn app_bar_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    vec![
        AccessNode::new("pv.appbar", AriaRole::Group)
            .with_name("packet view")
            .with_child("pv.appbar.interface")
            .with_child("pv.appbar.rate")
            .with_child("pv.appbar.said"),
        AccessNode::new("pv.appbar.interface", AriaRole::Status),
        AccessNode::new("pv.appbar.rate", AriaRole::Status),
        // ★ A live region. It opens EMPTY and fills as the screen is driven, so
        // its name is what the region is and the commentary is its value — a
        // name taken from the contents would be absent at boot, which is the
        // `mumbled` defect and not a naming style.
        // ★★★★★ R1719 — the urgency is derived. It was `Polite`, flat, so a
        // query this screen refused to run waited for a pause a person working
        // the tool does not leave, while every count interrupted nobody — one
        // constant, right for half of what the screen says.
        AccessNode::new("pv.appbar.said", AriaRole::Status)
            .with_name("activity")
            .with_value(AccessValue::Text(state.said_sentence()))
            .with_live(state.said.showing().map_or(AccessLive::Polite, |said| {
                AccessLive::for_urgency(said.urgency())
            })),
    ]
}

/// The filter bar: the query box, the saved filters as toggles, and how much of
/// the capture matched.
fn filter_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    let mut group = AccessNode::new("pv.filter", AriaRole::Group).with_name("Filter");
    let mut nodes = Vec::new();
    // ★★★ R1707 — the query is a text box, and it announces what it holds and
    // what became of it. A screen reader hearing "Filter" and nothing else
    // would be in the position the sighted reader was in before this round:
    // told there is a filter and unable to find out what it did.
    group = group.with_child("pv.filter.query");
    let typed = state.query.text();
    nodes.push(
        AccessNode::new("pv.filter.query", AriaRole::TextInput)
            .with_name("Filter query")
            .with_value(pinion_a11y::AccessValue::Text(if typed.is_empty() {
                spec::QUERY_PLACEHOLDER.to_owned()
            } else {
                typed
            })),
    );
    if let Some(why) = state.query_fault() {
        group = group.with_child("pv.filter.fault");
        nodes.push(AccessNode::new("pv.filter.fault", AriaRole::Status).with_name(why));
    }
    // ★★★★★ R1721 — the bar's whole subtree comes from its rule. It used to be
    // three `button`s with `aria-pressed`, hand-written here, over a set that can
    // never have two on: a screen reader was told "toggle button, not pressed"
    // three times where the truth is "one of three". The rule is
    // `spec::SAVED_ROW` and this call is the only thing that reads it into a
    // tree, so the roles, the selection attribute, the cursor and the focus
    // cannot be chosen here any more.
    group = group.with_child(SAVED_TAG);
    nodes.extend(pinion_a11y::chip_group_nodes(
        &saved_row(state),
        focus_state::focused().as_deref(),
    ));
    group = group.with_child("pv.filter.count");
    nodes.push(AccessNode::new("pv.filter.count", AriaRole::Status));
    nodes.insert(0, group);
    nodes
}

/// The negotiated session context: six values the decode is only interpretable
/// against, each announced as what it is and what it was negotiated to.
fn context_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    let mut group = AccessNode::new("pv.context", AriaRole::Group)
        .with_name("Session context")
        .with_child("pv.context.session");
    // ★★★★★ R1852 — the session run says its own REACH, and when the selected
    // row is outside it, WHICH hop that row is.
    //
    // The paint says *not this row* and stops there because the strip is a fixed
    // band; a reader who cannot see it is under no such constraint, so this is
    // where the hop goes. Same predicate as the paint, so the two cannot
    // disagree about whether the premise applies — only about how much room they
    // have to say it in.
    let covered = spec::row_in_session(state.row.get());
    let hop = spec::ROWS.get(state.row.get()).map_or("", |row| row.hop);
    let mut nodes = vec![
        AccessNode::new("pv.context.session", AriaRole::Status)
            .with_name("Negotiated session")
            .with_value(AccessValue::Text(if covered {
                format!(
                    "{}, covering {} of {} rows, including this one",
                    spec::SESSION,
                    spec::rows_in_session(),
                    spec::ROWS.len(),
                )
            } else {
                format!(
                    "{}, covering {} of {} rows — the selected row is {hop}, so these \
                     values were not negotiated for it",
                    spec::SESSION,
                    spec::rows_in_session(),
                    spec::ROWS.len(),
                )
            })),
    ];
    for value in spec::CONTEXT {
        let tag = format!("pv.context.{}", value.key.replace(' ', "_"));
        group = group.with_child(tag.clone());
        // The KEY names the region and the negotiated setting is its value. The
        // painted run carrying the tag holds only the value, so a name derived
        // from the paint would announce `off` and never say what is off.
        nodes.push(
            AccessNode::new(tag, AriaRole::Status)
                .with_name(value.key)
                .with_value(AccessValue::Text(context_reading(value))),
        );
    }
    nodes.insert(0, group);
    nodes
}

/// What one context value reads as, its consequence note included — the strip
/// paints the two as separate runs and a reader receives them as one fact.
fn context_reading(value: &spec::ContextValue) -> String {
    if value.note.is_empty() {
        value.value.to_owned()
    } else {
        format!("{} · {}", value.value, value.note)
    }
}

/// ★★★★★ R1693 — the message list as a **grid**: a header row of column
/// headers, then one row per message holding one cell per column.
///
/// This is the shape the floor's item view builds from its model, and the shape
/// a hand-painted table has to build for itself or it has none. Measured at
/// 6.11.1, an emptied item view still answers `role = Table` with no diagnostic;
/// here the same emptiness would be `scene/conform`'s `empty` arm unless the
/// grid declared its row count to be zero.
fn list_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    let selected = state.row.get();
    let columns = spec::COLUMNS.len();
    // `aria-rowcount` is the total, so the header row is counted — the reading
    // WAI-ARIA states and the one this tree's chart tables already use.
    // ★★★★★ R1694 — built by [`grid_table_nodes`] rather than by hand. This
    // was the hand-rolled copy, and the sibling dashboard screen wrote a second
    // one; the two disagreed about where the header row sits. WAI-ARIA counts
    // it in `aria-rowcount`, so it has to be counted in `aria-rowindex` too,
    // and this copy counted it in one and not the other: sixteen messages
    // numbered one to sixteen out of seventeen, a header with no index at all,
    // and the first message standing where the header belongs. The rule now
    // lives once, in the builder, and the row's name-from-contents with it.
    // ★★★★★ R1829 — `aria-sort`, and this slot was hard-coded `None` in exactly
    // the way `GridCell::focused` was hard-coded `false` before R1699 — the
    // defect this very function's comment records one paragraph up. A grid that
    // can be ordered and never says so tells a reader the rows are in no
    // particular order, which is a false statement rather than a missing one.
    // `col_sort_dir` is the one home of "does THIS header carry the attribute",
    // and `from_ascending` the one home of bool -> direction; both are taken
    // rather than re-matched here.
    let sort = state.sort.get();
    let grid_columns: Vec<GridColumn> = (0..columns)
        .map(|n| GridColumn {
            tag: format!("pv.list.head.{n}"),
            sort: col_sort_dir(sort, n).map(SortDirection::from_ascending),
        })
        .collect();
    // ★★★ R1707 — a reader hears the list the query kept. WAI-ARIA's row count
    // is what is PRESENTED, so a table announcing the hidden rows would tell a
    // screen reader there are sixteen messages while the screen draws three.
    let grid_rows: Vec<GridRow> = state
        .kept()
        .into_iter()
        .map(|n| GridRow {
            tag: format!("pv.list.row.{n}"),
            selected: n == selected,
            state: RadioState::Idle,
            cells: row_cells(n)
                .into_iter()
                .enumerate()
                .map(|(c, text)| GridCell {
                    tag: list_cell_tag(n, c),
                    name: text,
                    // ★★★★★ R1699 — the cell a reader has gone into. The slot
                    // existed from R1694 and was hard-coded `false`, which is
                    // what a grid with no way into its rows looks like from the
                    // accessibility side: seven cells per row, none of them ever
                    // current.
                    focused: n == selected && state.cell.get() == Some(c),
                    selected: None,
                })
                .collect(),
        })
        .collect();
    grid_table_nodes(
        "pv.list",
        spec::PANES[0].title,
        false,
        LIST_HEADER,
        &grid_columns,
        &grid_rows,
    )
}

/// The tag the header row is announced under. Nothing paints it — the seven
/// column headers are painted individually and the row is what a reader
/// descends through — so it is anchored by the members it composes.
const LIST_HEADER: &str = "pv.list.header";

/// What one message's cells **paint**, left to right — one entry per
/// [`spec::COLUMNS`] entry.
///
/// The painter's own source, so a column that changes what it shows changes it
/// once. The name column holds the resource name only; the annotations painted
/// beside it in that column are separate runs, placed from the right edge.
///
/// ★ R1827 — takes the row's INDEX rather than the row, and stays that way even
/// though every value below is the row's own: its peer [`row_cells`] announces a
/// relation BETWEEN rows, which a `&RowSpec` cannot see. The two are a pair on
/// purpose — that is the whole reason they share a function — so they take the
/// same argument, and a caller cannot hold the one that would drift.
fn cell_texts(n: usize) -> Vec<String> {
    let message = &spec::ROWS[n];
    vec![
        message.time.to_owned(),
        message.hop.to_owned(),
        message.channel.to_owned(),
        message.sn.to_string(),
        message.kind.to_owned(),
        message.name.to_owned(),
        message.len.to_string(),
    ]
}

/// How many digits `n` is written with.
const fn digits(n: u32) -> u32 {
    let mut digits = 1;
    let mut rest = n / 10;
    while rest > 0 {
        digits += 1;
        rest /= 10;
    }
    digits
}

/// The width row `n`'s link annotation takes out of the name column, including
/// the gap the painter puts before it. Zero for a row in no pair.
///
/// ★★★★★ R1827 — the width **without** the string, because the string cannot be
/// built in a `const` context and [`NAME_FLOOR`] needs this number in one. That
/// makes it the round's one deliberate second spelling, so it is the round's one
/// deliberate cross-check: `r1827_the_link_annotations_reserved_width_is_the_
/// width_it_paints` measures the painted run against this number for every row,
/// which is what turns "they agree" from care into a fact.
const fn link_width(rows: &[spec::RowSpec], n: usize) -> u32 {
    let word = spec::link_word(rows, n);
    if word.is_empty() {
        return 0;
    }
    match spec::correlation_in(rows, n) {
        None => 0,
        Some(other) => (char_count(word) + digits(rows[other].sn)) * (FONT_SMALL - 4) + 10 + 8,
    }
}

/// What one message's cells **announce**.
///
/// [`cell_texts`] plus the runs painted beside them, so the two cannot drift on
/// every plain column and the name column's difference is stated exactly here.
/// That column announces all three of its runs because all three are painted in
/// it, and a reader told only the first would never learn a piece was dropped.
fn row_cells(n: usize) -> Vec<String> {
    let message = &spec::ROWS[n];
    let mut cells = cell_texts(n);
    let name = &mut cells[NAME_COLUMN];
    if let Some(fragment) = &message.fragment {
        name.push(' ');
        name.push_str(fragment.marker);
        name.push(' ');
        name.push_str(fragment.piece);
    }
    if !message.note.is_empty() {
        name.push(' ');
        name.push_str(message.note);
    }
    // ★★★★★ R1827 — and the exchange this message is half of, announced in the
    // same cell it is painted in. A reader who cannot see the accent ink is the
    // reason this is here rather than only in the paint: the link is the one
    // annotation whose whole value is that it points somewhere, and a reader
    // told "store/config" with no "answers 1182" would not know an exchange had
    // happened at all.
    if let Some(link) = spec::link_text(n) {
        name.push(' ');
        name.push_str(&link);
    }
    cells
}

/// The column the resource name and its annotations share.
const NAME_COLUMN: usize = 5;

/// ★★★★★ R1693 — the decode as a **tree**: one item per visible field, carrying
/// its depth, its position among its siblings, and **its value**.
///
/// The value is where this beats the floor rather than matching it. Built and
/// run at 6.11.1, a two-column tree announces a row as **two sibling items** —
/// `L1 transport` and `v0x09` are peers, the value reports `expandable = 1` like
/// the field it belongs to, and leaf rows report `expanded = 1` while reporting
/// they cannot expand. The hierarchy is gone too: every item is a direct child
/// of the tree whatever its depth. Here a field is one item, its value is its
/// value, and `aria-level` carries the depth the paint indents by.
fn tree_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    let selected = state.field.get();
    let folded = state.folded.get();
    let visible = visible_fields(state);
    let mut tree = AccessNode::new("pv.tree", AriaRole::Tree)
        // The pane paints its own title and that run declares itself this
        // node's name, so the redirect is true rather than merely well formed.
        .with_name_from_tag("pv.tree.title")
        .with_size_of_set(u32::try_from(visible.len()).unwrap_or(u32::MAX));
    let mut nodes = Vec::new();
    for (n, (path, name, value, depth)) in visible.iter().enumerate() {
        let tag = format!("pv.tree.field.{path}");
        tree = tree.with_child(tag.clone());
        let (position, siblings) = sibling_place(&visible, n);
        let mut item = AccessNode::new(tag, AriaRole::TreeItem)
            .with_name(name.clone())
            .with_value(AccessValue::Text(value.clone()))
            .with_level(u32::try_from(*depth).unwrap_or(0) + 1)
            .with_set_position(position, siblings)
            .with_selected(*path == selected);
        // A layer heading folds; a field does not. `aria-expanded` is the state
        // the chevron draws, which is why the chevron itself is declared part of
        // this item rather than announced as a glyph of its own.
        if let Some(index) = spec::LAYERS.iter().position(|(id, _)| id == path) {
            item = item.with_expanded(!folded.get(index).copied().unwrap_or(false));
        }
        nodes.push(item);
    }
    nodes.insert(0, tree);
    nodes
}

/// Where the `n`-th visible field sits **among its own siblings**, and how many
/// of them there are.
///
/// `aria-posinset` counts within a level, not within the flattened list the tree
/// is painted as — a field announced as "3 of 24" when it is the third of four
/// under its layer tells a reader the wrong shape.
///
/// ★ The block is the run between the nearest shallower entries either side, and
/// the count inside it is of entries at **this** depth. The first draft returned
/// the block's whole length, which is right for a layer's children and wrong for
/// the layers themselves: a top-level heading counted every row in the tree. The
/// test written for this function is what said so — it had none for an hour, and
/// that is the shape R1692 measured five times in one round.
fn sibling_place(visible: &[(String, String, String, usize)], n: usize) -> (usize, usize) {
    let depth = visible[n].3;
    let start = visible[..n]
        .iter()
        .rposition(|(_, _, _, d)| *d < depth)
        .map_or(0, |index| index + 1);
    let end = visible[start..]
        .iter()
        .position(|(_, _, _, d)| *d < depth)
        .map_or(visible.len(), |offset| start + offset);
    let at_depth = |slice: &[(String, String, String, usize)]| {
        slice.iter().filter(|(_, _, _, d)| *d == depth).count()
    };
    (at_depth(&visible[start..n]), at_depth(&visible[start..end]))
}

/// ★★★★ R1693 — the bytes as a **grid** of nine columns: the offset as each
/// row's header, then eight cells.
///
/// This is the pane the floor cannot express at all. Measured at 6.11.1, a
/// custom-painted 72-cell widget answers **one** node, empty-named, with no
/// children — because everything there is derived from a model, and a pane that
/// paints itself has no model to derive from. A cell in an item view does report
/// its `rowHeaderCells`, which is why the offsets are headers here rather than
/// decoration.
fn bytes_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    let buffer = state.frame_bytes();
    let layout = hex_layout();
    let lit = state.lit_selection();
    let per_row = spec::BYTES_PER_ROW;
    let mut grid = AccessNode::new("pv.bytes", AriaRole::Grid)
        .with_name_from_tag("pv.bytes.title")
        // The readout beside the title says which bytes the open field covers;
        // it is the grid's description rather than a member of it, because a
        // `grid` owns rows and nothing else.
        .with_described_by("pv.bytes.span")
        .with_row_count(u32::try_from(layout.rows()).unwrap_or(u32::MAX))
        .with_column_count(u32::try_from(per_row).unwrap_or(u32::MAX) + 1);
    let mut nodes = vec![AccessNode::new("pv.bytes.span", AriaRole::Status)];
    for r in 0..layout.rows() {
        let row_tag = bytes_row_tag(r);
        grid = grid.with_child(row_tag.clone());
        let offset = bytes_offset_tag(r);
        let mut row = AccessNode::new(row_tag, AriaRole::Row)
            .with_row(r)
            .with_child(offset.clone());
        nodes.push(
            AccessNode::new(offset, AriaRole::RowHeader)
                .with_row(r)
                .with_column(0),
        );
        for c in 0..per_row {
            let byte = r * per_row + c;
            let Some(value) = buffer.get(byte) else {
                break;
            };
            let tag = format!("pv.bytes.cell.{byte}");
            row = row.with_child(tag.clone());
            nodes.push(
                AccessNode::new(tag, AriaRole::GridCell)
                    .with_name(format!("{value:02x}"))
                    .with_row(r)
                    .with_column(c + 1)
                    .with_selected(lit.is_some_and(|sel| sel.contains(byte))),
            );
        }
        nodes.push(row);
    }
    nodes.insert(0, grid);
    nodes
}

/// The reassembly strip: one lane per channel carrying traffic, and the totals.
fn reassembly_nodes() -> Vec<AccessNode> {
    let mut group = AccessNode::new("pv.reassembly", AriaRole::Group)
        .with_name_from_tag("pv.reassembly.title")
        .with_child("pv.reassembly.counts");
    // ★ R1845 — the totals reach a reader who cannot see the label. This node
    // was a bare `Status` with no value at all, so the one sentence that says
    // how much of the session is carrying traffic was paint-only.
    let mut nodes = vec![
        AccessNode::new("pv.reassembly.counts", AriaRole::Status)
            .with_value(AccessValue::Text(reassembly_counts())),
    ];
    for (n, lane) in spec::LANES.iter().enumerate() {
        let tag = format!("pv.reassembly.lane.{n}");
        group = group.with_child(tag.clone());
        // The lane paints its name and its continuity as siblings of its box, so
        // both come from the table the painter reads.
        nodes.push(
            AccessNode::new(tag, AriaRole::Status)
                .with_name(lane.name)
                .with_value(AccessValue::Text(lane_reading(lane))),
        );
    }
    nodes.insert(0, group);
    nodes
}

/// What one reassembly lane reads as — the sequence number its channel has
/// reached, and what if anything is wrong with the sequence.
///
/// ★★★★★ R1845 — every word of this is now DERIVED from the capture's rows.
/// Before, all three numbers were written down beside the lane with no channel
/// code linking them to anything, and the else-arm reported `{dropped}
/// abandoned` whatever the break actually was.
fn lane_reading(lane: &spec::LaneSpec) -> String {
    let faults = lane.faults();
    if faults.is_empty() {
        format!("{} · unbroken", lane.sn())
    } else {
        format!("{} · {}", lane.sn(), faults.join(" · "))
    }
}

impl WidgetView for PacketView {
    type Renderer = HelloPacketViewRenderer;

    /// ★★★ R1707 — a press inside the query box puts the caret where the
    /// pointer landed, through the framework's own hit test.
    ///
    /// The sibling screen measured why this is not automatic: every press here
    /// is routed to the ONE root external that does this screen's own hit test,
    /// and the field's external is a focus owner and a keystroke sink rather
    /// than a second pointer target. Without these two hooks the box can be
    /// typed into and never clicked into — no caret placement and no selection
    /// sweep on the only text entry this screen has.
    fn position_caret_for_point(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
        _hit_tag: Option<&str>,
        x: f32,
        y: f32,
        extend: bool,
    ) -> Option<usize> {
        let byte = query_byte_at(state.0, scene, focused, x, y)?;
        let edit = use_text_edit_state(QUERY_TAG);
        if extend {
            let anchor = edit.selection_anchor().unwrap_or_else(|| edit.caret());
            edit.set_selection(anchor, byte);
            Some(anchor)
        } else {
            edit.set_caret(byte);
            Some(byte)
        }
    }

    /// The other half of the same hit test: a drag inside the box sweeps a
    /// selection from the byte the press pinned.
    fn select_drag_to_point(
        state: &(TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
        anchor: usize,
        x: f32,
        y: f32,
    ) -> bool {
        let Some(byte) = query_byte_at(state.0, scene, focused, x, y) else {
            return false;
        };
        let edit = use_text_edit_state(QUERY_TAG);
        let before = (edit.caret(), edit.selection_anchor());
        edit.set_selection(anchor, byte);
        before != (edit.caret(), edit.selection_anchor())
    }

    /// ★★★★★ R1711 — this screen told its own layout it lays out down to
    /// `MIN_W` x `MIN_H` and told the window system, through
    /// `SizeStrategy::Fixed`, that it can never be smaller than the size it
    /// opens at. Two declarations about one screen, contradicting each other,
    /// and nothing compared them until `scene/size_floor` measured the screen
    /// and answered `roomier`: the window refused **554 pixels of height** the
    /// screen can actually take.
    ///
    /// `Fixed` here was never a decision — the sibling screens are resizable,
    /// this one is a three-pane capture viewer whose whole point is being sized
    /// to the reader's display, and the floor it declares to the layout is the
    /// floor it now declares to the window. The gate asserts the two agree with
    /// what was MEASURED (verdict `exact`), so a change that makes the screen
    /// need more room fails there rather than shipping a window a reader can
    /// shrink past its own content.
    /// ★ R1712 — and the two numbers now come out of one declaration. This
    /// screen concedes nothing: `SHRINK` is rigid, so its window stops
    /// exactly where its layout does. That is a decision — measured, this
    /// screen could go 195 pixels narrower and 37 shorter with everything still
    /// reachable — and it is left unmade here because the band's honest
    /// declaration is *every pane*: the reassembly lanes, the byte pane and the
    /// filter count all clip at once, which buys a reader little and would cost
    /// the concession list its meaning. The node lab is where the band earns
    /// itself, because one display width sits inside it.
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::shrinking(SHRINK, (WIN_W, WIN_H))
    }

    fn shrink_policy() -> Option<ShrinkPolicy> {
        Some(SHRINK)
    }

    /// ★★★★★ R1861 — the reassembly strip, which is what a host's floating
    /// overlay lands on here.
    ///
    /// **Derived from `reassembly_rect` rather than written down.** Measured
    /// before this existed: the host's toast covered TWO of this strip's lane
    /// readouts *entirely* — 12 pixels of a 12-pixel run each — and nobody had
    /// ever seen it, because the person who reported the same defect on the
    /// sibling screen was looking at the sibling screen.
    fn keeps_clear(region: Rect) -> Option<Rect> {
        let strip = reassembly_rect();
        Some(Rect::new(
            region.x + strip.x,
            region.y + strip.y,
            strip.w,
            strip.h,
        ))
    }

    /// ★★★★★ R1747 — the verdict this screen has computed since R1663,
    /// answered where the application it is a page of can reach it.
    ///
    /// R1663 wrote screen B's specification as a value and compared the painted
    /// scene against it inside a unit test of this binary. R1738 then recorded
    /// this section as unjudged and gave the reason as *no written
    /// specification at all*, which was false; what was true is that the
    /// comparison was `#[cfg(test)]` and this hook was not implemented. See
    /// `judge` for what a screen says about a surface a session has not put on
    /// screen, and for the two reasons a decode row can light no bytes.
    fn conformance() -> Option<pinion_core::conformance::DocumentReport> {
        Some(judge::conformance())
    }
}

/// Run the capture viewer as an application of its own.
///
/// ★ R1729 — the four lines the standalone binary is, kept here so the binary
/// and the mounted page name the same binding. `src/main.rs` calls this.
pub fn run() {
    pinion_shell::run::<PacketView>();
}
