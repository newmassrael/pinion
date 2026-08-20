// R1412 §5.49 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-key-patterns` — R1730 §5.27 §5.40 §5.41 — the analysis tool's
//! **key-pattern section**, the third seat of the reference's rail, assembled
//! as one screen against a specification written down somewhere else.
//!
//! ## What forced this example
//!
//! `docs/analyzer-rail-spec.json` carried two accepted divergences from the
//! reference's navigation, and this is one of them: *`keys` is specified open
//! and is closed (unbuilt)*. The reference implements the section — rows and a
//! record pane — and this tree drew the seat, named it and refused it. R1728
//! made that refusal honest; a refusal is not a reproduction.
//!
//! ## What is new, and what is assembly
//!
//! The screen is assembly: a list, a record pane and a filter, all out of
//! substrates this tree already had. What is new is the shape the round is
//! named for — [`pinion_core::conformance`], a **surface** written down and
//! compared with the built one in both directions, with a ledger of accepted
//! differences that fails when a difference is paid off and not recorded.
//!
//! R1728 gave a *navigation* that treatment and it found three defects in its
//! first three runs. Everything a screen is made of that is not a navigation —
//! a list's columns, a pane's sections, a header's parts — had no way to be
//! checked at all. This screen is the first consumer, three times over: its
//! three surfaces are compared with `docs/analyzer-keys-spec.json`, and it
//! publishes the result on the wire so an agent can ask how much of the section
//! is really here before it plans anything.
//!
//! ## The screen
//!
//! ```text
//! cargo run -p hello-key-patterns --release
//! ```
//!
//! A 46-high section header — the section's name, a **derived** summary and a
//! live filter — over a seven-column declaration list, beside a 320-wide record
//! pane of eleven parts. Click a declaration to open its record; type in the
//! filter to narrow the list; the arrows walk it. The record pane's last part
//! is the reference's own action out of the section, and it refuses with the
//! reason the rail specification gives that seat.
//!
//! See `tools/demos/r1730_a_section_is_the_one_the_reference_draws.py`.

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
use pinion_core::availability::{Recourse, Unavailable, UnavailableKind};
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

vello_renderer_impl!(HelloKeyPatternsRenderer, HelloKeyPatternsRendererError);

// ── Tags ────────────────────────────────────────────────────────────────────

/// The tag the widget is registered under — the receiver a press resolves to.
const VIEW_TAG: &str = "key_patterns";
/// The root address, for `scene/snapshot` and the sweep.
const ROOT_TAG: &str = "kp.root";
/// The theme scope.
const THEME_TAG: &str = "app";
/// The filter box: the tag its own external is addressed by, its buffer is
/// keyed on, and it is painted under. One name.
///
/// Deliberately **outside** the `kp.header.` namespace the header's parts live
/// in. A surface's parts are read back by walking that prefix and taking the
/// names with no further dot in them, so a child tagged inside a part would
/// have to be excluded by name — and R1728 measured what naming an exclusion
/// costs: the gate is then only as good as whoever last updated the list.
const QUERY_TAG: &str = "kp.filter.query";
/// The list's accessibility header row. Nothing paints it — the column headers
/// are painted individually — so it is anchored by the members it composes.
const LIST_HEADER: &str = "kp.list.header";
/// The list's grid.
const LIST_TAG: &str = "kp.list";
/// The record pane.
const DETAIL_TAG: &str = "kp.detail";
/// The section header.
const HEADER_TAG: &str = "kp.header";

const FONT_SMALL: u32 = 11;
const FONT_BODY: u32 = 12;
const FONT_TITLE: u32 = 14;
const FONT_SUBJECT: u32 = 16;

// ── Geometry ────────────────────────────────────────────────────────────────

const WIN_W: u32 = spec::WIN_W;
const WIN_H: u32 = spec::WIN_H;
const HEADER_H: u32 = spec::HEADER_H;
const COLHEAD_H: u32 = spec::COLHEAD_H;
const ROW_H: u32 = spec::ROW_H;
const DETAIL_W: u32 = spec::DETAIL_W;
const PAD: u32 = spec::PAD;
const GAP: u32 = spec::GAP;

/// The smallest width this section lays out completely at, **derived from the
/// reference's own grid**.
///
/// The reference gives its pattern column `minmax(150px, 1fr)` — a stated
/// minimum for the one column that flexes — and fixes the other six. So the
/// narrowest complete layout is the record pane, plus the row's padding, plus
/// the six fixed widths and their gaps, plus that 150. Written as a sum rather
/// than as a number because every term of it is a fact somewhere else, and a
/// number here would go stale the first time a column's width moved.
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
    DETAIL_W + 2 * PAD + fixed + gaps + spec::PATTERN_MIN
}

/// The smallest width the layout is complete at.
const MIN_W: u32 = min_width();
/// The smallest height it is complete at — the record pane's own stack, which
/// is the taller of the two columns. Asserted against the laid-out stack by
/// `tests.rs` rather than trusted, because it is the one term of the floor that
/// arithmetic here cannot derive.
const MIN_H: u32 = 460;

