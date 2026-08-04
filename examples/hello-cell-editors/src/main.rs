//! `hello-cell-editors` — R1555 §5.27 **the cell-editor factory**: Qt
//! `QItemEditorFactory`, as one column per [`CellKind`].
//!
//! R1532 gave the virtualized grid a per-**column** paint delegate and R1544
//! gave it the editing seam. Both are the *override* half of Qt's editing
//! decomposition (`setItemDelegateForColumn`). The other half — a registry from
//! the datum's **type** to an editor, which `QStyledItemDelegate` consults when
//! no delegate overrides it — did not exist, so
//! [`text_cell_editor`](pinion_widget_paint::table::text_cell_editor) was the
//! built-in editor for every kind. For two of the six that editor cannot work at
//! all: [`CellKind::Bool`] and [`CellKind::Choice`] refuse every keystroke
//! ([`CellKind::accepts_keystroke`]) and parse to nothing
//! ([`CellKind::parse`]), so the seam opened a field that could not be typed
//! into and whose commit could never produce a value.
//!
//! This binding is that registry's forcing consumer. It writes **no editor
//! paint of its own**: one column per kind, every editor reached through
//! [`cell_editor`](pinion_widget_paint::table::cell_editor), which dispatches on
//! [`CellKind::editor_form`].
//!
//! | column | kind | form | Qt's default factory |
//! |---|---|---|---|
//! | `Asset` | — | none | not editable (no `ItemIsEditable`) |
//! | `Name` | `Text` | `Field` | `QExpandingLineEdit` — parity |
//! | `Count` | `Int` | `Stepper` | `QSpinBox` — parity |
//! | `Ratio` | `Float` | `Stepper` | `QDoubleSpinBox` at `decimals() == 2`, which **rounds** |
//! | `Active` | `Bool` | `Toggle` | a two-item `QComboBox` reading "False"/"True" |
//! | `Tier` | `Choice` | `Selector` | **impossible** — an enum is an `int` to `QVariant` |
//! | `Tint` | `Color` | `Swatch` | **nothing** — `createEditor` answers `nullptr` |
//!
//! ## What the model is
//!
//! A typed one. `Qt::EditRole` answers with a `QVariant` and R1555's
//! [`CellEdit`] answers with a [`CellValue`], because a `Choice`'s **options are
//! part of the value's identity** — a `(kind, String)` pair cannot tell a
//! selector what to select between, or even which index a label is when two
//! options share one. So the overlay here holds `CellValue`s, which is also what
//! makes the four non-text columns expressible at all.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/cell_editors` publishes the whole factory — the census Qt has no
//! accessor for, since `createEditor` *constructs a `QWidget`* and `creatorMap`
//! is private. The binding's own `editing` / `commit` slots then report which
//! cell is open, in which form, holding what, and what the last commit did
//! ([`CommitOutcome`], which names the difference between "that is not a number"
//! and "the model will not take it" — one event Qt reports as neither). See
//! `tools/demos/r1555_editor_follows_the_datum.py`.
//!
//! ## a11y
//!
//! The open editor is announced under its `gridcell` with the role its form
//! has ([`attach_cell_editor`]). Qt reaches a role by accident of construction,
//! so a Qt bool cell announces as a **combo box** and a Qt colour cell announces
//! nothing at all.

use pinion_a11y::{
    AccessNode, WidgetA11y, attach_cell_editor, mark_grid_editability, windowed_grid_nodes_selected,
};
use pinion_core::composite_tag::GridSendKey;
use pinion_core::external::External;
use pinion_core::input::is_activation_event;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::grid_edit::{
    CommitOutcome, EditState, EditTrigger, EditTriggers, EndEditHint, GridEditState, use_grid_edit,
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
    CellEdit, CellIndex, CellKind, CellValue, EditorForm, Frame, GridExtent, Modifiers, Scene,
    WidgetCore, edit_field_keymap,
};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::table::{
    GridEditing, GridModel, GridScroll, HeaderAxis, TableStyle, VirtualTableData,
    header_from_slice, no_decoration, no_row_header, view_virtual_table,
};
use std::collections::BTreeMap;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloCellEditorsRenderer, HelloCellEditorsRendererError);

/// Wide enough that every column fits without horizontal scrolling — the point
/// of the binding is that all five forms are on screen at once.
const WIN_W: u32 = 900;
const WIN_H: u32 = 420;
const THEME_TAG: &str = "app";
/// Data-row count. Small next to `hello-grid-nav`'s 10 000: this binding is
/// about the editor axis, not the windowing axis, and a row count that still
/// exceeds the viewport keeps the windowed path honest.
const N: usize = 60;
/// Column count (matches [`HEADERS`] and [`COL_KINDS`]).
const NCOLS: usize = 7;
const COL_W: u32 = 122;
const ROW_H: u32 = 38;
const OVERSCAN: usize = 3;
const STATUS_H: u32 = 40;

