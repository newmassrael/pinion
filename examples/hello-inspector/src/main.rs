//! `hello-inspector` — R909/R910/R922 §5.38 §5.22 §5.40 multi-object inspector.
//!
//! ## What this demonstrates
//!
//! The editor "Details" panel that Unreal, Unity (Inspector), and Qt
//! (`QtPropertyBrowser`) are built around — extended (R922) to its
//! **multi-object** core: select several scene objects at once and the
//! panel shows the properties **common** to every selected object, a
//! "Multiple Values" placeholder wherever the selected objects disagree,
//! and an edit that writes the new value into **all** of them at once.
//!
//! - An object list (left) of scene entities — Player / Camera / Light —
//!   each carrying a typed [`CellValue`] property schema that *shares a
//!   common base* (`Visible` / `Layer` / `Locked`, the actor-level
//!   properties) plus its own type-specific tail. The list is a
//!   **multi-select** WAI-ARIA `listbox`.
//! - A Details panel (right) that reactively reflects the selection: a
//!   header (the selection summary — one object's name, or "N objects
//!   selected"), then one row per property **common to all selected
//!   objects**. A common property whose value is identical across the
//!   selection shows that value; one that differs shows **"Multiple
//!   Values"**.
//! - Selection + editing over RPC (the §2 #2 AI-first primary path):
//!   `invoke select/toggle/extend_to/select_all/clear` drive the
//!   multi-select model; `intervene value.<i>` edits common property `i`
//!   on **every** selected object at once (the [`CellValue::with_intervene`]
//!   typed write). `query mixed.<i>` reports whether the selected objects
//!   disagree on that property.
//!
//! ## Substrate reuse (no hand-rolled equivalents)
//!
//! - The selection is the pure [`VirtualSelect`] index-set model (R780),
//!   embedded directly as a shared reactive holder — flat, stable object
//!   indices, so `Shift`-range / `Ctrl`-toggle / `Ctrl+A` come from the
//!   model, not a re-implementation. (A collapse/expand tree, whose flat
//!   index shifts, needs a stable-id model instead — R902's `TreeSelect`.)
//! - Modifier-aware click selection decodes through [`SelectionChord`];
//!   keyboard navigation through [`clamp_nav`] + [`MultiSelectKeyOp`]; the
//!   composite pointer wire through [`split_send_payload`]. Keyboard,
//!   pointer, and RPC all converge on one select / toggle / extend funnel.
//! - "Do the selected objects agree on this value?" is
//!   [`CellValue::value_eq`] — the NaN-safe total-order equality (the 3rd
//!   consumer after the node-editor no-op guard and the property grid's
//!   modified indicator), never the derived IEEE `PartialEq`.
//! - The `"selection"` / `"selected"` wire reuses the canonical
//!   [`selection_to_value`] / [`selected_to_value`] encode + [`read_selection`]
//!   / [`read_selected`] decode SSOTs.
//! - The object list a11y is the multi-select [`listbox_option_nodes`]
//!   (`aria-multiselectable`, per-option `aria-selected` + the active
//!   descendant as `focused`).
//!
//! ## Scope
//!
//! The §2 AI-first vertical slice — multi-select (RPC + keyboard +
//! modifier-click), common-property derivation, mixed-value reporting, and
//! the write-all edit — is complete and entirely RPC-driven. Inline
//! click-to-edit cell delegates (the property grid's popup / scrub
//! richness) remain the documented GUI follow-up; the property *value* is
//! edited through `intervene value.<i>` (the §2 primary path).
//!
//! ## Verification
//!
//! `tools/demos/r909_inspector.py` drives single-select + editing (the
//! cardinality-1 degenerate case), `tools/demos/r910_inspector_interaction.py`
//! the pointer/keyboard navigation, and `tools/demos/r922_inspector_multi.py`
//! the multi-object core (multi-select, common-property panel, "Multiple
//! Values", write-all). All scene-as-data, deterministic
//! ([[ai-first-rpc-introspection-obligation]],
//! [[introspection-from-paint-not-screen]]).

use std::collections::BTreeSet;
use std::rc::Rc;

use pinion_core::cell_value::CellValue;
use pinion_core::composite_tag::split_send_payload;
use pinion_core::external::query_proxy_external_impl;
use pinion_core::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
};
use pinion_core::input::{is_activation_event, Modifiers, MultiSelectKeyOp, SelectionChord};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, FontWeight, JustifyContent, LayoutStyle,
    Size, TextStyle,
};
use pinion_core::widgets::virtual_select::{
    clamp_nav, read_selected, read_selection, selected_to_value, selection_to_value, SelectionMode,
    VirtualSelect,
};
use pinion_core::{ColorRole, Frame, Owner, Scene, Signal, use_theme};
use pinion_a11y::{listbox_option_nodes, AccessNode, AriaRole, ListOption, WidgetA11y};
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

// pinion-forge codegen output: `pub struct HelloInspectorRenderer`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so `AppShell<V>` can drive it.
vello_renderer_impl!(HelloInspectorRenderer, HelloInspectorRendererError);

const WIN_W: u32 = 460;
const WIN_H: u32 = 420;

const INSPECTOR_TAG: &str = "inspector";
/// The object-list container tag — the `aria-multiselectable` `listbox`.
const OBJECTS_TAG: &str = "inspector_objects";
const THEME_TAG: &str = "app";

const TITLE_FONT_PX: u32 = 16;
const HEADER_FONT_PX: u32 = 15;
const ROW_FONT_PX: u32 = 13;

const LIST_W: u32 = 150;
const ROW_H: u32 = 30;
const SWATCH: u32 = 16;

// ─── Model ────────────────────────────────────────────────────────

/// One typed property: a display name plus its [`CellValue`].
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Property {
    name: String,
    value: CellValue,
}

impl Property {
    fn new(name: &str, value: CellValue) -> Self {
        Self { name: name.to_owned(), value }
    }
}

/// A selectable scene object with its own typed property schema.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct ObjectData {
    name: String,
    properties: Vec<Property>,
}

impl ObjectData {
    fn new(name: &str, properties: Vec<Property>) -> Self {
        Self { name: name.to_owned(), properties }
    }
}

