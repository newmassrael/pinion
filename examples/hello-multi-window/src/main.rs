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

use std::collections::HashMap;
use std::rc::Rc;

use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Color, Frame, Owner, Scene, Signal, WidgetCore};
#[cfg(test)]
use pinion_a11y::WidgetA11y;
use pinion_shell::{vello_renderer_impl, SizeStrategy, WidgetView, WindowSpec};
use pinion_widget_paint::tree_view::{
    view_tree_focused, TreeItem, TreeRowClickExternal, TreeViewFocus, TreeViewStyle,
};

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

/// R676 §5.16 §5.49 — DevTools highlight overlay border width in
/// logical pixels. M3 / Material You spec is silent on
/// inspector/overlay strokes (they sit outside the system token
/// vocabulary); 2 px is the conventional Browser-DevTools selection
/// border width across Chrome / Firefox / Safari inspectors so the
/// affordance reads as "selected element" without disturbing the
/// underlying paint. Constant rather than inline so a future
/// substrate lift (R677+ when the 2nd consumer surfaces) inherits
/// one canonical value.
const HIGHLIGHT_BORDER_WIDTH: u32 = 2;

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
    let highlight_color = theme.resolve(ColorRole::Error);
    let raw_scene = view_main_raw(state);
    let selected = use_selected_path().get();
    match selected.as_deref() {
        Some(path) if find_main_node_at_path(&raw_scene, path).is_some() => {
            rebuild_with_highlight_at_path(raw_scene, path, highlight_color)
        }
        _ => raw_scene,
    }
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
    let main_root_path = scene_root_path_segment(&main_scene);
    let tree_items = vec![
        TreeItem::leaf("state", format!("State: {state:?}")),
        TreeItem::branch(
            "main",
            "main window scene",
            true,
            vec![scene_to_tree_item(&main_scene, &main_root_path)],
        ),
    ];
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
    view_tree_focused(
        INSPECTOR_TREE_TAG,
        &tree_items,
        &theme,
        &TreeViewStyle::m3_default(),
        &focus,
    )
}

/// R676 §5.16 §5.49 — Browser-DevTools-canonical type name for a
/// [`Scene`] variant. Returned strings are short literal identifiers
/// that participate in path segments (e.g. `Container[main_btn]`,
/// `Text[0]`). The mapping is exhaustive over the known
/// `#[non_exhaustive]` variants of `pinion_core::Scene`; unknown
/// future variants collapse to `"Unknown"` so the path scheme stays
/// total.
fn scene_type_name(scene: &Scene) -> &'static str {
    match scene {
        Scene::Container(_) => "Container",
        Scene::Text(_) => "Text",
        Scene::External(_) => "External",
        Scene::Box(_) => "Box",
        Scene::Path(_) => "Path",
        Scene::Image(_) => "Image",
        Scene::Effect(_) => "Effect",
        Scene::Scroll(_) => "Scroll",
        _ => "Unknown",
    }
}

/// R676 §5.16 §5.49 — tag of a [`Scene`] node when the variant carries
/// one. Only `Container` and `External` are tag-bearing; every other
/// variant returns `None` and uses the nth-of-type index form in path
/// segments.
fn scene_tag(scene: &Scene) -> Option<&str> {
    match scene {
        Scene::Container(c) => c.tag.as_deref(),
        Scene::External(e) => e.tag.as_deref(),
        _ => None,
    }
}

/// R676 §5.16 §5.49 — root-level path segment for a [`Scene`] node.
///
/// The root has no siblings to disambiguate against, so the
/// nth-of-type index is omitted. Tagged roots keep their tag bracket
/// (`Container[main_btn]`); untagged roots collapse to the bare type
/// name (`Container`). Mirrors the leading segment of the canonical
/// `"Container/Container[main_btn]/Text[0]"` shape.
fn scene_root_path_segment(scene: &Scene) -> String {
    let ty = scene_type_name(scene);
    match scene_tag(scene) {
        Some(tag) => format!("{ty}[{tag}]"),
        None => ty.to_owned(),
    }
}

/// R676 §5.16 §5.49 — non-root path segment for a [`Scene`] node
/// given its **nth-of-type** sibling index (0-based, counted only
/// against siblings of the same `Scene` variant — `Text[0]` and
/// `Container[0]` coexist at the same parent level).
///
/// Tagged Container/External use the tag form
/// (`Container[main_btn]`, `External[grid]`); untagged or non-
/// tag-bearing variants use the index form (`Text[0]`, `Box[2]`).
/// CSS-selector mirror: stable element ids (tags) take priority,
/// nth-of-type is the ordinal fallback for anonymous siblings.
fn scene_child_path_segment(scene: &Scene, nth_of_type: usize) -> String {
    let ty = scene_type_name(scene);
    match scene_tag(scene) {
        Some(tag) => format!("{ty}[{tag}]"),
        None => format!("{ty}[{nth_of_type}]"),
    }
}

/// R676 §5.16 §5.49 — disambiguator portion of a non-root path
/// segment. Tag form (`Container[main_btn]`) carries the borrowed tag
/// string; nth-of-type form (`Text[0]`) carries the parsed index.
///
/// Numeric strings inside the bracket are always interpreted as
/// nth-of-type indices — tags in pinion are identifier-shaped (never
/// pure numerics in the canonical paint code), so the parser
/// disambiguates by `usize::from_str` success. This matches the
/// CSS-selector mirror convention: `:nth-of-type(N)` for ordinal
/// access, `#tag` for id access.
///
#[derive(Debug, PartialEq, Eq)]
enum PathDisambiguator<'p> {
    Tag(&'p str),
    NthOfType(usize),
}

