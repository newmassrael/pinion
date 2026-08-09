// R986 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the architectural narrative carries many proper-noun
// identifiers (ContextMenu, GridSortExternal, WAI-ARIA, …).
#![allow(clippy::doc_markdown)]

//! `hello-grid-header-menu` — R986 §5.27 §5.38 §5.53 **data-grid column-header
//! context menu**.
//!
//! ## Why this binding exists
//!
//! Phase B DCC-widget axis: the editor / IDE / spreadsheet column menu every
//! data grid ships, where a **right-click on a column header — or, R988, any
//! data cell in that column** — opens a command popup scoped to the column
//! (Sort Ascending / Sort Descending / Clear Sort). R988 also names the column
//! in the labels ("Sort by Score ascending"), so the menu reflects what it
//! acts on. R965.1 deferred exactly this ("a header / context-menu control") as
//! a follow-up; this binding lands it as **pure composition** of the
//! substrates already in tree — no framework change:
//!
//! * **the popup** — the R772 [`ContextMenuExternal`] (the *primary* external
//!   here, so the WAI-ARIA §3.5 keyboard model and the R715 click-outside
//!   dismiss barrier come for free, exactly as in `hello-contextmenu`), drawn
//!   by pinion's own scene-graph renderer (R771.1 §5.53).
//! * **the right-click arc** — the R887 secondary-click wire
//!   ([`WidgetCore::apply_secondary_click`]); a human right-click and
//!   `scene/click {button: "right"}` reach the same override (§2 invariant
//!   #2). The override maps the press point to a column via the header
//!   geometry ([`grid_target_at`] — the toolkit's `logicalIndexAt`), records
//!   the target column, and anchors the popup.
//! * **the sort** — the R778 [`GridSortExternal`] over a shared
//!   [`GridSortState`] (an *extra* external): a left-click on a header still
//!   cycles its sort, and `query("sort")` / `query("source_at.<pos>")` expose
//!   the order to an AI client. The context-menu command drives the **same**
//!   [`GridSortState::set_sort`] through the reducer, so both interaction
//!   paths converge on one model.
//!
//! ## The captured-target pattern
//!
//! A real header menu acts on the column the cursor landed on, captured at
//! open time (the toolkit's `exec` / the web `contextmenu` `event.target`). So the popup itself stays
//! target-agnostic; the binding stores the right-clicked column in a reactive
//! [`Signal`] ([`use_menu_target`]) at `apply_secondary_click` time and reads it back in [`update`](GridHeaderMenuView)
//! when the `"command"` intent fires. The status bar surfaces the captured column as
//! scene-as-data (§2 invariant #7) so an AI client sees the menu's target
//! before activating an item.
//!
//! ## AI clients
//!
//! `scene/click {button: "right", at: {x, y}}` over a header opens the popup
//! (`query("open")` / `open_x` / `open_y`); `invoke("send", "i<i>:PointerUp")`
//! (or a left `scene/click` on `colmenu#i<i>`) activates an item, applying its
//! sort to the captured column — observable through `/gsort/external/sort` and
//! `/gsort/external/source_at.<pos>`. The keyboard model (`invoke("key", …)`)
//! and the dismiss barrier (`colmenu#barrier`) mirror `hello-contextmenu`.

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, MenuItemCell, WidgetA11y, menu_item_nodes,
    windowed_grid_nodes_sorted,
};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::intent::Intent;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::context_menu::{ContextMenuExternal, read_open_state};
use pinion_core::widgets::grid_sort::{
    GridSortExternal, GridSortState, grid_sort_str, use_grid_sort,
};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Command, Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::barrier::dismiss_barrier;
use pinion_widget_paint::coord::saturating_f32_to_u32;
use pinion_widget_paint::menu::{
    ContextMenuPlacement, MenuStyle, composite_item_tag, parse_item_sub_tag, view_context_menu,
};
use pinion_widget_paint::table::{
    CellIndex, GridModel, GridScroll, HeaderAxis, TableStyle, VirtualTableData, header_from_slice,
    materialize_cells, no_decoration, no_edit, no_row_header, view_virtual_table,
};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(
    HelloGridHeaderMenuRenderer,
    HelloGridHeaderMenuRendererError
);