/// Three scene objects. Each carries the **common actor base** — `Visible`
/// (Bool), `Layer` (Int), `Locked` (Bool) — that the multi-object panel
/// edits across a selection, plus its own type-specific tail. The base
/// values are deliberately a mix so a multi-selection surfaces both uniform
/// rows (Player + Camera both `Layer 1`) and "Multiple Values" rows (all
/// three: `Layer` is `1, 1, 2`).
fn default_objects() -> Vec<ObjectData> {
    vec![
        ObjectData::new(
            "Player",
            vec![
                Property::new("Visible", CellValue::Bool(true)),
                Property::new("Layer", CellValue::Int(1)),
                Property::new("Locked", CellValue::Bool(false)),
                Property::new("Health", CellValue::Int(100)),
                Property::new("Speed", CellValue::Float(6.5)),
                Property::new(
                    "Team",
                    CellValue::Choice {
                        selected: 0,
                        options: vec!["Red".to_owned(), "Blue".to_owned(), "Neutral".to_owned()],
                    },
                ),
                Property::new("Tint", CellValue::Color(Color::rgb(0x4f, 0x9d, 0xff))),
            ],
        ),
        ObjectData::new(
            "Main Camera",
            vec![
                Property::new("Visible", CellValue::Bool(true)),
                Property::new("Layer", CellValue::Int(1)),
                Property::new("Locked", CellValue::Bool(false)),
                Property::new("Field of View", CellValue::Float(60.0)),
                Property::new(
                    "Projection",
                    CellValue::Choice {
                        selected: 0,
                        options: vec!["Perspective".to_owned(), "Orthographic".to_owned()],
                    },
                ),
            ],
        ),
        ObjectData::new(
            "Sun Light",
            vec![
                Property::new("Visible", CellValue::Bool(false)),
                Property::new("Layer", CellValue::Int(2)),
                Property::new("Locked", CellValue::Bool(true)),
                Property::new("Intensity", CellValue::Float(1.2)),
                Property::new("Color", CellValue::Color(Color::rgb(0xff, 0xe0, 0x9a))),
                Property::new("Cast Shadows", CellValue::Bool(false)),
            ],
        ),
    ]
}

/// The shared object model — the same `Rc<Signal>` the
/// [`InspectorExternal`] mutates and the view reads ([[reactive-holder-for-shared-external-view-state]]).
fn use_objects() -> Rc<Signal<Vec<ObjectData>>> {
    let owner = Owner::current().expect("use_objects requires an active Owner scope");
    owner.cache("inspector.objects", || Signal::new(default_objects()))
}

/// The shared multi-select model over the object list (R922): the pure
/// [`VirtualSelect`] index-set, persisted across rebuilds like the object
/// model. Constructed in `Multi` mode so `Shift`-range / `Ctrl`-toggle /
/// `Ctrl+A` all hold an arbitrary set with an anchor.
fn use_selection() -> Rc<Signal<VirtualSelect>> {
    let owner = Owner::current().expect("use_selection requires an active Owner scope");
    owner.cache("inspector.selection", || {
        let mut model = VirtualSelect::new(default_objects().len(), SelectionMode::Multi);
        // Boot with the first object selected (the pre-R922 default selection,
        // so the single-object demos keep their cardinality-1 contract).
        model.select(0);
        Signal::new(model)
    })
}

// ─── Common-property derivation (the SSOT shared by query / paint / a11y) ──

/// One row of the multi-object Details panel: a property common to every
/// selected object, plus whether the selection agrees on its value.
#[derive(Clone, Debug, PartialEq)]
struct CommonProperty {
    name: String,
    /// The representative value — the first selected object's value for this
    /// property (the order source). Concrete even when [`mixed`](Self::mixed),
    /// so an AI reading `value.<i>` always sees a typed value.
    value: CellValue,
    /// `true` when the selected objects disagree on this property's value (the
    /// Details "Multiple Values" state), tested with the NaN-safe
    /// [`CellValue::value_eq`].
    mixed: bool,
}

/// Whether two property values are the **same property** for multi-object
/// editing: the same [`CellValue` kind](pinion_core::cell_value::CellKind),
/// and for a [`CellValue::Choice`] the same option list (only the selected
/// index — the *value* — may differ). A choice over `[Red, Blue]` and one over
/// `[Red, Blue, Green]` are NOT the same property: a write-all of "option 2"
/// is in range for one and out of range for the other, so grouping them would
/// silently half-apply the edit ([`with_intervene`](CellValue::with_intervene)
/// bounds a `Choice` index to its own `options.len()`). For every scalar kind
/// `with_intervene` is value-shape-independent, so kind equality suffices.
fn same_property_shape(a: &CellValue, b: &CellValue) -> bool {
    match (a, b) {
        (CellValue::Choice { options: oa, .. }, CellValue::Choice { options: ob, .. }) => oa == ob,
        _ => a.kind() == b.kind(),
    }
}

/// The properties common to **every** selected object, in the order they
/// appear on the lowest-indexed selected object (a stable order: it does not
/// shift when the cursor moves within a fixed selection). A property is
/// "common" when every selected object has one with the same name and the
/// same [`same_property_shape`] (kind, plus a `Choice`'s option list). The
/// free-fn SSOT that the RPC query, the paint, and the a11y all derive their
/// rows from — one gate, no drift (R886.1). An empty selection yields no rows.
///
/// Assumes property names are unique within an object (the schema invariant of
/// every Details model); a duplicate name resolves to the first match.
///
/// For a single selection this is exactly that object's full property list
/// (every value trivially uniform), so the cardinality-1 case is the
/// pre-R922 fixed Details panel.
fn common_properties(objects: &[ObjectData], selection: &BTreeSet<usize>) -> Vec<CommonProperty> {
    let mut indices = selection.iter().copied().filter(|&i| i < objects.len());
    let Some(first) = indices.next() else {
        return Vec::new();
    };
    let others: Vec<usize> = indices.collect();
    let mut rows = Vec::new();
    for prop in &objects[first].properties {
        // Every other selected object must carry a same-name, same-shape
        // property for this to be common; collect their values to test mix.
        let mut values = Vec::with_capacity(others.len());
        let mut all_have = true;
        for &j in &others {
            if let Some(p) = objects[j]
                .properties
                .iter()
                .find(|p| p.name == prop.name && same_property_shape(&p.value, &prop.value))
            {
                values.push(&p.value);
            } else {
                all_have = false;
                break;
            }
        }
        if !all_have {
            continue;
        }
        let mixed = values.iter().any(|v| !v.value_eq(&prop.value));
        rows.push(CommonProperty { name: prop.name.clone(), value: prop.value.clone(), mixed });
    }
    rows
}

/// The Details header text for a selection: the lone object's name when one
/// is selected, "N objects selected" for several, "No selection" for none.
fn selection_summary(objects: &[ObjectData], selection: &BTreeSet<usize>) -> String {
    let live: Vec<usize> = selection.iter().copied().filter(|&i| i < objects.len()).collect();
    match live.as_slice() {
        [] => "No selection".to_owned(),
        [only] => objects[*only].name.clone(),
        many => format!("{} objects selected", many.len()),
    }
}

// ─── External (the §5.15 AI surface) ──────────────────────────────

/// The inspector coordinator: owns the object model + a [`VirtualSelect`]
/// multi-selection, and exposes the selection wire (mirroring
/// `VirtualSelectExternal`) plus selection-relative common-property
/// query / intervene. The `value.<i>` addressing resolves against the
/// derived common-property list, and an edit writes through to every
/// selected object — the Unreal multi-object Details core.
struct InspectorExternal {
    objects: Rc<Signal<Vec<ObjectData>>>,
    selection: Rc<Signal<VirtualSelect>>,
}

