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
//!   [`pinion_shell::typeahead`](pinion_shell::typeahead) substrate
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

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::widgets::tree_nav::{flat_visible, resolve_tree_key, TreeKey, TreeNode};
use pinion_core::{reactive, Frame, Owner, Scene, Signal, WidgetCore};
use pinion_shell::typeahead::{is_typeahead_char, TypeaheadCursor};
use pinion_shell::{vello_renderer_impl, SizeStrategy, WidgetView};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;
use pinion_widget_paint::tree_view::{
    view_tree_focused, TreeItem, TreeRowClickExternal, TreeViewFocus, TreeViewStyle,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloTreeViewRenderer, HelloTreeViewRendererError);

const TREE_TAG: &str = "file_tree";
const ROOT_BTN_TAG: &str = "tree_root";
const THEME_TAG: &str = "app";

const WIN_W: u32 = 480;
const WIN_H: u32 = 400;

/// R809 §5.50 — Page Up / Page Down jump size, in visible rows. The
/// content viewport (window height minus the header / footer / padding
/// chrome) holds roughly `(400 - ~64) / 48 ≈ 7` M3 rows, so a
/// page-key advances a near-viewport-ful. `clamp_nav` clamps the
/// result to the listing's ends, so an over-shoot on a short tree just
/// lands on the first / last visible row.
const NAV_PAGE: usize = 7;

/// R674 §5.20 — dotted wire-form intent tag the [`WidgetView::update`]
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
/// in a `Signal<Vec<FileNode>>` so [`apply_key`] mutations
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
        &theme,
        &TreeViewStyle::m3_default(),
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
            .with_layout(LayoutStyle::new().with_size(Size::px(0, 0))),
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

fn find_node_mut<'a>(
    nodes: &'a mut [FileNode],
    target_id: &str,
) -> Option<&'a mut FileNode> {
    for node in nodes {
        if node.id == target_id {
            return Some(node);
        }
        if let Some(found) = find_node_mut(&mut node.children, target_id) {
            return Some(found);
        }
    }
    None
}


/// R674 §5.50 — single source of truth for "toggle the expanded flag
/// on `id`". Used by both [`apply_key_impl`]'s `Space` / `Enter` arm
/// (keyboard path) and [`TreeViewBinding::update`]'s
/// `file_tree.click` reducer arm (click path), so the two paths
/// produce bit-identical reactive state mutations on identical input
/// and the §6.3 `dry_run` invariant continues to hold under both
/// input modes.
///
/// Leaves are a no-op: only branches with children expand or
/// collapse. Wrapping the mutation in [`reactive::batch`] suppresses
/// double-paint on the single-Signal `set` that follows, matching
/// the keyboard handler's pre-R674 wire.
fn toggle_expanded_in_signal(nodes_signal: &Signal<Vec<FileNode>>, id: &str) {
    reactive::batch(|| {
        let mut nodes = nodes_signal.get();
        if let Some(node) = find_node_mut(&mut nodes, id) {
            if node.children.is_empty() {
                return;
            }
            node.expanded = !node.expanded;
            nodes_signal.set(nodes);
        }
    });
}

/// R809 §5.50 — set the expanded flag on branch `id` to `expanded`
/// (the Arrow Right "expand" / Arrow Left "collapse" arms, which set a
/// specific value rather than [`toggle_expanded_in_signal`]'s flip).
/// A leaf or a redundant set is a no-op (no Signal write, no repaint).
/// [`reactive::batch`]ed like its toggle sibling.
fn set_expanded_in_signal(nodes_signal: &Signal<Vec<FileNode>>, id: &str, expanded: bool) {
    reactive::batch(|| {
        let mut nodes = nodes_signal.get();
        if let Some(node) = find_node_mut(&mut nodes, id) {
            if node.children.is_empty() || node.expanded == expanded {
                return;
            }
            node.expanded = expanded;
            nodes_signal.set(nodes);
        }
    });
}

