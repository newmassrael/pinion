//! `hello-grid-hscroll` — R784 §5.45 **horizontal scroll + frozen header**.
//!
//! A 10,000-row Material-3 data-grid whose **8 columns are wider than the
//! window**, so the grid scrolls *horizontally* as well as vertically. The
//! header band and the body share one outer
//! [`ScrollAxis::Horizontal`](pinion_core::scene::ScrollAxis) scroll, so the
//! header tracks the body's horizontal offset exactly — it can never drift
//! out of column alignment — while staying **vertically pinned** above the
//! inner vertical body scroll (the R784 nested single-axis composition:
//! outer horizontal over `[frozen header, inner vertical body]`).
//!
//! This is the consumer that motivates the R784 layout substrate: before
//! R784 the scroll layout pass only unbounded the *height* axis, so scroll
//! content could never overflow its viewport width and `max_x` was always
//! 0. `ScrollAxis::Horizontal` makes the pass unbound the *width* axis so a
//! `NCOLS × COL_W`-wide column overflows a narrower window and the layout
//! pass writes the overflow into the outer scroll's `max_x`.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/set_scroll_offset` on the outer horizontal scroll's tag
//! (`ghs_hscroll`) slides the whole column; `scene/snapshot` then reports
//! the header cells AND the body cells shifted left by the same amount
//! (frozen-header horizontal sync), while their vertical positions are
//! unchanged (the header is pinned). Vertical `scene/set_scroll_offset` on
//! the body scroll (`ghs_scroll`) slides the rows but never the header. No
//! pixels required (see `tools/r784_grid_hscroll.py`).
//!
//! ## a11y (WAI-ARIA virtualized grid)
//!
//! Identical topology to `hello-virtual-table` — one `AriaRole::Grid` over
//! the header `row` + the windowed data `row`s (`windowed_grid_nodes`); the
//! horizontal scroll is a viewport concern, orthogonal to the row/column
//! structure the a11y tree conveys.

use std::rc::Rc;

use pinion_a11y::{AccessNode, WidgetA11y, windowed_grid_nodes};
use pinion_core::command::Command;
use pinion_core::external::{External, IntrospectValue, StubExternal};
use pinion_core::intent::Intent;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, FlexDirection, LayoutStyle, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::column_widths::{
    ColumnWidthExternal, column_resize_externals, use_column_widths,
};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::table::{
    CellIndex, GridScroll, TableStyle, VirtualTableData, view_virtual_table,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGridHscrollRenderer, HelloGridHscrollRendererError);

/// Initial window size. Deliberately narrower than `NCOLS × COL_W` so the
/// columns overflow and the grid scrolls horizontally.
const WIN_W: u32 = 520;
const WIN_H: u32 = 440;
/// Shared [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key.
const THEME_TAG: &str = "app";
/// Total data-row count — large while the rendered node count stays small.
const N: usize = 10_000;
/// Column count (matches `HEADERS.len()`).
const NCOLS: usize = 8;
/// Uniform column width; `NCOLS × COL_W = 1040 > WIN_W` so the grid scrolls
/// horizontally and the frozen header tracks the body's horizontal offset.
const COL_W: u32 = 130;
/// Data-row height (must match the windowing pitch used in `access_node`).
const ROW_H: u32 = 36;
/// Rows built beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Column header labels — a process-table-style wide row.
const HEADERS: [&str; NCOLS] = [
    "PID", "Name", "CPU%", "Memory", "Threads", "Status", "User", "Started",
];
/// Paint-root + a11y `grid` tag, and the [`StubExternal`] anchor tag.
const TABLE_TAG: &str = "ghs";
/// Cache key (and input-router tag) for the vertical body `ScrollState`.
const SCROLL_KEY: &str = "ghs_scroll";
/// R784 — cache key (and input-router tag) for the outer horizontal
/// `ScrollState`. `scene/set_scroll_offset` on this tag slides the grid
/// sideways; the header and body move together.
const H_SCROLL_KEY: &str = "ghs_hscroll";
/// R785 — cache key for the shared `ColumnWidths` model, and the tag of
/// the [`ColumnWidthExternal`] that drives it. `invoke set_col_width` on this
/// tag resizes a column; widening past the window grows the R784 horizontal
/// scroll extent.
const COLS_KEY: &str = "ghs_cols";