impl InspectorExternal {
    fn new(objects: Rc<Signal<Vec<ObjectData>>>, selection: Rc<Signal<VirtualSelect>>) -> Self {
        Self { objects, selection }
    }

    fn object_count(&self) -> usize {
        self.objects.get().len()
    }

    /// The selected object indices.
    fn selection_set(&self) -> BTreeSet<usize> {
        self.selection.get().selection().clone()
    }

    /// The active (cursor) object index, the `"selected"` value.
    fn cursor(&self) -> Option<usize> {
        self.selection.get().cursor()
    }

    /// The Details rows for the current selection (the [`common_properties`]
    /// SSOT over the live object + selection state).
    fn common(&self) -> Vec<CommonProperty> {
        common_properties(&self.objects.get(), &self.selection_set())
    }

    // ─── selection funnel (keyboard / pointer / RPC all converge here) ──

    fn select(&self, index: usize) {
        self.selection.set_with(|prev| {
            let mut next = prev.clone();
            next.select(index);
            next
        });
    }

    fn toggle(&self, index: usize) {
        self.selection.set_with(|prev| {
            let mut next = prev.clone();
            next.toggle(index);
            next
        });
    }

    fn extend_to(&self, index: usize) {
        self.selection.set_with(|prev| {
            let mut next = prev.clone();
            next.extend_to(index);
            next
        });
    }

    fn select_all(&self) {
        self.selection.set_with(|prev| {
            let mut next = prev.clone();
            next.select_all();
            next
        });
    }

    fn clear(&self) {
        self.selection.set_with(|prev| {
            let mut next = prev.clone();
            next.clear();
            next
        });
    }

    fn set_selection(&self, indices: BTreeSet<usize>) {
        self.selection.set_with(move |prev| {
            let mut next = prev.clone();
            next.set_selection(&indices);
            next
        });
    }

    /// The R902.1 modifier-aware composite click wire: `inspector#<i>` routes
    /// the pointer arc here as `"<i>:<Event>[:<mods>]"`. On the activation
    /// edge the held modifiers decode through [`SelectionChord`] — `Ctrl`/`Cmd`
    /// toggles, `Shift` extends the range from the anchor, a plain click
    /// replaces — exactly the keyboard ops, one funnel.
    ///
    /// This body mirrors `VirtualSelectExternal::handle_send`, but is NOT a
    /// liftable duplicate: the chord DECISION is already the shared
    /// [`SelectionChord`] / [`is_activation_event`] SSOT; what differs is the
    /// key parse (the wrapper also decodes grid `r:c` keys) and the mutate tail
    /// (the wrapper pushes a §5.20 intent through its `IntentEmitter`; this
    /// embedder writes its shared `Signal`). A scroll-free dispatch core taking
    /// `&mut VirtualSelect` would serve neither cleanly — it lifts only when a
    /// 3rd bare-model embedder makes the mutate tail uniform (R922.1 carry; the
    /// keyboard `apply_key` is the peer fork of `nav_select_key`).
    fn handle_send(&self, payload: &str) {
        let Some((key, event_name, modifiers)) = split_send_payload(payload) else {
            return;
        };
        let Some(index) = key.parse::<usize>().ok() else {
            return;
        };
        if is_activation_event(event_name) {
            match SelectionChord::from_modifiers(modifiers) {
                SelectionChord::Toggle => self.toggle(index),
                SelectionChord::Extend => self.extend_to(index),
                SelectionChord::Replace => self.select(index),
            }
        }
    }

    /// Typed write into common property `idx` across **every** selected
    /// object (the multi-object edit). Validates the wire value once against
    /// the representative (surfacing a `TypeMismatch` without mutating), then
    /// applies [`CellValue::with_intervene`] per object so a choice sets its
    /// own option index and a colour parses the hex against its own value.
    fn set_property(&self, idx: usize, value: &IntrospectValue) -> Result<(), InterveneError> {
        let common = self.common();
        let target = common.get(idx).ok_or(InterveneError::UnknownPath)?;
        // Validate against the representative once. Every selected object's
        // matching property has the SAME shape (`same_property_shape` — kind
        // plus a Choice's option list), so this accept/reject is the verdict
        // for the whole write: a value the representative accepts cannot be
        // rejected by a same-shape property.
        target.value.with_intervene(value.clone())?;
        let name = target.name.clone();
        let shape = target.value.clone();
        let selection = self.selection_set();
        let value = value.clone();
        self.objects.set_with(move |prev| {
            let mut next = prev.clone();
            for &j in &selection {
                if let Some(prop) = next.get_mut(j).and_then(|o| {
                    o.properties.iter_mut().find(|p| p.name == name && same_property_shape(&p.value, &shape))
                }) {
                    match prop.value.with_intervene(value.clone()) {
                        Ok(updated) => prop.value = updated,
                        // Unreachable: same shape as the representative, which
                        // accepted `value`. Fail loud in dev, preserve the
                        // existing value in release (R906 — no silent fallback
                        // on a should-be-impossible branch).
                        Err(e) => debug_assert!(
                            false,
                            "multi-write: a value valid for the representative was rejected by a same-shape property: {e:?}"
                        ),
                    }
                }
            }
            next
        });
        Ok(())
    }
}

impl core::fmt::Debug for InspectorExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InspectorExternal")
            .field("object_count", &self.object_count())
            .field("selection", &self.selection_set())
            .field("cursor", &self.cursor())
            .finish()
    }
}

// R913.1 — the Gui+Rpc / Framework / UiThreadSync `impl External` skeleton is
// the `query_proxy_external_impl!` SSOT (a config-holder External whose state
// is a shared reactive holder); hand-rolling it was a
// [[use-substrate-not-hand-rolled-equivalent]] miss.
query_proxy_external_impl!(InspectorExternal);

