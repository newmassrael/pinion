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

use pinion_a11y::{
    AccessNode, AccessValue, AriaRole, ListOption, WidgetA11y, listbox_option_nodes,
};
use pinion_core::cell_value::CellValue;
use pinion_core::composite_tag::split_send_payload;
use pinion_core::external::query_proxy_external_impl;
use pinion_core::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
};
use pinion_core::input::{Modifiers, MultiSelectKeyOp, SelectionChord, is_activation_event};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, FontWeight, JustifyContent, LayoutStyle,
    Size, TextStyle,
};
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::widgets::virtual_select::{
    SelectionMode, VirtualSelect, clamp_nav, read_selected, read_selection, selected_to_value,
    selection_to_value,
};
use pinion_core::{ColorRole, Frame, Owner, Scene, Signal, use_theme};
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
/// R958 — the reset-arrow mark size (px), the trailing "modified, click to
/// reset" affordance on a Details row.
const RESET_DOT: u32 = 10;
/// R1221 — the trailing gutter (px) an INTERACTIVE value cell (Bool toggle /
/// numeric steppers) reserves so it never sits under the absolutely-positioned
/// reset dot at the row's trailing edge — else a `+`/toggle click on a *modified*
/// row would land on the reset dot instead. Read-only value cells (Choice / Color
/// / Text) do not need it (a click on them does nothing anyway).
const RESET_GUTTER: u32 = 16;
/// R958.1 — the Details (property) row width; the reset arrow's trailing X
/// inset derives from it so the arrow tracks the row edge if the width changes.
const DETAIL_ROW_W: u32 = 260;
const SWATCH: u32 = 16;
/// R1221 — the +/- numeric-stepper button size (px) on a Details value cell.
const STEPPER: u32 = 18;
/// R1224 — the keyboard-cursor focus ring width (px) painted around the active
/// row of whichever pane currently owns the keyboard (the paint peer of the a11y
/// active-descendant `focused`).
const FOCUS_RING: u32 = 2;

/// R1224 — which pane owns the keyboard cursor. The inspector is a single
/// focus stop (`with_focusable(true)` on the root) that hosts TWO composite
/// sub-regions — the object `listbox` (left) and the Details property form
/// (right) — so a `region` axis tracks which one the Arrow keys drive, the
/// WAI-ARIA "roving focus between grouped widgets" pattern (`Tab` cycles the
/// region, exactly as it would move between two separate widgets). Lives in the
/// External beside the selection so it is observable + drivable over RPC (§2 #2):
/// `focus_region` reads/sets it, matching the keyboard funnel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum FocusRegion {
    /// The multi-select object list (left) — Arrow keys move the object cursor.
    #[default]
    Objects,
    /// The Details property form (right) — Arrow keys move the property cursor
    /// and edit the value at it (toggle a Bool / step a numeric / reset).
    Details,
}

impl FocusRegion {
    /// The wire token the `focus_region` query reports + `intervene` accepts —
    /// one SSOT so the RPC read and write cannot drift.
    fn wire(self) -> &'static str {
        match self {
            FocusRegion::Objects => "objects",
            FocusRegion::Details => "details",
        }
    }

    /// Decode a wire token back to a region; `None` on an unknown token (the
    /// caller Rejects rather than silently defaulting).
    fn from_wire(token: &str) -> Option<Self> {
        match token {
            "objects" => Some(FocusRegion::Objects),
            "details" => Some(FocusRegion::Details),
            _ => None,
        }
    }

    /// The other region — the `Tab` cycle target.
    fn toggled(self) -> Self {
        match self {
            FocusRegion::Objects => FocusRegion::Details,
            FocusRegion::Details => FocusRegion::Objects,
        }
    }
}

// ─── Model ────────────────────────────────────────────────────────

/// One typed property: a display name plus its [`CellValue`].
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Property {
    name: String,
    value: CellValue,
}

impl Property {
    fn new(name: &str, value: CellValue) -> Self {
        Self {
            name: name.to_owned(),
            value,
        }
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
        Self {
            name: name.to_owned(),
            properties,
        }
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

/// R958 — the frozen class-defaults snapshot (the original [`default_objects`]),
/// the baseline every property's "modified" indicator + reset compares against.
/// Immutable `Rc<Vec<ObjectData>>` cached per Owner (mirrors hello-property-grid's
/// `use_property_defaults`); never mutated, so a reset always restores the boot
/// value. Same object indices + property names as the live model, so a lookup by
/// `(object index, property name)` always resolves.
fn use_object_defaults() -> Rc<Vec<ObjectData>> {
    let owner = Owner::current().expect("use_object_defaults requires an active Owner scope");
    owner.cache("inspector.defaults", default_objects)
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

/// R1224 — the keyboard-focus state: which pane owns the cursor, plus the active
/// Details property row (the a11y active-descendant + keyboard-edit target).
/// `prop_cursor` is stored raw and clamped against the live common-property
/// count on read (the selection can shrink it out of range), so it is not the
/// authority for "is this a valid row" — [`InspectorExternal::edit_cursor`] is.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct InspectorFocus {
    region: FocusRegion,
    prop_cursor: Option<usize>,
}

/// R1224 — the shared keyboard-focus holder, persisted across rebuilds like the
/// object + selection models so the region / property-cursor survive a re-render.
fn use_focus() -> Rc<Signal<InspectorFocus>> {
    let owner = Owner::current().expect("use_focus requires an active Owner scope");
    owner.cache("inspector.focus", || Signal::new(InspectorFocus::default()))
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
        rows.push(CommonProperty {
            name: prop.name.clone(),
            value: prop.value.clone(),
            mixed,
        });
    }
    rows
}

/// R958 — the composite-tag key prefix the Details-panel reset arrow routes
/// under (`inspector#reset<i>`), distinguishing a reset click from a row-select
/// click (`inspector#<i>`) in [`InspectorExternal::handle_send`].
const RESET_PREFIX: &str = "reset";
/// R1221 — the Details value-cell edit-gesture key prefixes (the `inspector#`
/// funnel carries them alongside `<i>` row-select and `reset<i>`, distinguished
/// by prefix in [`InspectorExternal::handle_send`]): `toggle<i>` flips a Bool,
/// `inc<i>` / `dec<i>` step a numeric across the whole selection.
const TOGGLE_PREFIX: &str = "toggle";
const INC_PREFIX: &str = "inc";
const DEC_PREFIX: &str = "dec";
/// R1221 — the per-`inc`/`dec` step for a `Float` common property (an `Int`
/// steps by 1). Common Float rows do not arise in the sample data (the shared
/// actor properties are Bool/Int), so this only bites a future shared Float.
const FLOAT_STEP: f64 = 0.5;

/// R1221 — parse the `step_property` arg `"<idx>,<dir>"` (a common-property index
/// and a signed unit count). `None` on a malformed spec, so the verb Rejects
/// rather than silently no-op'ing.
fn parse_step_spec(spec: &str) -> Option<(usize, i32)> {
    let (idx, dir) = spec.split_once(',')?;
    Some((idx.trim().parse().ok()?, dir.trim().parse().ok()?))
}

/// R1221 — the common-property index a `<prefix><i>` send key carries (`reset3`,
/// `toggle0`, `inc1`, `dec1`), or `None` when `key` lacks `prefix` or the tail is
/// not an index. The one SSOT the [`InspectorExternal::handle_send`] Details-cell
/// gesture dispatch reads, so each gesture is just a `prefix -> action` line (the
/// `strip_prefix`+`parse` wiring is factored out — R727/R732 3rd-consumer lift).
fn prefixed_index(key: &str, prefix: &str) -> Option<usize> {
    key.strip_prefix(prefix)?.parse().ok()
}

/// R958 — is the common property `name` MODIFIED from its class default in ANY
/// selected object? Each selected object compares its own current value (by
/// name) to its own frozen default (by name) via the NaN-safe
/// [`CellValue::value_eq`] SSOT, so the "reset to default" arrow shows whenever
/// the selection diverges from the baseline — the Unreal / Qt inspector
/// affordance. Orthogonal to [`CommonProperty::mixed`]: a property can be
/// uniform-but-modified or mixed-but-default. The single source the External
/// query, the reset gate, and the paint indicator all read (divergence-is-a-bug).
fn property_modified_from_default(
    objects: &[ObjectData],
    selection: &BTreeSet<usize>,
    defaults: &[ObjectData],
    name: &str,
) -> bool {
    selection.iter().any(|&j| {
        let cur = objects
            .get(j)
            .and_then(|o| o.properties.iter().find(|p| p.name == name));
        let def = defaults
            .get(j)
            .and_then(|o| o.properties.iter().find(|p| p.name == name));
        matches!((cur, def), (Some(c), Some(d)) if !c.value.value_eq(&d.value))
    })
}

/// R958 — restore every property in `names` to its class default across every
/// selected object (the write SSOT shared by single-property reset and
/// reset-all). Each object restores its OWN default (defaults differ per object,
/// e.g. `Layer` is `1, 1, 2`), so after a reset the selection may read "Multiple
/// Values" yet "not modified".
fn reset_names_to_default(
    next: &mut [ObjectData],
    selection: &BTreeSet<usize>,
    defaults: &[ObjectData],
    names: &[String],
) {
    for &j in selection {
        for name in names {
            let def = defaults
                .get(j)
                .and_then(|o| o.properties.iter().find(|p| &p.name == name))
                .map(|p| p.value.clone());
            if let Some(defv) = def {
                if let Some(p) = next
                    .get_mut(j)
                    .and_then(|o| o.properties.iter_mut().find(|p| &p.name == name))
                {
                    p.value = defv;
                }
            }
        }
    }
}

/// The Details header text for a selection: the lone object's name when one
/// is selected, "N objects selected" for several, "No selection" for none.
fn selection_summary(objects: &[ObjectData], selection: &BTreeSet<usize>) -> String {
    let live: Vec<usize> = selection
        .iter()
        .copied()
        .filter(|&i| i < objects.len())
        .collect();
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
    /// R958 — the frozen class defaults the "modified" indicator + reset compare
    /// against (the [`use_object_defaults`] snapshot).
    defaults: Rc<Vec<ObjectData>>,
    /// R1224 — the keyboard-focus region + active Details property cursor (the
    /// `focus_region` / `focus_property` RPC surface + the keyboard funnel).
    focus: Rc<Signal<InspectorFocus>>,
}

impl InspectorExternal {
    fn new(
        objects: Rc<Signal<Vec<ObjectData>>>,
        selection: Rc<Signal<VirtualSelect>>,
        defaults: Rc<Vec<ObjectData>>,
        focus: Rc<Signal<InspectorFocus>>,
    ) -> Self {
        Self {
            objects,
            selection,
            defaults,
            focus,
        }
    }

