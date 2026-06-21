// R683.C §5.16 — example bindings tolerate looser doc-markdown lints
// than the substrate crates. Same waiver hello-multi-window /
// hello-tree-view carry — the workspace `clippy::pedantic` floor
// otherwise demands every identifier in every doc-comment carry
// backticks; for an example-binding main.rs with dense
// architecture-narrative prose, that pushes the lint surface into
// the doc text itself.
#![allow(clippy::doc_markdown)]

//! `hello-dock-panels` — R683.C §5.16 §5.41 §5.45 first **dock-panel +
//! tear-off** consumer + 2nd `ClickRouter` consumer (closes the R679
//! Rule-of-Three substrate-lift gate alongside `hello-multi-window`).
//!
//! ## Architecture
//!
//! Three retained widget panels composed into the canonical pro-tool
//! authoring topology every DCC / IDE / CAD ships:
//!
//! ```text
//!  ┌───────────────────────────┬──────────────────────┐
//!  │  Inspector (DevTools tree)│                      │
//!  ├───────────────────────────┤        Viewport      │
//!  │     Property pane         │  (Button + content)  │
//!  └───────────────────────────┴──────────────────────┘
//! ```
//!
//! Three [`DockPanel`](pinion_widget_paint::dock::view_dock_panel)
//! widgets in a Splitter(H) → Splitter(V) hierarchy. The main split
//! divides inspector+property (left) from viewport (right); the left
//! split divides inspector (top) from property (bottom).
//!
//! ## Tear-off / dock-back
//!
//! Each panel's header is a draggable handle wired to a
//! [`DockPanelExternal`](pinion_widget_paint::dock::DockPanelExternal)
//! registered at the composite tag `{panel_tag}#header`. Dragging the
//! header past the M3-canonical 0.5 threshold fraction emits a
//! `tear_off` intent carrying the panel id; the binding reducer
//! responds by:
//!
//! * **Tear-off**: if no floating window exists for the panel,
//!   `Signal::set` pushes a new [`WindowSpec`] onto the reactive
//!   `Signal<Vec<WindowSpec>>` returned by [`Self::windows_signal`].
//!   The shell's R683.A reconcile-diff Effect spawns a new winit
//!   window with the floating panel content via
//!   [`view_floating_panel`].
//! * **Dock-back**: if a floating window exists for the panel,
//!   `Signal::set` removes it from the vec. The reconcile Effect
//!   drops the winit window + the per-window scope; the dock layout
//!   re-paints the next frame with the panel re-installed.
//!
//! Either gesture goes through the same `tear_off` intent — the
//! binding's `Signal<Vec<WindowSpec>>` is the source-of-truth for
//! "which panels are floating". This keeps the dock + tear-off
//! substrate symmetric (one `DockPanelExternal` per panel; both
//! docked-→floating and floating-→docked transitions share the same
//! gesture + same intent shape).
//!
//! ## Cross-panel selection (R679 closure)
//!
//! The viewport panel hosts a [`ClickRouter`](pinion_widget_paint::devtools::ClickRouter)
//! (the **2nd** `ClickRouter` consumer — `hello-multi-window` was the
//! 1st, R683.C closes the [[abstraction-needs-second-consumer]]
//! Rule-of-Three gate). AI clients drive `scene/invoke
//! /viewport_click_router/external/click {Text(path)}` to select a
//! viewport node; the inspector tree row paints the M3
//! `SurfaceContainerHighest` focus state-layer + the property pane
//! displays the selected node's type. The user-mouse half closes
//! through the viewport `Button`'s SCXML emit (the canonical
//! `viewport_btn.click` intent the reducer mirrors into the same
//! `Signal<Option<String>>`).
//!
//! ## Why this matters for the northern-star
//!
//! Phase B (Pro GUI) and Phase D (editor self-hosted in pinion) both
//! ride this substrate: every Photoshop / Figma / Unreal Editor /
//! `VSCode` pane system is a dock + tear-off lattice. R683.C is the
//! smallest binding that exercises the full chain — dynamic window
//! lifecycle through `windows_signal`, draggable Splitter handles,
//! tear-off intent emission, multi-panel cross-window state sync —
//! so substrate gaps surface immediately rather than waiting for a
//! real DCC binding.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::rc::Rc;

use pinion_a11y::{AccessFocus, AccessNode, WidgetA11y, tree_access_nodes};
use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::widgets::tree_nav::{TreeKey, effective_cursor, flat_visible, resolve_tree_key};
use pinion_core::{Color, Frame, Owner, Scene, Signal, WidgetCore};
use pinion_shell::typeahead::tree_typeahead_jump;
use pinion_shell::{SizeStrategy, WidgetView, WindowSpec, vello_renderer_impl};
use pinion_widget_paint::devtools::{
    ClickRouter, find_node_at_path, rebuild_with_highlight_at_path, scene_root_path_segment,
    scene_to_tree_item, scene_type_name,
};
use pinion_widget_paint::dock::{
    DockPanelExternal, DockPanelStyle, FloatingPlaceholderStyle, view_dock_panel,
    view_floating_placeholder,
};
use pinion_widget_paint::splitter::{
    SplitterExternal, SplitterOrientation, SplitterStyle, view_splitter,
};
use pinion_widget_paint::tree_view::{
    TreeItem, TreeRowClickExternal, TreeViewFocus, TreeViewStyle, composite_row_tag,
    view_tree_focused,
};

use pinion_widget_paint::state_layer::{HOVER, PRESSED};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloDockPanelsRenderer, HelloDockPanelsRendererError);

// ─── window dimensions ──────────────────────────────────────────────

/// Main window dimensions. Wide enough to host the 3-panel dock
/// layout (inspector + property left column ≈ 280 px, viewport ≈ 600
/// px) without crowding the splitter handles. Tall enough that the
/// vertical splitter on the left column splits the inspector tree
/// (top) from the property pane (bottom) with room for both to read.
const MAIN_W: u32 = 880;
const MAIN_H: u32 = 600;
/// Floating window dimensions (one per torn-off panel). Smaller than
/// the main — torn-off panels are reference views the user docks into
/// a secondary monitor or floats alongside the main editing surface.
const FLOATING_W: u32 = 360;
const FLOATING_H: u32 = 360;

// ─── theme + paint-side tags ─────────────────────────────────────────

const THEME_TAG: &str = "app";

/// Paint-side tag the [`InputRouter`](pinion_runtime::InputRouter)
/// hit-tests against for the viewport Button. Routes pointer events
/// to the primary [`ButtonExternal`].
const VIEWPORT_BTN_TAG: &str = "viewport_btn";

/// Inspector panel root tag. The dock substrate emits
/// [`view_dock_panel`] with this tag on the outer Container + the
/// composite tags `inspector#header` and `inspector#content` on the
/// header strip + content area.
const INSPECTOR_PANEL_TAG: &str = "inspector";
/// Property panel root tag (same convention as inspector).
const PROPERTY_PANEL_TAG: &str = "property";
/// Viewport panel root tag (same convention).
const VIEWPORT_PANEL_TAG: &str = "viewport";

// R683.C InputRouter routing correction: DockPanelExternal lives at
// the panel root tag (per `pinion_widget_paint::dock::DockPanelExternal`
// rustdoc — the R51.42 `dispatch_send` split-on-`#` discipline forces
// the External onto the primary half of the composite tag). The
// composite header tags (e.g. `"inspector#header"`) are PAINT-side
// tags the dock substrate emits on the header child Container; they
// are NOT used as ExtraExternal registration keys. The InputRouter
// resolves them to the panel primary tag via `split_subindex` and
// dispatches the wire payload as `"header:PointerDown"` etc., which
// the DockPanelExternal parses internally.

/// Main splitter (Horizontal) — divides the inspector+property left
/// column from the viewport right column. The paint-side tag the
/// `SplitterExternal` is registered against.
const MAIN_SPLITTER_TAG: &str = "main_splitter";
/// Left splitter (Vertical) — divides the inspector (top) from the
/// property pane (bottom) inside the left column.
const LEFT_SPLITTER_TAG: &str = "left_splitter";

/// Inspector tree-view root tag — same convention as the
/// `hello-multi-window` inspector. Composite per-row tags are
/// `inspector_tree#{path}` per the R51.42 substrate.
const INSPECTOR_TREE_TAG: &str = "inspector_tree";
/// R812 §5.40 — accessible name for the inspector's `role=tree` root, so a
/// screen reader announces the tree by purpose before its rows.
const INSPECTOR_TREE_NAME: &str = "Scene inspector";
/// Property pane root tag — outermost Container around the field-row
/// stack. AI clients walk via `scene/snapshot {path:
/// "/property_pane"}` to read the pane's content.
const PROPERTY_PANE_TAG: &str = "property_pane";
/// R683.C §5.16 §5.49 — viewport click router tag. Registered as the
/// 2nd `ExtraExternal` of type [`ClickRouter`] across the pinion
/// example gallery (`hello-multi-window` = 1st at `main_click_router`,
/// `hello-dock-panels` = 2nd here). Closes the
/// [[abstraction-needs-second-consumer]] Rule-of-Three gate that drove
/// the R683.C substrate lift.
const VIEWPORT_CLICK_ROUTER_TAG: &str = "viewport_click_router";