/// Window size: fixed, wide enough that `NCOLS * COL_W` fits without an
/// h-scroll and tall enough that all `N` rows + the popup are visible.
const WIN_W: u32 = 520;
const WIN_H: u32 = 460;
const THEME_TAG: &str = "app";
/// Row count — small so the whole grid paints without scrolling.
const N: usize = 8;
/// Column count (matches `HEADERS.len()`).
const NCOLS: usize = 3;
/// Uniform column width; `NCOLS * COL_W = 450 < WIN_W` so the columns start at
/// window x-origin 0 with no horizontal scroll (the [`grid_target_at`] mapping
/// relies on this).
const COL_W: u32 = 150;
/// Data-row height.
const ROW_H: u32 = 36;
/// Rows built beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Status-bar height above the grid: the header band therefore occupies
/// `[STATUS_H, STATUS_H + header_height)` in window space.
const STATUS_H: u32 = 40;
/// Column header labels.
const HEADERS: [&str; NCOLS] = ["Name", "Score", "Status"];
/// Paint-root + a11y `grid` tag; header cells paint as `"grid_ch<col>"`.
const GRID_TAG: &str = "grid";
/// The [`GridSortExternal`] anchor + the `use_grid_sort` cache key: left-clicked
/// headers (`gsort#h<col>`) route here, and `/gsort/external/sort` exposes the
/// order to AI clients.
const SORT_TAG: &str = "gsort";
/// The [`ContextMenuExternal`] scope tag — the *primary* external, the painted
/// popup panel, and the snapshot anchor share it; item rows paint as the R51.42
/// composite `colmenu#i<i>`.
const MENU_TAG: &str = "colmenu";
/// R715 §5.16 — the transparent click-outside dismiss barrier composite.
const BARRIER_TAG: &str = "colmenu#barrier";
/// The popup command count — derived from [`SORT_COMMANDS`] so the item count,
/// labels, and actions share one source.
const MENU_ITEM_COUNT: usize = SORT_COMMANDS.len();
/// The `"command"` intent the [`ContextMenuExternal`] emits on activation,
/// dotted with this primary external's scope tag (R51.173 §5.23). Built via
/// `pinion_core::intent_tag!`; the literal must match [`MENU_TAG`].
const COMMAND_INTENT_TAG_FULL: &str = pinion_core::intent_tag!("colmenu", "command");
const SCROLL_KEY: &str = "grid_scroll";
const H_SCROLL_KEY: &str = "grid_hscroll";
const STATUS_TAG: &str = "grid_status";
/// The reactive holder cache key for the captured header column (the
/// [`use_menu_target`] hook).
const HEADER_CTX_KEY: &str = "grid_header_menu.target_col";

const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
const STATUS: [&str; 3] = ["Idle", "Active", "Done"];

fn table_style() -> TableStyle {
    TableStyle {
        col_width: COL_W,
        row_height: ROW_H,
        ..TableStyle::m3()
    }
}

/// The Score column value for data row `id`: a non-monotonic pseudo-random
/// number with varying digit-length, so a numeric-aware sort visibly differs
/// from a lexicographic one. The RPC demo replicates this formula.
fn score(id: usize) -> usize {
    (id * 37 + 11) % 100
}

/// The synthetic dataset, one cell at a time — the **seed** for the model
/// below, and nothing else.
///
/// R1525 — until this round the grid was ALSO painted from this function, while
/// its sort / filter / search were computed from the model's materialized cells.
/// Two paths to the same data, and the one the user reads was not the one the
/// ordering came from. R1524's `materialize_cells` made them agree by deriving
/// one from the other; this round removes the second path instead. The model is
/// the store, this is the generator, and the view asks the store.
fn cell_text(c: CellIndex) -> String {
    let id = c.row;
    match c.col {
        0 => format!("{}{id}", CATEGORIES[id % CATEGORIES.len()]),
        1 => score(id).to_string(),
        _ => STATUS[id % STATUS.len()].to_string(),
    }
}

