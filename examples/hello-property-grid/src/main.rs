// R836 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (WAI-ARIA, PropertyGridExternal, TextFieldExternal, gridcell, …).
#![allow(clippy::doc_markdown)]

//! `hello-property-grid` — R836 §5.38 §5.40 §5.50 **property-grid /
//! inspector detail panel**: the editor's "Details" panel (Unreal Details
//! / Qt `QtPropertyBrowser` / a CSS-devtools style editor) — a vertical
//! list of `(name, typed-value)` rows where each value is editable in place
//! by a *type-appropriate* control.
//!
//! ## Why this is the Phase-B #1 leverage item
//!
//! The northern-star is an Unreal-class editor self-hosted in pinion. That
//! editor's Details / Inspector panel is a property grid; nothing in the
//! catalog composed one yet. This binding builds it as a **pure composition
//! of existing substrate** — no new framework crate, the
//! `[[abstraction-needs-second-consumer]]` discipline (the property-grid is
//! the 1st consumer of a typed-editable-row model; the self-hosted editor
//! will be the 2nd, at which point the stable parts lift to a framework
//! crate). It is the accordion (R697) / settings-panel (R667) "Nth-consumer
//! validates substrate health" pattern applied to the editable-grid axis.
//!
//! ## Architecture — two externals, the todomvc edit-in-cell shape
//!
//! * **`PropertyGridExternal`** (`property_grid`, primary) — the grid
//!   coordinator. It owns three reactive holders shared with the view fn
//!   (`Owner::cache` dedup, the todomvc pattern): the typed value model
//!   ([`Signal<Vec<CellValue>>`] — the value SSOT), the roving cursor
//!   ([`Signal<usize>`] `focused_row`), and the edit-mode latch
//!   ([`Signal<Option<usize>>`] `editing_row`, the todomvc `editing_id`
//!   generalised to a row index). It exposes the whole grid for AI-first
//!   introspection (§2 #2): `query "value.<i>"` reads each typed value,
//!   `query "name.<i>"` / `"kind.<i>"` the row metadata, `intervene
//!   "value.<i>"` sets a value programmatically (the deterministic AI
//!   driving path — no simulated typing), `intervene "focused_row"` moves
//!   the cursor, `invoke "toggle"` flips the focused bool, `invoke "begin"`
//!   enters edit mode.
//! * **`TextFieldExternal`** (`property_grid_edit`, extra) — ONE shared
//!   inline editor reused across every text / int / float row (the todomvc
//!   single-editor pattern; scales to any row count). It paints only inside
//!   the value cell of the row being edited; the rest of the time the value
//!   cell shows the formatted value as text (or a checkbox glyph for bools).
//!
//! There is no per-row external — bools toggle through the coordinator
//! (`Space` / single-click, the checkbox affordance), and text / number
//! rows route their inline edit through the one shared field. Two externals
//! drive an arbitrary number of typed rows.
//!
//! ## Keyboard model (WAI-ARIA editable data-grid)
//!
//! The grid is a **single Tab stop** with a roving row cursor (the APG
//! data-grid pattern, scales to large grids — unlike one-Tab-stop-per-row).
//! While the grid holds focus: `ArrowUp` / `ArrowDown` move the cursor
//! (clamped — a grid has ends, no wrap), `Home` / `End` jump; `Space`
//! toggles a bool row; `Enter` / `F2` toggles a bool or enters edit mode on
//! a text / int / float row (focus moves into the shared inline field via
//! the [`pinion_core::focus_request`] mailbox). While editing: `Enter`
//! commits (parse → write back to the model), `Escape` cancels (the value is
//! left untouched), and the int / float rows gate non-numeric keystrokes the
//! way `hello-number-input` does. A click-away commit-on-blur rides the
//! field's `with_blur_intent` (R793), the todomvc commit-on-blur shape.
//!
//! ## a11y (R836 §5.40) — 3rd consumer of the grid SSOT
//!
//! The panel lowers to a WAI-ARIA `grid` through the lifted
//! [`pinion_a11y::grid::grid_table_nodes`] builder (hello-table /
//! hello-table-multi are the 1st / 2nd consumers): a `Property` / `Value`
//! header row over one data `row` per property, each row a `rowheader`-like
//! name cell + a `gridcell` value cell. The roving cursor is exposed as
//! `aria-activedescendant` through `access_focus_target` (R870 — the
//! authoritative channel; the per-cell `focused` flag is a redundant marker).
//! The typed value is encoded in the cell's accessible name (`"Opacity: 1"`)
//! so an AT user hears the value with its column context.
//!
//! ## Known gaps (honest carry)
//!
//! - **Native checkbox / textbox cell roles.** Bool cells encode their state
//!   as cell text (`"Visible: On"`) rather than a nested `checkbox` role, and
//!   the inline editor is a plain `textbox`. A per-cell-role grid a11y axis
//!   is additive and deferred until the self-hosted editor (2nd consumer)
//!   pins the exact shape (`[[abstraction-needs-second-consumer]]`).
//! - **Per-property validation / clamp ranges.** Numeric rows accept any
//!   parseable value; a malformed commit reverts to the prior value (no data
//!   loss) rather than clamping into a per-property `[min, max]`. Range
//!   metadata is an additive model field (the `hello-number-input`
//!   `parse_clamp` shape) deferred to the same 2nd-consumer round.

use std::rc::Rc;

use pinion_a11y::{
    grid_table_nodes, listbox_option_nodes, AccessFocus, AccessNode, GridCell, GridColumn, GridRow,
    ListOption, WidgetA11y,
};
use pinion_core::composite_tag::split_send_payload;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, IntrospectSchema,
    IntrospectValue, InterveneError, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::checkbox::CheckboxState;
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::text_edit::{use_text_edit_state, TextEditState};
use pinion_core::widgets::text_field::{TextFieldExternal, TextFieldState};
use pinion_core::cell_value::{CellKind, CellValue};
use pinion_core::{Color, Command, Frame, Modifiers, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::barrier::dismiss_barrier;
use pinion_widget_paint::checkbox::{view_checkbox_box, CheckboxStyle};
use pinion_widget_paint::listbox::{view_option, OptionRow};
use pinion_widget_paint::popup::popup_surface;
use pinion_widget_paint::text_field as tf_paint;

use pinion_widget_paint::state_layer::HOVER;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloPropertyGridRenderer, HelloPropertyGridRendererError);

// ─── window + layout constants ─────────────────────────────────────

const WIN_W: u32 = 460;
// R867/R869 — 12 rows (the bool/int/float/text quartet + two enum/choice
// rows + one colour row) plus the title band; a popup may flip above its row
// near the bottom.
const WIN_H: u32 = 620;
const THEME_TAG: &str = "app";

const TITLE_PX: u32 = 22;
const HEADER_PX: u32 = 13;
const CELL_PX: u32 = 15;

const NAME_COL_W: u32 = 150;
const VALUE_COL_W: u32 = 250;
const ROW_H: u32 = 38;
const CELL_PAD: u32 = 10;
const CHECKBOX_SIZE: u32 = 20;
const PANEL_PAD: u32 = 20;
const ROW_GAP: u32 = 2;

// ─── choice-popup geometry (R867) ─────────────────────────────────

/// Fixed title-band height — makes the row → popup anchor math (and the
/// flip-above decision) deterministic instead of font-metric dependent.
const TITLE_H: u32 = 30;
/// Gap between the title band and the grid (the outer flex column gap).
const TITLE_GAP: u32 = ROW_GAP * 6;
/// The grid container's outline width — part of the row-offset math.
const GRID_BORDER: u32 = 1;
/// Choice popup option-row height + the panel's inner padding + width.
const POPUP_OPT_H: u32 = 32;
const POPUP_PAD: u32 = 6;
const POPUP_W: u32 = VALUE_COL_W;
/// U+25BE BLACK DOWN-POINTING SMALL TRIANGLE — the choice-cell dropdown
/// affordance ([[non-ascii-literal-named-const-escape]]).
const CHOICE_CHEVRON: &str = "\u{25BE}";

// ─── tags + intents ───────────────────────────────────────────────

/// Primary External — the grid coordinator (the single keyboard Tab stop).
const GRID_TAG: &str = "property_grid";
/// Extra External — the one shared inline text / number editor.
const EDIT_TF_TAG: &str = "property_grid_edit";
/// Commit-on-blur intent the inline field raises on a click-away (R793).
const EDIT_TF_BLUR_INTENT_TAG: &str = pinion_core::intent_tag!("property_grid_edit", "blur");
/// The choice popup's paint panel + WAI-ARIA `listbox` container tag.
const CHOICE_POPUP_TAG: &str = "property_grid_choice";
/// The choice popup's light-dismiss barrier — a composite tag routing back
/// to the grid coordinator (`{GRID_TAG}#dismiss`), so the popup needs no
/// separate barrier external (a coordinator-owned overlay, not a
/// `ListBoxExternal`: the cell's model is the grid's `CellValue`, so a
/// model-owning listbox widget would split the selection source).
const POPUP_DISMISS_TAG: &str = "property_grid#dismiss";
/// Composite sub-tag prefix for a popup option cell (`{GRID_TAG}#opt{i}`),
/// routing the click / hover to the coordinator's `send`.
const CHOICE_OPT_PREFIX: &str = "opt";

// ─── colour-cell popup (R869) ─────────────────────────────────────

/// The colour popup's paint panel + WAI-ARIA `listbox` container tag.
const COLOR_POPUP_TAG: &str = "property_grid_color";
/// Composite sub-tag prefix for a swatch cell (`{GRID_TAG}#sw{i}`).
const COLOR_SW_PREFIX: &str = "sw";
/// Swatch box size, columns, and gap in the popup grid.
const SWATCH_SIZE: u32 = 34;
const SWATCH_COLS: usize = 4;
const SWATCH_GAP: u32 = 6;
/// The colour popup's hex-entry field height (an arbitrary `#RRGGBB`).
const HEX_FIELD_H: u32 = 28;

/// The preset colour palette the popup offers (name = the AT label). An
/// arbitrary colour is set through `intervene value.<i>` with a hex string —
/// the AI-first path; a GUI hex-entry field is a documented follow-up.
const COLOR_SWATCHES: [(Color, &str); 8] = [
    (Color::rgb(0xff, 0xff, 0xff), "White"),
    (Color::rgb(0x21, 0x21, 0x21), "Black"),
    (Color::rgb(0xe5, 0x39, 0x35), "Red"),
    (Color::rgb(0x43, 0xa0, 0x47), "Green"),
    (Color::rgb(0x1e, 0x88, 0xe5), "Blue"),
    (Color::rgb(0xfd, 0xd8, 0x35), "Yellow"),
    (Color::rgb(0x00, 0xac, 0xc1), "Cyan"),
    (Color::rgb(0x8e, 0x24, 0xaa), "Purple"),
];


// ─── typed property model ─────────────────────────────────────────

const ROW_COUNT: usize = 12;

