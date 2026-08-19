// R1412 §5.49 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-log-view` — R1731 §5.27 §5.40 §5.41 — the analysis tool's **log
//! section**, the fourth seat of the reference's rail and the last one this
//! build owed.
//!
//! ## What forced this example
//!
//! `docs/analyzer-rail-spec.json` carried one accepted divergence from the
//! reference's navigation after R1730 paid off the third: *`logs` is specified
//! open and is closed (unbuilt)*. The reference implements the section — an
//! event list and a decode pane — and this tree drew the seat, named it and
//! refused it.
//!
//! ## What is assembly, and what is not
//!
//! Almost all of it is assembly, and that is the point: R1730 built
//! [`pinion_core::conformance`] and the round after it needed no new framework
//! to hold a second specified screen. What R1731 *did* add is the part R1730
//! would otherwise have been copied for —
//! [`SpecDocument`](pinion_core::conformance::SpecDocument) and
//! `pinion_core::test_fixtures::surface`, which are the loader and the
//! paint-side reader both screens now share.
//!
//! Two things here are this section's own rather than the sibling's:
//!
//! * the severity choice is **exclusive and ordered** — *warnings* means
//!   warnings and errors — which three independent toggles could not express;
//! * the decode pane's last part is the frame's **bytes**, drawn through the
//!   framework's own byte-dump geometry, so what is on screen is what
//!   [`pinion_core::widgets::hex_dump::HexLayout`] says is there
//!   rather than a second formatting loop.
//!
//! ## The screen
//!
//! ```text
//! cargo run -p hello-log-view --release
//! ```
//!
//! A 46-high header — the section's name, whether a capture is running, a live
//! filter and the severity choice — over a five-column event list, beside a
//! 340-wide decode pane. Click an event to decode it; type in the filter to
//! narrow the list; click a severity to keep that severity and worse; the
//! arrows walk it.
//!
//! See `tools/demos/r1731_a_log_section_is_the_one_the_reference_draws.py`.

mod spec;

#[cfg(test)]
mod painted;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::rc::Rc;

use pinion_a11y::{
    AccessFocus, AccessLive, AccessNode, AccessValue, AriaRole, GridCell, GridColumn, GridRow,
    WidgetA11y, grid_table_nodes,
};
use pinion_core::conformance::Part;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, PointerTarget, ReadRefusal, RepaintOwner,
    SchemaArg, SchemaField, ThreadOwnership,
};
use pinion_core::input::PointerReading;
use pinion_core::reactive::Signal;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::shrink::ShrinkPolicy;
use pinion_core::style::{Border, BoxStyle, Color, LayoutStyle, Size, TextOverflow, TextStyle};
use pinion_core::theme::{Theme, use_theme};
use pinion_core::utterance::{Tone, Utterance};
use pinion_core::voice::Silence;
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::grid_sort::Admission;
use pinion_core::widgets::hex_dump::HexLayout;
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::row_query::RowQuery;
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::text_edit::{TextEditState, use_text_edit_state};
use pinion_core::widgets::text_field::TextFieldState;
use pinion_core::{CellKind, Frame, Scene, WidgetCore, edit_field_keymap};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::pane::{PanePointer, scroll_pane};
use pinion_widget_paint::run::text_run;
use pinion_widget_paint::text_field as tf_paint;

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloLogViewRenderer, HelloLogViewRendererError);

// ── Tags ────────────────────────────────────────────────────────────────────

/// The tag the widget is registered under — the receiver a press resolves to.
const VIEW_TAG: &str = "log_view";
/// The root address, for `scene/snapshot` and the sweep.
const ROOT_TAG: &str = "lv.root";
/// The theme scope.
const THEME_TAG: &str = "app";
/// The filter box. Deliberately outside the `lv.header.` namespace the header's
/// parts live in — a surface's parts are read back by walking that prefix and
/// taking the names with no further dot, so a child tagged inside a part would
/// have to be excluded by name.
const QUERY_TAG: &str = "lv.filter.query";
/// The list's accessibility header row. Nothing paints it — the column headers
/// are painted individually — so it is anchored by the members it composes.
const LIST_HEADER: &str = "lv.list.header";
/// The list's grid.
const LIST_TAG: &str = "lv.list";
/// The decode pane.
const DETAIL_TAG: &str = "lv.detail";
/// The section header.
const HEADER_TAG: &str = "lv.header";

const FONT_SMALL: u32 = 11;
const FONT_BODY: u32 = 12;
const FONT_TITLE: u32 = 14;

// ── Geometry ────────────────────────────────────────────────────────────────

const WIN_W: u32 = spec::WIN_W;
const WIN_H: u32 = spec::WIN_H;
const HEADER_H: u32 = spec::HEADER_H;
const COLHEAD_H: u32 = spec::COLHEAD_H;
const ROW_H: u32 = spec::ROW_H;
const DETAIL_W: u32 = spec::DETAIL_W;
const PAD: u32 = spec::PAD;
const GAP: u32 = spec::GAP;

/// The smallest width this section lays out completely at, derived from the
/// reference's own grid: the decode pane, plus the row's padding, plus the four
/// fixed widths and their gaps, plus the minimum the reference gives the message
/// column.
const fn min_width() -> u32 {
    let mut fixed = 0;
    let mut gaps = 0;
    let mut n = 0;
    while n < spec::COLUMNS.len() {
        fixed += spec::COLUMNS[n].width;
        if n > 0 {
            gaps += GAP;
        }
        n += 1;
    }
    DETAIL_W + 2 * PAD + fixed + gaps + spec::MESSAGE_MIN
}

/// The smallest width the layout is complete at.
const MIN_W: u32 = min_width();
/// The smallest height it is complete at — the decode pane's own stack, which is
/// the taller of the two columns. Asserted against the laid-out stack by
/// `tests.rs` rather than trusted.
const MIN_H: u32 = 420;

const _: () = assert!(
    MIN_W < WIN_W && MIN_H < WIN_H,
    "the section must open larger than the narrowest layout it can manage",
);

/// What this screen concedes when it is not given the room it wants.
const SHRINK: ShrinkPolicy = ShrinkPolicy::conceding(
    (MIN_W, MIN_H),
    (720, 380),
    &["the columns right of the message clip before the decode pane narrows"],
);

fn window_size() -> (u32, u32) {
    pinion_core::external::layout_size(VIEW_TAG, SHRINK.comfortable(), (WIN_W, WIN_H))
}

fn list_column_rect() -> Rect {
    let (w, h) = window_size();
    Rect::new(0, 0, w.saturating_sub(DETAIL_W), h)
}

