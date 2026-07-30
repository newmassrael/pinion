// R1450 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the architectural narrative carries many proper-noun
// identifiers (QHeaderView, ReorderModel, WAI-ARIA, …).
#![allow(clippy::doc_markdown)]

//! `hello-column-reorder` — R1450 / R1451 §5.27 §5.40 §5.51 **a column carries
//! its width and its place wherever its header is dragged**: Qt's
//! `QHeaderView`, whole.
//!
//! ## The gap this closes
//!
//! R1450 added the axis Qt had and pinion did not — **section order**
//! (`setSectionsMovable` / `moveSection` / `visualIndex` <-> `logicalIndex`) —
//! as the 4th consumer of the lifted
//! [`ReorderModel`](pinion_core::widgets::reorder::ReorderModel) (R743). R1451
//! finishes the widget: Qt keys `sectionSize` / `isSectionHidden` by the **logical**
//! section, so a resized or hidden column stays resized and hidden wherever it
//! is dragged. pinion's width model (R785) indexed by *screen position*, and
//! its visibility model (R990) lived in another binding entirely, so that
//! composition had no home and moving a column left its width behind.
//!
//! The home is [`ColumnLayout`], and this binding is its first consumer: the
//! permutation, the sizes, the hidden flags, the derived geometry, and the
//! `saveState` / `restoreState` round-trip all come from one model. What is
//! left here is the column *names* and the keyboard *policy* — everything a
//! `QHeaderView` does is the substrate's.
//!
//! ## Why the header strip is its own external (and matches the reference)
//!
//! In Qt a `QHeaderView` is a **separate widget** the view owns, not a band
//! inside it — so modelling the header as its own external is the faithful
//! shape, not a shortcut. It is also the shape the substrate needs:
//! [`ReorderModel`](pinion_core::widgets::reorder::ReorderModel)'s drop
//! classification reads the composite `#<visual>`
//! subindex off the hovered tag, and the eager table's header cells are tagged
//! `{tag}_ch{col}` (no subindex) because their click routes to the table's own
//! sort. A strip whose cells ARE `colhdr#<visual>` gives the drag session real
//! per-section hit nodes, and the body below simply paints through the order.
//!
//! R1491 — those two tag shapes read as an either/or until this round, and one
//! pinion header was accordingly either movable or sortable, never both, while
//! Qt's is both at once (`setSectionsMovable` + `setSectionsClickable`). They
//! are not alternatives: the subindex a drop classifier needs is also enough to
//! name the section a *click* landed on, and what separates a click from a drag
//! is not the tag but the release. So this strip now carries the sort indicator
//! too (`sortIndicatorSection` / `sortIndicatorOrder` / `sortIndicatorShown`),
//! keyed logically like the sizes and the hidden flags, and the arrow travels
//! with its column instead of staying on a position.
//!
//! R1496 — and the release that separates them is the router's, not this
//! widget's. R1491 read the click off the drop commit ("the permutation came
//! out unchanged"); R794 §5.51 already owns click-vs-drag and withholds the
//! trailing `PointerUp` after a gesture that travelled, so the click arrives
//! there ([`ColumnLayout::handle_send`]) and [`ColumnLayout::end_section_drag`]
//! only commits the drop. That is also what lets Qt's two permissions be
//! independent here: a header that opens no drag session
//! ([`ColumnLayout::sections_movable`] off) is still sortable, which it could
//! not be while its click was a by-product of a drag.
//!
//! R1497 — and the per-section *label* inside each cell is tagged too
//! (`colhdr_label#<visual>`), for the snapshot assertions and the a11y walk. That
//! made this binding the forcing consumer for a framework rule: a tag is a name,
//! not an event target, so a press resolved onto one with no `External` behind it
//! used to be discarded in silence. Since the label is centred, it covered the
//! rect centre `scene/click {path}` presses on two of the five sections, and
//! exactly those two could not be clicked. The router now resolves the deepest
//! node that can RECEIVE the event, so a widget may tag its own decoration
//! without stealing its own input — which is why the eager table can keep its
//! presentational `{tag}_ch{col}` on the OUTER container instead of having to
//! bury a dispatchable tag deeper than its own name.
//!
//! ## One projection, or the failure mode is unpaintable
//!
//! `order[visual] = logical` plus the logical-keyed sizes and hidden flags is
//! the whole model, and
//! [`ColumnLayout::visible_placements`] is the *only* walk that turns it into
//! geometry. The header cells, the body cells, the drag insertion line, and
//! the a11y column headers all read that one vector, so a section cannot move
//! its label without moving its data, and a body cell cannot land under a
//! different column than its own header — the failure modes a second
//! projection or a second width sum would invite.
//!
//! The sections are deliberately **non-uniform** in width, because on a
//! uniform strip "the width travelled with the column" and "the widths never
//! moved at all" paint identical pixels.
//!
//! ## AI clients (§2 #7 + §2 #2 — where Qt cannot follow)
//!
//! Qt persists a header layout as `QHeaderView::saveState()`, an **opaque
//! versioned `QByteArray`**: an agent cannot read "which column is third now,
//! and how wide is it" out of it, and cannot write one either without a live
//! `QHeaderView`. Here the entire state is typed data both ways —
//! `query("state")` / `"order"` / `"sizes"` / `"hidden"` / `"placements"` /
//! `"visual_index.<logical>"` / `"section_position.<logical>"` /
//! `"logical_index_at.<x>"` read it; `invoke("move_section" |
//! "swap_sections" | "resize_section" | "set_section_hidden", "<a>:<b>")`
//! performs Qt's own section calls; and `intervene("state", {..})` is
//! `restoreState` with every field legible. The keyboard reaches exactly the
//! same model (`[` / `]` resize, `h` hides, arrows and Space/Escape drag), so
//! neither door can do something the other cannot.

use pinion_a11y::{AccessNode, AccessState, AriaRole, SortDirection, WidgetA11y};
use pinion_core::command::Command;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, DragPayload, DropPoint, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg,
    SchemaField, ThreadOwnership,
};
use pinion_core::reactive::measured_text_extent;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::column_layout::{
    ColumnLayout, ColumnLayoutView, DEFAULT_SECTION_SIZE, SectionPlacement, SectionResizeMode,
    read_column_layout, use_column_layout_with,
};
use pinion_core::widgets::column_widths::{DEFAULT_MAX_COL_WIDTH, DEFAULT_MIN_COL_WIDTH};
use pinion_core::widgets::grid_sort::col_sort_dir;
use pinion_core::widgets::table::{cell_cmp, grid_order_by};
use pinion_core::{Frame, Intent, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::glyph::sort_glyph;
use std::borrow::Cow;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloColumnReorderRenderer, HelloColumnReorderRendererError);

const WIN_W: u32 = 700;
const WIN_H: u32 = 420;
const THEME_TAG: &str = "app";

/// The header strip (primary external) — the `QHeaderView`. Section cells paint
/// as `colhdr#<visual>`, which is what gives the drag session per-section hit
/// nodes.
const HDR_TAG: &str = "colhdr";
/// The body container tag; data cells paint as `colbody#<row>_<visual>`.
const BODY_TAG: &str = "colbody";
/// Scene-as-data readout of the current section order.
const ORDER_TAG: &str = "colreorder_order";
/// R1451 — scene-as-data readout of the rest of the header state: the
/// logical-keyed sizes and the hidden sections.
const LAYOUT_TAG: &str = "colreorder_layout";
/// The `DragPayload::kind` discriminator for a section drag.
const DRAG_KIND: &str = "column-section";

/// Logical columns — the source schema. `order[visual] = logical` indexes this.
const HEADERS: [&str; 5] = ["Name", "Type", "Size", "Modified", "Owner"];
const NCOLS: usize = HEADERS.len();
const NROWS: usize = 6;
/// The un-reordered section order, and the fallback when a read comes back
/// malformed.
const IDENTITY_ORDER: [usize; NCOLS] = [0, 1, 2, 3, 4];

const NAMES: [&str; NROWS] = [
    "report.pdf",
    "photo.png",
    "notes.txt",
    "build.rs",
    "data.csv",
    "movie.mp4",
];
const TYPES: [&str; NROWS] = ["PDF", "Image", "Text", "Rust", "CSV", "Video"];
const SIZES: [&str; NROWS] = ["2.1 MB", "880 KB", "4 KB", "1 KB", "32 KB", "1.4 GB"];
const MODIFIED: [&str; NROWS] = [
    "2026-06-01",
    "2026-05-30",
    "2026-06-10",
    "2026-06-18",
    "2026-04-22",
    "2026-03-09",
];
const OWNERS: [&str; NROWS] = ["coin", "coin", "alex", "coin", "alex", "guest"];

/// Cell text for **logical** `(row, col)` — the source model, which a reorder
/// never touches.
fn cell_text(row: usize, logical: usize) -> &'static str {
    [NAMES, TYPES, SIZES, MODIFIED, OWNERS][logical][row]
}

// Geometry. R1451 — the sections are deliberately **non-uniform**: on a
// uniform strip "the width travelled with the column" and "the widths never
// moved" paint the same pixels, so a uniform demo could not tell the two
// apart. Widths are keyed by LOGICAL section, so this array is indexed by
// `HEADERS`, not by screen position.
const SECTION_W: [u32; NCOLS] = [150, 90, 100, 130, 100];
const GRID_X: u32 = 30;
const GRID_Y: u32 = 90;
const HDR_H: u32 = 40;
const ROW_H: u32 = 34;
/// Resize step for the `[` / `]` keyboard gesture.
const RESIZE_STEP: u32 = 20;
/// R1452 — cache key for the shared [`ColumnLayout`]: the External mutates it,
/// the view fn publishes the two inputs only the view knows.
const LAYOUT_KEY: &str = "colreorder.layout";
/// R1452 — one text size for both the header and the body, so a single
/// measured monospace cell answers for every cell in the grid.
const TEXT_PX: u32 = 13;
/// Horizontal padding inside a section, both sides — what a content-fitted
/// column needs on top of its text.
const CELL_PAD: u32 = 12;
/// The width a `Stretch` row divides: the grid's span inside the window.
const AVAILABLE_W: u32 = WIN_W - 2 * GRID_X;

/// The face the header and the body both paint in.
///
/// R1453 — the grid's own face again. R1452 forced it to **monospace** because
/// the only measurement a view fn could make was a monospace cell; with
/// [`measured_text_extent`] the hint below is measured in whatever face the
/// cells actually use, so the constraint is gone.
fn grid_text(role: ColorRole, theme: &Theme) -> TextStyle {
    TextStyle::new()
        .with_size_px(TEXT_PX)
        .with_fg(theme.resolve(role))
}

/// The strings logical column `logical` is measured against: its header label
/// and the first `rows` cells.
///
/// R1454 — the header is always measured (Qt measures it too) and the body is
/// **sampled**, because measuring every row of a grid each frame is what makes
/// a content-fitted column expensive: a shape miss costs 18.5 us against a
/// 118 ns cache hit, and a working set past the measurement cache's 256 slots
/// re-shapes in full every frame.
fn column_strings(logical: usize, rows: usize) -> impl Iterator<Item = &'static str> {
    std::iter::once(HEADERS[logical])
        .chain((0..NROWS.min(rows)).map(move |r| cell_text(r, logical)))
}

/// R1452 — the per-logical-section content size hints: the peer of Qt's
/// delegate `sizeHint`, which is where Qt gets them too (`QHeaderView` does not
/// measure either).
///
/// R1453 — **exact**, and for any face: each string is measured in the very
/// [`TextStyle`] the cell paints with, and the column takes the widest.
///
/// R1452 could only multiply a character count by a monospace cell, so the
/// grid had to be monospace and the hint ran up to a pixel per character wide
/// (`CellMetric` is a whole number, hence the ceiling of the real advance).
/// Measuring the strings themselves retires both limits — and in a
/// proportional face the widest column is not always the one with the most
/// characters, which a character count could never notice.
///
/// R1454 — the body is sampled at the layout's
/// [`resize_contents_precision`](ColumnLayout::resize_contents_precision)
/// (Qt's `QHeaderView::resizeContentsPrecision`), so the per-frame working set
/// is bounded no matter how many rows the grid has.
///
/// All-or-nothing: if any string cannot be measured (headless, no provider)
/// the whole hint set is `None` and the caller publishes nothing rather than
/// mixing a real width with a made-up one.
fn content_hints(theme: &Theme, rows: usize) -> Option<Vec<u32>> {
    let style = grid_text(ColorRole::OnSurface, theme);
    (0..NCOLS)
        .map(|l| {
            column_strings(l, rows)
                .try_fold(0, |widest, s| {
                    Some(widest.max(measured_text_extent(s, &style, None)?.width()))
                })
                .map(|w| w + 2 * CELL_PAD)
        })
        .collect()
}

/// R1452 — the one-letter code the readout row shows for a section's mode,
/// derived from the wire spelling rather than tabled again (`interactive` /
/// `fixed` / `stretch` / `resize_to_contents` have four distinct initials, so
/// the derivation is total).
fn mode_code(mode: SectionResizeMode) -> char {
    mode.as_wire().chars().next().unwrap_or('?')
}

/// The section paint / hit tag for visual position `i` (`"colhdr#0"` …).
fn section_tag(visual: usize) -> String {
    format!("{HDR_TAG}#{visual}")
}

