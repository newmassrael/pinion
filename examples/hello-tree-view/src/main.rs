// R673 §5.16 — example bindings tolerate looser doc-markdown lints
// than substrate crates. The example narrative carries a lot of
// proper-noun identifiers (`TreeView`, `DevTools`, etc.) inside
// architectural doc comments; pushing every one through backticks
// inflates the prose for negligible signal gain.
#![allow(clippy::doc_markdown)]

//! `hello-tree-view` — R673 §5.16 §5.50 second consumer of the
//! `pinion_widget_paint::tree_view` substrate (the R671 first
//! consumer was the read-only `hello-multi-window` inspector).
//!
//! ## Why this binding exists
//!
//! Per [[abstraction-needs-second-consumer]], framework substrates
//! land their canonical maturity at the second consumer. R671 lifted
//! the TreeView paint helper for a single consumer (the inspector
//! tree mirroring main's paint scene) — read-only, no keyboard model,
//! no focus highlight. R673 adds the second consumer to verify the
//! substrate scales: an interactive keyboard-driven hierarchical
//! browser with Arrow Up/Down/Left/Right navigation, Home/End jump
//! to first/last visible row, Space/Enter to toggle expand on the
//! focused branch, and a Material 3 focused-row state-layer overlay
//! that follows the keyboard cursor.
//!
//! ## Keyboard model (WAI-ARIA 1.2 Tree, APG 6.13 — completed at R809)
//!
//! R673 shipped the arrow skeleton with two declared *future axes*
//! (Right-descends-into-child / Left-ascends-to-parent) and a
//! non-canonical wrap-at-edges Up/Down. R809 completes the WAI-ARIA
//! APG Tree keyboard contract and routes the vertical axis through the
//! shared `clamp_nav` SSOT (clamp, not wrap — a tree has ends), so the
//! tree now navigates byte-identically to the virtualized list / grid:
//!
//! - Arrow Up / Down — move focus to the previous / next visible row.
//!   **Clamp at the ends** (no wrap), via
//!   [`clamp_nav`](pinion_core::widgets::virtual_select::clamp_nav) —
//!   the same finite-collection policy `hello-virtual-nav` reuses.
//! - Page Up / Page Down — jump a viewport-ful of rows
//!   ([`NAV_PAGE`]), clamped, also through `clamp_nav`.
//! - Arrow Right — on a *collapsed* branch, expand it; on an
//!   *already-expanded* branch, move focus to its **first child**; a
//!   no-op on leaves (WAI-ARIA APG 6.13 expand-or-descend).
//! - Arrow Left — on an *expanded* branch, collapse it; on a
//!   *collapsed* branch or a leaf, move focus to its **parent**; a
//!   no-op at a root with no parent (WAI-ARIA APG 6.13
//!   collapse-or-ascend).
//! - Home — focus the first visible row; End — the last.
//! - Space / Enter — toggle expanded on the focused branch (no-op
//!   on leaves).
//! - A printable character — **type-ahead** to the next visible row
//!   whose label matches, via the
//!   [`pinion_shell::typeahead`] substrate
//!   (this binding is the `TreeView` consumer that substrate's module
//!   doc named as a future taker). Single-char taps cycle; multi-char
//!   within [`TYPEAHEAD_TIMEOUT`](pinion_shell::typeahead::TYPEAHEAD_TIMEOUT)
//!   match a growing prefix (WAI-ARIA APG text-search algorithm).
//!
//! ## Why no new substrate (R809)
//!
//! The arrow / page vertical motion delegates to the existing
//! `clamp_nav` SSOT and type-ahead delegates to the existing
//! `typeahead` substrate — R809 adds *no* framework code, only
//! consumes two SSOTs the family already owns. The tree-specific
//! pieces (the visible-row flattening, the Right-descend / Left-ascend
//! traversal) stay inline here: `hello-tree-view` is still the *only*
//! tree-keyboard consumer, so per [[abstraction-needs-second-consumer]]
//! a `TreeNav` substrate waits for the second interactive tree
//! (file-tree editor, property grid, scene outliner).
//!
//! ## Non-goals
//!
//! Multi-select, drag-drop, inline rename: all deferred to future
//! consumers per [[abstraction-needs-second-consumer]]. Click-to-toggle
//! is already wired (R674 [`TreeRowClickExternal`]).