impl ExternalIntrospect for InspectorExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("object_count", "int"),
            ("selected", "int"),
            ("selection", "json"),
            ("selection_count", "int"),
            ("selection_summary", "string"),
            ("mode", "string"),
            ("row_count", "int"),
            ("object_name.<j>", "string"),
            ("name.<i>", "string"),
            ("kind.<i>", "string"),
            ("value.<i>", "json"),
            ("mixed.<i>", "bool"),
            ("select", "int"),
            ("toggle", "int"),
            ("extend_to", "int"),
            ("select_all", "null"),
            ("clear", "null"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let objects = self.objects.get();
        let selection = self.selection_set();
        match path {
            "object_count" => Some(IntrospectValue::Int(i64::try_from(objects.len()).ok()?)),
            "selected" => Some(selected_to_value(self.cursor())),
            "selection" => Some(selection_to_value(&selection)),
            "selection_count" => Some(IntrospectValue::Int(i64::try_from(selection.len()).ok()?)),
            "selection_summary" => {
                Some(IntrospectValue::Text(selection_summary(&objects, &selection)))
            }
            "mode" => Some(IntrospectValue::Text("multi".to_owned())),
            "row_count" => {
                Some(IntrospectValue::Int(i64::try_from(common_properties(&objects, &selection).len()).ok()?))
            }
            _ => {
                if let Some(j) = path.strip_prefix("object_name.") {
                    let j: usize = j.parse().ok()?;
                    return objects.get(j).map(|o| IntrospectValue::Text(o.name.clone()));
                }
                let common = common_properties(&objects, &selection);
                if let Some(i) = path.strip_prefix("name.") {
                    let i: usize = i.parse().ok()?;
                    return common.get(i).map(|c| IntrospectValue::Text(c.name.clone()));
                }
                if let Some(i) = path.strip_prefix("kind.") {
                    let i: usize = i.parse().ok()?;
                    return common.get(i).map(|c| IntrospectValue::Text(c.value.kind().name().to_owned()));
                }
                if let Some(i) = path.strip_prefix("value.") {
                    let i: usize = i.parse().ok()?;
                    return common.get(i).map(|c| c.value.to_introspect());
                }
                if let Some(i) = path.strip_prefix("mixed.") {
                    let i: usize = i.parse().ok()?;
                    return common.get(i).map(|c| IntrospectValue::Bool(c.mixed));
                }
                None
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "object_count" | "selection_count" | "selection_summary" | "mode" | "row_count" => {
                Err(InterveneError::ReadOnly)
            }
            // Admin / restore: move the cursor + replace the selection with a
            // single row (`Int`) or clear it (`Null`).
            "selected" => match value {
                IntrospectValue::Int(n) => {
                    let idx = usize::try_from(n).map_err(|_| InterveneError::OutOfRange)?;
                    if idx >= self.object_count() {
                        return Err(InterveneError::OutOfRange);
                    }
                    self.select(idx);
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.clear();
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // Admin / restore: replace the whole selection set (out-of-range
            // indices are dropped by the model); `Null` clears.
            "selection" => match value {
                IntrospectValue::Json(serde_json::Value::Array(items)) => {
                    let indices: BTreeSet<usize> = items
                        .iter()
                        .filter_map(serde_json::Value::as_u64)
                        .filter_map(|v| usize::try_from(v).ok())
                        .collect();
                    self.set_selection(indices);
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.set_selection(BTreeSet::new());
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            _ => {
                if path.starts_with("name.")
                    || path.starts_with("kind.")
                    || path.starts_with("mixed.")
                    || path.starts_with("object_name.")
                {
                    return Err(InterveneError::ReadOnly);
                }
                let Some(idx_str) = path.strip_prefix("value.") else {
                    return Err(InterveneError::UnknownPath);
                };
                let idx: usize = idx_str.parse().map_err(|_| InterveneError::UnknownPath)?;
                self.set_property(idx, &value)
            }
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let int_arg = |a: IntrospectValue| match a {
            IntrospectValue::Int(n) => usize::try_from(n).map_err(|_| InvokeError::TypeMismatch),
            _ => Err(InvokeError::TypeMismatch),
        };
        match path {
            "select" => {
                let idx = int_arg(args)?;
                if idx >= self.object_count() {
                    return Err(InvokeError::Rejected);
                }
                self.select(idx);
                Ok(selected_to_value(self.cursor()))
            }
            "toggle" => {
                let idx = int_arg(args)?;
                if idx >= self.object_count() {
                    return Err(InvokeError::Rejected);
                }
                self.toggle(idx);
                Ok(selection_to_value(&self.selection_set()))
            }
            "extend_to" => {
                let idx = int_arg(args)?;
                if idx >= self.object_count() {
                    return Err(InvokeError::Rejected);
                }
                self.extend_to(idx);
                Ok(selection_to_value(&self.selection_set()))
            }
            "select_all" => {
                self.select_all();
                Ok(selection_to_value(&self.selection_set()))
            }
            "clear" => {
                self.clear();
                Ok(selected_to_value(self.cursor()))
            }
            // R910/R902.1 — the composite pointer wire (modifier-aware).
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    self.handle_send(payload);
                    Ok(selection_to_value(&self.selection_set()))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

fn make_inspector_external() -> InspectorExternal {
    InspectorExternal::new(use_objects(), use_selection())
}

// ─── View ─────────────────────────────────────────────────────────

/// The number of scene objects (fixed). The selection bitmap in
/// [`InspectorState`] is sized by it; `default_objects().len()` must match it
/// (asserted in tests).
const N_OBJECTS: usize = 3;

/// The widget state read back from the External's introspect: the selected
/// object indices + the active (cursor) index. `Copy` (the `WidgetCore::State`
/// bound), so the multi-selection is an **absolute-index bitmap** rather than a
/// `Vec` ([[virtualized-multiselect-state-window-independent]] — the
/// `hello-multi-select` `[bool; N]` shape): `selected[i]` is object `i`'s
/// membership, `cursor` the active (WAI-ARIA active-descendant) object. Drives
/// both the painted selection highlight and the a11y `aria-selected` / active
/// descendant.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct InspectorState {
    selected: [bool; N_OBJECTS],
    cursor: Option<usize>,
}

impl InspectorState {
    /// Build the bitmap state from the decoded selection set + cursor.
    fn from_parts(selection: &[usize], cursor: Option<usize>) -> Self {
        let mut selected = [false; N_OBJECTS];
        for &i in selection {
            if let Some(slot) = selected.get_mut(i) {
                *slot = true;
            }
        }
        Self { selected, cursor }
    }

    fn is_selected(&self, index: usize) -> bool {
        self.selected.get(index).copied().unwrap_or(false)
    }

    /// The selected object indices as a set — for the object-count-agnostic
    /// [`common_properties`] / [`selection_summary`] derivations.
    fn selection_set(&self) -> BTreeSet<usize> {
        (0..N_OBJECTS).filter(|&i| self.selected[i]).collect()
    }
}

/// A type-appropriate value visual for the right column of a property row: a
/// swatch for a colour, an On/Off pill for a bool, the display text otherwise.
fn value_visual(value: &CellValue, fg: Color, accent: Color, muted: Color) -> Scene {
    match value {
        CellValue::Color(c) => {
            let swatch = Scene::Container(
                ContainerNode::new(Vec::new())
                    .with_style(BoxStyle::filled(*c).with_corner_radius(3))
                    .with_layout(LayoutStyle::new().with_size(Size::px(SWATCH, SWATCH))),
            );
            let hex = Scene::Text(TextNode::styled(
                c.to_hex(),
                Rect::default(),
                TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(fg),
            ));
            Scene::Container(
                ContainerNode::new(vec![swatch, hex]).with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_gap(6),
                ),
            )
        }
        CellValue::Bool(b) => {
            let fill = if *b { accent } else { muted };
            Scene::Container(
                ContainerNode::new(vec![Scene::Text(TextNode::styled(
                    bool_label(*b),
                    Rect::default(),
                    TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(Color::rgb(0xff, 0xff, 0xff)),
                ))])
                .with_style(BoxStyle::filled(fill).with_corner_radius(8))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(JustifyContent::Center)
                        .with_align_items(AlignItems::Center)
                        .with_size(Size::px(40, 20)),
                ),
            )
        }
        other => Scene::Text(TextNode::styled(
            other.display(),
            Rect::default(),
            TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(fg),
        )),
    }
}

/// The placeholder shown for a property whose selected objects disagree (the
/// Unreal "Multiple Values" mixed state). One SSOT so the painted text and the
/// a11y name announce the same word.
const MULTIPLE_VALUES: &str = "Multiple Values";

/// The On/Off label for a bool value — one SSOT for the painted pill text and
/// the a11y name (they must announce the same word, R886.1 one-gate).
fn bool_label(b: bool) -> &'static str {
    if b { "On" } else { "Off" }
}

/// The text rendering of a common-property value for a11y / introspection: the
/// "Multiple Values" placeholder when mixed, else the typed value the way the
/// paint shows it (the bool pill word, a colour's hex, otherwise `display`).
/// The a11y peer of [`detail_value_visual`]'s paint — one gate on the mixed
/// state so a screen reader and the screen never disagree.
fn common_value_label(prop: &CommonProperty) -> String {
    if prop.mixed {
        return MULTIPLE_VALUES.to_owned();
    }
    match &prop.value {
        CellValue::Bool(b) => bool_label(*b).to_owned(),
        CellValue::Color(c) => c.to_hex(),
        other => other.display(),
    }
}

/// The right-column visual for a common-property row: the typed value when
/// the selection agrees, the "Multiple Values" placeholder when it differs
/// (the Unreal mixed-value state — paint peer of `query mixed.<i>` and of the
/// a11y [`common_value_label`]).
fn detail_value_visual(prop: &CommonProperty, fg: Color, accent: Color, muted: Color) -> Scene {
    if prop.mixed {
        Scene::Text(TextNode::styled(
            MULTIPLE_VALUES,
            Rect::default(),
            TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(muted),
        ))
    } else {
        value_visual(&prop.value, fg, accent, muted)
    }
}

/// One Details row: `name` (left, muted) + value visual (right). Tagged
/// `prop_<i>` so the demo can locate it.
fn property_row(index: usize, prop: &CommonProperty, fg: Color, muted: Color, accent: Color) -> Scene {
    let name = Scene::Text(TextNode::styled(
        prop.name.clone(),
        Rect::default(),
        TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(muted),
    ));
    Scene::Container(
        ContainerNode::new(vec![name, detail_value_visual(prop, fg, accent, muted)])
            .with_tag(format!("prop_{index}"))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::SpaceBetween)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(260, ROW_H)),
            ),
    )
}

/// One object-list entry. A selected row carries an accent background; the
/// active (cursor) row additionally carries a light border — the paint peer
/// of the a11y `aria-selected` (fill) and active descendant (`focused`,
/// border).
fn object_row(index: usize, name: &str, selected: bool, is_cursor: bool, fg: Color, accent: Color) -> Scene {
    let fill = if selected { accent } else { Color::TRANSPARENT };
    let text_fg = if selected { Color::rgb(0xff, 0xff, 0xff) } else { fg };
    let mut style = BoxStyle::filled(fill).with_corner_radius(5);
    if is_cursor {
        style = style.with_border(Border::new(Color::rgb(0xff, 0xff, 0xff), 2));
    }
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            name.to_owned(),
            Rect::default(),
            TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(text_fg),
        ))])
        // Composite tag `inspector#<i>`: the input router's `#` split (R51.42)
        // routes a click here to the primary External's `send` wire as
        // `"<i>:<EventName>[:<mods>]"` — the hello-tabs / radio-group click
        // pattern, now modifier-aware for multi-select.
        .with_tag(format!("{INSPECTOR_TAG}#{index}"))
        .with_style(style)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(LIST_W - 16, ROW_H)),
        ),
    )
}

