// R836 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (WAI-ARIA, PropertyGridExternal, TextFieldExternal, gridcell, …).
#![allow(clippy::doc_markdown)]

//! `hello-property-grid` — R836 / R921 §5.38 §5.40 §5.50 **property-grid /
//! inspector detail panel**: the editor's "Details" panel (Unreal Details
//! / Qt `QtPropertyBrowser` / a CSS-devtools style editor) — a **tree** of
//! `(name, typed-value)` rows where each value is editable in place by a
//! *type-appropriate* control. Scalar properties nest under collapsible
//! **category** branches (Identity / Appearance / Transform / …) and **struct**
//! branches (a `Vector3` like Position expands into its X / Y / Z field rows —
//! the Unreal / Qt Details core depth), with a **live name search box** (R872 —
//! the Details-panel filter).
//!
//! ## Why this is the Phase-B #1 leverage item
//!
//! The northern-star is an Unreal-class editor self-hosted in pinion. That
//! editor's Details / Inspector panel is a property grid; this binding builds
//! it as a **pure composition of existing substrate** — no new framework crate.
//! R836–R920 built the flat, category-grouped grid; **R921** migrated its row
//! backbone from the 2-level `GroupOrderState` group-by proxy to the
//! arbitrary-depth WAI-ARIA Tree substrate (`flat_visible` / `resolve_tree_key`
//! / `tree_access_nodes`), because categories and struct properties are the
//! same thing — a collapsible branch — and a struct's X/Y/Z fields need a third
//! level the group proxy cannot express. The self-hosted editor will be the 2nd
//! consumer of the typed-editable-row model, at which point the stable parts
//! lift to a framework crate (`[[abstraction-needs-second-consumer]]`).
//!
//! ## Architecture — the value model ⊥ the structure tree
//!
//! The **value model** stays a flat `Signal<Vec<CellValue>>` keyed by **value
//! index** (the scrub / popup / inline-edit / reset / `value.<i>` RPC machinery
//! is unchanged across the R921 migration); the **structure tree** is a separate
//! `Signal<Vec<PropertyNode>>` carrying the hierarchy + the per-branch collapse
//! (`expanded`) state. A **leaf** node's id IS its value index in decimal ("6"),
//! so the painted row tag `{GRID_TAG}#{i}`, the `reset{i}` arrow and the
//! `value.<i>` path are byte-identical to the flat era; a **branch** node's id
//! is prefixed (`cat.` / `struct.`). Four externals:
//!
//! * **`PropertyGridExternal`** (`property_grid`, primary) — the coordinator. It
//!   owns the value model, the structure tree + the roving cursor (a node id),
//!   and the edit-mode latch. AI-first introspection (§2 #2): `query
//!   "value.<i>"` reads a typed value, `"name.<i>"` / `"kind.<i>"` the metadata,
//!   `"modified.<i>"` the dirty flag; `intervene "value.<i>"` sets a value;
//!   `invoke "toggle"/"begin"/"reset"/"reset_all" <i>` drive a leaf;
//!   `"expanded.<branch_id>"` (read/intervene) + `invoke "toggle_branch"` drive
//!   collapse; `"struct_summary.<id>"` / `"struct_modified.<id>"` + `invoke
//!   "reset_struct"` drive the struct aggregate.
//! * **`TreeViewIntrospect`** (`property_grid_tree`, extra, read-only) — surfaces
//!   the visible-row flatten + the cursor to `scene/query` (`row_count` /
//!   `id_at.<pos>` / `level_at.<pos>` / `expanded_at.<pos>` / `cursor`), so an AI
//!   client walks the Inspector hierarchy as data. Collapse / cursor *mutation*
//!   routes through the primary, so this node owns no mutation path.
//! * **`TextFieldExternal`** (`property_grid_edit`, extra) — ONE shared inline
//!   editor reused across every text / int / float leaf (the todomvc
//!   single-editor pattern). It paints only inside the editing leaf's value cell.
//! * **`TextFieldExternal`** (`property_grid_search`, extra) — the R872 search
//!   box whose live text is the name filter (`flat_visible_filtered`'s
//!   path-to-match reveals matches inside collapsed branches).
//!
//! There is no per-row external — bools toggle through the coordinator, and
//! text / number leaves route their inline edit through the one shared field.
//!
//! ## Keyboard model (WAI-ARIA APG Tree)
//!
//! The grid is a **single Tab stop** with a roving id-keyed cursor over the
//! flattened tree (the shared [`resolve_tree_key`] policy: `ArrowUp` /
//! `ArrowDown` / `Home` / `End` clamp, no wrap; `ArrowRight` / `ArrowLeft` /
//! `Enter` / `Space` expand / collapse / descend a **branch**). On a **leaf**,
//! `Space` toggles a bool, `Enter` / `F2` edits a text / int / float leaf or
//! opens a choice / colour popup, and `Delete` resets it (focus moves into the
//! shared inline field via the [`pinion_core::focus_request`] mailbox). While
//! editing: `Enter` commits, `Escape` cancels, and int / float leaves gate
//! non-numeric keystrokes. A click-away commit-on-blur rides the field's
//! `with_blur_intent` (R793).
//!
//! ## a11y (R921 §5.40 §5.27 §5.50) — WAI-ARIA Tree SSOT
//!
//! The panel lowers to a WAI-ARIA `tree` through the lifted
//! [`pinion_a11y::tree_access_nodes`] builder: each visible row is a `treeitem`
//! carrying its hierarchical axes (`aria-level` = depth + 1, `aria-expanded` on
//! branches, `aria-posinset` / `aria-setsize`). The single-column tree folds
//! each row's value into its accessible name (a leaf → `"Position X: 12.5"`, a
//! struct → `"Position (12.5, -4, 0)"`, a category → `"Identity (3)"`), so the
//! value is announced without a separate `gridcell`. The Inspector has **no
//! selection model**, so no row carries `aria-selected`: the roving cursor is
//! keyboard focus, exposed as `aria-activedescendant` through
//! `access_focus_target` (the authoritative channel; the per-node `focused`
//! flag is a redundant marker), ringing the cursor's row tag `{GRID_TAG}#{id}`.
//!
//! ## Known gaps (honest carry)
//!
//! - **Native checkbox / textbox cell roles.** Bool cells encode their state
//!   as cell text (the value `"On"` / `"Off"`) rather than a nested `checkbox`
//!   role, and the inline editor is a plain `textbox`. A per-cell-role grid a11y axis
//!   is additive and deferred until the self-hosted editor (2nd consumer)
//!   pins the exact shape (`[[abstraction-needs-second-consumer]]`).
//! - **Per-property validation / clamp ranges.** Numeric rows accept any
//!   parseable value; a malformed commit reverts to the prior value (no data
//!   loss) rather than clamping into a per-property `[min, max]`. Range
//!   metadata is an additive model field (the `hello-number-input`
//!   `parse_clamp` shape) deferred to the same 2nd-consumer round.

use std::cell::Cell;
use std::rc::Rc;

use pinion_a11y::{
    listbox_option_nodes, tree_access_nodes, tree_row_tag, AccessFocus, AccessNode, AriaRole,
    ListOption, WidgetA11y,
};
use pinion_core::composite_tag::split_send_payload;
use pinion_core::input::{DragCalibration, DRAG_CLICK_THRESHOLD_PX};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, CaptureNormalize, External, ExternalIntrospect,
    IntrospectSchema, IntrospectValue, InterveneError, InvokeError, RepaintOwner, ThreadOwnership,
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
use pinion_core::widgets::text_edit::{use_text_edit_state, TextEditState};
use pinion_core::widgets::text_field::{TextFieldExternal, TextFieldState};
use pinion_core::widgets::tree_nav::{
    flat_visible, flat_visible_filtered, resolve_tree_key, set_expanded_in, toggle_expanded,
    tree_view_introspection_extra, TreeKey, TreeNode, VisibleRow,
};
use pinion_core::cell_value::{CellKind, CellValue};
use pinion_core::{Color, Command, Frame, Modifiers, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::barrier::dismiss_barrier;
use pinion_widget_paint::checkbox::{view_checkbox_box, CheckboxStyle};
use pinion_widget_paint::group_header::group_header_row;
use pinion_widget_paint::listbox::{view_option, OptionRow};
use pinion_widget_paint::popup::popup_surface;
use pinion_widget_paint::text_field as tf_paint;

use pinion_widget_paint::state_layer::HOVER;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloPropertyGridRenderer, HelloPropertyGridRendererError);

// ─── window + layout constants ─────────────────────────────────────

const WIN_W: u32 = 460;
// R921 — the property tree fully expanded is 23 visible rows (5 category
// branches + 2 struct branches + 16 leaf rows), plus the `Property` / `Value`
// column header and the title band (a popup may flip above its row near the
// bottom). Collapsing a branch hides its subtree, shrinking the list.
const WIN_H: u32 = 820;
const THEME_TAG: &str = "app";

const TITLE_PX: u32 = 22;
const HEADER_PX: u32 = 13;
const CELL_PX: u32 = 15;

const NAME_COL_W: u32 = 150;
const VALUE_COL_W: u32 = 250;
const ROW_H: u32 = 38;
/// R919 — the reset-arrow mark size (px) and its inset from the row's trailing
/// edge (mark size + a small margin), for a modified row's reset affordance.
const RESET_DOT: u32 = 10;
const RESET_DOT_X: u32 = 16;
const CELL_PAD: u32 = 10;
const CHECKBOX_SIZE: u32 = 20;
const PANEL_PAD: u32 = 20;
const ROW_GAP: u32 = 2;
/// R921 — per-depth indent (px) of a nested tree row's name cell (leaf rows
/// under a category = depth 1; struct fields = depth 2 — the Details panel's
/// hierarchical inset).
const INDENT_STEP: u32 = 16;
/// U+25B8 BLACK RIGHT-POINTING SMALL TRIANGLE — a collapsed branch's disclosure
/// ([[non-ascii-literal-named-const-escape]]).
const DISCLOSURE_COLLAPSED: &str = "\u{25B8}";
/// U+25BE BLACK DOWN-POINTING SMALL TRIANGLE — an expanded branch's disclosure.
const DISCLOSURE_EXPANDED: &str = "\u{25BE}";

// ─── numeric scrub (R875) ─────────────────────────────────────────

/// The stable pixel reference the numeric scrub normalizes the captured cursor
/// against: the grid container's painted width (`GRID_TAG` is a row-wide flex
/// column, so its rect is exactly `NAME_COL_W + VALUE_COL_W`). The cursor-
/// fraction delta times this width recovers true pixel travel — the column-
/// resize "stable basis" idiom (a scrub never resizes the grid, so its width
/// is constant across the drag).
const GRID_W_PX: f64 = (NAME_COL_W + VALUE_COL_W) as f64;
/// Float scrub sensitivity: value units per pixel of horizontal drag (100 px
/// ⇒ +1.0), the Blender / Unreal "drag the number field" gesture.
const SCRUB_FLOAT_PER_PX: f64 = 0.01;
/// Int scrub sensitivity: pixels of horizontal drag per integer step (8 px ⇒
/// +1), so an int scrubs in whole units without runaway.
const SCRUB_INT_PX_PER_STEP: f64 = 8.0;

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
/// Extra External — the read-only tree-structure introspection node (R921
/// `TreeViewIntrospect`): surfaces the visible-row flatten + the roving cursor
/// to `scene/query` (`row_count` / `cursor` / `id_at.<pos>` / `level_at.<pos>` /
/// `expanded_at.<pos>` …). Collapse / cursor *mutation* routes through the
/// primary `{GRID_TAG}` coordinator (a branch row is tagged `{GRID_TAG}#{id}`
/// and `scene/key` drives the cursor), so this node owns no mutation path.
const TREE_TAG: &str = "property_grid_tree";
/// Extra External — the one shared inline text / number editor.
const EDIT_TF_TAG: &str = "property_grid_edit";
/// Extra External — the live property-name search / filter box (R872), in the
/// title band. Its text is the filter query: the grouped order source keeps
/// only the rows whose name matches, composing the R844 filter -> group chain.
const SEARCH_TF_TAG: &str = "property_grid_search";
/// Search box width inside the title band.
const SEARCH_W: u32 = 190;
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
/// R919 — composite sub-tag prefix for a row's reset arrow
/// (`{GRID_TAG}#reset{source}`), routing its click to the coordinator's `reset`.
/// A distinct prefix so `dispatch_send` never reads it as a numeric cell index.
const RESET_PREFIX: &str = "reset";

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

/// Value-model slot count: the 12 R836 scalar leaves (indices 0..12, unchanged —
/// their `value.<i>` RPC path, edit latch and reset baseline stay stable) plus
/// the R921 struct-field leaves appended at 12.. (Position Z, Scale X/Y/Z), so a
/// composite never displaces an existing leaf's `value_index`.
const VALUE_COUNT: usize = 16;

/// The property names, indexed by **value index** (the leaf-node id, the RPC
/// `name.<i>` answer). Struct-field leaves are *qualified* ("Position X") so the
/// AI-first read names a field unambiguously; the *tree* gives each field its
/// short in-context label ("X") under its struct branch (R921). Static — only
/// the [`CellValue`]s mutate, so names live in a `const`.
const PROPERTY_NAMES: [&str; VALUE_COUNT] = [
    "Name",       // 0  Identity
    "Tag",        // 1  Identity
    "Visible",    // 2  Appearance
    "Locked",     // 3  Physics
    "Layer",      // 4  Identity
    "Health",     // 5  Stats
    "Position X", // 6  Transform → Position
    "Position Y", // 7  Transform → Position
    "Opacity",    // 8  Appearance
    "Blend",      // 9  Appearance
    "Body",       // 10 Physics
    "Tint",       // 11 Appearance
    "Position Z", // 12 Transform → Position
    "Scale X",    // 13 Transform → Scale
    "Scale Y",    // 14 Transform → Scale
    "Scale Z",    // 15 Transform → Scale
];

// R837 §5.38 — the typed value model + its pure helpers (kind dispatch,
// display / edit formatting, parse, the keystroke gate, the introspect read
// / intervene write) were lifted to `pinion_core::cell_value` at the 2nd
// consumer (`hello-data-grid`); this binding consumes that SSOT.

/// First-paint property values, indexed by [value index](PROPERTY_NAMES). A
/// game-object inspector — the kinds the self-hosted editor's Details panel
/// needs (name / tag text, visibility / lock flags, layer / health ints, the
/// transform / opacity floats), now including the R921 composite **Position**
/// and **Scale** `Vector3` struct fields (12..16).
fn default_properties() -> Vec<CellValue> {
    vec![
        CellValue::Text("Player".to_owned()), // 0  Name
        CellValue::Text("hero".to_owned()),   // 1  Tag
        CellValue::Bool(true),                // 2  Visible
        CellValue::Bool(false),               // 3  Locked
        CellValue::Int(3),                    // 4  Layer
        CellValue::Int(100),                  // 5  Health
        CellValue::Float(12.5),               // 6  Position X
        CellValue::Float(-4.0),               // 7  Position Y
        CellValue::Float(1.0),                // 8  Opacity
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
        }, // 9  Blend
        CellValue::Choice {
            selected: 2,
            options: vec!["None".to_owned(), "Trigger".to_owned(), "Solid".to_owned()],
        }, // 10 Body
        // R869 — the colour cell (popup swatch palette): the object tint.
        CellValue::Color(COLOR_SWATCHES[4].0), // 11 Tint (Blue)
        // R921 — the composite struct fields (Transform → Position / Scale).
        CellValue::Float(0.0), // 12 Position Z
        CellValue::Float(1.0), // 13 Scale X
        CellValue::Float(1.0), // 14 Scale Y
        CellValue::Float(1.0), // 15 Scale Z
    ]
}