/// The shared [`GridSortState`] — the single source of truth for the grid's
/// column sort order. The view, the a11y tree, the [`GridSortExternal`], and
/// the context-menu reducer all reach the same `Rc` through this hook, so the
/// order is computed once (the `hello-grid-sort` pattern).
fn use_grid_data() -> Rc<GridSortState> {
    use_grid_sort(SORT_TAG, || (NCOLS, materialize_cells(N, NCOLS, cell_text)))
}

/// R988 — the grid position the context menu was opened on: the column it
/// sorts, plus the row when the click landed on a body cell (`None` for a
/// header click). Captured at right-click time and read back in the view
/// (dynamic labels + status) and the reducer (the sort).
#[derive(Copy, Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
struct MenuTarget {
    /// The column the menu sorts (resolved from the click x, header or cell).
    col: usize,
    /// The body row when the click hit a data cell; `None` for a header click.
    row: Option<usize>,
}

/// The captured [`MenuTarget`] — a reactive holder (not projected widget state)
/// because the secondary-click override mutates it outside the dispatch
/// reducer; the view subscribes to surface it as scene-as-data in the status
/// bar and to name the column in the menu labels.
fn use_menu_target() -> Rc<Signal<Option<MenuTarget>>> {
    let owner = Owner::current().expect("use_menu_target requires an active Owner scope");
    owner.cache(HEADER_CTX_KEY, || Signal::new(None))
}

/// R988.1 — the single source for the menu's items: each variant owns BOTH its
/// label (named for the right-clicked column) AND its sort action, so the label
/// order (the view) and the action order (the reducer) cannot silently desync.
/// The items paint and dispatch in [`SORT_COMMANDS`] order, and
/// [`MENU_ITEM_COUNT`] derives from it.
#[derive(Copy, Clone)]
enum SortCommand {
    Ascending,
    Descending,
    Clear,
}

/// The menu's items, in paint / dispatch order.
const SORT_COMMANDS: [SortCommand; 3] = [
    SortCommand::Ascending,
    SortCommand::Descending,
    SortCommand::Clear,
];

impl SortCommand {
    /// The popup label for this command, naming the right-clicked column.
    fn label(self, col_name: &str) -> String {
        match self {
            SortCommand::Ascending => format!("Sort by {col_name} ascending"),
            SortCommand::Descending => format!("Sort by {col_name} descending"),
            SortCommand::Clear => "Clear Sort".to_string(),
        }
    }

    /// Apply this command's sort to `col` on the shared [`GridSortState`].
    fn apply(self, grid_sort: &GridSortState, col: usize) {
        match self {
            SortCommand::Ascending => grid_sort.set_sort(Some((col, true))),
            SortCommand::Descending => grid_sort.set_sort(Some((col, false))),
            SortCommand::Clear => grid_sort.set_sort(None),
        }
    }
}

