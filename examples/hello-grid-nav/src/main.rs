//! `hello-grid-nav` — R777 §5.27 **data-grid keyboard navigation at scale**.
//!
//! R775 (`hello-virtual-table`) lands a *display-only* virtualized
//! data-grid: a frozen header above a flex-viewport (`AutoSizer`)
//! virtualized body, 10 000 rows, only the window materialized. R776
//! (`hello-virtual-nav`) lands selectable keyboard navigation over a
//! virtualized *list*. This binding brings the two together: the grid
//! becomes **selectable and keyboard-navigable at scale**, the interactive
//! Model/View grid every Phase-B DCC / IDE inspector needs.
//!
//! It is pure composition — the round adds **no** new substrate:
//!
//! * selection model — the R746 [`VirtualSelectExternal`], an index-held
//!   single-select coordinator. The *same* coordinator drives the list and
//!   the grid: a grid cell click (`vtbl#<row>_<col>`) selects the **row**
//!   (WAI-ARIA / Qt `QItemSelectionModel` `SelectRows`; the column is
//!   irrelevant to a row selection).
//! * windowed body + frozen header — the R775
//!   [`view_virtual_table`],
//!   now forwarding a `selected` row so the selected strip paints accent.
//! * scroll-into-view — the R776
//!   [`scroll_offset_to_reveal`](pinion_core::widgets::virtual_list::scroll_offset_to_reveal),
//!   here getting its **second consumer**: navigating to a row that was
//!   never materialized scrolls there.
//!
//! ## Keyboard model (single-select, selection-follows-focus)
//!
//! The grid is a single tab stop (roving by data-row index). Selection is
//! the cursor — the macOS/Windows data-grid model:
//!
//! * `ArrowDown` / `ArrowUp` — move the selected row one step (clamped, no
//!   wrap).
//! * `Home` / `End` — first / last row.
//! * `PageDown` / `PageUp` — move by one measured viewport-ful of rows.
//!
//! Every move scrolls the new selection into view.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/key` `End` → `query("selected")` reports `9999`, and the
//! `scene/snapshot` window has scrolled so `vtbl_row9999` is a rendered
//! node — a row that did not exist at offset 0. A `scene/click` on a cell
//! selects its row. Pure data, no pixels (see `tools/r777_grid_nav.py`).
//!
//! ## a11y
//!
//! Single-select WAI-ARIA virtualized `grid` via the R777-lifted
//! [`windowed_grid_nodes_selected`] (shared with the display-only
//! `hello-virtual-table`): `aria-setsize = N`, one `row` per windowed index
//! with `aria-posinset` + `aria-selected = (id == selected)` and a
//! `gridcell` per column, under a frozen header row of `columnheader`s.

use pinion_a11y::{AccessNode, WidgetA11y, mark_grid_editability, windowed_grid_nodes_selected};
use pinion_core::external::External;
use pinion_core::input::is_activation_event;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::grid_edit::{
    EditTrigger, EditTriggers, EndEditHint, GridEditState, use_grid_edit,
};
use pinion_core::widgets::scroll::use_scroll_state;
#[cfg(test)]
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::TextFieldState;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::{
    RowMetrics, VirtualSelectExternal, nav_select_key, read_selected,
};
use pinion_core::{
    CellEdit, CellIndex, CellKind, CellValue, Frame, GridExtent, Modifiers, Scene, WidgetCore,
    edit_field_keymap,
};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::table::{
    CellEditRender, CellEditorPainter, GridEditing, GridModel, GridScroll, HeaderAxis, TableStyle,
    VirtualTableData, header_from_slice, no_decoration, no_row_header, view_virtual_table,
};
use std::collections::BTreeMap;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGridNavRenderer, HelloGridNavRendererError);