    /// R958 — is common property `idx` modified from default in any selected
    /// object (the per-row reset-arrow gate + the `modified.<i>` query SSOT).
    fn common_modified(&self, idx: usize) -> bool {
        let common = self.common();
        common.get(idx).is_some_and(|prop| {
            property_modified_from_default(
                &self.objects.get(),
                &self.selection_set(),
                &self.defaults,
                &prop.name,
            )
        })
    }

    /// R958 — does the selection diverge from default on any common property
    /// (the panel-level "reset all" enable + `any_modified` query)?
    fn any_modified(&self) -> bool {
        let objects = self.objects.get();
        let selection = self.selection_set();
        self.common().iter().any(|prop| {
            property_modified_from_default(&objects, &selection, &self.defaults, &prop.name)
        })
    }

    /// R958 — reset common property `idx` to its class default across every
    /// selected object. No-op (returns `false`) when the property is already at
    /// default, so a redundant reset is idempotent.
    fn reset_property(&self, idx: usize) -> bool {
        // One `common()` build + one `objects.get()` (the cleaner shape
        // `reset_all_modified` already uses): resolve the name, gate on
        // modified, then write — not the prior common()-twice rebuild.
        let objects = self.objects.get();
        let selection = self.selection_set();
        let Some(name) = common_properties(&objects, &selection)
            .get(idx)
            .map(|p| p.name.clone())
        else {
            return false;
        };
        if !property_modified_from_default(&objects, &selection, &self.defaults, &name) {
            return false;
        }
        let names = vec![name];
        let defaults = Rc::clone(&self.defaults);
        self.objects.set_with(move |prev| {
            let mut next = prev.clone();
            reset_names_to_default(&mut next, &selection, &defaults, &names);
            next
        });
        true
    }