// ─── property tree (R921 §5.38 §5.50) ─────────────────────────────
//
// The Inspector is a **tree**, not a flat list: the editor's Details panel
// nests scalar properties under collapsible **category** branches (Identity /
// Appearance / Transform …) and **struct** branches (a `Vector3` like Position
// expands into its X / Y / Z field rows — the Unreal / Qt Details core depth).
// Categories and structs are the SAME thing — a collapsible branch — so the
// panel migrates off the 2-level `GroupOrderState` group-by proxy onto the
// arbitrary-depth WAI-ARIA Tree substrate (`flat_visible` / `resolve_tree_key`
// / `TreeNode`, R811/R820/R821): one visible-row sequence the paint, the
// id-keyed roving cursor, the collapse set, the a11y `tree` and the
// name filter all read, so no two derived sequences can diverge.
//
// The value model stays a flat `Vec<CellValue>` keyed by **value index** (the
// scrub / popup / inline-edit / reset / `value.<i>` RPC machinery is unchanged):
// a **leaf** node's id IS its value index in decimal ("6"), so the existing
// `{GRID_TAG}#{i}` row tag, `reset{i}` and `value.<i>` paths are byte-identical;
// a **branch** node's id is prefixed ([`CAT_PREFIX`] / [`STRUCT_PREFIX`]) so the
// id namespaces never collide and the click router / cursor disambiguate them.

/// Branch-node id prefix for a category section (`cat.Identity`). The separator
/// is `.` (not `:`): a branch id rides the composite-tag pointer wire as the
/// `send` payload's key, and that payload is `:`-delimited
/// ([`split_send_payload`]), so a `:` in the id would mis-split a header click.
const CAT_PREFIX: &str = "cat.";
/// Branch-node id prefix for a struct property (`struct.Position`).
const STRUCT_PREFIX: &str = "struct.";

/// One node of the Inspector property tree: a collapsible **branch** (a category
/// section or a struct property, `value_index = None`, `children` non-empty) or
/// an editable **leaf** (a scalar property / struct field, `value_index =
/// Some`, no children). Held in a `Signal<Vec<PropertyNode>>` carrying the
/// structure + the collapse (`expanded`) state — the *values* live in the
/// separate value-model `Signal`, keyed by `value_index`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct PropertyNode {
    /// Stable node id: a leaf's value index in decimal ("6"); a branch's
    /// prefixed label ([`CAT_PREFIX`] / [`STRUCT_PREFIX`]).
    id: String,
    /// In-context display label: a top-level property's name, a struct field's
    /// short axis ("X"), a section's category name.
    label: String,
    /// Whether this branch is expanded (a leaf is always `false`).
    expanded: bool,
    /// `Some(value_index)` for a leaf; `None` for a branch.
    value_index: Option<usize>,
    /// Child nodes (empty for a leaf).
    children: Vec<PropertyNode>,
}

impl PropertyNode {
    /// A scalar / struct-field leaf addressing value-model slot `value_index`,
    /// shown with `label`.
    fn leaf(value_index: usize, label: &str) -> Self {
        Self {
            id: value_index.to_string(),
            label: label.to_owned(),
            expanded: false,
            value_index: Some(value_index),
            children: Vec::new(),
        }
    }

    /// A collapsible branch (`id` already prefixed) with `children`, expanded by
    /// default (the Details panel boots fully open).
    fn branch(id: String, label: &str, children: Vec<PropertyNode>) -> Self {
        Self { id, label: label.to_owned(), expanded: true, value_index: None, children }
    }
}

impl TreeNode for PropertyNode {
    fn id(&self) -> &str {
        &self.id
    }
    fn label(&self) -> &str {
        &self.label
    }
    fn expanded(&self) -> bool {
        self.expanded
    }
    fn children(&self) -> &[Self] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut [Self] {
        &mut self.children
    }
    fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }
}

/// The leaf value index a tree-row id addresses, or `None` for a branch id
/// (which has a non-numeric `cat.` / `struct.` prefix). The one place the
/// "is this row an editable leaf?" decision is made, shared by the click
/// router, the keyboard activation and the a11y builder.
fn row_value_index(id: &str) -> Option<usize> {
    id.parse::<usize>().ok()
}

/// Depth-first find of the node carrying `id` in a `PropertyNode` tree — the
/// shared lookup behind the struct aggregate (field indices) and the by-id
/// collapse read. `None` for an unknown id.
fn find_node<'a>(nodes: &'a [PropertyNode], id: &str) -> Option<&'a PropertyNode> {
    for n in nodes {
        if n.id == id {
            return Some(n);
        }
        if let Some(found) = find_node(&n.children, id) {
            return Some(found);
        }
    }
    None
}

/// The value indices of a struct branch's field leaves (in order), or empty for
/// a non-struct / unknown id — the SSOT for a struct's aggregate summary, its
/// modified-from-default roll-up, and its reset-all funnel.
fn struct_field_indices(tree: &[PropertyNode], struct_id: &str) -> Vec<usize> {
    find_node(tree, struct_id)
        .map(|n| n.children.iter().filter_map(|c| c.value_index).collect())
        .unwrap_or_default()
}

/// R921 — a struct's collapsed-summary tuple, the parenthesised list of its
/// field display values (`"(12.5, -4, 0)"`). The ONE summary decision the paint
/// (struct header value cell), the RPC (`struct_summary.<id>`) and the External
/// share, so they never disagree.
fn struct_value_summary(model: &[CellValue], tree: &[PropertyNode], struct_id: &str) -> String {
    let parts: Vec<String> = struct_field_indices(tree, struct_id)
        .into_iter()
        .filter_map(|i| model.get(i).map(CellValue::display))
        .collect();
    format!("({})", parts.join(", "))
}

/// R921 — whether any field of struct `struct_id` differs from its class
/// default. The ONE roll-up the paint (struct reset arrow), the a11y (struct
/// reset `button`) and the RPC (`struct_modified.<id>`) share — the R886.1
/// one-gate extended to the struct row, so the arrow, the AT button and the
/// query can never disagree.
fn struct_is_modified(model: &[CellValue], defaults: &[CellValue], tree: &[PropertyNode], id: &str) -> bool {
    struct_field_indices(tree, id).iter().any(|&i| leaf_modified(model, defaults, i))
}

/// R921.1 — whether the leaf at `value_index` differs from its class default.
/// The ONE leaf modified-gate the paint (reset arrow), the a11y (reset
/// `button`), the External (`is_modified` / `struct_is_modified`) and the RPC
/// (`modified.<i>`) share — the R886.1 one-gate, the leaf peer of
/// [`struct_is_modified`] (lifted at R921.1 so the arrow and the AT button can
/// never disagree). Out-of-range / absent reads as not-modified.
fn leaf_modified(model: &[CellValue], defaults: &[CellValue], value_index: usize) -> bool {
    match (model.get(value_index), defaults.get(value_index)) {
        (Some(value), Some(baseline)) => property_modified(value, baseline),
        _ => false,
    }
}

/// R921 — the number of leaf (editable property) descendants of a branch — the
/// `(count)` detail a category header shows ("Identity (3)", "Transform (6)").
fn leaf_descendant_count(tree: &[PropertyNode], id: &str) -> usize {
    fn count(node: &PropertyNode) -> usize {
        if node.value_index.is_some() {
            1
        } else {
            node.children.iter().map(count).sum()
        }
    }
    find_node(tree, id).map_or(0, count)
}