/// The property row names. Static — only the [`CellValue`]s mutate, so
/// names live in a `const` (the value SSOT is the coordinator's Signal).
const PROPERTY_NAMES: [&str; ROW_COUNT] = [
    "Name", "Tag", "Visible", "Locked", "Layer", "Health", "Pos X", "Pos Y", "Opacity", "Blend",
    "Body", "Tint",
];

// R837 §5.38 — the typed value model + its pure helpers (kind dispatch,
// display / edit formatting, parse, the keystroke gate, the introspect read
// / intervene write) were lifted to `pinion_core::cell_value` at the 2nd
// consumer (`hello-data-grid`); this binding consumes that SSOT. The R836
// `CellValue` / `CellKind` are now `CellValue` / `CellKind`.

/// First-paint property values. A game-object inspector — the kinds the
/// self-hosted editor's Details panel needs (name / tag text, visibility /
/// lock flags, layer / health ints, transform / opacity floats).
fn default_properties() -> Vec<CellValue> {
    vec![
        CellValue::Text("Player".to_owned()),
        CellValue::Text("hero".to_owned()),
        CellValue::Bool(true),
        CellValue::Bool(false),
        CellValue::Int(3),
        CellValue::Int(100),
        CellValue::Float(12.5),
        CellValue::Float(-4.0),
        CellValue::Float(1.0),
        // R867 — enum/choice rows (the popup-listbox cell): a render blend
        // mode (4 options) and a collision-body type (3 options, Solid set).
        CellValue::Choice {
            selected: 0,
            options: vec![
                "Normal".to_owned(),
                "Additive".to_owned(),
                "Multiply".to_owned(),
                "Screen".to_owned(),
            ],
        },
        CellValue::Choice {
            selected: 2,
            options: vec!["None".to_owned(), "Trigger".to_owned(), "Solid".to_owned()],
        },
        // R869 — the colour cell (popup swatch palette): the object tint.
        CellValue::Color(COLOR_SWATCHES[4].0), // Blue
    ]
}

// ─── reactive holders (Owner::cache, shared view ↔ coordinator) ────

/// Typed value SSOT. A `Signal` so a value change (keyboard commit, RPC
/// intervene, bool toggle) re-runs the subscribed view fn.
#[must_use]
fn use_property_model() -> Rc<Signal<Vec<CellValue>>> {
    let owner = Owner::current().expect("use_property_model requires an active Owner scope");
    owner.cache("property_grid.model", || Signal::new(default_properties()))
}

/// Roving row cursor. `Signal` so navigation re-runs the view fn (the
/// focused-row highlight + `aria-activedescendant` follow it).
#[must_use]
fn use_focused_row() -> Rc<Signal<usize>> {
    let owner = Owner::current().expect("use_focused_row requires an active Owner scope");
    owner.cache("property_grid.focused_row", || Signal::new(0_usize))
}

/// Edit-mode latch — `Some(row)` while that row's value is being text-edited
/// (the todomvc `editing_id`, keyed by row index). `None` = navigating.
#[must_use]
fn use_editing_row() -> Rc<Signal<Option<usize>>> {
    let owner = Owner::current().expect("use_editing_row requires an active Owner scope");
    owner.cache("property_grid.editing_row", || Signal::new(None))
}

/// R867 — the choice popup's roving active descendant (the keyboard cursor
/// within the open dropdown). `Some(i)` while a popup is open; `None` when
/// closed. Reused across every choice row (one popup is open at a time, the
/// shared inline editor's discipline).
#[must_use]
fn use_popup_cursor() -> Rc<Signal<Option<usize>>> {
    let owner = Owner::current().expect("use_popup_cursor requires an active Owner scope");
    owner.cache("property_grid.popup_cursor", || Signal::new(None))
}

/// R867 — the choice popup's pointer-hovered option (the mouse highlight),
/// or `None`. Set by `PointerEnter` / `PointerLeave` on the option cells.
#[must_use]
fn use_popup_hover() -> Rc<Signal<Option<usize>>> {
    let owner = Owner::current().expect("use_popup_hover requires an active Owner scope");
    owner.cache("property_grid.popup_hover", || Signal::new(None))
}

// ─── choice-popup mutation SSOT ───────────────────────────────────
// Signal-only (no scene), so the coordinator's pointer / RPC path (its `Rc`
// fields) and the keyboard / reducer free-fn path (the `use_*` hooks — the
// same Owner-cached instances) share one mutation each.

/// Write the choice cell at `row` to option `i` (no-op if the row is not a
/// choice or `i` is out of range). Returns whether it committed.
fn set_choice_selected(model: &Signal<Vec<CellValue>>, row: usize, i: usize) -> bool {
    let mut committed = false;
    model.set_with(|prev| {
        let mut next = prev.clone();
        if let Some(CellValue::Choice { options, selected }) = next.get_mut(row) {
            if i < options.len() {
                *selected = i;
                committed = true;
            }
        }
        next
    });
    committed
}

/// Write the colour cell at `row` to swatch `i` (no-op if the row is not a
/// colour or `i` is out of range). Returns whether it committed.
fn set_color_swatch(model: &Signal<Vec<CellValue>>, row: usize, i: usize) -> bool {
    let Some((color, _)) = COLOR_SWATCHES.get(i) else {
        return false;
    };
    let mut committed = false;
    model.set_with(|prev| {
        let mut next = prev.clone();
        if let Some(cell @ CellValue::Color(_)) = next.get_mut(row) {
            *cell = CellValue::Color(*color);
            committed = true;
        }
        next
    });
    committed
}

/// Tear down the open choice popup — clear the edit latch, the keyboard
/// cursor, and the pointer hover in one place.
fn clear_popup(
    editing: &Signal<Option<usize>>,
    cursor: &Signal<Option<usize>>,
    hover: &Signal<Option<usize>>,
) {
    editing.set(None);
    cursor.set(None);
    hover.set(None);
}

// ─── grid coordinator External ────────────────────────────────────

/// The property-grid coordinator. Holds `Rc` clones of the reactive holders
/// (resolved through the `use_*` hooks at construction, so they are the same
/// instances the view fn reads) + the shared editor's [`TextEditState`] so
/// [`Self::begin_edit`] can seed it. Mutations write the Signals directly —
/// no hooks at invoke time, so the External is self-contained on any thread
/// (the todomvc `TodoEditExternal` shape).
struct PropertyGridExternal {
    model: Rc<Signal<Vec<CellValue>>>,
    focused_row: Rc<Signal<usize>>,
    editing_row: Rc<Signal<Option<usize>>>,
    editor: Rc<TextEditState>,
    popup_cursor: Rc<Signal<Option<usize>>>,
    popup_hover: Rc<Signal<Option<usize>>>,
}

impl PropertyGridExternal {
    fn new(
        model: Rc<Signal<Vec<CellValue>>>,
        focused_row: Rc<Signal<usize>>,
        editing_row: Rc<Signal<Option<usize>>>,
        editor: Rc<TextEditState>,
        popup_cursor: Rc<Signal<Option<usize>>>,
        popup_hover: Rc<Signal<Option<usize>>>,
    ) -> Self {
        Self { model, focused_row, editing_row, editor, popup_cursor, popup_hover }
    }

    fn count(&self) -> usize {
        self.model.get().len()
    }

    /// Toggle the bool at `row`; no-op (returns `false`) if the row is not a
    /// bool. The checkbox affordance behind `Space` + single-click.
    fn toggle(&self, row: usize) -> bool {
        let mut toggled = false;
        self.model.set_with(|prev| {
            let mut next = prev.clone();
            if let Some(CellValue::Bool(b)) = next.get_mut(row) {
                *b = !*b;
                toggled = true;
            }
            next
        });
        toggled
    }

    /// Enter edit mode on `row`. A text / int / float row latches
    /// `editing_row`, seeds the shared inline editor with the formatted value
    /// (caret at the trailing edge), and requests focus into the field. A
    /// choice row opens its popup instead (`open_choice`, focus stays on the
    /// grid). Returns `false` for a bool row (bools toggle) or an
    /// out-of-range index.
    fn begin_edit(&self, row: usize) -> bool {
        let model = self.model.get();
        let Some(value) = model.get(row) else {
            return false;
        };
        if matches!(value, CellValue::Choice { .. }) {
            return self.open_choice(row);
        }
        if matches!(value, CellValue::Color(_)) {
            return self.open_color(row);
        }
        if !value.kind().is_text_editable() {
            return false;
        }
        let text = value.edit_text();
        let len = text.len();
        self.editing_row.set(Some(row));
        self.editor.set_text(text);
        self.editor.set_caret(len);
        pinion_core::focus_request::request(EDIT_TF_TAG);
        true
    }

    /// Open the choice popup on `row`: latch `editing_row` and seed the
    /// keyboard cursor at the committed option (so arrows start from the
    /// current value). Focus stays on the grid — the popup is the grid's
    /// roving active descendant, not a separate Tab stop. Returns `false`
    /// for a non-choice row.
    fn open_choice(&self, row: usize) -> bool {
        let model = self.model.get();
        let Some(CellValue::Choice { selected, .. }) = model.get(row) else {
            return false;
        };
        self.editing_row.set(Some(row));
        self.popup_cursor.set(Some(*selected));
        self.popup_hover.set(None);
        true
    }

    /// Commit option `i` into the open choice row, then close the popup. The
    /// pointer (option click) + RPC (`choose`) commit path; returns whether
    /// a choice was committed.
    fn commit_choice_index(&self, i: usize) -> bool {
        let Some(row) = self.editing_row.get() else {
            return false;
        };
        let committed = set_choice_selected(&self.model, row, i);
        self.close_popup();
        committed
    }

    /// Close the choice popup without committing (the dismiss-barrier +
    /// RPC `close_popup` path). Generic popup teardown — also closes the
    /// colour popup (the cursor / hover Signals are shared, only one popup is
    /// open at a time).
    fn close_popup(&self) {
        clear_popup(&self.editing_row, &self.popup_cursor, &self.popup_hover);
    }

    /// Open the colour popup on `row`: latch `editing_row`, seed the swatch
    /// cursor at the preset matching the current colour (or 0), and seed the
    /// shared editor with the colour's hex so the popup's hex field shows it.
    /// Focus stays on the grid (the swatch grid is the roving active
    /// descendant; Tab reaches the hex field). Returns `false` for a
    /// non-colour row. Reuses the shared popup cursor / hover.
    fn open_color(&self, row: usize) -> bool {
        let model = self.model.get();
        let Some(CellValue::Color(c)) = model.get(row) else {
            return false;
        };
        let cursor = COLOR_SWATCHES.iter().position(|(sw, _)| sw == c).unwrap_or(0);
        let hex = c.to_hex();
        let len = hex.len();
        self.editing_row.set(Some(row));
        self.popup_cursor.set(Some(cursor));
        self.popup_hover.set(None);
        self.editor.set_text(hex);
        self.editor.set_caret(len);
        true
    }