    /// R958 — reset every modified common property across the selection in one
    /// atomic write; returns the number of properties reset.
    fn reset_all_modified(&self) -> usize {
        let names: Vec<String> = {
            let objects = self.objects.get();
            let selection = self.selection_set();
            self.common()
                .into_iter()
                .filter(|prop| {
                    property_modified_from_default(&objects, &selection, &self.defaults, &prop.name)
                })
                .map(|prop| prop.name)
                .collect()
        };
        if names.is_empty() {
            return 0;
        }
        let count = names.len();
        let selection = self.selection_set();
        let defaults = Rc::clone(&self.defaults);
        self.objects.set_with(move |prev| {
            let mut next = prev.clone();
            reset_names_to_default(&mut next, &selection, &defaults, &names);
            next
        });
        count
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

    /// The number of Details rows (common properties) for the current selection —
    /// the [`clamp_nav`] bound the property cursor navigates within.
    fn row_count(&self) -> usize {
        self.common().len()
    }

    // ─── focus funnel (keyboard / RPC converge here) ─── R1224 ─────────

    /// The pane that currently owns the keyboard cursor.
    fn focus_region(&self) -> FocusRegion {
        self.focus.get().region
    }

    /// The active Details property row, CLAMPED to the live common-property
    /// count (a shrinking selection can leave the stored cursor out of range).
    /// `None` when there are no rows OR the cursor was never placed — the SSOT
    /// the `prop_cursor` query, the keyboard edit, the paint ring, and the a11y
    /// active-descendant all read, so none can address a stale row.
    fn edit_cursor(&self) -> Option<usize> {
        let last = self.row_count().checked_sub(1)?;
        self.focus.get().prop_cursor.map(|i| i.min(last))
    }

    /// Move the keyboard cursor to `region`. Entering the Details pane with no
    /// property cursor yet seeds it at the first row (if any), so the Arrow keys
    /// have an anchor immediately — the roving-focus "land on the first item"
    /// convention.
    fn set_region(&self, region: FocusRegion) {
        let seed = region == FocusRegion::Details && self.edit_cursor().is_none();
        let first = (self.row_count() > 0).then_some(0);
        self.focus.set_with(move |prev| {
            let mut next = *prev;
            next.region = region;
            if seed {
                next.prop_cursor = first;
            }
            next
        });
    }

    /// Place the Details property cursor at `idx` (clamped to a valid row, or
    /// `None` when there are no rows) and focus the Details pane — the single
    /// "focus this property" intent the `focus_property` verb and the keyboard
    /// row-navigation both drive.
    fn set_prop_cursor(&self, idx: Option<usize>) {
        let clamped = match self.row_count().checked_sub(1) {
            Some(last) => idx.map(|i| i.min(last)),
            None => None,
        };
        self.focus.set_with(move |prev| {
            let mut next = *prev;
            next.region = FocusRegion::Details;
            next.prop_cursor = clamped;
            next
        });
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
        if !is_activation_event(event_name) {
            return;
        }
        // R958 — a `reset<i>` key is the Details-panel reset arrow; route it to
        // the per-property reset instead of the row-select chord (the `inspector#`
        // funnel carries both, distinguished by the key prefix — the
        // hello-property-grid `reset<i>` send pattern).
        // R958 — the reset arrow; R1221 — the Details value-cell edit gestures
        // (toggle a Bool, step a numeric across the whole selection). Each is a
        // `prefix -> action` line over the [`prefixed_index`] SSOT; all write
        // through the same funnels the `reset` / `toggle_property` / `step_property`
        // verbs use (the invoke-funnel discipline). Checked before the row-select
        // `<i>` parse because these keys never parse as a bare index.
        if let Some(idx) = prefixed_index(key, RESET_PREFIX) {
            self.reset_property(idx);
            return;
        }
        if let Some(idx) = prefixed_index(key, TOGGLE_PREFIX) {
            self.toggle_property(idx);
            return;
        }
        if let Some(idx) = prefixed_index(key, INC_PREFIX) {
            self.step_property(idx, 1);
            return;
        }
        if let Some(idx) = prefixed_index(key, DEC_PREFIX) {
            self.step_property(idx, -1);
            return;
        }
        let Some(index) = key.parse::<usize>().ok() else {
            return;
        };
        match SelectionChord::from_modifiers(modifiers) {
            SelectionChord::Toggle => self.toggle(index),
            SelectionChord::Extend => self.extend_to(index),
            SelectionChord::Replace => self.select(index),
        }
    }

    /// Typed write into common property `idx` across **every** selected
    /// object (the multi-object edit). Validates the wire value once against
    /// the representative (surfacing a `TypeMismatch` without mutating), then
    /// applies [`CellValue::with_intervene`] per object so a choice sets its
    /// own option index and a colour parses the hex against its own value.
    /// R1223 — the single "transform a named property across the whole
    /// selection" funnel: for every selected object, find its property named
    /// `name` and, when `mutate` returns `Some`, replace the value. One
    /// `objects.set_with` + one iterate-find loop, shared by
    /// [`set_property`](Self::set_property) (absolute) and
    /// [`step_property`](Self::step_property) (relative) so the two cannot drift —
    /// `step_property` had INLINED a second copy of this scaffold (the R1221 3b
    /// self-grep missed it; R727/R732 mandate). `mutate` receives the object
    /// index (for a per-object lookup, though the current callers ignore it) and
    /// the object's own current value; `None` leaves the property untouched (no
    /// silent value-clobber). NOTE: [`reset_names_to_default`] stays a separate
    /// peer — restoring each object's own default across a *set* of names in one
    /// atomic write is a genuinely different operation, not this scaffold.
    fn mutate_selected(
        &self,
        name: &str,
        mut mutate: impl FnMut(usize, &CellValue) -> Option<CellValue>,
    ) {
        let name = name.to_owned();
        let selection = self.selection_set();
        self.objects.set_with(move |prev| {
            let mut next = prev.clone();
            for &j in &selection {
                if let Some(p) = next
                    .get_mut(j)
                    .and_then(|o| o.properties.iter_mut().find(|p| p.name == name))
                {
                    if let Some(v) = mutate(j, &p.value) {
                        p.value = v;
                    }
                }
            }
            next
        });
    }

    fn set_property(&self, idx: usize, value: &IntrospectValue) -> Result<(), InterveneError> {
        let common = self.common();
        let target = common.get(idx).ok_or(InterveneError::UnknownPath)?;
        // Validate against the representative once. Every selected object's
        // matching property has the SAME shape (`same_property_shape` — kind
        // plus a Choice's option list), so this accept/reject is the verdict
        // for the whole write: a value the representative accepts cannot be
        // rejected by a same-shape property.
        target.value.with_intervene(value.clone())?;
        let shape = target.value.clone();
        let value = value.clone();
        self.mutate_selected(&target.name.clone(), move |_j, cur| {
            // The shape gate the pre-R1223 find-predicate carried, now in the
            // per-object closure. Under the unique-name invariant this matches
            // the representative for every selected object.
            if !same_property_shape(cur, &shape) {
                return None;
            }
            match cur.with_intervene(value.clone()) {
                Ok(updated) => Some(updated),
                // Unreachable: same shape as the representative, which accepted
                // `value`. Fail loud in dev, no-op (not a silent clobber) in
                // release (R906 — no silent fallback on a should-be-impossible
                // branch).
                Err(e) => {
                    debug_assert!(
                        false,
                        "multi-write: a value valid for the representative was rejected by a same-shape property: {e:?}"
                    );
                    None
                }
            }
        });
        Ok(())
    }

    /// R1221 — set common **Bool** property `idx` across EVERY selected object
    /// (the multi-object toggle), writing through [`set_property`](Self::set_property)
    /// — the SAME path `intervene value.<i>` drives. R1223 — a **mixed** Bool
    /// resolves to `true` (checked), the Qt / Unreal convention for clicking an
    /// indeterminate checkbox: an order-INDEPENDENT definite state, not the
    /// pre-R1223 `!first_selected` (which turned a mostly-on group OFF depending
    /// on which object happened to be first). A **uniform** Bool flips. `false`
    /// (no write) when `idx` is out of range or the common property is not a Bool.
    fn toggle_property(&self, idx: usize) -> bool {
        let common = self.common();
        let Some(prop) = common.get(idx) else {
            return false;
        };
        let CellValue::Bool(cur) = prop.value else {
            return false;
        };
        let next = if prop.mixed { true } else { !cur };
        self.set_property(idx, &IntrospectValue::Bool(next)).is_ok()
    }

    /// R1221 — step common **numeric** property `idx` by `dir` (a signed unit
    /// count) on EVERY selected object, RELATIVELY (each object's own value +=
    /// dir·step), so a mixed numeric stays mixed but shifts together — the
    /// multi-object spinbox that PRESERVES the per-object differences (unlike the
    /// Bool toggle, which resolves to uniform). `Int` steps by `dir`, `Float` by
    /// `dir · FLOAT_STEP`. R1223 — funnels through the shared
    /// [`mutate_selected`](Self::mutate_selected).
    /// `false` when `idx` is out of range or the common property is not numeric.
    fn step_property(&self, idx: usize, dir: i32) -> bool {
        let common = self.common();
        let Some(prop) = common.get(idx) else {
            return false;
        };
        if !matches!(prop.value, CellValue::Int(_) | CellValue::Float(_)) {
            return false;
        }
        self.mutate_selected(&prop.name.clone(), move |_j, cur| match cur {
            CellValue::Int(v) => Some(CellValue::Int(v.saturating_add(i64::from(dir)))),
            CellValue::Float(v) => Some(CellValue::Float(v + f64::from(dir) * FLOAT_STEP)),
            // Unreachable: the common property is numeric, so every selected
            // object's same-name prop is numeric. `None` = no change (not the
            // pre-R1223 silent `clone()` — R906 discipline).
            _ => None,
        });
        true
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
            ("modified.<i>", "bool"),
            ("any_modified", "bool"),
            ("select", "int"),
            ("toggle", "int"),
            ("extend_to", "int"),
            ("select_all", "null"),
            ("clear", "null"),
            ("send", "string"),
            ("reset", "int"),
            ("reset_all", "null"),
            // R1221 — the Details inline-edit verbs (the AI-first peers of the
            // value-cell click gestures): flip a common Bool across the whole
            // selection, or step a common numeric (arg "<i>,<dir>", dir a signed
            // unit count). Both write across every selected object.
            ("toggle_property", "int"),
            ("step_property", "string"),
            // R1224 — the keyboard-focus surface (§2 #2: the region + property
            // cursor the Arrow keys drive are observable + drivable over RPC).
            // `focus_region` reads/sets which pane owns the cursor ("objects" /
            // "details"); `prop_cursor` reads the active Details row (clamped);
            // `focus_property` places the cursor at a row (and focuses Details).
            ("focus_region", "string"),
            ("prop_cursor", "int"),
            ("focus_property", "int"),
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
            "selection_summary" => Some(IntrospectValue::Text(selection_summary(
                &objects, &selection,
            ))),
            "mode" => Some(IntrospectValue::Text("multi".to_owned())),
            "row_count" => Some(IntrospectValue::Int(
                i64::try_from(common_properties(&objects, &selection).len()).ok()?,
            )),
            "any_modified" => Some(IntrospectValue::Bool(self.any_modified())),
            // R1224 — the keyboard-focus reads (the paint + a11y + AI all resolve
            // the active pane / property row through these).
            "focus_region" => Some(IntrospectValue::Text(self.focus_region().wire().to_owned())),
            "prop_cursor" => Some(selected_to_value(self.edit_cursor())),
            _ => {
                if let Some(j) = path.strip_prefix("object_name.") {
                    let j: usize = j.parse().ok()?;
                    return objects
                        .get(j)
                        .map(|o| IntrospectValue::Text(o.name.clone()));
                }
                let common = common_properties(&objects, &selection);
                if let Some(i) = path.strip_prefix("name.") {
                    let i: usize = i.parse().ok()?;
                    return common.get(i).map(|c| IntrospectValue::Text(c.name.clone()));
                }
                if let Some(i) = path.strip_prefix("kind.") {
                    let i: usize = i.parse().ok()?;
                    return common
                        .get(i)
                        .map(|c| IntrospectValue::Text(c.value.kind().name().to_owned()));
                }
                if let Some(i) = path.strip_prefix("value.") {
                    let i: usize = i.parse().ok()?;
                    return common.get(i).map(|c| c.value.to_introspect());
                }
                if let Some(i) = path.strip_prefix("mixed.") {
                    let i: usize = i.parse().ok()?;
                    return common.get(i).map(|c| IntrospectValue::Bool(c.mixed));
                }
                if let Some(i) = path.strip_prefix("modified.") {
                    let i: usize = i.parse().ok()?;
                    return (i < common.len())
                        .then(|| IntrospectValue::Bool(self.common_modified(i)));
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

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
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
            // R958 — reset common property `idx` to default across the selection;
            // returns whether anything changed (idempotent on an at-default row).
            "reset" => {
                let idx = int_arg(args)?;
                Ok(IntrospectValue::Bool(self.reset_property(idx)))
            }
            // R958 — reset every modified property across the selection; returns
            // the count reset (the panel-level "reset all").
            "reset_all" => Ok(IntrospectValue::Int(
                i64::try_from(self.reset_all_modified()).expect("reset count fits in i64"),
            )),
            // R1221 — flip a common Bool across the whole selection (the value-cell
            // click twin); `false` when `idx` is not a common Bool row.
            "toggle_property" => {
                let idx = int_arg(args)?;
                Ok(IntrospectValue::Bool(self.toggle_property(idx)))
            }
            // R1221 — step a common numeric across the whole selection by a signed
            // unit count (the +/- stepper twin). Arg `"<i>,<dir>"`; `false` when
            // `idx` is not a common numeric row.
            "step_property" => match args {
                IntrospectValue::Text(spec) => {
                    let (idx, dir) = parse_step_spec(&spec).ok_or(InvokeError::Rejected)?;
                    Ok(IntrospectValue::Bool(self.step_property(idx, dir)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R1224 — move the keyboard cursor to a pane (the `Tab`-toggle twin);
            // an unknown token is a typed Rejected, never a silent default.
            "focus_region" => match args {
                IntrospectValue::Text(token) => {
                    let region = FocusRegion::from_wire(&token).ok_or(InvokeError::Rejected)?;
                    self.set_region(region);
                    Ok(IntrospectValue::Text(self.focus_region().wire().to_owned()))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R1224 — place the Details property cursor at a row and focus the
            // Details pane (the Arrow-key row-navigation twin). Returns the
            // clamped cursor actually landed on (`Null` when the panel is empty).
            "focus_property" => {
                let idx = int_arg(args)?;
                self.set_prop_cursor(Some(idx));
                Ok(selected_to_value(self.edit_cursor()))
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
    InspectorExternal::new(
        use_objects(),
        use_selection(),
        use_object_defaults(),
        use_focus(),
    )
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
    /// R1224 — which pane owns the keyboard cursor (default [`FocusRegion::Objects`]).
    region: FocusRegion,
    /// R1224 — the active Details property row (already clamped by the External),
    /// the a11y active-descendant + keyboard-edit target when `region` is Details.
    prop_cursor: Option<usize>,
}

impl InspectorState {
    /// Build the bitmap state from the decoded selection set + cursor. The
    /// keyboard-focus axis defaults to [`FocusRegion::Objects`] with no property
    /// cursor; [`with_focus`](Self::with_focus) layers it on for `read_state`.
    fn from_parts(selection: &[usize], cursor: Option<usize>) -> Self {
        let mut selected = [false; N_OBJECTS];
        for &i in selection {
            if let Some(slot) = selected.get_mut(i) {
                *slot = true;
            }
        }
        Self {
            selected,
            cursor,
            ..Self::default()
        }
    }

    /// R1224 — layer the keyboard-focus axis (region + Details property cursor)
    /// onto the selection state, kept a separate builder so the many existing
    /// `from_parts` call sites (all selection-only) stay unchanged.
    fn with_focus(mut self, region: FocusRegion, prop_cursor: Option<usize>) -> Self {
        self.region = region;
        self.prop_cursor = prop_cursor;
        self
    }

    /// Whether the Details property row `i` is the active keyboard cursor — the
    /// one gate the paint focus ring and the a11y active-descendant share, so the
    /// screen and the AT never disagree on where focus rests. Only true when the
    /// Details pane owns the keyboard (an Objects-region cursor rests on the list).
    fn is_prop_cursor(&self, i: usize) -> bool {
        self.region == FocusRegion::Details && self.prop_cursor == Some(i)
    }

    /// Whether object row `i` is the active keyboard cursor — the object list's
    /// active-descendant, shown only while the Objects pane owns the keyboard
    /// (peer of [`is_prop_cursor`](Self::is_prop_cursor)).
    fn is_object_cursor(&self, i: usize) -> bool {
        self.region == FocusRegion::Objects && self.cursor == Some(i)
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
                    TextStyle::new()
                        .with_size_px(ROW_FONT_PX)
                        .with_fg(Color::rgb(0xff, 0xff, 0xff)),
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

/// R1221 — one +/- numeric stepper button, tagged so a click routes to
/// [`InspectorExternal::step_property`] via `handle_send`.
fn stepper_button(glyph: &str, tag: String, accent: Color) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            glyph.to_owned(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(ROW_FONT_PX)
                .with_fg(Color::rgb(0xff, 0xff, 0xff)),
        ))])
        .with_tag(tag)
        .with_style(BoxStyle::filled(accent).with_corner_radius(4))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(STEPPER, STEPPER)),
        ),
    )
}

/// R1221 — the interactive Details value cell. A **Bool** paints its pill (or
/// the mixed placeholder) inside a `inspector#toggle<i>` click target (flip
/// across the whole selection, resolving a mixed Bool to uniform); an **Int** /
/// **Float** paints `[-] value [+]` steppers (`inspector#dec<i>` /
/// `inspector#inc<i>`, a mix-preserving relative shift). Every other kind
/// (Choice / Color / Text) keeps the read-only [`detail_value_visual`] display
/// (still RPC-editable via `intervene value.<i>` — inline delegates for those
/// kinds are the property-grid's, reusable when a shared such property arises).
fn detail_value_cell(
    index: usize,
    prop: &CommonProperty,
    fg: Color,
    accent: Color,
    muted: Color,
) -> Scene {
    match prop.value {
        CellValue::Bool(_) => Scene::Container(
            ContainerNode::new(vec![detail_value_visual(prop, fg, accent, muted)])
                .with_tag(format!("{INSPECTOR_TAG}#{TOGGLE_PREFIX}{index}"))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_margin(Rect::new(0, 0, RESET_GUTTER, 0)),
                ),
        ),
        CellValue::Int(_) | CellValue::Float(_) => {
            let label = Scene::Text(TextNode::styled(
                common_value_label(prop),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(ROW_FONT_PX)
                    .with_fg(if prop.mixed { muted } else { fg }),
            ));
            Scene::Container(
                ContainerNode::new(vec![
                    stepper_button("-", format!("{INSPECTOR_TAG}#{DEC_PREFIX}{index}"), accent),
                    label,
                    stepper_button("+", format!("{INSPECTOR_TAG}#{INC_PREFIX}{index}"), accent),
                ])
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_gap(6)
                        .with_margin(Rect::new(0, 0, RESET_GUTTER, 0)),
                ),
            )
        }
        _ => detail_value_visual(prop, fg, accent, muted),
    }
}

/// One Details row: `name` (left, muted) + value visual (right). Tagged
/// `prop_<i>` so the demo can locate it. R958 — when `modified` (the property
/// diverges from its class default in any selected object) a reset arrow is
/// absolutely positioned at the trailing edge, tagged `inspector#reset<i>` so a
/// click routes to [`InspectorExternal::reset_property`] (the Unreal / Qt
/// "reset to default" affordance; the arrow paints only on a changed property).
/// R1224 — when `focused` (the Details pane owns the keyboard and this is the
/// property cursor) the row carries an accent focus ring, the paint peer of the
/// a11y active-descendant.
fn property_row(
    index: usize,
    prop: &CommonProperty,
    modified: bool,
    focused: bool,
    fg: Color,
    muted: Color,
    accent: Color,
) -> Scene {
    let name = Scene::Text(TextNode::styled(
        prop.name.clone(),
        Rect::default(),
        TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(muted),
    ));
    let mut children = vec![name, detail_value_cell(index, prop, fg, accent, muted)];
    if modified {
        // A small accent square at the trailing edge: the "modified, click to
        // reset" mark. Absolutely positioned (out of the SpaceBetween flow) so
        // the name / value layout is byte-identical to an unmodified row.
        children.push(Scene::Box(
            BoxNode::new(
                Rect::default(),
                BoxStyle::filled(accent).with_corner_radius(RESET_DOT / 2),
            )
            .with_tag(format!("{INSPECTOR_TAG}#{RESET_PREFIX}{index}"))
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(RESET_DOT, RESET_DOT))
                    .with_absolute_position(DETAIL_ROW_W - RESET_DOT, (ROW_H - RESET_DOT) / 2),
            ),
        ));
    }
    // R1224 — the keyboard focus ring: a rounded accent border on the active
    // property row (transparent fill, so the row content is unchanged). Only the
    // style is conditional; the layout is byte-identical focused or not.
    let mut style = BoxStyle::filled(Color::TRANSPARENT).with_corner_radius(5);
    if focused {
        style = style.with_border(Border::new(accent, FOCUS_RING));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("prop_{index}"))
            .with_style(style)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::SpaceBetween)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(DETAIL_ROW_W, ROW_H)),
            ),
    )
}