/// The Inspector property tree (R921): five category branches, with the
/// **Transform** category nesting the **Position** and **Scale** `Vector3`
/// struct branches (each expanding to X / Y / Z field leaves). The structure +
/// labels SSOT — `default_tree` builds the structure, the value model holds the
/// editable values keyed by the leaf ids' value indices.
fn default_tree() -> Vec<PropertyNode> {
    let cat = |label: &str, children: Vec<PropertyNode>| {
        PropertyNode::branch(format!("{CAT_PREFIX}{label}"), label, children)
    };
    let vec3 = |label: &str, x: usize, y: usize, z: usize| {
        PropertyNode::branch(
            format!("{STRUCT_PREFIX}{label}"),
            label,
            vec![PropertyNode::leaf(x, "X"), PropertyNode::leaf(y, "Y"), PropertyNode::leaf(z, "Z")],
        )
    };
    vec![
        cat("Identity", vec![
            PropertyNode::leaf(0, "Name"),
            PropertyNode::leaf(1, "Tag"),
            PropertyNode::leaf(4, "Layer"),
        ]),
        cat("Appearance", vec![
            PropertyNode::leaf(2, "Visible"),
            PropertyNode::leaf(8, "Opacity"),
            PropertyNode::leaf(9, "Blend"),
            PropertyNode::leaf(11, "Tint"),
        ]),
        cat("Physics", vec![PropertyNode::leaf(3, "Locked"), PropertyNode::leaf(10, "Body")]),
        cat("Stats", vec![PropertyNode::leaf(5, "Health")]),
        cat("Transform", vec![vec3("Position", 6, 7, 12), vec3("Scale", 13, 14, 15)]),
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

/// R919 — the frozen baseline: the class default value of every property (the
/// same `default_properties()` the model starts from). A property is "modified"
/// when its current value differs from this baseline (the Unreal / Qt Details
/// "reset arrow" appears only on a changed property), and `reset` restores it.
/// One immutable home (an `Rc<Vec<…>>`, never a `Signal` — defaults do not
/// change) the External, the view, and the a11y all read, so the modified
/// indicator and the reset target can never disagree.
#[must_use]
fn use_property_defaults() -> Rc<Vec<CellValue>> {
    let owner = Owner::current().expect("use_property_defaults requires an active Owner scope");
    owner.cache("property_grid.defaults", default_properties)
}

/// R919 / R920 — whether `value` differs from its class default `baseline`. Uses
/// the substrate's NaN-safe value equality ([`CellValue::value_eq`], built on the
/// TOTAL order), not the derived IEEE `PartialEq`, so a `Float` of `NaN` reads as
/// unmodified against a `NaN` default (the R900 no-spurious-state discipline). The
/// 2nd consumer of `value_eq` (after the node-editor's no-op guard) — R920 lifted
/// the shared `sort_cmp == Equal` decision onto `CellValue`.
fn property_modified(value: &CellValue, baseline: &CellValue) -> bool {
    !value.value_eq(baseline)
}

/// R921 — the Inspector property **tree** SSOT (structure + per-branch collapse
/// state). Shared by the [`PropertyGridExternal`] (toggles a branch / moves the
/// cursor on a click), the view fn (flattens it for paint — reading it
/// subscribes, so a collapse repaints), the keyboard nav and the a11y tree.
/// Built once from [`default_tree`]; the editable *values* live in the separate
/// [`use_property_model`] `Signal`, keyed by the leaf ids' value indices.
#[must_use]
fn use_property_tree() -> Rc<Signal<Vec<PropertyNode>>> {
    let owner = Owner::current().expect("use_property_tree requires an active Owner scope");
    owner.cache("property_grid.tree", || Signal::new(default_tree()))
}

/// R921 — the roving keyboard cursor: the **node id** of the focused visible row
/// (a leaf value index "6" or a branch id `cat:…` / `struct:…`), or `None`
/// before the first key. Id-keyed (not a visual position) so it survives a
/// collapse / filter that reshuffles the flatten — the WAI-ARIA Tree cursor
/// model ([`resolve_tree_key`]). Read inside the view-fn it subscribes, so a
/// cursor move repaints and `access_focus_target` rings the cursor row.
#[must_use]
fn use_property_cursor() -> Rc<Signal<Option<String>>> {
    let owner = Owner::current().expect("use_property_cursor requires an active Owner scope");
    owner.cache("property_grid.cursor", || Signal::new(None))
}

/// The live filter query (the R872 search box text), trimmed + lowercased; the
/// empty string means "no filter".
fn current_search_query() -> String {
    use_text_edit_state(SEARCH_TF_TAG).text().trim().to_lowercase()
}

/// R921 — the per-node filter match: a node survives the search iff its
/// *searchable name* contains the (already-lowercased) query. A **leaf** matches
/// on its qualified [`PROPERTY_NAMES`] entry (so "position" finds "Position X"
/// even though the in-tree label is the short "X"); a **branch** matches on its
/// category / struct label. [`flat_visible_filtered`] then keeps any node on a
/// path to a match (Qt recursive-filter semantics).
fn node_matches_query(node: &PropertyNode, query: &str) -> bool {
    let name = match node.value_index {
        Some(i) => PROPERTY_NAMES.get(i).copied().unwrap_or(node.label.as_str()),
        None => node.label.as_str(),
    };
    name.to_lowercase().contains(query)
}

/// R921 — the visible-row flatten the paint, the cursor, the a11y and the
/// geometry all read (one SSOT). With no filter it is the depth-first flatten
/// honouring each branch's collapse state ([`flat_visible`]); with a live query
/// it is the recursive path-to-match flatten that reveals matches inside
/// collapsed branches ([`flat_visible_filtered`]).
fn visible_property_rows(tree: &[PropertyNode], query: &str) -> Vec<VisibleRow> {
    if query.is_empty() {
        flat_visible(tree)
    } else {
        flat_visible_filtered(tree, |n| node_matches_query(n, query))
    }
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
/// R875 / R914 — the per-drag payload the numeric scrub's [`DragCalibration`]
/// snapshots on the first capture `pointer_move`: the dragged source, its
/// [`CellKind`], and its value at press. The cursor's anchor fraction lives in
/// the [`DragCalibration`] itself; each later move applies
/// `base + travel_px · sensitivity`. `Copy` so it rides in the calibration's
/// `Cell`.
#[derive(Clone, Copy)]
struct ScrubDrag {
    source: usize,
    kind: CellKind,
    base: f64,
}

struct PropertyGridExternal {
    model: Rc<Signal<Vec<CellValue>>>,
    /// R919 — the class-default baseline ([`use_property_defaults`]). A property
    /// is modified when `model[i]` differs from `defaults[i]`; `reset` writes the
    /// baseline back through the [`set_value`](Self::set_value) funnel.
    defaults: Rc<Vec<CellValue>>,
    /// R921 — the property tree SSOT (structure + per-branch collapse). Held so
    /// a click on a branch row can toggle its expand state, and so the struct
    /// aggregate (summary / modified / reset) can read a struct's field indices.
    tree: Rc<Signal<Vec<PropertyNode>>>,
    /// R921 — the roving keyboard cursor (a visible-row node id). Held so a
    /// data-row click can move the cursor onto the clicked row.
    cursor: Rc<Signal<Option<String>>>,
    /// R921.1 — the live search/filter text (the same `TextEditState` the search
    /// box owns). Held so the RPC `query` path (no Owner) can compute the filtered
    /// visible flatten to gate `editing`/`popup_cursor` on row visibility — a
    /// collapsed/filtered edit must report as not-editing (R901.1 introspection
    /// must match paint), `.text()` reads the `Rc` with no Owner scope.
    search: Rc<TextEditState>,
    editing_row: Rc<Signal<Option<usize>>>,
    editor: Rc<TextEditState>,
    popup_cursor: Rc<Signal<Option<usize>>>,
    popup_hover: Rc<Signal<Option<usize>>>,
    /// R875 — the numeric source armed by a `PointerDown` over a numeric row,
    /// before the first `pointer_move` calibrates the drag. `None` for a press
    /// on a non-numeric row (which never scrubs).
    scrub_armed: Cell<Option<usize>>,
    /// R875 / R914 — the live scrub calibration ([`DragCalibration`]) once
    /// dragging begins; active between the first `pointer_move` and the release.
    /// Its activity at `PointerUp` distinguishes a scrub (commit, suppress the
    /// click) from a click.
    scrub_cal: DragCalibration<ScrubDrag>,
}

impl PropertyGridExternal {
    fn new(
        model: Rc<Signal<Vec<CellValue>>>,
        tree: Rc<Signal<Vec<PropertyNode>>>,
        cursor: Rc<Signal<Option<String>>>,
        editing_row: Rc<Signal<Option<usize>>>,
        editor: Rc<TextEditState>,
        popup_cursor: Rc<Signal<Option<usize>>>,
        popup_hover: Rc<Signal<Option<usize>>>,
    ) -> Self {
        Self {
            model,
            defaults: use_property_defaults(),
            tree,
            cursor,
            // Resolved via its hook (like `defaults`) — the same cached
            // `TextEditState` the search box owns (Owner::cache dedup).
            search: use_text_edit_state(SEARCH_TF_TAG),
            editing_row,
            editor,
            popup_cursor,
            popup_hover,
            scrub_armed: Cell::new(None),
            scrub_cal: DragCalibration::new(),
        }
    }

    fn count(&self) -> usize {
        self.model.get().len()
    }

    /// R921.1 — the editing leaf's value index, but only when its row is
    /// actually visible in the current flatten (not collapsed / filtered away).
    /// The visibility gate the `editing` / `popup_cursor` queries share with the
    /// paint + a11y (`popup_view_pos`): a collapsed / filtered edit reports as
    /// not-editing, so the RPC introspection matches what the screen shows
    /// (R901.1 — `editing` must point at a painted row). Reads the captured tree
    /// + search `Rc`s, so it is sound on the RPC thread (no Owner scope).
    fn editing_if_visible(&self) -> Option<usize> {
        let row = self.editing_row.get()?;
        let query = self.search.text().trim().to_lowercase();
        let id = row.to_string();
        visible_property_rows(&self.tree.get(), &query)
            .iter()
            .any(|r| r.id == id)
            .then_some(row)
    }

    /// R919 — whether the property at `source` differs from its class default
    /// (the modified-indicator predicate; the reset arrow paints only when true).
    /// Out-of-range / absent reads as not-modified.
    fn is_modified(&self, source: usize) -> bool {
        leaf_modified(&self.model.get(), &self.defaults, source)
    }

    /// R919 — whether any property is modified (the "Reset all" enable gate / the
    /// AI-first "is this object dirty?" read).
    fn any_modified(&self) -> bool {
        (0..self.count()).any(|i| self.is_modified(i))
    }

    /// R919 — reset the property at `source` to its class default, returning
    /// whether it actually changed (a no-op `false` for an already-default or
    /// out-of-range row — no spurious model churn). Routes through the shared
    /// [`set_value`](Self::set_value) funnel, so a reset is the same model write
    /// a keyboard commit / RPC `intervene value.<i>` makes.
    fn reset_to_default(&self, source: usize) -> bool {
        if !self.is_modified(source) {
            return false;
        }
        self.set_value(source, self.defaults[source].clone());
        true
    }

    /// R919 — reset every modified property to its default, returning the count
    /// reset (`0` when nothing was modified).
    fn reset_all(&self) -> usize {
        (0..self.count()).filter(|&i| self.reset_to_default(i)).count()
    }

    /// R921 — move the roving cursor onto the tree node `id` (a leaf value index
    /// or a branch id), the pointer peer of the keyboard [`resolve_tree_key`]
    /// cursor motion. The cursor is id-keyed, so it stays valid across a
    /// collapse / filter that hides the row (a hidden cursor simply rings
    /// nothing until the row reappears).
    fn move_cursor(&self, id: &str) {
        self.cursor.set(Some(id.to_owned()));
    }

    /// R921 — toggle a branch's (category / struct) collapse state through the
    /// `tree_nav` flag-store SSOT (the click peer of the keyboard `ArrowLeft` /
    /// `ArrowRight` / `Enter` expand-collapse). A leaf / unknown id is a no-op.
    fn toggle_branch(&self, id: &str) {
        toggle_expanded(&self.tree, id);
    }

    /// R921 — whether any field of struct `struct_id` is modified from its
    /// default (the struct row's reset-arrow gate — a struct is "modified" iff
    /// any of its components is, the Unreal Details struct-row roll-up). Delegates
    /// to the [`struct_is_modified`] one-gate the paint + a11y also use.
    fn struct_modified(&self, struct_id: &str) -> bool {
        struct_is_modified(&self.model.get(), &self.defaults, &self.tree.get(), struct_id)
    }

    /// R921 — reset every field of struct `struct_id` to its class default,
    /// returning the count reset. Routes each field through the shared
    /// [`reset_to_default`](Self::reset_to_default) → [`set_value`](Self::set_value)
    /// funnel, so a struct reset is N ordinary field writes (no special path).
    fn reset_struct(&self, struct_id: &str) -> usize {
        struct_field_indices(&self.tree.get(), struct_id)
            .into_iter()
            .filter(|&i| self.reset_to_default(i))
            .count()
    }

    /// R921 — a struct row's collapsed-summary value, the parenthesised tuple of
    /// its field display values (`"(12.5, -4, 0)"` for a `Vector3`), so a
    /// collapsed struct still shows its value at a glance (the Unreal / Qt
    /// Details struct-row summary). Reads the field displays through the same
    /// [`CellValue::display`] the leaf cells use.
    fn struct_summary(&self, struct_id: &str) -> String {
        struct_value_summary(&self.model.get(), &self.tree.get(), struct_id)
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

    /// Write a typed value into the model slot — the scrub's live commit. It
    /// writes the same shared model `Signal` the inline editor (`commit_edit`)
    /// and the RPC `value.<i>` intervene also write — each through its own path,
    /// converging on one source of truth (the grid has no per-column range, so
    /// unlike the data-grid's `set_cell` there is no clamp/re-anchor to funnel).
    fn set_value(&self, source: usize, value: CellValue) {
        self.model.set_with(|prev| {
            let mut next = prev.clone();
            if let Some(slot) = next.get_mut(source) {
                *slot = value;
            }
            next
        });
    }

    /// R875 — arm a numeric scrub: a `PointerDown` over a numeric (`Int` /
    /// `Float`) row records the source so the first capture `pointer_move` can
    /// calibrate. A press on a non-numeric row leaves the arm clear (it never
    /// scrubs — bool toggles, choice / colour open popups, text edits).
    fn arm_scrub(&self, source: usize) {
        // R917 — a fresh press starts a fresh calibration (self-contained scrub;
        // never inherits a stale base from a drag whose release was missed — the
        // R51.34 capture lock makes that unreachable, but the arm should not
        // depend on it).
        self.scrub_cal.end();
        let numeric = matches!(
            self.model.get().get(source).map(CellValue::kind),
            Some(CellKind::Int | CellKind::Float)
        );
        self.scrub_armed.set(numeric.then_some(source));
    }

    /// R875 / R914 — drive the live numeric scrub from the captured cursor's
    /// horizontal fraction `x_rel` across the grid (`GRID_TAG`) through the
    /// [`DragCalibration`] substrate. The first move calibrates: `seed` snapshots
    /// the armed source's kind + base value (declining — `None` — if nothing is
    /// armed or the source is no longer numeric), and the move mutates nothing
    /// (the user has not dragged yet). Each later move yields the fraction delta,
    /// which `· GRID_W_PX` recovers as pixel travel; the scrub writes
    /// `base + travel_px · sensitivity`. An int scrub steps in whole units; a
    /// float scrub is continuous.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "scrub values are small game-object property magnitudes (layer / \
                  health / transform), nowhere near f64's 2^53 exact-int limit or \
                  i64's range; the f64→i64 step is an intentional round-to-unit"
    )]
    fn scrub_to(&self, x_rel: f64) {
        let Some((drag, delta)) = self.scrub_cal.drive(x_rel, || {
            let source = self.scrub_armed.get()?;
            let model = self.model.get();
            match model.get(source) {
                Some(CellValue::Int(i)) => Some(ScrubDrag { source, kind: CellKind::Int, base: *i as f64 }),
                Some(CellValue::Float(f)) => Some(ScrubDrag { source, kind: CellKind::Float, base: *f }),
                // Nothing armed, or the armed source is no longer numeric.
                _ => None,
            }
        }) else {
            return;
        };
        // R915 — a sub-threshold press is a click, not a scrub: stay in the dead
        // zone (no mutation) until the cursor strays past DRAG_CLICK_THRESHOLD_PX,
        // so a plain click on a numeric row moves the roving cursor onto it
        // instead of nudging its value.
        if !self.is_scrubbing() {
            return;
        }
        let travel_px = delta * GRID_W_PX;
        let next = match drag.kind {
            CellKind::Int => {
                let steps = (travel_px / SCRUB_INT_PX_PER_STEP).round() as i64;
                CellValue::Int(drag.base as i64 + steps)
            }
            _ => CellValue::Float(drag.base + travel_px * SCRUB_FLOAT_PER_PX),
        };
        self.set_value(drag.source, next);
    }

    /// R875 / R915 — tear down the scrub at release. Returns whether a real
    /// scrub ran (the cursor strayed past the click dead zone), so `PointerUp`
    /// can suppress the click action: a scrub must not also open the inline
    /// editor or move the cursor. A sub-threshold press returns `false` — it was
    /// a click, so the release moves the roving cursor as usual.
    fn end_scrub(&self) -> bool {
        self.scrub_armed.set(None);
        let was_scrub = self.is_scrubbing();
        self.scrub_cal.end();
        was_scrub
    }

    /// R875 / R915 — whether a *real* numeric scrub is live: the press has
    /// strayed past `DRAG_CLICK_THRESHOLD_PX` of travel across the grid basis
    /// (`GRID_W_PX`). The one decision the scrub mutation gate, the
    /// click-suppression at release, and the AI-first `scrubbing` query share.
    fn is_scrubbing(&self) -> bool {
        self.scrub_cal.traveled_beyond(GRID_W_PX, DRAG_CLICK_THRESHOLD_PX)
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
        self.editing_row.set(Some(row));
        // R878 — `seed` = set_text + caret-at-end (the lifted pair).
        self.editor.seed(value.edit_text());
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
        self.editing_row.set(Some(row));
        self.popup_cursor.set(Some(cursor));
        self.popup_hover.set(None);
        self.editor.seed(c.to_hex());
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
    /// commits a colour), a row's reset arrow (`reset<i>` leaf / `reset<branch>`
    /// struct), a **branch** row (`cat:…` / `struct:…` toggles collapse), or a
    /// numeric leaf row (focus + bool-toggle / popup-open / `DoubleClick`-edit)
    /// — all route into this one coordinator.
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
        // R919 / R921 — a click on a row's reset arrow resets that property to
        // its default (the same `reset_*` funnel the RPC / keyboard use). The
        // suffix is a leaf value index (`reset6`) or a struct branch id
        // (`resetstruct.Position`, resetting every field).
        if let Some(rest) = key.strip_prefix(RESET_PREFIX) {
            if event_name == "PointerUp" {
                match row_value_index(rest) {
                    Some(i) => {
                        self.reset_to_default(i);
                    }
                    None => {
                        self.reset_struct(rest);
                    }
                }
            }
            return Ok(IntrospectValue::Null);
        }
        // R921 — a click on a branch row (category / struct) toggles its
        // collapse on the activation edge and moves the cursor onto it.
        if key.starts_with(CAT_PREFIX) || key.starts_with(STRUCT_PREFIX) {
            if event_name == "PointerUp" {
                self.toggle_branch(key);
                self.move_cursor(key);
            }
            return Ok(IntrospectValue::Null);
        }
        let idx: usize = key.parse().map_err(|_| InvokeError::Rejected)?;
        if idx >= self.count() {
            return Err(InvokeError::Rejected);
        }
        match event_name {
            // R875 — arm a numeric scrub; the first capture `pointer_move`
            // calibrates it. A non-numeric press leaves the arm clear.
            "PointerDown" => {
                self.arm_scrub(idx);
                Ok(IntrospectValue::Null)
            }
            "PointerUp" => {
                // R875 — a scrub committed its value live during the drag; its
                // release must NOT also fire the click action (open editor /
                // toggle / move cursor). `end_scrub` reports whether a drag ran.
                if self.end_scrub() {
                    return Ok(IntrospectValue::Null);
                }
                self.move_cursor(key);
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
            // R875 — the capture lock lets the cursor stray off the row; a
            // release there arrives as PointerLeave / PointerCancel. Tear the
            // scrub down (the value is already committed) with no click.
            "PointerLeave" | "PointerCancel" => {
                self.end_scrub();
                Ok(IntrospectValue::Null)
            }
            "DoubleClick" => Ok(IntrospectValue::Bool(self.begin_edit(idx))),
            _ => Ok(IntrospectValue::Null),
        }
    }
}

impl core::fmt::Debug for PropertyGridExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PropertyGridExternal")
            .field("row_count", &self.count())
            .field("cursor", &self.cursor.get())
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

    /// R875 — opt into the R51.34 capture lock so a numeric scrub survives the
    /// cursor straying off the row (the slider / column-resize stance). A press
    /// that never moves is still a click — the release dispatches `PointerUp`
    /// with no scrub calibrated, so the existing click path runs unchanged.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// R875 — normalize the captured cursor against the grid container
    /// (`GRID_TAG`), a stable-width rect, so the cursor-fraction delta recovers
    /// true pixel travel for the scrub (the column-resize stable-basis rule —
    /// the scrubbed cell never resizes, so the whole grid is a fine basis).
    fn capture_normalize(&self) -> CaptureNormalize<'_> {
        CaptureNormalize::Tag(GRID_TAG)
    }

    /// R875 — drive the live numeric scrub from the captured cursor's horizontal
    /// fraction across the grid; `y_rel` is ignored (scrub is the X axis only).
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

impl ExternalIntrospect for PropertyGridExternal {
    fn schema(&self) -> IntrospectSchema {
        // R921 — the *visible-row structure* (the flatten + the roving cursor)
        // is read off the sibling `TREE_TAG` `TreeViewIntrospect` (`row_count` /
        // `cursor` / `id_at.<pos>` / `level_at.<pos>` / `expanded_at.<pos>`).
        // This coordinator owns the value-index-keyed value model + the
        // per-branch collapse / struct aggregate / edit / popup state.
        IntrospectSchema::new(&[
            ("row_count", "int"),
            ("editing", "json"),
            ("name.<index>", "string"),
            ("kind.<index>", "string"),
            ("value.<index>", "json"),
            // R919 — the modified-from-default reads + the reset writes.
            ("modified.<index>", "bool"),
            ("any_modified", "bool"),
            ("reset", "int"),
            ("reset_all", "json"),
            // R921 — per-branch collapse (read + intervene + toggle) and the
            // struct aggregate (summary tuple + modified roll-up + reset-all).
            ("expanded.<branch_id>", "bool"),
            ("struct_summary.<struct_id>", "string"),
            ("struct_modified.<struct_id>", "bool"),
            ("toggle_branch", "bool"),
            ("reset_struct", "int"),
            // R921 — the roving keyboard cursor's node id (read + intervene; a
            // leaf value-index string "6" or a branch `cat.` / `struct.` id,
            // Null when unset). The AI-first cursor move (no click side effect).
            ("cursor", "string"),
            ("popup_cursor", "int"),
            // R875 — live numeric-scrub flag (true between the first drag move
            // and the release); the AI-first witness of a scrub in flight.
            ("scrubbing", "bool"),
            ("send", "string"),
            ("toggle", "int"),
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
            // R921.1 — gated on `editing_if_visible`: an edit whose row is
            // collapsed / filtered out of the flatten reports Null (it paints
            // nowhere, so advertising it would be an R901.1 introspection lie).
            "editing" => Some(IntrospectValue::Json(match self.editing_if_visible() {
                Some(row) => serde_json::Value::from(
                    i64::try_from(row).expect("row index fits in i64"),
                ),
                None => serde_json::Value::Null,
            })),
            // R921.1 — Null unless the open popup's row is visible (same gate):
            // a popup hidden by a collapse / filter paints no panel, so its
            // cursor is not reported either.
            "popup_cursor" => Some(match self.editing_if_visible().and(self.popup_cursor.get()) {
                Some(i) => {
                    IntrospectValue::Int(i64::try_from(i).expect("cursor index fits in i64"))
                }
                None => IntrospectValue::Null,
            }),
            "scrubbing" => Some(IntrospectValue::Bool(self.is_scrubbing())),
            // R921 — the roving cursor's node id (Null when unset).
            "cursor" => Some(self.cursor.get().map_or(IntrospectValue::Null, IntrospectValue::Text)),
            // R919 — any property modified from its default (the dirty read).
            "any_modified" => Some(IntrospectValue::Bool(self.any_modified())),
            _ => {
                // R919 — is property `<index>` modified from its class default?
                // (out-of-range -> `None`, like the other `<index>` reads).
                if let Some(idx_str) = path.strip_prefix("modified.") {
                    let idx: usize = idx_str.parse().ok()?;
                    if idx >= self.count() {
                        return None;
                    }
                    return Some(IntrospectValue::Bool(self.is_modified(idx)));
                }
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
                // R921 — a branch's collapse flag, by node id (`expanded.cat:…`
                // / `expanded.struct:…`). Null for a leaf / unknown id.
                if let Some(id) = path.strip_prefix("expanded.") {
                    let tree = self.tree.get();
                    return Some(
                        find_node(&tree, id)
                            .filter(|n| !n.children.is_empty())
                            .map_or(IntrospectValue::Null, |n| IntrospectValue::Bool(n.expanded)),
                    );
                }
                // R921 — a struct's collapsed-summary tuple (`struct_summary.struct.Position`).
                if let Some(id) = path.strip_prefix("struct_summary.") {
                    if struct_field_indices(&self.tree.get(), id).is_empty() {
                        return Some(IntrospectValue::Null);
                    }
                    return Some(IntrospectValue::Text(self.struct_summary(id)));
                }
                // R921 — whether any of a struct's fields is modified.
                if let Some(id) = path.strip_prefix("struct_modified.") {
                    if struct_field_indices(&self.tree.get(), id).is_empty() {
                        return Some(IntrospectValue::Null);
                    }
                    return Some(IntrospectValue::Bool(self.struct_modified(id)));
                }
                None
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "row_count" | "editing" | "popup_cursor" => Err(InterveneError::ReadOnly),
            // R921 — move the roving cursor to a node id (the AI-first cursor
            // move, no click side effect); Null clears it. A stale / off-screen
            // id is stored as given (it simply rings nothing), the tree-cursor
            // contract.
            "cursor" => match value {
                IntrospectValue::Text(id) => {
                    self.cursor.set(Some(id));
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.cursor.set(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            _ => {
                // R921 — set a branch's collapse flag by node id (admin / restore;
                // `set_expanded_in` is a no-op on a leaf / redundant set). A
                // non-Bool is a type mismatch; an unknown branch is a silent
                // no-op (symmetric with the `query` Null), so only a non-`value.`
                // / non-`expanded.` path is a malformed path.
                if let Some(id) = path.strip_prefix("expanded.") {
                    return match value {
                        IntrospectValue::Bool(b) => {
                            set_expanded_in(&self.tree, id, b);
                            Ok(())
                        }
                        _ => Err(InterveneError::TypeMismatch),
                    };
                }
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
            // Toggle the bool at a given source row (the `Space` keyboard path
            // resolves the cursor → source and passes it; the RPC affordance
            // names the row directly — deterministic, no hidden focused-row
            // dependency). No-op (returns `false`) on a non-bool / out-of-range
            // row.
            "toggle" => match args {
                IntrospectValue::Int(i) => {
                    let row = usize::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    Ok(IntrospectValue::Bool(self.toggle(row)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R919 — reset the property at a given source row to its class
            // default (the RPC twin of clicking its reset arrow). `false` when the
            // row is already default / out of range. The keyboard + click paths
            // route here too, so a reset is one funnel.
            "reset" => match args {
                IntrospectValue::Int(i) => {
                    let row = usize::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    Ok(IntrospectValue::Bool(self.reset_to_default(row)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R919 — reset every modified property; returns the count reset.
            "reset_all" => Ok(IntrospectValue::Int(
                i64::try_from(self.reset_all()).expect("reset count fits in i64"),
            )),
            // R921 — toggle a branch's (category / struct) collapse by node id
            // (the RPC twin of clicking the disclosure / the keyboard
            // ArrowLeft/Right). Returns the resulting expanded flag; a leaf /
            // unknown id is a no-op returning `false`.
            "toggle_branch" => match args {
                IntrospectValue::Text(ref id) => {
                    self.toggle_branch(id);
                    let expanded = find_node(&self.tree.get(), id).is_some_and(|n| n.expanded);
                    Ok(IntrospectValue::Bool(expanded))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R921 — reset every field of a struct to its default by struct id
            // (the RPC twin of the struct row's reset arrow); returns the count
            // reset. Routes each field through the shared `reset_to_default`.
            "reset_struct" => match args {
                IntrospectValue::Text(ref id) => Ok(IntrospectValue::Int(
                    i64::try_from(self.reset_struct(id)).expect("reset count fits in i64"),
                )),
                _ => Err(InvokeError::TypeMismatch),
            },
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
    // R921.1 — gate on `popup_view_pos` (the same visibility predicate the paint
    // + a11y use): a popup whose row is collapsed / filtered out of the flatten
    // paints no panel, so it must NOT intercept the grid keymap (else an
    // invisible popup hijacks the arrows and traps tree navigation — the row
    // can never be re-expanded by keyboard). Only a *visible* open popup keeps
    // the keymap.
    let (row, _) = popup_view_pos(use_editing_row().get())?;
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

/// Grid-focused keymap (R921): the roving cursor moves over the flattened
/// tree (category headers + struct headers + visible leaf rows). An open
/// choice / colour popup intercepts the keymap first. Otherwise:
///
/// - On a **leaf** row, `Space` toggles a bool, `Enter` / `F2` edits a text /
///   int / float leaf or opens a choice / colour popup, and `Delete` resets it
///   — routed through the coordinator's `invoke` so the keyboard path is
///   identical to the RPC path. (Leaves are editable cells, so these keys
///   activate the cell rather than re-affirming a selection.)
/// - Everything else — `ArrowUp` / `ArrowDown` / `Home` / `End` movement over
///   the whole flatten, and `ArrowRight` / `ArrowLeft` / `Enter` / `Space` on a
///   **branch** (category / struct) to expand / collapse / descend — is the
///   shared WAI-ARIA Tree [`resolve_tree_key`] policy, so the collapse +
///   roving semantics match every other tree consumer.
fn apply_key_grid(scene: &mut Scene, key: &str) -> bool {
    if let Some((row, kind)) = open_popup_kind() {
        return match kind {
            CellKind::Color => apply_key_color(row, key),
            _ => apply_key_choice(row, key),
        };
    }
    let tree = use_property_tree();
    let cursor = use_property_cursor();
    let query = current_search_query();
    let rows = visible_property_rows(&tree.get(), &query);
    if rows.is_empty() {
        return false;
    }
    let cursor_id = cursor.get();
    // In-cell activation when the cursor rests on a leaf (editable) row. A
    // branch cursor falls through to the tree policy (Space / Enter toggle it).
    if let Some(vi) = cursor_id.as_deref().and_then(row_value_index) {
        match key {
            "Space" => return activate_source(scene, vi, false),
            "Enter" | "F2" => return activate_source(scene, vi, true),
            // R920 — Delete resets the cursor leaf to its class default (the
            // keyboard path to the reset arrow, which is not itself a tab stop).
            "Delete" => return reset_source(scene, vi),
            _ => {}
        }
    }
    // The WAI-ARIA Tree keyboard policy over the visible flatten (clamp, not
    // wrap — the tree has ends). `page = rows.len()` (non-virtualized) so
    // PageUp / PageDown jump to the ends.
    match resolve_tree_key(&rows, cursor_id.as_deref(), key, rows.len()) {
        TreeKey::Focus(id) => {
            cursor.set(Some(id));
            true
        }
        TreeKey::Expand(id) => {
            set_expanded_in(&tree, &id, true);
            true
        }
        TreeKey::Collapse(id) => {
            set_expanded_in(&tree, &id, false);
            true
        }
        TreeKey::Toggle(id) => {
            toggle_expanded(&tree, &id);
            true
        }
        TreeKey::Consumed => true,
        TreeKey::Unhandled => false,
    }
}

/// Activate the leaf at value index `row`: toggle a bool, open a choice /
/// colour popup, or (when `allow_edit`) enter edit mode on a text / int / float
/// leaf. Routes through the coordinator's `invoke` so toggle / begin live in
/// one place (the RPC path).
fn activate_source(scene: &mut Scene, row: usize, allow_edit: bool) -> bool {
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
    let arg = IntrospectValue::Int(i64::try_from(row).expect("row index fits in i64"));
    match kind {
        CellKind::Bool => intro.invoke("toggle", arg).is_ok(),
        // A choice / colour row opens its popup on both Space and Enter (the
        // dropdown affordance) — `allow_edit` only gates the text editors.
        CellKind::Choice | CellKind::Color => intro.invoke("begin", arg).is_ok(),
        _ if allow_edit => intro.invoke("begin", arg).is_ok(),
        _ => false,
    }
}

/// R920 — reset the data row at stable source index `row` to its class default
/// via the coordinator's `reset` invoke (the keyboard twin of the reset-arrow
/// click + the RPC — one funnel). The reset arrow's AccessNode is not itself a
/// tab stop, so this `Delete`-on-the-cursor-row path is how a keyboard / AT user
/// reaches reset. Consumes the key on any data row (a no-op on an already-default
/// row), so `Delete` never falls through to navigation.
fn reset_source(scene: &mut Scene, row: usize) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(GRID_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    let _ = intro.invoke("reset", IntrospectValue::Int(i64::try_from(row).expect("row fits in i64")));
    true
}

/// Edit-mode keymap over the shared inline field — the lifted
/// [`pinion_core::edit_field_keymap`] SSOT (R878; this binding carried one
/// of the two pre-lift copies): Enter commits, Escape cancels, caret /
/// deletion keys always reach the field, printable keys pass the int /
/// float keystroke gate (text rows accept everything). A defensive "no row
/// is editing" resolves to [`CellKind::Bool`] (accepts no keystroke).
fn apply_key_edit(scene: &mut Scene, key: &str, modifiers: Modifiers) -> bool {
    let kind = editing_kind().unwrap_or(CellKind::Bool);
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

/// R872 — the search-box keymap (the live property filter). Every printable /
/// editing key flows into the field (no numeric gate — a name search accepts
/// anything), and the order source re-filters reactively on the text change.
/// `Escape` clears the filter and returns focus to the grid; `Enter` (the
/// filter is already live) just hands focus back to the grid.
fn apply_key_search(scene: &mut Scene, key: &str, modifiers: Modifiers) -> bool {
    match key {
        "Escape" => {
            use_text_edit_state(SEARCH_TF_TAG).set_text(String::new());
            pinion_core::focus_request::request(GRID_TAG);
            true
        }
        "Enter" => {
            pinion_core::focus_request::request(GRID_TAG);
            true
        }
        other => pinion_core::forward_key_to_field(scene, SEARCH_TF_TAG, other, modifiers),
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

/// One leaf property row: `[ name cell | value cell ]`, tagged
/// `property_grid#<value_index>` (`row.id`, the leaf node id) so a click routes
/// to the coordinator. `row.label` is the in-context name (a struct field's
/// short "X", a top-level property's full name); `row.depth` insets the name
/// cell under its branch. The value cell paints the shared inline field while
/// editing, else a checkbox glyph (bool) or the value text.
fn view_row(
    row: &VisibleRow,
    value: &CellValue,
    is_focused: bool,
    edit_active: bool,
    modified: bool,
    theme: &Theme,
    edit_field: (TextFieldState, u32),
) -> Scene {
    let mut name_children: Vec<Scene> = Vec::new();
    let indent_px = row.depth * INDENT_STEP;
    if indent_px > 0 {
        name_children.push(Scene::Container(
            ContainerNode::new(Vec::new())
                .with_layout(LayoutStyle::new().with_size(Size::px(indent_px, ROW_H))),
        ));
    }
    name_children.push(Scene::Text(TextNode::styled(
        row.label.as_str(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(CELL_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    )));
    let name_cell = Scene::Container(
        ContainerNode::new(name_children)
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

    // R919 — a modified row paints its reset arrow at the trailing edge: a
    // clickable accent mark (`{GRID_TAG}#reset<index>`) whose presence IS the
    // modified indicator (the Unreal / Qt "reset to default" arrow appears only
    // on a changed property). Absolutely positioned, so it overlays the row's
    // trailing padding without shifting the Property / Value column layout, and
    // last in paint order so its click wins over the row beneath it.
    let mut children = vec![name_cell, value_cell];
    if modified {
        children.push(reset_arrow(&row.id, theme));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("{GRID_TAG}#{}", row.id))
            .with_style(BoxStyle::filled(row_fill(theme, is_focused)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(NAME_COL_W + VALUE_COL_W, ROW_H)),
            ),
    )
}

/// R919 / R921 — the reset arrow for a modified row, a small accent mark at the
/// row's trailing edge tagged `{GRID_TAG}#reset<suffix>` so a click routes to
/// the coordinator's reset funnel (`suffix` = a leaf value index "6" or a struct
/// branch id `struct.Position`). Painted only for a modified row, so its
/// presence doubles as the modified indicator; its a11y `button` peer is gated
/// on the same predicate (R886.1 one-gate).
fn reset_arrow(suffix: &str, theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(format!("{GRID_TAG}#{RESET_PREFIX}{suffix}"))
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Accent)))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(
                        (NAME_COL_W + VALUE_COL_W).saturating_sub(RESET_DOT_X),
                        ROW_H.saturating_sub(RESET_DOT) / 2,
                    )
                    .with_size(Size::px(RESET_DOT, RESET_DOT)),
            ),
    )
}

/// R921 — a **struct** branch header row: `[ ⟨disclosure⟩ name | (x, y, z)
/// summary ]`, tagged `{GRID_TAG}#<id>` (the `row`'s node id) so a click toggles
/// its collapse. Indented by the row's depth, with the disclosure glyph
/// reflecting its expanded flag, the collapsed-value `summary` in the value
/// column, and — when `modified` — the reset arrow (resetting every field). The
/// Unreal / Qt Details struct row.
fn struct_header_row(row: &VisibleRow, summary: &str, modified: bool, is_focused: bool, theme: &Theme) -> Scene {
    let struct_id = row.id.as_str();
    let label = row.label.as_str();
    let glyph = if row.expanded { DISCLOSURE_EXPANDED } else { DISCLOSURE_COLLAPSED };
    let mut name_children: Vec<Scene> = Vec::new();
    let indent_px = row.depth * INDENT_STEP;
    if indent_px > 0 {
        name_children.push(Scene::Container(
            ContainerNode::new(Vec::new())
                .with_layout(LayoutStyle::new().with_size(Size::px(indent_px, ROW_H))),
        ));
    }
    name_children.push(Scene::Text(TextNode::styled(
        format!("{glyph}  {label}"),
        Rect::default(),
        TextStyle::new().with_size_px(CELL_PX).with_fg(theme.resolve(ColorRole::OnSurface)),
    )));
    let name_cell = Scene::Container(ContainerNode::new(name_children).with_layout(
        LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_align_items(AlignItems::Center)
            .with_padding(Rect::new(CELL_PAD, 0, CELL_PAD, 0))
            .with_size(Size::px(NAME_COL_W, ROW_H)),
    ));
    let value_cell = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            summary,
            Rect::default(),
            TextStyle::new().with_size_px(CELL_PX).with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_padding(Rect::new(CELL_PAD, 0, CELL_PAD, 0))
                .with_size(Size::px(VALUE_COL_W, ROW_H)),
        ),
    );
    let mut children = vec![name_cell, value_cell];
    if modified {
        children.push(reset_arrow(struct_id, theme));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("{GRID_TAG}#{struct_id}"))
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
    view_pos: usize,
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
    let (x, y) = popup_origin(view_pos, panel_h);
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
    view_pos: usize,
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
    let (x, y) = popup_origin(view_pos, panel_h);
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

/// The top-left of a popup of height `panel_h` for the editing row at flatten
/// **visual position** `view_pos`: anchored at the value column, dropping below
/// the row — or flipped above it when it would overflow the window bottom (the
/// native dropdown edge behaviour). Shared by the choice + colour popups;
/// deterministic because every row (the column header, the category headers,
/// and the data rows) shares the uniform [`ROW_H`] + [`ROW_GAP`] pitch and the
/// title band has a fixed height ([`TITLE_H`]). The leading `row_step` skips
/// the `Property` / `Value` column-header row above the flatten.
fn popup_origin(view_pos: usize, panel_h: u32) -> (u32, u32) {
    let x = PANEL_PAD + GRID_BORDER + NAME_COL_W;
    let row_step = ROW_H + ROW_GAP;
    let grid_top = PANEL_PAD + TITLE_H + TITLE_GAP + GRID_BORDER;
    let row_top = grid_top + row_step + u32::try_from(view_pos).expect("row fits in u32") * row_step;
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

/// The popup overlay (a full-window light-dismiss barrier + the open choice /
/// colour panel) for the editing row, anchored at its **visual position** in
/// the current flatten. Empty when nothing is editing or the editing row's
/// category is collapsed (the row is hidden, so no popup is shown). The barrier
/// sorts first so the panel hit-tests on top; a click outside the panel routes
/// `dismiss` to the coordinator (the toggle-close convention).
/// R873 — the open popup's editing row resolved to `(source, visual_position)`,
/// or `None` when nothing is editing **or** the editing row is filtered /
/// collapsed out of the current flatten (its category hidden). The single SSOT
/// the popup *paint* ([`view_popup_overlay`]) and the popup *a11y*
/// ([`PropertyGridView::access_node`] / `access_focus_target`) both gate on, so
/// a popup whose row is hidden is neither painted nor announced — the paint
/// scene and the AT `aria-activedescendant` cannot diverge (the bug an
/// RPC-driven filter on an open popup would otherwise expose).
fn popup_view_pos(editing: Option<usize>) -> Option<(usize, usize)> {
    let row = editing?;
    let id = row.to_string();
    let tree = use_property_tree();
    let rows = visible_property_rows(&tree.get(), &current_search_query());
    let pos = rows.iter().position(|r| r.id == id)?;
    Some((row, pos))
}

/// R867/R869/R873 — the open popup's `listbox` a11y nodes (choice options or
/// colour swatches), or empty when nothing is editing **or** the editing row is
/// filtered / collapsed out of the flatten. Gated on [`popup_view_pos`] (the
/// SSOT the paint uses) so the AT `listbox` is never emitted for a popup the
/// screen does not show.
fn popup_listbox_nodes(model: &[CellValue]) -> Vec<AccessNode> {
    let Some((row, _)) = popup_view_pos(use_editing_row().get()) else { return Vec::new() };
    match model.get(row) {
        Some(CellValue::Choice { selected, options }) => {
            let cursor = use_popup_cursor().get().unwrap_or(*selected);
            let hover = use_popup_hover().get();
            let tags: Vec<String> =
                (0..options.len()).map(|i| format!("{GRID_TAG}#{CHOICE_OPT_PREFIX}{i}")).collect();
            let opts: Vec<ListOption<'_>> = options
                .iter()
                .enumerate()
                .map(|(i, label)| ListOption {
                    tag: &tags[i],
                    label: Some(label.as_str()),
                    state: if hover == Some(i) { ListboxItemState::Hover } else { ListboxItemState::Idle },
                    selected: *selected == i,
                    focused: cursor == i,
                })
                .collect();
            let name = format!("{} options", PROPERTY_NAMES[row]);
            listbox_option_nodes(CHOICE_POPUP_TAG, &name, false, &opts)
        }
        Some(CellValue::Color(current)) => {
            let cursor = use_popup_cursor().get().unwrap_or(0);
            let hover = use_popup_hover().get();
            let tags: Vec<String> =
                (0..COLOR_SWATCHES.len()).map(|i| format!("{GRID_TAG}#{COLOR_SW_PREFIX}{i}")).collect();
            let opts: Vec<ListOption<'_>> = COLOR_SWATCHES
                .iter()
                .enumerate()
                .map(|(i, &(color, label))| ListOption {
                    tag: &tags[i],
                    label: Some(label),
                    state: if hover == Some(i) { ListboxItemState::Hover } else { ListboxItemState::Idle },
                    selected: color == *current,
                    focused: cursor == i,
                })
                .collect();
            let name = format!("{} swatches", PROPERTY_NAMES[row]);
            listbox_option_nodes(COLOR_POPUP_TAG, &name, false, &opts)
        }
        _ => Vec::new(),
    }
}

fn view_popup_overlay(
    editing: Option<usize>,
    model: &[CellValue],
    edit_field: (TextFieldState, u32),
    theme: &Theme,
) -> Vec<Scene> {
    let Some((row, view_pos)) = popup_view_pos(editing) else { return Vec::new() };
    match model.get(row) {
        Some(CellValue::Choice { selected, options }) => {
            let cursor = use_popup_cursor().get().unwrap_or(*selected);
            let hover = use_popup_hover().get();
            vec![
                dismiss_barrier(POPUP_DISMISS_TAG, (0, 0), (WIN_W, WIN_H)),
                view_choice_popup(view_pos, options, *selected, cursor, hover, theme),
            ]
        }
        Some(CellValue::Color(c)) => {
            let cursor = use_popup_cursor().get().unwrap_or(0);
            let hover = use_popup_hover().get();
            vec![
                dismiss_barrier(POPUP_DISMISS_TAG, (0, 0), (WIN_W, WIN_H)),
                view_color_popup(view_pos, *c, cursor, hover, edit_field, theme),
            ]
        }
        _ => Vec::new(),
    }
}

/// Fixed-height title band — the "Inspector" label + the live search box
/// (R872). Fixed height keeps the row → choice-popup anchor math deterministic
/// (a bare Text node's height is font-metric dependent), and hosting the search
/// box here (not above the grid) keeps `popup_origin` unchanged — the grid does
/// not shift down.
fn view_title(search_state: TextFieldState, search_caret: u32, theme: &Theme) -> Scene {
    let title_label = Scene::Text(TextNode::styled(
        "Inspector",
        Rect::default(),
        TextStyle::new().with_size_px(TITLE_PX).with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let search_style = tf_paint::TextFieldStyle {
        field_w: SEARCH_W,
        field_h: TITLE_H - 4,
        ..tf_paint::TextFieldStyle::m3_filled()
    };
    let search_field =
        tf_paint::view_field(SEARCH_TF_TAG, search_state, search_caret, theme, &search_style, "Filter");
    Scene::Container(
        ContainerNode::new(vec![title_label, search_field]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::SpaceBetween)
                .with_size(Size::px(NAME_COL_W + VALUE_COL_W, TITLE_H)),
        ),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: RootState, _frame: &Frame) -> Scene {
    let ((edit_state, edit_caret), (search_state, search_caret)) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let model = use_property_model().get();
    // R919 — the class-default baseline, to paint a reset arrow on any row whose
    // value differs from it.
    let defaults = use_property_defaults();
    // R921 — the property tree + the live name filter flatten to the one visible
    // row sequence the paint, the cursor and the a11y share. Reading the tree
    // `Signal` (collapse), the cursor `Signal` and the search text inside the
    // view-fn subscribes, so a collapse / cursor move / filter change repaints.
    let tree_nodes = use_property_tree().get();
    let visible = visible_property_rows(&tree_nodes, &current_search_query());
    let cursor_id = use_property_cursor().get();
    let editing = use_editing_row().get();

    let title = view_title(search_state, search_caret, &theme);

    let mut rows: Vec<Scene> = Vec::with_capacity(visible.len() + 1);
    rows.push(view_header(&theme));
    for vr in &visible {
        let is_focused = cursor_id.as_deref() == Some(vr.id.as_str());
        if let Some(vi) = row_value_index(&vr.id) {
            // A leaf (editable) row.
            let value = &model[vi];
            let edit_active = editing == Some(vi) && value.kind().is_text_editable();
            let modified = leaf_modified(&model, &defaults, vi);
            rows.push(view_row(vr, value, is_focused, edit_active, modified, &theme, (edit_state, edit_caret)));
        } else if vr.id.starts_with(STRUCT_PREFIX) {
            // A struct branch row: disclosure + name + the collapsed-value tuple.
            let summary = struct_value_summary(&model, &tree_nodes, &vr.id);
            let modified = struct_is_modified(&model, &defaults, &tree_nodes, &vr.id);
            rows.push(struct_header_row(vr, &summary, modified, is_focused, &theme));
        } else {
            // A category branch row: the full-width section header (`collapsed`
            // is the inverse of `expanded`; a filter auto-expands branches).
            rows.push(group_header_row(
                format!("{GRID_TAG}#{}", vr.id),
                &vr.label,
                &leaf_descendant_count(&tree_nodes, &vr.id).to_string(),
                !vr.expanded,
                &theme,
                NAME_COL_W + VALUE_COL_W,
                ROW_H,
            ));
        }
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

    // A choice / colour popup floats over the grid (absolutely positioned)
    // with a full-window light-dismiss barrier beneath it — the barrier is
    // pushed first so the panel hit-tests on top; a click outside the panel
    // routes `dismiss` to the coordinator (the toggle-close convention).
    let mut children = vec![title, grid];
    children.extend(view_popup_overlay(editing, &model, (edit_state, edit_caret), &theme));

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

/// Cached paint posture for the two text fields — `(inline-cell-editor,
/// search-box)`, each `(interaction-state, caret)`. The model / cursor /
/// edit-mode / filter query are read reactively in the view fn (the todomvc
/// shape: read_state carries the field postures, hooks carry the reactive
/// model + the search text).
type RootState = ((TextFieldState, u32), (TextFieldState, u32));

struct PropertyGridView;

impl WidgetCore for PropertyGridView {
    type State = RootState;
    // Editing flows through apply_key + the pointer router; the coordinator's
    // intents are observed in `update`. No keybinding-channel events.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let model = use_property_model();
        let tree = use_property_tree();
        let cursor = use_property_cursor();
        let editing = use_editing_row();
        let editor = use_text_edit_state(EDIT_TF_TAG);
        let popup_cursor = use_popup_cursor();
        let popup_hover = use_popup_hover();
        Box::new(PropertyGridExternal::new(
            model,
            tree,
            cursor,
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
        let search_state = use_text_edit_state(SEARCH_TF_TAG);
        let search_blink = use_caret_blink(SEARCH_TF_TAG);
        // R921 — resolve the tree / cursor / filter `Rc`s NOW (this hook runs in
        // an Owner scope); the introspection closures run at RPC-query time
        // *outside* any Owner, so they read the captured `Rc`s rather than call a
        // `use_*` hook (which would `Owner::current().expect` panic).
        let tree = use_property_tree();
        let cursor = use_property_cursor();
        let filter = use_text_edit_state(SEARCH_TF_TAG);
        vec![
            // R921 — the read-only tree-structure introspection node: surfaces
            // the visible-row flatten + the roving cursor to `scene/query`
            // (`row_count` / `id_at.<pos>` / `level_at.<pos>` / `expanded_at.<pos>`
            // / `cursor`), so an AI client walks the Inspector's hierarchy as
            // data. Collapse / cursor *mutation* routes through the primary
            // `{GRID_TAG}` coordinator (the branch click wire + `scene/key`).
            tree_view_introspection_extra(
                TREE_TAG,
                move || {
                    let query = filter.text().trim().to_lowercase();
                    visible_property_rows(&tree.get(), &query)
                },
                move || cursor.get(),
            ),
            ExtraExternal::new(
                EDIT_TF_TAG,
                Box::new(
                    TextFieldExternal::new()
                        .attach_state(editor_state)
                        .attach_blink(blink)
                        .with_blur_intent(),
                ),
            ),
            // R872 — the live search / filter box. No commit-on-blur intent:
            // the filter is live (every keystroke re-filters), not commit-gated.
            ExtraExternal::new(
                SEARCH_TF_TAG,
                Box::new(TextFieldExternal::new().attach_state(search_state).attach_blink(search_blink)),
            ),
        ]
    }

    fn read_state(scene: &Scene) -> RootState {
        (
            tf_paint::read_text_field_state(scene, EDIT_TF_TAG),
            tf_paint::read_text_field_state(scene, SEARCH_TF_TAG),
        )
    }

    fn view(state: RootState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-property-grid (R921 §5.38 inspector detail tree)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// The grid is one Tab stop; the inline editor is the second (focusable
    /// only while painted — entered through `focus_request`, the todomvc
    /// dynamic-editor shape).
    fn focusable_tags() -> Vec<&'static str> {
        vec![GRID_TAG, EDIT_TF_TAG, SEARCH_TF_TAG]
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
            Some(SEARCH_TF_TAG) => apply_key_search(scene, key, modifiers),
            _ => false,
        }
    }

    /// Route IME composition to whichever text field owns focus — the inline
    /// cell editor or the R872 search box — through the lifted R764.1 SSOT
    /// (the todomvc two-field shape). R878 audit: the pre-R878 hand-rolled
    /// copy of the reformat block also only knew `EDIT_TF_TAG`, leaving the
    /// search box IME-deaf; the focus-routed forward closes both.
    fn apply_composition(
        scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        let target = match focused {
            Some(EDIT_TF_TAG) => EDIT_TF_TAG,
            Some(SEARCH_TF_TAG) => SEARCH_TF_TAG,
            _ => return false,
        };
        tf_paint::forward_composition_to_field(scene, target, event)
    }
}

/// R921 — a visible row's WAI-ARIA accessible name. The single-column `tree`
/// folds the row's VALUE into its name so an AT user hears it without a separate
/// `gridcell`: a leaf → `"Position X: 12.5"` (the qualified [`PROPERTY_NAMES`]
/// entry, since a struct field's in-tree label is the short "X"), a struct →
/// `"Position (12.5, -4, 0)"`, a category → `"Identity (3)"`.
fn row_access_name(row: &VisibleRow, model: &[CellValue], tree: &[PropertyNode]) -> String {
    if let Some(vi) = row_value_index(&row.id) {
        let name = PROPERTY_NAMES.get(vi).copied().unwrap_or(row.label.as_str());
        model.get(vi).map_or_else(|| name.to_owned(), |v| format!("{name}: {}", v.display()))
    } else if row.id.starts_with(STRUCT_PREFIX) {
        format!("{} {}", row.label, struct_value_summary(model, tree, &row.id))
    } else {
        format!("{} ({})", row.label, leaf_descendant_count(tree, &row.id))
    }
}

impl WidgetA11y for PropertyGridView {
    /// R921 §5.40 §5.50 — the Inspector lowers to a WAI-ARIA `tree` through the
    /// lifted [`tree_access_nodes`] SSOT: each visible row is a `treeitem` with
    /// its hierarchical axes (`aria-level` / `aria-expanded` / `aria-posinset` /
    /// `aria-setsize`). The single-column tree folds each row's value into its
    /// accessible name ([`row_access_name`]), so a leaf, a struct summary and a
    /// category count are all announced. The roving cursor's
    /// `aria-activedescendant` is supplied by [`Self::access_focus_target`]; the
    /// Inspector has no selection model, so no row carries `aria-selected`.
    ///
    /// R867 — an open choice popup additionally lowers to a WAI-ARIA
    /// `listbox` (the lifted [`listbox_option_nodes`] SSOT, the combobox a11y
    /// shape): each option carries `aria-selected` (the committed value), and
    /// `access_focus_target` points the active descendant at the cursor
    /// option / swatch.
    fn access_node(state: &RootState, focused: Option<&str>) -> Vec<AccessNode> {
        let model = use_property_model().get();
        let defaults = use_property_defaults();
        let tree_nodes = use_property_tree().get();
        let rows = visible_property_rows(&tree_nodes, &current_search_query());
        let cursor = use_property_cursor().get();
        // The single-column tree names each row with its value folded in. The
        // row tags (`{GRID_TAG}#{id}`) match the painted rows, so bounds resolve.
        let labelled: Vec<VisibleRow> = rows
            .iter()
            .map(|r| VisibleRow { label: row_access_name(r, &model, &tree_nodes), ..r.clone() })
            .collect();
        let mut nodes = tree_access_nodes(
            GRID_TAG,
            GRID_TAG,
            Some("Inspector"),
            &labelled,
            None, // no selection model — the cursor is keyboard focus only
            cursor.as_deref(),
        );
        // R919 / R921 — each modified *visible* row (leaf or struct) gains a
        // reset `button` child (`{GRID_TAG}#reset<id>`), named for the property /
        // struct and gated on the SAME modified predicate the painted reset arrow
        // uses, so the AT tree advertises reset exactly when it paints (R886.1
        // one-gate). A collapsed / filtered-out row emits neither.
        for r in &rows {
            let (modified, name) = if let Some(vi) = row_value_index(&r.id) {
                let m = leaf_modified(&model, &defaults, vi);
                (m, PROPERTY_NAMES.get(vi).copied().unwrap_or(r.label.as_str()).to_owned())
            } else if r.id.starts_with(STRUCT_PREFIX) {
                (struct_is_modified(&model, &defaults, &tree_nodes, &r.id), r.label.clone())
            } else {
                continue; // a category is never modified-resettable
            };
            if !modified {
                continue;
            }
            let reset_tag = format!("{GRID_TAG}#{RESET_PREFIX}{}", r.id);
            // The row node tag must equal what `tree_access_nodes` emitted — use
            // its `tree_row_tag` SSOT so they cannot drift (R921.1 use-substrate).
            let row_tag = tree_row_tag(GRID_TAG, &r.id);
            if let Some(node) = nodes.iter_mut().find(|n| n.tag == row_tag) {
                node.children.push(reset_tag.clone());
            }
            nodes.push(
                AccessNode::new(reset_tag, AriaRole::Button)
                    .with_name(format!("Reset {name} to default")),
            );
        }
        // R873 — the live search box is a Tab stop; emit its textbox node (the
        // lifted `text_field_a11y_node` SSOT) so an AT user who tabs into it
        // hears a named `textbox` with the current query, not a silent node.
        let ((_, _), (search_posture, _)) = *state;
        nodes.push(
            tf_paint::text_field_a11y_node(
                SEARCH_TF_TAG,
                use_text_edit_state(SEARCH_TF_TAG).text(),
                search_posture,
                focused == Some(SEARCH_TF_TAG),
            )
            .with_name("Filter properties"),
        );
        // R873 — a polite live region reporting the filtered leaf-row count, so
        // the filter narrowing / emptying the set is announced (the search/
        // filter APG pattern). Recomputed from the live flatten.
        let data_count = rows.iter().filter(|r| row_value_index(&r.id).is_some()).count();
        nodes.push(
            AccessNode::new("pg_search_status", AriaRole::Status)
                .with_name(format!("{data_count} properties")),
        );
        // The open choice / colour popup's `listbox` nodes (gated on the same
        // `popup_view_pos` visibility predicate the paint uses, so the AT tree
        // never advertises an unpainted popup — R873).
        nodes.extend(popup_listbox_nodes(&model));
        nodes
    }

    /// R870 — composite focus: while the grid owns shell focus, the
    /// `aria-activedescendant` follows the keyboard cursor. With a popup open
    /// it names the active popup option / swatch; otherwise it names the
    /// focused value cell (the roving row cursor). This is the authoritative
    /// active-descendant channel (the per-node `with_focused` flag is a
    /// redundant marker the AT layer does not lower) — the combobox / tree
    /// pattern, previously missing here.
    fn access_focus_target(_state: &RootState, focused: Option<&str>) -> Option<AccessFocus> {
        // A different element (the search box / inline editor) owns focus → ring
        // it atomically (the `grouped_focus_target` non-owner arm).
        if focused != Some(GRID_TAG) {
            return focused.map(AccessFocus::atomic);
        }
        // A popup open while the grid holds focus → the active descendant is the
        // cursor option / swatch in the popup (the combobox a11y shape, R870).
        // Only when that popup is actually visible (its row not filtered /
        // collapsed away) — the same `popup_view_pos` gate the paint + access_node
        // use (R873).
        if let Some((row, _)) = popup_view_pos(use_editing_row().get()) {
            let cur = use_popup_cursor().get().unwrap_or(0);
            match use_property_model().get().get(row).map(CellValue::kind) {
                Some(CellKind::Choice) => {
                    return Some(AccessFocus::composite(
                        GRID_TAG,
                        format!("{GRID_TAG}#{CHOICE_OPT_PREFIX}{cur}"),
                    ));
                }
                Some(CellKind::Color) => {
                    return Some(AccessFocus::composite(
                        GRID_TAG,
                        format!("{GRID_TAG}#{COLOR_SW_PREFIX}{cur}"),
                    ));
                }
                _ => {}
            }
        }
        // Otherwise the active descendant follows the roving tree cursor (a leaf
        // value-index id or a branch `cat.` / `struct.` id), ringing its row tag
        // via the `tree_row_tag` SSOT (so it matches what `tree_access_nodes`
        // emitted — R921.1 use-substrate); the grid atomically when no cursor yet.
        Some(use_property_cursor().get().map_or_else(
            || AccessFocus::atomic(GRID_TAG),
            |id| AccessFocus::composite(GRID_TAG, tree_row_tag(GRID_TAG, &id)),
        ))
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
    fn r921_tree_leaves_cover_every_value_slot_once() {
        // Every leaf id in the tree indexes the value model, and every value
        // slot is reached by exactly one leaf (no orphan / no duplicate).
        fn collect(nodes: &[PropertyNode], out: &mut Vec<usize>) {
            for n in nodes {
                if let Some(i) = n.value_index {
                    out.push(i);
                }
                collect(&n.children, out);
            }
        }
        assert_eq!(default_properties().len(), VALUE_COUNT);
        assert_eq!(PROPERTY_NAMES.len(), VALUE_COUNT);
        let mut leaves = Vec::new();
        collect(&default_tree(), &mut leaves);
        leaves.sort_unstable();
        assert_eq!(
            leaves,
            (0..VALUE_COUNT).collect::<Vec<_>>(),
            "every value slot is reached by exactly one tree leaf",
        );
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

    /// Run `f` against the grid's mutable introspection in a borrow scope.
    fn with_grid_mut<R>(scene: &mut Scene, f: impl FnOnce(&mut dyn ExternalIntrospect) -> R) -> R {
        let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
        f(node.handle.introspect_mut().expect("introspectable"))
    }

    /// R919 — a property is modified once its value differs from the class
    /// default; `reset` restores it through the shared value funnel, and the
    /// reads (`modified.<i>` / `any_modified`) track it. Row 4 ("Layer") is an
    /// `Int` defaulting to 3.
    #[test]
    fn r919_modified_and_reset_via_rpc() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert_eq!(grid_intro(&scene).query("any_modified"), Some(IntrospectValue::Bool(false)), "boot: clean");
            assert_eq!(grid_intro(&scene).query("modified.4"), Some(IntrospectValue::Bool(false)));
            with_grid_mut(&mut scene, |i| i.intervene("value.4", IntrospectValue::Int(17)).unwrap());
            assert_eq!(grid_intro(&scene).query("modified.4"), Some(IntrospectValue::Bool(true)), "an edited value is modified");
            assert_eq!(grid_intro(&scene).query("any_modified"), Some(IntrospectValue::Bool(true)));
            assert_eq!(grid_intro(&scene).query("modified.6"), Some(IntrospectValue::Bool(false)), "an untouched row stays clean");
            assert_eq!(grid_intro(&scene).query("modified.99"), None, "out-of-range modified -> None");
            // Reset restores the default and clears modified.
            assert_eq!(
                with_grid_mut(&mut scene, |i| i.invoke("reset", IntrospectValue::Int(4))),
                Ok(IntrospectValue::Bool(true)),
                "reset changed the modified row",
            );
            assert_eq!(grid_intro(&scene).query("value.4"), Some(IntrospectValue::Int(3)), "reset restored the default");
            assert_eq!(grid_intro(&scene).query("modified.4"), Some(IntrospectValue::Bool(false)));
            assert_eq!(grid_intro(&scene).query("any_modified"), Some(IntrospectValue::Bool(false)));
            // Resetting an already-default row is a no-op `false`.
            assert_eq!(
                with_grid_mut(&mut scene, |i| i.invoke("reset", IntrospectValue::Int(4))),
                Ok(IntrospectValue::Bool(false)),
                "reset of an unmodified row is a no-op",
            );
        });
    }

    /// R919 — `reset_all` restores every modified property and reports the count.
    #[test]
    fn r919_reset_all_restores_every_modified() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.4", IntrospectValue::Int(17)).unwrap();
                i.intervene("value.6", IntrospectValue::Float(99.0)).unwrap();
                i.intervene("value.2", IntrospectValue::Bool(false)).unwrap();
            });
            assert_eq!(grid_intro(&scene).query("any_modified"), Some(IntrospectValue::Bool(true)));
            assert_eq!(
                with_grid_mut(&mut scene, |i| i.invoke("reset_all", IntrospectValue::Null)),
                Ok(IntrospectValue::Int(3)),
                "reset_all returns the count reset",
            );
            assert_eq!(grid_intro(&scene).query("value.4"), Some(IntrospectValue::Int(3)), "Int restored");
            assert_eq!(grid_intro(&scene).query("value.6"), Some(IntrospectValue::Float(12.5)), "Float restored");
            assert_eq!(grid_intro(&scene).query("value.2"), Some(IntrospectValue::Bool(true)), "Bool restored");
            assert_eq!(grid_intro(&scene).query("any_modified"), Some(IntrospectValue::Bool(false)));
            assert_eq!(
                with_grid_mut(&mut scene, |i| i.invoke("reset_all", IntrospectValue::Null)),
                Ok(IntrospectValue::Int(0)),
                "reset_all on a clean grid resets nothing",
            );
        });
    }

    /// R919 — a modified row paints its reset arrow, and the a11y tree advertises
    /// a matching reset `button` child of the value cell (the paint==a11y one-gate,
    /// R886.1); an unmodified row paints / advertises neither.
    #[test]
    fn r919_modified_row_paints_reset_arrow_with_a11y_button() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let arrow_tag = format!("{GRID_TAG}#{RESET_PREFIX}4");
            assert!(!view(idle_state(), &Frame::new()).contains_tag(&arrow_tag), "no reset arrow on a clean row");
            assert!(
                PropertyGridView::access_node(&idle_state(), Some(GRID_TAG)).iter().all(|n| n.tag != arrow_tag),
                "no reset button advertised while clean",
            );
            with_grid_mut(&mut scene, |i| i.intervene("value.4", IntrospectValue::Int(17)).unwrap());
            assert!(view(idle_state(), &Frame::new()).contains_tag(&arrow_tag), "the modified row paints a reset arrow");
            let a11y = PropertyGridView::access_node(&idle_state(), Some(GRID_TAG));
            let btn = a11y.iter().find(|n| n.tag == arrow_tag).expect("reset button advertised");
            assert_eq!(btn.role, AriaRole::Button);
            assert_eq!(btn.name.as_deref(), Some("Reset Layer to default"), "named for the property");
            // R921 — the reset button is a child of the Layer leaf's `treeitem`
            // row node (`{GRID_TAG}#4`), the painted-row tag.
            let row = a11y.iter().find(|n| n.tag == format!("{GRID_TAG}#4")).expect("Layer row node");
            assert!(row.children.iter().any(|c| c.as_str() == arrow_tag), "the button is the row's child");
        });
    }

    /// R920 (audit) — `Delete` on the cursor data row resets it to default (the
    /// keyboard path to the reset arrow, whose AccessNode is not a tab stop). The
    /// keyboard, click, and RPC all share the one `reset` funnel.
    #[test]
    fn r920_keyboard_delete_resets_cursor_row() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            with_grid_mut(&mut scene, |i| i.intervene("value.4", IntrospectValue::Int(17)).unwrap());
            // Click row 4 (an Int row: a click only moves the roving cursor onto it).
            with_grid_mut(&mut scene, |i| {
                let _ = i.invoke("send", IntrospectValue::Text("4:PointerUp".to_owned()));
            });
            assert!(apply_key_grid(&mut scene, "Delete"), "Delete on a data row is consumed");
            assert_eq!(grid_intro(&scene).query("value.4"), Some(IntrospectValue::Int(3)), "Delete reset the cursor row");
            assert_eq!(grid_intro(&scene).query("modified.4"), Some(IntrospectValue::Bool(false)));
        });
    }

    /// R919 — clicking a row's reset arrow routes to the same `reset` funnel the
    /// RPC and keyboard use (the `{GRID_TAG}#reset<source>` wire).
    #[test]
    fn r919_reset_arrow_click_routes_to_reset() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.4", IntrospectValue::Int(17)).unwrap();
                let _ = i.invoke("send", IntrospectValue::Text(format!("{RESET_PREFIX}4:PointerUp")));
            });
            assert_eq!(grid_intro(&scene).query("value.4"), Some(IntrospectValue::Int(3)), "the arrow click reset the row");
            assert_eq!(grid_intro(&scene).query("modified.4"), Some(IntrospectValue::Bool(false)));
        });
    }

    #[test]
    fn r836_query_exposes_typed_values_names_kinds() {
        Owner::new().run(|| {
            let scene = boot_scene();
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("row_count"), Some(IntrospectValue::Int(16)), "12 scalars + 4 struct fields");
            assert_eq!(intro.query("name.0"), Some(IntrospectValue::Text("Name".to_owned())));
            assert_eq!(intro.query("name.6"), Some(IntrospectValue::Text("Position X".to_owned())));
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

    /// R921 — the tree structure is read off the `TREE_TAG` introspection
    /// (`row_count` / `id_at` / `level_at` / `expanded_at`), and the primary
    /// coordinator owns the per-branch collapse + struct aggregate (collapse via
    /// `toggle_branch`, the struct summary / modified roll-up / `reset_struct`).
    #[test]
    fn r921_tree_introspection_and_collapse_struct_rpc() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let tree_intro = |scene: &Scene| -> (i64, String, i64, bool, String, i64) {
                let tn = scene
                    .find_external_with_tag(TREE_TAG)
                    .and_then(|n| n.handle.introspect())
                    .expect("tree introspection present");
                let int = |p: &str| match tn.query(p) {
                    Some(IntrospectValue::Int(i)) => i,
                    other => panic!("{p} -> {other:?}"),
                };
                let text = |p: &str| match tn.query(p) {
                    Some(IntrospectValue::Text(t)) => t,
                    other => panic!("{p} -> {other:?}"),
                };
                let expanded0 = tn.query("expanded_at.0") == Some(IntrospectValue::Bool(true));
                // Row 15 = the Position struct header (after 5 cats + Identity's 3
                // leaves + Appearance's 4 + Physics's 2 + Stats's 1 + the Transform
                // header); row 16 = its first field (Position X).
                (int("row_count"), text("id_at.0"), int("level_at.0"), expanded0, text("id_at.15"), int("level_at.16"))
            };
            // 5 categories + 2 structs + 16 leaves = 23 visible rows (all open).
            let (rows, id0, level0, exp0, id15, level16) = tree_intro(&scene);
            assert_eq!(rows, 23, "5 categories + 2 structs + 16 leaves");
            assert_eq!(id0, "cat.Identity");
            assert_eq!(level0, 1, "a category is aria-level 1");
            assert!(exp0, "categories boot expanded");
            assert_eq!(id15, "struct.Position", "the Transform category nests the Position struct");
            assert_eq!(level16, 3, "a struct field is aria-level 3 (category > struct > field)");
            // The primary owns the struct aggregate + the per-branch collapse.
            let gi = grid_intro(&scene);
            let IntrospectValue::Text(summary) = gi.query("struct_summary.struct.Position").expect("summary") else {
                panic!("summary is text");
            };
            assert!(
                summary.starts_with('(') && summary.ends_with(')') && summary.contains("12.5"),
                "Position summary is a tuple of its field values, got {summary}",
            );
            assert_eq!(gi.query("struct_modified.struct.Position"), Some(IntrospectValue::Bool(false)), "boot clean");
            assert_eq!(gi.query("expanded.cat.Identity"), Some(IntrospectValue::Bool(true)));
            assert_eq!(gi.query("expanded.0"), Some(IntrospectValue::Null), "a leaf has no expanded flag");
            // Collapse Identity via toggle_branch → its 3 leaves vanish (23 − 3).
            assert_eq!(
                with_grid_mut(&mut scene, |i| i.invoke("toggle_branch", IntrospectValue::Text("cat.Identity".to_owned()))),
                Ok(IntrospectValue::Bool(false)),
                "toggle_branch returns the resulting expanded flag",
            );
            assert_eq!(grid_intro(&scene).query("expanded.cat.Identity"), Some(IntrospectValue::Bool(false)));
            assert_eq!(tree_intro(&scene).0, 20, "23 − 3 Identity leaves");
            // Editing a struct field marks the struct modified; reset_struct clears it.
            with_grid_mut(&mut scene, |i| i.intervene("value.6", IntrospectValue::Float(99.0)).unwrap());
            assert_eq!(grid_intro(&scene).query("struct_modified.struct.Position"), Some(IntrospectValue::Bool(true)));
            assert_eq!(
                with_grid_mut(&mut scene, |i| i.invoke("reset_struct", IntrospectValue::Text("struct.Position".to_owned()))),
                Ok(IntrospectValue::Int(1)),
                "reset_struct restores 1 modified field",
            );
            assert_eq!(grid_intro(&scene).query("value.6"), Some(IntrospectValue::Float(12.5)));
            assert_eq!(grid_intro(&scene).query("struct_modified.struct.Position"), Some(IntrospectValue::Bool(false)));
        });
    }

    /// R921 — the roving cursor is read/write over RPC (the AI-first cursor
    /// move): `intervene cursor` sets a node id with no click side effect, `query
    /// cursor` reads it, and `Null` clears it.
    #[test]
    fn r921_cursor_read_write_over_rpc() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert_eq!(grid_intro(&scene).query("cursor"), Some(IntrospectValue::Null), "no cursor at boot");
            with_grid_mut(&mut scene, |i| i.intervene("cursor", IntrospectValue::Text("struct.Position".to_owned())).unwrap());
            assert_eq!(grid_intro(&scene).query("cursor"), Some(IntrospectValue::Text("struct.Position".to_owned())));
            // A pure cursor move does NOT toggle / edit the row (no side effect).
            assert_eq!(grid_intro(&scene).query("editing"), Some(IntrospectValue::Json(serde_json::Value::Null)));
            with_grid_mut(&mut scene, |i| i.intervene("cursor", IntrospectValue::Null).unwrap());
            assert_eq!(grid_intro(&scene).query("cursor"), Some(IntrospectValue::Null), "Null clears the cursor");
        });
    }

    /// R921.1 (audit) — an edit/popup whose row is collapsed away must NOT be
    /// advertised by `query editing` / `popup_cursor` (it paints nowhere — the
    /// R901.1 introspection-must-match-paint invariant), and the invisible popup
    /// must NOT hijack the grid keymap (else the arrows are trapped and the
    /// branch can never be re-expanded by keyboard). Re-expanding re-advertises.
    #[test]
    fn r921_1_collapsed_edit_not_advertised_and_no_keymap_hijack() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Open the Blend choice popup (value 9, under cat.Appearance).
            with_grid_mut(&mut scene, |i| {
                let _ = i.invoke("begin", IntrospectValue::Int(9));
            });
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Some(IntrospectValue::Json(serde_json::Value::from(9))),
                "while visible the open popup is advertised",
            );
            assert_eq!(grid_intro(&scene).query("popup_cursor"), Some(IntrospectValue::Int(0)));
            // Collapse Appearance (hides the Blend row) via RPC.
            with_grid_mut(&mut scene, |i| {
                let _ = i.invoke("toggle_branch", IntrospectValue::Text("cat.Appearance".to_owned()));
            });
            // The now-hidden edit/popup reports as not-editing (matches paint).
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Some(IntrospectValue::Json(serde_json::Value::Null)),
                "a collapsed edit is not advertised",
            );
            assert_eq!(grid_intro(&scene).query("popup_cursor"), Some(IntrospectValue::Null));
            // The invisible popup does not intercept the grid keymap: ArrowDown
            // moves the tree cursor (Identity branch -> its first leaf).
            set_cursor_id("cat.Identity");
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", Modifiers::empty()));
            assert_eq!(cursor_id().as_deref(), Some("0"), "tree nav advanced, not hijacked by the hidden popup");
            // Re-expanding re-advertises the still-open edit (state suspended, not destroyed).
            with_grid_mut(&mut scene, |i| {
                let _ = i.invoke("toggle_branch", IntrospectValue::Text("cat.Appearance".to_owned()));
            });
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Some(IntrospectValue::Json(serde_json::Value::from(9))),
                "re-expanding restores the advertisement",
            );
        });
    }

    #[test]
    fn r836_toggle_invoke_flips_bool_by_source() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Toggle the Visible bool by its stable source index (2).
            assert_eq!(intro.invoke("toggle", IntrospectValue::Int(2)), Ok(IntrospectValue::Bool(true)));
            assert_eq!(intro.query("value.2"), Some(IntrospectValue::Bool(false)));
            // A non-bool source -> no-op.
            assert_eq!(intro.invoke("toggle", IntrospectValue::Int(0)), Ok(IntrospectValue::Bool(false)));
            assert_eq!(intro.query("value.0"), Some(IntrospectValue::Text("Player".to_owned())));
        });
    }

    #[test]
    fn r836_click_moves_cursor_and_toggles_bool() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // PointerUp on the Locked bool (source 3) moves the cursor onto its
            // leaf-id row + toggles it.
            let _ = intro.invoke("send", IntrospectValue::Text("3:PointerUp".to_owned()));
            assert_eq!(intro.query("value.3"), Some(IntrospectValue::Bool(true)), "false -> true");
            assert_eq!(cursor_id().as_deref(), Some("3"), "cursor moved onto the clicked leaf");
            // PointerUp on a text row moves the cursor but does not toggle.
            let _ = intro.invoke("send", IntrospectValue::Text("0:PointerUp".to_owned()));
            assert_eq!(cursor_id().as_deref(), Some("0"));
        });
    }

    // ----- R875 numeric scrub -----

    /// Float scrub: a rightward capture drag adds `travel_px · 0.01` to Pos X.
    #[test]
    fn r875_float_scrub_adds_pixel_travel_times_sensitivity() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            // Pos X (source 6) boots at 12.5.
            assert_eq!(node.handle.introspect().unwrap().query("value.6"), Some(IntrospectValue::Float(12.5)));
            // Press the row (arm), then drag the captured cursor from x_rel 0.5
            // to 0.75 across the 400px grid: travel = 0.25 · 400 = 100px → +1.0.
            node.handle.introspect_mut().unwrap()
                .invoke("send", IntrospectValue::Text("6:PointerDown".to_owned())).unwrap();
            node.handle.pointer_move(0.5, 0.5); // calibrate (no mutation)
            assert_eq!(node.handle.introspect().unwrap().query("value.6"), Some(IntrospectValue::Float(12.5)),
                "first move only calibrates");
            assert_eq!(node.handle.introspect().unwrap().query("scrubbing"), Some(IntrospectValue::Bool(false)),
                "R915: the calibration frame is a click so far, not yet a scrub");
            node.handle.pointer_move(0.75, 0.5); // apply +100px (past the 4px dead zone)
            assert_eq!(node.handle.introspect().unwrap().query("scrubbing"), Some(IntrospectValue::Bool(true)),
                "a real drag past the threshold is a scrub");
            let IntrospectValue::Float(v) = node.handle.introspect().unwrap().query("value.6").unwrap() else {
                panic!("Pos X stays a float");
            };
            assert!((v - 13.5).abs() < 1e-6, "12.5 + 100px·0.01 = 13.5, got {v}");
            // Release clears the scrub; the value is the committed live value.
            node.handle.introspect_mut().unwrap()
                .invoke("send", IntrospectValue::Text("6:PointerUp".to_owned())).unwrap();
            assert_eq!(node.handle.introspect().unwrap().query("scrubbing"), Some(IntrospectValue::Bool(false)));
        });
    }

    /// Int scrub steps in whole units (8px/step) and a leftward drag decrements.
    #[test]
    fn r875_int_scrub_steps_in_whole_units_both_directions() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            // Layer (source 4) boots at 3. Drag +80px → +10 steps → 13.
            node.handle.introspect_mut().unwrap()
                .invoke("send", IntrospectValue::Text("4:PointerDown".to_owned())).unwrap();
            node.handle.pointer_move(0.5, 0.5);
            node.handle.pointer_move(0.7, 0.5); // +0.2·400 = 80px → +10
            assert_eq!(node.handle.introspect().unwrap().query("value.4"), Some(IntrospectValue::Int(13)));
            // Same gesture, leftward: a fresh press anchors, drag −80px → 3.
            node.handle.introspect_mut().unwrap()
                .invoke("send", IntrospectValue::Text("4:PointerUp".to_owned())).unwrap();
            node.handle.introspect_mut().unwrap()
                .invoke("send", IntrospectValue::Text("4:PointerDown".to_owned())).unwrap();
            node.handle.pointer_move(0.5, 0.5);
            node.handle.pointer_move(0.3, 0.5); // −80px → −10 → 3
            assert_eq!(node.handle.introspect().unwrap().query("value.4"), Some(IntrospectValue::Int(3)));
        });
    }

    /// A scrub suppresses the trailing click (no editor opens), and a drag on a
    /// non-numeric row never scrubs (its release still performs the click).
    #[test]
    fn r875_scrub_suppresses_click_and_skips_non_numeric() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            // Scrub the Float row, then release: the editor must NOT open.
            node.handle.introspect_mut().unwrap()
                .invoke("send", IntrospectValue::Text("6:PointerDown".to_owned())).unwrap();
            node.handle.pointer_move(0.5, 0.5);
            node.handle.pointer_move(0.6, 0.5);
            node.handle.introspect_mut().unwrap()
                .invoke("send", IntrospectValue::Text("6:PointerUp".to_owned())).unwrap();
            assert_eq!(node.handle.introspect().unwrap().query("editing"),
                Some(IntrospectValue::Json(serde_json::Value::Null)), "a scrub does not open the editor");
            // A drag on the Visible bool (source 2) does not scrub; the release
            // still toggles it (true → false).
            node.handle.introspect_mut().unwrap()
                .invoke("send", IntrospectValue::Text("2:PointerDown".to_owned())).unwrap();
            node.handle.pointer_move(0.5, 0.5);
            node.handle.pointer_move(0.8, 0.5); // no-op (non-numeric armed → none)
            assert_eq!(node.handle.introspect().unwrap().query("scrubbing"), Some(IntrospectValue::Bool(false)),
                "a non-numeric press never calibrates a scrub");
            node.handle.introspect_mut().unwrap()
                .invoke("send", IntrospectValue::Text("2:PointerUp".to_owned())).unwrap();
            assert_eq!(node.handle.introspect().unwrap().query("value.2"), Some(IntrospectValue::Bool(false)),
                "the bool still toggles on release (drag did not scrub it)");
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

    /// The current visible-row flatten (tree + live filter), the SSOT the paint /
    /// cursor / a11y read.
    fn visible() -> Vec<VisibleRow> {
        visible_property_rows(&use_property_tree().get(), &current_search_query())
    }

    /// The visible rows' node ids, in flatten order.
    fn visible_ids() -> Vec<String> {
        visible().into_iter().map(|r| r.id).collect()
    }

    /// The value indices of the visible LEAF rows, in flatten order.
    fn visible_leaf_indices() -> Vec<usize> {
        visible().iter().filter_map(|r| row_value_index(&r.id)).collect()
    }

    /// Park the roving cursor on tree node `id` (a leaf value-index string or a
    /// branch `cat.` / `struct.` id) — the tree-cursor peer of the old visual
    /// `set_cursor(pos)`.
    fn set_cursor_id(id: &str) {
        use_property_cursor().set(Some(id.to_owned()));
    }

    /// The roving cursor's node id, or `None`.
    fn cursor_id() -> Option<String> {
        use_property_cursor().get()
    }

    #[test]
    fn r921_grid_arrows_navigate_over_flatten_and_clamp() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let ids = visible_ids();
            let first = ids.first().expect("non-empty flatten").clone(); // "cat.Identity"
            let last = ids.last().expect("non-empty flatten").clone(); // "15" (Scale Z)
            // First ArrowDown from no cursor lands on row 0 (the Identity header).
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", Modifiers::empty()));
            assert_eq!(cursor_id().as_deref(), Some(first.as_str()));
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "End", Modifiers::empty()));
            assert_eq!(cursor_id().as_deref(), Some(last.as_str()));
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", Modifiers::empty()));
            assert_eq!(cursor_id().as_deref(), Some(last.as_str()), "clamps at the bottom");
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "Home", Modifiers::empty()));
            assert_eq!(cursor_id().as_deref(), Some(first.as_str()));
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowUp", Modifiers::empty()));
            assert_eq!(cursor_id().as_deref(), Some(first.as_str()), "clamps at the top");
        });
    }

    #[test]
    fn r921_keyboard_collapses_and_expands_category_header() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Cursor on the Identity category branch.
            set_cursor_id("cat.Identity");
            let collapsed = || !find_node(&use_property_tree().get(), "cat.Identity").unwrap().expanded;
            // ArrowLeft on an expanded branch collapses it; its 3 leaves vanish.
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowLeft", Modifiers::empty()));
            assert!(collapsed(), "ArrowLeft collapses the focused category");
            assert_eq!(visible().len(), 20, "23 − 3 Identity leaves");
            // ArrowRight re-expands.
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowRight", Modifiers::empty()));
            assert!(!collapsed());
            assert_eq!(visible().len(), 23);
            // Enter on a branch toggles it too.
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "Enter", Modifiers::empty()));
            assert!(collapsed(), "Enter on a branch toggles collapse");
        });
    }

    #[test]
    fn r836_space_toggles_bool_enter_edits_text() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Cursor onto the Visible bool (leaf 2) and Space-toggle it.
            set_cursor_id("2");
            assert!(PropertyGridView::apply_key(&mut scene, Some(GRID_TAG), "Space", Modifiers::empty()));
            assert_eq!(grid_intro(&scene).query("value.2"), Some(IntrospectValue::Bool(false)));
            // Cursor onto the Name text row (leaf 0) and Enter -> edit mode.
            set_cursor_id("0");
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
            assert_eq!(cursor_id(), None, "cursor unchanged");
        });
    }

    // ----- a11y -----

    #[test]
    fn r921_access_node_emits_tree_with_levels_and_active_row() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            // Cursor on the Visible leaf (value 2).
            scene_focus("2");
            let nodes = PropertyGridView::access_node(&idle_state(), Some(GRID_TAG));
            // R921 — role=tree; the root carries the Inspector name.
            assert_eq!(nodes[0].role, AriaRole::Tree);
            assert_eq!(nodes[0].name.as_deref(), Some("Inspector"));
            // A category lowers to a level-1 treeitem (aria-expanded + a count name).
            let cat = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#cat.Identity"))
                .expect("Identity category treeitem");
            assert_eq!(cat.role, AriaRole::TreeItem);
            assert_eq!(cat.level, Some(1), "a category is aria-level 1");
            assert_eq!(cat.expanded, Some(true), "expanded category");
            assert_eq!(cat.name.as_deref(), Some("Identity (3)"), "category name folds its leaf count");
            // The cursor leaf is the active descendant (focus, NO aria-selected —
            // the Inspector has no selection model), level 2, value folded in.
            let active = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#2"))
                .expect("Visible leaf node");
            assert_eq!(active.level, Some(2), "a category leaf is aria-level 2");
            assert!(active.state.focused, "cursor row is the active descendant");
            assert_eq!(active.selected, None, "no selection model → no aria-selected axis");
            assert_eq!(active.name.as_deref(), Some("Visible: On"), "leaf name folds its value");
            // A struct branch lowers to a level-2 treeitem with its summary; its
            // fields are level-3 treeitems named with the qualified property name.
            let pos = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#struct.Position"))
                .expect("Position struct node");
            assert_eq!(pos.level, Some(2));
            assert_eq!(pos.expanded, Some(true));
            assert!(
                pos.name.as_deref().unwrap_or("").starts_with("Position ("),
                "struct name folds its summary, got {:?}",
                pos.name,
            );
            let field =
                nodes.iter().find(|n| n.tag == format!("{GRID_TAG}#6")).expect("Position X field node");
            assert_eq!(field.level, Some(3), "a struct field is aria-level 3");
            assert!(
                field.name.as_deref().unwrap_or("").starts_with("Position X:"),
                "field name is qualified, got {:?}",
                field.name,
            );
        });
    }

    fn scene_focus(id: &str) {
        set_cursor_id(id);
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
            let closed = view(((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), &Frame::new());
            assert!(!closed.contains_tag(CHOICE_POPUP_TAG), "no panel when closed");
            let closed_nodes =
                PropertyGridView::access_node(&((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), Some(GRID_TAG));
            assert!(
                !closed_nodes.iter().any(|n| n.role == pinion_a11y::AriaRole::Listbox),
                "no listbox when closed",
            );
            // Open the Blend popup.
            open_choice(&mut scene, BLEND_ROW);
            let open = view(((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), &Frame::new());
            assert!(open.contains_tag(CHOICE_POPUP_TAG), "panel painted when open");
            assert!(open.contains_tag(POPUP_DISMISS_TAG), "dismiss barrier painted");
            assert!(open.contains_tag(&format!("{GRID_TAG}#opt0")), "option 0 painted");
            assert!(open.contains_tag(&format!("{GRID_TAG}#opt3")), "option 3 painted");
            let nodes = PropertyGridView::access_node(&((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), Some(GRID_TAG));
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
            let closed = view(((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), &Frame::new());
            assert!(!closed.contains_tag(COLOR_POPUP_TAG), "no colour panel when closed");
            open_choice(&mut scene, TINT_ROW);
            let open = view(((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), &Frame::new());
            assert!(open.contains_tag(COLOR_POPUP_TAG), "colour panel painted when open");
            assert!(open.contains_tag(POPUP_DISMISS_TAG), "dismiss barrier painted");
            assert!(open.contains_tag(&format!("{GRID_TAG}#sw0")), "swatch 0 painted");
            assert!(open.contains_tag(&format!("{GRID_TAG}#sw7")), "swatch 7 painted");
            let nodes = PropertyGridView::access_node(&((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), Some(GRID_TAG));
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
            // Navigating: the active descendant is the cursor's leaf row tag.
            set_cursor_id("2");
            let f = PropertyGridView::access_focus_target(&idle_state(), Some(GRID_TAG))
                .expect("grid focused -> composite focus target");
            assert_eq!(f.focus_tag, GRID_TAG);
            assert_eq!(f.active_descendant.as_deref(), Some(format!("{GRID_TAG}#2").as_str()));
            // Cursor on a category branch rings the branch's composite tag.
            set_cursor_id("cat.Identity");
            let h = PropertyGridView::access_focus_target(&idle_state(), Some(GRID_TAG))
                .expect("composite");
            assert_eq!(h.active_descendant.as_deref(), Some(format!("{GRID_TAG}#cat.Identity").as_str()));
            // Choice popup open -> the active option (Blend cursor boots 0).
            open_choice(&mut scene, BLEND_ROW);
            let f = PropertyGridView::access_focus_target(&((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), Some(GRID_TAG))
                .expect("composite");
            assert_eq!(f.active_descendant.as_deref(), Some(format!("{GRID_TAG}#opt0").as_str()));
            // Colour popup open -> the active swatch (Tint boots Blue=4).
            open_choice(&mut scene, TINT_ROW);
            let f = PropertyGridView::access_focus_target(&((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), Some(GRID_TAG))
                .expect("composite");
            assert_eq!(f.active_descendant.as_deref(), Some(format!("{GRID_TAG}#sw4").as_str()));
            // Focus elsewhere -> atomic, no active descendant.
            let other =
                PropertyGridView::access_focus_target(&((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), Some(EDIT_TF_TAG));
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
            let painted = view(((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), &Frame::new());
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
            let scene = view(((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), &Frame::new());
            assert!(scene.contains_tag(GRID_TAG), "grid root painted");
            assert!(scene.contains_tag(&format!("{GRID_TAG}#0")), "leaf row 0 painted");
            assert!(scene.contains_tag(&format!("{GRID_TAG}#8")), "leaf row 8 painted");
            assert!(scene.contains_tag(&format!("{GRID_TAG}#cat.Identity")), "Identity header painted");
            assert!(scene.contains_tag(&format!("{GRID_TAG}#cat.Transform")), "Transform header painted");
            assert!(scene.contains_tag(&format!("{GRID_TAG}#struct.Position")), "Position struct header painted");
            assert!(scene.contains_tag(&format!("{GRID_TAG}#12")), "Position Z field row painted");
        });
    }

    #[test]
    fn r921_collapse_hides_branch_rows_in_view() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let before = view(idle_state(), &Frame::new());
            assert!(before.contains_tag(&format!("{GRID_TAG}#0")), "Name row painted when expanded");
            assert!(before.contains_tag(&format!("{GRID_TAG}#cat.Identity")), "Identity header painted");
            assert!(before.contains_tag(&format!("{GRID_TAG}#6")), "Position X painted when struct expanded");
            // Collapse Identity: its leaves (Name/Tag/Layer) vanish, header stays.
            set_expanded_in(&use_property_tree(), "cat.Identity", false);
            // Collapse the Position struct: its X/Y/Z fields vanish, header stays.
            set_expanded_in(&use_property_tree(), "struct.Position", false);
            let after = view(idle_state(), &Frame::new());
            assert!(after.contains_tag(&format!("{GRID_TAG}#cat.Identity")), "category header stays on collapse");
            assert!(!after.contains_tag(&format!("{GRID_TAG}#0")), "Name row hidden when collapsed");
            assert!(!after.contains_tag(&format!("{GRID_TAG}#1")), "Tag row hidden when collapsed");
            assert!(after.contains_tag(&format!("{GRID_TAG}#struct.Position")), "struct header stays on collapse");
            assert!(!after.contains_tag(&format!("{GRID_TAG}#6")), "Position X hidden when struct collapsed");
        });
    }

    #[test]
    fn r836_view_paints_inline_field_only_while_editing() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let before = view(((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), &Frame::new());
            assert!(!before.contains_tag(EDIT_TF_TAG), "no inline field when not editing");
            use_editing_row().set(Some(0));
            let during = view(((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), &Frame::new());
            assert!(during.contains_tag(EDIT_TF_TAG), "inline field painted in the editing row");
        });
    }

    #[test]
    fn r836_view_contains_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<PropertyGridView>(
            ((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)),
            &Frame::default(),
        );
    }

    // ----- R872 live search / filter -----

    #[test]
    fn r872_search_filters_rows_and_clears() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            assert_eq!(visible().len(), 23, "5 cats + 2 structs + 16 leaves with empty query");
            // "pos" matches the qualified Position X/Y/Z field names; the
            // recursive filter reveals the Transform > Position path.
            use_text_edit_state(SEARCH_TF_TAG).set_text("pos".to_owned());
            assert_eq!(
                visible_ids(),
                vec!["cat.Transform", "struct.Position", "6", "7", "12"],
                "path-to-match reveals the matching fields inside their struct",
            );
            assert_eq!(visible_leaf_indices(), vec![6, 7, 12], "only the Position fields match");
            // Clearing restores every row (the filter recomputes reactively).
            use_text_edit_state(SEARCH_TF_TAG).set_text(String::new());
            assert_eq!(visible().len(), 23, "cleared query restores every row");
        });
    }

    #[test]
    fn r872_filter_drops_unmatched_branches() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            // "name" matches only Name (leaf 0, under Identity) — every other
            // branch is pruned (no match anywhere on its path).
            use_text_edit_state(SEARCH_TF_TAG).set_text("name".to_owned());
            assert_eq!(visible_ids(), vec!["cat.Identity", "0"], "only Identity > Name survives");
            assert_eq!(visible_leaf_indices(), vec![0]);
        });
    }

    #[test]
    fn r872_search_field_painted_and_escape_clears() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let painted = view(((TextFieldState::Idle, 0), (TextFieldState::Idle, 0)), &Frame::new());
            assert!(painted.contains_tag(SEARCH_TF_TAG), "search box painted in the title band");
            // Escape on the focused search box clears the live filter.
            use_text_edit_state(SEARCH_TF_TAG).set_text("xyz".to_owned());
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(SEARCH_TF_TAG),
                "Escape",
                Modifiers::empty(),
            ));
            assert_eq!(use_text_edit_state(SEARCH_TF_TAG).text(), "", "Escape clears the query");
        });
    }

    // ----- R873 audit remediation (a11y) -----

    /// A neutral RootState (both fields idle) for a11y assertions.
    fn idle_state() -> RootState {
        ((TextFieldState::Idle, 0), (TextFieldState::Idle, 0))
    }

    #[test]
    fn r873_search_box_has_textbox_a11y_node() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            use_text_edit_state(SEARCH_TF_TAG).set_text("pos".to_owned());
            let nodes = PropertyGridView::access_node(&idle_state(), Some(SEARCH_TF_TAG));
            let search = nodes
                .iter()
                .find(|n| n.tag == SEARCH_TF_TAG)
                .expect("search box emits a textbox a11y node");
            assert_eq!(search.role, pinion_a11y::AriaRole::TextInput, "searchbox -> textbox role");
            assert_eq!(search.name.as_deref(), Some("Filter properties"), "accessible name");
            assert!(search.state.focused, "the focused search box is announced focused");
        });
    }

    #[test]
    fn r873_filter_status_live_region_reports_count() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            let status_count = || {
                let nodes = PropertyGridView::access_node(&idle_state(), Some(GRID_TAG));
                nodes
                    .iter()
                    .find(|n| n.tag == "pg_search_status")
                    .map(|n| (n.role, n.name.clone()))
                    .expect("a polite status live region")
            };
            let (role, name) = status_count();
            assert_eq!(role, AriaRole::Status, "filter result = aria-live Status");
            assert_eq!(name.as_deref(), Some("16 properties"), "all 16 leaves with no filter");
            // Narrowing the filter updates the announced count.
            use_text_edit_state(SEARCH_TF_TAG).set_text("pos".to_owned());
            assert_eq!(status_count().1.as_deref(), Some("3 properties"), "filtered to Position X/Y/Z");
        });
    }

    #[test]
    fn r873_popup_a11y_gated_on_row_visibility() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            open_choice(&mut scene, BLEND_ROW); // editing the Blend choice (source 9)
            // Visible: the popup lowers to a listbox + the active descendant
            // points into it.
            let nodes = PropertyGridView::access_node(&idle_state(), Some(GRID_TAG));
            assert!(
                nodes.iter().any(|n| n.role == pinion_a11y::AriaRole::Listbox),
                "popup listbox emitted while its row is visible",
            );
            let f = PropertyGridView::access_focus_target(&idle_state(), Some(GRID_TAG))
                .expect("composite focus");
            assert!(
                f.active_descendant.as_deref().is_some_and(|d| d.contains(CHOICE_OPT_PREFIX)),
                "active descendant points into the open popup",
            );
            // Filter the Blend row out of the flatten — the popup is now neither
            // painted (view_popup_overlay) nor announced (access_node / focus
            // target): paint and a11y stay in agreement (R873).
            use_text_edit_state(SEARCH_TF_TAG).set_text("zzz".to_owned());
            assert!(
                !view(idle_state(), &Frame::new()).contains_tag(CHOICE_POPUP_TAG),
                "popup not painted when its row is filtered out",
            );
            let nodes = PropertyGridView::access_node(&idle_state(), Some(GRID_TAG));
            assert!(
                !nodes.iter().any(|n| n.role == pinion_a11y::AriaRole::Listbox),
                "no listbox a11y when the popup's row is filtered out",
            );
            let f = PropertyGridView::access_focus_target(&idle_state(), Some(GRID_TAG))
                .expect("focus target");
            assert!(
                !f.active_descendant.as_deref().unwrap_or("").contains(CHOICE_OPT_PREFIX),
                "active descendant no longer points at the unpainted popup",
            );
        });
    }
}
