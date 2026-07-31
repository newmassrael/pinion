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
//! the write-all edit — is complete and entirely RPC-driven. R1373 adds the
//! iconic DCC **numeric drag-scrub** (press-and-drag a common `Int` / `Float`
//! row to shift it across the WHOLE selection, mix-preserving — the continuous
//! peer of the R1221 steppers), riding the already-lifted [`DragCalibration`]
//! substrate. The remaining property-grid delegate richness — the Choice
//! dropdown popup and the Colour swatch popup (both a floating overlay, not yet
//! a lifted substrate) — stays the documented GUI follow-up; those kinds keep
//! their click-cycle / hex type-in, and every value is editable through
//! `intervene value.<i>` (the §2 primary path).
//!
//! ## Verification
//!
//! `tools/demos/r909_inspector.py` drives single-select + editing (the
//! cardinality-1 degenerate case), `tools/demos/r910_inspector_interaction.py`
//! the pointer/keyboard navigation, `tools/demos/r922_inspector_multi.py`
//! the multi-object core (multi-select, common-property panel, "Multiple
//! Values", write-all), and `tools/demos/r1373_inspector_scrub.py` the numeric
//! drag-scrub (single + multi-object mix-preserving). All scene-as-data,
//! deterministic ([[ai-first-rpc-introspection-obligation]],
//! [[introspection-from-paint-not-screen]]).

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use pinion_a11y::{
    AccessNode, AccessValue, AriaRole, ListOption, WidgetA11y, listbox_option_nodes,
};
use pinion_core::cell_value::{CellKind, CellValue};
use pinion_core::command::Command;
use pinion_core::composite_tag::{prefixed_index, split_send_payload};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, CaptureNormalize, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg,
    SchemaField, ThreadOwnership, read_only_or_unknown,
};
use pinion_core::input::{
    DRAG_CLICK_THRESHOLD_PX, DragCalibration, Modifiers, MultiSelectKeyOp, SelectionChord,
    edit_field_keymap, is_activation_event,
};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, FontWeight, JustifyContent, LayoutStyle,
    Size, TextStyle,
};
use pinion_core::theme::Theme;
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::widgets::text_edit::{TextEditState, use_text_edit_state};
use pinion_core::widgets::text_field::{TextFieldState, blur_committing_field_extra};
use pinion_core::widgets::virtual_select::{
    SelectionMode, VirtualSelect, clamp_nav, read_selected, read_selection, selected_to_value,
    selection_to_value,
};
use pinion_core::{ColorRole, Frame, Owner, Scene, Signal, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::text_field as tf_paint;

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
/// The Details-panel container tag — a WAI-ARIA `group` of the common-property
/// controls (R1224; a `list` of read-only rows pre-R1224), and (R1373) the
/// numeric scrub's [`capture_normalize`](External::capture_normalize) basis rect
/// (a fixed [`DETAIL_PANEL_W`]-wide column, so a captured cursor's fraction
/// recovers true pixel travel). One tag: the view, the a11y group node, and the
/// scrub basis cannot drift.
const DETAIL_TAG: &str = "detail_panel";
const THEME_TAG: &str = "app";

/// R1249 — the ONE shared inline numeric type-in editor hosted as an extra
/// `TextFieldExternal` (the property-grid `EDIT_TF` precedent). A
/// double-click on an `Int` / `Float` Details cell (or `invoke begin_edit
/// <i>`) seeds it with the property's current value and focuses it; `Enter`
/// commits the typed value across the whole selection, `Escape` cancels,
/// blur commits. Its state-scene registration turns the root scene into a
/// `Scene::Container([primary, edit_field])` — hence `read_state` /
/// `apply_key` walk via [`Scene::find_external_with_tag`], not a bare
/// `Scene::External` match.
const EDIT_TF_TAG: &str = "inspector_edit";
/// R1249 — the R793 commit-on-blur intent the editor raises when it loses
/// focus mid-edit; the `update` reducer drains it to a `commit_edit`. Built
/// through the drift-safe `intent_tag!` macro so the raise + drain spellings
/// cannot diverge.
const EDIT_TF_BLUR_INTENT_TAG: &str = pinion_core::intent_tag!("inspector_edit", "blur");

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
/// R1249 — the inline absolute type-in editor's field width (px), replacing the
/// stepper row on the row being edited. Sized to sit in the value column
/// alongside the reset gutter without overflowing [`DETAIL_ROW_W`].
const EDIT_FIELD_W: u32 = 110;
/// R1224 — the keyboard-cursor focus ring width (px) painted around the active
/// row of whichever pane currently owns the keyboard (the paint peer of the a11y
/// active-descendant `focused`).
const FOCUS_RING: u32 = 2;

/// R1373 — the Details panel width (px), the scrub's stable pixel basis. One
/// SSOT: the view sizes the [`DETAIL_TAG`] container to this and the scrub
/// multiplies the captured cursor fraction by it, so the drag distance is
/// measured against exactly the column it drags in (the property-grid
/// `GRID_W_PX` rule). `WIN_W - LIST_W - 40` = the row area minus the list column
/// and the inter-pane / edge gaps.
const DETAIL_PANEL_W: u32 = WIN_W - LIST_W - 40;
/// R1373 — this widget's numeric drag-scrub sensitivities: a `Float` moves 0.01
/// per pixel of cursor travel; an `Int` steps one whole unit every 8px. Applied
/// as `base + travel_px · sensitivity` per selected object. These match the
/// property-grid / data-grid values, but — per the [`DragCalibration`] contract
/// (value application stays with the caller, R935) — the sensitivity is this
/// widget's own tuning, NOT a shared invariant; a DCC panel is free to tune its
/// own feel, so the identical values are convention, not a lifted SSOT.
const SCRUB_FLOAT_PER_PX: f64 = 0.01;
const SCRUB_INT_PX_PER_STEP: f64 = 8.0;

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

/// R1249 — the common-property index currently being type-in edited (`None`
/// when the inline editor is closed). Owner-scoped like the object / selection /
/// focus models so the edit lifecycle ([`InspectorExternal::begin_edit`] /
/// [`commit_edit`] / [`cancel_edit`]) and the view / a11y all read one authority
/// — the property-grid `use_editing_row` precedent. The index addresses the
/// live common-property list (the same space `focus_property` / the value cells
/// use); a selection change that shrinks the list is handled by the
/// [`InspectorExternal::common`] bounds gate in `commit_edit`.
fn use_editing_prop() -> Rc<Signal<Option<usize>>> {
    let owner = Owner::current().expect("use_editing_prop requires an active Owner scope");
    owner.cache("inspector.editing_prop", || Signal::new(None))
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
/// R1225 — the Choice value-cell click prefix (`inspector#cycle<i>`): a click on
/// the enum cell advances it to the next option across the whole selection (the
/// segmented-control gesture, the keyboard `ArrowRight` twin).
const CYCLE_PREFIX: &str = "cycle";
/// R1249 — the `Int` / `Float` value-cell double-click prefix
/// (`inspector#typein<i>`): a double-click opens the inline absolute type-in
/// editor ([`EDIT_TF_TAG`]) on that row (the steppers stay for single-click).
/// Distinct from `inc`/`dec` so a stepper click and a cell double-click never
/// alias.
const TYPEIN_PREFIX: &str = "typein";
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

/// R1373 — the target the numeric scrub snapshots at its first captured
/// `pointer_move`: the scrubbed property's STABLE `name` plus each selected
/// object's base value. Keyed by name (not the selection-derived row index) so a
/// later move re-resolves the SAME property even if a mid-drag RPC `select`
/// reordered the `common_properties` list — the property-grid stable-`ValueRef`
/// discipline (R1372.2: never re-resolve a gesture target through a mutable
/// index). The cursor anchor + the `CellKind` ride the `Copy`
/// [`DragCalibration`] payload; the per-object bases do not fit a `Copy` payload
/// (a MULTI-object scrub), so they live here behind the `RefCell`.
#[derive(Clone)]
struct ScrubTarget {
    name: String,
    bases: Vec<(usize, CellValue)>,
}

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
    /// R1249 — the common-property index the inline type-in editor is open on
    /// (`None` = closed), captured from [`use_editing_prop`] so
    /// [`InspectorExternal::begin_edit`] can latch it from the RPC-invoke /
    /// pointer-send paths (which are not
    /// reliably inside an `Owner` scope — the property-grid captured-`Rc`
    /// precedent). The Owner cache makes it the SAME signal the view / a11y /
    /// free-fn `commit_edit` resolve.
    editing_prop: Rc<Signal<Option<usize>>>,
    /// R1249 — the shared inline editor's text buffer, captured from
    /// [`use_text_edit_state`] so `begin_edit` can seed it off-Owner.
    editor: Rc<TextEditState>,
    /// R1373 — the common-property row armed by a `PointerDown` over a numeric
    /// Details value cell (`inspector#typein<i>`), before the first captured
    /// `pointer_move` calibrates the drag. `None` for a press on a non-numeric
    /// cell — a Colour typein cell arms nothing, so its drag stays a click.
    scrub_armed: Cell<Option<usize>>,
    /// R1373 — the live scrub calibration ([`DragCalibration`], the already-lifted
    /// substrate — R935 ruled the per-widget scrub glue divergent, the basis /
    /// value application staying with the caller). Its `Copy` payload is the
    /// scrubbed [`CellKind`]; active between the first `pointer_move` and the
    /// release; its travel at `PointerUp` distinguishes a scrub (committed live)
    /// from a click.
    scrub_cal: DragCalibration<CellKind>,
    /// R1373 — the name-keyed target + per-object base snapshot captured at the
    /// scrub's first move ([`ScrubTarget`]). Each later move writes `base +
    /// travel` per object, so a mixed numeric stays mixed but shifts TOGETHER —
    /// the multi-object relative scrub (the divergence from the single-source
    /// property-grid / data-grid scrub, whose one base fits the `Copy` payload).
    /// `None` when no scrub is calibrated.
    scrub_target: RefCell<Option<ScrubTarget>>,
}

impl InspectorExternal {
    fn new(
        objects: Rc<Signal<Vec<ObjectData>>>,
        selection: Rc<Signal<VirtualSelect>>,
        defaults: Rc<Vec<ObjectData>>,
        focus: Rc<Signal<InspectorFocus>>,
        editing_prop: Rc<Signal<Option<usize>>>,
        editor: Rc<TextEditState>,
    ) -> Self {
        Self {
            objects,
            selection,
            defaults,
            focus,
            editing_prop,
            editor,
            scrub_armed: Cell::new(None),
            scrub_cal: DragCalibration::new(),
            scrub_target: RefCell::new(None),
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

    /// R1252 — the single selection-mutation chokepoint. Every selection change
    /// (select / toggle / extend / select-all / clear) routes here, and it
    /// **closes any open inline editor FIRST**. The editor's `editing_prop`
    /// index addresses the selection-DERIVED `common_properties` list, so a
    /// mid-edit selection change would silently retarget it to a *different*
    /// property (the R1252 wrong-property-write bug). The edit is semantically
    /// bound to a stable selection ("the common Layer of {Player, Camera}");
    /// changing the selection ends it. `false` = do not steal focus back (the
    /// selection gesture already moved it).
    fn mutate_selection(&self, f: impl FnOnce(&mut VirtualSelect)) {
        if self.editing_prop.get().is_some() {
            self.close_edit(false);
        }
        self.selection.set_with(move |prev| {
            let mut next = prev.clone();
            f(&mut next);
            next
        });
    }

    fn select(&self, index: usize) {
        self.mutate_selection(|s| {
            s.select(index);
        });
    }

    fn toggle(&self, index: usize) {
        self.mutate_selection(|s| {
            s.toggle(index);
        });
    }

    fn extend_to(&self, index: usize) {
        self.mutate_selection(|s| {
            s.extend_to(index);
        });
    }

    fn select_all(&self) {
        self.mutate_selection(|s| {
            s.select_all();
        });
    }

    fn clear(&self) {
        self.mutate_selection(|s| {
            s.clear();
        });
    }

    fn set_selection(&self, indices: BTreeSet<usize>) {
        self.mutate_selection(move |s| {
            s.set_selection(&indices);
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
        // R1249 — a DOUBLE-click on an Int/Float value cell (`inspector#typein<i>`)
        // opens the inline absolute type-in editor; a single click keeps hitting
        // the steppers. `DoubleClick` is not an activation event, so branch on it
        // before the `is_activation_event` gate below (the node-editor
        // double-click precedent). A non-numeric row's cell carries no `typein`
        // tag, so this never fires there.
        if event_name == "DoubleClick" {
            if let Some(idx) = prefixed_index(key, TYPEIN_PREFIX) {
                self.begin_edit(idx);
            }
            return;
        }
        // R1373 — a numeric Details value cell (`inspector#typein<i>`) is a
        // press-and-drag SCRUB target (the Blender / Unreal "drag the number"
        // gesture): a `PointerDown` arms it, the captured `pointer_move` drives it
        // across the whole selection (mix-preserving), and the release (or a
        // capture-stray `PointerLeave` / `PointerCancel`) tears it down. A real
        // scrub committed live, so its trailing click is suppressed — the typein
        // cell's only click action is `DoubleClick` (its own event), so nothing
        // further needs gating here. A Colour typein cell arms nothing
        // (`arm_scrub` declines a non-numeric row), so a Colour drag stays a click.
        if let Some(idx) = prefixed_index(key, TYPEIN_PREFIX) {
            match event_name {
                "PointerDown" => {
                    self.arm_scrub(idx);
                    return;
                }
                "PointerUp" | "PointerLeave" | "PointerCancel" => {
                    self.end_scrub();
                    return;
                }
                _ => {}
            }
        } else if matches!(
            event_name,
            "PointerDown" | "PointerUp" | "PointerLeave" | "PointerCancel"
        ) {
            // R1373 — a pointer press/release on any NON-scrub affordance abandons
            // an armed scrub. This defends the desync where an RPC `send
            // "typein<i>:PointerDown"` with no paired `PointerUp` leaks `scrub_armed`
            // (the RPC `send` path never enters the router, so no capture + no
            // release): the next REAL press elsewhere would otherwise have its
            // forwarded initial `pointer_move` seed a PHANTOM scrub off the leaked
            // arm. Clearing here (the router dispatches this `PointerDown` before it
            // forwards that initial move) makes a fresh gesture start clean. Falls
            // through: a `PointerUp` still needs its activation dispatch below.
            self.end_scrub();
        }
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
        if let Some(idx) = prefixed_index(key, CYCLE_PREFIX) {
            self.cycle_property(idx, 1);
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

    /// R1252 — the typed multi-object write SSOT: set the common property named
    /// `name` to `value` (an already-typed, already-validated [`CellValue`]) on
    /// EVERY selected object. Both the wire path
    /// ([`set_property`](Self::set_property), after `with_intervene` converts the
    /// `IntrospectValue`) and the type-in commit
    /// ([`commit_edit_text`](Self::commit_edit_text), after `CellKind` parses the
    /// text) funnel here — so the per-object shape gate + the R906 loud-fail live
    /// in ONE place, not three divergent copies. Under the common-property
    /// invariant every selected object shares the target's shape, so the gate
    /// never rejects; a rejection is a should-be-impossible bug (loud in dev, no
    /// silent clobber in release).
    fn set_property_value(&self, name: &str, value: &CellValue) {
        let value = value.clone();
        self.mutate_selected(name, move |_j, cur| {
            if same_property_shape(cur, &value) {
                Some(value.clone())
            } else {
                debug_assert!(
                    false,
                    "multi-write: the same-shape gate rejected a typed value \
                     (the common-property invariant was violated)"
                );
                None
            }
        });
    }

    fn set_property(&self, idx: usize, value: &IntrospectValue) -> Result<(), InterveneError> {
        let common = self.common();
        let target = common.get(idx).ok_or(InterveneError::UnknownPath)?;
        // Validate + convert the wire value ONCE against the representative. Every
        // selected object's matching property has the SAME shape
        // (`same_property_shape` — kind plus a Choice's option list), so the
        // typed value the representative yields is the verdict for the whole
        // write; a same-shape property cannot reject it.
        let typed = target.value.with_intervene(value.clone())?;
        self.set_property_value(&target.name.clone(), &typed);
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

    /// R1225 — cycle common **Choice** property `idx` by `dir` (prev/next option)
    /// on EVERY selected object, RELATIVELY and WRAPPING: each object advances its
    /// OWN selected index by `dir` modulo its option count. Like the numeric
    /// [`step_property`](Self::step_property) (the shared
    /// [`mutate_selected`](Self::mutate_selected) funnel, mix-PRESERVING) rather
    /// than the Bool toggle — a mixed enum stays
    /// mixed but rotates together, the multi-object segmented control. Every
    /// selected object's same-name Choice has the SAME options
    /// ([`same_property_shape`] gate), so the wrap bound is uniform.
    /// `false` when `idx` is out of range or the common property is not a Choice.
    fn cycle_property(&self, idx: usize, dir: i32) -> bool {
        let common = self.common();
        let Some(prop) = common.get(idx) else {
            return false;
        };
        if !matches!(prop.value, CellValue::Choice { .. }) {
            return false;
        }
        self.mutate_selected(&prop.name.clone(), move |_j, cur| match cur {
            CellValue::Choice { selected, options } if !options.is_empty() => {
                let len = i64::try_from(options.len()).ok()?;
                let next = usize::try_from(
                    (i64::try_from(*selected).ok()? + i64::from(dir)).rem_euclid(len),
                )
                .ok()?;
                Some(CellValue::Choice {
                    selected: next,
                    options: options.clone(),
                })
            }
            // Unreachable (common Choice everywhere) or empty-option Choice: no
            // change, never a silent clobber (R906).
            _ => None,
        });
        true
    }

    /// R1373 — arm a numeric scrub: a `PointerDown` over a numeric (`Int` /
    /// `Float`) Details value cell records the row so the first captured
    /// `pointer_move` can calibrate. A press on a non-numeric cell (a Colour
    /// typein) leaves the arm clear — it never scrubs. A fresh press starts a
    /// fresh calibration so a scrub never inherits a stale base from a drag whose
    /// release was missed (the R51.34 capture lock makes that unreachable, but
    /// the arm should not depend on it — the property-grid `arm_scrub` rule).
    fn arm_scrub(&self, row: usize) {
        self.scrub_cal.end();
        let numeric = self
            .common()
            .get(row)
            .is_some_and(|p| matches!(p.value, CellValue::Int(_) | CellValue::Float(_)));
        self.scrub_armed.set(numeric.then_some(row));
    }

    /// R1373 — drive the live numeric scrub from the captured cursor's horizontal
    /// fraction `x_rel` across the Details panel ([`DETAIL_TAG`]) through the
    /// already-lifted [`DragCalibration`] substrate. The FIRST move calibrates:
    /// `seed` snapshots the armed row's [`CellKind`] into the `Copy` payload and
    /// the STABLE property name + every selected object's base value into
    /// [`ScrubTarget`] (declining — `None` — if nothing is armed or the row is no
    /// longer numeric), and mutates nothing (the user has not dragged yet). Each
    /// LATER move yields the fraction delta, which `· DETAIL_PANEL_W` recovers as
    /// pixel travel; the scrub writes `base + travel · sensitivity` to EVERY
    /// selected object relative to its OWN snapshot base — so a mixed numeric
    /// stays mixed but shifts together (the multi-object relative scrub, the
    /// continuous peer of the discrete [`step_property`](Self::step_property)).
    ///
    /// The later moves re-resolve the target by the snapshot NAME, never the
    /// selection-derived row index: a mid-drag RPC `select` can reorder
    /// `common_properties` (the capture lock is pointer-routing only, so RPC is
    /// reachable between moves), and re-deriving through a stale row index is the
    /// R1372.2 stale-index hazard.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "scrub values are small game-object property magnitudes (layer / \
                  health / speed), nowhere near f64's 2^53 exact-int limit or i64's \
                  range; the f64->i64 step is an intentional round-to-unit"
    )]
    fn scrub_to(&self, x_rel: f64) {
        let Some((kind, delta)) = self.scrub_cal.drive(x_rel, || {
            let row = self.scrub_armed.get()?;
            let common = self.common();
            let prop = common.get(row)?;
            let kind = match prop.value {
                CellValue::Int(_) => CellKind::Int,
                CellValue::Float(_) => CellKind::Float,
                // Nothing armed to a numeric (e.g. the armed row changed shape) —
                // decline the drag.
                _ => return None,
            };
            // Snapshot the STABLE name + each selected object's base value, so every
            // later move writes `base + travel` (mix-PRESERVING) against a target
            // that a mid-drag selection change cannot silently retarget.
            let name = prop.name.clone();
            let objects = self.objects.get();
            let bases: Vec<(usize, CellValue)> = self
                .selection_set()
                .into_iter()
                .filter_map(|j| {
                    objects
                        .get(j)
                        .and_then(|o| o.properties.iter().find(|p| p.name == name))
                        .map(|p| (j, p.value.clone()))
                })
                .collect();
            *self.scrub_target.borrow_mut() = Some(ScrubTarget { name, bases });
            Some(kind)
        }) else {
            return;
        };
        // R915 dead zone — a sub-threshold press is a click, not a scrub: stay put
        // until the cursor strays past DRAG_CLICK_THRESHOLD_PX, so a plain click on
        // a numeric cell does not nudge its value.
        if !self.is_scrubbing() {
            return;
        }
        let travel_px = delta * f64::from(DETAIL_PANEL_W);
        let Some(ScrubTarget { name, bases }) = self.scrub_target.borrow().clone() else {
            return;
        };
        self.mutate_selected(&name, move |j, _cur| {
            // `base + travel` off THIS object's own press snapshot (mix-preserving);
            // an object dropped from the selection since the snapshot simply is not
            // iterated by `mutate_selected`, and one absent from `bases` (a mid-drag
            // extend) is left untouched — never a wrong-value write.
            let base = bases.iter().find(|(bj, _)| *bj == j).map(|(_, v)| v)?;
            match (kind, base) {
                (CellKind::Int, CellValue::Int(b)) => {
                    let steps = (travel_px / SCRUB_INT_PX_PER_STEP).round() as i64;
                    Some(CellValue::Int(b.saturating_add(steps)))
                }
                (CellKind::Float, CellValue::Float(b)) => {
                    Some(CellValue::Float(b + travel_px * SCRUB_FLOAT_PER_PX))
                }
                _ => None,
            }
        });
    }

    /// R1373 — tear the scrub down: clear the arm, end the calibration, drop the
    /// snapshot. Returns whether a REAL scrub ran (the cursor strayed past the
    /// click dead zone); the sibling grids read this to suppress the trailing
    /// click, but this binding's typein cell has no click action to suppress (its
    /// only click gesture is `DoubleClick`, a separate event), so the caller
    /// discards it — the return is kept for parity + the state-machine assertions
    /// in the tests (scrub-vs-click).
    fn end_scrub(&self) -> bool {
        self.scrub_armed.set(None);
        let was_scrub = self.is_scrubbing();
        self.scrub_cal.end();
        *self.scrub_target.borrow_mut() = None;
        was_scrub
    }

    /// R1373 — whether a *real* numeric scrub is live: the press has strayed past
    /// `DRAG_CLICK_THRESHOLD_PX` of travel across the Details-panel basis
    /// ([`DETAIL_PANEL_W`]). The one decision the scrub mutation gate, the
    /// click-suppression at release, and the AI-first `scrubbing` query share.
    /// A thin wrapper over the lifted [`DragCalibration::traveled_beyond`] SSOT —
    /// only the per-widget basis diverges (R935: the basis stays with the caller).
    fn is_scrubbing(&self) -> bool {
        self.scrub_cal
            .traveled_beyond(f64::from(DETAIL_PANEL_W), DRAG_CLICK_THRESHOLD_PX)
    }

    /// R1249/R1251 — open the inline ABSOLUTE type-in editor on common property
    /// `idx`. Gated to the field-editable kinds — `Int` / `Float` (R1249, the
    /// gap the steppers left) and `Color` (R1251, its `#RRGGBB` hex, the
    /// property-grid colour-popup's hex-field precedent). A `Bool` has its
    /// click-toggle, a `Choice` its click-cycle — those never open the field.
    /// Each field-editable kind rides the SAME editor: the [`CellKind`]
    /// keystroke gate accepts digits for a numeric and hex digits + `#` for a
    /// Colour, and [`CellKind::parse`] turns the committed text back into the
    /// typed value. Seeds the shared editor with the representative value's
    /// [`edit_text`](CellValue::edit_text) (a Colour seeds its hex; a mixed
    /// selection anchors on the first object's value — a concrete value to type
    /// over), latches `editing_prop`, and focuses the field. Uses the CAPTURED
    /// `Rc`s (not `use_*` hooks) so the RPC-invoke / double-click paths open it
    /// whether or not they run inside an `Owner` scope. Returns `false` (no-op)
    /// for a non-field-editable or out-of-range row.
    fn begin_edit(&self, idx: usize) -> bool {
        let common = self.common();
        let Some(prop) = common.get(idx) else {
            return false;
        };
        if !matches!(
            prop.value,
            CellValue::Int(_) | CellValue::Float(_) | CellValue::Color(_)
        ) {
            return false;
        }
        // `seed` = set_text + caret-at-end (the lifted TextEditState pair).
        self.editor.seed(prop.value.edit_text());
        self.editing_prop.set(Some(idx));
        // R1254 — the edit leads the Details cursor: place the keyboard cursor on
        // the edited row + focus the Details pane (`set_prop_cursor` does both).
        // So the editing textbox's a11y `focused` (sourced from `is_prop_cursor`)
        // matches the field's real focus, and a mouse/RPC-opened edit lands the
        // cursor where the user is typing (no divergence between the two rings).
        self.set_prop_cursor(Some(idx));
        pinion_core::focus_request::request(EDIT_TF_TAG);
        true
    }

    /// R1249/R1251 — parse `text` by the editing row's [`CellKind`] and write the
    /// parsed value ABSOLUTELY across the WHOLE selection through the typed
    /// [`set_property_value`](Self::set_property_value) SSOT (the same funnel the
    /// wire `intervene value.<i>` path reaches), then tear the editor down. A
    /// malformed commit keeps the prior value (the `parse` gate — no data loss).
    /// Returns `true` iff a value was written.
    ///
    /// R1251 — writes the PARSED [`CellValue`], NOT `parsed.to_introspect()`: a
    /// Colour's `to_introspect` is rich JSON while the write funnel takes the
    /// typed value directly (the read/write wire asymmetry is fixed in
    /// `cell_value.rs`, but the already-parsed value is the authority here — no
    /// round-trip through the wire forms). R1252 — the editor is closed on any
    /// selection change ([`mutate_selection`](Self::mutate_selection)), so
    /// `editing_prop` addresses the SAME `common_properties` list `begin_edit`
    /// seeded from; the index cannot have silently retargeted. The CAPTURED `Rc`s
    /// let the RPC-invoke path commit with no `Owner` scope.
    fn commit_edit_text(&self, text: &str, restore_focus: bool) -> bool {
        let write = self.editing_prop.get().and_then(|idx| {
            let common = self.common();
            let prop = common.get(idx)?;
            let parsed = prop.value.kind().parse(text)?;
            Some((prop.name.clone(), parsed))
        });
        let written = write.is_some();
        if let Some((name, parsed)) = write {
            self.set_property_value(&name, &parsed);
        }
        self.close_edit(restore_focus);
        written
    }

    /// R1249 — editor teardown SSOT: clear `editing_prop`, wipe the buffer so the
    /// next open starts from a fresh seed, and (on request) return focus to the
    /// inspector root. Captured-`Rc` based (no `Owner`), so the RPC `cancel_edit`
    /// verb reaches it. The `editing_prop` gate makes a post-commit blur a no-op.
    fn close_edit(&self, restore_focus: bool) {
        self.editing_prop.set(None);
        self.editor.set_text(String::new());
        if restore_focus {
            pinion_core::focus_request::request(INSPECTOR_TAG);
        }
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

// R913.1 / R1373 — the Gui+Rpc / Framework / UiThreadSync skeleton was the
// `query_proxy_external_impl!` config-holder SSOT. R1373 adds the numeric-scrub
// pointer capture (`wants_pointer_capture` / `capture_normalize` / `pointer_move`),
// which the macro does not express, so the impl is hand-written here — the
// property-grid / data-grid scrub precedent: a capture External is no longer a
// pure query proxy. The five skeleton methods stay byte-identical to the macro's;
// only the three capture methods are new.
impl External for InspectorExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// R1373 — opt into the R51.34 capture lock so a numeric scrub survives the
    /// cursor straying off the Details row (the property-grid / data-grid scrub
    /// stance). A press that never moves is still a click (the release dispatches
    /// `PointerUp` with no scrub calibrated, and a non-`typein` press seeds no
    /// scrub), so every CLICK path — object-select / stepper / toggle / cycle /
    /// reset — runs unchanged (pinned by the r909-r1251 demos + the tests).
    ///
    /// Capture is per-External, so it also spans the object `listbox`, not only
    /// the Details panel. This changes one PRESS-DRAG behavior there: an
    /// object-row press dragged onto a DIFFERENT row and released now selects the
    /// PRESS row (the capture lock delivers `PointerUp` to the press tag), where
    /// free-mode routed it to the release-hover row. There is no drag-select
    /// gesture, so this is the more intuitive "select what you pressed" — and it
    /// matches the sibling grids; pinned by
    /// `r1373_object_row_press_drag_selects_the_press_row`.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// R1373 — normalize the captured cursor against the Details panel
    /// ([`DETAIL_TAG`], a fixed-width rect), so the cursor-fraction delta recovers
    /// true pixel travel for the scrub (the property-grid stable-basis rule — the
    /// scrubbed row never resizes, so the whole panel is a fine basis).
    fn capture_normalize(&self) -> CaptureNormalize<'_> {
        CaptureNormalize::Tag(DETAIL_TAG)
    }

    /// R1373 — drive the live numeric scrub from the captured cursor's horizontal
    /// fraction across the Details panel; `y_rel` is ignored (scrub is the X axis).
    fn pointer_move(&mut self, x_rel: f32, _y_rel: f32) {
        self.scrub_to(f64::from(x_rel));
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for InspectorExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("object_count", "int"),
                    SchemaField::new("selected", "int"),
                    SchemaField::new("selection", "json"),
                    SchemaField::new("selection_count", "int"),
                    SchemaField::new("selection_summary", "string"),
                    SchemaField::new("mode", "string"),
                    SchemaField::new("row_count", "int"),
                    SchemaField::parametric(
                        "object_name.<j>",
                        "string",
                        const { &[SchemaArg::index("j", "object_count")] },
                    ),
                    SchemaField::parametric(
                        "name.<i>",
                        "string",
                        const { &[SchemaArg::index("i", "row_count")] },
                    ),
                    SchemaField::parametric(
                        "kind.<i>",
                        "string",
                        const { &[SchemaArg::index("i", "row_count")] },
                    ),
                    SchemaField::parametric(
                        "value.<i>",
                        "json",
                        const { &[SchemaArg::index("i", "row_count")] },
                    ),
                    SchemaField::parametric(
                        "mixed.<i>",
                        "bool",
                        const { &[SchemaArg::index("i", "row_count")] },
                    ),
                    SchemaField::parametric(
                        "modified.<i>",
                        "bool",
                        const { &[SchemaArg::index("i", "row_count")] },
                    ),
                    SchemaField::new("any_modified", "bool"),
                    SchemaField::new("select", "int"),
                    SchemaField::new("toggle", "int"),
                    SchemaField::new("extend_to", "int"),
                    SchemaField::new("select_all", "null"),
                    SchemaField::new("clear", "null"),
                    SchemaField::new("send", "string"),
                    SchemaField::new("reset", "int"),
                    SchemaField::new("reset_all", "null"),
                    // R1221 — the Details inline-edit verbs (the AI-first peers of the
                    // value-cell click gestures): flip a common Bool across the whole
                    // selection, or step a common numeric (arg "<i>,<dir>", dir a signed
                    // unit count). Both write across every selected object.
                    SchemaField::new("toggle_property", "int"),
                    SchemaField::new("step_property", "string"),
                    // R1225 — cycle a common Choice across the selection (arg "<i>,<dir>",
                    // the enum peer of step_property; the value-cell click twin).
                    SchemaField::new("cycle_property", "string"),
                    // R1224 — the keyboard-focus surface (§2 #2: the region + property
                    // cursor the Arrow keys drive are observable + drivable over RPC).
                    // `focus_region` reads/sets which pane owns the cursor ("objects" /
                    // "details"); `prop_cursor` reads the active Details row (clamped);
                    // `focus_property` places the cursor at a row (and focuses Details).
                    SchemaField::new("focus_region", "string"),
                    SchemaField::new("prop_cursor", "int"),
                    SchemaField::new("focus_property", "int"),
                    // R1249 — the inline absolute type-in editor surface (§2 #2: the
                    // editor's open row + live buffer are observable, and the AI drives
                    // the whole edit over RPC without char-by-char keys). `editing` reads
                    // the common-property index the field is open on (Null when closed);
                    // `edit_text` reads the live buffer. `begin_edit` opens it on a
                    // numeric row, `commit_edit` writes the wire text across the selection
                    // and closes, `cancel_edit` closes without writing.
                    SchemaField::new("editing", "int"),
                    SchemaField::new("edit_text", "string"),
                    SchemaField::new("begin_edit", "int"),
                    SchemaField::new("commit_edit", "string"),
                    SchemaField::new("cancel_edit", "null"),
                    // R1373 — the numeric-scrub live state (§2 #2: the AI reads
                    // whether a press-drag is mid-scrub, the property-grid /
                    // data-grid `scrubbing` peer). The scrub itself is driven by
                    // the deterministic `scene/drag` RPC (press + captured moves +
                    // release under the capture lock), not a bespoke verb.
                    SchemaField::new("scrubbing", "bool"),
                ]
            },
        )
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
            // R1249 — the inline editor's open row (Null when closed) + live
            // buffer, the read half of the type-in surface.
            "editing" => Some(selected_to_value(self.editing_prop.get())),
            "edit_text" => Some(IntrospectValue::Text(self.editor.text())),
            // R1373 — is a numeric press-drag mid-scrub (past the click dead zone)?
            "scrubbing" => Some(IntrospectValue::Bool(self.is_scrubbing())),
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
                    // R1373 — a schema-declared but non-intervene path (`scrubbing`,
                    // `editing`, `edit_text`, `prop_cursor`, `any_modified`, the
                    // invoke verbs) is `ReadOnly`, not `UnknownPath`: an agent that
                    // can plainly `query` it must not be told it does not exist
                    // (§2 #7; the `read_only_or_unknown` SSOT). Truly-unknown paths
                    // still return `UnknownPath`.
                    return Err(read_only_or_unknown(&self.schema(), path));
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
            // R1225 — cycle a common Choice across the selection by a signed unit
            // count (the enum segmented-control twin). Arg `"<i>,<dir>"`; `false`
            // when `idx` is not a common Choice row.
            "cycle_property" => match args {
                IntrospectValue::Text(spec) => {
                    let (idx, dir) = parse_step_spec(&spec).ok_or(InvokeError::Rejected)?;
                    Ok(IntrospectValue::Bool(self.cycle_property(idx, dir)))
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
            // R1249 — open the inline ABSOLUTE type-in editor on common property
            // `idx` (the double-click twin). `false` for a non-numeric / out-of-
            // range row (a benign miss). Seeds the field with the current value
            // and focuses it — the AI then drives `commit_edit`/`cancel_edit`.
            "begin_edit" => {
                let idx = int_arg(args)?;
                Ok(IntrospectValue::Bool(self.begin_edit(idx)))
            }
            // R1249 — commit a typed ABSOLUTE value across the whole selection +
            // close (the `Enter` twin). Arg = the wire text; parsed by the editing
            // row's kind (a malformed numeric keeps the prior value). `true` iff a
            // value was written; `false` when no editor is open or the parse fails.
            "commit_edit" => match args {
                IntrospectValue::Text(text) => {
                    Ok(IntrospectValue::Bool(self.commit_edit_text(&text, true)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R1249 — close the editor without writing (the `Escape` twin).
            "cancel_edit" => {
                self.close_edit(true);
                Ok(IntrospectValue::Null)
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
        use_editing_prop(),
        use_text_edit_state(EDIT_TF_TAG),
    )
}

/// R1249 — the sibling `External`s the `#[widget(extra_externals = ...)]`
/// attribute registers alongside the primary [`InspectorExternal`]: the ONE
/// shared inline numeric type-in editor. R1250 — the registration is the lifted
/// [`blur_committing_field_extra`] SSOT (the `TextFieldExternal` sharing the
/// Owner-cached `TextEditState` + `CaretBlink` the view fn ([`detail_value_cell`])
/// and the free-fn edit lifecycle read, with the R793 commit-on-blur intent the
/// `update` reducer drains). Runs in the root Owner scope. The registration
/// reshapes the state scene into a `Scene::Container([inspector, inspector_edit])`.
fn make_inspector_extras() -> Vec<ExtraExternal> {
    vec![blur_committing_field_extra(EDIT_TF_TAG)]
}

// ─── Inline type-in edit lifecycle (R1249) ────────────────────────
//
// `begin_edit` is an `InspectorExternal` method (its callers — RPC invoke +
// double-click send — hold `&self`). `commit` / `cancel` are free fns: the
// commit-on-blur path (the `update` reducer) has neither `&self` nor the scene,
// so it resolves the Owner-cached signals through `use_*` hooks + rebuilds the
// write handle via `make_inspector_external()` (the same signals `begin_edit`
// captured), reusing the audit-cleared `set_property` multi-object funnel
// without a second copy of the write scaffold.

/// R1249 — the keyboard/blur commit: read the LIVE editor buffer and commit it
/// across the selection through the [`InspectorExternal::commit_edit_text`]
/// write SSOT. Reached from the Owner-scoped `apply_key` (`Enter`) + `update`
/// (blur) paths, so it rebuilds the handle over the same Owner-cached signals.
/// `restore_focus` returns focus to the root on `Enter`; a blur passes `false`
/// (the click already moved focus).
fn commit_edit(restore_focus: bool) {
    let text = use_text_edit_state(EDIT_TF_TAG).text();
    make_inspector_external().commit_edit_text(&text, restore_focus);
}

/// R1249 — the keyboard cancel (`Escape`): leave every selected object's value
/// untouched, close, restore focus. Delegates to the
/// [`InspectorExternal::close_edit`] teardown SSOT.
fn cancel_edit() {
    make_inspector_external().close_edit(true);
}

/// R1249 — the [`CellKind`] of the row the inline editor is open on (`None`
/// when closed), the int/float keystroke gate [`edit_field_keymap`] consults.
fn editing_kind() -> Option<CellKind> {
    let idx = use_editing_prop().get()?;
    make_inspector_external()
        .common()
        .get(idx)
        .map(|p| p.value.kind())
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
#[derive(Clone, Copy, Debug, PartialEq)]
struct InspectorState {
    selected: [bool; N_OBJECTS],
    cursor: Option<usize>,
    /// R1224 — which pane owns the keyboard cursor (default [`FocusRegion::Objects`]).
    region: FocusRegion,
    /// R1224 — the active Details property row (already clamped by the External),
    /// the a11y active-descendant + keyboard-edit target when `region` is Details.
    prop_cursor: Option<usize>,
    /// R1249 — the inline numeric type-in editor's paint posture (interaction
    /// state + caret byte), read off the sibling `inspector_edit` External in
    /// `read_state`. Consumed by [`detail_value_cell`] / [`detail_access_node`]
    /// only for the row equal to [`use_editing_prop`]; `(Idle, 0)` otherwise.
    /// [`TextFieldState`] is not `Default`, so [`InspectorState`]'s `Default` is
    /// hand-written below rather than derived.
    edit_field: (TextFieldState, u32),
}

impl Default for InspectorState {
    fn default() -> Self {
        Self {
            selected: [false; N_OBJECTS],
            cursor: None,
            region: FocusRegion::default(),
            prop_cursor: None,
            edit_field: (TextFieldState::Idle, 0),
        }
    }
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

    /// R1249 — layer the inline editor's paint posture (read off the sibling
    /// `inspector_edit` External) onto the state; the many selection-only
    /// `from_parts` call sites stay `(Idle, 0)` (the [`Default`] editor field).
    fn with_edit_field(mut self, field: (TextFieldState, u32)) -> Self {
        self.edit_field = field;
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

/// R1251 — a read-only value visual (Bool pill / Choice / Colour swatch)
/// wrapped in a click / double-click target `tag` with the shared value-cell
/// layout. The Bool (`inspector#toggle<i>`), Choice (`inspector#cycle<i>`), and
/// Colour (`inspector#typein<i>`) cells were three byte-identical container
/// wraps differing only in the tag — lifted here (R727/R732 3b, the 3rd site).
fn tagged_value_cell(
    prop: &CommonProperty,
    fg: Color,
    accent: Color,
    muted: Color,
    tag: String,
) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![detail_value_visual(prop, fg, accent, muted)])
            .with_tag(tag)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_margin(Rect::new(0, 0, RESET_GUTTER, 0)),
            ),
    )
}

/// R1221 — the interactive Details value cell. A **Bool** paints its pill (or
/// the mixed placeholder) inside a `inspector#toggle<i>` click target (flip
/// across the whole selection, resolving a mixed Bool to uniform); an **Int** /
/// **Float** paints `[-] value [+]` steppers (`inspector#dec<i>` /
/// `inspector#inc<i>`, a mix-preserving relative shift) wrapped in a
/// `inspector#typein<i>` double-click target (R1249 — a double-click opens the
/// inline absolute type-in editor). R1251 — a **Colour** cell is likewise a
/// `inspector#typein<i>` double-click target (its swatch + hex over the hex
/// editor). A **Choice** keeps its `inspector#cycle<i>` click target (R1225); a
/// **Text** keeps the read-only [`detail_value_visual`] display (no shared Text
/// property in the model — still RPC-editable via `intervene value.<i>`).
///
/// R1249/R1251 — when `editing` (this row is the one [`use_editing_prop`] points
/// at) the Int/Float/Colour arm paints the shared inline [`EDIT_TF_TAG`] field in
/// place of the steppers / swatch, seeded + focused by
/// [`InspectorExternal::begin_edit`]. Colours derive from `theme` so the value
/// column and the field share one palette.
fn detail_value_cell(
    index: usize,
    prop: &CommonProperty,
    theme: &Theme,
    editing: bool,
    edit_field: (TextFieldState, u32),
) -> Scene {
    let fg = theme.resolve(ColorRole::OnSurface);
    let accent = theme.resolve(ColorRole::Accent);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    match prop.value {
        CellValue::Bool(_) => tagged_value_cell(
            prop,
            fg,
            accent,
            muted,
            format!("{INSPECTOR_TAG}#{TOGGLE_PREFIX}{index}"),
        ),
        CellValue::Int(_) | CellValue::Float(_) | CellValue::Color(_) if editing => {
            // R1249/R1251 — the inline absolute type-in editor replaces the
            // steppers (numeric) / swatch (colour) on the edited row. `view_field`
            // tags its node `EDIT_TF_TAG`, so the router routes clicks / focus to
            // the sibling `inspector_edit` External. The reset gutter is preserved
            // so a modified row's reset dot still clears the field.
            let style = tf_paint::TextFieldStyle {
                field_w: EDIT_FIELD_W,
                field_h: ROW_H - 8,
                ..tf_paint::TextFieldStyle::m3_filled()
            };
            Scene::Container(
                ContainerNode::new(vec![tf_paint::view_field(
                    EDIT_TF_TAG,
                    edit_field.0,
                    edit_field.1,
                    theme,
                    &style,
                    "",
                )])
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_margin(Rect::new(0, 0, RESET_GUTTER, 0)),
                ),
            )
        }
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
                // R1249 — a double-click anywhere on the numeric cell (the label
                // between the steppers) opens the type-in editor; single-click
                // still hits the steppers (their own deeper tags win the hit-test).
                .with_tag(format!("{INSPECTOR_TAG}#{TYPEIN_PREFIX}{index}"))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_gap(6)
                        .with_margin(Rect::new(0, 0, RESET_GUTTER, 0)),
                ),
            )
        }
        CellValue::Choice { .. } => tagged_value_cell(
            prop,
            fg,
            accent,
            muted,
            format!("{INSPECTOR_TAG}#{CYCLE_PREFIX}{index}"),
        ),
        // R1251 — a Colour cell paints its swatch + hex, wrapped in a
        // `inspector#typein<i>` double-click target that opens the same inline
        // editor to type a new `#RRGGBB` hex (the property-grid colour hex-field,
        // without the swatch popup — the type-in is the inspector's affordance).
        CellValue::Color(_) => tagged_value_cell(
            prop,
            fg,
            accent,
            muted,
            format!("{INSPECTOR_TAG}#{TYPEIN_PREFIX}{index}"),
        ),
        // A Text cell keeps the read-only display (no shared Text property in the
        // model; RPC-editable via `intervene value.<i>`).
        CellValue::Text(_) => detail_value_visual(prop, fg, accent, muted),
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
    theme: &Theme,
    editing: bool,
    edit_field: (TextFieldState, u32),
) -> Scene {
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let accent = theme.resolve(ColorRole::Accent);
    let name = Scene::Text(TextNode::styled(
        prop.name.clone(),
        Rect::default(),
        TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(muted),
    ));
    let mut children = vec![
        name,
        detail_value_cell(index, prop, theme, editing, edit_field),
    ];
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
    // R1249 — the common-property index the inline editor is open on; the row
    // equal to it paints the field instead of the steppers.
    let editing_prop = use_editing_prop().get();
    for (i, prop) in common_properties(&objects, &selection).iter().enumerate() {
        let modified = property_modified_from_default(&objects, &selection, &defaults, &prop.name);
        detail_children.push(property_row(
            i,
            prop,
            modified,
            state.is_prop_cursor(i),
            &theme,
            editing_prop == Some(i),
            state.edit_field,
        ));
    }
    let detail = Scene::Container(
        ContainerNode::new(detail_children)
            .with_tag(DETAIL_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(6)
                    .with_size(Size::px(DETAIL_PANEL_W, WIN_H - 70)),
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
    // R1249 — host the shared inline numeric type-in editor as a sibling
    // External (the state scene becomes a Container); `read_state` / `apply_key`
    // walk it via `find_external_with_tag`.
    extra_externals = make_inspector_extras,
    a11y_manual,
    apply_key,
    // R1249 — drain the editor's R793 commit-on-blur intent.
    update,
)]
struct InspectorView;

impl InspectorView {
    /// Read the selection (set + cursor) off the primary External's introspect,
    /// reusing the canonical [`read_selection`] / [`read_selected`] decoders.
    fn read_state(scene: &Scene) -> InspectorState {
        // R1249 — the state scene is a `Container([inspector, inspector_edit])`
        // now that the inline editor is a sibling External, so walk to the
        // primary by tag rather than matching `Scene::External` directly (which
        // would silently read the default state off the container — the R832
        // multi-External no-op). The inline editor's own paint posture (caret +
        // interaction) is read off the sibling for the editing value cell.
        if let Some(node) = scene.find_external_with_tag(INSPECTOR_TAG) {
            if let Some(intro) = node.handle.introspect() {
                return InspectorState::from_parts(&read_selection(intro), read_selected(intro))
                    .with_focus(read_focus_region(intro), read_prop_cursor(intro))
                    .with_edit_field(tf_paint::read_text_field_state(scene, EDIT_TF_TAG));
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

    /// R1249 §5.38 — commit-on-blur: the inline numeric editor lost focus (a
    /// click elsewhere) mid-edit → commit the typed value without restoring
    /// focus (the click already moved it). Gated on an open editor so the
    /// post-commit blur (focus already restored to the root) is a no-op — only a
    /// genuine click-away commits (the R793 discipline). Runs in the root Owner
    /// scope (`CoreShell` wraps `update`), so the `use_*` hooks resolve.
    fn update(_state: InspectorState, intent: &pinion_core::Intent) -> Vec<Command> {
        if intent.tag_str() == EDIT_TF_BLUR_INTENT_TAG && use_editing_prop().get().is_some() {
            commit_edit(false);
        }
        Vec::new()
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
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        // R1249 — while the inline numeric editor owns focus it owns the keys:
        // `Enter` commits the typed value across the selection, `Escape` cancels,
        // the caret / deletion keys forward to the field, and a non-numeric
        // keystroke is rejected at the `CellKind` gate (the shared
        // `edit_field_keymap` SSOT). Must run before the primary is borrowed —
        // the keymap forwards into the sibling field via `&mut scene`.
        if focused == Some(EDIT_TF_TAG) {
            let kind = editing_kind().unwrap_or(CellKind::Float);
            return edit_field_keymap(
                scene,
                EDIT_TF_TAG,
                key,
                modifiers,
                kind,
                || commit_edit(true),
                cancel_edit,
            );
        }

        // R1249 — the state scene is a `Container`; walk to the primary by tag.
        let Some(node) = scene.find_external_with_tag_mut(INSPECTOR_TAG) else {
            return false;
        };

        // R1228 (B3) — AT-driven Details edit. The shell routes an AT
        // Increment / Decrement / Click on an operable Details row (switch /
        // spinbutton) here as `apply_key(Some("prop_<i>"), "ArrowRight" /
        // "ArrowLeft" / "Enter")`. Operate THAT row by kind, independent of the
        // pointer cursor / region — so a screen reader drives a cell exactly as
        // the pointer / keyboard / RPC do (the fix for the "operable role is a
        // facade for AT" gap). A `prop_<i>` tag never reaches here from ordinary
        // keyboard (the Details rows are not focus stops — only the root is), so
        // this gate is AT-exclusive. The AT-targeted row is also focused, so the
        // paint ring + a11y active-descendant follow the AT action.
        if let Some(idx) = focused
            .and_then(|t| t.strip_prefix("prop_"))
            .and_then(|s| s.parse::<usize>().ok())
        {
            let kind = node.handle.introspect().and_then(|i| read_kind_at(i, idx));
            let Some(intro) = node.handle.introspect_mut() else {
                return false;
            };
            if let Ok(i) = i64::try_from(idx) {
                let _ = intro.invoke("focus_property", IntrospectValue::Int(i));
            }
            return edit_property_at(intro, idx, kind.as_deref(), key);
        }

        // Snapshot everything the dispatch reads, releasing the immutable borrow
        // before any `introspect_mut` mutation below.
        let (region, obj_count, obj_cursor, row_count, prop_cursor, cursor_kind) = {
            let Some(intro) = node.handle.introspect() else {
                return false;
            };
            let prop_cursor = read_prop_cursor(intro);
            (
                read_focus_region(intro),
                read_count(intro),
                read_selected(intro),
                read_row_count(intro),
                prop_cursor,
                // R1225 — the cursor cell's kind, so ArrowLeft/Right routes to the
                // right edit (a numeric steps, a Choice cycles).
                prop_cursor.and_then(|i| read_kind_at(intro, i)),
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
            // Value edits at the cursor, through the shared `edit_property_at`
            // funnel (the SAME dispatch the AT path above uses). A cursor-less
            // panel (empty selection) leaves every edit a benign no-op.
            let Some(cursor) = prop_cursor else {
                return false;
            };
            let Some(intro) = node.handle.introspect_mut() else {
                return false;
            };
            return edit_property_at(intro, cursor, cursor_kind.as_deref(), key);
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

/// R1225 — the kind token of common property `i` off the `kind.<i>` query
/// (`"int"` / `"float"` / `"choice"` / …), so the keyboard routes ArrowLeft/Right
/// to the kind-appropriate edit; `None` when the row is absent.
fn read_kind_at(intro: &dyn ExternalIntrospect, i: usize) -> Option<String> {
    match intro.query(&format!("kind.{i}")) {
        Some(IntrospectValue::Text(k)) => Some(k),
        _ => None,
    }
}

/// R1228 — dispatch a value-edit key onto Details property `idx` by its `kind`,
/// through the shared verb funnel: `Space`/`Enter` toggle a Bool,
/// `ArrowRight`/`+` and `ArrowLeft`/`-` step a numeric while `ArrowRight`/`ArrowLeft`
/// cycle a Choice, `Delete`/`Backspace` reset. The ONE edit dispatch shared by BOTH
/// the keyboard property-cursor path AND the AT operable-role path
/// (`apply_key(Some("prop_<i>"), …)`), so a screen reader operates a cell exactly
/// as the pointer / keyboard / RPC do — the fix for the R1224/R1225 "operable role
/// is a facade for AT" gap. Returns whether a value-edit key was recognised.
fn edit_property_at(
    intro: &mut dyn ExternalIntrospect,
    idx: usize,
    kind: Option<&str>,
    key: &str,
) -> bool {
    let Ok(i) = i64::try_from(idx) else {
        return false;
    };
    let is_choice = kind == Some("choice");
    // R1254 — the kinds the inline type-in editor opens on. `F2` (the universal
    // "edit cell" key) or `Enter` on such a row opens the editor — the KEYBOARD
    // and AT open path the R1249/R1251 type-in previously lacked (it was
    // mouse-double-click + RPC only). `edit_property_at` is the shared dispatch
    // for BOTH the Details-cursor keyboard funnel AND the R1228 AT `prop_<i>`
    // route, so this one arm gives a screen-reader / keyboard user the editor —
    // and, for a Colour, its ONLY non-mouse edit (step/toggle no-op on a Colour).
    let is_field_editable = matches!(kind, Some("int" | "float" | "color"));
    let spec = |dir: i32| IntrospectValue::Text(format!("{idx},{dir}"));
    let (verb, arg) = match key {
        // `F2` (universal edit key) or `Enter` on a field-editable row opens the
        // editor. `Enter` on a Bool falls through to toggle (below) — on a numeric
        // / colour `Enter` was a dead no-op (toggle_property short-circuits on a
        // non-Bool), so opening the editor there repurposes a wasted key.
        "F2" | "Enter" if is_field_editable => ("begin_edit", IntrospectValue::Int(i)),
        " " | "Enter" => ("toggle_property", IntrospectValue::Int(i)),
        "ArrowRight" if is_choice => ("cycle_property", spec(1)),
        "ArrowLeft" if is_choice => ("cycle_property", spec(-1)),
        "ArrowRight" | "+" => ("step_property", spec(1)),
        "ArrowLeft" | "-" => ("step_property", spec(-1)),
        "Delete" | "Backspace" => ("reset", IntrospectValue::Int(i)),
        _ => return false,
    };
    let _ = intro.invoke(verb, arg);
    true
}

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
    fn access_node(state: &InspectorState, focused: Option<&str>) -> Vec<AccessNode> {
        let objects = use_objects().get();
        // R1517 §5.39 §5.40 — the pane regions below are the binding's INTERNAL
        // keyboard split; the shell sees one focus stop. `aria-activedescendant`
        // is defined only while that stop owns focus, so both roving cursors are
        // gated on it. Ungated (measured before this round) object row 0 claimed
        // AT focus at boot, with the focus manager holding nothing at all.
        let composite_focused = focused == Some(INSPECTOR_TAG);
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
                // the Objects pane owns the keyboard (region-gated roving focus),
                // and R1517 — only while the binding owns the shell's focus.
                focused: composite_focused && state.is_object_cursor(i),
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
        // R1249 — the row being type-in edited: its AT node is the inline field's
        // `textbox` (tagged `EDIT_TF_TAG`, resolving to the painted field bounds)
        // in place of the row's `spinbutton`, so the Details group references
        // that tag as its child for the edited row.
        let editing_prop = use_editing_prop().get();
        let edit_text = use_text_edit_state(EDIT_TF_TAG).text();
        let mut panel = AccessNode::new(DETAIL_TAG, AriaRole::Group)
            .with_name(selection_summary(&objects, &selection));
        for i in 0..rows.len() {
            if editing_prop == Some(i) {
                panel = panel.with_child(EDIT_TF_TAG.to_owned());
            } else {
                panel = panel.with_child(format!("prop_{i}"));
            }
        }
        nodes.push(panel);
        for (i, prop) in rows.iter().enumerate() {
            let editing =
                (editing_prop == Some(i)).then(|| (edit_text.clone(), state.edit_field.0));
            nodes.push(detail_access_node(
                i,
                prop,
                composite_focused && state.is_prop_cursor(i),
                editing,
            ));
        }
        nodes
    }
}

/// R1224 — the a11y node for one Details property row, its operable WAI-ARIA
/// role matching the interactive paint cell so an AT drives it the way the
/// pointer / keyboard do:
///
/// - **Bool** → tri-state `checkbox` (R1229): uniform → `aria-checked` true/false
///   (`with_value(AccessValue::Bool)`); mixed (the selected objects disagree) →
///   `aria-checked="mixed"` (`with_mixed`), the standard indeterminate
///   multi-object boolean. NOT a `switch` — a switch is two-state and cannot be
///   indeterminate (the R1224 mixed `switch` emitted no `aria-checked` at all).
/// - **Int / Float** → `spinbutton` with the numeric display as
///   `aria-valuetext` (the values carry no declared min/max, so a bare
///   value-text is the faithful reading — the AT still exposes Increment /
///   Decrement actions from the role).
/// - **Int / Float / Choice** → `spinbutton` carrying the value as
///   `aria-valuetext` (`"Multiple Values"` when mixed). R1228 — a Choice is a
///   spinbutton too (NOT the R1225 `combobox`: a WAI-ARIA combobox REQUIRES
///   `aria-expanded` + a controlled popup, which this in-place `ArrowLeft` /
///   `ArrowRight` cycle cell has NOT); its AT Increment / Decrement map to the
///   arrow-cycle exactly as a numeric's do.
/// - **Color / Text** → an informational `listitem` named `"{name}: {value}"`
///   (read-only over RPC via `intervene value.<i>`).
///
/// `focused` sets the active-descendant flag (the Details pane owns the
/// keyboard and this is the property cursor), the a11y peer of the painted ring.
/// The operable roles are genuinely AT-operable: the shell routes an AT
/// Increment / Decrement / Click on `prop_<i>` to `apply_key(Some("prop_<i>"), …)`,
/// which `edit_property_at` dispatches to the same verb (R1228 B3).
fn detail_access_node(
    index: usize,
    prop: &CommonProperty,
    focused: bool,
    editing: Option<(String, TextFieldState)>,
) -> AccessNode {
    // R1249 — while type-in editing, the row's operable role yields to the
    // inline field's `textbox` (the lifted `text_field_a11y_node` SSOT), tagged
    // `EDIT_TF_TAG` so it resolves to the painted field bounds and the AT reads
    // the live buffer. The spinbutton returns once the edit commits / cancels —
    // the paint==a11y one-gate, both keyed off `use_editing_prop`.
    if let Some((text, posture)) = editing {
        return tf_paint::text_field_a11y_node(EDIT_TF_TAG, text, posture, focused)
            .with_name(prop.name.clone());
    }
    let tag = format!("prop_{index}");
    match prop.value {
        CellValue::Bool(b) => {
            // R1229 — a multi-object boolean is a tri-state `checkbox`, NOT a
            // `switch`: a switch is two-state and cannot be indeterminate. Uniform
            // → `aria-checked` true/false; mixed (the members disagree) →
            // `aria-checked="mixed"` via `with_mixed` — the standard, and the fix
            // for the R1224 invalid "switch with no aria-checked" (B2).
            let node = AccessNode::new(tag, AriaRole::CheckBox)
                .with_name(prop.name.clone())
                .with_focused(focused);
            if prop.mixed {
                node.with_mixed()
            } else {
                node.with_value(AccessValue::Bool(b))
            }
        }
        // R1228 — numeric AND Choice are both `spinbutton`s (arrow-cycle =
        // Increment/Decrement); one arm, no divergence.
        CellValue::Int(_) | CellValue::Float(_) | CellValue::Choice { .. } => {
            AccessNode::new(tag, AriaRole::SpinButton)
                .with_name(prop.name.clone())
                .with_value_text(common_value_label(prop))
                .with_focused(focused)
        }
        // R1254 — a Colour cell is a `textbox` (its `#RRGGBB` hex value): the AT
        // reads the value AND can open the inline hex editor by activating it (the
        // R1228 AT-Click -> "Enter" route reaches `edit_property_at` -> `begin_edit`
        // for a field-editable kind). This is the AT/keyboard open path the R1251
        // Colour cell lacked — step / toggle no-op on a Colour, so a `listitem`
        // left it with NO non-mouse edit. Not a facade: the AT genuinely operates
        // it (Enter opens the field). A Text cell (no shared inline field) stays a
        // read-only `listitem`.
        CellValue::Color(_) => AccessNode::new(tag, AriaRole::TextInput)
            .with_name(prop.name.clone())
            .with_value(AccessValue::Text(common_value_label(prop)))
            .with_focused(focused),
        CellValue::Text(_) => AccessNode::new(tag, AriaRole::ListItem)
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

    // ─── R1373 numeric drag-scrub ─────────────────────────────────

    /// The cursor fraction across the Details panel that a `travel_px` pixel drag
    /// produces (the scrub's inverse basis), so the tests read in pixels and stay
    /// basis-agnostic if [`DETAIL_PANEL_W`] changes.
    fn frac(travel_px: f64) -> f64 {
        travel_px / f64::from(DETAIL_PANEL_W)
    }

    /// The scrub arms only on a numeric cell; a Bool / Choice / Colour / out-of-
    /// range press leaves the arm clear (those never scrub).
    #[test]
    fn r1373_scrub_arms_only_a_numeric_row() {
        let e = ext();
        e.select(0); // Player: Visible/Layer/Locked/Health/Speed/Team/Tint
        e.arm_scrub(4); // Speed (Float)
        assert_eq!(e.scrub_armed.get(), Some(4), "a Float cell arms");
        e.arm_scrub(1); // Layer (Int)
        assert_eq!(e.scrub_armed.get(), Some(1), "an Int cell arms");
        e.arm_scrub(0); // Visible (Bool)
        assert_eq!(e.scrub_armed.get(), None, "a Bool cell does not arm");
        e.arm_scrub(5); // Team (Choice)
        assert_eq!(e.scrub_armed.get(), None, "a Choice cell does not arm");
        e.arm_scrub(6); // Tint (Colour)
        assert_eq!(e.scrub_armed.get(), None, "a Colour cell does not arm");
        e.arm_scrub(99); // out of range
        assert_eq!(
            e.scrub_armed.get(),
            None,
            "an out-of-range row does not arm"
        );
    }

    /// The headline gesture: a scrub over a MULTI-object selection writes
    /// `base + travel` to every selected object relative to its OWN press value,
    /// so a mixed numeric stays mixed but shifts together (the continuous peer of
    /// the discrete `step_property`). Layer is Player 1 / Camera 1 / Light 2.
    #[test]
    fn r1373_multi_object_scrub_preserves_the_mix() {
        let e = ext();
        e.select_all();
        assert_eq!(
            e.query("mixed.1"),
            Some(IntrospectValue::Bool(true)),
            "Layer starts mixed (1,1,2)"
        );
        e.arm_scrub(1);
        e.scrub_to(0.0); // calibrate: no mutation
        assert_eq!(
            obj_value(&e, 2, "Layer"),
            CellValue::Int(2),
            "the first move only calibrates"
        );
        assert!(
            !e.is_scrubbing(),
            "the calibration frame is not yet a scrub"
        );
        // +24px -> round(24/8) = +3 steps to EACH object.
        e.scrub_to(frac(3.0 * SCRUB_INT_PX_PER_STEP));
        assert!(e.is_scrubbing(), "a drag past the dead zone is a scrub");
        assert_eq!(obj_value(&e, 0, "Layer"), CellValue::Int(4), "Player 1 + 3");
        assert_eq!(obj_value(&e, 1, "Layer"), CellValue::Int(4), "Camera 1 + 3");
        assert_eq!(obj_value(&e, 2, "Layer"), CellValue::Int(5), "Light 2 + 3");
        assert_eq!(
            e.query("mixed.1"),
            Some(IntrospectValue::Bool(true)),
            "still mixed after a uniform shift (4,4,5)"
        );
        assert!(e.end_scrub(), "end_scrub reports a real scrub ran");
        assert!(
            e.scrub_target.borrow().is_none(),
            "the name-keyed snapshot is dropped at release"
        );
        assert!(!e.is_scrubbing(), "the release cleared the scrub");
    }

    /// A `Float` scrub is continuous and ABSOLUTE from the press snapshot: the
    /// value tracks the cursor (a move back to the press fraction restores the
    /// base), it does not accumulate; a leftward drag is signed.
    #[test]
    fn r1373_float_scrub_is_continuous_and_absolute_from_base() {
        let e = ext();
        e.select(0); // Speed (row 4) = 6.5
        e.arm_scrub(4);
        e.scrub_to(0.0);
        let CellValue::Float(v0) = obj_value(&e, 0, "Speed") else {
            panic!("Speed is Float");
        };
        assert!((v0 - 6.5).abs() < 1e-9, "the first move only calibrates");
        e.scrub_to(frac(100.0)); // +100px * 0.01 = +1.0
        let CellValue::Float(v1) = obj_value(&e, 0, "Speed") else {
            panic!()
        };
        assert!((v1 - 7.5).abs() < 1e-6, "6.5 + 100px*0.01 = 7.5, got {v1}");
        e.scrub_to(0.0); // back to the press fraction
        let CellValue::Float(v2) = obj_value(&e, 0, "Speed") else {
            panic!()
        };
        assert!(
            (v2 - 6.5).abs() < 1e-6,
            "absolute-from-snapshot: back at press restores 6.5, got {v2}"
        );
        e.scrub_to(frac(-50.0)); // signed: leftward decreases
        let CellValue::Float(v3) = obj_value(&e, 0, "Speed") else {
            panic!()
        };
        assert!((v3 - 6.0).abs() < 1e-6, "6.5 - 50px*0.01 = 6.0, got {v3}");
    }

    /// An `Int` scrub steps in whole units (8px/step, rounded to the nearest).
    #[test]
    fn r1373_int_scrub_steps_in_whole_units() {
        let e = ext();
        e.select(0); // Health (row 3) = 100
        e.arm_scrub(3);
        e.scrub_to(0.0);
        e.scrub_to(frac(5.0 * SCRUB_INT_PX_PER_STEP)); // +40px -> +5
        assert_eq!(
            obj_value(&e, 0, "Health"),
            CellValue::Int(105),
            "100 + round(40/8) = 105"
        );
        e.scrub_to(frac(2.0 * SCRUB_INT_PX_PER_STEP + 3.0)); // 19px -> round(2.375) = 2
        assert_eq!(
            obj_value(&e, 0, "Health"),
            CellValue::Int(102),
            "19px rounds to +2 whole steps"
        );
    }

    /// A sub-threshold press is a click, not a scrub: within `DRAG_CLICK_THRESHOLD_PX`
    /// the value does not move and `end_scrub` reports no scrub ran.
    #[test]
    fn r1373_a_sub_threshold_press_is_a_click_not_a_scrub() {
        let e = ext();
        e.select(0);
        e.arm_scrub(4); // Speed
        e.scrub_to(0.0);
        e.scrub_to(frac(2.0)); // 2px < 4px threshold
        assert!(!e.is_scrubbing(), "2px is inside the click dead zone");
        let CellValue::Float(v) = obj_value(&e, 0, "Speed") else {
            panic!()
        };
        assert!(
            (v - 6.5).abs() < 1e-9,
            "a dead-zone press does not nudge the value"
        );
        assert!(
            !e.end_scrub(),
            "end_scrub reports NO scrub ran (it was a click)"
        );
    }

    /// A drag with nothing armed never scrubs: the first move's seed declines, so
    /// no calibration is ever set and later moves stay inert.
    #[test]
    fn r1373_scrub_with_nothing_armed_is_a_noop() {
        let e = ext();
        e.select(0);
        e.scrub_to(0.0);
        e.scrub_to(frac(100.0));
        assert!(!e.is_scrubbing(), "an un-armed drag never scrubs");
        let CellValue::Float(v) = obj_value(&e, 0, "Speed") else {
            panic!()
        };
        assert!((v - 6.5).abs() < 1e-9, "nothing moved");
    }

    /// The AI-first `scrubbing` state is schema-declared and reflects the live
    /// drag (the read half of the §2 #2 scrub surface).
    #[test]
    fn r1373_scrubbing_query_and_schema() {
        let e = ext();
        assert!(
            e.schema().fields.iter().any(|f| f.path == "scrubbing"),
            "scrubbing is schema-declared"
        );
        e.select(0);
        assert_eq!(
            e.query("scrubbing"),
            Some(IntrospectValue::Bool(false)),
            "not scrubbing at rest"
        );
        e.arm_scrub(4);
        e.scrub_to(0.0);
        e.scrub_to(frac(100.0));
        assert_eq!(
            e.query("scrubbing"),
            Some(IntrospectValue::Bool(true)),
            "mid-drag scrubbing = true"
        );
        e.end_scrub();
        assert_eq!(
            e.query("scrubbing"),
            Some(IntrospectValue::Bool(false)),
            "released -> false"
        );
    }

    /// The scrub is driven end-to-end over the wire: a `typein<i>:PointerDown`
    /// arms, `pointer_move` (the captured cursor the router forwards) scrubs, and
    /// a `typein<i>:PointerUp` clears — the path a real mouse / the `scene/drag`
    /// RPC takes. A Colour typein press arms nothing (stays a click).
    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a scrub fraction in [0,1]; narrowing it to the f32 pointer_move \
                  takes is exact enough for the >=0.05-tolerance assertion"
    )]
    fn r1373_scrub_routes_through_the_send_wire_and_pointer_move() {
        let mut e = ext();
        e.invoke("select", IntrospectValue::Int(0)).unwrap(); // Player
        e.invoke(
            "send",
            IntrospectValue::Text("typein4:PointerDown".to_owned()),
        )
        .unwrap();
        assert_eq!(e.scrub_armed.get(), Some(4), "the send wire armed Speed");
        e.pointer_move(0.0, 0.0); // calibrate
        e.pointer_move(frac(100.0) as f32, 0.0); // +1.0
        assert_eq!(
            e.query("scrubbing"),
            Some(IntrospectValue::Bool(true)),
            "the wire drag scrubs"
        );
        let CellValue::Float(v) = obj_value(&e, 0, "Speed") else {
            panic!()
        };
        // The only imprecision on this in-process path is the f64->f32 fraction
        // narrowing (~1e-5); a wrong basis / sensitivity would be off by >=0.5.
        assert!((v - 7.5).abs() < 1e-3, "Speed scrubbed to ~7.5, got {v}");
        e.invoke(
            "send",
            IntrospectValue::Text("typein4:PointerUp".to_owned()),
        )
        .unwrap();
        assert_eq!(e.scrub_armed.get(), None, "the release cleared the arm");
        assert_eq!(
            e.query("scrubbing"),
            Some(IntrospectValue::Bool(false)),
            "the release cleared the scrub"
        );
        // A Colour typein press arms nothing.
        e.invoke(
            "send",
            IntrospectValue::Text("typein6:PointerDown".to_owned()),
        )
        .unwrap();
        assert_eq!(
            e.scrub_armed.get(),
            None,
            "a Colour cell press does not arm a scrub"
        );
    }

    /// R1373 — a leaked arm (an RPC `send "typein<i>:PointerDown"` with no paired
    /// release) is abandoned by the next press on a non-scrub affordance, so it
    /// cannot seed a PHANTOM scrub off a stale row.
    #[test]
    fn r1373_a_press_elsewhere_abandons_a_leaked_scrub_arm() {
        let mut e = ext();
        e.invoke("select", IntrospectValue::Int(0)).unwrap();
        // Leak the arm: a bare PointerDown over the numeric cell, no PointerUp.
        e.invoke(
            "send",
            IntrospectValue::Text("typein4:PointerDown".to_owned()),
        )
        .unwrap();
        assert_eq!(e.scrub_armed.get(), Some(4), "the arm is leaked");
        // A press on the object list (a non-scrub affordance) clears it before its
        // forwarded initial pointer_move could seed a phantom scrub.
        e.invoke("send", IntrospectValue::Text("2:PointerDown".to_owned()))
            .unwrap();
        assert_eq!(e.scrub_armed.get(), None, "the leaked arm is abandoned");
        // Proof it cannot phantom-scrub: a drive now declines (nothing armed).
        e.pointer_move(0.0, 0.0);
        e.pointer_move(0.5, 0.0);
        assert!(
            !e.is_scrubbing(),
            "no phantom scrub after the leak was cleared"
        );
    }

    /// R1373 — `intervene` on the query-only `scrubbing` field is `ReadOnly`, not
    /// `UnknownPath`: an agent that can `query` it must not be told it does not
    /// exist (§2 #7). The pre-existing query-only peers now share this honesty.
    #[test]
    fn r1373_intervene_scrubbing_is_read_only_not_unknown() {
        let mut e = ext();
        assert_eq!(
            e.intervene("scrubbing", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly),
            "scrubbing is queryable, so intervene is ReadOnly (not UnknownPath)"
        );
        assert_eq!(
            e.intervene("editing", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly),
            "the pre-existing query-only peers are honest too"
        );
        assert_eq!(
            e.intervene("no_such_field", IntrospectValue::Null),
            Err(InterveneError::UnknownPath),
            "a truly-unknown path is still UnknownPath"
        );
    }

    /// R1373 — the capture broadening's one behavior change on the object list: a
    /// press-drag-release across rows selects the PRESS row (the capture lock
    /// delivers `PointerUp` to the press tag), disclosed on `wants_pointer_capture`.
    #[test]
    fn r1373_object_row_press_drag_selects_the_press_row() {
        let mut e = ext();
        e.invoke("select", IntrospectValue::Int(0)).unwrap();
        // Press row 2, then release on row 2's tag (the capture lock routes the
        // release to the PRESS tag even if the cursor strayed). Selection = press.
        e.invoke("send", IntrospectValue::Text("2:PointerDown".to_owned()))
            .unwrap();
        e.invoke("send", IntrospectValue::Text("2:PointerUp".to_owned()))
            .unwrap();
        assert_eq!(
            e.query("selected"),
            Some(IntrospectValue::Int(2)),
            "the press row (2) is selected, not row 0"
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
        let fields: Vec<&str> = e.schema().fields.iter().map(|f| f.path).collect();
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
                Some(INSPECTOR_TAG),
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
                use_editing_prop(),
                use_text_edit_state(EDIT_TF_TAG),
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
                Some(INSPECTOR_TAG),
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
        // "Visible" is (t, t, f) → a mixed Bool: a tri-state `checkbox` carrying
        // aria-checked="mixed" (R1229; NOT a switch, which cannot be indeterminate).
        let visible = row("Visible");
        assert_eq!(visible.role, AriaRole::CheckBox, "a Bool row is a checkbox");
        assert!(
            visible.state.mixed,
            "a mixed Bool is aria-checked=\"mixed\""
        );
        assert!(
            visible.value.is_none(),
            "a mixed checkbox carries no definite on/off value"
        );
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
                Some(INSPECTOR_TAG),
            )
        });
        let row = |tag: &str| nodes.iter().find(|n| n.tag == tag).unwrap();
        // Visible (Bool, uniform true) → checkbox carrying aria-checked=true, and
        // the active descendant (Details cursor on row 0). R1229 — a uniform
        // multi-object Bool is a definite checkbox (not mixed).
        let visible = row("prop_0");
        assert_eq!(visible.role, AriaRole::CheckBox);
        assert_eq!(visible.value, Some(AccessValue::Bool(true)));
        assert!(!visible.state.mixed, "a uniform Bool is not indeterminate");
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
                Some(INSPECTOR_TAG),
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
                Some(INSPECTOR_TAG),
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

    // ─── R1225 Choice cell cycle ───────────────────────────────────

    fn choice_sel(v: &CellValue) -> usize {
        match v {
            CellValue::Choice { selected, .. } => *selected,
            other => panic!("expected a Choice, got {other:?}"),
        }
    }

    fn cycle(e: &mut InspectorExternal, spec: &str) -> IntrospectValue {
        e.invoke("cycle_property", IntrospectValue::Text(spec.to_owned()))
            .expect("cycle_property")
    }

    #[test]
    fn r1225_cycle_choice_wraps_prev_and_next() {
        let mut e = ext(); // Player selected; Team (Red/Blue/Neutral) is common row 5.
        assert_eq!(
            e.query("kind.5"),
            Some(IntrospectValue::Text("choice".to_owned())),
            "row 5 is Team, a Choice"
        );
        assert_eq!(choice_sel(&obj_value(&e, 0, "Team")), 0, "Team starts Red");
        assert_eq!(
            cycle(&mut e, "5,1"),
            IntrospectValue::Bool(true),
            "cycle +1"
        );
        assert_eq!(choice_sel(&obj_value(&e, 0, "Team")), 1, "Red -> Blue");
        cycle(&mut e, "5,1");
        cycle(&mut e, "5,1");
        assert_eq!(
            choice_sel(&obj_value(&e, 0, "Team")),
            0,
            "Neutral wraps to Red"
        );
        cycle(&mut e, "5,-1");
        assert_eq!(
            choice_sel(&obj_value(&e, 0, "Team")),
            2,
            "Red wraps back to Neutral"
        );
    }

    #[test]
    fn r1225_cycle_multi_object_is_relative_and_mix_preserving() {
        let mode = |sel: usize| {
            ObjectData::new(
                "obj",
                vec![Property::new(
                    "Mode",
                    CellValue::Choice {
                        selected: sel,
                        options: vec!["A".to_owned(), "B".to_owned(), "C".to_owned()],
                    },
                )],
            )
        };
        let mut e = Owner::new().run(|| {
            let objects = Rc::new(Signal::new(vec![mode(0), mode(2)])); // A, C
            let mut model = VirtualSelect::new(2, SelectionMode::Multi);
            model.set_selection(&[0usize, 1].into_iter().collect());
            InspectorExternal::new(
                objects,
                Rc::new(Signal::new(model)),
                Rc::new(vec![mode(0), mode(2)]),
                Rc::new(Signal::new(InspectorFocus::default())),
                use_editing_prop(),
                use_text_edit_state(EDIT_TF_TAG),
            )
        });
        assert_eq!(
            e.query("row_count"),
            Some(IntrospectValue::Int(1)),
            "Mode is common"
        );
        assert_eq!(
            e.query("mixed.0"),
            Some(IntrospectValue::Bool(true)),
            "A vs C -> mixed"
        );
        // Cycle +1: each advances from its OWN option (A->B, C->A wrap), so they
        // stay one-apart — mix PRESERVED (the numeric-step semantics, not the
        // Bool toggle's resolve-to-uniform).
        cycle(&mut e, "0,1");
        assert_eq!(choice_sel(&obj_value(&e, 0, "Mode")), 1, "obj0 A -> B");
        assert_eq!(
            choice_sel(&obj_value(&e, 1, "Mode")),
            0,
            "obj1 C wraps -> A"
        );
        assert_eq!(
            e.query("mixed.0"),
            Some(IntrospectValue::Bool(true)),
            "still mixed -> the cycle is relative, not absolute"
        );
    }

    #[test]
    fn r1225_cycle_rejects_nonchoice_and_malformed() {
        let mut e = ext(); // Player
        assert_eq!(
            cycle(&mut e, "0,1"),
            IntrospectValue::Bool(false),
            "cycle on a Bool is a no-op"
        );
        assert_eq!(
            cycle(&mut e, "1,1"),
            IntrospectValue::Bool(false),
            "cycle on an Int is a no-op"
        );
        assert_eq!(
            cycle(&mut e, "99,1"),
            IntrospectValue::Bool(false),
            "out-of-range is a no-op"
        );
        assert!(
            e.invoke("cycle_property", IntrospectValue::Text("bad".to_owned()))
                .is_err(),
            "a malformed spec is a typed Rejected"
        );
        assert!(
            e.invoke("cycle_property", IntrospectValue::Int(1)).is_err(),
            "a non-string arg is a TypeMismatch"
        );
    }

    #[test]
    fn r1225_cycle_verb_schema_declared_and_choice_cell_click_cycles() {
        let mut e = ext();
        let fields: Vec<&str> = e.schema().fields.iter().map(|f| f.path).collect();
        assert!(
            fields.contains(&"cycle_property"),
            "cycle_property schema-declared"
        );
        // The painted Choice cell is `inspector#cycle<i>`; a click routes to
        // cycle_property(+1) through the send wire (the segmented-control gesture).
        assert_eq!(choice_sel(&obj_value(&e, 0, "Team")), 0, "Team starts Red");
        e.invoke("send", IntrospectValue::Text("cycle5:PointerUp".to_owned()))
            .unwrap();
        assert_eq!(
            choice_sel(&obj_value(&e, 0, "Team")),
            1,
            "clicking the Choice cell cycles Red -> Blue"
        );
    }

    #[test]
    fn r1228_a11y_choice_is_spinbutton_with_selected_option() {
        let nodes = Owner::new().run(|| {
            <InspectorView as WidgetA11y>::access_node(
                &InspectorState::from_parts(&[0], Some(0))
                    .with_focus(FocusRegion::Details, Some(5)),
                Some(INSPECTOR_TAG),
            )
        });
        let team = nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("Team"))
            .expect("Team row");
        // R1228 — a Choice row is a `spinbutton` (NOT a combobox: no popup /
        // aria-expanded; arrow-cycle maps to Increment/Decrement).
        assert_eq!(
            team.role,
            AriaRole::SpinButton,
            "a Choice row is a spinbutton"
        );
        assert_eq!(
            team.value_text.as_deref(),
            Some("Red"),
            "the selected option"
        );
        assert!(
            team.state.focused,
            "the Details cursor on Team is the active descendant"
        );
    }

    #[test]
    fn r1228_edit_property_at_is_the_shared_kind_dispatch() {
        // The ONE dispatch both the keyboard property-cursor path and the AT
        // operable-role path funnel through — verified by kind here.
        let mut e = ext(); // Player: Visible(bool,0) Layer(int,1) … Team(choice,5)
        assert_eq!(choice_sel(&obj_value(&e, 0, "Team")), 0, "Team starts Red");
        assert!(edit_property_at(&mut e, 5, Some("choice"), "ArrowRight"));
        assert_eq!(choice_sel(&obj_value(&e, 0, "Team")), 1, "a Choice cycles");
        assert_eq!(obj_value(&e, 0, "Visible"), CellValue::Bool(true));
        assert!(edit_property_at(&mut e, 0, Some("bool"), "Enter"));
        assert_eq!(
            obj_value(&e, 0, "Visible"),
            CellValue::Bool(false),
            "a Bool toggles"
        );
        assert_eq!(obj_value(&e, 0, "Layer"), CellValue::Int(1));
        assert!(edit_property_at(&mut e, 1, Some("int"), "ArrowLeft"));
        assert_eq!(
            obj_value(&e, 0, "Layer"),
            CellValue::Int(0),
            "a numeric steps"
        );
        assert!(
            !edit_property_at(&mut e, 0, Some("bool"), "F1"),
            "an unrecognised key is not consumed"
        );
    }

    #[test]
    fn r1228_at_action_operates_the_targeted_row_regardless_of_region() {
        use pinion_core::external::External;
        use pinion_core::scene::ExternalNode;
        Owner::new().run(|| {
            // R1249 — tag the primary as the runtime does (`CoreShell` wraps
            // `ExternalNode::new(...).with_tag(V::tag())`), so `apply_key`'s
            // `find_external_with_tag(INSPECTOR_TAG)` container-walk resolves it.
            // The pre-R1249 untagged node only worked because the old code matched
            // `Scene::External` shape-blind.
            let mut scene = Scene::External(
                ExternalNode::new(Box::new(make_inspector_external()) as Box<dyn External>)
                    .with_tag(INSPECTOR_TAG),
            );
            let team = || {
                let objs = use_objects().get();
                choice_sel(
                    &objs[0]
                        .properties
                        .iter()
                        .find(|p| p.name == "Team")
                        .unwrap()
                        .value,
                )
            };
            assert_eq!(
                team(),
                0,
                "Team starts Red; boot region is Objects (NOT Details)"
            );
            // The shell routes an AT Increment on the Team spinbutton as
            // apply_key(Some("prop_5"), "ArrowRight"). It must operate row 5 even
            // though the pointer cursor / region is on the Objects pane.
            let handled = InspectorView::apply_key(
                &mut scene,
                Some("prop_5"),
                "ArrowRight",
                Modifiers::empty(),
            );
            assert!(handled, "the AT action is handled");
            assert_eq!(
                team(),
                1,
                "AT Increment cycled Team from the Objects region"
            );
            // The AT-targeted row became the focused Details cursor (ring + a11y).
            let focus = use_focus().get();
            assert_eq!(focus.region, FocusRegion::Details);
            assert_eq!(focus.prop_cursor, Some(5));
        });
    }

    // ── R1249 inline absolute type-in editor ─────────────────────────────
    // Common numeric row across all three objects: Layer(1) = 1, 1, 2. The
    // steppers shift RELATIVELY; the type-in editor writes an ABSOLUTE value.

    fn layer_default(obj: usize) -> CellValue {
        default_objects()[obj].properties[1].value.clone()
    }

    #[test]
    fn r1249_begin_edit_opens_a_numeric_row_and_seeds_the_value() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        assert_eq!(
            e.query("name.1"),
            Some(IntrospectValue::Text("Layer".to_owned()))
        );
        assert!(e.begin_edit(1), "a numeric row opens the editor");
        assert_eq!(e.query("editing"), Some(IntrospectValue::Int(1)));
        // Seeded with the representative (first selected object's) value.
        assert_eq!(e.editor.text(), "1", "seeded with the representative value");
    }

    #[test]
    fn r1249_begin_edit_rejects_non_numeric_and_out_of_range_rows() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        // Visible(0) + Locked(2) are Bool rows — the type-in editor never opens.
        assert!(
            !e.begin_edit(0),
            "a Bool row does not open the type-in editor"
        );
        assert!(
            !e.begin_edit(2),
            "a Bool row does not open the type-in editor"
        );
        assert!(!e.begin_edit(99), "an out-of-range row is a benign miss");
        assert_eq!(
            e.query("editing"),
            Some(IntrospectValue::Null),
            "still closed"
        );
    }

    #[test]
    fn r1249_commit_writes_the_absolute_value_across_the_whole_selection() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        assert!(e.begin_edit(1));
        // Type 7 + commit -> EVERY selected object's Layer becomes 7 (absolute,
        // collapsing the prior 1/1/2 divergence) — the multi-object write, for
        // free through `set_property`.
        assert!(e.commit_edit_text("7", true), "a valid numeric commits");
        for obj in 0..3 {
            assert_eq!(obj_value(&e, obj, "Layer"), CellValue::Int(7));
        }
        assert_eq!(
            e.query("editing"),
            Some(IntrospectValue::Null),
            "closed after commit"
        );
        assert_eq!(e.editor.text(), "", "buffer wiped for the next edit");
    }

    #[test]
    fn r1249_commit_of_a_malformed_number_keeps_the_prior_value() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        assert!(e.begin_edit(1));
        assert!(
            !e.commit_edit_text("not a number", true),
            "a malformed numeric is not written (no data loss)"
        );
        for obj in 0..3 {
            assert_eq!(obj_value(&e, obj, "Layer"), layer_default(obj));
        }
        assert_eq!(e.query("editing"), Some(IntrospectValue::Null));
    }

    #[test]
    fn r1249_cancel_leaves_every_value_untouched() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        assert!(e.begin_edit(1));
        e.close_edit(true);
        for obj in 0..3 {
            assert_eq!(obj_value(&e, obj, "Layer"), layer_default(obj));
        }
        assert_eq!(e.query("editing"), Some(IntrospectValue::Null));
    }

    #[test]
    fn r1249_editing_and_edit_text_are_rpc_introspectable() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        assert_eq!(
            e.query("editing"),
            Some(IntrospectValue::Null),
            "closed at boot"
        );
        e.begin_edit(1);
        assert_eq!(e.query("editing"), Some(IntrospectValue::Int(1)));
        assert_eq!(
            e.query("edit_text"),
            Some(IntrospectValue::Text("1".to_owned()))
        );
    }

    #[test]
    fn r1249_rpc_begin_and_commit_verbs_drive_the_whole_edit() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        // The AI-first path: begin on a numeric row, commit the wire text.
        assert_eq!(
            e.invoke("begin_edit", IntrospectValue::Int(1)).unwrap(),
            IntrospectValue::Bool(true)
        );
        assert_eq!(
            e.invoke("commit_edit", IntrospectValue::Text("42".to_owned()))
                .unwrap(),
            IntrospectValue::Bool(true)
        );
        for obj in 0..3 {
            assert_eq!(obj_value(&e, obj, "Layer"), CellValue::Int(42));
        }
        // begin_edit on a Bool row via RPC -> false, editor stays closed.
        assert_eq!(
            e.invoke("begin_edit", IntrospectValue::Int(0)).unwrap(),
            IntrospectValue::Bool(false)
        );
        // cancel_edit verb closes without writing.
        e.invoke("begin_edit", IntrospectValue::Int(1)).unwrap();
        assert_eq!(
            e.invoke("cancel_edit", IntrospectValue::Null).unwrap(),
            IntrospectValue::Null
        );
        assert_eq!(e.query("editing"), Some(IntrospectValue::Null));
    }

    #[test]
    fn r1249_double_click_send_opens_the_editor_single_click_does_not() {
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        // A single click (PointerUp) on the numeric cell is the stepper row — it
        // does NOT open the type-in editor.
        e.invoke(
            "send",
            IntrospectValue::Text("typein1:PointerUp".to_owned()),
        )
        .unwrap();
        assert_eq!(
            e.query("editing"),
            Some(IntrospectValue::Null),
            "a single click does not open the editor"
        );
        // A double-click opens it.
        e.invoke(
            "send",
            IntrospectValue::Text("typein1:DoubleClick".to_owned()),
        )
        .unwrap();
        assert_eq!(
            e.query("editing"),
            Some(IntrospectValue::Int(1)),
            "a double-click opens the editor"
        );
    }

    #[test]
    fn r1249_editing_row_paints_the_field_not_the_steppers() {
        Owner::new().run(|| {
            let mut e = make_inspector_external();
            e.invoke("select_all", IntrospectValue::Null).unwrap();
            let state = InspectorState::from_parts(&[0, 1, 2], Some(0));
            // Idle: the numeric cell is the stepper row (a `typein1` double-click
            // target); no inline field.
            let idle = view(&state, &Frame::new());
            assert!(idle.contains_tag(&format!("{INSPECTOR_TAG}#{TYPEIN_PREFIX}1")));
            assert!(!idle.contains_tag(EDIT_TF_TAG));
            // Editing row 1: the inline field replaces the steppers.
            e.begin_edit(1);
            let editing = view(&state, &Frame::new());
            assert!(editing.contains_tag(EDIT_TF_TAG), "the inline field paints");
            assert!(
                !editing.contains_tag(&format!("{INSPECTOR_TAG}#{TYPEIN_PREFIX}1")),
                "the steppers give way to the field"
            );
        });
    }

    #[test]
    fn r1249_editing_row_a11y_is_a_textbox_not_a_spinbutton() {
        Owner::new().run(|| {
            let mut e = make_inspector_external();
            e.invoke("select_all", IntrospectValue::Null).unwrap();
            let state = InspectorState::from_parts(&[0, 1, 2], Some(0));
            e.begin_edit(1);
            let nodes = <InspectorView as WidgetA11y>::access_node(&state, None);
            // The edited row's AT node is the inline field's `textbox` (tagged
            // EDIT_TF_TAG so it resolves to the field bounds), NOT the row's
            // spinbutton — the paint==a11y one-gate.
            let field = nodes
                .iter()
                .find(|n| n.tag == EDIT_TF_TAG)
                .expect("the edited row emits a textbox a11y node");
            assert_eq!(field.role, AriaRole::TextInput);
            assert!(
                !nodes.iter().any(|n| n.tag == "prop_1"),
                "the spinbutton yields to the textbox while editing"
            );
        });
    }

    // ── R1251 Colour hex type-in ─────────────────────────────────────────
    // Player's Tint (Color) is common row 6 in the boot single-selection
    // (Visible/Layer/Locked/Health/Speed/Team/Tint). The SAME inline field
    // edits it as a `#RRGGBB` hex — the CellKind::Color keystroke gate + parse.

    const TINT: usize = 6;

    fn color_hex(e: &InspectorExternal, idx: usize) -> String {
        match e.query(&format!("value.{idx}")) {
            Some(IntrospectValue::Json(v)) => v["hex"].as_str().unwrap().to_owned(),
            other => panic!("expected a Colour json, got {other:?}"),
        }
    }

    #[test]
    fn r1251_begin_edit_opens_a_colour_row_seeded_with_the_hex() {
        let e = ext(); // boot: Player single-selected
        assert_eq!(
            e.query("name.6"),
            Some(IntrospectValue::Text("Tint".to_owned()))
        );
        assert_eq!(
            e.query("kind.6"),
            Some(IntrospectValue::Text("color".to_owned()))
        );
        assert!(e.begin_edit(TINT), "a Colour row opens the type-in editor");
        assert_eq!(e.query("editing"), Some(IntrospectValue::Int(6)));
        // Seeded with the current hex (the property's edit_text).
        assert_eq!(e.editor.text(), color_hex(&e, TINT), "seeded with the hex");
    }

    #[test]
    fn r1251_commit_writes_the_new_colour_from_the_typed_hex() {
        let e = ext();
        assert!(e.begin_edit(TINT));
        // This is the path R1249's `set_property(&to_introspect())` could NOT
        // take: a Colour's to_introspect is rich JSON, not the hex `Text`
        // with_intervene wants. The direct mutate_selected write makes it work.
        assert!(e.commit_edit_text("#ff0000", true), "a valid hex commits");
        assert_eq!(color_hex(&e, TINT), "#ff0000", "Tint is now red");
        assert_eq!(e.query("editing"), Some(IntrospectValue::Null));
    }

    #[test]
    fn r1251_commit_of_a_malformed_hex_keeps_the_prior_colour() {
        let e = ext();
        let before = color_hex(&e, TINT);
        assert!(e.begin_edit(TINT));
        assert!(
            !e.commit_edit_text("not-a-hex", true),
            "a malformed hex is not written (no data loss)"
        );
        assert_eq!(color_hex(&e, TINT), before, "the prior colour is kept");
        assert_eq!(e.query("editing"), Some(IntrospectValue::Null));
    }

    #[test]
    fn r1251_rpc_colour_edit_via_begin_and_commit_verbs() {
        let mut e = ext();
        assert_eq!(
            e.invoke("begin_edit", IntrospectValue::Int(6)).unwrap(),
            IntrospectValue::Bool(true)
        );
        assert_eq!(
            e.invoke("commit_edit", IntrospectValue::Text("#00ff00".to_owned()))
                .unwrap(),
            IntrospectValue::Bool(true)
        );
        assert_eq!(color_hex(&e, TINT), "#00ff00");
    }

    #[test]
    fn r1251_colour_row_is_a_typein_target_and_paints_the_field_when_editing() {
        Owner::new().run(|| {
            let e = make_inspector_external(); // Player single-selected (boot)
            let state = InspectorState::from_parts(&[0], Some(0));
            let idle = view(&state, &Frame::new());
            assert!(
                idle.contains_tag(&format!("{INSPECTOR_TAG}#{TYPEIN_PREFIX}6")),
                "the Colour cell is a double-click type-in target"
            );
            assert!(!idle.contains_tag(EDIT_TF_TAG), "no field until editing");
            e.begin_edit(TINT);
            let editing = view(&state, &Frame::new());
            assert!(
                editing.contains_tag(EDIT_TF_TAG),
                "editing the Colour row paints the hex field"
            );
        });
    }

    // ── R1252 selection change mid-edit closes the editor (no wrong-write) ──

    #[test]
    fn r1252_selection_change_mid_edit_closes_the_editor_no_wrong_property_write() {
        // The execution-proven data-corruption repro: `editing_prop` is a
        // positional index into the selection-DERIVED common list, so a mid-edit
        // selection change used to retarget it to a DIFFERENT property.
        let mut e = ext();
        // Single-select Camera (obj 1): common idx3 = "Field of View" (Float 60).
        e.invoke("select", IntrospectValue::Int(1)).unwrap();
        assert_eq!(
            e.query("name.3"),
            Some(IntrospectValue::Text("Field of View".to_owned()))
        );
        assert!(e.begin_edit(3), "open the Field of View editor");
        assert_eq!(e.query("editing"), Some(IntrospectValue::Int(3)));
        // Switch to Sun Light (obj 2): common idx3 = "Intensity" (Float 1.2). This
        // MUST close the editor so the stale index cannot retarget Intensity.
        e.invoke("select", IntrospectValue::Int(2)).unwrap();
        assert_eq!(
            e.query("editing"),
            Some(IntrospectValue::Null),
            "the selection change closed the editor"
        );
        assert_eq!(
            e.query("name.3"),
            Some(IntrospectValue::Text("Intensity".to_owned()))
        );
        // A commit now writes nothing (no open editor); BOTH properties untouched.
        assert!(
            !e.commit_edit_text("45", true),
            "commit with a closed editor writes nothing"
        );
        assert_eq!(obj_value(&e, 1, "Field of View"), CellValue::Float(60.0));
        assert_eq!(
            obj_value(&e, 2, "Intensity"),
            CellValue::Float(1.2),
            "Intensity was NOT clobbered (the R1252 fix)"
        );
    }

    #[test]
    fn r1252_select_all_mid_edit_closes_the_editor() {
        let mut e = ext(); // boot: Player single-selected
        assert!(e.begin_edit(1), "open the Layer editor on Player");
        assert_eq!(e.query("editing"), Some(IntrospectValue::Int(1)));
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        assert_eq!(
            e.query("editing"),
            Some(IntrospectValue::Null),
            "select_all closed the open editor"
        );
    }

    #[test]
    fn r1252_a_stable_selection_still_commits_normally() {
        // The fix must NOT break the normal path: no selection change between
        // begin and commit -> the edit commits across the (stable) selection.
        let mut e = ext();
        e.invoke("select_all", IntrospectValue::Null).unwrap();
        assert!(e.begin_edit(1), "Layer is common across all three");
        assert!(
            e.commit_edit_text("7", true),
            "a stable-selection commit writes"
        );
        for obj in 0..3 {
            assert_eq!(obj_value(&e, obj, "Layer"), CellValue::Int(7));
        }
    }

    // ── R1254 keyboard / AT open path for the type-in editor ──────────────

    fn tagged_state_scene() -> Scene {
        use pinion_core::external::External;
        use pinion_core::scene::ExternalNode;
        Scene::External(
            ExternalNode::new(Box::new(make_inspector_external()) as Box<dyn External>)
                .with_tag(INSPECTOR_TAG),
        )
    }

    #[test]
    fn r1254_f2_and_enter_open_the_editor_on_a_field_editable_cursor() {
        Owner::new().run(|| {
            let mut scene = tagged_state_scene();
            let ext = make_inspector_external(); // shares the Owner-cached signals
            ext.set_prop_cursor(Some(1)); // Details cursor on Layer (row 1, Int)
            assert_eq!(use_editing_prop().get(), None, "editor closed to start");
            // F2 on the Details cursor opens the type-in editor (was mouse+RPC only).
            assert!(InspectorView::apply_key(
                &mut scene,
                Some(INSPECTOR_TAG),
                "F2",
                Modifiers::empty()
            ));
            assert_eq!(
                use_editing_prop().get(),
                Some(1),
                "F2 opened the editor on Layer"
            );
            ext.close_edit(true);
            ext.set_prop_cursor(Some(1));
            // Enter also opens it on a field-editable row.
            assert!(InspectorView::apply_key(
                &mut scene,
                Some(INSPECTOR_TAG),
                "Enter",
                Modifiers::empty()
            ));
            assert_eq!(
                use_editing_prop().get(),
                Some(1),
                "Enter opened the editor on a numeric"
            );
        });
    }

    #[test]
    fn r1254_enter_on_a_bool_toggles_it_does_not_open_an_editor() {
        Owner::new().run(|| {
            let mut scene = tagged_state_scene();
            let ext = make_inspector_external();
            ext.set_prop_cursor(Some(0)); // Visible (row 0) is a Bool
            let before = obj_value(&ext, 0, "Visible");
            assert!(InspectorView::apply_key(
                &mut scene,
                Some(INSPECTOR_TAG),
                "Enter",
                Modifiers::empty()
            ));
            assert_eq!(
                use_editing_prop().get(),
                None,
                "Enter on a Bool does not open the editor"
            );
            assert_ne!(
                obj_value(&ext, 0, "Visible"),
                before,
                "Enter toggled the Bool (kept its existing behaviour)"
            );
        });
    }

    #[test]
    fn r1254_at_activate_opens_the_colour_editor_via_prop_tag() {
        Owner::new().run(|| {
            let mut scene = tagged_state_scene();
            // The R1228 AT route: apply_key(Some("prop_6"), "Enter") — an AT
            // activate on Player's Tint (Colour, row 6). Before R1254 a Colour had
            // NO non-mouse edit (step/toggle no-op on it).
            assert!(InspectorView::apply_key(
                &mut scene,
                Some("prop_6"),
                "Enter",
                Modifiers::empty()
            ));
            assert_eq!(
                use_editing_prop().get(),
                Some(6),
                "an AT activate opened the Colour editor"
            );
        });
    }

    #[test]
    fn r1254_begin_edit_places_the_details_cursor_on_the_edited_row() {
        Owner::new().run(|| {
            let ext = make_inspector_external();
            assert!(ext.begin_edit(1), "open Layer");
            let focus = use_focus().get();
            assert_eq!(
                focus.region,
                FocusRegion::Details,
                "begin_edit focuses the Details pane"
            );
            assert_eq!(
                focus.prop_cursor,
                Some(1),
                "the keyboard cursor lands on the edited row (a11y focused-bool match)"
            );
        });
    }

    #[test]
    fn r1254_colour_row_a11y_is_a_textbox_not_a_listitem() {
        Owner::new().run(|| {
            let state = InspectorState::from_parts(&[0], Some(0)); // Player
            let nodes = <InspectorView as WidgetA11y>::access_node(&state, None);
            let tint = nodes
                .iter()
                .find(|n| n.tag == "prop_6")
                .expect("Tint emits an a11y node");
            assert_eq!(
                tint.role,
                AriaRole::TextInput,
                "a Colour row is an operable textbox (AT can activate to edit)"
            );
        });
    }
}
