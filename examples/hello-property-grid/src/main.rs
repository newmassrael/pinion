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
//!   ([`Signal<Vec<PropertyValue>>`] — the value SSOT), the roving cursor
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
//! name cell + a `gridcell` value cell. The focused row's value cell carries
//! the roving `focused` flag (`aria-activedescendant`). The typed value is
//! encoded in the cell's accessible name (`"Opacity: 1"`) so an AT user
//! hears the value with its column context.
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
//! - **Enum / choice rows.** A combobox-in-cell is a nested popup-overlay +
//!   3-level focus question (grid → cell → popup) that is its own substrate
//!   round; this binding ships the bool / int / float / text quartet.

use std::rc::Rc;

use pinion_a11y::{grid_table_nodes, AccessNode, GridCell, GridColumn, GridRow, WidgetA11y};
use pinion_core::composite_tag::parse_send_payload;
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
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::text_edit::{use_text_edit_state, TextEditState};
use pinion_core::widgets::text_field::{TextFieldExternal, TextFieldState};
use pinion_core::{Color, Command, Frame, Modifiers, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::text_field as tf_paint;

use pinion_widget_paint::state_layer::HOVER;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloPropertyGridRenderer, HelloPropertyGridRendererError);

// ─── window + layout constants ─────────────────────────────────────

const WIN_W: u32 = 460;
const WIN_H: u32 = 440;
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

// ─── tags + intents ───────────────────────────────────────────────

/// Primary External — the grid coordinator (the single keyboard Tab stop).
const GRID_TAG: &str = "property_grid";
/// Extra External — the one shared inline text / number editor.
const EDIT_TF_TAG: &str = "property_grid_edit";
/// Commit-on-blur intent the inline field raises on a click-away (R793).
const EDIT_TF_BLUR_INTENT_TAG: &str = pinion_core::intent_tag!("property_grid_edit", "blur");

/// U+2713 CHECK MARK — the bool-cell affordance (named const + escape per
/// [[non-ascii-literal-named-const-escape]]; raw glyph in docs only).
const CHECK_GLYPH: &str = "\u{2713}";

// ─── typed property model ─────────────────────────────────────────

const ROW_COUNT: usize = 9;

/// The property row names. Static — only the [`PropertyValue`]s mutate, so
/// names live in a `const` (the value SSOT is the coordinator's Signal).
const PROPERTY_NAMES: [&str; ROW_COUNT] = [
    "Name", "Tag", "Visible", "Locked", "Layer", "Health", "Pos X", "Pos Y", "Opacity",
];

/// A typed property value — the editable cell's payload. The unifying
/// abstraction a property grid adds over a plain form: one row model, four
/// editor renderings, dispatched by [`kind_of`].
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum PropertyValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

/// The static descriptor of a [`PropertyValue`] — drives editor behaviour
/// (toggle vs text-edit), the keystroke gate, and parse / format.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum PropertyKind {
    Bool,
    Int,
    Float,
    Text,
}

fn kind_of(value: &PropertyValue) -> PropertyKind {
    match value {
        PropertyValue::Bool(_) => PropertyKind::Bool,
        PropertyValue::Int(_) => PropertyKind::Int,
        PropertyValue::Float(_) => PropertyKind::Float,
        PropertyValue::Text(_) => PropertyKind::Text,
    }
}

fn kind_name(kind: PropertyKind) -> &'static str {
    match kind {
        PropertyKind::Bool => "bool",
        PropertyKind::Int => "int",
        PropertyKind::Float => "float",
        PropertyKind::Text => "text",
    }
}

/// f64 → canonical text (`12.5`, `-4`, `1`). Used for both the cell display
/// and the inline-editor seed so the parse round-trips it.
fn format_float(value: f64) -> String {
    format!("{value}")
}

/// The value shown in a non-edited value cell. Bools render as a checkbox
/// glyph (not this text), but the AT name path reuses the `On` / `Off`
/// wording so the spoken value reads naturally.
fn display_value(value: &PropertyValue) -> String {
    match value {
        PropertyValue::Bool(b) => if *b { "On" } else { "Off" }.to_string(),
        PropertyValue::Int(i) => i.to_string(),
        PropertyValue::Float(f) => format_float(*f),
        PropertyValue::Text(s) => s.clone(),
    }
}