/// R676 §5.16 §5.49 — parse one `/`-separated path segment into a
/// `(type_name, disambiguator)` pair. Returns `None` only when the
/// bracket form is malformed (`Container[` without closing `]`).
///
/// Three shapes:
///
/// * `"Container"` — bare type, root-only. `disambiguator = None`.
/// * `"Container[main_btn]"` — tag form. `disambiguator =
///   Some(Tag("main_btn"))`.
/// * `"Text[0]"` — nth-of-type form. `disambiguator =
///   Some(NthOfType(0))`.
fn parse_path_segment(segment: &str) -> Option<(&str, Option<PathDisambiguator<'_>>)> {
    if let Some((ty, rest)) = segment.split_once('[') {
        let disambiguator_str = rest.strip_suffix(']')?;
        let disambiguator = match disambiguator_str.parse::<usize>() {
            Ok(idx) => PathDisambiguator::NthOfType(idx),
            Err(_) => PathDisambiguator::Tag(disambiguator_str),
        };
        Some((ty, Some(disambiguator)))
    } else {
        // Bare type — valid only at the root segment.
        Some((segment, None))
    }
}

/// R676 §5.16 §5.49 — find the child of `container` matching the
/// `(type, disambiguator)` shape carried by a single non-root path
/// segment.
///
/// For tag-form disambiguators, returns the first child of the
/// requested Scene variant whose tag equals the requested tag. For
/// nth-of-type-form, returns the child at the requested index among
/// all siblings of the requested variant (tagged or untagged — the
/// counting exactly mirrors the forward walker's
/// `scene_to_tree_item`).
fn find_child_in_container<'s>(
    container: &'s ContainerNode,
    ty: &str,
    disambiguator: &PathDisambiguator<'_>,
) -> Option<&'s Scene> {
    match disambiguator {
        PathDisambiguator::Tag(tag) => container.children.iter().find(|child| {
            scene_type_name(child) == ty && scene_tag(child) == Some(*tag)
        }),
        PathDisambiguator::NthOfType(idx) => container
            .children
            .iter()
            .filter(|child| scene_type_name(child) == ty)
            .nth(*idx),
    }
}

/// R676 §5.16 §5.49 — **the inverse walker.** Given a path string
/// produced by [`scene_to_tree_item`] (or
/// [`scene_root_path_segment`] for the root), descend `root` to
/// return the matching `&Scene` borrow.
///
/// Returns `None` when the path cannot be resolved against the
/// current scene shape — for example:
///
/// * inspector-only TreeItem ids (`"state"`, `"main"`) that were
///   never produced by [`scene_to_tree_item`];
/// * paths into untagged Containers whose sibling shape has churned
///   (a tagged element's path stays stable, but a `Text[2]`-form path
///   can shift if untagged sibling counts change between paint
///   cycles);
/// * paths that bottom out at a non-Container variant before all
///   segments are consumed (e.g. trying to descend `Container/Text[0]/Foo`
///   when `Text[0]` is a leaf);
/// * malformed segments (`Container[` with no closing `]`).
///
/// `None` is a soft signal — the caller (R676 atomic 2's highlight
/// overlay) treats it as "no match, do not wrap" and continues the
/// paint normally. This is the **textbook canonical contract** for
/// DevTools-style selection bridges: selection state may carry
/// stale or inspector-only ids; the renderer's job is to gracefully
/// skip highlight when the id no longer resolves.
fn find_main_node_at_path<'s>(root: &'s Scene, path: &str) -> Option<&'s Scene> {
    let mut segments = path.split('/');
    let root_seg = segments.next()?;
    let (root_ty, root_disambiguator) = parse_path_segment(root_seg)?;

    if scene_type_name(root) != root_ty {
        return None;
    }
    // The root may carry a tag disambiguator (rare but supported);
    // an nth-of-type disambiguator at the root is malformed because
    // the root has no siblings. Reject both mismatches as None per
    // the soft-signal contract documented above.
    if let Some(disambiguator) = &root_disambiguator {
        match disambiguator {
            PathDisambiguator::Tag(tag) => {
                if scene_tag(root) != Some(*tag) {
                    return None;
                }
            }
            PathDisambiguator::NthOfType(_) => return None,
        }
    }

    let mut current = root;
    for segment in segments {
        let (ty, disambiguator) = parse_path_segment(segment)?;
        // Non-root segments must carry a disambiguator (the forward
        // walker always emits the bracket form for non-root nodes).
        // A bare-type non-root segment is malformed; treat as None.
        let disambiguator = disambiguator?;
        let Scene::Container(c) = current else {
            return None;
        };
        current = find_child_in_container(c, ty, &disambiguator)?;
    }
    Some(current)
}

/// R676 §5.16 §5.49 — wrap `scene` in a transparent
/// [`Scene::Container`] carrying a stroked border in `color`. The
/// inner scene paints normally; the outer wrapper contributes only the
/// 2 px border outline that flags the node as "selected" in the
/// DevTools selection bridge.
///
/// **Why a wrapper container and not a [`BoxStyle`] mutation on the
/// node itself?** The selection overlay must be non-destructive: a
/// future round may want to highlight a `Scene::Text` leaf, or a
/// `Scene::External` whose `BoxStyle` is owned by the External
/// (immutable from the binding). Wrapping is the canonical
/// non-destructive composition path — the underlying node keeps its
/// paint exactly, the wrapper only adds the border outline + an
/// auto-sized layout sidecar inheriting from the child.
///
/// The wrapper uses `Default` `LayoutStyle` so it does not force any
/// flex / size constraints onto the child; the child's own layout
/// drives the final size, and the wrapper's `Rect` is computed by
/// `pinion-runtime`'s layout pass to fit the child's bounding box.
fn wrap_with_highlight(scene: Scene, color: Color) -> Scene {
    Scene::Container(ContainerNode::new(vec![scene]).with_style(
        BoxStyle::default().with_border(Border::new(color, HIGHLIGHT_BORDER_WIDTH)),
    ))
}