/// ★ R1712 — what this screen concedes when it is not given the room it wants.
///
/// The list's own columns are what it gives up first, and naming that is the
/// point of the declaration: six of the seven have widths the specification
/// fixes, so a window narrower than the layout takes the difference out of the
/// pattern column until that column reaches the minimum the reference gives it,
/// and below the floor the right-hand columns clip. The record pane does not
/// shrink — it restates a row the list is still showing, and a pane that
/// narrowed would elide the very values a reader opened it for.
const SHRINK: ShrinkPolicy = ShrinkPolicy::conceding(
    (MIN_W, MIN_H),
    (760, 420),
    &["the columns right of the pattern clip before the record pane narrows"],
);

/// The section opens larger than the narrowest layout it can manage.
///
/// A `const` assertion rather than a test, because it is decidable at compile
/// time and a screen whose opening size is below its own layout floor should
/// not build. `ShrinkPolicy::conceding` already refuses a floor above the
/// comfortable size; this is the other pair, which nothing else checks.
const _: () = assert!(
    MIN_W < WIN_W && MIN_H < WIN_H,
    "the section must open larger than the narrowest layout it can manage",
);

fn window_size() -> (u32, u32) {
    pinion_core::external::layout_size(VIEW_TAG, SHRINK.comfortable(), (WIN_W, WIN_H))
}

/// The whole area left of the record pane.
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
///
/// One derivation, read by the header painter, every row painter, the hit test
/// and the accessibility tree — so a column cannot move for one of them.
fn column_rect(n: usize) -> Rect {
    let gaps = u32::try_from(spec::COLUMNS.len().saturating_sub(1)).unwrap_or(0);
    let flexible = list_column_rect()
        .w
        .saturating_sub(2 * PAD)
        .saturating_sub(fixed_width())
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

/// The widths the specification fixes, added up.
fn fixed_width() -> u32 {
    spec::COLUMNS.iter().map(|c| c.width).sum()
}

/// Row `visual`'s rectangle inside the scrolling list body.
fn list_row_rect(visual: usize) -> Rect {
    Rect::new(
        0,
        u32::try_from(visual).unwrap_or(0) * ROW_H,
        list_rect().w,
        ROW_H,
    )
}

/// The record pane's parts, in paint order, each with the rectangle it was
/// given — in the pane's own coordinates.
///
/// ★★★★★ The one derivation the painter, the hit test and the painted-roster
/// check all read. A part that shares its row with the next one is laid out
/// beside it, which is how the reference draws its four single facts as a
/// two-by-two grid; everything else stacks.
fn detail_parts() -> Vec<(&'static str, Rect)> {
    let inner = DETAIL_W.saturating_sub(2 * PAD);
    let half = inner.saturating_sub(9) / 2;
    let mut out = Vec::with_capacity(spec::DETAIL.len());
    // The pane's own heading strip holds the first two parts side by side.
    out.push(("subject", Rect::new(PAD, 14, 160, 20)));
    out.push((
        "ordinal",
        Rect::new(DETAIL_W.saturating_sub(PAD + 48), 14, 48, 20),
    ));
    let mut y = HEADER_H + 18;
    let mut n = 2;
    while n < spec::DETAIL.len() {
        let part = &spec::DETAIL[n];
        if part.pairs {
            out.push((part.key, Rect::new(PAD, y, half, part.height)));
            let beside = &spec::DETAIL[n + 1];
            out.push((
                beside.key,
                Rect::new(PAD + half + 9, y, half, beside.height),
            ));
            y += part.height.max(beside.height) + 9;
            n += 2;
            continue;
        }
        out.push((part.key, Rect::new(PAD, y, inner, part.height)));
        y += part.height + 16;
        n += 1;
    }
    out
}

/// One part's rectangle, by key.
fn detail_part_rect(key: &str) -> Option<Rect> {
    detail_parts()
        .into_iter()
        .find(|(k, _)| *k == key)
        .map(|(_, rect)| rect)
}

/// The header's three parts, left to right, in the header's own coordinates.
fn header_parts() -> Vec<(&'static str, Rect)> {
    let w = header_rect().w;
    vec![
        ("title", Rect::new(PAD, 14, 120, 18)),
        ("summary", Rect::new(PAD + 132, 15, 260, 16)),
        (
            "filter",
            Rect::new(
                w.saturating_sub(PAD + spec::FILTER_W),
                (HEADER_H.saturating_sub(spec::FILTER_H)) / 2,
                spec::FILTER_W,
                spec::FILTER_H,
            ),
        ),
    ]
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
    ok: Color,
    warn: Color,
    declaration: Color,
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
        ok: rgb(0x35_C08B),
        warn: rgb(0xD9_A21B),
        declaration: rgb(0xC7_7800),
    }
}

fn health_ink(health: spec::Health, ink: Ink) -> Color {
    match health {
        spec::Health::Resolved => ink.ok,
        spec::Health::NumericOnly => ink.warn,
    }
}

// ── State ───────────────────────────────────────────────────────────────────

/// Everything the screen holds, and nothing it can derive.
struct ViewState {
    /// Which declaration's record is open, by index into [`spec::ROWS`].
    row: Signal<usize>,
    /// ★ The filter's own buffer, **not** a copy of it. A `Signal<String>`
    /// beside the field is the two-copies shape this tree has paid for
    /// repeatedly, and here it would fail visibly: the list would filter on the
    /// last committed query while the box showed the one being typed.
    query: Rc<TextEditState>,
    /// The list body's scroll offset.
    list_scroll: Rc<ScrollState>,
    /// Where the cursor last was, because a press carries no coordinates.
    cursor: Signal<(u32, u32)>,
    /// The last thing the screen said, for the live region and the wire.
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