/// R1347 §5.20 — the per-column resize handles are tagged `ghs_ch<col>`, so a
/// drag-end commit reaches the reducer as `ghs_ch<col>.width_committed` (the
/// §5.20 R22 tag prefix applied to the widget's `"width_committed"` event). The
/// reducer recovers the column by stripping the `<table>_ch` prefix and this
/// suffix, then parsing what remains (`column_of_width_commit`).
const WIDTH_COMMITTED_SUFFIX: &str = ".width_committed";
/// R1347 §2 #7 — paint tag of the committed-width witness row, read as
/// scene-as-data by `tools/demos/r1347_column_width_commit.py`.
const WIDTH_COMMIT_LOG_TAG: &str = "ghs_width_commit_log";

/// R1347 §5.20 — the **committed** column-resize log: `(commit_count, col,
/// width)` of the most recent settle.
///
/// The dogfood consumer for `ColumnResizeExternal`'s new `"width_committed"`
/// channel — the state a real grid persists (an IDE writes column widths to a
/// layout file; sprag mirrors them to its host). Deliberately a SEPARATE handle
/// from the live [`use_column_widths`] model the drag writes at pointer rate:
/// this one moves only on the settle edge, so the pair demonstrates pinion's
/// two-channel drag contract for the column-resize widget exactly as
/// `hello-dock-panels` does for the splitter. The count makes the contract
/// falsifiable: a click commits zero times, a drag exactly once.
fn use_committed_width_log() -> Rc<Signal<(u32, usize, u32)>> {
    Owner::current()
        .expect("hello-grid-hscroll: view fn runs inside the substrate root owner scope")
        .cache("ghs_committed_width_log", || Signal::new((0, 0, 0)))
}

/// R1347 — parse the column index out of a `ghs_ch<col>.width_committed` intent
/// tag. Returns `None` for any other tag (the reducer ignores it). Keyed on the
/// `<table>_ch` prefix + the `.width_committed` suffix so a future intent on the
/// same widget (or a different table tag) cannot be misread as a width commit.
fn column_of_width_commit(tag: &str) -> Option<usize> {
    let prefix = concat_ch_prefix();
    let rest = tag.strip_prefix(&prefix)?;
    let col_str = rest.strip_suffix(WIDTH_COMMITTED_SUFFIX)?;
    col_str.parse::<usize>().ok()
}

/// The `"ghs_ch"` prefix every per-column resize tag shares (`TABLE_TAG` +
/// `_ch`). One place so the reducer's parse and the paint-side tag cannot drift.
fn concat_ch_prefix() -> String {
    format!("{TABLE_TAG}_ch")
}

// Display-only grid apart from the R1347 committed-width log: repaints are
// driven by the theme + scroll-offset + measured-viewport reactive `Signal`
// subscriptions the view opens, plus the committed-log signal.

fn table_style() -> TableStyle {
    TableStyle {
        col_width: COL_W,
        row_height: ROW_H,
        ..TableStyle::m3()
    }
}

/// Synthetic cell texts for a data row across all 8 columns. The
/// five-digit PID keeps every `scene/snapshot` cell unambiguous; the
/// other columns cycle so the eye (and the diff) has something to track
/// while scrolling either axis.
fn cell_text(c: CellIndex) -> String {
    const NAMES: [&str; 5] = ["alpha", "bravo", "charlie", "delta", "echo"];
    const STATUS: [&str; 3] = ["Idle", "Active", "Done"];
    const USERS: [&str; 4] = ["root", "daemon", "coin", "ai"];
    let id = c.row;
    match c.col {
        0 => format!("{id:05}"),
        1 => format!("{}-{id}", NAMES[id % NAMES.len()]),
        2 => format!("{}.{}", id % 100, id % 10),
        3 => format!("{} MB", 16 + (id % 480)),
        4 => format!("{}", 1 + (id % 32)),
        5 => STATUS[id % STATUS.len()].to_string(),
        6 => USERS[id % USERS.len()].to_string(),
        _ => format!("T-{:04}", id % 1440),
    }
}