    /// Commit swatch `i` into the open colour row, then close the popup (the
    /// swatch click + RPC `pick_color` + keyboard path); `false` if out of
    /// range or not a colour row.
    fn commit_color_swatch(&self, i: usize) -> bool {
        let Some(row) = self.editing_row.get() else {
            return false;
        };
        let committed = set_color_swatch(&self.model, row, i);
        self.close_popup();
        committed
    }

    /// Set / clear the popup item hover — the shared `PointerEnter` /
    /// `PointerLeave` handler for choice options + colour swatches.
    fn set_popup_hover(&self, event_name: &str, i: usize) {
        match event_name {
            "PointerEnter" => self.popup_hover.set(Some(i)),
            "PointerLeave" => {
                if self.popup_hover.get() == Some(i) {
                    self.popup_hover.set(None);
                }
            }
            _ => {}
        }
    }

    /// Route a composite-tag `send` payload: the dismiss barrier (`dismiss`),
    /// a popup option (`opt<i>` commits a choice), a colour swatch (`sw<i>`
    /// commits a colour), or a numeric row (focus + bool-toggle / popup-open /
    /// `DoubleClick`-edit) — all four route into this one coordinator.
    fn dispatch_send(&mut self, s: &str) -> Result<IntrospectValue, InvokeError> {
        let (key, event_name, _) = split_send_payload(s).ok_or(InvokeError::Rejected)?;
        if key == "dismiss" {
            if event_name == "PointerUp" {
                self.close_popup();
            }
            return Ok(IntrospectValue::Null);
        }
        if let Some(opt) = key.strip_prefix(CHOICE_OPT_PREFIX) {
            let i: usize = opt.parse().map_err(|_| InvokeError::Rejected)?;
            if event_name == "PointerUp" {
                self.commit_choice_index(i);
            } else {
                self.set_popup_hover(event_name, i);
            }
            return Ok(IntrospectValue::Null);
        }
        if let Some(sw) = key.strip_prefix(COLOR_SW_PREFIX) {
            let i: usize = sw.parse().map_err(|_| InvokeError::Rejected)?;
            if event_name == "PointerUp" {
                self.commit_color_swatch(i);
            } else {
                self.set_popup_hover(event_name, i);
            }
            return Ok(IntrospectValue::Null);
        }
        let idx: usize = key.parse().map_err(|_| InvokeError::Rejected)?;
        if idx >= self.count() {
            return Err(InvokeError::Rejected);
        }
        match event_name {
            "PointerUp" => {
                self.focused_row.set(idx);
                match self.model.get().get(idx) {
                    Some(CellValue::Bool(_)) => {
                        self.toggle(idx);
                    }
                    Some(CellValue::Choice { .. }) => {
                        self.open_choice(idx);
                    }
                    Some(CellValue::Color(_)) => {
                        self.open_color(idx);
                    }
                    _ => {}
                }
                Ok(IntrospectValue::Int(i64::try_from(idx).expect("row index fits in i64")))
            }
            "DoubleClick" => Ok(IntrospectValue::Bool(self.begin_edit(idx))),
            _ => Ok(IntrospectValue::Null),
        }
    }

    fn set_focused_clamped(&self, row: usize) {
        let max = self.count().saturating_sub(1);
        self.focused_row.set(row.min(max));
    }
}

impl core::fmt::Debug for PropertyGridExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PropertyGridExternal")
            .field("row_count", &self.count())
            .field("focused_row", &self.focused_row.get())
            .field("editing_row", &self.editing_row.get())
            .finish_non_exhaustive()
    }
}