/// One object-list entry. A selected row carries an accent background; the
/// active (cursor) row additionally carries a light border — the paint peer
/// of the a11y `aria-selected` (fill) and active descendant (`focused`,
/// border).
fn object_row(
    index: usize,
    name: &str,
    selected: bool,
    is_cursor: bool,
    fg: Color,
    accent: Color,
) -> Scene {
    let fill = if selected { accent } else { Color::TRANSPARENT };
    let text_fg = if selected {
        Color::rgb(0xff, 0xff, 0xff)
    } else {
        fg
    };
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
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_weight(FontWeight::BOLD)
            .with_fg(on_surface),
    ));

    // Left: the multi-select object list.
    let list_rows: Vec<Scene> = objects
        .iter()
        .enumerate()
        .map(|(i, o)| {
            object_row(
                i,
                &o.name,
                state.is_selected(i),
                state.is_object_cursor(i),
                on_surface,
                accent,
            )
        })
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
        TextStyle::new()
            .with_size_px(HEADER_FONT_PX)
            .with_weight(FontWeight::BOLD)
            .with_fg(on_surface),
    ));
    let mut detail_children = vec![header];
    // R958 — the frozen defaults the per-row reset arrow compares against
    // (same SSOT the External's `modified.<i>` query reads).
    let defaults = use_object_defaults();
    for (i, prop) in common_properties(&objects, &selection).iter().enumerate() {
        let modified = property_modified_from_default(&objects, &selection, &defaults, &prop.name);
        detail_children.push(property_row(
            i,
            prop,
            modified,
            state.is_prop_cursor(i),
            on_surface,
            muted,
            accent,
        ));
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
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_gap(16)
                .with_align_items(AlignItems::Start),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![title, panes])
            .with_tag(INSPECTOR_TAG)
            .with_style(BoxStyle::filled(surface))
            // (R1030 §5.39) hand-composed focus stop — composing view owns the opt-in.
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_focusable(true)
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
                return InspectorState::from_parts(&read_selection(intro), read_selected(intro))
                    .with_focus(read_focus_region(intro), read_prop_cursor(intro));
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

    /// Keyboard control over BOTH inspector panes, `Tab`-switched (R1224). The
    /// inspector is one focus stop hosting two composite sub-regions, so a
    /// `focus_region` axis selects which the Arrow keys drive:
    ///
    /// - **`Tab`** cycles the region (Objects ⇄ Details). Consumed — the
    ///   inspector is the app's only focusable content, so it keeps focus
    ///   rather than falling through to the shell's focus-traverse.
    /// - **Objects** (the pre-R1224 multi-select nav): `Ctrl+A` selects all,
    ///   `Ctrl+Space` toggles the active row ([`MultiSelectKeyOp`]); Arrow /
    ///   Home / End navigate ([`clamp_nav`]), `Shift` extends the range.
    /// - **Details**: Arrow-Up/Down + Home/End move the property cursor
    ///   ([`clamp_nav`], funneled through `focus_property`); at the cursor
    ///   `Space`/`Enter` toggles a Bool, `ArrowLeft`/`-` and `ArrowRight`/`+`
    ///   step a numeric, `Delete`/`Backspace` resets a modified row.
    ///
    /// Every op goes through the SAME External verb its pointer / RPC twin uses
    /// (`select` / `toggle` / `focus_region` / `focus_property` / `toggle_property`
    /// / `step_property` / `reset`) — keyboard, pointer, and RPC are one path.
    fn apply_key(
        scene: &mut Scene,
        _focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        let Scene::External(node) = scene else {
            return false;
        };
        // Snapshot everything the dispatch reads, releasing the immutable borrow
        // before any `introspect_mut` mutation below.
        let (region, obj_count, obj_cursor, row_count, prop_cursor) = {
            let Some(intro) = node.handle.introspect() else {
                return false;
            };
            (
                read_focus_region(intro),
                read_count(intro),
                read_selected(intro),
                read_row_count(intro),
                read_prop_cursor(intro),
            )
        };

        if key == "Tab" {
            if let Some(intro) = node.handle.introspect_mut() {
                let token = region.toggled().wire().to_owned();
                let _ = intro.invoke("focus_region", IntrospectValue::Text(token));
            }
            return true;
        }

        if region == FocusRegion::Details {
            // Row navigation (Up/Down/Home/End) via the shared policy, funneled
            // through `focus_property` so keyboard and RPC place the cursor
            // identically.
            if let Some(target) = clamp_nav(prop_cursor, key, row_count, row_count) {
                if let (Some(intro), Ok(t)) = (node.handle.introspect_mut(), i64::try_from(target))
                {
                    let _ = intro.invoke("focus_property", IntrospectValue::Int(t));
                }
                return true;
            }
            // Value edits at the cursor. A cursor-less panel (empty selection)
            // leaves every edit a benign no-op.
            let Some(cursor) = prop_cursor else {
                return false;
            };
            let Ok(idx) = i64::try_from(cursor) else {
                return false;
            };
            let (verb, arg) = match key {
                " " | "Enter" => ("toggle_property", IntrospectValue::Int(idx)),
                "ArrowRight" | "+" => (
                    "step_property",
                    IntrospectValue::Text(format!("{cursor},1")),
                ),
                "ArrowLeft" | "-" => (
                    "step_property",
                    IntrospectValue::Text(format!("{cursor},-1")),
                ),
                "Delete" | "Backspace" => ("reset", IntrospectValue::Int(idx)),
                _ => return false,
            };
            if let Some(intro) = node.handle.introspect_mut() {
                let _ = intro.invoke(verb, arg);
            }
            return true;
        }

        // ── Objects region ──
        match MultiSelectKeyOp::classify(key, modifiers) {
            Some(MultiSelectKeyOp::SelectAll) => {
                if let Some(intro) = node.handle.introspect_mut() {
                    let _ = intro.invoke("select_all", IntrospectValue::Null);
                }
                return true;
            }
            Some(MultiSelectKeyOp::ToggleCursor) => {
                if let (Some(intro), Some(c)) = (node.handle.introspect_mut(), obj_cursor) {
                    if let Ok(c) = i64::try_from(c) {
                        let _ = intro.invoke("toggle", IntrospectValue::Int(c));
                    }
                }
                return obj_cursor.is_some();
            }
            None => {}
        }

        // Plain / Shift navigation. `clamp_nav` maps the key to a target index
        // (no paging here, so page == item_count); Shift extends, else replace.
        let Some(target) = clamp_nav(obj_cursor, key, obj_count, obj_count) else {
            return false;
        };
        let action = if modifiers.shift_key() {
            "extend_to"
        } else {
            "select"
        };
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

/// R1224 — the Details row count off introspect (the property-cursor
/// [`clamp_nav`] bound), mirroring [`read_count`] for the `row_count` query.
fn read_row_count(intro: &dyn ExternalIntrospect) -> usize {
    match intro.query("row_count") {
        Some(IntrospectValue::Int(n)) => usize::try_from(n).unwrap_or(0),
        _ => 0,
    }
}

/// R1224 — decode the keyboard-focus region off the External's introspect (the
/// `read_selected` peer for the `focus_region` axis); an unknown / absent value
/// falls back to [`FocusRegion::Objects`], the boot pane.
fn read_focus_region(intro: &dyn ExternalIntrospect) -> FocusRegion {
    match intro.query("focus_region") {
        Some(IntrospectValue::Text(token)) => FocusRegion::from_wire(&token).unwrap_or_default(),
        _ => FocusRegion::default(),
    }
}

/// R1224 — decode the active Details property cursor (already clamped by the
/// External) off introspect; `Null` / absent is `None`.
fn read_prop_cursor(intro: &dyn ExternalIntrospect) -> Option<usize> {
    match intro.query("prop_cursor") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    }
}