/// R986 §5.53 / R988 / R988.1 — map a window-space secondary-click point to the
/// grid column it landed on (a header cell OR a data cell), or `None` when the
/// click is outside the grid's columns/rows.
///
/// The geometric hit-test (the toolkit's `logicalIndexAt`, extended to the body). `view_virtual_table`
/// insets its content by `block_pad` (the `TableStyle` SSOT the paint reads), so in window
/// coords the columns start at `x = block_pad` and the frozen header band at `y = STATUS_H + block_pad`, with the
/// `N` data rows (`ROW_H` each) below it. **R988.1** fixes the original hit-test,
/// which omitted `block_pad` and mis-resolved the column within 8 px of every
/// boundary; it now reads the same `block_pad` the paint uses. The uniform `COL_W`
/// columns and the fixed `N`-row body fit the window (`NCOLS * COL_W < WIN_W`, grid height <
/// `WIN_H`), so both `ScrollState`s clamp to offset 0 and the hit-test needs no scroll
/// term — a scrolling grid would subtract the offsets (the editable-grid
/// follow-up). A header click resolves only its column (`row = None`); a body click
/// also resolves its row. The RPC demo validates the boundaries (not just
/// centers) against the rects.
fn grid_target_at(x: f32, y: f32) -> Option<MenuTarget> {
    let xi = saturating_f32_to_u32(x);
    let yi = saturating_f32_to_u32(y);
    let pad = table_style().block_pad;
    let header_top = STATUS_H + pad;
    let header_bottom = header_top + table_style().header_height;
    let rows_h = u32::try_from(N).unwrap_or(u32::MAX).saturating_mul(ROW_H);
    // Left of the columns, in the pad gap above the header, or below the last row.
    if xi < pad || yi < header_top || yi >= header_bottom.saturating_add(rows_h) {
        return None;
    }
    let col = ((xi - pad) / COL_W) as usize;
    if col >= NCOLS {
        return None;
    }
    // `row` is `Some` only below the (frozen) header band; the y-range bounds it < N.
    let row = (yi >= header_bottom).then(|| ((yi - header_bottom) / ROW_H) as usize);
    Some(MenuTarget { col, row })
}

/// Status bar above the grid: a literal scene-as-data readout of the active
/// sort + the captured header-menu target column (so an AI client sees which
/// column the menu will act on before activating an item).
fn status_bar(theme: &Theme, sort: Option<(usize, bool)>, target: Option<MenuTarget>) -> Scene {
    let tgt = target.map_or_else(
        || "none".to_string(),
        |t| match t.row {
            Some(r) => format!("{} (cell row {r})", HEADERS[t.col]),
            None => format!("{} (header)", HEADERS[t.col]),
        },
    );
    let text = Scene::Text(
        TextNode::styled(
            format!("sort {} \u{00B7} menu target {tgt}", grid_sort_str(sort)),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(STATUS_TAG),
    );
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHigh),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::auto().with_height(SizeValue::Px(STATUS_H)))
                    .with_flex_grow(0.0)
                    .with_padding(Rect::new(12, 0, 12, 0)),
            ),
    )
}

