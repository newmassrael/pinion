// R670.B §5.16 — example bindings tolerate looser doc-markdown lints
// than the substrate crates. The workspace `clippy::pedantic` floor
// otherwise demands every identifier in every doc comment carry
// backticks; for an example-binding main.rs with dense
// architecture-narrative doc-comments, that pushes the lint
// surface into the prose itself. Substrate crates (pinion-shell /
// -tui / -rpc / -core / -derive) keep the strict default.
#![allow(clippy::doc_markdown)]

//! `hello-multi-window` — R670.B §5.16 §5.41 first multi-window
//! consumer. Phase B (R700+) first real dogfood of the R670.A
//! `WidgetView::windows() -> Vec<WindowSpec>` trait foundation + the
//! R670.B AppShell multi-window refactor + RPC `{window: "<id>"}`
//! per-window scope + `WidgetView::view_for_window` per-window paint
//! hook.
//!
//! ## Architecture
//!
//! - **Main window** (`id = "main"`, primary): a Button widget the
//!   user clicks. Standard hello-button shape — Idle/Hover/Pressed/
//!   Disabled state, ARIA Button role, Space/Enter keyboard activate.
//! - **Inspector window** (`id = "inspector"`, secondary): a Text
//!   node that mirrors the main window's `ButtonState` as
//!   `format!("{:?}", state)`. Read-only — clicks in the inspector
//!   do nothing (no interactive widgets there).
//!
//! Both windows share the same `ShellCore` (single binding state).
//! `WidgetView::view_for_window(window_id, state, frame)` switches
//! by window_id — main returns the Button scene, inspector returns
//! the state-debug text. State mutations in the main window
//! propagate to the inspector because both windows read the same
//! cached_state on every paint.
//!
//! ## Why this matters for the northern-star
//!
//! Multi-window is the structural unlock for Phase B (Pro GUI) and
//! Phase D (editor self-hosted in pinion). DevTools / Inspector /
//! floating panels all ride this substrate. `hello-multi-window`
//! is the smallest binding that exercises the full multi-window
//! dispatch chain (winit per-window event routing + AccessKit
//! per-window adapter + IME per-window session + per-window paint
//! cycle + RPC per-window scope) so substrate gaps surface
//! immediately rather than waiting for the first real DevTools
//! consumer.

use std::rc::Rc;

use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Color, Frame, Owner, Scene, Signal, WidgetCore};
#[cfg(test)]
use pinion_a11y::WidgetA11y;
// R813 §5.40 — per-window AT nodes for the inspector window (the 3rd
// consumer of the lifted tree AccessNode builder).
use pinion_a11y::{tree_access_nodes, AccessNode};
use pinion_core::widgets::tree_nav::flat_visible;
use pinion_shell::{vello_renderer_impl, SizeStrategy, WidgetView, WindowSpec};
// R678 §5.16 §5.49 — `DevTools` substrate (path addressing + highlight
// overlay composition) consumer; lifted from this binding at R678
// atomic (2) per [[abstraction-needs-second-consumer]] Rule-of-Three
// after the selection wrap (R676) + hover wrap (R678 atomic 1)
// surfaced as 2 consumers in `view_main`. The `_for_tree` aliases
// below preserve the binding's local call-site names where they still
// read more clearly in context (e.g. `find_main_node_at_path` named
// for its main-window scope) while the substrate names stay
// binding-agnostic per the substrate's contract.
use pinion_widget_paint::devtools::{
    find_node_at_path as find_main_node_at_path, rebuild_with_highlight_at_path,
    scene_root_path_segment, scene_to_tree_item, scene_type_name, ClickRouter,
};
use pinion_widget_paint::tree_view::{
    view_tree_focused, TreeItem, TreeRowClickExternal, TreeViewFocus, TreeViewStyle,
};

use pinion_widget_paint::state_layer::{HOVER, PRESSED};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloMultiWindowRenderer, HelloMultiWindowRendererError);

/// Main window dimensions — large enough for a comfortable Button
/// click target + the "Click me!" label.
const MAIN_W: u32 = 320;
const MAIN_H: u32 = 200;
/// R677 §5.16 §5.49 — inspector window dimensions sized for the new
/// 2-pane DevTools layout (tree pane + property pane side-by-side).
/// Width: 480 px hosts the tree column (~260 px paint width) plus a
/// ~220 px property pane that fits the Computed-pane field rows
/// without horizontal scroll. Height: 320 px clears 5+ tree rows
/// stacked (5×48 = 240 plus header room) so the full mirrored main
/// scene tree fits at one glance.
const INSPECTOR_W: u32 = 480;
const INSPECTOR_H: u32 = 320;

const BTN_W: u32 = 160;
const BTN_H: u32 = 64;
const LABEL_FONT_PX: u32 = 18;

/// Shared `ThemeProvider` cache tag — main + inspector share one
/// palette so a theme swap (e.g. an RPC `scene/theme_tokens`
/// future-axis push) repaints both windows consistently.
const THEME_TAG: &str = "app";

/// Tag the paint-side InputRouter hit-tests against. Routes pointer
/// events from the main window through to the `ButtonExternal` at
/// the scene root.
const MAIN_BTN_TAG: &str = "main_btn";

/// R671 §5.16 §5.50 — root tag of the inspector window's `TreeView`.
/// Composite-row tags are the form `inspector_tree#{path}` per the
/// [[multi-external-substrate-extra-externals-pattern]] convention.
/// `scene/snapshot {window: "inspector"}` walks down to per-row
/// containers via this prefix.
const INSPECTOR_TREE_TAG: &str = "inspector_tree";
/// R813 §5.40 — accessible name for the inspector's `role=tree` root.
const INSPECTOR_TREE_NAME: &str = "Main window scene";

/// R675 §5.16 §5.20 — dotted-form intent tag the
/// [`TreeRowClickExternal`] (substrate at
/// [`pinion_widget_paint::tree_view::TreeRowClickExternal`]) registered
/// under [`INSPECTOR_TREE_TAG`] emits on a completed row click. The
/// runtime intent-queue walker prefixes the substrate's bare
/// [`pinion_widget_paint::tree_view::TREE_ROW_CLICK_EVENT`] = `"click"`
/// with the producing External's tag = [`INSPECTOR_TREE_TAG`], so the
/// reducer matches `"inspector_tree.click"` literally per
/// [[intent-tag-dotted-wire-form]]. (`intent_tag!` macro is
/// `literal`-only at the stable-Rust layer so the tree tag has to
/// be a literal at the call site.)
const INSPECTOR_CLICK_INTENT_TAG: &str = intent_tag!("inspector_tree", "click");

/// R678 §5.16 §5.20 §5.49 — dotted-form intent tag the same
/// [`TreeRowClickExternal`] under [`INSPECTOR_TREE_TAG`] emits on a
/// hover-state transition (`PointerEnter` or `PointerLeave` on a
/// row). The runtime intent-queue walker prefixes the substrate's
/// bare [`pinion_widget_paint::tree_view::TREE_ROW_HOVER_EVENT`] =
/// `"hover"` with the External's tag, so the reducer matches
/// `"inspector_tree.hover"` literally per
/// [[intent-tag-dotted-wire-form]] — same shape as the click intent
/// arc, distinct event name so the reducer can dispatch each axis
/// independently.
///
/// Payload semantics (per the substrate's
/// [`pinion_widget_paint::tree_view::TREE_ROW_HOVER_EVENT`] contract):
/// `IntrospectValue::Text(path)` on Enter (mirror into the
/// [`use_hovered_path`] Signal as `Some(path)`),
/// `IntrospectValue::Null` on Leave (mirror as `None`).
const INSPECTOR_HOVER_INTENT_TAG: &str = intent_tag!("inspector_tree", "hover");

/// R679 §5.16 §5.49 — paint-side tag of the main window's
/// [`MainWindowClickRouter`] External. Registered as a 2nd
/// `ExtraExternal` next to the inspector window's
/// [`TreeRowClickExternal`]. The state scene now carries three
/// `ExternalNode` slots (ButtonExternal at [`MAIN_BTN_TAG`],
/// TreeRowClickExternal at [`INSPECTOR_TREE_TAG`],
/// MainWindowClickRouter at this tag) — per R55.D.5 multi-External
/// substrate the runtime composes them as a `Scene::Container`
/// holding all three so dispatchers walk DFS to resolve targets.
///
/// The router itself is **AI-invoke-only** in R679 — it has no
/// `send` paint-side handler. A `scene/invoke
/// /main_click_router/external/click` call writes the supplied
/// path into the cross-window [`use_selected_path`] Signal so the
/// inspector tree row paints the focus state-layer in lockstep.
/// The user-mouse path is closed independently by the
/// [`MAIN_BTN_CLICK_INTENT_TAG`] reducer arm below — when the
/// user clicks the main button, ButtonExternal fires the canonical
/// `click` intent and the reducer mirrors the button's known raw
/// path into the Signal. Background-click (user mouse on
/// non-button area) is **no-op** in R679; deselect is reachable
/// via AI invoke with a `Null` payload — pinned design decision
/// per the R679 atomic 1 scope note.
const MAIN_CLICK_ROUTER_TAG: &str = "main_click_router";

/// R679 §5.16 §5.20 §5.49 — dotted-form intent tag the
/// [`MainWindowClickRouter`] under [`MAIN_CLICK_ROUTER_TAG`] emits
/// when AI invokes the typed `click` shortcut. The runtime
/// intent-queue walker prefixes the substrate's bare event name
/// (`"click"`) with the producing External's tag, so the reducer
/// matches `"main_click_router.click"` literally per
/// [[intent-tag-dotted-wire-form]].
///
/// Payload semantics: `IntrospectValue::Text(path)` mirrors into
/// [`use_selected_path`] as `Some(path)` (AI-driven select);
/// `IntrospectValue::Null` mirrors as `None` (AI-driven
/// deselect). Symmetric with the
/// [[w3c-dom-selection-shape]] anchor/collapse pair — `Text` =
/// non-empty selection, `Null` = empty selection.
const MAIN_CLICK_ROUTER_INTENT_TAG: &str = intent_tag!("main_click_router", "click");

/// R679 §5.16 §5.20 §5.49 — dotted-form intent tag the
/// [`ButtonExternal`] under [`MAIN_BTN_TAG`] emits on each
/// completed `Pressed → Hover` cycle (the canonical button click
/// per [`pinion_core::widgets::button::Button`]'s SCXML transition
/// contract). The button's intent payload is always
/// [`IntrospectValue::Null`] — Button has no semantic value to
/// carry, the kind alone is the signal.
///
/// R679 grew an `update` reducer arm matching this tag to bridge
/// user-mouse-driven button presses into
/// [`use_selected_path`]. The known raw path the arm writes is
/// [`MAIN_BUTTON_RAW_PATH`].
const MAIN_BTN_CLICK_INTENT_TAG: &str = intent_tag!("main_btn", "click");

/// R679 §5.16 §5.49 — the **raw-scene** path identifying the
/// `Container[main_btn]` node inside the unwrapped
/// [`view_main_raw`] paint. Hard-coded because the binding's view
/// shape is stable (root untagged Container with the optional
/// banner Text + the tagged button Container; the button's tag-
/// form path takes priority over nth-of-type so it stays
/// `Container/Container[main_btn]` whether or not the banner sits
/// in front).
///
/// Why hard-coded vs. dynamic
/// [`pinion_widget_paint::devtools::path_for_paint_hit`] resolve?
/// The reducer arm fires on a `Null`-payload button-click intent
/// (per [`pinion_core::widgets::button::Button`]'s SCXML emit
/// contract) — the intent carries no coordinate to feed
/// `path_for_paint_hit`. A future-axis substrate that propagates
/// the originating cursor position into the button intent payload
/// would let the reducer resolve dynamically; for R679 the static
/// shape suffices because the binding tree is stable. The R679
/// atomic 0 substrate test
/// (`r679_path_for_paint_hit_on_tagged_container_returns_container_path`)
/// pins this exact path shape so a future refactor that drifts
/// the raw scene structure surfaces immediately at the substrate
/// level too.
const MAIN_BUTTON_RAW_PATH: &str = "Container/Container[main_btn]";

/// R677 §5.16 §5.49 — root tag of the inspector window's **property
/// pane** (right-hand pane in the new 2-pane DevTools layout). The
/// pane renders one [`TextNode`] per field of the
/// [`use_selected_path`]-resolved scene node — type, tag,
/// style.fill, style.border, layout.size, children count — mirroring
/// Chrome DevTools' Computed pane.
///
/// `scene/snapshot {window: "inspector"}` walks down to the pane
/// container via this tag; the demo introspects field rows by
/// scanning the pane's `Scene::Text` children.
const PROPERTY_PANE_TAG: &str = "property_pane";

/// R677 §5.16 §5.49 — placeholder text shown in the property pane
/// when [`use_selected_path`] returns `None` (no row selected) or
/// when [`find_main_node_at_path`] returns `None` (inspector-only id
/// like "state" / "main" or stale path). Kept short so the pane
/// reads as "I'm here, but nothing's selected" rather than blank.
const PROPERTY_PANE_NO_SELECTION_TEXT: &str = "(no selection)";

/// R677 §5.16 §5.49 — font size for property pane rows in logical
/// pixels. Matches M3 Body Medium (16 px) so the pane reads as a
/// dense field listing without crowding — the same scale the tree
/// pane uses for row labels.
const PROPERTY_PANE_FONT_PX: u32 = 16;

/// R677 §5.16 §5.49 — vertical gap between property pane rows in
/// logical pixels. M3 Density 0 lists use 4 px row spacing; the
/// property pane is closer to a dense data table so 4 px reads as
/// cohesive without crowding.
const PROPERTY_PANE_ROW_GAP_PX: u32 = 4;