    /// The running query, parsed.
    ///
    /// A malformed query keeps everything rather than nothing: a half-typed
    /// query is malformed on nearly every keystroke, and a screen that emptied
    /// its list while a person typed would flash the section away and back. The
    /// refusal is not swallowed — it is what [`query_fault`](Self::query_fault)
    /// answers and what the header paints.
    fn query(&self) -> RowQuery {
        RowQuery::parse(&self.query.text(), &spec::query_columns()).unwrap_or_default()
    }

    fn query_fault(&self) -> Option<String> {
        RowQuery::parse(&self.query.text(), &spec::query_columns())
            .err()
            .map(|e| e.to_string())
    }

    /// The source indices the query keeps, in declaration order.
    ///
    /// The ONE derivation. The painter, the hit test, the keyboard, the
    /// accessibility tree and the wire all read it, so the list a person sees
    /// and the list a press lands in cannot be two lists.
    fn kept(&self) -> Vec<usize> {
        let query = self.query();
        if query.is_everything() {
            return (0..spec::ROWS.len()).collect();
        }
        (0..spec::ROWS.len())
            .filter(|&n| {
                let cells = spec::ROWS[n].attributes();
                query.admit(|c| cells.get(c).map_or("", String::as_str)) == Admission::Admitted
            })
            .collect()
    }

    /// Which declaration the list's cursor is on: the open one when the query
    /// kept it, else the first it did keep, else the open one again.
    fn cursor_row(&self) -> usize {
        let open = self.row.get();
        let kept = self.kept();
        if kept.contains(&open) {
            return open;
        }
        kept.first().copied().unwrap_or(open)
    }

    /// The record the pane is showing.
    fn record(&self) -> &'static spec::RowSpec {
        &spec::ROWS[self.cursor_row()]
    }

    /// What the header's summary says.
    ///
    /// **Derived**, because the reference derives it: it counts the
    /// declarations and the ones that resolved to a number only. A build that
    /// painted the sentence as a constant would show `8 declared` under a
    /// filter that kept two.
    fn summary(&self) -> String {
        let kept = self.kept();
        let unresolved = kept
            .iter()
            .filter(|&&n| spec::ROWS[n].health == spec::Health::NumericOnly)
            .count();
        let scope = if self.query().is_everything() {
            format!("{} declared", kept.len())
        } else {
            format!("{} of {} declared", kept.len(), spec::ROWS.len())
        };
        format!("{scope} · {unresolved} numeric-only")
    }
}

fn use_view_state() -> Rc<ViewState> {
    // ★ [[owner-cache-no-nested-factory]] — every cached slot this one holds is
    // resolved BEFORE the factory runs, because `Owner::cache` cannot re-enter
    // itself and a factory that calls another `use_*` hook does exactly that.
    let list_scroll = pinion_core::widgets::scroll::use_scroll_state("kp.list.body");
    let query = use_text_edit_state(QUERY_TAG);
    let owner = pinion_core::reactive::Owner::current()
        .expect("use_view_state requires an active Owner scope");
    owner.cache("key_patterns.state", || ViewState {
        row: Signal::new(spec::OPENING_ROW),
        query,
        list_scroll,
        cursor: Signal::new((0, 0)),
        said: RefCell::new(None),
    })
}

// ── The action out of the section ───────────────────────────────────────────

/// The rail specification, as text, compiled in.
///
/// The **same** artifact the shell's rail is judged against, read rather than
/// copied. The record pane's last part points at a section whose standing is
/// that file's fact, and a second copy of that fact here is how the button
/// would come to promise something the navigation refuses.
const RAIL_SPEC_JSON: &str = include_str!("../../../docs/analyzer-rail-spec.json");

/// Why the reference's own action out of this section cannot be taken.
///
/// The reference draws a live button into its topology section. The scope
/// reference draws that section **locked**, booked under a requirement of a
/// release that has not shipped — so reproducing the affordance faithfully
/// means drawing it and refusing it with that reason. Leaving it off would be a
/// divergence; making it navigate would be a promise the specification does not
/// keep.
///
/// # Panics
///
/// If the rail specification does not describe the section this points at — a
/// defect in the pin rather than a state the screen can reach.
fn declarer_standing() -> Unavailable {
    let doc: serde_json::Value =
        serde_json::from_str(RAIL_SPEC_JSON).expect("the rail specification is readable JSON");
    let seat = doc["canon"]
        .as_array()
        .expect("the rail specification declares a canon array")
        .iter()
        .find(|seat| seat["key"].as_str() == Some(spec::DECLARER_SECTION))
        .expect("the rail specification names the section this action leads to");
    let kind = seat["kind"]
        .as_str()
        .and_then(UnavailableKind::from_name)
        .unwrap_or(UnavailableKind::Unbuilt);
    let detail = seat["$note"]
        .as_str()
        .and_then(|note| note.split_once("under ").map(|(_, rest)| rest))
        .map_or_else(
            || "the behaviour reference".to_owned(),
            |rest| rest.trim_end_matches('.').to_owned(),
        );
    Unavailable::new(kind, detail)
}

// ── The hit test ────────────────────────────────────────────────────────────

/// What is under a point. One enum, resolved from the same rectangles the
/// painter uses.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hit {
    /// A declaration row, by its index in [`spec::ROWS`].
    Declaration(usize),
    /// The record pane's action, which refuses.
    Declarer,
    /// Nothing that answers.
    None,
}

