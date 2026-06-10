// R837 §5.38 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (WAI-ARIA, DataGridExternal, TextFieldExternal, gridcell, …).
#![allow(clippy::doc_markdown)]

//! `hello-data-grid` — R837 §5.38 §5.40 §5.50 **editable data grid**: a
//! 2-D table where every cell is editable in place by a type-appropriate
//! control (the spreadsheet / DCC data-table form factor). It is the 2-D
//! generalisation of the R836 property grid: the property grid is the 1
//! column-of-values special case, this is N typed columns × M rows.
//!
//! ## 2nd consumer of two SSOTs (the lift these consumers justify)
//!
//! * **`pinion_core::cell_value`** — the typed value model (`CellValue` /
//!   `CellKind` + kind dispatch / display / parse / the keystroke gate / the
//!   introspect read+write). R836 minted it locally as the 1st consumer; the
//!   data grid is the 2nd consumer, so it was lifted to a framework crate at
//!   this round (the `[[abstraction-needs-second-consumer]]` "lift the
//!   pure-logic model at the 2nd consumer" discipline — a divergence between
//!   the two grids' typed-value logic would be a bug, not a style choice).
//! * **`pinion_a11y::grid_table_nodes`** — the WAI-ARIA `grid` a11y skeleton
//!   (hello-table / hello-table-multi are the 1st / 2nd consumers; this is
//!   the 3rd).
//!
//! ## Architecture — two externals, the R836 edit-in-cell shape at 2-D
//!
//! * **`DataGridExternal`** (`data_grid`, primary) — the grid coordinator.
//!   Owns the flat `Signal<Vec<CellValue>>` cell model (`row * NCOLS + col`),
//!   the 2-D roving cursor (`focused_row` / `focused_col` Signals), and the
//!   edit latch (`Signal<Option<(row, col)>>`). Each column carries a fixed
//!   [`CellKind`] ([`COL_KINDS`]). Exposes the whole grid for AI-first
//!   introspection: `query value.<r>.<c>` / `col_name.<c>` / `col_kind.<c>` /
//!   `focused_row` / `focused_col` / `editing_row` / `editing_col`,
//!   `intervene value.<r>.<c>` (the deterministic typed-set path), `invoke
//!   toggle` / `begin` / `send`.
//! * **`TextFieldExternal`** (`data_grid_edit`, extra) — ONE shared inline
//!   editor reused across every editable cell (the R836 single-editor
//!   pattern; scales to any cell count). Paints only inside the cell being
//!   edited.
//!
//! ## Keyboard model (WAI-ARIA editable data grid)
//!
//! Single Tab stop with a 2-D roving cursor: `ArrowUp` / `ArrowDown` move
//! the row, `ArrowLeft` / `ArrowRight` the column, `Home` / `End` jump to the
//! first / last column (all clamped — a grid has ends). `Space` toggles a
//! bool cell; `Enter` / `F2` toggles a bool or enters edit mode on a text /
//! int / float cell (focus moves into the shared inline field). While
//! editing: `Enter` commits (parse → write back), `Escape` cancels, and int /
//! float columns gate non-numeric keystrokes. A click-away commit-on-blur
//! rides the field's `with_blur_intent`.
//!
//! ## a11y (R837 §5.40)
//!
//! A WAI-ARIA `grid` (the [`grid_table_nodes`] SSOT): a header row of column
//! names over one data `row` per record, each a row of `gridcell`s. The
//! focused cell carries the roving `focused` flag (`aria-activedescendant`);
//! the typed value is encoded in the cell name (`"Count: 24"`).
//!
//! ## Known gaps (honest carry, shared with R836)
//!
//! - Native checkbox / textbox cell roles (per-cell a11y role) — additive.
//! - Per-column validation / clamp ranges — additive.
//! - Column sort / filter / grouping / frozen panes — own substrate rounds
//!   (the read-only catalog already has sort / filter / h-scroll; folding
//!   them onto an *editable* grid is deferred).

use std::rc::Rc;