fn header_rect() -> Rect {
    Rect::new(0, 0, list_column_rect().w, HEADER_H)
}

fn colhead_rect() -> Rect {
    Rect::new(0, HEADER_H, list_column_rect().w, COLHEAD_H)
}

fn list_rect() -> Rect {
    let column = list_column_rect();
    let top = HEADER_H + COLHEAD_H;
    Rect::new(0, top, column.w, column.h.saturating_sub(top))
}

fn detail_rect() -> Rect {
    let (w, h) = window_size();
    Rect::new(w.saturating_sub(DETAIL_W), 0, DETAIL_W, h)
}

/// The x and width of column `n`, in the list column's own coordinates.
fn column_rect(n: usize) -> Rect {
    let gaps = u32::try_from(spec::COLUMNS.len().saturating_sub(1)).unwrap_or(0);
    let fixed: u32 = spec::COLUMNS.iter().map(|c| c.width).sum();
    let flexible = list_column_rect()
        .w
        .saturating_sub(2 * PAD)
        .saturating_sub(fixed)
        .saturating_sub(GAP * gaps);
    let mut x = PAD;
    for (i, column) in spec::COLUMNS.iter().enumerate() {
        let w = if column.width == 0 {
            flexible
        } else {
            column.width
        };
        if i == n {
            return Rect::new(x, 0, w, ROW_H);
        }
        x += w + GAP;
    }
    Rect::new(x, 0, 0, ROW_H)
}

fn list_row_rect(visual: usize) -> Rect {
    Rect::new(
        0,
        u32::try_from(visual).unwrap_or(0) * ROW_H,
        list_rect().w,
        ROW_H,
    )
}

/// The header's four parts, left to right, in the header's own coordinates.
fn header_parts() -> Vec<(&'static str, Rect)> {
    let w = header_rect().w;
    let filter_x = w.saturating_sub(PAD + spec::SEVERITY_W + 10 + spec::FILTER_W);
    vec![
        ("title", Rect::new(PAD, 14, 60, 18)),
        ("live", Rect::new(PAD + 72, 15, 96, 16)),
        (
            "filter",
            Rect::new(
                filter_x,
                (HEADER_H.saturating_sub(spec::FILTER_H)) / 2,
                spec::FILTER_W,
                spec::FILTER_H,
            ),
        ),
        (
            "severity",
            Rect::new(
                w.saturating_sub(PAD + spec::SEVERITY_W),
                (HEADER_H.saturating_sub(26)) / 2,
                spec::SEVERITY_W,
                26,
            ),
        ),
    ]
}

/// One severity choice's rectangle, in the severity part's own coordinates.
fn choice_rect(n: usize) -> Rect {
    let each = spec::SEVERITY_W / u32::try_from(spec::CHOICES.len()).unwrap_or(1);
    Rect::new(u32::try_from(n).unwrap_or(0) * each, 0, each - 4, 26)
}

/// The byte dump's geometry for `bytes`.
///
/// Eight per row rather than the classic sixteen, because the pane is 340 wide
/// and a sixteen-wide row of hex does not fit in it — the reference wraps for
/// the same reason.
fn byte_layout(bytes: &[u8]) -> HexLayout {
    HexLayout::new(bytes.len())
        .with_bytes_per_row(8)
        .with_offset_digits(4)
}

/// The decode pane's parts, in paint order, each with the rectangle it was
/// given — in the pane's own coordinates.
///
/// Two of them are measured rather than fixed: the decoded fields and the bytes
/// are lists, and a list's height is its content's. Passing the record in is
/// what makes that possible, and it is why this takes an argument where the
/// sibling section's peer does not.
fn detail_parts(record: &'static spec::RowSpec) -> Vec<(&'static str, Rect)> {
    let inner = DETAIL_W.saturating_sub(2 * PAD);
    let mut out = Vec::with_capacity(spec::DETAIL.len());
    out.push(("subject", Rect::new(PAD, 14, 150, 20)));
    out.push((
        "kind",
        Rect::new(DETAIL_W.saturating_sub(PAD + 96), 14, 96, 20),
    ));
    let mut y = HEADER_H + 18;
    for part in &spec::DETAIL[2..] {
        let height = match part.key {
            "layers" => {
                spec::LIST_LABEL_H + spec::FIELD_H * u32::try_from(record.fields.len()).unwrap_or(0)
            }
            "bytes" => {
                spec::LIST_LABEL_H
                    + 18 * u32::try_from(byte_layout(record.bytes).rows().max(1)).unwrap_or(1)
            }
            _ => part.height,
        };
        out.push((part.key, Rect::new(PAD, y, inner, height)));
        y += height + if part.height == 0 { 18 } else { 9 };
    }
    out
}

const fn contains(rect: Rect, px: u32, py: u32) -> bool {
    px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h
}

/// The centre of a rectangle — where the sweep presses.
#[cfg(test)]
const fn centre(rect: Rect) -> (u32, u32) {
    (rect.x + rect.w / 2, rect.y + rect.h / 2)
}

// ── Ink ─────────────────────────────────────────────────────────────────────