/// R676 §5.16 §5.49 — **the path-stable rebuild walker.** Given a
/// scene root and a path string produced by [`scene_to_tree_item`] /
/// [`scene_root_path_segment`], descend the scene by-value and wrap
/// the matching node with [`wrap_with_highlight`].
///
/// Returns the scene unchanged when the path does not resolve (see
/// [`find_main_node_at_path`] for the matching `&Scene`-borrow inverse
/// walker — both helpers share the soft-signal contract). Mirrors the
/// CSS-selector descent semantics of `find_main_node_at_path`: root
/// segment may be bare type or tag form; non-root segments must
/// carry a disambiguator.
///
/// The walker takes `scene` by value rather than by `&mut Scene`
/// because the matching subtree is **moved** into the wrapper and
/// replaced with the wrapper at the original child slot; mutating
/// `&mut Scene` in place would require `mem::replace` ceremony for a
/// transient placeholder. By-value descent + per-step ownership
/// transfer is the cleaner Rust idiom for tree rewrites of this
/// shape.
fn rebuild_with_highlight_at_path(scene: Scene, path: &str, color: Color) -> Scene {
    let mut segments = path.split('/');
    let Some(root_seg) = segments.next() else {
        return scene;
    };
    let Some((root_ty, root_disambiguator)) = parse_path_segment(root_seg) else {
        return scene;
    };
    if scene_type_name(&scene) != root_ty {
        return scene;
    }
    if let Some(disambiguator) = &root_disambiguator {
        match disambiguator {
            PathDisambiguator::Tag(tag) => {
                if scene_tag(&scene) != Some(*tag) {
                    return scene;
                }
            }
            // Root nth-of-type is malformed (root has no siblings);
            // match `find_main_node_at_path`'s rejection so the two
            // helpers carry the same soft-signal semantics.
            PathDisambiguator::NthOfType(_) => return scene,
        }
    }
    let remaining: Vec<&str> = segments.collect();
    if remaining.is_empty() {
        return wrap_with_highlight(scene, color);
    }
    descend_and_wrap(scene, &remaining, color)
}

/// R676 §5.16 §5.49 — recursive descent helper for
/// [`rebuild_with_highlight_at_path`]. Walks `segments` one step at a
/// time, descending into the matching child by-value and rebuilding
/// the container with the rewritten child slot. Returns the scene
/// unchanged on any soft-fail (non-Container current node, malformed
/// segment, child not found).
///
/// The nth-of-type counting **exactly mirrors** the forward walker's
/// per-type increment so a path produced by [`scene_to_tree_item`]
/// resolves to the same node on rebuild as it does on
/// [`find_main_node_at_path`] lookup.
fn descend_and_wrap(scene: Scene, segments: &[&str], color: Color) -> Scene {
    let Scene::Container(mut c) = scene else {
        // Non-Container variants are leaves — can't descend into
        // them. Return the original variant unchanged so the rest of
        // the scene paints normally (the selection path is stale).
        return scene;
    };
    let Some(first_segment) = segments.first() else {
        // Defensive — the caller guarantees segments.len() >= 1
        // before invoking this helper. Empty slice = no-op.
        return Scene::Container(c);
    };
    let Some((ty, Some(disambiguator))) = parse_path_segment(first_segment) else {
        // Malformed non-root segment (bare type or unparseable
        // bracket) → no match → return container unchanged.
        return Scene::Container(c);
    };
    let matching_idx: Option<usize> = match &disambiguator {
        PathDisambiguator::Tag(tag) => c.children.iter().position(|child| {
            scene_type_name(child) == ty && scene_tag(child) == Some(*tag)
        }),
        PathDisambiguator::NthOfType(idx) => {
            let mut count = 0_usize;
            c.children.iter().position(|child| {
                if scene_type_name(child) == ty {
                    if count == *idx {
                        return true;
                    }
                    count += 1;
                }
                false
            })
        }
    };
    if let Some(child_idx) = matching_idx {
        // `mem::replace` swaps the matching child out for a transient
        // empty Container, transforms the extracted child, then puts
        // the (now-wrapped) result back at the same slot. This is the
        // canonical Rust idiom for owning-mutating a `Vec` entry
        // through a `&mut Container` reference.
        let extracted = std::mem::replace(
            &mut c.children[child_idx],
            Scene::Container(ContainerNode::new(vec![])),
        );
        c.children[child_idx] = if segments.len() == 1 {
            wrap_with_highlight(extracted, color)
        } else {
            descend_and_wrap(extracted, &segments[1..], color)
        };
    }
    Scene::Container(c)
}