/// R809 §5.22 §5.38 — owner-cache key for this binding's type-ahead
/// cursor (buffer + last-typed instant). Mirrors `hello-listbox`'s
/// `TYPEAHEAD_KEY`: the cursor lives on the shell's root [`Owner`] and
/// drops with the shell, so no stale search survives across shells in
/// the same thread.
const TYPEAHEAD_KEY: &str = "hello_tree_view::typeahead";

/// R809 §5.38 — type-ahead jump: focus the next visible row whose
/// label matches `key`, via the [`pinion_shell::typeahead`] substrate
/// (the `TreeView` consumer that substrate's module doc named as a
/// future taker). Gated by [`is_typeahead_char`] (a single printable
/// codepoint); a named / non-printable key returns `false` so the
/// caller's `apply_key` reports unhandled. The search indexes into the
/// flat visible rows (the same sequence the arrow keys navigate), so a
/// match lands on a row the user can actually see.
fn type_ahead_jump(focused_id: &Signal<Option<String>>, nodes: &[FileNode], key: &str) -> bool {
    let Some(ch) = is_typeahead_char(key) else {
        return false;
    };
    let visible = flat_visible(nodes);
    if visible.is_empty() {
        return false;
    }
    let current = focused_id
        .get()
        .and_then(|id| visible.iter().position(|row| row.id == id));
    let labels: Vec<&str> = visible.iter().map(|row| row.label.as_str()).collect();
    // R51.152 precedent — `Owner::current()` resolves to the shell's
    // root scope because `CoreShell::apply_key` wraps the dispatch in
    // `root_owner().run(...)`. A missing Owner means the apply_key path
    // bypassed the framework wrap (a broken integration).
    let owner = Owner::current()
        .expect("hello-tree-view type-ahead must run inside CoreShell::apply_key wrap");
    let cursor: Rc<RefCell<TypeaheadCursor>> =
        owner.cache(TYPEAHEAD_KEY, || RefCell::new(TypeaheadCursor::new()));
    let target = cursor.borrow_mut().step(ch, current, Instant::now(), &labels);
    let Some(idx) = target else {
        return false;
    };
    focused_id.set(Some(visible[idx].id.clone()));
    true
}

/// R673 §5.50 / R809 — apply one key to the reactive [`TreeState`].
/// Resolves the WAI-ARIA outcome via the pure [`resolve_tree_key`],
/// writes the resulting signal mutation, and falls through to
/// [`type_ahead_jump`] for an unrecognised (printable) key.
fn apply_key_impl(key: &str) -> bool {
    let tree_state = use_tree_state();
    let nodes = tree_state.nodes.get();
    let current = tree_state.focused_id.get();
    let rows = flat_visible(&nodes);
    match resolve_tree_key(&rows, current.as_deref(), key, NAV_PAGE) {
        TreeKey::Focus(id) => {
            tree_state.focused_id.set(Some(id));
            true
        }
        TreeKey::Expand(id) => {
            set_expanded_in_signal(&tree_state.nodes, &id, true);
            true
        }
        TreeKey::Collapse(id) => {
            set_expanded_in_signal(&tree_state.nodes, &id, false);
            true
        }
        TreeKey::Toggle(id) => {
            toggle_expanded_in_signal(&tree_state.nodes, &id);
            true
        }
        TreeKey::Consumed => true,
        TreeKey::Unhandled => type_ahead_jump(&tree_state.focused_id, &nodes, key),
    }
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
        vec![ExtraExternal::new(
            TREE_TAG,
            Box::new(TreeRowClickExternal::new()),
        )]
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
        let _ = frame;
        view(state)
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let _ = (scene, focused);
        apply_key_impl(key)
    }

    /// R674 §5.23 R27 — bridge [`FileTreeRowExternal`]'s `click`
    /// intent into the shared [`toggle_expanded_in_signal`] sink.
    /// Side-effect-only ([[scxml-as-model-update-transient]]) — empty
    /// `Vec<Command>` return; the `Signal::set` write inside
    /// `toggle_expanded_in_signal` is the mutation.
    ///
    /// Reducer arms compare against the dotted wire form
    /// [`FILE_TREE_CLICK_INTENT_TAG`] per
    /// [[intent-tag-dotted-wire-form]] — bare-event-name matching is
    /// always silent.
    fn update(
        _state: Self::State,
        intent: &Intent,
    ) -> Vec<pinion_core::command::Command> {
        if intent.tag_str() == FILE_TREE_CLICK_INTENT_TAG
            && let IntrospectValue::Text(id) = &intent.payload
        {
            let tree_state = use_tree_state();
            toggle_expanded_in_signal(&tree_state.nodes, id);
        }
        Vec::new()
    }
}