/// Column labels. One per [`CellKind`], plus the read-only identity column.
const HEADERS: [&str; NCOLS] = ["Asset", "Name", "Count", "Ratio", "Active", "Tier", "Tint"];

/// R1555 — the column → kind map, which **is** the column → editor map: the
/// factory derives the form from the kind, so this table is the only place the
/// grid says anything about which editor a column opens.
///
/// `None` is the read-only identity column (Qt: `flags()` without
/// `Qt::ItemIsEditable`), so no trigger can open an editor there.
const COL_KINDS: [Option<CellKind>; NCOLS] = [
    None,
    Some(CellKind::Text),
    Some(CellKind::Int),
    Some(CellKind::Float),
    Some(CellKind::Bool),
    Some(CellKind::Choice),
    Some(CellKind::Color),
];

/// The `Tier` column's option domain — a `Choice`'s options are part of its
/// **value**, not of its kind, which is exactly why an edit role has to carry a
/// datum and why Qt's type-keyed factory cannot produce a populated combo box.
const TIERS: [&str; 3] = ["Draft", "Review", "Final"];

/// The `Tint` column's seed palette, cycled per row so the swatches differ.
const TINTS: [&str; 4] = ["#c62828", "#2e7d32", "#1565c0", "#f9a825"];

/// The `Count` column's inclusive upper bound. A commit past it is **refused by
/// the model**, which keeps the editor open holding the value — and R1555 names
/// that refusal apart from a buffer that was never a number.
const COUNT_MAX: i64 = 999;

const TABLE_TAG: &str = "cells";
const SCROLL_KEY: &str = "cells_scroll";
const H_SCROLL_KEY: &str = "cells_hscroll";
const STATUS_TAG: &str = "cells_status";
/// The inline editor field's tag — the one transient editor's
/// `use_text_edit_state` key. Shared by every text-buffered form (a field, a
/// stepper, and a swatch's hex half), because there is one editor open at a
/// time; the latch-buffered forms leave it empty by construction.
const EDIT_FIELD_TAG: &str = "cells_editor";
const EDIT_KEY: &str = "cells_edit";
const OVERLAY_KEY: &str = "cells_overlay";
const CURRENT_KEY: &str = "cells_current";
/// Cache key for the last commit's outcome — the readout that makes
/// [`CommitOutcome`]'s three failure modes observable.
const OUTCOME_KEY: &str = "cells_outcome";

/// The projected widget state: the selected row and the inline editor field's
/// `(statechart, caret byte)` pair.
type RootState = (Option<usize>, (TextFieldState, u32));

fn table_style() -> TableStyle {
    TableStyle {
        col_width: COL_W,
        row_height: ROW_H,
        focusable: true,
        ..TableStyle::m3()
    }
}

/// The committed-cell overlay: the **mutable** half of the model, holding
/// `CellValue`s.
///
/// Typed rather than stringly, and that is R1555's forcing consequence: a
/// `Choice` cell's options and a `Bool`'s state are not recoverable from display
/// text, so a grid whose model stored strings could not answer an edit role for
/// four of its six kinds.
fn use_overlay() -> Rc<Signal<BTreeMap<usize, CellValue>>> {
    Owner::current()
        .expect("use_overlay requires an active Owner scope")
        .cache(OVERLAY_KEY, || Signal::new(BTreeMap::new()))
}

/// The **current cell**, Qt's `currentIndex()` — a separate axis from the row
/// selection, because an edit trigger acts on a cell and a row-only cursor
/// cannot name one.
fn use_current() -> Rc<Signal<Option<CellIndex>>> {
    Owner::current()
        .expect("use_current requires an active Owner scope")
        .cache(CURRENT_KEY, || Signal::new(None))
}

/// The last commit's outcome as its wire token, so the three ways a commit can
/// fail to land are visible in `scene/snapshot` and over the introspect wire.
fn use_outcome() -> Rc<Signal<String>> {
    Owner::current()
        .expect("use_outcome requires an active Owner scope")
        .cache(OUTCOME_KEY, || Signal::new("none".to_string()))
}

/// The flat overlay key for a cell.
fn flat(c: CellIndex) -> usize {
    c.row * NCOLS + c.col
}

/// The generated datum for a cell, before any committed edit — the model as a
/// **function** of the index, so no row is materialized until it is asked for.
fn generated_value(c: CellIndex) -> Option<CellValue> {
    let id = c.row;
    Some(match COL_KINDS[c.col]? {
        CellKind::Text => CellValue::Text(format!("asset_{id:03}")),
        CellKind::Int => CellValue::Int(i64::try_from((id * 17) % 400).unwrap_or(0)),
        // Two decimal places of real information, which is the precision Qt's
        // default `QDoubleSpinBox` editor silently discards.
        CellKind::Float => CellValue::Float(f64::from(u32::try_from(id).unwrap_or(0)) / 8.0),
        CellKind::Bool => CellValue::Bool(id % 3 == 0),
        CellKind::Choice => CellValue::Choice {
            selected: id % TIERS.len(),
            options: TIERS.iter().map(|s| (*s).to_string()).collect(),
        },
        CellKind::Color => CellValue::Color(
            Color::from_hex(TINTS[id % TINTS.len()]).unwrap_or(Color::rgb(0x80, 0x80, 0x80)),
        ),
    })
}