/// Initial window size — freely resizable; the grid body re-windows on
/// every `Resized` event. Wide enough that `NCOLS × COL_W` fits.
const WIN_W: u32 = 400;
const WIN_H: u32 = 480;
const THEME_TAG: &str = "app";
/// Total data-row count — large while the rendered node count stays small.
const N: usize = 10_000;
/// Column count (matches `HEADERS.len()`).
const NCOLS: usize = 3;
/// Uniform column width; `NCOLS × COL_W = 330 < WIN_W` so no h-scroll.
const COL_W: u32 = 110;
/// Data-row height (the windowing pitch + the scroll-into-view pitch).
const ROW_H: u32 = 36;
/// Rows built beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Status-bar height above the grid.
const STATUS_H: u32 = 40;
/// Column header labels. R1544 — `Index` is the **read-only** identity
/// column, `Name` a free-text one and `Score` a bounded integer, so the
/// grid's `Qt::EditRole` answer differs three ways across three columns.
const HEADERS: [&str; NCOLS] = ["Index", "Name", "Score"];
/// R1544 — the identity column: [`edit_role`] answers `None` for it (Qt:
/// `flags()` without `Qt::ItemIsEditable`), so no trigger opens an editor
/// there and every one of its cells is `aria-readonly` to assistive tech.
const INDEX_COL: usize = 0;
/// R1544 — the free-text column, edited through the built-in
/// [`text_cell_editor`](pinion_widget_paint::table::text_cell_editor).
const NAME_COL: usize = 1;
/// R1544 — the bounded-integer column, edited through the
/// [`score_editor`] **editor delegate**.
const SCORE_COL: usize = 2;
/// R1544 — the `Score` column's inclusive upper bound. A commit past it is
/// **refused by the model**, which keeps the editor open holding the typed
/// text — the behaviour Qt's `void setModelData` cannot express.
const SCORE_MAX: i64 = 100;
/// Paint-root + a11y `grid` tag, and the [`VirtualSelectExternal`] anchor
/// (cell clicks on `vtbl#<id>_<col>` route here via the R51.42 composite
/// protocol).
const TABLE_TAG: &str = "vtbl";
const SCROLL_KEY: &str = "vtbl_scroll";
/// R784 — outer horizontal scroll `ScrollState` cache key (columns fit
/// the window here, so `max_x` stays 0 — wiring present for parity).
const H_SCROLL_KEY: &str = "vtbl_hscroll";
const STATUS_TAG: &str = "vtbl_status";
/// R1544 — the inline editor field's tag: the one transient editor's
/// `use_text_edit_state` key, and the node an `edit_field_keymap` forwards
/// keystrokes to.
const EDIT_FIELD_TAG: &str = "vtbl_editor";
/// R1544 — the shared [`GridEditState`] cache key.
const EDIT_KEY: &str = "vtbl_edit";
/// R1544 — cache key for the committed-cell overlay (the mutable half of an
/// otherwise synthetic 10 000-row model).
const OVERLAY_KEY: &str = "vtbl_overlay";
/// R1544 — cache key for the **current cell** (Qt `currentIndex()`).
const CURRENT_KEY: &str = "vtbl_current";

/// R1544 — the projected widget state: the selected data-row index and the
/// inline editor field's `(statechart, caret byte)` pair. The scroll offset +
/// measured viewport drive their own repaints through the reactive
/// `ScrollState` subscriptions the view opens.
type RootState = (Option<usize>, (TextFieldState, u32));

fn table_style() -> TableStyle {
    TableStyle {
        col_width: COL_W,
        row_height: ROW_H,
        // R1020 §5.39 — the grid is a single Tab stop; opt the table into the
        // scene-derived focus enumeration so its TABLE_TAG is collected.
        focusable: true,
        ..TableStyle::m3()
    }
}

/// R1544 — the committed-cell overlay: the *mutable* half of the model.
///
/// The dataset stays a **function** of the index (10 000 rows are never
/// materialized), and a commit records only the cells that were actually
/// edited — which is what makes "editable at scale" mean anything. Qt's
/// `QAbstractItemModel` is the same shape: `data()` computes, `setData()`
/// records.
///
/// Keyed by the flat `row * NCOLS + col`, the same flattening
/// [`GridEditState::advance`] walks, rather than by a `(row, col)` tuple —
/// one linear key, one ordering, and it round-trips through the `Signal`
/// serde bound that a tuple-keyed map does not.
fn use_overlay() -> Rc<Signal<BTreeMap<usize, String>>> {
    Owner::current()
        .expect("use_overlay requires an active Owner scope")
        .cache(OVERLAY_KEY, || Signal::new(BTreeMap::new()))
}

/// R1544 — the **current cell**, Qt's `currentIndex()`.
///
/// A separate axis from the row *selection* the [`VirtualSelectExternal`]
/// holds, exactly as in Qt: selection is what is highlighted, current is what
/// the keyboard acts on — and an edit trigger acts on the current **cell**,
/// which a row-only cursor cannot name. Moved by the arrow keys and by a
/// cell click; `None` until the grid is first navigated or clicked.
fn use_current() -> Rc<Signal<Option<CellIndex>>> {
    Owner::current()
        .expect("use_current requires an active Owner scope")
        .cache(CURRENT_KEY, || Signal::new(None))
}

/// The flat overlay key for a cell — `row * NCOLS + col`.
fn flat(c: CellIndex) -> usize {
    c.row * NCOLS + c.col
}

/// Synthetic cell texts for a data row (same dataset as `hello-virtual-table`),
/// before any committed edit is applied.
fn generated_cell_text(c: CellIndex) -> String {
    const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    let id = c.row;
    match c.col {
        INDEX_COL => format!("{id:05}"),
        NAME_COL => CATEGORIES[id % CATEGORIES.len()].to_string(),
        _ => ((id * 7) % 101).to_string(),
    }
}

/// R1544 — the model's `Qt::DisplayRole`: the committed value if this cell
/// has one, else the generated datum.
fn cell_text(c: CellIndex, overlay: &BTreeMap<usize, String>) -> String {
    overlay
        .get(&flat(c))
        .cloned()
        .unwrap_or_else(|| generated_cell_text(c))
}

/// R1544 — which editor a column opens, or `None` when the column is not
/// editable at all. The single place this grid states its `Qt::ItemIsEditable`
/// flag, so [`edit_role`] and [`commit_cell`] cannot disagree about it.
fn column_kind(col: usize) -> Option<CellKind> {
    match col {
        INDEX_COL => None,
        NAME_COL => Some(CellKind::Text),
        _ => Some(CellKind::Int),
    }
}