/// R674 §5.40 — composite row tag the AT-side
/// [`TreeViewBinding::access_node`] walker emits per visible row.
/// Same format the paint substrate stamps so the AT NodeId hashes
/// through the same key as the hit-test target — AT actions on a
/// row land on the same tag the click router consumes.
fn tree_row_access_tag(node_id: &str) -> String {
    format!("{TREE_TAG}#{node_id}")
}

/// R674 §5.40 / R809.1 — one [`AccessNode`] per visible row, built as a
/// pure map over the [`flat_visible`] SSOT (R809.1 retired the separate
/// `walk_access_rows` traversal that had to track the paint / nav order
/// by hand). Each row carries:
///
/// * `role: AriaRole::TreeItem` — AT announces "tree item …".
/// * `name: label` — accessible name (announced first by the AT).
/// * `level: depth + 1` — WAI-ARIA 1.2 §6.6.8 one-based depth (root
///   children → 1, grandchildren → 2, …).
/// * `position_in_set` / `size_of_set` — WAI-ARIA 1.2 §6.6.9 / §6.6.10
///   sibling addressing.
/// * `aria-expanded` on branches — WAI-ARIA 1.2 §6.6.3 disclosure
///   state (R809.1; leaves omit it). The `expanded` flag rides the
///   same SSOT row the keyboard model toggles, so the AT state can
///   never disagree with the painted glyph.
///
/// Because it maps [`flat_visible`], the AT announces exactly the row
/// set the user sees and the keyboard cursor navigates.
fn access_rows(nodes: &[FileNode]) -> Vec<AccessNode> {
    flat_visible(nodes)
        .into_iter()
        .map(|row| {
            let node = AccessNode::new(tree_row_access_tag(&row.id), AriaRole::TreeItem)
                .with_name(row.label)
                .with_level(row.depth + 1)
                .with_position_in_set(row.position_in_set)
                .with_size_of_set(row.size_of_set);
            if row.has_children {
                node.with_expanded(row.expanded)
            } else {
                node
            }
        })
        .collect()
}

