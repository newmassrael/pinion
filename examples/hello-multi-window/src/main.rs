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

use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Color, Frame, Scene, WidgetCore};
#[cfg(test)]
use pinion_a11y::WidgetA11y;
use pinion_shell::{vello_renderer_impl, SizeStrategy, WidgetView, WindowSpec};
use pinion_widget_paint::tree_view::{view_tree, TreeItem, TreeViewStyle};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloMultiWindowRenderer, HelloMultiWindowRendererError);

/// Main window dimensions — large enough for a comfortable Button
/// click target + the "Click me!" label.
const MAIN_W: u32 = 320;
const MAIN_H: u32 = 200;
/// Inspector window dimensions — narrower + shorter than main since
/// the only content is a 2-line state-debug text.
const INSPECTOR_W: u32 = 280;
const INSPECTOR_H: u32 = 140;

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

/// Main window paint — Button widget centred in the window. Mirrors
/// `hello-button`'s view fn shape exactly (label + filled box +
/// state-based fill colour) so the inspector mirror has familiar
/// `ButtonState` values to display.
fn view_main(state: ButtonState) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let surface = theme.resolve(ColorRole::Surface);
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let idle_fill = theme.resolve(ColorRole::SurfaceContainerHighest);
    let btn_fill: Color = match state {
        ButtonState::Idle => idle_fill,
        ButtonState::Hover => idle_fill.lerp(on_surface, 0.08),
        ButtonState::Pressed => idle_fill.lerp(on_surface, 0.12),
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
    Scene::Container(
        ContainerNode::new(vec![button])
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
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
    let main_scene = view_main(state);
    // R671 §5.50 — surface the live `ButtonState` variant as a
    // dedicated leaf at the top of the tree so AI clients can read
    // it through `scene/snapshot {window: "inspector"}` text-walk
    // without having to parse the deeper button-rect / fill-colour
    // structure. The composite-row tag this produces
    // (`inspector_tree#state`) is the canonical RPC entry point for
    // state introspection; the main scene tree underneath the
    // `main` row carries the same information structurally.
    let tree_items = vec![
        TreeItem::leaf("state", format!("State: {state:?}")),
        TreeItem::branch(
            "main",
            "main window scene",
            true,
            vec![scene_to_tree_item(&main_scene, "0")],
        ),
    ];
    view_tree(
        INSPECTOR_TREE_TAG,
        &tree_items,
        &theme,
        &TreeViewStyle::m3_default(),
    )
}

/// R671 §5.16 — walk one `Scene` node into a [`TreeItem`]. Containers
/// become branches whose `label` shows the variant + tag (when
/// tagged); text nodes become leaves carrying the rendered content;
/// every other variant carries a minimal "variant name" leaf.
///
/// `id` carries the path through the parent tree (`"0"` at root, then
/// `"0/0"`, `"0/0/1"` etc. per the JSON-Pointer-style convention RPC
/// `scene/snapshot` uses). All branches default to `expanded = true`
/// so the inspector shows the full main scene tree the moment it
/// boots; collapse + multi-select are 2nd-consumer carries
/// per [[abstraction-needs-second-consumer]].
fn scene_to_tree_item(scene: &Scene, id: &str) -> TreeItem {
    match scene {
        Scene::Container(c) => {
            let tag_segment = c
                .tag
                .as_deref()
                .map_or(String::new(), |t| format!(" [{t}]"));
            let label = format!("Container{tag_segment}");
            let children: Vec<TreeItem> = c
                .children
                .iter()
                .enumerate()
                .map(|(idx, child)| {
                    let child_id = format!("{id}/{idx}");
                    scene_to_tree_item(child, &child_id)
                })
                .collect();
            TreeItem::branch(id.to_owned(), label, true, children)
        }
        Scene::Text(t) => {
            let label = format!("Text: {:?}", t.content);
            TreeItem::leaf(id.to_owned(), label)
        }
        Scene::External(e) => {
            let tag_segment = e
                .tag
                .as_deref()
                .map_or(String::new(), |t| format!(" [{t}]"));
            TreeItem::leaf(id.to_owned(), format!("External{tag_segment}"))
        }
        Scene::Box(_) => TreeItem::leaf(id.to_owned(), "Box"),
        Scene::Path(_) => TreeItem::leaf(id.to_owned(), "Path"),
        Scene::Image(_) => TreeItem::leaf(id.to_owned(), "Image"),
        Scene::Effect(_) => TreeItem::leaf(id.to_owned(), "Effect"),
        Scene::Scroll(_) => TreeItem::leaf(id.to_owned(), "Scroll"),
        // `pinion_core::Scene` is `#[non_exhaustive]`; any future
        // variant lands here as an opaque leaf so the inspector
        // never panics on an unknown node.
        _ => TreeItem::leaf(id.to_owned(), "(unknown variant)"),
    }
}

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

    fn read_state(scene: &Scene) -> Self::State {
        // ButtonState reads from the SCXML state slot via the
        // standard `query("state")` introspect — same as
        // hello-button. Default Idle when introspect is missing
        // (single-widget binding, no composite state to merge).
        if let Scene::External(node) = scene
            && let Some(intro) = node.handle.introspect()
            && let Some(pinion_core::external::IntrospectValue::Text(name)) =
                intro.query("state")
        {
            return <Self::State as pinion_core::WidgetStateName>::from_name_or_default(
                &name,
            );
        }
        ButtonState::Idle
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
}