impl Hit {
    /// What a **key** press at `tag` addresses.
    ///
    /// A keyboard activation names a thing, not a pixel, so synthesising a
    /// press at the middle of the tag's rectangle would be wrong for a row
    /// scrolled out of the list.
    fn of_tag(tag: &str) -> Self {
        if let Some(n) = tag
            .strip_prefix("kp.list.row.")
            .and_then(|n| n.parse::<usize>().ok())
            && n < spec::ROWS.len()
        {
            return Self::Declaration(n);
        }
        // A cell's press is its row's press — the cell labels are text runs,
        // transparent to the pointer, so a press anywhere on a cell already
        // reaches the row.
        if let Some((row, _column)) = tag
            .strip_prefix("kp.list.cell.")
            .and_then(|rest| rest.split_once('_'))
            && let Ok(row) = row.parse::<usize>()
            && row < spec::ROWS.len()
        {
            return Self::Declaration(row);
        }
        if tag == "kp.detail.declarer" {
            return Self::Declarer;
        }
        Self::None
    }

    /// The word the wire answers a press with, and therefore the word
    /// [`External::target_at`] answers with too.
    fn word(&self) -> Option<String> {
        Some(match self {
            Self::Declaration(n) => format!("declaration.{n}"),
            Self::Declarer => "declarer".to_owned(),
            Self::None => return None,
        })
    }