impl WidgetA11y for TreeViewBinding {
    fn access_node(
        _state: &<Self as WidgetCore>::State,
        _focused: Option<&str>,
    ) -> Vec<AccessNode> {
        // R674 §5.40 — root advertises Tree role + lists every
        // visible row as a child so the AT-side topology mirrors the
        // paint topology. Each child row carries hierarchical axes
        // (level / posinset / setsize) per
        // WAI-ARIA 1.2 §6.6.8 / §6.6.9 / §6.6.10 — required for
        // custom-widget roles
        // because AT does NOT infer hierarchy from DOM nesting on
        // `role="treeitem"`.
        let tree_state = use_tree_state();
        let nodes = tree_state.nodes.get();

        let rows = access_rows(&nodes);
        let mut root = AccessNode::new(ROOT_BTN_TAG, AriaRole::Tree);
        for row in &rows {
            root = root.with_child(row.tag.clone());
        }
        let mut out = vec![root];
        out.extend(rows);
        out
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
mod r674_per_row_access_node_tests {
    //! R674 §5.40 / R809.1 — per-row [`AriaRole::TreeItem`]
    //! [`AccessNode`] emission contract. Tests the [`access_rows`]
    //! builder directly (R809.1: a pure map over the `flat_visible`
    //! SSOT) so the WAI-ARIA 1.2 hierarchical axes (level / posinset /
    //! setsize / aria-expanded) are verified without the Owner cache.

    use super::{access_rows, tree_row_access_tag, FileNode, TREE_TAG};
    use pinion_a11y::AriaRole;

    fn sample_tree() -> Vec<FileNode> {
        vec![
            FileNode::branch(
                "src",
                "src",
                true,
                vec![
                    FileNode::leaf("src/main.rs", "main.rs"),
                    FileNode::branch(
                        "src/widgets",
                        "widgets",
                        true,
                        vec![FileNode::leaf("src/widgets/mod.rs", "mod.rs")],
                    ),
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

    #[test]
    fn r674_walk_emits_one_node_per_visible_row_in_paint_order() {
        // src (expanded) → src/main.rs, src/widgets (expanded) →
        // src/widgets/mod.rs ; docs (collapsed). 5 visible rows.
        let nodes = sample_tree();
        let out = access_rows(&nodes);
        let tags: Vec<&str> = out.iter().map(|n| n.tag.as_str()).collect();
        assert_eq!(
            tags,
            vec![
                "file_tree#src",
                "file_tree#src/main.rs",
                "file_tree#src/widgets",
                "file_tree#src/widgets/mod.rs",
                "file_tree#docs",
            ],
            "paint order = depth-first preorder over visible rows",
        );
    }

    #[test]
    fn r674_all_rows_carry_tree_item_role() {
        let nodes = sample_tree();
        let out = access_rows(&nodes);
        for node in &out {
            assert_eq!(
                node.role,
                AriaRole::TreeItem,
                "per-row tag {} must report treeitem role",
                node.tag,
            );
        }
    }

    #[test]
    fn r674_level_is_depth_plus_one_one_based() {
        // WAI-ARIA 1.2 §6.6.8 — root children are at level 1, not 0.
        let nodes = sample_tree();
        let out = access_rows(&nodes);
        // src, docs → level 1
        // src/main.rs, src/widgets → level 2
        // src/widgets/mod.rs → level 3
        let by_tag: std::collections::HashMap<&str, Option<u32>> = out
            .iter()
            .map(|n| (n.tag.as_str(), n.level))
            .collect();
        assert_eq!(by_tag["file_tree#src"], Some(1));
        assert_eq!(by_tag["file_tree#docs"], Some(1));
        assert_eq!(by_tag["file_tree#src/main.rs"], Some(2));
        assert_eq!(by_tag["file_tree#src/widgets"], Some(2));
        assert_eq!(by_tag["file_tree#src/widgets/mod.rs"], Some(3));
    }

    #[test]
    fn r674_position_in_set_is_sibling_index_plus_one() {
        // WAI-ARIA 1.2 §6.6.9 — one-based; "item N of M" sentence
        // has N = position_in_set.
        let nodes = sample_tree();
        let out = access_rows(&nodes);
        let by_tag: std::collections::HashMap<&str, Option<u32>> = out
            .iter()
            .map(|n| (n.tag.as_str(), n.position_in_set))
            .collect();
        // Root: src is 1st of 2, docs is 2nd of 2.
        assert_eq!(by_tag["file_tree#src"], Some(1));
        assert_eq!(by_tag["file_tree#docs"], Some(2));
        // Under src: main.rs is 1st of 2, widgets is 2nd of 2.
        assert_eq!(by_tag["file_tree#src/main.rs"], Some(1));
        assert_eq!(by_tag["file_tree#src/widgets"], Some(2));
        // Under src/widgets: mod.rs is 1st of 1.
        assert_eq!(by_tag["file_tree#src/widgets/mod.rs"], Some(1));
    }

    #[test]
    fn r674_size_of_set_is_sibling_count() {
        // WAI-ARIA 1.2 §6.6.10 — total siblings in the parent's
        // visible set. Collapsed branches contribute to the parent
        // count (the branch itself is visible).
        let nodes = sample_tree();
        let out = access_rows(&nodes);
        let by_tag: std::collections::HashMap<&str, Option<u32>> = out
            .iter()
            .map(|n| (n.tag.as_str(), n.size_of_set))
            .collect();
        // Root has 2 siblings (src, docs).
        assert_eq!(by_tag["file_tree#src"], Some(2));
        assert_eq!(by_tag["file_tree#docs"], Some(2));
        // src has 2 children siblings (main.rs, widgets).
        assert_eq!(by_tag["file_tree#src/main.rs"], Some(2));
        assert_eq!(by_tag["file_tree#src/widgets"], Some(2));
        // src/widgets has 1 child sibling (mod.rs).
        assert_eq!(by_tag["file_tree#src/widgets/mod.rs"], Some(1));
    }

    #[test]
    fn r674_name_mirrors_label() {
        // AT-side announcement = the visible label, by convention
        // — keeps the AT user and the sighted user reading the same
        // identifier.
        let nodes = sample_tree();
        let out = access_rows(&nodes);
        let by_tag: std::collections::HashMap<&str, Option<&str>> = out
            .iter()
            .map(|n| (n.tag.as_str(), n.name.as_deref()))
            .collect();
        assert_eq!(by_tag["file_tree#src"], Some("src"));
        assert_eq!(by_tag["file_tree#src/main.rs"], Some("main.rs"));
        assert_eq!(by_tag["file_tree#src/widgets/mod.rs"], Some("mod.rs"));
        assert_eq!(by_tag["file_tree#docs"], Some("docs"));
    }

    #[test]
    fn r674_collapsed_branch_hides_children_from_at_tree() {
        // docs is collapsed; docs/README.md must NOT appear in the
        // visible row sequence (AT announces what the user sees).
        let nodes = sample_tree();
        let out = access_rows(&nodes);
        let tags: Vec<&str> = out.iter().map(|n| n.tag.as_str()).collect();
        assert!(
            !tags.contains(&"file_tree#docs/README.md"),
            "collapsed branch must hide descendants from the visible row set",
        );
        // But docs itself stays visible.
        assert!(tags.contains(&"file_tree#docs"));
    }

    #[test]
    fn r674_empty_tree_emits_no_rows() {
        let nodes: Vec<FileNode> = Vec::new();
        let out = access_rows(&nodes);
        assert!(out.is_empty(), "empty tree contributes zero TreeItem rows");
    }

    #[test]
    fn r809_1_branches_carry_aria_expanded_leaves_omit_it() {
        // WAI-ARIA 1.2 §6.6.3 — an expandable treeitem must expose
        // aria-expanded; a leaf must omit it. R809.1 sources the flag
        // from the same flat_visible row the keyboard toggles, so the
        // AT state can never disagree with the painted disclosure glyph.
        let nodes = sample_tree();
        let out = access_rows(&nodes);
        let by_tag: std::collections::HashMap<&str, Option<bool>> =
            out.iter().map(|n| (n.tag.as_str(), n.expanded)).collect();
        // src is an expanded branch; src/widgets is an expanded branch;
        // docs is a collapsed branch.
        assert_eq!(by_tag["file_tree#src"], Some(true));
        assert_eq!(by_tag["file_tree#src/widgets"], Some(true));
        assert_eq!(by_tag["file_tree#docs"], Some(false));
        // src/main.rs is a leaf — aria-expanded omitted.
        assert_eq!(by_tag["file_tree#src/main.rs"], None);
        assert_eq!(by_tag["file_tree#src/widgets/mod.rs"], None);
    }

    #[test]
    fn r674_tree_row_access_tag_uses_composite_format() {
        // Lockstep with `composite_row_tag` in the paint substrate —
        // AT NodeId hashes through the same key the hit-test walker
        // uses, so AT-side `Click` actions resolve to the same row
        // the click router consumes.
        assert_eq!(
            tree_row_access_tag("src/lib.rs"),
            format!("{TREE_TAG}#src/lib.rs"),
        );
    }
}