const fn rgb(hex: u32) -> Color {
    Color::rgb(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

/// The inks this screen paints with.
#[derive(Clone, Copy)]
struct Ink {
    bg: Color,
    surface: Color,
    surface_2: Color,
    outline: Color,
    text: Color,
    text_2: Color,
    text_3: Color,
    accent: Color,
    accent_soft: Color,
    warn: Color,
    err: Color,
}

fn ink() -> Ink {
    Ink {
        bg: rgb(0x0E_0F12),
        surface: rgb(0x16_181D),
        surface_2: rgb(0x1D_2026),
        outline: rgb(0x2A_2E36),
        text: rgb(0xE8_EBEF),
        text_2: rgb(0x98_A2AD),
        text_3: rgb(0x69_7180),
        accent: rgb(0xEC_5AA0),
        accent_soft: Color::rgba(0xEC, 0x5A, 0xA0, 0x28),
        warn: rgb(0xD9_A21B),
        err: rgb(0xE0_5252),
    }
}

fn severity_ink(severity: spec::Severity, ink: Ink) -> Color {
    match severity {
        spec::Severity::Info => ink.text_2,
        spec::Severity::Warn => ink.warn,
        spec::Severity::Error => ink.err,
    }
}

/// The ink a message class is drawn in — a reader's index into a list going
/// past, which is why the reference gives its classes different colours.
fn kind_ink(kind: &str) -> Color {
    match kind {
        "Data" => rgb(0x3d_8b_fd),
        "Query" => rgb(0xb0_69_d8),
        "Response" => rgb(0x2e_a0_67),
        "Declaration" => rgb(0xd1_8b_1f),
        _ => rgb(0x77_82_8c),
    }
}

// ── State ───────────────────────────────────────────────────────────────────

/// Everything the screen holds, and nothing it can derive.
struct ViewState {
    /// Which event's decode is open, by index into [`spec::ROWS`].
    row: Signal<usize>,
    /// Which severity choice is on. **One** index rather than a set of flags:
    /// the choice is exclusive and the floors are ordered.
    choice: Signal<usize>,
    /// Whether a capture is running, which is what the header's live mark reads.
    capturing: Signal<bool>,
    /// The filter's own buffer, not a copy of it.
    query: Rc<TextEditState>,
    /// The list body's scroll offset.
    list_scroll: Rc<ScrollState>,
    /// Where the cursor last was, because a press carries no coordinates.
    cursor: Signal<(u32, u32)>,
    /// The last thing the screen said.
    said: RefCell<Option<Utterance>>,
}

impl ViewState {
    fn say(&self, what: Utterance) {
        *self.said.borrow_mut() = Some(what);
    }

    fn said_sentence(&self) -> String {
        self.said
            .borrow()
            .as_ref()
            .map(Utterance::sentence)
            .unwrap_or_default()
    }

    fn query(&self) -> RowQuery {
        RowQuery::parse(&self.query.text(), &spec::query_columns()).unwrap_or_default()
    }

    fn query_fault(&self) -> Option<String> {
        RowQuery::parse(&self.query.text(), &spec::query_columns())
            .err()
            .map(|e| e.to_string())
    }

    /// The severity floor the chosen control sets.
    fn floor(&self) -> Option<spec::Severity> {
        spec::CHOICES
            .get(self.choice.get())
            .and_then(|choice| choice.floor)
    }

    /// The source indices the section is showing, in capture order.
    ///
    /// The ONE derivation, and it folds **both** narrowings: the severity floor
    /// and the query. Two lists — one per control — is how a screen comes to
    /// press the wrong row.
    fn kept(&self) -> Vec<usize> {
        let query = self.query();
        let floor = self.floor();
        (0..spec::ROWS.len())
            .filter(|&n| floor.is_none_or(|least| spec::ROWS[n].severity >= least))
            .filter(|&n| {
                if query.is_everything() {
                    return true;
                }
                let cells = spec::ROWS[n].attributes();
                query.admit(|c| cells.get(c).map_or("", String::as_str)) == Admission::Admitted
            })
            .collect()
    }

    fn cursor_row(&self) -> usize {
        let open = self.row.get();
        let kept = self.kept();
        if kept.contains(&open) {
            return open;
        }
        kept.first().copied().unwrap_or(open)
    }

    fn record(&self) -> &'static spec::RowSpec {
        &spec::ROWS[self.cursor_row()]
    }

    /// What the header's live mark reads.
    ///
    /// ★ The reference draws this mark while a capture is running and NOTHING
    /// there when it is not. This build always draws the part and changes what
    /// it says: a blank cannot be told apart from a build that forgot to draw
    /// it, and the specification fixes that the part is there rather than what
    /// it reads.
    fn capture_reading(&self) -> String {
        if self.capturing.get() {
            format!("LIVE · {} events", self.kept().len())
        } else {
            format!("PAUSED · {} events", self.kept().len())
        }
    }
}

fn use_view_state() -> Rc<ViewState> {
    // ★ [[owner-cache-no-nested-factory]] — every cached slot this one holds is
    // resolved BEFORE the factory runs.
    let list_scroll = pinion_core::widgets::scroll::use_scroll_state("lv.list.body");
    let query = use_text_edit_state(QUERY_TAG);
    let owner = pinion_core::reactive::Owner::current()
        .expect("use_view_state requires an active Owner scope");
    owner.cache("log_view.state", || ViewState {
        row: Signal::new(spec::OPENING_ROW),
        choice: Signal::new(spec::OPENING_CHOICE),
        capturing: Signal::new(true),
        query,
        list_scroll,
        cursor: Signal::new((0, 0)),
        said: RefCell::new(None),
    })
}

// ── The hit test ────────────────────────────────────────────────────────────

/// What is under a point. One enum, resolved from the same rectangles the
/// painter uses.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hit {
    /// An event row, by its index in [`spec::ROWS`].
    Event(usize),
    /// A severity choice, by its index in [`spec::CHOICES`].
    Choice(usize),
    /// Nothing that answers.
    None,
}

impl Hit {
    /// What a **key** press at `tag` addresses.
    fn of_tag(tag: &str) -> Self {
        if let Some(n) = tag
            .strip_prefix("lv.list.row.")
            .and_then(|n| n.parse::<usize>().ok())
            && n < spec::ROWS.len()
        {
            return Self::Event(n);
        }
        if let Some((row, _column)) = tag
            .strip_prefix("lv.list.cell.")
            .and_then(|rest| rest.split_once('_'))
            && let Ok(row) = row.parse::<usize>()
            && row < spec::ROWS.len()
        {
            return Self::Event(row);
        }
        if let Some(key) = tag.strip_prefix("lv.severity.")
            && let Some(n) = spec::CHOICES.iter().position(|c| c.key == key)
        {
            return Self::Choice(n);
        }
        Self::None
    }

    fn word(&self) -> Option<String> {
        Some(match self {
            Self::Event(n) => format!("event.{n}"),
            Self::Choice(n) => format!("severity.{}", spec::CHOICES[*n].key),
            Self::None => return None,
        })
    }

    /// What answers at the window point `(px, py)`.
    fn at(state: &ViewState, px: u32, py: u32) -> Self {
        let header = header_rect();
        if contains(header, px, py) {
            if let Some((_, at)) = header_parts().into_iter().find(|(k, _)| *k == "severity")
                && contains(at, px, py)
            {
                let (cx, cy) = (px - at.x, py - at.y);
                for n in 0..spec::CHOICES.len() {
                    if contains(choice_rect(n), cx, cy) {
                        return Self::Choice(n);
                    }
                }
            }
            return Self::None;
        }
        let list = list_rect();
        if contains(list, px, py) {
            let (ox, oy) = state.list_scroll.offset();
            let lx = px.saturating_sub(list.x).saturating_add(clamp_offset(ox));
            let ly = py.saturating_sub(list.y).saturating_add(clamp_offset(oy));
            for (visual, &n) in state.kept().iter().enumerate() {
                if contains(list_row_rect(visual), lx, ly) {
                    return Self::Event(n);
                }
            }
        }
        Self::None
    }
}