/// view-fn (§6.3): pure sync `() -> Scene`. The dataset is virtual —
/// `view_virtual_table` invokes [`cell_text`] only for the cells in the
/// current vertical window; the horizontal axis is a viewport offset (no
/// column virtualization at 8 columns).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
    // R785 — the shared per-column widths, seeded uniform so the boot layout
    // matches the R784 slice; `set_col_width` (via the external) widens a
    // column and this view re-runs with the new widths.
    let widths = use_column_widths(COLS_KEY, || vec![COL_W; NCOLS]);
    let col_widths = widths.widths();
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = table_style();

    let grid = view_virtual_table(
        TABLE_TAG,
        GridScroll {
            body: &scroll,
            horizontal: &h_scroll,
        },
        VirtualTableData {
            headers: &HEADERS,
            item_count: N,
            overscan: OVERSCAN,
            sort: None,
            sort_tag: None,
            order: None,
            col_widths: Some(&col_widths),
            // R786 — drag a column border to resize it live (the grabber is the
            // header cell's trailing edge); widening past the window grows the
            // R784 horizontal scroll. Driven by the per-column
            // `ColumnResizeExternal`s registered in `create_extra_externals`.
            resizable: true,
            // R859 — no frozen columns here (this is the live h-scroll demo).
            frozen_cols: 0,
            row_style: None,
        },
        &theme,
        &style,
        |_| false, // display-only grid: no selection
        cell_text,
    );

    // R1347 §5.20 §2 #7 — the committed-width witness, painted below the grid.
    // Reads the reducer's settle-log signal as scene-as-data; a demo observes
    // it over RPC to prove the drag-end commit completed the round trip through
    // `V::update` (not merely that the External queued something).
    let (commits, col, width) = use_committed_width_log().get();
    let witness = Scene::Text(
        TextNode::styled(
            format!("committed width: {commits} commits, last col {col} = {width}px"),
            Rect::default(),
            TextStyle::new().with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(WIDTH_COMMIT_LOG_TAG),
    );

    Scene::Container(
        ContainerNode::new(vec![grid, witness])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

struct GridHscrollView;

impl WidgetCore for GridHscrollView {
    type State = ();
    type Event = ();

    /// Display-only: the only addressable anchor is the no-op
    /// [`StubExternal`] at [`TABLE_TAG`] (input router + a11y `grid`
    /// bounds). Wheel input routes to the body / horizontal `ScrollNode`s
    /// via their `ScrollState`s, no extra External needed.
    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    /// R785 / R786 — the column-width axis surfaces, all over the **same**
    /// shared `ColumnWidths` the view reads via [`use_column_widths`]:
    ///
    /// * The [`ColumnWidthExternal`] at [`COLS_KEY`] — the AI-first RPC path
    ///   (`invoke set_col_width "<col>=<width>"` widens a column).
    /// * One [`ColumnResizeExternal`](pinion_core::widgets::column_widths::ColumnResizeExternal)
    ///   per column (via
    ///   [`column_resize_externals`]) registered at the header-cell tags
    ///   (`ghs_ch<col>`) — the **live-drag** path: grabbing a header border and
    ///   dragging it widens that column. Both mutate the one width `Signal`, so
    ///   a drag and an RPC `set_col_width` agree; the new total content width
    ///   grows the R784 horizontal scroll.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let widths = use_column_widths(COLS_KEY, || vec![COL_W; NCOLS]);
        // The resize handles normalize their pixel drag against the horizontal
        // scroll viewport (a stable width — the dragged cell resizes, the
        // viewport does not), so they share the grid's h-scroll state + tag.
        let h_scroll = use_scroll_state(H_SCROLL_KEY);
        let mut externals = vec![ExtraExternal::new(
            COLS_KEY,
            Box::new(ColumnWidthExternal::new(Rc::clone(&widths))),
        )];
        externals.extend(column_resize_externals(
            TABLE_TAG,
            &widths,
            &h_scroll,
            H_SCROLL_KEY,
        ));
        externals
    }

    fn tag() -> &'static str {
        TABLE_TAG
    }

    fn read_state(_scene: &Scene) {}

    /// R1347 §5.20 — persist the settled column width. This is the
    /// `onChangeEnd` arm a real grid writes from: it fires once per column-drag
    /// that actually changed a width, never during the drag and never on a
    /// click on the grabber. There is deliberately no live-`width_changing`
    /// peer — the live width is read straight off the shared `ColumnWidths`
    /// model by the view; the intent channel carries only the settle edge worth
    /// persisting.
    fn update(_state: (), intent: &Intent) -> Vec<Command> {
        if let Some(col) = column_of_width_commit(intent.tag_str())
            && let IntrospectValue::Int(width) = intent.payload
        {
            let log = use_committed_width_log();
            let (count, _, _) = log.get();
            let width32 = u32::try_from(width).unwrap_or(0);
            log.set((count + 1, col, width32));
        }
        Vec::new()
    }

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-grid-hscroll (R784 §5.45 horizontal scroll + frozen header)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (no widget state)".to_string()
    }
}