/// R671 §5.16 / R676 §5.16 §5.49 — walk one `Scene` node into a
/// [`TreeItem`] carrying a **path-stable** id. Containers become
/// branches whose `label` shows the variant + tag (when tagged); text
/// nodes become leaves carrying the rendered content; every other
/// variant carries a minimal "variant name" leaf.
///
/// `path` is the accumulated path string identifying this node within
/// the parent tree. The root caller passes [`scene_root_path_segment`]
/// of the scene (`"Container"` for an untagged root); recursive calls
/// append `/{Type[disambiguator]}` per child, where the disambiguator
/// is the child's tag when tag-bearing, or its nth-of-type sibling
/// index otherwise. The path scheme mirrors Browser DevTools' Elements
/// panel canonical (CSS-selector tag-or-nth-of-type) so a single
/// tagged element (like `main_btn`) keeps the same path regardless of
/// untagged sibling shape changes around it (e.g. R675's
/// banner-on/off case where adding a `Text` sibling above the button
/// previously shifted the button's sibling index from `0` to `1` and
/// corrupted the [`use_selected_path`] Signal across paint cycles).
///
/// All branches default to `expanded = true` so the inspector shows
/// the full main scene tree the moment it boots; collapse + multi-
/// select are 2nd-consumer carries
/// per [[abstraction-needs-second-consumer]].
fn scene_to_tree_item(scene: &Scene, path: &str) -> TreeItem {
    match scene {
        Scene::Container(c) => {
            let tag_segment = c
                .tag
                .as_deref()
                .map_or(String::new(), |t| format!(" [{t}]"));
            let label = format!("Container{tag_segment}");
            let mut type_counts: HashMap<&'static str, usize> = HashMap::new();
            let children: Vec<TreeItem> = c
                .children
                .iter()
                .map(|child| {
                    let ty = scene_type_name(child);
                    let nth_ref = type_counts.entry(ty).or_insert(0);
                    let nth = *nth_ref;
                    *nth_ref += 1;
                    let segment = scene_child_path_segment(child, nth);
                    let child_path = format!("{path}/{segment}");
                    scene_to_tree_item(child, &child_path)
                })
                .collect();
            TreeItem::branch(path.to_owned(), label, true, children)
        }
        Scene::Text(t) => {
            let label = format!("Text: {:?}", t.content);
            TreeItem::leaf(path.to_owned(), label)
        }
        Scene::External(e) => {
            let tag_segment = e
                .tag
                .as_deref()
                .map_or(String::new(), |t| format!(" [{t}]"));
            TreeItem::leaf(path.to_owned(), format!("External{tag_segment}"))
        }
        Scene::Box(_) => TreeItem::leaf(path.to_owned(), "Box"),
        Scene::Path(_) => TreeItem::leaf(path.to_owned(), "Path"),
        Scene::Image(_) => TreeItem::leaf(path.to_owned(), "Image"),
        Scene::Effect(_) => TreeItem::leaf(path.to_owned(), "Effect"),
        Scene::Scroll(_) => TreeItem::leaf(path.to_owned(), "Scroll"),
        // `pinion_core::Scene` is `#[non_exhaustive]`; any future
        // variant lands here as an opaque leaf so the inspector
        // never panics on an unknown node.
        _ => TreeItem::leaf(path.to_owned(), "(unknown variant)"),
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
        vec![ExtraExternal::new(
            INSPECTOR_TREE_TAG,
            Box::new(TreeRowClickExternal::new()),
        )]
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
    /// [`use_selected_path`] signal. Side-effect-only — empty
    /// `Vec<Command>` return; the `Signal::set` write is the
    /// mutation. Both windows' view fns observe the Signal change
    /// on their next paint cycle (per the substrate's reactive
    /// any-animation-active redraw wire).
    fn update(
        _state: Self::State,
        intent: &Intent,
    ) -> Vec<pinion_core::command::Command> {
        if intent.tag_str() == INSPECTOR_CLICK_INTENT_TAG
            && let IntrospectValue::Text(path) = &intent.payload
        {
            use_selected_path().set(Some(path.clone()));
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

    // R675 §5.16 §5.20 §5.45 §5.49 — cross-window selection bridge
    // regression suite. Pins the substrate plumbing (selected_path
    // shared Signal, click intent dotted form lockstep, banner
    // appears in main view only when path is Some) so a future
    // regression that drops one wire surfaces at unit-test time.

    #[test]
    fn r675_create_extra_externals_registers_tree_row_click_at_inspector_tag() {
        // The single ExtraExternal entry routes inspector composite-
        // tag clicks through the substrate router. Pin both the
        // length (no accidental extras) and the tag.
        let extras = <MultiWindowView as WidgetCore>::create_extra_externals();
        assert_eq!(extras.len(), 1, "exactly one tree-row click router");
        assert_eq!(extras[0].tag, INSPECTOR_TREE_TAG);
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
    fn r675_update_reducer_ignores_unrelated_intent_tags() {
        let owner = Owner::new();
        owner.run(|| {
            let foreign = Intent::new_static(
                "main_btn.click",
                IntrospectValue::Null,
            );
            let _ = <MultiWindowView as WidgetCore>::update(ButtonState::Idle, &foreign);
            assert!(
                use_selected_path().get().is_none(),
                "non-inspector intents must not mutate the selection slot",
            );
        });
    }
}

#[cfg(test)]
mod r676_path_stable_indexing_tests {
    //! R676 §5.16 §5.49 — path-stable indexing migration regression
    //! suite. The pre-R676 [`scene_to_tree_item`] walker built TreeItem
    //! ids by appending the raw sibling index (`"0/2/1"`); R675's
    //! banner-on/off case (selection → banner appears → button's
    //! sibling index shifts from 0 to 1 → `"0/0"` now points to the
    //! banner instead of the button) corrupted [`use_selected_path`]
    //! across paint cycles. R676 rewrites the walker to the
    //! Browser-DevTools-canonical CSS-selector form
    //! `Type[tag-or-nth-of-type]` so tagged elements (`main_btn`) keep
    //! the same path regardless of untagged-sibling churn. These tests
    //! pin (a) the helper functions' shapes and (b) the full-walk
    //! invariant that the button path is stable across banner toggle.
    use super::*;
    use pinion_core::external::StubExternal;
    use pinion_core::scene::{ContainerNode, ExternalNode, Rect, TextNode};
    use pinion_core::style::TextStyle;

    /// R676 helper — minimal tagged External fixture for path-scheme
    /// tests. `StubExternal` is the canonical no-op `External` from
    /// `pinion_core::external` (Gui backend, framework-driven repaint,
    /// UI-thread sync) — picked here because it implements `External`
    /// without dragging in widget-specific state.
    fn stub_external_with_tag(tag: &'static str) -> Scene {
        Scene::External(
            ExternalNode::new(Box::new(StubExternal::new())).with_tag(tag),
        )
    }

    /// R676 helper — minimal untagged External fixture for the
    /// nth-of-type fallback test.
    fn stub_external_untagged() -> Scene {
        Scene::External(ExternalNode::new(Box::new(StubExternal::new())))
    }

    /// (R676) Root segment for an untagged Container is the bare type
    /// name (no bracket) — root has no siblings to disambiguate
    /// against, so the nth-of-type fallback is omitted.
    #[test]
    fn r676_root_segment_for_untagged_container_is_bare_type_name() {
        let scene = Scene::Container(ContainerNode::new(vec![]));
        assert_eq!(scene_root_path_segment(&scene), "Container");
    }

    /// (R676) Root segment for a tagged Container carries the tag
    /// bracket — even the root keeps stable element-id semantics when
    /// it carries one.
    #[test]
    fn r676_root_segment_for_tagged_container_uses_tag_form() {
        let scene =
            Scene::Container(ContainerNode::new(vec![]).with_tag(MAIN_BTN_TAG));
        assert_eq!(
            scene_root_path_segment(&scene),
            format!("Container[{MAIN_BTN_TAG}]"),
        );
    }

    /// (R676) Child segment for a tagged Container uses the tag form
    /// regardless of the nth-of-type index (tags take priority).
    #[test]
    fn r676_child_segment_for_tagged_container_ignores_nth_of_type() {
        let scene =
            Scene::Container(ContainerNode::new(vec![]).with_tag(MAIN_BTN_TAG));
        // nth = 7 is intentionally arbitrary — tags take priority so
        // the bracket carries the tag regardless of sibling order.
        assert_eq!(
            scene_child_path_segment(&scene, 7),
            format!("Container[{MAIN_BTN_TAG}]"),
        );
    }

    /// (R676) Child segment for an untagged Text uses the
    /// `Text[<nth-of-type>]` form. The nth index counts Text siblings
    /// only (not all siblings) — see
    /// `r676_mixed_type_siblings_have_per_type_idx` for the cross-type
    /// counting case.
    #[test]
    fn r676_child_segment_for_untagged_text_uses_type_idx_form() {
        let text = Scene::Text(TextNode::styled(
            "x",
            Rect::default(),
            TextStyle::new(),
        ));
        assert_eq!(scene_child_path_segment(&text, 0), "Text[0]");
        assert_eq!(scene_child_path_segment(&text, 3), "Text[3]");
    }

    /// (R676) **The R675 architectural fix.** With banner off, the
    /// button is the root Container's first (and only) child; with
    /// banner on, a `Selected: …` Text precedes the button. The pre-
    /// R676 walker produced `"0/0"` for the button when the banner
    /// was off and `"0/1"` for the same button when the banner was on
    /// (sibling-index drift). The R676 walker produces the same
    /// `"Container/Container[main_btn]"` in both cases because the
    /// path keys off the tag, not the sibling index.
    #[test]
    fn r676_button_path_stable_across_banner_toggle() {
        let owner = Owner::new();
        // Banner off — fresh Owner, selection slot empty.
        let scene_no_banner =
            owner.run(|| view_main(ButtonState::Idle));
        let root_path_no_banner = scene_root_path_segment(&scene_no_banner);
        let tree_no_banner =
            scene_to_tree_item(&scene_no_banner, &root_path_no_banner);
        let button_path_no_banner =
            find_first_tree_item_with_tag(&tree_no_banner, MAIN_BTN_TAG)
                .expect("banner-off tree must include the button row");

        // Banner on — seed the selection signal.
        owner.run(|| {
            use_selected_path().set(Some("seed".to_string()));
        });
        let scene_with_banner = owner.run(|| view_main(ButtonState::Idle));
        let root_path_with_banner =
            scene_root_path_segment(&scene_with_banner);
        let tree_with_banner =
            scene_to_tree_item(&scene_with_banner, &root_path_with_banner);
        let button_path_with_banner =
            find_first_tree_item_with_tag(&tree_with_banner, MAIN_BTN_TAG)
                .expect("banner-on tree must still include the button row");

        assert_eq!(
            button_path_no_banner, button_path_with_banner,
            "button path must be stable across banner toggle — R675 \
             architectural debt fix; pre-R676 produced \"0/0\" vs \"0/1\"",
        );
        // And the actual textbook canonical shape — tag-form path
        // joined to the bare root segment by `/`.
        assert!(
            button_path_with_banner
                .ends_with(&format!("Container[{MAIN_BTN_TAG}]")),
            "button path must end with the tagged form Container[{MAIN_BTN_TAG}]; \
             got: {button_path_with_banner:?}",
        );
    }

    /// (R676) Multiple Text siblings get distinct nth-of-type indices
    /// (`Text[0]`, `Text[1]`, `Text[2]`) — the canonical CSS-selector
    /// disambiguation for anonymous repeated children.
    #[test]
    fn r676_sibling_text_nodes_get_distinct_nth_of_type_idx() {
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Text(TextNode::styled("a", Rect::default(), TextStyle::new())),
            Scene::Text(TextNode::styled("b", Rect::default(), TextStyle::new())),
            Scene::Text(TextNode::styled("c", Rect::default(), TextStyle::new())),
        ]));
        let root_path = scene_root_path_segment(&scene);
        let tree = scene_to_tree_item(&scene, &root_path);
        let TreeItem { children, .. } = tree;
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].id, "Container/Text[0]");
        assert_eq!(children[1].id, "Container/Text[1]");
        assert_eq!(children[2].id, "Container/Text[2]");
    }

    /// (R676) **The cross-type nth-of-type counting case.** A parent
    /// with mixed-type children — `[Text, Container[main_btn], Text,
    /// External[grid]]` — assigns `Text[0]` and `Text[1]` (not
    /// `Text[0]` and `Text[2]`) because the index counts siblings of
    /// the same Scene variant only. Tagged children use the tag form
    /// regardless of how many other tagged children sit beside them.
    #[test]
    fn r676_mixed_type_siblings_have_per_type_idx() {
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Text(TextNode::styled("a", Rect::default(), TextStyle::new())),
            Scene::Container(ContainerNode::new(vec![]).with_tag(MAIN_BTN_TAG)),
            Scene::Text(TextNode::styled("b", Rect::default(), TextStyle::new())),
            stub_external_with_tag("grid"),
        ]));
        let root_path = scene_root_path_segment(&scene);
        let tree = scene_to_tree_item(&scene, &root_path);
        let TreeItem { children, .. } = tree;
        assert_eq!(children.len(), 4);
        assert_eq!(children[0].id, "Container/Text[0]");
        assert_eq!(
            children[1].id,
            format!("Container/Container[{MAIN_BTN_TAG}]"),
        );
        assert_eq!(
            children[2].id, "Container/Text[1]",
            "second Text gets nth-of-type idx 1 — Container[main_btn] in \
             between does not consume the Text counter",
        );
        assert_eq!(children[3].id, "Container/External[grid]");
    }

    /// (R676) Nested 3-level path round-trip — the canonical example
    /// from the seed prompt: `Container/Container[main_btn]/Text[0]`
    /// for a labelled button inside an untagged outer Container.
    #[test]
    fn r676_nested_path_three_levels_matches_seed_example() {
        let label_text = Scene::Text(TextNode::styled(
            "Click me!",
            Rect::default(),
            TextStyle::new(),
        ));
        let button = Scene::Container(
            ContainerNode::new(vec![label_text]).with_tag(MAIN_BTN_TAG),
        );
        let root = Scene::Container(ContainerNode::new(vec![button]));
        let root_path = scene_root_path_segment(&root);
        let tree = scene_to_tree_item(&root, &root_path);

        // Walk down — root → button → label.
        let TreeItem {
            id: root_id,
            children: root_children,
            ..
        } = tree;
        assert_eq!(root_id, "Container");
        assert_eq!(root_children.len(), 1);
        let TreeItem {
            id: button_id,
            children: button_children,
            ..
        } = &root_children[0];
        assert_eq!(button_id, &format!("Container/Container[{MAIN_BTN_TAG}]"));
        assert_eq!(button_children.len(), 1);
        assert_eq!(
            button_children[0].id,
            format!("Container/Container[{MAIN_BTN_TAG}]/Text[0]"),
            "deepest label path must match the seed-prompt canonical example",
        );
    }

    /// (R676) Tagged External uses the tag form (`External[grid]`)
    /// not the nth-of-type form. Mirrors the Container tagged-path
    /// branch; pins the `scene_tag` helper covers both tag-bearing
    /// variants.
    #[test]
    fn r676_tagged_external_path_uses_tag_form() {
        let scene = Scene::Container(ContainerNode::new(vec![
            stub_external_with_tag("grid"),
        ]));
        let root_path = scene_root_path_segment(&scene);
        let tree = scene_to_tree_item(&scene, &root_path);
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].id, "Container/External[grid]");
    }

    /// (R676) Untagged External falls back to the nth-of-type form
    /// (`External[0]`) — symmetric with Container.
    #[test]
    fn r676_untagged_external_path_uses_type_idx_form() {
        let scene = Scene::Container(ContainerNode::new(vec![
            stub_external_untagged(),
        ]));
        let root_path = scene_root_path_segment(&scene);
        let tree = scene_to_tree_item(&scene, &root_path);
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].id, "Container/External[0]");
    }

    /// (R676) `scene_type_name` returns the exhaustive set of literal
    /// type names that appear in path segments. Pins the mapping so a
    /// future variant addition to `pinion_core::Scene` surfaces a
    /// compile-time path-scheme decision (the `_ => "Unknown"` arm
    /// quietly catches the new variant; this test flags that a real
    /// type-name should be picked instead).
    #[test]
    fn r676_scene_type_name_covers_known_variants() {
        assert_eq!(
            scene_type_name(&Scene::Container(ContainerNode::new(vec![]))),
            "Container",
        );
        assert_eq!(
            scene_type_name(&Scene::Text(TextNode::styled(
                "x",
                Rect::default(),
                TextStyle::new(),
            ))),
            "Text",
        );
        assert_eq!(
            scene_type_name(&stub_external_untagged()),
            "External",
        );
    }

    /// R676 helper — depth-first scan for the first `TreeItem` whose
    /// id ends with `Container[<tag>]` (the canonical tag-form
    /// suffix). Returns the full path string of the matching node, or
    /// `None`. Test-only utility (kept in this module rather than the
    /// production code) — production paths go through the R676
    /// atomic (1) `find_main_node_at_path` instead.
    fn find_first_tree_item_with_tag(
        item: &TreeItem,
        tag: &str,
    ) -> Option<String> {
        let suffix = format!("Container[{tag}]");
        if item.id.ends_with(&suffix) {
            return Some(item.id.clone());
        }
        item.children
            .iter()
            .find_map(|child| find_first_tree_item_with_tag(child, tag))
    }
}