/// A scroll offset as a positive displacement into the body.
#[allow(clippy::cast_sign_loss)]
const fn clamp_offset(offset: i32) -> u32 {
    if offset > 0 { offset as u32 } else { 0 }
}

// ── The handlers a press and the wire both reach ────────────────────────────

fn select_event(state: &Rc<ViewState>, row: usize) {
    if row >= spec::ROWS.len() {
        return;
    }
    state.row.set(row);
    let record = &spec::ROWS[row];
    state.say(Utterance::done(format!(
        "{} at {} from {}",
        record.message, record.time, record.source
    )));
}

/// Choose a severity floor.
///
/// The choice is exclusive, so this SETS rather than toggles — and it says what
/// the list became, because narrowing a log to nothing looks exactly like a
/// screen that broke.
fn choose_severity(state: &Rc<ViewState>, n: usize) {
    if n >= spec::CHOICES.len() {
        return;
    }
    state.choice.set(n);
    let kept = state.kept().len();
    let choice = &spec::CHOICES[n];
    state.say(if kept == 0 {
        Utterance::new(
            Tone::Unchanged,
            format!("{} keeps nothing in this capture", choice.title),
        )
    } else {
        Utterance::done(format!("{} · {kept} of {}", choice.title, spec::ROWS.len()))
    });
}

fn set_capturing(state: &Rc<ViewState>, on: bool) {
    state.capturing.set(on);
    state.say(Utterance::done(state.capture_reading()));
}

fn set_query(state: &Rc<ViewState>, text: &str) {
    state.query.set_text(text.to_owned());
    state.query.set_caret(text.len());
}

fn announce_query(state: &Rc<ViewState>) {
    match state.query_fault() {
        Some(why) => state.say(Utterance::new(Tone::Refused, why)),
        None => state.say(Utterance::done(format!(
            "{} of {} shown",
            state.kept().len(),
            spec::ROWS.len()
        ))),
    }
}

fn move_cursor(state: &Rc<ViewState>, px: u32, py: u32) {
    state.cursor.set((px, py));
}

fn press(state: &Rc<ViewState>) -> bool {
    let (px, py) = state.cursor.get();
    act_on_hit(state, &Hit::at(state, px, py))
}

fn act_on_hit(state: &Rc<ViewState>, hit: &Hit) -> bool {
    match hit {
        Hit::Event(n) => {
            select_event(state, *n);
            true
        }
        Hit::Choice(n) => {
            choose_severity(state, *n);
            true
        }
        Hit::None => false,
    }
}

fn step(state: &Rc<ViewState>, delta: i32) -> bool {
    let kept = state.kept();
    if kept.is_empty() {
        return false;
    }
    let here = kept
        .iter()
        .position(|&n| n == state.cursor_row())
        .unwrap_or(0);
    let last = kept.len() - 1;
    let next = if delta < 0 {
        here.saturating_sub(delta.unsigned_abs() as usize)
    } else {
        (here + delta.unsigned_abs() as usize).min(last)
    };
    if next == here {
        return false;
    }
    select_event(state, kept[next]);
    true
}

/// A key belongs to what has focus, and this screen answers only for its own
/// stops.
///
/// ★ The rule R1730 measured by mounting its sibling: a page that matches on the
/// chord alone eats the presses its host's navigation was aimed at.
fn key_at(state: &Rc<ViewState>, focused: Option<&str>, chord: &str) -> bool {
    match focused {
        Some(LIST_TAG) | None => {}
        Some(_) => return false,
    }
    let kept = state.kept();
    match chord {
        "ArrowDown" => step(state, 1),
        "ArrowUp" => step(state, -1),
        "Home" => kept.first().is_some_and(|&n| {
            let moved = n != state.cursor_row();
            select_event(state, n);
            moved
        }),
        "End" => kept.last().is_some_and(|&n| {
            let moved = n != state.cursor_row();
            select_event(state, n);
            moved
        }),
        _ => false,
    }
}

// ── Scene helpers ───────────────────────────────────────────────────────────

fn absolute(rect: Rect) -> LayoutStyle {
    LayoutStyle::new()
        .with_absolute_position(rect.x, rect.y)
        .with_size(Size::px(rect.w, rect.h))
        .with_pointer_transparent(true)
}

fn run_style(px: u32, fg: Color) -> TextStyle {
    TextStyle::new()
        .with_size_px(px)
        .with_fg(fg)
        .with_overflow(TextOverflow::Ellipsis)
}

fn label(text: impl Into<String>, rect: Rect, px: u32, fg: Color) -> Scene {
    Scene::Text(TextNode::styled(text.into(), rect, run_style(px, fg)).with_layout(absolute(rect)))
}

fn tagged_label(tag: &str, text: impl Into<String>, rect: Rect, px: u32, fg: Color) -> Scene {
    text_run(tag, text, rect, run_style(px, fg))
}

const PANEL_FRAME: u32 = 1;

