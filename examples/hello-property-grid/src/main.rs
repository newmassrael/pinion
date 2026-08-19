// R836 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (WAI-ARIA, PropertyGridExternal, TextFieldExternal, gridcell, …).
#![allow(clippy::doc_markdown)]

//! `hello-property-grid` — R836 / R921 §5.38 §5.40 §5.50 **property-grid / inspector detail
//! panel**: the editor's "Details" panel (the engine Details / the toolkit `QtPropertyBrowser`
//! / a CSS-devtools style editor) — a **tree** of `(name, typed-value)` rows where each value is
//! editable in place by a *type-appropriate* control. Scalar properties nest
//! under collapsible **category** branches (Identity / Appearance / Transform
//! / …) and **struct** branches (a `Vector3` like Position expands into its X / Y /
//! Z field rows — the engine / the toolkit Details core depth), with a **live
//! name search box** (R872 — the Details-panel filter).
//!
//! ## Why this is the Phase-B #1 leverage item
//!
//! The northern-star is an engine-class editor self-hosted in pinion. That
//! editor's Details / Inspector panel is a property grid; this binding builds
//! it as a **pure composition of existing substrate** — no new framework
//! crate. R836–R920 built the flat, category-grouped grid; **R921** migrated
//! its row backbone from the 2-level `GroupOrderState` group-by proxy to the arbitrary-depth
//! WAI-ARIA Tree substrate (`flat_visible` / `resolve_tree_key` / `tree_access_nodes`), because categories and struct
//! properties are the same thing — a collapsible branch — and a struct's X/Y/Z
//! fields need a third level the group proxy cannot express. The self-hosted
//! editor will be the 2nd consumer of the typed-editable-row model, at which
//! point the stable parts lift to a framework crate (`[[abstraction-needs-second-consumer]]`).
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
//! - **Per-property clamp ranges** (R964): a ranged scalar leaf (today only
//!   `Opacity`, a normalised `0..1` factor) carries a `[min, max]`
//!   (`scalar_range`) and clamps every write through the one `clamp_to_range` /
//!   `set_value` funnel (the data-grid R894 `ColRange` sibling), painting an
//!   in-cell slider gauge; an out-of-range write reads back as the clamped
//!   bound. Unranged leaves still accept any parseable value (a malformed commit
//!   reverts, no data loss) — a general per-property validation / range UI is
//!   the additive 2nd-consumer axis.