// ─── intent tag constants (dotted form) ──────────────────────────────

/// Inspector panel's `tear_off` intent — emitted by the
/// [`DockPanelExternal`] registered at [`INSPECTOR_PANEL_TAG`]
/// whenever the user drags the inspector header past the threshold.
/// Runtime walker prefixes the bare event with the External's
/// registered tag (the panel root tag per R683.C routing correction),
/// so the literal dotted form is `inspector.tear_off`.
const INSPECTOR_TEAR_OFF_INTENT_TAG: &str = intent_tag!("inspector", "tear_off");
/// Property panel's `tear_off` intent (same convention).
const PROPERTY_TEAR_OFF_INTENT_TAG: &str = intent_tag!("property", "tear_off");
/// Viewport panel's `tear_off` intent (same convention).
const VIEWPORT_TEAR_OFF_INTENT_TAG: &str = intent_tag!("viewport", "tear_off");

/// Inspector tree-row click intent (same as hello-multi-window's R675
/// arc). The `TreeRowClickExternal` registered at `inspector_tree`
/// emits a bare `click` event the runtime prefixes to
/// `inspector_tree.click`. Payload: `IntrospectValue::Text(path)`.
const INSPECTOR_CLICK_INTENT_TAG: &str = intent_tag!("inspector_tree", "click");
/// Inspector tree-row hover intent (same as hello-multi-window's R678
/// arc). Payload: `Text(path)` on enter, `Null` on leave.
const INSPECTOR_HOVER_INTENT_TAG: &str = intent_tag!("inspector_tree", "hover");
/// Viewport `ClickRouter`'s `click` intent. AI invokes
/// `scene/invoke /viewport_click_router/external/click {Text(path)}`
/// to drive the cross-panel select arc; the dotted intent the
/// reducer matches against is `viewport_click_router.click`.
const VIEWPORT_CLICK_ROUTER_INTENT_TAG: &str = intent_tag!("viewport_click_router", "click");
/// Viewport Button's `click` intent. The `ButtonExternal`'s SCXML
/// `Pressed → Hover` transition emits a `Null`-payload `click` event
/// on every completed user click; the reducer mirrors the button's
/// known raw path (see [`VIEWPORT_BTN_RAW_PATH`]) into the shared
/// `selected_path` Signal.
const VIEWPORT_BTN_CLICK_INTENT_TAG: &str = intent_tag!("viewport_btn", "click");

// ─── default ratios + viewport content ───────────────────────────────

/// Main splitter default ratio — inspector+property column gets ~32 %
/// of the main window's width, viewport gets ~68 %. Matches the
/// VSCode / IntelliJ "side panel + editor canvas" default split feel.
const MAIN_SPLIT_RATIO_DEFAULT: f32 = 0.32;
/// Left splitter default ratio — inspector tree gets ~55 % of the
/// left column's height, property pane gets ~45 %. Matches the
/// Chrome / Firefox / Safari Inspector "Elements pane + Computed
/// pane" canonical proportion.
const LEFT_SPLIT_RATIO_DEFAULT: f32 = 0.55;
/// Tear-off threshold fraction — matches M3 + dock substrate's
/// [`DockPanelStyle::m3_default`] (header drag past half its own width
/// fires the `tear_off` intent).
const TEAR_OFF_FRAC_DEFAULT: f32 = 0.5;

/// Viewport header label rendered above the viewport Button. Static
/// text so the inspector tree has a stable Text[0] leaf.
const VIEWPORT_HEADER_TEXT: &str = "Viewport";
/// Viewport button label.
const VIEWPORT_BTN_LABEL: &str = "Click me (viewport)";
/// Property pane row font size — M3 Body Medium 14 sp scales to the
/// pane's dense field listing without crowding the 28-px header.
const PROPERTY_PANE_FONT_PX: u32 = 14;
/// Property pane placeholder when no selection is active or the
/// resolved path is stale.
const PROPERTY_PANE_NO_SELECTION_TEXT: &str = "(no selection)";

/// Floating-window id prefix the binding mints when a panel is torn
/// off. The view_for_window dispatch reads back this prefix to know
/// which panel a floating window hosts.
const FLOATING_WINDOW_PREFIX: &str = "torn-";

/// R683.C §5.16 §5.49 — the raw-scene path identifying the
/// `Container[viewport_btn]` node inside the unwrapped
/// [`view_viewport_raw`] paint. Same shape as
/// `hello-multi-window`'s `MAIN_BUTTON_RAW_PATH` — hard-coded
/// because the viewport view fn's raw shape is stable (outer
/// untagged Container with the header Text + the tagged button
/// Container; the button's tag-form path takes priority over
/// nth-of-type so it stays `Container/Container[viewport_btn]`
/// whether or not the optional selection banner sits in front).
const VIEWPORT_BTN_RAW_PATH: &str = "Container/Container[viewport_btn]";

// ─── reactive substrate hooks ────────────────────────────────────────

/// Main-splitter ratio handle, memoised through
/// [`Owner::cache`](pinion_core::Owner::cache). The view fn reads
/// `.get()` to subscribe each paint to ratio mutations; the
/// [`SplitterExternal`] registered at [`MAIN_SPLITTER_TAG`] writes
/// `.set()` on every drag frame; both observe the same `Rc<Signal>`
/// because `Owner::cache` returns the cached value on every call with
/// the same key.
fn use_main_split_ratio() -> Rc<Signal<f32>> {
    Owner::current()
        .expect("hello-dock-panels: view fn runs inside the substrate root owner scope")
        .cache("hello_dock_main_split_ratio", || {
            Signal::new(MAIN_SPLIT_RATIO_DEFAULT)
        })
}

/// Left-splitter ratio handle (same `Owner::cache` discipline as
/// [`use_main_split_ratio`]).
fn use_left_split_ratio() -> Rc<Signal<f32>> {
    Owner::current()
        .expect("hello-dock-panels: view fn runs inside the substrate root owner scope")
        .cache("hello_dock_left_split_ratio", || {
            Signal::new(LEFT_SPLIT_RATIO_DEFAULT)
        })
}

/// R683.A + R683.C §5.16 §5.41 — runtime topology signal. The shell's
/// `reconcile_windows` Effect subscribes this Signal at boot; each
/// `Signal::set` triggers a diff-based add/drop pass against winit
/// windows + the per-window substrate scopes.
///
/// Initial topology = the canonical single main window. Tear-off
/// intents append floating-window specs with ids of the form
/// `torn-{panel_id}`; dock-back intents remove them.
fn use_windows_topology() -> Rc<Signal<Vec<WindowSpec>>> {
    Owner::current()
        .expect("hello-dock-panels: windows_signal runs inside the substrate root owner scope")
        .cache("hello_dock_windows", || {
            Signal::new(vec![WindowSpec::new(
                Cow::Borrowed("main"),
                "hello-dock-panels — Main (R683.C)",
                SizeStrategy::Fixed {
                    width: MAIN_W,
                    height: MAIN_H,
                },
            )])
        })
}

/// Cross-panel + cross-window shared selection slot. The viewport
/// click router writes `Text(path)` on AI invokes; the viewport
/// Button's SCXML emit writes [`VIEWPORT_BTN_RAW_PATH`] on user
/// mouse clicks; the inspector tree-row click writes the inspector-
/// resolved path. All three converge on the same Signal so the
/// inspector focus state-layer + property pane field listing + the
/// viewport highlight wrap all repaint in lockstep.
fn use_selected_path() -> Rc<Signal<Option<String>>> {
    Owner::current()
        .expect("hello-dock-panels: view fn runs inside the substrate root owner scope")
        .cache("hello_dock_selected_path", || Signal::new(None::<String>))
}

/// Cross-panel hover slot — parallel to [`use_selected_path`].
/// Inspector tree-row PointerEnter writes `Some(path)`; PointerLeave
/// clears to `None`. The viewport paints a transient M3
/// `SurfaceContainerHighest` wrap on the hovered node (layered under
/// the Error-red selection wrap; selection wins on the same node).
fn use_hovered_path() -> Rc<Signal<Option<String>>> {
    Owner::current()
        .expect("hello-dock-panels: view fn runs inside the substrate root owner scope")
        .cache("hello_dock_hovered_path", || Signal::new(None::<String>))
}

/// R811 §5.27 — Page Up/Down jump size for inspector keyboard nav
/// (mirrors `hello-tree-view`'s `NAV_PAGE`).
const INSPECTOR_NAV_PAGE: usize = 7;