/// view-fn (§6.3): pure sync mapping `HeaderMenuState -> Scene`. Status bar +
/// grid; when the menu is open, the R715 dismiss barrier + the floating popup
/// are placed **last** so the absolutely-positioned popup paints over both.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: HeaderMenuState, _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let tstyle = table_style();
    let mstyle = MenuStyle::m3_default();

    let grid_sort = use_grid_data();
    let sort = grid_sort.sort();
    let order = grid_sort.order();
    let target = use_menu_target().get();

    let grid = view_virtual_table(
        GRID_TAG,
        GridScroll {
            body: &scroll,
            horizontal: &h_scroll,
        },
        VirtualTableData {
            column_count: NCOLS,
            item_count: N,
            overscan: OVERSCAN,
            sort,
            sort_tag: Some(SORT_TAG),
            order: Some(order.as_slice()),
            col_widths: None,
            resizable: false,
            frozen_cols: 0,
            row_style: None,
            delegate: None,
            editing: None,
        },
        &theme,
        &tstyle,
        &|_id| false,
        GridModel {
            // R1525 — the view asks the MODEL, not the formula. See `cell_text`.
            cell: |c: CellIndex| grid_sort.cell(c.row, c.col).to_string(),
            columns: HeaderAxis::labelled(header_from_slice(&HEADERS)),
            rows: no_row_header(),
            decoration: no_decoration,
            edit: no_edit,
        },
    );

    let mut children = vec![status_bar(&theme, sort, target), grid];
    if let Some(anchor) = state.open_at {
        // R988 — the labels name the right-clicked column ("Sort by Score
        // ascending"), so the menu reflects what it acts on (the spreadsheet /
        // DCC). R988.1 — built from the `SORT_COMMANDS` SSOT, the same order the reducer
        // reads.
        let col_name = target.map_or("column", |t| HEADERS[t.col]);
        let labels = SORT_COMMANDS.map(|cmd| cmd.label(col_name));
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        children.push(dismiss_barrier(BARRIER_TAG, (0, 0), (WIN_W, WIN_H)));
        children.push(view_context_menu(
            MENU_TAG,
            MENU_TAG,
            &label_refs,
            state.active,
            ContextMenuPlacement {
                anchor,
                window: (WIN_W, WIN_H),
            },
            &theme,
            &mstyle,
        ));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

/// Cached projection of the context menu: the open anchor (window-space) and
/// the active (highlighted) item. The sort + captured column are auxiliary
/// reactive axes read in the view, not projected here.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
struct HeaderMenuState {
    /// Window-space anchor when open, `None` when closed.
    open_at: Option<(f32, f32)>,
    /// Highlighted item (WAI-ARIA active descendant), or `None`.
    active: Option<usize>,
}

struct GridHeaderMenuView;

impl WidgetCore for GridHeaderMenuView {
    type State = HeaderMenuState;
    // Every state change flows through `apply_secondary_click` (open),
    // `apply_key` (the External `"key"` wire), the InputRouter composite
    // `send` dispatch, and the `update` reducer (sort), so no keybinding
    // events flow through `event_name`. `()` keeps the `Copy` bound satisfied.
    type Event = ();

    /// Primary = the right-click command popup (R772), so the WAI-ARIA §3.5
    /// keyboard model + the dismiss barrier resolve through the proven
    /// `hello-contextmenu` focus path (the menu is the single tab stop).
    fn create_external() -> Box<dyn External> {
        Box::new(ContextMenuExternal::new(MENU_ITEM_COUNT))
    }

    /// Extra = the sort proxy over the shared [`GridSortState`] the view reads
    /// (R778): left-clicked headers (`gsort#h<col>`) cycle its sort, and its
    /// `sort` / `source_at` slots expose the order to RPC.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(
            SORT_TAG,
            Box::new(GridSortExternal::new(use_grid_data())),
        )]
    }

    fn tag() -> &'static str {
        MENU_TAG
    }

    /// Project the menu open/active state off the primary external through the
    /// shared [`read_open_state`] reader (the menu is composed as an extra-bearing
    /// scene, so it is reached via `find_external_with_tag`, not `Scene::External`).
    fn read_state(scene: &Scene) -> HeaderMenuState {
        let mut out = HeaderMenuState::default();
        let Some(node) = scene.find_external_with_tag(MENU_TAG) else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        let (open_at, active) = read_open_state(intro);
        out.open_at = open_at;
        out.active = active;
        out
    }

    fn view(state: HeaderMenuState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-grid-header-menu (R986 §5.27 §5.38 §5.53 column-header context menu)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        // The keymap SSOT is the External's `apply_key` (WAI-ARIA §3.5),
        // reached via `apply_key` below.
        None
    }

    /// R887 §5.53 / R988 — a right-click over a column header OR a data cell
    /// opens (or re-anchors) the popup at the press point and records the
    /// clicked column (and row, for a cell). A right-click outside the grid is
    /// ignored.
    fn apply_secondary_click(scene: &mut Scene, x: f32, y: f32) -> bool {
        let Some(target) = grid_target_at(x, y) else {
            return false;
        };
        use_menu_target().set(Some(target));
        let Some(node) = scene.find_external_with_tag_mut(MENU_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        matches!(
            intro.invoke("open_at", ContextMenuExternal::open_at_args(x, y)),
            Ok(IntrospectValue::Bool(true))
        )
    }

    /// Forward the focused key to the menu's WAI-ARIA §3.5 keyboard model
    /// through the R804/R834 [`forward_key_to_field`](pinion_core::input::forward_key_to_field)
    /// substrate (the generic "find the External by tag + invoke its `key`
    /// wire"): the default `forward_key_to_external` matches only a bare
    /// `Scene::External` root, but the sort proxy makes the boot scene a
    /// `Container([colmenu, gsort])`, so the menu is reached by tag — the same
    /// path `hello-menu-app` takes. The menu is one tab stop, so gate on focus
    /// first (WAI-ARIA §3.5).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(MENU_TAG) {
            return false;
        }
        pinion_core::input::forward_key_to_field(scene, MENU_TAG, key, modifiers)
    }

    /// R51.173 §5.23 — the menu's `"command"` intent applies its sort to the
    /// captured column on the shared [`GridSortState`] (a reactive side effect,
    /// mirroring `hello-toggle`'s reducer; no `Command` is emitted). The popup
    /// already closed itself on activation, so the menu state needs no edit.
    fn update(_state: HeaderMenuState, intent: &Intent) -> Vec<Command> {
        if intent.tag.as_ref() == COMMAND_INTENT_TAG_FULL {
            if let IntrospectValue::Text(idx) = &intent.payload {
                if let (Ok(item), Some(target)) = (idx.parse::<usize>(), use_menu_target().get()) {
                    // R988.1 — index the `SORT_COMMANDS` SSOT, the same order the
                    // view labels paint, so label and action cannot desync.
                    if let Some(cmd) = SORT_COMMANDS.get(item) {
                        cmd.apply(&use_grid_data(), target.col);
                    }
                }
            }
        }
        Vec::new()
    }

    fn fmt_state_log(state: &HeaderMenuState) -> String {
        match (state.open_at, state.active) {
            (Some((x, y)), Some(a)) => format!("menu open=({x},{y}) active={a}"),
            (Some((x, y)), None) => format!("menu open=({x},{y})"),
            (None, _) => "menu closed".to_string(),
        }
    }
}