use std::cell::Cell;
use std::rc::Rc;

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, AriaRole, ListOption, WidgetA11y, attach_child_button,
    listbox_option_nodes, tree_access_nodes, tree_row_tag, windowed_list_nodes_selected,
};
use pinion_core::cell_value::{CellKind, CellValue};
use pinion_core::composite_tag::prefixed_index;
use pinion_core::directory::{Directory, InMemoryDirectory};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, CaptureNormalize, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner,
    SchemaArg, SchemaField, ThreadOwnership,
};
use pinion_core::input::{DRAG_CLICK_THRESHOLD_PX, DragCalibration, PointerReading};
use pinion_core::reactive::{Owner, Signal, batch};
use pinion_core::scene::ScrollAxis;
use pinion_core::scene::{ContainerNode, Rect, ScrollNode, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::checkbox::CheckboxState;
use pinion_core::widgets::file_browser::{
    DirectoryExternal, DirectoryState, dir_nav_key_selecting, use_directory_state,
};
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::widgets::modal::{ModalState, modal_introspection_extra, use_modal};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::slider::SliderState;
use pinion_core::widgets::text_edit::{TextEditState, use_text_edit_state};
use pinion_core::widgets::text_field::{
    TextFieldExternal, TextFieldState, blur_committing_field_extra,
};
use pinion_core::widgets::tree_nav::{
    TreeKey, TreeNode, VisibleRow, find_node, find_node_mut, flat_visible, flat_visible_filtered,
    resolve_tree_key, set_expanded_in, toggle_expanded, tree_view_introspection_extra,
};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Color, Command, DirEntry, Frame, Modifiers, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::anchor::{AnchorSide, flip_y};
use pinion_widget_paint::barrier::dismiss_barrier;
use pinion_widget_paint::button::{
    ButtonColors, ButtonStyle, button_a11y_state, button_scene, read_button_state,
};
use pinion_widget_paint::checkbox::{CheckboxStyle, view_checkbox_box};
use pinion_widget_paint::dialog::{DialogContent, DialogStyle, view_dialog};
use pinion_widget_paint::file_browser::{FileBrowserMetrics, file_browser_pane};
use pinion_widget_paint::group_header::group_header_row;
use pinion_widget_paint::listbox::{OptionRow, view_option};
use pinion_widget_paint::popup::popup_surface;
use pinion_widget_paint::slider::{slider_accent_for, slider_track_inactive};
use pinion_widget_paint::text_field as tf_paint;

use pinion_widget_paint::state_layer::focus_fill;

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

/// A trailing-edge glyph button's box: at least the LINE BOX of the face it
/// holds, so the glyph cannot paint above and below its own button.
///
/// ★ R1673 — it was [`RESET_DOT`] square, a token, while the glyph inside is a
/// [`CELL_PX`] face. Measured: the `+` and all three `−` painted six pixels
/// above and five below their buttons.
fn glyph_button_side() -> u32 {
    RESET_DOT.max(pinion_core::containment::line_box(CELL_PX))
}
const RESET_DOT_X: u32 = 16;
/// R936 — the **secondary** trailing-slot inset, one mark + gap to the LEFT of
/// [`RESET_DOT_X`]. A row with two trailing affordances (an array element's
/// remove button / the array branch's add button at the trailing edge **plus**
/// a modified reset arrow) parks the reset arrow here so the two never overlap;
/// scalar / struct rows have only the reset arrow, so it stays at the edge.
const RESET_DOT_X2: u32 = RESET_DOT_X + RESET_DOT + 4;
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
/// R931 — the array branch's "add element" glyph (ASCII plus).
const ADD_GLYPH: &str = "+";
/// R931 — U+2212 MINUS SIGN — an array element's "remove" glyph
/// ([[non-ascii-literal-named-const-escape]]).
const REMOVE_GLYPH: &str = "\u{2212}";

// ─── numeric scrub (R875) ─────────────────────────────────────────

/// The stable pixel reference the numeric scrub normalizes the captured cursor
/// against: the grid container's painted width (`GRID_TAG` is a row-wide flex
/// column, so its rect is exactly `NAME_COL_W + VALUE_COL_W`). The cursor-
/// fraction delta times this width recovers true pixel travel — the column-
/// resize "stable basis" idiom (a scrub never resizes the grid, so its width
/// is constant across the drag).
const GRID_W_PX: f64 = (NAME_COL_W + VALUE_COL_W) as f64;
/// Float scrub sensitivity: value units per pixel of horizontal drag (100 px
/// ⇒ +1.0), the DCC / the engine "drag the number field" gesture.
const SCRUB_FLOAT_PER_PX: f64 = 0.01;
/// Int scrub sensitivity: pixels of horizontal drag per integer step (8 px ⇒
/// +1), so an int scrubs in whole units without runaway.
const SCRUB_INT_PX_PER_STEP: f64 = 8.0;

// ─── bounded slider delegate (R964) ───────────────────────────────

/// R964 — the flat-model slot of the `Opacity` leaf, the one bounded
/// ("ranged") scalar (a normalised `0..1` factor). Named so [`scalar_range`]
/// and the boot model ([`default_properties`]) agree on which slot is bounded.
const OPACITY_SLOT: usize = 8;
/// R964 — the in-cell slider gauge's track / fill strip height (px).
const GAUGE_TRACK_H: u32 = 6;
/// R964 — paint-tag prefix for a ranged leaf's gauge fill, namespaced under
/// `GRID_TAG`. The fill is `pointer_transparent` (a press falls through to the
/// row's scrub, the R954 cell-selection-overlay stance), so the `#` segment is
/// a pure introspection name, never a router target.
const GAUGE_PREFIX: &str = "gauge";

/// R964 — the `[min, max]` interval of a bounded ("ranged") scalar leaf, or
/// `None` for an unbounded one. The interval is fixed class metadata — it is not
/// part of the value, so it lives here keyed by the flat-model slot, not on the
/// [`CellValue`]. Only `Opacity` (a normalised factor) is ranged today.
///
/// The bounded-numeric-DCC clamp pattern's sibling is the data-grid's R894
/// `ColRange` / `clamp_for_col` / `col_range.<col>` (its 1st consumer). The two
/// are kept as separate example-local tables rather than lifted to a shared core
/// type because they diverge in shape — R894 is `Int`-only and column-keyed,
/// this is `Float`-only and flat-slot-keyed — so the only common ground is the
/// stdlib `clamp` atom (already SSOT). A 3rd consumer or a unified
/// Int+Float `NumericRange` is what would justify the lift
/// ([[abstraction-needs-second-consumer]]); the RPC wire is already aligned
/// (`"<lo>..<hi>"` / `"none"`) so that lift would be wire-compatible.
fn scalar_range(slot: usize) -> Option<(f64, f64)> {
    match slot {
        OPACITY_SLOT => Some((0.0, 1.0)),
        _ => None,
    }
}

/// R964 — clamp a ranged scalar's `Float` write to its interval; a no-op for an
/// unranged slot or a non-`Float` value. The data-grid `set_cell` clamp analogue
/// the [`set_value`](PropertyGridExternal::set_value) doc flagged as absent:
/// threading it through `set_value` — the one funnel the scrub, the inline-edit
/// commit, and the RPC `value.<i>` intervene all converge on — bounds every
/// writer at one place.
fn clamp_to_range(slot: usize, value: CellValue) -> CellValue {
    match (scalar_range(slot), value) {
        (Some((lo, hi)), CellValue::Float(f)) => CellValue::Float(f.clamp(lo, hi)),
        (_, other) => other,
    }
}

// ─── asset-path file picker (R1176) ───────────────────────────────

/// R1176 — the flat-model slot of the `Mesh` leaf, the one **asset-path**
/// property (a `CellValue::Text` holding a `/`-path to a project asset). Named
/// so [`is_asset_slot`] and the boot model ([`default_properties`]) agree on
/// which slot opens the embedded file picker rather than the inline text editor.
const MESH_SLOT: usize = 1;

/// R1176 — whether a leaf slot is an **asset path** (activation opens the
/// embedded file picker instead of inline text editing). The editor refinement
/// of a `Text` leaf, the sibling of [`scalar_range`]'s "ranged `Float`"
/// refinement: the path stays a plain `CellValue::Text` value — **no new
/// `CellKind` variant**, since the picker is a host-local affordance and the
/// property grid is the 1st "dialog-embedded-in-a-host" consumer
/// ([[abstraction-needs-second-consumer]]) — so the RPC `value.<i>` read /
/// intervene and undo are unchanged. Only `Mesh` is an asset slot today.
fn is_asset_slot(slot: usize) -> bool {
    slot == MESH_SLOT
}

/// R1176 — the embedded picker's modal-lifecycle key (the R788 lifted
/// open-lifecycle SSOT, [`use_modal`]). One [`ModalState`] `Rc` across the view,
/// the reducer, and `access_node`.
const ASSET_MODAL_KEY: &str = "property_grid.asset_dialog";
/// R1176 — the [`DirectoryExternal`] / file-list a11y `list` tag; rows are
/// `asset_fb#<i>`, the breadcrumb `up` row `asset_fb#up`.
const ASSET_DIR_TAG: &str = "asset_fb";
/// R1176 — the OK / Cancel action-button tags (their `<tag>.click` intents route
/// through [`PropertyGridView::update`]).
const ASSET_OK_TAG: &str = "asset_ok";
const ASSET_CANCEL_TAG: &str = "asset_cancel";
/// R1177 — the action buttons' `<tag>.click` intent tags, built via the
/// drift-safe `intent_tag!` macro (the `EDIT_TF_BLUR_INTENT_TAG` convention)
/// rather than hand-spelled `"asset_ok.click"` literals in [`PropertyGridView::update`].
/// A pin test (`r1177_asset_click_intents_match_tags`) asserts they stay
/// `<ASSET_OK_TAG>.click` / `<ASSET_CANCEL_TAG>.click`.
const ASSET_OK_CLICK: &str = pinion_core::intent_tag!("asset_ok", "click");
const ASSET_CANCEL_CLICK: &str = pinion_core::intent_tag!("asset_cancel", "click");
/// R1176 — the modal scrim / panel paint tags + the file-list scroll key + the
/// modal-open introspection node tag (a query-only `open` bool).
const ASSET_SCRIM_TAG: &str = "asset_scrim";
const ASSET_PANEL_TAG: &str = "asset_panel";
const ASSET_SCROLL_KEY: &str = "property_grid.asset_scroll";

/// R1673 — the scroll state the grid's own viewport is bound to.
const GRID_SCROLL_KEY: &str = "property_grid.grid_scroll";
const ASSET_MODAL_STATE_TAG: &str = "asset_modal";
/// R1176 — the picker's root path + virtualized file-list geometry.
const ASSET_ROOT_DIR: &str = "/proj";
const ASSET_LIST_W: u32 = 320;
const ASSET_LIST_H: u32 = 200;
const ASSET_ROW_PITCH: u32 = 28;
const ASSET_OVERSCAN: usize = 4;
const ASSET_PANEL_W: u32 = 360;

/// R1176 — the shared [`ModalState`] for the asset picker.
fn asset_modal() -> Rc<ModalState> {
    use_modal(ASSET_MODAL_KEY)
}

/// R1176 — the shared [`DirectoryState`] the picker's browser pane reads and the
/// [`DirectoryExternal`] mutates (one `Rc` via the cache key `ASSET_DIR_TAG`).
fn asset_directory() -> Rc<DirectoryState> {
    use_directory_state(ASSET_DIR_TAG, seed_asset_directory, || {
        ASSET_ROOT_DIR.to_string()
    })
}

/// R1176 — the flat-model slot the open picker will write its chosen path into
/// (set by [`open_asset_dialog`](PropertyGridExternal::open_asset_dialog), read
/// by [`confirm_asset`]). `None` while the picker is closed.
fn use_asset_target() -> Rc<Signal<Option<usize>>> {
    Owner::current()
        .expect("use_asset_target requires an active Owner scope")
        .cache("property_grid.asset_target", || Signal::new(None))
}

/// R1176 — the dialog's focusable controls in Tab order: the file list first
/// (the R802 native open-panel default — opening auto-focuses the list so arrows
/// drive selection-follows-focus), then Cancel / Open.
fn asset_dialog_members() -> Vec<String> {
    vec![
        ASSET_DIR_TAG.to_string(),
        ASSET_CANCEL_TAG.to_string(),
        ASSET_OK_TAG.to_string(),
    ]
}

/// R1176 — seed the synthetic project asset tree the picker walks. A
/// deterministic [`InMemoryDirectory`] (the `Storage`/`InMemoryStorage`
/// precedent) so the demo + tests see a fixed tree without touching the real fs;
/// the real-fs `FsDirectory` is unit-tested in `pinion-platform-storage`.
fn seed_asset_directory() -> Rc<dyn Directory> {
    let d = InMemoryDirectory::new();
    d.insert(
        "/proj",
        vec![
            DirEntry::dir("meshes"),
            DirEntry::dir("textures"),
            DirEntry::file("scene.json"),
        ],
    );
    d.insert(
        "/proj/meshes",
        vec![
            DirEntry::file("hero.fbx"),
            DirEntry::file("enemy.fbx"),
            DirEntry::file("prop.obj"),
        ],
    );
    d.insert(
        "/proj/textures",
        vec![
            DirEntry::file("hero_diffuse.png"),
            DirEntry::file("normal.png"),
        ],
    );
    Rc::new(d)
}

/// R1177 — the property-value write funnel: clamp a ranged scalar to its
/// interval ([`clamp_to_range`]) then write the flat-model slot. The ONE SSOT
/// the scrub / inline-edit commit / RPC `value.<i>` intervene (via
/// [`PropertyGridExternal::set_value`]) and the asset picker ([`confirm_asset`])
/// all converge on, so the picked path is clamped + written through identical
/// machinery — no second copy of the clamp + `set_with` body (the R1176
/// `confirm_asset` "mirror of set_value" was that duplicate; R1177 lifts it).
fn write_property_value(model: &Signal<Vec<CellValue>>, slot: usize, value: CellValue) {
    let value = clamp_to_range(slot, value);
    model.set_with(|prev| {
        let mut next = prev.clone();
        if let Some(cell) = next.get_mut(slot) {
            *cell = value;
        }
        next
    });
}

/// R1176 — dismiss the picker without choosing (the Cancel / Escape path): drop
/// the target slot and close the modal, leaving the property value untouched.
fn close_asset_dialog() {
    use_asset_target().set(None);
    asset_modal().close();
}

/// R1176/R1177 — confirm the picked asset: write the selected file into the
/// target slot through the [`write_property_value`] SSOT (the same funnel
/// `set_value` uses, so the write is clamped + RPC-visible via `value.<i>` +
/// reset-tracked like any edit), then close the modal. A no-op when nothing is
/// selected (the OK gate) or the target slot was lost. The write + close
/// [`batch`] so the view re-renders once.
fn confirm_asset() {
    let Some(path) = asset_directory().selected() else {
        return;
    };
    let Some(slot) = use_asset_target().get() else {
        return;
    };
    let picker = asset_modal();
    batch(|| {
        write_property_value(&use_property_model(), slot, CellValue::Text(path));
        use_asset_target().set(None);
        picker.close();
    });
}

/// R964 — the bounded-slider value cell for a ranged `Float` leaf: the engine
/// Details / the DCC "factor" gauge. The formatted number renders as before
/// (flow, vertically centred), with a thin track + active fill pinned along
/// the cell's bottom edge showing the value's position in `[min, max]`. Editing is
/// unchanged (the existing clamped scrub / inline-edit / RPC write) — the
/// gauge is the *visible range* affordance, so its bars are `pointer_transparent` and a press
/// falls through to the row's scrub. The fill is tagged `{GRID_TAG}#gauge<slot>` so its rendered
/// fraction is introspectable — the AI reads the gauge position from the
/// painted frame, not just the value.
fn ranged_slider_cell(slot: usize, value: f64, min: f64, max: f64, theme: &Theme) -> Scene {
    let frac = if max > min {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let track_w = VALUE_COL_W.saturating_sub(2 * CELL_PAD);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "frac in [0,1] times a small track width — non-negative, bounded by track_w"
    )]
    let fill_w = (frac * f64::from(track_w)) as u32;
    let strip_y = ROW_H.saturating_sub(GAUGE_TRACK_H);
    let bar = |w: u32, fill: Color, tag: Option<String>| {
        let mut node = ContainerNode::new(Vec::new())
            .with_style(BoxStyle::filled(fill).with_corner_radius(GAUGE_TRACK_H / 2))
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(w, GAUGE_TRACK_H))
                    .with_absolute_position(0, strip_y)
                    .with_pointer_transparent(true),
            );
        if let Some(t) = tag {
            node = node.with_tag(t);
        }
        Scene::Container(node)
    };
    let idle = SliderState::Idle;
    Scene::Container(
        ContainerNode::new(vec![
            // The number flows + centres exactly as the plain Float cell did.
            Scene::Text(TextNode::styled(
                CellValue::Float(value).display(),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(CELL_PX)
                    .with_fg(theme.resolve(ColorRole::OnSurface)),
            )),
            bar(track_w, slider_track_inactive(theme, idle), None),
            bar(
                fill_w,
                slider_accent_for(theme, idle),
                Some(format!("{GRID_TAG}#{GAUGE_PREFIX}{slot}")),
            ),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(track_w, ROW_H)),
        ),
    )
}

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
/// R931 — composite sub-tag for the array branch's "add element" button
/// (`{GRID_TAG}#addelem`), routing its click to [`PropertyGridExternal::add_elem`].
const ADD_ELEM_TAG: &str = "addelem";
/// R931 — composite sub-tag prefix for one element's "remove" button
/// (`{GRID_TAG}#rmelem{k}`), routing its click to
/// [`PropertyGridExternal::remove_elem`]. A distinct prefix so `dispatch_send`
/// never reads it as a numeric cell index or an `elem.<k>` leaf.
const RM_ELEM_PREFIX: &str = "rmelem";

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
/// arbitrary colour is set either through the popup's GUI hex-entry field
/// (R869/R870 — the shared `EDIT_TF`, Tab/click to focus, Enter commits via
/// `Color::from_hex`) or `intervene value.<i>` with a hex string (the AI path).
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
    "Mesh",       // 1  Identity (R1176 asset path)
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
        CellValue::Text("Player".to_owned()),                // 0  Name
        CellValue::Text("/proj/meshes/hero.fbx".to_owned()), // 1  Mesh (asset path)
        CellValue::Bool(true),                               // 2  Visible
        CellValue::Bool(false),                              // 3  Locked
        CellValue::Int(3),                                   // 4  Layer
        CellValue::Int(100),                                 // 5  Health
        CellValue::Float(12.5),                              // 6  Position X
        CellValue::Float(-4.0),                              // 7  Position Y
        CellValue::Float(1.0),                               // 8  Opacity
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
// expands into its X / Y / Z field rows — the engine / the toolkit Details
// core depth). Categories and structs are the SAME thing — a collapsible
// branch — so the panel migrates off the 2-level `GroupOrderState` group-by proxy onto the
// arbitrary-depth WAI-ARIA Tree substrate (`flat_visible` / `resolve_tree_key` / `TreeNode`, R811/R820/R821):
// one visible-row sequence the paint, the id-keyed roving cursor, the collapse
// set, the a11y `tree` and the name filter all read, so no two derived sequences
// can diverge.
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
/// ([`split_send_payload`](pinion_core::composite_tag::split_send_payload)), so a `:`
/// in the id would mis-split a header click.
const CAT_PREFIX: &str = "cat.";
/// Branch-node id prefix for a struct property (`struct.Position`).
const STRUCT_PREFIX: &str = "struct.";
/// R931 — branch-node id prefix for an **array** property (`arr.weights`). An
/// array is a collapsible branch like a struct, but its children are *dynamic*
/// (the editor grows / shrinks the element list) and live in a **separate**
/// `Signal<Vec<CellValue>>` sub-model, not the flat value model — so add /
/// remove / reorder never displace a scalar leaf's stable `value_index`.
const ARR_PREFIX: &str = "arr.";
/// R931 — leaf-node id prefix for one array **element** (`elem.2`). The suffix
/// is the element's position in the array sub-model; unlike a scalar leaf id
/// (a bare decimal addressing the flat model), an element leaf's value lives in
/// the array sub-model at that position ([`ValueRef::Elem`]).
const ELEM_PREFIX: &str = "elem.";
/// R931 — the one demo array property: a `TArray<f32>` (the engine Details "growable
/// list of floats" — spawn weights / LOD distances). One array proves the
/// dynamic-collection path; multi-array is the same pattern repeated.
const ARR_BRANCH_ID: &str = "arr.weights";
/// R931 — the array property's display label (the branch row name).
const ARRAY_LABEL: &str = "Spawn Weights";
/// R931 — the homogeneous element kind of the demo array. `Float`, so an
/// element row reuses the scalar `Float` scrub + inline-edit path unchanged
/// (the whole point of the unified [`ValueRef`] value address).
const ARRAY_ELEM_KIND: CellKind = CellKind::Float;

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
        Self {
            id,
            label: label.to_owned(),
            expanded: true,
            value_index: None,
            children,
        }
    }

    /// R931 — an **array element** leaf at array position `k`. Its `value_index` is `None`
    /// (the value lives in the array sub-model, not the flat model, addressed
    /// by [`ValueRef::Elem`]); its id (`elem.<k>`) is what marks it an editable leaf to [`row_ref`]. The
    /// label is the engine-style `[k]` index.
    fn elem_leaf(k: usize) -> Self {
        Self {
            id: format!("{ELEM_PREFIX}{k}"),
            label: format!("[{k}]"),
            expanded: false,
            value_index: None,
            children: Vec::new(),
        }
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

/// R931 — a unified **value address**: where the editable value behind a leaf
/// row lives. A `Scalar` leaf addresses the flat value model (`model[i]`, every
/// R836 scalar property + R921 struct field); an `Elem` leaf addresses the
/// array sub-model (`array[k]`, an R931 array element). Generalising the edit
/// latch / scrub arm / read+write funnel over this enum (rather than a bare
/// `usize`) is what lets an array element reuse the *same* inline editor and
/// scrub path as a scalar — no parallel edit machinery, just a two-armed value
/// accessor ([`PropertyGridExternal::value_at`] / [`set_value_at`]).
///
/// [`set_value_at`]: PropertyGridExternal::set_value_at
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum ValueRef {
    /// A flat value-model slot (`model[i]`) — every scalar / struct-field leaf.
    Scalar(usize),
    /// An array sub-model element (`array[k]`) — an R931 array element leaf.
    Elem(usize),
}

impl ValueRef {
    /// The index within this address's backing store (the flat-model slot for a
    /// `Scalar`, the array position for an `Elem`) — the one place the
    /// `Scalar(i) | Elem(i) => i` projection is written.
    fn slot(self) -> usize {
        match self {
            ValueRef::Scalar(i) | ValueRef::Elem(i) => i,
        }
    }
}

/// R931 — the value address a tree-row id points at, or `None` for a branch id
/// (a `cat.` / `struct.` / `arr.` prefix). The unified peer of
/// [`row_value_index`]: a bare decimal is a [`ValueRef::Scalar`]; an `elem.<k>`
/// id is a [`ValueRef::Elem`]. The one place "is this row an editable leaf, and
/// where does its value live?" is decided, shared by the click router, the
/// keyboard activation, the view and the a11y builder.
fn row_ref(id: &str) -> Option<ValueRef> {
    // R1231 — the `elem<k>` decode via the shared `composite_tag::prefixed_index`
    // (the bare-decimal Scalar fallback is not a prefixed key).
    if let Some(k) = prefixed_index(id, ELEM_PREFIX) {
        return Some(ValueRef::Elem(k));
    }
    id.parse::<usize>().ok().map(ValueRef::Scalar)
}

/// R931 — the tree-row node id that addresses `value_ref` (the inverse of
/// [`row_ref`]). A `Scalar(i)` is the bare decimal "6"; an `Elem(k)` is
/// `elem.<k>`. Used to map the edit latch / cursor back to a visible row id for
/// the visibility gate ([`PropertyGridExternal::editing_if_visible`]).
fn ref_node_id(value_ref: ValueRef) -> String {
    match value_ref {
        ValueRef::Scalar(i) => i.to_string(),
        ValueRef::Elem(k) => format!("{ELEM_PREFIX}{k}"),
    }
}

// R935.1 — the immutable `find_node` is the lifted `tree_nav::find_node`
// (generic over `TreeNode`), shared with `hello-tree-reparent`; the local copy
// was removed when that 2nd consumer landed.

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
fn struct_is_modified(
    model: &[CellValue],
    defaults: &[CellValue],
    tree: &[PropertyNode],
    id: &str,
) -> bool {
    struct_field_indices(tree, id)
        .iter()
        .any(|&i| leaf_modified(model, defaults, i))
}

/// R921.1 / R936 — whether the leaf at slot `i` of `values` differs from its
/// class default at the same slot in `defaults`. Model-agnostic — `values` /
/// `defaults` are the flat scalar model + its baseline for a **scalar** leaf, or
/// the array sub-model + [`use_array_defaults`] baseline for an **array element**
/// leaf (R936) — both are tree leaves comparing a value to a same-index baseline,
/// so the one gate serves both (no byte-identical element copy). The ONE leaf
/// modified-gate the paint (reset arrow), the a11y (reset `button`), the External
/// (`is_modified` / `struct_is_modified` / element modified) and the RPC
/// (`modified.<addr>`) share — the R886.1 one-gate, the leaf peer of
/// [`struct_is_modified`]. Out-of-range / absent (an added element past the
/// default length, with no class counterpart) reads as not-modified.
fn leaf_modified(values: &[CellValue], defaults: &[CellValue], i: usize) -> bool {
    match (values.get(i), defaults.get(i)) {
        (Some(value), Some(baseline)) => property_modified(value, baseline),
        _ => false,
    }
}

/// R936 — whether the array branch differs from its class default: its length
/// changed (elements added / removed), or any in-range element differs. The
/// array peer of [`struct_is_modified`] — the ONE roll-up the paint (array
/// branch reset arrow), the a11y (array reset `button`) and the RPC
/// (`array_modified.<id>`) share, so the arrow, the AT button and the query can
/// never disagree. An added element (past the default length) is caught by the
/// length term, not the per-element [`leaf_modified`] (which has no baseline for
/// it).
fn array_is_modified(elements: &[CellValue], defaults: &[CellValue]) -> bool {
    elements.len() != defaults.len()
        || (0..elements.len()).any(|k| leaf_modified(elements, defaults, k))
}

/// R921 — the number of leaf (editable property) descendants of a branch — the
/// `(count)` detail a category header shows ("Identity (3)", "Transform (6)").
fn leaf_descendant_count(tree: &[PropertyNode], id: &str) -> usize {
    fn count(node: &PropertyNode) -> usize {
        // R931 — a leaf is any node whose id is a value address (a scalar slot
        // OR an array element), not just `value_index.is_some()`: an array
        // element leaf has `value_index = None` (its value lives in the array
        // sub-model) yet is still an editable property the category count.
        if row_ref(&node.id).is_some() {
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
/// R931 — first-paint array elements (the `TArray<f32>` sub-model). Three
/// floats, so the array boots non-empty (the editor can scrub / remove from a
/// populated list, then grow it). The elements live here, **not** in the flat
/// [`default_properties`] model, so add / remove never shifts a scalar leaf's
/// `value_index`.
fn default_array() -> Vec<CellValue> {
    vec![
        CellValue::Float(1.0),
        CellValue::Float(0.5),
        CellValue::Float(0.25),
    ]
}

/// R931 — the value a freshly-added array element starts at: the zero of the
/// array's homogeneous element kind ([`ARRAY_ELEM_KIND`], the SSOT). A new
/// element seeds editable like any other (the data-grid R930 `default_row`
/// peer).
fn default_element() -> CellValue {
    match ARRAY_ELEM_KIND {
        CellKind::Int => CellValue::Int(0),
        CellKind::Text => CellValue::Text(String::new()),
        CellKind::Bool => CellValue::Bool(false),
        // Float (this demo's array) + the popup kinds fall back to a 0.0 float.
        _ => CellValue::Float(0.0),
    }
}

/// R931 — parse a `"from,to"` reorder payload (the `move_elem` invoke argument)
/// into a pair of element indices, or `None` if it is malformed.
fn parse_move_pair(s: &str) -> Option<(usize, usize)> {
    let (from, to) = s.split_once(',')?;
    Some((from.trim().parse().ok()?, to.trim().parse().ok()?))
}

/// R931 — the synthesized element leaves of the array branch: one `elem.<k>`
/// leaf per element of an array of length `len`. The array sub-model is the
/// SSOT for the count; this is a pure function of `len`, regenerated (never
/// incrementally patched) by every mutator ([`PropertyGridExternal::add_elem`]
/// / [`remove_elem`] / [`move_elem`]) so the tree's element rows can never
/// drift from the sub-model.
///
/// [`remove_elem`]: PropertyGridExternal::remove_elem
/// [`move_elem`]: PropertyGridExternal::move_elem
fn array_children(len: usize) -> Vec<PropertyNode> {
    (0..len).map(PropertyNode::elem_leaf).collect()
}

fn default_tree() -> Vec<PropertyNode> {
    let cat = |label: &str, children: Vec<PropertyNode>| {
        PropertyNode::branch(format!("{CAT_PREFIX}{label}"), label, children)
    };
    let vec3 = |label: &str, x: usize, y: usize, z: usize| {
        PropertyNode::branch(
            format!("{STRUCT_PREFIX}{label}"),
            label,
            vec![
                PropertyNode::leaf(x, "X"),
                PropertyNode::leaf(y, "Y"),
                PropertyNode::leaf(z, "Z"),
            ],
        )
    };
    vec![
        cat(
            "Identity",
            vec![
                PropertyNode::leaf(0, "Name"),
                PropertyNode::leaf(1, "Mesh"),
                PropertyNode::leaf(4, "Layer"),
            ],
        ),
        cat(
            "Appearance",
            vec![
                PropertyNode::leaf(2, "Visible"),
                PropertyNode::leaf(8, "Opacity"),
                PropertyNode::leaf(9, "Blend"),
                PropertyNode::leaf(11, "Tint"),
            ],
        ),
        cat(
            "Physics",
            vec![
                PropertyNode::leaf(3, "Locked"),
                PropertyNode::leaf(10, "Body"),
            ],
        ),
        cat("Stats", vec![PropertyNode::leaf(5, "Health")]),
        cat(
            "Transform",
            vec![vec3("Position", 6, 7, 12), vec3("Scale", 13, 14, 15)],
        ),
        // R931 — the Gameplay category holds the demo **array** property: a
        // collapsible `arr.weights` branch whose element leaves are synthesized
        // from the array sub-model length (`default_array().len()` at boot).
        cat(
            "Gameplay",
            vec![PropertyNode::branch(
                ARR_BRANCH_ID.to_owned(),
                ARRAY_LABEL,
                array_children(default_array().len()),
            )],
        ),
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
/// same `default_properties()` the model starts from). A property is "modified" when its current
/// value differs from this baseline (the engine / the toolkit Details "reset
/// arrow" appears only on a changed property), and `reset` restores it. One
/// immutable home (an `Rc<Vec<…>>`, never a `Signal` — defaults do not change) the External,
/// the view, and the a11y all read, so the modified indicator and the reset
/// target can never disagree.
#[must_use]
fn use_property_defaults() -> Rc<Vec<CellValue>> {
    let owner = Owner::current().expect("use_property_defaults requires an active Owner scope");
    owner.cache("property_grid.defaults", default_properties)
}

/// R931 — the **array sub-model** SSOT: the `TArray<f32>` elements, a
/// `Signal<Vec<CellValue>>` orthogonal to the flat [`use_property_model`]. Held
/// separately so growing / shrinking / reordering the element list (the dynamic
/// collection) never displaces a scalar leaf's stable `value_index` — an array
/// element is addressed by its position here ([`ValueRef::Elem`]), not by a
/// flat-model index. Shared by the coordinator (mutates it), the view (reads an
/// element value), the a11y and the RPC; reading it inside the view subscribes,
/// so an element edit / add / remove repaints.
#[must_use]
fn use_array_model() -> Rc<Signal<Vec<CellValue>>> {
    let owner = Owner::current().expect("use_array_model requires an active Owner scope");
    owner.cache("property_grid.array", || Signal::new(default_array()))
}

/// R936 — the frozen array baseline: the class-default elements
/// ([`default_array`] at boot), the array peer of [`use_property_defaults`]. An
/// element is "modified" when it differs from the element at its position here,
/// and a reset restores it; the array branch is modified when its length or any
/// element differs. One immutable home (an `Rc<Vec<…>>`, never a `Signal` —
/// defaults do not change) the External, the view and the a11y all read, so the
/// per-element / array modified indicator and the reset target cannot disagree.
#[must_use]
fn use_array_defaults() -> Rc<Vec<CellValue>> {
    let owner = Owner::current().expect("use_array_defaults requires an active Owner scope");
    owner.cache("property_grid.array_defaults", default_array)
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
    use_text_edit_state(SEARCH_TF_TAG)
        .text()
        .trim()
        .to_lowercase()
}

/// R921 — the per-node filter match: a node survives the search iff its
/// *searchable name* contains the (already-lowercased) query. A **leaf**
/// matches on its qualified [`PROPERTY_NAMES`] entry (so "position" finds "Position X" even
/// though the in-tree label is the short "X"); a **branch** matches on its
/// category / struct label. [`flat_visible_filtered`] then keeps any node on a path to a match
/// (the toolkit recursive-filter semantics).
fn node_matches_query(node: &PropertyNode, query: &str) -> bool {
    // R931 — an array element leaf (`value_index = None`, id `elem.<k>`) has no
    // per-element name, so it matches on its array's label ([`ARRAY_LABEL`]) —
    // so "weights" surfaces every element under the array, the same way a struct
    // field is searchable by its qualified name (no scalar-vs-element asymmetry).
    let name = match node.value_index {
        Some(i) => PROPERTY_NAMES
            .get(i)
            .copied()
            .unwrap_or(node.label.as_str()),
        None if node.id.starts_with(ELEM_PREFIX) => ARRAY_LABEL,
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

/// Edit-mode latch — `Some(value_ref)` while that leaf's value is being
/// text-edited (the todomvc `editing_id`). `None` = navigating. R931 — keyed by
/// [`ValueRef`], not a bare index, so the *one* shared inline editor + popup
/// machinery serves a scalar leaf and an array element identically.
#[must_use]
fn use_editing_row() -> Rc<Signal<Option<ValueRef>>> {
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
    editing: &Signal<Option<ValueRef>>,
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
/// `Self::begin_edit` can seed it. Mutations write the Signals directly —
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
    /// R931 — the numeric leaf being scrubbed (a scalar `Int`/`Float` slot or a
    /// `Float` array element), addressed uniformly by [`ValueRef`].
    source: ValueRef,
    kind: CellKind,
    base: f64,
}

struct PropertyGridExternal {
    model: Rc<Signal<Vec<CellValue>>>,
    /// R931 — the array sub-model ([`use_array_model`]): the `TArray<f32>`
    /// elements, orthogonal to the flat `model`. An [`ValueRef::Elem`] addresses
    /// a position here; add / remove / reorder mutate this `Signal`, never the
    /// flat model, so a scalar leaf's `value_index` is permanent.
    array: Rc<Signal<Vec<CellValue>>>,
    /// R919 — the class-default baseline ([`use_property_defaults`]). A property
    /// is modified when `model[i]` differs from `defaults[i]`; `reset` writes the
    /// baseline back through the [`set_value`](Self::set_value) funnel.
    defaults: Rc<Vec<CellValue>>,
    /// R936 — the frozen array baseline ([`use_array_defaults`]), the element peer
    /// of `defaults`. An element is modified when it differs from the element here;
    /// `reset_element` / `reset_array` write it back. Held so the modified
    /// roll-up / reset funnels are sound on the RPC thread (no Owner scope).
    array_defaults: Rc<Vec<CellValue>>,
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
    editing_row: Rc<Signal<Option<ValueRef>>>,
    editor: Rc<TextEditState>,
    popup_cursor: Rc<Signal<Option<usize>>>,
    popup_hover: Rc<Signal<Option<usize>>>,
    /// R1177 — the embedded asset picker's state, captured as `Rc`s (the
    /// `model` / `search` pattern) so [`open_asset_dialog`](Self::open_asset_dialog)
    /// is a method over captured state with NO `use_*` hooks. It opens Owner-free,
    /// so the picker is RPC-openable via `invoke begin <asset_slot>` exactly like
    /// the choice / colour popups (the R1176 free-fn-with-hooks shape no-op'd a
    /// direct RPC `begin` on the RPC thread — the §2 #2 asymmetry this clears).
    asset_modal: Rc<ModalState>,
    asset_dir: Rc<DirectoryState>,
    asset_target: Rc<Signal<Option<usize>>>,
    /// R875 / R931 — the numeric leaf armed by a `PointerDown` over a numeric
    /// row, before the first `pointer_move` calibrates the drag. `None` for a
    /// press on a non-numeric row (which never scrubs). A [`ValueRef`] so a
    /// `Float` array element scrubs through the same path as a scalar `Float`.
    scrub_armed: Cell<Option<ValueRef>>,
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
        editing_row: Rc<Signal<Option<ValueRef>>>,
        editor: Rc<TextEditState>,
        popup_cursor: Rc<Signal<Option<usize>>>,
        popup_hover: Rc<Signal<Option<usize>>>,
    ) -> Self {
        Self {
            model,
            // R931 — resolved via its hook (like `defaults` / `search`), the same
            // cached array sub-model the view reads (Owner::cache dedup).
            array: use_array_model(),
            defaults: use_property_defaults(),
            // R936 — resolved via its hook (like `defaults` / `array`), the same
            // cached frozen baseline the view + a11y read (Owner::cache dedup).
            array_defaults: use_array_defaults(),
            tree,
            cursor,
            // Resolved via its hook (like `defaults`) — the same cached
            // `TextEditState` the search box owns (Owner::cache dedup).
            search: use_text_edit_state(SEARCH_TF_TAG),
            editing_row,
            editor,
            popup_cursor,
            popup_hover,
            // R1177 — resolved via their hooks (like `defaults` / `search`), the
            // same cached `Rc`s the view / access_node / reducer read (Owner::cache
            // dedup). Captured so `open_asset_dialog` opens the picker Owner-free.
            asset_modal: use_modal(ASSET_MODAL_KEY),
            asset_dir: use_directory_state(ASSET_DIR_TAG, seed_asset_directory, || {
                ASSET_ROOT_DIR.to_string()
            }),
            asset_target: use_asset_target(),
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
    fn editing_if_visible(&self) -> Option<ValueRef> {
        let row = self.editing_row.get()?;
        let query = self.search.text().trim().to_lowercase();
        let id = ref_node_id(row);
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
        // R936 — the object is dirty if any scalar OR the array branch is
        // modified; ignoring the array here would let `any_modified` falsely
        // report a clean object while the element list differs from default.
        (0..self.count()).any(|i| self.is_modified(i)) || self.array_modified()
    }

    /// R936 — whether the array element at `k` differs from its class default
    /// (the per-element reset-arrow gate). The element peer of
    /// [`is_modified`](Self::is_modified); both delegate to the [`leaf_modified`]
    /// one-gate (a scalar leaf reads the flat model + `defaults`, an element leaf
    /// the array sub-model + `array_defaults`) the paint + a11y + RPC also use.
    fn element_is_modified(&self, k: usize) -> bool {
        leaf_modified(&self.array.get(), &self.array_defaults, k)
    }

    /// R936 — whether the array branch differs from its class default (length or
    /// any element). The array peer of [`struct_modified`](Self::struct_modified);
    /// delegates to the [`array_is_modified`] one-gate.
    fn array_modified(&self) -> bool {
        array_is_modified(&self.array.get(), &self.array_defaults)
    }

    /// R936 — reset the array element at `k` to its class default, returning
    /// whether it changed. The element peer of
    /// [`reset_to_default`](Self::reset_to_default): a no-op `false` when the
    /// element is already default or has no class counterpart (an added element
    /// past the default length — [`reset_array`](Self::reset_array) truncates
    /// those). Routes through the shared [`set_value_at`](Self::set_value_at)
    /// `Elem` funnel, so a reset is the same write a scrub / inline-edit commit
    /// makes — a value write, no length change, so no cursor re-anchor is needed
    /// (exactly like the scalar `reset_to_default`).
    fn reset_element(&self, k: usize) -> bool {
        if !self.element_is_modified(k) {
            return false;
        }
        let Some(baseline) = self.array_defaults.get(k).cloned() else {
            return false;
        };
        self.set_value_at(ValueRef::Elem(k), baseline);
        true
    }

    /// R936 — reset the whole array branch to its class default (length +
    /// content), returning whether it changed. The array peer of
    /// [`reset_struct`](Self::reset_struct), but a wholesale restore rather than
    /// N field writes, because the length itself can differ. Follows the R930.1
    /// destructive-mutation discipline a length change demands: cancel an
    /// in-flight element edit (the array content is wholesale-replaced, so an
    /// `Elem` latch would commit a stale slot), regenerate the element leaves,
    /// and re-anchor the roving cursor through the same
    /// [`reanchor_array_cursor`](Self::reanchor_array_cursor) every element
    /// mutator uses (so a cursor on an element the restore truncated never
    /// strands on a vanished `elem.<k>`).
    fn reset_array(&self) -> bool {
        if !self.array_modified() {
            return false;
        }
        if matches!(self.editing_row.get(), Some(ValueRef::Elem(_))) {
            self.cancel_active_edit();
        }
        self.array.set(self.array_defaults.as_ref().clone());
        self.sync_array_children();
        self.reanchor_array_cursor(self.array_len());
        true
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

    /// R919 / R936 — reset every modified property to its default, returning the
    /// count reset (`0` when nothing was modified). The array branch counts as
    /// one reset unit (a wholesale [`reset_array`](Self::reset_array) restore),
    /// so "Reset all" returns the whole object to default — scalars AND the
    /// element list — never leaving the array dirty behind a "clean" readout.
    fn reset_all(&self) -> usize {
        let scalars = (0..self.count())
            .filter(|&i| self.reset_to_default(i))
            .count();
        scalars + usize::from(self.reset_array())
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

    /// R921 — whether any field of struct `struct_id` is modified from its default
    /// (the struct row's reset-arrow gate — a struct is "modified" iff any of
    /// its components is, the engine Details struct-row roll-up). Delegates to
    /// the [`struct_is_modified`] one-gate the paint + a11y also use.
    fn struct_modified(&self, struct_id: &str) -> bool {
        struct_is_modified(
            &self.model.get(),
            &self.defaults,
            &self.tree.get(),
            struct_id,
        )
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

    /// R921 — a struct row's collapsed-summary value, the parenthesised tuple
    /// of its field display values (`"(12.5, -4, 0)"` for a `Vector3`), so a collapsed struct
    /// still shows its value at a glance (the engine / the toolkit Details
    /// struct-row summary). Reads the field displays through the same [`CellValue::display`]
    /// the leaf cells use.
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
    /// writes the same shared model `Signal` the inline editor (`commit_edit`),
    /// the RPC `value.<i>` intervene, and the asset picker ([`confirm_asset`])
    /// also write, converging on one source of truth via [`write_property_value`]
    /// (R964 — a ranged scalar's Float write is clamped to its interval there, so
    /// that one funnel bounds every writer — the data-grid `set_cell` pattern).
    fn set_value(&self, source: usize, value: CellValue) {
        write_property_value(&self.model, source, value);
    }

    /// R931 — the array sub-model length (the element count). The SSOT for the
    /// number of `elem.<k>` rows; the tree's array children are a pure function
    /// of it ([`array_children`]).
    fn array_len(&self) -> usize {
        self.array.get().len()
    }

    /// R931 — cancel any in-flight inline edit / popup at the signal level (no
    /// Owner / focus restore), for a destructive mutation that runs on the RPC
    /// thread ([`remove_elem`](Self::remove_elem)). Clears the latch, wipes the
    /// shared editor, and clears the popup cursor / hover — the captured `Rc`
    /// peer of the Owner-scoped [`end_edit_mode`].
    fn cancel_active_edit(&self) {
        self.editor.set_text(String::new());
        clear_popup(&self.editing_row, &self.popup_cursor, &self.popup_hover);
    }

    /// R931 — read the value behind a [`ValueRef`], branching on which model it
    /// addresses. The unified read peer of [`set_value_at`](Self::set_value_at):
    /// a scalar leaf reads the flat model, an array element reads the array
    /// sub-model. `None` for an out-of-range address.
    fn value_at(&self, value_ref: ValueRef) -> Option<CellValue> {
        match value_ref {
            ValueRef::Scalar(i) => self.model.get().get(i).cloned(),
            ValueRef::Elem(k) => self.array.get().get(k).cloned(),
        }
    }

    /// R931 — write a value behind a [`ValueRef`]. A `Scalar` routes through the
    /// existing flat-model [`set_value`](Self::set_value) funnel (unchanged); an
    /// `Elem` writes the array sub-model slot. The one write the scrub commit and
    /// the inline-editor commit (`commit_edit`) both call, so a scalar and an
    /// array element commit through identical machinery.
    fn set_value_at(&self, value_ref: ValueRef, value: CellValue) {
        match value_ref {
            ValueRef::Scalar(i) => self.set_value(i, value),
            ValueRef::Elem(k) => self.array.set_with(|prev| {
                let mut next = prev.clone();
                if let Some(slot) = next.get_mut(k) {
                    *slot = value;
                }
                next
            }),
        }
    }

    /// R931 — regenerate the array branch's element leaves to match the current
    /// sub-model length (the data-grid R930 "tree children are a pure function of
    /// the model" discipline): one writer, so the tree's `elem.<k>` rows can
    /// never drift from the array sub-model. Called after every element add /
    /// remove / reorder.
    fn sync_array_children(&self) {
        let len = self.array_len();
        self.tree.set_with(|prev| {
            let mut next = prev.clone();
            if let Some(branch) = find_node_mut(&mut next, ARR_BRANCH_ID) {
                branch.children = array_children(len);
            }
            next
        });
    }

    /// R931 — append a new element (`0.0`) to the array, regenerate the element
    /// leaves, and return the new element's index. Mirrors the data-grid R930
    /// `add_row`: the sub-model is the SSOT, a push grows it, and the next paint
    /// re-derives the rows. Moves the cursor onto the new element so the AI / a
    /// keyboard user lands on what they just created.
    fn add_elem(&self) -> usize {
        let index = self.array_len();
        self.array.set_with(|prev| {
            let mut next = prev.clone();
            next.push(default_element());
            next
        });
        self.sync_array_children();
        self.move_cursor(&format!("{ELEM_PREFIX}{index}"));
        index
    }

    /// R931 — remove the element at `index`, returning whether it existed. The
    /// R930.1 destructive-mutation discipline: a structural removal **must**
    /// invalidate any latch / cursor that addressed the gone (or now-shifted)
    /// element. (1) If an in-flight edit was on *this* array (a removed or
    /// re-indexed element), cancel it — committing would write a stale slot
    /// (the R930.1 reachable-panic class). (2) Re-anchor the roving cursor
    /// onto the element now occupying the freed slot (or the new last
    /// element), the spreadsheet / the toolkit list behaviour, so the cursor
    /// never strands on a vanished `elem.<k>`.
    fn remove_elem(&self, index: usize) -> bool {
        if index >= self.array_len() {
            return false;
        }
        // (1) Cancel an in-flight element edit only when the removal actually
        // disturbs it — an edit on `elem.k` is stale iff `k >= index` (that
        // element vanished or shifted down one); an edit on an *earlier* element
        // (`k < index`) is untouched, so it must NOT be cancelled. This mirrors
        // the precise `reanchor_array_cursor` gate below — the latch and the
        // cursor stay consistent (cancel rather than silently retarget a shifted
        // slot, the R930.1 reachable-stale-index class).
        if matches!(self.editing_row.get(), Some(ValueRef::Elem(k)) if k >= index) {
            self.cancel_active_edit();
        }
        self.array.set_with(|prev| {
            let mut next = prev.clone();
            next.remove(index);
            next
        });
        self.sync_array_children();
        self.reanchor_array_cursor(index);
        true
    }

    /// R931 — move the element at `from` to position `to` (a Vec remove+insert,
    /// the array's canonical reorder — the AI-first / keyboard primary path; a
    /// live drag-to-reorder *gesture* is honest carry, R929 precedent). Returns
    /// whether it moved. Follows the moved element with the cursor. Both indices
    /// are clamped into range; `from == to` is a no-op.
    fn move_elem(&self, from: usize, to: usize) -> bool {
        let len = self.array_len();
        if from >= len || to >= len || from == to {
            return false;
        }
        self.array.set_with(|prev| {
            let mut next = prev.clone();
            let item = next.remove(from);
            next.insert(to, item);
            next
        });
        self.sync_array_children();
        self.move_cursor(&format!("{ELEM_PREFIX}{to}"));
        true
    }

    /// R931 — re-anchor the roving cursor after an element at `removed` was
    /// dropped (the R930.1 `reanchor` peer for the array). If the cursor was on
    /// an array element that no longer exists (or was past the freed slot), land
    /// it on the element now at `removed` (clamped to the new last element); if
    /// the array emptied, fall onto the array branch row. A cursor elsewhere
    /// (a scalar leaf, another branch) is untouched.
    fn reanchor_array_cursor(&self, removed: usize) {
        let Some(cursor_id) = self.cursor.get() else {
            return;
        };
        let Some(ValueRef::Elem(k)) = row_ref(&cursor_id) else {
            return;
        };
        // The cursor's element survives unmoved only if it was strictly before
        // the removed slot; at-or-after it either vanished (`k == removed`) or
        // shifted down one (`k > removed`).
        if k < removed {
            return;
        }
        let len = self.array_len();
        if len == 0 {
            self.move_cursor(ARR_BRANCH_ID);
            return;
        }
        // Follow the element: a cursor *after* the removal lands on its shifted
        // position (`k - 1`); a cursor *on* the removed element lands on the
        // element that took its slot. Clamp to the new last element.
        let target = if k > removed { k - 1 } else { removed };
        self.move_cursor(&format!("{ELEM_PREFIX}{}", target.min(len - 1)));
    }

    /// R875 — arm a numeric scrub: a `PointerDown` over a numeric (`Int` /
    /// `Float`) row records the source so the first capture `pointer_move` can
    /// calibrate. A press on a non-numeric row leaves the arm clear (it never
    /// scrubs — bool toggles, choice / colour open popups, text edits).
    fn arm_scrub(&self, source: ValueRef) {
        // R917 — a fresh press starts a fresh calibration (self-contained scrub;
        // never inherits a stale base from a drag whose release was missed — the
        // R51.34 capture lock makes that unreachable, but the arm should not
        // depend on it).
        self.scrub_cal.end();
        let numeric = matches!(
            self.value_at(source).map(|v| v.kind()),
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
            match self.value_at(source) {
                Some(CellValue::Int(i)) => Some(ScrubDrag {
                    source,
                    kind: CellKind::Int,
                    base: i as f64,
                }),
                Some(CellValue::Float(f)) => Some(ScrubDrag {
                    source,
                    kind: CellKind::Float,
                    base: f,
                }),
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
        self.set_value_at(drag.source, next);
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
        self.scrub_cal
            .traveled_beyond(GRID_W_PX, DRAG_CLICK_THRESHOLD_PX)
    }

    /// Enter edit mode on `row`. A text / int / float row latches
    /// `editing_row`, seeds the shared inline editor with the formatted value
    /// (caret at the trailing edge), and requests focus into the field. A
    /// choice row opens its popup instead (`open_choice`, focus stays on the
    /// grid). Returns `false` for a bool row (bools toggle) or an
    /// out-of-range index.
    fn begin_edit(&self, row: ValueRef) -> bool {
        let Some(value) = self.value_at(row) else {
            return false;
        };
        // R1176 — an asset-path leaf opens the embedded file picker instead of
        // the inline text editor (the Choice / Colour precedent: a non-text
        // editing surface). R1177 — `open_asset_dialog` is a method over captured
        // `Rc`s (no hooks), so it opens Owner-free: a direct RPC `invoke begin`
        // opens the picker for an AI to drive, exactly like the choice / colour
        // popups (no more GUI-only asymmetry).
        if let ValueRef::Scalar(i) = row {
            if is_asset_slot(i) {
                return self.open_asset_dialog(i);
            }
        }
        // Choice / Colour are scalar-only (an array element is a `Float`); their
        // popup path addresses the flat model by index, so they only fire on a
        // `Scalar` leaf.
        if matches!(value, CellValue::Choice { .. }) {
            return match row {
                ValueRef::Scalar(i) => self.open_choice(i),
                ValueRef::Elem(_) => false,
            };
        }
        if matches!(value, CellValue::Color(_)) {
            return match row {
                ValueRef::Scalar(i) => self.open_color(i),
                ValueRef::Elem(_) => false,
            };
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
        self.editing_row.set(Some(ValueRef::Scalar(row)));
        self.popup_cursor.set(Some(*selected));
        self.popup_hover.set(None);
        true
    }

    /// Commit option `i` into the open choice row, then close the popup. The
    /// pointer (option click) + RPC (`choose`) commit path; returns whether
    /// a choice was committed.
    fn commit_choice_index(&self, i: usize) -> bool {
        let Some(ValueRef::Scalar(row)) = self.editing_row.get() else {
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
        let cursor = COLOR_SWATCHES
            .iter()
            .position(|(sw, _)| sw == c)
            .unwrap_or(0);
        self.editing_row.set(Some(ValueRef::Scalar(row)));
        self.popup_cursor.set(Some(cursor));
        self.popup_hover.set(None);
        self.editor.seed(c.to_hex());
        true
    }

    /// R1176/R1177 — open the embedded file picker targeting flat-model slot
    /// `slot`: reset the browser to its root with no selection (a file dialog
    /// opens at a default location), stash the target slot, and install the modal
    /// trap. A method over captured `Rc`s (the `open_choice` / `open_color`
    /// shape — no `use_*` hooks), so it opens Owner-free and is RPC-openable via
    /// `invoke begin <asset_slot>`. Returns `true`.
    fn open_asset_dialog(&self, slot: usize) -> bool {
        self.asset_dir.open_dir(ASSET_ROOT_DIR);
        self.asset_target.set(Some(slot));
        self.asset_modal.open(asset_dialog_members());
        true
    }

    /// Commit swatch `i` into the open colour row, then close the popup (the
    /// swatch click + RPC `pick_color` + keyboard path); `false` if out of
    /// range or not a colour row.
    fn commit_color_swatch(&self, i: usize) -> bool {
        let Some(ValueRef::Scalar(row)) = self.editing_row.get() else {
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
    /// R1564 — the popup-commit verbs, lifted out of
    /// [`invoke`](ExternalIntrospect::invoke) to keep that dispatch under the
    /// workspace line ceiling. Same split this file already made for
    /// [`dispatch_send`](Self::dispatch_send).
    ///
    /// # Errors
    ///
    /// [`InvokeError::TypeMismatch`] when the argument is not an index, and
    /// [`InvokeError::Rejected`] naming an index this surface cannot address.
    fn dispatch_popup(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match (path, args) {
            // Commit a popup option by index, closing the popup (the option
            // click + RPC choice-commit path). Requires an open choice popup.
            ("choose", IntrospectValue::Int(i)) => {
                let opt = usize::try_from(*i).map_err(|_| {
                    InvokeError::rejected(format!("{path}: {i} is not an option index"))
                })?;
                Ok(IntrospectValue::Bool(self.commit_choice_index(opt)))
            }
            // Commit a colour swatch by index, closing the popup (the swatch
            // click + RPC path). Requires an open colour popup.
            ("pick_color", IntrospectValue::Int(i)) => {
                let sw = usize::try_from(*i).map_err(|_| {
                    InvokeError::rejected(format!("{path}: {i} is not a swatch index"))
                })?;
                Ok(IntrospectValue::Bool(self.commit_color_swatch(sw)))
            }
            // Dismiss the open popup without committing (RPC + the keyboard
            // `Escape` / barrier path share `close_popup`).
            ("close_popup", _) => {
                self.close_popup();
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::TypeMismatch),
        }
    }

    fn dispatch_send(&mut self, s: &str) -> Result<IntrospectValue, InvokeError> {
        let pinion_core::composite_tag::SendPayload {
            key,
            event: event_name,
            ..
        } = pinion_core::composite_tag::require_send_payload("property_grid.send", s)?;
        if key == "dismiss" {
            if event_name == "PointerUp" {
                self.close_popup();
            }
            return Ok(IntrospectValue::Null);
        }
        if let Some(opt) = key.strip_prefix(CHOICE_OPT_PREFIX) {
            let i: usize = opt.parse().map_err(|_| {
                InvokeError::rejected(format!(
                    "property_grid.send: choice target {key:?} carries no option index"
                ))
            })?;
            if event_name == "PointerUp" {
                self.commit_choice_index(i);
            } else {
                self.set_popup_hover(event_name, i);
            }
            return Ok(IntrospectValue::Null);
        }
        if let Some(sw) = key.strip_prefix(COLOR_SW_PREFIX) {
            let i: usize = sw.parse().map_err(|_| {
                InvokeError::rejected(format!(
                    "property_grid.send: swatch target {key:?} carries no preset index"
                ))
            })?;
            if event_name == "PointerUp" {
                self.commit_color_swatch(i);
            } else {
                self.set_popup_hover(event_name, i);
            }
            return Ok(IntrospectValue::Null);
        }
        // R919 / R921 / R936 — a click on a row's reset arrow resets that target
        // to its default (the same `reset_*` funnel the RPC / keyboard use). The
        // suffix is a leaf value address (a scalar `reset6` or an element
        // `resetelem.2`) or a branch id (a struct `resetstruct.Position` reseting
        // every field, or the array `resetarr.weights` reseting the whole list).
        // The same `row_ref` vocabulary the click router uses elsewhere, so the
        // four reset targets route through one decode.
        if let Some(rest) = key.strip_prefix(RESET_PREFIX) {
            if event_name == "PointerUp" {
                match row_ref(rest) {
                    Some(ValueRef::Scalar(i)) => {
                        self.reset_to_default(i);
                    }
                    Some(ValueRef::Elem(k)) => {
                        self.reset_element(k);
                    }
                    None if rest.starts_with(ARR_PREFIX) => {
                        self.reset_array();
                    }
                    None => {
                        self.reset_struct(rest);
                    }
                }
            }
            return Ok(IntrospectValue::Null);
        }
        // R921 / R931 — a click on a branch row (category / struct / array)
        // toggles its collapse on the activation edge and moves the cursor onto
        // it.
        if key.starts_with(CAT_PREFIX)
            || key.starts_with(STRUCT_PREFIX)
            || key.starts_with(ARR_PREFIX)
        {
            if event_name == "PointerUp" {
                self.toggle_branch(key);
                self.move_cursor(key);
            }
            return Ok(IntrospectValue::Null);
        }
        // R931 — the array branch's "add element" button appends a new element
        // and returns its index. A non-`PointerUp` event is ignored (no hover).
        if key == ADD_ELEM_TAG {
            if event_name == "PointerUp" {
                let index = self.add_elem();
                return Ok(IntrospectValue::Int(
                    i64::try_from(index).expect("element index fits in i64"),
                ));
            }
            return Ok(IntrospectValue::Null);
        }
        // R931 — an element's "remove" button (`rmelem{k}`) drops that element
        // (with the R930.1 latch / cursor invalidation in `remove_elem`).
        if let Some(suffix) = key.strip_prefix(RM_ELEM_PREFIX) {
            if event_name == "PointerUp" {
                if let Ok(k) = suffix.parse::<usize>() {
                    self.remove_elem(k);
                }
            }
            return Ok(IntrospectValue::Null);
        }
        // A scalar leaf (a bare decimal addressing the flat model) or an array
        // element leaf (`elem.<k>`): both scrub / edit through the unified
        // `ValueRef` path, so an element row behaves exactly like a numeric
        // scalar row (R931).
        let value_ref = row_ref(key).ok_or_else(|| {
            InvokeError::rejected(format!("property_grid.send: {key:?} is not a row address"))
        })?;
        if self.value_at(value_ref).is_none() {
            return Err(InvokeError::rejected(format!(
                "property_grid.send: row {key:?} holds no editable value"
            )));
        }
        Ok(self.dispatch_leaf(key, event_name, value_ref))
    }

    /// R875 / R931 — the pointer handling for a leaf row (a scalar slot or an
    /// array element), split out of [`dispatch_send`](Self::dispatch_send): a
    /// `PointerDown` arms the numeric scrub, a `PointerUp` commits the click
    /// (suppressed if a scrub ran) — bool toggle / popup open being scalar-only
    /// affordances — and a `DoubleClick` enters edit mode. Both leaf kinds share
    /// this one path via [`ValueRef`]. Always succeeds (an unknown event is a
    /// no-op `Null`), so it returns the value directly.
    fn dispatch_leaf(
        &mut self,
        key: &str,
        event_name: &str,
        value_ref: ValueRef,
    ) -> IntrospectValue {
        match event_name {
            // R875 — arm a numeric scrub; the first capture `pointer_move`
            // calibrates it. A non-numeric press leaves the arm clear.
            "PointerDown" => {
                self.arm_scrub(value_ref);
                IntrospectValue::Null
            }
            "PointerUp" => {
                // R875 — a scrub committed its value live during the drag; its
                // release must NOT also fire the click action (open editor /
                // toggle / move cursor). `end_scrub` reports whether a drag ran.
                if self.end_scrub() {
                    return IntrospectValue::Null;
                }
                self.move_cursor(key);
                // Bool toggle / popup open are scalar-only affordances; an array
                // element is a `Float`, so a single click only moves the cursor
                // onto it (a double-click edits, a scrub drags its value).
                if let ValueRef::Scalar(idx) = value_ref {
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
                }
                IntrospectValue::Int(
                    i64::try_from(value_ref.slot()).expect("row index fits in i64"),
                )
            }
            // R875 — the capture lock lets the cursor stray off the row; a
            // release there arrives as PointerLeave / PointerCancel. Tear the
            // scrub down (the value is already committed) with no click.
            "PointerLeave" | "PointerCancel" => {
                self.end_scrub();
                IntrospectValue::Null
            }
            "DoubleClick" => IntrospectValue::Bool(self.begin_edit(value_ref)),
            _ => IntrospectValue::Null,
        }
    }

    /// R1667 — the `<slot>.<addr>` half of `query`: every per-leaf and
    /// per-element read.
    ///
    /// Split out because stating a bound is what made the combined function
    /// outgrow its line ceiling — each arm now names the range it refused
    /// against, where before every one of them answered the same word as a
    /// slot this grid does not publish.
    ///
    /// # Errors
    ///
    /// Returns [`ReadRefusal`] per the variants there.
    fn query_addressed(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        // R919 / R936 — is the leaf at `<addr>` modified from its class
        // default? A scalar index ("modified.6") reads the flat baseline;
        // an element address ("modified.elem.2") reads the array baseline.
        if let Some(addr) = path.strip_prefix("modified.") {
            return match row_ref(addr).ok_or(ReadRefusal::QueryTypeMismatch)? {
                ValueRef::Scalar(i) => (i < self.count())
                    .then(|| IntrospectValue::Bool(self.is_modified(i)))
                    .ok_or_else(|| outside("property", i, self.count())),
                ValueRef::Elem(k) => (k < self.array_len())
                    .then(|| IntrospectValue::Bool(self.element_is_modified(k)))
                    .ok_or_else(|| outside("element", k, self.array_len())),
            };
        }
        // R931 — `name.<addr>` / `kind.<addr>` / `value.<addr>` address
        // either a scalar leaf (a bare decimal) or an array element
        // (`elem.<k>`), the same `ValueRef` vocabulary the wire uses
        // throughout: the read shape is identical for both.
        if let Some(addr) = path.strip_prefix("name.") {
            return match row_ref(addr).ok_or(ReadRefusal::QueryTypeMismatch)? {
                ValueRef::Scalar(i) => PROPERTY_NAMES
                    .get(i)
                    .map(|name| IntrospectValue::Text((*name).to_owned()))
                    .ok_or_else(|| outside("property", i, PROPERTY_NAMES.len())),
                ValueRef::Elem(k) => (k < self.array_len())
                    .then(|| IntrospectValue::Text(format!("{ARRAY_LABEL} [{k}]")))
                    .ok_or_else(|| outside("element", k, self.array_len())),
            };
        }
        if let Some(addr) = path.strip_prefix("kind.") {
            let value = self
                .value_at(row_ref(addr).ok_or(ReadRefusal::QueryTypeMismatch)?)
                .ok_or_else(|| ReadRefusal::no_such_member(format!("no leaf at `{addr}`")))?;
            return Ok(IntrospectValue::Text(value.kind().name().to_owned()));
        }
        if let Some(addr) = path.strip_prefix("value.") {
            let value = self
                .value_at(row_ref(addr).ok_or(ReadRefusal::QueryTypeMismatch)?)
                .ok_or_else(|| ReadRefusal::no_such_member(format!("no leaf at `{addr}`")))?;
            return Ok(value.to_introspect());
        }
        // R964 — the bounded scalar's `"<lo>..<hi>"` interval, or
        // `"none"` for an unranged leaf / array element. The AI reads the
        // bounds before a `value.<i>` write (the clamp would otherwise be
        // silent), the §2#7 scene-as-data peer of the painted gauge. The
        // wire mirrors the data-grid R894 `col_range.<col>` sibling
        // (`"0..1000"` / `"none"`) so one range format reads across both
        // DCC widgets.
        if let Some(addr) = path.strip_prefix("range.") {
            return Ok(IntrospectValue::Text(
                match row_ref(addr).ok_or(ReadRefusal::QueryTypeMismatch)? {
                    ValueRef::Scalar(i) => scalar_range(i)
                        .map_or_else(|| "none".to_owned(), |(lo, hi)| format!("{lo}..{hi}")),
                    ValueRef::Elem(_) => "none".to_owned(),
                },
            ));
        }
        // R921 — a branch's collapse flag, by node id (`expanded.cat:…`
        // / `expanded.struct:…`). Null for a leaf / unknown id.
        if let Some(id) = path.strip_prefix("expanded.") {
            let tree = self.tree.get();
            return Ok(find_node(&tree, id)
                .filter(|n| !n.children.is_empty())
                .map_or(IntrospectValue::Null, |n| IntrospectValue::Bool(n.expanded)));
        }
        // R921 — a struct's collapsed-summary tuple (`struct_summary.struct.Position`).
        if let Some(id) = path.strip_prefix("struct_summary.") {
            if struct_field_indices(&self.tree.get(), id).is_empty() {
                return Ok(IntrospectValue::Null);
            }
            return Ok(IntrospectValue::Text(self.struct_summary(id)));
        }
        // R921 — whether any of a struct's fields is modified.
        if let Some(id) = path.strip_prefix("struct_modified.") {
            if struct_field_indices(&self.tree.get(), id).is_empty() {
                return Ok(IntrospectValue::Null);
            }
            return Ok(IntrospectValue::Bool(self.struct_modified(id)));
        }
        // R936 — whether the array branch is modified (length or any
        // element), keyed by its branch id; the array peer of
        // `struct_modified.<id>`. Null for any id but the array branch.
        if let Some(id) = path.strip_prefix("array_modified.") {
            return (id == ARR_BRANCH_ID)
                .then(|| IntrospectValue::Bool(self.array_modified()))
                .ok_or_else(|| {
                    ReadRefusal::no_such_member(format!(
                        "`{id}` is not the array branch (`{ARR_BRANCH_ID}`)"
                    ))
                });
        }
        Err(ReadRefusal::UnknownPath)
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
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
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
    fn pointer_move(&mut self, at: PointerReading) {
        self.scrub_to(f64::from(at.u()));
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

/// R1667 — the refusal an out-of-range property address owes its caller.
///
/// This grid addresses two spaces (`<i>` a scalar leaf, `elem.<k>` an array
/// element) and bounds both at half a dozen sites; before this round every one
/// of them answered a bare `None`, indistinguishable on the wire from asking
/// for a slot the grid does not publish at all.
fn outside(what: &str, i: usize, len: usize) -> ReadRefusal {
    ReadRefusal::no_such_member(format!("{what} {i} is outside 0..{len}"))
}

impl ExternalIntrospect for PropertyGridExternal {
    fn schema(&self) -> IntrospectSchema {
        // R921 — the *visible-row structure* (the flatten + the roving cursor)
        // is read off the sibling `TREE_TAG` `TreeViewIntrospect` (`row_count` /
        // `cursor` / `id_at.<pos>` / `level_at.<pos>` / `expanded_at.<pos>`).
        // This coordinator owns the value-index-keyed value model + the
        // per-branch collapse / struct aggregate / edit / popup state.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("row_count", "int"),
                    SchemaField::new("editing", "json"),
                    // (R1353.1) `index` is an ADDRESS, not a row index: `row_ref`
                    // parses either a scalar row (`3`) or an element address
                    // (`elem.2`) — a small grammar with two shapes. `row_count`
                    // bounds only the first, so `IndexOf("row_count")` would deny
                    // every `elem.<k>` address. Declared unknown rather than
                    // half-true; `ArgDomain` cannot express a sum of two domains.
                    SchemaField::parametric(
                        "name.<index>",
                        "string",
                        const { &[SchemaArg::open("index", "int")] },
                    ),
                    SchemaField::parametric(
                        "kind.<index>",
                        "string",
                        const { &[SchemaArg::open("index", "int")] },
                    ),
                    SchemaField::parametric(
                        "value.<index>",
                        "json",
                        const { &[SchemaArg::open("index", "int")] },
                    ),
                    // R964 — a bounded scalar's `"<lo>..<hi>"` interval ("none" when
                    // unranged); the AI reads it before a `value.<i>` write. Same wire
                    // as the data-grid R894 `col_range.<col>` sibling.
                    SchemaField::parametric(
                        "range.<index>",
                        "string",
                        const { &[SchemaArg::open("index", "int")] },
                    ),
                    // R919 / R936 — the modified-from-default reads + the reset writes.
                    // `modified.<addr>` takes a scalar index ("6") OR an element address
                    // ("elem.2"), the unified `ValueRef` vocabulary; `reset` likewise.
                    SchemaField::parametric(
                        "modified.<addr>",
                        "bool",
                        const { &[SchemaArg::open("addr", "string")] },
                    ),
                    SchemaField::new("any_modified", "bool"),
                    SchemaField::action("reset", "int"),
                    SchemaField::action("reset_all", "json"),
                    // R931 — the dynamic array: element count + the add / remove /
                    // reorder verbs. Element values read / write through the same
                    // `value.elem.<k>` / `kind.elem.<k>` / `name.elem.<k>` paths as a
                    // scalar (the unified `ValueRef` wire address).
                    SchemaField::new("elem_count", "int"),
                    SchemaField::action("add_elem", "int"),
                    SchemaField::action("remove_elem", "int"),
                    SchemaField::action("move_elem", "string"),
                    // R936 — the array branch's modified roll-up (length or any element
                    // differs) + its wholesale reset, the array peer of
                    // `struct_modified.<id>` / `reset_struct`.
                    SchemaField::parametric(
                        "array_modified.<branch_id>",
                        "bool",
                        const { &[SchemaArg::open("branch_id", "string")] },
                    ),
                    SchemaField::action("reset_array", "bool"),
                    // R921 — per-branch collapse (read + intervene + toggle) and the
                    // struct aggregate (summary tuple + modified roll-up + reset-all).
                    SchemaField::parametric(
                        "expanded.<branch_id>",
                        "bool",
                        const { &[SchemaArg::open("branch_id", "string")] },
                    ),
                    SchemaField::parametric(
                        "struct_summary.<struct_id>",
                        "string",
                        const { &[SchemaArg::open("struct_id", "string")] },
                    ),
                    SchemaField::parametric(
                        "struct_modified.<struct_id>",
                        "bool",
                        const { &[SchemaArg::open("struct_id", "string")] },
                    ),
                    SchemaField::action("toggle_branch", "bool"),
                    SchemaField::action("reset_struct", "int"),
                    // R921 — the roving keyboard cursor's node id (read + intervene; a
                    // leaf value-index string "6" or a branch `cat.` / `struct.` id,
                    // Null when unset). The AI-first cursor move (no click side effect).
                    SchemaField::new("cursor", "string"),
                    SchemaField::new("popup_cursor", "int"),
                    // R875 — live numeric-scrub flag (true between the first drag move
                    // and the release); the AI-first witness of a scrub in flight.
                    SchemaField::new("scrubbing", "bool"),
                    SchemaField::send("string"),
                    SchemaField::action("toggle", "int"),
                    SchemaField::action("begin", "int"),
                    SchemaField::action("choose", "int"),
                    SchemaField::action("pick_color", "int"),
                    SchemaField::action("close_popup", "json"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        match path {
            "row_count" => Ok(IntrospectValue::Int(
                i64::try_from(self.count()).expect("row count fits in i64"),
            )),
            // R921.1 — gated on `editing_if_visible`: an edit whose row is
            // collapsed / filtered out of the flatten reports Null (it paints
            // nowhere, so advertising it would be an R901.1 introspection lie).
            "editing" => Ok(IntrospectValue::Json(match self.editing_if_visible() {
                // R931 — the editing leaf's **node id** (a scalar "6" or an
                // `elem.2`), the same addressing vocabulary `cursor` uses, so a
                // scalar and an array element are reported uniformly.
                Some(value_ref) => serde_json::Value::from(ref_node_id(value_ref)),
                None => serde_json::Value::Null,
            })),
            // R921.1 — Null unless the open popup's row is visible (same gate):
            // a popup hidden by a collapse / filter paints no panel, so its
            // cursor is not reported either.
            "popup_cursor" => Ok(
                match self.editing_if_visible().and(self.popup_cursor.get()) {
                    Some(i) => {
                        IntrospectValue::Int(i64::try_from(i).expect("cursor index fits in i64"))
                    }
                    None => IntrospectValue::Null,
                },
            ),
            "scrubbing" => Ok(IntrospectValue::Bool(self.is_scrubbing())),
            // R921 — the roving cursor's node id (Null when unset).
            "cursor" => Ok(self
                .cursor
                .get()
                .map_or(IntrospectValue::Null, IntrospectValue::Text)),
            // R919 — any property modified from its default (the dirty read).
            "any_modified" => Ok(IntrospectValue::Bool(self.any_modified())),
            // R931 — the array element count (the dynamic-collection length).
            "elem_count" => Ok(IntrospectValue::Int(
                i64::try_from(self.array_len()).expect("element count fits in i64"),
            )),
            _ => self.query_addressed(path),
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
                // R931 — `value.<addr>` writes a scalar leaf (a bare decimal) or
                // an array element (`elem.<k>`), the unified `ValueRef` address.
                let Some(addr) = path.strip_prefix("value.") else {
                    return Err(InterveneError::UnknownPath);
                };
                let Some(value_ref) = row_ref(addr) else {
                    return Err(InterveneError::UnknownPath);
                };
                let Some(current) = self.value_at(value_ref) else {
                    return Err(InterveneError::UnknownPath);
                };
                // `with_intervene` (not `kind().coerce`) so a choice cell sets
                // its option by index while preserving its option list; an array
                // element (`Float`) coerces a numeric write through the same path.
                let new_value = current.with_intervene(value)?;
                self.set_value_at(value_ref, new_value);
                Ok(())
            }
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
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
                    let row = usize::try_from(i).map_err(|_| {
                        InvokeError::rejected(format!("{path}: {i} is not a row index"))
                    })?;
                    Ok(IntrospectValue::Bool(self.toggle(row)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R919 / R936 — reset the leaf at a given address to its class
            // default (the RPC twin of clicking its reset arrow). `false` when the
            // target is already default / out of range. An `Int` names a scalar
            // source row; a `Text` node-id resets a scalar ("6") OR an array
            // element ("elem.2"), the unified `ValueRef` address the click /
            // keyboard reset also route through (one funnel, two addressings).
            "reset" => match args {
                IntrospectValue::Int(i) => {
                    let row = usize::try_from(i).map_err(|_| {
                        InvokeError::rejected(format!("{path}: {i} is not a row index"))
                    })?;
                    Ok(IntrospectValue::Bool(self.reset_to_default(row)))
                }
                IntrospectValue::Text(ref id) => match row_ref(id) {
                    Some(ValueRef::Scalar(i)) => {
                        Ok(IntrospectValue::Bool(self.reset_to_default(i)))
                    }
                    Some(ValueRef::Elem(k)) => Ok(IntrospectValue::Bool(self.reset_element(k))),
                    None => Err(InvokeError::rejected(format!(
                        "{path}: {id:?} is not a row address"
                    ))),
                },
                _ => Err(InvokeError::TypeMismatch),
            },
            // R919 — reset every modified property; returns the count reset.
            "reset_all" => Ok(IntrospectValue::Int(
                i64::try_from(self.reset_all()).expect("reset count fits in i64"),
            )),
            // R936 — reset the whole array branch to its class default (length +
            // content); returns whether it changed (the RPC twin of the array
            // branch's reset arrow, the array peer of `reset_struct`).
            "reset_array" => Ok(IntrospectValue::Bool(self.reset_array())),
            // R931 — append a new array element; returns its index (the RPC twin
            // of clicking the array branch's "+" button).
            "add_elem" => Ok(IntrospectValue::Int(
                i64::try_from(self.add_elem()).expect("element index fits in i64"),
            )),
            // R931 — remove the array element at the given index; returns whether
            // it existed (the RPC twin of an element's "−" button, with the
            // R930.1 latch / cursor invalidation).
            "remove_elem" => match args {
                IntrospectValue::Int(i) => {
                    let k = usize::try_from(i).map_err(|_| {
                        InvokeError::rejected(format!("{path}: {i} is not an element index"))
                    })?;
                    Ok(IntrospectValue::Bool(self.remove_elem(k)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R931 — reorder: move the element at `from` to position `to`,
            // passed as a `"from,to"` payload (the AI-first / keyboard reorder
            // primary path; a live drag-to-reorder gesture is deferred). Returns
            // whether it moved.
            "move_elem" => match args {
                IntrospectValue::Text(ref s) => {
                    let (from, to) = parse_move_pair(s).ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "{path}: malformed argument {s:?} (expected \"<from>,<to>\")"
                        ))
                    })?;
                    Ok(IntrospectValue::Bool(self.move_elem(from, to)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
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
                    let row = usize::try_from(i).map_err(|_| {
                        InvokeError::rejected(format!("{path}: {i} is not a row index"))
                    })?;
                    if row >= self.count() {
                        return Err(InvokeError::rejected(format!(
                            "{path}: no row {row} in this grid (it has {})",
                            self.count()
                        )));
                    }
                    Ok(IntrospectValue::Bool(
                        self.begin_edit(ValueRef::Scalar(row)),
                    ))
                }
                // R931 — a node-id payload begins editing any leaf (a scalar "6"
                // or an array element "elem.2"), the unified `ValueRef` address
                // the keyboard element-edit path uses.
                IntrospectValue::Text(ref id) => {
                    let value_ref = row_ref(id).ok_or_else(|| {
                        InvokeError::rejected(format!("{path}: {id:?} is not a row address"))
                    })?;
                    Ok(IntrospectValue::Bool(self.begin_edit(value_ref)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // The three popup verbs, split out (SRP + the workspace line
            // ceiling, which R1564's reasons pushed this dispatch over) — the
            // same lift `dispatch_send` already is.
            "choose" | "pick_color" | "close_popup" => self.dispatch_popup(path, &args),
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// ─── inline-editor commit / cancel (keyboard, owner-scoped) ───────

/// Commit the in-flight edit: parse the editor text by the editing row's
/// kind and write it back to the model. A malformed numeric commit keeps the
/// prior value (no data loss). Mirrors `todomvc::commit_edit`.
fn commit_edit(restore_focus: bool) {
    let Some(row) = use_editing_row().get() else {
        return;
    };
    let text = use_text_edit_state(EDIT_TF_TAG).text();
    // R931 — parse by the editing leaf's kind (scalar flat model OR array
    // sub-model) and write through the model that backs it. A malformed numeric
    // commit keeps the prior value (no data loss).
    if let Some(value) = ref_value(row) {
        if let Some(parsed) = value.kind().parse(&text) {
            set_ref_value(row, parsed);
        }
    }
    end_edit_mode(restore_focus);
}

/// R931 — the value behind a [`ValueRef`], read from whichever model backs it
/// (the Owner-scoped peer of [`PropertyGridExternal::value_at`], for the
/// keyboard-side free fns). A `Scalar` reads the flat value model; an `Elem`
/// reads the array sub-model.
fn ref_value(row: ValueRef) -> Option<CellValue> {
    match row {
        ValueRef::Scalar(i) => use_property_model().get().get(i).cloned(),
        ValueRef::Elem(k) => use_array_model().get().get(k).cloned(),
    }
}

/// R931 — write the value behind a [`ValueRef`] into its backing model (the
/// Owner-scoped peer of [`PropertyGridExternal::set_value_at`]).
fn set_ref_value(row: ValueRef, value: CellValue) {
    let (signal, slot) = match row {
        ValueRef::Scalar(i) => (use_property_model(), i),
        ValueRef::Elem(k) => (use_array_model(), k),
    };
    signal.set_with(move |prev| {
        let mut next = prev.clone();
        if let Some(cell) = next.get_mut(slot) {
            *cell = value.clone();
        }
        next
    });
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
    ref_value(use_editing_row().get()?).map(|v| v.kind())
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
    // R931 — a scalar leaf and an array element leaf activate differently: a
    // scalar's Delete resets to default; an element's Delete *removes* it (the
    // keyboard grow/shrink twin of the +/− buttons, which are not tab stops).
    if let Some(id) = cursor_id.as_deref() {
        match row_ref(id) {
            Some(ValueRef::Scalar(vi)) => match key {
                "Space" => return activate_source(scene, vi, false),
                "Enter" | "F2" => return activate_source(scene, vi, true),
                "Delete" => return reset_source(scene, vi),
                _ => {}
            },
            Some(ValueRef::Elem(k)) => match key {
                "Enter" | "F2" => return begin_edit_id(scene, id),
                "Delete" => return remove_elem_kbd(scene, k),
                _ => {}
            },
            None => {}
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
    // R1176 — an asset-path leaf opens the embedded file picker on activation
    // (Space / Enter / click), the Choice / Colour affordance shape rather than
    // the text editor (`allow_edit` does not gate it).
    if is_asset_slot(row) {
        return intro.invoke("begin", arg).is_ok();
    }
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
    let _ = intro.invoke(
        "reset",
        IntrospectValue::Int(i64::try_from(row).expect("row fits in i64")),
    );
    true
}

/// R931 — begin editing the leaf with node `id` (a scalar "6" or an element
/// "elem.2") via the coordinator's `begin` invoke (the keyboard `Enter` / `F2`
/// path for an array element, which routes through the same wire the scalar
/// path does — one funnel). Consumes the key.
fn begin_edit_id(scene: &mut Scene, id: &str) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(GRID_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    let _ = intro.invoke("begin", IntrospectValue::Text(id.to_owned()));
    true
}

/// R931 — remove the array element at `k` via the coordinator's `remove_elem`
/// invoke (the keyboard `Delete`-on-an-element twin of the "−" button, which is
/// not a tab stop). Consumes the key on any element row.
fn remove_elem_kbd(scene: &mut Scene, k: usize) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(GRID_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    let _ = intro.invoke(
        "remove_elem",
        IntrospectValue::Int(i64::try_from(k).expect("k fits in i64")),
    );
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

/// Cell-sized M3 checkbox-box style. The bool value cell renders the lifted
/// `view_checkbox_box` SSOT non-interactively (the grid coordinator owns the
/// toggle, so there is no per-cell `CheckboxExternal`) — keeping one M3
/// checkbox rendering across the catalog instead of a hand-rolled copy.
fn cell_checkbox_style() -> CheckboxStyle {
    CheckboxStyle {
        box_size: CHECKBOX_SIZE,
        glyph_size_px: 16,
        ..CheckboxStyle::m3_filled()
    }
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
    trailing: Vec<Scene>,
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
        ContainerNode::new(name_children).with_layout(
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
            // R964 — a ranged Float leaf renders the bounded-slider gauge; every
            // other scalar (unranged Float / Int / Text) keeps the plain value
            // text. The range is keyed by the row's leaf slot, so an array
            // element (id `elem.<k>` → no slot) never gauges.
            other => match (
                other,
                row_value_index(&row.id).and_then(|i| scalar_range(i).map(|(lo, hi)| (i, lo, hi))),
            ) {
                (CellValue::Float(f), Some((slot, lo, hi))) => {
                    ranged_slider_cell(slot, *f, lo, hi, theme)
                }
                _ => Scene::Text(TextNode::styled(
                    other.display(),
                    Rect::default(),
                    TextStyle::new()
                        .with_size_px(CELL_PX)
                        .with_fg(theme.resolve(ColorRole::OnSurface)),
                )),
            },
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

    // R919 / R931 / R936 — the row's trailing affordances at its trailing edge:
    // a scalar's reset arrow (`{GRID_TAG}#reset<index>`, whose presence IS the
    // modified indicator), or an array element's remove button PLUS — when the
    // element is modified — its reset arrow one slot to the left. Each is
    // absolutely positioned (at its own [`RESET_DOT_X`] / [`RESET_DOT_X2`] inset),
    // so they overlay the trailing padding without shifting the Property / Value
    // column layout, and come last in paint order so a click wins over the row.
    let mut children = vec![name_cell, value_cell];
    children.extend(trailing);
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("{GRID_TAG}#{}", row.id))
            .with_style(BoxStyle::filled(focus_fill(theme, is_focused)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(NAME_COL_W + VALUE_COL_W, ROW_H)),
            ),
    )
}

/// R919 / R921 / R936 — the reset arrow for a modified row, a small accent mark
/// tagged `{GRID_TAG}#reset<suffix>` so a click routes to the coordinator's reset
/// funnel (`suffix` = a scalar value index "6", a struct branch id
/// `struct.Position`, an element address `elem.2`, or the array branch id
/// `arr.weights`). `x_inset` is the inset from the row's trailing edge:
/// [`RESET_DOT_X`] (the edge) for a scalar / struct row whose only trailing mark
/// is the arrow, or [`RESET_DOT_X2`] (one slot left) for an element / array-branch
/// row whose remove / add button already owns the edge. Painted only for a
/// modified row, so its presence doubles as the modified indicator; its a11y
/// `button` peer is gated on the same predicate (R886.1 one-gate).
fn reset_arrow(suffix: &str, x_inset: u32, theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(format!("{GRID_TAG}#{RESET_PREFIX}{suffix}"))
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Accent)))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(
                        (NAME_COL_W + VALUE_COL_W).saturating_sub(x_inset),
                        ROW_H.saturating_sub(RESET_DOT) / 2,
                    )
                    .with_size(Size::px(RESET_DOT, RESET_DOT)),
            ),
    )
}

/// R931 — an array element's "remove" button at its trailing edge, a clickable
/// minus glyph tagged `{GRID_TAG}#rmelem<k>` so a click routes to the
/// coordinator's [`remove_elem`](PropertyGridExternal::remove_elem). Absolutely
/// positioned in the trailing padding (the element row's reset-arrow slot is
/// free — an element has no reset), last in paint order so its click wins.
fn remove_button(k: usize, theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            REMOVE_GLYPH,
            Rect::default(),
            TextStyle::new()
                .with_size_px(CELL_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(format!("{GRID_TAG}#{RM_ELEM_PREFIX}{k}"))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                // The button's RIGHT edge stays where the 10px dot's was, so
                // growing the box to hold its glyph moves nothing visible and
                // cannot push the button past the row that owns it.
                .with_absolute_position(
                    (NAME_COL_W + VALUE_COL_W + RESET_DOT)
                        .saturating_sub(RESET_DOT_X + glyph_button_side()),
                    ROW_H.saturating_sub(glyph_button_side()) / 2,
                )
                .with_size(Size::px(glyph_button_side(), glyph_button_side())),
        ),
    )
}

/// R931 — the array branch's "add element" button at its trailing edge, a
/// clickable plus glyph tagged `{GRID_TAG}#addelem` routing to
/// [`add_elem`](PropertyGridExternal::add_elem). Same trailing-edge geometry as
/// the element [`remove_button`].
fn add_button(theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            ADD_GLYPH,
            Rect::default(),
            TextStyle::new()
                .with_size_px(CELL_PX)
                .with_fg(theme.resolve(ColorRole::Accent)),
        ))])
        .with_tag(format!("{GRID_TAG}#{ADD_ELEM_TAG}"))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                // The button's RIGHT edge stays where the 10px dot's was, so
                // growing the box to hold its glyph moves nothing visible and
                // cannot push the button past the row that owns it.
                .with_absolute_position(
                    (NAME_COL_W + VALUE_COL_W + RESET_DOT)
                        .saturating_sub(RESET_DOT_X + glyph_button_side()),
                    ROW_H.saturating_sub(glyph_button_side()) / 2,
                )
                .with_size(Size::px(glyph_button_side(), glyph_button_side())),
        ),
    )
}

/// R931 / R936 — the **array** branch header row: `[ ⟨disclosure⟩ Spawn Weights |
/// N elements  ⟨reset?⟩ (+) ]`, tagged `{GRID_TAG}#arr.weights` so a click toggles
/// its collapse, with the element-count summary in the value column, a trailing
/// "add element" button, and — when `modified` (length or any element differs) —
/// a reset arrow one slot to the left of the add button. The dynamic-collection
/// peer of [`struct_header_row`], now with the same modified roll-up + reset
/// affordance as the struct row.
fn array_header_row(
    row: &VisibleRow,
    count: usize,
    modified: bool,
    is_focused: bool,
    theme: &Theme,
) -> Scene {
    let label = row.label.as_str();
    let glyph = if row.expanded {
        DISCLOSURE_EXPANDED
    } else {
        DISCLOSURE_COLLAPSED
    };
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
        TextStyle::new()
            .with_size_px(CELL_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    )));
    let name_cell = Scene::Container(
        ContainerNode::new(name_children).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_padding(Rect::new(CELL_PAD, 0, CELL_PAD, 0))
                .with_size(Size::px(NAME_COL_W, ROW_H)),
        ),
    );
    let summary = if count == 1 {
        "1 element".to_owned()
    } else {
        format!("{count} elements")
    };
    let value_cell = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            summary,
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
                .with_size(Size::px(VALUE_COL_W, ROW_H)),
        ),
    );
    let mut children = vec![name_cell, value_cell, add_button(theme)];
    if modified {
        children.push(reset_arrow(&row.id, RESET_DOT_X2, theme));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("{GRID_TAG}#{}", row.id))
            .with_style(BoxStyle::filled(focus_fill(theme, is_focused)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(NAME_COL_W + VALUE_COL_W, ROW_H)),
            ),
    )
}

/// R921 — a **struct** branch header row: `[ ⟨disclosure⟩ name | (x, y, z) summary ]`, tagged `{GRID_TAG}#<id>` (the `row`'s node id)
/// so a click toggles its collapse. Indented by the row's depth, with the
/// disclosure glyph reflecting its expanded flag, the collapsed-value `summary` in
/// the value column, and — when `modified` — the reset arrow (resetting every field).
/// The engine / the toolkit Details struct row.
fn struct_header_row(
    row: &VisibleRow,
    summary: &str,
    modified: bool,
    is_focused: bool,
    theme: &Theme,
) -> Scene {
    let struct_id = row.id.as_str();
    let label = row.label.as_str();
    let glyph = if row.expanded {
        DISCLOSURE_EXPANDED
    } else {
        DISCLOSURE_COLLAPSED
    };
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
        TextStyle::new()
            .with_size_px(CELL_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    )));
    let name_cell = Scene::Container(
        ContainerNode::new(name_children).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_padding(Rect::new(CELL_PAD, 0, CELL_PAD, 0))
                .with_size(Size::px(NAME_COL_W, ROW_H)),
        ),
    );
    let value_cell = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            summary,
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
                .with_size(Size::px(VALUE_COL_W, ROW_H)),
        ),
    );
    let mut children = vec![name_cell, value_cell];
    if modified {
        children.push(reset_arrow(struct_id, RESET_DOT_X, theme));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("{GRID_TAG}#{struct_id}"))
            .with_style(BoxStyle::filled(focus_fill(theme, is_focused)))
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
        TextStyle::new()
            .with_size_px(CELL_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let chevron = Scene::Text(TextNode::styled(
        CHOICE_CHEVRON,
        Rect::default(),
        TextStyle::new()
            .with_size_px(CELL_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
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
    let swatch = Scene::Container(
        ContainerNode::new(vec![])
            .with_style(
                BoxStyle::filled(color)
                    .with_corner_radius(4)
                    .with_border(Border::new(theme.resolve(ColorRole::Outline), 1)),
            )
            .with_layout(LayoutStyle::new().with_size(Size::px(CELL_PX + 6, CELL_PX + 6))),
    );
    let hex = Scene::Text(TextNode::styled(
        color.to_hex(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(CELL_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
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
                    view_swatch(
                        i,
                        color,
                        color == current,
                        cursor == i,
                        hover == Some(i),
                        theme,
                    )
                })
                .collect();
            Scene::Container(
                ContainerNode::new(cells).with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_gap(SWATCH_GAP),
                ),
            )
        })
        .collect();
    // The hex-entry field below the palette — the arbitrary-colour path.
    let field_style = tf_paint::TextFieldStyle {
        field_w: inner_w,
        field_h: HEX_FIELD_H,
        ..tf_paint::TextFieldStyle::m3_filled()
    };
    children.push(tf_paint::view_field(
        EDIT_TF_TAG,
        edit_field.0,
        edit_field.1,
        theme,
        &field_style,
        "",
    ));
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
/// **visual position** `view_pos`: anchored at the value column, its `y`
/// resolved by the shared [`flip_y`] positioner (drop below the row, flip above
/// on bottom overflow). This fn owns only the grid-specific row → anchor `y`
/// arithmetic: deterministic because every row (the column header, the category
/// headers, and the data rows) shares the uniform [`ROW_H`] + [`ROW_GAP`] pitch
/// and the title band has a fixed height ([`TITLE_H`]). The leading `row_step`
/// skips the `Property` / `Value` column-header row above the flatten. Shared
/// by the choice + colour popups.
fn popup_origin(view_pos: usize, panel_h: u32) -> (u32, u32) {
    let x = PANEL_PAD + GRID_BORDER + NAME_COL_W;
    let row_step = ROW_H + ROW_GAP;
    let grid_top = PANEL_PAD + TITLE_H + TITLE_GAP + GRID_BORDER;
    // ★★ R1673 — MINUS what the grid is scrolled by, because the grid scrolls
    // now and this popup does not: it is a sibling of the scroll region, in
    // window coordinates, while the row it points at moves under it.
    //
    // Written the moment the scroll landed rather than left for a person to
    // find, because this is the shape this project keeps paying for — one fact
    // (where is row N) with two derivations, the scene's and this one's. The
    // scene's is authoritative and folds the offset (`NodeVisit::offset`); this
    // arithmetic is the copy, so the copy is the one that has to be corrected.
    let scrolled_by = use_scroll_state(GRID_SCROLL_KEY).offset_y().unsigned_abs();
    let row_top =
        (grid_top + row_step + u32::try_from(view_pos).expect("row fits in u32") * row_step)
            .saturating_sub(scrolled_by);
    // Drop below the row, flipping above when it overflows the padded content
    // bottom — the shared R1378 [`anchor::flip_y`] positioner.
    let y = flip_y(
        row_top,
        ROW_H,
        panel_h,
        WIN_H - PANEL_PAD,
        AnchorSide::Below,
    );
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
        ContainerNode::new(vec![
            cell("Property", NAME_COL_W),
            cell("Value", VALUE_COL_W),
        ])
        .with_style(BoxStyle::filled(
            theme.resolve(ColorRole::SurfaceContainerHighest),
        ))
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
fn popup_view_pos(editing: Option<ValueRef>) -> Option<(usize, usize)> {
    // R931 — a popup is a Choice / Colour editor, which only opens on a scalar
    // leaf; an `Elem` edit is an inline text field with no popup, so it resolves
    // to `None` here (no panel painted / announced). The first tuple element is
    // the scalar flat-model index every popup consumer indexes by.
    let ValueRef::Scalar(row) = editing? else {
        return None;
    };
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
    let Some((row, _)) = popup_view_pos(use_editing_row().get()) else {
        return Vec::new();
    };
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
            listbox_option_nodes(CHOICE_POPUP_TAG, &name, false, &opts)
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
            listbox_option_nodes(COLOR_POPUP_TAG, &name, false, &opts)
        }
        _ => Vec::new(),
    }
}

fn view_popup_overlay(
    editing: Option<ValueRef>,
    model: &[CellValue],
    edit_field: (TextFieldState, u32),
    theme: &Theme,
) -> Vec<Scene> {
    let Some((row, view_pos)) = popup_view_pos(editing) else {
        return Vec::new();
    };
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

/// R1176 — action-button width / height in the asset dialog.
const ASSET_ACTION_W: u32 = 88;
const ASSET_ACTION_H: u32 = 34;

/// R1176 — the asset picker's modal panel style: the M3 default widened to hold
/// the file browser.
fn asset_dialog_style() -> DialogStyle {
    DialogStyle {
        panel_width: ASSET_PANEL_W,
        ..DialogStyle::m3_default()
    }
}

/// R1176/R1177 — one dialog action button, painted with the **live** posture
/// (`state` + keyboard `focused`) the caller read from the scene via `read_state`
/// — so the painted hover / press / focus ring matches the a11y tree's reported
/// posture (one source, no paint↔a11y divergence). The OK selection gate is
/// applied by the caller (passing `ButtonState::Disabled`).
fn asset_action_button(
    tag: &'static str,
    label: &str,
    state: ButtonState,
    colors: &ButtonColors,
) -> Scene {
    button_scene(
        label,
        state,
        tag,
        colors,
        &ButtonStyle::m3_default(tag)
            .with_size(Size::px(ASSET_ACTION_W, ASSET_ACTION_H))
            .with_label_font_size_px(15),
    )
}

/// R1176 — the embedded asset file picker overlay: empty when closed, else the
/// R788 modal dialog (scrim + centred panel) hosting the lifted
/// [`file_browser_pane`] in its [`DialogContent::body`] slot, with Cancel / Open
/// action buttons. The OK button is gated disabled until a file is selected
/// (read reactively from the shared [`DirectoryState`]). The property grid is
/// the Nth consumer of each substrate (modal / directory / dialog chrome /
/// browser pane), the 1st to embed the picker in a host widget.
fn view_asset_dialog(theme: &Theme, buttons: AssetButtons) -> Vec<Scene> {
    if !asset_modal().is_open() {
        return Vec::new();
    }
    let dir = asset_directory();
    let scroll = use_scroll_state(ASSET_SCROLL_KEY);
    let has_selection = dir.selected().is_some();
    let (ok_state, cancel_state) = buttons;
    let pane = file_browser_pane(
        ASSET_DIR_TAG,
        &dir,
        &scroll,
        theme,
        FileBrowserMetrics {
            list_width: ASSET_LIST_W,
            list_height: ASSET_LIST_H,
            row_pitch: ASSET_ROW_PITCH,
            overscan: ASSET_OVERSCAN,
            focusable: true,
        },
        None,
    );
    // Action order matches `asset_dialog_members()` (Cancel, then Open).
    let cancel = asset_action_button(
        ASSET_CANCEL_TAG,
        "Cancel",
        cancel_state,
        &ButtonColors::filled_tonal(theme),
    );
    // OK gated disabled until a file is selected (the gate overrides the read
    // posture; same gate the a11y tree applies, so paint + a11y agree).
    let ok_posture = if has_selection {
        ok_state
    } else {
        ButtonState::Disabled
    };
    let ok = asset_action_button(
        ASSET_OK_TAG,
        "Open",
        ok_posture,
        &ButtonColors::accent(theme),
    );
    vec![view_dialog(
        ASSET_SCRIM_TAG,
        ASSET_PANEL_TAG,
        DialogContent {
            title: "Pick asset",
            message: "",
            body: Some(pane),
        },
        vec![cancel, ok],
        (WIN_W, WIN_H),
        theme,
        &asset_dialog_style(),
    )]
}

/// R1176 — the asset picker's modal dialog a11y subtree when open (empty when
/// closed): an aria-modal `Dialog` owning the file `list` (the lifted
/// windowed-list SSOT) + the two action buttons, OK aria-disabled until a file
/// is selected. A free helper so `access_node` stays under the line cap.
///
/// R1517 — `focused` is the caller's (`access_node`'s) shell-focus argument, the
/// same source the search box's node reads two dozen lines below it. Before this
/// round these two buttons were the workspace's only AT nodes whose focus flag
/// came from a paint-cadence snapshot instead.
fn asset_dialog_access_nodes(buttons: AssetButtons, focused: Option<&str>) -> Vec<AccessNode> {
    if !asset_modal().is_open() {
        return Vec::new();
    }
    let dir = asset_directory();
    let scroll = use_scroll_state(ASSET_SCROLL_KEY);
    let count = dir.entries().len();
    let window = compute_visible_range(
        scroll.offset_y(),
        ASSET_LIST_H,
        count,
        ASSET_ROW_PITCH,
        ASSET_OVERSCAN,
    );
    // R1177 — the SAME live postures the paint reads (`read_state`), with the
    // SAME OK selection gate, so the AT tree never advertises a posture the
    // painted button contradicts.
    let (ok_state, cancel_state) = buttons;
    let (ok_focused, cancel_focused) = (
        focused == Some(ASSET_OK_TAG),
        focused == Some(ASSET_CANCEL_TAG),
    );
    let mut nodes = vec![
        AccessNode::new(ASSET_PANEL_TAG, AriaRole::Dialog)
            .with_modal()
            .with_child(ASSET_DIR_TAG)
            .with_child(ASSET_CANCEL_TAG)
            .with_child(ASSET_OK_TAG),
    ];
    nodes.extend(windowed_list_nodes_selected(
        ASSET_DIR_TAG,
        "Assets",
        u32::try_from(count).unwrap_or(u32::MAX),
        &window,
        dir.selected_index(),
    ));
    nodes.push(
        AccessNode::new(ASSET_CANCEL_TAG, AriaRole::Button)
            .with_state(button_a11y_state(cancel_state, cancel_focused))
            .with_position_in_set(1)
            .with_size_of_set(2),
    );
    let ok_posture = if dir.selected().is_some() {
        ok_state
    } else {
        ButtonState::Disabled
    };
    nodes.push(
        AccessNode::new(ASSET_OK_TAG, AriaRole::Button)
            .with_state(button_a11y_state(ok_posture, ok_focused))
            .with_position_in_set(2)
            .with_size_of_set(2),
    );
    nodes
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
        TextStyle::new()
            .with_size_px(TITLE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let search_style = tf_paint::TextFieldStyle {
        field_w: SEARCH_W,
        field_h: TITLE_H - 4,
        ..tf_paint::TextFieldStyle::m3_filled()
    };
    let search_field = tf_paint::view_field(
        SEARCH_TF_TAG,
        search_state,
        search_caret,
        theme,
        &search_style,
        "Filter",
    );
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

// R1026 — rustfmt's reflow pushed this example view past too_many_lines (100).
#[allow(clippy::trivially_copy_pass_by_ref, clippy::too_many_lines)]
fn view(state: RootState, _frame: &Frame) -> Scene {
    let ((edit_state, edit_caret), (search_state, search_caret), asset_buttons) = state;
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
    // R931 — the array sub-model snapshot, for the element rows.
    let array = use_array_model().get();
    // R936 — the frozen array baseline, to paint a reset arrow on a modified
    // element / array branch (the element peer of `defaults`).
    let array_defaults = use_array_defaults();
    let visible = visible_property_rows(&tree_nodes, &current_search_query());
    let cursor_id = use_property_cursor().get();
    let editing = use_editing_row().get();

    let title = view_title(search_state, search_caret, &theme);

    let mut rows: Vec<Scene> = Vec::with_capacity(visible.len() + 1);
    rows.push(view_header(&theme));
    for vr in &visible {
        let is_focused = cursor_id.as_deref() == Some(vr.id.as_str());
        if let Some(vi) = row_value_index(&vr.id) {
            // A scalar leaf (editable) row — trailing reset arrow iff modified.
            let value = &model[vi];
            let edit_active =
                editing == Some(ValueRef::Scalar(vi)) && value.kind().is_text_editable();
            let trailing = leaf_modified(&model, &defaults, vi)
                .then(|| reset_arrow(&vr.id, RESET_DOT_X, &theme))
                .into_iter()
                .collect();
            rows.push(view_row(
                vr,
                value,
                is_focused,
                edit_active,
                trailing,
                &theme,
                (edit_state, edit_caret),
            ));
        } else if let Some(ValueRef::Elem(k)) = row_ref(&vr.id) {
            // R931 / R936 — an array element leaf: render the element value (from
            // the array sub-model) through the *same* `view_row` as a scalar
            // `Float`, with a trailing remove button and — when the element
            // differs from its class default — a reset arrow one slot to its left
            // (the element peer of a scalar leaf's reset arrow, R936 clearing the
            // R931 "no per-element modified arrow" deferral).
            let value = array.get(k).cloned().unwrap_or(CellValue::Float(0.0));
            let edit_active = editing == Some(ValueRef::Elem(k)) && value.kind().is_text_editable();
            let mut trailing = vec![remove_button(k, &theme)];
            if leaf_modified(&array, &array_defaults, k) {
                trailing.push(reset_arrow(&vr.id, RESET_DOT_X2, &theme));
            }
            rows.push(view_row(
                vr,
                &value,
                is_focused,
                edit_active,
                trailing,
                &theme,
                (edit_state, edit_caret),
            ));
        } else if vr.id.starts_with(STRUCT_PREFIX) {
            // A struct branch row: disclosure + name + the collapsed-value tuple.
            let summary = struct_value_summary(&model, &tree_nodes, &vr.id);
            let modified = struct_is_modified(&model, &defaults, &tree_nodes, &vr.id);
            rows.push(struct_header_row(
                vr, &summary, modified, is_focused, &theme,
            ));
        } else if vr.id.starts_with(ARR_PREFIX) {
            // R931 / R936 — the array branch row: disclosure + name + element
            // count, with a trailing "add element" button (the grow verb) and a
            // reset arrow when the list differs from its class default (length or
            // any element) — the array peer of the struct row's modified roll-up.
            let modified = array_is_modified(&array, &array_defaults);
            rows.push(array_header_row(
                vr,
                array.len(),
                modified,
                is_focused,
                &theme,
            ));
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
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::Surface))
                    .with_border(Border::new(theme.resolve(ColorRole::Outline), GRID_BORDER)),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_gap(ROW_GAP)
                    // ★★ R1673 — the frame's own pixels are RESERVED, so a row
                    // laid at the full column width no longer covers the
                    // outline. Measured: 29 of this screen's 35 escapes were
                    // exactly this, one pixel on three edges, every row.
                    // Padding is the right instrument here and not for the
                    // stepper buttons below, because these rows are laid out by
                    // the FLOW and an absolutely-placed child ignores padding.
                    .with_padding(Rect::new(
                        GRID_BORDER,
                        GRID_BORDER,
                        GRID_BORDER,
                        GRID_BORDER,
                    ))
                    .with_focusable(true),
            ),
    );

    // A choice / colour popup floats over the grid (absolutely positioned)
    // with a full-window light-dismiss barrier beneath it — the barrier is
    // pushed first so the panel hit-tests on top; a click outside the panel
    // routes `dismiss` to the coordinator (the toggle-close convention).
    // ★★ R1673 — the grid SCROLLS, because it is taller than the window that
    // holds it and until now it simply ran off the bottom.
    //
    // Measured before this: the grid is 1,160px in an 820px window, so 402px of
    // it were painted where nobody can see them and `scene/pointer_reach`
    // reported fourteen widgets a person cannot press. That is the one escape
    // on this screen `containment` calls a layout decision rather than a slip —
    // and a decision it is, so it is made here rather than left to a budget
    // file: a scroll region is what a property grid with more rows than window
    // has, and the framework has had one since R55.G.4.
    //
    // The viewport is DERIVED from what the column above it takes, so a title
    // that changes height cannot leave this number stale.
    let grid_viewport_h = WIN_H.saturating_sub(PANEL_PAD * 2 + TITLE_H + TITLE_GAP);
    let grid = Scene::Scroll(
        ScrollNode::from_state(
            use_scroll_state(GRID_SCROLL_KEY),
            Rect::new(
                0,
                0,
                NAME_COL_W + VALUE_COL_W + GRID_BORDER * 2,
                grid_viewport_h,
            ),
            grid,
        )
        .with_axis(ScrollAxis::Vertical),
    );

    let mut children = vec![title, grid];
    children.extend(view_popup_overlay(
        editing,
        &model,
        (edit_state, edit_caret),
        &theme,
    ));
    // R1176 — the embedded asset file picker floats over everything when open
    // (the R788 modal scrim + centred panel + the lifted file_browser_pane).
    children.extend(view_asset_dialog(&theme, asset_buttons));

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

/// R1177 — the two asset-dialog action buttons' live interaction posture read
/// from the painted scene — `(ok_state, cancel_state)`. Paint and a11y read this
/// one source, so the AT tree never advertises a posture the painted button
/// contradicts (the R1176 static-posture cut had them diverge).
///
/// R1517 — the **focus** halves are deliberately NOT here. Posture (hover /
/// pressed / disabled) is the widget's own and belongs on the paint's cadence;
/// focus is the shell's, and the AT tree reads it from the `access_node`
/// argument like every other binding. See [`focus_state`](pinion_core::focus_state)
/// for which channel answers which question.
type AssetButtons = (ButtonState, ButtonState);

/// Cached paint posture for the two text fields — `(inline-cell-editor,
/// search-box)`, each `(interaction-state, caret)` — plus the asset dialog's
/// action-button postures ([`AssetButtons`]). The model / cursor / edit-mode /
/// filter query are read reactively in the view fn (the todomvc shape:
/// read_state carries the field + button postures, hooks carry the reactive
/// model + the search text).
type RootState = ((TextFieldState, u32), (TextFieldState, u32), AssetButtons);

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
            // R1250 — the shared commit-on-blur inline editor (lifted SSOT).
            blur_committing_field_extra(EDIT_TF_TAG),
            // R872 — the live search / filter box. No commit-on-blur intent:
            // the filter is live (every keystroke re-filters), not commit-gated.
            ExtraExternal::new(
                SEARCH_TF_TAG,
                Box::new(
                    TextFieldExternal::new()
                        .attach_state(search_state)
                        .attach_blink(search_blink),
                ),
            ),
            // R1176 — the embedded asset file picker: the `DirectoryExternal`
            // over the shared `DirectoryState` (so `asset_fb#<i>` row clicks +
            // `asset_fb#up` route to its `send`, and `navigate`/`select`/`cwd`/
            // `selected` are RPC-introspectable), the two action `ButtonExternal`s
            // (their `.click` intents drive `confirm_asset`/close), and the
            // modal-open introspection node (a query-only `open` bool).
            ExtraExternal::new(
                ASSET_DIR_TAG,
                Box::new(DirectoryExternal::new(asset_directory())),
            ),
            ExtraExternal::new(ASSET_OK_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(ASSET_CANCEL_TAG, Box::new(ButtonExternal::new())),
            modal_introspection_extra(ASSET_MODAL_STATE_TAG, asset_modal()),
        ]
    }

    fn read_state(scene: &Scene) -> RootState {
        (
            tf_paint::read_text_field_state(scene, EDIT_TF_TAG),
            tf_paint::read_text_field_state(scene, SEARCH_TF_TAG),
            // R1177 — the asset dialog buttons' live interaction posture, so
            // paint reads the same source a11y does (no static-`false` divergence).
            // R1517 dropped the focus halves: they fed the AT tree alone (the
            // paint discarded them), and the AT tree's focus source is the
            // shell's `focused` argument, so re-deriving it here walked the whole
            // scene twice a frame to answer a question the shell already answers.
            (
                read_button_state(scene, ASSET_OK_TAG),
                read_button_state(scene, ASSET_CANCEL_TAG),
            ),
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
        // R1176 — bridge the asset dialog's action buttons into the modal
        // lifecycle (the `<tag>.click` intent the `ButtonExternal`s emit). The
        // browser's row clicks route to the `DirectoryExternal` directly.
        match intent.tag_str() {
            t if t == ASSET_OK_CLICK => confirm_asset(),
            t if t == ASSET_CANCEL_CLICK => close_asset_dialog(),
            _ => {}
        }
        Vec::new()
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        // R1176 — while the asset picker is open, the modal trap owns the keys.
        // Escape cancels; the file list (the first member) drives
        // selection-follows-focus arrow nav, with Enter on a *file* confirming
        // and Enter on a *directory* descending; Enter / Space on an action
        // button activates it (emitting its `.click` intent).
        if asset_modal().is_open() {
            if key == "Escape" {
                close_asset_dialog();
                return true;
            }
            if focused == Some(ASSET_DIR_TAG) {
                let dir = asset_directory();
                if key == "Enter" {
                    if let Some(idx) = dir.cursor() {
                        if dir.entries().get(idx).is_some_and(|e| !e.is_dir) {
                            confirm_asset();
                            return true;
                        }
                    }
                }
                return dir_nav_key_selecting(
                    &dir,
                    &use_scroll_state(ASSET_SCROLL_KEY),
                    focused,
                    ASSET_DIR_TAG,
                    key,
                    ASSET_ROW_PITCH,
                );
            }
            return apply_aria_activate(scene, focused, key, ASSET_OK_TAG)
                || apply_aria_activate(scene, focused, key, ASSET_CANCEL_TAG);
        }
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
fn row_access_name(
    row: &VisibleRow,
    model: &[CellValue],
    array: &[CellValue],
    tree: &[PropertyNode],
) -> String {
    if let Some(vi) = row_value_index(&row.id) {
        let name = PROPERTY_NAMES
            .get(vi)
            .copied()
            .unwrap_or(row.label.as_str());
        model
            .get(vi)
            .map_or_else(|| name.to_owned(), |v| format!("{name}: {}", v.display()))
    } else if let Some(ValueRef::Elem(k)) = row_ref(&row.id) {
        // R931 — an array element: the qualified "Spawn Weights [k]" name folds
        // in the element value (the same single-column-tree convention as a
        // scalar leaf).
        let name = format!("{ARRAY_LABEL} [{k}]");
        array
            .get(k)
            .map_or(name.clone(), |v| format!("{name}: {}", v.display()))
    } else if row.id.starts_with(STRUCT_PREFIX) {
        format!(
            "{} {}",
            row.label,
            struct_value_summary(model, tree, &row.id)
        )
    } else if row.id.starts_with(ARR_PREFIX) {
        // R931 — the array branch announces its element count.
        let n = array.len();
        let unit = if n == 1 { "element" } else { "elements" };
        format!("{} ({n} {unit})", row.label)
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
        let array = use_array_model().get();
        let defaults = use_property_defaults();
        let array_defaults = use_array_defaults();
        let tree_nodes = use_property_tree().get();
        let rows = visible_property_rows(&tree_nodes, &current_search_query());
        // R1517 §5.39 §5.40 — the roving cursor is the `aria-activedescendant`,
        // which WAI-ARIA defines only while the COMPOSITE owns focus, so the row
        // flag is gated on the shell's focused tag exactly as
        // `access_focus_target` below already gates the target it points at.
        // Ungated (measured before this round) the cursor row kept claiming
        // focus after Tab moved to the search box, so two nodes claimed it at
        // once and the tree contradicted its own focus target.
        let cursor = use_property_cursor()
            .get()
            .filter(|_| focused == Some(GRID_TAG));
        // The single-column tree names each row with its value folded in. The
        // row tags (`{GRID_TAG}#{id}`) match the painted rows, so bounds resolve.
        let labelled: Vec<VisibleRow> = rows
            .iter()
            .map(|r| VisibleRow {
                label: row_access_name(r, &model, &array, &tree_nodes),
                ..r.clone()
            })
            .collect();
        let mut nodes = tree_access_nodes(
            GRID_TAG,
            GRID_TAG,
            Some("Inspector"),
            &labelled,
            None, // no selection model — the cursor is keyboard focus only
            cursor.as_deref(),
        );
        // R919 / R921 / R936 — each modified *visible* row (scalar leaf, struct,
        // array element or array branch) gains a reset `button` child
        // (`{GRID_TAG}#reset<id>`), named for the target and gated on the SAME
        // modified predicate the painted reset arrow uses, so the AT tree
        // advertises reset exactly when it paints (R886.1 one-gate). A collapsed /
        // filtered-out row emits neither.
        for r in &rows {
            let (modified, name) = if let Some(vi) = row_value_index(&r.id) {
                let m = leaf_modified(&model, &defaults, vi);
                (
                    m,
                    PROPERTY_NAMES
                        .get(vi)
                        .copied()
                        .unwrap_or(r.label.as_str())
                        .to_owned(),
                )
            } else if let Some(ValueRef::Elem(k)) = row_ref(&r.id) {
                (
                    leaf_modified(&array, &array_defaults, k),
                    format!("{ARRAY_LABEL} [{k}]"),
                )
            } else if r.id.starts_with(STRUCT_PREFIX) {
                (
                    struct_is_modified(&model, &defaults, &tree_nodes, &r.id),
                    r.label.clone(),
                )
            } else if r.id.starts_with(ARR_PREFIX) {
                (array_is_modified(&array, &array_defaults), r.label.clone())
            } else {
                continue; // a category is never modified-resettable
            };
            if !modified {
                continue;
            }
            // The host node tag must equal what `tree_access_nodes` emitted — use
            // its `tree_row_tag` SSOT so they cannot drift (R921.1 use-substrate).
            // R980 — the find+child+emit is the `attach_child_button` SSOT.
            attach_child_button(
                &mut nodes,
                &tree_row_tag(GRID_TAG, &r.id),
                format!("{GRID_TAG}#{RESET_PREFIX}{}", r.id),
                format!("Reset {name} to default"),
            );
        }
        // R931 — the dynamic-collection affordances as `button` children: each
        // array element row gains a "Remove" button, and the array branch row
        // gains an "Add" button (always present, unlike the modified-gated
        // reset, so an AT user can grow / shrink the list). Tags match the
        // painted buttons, so a click routes identically.
        for r in &rows {
            let row_tag = tree_row_tag(GRID_TAG, &r.id);
            if let Some(ValueRef::Elem(k)) = row_ref(&r.id) {
                attach_child_button(
                    &mut nodes,
                    &row_tag,
                    format!("{GRID_TAG}#{RM_ELEM_PREFIX}{k}"),
                    format!("Remove {ARRAY_LABEL} [{k}]"),
                );
            } else if r.id.starts_with(ARR_PREFIX) {
                attach_child_button(
                    &mut nodes,
                    &row_tag,
                    format!("{GRID_TAG}#{ADD_ELEM_TAG}"),
                    format!("Add {ARRAY_LABEL} element"),
                );
            }
        }
        // R873 — the live search box is a Tab stop; emit its textbox node (the
        // lifted `text_field_a11y_node` SSOT) so an AT user who tabs into it
        // hears a named `textbox` with the current query, not a silent node.
        let ((_, _), (search_posture, _), _) = *state;
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
        // R931 — count both scalar leaves and array element leaves (a `ValueRef`
        // address), so the dynamic elements are part of the announced total.
        let data_count = rows.iter().filter(|r| row_ref(&r.id).is_some()).count();
        nodes.push(
            // R1692 — declared the live region it was already described as.
            // R873's own comment above calls this "a polite live region" and
            // the node never said so, which left it announced with no painted
            // rectangle and nothing pointing at it: a `ghost` to `scene/voice`,
            // and to an AT a region that is neither reachable nor spoken.
            AccessNode::new("pg_search_status", AriaRole::Status)
                .with_name(format!("{data_count} properties"))
                .with_live(pinion_a11y::AccessLive::Polite),
        );
        // The open choice / colour popup's `listbox` nodes (gated on the same
        // `popup_view_pos` visibility predicate the paint uses, so the AT tree
        // never advertises an unpainted popup — R873).
        nodes.extend(popup_listbox_nodes(&model));
        // R1176 — when the asset picker is open, append its modal dialog tree.
        nodes.extend(asset_dialog_access_nodes(state.2, focused));
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
        // R1176 — while the asset picker's file list holds focus, the ring
        // follows the directory's roving cursor (the R802 file-open-dialog shape):
        // ring the cursor row's composite tag, not the whole list container.
        if focused == Some(ASSET_DIR_TAG) && asset_modal().is_open() {
            if let Some(cursor) = asset_directory().cursor() {
                return Some(AccessFocus::composite(
                    ASSET_DIR_TAG,
                    format!("{ASSET_DIR_TAG}#{cursor}"),
                ));
            }
        }
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
        Some(AccessFocus::addressing(
            GRID_TAG,
            use_property_cursor()
                .get()
                .map(|id| tree_row_tag(GRID_TAG, &id)),
        ))
    }

    /// R981 §5.40 — an AT Click / Default on an in-widget control button (the
    /// R919/R936 reset arrow, the R931 remove / add-element buttons) routes
    /// through the SAME `send` funnel a pointer click on that button drains, so
    /// AT activation and pointer activation are identical (the R980 carry,
    /// property-grid slice — the buttons were already announced via `access_node`
    /// and `attach_child_button` but, lacking this hook, an AT Click fell through
    /// to the parent grid's Enter). The gate keeps it to the control affordances:
    /// branch-row / leaf activation stays on the grid's keyboard roving (its
    /// cursor + Enter), so only a button child is send-routed here.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        if !matches!(action, AccessAction::Click | AccessAction::Default) {
            return false;
        }
        let is_control = sub_tag.starts_with(RESET_PREFIX)
            || sub_tag.starts_with(RM_ELEM_PREFIX)
            || sub_tag == ADD_ELEM_TAG;
        if !is_control {
            return false;
        }
        let Some(node) = scene.find_external_with_tag_mut(GRID_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        intro
            .invoke(
                "send",
                IntrospectValue::Text(format!("{sub_tag}:PointerUp")),
            )
            .is_ok()
    }
}

impl WidgetView for PropertyGridView {
    type Renderer = HelloPropertyGridRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<PropertyGridView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;
    use pinion_core::test_fixtures::assert_out_of_range_saying;

    // The typed-value pure helpers (kind / name / parse / display / the
    // keystroke gate) are now tested in `pinion_core::cell_value`; this
    // module tests the property-grid WIRING (coordinator, edit flow,
    // keyboard, a11y, paint) on top of the lifted SSOT.

    #[test]
    fn r921_tree_leaves_cover_every_value_slot_once() {
        // Every *flat-model* leaf (a scalar / struct field, `value_index = Some`)
        // indexes the value model, and every value slot is reached by exactly one
        // such leaf (no orphan / no duplicate). R931 array element leaves
        // (`value_index = None`, backed by the separate array sub-model) are
        // intentionally excluded — they address the array, not the flat model.
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
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag),
            ));
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
        let node = scene
            .find_external_with_tag_mut(GRID_TAG)
            .expect("grid present");
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
            assert_eq!(
                grid_intro(&scene).query("any_modified"),
                Ok(IntrospectValue::Bool(false)),
                "boot: clean"
            );
            assert_eq!(
                grid_intro(&scene).query("modified.4"),
                Ok(IntrospectValue::Bool(false))
            );
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.4", IntrospectValue::Int(17)).unwrap();
            });
            assert_eq!(
                grid_intro(&scene).query("modified.4"),
                Ok(IntrospectValue::Bool(true)),
                "an edited value is modified"
            );
            assert_eq!(
                grid_intro(&scene).query("any_modified"),
                Ok(IntrospectValue::Bool(true))
            );
            assert_eq!(
                grid_intro(&scene).query("modified.6"),
                Ok(IntrospectValue::Bool(false)),
                "an untouched row stays clean"
            );
            assert!(
                matches!(
                    grid_intro(&scene).query("modified.99"),
                    Err(ReadRefusal::NoSuchMember(_))
                ),
                "out-of-range modified -> None"
            );
            // Reset restores the default and clears modified.
            assert_eq!(
                with_grid_mut(&mut scene, |i| i.invoke("reset", IntrospectValue::Int(4))),
                Ok(IntrospectValue::Bool(true)),
                "reset changed the modified row",
            );
            assert_eq!(
                grid_intro(&scene).query("value.4"),
                Ok(IntrospectValue::Int(3)),
                "reset restored the default"
            );
            assert_eq!(
                grid_intro(&scene).query("modified.4"),
                Ok(IntrospectValue::Bool(false))
            );
            assert_eq!(
                grid_intro(&scene).query("any_modified"),
                Ok(IntrospectValue::Bool(false))
            );
            // Resetting an already-default row is a no-op `false`.
            assert_eq!(
                with_grid_mut(&mut scene, |i| i.invoke("reset", IntrospectValue::Int(4))),
                Ok(IntrospectValue::Bool(false)),
                "reset of an unmodified row is a no-op",
            );
        });
    }

    /// R981 §5.40 — an AT Click on the reset arrow routes through the same `send`
    /// funnel a pointer click drains, so AT reset == pointer reset (the R980
    /// carry, property-grid slice; the reset button was already announced via
    /// `access_node` but lacked this activation hook). A non-control Click still
    /// falls through to the grid's keyboard roving.
    #[test]
    fn r981_at_click_on_reset_button_resets_via_access_child_invoke() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Modify row 4 ("Layer", Int default 3) so its reset arrow is active.
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.4", IntrospectValue::Int(17)).unwrap();
            });
            assert_eq!(
                grid_intro(&scene).query("modified.4"),
                Ok(IntrospectValue::Bool(true))
            );
            // An AT Click on the reset button child resets the row to its default.
            assert!(
                PropertyGridView::access_child_invoke(
                    &mut scene,
                    GRID_TAG,
                    "reset4",
                    AccessAction::Click
                ),
                "an AT Click on the reset button is handled",
            );
            assert_eq!(
                grid_intro(&scene).query("modified.4"),
                Ok(IntrospectValue::Bool(false)),
                "the AT reset restored the row to its default",
            );
            // A non-control Click (a bare leaf row id) falls through (no prefix).
            assert!(
                !PropertyGridView::access_child_invoke(
                    &mut scene,
                    GRID_TAG,
                    "6",
                    AccessAction::Click
                ),
                "a non-control Click falls through to the grid's roving",
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
                i.intervene("value.6", IntrospectValue::Float(99.0))
                    .unwrap();
                i.intervene("value.2", IntrospectValue::Bool(false))
                    .unwrap();
            });
            assert_eq!(
                grid_intro(&scene).query("any_modified"),
                Ok(IntrospectValue::Bool(true))
            );
            assert_eq!(
                with_grid_mut(&mut scene, |i| i.invoke("reset_all", IntrospectValue::Null)),
                Ok(IntrospectValue::Int(3)),
                "reset_all returns the count reset",
            );
            assert_eq!(
                grid_intro(&scene).query("value.4"),
                Ok(IntrospectValue::Int(3)),
                "Int restored"
            );
            assert_eq!(
                grid_intro(&scene).query("value.6"),
                Ok(IntrospectValue::Float(12.5)),
                "Float restored"
            );
            assert_eq!(
                grid_intro(&scene).query("value.2"),
                Ok(IntrospectValue::Bool(true)),
                "Bool restored"
            );
            assert_eq!(
                grid_intro(&scene).query("any_modified"),
                Ok(IntrospectValue::Bool(false))
            );
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
            assert!(
                !view(idle_state(), &Frame::new()).contains_tag(&arrow_tag),
                "no reset arrow on a clean row"
            );
            assert!(
                PropertyGridView::access_node(&idle_state(), Some(GRID_TAG))
                    .iter()
                    .all(|n| n.tag != arrow_tag),
                "no reset button advertised while clean",
            );
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.4", IntrospectValue::Int(17)).unwrap();
            });
            assert!(
                view(idle_state(), &Frame::new()).contains_tag(&arrow_tag),
                "the modified row paints a reset arrow"
            );
            let a11y = PropertyGridView::access_node(&idle_state(), Some(GRID_TAG));
            let btn = a11y
                .iter()
                .find(|n| n.tag == arrow_tag)
                .expect("reset button advertised");
            assert_eq!(btn.role, AriaRole::Button);
            assert_eq!(
                btn.name.as_deref(),
                Some("Reset Layer to default"),
                "named for the property"
            );
            // R921 — the reset button is a child of the Layer leaf's `treeitem`
            // row node (`{GRID_TAG}#4`), the painted-row tag.
            let row = a11y
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#4"))
                .expect("Layer row node");
            assert!(
                row.children.iter().any(|c| c.as_str() == arrow_tag),
                "the button is the row's child"
            );
        });
    }

    /// R920 (audit) — `Delete` on the cursor data row resets it to default (the
    /// keyboard path to the reset arrow, whose AccessNode is not a tab stop). The
    /// keyboard, click, and RPC all share the one `reset` funnel.
    #[test]
    fn r920_keyboard_delete_resets_cursor_row() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.4", IntrospectValue::Int(17)).unwrap();
            });
            // Click row 4 (an Int row: a click only moves the roving cursor onto it).
            with_grid_mut(&mut scene, |i| {
                let _ = i.invoke("send", IntrospectValue::Text("4:PointerUp".to_owned()));
            });
            assert!(
                apply_key_grid(&mut scene, "Delete"),
                "Delete on a data row is consumed"
            );
            assert_eq!(
                grid_intro(&scene).query("value.4"),
                Ok(IntrospectValue::Int(3)),
                "Delete reset the cursor row"
            );
            assert_eq!(
                grid_intro(&scene).query("modified.4"),
                Ok(IntrospectValue::Bool(false))
            );
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
                let _ = i.invoke(
                    "send",
                    IntrospectValue::Text(format!("{RESET_PREFIX}4:PointerUp")),
                );
            });
            assert_eq!(
                grid_intro(&scene).query("value.4"),
                Ok(IntrospectValue::Int(3)),
                "the arrow click reset the row"
            );
            assert_eq!(
                grid_intro(&scene).query("modified.4"),
                Ok(IntrospectValue::Bool(false))
            );
        });
    }

    /// R936 — an array element is modified once its value differs from the class
    /// default `[1.0, 0.5, 0.25]`; the unified `reset` Text node-id funnel
    /// restores it, and `modified.elem.<k>` tracks it (the element peer of
    /// `r919_modified_and_reset_via_rpc`).
    #[test]
    fn r936_element_modified_and_reset_via_rpc() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert_eq!(
                grid_intro(&scene).query("modified.elem.0"),
                Ok(IntrospectValue::Bool(false)),
                "boot: clean"
            );
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.elem.0", IntrospectValue::Float(9.0))
                    .unwrap();
            });
            assert_eq!(
                grid_intro(&scene).query("modified.elem.0"),
                Ok(IntrospectValue::Bool(true)),
                "an edited element is modified"
            );
            assert_eq!(
                grid_intro(&scene).query("modified.elem.1"),
                Ok(IntrospectValue::Bool(false)),
                "a sibling element stays clean"
            );
            assert!(
                matches!(
                    grid_intro(&scene).query("modified.elem.9"),
                    Err(ReadRefusal::NoSuchMember(_))
                ),
                "out-of-range element modified -> None"
            );
            // The unified `reset` funnel takes an element node id, like the click.
            assert_eq!(
                with_grid_mut(&mut scene, |i| i
                    .invoke("reset", IntrospectValue::Text("elem.0".to_owned()))),
                Ok(IntrospectValue::Bool(true)),
                "reset changed the modified element",
            );
            assert_eq!(
                grid_intro(&scene).query("value.elem.0"),
                Ok(IntrospectValue::Float(1.0)),
                "reset restored the default"
            );
            assert_eq!(
                grid_intro(&scene).query("modified.elem.0"),
                Ok(IntrospectValue::Bool(false))
            );
            assert_eq!(
                with_grid_mut(&mut scene, |i| i
                    .invoke("reset", IntrospectValue::Text("elem.0".to_owned()))),
                Ok(IntrospectValue::Bool(false)),
                "reset of an already-default element is a no-op",
            );
        });
    }

    /// R936 — the array branch's modified roll-up (`array_modified.<id>`) is true
    /// when any element differs OR the length changed; `reset_array` restores the
    /// whole list (length + content). The array peer of the struct aggregate.
    #[test]
    fn r936_array_modified_rollup_and_reset() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let arr_q = format!("array_modified.{ARR_BRANCH_ID}");
            assert_eq!(
                grid_intro(&scene).query(&arr_q),
                Ok(IntrospectValue::Bool(false)),
                "boot: array clean"
            );
            assert!(
                matches!(
                    grid_intro(&scene).query("array_modified.struct.Position"),
                    Err(ReadRefusal::NoSuchMember(_))
                ),
                "only the array branch id is valid"
            );
            // (a) an element edit makes the branch modified; reset clears it.
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.elem.1", IntrospectValue::Float(9.0))
                    .unwrap();
            });
            assert_eq!(
                grid_intro(&scene).query(&arr_q),
                Ok(IntrospectValue::Bool(true)),
                "edited element -> array modified"
            );
            with_grid_mut(&mut scene, |i| {
                i.invoke("reset", IntrospectValue::Text("elem.1".to_owned()))
                    .unwrap();
            });
            assert_eq!(
                grid_intro(&scene).query(&arr_q),
                Ok(IntrospectValue::Bool(false)),
                "element reset -> array clean"
            );
            // (b) a length change makes the branch modified; `reset_array` restores
            // both length and content in one wholesale step.
            with_grid_mut(&mut scene, |i| {
                i.invoke("add_elem", IntrospectValue::Null).unwrap();
            });
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.elem.0", IntrospectValue::Float(7.0))
                    .unwrap();
            });
            assert_eq!(
                grid_intro(&scene).query(&arr_q),
                Ok(IntrospectValue::Bool(true)),
                "length + content differ"
            );
            assert_eq!(
                with_grid_mut(&mut scene, |i| i
                    .invoke("reset_array", IntrospectValue::Null)),
                Ok(IntrospectValue::Bool(true)),
                "reset_array changed the list",
            );
            assert_eq!(
                grid_intro(&scene).query("elem_count"),
                Ok(IntrospectValue::Int(3)),
                "length restored"
            );
            assert_eq!(
                grid_intro(&scene).query("value.elem.0"),
                Ok(IntrospectValue::Float(1.0)),
                "content restored"
            );
            assert_eq!(
                grid_intro(&scene).query(&arr_q),
                Ok(IntrospectValue::Bool(false))
            );
            assert_eq!(
                with_grid_mut(&mut scene, |i| i
                    .invoke("reset_array", IntrospectValue::Null)),
                Ok(IntrospectValue::Bool(false)),
                "reset_array on a default list is a no-op",
            );
        });
    }

    /// R936 — a modified element paints its reset arrow (`reset elem.<k>`, one slot
    /// left of the remove button) and the a11y tree advertises a matching reset
    /// `button` child of the element row, gated on the same predicate (R886.1
    /// one-gate). An added element (no class default) gets NO per-element reset
    /// arrow — the array-level reset truncates it instead.
    #[test]
    fn r936_element_reset_arrow_paints_with_a11y_button() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let arrow_tag = format!("{GRID_TAG}#{RESET_PREFIX}elem.0");
            assert!(
                !view(idle_state(), &Frame::new()).contains_tag(&arrow_tag),
                "no reset arrow on a clean element"
            );
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.elem.0", IntrospectValue::Float(9.0))
                    .unwrap();
            });
            assert!(
                view(idle_state(), &Frame::new()).contains_tag(&arrow_tag),
                "the modified element paints a reset arrow"
            );
            // The remove button stays — both trailing affordances coexist.
            assert!(
                view(idle_state(), &Frame::new())
                    .contains_tag(&format!("{GRID_TAG}#{RM_ELEM_PREFIX}0")),
                "remove button still painted"
            );
            let a11y = PropertyGridView::access_node(&idle_state(), Some(GRID_TAG));
            let btn = a11y
                .iter()
                .find(|n| n.tag == arrow_tag)
                .expect("element reset button advertised");
            assert_eq!(btn.role, AriaRole::Button);
            assert_eq!(
                btn.name.as_deref(),
                Some("Reset Spawn Weights [0] to default")
            );
            let row = a11y
                .iter()
                .find(|n| n.tag == tree_row_tag(GRID_TAG, "elem.0"))
                .expect("element row node");
            assert!(
                row.children.iter().any(|c| c.as_str() == arrow_tag),
                "the reset button is the element row's child"
            );
            // An added element (index 3, no class counterpart) is array-modified but
            // never per-element-resettable.
            with_grid_mut(&mut scene, |i| {
                i.invoke("add_elem", IntrospectValue::Null).unwrap();
            });
            assert!(
                !view(idle_state(), &Frame::new())
                    .contains_tag(&format!("{GRID_TAG}#{RESET_PREFIX}elem.3")),
                "an added element has no per-element reset arrow"
            );
        });
    }

    /// R936 — a modified array branch paints its reset arrow (`reset arr.weights`,
    /// left of the add button) with a matching a11y reset `button` child of the
    /// branch row (the array peer of the struct reset-arrow one-gate).
    #[test]
    fn r936_array_branch_reset_arrow_paints_with_a11y_button() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let arrow_tag = format!("{GRID_TAG}#{RESET_PREFIX}{ARR_BRANCH_ID}");
            assert!(
                !view(idle_state(), &Frame::new()).contains_tag(&arrow_tag),
                "no reset arrow on a clean array branch"
            );
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.elem.0", IntrospectValue::Float(9.0))
                    .unwrap();
            });
            assert!(
                view(idle_state(), &Frame::new()).contains_tag(&arrow_tag),
                "the modified array branch paints a reset arrow"
            );
            assert!(
                view(idle_state(), &Frame::new())
                    .contains_tag(&format!("{GRID_TAG}#{ADD_ELEM_TAG}")),
                "add button still painted"
            );
            let a11y = PropertyGridView::access_node(&idle_state(), Some(GRID_TAG));
            let btn = a11y
                .iter()
                .find(|n| n.tag == arrow_tag)
                .expect("array branch reset button advertised");
            assert_eq!(btn.role, AriaRole::Button);
            assert_eq!(btn.name.as_deref(), Some("Reset Spawn Weights to default"));
            let row = a11y
                .iter()
                .find(|n| n.tag == tree_row_tag(GRID_TAG, ARR_BRANCH_ID))
                .expect("array branch row node");
            assert!(
                row.children.iter().any(|c| c.as_str() == arrow_tag),
                "the reset button is the array branch row's child"
            );
        });
    }

    /// R936 — clicking an element's / the array branch's reset arrow routes to the
    /// same `reset_element` / `reset_array` funnel the RPC uses (the
    /// `{GRID_TAG}#reset<id>` wire, one decode for all four reset targets).
    #[test]
    fn r936_reset_arrow_clicks_route() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Element arrow click.
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.elem.0", IntrospectValue::Float(9.0))
                    .unwrap();
                let _ = i.invoke(
                    "send",
                    IntrospectValue::Text(format!("{RESET_PREFIX}elem.0:PointerUp")),
                );
            });
            assert_eq!(
                grid_intro(&scene).query("value.elem.0"),
                Ok(IntrospectValue::Float(1.0)),
                "element arrow click reset it"
            );
            // Array branch arrow click (after a length change).
            with_grid_mut(&mut scene, |i| {
                i.invoke("add_elem", IntrospectValue::Null).unwrap();
                i.intervene("value.elem.1", IntrospectValue::Float(8.0))
                    .unwrap();
                let _ = i.invoke(
                    "send",
                    IntrospectValue::Text(format!("{RESET_PREFIX}{ARR_BRANCH_ID}:PointerUp")),
                );
            });
            assert_eq!(
                grid_intro(&scene).query("elem_count"),
                Ok(IntrospectValue::Int(3)),
                "array arrow click restored length"
            );
            assert_eq!(
                grid_intro(&scene).query("value.elem.1"),
                Ok(IntrospectValue::Float(0.5)),
                "array arrow click restored content"
            );
        });
    }

    /// R936 — `reset_all` returns the WHOLE object to default: scalars AND the
    /// array branch (one reset unit). `any_modified` must reflect the array too,
    /// so a dirty array never hides behind a "clean" object readout.
    #[test]
    fn r936_reset_all_includes_array() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.4", IntrospectValue::Int(17)).unwrap(); // a scalar
                i.intervene("value.elem.0", IntrospectValue::Float(9.0))
                    .unwrap(); // the array
            });
            assert_eq!(
                grid_intro(&scene).query("any_modified"),
                Ok(IntrospectValue::Bool(true)),
                "scalar OR array dirties the object"
            );
            assert_eq!(
                with_grid_mut(&mut scene, |i| i.invoke("reset_all", IntrospectValue::Null)),
                Ok(IntrospectValue::Int(2)),
                "reset_all counts the scalar + the array as 2 reset units",
            );
            assert_eq!(
                grid_intro(&scene).query("value.4"),
                Ok(IntrospectValue::Int(3)),
                "scalar restored"
            );
            assert_eq!(
                grid_intro(&scene).query("value.elem.0"),
                Ok(IntrospectValue::Float(1.0)),
                "array restored"
            );
            assert_eq!(
                grid_intro(&scene).query("any_modified"),
                Ok(IntrospectValue::Bool(false)),
                "object clean after reset_all"
            );
        });
    }

    /// R936 — a dirty array alone (no scalar touched) still reports `any_modified`
    /// true, the regression guard for the object-level roll-up that R936 added.
    #[test]
    fn r936_dirty_array_alone_dirties_the_object() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.elem.2", IntrospectValue::Float(3.0))
                    .unwrap();
            });
            assert_eq!(
                grid_intro(&scene).query("any_modified"),
                Ok(IntrospectValue::Bool(true)),
                "a dirty array alone is dirty"
            );
            with_grid_mut(&mut scene, |i| {
                i.invoke("reset", IntrospectValue::Text("elem.2".to_owned()))
                    .unwrap();
            });
            assert_eq!(
                grid_intro(&scene).query("any_modified"),
                Ok(IntrospectValue::Bool(false))
            );
        });
    }

    #[test]
    fn r836_query_exposes_typed_values_names_kinds() {
        Owner::new().run(|| {
            let scene = boot_scene();
            let intro = grid_intro(&scene);
            assert_eq!(
                intro.query("row_count"),
                Ok(IntrospectValue::Int(16)),
                "12 scalars + 4 struct fields"
            );
            assert_eq!(
                intro.query("name.0"),
                Ok(IntrospectValue::Text("Name".to_owned()))
            );
            assert_eq!(
                intro.query("name.6"),
                Ok(IntrospectValue::Text("Position X".to_owned()))
            );
            assert_eq!(
                intro.query("kind.2"),
                Ok(IntrospectValue::Text("bool".to_owned()))
            );
            assert_eq!(
                intro.query("kind.4"),
                Ok(IntrospectValue::Text("int".to_owned()))
            );
            assert_eq!(
                intro.query("kind.6"),
                Ok(IntrospectValue::Text("float".to_owned()))
            );
            assert_eq!(intro.query("value.2"), Ok(IntrospectValue::Bool(true)));
            assert_eq!(intro.query("value.4"), Ok(IntrospectValue::Int(3)));
            assert_eq!(intro.query("value.6"), Ok(IntrospectValue::Float(12.5)));
            assert_eq!(
                intro.query("value.0"),
                Ok(IntrospectValue::Text("Player".to_owned()))
            );
            assert!(
                matches!(intro.query("value.99"), Err(ReadRefusal::NoSuchMember(_))),
                "R1667 - the family is declared and this argument addresses nothing"
            );
            assert_eq!(
                intro.query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null))
            );
        });
    }

    #[test]
    fn r836_intervene_sets_typed_value_strictly() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Strict per kind.
            assert!(intro.intervene("value.4", IntrospectValue::Int(17)).is_ok());
            assert_eq!(
                intro.intervene("value.4", IntrospectValue::Text("no".to_owned())),
                Err(InterveneError::TypeMismatch),
                "int row rejects text",
            );
            assert!(
                intro
                    .intervene("value.2", IntrospectValue::Bool(false))
                    .is_ok()
            );
            assert_eq!(
                intro.intervene("editing", IntrospectValue::Int(0)),
                Err(InterveneError::ReadOnly),
            );
            assert_eq!(intro.query("value.4"), Ok(IntrospectValue::Int(17)));
            assert_eq!(intro.query("value.2"), Ok(IntrospectValue::Bool(false)));
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
                    Ok(IntrospectValue::Int(i)) => i,
                    other => panic!("{p} -> {other:?}"),
                };
                let text = |p: &str| match tn.query(p) {
                    Ok(IntrospectValue::Text(t)) => t,
                    other => panic!("{p} -> {other:?}"),
                };
                let expanded0 = tn.query("expanded_at.0") == Ok(IntrospectValue::Bool(true));
                // Row 15 = the Position struct header (after 5 cats + Identity's 3
                // leaves + Appearance's 4 + Physics's 2 + Stats's 1 + the Transform
                // header); row 16 = its first field (Position X).
                (
                    int("row_count"),
                    text("id_at.0"),
                    int("level_at.0"),
                    expanded0,
                    text("id_at.15"),
                    int("level_at.16"),
                )
            };
            // 6 categories + 2 structs + 1 array branch + 19 leaves (16 scalar +
            // 3 R931 array elements) = 28 visible rows (all open).
            let (rows, id0, level0, exp0, id15, level16) = tree_intro(&scene);
            assert_eq!(rows, 28, "6 categories + 2 structs + 1 array + 19 leaves");
            assert_eq!(id0, "cat.Identity");
            assert_eq!(level0, 1, "a category is aria-level 1");
            assert!(exp0, "categories boot expanded");
            assert_eq!(
                id15, "struct.Position",
                "the Transform category nests the Position struct"
            );
            assert_eq!(
                level16, 3,
                "a struct field is aria-level 3 (category > struct > field)"
            );
            // The primary owns the struct aggregate + the per-branch collapse.
            let gi = grid_intro(&scene);
            let IntrospectValue::Text(summary) =
                gi.query("struct_summary.struct.Position").expect("summary")
            else {
                panic!("summary is text");
            };
            assert!(
                summary.starts_with('(') && summary.ends_with(')') && summary.contains("12.5"),
                "Position summary is a tuple of its field values, got {summary}",
            );
            assert_eq!(
                gi.query("struct_modified.struct.Position"),
                Ok(IntrospectValue::Bool(false)),
                "boot clean"
            );
            assert_eq!(
                gi.query("expanded.cat.Identity"),
                Ok(IntrospectValue::Bool(true))
            );
            assert_eq!(
                gi.query("expanded.0"),
                Ok(IntrospectValue::Null),
                "a leaf has no expanded flag"
            );
            // Collapse Identity via toggle_branch → its 3 leaves vanish (23 − 3).
            assert_eq!(
                with_grid_mut(&mut scene, |i| i.invoke(
                    "toggle_branch",
                    IntrospectValue::Text("cat.Identity".to_owned())
                )),
                Ok(IntrospectValue::Bool(false)),
                "toggle_branch returns the resulting expanded flag",
            );
            assert_eq!(
                grid_intro(&scene).query("expanded.cat.Identity"),
                Ok(IntrospectValue::Bool(false))
            );
            assert_eq!(tree_intro(&scene).0, 25, "28 − 3 Identity leaves");
            // Editing a struct field marks the struct modified; reset_struct clears it.
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.6", IntrospectValue::Float(99.0))
                    .unwrap();
            });
            assert_eq!(
                grid_intro(&scene).query("struct_modified.struct.Position"),
                Ok(IntrospectValue::Bool(true))
            );
            assert_eq!(
                with_grid_mut(&mut scene, |i| i.invoke(
                    "reset_struct",
                    IntrospectValue::Text("struct.Position".to_owned())
                )),
                Ok(IntrospectValue::Int(1)),
                "reset_struct restores 1 modified field",
            );
            assert_eq!(
                grid_intro(&scene).query("value.6"),
                Ok(IntrospectValue::Float(12.5))
            );
            assert_eq!(
                grid_intro(&scene).query("struct_modified.struct.Position"),
                Ok(IntrospectValue::Bool(false))
            );
        });
    }

    /// R921 — the roving cursor is read/write over RPC (the AI-first cursor
    /// move): `intervene cursor` sets a node id with no click side effect, `query
    /// cursor` reads it, and `Null` clears it.
    #[test]
    fn r921_cursor_read_write_over_rpc() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert_eq!(
                grid_intro(&scene).query("cursor"),
                Ok(IntrospectValue::Null),
                "no cursor at boot"
            );
            with_grid_mut(&mut scene, |i| {
                i.intervene(
                    "cursor",
                    IntrospectValue::Text("struct.Position".to_owned()),
                )
                .unwrap();
            });
            assert_eq!(
                grid_intro(&scene).query("cursor"),
                Ok(IntrospectValue::Text("struct.Position".to_owned()))
            );
            // A pure cursor move does NOT toggle / edit the row (no side effect).
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null))
            );
            with_grid_mut(&mut scene, |i| {
                i.intervene("cursor", IntrospectValue::Null).unwrap();
            });
            assert_eq!(
                grid_intro(&scene).query("cursor"),
                Ok(IntrospectValue::Null),
                "Null clears the cursor"
            );
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
                Ok(IntrospectValue::Json(serde_json::Value::from("9"))),
                "while visible the open popup is advertised",
            );
            assert_eq!(
                grid_intro(&scene).query("popup_cursor"),
                Ok(IntrospectValue::Int(0))
            );
            // Collapse Appearance (hides the Blend row) via RPC.
            with_grid_mut(&mut scene, |i| {
                let _ = i.invoke(
                    "toggle_branch",
                    IntrospectValue::Text("cat.Appearance".to_owned()),
                );
            });
            // The now-hidden edit/popup reports as not-editing (matches paint).
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null)),
                "a collapsed edit is not advertised",
            );
            assert_eq!(
                grid_intro(&scene).query("popup_cursor"),
                Ok(IntrospectValue::Null)
            );
            // The invisible popup does not intercept the grid keymap: ArrowDown
            // moves the tree cursor (Identity branch -> its first leaf).
            set_cursor_id("cat.Identity");
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowDown",
                Modifiers::empty()
            ));
            assert_eq!(
                cursor_id().as_deref(),
                Some("0"),
                "tree nav advanced, not hijacked by the hidden popup"
            );
            // Re-expanding re-advertises the still-open edit (state suspended, not destroyed).
            with_grid_mut(&mut scene, |i| {
                let _ = i.invoke(
                    "toggle_branch",
                    IntrospectValue::Text("cat.Appearance".to_owned()),
                );
            });
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::from("9"))),
                "re-expanding restores the advertisement",
            );
        });
    }

    #[test]
    fn r836_toggle_invoke_flips_bool_by_source() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Toggle the Visible bool by its stable source index (2).
            assert_eq!(
                intro.invoke("toggle", IntrospectValue::Int(2)),
                Ok(IntrospectValue::Bool(true))
            );
            assert_eq!(intro.query("value.2"), Ok(IntrospectValue::Bool(false)));
            // A non-bool source -> no-op.
            assert_eq!(
                intro.invoke("toggle", IntrospectValue::Int(0)),
                Ok(IntrospectValue::Bool(false))
            );
            assert_eq!(
                intro.query("value.0"),
                Ok(IntrospectValue::Text("Player".to_owned()))
            );
        });
    }

    #[test]
    fn r836_click_moves_cursor_and_toggles_bool() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // PointerUp on the Locked bool (source 3) moves the cursor onto its
            // leaf-id row + toggles it.
            let _ = intro.invoke("send", IntrospectValue::Text("3:PointerUp".to_owned()));
            assert_eq!(
                intro.query("value.3"),
                Ok(IntrospectValue::Bool(true)),
                "false -> true"
            );
            assert_eq!(
                cursor_id().as_deref(),
                Some("3"),
                "cursor moved onto the clicked leaf"
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            // Pos X (source 6) boots at 12.5.
            assert_eq!(
                node.handle.introspect().unwrap().query("value.6"),
                Ok(IntrospectValue::Float(12.5))
            );
            // Press the row (arm), then drag the captured cursor from x_rel 0.5
            // to 0.75 across the 400px grid: travel = 0.25 · 400 = 100px → +1.0.
            node.handle
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("6:PointerDown".to_owned()))
                .unwrap();
            node.handle
                .pointer_move(PointerReading::over_unit((0.5, 0.5))); // calibrate (no mutation)
            assert_eq!(
                node.handle.introspect().unwrap().query("value.6"),
                Ok(IntrospectValue::Float(12.5)),
                "first move only calibrates"
            );
            assert_eq!(
                node.handle.introspect().unwrap().query("scrubbing"),
                Ok(IntrospectValue::Bool(false)),
                "R915: the calibration frame is a click so far, not yet a scrub"
            );
            node.handle
                .pointer_move(PointerReading::over_unit((0.75, 0.5))); // apply +100px (past the 4px dead zone)
            assert_eq!(
                node.handle.introspect().unwrap().query("scrubbing"),
                Ok(IntrospectValue::Bool(true)),
                "a real drag past the threshold is a scrub"
            );
            let IntrospectValue::Float(v) =
                node.handle.introspect().unwrap().query("value.6").unwrap()
            else {
                panic!("Pos X stays a float");
            };
            assert!((v - 13.5).abs() < 1e-6, "12.5 + 100px·0.01 = 13.5, got {v}");
            // Release clears the scrub; the value is the committed live value.
            node.handle
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("6:PointerUp".to_owned()))
                .unwrap();
            assert_eq!(
                node.handle.introspect().unwrap().query("scrubbing"),
                Ok(IntrospectValue::Bool(false))
            );
        });
    }

    /// R964 — `clamp_to_range` bounds only a ranged scalar's Float; an unranged
    /// slot and a non-Float value pass through unchanged.
    #[test]
    fn r964_clamp_to_range_bounds_only_ranged_float() {
        assert_eq!(scalar_range(OPACITY_SLOT), Some((0.0, 1.0)));
        assert_eq!(scalar_range(6), None, "Pos X is unranged");
        assert_eq!(
            clamp_to_range(OPACITY_SLOT, CellValue::Float(2.5)),
            CellValue::Float(1.0)
        );
        assert_eq!(
            clamp_to_range(OPACITY_SLOT, CellValue::Float(-0.5)),
            CellValue::Float(0.0)
        );
        assert_eq!(
            clamp_to_range(OPACITY_SLOT, CellValue::Float(0.25)),
            CellValue::Float(0.25)
        );
        // Unranged slot / non-Float value are never clamped.
        assert_eq!(
            clamp_to_range(6, CellValue::Float(2.5)),
            CellValue::Float(2.5)
        );
        assert_eq!(
            clamp_to_range(OPACITY_SLOT, CellValue::Int(9)),
            CellValue::Int(9)
        );
    }

    /// R964 — `range.<i>` reports the interval for a ranged scalar and Null for an
    /// unranged leaf; the RPC `value.<i>` intervene clamps every write.
    #[test]
    fn r964_range_query_and_intervene_clamp() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            assert_eq!(
                node.handle.introspect().unwrap().query("range.8"),
                Ok(IntrospectValue::Text("0..1".to_owned()))
            );
            assert_eq!(
                node.handle.introspect().unwrap().query("range.6"),
                Ok(IntrospectValue::Text("none".to_owned())),
                "Pos X is unranged"
            );
            // An out-of-range RPC write clamps both ends through the set_value funnel.
            node.handle
                .introspect_mut()
                .unwrap()
                .intervene("value.8", IntrospectValue::Float(2.5))
                .unwrap();
            assert_eq!(
                node.handle.introspect().unwrap().query("value.8"),
                Ok(IntrospectValue::Float(1.0)),
                "above-max write clamps to 1.0"
            );
            node.handle
                .introspect_mut()
                .unwrap()
                .intervene("value.8", IntrospectValue::Float(-3.0))
                .unwrap();
            assert_eq!(
                node.handle.introspect().unwrap().query("value.8"),
                Ok(IntrospectValue::Float(0.0)),
                "below-min write clamps to 0.0"
            );
        });
    }

    /// R964 — a scrub past the interval clamps, and returning the cursor
    /// un-clamps cleanly (the value is recomputed from the press base each move,
    /// never from the clamped result).
    #[test]
    fn r964_scrub_clamps_at_top_and_unclamps_on_return() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            // Opacity (source 8) boots at the top of [0, 1].
            assert_eq!(
                node.handle.introspect().unwrap().query("value.8"),
                Ok(IntrospectValue::Float(1.0))
            );
            node.handle
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("8:PointerDown".to_owned()))
                .unwrap();
            node.handle
                .pointer_move(PointerReading::over_unit((0.5, 0.5))); // calibrate (base = 1.0)
            node.handle
                .pointer_move(PointerReading::over_unit((0.6, 0.5))); // +40px → 1.0 + 0.4 = 1.4 → clamp 1.0
            assert_eq!(
                node.handle.introspect().unwrap().query("value.8"),
                Ok(IntrospectValue::Float(1.0)),
                "scrub past the top clamps to max"
            );
            node.handle
                .pointer_move(PointerReading::over_unit((0.45, 0.5))); // −20px from press → 1.0 − 0.2 = 0.8
            let IntrospectValue::Float(v) =
                node.handle.introspect().unwrap().query("value.8").unwrap()
            else {
                panic!("Opacity stays a float");
            };
            assert!(
                (v - 0.8).abs() < 1e-6,
                "the intermediate clamp un-clamps on return: got {v}"
            );
            node.handle
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("8:PointerUp".to_owned()))
                .unwrap();
        });
    }

    /// Int scrub steps in whole units (8px/step) and a leftward drag decrements.
    #[test]
    fn r875_int_scrub_steps_in_whole_units_both_directions() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            // Layer (source 4) boots at 3. Drag +80px → +10 steps → 13.
            node.handle
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("4:PointerDown".to_owned()))
                .unwrap();
            node.handle
                .pointer_move(PointerReading::over_unit((0.5, 0.5)));
            node.handle
                .pointer_move(PointerReading::over_unit((0.7, 0.5))); // +0.2·400 = 80px → +10
            assert_eq!(
                node.handle.introspect().unwrap().query("value.4"),
                Ok(IntrospectValue::Int(13))
            );
            // Same gesture, leftward: a fresh press anchors, drag −80px → 3.
            node.handle
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("4:PointerUp".to_owned()))
                .unwrap();
            node.handle
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("4:PointerDown".to_owned()))
                .unwrap();
            node.handle
                .pointer_move(PointerReading::over_unit((0.5, 0.5)));
            node.handle
                .pointer_move(PointerReading::over_unit((0.3, 0.5))); // −80px → −10 → 3
            assert_eq!(
                node.handle.introspect().unwrap().query("value.4"),
                Ok(IntrospectValue::Int(3))
            );
        });
    }

    /// A scrub suppresses the trailing click (no editor opens), and a drag on a
    /// non-numeric row never scrubs (its release still performs the click).
    #[test]
    fn r875_scrub_suppresses_click_and_skips_non_numeric() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            // Scrub the Float row, then release: the editor must NOT open.
            node.handle
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("6:PointerDown".to_owned()))
                .unwrap();
            node.handle
                .pointer_move(PointerReading::over_unit((0.5, 0.5)));
            node.handle
                .pointer_move(PointerReading::over_unit((0.6, 0.5)));
            node.handle
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("6:PointerUp".to_owned()))
                .unwrap();
            assert_eq!(
                node.handle.introspect().unwrap().query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null)),
                "a scrub does not open the editor"
            );
            // A drag on the Visible bool (source 2) does not scrub; the release
            // still toggles it (true → false).
            node.handle
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("2:PointerDown".to_owned()))
                .unwrap();
            node.handle
                .pointer_move(PointerReading::over_unit((0.5, 0.5)));
            node.handle
                .pointer_move(PointerReading::over_unit((0.8, 0.5))); // no-op (non-numeric armed → none)
            assert_eq!(
                node.handle.introspect().unwrap().query("scrubbing"),
                Ok(IntrospectValue::Bool(false)),
                "a non-numeric press never calibrates a scrub"
            );
            node.handle
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("2:PointerUp".to_owned()))
                .unwrap();
            assert_eq!(
                node.handle.introspect().unwrap().query("value.2"),
                Ok(IntrospectValue::Bool(false)),
                "the bool still toggles on release (drag did not scrub it)"
            );
        });
    }

    // ----- edit-in-cell flow -----

    #[test]
    fn r836_begin_commit_writes_back_parsed_value() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // begin_edit on the Layer int row (index 4) via invoke (the RPC
            // edit-entry path) seeds the shared editor with the value text.
            let n = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = n.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.invoke("begin", IntrospectValue::Int(4)),
                Ok(IntrospectValue::Bool(true))
            );
            assert_eq!(
                intro.query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::from("4")))
            );
            assert_eq!(
                use_text_edit_state(EDIT_TF_TAG).text(),
                "3",
                "seeded with Layer value"
            );
            // Type a new value + commit.
            use_text_edit_state(EDIT_TF_TAG).set_text("12".to_owned());
            commit_edit(true);
            let intro = grid_intro(&scene);
            assert_eq!(
                intro.query("value.4"),
                Ok(IntrospectValue::Int(12)),
                "committed"
            );
            assert_eq!(
                intro.query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null))
            );
        });
    }

    #[test]
    fn r836_begin_rejects_bool_rows() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let n = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = n.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.invoke("begin", IntrospectValue::Int(2)),
                Ok(IntrospectValue::Bool(false))
            );
            assert_eq!(
                intro.query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null))
            );
        });
    }

    #[test]
    fn r836_cancel_keeps_prior_value() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let n = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = n.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("begin", IntrospectValue::Int(0));
            use_text_edit_state(EDIT_TF_TAG).set_text("changed".to_owned());
            cancel_edit();
            let intro = grid_intro(&scene);
            assert_eq!(
                intro.query("value.0"),
                Ok(IntrospectValue::Text("Player".to_owned()))
            );
            assert_eq!(
                intro.query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null))
            );
        });
    }

    #[test]
    fn r836_commit_malformed_number_reverts() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let n = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = n.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("begin", IntrospectValue::Int(4));
            use_text_edit_state(EDIT_TF_TAG).set_text("not a number".to_owned());
            commit_edit(true);
            let intro = grid_intro(&scene);
            assert_eq!(
                intro.query("value.4"),
                Ok(IntrospectValue::Int(3)),
                "kept prior value"
            );
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
        visible()
            .iter()
            .filter_map(|r| row_value_index(&r.id))
            .collect()
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
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowDown",
                Modifiers::empty()
            ));
            assert_eq!(cursor_id().as_deref(), Some(first.as_str()));
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "End",
                Modifiers::empty()
            ));
            assert_eq!(cursor_id().as_deref(), Some(last.as_str()));
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowDown",
                Modifiers::empty()
            ));
            assert_eq!(
                cursor_id().as_deref(),
                Some(last.as_str()),
                "clamps at the bottom"
            );
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "Home",
                Modifiers::empty()
            ));
            assert_eq!(cursor_id().as_deref(), Some(first.as_str()));
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowUp",
                Modifiers::empty()
            ));
            assert_eq!(
                cursor_id().as_deref(),
                Some(first.as_str()),
                "clamps at the top"
            );
        });
    }

    #[test]
    fn r921_keyboard_collapses_and_expands_category_header() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Cursor on the Identity category branch.
            set_cursor_id("cat.Identity");
            let collapsed = || {
                !find_node(&use_property_tree().get(), "cat.Identity")
                    .unwrap()
                    .expanded
            };
            // ArrowLeft on an expanded branch collapses it; its 3 leaves vanish.
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowLeft",
                Modifiers::empty()
            ));
            assert!(collapsed(), "ArrowLeft collapses the focused category");
            assert_eq!(visible().len(), 25, "28 − 3 Identity leaves");
            // ArrowRight re-expands.
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowRight",
                Modifiers::empty()
            ));
            assert!(!collapsed());
            assert_eq!(visible().len(), 28);
            // Enter on a branch toggles it too.
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "Enter",
                Modifiers::empty()
            ));
            assert!(collapsed(), "Enter on a branch toggles collapse");
        });
    }

    #[test]
    fn r836_space_toggles_bool_enter_edits_text() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Cursor onto the Visible bool (leaf 2) and Space-toggle it.
            set_cursor_id("2");
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "Space",
                Modifiers::empty()
            ));
            assert_eq!(
                grid_intro(&scene).query("value.2"),
                Ok(IntrospectValue::Bool(false))
            );
            // Cursor onto the Name text row (leaf 0) and Enter -> edit mode.
            set_cursor_id("0");
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "Enter",
                Modifiers::empty()
            ));
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::from("0"))),
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
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(EDIT_TF_TAG),
                "Enter",
                Modifiers::empty()
            ));
            assert_eq!(
                grid_intro(&scene).query("value.0"),
                Ok(IntrospectValue::Text("Enemy".to_owned()))
            );
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
            assert!(
                PropertyGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), "9", Modifiers::empty()),
                "digit accepted"
            );
            assert!(
                !PropertyGridView::apply_key(
                    &mut scene,
                    Some(EDIT_TF_TAG),
                    "x",
                    Modifiers::empty()
                ),
                "letter dropped"
            );
            assert_eq!(use_text_edit_state(EDIT_TF_TAG).text(), "9");
        });
    }

    #[test]
    fn r836_keys_ignored_when_unfocused() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(!PropertyGridView::apply_key(
                &mut scene,
                None,
                "ArrowDown",
                Modifiers::empty()
            ));
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
            assert_eq!(
                cat.name.as_deref(),
                Some("Identity (3)"),
                "category name folds its leaf count"
            );
            // The cursor leaf is the active descendant (focus, NO aria-selected —
            // the Inspector has no selection model), level 2, value folded in.
            let active = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#2"))
                .expect("Visible leaf node");
            assert_eq!(active.level, Some(2), "a category leaf is aria-level 2");
            assert!(active.state.focused, "cursor row is the active descendant");
            assert_eq!(
                active.selected, None,
                "no selection model → no aria-selected axis"
            );
            assert_eq!(
                active.name.as_deref(),
                Some("Visible: On"),
                "leaf name folds its value"
            );
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
            let field = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#6"))
                .expect("Position X field node");
            assert_eq!(field.level, Some(3), "a struct field is aria-level 3");
            assert!(
                field
                    .name
                    .as_deref()
                    .unwrap_or("")
                    .starts_with("Position X:"),
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
    fn r1176_asset_slot_predicate() {
        // Only the Mesh leaf (slot 1) opens the file picker; scalars / other text
        // leaves keep their inline / popup editors.
        assert!(is_asset_slot(MESH_SLOT));
        assert!(!is_asset_slot(0)); // Name (inline text)
        assert!(!is_asset_slot(8)); // Opacity (ranged slider)
    }

    #[test]
    fn r1177_asset_click_intents_match_tags() {
        // Drift guard (F4): the `<tag>.click` intent consts the reducer matches
        // must track the button tags — rename a tag and this fails before the
        // button silently stops working.
        assert_eq!(ASSET_OK_CLICK, format!("{ASSET_OK_TAG}.click"));
        assert_eq!(ASSET_CANCEL_CLICK, format!("{ASSET_CANCEL_TAG}.click"));
    }

    #[test]
    fn r1176_mesh_cell_opens_picker_navigates_and_writes_path() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Boot: the Mesh leaf holds the seeded asset path; the picker is shut.
            assert_eq!(
                grid_intro(&scene).query("value.1"),
                Ok(IntrospectValue::Text("/proj/meshes/hero.fbx".to_owned())),
            );
            assert!(!asset_modal().is_open());

            // Activating the Mesh row opens the embedded picker (the Choice /
            // Colour activation shape), targeting slot 1, the browser at root.
            open_choice(&mut scene, 1);
            assert!(
                asset_modal().is_open(),
                "activating the Mesh row opens the picker"
            );
            assert_eq!(use_asset_target().get(), Some(1));
            assert_eq!(asset_directory().cwd(), "/proj");

            // Navigate into meshes/ and select a different asset.
            assert_eq!(asset_directory().navigate("meshes"), "/proj/meshes");
            assert_eq!(
                asset_directory().select("enemy.fbx").as_deref(),
                Some("/proj/meshes/enemy.fbx"),
            );

            // Confirm writes the chosen path through the value SSOT, then closes.
            confirm_asset();
            assert!(!asset_modal().is_open(), "confirm closes the picker");
            assert_eq!(use_asset_target().get(), None);
            assert_eq!(
                grid_intro(&scene).query("value.1"),
                Ok(IntrospectValue::Text("/proj/meshes/enemy.fbx".to_owned())),
                "the picked path is written into the Mesh slot",
            );
        });
    }

    #[test]
    fn r1176_cancel_leaves_the_path_untouched() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            open_choice(&mut scene, 1);
            assert!(asset_modal().is_open());
            // Select a different file, then cancel (close without confirming).
            assert_eq!(asset_directory().navigate("textures"), "/proj/textures");
            let _ = asset_directory().select("normal.png");
            asset_modal().close();
            assert!(!asset_modal().is_open());
            // Only confirm writes — a cancel leaves the Mesh value as it was.
            assert_eq!(
                grid_intro(&scene).query("value.1"),
                Ok(IntrospectValue::Text("/proj/meshes/hero.fbx".to_owned())),
                "cancel leaves the path untouched",
            );
        });
    }

    #[test]
    fn r1176_open_picker_lowers_to_an_aria_modal_dialog() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Closed: no Dialog node in the inspector a11y tree.
            assert!(
                !PropertyGridView::access_node(&PropertyGridView::read_state(&scene), None)
                    .iter()
                    .any(|n| n.role == AriaRole::Dialog),
                "no dialog node while the picker is shut",
            );
            open_choice(&mut scene, 1);
            let nodes = PropertyGridView::access_node(&PropertyGridView::read_state(&scene), None);
            assert!(
                nodes.iter().any(|n| n.role == AriaRole::Dialog),
                "the open picker lowers to an aria-modal Dialog",
            );
        });
    }

    /// R1517 §5.39 §5.40 — the dialog buttons' AT focus flag is sourced from the
    /// **shell's** focus (the `access_node` argument), not from a posture the
    /// paint path snapshotted. Both sources agree in every reachable dispatch
    /// today (measured over the wire in `r1517_at_focus_one_source.py`), so this
    /// pins the SOURCE rather than repairing an observed wrong announcement:
    /// which of the two the tree speaks was unstated, and a stop whose flag is
    /// re-derived on the paint's cadence is only fresh by an ordering
    /// coincidence the a11y builder does not control.
    #[test]
    fn r1517_dialog_button_at_focus_comes_from_the_shell_argument() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            open_choice(&mut scene, MESH_SLOT);
            let state = PropertyGridView::read_state(&scene);
            let nodes = PropertyGridView::access_node(&state, Some(ASSET_CANCEL_TAG));
            let node = |tag: &str| {
                nodes
                    .iter()
                    .find(|n| n.tag == tag)
                    .unwrap_or_else(|| panic!("{tag} is in the open picker's AT tree"))
            };
            assert!(
                node(ASSET_CANCEL_TAG).state.focused,
                "the AT tree reports the shell's focused stop",
            );
            assert!(
                !node(ASSET_OK_TAG).state.focused,
                "and only that stop carries the flag",
            );
            // The mirror direction: move the shell's focus, keep the same
            // snapshot. A flag that tracked the snapshot would not move.
            let moved = PropertyGridView::access_node(&state, Some(ASSET_OK_TAG));
            let moved_node = |tag: &str| {
                moved
                    .iter()
                    .find(|n| n.tag == tag)
                    .unwrap_or_else(|| panic!("{tag} is in the open picker's AT tree"))
            };
            assert!(moved_node(ASSET_OK_TAG).state.focused);
            assert!(!moved_node(ASSET_CANCEL_TAG).state.focused);
        });
    }

    #[test]
    fn r867_choice_boot_taxonomy() {
        Owner::new().run(|| {
            let scene = boot_scene();
            let intro = grid_intro(&scene);
            assert_eq!(
                intro.query("kind.9"),
                Ok(IntrospectValue::Text("choice".to_owned()))
            );
            assert_eq!(
                intro.query("kind.10"),
                Ok(IntrospectValue::Text("choice".to_owned()))
            );
            let Ok(IntrospectValue::Json(blend)) = intro.query("value.9") else {
                panic!("choice value is json");
            };
            assert_eq!(blend["selected"], serde_json::json!(0));
            assert_eq!(blend["label"], serde_json::json!("Normal"));
            assert_eq!(
                blend["options"],
                serde_json::json!(["Normal", "Additive", "Multiply", "Screen"]),
            );
            assert_eq!(
                intro.query("popup_cursor"),
                Ok(IntrospectValue::Null),
                "no popup at boot"
            );
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
            assert_eq!(
                intro.query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::from("9")))
            );
            assert_eq!(
                intro.query("popup_cursor"),
                Ok(IntrospectValue::Int(0)),
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
            assert_eq!(
                grid_intro(&scene).query("popup_cursor"),
                Ok(IntrospectValue::Int(3))
            );
            // Enter commits the cursor (Screen) and closes the popup.
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "Enter",
                Modifiers::empty()
            ));
            let Ok(IntrospectValue::Json(v)) = grid_intro(&scene).query("value.9") else {
                panic!("json");
            };
            assert_eq!(v["label"], serde_json::json!("Screen"));
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null))
            );
        });
    }

    #[test]
    fn r867_escape_dismisses_without_commit() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            open_choice(&mut scene, BLEND_ROW);
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowDown",
                Modifiers::empty()
            ));
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "Escape",
                Modifiers::empty()
            ));
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null))
            );
            let Ok(IntrospectValue::Json(v)) = grid_intro(&scene).query("value.9") else {
                panic!("json");
            };
            assert_eq!(
                v["selected"],
                serde_json::json!(0),
                "Escape leaves the committed value"
            );
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
            assert_eq!(
                intro.query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::from("9")))
            );
            let _ = intro.invoke("send", IntrospectValue::Text("opt2:PointerUp".to_owned()));
            assert_eq!(
                intro.query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null))
            );
            let Ok(IntrospectValue::Json(v)) = intro.query("value.9") else {
                panic!("json")
            };
            assert_eq!(v["label"], serde_json::json!("Multiply"));
            // The RPC `choose` path commits + closes too (Body row 10 -> None).
            let _ = intro.invoke("begin", IntrospectValue::Int(10));
            assert_eq!(
                intro.invoke("choose", IntrospectValue::Int(0)),
                Ok(IntrospectValue::Bool(true))
            );
            let Ok(IntrospectValue::Json(v)) = intro.query("value.10") else {
                panic!("json")
            };
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
            let _ = intro.invoke(
                "send",
                IntrospectValue::Text("dismiss:PointerUp".to_owned()),
            );
            assert_eq!(
                intro.query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null))
            );
            // Direct AI set by index (no popup needed) + strict errors.
            assert!(intro.intervene("value.9", IntrospectValue::Int(1)).is_ok());
            let Ok(IntrospectValue::Json(v)) = intro.query("value.9") else {
                panic!("json")
            };
            assert_eq!(v["label"], serde_json::json!("Additive"));
            assert_out_of_range_saying(
                &intro.intervene("value.9", IntrospectValue::Int(9)),
                "no option 9 on this cell",
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
            let closed = view(idle_state(), &Frame::new());
            assert!(
                !closed.contains_tag(CHOICE_POPUP_TAG),
                "no panel when closed"
            );
            let closed_nodes = PropertyGridView::access_node(&idle_state(), Some(GRID_TAG));
            assert!(
                !closed_nodes
                    .iter()
                    .any(|n| n.role == pinion_a11y::AriaRole::Listbox),
                "no listbox when closed",
            );
            // Open the Blend popup.
            open_choice(&mut scene, BLEND_ROW);
            let open = view(idle_state(), &Frame::new());
            assert!(
                open.contains_tag(CHOICE_POPUP_TAG),
                "panel painted when open"
            );
            assert!(
                open.contains_tag(POPUP_DISMISS_TAG),
                "dismiss barrier painted"
            );
            assert!(
                open.contains_tag(&format!("{GRID_TAG}#opt0")),
                "option 0 painted"
            );
            assert!(
                open.contains_tag(&format!("{GRID_TAG}#opt3")),
                "option 3 painted"
            );
            let nodes = PropertyGridView::access_node(&idle_state(), Some(GRID_TAG));
            let listbox = nodes
                .iter()
                .find(|n| n.role == pinion_a11y::AriaRole::Listbox)
                .expect("listbox node when open");
            assert_eq!(listbox.name.as_deref(), Some("Blend options"));
            let options: Vec<_> = nodes
                .iter()
                .filter(|n| n.role == pinion_a11y::AriaRole::ListBoxOption)
                .collect();
            assert_eq!(options.len(), 4, "one option node per choice");
            assert_eq!(
                options[0].selected,
                Some(true),
                "option 0 is aria-selected (Normal)"
            );
            assert!(
                options[0].state.focused,
                "cursor 0 is the active descendant"
            );
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
            assert_eq!(
                intro.query("kind.11"),
                Ok(IntrospectValue::Text("color".to_owned()))
            );
            let Ok(IntrospectValue::Json(tint)) = intro.query("value.11") else {
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
            assert_eq!(
                intro.query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::from("11")))
            );
            assert_eq!(
                intro.query("popup_cursor"),
                Ok(IntrospectValue::Int(4)),
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
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowRight",
                Modifiers::empty()
            ));
            assert_eq!(
                grid_intro(&scene).query("popup_cursor"),
                Ok(IntrospectValue::Int(5))
            );
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "Enter",
                Modifiers::empty()
            ));
            let Ok(IntrospectValue::Json(v)) = grid_intro(&scene).query("value.11") else {
                panic!("json");
            };
            assert_eq!(v["hex"], serde_json::json!("#fdd835"), "committed Yellow");
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null))
            );
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
            assert_eq!(
                intro.query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::from("11")))
            );
            let _ = intro.invoke("send", IntrospectValue::Text("sw2:PointerUp".to_owned()));
            let Ok(IntrospectValue::Json(v)) = intro.query("value.11") else {
                panic!("json")
            };
            assert_eq!(v["hex"], serde_json::json!("#e53935"), "clicked Red");
            assert_eq!(
                intro.query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null))
            );
            // The RPC pick_color path commits + closes too.
            let _ = intro.invoke("begin", IntrospectValue::Int(11));
            assert_eq!(
                intro.invoke("pick_color", IntrospectValue::Int(0)),
                Ok(IntrospectValue::Bool(true))
            );
            let Ok(IntrospectValue::Json(v)) = intro.query("value.11") else {
                panic!("json")
            };
            assert_eq!(
                v["hex"],
                serde_json::json!("#ffffff"),
                "pick_color 0 -> White"
            );
            // intervene sets an arbitrary colour by hex (the AI-first path).
            assert!(
                intro
                    .intervene("value.11", IntrospectValue::Text("#abcdef".to_owned()))
                    .is_ok()
            );
            let Ok(IntrospectValue::Json(v)) = intro.query("value.11") else {
                panic!("json")
            };
            assert_eq!(v["hex"], serde_json::json!("#abcdef"));
            assert_out_of_range_saying(
                &intro.intervene("value.11", IntrospectValue::Text("nope".to_owned())),
                r#""nope" is not a colour"#,
            );
        });
    }

    #[test]
    fn r869_view_and_a11y_expose_the_open_color_popup() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let closed = view(idle_state(), &Frame::new());
            assert!(
                !closed.contains_tag(COLOR_POPUP_TAG),
                "no colour panel when closed"
            );
            open_choice(&mut scene, TINT_ROW);
            let open = view(idle_state(), &Frame::new());
            assert!(
                open.contains_tag(COLOR_POPUP_TAG),
                "colour panel painted when open"
            );
            assert!(
                open.contains_tag(POPUP_DISMISS_TAG),
                "dismiss barrier painted"
            );
            assert!(
                open.contains_tag(&format!("{GRID_TAG}#sw0")),
                "swatch 0 painted"
            );
            assert!(
                open.contains_tag(&format!("{GRID_TAG}#sw7")),
                "swatch 7 painted"
            );
            let nodes = PropertyGridView::access_node(&idle_state(), Some(GRID_TAG));
            let listbox = nodes
                .iter()
                .find(|n| n.role == pinion_a11y::AriaRole::Listbox)
                .expect("colour listbox node when open");
            assert_eq!(listbox.name.as_deref(), Some("Tint swatches"));
            let swatches: Vec<_> = nodes
                .iter()
                .filter(|n| n.role == pinion_a11y::AriaRole::ListBoxOption)
                .collect();
            assert_eq!(
                swatches.len(),
                COLOR_SWATCHES.len(),
                "one option node per swatch"
            );
            // Blue (index 4) is the committed selection + the cursor.
            assert_eq!(
                swatches[4].selected,
                Some(true),
                "Blue swatch is aria-selected"
            );
            assert!(
                swatches[4].state.focused,
                "Blue swatch is the active descendant"
            );
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
            assert_eq!(
                f.active_descendant.as_deref(),
                Some(format!("{GRID_TAG}#2").as_str())
            );
            // Cursor on a category branch rings the branch's composite tag.
            set_cursor_id("cat.Identity");
            let h = PropertyGridView::access_focus_target(&idle_state(), Some(GRID_TAG))
                .expect("composite");
            assert_eq!(
                h.active_descendant.as_deref(),
                Some(format!("{GRID_TAG}#cat.Identity").as_str())
            );
            // Choice popup open -> the active option (Blend cursor boots 0).
            open_choice(&mut scene, BLEND_ROW);
            let f = PropertyGridView::access_focus_target(&idle_state(), Some(GRID_TAG))
                .expect("composite");
            assert_eq!(
                f.active_descendant.as_deref(),
                Some(format!("{GRID_TAG}#opt0").as_str())
            );
            // Colour popup open -> the active swatch (Tint boots Blue=4).
            open_choice(&mut scene, TINT_ROW);
            let f = PropertyGridView::access_focus_target(&idle_state(), Some(GRID_TAG))
                .expect("composite");
            assert_eq!(
                f.active_descendant.as_deref(),
                Some(format!("{GRID_TAG}#sw4").as_str())
            );
            // Focus elsewhere -> atomic, no active descendant.
            let other = PropertyGridView::access_focus_target(&idle_state(), Some(EDIT_TF_TAG));
            assert!(other.expect("atomic").active_descendant.is_none());
        });
    }

    #[test]
    fn r870_hex_field_commits_an_arbitrary_colour() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            open_choice(&mut scene, TINT_ROW); // opens the colour popup, seeds the hex field
            assert_eq!(
                use_text_edit_state(EDIT_TF_TAG).text(),
                "#1e88e5",
                "hex field seeded Blue"
            );
            // The popup paints the hex field (the shared EDIT_TF).
            let painted = view(idle_state(), &Frame::new());
            assert!(
                painted.contains_tag(EDIT_TF_TAG),
                "hex field painted in the colour popup"
            );
            // Type an arbitrary hex + Enter (the EDIT_TF-focused commit path).
            use_text_edit_state(EDIT_TF_TAG).set_text("#abcdef".to_owned());
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(EDIT_TF_TAG),
                "Enter",
                Modifiers::empty(),
            ));
            let Ok(IntrospectValue::Json(v)) = grid_intro(&scene).query("value.11") else {
                panic!("json");
            };
            assert_eq!(
                v["hex"],
                serde_json::json!("#abcdef"),
                "hex field commits arbitrary colour"
            );
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null)),
                "popup closed after hex commit",
            );
        });
    }

    // ----- view -----

    #[test]
    fn r836_view_carries_grid_and_row_tags() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let scene = view(idle_state(), &Frame::new());
            assert!(scene.contains_tag(GRID_TAG), "grid root painted");
            assert!(
                scene.contains_tag(&format!("{GRID_TAG}#0")),
                "leaf row 0 painted"
            );
            assert!(
                scene.contains_tag(&format!("{GRID_TAG}#8")),
                "leaf row 8 painted"
            );
            assert!(
                scene.contains_tag(&format!("{GRID_TAG}#cat.Identity")),
                "Identity header painted"
            );
            assert!(
                scene.contains_tag(&format!("{GRID_TAG}#cat.Transform")),
                "Transform header painted"
            );
            assert!(
                scene.contains_tag(&format!("{GRID_TAG}#struct.Position")),
                "Position struct header painted"
            );
            assert!(
                scene.contains_tag(&format!("{GRID_TAG}#12")),
                "Position Z field row painted"
            );
        });
    }

    #[test]
    fn r921_collapse_hides_branch_rows_in_view() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let before = view(idle_state(), &Frame::new());
            assert!(
                before.contains_tag(&format!("{GRID_TAG}#0")),
                "Name row painted when expanded"
            );
            assert!(
                before.contains_tag(&format!("{GRID_TAG}#cat.Identity")),
                "Identity header painted"
            );
            assert!(
                before.contains_tag(&format!("{GRID_TAG}#6")),
                "Position X painted when struct expanded"
            );
            // Collapse Identity: its leaves (Name/Tag/Layer) vanish, header stays.
            set_expanded_in(&use_property_tree(), "cat.Identity", false);
            // Collapse the Position struct: its X/Y/Z fields vanish, header stays.
            set_expanded_in(&use_property_tree(), "struct.Position", false);
            let after = view(idle_state(), &Frame::new());
            assert!(
                after.contains_tag(&format!("{GRID_TAG}#cat.Identity")),
                "category header stays on collapse"
            );
            assert!(
                !after.contains_tag(&format!("{GRID_TAG}#0")),
                "Name row hidden when collapsed"
            );
            assert!(
                !after.contains_tag(&format!("{GRID_TAG}#1")),
                "Tag row hidden when collapsed"
            );
            assert!(
                after.contains_tag(&format!("{GRID_TAG}#struct.Position")),
                "struct header stays on collapse"
            );
            assert!(
                !after.contains_tag(&format!("{GRID_TAG}#6")),
                "Position X hidden when struct collapsed"
            );
        });
    }

    #[test]
    fn r836_view_paints_inline_field_only_while_editing() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let before = view(idle_state(), &Frame::new());
            assert!(
                !before.contains_tag(EDIT_TF_TAG),
                "no inline field when not editing"
            );
            use_editing_row().set(Some(ValueRef::Scalar(0)));
            let during = view(idle_state(), &Frame::new());
            assert!(
                during.contains_tag(EDIT_TF_TAG),
                "inline field painted in the editing row"
            );
        });
    }

    #[test]
    fn r836_view_contains_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<PropertyGridView>(
            idle_state(),
            &Frame::default(),
        );
    }

    // ----- R872 live search / filter -----

    #[test]
    fn r872_search_filters_rows_and_clears() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            assert_eq!(
                visible().len(),
                28,
                "6 cats + 2 structs + 1 array + 19 leaves, empty query"
            );
            // "pos" matches the qualified Position X/Y/Z field names; the
            // recursive filter reveals the Transform > Position path.
            use_text_edit_state(SEARCH_TF_TAG).set_text("pos".to_owned());
            assert_eq!(
                visible_ids(),
                vec!["cat.Transform", "struct.Position", "6", "7", "12"],
                "path-to-match reveals the matching fields inside their struct",
            );
            assert_eq!(
                visible_leaf_indices(),
                vec![6, 7, 12],
                "only the Position fields match"
            );
            // Clearing restores every row (the filter recomputes reactively).
            use_text_edit_state(SEARCH_TF_TAG).set_text(String::new());
            assert_eq!(visible().len(), 28, "cleared query restores every row");
        });
    }

    #[test]
    fn r872_filter_drops_unmatched_branches() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            // "name" matches only Name (leaf 0, under Identity) — every other
            // branch is pruned (no match anywhere on its path).
            use_text_edit_state(SEARCH_TF_TAG).set_text("name".to_owned());
            assert_eq!(
                visible_ids(),
                vec!["cat.Identity", "0"],
                "only Identity > Name survives"
            );
            assert_eq!(visible_leaf_indices(), vec![0]);
        });
    }

    #[test]
    fn r872_search_field_painted_and_escape_clears() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let painted = view(idle_state(), &Frame::new());
            assert!(
                painted.contains_tag(SEARCH_TF_TAG),
                "search box painted in the title band"
            );
            // Escape on the focused search box clears the live filter.
            use_text_edit_state(SEARCH_TF_TAG).set_text("xyz".to_owned());
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(SEARCH_TF_TAG),
                "Escape",
                Modifiers::empty(),
            ));
            assert_eq!(
                use_text_edit_state(SEARCH_TF_TAG).text(),
                "",
                "Escape clears the query"
            );
        });
    }

    // ----- R873 audit remediation (a11y) -----

    /// A neutral RootState (both text fields + both dialog buttons idle) for
    /// view / a11y assertions. Focus is not part of it — R1517 made the shell's
    /// `focused` argument the AT tree's only focus source.
    fn idle_state() -> RootState {
        (
            (TextFieldState::Idle, 0),
            (TextFieldState::Idle, 0),
            (ButtonState::Idle, ButtonState::Idle),
        )
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
            assert_eq!(
                search.role,
                pinion_a11y::AriaRole::TextInput,
                "searchbox -> textbox role"
            );
            assert_eq!(
                search.name.as_deref(),
                Some("Filter properties"),
                "accessible name"
            );
            assert!(
                search.state.focused,
                "the focused search box is announced focused"
            );
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
            assert_eq!(
                name.as_deref(),
                Some("19 properties"),
                "16 scalar + 3 array element leaves, no filter"
            );
            // Narrowing the filter updates the announced count.
            use_text_edit_state(SEARCH_TF_TAG).set_text("pos".to_owned());
            assert_eq!(
                status_count().1.as_deref(),
                Some("3 properties"),
                "filtered to Position X/Y/Z"
            );
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
                nodes
                    .iter()
                    .any(|n| n.role == pinion_a11y::AriaRole::Listbox),
                "popup listbox emitted while its row is visible",
            );
            let f = PropertyGridView::access_focus_target(&idle_state(), Some(GRID_TAG))
                .expect("composite focus");
            assert!(
                f.active_descendant
                    .as_deref()
                    .is_some_and(|d| d.contains(CHOICE_OPT_PREFIX)),
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
                !nodes
                    .iter()
                    .any(|n| n.role == pinion_a11y::AriaRole::Listbox),
                "no listbox a11y when the popup's row is filtered out",
            );
            let f = PropertyGridView::access_focus_target(&idle_state(), Some(GRID_TAG))
                .expect("focus target");
            assert!(
                !f.active_descendant
                    .as_deref()
                    .unwrap_or("")
                    .contains(CHOICE_OPT_PREFIX),
                "active descendant no longer points at the unpainted popup",
            );
        });
    }

    // ─── R931 array / Vec property editing ────────────────────────────

    /// R931 — the array branch + its element leaves appear in the visible
    /// flatten, addressed by `elem.<k>` ids; `elem_count` reports the sub-model
    /// length.
    #[test]
    fn r931_array_branch_and_elements_in_flatten() {
        Owner::new().run(|| {
            let scene = boot_scene();
            let ids = visible_ids();
            assert!(
                ids.iter().any(|id| id == ARR_BRANCH_ID),
                "array branch row present"
            );
            assert!(
                ids.iter().any(|id| id == "elem.0"),
                "element 0 leaf present"
            );
            assert!(
                ids.iter().any(|id| id == "elem.2"),
                "element 2 leaf present"
            );
            assert_eq!(
                grid_intro(&scene).query("elem_count"),
                Ok(IntrospectValue::Int(3))
            );
        });
    }

    /// R931 — an element value reads / writes through the same `value.<addr>`
    /// wire as a scalar, with the `elem.<k>` address (read/write symmetry).
    #[test]
    fn r931_element_value_read_write_via_rpc() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert_eq!(
                grid_intro(&scene).query("value.elem.1"),
                Ok(IntrospectValue::Float(0.5))
            );
            with_grid_mut(&mut scene, |i| {
                i.intervene("value.elem.1", IntrospectValue::Float(2.0))
            })
            .expect("set element value");
            assert_eq!(
                grid_intro(&scene).query("value.elem.1"),
                Ok(IntrospectValue::Float(2.0))
            );
            // An out-of-range element address reads None (like a scalar OOB).
            assert!(matches!(
                grid_intro(&scene).query("value.elem.9"),
                Err(ReadRefusal::NoSuchMember(_))
            ));
        });
    }

    /// R931 — `name.<addr>` / `kind.<addr>` answer for an element too: the
    /// qualified "Spawn Weights [k]" name and the homogeneous `float` kind.
    #[test]
    fn r931_element_name_and_kind_rpc() {
        Owner::new().run(|| {
            let scene = boot_scene();
            assert_eq!(
                grid_intro(&scene).query("name.elem.0"),
                Ok(IntrospectValue::Text("Spawn Weights [0]".to_owned()))
            );
            assert_eq!(
                grid_intro(&scene).query("kind.elem.0"),
                Ok(IntrospectValue::Text("float".to_owned()))
            );
        });
    }

    /// R931 — `add_elem` appends a `0.0` element and returns its index; the count
    /// grows. `remove_elem` shrinks it and shifts later elements down (array
    /// semantics). An out-of-range remove is a no-op `false`.
    #[test]
    fn r931_add_and_remove_elements() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let idx =
                with_grid_mut(&mut scene, |i| i.invoke("add_elem", IntrospectValue::Null)).unwrap();
            assert_eq!(idx, IntrospectValue::Int(3), "new element index");
            assert_eq!(
                grid_intro(&scene).query("elem_count"),
                Ok(IntrospectValue::Int(4))
            );
            assert_eq!(
                grid_intro(&scene).query("value.elem.3"),
                Ok(IntrospectValue::Float(0.0)),
                "a new element seeds 0.0"
            );
            let ok = with_grid_mut(&mut scene, |i| {
                i.invoke("remove_elem", IntrospectValue::Int(0))
            })
            .unwrap();
            assert_eq!(ok, IntrospectValue::Bool(true));
            assert_eq!(
                grid_intro(&scene).query("elem_count"),
                Ok(IntrospectValue::Int(3))
            );
            assert_eq!(
                grid_intro(&scene).query("value.elem.0"),
                Ok(IntrospectValue::Float(0.5)),
                "elements shift down on remove"
            );
            assert_eq!(
                with_grid_mut(&mut scene, |i| i
                    .invoke("remove_elem", IntrospectValue::Int(9)))
                .unwrap(),
                IntrospectValue::Bool(false),
                "out-of-range remove is a no-op"
            );
        });
    }

    /// R931 — `move_elem "from,to"` reorders the sub-model (a Vec remove+insert);
    /// `from == to` is a no-op and a malformed payload is rejected.
    #[test]
    fn r931_move_element_reorders() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // [1.0, 0.5, 0.25] — move 0 → 2 — [0.5, 0.25, 1.0].
            let ok = with_grid_mut(&mut scene, |i| {
                i.invoke("move_elem", IntrospectValue::Text("0,2".to_owned()))
            })
            .unwrap();
            assert_eq!(ok, IntrospectValue::Bool(true));
            assert_eq!(
                grid_intro(&scene).query("value.elem.0"),
                Ok(IntrospectValue::Float(0.5))
            );
            assert_eq!(
                grid_intro(&scene).query("value.elem.2"),
                Ok(IntrospectValue::Float(1.0))
            );
            assert_eq!(
                with_grid_mut(&mut scene, |i| i
                    .invoke("move_elem", IntrospectValue::Text("1,1".to_owned())))
                .unwrap(),
                IntrospectValue::Bool(false),
                "from == to is a no-op"
            );
            assert!(
                with_grid_mut(&mut scene, |i| i
                    .invoke("move_elem", IntrospectValue::Text("oops".to_owned())))
                .is_err(),
                "malformed payload rejected"
            );
        });
    }

    /// R931 / R930.1 — removing an element cancels an in-flight edit on the array
    /// (every element at / after the removal shifts, so even a survivor edit is
    /// stale): the latch clears, so `editing` reports Null.
    #[test]
    fn r931_remove_cancels_inflight_element_edit() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            with_grid_mut(&mut scene, |i| {
                i.invoke("begin", IntrospectValue::Text("elem.1".to_owned()))
            })
            .unwrap();
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::from("elem.1"))),
                "editing the element"
            );
            with_grid_mut(&mut scene, |i| {
                i.invoke("remove_elem", IntrospectValue::Int(0))
            })
            .unwrap();
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::Null)),
                "the in-flight element edit is cancelled on remove"
            );
        });
    }

    /// R931 / R930.1 — removing the element under the cursor re-anchors it onto
    /// the element that took its slot (it never strands on a vanished id). A
    /// cursor on `elem.2` after removing `elem.0` follows the element to `elem.1`.
    #[test]
    fn r931_remove_reanchors_cursor() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            use_property_cursor().set(Some("elem.2".to_owned()));
            with_grid_mut(&mut scene, |i| {
                i.invoke("remove_elem", IntrospectValue::Int(0))
            })
            .unwrap();
            assert_eq!(
                use_property_cursor().get().as_deref(),
                Some("elem.1"),
                "cursor re-anchored onto the surviving element"
            );
        });
    }

    /// R931 — a scalar leaf's `value.<i>` address is permanent across array add /
    /// remove (the whole point of the separate array sub-model), and the flat
    /// model length never changes.
    #[test]
    fn r931_scalar_addresses_stable_across_array_ops() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let before = grid_intro(&scene).query("value.6");
            with_grid_mut(&mut scene, |i| i.invoke("add_elem", IntrospectValue::Null)).unwrap();
            with_grid_mut(&mut scene, |i| {
                i.invoke("remove_elem", IntrospectValue::Int(0))
            })
            .unwrap();
            assert_eq!(
                grid_intro(&scene).query("value.6"),
                before,
                "value.6 unchanged by array mutation"
            );
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Ok(IntrospectValue::Int(16)),
                "the flat model length is permanent"
            );
        });
    }

    /// R931 — the view paints each element row, its remove button, and the array
    /// branch's add button.
    #[test]
    fn r931_view_paints_element_rows_and_buttons() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            let scene = view(idle_state(), &Frame::new());
            assert!(
                scene.contains_tag(&format!("{GRID_TAG}#elem.0")),
                "element row painted"
            );
            assert!(
                scene.contains_tag(&format!("{GRID_TAG}#{ADD_ELEM_TAG}")),
                "add button painted"
            );
            assert!(
                scene.contains_tag(&format!("{GRID_TAG}#{RM_ELEM_PREFIX}0")),
                "remove button painted"
            );
        });
    }

    /// R931 — a11y: the array branch gains an "Add" button child, each element
    /// row a "Remove" button child, and an element treeitem folds its value into
    /// its accessible name.
    #[test]
    fn r931_a11y_add_remove_buttons() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            let nodes = PropertyGridView::access_node(&idle_state(), Some(GRID_TAG));
            let add = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#{ADD_ELEM_TAG}"))
                .expect("add button node");
            assert_eq!(add.role, AriaRole::Button);
            assert_eq!(add.name.as_deref(), Some("Add Spawn Weights element"));
            let rm = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#{RM_ELEM_PREFIX}0"))
                .expect("remove button node");
            assert_eq!(rm.name.as_deref(), Some("Remove Spawn Weights [0]"));
            let elem = nodes
                .iter()
                .find(|n| n.tag == tree_row_tag(GRID_TAG, "elem.0"))
                .expect("element treeitem");
            assert!(
                elem.name
                    .as_deref()
                    .unwrap_or("")
                    .contains("Spawn Weights [0]: 1"),
                "the element name folds in its value"
            );
        });
    }

    /// R931 — keyboard: Enter on an element cursor begins its edit; Delete on an
    /// element cursor removes it (the grow / shrink twin of the +/− buttons,
    /// which are not tab stops).
    #[test]
    fn r931_keyboard_edit_and_delete_element() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            use_property_cursor().set(Some("elem.0".to_owned()));
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "Enter",
                Modifiers::empty()
            ));
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::from("elem.0"))),
                "Enter begins the element edit"
            );
            // Reset the latch, then Delete on the element cursor removes it.
            use_editing_row().set(None);
            use_text_edit_state(EDIT_TF_TAG).set_text(String::new());
            use_property_cursor().set(Some("elem.0".to_owned()));
            assert!(PropertyGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "Delete",
                Modifiers::empty()
            ));
            assert_eq!(
                grid_intro(&scene).query("elem_count"),
                Ok(IntrospectValue::Int(2)),
                "Delete removed the element"
            );
        });
    }

    /// R931 (session-review fix) — removing an element BEFORE the one being
    /// edited must NOT cancel the edit: `elem.0`'s edit survives a removal of
    /// `elem.2` (only `k >= index` is disturbed, matching the cursor reanchor).
    #[test]
    fn r931_remove_later_element_keeps_earlier_edit() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            with_grid_mut(&mut scene, |i| {
                i.invoke("begin", IntrospectValue::Text("elem.0".to_owned()))
            })
            .unwrap();
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::from("elem.0"))),
                "editing elem.0"
            );
            // Remove a LATER element (elem.2) — elem.0 is unaffected, so the edit
            // continues (over-cancelling an earlier edit would be a real smell).
            with_grid_mut(&mut scene, |i| {
                i.invoke("remove_elem", IntrospectValue::Int(2))
            })
            .unwrap();
            assert_eq!(
                grid_intro(&scene).query("editing"),
                Ok(IntrospectValue::Json(serde_json::Value::from("elem.0"))),
                "an edit before the removal survives"
            );
        });
    }

    /// R931 (session-review fix) — an array element is discoverable by the
    /// array's name in the search filter (no scalar-field-vs-element asymmetry):
    /// "weights" reveals the element rows, not just the array branch.
    #[test]
    fn r931_search_reveals_elements_by_array_label() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            use_text_edit_state(SEARCH_TF_TAG).set_text("weights".to_owned());
            let ids = visible_ids();
            assert!(
                ids.iter().any(|id| id == ARR_BRANCH_ID),
                "the array branch matches 'weights'"
            );
            assert!(
                ids.iter().any(|id| id == "elem.0"),
                "each element matches the array label 'weights'"
            );
        });
    }
}