/// R811 §5.38 — owner-cache key for the inspector type-ahead cursor
/// (buffer + last-typed instant); the shell-root `Owner` slot drops
/// with the shell, so no stale search survives across shells.
const INSPECTOR_TYPEAHEAD_KEY: &str = "hello_dock_panels::inspector_typeahead";

/// R811 §5.50 §5.27 — the inspector tree's per-path collapse overlay.
/// The inspector projects a fresh `TreeItem` tree from the live
/// viewport scene every paint (all branches default expanded), so the
/// expand state cannot live on the derived nodes; it lives here, keyed
/// by node id (= the scene path / synthetic inspector id). A path in
/// the set is collapsed; its absence means expanded — the boot default
/// preserving R683.C's fully-expanded inspector.
fn use_collapsed_paths() -> Rc<Signal<BTreeSet<String>>> {
    Owner::current()
        .expect("hello-dock-panels: view fn runs inside the substrate root owner scope")
        .cache(
            "hello_dock_collapsed_paths",
            || Signal::new(BTreeSet::new()),
        )
}

/// R811 §5.50 — set branch `id`'s collapsed flag, writing the Signal
/// only on an actual change (no redundant repaint). Inserting collapses
/// the branch; removing expands it.
fn set_collapsed(id: &str, collapsed: bool) {
    let store = use_collapsed_paths();
    let mut set = store.get();
    let changed = if collapsed {
        set.insert(id.to_owned())
    } else {
        set.remove(id)
    };
    if changed {
        store.set(set);
    }
}

/// R811 §5.50 — flip branch `id`'s collapsed flag (Space / Enter).
fn toggle_collapsed(id: &str) {
    let store = use_collapsed_paths();
    let mut set = store.get();
    // `remove` returns false when the id was absent (expanded) → collapse
    // it; true when present (collapsed) → leave it removed (expand).
    if !set.remove(id) {
        set.insert(id.to_owned());
    }
    store.set(set);
}

// R820.1 §5.27 §5.38 — the inspector's type-ahead glue was lifted to
// `pinion_shell::typeahead::tree_typeahead_jump` when it became the third
// byte-identical tree-typeahead copy (with hello-tree-view + hello-virtual-
// tree). The per-binding policy is just the cache key + which Signal moves,
// both passed as arguments; the match step was already the shared substrate.

/// R811 §5.50 §5.27 — the inspector's visible `TreeItem` tree: the fixed
/// two-row top level (the `ButtonState` leaf + the `viewport` branch
/// wrapping the live scene projection) with the per-path collapse
/// overlay applied. Shared by the paint path
/// ([`view_inspector_content`]) and the keyboard-nav path
/// ([`DockPanelsView`]'s `apply_key`) so both flatten the identical row
/// sequence — the R809.1 single-traversal invariant across the two code
/// paths.
fn inspector_tree_items(state: ButtonState, theme: &Theme) -> Vec<TreeItem> {
    let viewport_scene = view_viewport_raw(state, theme);
    let root_segment = scene_root_path_segment(&viewport_scene);
    let mut items = vec![
        TreeItem::leaf("state", format!("ButtonState: {state:?}")),
        TreeItem::branch(
            "viewport",
            "viewport scene",
            true,
            vec![scene_to_tree_item(&viewport_scene, &root_segment)],
        ),
    ];
    let collapsed = use_collapsed_paths().get();
    for item in &mut items {
        apply_collapsed(item, &collapsed);
    }
    items
}

/// R811 §5.50 — apply the [`use_collapsed_paths`] overlay to a derived
/// `TreeItem` subtree: a branch whose id is in `collapsed` paints
/// collapsed (its descendants drop out of the flat visible sequence).
/// Leaves and absent ids keep their projected (expanded) state.
fn apply_collapsed(item: &mut TreeItem, collapsed: &BTreeSet<String>) {
    if !item.children.is_empty() && collapsed.contains(&item.id) {
        item.expanded = false;
    }
    for child in &mut item.children {
        apply_collapsed(child, collapsed);
    }
}

// ─── floating-window helpers ─────────────────────────────────────────

/// Construct the canonical floating-window id for `panel_id`. Used by
/// both the reducer (to push / locate `WindowSpec` entries) and
/// `view_for_window` (to strip the prefix and dispatch to
/// [`view_floating_panel`]).
fn floating_window_id(panel_id: &str) -> String {
    format!("{FLOATING_WINDOW_PREFIX}{panel_id}")
}

/// Human-readable title for a floating window hosting `panel_id`.
fn floating_window_title(panel_id: &str) -> String {
    format!("hello-dock-panels — {panel_id} (floating)")
}

/// `true` iff a floating window for `panel_id` currently exists in
/// `panels` (the `Signal<Vec<WindowSpec>>` snapshot).
fn is_panel_floating(panels: &[WindowSpec], panel_id: &str) -> bool {
    let target = floating_window_id(panel_id);
    panels.iter().any(|w| w.id == target)
}

/// Reducer mutation for a `tear_off` intent. Idempotent on the
/// canonical alternation:
///
/// * Panel is docked (no matching floating window in the topology) →
///   push a new floating `WindowSpec` onto the signal.
/// * Panel is floating (matching window exists) → remove the
///   `WindowSpec` (dock-back). The reconcile Effect drops the winit
///   window + per-window scope; the main dock layout repaints with
///   the panel re-installed in its slot.
///
/// `Signal::set` triggers the equality-skip-aware R26 + R36 reactive
/// fan-out: subscribed view fns (the main dock layout + the floating
/// window view fns) re-run automatically.
fn toggle_panel_floating(panel_id: &str) {
    let signal = use_windows_topology();
    let mut current = signal.get();
    let target = floating_window_id(panel_id);
    if let Some(idx) = current.iter().position(|w| w.id == target) {
        current.remove(idx);
    } else {
        current.push(WindowSpec::new(
            Cow::Owned(target),
            floating_window_title(panel_id),
            SizeStrategy::Fixed {
                width: FLOATING_W,
                height: FLOATING_H,
            },
        ));
    }
    signal.set(current);
}

/// Depth of a slash-separated path (count of segment separators).
/// Hoisted out of the view fn body to keep
/// `clippy::items_after_statements` clean — same idiom
/// hello-multi-window uses.
fn path_depth(p: &str) -> usize {
    p.matches('/').count()
}

/// Display title for a docked panel header. Match the panel id to its
/// human-readable label.
fn panel_title_for(panel_id: &str) -> &'static str {
    match panel_id {
        INSPECTOR_PANEL_TAG => "Inspector",
        PROPERTY_PANEL_TAG => "Property",
        VIEWPORT_PANEL_TAG => "Viewport",
        _ => "Panel",
    }
}

// ─── view fns ────────────────────────────────────────────────────────

/// R683.C §5.16 §5.49 — **raw** viewport scene, the source-of-truth
/// the inspector tree mirrors + the highlight-overlay walker
/// composes on top of (R676 path-stability discipline — wraps
/// inserted by [`view_viewport_content`] would otherwise shift the
/// tree-row path indices).
///
/// Layout: outer untagged Container with a header `Text` (the
/// viewport label) above a tagged `Container[viewport_btn]` holding
/// the Button.
fn view_viewport_raw(state: ButtonState, theme: &Theme) -> Scene {
    let surface = theme.resolve(ColorRole::Surface);
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let idle_fill = theme.resolve(ColorRole::SurfaceContainerHighest);
    let btn_fill: Color = match state {
        ButtonState::Idle => idle_fill,
        ButtonState::Hover => idle_fill.lerp(on_surface, HOVER),
        ButtonState::Pressed => idle_fill.lerp(on_surface, PRESSED),
        ButtonState::Disabled => idle_fill.lerp(surface, 0.38),
    };
    let label = match state {
        ButtonState::Disabled => "Disabled",
        _ => VIEWPORT_BTN_LABEL,
    };
    let label_fg = if matches!(state, ButtonState::Disabled) {
        on_surface_muted
    } else {
        on_surface
    };
    let header = Scene::Text(TextNode::styled(
        VIEWPORT_HEADER_TEXT,
        Rect::default(),
        TextStyle::new().with_size_px(14).with_fg(on_surface_muted),
    ));
    let button = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            label,
            Rect::default(),
            TextStyle::new().with_size_px(14).with_fg(label_fg),
        ))])
        .with_tag(VIEWPORT_BTN_TAG)
        .with_style(BoxStyle::filled(btn_fill))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(180, 48))
                // (R1020 §5.39) The viewport button is a Tab stop —
                // `.with_focusable(true)` marks it alongside the inspector
                // tree. This binding hand-rolls the button Container (not
                // `ButtonStyle`/`button_scene`), so the focus-stop opt-in
                // is the direct-node marker on its `LayoutStyle`.
                .with_focusable(true),
        ),
    );
    Scene::Container(
        ContainerNode::new(vec![header, button])
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(12),
            ),
    )
}