/// The header strip external: the `QHeaderView`. R1451 — it owns the whole
/// section layout through the lifted [`ColumnLayout`]: the permutation, the
/// per-section sizes, and the hidden flags, plus the Qt index mapping and the
/// `saveState` / `restoreState` round-trip. It holds no column *data* — the
/// view projects the schema through
/// [`ColumnLayout::visible_placements`].
#[derive(Debug)]
struct ColumnHeaderExternal {
    layout: Rc<ColumnLayout>,
}

impl ColumnHeaderExternal {
    /// R1452 — resolves the SHARED layout rather than owning one, because the
    /// view fn has to publish two inputs only it knows: the measured content
    /// hints and the width a `Stretch` row divides.
    fn new() -> Self {
        Self {
            layout: use_column_layout_with(LAYOUT_KEY, boot_layout),
        }
    }
}

/// The header this app declares, built once — everything a `QHeaderView`
/// consumer sets right after constructing one.
///
/// It is a free function, not two copies inside the two `use_column_layout_with`
/// call sites, because `Owner::cache` builds on whichever pass reaches it
/// first: the `External` factory or the view fn. Configuring it at only one of
/// them would make the header's rules depend on which ran — and R1491 shipped
/// exactly that, with `set_sort_indicator_shown(true)` living in the `External`
/// alone.
fn boot_layout() -> ColumnLayout {
    let layout = ColumnLayout::new(SECTION_W.to_vec());
    // R1491 — `ColumnLayout` boots with the indicator hidden, as Qt's
    // `QHeaderView` does; a *sortable* view turns it on, which is where
    // `QTableView::setSortingEnabled` puts the same decision. Leaving it to
    // the header's default would paint no arrow however the grid sorted.
    layout.set_sort_indicator_shown(true);
    // R1496 — and the same `setSortingEnabled` is what turns Qt's
    // `sectionsClickable` on, because a click that sorts nothing is not a
    // click. `sectionsMovable` is this app's own affordance: the caption says
    // "drag a header to move it", so it says so to the header too. Both default
    // to `false`, as in Qt, which is why an app that wants them declares them.
    layout.set_sections_clickable(true);
    layout.set_sections_movable(true);
    layout
}

impl External for ColumnHeaderExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }
    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }
    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }
    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }
    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    /// The three R742 drag hooks delegate straight to the model — a section
    /// drag is mechanically the tab-strip drag with a different paint.
    ///
    /// R1496 — through [`ColumnLayout::begin_section_drag`] rather than the
    /// reorder model's own arm, so a header that is not movable opens no
    /// session at all: nothing previews a drop it would refuse to commit.
    fn begin_drag(&self) -> Option<DragPayload> {
        self.layout.begin_section_drag(Cow::Borrowed(DRAG_KIND))
    }

    fn drag_to(&mut self, payload: &DragPayload, over: Option<DropPoint>) {
        self.layout.sections().drag_to(payload, over.as_ref());
    }

    /// R1496 — the release commits the drop and nothing else. The click is no
    /// longer derived from it: it arrives as the trailing `PointerUp` the R794
    /// router synthesizes only for a press-release that never became a drag,
    /// which is the framework's own click-vs-drag determination and the one
    /// `file_browser` already takes its row activation from.
    fn drag_release(&mut self, payload: &DragPayload, over: Option<DropPoint>) {
        self.layout.end_section_drag(payload, over.as_ref());
    }
}

impl ExternalIntrospect for ColumnHeaderExternal {
    fn schema(&self) -> IntrospectSchema {
        // Everything below the `labels` / `count` pair is the layout model's —
        // the permutation, the sizes, the hidden flags, the derived geometry,
        // and the whole-state round-trip. This binding contributes the column
        // names and nothing else, which is what the lift bought.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("labels", "json"),
                    SchemaField::new("count", "int"),
                    SchemaField::new("state", "json"),
                    SchemaField::new("order", "json"),
                    SchemaField::new("sizes", "json"),
                    // R1493 — the effective peer of `sizes`. Declared next to
                    // it because the pair is the point: `sizes` is what a
                    // restore replays, this is what the strip paints, and under
                    // `Stretch` / `ResizeToContents` they differ.
                    SchemaField::new("section_sizes", "json"),
                    SchemaField::new("default_section_size", "int"),
                    // R1494 — Qt's `cascadingSectionResizes`, and the resize it
                    // governs. `resize_section` is declared by the layout model
                    // beside it; the two are separate methods on purpose.
                    SchemaField::new("cascading_section_resizes", "boolean"),
                    SchemaField::new("interactive_resize_section", "string"),
                    // R1496 — Qt's two interaction permissions. Declared beside
                    // the resize rule above because they are the same kind of
                    // thing: what the header lets a gesture do.
                    SchemaField::new("sections_movable", "boolean"),
                    SchemaField::new("sections_clickable", "boolean"),
                    SchemaField::new("hidden", "json"),
                    SchemaField::new("placements", "json"),
                    SchemaField::new("visible_sections", "json"),
                    SchemaField::new("visible_widths", "json"),
                    SchemaField::new("visible_total", "int"),
                    SchemaField::new("hidden_count", "int"),
                    SchemaField::new("resize_modes", "json"),
                    SchemaField::new("min_section_size", "int"),
                    SchemaField::new("max_section_size", "int"),
                    SchemaField::new("sort_indicator", "string"),
                    SchemaField::new("sort_indicator_section", "int"),
                    SchemaField::new("sort_indicator_order", "string"),
                    SchemaField::new("sort_indicator_shown", "boolean"),
                    SchemaField::new("content_widths", "json"),
                    SchemaField::new("available_width", "int"),
                    SchemaField::parametric(
                        "resize_mode.<logical>",
                        "string",
                        const { &[SchemaArg::index("logical", "count")] },
                    ),
                    SchemaField::parametric(
                        "content_width.<logical>",
                        "int",
                        const { &[SchemaArg::index("logical", "count")] },
                    ),
                    SchemaField::new("preview", "json"),
                    SchemaField::new("focused_index", "int"),
                    SchemaField::new("grabbed", "boolean"),
                    SchemaField::parametric(
                        "visual_index.<logical>",
                        "int",
                        const { &[SchemaArg::index("logical", "count")] },
                    ),
                    SchemaField::parametric(
                        "logical_index.<visual>",
                        "int",
                        const { &[SchemaArg::index("visual", "count")] },
                    ),
                    SchemaField::parametric(
                        "section_size.<logical>",
                        "int",
                        const { &[SchemaArg::index("logical", "count")] },
                    ),
                    SchemaField::parametric(
                        "section_hidden.<logical>",
                        "boolean",
                        const { &[SchemaArg::index("logical", "count")] },
                    ),
                    SchemaField::parametric(
                        "section_position.<logical>",
                        "int",
                        const { &[SchemaArg::index("logical", "count")] },
                    ),
                    SchemaField::new("logical_index_at.<x>", "int"),
                    SchemaField::new("send", "string"),
                    SchemaField::new("move", "int"),
                    SchemaField::new("move_section", "string"),
                    SchemaField::new("swap_sections", "string"),
                    SchemaField::new("resize_section", "string"),
                    SchemaField::new("set_section_hidden", "string"),
                    SchemaField::new("set_resize_mode", "string"),
                    SchemaField::new("set_all_resize_modes", "string"),
                    SchemaField::new("set_sort_indicator", "string"),
                    SchemaField::new("cycle_sort_indicator", "int"),
                    SchemaField::new("clear_sort_indicator", "string"),
                    SchemaField::new("grab", "boolean"),
                    SchemaField::new("grab_cancel", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            // Header labels of the sections that are actually painted, left to
            // right — the thing a human compares a snapshot against. Read
            // through the layout's own projection so a hidden column drops out
            // of the labels for the same reason it drops out of the paint.
            "labels" => {
                let arr: Vec<serde_json::Value> = self
                    .layout
                    .visible_sections()
                    .iter()
                    .map(|&l| serde_json::Value::from(HEADERS[l]))
                    .collect();
                Some(IntrospectValue::Json(serde_json::Value::Array(arr)))
            }
            "count" => Some(IntrospectValue::Int(
                i64::try_from(NCOLS).unwrap_or(i64::MAX),
            )),
            // Every other slot is the layout's (which itself falls through to
            // the reorder model); it returns None for anything unknown, so the
            // two above win.
            other => self.layout.query(other),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "labels" | "count" => Err(InterveneError::ReadOnly),
            // `state` (Qt restoreState), `sizes`, `hidden`, `order`, and
            // `focused_index` — all typed data rather than an opaque blob.
            other => self.layout.intervene(other, &value),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        // Qt's whole section vocabulary, all of it the model's; this binding
        // adds no action of its own, which is the point of the lift.
        let out = self.layout.invoke(path, &args)?;
        // R1496 — except this one, which is not an action but a *policy*: what
        // a click means. The header reports WHICH section was clicked (`Null`
        // when the release was not one, or when the header is not clickable);
        // deciding that it sorts is the view's call, exactly as Qt leaves it to
        // whoever connects `sectionClicked` / `sortIndicatorChanged`.
        //
        // It reads the value returned by its own delegation rather than a
        // latch, because the router discards the OUTER return on the real
        // pointer path — a click that only an RPC caller could see would not be
        // a click.
        if let IntrospectValue::Int(logical) = out {
            if path == "send" {
                self.layout
                    .cycle_sort_indicator(usize::try_from(logical).unwrap_or(0));
                return Ok(self
                    .layout
                    .query("sort_indicator")
                    .unwrap_or(IntrospectValue::Null));
            }
        }
        Ok(out)
    }
}

/// R1498 — the header-wide sizing rules Qt's `saveState()` carries, grouped
/// because they are one kind of thing: a policy that belongs to the header
/// rather than to a column, that the readout names, and that a keystroke moves.
///
/// Grouped rather than left flat for the reason the R1496 permissions are a
/// pair: the state this view paints from is `Copy` and flat by construction,
/// and a run of loose policy booleans in it stops saying which ones are read
/// together. Qt has more of these (`highlightSelected`, `firstSectionMovable`),
/// so this is where they land.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LayoutRules {
    /// R1494 — Qt's `cascadingSectionResizes`: whether the `[` / `]` gesture
    /// takes its space from the following sections or from the row's width.
    /// Painted, because the same keystroke does two visibly different things
    /// depending on it.
    cascading: bool,
    /// R1498 — Qt's `stretchLastSection`: whether the section painted last
    /// absorbs the leftover viewport width. Painted for the same reason.
    stretch_last: bool,
}

/// The `Copy` posture the view paints from — the whole header layout plus the
/// live drag preview, all read off the primary external.
///
/// R1451 — the layout arrives as one decoded [`ColumnLayoutView`], flattened
/// here into the fixed-`NCOLS` buffers a `WidgetCore::State` must be (it is
/// `Copy`), exactly the conversion the decoder documents for a fixed-`N`
/// binding. `placements[..painted]` is then the single projection the header
/// paint, the body paint, the insertion line, and the a11y tree all read:
/// there is no second copy of the order and no locally re-summed geometry, so
/// a section cannot move its label without moving its data, and a body cell
/// cannot land under a different column than its header.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct HeaderState {
    /// `order[visual] = logical`.
    order: [usize; NCOLS],
    /// Per-**logical**-section size.
    sizes: [u32; NCOLS],
    /// Per-**logical**-section hidden flag.
    hidden: [bool; NCOLS],
    /// R1452 — per-**logical**-section sizing policy.
    modes: [SectionResizeMode; NCOLS],
    /// The painted sections; only `[..painted]` is meaningful.
    placements: [SectionPlacement; NCOLS],
    /// How many sections are painted (`NCOLS` minus the hidden ones).
    painted: usize,
    /// The dragged visual position, while a drag is in flight.
    dragging: Option<usize>,
    /// The insertion gap the drop would target (`0..=NCOLS`).
    insert_at: Option<usize>,
    /// Keyboard cursor / active descendant (a **visual** index in the full
    /// permutation, hidden sections included).
    focused: Option<usize>,
    /// Whether an APG keyboard grab is in flight.
    grabbed: bool,
    /// R1491 — `(logical, ascending)`: which section carries the sort arrow.
    /// **Logical**, so it names a column rather than a place on the strip.
    sort_indicator: Option<(usize, bool)>,
    /// R1491 — whether the arrow is painted at all (Qt `sortIndicatorShown`).
    sort_indicator_shown: bool,
    /// R1492 — the bounds every section size is clamped into. Painted in the
    /// readout because a clamp a reader cannot see the rule for looks like a
    /// bug: without these, "I dragged and it stopped" has no visible cause.
    bounds: (u32, u32),
    /// R1493 — the **effective** size of each logical section: what the strip
    /// actually paints, as distinct from [`sizes`](Self::sizes), which is what
    /// a restore replays.
    ///
    /// The readout row used to print the stored sizes and call them "sizes",
    /// which under `Stretch` or `ResizeToContents` contradicted the strip
    /// directly above it — the readout said `Name=150` over a section painted
    /// 128 wide. Derived from the placements rather than read as its own slot,
    /// because a view that held both could disagree with itself (the reason
    /// `ColumnLayoutView` carries the placements and no width vector).
    section_sizes: [u32; NCOLS],
    /// R1493 — Qt's `defaultSectionSize`: the size a section takes when nothing
    /// else determined it, and the size `reset_default_section_size` returns
    /// every shown section to.
    default_size: u32,
    /// R1498 — the header-wide sizing rules, grouped. See [`LayoutRules`].
    rules: LayoutRules,
    /// R1498 — the mode the layout **applies** to each section, as distinct
    /// from [`modes`](Self::modes), which is what was set on it. The two differ
    /// only under [`LayoutRules::stretch_last`].
    ///
    /// Carried for the reason [`section_sizes`](Self::section_sizes) is: the
    /// readout names the modes, and naming the stored one over a section the
    /// header is stretching is the R1493 defect in the vocabulary next door.
    effective_modes: [SectionResizeMode; NCOLS],
    /// R1496 — Qt's `sectionsMovable` / `sectionsClickable`: whether a drag on
    /// the strip moves anything, and whether a press-release sorts.
    ///
    /// Painted for the R1492 reason the bounds are: a gesture that does nothing
    /// looks like a broken widget unless the rule that refused it is on screen.
    permissions: (bool, bool),
}