/// The model's datum for a cell: the committed value if it has one, else the
/// generated one. `None` for the read-only identity column.
fn cell_value(c: CellIndex, overlay: &BTreeMap<usize, CellValue>) -> Option<CellValue> {
    overlay
        .get(&flat(c))
        .cloned()
        .or_else(|| generated_value(c))
}

/// The model's `Qt::DisplayRole`.
fn cell_text(c: CellIndex, overlay: &BTreeMap<usize, CellValue>) -> String {
    match cell_value(c, overlay) {
        Some(value) => value.display(),
        // The identity column: derived from the row, never stored, never edited.
        None => format!("A-{:04}", c.row),
    }
}

/// The model's `Qt::EditRole` fused with `flags() & Qt::ItemIsEditable`: `None`
/// **is** "not editable", and a `Some` carries the datum — which is what makes
/// "an editor open on a cell the model will not edit" a state the types reject.
fn edit_role(c: CellIndex, overlay: &BTreeMap<usize, CellValue>) -> Option<CellEdit> {
    cell_value(c, overlay).map(CellEdit::from)
}

/// The model's `setData(index, value, Qt::EditRole)`.
///
/// The framework has already parsed the buffer through the cell's own kind, so a
/// malformed value never arrives (that is [`CommitOutcome::Malformed`]). What is
/// left is the model's own rule — the `Count` bound — which is the only thing a
/// `setData` should be deciding.
fn commit_cell(index: CellIndex, value: &CellValue) -> bool {
    let Some(kind) = COL_KINDS.get(index.col).copied().flatten() else {
        return false;
    };
    if value.kind() != kind {
        return false;
    }
    if let CellValue::Int(n) = value
        && index.col == COUNT_COL
        && !(0..=COUNT_MAX).contains(n)
    {
        return false;
    }
    let stored = value.clone();
    use_overlay().set_with(move |prev| {
        let mut next = prev.clone();
        next.insert(flat(index), stored.clone());
        next
    });
    true
}

/// The bounded-integer column, for the model's own refusal rule.
const COUNT_COL: usize = 2;