/// R683.C §5.16 §5.49 — viewport paint with the DevTools highlight
/// overlay composed on top of the raw scene. Mirrors
/// `hello-multi-window`'s `view_main` two-pass design: build the raw
/// scene (the inspector mirrors this), then walk + wrap the
/// selection / hover paths.
fn view_viewport_content(state: ButtonState, theme: &Theme) -> Scene {
    let raw = view_viewport_raw(state, theme);
    let selection_color = theme.resolve(ColorRole::Error);
    let hover_color = theme.resolve(ColorRole::SurfaceContainerHighest);
    let selected = use_selected_path().get();
    let hovered = use_hovered_path().get();

    let selected_resolves = selected
        .as_deref()
        .filter(|p| find_node_at_path(&raw, p).is_some());
    let hovered_resolves = hovered
        .as_deref()
        .filter(|p| find_node_at_path(&raw, p).is_some())
        // Selection wins on the same node — suppress the hover wrap
        // when both signals point at the same path so the Error red
        // selection ring reads cleanly (no duplicate nested borders).
        .filter(|p| selected_resolves != Some(*p));

    let mut wraps: Vec<(&str, Color)> = Vec::with_capacity(2);
    if let Some(hpath) = hovered_resolves {
        wraps.push((hpath, hover_color));
    }
    if let Some(spath) = selected_resolves {
        wraps.push((spath, selection_color));
    }
    // Apply the deeper wrap first so the shallower wrap's anonymous-
    // ancestor insertion preserves the deeper wrap's path lookup —
    // same depth-desc discipline as hello-multi-window's view_main.
    wraps.sort_by(|a, b| path_depth(b.0).cmp(&path_depth(a.0)));
    let mut scene = raw;
    for (path, color) in wraps {
        scene = rebuild_with_highlight_at_path(scene, path, color);
    }
    scene
}

/// R683.C §5.16 §5.49 — inspector pane content. Mirrors the raw
/// viewport scene as a TreeView (R675 substrate consumer; same shape
/// as hello-multi-window's inspector). Reads
/// [`use_selected_path`] for the focus state-layer.
fn view_inspector_content(state: ButtonState, theme: &Theme) -> Scene {
    let tree_items = inspector_tree_items(state, theme);
    let selected = use_selected_path().get();
    let focus = TreeViewFocus {
        focused_id: selected.as_deref(),
    };
    view_tree_focused(
        INSPECTOR_TREE_TAG,
        &tree_items,
        theme,
        &TreeViewStyle::m3_default(),
        &focus,
    )
}

/// R683.C §5.16 §5.49 — property pane content. Resolves
/// [`use_selected_path`] against the raw viewport scene via
/// [`find_node_at_path`] (the soft-signal walker — None on stale /
/// inspector-only paths returns the placeholder) and renders one row
/// per scene-node field. M3 Body Medium dense listing convention.
fn view_property_content(state: ButtonState, theme: &Theme) -> Scene {
    let viewport_scene = view_viewport_raw(state, theme);
    let selected = use_selected_path().get();
    let resolved = selected
        .as_deref()
        .and_then(|p| find_node_at_path(&viewport_scene, p));
    let rows: Vec<Scene> = match resolved {
        Some(node) => vec![
            property_row(format!("type: {}", scene_type_name(node)), theme),
            property_row(
                format!(
                    "path: {}",
                    selected
                        .as_deref()
                        .unwrap_or(PROPERTY_PANE_NO_SELECTION_TEXT)
                ),
                theme,
            ),
        ],
        None => vec![property_row(
            PROPERTY_PANE_NO_SELECTION_TEXT.to_string(),
            theme,
        )],
    };
    Scene::Container(
        ContainerNode::new(rows)
            .with_tag(PROPERTY_PANE_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_justify(JustifyContent::Start)
                    .with_padding(Rect::new(8, 8, 8, 8))
                    .with_gap(4),
            ),
    )
}

/// One row of the property pane — `Scene::Text` with the M3 Body
/// Medium font scale + the surface-aware foreground.
fn property_row(text: String, theme: &Theme) -> Scene {
    Scene::Text(TextNode::styled(
        text,
        Rect::default(),
        TextStyle::new()
            .with_size_px(PROPERTY_PANE_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ))
}

/// R683.C §5.16 §5.45 — dock-or-placeholder dispatch. When `panel_id`
/// is docked (no matching floating window in the topology) returns
/// the canonical [`view_dock_panel`] composition; when floating,
/// returns a "(panel torn off)" placeholder Container so the layout
/// slot stays present + ready to receive the panel back on dock-back.
fn dock_or_placeholder<F>(
    state: ButtonState,
    theme: &Theme,
    panel_id: &'static str,
    content_fn: F,
) -> Scene
where
    F: FnOnce(ButtonState, &Theme) -> Scene,
{
    let panels = use_windows_topology().get();
    if is_panel_floating(&panels, panel_id) {
        view_floating_placeholder_for(panel_id, theme)
    } else {
        let style = DockPanelStyle::m3_default(panel_id);
        view_dock_panel(
            panel_title_for(panel_id),
            content_fn(state, theme),
            theme,
            &style,
        )
    }
}

/// Placeholder Container painted in the dock slot when its panel is
/// currently floating. R685 §5.16 §5.49 — substrate consumer
/// (lifted into [`pinion_widget_paint::dock::view_floating_placeholder`]
/// in the R685 round as the [[abstraction-needs-second-consumer]]
/// Rule-of-Three closure; pre-R685 this was a binding-local
/// composition. The lifted form takes a [`FloatingPlaceholderStyle`]
/// sidecar; passing `with_label_font_size_px(PROPERTY_PANE_FONT_PX)`
/// preserves the pre-lift 14-px label size bit-identically.
fn view_floating_placeholder_for(panel_id: &str, theme: &Theme) -> Scene {
    view_floating_placeholder(
        panel_id,
        theme,
        &FloatingPlaceholderStyle::m3_default().with_label_font_size_px(PROPERTY_PANE_FONT_PX),
    )
}

/// R683.C §5.16 §5.41 §5.45 — main-window dock layout. Composes the
/// 3 panels through a Splitter(H) → Splitter(V) hierarchy with
/// dock-or-placeholder substitution per panel.
fn view_main_dock(state: ButtonState) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let inspector_pane =
        dock_or_placeholder(state, &theme, INSPECTOR_PANEL_TAG, view_inspector_content);
    let property_pane =
        dock_or_placeholder(state, &theme, PROPERTY_PANEL_TAG, view_property_content);
    let viewport_pane =
        dock_or_placeholder(state, &theme, VIEWPORT_PANEL_TAG, view_viewport_content);

    let left_ratio = use_left_split_ratio();
    let main_ratio = use_main_split_ratio();

    let left_column = view_splitter(
        inspector_pane,
        property_pane,
        &left_ratio,
        &theme,
        &SplitterStyle::m3_default(SplitterOrientation::Vertical, LEFT_SPLITTER_TAG),
        false,
    );
    view_splitter(
        left_column,
        viewport_pane,
        &main_ratio,
        &theme,
        &SplitterStyle::m3_default(SplitterOrientation::Horizontal, MAIN_SPLITTER_TAG),
        false,
    )
}

/// R683.C §5.16 §5.45 — floating-window paint. Wraps the addressed
/// panel's content in a `view_dock_panel` so the floating window
/// carries a draggable header (a second tear-off gesture on the
/// floating header re-emits the `tear_off` intent → the reducer
/// removes the WindowSpec → dock-back). The same
/// [`DockPanelExternal`] instance services both the docked and the
/// floating header (1 External per panel, shared across windows; each
/// per-window `InputRouter` resolves the composite-tag against its
/// own paint scene).
fn view_floating_panel(panel_id: &str, state: ButtonState) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let (content, panel_tag) = match panel_id {
        INSPECTOR_PANEL_TAG => (view_inspector_content(state, &theme), INSPECTOR_PANEL_TAG),
        PROPERTY_PANEL_TAG => (view_property_content(state, &theme), PROPERTY_PANEL_TAG),
        VIEWPORT_PANEL_TAG => (view_viewport_content(state, &theme), VIEWPORT_PANEL_TAG),
        _ => {
            // Defensive: unknown panel id (e.g. a future round adds a
            // 4th panel without updating this match) renders an
            // empty Container so the window is not blank-screen.
            return Scene::Container(ContainerNode::new(vec![]));
        }
    };
    let style = DockPanelStyle::m3_default(panel_tag);
    view_dock_panel(panel_title_for(panel_tag), content, &theme, &style)
}

// ─── trait wiring ────────────────────────────────────────────────────

/// Binding carrier. No fields — every trait method is associated.
struct DockPanelsView;