/// The Details-panel tag — a WAI-ARIA `group` of the common-property controls
/// (R1224; a `list` of read-only rows pre-R1224).
const DETAIL_TAG: &str = "detail_panel";

impl WidgetA11y for InspectorView {
    /// The whole inspector a11y tree, all derived from one `InspectorState`:
    ///
    /// - the object list as a multi-select WAI-ARIA `listbox`
    ///   ([`listbox_option_nodes`]): `aria-multiselectable`, each row an
    ///   `option` whose `aria-selected` is its membership and whose `focused`
    ///   (active descendant) is the cursor — shown only while the Objects pane
    ///   owns the keyboard (R1224 region-gated), the peer of the painted accent
    ///   fill (selected) + border (cursor);
    /// - the **Details panel** as a `group` named by the selection summary, one
    ///   node per common property in its operable role ([`detail_access_node`]):
    ///   a Bool `switch`, a numeric `spinbutton`, else an informational
    ///   `listitem` — each carrying the value the paint shows (R886.1 one-gate)
    ///   and, in the Details pane, the active-descendant on the property cursor.
    ///
    /// The root `Group` references both regions; keyboard focus roves between
    /// them (R1224), so at most one pane shows an active descendant at a time.
    fn access_node(state: &InspectorState, _focused: Option<&str>) -> Vec<AccessNode> {
        let objects = use_objects().get();
        let selection = state.selection_set();
        let tags: Vec<String> = (0..objects.len())
            .map(|i| format!("{INSPECTOR_TAG}#{i}"))
            .collect();
        let options: Vec<ListOption<'_>> = objects
            .iter()
            .enumerate()
            .map(|(i, o)| ListOption {
                tag: &tags[i],
                label: Some(&o.name),
                state: ListboxItemState::Idle,
                selected: state.is_selected(i),
                // R1224 — the object list's active-descendant shows only while
                // the Objects pane owns the keyboard (region-gated roving focus).
                focused: state.is_object_cursor(i),
            })
            .collect();

        let mut nodes = vec![
            AccessNode::new(INSPECTOR_TAG, AriaRole::Group)
                .with_name("Scene Inspector")
                .with_child(OBJECTS_TAG)
                .with_child(DETAIL_TAG),
        ];
        nodes.extend(listbox_option_nodes(
            OBJECTS_TAG,
            "Scene objects",
            true,
            &options,
        ));

        // R1224 — the Details panel: a `group` of property controls named by the
        // selection summary. Each interactive row is its operable WAI-ARIA role —
        // a Bool a `switch`, a numeric a `spinbutton` — carrying the SAME value
        // the paint shows and, when the Details pane owns the keyboard, the
        // active-descendant `focused` flag (peer of the painted focus ring).
        // Read-only rows (Choice / Color / Text) stay informational `listitem`s
        // named `"{name}: {value}"`. All tagged `prop_<i>`, the same tag the
        // paint uses, so each AT node resolves to its painted row's bounds.
        let rows = common_properties(&objects, &selection);
        let mut panel = AccessNode::new(DETAIL_TAG, AriaRole::Group)
            .with_name(selection_summary(&objects, &selection));
        for i in 0..rows.len() {
            panel = panel.with_child(format!("prop_{i}"));
        }
        nodes.push(panel);
        for (i, prop) in rows.iter().enumerate() {
            nodes.push(detail_access_node(i, prop, state.is_prop_cursor(i)));
        }
        nodes
    }
}