/// R1544 §5.27 — the model's `Qt::EditRole`: what an editor opened on this
/// cell is seeded with, and which editor to open. `None` **is** "not
/// editable".
///
/// The seed is the display text here because the two forms coincide for both
/// editable columns; a currency or unit column would differ — see
/// [`CellValue::edit_text`], the canonical source of the seed.
fn edit_role(c: CellIndex, overlay: &BTreeMap<usize, String>) -> Option<CellEdit> {
    column_kind(c.col).map(|kind| CellEdit::new(kind, cell_text(c, overlay)))
}

/// R1544 §5.27 — the model's `setData(index, value, Qt::EditRole)`: parse the
/// editor buffer by the column's kind and record it, or **refuse**.
///
/// Returns `false` for a malformed value and for a `Score` outside
/// `0..=SCORE_MAX`. A refusal leaves the editor open holding what the user
/// typed ([`GridEditState::commit_with`]), which is the only state they can
/// correct it from — Qt's `setModelData` returns `void`, so there the editor
/// closes and the typing is discarded.
fn commit_cell(index: CellIndex, text: &str) -> bool {
    let Some(kind) = column_kind(index.col) else {
        return false;
    };
    let Some(value) = kind.parse(text) else {
        return false;
    };
    if index.col == SCORE_COL {
        let CellValue::Int(n) = value else {
            return false;
        };
        if !(0..=SCORE_MAX).contains(&n) {
            return false;
        }
    }
    let display = value.display();
    use_overlay().set_with(move |prev| {
        let mut next = prev.clone();
        next.insert(flat(index), display.clone());
        next
    });
    true
}