#[cfg(test)]
mod r676_find_node_at_path_tests {
    //! R676 §5.16 §5.49 — inverse-walker regression suite. Atomic (1)
    //! pairs with the atomic (0) forward walker: given a path string
    //! produced by [`scene_to_tree_item`], [`find_main_node_at_path`]
    //! returns the matching `&Scene` borrow (or `None` for paths that
    //! cannot resolve against the current scene shape). Tests pin
    //! both the happy path (tagged + nth-of-type + nested) and the
    //! soft-signal `None` returns (inspector-only ids, stale
    //! disambiguators, malformed segments) so the upcoming atomic (2)
    //! highlight overlay can rely on graceful no-match behaviour.
    use super::*;
    use pinion_core::scene::{ContainerNode, Rect, TextNode};
    use pinion_core::style::TextStyle;

    /// (R676) `parse_path_segment` returns the bare-type form for
    /// segments without brackets — only valid at the root.
    #[test]
    fn r676_parse_segment_bare_type_returns_disambiguator_none() {
        let (ty, disambiguator) = parse_path_segment("Container").unwrap();
        assert_eq!(ty, "Container");
        assert!(disambiguator.is_none());
    }

    /// (R676) `parse_path_segment` returns the tag form when the
    /// disambiguator string fails `usize::from_str`. Tags are
    /// identifier-shaped in pinion's paint code (never pure digits) so
    /// the from_str fallback is reliable.
    #[test]
    fn r676_parse_segment_tag_form_returns_tag_disambiguator() {
        let (ty, disambiguator) = parse_path_segment("Container[main_btn]").unwrap();
        assert_eq!(ty, "Container");
        assert_eq!(
            disambiguator,
            Some(PathDisambiguator::Tag("main_btn")),
        );
    }