fn panel_content(rect: Rect) -> Rect {
    pinion_core::containment::content_of(
        Rect::new(0, 0, rect.w, rect.h),
        Some(&Border::new(Color::rgba(0, 0, 0, 0), PANEL_FRAME)),
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

/// One part of a specified surface: the tag the specification compares, on a box
/// that HOLDS that part's marks, in the part's own coordinates.
fn part_box(tag: &str, rect: Rect, children: Vec<Scene>) -> Scene {
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(tag.to_owned())
            .with_layout(absolute(rect)),
    )
}

// ── The view ────────────────────────────────────────────────────────────────

fn view(field: (TextFieldState, u32), _frame: Frame) -> Scene {
    let state = use_view_state();
    let theme = use_theme(THEME_TAG).theme_animated();
    let ink = ink();
    let (w, h) = window_size();
    Scene::Container(
        ContainerNode::new(vec![
            panel(
                ROOT_TAG,
                Rect::new(0, 0, w, h),
                ink.bg,
                None,
                vec![
                    header_bar(&state, field, &theme, ink),
                    column_header(ink),
                    list_pane(&state, ink),
                    detail_pane(&state, ink),
                ],
            )
            .silenced(Silence::layout(
                "places the header, the column row, the event list and the decode pane",
            )),
        ])
        .with_tag(VIEW_TAG)
        .with_layout(
            LayoutStyle::new()
                .with_size(Size::px(w, h))
                .with_silence(Silence::layout(
                    "the window's receiver; it holds the screen",
                )),
        ),
    )
}

fn filter_field_style() -> tf_paint::TextFieldStyle {
    tf_paint::TextFieldStyle {
        field_w: spec::FILTER_W,
        field_h: spec::FILTER_H,
        field_pad: 8,
        font_size_px: FONT_SMALL,
        ..tf_paint::TextFieldStyle::m3_filled()
    }
}

fn header_bar(
    state: &Rc<ViewState>,
    field: (TextFieldState, u32),
    theme: &Theme,
    ink: Ink,
) -> Scene {
    let rect = header_rect();
    let mut children = Vec::new();
    for (key, at) in header_parts() {
        let tag = format!("{HEADER_TAG}.{key}");
        match key {
            "title" => children.push(
                tagged_label(&tag, spec::HEADER[0].title, at, FONT_TITLE, ink.text)
                    .silenced(Silence::name_of(HEADER_TAG)),
            ),
            "live" => children.push(tagged_label(
                &tag,
                state.capture_reading(),
                at,
                FONT_SMALL,
                if state.capturing.get() {
                    ink.accent
                } else {
                    ink.text_3
                },
            )),
            "filter" => children.push(
                part_box(
                    &tag,
                    at,
                    vec![Scene::Scroll(pinion_core::scene::ScrollNode::new(
                        Rect::new(0, 0, at.w, at.h),
                        tf_paint::view_field(
                            QUERY_TAG,
                            field.0,
                            field.1,
                            theme,
                            &filter_field_style(),
                            spec::FILTER_PLACEHOLDER,
                        ),
                    ))],
                )
                .silenced(Silence::layout(
                    "places the filter box; the field inside it is what a reader lands on",
                )),
            ),
            _ => children.push(part_box(&tag, at, severity_choice(state, ink))),
        }
    }
    panel(HEADER_TAG, rect, ink.surface, Some(ink.outline), children)
}

/// The three severity marks, in the severity part's own coordinates.
fn severity_choice(state: &Rc<ViewState>, ink: Ink) -> Vec<Scene> {
    let chosen = state.choice.get();
    let mut out = Vec::with_capacity(spec::CHOICES.len() * 2);
    for (n, choice) in spec::CHOICES.iter().enumerate() {
        let at = choice_rect(n);
        let on = n == chosen;
        out.push(
            box_at(
                &format!("lv.severity.{}", choice.key),
                at,
                if on { ink.accent_soft } else { ink.surface_2 },
                Some(if on { ink.accent } else { ink.outline }),
                7,
            )
            .with_focusable(false),
        );
        out.push(label(
            choice.title,
            Rect::new(at.x + 8, at.y + 7, at.w.saturating_sub(16), 13),
            FONT_SMALL,
            match choice.floor {
                _ if on => ink.accent,
                Some(spec::Severity::Warn) => ink.warn,
                Some(spec::Severity::Error) => ink.err,
                _ => ink.text_2,
            },
        ));
    }
    out
}

fn column_header(ink: Ink) -> Scene {
    let rect = colhead_rect();
    let mut children = Vec::new();
    for (n, column) in spec::COLUMNS.iter().enumerate() {
        let at = column_rect(n);
        children.push(tagged_label(
            &format!("lv.column.{}", column.key),
            column.title,
            Rect::new(at.x, 10, at.w, 12),
            10,
            ink.text_3,
        ));
    }
    panel("lv.colhead", rect, ink.bg, Some(ink.outline), children).silenced(Silence::layout(
        "places the column headers; the grid announces them as its header row",
    ))
}

fn list_pane(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let rect = list_rect();
    let open = state.cursor_row();
    let mut children = Vec::new();
    for (visual, &n) in state.kept().iter().enumerate() {
        children.extend(list_row_paint(n, visual, open, ink));
    }
    panel(
        LIST_TAG,
        rect,
        ink.bg,
        Some(ink.outline),
        vec![
            scroll_pane(
                &state.list_scroll,
                panel_content(rect),
                (0, 0),
                PanePointer::PassesThrough,
                children,
            )
            .silenced(Silence::layout(
                "scrolls the events; the rows inside it are what a reader lands on",
            )),
        ],
    )
    .with_focusable(true)
}

fn list_row_paint(n: usize, visual: usize, open: usize, ink: Ink) -> Vec<Scene> {
    let row = &spec::ROWS[n];
    let at = list_row_rect(visual);
    let mut children = Vec::with_capacity(spec::COLUMNS.len() + 3);
    if n == open {
        children.push(
            box_at("lv.list.open", at, ink.accent_soft, Some(ink.accent), 0).silenced(
                Silence::decorative("the band behind the open event; the row says so"),
            ),
        );
    }
    children.push(box_at(
        &format!("lv.list.row.{n}"),
        at,
        Color::rgba(0, 0, 0, 0),
        None,
        0,
    ));
    for (c, column) in spec::COLUMNS.iter().enumerate() {
        let col = column_rect(c);
        let cell = Rect::new(col.x, at.y + 11, col.w, 14);
        let fg = match column.key {
            "time" | "source" => ink.text_2,
            "severity" => severity_ink(row.severity, ink),
            "type" => kind_ink(row.kind),
            _ => ink.text,
        };
        if column.key == "severity" {
            children.push(
                box_at(
                    &format!("lv.list.dot.{n}"),
                    Rect::new(cell.x, cell.y + 4, 6, 6),
                    fg,
                    None,
                    3,
                )
                .silenced(Silence::decorative("repeats the severity the cell reads")),
            );
        }
        let text_x = if column.key == "severity" {
            cell.x + 12
        } else {
            cell.x
        };
        children.push(tagged_label(
            &format!("lv.list.cell.{n}_{}", column.key),
            row.cell(column.key),
            Rect::new(text_x, cell.y, cell.w.saturating_sub(text_x - cell.x), 14),
            FONT_BODY,
            fg,
        ));
    }
    children
}

fn detail_pane(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let rect = detail_rect();
    let record = state.record();
    let mut children = Vec::new();
    for (key, at) in detail_parts(record) {
        let tag = format!("{DETAIL_TAG}.{key}");
        let marks = detail_part_paint(key, at, record, ink);
        children.push(if key == "subject" {
            part_box(&tag, at, marks).silenced(Silence::name_of(DETAIL_TAG))
        } else {
            part_box(&tag, at, marks)
        });
    }
    panel(DETAIL_TAG, rect, ink.surface, Some(ink.outline), children).with_focusable(true)
}

/// What one part of the decode pane draws, in the part's own coordinates.
fn detail_part_paint(key: &str, at: Rect, record: &'static spec::RowSpec, ink: Ink) -> Vec<Scene> {
    let whole = Rect::new(0, 0, at.w, at.h);
    let title_of = |key: &str| {
        spec::DETAIL
            .iter()
            .find(|p| p.key == key)
            .map_or("", |p| p.title)
    };
    match key {
        "subject" => vec![label(title_of("subject"), whole, FONT_TITLE, ink.text)],
        "kind" => vec![
            box_at(
                "lv.detail.kind.pill",
                whole,
                Color::rgba(0x2A, 0x2E, 0x36, 0xB0),
                None,
                6,
            )
            .silenced(Silence::decorative("the tone behind the type tag")),
            label(
                record.kind,
                Rect::new(9, 4, whole.w.saturating_sub(18), 13),
                FONT_SMALL,
                kind_ink(record.kind),
            ),
        ],
        "message" => vec![label(record.message, whole, FONT_BODY, ink.text)],
        "meta" => vec![label(
            format!(
                "{} · {} · src {}",
                record.time,
                record.severity.label(),
                record.source
            ),
            whole,
            FONT_SMALL,
            severity_ink(record.severity, ink),
        )],
        "layers" => decoded_fields(whole, record, ink),
        "bytes" => wire_bytes(whole, record, ink),
        _ => Vec::new(),
    }
}

fn decoded_fields(at: Rect, record: &'static spec::RowSpec, ink: Ink) -> Vec<Scene> {
    let mut out = vec![label(
        "DECODED LAYERS",
        Rect::new(0, 0, at.w, 12),
        10,
        ink.text_3,
    )];
    for (n, (name, value)) in record.fields.iter().enumerate() {
        let y = spec::LIST_LABEL_H + u32::try_from(n).unwrap_or(0) * spec::FIELD_H;
        out.push(label(
            *name,
            Rect::new(0, y + 4, 88, 13),
            FONT_SMALL,
            ink.text_3,
        ));
        out.push(label(
            *value,
            Rect::new(96, y + 4, at.w.saturating_sub(96), 13),
            FONT_SMALL,
            ink.text,
        ));
    }
    out
}

/// The frame's bytes, drawn from the framework's own dump geometry.
///
/// ★ The glyphs come from [`HexLayout::glyph_at`] rather than from a second
/// formatting loop here, which is R1613's rule: what is drawn and what the
/// geometry says is there are one fact. A row with no frame says so — the
/// reference draws that case rather than hiding it, and a blank block would be
/// indistinguishable from a decode that failed.
fn wire_bytes(at: Rect, record: &'static spec::RowSpec, ink: Ink) -> Vec<Scene> {
    let mut out = vec![label(
        "WIRE BYTES",
        Rect::new(0, 0, at.w, 12),
        10,
        ink.text_3,
    )];
    if record.bytes.is_empty() {
        out.push(label(
            spec::NO_FRAME,
            Rect::new(0, spec::LIST_LABEL_H + 2, at.w, 14),
            FONT_SMALL,
            ink.warn,
        ));
        return out;
    }
    let layout = byte_layout(record.bytes);
    for row in 0..layout.rows() {
        let mut line = String::with_capacity(layout.total_cols());
        for col in 0..layout.total_cols() {
            line.push(layout.glyph_at(
                record.bytes,
                pinion_core::widgets::hex_dump::Cell::new(col, row),
            ));
        }
        let y = spec::LIST_LABEL_H + u32::try_from(row).unwrap_or(0) * 18;
        out.push(label(
            line.trim_end().to_owned(),
            Rect::new(0, y, at.w, 15),
            FONT_SMALL,
            ink.text_2,
        ));
    }
    out
}

// ── The three surfaces, as this screen builds them ──────────────────────────

/// One surface's parts, as the running screen's own tables declare them.
///
/// # Panics
///
/// If asked for a surface the specification does not name, which is a defect in
/// this file.
#[must_use]
fn built(surface: &str) -> Vec<Part> {
    match surface {
        "header" => spec::HEADER
            .iter()
            .map(|p| Part::new(p.key, p.title))
            .collect(),
        "columns" => spec::COLUMNS
            .iter()
            .map(|c| Part::new(c.key, c.title))
            .collect(),
        "detail" => spec::DETAIL
            .iter()
            .map(|p| Part::new(p.key, p.title))
            .collect(),
        other => panic!("no surface named {other}"),
    }
}

fn conformance_json() -> serde_json::Value {
    spec::document().wire(&built)
}

/// The screen, as a table a client reads instead of a screenshot.
fn spec_json() -> serde_json::Value {
    serde_json::json!({
        "window": { "w": WIN_W, "h": WIN_H },
        "columns": spec::COLUMNS
            .iter()
            .map(|c| serde_json::json!({ "key": c.key, "title": c.title, "width": c.width }))
            .collect::<Vec<_>>(),
        "detail": spec::DETAIL
            .iter()
            .map(|p| serde_json::json!({ "key": p.key, "title": p.title }))
            .collect::<Vec<_>>(),
        "header": spec::HEADER
            .iter()
            .map(|p| serde_json::json!({ "key": p.key, "title": p.title }))
            .collect::<Vec<_>>(),
        // ★ The vocabulary AND the control, because they are two facts: how bad
        // an event can be, and which floors a reader may set. An agent given
        // only the controls could not tell whether a severity it saw on a row
        // is one it can filter to.
        "severity_vocabulary": spec::Severity::ALL
            .iter()
            .map(|s| s.label())
            .collect::<Vec<_>>(),
        "severities": spec::CHOICES
            .iter()
            .map(|c| serde_json::json!({
                "key": c.key,
                "title": c.title,
                "floor": c.floor.map(spec::Severity::label),
            }))
            .collect::<Vec<_>>(),
        "gestures": spec::GESTURES
            .iter()
            .map(|(g, does)| serde_json::json!({ "gesture": g, "does": does }))
            .collect::<Vec<_>>(),
        "rows": spec::ROWS
            .iter()
            .map(|r| serde_json::json!({
                "time": r.time,
                "severity": r.severity.label(),
                "source": r.source,
                "type": r.kind,
                "message": r.message,
                "fields": r.fields.iter().map(|(k, v)| serde_json::json!([k, v]))
                    .collect::<Vec<_>>(),
                "bytes": r.bytes.len(),
            }))
            .collect::<Vec<_>>(),
    })
}

// ── The external ────────────────────────────────────────────────────────────

/// The screen's introspection surface, and the pointer target for its own hit
/// test.
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
        let (px, py) = pinion_core::external::layout_point(VIEW_TAG, at.at);
        move_cursor(&state, px, py);
    }

    fn target_at(&self, x: u32, y: u32) -> PointerTarget {
        self.state.as_ref().map_or(PointerTarget::Unanswered, |s| {
            Hit::at(s, x, y)
                .word()
                .map_or(PointerTarget::Nothing, PointerTarget::Word)
        })
    }

    fn target_of_tag(&self, tag: &str) -> PointerTarget {
        Hit::of_tag(tag)
            .word()
            .map_or(PointerTarget::Nothing, PointerTarget::Word)
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
                    SchemaField::new("conformance", "json"),
                    SchemaField::new("row_count", "int"),
                    SchemaField::new("selected_row", "int"),
                    SchemaField::new("record", "json"),
                    SchemaField::new("severity", "string"),
                    SchemaField::new("capturing", "bool"),
                    SchemaField::new("capture_reading", "string"),
                    SchemaField::new("kept_rows", "json"),
                    SchemaField::new("query", "string"),
                    SchemaField::new("query_fault", "string"),
                    SchemaField::new("why_hidden", "json"),
                    SchemaField::new("said", "object"),
                    SchemaField::parametric(
                        "hit.<x>.<y>",
                        "string",
                        const { &[SchemaArg::open("x", "int"), SchemaArg::open("y", "int")] },
                    ),
                    SchemaField::action("select_event", "int"),
                    SchemaField::action("choose_severity", "string"),
                    SchemaField::action("capture", "string"),
                    SchemaField::action("filter", "string"),
                    SchemaField::action("point", "string"),
                    SchemaField::action("press", "string"),
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
            "conformance" => Ok(IntrospectValue::Json(conformance_json())),
            "row_count" => Ok(IntrospectValue::Int(
                i64::try_from(spec::ROWS.len()).unwrap_or(i64::MAX),
            )),
            "selected_row" => Ok(IntrospectValue::Int(
                i64::try_from(state.cursor_row()).unwrap_or(i64::MAX),
            )),
            "record" => {
                let record = state.record();
                Ok(IntrospectValue::Json(serde_json::json!({
                    "time": record.time,
                    "severity": record.severity.label(),
                    "source": record.source,
                    "type": record.kind,
                    "message": record.message,
                    "fields": record.fields.iter().map(|(k, v)| serde_json::json!([k, v]))
                        .collect::<Vec<_>>(),
                    "bytes": record.bytes.len(),
                })))
            }
            "severity" => Ok(IntrospectValue::Text(
                spec::CHOICES[state.choice.get()].key.to_owned(),
            )),
            "capturing" => Ok(IntrospectValue::Bool(state.capturing.get())),
            "capture_reading" => Ok(IntrospectValue::Text(state.capture_reading())),
            "kept_rows" => Ok(IntrospectValue::Json(serde_json::json!(state.kept()))),
            "query" => Ok(IntrospectValue::Text(state.query.text())),
            "query_fault" => Ok(IntrospectValue::Text(
                state.query_fault().unwrap_or_default(),
            )),
            "why_hidden" => {
                let kept = state.kept();
                let query = state.query();
                let floor = state.floor();
                Ok(IntrospectValue::Json(serde_json::json!(
                    (0..spec::ROWS.len())
                        .filter(|n| !kept.contains(n))
                        .map(|n| {
                            let cells = spec::ROWS[n].attributes();
                            // ★ WHICH narrowing dropped it. Two controls narrow
                            // this list and a reader who is told only "hidden"
                            // has to guess which one to undo.
                            let by_severity =
                                floor.is_some_and(|least| spec::ROWS[n].severity < least);
                            serde_json::json!({
                                "row": n,
                                "severity": by_severity,
                                "clause": query
                                    .rejecting_clause(|c| {
                                        cells.get(c).map_or("", String::as_str)
                                    })
                                    .map(|clause| clause.text.clone()),
                            })
                        })
                        .collect::<Vec<_>>()
                )))
            }
            "said" => Ok(IntrospectValue::Text(state.said_sentence())),
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

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.state()?.clone();
        match path {
            "select_event" => {
                let n = args
                    .as_usize()
                    .ok_or_else(|| InvokeError::rejected("expected a row index"))?;
                if n >= spec::ROWS.len() {
                    return Err(InvokeError::rejected(format!("no event {n}")));
                }
                select_event(&state, n);
                Ok(IntrospectValue::Int(i64::try_from(n).unwrap_or(i64::MAX)))
            }
            "choose_severity" => {
                let key = Self::text(&args)?;
                let n = spec::CHOICES
                    .iter()
                    .position(|c| c.key == key)
                    .ok_or_else(|| InvokeError::rejected("no such severity choice"))?;
                choose_severity(&state, n);
                Ok(IntrospectValue::Int(
                    i64::try_from(state.kept().len()).unwrap_or(i64::MAX),
                ))
            }
            "capture" => {
                let on = match Self::text(&args)?.as_str() {
                    "on" => true,
                    "off" => false,
                    _ => return Err(InvokeError::rejected("expected \"on\" or \"off\"")),
                };
                set_capturing(&state, on);
                Ok(IntrospectValue::Bool(on))
            }
            "filter" => {
                set_query(&state, &Self::text(&args)?);
                announce_query(&state);
                Ok(IntrospectValue::Int(
                    i64::try_from(state.kept().len()).unwrap_or(i64::MAX),
                ))
            }
            "point" => {
                let text = Self::text(&args)?;
                let (x, y) = text
                    .split_once(',')
                    .ok_or_else(|| InvokeError::rejected("expected \"x,y\""))?;
                let point = (
                    x.trim()
                        .parse()
                        .map_err(|_| InvokeError::rejected("x is not a number"))?,
                    y.trim()
                        .parse()
                        .map_err(|_| InvokeError::rejected("y is not a number"))?,
                );
                move_cursor(&state, point.0, point.1);
                Ok(IntrospectValue::Text(format!("{},{}", point.0, point.1)))
            }
            "press" | "send" => Ok(IntrospectValue::Bool(press(&state))),
            "key" => {
                let chord = Self::text(&args)?;
                Ok(IntrospectValue::Bool(key_at(&state, None, &chord)))
            }
            _ => Err(InvokeError::rejected("no such action")),
        }
    }
}