use pinion_a11y::{AccessFocus, AccessNode, WidgetA11y, tree_access_nodes, tree_row_tag};
use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::widgets::tree_nav::{
    TreeNode, apply_tree_key, flat_visible, toggle_expanded, tree_view_introspection_extra,
};
use pinion_core::{Frame, Owner, Scene, Signal, WidgetCore};
use pinion_shell::typeahead::tree_typeahead_jump;
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::tree_view::{
    TreeItem, TreeRowClickExternal, TreeViewFocus, TreeViewStyle, view_tree_focused,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloTreeViewRenderer, HelloTreeViewRendererError);

const TREE_TAG: &str = "file_tree";
const ROOT_BTN_TAG: &str = "tree_root";
const THEME_TAG: &str = "app";
/// R823 §5.50 §5.12 — query-only tree-state introspection External
/// ([`tree_view_introspection_extra`]). Surfaces the tree's structure +
/// cursor (`row_count` / `cursor` / `cursor_index` / `id_at` / `label_at`
/// / `level_at` / `expanded_at`) to `scene/query`, so an AI client reads
/// the tree as data instead of scraping the paint scene.
const TREE_STATE_TAG: &str = "file_tree_state";

const WIN_W: u32 = 480;
const WIN_H: u32 = 400;

/// R809 §5.50 — Page Up / Page Down jump size, in visible rows. The
/// content viewport (window height minus the header / footer / padding
/// chrome) holds roughly `(400 - ~64) / 48 ≈ 7` M3 rows, so a
/// page-key advances a near-viewport-ful. `clamp_nav` clamps the
/// result to the listing's ends, so an over-shoot on a short tree just
/// lands on the first / last visible row.
const NAV_PAGE: usize = 7;

/// R674 §5.20 — dotted wire-form intent tag the `WidgetView::update`
/// reducer matches against for click-driven row toggles. Compile-time
/// concat of [`TREE_TAG`] and the substrate's `"click"` event name
/// via the [`intent_tag!`] macro so the literal stays in lockstep
/// with the substrate-emitting External + the View tag. (R675 §5.16 —
/// the External lifted to `pinion_widget_paint::tree_view::TreeRowClickExternal`,
/// the event name lifted to
/// [`pinion_widget_paint::tree_view::TREE_ROW_CLICK_EVENT`] = `"click"`;
/// the binding owns the dotted form because the `intent_tag!` macro
/// is `literal`-only at the stable-Rust layer.)
const FILE_TREE_CLICK_INTENT_TAG: &str = intent_tag!("file_tree", "click");

const HEADER_FONT_PX: u32 = 14;
const FOOTER_FONT_PX: u32 = 12;
const HEADER_BOTTOM_GAP: u32 = 4;
const FOOTER_TOP_GAP: u32 = 8;

/// R673 §5.50 — sample tree data. Three top-level branches (`src`,
/// `tests`, `docs`) each carrying a small child tree. The data lives
/// in a `Signal<Vec<FileNode>>` so `apply_key` mutations
/// (expand/collapse toggles) re-run the view fn deterministically.
/// `serde::Serialize` / `Deserialize` derive is required because
/// `pinion_core::Signal<T>` carries the §5.22 introspect bound on T;
/// the bound traces back to `pinion_core::reactive::Signal::set`'s
/// preview-snapshot path even when the binding never actually emits
/// preview frames against this signal.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct FileNode {
    id: String,
    label: String,
    expanded: bool,
    children: Vec<FileNode>,
}