    /// (R676) `parse_path_segment` returns the nth-of-type form when
    /// the disambiguator string is numeric. Mirrors the forward
    /// walker's index-based fallback for untagged siblings.
    #[test]
    fn r676_parse_segment_nth_of_type_form_returns_idx_disambiguator() {
        let (ty, disambiguator) = parse_path_segment("Text[0]").unwrap();
        assert_eq!(ty, "Text");
        assert_eq!(disambiguator, Some(PathDisambiguator::NthOfType(0)));
        let (ty, disambiguator) = parse_path_segment("Text[42]").unwrap();
        assert_eq!(ty, "Text");
        assert_eq!(disambiguator, Some(PathDisambiguator::NthOfType(42)));
    }

    /// (R676) `parse_path_segment` returns None for a malformed open
    /// bracket without a closing `]`. The inverse walker propagates
    /// the None as a soft-signal "do not resolve".
    #[test]
    fn r676_parse_segment_malformed_open_bracket_returns_none() {
        assert!(parse_path_segment("Container[").is_none());
        assert!(parse_path_segment("Container[main_btn").is_none());
    }

    /// (R676) Single-segment resolution: bare-type root resolves to
    /// the root scene itself (no descent).
    #[test]
    fn r676_find_root_with_bare_type_segment_returns_root() {
        let scene = Scene::Container(ContainerNode::new(vec![]));
        let found = find_main_node_at_path(&scene, "Container").unwrap();
        // Pointer equality through the variant — both should be the
        // same `Scene::Container` instance.
        assert!(matches!(found, Scene::Container(_)));
    }