impl WidgetCore for DockPanelsView {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn tag() -> &'static str {
        // R55.G.17 — primary input-routing target. The viewport
        // Button is the single interactive primary widget; every
        // other interactive surface (splitter handles, dock-panel
        // headers, inspector rows) flows through `ExtraExternal`
        // registrations.
        VIEWPORT_BTN_TAG
    }

    fn title() -> &'static str {
        "hello-dock-panels (R683.C §5.16 §5.41 §5.45)"
    }

    fn create_external() -> Box<dyn pinion_core::external::External> {
        Box::new(ButtonExternal::new())
    }

    /// R683.C §5.16 §5.41 §5.45 §5.49 — 7 extra Externals:
    ///
    /// 1. Inspector dock-panel tear-off External — `inspector#header`.
    /// 2. Property dock-panel tear-off External — `property#header`.
    /// 3. Viewport dock-panel tear-off External — `viewport#header`.
    /// 4. Main splitter (Horizontal) drag External — `main_splitter`.
    /// 5. Left splitter (Vertical) drag External — `left_splitter`.
    /// 6. Inspector tree row click + hover External — `inspector_tree`.
    ///    (R675/R678 substrate, same as hello-multi-window's inspector.)
    /// 7. Viewport click router — `viewport_click_router` (R683.C
    ///    2nd `ClickRouter` consumer, [[abstraction-needs-second-consumer]]
    ///    closure).
    ///
    /// Two splitter Externals share their ratio Signal handles with
    /// the view fn through `Owner::cache` — the same `Rc<Signal<f32>>`
    /// instance lives in both the External (write-side) and the
    /// `view_splitter` paint helper (read-side), so a drag on the
    /// handle propagates to layout reflow on the next paint without
    /// manual subscription routing.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let main_ratio = use_main_split_ratio();
        let left_ratio = use_left_split_ratio();
        vec![
            // R683.C InputRouter routing correction — register each
            // DockPanelExternal at the panel root tag, NOT the composite
            // header tag. R51.42 `dispatch_send` + `forward_pointer_move`
            // split paint tags at `#` and look up the state-scene
            // External by the primary half; the External must live at
            // the primary half (panel root tag) to receive events. See
            // `pinion_widget_paint::dock::DockPanelExternal` rustdoc.
            ExtraExternal::new(
                INSPECTOR_PANEL_TAG,
                Box::new(DockPanelExternal::new(
                    INSPECTOR_PANEL_TAG,
                    TEAR_OFF_FRAC_DEFAULT,
                )),
            ),
            ExtraExternal::new(
                PROPERTY_PANEL_TAG,
                Box::new(DockPanelExternal::new(
                    PROPERTY_PANEL_TAG,
                    TEAR_OFF_FRAC_DEFAULT,
                )),
            ),
            ExtraExternal::new(
                VIEWPORT_PANEL_TAG,
                Box::new(DockPanelExternal::new(
                    VIEWPORT_PANEL_TAG,
                    TEAR_OFF_FRAC_DEFAULT,
                )),
            ),
            ExtraExternal::new(
                MAIN_SPLITTER_TAG,
                Box::new(
                    SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(main_ratio),
                ),
            ),
            ExtraExternal::new(
                LEFT_SPLITTER_TAG,
                Box::new(
                    SplitterExternal::new(SplitterOrientation::Vertical).attach_ratio(left_ratio),
                ),
            ),
            ExtraExternal::new(INSPECTOR_TREE_TAG, Box::new(TreeRowClickExternal::new())),
            ExtraExternal::new(VIEWPORT_CLICK_ROUTER_TAG, Box::new(ClickRouter::new())),
        ]
    }

    fn read_state(scene: &Scene) -> Self::State {
        // R55.D.5 — `create_extra_externals` is non-empty so the
        // state scene root is `Scene::Container([primary, ...extras])`.
        // Locate the primary `ButtonExternal` by its tag rather than
        // pattern-matching `Scene::External` at the root.
        if let Some(node) = scene.find_external_with_tag(VIEWPORT_BTN_TAG)
            && let Some(intro) = node.handle.introspect()
            && let Some(IntrospectValue::Text(name)) = intro.query("state")
        {
            return <Self::State as pinion_core::WidgetStateName>::from_name_or_default(&name);
        }
        ButtonState::Idle
    }

    fn view(state: Self::State, _frame: &Frame) -> Scene {
        // Default `WidgetCore::view` body — single-window fallback.
        // The live render loop always calls `view_for_window` (R670.B
        // dispatch); this path is the legacy single-window route the
        // RPC `scene/snapshot` request without `{window}` resolves
        // through the default `view_for_window` impl that forwards
        // back here.
        view_main_dock(state)
    }

    fn event_name(event: Self::Event) -> &'static str {
        <Self::Event as pinion_core::WidgetEventName>::as_name(&event)
    }

    /// R683.C §5.16 §5.20 §5.45 §5.49 — reducer arms for the
    /// dock + cross-panel select arc.
    fn update(_state: Self::State, intent: &Intent) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            // Tear-off arms — one per panel. Payload carries the
            // panel id the DockPanelExternal was constructed with.
            tag if tag == INSPECTOR_TEAR_OFF_INTENT_TAG
                || tag == PROPERTY_TEAR_OFF_INTENT_TAG
                || tag == VIEWPORT_TEAR_OFF_INTENT_TAG =>
            {
                if let IntrospectValue::Text(panel_id) = &intent.payload {
                    toggle_panel_floating(panel_id);
                }
            }
            // Cross-panel select — inspector tree row click writes
            // the resolved tree path into the shared `selected_path`
            // Signal so the viewport repaints with the highlight wrap
            // + the property pane updates its field listing.
            tag if tag == INSPECTOR_CLICK_INTENT_TAG => {
                if let IntrospectValue::Text(path) = &intent.payload {
                    use_selected_path().set(Some(path.clone()));
                }
            }
            // Cross-panel hover — same arc as the select but writing
            // the parallel `hovered_path` Signal. PointerLeave clears.
            tag if tag == INSPECTOR_HOVER_INTENT_TAG => match &intent.payload {
                IntrospectValue::Text(path) => use_hovered_path().set(Some(path.clone())),
                IntrospectValue::Null => use_hovered_path().set(None),
                _ => {}
            },
            // AI-driven viewport select — `scene/invoke
            // /viewport_click_router/external/click {Text(path)}`
            // writes the supplied path into the shared Signal.
            tag if tag == VIEWPORT_CLICK_ROUTER_INTENT_TAG => match &intent.payload {
                IntrospectValue::Text(path) => use_selected_path().set(Some(path.clone())),
                IntrospectValue::Null => use_selected_path().set(None),
                _ => {}
            },
            // User-mouse-driven viewport select — the viewport
            // Button's SCXML Pressed→Hover emit. The reducer mirrors
            // the button's known raw path into the Signal so the
            // inspector + property panes paint the focus state-layer
            // in lockstep with a real user click.
            tag if tag == VIEWPORT_BTN_CLICK_INTENT_TAG => {
                use_selected_path().set(Some(VIEWPORT_BTN_RAW_PATH.to_owned()));
            }
            _ => {}
        }
        Vec::new()
    }

    /// R811 §5.16 §5.27 §5.50 — WAI-ARIA APG 6.13 Tree keyboard
    /// navigation for the inspector, the second consumer of the lifted
    /// `pinion_core::widgets::tree_nav` substrate (`hello-tree-view` is
    /// the first). Gated on the inspector tree holding keyboard focus
    /// (routing and focus are separate axes): the viewport button keeps
    /// Space/Enter for its own activation when *it* is focused.
    ///
    /// Selection-follows-focus — arrows move the shared
    /// [`use_selected_path`] cursor, which already drives the inspector
    /// focus state-layer + the viewport highlight bridge, so navigating
    /// the tree previews the corresponding scene node. Expand / collapse
    /// / toggle write the [`use_collapsed_paths`] overlay; an
    /// unrecognised printable key falls through to
    /// [`inspector_typeahead`].
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(INSPECTOR_TREE_TAG) {
            return false;
        }
        let state = Self::read_state(scene);
        let theme = use_theme(THEME_TAG).theme_animated();
        let items = inspector_tree_items(state, &theme);
        let rows = flat_visible(&items);
        let selected = use_selected_path();
        let current = selected.get();
        match resolve_tree_key(&rows, current.as_deref(), key, INSPECTOR_NAV_PAGE) {
            TreeKey::Focus(id) => {
                selected.set(Some(id));
                true
            }
            TreeKey::Expand(id) => {
                set_collapsed(&id, false);
                true
            }
            TreeKey::Collapse(id) => {
                set_collapsed(&id, true);
                true
            }
            TreeKey::Toggle(id) => {
                toggle_collapsed(&id);
                true
            }
            TreeKey::Consumed => true,
            TreeKey::Unhandled => {
                tree_typeahead_jump(&selected, &rows, INSPECTOR_TYPEAHEAD_KEY, key)
            }
        }
    }
}