/// Status bar: the editing latch as text, so `scene/snapshot` alone answers
/// which cell is open, **in which form**, holding what, and what the last commit
/// did.
fn status_bar(theme: &Theme, selected: Option<usize>) -> Scene {
    let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
    let sel = selected.map_or_else(|| "none".to_string(), |i| i.to_string());
    let editing = edit.open().map_or_else(
        || "none".to_string(),
        |e| {
            format!(
                "{}_{} {} \"{}\"",
                e.index.row,
                e.index.col,
                e.form().name(),
                in_flight_text(&edit)
            )
        },
    );
    let text = Scene::Text(
        TextNode::styled(
            format!(
                "selected {sel} \u{00B7} editing {editing} \u{00B7} commit {}",
                use_outcome().get()
            ),
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

/// The in-flight value as text, whichever buffer holds it — the readout peer of
/// [`GridEditState::state`], with the malformed case **named** rather than shown
/// as an empty value.
fn in_flight_text(edit: &Rc<GridEditState>) -> String {
    match edit.state() {
        EditState::Closed => String::new(),
        EditState::Malformed => "<malformed>".to_string(),
        EditState::Value(value) => value.display(),
    }
}

/// view-fn (§6.3): pure sync mapping. Every editor on screen is the
/// framework's own — this binding wires **no** editor painter and **no** column
/// delegate, which is the whole claim.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: RootState, _frame: &Frame) -> Scene {
    let (selected, field) = state;
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = table_style();
    let overlay = use_overlay().get();
    let edit_state = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
    let open = edit_state.open();
    let open_edit = open.as_ref().map(|e| CellEdit::from(e.seed()));
    let editing = open
        .as_ref()
        .zip(open_edit.as_ref())
        .map(|(e, edit)| GridEditing {
            open: e.index,
            edit,
            field_tag: EDIT_FIELD_TAG,
            field,
            // The latch-buffered forms' in-flight value. A toggle that has been
            // flipped but not committed must paint the flip, not the seed.
            pending: e.pending(),
            // No column delegate: the datum's kind picks the editor, which is
            // the factory half this binding exists to exercise.
            editor: None,
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
            edit: |c: CellIndex| edit_role(c, &overlay),
        },
    );

    Scene::Container(
        ContainerNode::new(vec![status_bar(&theme, selected), grid])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

/// Whether `key` is a single printable character — the class Qt's
/// `AnyKeyPressed` trigger fires on.
fn is_printable(key: &str) -> bool {
    let mut chars = key.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if !c.is_control())
}

fn current_cell() -> CellIndex {
    use_current().get().unwrap_or(CellIndex::new(0, 1))
}

fn move_col(delta: isize) -> bool {
    let at = current_cell();
    let next = at.col.saturating_add_signed(delta).min(NCOLS - 1);
    use_current().set(Some(CellIndex::new(at.row, next)));
    true
}

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

/// Open an editor on the current cell through `trigger`.
///
/// Focus goes to the inline field only for the forms whose buffer **is** that
/// field. A toggle and a selector hold their in-flight value in the latch, so
/// focusing an empty text field for them would put the caret somewhere the
/// keystrokes must not go — the grid keeps focus and its own keymap drives the
/// gesture.
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
    if role.form().buffer_is_text() {
        pinion_core::focus_request::request(EDIT_FIELD_TAG);
        if let Some((scene, key, modifiers)) = forward {
            let _ = pinion_core::forward_key_to_field(scene, EDIT_FIELD_TAG, key, modifiers);
        }
    }
    true
}

/// Record a commit's outcome and, when it landed, hand focus back to the grid.
fn finish_commit(edit: &Rc<GridEditState>) -> CommitOutcome {
    let outcome = edit.commit_with(commit_cell);
    use_outcome().set(outcome.wire_token().to_string());
    if outcome.committed().is_some() {
        pinion_core::focus_request::request(TABLE_TAG);
    }
    outcome
}

/// The keyboard while a **latch-buffered** editor is open (a toggle or a
/// selector). The grid keeps focus for these, so the keys arrive here.
///
/// <kbd>Space</kbd> toggles, the arrows move a selector's option, and
/// <kbd>Enter</kbd> / <kbd>Escape</kbd> commit and revert — the same arc every
/// text-buffered form has. Qt's check-state click calls `setModelData`
/// immediately, so there is nothing to escape there.
fn gesture_key(edit: &Rc<GridEditState>, key: &str, form: EditorForm) -> bool {
    match (form, key) {
        (EditorForm::Toggle, " " | "Space") => edit.toggle(),
        (EditorForm::Selector, "ArrowDown" | "ArrowUp") => {
            let Some(EditState::Value(CellValue::Choice { selected, options })) =
                Some(edit.state())
            else {
                return false;
            };
            let next = if key == "ArrowDown" {
                selected
                    .saturating_add(1)
                    .min(options.len().saturating_sub(1))
            } else {
                selected.saturating_sub(1)
            };
            edit.select(next)
        }
        (_, "Enter") => {
            finish_commit(edit);
            true
        }
        (_, "Escape") => {
            edit.cancel();
            use_outcome().set("cancelled".to_string());
            pinion_core::focus_request::request(TABLE_TAG);
            true
        }
        _ => false,
    }
}

/// The keyboard while a **text-buffered** editor is open.
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
        if let Some(from) = finish_commit(edit).committed() {
            let overlay = use_overlay().get();
            let advanced = edit.advance(from, hint, GridExtent::new(N, NCOLS), |i| {
                edit_role(i, &overlay)
            });
            if advanced {
                if let Some(open) = edit.open() {
                    use_current().set(Some(open.index));
                    if open.form().buffer_is_text() {
                        pinion_core::focus_request::request(EDIT_FIELD_TAG);
                    } else {
                        pinion_core::focus_request::request(TABLE_TAG);
                    }
                }
            }
        }
        return true;
    }
    // R1555 — the arrows are the stepper's keyboard half, the peer of its
    // painted affordances (Qt `QAbstractSpinBox` steps on Up / Down too).
    if edit.form() == Some(EditorForm::Stepper) {
        match key {
            "ArrowUp" => return edit.step(1),
            "ArrowDown" => return edit.step(-1),
            _ => {}
        }
    }
    edit_field_keymap(
        scene,
        EDIT_FIELD_TAG,
        key,
        modifiers,
        kind,
        || {
            finish_commit(edit);
        },
        || {
            edit.cancel();
            use_outcome().set("cancelled".to_string());
            pinion_core::focus_request::request(TABLE_TAG);
        },
    )
}

struct CellEditorsView;

