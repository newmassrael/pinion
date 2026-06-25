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
use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{AlignItems, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::undo::{UndoStack, UndoStackExternal, use_undo_stack};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Frame, Owner, Scene, Signal, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::button::{ButtonColors, ButtonStyle, view_button};
use pinion_widget_paint::dock::{
    DockDropPreview, DockNode, DockPanelExternal, DockReorganizeExternal, DockReorganizer,
    DockSplitState, DockTopology, dock_tablist_access_nodes, view_dock_surface,
};
use pinion_widget_paint::splitter::SplitterExternal;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(
    HelloDockPanelsEditorRenderer,
    HelloDockPanelsEditorRendererError
);

// ─── window dimensions ────────────────────────────────────────────────

const MAIN_W: u32 = 1200;
const MAIN_H: u32 = 800;

// ─── theme + paint-side tags ──────────────────────────────────────────

const THEME_TAG: &str = "app";

const TOOLBAR_PANEL_TAG: &str = "toolbar";
const OUTLINER_PANEL_TAG: &str = "outliner";
const VIEWPORT_PANEL_TAG: &str = "viewport";
const PROPERTIES_PANEL_TAG: &str = "properties";
const CONSOLE_PANEL_TAG: &str = "console";

/// Viewport Button paint-side tag — the only interactive widget in
/// the v1 editor binding. Routes pointer events to the primary
/// [`ButtonExternal`].
const VIEWPORT_BTN_TAG: &str = "viewport_btn";

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

const CONSOLE_ROWS: &[&str] = &[
    "[info] scene loaded (5 panels)",
    "[info] viewport camera initialised",
    "[warn] no light selected",
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
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            TOOLBAR_LABEL.to_string(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(PANEL_BODY_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
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
            externals.reserve(topology.split_count() + topology.panel_count() + 2);
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
            // R1095.1 — this editor is DOCK-ONLY: it consumes the reorganizer
            // (dock / split / tabify) but has no `windows_signal` and wires NO
            // tear-off reducer arm, so an escape-drop's `tear_off`/`tear_off_follow`/
            // `tear_off_redock` intents (and the panel's `detached` latch) are an
            // intentional no-op here. Float-to-window is the deferred PR-31
            // 2nd consumer (it would also unlock lifting the flat example's
            // follow/redock/desktop-conversion trio into a dock substrate).
            for panel_id in topology.panel_ids() {
                let panel = DockPanelExternal::new(panel_id.to_string())
                    .with_reorganizer(Rc::clone(&reorganizer))
                    .with_drop_preview(Rc::clone(&preview));
                externals.push(ExtraExternal::new(panel_id.to_string(), Box::new(panel)));
            }
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
        let theme = use_theme(THEME_TAG).theme_animated();
        // (R686 §5.45) Read the live topology from its reactive Signal
        // (subscribes the view; a reorganize gesture's `Signal::set`
        // re-renders the new layout). The boot value is
        // `build_editor_topology()` (seeded by `use_editor_topology`).
        let topology = use_editor_topology().get();
        // (R685.B atomic 1 SSOT walker) — the walker auto-builds
        // DockPanelStyle / SplitterStyle from the topology's
        // panel_id / split_id / orientation, and threads the
        // topology's declared initial_ratio through to the binding's
        // Signal constructor. Binding callbacks supply only:
        //   - panel content `Scene` per panel_id
        //   - reactive `DockSplitState` per split_id (memoised Signal
        //     + dragging flag); the initial_ratio arg comes from the
        //     topology, so no caller-side ratio default duplication.
        // (R686) The split_state callback keys `use_split_ratio` on the
        // raw split id (Cow) so a reorganize-minted runtime split id
        // resolves an Owner::cache slot without a `&'static str` bridge.
        // (R1082 §5.51) Read the shared drop-preview (subscribes the view,
        // so a drag's `drag_to` re-renders the overlay). The closure maps a
        // panel id to the live zone iff that panel is the current drop
        // target — the panel under the cursor paints its zone affordance.
        let preview = use_drop_preview().get();
        // (R1084 §5.51) The dock surface is total over the empty (`None`)
        // state — an empty dock paints a bare container. The editor seeds
        // `Some` and never empties, but the view honours the universal Option.
        match topology {
            Some(topology) => view_dock_surface(
                &topology,
                |panel_id| panel_content_for(panel_id, state, &theme),
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
                &theme,
            ),
            None => Scene::Container(ContainerNode::new(vec![])),
        }
    }

    fn event_name(event: Self::Event) -> &'static str {
        <Self::Event as pinion_core::WidgetEventName>::as_name(&event)
    }

    fn update(_state: Self::State, intent: &Intent) -> Vec<Command> {
        if intent.tag_str() == VIEWPORT_BTN_CLICK_INTENT_TAG {
            let counter = use_viewport_click_counter();
            counter.set(counter.get().wrapping_add(1));
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
    use pinion_widget_paint::dock::{DockReorganizeIntent, DockSplitPosition};
    use pinion_widget_paint::splitter::SplitterOrientation;
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
            // (R749 §5.52 reorganize history).
            assert_eq!(externals.len(), 11);
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
}