/// R675 §5.16 §5.49 — cross-window shared selection slot.
///
/// Both windows reach the same `Rc<Signal<Option<String>>>` through
/// the single `ShellCore`'s `Owner::cache`. The inspector window's
/// click reducer writes the just-clicked tree row's path id here;
/// `view_inspector` reads the slot through `TreeViewFocus` to paint
/// the M3 focus state-layer on the selected row; `view_main` reads
/// the same slot to paint a "Selected: …" banner above the button.
/// The result is the first **cross-window reactive state-sync**
/// demonstration in pinion — both windows observe the same Signal,
/// both repaint on Signal mutation (the substrate's reactive
/// any-animation-active wire schedules the redraw), AI clients
/// observing through `scene/snapshot {window: …}` see the
/// synchronised projection.
///
/// `None` = no row selected; `Some(path)` = the inspector row tagged
/// `{INSPECTOR_TREE_TAG}#{path}` is selected.
fn use_selected_path() -> Rc<Signal<Option<String>>> {
    Owner::current()
        .expect("hello-multi-window: view fn runs inside the substrate root owner scope")
        .cache("hello_multi_window_selected_path", || {
            Signal::new(None::<String>)
        })
}

/// R678 §5.16 §5.49 — cross-window shared hover slot.
///
/// Parallel to [`use_selected_path`] — same `Owner::cache` mechanism,
/// distinct slot key, so the two signals stay decoupled. The
/// inspector window's hover reducer writes the currently-hovered tree
/// row's path id here on `PointerEnter` and clears it on
/// `PointerLeave`; `view_main` reads the slot through the DevTools
/// highlight-overlay walker to paint a *transient* hover wrap on the
/// matching main-window node (M3 `SurfaceContainerHighest` 2 px
/// Border, distinct from the Error red selection wrap). The hover
/// wrap is **layered under the selection wrap** — selection wins on
/// the same node.
///
/// AI clients observe the slot through `scene/snapshot {window: …}`
/// at both the source side (`hovered_id` query on the
/// `inspector_tree` External) and the projected side (the new
/// SurfaceContainerHighest-bordered ancestor on the hovered
/// main-window node) — the cross-window hover-highlight bridge is
/// the **2nd visible Phase D editor feature** (R676 selection wrap
/// = 1st) and a verifiable RPC introspect signal per
/// [[ai-first-rpc-introspection-obligation]].
///
/// `None` = no row hovered; `Some(path)` = the inspector row tagged
/// `{INSPECTOR_TREE_TAG}#{path}` is currently under the pointer.
fn use_hovered_path() -> Rc<Signal<Option<String>>> {
    Owner::current()
        .expect("hello-multi-window: view fn runs inside the substrate root owner scope")
        .cache("hello_multi_window_hovered_path", || {
            Signal::new(None::<String>)
        })
}

/// R676 §5.16 §5.49 — unwrapped main-window paint, **the
/// inspector-tree source-of-truth.** The highlight-overlay rewrap in
/// [`view_main`] composes on top of this raw scene; the inspector
/// mirrors the raw scene so its tree row paths remain stable across
/// selection toggles (wrapping changes structure → row paths drift →
/// next click writes a stale path → oscillation; splitting raw vs.
/// wrapped is the canonical DevTools-style separation between
/// **underlying tree** and **overlay paint** that browser inspectors
/// also enforce — Chrome's Elements panel reflects the un-overlaid
/// DOM, the Inspect overlay is a separate compositor layer on top).
///
/// Sharing the buffer between the two callers (view_main wraps the
/// result, view_inspector mirrors the result) requires `Scene` value
/// duplication — and `Scene` cannot be `Clone` because
/// [`pinion_core::scene::ExternalNode`] owns a
/// `Box<dyn External>`. The view_main scene contains no Externals (it
/// is Container/Text only), but pinion's substrate cannot encode that
/// invariant statically. The split here builds the scene **once per
/// caller**: `view_main_raw` is invoked twice per paint cycle (once
/// from `view_main` itself, once from `view_inspector` via the tree
/// mirror). The duplicate construction cost is negligible (a handful
/// of allocations per paint) and matches the read-only-tree invariant
/// of DevTools inspectors.
fn view_main_raw(state: ButtonState) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
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
        _ => "Click me!",
    };
    let label_fg = if matches!(state, ButtonState::Disabled) {
        on_surface_muted
    } else {
        on_surface
    };
    let label_text = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new().with_size_px(LABEL_FONT_PX).with_fg(label_fg),
    ));
    let button = Scene::Container(
        ContainerNode::new(vec![label_text])
            .with_tag(MAIN_BTN_TAG)
            .with_style(BoxStyle::filled(btn_fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(BTN_W, BTN_H)),
            ),
    );

    // R675 §5.16 §5.49 — cross-window selection banner. The
    // `selected_path` Signal is shared with the inspector window
    // through `Owner::cache`; reading `.get()` subscribes this
    // view fn so the next paint observes the new selection
    // automatically when inspector clicks mutate the Signal.
    let selected = use_selected_path().get();
    let mut main_children: Vec<Scene> = Vec::with_capacity(2);
    if let Some(path) = selected.as_deref() {
        // Banner tag MAIN_SELECTED_BANNER_TAG would let RPC
        // clients pin its presence; the demo introspects via the
        // selected_path query slot instead so the visible-only
        // banner stays presentational.
        let banner_label = format!("Selected: {path}");
        main_children.push(Scene::Text(TextNode::styled(
            banner_label,
            Rect::default(),
            TextStyle::new()
                .with_size_px(LABEL_FONT_PX)
                .with_fg(on_surface_muted),
        )));
    }
    main_children.push(button);

    Scene::Container(
        ContainerNode::new(main_children)
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(8),
            ),
    )
}

/// R676 §5.16 §5.49 — main-window paint with DevTools highlight
/// overlay composition. Builds the raw scene via [`view_main_raw`],
/// then walks + wraps the node matching the cross-window
/// [`use_selected_path`] Signal with a 2 px error-coloured stroked
/// border. Two-pass design: build first (the inspector mirrors this
/// raw scene through its own `view_main_raw` call), then overlay.
///
/// The pre-resolution via [`find_main_node_at_path`] is the
/// soft-signal gate — inspector-only paths (`"state"`, `"main"`),
/// stale paths after structural change, and malformed paths all
/// route through the unchanged-scene early-return so the banner
/// continues to paint without breaking the layout. AI clients
/// observe the wrapped scene through `scene/snapshot {window:
/// "main"}` — the stroked Border is a verifiable RPC introspect
/// signal per [[ai-first-rpc-introspection-obligation]].
fn view_main(state: ButtonState) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let selection_color = theme.resolve(ColorRole::Error);
    let hover_color = theme.resolve(ColorRole::SurfaceContainerHighest);
    let raw_scene = view_main_raw(state);
    let selected = use_selected_path().get();
    let hovered = use_hovered_path().get();

    // Pre-resolve both paths against the *raw* scene (before any
    // wrap is applied). The find walker takes a borrowed scene and
    // walks read-only; passing `&raw_scene` to both lookups gives a
    // single fixed source of truth — wrapping later (which inserts
    // an ancestor Container at the wrapped path) does not interfere
    // with the post-resolution descent.
    let selected_resolves = selected
        .as_deref()
        .filter(|p| find_main_node_at_path(&raw_scene, p).is_some());
    let hovered_resolves = hovered
        .as_deref()
        .filter(|p| find_main_node_at_path(&raw_scene, p).is_some())
        // R678 — selection wins on the same node. When both signals
        // point at the same path, the hover wrap is suppressed so the
        // Error red selection wrap reads cleanly (no duplicate nested
        // borders).
        .filter(|p| selected_resolves != Some(*p));

    // Apply the **deeper** wrap first (path with more `/`-separated
    // segments = nearer to the leaves), then the shallower wrap on
    // top. Each `rebuild_with_highlight_at_path` call inserts an
    // anonymous Container ancestor at the matched path; applying
    // the shallower (ancestor) path first would invalidate the
    // deeper (descendant) path's lookup on the second pass. Sort
    // by depth so disjoint cases trivially work either order while
    // nested cases (hover on root + selection on a descendant, or
    // vice versa) preserve both wraps.
    let mut wraps: Vec<(&str, Color)> = Vec::with_capacity(2);
    if let Some(hpath) = hovered_resolves {
        wraps.push((hpath, hover_color));
    }
    if let Some(spath) = selected_resolves {
        wraps.push((spath, selection_color));
    }
    // Stable sort by `depth desc` so equal-depth ties preserve the
    // insertion order (hover first, then selection — the visual
    // layering convention: selection always paints on top when both
    // sit at the same depth).
    wraps.sort_by(|a, b| path_depth(b.0).cmp(&path_depth(a.0)));
    let mut scene = raw_scene;
    for (path, color) in wraps {
        scene = rebuild_with_highlight_at_path(scene, path, color);
    }
    scene
}

/// R678 §5.16 §5.49 — depth of a path string (count of `/`
/// separators). Hoisted out of [`view_main`] so
/// `clippy::items_after_statements` stays clean. The
/// double-highlight stacking order in `view_main` is sorted by
/// this depth descending so the deeper wrap (closer to the leaves)
/// applies first — the shallower wrap's anonymous-ancestor
/// insertion then preserves the deeper wrap's path lookup.
fn path_depth(p: &str) -> usize {
    p.matches('/').count()
}

/// R671 §5.50 / R813 §5.40 — the inspector's visible `TreeItem` tree: a
/// fixed `State:` leaf plus a `main` branch wrapping the main window's
/// live paint-scene projection. Shared by the paint path
/// ([`view_inspector`]) and the per-window AT path
/// ([`MultiWindowView::access_node_for_window`]) so both flatten the
/// identical row sequence (the R811.1 single-traversal invariant across
/// the two code paths).
fn inspector_tree_items(state: ButtonState) -> Vec<TreeItem> {
    let main_scene = view_main_raw(state);
    let main_root_path = scene_root_path_segment(&main_scene);
    vec![
        TreeItem::leaf("state", format!("State: {state:?}")),
        TreeItem::branch(
            "main",
            "main window scene",
            true,
            vec![scene_to_tree_item(&main_scene, &main_root_path)],
        ),
    ]
}

/// R671 §5.16 §5.50 — `Inspector` window paint upgraded from a single
/// state-debug `TextNode` to a `TreeView` mirror of the main window's
/// live paint scene. Builds `view_main(state)` internally each paint
/// cycle, walks the resulting `Scene` tree into a [`TreeItem`] model,
/// and renders through [`view_tree`]. Same `ShellCore` underlies both
/// windows so the inspector's tree refreshes whenever the main
/// window's state changes (the next inspector paint observes the
/// updated `cached_state` and rebuilds the tree).
///
/// First consumer of `pinion_widget_paint::tree_view` — proves the
/// substrate against the AI-introspect-first northern-star (`DevTools`
/// / `Inspector` is the canonical Phase B → Phase D bridge).
fn view_inspector(state: ButtonState) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    // R676 §5.16 §5.49 — mirror the **unwrapped** main scene so the
    // inspector's tree row paths are stable across highlight overlay
    // toggle. See `view_main_raw` for the underlying-tree vs. overlay-
    // paint separation rationale.
    let main_scene = view_main_raw(state);
    // R671 §5.50 — surface the live `ButtonState` variant as a
    // dedicated leaf at the top of the tree so AI clients can read
    // it through `scene/snapshot {window: "inspector"}` text-walk
    // without having to parse the deeper button-rect / fill-colour
    // structure. The composite-row tag this produces
    // (`inspector_tree#state`) is the canonical RPC entry point for
    // state introspection; the main scene tree underneath the
    // `main` row carries the same information structurally.
    let tree_items = inspector_tree_items(state);
    // R675 §5.16 §5.50 — paint the focus state-layer on the row
    // matching the cross-window-shared `selected_path` Signal so
    // the user sees which row they just clicked. The substrate's
    // `view_tree_focused` collapses to plain `view_tree` behaviour
    // when `focused_id` is `None`, so this opt-in change preserves
    // the R671 read-only baseline pre-selection.
    let selected = use_selected_path().get();
    let focus = TreeViewFocus {
        focused_id: selected.as_deref(),
    };
    let tree_pane = view_tree_focused(
        INSPECTOR_TREE_TAG,
        &tree_items,
        &theme,
        &TreeViewStyle::m3_default(),
        &focus,
    );

    // R677 §5.16 §5.49 — the right-hand property pane. Resolves the
    // shared `selected_path` Signal against the live `main_scene`
    // through `find_main_node_at_path` (R676 helper, 2nd consumer —
    // tracking the [[abstraction-needs-second-consumer]] Rule-of-
    // Three threshold). Field rows are produced by
    // `property_pane_rows` per R677 atomic (1); atomic (0) gives the
    // pane its structural shape with the no-selection placeholder.
    let pane = property_pane(&main_scene, selected.as_deref(), &theme);

    // 2-pane Row layout — tree on the left, property pane on the
    // right. Both panes use `flex_grow = 1.0` so a future inspector
    // window resize splits the panes evenly; the substrate's flexbox
    // engine handles cross-axis stretch. R667 §5.34 `flex_grow`
    // primitive 2nd application consumer (settings-panel = 1st).
    let surface = theme.resolve(ColorRole::Surface);
    Scene::Container(
        ContainerNode::new(vec![tree_pane, pane])
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Stretch),
            ),
    )
}

/// R677 §5.16 §5.49 — property pane container (right side of the
/// inspector's 2-pane layout). Renders one [`TextNode`] per field of
/// the [`use_selected_path`]-resolved scene node. When no path
/// resolves (`use_selected_path()` is `None`, or
/// [`find_main_node_at_path`] returns `None`), shows the
/// [`PROPERTY_PANE_NO_SELECTION_TEXT`] placeholder.
///
/// Atomic (0) lands the structural shape: tagged container,
/// flex_grow = 1.0 (right pane fills available width), Surface fill,
/// placeholder Text body. Atomic (1) replaces the placeholder body
/// with the real `property_pane_rows` field walker.
fn property_pane(main_scene: &Scene, selected: Option<&str>, theme: &Theme) -> Scene {
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let surface = theme.resolve(ColorRole::Surface);

    let resolved = selected.and_then(|path| find_main_node_at_path(main_scene, path));

    let body: Vec<Scene> = match resolved {
        Some(node) => property_pane_rows(node)
            .into_iter()
            .map(|text| {
                Scene::Text(TextNode::styled(
                    text,
                    Rect::default(),
                    TextStyle::new()
                        .with_size_px(PROPERTY_PANE_FONT_PX)
                        .with_fg(on_surface),
                ))
            })
            .collect(),
        None => vec![Scene::Text(TextNode::styled(
            PROPERTY_PANE_NO_SELECTION_TEXT,
            Rect::default(),
            TextStyle::new()
                .with_size_px(PROPERTY_PANE_FONT_PX)
                .with_fg(on_surface_muted),
        ))],
    };

    Scene::Container(
        ContainerNode::new(body)
            .with_tag(PROPERTY_PANE_TAG)
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Start)
                    .with_gap(PROPERTY_PANE_ROW_GAP_PX)
                    .with_flex_grow(1.0),
            ),
    )
}