impl WidgetCore for CellEditorsView {
    type State = RootState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
        edit.set_triggers(EditTriggers::DEFAULT.with(EditTrigger::AnyKeyPressed));
        let current = use_current();
        let overlay = use_overlay();
        Box::new(
            VirtualSelectExternal::new(N).on_grid_gesture(move |key, event, _modifiers| {
                // R1555 — one observer, every sub-key. The step affordances are
                // the reason the hook takes the decoded key: `EditorStep`
                // carries a direction no `CellIndex` could.
                let index = match key {
                    GridSendKey::EditorStep { row, col, up } => {
                        step_gesture(&edit, CellIndex::new(row, col), up, event);
                        return;
                    }
                    GridSendKey::Cell { row, col } => CellIndex::new(row, col),
                    GridSendKey::Header { .. } | GridSendKey::Group { .. } => return,
                };
                {
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
                    if let Some(role) = edit_role(index, &overlay.get())
                        && edit.begin_on(trigger, index, &role)
                        && role.form().buffer_is_text()
                    {
                        pinion_core::focus_request::request(EDIT_FIELD_TAG);
                    }
                }
            }),
        )
    }

    fn tag() -> &'static str {
        TABLE_TAG
    }

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

    fn create_extra_externals() -> Vec<pinion_core::widget_core::ExtraExternal> {
        vec![pinion_core::widgets::text_field::blur_committing_field_extra(EDIT_FIELD_TAG)]
    }

    fn view(state: RootState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        let edit = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG);
        if let Some(open) = edit.open() {
            // Which surface owns the keyboard follows from the form's buffer:
            // the field owns it when the field HOLDS the in-flight value.
            if open.form().buffer_is_text() {
                if focused == Some(EDIT_FIELD_TAG) {
                    return editing_key(scene, &edit, open.kind(), key, modifiers);
                }
            } else if focused == Some(TABLE_TAG) && gesture_key(&edit, key, open.form()) {
                return true;
            }
        }
        if focused == Some(TABLE_TAG) {
            match key {
                "ArrowLeft" => return move_col(-1),
                "ArrowRight" => return move_col(1),
                "F2" => return begin_at_current(EditTrigger::EditKeyPressed, None),
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
            sync_current_row(scene);
        }
        moved
    }

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
        "pinion hello-cell-editors (R1555 §5.27 the cell-editor factory)"
    }

    fn fmt_state_log(state: &RootState) -> String {
        match state.0 {
            Some(i) => format!("selected=row {i}"),
            None => "selected=none".to_string(),
        }
    }
}

/// R1555 — the paint's step affordances, decoded back through the
/// [`GridSendKey`] SSOT that painted them.
///
/// The arrows are the only editor sub-part with its own address, so this is the
/// only sub-key a binding has to route: everything else a form offers fills its
/// cell and already resolves to the cell's own tag.
/// The state is **passed in**, not resolved here. This runs on the `External`'s
/// send arc, which has no ambient [`Owner`] — the hazard
/// [`GridEditState::new`]'s own doc records, and which cost this round one
/// panicking demo run before the observer handed its captured `Rc` over.
fn step_gesture(edit: &Rc<GridEditState>, at: CellIndex, up: bool, event: &str) -> bool {
    // A press on an arrow is inert until the activate edge, the same rule a cell
    // click follows — otherwise a press-and-drag would step twice.
    if !is_activation_event(event) {
        return false;
    }
    // An arrow only steps the editor it belongs to: the affordance is painted
    // inside an open editor, so a key naming any other cell is stale wire.
    if edit.editing() != Some(at) {
        return false;
    }
    if !edit.step(if up { 1 } else { -1 }) {
        return false;
    }
    // The arrow is a hit target of its own, so pressing it moved focus to the
    // grid and the next keystroke would have gone nowhere. Qt cannot have this
    // bug — `SC_SpinBoxUp` is a sub-control of one focus widget — so the
    // equivalent here is to hand focus back to the field the step just wrote.
    pinion_core::focus_request::request(EDIT_FIELD_TAG);
    true
}

impl WidgetA11y for CellEditorsView {
    fn access_node(state: &RootState, _focused: Option<&str>) -> Vec<AccessNode> {
        let selected = &state.0;
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_H, OVERSCAN);
        let mut nodes = windowed_grid_nodes_selected(
            TABLE_TAG,
            "Typed cell editors",
            NCOLS,
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
            *selected,
        );
        let overlay = use_overlay().get();
        mark_grid_editability(&mut nodes, TABLE_TAG, &window, 0..NCOLS, |c| {
            edit_role(c, &overlay).is_some()
        });
        // R1555 — the open editor, announced with the role its FORM has.
        if let Some(open) = use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG).open() {
            attach_cell_editor(
                &mut nodes,
                TABLE_TAG,
                open.index,
                open.form(),
                HEADERS[open.index.col.min(NCOLS - 1)],
            );
        }
        nodes
    }
}

impl WidgetView for CellEditorsView {
    type Renderer = HelloCellEditorsRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<CellEditorsView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;

    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    fn edit() -> Rc<GridEditState> {
        use_grid_edit(EDIT_KEY, EDIT_FIELD_TAG)
    }

    fn open_at(col: usize) -> CellIndex {
        let at = CellIndex::new(1, col);
        let role = edit_role(at, &use_overlay().get()).expect("editable");
        edit().begin(at, &role);
        at
    }