use pinion_a11y::{grid_table_nodes, AccessNode, GridCell, GridColumn, GridRow, WidgetA11y};
use pinion_core::cell_value::{CellKind, CellValue};
use pinion_core::composite_tag::GridSendKey;
use pinion_core::external::{
    int_of, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    IntrospectSchema, IntrospectValue, InterveneError, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::checkbox::CheckboxState;
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::text_edit::{use_text_edit_state, TextEditState};
use pinion_core::widgets::text_field::{TextFieldExternal, TextFieldState};
use pinion_core::{Color, Command, Frame, Modifiers, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::checkbox::{view_checkbox_box, CheckboxStyle};
use pinion_widget_paint::text_field as tf_paint;

use pinion_widget_paint::state_layer::HOVER;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloDataGridRenderer, HelloDataGridRendererError);

// ─── window + layout constants ─────────────────────────────────────

const WIN_W: u32 = 460;
const WIN_H: u32 = 320;
const THEME_TAG: &str = "app";

const TITLE_PX: u32 = 22;
const HEADER_PX: u32 = 13;
const CELL_PX: u32 = 15;

const ROW_H: u32 = 36;
const CELL_PAD: u32 = 8;
const CHECKBOX_SIZE: u32 = 18;
const PANEL_PAD: u32 = 20;
const ROW_GAP: u32 = 1;

// ─── tags + intents ───────────────────────────────────────────────

/// Primary External — the grid coordinator (the single keyboard Tab stop).
const GRID_TAG: &str = "data_grid";
/// Extra External — the one shared inline cell editor.
const EDIT_TF_TAG: &str = "data_grid_edit";
/// Commit-on-blur intent the inline field raises on a click-away (R793).
const EDIT_TF_BLUR_INTENT_TAG: &str = pinion_core::intent_tag!("data_grid_edit", "blur");


// ─── grid shape (an editable asset table) ─────────────────────────

const NROWS: usize = 4;
const NCOLS: usize = 5;

/// Column titles (the header row + the AT cell-name prefix).
const COL_NAMES: [&str; NCOLS] = ["Asset", "Type", "Count", "Scale", "Active"];

/// The per-column [`CellKind`] — every cell in a column shares its column's
/// kind (the editor dispatch, parse, keystroke gate, and intervene coercion
/// all read from here).
const COL_KINDS: [CellKind; NCOLS] = [
    CellKind::Text,
    CellKind::Text,
    CellKind::Int,
    CellKind::Float,
    CellKind::Bool,
];

/// Per-column paint width (logical px). Text columns are wider.
const COL_W: [u32; NCOLS] = [120, 90, 70, 70, 70];

/// `(row, col)` → flat model index.
fn idx(row: usize, col: usize) -> usize {
    row * NCOLS + col
}

/// First-paint cell values (row-major). Each column's values match
/// [`COL_KINDS`].
fn default_cells() -> Vec<CellValue> {
    vec![
        CellValue::Text("Hero".to_owned()), CellValue::Text("sprite".to_owned()),
        CellValue::Int(1), CellValue::Float(1.0), CellValue::Bool(true),
        CellValue::Text("Tree".to_owned()), CellValue::Text("mesh".to_owned()),
        CellValue::Int(24), CellValue::Float(2.5), CellValue::Bool(true),
        CellValue::Text("Coin".to_owned()), CellValue::Text("sprite".to_owned()),
        CellValue::Int(99), CellValue::Float(0.5), CellValue::Bool(false),
        CellValue::Text("Boss".to_owned()), CellValue::Text("mesh".to_owned()),
        CellValue::Int(1), CellValue::Float(4.0), CellValue::Bool(true),
    ]
}

// ─── reactive holders (Owner::cache, shared view ↔ coordinator) ────

#[must_use]
fn use_data_model() -> Rc<Signal<Vec<CellValue>>> {
    let owner = Owner::current().expect("use_data_model requires an active Owner scope");
    owner.cache("data_grid.model", || Signal::new(default_cells()))
}

#[must_use]
fn use_focused_row() -> Rc<Signal<usize>> {
    let owner = Owner::current().expect("use_focused_row requires an active Owner scope");
    owner.cache("data_grid.focused_row", || Signal::new(0_usize))
}

#[must_use]
fn use_focused_col() -> Rc<Signal<usize>> {
    let owner = Owner::current().expect("use_focused_col requires an active Owner scope");
    owner.cache("data_grid.focused_col", || Signal::new(0_usize))
}

/// Edit-mode latch — `Some((row, col))` while that cell is being text-edited
/// (the todomvc `editing_id`, keyed by a 2-D cell). `None` = navigating.
#[must_use]
fn use_editing_cell() -> Rc<Signal<Option<(usize, usize)>>> {
    let owner = Owner::current().expect("use_editing_cell requires an active Owner scope");
    owner.cache("data_grid.editing_cell", || Signal::new(None))
}

// ─── grid coordinator External ────────────────────────────────────

/// The data-grid coordinator. Holds `Rc` clones of the reactive holders
/// (same instances the view fn reads) + the shared editor's
/// [`TextEditState`] for [`Self::begin_edit`] seeding. Mutations write the
/// Signals directly — the R836 `PropertyGridExternal` shape at 2-D.
struct DataGridExternal {
    model: Rc<Signal<Vec<CellValue>>>,
    focused_row: Rc<Signal<usize>>,
    focused_col: Rc<Signal<usize>>,
    editing_cell: Rc<Signal<Option<(usize, usize)>>>,
    editor: Rc<TextEditState>,
}

impl DataGridExternal {
    fn new(
        model: Rc<Signal<Vec<CellValue>>>,
        focused_row: Rc<Signal<usize>>,
        focused_col: Rc<Signal<usize>>,
        editing_cell: Rc<Signal<Option<(usize, usize)>>>,
        editor: Rc<TextEditState>,
    ) -> Self {
        Self { model, focused_row, focused_col, editing_cell, editor }
    }

    /// Toggle the bool at `(row, col)`; no-op (returns `false`) unless the
    /// column is a bool. The checkbox affordance behind `Space` + click.
    fn toggle(&self, row: usize, col: usize) -> bool {
        if col >= NCOLS || COL_KINDS[col] != CellKind::Bool {
            return false;
        }
        let mut toggled = false;
        self.model.set_with(|prev| {
            let mut next = prev.clone();
            if let Some(CellValue::Bool(b)) = next.get_mut(idx(row, col)) {
                *b = !*b;
                toggled = true;
            }
            next
        });
        toggled
    }

    /// Enter edit mode on `(row, col)`: latch the cell, seed the shared
    /// editor with the formatted value (caret parked at the trailing edge),
    /// and request focus into the field. Returns `false` for a bool column
    /// (bools toggle) or an out-of-range cell.
    fn begin_edit(&self, row: usize, col: usize) -> bool {
        if row >= NROWS || col >= NCOLS || !COL_KINDS[col].is_text_editable() {
            return false;
        }
        let model = self.model.get();
        let Some(value) = model.get(idx(row, col)) else {
            return false;
        };
        self.editing_cell.set(Some((row, col)));
        // R878 — `seed` = set_text + caret-at-end (the lifted pair).
        self.editor.seed(value.edit_text());
        pinion_core::focus_request::request(EDIT_TF_TAG);
        true
    }

    fn set_focused_row_clamped(&self, row: usize) {
        self.focused_row.set(row.min(NROWS - 1));
    }

    fn set_focused_col_clamped(&self, col: usize) {
        self.focused_col.set(col.min(NCOLS - 1));
    }
}

impl core::fmt::Debug for DataGridExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DataGridExternal")
            .field("focused_row", &self.focused_row.get())
            .field("focused_col", &self.focused_col.get())
            .field("editing_cell", &self.editing_cell.get())
            .finish_non_exhaustive()
    }
}