    /// (R676) Single-segment root resolution with a tagged root —
    /// `Container[some_tag]` matches an untagged root → None per the
    /// soft-signal contract.
    #[test]
    fn r676_find_tagged_root_segment_against_untagged_root_returns_none() {
        let scene = Scene::Container(ContainerNode::new(vec![]));
        assert!(find_main_node_at_path(&scene, "Container[main_btn]").is_none());
    }

    /// (R676) Root with nth-of-type disambiguator is malformed
    /// (root has no siblings) — returns None.
    #[test]
    fn r676_find_root_with_nth_of_type_disambiguator_returns_none() {
        let scene = Scene::Container(ContainerNode::new(vec![]));
        assert!(find_main_node_at_path(&scene, "Container[0]").is_none());
    }

    /// (R676) Type mismatch at root — looking for a `Text` root in a
    /// `Container` root returns None.
    #[test]
    fn r676_find_root_type_mismatch_returns_none() {
        let scene = Scene::Container(ContainerNode::new(vec![]));
        assert!(find_main_node_at_path(&scene, "Text").is_none());
    }

    /// (R676) Find tagged child two levels deep — the canonical
    /// happy path. Mirrors the R675 banner-on selection flow:
    /// inspector emits the tagged path `Container/Container[main_btn]`,
    /// the highlight overlay (atomic 2) resolves it back to the
    /// button container.
    #[test]
    fn r676_find_tagged_child_returns_matching_scene() {
        let button = Scene::Container(
            ContainerNode::new(vec![]).with_tag(MAIN_BTN_TAG),
        );
        let scene = Scene::Container(ContainerNode::new(vec![button]));
        let found = find_main_node_at_path(
            &scene,
            &format!("Container/Container[{MAIN_BTN_TAG}]"),
        )
        .expect("tagged-form child path must resolve");
        match found {
            Scene::Container(c) => assert_eq!(c.tag.as_deref(), Some(MAIN_BTN_TAG)),
            _ => panic!("expected Scene::Container at the resolved path"),
        }
    }

    /// (R676) Find untagged child by nth-of-type — pins the
    /// alternative resolution path (when no tag is present).
    #[test]
    fn r676_find_untagged_child_by_nth_of_type_returns_matching_scene() {
        let text_a = Scene::Text(TextNode::styled("a", Rect::default(), TextStyle::new()));
        let text_b = Scene::Text(TextNode::styled("b", Rect::default(), TextStyle::new()));
        let scene =
            Scene::Container(ContainerNode::new(vec![text_a, text_b]));
        let first = find_main_node_at_path(&scene, "Container/Text[0]")
            .expect("Text[0] must resolve to first text child");
        let second = find_main_node_at_path(&scene, "Container/Text[1]")
            .expect("Text[1] must resolve to second text child");
        match first {
            Scene::Text(t) => assert_eq!(t.content, "a"),
            _ => panic!("expected Scene::Text at Text[0]"),
        }
        match second {
            Scene::Text(t) => assert_eq!(t.content, "b"),
            _ => panic!("expected Scene::Text at Text[1]"),
        }
    }

    /// (R676) Find nested 3-deep — the canonical seed-prompt example
    /// `Container/Container[main_btn]/Text[0]` resolves to the button
    /// label.
    #[test]
    fn r676_find_nested_three_deep_returns_label_text() {
        let label = Scene::Text(TextNode::styled(
            "Click me!",
            Rect::default(),
            TextStyle::new(),
        ));
        let button = Scene::Container(
            ContainerNode::new(vec![label]).with_tag(MAIN_BTN_TAG),
        );
        let scene = Scene::Container(ContainerNode::new(vec![button]));
        let found = find_main_node_at_path(
            &scene,
            &format!("Container/Container[{MAIN_BTN_TAG}]/Text[0]"),
        )
        .expect("3-deep tagged-then-text path must resolve");
        match found {
            Scene::Text(t) => assert_eq!(t.content, "Click me!"),
            _ => panic!("expected Scene::Text at the deepest segment"),
        }
    }