/// The text the inline editor is seeded with when a row enters edit mode.
fn format_for_edit(value: &PropertyValue) -> String {
    match value {
        PropertyValue::Bool(b) => b.to_string(),
        PropertyValue::Int(i) => i.to_string(),
        PropertyValue::Float(f) => format_float(*f),
        PropertyValue::Text(s) => s.clone(),
    }
}

/// Parse the committed editor text back into a typed value. `None` on a
/// malformed numeric commit — the caller keeps the prior value (no data
/// loss). Bool never reaches here (bools toggle, they are not text-edited).
fn parse_for_kind(kind: PropertyKind, text: &str) -> Option<PropertyValue> {
    let trimmed = text.trim();
    match kind {
        PropertyKind::Bool => None,
        PropertyKind::Int => trimmed.parse::<i64>().ok().map(PropertyValue::Int),
        PropertyKind::Float => trimmed.parse::<f64>().ok().map(PropertyValue::Float),
        PropertyKind::Text => Some(PropertyValue::Text(trimmed.to_owned())),
    }
}

fn value_to_introspect(value: &PropertyValue) -> IntrospectValue {
    match value {
        PropertyValue::Bool(b) => IntrospectValue::Bool(*b),
        PropertyValue::Int(i) => IntrospectValue::Int(*i),
        PropertyValue::Float(f) => IntrospectValue::Float(*f),
        PropertyValue::Text(s) => IntrospectValue::Text(s.clone()),
    }
}

/// Validate an `intervene "value.<i>"` payload against the row's kind — the
/// AI-first programmatic set path. Strict per kind (no silent coercion) so
/// an RPC writes exactly the type the row holds.
fn coerce_intervene(
    kind: PropertyKind,
    value: IntrospectValue,
) -> Result<PropertyValue, InterveneError> {
    match (kind, value) {
        (PropertyKind::Bool, IntrospectValue::Bool(b)) => Ok(PropertyValue::Bool(b)),
        (PropertyKind::Int, IntrospectValue::Int(i)) => Ok(PropertyValue::Int(i)),
        (PropertyKind::Float, IntrospectValue::Float(f)) => Ok(PropertyValue::Float(f)),
        (PropertyKind::Text, IntrospectValue::Text(s)) => Ok(PropertyValue::Text(s)),
        _ => Err(InterveneError::TypeMismatch),
    }
}

/// First-paint property values. A game-object inspector — the kinds the
/// self-hosted editor's Details panel needs (name / tag text, visibility /
/// lock flags, layer / health ints, transform / opacity floats).
fn default_properties() -> Vec<PropertyValue> {
    vec![
        PropertyValue::Text("Player".to_owned()),
        PropertyValue::Text("hero".to_owned()),
        PropertyValue::Bool(true),
        PropertyValue::Bool(false),
        PropertyValue::Int(3),
        PropertyValue::Int(100),
        PropertyValue::Float(12.5),
        PropertyValue::Float(-4.0),
        PropertyValue::Float(1.0),
    ]
}

/// Whether `key` is a single keystroke allowed into an `int` row (digit or
/// sign). Multi-codepoint named keys are not keystrokes.
fn is_int_char(key: &str) -> bool {
    single_char(key, |c| c.is_ascii_digit() || c == '-')
}

/// Whether `key` is a single keystroke allowed into a `float` row (digit,
/// sign, or decimal point).
fn is_float_char(key: &str) -> bool {
    single_char(key, |c| c.is_ascii_digit() || c == '-' || c == '.')
}

fn single_char(key: &str, pred: impl Fn(char) -> bool) -> bool {
    let mut chars = key.chars();
    let Some(c) = chars.next() else {
        return false;
    };
    if chars.next().is_some() {
        return false;
    }
    pred(c)
}

// ─── reactive holders (Owner::cache, shared view ↔ coordinator) ────