/// R677 §5.16 §5.49 — field walker for the property pane. Returns
/// one rendered row per scene-node field. Mirrors Chrome DevTools'
/// Computed pane: each row is a `field: value` line.
///
/// Universal rows: `type:` (the Scene variant name from
/// [`scene_type_name`]) is always first. Variant-specific rows
/// follow:
///
/// * **Container** — `tag:`, `style.fill:`, `style.border:`,
///   `layout.size:`, `children:`
/// * **Text** — `content:`, `style.font_size:`, `style.fg:`
/// * **External** — `tag:`
/// * other variants — `type:` only (atomic 1 scope keeps the row
///   set small; Box/Path/Image/Effect/Scroll field shapes are
///   future-axis when a real consumer needs them)
///
/// Format choices follow Chrome DevTools' convention: colour as
/// `rgba(R,G,B,A)`, border as `<width>px @ rgba(...)` or `none`,
/// size as `<W> × <H>` (using SizeValue's display form). Strings
/// stay short so a 240 px-wide pane reads without horizontal scroll.
fn property_pane_rows(scene: &Scene) -> Vec<String> {
    let mut rows = Vec::new();
    rows.push(format!("type: {}", scene_type_name(scene)));

    match scene {
        Scene::Container(c) => {
            rows.push(format!("tag: {}", format_optional_tag(c.tag.as_deref())));
            rows.push(format!("style.fill: {}", format_color(c.style.fill)));
            rows.push(format!(
                "style.border: {}",
                format_border(c.style.border),
            ));
            rows.push(format!("layout.size: {}", format_size(c.layout.size)));
            rows.push(format!("children: {}", c.children.len()));
        }
        Scene::Text(t) => {
            rows.push(format!("content: {:?}", t.content));
            rows.push(format!("style.font_size: {}px", t.style.font_size_px));
            rows.push(format!("style.fg: {}", format_color(t.style.fg_color)));
        }
        Scene::External(e) => {
            rows.push(format!("tag: {}", format_optional_tag(e.tag.as_deref())));
        }
        // Box/Path/Image/Effect/Scroll/unknown — `type:` row only.
        // Future-axis: add per-variant field rows when a real consumer
        // (R678+ hover bridge, R679+ bidirectional select) drives the
        // need.
        _ => {}
    }
    rows
}

/// R677 §5.16 §5.49 — format an `Option<&str>` tag as either the tag
/// itself or `"—"` (em dash) for absent tags. The em dash is the
/// canonical typographic stand-in for "no value" in property listings
/// (Chrome DevTools / Firefox Inspector both use it).
fn format_optional_tag(tag: Option<&str>) -> String {
    match tag {
        Some(t) => t.to_owned(),
        None => "—".to_owned(),
    }
}

/// R677 §5.16 §5.49 — format a [`Color`] as `rgba(R,G,B,A)`. CSS
/// canonical form; AI clients parse it the same way they parse CSS
/// colour literals. Alpha at 255 still emits `,A` for consistency
/// (a future "omit alpha when fully opaque" pass is a UI polish
/// concern, not a substrate concern).
fn format_color(color: Color) -> String {
    format!("rgba({},{},{},{})", color.r, color.g, color.b, color.a)
}

/// R677 §5.16 §5.49 — format a [`BoxStyle::border`] sidecar as
/// `<width>px @ rgba(...)` when present, or `"none"` when absent.
/// Mirrors CSS border shorthand without the line-style word
/// (pinion's `Border` does not carry a line-style enum — it's always
/// solid for the v1 spec scope).
fn format_border(border: Option<pinion_core::style::Border>) -> String {
    match border {
        Some(b) if b.width > 0 => format!("{}px @ {}", b.width, format_color(b.color)),
        _ => "none".to_owned(),
    }
}

/// R677 §5.16 §5.49 — format a [`Size`] (width × height pair) as
/// `<width> × <height>`. Each axis renders via [`format_size_value`]
/// so `Auto`, `Px(n)`, `Percent(n)` all read naturally.
fn format_size(size: pinion_core::style::Size) -> String {
    format!(
        "{} × {}",
        format_size_value(size.width),
        format_size_value(size.height),
    )
}

/// R677 §5.16 §5.49 — format a [`SizeValue`] as a short CSS-mirror
/// string: `Auto` → `"auto"`, `Px(n)` → `"<n>px"`, `Percent(n)` →
/// `"<n>%"`. Mirrors CSS shorthand exactly — readers familiar with
/// CSS read the value at a glance.
fn format_size_value(value: pinion_core::style::SizeValue) -> String {
    use pinion_core::style::SizeValue;
    match value {
        SizeValue::Auto => "auto".to_owned(),
        SizeValue::Px(n) => format!("{n}px"),
        SizeValue::Percent(n) => format!("{n}%"),
        _ => "unknown".to_owned(),
    }
}


/// R683.C §5.16 §5.20 §5.49 — `DevTools` bidirectional select router
/// for the **main** window. Now powered by the lifted substrate
/// [`pinion_widget_paint::devtools::ClickRouter`] (R683.C
/// Rule-of-Three lift; `hello-multi-window` = 1st consumer, R679
/// origin; `hello-dock-panels` = 2nd consumer, R683.C). AI-invoke-only
/// entry point: a `scene/invoke /main_click_router/external/click
/// {Text(path)}` (or `{Null}`) writes the supplied path into the
/// shared [`use_selected_path`] Signal via the
/// [`MAIN_CLICK_ROUTER_INTENT_TAG`] reducer arm. The user-mouse half
/// (clicks on the main button) routes through
/// [`MAIN_BTN_CLICK_INTENT_TAG`].
///
/// Schema slots, wire shape, and intent payload semantics are pinned
/// at the substrate layer (see [`ClickRouter`]'s rustdoc); the
/// binding only carries the tag constant + reducer arm matching the
/// dotted intent form `main_click_router.click`.
///
/// Binding carrier. `MultiWindowView` carries no fields — every
/// trait method is associated so AppShell instantiates the binding
/// without holding a value.
struct MultiWindowView;

impl WidgetCore for MultiWindowView {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn tag() -> &'static str {
        // The single ShellCore + multi-window approach uses one
        // primary tag for input routing; the inspector window has
        // no interactive widgets so this tag belongs to the main
        // window's Button.
        MAIN_BTN_TAG
    }

    fn title() -> &'static str {
        "pinion hello-multi-window (R670.B §5.16)"
    }

    fn create_external() -> Box<dyn pinion_core::external::External> {
        Box::new(ButtonExternal::new())
    }

    /// R675 §5.45 — register a substrate
    /// [`TreeRowClickExternal`] sibling under
    /// [`INSPECTOR_TREE_TAG`]. Inspector tree rows tagged
    /// `inspector_tree#{path}` route their composite-tag clicks
    /// through this External, which emits the dotted
    /// [`INSPECTOR_CLICK_INTENT_TAG`] = `"inspector_tree.click"`
    /// the [`WidgetCore::update`] reducer matches below.
    ///
    /// This is the **2nd consumer** of
    /// [`pinion_widget_paint::tree_view::TreeRowClickExternal`]
    /// after `examples/hello-tree-view` (1st), firing the
    /// [[abstraction-needs-second-consumer]] Rule-of-Three gate
    /// that drove the R674 → R675 binding-to-substrate lift.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(
                INSPECTOR_TREE_TAG,
                Box::new(TreeRowClickExternal::new()),
            ),
            // R679 §5.16 §5.49 — second `ExtraExternal` registering
            // the `DevTools` bidirectional-select router for the
            // main window. AI clients write a path via
            // `scene/invoke /main_click_router/external/click`;
            // the reducer arm below mirrors it into the cross-
            // window [`use_selected_path`] Signal so the inspector
            // tree row paints focus state-layer + the main window
            // banner appears in lockstep.
            //
            // The `ButtonExternal` (primary) + `TreeRowClickExternal`
            // + this router are the 3 External slots the state scene
            // now carries (R55.D.5 multi-External composition wraps
            // them into a Container holding the primary plus
            // extras).
            ExtraExternal::new(
                MAIN_CLICK_ROUTER_TAG,
                Box::new(ClickRouter::new()),
            ),
        ]
    }

    fn read_state(scene: &Scene) -> Self::State {
        // ButtonState reads from the SCXML state slot via the
        // standard `query("state")` introspect — same as
        // hello-button. Default Idle when introspect is missing
        // (single-widget binding, no composite state to merge).
        //
        // R675 §5.45 — `create_extra_externals` is now non-empty
        // (inspector TreeRowClickExternal), so the state scene
        // root is `Scene::Container([primary, extra])` per R55.D.5.
        // Locate the primary `ButtonExternal` by its tag rather
        // than pattern-matching `Scene::External` at the root.
        if let Some(node) = scene.find_external_with_tag(MAIN_BTN_TAG)
            && let Some(intro) = node.handle.introspect()
            && let Some(IntrospectValue::Text(name)) = intro.query("state")
        {
            return <Self::State as pinion_core::WidgetStateName>::from_name_or_default(
                &name,
            );
        }
        ButtonState::Idle
    }

    /// R675 §5.23 R27 — bridge the inspector
    /// [`TreeRowClickExternal`]'s `click` intent into the shared
    /// [`use_selected_path`] signal.
    ///
    /// R678 §5.23 R27 extension — also bridge the same External's
    /// `hover` intent into the parallel [`use_hovered_path`] signal.
    /// `Text(path)` payload (PointerEnter) writes `Some(path)`;
    /// `Null` payload (PointerLeave) clears the slot to `None`.
    ///
    /// Side-effect-only — empty `Vec<Command>` return; the
    /// `Signal::set` writes are the mutation. Both windows' view fns
    /// observe each Signal change on their next paint cycle (per the
    /// substrate's reactive any-animation-active redraw wire).
    fn update(
        _state: Self::State,
        intent: &Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            tag if tag == INSPECTOR_CLICK_INTENT_TAG => {
                if let IntrospectValue::Text(path) = &intent.payload {
                    use_selected_path().set(Some(path.clone()));
                }
            }
            tag if tag == INSPECTOR_HOVER_INTENT_TAG => {
                match &intent.payload {
                    IntrospectValue::Text(path) => {
                        use_hovered_path().set(Some(path.clone()));
                    }
                    IntrospectValue::Null => {
                        use_hovered_path().set(None);
                    }
                    // Unknown payload variant — silent no-op rather
                    // than panic; defensive against a future substrate
                    // axis (richer hover payload, mouse-button mask,
                    // pointer-id discriminator) being introduced
                    // without lockstep binding update.
                    _ => {}
                }
            }
            tag if tag == MAIN_CLICK_ROUTER_INTENT_TAG => {
                // R679 §5.16 §5.49 — `DevTools` bidirectional select
                // arc from the main window. AI invokes the typed
                // `click` shortcut on the router External; this arm
                // mirrors the supplied payload into the cross-window
                // [`use_selected_path`] Signal. `Text(path)` → select
                // the matching node; `Null` → deselect.
                match &intent.payload {
                    IntrospectValue::Text(path) => {
                        use_selected_path().set(Some(path.clone()));
                    }
                    IntrospectValue::Null => {
                        use_selected_path().set(None);
                    }
                    _ => {}
                }
            }
            tag if tag == MAIN_BTN_CLICK_INTENT_TAG => {
                // R679 §5.16 §5.49 — user-mouse-driven bidirectional
                // select. The InputRouter routes a user mouse click
                // on the main button to ButtonExternal; its
                // `Pressed → Hover` SCXML transition emits a
                // `Null`-payload `click` intent (the canonical button
                // intent per
                // [`pinion_core::widgets::button::Button`]). This
                // arm mirrors the button's static raw path into
                // [`use_selected_path`] so the inspector tree row
                // paints focus state-layer in lockstep — closing
                // the user-mouse half of the bidirectional select
                // arc without needing shell-level hit-test hooks.
                //
                // Why a static path: the button's intent payload is
                // `Null` (Button carries no semantic value), so the
                // reducer cannot derive the path from the intent
                // itself. A future-axis substrate that propagates
                // cursor coordinates into the button intent payload
                // would let the reducer call
                // `pinion_widget_paint::devtools::path_for_paint_hit`
                // dynamically; for R679 the static path suffices
                // because the binding's raw scene shape is stable
                // (R678 substrate test
                // `r679_path_for_paint_hit_on_tagged_container_returns_container_path`
                // pins this exact path shape).
                use_selected_path().set(Some(MAIN_BUTTON_RAW_PATH.to_owned()));
            }
            _ => {}
        }
        Vec::new()
    }

    fn event_name(event: Self::Event) -> &'static str {
        <Self::Event as pinion_core::WidgetEventName>::as_name(&event)
    }

    fn view(state: Self::State, frame: &Frame) -> Scene {
        // Default `WidgetCore::view` body — single-window fallback
        // when no window_id context is available. AppShell always
        // calls `view_for_window` for the live render loop; this
        // path remains for tests + legacy single-window dispatch
        // (RPC `scene/snapshot` without `{window}` defaults here
        // via the default `view_for_window` impl which forwards
        // back).
        let _ = frame;
        view_main(state)
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }

    fn keybinding(key: &str) -> Option<Self::Event> {
        match key {
            "d" => Some(ButtonEvent::Disable),
            "e" => Some(ButtonEvent::Enable),
            _ => None,
        }
    }
}

impl pinion_a11y::WidgetA11y for MultiWindowView {
    // Default empty AT impl — the Button widget surface auto-emits
    // the AriaRole::Button + state flags via the standard pinion-
    // a11y derive when a binding declares role=Button. This binding
    // is minimal (one Button) so the default impl is fine.
}