impl WidgetA11y for GridHscrollView {
    /// WAI-ARIA virtualized `grid` — same topology as `hello-virtual-table`
    /// (the horizontal scroll is a viewport concern, orthogonal to the
    /// row/column structure). Built by the shared
    /// `pinion_a11y::windowed_grid_nodes` so the virtualized-grid topology
    /// stays one source of truth.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_H, OVERSCAN);
        windowed_grid_nodes(
            TABLE_TAG,
            "Horizontally scrolling data grid",
            &HEADERS,
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
        )
    }
}

impl WidgetView for GridHscrollView {
    type Renderer = HelloGridHscrollRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GridHscrollView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::Owner;

    #[test]
    fn columns_overflow_the_window() {
        // The premise of this example: the column total exceeds the window
        // width, so the grid genuinely scrolls horizontally.
        let total_w = u32::try_from(NCOLS).unwrap() * COL_W;
        assert!(
            total_w > WIN_W,
            "NCOLS×COL_W ({total_w}) must exceed WIN_W ({WIN_W})",
        );
    }

    /// R1524 — every column answers, and answers distinctly.
    ///
    /// The per-cell equivalent of the pre-R1524 `cells.len() == NCOLS` check,
    /// and a strictly stronger one: a correct length said nothing about whether
    /// two columns returned the *same* text, which is exactly how a `match`
    /// arm that falls through to its neighbour fails.
    #[test]
    fn every_column_answers_distinctly() {
        let texts: Vec<String> = (0..NCOLS)
            .map(|col| cell_text(CellIndex { row: 42, col }))
            .collect();
        assert!(
            texts.iter().all(|t| !t.is_empty()),
            "every column of a filled row has text: {texts:?}",
        );
        let distinct: std::collections::BTreeSet<&String> = texts.iter().collect();
        assert_eq!(
            distinct.len(),
            NCOLS,
            "each column reports its own value, not a neighbour's: {texts:?}",
        );
        assert_eq!(texts[0], "00042", "PID is the zero-padded index");
    }

    /// R1347 §5.20 R22 — the reducer's `width_committed` parse must recover the
    /// column from the tag the `ColumnResizeExternal`s are ACTUALLY registered
    /// under. That tag is `GridTag::col_header(TABLE_TAG, col)`
    /// (`column_resize_externals` builds it), so the test constructs the wire
    /// form through that same function — NOT a hand-spelled `_ch` literal —
    /// then applies the §5.20 R22 suffix the walk appends. This way a drift in
    /// `col_header`'s spelling breaks the test, which a hard-coded literal
    /// would silently survive while the runtime coupling broke.
    /// R1349 — the suffix this binding matches on is pinion's word, not a
    /// lookalike. Without this the whole `column_of_width_commit` suite is
    /// self-referential: it builds its wire strings from `WIDTH_COMMITTED_SUFFIX`
    /// and then asserts they parse, so it stays green if pinion renames the
    /// event and every real commit stops matching.
    #[test]
    fn r1349_width_committed_suffix_tracks_the_upstream_word() {
        assert_eq!(
            WIDTH_COMMITTED_SUFFIX,
            format!(".{}", pinion_core::widgets::commit::WIDTH_COMMITTED_EVENT),
        );
    }

    #[test]
    fn r1347_width_commit_tag_round_trips_the_column() {
        use pinion_core::composite_tag::GridTag;
        for col in [0usize, 3, NCOLS - 1] {
            let registered = GridTag::col_header(TABLE_TAG, col);
            let wire = format!("{registered}{WIDTH_COMMITTED_SUFFIX}");
            assert_eq!(
                column_of_width_commit(&wire),
                Some(col),
                "reducer must recover column {col} from the registered tag {wire:?}",
            );
        }
        // Foreign tags must not be misread as a width commit.
        assert_eq!(column_of_width_commit("ghs_ch2.dragging"), None);
        assert_eq!(column_of_width_commit("other_ch2.width_committed"), None);
        assert_eq!(column_of_width_commit("ghs_scroll"), None);
    }

    #[test]
    fn grid_a11y_setsize_is_full_dataset() {
        let nodes = Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 360);
            GridHscrollView::access_node(&(), None)
        });
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert_eq!(
            nodes[0].size_of_set,
            Some(u32::try_from(N).unwrap()),
            "grid setsize conveys the FULL 10_000-row dataset",
        );
        let columnheaders = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .count();
        assert_eq!(columnheaders, NCOLS, "one columnheader per column");
    }
}
