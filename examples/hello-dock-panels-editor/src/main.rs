// R685 §5.16 — example bindings tolerate looser doc-markdown lints
// than the substrate crates (same waiver hello-dock-panels carries).
#![allow(clippy::doc_markdown)]

//! `hello-dock-panels-editor` — R685 §5.16 §5.41 §5.49 **2nd dock
//! consumer** triggering the [[abstraction-needs-second-consumer]]
//! Rule-of-Three substrate lift gate.
//!
//! Five retained dock panels composed into the canonical pro-tool
//! authoring editor layout every DCC / IDE / 3D-suite shell ships:
//!
//! ```text
//!  ┌──────────────────────────────────────────────────────┐
//!  │                       Toolbar                        │ Top
//!  ├──────────┬────────────────────────────┬──────────────┤
//!  │          │                            │              │
//!  │ Outliner │          Viewport          │  Properties  │
//!  │   (Left) │           (Center)         │     (Right)  │
//!  │          │                            │              │
//!  ├──────────┴────────────────────────────┴──────────────┤
//!  │                       Console                        │ Bottom
//!  └──────────────────────────────────────────────────────┘
//! ```
//!
//! The topology is declared at boot time via
//! [`pinion_widget_paint::dock::DockTopology`] — a recursive binary
//! split tree built from `DockNode::split_vertical` /
//! `DockNode::split_horizontal` / `DockNode::leaf`. The R685 atomic 1
//! walker [`view_dock_surface`] lowers the topology into a nested
//! Splitter + DockPanel scene each paint, threading per-Split
//! [`Signal<f32>`] ratios + per-panel [`Scene`] content through
//! binding-supplied closures.
//!
//! ## Substrate Rule-of-Three evidence (R685 lifts)
//!
//! Lifted from `hello-dock-panels` (R683.C, 1st consumer) into
//! [`pinion_widget_paint::dock`] in the R685 round:
//!
//! * `view_floating_placeholder(panel_id, theme, style)` — paint
//!   "(panel torn off)" placeholder for a dock slot whose panel is
//!   currently floating.
//! * `floating_window_id(prefix, panel_id)` + `DEFAULT_FLOATING_WINDOW_PREFIX` —
//!   `"torn-{panel_id}"` convention.

use pinion_a11y::{AccessNode, WidgetA11y};
use pinion_core::command::Command;
use pinion_core::external::OUTER_DOCK_ZONE_TAG;
use pinion_core::external::{DropPoint, IntrospectValue};
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{AlignItems, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::undo::{UndoStack, UndoStackExternal, use_undo_stack};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Frame, Owner, Scene, Signal, WidgetCore};
use pinion_shell::{
    SizeStrategy, WINDOW_CHROME_CLOSE_TAG, WINDOW_CHROME_MAXIMIZE_TAG, WINDOW_CHROME_MINIMIZE_TAG,
    WidgetView, WindowSpec, desktop_position_from, vello_renderer_impl, window_exists,
};
use pinion_widget_paint::button::{ButtonColors, ButtonStyle, view_button};
use pinion_widget_paint::dock::{
    DEFAULT_FLOATING_WINDOW_PREFIX, DockDropPreview, DockNode, DockPanelExternal, DockPanelStyle,
    DockReorganizeExternal, DockReorganizer, DockSplitState, DockTopology, DropResolution,
    FloatPolicy, FloatingPlaceholderStyle, TEAR_OFF_EVENT, TEAR_OFF_FOLLOW_EVENT,
    TEAR_OFF_REDOCK_AT_EVENT, TEAR_OFF_REDOCK_EVENT, TabWellExternal, WINDOW_MOVE_EVENT,
    WindowControlTags, dock_drop_preview_overlay, dock_outer_preview_overlay,
    dock_outer_zone_highlight, dock_redock_preview_tint, dock_tablist_access_nodes,
    floating_window_id as dock_floating_window_id, resolve_drop, view_dock_panel_with_actions,
    view_dock_surface_styled, view_floating_placeholder, view_window_controls,
};
use pinion_widget_paint::splitter::SplitterExternal;
use std::borrow::Cow;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(
    HelloDockPanelsEditorRenderer,
    HelloDockPanelsEditorRendererError
);

// ─── window dimensions ────────────────────────────────────────────────

const MAIN_W: u32 = 1200;
const MAIN_H: u32 = 800;

/// (R1105 §5.51 §5.16 PR-31) The slot-bearing main window id — the only
/// window whose dock topology can host a panel (a torn-off floater is a
/// single-panel `WindowSpec` with no slot to receive another). The shell's
/// single-window convention is `"main"`; the editor declares it explicitly
/// once it opts into the reactive `windows_signal`.
const MAIN_WINDOW_ID: &str = "main";

/// (R1105 §5.51 §5.16 PR-31) A torn-off panel's floating window size. Larger
/// than the flat `hello-dock-panels` floaters (360²) — an editor panel
/// (outliner tree / properties grid / viewport) wants more room than the
/// flat demo's inspector slice.
const FLOATING_W: u32 = 460;
const FLOATING_H: u32 = 380;

// ─── theme + paint-side tags ──────────────────────────────────────────

const THEME_TAG: &str = "app";

const TOOLBAR_PANEL_TAG: &str = "toolbar";
const OUTLINER_PANEL_TAG: &str = "outliner";
const VIEWPORT_PANEL_TAG: &str = "viewport";
const PROPERTIES_PANEL_TAG: &str = "properties";
const CONSOLE_PANEL_TAG: &str = "console";

/// Viewport Button paint-side tag — the primary [`ButtonExternal`] of
/// the editor binding. Routes pointer events to the primary external.
const VIEWPORT_BTN_TAG: &str = "viewport_btn";

/// (R1135 §5.51.1) Toolbar float-policy toggle button tag — the HUMAN GUI
/// path for the R1134 collapse|placeholder selection (R1134 added the
/// AI/RPC `set_float_policy` invoke; this lets a person flip it by
/// clicking). A second [`ButtonExternal`] registered as an extra external;
/// its click toggles the shared [`DockReorganizer`]'s [`FloatPolicy`].
const POLICY_BTN_TAG: &str = "float_policy_btn";

/// (R1145 §5.51) The "Undock tab" toolbar button's paint tag — shown only while
/// some panels are tabbed; its click undocks the active tab back into a split
/// (the human path for `DockReorganizer::undock_active_tab`; the AI path is the
/// `undock_tab` invoke). A third [`ButtonExternal`] registered as an extra.
const UNDOCK_BTN_TAG: &str = "undock_tab_btn";

/// Splitter paint-side tags. Four splits in depth-first pre-order
/// per R685 [`view_dock_surface`]'s threading scheme:
/// idx 0 → outer V (toolbar | rest), idx 1 → inner V (middle | console),
/// idx 2 → middle H (outliner | rest), idx 3 → inner H (viewport | properties).
const SPLIT_OUTER_TAG: &str = "editor_split_outer";
const SPLIT_INNER_V_TAG: &str = "editor_split_inner_v";
const SPLIT_MIDDLE_H_TAG: &str = "editor_split_middle_h";
const SPLIT_INNER_H_TAG: &str = "editor_split_inner_h";

/// (R686 §5.16 §5.45) Extra-external tag the [`DockReorganizeExternal`]
/// registers under. AI clients drive drag-to-reorganize through
/// `scene/invoke /dock_reorganize/external/reorganize` (the §2 #2
/// RPC-as-primary-path); the external mutates the shared topology
/// `Signal`, and the view fn's reactive subscription re-renders.
const DOCK_REORGANIZE_TAG: &str = "dock_reorganize";
/// (R749 §5.52) The [`UndoStackExternal`] anchor — the editor's reorganize
/// workspace history (Ctrl+Z over panel moves). AI clients drive it via
/// `scene/invoke /dock_undo_stack/external/undo` (and `redo`).
const DOCK_UNDO_TAG: &str = "dock_undo_stack";
/// `use_undo_stack` key for the shared reorganize history.
const DOCK_UNDO_KEY: &str = "editor_dock_undo";

// ─── intent tag constants ─────────────────────────────────────────────

const VIEWPORT_BTN_CLICK_INTENT_TAG: &str = intent_tag!("viewport_btn", "click");
/// (R1135) The float-policy toggle button's click intent (`float_policy_btn.click`).
const POLICY_BTN_CLICK_INTENT_TAG: &str = intent_tag!("float_policy_btn", "click");
/// (R1145) The undock-tab button's click intent (`undock_tab_btn.click`).
const UNDOCK_BTN_CLICK_INTENT_TAG: &str = intent_tag!("undock_tab_btn", "click");

// ─── default split ratios ─────────────────────────────────────────────

const SPLIT_OUTER_RATIO_DEFAULT: f32 = 0.06;
const SPLIT_INNER_V_RATIO_DEFAULT: f32 = 0.78;
const SPLIT_MIDDLE_H_RATIO_DEFAULT: f32 = 0.18;
const SPLIT_INNER_H_RATIO_DEFAULT: f32 = 0.78;

// ─── panel content text ───────────────────────────────────────────────

const TOOLBAR_LABEL: &str = "File   Edit   View   Window   Help";

const OUTLINER_ROWS: &[&str] = &[
    "Scene",
    "  Camera",
    "  Lights",
    "    Sun",
    "    Fill",
    "  Meshes",
    "    Cube",
    "    Sphere",
];

const VIEWPORT_HEADER_TEXT: &str = "Viewport";
const VIEWPORT_BTN_LABEL: &str = "Reset camera";
const VIEWPORT_CLICK_LABEL_PREFIX: &str = "Clicks: ";

const PROPERTIES_ROWS: &[(&str, &str)] = &[
    ("Position", "0.00, 0.00, 0.00"),
    ("Rotation", "0.00, 0.00, 0.00"),
    ("Scale", "1.00, 1.00, 1.00"),
    ("Visible", "true"),
];

// (R1109 PR-35) A realistic console scrollback — many more lines than the
// console pane is tall. Pre-R1109 the dock panel's content wrapper clamped to
// this intrinsic height and overflowed the pane; the R1086 flex-main idiom
// (`crates/pinion-widget-paint/src/dock.rs`) now lets it shrink to the pane.
// This is the forcing consumer the R1109 verify demo asserts against. The
// [info]/[warn] levels are kept for the editor's content-sanity tests.
const CONSOLE_ROWS: &[&str] = &[
    "[info] scene loaded (5 panels)",
    "[info] viewport camera initialised",
    "[warn] no light selected",
    "[info] mesh 'ground_plane' imported (12 tris)",
    "[info] mesh 'character_base' imported (48210 tris)",
    "[info] material 'skin_sss' compiled",
    "[info] material 'cloth_weave' compiled",
    "[warn] texture 'normal_hi' missing mips, generating",
    "[info] texture atlas packed (4096x4096)",
    "[info] skeleton 'rig_main' bound (214 joints)",
    "[info] animation 'idle' baked (120 frames)",
    "[info] animation 'walk' baked (32 frames)",
    "[warn] animation 'run' has unbound root motion",
    "[info] navmesh generated (1820 polys)",
    "[info] lightmap bake queued",
    "[info] lightmap bake 25%",
    "[info] lightmap bake 50%",
    "[info] lightmap bake 75%",
    "[info] lightmap bake complete (3.2s)",
    "[error] shader 'water_caustics' link failed, using fallback",
    "[info] reflection probes captured (6)",
    "[info] audio bank 'ambient' loaded",
    "[info] audio bank 'sfx' loaded",
    "[warn] gamepad 0 not detected, keyboard fallback active",
    "[info] physics world stepped (dt=16.6ms)",
    "[info] scene serialized to disk (482 KiB)",
    "[info] autosave written",
    "[info] ready",
];

const PANEL_BODY_FONT_PX: u32 = 13;

// ─── reactive substrate hooks ─────────────────────────────────────────

/// (R685.B §5.16 §5.49) Per-Split ratio handle resolver — keyed by
/// the Split's stable id, seeded with the initial ratio the
/// `view_dock_surface` walker hands through from the topology's
/// declared [`DockNode::Split::ratio`] field. SSOT: the topology
/// IS the source of truth for initial ratios; this helper just
/// pairs the id with the live `Rc<Signal<f32>>` via `Owner::cache`
/// memoisation.
///
/// Pre-R685.B atomic 5d the binding had a `default_ratio_for_split`
/// helper that duplicated the topology's ratios (Smell 10 — SSOT
/// violation). R685.B atomic 1 lifts the initial ratio into the
/// walker's `split_state` callback signature, so the helper is no
/// longer needed.
///
/// (R686 §5.16 §5.45) `cache_key` is `impl Into<Cow<'static, str>>`
/// (R685.C S12 lifted `Owner::cache` to owned keys) so the view fn can
/// pass a Split's runtime id directly. The boot topology's static
/// split ids and a reorganize-minted `reorg-split-{n}` id both address
/// an `Owner::cache` slot by string value — a static `&str` and an
/// owned `String` carrying the same characters hit the same slot, so
/// the splitter's `SplitterExternal` (registered at boot) and the view
/// fn share one `Rc<Signal<f32>>` for the splits that existed at boot.
fn use_split_ratio(
    cache_key: impl Into<std::borrow::Cow<'static, str>>,
    initial_ratio: f32,
) -> Rc<Signal<f32>> {
    Owner::current()
        .expect("hello-dock-panels-editor: view fn runs inside owner scope")
        .cache(cache_key, || Signal::new(initial_ratio))
}

/// (R686 §5.16 §5.45) The live editor dock surface, owned by the binding
/// as a reactive `Signal<Option<DockTopology>>` so drag-to-reorganize
/// gestures can mutate it. (R1084 §5.51) The dock-surface signal is
/// `Option`-typed because an empty dock (`None`) is a first-class state
/// of the dock model — the editor seeds `Some(build_editor_topology())`
/// and never empties, but it shares the universal `Option` surface type
/// the reorganize coordinator is total over. The [`DockReorganizeExternal`]
/// holds a clone and `set`s `Some(mutated)` on each gesture; the view fn's
/// `get()` subscription re-renders the new layout.
fn use_editor_topology() -> Rc<Signal<Option<DockTopology>>> {
    Owner::current()
        .expect("hello-dock-panels-editor: view fn runs inside owner scope")
        .cache("editor_topology", || {
            Signal::new(Some(build_editor_topology()))
        })
}

/// (R749 §5.52) The shared reorganize [`UndoStack`] — the editor's
/// workspace history. The [`DockReorganizeExternal`] records each applied
/// gesture onto it; the [`UndoStackExternal`] surfaces undo/redo to RPC.
/// Both reach the same `Rc` (the `use_undo_stack` sharing).
fn use_dock_undo() -> Rc<UndoStack> {
    use_undo_stack(DOCK_UNDO_KEY)
}

/// (R1081/R1082 §5.51) The ONE shared reorganize coordinator. Cached (not
/// rebuilt per `create_extra_externals` run) so the `split_seq` + undo
/// stack persist across reorganizes — and so the invoke
/// [`DockReorganizeExternal`] and every R742 [`DockPanelExternal`] share
/// the same counter (no `reorg-split-{n}` collision). The topology +
/// undo deps are resolved BEFORE the cache factory (the `Owner::cache`
/// factory must not nest another `cache` resolution).
fn use_editor_reorganizer() -> Rc<DockReorganizer> {
    let topology = use_editor_topology();
    let undo = use_dock_undo();
    Owner::current()
        .expect("hello-dock-panels-editor: view fn runs inside owner scope")
        .cache("editor_reorganizer", move || {
            DockReorganizer::new(topology).with_undo(undo)
        })
}