impl Default for HeaderState {
    fn default() -> Self {
        Self {
            order: IDENTITY_ORDER,
            sizes: SECTION_W,
            hidden: [false; NCOLS],
            modes: [SectionResizeMode::Interactive; NCOLS],
            placements: [SectionPlacement::default(); NCOLS],
            painted: 0,
            dragging: None,
            insert_at: None,
            focused: None,
            grabbed: false,
            sort_indicator: None,
            // The view enables it, as `QTableView::setSortingEnabled` does —
            // this grid is sortable, so it is on from the first frame.
            sort_indicator_shown: true,
            bounds: (DEFAULT_MIN_COL_WIDTH, DEFAULT_MAX_COL_WIDTH),
            section_sizes: SECTION_W,
            default_size: DEFAULT_SECTION_SIZE,
            rules: LayoutRules::default(),
            effective_modes: [SectionResizeMode::Interactive; NCOLS],
            // This app declares both (`boot_layout`), so the boot posture the
            // first frame paints is the one the header will report.
            permissions: (true, true),
        }
    }
}

impl HeaderState {
    /// The painted sections, in visual order.
    fn placements(&self) -> &[SectionPlacement] {
        &self.placements[..self.painted]
    }

    /// Header labels in the order the strip reads, for the scene-as-data
    /// readout row.
    fn labels(&self) -> Vec<&'static str> {
        self.placements()
            .iter()
            .map(|p| HEADERS[p.logical])
            .collect()
    }

    /// The x offset of insertion gap `gap` (a gap index in the **full** visual
    /// order): the leading edge of the first painted section at or after it,
    /// or the trailing edge of the strip. A lookup into the one geometry walk,
    /// not a second one.
    fn gap_x(&self, gap: usize) -> u32 {
        self.placements()
            .iter()
            .find(|p| p.visual >= gap)
            .map_or_else(|| self.total_w(), |p| p.x)
    }

    /// Total painted width of the strip.
    fn total_w(&self) -> u32 {
        self.placements().last().map_or(0, |p| p.x + p.size)
    }

    /// Flatten a decoded layout into the fixed-`NCOLS` buffers. A short or
    /// malformed read leaves the corresponding field at its boot value rather
    /// than painting a partial grid.
    fn absorb(&mut self, view: &ColumnLayoutView) {
        if let Ok(order) = <[usize; NCOLS]>::try_from(view.state.order.clone()) {
            self.order = order;
        }
        if let Ok(sizes) = <[u32; NCOLS]>::try_from(view.state.sizes.clone()) {
            self.sizes = sizes;
        }
        if let Ok(hidden) = <[bool; NCOLS]>::try_from(view.state.hidden.clone()) {
            self.hidden = hidden;
        }
        if let Ok(modes) = <[SectionResizeMode; NCOLS]>::try_from(view.state.modes.clone()) {
            self.modes = modes;
        }
        self.sort_indicator = view.state.sort_indicator;
        self.sort_indicator_shown = view.state.sort_indicator_shown;
        self.painted = view.placements.len().min(NCOLS);
        for (slot, p) in self.placements.iter_mut().zip(&view.placements) {
            *slot = *p;
        }
        // R1493 — the effective sizes, from the one geometry walk. A section
        // that is not painted has no place in the division, so it reports the
        // size it was given, which is also the size it will come back at.
        self.section_sizes = self.sizes;
        for p in &view.placements {
            if let Some(slot) = self.section_sizes.get_mut(p.logical) {
                *slot = p.size;
            }
        }
    }

    /// R1491 — the body row order the header's indicator implies: Qt's
    /// `sortIndicatorChanged` → `sortByColumn` connection, written as a
    /// projection rather than a second stored order.
    ///
    /// The rows are sorted by the **logical** column the indicator names, so
    /// moving that column re-paints its arrow somewhere else and leaves the row
    /// order alone — which is the correct answer, and the one a visually-keyed
    /// indicator gets wrong.
    fn row_order(&self) -> Vec<usize> {
        grid_order_by(
            NROWS,
            self.sort_indicator,
            |col, a, b| cell_cmp(cell_text(a, col), cell_text(b, col)),
            |_| true,
        )
    }
}

fn read_header_state(scene: &Scene) -> HeaderState {
    let mut out = HeaderState::default();
    let Some(intro) = scene
        .find_external_with_tag(HDR_TAG)
        .and_then(|n| n.handle.introspect())
    else {
        return out;
    };
    out.absorb(&read_column_layout(intro));
    if let Some(IntrospectValue::Json(p)) = intro.query("preview") {
        out.dragging = p
            .get("from_visual")
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| usize::try_from(n).ok());
        out.insert_at = p
            .get("insert_at")
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| usize::try_from(n).ok());
    }
    if let Some(IntrospectValue::Int(i)) = intro.query("focused_index") {
        out.focused = usize::try_from(i).ok();
    }
    if let Some(IntrospectValue::Bool(g)) = intro.query("grabbed") {
        out.grabbed = g;
    }
    // R1492 — read off the wire like everything else the readout shows, so the
    // painted rule and the rule an agent queries cannot drift apart.
    if let (Some(IntrospectValue::Int(lo)), Some(IntrospectValue::Int(hi))) = (
        intro.query("min_section_size"),
        intro.query("max_section_size"),
    ) {
        out.bounds = (
            u32::try_from(lo).unwrap_or(DEFAULT_MIN_COL_WIDTH),
            u32::try_from(hi).unwrap_or(DEFAULT_MAX_COL_WIDTH),
        );
    }
    // R1493 — the third scalar rule, harvested the same way for the same
    // reason: the readout names it, so it must be the wire's value and not a
    // constant this binding remembers.
    if let Some(IntrospectValue::Int(d)) = intro.query("default_section_size") {
        out.default_size = u32::try_from(d).unwrap_or(DEFAULT_SECTION_SIZE);
    }
    if let Some(IntrospectValue::Bool(c)) = intro.query("cascading_section_resizes") {
        out.rules.cascading = c;
    }
    // R1498 — the rule, and the modes it overrides. Both off the wire, so the
    // readout cannot claim a mode the header is not applying.
    if let Some(IntrospectValue::Bool(s)) = intro.query("stretch_last_section") {
        out.rules.stretch_last = s;
    }
    out.effective_modes = out.modes;
    if let Some(IntrospectValue::Json(serde_json::Value::Array(items))) =
        intro.query("effective_resize_modes")
    {
        for (slot, m) in out.effective_modes.iter_mut().zip(&items) {
            if let Some(parsed) = m.as_str().and_then(|s| s.parse().ok()) {
                *slot = parsed;
            }
        }
    }
    // R1496 — the two permissions, off the wire for the same reason: the strip
    // paints them, so what it paints has to be what an agent is told.
    if let (Some(IntrospectValue::Bool(m)), Some(IntrospectValue::Bool(k))) = (
        intro.query("sections_movable"),
        intro.query("sections_clickable"),
    ) {
        out.permissions = (m, k);
    }
    out
}

/// The visual indices of the **painted** sections, left to right — the
/// keyboard's world. Read through the layout's own projection so the cursor
/// walks exactly what the eye sees.
fn visible_visuals(intro: &dyn ExternalIntrospect) -> Vec<usize> {
    read_column_layout(intro)
        .placements
        .iter()
        .map(|p| p.visual)
        .collect()
}

/// The logical column under the keyboard cursor, if the cursor is on a painted
/// section.
fn logical_at(intro: &dyn ExternalIntrospect, cursor: Option<usize>) -> Option<usize> {
    match intro.query(&format!("logical_index.{}", cursor?)) {
        Some(IntrospectValue::Int(l)) => usize::try_from(l).ok(),
        _ => None,
    }
}

/// R1452 — the sizing policy of logical section `logical`, read back through
/// the same wire slot an RPC client would use.
fn read_mode(intro: &dyn ExternalIntrospect, logical: usize) -> SectionResizeMode {
    match intro.query(&format!("resize_mode.{logical}")) {
        Some(IntrospectValue::Text(m)) => m.parse().unwrap_or_default(),
        _ => SectionResizeMode::default(),
    }
}

/// R1498 — the mode the header is **applying**, which is what a gesture has to
/// be judged against. The `m` cycle still reads [`read_mode`]: cycling advances
/// the mode that was set, and reading the override there would let one keypress
/// turn a filled `Interactive` section into a `ResizeToContents` one.
fn read_effective_mode(intro: &dyn ExternalIntrospect, logical: usize) -> SectionResizeMode {
    match intro.query(&format!("effective_resize_mode.{logical}")) {
        Some(IntrospectValue::Text(m)) => m.parse().unwrap_or_default(),
        _ => read_mode(intro, logical),
    }
}

/// The next mode in the `m`-key cycle. Ordered so the two stored-size modes sit
/// together and the two derived ones follow, which is the order the readout
/// reads.
fn next_mode(mode: SectionResizeMode) -> SectionResizeMode {
    match mode {
        SectionResizeMode::Interactive => SectionResizeMode::Fixed,
        SectionResizeMode::Fixed => SectionResizeMode::Stretch,
        SectionResizeMode::Stretch => SectionResizeMode::ResizeToContents,
        SectionResizeMode::ResizeToContents => SectionResizeMode::Interactive,
    }
}

/// The painted section `delta` steps along the strip from `cursor` (clamped at
/// the ends). With no cursor yet, entering from the left starts at the first
/// section and from the right at the last.
fn step_visual(visuals: &[usize], cursor: Option<usize>, delta: i64) -> Option<usize> {
    let last = i64::try_from(visuals.len().checked_sub(1)?).unwrap_or(0);
    let next = match cursor.and_then(|c| visuals.iter().position(|&v| v == c)) {
        Some(i) => (i64::try_from(i).unwrap_or(0) + delta).clamp(0, last),
        None if delta >= 0 => 0,
        None => last,
    };
    visuals.get(usize::try_from(next).unwrap_or(0)).copied()
}

/// Qt `setSectionHidden` as a keyboard gesture. The cursor then steps off the
/// section it just hid, because a cursor on an unpainted section has nothing to
/// point at.
fn toggle_hidden_at(intro: &mut dyn ExternalIntrospect, cursor: Option<usize>) -> bool {
    let Some(logical) = logical_at(&*intro, cursor) else {
        return false;
    };
    let hidden = matches!(
        intro.query(&format!("section_hidden.{logical}")),
        Some(IntrospectValue::Bool(true))
    );
    if intro
        .invoke(
            "set_section_hidden",
            IntrospectValue::Text(format!("{logical}:{}", !hidden)),
        )
        .is_err()
    {
        return false;
    }
    let after = visible_visuals(&*intro);
    if let Some(next) = cursor.and_then(|c| {
        after
            .iter()
            .find(|&&v| v >= c)
            .or_else(|| after.last())
            .copied()
    }) {
        set_cursor(intro, next);
    }
    true
}

/// R1452 — Qt `setSectionResizeMode`, cycled in place. The whole row re-sizes
/// when a `Stretch` section joins or leaves it, which is why the model answers
/// with the resulting widths.
fn cycle_mode_at(intro: &mut dyn ExternalIntrospect, cursor: Option<usize>) -> bool {
    let Some(logical) = logical_at(&*intro, cursor) else {
        return false;
    };
    let next = next_mode(read_mode(&*intro, logical));
    intro
        .invoke(
            "set_resize_mode",
            IntrospectValue::Text(format!("{logical}:{next}")),
        )
        .is_ok()
}

/// R1498 — Qt `setStretchLastSection`, toggled. A header-wide rule, so it takes
/// no cursor: there is no section to aim it at, which is exactly what makes it
/// different from the `Stretch` mode `m` can put on one.
fn toggle_stretch_last(intro: &mut dyn ExternalIntrospect) -> bool {
    let on = matches!(
        intro.query("stretch_last_section"),
        Some(IntrospectValue::Bool(true))
    );
    intro
        .intervene("stretch_last_section", IntrospectValue::Bool(!on))
        .is_ok()
}