/// R1544 §5.27 — the `Score` column's **editor delegate** (Qt
/// `QStyledItemDelegate::createEditor` + `setEditorData`): the inline field
/// with the column's accepted range spelled beside it.
///
/// This is the editor a built-in one cannot be. `text_cell_editor` opens a
/// field seeded from the model and nothing else, which is right for free
/// text; a bounded column wants to say what its bound *is*, and saying it in
/// the editor is the difference between a refused commit that teaches and one
/// that merely rejects. Qt reaches the same place by returning a
/// `QSpinBox` from `createEditor`.
///
/// It builds its own container rather than wrapping
/// [`text_cell_editor`](pinion_widget_paint::table::text_cell_editor),
/// because the cell tag belongs on exactly one node and that helper already
/// puts it on its own — the same rule the paint delegate `load_bar` follows in
/// `hello-virtual-table`.
fn score_editor(c: &CellEditRender<'_>) -> Scene {
    let cell = c.cell;
    let hint_w = 34;
    let pad = cell.style.cell_pad_x;
    let field_w = cell.width.saturating_sub(pad * 2 + hint_w);
    let field_style = pinion_widget_paint::text_field::TextFieldStyle {
        field_w,
        field_h: cell.height.saturating_sub(6),
        ..pinion_widget_paint::text_field::TextFieldStyle::m3_filled()
    };
    let field = pinion_widget_paint::text_field::view_field(
        EDIT_FIELD_TAG,
        c.field.0,
        c.field.1,
        cell.theme,
        &field_style,
        "",
    );
    let hint = Scene::Text(TextNode::styled(
        format!("/{SCORE_MAX}"),
        Rect::default(),
        TextStyle::new()
            .with_size_px(11)
            .with_fg(cell.theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    Scene::Container(
        ContainerNode::new(vec![field, hint])
            // The cell's own tag — a painter that omits it drops the cell out
            // of pointer routing and out of every tag-addressed RPC.
            .with_tag(cell.tag.to_string())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(cell.width, cell.height))
                    .with_padding(Rect::new(pad, 0, pad, 0)),
            ),
    )
}

/// Status bar above the grid: a literal scene-as-data readout of the
/// selected row + measured viewport. Press `End` and it reports
/// `selected 9999`, proving the selection survives a row that was never
/// materialized at boot.
fn status_bar(
    scroll: &std::rc::Rc<pinion_core::widgets::scroll::ScrollState>,
    theme: &Theme,
    selected: Option<usize>,
) -> Scene {
    let (mw, mh) = scroll.measured_viewport();
    let sel = selected.map_or_else(|| "none".to_string(), |i| i.to_string());
    // R1544 — the editing latch as text, so `scene/snapshot` alone answers
    // "which cell has an open editor and what is in it" (§2 #7). Qt has no
    // public equivalent: `isPersistentEditorOpen` covers only the persistent
    // kind, and a transient editor's buffer lives inside an opaque QWidget.
    let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
    let editing = edit.open().map_or_else(
        || "none".to_string(),
        |e| format!("{}_{} \"{}\"", e.index.row, e.index.col, edit.text()),
    );
    let text = Scene::Text(
        TextNode::styled(
            format!("selected {sel} \u{00B7} viewport {mw}\u{00D7}{mh} \u{00B7} editing {editing}"),
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

/// view-fn (§6.3): pure sync mapping `selected row -> Scene`. The dataset
/// is virtual — `view_virtual_table` invokes [`cell_text`] only for the
/// indices in the current window, whose *size* is the runtime-measured
/// viewport height.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: RootState, _frame: &Frame) -> Scene {
    let (selected, field) = state;
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = table_style();
    // Read once per frame rather than once per painted cell: the accessors
    // below close over it, so the display role and the edit role are answered
    // from the same snapshot of the model.
    let overlay = use_overlay().get();
    let edit_state = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
    let open = edit_state.open();
    // The latch holds the seed as a `String`; the paint contract wants the
    // model's `CellEdit`. Rebuilt here rather than stored twice, and bound to
    // a local so the borrow the painter takes outlives the call.
    let open_edit = open.as_ref().map(|e| CellEdit::new(e.kind, e.seed.clone()));
    // Qt `setItemDelegateForColumn`, editing half: the bounded column opens an
    // editor that states its bound; every other column takes the built-in
    // field.
    let pick_editor =
        |col: usize| (col == SCORE_COL).then_some(&score_editor as CellEditorPainter<'_>);
    let editing = open
        .as_ref()
        .zip(open_edit.as_ref())
        .map(|(e, edit)| GridEditing {
            open: e.index,
            edit,
            field_tag: EDIT_FIELD_TAG,
            field,
            editor: Some(&pick_editor),
        });

    let grid = view_virtual_table(
        TABLE_TAG,
        GridScroll {
            body: &scroll,
            horizontal: &h_scroll,
        },
        VirtualTableData {
            column_count: NCOLS,
            item_count: N,
            overscan: OVERSCAN,
            sort: None,
            sort_tag: None,
            order: None,
            col_widths: None,
            resizable: false,
            frozen_cols: 0,
            row_style: None,
            delegate: None,
            editing,
        },
        &theme,
        &style,
        |id| selected == Some(id),
        GridModel {
            cell: |c: CellIndex| cell_text(c, &overlay),
            columns: HeaderAxis::labelled(header_from_slice(&HEADERS)),
            rows: no_row_header(),
            decoration: no_decoration,
            // R1544 — Qt `data(index, Qt::EditRole)` fused with
            // `flags() & Qt::ItemIsEditable`: the identity column answers
            // `None`, which is what makes an editor on it unrepresentable
            // rather than merely unwired.
            edit: |c: CellIndex| edit_role(c, &overlay),
        },
    );

    Scene::Container(
        ContainerNode::new(vec![status_bar(&scroll, &theme, selected), grid])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

/// R1544 — whether `key` is a single printable character, the class Qt's
/// `AnyKeyPressed` trigger fires on. Named keys (`ArrowDown`, `F2`) are
/// multi-codepoint, so length alone separates them.
fn is_printable(key: &str) -> bool {
    let mut chars = key.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if !c.is_control())
}

/// R1544 — the current cell, defaulting to the first cell so a keyboard user
/// who has not clicked can still start editing (Qt seeds `currentIndex()` the
/// same way when a view first takes focus).
fn current_cell() -> CellIndex {
    use_current().get().unwrap_or(CellIndex::new(0, 0))
}

/// R1544 — move the current cell one column, clamped (no wrap: wrapping is
/// what <kbd>Tab</kbd> does, and only while editing).
fn move_col(_scene: &mut Scene, delta: isize) -> bool {
    let at = current_cell();
    let next = at.col.saturating_add_signed(delta).min(NCOLS - 1);
    if next == at.col {
        return true;
    }
    use_current().set(Some(CellIndex::new(at.row, next)));
    true
}

/// R1544 — after the row selection moves, put the current cell on the new
/// row, keeping the column. Read back from the coordinator so the current
/// cell cannot drift from the selection it follows.
fn sync_current_row(scene: &Scene) {
    let Some(row) = scene
        .find_external_with_tag(TABLE_TAG)
        .and_then(|node| node.handle.introspect())
        .and_then(read_selected)
    else {
        return;
    };
    use_current().set(Some(CellIndex::new(row, current_cell().col)));
}

/// R1544 — open an editor on the current cell through `trigger`, and (for the
/// type-to-replace trigger) forward the keystroke that opened it.
///
/// Returns whether the key was consumed. A cell the model will not edit
/// produces no [`CellEdit`], so `begin_on` is never reached for it — the
/// read-only column is unreachable by construction, not by a second check.
fn begin_at_current(trigger: EditTrigger, forward: Option<(&mut Scene, &str, Modifiers)>) -> bool {
    let at = current_cell();
    let overlay = use_overlay().get();
    let Some(role) = edit_role(at, &overlay) else {
        return false;
    };
    let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
    if !edit.begin_on(trigger, at, &role) {
        return false;
    }
    // Qt's editor is a child widget that takes focus; here the request is the
    // binding's, because `edit_field_keymap`'s contract leaves *where focus
    // goes* a binding decision.
    pinion_core::focus_request::request(EDIT_FIELD_TAG);
    match forward {
        Some((scene, key, modifiers)) => {
            // The editor just opened with its seed fully selected, so this
            // keystroke replaces it. A key the kind rejects (a letter into an
            // int column) leaves the seed intact and the editor stays open —
            // Qt's `AnyKeyPressed` opens first and lets the editor filter. The
            // key is consumed either way: it opened an editor, so letting it
            // fall through to navigation would move the cursor as well.
            let _ = pinion_core::forward_key_to_field(scene, EDIT_FIELD_TAG, key, modifiers);
            true
        }
        None => true,
    }
}

/// R1544 — the keyboard while an editor is open.
///
/// <kbd>Tab</kbd> / <kbd>Shift+Tab</kbd> are Qt's `EditNextItem` /
/// `EditPreviousItem` end-edit hints: commit, then open an editor on the next
/// **editable** cell. Everything else goes through the shared
/// [`edit_field_keymap`] SSOT, which maps <kbd>Enter</kbd> to commit,
/// <kbd>Escape</kbd> to cancel, and gates printable keystrokes by the open
/// editor's [`CellKind`].
fn editing_key(
    scene: &mut Scene,
    edit: &Rc<GridEditState>,
    kind: CellKind,
    key: &str,
    modifiers: Modifiers,
) -> bool {
    if key == "Tab" {
        let hint = if modifiers.shift {
            EndEditHint::EditPreviousItem
        } else {
            EndEditHint::EditNextItem
        };
        // A refused commit returns `None` and leaves the editor open holding
        // the typed text — so Tab does not silently discard it either.
        if let Some(from) = edit.commit_with(commit_cell) {
            let overlay = use_overlay().get();
            edit.advance(from, hint, GridExtent::new(N, NCOLS), |i| {
                edit_role(i, &overlay)
            });
            match edit.open() {
                Some(open) => use_current().set(Some(open.index)),
                // Nothing editable to advance to: the edit is done, so focus
                // goes back to the grid rather than to a closed field.
                None => pinion_core::focus_request::request(TABLE_TAG),
            }
        }
        return true;
    }
    edit_field_keymap(
        scene,
        EDIT_FIELD_TAG,
        key,
        modifiers,
        kind,
        || {
            if edit.commit_with(commit_cell).is_some() {
                pinion_core::focus_request::request(TABLE_TAG);
            }
        },
        || {
            edit.cancel();
            pinion_core::focus_request::request(TABLE_TAG);
        },
    )
}

struct GridNavView;

impl WidgetCore for GridNavView {
    /// R1544 — the selected data-row index plus the inline editor field's
    /// statechart snapshot and caret byte. The editor is a real
    /// [`TextField`](pinion_core::widgets::text_field::TextField) sibling
    /// (`create_extra_externals`), so its focus posture and caret are read
    /// back from the scene exactly as every other widget's state is, rather
    /// than synthesized by the view from what it believes ought to be true.
    type State = RootState;
    type Event = ();

    /// The primary External is the R746 index-held selection coordinator,
    /// addressable at [`TABLE_TAG`]. Cell clicks on the windowed
    /// `vtbl#<id>_<col>` cells route here via the R51.42 composite protocol
    /// (selecting the row); `apply_key` drives it from the keyboard.
    fn create_external() -> Box<dyn External> {
        let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
        // R1544 — Qt's own `QAbstractItemView` default plus type-to-replace,
        // the spreadsheet gesture a grid with a current cell can offer.
        edit.set_triggers(EditTriggers::DEFAULT.with(EditTrigger::AnyKeyPressed));
        let current = use_current();
        let overlay = use_overlay();
        Box::new(
            VirtualSelectExternal::new(N).on_cell_gesture(move |index, event, _modifiers| {
                // Qt's `SelectedClicked`: a plain click on the cell that was
                // ALREADY current. The observer runs before the coordinator
                // moves the selection, so `current` still holds the pre-click
                // cell — which is the whole reason the hook is offered there.
                let was_current = current.get() == Some(index);
                let trigger = if event == "DoubleClick" {
                    EditTrigger::DoubleClicked
                } else if is_activation_event(event) {
                    if was_current {
                        EditTrigger::SelectedClicked
                    } else {
                        current.set(Some(index));
                        return;
                    }
                } else {
                    return;
                };
                current.set(Some(index));
                // The model decides: a cell it will not edit produces no
                // `CellEdit`, so no gesture can open an editor on it.
                if let Some(edit_role) = edit_role(index, &overlay.get())
                    && edit.begin_on(trigger, index, &edit_role)
                {
                    pinion_core::focus_request::request(EDIT_FIELD_TAG);
                }
            }),
        )
    }

    fn tag() -> &'static str {
        TABLE_TAG
    }

    /// Project the selected row off the primary coordinator. A selection
    /// change repaints; scroll offset repaints via its own reactive
    /// `Signal` subscription the view opens.
    fn read_state(scene: &Scene) -> RootState {
        let selected = scene
            .find_external_with_tag(TABLE_TAG)
            .and_then(|node| node.handle.introspect())
            .and_then(read_selected);
        (
            selected,
            pinion_widget_paint::text_field::read_text_field_state(scene, EDIT_FIELD_TAG),
        )
    }

    /// R1544 — the inline editor sibling: a `TextField` External at
    /// [`EDIT_FIELD_TAG`] sharing the very `TextEditState` the editing verbs
    /// read and write, through the lifted R1250 registration (this is its
    /// fifth consumer). Without it the field would be paint with no input
    /// path — `forward_key_to_field` addresses an External, not a painted
    /// node. Its commit-on-blur intent is inert here because focus returns to
    /// the grid through an explicit request rather than by being taken away.
    fn create_extra_externals() -> Vec<pinion_core::widget_core::ExtraExternal> {
        vec![pinion_core::widgets::text_field::blur_committing_field_extra(EDIT_FIELD_TAG)]
    }

    fn view(state: RootState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// R777 §5.27 — keyboard navigation over the windowed grid, delegated
    /// to the shared `nav_select_key` controller (the same one
    /// `hello-virtual-nav` uses for the list): keys only route when the grid
    /// is focused (single tab stop); each handled key moves the index-model
    /// row selection (linear clamp, no wrap) and scrolls the new selection
    /// into view. The pitch is the data-row height, the pitch the body
    /// windows against.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
        // R1544 — an open editor owns the keyboard, and it owns focus too
        // (the editor is a real `TextField` sibling), so this dispatches on
        // the same focused tag every other multi-stop binding does.
        if let Some(open) = edit.open() {
            if focused == Some(EDIT_FIELD_TAG) {
                return editing_key(scene, &edit, open.kind, key, modifiers);
            }
        }
        if focused == Some(TABLE_TAG) {
            match key {
                // R1544 — the column half of the cursor. Qt's edit triggers
                // act on `currentIndex()`, which is a CELL: a grid that could
                // only move between rows had no index to open an editor on.
                "ArrowLeft" => return move_col(scene, -1),
                "ArrowRight" => return move_col(scene, 1),
                // Qt `EditKeyPressed` — F2 on every desktop platform.
                "F2" => return begin_at_current(EditTrigger::EditKeyPressed, None),
                // Qt `AnyKeyPressed` — type-to-replace. The seed is fully
                // selected on open, so forwarding the keystroke replaces it.
                k if is_printable(k) && !modifiers.command_key() => {
                    return begin_at_current(
                        EditTrigger::AnyKeyPressed,
                        Some((scene, k, modifiers)),
                    );
                }
                _ => {}
            }
        }
        let moved = nav_select_key(
            scene,
            &use_scroll_state(SCROLL_KEY),
            TABLE_TAG,
            focused,
            key,
            modifiers,
            RowMetrics {
                item_count: N,
                row_pitch: ROW_H,
            },
        );
        if moved {
            // Keep the current CELL on the row the selection just moved to,
            // holding the column. Read back from the coordinator rather than
            // recomputed, so the two cannot drift.
            sync_current_row(scene);
        }
        moved
    }

    /// R1544 — IME composition into the inline editor. Without it a CJK or
    /// dead-key sequence would have nowhere to land while a cell is open,
    /// which is the difference between an editor and a Latin-only one.
    fn apply_composition(
        scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        if focused != Some(EDIT_FIELD_TAG) {
            return false;
        }
        pinion_widget_paint::text_field::forward_composition_to_field(scene, EDIT_FIELD_TAG, event)
    }

    fn title() -> &'static str {
        "pinion hello-grid-nav (R777 §5.27 data-grid keyboard navigation at scale)"
    }

    fn fmt_state_log(state: &RootState) -> String {
        match state.0 {
            Some(i) => format!("selected=row {i}"),
            None => "selected=none".to_string(),
        }
    }
}

impl WidgetA11y for GridNavView {
    /// Single-select WAI-ARIA virtualized `grid` via the R777-lifted
    /// [`windowed_grid_nodes_selected`] (shared with `hello-virtual-table`
    /// so the virtualized-grid topology is one source of truth): each
    /// windowed data row carries `aria-posinset` + `aria-selected = (id ==
    /// selected)`; one `gridcell` per column; a frozen header row of
    /// `columnheader`s. The window is the same `compute_visible_range` over
    /// the measured viewport the view fn uses, so the a11y tree and the
    /// painted tree never disagree on which rows exist.
    fn access_node(state: &RootState, _focused: Option<&str>) -> Vec<AccessNode> {
        let selected = &state.0;
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_H, OVERSCAN);
        let mut nodes = windowed_grid_nodes_selected(
            TABLE_TAG,
            "Navigable data grid",
            HEADERS.len(),
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
            *selected,
        );
        // R1544 §5.40 — the same `edit` role the editors open from decides
        // `aria-readonly`, so a screen-reader user learns the identity column
        // is fixed instead of discovering it by typing into it. Qt says
        // nothing here: `QAccessibleTableCell` builds its state from the
        // view's selection, never from the model's `Qt::ItemIsEditable`.
        let overlay = use_overlay().get();
        mark_grid_editability(&mut nodes, TABLE_TAG, &window, 0..NCOLS, |c| {
            edit_role(c, &overlay).is_some()
        });
        nodes
    }
}

impl WidgetView for GridNavView {
    type Renderer = HelloGridNavRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GridNavView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::Owner;
    use pinion_core::widgets::scroll::ScrollState;
    use pinion_core::widgets::virtual_list::scroll_offset_to_reveal;
    use std::rc::Rc;

    // Keyboard nav policy + controller (`clamp_nav` / `nav_select_key`) are
    // unit-tested in `pinion_core::widgets::virtual_select`; this binding's
    // apply_key is a thin delegation. End-to-end keyboard drive is covered
    // by `tools/r777_grid_nav.py`.

    fn run_view_with_measured(selected: Option<usize>, offset_y: i32, measured_h: u32) -> Scene {
        Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_max(0, i32::try_from(N).unwrap() * i32::try_from(ROW_H).unwrap());
            scroll.set_measured_viewport(WIN_W, measured_h);
            scroll.scroll_to(0, offset_y);
            view((selected, (TextFieldState::Idle, 0)), &Frame::default())
        })
    }

    /// Find the `vtbl_row<id>` strip and return its fill color.
    fn row_fill(scene: &Scene, id: usize) -> Option<pinion_core::style::Color> {
        fn walk(scene: &Scene, want: &str) -> Option<pinion_core::style::Color> {
            match scene {
                Scene::Container(c) => {
                    if c.tag.as_deref() == Some(want) {
                        return Some(c.style.fill);
                    }
                    c.children.iter().find_map(|ch| walk(ch, want))
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), want),
                _ => None,
            }
        }
        walk(scene, &format!("{TABLE_TAG}_row{id}"))
    }

    #[test]
    fn selected_row_paints_accent_wash() {
        // The grid selection tint is a 16% accent wash over Surface (the
        // shared `table::row_fill` selection path), distinct from any
        // unselected row.
        let theme = pinion_core::theme::Theme::light();
        let wash = theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::Accent), 0.16);
        let scene = run_view_with_measured(Some(2), 0, 384);
        assert_eq!(
            row_fill(&scene, 2),
            Some(wash),
            "selected row strip is the accent wash"
        );
        assert_ne!(
            row_fill(&scene, 3),
            Some(wash),
            "an unselected neighbor differs"
        );
    }

    #[test]
    fn a11y_marks_selected_row_and_tracks_window() {
        let nodes = Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 384);
            GridNavView::access_node(&(Some(1), (TextFieldState::Idle, 0)), None)
        });
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert_eq!(nodes[0].size_of_set, Some(u32::try_from(N).unwrap()));
        let row1 = nodes
            .iter()
            .find(|n| n.tag == format!("{TABLE_TAG}_row1"))
            .unwrap();
        assert_eq!(
            row1.selected,
            Some(true),
            "selected row carries aria-selected=true"
        );
        let row0 = nodes
            .iter()
            .find(|n| n.tag == format!("{TABLE_TAG}_row0"))
            .unwrap();
        assert_eq!(row0.selected, Some(false));
    }

    /// Walk the paint scene for the node tagged `want`.
    fn find_tagged<'s>(scene: &'s Scene, want: &str) -> Option<&'s Scene> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(want) {
                    return Some(scene);
                }
                c.children.iter().find_map(|ch| find_tagged(ch, want))
            }
            Scene::Scroll(s) => find_tagged(s.content.as_ref(), want),
            _ => None,
        }
    }

    fn walk_texts(scene: &Scene, out: &mut Vec<String>) {
        match scene {
            Scene::Text(t) => out.push(t.content.clone()),
            Scene::Container(c) => c.children.iter().for_each(|ch| walk_texts(ch, out)),
            Scene::Scroll(s) => walk_texts(s.content.as_ref(), out),
            _ => {}
        }
    }

    /// Every `Scene::Text` string under `scene`, in paint order.
    fn texts(scene: &Scene) -> Vec<String> {
        let mut out = Vec::new();
        walk_texts(scene, &mut out);
        out
    }

    fn cell(scene: &Scene, row: usize, col: usize) -> &Scene {
        find_tagged(scene, &format!("{TABLE_TAG}#{row}_{col}")).expect("cell painted")
    }

    /// Run `f` in one Owner scope with a measured viewport, then paint.
    fn edit_scene(f: impl FnOnce()) -> Scene {
        Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_max(0, i32::try_from(N).unwrap() * i32::try_from(ROW_H).unwrap());
            scroll.set_measured_viewport(WIN_W, 384);
            // R1523 — the COLUMN window is measured too: without a measured
            // horizontal viewport the grid windows zero columns and every row
            // paints empty, which is a scene no cell assertion can find.
            use_scroll_state(H_SCROLL_KEY).set_measured_viewport(WIN_W, 384);
            f();
            view((Some(1), (TextFieldState::Focused, 0)), &Frame::default())
        })
    }

    #[test]
    fn r1544_an_open_editor_replaces_the_cell_it_is_open_on() {
        let at = CellIndex::new(1, NAME_COL);
        let scene = edit_scene(|| {
            let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
            let role = edit_role(at, &use_overlay().get()).expect("Name is editable");
            edit.begin(at, &role);
        });
        // The editing cell now carries the inline field, and its neighbour in
        // the same row does not — an editor replaces ONE cell, not the row.
        assert!(
            find_tagged(cell(&scene, 1, NAME_COL), EDIT_FIELD_TAG).is_some(),
            "the editing cell hosts the inline field"
        );
        assert!(
            find_tagged(cell(&scene, 1, INDEX_COL), EDIT_FIELD_TAG).is_none(),
            "its neighbour keeps the display painter"
        );
        assert!(
            find_tagged(cell(&scene, 2, NAME_COL), EDIT_FIELD_TAG).is_none(),
            "the same column in another row keeps the display painter"
        );
    }

    #[test]
    fn r1544_the_editor_delegate_paints_the_bounded_column() {
        let at = CellIndex::new(1, SCORE_COL);
        let scene = edit_scene(|| {
            let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
            let role = edit_role(at, &use_overlay().get()).expect("Score is editable");
            edit.begin(at, &role);
        });
        let painted = texts(cell(&scene, 1, SCORE_COL));
        assert!(
            painted.iter().any(|t| t == &format!("/{SCORE_MAX}")),
            "the delegate's range hint is painted: {painted:?}"
        );
        // The built-in editor has no such hint, so this is the delegate's own
        // output rather than the default one.
        let name_at = CellIndex::new(1, NAME_COL);
        let default_scene = edit_scene(|| {
            let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
            let role = edit_role(name_at, &use_overlay().get()).expect("editable");
            edit.begin(name_at, &role);
        });
        assert!(
            !texts(cell(&default_scene, 1, NAME_COL))
                .iter()
                .any(|t| t == &format!("/{SCORE_MAX}")),
            "the built-in editor paints no hint"
        );
    }

    #[test]
    fn r1544_the_read_only_column_answers_no_edit_role() {
        Owner::new().run(|| {
            let overlay = use_overlay().get();
            assert!(
                edit_role(CellIndex::new(0, INDEX_COL), &overlay).is_none(),
                "the identity column is not editable"
            );
            assert!(edit_role(CellIndex::new(0, NAME_COL), &overlay).is_some());
            assert!(edit_role(CellIndex::new(0, SCORE_COL), &overlay).is_some());
            // A trigger on it opens nothing, because there is no `CellEdit` to
            // open with — unrepresentable rather than merely unwired.
            let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
            use_current().set(Some(CellIndex::new(0, INDEX_COL)));
            assert!(!begin_at_current(EditTrigger::EditKeyPressed, None));
            assert_eq!(edit.open(), None);
        });
    }

    #[test]
    fn r1544_a_refused_commit_keeps_the_editor_open_and_the_model_unchanged() {
        Owner::new().run(|| {
            let at = CellIndex::new(4, SCORE_COL);
            let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
            let before = cell_text(at, &use_overlay().get());
            edit.begin(at, &edit_role(at, &use_overlay().get()).unwrap());
            use_text_edit_state(EDIT_FIELD_TAG).set_text("500".to_string());
            assert_eq!(edit.commit_with(commit_cell), None, "500 is past the bound");
            assert_eq!(edit.editing(), Some(at), "the editor stays open");
            assert_eq!(edit.text(), "500", "holding what was typed");
            assert_eq!(
                cell_text(at, &use_overlay().get()),
                before,
                "model untouched"
            );
            // Correcting it in place commits.
            use_text_edit_state(EDIT_FIELD_TAG).set_text("50".to_string());
            assert_eq!(edit.commit_with(commit_cell), Some(at));
            assert_eq!(edit.open(), None);
            assert_eq!(cell_text(at, &use_overlay().get()), "50");
        });
    }

    #[test]
    fn r1544_a_committed_edit_is_what_the_grid_paints() {
        let at = CellIndex::new(2, NAME_COL);
        let scene = edit_scene(|| {
            let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
            edit.begin(at, &edit_role(at, &use_overlay().get()).unwrap());
            use_text_edit_state(EDIT_FIELD_TAG).set_text("Renamed".to_string());
            assert_eq!(edit.commit_with(commit_cell), Some(at));
        });
        assert!(
            texts(cell(&scene, 2, NAME_COL))
                .iter()
                .any(|t| t == "Renamed"),
            "the display role reads the committed overlay"
        );
    }

    #[test]
    fn r1544_a11y_marks_the_read_only_column_and_leaves_the_others_silent() {
        let nodes = Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 384);
            GridNavView::access_node(&(Some(1), (TextFieldState::Idle, 0)), None)
        });
        let read_only = |row: usize, col: usize| {
            nodes
                .iter()
                .find(|n| n.tag == format!("{TABLE_TAG}#{row}_{col}"))
                .expect("cell node")
                .state
                .read_only
        };
        assert!(read_only(0, INDEX_COL), "identity column is aria-readonly");
        assert!(!read_only(0, NAME_COL));
        assert!(!read_only(0, SCORE_COL));
    }

    #[test]
    fn r1544_tab_advances_past_the_read_only_column() {
        Owner::new().run(|| {
            let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
            let overlay = use_overlay().get();
            // From the last editable cell of row 0, forward lands on row 1's
            // Name — skipping row 1's read-only Index column entirely.
            assert!(edit.advance(
                CellIndex::new(0, SCORE_COL),
                EndEditHint::EditNextItem,
                GridExtent::new(N, NCOLS),
                |c| edit_role(c, &overlay)
            ));
            assert_eq!(edit.editing(), Some(CellIndex::new(1, NAME_COL)));
        });
    }

    #[test]
    fn reveal_scrolls_a_deep_target_into_view() {
        // Selecting the last row and revealing it moves the offset deep so
        // the window now includes row 9999 — never materialized at offset 0.
        let s = Rc::new(ScrollState::new());
        s.set_max(0, i32::try_from(N).unwrap() * i32::try_from(ROW_H).unwrap());
        let measured_h = 384;
        let reveal = scroll_offset_to_reveal(N - 1, 0, measured_h, ROW_H);
        s.scroll_to(0, reveal);
        let window = compute_visible_range(s.offset_y(), measured_h, N, ROW_H, OVERSCAN);
        assert!(
            window.indices().any(|i| i == N - 1),
            "after reveal, the last row is inside the window {window:?}",
        );
    }
}