// ── The binding ─────────────────────────────────────────────────────────────

/// ★ R1731 — public from the first round, because this screen is both a window
/// of its own and a **page** of the analysis-tool shell
/// (`pinion_screen::Mount<LogView>`).
pub struct LogView;

impl WidgetCore for LogView {
    type State = (TextFieldState, u32);
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = ViewOracle::new();
        oracle.attach(use_view_state());
        Box::new(oracle)
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![pinion_core::widgets::text_field::blur_committing_field_extra(QUERY_TAG)]
    }

    fn tag() -> &'static str {
        VIEW_TAG
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
        "pinion hello-log-view (R1731 §5.41 log section)"
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        chord: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused == Some(QUERY_TAG) {
            let state = use_view_state();
            return edit_field_keymap(
                scene,
                QUERY_TAG,
                chord,
                modifiers,
                CellKind::Text,
                || announce_query(&state),
                || {},
            );
        }
        key_at(&use_view_state(), focused, chord)
    }
}

impl WidgetA11y for LogView {
    fn access_node(_state: &(TextFieldState, u32), focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_view_state();
        let mut nodes = vec![
            AccessNode::new(ROOT_TAG, AriaRole::Group)
                .with_name("Logs")
                .with_child(HEADER_TAG)
                .with_child(LIST_TAG)
                .with_child(DETAIL_TAG),
        ];
        nodes.extend(header_nodes(&state));
        nodes.extend(list_nodes(&state, focused));
        nodes.extend(detail_nodes(&state));
        nodes
    }