/// Qt `resizeSection` — the size is keyed by the logical section, so a column
/// widened here stays wide wherever it is dragged next.
///
/// R1452 — but only where the mode says a USER may resize. `Fixed` is exactly
/// the mode that refuses this while still accepting the programmatic
/// `resize_section`, so the gate is the mode's own predicate rather than a
/// second rule stated here.
///
/// R1494 — and because this IS the user's resize, it goes through
/// `interactive_resize_section`, the entry point Qt's `cascadingSectionResizes`
/// governs. It used to call the programmatic `resize_section`, which the very
/// doc above says is the one a `Fixed` section still accepts: this gesture was
/// gated like an interactive resize while writing like a programmatic one.
/// This binding has no pointer grabber (`column_resize_externals` lives in
/// `hello-grid-hscroll`, over a `ColumnWidths` with no layout), so the keyboard
/// is its interactive resize.
///
/// R1498 — gated on the **effective** mode. Under `stretchLastSection` the last
/// painted section's set mode is still `Interactive` while its width comes from
/// the leftover, so gating on the set mode would accept the keystroke and paint
/// nothing — the same "gated like one resize, writing like another" split R1494
/// found here.
fn nudge_size_at(intro: &mut dyn ExternalIntrospect, cursor: Option<usize>, grow: bool) -> bool {
    let Some(logical) = logical_at(&*intro, cursor) else {
        return false;
    };
    if !read_effective_mode(&*intro, logical).user_resizable() {
        return false;
    }
    let size = match intro.query(&format!("section_size.{logical}")) {
        Some(IntrospectValue::Int(n)) => u32::try_from(n).unwrap_or(0),
        _ => return false,
    };
    let next = if grow {
        size.saturating_add(RESIZE_STEP)
    } else {
        size.saturating_sub(RESIZE_STEP)
    };
    intro
        .invoke(
            "interactive_resize_section",
            IntrospectValue::Text(format!("{logical}:{next}")),
        )
        .is_ok()
}

/// Move the keyboard cursor to visual index `target`.
fn set_cursor(intro: &mut dyn ExternalIntrospect, target: usize) -> bool {
    intro
        .intervene(
            "focused_index",
            IntrospectValue::Int(i64::try_from(target).unwrap_or(0)),
        )
        .is_ok()
}

/// One header section cell, tagged `colhdr#<visual>` so the router's `'#'`
/// split reaches the composite external and the model's drop classification
/// sees a real subindex.
fn section_cell(p: &SectionPlacement, state: &HeaderState, theme: &Theme) -> Scene {
    let is_dragged = state.dragging == Some(p.visual);
    let fill = if is_dragged {
        theme.resolve(ColorRole::SurfaceContainerLow)
    } else if state.focused == Some(p.visual) {
        theme.resolve(ColorRole::SurfaceContainerHighest)
    } else {
        theme.resolve(ColorRole::SurfaceContainerHigh)
    };
    let visual = p.visual;
    let label = Scene::Text(
        TextNode::styled(
            HEADERS[p.logical],
            Rect::default(),
            grid_text(
                if is_dragged {
                    ColorRole::OnSurfaceMuted
                } else {
                    ColorRole::OnSurface
                },
                theme,
            ),
        )
        .with_tag(format!("colhdr_label#{visual}"))
        .with_layout(LayoutStyle::new().with_absolute_position(12, 12)),
    );
    // R1491 — the sort arrow, asked for by LOGICAL column through the same
    // `col_sort_dir` / `sort_glyph` pair every other pinion grid header uses.
    // It rides in this cell, so it moves when the section moves and needs no
    // second geometry walk to find its x.
    let mut children = vec![label];
    if let Some(glyph) = sort_glyph(
        state
            .sort_indicator_shown
            .then(|| col_sort_dir(state.sort_indicator, p.logical))
            .flatten(),
    ) {
        children.push(Scene::Text(
            TextNode::styled(glyph, Rect::default(), grid_text(ColorRole::Accent, theme))
                .with_tag(format!("colhdr_sort#{visual}"))
                .with_layout(
                    LayoutStyle::new().with_absolute_position(p.size.saturating_sub(24), 12),
                ),
        ));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(section_tag(visual))
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(p.x, 0)
                    .with_size(Size::px(p.size.saturating_sub(2), HDR_H)),
            ),
    )
}

/// The strip that owns the sections. It carries the external's own tag and is
/// the §5.39 Tab stop, so the keyboard model has something to focus — Qt's
/// `QHeaderView` is one focusable widget whose sections are its parts, not five
/// separate tab stops.
fn header_strip(sections: Vec<Scene>, total_w: u32, theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(sections)
            .with_tag(HDR_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainer)))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(GRID_X, GRID_Y)
                    .with_size(Size::px(total_w, HDR_H))
                    .with_focusable(true),
            ),
    )
}

/// The insertion line the live drag draws at gap `insert_at`.
fn insertion_line(state: &HeaderState, insert_at: usize, theme: &Theme) -> Scene {
    let x = GRID_X + state.gap_x(insert_at);
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag("colhdr_dropline")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Accent)))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(x.saturating_sub(1), GRID_Y)
                    .with_size(Size::px(
                        3,
                        HDR_H + u32::try_from(NROWS).unwrap_or(0) * ROW_H,
                    )),
            ),
    )
}

/// One body cell, tagged `colbody#<slot>_<visual>` — the data at the logical
/// column now displayed at `visual`, for the record the sort put in `slot`.
///
/// R1491 — `slot` and `data_row` were one argument until the header learned to
/// sort. Both axes now name a *place* on the paint (`slot` down, `visual`
/// across) while the content comes from the model (`data_row`, `logical`); a
/// tag that carried the record id instead would move under a client between
/// sorts, which is the opposite of what a paint tag is for.
fn body_cell(slot: usize, data_row: usize, p: &SectionPlacement, theme: &Theme) -> Scene {
    let (row_i, visual) = (slot, p.visual);
    let label = Scene::Text(
        TextNode::styled(
            cell_text(data_row, p.logical),
            Rect::default(),
            grid_text(ColorRole::OnSurface, theme),
        )
        .with_tag(format!("{BODY_TAG}#{row_i}_{visual}"))
        .with_layout(LayoutStyle::new().with_absolute_position(12, 9)),
    );
    Scene::Container(
        ContainerNode::new(vec![label])
            // Banding follows the painted slot, not the record: the stripes
            // must stay put when the rows behind them are reordered.
            .with_style(BoxStyle::filled(if slot % 2 == 0 {
                theme.resolve(ColorRole::Surface)
            } else {
                theme.resolve(ColorRole::SurfaceContainerLow)
            }))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(
                        GRID_X + p.x,
                        GRID_Y + HDR_H + u32::try_from(slot).unwrap_or(0) * ROW_H,
                    )
                    .with_size(Size::px(p.size.saturating_sub(2), ROW_H - 2)),
            ),
    )
}

/// R1451 — the header axes the order row cannot show: the LOGICAL-keyed sizes
/// (so a reader can see which section owns a width, not which position), the
/// hidden set, the modes, and (R1492 / R1493) the three scalar rules that shape
/// every size.
///
/// R1493 — a section has a size it was **given** and a size it **has**. This
/// leads with the one on screen, and names the other only when they differ,
/// which is exactly when a reader needs to be told there are two. Printing the
/// stored size under a `Stretch` header, as this row used to, put a number
/// directly beneath a section painted at a different width.
fn layout_readout_text(state: &HeaderState) -> String {
    let hidden: Vec<&str> = state
        .hidden
        .iter()
        .enumerate()
        .filter(|(_, h)| **h)
        .map(|(l, _)| HEADERS[l])
        .collect();
    let keyed = |sizes: &[u32; NCOLS]| {
        sizes
            .iter()
            .enumerate()
            .map(|(l, w)| format!("{}={w}", HEADERS[l]))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let stored = if state.section_sizes == state.sizes {
        String::new()
    } else {
        format!(" | stored {}", keyed(&state.sizes))
    };
    let codes = |modes: &[SectionResizeMode; NCOLS]| -> String {
        modes.iter().map(|m| mode_code(*m)).collect()
    };
    // R1498 — the same shape the sizes use above, for the same reason: the row
    // names what the header APPLIES, and shows what was set beside it only when
    // a rule has overridden it.
    let stored_modes = if state.effective_modes == state.modes {
        String::new()
    } else {
        format!(" | stored modes {}", codes(&state.modes))
    };
    // R1496 — one word for the pair, because they are read together: what this
    // header lets a hand do to it.
    let allows = match state.permissions {
        (true, true) => "move+click",
        (true, false) => "move",
        (false, true) => "click",
        (false, false) => "-",
    };
    format!(
        "sizes {}{stored} | hidden {} | modes {}{stored_modes} | default {} | \
         cascade {} | stretch-last {} | bounds {}..{} | allows {allows}",
        keyed(&state.section_sizes),
        if hidden.is_empty() {
            "-".to_string()
        } else {
            hidden.join(" ")
        },
        codes(&state.effective_modes),
        state.default_size,
        if state.rules.cascading { "on" } else { "off" },
        if state.rules.stretch_last {
            "on"
        } else {
            "off"
        },
        state.bounds.0,
        // R1492 — the unbounded default reads as `-`, because printing
        // 4294967295 would look like a limit somebody chose.
        if state.bounds.1 == DEFAULT_MAX_COL_WIDTH {
            "-".to_string()
        } else {
            state.bounds.1.to_string()
        },
    )
}

// The `&Frame` is the shape `WidgetCore::view` hands over; the state is now a
// multi-array `Copy` struct, so it is the frame this lint is about.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &HeaderState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    // R1452 — publish into the shared layout what only the view knows: the
    // width a `Stretch` row divides, and the MEASURED content hints a
    // `ResizeToContents` section sizes to. Both are constant for this grid, so
    // only the very first frame paints before them (the measure-then-settle
    // warmup every measured seam in the tree has); a grid whose content
    // changes republishes here each frame for the same reason.
    let layout = use_column_layout_with(LAYOUT_KEY, boot_layout);
    layout.set_available_width(Some(AVAILABLE_W));
    if let Some(hints) = content_hints(&theme, layout.resize_contents_precision()) {
        layout.set_content_widths(hints);
    }
    let mut children: Vec<Scene> = Vec::with_capacity(NCOLS * (NROWS + 1) + 4);

    let caption = Scene::Text(
        TextNode::styled(
            "Drag a header to move it; [ ] resize, h hides, m cycles sizing, f fills",
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(GRID_X, 30)),
    );
    let order_row = Scene::Text(
        TextNode::styled(
            format!(
                "order {} | grabbed {}",
                state.labels().join(" "),
                state.grabbed,
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(ORDER_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(GRID_X, 55)),
    );
    let layout_row = Scene::Text(
        TextNode::styled(
            layout_readout_text(state),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(LAYOUT_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(GRID_X, 72)),
    );
    children.push(caption);
    children.push(order_row);
    children.push(layout_row);

    // R1491 — one row permutation for the whole body, computed once from the
    // header's indicator and shared by every column, so two columns can never
    // paint two different orderings of the same records.
    let rows = state.row_order();
    let mut sections: Vec<Scene> = Vec::with_capacity(NCOLS);
    for p in state.placements() {
        sections.push(section_cell(p, state, &theme));
        for (slot, &data_row) in rows.iter().enumerate() {
            children.push(body_cell(slot, data_row, p, &theme));
        }
    }
    children.push(header_strip(sections, state.total_w(), &theme));
    if let Some(gap) = state.insert_at {
        children.push(insertion_line(state, gap, &theme));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start),
            ),
    )
}

struct ColumnReorderView;

impl WidgetCore for ColumnReorderView {
    type State = HeaderState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ColumnHeaderExternal::new())
    }

    fn tag() -> &'static str {
        HDR_TAG
    }

    fn read_state(scene: &Scene) -> HeaderState {
        read_header_state(scene)
    }

    fn view(state: HeaderState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-column-reorder (R1451 §5.27 QHeaderView section layout)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// APG keyboard drag, modifier-free so it drives through plain `scene/key`:
    /// arrows move the cursor, or the grabbed section; Space / Enter picks up
    /// and drops; Escape cancels back to the pre-grab order. The policy is the
    /// binding's; every mutation is the model's.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(HDR_TAG) {
            return false;
        }
        let Some(intro) = scene
            .find_external_with_tag_mut(HDR_TAG)
            .and_then(|n| n.handle.introspect_mut())
        else {
            return false;
        };
        let cursor = match intro.query("focused_index") {
            Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
            _ => None,
        };
        let grabbed = matches!(intro.query("grabbed"), Some(IntrospectValue::Bool(true)));
        // R1451 — the cursor walks PAINTED sections, so a hidden column is not
        // a hole the keyboard can fall into. One list feeds both the cursor and
        // a grabbed section's move, so the two cannot disagree about what "the
        // next column" is.
        let visuals = visible_visuals(&*intro);
        match key {
            "ArrowRight" | "ArrowLeft" | "Home" | "End" => {
                let target = match key {
                    "Home" => visuals.first().copied(),
                    "End" => visuals.last().copied(),
                    "ArrowRight" => step_visual(&visuals, cursor, 1),
                    _ => step_visual(&visuals, cursor, -1),
                };
                let Some(target) = target else { return false };
                if grabbed {
                    // Move the picked-up section to that position; the model's
                    // move funnel carries the cursor with it.
                    intro
                        .invoke(
                            "move_section",
                            IntrospectValue::Text(format!("{}:{target}", cursor.unwrap_or(0))),
                        )
                        .is_ok()
                } else {
                    set_cursor(intro, target)
                }
            }
            " " | "Enter" => {
                cursor.is_some() && intro.invoke("grab", IntrospectValue::Null).is_ok()
            }
            "Escape" => grabbed && intro.invoke("grab_cancel", IntrospectValue::Null).is_ok(),
            // Qt `setSectionHidden` — the column chooser as a keyboard gesture.
            "h" => toggle_hidden_at(intro, cursor),
            // R1452 — Qt `setSectionResizeMode`, cycled in place.
            "m" => cycle_mode_at(intro, cursor),
            // R1498 — Qt `setStretchLastSection`. A header rule rather than a
            // per-section one, so unlike `m` and `h` it needs no cursor.
            "f" => toggle_stretch_last(intro),
            // Qt `resizeSection`, gated by the mode (R1452).
            "]" | "[" => nudge_size_at(intro, cursor, key == "]"),
            _ => false,
        }
    }

    fn update(_state: HeaderState, _intent: &Intent) -> Vec<Command> {
        Vec::new()
    }

    fn fmt_state_log(state: &HeaderState) -> String {
        format!(
            "order={:?} sizes={:?} hidden={:?} modes={:?} focused={:?}",
            state.order, state.sizes, state.hidden, state.modes, state.focused
        )
    }
}