impl FileNode {
    fn leaf<I: Into<String>, L: Into<String>>(id: I, label: L) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            expanded: false,
            children: Vec::new(),
        }
    }

    fn branch<I: Into<String>, L: Into<String>>(
        id: I,
        label: L,
        expanded: bool,
        children: Vec<FileNode>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            expanded,
            children,
        }
    }

    fn to_tree_item(&self) -> TreeItem {
        TreeItem::branch(
            self.id.clone(),
            self.label.clone(),
            self.expanded,
            self.children.iter().map(FileNode::to_tree_item).collect(),
        )
    }
}

/// R811 §5.50 §5.27 — drive the shared keyboard-navigation substrate
/// (`pinion_core::widgets::tree_nav`) over the retained `FileNode`
/// model. R809/R809.1's flat-walk + WAI-ARIA resolver were lifted to
/// the substrate at R811's second tree consumer; this binding now
/// supplies its tree through the trait rather than re-deriving the
/// flattening locally.
impl TreeNode for FileNode {
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

fn initial_nodes() -> Vec<FileNode> {
    vec![
        FileNode::branch(
            "src",
            "src",
            true,
            vec![
                FileNode::leaf("src/main.rs", "main.rs"),
                FileNode::leaf("src/lib.rs", "lib.rs"),
                FileNode::branch(
                    "src/widgets",
                    "widgets",
                    false,
                    vec![
                        FileNode::leaf("src/widgets/mod.rs", "mod.rs"),
                        FileNode::leaf("src/widgets/tree_view.rs", "tree_view.rs"),
                    ],
                ),
            ],
        ),
        FileNode::branch(
            "tests",
            "tests",
            false,
            vec![
                FileNode::leaf("tests/integration.rs", "integration.rs"),
                FileNode::leaf("tests/snapshot.rs", "snapshot.rs"),
            ],
        ),
        FileNode::branch(
            "docs",
            "docs",
            false,
            vec![FileNode::leaf("docs/README.md", "README.md")],
        ),
    ]
}

/// R673 §5.50 — reactive primitive holding the binding's mutable
/// tree state. `nodes` carries the data + per-node expanded flags;
/// `focused_id` carries the row id the keyboard cursor sits on.
/// Lifted into `Owner::cache` so every view-fn pass reads the same
/// shared instance.
fn use_tree_state() -> std::rc::Rc<TreeState> {
    Owner::current()
        .expect("hello-tree-view: view fn runs inside the substrate root owner scope")
        .cache("hello_tree_view_state", || TreeState {
            nodes: Signal::new(initial_nodes()),
            focused_id: Signal::new(Some(String::from("src"))),
        })
}

struct TreeState {
    nodes: Signal<Vec<FileNode>>,
    focused_id: Signal<Option<String>>,
}

fn view(state: ButtonState) -> Scene {
    let _ = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let surface = theme.resolve(ColorRole::Surface);
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let tree_state = use_tree_state();
    let nodes = tree_state.nodes.get();
    let focused = tree_state.focused_id.get();
    let items: Vec<TreeItem> = nodes.iter().map(FileNode::to_tree_item).collect();
    let focus = TreeViewFocus {
        focused_id: focused.as_deref(),
    };
    let tree_scene = view_tree_focused(
        TREE_TAG,
        &items,
        // (R1030.2 §5.39) This binding routes keyboard focus through the
        // invisible `tree_root` (apply_key gates on it); the painted tree
        // container must NOT also be a focus stop, or a row click would move
        // shell focus onto `file_tree` and the Space/Arrow gate would reject
        // keys. Opt the container out of the R1030 fail-safe default.
        &theme,
        &TreeViewStyle::m3_default().with_focusable(false),
        &focus,
    );

    let header = Scene::Text(TextNode::styled(
        "File explorer (TreeView demo)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(HEADER_FONT_PX)
            .with_fg(on_surface_muted),
    ));
    let footer = Scene::Text(TextNode::styled(
        "\u{2191}/\u{2193} navigate  \u{2192} expand/child  \u{2190} collapse/parent  \
         PgUp/PgDn page  A-Z jump  Space toggle  Home/End",
        Rect::default(),
        TextStyle::new()
            .with_size_px(FOOTER_FONT_PX)
            .with_fg(on_surface_muted),
    ));
    // R673 §5.50 — invisible Button widget at the scene root keeps
    // the framework's SCXML state surface alive for read_state /
    // forward / RPC introspect. The keyboard interactivity is driven
    // by [`apply_key`] which writes directly to the reactive
    // [`TreeState`] signals; the Button state stays at Idle/Hover
    // depending on the SCXML transition, but no visual button paints.
    let invisible_root = Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(ROOT_BTN_TAG)
            // (R1030 §5.39) The invisible root IS the tree's keyboard focus
            // stop (apply_key drives the tree off it). Hand-composed node, so
            // the composing view owns the focus opt-in.
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(0, 0))
                    .with_focusable(true),
            ),
    );

    let header_text = Scene::Text(TextNode::styled(
        "",
        Rect::default(),
        TextStyle::new()
            .with_size_px(HEADER_FONT_PX)
            .with_fg(on_surface),
    ));
    let _ = header_text;
    Scene::Container(
        ContainerNode::new(vec![header, tree_scene, footer, invisible_root])
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    // R673 §5.50 — Stretch so the TreeView fills the
                    // window's cross-axis width; rows then expand to
                    // the full width via the substrate's own
                    // `AlignItems::Stretch` on the tree root.
                    .with_align_items(AlignItems::Stretch)
                    .with_gap(HEADER_BOTTOM_GAP.max(FOOTER_TOP_GAP))
                    .with_padding(Rect::new(12, 12, 12, 12)),
            ),
    )
}