    fn access_focus_target(
        _state: &(TextFieldState, u32),
        focused: Option<&str>,
    ) -> Option<AccessFocus> {
        let state = use_view_state();
        (focused == Some(LIST_TAG)).then(|| {
            AccessFocus::composite(LIST_TAG, format!("lv.list.row.{}", state.cursor_row()))
        })
    }
}

fn header_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    let chosen = state.choice.get();
    let mut nodes = vec![
        AccessNode::new(HEADER_TAG, AriaRole::Group)
            .with_name(spec::HEADER[0].title)
            .with_child("lv.header.live")
            .with_child(QUERY_TAG)
            .with_child("lv.header.severity"),
        // The capture state is a live region: it changes without a reader
        // touching it, which is the definition.
        AccessNode::new("lv.header.live", AriaRole::Status)
            .with_name("Capture state")
            .with_value(AccessValue::Text(state.capture_reading()))
            .with_live(AccessLive::Polite),
        AccessNode::new(QUERY_TAG, AriaRole::TextInput)
            .with_name("Filter")
            .with_value(AccessValue::Text(state.query.text())),
        // ★ The group OWNS its members. A `radiogroup` that declares no child
        // of the role it promises is what the framework's structure gate calls
        // *empty*, and it caught this on the demo's first run: the three marks
        // were built and announced and the group did not say they were its.
        spec::CHOICES.iter().fold(
            AccessNode::new("lv.header.severity", AriaRole::RadioGroup)
                .with_name("Severity")
                .with_value(AccessValue::Text(spec::CHOICES[chosen].title.to_owned())),
            |group, choice| group.with_child(format!("lv.severity.{}", choice.key)),
        ),
    ];
    for (n, choice) in spec::CHOICES.iter().enumerate() {
        nodes.push(
            AccessNode::new(format!("lv.severity.{}", choice.key), AriaRole::RadioButton)
                .with_name(choice.title)
                .with_selected(n == chosen)
                .with_set_position(n, spec::CHOICES.len()),
        );
    }
    nodes
}