/// R1224 — the a11y node for one Details property row, its operable WAI-ARIA
/// role matching the interactive paint cell so an AT drives it the way the
/// pointer / keyboard do:
///
/// - **Bool** → `switch` (`with_value(AccessValue::Bool)` = `aria-checked`);
///   a mixed multi-object Bool has no definite check, so it carries the
///   `"Multiple Values"` `aria-valuetext` instead (the honest indeterminate
///   readout, one-gate with [`common_value_label`]).
/// - **Int / Float** → `spinbutton` with the numeric display as
///   `aria-valuetext` (the values carry no declared min/max, so a bare
///   value-text is the faithful reading — the AT still exposes Increment /
///   Decrement actions from the role).
/// - **Choice / Color / Text** → an informational `listitem` named
///   `"{name}: {value}"` (read-only over RPC via `intervene value.<i>`).
///
/// `focused` sets the active-descendant flag (the Details pane owns the
/// keyboard and this is the property cursor), the a11y peer of the painted ring.
fn detail_access_node(index: usize, prop: &CommonProperty, focused: bool) -> AccessNode {
    let tag = format!("prop_{index}");
    match prop.value {
        CellValue::Bool(b) => {
            let node = AccessNode::new(tag, AriaRole::Switch)
                .with_name(prop.name.clone())
                .with_focused(focused);
            if prop.mixed {
                node.with_value_text(MULTIPLE_VALUES)
            } else {
                node.with_value(AccessValue::Bool(b))
            }
        }
        CellValue::Int(_) | CellValue::Float(_) => AccessNode::new(tag, AriaRole::SpinButton)
            .with_name(prop.name.clone())
            .with_value_text(common_value_label(prop))
            .with_focused(focused),
        _ => AccessNode::new(tag, AriaRole::ListItem)
            .with_name(format!("{}: {}", prop.name, common_value_label(prop)))
            .with_focused(focused),
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

    #[test]
    fn r958_multi_object_modified_reset_to_each_default() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        // Common base across all three: Visible(0), Layer(1), Locked(2).
        assert_eq!(
            e.query("name.1"),
            Some(IntrospectValue::Text("Layer".to_owned()))
        );
        assert_eq!(
            e.query("modified.1"),
            Some(IntrospectValue::Bool(false)),
            "boot Layer is at default",
        );
        // Edit Layer to 5 across all three -> each diverges from its own default.
        e.intervene("value.1", IntrospectValue::Int(5)).unwrap();
        assert_eq!(
            e.query("modified.1"),
            Some(IntrospectValue::Bool(true)),
            "edited Layer is modified"
        );
        assert_eq!(e.query("any_modified"), Some(IntrospectValue::Bool(true)));
        // Reset Layer -> each object restores its OWN default (1, 1, 2).
        assert_eq!(
            e.invoke("reset", IntrospectValue::Int(1)).unwrap(),
            IntrospectValue::Bool(true)
        );
        assert_eq!(
            e.query("modified.1"),
            Some(IntrospectValue::Bool(false)),
            "reset clears modified"
        );
        // The per-object defaults differ (1, 1, 2), so the selection now reads
        // "Multiple Values" — reset-to-default is per object, not to a shared value.
        assert_eq!(
            e.query("mixed.1"),
            Some(IntrospectValue::Bool(true)),
            "per-object defaults -> mixed after reset",
        );
        // Idempotent: a reset on an at-default row changes nothing.
        assert_eq!(
            e.invoke("reset", IntrospectValue::Int(1)).unwrap(),
            IntrospectValue::Bool(false)
        );
    }

    // ── R1221 Details inline-edit gestures (toggle Bool / step numeric) ──────
    // Common rows across all three objects: Visible(0)=Bool, Layer(1)=Int,
    // Locked(2)=Bool.

    fn obj_value(e: &InspectorExternal, obj: usize, name: &str) -> CellValue {
        e.objects
            .get()
            .get(obj)
            .and_then(|o| o.properties.iter().find(|p| p.name == name).cloned())
            .unwrap_or_else(|| panic!("object {obj} has no {name}"))
            .value
    }

    #[test]
    fn r1221_toggle_bool_resolves_mixed_to_uniform_across_all() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        // Visible: Player=true, Camera=true, Light=false -> mixed.
        assert_eq!(
            e.query("mixed.0"),
            Some(IntrospectValue::Bool(true)),
            "Visible starts mixed"
        );
        // R1223 — a MIXED Bool resolves to `true` (checked), the Qt/Unreal
        // indeterminate-checkbox convention (order-independent), NOT
        // `!first_selected`.
        assert_eq!(
            e.invoke("toggle_property", IntrospectValue::Int(0))
                .unwrap(),
            IntrospectValue::Bool(true)
        );
        assert_eq!(
            e.query("mixed.0"),
            Some(IntrospectValue::Bool(false)),
            "toggle resolved the mix"
        );
        for obj in 0..3 {
            assert_eq!(
                obj_value(&e, obj, "Visible"),
                CellValue::Bool(true),
                "obj {obj} Visible resolved to checked (mixed -> true)"
            );
        }
        // A second toggle flips the (now uniform, true) value to false across all.
        assert_eq!(
            e.invoke("toggle_property", IntrospectValue::Int(0))
                .unwrap(),
            IntrospectValue::Bool(true)
        );
        for obj in 0..3 {
            assert_eq!(
                obj_value(&e, obj, "Visible"),
                CellValue::Bool(false),
                "obj {obj} Visible flipped (uniform true -> false)"
            );
        }
    }

    #[test]
    fn r1221_step_int_shifts_each_object_mix_preserving() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        // Layer: Player=1, Camera=1, Light=2 -> mixed. Step +1 shifts EACH by 1
        // (relative), so the mix is PRESERVED (1,1,2 -> 2,2,3), not collapsed.
        assert_eq!(
            e.query("mixed.1"),
            Some(IntrospectValue::Bool(true)),
            "Layer starts mixed"
        );
        assert_eq!(
            e.invoke("step_property", IntrospectValue::Text("1,1".to_owned()))
                .unwrap(),
            IntrospectValue::Bool(true)
        );
        assert_eq!(
            obj_value(&e, 0, "Layer"),
            CellValue::Int(2),
            "Player Layer +1"
        );
        assert_eq!(
            obj_value(&e, 1, "Layer"),
            CellValue::Int(2),
            "Camera Layer +1"
        );
        assert_eq!(
            obj_value(&e, 2, "Layer"),
            CellValue::Int(3),
            "Light Layer +1"
        );
        assert_eq!(
            e.query("mixed.1"),
            Some(IntrospectValue::Bool(true)),
            "still mixed after a uniform shift"
        );
        // Step -2 shifts each back below the start.
        assert!(matches!(
            e.invoke("step_property", IntrospectValue::Text("1,-2".to_owned())),
            Ok(IntrospectValue::Bool(true))
        ));
        assert_eq!(
            obj_value(&e, 2, "Layer"),
            CellValue::Int(1),
            "Light Layer 3 - 2"
        );
    }

    #[test]
    fn r1221_step_on_agreeing_selection_stays_uniform() {
        let mut e = ext();
        // Player + Camera both have Layer=1 (common, NOT mixed).
        e.intervene(
            "selection",
            IntrospectValue::Json(serde_json::json!([0, 1])),
        )
        .unwrap();
        assert_eq!(
            e.query("mixed.1"),
            Some(IntrospectValue::Bool(false)),
            "agreeing Layer is not mixed"
        );
        e.invoke("step_property", IntrospectValue::Text("1,1".to_owned()))
            .unwrap();
        assert_eq!(obj_value(&e, 0, "Layer"), CellValue::Int(2));
        assert_eq!(obj_value(&e, 1, "Layer"), CellValue::Int(2));
        assert_eq!(
            e.query("mixed.1"),
            Some(IntrospectValue::Bool(false)),
            "both shifted equally -> still uniform"
        );
    }

    #[test]
    fn r1221_edit_gestures_route_through_the_send_wire() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        // A value-cell click on Locked(2) arrives as `toggle2:PointerUp`.
        e.invoke(
            "send",
            IntrospectValue::Text("toggle2:PointerUp".to_owned()),
        )
        .unwrap();
        assert_eq!(
            e.query("mixed.2"),
            Some(IntrospectValue::Bool(false)),
            "gesture toggled Locked across all"
        );
        // A +/- stepper click on Layer(1) arrives as `inc1` / `dec1`.
        let before = obj_value(&e, 2, "Layer");
        e.invoke("send", IntrospectValue::Text("inc1:PointerUp".to_owned()))
            .unwrap();
        e.invoke("send", IntrospectValue::Text("inc1:PointerUp".to_owned()))
            .unwrap();
        e.invoke("send", IntrospectValue::Text("dec1:PointerUp".to_owned()))
            .unwrap();
        let CellValue::Int(b) = before else {
            panic!("Layer is Int")
        };
        assert_eq!(
            obj_value(&e, 2, "Layer"),
            CellValue::Int(b + 1),
            "net +1 from inc,inc,dec"
        );
    }

    #[test]
    fn r1221_toggle_rejects_nonbool_and_step_rejects_nonnumeric() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        // Layer(1) is Int, not Bool -> toggle is a no-op false.
        assert_eq!(
            e.invoke("toggle_property", IntrospectValue::Int(1))
                .unwrap(),
            IntrospectValue::Bool(false)
        );
        // Visible(0) is Bool, not numeric -> step is a no-op false.
        assert_eq!(
            e.invoke("step_property", IntrospectValue::Text("0,1".to_owned()))
                .unwrap(),
            IntrospectValue::Bool(false)
        );
        // An out-of-range index is a no-op false (never panics).
        assert_eq!(
            e.invoke("toggle_property", IntrospectValue::Int(99))
                .unwrap(),
            IntrospectValue::Bool(false)
        );
    }

    #[test]
    fn r1221_step_spec_parse_and_malformed_arg_rejected() {
        assert_eq!(parse_step_spec("1,-2"), Some((1, -2)));
        assert_eq!(parse_step_spec(" 0 , 1 "), Some((0, 1)));
        assert_eq!(parse_step_spec("nope"), None);
        assert_eq!(parse_step_spec("1"), None);
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        assert_eq!(
            e.invoke("step_property", IntrospectValue::Text("bad".to_owned())),
            Err(InvokeError::Rejected)
        );
        assert_eq!(
            e.invoke("step_property", IntrospectValue::Int(1)),
            Err(InvokeError::TypeMismatch)
        );
    }

    #[test]
    fn r1221_edit_verbs_are_schema_declared() {
        let e = ext();
        let fields: Vec<&str> = e.schema().fields.iter().map(|(p, _)| *p).collect();
        assert!(
            fields.contains(&"toggle_property"),
            "toggle_property schema-declared"
        );
        assert!(
            fields.contains(&"step_property"),
            "step_property schema-declared"
        );
    }

    #[test]
    fn r1221_edits_with_no_selection_are_no_ops() {
        let mut e = ext();
        e.invoke("clear", IntrospectValue::Null).unwrap();
        assert_eq!(
            e.invoke("toggle_property", IntrospectValue::Int(0))
                .unwrap(),
            IntrospectValue::Bool(false)
        );
        assert_eq!(
            e.invoke("step_property", IntrospectValue::Text("0,1".to_owned()))
                .unwrap(),
            IntrospectValue::Bool(false)
        );
    }

    #[test]
    fn r958_reset_all_clears_every_modified_property() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        e.intervene("value.0", IntrospectValue::Bool(false))
            .unwrap(); // Visible
        e.intervene("value.1", IntrospectValue::Int(9)).unwrap(); // Layer
        assert_eq!(e.query("any_modified"), Some(IntrospectValue::Bool(true)));
        // reset_all returns the count of modified properties it cleared (Locked
        // was never touched, so only Visible + Layer = 2).
        assert_eq!(
            e.invoke("reset_all", IntrospectValue::Null).unwrap(),
            IntrospectValue::Int(2)
        );
        assert_eq!(e.query("any_modified"), Some(IntrospectValue::Bool(false)));
        assert_eq!(e.query("modified.0"), Some(IntrospectValue::Bool(false)));
        assert_eq!(e.query("modified.1"), Some(IntrospectValue::Bool(false)));
    }

    #[test]
    fn r958_reset_arrow_paints_only_on_a_modified_row() {
        // The Details reset arrow (`inspector#reset<i>`) appears only when the
        // property is modified — the paint reads the same SSOT as the query.
        let owner = Owner::new();
        owner.run(|| {
            let mut e = make_inspector_external();
            e.invoke("select_all", IntrospectValue::Null).unwrap();
            let reset_tag = format!("{INSPECTOR_TAG}#{RESET_PREFIX}1"); // Layer (common idx 1)
            let sel = InspectorState::from_parts(&[0, 1, 2], Some(0));
            assert!(
                !view(&sel, &Frame::new()).contains_tag(&reset_tag),
                "no reset arrow on an at-default Layer row",
            );
            e.intervene("value.1", IntrospectValue::Int(5)).unwrap();
            assert!(
                view(&sel, &Frame::new()).contains_tag(&reset_tag),
                "the reset arrow paints once Layer is modified",
            );
        });
    }

    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
        meta: false,
    };
    const CTRL: Modifiers = Modifiers {
        shift: false,
        ctrl: true,
        alt: false,
        meta: false,
    };

    #[test]
    fn r909_default_selection_is_first_object() {
        let e = ext();
        assert_eq!(e.object_count(), 3);
        assert_eq!(e.cursor(), Some(0));
        assert!(
            e.query("selected_name").is_none(),
            "no selected_name slot (use selection_summary)"
        );
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
        assert_eq!(
            e.query("selection_summary"),
            Some(IntrospectValue::Text("Main Camera".to_owned()))
        );
        assert_eq!(
            e.query("name.0"),
            Some(IntrospectValue::Text("Visible".to_owned()))
        );
    }

    #[test]
    fn r909_select_out_of_range_is_rejected() {
        let mut e = ext();
        assert!(e.invoke("select", IntrospectValue::Int(9)).is_err());
        assert_eq!(
            e.cursor(),
            Some(0),
            "selection unchanged after a rejected select"
        );
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
        assert_eq!(
            e.query("name.0"),
            Some(IntrospectValue::Text("Visible".to_owned()))
        );
        assert_eq!(
            e.query("name.1"),
            Some(IntrospectValue::Text("Layer".to_owned()))
        );
        assert_eq!(
            e.query("name.2"),
            Some(IntrospectValue::Text("Locked".to_owned()))
        );
        // Player + Camera agree on every base property.
        assert_eq!(e.query("mixed.0"), Some(IntrospectValue::Bool(false)));
        assert_eq!(e.query("mixed.1"), Some(IntrospectValue::Bool(false)));
        assert_eq!(
            e.query("value.1"),
            Some(IntrospectValue::Int(1)),
            "both Layer 1"
        );
    }

    #[test]
    fn r922_multi_select_mixed_values_reported() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        assert_eq!(json_indices(&e.query("selection").unwrap()), vec![0, 1, 2]);
        assert_eq!(
            e.query("selection_summary"),
            Some(IntrospectValue::Text("3 objects selected".to_owned()))
        );
        // All three: Visible (true,true,false) and Layer (1,1,2) differ.
        assert_eq!(
            e.query("mixed.0"),
            Some(IntrospectValue::Bool(true)),
            "Visible mixed"
        );
        assert_eq!(
            e.query("mixed.1"),
            Some(IntrospectValue::Bool(true)),
            "Layer mixed"
        );
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
            assert_eq!(
                e.query("value.1"),
                Some(IntrospectValue::Int(5)),
                "object {obj} Layer == 5"
            );
        }
    }

    #[test]
    fn r922_shift_extend_and_ctrl_toggle() {
        let mut e = ext();
        // Plain select 0, Shift-extend to 2 → {0,1,2}.
        e.invoke("send", IntrospectValue::Text("0:PointerUp".to_owned()))
            .unwrap();
        e.invoke(
            "send",
            IntrospectValue::Text(format!("2:PointerUp:{}", SHIFT.as_wire_token())),
        )
        .unwrap();
        assert_eq!(json_indices(&e.query("selection").unwrap()), vec![0, 1, 2]);
        // Ctrl-toggle 1 out → {0,2}.
        e.invoke(
            "send",
            IntrospectValue::Text(format!("1:PointerUp:{}", CTRL.as_wire_token())),
        )
        .unwrap();
        assert_eq!(json_indices(&e.query("selection").unwrap()), vec![0, 2]);
    }

    #[test]
    fn r922_clear_empties_the_panel() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        e.invoke("clear", IntrospectValue::Null).unwrap();
        assert_eq!(
            json_indices(&e.query("selection").unwrap()),
            Vec::<u64>::new()
        );
        assert_eq!(e.query("selected"), Some(IntrospectValue::Null));
        assert_eq!(e.query("row_count"), Some(IntrospectValue::Int(0)));
        assert_eq!(
            e.query("selection_summary"),
            Some(IntrospectValue::Text("No selection".to_owned()))
        );
    }

    #[test]
    fn r922_selection_intervene_restores_a_set() {
        let mut e = ext();
        e.intervene(
            "selection",
            IntrospectValue::Json(serde_json::json!([2, 0])),
        )
        .unwrap();
        assert_eq!(json_indices(&e.query("selection").unwrap()), vec![0, 2]);
        // Player + Light share the base; Layer is 1 vs 2 → mixed.
        assert_eq!(e.query("mixed.1"), Some(IntrospectValue::Bool(true)));
    }

    #[test]
    fn r910_send_wire_selects_on_activation_edge() {
        let mut e = ext();
        e.invoke("send", IntrospectValue::Text("2:PointerEnter".to_owned()))
            .unwrap();
        assert_eq!(e.cursor(), Some(0), "hover (PointerEnter) must not select");
        e.invoke("send", IntrospectValue::Text("2:PointerDown".to_owned()))
            .unwrap();
        assert_eq!(e.cursor(), Some(0), "press (PointerDown) must not select");
        e.invoke("send", IntrospectValue::Text("2:PointerUp".to_owned()))
            .unwrap();
        assert_eq!(e.cursor(), Some(2), "release (PointerUp) selects");
    }

    #[test]
    fn r909_object_names_addressable_regardless_of_selection() {
        let e = ext();
        assert_eq!(
            e.query("object_name.0"),
            Some(IntrospectValue::Text("Player".to_owned()))
        );
        assert_eq!(
            e.query("object_name.2"),
            Some(IntrospectValue::Text("Sun Light".to_owned()))
        );
        assert_eq!(e.query("object_name.9"), None);
    }

    #[test]
    fn r909_read_only_axes_reject_intervene() {
        let mut e = ext();
        assert_eq!(
            e.intervene("object_count", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            e.intervene("row_count", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            e.intervene("mixed.0", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            e.intervene("selection_summary", IntrospectValue::Text("x".to_owned())),
            Err(InterveneError::ReadOnly)
        );
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
        assert_eq!(
            default_objects().len(),
            N_OBJECTS,
            "the bitmap N must track the object roster"
        );
    }

    #[test]
    fn view_reflects_multi_selection_with_mixed_placeholder() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            view(
                &InspectorState::from_parts(&[0, 1, 2], Some(2)),
                &Frame::new(),
            )
        });
        assert!(
            has_text(&scene, "3 objects selected"),
            "header summarises the selection"
        );
        assert!(has_text(&scene, "Layer"), "common property shown");
        assert!(
            has_text(&scene, "Multiple Values"),
            "mixed property shows the placeholder"
        );
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
        let listbox = nodes
            .iter()
            .find(|n| n.tag == OBJECTS_TAG)
            .expect("listbox node");
        assert_eq!(listbox.role, AriaRole::Listbox);
        assert!(
            listbox.multiselectable,
            "object list is aria-multiselectable"
        );
        let opt = |i: usize| {
            nodes
                .iter()
                .find(|n| n.tag == format!("{INSPECTOR_TAG}#{i}"))
                .unwrap()
        };
        assert_eq!(opt(0).selected, Some(true), "object 0 selected");
        assert_eq!(opt(1).selected, Some(false), "object 1 not selected");
        assert_eq!(opt(2).selected, Some(true), "object 2 selected");
        assert!(
            opt(2).state.focused,
            "cursor object is the active descendant"
        );
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
            // R958 — defaults match the boot values (option 0), so the initial
            // state reads "not modified"; the test edits then asserts the change.
            let defaults = Rc::new(vec![mode(0), mode(1)]);
            InspectorExternal::new(
                objects,
                Rc::new(Signal::new(model)),
                defaults,
                Rc::new(Signal::new(InspectorFocus::default())),
            )
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
        assert_eq!(
            selected_index(&e),
            Some(2),
            "object 1 also written to option 2"
        );
    }

    #[test]
    fn r922_1_details_panel_is_a11y_reachable_with_mixed_label() {
        // All three objects selected → the base properties are mixed. The
        // Details panel (the headline content) must be IN the a11y tree (not
        // orphaned). R1224 — the panel is a `group` and each interactive row its
        // operable role (Bool `switch`, numeric `spinbutton`), each carrying the
        // value the paint shows ("Multiple Values" when mixed).
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
        let panel = nodes
            .iter()
            .find(|n| n.tag == DETAIL_TAG)
            .expect("detail panel node");
        assert_eq!(panel.role, AriaRole::Group);
        assert_eq!(panel.name.as_deref(), Some("3 objects selected"));
        let row = |name: &str| {
            nodes
                .iter()
                .find(|n| n.name.as_deref() == Some(name))
                .unwrap_or_else(|| panic!("{name} row present"))
        };
        // "Visible" is (t, t, f) → a mixed Bool: a `switch` with no definite
        // check, announcing "Multiple Values" via aria-valuetext (one-gate).
        let visible = row("Visible");
        assert_eq!(visible.role, AriaRole::Switch, "a Bool row is a switch");
        assert_eq!(
            visible.value_text.as_deref(),
            Some(MULTIPLE_VALUES),
            "a mixed Bool announces Multiple Values, no definite check"
        );
        assert!(visible.value.is_none(), "mixed Bool has no aria-checked");
        // "Layer" is (1, 1, 2) → a mixed numeric: a `spinbutton`.
        let layer = row("Layer");
        assert_eq!(
            layer.role,
            AriaRole::SpinButton,
            "a numeric row is a spinbutton"
        );
        assert_eq!(layer.value_text.as_deref(), Some(MULTIPLE_VALUES));
    }

    // ─── R1224 keyboard-focus + interactive-cell a11y ─────────────────

    #[test]
    fn r1224_focus_region_verb_roundtrips_and_rejects_unknown() {
        let mut e = ext();
        assert_eq!(
            e.query("focus_region"),
            Some(IntrospectValue::Text("objects".to_owned())),
            "boot pane is the object list"
        );
        assert_eq!(
            e.invoke("focus_region", IntrospectValue::Text("details".to_owned()))
                .unwrap(),
            IntrospectValue::Text("details".to_owned()),
            "the setter returns the read-back region"
        );
        assert_eq!(
            e.query("focus_region"),
            Some(IntrospectValue::Text("details".to_owned()))
        );
        assert!(
            e.invoke("focus_region", IntrospectValue::Text("bogus".to_owned()))
                .is_err(),
            "an unknown region token is a typed Rejected, not a silent default"
        );
        assert!(
            e.invoke("focus_region", IntrospectValue::Int(1)).is_err(),
            "a non-string region arg is a TypeMismatch"
        );
    }

    #[test]
    fn r1224_focus_property_clamps_focuses_details_and_tracks_row_count() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        assert_eq!(e.query("row_count"), Some(IntrospectValue::Int(3)));
        // A cursor past the last row clamps to the last, and placing it focuses
        // the Details pane (the "focus this property" intent).
        assert_eq!(
            e.invoke("focus_property", IntrospectValue::Int(99))
                .unwrap(),
            IntrospectValue::Int(2),
            "an out-of-range cursor clamps to the last row"
        );
        assert_eq!(e.query("prop_cursor"), Some(IntrospectValue::Int(2)));
        assert_eq!(
            e.query("focus_region"),
            Some(IntrospectValue::Text("details".to_owned())),
            "focus_property focuses the Details pane"
        );
        // Shrinking the panel re-clamps the reported cursor; an empty panel has none.
        e.invoke("clear", IntrospectValue::Null).unwrap();
        assert_eq!(e.query("row_count"), Some(IntrospectValue::Int(0)));
        assert_eq!(
            e.query("prop_cursor"),
            Some(IntrospectValue::Null),
            "an empty panel reports no property cursor"
        );
    }

    #[test]
    fn r1224_entering_details_seeds_the_first_property_cursor() {
        let mut e = ext();
        // Boot: object 0 selected, region Objects, no property cursor yet.
        assert_eq!(e.query("prop_cursor"), Some(IntrospectValue::Null));
        e.invoke("focus_region", IntrospectValue::Text("details".to_owned()))
            .unwrap();
        assert_eq!(
            e.query("prop_cursor"),
            Some(IntrospectValue::Int(0)),
            "entering the Details pane seeds the cursor at the first row"
        );
    }

    #[test]
    fn r1224_a11y_bool_is_switch_numeric_is_spinbutton_with_active_descendant() {
        // Single selection (Player) → every property uniform; cursor on row 0
        // (Visible, a Bool) with the Details pane focused.
        let nodes = Owner::new().run(|| {
            <InspectorView as WidgetA11y>::access_node(
                &InspectorState::from_parts(&[0], Some(0))
                    .with_focus(FocusRegion::Details, Some(0)),
                None,
            )
        });
        let row = |tag: &str| nodes.iter().find(|n| n.tag == tag).unwrap();
        // Visible (Bool, uniform true) → switch carrying aria-checked, and the
        // active descendant (Details cursor on row 0).
        let visible = row("prop_0");
        assert_eq!(visible.role, AriaRole::Switch);
        assert_eq!(visible.value, Some(AccessValue::Bool(true)));
        assert!(
            visible.state.focused,
            "the cursor row is the active descendant"
        );
        // Layer (Int) → spinbutton, not focused.
        let layer = row("prop_1");
        assert_eq!(layer.role, AriaRole::SpinButton);
        assert!(!layer.state.focused);
    }

    #[test]
    fn r1224_active_descendant_roves_between_the_two_panes() {
        let obj_focused = |nodes: &[AccessNode]| {
            nodes
                .iter()
                .find(|n| n.tag == format!("{INSPECTOR_TAG}#1"))
                .unwrap()
                .state
                .focused
        };
        let detail_focused = |nodes: &[AccessNode]| {
            nodes
                .iter()
                .find(|n| n.tag == "prop_0")
                .unwrap()
                .state
                .focused
        };

        // Objects pane owns the keyboard: the object cursor is the active
        // descendant; no Details row is.
        let objects = Owner::new().run(|| {
            <InspectorView as WidgetA11y>::access_node(
                &InspectorState::from_parts(&[0, 1], Some(1))
                    .with_focus(FocusRegion::Objects, Some(0)),
                None,
            )
        });
        assert!(obj_focused(&objects), "Objects pane: object cursor focused");
        assert!(
            !detail_focused(&objects),
            "Objects pane: no Details active descendant"
        );

        // Details pane owns the keyboard: the roles flip — exactly one pane
        // shows an active descendant at a time.
        let details = Owner::new().run(|| {
            <InspectorView as WidgetA11y>::access_node(
                &InspectorState::from_parts(&[0, 1], Some(1))
                    .with_focus(FocusRegion::Details, Some(0)),
                None,
            )
        });
        assert!(
            !obj_focused(&details),
            "Details pane: object cursor not focused"
        );
        assert!(
            detail_focused(&details),
            "Details pane: Details cursor focused"
        );
    }

    /// Whether the tagged container in `scene` carries a border (the R1224 focus
    /// ring) — the paint-side probe for the focus-ring test.
    fn container_has_border(scene: &Scene, tag: &str) -> Option<bool> {
        if let Scene::Container(c) = scene {
            if c.tag.as_deref() == Some(tag) {
                return Some(c.style.border.is_some());
            }
            for child in &c.children {
                if let Some(hit) = container_has_border(child, tag) {
                    return Some(hit);
                }
            }
        }
        None
    }

    #[test]
    fn r1224_paint_focus_ring_only_on_the_details_cursor_row() {
        // The paint focus ring (a border on the property row) is the peer of the
        // a11y active descendant: present on the cursor row iff the Details pane
        // owns the keyboard, byte-identical layout otherwise.
        let ringed = |state: &InspectorState| {
            let scene = Owner::new().run(|| view(state, &Frame::new()));
            container_has_border(&scene, "prop_0").expect("prop_0 row present")
        };
        let details_cursor =
            InspectorState::from_parts(&[0], Some(0)).with_focus(FocusRegion::Details, Some(0));
        let objects_cursor =
            InspectorState::from_parts(&[0], Some(0)).with_focus(FocusRegion::Objects, Some(0));
        assert!(
            ringed(&details_cursor),
            "Details cursor row carries the focus ring"
        );
        assert!(
            !ringed(&objects_cursor),
            "no ring when the Objects pane owns the keyboard"
        );
    }
}