impl External for PropertyGridExternal {
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

impl ExternalIntrospect for PropertyGridExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("row_count", "int"),
            ("focused_row", "int"),
            ("editing", "json"),
            ("name.<index>", "string"),
            ("kind.<index>", "string"),
            ("value.<index>", "json"),
            ("popup_cursor", "int"),
            ("send", "string"),
            ("toggle", "json"),
            ("begin", "int"),
            ("choose", "int"),
            ("pick_color", "int"),
            ("close_popup", "json"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "row_count" => Some(IntrospectValue::Int(
                i64::try_from(self.count()).expect("row count fits in i64"),
            )),
            "focused_row" => Some(IntrospectValue::Int(
                i64::try_from(self.focused_row.get()).expect("row index fits in i64"),
            )),
            "editing" => Some(IntrospectValue::Json(match self.editing_row.get() {
                Some(row) => serde_json::Value::from(
                    i64::try_from(row).expect("row index fits in i64"),
                ),
                None => serde_json::Value::Null,
            })),
            "popup_cursor" => Some(match self.popup_cursor.get() {
                Some(i) => {
                    IntrospectValue::Int(i64::try_from(i).expect("cursor index fits in i64"))
                }
                None => IntrospectValue::Null,
            }),
            _ => {
                if let Some(idx_str) = path.strip_prefix("name.") {
                    let idx: usize = idx_str.parse().ok()?;
                    return PROPERTY_NAMES
                        .get(idx)
                        .map(|name| IntrospectValue::Text((*name).to_owned()));
                }
                if let Some(idx_str) = path.strip_prefix("kind.") {
                    let idx: usize = idx_str.parse().ok()?;
                    let model = self.model.get();
                    let value = model.get(idx)?;
                    return Some(IntrospectValue::Text(value.kind().name().to_owned()));
                }
                if let Some(idx_str) = path.strip_prefix("value.") {
                    let idx: usize = idx_str.parse().ok()?;
                    let model = self.model.get();
                    let value = model.get(idx)?;
                    return Some(value.to_introspect());
                }
                None
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "row_count" | "editing" | "popup_cursor" => Err(InterveneError::ReadOnly),
            "focused_row" => match value {
                IntrospectValue::Int(i) => {
                    let row = usize::try_from(i).map_err(|_| InterveneError::TypeMismatch)?;
                    self.set_focused_clamped(row);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            _ => {
                let Some(idx_str) = path.strip_prefix("value.") else {
                    return Err(InterveneError::UnknownPath);
                };
                let idx: usize = idx_str.parse().map_err(|_| InterveneError::UnknownPath)?;
                if idx >= self.count() {
                    return Err(InterveneError::UnknownPath);
                }
                // `with_intervene` (not `kind().coerce`) so a choice cell sets
                // its option by index while preserving its option list.
                let new_value = self.model.get()[idx].with_intervene(value)?;
                self.model.set_with(move |prev| {
                    let mut next = prev.clone();
                    next[idx] = new_value.clone();
                    next
                });
                Ok(())
            }
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Composite wire `"<key>:<EventName>"` — routed in `dispatch_send`.
            "send" => match args {
                IntrospectValue::Text(ref s) => self.dispatch_send(s),
                _ => Err(InvokeError::TypeMismatch),
            },
            // Toggle the focused bool (the `Space` keyboard path + the RPC
            // affordance). No-op on a non-bool focused row.
            "toggle" => Ok(IntrospectValue::Bool(self.toggle(self.focused_row.get()))),
            // Enter edit mode on a given row (the `Enter` / `F2` keyboard
            // path + the RPC edit-entry affordance) — text edit, or for a
            // choice row, opens the popup.
            "begin" => match args {
                IntrospectValue::Int(i) => {
                    let row = usize::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    if row >= self.count() {
                        return Err(InvokeError::Rejected);
                    }
                    Ok(IntrospectValue::Bool(self.begin_edit(row)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Commit a popup option by index, closing the popup (the option
            // click + RPC choice-commit path). Requires an open choice popup.
            "choose" => match args {
                IntrospectValue::Int(i) => {
                    let opt = usize::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    Ok(IntrospectValue::Bool(self.commit_choice_index(opt)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Commit a colour swatch by index, closing the popup (the swatch
            // click + RPC path). Requires an open colour popup.
            "pick_color" => match args {
                IntrospectValue::Int(i) => {
                    let sw = usize::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    Ok(IntrospectValue::Bool(self.commit_color_swatch(sw)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Dismiss the open popup without committing (RPC + the keyboard
            // `Escape` / barrier path share `close_popup`).
            "close_popup" => {
                self.close_popup();
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// ─── inline-editor commit / cancel (keyboard, owner-scoped) ───────

/// Commit the in-flight edit: parse the editor text by the editing row's
/// kind and write it back to the model. A malformed numeric commit keeps the
/// prior value (no data loss). Mirrors `todomvc::commit_edit`.
fn commit_edit(restore_focus: bool) {
    let editing_row = use_editing_row();
    let Some(row) = editing_row.get() else {
        return;
    };
    let model = use_property_model();
    let text = use_text_edit_state(EDIT_TF_TAG).text();
    let current = model.get();
    if let Some(value) = current.get(row) {
        if let Some(parsed) = value.kind().parse(&text) {
            model.set_with(move |prev| {
                let mut next = prev.clone();
                next[row] = parsed.clone();
                next
            });
        }
    }
    end_edit_mode(restore_focus);
}

/// Cancel the in-flight edit — leave the value untouched, restore focus.
fn cancel_edit() {
    end_edit_mode(true);
}

/// Shared finish-edit teardown — clear `editing_row` + wipe the editor so
/// the next edit starts from a fresh seed; clear the popup cursor / hover (the
/// colour hex field commits through this path, so it must also tear the swatch
/// popup down); restore grid focus on request.
fn end_edit_mode(restore_focus: bool) {
    clear_popup(&use_editing_row(), &use_popup_cursor(), &use_popup_hover());
    use_text_edit_state(EDIT_TF_TAG).set_text(String::new());
    if restore_focus {
        pinion_core::focus_request::request(GRID_TAG);
    }
}

/// The kind of the row currently being edited (`None` when not editing) —
/// drives the int / float keystroke gate.
fn editing_kind() -> Option<CellKind> {
    let row = use_editing_row().get()?;
    use_property_model().get().get(row).map(CellValue::kind)
}

// ─── keyboard ─────────────────────────────────────────────────────

/// The kind of the open popup row (`Choice` / `Color`), or `None` when no
/// popup is open. While a popup is open the grid keeps focus but the popup
/// owns the keymap.
fn open_popup_kind() -> Option<(usize, CellKind)> {
    let row = use_editing_row().get()?;
    match use_property_model().get().get(row).map(CellValue::kind) {
        Some(kind @ (CellKind::Choice | CellKind::Color)) => Some((row, kind)),
        _ => None,
    }
}

/// Commit the popup cursor into the choice row + close (the keyboard
/// `Enter` / `Space` path, sharing the model SSOT with the pointer path).
fn commit_choice_keyboard(row: usize, i: usize) {
    set_choice_selected(&use_property_model(), row, i);
    close_popup();
}

/// Commit the swatch cursor into the colour row + close (the keyboard path,
/// sharing the model SSOT with the pointer / RPC path).
fn commit_color_keyboard(row: usize, i: usize) {
    set_color_swatch(&use_property_model(), row, i);
    close_popup();
}

/// Close the open popup without committing (keyboard `Escape`). Generic —
/// the cursor / hover Signals are shared across the choice + colour popups.
fn close_popup() {
    clear_popup(&use_editing_row(), &use_popup_cursor(), &use_popup_hover());
}

/// Choice-popup keymap (the grid is focused, the popup is open): arrows /
/// Home / End rove the active descendant (clamped to the option list — the
/// popup has ends), `Enter` / `Space` commit the cursor, `Escape` dismisses.
fn apply_key_choice(row: usize, key: &str) -> bool {
    let model = use_property_model().get();
    let Some(CellValue::Choice { options, selected }) = model.get(row) else {
        return false;
    };
    let len = options.len();
    if len == 0 {
        return false;
    }
    let cursor = use_popup_cursor().get().unwrap_or(*selected).min(len - 1);
    let target = match key {
        "ArrowDown" => (cursor + 1).min(len - 1),
        "ArrowUp" => cursor.saturating_sub(1),
        "Home" => 0,
        "End" => len - 1,
        "Enter" | "Space" => {
            commit_choice_keyboard(row, cursor);
            return true;
        }
        "Escape" => {
            close_popup();
            return true;
        }
        _ => return false,
    };
    use_popup_cursor().set(Some(target));
    true
}

/// Colour-popup keymap (the grid is focused, the popup is open): arrows rove
/// the swatch cursor over the 2-D palette grid (Left/Right step, Up/Down jump
/// a row), `Enter` / `Space` commit the cursor swatch, `Escape` dismisses.
fn apply_key_color(row: usize, key: &str) -> bool {
    let len = COLOR_SWATCHES.len();
    let cursor = use_popup_cursor().get().unwrap_or(0).min(len - 1);
    let target = match key {
        "ArrowRight" => (cursor + 1).min(len - 1),
        "ArrowLeft" => cursor.saturating_sub(1),
        "ArrowDown" => (cursor + SWATCH_COLS).min(len - 1),
        "ArrowUp" => cursor.saturating_sub(SWATCH_COLS),
        "Home" => 0,
        "End" => len - 1,
        "Enter" | "Space" => {
            commit_color_keyboard(row, cursor);
            return true;
        }
        "Escape" => {
            close_popup();
            return true;
        }
        _ => return false,
    };
    use_popup_cursor().set(Some(target));
    true
}

/// Grid-focused keymap: roving navigation + activate. Navigation writes the
/// `focused_row` Signal directly (pure cursor motion, no cross-external
/// effect); `Space` / `Enter` / `F2` route through the coordinator's
/// `invoke` so the keyboard path is identical to the RPC path. An open
/// choice / colour popup intercepts the keymap first.
fn apply_key_grid(scene: &mut Scene, key: &str) -> bool {
    if let Some((row, kind)) = open_popup_kind() {
        return match kind {
            CellKind::Color => apply_key_color(row, key),
            _ => apply_key_choice(row, key),
        };
    }
    let focused = use_focused_row();
    let count = use_property_model().get().len();
    if count == 0 {
        return false;
    }
    let current = focused.get().min(count - 1);
    match key {
        "ArrowDown" => {
            focused.set((current + 1).min(count - 1));
            true
        }
        "ArrowUp" => {
            focused.set(current.saturating_sub(1));
            true
        }
        "Home" => {
            focused.set(0);
            true
        }
        "End" => {
            focused.set(count - 1);
            true
        }
        "Space" => activate_focused(scene, current, false),
        "Enter" | "F2" => activate_focused(scene, current, true),
        _ => false,
    }
}

/// Activate the focused row: toggle a bool, open a choice popup, or (when
/// `allow_edit`) enter edit mode on a text / int / float row. Routes through
/// the coordinator's `invoke` so toggle / begin live in one place (the RPC
/// path).
fn activate_focused(scene: &mut Scene, row: usize, allow_edit: bool) -> bool {
    let kind = match use_property_model().get().get(row) {
        Some(value) => value.kind(),
        None => return false,
    };
    let Some(node) = scene.find_external_with_tag_mut(GRID_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    match kind {
        CellKind::Bool => intro.invoke("toggle", IntrospectValue::Null).is_ok(),
        // A choice / colour row opens its popup on both Space and Enter (the
        // dropdown affordance) — `allow_edit` only gates the text editors.
        CellKind::Choice | CellKind::Color => {
            let arg = IntrospectValue::Int(i64::try_from(row).expect("row index fits in i64"));
            intro.invoke("begin", arg).is_ok()
        }
        _ if allow_edit => {
            let arg = IntrospectValue::Int(i64::try_from(row).expect("row index fits in i64"));
            intro.invoke("begin", arg).is_ok()
        }
        _ => false,
    }
}

/// Edit-mode keymap over the shared inline field. Enter commits, Escape
/// cancels, caret / deletion keys always reach the field, and printable keys
/// pass the int / float keystroke gate (text rows accept everything).
fn apply_key_edit(scene: &mut Scene, key: &str, modifiers: Modifiers) -> bool {
    match key {
        "Enter" => {
            commit_edit(true);
            true
        }
        "Escape" => {
            cancel_edit();
            true
        }
        "ArrowLeft" | "ArrowRight" | "Home" | "End" | "Backspace" | "Delete" => {
            pinion_core::forward_key_to_field(scene, EDIT_TF_TAG, key, modifiers)
        }
        other => {
            let allowed = editing_kind().is_some_and(|kind| kind.accepts_keystroke(other));
            if allowed {
                pinion_core::forward_key_to_field(scene, EDIT_TF_TAG, other, modifiers)
            } else {
                false
            }
        }
    }
}

// ─── paint ────────────────────────────────────────────────────────

/// Focused-row background = the M3 `OnSurface` state-layer over the surface
/// (the hover / pressed overlay the catalog widgets share).
fn row_fill(theme: &Theme, focused: bool) -> Color {
    if focused {
        theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::OnSurface), HOVER)
    } else {
        Color::TRANSPARENT
    }
}

/// Cell-sized M3 checkbox-box style. The bool value cell renders the lifted
/// `view_checkbox_box` SSOT non-interactively (the grid coordinator owns the
/// toggle, so there is no per-cell `CheckboxExternal`) — keeping one M3
/// checkbox rendering across the catalog instead of a hand-rolled copy.
fn cell_checkbox_style() -> CheckboxStyle {
    CheckboxStyle { box_size: CHECKBOX_SIZE, glyph_size_px: 16, ..CheckboxStyle::m3_filled() }
}

/// One property row: `[ name cell | value cell ]`, tagged `property_grid#<i>`
/// so a click routes to the coordinator. The value cell paints the shared
/// inline field while editing, else a checkbox glyph (bool) or the value
/// text.
fn view_row(
    index: usize,
    value: &CellValue,
    is_focused: bool,
    edit_active: bool,
    theme: &Theme,
    edit_field: (TextFieldState, u32),
) -> Scene {
    let name_cell = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            PROPERTY_NAMES[index],
            Rect::default(),
            TextStyle::new()
                .with_size_px(CELL_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_padding(Rect::new(CELL_PAD, 0, CELL_PAD, 0))
                .with_size(Size::px(NAME_COL_W, ROW_H)),
        ),
    );

    let value_inner = if edit_active {
        let style = tf_paint::TextFieldStyle {
            field_w: VALUE_COL_W - CELL_PAD,
            field_h: ROW_H - 6,
            ..tf_paint::TextFieldStyle::m3_filled()
        };
        tf_paint::view_field(EDIT_TF_TAG, edit_field.0, edit_field.1, theme, &style, "")
    } else {
        match value {
            CellValue::Bool(b) => {
                view_checkbox_box(*b, CheckboxState::Idle, theme, &cell_checkbox_style())
            }
            CellValue::Choice { selected, options } => choice_value_cell(*selected, options, theme),
            CellValue::Color(c) => color_value_cell(*c, theme),
            other => Scene::Text(TextNode::styled(
                other.display(),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(CELL_PX)
                    .with_fg(theme.resolve(ColorRole::OnSurface)),
            )),
        }
    };
    let value_cell = Scene::Container(
        ContainerNode::new(vec![value_inner]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_padding(Rect::new(CELL_PAD, 0, CELL_PAD, 0))
                .with_size(Size::px(VALUE_COL_W, ROW_H)),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![name_cell, value_cell])
            .with_tag(format!("{GRID_TAG}#{index}"))
            .with_style(BoxStyle::filled(row_fill(theme, is_focused)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(NAME_COL_W + VALUE_COL_W, ROW_H)),
            ),
    )
}

/// A closed choice cell: the selected option label on the left and a
/// dropdown chevron on the right (the combobox affordance). Fills the value
/// cell's inner width so the chevron sits at the trailing edge.
fn choice_value_cell(selected: usize, options: &[String], theme: &Theme) -> Scene {
    let label = options.get(selected).map_or("", String::as_str);
    let label_node = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new().with_size_px(CELL_PX).with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let chevron = Scene::Text(TextNode::styled(
        CHOICE_CHEVRON,
        Rect::default(),
        TextStyle::new().with_size_px(CELL_PX).with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label_node, chevron]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::SpaceBetween)
                .with_size(Size::px(VALUE_COL_W - 2 * CELL_PAD, ROW_H)),
        ),
    )
}

/// A closed colour cell: a filled swatch chip plus the `#RRGGBB` hex value.
fn color_value_cell(color: Color, theme: &Theme) -> Scene {
    let swatch = Scene::Container(ContainerNode::new(vec![]).with_style(
        BoxStyle::filled(color)
            .with_corner_radius(4)
            .with_border(Border::new(theme.resolve(ColorRole::Outline), 1)),
    ).with_layout(LayoutStyle::new().with_size(Size::px(CELL_PX + 6, CELL_PX + 6))));
    let hex = Scene::Text(TextNode::styled(
        color.to_hex(),
        Rect::default(),
        TextStyle::new().with_size_px(CELL_PX).with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![swatch, hex]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_gap(8)
                .with_size(Size::px(VALUE_COL_W - 2 * CELL_PAD, ROW_H)),
        ),
    )
}

/// One popup swatch chip, tagged `{GRID_TAG}#sw{i}` so its click / hover
/// routes to the coordinator. The cursor (active descendant) and the
/// committed selection get a 2 px ring; hover too.
fn view_swatch(
    i: usize,
    color: Color,
    is_selected: bool,
    is_active: bool,
    is_hover: bool,
    theme: &Theme,
) -> Scene {
    let (border_color, border_w) = if is_active || is_hover {
        (theme.resolve(ColorRole::Accent), 2)
    } else if is_selected {
        (theme.resolve(ColorRole::OnSurface), 2)
    } else {
        (theme.resolve(ColorRole::Outline), 1)
    };
    Scene::Container(
        ContainerNode::new(vec![])
            .with_tag(format!("{GRID_TAG}#{COLOR_SW_PREFIX}{i}"))
            .with_style(
                BoxStyle::filled(color)
                    .with_corner_radius(4)
                    .with_border(Border::new(border_color, border_w)),
            )
            .with_layout(LayoutStyle::new().with_size(Size::px(SWATCH_SIZE, SWATCH_SIZE))),
    )
}

/// The open choice popup: the dropdown panel of option rows (the lifted
/// `view_option` skin, R867's 3rd consumer), absolutely positioned at — or
/// flipped above — the editing row's value cell. Each option is tagged
/// `{GRID_TAG}#opt{i}` so its click / hover routes to the coordinator. The
/// caller pushes a full-window dismiss barrier beneath it.
fn view_choice_popup(
    row: usize,
    options: &[String],
    selected: usize,
    cursor: usize,
    hover: Option<usize>,
    theme: &Theme,
) -> Scene {
    let rows: Vec<Scene> = options
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let state = if hover == Some(i) {
                ListboxItemState::Hover
            } else {
                ListboxItemState::Idle
            };
            view_option(
                &OptionRow {
                    tag: format!("{GRID_TAG}#{CHOICE_OPT_PREFIX}{i}"),
                    label,
                    state,
                    active: cursor == i,
                    selected: selected == i,
                },
                POPUP_W - 2 * POPUP_PAD,
                POPUP_OPT_H,
                theme,
            )
        })
        .collect();
    let panel_h = u32::try_from(options.len()).expect("option count fits in u32") * POPUP_OPT_H
        + 2 * POPUP_PAD;
    let (x, y) = popup_origin(row, panel_h);
    Scene::Container(
        ContainerNode::new(rows)
            .with_tag(CHOICE_POPUP_TAG)
            .with_style(popup_surface(theme))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(x, y)
                    .with_size(Size::px(POPUP_W, panel_h))
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_gap(2)
                    .with_padding(Rect::new(POPUP_PAD, POPUP_PAD, POPUP_PAD, POPUP_PAD)),
            ),
    )
}

/// The open colour popup: a grid of swatch chips for the presets plus a hex
/// field for an arbitrary `#RRGGBB[AA]` value (R869 / R870), absolutely
/// positioned at — or flipped above — the editing row's value cell. Each
/// swatch is tagged `{GRID_TAG}#sw{i}` so its click / hover routes to the
/// coordinator; the hex field is the shared `EDIT_TF` (Tab / click to focus,
/// Enter commits through `commit_edit` → `Color::from_hex`). The caller
/// pushes a full-window dismiss barrier beneath it.
fn view_color_popup(
    row: usize,
    current: Color,
    cursor: usize,
    hover: Option<usize>,
    edit_field: (TextFieldState, u32),
    theme: &Theme,
) -> Scene {
    let cols = u32::try_from(SWATCH_COLS).expect("cols fit in u32");
    let n_rows =
        u32::try_from(COLOR_SWATCHES.len().div_ceil(SWATCH_COLS)).expect("row count fits in u32");
    let inner_w = cols * SWATCH_SIZE + (cols - 1) * SWATCH_GAP;
    let mut children: Vec<Scene> = (0..COLOR_SWATCHES.len())
        .step_by(SWATCH_COLS)
        .map(|start| {
            let end = (start + SWATCH_COLS).min(COLOR_SWATCHES.len());
            let cells: Vec<Scene> = (start..end)
                .map(|i| {
                    let (color, _) = COLOR_SWATCHES[i];
                    view_swatch(i, color, color == current, cursor == i, hover == Some(i), theme)
                })
                .collect();
            Scene::Container(ContainerNode::new(cells).with_layout(
                LayoutStyle::new().flex(FlexDirection::Row).with_gap(SWATCH_GAP),
            ))
        })
        .collect();
    // The hex-entry field below the palette — the arbitrary-colour path.
    let field_style = tf_paint::TextFieldStyle {
        field_w: inner_w,
        field_h: HEX_FIELD_H,
        ..tf_paint::TextFieldStyle::m3_filled()
    };
    children.push(tf_paint::view_field(EDIT_TF_TAG, edit_field.0, edit_field.1, theme, &field_style, ""));
    let panel_w = inner_w + 2 * POPUP_PAD;
    let panel_h =
        n_rows * SWATCH_SIZE + (n_rows - 1) * SWATCH_GAP + SWATCH_GAP + HEX_FIELD_H + 2 * POPUP_PAD;
    let (x, y) = popup_origin(row, panel_h);
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(COLOR_POPUP_TAG)
            .with_style(popup_surface(theme))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(x, y)
                    .with_size(Size::px(panel_w, panel_h))
                    .flex(FlexDirection::Column)
                    .with_gap(SWATCH_GAP)
                    .with_padding(Rect::new(POPUP_PAD, POPUP_PAD, POPUP_PAD, POPUP_PAD)),
            ),
    )
}

/// The top-left of a popup of height `panel_h` for `row`: anchored at the
/// value column, dropping below the row — or flipped above it when it would
/// overflow the window bottom (the native dropdown edge behaviour). Shared by
/// the choice + colour popups; deterministic because the title band has a
/// fixed height ([`TITLE_H`]).
fn popup_origin(row: usize, panel_h: u32) -> (u32, u32) {
    let x = PANEL_PAD + GRID_BORDER + NAME_COL_W;
    let row_step = ROW_H + ROW_GAP;
    let grid_top = PANEL_PAD + TITLE_H + TITLE_GAP + GRID_BORDER;
    let row_top = grid_top + row_step + u32::try_from(row).expect("row fits in u32") * row_step;
    let below = row_top + ROW_H;
    let content_bottom = WIN_H - PANEL_PAD;
    let y = if below + panel_h <= content_bottom {
        below
    } else {
        row_top.saturating_sub(panel_h)
    };
    (x, y)
}

/// The `Property` / `Value` column-header row (the grid's static chrome).
fn view_header(theme: &Theme) -> Scene {
    let cell = |label: &str, width: u32| {
        Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::styled(
                label,
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
                    .with_size(Size::px(width, ROW_H)),
            ),
        )
    };
    Scene::Container(
        ContainerNode::new(vec![cell("Property", NAME_COL_W), cell("Value", VALUE_COL_W)])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(NAME_COL_W + VALUE_COL_W, ROW_H)),
            ),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: RootState, _frame: &Frame) -> Scene {
    let (edit_state, edit_caret) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let model = use_property_model().get();
    let focused = use_focused_row().get();
    let editing = use_editing_row().get();

    // Fixed-height title band — keeps the row → choice-popup anchor math
    // deterministic (a bare Text node's height is font-metric dependent).
    let title = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            "Inspector",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(NAME_COL_W + VALUE_COL_W, TITLE_H)),
        ),
    );

    let mut rows: Vec<Scene> = Vec::with_capacity(ROW_COUNT + 1);
    rows.push(view_header(&theme));
    for (index, value) in model.iter().enumerate() {
        let edit_active = editing == Some(index) && value.kind().is_text_editable();
        rows.push(view_row(
            index,
            value,
            index == focused,
            edit_active,
            &theme,
            (edit_state, edit_caret),
        ));
    }
    let grid = Scene::Container(
        ContainerNode::new(rows)
            .with_tag(GRID_TAG)
            .with_aria_label("Inspector")
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

    let mut children = vec![title, grid];
    // A choice / colour popup floats over the grid (absolutely positioned)
    // with a full-window light-dismiss barrier beneath it — the barrier is
    // pushed first so the panel hit-tests on top; a click outside the panel
    // routes `dismiss` to the coordinator (the toggle-close convention).
    if let Some(row) = editing {
        match model.get(row) {
            Some(CellValue::Choice { selected, options }) => {
                let cursor = use_popup_cursor().get().unwrap_or(*selected);
                let hover = use_popup_hover().get();
                children.push(dismiss_barrier(POPUP_DISMISS_TAG, (0, 0), (WIN_W, WIN_H)));
                children.push(view_choice_popup(row, options, *selected, cursor, hover, &theme));
            }
            Some(CellValue::Color(c)) => {
                let cursor = use_popup_cursor().get().unwrap_or(0);
                let hover = use_popup_hover().get();
                children.push(dismiss_barrier(POPUP_DISMISS_TAG, (0, 0), (WIN_W, WIN_H)));
                children.push(view_color_popup(
                    row,
                    *c,
                    cursor,
                    hover,
                    (edit_state, edit_caret),
                    &theme,
                ));
            }
            _ => {}
        }
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_padding(Rect::new(PANEL_PAD, PANEL_PAD, PANEL_PAD, PANEL_PAD))
                    .with_gap(TITLE_GAP)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

// ─── WidgetCore impl ──────────────────────────────────────────────

/// Cached paint posture — only the shared inline field's interaction state +
/// caret. The model / cursor / edit-mode are read reactively in the view fn
/// (the todomvc shape: read_state carries the field posture, hooks carry the
/// reactive model).
type RootState = (TextFieldState, u32);

struct PropertyGridView;

impl WidgetCore for PropertyGridView {
    type State = RootState;
    // Editing flows through apply_key + the pointer router; the coordinator's
    // intents are observed in `update`. No keybinding-channel events.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let model = use_property_model();
        let focused = use_focused_row();
        let editing = use_editing_row();
        let editor = use_text_edit_state(EDIT_TF_TAG);
        let popup_cursor = use_popup_cursor();
        let popup_hover = use_popup_hover();
        Box::new(PropertyGridExternal::new(
            model,
            focused,
            editing,
            editor,
            popup_cursor,
            popup_hover,
        ))
    }

    fn tag() -> &'static str {
        GRID_TAG
    }

    /// One shared inline editor as an extra external — the same
    /// `TextEditState` the coordinator's `begin_edit` seeds (Owner::cache
    /// dedup). `with_blur_intent` opts the field into the R793 commit-on-blur
    /// signal the `update` reducer drains.
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
        "pinion hello-property-grid (R836 §5.38 inspector detail panel)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// The grid is one Tab stop; the inline editor is the second (focusable
    /// only while painted — entered through `focus_request`, the todomvc
    /// dynamic-editor shape).
    fn focusable_tags() -> Vec<&'static str> {
        vec![GRID_TAG, EDIT_TF_TAG]
    }

    /// R793 §5.38 — commit-on-blur: the inline editor lost focus (a click
    /// elsewhere) while editing → commit without restoring focus (the click
    /// already moved it). The `editing_row` gate makes the post-commit blur
    /// (focus restored to the grid) a no-op, so only a genuine click-away
    /// commits. Gated to a text-editable row so a choice popup (which shares
    /// `editing_row` but paints no field) is never blur-committed.
    fn update(_state: RootState, intent: &pinion_core::Intent) -> Vec<Command> {
        let editing_text = editing_kind().is_some_and(CellKind::is_text_editable);
        if intent.tag_str() == EDIT_TF_BLUR_INTENT_TAG && editing_text {
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

    fn apply_composition(
        scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        if focused != Some(EDIT_TF_TAG) {
            return false;
        }
        let Some(node) = scene.find_external_with_tag_mut(EDIT_TF_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        let args = match event {
            pinion_core::CompositionEvent::Start => {
                IntrospectValue::Json(serde_json::json!({ "action": "start" }))
            }
            pinion_core::CompositionEvent::Update(text) => {
                IntrospectValue::Json(serde_json::json!({ "action": "update", "data": text }))
            }
            pinion_core::CompositionEvent::Commit(text) => {
                IntrospectValue::Json(serde_json::json!({ "action": "end", "data": text }))
            }
            _ => IntrospectValue::Json(serde_json::json!({ "action": "cancel" })),
        };
        intro.invoke("composition", args).is_ok()
    }
}

impl WidgetA11y for PropertyGridView {
    /// R836 §5.40 — the panel lowers to a WAI-ARIA `grid` through the lifted
    /// [`grid_table_nodes`] SSOT (3rd consumer). `Property` / `Value` header
    /// over one data row per property. The roving cursor's
    /// `aria-activedescendant` is supplied by [`Self::access_focus_target`]
    /// (R870); the typed value is encoded in the cell name so AT hears the
    /// value with its context.
    ///
    /// R867 — an open choice popup additionally lowers to a WAI-ARIA
    /// `listbox` (the lifted [`listbox_option_nodes`] SSOT, the combobox a11y
    /// shape): each option carries `aria-selected` (the committed value), and
    /// `access_focus_target` points the active descendant at the cursor
    /// option / swatch.
    fn access_node(_state: &RootState, _focused: Option<&str>) -> Vec<AccessNode> {
        let model = use_property_model().get();
        let focused = use_focused_row().get();
        let columns = vec![
            GridColumn { tag: "pg_col_name".to_owned(), label: "Property".to_owned(), sort: None },
            GridColumn { tag: "pg_col_value".to_owned(), label: "Value".to_owned(), sort: None },
        ];
        let rows: Vec<GridRow> = model
            .iter()
            .enumerate()
            .map(|(index, value)| GridRow {
                tag: format!("pg_row{index}"),
                selected: false,
                state: RadioState::Idle,
                cells: vec![
                    GridCell {
                        tag: format!("pg_name{index}"),
                        name: PROPERTY_NAMES[index].to_owned(),
                        focused: false,
                    },
                    GridCell {
                        tag: format!("{GRID_TAG}#{index}"),
                        name: format!("{}: {}", PROPERTY_NAMES[index], value.display()),
                        focused: index == focused,
                    },
                ],
            })
            .collect();
        let mut nodes =
            grid_table_nodes(GRID_TAG, "Inspector", false, "pg_header", &columns, &rows);
        if let Some(row) = use_editing_row().get() {
            match model.get(row) {
                Some(CellValue::Choice { selected, options }) => {
                    let cursor = use_popup_cursor().get().unwrap_or(*selected);
                    let hover = use_popup_hover().get();
                    let tags: Vec<String> = (0..options.len())
                        .map(|i| format!("{GRID_TAG}#{CHOICE_OPT_PREFIX}{i}"))
                        .collect();
                    let opts: Vec<ListOption<'_>> = options
                        .iter()
                        .enumerate()
                        .map(|(i, label)| ListOption {
                            tag: &tags[i],
                            label: Some(label.as_str()),
                            state: if hover == Some(i) {
                                ListboxItemState::Hover
                            } else {
                                ListboxItemState::Idle
                            },
                            selected: *selected == i,
                            focused: cursor == i,
                        })
                        .collect();
                    let name = format!("{} options", PROPERTY_NAMES[row]);
                    nodes.extend(listbox_option_nodes(CHOICE_POPUP_TAG, &name, false, &opts));
                }
                Some(CellValue::Color(current)) => {
                    let cursor = use_popup_cursor().get().unwrap_or(0);
                    let hover = use_popup_hover().get();
                    let tags: Vec<String> = (0..COLOR_SWATCHES.len())
                        .map(|i| format!("{GRID_TAG}#{COLOR_SW_PREFIX}{i}"))
                        .collect();
                    let opts: Vec<ListOption<'_>> = COLOR_SWATCHES
                        .iter()
                        .enumerate()
                        .map(|(i, &(color, label))| ListOption {
                            tag: &tags[i],
                            label: Some(label),
                            state: if hover == Some(i) {
                                ListboxItemState::Hover
                            } else {
                                ListboxItemState::Idle
                            },
                            selected: color == *current,
                            focused: cursor == i,
                        })
                        .collect();
                    let name = format!("{} swatches", PROPERTY_NAMES[row]);
                    nodes.extend(listbox_option_nodes(COLOR_POPUP_TAG, &name, false, &opts));
                }
                _ => {}
            }
        }
        nodes
    }

    /// R870 — composite focus: while the grid owns shell focus, the
    /// `aria-activedescendant` follows the keyboard cursor. With a popup open
    /// it names the active popup option / swatch; otherwise it names the
    /// focused value cell (the roving row cursor). This is the authoritative
    /// active-descendant channel (the per-node `with_focused` flag is a
    /// redundant marker the AT layer does not lower) — the combobox / treegrid
    /// pattern, previously missing here.
    fn access_focus_target(_state: &RootState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(GRID_TAG) {
            if let Some(row) = use_editing_row().get() {
                let cursor = use_popup_cursor().get().unwrap_or(0);
                match use_property_model().get().get(row).map(CellValue::kind) {
                    Some(CellKind::Choice) => {
                        return Some(AccessFocus::composite(
                            GRID_TAG,
                            format!("{GRID_TAG}#{CHOICE_OPT_PREFIX}{cursor}"),
                        ));
                    }
                    Some(CellKind::Color) => {
                        return Some(AccessFocus::composite(
                            GRID_TAG,
                            format!("{GRID_TAG}#{COLOR_SW_PREFIX}{cursor}"),
                        ));
                    }
                    _ => {}
                }
            }
            let focused_row = use_focused_row().get();
            return Some(AccessFocus::composite(GRID_TAG, format!("{GRID_TAG}#{focused_row}")));
        }
        focused.map(AccessFocus::atomic)
    }
}

impl WidgetView for PropertyGridView {
    type Renderer = HelloPropertyGridRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<PropertyGridView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    // The typed-value pure helpers (kind / name / parse / display / the
    // keystroke gate) are now tested in `pinion_core::cell_value`; this
    // module tests the property-grid WIRING (coordinator, edit flow,
    // keyboard, a11y, paint) on top of the lifted SSOT.

    #[test]
    fn r836_default_model_matches_name_count() {
        assert_eq!(default_properties().len(), ROW_COUNT);
        assert_eq!(PROPERTY_NAMES.len(), ROW_COUNT);
    }

    // ----- coordinator + scene fixture -----

    fn boot_scene() -> Scene {
        let mut children = vec![Scene::External(
            ExternalNode::new(PropertyGridView::create_external()).with_tag(GRID_TAG),
        )];
        for extra in PropertyGridView::create_extra_externals() {
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
    fn r836_query_exposes_typed_values_names_kinds() {
        Owner::new().run(|| {
            let scene = boot_scene();
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("row_count"), Some(IntrospectValue::Int(12)));
            assert_eq!(intro.query("focused_row"), Some(IntrospectValue::Int(0)));
            assert_eq!(intro.query("name.0"), Some(IntrospectValue::Text("Name".to_owned())));
            assert_eq!(intro.query("kind.2"), Some(IntrospectValue::Text("bool".to_owned())));
            assert_eq!(intro.query("kind.4"), Some(IntrospectValue::Text("int".to_owned())));
            assert_eq!(intro.query("kind.6"), Some(IntrospectValue::Text("float".to_owned())));
            assert_eq!(intro.query("value.2"), Some(IntrospectValue::Bool(true)));
            assert_eq!(intro.query("value.4"), Some(IntrospectValue::Int(3)));
            assert_eq!(intro.query("value.6"), Some(IntrospectValue::Float(12.5)));
            assert_eq!(intro.query("value.0"), Some(IntrospectValue::Text("Player".to_owned())));
            assert_eq!(intro.query("value.99"), None, "out-of-range -> None");
            assert_eq!(intro.query("editing"), Some(IntrospectValue::Json(serde_json::Value::Null)));
        });
    }

    #[test]
    fn r836_intervene_sets_typed_value_strictly() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Strict per kind.
            assert!(intro.intervene("value.4", IntrospectValue::Int(17)).is_ok());
            assert_eq!(
                intro.intervene("value.4", IntrospectValue::Text("no".to_owned())),
                Err(InterveneError::TypeMismatch),
                "int row rejects text",
            );
            assert!(intro.intervene("value.2", IntrospectValue::Bool(false)).is_ok());
            assert_eq!(
                intro.intervene("editing", IntrospectValue::Int(0)),
                Err(InterveneError::ReadOnly),
            );
            assert_eq!(intro.query("value.4"), Some(IntrospectValue::Int(17)));
            assert_eq!(intro.query("value.2"), Some(IntrospectValue::Bool(false)));
        });
    }

    #[test]
    fn r836_intervene_focused_row_clamps() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert!(intro.intervene("focused_row", IntrospectValue::Int(4)).is_ok());
            assert_eq!(intro.query("focused_row"), Some(IntrospectValue::Int(4)));
            assert!(intro.intervene("focused_row", IntrospectValue::Int(999)).is_ok());
            assert_eq!(intro.query("focused_row"), Some(IntrospectValue::Int(11)), "clamped to last");
        });
    }

    #[test]
    fn r836_toggle_invoke_flips_focused_bool_only() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Focus a bool row (index 2 = Visible) then toggle.
            let _ = intro.intervene("focused_row", IntrospectValue::Int(2));
            assert_eq!(intro.invoke("toggle", IntrospectValue::Null), Ok(IntrospectValue::Bool(true)));
            assert_eq!(intro.query("value.2"), Some(IntrospectValue::Bool(false)));
            // Focus a non-bool row -> toggle is a no-op.
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            assert_eq!(intro.invoke("toggle", IntrospectValue::Null), Ok(IntrospectValue::Bool(false)));
            assert_eq!(intro.query("value.0"), Some(IntrospectValue::Text("Player".to_owned())));
        });
    }

    #[test]
    fn r836_click_focuses_row_and_toggles_bool() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // PointerUp on the Locked bool (index 3) focuses + toggles it.
            let _ = intro.invoke("send", IntrospectValue::Text("3:PointerUp".to_owned()));
            assert_eq!(intro.query("focused_row"), Some(IntrospectValue::Int(3)));
            assert_eq!(intro.query("value.3"), Some(IntrospectValue::Bool(true)), "false -> true");
            // PointerUp on a text row focuses but does not toggle.
            let _ = intro.invoke("send", IntrospectValue::Text("0:PointerUp".to_owned()));
            assert_eq!(intro.query("focused_row"), Some(IntrospectValue::Int(0)));
        });
    }

    // ----- edit-in-cell flow -----

    #[test]
    fn r836_begin_commit_writes_back_parsed_value() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // begin_edit on the Layer int row (index 4) via invoke (the RPC
            // edit-entry path) seeds the shared editor with the value text.
            let n = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = n.handle.introspect_mut().expect("introspectable");
            assert_eq!(intro.invoke("begin", IntrospectValue::Int(4)), Ok(IntrospectValue::Bool(true)));
            assert_eq!(intro.query("editing"), Some(IntrospectValue::Json(serde_json::Value::from(4))));
            assert_eq!(use_text_edit_state(EDIT_TF_TAG).text(), "3", "seeded with Layer value");
            // Type a new value + commit.
            use_text_edit_state(EDIT_TF_TAG).set_text("12".to_owned());
            commit_edit(true);
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("value.4"), Some(IntrospectValue::Int(12)), "committed");
            assert_eq!(intro.query("editing"), Some(IntrospectValue::Json(serde_json::Value::Null)));
        });
    }

    #[test]
    fn r836_begin_rejects_bool_rows() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let n = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = n.handle.introspect_mut().expect("introspectable");
            assert_eq!(intro.invoke("begin", IntrospectValue::Int(2)), Ok(IntrospectValue::Bool(false)));
            assert_eq!(intro.query("editing"), Some(IntrospectValue::Json(serde_json::Value::Null)));
        });
    }

    #[test]
    fn r836_cancel_keeps_prior_value() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let n = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = n.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("begin", IntrospectValue::Int(0));
            use_text_edit_state(EDIT_TF_TAG).set_text("changed".to_owned());
            cancel_edit();
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("value.0"), Some(IntrospectValue::Text("Player".to_owned())));
            assert_eq!(intro.query("editing"), Some(IntrospectValue::Json(serde_json::Value::Null)));
        });
    }

    #[test]
    fn r836_commit_malformed_number_reverts() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let n = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = n.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("begin", IntrospectValue::Int(4));
            use_text_edit_state(EDIT_TF_TAG).set_text("not a number".to_owned());
            commit_edit(true);
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("value.4"), Some(IntrospectValue::Int(3)), "kept prior value");
        });
    }

    // ----- keyboard -----

    #[test]
    fn r836_grid_arrows_navigate_and_clamp() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", Modifiers::empty()));
            assert_eq!(use_focused_row().get(), 1);
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "End", Modifiers::empty()));
            assert_eq!(use_focused_row().get(), ROW_COUNT - 1);
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", Modifiers::empty()));
            assert_eq!(use_focused_row().get(), ROW_COUNT - 1, "clamps at the bottom");
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "Home", Modifiers::empty()));
            assert_eq!(use_focused_row().get(), 0);
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowUp", Modifiers::empty()));
            assert_eq!(use_focused_row().get(), 0, "clamps at the top");
        });
    }

    #[test]
    fn r836_space_toggles_bool_enter_edits_text() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Move to the Visible bool (index 2) and Space-toggle it.
            let _ = scene
                .find_external_with_tag_mut(GRID_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .map(|i| i.intervene("focused_row", IntrospectValue::Int(2)));
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "Space", Modifiers::empty()));
            assert_eq!(grid_intro(&scene).query("value.2"), Some(IntrospectValue::Bool(false)));
            // Move to the Name text row and Enter -> edit mode.
            let _ = scene
                .find_external_with_tag_mut(GRID_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .map(|i| i.intervene("focused_row", IntrospectValue::Int(0)));
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "Enter", Modifiers::empty()));
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Some(IntrospectValue::Json(serde_json::Value::from(0))),
            );
        });
    }

    #[test]
    fn r836_edit_enter_commits_escape_cancels() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let _ = scene
                .find_external_with_tag_mut(GRID_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .map(|i| i.invoke("begin", IntrospectValue::Int(0)));
            use_text_edit_state(EDIT_TF_TAG).set_text("Enemy".to_owned());
            assert!(PropertyGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), "Enter", Modifiers::empty()));
            assert_eq!(grid_intro(&scene).query("value.0"), Some(IntrospectValue::Text("Enemy".to_owned())));
        });
    }

    #[test]
    fn r836_edit_int_gate_drops_letters() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let _ = scene
                .find_external_with_tag_mut(GRID_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .map(|i| i.invoke("begin", IntrospectValue::Int(4)));
            use_text_edit_state(EDIT_TF_TAG).set_text(String::new());
            use_text_edit_state(EDIT_TF_TAG).set_caret(0);
            assert!(PropertyGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), "9", Modifiers::empty()), "digit accepted");
            assert!(!PropertyGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), "x", Modifiers::empty()), "letter dropped");
            assert_eq!(use_text_edit_state(EDIT_TF_TAG).text(), "9");
        });
    }

    #[test]
    fn r836_keys_ignored_when_unfocused() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(!PropertyGridView::apply_key(&mut scene, None, "ArrowDown", Modifiers::empty()));
            assert_eq!(use_focused_row().get(), 0, "cursor unchanged");
        });
    }

    // ----- a11y -----

    #[test]
    fn r836_access_node_emits_grid_with_rows_and_active_cell() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            scene_focus(4);
            let nodes = PropertyGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            // grid + header row + 2 columnheaders + 9 rows + 18 cells.
            assert_eq!(nodes.len(), 1 + 1 + 2 + ROW_COUNT + ROW_COUNT * 2);
            assert_eq!(nodes[0].role, pinion_a11y::AriaRole::Grid);
            assert_eq!(nodes[0].name.as_deref(), Some("Inspector"));
            let active = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#4"))
                .expect("focused value cell present");
            assert!(active.state.focused, "focused row's value cell is the active descendant");
            let visible_cell = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#2"))
                .expect("bool value cell present");
            assert_eq!(visible_cell.name.as_deref(), Some("Visible: On"));
        });
    }

    fn scene_focus(row: usize) {
        use_focused_row().set(row);
    }

    // ----- choice popup (R867) -----

    /// The Blend choice row — index 9, 4 options, default `Normal`.
    const BLEND_ROW: usize = 9;

    fn open_choice(scene: &mut Scene, row: usize) {
        let _ = scene
            .find_external_with_tag_mut(GRID_TAG)
            .and_then(|n| n.handle.introspect_mut())
            .map(|i| i.invoke("begin", IntrospectValue::Int(i64::try_from(row).unwrap())));
    }

    #[test]
    fn r867_choice_boot_taxonomy() {
        Owner::new().run(|| {
            let scene = boot_scene();
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("kind.9"), Some(IntrospectValue::Text("choice".to_owned())));
            assert_eq!(intro.query("kind.10"), Some(IntrospectValue::Text("choice".to_owned())));
            let Some(IntrospectValue::Json(blend)) = intro.query("value.9") else {
                panic!("choice value is json");
            };
            assert_eq!(blend["selected"], serde_json::json!(0));
            assert_eq!(blend["label"], serde_json::json!("Normal"));
            assert_eq!(
                blend["options"],
                serde_json::json!(["Normal", "Additive", "Multiply", "Screen"]),
            );
            assert_eq!(intro.query("popup_cursor"), Some(IntrospectValue::Null), "no popup at boot");
        });
    }

    #[test]
    fn r867_dismiss_tag_is_the_grid_composite() {
        // SSOT guard: the &'static dismiss tag must stay `{GRID_TAG}#dismiss`.
        assert_eq!(POPUP_DISMISS_TAG, format!("{GRID_TAG}#dismiss"));
    }

    #[test]
    fn r867_begin_opens_popup_and_seeds_cursor() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            open_choice(&mut scene, BLEND_ROW);
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("editing"), Some(IntrospectValue::Json(serde_json::Value::from(9))));
            assert_eq!(
                intro.query("popup_cursor"),
                Some(IntrospectValue::Int(0)),
                "cursor seeded at the committed option",
            );
        });
    }

    #[test]
    fn r867_keyboard_roves_clamps_and_commits() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            open_choice(&mut scene, BLEND_ROW);
            // Down x5 clamps at the last of 4 options (index 3).
            for _ in 0..5 {
                assert!(PropertyGridView::apply_key(
                    &mut scene,
                    Some(GRID_TAG),
                    "ArrowDown",
                    Modifiers::empty(),
                ));
            }
            assert_eq!(grid_intro(&scene).query("popup_cursor"), Some(IntrospectValue::Int(3)));
            // Enter commits the cursor (Screen) and closes the popup.
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "Enter", Modifiers::empty()));
            let Some(IntrospectValue::Json(v)) = grid_intro(&scene).query("value.9") else {
                panic!("json");
            };
            assert_eq!(v["label"], serde_json::json!("Screen"));
            assert_eq!(grid_intro(&scene).query("editing"), Some(IntrospectValue::Json(serde_json::Value::Null)));
        });
    }

    #[test]
    fn r867_escape_dismisses_without_commit() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            open_choice(&mut scene, BLEND_ROW);
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", Modifiers::empty()));
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "Escape", Modifiers::empty()));
            assert_eq!(grid_intro(&scene).query("editing"), Some(IntrospectValue::Json(serde_json::Value::Null)));
            let Some(IntrospectValue::Json(v)) = grid_intro(&scene).query("value.9") else {
                panic!("json");
            };
            assert_eq!(v["selected"], serde_json::json!(0), "Escape leaves the committed value");
        });
    }

    #[test]
    fn r867_pointer_click_and_choose_commit_and_close() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let n = scene.find_external_with_tag_mut(GRID_TAG).expect("grid");
            let intro = n.handle.introspect_mut().expect("introspectable");
            // Single-click the Blend row opens the popup; clicking option 2
            // (Multiply) commits + closes.
            let _ = intro.invoke("send", IntrospectValue::Text("9:PointerUp".to_owned()));
            assert_eq!(intro.query("editing"), Some(IntrospectValue::Json(serde_json::Value::from(9))));
            let _ = intro.invoke("send", IntrospectValue::Text("opt2:PointerUp".to_owned()));
            assert_eq!(intro.query("editing"), Some(IntrospectValue::Json(serde_json::Value::Null)));
            let Some(IntrospectValue::Json(v)) = intro.query("value.9") else { panic!("json") };
            assert_eq!(v["label"], serde_json::json!("Multiply"));
            // The RPC `choose` path commits + closes too (Body row 10 -> None).
            let _ = intro.invoke("begin", IntrospectValue::Int(10));
            assert_eq!(intro.invoke("choose", IntrospectValue::Int(0)), Ok(IntrospectValue::Bool(true)));
            let Some(IntrospectValue::Json(v)) = intro.query("value.10") else { panic!("json") };
            assert_eq!(v["label"], serde_json::json!("None"));
        });
    }

    #[test]
    fn r867_barrier_dismisses_and_intervene_sets_by_index() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let n = scene.find_external_with_tag_mut(GRID_TAG).expect("grid");
            let intro = n.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("send", IntrospectValue::Text("9:PointerUp".to_owned()));
            // Clicking the dismiss barrier closes without committing.
            let _ = intro.invoke("send", IntrospectValue::Text("dismiss:PointerUp".to_owned()));
            assert_eq!(intro.query("editing"), Some(IntrospectValue::Json(serde_json::Value::Null)));
            // Direct AI set by index (no popup needed) + strict errors.
            assert!(intro.intervene("value.9", IntrospectValue::Int(1)).is_ok());
            let Some(IntrospectValue::Json(v)) = intro.query("value.9") else { panic!("json") };
            assert_eq!(v["label"], serde_json::json!("Additive"));
            assert_eq!(
                intro.intervene("value.9", IntrospectValue::Int(9)),
                Err(InterveneError::OutOfRange),
            );
            assert_eq!(
                intro.intervene("value.9", IntrospectValue::Text("x".to_owned())),
                Err(InterveneError::TypeMismatch),
            );
            assert_eq!(
                intro.intervene("popup_cursor", IntrospectValue::Int(0)),
                Err(InterveneError::ReadOnly),
            );
        });
    }

    #[test]
    fn r867_view_and_a11y_expose_the_open_popup() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Closed: no popup paint, no listbox a11y.
            let closed = view((TextFieldState::Idle, 0), &Frame::new());
            assert!(!closed.contains_tag(CHOICE_POPUP_TAG), "no panel when closed");
            let closed_nodes =
                PropertyGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            assert!(
                !closed_nodes.iter().any(|n| n.role == pinion_a11y::AriaRole::Listbox),
                "no listbox when closed",
            );
            // Open the Blend popup.
            open_choice(&mut scene, BLEND_ROW);
            let open = view((TextFieldState::Idle, 0), &Frame::new());
            assert!(open.contains_tag(CHOICE_POPUP_TAG), "panel painted when open");
            assert!(open.contains_tag(POPUP_DISMISS_TAG), "dismiss barrier painted");
            assert!(open.contains_tag(&format!("{GRID_TAG}#opt0")), "option 0 painted");
            assert!(open.contains_tag(&format!("{GRID_TAG}#opt3")), "option 3 painted");
            let nodes = PropertyGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            let listbox = nodes
                .iter()
                .find(|n| n.role == pinion_a11y::AriaRole::Listbox)
                .expect("listbox node when open");
            assert_eq!(listbox.name.as_deref(), Some("Blend options"));
            let options: Vec<_> =
                nodes.iter().filter(|n| n.role == pinion_a11y::AriaRole::ListBoxOption).collect();
            assert_eq!(options.len(), 4, "one option node per choice");
            assert_eq!(options[0].selected, Some(true), "option 0 is aria-selected (Normal)");
            assert!(options[0].state.focused, "cursor 0 is the active descendant");
        });
    }

    // ----- colour popup (R869) -----

    /// The Tint colour row — index 11, default Blue (swatch index 4).
    const TINT_ROW: usize = 11;

    #[test]
    fn r869_color_boot_taxonomy() {
        Owner::new().run(|| {
            let scene = boot_scene();
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("kind.11"), Some(IntrospectValue::Text("color".to_owned())));
            let Some(IntrospectValue::Json(tint)) = intro.query("value.11") else {
                panic!("colour value is json");
            };
            assert_eq!(tint["hex"], serde_json::json!("#1e88e5"), "Tint boots Blue");
            assert_eq!(tint["r"], serde_json::json!(0x1e));
            assert_eq!(tint["b"], serde_json::json!(0xe5));
        });
    }

    #[test]
    fn r869_begin_opens_popup_and_seeds_swatch_cursor() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            open_choice(&mut scene, TINT_ROW); // shared open helper (invoke begin)
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("editing"), Some(IntrospectValue::Json(serde_json::Value::from(11))));
            assert_eq!(
                intro.query("popup_cursor"),
                Some(IntrospectValue::Int(4)),
                "cursor seeded at the swatch matching the committed colour (Blue=4)",
            );
        });
    }

    #[test]
    fn r869_keyboard_roves_and_commits_swatch() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            open_choice(&mut scene, TINT_ROW);
            // Blue(4) -> Right -> Yellow(5).
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowRight", Modifiers::empty()));
            assert_eq!(grid_intro(&scene).query("popup_cursor"), Some(IntrospectValue::Int(5)));
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "Enter", Modifiers::empty()));
            let Some(IntrospectValue::Json(v)) = grid_intro(&scene).query("value.11") else {
                panic!("json");
            };
            assert_eq!(v["hex"], serde_json::json!("#fdd835"), "committed Yellow");
            assert_eq!(grid_intro(&scene).query("editing"), Some(IntrospectValue::Json(serde_json::Value::Null)));
        });
    }

    #[test]
    fn r869_pick_color_click_and_intervene_hex() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let n = scene.find_external_with_tag_mut(GRID_TAG).expect("grid");
            let intro = n.handle.introspect_mut().expect("introspectable");
            // Single-click the Tint row opens; clicking swatch 2 (Red) commits.
            let _ = intro.invoke("send", IntrospectValue::Text("11:PointerUp".to_owned()));
            assert_eq!(intro.query("editing"), Some(IntrospectValue::Json(serde_json::Value::from(11))));
            let _ = intro.invoke("send", IntrospectValue::Text("sw2:PointerUp".to_owned()));
            let Some(IntrospectValue::Json(v)) = intro.query("value.11") else { panic!("json") };
            assert_eq!(v["hex"], serde_json::json!("#e53935"), "clicked Red");
            assert_eq!(intro.query("editing"), Some(IntrospectValue::Json(serde_json::Value::Null)));
            // The RPC pick_color path commits + closes too.
            let _ = intro.invoke("begin", IntrospectValue::Int(11));
            assert_eq!(intro.invoke("pick_color", IntrospectValue::Int(0)), Ok(IntrospectValue::Bool(true)));
            let Some(IntrospectValue::Json(v)) = intro.query("value.11") else { panic!("json") };
            assert_eq!(v["hex"], serde_json::json!("#ffffff"), "pick_color 0 -> White");
            // intervene sets an arbitrary colour by hex (the AI-first path).
            assert!(intro.intervene("value.11", IntrospectValue::Text("#abcdef".to_owned())).is_ok());
            let Some(IntrospectValue::Json(v)) = intro.query("value.11") else { panic!("json") };
            assert_eq!(v["hex"], serde_json::json!("#abcdef"));
            assert_eq!(
                intro.intervene("value.11", IntrospectValue::Text("nope".to_owned())),
                Err(InterveneError::OutOfRange),
            );
        });
    }

    #[test]
    fn r869_view_and_a11y_expose_the_open_color_popup() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let closed = view((TextFieldState::Idle, 0), &Frame::new());
            assert!(!closed.contains_tag(COLOR_POPUP_TAG), "no colour panel when closed");
            open_choice(&mut scene, TINT_ROW);
            let open = view((TextFieldState::Idle, 0), &Frame::new());
            assert!(open.contains_tag(COLOR_POPUP_TAG), "colour panel painted when open");
            assert!(open.contains_tag(POPUP_DISMISS_TAG), "dismiss barrier painted");
            assert!(open.contains_tag(&format!("{GRID_TAG}#sw0")), "swatch 0 painted");
            assert!(open.contains_tag(&format!("{GRID_TAG}#sw7")), "swatch 7 painted");
            let nodes = PropertyGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            let listbox = nodes
                .iter()
                .find(|n| n.role == pinion_a11y::AriaRole::Listbox)
                .expect("colour listbox node when open");
            assert_eq!(listbox.name.as_deref(), Some("Tint swatches"));
            let swatches: Vec<_> =
                nodes.iter().filter(|n| n.role == pinion_a11y::AriaRole::ListBoxOption).collect();
            assert_eq!(swatches.len(), COLOR_SWATCHES.len(), "one option node per swatch");
            // Blue (index 4) is the committed selection + the cursor.
            assert_eq!(swatches[4].selected, Some(true), "Blue swatch is aria-selected");
            assert!(swatches[4].state.focused, "Blue swatch is the active descendant");
            assert_eq!(swatches[2].name.as_deref(), Some("Red"));
        });
    }

    #[test]
    fn r870_access_focus_target_tracks_the_cursor() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Navigating: the focused value cell is the active descendant.
            use_focused_row().set(5);
            let f = PropertyGridView::access_focus_target(&(TextFieldState::Idle, 0), Some(GRID_TAG))
                .expect("grid focused -> composite focus target");
            assert_eq!(f.focus_tag, GRID_TAG);
            assert_eq!(f.active_descendant.as_deref(), Some(format!("{GRID_TAG}#5").as_str()));
            // Choice popup open -> the active option (Blend cursor boots 0).
            open_choice(&mut scene, BLEND_ROW);
            let f = PropertyGridView::access_focus_target(&(TextFieldState::Idle, 0), Some(GRID_TAG))
                .expect("composite");
            assert_eq!(f.active_descendant.as_deref(), Some(format!("{GRID_TAG}#opt0").as_str()));
            // Colour popup open -> the active swatch (Tint boots Blue=4).
            open_choice(&mut scene, TINT_ROW);
            let f = PropertyGridView::access_focus_target(&(TextFieldState::Idle, 0), Some(GRID_TAG))
                .expect("composite");
            assert_eq!(f.active_descendant.as_deref(), Some(format!("{GRID_TAG}#sw4").as_str()));
            // Focus elsewhere -> atomic, no active descendant.
            let other =
                PropertyGridView::access_focus_target(&(TextFieldState::Idle, 0), Some(EDIT_TF_TAG));
            assert!(other.expect("atomic").active_descendant.is_none());
        });
    }

    #[test]
    fn r870_hex_field_commits_an_arbitrary_colour() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            open_choice(&mut scene, TINT_ROW); // opens the colour popup, seeds the hex field
            assert_eq!(use_text_edit_state(EDIT_TF_TAG).text(), "#1e88e5", "hex field seeded Blue");
            // The popup paints the hex field (the shared EDIT_TF).
            let painted = view((TextFieldState::Idle, 0), &Frame::new());
            assert!(painted.contains_tag(EDIT_TF_TAG), "hex field painted in the colour popup");
            // Type an arbitrary hex + Enter (the EDIT_TF-focused commit path).
            use_text_edit_state(EDIT_TF_TAG).set_text("#abcdef".to_owned());
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(EDIT_TF_TAG),
                "Enter",
                Modifiers::empty(),
            ));
            let Some(IntrospectValue::Json(v)) = grid_intro(&scene).query("value.11") else {
                panic!("json");
            };
            assert_eq!(v["hex"], serde_json::json!("#abcdef"), "hex field commits arbitrary colour");
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Some(IntrospectValue::Json(serde_json::Value::Null)),
                "popup closed after hex commit",
            );
        });
    }

    // ----- view -----

    #[test]
    fn r836_view_carries_grid_and_row_tags() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let scene = view((TextFieldState::Idle, 0), &Frame::new());
            assert!(scene.contains_tag(GRID_TAG), "grid root painted");
            assert!(scene.contains_tag(&format!("{GRID_TAG}#0")), "row 0 painted");
            assert!(scene.contains_tag(&format!("{GRID_TAG}#8")), "row 8 painted");
        });
    }

    #[test]
    fn r836_view_paints_inline_field_only_while_editing() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let before = view((TextFieldState::Idle, 0), &Frame::new());
            assert!(!before.contains_tag(EDIT_TF_TAG), "no inline field when not editing");
            use_editing_row().set(Some(0));
            let during = view((TextFieldState::Idle, 0), &Frame::new());
            assert!(during.contains_tag(EDIT_TF_TAG), "inline field painted in the editing row");
        });
    }

    #[test]
    fn r836_view_contains_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<PropertyGridView>(
            (TextFieldState::Idle, 0),
            &Frame::default(),
        );
    }
}