fn list_nodes(state: &Rc<ViewState>, focused: Option<&str>) -> Vec<AccessNode> {
    let open = state.cursor_row();
    let columns: Vec<GridColumn> = spec::COLUMNS
        .iter()
        .map(|column| GridColumn {
            tag: format!("lv.column.{}", column.key),
            sort: None,
        })
        .collect();
    let rows: Vec<GridRow> = state
        .kept()
        .into_iter()
        .map(|n| GridRow {
            tag: format!("lv.list.row.{n}"),
            selected: n == open,
            state: RadioState::Idle,
            cells: spec::COLUMNS
                .iter()
                .map(|column| GridCell {
                    tag: format!("lv.list.cell.{n}_{}", column.key),
                    name: format!("{}: {}", column.title, spec::ROWS[n].cell(column.key)),
                    focused: focused == Some(LIST_TAG) && n == open,
                    selected: None,
                })
                .collect(),
        })
        .collect();
    grid_table_nodes(LIST_TAG, "Events", false, LIST_HEADER, &columns, &rows)
}

fn detail_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    let record = state.record();
    let mut pane = AccessNode::new(DETAIL_TAG, AriaRole::Group).with_name(spec::DETAIL[0].title);
    for part in &spec::DETAIL[1..] {
        pane = pane.with_child(format!("lv.detail.{}", part.key));
    }
    let mut nodes = vec![pane];
    for part in &spec::DETAIL[1..] {
        let tag = format!("lv.detail.{}", part.key);
        nodes.push(
            AccessNode::new(tag, AriaRole::Group)
                .with_name(part.title)
                .with_value(AccessValue::Text(detail_reading(part.key, record))),
        );
    }
    nodes
}

/// What one part of the decode pane reads as.
fn detail_reading(key: &str, record: &'static spec::RowSpec) -> String {
    match key {
        "kind" => record.kind.to_owned(),
        "message" => record.message.to_owned(),
        "meta" => format!(
            "{}, {}, from {}",
            record.time,
            record.severity.label(),
            record.source
        ),
        "layers" => record
            .fields
            .iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect::<Vec<_>>()
            .join(", "),
        "bytes" => {
            if record.bytes.is_empty() {
                spec::NO_FRAME.to_owned()
            } else {
                format!("{} bytes", record.bytes.len())
            }
        }
        _ => String::new(),
    }
}

impl WidgetView for LogView {
    type Renderer = HelloLogViewRenderer;

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

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::shrinking(SHRINK, (WIN_W, WIN_H))
    }

    fn shrink_policy() -> Option<ShrinkPolicy> {
        Some(SHRINK)
    }
}

/// Which byte of the filter's text a window point lands on.
fn query_byte_at(
    posture: TextFieldState,
    scene: &Scene,
    focused: Option<&str>,
    x: f32,
    y: f32,
) -> Option<usize> {
    if focused != Some(QUERY_TAG) {
        return None;
    }
    let rect = pinion_shell::rect_for_tag(scene, QUERY_TAG)?;
    if !rect.contains_point(x, y) {
        return None;
    }
    tf_paint::byte_for_scene_point(
        QUERY_TAG,
        posture,
        scene,
        x,
        y,
        &use_theme(THEME_TAG).theme_animated(),
        &filter_field_style(),
    )
}

/// Run the log section as an application of its own.
pub fn run() {
    pinion_shell::run::<LogView>();
}