/// Typed value SSOT. A `Signal` so a value change (keyboard commit, RPC
/// intervene, bool toggle) re-runs the subscribed view fn.
#[must_use]
fn use_property_model() -> Rc<Signal<Vec<PropertyValue>>> {
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

// ─── grid coordinator External ────────────────────────────────────

/// The property-grid coordinator. Holds `Rc` clones of the reactive holders
/// (resolved through the `use_*` hooks at construction, so they are the same
/// instances the view fn reads) + the shared editor's [`TextEditState`] so
/// [`Self::begin_edit`] can seed it. Mutations write the Signals directly —
/// no hooks at invoke time, so the External is self-contained on any thread
/// (the todomvc `TodoEditExternal` shape).
struct PropertyGridExternal {
    model: Rc<Signal<Vec<PropertyValue>>>,
    focused_row: Rc<Signal<usize>>,
    editing_row: Rc<Signal<Option<usize>>>,
    editor: Rc<TextEditState>,
}

impl PropertyGridExternal {
    fn new(
        model: Rc<Signal<Vec<PropertyValue>>>,
        focused_row: Rc<Signal<usize>>,
        editing_row: Rc<Signal<Option<usize>>>,
        editor: Rc<TextEditState>,
    ) -> Self {
        Self { model, focused_row, editing_row, editor }
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
            if let Some(PropertyValue::Bool(b)) = next.get_mut(row) {
                *b = !*b;
                toggled = true;
            }
            next
        });
        toggled
    }

    /// Enter edit mode on `row`: latch `editing_row`, seed the shared editor
    /// with the formatted value (caret parked at the trailing edge), and
    /// request focus into the field (drained before the next paint). Returns
    /// `false` for a bool row (bools toggle, they are not text-edited) or an
    /// out-of-range index.
    fn begin_edit(&self, row: usize) -> bool {
        let model = self.model.get();
        let Some(value) = model.get(row) else {
            return false;
        };
        if kind_of(value) == PropertyKind::Bool {
            return false;
        }
        let text = format_for_edit(value);
        let len = text.len();
        self.editing_row.set(Some(row));
        self.editor.set_text(text);
        self.editor.set_caret(len);
        pinion_core::focus_request::request(EDIT_TF_TAG);
        true
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
            ("send", "string"),
            ("toggle", "json"),
            ("begin", "int"),
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
                    return Some(IntrospectValue::Text(kind_name(kind_of(value)).to_owned()));
                }
                if let Some(idx_str) = path.strip_prefix("value.") {
                    let idx: usize = idx_str.parse().ok()?;
                    let model = self.model.get();
                    let value = model.get(idx)?;
                    return Some(value_to_introspect(value));
                }
                None
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "row_count" | "editing" => Err(InterveneError::ReadOnly),
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
                let kind = kind_of(&self.model.get()[idx]);
                let new_value = coerce_intervene(kind, value)?;
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
            // Composite wire `"<row>:<EventName>"`. PointerUp focuses the row
            // (and toggles it when it is a bool — the single-click checkbox
            // affordance); DoubleClick enters edit mode on an editable row.
            // Other pointer phases are accepted as no-ops.
            "send" => match args {
                IntrospectValue::Text(ref s) => {
                    let (idx, event_name, _) =
                        parse_send_payload(s).ok_or(InvokeError::Rejected)?;
                    if idx >= self.count() {
                        return Err(InvokeError::Rejected);
                    }
                    match event_name {
                        "PointerUp" => {
                            self.focused_row.set(idx);
                            let is_bool = matches!(
                                self.model.get().get(idx),
                                Some(PropertyValue::Bool(_))
                            );
                            if is_bool {
                                self.toggle(idx);
                            }
                            Ok(IntrospectValue::Int(
                                i64::try_from(idx).expect("row index fits in i64"),
                            ))
                        }
                        "DoubleClick" => Ok(IntrospectValue::Bool(self.begin_edit(idx))),
                        _ => Ok(IntrospectValue::Null),
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Toggle the focused bool (the `Space` keyboard path + the RPC
            // affordance). No-op on a non-bool focused row.
            "toggle" => Ok(IntrospectValue::Bool(self.toggle(self.focused_row.get()))),
            // Enter edit mode on a given row (the `Enter` / `F2` keyboard
            // path + the RPC edit-entry affordance).
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
        if let Some(parsed) = parse_for_kind(kind_of(value), &text) {
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
/// the next edit starts from a fresh seed; restore grid focus on request.
fn end_edit_mode(restore_focus: bool) {
    use_editing_row().set(None);
    use_text_edit_state(EDIT_TF_TAG).set_text(String::new());
    if restore_focus {
        pinion_core::focus_request::request(GRID_TAG);
    }
}

/// The kind of the row currently being edited (`None` when not editing) —
/// drives the int / float keystroke gate.
fn editing_kind() -> Option<PropertyKind> {
    let row = use_editing_row().get()?;
    use_property_model().get().get(row).map(kind_of)
}

// ─── keyboard ─────────────────────────────────────────────────────

/// Grid-focused keymap: roving navigation + activate. Navigation writes the
/// `focused_row` Signal directly (pure cursor motion, no cross-external
/// effect); `Space` / `Enter` / `F2` route through the coordinator's
/// `invoke` so the keyboard path is identical to the RPC path.
fn apply_key_grid(scene: &mut Scene, key: &str) -> bool {
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

/// Activate the focused row: toggle a bool, or (when `allow_edit`) enter
/// edit mode on a text / int / float row. Routes through the coordinator's
/// `invoke` so toggle / begin live in one place (the RPC path).
fn activate_focused(scene: &mut Scene, row: usize, allow_edit: bool) -> bool {
    let kind = match use_property_model().get().get(row) {
        Some(value) => kind_of(value),
        None => return false,
    };
    let Some(node) = scene.find_external_with_tag_mut(GRID_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    if kind == PropertyKind::Bool {
        intro.invoke("toggle", IntrospectValue::Null).is_ok()
    } else if allow_edit {
        let arg = IntrospectValue::Int(i64::try_from(row).expect("row index fits in i64"));
        intro.invoke("begin", arg).is_ok()
    } else {
        false
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
            let allowed = match editing_kind() {
                Some(PropertyKind::Int) => is_int_char(other),
                Some(PropertyKind::Float) => is_float_char(other),
                Some(PropertyKind::Text) => true,
                _ => false,
            };
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

/// A non-interactive checkbox affordance for a bool value cell (the
/// coordinator owns the toggle; this is paint only, no per-cell external).
fn checkbox_visual(checked: bool, theme: &Theme) -> Scene {
    let mark = if checked {
        vec![Scene::Text(TextNode::styled(
            CHECK_GLYPH,
            Rect::default(),
            TextStyle::new()
                .with_size_px(16)
                .with_fg(theme.resolve(ColorRole::OnAccent)),
        ))]
    } else {
        Vec::new()
    };
    let fill = if checked {
        theme.resolve(ColorRole::Accent)
    } else {
        theme.resolve(ColorRole::SurfaceContainerHighest)
    };
    Scene::Container(
        ContainerNode::new(mark)
            .with_style(
                BoxStyle::filled(fill)
                    .with_corner_radius(4)
                    .with_border(Border::new(theme.resolve(ColorRole::Outline), 1)),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(CHECKBOX_SIZE, CHECKBOX_SIZE)),
            ),
    )
}

/// One property row: `[ name cell | value cell ]`, tagged `property_grid#<i>`
/// so a click routes to the coordinator. The value cell paints the shared
/// inline field while editing, else a checkbox glyph (bool) or the value
/// text.
fn view_row(
    index: usize,
    value: &PropertyValue,
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
            PropertyValue::Bool(b) => checkbox_visual(*b, theme),
            other => Scene::Text(TextNode::styled(
                display_value(other),
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

    let title = Scene::Text(TextNode::styled(
        "Inspector",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let mut rows: Vec<Scene> = Vec::with_capacity(ROW_COUNT + 1);
    rows.push(view_header(&theme));
    for (index, value) in model.iter().enumerate() {
        let edit_active = editing == Some(index) && kind_of(value) != PropertyKind::Bool;
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

    Scene::Container(
        ContainerNode::new(vec![title, grid])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_padding(Rect::new(PANEL_PAD, PANEL_PAD, PANEL_PAD, PANEL_PAD))
                    .with_gap(ROW_GAP * 6)
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
        Box::new(PropertyGridExternal::new(model, focused, editing, editor))
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
    /// commits.
    fn update(_state: RootState, intent: &pinion_core::Intent) -> Vec<Command> {
        if intent.tag_str() == EDIT_TF_BLUR_INTENT_TAG && use_editing_row().get().is_some() {
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
    /// over one data row per property; the focused row's value cell carries
    /// the roving `focused` flag (`aria-activedescendant`). The typed value
    /// is encoded in the cell name so AT hears the value with its context.
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
                        name: format!("{}: {}", PROPERTY_NAMES[index], display_value(value)),
                        focused: index == focused,
                    },
                ],
            })
            .collect();
        grid_table_nodes(GRID_TAG, "Inspector", false, "pg_header", &columns, &rows)
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

    // ----- pure helpers -----

    #[test]
    fn r836_kind_of_classifies_every_variant() {
        assert_eq!(kind_of(&PropertyValue::Bool(true)), PropertyKind::Bool);
        assert_eq!(kind_of(&PropertyValue::Int(1)), PropertyKind::Int);
        assert_eq!(kind_of(&PropertyValue::Float(1.0)), PropertyKind::Float);
        assert_eq!(kind_of(&PropertyValue::Text(String::new())), PropertyKind::Text);
    }

    #[test]
    fn r836_kind_name_round_trips_the_wire_vocab() {
        assert_eq!(kind_name(PropertyKind::Bool), "bool");
        assert_eq!(kind_name(PropertyKind::Int), "int");
        assert_eq!(kind_name(PropertyKind::Float), "float");
        assert_eq!(kind_name(PropertyKind::Text), "text");
    }

    #[test]
    fn r836_parse_for_kind_int_and_float_and_text() {
        assert_eq!(parse_for_kind(PropertyKind::Int, " 42 "), Some(PropertyValue::Int(42)));
        assert_eq!(parse_for_kind(PropertyKind::Int, "x"), None, "garbage int -> None");
        assert_eq!(parse_for_kind(PropertyKind::Float, "-3.5"), Some(PropertyValue::Float(-3.5)));
        assert_eq!(parse_for_kind(PropertyKind::Float, "."), None, "lone dot -> None");
        assert_eq!(
            parse_for_kind(PropertyKind::Text, "  hi "),
            Some(PropertyValue::Text("hi".to_owned())),
            "text trims",
        );
        assert_eq!(parse_for_kind(PropertyKind::Bool, "true"), None, "bool never text-parsed");
    }

    #[test]
    fn r836_display_value_renders_each_kind() {
        assert_eq!(display_value(&PropertyValue::Bool(true)), "On");
        assert_eq!(display_value(&PropertyValue::Bool(false)), "Off");
        assert_eq!(display_value(&PropertyValue::Int(7)), "7");
        assert_eq!(display_value(&PropertyValue::Float(12.5)), "12.5");
        assert_eq!(display_value(&PropertyValue::Text("hi".to_owned())), "hi");
    }

    #[test]
    fn r836_numeric_gates() {
        assert!(is_int_char("3"));
        assert!(is_int_char("-"));
        assert!(!is_int_char("."), "int rejects the decimal point");
        assert!(!is_int_char("a"));
        assert!(is_float_char("."), "float accepts the decimal point");
        assert!(is_float_char("0"));
        assert!(!is_float_char("a"));
        assert!(!is_int_char("Enter"), "named key rejected");
    }

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
            assert_eq!(intro.query("row_count"), Some(IntrospectValue::Int(9)));
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
            assert_eq!(intro.query("focused_row"), Some(IntrospectValue::Int(8)), "clamped to last");
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