impl WidgetView for MultiWindowView {
    type Renderer = HelloMultiWindowRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        // Primary spec's fallback — only consulted when the binding
        // uses the default `windows()` impl. We override `windows()`
        // below so each spec declares its own strategy; this method
        // returns the main window's strategy for backward-compat.
        SizeStrategy::Fixed {
            width: MAIN_W,
            height: MAIN_H,
        }
    }

    /// R670.A §5.16 — the canonical multi-window declaration.
    /// Main + inspector windows; main is the primary (first in
    /// list = RPC scope default). Both `Fixed` strategies; multi-
    /// window `IntrinsicAfterFirstPaint` would also work, but
    /// hello-multi-window keeps the first dogfood minimal so
    /// dimensions are easier to demo-verify.
    fn windows() -> Vec<WindowSpec> {
        vec![
            WindowSpec::new(
                "main",
                "hello-multi-window — Main",
                SizeStrategy::Fixed {
                    width: MAIN_W,
                    height: MAIN_H,
                },
            ),
            WindowSpec::new(
                "inspector",
                "hello-multi-window — Inspector",
                SizeStrategy::Fixed {
                    width: INSPECTOR_W,
                    height: INSPECTOR_H,
                },
            ),
        ]
    }

    /// R670.B §5.16 — per-window paint dispatch. Each window's id
    /// resolves to a different view-fn branch so main and inspector
    /// paint different content from the same shared `cached_state`.
    /// The fall-through to `view_main` for unknown ids is defensive:
    /// AppShell only ever calls this with ids declared in
    /// [`Self::windows`], but the fallback avoids a panic if a
    /// future round adds a window without updating this match.
    fn view_for_window(window_id: &str, state: Self::State, frame: &Frame) -> Scene {
        let _ = frame;
        match window_id {
            // R670.B §5.16 — the inspector spec is the only one
            // that diverges from view_main; every other (including
            // the canonical primary "main") falls through to the
            // main view fn.
            "inspector" => view_inspector(state),
            _ => view_main(state),
        }
    }

    /// R813 §5.40 §5.16 — per-window AT nodes: the inspector window gets
    /// the WAI-ARIA `tree` + `treeitem` semantic tree (the 3rd consumer
    /// of the lifted [`tree_access_nodes`](pinion_a11y::tree_access_nodes)
    /// builder, unblocked by R813's per-window `access_node`). The main
    /// window contributes none (its single Button keeps the pre-existing
    /// default-empty AT surface). Before R813 the shell's global
    /// `access_node` would have stamped the inspector's tree rows onto the
    /// main window's AT tree as ghosts — the gap this round closes. The
    /// click-selected row ([`use_selected_path`]) carries `aria-selected`.
    fn access_node_for_window(
        window_id: &str,
        state: &Self::State,
        _focused: Option<&str>,
    ) -> Vec<AccessNode> {
        if window_id != "inspector" {
            return Vec::new();
        }
        let items = inspector_tree_items(*state);
        let rows = flat_visible(&items);
        let selected = use_selected_path().get();
        tree_access_nodes(
            INSPECTOR_TREE_TAG,
            INSPECTOR_TREE_TAG,
            Some(INSPECTOR_TREE_NAME),
            &rows,
            selected.as_deref(),
        )
    }
}

fn main() {
    pinion_shell::run::<MultiWindowView>();
}

#[cfg(test)]
mod r670_b_multi_window_tests {
    use super::*;

    /// (R670.B §5.16) `WidgetView::windows()` declares main +
    /// inspector specs in the expected order. The primary spec is
    /// always at index 0; RPC `{window: "..."}`-less frames default
    /// to it.
    #[test]
    fn r670_b_windows_list_has_main_and_inspector() {
        let specs = <MultiWindowView as WidgetView>::windows();
        assert_eq!(specs.len(), 2, "main + inspector");
        assert_eq!(specs[0].id, "main");
        assert_eq!(specs[1].id, "inspector");
    }

    /// (R813 §5.40 §5.16) Per-window AT nodes: the inspector window emits
    /// the WAI-ARIA tree subtree (3rd consumer of `tree_access_nodes`);
    /// the main window emits none, so its AT tree never carries the
    /// inspector's tree rows as ghosts (the cross-window leak R813 closes).
    #[test]
    fn r813_only_inspector_window_carries_tree_at_nodes() {
        use pinion_a11y::AriaRole;
        Owner::new().run(|| {
            let inspector = <MultiWindowView as WidgetView>::access_node_for_window(
                "inspector",
                &ButtonState::Idle,
                None,
            );
            assert!(!inspector.is_empty(), "inspector window emits the tree AT subtree");
            assert_eq!(inspector[0].role, AriaRole::Tree, "root node is role=tree");
            assert_eq!(inspector[0].tag, INSPECTOR_TREE_TAG);
            assert!(
                inspector[1..].iter().all(|n| n.role == AriaRole::TreeItem),
                "every non-root node is a treeitem",
            );
            assert!(
                inspector.iter().any(|n| n.tag == format!("{INSPECTOR_TREE_TAG}#state")),
                "the fixed State leaf is present as a row",
            );

            let main = <MultiWindowView as WidgetView>::access_node_for_window(
                "main",
                &ButtonState::Idle,
                None,
            );
            assert!(main.is_empty(), "main window AT tree carries no inspector ghost nodes");
        });
    }

    /// (R677 §5.16 §5.49) Inspector window resized to 480×320 to
    /// host the new 2-pane DevTools layout (tree + property pane).
    /// Pin the dimensions so a future tweak surfaces immediately —
    /// the demo's section (A) verifies the same shape through
    /// `windows()` RPC introspection.
    #[test]
    fn r677_inspector_window_dimensions_480x320() {
        let specs = <MultiWindowView as WidgetView>::windows();
        let inspector_spec = specs
            .iter()
            .find(|s| s.id == "inspector")
            .expect("inspector spec must exist");
        match inspector_spec.strategy {
            SizeStrategy::Fixed { width, height } => {
                assert_eq!(width, 480, "R677 widened inspector to 480 px");
                assert_eq!(height, 320, "R677 tallened inspector to 320 px");
            }
            other => panic!("inspector must use Fixed strategy; got: {other:?}"),
        }
    }

    /// (R670.B §5.16) `view_for_window` switches by window_id; main
    /// returns a scene containing the Button tag, inspector returns
    /// a scene containing the state-debug text tag.
    #[test]
    fn r670_b_view_for_window_main_contains_button_tag() {
        let owner = pinion_core::Owner::new();
        let scene =
            owner.run(|| MultiWindowView::view_for_window("main", ButtonState::Idle, &Frame::new()));
        assert!(
            scene.contains_tag(MAIN_BTN_TAG),
            "main view must carry the {MAIN_BTN_TAG:?} tag for input routing",
        );
    }