/// (R1082 §5.51) The ONE shared live drop-preview the R742 panel
/// externals write (the dragged panel's `drag_to`) and the view fn reads
/// (to paint the target panel's zone overlay). Cached so every panel
/// external + the view fn reach the same signal.
fn use_drop_preview() -> Rc<Signal<Option<DockDropPreview>>> {
    Owner::current()
        .expect("hello-dock-panels-editor: view fn runs inside owner scope")
        .cache("editor_drop_preview", || Signal::new(None))
}

fn use_viewport_click_counter() -> Rc<Signal<u32>> {
    Owner::current()
        .expect("hello-dock-panels-editor: view fn runs inside owner scope")
        .cache("editor_viewport_click_counter", || Signal::new(0_u32))
}

// ─── float-to-window substrate (R1105 §5.51 §5.16 §5.41 PR-31) ─────────
//
// The editor is the **2nd dock consumer** of multi-window tear-off (the
// flat `hello-dock-panels` is the 1st). A panel's `DockPanelExternal`
// already emits the `tear_off` / `tear_off_follow` / `tear_off_redock` /
// `tear_off_redock_at` intents (R1094/R1100); before R1105 the editor wired
// no reducer arm + no `windows_signal`, so they were a no-op (the
// create_extra_externals R1095.1 comment). R1105 wires them: a torn-off
// panel moves to its own floating `WindowSpec` (observable over
// `scene/windows`) while its dock leaf paints a `view_floating_placeholder`.
//
// These helpers mutate the binding-local `Signal<Vec<WindowSpec>>`. They
// CANNOT live in `pinion_widget_paint::dock` — `WindowSpec` is a
// `pinion-shell` type and pinion-widget-paint deliberately does NOT depend
// on pinion-shell (it even mirrors `WindowSpec`'s serde to avoid the edge),
// so the SEED's "dock-substrate lift" of the flat trio is architecturally
// infeasible as a widget-paint lift. The only viable lift target is a
// pinion-shell generic window helper, deferred: the editor consumer
// DIVERGES from the flat at R1106 (zone-honoring redock relocates the panel
// in the `DockTopology` the flat lacks), so the truly-shared surface is not
// yet settled — lift after the divergence materialises, not before
// ([[abstraction-needs-second-consumer]]).

/// (R1105 §5.51 §5.16 §5.41 PR-31) The editor's runtime window topology.
/// The shell's `reconcile_windows` Effect subscribes this Signal at boot;
/// each `Signal::set` diffs add/drop against winit windows. Initial
/// topology = the canonical single main window; tear-off intents append
/// `torn-{panel_id}` floating-window specs, dock-back intents remove them.
fn use_editor_windows() -> Rc<Signal<Vec<WindowSpec>>> {
    Owner::current()
        .expect("hello-dock-panels-editor: windows_signal runs inside owner scope")
        .cache("editor_windows", || {
            Signal::new(vec![WindowSpec::new(
                Cow::Borrowed(MAIN_WINDOW_ID),
                "hello-dock-panels-editor — R685 5-pane editor",
                SizeStrategy::Fixed {
                    width: MAIN_W,
                    height: MAIN_H,
                },
            )])
        })
}

/// Canonical floating-window id for `panel_id` — `"torn-{panel_id}"`. A thin
/// wrapper over the lifted [`dock_floating_window_id`] +
/// [`DEFAULT_FLOATING_WINDOW_PREFIX`] SSOT (no duplicate prefix literal).
fn floating_window_id(panel_id: &str) -> String {
    dock_floating_window_id(DEFAULT_FLOATING_WINDOW_PREFIX, panel_id)
}

/// Human-readable title for a floating window hosting `panel_id`.
fn floating_window_title(panel_id: &str) -> String {
    format!("hello-dock-panels-editor — {panel_id} (floating)")
}

/// Declared logical-pixel position a torn-off panel's floating window opens
/// at — a per-panel cascade offset so multiple tear-offs do not stack
/// exactly (the [`WindowSpec::with_position`] consumer the flat demo
/// pioneered at R1087).
fn floating_window_position(panel_id: &str) -> (i32, i32) {
    let step = match panel_id {
        TOOLBAR_PANEL_TAG => 0,
        OUTLINER_PANEL_TAG => 1,
        VIEWPORT_PANEL_TAG => 2,
        PROPERTIES_PANEL_TAG => 3,
        _ => 4,
    };
    (160 + step * 44, 120 + step * 44)
}

/// (R1115 §5.51 §5.16 §5.41 PR-38) The floating `WindowSpec` a torn-off
/// `panel_id` opens into, placed at outer position `pos`. The single
/// construction site for the editor's two float paths (the `tear_off` toggle +
/// the live follow) so they cannot drift. Declares `with_decorations(false)`:
/// the editor owns a floating panel's chrome (the panel paints its own header),
/// so the OS draws no redundant title bar over a torn-off DCC panel — the
/// custom-chrome floating panel a self-hosted editor wants (Blender/Unreal show
/// no OS title bar on a torn-off panel). Observable as `scene/windows`
/// `decorations:false`; the main window keeps the default `decorations:true`.
fn floating_window_spec(panel_id: &str, pos: (i32, i32)) -> WindowSpec {
    WindowSpec::new(
        Cow::Owned(floating_window_id(panel_id)),
        floating_window_title(panel_id),
        SizeStrategy::Fixed {
            width: FLOATING_W,
            height: FLOATING_H,
        },
    )
    .with_position(pos.0, pos.1)
    .with_decorations(false)
}

/// `true` iff a floating window for `panel_id` currently exists in `panels` —
/// a thin binding wrapper over the lifted [`window_exists`] predicate
/// (R1107.1) keyed on the panel's `torn-<panel>` floating-window id.
fn is_panel_floating(panels: &[WindowSpec], panel_id: &str) -> bool {
    window_exists(panels, &floating_window_id(panel_id))
}

/// (R1134 §5.51.1) The panel ids that currently have a floating window — every
/// `torn-<panel>` window id stripped of the [`DEFAULT_FLOATING_WINDOW_PREFIX`].
/// Under `FloatPolicy::Collapse` a floated panel's leaf LEAVES the topology
/// (the slot reflows), so the external factory unions these with
/// [`DockTopology::panel_ids`] (step ②) to keep the floated panel's
/// [`DockPanelExternal`] registered — its window header still drags + docks back.
/// Under the placeholder default the leaf stays, so a floating panel is already in
/// `panel_ids()` and the union is a no-op (the main window is never `torn-`).
fn floating_panel_ids(panels: &[WindowSpec]) -> Vec<String> {
    panels
        .iter()
        .filter_map(|w| {
            w.id.strip_prefix(DEFAULT_FLOATING_WINDOW_PREFIX)
                .map(str::to_string)
        })
        .collect()
}

/// Reducer mutation for a `tear_off` intent — the canonical toggle:
///
/// * Panel docked (no matching floating window) → push a floating
///   `WindowSpec`. The dock leaf stays in the topology and repaints a
///   [`view_floating_placeholder`] (slot preserved for a trivial dock-back).
/// * Panel floating → remove the `WindowSpec` (dock-back); the placeholder
///   reverts to the live panel content.
fn toggle_panel_floating(panel_id: &str) {
    let signal = use_editor_windows();
    let mut current = signal.get();
    let target = floating_window_id(panel_id);
    if let Some(idx) = current.iter().position(|w| w.id == target) {
        current.remove(idx);
    } else {
        current.push(floating_window_spec(
            panel_id,
            floating_window_position(panel_id),
        ));
    }
    signal.set(current);
}

/// Remove `panel_id`'s floating window (redock / restore). Idempotent no-op
/// when the panel is already docked, so a redock intent that arrives without
/// a window (a cancelled / never-floated gesture) is harmless.
fn redock_panel_floating(panel_id: &str) {
    let signal = use_editor_windows();
    let mut current = signal.get();
    let target = floating_window_id(panel_id);
    if let Some(idx) = current.iter().position(|w| w.id == target) {
        current.remove(idx);
        signal.set(current);
    }
}

/// (R1163b §5.51 §2 #7) Apply a cross-window dock-back drop through the SAME
/// [`resolve_drop`] SSOT the same-window drag and the on-target preview
/// ([`DockPanelsEditorView::dock_drop_preview`]) use, so the cross-window RESULT ==
/// the PREVIEW by construction (R1163b retired the editor's legacy continuous
/// `dock_panel_at_zone` here — before, the cross-window path classified with the
/// continuous geometry while the same-window path used the banded `resolve_drop`, a
/// two-geometry inconsistency). The `{target, x_rel, y_rel}` the shell resolved is
/// rebuilt into a [`DropPoint`] and resolved against the editor's reorganizer (its
/// topology = `is_panel`, its `tabbing` policy):
///
/// * `Dock` / `OuterDock` — relocate the panel's leaf to that zone, then drop its
///   floating window so the docked leaf paints content ([`redock_panel_floating`]).
/// * `Float` — the cursor was in a panel's dead-zone ring (a discrete-target FLOAT,
///   not a dock); the panel stays FLOATING (no `redock_panel_floating`), matching the
///   blank preview.
/// * `SnapBack` — over its own bare slot (a no-move return home). Unreachable in the
///   cross-window case (a floating panel's home is a placeholder, which `resolve_drop`
///   folds to `Dock { Center }`), but treated as a home redock for totality.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the shell serialised f32 DropPoint coords to JSON f64; reading back to f32 round-trips losslessly"
)]
fn redock_cross_window(panel: &str, v: &serde_json::Value) {
    let Some(target) = v.get("target").and_then(serde_json::Value::as_str) else {
        return;
    };
    let (Some(x_rel), Some(y_rel)) = (
        v.get("x_rel").and_then(serde_json::Value::as_f64),
        v.get("y_rel").and_then(serde_json::Value::as_f64),
    ) else {
        return;
    };
    let reorganizer = use_editor_reorganizer();
    let point = DropPoint {
        tag: target.to_string(),
        x_rel: x_rel as f32,
        y_rel: y_rel as f32,
    };
    match resolve_drop(
        Some(&point),
        panel,
        |t| reorganizer.is_panel(t),
        reorganizer.tabbing(),
    ) {
        DropResolution::Dock { target, zone } => {
            let _ = reorganizer.dock_panel_at_resolved_zone(panel, &target, zone);
            redock_panel_floating(panel);
        }
        DropResolution::OuterDock { edge } => {
            let _ = reorganizer.dock_panel_outer(panel, edge);
            redock_panel_floating(panel);
        }
        DropResolution::SnapBack { .. } => redock_panel_floating(panel),
        DropResolution::Float => {}
    }
}

/// (R1105/R1107 §5.16 §5.41 PR-31) Ensure `panel_id` has a floating window and
/// write its outer position from the desktop-converted cursor (the live
/// follow). Idempotent: creates on the first escaped move, repositions on
/// every subsequent move. `Signal::set`'s equality-skip collapses a
/// stationary cursor. Non-toggling — the AI dock-back stays on
/// [`toggle_panel_floating`]. The desktop conversion runs through the lifted
/// [`desktop_position_from`] (R1107.1): `source_window` (R1107) names the
/// window the cursor is measured in, so the follower lands at the RIGHT
/// origin (re-dragging an already-floating header uses that floater's frame,
/// not main's — the R1095.1 fix, now a `pinion-shell` SSOT shared with the
/// flat consumer).
fn follow_panel_floating(panel_id: &str, source_window: Option<&str>, cursor: (f64, f64)) {
    let signal = use_editor_windows();
    let mut current = signal.get();
    let pos = desktop_position_from(&current, source_window, cursor);
    let target = floating_window_id(panel_id);
    if let Some(spec) = current.iter_mut().find(|w| w.id == target) {
        spec.position = Some(pos);
    } else {
        current.push(floating_window_spec(panel_id, pos));
    }
    signal.set(current);
}

/// (R1118 §5.51 §5.16 §5.41 PR-38) Move `panel_id`'s floating window BY the
/// grab-relative displacement `delta` (the [`WINDOW_MOVE_EVENT`] reducer): the
/// window's title bar was dragged, so `new_pos = current_pos + delta` keeps the
/// grabbed point under the cursor. Distinct from [`follow_panel_floating`],
/// which PLACES a torn-off panel AT a cursor (`origin + cursor`); this relocates
/// an already-floating window by a delta. Idempotent no-op if the panel is not
/// currently floating (a stray move with no window).
#[allow(
    clippy::cast_possible_truncation,
    reason = "logical-pixel displacement f64 -> i32 outer position; sub-pixel is irrelevant to window placement"
)]
fn move_floating_window(panel_id: &str, delta: (f64, f64)) {
    let signal = use_editor_windows();
    let mut current = signal.get();
    let target = floating_window_id(panel_id);
    if let Some(spec) = current.iter_mut().find(|w| w.id == target) {
        let (x, y) = spec.position.unwrap_or((0, 0));
        spec.position = Some((x + delta.0.round() as i32, y + delta.1.round() as i32));
        signal.set(current);
    }
}

// ─── topology constructor ─────────────────────────────────────────────

/// R685 §5.16 §5.49 — declarative editor topology. Built once per
/// view-fn paint (the topology is data; the per-split ratios live
/// in separate `Rc<Signal<f32>>` handles the view fn re-reads each
/// paint). Each Split carries its stable `id` — the
/// [`view_dock_surface`] walker dispatches `split_handle` by that
/// id, so binding state stays bound to the right Split across any
/// future topology mutation.
fn build_editor_topology() -> DockTopology {
    DockTopology::new(DockNode::split_vertical(
        SPLIT_OUTER_TAG,
        SPLIT_OUTER_RATIO_DEFAULT,
        DockNode::leaf(TOOLBAR_PANEL_TAG),
        DockNode::split_vertical(
            SPLIT_INNER_V_TAG,
            SPLIT_INNER_V_RATIO_DEFAULT,
            DockNode::split_horizontal(
                SPLIT_MIDDLE_H_TAG,
                SPLIT_MIDDLE_H_RATIO_DEFAULT,
                DockNode::leaf(OUTLINER_PANEL_TAG),
                DockNode::split_horizontal(
                    SPLIT_INNER_H_TAG,
                    SPLIT_INNER_H_RATIO_DEFAULT,
                    DockNode::leaf(VIEWPORT_PANEL_TAG),
                    DockNode::leaf(PROPERTIES_PANEL_TAG),
                ),
            ),
            DockNode::leaf(CONSOLE_PANEL_TAG),
        ),
    ))
}

// (R688) `split_cache_key_for` removed. Pre-R688 it recovered a boot
// split's id as a `&'static str` because `ExtraExternal::new` required
// that lifetime; the R688 Cow lift lets `create_extra_externals` pass
// `id.to_string()` directly, so boot + runtime splits share one
// Cow-keyed path with no static recovery table (and no `unreachable!`).

// ─── panel content view fns ───────────────────────────────────────────