// R820 §5.27 §5.50 — the find / toggle / set-expanded flag-store glue
// that R674–R809 kept inline here was lifted to
// `pinion_core::widgets::tree_nav` (`find_node_mut` / `toggle_expanded` /
// `set_expanded_in`) when `hello-virtual-tree` became the 2nd retained
// flag-on-node consumer. The `apply_tree_key` bridge below replaces the
// R809 hand-rolled `resolve_tree_key` match; type-ahead stays caller-side
// per the substrate's purity boundary.

/// R809 §5.22 §5.38 — owner-cache key for this binding's type-ahead
/// cursor (buffer + last-typed instant). Mirrors `hello-listbox`'s
/// `TYPEAHEAD_KEY`: the cursor lives on the shell's root [`Owner`] and
/// drops with the shell, so no stale search survives across shells in
/// the same thread.
const TYPEAHEAD_KEY: &str = "hello_tree_view::typeahead";

/// R673 §5.50 / R809 / R820 / R820.1 — apply one key to the reactive
/// [`TreeState`]. The WAI-ARIA resolve → flag-store bridge is the lifted
/// [`apply_tree_key`] (R820); an unrecognised (printable) key falls
/// through to the lifted [`tree_typeahead_jump`] (R820.1, shared by all
/// tree consumers — the cursor cache key [`TYPEAHEAD_KEY`] is the only
/// caller-side bit).
fn apply_key_impl(key: &str) -> bool {
    let tree_state = use_tree_state();
    if apply_tree_key(&tree_state.nodes, &tree_state.focused_id, key, NAV_PAGE) {
        return true;
    }
    tree_typeahead_jump(
        &tree_state.focused_id,
        &flat_visible(&tree_state.nodes.get()),
        TYPEAHEAD_KEY,
        key,
    )
}

struct TreeViewBinding;