    /// What answers at the window point `(px, py)`.
    fn at(state: &ViewState, px: u32, py: u32) -> Self {
        let detail = detail_rect();
        if contains(detail, px, py) {
            let (dx, dy) = (px - detail.x, py - detail.y);
            if detail_part_rect("declarer").is_some_and(|r| contains(r, dx, dy)) {
                return Self::Declarer;
            }
            return Self::None;
        }
        let list = list_rect();
        if contains(list, px, py) {
            let (ox, oy) = state.list_scroll.offset();
            let lx = px.saturating_sub(list.x).saturating_add(clamp_offset(ox));
            let ly = py.saturating_sub(list.y).saturating_add(clamp_offset(oy));
            // Walk what is DRAWN: a hit test over the source rows would answer
            // a hidden declaration under a filtered list.
            for (visual, &n) in state.kept().iter().enumerate() {
                if contains(list_row_rect(visual), lx, ly) {
                    return Self::Declaration(n);
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

fn select_declaration(state: &Rc<ViewState>, row: usize) {
    if row >= spec::ROWS.len() {
        return;
    }
    state.row.set(row);
    state.say(Utterance::done(format!(
        "{} declared by {}",
        spec::ROWS[row].pattern,
        spec::ROWS[row].by
    )));
}

/// The reference's action out of the section, taken.
///
/// It refuses, and the refusal reaches the person rather than being a disabled
/// bit: the reason and what to do about it are both derived from the rail
/// specification's own standing for that seat.
fn show_declarer(state: &Rc<ViewState>) {
    let why = declarer_standing();
    let recourse = match why.kind().recourse() {
        Recourse::AwaitRelease => "it opens with the release it is booked under",
        Recourse::Nothing => "nothing here opens it",
        _ => "it is not this section's to open",
    };
    state.say(Utterance::new(
        Tone::Refused,
        format!(
            "{} is {} ({}) — {recourse}",
            spec::DECLARER_SECTION,
            why.kind().name(),
            why.detail(),
        ),
    ));
}

fn set_query(state: &Rc<ViewState>, text: &str) {
    state.query.set_text(text.to_owned());
    state.query.set_caret(text.len());
}

fn announce_query(state: &Rc<ViewState>) {
    match state.query_fault() {
        Some(why) => state.say(Utterance::new(Tone::Refused, why)),
        None => state.say(Utterance::done(state.summary())),
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
        Hit::Declaration(n) => {
            select_declaration(state, *n);
            true
        }
        Hit::Declarer => {
            show_declarer(state);
            true
        }
        Hit::None => false,
    }
}

/// Step the list's cursor by `delta` visible rows.
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
    select_declaration(state, kept[next]);
    true
}

/// ★★★★★ R1730 — a key belongs to what has focus, and this screen answers only
/// for its own stops.
///
/// **Measured by mounting it.** The first draft matched on the chord alone, so
/// the list's arrows were consumed whatever had focus — and the moment the
/// shell mounted this section at its rail seat, walking the rail with the arrow
/// keys stopped one seat short, because the page took the press the rail was
/// aimed at. A page that eats its host's navigation keys is not a page.
///
/// `None` is this screen's own stops driven with nothing focused, which is what
/// the wire's `key` action and the model tests do.
fn key_at(state: &Rc<ViewState>, focused: Option<&str>, chord: &str) -> bool {
    match focused {
        Some(DETAIL_TAG) => {
            return match chord {
                "Enter" | "Space" => {
                    show_declarer(state);
                    true
                }
                _ => false,
            };
        }
        Some(LIST_TAG) | None => {}
        // Somebody else's stop — the host's rail, the filter box, a sibling
        // page. Refused rather than handled, so the press reaches whatever it
        // was aimed at.
        Some(_) => return false,
    }
    let kept = state.kept();
    match chord {
        "ArrowDown" => step(state, 1),
        "ArrowUp" => step(state, -1),
        "Home" => kept.first().is_some_and(|&n| {
            let moved = n != state.cursor_row();
            select_declaration(state, n);
            moved
        }),
        "End" => kept.last().is_some_and(|&n| {
            let moved = n != state.cursor_row();
            select_declaration(state, n);
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

/// ★★★★★ R1730 — one part of a specified surface: the tag the specification
/// compares, on a box that **holds that part's marks**.
///
/// Its children are in the part's own coordinates, which is what makes the tag
/// honest — the rectangle the painted roster reads back is the extent of the
/// thing, not a marker floating beside it.
///
/// The first draft did float a marker beside it: an empty tagged box per part,
/// with a `layout` silence. The framework's own structure gate reported eleven
/// of them as **hollow** — a box declared to hold a layout and holding nothing —
/// and it was right twice over. A reader landing there would have heard an
/// empty region, and the roster would have gone on reading correctly while the
/// part it named stopped being painted.
fn part_box(tag: &str, rect: Rect, children: Vec<Scene>) -> Scene {
    part_box_styled(tag, rect, None, children)
}

/// A part that draws a face of its own — a card, a button.
///
/// The face is this node's *style* rather than a child box, and that is not
/// tidiness. A part declared unavailable cascades onto everything inside it, so
/// a tagged face inside a refusing part is a second region carrying the reason
/// and announcing nothing — which the framework's stated-reason gate reports as
/// *a sentence written for somebody who cannot receive it*. One node per part
/// cannot have that shape.
fn part_box_styled(tag: &str, rect: Rect, face: Option<BoxStyle>, children: Vec<Scene>) -> Scene {
    let mut node = ContainerNode::new(children)
        .with_tag(tag.to_owned())
        .with_layout(absolute(rect));
    if let Some(style) = face {
        node = node.with_style(style);
    }
    Scene::Container(node)
}

/// The card a single fact and the action are drawn on.
fn card_face(ink: Ink, radius: u32) -> BoxStyle {
    BoxStyle::filled(ink.surface_2)
        .with_corner_radius(radius)
        .with_border(Border::new(ink.outline, PANEL_FRAME))
}

/// The same box for a part that cannot be used, carrying why.
///
/// The reason rides on **this** tag because this is the tag the accessibility
/// tree announces. Declared one node down — on the button's face — it never
/// reaches a reader, and the framework's stated-reason gate says so: *a reason
/// on `scene/disabled` that never reaches `scene/access` is a sentence written
/// for somebody who cannot receive it*.
fn part_box_unavailable(
    tag: &str,
    rect: Rect,
    why: Unavailable,
    face: BoxStyle,
    children: Vec<Scene>,
) -> Scene {
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(tag.to_owned())
            .with_style(face)
            .with_layout(absolute(rect).with_unavailable(why)),
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
                "places the header, the column row, the list and the record pane",
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
            // ★ It IS the header's name, so it is silent and says whose name
            // it is. Announced on its own it would tell a reader the section is
            // called Key Patterns twice.
            "title" => children.push(
                tagged_label(&tag, spec::HEADER[0].title, at, FONT_TITLE, ink.text)
                    .silenced(Silence::name_of(HEADER_TAG)),
            ),
            "summary" => children.push(tagged_label(
                &tag,
                state.summary(),
                at,
                FONT_SMALL,
                state.query_fault().map_or(ink.text_3, |_| ink.warn),
            )),
            // ★★★★★ R1730 — the field is painted inside a viewport of its own
            // size, and that is a MEASURED repair rather than decoration.
            //
            // A single-line field does not clip: the framework's own painter
            // wraps its content in a `Scene::Scroll` for a multi-line field and
            // keeps a flat child list for a single-line one, so text wider than
            // the box is painted straight over whatever is beside it. This
            // screen's own containment gate caught it on its first run — a
            // query of ordinary length overhung the box by 128 pixels, into the
            // list's column header.
            //
            // Clipped here because the fix belongs to the framework and reaches
            // every field in the tree, which is a blast radius of its own round:
            // see the debt note this round opened. What a viewport does NOT
            // give is the caret staying visible while a person types past the
            // edge, and that is the same missing axis.
            _ => children.push(
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
        }
    }
    panel(HEADER_TAG, rect, ink.surface, Some(ink.outline), children)
}

fn column_header(ink: Ink) -> Scene {
    let rect = colhead_rect();
    let mut children = Vec::new();
    for (n, column) in spec::COLUMNS.iter().enumerate() {
        let at = column_rect(n);
        // ★ ONE tag per column, on the header a reader actually sees. The first
        // draft had two — an empty anchor for the roster beside a label for the
        // reader — and the two could have drifted apart without anything
        // noticing, which is the shape this project keeps paying for.
        children.push(tagged_label(
            &format!("kp.column.{}", column.key),
            column.title,
            Rect::new(at.x, 10, at.w, 12),
            10,
            ink.text_3,
        ));
    }
    panel("kp.colhead", rect, ink.bg, Some(ink.outline), children).silenced(Silence::layout(
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
                "scrolls the declarations; the rows inside it are what a reader lands on",
            )),
        ],
    )
    .with_focusable(true)
}

/// One declaration row, in the list pane's own coordinates.
///
/// `n` is the row's index in [`spec::ROWS`] — its identity, which is what every
/// tag carries. `visual` is where it sits right now, which the query decides.
fn list_row_paint(n: usize, visual: usize, open: usize, ink: Ink) -> Vec<Scene> {
    let row = &spec::ROWS[n];
    let at = list_row_rect(visual);
    let mut children = Vec::with_capacity(spec::COLUMNS.len() + 3);
    if n == open {
        children.push(
            box_at("kp.list.open", at, ink.accent_soft, Some(ink.accent), 0).silenced(
                Silence::decorative("the band behind the open declaration; the row says so"),
            ),
        );
    }
    children.push(box_at(
        &format!("kp.list.row.{n}"),
        at,
        Color::rgba(0, 0, 0, 0),
        None,
        0,
    ));
    for (c, column) in spec::COLUMNS.iter().enumerate() {
        let col = column_rect(c);
        let cell = Rect::new(col.x, at.y + 13, col.w, 14);
        let fg = match column.key {
            "id" => ink.accent,
            "by" | "direction" => ink.text_2,
            "status" => health_ink(row.health, ink),
            _ => ink.text,
        };
        if column.key == "status" {
            children.push(
                box_at(
                    &format!("kp.list.dot.{n}"),
                    Rect::new(cell.x, cell.y + 4, 7, 7),
                    fg,
                    None,
                    4,
                )
                .silenced(Silence::decorative("repeats the status the cell reads")),
            );
        }
        let text_x = if column.key == "status" {
            cell.x + 13
        } else {
            cell.x
        };
        children.push(tagged_label(
            &format!("kp.list.cell.{n}_{}", column.key),
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
    for (key, at) in detail_parts() {
        let tag = format!("{DETAIL_TAG}.{key}");
        let marks = detail_part_paint(key, at, record, ink);
        children.push(match key {
            // ★ The pane's heading IS the pane's name. Announced on its own a
            // reader would be told what the pane is called twice.
            "subject" => part_box(&tag, at, marks).silenced(Silence::name_of(DETAIL_TAG)),
            "declarer" => {
                part_box_unavailable(&tag, at, declarer_standing(), card_face(ink, 8), marks)
            }
            // The four single facts the reference draws as a two-by-two grid,
            // each on a card of its own.
            "declared_by" | "direction" | "matches" | "rate" => {
                part_box_styled(&tag, at, Some(card_face(ink, 9)), marks)
            }
            _ => part_box(&tag, at, marks),
        });
    }
    // The refusal's own sentence, under the action rather than inside it: it is
    // about the part and not a second name for it.
    if let Some(at) = detail_part_rect("declarer") {
        let why = declarer_standing();
        children.push(label(
            format!("{} · {}", why.kind().name(), why.detail()),
            Rect::new(at.x, at.y + at.h + 4, at.w, 12),
            10,
            ink.text_3,
        ));
    }
    panel(DETAIL_TAG, rect, ink.surface, Some(ink.outline), children).with_focusable(true)
}

/// What one part of the record pane draws, **in the part's own coordinates**.
///
/// Split from [`detail_pane`] so each arm is readable, and keyed by the same
/// string [`spec::DETAIL`] declares — a part added to the table with no arm here
/// draws an empty box, which the framework's structure gate reports as hollow
/// rather than letting the roster read correctly over nothing.
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
        "ordinal" => vec![label(
            format!("#{}", record.id),
            whole,
            FONT_SMALL,
            ink.text_2,
        )],
        "pattern" => vec![label(record.pattern, whole, FONT_SUBJECT, ink.text)],
        "standing" => standing_pills(whole, record, ink),
        "declared_by" => fact_box(whole, title_of(key), record.by, ink),
        "direction" => fact_box(whole, title_of(key), record.direction, ink),
        "matches" => fact_box(whole, title_of(key), &record.matches.to_string(), ink),
        "rate" => fact_box(whole, title_of(key), record.rate, ink),
        "endpoints" => endpoint_chips(whole, record, ink),
        "first_seen" => vec![label(
            format!("first seen · {}", record.first_seen),
            whole,
            FONT_SMALL,
            ink.text_3,
        )],
        "declarer" => vec![label(
            title_of("declarer"),
            Rect::new(12, 10, whole.w.saturating_sub(24), 14),
            FONT_BODY,
            ink.text_3,
        )],
        _ => Vec::new(),
    }
}

fn standing_pills(at: Rect, record: &'static spec::RowSpec, ink: Ink) -> Vec<Scene> {
    let kind = Rect::new(0, 0, 96, at.h);
    let health = Rect::new(104, 0, 116, at.h);
    vec![
        box_at(
            "kp.detail.standing.kind",
            kind,
            Color::rgba(0xC7, 0x78, 0x00, 0x29),
            None,
            6,
        )
        .silenced(Silence::decorative("the tone behind the declaration tag")),
        label(
            "Declaration",
            Rect::new(kind.x + 9, kind.y + 5, kind.w - 18, 13),
            FONT_SMALL,
            ink.declaration,
        ),
        box_at(
            "kp.detail.standing.health",
            health,
            Color::rgba(0x35, 0xC0, 0x8B, 0x29),
            None,
            11,
        )
        .silenced(Silence::decorative("the tone behind the resolution pill")),
        label(
            record.health.label(),
            Rect::new(health.x + 10, health.y + 5, health.w - 20, 13),
            FONT_SMALL,
            health_ink(record.health, ink),
        ),
    ]
}

fn fact_box(at: Rect, title: &str, value: &str, ink: Ink) -> Vec<Scene> {
    vec![
        label(
            title.to_owned(),
            Rect::new(11, 9, at.w.saturating_sub(22), 12),
            10,
            ink.text_3,
        ),
        label(
            value.to_owned(),
            Rect::new(11, 25, at.w.saturating_sub(22), 16),
            FONT_BODY,
            ink.text,
        ),
    ]
}

fn endpoint_chips(at: Rect, record: &'static spec::RowSpec, ink: Ink) -> Vec<Scene> {
    let mut out = vec![label(
        "MATCHED ENDPOINTS",
        Rect::new(0, 0, at.w, 12),
        10,
        ink.text_3,
    )];
    let mut x = 0;
    for (n, endpoint) in record.endpoints.iter().enumerate() {
        let chip = Rect::new(x, 18, 62, 26);
        out.push(
            box_at(
                &format!("kp.detail.endpoint.{n}"),
                chip,
                ink.surface_2,
                Some(ink.outline),
                7,
            )
            .silenced(Silence::decorative("the card behind one matched endpoint")),
        );
        out.push(label(
            (*endpoint).to_owned(),
            Rect::new(chip.x + 10, chip.y + 7, chip.w - 20, 13),
            FONT_SMALL,
            ink.text,
        ));
        x += chip.w + 6;
    }
    out
}

// ── The three surfaces, as this screen builds them ──────────────────────────

/// One surface's parts, as the running screen's own tables declare them.
///
/// What the wire publishes and what the specification is compared against. The
/// painted scene is compared with **this** by `painted.rs`, so the chain runs
/// specification → tables → paint with both links checked, rather than a
/// specification checked against a copy of itself.
///
/// # Panics
///
/// If asked for a surface `docs/analyzer-keys-spec.json` does not fix, which is
/// a defect in this file.
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

/// How much of the specified section this build reproduces, and where it does
/// not.
///
/// Not a test fixture: this is the sentence an agent driving the tool needs
/// before it plans anything.
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
        "gestures": spec::GESTURES
            .iter()
            .map(|(g, does)| serde_json::json!({ "gesture": g, "does": does }))
            .collect::<Vec<_>>(),
        "rows": spec::ROWS
            .iter()
            .map(|r| serde_json::json!({
                "id": r.id,
                "pattern": r.pattern,
                "by": r.by,
                "direction": r.direction,
                "matches": r.matches,
                "rate": r.rate,
                "health": r.health.name(),
                "first_seen": r.first_seen,
                "endpoints": r.endpoints,
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

    fn index(args: &IntrospectValue) -> Result<usize, InvokeError> {
        args.as_i64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| InvokeError::rejected("expected a row index"))
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
                    // ★★★★★ R1730 — the reading this round exists for.
                    SchemaField::new("conformance", "json"),
                    SchemaField::new("row_count", "int"),
                    SchemaField::new("selected_row", "int"),
                    SchemaField::new("record", "json"),
                    SchemaField::new("summary", "string"),
                    SchemaField::new("kept_rows", "json"),
                    SchemaField::new("query", "string"),
                    SchemaField::new("query_fault", "string"),
                    SchemaField::new("why_hidden", "json"),
                    SchemaField::new("declarer", "json"),
                    SchemaField::new("said", "object"),
                    SchemaField::parametric(
                        "hit.<x>.<y>",
                        "string",
                        const { &[SchemaArg::open("x", "int"), SchemaArg::open("y", "int")] },
                    ),
                    SchemaField::action("select_declaration", "int"),
                    SchemaField::action("show_declarer", "string"),
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
            .ok_or_else(|| ReadRefusal::unavailable("no session is loaded"))?;
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
                    "id": record.id,
                    "pattern": record.pattern,
                    "by": record.by,
                    "direction": record.direction,
                    "matches": record.matches,
                    "rate": record.rate,
                    "health": record.health.name(),
                    "first_seen": record.first_seen,
                    "endpoints": record.endpoints,
                })))
            }
            "summary" => Ok(IntrospectValue::Text(state.summary())),
            "kept_rows" => Ok(IntrospectValue::Json(serde_json::json!(state.kept()))),
            "query" => Ok(IntrospectValue::Text(state.query.text())),
            "query_fault" => Ok(IntrospectValue::Text(
                state.query_fault().unwrap_or_default(),
            )),
            "why_hidden" => {
                let kept = state.kept();
                let query = state.query();
                Ok(IntrospectValue::Json(serde_json::json!(
                    (0..spec::ROWS.len())
                        .filter(|n| !kept.contains(n))
                        .map(|n| {
                            let cells = spec::ROWS[n].attributes();
                            serde_json::json!({
                                "row": n,
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
            "declarer" => {
                let why = declarer_standing();
                Ok(IntrospectValue::Json(serde_json::json!({
                    "section": spec::DECLARER_SECTION,
                    "kind": why.kind().name(),
                    "detail": why.detail(),
                    "recourse": why.kind().recourse().name(),
                })))
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
            "select_declaration" => {
                let n = Self::index(&args)?;
                if n >= spec::ROWS.len() {
                    return Err(InvokeError::rejected(format!("no declaration {n}")));
                }
                select_declaration(&state, n);
                Ok(IntrospectValue::Int(i64::try_from(n).unwrap_or(i64::MAX)))
            }
            "show_declarer" => {
                show_declarer(&state);
                Ok(IntrospectValue::Text(state.said_sentence()))
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

/// ★ R1730 — public from the first round, because this screen is both a window
/// of its own and a **page** of the analysis-tool shell
/// (`pinion_screen::Mount<KeyPatternView>`).
pub struct KeyPatternView;

impl WidgetCore for KeyPatternView {
    /// The filter box's posture and caret, which the shell reads out of the
    /// painted scene and hands back to the view.
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
        "pinion hello-key-patterns (R1730 §5.41 key-pattern section)"
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        chord: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        // While the filter has focus every key is the box's, through the
        // framework's own keymap rather than another copy of one.
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

impl WidgetA11y for KeyPatternView {
    fn access_node(_state: &(TextFieldState, u32), focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_view_state();
        let mut nodes = vec![
            AccessNode::new(ROOT_TAG, AriaRole::Group)
                .with_name("Key patterns")
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
            AccessFocus::composite(LIST_TAG, format!("kp.list.row.{}", state.cursor_row()))
        })
    }
}

fn header_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    vec![
        AccessNode::new(HEADER_TAG, AriaRole::Group)
            .with_name(spec::HEADER[0].title)
            .with_child("kp.header.summary")
            .with_child(QUERY_TAG),
        AccessNode::new("kp.header.summary", AriaRole::Status)
            .with_name("Summary")
            .with_value(AccessValue::Text(state.summary()))
            .with_live(AccessLive::Polite),
        AccessNode::new(QUERY_TAG, AriaRole::TextInput)
            .with_name("Filter")
            .with_value(AccessValue::Text(state.query.text())),
    ]
}

fn list_nodes(state: &Rc<ViewState>, focused: Option<&str>) -> Vec<AccessNode> {
    let open = state.cursor_row();
    let columns: Vec<GridColumn> = spec::COLUMNS
        .iter()
        .map(|column| GridColumn {
            tag: format!("kp.column.{}", column.key),
            sort: None,
        })
        .collect();
    let rows: Vec<GridRow> = state
        .kept()
        .into_iter()
        .map(|n| GridRow {
            tag: format!("kp.list.row.{n}"),
            selected: n == open,
            state: RadioState::Idle,
            cells: spec::COLUMNS
                .iter()
                .map(|column| GridCell {
                    tag: format!("kp.list.cell.{n}_{}", column.key),
                    name: format!("{}: {}", column.title, spec::ROWS[n].cell(column.key)),
                    focused: focused == Some(LIST_TAG) && n == open,
                    selected: None,
                })
                .collect(),
        })
        .collect();
    grid_table_nodes(
        LIST_TAG,
        "Declarations",
        false,
        LIST_HEADER,
        &columns,
        &rows,
    )
}

fn detail_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    let record = state.record();
    let why = declarer_standing();
    // ★ Every part but the first. The pane's heading IS the pane's name (the
    // paint says so with `Silence::name_of`), so announcing it again would tell
    // a reader what the pane is called twice; every other part, the declaration
    // number included, is a fact a reader wants.
    let mut pane = AccessNode::new(DETAIL_TAG, AriaRole::Group).with_name(spec::DETAIL[0].title);
    for part in &spec::DETAIL[1..] {
        pane = pane.with_child(format!("kp.detail.{}", part.key));
    }
    let mut nodes = vec![pane];
    for part in &spec::DETAIL[1..] {
        let tag = format!("kp.detail.{}", part.key);
        let node = match part.key {
            "declarer" => {
                // The reason travels as a value rather than as a disabled bit.
                // The paint declares it too (`LayoutStyle::with_unavailable` on
                // the button's face), and this is the explicit-node half of the
                // same fact for a screen that publishes its own tree.
                let mut node = AccessNode::new(tag, AriaRole::Button).with_name(part.title);
                node.unavailable = Some(why.clone());
                node
            }
            "endpoints" => AccessNode::new(tag, AriaRole::Group)
                .with_name(part.title)
                .with_value(AccessValue::Text(record.endpoints.join(", "))),
            key => AccessNode::new(tag, AriaRole::Group)
                .with_name(part.title)
                .with_value(AccessValue::Text(detail_reading(key, record))),
        };
        nodes.push(node);
    }
    nodes
}

/// What one part of the record pane reads as.
fn detail_reading(key: &str, record: &'static spec::RowSpec) -> String {
    match key {
        "ordinal" => format!("#{}", record.id),
        "pattern" => record.pattern.to_owned(),
        "standing" => format!("Declaration, {}", record.health.label()),
        "declared_by" => record.by.to_owned(),
        "direction" => record.direction.to_owned(),
        "matches" => record.matches.to_string(),
        "rate" => record.rate.to_owned(),
        "first_seen" => record.first_seen.to_owned(),
        _ => String::new(),
    }
}

impl WidgetView for KeyPatternView {
    type Renderer = HelloKeyPatternsRenderer;

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

/// Which byte of the filter's text a window point lands on, when the filter is
/// what has focus and the point is inside it.
///
/// Resolved against [`filter_field_style`], which is also what paints the box —
/// two styles here would put the caret on a different letter from the one under
/// the cursor.
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
    // Compared in the pointer's own units rather than by casting it to the
    // rectangle's: a cast would round a point just outside the left edge INTO
    // the box, and a press half a pixel above it out of one it is in.
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

/// Run the key-pattern section as an application of its own.
pub fn run() {
    pinion_shell::run::<KeyPatternView>();
}