impl WidgetA11y for GridHeaderMenuView {
    /// The WAI-ARIA virtualized `grid` over the current sort order (the
    /// R783-lifted [`windowed_grid_nodes_sorted`], shared with `hello-grid-sort`)
    /// plus, while the popup is open, the R772 `menu` / `menuitem` subtree (the
    /// R817-lifted [`menu_item_nodes`], shared with `hello-contextmenu`). The
    /// two are independent roots in the access forest.
    fn access_node(state: &HeaderMenuState, focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let grid_sort = use_grid_data();
        let sort = grid_sort.sort();
        let order = grid_sort.order();
        let (_, measured_h) = scroll.measured_viewport();
        let window =
            compute_visible_range(scroll.offset_y(), measured_h, order.len(), ROW_H, OVERSCAN);
        let mut nodes = windowed_grid_nodes_sorted(
            GRID_TAG,
            "Sortable data grid (right-click a header for column actions)",
            HEADERS.len(),
            order.as_slice(),
            sort,
            None,
            &window,
        );
        if state.open_at.is_some() {
            let group_focused = focused == Some(MENU_TAG);
            let tags: Vec<String> = (0..MENU_ITEM_COUNT)
                .map(|i| composite_item_tag(MENU_TAG, i))
                .collect();
            let items: Vec<MenuItemCell<'_>> = (0..MENU_ITEM_COUNT)
                .map(|i| MenuItemCell {
                    tag: &tags[i],
                    label: None,
                    checked: None,
                    disabled: false,
                    focused: group_focused && state.active == Some(i),
                    ..MenuItemCell::default()
                })
                .collect();
            nodes.extend(menu_item_nodes(MENU_TAG, "Column actions", &items));
        }
        nodes
    }

    /// While the menu owns focus and is open, the parent (`colmenu`) stays
    /// focused and the active descendant points at the highlighted item
    /// (mirrors `hello-contextmenu`; the grid is not a focus stop).
    fn access_focus_target(state: &HeaderMenuState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused != Some(MENU_TAG) {
            return focused.map(AccessFocus::atomic);
        }
        state.open_at?;
        Some(match state.active {
            Some(i) => AccessFocus::composite(MENU_TAG, composite_item_tag(MENU_TAG, i)),
            None => AccessFocus::atomic(MENU_TAG),
        })
    }