/// R813 §5.40 §5.50 — which window currently hosts the inspector tree
/// (so its AT nodes land in the window that actually paints them, not as
/// ghosts in a sibling window). The inspector pane shows in the main dock
/// while docked, and moves to its own floating window once torn off.
fn inspector_host_window() -> String {
    let panels = use_windows_topology().get();
    if is_panel_floating(&panels, INSPECTOR_PANEL_TAG) {
        floating_window_id(INSPECTOR_PANEL_TAG)
    } else {
        "main".to_owned()
    }
}

impl WidgetA11y for DockPanelsView {
    /// R812 §5.40 §5.27 — when the inspector tree owns the keyboard tab
    /// stop, AT focus stays on the tree root and `aria-activedescendant`
    /// names the selection-follows-focus cursor row (the WAI-ARIA
    /// roving-tabindex tree model: one tab stop, a moving active
    /// descendant). Any other focus (the viewport button) is atomic.
    ///
    /// Global (not per-window): the shell passes this one target to every
    /// window's `AccessTreeBuilder`, which drops the composite *parent* tag
    /// back to the window root for any window whose node set
    /// ([`Self::access_node_for_window`]) lacks it (R813) — but it sets the
    /// active-descendant **child** tag unconditionally. So the binding must
    /// gate the child itself: R945.1 routes the selection through
    /// [`effective_cursor`], emitting the composite only while the selected
    /// path still names a visible row. A pointer selection (the viewport
    /// click router) can land on a node whose inspector-tree ancestor is then
    /// collapsed; without this gate the active descendant would dangle at a
    /// node the AT can no longer reach (the lazy-outliner R944 gate, lifted
    /// here on its second consumer).
    fn access_focus_target(state: &Self::State, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(INSPECTOR_TREE_TAG)
            && let Some(id) = use_selected_path().get()
        {
            let theme = use_theme(THEME_TAG).theme_animated();
            let rows = flat_visible(&inspector_tree_items(*state, &theme));
            if let Some(visible) = effective_cursor(&rows, Some(&id)) {
                return Some(AccessFocus::composite(
                    INSPECTOR_TREE_TAG,
                    composite_row_tag(INSPECTOR_TREE_TAG, visible),
                ));
            }
        }
        focused.map(AccessFocus::atomic)
    }
}

impl WidgetView for DockPanelsView {
    type Renderer = HelloDockPanelsRenderer;

    /// R812 §5.40 §5.50 §5.27 / R813 §5.40 §5.16 — the `DevTools`
    /// inspector tree's WAI-ARIA `tree` + `treeitem` semantic tree, built
    /// through the lifted [`tree_access_nodes`](pinion_a11y::tree_access_nodes)
    /// substrate (the second consumer; `hello-tree-view` is the first).
    /// Clears the R811 inspector "AT-tree nodes 0" carry: the R811
    /// keyboard nav now has an AT counterpart, so a screen reader
    /// announces "tree item, <label>, level N, M of K, expanded" as the
    /// cursor moves. The selection-follows-focus row ([`use_selected_path`])
    /// carries `aria-selected`; the expand state rides the same
    /// [`flat_visible`] SSOT the painted glyph and keyboard model read, so
    /// AT state can never disagree with the screen.
    ///
    /// R813 §5.40 — emitted only for the window that actually paints the
    /// tree ([`inspector_host_window`]): the main dock while docked, the
    /// floating window once torn off. Every other window returns no nodes,
    /// so a sibling window's AT tree never carries ghost inspector rows.
    fn access_node_for_window(
        window_id: &str,
        state: &Self::State,
        _focused: Option<&str>,
    ) -> Vec<AccessNode> {
        if window_id != inspector_host_window() {
            return Vec::new();
        }
        let theme = use_theme(THEME_TAG).theme_animated();
        let items = inspector_tree_items(*state, &theme);
        let rows = flat_visible(&items);
        let selected = use_selected_path().get();
        // R868 — the inspector's selection is also its keyboard cursor (the
        // `access_focus_target` override already names it as the active
        // descendant); pass it as `focused_id` so the row node carries
        // `with_focused` to match.
        tree_access_nodes(
            INSPECTOR_TREE_TAG,
            INSPECTOR_TREE_TAG,
            Some(INSPECTOR_TREE_NAME),
            &rows,
            selected.as_deref(),
            selected.as_deref(),
        )
    }

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: MAIN_W,
            height: MAIN_H,
        }
    }

    /// R683.A + R683.C §5.16 §5.41 — opt-in runtime window topology.
    /// The shell's `reconcile_windows` Effect subscribes the returned
    /// Signal at boot + drives winit window add/drop on each
    /// `Signal::set`. Initial topology is the canonical single main
    /// window; tear-off intents append floating-window specs.
    fn windows_signal() -> Option<Rc<Signal<Vec<WindowSpec>>>> {
        Some(use_windows_topology())
    }

    /// R670.B + R683.A §5.16 — per-window paint dispatch. The main
    /// window paints the dock layout; floating windows (id prefix
    /// `torn-`) paint their hosted panel via [`view_floating_panel`].
    fn view_for_window(window_id: &str, state: Self::State, _frame: &Frame) -> Scene {
        match window_id {
            "main" => view_main_dock(state),
            other => match other.strip_prefix(FLOATING_WINDOW_PREFIX) {
                Some(panel_id) => view_floating_panel(panel_id, state),
                None => view_main_dock(state),
            },
        }
    }
}

fn main() {
    pinion_shell::run::<DockPanelsView>();
}

#[cfg(test)]
mod tests {
    //! R683.C §5.16 §5.41 §5.45 §5.49 — `hello-dock-panels` regression
    //! suite. Pins:
    //!
    //! * `tag()` returns the viewport Button tag (primary input
    //!   routing target).
    //! * `windows_signal()` returns `Some(..)` with the canonical
    //!   single-main initial topology.
    //! * `toggle_panel_floating` round-trips the windows signal
    //!   (tear-off appends, dock-back removes).
    //! * The dock-or-placeholder substitution picks the right branch
    //!   per panel state.
    //! * `view_for_window` dispatches main vs floating by id prefix.
    //! * `create_extra_externals` registers all 7 expected entries
    //!   in declaration order.
    //! * The reducer arms route their intents into the shared
    //!   Signals.

    use super::*;
    use pinion_core::Owner;

    /// Test helper — depth-first walk for any text node containing
    /// `needle`. Hoisted to the test mod's top so
    /// `clippy::items_after_statements` stays clean.
    fn walk_for_text(scene: &Scene, needle: &str) -> bool {
        match scene {
            Scene::Text(t) => t.content.contains(needle),
            Scene::Container(c) => c.children.iter().any(|ch| walk_for_text(ch, needle)),
            _ => false,
        }
    }

    /// Test helper — depth-first walk for any Container carrying a
    /// `Border` in its `BoxStyle`. Used to verify the highlight wrap
    /// paints a stroked outline on the selected node.
    fn walk_for_border(scene: &Scene) -> bool {
        match scene {
            Scene::Container(c) => {
                c.style.border.is_some() || c.children.iter().any(walk_for_border)
            }
            _ => false,
        }
    }

    #[test]
    fn r683_c_tag_is_viewport_btn() {
        assert_eq!(<DockPanelsView as WidgetCore>::tag(), VIEWPORT_BTN_TAG);
    }

    #[test]
    fn r683_c_title_is_non_empty() {
        assert!(!<DockPanelsView as WidgetCore>::title().is_empty());
    }

    #[test]
    fn r683_c_initial_size_strategy_is_fixed_main_wxh() {
        match <DockPanelsView as WidgetView>::initial_size_strategy() {
            SizeStrategy::Fixed { width, height } => {
                assert_eq!(width, MAIN_W);
                assert_eq!(height, MAIN_H);
            }
            other => panic!("expected Fixed strategy, got {other:?}"),
        }
    }

    #[test]
    fn r683_c_windows_signal_opt_in_returns_some() {
        let signal = Owner::new()
            .run(<DockPanelsView as WidgetView>::windows_signal)
            .expect("windows_signal must be Some — R683.A opt-in");
        let initial = signal.get();
        assert_eq!(initial.len(), 1, "initial topology has one main window");
        assert_eq!(initial[0].id.as_ref(), "main");
    }

    #[test]
    fn r683_c_windows_signal_is_memoised_across_calls() {
        // The same Owner scope must hand back identical `Rc<Signal<..>>`
        // identity across calls so the shell's reconcile Effect's
        // subscription is stable (mutations propagate to the same
        // subscriber list).
        let owner = Owner::new();
        let first = owner
            .run(<DockPanelsView as WidgetView>::windows_signal)
            .unwrap();
        let second = owner
            .run(<DockPanelsView as WidgetView>::windows_signal)
            .unwrap();
        assert!(
            Rc::ptr_eq(&first, &second),
            "Owner::cache must hand back the same Rc<Signal<..>> instance",
        );
    }