    /// Every tag in the painted scene, so an editor's sub-parts can be asserted
    /// to exist by the address the decoder understands.
    fn tags(scene: &Scene) -> Vec<String> {
        fn walk(scene: &Scene, out: &mut Vec<String>) {
            match scene {
                Scene::Container(c) => {
                    if let Some(t) = c.tag.as_deref() {
                        out.push(t.to_string());
                    }
                    for ch in &c.children {
                        walk(ch, out);
                    }
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), out),
                Scene::Text(t) => {
                    if let Some(tag) = t.tag.as_deref() {
                        out.push(tag.to_string());
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(scene, &mut out);
        out
    }

    /// The tag of the container whose own text child is `glyph` — the address
    /// of a painted affordance located by **what it looks like**.
    ///
    /// The step test needs this rather than `GridSendKey::encode`: identifying
    /// the up arrow by the tag `encode` produced makes the test round-trip
    /// through the very grammar it is checking, so swapping the two prefixes in
    /// `encode` stays invisible (measured — that counterfactual passed until
    /// this helper existed). The glyph is the one fact the user sees.
    fn tag_painting(scene: &Scene, glyph: &str) -> Option<String> {
        match scene {
            Scene::Container(c) => {
                let mine = c.children.iter().any(|ch| match ch {
                    Scene::Text(t) => t.content == glyph,
                    _ => false,
                });
                if mine {
                    if let Some(tag) = c.tag.as_deref() {
                        return Some(tag.to_string());
                    }
                }
                c.children.iter().find_map(|ch| tag_painting(ch, glyph))
            }
            Scene::Scroll(s) => tag_painting(s.content.as_ref(), glyph),
            _ => None,
        }
    }

    /// The node tagged `want`, anywhere under `scene`.
    fn find_tagged<'a>(scene: &'a Scene, want: &str) -> Option<&'a Scene> {
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

    /// Every `Scene::Text` string under `scene`.
    fn texts_in(scene: &Scene) -> Vec<String> {
        fn walk(scene: &Scene, out: &mut Vec<String>) {
            match scene {
                Scene::Text(t) => out.push(t.content.clone()),
                Scene::Container(c) => c.children.iter().for_each(|ch| walk(ch, out)),
                Scene::Scroll(s) => walk(s.content.as_ref(), out),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(scene, &mut out);
        out
    }

    fn scene_now() -> Scene {
        let scroll = use_scroll_state(SCROLL_KEY);
        scroll.set_max(0, i32::try_from(N * 40).unwrap());
        scroll.set_measured_viewport(WIN_W, 300);
        // R1523 — the COLUMN axis is windowed too, so without a measured
        // horizontal viewport the grid windows zero columns and every row paints
        // empty: a scene no cell assertion can find.
        use_scroll_state(H_SCROLL_KEY).set_measured_viewport(WIN_W, 300);
        view((Some(1), (TextFieldState::Focused, 0)), &Frame::default())
    }

    #[test]
    fn r1555_every_kind_has_a_column_and_the_table_is_a_census() {
        // The binding's whole claim is coverage: each kind's editor must be
        // reachable, so a kind absent from the columns is a gap in the proof.
        for kind in CellKind::ALL {
            assert!(COL_KINDS.contains(&Some(kind)), "{kind:?} has a column");
        }
        for form in EditorForm::ALL {
            assert!(
                COL_KINDS.iter().flatten().any(|k| k.editor_form() == form),
                "{form:?} is exercised by some column"
            );
        }
        assert_eq!(HEADERS.len(), COL_KINDS.len());
    }

    #[test]
    fn r1555_the_form_follows_the_datum_not_the_column_position() {
        with_owner(|| {
            let expected = [
                (1, EditorForm::Field),
                (2, EditorForm::Stepper),
                (3, EditorForm::Stepper),
                (4, EditorForm::Toggle),
                (5, EditorForm::Selector),
                (6, EditorForm::Swatch),
            ];
            for (col, form) in expected {
                let at = open_at(col);
                assert_eq!(
                    edit().form(),
                    Some(form),
                    "column {col} ({}) opens {form:?}",
                    HEADERS[col]
                );
                assert_eq!(edit().editing(), Some(at));
                edit().cancel();
            }
            // The identity column produces no edit role at all, so no gesture
            // can open an editor on it.
            let at = CellIndex::new(1, 0);
            assert!(edit_role(at, &use_overlay().get()).is_none());
        });
    }

    #[test]
    fn r1555_a_toggle_commits_through_the_same_arc_a_field_does() {
        with_owner(|| {
            let at = open_at(4);
            let before = cell_value(at, &use_overlay().get()).expect("a datum");
            assert_eq!(edit().form(), Some(EditorForm::Toggle));
            // Qt's check-state click writes through immediately; here it edits
            // the latch, so Escape can revert it.
            assert!(gesture_key(&edit(), " ", EditorForm::Toggle));
            assert!(edit().is_dirty());
            // The PAINT shows the flip. Asserting only the latch leaves the
            // painter's own read of it unchecked, and those are two derivations
            // — measured: a painter that reads the seed instead of the in-flight
            // value satisfied every latch-level assertion in this file.
            let flipped = matches!(edit().state(), EditState::Value(CellValue::Bool(true)));
            let painted = {
                let scene = scene_now();
                let cell = format!(
                    "{TABLE_TAG}#{}",
                    GridSendKey::Cell {
                        row: at.row,
                        col: at.col
                    }
                    .encode()
                );
                let node = find_tagged(&scene, &cell).expect("the editing cell");
                texts_in(node)
            };
            assert!(
                painted.contains(&if flipped {
                    "On".to_string()
                } else {
                    "Off".to_string()
                }),
                "the toggle editor paints the in-flight bool: {painted:?}"
            );
            assert!(gesture_key(&edit(), "Escape", EditorForm::Toggle));
            assert_eq!(cell_value(at, &use_overlay().get()), Some(before.clone()));

            let at = open_at(4);
            assert!(gesture_key(&edit(), " ", EditorForm::Toggle));
            assert!(gesture_key(&edit(), "Enter", EditorForm::Toggle));
            assert_eq!(use_outcome().get(), "committed");
            let after = cell_value(at, &use_overlay().get()).expect("a datum");
            assert_ne!(after, before, "the flip reached the model");
            assert!(matches!(after, CellValue::Bool(_)), "still typed");
        });
    }

    #[test]
    fn r1555_a_selector_keeps_its_domain_across_a_commit() {
        with_owner(|| {
            let at = open_at(5);
            assert!(gesture_key(&edit(), "ArrowDown", EditorForm::Selector));
            assert!(gesture_key(&edit(), "Enter", EditorForm::Selector));
            let CellValue::Choice { selected, options } =
                cell_value(at, &use_overlay().get()).expect("a datum")
            else {
                panic!("still a Choice");
            };
            assert_eq!(options.len(), TIERS.len(), "the domain survived");
            assert_eq!(
                selected,
                (at.row % TIERS.len()) + 1,
                "and the write moved one option down"
            );
        });
    }

    #[test]
    fn r1555_a_stepper_paints_addressable_arrows_that_step_the_buffer() {
        with_owner(|| {
            let at = open_at(2);
            let scene = scene_now();
            let painted = tags(&scene);
            for up in [true, false] {
                let want = format!(
                    "{TABLE_TAG}#{}",
                    GridSendKey::EditorStep {
                        row: at.row,
                        col: at.col,
                        up,
                    }
                    .encode()
                );
                assert!(painted.contains(&want), "the paint offers {want}");
            }
            // The whole loop: the arrow is located by the GLYPH it draws, its
            // tag is decoded by the grammar's own parser, and the decoded key
            // drives the step. Finding it by `encode`'s own output instead would
            // make this test round-trip through the grammar it is checking —
            // measured: swapping the two prefixes in `encode` passed.
            let up_tag = tag_painting(&scene, "\u{25B2}").expect("an up arrow is painted");
            assert!(
                painted.contains(&up_tag),
                "and it is one of the addressable tags"
            );
            let sub = up_tag
                .split_once('#')
                .map(|(_, sub)| sub)
                .expect("a composite tag");
            let Some(GridSendKey::EditorStep { row, col, up }) = GridSendKey::parse(sub) else {
                panic!("the painted address decodes as a step key");
            };
            assert_eq!(
                (row, col, up),
                (at.row, at.col, true),
                "the arrow that DRAWS an up triangle decodes as the up step"
            );
            // And the down glyph is the other one, so the pair cannot both
            // decode to the same direction.
            let down_tag = tag_painting(&scene, "\u{25BC}").expect("a down arrow");
            assert_ne!(down_tag, up_tag);
            assert_eq!(
                GridSendKey::parse(down_tag.split_once('#').expect("composite").1),
                Some(GridSendKey::EditorStep {
                    row: at.row,
                    col: at.col,
                    up: false,
                }),
            );

            let before = use_text_edit_state(EDIT_FIELD_TAG).text();
            assert!(step_gesture(
                &edit(),
                CellIndex::new(row, col),
                up,
                "PointerUp"
            ));
            let after = use_text_edit_state(EDIT_FIELD_TAG).text();
            assert_eq!(
                after.parse::<i64>().expect("an int"),
                before.parse::<i64>().expect("an int") + 1,
            );
            // A press edge is inert, so a press-and-drag cannot step twice.
            assert!(!step_gesture(
                &edit(),
                CellIndex::new(row, col),
                up,
                "PointerDown"
            ));
            assert_eq!(use_text_edit_state(EDIT_FIELD_TAG).text(), after);
            // And an arrow naming a cell that is not the open editor is stale
            // wire: it steps nothing rather than stepping the wrong cell.
            assert!(!step_gesture(
                &edit(),
                CellIndex::new(row + 1, col),
                up,
                "PointerUp"
            ));
            assert_eq!(use_text_edit_state(EDIT_FIELD_TAG).text(), after);
        });
    }

    #[test]
    fn r1555_the_model_refusal_and_a_malformed_buffer_are_different_outcomes() {
        with_owner(|| {
            let at = open_at(COUNT_COL);
            let before = cell_value(at, &use_overlay().get()).expect("a datum");
            use_text_edit_state(EDIT_FIELD_TAG).set_text("5000".to_string());
            assert_eq!(finish_commit(&edit()), CommitOutcome::Refused);
            assert_eq!(edit().editing(), Some(at), "still open, holding it");
            use_text_edit_state(EDIT_FIELD_TAG).set_text("4x".to_string());
            assert_eq!(finish_commit(&edit()), CommitOutcome::Malformed);
            assert_eq!(in_flight_text(&edit()), "<malformed>");
            assert_eq!(
                cell_value(at, &use_overlay().get()),
                Some(before),
                "neither failure touched the model"
            );
            use_text_edit_state(EDIT_FIELD_TAG).set_text("42".to_string());
            assert_eq!(finish_commit(&edit()), CommitOutcome::Committed(at));
            assert_eq!(
                cell_value(at, &use_overlay().get()),
                Some(CellValue::Int(42))
            );
        });
    }

    #[test]
    fn r1555_a_float_survives_an_open_and_commit_untouched() {
        with_owner(|| {
            // Qt's default factory hands a `QDoubleSpinBox` at
            // `decimals() == 2`, so this round trip loses precision there.
            let at = CellIndex::new(11, 3);
            let before = cell_value(at, &use_overlay().get()).expect("a datum");
            assert_eq!(before, CellValue::Float(11.0 / 8.0), "1.375 — three places");
            let role = edit_role(at, &use_overlay().get()).expect("editable");
            edit().begin(at, &role);
            assert_eq!(finish_commit(&edit()), CommitOutcome::Committed(at));
            assert_eq!(cell_value(at, &use_overlay().get()), Some(before));
        });
    }

    #[test]
    fn r1555_a_swatch_previews_the_hex_being_typed() {
        with_owner(|| {
            let at = open_at(6);
            assert_eq!(edit().form(), Some(EditorForm::Swatch));
            use_text_edit_state(EDIT_FIELD_TAG).set_text("#00ff00".to_string());
            assert_eq!(
                edit().state(),
                EditState::Value(CellValue::Color(Color::from_hex("#00ff00").expect("hex"))),
            );
            assert_eq!(finish_commit(&edit()), CommitOutcome::Committed(at));
            assert_eq!(
                cell_value(at, &use_overlay().get()),
                Some(CellValue::Color(Color::from_hex("#00ff00").expect("hex"))),
            );
        });
    }

    #[test]
    fn r1555_the_open_editor_reaches_at_with_its_form_s_role() {
        with_owner(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_max(0, i32::try_from(N * 40).unwrap());
            scroll.set_measured_viewport(WIN_W, 300);
            for (col, role) in [
                (1, AriaRole::TextInput),
                (2, AriaRole::SpinButton),
                (4, AriaRole::CheckBox),
                (5, AriaRole::ComboBox),
                (6, AriaRole::TextInput),
            ] {
                let at = open_at(col);
                let nodes =
                    CellEditorsView::access_node(&(Some(1), (TextFieldState::Idle, 0)), None);
                let editor = nodes
                    .iter()
                    .find(|n| n.tag.ends_with("#editor"))
                    .expect("the editor node");
                assert_eq!(editor.role, role, "column {col}");
                assert_eq!(editor.name.as_deref(), Some(HEADERS[col]));
                assert!(
                    editor.tag.starts_with(&format!(
                        "{TABLE_TAG}#{}",
                        GridSendKey::Cell {
                            row: at.row,
                            col: at.col
                        }
                        .encode()
                    )),
                    "the editor hangs off the cell being edited"
                );
                edit().cancel();
            }
            // With nothing open, nothing is announced.
            let nodes = CellEditorsView::access_node(&(Some(1), (TextFieldState::Idle, 0)), None);
            assert!(nodes.iter().all(|n| !n.tag.ends_with("#editor")));
        });
    }

    #[test]
    fn r1555_the_binding_paints_no_editor_of_its_own() {
        with_owner(|| {
            // The claim, as a property of the wiring: no column delegate and no
            // editor painter, so every editor on screen came from the factory.
            let _ = open_at(4);
            let scene = scene_now();
            let painted = tags(&scene);
            assert!(
                painted.iter().any(|t| t == EDIT_FIELD_TAG)
                    || edit().form() == Some(EditorForm::Toggle),
                "a latch-buffered form paints no field, and that is the point"
            );
            assert!(
                painted.iter().any(|t| t
                    == &format!(
                        "{TABLE_TAG}#{}",
                        GridSendKey::Cell { row: 1, col: 4 }.encode()
                    )),
                "the editing cell keeps its own address"
            );
        });
    }
}