    #[test]
    fn r671_view_for_window_inspector_carries_tree_root_tag() {
        // R671 §5.16 — inspector window upgraded from the single
        // [`Container`] tagged `inspector_state_text` to a
        // [`pinion_widget_paint::tree_view::view_tree`] root tagged
        // [`INSPECTOR_TREE_TAG`]. Pin the new tag so a regression
        // that drops the TreeView wiring surfaces immediately.
        let owner = pinion_core::Owner::new();
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("inspector", ButtonState::Hover, &Frame::new())
        });
        assert!(
            scene.contains_tag(INSPECTOR_TREE_TAG),
            "inspector view must carry the {INSPECTOR_TREE_TAG:?} tag for RPC scene/snapshot",
        );
    }

    /// (R670.B §5.16) Inspector view text reflects the cached state.
    /// Test pin so a regression that drops `format!("{:?}", state)`
    /// surfaces immediately.
    /// Helper for `r670_b_inspector_view_reflects_state_value` —
    /// depth-first walk for any text node whose content matches.
    /// Hoisted out of the test fn so `clippy::items_after_statements`
    /// stays clean.
    fn walk_for_text(scene: &Scene, needle: &str) -> bool {
        match scene {
            Scene::Text(t) => t.content.contains(needle),
            Scene::Container(c) => c.children.iter().any(|ch| walk_for_text(ch, needle)),
            _ => false,
        }
    }

    #[test]
    fn r670_b_inspector_view_reflects_state_value() {
        let owner = pinion_core::Owner::new();
        let scene_idle = owner
            .run(|| MultiWindowView::view_for_window("inspector", ButtonState::Idle, &Frame::new()));
        let scene_hover = owner.run(|| {
            MultiWindowView::view_for_window("inspector", ButtonState::Hover, &Frame::new())
        });
        assert!(
            walk_for_text(&scene_idle, "Idle"),
            "inspector view must render state name",
        );
        assert!(
            walk_for_text(&scene_hover, "Hover"),
            "inspector view must render state name",
        );
    }

    /// (R670.B §5.16) `view_for_window` with an unknown id falls
    /// back to `view_main` — defensive against a future round
    /// adding a window spec without updating the match.
    #[test]
    fn r670_b_view_for_window_unknown_id_falls_back_to_main() {
        let owner = pinion_core::Owner::new();
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("ghost", ButtonState::Idle, &Frame::new())
        });
        assert!(
            scene.contains_tag(MAIN_BTN_TAG),
            "unknown id falls back to main view",
        );
    }

    /// (R670.B §5.16) Sanity: the `access_node` default impl returns
    /// no nodes (main button's a11y is delegated to the standard
    /// pinion-core Button widget surface, not surfaced through this
    /// binding's WidgetA11y impl).
    #[test]
    fn r670_b_default_access_node_is_empty() {
        let nodes = <MultiWindowView as WidgetA11y>::access_node(&ButtonState::Idle, None);
        assert!(nodes.is_empty(), "default WidgetA11y impl returns no nodes");
    }

    // R675 §5.16 §5.20 §5.45 §5.49 — cross-window selection bridge
    // regression suite. Pins the substrate plumbing (selected_path
    // shared Signal, click intent dotted form lockstep, banner
    // appears in main view only when path is Some) so a future
    // regression that drops one wire surfaces at unit-test time.

    #[test]
    fn r679_create_extra_externals_registers_tree_row_click_and_main_click_router() {
        // R679 §5.16 §5.49 — the binding now declares 2 extra
        // Externals: the R675 TreeRowClickExternal at
        // `inspector_tree` + the R679 MainWindowClickRouter at
        // `main_click_router`. Pin both the length AND the
        // declaration order (the router is the 2nd entry so a
        // future round adding a 3rd extra surfaces if it gets
        // wedged in front of the existing pair).
        let extras = <MultiWindowView as WidgetCore>::create_extra_externals();
        assert_eq!(extras.len(), 2, "tree-row click router + main click router");
        assert_eq!(extras[0].tag, INSPECTOR_TREE_TAG);
        assert_eq!(extras[1].tag, MAIN_CLICK_ROUTER_TAG);
    }

    #[test]
    fn r675_inspector_click_intent_tag_matches_runtime_dotted_form() {
        // [[intent-tag-dotted-wire-form]] — the compile-time
        // INSPECTOR_CLICK_INTENT_TAG literal must match the runtime
        // walker's `format!("{prefix}.{event}", ...)` shape exactly
        // so the V::update reducer arm matches. The substrate's
        // bare event name ("click") is the canonical
        // pinion_widget_paint::tree_view::TREE_ROW_CLICK_EVENT.
        assert_eq!(
            INSPECTOR_CLICK_INTENT_TAG,
            format!(
                "{INSPECTOR_TREE_TAG}.{}",
                pinion_widget_paint::tree_view::TREE_ROW_CLICK_EVENT
            ),
        );
    }

    /// Recursive helper: pull the text content of the first
    /// `Scene::Text` node whose content contains `needle`. Used by
    /// the banner-presence tests below.
    fn first_text_containing<'s>(scene: &'s Scene, needle: &str) -> Option<&'s str> {
        match scene {
            Scene::Text(t) => {
                if t.content.contains(needle) {
                    Some(t.content.as_str())
                } else {
                    None
                }
            }
            Scene::Container(c) => c
                .children
                .iter()
                .find_map(|ch| first_text_containing(ch, needle)),
            _ => None,
        }
    }

    #[test]
    fn r675_view_main_without_selection_has_no_selected_banner() {
        // Fresh Owner → use_selected_path() returns Signal(None) →
        // view_main must not render a "Selected: …" banner.
        let owner = Owner::new();
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("main", ButtonState::Idle, &Frame::new())
        });
        assert!(
            first_text_containing(&scene, "Selected:").is_none(),
            "fresh boot must not render the cross-window selection banner",
        );
    }

    #[test]
    fn r675_view_main_with_selection_renders_banner_text() {
        // Seed the cross-window selection signal, then verify
        // view_main projects it into the banner.
        let owner = Owner::new();
        owner.run(|| {
            use_selected_path().set(Some("state".to_string()));
        });
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("main", ButtonState::Idle, &Frame::new())
        });
        assert_eq!(
            first_text_containing(&scene, "Selected:"),
            Some("Selected: state"),
            "main view must render the banner reflecting the shared signal",
        );
    }

    #[test]
    fn r675_view_main_still_carries_button_tag_when_banner_present() {
        // Sanity: banner addition must not displace the button —
        // the input router still needs to hit-test MAIN_BTN_TAG.
        let owner = Owner::new();
        owner.run(|| {
            use_selected_path().set(Some("main/0".to_string()));
        });
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("main", ButtonState::Idle, &Frame::new())
        });
        assert!(
            scene.contains_tag(MAIN_BTN_TAG),
            "main view must continue to carry {MAIN_BTN_TAG:?} alongside the banner",
        );
    }

    #[test]
    fn r675_update_reducer_routes_inspector_click_to_selected_path_signal() {
        // Synthesise the dotted intent the runtime would deliver
        // after a TreeRowClickExternal commit + walker prefix; the
        // reducer must mirror the payload into the shared signal.
        let owner = Owner::new();
        owner.run(|| {
            assert!(
                use_selected_path().get().is_none(),
                "baseline: selection slot empty",
            );
            let intent = Intent::new_static(
                INSPECTOR_CLICK_INTENT_TAG,
                IntrospectValue::Text("main/0/3".to_string()),
            );
            let commands = <MultiWindowView as WidgetCore>::update(
                ButtonState::Idle,
                &intent,
            );
            assert!(
                commands.is_empty(),
                "side-effect-only reducer returns no commands",
            );
            assert_eq!(
                use_selected_path().get().as_deref(),
                Some("main/0/3"),
                "reducer must mirror intent payload into the shared selection signal",
            );
        });
    }

    #[test]
    fn r675_update_reducer_ignores_truly_unrelated_intent_tags() {
        // R679 grew the reducer to handle `main_btn.click` (user-
        // mouse-driven bidirectional select). Pick a tag the
        // reducer still ignores — `disable.click` is a synthetic
        // shape that no External in this binding emits, so it
        // proves the reducer's `_ => {}` fallthrough still works.
        let owner = Owner::new();
        owner.run(|| {
            let foreign = Intent::new_static(
                "disable.click",
                IntrospectValue::Null,
            );
            let _ = <MultiWindowView as WidgetCore>::update(ButtonState::Idle, &foreign);
            assert!(
                use_selected_path().get().is_none(),
                "truly unrelated intent tags must not mutate the selection slot",
            );
        });
    }

    // R679 §5.16 §5.20 §5.49 — bidirectional select reducer
    // regression suite. Pins the new `main_click_router.click`
    // (AI-invoke-driven) + `main_btn.click` (user-mouse-driven)
    // reducer arms so a future regression that drops one wire
    // surfaces at unit-test time. The R675 inspector→main arc is
    // covered above; these tests cover the main→inspector arc
    // (the R679 closure of the bidirectional bridge).

    #[test]
    fn r679_main_click_router_intent_tag_matches_runtime_dotted_form() {
        // [[intent-tag-dotted-wire-form]] — the compile-time
        // MAIN_CLICK_ROUTER_INTENT_TAG literal must match the
        // runtime walker's `format!("{prefix}.{event}", ...)`
        // shape exactly so the V::update reducer arm matches.
        // The bare event name is hard-coded `"click"` to mirror
        // the substrate's
        // pinion_widget_paint::tree_view::TREE_ROW_CLICK_EVENT
        // convention (every R27 router emits `"click"` as its
        // bare canonical event name).
        assert_eq!(
            MAIN_CLICK_ROUTER_INTENT_TAG,
            format!("{MAIN_CLICK_ROUTER_TAG}.click"),
        );
    }

    #[test]
    fn r679_main_btn_click_intent_tag_matches_button_external_dotted_form() {
        // ButtonExternal's intent stream emits `"click"` on the
        // Pressed→Hover SCXML transition (canonical per
        // `pinion_core::widgets::button::Button`'s
        // WidgetTransition::detect). Runtime prefixes with the
        // External's tag → `main_btn.click`.
        assert_eq!(
            MAIN_BTN_CLICK_INTENT_TAG,
            format!("{MAIN_BTN_TAG}.click"),
        );
    }

    #[test]
    fn r679_update_reducer_routes_main_router_text_payload_to_selected_path_signal() {
        // AI-invoke-driven select: a Text payload from the main
        // click router writes the supplied path into the shared
        // selection Signal — the main→inspector half of the
        // bidirectional bridge.
        let owner = Owner::new();
        owner.run(|| {
            assert!(use_selected_path().get().is_none(), "baseline empty");
            let intent = Intent::new_static(
                MAIN_CLICK_ROUTER_INTENT_TAG,
                IntrospectValue::Text("Container/Container[main_btn]".to_string()),
            );
            let commands = <MultiWindowView as WidgetCore>::update(
                ButtonState::Idle,
                &intent,
            );
            assert!(commands.is_empty(), "side-effect-only reducer");
            assert_eq!(
                use_selected_path().get().as_deref(),
                Some("Container/Container[main_btn]"),
                "main router Text payload must mirror into selected_path",
            );
        });
    }

    #[test]
    fn r679_update_reducer_routes_main_router_null_payload_to_deselect() {
        // AI-invoke-driven deselect: a Null payload from the main
        // click router clears the selection Signal.
        let owner = Owner::new();
        owner.run(|| {
            use_selected_path().set(Some("Container/Container[main_btn]".to_string()));
            let intent = Intent::new_static(
                MAIN_CLICK_ROUTER_INTENT_TAG,
                IntrospectValue::Null,
            );
            let _ = <MultiWindowView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert!(
                use_selected_path().get().is_none(),
                "main router Null payload must clear selected_path",
            );
        });
    }

    #[test]
    fn r679_update_reducer_routes_button_click_intent_to_button_raw_path() {
        // User-mouse-driven bidirectional select: a `main_btn.click`
        // intent (emitted by ButtonExternal's SCXML Pressed→Hover
        // transition on a real user click) writes the button's
        // static raw path into the Signal. The intent payload is
        // Null per Button's WidgetTransition::detect contract;
        // the reducer derives the path from the static
        // MAIN_BUTTON_RAW_PATH constant.
        let owner = Owner::new();
        owner.run(|| {
            assert!(use_selected_path().get().is_none(), "baseline empty");
            let intent = Intent::new_static(
                MAIN_BTN_CLICK_INTENT_TAG,
                IntrospectValue::Null,
            );
            let _ = <MultiWindowView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert_eq!(
                use_selected_path().get().as_deref(),
                Some(MAIN_BUTTON_RAW_PATH),
                "button click must mirror MAIN_BUTTON_RAW_PATH into selected_path",
            );
        });
    }

    #[test]
    fn r679_button_raw_path_matches_view_main_raw_button_position() {
        // The hard-coded MAIN_BUTTON_RAW_PATH must resolve against
        // the raw scene (the inspector mirrors). Verifies the
        // substrate's `find_node_at_path` round-trips the path
        // against an actual view_main_raw output.
        use pinion_widget_paint::devtools::find_node_at_path;
        let owner = Owner::new();
        let raw_scene = owner.run(|| {
            // Use view_main directly because view_main_raw is
            // private; view_main returns the wrapped scene only
            // when a selection is present. With no selection, the
            // raw and wrapped scenes are identical so view_main
            // works as the test surface.
            MultiWindowView::view_for_window("main", ButtonState::Idle, &Frame::new())
        });
        assert!(
            find_node_at_path(&raw_scene, MAIN_BUTTON_RAW_PATH).is_some(),
            "static MAIN_BUTTON_RAW_PATH must resolve in the raw scene",
        );
    }

    #[test]
    fn r679_bidirectional_alternation_preserves_invariant() {
        // Stress test for the bidirectional bridge: alternating
        // inspector→main + main→inspector mutations all settle
        // the Signal to the latest written value (latest-write-
        // wins, no oscillation).
        let owner = Owner::new();
        owner.run(|| {
            // Inspector click writes path X.
            let i1 = Intent::new_static(
                INSPECTOR_CLICK_INTENT_TAG,
                IntrospectValue::Text("X".to_string()),
            );
            let _ = <MultiWindowView as WidgetCore>::update(ButtonState::Idle, &i1);
            assert_eq!(use_selected_path().get().as_deref(), Some("X"));

            // Main router writes path Y.
            let i2 = Intent::new_static(
                MAIN_CLICK_ROUTER_INTENT_TAG,
                IntrospectValue::Text("Y".to_string()),
            );
            let _ = <MultiWindowView as WidgetCore>::update(ButtonState::Idle, &i2);
            assert_eq!(use_selected_path().get().as_deref(), Some("Y"));

            // Inspector click writes path Z.
            let i3 = Intent::new_static(
                INSPECTOR_CLICK_INTENT_TAG,
                IntrospectValue::Text("Z".to_string()),
            );
            let _ = <MultiWindowView as WidgetCore>::update(ButtonState::Idle, &i3);
            assert_eq!(use_selected_path().get().as_deref(), Some("Z"));

            // Main router deselect (Null).
            let i4 = Intent::new_static(
                MAIN_CLICK_ROUTER_INTENT_TAG,
                IntrospectValue::Null,
            );
            let _ = <MultiWindowView as WidgetCore>::update(ButtonState::Idle, &i4);
            assert!(use_selected_path().get().is_none(), "deselect wins last");
        });
    }

    // R679 §5.16 §5.50 — main→inspector cross-arc lockstep
    // verification. The view_inspector consumer of TreeViewFocus
    // landed at R675 (atomic 2). R679 atomic 2 verifies that
    // *writing* selected_path via the *main* arc (router invoke
    // OR button click) ends up reflecting in the inspector's
    // focus state-layer paint at the matching tree row tag —
    // closing the bidirectional verification at the view level.

    /// Walk `scene` depth-first for a `Scene::Container` whose
    /// tag equals `target`. Returns the matched container's fill
    /// colour so the test can assert focus state-layer presence.
    fn find_container_fill_by_tag(
        scene: &Scene,
        target: &str,
    ) -> Option<pinion_core::Color> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(target) {
                    return Some(c.style.fill);
                }
                c.children
                    .iter()
                    .find_map(|child| find_container_fill_by_tag(child, target))
            }
            _ => None,
        }
    }

    #[test]
    fn r679_inspector_paints_focus_state_layer_after_main_router_invoke() {
        // Bidirectional bridge end-to-end: write the button's raw
        // path via the main router's reducer arm (mirrors what
        // `scene/invoke /main_click_router/external/click
        // {Text(path)}` would produce in production). Render
        // view_inspector. The tree row tagged
        // `inspector_tree#<path>` must paint with a non-transparent
        // fill (the M3 SurfaceContainerHighest focus state-layer
        // per pinion_widget_paint::tree_view::view_tree_focused).
        let owner = Owner::new();
        owner.run(|| {
            // Step 1: trigger the main router reducer (simulating
            // an AI invoke).
            let intent = Intent::new_static(
                MAIN_CLICK_ROUTER_INTENT_TAG,
                IntrospectValue::Text(MAIN_BUTTON_RAW_PATH.to_string()),
            );
            let _ = <MultiWindowView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert_eq!(
                use_selected_path().get().as_deref(),
                Some(MAIN_BUTTON_RAW_PATH),
                "precondition: selected_path written via main arc",
            );
        });
        // Step 2: render inspector view. The Signal mutation from
        // step 1 is observed via use_selected_path().get() inside
        // view_inspector.
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("inspector", ButtonState::Idle, &Frame::new())
        });
        // Step 3: walk to the matching row. The composite tag
        // mirrors `pinion_widget_paint::tree_view::composite_row_tag`:
        // `{INSPECTOR_TREE_TAG}#{path}`.
        let row_tag = format!("{INSPECTOR_TREE_TAG}#{MAIN_BUTTON_RAW_PATH}");
        let fill = find_container_fill_by_tag(&scene, &row_tag).unwrap_or_else(|| {
            panic!(
                "inspector view must carry a row Container tagged {row_tag:?}",
            )
        });
        // SurfaceContainerHighest is non-transparent in both
        // light and dark M3 palettes; comparing against
        // Color::TRANSPARENT (the non-focused row default) is the
        // cleanest assertion that the focus state-layer fired.
        assert_ne!(
            fill,
            pinion_core::Color::TRANSPARENT,
            "main router invoke must paint focus state-layer on \
             matching inspector row (M3 SurfaceContainerHighest)",
        );
    }

    #[test]
    fn r679_inspector_paints_focus_state_layer_after_button_click_intent() {
        // User-mouse-driven half of the bidirectional bridge:
        // simulate a button.click intent (what ButtonExternal
        // emits on a real mouse click). Inspector tree row at
        // MAIN_BUTTON_RAW_PATH paints the focus state-layer.
        let owner = Owner::new();
        owner.run(|| {
            let intent =
                Intent::new_static(MAIN_BTN_CLICK_INTENT_TAG, IntrospectValue::Null);
            let _ = <MultiWindowView as WidgetCore>::update(ButtonState::Idle, &intent);
            assert_eq!(
                use_selected_path().get().as_deref(),
                Some(MAIN_BUTTON_RAW_PATH),
                "precondition: button click wrote MAIN_BUTTON_RAW_PATH",
            );
        });
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("inspector", ButtonState::Idle, &Frame::new())
        });
        let row_tag = format!("{INSPECTOR_TREE_TAG}#{MAIN_BUTTON_RAW_PATH}");
        let fill = find_container_fill_by_tag(&scene, &row_tag).unwrap_or_else(|| {
            panic!("inspector view must carry row tagged {row_tag:?}")
        });
        assert_ne!(
            fill,
            pinion_core::Color::TRANSPARENT,
            "button click must paint focus state-layer on \
             inspector's matching row",
        );
    }

    #[test]
    fn r679_inspector_no_focus_state_layer_after_main_router_null_deselect() {
        // Round-trip the bridge: select then deselect. After the
        // Null-payload main router invoke, the inspector tree row
        // paints transparent (focus state-layer cleared).
        let owner = Owner::new();
        owner.run(|| {
            // Select first.
            let select = Intent::new_static(
                MAIN_CLICK_ROUTER_INTENT_TAG,
                IntrospectValue::Text(MAIN_BUTTON_RAW_PATH.to_string()),
            );
            let _ = <MultiWindowView as WidgetCore>::update(ButtonState::Idle, &select);
            // Then deselect via Null.
            let deselect = Intent::new_static(
                MAIN_CLICK_ROUTER_INTENT_TAG,
                IntrospectValue::Null,
            );
            let _ = <MultiWindowView as WidgetCore>::update(ButtonState::Idle, &deselect);
            assert!(use_selected_path().get().is_none(), "precondition: deselected");
        });
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("inspector", ButtonState::Idle, &Frame::new())
        });
        let row_tag = format!("{INSPECTOR_TREE_TAG}#{MAIN_BUTTON_RAW_PATH}");
        let fill = find_container_fill_by_tag(&scene, &row_tag).unwrap_or_else(|| {
            panic!("inspector row tagged {row_tag:?} must still exist after deselect")
        });
        assert_eq!(
            fill,
            pinion_core::Color::TRANSPARENT,
            "deselect via main router Null must clear focus state-layer",
        );
    }
}


#[cfg(test)]
mod r676_highlight_overlay_tests {
    //! R676 §5.16 §5.49 — DevTools highlight overlay regression
    //! suite. Atomic (2) wires the path-stable rebuild into
    //! [`view_main`]: when [`use_selected_path`] returns `Some(path)`
    //! and the path resolves against the current scene shape, the
    //! matching node is wrapped in a 2 px error-coloured stroked
    //! Container. These tests pin both the happy path (tagged path
    //! → stroked border somewhere in the painted scene) and the
    //! soft-fail paths (None / inspector-only / banner-on path-
    //! stable proof).
    use super::*;

    /// (R676) Walk a scene and return true if **any** node carries a
    /// `BoxStyle` with a stroked Border. The overlay wrapper is the
    /// only producer in `view_main` (the button uses `BoxStyle::filled`
    /// and the root uses `BoxStyle::filled(surface)`); a present
    /// Border ⇒ the overlay is wired.
    fn scene_has_stroked_border(scene: &Scene) -> bool {
        match scene {
            Scene::Container(c) => {
                if c.style.border.is_some_and(|b| b.width > 0) {
                    return true;
                }
                c.children.iter().any(scene_has_stroked_border)
            }
            _ => false,
        }
    }