fn view_toolbar_content(theme: &Theme) -> Scene {
    // (R1135 §5.51.1) Read the live torn-slot policy from the shared coordinator.
    // `float_policy()` reads a reactive `Signal`, so THIS subscribe repaints the
    // toolbar (the toggle label) whenever the policy flips — from this button OR
    // the AI `set_float_policy` invoke (one SSOT, both paths consistent).
    let policy = use_editor_reorganizer().float_policy();
    let label = Scene::Text(TextNode::styled(
        TOOLBAR_LABEL.to_string(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(PANEL_BODY_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    // The toggle button shows the CURRENT mode; clicking flips it (the
    // `policy_btn.click` reducer arm). Rendered discrete (Idle, no hover spring) —
    // the label change is the affordance; a person no longer needs the RPC path.
    let policy_btn = view_button(
        policy_toggle_label(policy),
        ButtonState::Idle,
        0.0,
        false,
        &ButtonColors::accent(theme),
        &ButtonStyle::m3_default(POLICY_BTN_TAG)
            .with_size(Size::px(210, 28))
            .with_padding(Rect::new(12, 4, 12, 4))
            .with_label_font_size_px(PANEL_BODY_FONT_PX),
    );
    let mut children = vec![label, policy_btn];
    // (R1145 §5.51) Show "Undock tab" ONLY while some panels are tabbed (the
    // reorganizer's reactive `tabs_well_count` subscribes the toolbar, so the
    // button appears the instant a tabify lands and vanishes when the last well
    // splits back out). Clicking it undocks the active tab into a split sibling.
    if use_editor_reorganizer().has_tab_wells() {
        children.push(view_button(
            "Undock tab",
            ButtonState::Idle,
            0.0,
            false,
            &ButtonColors::accent(theme),
            &ButtonStyle::m3_default(UNDOCK_BTN_TAG)
                .with_size(Size::px(110, 28))
                .with_padding(Rect::new(12, 4, 12, 4))
                .with_label_font_size_px(PANEL_BODY_FONT_PX),
        ));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag("toolbar_content_body")
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Start)
                    .with_padding(Rect::new(12, 0, 12, 0)),
            ),
    )
}

/// (R1135 §5.51.1) The float-policy toggle button label for the current mode —
/// the SSOT for the human-readable policy text the toolbar paints (and the demo
/// asserts on). Shows the CURRENT torn-slot policy.
fn policy_toggle_label(policy: FloatPolicy) -> &'static str {
    match policy {
        FloatPolicy::Collapse => "torn slot: collapse",
        FloatPolicy::Placeholder => "torn slot: placeholder",
    }
}

/// (R1135 §5.51.1) The opposite policy — the toggle the `policy_btn.click` reducer
/// arm applies (`Placeholder` <-> `Collapse`).
fn toggled_float_policy(policy: FloatPolicy) -> FloatPolicy {
    match policy {
        FloatPolicy::Placeholder => FloatPolicy::Collapse,
        FloatPolicy::Collapse => FloatPolicy::Placeholder,
    }
}

fn view_outliner_content(theme: &Theme) -> Scene {
    let row_scenes: Vec<Scene> = OUTLINER_ROWS
        .iter()
        .enumerate()
        .map(|(i, label)| {
            Scene::Container(
                ContainerNode::new(vec![Scene::Text(TextNode::styled(
                    (*label).to_string(),
                    Rect::default(),
                    TextStyle::new()
                        .with_size_px(PANEL_BODY_FONT_PX)
                        .with_fg(theme.resolve(ColorRole::OnSurface)),
                ))])
                .with_tag(format!("outliner_row_{i}"))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_padding(Rect::new(4, 2, 4, 2)),
                ),
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(row_scenes)
            .with_tag("outliner_content_body")
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_padding(Rect::new(8, 8, 8, 8)),
            ),
    )
}

fn view_viewport_content(state: ButtonState, theme: &Theme) -> Scene {
    let click_count = use_viewport_click_counter().get();
    let header = Scene::Text(TextNode::styled(
        VIEWPORT_HEADER_TEXT.to_string(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(PANEL_BODY_FONT_PX + 2)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let counter = Scene::Text(TextNode::styled(
        format!("{VIEWPORT_CLICK_LABEL_PREFIX}{click_count}"),
        Rect::default(),
        TextStyle::new()
            .with_size_px(PANEL_BODY_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    let button_paint = view_viewport_button(state, theme);
    Scene::Container(
        ContainerNode::new(vec![header, counter, button_paint])
            .with_tag("viewport_content_body")
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_padding(Rect::new(16, 16, 16, 16)),
            ),
    )
}

/// (R686.B §5.16) M3 Accent-tinted viewport button via the
/// `pinion_widget_paint::button` substrate. `ButtonColors::accent`
/// resolves the same Accent / OnAccent roles + the
/// SurfaceContainerHigh disabled tier the pre-R686.B inline match did.
///
/// This binding's button is **discrete** (no hover spring), so it
/// passes a step `hover_progress` — `1.0` on Hover, `0.0` otherwise —
/// which lands `m3_button_fill` on exactly the endpoints the old
/// inline `match` produced (Idle = Accent, Hover = lerp 0.08).
fn view_viewport_button(state: ButtonState, theme: &Theme) -> Scene {
    let hover_progress = if matches!(state, ButtonState::Hover) {
        1.0
    } else {
        0.0
    };
    view_button(
        VIEWPORT_BTN_LABEL,
        state,
        hover_progress,
        // R694 §5.39 — the viewport action button is pointer-driven and
        // is not the focus-ring debt target (Dialog / Toolbar / Tabs are);
        // pass `false` until the editor threads per-control focus posture.
        false,
        &ButtonColors::accent(theme),
        &ButtonStyle::m3_default(VIEWPORT_BTN_TAG)
            .with_size(Size::px(180, 40))
            .with_padding(Rect::new(16, 8, 16, 8))
            .with_label_font_size_px(PANEL_BODY_FONT_PX),
    )
}

fn view_properties_content(theme: &Theme) -> Scene {
    let row_scenes: Vec<Scene> = PROPERTIES_ROWS
        .iter()
        .enumerate()
        .map(|(i, (key, value))| {
            let key_text = Scene::Text(TextNode::styled(
                (*key).to_string(),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(PANEL_BODY_FONT_PX)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            ));
            let value_text = Scene::Text(TextNode::styled(
                (*value).to_string(),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(PANEL_BODY_FONT_PX)
                    .with_fg(theme.resolve(ColorRole::OnSurface)),
            ));
            Scene::Container(
                ContainerNode::new(vec![key_text, value_text])
                    .with_tag(format!("property_row_{i}"))
                    .with_layout(
                        LayoutStyle::new()
                            .flex(FlexDirection::Row)
                            .with_justify(JustifyContent::SpaceBetween)
                            .with_align_items(AlignItems::Center)
                            .with_padding(Rect::new(8, 4, 8, 4)),
                    ),
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(row_scenes)
            .with_tag("properties_content_body")
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_padding(Rect::new(8, 8, 8, 8)),
            ),
    )
}

fn view_console_content(theme: &Theme) -> Scene {
    let row_scenes: Vec<Scene> = CONSOLE_ROWS
        .iter()
        .enumerate()
        .map(|(i, line)| {
            Scene::Container(
                ContainerNode::new(vec![Scene::Text(TextNode::styled(
                    (*line).to_string(),
                    Rect::default(),
                    TextStyle::new()
                        .with_size_px(PANEL_BODY_FONT_PX)
                        .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
                ))])
                .with_tag(format!("console_row_{i}"))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_padding(Rect::new(8, 2, 8, 2)),
                ),
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(row_scenes)
            .with_tag("console_content_body")
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_padding(Rect::new(8, 8, 8, 8)),
            ),
    )
}

fn panel_content_for(panel_id: &str, state: ButtonState, theme: &Theme) -> Scene {
    match panel_id {
        TOOLBAR_PANEL_TAG => view_toolbar_content(theme),
        OUTLINER_PANEL_TAG => view_outliner_content(theme),
        VIEWPORT_PANEL_TAG => view_viewport_content(state, theme),
        PROPERTIES_PANEL_TAG => view_properties_content(theme),
        CONSOLE_PANEL_TAG => view_console_content(theme),
        other => Scene::Text(TextNode::styled(
            format!("(unknown panel: {other})"),
            Rect::default(),
            TextStyle::new()
                .with_size_px(PANEL_BODY_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )),
    }
}

// (R685.B atomic 3) `split_tag_for` removed — pre-R685.B identity
// function. R685.B walker auto-builds the splitter's paint-side tag
// from `DockNode::Split::id` directly (SSOT), so no caller-side
// indirection needed. (R688) `split_cache_key_for` also removed — the
// `ExtraExternal` Cow lift made the split id usable as the cache key +
// registration tag directly, so boot + runtime splits share one path.
//
// (R685.C atomic 2) binding-local `for_each_split` removed — the
// walk lifted to `DockTopology::for_each_split` substrate accessor
// (DRY — pre-R685.C the binding copied the substrate's
// view_dock_surface_node traversal).

// ─── per-window paint (R1105 §5.51 §5.16 §5.45 PR-31) ─────────────────

/// (R1105 §5.51 §5.16 §5.45 PR-31) The main window's dock surface. Extracted
/// from `WidgetCore::view` so [`view_for_window`](DockPanelsEditorView)
/// dispatches the main dock vs a torn-off panel's floating window. A panel
/// whose floating window currently exists paints a [`view_floating_placeholder`]
/// in its dock leaf — the slot is preserved (a [`DockNode::Leaf`] still),
/// so the panel is in exactly one place (the floating window), never
/// duplicated (SSOT), and dock-back reverts the placeholder to live content.
fn view_main_dock(state: ButtonState) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    // (R686 §5.45) Read the live topology from its reactive Signal
    // (subscribes the view; a reorganize gesture's `Signal::set` re-renders).
    let topology = use_editor_topology().get();
    // (R1082 §5.51) The shared drop-preview (subscribes the view, so a drag's
    // `drag_to` re-renders the target panel's zone overlay).
    let preview = use_drop_preview().get();
    // (R1105 §5.51) The live floating-window set; a panel listed here paints
    // a placeholder in its dock leaf instead of its content.
    let windows = use_editor_windows().get();
    // (R1084 §5.51) The dock surface is total over the empty (`None`) state.
    // The editor seeds `Some` and never empties, but the view honours the
    // universal Option (the R685.B SSOT walker auto-builds DockPanelStyle /
    // SplitterStyle from the topology + threads its declared initial_ratio).
    let surface = match topology {
        Some(topology) => view_dock_surface_styled(
            &topology,
            |panel_id| {
                if is_panel_floating(&windows, panel_id) {
                    view_floating_placeholder(
                        panel_id,
                        &theme,
                        &FloatingPlaceholderStyle::m3_default()
                            .with_label_font_size_px(PANEL_BODY_FONT_PX),
                    )
                } else {
                    panel_content_for(panel_id, state, &theme)
                }
            },
            |split_id, initial_ratio| DockSplitState {
                ratio_signal: use_split_ratio(split_id.to_string(), initial_ratio),
                dragging: false,
            },
            |panel_id| {
                preview
                    .as_ref()
                    .filter(|p| p.target == panel_id)
                    .map(|p| p.zone)
            },
            // (R1173 §5.16) LOCK the toolbar fully — HEADERLESS (no title-bar drag
            // handle; the "File Edit View…" menu strip IS the content) + NON-RECEIVING
            // (`drop_target=false`: no panel can dock INTO it). Composed with the
            // factory's `with_movable(false)` (R1172): the toolbar is a fixed,
            // immovable, headerless strip — per-panel chrome freely combined.
            |panel_id, style| {
                if panel_id == TOOLBAR_PANEL_TAG {
                    style.with_show_header(false).with_drop_target(false)
                } else {
                    style
                }
            },
            &theme,
        ),
        None => Scene::Container(ContainerNode::new(vec![])),
    };
    // (R1167 §5.51) Same-window OUTER full-span preview: a dock-panel drag whose
    // cursor entered the window's outer band resolves to a full-span outer dock —
    // the preview's `target` is the OUTER_DOCK_ZONE_TAG sentinel (no panel matches
    // the per-panel `preview_zone` callback above, so the inner panels stay
    // un-highlighted). Overlay a full-span band across the WHOLE surface (a
    // row/column over every pane) at the previewed edge, of the thickness
    // `dock_panel_outer` lands at — preview == result, the same affordance the
    // cross-window floater preview shows. Appended as an absolute (out-of-flow)
    // child of the surface root, exactly as a panel appends its inner-zone
    // highlight, so the dock layout is undisturbed.
    match preview.as_ref().filter(|p| p.target == OUTER_DOCK_ZONE_TAG) {
        Some(p) => match surface {
            Scene::Container(mut root) => {
                root.children
                    .push(dock_outer_zone_highlight(p.zone, &theme));
                Scene::Container(root)
            }
            other => other,
        },
        None => surface,
    }
}

/// (R1105 §5.51 §5.16 §5.45 PR-31) A torn-off panel's floating-window paint.
/// Wraps the panel's content in a [`view_dock_panel`] so the floating window
/// carries a draggable header. The same [`DockPanelExternal`] registered at
/// `panel_id` services both the docked placeholder header AND the floating
/// header (1 External per panel, shared across windows; each per-window
/// `InputRouter` resolves the composite tag against its own paint scene), so
/// a second tear-off on the floating header dock-backs it (toggle). The
/// panel id is the header title — the same SSOT the dock walker uses.
fn view_floating_panel(panel_id: &str, state: ButtonState) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let content = panel_content_for(panel_id, state, &theme);
    // (R1116/R1118 §5.51 PR-38) The floater is the SOLE content of its own
    // window, so `drop_target=false`: the floater exposes no drop target, so
    // another panel cannot be docked INTO it (cross-window-drop rejection — the
    // flag's load-bearing effect). Dragging the borderless floater by its own
    // header (the OS title bar `decorations:false` removed) is a WINDOW MOVE,
    // driven by the dedicated `drag_to_at` branch (`source_window ==
    // floating_window`) → `WINDOW_MOVE_EVENT`, NOT by this flag. A drop onto the
    // MAIN dock still redocks cross-window (`over_window`, independent of this).
    let style = DockPanelStyle::m3_default(panel_id.to_string()).with_drop_target(false);
    // (R1171 §5.16) The window controls (min / max / close) live IN the panel
    // header (the title bar that is already the redock drag handle) — ONE title
    // bar, auto-sized by the layout. This replaced the R1170 shell-overlay chrome
    // (whose fixed pixel height the binding had to dimension-match — the
    // controls-in-header redesign the user's "치수를 맞춰야 하는 것 자체가 잘못된
    // 설계" caught). The shell still routes a press on these tags to
    // `set_minimized` / `set_maximized` / `window_close_requested`.
    view_dock_panel_with_actions(
        panel_id,
        content,
        &theme,
        &style,
        None,
        // (R1187 §5.16) The window controls (min / max / close) IN the header —
        // now the lifted `pinion_widget_paint::view_window_controls` (R1171 was a
        // binding-local copy; sprag is the 2nd consumer, so the composition SSOT
        // moved to the widget crate). The BINDING supplies the shell's routing
        // tags (widget-paint stays overlay-independent) so `try_chrome_press`
        // routes close → `window_close_requested` (dock-back), min / max → the
        // per-window `set_minimized` / `set_maximized`.
        Some(view_window_controls(
            &theme,
            PANEL_BODY_FONT_PX,
            WindowControlTags {
                minimize: WINDOW_CHROME_MINIMIZE_TAG,
                maximize: WINDOW_CHROME_MAXIMIZE_TAG,
                close: WINDOW_CHROME_CLOSE_TAG,
            },
        )),
    )
}

// ─── trait wiring ─────────────────────────────────────────────────────

/// R685 §5.16 §5.41 §5.49 — editor binding carrier. No fields — every
/// trait method is associated.
pub struct DockPanelsEditorView;

impl WidgetCore for DockPanelsEditorView {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn tag() -> &'static str {
        VIEWPORT_BTN_TAG
    }

    fn title() -> &'static str {
        "hello-dock-panels-editor — R685 5-pane editor"
    }

    fn create_external() -> Box<dyn pinion_core::external::External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // (R685.C atomic 2) Enumerate the topology's `DockNode::Split`
        // nodes via the `DockTopology::for_each_split` substrate
        // accessor — each visit carries the Split's stable id +
        // orientation + initial ratio. Register one `SplitterExternal`
        // per Split keyed on the Split's id (which IS the paint-side tag
        // post-R685.B walker SSOT enforcement).
        //
        // (R688) Walk the *live* topology signal's current value. After
        // the R688 `ExtraExternal` Cow lift the Split's id is the cache
        // key AND the registration tag directly — `id.to_string()`, no
        // `&'static str` recovery table. A runtime reorganize that mints
        // `reorg-split-N` flows through this same loop: the R688
        // `CoreShell::reconcile_externals` re-runs this factory when the
        // topology Signal changes, so the new split registers its
        // `SplitterExternal` (and becomes drag-resizable) automatically.
        // (R1081/R1082 §5.51) The ONE shared coordinator + drop-preview —
        // cached so they persist across this factory's re-runs (the R688
        // dynamic external set re-runs on each topology change).
        let reorganizer = use_editor_reorganizer();
        let preview = use_drop_preview();
        // (R1133 §5.51.1 §2 #5) Snapshot the live float truth so each
        // reconstructed panel External re-hydrates its lifecycle chart from it.
        // This factory re-runs on every topology change (R688 reconcile); without
        // re-hydration a panel that is floating when an UNRELATED reorganize
        // rebuilds the external set would have its chart reset to Docked
        // (desyncing the R1131 2-layer invariant). `windows_signal` is the
        // persistent SSOT; the chart is its re-hydratable projection.
        let windows = use_editor_windows().get();
        let mut externals = Vec::new();
        // (R1084 §5.51) The dock surface may be empty (`None`, a first-class
        // state); only a `Some(topology)` contributes splitter + panel
        // externals. The reorganize + undo anchors register unconditionally
        // (an AI client can `invoke` reorganize on an empty surface — it is
        // the identity no-op). The editor seeds `Some` and never empties, but
        // it shares the universal `Option` dock surface.
        if let Some(topology) = use_editor_topology().get() {
            // (R1083) One external per Split + one per panel (`panel_count`
            // counts tab-well panels individually, unlike `leaf_count`) + the
            // two unconditional reorganize + undo anchors pushed below.
            externals.reserve(
                topology.split_count() + topology.panel_count() + topology.tabs_well_count() + 2,
            );
            topology.for_each_split(|id, orientation, ratio| {
                let signal = use_split_ratio(id.to_string(), ratio);
                let external = SplitterExternal::new(orientation).attach_ratio(signal);
                externals.push(ExtraExternal::new(id.to_string(), Box::new(external)));
            });
            // (R1082 §5.51) One R742 drag External per panel, registered at the
            // panel root tag (= panel_id, the view_dock_panel root). Each shares
            // the ONE coordinator (a pointer dock + an AI invoke mint from one
            // split_seq) and the ONE preview (the dragged panel's drag_to writes
            // the affordance the target panel paints).
            //
            // R1105+ — this editor is the dock+float 2nd consumer: it consumes
            // the reorganizer (dock / split / tabify) AND has a `windows_signal`
            // (`use_editor_windows`) + wires the `tear_off`/`tear_off_follow`/
            // `tear_off_redock(_at)`/`window_move` reducer arms (below), so a
            // panel tears off into its own floating window and (R1116/R1117) is
            // dragged by its title bar. (Superseded the pre-R1105 "DOCK-ONLY,
            // float-to-window deferred" note that used to sit here.)
            // (R1134 §5.51.1 step②) The panel-external set is the UNION of docked
            // panels (`panel_ids()`) + floating panels (the `torn-` windows). Under
            // FloatPolicy::Collapse a float REMOVES the leaf, so a floated panel
            // leaves `panel_ids()` — register its external from the windows truth
            // too, else the floating window's header loses its drag / dock-back.
            // Under the placeholder default the leaf stays, so the floating id is
            // already in `panel_ids()` and this union is a no-op (identical tag
            // set). `reconcile_externals`' preserve-by-tag then keeps the floated
            // panel's live instance (+ its lifecycle chart) across the collapse
            // topology change — the chart is never reconstructed for a surviving tag.
            let mut panel_ids: Vec<String> = topology
                .panel_ids()
                .iter()
                .map(|p| (*p).to_string())
                .collect();
            for fid in floating_panel_ids(&windows) {
                if !panel_ids.iter().any(|p| p == &fid) {
                    panel_ids.push(fid);
                }
            }
            for panel_id in &panel_ids {
                let panel = DockPanelExternal::new(panel_id.clone())
                    .with_reorganizer(Rc::clone(&reorganizer))
                    .with_drop_preview(Rc::clone(&preview))
                    // (R1116 §5.51 PR-38) Declare this panel's own floating window
                    // so a header drag IN it is a borderless title-bar WINDOW MOVE
                    // (grab-offset), not a dock tear-off. The id is the same
                    // `floating_window_id` SSOT the tear-off reducer + view use.
                    .with_floating_window(floating_window_id(panel_id))
                    // (R1172 §5.16) LOCK the toolbar: a fixed top strip is not
                    // movable — its header starts no drag, so it can never be
                    // reordered, docked elsewhere, or torn off (a pro dock's
                    // non-movable toolbar). Every other panel stays freely movable.
                    .with_movable(panel_id != TOOLBAR_PANEL_TAG)
                    // (R1133 §5.51.1) Re-hydrate the lifecycle chart from the live
                    // float truth so a reconstruct (any topology change) does not
                    // reset a floating panel's chart to Docked.
                    .with_initial_floating(is_panel_floating(&windows, panel_id));
                externals.push(ExtraExternal::new(panel_id.clone(), Box::new(panel)));
            }
            // (R1096 §5.51) One TabWellExternal per Tabs well, registered at
            // the well's stable id (= the view_tabs strip tag = the R51.42
            // primary half of every `{well_id}#{i}` tab tag the walker
            // paints). A click on a tab routes through the `#`-split protocol
            // to this external, which switches the visible tab via the SAME
            // shared coordinator the pointer drags + AI invokes use. A
            // freshly-tabified `reorg-tabs-{seq}` well wires automatically:
            // this factory re-runs on every topology change (R688).
            topology.for_each_tabs_well(|well_id, _active, _panels| {
                // (R1158 §5.51) Share the live drop-preview so a tab dragged over a
                // dock zone paints the same bold cursor-zone affordance a panel
                // header drag does — a tab drag IS a panel drag (preview + result).
                let well = TabWellExternal::new(well_id.to_string(), Rc::clone(&reorganizer))
                    .with_drop_preview(Rc::clone(&preview));
                externals.push(ExtraExternal::new(well_id.to_string(), Box::new(well)));
            });
        }
        // (R686/R1081/R1082.1 §5.45 §5.51) The invoke (RPC) drive of dock
        // reorganize shares the SAME coordinator AND the SAME live preview as
        // the pointer panels, so an AI client driving reorganize through this
        // one canonical tag also observes the in-flight pointer drag via
        // query("drop_preview").
        externals.push(ExtraExternal::new(
            DOCK_REORGANIZE_TAG,
            Box::new(
                DockReorganizeExternal::from_reorganizer(Rc::clone(&reorganizer))
                    .with_drop_preview(Rc::clone(&preview)),
            ),
        ));
        externals.push(ExtraExternal::new(
            DOCK_UNDO_TAG,
            Box::new(UndoStackExternal::new(use_dock_undo())),
        ));
        // (R1135 §5.51.1) The toolbar float-policy toggle button — a second
        // ButtonExternal whose click flips collapse|placeholder (the human GUI
        // path for the R1134 policy the AI drives via `set_float_policy`). Click
        // routed to the `policy_btn.click` reducer arm.
        externals.push(ExtraExternal::new(
            POLICY_BTN_TAG,
            Box::new(ButtonExternal::new()),
        ));
        // (R1145 §5.51) The "Undock tab" button — a third ButtonExternal whose
        // click undocks the active tab back into a split (the human path for the
        // AI `undock_tab` invoke). Registered unconditionally so the External is
        // present whenever the button paints; the button only PAINTS while tabbed
        // (the view gate), and a click while nothing is tabbed is a no-op.
        externals.push(ExtraExternal::new(
            UNDOCK_BTN_TAG,
            Box::new(ButtonExternal::new()),
        ));
        externals
    }

    fn external_set_is_dynamic() -> bool {
        // (R689 §5.16 §5.35) The factory above walks the live
        // `Signal<Option<DockTopology>>` (R1084), so a runtime reorganize
        // that mints a `reorg-split-{n}` changes the returned tag set. Opt into
        // `CoreShell::reconcile_externals` so the new SplitterExternal
        // registers a routable target (and becomes drag-resizable);
        // every static binding leaves this at the `false` default.
        true
    }

    fn read_state(scene: &Scene) -> Self::State {
        if let Some(node) = scene.find_external_with_tag(VIEWPORT_BTN_TAG)
            && let Some(intro) = node.handle.introspect()
            && let Some(IntrospectValue::Text(name)) = intro.query("state")
        {
            return <Self::State as pinion_core::WidgetStateName>::from_name_or_default(&name);
        }
        ButtonState::Idle
    }

    fn view(state: Self::State, _frame: &Frame) -> Scene {
        // (R1105 §5.51) Single-window fallback — the default `view_for_window`
        // forwards here for `"main"`. The live render loop always calls
        // `view_for_window` (R670.B per-window dispatch); an RPC
        // `scene/snapshot` without `{window}` resolves through this path.
        view_main_dock(state)
    }

    fn event_name(event: Self::Event) -> &'static str {
        <Self::Event as pinion_core::WidgetEventName>::as_name(&event)
    }

    fn update(_state: Self::State, intent: &Intent) -> Vec<Command> {
        let tag = intent.tag_str();
        // (R1105 §5.51 §5.16 §5.41 PR-31) Tear-off family. A panel's
        // `DockPanelExternal` emits a BARE dock event (`tear_off`, …); the
        // intent-queue drain prefixes it with the panel's registration tag →
        // `{panel}.{event}` (the `intent_tag!` dotted wire form). Dispatch on
        // the event suffix; the payload carries the panel id (the SSOT for
        // which panel — the tag's prefix agrees). The editor wires all five
        // panels uniformly rather than per-panel constants (the flat demo's
        // 3-panel idiom would be 5×4 = 20 constants here).
        if let Some((_, event)) = tag.rsplit_once('.') {
            match event {
                // Toggle: float the panel into its own window, or dock it back.
                TEAR_OFF_EVENT => {
                    if let IntrospectValue::Text(panel) = &intent.payload {
                        toggle_panel_floating(panel);
                    }
                    return Vec::new();
                }
                // Live follow: a drag escaped every dock zone — ensure the
                // panel's floating window exists + write its position from the
                // forwarded (window-logical) cursor, desktop-converted.
                // Non-toggling (every per-move re-emit only repositions).
                TEAR_OFF_FOLLOW_EVENT => {
                    if let IntrospectValue::Json(v) = &intent.payload
                        && let (Some(panel), Some(x), Some(y)) = (
                            v.get("panel").and_then(serde_json::Value::as_str),
                            v.get("x").and_then(serde_json::Value::as_f64),
                            v.get("y").and_then(serde_json::Value::as_f64),
                        )
                    {
                        // R1107 — the window the cursor is measured in (null for
                        // the degenerate fallback → main); converts to a desktop
                        // position via the right origin.
                        let source_window =
                            v.get("source_window").and_then(serde_json::Value::as_str);
                        follow_panel_floating(panel, source_window, (x, y));
                    }
                    return Vec::new();
                }
                // (R1118) Window move: the panel's OWN floating window was
                // dragged by its title bar. Move it BY the grab-relative
                // displacement (dx, dy) — a delta added to the current position,
                // NOT a cursor placed via an origin. Distinct event from the
                // tear-off follow so the wire form is honest (a window move
                // carries a displacement, a tear-off carries a cursor).
                WINDOW_MOVE_EVENT => {
                    if let IntrospectValue::Json(v) = &intent.payload
                        && let (Some(panel), Some(dx), Some(dy)) = (
                            v.get("panel").and_then(serde_json::Value::as_str),
                            v.get("dx").and_then(serde_json::Value::as_f64),
                            v.get("dy").and_then(serde_json::Value::as_f64),
                        )
                    {
                        move_floating_window(panel, (dx, dy));
                    }
                    return Vec::new();
                }
                // Redock / restore: a drag that had torn the panel off ended
                // back in the dock or snapped back / cancelled — remove the
                // floating window this gesture created (idempotent).
                TEAR_OFF_REDOCK_EVENT => {
                    if let IntrospectValue::Text(panel) = &intent.payload {
                        redock_panel_floating(panel);
                    }
                    return Vec::new();
                }
                // (R1105/R1106/R1128/R1163b) Cross-window dock-at into the
                // slot-bearing main window: a floating panel dropped onto main's
                // dock (the live shell-composed `over_window`, or the AI-primary
                // `tear_off_redock_at` invoke). Only `MAIN_WINDOW_ID` hosts a panel
                // (a floater has no slot); a non-main target fires the intent (the
                // panel's `redock_at` diagnostic records it) but executes no move.
                // The drop is resolved through the SAME `resolve_drop` SSOT the
                // same-window drag + the on-target preview use — see
                // [`redock_cross_window`].
                TEAR_OFF_REDOCK_AT_EVENT => {
                    if let IntrospectValue::Json(v) = &intent.payload
                        && let (Some(panel), Some(window)) = (
                            v.get("panel").and_then(serde_json::Value::as_str),
                            v.get("window").and_then(serde_json::Value::as_str),
                        )
                        && window == MAIN_WINDOW_ID
                    {
                        redock_cross_window(panel, v);
                    }
                    return Vec::new();
                }
                _ => {}
            }
        }
        if tag == VIEWPORT_BTN_CLICK_INTENT_TAG {
            let counter = use_viewport_click_counter();
            counter.set(counter.get().wrapping_add(1));
        }
        // (R1135 §5.51.1) The toolbar toggle flips the shared coordinator's
        // FloatPolicy. `set_float_policy` writes the reactive `Signal`, so the
        // dock externals honour the new mode on the next float AND the toolbar
        // repaints with the flipped label — the SAME SSOT the AI `set_float_policy`
        // invoke drives, so the GUI + RPC paths stay consistent.
        if tag == POLICY_BTN_CLICK_INTENT_TAG {
            let reorganizer = use_editor_reorganizer();
            reorganizer.set_float_policy(toggled_float_policy(reorganizer.float_policy()));
        }
        // (R1145 §5.51) The "Undock tab" button pulls the active tab out of its
        // well into a split sibling — the human path for the AI `undock_tab`
        // invoke (both funnel through the shared reorganizer's one commit). A
        // no-op when nothing is tabbed (the button is hidden then anyway).
        if tag == UNDOCK_BTN_CLICK_INTENT_TAG {
            let _ = use_editor_reorganizer().undock_active_tab();
        }
        Vec::new()
    }
}

impl WidgetA11y for DockPanelsEditorView {
    /// R1095 §5.51 §5.27 §5.40 — the editor's AT contribution: the WAI-ARIA
    /// `tablist` / `tab` / `tabpanel` nodes for every `DockNode::Tabs` well
    /// in the live topology, so a screen reader announces the dock's tab
    /// wells + which tab is selected (R1083 tabbed docking + R1085
    /// `activate_tab` were create+AI-activate only — this makes them
    /// AT-navigable). Reads the same `use_editor_topology` signal the view
    /// walker renders from (one SSOT; `access_node` runs inside the shell's
    /// `root_owner` scope, so the `Owner::cache` hook resolves). Empty
    /// surface (`None`) / a Leaf/Split-only topology → no nodes.
    fn access_node(_state: &Self::State, focused: Option<&str>) -> Vec<AccessNode> {
        use_editor_topology()
            .get()
            .as_ref()
            .map(|topology| dock_tablist_access_nodes(topology, focused))
            .unwrap_or_default()
    }
}

impl WidgetView for DockPanelsEditorView {
    type Renderer = HelloDockPanelsEditorRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: MAIN_W,
            height: MAIN_H,
        }
    }

    /// (R1105 §5.51 §5.16 §5.41 PR-31) Opt into the reactive multi-window
    /// topology. The shell's `reconcile_windows` Effect subscribes this at
    /// boot + drives winit window add/drop on each `Signal::set`. Initial =
    /// the single main window; tear-off intents append `torn-{panel}`
    /// floating windows, dock-back intents remove them.
    fn windows_signal() -> Option<Rc<Signal<Vec<WindowSpec>>>> {
        Some(use_editor_windows())
    }

    /// (R1170 §5.16 §5.39) Per-window CLOSE: a torn-off panel's floating window
    /// (`torn-{panel}`) closes back to its DOCK (drop its `WindowSpec` → the
    /// placeholder reverts to live content), NOT by quitting the editor. The main
    /// window is not a `torn-` window, so its close falls through to the shell's
    /// app-exit default. Triggered by the chrome close button
    /// ([`view_floating_panel`] tags it [`WINDOW_CHROME_CLOSE_TAG`]).
    fn window_close_requested(window_id: &str) -> bool {
        match window_id.strip_prefix(DEFAULT_FLOATING_WINDOW_PREFIX) {
            Some(panel) => {
                redock_panel_floating(panel);
                true
            }
            None => false,
        }
    }

    /// (R1186 §5.16 §5.39) A torn-off panel's floating window IS resizable even
    /// though it draws NO shell chrome (`window_chrome == None` — its title bar is
    /// the dock HEADER, R1171). Pre-R1186 the resize border rode the chrome gate, so
    /// a controls-in-header floater had no edge / corner resize; this decouples them.
    /// The floating spec's `decorations:false` removed the OS frame, so this
    /// client-side border is the sole resize affordance. The main window returns
    /// `None` (derive from chrome) → it is OS-decorated, so the OS frame resizes it
    /// and no client border is drawn.
    fn window_resizable(window_id: &str) -> Option<bool> {
        window_id
            .strip_prefix(DEFAULT_FLOATING_WINDOW_PREFIX)
            .map(|_| true)
    }

    // (R1171 §5.16 §5.39) The editor no longer overrides `window_chrome`: a torn-off
    // panel's window controls (min / max / close) are rendered IN the panel HEADER
    // (`view_window_controls` → `view_dock_panel_with_actions`), one title bar that
    // is also the redock drag handle — NOT a separate shell-overlay the binding has
    // to dimension-match (the retired R1170 `controls_only` chrome). The shell still
    // routes a press on the control tags via `try_chrome_press` + the
    // `window_close_requested` seam below.

    /// (R1125/R1163b §5.51 §2 #7 PR-33) Render the cross-window dock drop-zone
    /// PREVIEW. The shell does the widget-agnostic half (resolves the incoming
    /// floater's drop onto this window, looks up the target's `panel_rect`, and runs
    /// this hook inside `root_owner.run` so the reactive reorganizer resolves); this
    /// supplies the dock-domain strip — the RESULT region of the zone under the
    /// cursor, in the bold redock tint. R1163b DERIVES the zone from the SAME
    /// [`resolve_drop`] SSOT the [`redock_cross_window`] reducer applies, so the
    /// cross-window preview == result by construction (was the legacy continuous
    /// `dock_drop_zone_normalized` + a duplicated `_placeholder` check — a
    /// two-geometry divergence vs the same-window banded path). The torn-slot
    /// placeholder fill (R1140) + the OUTER full-span perimeter (R1156) now live in
    /// `resolve_drop`. A `Dock`/`OuterDock` paints the zone overlay; a `Float` (a
    /// panel's dead-zone ring — "will float") / `SnapBack` paints nothing.
    fn dock_drop_preview(
        source_panel: &str,
        target_tag: &str,
        panel_rect: Rect,
        x_rel: f32,
        y_rel: f32,
    ) -> Option<Scene> {
        let reorganizer = use_editor_reorganizer();
        let point = DropPoint {
            tag: target_tag.to_string(),
            x_rel,
            y_rel,
        };
        // R1139 — the bolder cross-window redock tint (over opaque content), not
        // the subtler in-window highlight; the overlay adds an opaque accent
        // border so the result region reads regardless of the content behind it.
        let tint = dock_redock_preview_tint(&Theme::default());
        match resolve_drop(
            Some(&point),
            source_panel,
            |t| reorganizer.is_panel(t),
            reorganizer.tabbing(),
        ) {
            // (R1167 §5.51) A full-span OUTER dock previews the thin perimeter band
            // `dock_panel_outer` actually lands at (`panel_rect` is the WHOLE window
            // here), so the cross-window outer preview == result — the inner 50%
            // `dock_drop_preview_overlay` over-showed it pre-R1167.
            DropResolution::OuterDock { edge } => {
                dock_outer_preview_overlay(panel_rect, edge, tint)
            }
            DropResolution::Dock { zone, .. } => dock_drop_preview_overlay(panel_rect, zone, tint),
            DropResolution::Float | DropResolution::SnapBack { .. } => None,
        }
    }

    // (R1168 dropped the `dock_zone_guide` override — the static guide is retired;
    // the cursor-driven `dock_drop_preview` is the sole drop affordance.)

    /// (R1105 §5.51 §5.16 PR-31) Per-window paint dispatch. The main window
    /// paints the dock layout; a floating window (id prefix `torn-`) paints
    /// its hosted panel via [`view_floating_panel`]. An unrecognised id falls
    /// back to the main dock (defensive — never blank-screen).
    fn view_for_window(window_id: &str, state: Self::State, _frame: &Frame) -> Scene {
        match window_id {
            id if id == MAIN_WINDOW_ID => view_main_dock(state),
            other => match other.strip_prefix(DEFAULT_FLOATING_WINDOW_PREFIX) {
                Some(panel_id) => view_floating_panel(panel_id, state),
                None => view_main_dock(state),
            },
        }
    }

    /// (R1105 §5.51 §5.40 §5.27 PR-31) Per-window AT contribution. The dock
    /// tab-well `tablist` nodes (R1095) belong to the main window where the
    /// dock paints; a torn-off floating window hosts a single non-tabbed
    /// panel, so it contributes none. Without this gate the default
    /// `access_node_for_window` forwards [`WidgetA11y::access_node`] to EVERY
    /// window, ghosting the main dock's tablist nodes into each floater (no
    /// bounds / name there). Only the main window forwards.
    fn access_node_for_window(
        window_id: &str,
        state: &Self::State,
        focused: Option<&str>,
    ) -> Vec<AccessNode> {
        if window_id == MAIN_WINDOW_ID {
            <Self as WidgetA11y>::access_node(state, focused)
        } else {
            Vec::new()
        }
    }
}