impl WidgetA11y for ColumnReorderView {
    /// The header strip as a WAI-ARIA `row` of `columnheader`s, announced in
    /// **visual** order — an AT reading the strip left to right hears what the
    /// screen shows, which is the whole point of a movable section.
    fn access_node(state: &HeaderState, focused: Option<&str>) -> Vec<AccessNode> {
        let strip_focused = focused == Some(HDR_TAG);
        let mut nodes = vec![
            AccessNode::new(HDR_TAG, AriaRole::Row)
                .with_name("Columns")
                .with_state(AccessState {
                    focused: strip_focused,
                    ..AccessState::default()
                }),
        ];
        // R1451 — the same placements the paint uses, so a hidden column is
        // absent from the AT tree for exactly the reason it is absent from the
        // screen, and `aria-colcount` follows without a second rule.
        for p in state.placements() {
            let mut node = AccessNode::new(section_tag(p.visual), AriaRole::ColumnHeader)
                .with_name(HEADERS[p.logical])
                .with_state(AccessState {
                    focused: strip_focused && state.focused == Some(p.visual),
                    ..AccessState::default()
                });
            // R1491 — `aria-sort` on the section that carries the indicator,
            // asked by LOGICAL column, so an AT hears the sort move with its
            // column exactly as the eye sees the arrow move. Gated on `shown`
            // for the same reason the glyph is: a hidden indicator is not
            // something to announce.
            if let Some(dir) = state
                .sort_indicator_shown
                .then(|| col_sort_dir(state.sort_indicator, p.logical))
                .flatten()
            {
                node = node.with_sort(SortDirection::from_ascending(dir));
            }
            nodes.push(node);
        }
        nodes
    }
}

impl WidgetView for ColumnReorderView {
    type Renderer = HelloColumnReorderRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ColumnReorderView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;
    use pinion_core::scene::ExternalNode;

    /// R1452 — the external resolves the SHARED layout through
    /// `use_column_layout`, so a test builds it inside an owner scope exactly
    /// as the shell does. A fresh scope per call is a fresh layout, which is
    /// what each test wants; the `Rc` the external keeps outlives the scope.
    fn fresh() -> ColumnHeaderExternal {
        Owner::new().run(ColumnHeaderExternal::new)
    }

    fn boot_scene() -> Scene {
        Owner::new().run(|| {
            Scene::Container(ContainerNode::new(vec![Scene::External(
                ExternalNode::new(ColumnReorderView::create_external()).with_tag(HDR_TAG),
            )]))
        })
    }

    fn press(scene: &mut Scene, key: &str) -> bool {
        ColumnReorderView::apply_key(scene, Some(HDR_TAG), key, pinion_core::Modifiers::empty())
    }

    fn order_of(scene: &Scene) -> [usize; NCOLS] {
        read_header_state(scene).order
    }

    #[test]
    fn r1450_the_index_mapping_is_the_inverse_of_the_order() {
        let mut ext = fresh();
        ext.invoke("move_section", IntrospectValue::Text("0:2".into()))
            .expect("move_section is a known action");
        // order = [Type, Size, Name, Modified, Owner] = [1, 2, 0, 3, 4]
        assert_eq!(
            ext.layout.logical_index(2),
            Some(0),
            "Name is displayed third"
        );
        assert_eq!(
            ext.layout.visual_index(0),
            Some(2),
            "and Name's visual index is 2"
        );
        for logical in 0..NCOLS {
            let v = ext
                .layout
                .visual_index(logical)
                .expect("every column is placed");
            assert_eq!(
                ext.layout.logical_index(v),
                Some(logical),
                "the two directions must invert each other"
            );
        }
        assert_eq!(ext.query("visual_index.9"), Some(IntrospectValue::Null));
        assert_eq!(ext.query("logical_index.9"), Some(IntrospectValue::Null));
    }

    #[test]
    fn r1450_the_labels_readout_is_the_visual_order() {
        let mut ext = fresh();
        ext.invoke("move_section", IntrospectValue::Text("4:0".into()))
            .expect("move_section is a known action");
        assert_eq!(
            ext.query("labels"),
            Some(IntrospectValue::Json(serde_json::json!([
                "Owner", "Name", "Type", "Size", "Modified"
            ])))
        );
    }