impl External for DataGridExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Tui, Backend::Rpc], BackendFallback::Skip)
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
}

impl ExternalIntrospect for DataGridExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("row_count", "int"),
            ("col_count", "int"),
            ("focused_row", "int"),
            ("focused_col", "int"),
            ("editing_row", "int"),
            ("editing_col", "int"),
            ("col_name.<col>", "string"),
            ("col_kind.<col>", "string"),
            ("value.<row>.<col>", "json"),
            ("send", "string"),
            ("toggle", "json"),
            ("begin", "json"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "row_count" => Some(IntrospectValue::Int(int_of(NROWS))),
            "col_count" => Some(IntrospectValue::Int(int_of(NCOLS))),
            "focused_row" => Some(IntrospectValue::Int(int_of(self.focused_row.get()))),
            "focused_col" => Some(IntrospectValue::Int(int_of(self.focused_col.get()))),
            "editing_row" => Some(match self.editing_cell.get() {
                Some((row, _)) => IntrospectValue::Int(int_of(row)),
                None => IntrospectValue::Null,
            }),
            "editing_col" => Some(match self.editing_cell.get() {
                Some((_, col)) => IntrospectValue::Int(int_of(col)),
                None => IntrospectValue::Null,
            }),
            _ => {
                if let Some(col_str) = path.strip_prefix("col_name.") {
                    let col: usize = col_str.parse().ok()?;
                    return COL_NAMES.get(col).map(|n| IntrospectValue::Text((*n).to_owned()));
                }
                if let Some(col_str) = path.strip_prefix("col_kind.") {
                    let col: usize = col_str.parse().ok()?;
                    return COL_KINDS.get(col).map(|k| IntrospectValue::Text(k.name().to_owned()));
                }
                if let Some(rest) = path.strip_prefix("value.") {
                    let (row_str, col_str) = rest.split_once('.')?;
                    let row: usize = row_str.parse().ok()?;
                    let col: usize = col_str.parse().ok()?;
                    let model = self.model.get();
                    return model.get(idx(row, col)).map(CellValue::to_introspect);
                }
                None
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "row_count" | "col_count" | "editing_row" | "editing_col" => {
                Err(InterveneError::ReadOnly)
            }
            "focused_row" => match value {
                IntrospectValue::Int(i) => {
                    let row = usize::try_from(i).map_err(|_| InterveneError::TypeMismatch)?;
                    self.set_focused_row_clamped(row);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "focused_col" => match value {
                IntrospectValue::Int(i) => {
                    let col = usize::try_from(i).map_err(|_| InterveneError::TypeMismatch)?;
                    self.set_focused_col_clamped(col);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            _ => {
                let Some(rest) = path.strip_prefix("value.") else {
                    return Err(InterveneError::UnknownPath);
                };
                let (row_str, col_str) =
                    rest.split_once('.').ok_or(InterveneError::UnknownPath)?;
                let row: usize = row_str.parse().map_err(|_| InterveneError::UnknownPath)?;
                let col: usize = col_str.parse().map_err(|_| InterveneError::UnknownPath)?;
                if row >= NROWS || col >= NCOLS {
                    return Err(InterveneError::UnknownPath);
                }
                let new_value = COL_KINDS[col].coerce(value)?;
                self.model.set_with(move |prev| {
                    let mut next = prev.clone();
                    next[idx(row, col)] = new_value.clone();
                    next
                });
                Ok(())
            }
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Composite wire `"<row>_<col>:<EventName>"` (the shared
            // `GridSendKey` SSOT, the same grammar hello-table encodes /
            // decodes). PointerUp focuses the cell (and toggles a bool);
            // DoubleClick enters edit mode on an editable cell.
            "send" => match args {
                IntrospectValue::Text(ref s) => {
                    // R880.1 — the `split_send_payload` `:` grammar SSOT
                    // strips a held-modifier third segment (the hand-rolled
                    // split_once read "PointerUp:c" as the event name and a
                    // Ctrl+click on a cell was silently rejected).
                    let (key, event_name, _mods) =
                        pinion_core::composite_tag::split_send_payload(s)
                            .ok_or(InvokeError::Rejected)?;
                    let GridSendKey::Cell { row, col } =
                        GridSendKey::parse(key).ok_or(InvokeError::Rejected)?
                    else {
                        return Err(InvokeError::Rejected);
                    };
                    if row >= NROWS || col >= NCOLS {
                        return Err(InvokeError::Rejected);
                    }
                    match event_name {
                        "PointerUp" => {
                            self.focused_row.set(row);
                            self.focused_col.set(col);
                            self.toggle(row, col);
                            Ok(IntrospectValue::Null)
                        }
                        "DoubleClick" => Ok(IntrospectValue::Bool(self.begin_edit(row, col))),
                        _ => Ok(IntrospectValue::Null),
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Toggle the focused bool cell (the `Space` keyboard path + RPC).
            "toggle" => {
                let toggled = self.toggle(self.focused_row.get(), self.focused_col.get());
                Ok(IntrospectValue::Bool(toggled))
            }
            // Enter edit mode on the focused cell (the `Enter` / `F2` path).
            "begin" => {
                let started = self.begin_edit(self.focused_row.get(), self.focused_col.get());
                Ok(IntrospectValue::Bool(started))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// ─── inline-editor commit / cancel (keyboard, owner-scoped) ───────

/// Commit the in-flight edit: parse the editor text by the editing column's
/// kind and write it back to the cell. A malformed numeric commit keeps the
/// prior value (no data loss). Mirrors `todomvc::commit_edit`.
fn commit_edit(restore_focus: bool) {
    let editing = use_editing_cell();
    let Some((row, col)) = editing.get() else {
        return;
    };
    let model = use_data_model();
    let text = use_text_edit_state(EDIT_TF_TAG).text();
    if col < NCOLS {
        if let Some(parsed) = COL_KINDS[col].parse(&text) {
            model.set_with(move |prev| {
                let mut next = prev.clone();
                next[idx(row, col)] = parsed.clone();
                next
            });
        }
    }
    end_edit_mode(restore_focus);
}

fn cancel_edit() {
    end_edit_mode(true);
}

fn end_edit_mode(restore_focus: bool) {
    use_editing_cell().set(None);
    use_text_edit_state(EDIT_TF_TAG).set_text(String::new());
    if restore_focus {
        pinion_core::focus_request::request(GRID_TAG);
    }
}

/// The kind of the column currently being edited (`None` when not editing) —
/// drives the int / float keystroke gate.
fn editing_col_kind() -> Option<CellKind> {
    let (_, col) = use_editing_cell().get()?;
    COL_KINDS.get(col).copied()
}

// ─── keyboard ─────────────────────────────────────────────────────

/// Grid-focused keymap: 2-D roving navigation + activate.
fn apply_key_grid(scene: &mut Scene, key: &str) -> bool {
    let row_sig = use_focused_row();
    let col_sig = use_focused_col();
    let row = row_sig.get().min(NROWS - 1);
    let col = col_sig.get().min(NCOLS - 1);
    match key {
        "ArrowDown" => {
            row_sig.set((row + 1).min(NROWS - 1));
            true
        }
        "ArrowUp" => {
            row_sig.set(row.saturating_sub(1));
            true
        }
        "ArrowRight" => {
            col_sig.set((col + 1).min(NCOLS - 1));
            true
        }
        "ArrowLeft" => {
            col_sig.set(col.saturating_sub(1));
            true
        }
        "Home" => {
            col_sig.set(0);
            true
        }
        "End" => {
            col_sig.set(NCOLS - 1);
            true
        }
        "Space" => activate_focused(scene, col, false),
        "Enter" | "F2" => activate_focused(scene, col, true),
        _ => false,
    }
}

/// Activate the focused cell: toggle a bool, or (when `allow_edit`) enter
/// edit mode on a text / int / float cell. Routes through the coordinator's
/// `invoke` so toggle / begin live in one place (the RPC path).
fn activate_focused(scene: &mut Scene, col: usize, allow_edit: bool) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(GRID_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    if COL_KINDS.get(col).copied() == Some(CellKind::Bool) {
        intro.invoke("toggle", IntrospectValue::Null).is_ok()
    } else if allow_edit {
        intro.invoke("begin", IntrospectValue::Null).is_ok()
    } else {
        false
    }
}

/// Edit-mode keymap over the shared inline field — the lifted
/// [`pinion_core::edit_field_keymap`] SSOT (R878; this binding carried one
/// of the two pre-lift copies). Commit / cancel stay binding policy; a
/// defensive "no cell is editing" resolves to [`CellKind::Bool`] (accepts
/// no keystroke), so only commit / cancel / caret keys remain meaningful.
fn apply_key_edit(scene: &mut Scene, key: &str, modifiers: Modifiers) -> bool {
    let kind = editing_col_kind().unwrap_or(CellKind::Bool);
    pinion_core::edit_field_keymap(
        scene,
        EDIT_TF_TAG,
        key,
        modifiers,
        kind,
        || commit_edit(true),
        cancel_edit,
    )
}

// ─── paint ────────────────────────────────────────────────────────

/// Focused-cell background = the M3 `OnSurface` state-layer over the surface.
fn cell_fill(theme: &Theme, focused: bool) -> Color {
    if focused {
        theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::OnSurface), HOVER)
    } else {
        Color::TRANSPARENT
    }
}

/// Cell-sized M3 checkbox-box style. The bool cell renders the lifted
/// `view_checkbox_box` SSOT non-interactively (the grid coordinator owns the
/// toggle, so there is no per-cell `CheckboxExternal`) — one M3 checkbox
/// rendering across the catalog instead of a hand-rolled copy.
fn cell_checkbox_style() -> CheckboxStyle {
    CheckboxStyle { box_size: CHECKBOX_SIZE, glyph_size_px: 14, ..CheckboxStyle::m3_filled() }
}

/// One cell: tagged `data_grid#<row>_<col>` (the `GridSendKey` encoding) so a
/// click routes to the coordinator. Paints the shared inline field while
/// editing, else a checkbox (bool) or the value text.
fn view_cell(
    row: usize,
    col: usize,
    value: &CellValue,
    focused: bool,
    edit_active: bool,
    theme: &Theme,
    edit_field: (TextFieldState, u32),
) -> Scene {
    let inner = if edit_active {
        let style = tf_paint::TextFieldStyle {
            field_w: COL_W[col] - CELL_PAD,
            field_h: ROW_H - 6,
            ..tf_paint::TextFieldStyle::m3_filled()
        };
        tf_paint::view_field(EDIT_TF_TAG, edit_field.0, edit_field.1, theme, &style, "")
    } else if COL_KINDS[col] == CellKind::Bool {
        let checked = matches!(value, CellValue::Bool(true));
        view_checkbox_box(checked, CheckboxState::Idle, theme, &cell_checkbox_style())
    } else {
        Scene::Text(TextNode::styled(
            value.display(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(CELL_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))
    };
    Scene::Container(
        ContainerNode::new(vec![inner])
            .with_tag(format!("{GRID_TAG}#{}", GridSendKey::Cell { row, col }.encode()))
            .with_style(BoxStyle::filled(cell_fill(theme, focused)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_padding(Rect::new(CELL_PAD, 0, CELL_PAD, 0))
                    .with_size(Size::px(COL_W[col], ROW_H)),
            ),
    )
}

/// The column-header row.
fn view_header(theme: &Theme) -> Scene {
    let cells: Vec<Scene> = COL_NAMES
        .iter()
        .enumerate()
        .map(|(col, label)| {
            Scene::Container(
                ContainerNode::new(vec![Scene::Text(TextNode::styled(
                    *label,
                    Rect::default(),
                    TextStyle::new()
                        .with_size_px(HEADER_PX)
                        .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
                ))])
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_padding(Rect::new(CELL_PAD, 0, CELL_PAD, 0))
                        .with_size(Size::px(COL_W[col], ROW_H)),
                ),
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(cells)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row).with_align_items(AlignItems::Center)),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: RootState, _frame: &Frame) -> Scene {
    let (edit_state, edit_caret) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let model = use_data_model().get();
    let focused_row = use_focused_row().get();
    let focused_col = use_focused_col().get();
    let editing = use_editing_cell().get();

    let title = Scene::Text(TextNode::styled(
        "Asset table",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let mut rows: Vec<Scene> = Vec::with_capacity(NROWS + 1);
    rows.push(view_header(&theme));
    for row in 0..NROWS {
        let cells: Vec<Scene> = (0..NCOLS)
            .map(|col| {
                let value = &model[idx(row, col)];
                let focused = row == focused_row && col == focused_col;
                let edit_active = editing == Some((row, col)) && COL_KINDS[col].is_text_editable();
                view_cell(row, col, value, focused, edit_active, &theme, (edit_state, edit_caret))
            })
            .collect();
        rows.push(Scene::Container(
            ContainerNode::new(cells)
                .with_layout(LayoutStyle::new().flex(FlexDirection::Row).with_align_items(AlignItems::Center)),
        ));
    }
    let grid = Scene::Container(
        ContainerNode::new(rows)
            .with_tag(GRID_TAG)
            .with_aria_label("Asset table")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)).with_border(
                Border::new(theme.resolve(ColorRole::Outline), 1),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_gap(ROW_GAP),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![title, grid])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_padding(Rect::new(PANEL_PAD, PANEL_PAD, PANEL_PAD, PANEL_PAD))
                    .with_gap(ROW_GAP * 12)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

// ─── WidgetCore impl ──────────────────────────────────────────────

/// Cached paint posture — only the shared inline field's interaction state +
/// caret. The model / cursor / edit-mode are read reactively in the view fn.
type RootState = (TextFieldState, u32);

struct DataGridView;

impl WidgetCore for DataGridView {
    type State = RootState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let model = use_data_model();
        let focused_row = use_focused_row();
        let focused_col = use_focused_col();
        let editing = use_editing_cell();
        let editor = use_text_edit_state(EDIT_TF_TAG);
        Box::new(DataGridExternal::new(model, focused_row, focused_col, editing, editor))
    }

    fn tag() -> &'static str {
        GRID_TAG
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        let editor_state = use_text_edit_state(EDIT_TF_TAG);
        let blink = use_caret_blink(EDIT_TF_TAG);
        vec![ExtraExternal::new(
            EDIT_TF_TAG,
            Box::new(
                TextFieldExternal::new()
                    .attach_state(editor_state)
                    .attach_blink(blink)
                    .with_blur_intent(),
            ),
        )]
    }

    fn read_state(scene: &Scene) -> RootState {
        tf_paint::read_text_field_state(scene, EDIT_TF_TAG)
    }

    fn view(state: RootState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-data-grid (R837 §5.38 editable data grid)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn focusable_tags() -> Vec<&'static str> {
        vec![GRID_TAG, EDIT_TF_TAG]
    }

    /// R793 §5.38 — commit-on-blur: the inline editor lost focus (a click
    /// elsewhere) while editing → commit without restoring focus. The
    /// `editing_cell` gate makes the post-commit blur a no-op.
    fn update(_state: RootState, intent: &pinion_core::Intent) -> Vec<Command> {
        if intent.tag_str() == EDIT_TF_BLUR_INTENT_TAG && use_editing_cell().get().is_some() {
            commit_edit(false);
        }
        Vec::new()
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        match focused {
            Some(GRID_TAG) => apply_key_grid(scene, key),
            Some(EDIT_TF_TAG) => apply_key_edit(scene, key, modifiers),
            _ => false,
        }
    }

    /// Route IME composition to the inline editor while it owns focus —
    /// through the lifted R764.1 SSOT (R878 audit replaced a hand-rolled
    /// copy of the same reformat block).
    fn apply_composition(
        scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        if focused != Some(EDIT_TF_TAG) {
            return false;
        }
        tf_paint::forward_composition_to_field(scene, EDIT_TF_TAG, event)
    }
}

impl WidgetA11y for DataGridView {
    /// R837 §5.40 — the grid lowers through the lifted [`grid_table_nodes`]
    /// SSOT (3rd consumer). A column-name header over one row per record; the
    /// focused cell carries the roving `focused` flag (`aria-activedescendant`).
    fn access_node(_state: &RootState, _focused: Option<&str>) -> Vec<AccessNode> {
        let model = use_data_model().get();
        let focused_row = use_focused_row().get();
        let focused_col = use_focused_col().get();
        let columns: Vec<GridColumn> = COL_NAMES
            .iter()
            .enumerate()
            .map(|(col, label)| GridColumn {
                tag: format!("dg_col{col}"),
                label: (*label).to_owned(),
                sort: None,
            })
            .collect();
        let rows: Vec<GridRow> = (0..NROWS)
            .map(|row| GridRow {
                tag: format!("dg_row{row}"),
                selected: false,
                state: RadioState::Idle,
                cells: (0..NCOLS)
                    .map(|col| GridCell {
                        tag: format!("{GRID_TAG}#{}", GridSendKey::Cell { row, col }.encode()),
                        name: format!("{}: {}", COL_NAMES[col], model[idx(row, col)].display()),
                        focused: row == focused_row && col == focused_col,
                    })
                    .collect(),
            })
            .collect();
        grid_table_nodes(GRID_TAG, "Asset table", false, "dg_header", &columns, &rows)
    }
}

impl WidgetView for DataGridView {
    type Renderer = HelloDataGridRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<DataGridView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    fn boot_scene() -> Scene {
        let mut children = vec![Scene::External(
            ExternalNode::new(DataGridView::create_external()).with_tag(GRID_TAG),
        )];
        for extra in DataGridView::create_extra_externals() {
            children.push(Scene::External(ExternalNode::new(extra.handle).with_tag(extra.tag)));
        }
        Scene::Container(ContainerNode::new(children))
    }

    fn grid_intro(scene: &Scene) -> &dyn ExternalIntrospect {
        scene
            .find_external_with_tag(GRID_TAG)
            .and_then(|n| n.handle.introspect())
            .expect("grid external present")
    }

    #[test]
    fn r837_shape_and_defaults() {
        assert_eq!(default_cells().len(), NROWS * NCOLS);
        Owner::new().run(|| {
            let scene = boot_scene();
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("row_count"), Some(IntrospectValue::Int(4)));
            assert_eq!(intro.query("col_count"), Some(IntrospectValue::Int(5)));
            assert_eq!(intro.query("col_name.0"), Some(IntrospectValue::Text("Asset".to_owned())));
            assert_eq!(intro.query("col_kind.2"), Some(IntrospectValue::Text("int".to_owned())));
            assert_eq!(intro.query("col_kind.4"), Some(IntrospectValue::Text("bool".to_owned())));
            assert_eq!(intro.query("value.1.0"), Some(IntrospectValue::Text("Tree".to_owned())));
            assert_eq!(intro.query("value.1.2"), Some(IntrospectValue::Int(24)));
            assert_eq!(intro.query("value.2.4"), Some(IntrospectValue::Bool(false)));
            assert_eq!(intro.query("value.9.9"), None, "out-of-range -> None");
            assert_eq!(intro.query("editing_row"), Some(IntrospectValue::Null));
        });
    }

    #[test]
    fn r837_intervene_typed_value_strict() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert!(intro.intervene("value.0.2", IntrospectValue::Int(7)).is_ok());
            assert_eq!(
                intro.intervene("value.0.2", IntrospectValue::Text("no".to_owned())),
                Err(InterveneError::TypeMismatch),
                "int column rejects text",
            );
            assert!(intro.intervene("value.3.3", IntrospectValue::Float(9.5)).is_ok());
            assert_eq!(intro.query("value.0.2"), Some(IntrospectValue::Int(7)));
            assert_eq!(intro.query("value.3.3"), Some(IntrospectValue::Float(9.5)));
        });
    }

    #[test]
    fn r837_intervene_focus_clamps_both_axes() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert!(intro.intervene("focused_row", IntrospectValue::Int(99)).is_ok());
            assert!(intro.intervene("focused_col", IntrospectValue::Int(99)).is_ok());
            assert_eq!(intro.query("focused_row"), Some(IntrospectValue::Int(3)));
            assert_eq!(intro.query("focused_col"), Some(IntrospectValue::Int(4)));
        });
    }

    #[test]
    fn r837_click_focuses_cell_and_toggles_bool() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Cell (2,4) is the Active bool = false.
            let _ = intro.invoke("send", IntrospectValue::Text("2_4:PointerUp".to_owned()));
            assert_eq!(intro.query("focused_row"), Some(IntrospectValue::Int(2)));
            assert_eq!(intro.query("focused_col"), Some(IntrospectValue::Int(4)));
            assert_eq!(intro.query("value.2.4"), Some(IntrospectValue::Bool(true)), "toggled");
            // A click on a text cell focuses but does not toggle.
            let _ = intro.invoke("send", IntrospectValue::Text("0_0:PointerUp".to_owned()));
            assert_eq!(intro.query("value.0.0"), Some(IntrospectValue::Text("Hero".to_owned())));
        });
    }

    #[test]
    fn r837_double_click_begins_edit_on_editable_cell() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.invoke("send", IntrospectValue::Text("1_2:DoubleClick".to_owned())),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(intro.query("editing_row"), Some(IntrospectValue::Int(1)));
            assert_eq!(intro.query("editing_col"), Some(IntrospectValue::Int(2)));
            assert_eq!(use_text_edit_state(EDIT_TF_TAG).text(), "24", "seeded with the int value");
            // Double-click on a bool cell does not edit.
            assert_eq!(
                intro.invoke("send", IntrospectValue::Text("0_4:DoubleClick".to_owned())),
                Ok(IntrospectValue::Bool(false)),
            );
        });
    }

    #[test]
    fn r837_begin_commit_writes_back_parsed_value() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.intervene("focused_row", IntrospectValue::Int(1));
            let _ = intro.intervene("focused_col", IntrospectValue::Int(2));
            assert_eq!(intro.invoke("begin", IntrospectValue::Null), Ok(IntrospectValue::Bool(true)));
            use_text_edit_state(EDIT_TF_TAG).set_text("250".to_owned());
            commit_edit(true);
            assert_eq!(grid_intro(&scene).query("value.1.2"), Some(IntrospectValue::Int(250)));
            assert_eq!(grid_intro(&scene).query("editing_row"), Some(IntrospectValue::Null));
        });
    }

    #[test]
    fn r837_commit_malformed_reverts() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            let _ = intro.intervene("focused_col", IntrospectValue::Int(3));
            let _ = intro.invoke("begin", IntrospectValue::Null);
            use_text_edit_state(EDIT_TF_TAG).set_text("xyz".to_owned());
            commit_edit(true);
            assert_eq!(grid_intro(&scene).query("value.0.3"), Some(IntrospectValue::Float(1.0)));
        });
    }

    #[test]
    fn r837_keyboard_roves_both_axes_and_clamps() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let m = Modifiers::empty();
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowRight", m));
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", m));
            assert_eq!(use_focused_row().get(), 1);
            assert_eq!(use_focused_col().get(), 1);
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "End", m));
            assert_eq!(use_focused_col().get(), NCOLS - 1);
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowRight", m));
            assert_eq!(use_focused_col().get(), NCOLS - 1, "clamps at the last column");
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "Home", m));
            assert_eq!(use_focused_col().get(), 0);
        });
    }

    #[test]
    fn r837_space_toggles_bool_enter_edits_number() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let m = Modifiers::empty();
            // Focus the Active bool of row 0 (col 4) and Space-toggle.
            let _ = scene
                .find_external_with_tag_mut(GRID_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .map(|i| {
                    let _ = i.intervene("focused_row", IntrospectValue::Int(0));
                    i.intervene("focused_col", IntrospectValue::Int(4))
                });
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "Space", m));
            assert_eq!(grid_intro(&scene).query("value.0.4"), Some(IntrospectValue::Bool(false)));
            // Focus the Count int of row 0 (col 2) and Enter -> edit mode.
            let _ = scene
                .find_external_with_tag_mut(GRID_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .map(|i| i.intervene("focused_col", IntrospectValue::Int(2)));
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "Enter", m));
            assert_eq!(grid_intro(&scene).query("editing_col"), Some(IntrospectValue::Int(2)));
        });
    }

    #[test]
    fn r837_edit_float_gate_allows_dot_drops_letter() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            let _ = intro.intervene("focused_col", IntrospectValue::Int(3)); // Scale (float)
            let _ = intro.invoke("begin", IntrospectValue::Null);
            use_text_edit_state(EDIT_TF_TAG).set_text(String::new());
            use_text_edit_state(EDIT_TF_TAG).set_caret(0);
            let m = Modifiers::empty();
            assert!(DataGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), "2", m));
            assert!(DataGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), ".", m), "float accepts dot");
            assert!(DataGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), "5", m));
            assert!(!DataGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), "z", m), "letter dropped");
            assert_eq!(use_text_edit_state(EDIT_TF_TAG).text(), "2.5");
        });
    }

    #[test]
    fn r837_access_node_emits_grid_with_active_cell() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            use_focused_row().set(2);
            use_focused_col().set(2);
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            // grid + header row + 5 columnheaders + 4 rows + 20 cells.
            assert_eq!(nodes.len(), 1 + 1 + NCOLS + NROWS + NROWS * NCOLS);
            assert_eq!(nodes[0].role, pinion_a11y::AriaRole::Grid);
            let active = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#2_2"))
                .expect("focused cell present");
            assert!(active.state.focused, "the focused cell is the active descendant");
            assert_eq!(active.name.as_deref(), Some("Count: 99"));
        });
    }

    #[test]
    fn r837_view_carries_grid_and_cell_tags() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let scene = view((TextFieldState::Idle, 0), &Frame::new());
            assert!(scene.contains_tag(GRID_TAG), "grid root painted");
            assert!(scene.contains_tag(&format!("{GRID_TAG}#0_0")), "cell (0,0) painted");
            assert!(scene.contains_tag(&format!("{GRID_TAG}#3_4")), "cell (3,4) painted");
            assert!(!scene.contains_tag(EDIT_TF_TAG), "no inline field when not editing");
        });
    }

    #[test]
    fn r837_view_contains_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<DataGridView>(
            (TextFieldState::Idle, 0),
            &Frame::default(),
        );
    }
}