    #[test]
    fn r683_c_floating_window_id_format() {
        assert_eq!(floating_window_id("inspector"), "torn-inspector");
        assert_eq!(floating_window_id("property"), "torn-property");
        assert_eq!(floating_window_id("viewport"), "torn-viewport");
    }

    #[test]
    fn r683_c_is_panel_floating_detects_matching_window() {
        let panels = vec![
            WindowSpec::new(
                Cow::Borrowed("main"),
                "Main",
                SizeStrategy::Fixed {
                    width: 100,
                    height: 100,
                },
            ),
            WindowSpec::new(
                Cow::Owned("torn-inspector".to_string()),
                "Floating Inspector",
                SizeStrategy::Fixed {
                    width: 200,
                    height: 200,
                },
            ),
        ];
        assert!(is_panel_floating(&panels, "inspector"));
        assert!(!is_panel_floating(&panels, "property"));
        assert!(!is_panel_floating(&panels, "viewport"));
    }

    #[test]
    fn r683_c_toggle_panel_floating_round_trip() {
        Owner::new().run(|| {
            let signal = use_windows_topology();
            assert_eq!(signal.get().len(), 1, "initial single main window");

            // First toggle: inspector floats.
            toggle_panel_floating("inspector");
            let after_tear = signal.get();
            assert_eq!(after_tear.len(), 2);
            assert!(
                after_tear.iter().any(|w| w.id == "torn-inspector"),
                "tear-off must append a torn-inspector WindowSpec",
            );

            // Second toggle: inspector docks back.
            toggle_panel_floating("inspector");
            let after_dock = signal.get();
            assert_eq!(
                after_dock.len(),
                1,
                "dock-back must remove the floating spec"
            );
            assert!(after_dock.iter().all(|w| w.id != "torn-inspector"));
        });
    }

    /// (R813 §5.40 §5.16) Per-window AT nodes: the inspector tree's
    /// `tree` + `treeitem` subtree is emitted only for the window that
    /// paints it — "main" while docked, "torn-inspector" once floated —
    /// and a sibling window's node set is empty (no cross-window ghosts).
    #[test]
    fn r813_inspector_at_nodes_emit_for_host_window_only() {
        use pinion_a11y::AriaRole;
        Owner::new().run(|| {
            // Default topology: inspector docked → host is "main".
            let main = <DockPanelsView as WidgetView>::access_node_for_window(
                "main",
                &ButtonState::Idle,
                None,
            );
            assert!(
                !main.is_empty(),
                "docked inspector emits its tree for the main window"
            );
            assert_eq!(main[0].role, AriaRole::Tree, "root node is role=tree");
            assert_eq!(main[0].tag, INSPECTOR_TREE_TAG);
            // A non-host sibling window carries no ghost inspector nodes.
            let sibling = <DockPanelsView as WidgetView>::access_node_for_window(
                "torn-viewport",
                &ButtonState::Idle,
                None,
            );
            assert!(
                sibling.is_empty(),
                "non-host window AT tree has no inspector ghost nodes"
            );

            // Float the inspector: the host moves to its floating window.
            toggle_panel_floating(INSPECTOR_PANEL_TAG);
            let main_after = <DockPanelsView as WidgetView>::access_node_for_window(
                "main",
                &ButtonState::Idle,
                None,
            );
            assert!(
                main_after.is_empty(),
                "floated inspector leaves no ghosts in the main window"
            );
            let floating = <DockPanelsView as WidgetView>::access_node_for_window(
                &floating_window_id(INSPECTOR_PANEL_TAG),
                &ButtonState::Idle,
                None,
            );
            assert!(
                !floating.is_empty(),
                "the floating inspector window now carries the tree"
            );
            assert_eq!(floating[0].role, AriaRole::Tree);
        });
    }

    #[test]
    fn r683_c_multi_tear_off_appends_three_floating_windows() {
        Owner::new().run(|| {
            let signal = use_windows_topology();
            toggle_panel_floating("inspector");
            toggle_panel_floating("property");
            toggle_panel_floating("viewport");
            let panels = signal.get();
            assert_eq!(panels.len(), 4, "main + 3 floating");
            assert!(panels.iter().any(|w| w.id == "torn-inspector"));
            assert!(panels.iter().any(|w| w.id == "torn-property"));
            assert!(panels.iter().any(|w| w.id == "torn-viewport"));
        });
    }

    #[test]
    fn r683_c_panel_title_for_known_ids() {
        assert_eq!(panel_title_for(INSPECTOR_PANEL_TAG), "Inspector");
        assert_eq!(panel_title_for(PROPERTY_PANEL_TAG), "Property");
        assert_eq!(panel_title_for(VIEWPORT_PANEL_TAG), "Viewport");
        assert_eq!(panel_title_for("unknown"), "Panel");
    }

    #[test]
    fn r683_c_create_extra_externals_registers_seven_entries() {
        Owner::new().run(|| {
            let extras = <DockPanelsView as WidgetCore>::create_extra_externals();
            assert_eq!(extras.len(), 7);
            // Declaration order: dock-panel tear-off Externals (3),
            // splitter Externals (2), inspector tree (1), click router (1).
            let tags: Vec<&str> = extras.iter().map(|e| e.tag.as_ref()).collect();
            assert_eq!(
                tags,
                vec![
                    INSPECTOR_PANEL_TAG,
                    PROPERTY_PANEL_TAG,
                    VIEWPORT_PANEL_TAG,
                    MAIN_SPLITTER_TAG,
                    LEFT_SPLITTER_TAG,
                    INSPECTOR_TREE_TAG,
                    VIEWPORT_CLICK_ROUTER_TAG,
                ],
            );
        });
    }

    #[test]
    fn r683_c_view_main_dock_contains_all_three_panel_tags_when_none_floating() {
        Owner::new().run(|| {
            let scene = view_main_dock(ButtonState::Idle);
            assert!(scene.contains_tag(INSPECTOR_PANEL_TAG));
            assert!(scene.contains_tag(PROPERTY_PANEL_TAG));
            assert!(scene.contains_tag(VIEWPORT_PANEL_TAG));
        });
    }

    #[test]
    fn r683_c_view_main_dock_contains_splitter_tags() {
        Owner::new().run(|| {
            let scene = view_main_dock(ButtonState::Idle);
            assert!(scene.contains_tag(MAIN_SPLITTER_TAG));
            assert!(scene.contains_tag(LEFT_SPLITTER_TAG));
        });
    }

    #[test]
    fn r683_c_view_main_dock_substitutes_placeholder_when_panel_floating() {
        Owner::new().run(|| {
            // Tear off inspector first.
            toggle_panel_floating(INSPECTOR_PANEL_TAG);
            let scene = view_main_dock(ButtonState::Idle);
            // Inspector panel tag should NOT appear in the dock
            // (placeholder takes its slot). The placeholder Container
            // is tagged `{panel_id}_placeholder`.
            assert!(
                !scene.contains_tag(INSPECTOR_PANEL_TAG),
                "torn-off inspector panel must not paint in main",
            );
            assert!(scene.contains_tag("inspector_placeholder"));
            // Other panels keep their slots.
            assert!(scene.contains_tag(PROPERTY_PANEL_TAG));
            assert!(scene.contains_tag(VIEWPORT_PANEL_TAG));
        });
    }

    #[test]
    fn r683_c_view_floating_panel_inspector_carries_inspector_tag() {
        Owner::new().run(|| {
            let scene = view_floating_panel(INSPECTOR_PANEL_TAG, ButtonState::Idle);
            assert!(scene.contains_tag(INSPECTOR_PANEL_TAG));
            assert!(scene.contains_tag(INSPECTOR_TREE_TAG));
        });
    }

    #[test]
    fn r683_c_view_floating_panel_viewport_carries_button_tag() {
        Owner::new().run(|| {
            let scene = view_floating_panel(VIEWPORT_PANEL_TAG, ButtonState::Idle);
            assert!(scene.contains_tag(VIEWPORT_PANEL_TAG));
            assert!(scene.contains_tag(VIEWPORT_BTN_TAG));
        });
    }

    #[test]
    fn r683_c_view_for_window_dispatches_main_id_to_dock_layout() {
        Owner::new().run(|| {
            let scene = <DockPanelsView as WidgetView>::view_for_window(
                "main",
                ButtonState::Idle,
                &Frame::new(),
            );
            assert!(scene.contains_tag(MAIN_SPLITTER_TAG));
        });
    }