    /// (R676) Find the deepest container path-id (in tagged form)
    /// whose own paint Container has a stroked Border. Helper for the
    /// "highlight on button → border on tagged container" pin.
    fn find_stroked_container_tag(scene: &Scene) -> Option<String> {
        match scene {
            Scene::Container(c) => {
                if c.style.border.is_some_and(|b| b.width > 0) {
                    return c.tag.as_deref().map(str::to_owned).or_else(|| Some(String::new()));
                }
                for child in &c.children {
                    if let Some(found) = find_stroked_container_tag(child) {
                        return Some(found);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// (R676) Walk a scene and return the topmost
    /// `Container[main_btn]` regardless of wrapping. Used to assert
    /// the button paint survives unchanged underneath the wrapper.
    fn find_main_btn_container(scene: &Scene) -> bool {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(MAIN_BTN_TAG) {
                    return true;
                }
                c.children.iter().any(find_main_btn_container)
            }
            _ => false,
        }
    }

    /// (R676) Walk a scene for any Text node containing `needle`.
    /// Mirrors `first_text_containing` from the R675 test module but
    /// returns bool — used to assert banner survival post-wrap.
    fn scene_has_text(scene: &Scene, needle: &str) -> bool {
        match scene {
            Scene::Text(t) => t.content.contains(needle),
            Scene::Container(c) => c.children.iter().any(|ch| scene_has_text(ch, needle)),
            _ => false,
        }
    }

    /// (R676) Baseline — selection None ⇒ no stroked Border anywhere
    /// in view_main. The substrate's `view_main_raw` defaults are
    /// `BoxStyle::filled` (no border), so the only producer of a
    /// stroked border is the highlight overlay wrap.
    #[test]
    fn r676_highlight_overlay_absent_when_no_selection() {
        let owner = Owner::new();
        let scene = owner.run(|| view_main(ButtonState::Idle));
        assert!(
            !scene_has_stroked_border(&scene),
            "no-selection scene must paint without any stroked Border",
        );
    }

    /// (R676) **Tagged path → stroked border at the tagged container
    /// path.** Selection = `Container/Container[main_btn]` → the
    /// button Container gets a stroked Border wrapper. The exact
    /// shape: outer Container (wrapper, no fill, border) > inner
    /// Container (the actual button, `Container[main_btn]`, filled
    /// fill).
    #[test]
    fn r676_highlight_overlay_wraps_tagged_button_path() {
        let owner = Owner::new();
        let button_path = format!("Container/Container[{MAIN_BTN_TAG}]");
        owner.run(|| {
            use_selected_path().set(Some(button_path.clone()));
        });
        let scene = owner.run(|| view_main(ButtonState::Idle));

        assert!(
            scene_has_stroked_border(&scene),
            "tagged-path selection must produce a stroked Border somewhere",
        );
        assert!(
            find_main_btn_container(&scene),
            "the button Container[main_btn] must still exist underneath the wrapper",
        );
    }

    /// (R676) Tagged path matches the **same** path produced by the
    /// forward walker on the raw scene — the round-trip the seed
    /// atomic (3) demo's RPC assertions ride on. Picks the button
    /// path from `view_main_raw`, sets it as selection, repaints
    /// `view_main`, finds the stroked wrapper, and walks to verify
    /// the wrapped subtree is the button.
    #[test]
    fn r676_highlight_overlay_round_trip_forward_walker_path() {
        let owner = Owner::new();
        let raw = owner.run(|| view_main_raw(ButtonState::Idle));
        let root_path = scene_root_path_segment(&raw);
        let tree = scene_to_tree_item(&raw, &root_path);
        let button_path = walk_button(&tree)
            .expect("forward walker must produce a button path on banner-off");

        owner.run(|| {
            use_selected_path().set(Some(button_path.clone()));
        });
        let painted = owner.run(|| view_main(ButtonState::Idle));
        assert!(
            scene_has_stroked_border(&painted),
            "selection {button_path:?} must light up the overlay",
        );
        assert!(
            find_main_btn_container(&painted),
            "button Container[main_btn] must persist under the wrapper",
        );
    }

    /// (R676) **Inspector-only id case.** Selection = `"state"` — the
    /// inspector's own TreeItem id, not a Scene-type path. The
    /// soft-signal contract returns `None` from
    /// `find_main_node_at_path`; view_main paints **without** the
    /// highlight overlay but **with** the banner ("Selected: state").
    #[test]
    fn r676_highlight_overlay_skips_inspector_only_id_state() {
        let owner = Owner::new();
        owner.run(|| {
            use_selected_path().set(Some("state".to_string()));
        });
        let scene = owner.run(|| view_main(ButtonState::Idle));
        assert!(
            !scene_has_stroked_border(&scene),
            "inspector-only path 'state' must not produce a stroked Border",
        );
        assert!(
            scene_has_text(&scene, "Selected: state"),
            "the banner must still render for the inspector-only path",
        );
    }

    /// (R676) `"main"` (inspector branch id) — same soft-fail path
    /// as `"state"`. Banner renders, no overlay.
    #[test]
    fn r676_highlight_overlay_skips_inspector_only_id_main() {
        let owner = Owner::new();
        owner.run(|| {
            use_selected_path().set(Some("main".to_string()));
        });
        let scene = owner.run(|| view_main(ButtonState::Idle));
        assert!(
            !scene_has_stroked_border(&scene),
            "inspector-only path 'main' must not produce a stroked Border",
        );
        assert!(
            scene_has_text(&scene, "Selected: main"),
            "the banner must still render for the inspector-only path",
        );
    }

    /// (R676) Stale path (pre-R676 `"0/0"` sibling-index form) — no
    /// match → no overlay. Banner still renders.
    #[test]
    fn r676_highlight_overlay_skips_stale_pre_r676_path() {
        let owner = Owner::new();
        owner.run(|| {
            use_selected_path().set(Some("0/0".to_string()));
        });
        let scene = owner.run(|| view_main(ButtonState::Idle));
        assert!(
            !scene_has_stroked_border(&scene),
            "stale pre-R676 path '0/0' must not produce a stroked Border",
        );
        assert!(
            scene_has_text(&scene, "Selected: 0/0"),
            "banner reflects the raw payload regardless of resolution",
        );
    }

    /// (R676) **Root selection.** Selection = `"Container"` (the
    /// root path produced by `scene_root_path_segment` on
    /// view_main_raw's untagged outer Container). The whole content
    /// area gets wrapped — root wrap is a valid case, exercised by
    /// the R676 demo's section (D).
    #[test]
    fn r676_highlight_overlay_wraps_root_path() {
        let owner = Owner::new();
        owner.run(|| {
            use_selected_path().set(Some("Container".to_string()));
        });
        let scene = owner.run(|| view_main(ButtonState::Idle));
        assert!(
            scene_has_stroked_border(&scene),
            "root-path selection must wrap the outer Container",
        );
        // The outermost node in the painted scene should be the
        // wrapper (has stroked border) and its first child should be
        // the original raw root (no border, but `BoxStyle::filled`).
        if let Scene::Container(c) = &scene {
            assert!(
                c.style.border.is_some_and(|b| b.width > 0),
                "root-path wrap places the stroked Border on the outermost Container",
            );
        } else {
            panic!("view_main must return a Scene::Container");
        }
    }

    /// (R676) **Path-stable proof at the visible-paint layer.** The
    /// button's tag-form path resolves and highlights regardless of
    /// banner presence. The banner adds a Text sibling above the
    /// button (pre-R676 this would shift the button's sibling
    /// index); the tag-form path doesn't change so the wrap lands at
    /// the same node, the same stroked Border appears, and the user
    /// experiences the highlight as "sticky to the selected
    /// element".
    #[test]
    fn r676_highlight_overlay_persists_under_banner_toggle() {
        let owner = Owner::new();
        let button_path = format!("Container/Container[{MAIN_BTN_TAG}]");

        // First: selection without banner (selection_path was
        // never set before, signal is None → no banner; we set the
        // signal to the button path, but the banner reflects the
        // current signal value which IS the button path).
        owner.run(|| {
            use_selected_path().set(Some(button_path.clone()));
        });
        let scene_with_banner = owner.run(|| view_main(ButtonState::Idle));
        assert!(
            scene_has_stroked_border(&scene_with_banner),
            "button selection (with banner) must produce a stroked Border",
        );
        assert!(
            scene_has_text(&scene_with_banner, &format!("Selected: {button_path}")),
            "banner reflects the selection payload",
        );

        // Verify the underlying button is still findable underneath
        // the wrapper.
        assert!(
            find_main_btn_container(&scene_with_banner),
            "the button persists under the wrapper across banner toggle",
        );
    }

    /// (R676) The wrapper's `BoxStyle` carries the **Error** ColorRole
    /// — pinning the canonical M3 token mapping so a future palette
    /// change does not silently swap the overlay colour.
    #[test]
    fn r676_highlight_overlay_uses_theme_error_role() {
        let owner = Owner::new();
        let expected = owner.run(|| {
            use_selected_path().set(Some(format!(
                "Container/Container[{MAIN_BTN_TAG}]",
            )));
            // `use_theme` requires an active Owner scope — read the
            // expected colour inside the same scope so the cached
            // ThemeProvider initialises against the same Owner the
            // view fn will use.
            use_theme(THEME_TAG).theme_animated().resolve(ColorRole::Error)
        });
        let scene = owner.run(|| view_main(ButtonState::Idle));
        let actual = find_first_stroked_border_color(&scene);
        assert_eq!(
            actual,
            Some(expected),
            "highlight overlay must use ColorRole::Error",
        );
    }

    /// (R676) Highlight overlay is **stable across button state
    /// mutations** — selection persists even when the user keyboards
    /// `d` / `e` to switch Idle ↔ Disabled. State changes the
    /// button's fill colour but not its path; the wrap stays put.
    #[test]
    fn r676_highlight_overlay_survives_button_state_change() {
        let owner = Owner::new();
        let button_path = format!("Container/Container[{MAIN_BTN_TAG}]");
        owner.run(|| {
            use_selected_path().set(Some(button_path.clone()));
        });
        let scene_idle = owner.run(|| view_main(ButtonState::Idle));
        let scene_disabled = owner.run(|| view_main(ButtonState::Disabled));
        assert!(
            scene_has_stroked_border(&scene_idle),
            "highlight present on Idle",
        );
        assert!(
            scene_has_stroked_border(&scene_disabled),
            "highlight survives Disabled state mutation",
        );
    }

    /// Test-only helper — walk a TreeItem produced by the forward
    /// walker for the first node whose id ends with the
    /// button's tag-form suffix.
    fn walk_button(item: &TreeItem) -> Option<String> {
        let suffix = format!("Container[{MAIN_BTN_TAG}]");
        if item.id.ends_with(&suffix) {
            return Some(item.id.clone());
        }
        item.children.iter().find_map(walk_button)
    }

    /// Test-only helper — depth-first walk for the colour of the
    /// first stroked Border encountered. Used by the ColorRole pin.
    fn find_first_stroked_border_color(scene: &Scene) -> Option<Color> {
        match scene {
            Scene::Container(c) => {
                if let Some(border) = c.style.border {
                    if border.width > 0 {
                        return Some(border.color);
                    }
                }
                for child in &c.children {
                    if let Some(color) = find_first_stroked_border_color(child) {
                        return Some(color);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// (R676) Wrapper composition shape pin — the wrapped subtree
    /// is the original raw subtree (no children copied/modified;
    /// only the wrapper Container is added). Exercises the
    /// `find_stroked_container_tag` helper which the demo uses for
    /// scene/snapshot verification.
    /// Test-only helper — depth-first walk for the first
    /// `ContainerNode` whose `BoxStyle` carries a non-zero stroked
    /// Border. Hoisted to module scope so
    /// `clippy::items_after_statements` stays clean.
    fn find_wrapper_container(scene: &Scene) -> Option<&ContainerNode> {
        match scene {
            Scene::Container(c) => {
                if c.style.border.is_some_and(|b| b.width > 0) {
                    return Some(c);
                }
                c.children.iter().find_map(find_wrapper_container)
            }
            _ => None,
        }
    }

    #[test]
    fn r676_highlight_overlay_wrapper_carries_single_child() {
        let owner = Owner::new();
        owner.run(|| {
            use_selected_path().set(Some(format!(
                "Container/Container[{MAIN_BTN_TAG}]",
            )));
        });
        let scene = owner.run(|| view_main(ButtonState::Idle));
        let wrapper = find_wrapper_container(&scene)
            .expect("wrapped scene must contain a stroked-Border wrapper Container");
        assert_eq!(
            wrapper.children.len(),
            1,
            "the wrapper Container must carry exactly one child (the wrapped subtree)",
        );
        let inner_tag = match &wrapper.children[0] {
            Scene::Container(c) => c.tag.as_deref(),
            _ => None,
        };
        assert_eq!(
            inner_tag,
            Some(MAIN_BTN_TAG),
            "the wrapper's single child must be the original tagged button",
        );
        // Silence the unused-helper warning when only one test
        // module touches the import in some build configurations.
        let _ = find_stroked_container_tag(&scene);
    }
}

#[cfg(test)]
mod r677_property_pane_layout_tests {
    //! R677 §5.16 §5.49 — inspector window 2-pane layout regression
    //! suite. Atomic (0) restructures `view_inspector` from the
    //! R675/R676 single-column `view_tree_focused` paint into a Row
    //! container with two children: tree pane (left, the existing
    //! TreeView) and property pane (right, new — atomic 1 populates
    //! the field rows). Tests pin the layout shape, both panes'
    //! presence, and the no-selection placeholder behaviour.
    use super::*;

    /// (R677) The inspector view's outermost Container uses
    /// `FlexDirection::Row` — the substrate's flexbox engine
    /// horizontally stacks the tree pane and the property pane.
    #[test]
    fn r677_inspector_outer_layout_is_row() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("inspector", ButtonState::Idle, &Frame::new())
        });
        match &scene {
            Scene::Container(c) => assert_eq!(
                c.layout.flex_direction,
                FlexDirection::Row,
                "inspector outer container must be a Row to host left+right panes",
            ),
            _ => panic!("inspector view must return a Scene::Container at the root"),
        }
    }

    /// (R677) Both panes are present at the inspector's first child
    /// level — left pane tagged `INSPECTOR_TREE_TAG`, right pane
    /// tagged `PROPERTY_PANE_TAG`.
    #[test]
    fn r677_inspector_carries_both_panes_at_top_level() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("inspector", ButtonState::Idle, &Frame::new())
        });
        let Scene::Container(c) = &scene else {
            panic!("inspector view must be a Container");
        };
        // First-level children list contains the two panes (in
        // left-to-right order: tree, then property pane).
        let pane_tags: Vec<Option<&str>> = c
            .children
            .iter()
            .map(|child| match child {
                Scene::Container(cc) => cc.tag.as_deref(),
                _ => None,
            })
            .collect();
        assert_eq!(pane_tags.len(), 2, "inspector must carry exactly 2 panes");
        assert_eq!(
            pane_tags[0],
            Some(INSPECTOR_TREE_TAG),
            "left pane must be the inspector tree",
        );
        assert_eq!(
            pane_tags[1],
            Some(PROPERTY_PANE_TAG),
            "right pane must be the property pane",
        );
    }

    /// (R677) The property pane's `LayoutStyle` carries `flex_grow =
    /// 1.0` so it fills available horizontal space when the
    /// inspector window resizes (atomic 2 expands the window to
    /// 480×320 to host the wider pane).
    #[test]
    fn r677_property_pane_flex_grow_one() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("inspector", ButtonState::Idle, &Frame::new())
        });
        let pane = find_pane_by_tag(&scene, PROPERTY_PANE_TAG)
            .expect("property pane must exist in the inspector view");
        assert!(
            (pane.layout.flex_grow - 1.0).abs() < f32::EPSILON,
            "property pane must declare flex_grow=1.0; got: {}",
            pane.layout.flex_grow,
        );
    }

    /// (R677) **No-selection baseline** — when `use_selected_path()`
    /// returns `None`, the property pane renders the
    /// `PROPERTY_PANE_NO_SELECTION_TEXT` placeholder. Pins the
    /// soft-fail default behaviour that the demo's section (B)
    /// verifies through scene/snapshot.
    #[test]
    fn r677_property_pane_renders_placeholder_when_no_selection() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("inspector", ButtonState::Idle, &Frame::new())
        });
        let pane = find_pane_by_tag(&scene, PROPERTY_PANE_TAG)
            .expect("property pane must exist");
        let text_contents: Vec<&str> = pane
            .children
            .iter()
            .filter_map(|child| match child {
                Scene::Text(t) => Some(t.content.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            text_contents,
            vec![PROPERTY_PANE_NO_SELECTION_TEXT],
            "no-selection pane must contain exactly the placeholder line",
        );
    }

    /// (R677) **Inspector-only id soft-fail** — selecting `"state"`
    /// (an inspector-only TreeItem id, not a Scene-type path)
    /// resolves to None through `find_main_node_at_path`; the pane
    /// renders the placeholder, NOT field rows. Demo section (F)
    /// verifies this.
    #[test]
    fn r677_property_pane_placeholder_for_inspector_only_id() {
        let owner = Owner::new();
        owner.run(|| {
            use_selected_path().set(Some("state".to_string()));
        });
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("inspector", ButtonState::Idle, &Frame::new())
        });
        let pane = find_pane_by_tag(&scene, PROPERTY_PANE_TAG)
            .expect("property pane must exist");
        let has_placeholder = pane.children.iter().any(|child| match child {
            Scene::Text(t) => t.content == PROPERTY_PANE_NO_SELECTION_TEXT,
            _ => false,
        });
        assert!(
            has_placeholder,
            "inspector-only id 'state' must resolve to placeholder, not field rows",
        );
    }

    /// (R677) Tree pane is preserved bit-identical — the R671→R676
    /// tree paint surface is unaffected by the atomic (0)
    /// restructure. Pin the tree pane's outer tag + the
    /// inspector_tree row tag presence (the tree rows themselves
    /// live deeper).
    #[test]
    fn r677_tree_pane_unaffected_by_layout_split() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("inspector", ButtonState::Idle, &Frame::new())
        });
        // Tree pane is the first child of the outer Row.
        let tree_pane = find_pane_by_tag(&scene, INSPECTOR_TREE_TAG)
            .expect("inspector_tree pane must persist");
        assert!(
            !tree_pane.children.is_empty(),
            "tree pane must carry its row children (TreeView paint)",
        );
    }

    /// (R677) `property_pane_rows` atomic (1) — minimum guarantee:
    /// every scene gets at least the `type:` row. Pre-atomic-(1)
    /// stub returned empty Vec; this test was rewritten to reflect
    /// the atomic (1) contract.
    #[test]
    fn r677_property_pane_rows_always_emits_type_row() {
        let scene = Scene::Container(
            pinion_core::scene::ContainerNode::new(vec![]).with_tag(MAIN_BTN_TAG),
        );
        let rows = property_pane_rows(&scene);
        assert!(
            !rows.is_empty(),
            "atomic (1) always emits at least the type: row",
        );
        assert!(
            rows[0].starts_with("type: "),
            "first row must be 'type: <Variant>'; got: {:?}",
            rows[0],
        );
    }

    /// Test-only helper — depth-first walk for the first
    /// `ContainerNode` with the requested tag.
    fn find_pane_by_tag<'s>(scene: &'s Scene, tag: &str) -> Option<&'s ContainerNode> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return Some(c);
                }
                c.children.iter().find_map(|child| find_pane_by_tag(child, tag))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod r677_field_walker_tests {
    //! R677 §5.16 §5.49 — property_pane_rows field walker
    //! regression suite. Atomic (1) replaces the atomic (0) empty
    //! stub with the real walker: emits `type:` plus variant-
    //! specific field rows. Tests pin (a) the universal `type:` row,
    //! (b) Container's full field set, (c) Text's content / font /
    //! fg rows, (d) External's tag row, (e) untagged Container shows
    //! `tag: —` em-dash placeholder, (f) format helpers' shape.
    use super::*;
    use pinion_core::external::StubExternal;
    use pinion_core::scene::{ContainerNode, ExternalNode, Rect, TextNode};
    use pinion_core::style::{BoxStyle, LayoutStyle, Size, TextStyle};

    /// (R677) Container rows include type + tag + style.fill +
    /// style.border + layout.size + children. Tagged container
    /// shows the tag itself, not the em-dash placeholder.
    #[test]
    fn r677_container_field_rows_full_set() {
        let scene = Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::styled(
                "x",
                Rect::default(),
                TextStyle::new(),
            ))])
            .with_tag(MAIN_BTN_TAG)
            .with_style(BoxStyle::filled(Color::rgba(255, 0, 0, 255)))
            .with_layout(LayoutStyle::new().with_size(Size::px(160, 64))),
        );
        let rows = property_pane_rows(&scene);
        // 6 rows: type, tag, style.fill, style.border, layout.size, children
        assert_eq!(
            rows.len(),
            6,
            "tagged Container produces 6 rows; got: {rows:?}",
        );
        assert_eq!(rows[0], "type: Container");
        assert_eq!(rows[1], format!("tag: {MAIN_BTN_TAG}"));
        assert_eq!(rows[2], "style.fill: rgba(255,0,0,255)");
        assert_eq!(rows[3], "style.border: none");
        assert_eq!(rows[4], "layout.size: 160px × 64px");
        assert_eq!(rows[5], "children: 1");
    }

    /// (R677) Untagged Container surfaces the em-dash placeholder
    /// in the tag row — canonical "no value" typographic convention
    /// used by Chrome / Firefox DevTools.
    #[test]
    fn r677_untagged_container_tag_row_is_em_dash() {
        let scene = Scene::Container(ContainerNode::new(vec![]));
        let rows = property_pane_rows(&scene);
        assert!(
            rows.iter().any(|r| r == "tag: —"),
            "untagged container must show em-dash placeholder; rows: {rows:?}",
        );
    }

    /// (R677) Container with `Auto × Auto` layout emits the auto
    /// CSS-mirror form (not zero-px). Pin the `format_size_value`
    /// helper's `Auto` branch via the integration shape.
    #[test]
    fn r677_container_size_auto_renders_as_css_auto() {
        let scene = Scene::Container(ContainerNode::new(vec![]));
        let rows = property_pane_rows(&scene);
        assert!(
            rows.iter().any(|r| r == "layout.size: auto × auto"),
            "default LayoutStyle::size = Auto×Auto renders as 'auto × auto'; rows: {rows:?}",
        );
    }

    /// (R677) Text node rows — content, font_size, fg. The variant
    /// has no children / layout / fill, so the row set is text-
    /// specific (no Container fields appear).
    #[test]
    fn r677_text_field_rows_content_font_fg() {
        let scene = Scene::Text(TextNode::styled(
            "Click me!",
            Rect::default(),
            TextStyle::new()
                .with_size_px(18)
                .with_fg(Color::rgba(26, 26, 26, 255)),
        ));
        let rows = property_pane_rows(&scene);
        // type + content + font_size + fg = 4 rows
        assert_eq!(
            rows.len(),
            4,
            "Text emits type + content + font_size + fg = 4 rows; got: {rows:?}",
        );
        assert_eq!(rows[0], "type: Text");
        assert_eq!(rows[1], r#"content: "Click me!""#);
        assert_eq!(rows[2], "style.font_size: 18px");
        assert_eq!(rows[3], "style.fg: rgba(26,26,26,255)");
    }

    /// (R677) External row set — just type + tag (the variant has
    /// no introspectable style/layout from the binding side; the
    /// External itself owns those internally).
    #[test]
    fn r677_external_field_rows_type_and_tag() {
        let scene = Scene::External(
            ExternalNode::new(Box::new(StubExternal::new())).with_tag("grid"),
        );
        let rows = property_pane_rows(&scene);
        assert_eq!(rows.len(), 2, "External emits 2 rows; got: {rows:?}");
        assert_eq!(rows[0], "type: External");
        assert_eq!(rows[1], "tag: grid");
    }

    /// (R677) Container with a stroked border emits the
    /// `<width>px @ rgba(...)` shape via `format_border`.
    #[test]
    fn r677_container_border_row_renders_width_at_color() {
        let scene = Scene::Container(
            ContainerNode::new(vec![]).with_style(
                BoxStyle::default().with_border(pinion_core::style::Border::new(
                    Color::rgba(186, 26, 26, 255),
                    2,
                )),
            ),
        );
        let rows = property_pane_rows(&scene);
        assert!(
            rows.iter().any(|r| r == "style.border: 2px @ rgba(186,26,26,255)"),
            "stroked Container border row must format as '<W>px @ rgba(...)'; rows: {rows:?}",
        );
    }

    /// (R677) Nested-children-count row reflects the actual length
    /// of `ContainerNode::children` (not a recursive descendant
    /// count). Mirrors Chrome DevTools' Elements panel children
    /// count.
    #[test]
    fn r677_container_children_count_is_direct_not_recursive() {
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Text(TextNode::styled("a", Rect::default(), TextStyle::new())),
            Scene::Container(ContainerNode::new(vec![Scene::Text(TextNode::styled(
                "deep",
                Rect::default(),
                TextStyle::new(),
            ))])),
        ]));
        let rows = property_pane_rows(&scene);
        // Direct children = 2 (Text + nested Container); the nested
        // Text inside the Container is NOT counted.
        assert!(
            rows.iter().any(|r| r == "children: 2"),
            "children count must be direct-only; rows: {rows:?}",
        );
    }

    /// (R677) **Percent SizeValue path** — `format_size_value`
    /// handles `Percent(N)` as `<N>%`. Direct unit test of the
    /// format helper for clarity (the integration path goes
    /// through `format_size`).
    #[test]
    fn r677_format_size_value_percent_path() {
        use pinion_core::style::SizeValue;
        assert_eq!(format_size_value(SizeValue::Auto), "auto");
        assert_eq!(format_size_value(SizeValue::Px(160)), "160px");
        assert_eq!(format_size_value(SizeValue::Percent(50)), "50%");
    }

    /// (R677) `format_color` produces the `rgba(R,G,B,A)` CSS
    /// canonical literal even at full opacity (alpha 255 still
    /// emits — consistent with the `format_color` doc contract).
    #[test]
    fn r677_format_color_includes_alpha_at_full_opacity() {
        assert_eq!(
            format_color(Color::rgba(0, 128, 255, 255)),
            "rgba(0,128,255,255)",
        );
        assert_eq!(
            format_color(Color::rgba(0, 0, 0, 0)),
            "rgba(0,0,0,0)",
        );
    }

    /// (R677) Integration: end-to-end inspector view with a
    /// selected button path → the property pane carries the
    /// expected field rows from `property_pane_rows` for the
    /// resolved Container node.
    #[test]
    fn r677_inspector_view_property_pane_carries_resolved_button_rows() {
        let owner = Owner::new();
        let button_path = format!("Container/Container[{MAIN_BTN_TAG}]");
        owner.run(|| {
            use_selected_path().set(Some(button_path.clone()));
        });
        let scene = owner.run(|| {
            MultiWindowView::view_for_window("inspector", ButtonState::Idle, &Frame::new())
        });
        let pane = find_pane_by_tag_in_scene(&scene, PROPERTY_PANE_TAG)
            .expect("property pane must exist");
        let text_contents: Vec<&str> = pane
            .children
            .iter()
            .filter_map(|child| match child {
                Scene::Text(t) => Some(t.content.as_str()),
                _ => None,
            })
            .collect();
        // The selected node is the main_btn Container. Pin the
        // canonical rows. (Exact fill / size values depend on the
        // theme; pin only the stable shape rows.)
        assert!(
            text_contents.contains(&"type: Container"),
            "pane must include 'type: Container' for the resolved button; got: {text_contents:?}",
        );
        let expected_tag_row = format!("tag: {MAIN_BTN_TAG}");
        assert!(
            text_contents.contains(&expected_tag_row.as_str()),
            "pane must include 'tag: main_btn' for the resolved button; got: {text_contents:?}",
        );
        assert!(
            text_contents.iter().any(|s| s.starts_with("style.fill: ")),
            "pane must include 'style.fill: rgba(...)' row; got: {text_contents:?}",
        );
        assert!(
            text_contents.iter().any(|s| s.starts_with("layout.size: ")),
            "pane must include 'layout.size: ...' row; got: {text_contents:?}",
        );
        assert!(
            text_contents.contains(&"children: 1"),
            "button container holds 1 child (the label Text); got: {text_contents:?}",
        );
    }

    /// Test-only helper — module-local duplicate of
    /// `r677_property_pane_layout_tests::find_pane_by_tag` (Rust's
    /// mod-private helpers don't cross test-module boundaries).
    fn find_pane_by_tag_in_scene<'s>(scene: &'s Scene, tag: &str) -> Option<&'s ContainerNode> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return Some(c);
                }
                c.children
                    .iter()
                    .find_map(|child| find_pane_by_tag_in_scene(child, tag))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod r678_hover_bridge_tests {
    //! R678 §5.16 §5.49 — DevTools cross-window hover-overlay
    //! regression suite. Atomic (1) wires the inspector
    //! [`TreeRowClickExternal`]'s new `hover` axis into a parallel
    //! cross-window [`use_hovered_path`] signal; `view_main` then
    //! reads BOTH `use_selected_path` and `use_hovered_path` and
    //! paints two distinct wraps — Error red for selection, M3
    //! `SurfaceContainerHighest` for hover, selection winning on the
    //! same node, both visible on different nodes.
    use super::*;
    use pinion_core::theme::Theme;

    /// Walk a scene depth-first and return every stroked-border color
    /// in declaration order. The hover-bridge tests assert on the
    /// **set** of border colors to verify which wraps painted (no
    /// false-positive count from the selection wrap when both should
    /// fire).
    fn collect_border_colors(scene: &Scene) -> Vec<Color> {
        fn walk(scene: &Scene, out: &mut Vec<Color>) {
            if let Scene::Container(c) = scene {
                if let Some(border) = c.style.border {
                    if border.width > 0 {
                        out.push(border.color);
                    }
                }
                for child in &c.children {
                    walk(child, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(scene, &mut out);
        out
    }

    /// (R678) `INSPECTOR_HOVER_INTENT_TAG` is the canonical dotted
    /// composition of the External tag and the substrate's hover
    /// event name. Pinned so a rename on either side surfaces here.
    #[test]
    fn r678_inspector_hover_intent_tag_is_canonical() {
        assert_eq!(INSPECTOR_HOVER_INTENT_TAG, "inspector_tree.hover");
    }

    /// (R678) `use_hovered_path()` returns a fresh Signal initialised
    /// to `None` (no row hovered at boot). Pinned so a regression
    /// that pre-seeds the slot surfaces immediately — the demo's
    /// section (A) cycle 1 verification rides on the boot-clean
    /// invariant.
    #[test]
    fn r678_use_hovered_path_initial_value_is_none() {
        let owner = Owner::new();
        owner.run(|| {
            let signal = use_hovered_path();
            assert!(
                signal.get().is_none(),
                "fresh Owner.cache slot for hovered_path must hold None",
            );
        });
    }

    /// (R678) `use_hovered_path()` is **per-Owner cached** — repeated
    /// calls within the same Owner scope return the same `Rc` handle.
    /// Same contract as `use_selected_path` — both arcs share the
    /// `Owner::cache` + `&'static str` slot-key convention.
    #[test]
    fn r678_use_hovered_path_is_owner_cached_singleton() {
        let owner = Owner::new();
        owner.run(|| {
            let a = use_hovered_path();
            let b = use_hovered_path();
            assert!(
                Rc::ptr_eq(&a, &b),
                "use_hovered_path must return the same Rc<Signal> on every call",
            );
        });
    }

    /// (R678) `use_hovered_path()` and `use_selected_path()` are
    /// independent slots — mutating one does NOT mutate the other.
    /// The cross-axis decoupling is a R678 design pin: bindings
    /// reduce hover and selection on separate Signal arcs so the
    /// wrap precedence (selection-wins-on-same-node) can be computed
    /// from both reads at paint time.
    #[test]
    fn r678_hovered_and_selected_signals_are_independent() {
        let owner = Owner::new();
        owner.run(|| {
            use_hovered_path().set(Some("path-a".to_string()));
            use_selected_path().set(Some("path-b".to_string()));
            assert_eq!(
                use_hovered_path().get().as_deref(),
                Some("path-a"),
                "hovered slot stays under hover mutation",
            );
            assert_eq!(
                use_selected_path().get().as_deref(),
                Some("path-b"),
                "selected slot stays under selection mutation",
            );
        });
    }

    /// (R678) Reducer routes `inspector_tree.hover` with `Text(path)`
    /// payload into `use_hovered_path()` as `Some(path)`. The
    /// PointerEnter arc.
    #[test]
    fn r678_update_reducer_routes_hover_text_intent_to_signal() {
        let owner = Owner::new();
        owner.run(|| {
            assert!(use_hovered_path().get().is_none(), "boot-clean precondition");
            let intent = Intent::new_static(
                INSPECTOR_HOVER_INTENT_TAG,
                IntrospectValue::Text("Container/Container[main_btn]".to_string()),
            );
            let cmds = MultiWindowView::update(ButtonState::Idle, &intent);
            assert!(cmds.is_empty(), "hover reducer is side-effect-only");
            assert_eq!(
                use_hovered_path().get().as_deref(),
                Some("Container/Container[main_btn]"),
                "Text(path) hover intent must write Some(path)",
            );
        });
    }

    /// (R678) Reducer routes `inspector_tree.hover` with `Null`
    /// payload into `use_hovered_path()` as `None`. The PointerLeave
    /// arc.
    #[test]
    fn r678_update_reducer_routes_hover_null_intent_clears_signal() {
        let owner = Owner::new();
        owner.run(|| {
            use_hovered_path().set(Some("pre-existing".to_string()));
            let intent = Intent::new_static(
                INSPECTOR_HOVER_INTENT_TAG,
                IntrospectValue::Null,
            );
            let _ = MultiWindowView::update(ButtonState::Idle, &intent);
            assert!(
                use_hovered_path().get().is_none(),
                "Null hover intent must clear the slot to None",
            );
        });
    }

    /// (R678) Hover intent does NOT disturb the selection slot — the
    /// two arcs are routed by tag prefix, so a hover intent should
    /// be a no-op on `use_selected_path`. Defensive pin against a
    /// future reducer refactor accidentally cross-wiring the slots.
    #[test]
    fn r678_hover_intent_does_not_disturb_selected_path_signal() {
        let owner = Owner::new();
        owner.run(|| {
            use_selected_path().set(Some("selected-only".to_string()));
            let intent = Intent::new_static(
                INSPECTOR_HOVER_INTENT_TAG,
                IntrospectValue::Text("hover-only".to_string()),
            );
            let _ = MultiWindowView::update(ButtonState::Idle, &intent);
            assert_eq!(
                use_selected_path().get().as_deref(),
                Some("selected-only"),
                "hover intent must not write into the selected_path slot",
            );
            assert_eq!(
                use_hovered_path().get().as_deref(),
                Some("hover-only"),
            );
        });
    }

    /// (R678) Hover intent with non-Text/non-Null payload is a silent
    /// no-op — defensive against a future substrate hover-axis
    /// expansion (mouse-button mask, pointer-id discriminator) being
    /// introduced without lockstep binding update.
    #[test]
    fn r678_hover_intent_with_unknown_payload_variant_silent() {
        let owner = Owner::new();
        owner.run(|| {
            use_hovered_path().set(Some("pre-existing".to_string()));
            let intent = Intent::new_static(
                INSPECTOR_HOVER_INTENT_TAG,
                IntrospectValue::Int(42),
            );
            let _ = MultiWindowView::update(ButtonState::Idle, &intent);
            assert_eq!(
                use_hovered_path().get().as_deref(),
                Some("pre-existing"),
                "unknown payload variant must preserve the slot",
            );
        });
    }

    /// (R678) `view_main` baseline — no hover signal set ⇒ no
    /// stroked-Border anywhere. Same shape as the R676 selection-
    /// absent baseline, kept as a separate R678 pin so a regression
    /// that crosses the axes is caught at the hover-empty case.
    #[test]
    fn r678_view_main_no_hover_no_extra_border() {
        let owner = Owner::new();
        let scene = owner.run(|| view_main(ButtonState::Idle));
        let colors = collect_border_colors(&scene);
        assert!(
            colors.is_empty(),
            "no-hover, no-selection scene must paint without any stroked Border",
        );
    }

    /// (R678) `view_main` with hover path resolving against the
    /// button ⇒ a stroked Border whose color is M3
    /// `SurfaceContainerHighest` (the canonical hover overlay color
    /// per the R678 atomic-1 design). The path resolves through
    /// `find_main_node_at_path` (R676 helper, now also driving the
    /// hover walker).
    #[test]
    fn r678_view_main_paints_hover_wrap_in_surface_container_highest() {
        let owner = Owner::new();
        owner.run(|| {
            use_hovered_path().set(Some(format!(
                "Container/Container[{MAIN_BTN_TAG}]",
            )));
        });
        let scene = owner.run(|| view_main(ButtonState::Idle));
        let colors = collect_border_colors(&scene);
        assert_eq!(
            colors.len(),
            1,
            "exactly one hover wrap when only the hover signal is set",
        );
        // Compare the color against the live theme — the theme system
        // owns the actual rgba value (light vs dark palette), so we
        // dereference through Theme rather than hardcoding the rgba.
        let expected = Theme::light().resolve(ColorRole::SurfaceContainerHighest);
        assert_eq!(
            colors[0], expected,
            "hover wrap must paint with M3 SurfaceContainerHighest",
        );
    }

    /// (R678) `view_main` with selection and hover on **different**
    /// paths ⇒ both wraps paint, with distinct colors. The demo's
    /// primary "AI-introspectable both-visible" assertion.
    #[test]
    fn r678_view_main_paints_both_wraps_on_different_paths() {
        let owner = Owner::new();
        let theme = Theme::light();
        let selection_color = theme.resolve(ColorRole::Error);
        let hover_color = theme.resolve(ColorRole::SurfaceContainerHighest);
        owner.run(|| {
            // Selection on the root Container; hover on the button.
            use_selected_path().set(Some("Container".to_string()));
            use_hovered_path().set(Some(format!(
                "Container/Container[{MAIN_BTN_TAG}]",
            )));
        });
        let scene = owner.run(|| view_main(ButtonState::Idle));
        let colors = collect_border_colors(&scene);
        assert_eq!(
            colors.len(),
            2,
            "selection + hover on different nodes ⇒ two stroked Borders",
        );
        assert!(
            colors.contains(&selection_color),
            "selection wrap (Error red) must paint",
        );
        assert!(
            colors.contains(&hover_color),
            "hover wrap (SurfaceContainerHighest) must paint",
        );
    }

    /// (R678) `view_main` with selection and hover on the **same**
    /// path ⇒ only the selection wrap paints. The "selection wins"
    /// rule the design canon explicitly calls out — duplicate nested
    /// borders on the same node would be visually noisy.
    #[test]
    fn r678_view_main_selection_wins_when_both_target_same_node() {
        let owner = Owner::new();
        let selection_color = Theme::light().resolve(ColorRole::Error);
        let path = format!("Container/Container[{MAIN_BTN_TAG}]");
        owner.run(|| {
            use_selected_path().set(Some(path.clone()));
            use_hovered_path().set(Some(path.clone()));
        });
        let scene = owner.run(|| view_main(ButtonState::Idle));
        let colors = collect_border_colors(&scene);
        assert_eq!(
            colors.len(),
            1,
            "same-node selection+hover ⇒ exactly one wrap (selection)",
        );
        assert_eq!(
            colors[0], selection_color,
            "the surviving wrap must be the Error red selection color",
        );
    }

    /// (R678) `view_main` soft-fails on inspector-only ids (`state`,
    /// `main`) in the hover slot — the find walker returns `None`,
    /// so no hover wrap paints. Same soft-fail contract as the
    /// selection wrap (R676), now extended to the hover axis.
    #[test]
    fn r678_view_main_hover_on_inspector_only_id_paints_no_wrap() {
        let owner = Owner::new();
        for stale in &["state", "main"] {
            owner.run(|| {
                use_hovered_path().set(Some((*stale).to_string()));
            });
            let scene = owner.run(|| view_main(ButtonState::Idle));
            assert!(
                collect_border_colors(&scene).is_empty(),
                "hover on inspector-only id {stale:?} must not paint a wrap",
            );
        }
    }

    /// (R678) `view_main` soft-fails on stale hover paths — when the
    /// path doesn't resolve against the current scene (e.g., a path
    /// from a prior scene shape), no hover wrap paints. The post-
    /// soft-fail behaviour matches the selection-wrap soft-fail.
    #[test]
    fn r678_view_main_hover_on_stale_path_paints_no_wrap() {
        let owner = Owner::new();
        owner.run(|| {
            use_hovered_path().set(Some(
                "Container/Container[ghost_tag_never_existed]".to_string(),
            ));
        });
        let scene = owner.run(|| view_main(ButtonState::Idle));
        assert!(
            collect_border_colors(&scene).is_empty(),
            "stale hover path must not paint a wrap",
        );
    }

    /// Recursive walker — does any Text node in `scene` start with
    /// the literal "Selected:" prefix the selection banner uses?
    /// Hoisted to module scope so `clippy::items_after_statements`
    /// stays clean inside the test fn that consumes it.
    fn has_selected_banner(scene: &Scene) -> bool {
        match scene {
            Scene::Text(t) => t.content.starts_with("Selected:"),
            Scene::Container(c) => c.children.iter().any(has_selected_banner),
            _ => false,
        }
    }

    /// (R678) `view_main` with hover-only (selection None) paints
    /// the hover wrap without disturbing the no-selection banner
    /// invariant (no "Selected:" banner). The hover axis is
    /// presentational-only — it does NOT set a banner.
    #[test]
    fn r678_view_main_hover_alone_paints_no_banner() {
        let owner = Owner::new();
        owner.run(|| {
            use_hovered_path().set(Some(format!(
                "Container/Container[{MAIN_BTN_TAG}]",
            )));
        });
        let scene = owner.run(|| view_main(ButtonState::Idle));
        assert!(
            !has_selected_banner(&scene),
            "hover-only must NOT introduce a 'Selected:' banner",
        );
    }
}