impl WidgetCore for TreeViewBinding {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn tag() -> &'static str {
        ROOT_BTN_TAG
    }

    fn title() -> &'static str {
        "pinion hello-tree-view (R674 §5.16)"
    }

    fn create_external() -> Box<dyn pinion_core::external::External> {
        Box::new(ButtonExternal::new())
    }

    /// R675 §5.45 — register the substrate
    /// [`TreeRowClickExternal`] sibling that routes composite-tag
    /// clicks on `{TREE_TAG}#{id}` rows into the §5.20 intent
    /// channel. The substrate composes the state scene as
    /// `Scene::Container([primary, tree_router])` so the input
    /// router's depth-first walk reaches both nodes without further
    /// changes. (R674 introduced this wire as binding-level
    /// `FileTreeRowExternal`; R675 lifted it into
    /// [`pinion_widget_paint::tree_view::TreeRowClickExternal`] when
    /// the hello-multi-window inspector became the 2nd consumer,
    /// firing the [[abstraction-needs-second-consumer]] gate.)
    fn create_extra_externals() -> Vec<ExtraExternal> {
        // R823 — the query-only tree-state introspection sibling reads the
        // same `Owner::cache`d `TreeState` Signals the view paints, so the
        // RPC `scene/query` surface reports the live cursor + visible rows.
        // `create_extra_externals` runs inside the root owner scope, so
        // `use_tree_state` resolves the shared handle ([[callback-root-owner-wrap]]).
        let tree_state = use_tree_state();
        let nodes = tree_state.nodes.clone();
        let focused = tree_state.focused_id.clone();
        vec![
            ExtraExternal::new(TREE_TAG, Box::new(TreeRowClickExternal::new())),
            tree_view_introspection_extra(
                TREE_STATE_TAG,
                move || flat_visible(&nodes.get()),
                move || focused.get(),
            ),
        ]
    }

    fn read_state(scene: &Scene) -> Self::State {
        // R55.D.5 — once `create_extra_externals` is non-empty the
        // state scene root is a `Scene::Container`, so locate the
        // primary `Scene::External` by tag rather than pattern
        // matching the root directly.
        if let Some(node) = scene.find_external_with_tag(ROOT_BTN_TAG)
            && let Some(intro) = node.handle.introspect()
            && let Some(IntrospectValue::Text(name)) = intro.query("state")
        {
            return <Self::State as pinion_core::WidgetStateName>::from_name_or_default(&name);
        }
        ButtonState::Idle
    }

    fn event_name(event: Self::Event) -> &'static str {
        <Self::Event as pinion_core::WidgetEventName>::as_name(&event)
    }

    fn view(state: Self::State, frame: &Frame) -> Scene {
        let _ = frame;
        view(state)
    }

    /// R820.1 — single tab stop: keys apply only while the tree root
    /// [`ROOT_BTN_TAG`] is focused (WAI-ARIA tree + `aria-activedescendant`,
    /// the same `focused == Some(tag)` guard `hello-virtual-nav` and the
    /// dock-panels inspector use). Gating here (rather than ignoring
    /// `focused`) keeps the tree from stealing keys when embedded beside a
    /// sibling focusable; the RPC demo issues `focus/set` first.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let _ = scene;
        if focused != Some(ROOT_BTN_TAG) {
            return false;
        }
        apply_key_impl(key)
    }

    /// R674 §5.23 R27 — bridge `FileTreeRowExternal`'s `click`
    /// intent into the lifted [`toggle_expanded`] sink (R820).
    /// Side-effect-only ([[scxml-as-model-update-transient]]) — empty
    /// `Vec<Command>` return; the `Signal::set` write inside
    /// `toggle_expanded` is the mutation.
    ///
    /// Reducer arms compare against the dotted wire form
    /// [`FILE_TREE_CLICK_INTENT_TAG`] per
    /// [[intent-tag-dotted-wire-form]] — bare-event-name matching is
    /// always silent.
    fn update(_state: Self::State, intent: &Intent) -> Vec<pinion_core::command::Command> {
        if intent.tag_str() == FILE_TREE_CLICK_INTENT_TAG
            && let IntrospectValue::Text(id) = &intent.payload
        {
            let tree_state = use_tree_state();
            toggle_expanded(&tree_state.nodes, id);
        }
        Vec::new()
    }
}