/// view-fn (§6.3): pure sync. The selection comes from the External's
/// introspect (`read_state`); the object data is read reactively.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &InspectorState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let surface_alt = theme.resolve(ColorRole::SurfaceContainerHighest);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let accent = theme.resolve(ColorRole::Accent);

    let objects = use_objects().get();
    let selection = state.selection_set();

    let title = Scene::Text(TextNode::styled(
        "Scene Inspector",
        Rect::default(),
        TextStyle::new().with_size_px(TITLE_FONT_PX).with_weight(FontWeight::BOLD).with_fg(on_surface),
    ));

    // Left: the multi-select object list.
    let list_rows: Vec<Scene> = objects
        .iter()
        .enumerate()
        .map(|(i, o)| object_row(i, &o.name, state.is_selected(i), state.cursor == Some(i), on_surface, accent))
        .collect();
    let list = Scene::Container(
        ContainerNode::new(list_rows)
            .with_tag(OBJECTS_TAG)
            .with_style(BoxStyle::filled(surface_alt).with_corner_radius(6))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(4)
                    .with_size(Size::px(LIST_W, WIN_H - 70)),
            ),
    );

    // Right: the Details panel for the common properties of the selection.
    let header = Scene::Text(TextNode::styled(
        selection_summary(&objects, &selection),
        Rect::default(),
        TextStyle::new().with_size_px(HEADER_FONT_PX).with_weight(FontWeight::BOLD).with_fg(on_surface),
    ));
    let mut detail_children = vec![header];
    for (i, prop) in common_properties(&objects, &selection).iter().enumerate() {
        detail_children.push(property_row(i, prop, on_surface, muted, accent));
    }
    let detail = Scene::Container(
        ContainerNode::new(detail_children)
            .with_tag("detail_panel")
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(6)
                    .with_size(Size::px(WIN_W - LIST_W - 40, WIN_H - 70)),
            ),
    );

    let panes = Scene::Container(
        ContainerNode::new(vec![list, detail]).with_layout(
            LayoutStyle::new().flex(FlexDirection::Row).with_gap(16).with_align_items(AlignItems::Start),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![title, panes])
            .with_tag(INSPECTOR_TAG)
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(12)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

// ─── Binding ──────────────────────────────────────────────────────

/// `WidgetView` binding. `#[widget]` derives the [`WidgetCore`] / `WidgetView`
/// pair; `a11y_manual` keeps the multi-select listbox a11y below
/// (`aria-multiselectable` is past the macro's single-node derive). The
/// selection lives in [`InspectorExternal`] and is read back through introspect.
///
/// [`WidgetCore`]: pinion_core::WidgetCore
#[widget(
    tag = "inspector",
    state = InspectorState,
    event = (),
    title = "pinion hello-inspector (R922 §5.40 multi-object Details)",
    renderer = HelloInspectorRenderer,
    initial_size = (WIN_W, WIN_H),
    external = make_inspector_external,
    a11y_manual,
    apply_key,
)]
struct InspectorView;