    #[test]
    fn r683_c_view_for_window_dispatches_torn_id_to_floating_panel() {
        Owner::new().run(|| {
            let scene = <DockPanelsView as WidgetView>::view_for_window(
                "torn-viewport",
                ButtonState::Idle,
                &Frame::new(),
            );
            // Floating viewport carries the viewport tag and the
            // button tag, but NOT the splitter tags (no dock layout).
            assert!(scene.contains_tag(VIEWPORT_PANEL_TAG));
            assert!(!scene.contains_tag(MAIN_SPLITTER_TAG));
        });
    }

    #[test]
    fn r683_c_view_for_window_unknown_id_falls_back_to_main() {
        Owner::new().run(|| {
            let scene = <DockPanelsView as WidgetView>::view_for_window(
                "ghost",
                ButtonState::Idle,
                &Frame::new(),
            );
            // Unknown id paints the main dock as a defensive fallback.
            assert!(scene.contains_tag(MAIN_SPLITTER_TAG));
        });
    }

    #[test]
    fn r683_c_reducer_tear_off_intent_routes_to_toggle() {
        Owner::new().run(|| {
            let signal = use_windows_topology();
            assert_eq!(signal.get().len(), 1);

            let intent = Intent::new_static(
                INSPECTOR_TEAR_OFF_INTENT_TAG,
                IntrospectValue::Text(INSPECTOR_PANEL_TAG.into()),
            );
            let cmds = <DockPanelsView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert!(cmds.is_empty(), "reducer is side-effect-only");
            assert_eq!(signal.get().len(), 2, "inspector tear-off appended");
        });
    }

    #[test]
    fn r683_c_reducer_inspector_click_intent_writes_selected_path() {
        Owner::new().run(|| {
            let signal = use_selected_path();
            assert_eq!(signal.get(), None);
            let intent = Intent::new_static(
                INSPECTOR_CLICK_INTENT_TAG,
                IntrospectValue::Text("Container/Container[viewport_btn]".into()),
            );
            let _ = <DockPanelsView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert_eq!(
                signal.get().as_deref(),
                Some("Container/Container[viewport_btn]"),
            );
        });
    }

    #[test]
    fn r683_c_reducer_viewport_click_router_intent_writes_selected_path() {
        Owner::new().run(|| {
            let signal = use_selected_path();
            let intent = Intent::new_static(
                VIEWPORT_CLICK_ROUTER_INTENT_TAG,
                IntrospectValue::Text("AI/path".into()),
            );
            let _ = <DockPanelsView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert_eq!(signal.get().as_deref(), Some("AI/path"));
        });
    }

    #[test]
    fn r683_c_reducer_viewport_btn_click_writes_canonical_raw_path() {
        Owner::new().run(|| {
            let signal = use_selected_path();
            // Pre-condition: clear the slot so the assertion is
            // self-contained.
            signal.set(None);
            let intent = Intent::new_static(VIEWPORT_BTN_CLICK_INTENT_TAG, IntrospectValue::Null);
            let _ = <DockPanelsView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert_eq!(signal.get().as_deref(), Some(VIEWPORT_BTN_RAW_PATH));
        });
    }

    #[test]
    fn r683_c_reducer_viewport_click_router_null_deselects() {
        Owner::new().run(|| {
            let signal = use_selected_path();
            signal.set(Some("primed".into()));
            let intent =
                Intent::new_static(VIEWPORT_CLICK_ROUTER_INTENT_TAG, IntrospectValue::Null);
            let _ = <DockPanelsView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert_eq!(signal.get(), None, "Null payload deselects");
        });
    }

    #[test]
    fn r683_c_reducer_hover_intent_routes_to_hovered_path_signal() {
        Owner::new().run(|| {
            let signal = use_hovered_path();
            signal.set(None);
            let intent = Intent::new_static(
                INSPECTOR_HOVER_INTENT_TAG,
                IntrospectValue::Text("Container".into()),
            );
            let _ = <DockPanelsView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert_eq!(signal.get().as_deref(), Some("Container"));

            let leave = Intent::new_static(INSPECTOR_HOVER_INTENT_TAG, IntrospectValue::Null);
            let _ = <DockPanelsView as WidgetCore>::update(ButtonState::Idle, &leave);
            assert_eq!(signal.get(), None, "PointerLeave clears hover");
        });
    }

    #[test]
    fn r683_c_view_viewport_raw_contains_button_tag() {
        Owner::new().run(|| {
            let theme = use_theme(THEME_TAG).theme_animated();
            let scene = view_viewport_raw(ButtonState::Idle, &theme);
            assert!(scene.contains_tag(VIEWPORT_BTN_TAG));
        });
    }

    #[test]
    fn r683_c_static_button_raw_path_resolves_against_raw_viewport_scene() {
        Owner::new().run(|| {
            let theme = use_theme(THEME_TAG).theme_animated();
            let raw = view_viewport_raw(ButtonState::Idle, &theme);
            assert!(
                find_node_at_path(&raw, VIEWPORT_BTN_RAW_PATH).is_some(),
                "static raw path must resolve in the raw viewport scene",
            );
        });
    }

    #[test]
    fn r683_c_property_pane_shows_no_selection_placeholder_when_signal_empty() {
        Owner::new().run(|| {
            use_selected_path().set(None);
            let theme = use_theme(THEME_TAG).theme_animated();
            let scene = view_property_content(ButtonState::Idle, &theme);
            assert!(walk_for_text(&scene, PROPERTY_PANE_NO_SELECTION_TEXT));
        });
    }

    #[test]
    fn r683_c_view_main_dock_paints_selection_wrap_when_signal_set() {
        Owner::new().run(|| {
            // Reset other signals so the test is self-contained.
            use_windows_topology().set(vec![WindowSpec::main(
                "Main",
                SizeStrategy::Fixed {
                    width: MAIN_W,
                    height: MAIN_H,
                },
            )]);
            use_selected_path().set(Some(VIEWPORT_BTN_RAW_PATH.into()));
            let scene = view_main_dock(ButtonState::Idle);
            // The highlight wrap inserts a Border on the matched node;
            // the wrap walker descends the viewport's dock-panel
            // content. Check that any node carries a Border.
            assert!(
                walk_for_border(&scene),
                "selection wrap must paint a Border somewhere in the dock layout",
            );
        });
    }

    #[test]
    fn r945_1_inspector_focus_target_gates_a_collapsed_selection() {
        // R945.1 — the inspector's keyboard cursor / pointer selection lowers to
        // aria-activedescendant; it must NOT dangle when the selected node leaves
        // the visible set (a pointer selection under a since-collapsed ancestor).
        // The shell sets the active-descendant child unconditionally, so the
        // binding gates through the lifted `effective_cursor`.
        Owner::new().run(|| {
            let theme = use_theme(THEME_TAG).theme_animated();
            let rows = flat_visible(&inspector_tree_items(ButtonState::Idle, &theme));
            assert!(
                rows.len() > 1,
                "inspector boots with a populated, expanded tree"
            );

            // (1) A selection on a visible row → composite naming that row.
            let visible = rows[0].id.clone();
            use_selected_path().set(Some(visible.clone()));
            let af =
                DockPanelsView::access_focus_target(&ButtonState::Idle, Some(INSPECTOR_TREE_TAG))
                    .expect("a focused inspector yields a focus target");
            assert_eq!(
                af.active_descendant.as_deref(),
                Some(composite_row_tag(INSPECTOR_TREE_TAG, &visible).as_str()),
                "a visible selection names the active descendant",
            );

            // (2) An off-tree selection never produces a dangling composite.
            use_selected_path().set(Some("ghost/path/never/in/the/tree".to_owned()));
            let af =
                DockPanelsView::access_focus_target(&ButtonState::Idle, Some(INSPECTOR_TREE_TAG))
                    .expect("focus target present");
            assert_eq!(
                af.active_descendant, None,
                "off-tree selection drops to atomic"
            );
            assert_eq!(
                af.focus_tag, INSPECTOR_TREE_TAG,
                "focus stays on the tree, atomic"
            );

            // (3) The realistic trigger: select a child, then collapse its parent
            // so the child leaves the visible set → the gate drops to atomic.
            if let Some(bi) = (0..rows.len() - 1).find(|&i| rows[i + 1].depth > rows[i].depth) {
                let parent = rows[bi].id.clone();
                let child = rows[bi + 1].id.clone();
                use_selected_path().set(Some(child.clone()));
                set_collapsed(&parent, true);
                let after = flat_visible(&inspector_tree_items(ButtonState::Idle, &theme));
                assert!(
                    !after.iter().any(|r| r.id == child),
                    "the collapse hid the selected child",
                );
                let af = DockPanelsView::access_focus_target(
                    &ButtonState::Idle,
                    Some(INSPECTOR_TREE_TAG),
                )
                .expect("focus target present");
                assert_eq!(
                    af.active_descendant, None,
                    "a selection under a collapsed branch never dangles the active descendant",
                );
            }
        });
    }
}