impl WidgetA11y for TreeViewBinding {
    /// R674 §5.40 / R812 §5.40 §5.50 — the WAI-ARIA `tree` + per-row
    /// `treeitem` semantic tree, built through the lifted
    /// [`tree_access_nodes`] substrate
    /// (R812 — `hello-tree-view` is its first consumer; the
    /// `hello-dock-panels` inspector is the second). The root advertises
    /// the `Tree` role on the focusable [`ROOT_BTN_TAG`] External and
    /// references every visible row; each row (under the [`TREE_TAG`]
    /// composite prefix the paint substrate stamps) carries the WAI-ARIA
    /// 1.2 hierarchical axes (level / posinset / setsize / aria-expanded)
    /// the builder derives from the [`flat_visible`] SSOT — so the AT
    /// announces exactly the row set the user sees and the keyboard cursor
    /// navigates. This tree carries no selection model, so `selected_id`
    /// is `None`; R868 conveys the keyboard cursor as `focused_id`
    /// (`aria-activedescendant`) instead.
    fn access_node(
        _state: &<Self as WidgetCore>::State,
        _focused: Option<&str>,
    ) -> Vec<AccessNode> {
        let tree_state = use_tree_state();
        let nodes = tree_state.nodes.get();
        let cursor = tree_state.focused_id.get();
        tree_access_nodes(
            ROOT_BTN_TAG,
            TREE_TAG,
            None,
            &flat_visible(&nodes),
            None,
            cursor.as_deref(),
        )
    }

    /// R868 — composite focus: a navigation tree (no selection) still owns a
    /// keyboard cursor, conveyed as `aria-activedescendant` while the tree's
    /// focusable root holds shell focus.
    fn access_focus_target(
        _state: &<Self as WidgetCore>::State,
        focused: Option<&str>,
    ) -> Option<AccessFocus> {
        if focused == Some(ROOT_BTN_TAG)
            && let Some(cursor) = use_tree_state().focused_id.get()
        {
            return Some(AccessFocus::composite(
                ROOT_BTN_TAG,
                tree_row_tag(TREE_TAG, &cursor),
            ));
        }
        focused.map(AccessFocus::atomic)
    }
}

impl WidgetView for TreeViewBinding {
    type Renderer = HelloTreeViewRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TreeViewBinding>();
}

#[cfg(test)]
mod r675_substrate_dotted_wire_test {
    //! R675 §5.16 §5.20 — keep one regression pin proving the
    //! binding's dotted intent tag still matches the substrate's
    //! bare event name. The complete substrate state-machine /
    //! payload coverage moved with the External lift to
    //! `pinion_widget_paint::tree_view::r675_tree_row_click_external_tests`;
    //! this binding-side anchor catches a substrate
    //! `TREE_ROW_CLICK_EVENT` rename that would silently break
    //! `WidgetView::update` reducer matching here.

    use super::{FILE_TREE_CLICK_INTENT_TAG, TREE_TAG};
    use pinion_widget_paint::tree_view::TREE_ROW_CLICK_EVENT;

    #[test]
    fn r675_binding_dotted_form_lockstep_with_substrate_event_name() {
        assert_eq!(
            FILE_TREE_CLICK_INTENT_TAG,
            format!("{TREE_TAG}.{TREE_ROW_CLICK_EVENT}"),
        );
    }
}