impl InspectorView {
    /// Read the selection (set + cursor) off the primary External's introspect,
    /// reusing the canonical [`read_selection`] / [`read_selected`] decoders.
    fn read_state(scene: &Scene) -> InspectorState {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                return InspectorState::from_parts(&read_selection(intro), read_selected(intro));
            }
        }
        InspectorState::default()
    }

    fn view(state: InspectorState, frame: Frame) -> Scene {
        view(&state, &frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// Keyboard control over the multi-select object list, reusing the policy
    /// substrate (no scroll — the object list is not virtualized, so the full
    /// `nav_select_key` controller does not apply): `Ctrl+A` selects all,
    /// `Ctrl+Space` toggles the active row ([`MultiSelectKeyOp`]); Arrow /
    /// Home / End navigate ([`clamp_nav`]) and, with `Shift`, extend the
    /// range. Every op goes through the External's `select` / `toggle` /
    /// `extend_to` / `select_all` funnel — keyboard and RPC are one path.
    fn apply_key(scene: &mut Scene, _focused: Option<&str>, key: &str, modifiers: Modifiers) -> bool {
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect() else {
            return false;
        };
        let count = read_count(intro);
        let cursor = read_selected(intro);

        match MultiSelectKeyOp::classify(key, modifiers) {
            Some(MultiSelectKeyOp::SelectAll) => {
                if let Some(intro) = node.handle.introspect_mut() {
                    let _ = intro.invoke("select_all", IntrospectValue::Null);
                }
                return true;
            }
            Some(MultiSelectKeyOp::ToggleCursor) => {
                if let (Some(intro), Some(c)) = (node.handle.introspect_mut(), cursor) {
                    if let Ok(c) = i64::try_from(c) {
                        let _ = intro.invoke("toggle", IntrospectValue::Int(c));
                    }
                }
                return cursor.is_some();
            }
            None => {}
        }

        // Plain / Shift navigation. `clamp_nav` maps the key to a target index
        // (no paging here, so page == item_count); Shift extends, else replace.
        let Some(target) = clamp_nav(cursor, key, count, count) else {
            return false;
        };
        let action = if modifiers.shift_key() { "extend_to" } else { "select" };
        if let (Some(intro), Ok(t)) = (node.handle.introspect_mut(), i64::try_from(target)) {
            let _ = intro.invoke(action, IntrospectValue::Int(t));
        }
        true
    }
}

/// The object count off the External's introspect (the `clamp_nav` /
/// activation bound).
fn read_count(intro: &dyn ExternalIntrospect) -> usize {
    match intro.query("object_count") {
        Some(IntrospectValue::Int(n)) => usize::try_from(n).unwrap_or(0),
        _ => 0,
    }
}

/// The Details-panel tag — a WAI-ARIA `list` of the common properties.
const DETAIL_TAG: &str = "detail_panel";

impl WidgetA11y for InspectorView {
    /// The whole inspector a11y tree, all derived from one `InspectorState`:
    ///
    /// - the object list as a multi-select WAI-ARIA `listbox`
    ///   ([`listbox_option_nodes`]): `aria-multiselectable`, each row an
    ///   `option` whose `aria-selected` is its membership and whose `focused`
    ///   (active descendant) is the cursor — the peer of the painted accent
    ///   fill (selected) + border (cursor);
    /// - the **Details panel** as a `list` named by the selection summary,
    ///   one `listitem` per common property named `"{name}: {value}"` — the
    ///   value text is [`common_value_label`], the SAME source the paint
    ///   renders, so "Multiple Values" announces exactly when it paints
    ///   (R886.1 one-gate: the panel content is AT-reachable, not orphaned).
    ///
    /// The root `Group` references both regions.
    fn access_node(state: &InspectorState, _focused: Option<&str>) -> Vec<AccessNode> {
        let objects = use_objects().get();
        let selection = state.selection_set();
        let tags: Vec<String> = (0..objects.len()).map(|i| format!("{INSPECTOR_TAG}#{i}")).collect();
        let options: Vec<ListOption<'_>> = objects
            .iter()
            .enumerate()
            .map(|(i, o)| ListOption {
                tag: &tags[i],
                label: Some(&o.name),
                state: ListboxItemState::Idle,
                selected: state.is_selected(i),
                focused: state.cursor == Some(i),
            })
            .collect();

        let mut nodes = vec![
            AccessNode::new(INSPECTOR_TAG, AriaRole::Group)
                .with_name("Scene Inspector")
                .with_child(OBJECTS_TAG)
                .with_child(DETAIL_TAG),
        ];
        nodes.extend(listbox_option_nodes(OBJECTS_TAG, "Scene objects", true, &options));

        // The Details panel: a `list` named by the selection summary, one
        // `listitem` per common property (tagged `prop_<i>`, the same tag the
        // paint uses, so the AT node resolves to the painted row's bounds).
        let rows = common_properties(&objects, &selection);
        let mut panel = AccessNode::new(DETAIL_TAG, AriaRole::List)
            .with_name(selection_summary(&objects, &selection));
        for i in 0..rows.len() {
            panel = panel.with_child(format!("prop_{i}"));
        }
        nodes.push(panel);
        for (i, prop) in rows.iter().enumerate() {
            nodes.push(
                AccessNode::new(format!("prop_{i}"), AriaRole::ListItem)
                    .with_name(format!("{}: {}", prop.name, common_value_label(prop))),
            );
        }
        nodes
    }
}

fn main() {
    pinion_shell::run::<InspectorView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext() -> InspectorExternal {
        let owner = Owner::new();
        owner.run(make_inspector_external)
    }

    fn json_indices(v: &IntrospectValue) -> Vec<u64> {
        match v {
            IntrospectValue::Json(serde_json::Value::Array(items)) => {
                items.iter().filter_map(serde_json::Value::as_u64).collect()
            }
            _ => panic!("expected a JSON array, got {v:?}"),
        }
    }

    const SHIFT: Modifiers = Modifiers { shift: true, ctrl: false, alt: false, meta: false };
    const CTRL: Modifiers = Modifiers { shift: false, ctrl: true, alt: false, meta: false };

    #[test]
    fn r909_default_selection_is_first_object() {
        let e = ext();
        assert_eq!(e.object_count(), 3);
        assert_eq!(e.cursor(), Some(0));
        assert!(e.query("selected_name").is_none(), "no selected_name slot (use selection_summary)");
        assert_eq!(
            e.query("selection_summary"),
            Some(IntrospectValue::Text("Player".to_owned()))
        );
    }

    #[test]
    fn r909_select_re_targets_the_common_property_set() {
        let mut e = ext();
        // Single selection: the common list IS that object's full schema.
        // Player has 7 props, Main Camera has 5.
        assert_eq!(e.query("row_count"), Some(IntrospectValue::Int(7)));
        e.invoke("select", IntrospectValue::Int(1)).unwrap();
        assert_eq!(e.cursor(), Some(1));
        assert_eq!(e.query("row_count"), Some(IntrospectValue::Int(5)));
        assert_eq!(e.query("selection_summary"), Some(IntrospectValue::Text("Main Camera".to_owned())));
        assert_eq!(e.query("name.0"), Some(IntrospectValue::Text("Visible".to_owned())));
    }

    #[test]
    fn r909_select_out_of_range_is_rejected() {
        let mut e = ext();
        assert!(e.invoke("select", IntrospectValue::Int(9)).is_err());
        assert_eq!(e.cursor(), Some(0), "selection unchanged after a rejected select");
    }

    #[test]
    fn r909_intervene_edits_the_selected_object_only_when_single() {
        let mut e = ext();
        // Single-selected Player: edit Health (index 3) to 42.
        e.intervene("value.3", IntrospectValue::Int(42)).unwrap();
        assert_eq!(e.query("value.3"), Some(IntrospectValue::Int(42)));
        // Camera (object 1) is untouched.
        e.invoke("select", IntrospectValue::Int(1)).unwrap();
        // Camera index 3 is Field of View (60.0), not Health.
        assert_eq!(e.query("value.3"), Some(IntrospectValue::Float(60.0)));
        // Re-select Player: the edit persisted.
        e.invoke("select", IntrospectValue::Int(0)).unwrap();
        assert_eq!(e.query("value.3"), Some(IntrospectValue::Int(42)));
    }

    #[test]
    fn r922_multi_select_panel_is_the_common_properties() {
        let mut e = ext();
        // Select Player + Camera: their common base is Visible / Layer / Locked
        // (the type-specific tails are not shared).
        e.invoke("select", IntrospectValue::Int(0)).unwrap();
        e.invoke("toggle", IntrospectValue::Int(1)).unwrap();
        assert_eq!(json_indices(&e.query("selection").unwrap()), vec![0, 1]);
        assert_eq!(e.query("row_count"), Some(IntrospectValue::Int(3)));
        assert_eq!(e.query("name.0"), Some(IntrospectValue::Text("Visible".to_owned())));
        assert_eq!(e.query("name.1"), Some(IntrospectValue::Text("Layer".to_owned())));
        assert_eq!(e.query("name.2"), Some(IntrospectValue::Text("Locked".to_owned())));
        // Player + Camera agree on every base property.
        assert_eq!(e.query("mixed.0"), Some(IntrospectValue::Bool(false)));
        assert_eq!(e.query("mixed.1"), Some(IntrospectValue::Bool(false)));
        assert_eq!(e.query("value.1"), Some(IntrospectValue::Int(1)), "both Layer 1");
    }

    #[test]
    fn r922_multi_select_mixed_values_reported() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        assert_eq!(json_indices(&e.query("selection").unwrap()), vec![0, 1, 2]);
        assert_eq!(e.query("selection_summary"), Some(IntrospectValue::Text("3 objects selected".to_owned())));
        // All three: Visible (true,true,false) and Layer (1,1,2) differ.
        assert_eq!(e.query("mixed.0"), Some(IntrospectValue::Bool(true)), "Visible mixed");
        assert_eq!(e.query("mixed.1"), Some(IntrospectValue::Bool(true)), "Layer mixed");
    }

    #[test]
    fn r922_edit_writes_every_selected_object() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        // Set the common Layer (index 1) to 5 across all three objects.
        e.intervene("value.1", IntrospectValue::Int(5)).unwrap();
        // Now uniform → not mixed, value 5.
        assert_eq!(e.query("mixed.1"), Some(IntrospectValue::Bool(false)));
        assert_eq!(e.query("value.1"), Some(IntrospectValue::Int(5)));
        // Each object individually carries Layer 5.
        for obj in 0..3 {
            e.invoke("select", IntrospectValue::Int(obj)).unwrap();
            assert_eq!(e.query("value.1"), Some(IntrospectValue::Int(5)), "object {obj} Layer == 5");
        }
    }

    #[test]
    fn r922_shift_extend_and_ctrl_toggle() {
        let mut e = ext();
        // Plain select 0, Shift-extend to 2 → {0,1,2}.
        e.invoke("send", IntrospectValue::Text("0:PointerUp".to_owned())).unwrap();
        e.invoke("send", IntrospectValue::Text(format!("2:PointerUp:{}", SHIFT.as_wire_token()))).unwrap();
        assert_eq!(json_indices(&e.query("selection").unwrap()), vec![0, 1, 2]);
        // Ctrl-toggle 1 out → {0,2}.
        e.invoke("send", IntrospectValue::Text(format!("1:PointerUp:{}", CTRL.as_wire_token()))).unwrap();
        assert_eq!(json_indices(&e.query("selection").unwrap()), vec![0, 2]);
    }

    #[test]
    fn r922_clear_empties_the_panel() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        e.invoke("clear", IntrospectValue::Null).unwrap();
        assert_eq!(json_indices(&e.query("selection").unwrap()), Vec::<u64>::new());
        assert_eq!(e.query("selected"), Some(IntrospectValue::Null));
        assert_eq!(e.query("row_count"), Some(IntrospectValue::Int(0)));
        assert_eq!(e.query("selection_summary"), Some(IntrospectValue::Text("No selection".to_owned())));
    }

    #[test]
    fn r922_selection_intervene_restores_a_set() {
        let mut e = ext();
        e.intervene("selection", IntrospectValue::Json(serde_json::json!([2, 0]))).unwrap();
        assert_eq!(json_indices(&e.query("selection").unwrap()), vec![0, 2]);
        // Player + Light share the base; Layer is 1 vs 2 → mixed.
        assert_eq!(e.query("mixed.1"), Some(IntrospectValue::Bool(true)));
    }

    #[test]
    fn r910_send_wire_selects_on_activation_edge() {
        let mut e = ext();
        e.invoke("send", IntrospectValue::Text("2:PointerEnter".to_owned())).unwrap();
        assert_eq!(e.cursor(), Some(0), "hover (PointerEnter) must not select");
        e.invoke("send", IntrospectValue::Text("2:PointerDown".to_owned())).unwrap();
        assert_eq!(e.cursor(), Some(0), "press (PointerDown) must not select");
        e.invoke("send", IntrospectValue::Text("2:PointerUp".to_owned())).unwrap();
        assert_eq!(e.cursor(), Some(2), "release (PointerUp) selects");
    }

    #[test]
    fn r909_object_names_addressable_regardless_of_selection() {
        let e = ext();
        assert_eq!(e.query("object_name.0"), Some(IntrospectValue::Text("Player".to_owned())));
        assert_eq!(e.query("object_name.2"), Some(IntrospectValue::Text("Sun Light".to_owned())));
        assert_eq!(e.query("object_name.9"), None);
    }

    #[test]
    fn r909_read_only_axes_reject_intervene() {
        let mut e = ext();
        assert_eq!(e.intervene("object_count", IntrospectValue::Int(1)), Err(InterveneError::ReadOnly));
        assert_eq!(e.intervene("row_count", IntrospectValue::Int(1)), Err(InterveneError::ReadOnly));
        assert_eq!(e.intervene("mixed.0", IntrospectValue::Bool(true)), Err(InterveneError::ReadOnly));
        assert_eq!(e.intervene("selection_summary", IntrospectValue::Text("x".to_owned())), Err(InterveneError::ReadOnly));
    }

    fn has_text(scene: &Scene, needle: &str) -> bool {
        match scene {
            Scene::Text(t) => t.content == needle,
            Scene::Container(c) => c.children.iter().any(|ch| has_text(ch, needle)),
            _ => false,
        }
    }

    #[test]
    fn default_object_count_matches_state_bitmap() {
        assert_eq!(default_objects().len(), N_OBJECTS, "the bitmap N must track the object roster");
    }

    #[test]
    fn view_reflects_multi_selection_with_mixed_placeholder() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            view(&InspectorState::from_parts(&[0, 1, 2], Some(2)), &Frame::new())
        });
        assert!(has_text(&scene, "3 objects selected"), "header summarises the selection");
        assert!(has_text(&scene, "Layer"), "common property shown");
        assert!(has_text(&scene, "Multiple Values"), "mixed property shows the placeholder");
    }

    #[test]
    fn a11y_object_list_is_multiselectable_with_aria_selected() {
        let nodes = Owner::new().run(|| {
            <InspectorView as WidgetA11y>::access_node(
                &InspectorState::from_parts(&[0, 2], Some(2)),
                None,
            )
        });
        // [root Group, listbox, option_0, option_1, option_2]
        assert_eq!(nodes[0].role, AriaRole::Group);
        assert_eq!(nodes[0].tag, INSPECTOR_TAG);
        let listbox = nodes.iter().find(|n| n.tag == OBJECTS_TAG).expect("listbox node");
        assert_eq!(listbox.role, AriaRole::Listbox);
        assert!(listbox.multiselectable, "object list is aria-multiselectable");
        let opt = |i: usize| nodes.iter().find(|n| n.tag == format!("{INSPECTOR_TAG}#{i}")).unwrap();
        assert_eq!(opt(0).selected, Some(true), "object 0 selected");
        assert_eq!(opt(1).selected, Some(false), "object 1 not selected");
        assert_eq!(opt(2).selected, Some(true), "object 2 selected");
        assert!(opt(2).state.focused, "cursor object is the active descendant");
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<InspectorView>(
            InspectorState::from_parts(&[0], Some(0)),
            &Frame::new(),
        );
    }

    // ─── R922.1 audit-clearance regressions ───────────────────────

    #[test]
    fn r922_1_choice_is_common_only_with_equal_options() {
        // Two objects, each with a "Mode" Choice. The correctness fix: a Choice
        // is "common" only if the option lists match (not just kind==Choice),
        // so a write-all index can never be valid for one object and out of
        // range for another (the silent half-write the audit found).
        let mode = |opts: &[&str]| {
            ObjectData::new(
                "obj",
                vec![Property::new(
                    "Mode",
                    CellValue::Choice {
                        selected: 0,
                        options: opts.iter().map(|s| (*s).to_owned()).collect(),
                    },
                )],
            )
        };
        let sel: BTreeSet<usize> = [0, 1].into_iter().collect();
        // Equal option lists → the Choice IS a common property.
        let equal = vec![mode(&["A", "B", "C"]), mode(&["A", "B", "C"])];
        let common = common_properties(&equal, &sel);
        assert_eq!(common.len(), 1, "equal-option Choice is common");
        assert_eq!(common[0].name, "Mode");
        // Different option lists → NOT common (grouping them would let a
        // write-all of option 2 silently half-apply: valid for [A,B,C], out of
        // range for [A,B]).
        let diverge = vec![mode(&["A", "B", "C"]), mode(&["A", "B"])];
        assert!(
            common_properties(&diverge, &sel).is_empty(),
            "divergent-option Choice is NOT a common property"
        );
    }

    #[test]
    fn r922_1_write_all_applies_to_an_equal_shape_choice() {
        // A same-shape common Choice: the write-all reaches every selected
        // object (no silent skip — the fail-loud path stays unreached).
        let owner = Owner::new();
        let mut e = owner.run(|| {
            let mode = |selected: usize| {
                ObjectData::new(
                    "o",
                    vec![Property::new(
                        "Mode",
                        CellValue::Choice {
                            selected,
                            options: vec!["A".to_owned(), "B".to_owned(), "C".to_owned()],
                        },
                    )],
                )
            };
            let objects = Rc::new(Signal::new(vec![mode(0), mode(1)]));
            let mut model = VirtualSelect::new(2, SelectionMode::Multi);
            model.set_selection(&[0usize, 1].into_iter().collect());
            InspectorExternal::new(objects, Rc::new(Signal::new(model)))
        });
        // value.0 (the common "Mode") set to option index 2 across both.
        e.intervene("value.0", IntrospectValue::Int(2)).unwrap();
        let selected_index = |e: &InspectorExternal| match e.query("value.0") {
            Some(IntrospectValue::Json(v)) => v["selected"].as_u64(),
            other => panic!("expected a Choice json, got {other:?}"),
        };
        e.invoke("select", IntrospectValue::Int(0)).unwrap();
        assert_eq!(selected_index(&e), Some(2), "object 0 written to option 2");
        e.invoke("select", IntrospectValue::Int(1)).unwrap();
        assert_eq!(selected_index(&e), Some(2), "object 1 also written to option 2");
    }

    #[test]
    fn r922_1_details_panel_is_a11y_reachable_with_mixed_label() {
        // All three objects selected → the base properties are mixed. The
        // Details panel (the headline content) must be IN the a11y tree
        // (not orphaned), with each property a `listitem` whose name carries
        // the value the paint shows ("Multiple Values" when mixed).
        let nodes = Owner::new().run(|| {
            <InspectorView as WidgetA11y>::access_node(
                &InspectorState::from_parts(&[0, 1, 2], Some(2)),
                None,
            )
        });
        assert!(
            nodes[0].children.iter().any(|c| c.as_str() == DETAIL_TAG),
            "root references the Details panel"
        );
        let panel = nodes.iter().find(|n| n.tag == DETAIL_TAG).expect("detail panel node");
        assert_eq!(panel.role, AriaRole::List);
        assert_eq!(panel.name.as_deref(), Some("3 objects selected"));
        let items: Vec<&AccessNode> = nodes.iter().filter(|n| n.role == AriaRole::ListItem).collect();
        assert!(!items.is_empty(), "common properties are AT-reachable listitems");
        // "Visible" is (true, true, false) across the trio → mixed.
        let visible = items
            .iter()
            .find(|n| n.name.as_deref().is_some_and(|s| s.starts_with("Visible")))
            .expect("Visible row present");
        assert_eq!(
            visible.name.as_deref(),
            Some("Visible: Multiple Values"),
            "a mixed property announces Multiple Values (one-gate with the paint)"
        );
    }
}