    #[test]
    fn r1450_the_body_follows_its_header() {
        // The projection is one function, so a section move must carry its data.
        let mut scene = boot_scene();
        let before: Vec<&str> = (0..NCOLS).map(|c| cell_text(0, c)).collect();
        assert_eq!(before[0], "report.pdf", "Name is the first column's data");
        {
            let intro = scene
                .find_external_with_tag_mut(HDR_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .expect("the header external is in the scene");
            intro
                .invoke("move_section", IntrospectValue::Text("0:4".into()))
                .expect("move_section is a known action");
        }
        let order = order_of(&scene);
        assert_eq!(order.last(), Some(&0), "Name is now displayed last");
        assert_eq!(
            cell_text(0, order[4]),
            "report.pdf",
            "and its data is what the last visual column paints"
        );
        assert_eq!(cell_text(0, order[0]), "PDF", "Type took the first slot");
    }

    #[test]
    fn r1450_a_saved_order_restores_over_the_wire() {
        let mut ext = fresh();
        ext.intervene(
            "order",
            IntrospectValue::Json(serde_json::json!([4, 3, 2, 1, 0])),
        )
        .expect("a permutation restores the layout");
        assert_eq!(
            ext.query("labels"),
            Some(IntrospectValue::Json(serde_json::json!([
                "Owner", "Modified", "Size", "Type", "Name"
            ])))
        );
        // Not a permutation: refused, and nothing moved.
        assert!(matches!(
            ext.intervene(
                "order",
                IntrospectValue::Json(serde_json::json!([0, 0, 1, 2, 3]))
            ),
            Err(InterveneError::OutOfRange)
        ));
        assert_eq!(
            ext.layout.logical_index(0),
            Some(4),
            "the refused write changed nothing"
        );
        assert!(matches!(
            ext.intervene("labels", IntrospectValue::Int(0)),
            Err(InterveneError::ReadOnly)
        ));
    }

    #[test]
    fn r1450_the_keyboard_grab_moves_a_section_and_escape_reverts() {
        let mut scene = boot_scene();
        assert!(press(&mut scene, "ArrowRight"), "cursor lands on section 0");
        assert!(press(&mut scene, "ArrowRight"), "cursor moves to 1");
        assert!(press(&mut scene, " "), "Space picks the section up");
        assert!(press(&mut scene, "ArrowRight"), "the grabbed section moves");
        assert_eq!(order_of(&scene), [0, 2, 1, 3, 4], "Type and Size swapped");
        assert!(press(&mut scene, "Escape"), "Escape cancels the grab");
        assert_eq!(
            order_of(&scene),
            [0, 1, 2, 3, 4],
            "and reverts to the pre-grab order"
        );
    }

    #[test]
    fn r1450_the_a11y_strip_announces_the_visual_order() {
        let mut scene = boot_scene();
        {
            let intro = scene
                .find_external_with_tag_mut(HDR_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .expect("the header external is in the scene");
            intro
                .invoke("move_section", IntrospectValue::Text("4:0".into()))
                .expect("move_section is a known action");
        }
        let state = read_header_state(&scene);
        let nodes = ColumnReorderView::access_node(&state, Some(HDR_TAG));
        let names: Vec<&str> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .filter_map(|n| n.name.as_deref())
            .collect();
        assert_eq!(
            names,
            ["Owner", "Name", "Type", "Size", "Modified"],
            "an AT reads the strip in the order the screen shows"
        );
    }

    /// Drive the header external in a scene and hand back the state the view
    /// would paint from.
    fn after(scene: &mut Scene, f: impl FnOnce(&mut dyn ExternalIntrospect)) -> HeaderState {
        {
            let intro = scene
                .find_external_with_tag_mut(HDR_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .expect("the header external is in the scene");
            f(intro);
        }
        read_header_state(scene)
    }

    #[test]
    fn r1451_a_resized_section_keeps_its_width_where_it_is_dragged() {
        // The composition R1450 could not express. `Name` starts 150 wide;
        // widen it to 240 and drag it to the middle, and it must arrive 240
        // wide with `Type` and `Size` closed up in front of it.
        let mut scene = boot_scene();
        let state = after(&mut scene, |i| {
            i.invoke("resize_section", IntrospectValue::Text("0:240".into()))
                .expect("resize");
            i.invoke("move_section", IntrospectValue::Text("0:2".into()))
                .expect("move");
        });

        assert_eq!(
            state.labels(),
            ["Type", "Size", "Name", "Modified", "Owner"]
        );
        // A position-keyed width model answers 150 here, because it never
        // learned the column moved — this number is the whole round.
        let name = state.placements()[2];
        assert_eq!(name.logical, 0, "the third section is Name");
        assert_eq!(name.size, 240, "and it kept the width it was given");
        assert_eq!(name.x, 190, "Type (90) + Size (100) precede it");
        assert_eq!(state.sizes[0], 240, "the size is keyed by logical section");
        assert_eq!(
            state.total_w(),
            240 + 90 + 100 + 130 + 100,
            "the strip grew by exactly what the section gained"
        );
        // The body under that header is Name's data at Name's geometry — one
        // projection feeds both, so they cannot disagree.
        assert_eq!(cell_text(0, name.logical), "report.pdf");
    }

    #[test]
    fn r1451_hiding_drops_a_column_from_the_paint_the_labels_and_the_at_tree() {
        let mut scene = boot_scene();
        let state = after(&mut scene, |i| {
            i.invoke("set_section_hidden", IntrospectValue::Text("1:true".into()))
                .expect("hide");
        });

        assert_eq!(state.labels(), ["Name", "Size", "Modified", "Owner"]);
        assert_eq!(state.painted, 4);
        // The sections after the hidden one close up, but keep the visual
        // indices the permutation knows them by.
        let visuals: Vec<usize> = state.placements().iter().map(|p| p.visual).collect();
        assert_eq!(visuals, [0, 2, 3, 4]);
        assert_eq!(state.placements()[1].x, 150, "Size closed up behind Name");
        assert_eq!(state.total_w(), 480, "the strip lost Type's 90");
        // Same projection, so the AT tree loses it for the same reason.
        let nodes = ColumnReorderView::access_node(&state, Some(HDR_TAG));
        let names: Vec<&str> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .filter_map(|n| n.name.as_deref())
            .collect();
        assert_eq!(names, ["Name", "Size", "Modified", "Owner"]);
        // Hidden is not forgotten: the section keeps its size and comes back
        // where it was.
        assert_eq!(state.sizes[1], 90);
        let shown = after(&mut scene, |i| {
            i.invoke(
                "set_section_hidden",
                IntrospectValue::Text("1:false".into()),
            )
            .expect("show");
        });
        assert_eq!(
            shown.labels(),
            ["Name", "Type", "Size", "Modified", "Owner"]
        );
    }

    #[test]
    fn r1451_the_keyboard_and_the_wire_reach_the_same_layout() {
        // §2 #2 — a human gesture and an RPC call are two doors onto one
        // model. Anything only one of them can do is a divergence waiting to
        // happen ([[r1449-completion-model]]).
        let mut keyed = boot_scene();
        assert!(press(&mut keyed, "ArrowRight"), "cursor to section 0");
        assert!(press(&mut keyed, "]"), "widen Name");
        assert!(press(&mut keyed, "]"), "and again");
        assert!(press(&mut keyed, "ArrowRight"), "cursor to section 1");
        assert!(press(&mut keyed, "h"), "hide Type");

        let mut wired = boot_scene();
        let wired_state = after(&mut wired, |i| {
            i.invoke(
                "resize_section",
                IntrospectValue::Text(format!("0:{}", 150 + 2 * RESIZE_STEP)),
            )
            .expect("resize");
            i.invoke("set_section_hidden", IntrospectValue::Text("1:true".into()))
                .expect("hide");
        });
        let keyed_state = read_header_state(&keyed);

        assert_eq!(keyed_state.sizes, wired_state.sizes);
        assert_eq!(keyed_state.hidden, wired_state.hidden);
        assert_eq!(keyed_state.order, wired_state.order);
        assert_eq!(keyed_state.sizes[0], 190, "and it is the value both meant");
    }

    #[test]
    fn r1451_the_keyboard_resizes_the_section_not_the_slot() {
        // The keyboard aims at a screen position; `resize_section` takes a
        // logical section. Move a column first, and the two must still name
        // the same thing.
        let mut scene = boot_scene();
        after(&mut scene, |i| {
            i.invoke("move_section", IntrospectValue::Text("0:4".into()))
                .expect("Name to the end");
        });
        // Cursor onto the LAST section, which is now Name.
        assert!(press(&mut scene, "End"));
        assert!(press(&mut scene, "]"));
        let state = read_header_state(&scene);
        assert_eq!(state.sizes[0], 170, "Name grew");
        assert_eq!(state.sizes[4], 100, "Owner, which used to be last, did not");
    }

    #[test]
    fn r1451_the_cursor_steps_over_a_hidden_section() {
        // A cursor that walked the raw permutation would land on a section
        // that is painted nowhere.
        let mut scene = boot_scene();
        after(&mut scene, |i| {
            i.invoke("set_section_hidden", IntrospectValue::Text("1:true".into()))
                .expect("hide Type");
        });
        assert!(press(&mut scene, "ArrowRight"), "cursor to section 0");
        assert_eq!(read_header_state(&scene).focused, Some(0));
        assert!(press(&mut scene, "ArrowRight"));
        assert_eq!(
            read_header_state(&scene).focused,
            Some(2),
            "visual 1 is hidden, so the cursor skips to visual 2"
        );
        assert!(press(&mut scene, "End"));
        assert_eq!(read_header_state(&scene).focused, Some(4));
    }

    #[test]
    fn r1451_hiding_under_the_cursor_moves_the_cursor_off_it() {
        let mut scene = boot_scene();
        assert!(press(&mut scene, "ArrowRight"), "cursor to section 0");
        assert!(press(&mut scene, "h"), "hide the section under the cursor");
        let state = read_header_state(&scene);
        assert!(state.hidden[0], "Name is hidden");
        assert_eq!(
            state.focused,
            Some(1),
            "and the cursor moved to the first section still painted"
        );
    }

    #[test]
    fn r1451_the_whole_layout_round_trips_over_the_wire() {
        // Qt's saveState / restoreState, except a client can read the state it
        // is holding. Arrange a distinctive layout, save, drift, restore.
        let mut scene = boot_scene();
        let arranged = after(&mut scene, |i| {
            i.invoke("resize_section", IntrospectValue::Text("3:220".into()))
                .expect("resize");
            i.invoke("set_section_hidden", IntrospectValue::Text("2:true".into()))
                .expect("hide");
            i.invoke("swap_sections", IntrospectValue::Text("0:4".into()))
                .expect("swap");
        });
        let saved = {
            let intro = scene
                .find_external_with_tag(HDR_TAG)
                .and_then(|n| n.handle.introspect())
                .expect("external");
            intro.query("state").expect("state is readable")
        };

        let drifted = after(&mut scene, |i| {
            i.invoke("move_section", IntrospectValue::Text("0:3".into()))
                .expect("move");
            i.invoke(
                "set_section_hidden",
                IntrospectValue::Text("2:false".into()),
            )
            .expect("show");
            i.invoke("resize_section", IntrospectValue::Text("3:60".into()))
                .expect("shrink");
        });
        assert_ne!(drifted, arranged, "the layout really did drift");

        let restored = after(&mut scene, |i| {
            i.intervene("state", saved.clone()).expect("restore");
        });
        assert_eq!(restored.order, arranged.order);
        assert_eq!(restored.sizes, arranged.sizes);
        assert_eq!(restored.hidden, arranged.hidden);
        assert_eq!(restored.labels(), arranged.labels());
    }

    #[test]
    fn r1452_the_mode_key_cycles_and_the_resize_key_obeys_it() {
        // §2 #2 — one model, two doors: `m` reaches the same
        // `set_resize_mode` an RPC client calls, and `[` / `]` is gated by the
        // mode's OWN predicate rather than a second rule in the binding.
        let mut scene = boot_scene();
        assert!(press(&mut scene, "ArrowRight"), "cursor onto Name");

        assert!(press(&mut scene, "m"), "Interactive -> Fixed");
        let state = read_header_state(&scene);
        assert_eq!(state.modes[0], SectionResizeMode::Fixed);
        // Fixed is precisely the mode a user may not drag but a program may set.
        assert!(!press(&mut scene, "]"), "the key is refused");
        assert_eq!(
            read_header_state(&scene).sizes[0],
            SECTION_W[0],
            "unchanged"
        );
        let after = after(&mut scene, |i| {
            i.invoke("resize_section", IntrospectValue::Text("0:220".into()))
                .expect("but the programmatic path still works");
        });
        assert_eq!(after.sizes[0], 220);

        assert!(press(&mut scene, "m"), "Fixed -> Stretch");
        assert_eq!(
            read_header_state(&scene).modes[0],
            SectionResizeMode::Stretch
        );
        assert!(
            !press(&mut scene, "]"),
            "a derived section is not user-sized"
        );
        assert!(press(&mut scene, "m"), "Stretch -> ResizeToContents");
        assert!(press(&mut scene, "m"), "and back to Interactive");
        assert_eq!(
            read_header_state(&scene).modes[0],
            SectionResizeMode::Interactive
        );
        assert!(press(&mut scene, "]"), "which the user may size again");
        assert_eq!(read_header_state(&scene).sizes[0], 220 + RESIZE_STEP);
    }

    #[test]
    fn r1452_a_stretch_row_fills_the_width_the_view_publishes() {
        let mut scene = boot_scene();
        let state = after(&mut scene, |i| {
            // The view fn publishes this every frame; a test says it directly.
            i.intervene(
                "available_width",
                IntrospectValue::Int(i64::from(AVAILABLE_W)),
            )
            .expect("publish the viewport");
            i.invoke("set_resize_mode", IntrospectValue::Text("1:stretch".into()))
                .expect("Type stretches");
            i.invoke("set_resize_mode", IntrospectValue::Text("4:stretch".into()))
                .expect("Owner too");
        });
        // 640 less Name(150) + Size(100) + Modified(130) = 260, split two ways.
        assert_eq!(
            state
                .placements()
                .iter()
                .map(|p| p.size)
                .collect::<Vec<_>>(),
            vec![150, 130, 100, 130, 130]
        );
        assert_eq!(state.total_w(), AVAILABLE_W, "the row fills the strip");
    }

    /// A stand-in shaper: width proportional to the character count, so the
    /// test can tell "the hint picked the widest string" from "the hint picked
    /// the first one" without depending on a real face.
    #[derive(Debug)]
    struct CountingMetrics;

    impl pinion_core::TextMetrics for CountingMetrics {
        fn measure(
            &self,
            text: &str,
            _style: &TextStyle,
            _max_width: Option<u32>,
        ) -> Option<pinion_core::TextExtent> {
            Some(pinion_core::TextExtent::new(
                u32::try_from(text.chars().count()).unwrap_or(0) * 7,
                13,
            ))
        }
    }

    #[test]
    fn r1453_the_content_hint_is_the_widest_string_a_column_shows() {
        // R1453 — the hint measures every string the column shows and takes the
        // largest, rather than assuming a per-character width.
        let owner = Owner::new();
        pinion_core::TEXT_METRICS.provide(&owner, std::rc::Rc::new(CountingMetrics));
        let hints = owner
            .run(|| content_hints(&Theme::light(), NROWS))
            .expect("a seeded provider measures");

        // Longest strings: report.pdf(10) Image/Video(5) 2.1 MB(6) a date(10)
        // Owner/guest(5) — at 7px per character, plus the padding on both sides.
        assert_eq!(hints, vec![94, 59, 66, 94, 59]);
        // The hint is never narrower than any string it has to show.
        for (l, hint) in hints.iter().enumerate() {
            for s in column_strings(l, NROWS) {
                let w = u32::try_from(s.chars().count()).unwrap_or(0) * 7;
                assert!(*hint >= w + 2 * CELL_PAD, "{s:?} fits column {l}");
            }
        }
    }

    #[test]
    fn r1454_the_precision_bound_decides_how_many_rows_are_measured() {
        // R1454 — a content-fitted column samples the body rather than reading
        // all of it, because measuring every row each frame is what makes the
        // policy expensive. The header is always measured (Qt measures it too).
        let owner = Owner::new();
        pinion_core::TEXT_METRICS.provide(&owner, std::rc::Rc::new(CountingMetrics));
        let (all, one) = owner.run(|| {
            (
                content_hints(&Theme::light(), NROWS).expect("measures"),
                content_hints(&Theme::light(), 1).expect("measures"),
            )
        });
        // Type: header "Type"(4) + rows PDF Image Text Rust CSV Video. Sampling
        // one row sees only "PDF"(3), so the header decides — 4 x 7 + padding.
        assert_eq!(
            all[1],
            5 * 7 + 2 * CELL_PAD,
            "Image/Video decide the full set"
        );
        assert_eq!(
            one[1],
            4 * 7 + 2 * CELL_PAD,
            "one row leaves the header widest"
        );
        // Sampling fewer rows can only narrow a hint or leave it alone.
        for (l, (a, o)) in all.iter().zip(&one).enumerate() {
            assert!(o <= a, "column {l}: sampling fewer rows cannot widen it");
        }
        // And a precision past the row count is the whole body, not an error.
        let plenty = owner
            .run(|| content_hints(&Theme::light(), 10_000))
            .expect("measures");
        assert_eq!(plenty, all, "more precision than rows is every row");
    }

    #[test]
    fn r1453_no_provider_publishes_nothing_rather_than_a_guess() {
        // Headless, and in any harness without a shell: the binding must not
        // invent a width. All-or-nothing, so a partial measurement cannot mix a
        // real number with a made-up one.
        assert_eq!(
            Owner::new().run(|| content_hints(&Theme::light(), NROWS)),
            None
        );
    }

    #[test]
    fn r1452_the_mode_code_is_derived_from_the_wire_spelling() {
        // Four distinct initials, so the readout's one-letter code needs no
        // second table that could drift from the wire names.
        let codes: Vec<char> = [
            SectionResizeMode::Interactive,
            SectionResizeMode::Fixed,
            SectionResizeMode::Stretch,
            SectionResizeMode::ResizeToContents,
        ]
        .into_iter()
        .map(mode_code)
        .collect();
        assert_eq!(codes, ['i', 'f', 's', 'r']);
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len(), "the codes must stay distinct");
    }

    #[test]
    fn r1451_a_malformed_layout_is_refused_whole() {
        let mut scene = boot_scene();
        let before = after(&mut scene, |i| {
            i.invoke("resize_section", IntrospectValue::Text("1:200".into()))
                .expect("resize");
        });
        let after_bad = after(&mut scene, |i| {
            // Well-shaped, but `order` repeats a section — every field of this
            // write must be dropped, not just the order.
            assert!(matches!(
                i.intervene(
                    "state",
                    IntrospectValue::Json(serde_json::json!({
                        "order": [0, 0, 2, 3, 4],
                        "sizes": [10, 10, 10, 10, 10],
                        "hidden": [true, true, true, true, true],
                    })),
                ),
                Err(InterveneError::OutOfRange)
            ));
        });
        assert_eq!(after_bad, before, "a refused restore changed nothing");
    }

    // ----- R1491: the same strip is movable AND clickable -----

    /// The scene tag of the sort arrow painted in section `visual`, if any.
    fn arrow_visuals(scene: &Scene) -> Vec<usize> {
        let state = read_header_state(scene);
        let theme = Owner::new().run(|| use_theme(THEME_TAG).theme_animated());
        state
            .placements()
            .iter()
            .filter(|p| {
                let painted = section_cell(p, &state, &theme);
                let Scene::Container(c) = &painted else {
                    return false;
                };
                c.children.iter().any(|child| {
                    matches!(child, Scene::Text(t)
                        if t.tag.as_deref() == Some(&format!("colhdr_sort#{}", p.visual)))
                })
            })
            .map(|p| p.visual)
            .collect()
    }

    /// Drive one whole gesture exactly as the R794 router drives it: the
    /// `PointerDown`, the arm, the drop commit, and — **only when the cursor
    /// never travelled far enough to become a drag** — the trailing
    /// `PointerUp`.
    ///
    /// R1496 — `became_drag` is the router's own variable, and it is a
    /// parameter rather than something derived here because that determination
    /// is made from cursor travel against `DRAG_CLICK_THRESHOLD_PX`, which a
    /// unit test has no cursor for. Before this round the helper simply never
    /// dispatched the release, which let R1491's permutation-derived click pass
    /// while modelling a router that does not exist.
    fn gesture(
        ext: &mut ColumnHeaderExternal,
        visual: usize,
        over: Option<DropPoint>,
        became_drag: bool,
    ) {
        ext.invoke(
            "send",
            IntrospectValue::Text(format!("{visual}:PointerDown")),
        )
        .expect("send accepts a pointer payload");
        let payload = ext.begin_drag().expect("a pressed section arms a drag");
        let released_over = over.as_ref().map(|o| o.tag.clone());
        ext.drag_release(&payload, over);
        if !became_drag {
            // The router aims the release at the tag under the cursor, which
            // for a press-release in place is the section that was pressed.
            let tag = released_over.unwrap_or_else(|| section_tag(visual));
            let sub = tag.rsplit('#').next().unwrap_or_default().to_string();
            ext.invoke("send", IntrospectValue::Text(format!("{sub}:PointerUp")))
                .expect("send accepts a pointer payload");
        }
    }

    /// A press-release in place — the gesture that is a click.
    fn click_section(ext: &mut ColumnHeaderExternal, visual: usize) {
        gesture(ext, visual, None, false);
    }

    /// A drag that travelled: it commits its drop and is not a click.
    fn drag_section(ext: &mut ColumnHeaderExternal, visual: usize, over: DropPoint) {
        gesture(ext, visual, Some(over), true);
    }

    #[test]
    fn r1491_a_click_sorts_the_column_and_a_drag_only_moves_it() {
        // The composition this demo could not express before: `colhdr#<visual>`
        // gives the drop classifier its subindex AND carries a sort click. Both
        // halves are asserted, because either alone passes an implementation
        // that always does one of them.
        let mut ext = fresh();
        click_section(&mut ext, 1);
        assert_eq!(
            ext.query("sort_indicator"),
            Some(IntrospectValue::Text("1:ascending".into())),
            "a release in place sorted the section it pressed"
        );
        assert_eq!(
            ext.query("order"),
            Some(IntrospectValue::Json(serde_json::json!([0, 1, 2, 3, 4]))),
            "and moved nothing"
        );

        drag_section(
            &mut ext,
            0,
            DropPoint {
                tag: section_tag(3),
                x_rel: 0.9,
                y_rel: 0.5,
            },
        );
        assert_eq!(
            ext.query("order"),
            Some(IntrospectValue::Json(serde_json::json!([1, 2, 3, 0, 4]))),
            "the drag committed"
        );
        assert_eq!(
            ext.query("sort_indicator"),
            Some(IntrospectValue::Text("1:ascending".into())),
            "and did NOT also sort the section it dragged"
        );
    }

    #[test]
    fn r1496_a_drag_back_into_its_own_gap_is_still_not_a_click() {
        // THE regression R1491 shipped, and the reason the click moved off the
        // drop commit. Its rule was "the permutation came out unchanged, so it
        // was a click" — which is exactly what a user does when they pick a
        // column up, change their mind, and put it back. That sorted the column
        // they had just decided not to move.
        let mut ext = fresh();
        drag_section(
            &mut ext,
            1,
            DropPoint {
                tag: section_tag(1),
                x_rel: 0.1,
                y_rel: 0.5,
            },
        );
        assert_eq!(
            ext.query("order"),
            Some(IntrospectValue::Json(serde_json::json!([0, 1, 2, 3, 4]))),
            "the section went back where it came from"
        );
        assert_eq!(
            ext.query("sort_indicator"),
            Some(IntrospectValue::Text("none".into())),
            "and changing your mind is not a sort"
        );
    }

    #[test]
    fn r1496_a_pinned_header_still_sorts() {
        // Qt's two permissions are independent, and this is the shape that
        // proves it end to end: `setSectionsMovable(false)` +
        // `setSectionsClickable(true)` is the ordinary sortable table.
        let mut ext = fresh();
        ext.intervene("sections_movable", IntrospectValue::Bool(false))
            .expect("the permission is writable");

        ext.invoke("send", IntrospectValue::Text("1:PointerDown".into()))
            .expect("send accepts a pointer payload");
        assert!(
            ext.begin_drag().is_none(),
            "a pinned header opens no drag session, so nothing previews a drop"
        );
        ext.invoke("send", IntrospectValue::Text("1:PointerUp".into()))
            .expect("send accepts a pointer payload");
        assert_eq!(
            ext.query("sort_indicator"),
            Some(IntrospectValue::Text("1:ascending".into())),
            "and the press it refused to drag still sorted"
        );
        assert_eq!(
            ext.query("order"),
            Some(IntrospectValue::Json(serde_json::json!([0, 1, 2, 3, 4]))),
            "with nothing moved"
        );
    }

    #[test]
    fn r1496_the_readout_names_what_the_header_allows() {
        // R1492's rule: a gesture that does nothing looks like a broken widget
        // unless the rule that refused it is on screen.
        let mut scene = boot_scene();
        let boot = read_header_state(&scene);
        assert_eq!(boot.permissions, (true, true), "this app declares both");

        let pinned = after(&mut scene, |i| {
            i.intervene("sections_movable", IntrospectValue::Bool(false))
                .expect("the permission is writable");
        });
        assert_eq!(pinned.permissions, (false, true));
        assert!(
            layout_readout_text(&pinned).ends_with("| allows click"),
            "the painted rule: {}",
            layout_readout_text(&pinned)
        );
        assert!(
            layout_readout_text(&boot).ends_with("| allows move+click"),
            "and the boot posture names both"
        );
    }

    #[test]
    fn r1491_the_arrow_travels_with_its_section() {
        // The reason the indicator is header state and logical-keyed. A
        // visually-keyed one paints the arrow on whatever is now first.
        let mut scene = boot_scene();
        let sorted = after(&mut scene, |i| {
            i.invoke("cycle_sort_indicator", IntrospectValue::Int(0))
                .expect("cycle is a known action");
        });
        assert_eq!(sorted.sort_indicator, Some((0, true)));
        assert_eq!(arrow_visuals(&scene), vec![0], "Name is first and arrowed");

        let moved = after(&mut scene, |i| {
            i.invoke("move_section", IntrospectValue::Text("0:3".into()))
                .expect("move_section is a known action");
        });
        assert_eq!(
            moved.sort_indicator,
            Some((0, true)),
            "still sorted by Name"
        );
        assert_eq!(
            arrow_visuals(&scene),
            vec![3],
            "and the arrow is on Name's new position, not on the first section"
        );
    }

    #[test]
    fn r1491_a_click_after_a_move_sorts_the_column_it_landed_on() {
        // A click that answered the visual index would sort the wrong column
        // here — the section painted fourth IS logical 0.
        let mut ext = fresh();
        ext.invoke("move_section", IntrospectValue::Text("0:3".into()))
            .expect("move_section is a known action");
        click_section(&mut ext, 3);
        assert_eq!(
            ext.query("sort_indicator"),
            Some(IntrospectValue::Text("0:ascending".into())),
            "the click named the column, not the place it was clicked"
        );
    }

    #[test]
    fn r1491_the_body_paints_the_order_the_indicator_asks_for() {
        // The indicator is not decoration: the rows follow it. Sizes sort
        // numeric-aware through `cell_cmp`, so this also pins that the shared
        // comparator is the one in use rather than a local string sort.
        let mut scene = boot_scene();
        let unsorted = read_header_state(&scene);
        assert_eq!(
            unsorted.row_order(),
            (0..NROWS).collect::<Vec<_>>(),
            "boots unsorted, so the body paints the source order"
        );

        let sorted = after(&mut scene, |i| {
            i.intervene(
                "sort_indicator",
                IntrospectValue::Text("2:ascending".into()),
            )
            .expect("the compound string is the restore half");
        });
        let by_size: Vec<&str> = sorted
            .row_order()
            .iter()
            .map(|&r| cell_text(r, 2))
            .collect();
        let mut ascending = by_size.clone();
        ascending.sort_by(|a, b| cell_cmp(a, b));
        assert_eq!(by_size, ascending, "rows ascend by the Size column");

        // A section move re-aims the arrow, NOT the rows: the sort names a
        // column, and that column's values did not change.
        let after_move = after(&mut scene, |i| {
            i.invoke("move_section", IntrospectValue::Text("2:0".into()))
                .expect("move_section is a known action");
        });
        assert_eq!(after_move.row_order(), sorted.row_order());
    }

    #[test]
    fn r1491_the_at_tree_announces_the_sort_on_the_section_that_carries_it() {
        let mut scene = boot_scene();
        let state = after(&mut scene, |i| {
            i.invoke(
                "set_sort_indicator",
                IntrospectValue::Text("3:false".into()),
            )
            .expect("set_sort_indicator is a known action");
        });
        let nodes = ColumnReorderView::access_node(&state, Some(HDR_TAG));
        let sorted: Vec<(&str, Option<SortDirection>)> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .map(|n| (n.name.as_deref().unwrap_or(""), n.sort))
            .collect();
        assert_eq!(
            sorted,
            vec![
                ("Name", None),
                ("Type", None),
                ("Size", None),
                ("Modified", Some(SortDirection::Descending)),
                ("Owner", None),
            ],
            "exactly one columnheader carries aria-sort, and it is the sorted one"
        );
    }

    #[test]
    fn r1491_hiding_the_arrow_leaves_the_rows_where_the_sort_put_them() {
        // Qt's split: `sortIndicatorShown` is presentation, the section is
        // state. Conflating them would re-shuffle the body on a view toggle.
        let mut scene = boot_scene();
        let shown = after(&mut scene, |i| {
            i.invoke("cycle_sort_indicator", IntrospectValue::Int(2))
                .expect("cycle is a known action");
        });
        assert!(shown.sort_indicator_shown, "the view enabled sorting");
        assert_eq!(arrow_visuals(&scene), vec![2]);

        let hidden = after(&mut scene, |i| {
            i.intervene("sort_indicator_shown", IntrospectValue::Bool(false))
                .expect("shown is writable");
        });
        assert!(arrow_visuals(&scene).is_empty(), "no arrow is painted");
        assert_eq!(
            hidden.row_order(),
            shown.row_order(),
            "and the rows did not move"
        );
        assert_eq!(hidden.sort_indicator, Some((2, true)));
    }

    // ----- R1492: the bounds the header applies are the bounds it shows -----

    #[test]
    fn r1492_the_readout_shows_the_rule_the_clamp_applies() {
        // A clamp whose rule is invisible reads as a bug. The readout and the
        // wire are the same two numbers, read from the same place.
        let mut scene = boot_scene();
        let boot = read_header_state(&scene);
        assert_eq!(boot.bounds, (DEFAULT_MIN_COL_WIDTH, DEFAULT_MAX_COL_WIDTH));

        let bounded = after(&mut scene, |i| {
            i.intervene("max_section_size", IntrospectValue::Int(130))
                .expect("Qt's setMaximumSectionSize has a wire peer");
        });
        assert_eq!(bounded.bounds, (DEFAULT_MIN_COL_WIDTH, 130));
        assert_eq!(
            bounded.sizes.to_vec(),
            vec![130, 90, 100, 130, 100],
            "the sections over the new ceiling came down to it"
        );
        // `view` resolves the theme and the shared layout, so it runs inside an
        // owner scope exactly as the shell runs it.
        let painted = Owner::new().run(|| view(&bounded, &Frame::default()));
        let Scene::Container(root) = painted else {
            panic!("the view paints a container");
        };
        let readout = root
            .children
            .iter()
            .find_map(|c| match c {
                Scene::Text(t) if t.tag.as_deref() == Some(LAYOUT_TAG) => Some(t.content.clone()),
                _ => None,
            })
            .expect("the layout readout is painted");
        assert!(
            readout.contains("| bounds 40..130 |"),
            "the painted rule names both ends: {readout}"
        );
    }

    #[test]
    fn r1492_the_ceiling_reaches_a_derived_size_too() {
        // The demo is the forcing consumer for the "every path" claim: its
        // Stretch row divides a published viewport, which is a size no stored
        // width ever passed through.
        let mut scene = boot_scene();
        // The viewport a share divides is published by the VIEW fn, which does
        // not run here — so without this the sections fall back to their stored
        // sizes and the share path is never exercised at all. A counterfactual
        // that removed the ceiling from the share caught exactly that.
        let stretched = after(&mut scene, |i| {
            i.intervene(
                "available_width",
                IntrospectValue::Int(i64::from(AVAILABLE_W)),
            )
            .expect("the viewport is publishable over the wire");
            i.invoke(
                "set_all_resize_modes",
                IntrospectValue::Text("stretch".into()),
            )
            .expect("set_all_resize_modes is a known action");
        });
        let share = stretched.placements()[0].size;
        assert_eq!(
            u32::try_from(stretched.painted).map(|n| share * n),
            Ok(AVAILABLE_W),
            "an unbounded stretch row divides the whole viewport: {share}"
        );

        let bounded = after(&mut scene, |i| {
            i.intervene("max_section_size", IntrospectValue::Int(80))
                .expect("the ceiling is writable");
        });
        assert!(
            bounded.placements().iter().all(|p| p.size == 80),
            "every stretch share obeys the ceiling: {:?}",
            bounded.placements()
        );
    }

    // ----- R1493: the size a section was given vs the size it has -----

    /// The `section_sizes` slot as an agent reads it — the other producer of
    /// the effective sizes this binding derives from the placements.
    fn wire_section_sizes(scene: &Scene) -> Vec<u32> {
        let intro = scene
            .find_external_with_tag(HDR_TAG)
            .and_then(|n| n.handle.introspect())
            .expect("the header external is in the scene");
        let Some(IntrospectValue::Json(serde_json::Value::Array(a))) = intro.query("section_sizes")
        else {
            panic!("section_sizes reads back as a JSON array");
        };
        a.iter()
            .filter_map(|v| u32::try_from(v.as_u64()?).ok())
            .collect()
    }

    /// The painted readout row, for the assertions that it agrees with the
    /// strip above it.
    fn layout_readout(state: &HeaderState) -> String {
        let painted = Owner::new().run(|| view(state, &Frame::default()));
        let Scene::Container(root) = painted else {
            panic!("the view paints a container");
        };
        root.children
            .iter()
            .find_map(|c| match c {
                Scene::Text(t) if t.tag.as_deref() == Some(LAYOUT_TAG) => Some(t.content.clone()),
                _ => None,
            })
            .expect("the layout readout is painted")
    }

    #[test]
    fn r1493_the_readout_stops_contradicting_the_strip_above_it() {
        // The measured defect, on screen rather than on the wire: under a
        // stretch header the readout printed the STORED sizes and called them
        // "sizes", directly above sections painted at the shares. A reader
        // comparing the two rows was told the header was lying.
        let mut scene = boot_scene();
        let boot = read_header_state(&scene);
        assert_eq!(
            boot.section_sizes, boot.sizes,
            "interactive: the two agree, which is why one name sufficed"
        );
        assert!(
            !layout_readout(&boot).contains("| stored "),
            "and with nothing to tell apart the readout says it once"
        );

        let stretched = after(&mut scene, |i| {
            i.intervene(
                "available_width",
                IntrospectValue::Int(i64::from(AVAILABLE_W)),
            )
            .expect("the viewport is publishable over the wire");
            i.invoke(
                "set_all_resize_modes",
                IntrospectValue::Text("stretch".into()),
            )
            .expect("set_all_resize_modes is a known action");
        });
        assert_ne!(
            stretched.section_sizes, stretched.sizes,
            "stretch: the size given and the size had have parted"
        );
        // The readout's first number is now the painted one, section by
        // section — the assertion the old readout failed.
        for p in stretched.placements() {
            assert_eq!(
                stretched.section_sizes[p.logical], p.size,
                "the readout's size for {} is the rect the strip paints",
                HEADERS[p.logical]
            );
        }
        // The binding derives the effective sizes from the placements, and the
        // wire publishes them as their own slot. Two producers of one fact, so
        // assert they are the same fact — the drift the encoder/decoder rule
        // exists to prevent, one layer up.
        assert_eq!(
            wire_section_sizes(&scene),
            stretched.section_sizes.to_vec(),
            "the slot an agent reads and the numbers this binding paints agree"
        );
        let readout = layout_readout(&stretched);
        assert!(
            readout.starts_with(&format!("sizes Name={}", stretched.placements()[0].size)),
            "the readout leads with what is on screen: {readout}"
        );
        assert!(
            readout.contains(&format!("| stored Name={}", stretched.sizes[0])),
            "and still shows the size a restore would replay: {readout}"
        );
    }

    #[test]
    fn r1493_the_default_resets_the_row_and_the_readout_names_it() {
        // Qt's `defaultSectionSize` / `resetDefaultSectionSize`, through the
        // wire, with the readout naming the rule it applied.
        let mut scene = boot_scene();
        assert!(
            layout_readout(&read_header_state(&scene))
                .contains(&format!("| default {DEFAULT_SECTION_SIZE} |")),
            "the boot default is painted"
        );

        let hidden_kept = after(&mut scene, |i| {
            i.invoke("set_section_hidden", IntrospectValue::Text("2:true".into()))
                .expect("set_section_hidden is a known action");
            i.intervene("default_section_size", IntrospectValue::Int(70))
                .expect("Qt's setDefaultSectionSize has a wire peer");
        });
        assert_eq!(hidden_kept.default_size, 70);
        assert_eq!(
            hidden_kept.sizes.to_vec(),
            vec![70, 70, SECTION_W[2], 70, 70],
            "every shown section took the default; the hidden one kept its size"
        );
        assert!(
            layout_readout(&hidden_kept).contains("| default 70 |"),
            "and the readout names the rule that moved them"
        );

        let reset = after(&mut scene, |i| {
            i.invoke("reset_default_section_size", IntrospectValue::Null)
                .expect("reset is a known action");
        });
        assert_eq!(reset.default_size, DEFAULT_SECTION_SIZE);
        assert_eq!(
            reset.sizes.to_vec(),
            vec![
                DEFAULT_SECTION_SIZE,
                DEFAULT_SECTION_SIZE,
                SECTION_W[2],
                DEFAULT_SECTION_SIZE,
                DEFAULT_SECTION_SIZE
            ],
            "the reset reaches the constant without the caller naming it"
        );
    }

    // ----- R1494: the keyboard resize is the user's resize -----

    #[test]
    fn r1494_the_same_keystroke_reads_the_rule_the_readout_names() {
        // The forcing consumer: `[` / `]` is this binding's interactive
        // resize (it has no pointer grabber), so it is the gesture Qt's
        // property governs. The same keystroke must do two visibly different
        // things depending on a rule the user can see.
        let mut scene = boot_scene();
        let boot = read_header_state(&scene);
        assert!(!boot.rules.cascading, "off by default, as in Qt");
        assert!(
            layout_readout(&boot).contains("| cascade off |"),
            "and the readout says so: {}",
            layout_readout(&boot)
        );

        // Cursor onto section 0, then grow it.
        let plain = after(&mut scene, |i| {
            i.intervene("focused_index", IntrospectValue::Int(0))
                .expect("the cursor is placeable");
        });
        assert_eq!(plain.focused, Some(0));
        let grown = after(&mut scene, |i| {
            assert!(nudge_size_at(i, Some(0), true), "the grow gesture ran");
        });
        assert_eq!(
            grown.sizes.to_vec(),
            vec![
                SECTION_W[0] + RESIZE_STEP,
                SECTION_W[1],
                SECTION_W[2],
                SECTION_W[3],
                SECTION_W[4]
            ],
            "cascading off: nobody else paid"
        );
        let widened_total: u32 = grown.sizes.iter().sum();

        // Same keystroke, rule on.
        let mut scene = boot_scene();
        let on = after(&mut scene, |i| {
            i.intervene("cascading_section_resizes", IntrospectValue::Bool(true))
                .expect("Qt's property has a wire peer");
        });
        assert!(on.rules.cascading);
        assert!(
            layout_readout(&on).contains("| cascade on |"),
            "the readout names the rule that is now in force: {}",
            layout_readout(&on)
        );
        let cascaded = after(&mut scene, |i| {
            assert!(nudge_size_at(i, Some(0), true), "the same gesture ran");
        });
        assert_eq!(
            cascaded.sizes[0],
            SECTION_W[0] + RESIZE_STEP,
            "the anchor grew by the same step"
        );
        assert_eq!(
            cascaded.sizes[1],
            SECTION_W[1] - RESIZE_STEP,
            "but the follower paid for it"
        );
        let cascaded_total: u32 = cascaded.sizes.iter().sum();
        assert_eq!(
            cascaded_total,
            SECTION_W.iter().sum::<u32>(),
            "so the row is exactly as wide as it was"
        );
        assert!(
            cascaded_total < widened_total,
            "which the non-cascading gesture was not: {cascaded_total} vs {widened_total}"
        );
    }

    #[test]
    fn r1491_a_saved_layout_carries_the_sort_qt_s_savestate_carries() {
        // The gap this round closed: the snapshot claimed to be the peer of
        // `QHeaderView::saveState()` while dropping a field it carries.
        let mut ext = fresh();
        ext.invoke("cycle_sort_indicator", IntrospectValue::Int(4))
            .expect("cycle is a known action");
        ext.invoke("move_section", IntrospectValue::Text("4:0".into()))
            .expect("move_section is a known action");
        let Some(IntrospectValue::Json(saved)) = ext.query("state") else {
            panic!("state reads back as JSON");
        };
        assert_eq!(
            saved
                .get("sort_indicator")
                .and_then(serde_json::Value::as_str),
            Some("4:ascending"),
            "the readable snapshot names the sorted column"
        );

        let mut restored = fresh();
        restored
            .intervene("state", IntrospectValue::Json(saved))
            .expect("the snapshot restores whole");
        assert_eq!(
            restored.query("sort_indicator"),
            Some(IntrospectValue::Text("4:ascending".into()))
        );
        assert_eq!(
            restored.query("labels"),
            Some(IntrospectValue::Json(serde_json::json!([
                "Owner", "Name", "Type", "Size", "Modified"
            ]))),
            "along with the order it was saved with"
        );
    }

    /// Publish the viewport the view fn publishes every frame, so a test that
    /// asks about a filled row is asking about a row that has something to
    /// fill. (R1452's own stretch test says this inline; three tests below
    /// need it too.)
    fn publish_viewport(i: &mut dyn ExternalIntrospect) {
        i.intervene(
            "available_width",
            IntrospectValue::Int(i64::from(AVAILABLE_W)),
        )
        .expect("publish the viewport");
    }

    #[test]
    fn r1498_the_f_key_fills_the_row_and_the_readout_names_the_rule() {
        let mut scene = boot_scene();
        let boot = after(&mut scene, publish_viewport);
        assert!(!boot.rules.stretch_last, "off by default, as in Qt");
        assert!(
            layout_readout(&boot).contains("| stretch-last off |"),
            "and the readout says so: {}",
            layout_readout(&boot)
        );
        assert_eq!(
            boot.total_w(),
            SECTION_W.iter().sum::<u32>(),
            "the row is as wide as its sections, not as wide as the strip"
        );
        assert!(
            boot.total_w() < AVAILABLE_W,
            "leaving {} px of the strip unpainted",
            AVAILABLE_W - boot.total_w()
        );

        let on = after(&mut scene, |i| {
            assert!(toggle_stretch_last(i), "the f gesture ran");
        });
        assert!(on.rules.stretch_last);
        assert!(
            layout_readout(&on).contains("| stretch-last on |"),
            "the readout names the rule now in force: {}",
            layout_readout(&on)
        );
        assert_eq!(on.total_w(), AVAILABLE_W, "and the row fills the strip");
        assert_eq!(
            on.section_sizes[4],
            SECTION_W[4] + (AVAILABLE_W - SECTION_W.iter().sum::<u32>()),
            "the last section took exactly what was left over"
        );
        assert_eq!(
            on.sizes, SECTION_W,
            "and no stored width moved, so f is its own undo"
        );

        let off = after(&mut scene, |i| {
            assert!(toggle_stretch_last(i), "and toggles back");
        });
        assert!(!off.rules.stretch_last);
        assert_eq!(off.section_sizes, SECTION_W);
    }

    #[test]
    fn r1498_the_readout_names_the_mode_the_header_applies() {
        // The R1493 rule in the vocabulary next door: the row above says the
        // last section is 170 wide, so the row below must not call it
        // `interactive` and leave the reader to guess where 170 came from.
        let mut scene = boot_scene();
        let plain = after(&mut scene, publish_viewport);
        assert!(
            layout_readout(&plain).contains("| modes iiiii |"),
            "{}",
            layout_readout(&plain)
        );
        assert!(
            !layout_readout(&plain).contains("stored modes"),
            "with nothing overridden there is no second row to show"
        );

        let on = after(&mut scene, |i| {
            assert!(toggle_stretch_last(i));
        });
        assert_eq!(
            on.effective_modes[4],
            SectionResizeMode::Stretch,
            "the header is stretching the last section"
        );
        assert_eq!(
            on.modes[4],
            SectionResizeMode::Interactive,
            "while the mode set on it is untouched"
        );
        let readout = layout_readout(&on);
        assert!(
            readout.contains("| modes iiiis | stored modes iiiii |"),
            "so the readout names both, applied first: {readout}"
        );
    }

    #[test]
    fn r1498_the_resize_key_refuses_the_section_the_rule_is_filling() {
        // The R1494 split, in a new place: the gesture is gated on the mode the
        // header APPLIES, or it would accept the keystroke and paint nothing.
        let mut scene = boot_scene();
        let on = after(&mut scene, |i| {
            publish_viewport(i);
            assert!(toggle_stretch_last(i));
            i.intervene("focused_index", IntrospectValue::Int(4))
                .expect("cursor onto the last section");
        });
        assert_eq!(on.focused, Some(4));
        assert!(!press(&mut scene, "]"), "the key is refused");
        assert_eq!(
            read_header_state(&scene).sizes,
            SECTION_W,
            "and nothing moved behind the refusal"
        );

        // The section BEFORE it is still resizable, and the fill absorbs it.
        let grown = after(&mut scene, |i| {
            assert!(nudge_size_at(i, Some(3), true), "its neighbour still is");
        });
        assert_eq!(grown.sizes[3], SECTION_W[3] + RESIZE_STEP);
        assert_eq!(
            grown.total_w(),
            AVAILABLE_W,
            "the row still fills, because the last section gave the space up"
        );
        assert_eq!(
            grown.section_sizes[4],
            on.section_sizes[4] - RESIZE_STEP,
            "which is where it came from"
        );
    }

    #[test]
    fn r1498_the_mode_cycle_still_reads_the_mode_that_was_set() {
        // `m` advances the SET mode. Reading the override there would let one
        // keypress turn a filled `Interactive` section into `ResizeToContents`,
        // skipping `Fixed` and `Stretch` — the cycle would depend on where the
        // section happens to be sitting.
        let mut scene = boot_scene();
        after(&mut scene, |i| {
            publish_viewport(i);
            assert!(toggle_stretch_last(i));
        });
        let cycled = after(&mut scene, |i| {
            assert!(cycle_mode_at(i, Some(4)), "the m gesture ran");
        });
        assert_eq!(
            cycled.modes[4],
            SectionResizeMode::Fixed,
            "one step from Interactive, not one step from Stretch"
        );
        assert_eq!(
            cycled.effective_modes[4],
            SectionResizeMode::Stretch,
            "and the rule still overrides whatever it was set to"
        );
        assert_eq!(cycled.total_w(), AVAILABLE_W, "so the row still fills");
    }
}