    /// (R676) Unknown tag returns None — the soft-signal contract.
    #[test]
    fn r676_find_unknown_tag_returns_none() {
        let button = Scene::Container(
            ContainerNode::new(vec![]).with_tag(MAIN_BTN_TAG),
        );
        let scene = Scene::Container(ContainerNode::new(vec![button]));
        assert!(
            find_main_node_at_path(&scene, "Container/Container[ghost]").is_none(),
            "unknown tag in path must return None, not panic",
        );
    }

    /// (R676) Out-of-range nth-of-type returns None.
    #[test]
    fn r676_find_nth_of_type_out_of_range_returns_none() {
        let scene = Scene::Container(ContainerNode::new(vec![Scene::Text(
            TextNode::styled("x", Rect::default(), TextStyle::new()),
        )]));
        assert!(
            find_main_node_at_path(&scene, "Container/Text[99]").is_none(),
            "out-of-range nth-of-type idx must return None",
        );
    }

    /// (R676) **The inspector-only TreeItem id case.** The
    /// inspector's `TreeItem::leaf("state", …)` produces composite
    /// row tag `inspector_tree#state`; clicks emit selected_path =
    /// `"state"`. That string starts with the "Type" position but
    /// `state` is not a Scene-variant type name → `scene_type_name`
    /// of the root is `"Container"` ≠ `"state"` → None. The atomic
    /// (2) highlight overlay reads this None as "no main-window node
    /// to wrap" and paints the banner only.
    #[test]
    fn r676_find_inspector_only_id_state_returns_none() {
        let scene = Scene::Container(ContainerNode::new(vec![]));
        assert!(
            find_main_node_at_path(&scene, "state").is_none(),
            "inspector-only TreeItem id must not resolve in main scene",
        );
        assert!(
            find_main_node_at_path(&scene, "main").is_none(),
            "inspector branch id must not resolve in main scene",
        );
    }

    /// (R676) Bare-type non-root segment is malformed (forward walker
    /// never emits one) → None.
    #[test]
    fn r676_find_bare_type_non_root_segment_returns_none() {
        let inner = Scene::Container(ContainerNode::new(vec![]));
        let scene = Scene::Container(ContainerNode::new(vec![inner]));
        assert!(
            find_main_node_at_path(&scene, "Container/Container").is_none(),
            "non-root segment without bracket disambiguator must return None",
        );
    }

    /// (R676) Descending into a leaf variant returns None — `Text`
    /// has no children, so a path requesting `Container/Text[0]/Foo`
    /// can't continue.
    #[test]
    fn r676_find_descend_into_leaf_returns_none() {
        let scene = Scene::Container(ContainerNode::new(vec![Scene::Text(
            TextNode::styled("x", Rect::default(), TextStyle::new()),
        )]));
        assert!(
            find_main_node_at_path(&scene, "Container/Text[0]/Container[x]").is_none(),
            "descending into a non-Container leaf must return None",
        );
    }

    /// (R676) **Build / resolve round-trip — the R675 architectural
    /// fix proof.** Build a path from view_main(banner=off) via the
    /// forward walker; resolve the same path against
    /// view_main(banner=on) via the inverse walker; both must point
    /// to the button Container. Cross-paint-cycle path stability is
    /// what unlocks the atomic (2) highlight overlay being robust to
    /// state mutations.
    #[test]
    fn r676_build_then_resolve_button_stable_across_banner_toggle() {
        let owner = Owner::new();
        // Banner off — derive button path via the forward walker.
        let scene_no_banner = owner.run(|| view_main(ButtonState::Idle));
        let root_path = scene_root_path_segment(&scene_no_banner);
        let button_path = walk_for_button_path(
            &scene_to_tree_item(&scene_no_banner, &root_path),
        )
        .expect("forward walker must produce a button path on banner-off");

        // Banner on — selection slot triggers banner. Same button
        // path must still resolve to the button container.
        owner.run(|| {
            use_selected_path().set(Some("seed".to_string()));
        });
        let scene_with_banner = owner.run(|| view_main(ButtonState::Idle));
        let found = find_main_node_at_path(&scene_with_banner, &button_path)
            .expect("same path must resolve in banner-on scene — R675 fix");
        match found {
            Scene::Container(c) => assert_eq!(c.tag.as_deref(), Some(MAIN_BTN_TAG)),
            _ => panic!("resolved node must be the button container"),
        }

        // And banner off again — symmetric back-resolution.
        owner.run(|| {
            use_selected_path().set(None);
        });
        let scene_again = owner.run(|| view_main(ButtonState::Idle));
        let found_again = find_main_node_at_path(&scene_again, &button_path)
            .expect("banner-off → on → off cycle preserves path resolution");
        match found_again {
            Scene::Container(c) => assert_eq!(c.tag.as_deref(), Some(MAIN_BTN_TAG)),
            _ => panic!("resolved node must remain the button container"),
        }
    }

    /// Test-only helper — depth-first walk for the first TreeItem
    /// whose id ends with the tagged-form path for `MAIN_BTN_TAG`.
    /// Mirrors the helper in `r676_path_stable_indexing_tests` but
    /// scoped to this module to keep the inverse-walker tests
    /// self-contained.
    fn walk_for_button_path(item: &TreeItem) -> Option<String> {
        let suffix = format!("Container[{MAIN_BTN_TAG}]");
        if item.id.ends_with(&suffix) {
            return Some(item.id.clone());
        }
        item.children
            .iter()
            .find_map(walk_for_button_path)
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