fn main() {
    pinion_shell::run::<DockPanelsEditorView>();
}

// ─── tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! R685 §5.16 §5.41 §5.49 — `hello-dock-panels-editor` binding
    //! tests. Pins the 2nd-dock-consumer + view-fn invariants:
    //!
    //! 1. Topology shape — 5 leaves + 4 splits + canonical
    //!    depth-first panel order.
    //! 2. Topology serde round-trip — every panel + slot label +
    //!    split orientation/ratio survives JSON.
    //! 3. View fn produces a non-empty Scene with the outer
    //!    splitter Container at root.
    //! 4. Each panel id is reachable inside the rendered scene.
    //! 5. Click intent reducer bumps the counter Signal.
    //! 6. `SplitterExternal` extra externals registered for each
    //!    of the 4 splits with the canonical tags.

    use super::*;
    use pinion_core::reactive::Owner;
    use pinion_widget_paint::dock::{
        DockReorganizeIntent, DockSplitPosition, FloatPolicy, PLACEHOLDER_TAG_SUFFIX,
    };
    use pinion_widget_paint::splitter::SplitterOrientation;

    #[test]
    fn r1140_dock_drop_preview_full_for_torn_slot_split_for_live_panel() {
        // R1140/R1163b §5.51 PR-39 — a floater dragged back over its OWN torn slot
        // (placeholder tag) previews the WHOLE slot (Center / dock-back full), not a
        // cursor-zone edge split: you fill an emptied slot, you do not split it. A
        // live-panel target keeps the edge-split classification. R1163b: the hook
        // resolves through `resolve_drop` reading the owner-scoped editor reorganizer
        // (is_panel / tabbing), so run it in an owner (as the shell does).
        run_in_owner(|| {
            let panel = Rect::new(0, 0, 200, 100);
            let placeholder = format!("properties{PLACEHOLDER_TAG_SUFFIX}");
            let Some(Scene::Box(full)) = <DockPanelsEditorView as WidgetView>::dock_drop_preview(
                "properties",
                &placeholder,
                panel,
                0.1,
                0.5,
            ) else {
                panic!("a torn slot paints a preview Box");
            };
            assert_eq!(
                full.rect, panel,
                "a torn slot previews the WHOLE slot (Center), even with an edge x_rel",
            );
            let Some(Scene::Box(half)) = <DockPanelsEditorView as WidgetView>::dock_drop_preview(
                "properties",
                "viewport",
                panel,
                0.1,
                0.5,
            ) else {
                panic!("a live panel paints a preview Box");
            };
            assert!(
                half.rect.w < panel.w,
                "a live panel target splits at the cursor edge (w={} < {}), not full",
                half.rect.w,
                panel.w,
            );
        });
    }

    #[test]
    fn r1171_floating_panel_header_has_window_controls_and_close_docks_back() {
        // R1171 §5.16 — a torn-off panel's window controls (min/max/close) are
        // rendered IN the panel HEADER (controls-in-header, NOT a shell overlay), so
        // `view_floating_panel`'s scene carries the three control hit tags; and the
        // close routes through `window_close_requested` to dock the panel BACK
        // (drop its WindowSpec), only the main window's close exits the app.
        run_in_owner(|| {
            // The floating panel's header carries the three window-control tags.
            let floater = view_floating_panel("properties", ButtonState::Idle);
            for tag in [
                WINDOW_CHROME_MINIMIZE_TAG,
                WINDOW_CHROME_MAXIMIZE_TAG,
                WINDOW_CHROME_CLOSE_TAG,
            ] {
                assert!(
                    scene_has_tag(&floater, tag),
                    "the floating panel header carries the {tag} control",
                );
            }
            // window_close_requested: float properties, then close its window.
            toggle_panel_floating("properties");
            assert!(
                is_panel_floating(&use_editor_windows().get(), "properties"),
                "properties is floating after the tear-off",
            );
            let torn = floating_window_id("properties");
            let handled = <DockPanelsEditorView as WidgetView>::window_close_requested(&torn);
            assert!(handled, "a torn window's close is HANDLED (no app exit)");
            assert!(
                !is_panel_floating(&use_editor_windows().get(), "properties"),
                "★the close docked the panel BACK (its WindowSpec is gone)",
            );
            // The main window's close is NOT handled → the shell exits the app.
            assert!(
                !<DockPanelsEditorView as WidgetView>::window_close_requested(MAIN_WINDOW_ID),
                "the main window's close is unhandled (the app exits)",
            );
        });
    }

    #[test]
    fn r1186_floating_window_is_chromeless_yet_resizable() {
        // R1186 §5.16 §5.39 (PR-43) — a torn-off panel's floating window is the
        // controls-in-header shape: it draws NO shell chrome (`window_chrome ==
        // None` — its title bar is the dock HEADER, R1171) YET is client-side
        // resizable (`window_resizable == Some(true)`), the two decoupled. The
        // shell's `chromeless_resizable_window_omits_top_but_keeps_sides_and_bottom`
        // test proves
        // that exact shape (chrome None + resizable Some(true)) gets the resize
        // border injected; this asserts the editor's torn window IS that shape.
        let torn = floating_window_id("properties");
        assert_eq!(
            <DockPanelsEditorView as WidgetView>::window_chrome(&torn),
            None,
            "the floater's title bar is its dock header — no separate shell chrome",
        );
        assert_eq!(
            <DockPanelsEditorView as WidgetView>::window_resizable(&torn),
            Some(true),
            "★a torn-off floating window is client-side resizable despite no chrome",
        );
        // The main window derives resize from chrome (None) — it is OS-decorated,
        // so the OS frame resizes it and the shell draws no client resize border.
        assert_eq!(
            <DockPanelsEditorView as WidgetView>::window_resizable(MAIN_WINDOW_ID),
            None,
            "the main window derives resize from chrome (OS frame resizes it)",
        );
    }

    /// Depth-first: does `scene` (or a descendant) carry `tag`?
    fn scene_has_tag(scene: &Scene, tag: &str) -> bool {
        if scene.tag() == Some(tag) {
            return true;
        }
        match scene {
            Scene::Container(c) => c.children.iter().any(|ch| scene_has_tag(ch, tag)),
            Scene::Scroll(s) => scene_has_tag(&s.content, tag),
            _ => false,
        }
    }

    #[test]
    fn r1167_dock_drop_preview_outer_is_a_thin_full_span_band() {
        // R1167 §5.51 — a cross-window OUTER dock (target == the OUTER sentinel,
        // `panel_rect` == the WHOLE window) previews the THIN full-span band
        // `dock_panel_outer` lands at (22%), NOT the inner 50% split — preview ==
        // result for the outer dock (pre-R1167 the outer case reused the 50%
        // `dock_drop_preview_overlay`). Resolved through `resolve_drop` in an owner,
        // exactly like the inner case (r1140).
        run_in_owner(|| {
            let win = Rect::new(0, 0, 1000, 800);
            // x_rel/y_rel near the BOTTOM edge → `outer_zone_for` picks Bottom.
            let Some(Scene::Box(band)) = <DockPanelsEditorView as WidgetView>::dock_drop_preview(
                "properties",
                OUTER_DOCK_ZONE_TAG,
                win,
                0.5,
                0.95,
            ) else {
                panic!("an outer dock paints a preview Box");
            };
            assert_eq!(
                band.rect.w, win.w,
                "the outer band spans the full window width"
            );
            assert_eq!(
                band.rect.h,
                win.h * 22 / 100,
                "a thin 22% band (== dock_panel_outer's ratio), not the inner 50%",
            );
            assert_eq!(
                band.rect.y,
                win.h - band.rect.h,
                "the band is flush to the bottom edge",
            );
            assert!(
                band.rect.h < win.h / 2,
                "an outer band is thinner than an inner 50% split (h={})",
                band.rect.h,
            );
        });
    }

    use std::borrow::Cow;

    fn run_in_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    #[test]
    fn r685_editor_topology_has_5_leaves_and_4_splits() {
        let topology = build_editor_topology();
        assert_eq!(topology.leaf_count(), 5);
        assert_eq!(topology.split_count(), 4);
    }

    #[test]
    fn r685_editor_topology_panel_ids_depth_first_order() {
        let topology = build_editor_topology();
        assert_eq!(
            topology.panel_ids(),
            vec![
                TOOLBAR_PANEL_TAG,
                OUTLINER_PANEL_TAG,
                VIEWPORT_PANEL_TAG,
                PROPERTIES_PANEL_TAG,
                CONSOLE_PANEL_TAG,
            ],
            "panel_ids walk = toolbar → outliner → viewport → properties → console",
        );
    }

    #[test]
    fn r685_editor_topology_split_ids_depth_first_pre_order() {
        let topology = build_editor_topology();
        assert_eq!(
            topology.split_ids(),
            vec![
                SPLIT_OUTER_TAG,
                SPLIT_INNER_V_TAG,
                SPLIT_MIDDLE_H_TAG,
                SPLIT_INNER_H_TAG
            ],
            "split_ids walk in pre-order: outer → inner_v → middle_h → inner_h",
        );
    }

    #[test]
    fn r685_editor_topology_serde_json_round_trip() {
        let topology = build_editor_topology();
        let serialized = serde_json::to_string(&topology).expect("serialize");
        let parsed: DockTopology = serde_json::from_str(&serialized).expect("parse back");
        assert_eq!(parsed, topology, "5-pane topology round-trips through JSON");
    }

    #[test]
    fn r685_editor_default_split_ratios_within_unit_interval() {
        for r in [
            SPLIT_OUTER_RATIO_DEFAULT,
            SPLIT_INNER_V_RATIO_DEFAULT,
            SPLIT_MIDDLE_H_RATIO_DEFAULT,
            SPLIT_INNER_H_RATIO_DEFAULT,
        ] {
            assert!(r > 0.0 && r < 1.0, "ratio {r} must be in (0,1)");
        }
    }

    #[test]
    fn r685_editor_panel_content_dispatch_known_panels() {
        run_in_owner(|| {
            let theme = Theme::light();
            for &panel_id in &[
                TOOLBAR_PANEL_TAG,
                OUTLINER_PANEL_TAG,
                VIEWPORT_PANEL_TAG,
                PROPERTIES_PANEL_TAG,
                CONSOLE_PANEL_TAG,
            ] {
                let scene = panel_content_for(panel_id, ButtonState::Idle, &theme);
                assert!(
                    matches!(scene, Scene::Container(_)),
                    "panel '{panel_id}' should render a Container",
                );
            }
        });
    }

    #[test]
    fn r685_editor_panel_content_dispatch_unknown_panel_falls_back_to_text() {
        run_in_owner(|| {
            let theme = Theme::light();
            let scene = panel_content_for("nonexistent", ButtonState::Idle, &theme);
            assert!(matches!(scene, Scene::Text(_)));
        });
    }

    #[test]
    fn r685_editor_outliner_renders_one_row_per_outliner_rows_entry() {
        run_in_owner(|| {
            let theme = Theme::light();
            let scene = view_outliner_content(&theme);
            let Scene::Container(outer) = scene else {
                panic!("outliner body should be a Container");
            };
            assert_eq!(outer.children.len(), OUTLINER_ROWS.len());
        });
    }

    #[test]
    fn r685_editor_properties_renders_one_row_per_property_entry() {
        run_in_owner(|| {
            let theme = Theme::light();
            let scene = view_properties_content(&theme);
            let Scene::Container(outer) = scene else {
                panic!("properties body should be a Container");
            };
            assert_eq!(outer.children.len(), PROPERTIES_ROWS.len());
        });
    }

    #[test]
    fn r685_editor_console_renders_one_row_per_console_log_line() {
        run_in_owner(|| {
            let theme = Theme::light();
            let scene = view_console_content(&theme);
            let Scene::Container(outer) = scene else {
                panic!("console body should be a Container");
            };
            assert_eq!(outer.children.len(), CONSOLE_ROWS.len());
        });
    }

    #[test]
    fn r685_editor_viewport_content_contains_header_counter_button() {
        run_in_owner(|| {
            let theme = Theme::light();
            let scene = view_viewport_content(ButtonState::Idle, &theme);
            let Scene::Container(outer) = scene else {
                panic!("viewport body should be a Container");
            };
            assert_eq!(outer.children.len(), 3, "header + counter + button");
        });
    }

    #[test]
    fn r685_editor_view_root_is_outer_splitter_container() {
        run_in_owner(|| {
            let frame = Frame::default();
            let scene = <DockPanelsEditorView as WidgetCore>::view(ButtonState::Idle, &frame);
            let Scene::Container(outer) = scene else {
                panic!("editor view root should be a splitter Container");
            };
            assert_eq!(outer.tag.as_deref(), Some(SPLIT_OUTER_TAG));
        });
    }

    #[test]
    fn r685_editor_view_renders_all_5_panels_in_scene() {
        run_in_owner(|| {
            let frame = Frame::default();
            let scene = <DockPanelsEditorView as WidgetCore>::view(ButtonState::Idle, &frame);
            let serialized = format!("{scene:?}");
            for &panel_id in &[
                TOOLBAR_PANEL_TAG,
                OUTLINER_PANEL_TAG,
                VIEWPORT_PANEL_TAG,
                PROPERTIES_PANEL_TAG,
                CONSOLE_PANEL_TAG,
            ] {
                assert!(
                    serialized.contains(panel_id),
                    "scene should contain panel tag '{panel_id}'",
                );
            }
        });
    }

    #[test]
    fn r685_editor_view_renders_viewport_button_tag() {
        run_in_owner(|| {
            let frame = Frame::default();
            let scene = <DockPanelsEditorView as WidgetCore>::view(ButtonState::Idle, &frame);
            let serialized = format!("{scene:?}");
            assert!(serialized.contains(VIEWPORT_BTN_TAG));
        });
    }

    #[test]
    fn r685_editor_view_renders_all_4_splitter_tags() {
        run_in_owner(|| {
            let frame = Frame::default();
            let scene = <DockPanelsEditorView as WidgetCore>::view(ButtonState::Idle, &frame);
            let serialized = format!("{scene:?}");
            for &split_tag in &[
                SPLIT_OUTER_TAG,
                SPLIT_INNER_V_TAG,
                SPLIT_MIDDLE_H_TAG,
                SPLIT_INNER_H_TAG,
            ] {
                assert!(
                    serialized.contains(split_tag),
                    "scene should contain splitter tag '{split_tag}'",
                );
            }
        });
    }

    #[test]
    fn r686_editor_create_extra_externals_registers_splitters_panels_reorganize() {
        run_in_owner(|| {
            let externals = <DockPanelsEditorView as WidgetCore>::create_extra_externals();
            // 4 SplitterExternals + 5 R742 DockPanelExternals (one per leaf,
            // R1082) + 1 DockReorganizeExternal (R686) + 1 UndoStackExternal
            // (R749 §5.52 reorganize history) + 1 float-policy toggle button
            // (R1135 §5.51.1) + 1 undock-tab button (R1145 §5.51).
            assert_eq!(externals.len(), 13);
            let tags: Vec<&str> = externals.iter().map(|e| e.tag.as_ref()).collect();
            for split in [
                SPLIT_OUTER_TAG,
                SPLIT_INNER_V_TAG,
                SPLIT_MIDDLE_H_TAG,
                SPLIT_INNER_H_TAG,
            ] {
                assert!(tags.contains(&split), "splitter {split} registered");
            }
            for panel in [
                TOOLBAR_PANEL_TAG,
                OUTLINER_PANEL_TAG,
                VIEWPORT_PANEL_TAG,
                PROPERTIES_PANEL_TAG,
                CONSOLE_PANEL_TAG,
            ] {
                assert!(
                    tags.contains(&panel),
                    "R742 panel external {panel} registered"
                );
            }
            assert!(tags.contains(&DOCK_REORGANIZE_TAG));
            assert!(tags.contains(&DOCK_UNDO_TAG));
            assert!(
                tags.contains(&POLICY_BTN_TAG),
                "R1135 policy toggle registered"
            );
            assert!(
                tags.contains(&UNDOCK_BTN_TAG),
                "R1145 undock-tab button registered"
            );
        });
    }

    #[test]
    fn r1135_policy_toggle_button_flips_float_policy_and_label() {
        // R1135 §5.51.1 — the toolbar toggle is the HUMAN GUI path for the R1134
        // collapse|placeholder policy: clicking flips the shared coordinator's
        // FloatPolicy, and the reactive label repaints to the new mode.
        run_in_owner(|| {
            let reorg = use_editor_reorganizer();
            assert_eq!(
                reorg.float_policy(),
                FloatPolicy::Placeholder,
                "default placeholder"
            );
            let scene0 =
                <DockPanelsEditorView as WidgetCore>::view(ButtonState::Idle, &Frame::default());
            assert!(
                format!("{scene0:?}").contains("torn slot: placeholder"),
                "toolbar paints the current placeholder mode",
            );
            let click = Intent {
                tag: Cow::Borrowed(POLICY_BTN_CLICK_INTENT_TAG),
                payload: IntrospectValue::Null,
            };
            let cmds = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &click);
            assert!(cmds.is_empty(), "no Commands for the toggle");
            assert_eq!(
                reorg.float_policy(),
                FloatPolicy::Collapse,
                "click flipped to collapse"
            );
            let scene1 =
                <DockPanelsEditorView as WidgetCore>::view(ButtonState::Idle, &Frame::default());
            assert!(
                format!("{scene1:?}").contains("torn slot: collapse"),
                "label repaints to collapse (reactive Signal)",
            );
            let _ = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &click);
            assert_eq!(
                reorg.float_policy(),
                FloatPolicy::Placeholder,
                "second click flips back"
            );
        });
    }

    #[test]
    fn r1133_reconstructed_external_rehydrates_floating_lifecycle() {
        // R1133 §5.51.1 §2 #5 — the external factory re-runs on EVERY topology
        // change (R688 reconcile), rebuilding every DockPanelExternal. A panel
        // that is floating at that moment must NOT have its lifecycle chart reset
        // to Docked (the latent reconstruct-while-floating desync). This drives
        // the reconstruct: float viewport, then re-run create_extra_externals
        // (what a reorganize triggers) and assert the rebuilt viewport external
        // re-hydrates to Floating while a docked panel stays Docked.
        run_in_owner(|| {
            toggle_panel_floating(VIEWPORT_PANEL_TAG); // viewport now floating
            let externals = <DockPanelsEditorView as WidgetCore>::create_extra_externals();
            let lifecycle = |tag: &str| {
                externals
                    .iter()
                    .find(|e| e.tag.as_ref() == tag)
                    .and_then(|e| e.handle.introspect())
                    .and_then(|i| i.query("lifecycle"))
            };
            assert_eq!(
                lifecycle(VIEWPORT_PANEL_TAG),
                Some(IntrospectValue::Text("Floating".to_string())),
                "a reconstructed floating panel re-hydrates its chart from windows_signal",
            );
            assert_eq!(
                lifecycle(OUTLINER_PANEL_TAG),
                Some(IntrospectValue::Text("Docked".to_string())),
                "a docked panel's reconstructed chart stays Docked",
            );
        });
    }

    #[test]
    fn r1134_external_union_registers_a_collapsed_floated_panel() {
        // R1134 §5.51.1 step② — under Collapse a float REMOVES viewport's leaf, so
        // it leaves panel_ids(). The factory's (topology ∪ floating) union must
        // still register viewport's DockPanelExternal from the windows truth (else
        // its floating header loses its drag / dock-back), re-hydrated to Floating.
        run_in_owner(|| {
            let reorg = use_editor_reorganizer();
            reorg.set_float_policy(FloatPolicy::Collapse);
            // The two sides of a release-as-floated under Collapse: collapse the
            // leaf (topology) + push the floating window (windows).
            reorg.float_out_panel(VIEWPORT_PANEL_TAG).unwrap();
            toggle_panel_floating(VIEWPORT_PANEL_TAG);
            assert!(
                !use_editor_topology()
                    .get()
                    .unwrap()
                    .panel_ids()
                    .contains(&VIEWPORT_PANEL_TAG),
                "viewport collapsed out of the topology (its slot reflowed)",
            );
            let externals = <DockPanelsEditorView as WidgetCore>::create_extra_externals();
            let viewport = externals
                .iter()
                .find(|e| e.tag.as_ref() == VIEWPORT_PANEL_TAG);
            assert!(
                viewport.is_some(),
                "step② union registers the collapsed (floated) panel's external",
            );
            assert_eq!(
                viewport
                    .and_then(|e| e.handle.introspect())
                    .and_then(|i| i.query("lifecycle")),
                Some(IntrospectValue::Text("Floating".to_string())),
                "the union'd external re-hydrates its chart to Floating",
            );
        });
    }

    #[test]
    fn r1134_editor_collapse_reflows_topology_then_restores_home() {
        // R1134 §5.51.1 — collapse viewport in the editor's real 5-pane topology:
        // the slot reflows (4 panels), and a dock-back restores it home (5 panels).
        run_in_owner(|| {
            let reorg = use_editor_reorganizer();
            reorg.set_float_policy(FloatPolicy::Collapse);
            assert_eq!(use_editor_topology().get().unwrap().panel_ids().len(), 5);
            reorg.float_out_panel(VIEWPORT_PANEL_TAG).unwrap();
            let collapsed = use_editor_topology().get().unwrap();
            assert_eq!(
                collapsed.panel_ids().len(),
                4,
                "viewport's slot collapsed (reflow)"
            );
            assert!(!collapsed.panel_ids().contains(&VIEWPORT_PANEL_TAG));
            reorg.restore_panel_home(VIEWPORT_PANEL_TAG).unwrap();
            let restored = use_editor_topology().get().unwrap();
            assert_eq!(
                restored.panel_ids().len(),
                5,
                "viewport restored to its home anchor"
            );
            assert!(restored.panel_ids().contains(&VIEWPORT_PANEL_TAG));
        });
    }

    #[test]
    fn r1082_panel_externals_share_one_coordinator_with_the_invoke_path() {
        run_in_owner(|| {
            // The cached coordinator is the SAME Rc the invoke external + every
            // panel external hold, so a split minted through one is visible to
            // all (no reorg-split-{n} collision across drives).
            let a = use_editor_reorganizer();
            let b = use_editor_reorganizer();
            assert!(Rc::ptr_eq(&a, &b), "Owner::cache memoises the coordinator");
            let preview_a = use_drop_preview();
            let preview_b = use_drop_preview();
            assert!(
                Rc::ptr_eq(&preview_a, &preview_b),
                "one shared preview signal"
            );
        });
    }

    #[test]
    fn r686_use_editor_topology_seeded_and_memoised() {
        run_in_owner(|| {
            let a = use_editor_topology();
            let b = use_editor_topology();
            assert!(
                Rc::ptr_eq(&a, &b),
                "Owner::cache memoises the topology signal"
            );
            assert_eq!(
                a.get().unwrap().panel_ids(),
                vec!["toolbar", "outliner", "viewport", "properties", "console"],
            );
        });
    }

    #[test]
    fn r686_reorganize_external_swap_reflows_topology_and_view() {
        run_in_owner(|| {
            // The external registered by create_extra_externals shares
            // this same Owner::cache topology signal.
            let topo = use_editor_topology();
            let ext = DockReorganizeExternal::new(Rc::clone(&topo));
            ext.apply_intent(&DockReorganizeIntent::Swap {
                source: "viewport".into(),
                target: "console".into(),
            })
            .unwrap();
            // Signal now holds the swapped topology.
            assert_eq!(
                topo.get().unwrap().panel_ids(),
                vec!["toolbar", "outliner", "console", "properties", "viewport"],
            );
            // The view fn reads the mutated topology + still renders the
            // viewport button (its content moved with the panel id).
            let scene =
                <DockPanelsEditorView as WidgetCore>::view(ButtonState::Idle, &Frame::default());
            assert!(format!("{scene:?}").contains(VIEWPORT_BTN_TAG));
        });
    }

    #[test]
    fn r686_view_renders_runtime_reorg_split_id_without_panic() {
        run_in_owner(|| {
            // Mint a runtime split id the way a SplitInsert gesture does
            // and push it into the live topology. The view fn keys
            // use_split_ratio on the raw id (Cow) — a static-str bridge
            // would `unreachable!` here; the R686 raw-id path must not.
            let topo = use_editor_topology();
            let mutated = topo
                .get()
                .unwrap()
                .remove_leaf("console")
                .unwrap()
                .split_leaf_into(
                    "viewport",
                    "console",
                    "reorg-split-0",
                    SplitterOrientation::Vertical,
                    0.5,
                    DockSplitPosition::Second,
                )
                .unwrap();
            topo.set(Some(mutated));
            let scene =
                <DockPanelsEditorView as WidgetCore>::view(ButtonState::Idle, &Frame::default());
            assert!(
                format!("{scene:?}").contains("reorg-split-0"),
                "view renders the runtime-minted split tag",
            );
        });
    }

    #[test]
    fn r685_editor_viewport_click_intent_bumps_counter() {
        run_in_owner(|| {
            let counter = use_viewport_click_counter();
            let before = counter.get();
            let intent = Intent {
                tag: Cow::Borrowed(VIEWPORT_BTN_CLICK_INTENT_TAG),
                payload: IntrospectValue::Null,
            };
            let commands = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert!(commands.is_empty(), "no Commands emitted for click");
            assert_eq!(counter.get(), before + 1);
        });
    }

    #[test]
    fn r685_editor_unrelated_intent_leaves_counter_unchanged() {
        run_in_owner(|| {
            let counter = use_viewport_click_counter();
            let before = counter.get();
            let intent = Intent {
                tag: Cow::Borrowed(intent_tag!("unrelated_widget", "click")),
                payload: IntrospectValue::Null,
            };
            let _ = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert_eq!(counter.get(), before, "counter untouched");
        });
    }

    #[test]
    fn r685_editor_initial_size_strategy_fixed_main_dimensions() {
        let SizeStrategy::Fixed { width, height } =
            <DockPanelsEditorView as WidgetView>::initial_size_strategy()
        else {
            panic!("expected Fixed strategy");
        };
        assert_eq!(width, MAIN_W);
        assert_eq!(height, MAIN_H);
    }

    #[test]
    fn r685_editor_title_contains_r685_marker() {
        let title = <DockPanelsEditorView as WidgetCore>::title();
        assert!(title.contains("R685"));
        assert!(title.contains("5-pane"));
    }

    #[test]
    fn r685_editor_tag_is_viewport_button() {
        assert_eq!(
            <DockPanelsEditorView as WidgetCore>::tag(),
            VIEWPORT_BTN_TAG,
        );
    }

    #[test]
    fn r685_editor_read_state_default_idle_when_no_button_external() {
        run_in_owner(|| {
            let empty = Scene::Container(ContainerNode::new(vec![]));
            let state = <DockPanelsEditorView as WidgetCore>::read_state(&empty);
            assert_eq!(state, ButtonState::Idle);
        });
    }

    #[test]
    fn r685_editor_topology_split_orientations_match_layout_intent() {
        let topology = build_editor_topology();
        let DockNode::Split {
            orientation: outer_o,
            first: outer_first,
            second: outer_second,
            ..
        } = topology.root()
        else {
            panic!("root is Split");
        };
        assert_eq!(*outer_o, SplitterOrientation::Vertical);
        assert!(matches!(outer_first.as_ref(), DockNode::Leaf { .. }));
        let DockNode::Split {
            orientation: inner_v_o,
            first: inner_v_first,
            second: inner_v_second,
            ..
        } = outer_second.as_ref()
        else {
            panic!("inner V is Split");
        };
        assert_eq!(*inner_v_o, SplitterOrientation::Vertical);
        let DockNode::Split {
            orientation: middle_h_o,
            ..
        } = inner_v_first.as_ref()
        else {
            panic!("middle H is Split");
        };
        assert_eq!(*middle_h_o, SplitterOrientation::Horizontal);
        assert!(matches!(inner_v_second.as_ref(), DockNode::Leaf { .. }));
    }

    #[test]
    fn r685_editor_panel_body_font_size_within_m3_dense_row_range() {
        // M3 Body Medium 14 sp; dense pro-tool surfaces collapse to
        // 12-13 sp. Pin the editor binding stays in the dense range
        // even if PANEL_BODY_FONT_PX is bumped in a future round.
        let px = PANEL_BODY_FONT_PX;
        assert!(
            (12..=14).contains(&px),
            "PANEL_BODY_FONT_PX {px} not in [12,14]"
        );
    }

    #[test]
    fn r685_editor_outliner_rows_contain_scene_root_label() {
        assert!(
            OUTLINER_ROWS.iter().any(|row| row.trim() == "Scene"),
            "outliner should surface the scene root label",
        );
    }

    #[test]
    fn r685_editor_properties_rows_contain_canonical_transform_keys() {
        let keys: Vec<&str> = PROPERTIES_ROWS.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"Position"));
        assert!(keys.contains(&"Rotation"));
        assert!(keys.contains(&"Scale"));
    }

    #[test]
    fn r685_editor_console_rows_include_info_and_warn_levels() {
        let any_info = CONSOLE_ROWS.iter().any(|row| row.contains("[info]"));
        let any_warn = CONSOLE_ROWS.iter().any(|row| row.contains("[warn]"));
        assert!(any_info, "console must include at least one info line");
        assert!(any_warn, "console must include at least one warn line");
    }

    #[test]
    fn r685_editor_toolbar_label_lists_canonical_editor_menus() {
        for menu in &["File", "Edit", "View", "Window", "Help"] {
            assert!(
                TOOLBAR_LABEL.contains(menu),
                "toolbar should list canonical {menu} menu",
            );
        }
    }

    #[test]
    fn r685_editor_use_split_ratio_returns_owner_cached_handle() {
        run_in_owner(|| {
            let a = use_split_ratio(SPLIT_OUTER_TAG, SPLIT_OUTER_RATIO_DEFAULT);
            let b = use_split_ratio(SPLIT_OUTER_TAG, SPLIT_OUTER_RATIO_DEFAULT);
            assert!(
                Rc::ptr_eq(&a, &b),
                "use_split_ratio is Owner::cache-memoised by id",
            );
        });
    }

    // ─── R1105 §5.51 §5.16 §5.41 PR-31 float-to-window (2nd consumer) ───

    /// Build the `{panel}.{event}` namespaced intent tag the intent-queue
    /// drain produces for a `DockPanelExternal` registered at `panel`.
    fn tear_off_tag(panel: &str, event: &str) -> Cow<'static, str> {
        Cow::Owned(format!("{panel}.{event}"))
    }

    #[test]
    fn r1105_editor_windows_seeded_single_main_and_memoised() {
        run_in_owner(|| {
            let a = use_editor_windows();
            let b = use_editor_windows();
            assert!(
                Rc::ptr_eq(&a, &b),
                "Owner::cache memoises the windows signal"
            );
            let specs = a.get();
            assert_eq!(specs.len(), 1, "boot = single main window");
            assert_eq!(specs[0].id.as_ref(), MAIN_WINDOW_ID);
            assert!(
                specs[0].position.is_none(),
                "main is WM-placed (null position)"
            );
        });
    }

    #[test]
    fn r1105_floating_window_id_uses_torn_prefix() {
        assert_eq!(floating_window_id(VIEWPORT_PANEL_TAG), "torn-viewport");
        assert_eq!(floating_window_id(OUTLINER_PANEL_TAG), "torn-outliner");
        assert!(
            floating_window_id(PROPERTIES_PANEL_TAG).starts_with(DEFAULT_FLOATING_WINDOW_PREFIX)
        );
    }

    #[test]
    fn r1105_floating_window_positions_cascade_distinctly() {
        // Each tearable panel opens at a distinct cascade offset so multiple
        // tear-offs do not stack exactly.
        let positions: Vec<(i32, i32)> = [
            TOOLBAR_PANEL_TAG,
            OUTLINER_PANEL_TAG,
            VIEWPORT_PANEL_TAG,
            PROPERTIES_PANEL_TAG,
            CONSOLE_PANEL_TAG,
        ]
        .iter()
        .map(|p| floating_window_position(p))
        .collect();
        let unique: std::collections::BTreeSet<_> = positions.iter().collect();
        assert_eq!(
            unique.len(),
            positions.len(),
            "no two panels share a cascade spot"
        );
    }

    #[test]
    fn r1105_toggle_panel_floating_adds_then_removes_torn_window() {
        run_in_owner(|| {
            let windows = use_editor_windows();
            assert!(!is_panel_floating(&windows.get(), VIEWPORT_PANEL_TAG));
            toggle_panel_floating(VIEWPORT_PANEL_TAG);
            let after = windows.get();
            assert_eq!(after.len(), 2, "main + torn-viewport");
            assert!(is_panel_floating(&after, VIEWPORT_PANEL_TAG));
            let torn = after
                .iter()
                .find(|w| w.id.as_ref() == "torn-viewport")
                .expect("torn-viewport spec present");
            assert!(
                torn.position.is_some(),
                "floating window opens at a declared position"
            );
            // Toggle again → dock back.
            toggle_panel_floating(VIEWPORT_PANEL_TAG);
            assert_eq!(
                windows.get().len(),
                1,
                "dock-back removes the floating window"
            );
        });
    }

    #[test]
    fn r1105_redock_panel_floating_idempotent_when_docked() {
        run_in_owner(|| {
            let windows = use_editor_windows();
            // No floating window for outliner → redock is a no-op.
            redock_panel_floating(OUTLINER_PANEL_TAG);
            assert_eq!(windows.get().len(), 1);
            // Float then redock removes it.
            toggle_panel_floating(OUTLINER_PANEL_TAG);
            assert_eq!(windows.get().len(), 2);
            redock_panel_floating(OUTLINER_PANEL_TAG);
            assert_eq!(windows.get().len(), 1);
        });
    }

    #[test]
    fn r1105_main_dock_paints_placeholder_for_floating_panel() {
        run_in_owner(|| {
            toggle_panel_floating(VIEWPORT_PANEL_TAG);
            let scene = view_main_dock(ButtonState::Idle);
            let serialized = format!("{scene:?}");
            assert!(
                serialized.contains("viewport_placeholder"),
                "floating viewport's dock leaf paints the placeholder",
            );
            assert!(
                !serialized.contains(VIEWPORT_BTN_TAG),
                "the viewport content (its button) moved to the floating window — \
                 not duplicated in the main dock",
            );
            // The other panels still paint their real content.
            assert!(serialized.contains("outliner_content_body"));
        });
    }

    #[test]
    fn r1105_floating_window_paints_panel_content_with_header() {
        run_in_owner(|| {
            let scene = view_floating_panel(VIEWPORT_PANEL_TAG, ButtonState::Idle);
            let serialized = format!("{scene:?}");
            assert!(
                serialized.contains(VIEWPORT_BTN_TAG),
                "floating window hosts the live content"
            );
            assert!(
                serialized.contains("viewport#header"),
                "floating window carries a draggable header"
            );
        });
    }

    #[test]
    fn r1105_view_for_window_dispatches_main_vs_torn() {
        run_in_owner(|| {
            let frame = Frame::default();
            let main = <DockPanelsEditorView as WidgetView>::view_for_window(
                MAIN_WINDOW_ID,
                ButtonState::Idle,
                &frame,
            );
            assert!(
                format!("{main:?}").contains(SPLIT_OUTER_TAG),
                "main paints the dock surface"
            );

            let torn = <DockPanelsEditorView as WidgetView>::view_for_window(
                "torn-viewport",
                ButtonState::Idle,
                &frame,
            );
            let torn_s = format!("{torn:?}");
            assert!(
                torn_s.contains(VIEWPORT_BTN_TAG),
                "torn window paints the viewport panel"
            );
            assert!(
                !torn_s.contains(SPLIT_OUTER_TAG),
                "torn window is NOT the whole dock"
            );

            // Defensive: an unrecognised, non-`torn-` id falls back to main.
            let other = <DockPanelsEditorView as WidgetView>::view_for_window(
                "mystery",
                ButtonState::Idle,
                &frame,
            );
            assert!(format!("{other:?}").contains(SPLIT_OUTER_TAG));
        });
    }

    #[test]
    fn r1105_tear_off_intent_toggles_floating_window() {
        run_in_owner(|| {
            let windows = use_editor_windows();
            let intent = Intent {
                tag: tear_off_tag(VIEWPORT_PANEL_TAG, TEAR_OFF_EVENT),
                payload: IntrospectValue::Text(VIEWPORT_PANEL_TAG.to_string()),
            };
            let commands = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert!(commands.is_empty());
            assert!(is_panel_floating(&windows.get(), VIEWPORT_PANEL_TAG));
            // Re-emit toggles back.
            let _ = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert!(!is_panel_floating(&windows.get(), VIEWPORT_PANEL_TAG));
        });
    }

    #[test]
    fn r1105_redock_at_main_removes_floating_window() {
        run_in_owner(|| {
            let windows = use_editor_windows();
            let topo = use_editor_topology();
            let before = topo.get().unwrap();
            toggle_panel_floating(VIEWPORT_PANEL_TAG);
            assert!(is_panel_floating(&windows.get(), VIEWPORT_PANEL_TAG));
            // (R1107.1) Target the panel's OWN slot → `dock_panel_at_zone`'s
            // source==target home no-op, so this test isolates the
            // window-removal (home redock) WITHOUT triggering a relocate/tabify
            // it doesn't assert. Zone-relocate is covered by the r1106 tests.
            let intent = Intent {
                tag: tear_off_tag(VIEWPORT_PANEL_TAG, TEAR_OFF_REDOCK_AT_EVENT),
                payload: IntrospectValue::Json(serde_json::json!({
                    "panel": VIEWPORT_PANEL_TAG,
                    "window": MAIN_WINDOW_ID,
                    "target": VIEWPORT_PANEL_TAG,
                    "x_rel": 0.5,
                    "y_rel": 0.5,
                })),
            };
            let _ = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert!(
                !is_panel_floating(&windows.get(), VIEWPORT_PANEL_TAG),
                "redock-at into main removes the floating window (panel returns to its slot)",
            );
            assert_eq!(
                topo.get().unwrap(),
                before,
                "a home redock (own-slot target) leaves the topology unchanged",
            );
        });
    }

    #[test]
    fn r1105_redock_at_non_main_target_is_a_noop() {
        run_in_owner(|| {
            let windows = use_editor_windows();
            toggle_panel_floating(VIEWPORT_PANEL_TAG);
            // Drop onto ANOTHER floater (no slot to host a panel) → observed, not moved.
            let intent = Intent {
                tag: tear_off_tag(VIEWPORT_PANEL_TAG, TEAR_OFF_REDOCK_AT_EVENT),
                payload: IntrospectValue::Json(serde_json::json!({
                    "panel": VIEWPORT_PANEL_TAG,
                    "window": "torn-properties",
                    "target": PROPERTIES_PANEL_TAG,
                    "x_rel": 0.5,
                    "y_rel": 0.5,
                })),
            };
            let _ = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert!(
                is_panel_floating(&windows.get(), VIEWPORT_PANEL_TAG),
                "a non-main target executes no move — the panel stays floating",
            );
        });
    }

    #[test]
    fn r1105_tear_off_redock_intent_removes_floating_window() {
        run_in_owner(|| {
            let windows = use_editor_windows();
            toggle_panel_floating(PROPERTIES_PANEL_TAG);
            assert!(is_panel_floating(&windows.get(), PROPERTIES_PANEL_TAG));
            let intent = Intent {
                tag: tear_off_tag(PROPERTIES_PANEL_TAG, TEAR_OFF_REDOCK_EVENT),
                payload: IntrospectValue::Text(PROPERTIES_PANEL_TAG.to_string()),
            };
            let _ = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert!(!is_panel_floating(&windows.get(), PROPERTIES_PANEL_TAG));
        });
    }

    #[test]
    fn r1105_follow_desktop_position_adds_main_origin() {
        // No known main origin → relative to desktop (0,0).
        assert_eq!(desktop_position_from(&[], None, (10.0, 20.0)), (10, 20));
        // With a positioned main window, the cursor is offset by its origin.
        let main_at = vec![
            WindowSpec::new(
                Cow::Borrowed(MAIN_WINDOW_ID),
                "main",
                SizeStrategy::Fixed {
                    width: MAIN_W,
                    height: MAIN_H,
                },
            )
            .with_position(200, 150),
        ];
        assert_eq!(
            desktop_position_from(&main_at, None, (10.0, 20.0)),
            (210, 170)
        );
    }

    #[test]
    fn r1105_follow_panel_floating_creates_then_repositions() {
        run_in_owner(|| {
            let windows = use_editor_windows();
            follow_panel_floating(OUTLINER_PANEL_TAG, None, (50.0, 60.0));
            let spec = windows
                .get()
                .into_iter()
                .find(|w| w.id.as_ref() == "torn-outliner")
                .expect("first escaped move creates the floating window");
            assert_eq!(spec.position, Some((50, 60)));
            // A later move repositions the SAME window (non-toggling, no second window).
            follow_panel_floating(OUTLINER_PANEL_TAG, None, (90.0, 100.0));
            let after = windows.get();
            assert_eq!(after.len(), 2, "still one floating window");
            let spec = after
                .iter()
                .find(|w| w.id.as_ref() == "torn-outliner")
                .unwrap();
            assert_eq!(spec.position, Some((90, 100)));
        });
    }

    #[test]
    fn r1105_access_node_for_window_gates_tab_a11y_to_main() {
        run_in_owner(|| {
            // Create a tab well so the dock contributes `tablist` AT nodes.
            use_editor_reorganizer()
                .apply_intent(&DockReorganizeIntent::Tabify {
                    source: VIEWPORT_PANEL_TAG.to_string(),
                    target: PROPERTIES_PANEL_TAG.to_string(),
                })
                .expect("tabify viewport onto properties");
            let main = <DockPanelsEditorView as WidgetView>::access_node_for_window(
                MAIN_WINDOW_ID,
                &ButtonState::Idle,
                None,
            );
            assert!(
                !main.is_empty(),
                "main window carries the dock tablist AT nodes"
            );
            let torn = <DockPanelsEditorView as WidgetView>::access_node_for_window(
                "torn-viewport",
                &ButtonState::Idle,
                None,
            );
            assert!(
                torn.is_empty(),
                "a floating window contributes no dock tablist ghosts"
            );
        });
    }

    #[test]
    fn r1105_windows_signal_opt_in_returns_editor_windows() {
        run_in_owner(|| {
            let from_trait = <DockPanelsEditorView as WidgetView>::windows_signal()
                .expect("editor opts into the reactive windows topology");
            assert!(
                Rc::ptr_eq(&from_trait, &use_editor_windows()),
                "same cached signal"
            );
        });
    }

    // ─── R1106 §5.51 §5.16 §5.41 PR-31 zone-honoring redock (slice-4(a)) ───

    /// (R1106) Recursive 2-leaf-sibling check (mirrors the dock crate's
    /// `reorganize_tests` helper) — proves a zone-honoring relocate landed.
    fn siblings_in_split(node: &DockNode, x: &str, y: &str) -> bool {
        if let DockNode::Split { first, second, .. } = node {
            if let (DockNode::Leaf { panel_id: p }, DockNode::Leaf { panel_id: q }) =
                (first.as_ref(), second.as_ref())
            {
                let (p, q) = (p.as_ref(), q.as_ref());
                if (p == x && q == y) || (p == y && q == x) {
                    return true;
                }
            }
            return siblings_in_split(first, x, y) || siblings_in_split(second, x, y);
        }
        false
    }

    #[test]
    fn r1106_redock_at_zone_relocates_panel_in_topology() {
        run_in_owner(|| {
            let windows = use_editor_windows();
            let topo = use_editor_topology();
            // Boot: viewport sits beside properties (the inner_h split).
            assert!(
                siblings_in_split(
                    topo.get().unwrap().root(),
                    VIEWPORT_PANEL_TAG,
                    PROPERTIES_PANEL_TAG
                ),
                "boot: viewport|properties are siblings",
            );
            // Tear off viewport, then redock onto console's LEFT edge.
            toggle_panel_floating(VIEWPORT_PANEL_TAG);
            assert!(is_panel_floating(&windows.get(), VIEWPORT_PANEL_TAG));
            let intent = Intent {
                tag: tear_off_tag(VIEWPORT_PANEL_TAG, TEAR_OFF_REDOCK_AT_EVENT),
                payload: IntrospectValue::Json(serde_json::json!({
                    "panel": VIEWPORT_PANEL_TAG,
                    "window": MAIN_WINDOW_ID,
                    "target": CONSOLE_PANEL_TAG,
                    "x_rel": 0.15,
                    "y_rel": 0.5,
                })),
            };
            let _ = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &intent);
            // The floating window is gone (re-docked) AND the panel RELOCATED.
            assert!(
                !is_panel_floating(&windows.get(), VIEWPORT_PANEL_TAG),
                "the floating window is removed (re-docked)",
            );
            let after = topo.get().unwrap();
            assert!(
                siblings_in_split(after.root(), VIEWPORT_PANEL_TAG, CONSOLE_PANEL_TAG),
                "viewport relocated beside console at the dropped edge (NOT its home slot)",
            );
            assert!(
                !siblings_in_split(after.root(), VIEWPORT_PANEL_TAG, PROPERTIES_PANEL_TAG),
                "viewport left its home inner_h slot",
            );
            assert_eq!(
                use_editor_reorganizer().last_outcome().as_deref(),
                Some("viewport -> console"),
                "the relocate fired through the shared reorganizer",
            );
        });
    }

    #[test]
    fn r1107_follow_desktop_position_uses_source_window_origin() {
        // The R1095.1 latent-defect fix: re-dragging an already-floating panel's
        // header reports a cursor in THAT window's frame, so its origin — not
        // main's — is the right one to add.
        let main = WindowSpec::new(
            Cow::Borrowed(MAIN_WINDOW_ID),
            "main",
            SizeStrategy::Fixed {
                width: MAIN_W,
                height: MAIN_H,
            },
        )
        .with_position(100, 50);
        let floater = WindowSpec::new(
            Cow::Owned(floating_window_id(VIEWPORT_PANEL_TAG)),
            "torn",
            SizeStrategy::Fixed {
                width: FLOATING_W,
                height: FLOATING_H,
            },
        )
        .with_position(600, 400);
        let panels = vec![main, floater];
        // Source = the floater → add the floater's origin (600,400).
        assert_eq!(
            desktop_position_from(&panels, Some("torn-viewport"), (10.0, 20.0)),
            (610, 420),
            "a floater-source follow adds the FLOATER's origin (not main's)",
        );
        // Source = main (a docked tear-off) → add main's origin (100,50).
        assert_eq!(
            desktop_position_from(&panels, Some(MAIN_WINDOW_ID), (10.0, 20.0)),
            (110, 70),
        );
        // None (degenerate fallback) → main.
        assert_eq!(
            desktop_position_from(&panels, None, (10.0, 20.0)),
            (110, 70)
        );
    }

    #[test]
    fn r1107_follow_intent_threads_source_window_to_reposition() {
        run_in_owner(|| {
            let windows = use_editor_windows();
            // Seed a positioned floating viewport (as if torn off).
            toggle_panel_floating(VIEWPORT_PANEL_TAG);
            let floater_origin = windows
                .get()
                .into_iter()
                .find(|w| w.id.as_ref() == "torn-viewport")
                .and_then(|w| w.position)
                .expect("floater positioned");
            // A follow whose source IS that floater repositions it relative to
            // its own origin (the payload carries the source_window the router
            // would have stamped).
            let intent = Intent {
                tag: tear_off_tag(VIEWPORT_PANEL_TAG, TEAR_OFF_FOLLOW_EVENT),
                payload: IntrospectValue::Json(serde_json::json!({
                    "panel": VIEWPORT_PANEL_TAG,
                    "x": 12.0,
                    "y": 8.0,
                    "source_window": "torn-viewport",
                })),
            };
            let _ = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &intent);
            let moved = windows
                .get()
                .into_iter()
                .find(|w| w.id.as_ref() == "torn-viewport")
                .and_then(|w| w.position)
                .expect("floater still present");
            assert_eq!(
                moved,
                (floater_origin.0 + 12, floater_origin.1 + 8),
                "the follow repositioned the floater relative to ITS own origin",
            );
        });
    }

    #[test]
    fn r1106_redock_at_non_main_target_does_not_relocate() {
        run_in_owner(|| {
            let topo = use_editor_topology();
            let before = topo.get().unwrap();
            toggle_panel_floating(VIEWPORT_PANEL_TAG);
            // Target a non-main window (another floater) → no relocate, no redock.
            let intent = Intent {
                tag: tear_off_tag(VIEWPORT_PANEL_TAG, TEAR_OFF_REDOCK_AT_EVENT),
                payload: IntrospectValue::Json(serde_json::json!({
                    "panel": VIEWPORT_PANEL_TAG,
                    "window": "torn-outliner",
                    "target": OUTLINER_PANEL_TAG,
                    "x_rel": 0.15,
                    "y_rel": 0.5,
                })),
            };
            let _ = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert!(
                is_panel_floating(&use_editor_windows().get(), VIEWPORT_PANEL_TAG),
                "still floating (non-main target hosts nothing)",
            );
            assert_eq!(
                topo.get().unwrap(),
                before,
                "topology untouched for a non-main target",
            );
        });
    }

    #[test]
    fn r1163b_redock_at_with_non_panel_target_stays_floating() {
        // R1163b — a `tear_off_redock_at` whose target names NO dockable panel
        // resolves through `resolve_drop` to `Float` (the discrete-target B model:
        // a release off every panel — incl. a stale / non-panel target — floats,
        // it does NOT force-redock home). So the panel STAYS FLOATING (no data
        // loss, the AI can retry with a valid target) and the topology is
        // unchanged. This SUPERSEDES the pre-B R1107.1 contract (which force-removed
        // the floating window on any redock_at, returning home even for a stale
        // target) — the cross-window path now agrees with the same-window banded
        // resolution. Reachable via `invoke("tear_off_redock_at",{target:"<stale>"})`.
        run_in_owner(|| {
            let windows = use_editor_windows();
            let topo = use_editor_topology();
            let before = topo.get().unwrap();
            toggle_panel_floating(VIEWPORT_PANEL_TAG);
            assert!(is_panel_floating(&windows.get(), VIEWPORT_PANEL_TAG));
            let intent = Intent {
                tag: tear_off_tag(VIEWPORT_PANEL_TAG, TEAR_OFF_REDOCK_AT_EVENT),
                payload: IntrospectValue::Json(serde_json::json!({
                    "panel": VIEWPORT_PANEL_TAG,
                    "window": MAIN_WINDOW_ID,
                    "target": "nonexistent_panel",
                    "x_rel": 0.15,
                    "y_rel": 0.5,
                })),
            };
            let _ = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert!(
                is_panel_floating(&windows.get(), VIEWPORT_PANEL_TAG),
                "a non-panel target resolves to Float — the panel stays floating",
            );
            assert_eq!(
                topo.get().unwrap(),
                before,
                "Float makes no topology change — the panel is unchanged, not lost",
            );
        });
    }

    /// Fire the cross-window `tear_off_redock_at` reducer for `panel` dropped onto
    /// main's `target` at the normalised cursor — the same wire shape the live shell
    /// + the AI `invoke` emit.
    fn fire_redock_at(panel: &str, target: &str, x_rel: f64, y_rel: f64) {
        let intent = Intent {
            tag: tear_off_tag(panel, TEAR_OFF_REDOCK_AT_EVENT),
            payload: IntrospectValue::Json(serde_json::json!({
                "panel": panel,
                "window": MAIN_WINDOW_ID,
                "target": target,
                "x_rel": x_rel,
                "y_rel": y_rel,
            })),
        };
        let _ = <DockPanelsEditorView as WidgetCore>::update(ButtonState::Idle, &intent);
    }

    #[test]
    fn r1163b_cross_window_preview_agrees_with_redock_result() {
        // R1163b — the cross-window PREVIEW (`dock_drop_preview`) and the cross-window
        // RESULT (`redock_cross_window`, the `tear_off_redock_at` reducer) both flow
        // from the ONE `resolve_drop` SSOT, so the preview shows an overlay EXACTLY
        // when the release redocks: a Dock zone previews + docks; a Float dead-zone
        // previews nothing + leaves the panel floating. Before R1163b the cross-window
        // path classified with the legacy continuous geometry while the preview used a
        // separate `_placeholder` + `dock_drop_zone_normalized` path — a two-geometry
        // divergence where preview could disagree with result. This pins preview ==
        // result by construction for the cross-window path.
        let rect = Rect::new(0, 0, 200, 100);

        // CASE 1 — the CENTRE of a live panel = a Dock (tabify) zone: the preview
        // paints an overlay AND the release redocks (no longer floating).
        run_in_owner(|| {
            let windows = use_editor_windows();
            toggle_panel_floating(PROPERTIES_PANEL_TAG);
            assert!(is_panel_floating(&windows.get(), PROPERTIES_PANEL_TAG));
            let preview = <DockPanelsEditorView as WidgetView>::dock_drop_preview(
                PROPERTIES_PANEL_TAG,
                VIEWPORT_PANEL_TAG,
                rect,
                0.5,
                0.5,
            );
            assert!(
                preview.is_some(),
                "a Dock (centre) zone previews an overlay",
            );
            fire_redock_at(PROPERTIES_PANEL_TAG, VIEWPORT_PANEL_TAG, 0.5, 0.5);
            assert!(
                !is_panel_floating(&windows.get(), PROPERTIES_PANEL_TAG),
                "preview Some == the release redocks (panel no longer floating)",
            );
        });

        // CASE 2 — a panel's dead-zone RING (between the 0.22 edge band and the 0.18
        // centre square) = Float: the preview paints NOTHING AND the release leaves
        // the panel floating. This is the unification's teeth — the legacy continuous
        // classifier had no dead zone, so this point used to dock.
        run_in_owner(|| {
            let windows = use_editor_windows();
            toggle_panel_floating(PROPERTIES_PANEL_TAG);
            assert!(is_panel_floating(&windows.get(), PROPERTIES_PANEL_TAG));
            let preview = <DockPanelsEditorView as WidgetView>::dock_drop_preview(
                PROPERTIES_PANEL_TAG,
                VIEWPORT_PANEL_TAG,
                rect,
                0.30,
                0.5,
            );
            assert!(preview.is_none(), "a Float dead-zone ring previews nothing",);
            fire_redock_at(PROPERTIES_PANEL_TAG, VIEWPORT_PANEL_TAG, 0.30, 0.5);
            assert!(
                is_panel_floating(&windows.get(), PROPERTIES_PANEL_TAG),
                "preview None == the release leaves the panel floating",
            );
        });
    }
}