    /// Composite child action dispatch for the menu items (`colmenu#i<i>`):
    /// `Click` / `Default` activates (command + close), `Focus` records the
    /// AT-side active descendant. Reached through `find_external_with_tag_mut`
    /// because the sort proxy makes the scene a `Container`.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let Some(idx) = parse_item_sub_tag(sub_tag) else {
            return false;
        };
        let Some(node) = scene.find_external_with_tag_mut(MENU_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match action {
            AccessAction::Click | AccessAction::Default => {
                let _ = intro.invoke("send", IntrospectValue::Text(format!("i{idx}:PointerUp")));
                true
            }
            AccessAction::Focus => {
                if let Ok(i) = i64::try_from(idx) {
                    let _ = intro.intervene("active", IntrospectValue::Int(i));
                }
                true
            }
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => false,
        }
    }
}

impl WidgetView for GridHeaderMenuView {
    type Renderer = HelloGridHeaderMenuRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GridHeaderMenuView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;

    fn open(active: Option<usize>) -> HeaderMenuState {
        HeaderMenuState {
            open_at: Some((180.0, 60.0)),
            active,
        }
    }

    // ----- header hit-test geometry -----

    #[test]
    #[allow(
        clippy::cast_precision_loss,
        reason = "the small layout consts are exact as f32 for the test points"
    )]
    fn grid_target_at_maps_band_to_column() {
        let pad = table_style().block_pad; // R988.1 — content inset the paint uses
        let hh = table_style().header_height;
        let band_y = (STATUS_H + pad + 5) as f32; // inside the header band
        let body_y = (STATUS_H + pad + hh + ROW_H + 2) as f32; // body row 1
        // Header click resolves the column, no row.
        assert_eq!(
            grid_target_at((pad + 1) as f32, band_y),
            Some(MenuTarget { col: 0, row: None }),
            "header col 0 near its left edge",
        );
        // R988.1 boundary guard: just inside col 0's right edge stays col 0; the
        // next pixel (col 1's left edge) flips to col 1. The pre-R988.1 hit-test
        // (no `block_pad`) mis-resolved both by one column.
        assert_eq!(
            grid_target_at((pad + COL_W - 1) as f32, band_y),
            Some(MenuTarget { col: 0, row: None }),
            "header col 0 right edge",
        );
        assert_eq!(
            grid_target_at((pad + COL_W) as f32, band_y),
            Some(MenuTarget { col: 1, row: None }),
            "header col 1 left edge",
        );
        // Body cell click resolves the column AND its row.
        assert_eq!(
            grid_target_at((pad + 2 * COL_W + 10) as f32, body_y),
            Some(MenuTarget {
                col: 2,
                row: Some(1)
            }),
            "body cell col 2, row 1",
        );
        // Outside the grid.
        assert_eq!(
            grid_target_at((pad - 1) as f32, band_y),
            None,
            "x left of the first column"
        );
        assert_eq!(
            grid_target_at((pad + 3 * COL_W + 1) as f32, band_y),
            None,
            "x past the last column"
        );
        assert_eq!(
            grid_target_at(10.0, (STATUS_H + pad - 1) as f32),
            None,
            "y in the pad gap above the header"
        );
        assert_eq!(
            grid_target_at(10.0, (STATUS_H - 1) as f32),
            None,
            "y in the status bar"
        );
        assert_eq!(
            grid_target_at(10.0, 10_000.0),
            None,
            "y far below the last row"
        );
    }

    // ----- the captured-column reducer -----

    #[test]
    fn menu_command_sorts_the_captured_column() {
        Owner::new().run(|| {
            // Capture column 1 (Score) via a header click, then activate each item.
            use_menu_target().set(Some(MenuTarget { col: 1, row: None }));
            let _ = GridHeaderMenuView::update(
                open(None),
                &Intent::new_static("colmenu.command", IntrospectValue::Text("0".to_string())),
            );
            assert_eq!(
                use_grid_data().sort(),
                Some((1, true)),
                "item 0 -> col 1 ascending"
            );
            let _ = GridHeaderMenuView::update(
                open(None),
                &Intent::new_static("colmenu.command", IntrospectValue::Text("1".to_string())),
            );
            assert_eq!(
                use_grid_data().sort(),
                Some((1, false)),
                "item 1 -> col 1 descending"
            );
            let _ = GridHeaderMenuView::update(
                open(None),
                &Intent::new_static("colmenu.command", IntrospectValue::Text("2".to_string())),
            );
            assert_eq!(use_grid_data().sort(), None, "item 2 -> clear sort");
        });
    }

    #[test]
    fn command_without_captured_column_is_inert() {
        Owner::new().run(|| {
            // No right-click captured a column -> the command applies no sort.
            let _ = GridHeaderMenuView::update(
                open(None),
                &Intent::new_static("colmenu.command", IntrospectValue::Text("0".to_string())),
            );
            assert_eq!(
                use_grid_data().sort(),
                None,
                "no captured column -> no sort change"
            );
        });
    }

    #[test]
    fn captured_column_is_per_right_click() {
        Owner::new().run(|| {
            use_menu_target().set(Some(MenuTarget { col: 0, row: None }));
            let _ = GridHeaderMenuView::update(
                open(None),
                &Intent::new_static("colmenu.command", IntrospectValue::Text("0".to_string())),
            );
            assert_eq!(
                use_grid_data().sort(),
                Some((0, true)),
                "first capture sorts col 0"
            );
            // A later right-click on a different column's data cell re-captures it.
            use_menu_target().set(Some(MenuTarget {
                col: 2,
                row: Some(3),
            }));
            let _ = GridHeaderMenuView::update(
                open(None),
                &Intent::new_static("colmenu.command", IntrospectValue::Text("1".to_string())),
            );
            assert_eq!(
                use_grid_data().sort(),
                Some((2, false)),
                "re-capture sorts col 2 desc"
            );
        });
    }

    // ----- a11y forest: grid + menu -----

    #[test]
    fn a11y_closed_emits_grid_only() {
        let nodes = Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 380);
            scroll.scroll_to(0, 0);
            GridHeaderMenuView::access_node(&HeaderMenuState::default(), None)
        });
        assert_eq!(
            nodes[0].role,
            AriaRole::Grid,
            "the grid root leads the forest"
        );
        assert!(
            !nodes.iter().any(|n| n.role == AriaRole::Menu),
            "a closed menu contributes no menu node",
        );
    }

    #[test]
    fn a11y_open_appends_menu_forest() {
        let nodes = Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 380);
            scroll.scroll_to(0, 0);
            GridHeaderMenuView::access_node(&open(Some(1)), Some(MENU_TAG))
        });
        assert_eq!(nodes[0].role, AriaRole::Grid, "grid root still leads");
        let menu = nodes
            .iter()
            .find(|n| n.role == AriaRole::Menu)
            .expect("open menu contributes a Menu node");
        assert_eq!(menu.tag, MENU_TAG);
        assert_eq!(
            menu.children.len(),
            MENU_ITEM_COUNT,
            "one menuitem per command"
        );
        let item1 = nodes
            .iter()
            .find(|n| n.tag == "colmenu#i1")
            .expect("item 1 node");
        assert!(item1.state.focused, "the active item carries AT focus");
    }

    #[test]
    fn a11y_focus_target_open_points_to_active_item() {
        let target = GridHeaderMenuView::access_focus_target(&open(Some(2)), Some(MENU_TAG))
            .expect("focused open menu returns Some");
        assert_eq!(target.focus_tag, MENU_TAG);
        assert_eq!(target.active_descendant.as_deref(), Some("colmenu#i2"));
    }
}