#[cfg(test)]
mod r812_tree_access_node_lockstep {
    //! R812 §5.40 §5.50 — binding-side pin that the lifted
    //! [`tree_access_nodes`](pinion_a11y::tree_access_nodes) builder, fed
    //! this binding's tags, emits row composite tags identical to the
    //! paint substrate's
    //! [`composite_row_tag`](pinion_widget_paint::tree_view::composite_row_tag)
    //! — the cross-crate format lockstep that can only be verified here
    //! (the example depends on both `pinion-a11y` and `pinion-widget-paint`).
    //! AT `NodeId`s hash through the same key the hit-test walker uses, so
    //! AT-side `Click` actions resolve to the same row the click router
    //! consumes. The WAI-ARIA hierarchical-axis coverage (level / posinset
    //! / setsize / aria-expanded / aria-selected) moved with the builder to
    //! `pinion_a11y::tree_view`'s own tests when it was lifted at R812.

    use super::{FileNode, ROOT_BTN_TAG, TREE_TAG};
    use pinion_a11y::{AriaRole, tree_access_nodes};
    use pinion_core::widgets::tree_nav::flat_visible;
    use pinion_widget_paint::tree_view::composite_row_tag;

    fn sample_tree() -> Vec<FileNode> {
        vec![
            FileNode::branch(
                "src",
                "src",
                true,
                vec![FileNode::leaf("src/main.rs", "main.rs")],
            ),
            FileNode::branch(
                "docs",
                "docs",
                false,
                vec![FileNode::leaf("docs/README.md", "README.md")],
            ),
        ]
    }

    #[test]
    fn r812_root_uses_focusable_tag_rows_use_paint_composite_format() {
        // hello-tree-view's focusable element is the invisible-root
        // External (ROOT_BTN_TAG), distinct from the painted tree
        // container's row prefix (TREE_TAG) — the builder takes both.
        let rows = flat_visible(&sample_tree());
        let out = tree_access_nodes(ROOT_BTN_TAG, TREE_TAG, None, &rows, None, None);
        assert_eq!(
            out[0].tag, ROOT_BTN_TAG,
            "Tree root node = the focusable External"
        );
        assert_eq!(out[0].role, AriaRole::Tree);
        // Every row tag matches the paint substrate's composite form, so
        // the AT NodeId and the hit-test target hash through one key.
        for (node, row) in out[1..].iter().zip(&rows) {
            assert_eq!(node.tag, composite_row_tag(TREE_TAG, &row.id));
            assert_eq!(node.role, AriaRole::TreeItem);
        }
    }
}

#[cfg(test)]
mod r868_active_descendant {
    //! R868 §5.40 — the navigation tree's keyboard cursor lowers to
    //! `aria-activedescendant` (composite focus + the cursor row's
    //! `with_focused`) while carrying no `aria-selected` (no selection model).
    use super::{ROOT_BTN_TAG, TREE_TAG, TreeViewBinding};
    use pinion_a11y::{WidgetA11y, tree_row_tag};
    use pinion_core::reactive::Owner;
    use pinion_core::widgets::button::ButtonState;

    #[test]
    fn cursor_is_active_descendant_without_selection() {
        Owner::new().run(|| {
            // Boot cursor is "src".
            let focus =
                TreeViewBinding::access_focus_target(&ButtonState::Idle, Some(ROOT_BTN_TAG))
                    .expect("tree focused -> composite focus target");
            assert_eq!(focus.focus_tag, ROOT_BTN_TAG);
            assert_eq!(
                focus.active_descendant.as_deref(),
                Some(tree_row_tag(TREE_TAG, "src").as_str()),
                "active descendant = the cursor row",
            );
            let nodes = TreeViewBinding::access_node(&ButtonState::Idle, None);
            let cursor = nodes
                .iter()
                .find(|n| n.tag == tree_row_tag(TREE_TAG, "src"))
                .expect("cursor row");
            assert!(cursor.state.focused, "cursor row carries with_focused");
            assert_eq!(
                cursor.selected, None,
                "navigation tree: cursor not aria-selected"
            );
            // Focus elsewhere -> atomic, no active descendant.
            let other = TreeViewBinding::access_focus_target(&ButtonState::Idle, Some("elsewhere"));
            assert!(other.expect("atomic").active_descendant.is_none());
        });
    }
}
