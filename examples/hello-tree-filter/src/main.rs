//! R821 §5.27 §5.40 §5.50 — **recursive tree filter proxy** consumer: a
//! scene-graph outliner whose search box filters the whole tree to the
//! paths that lead to a match.
//!
//! This is the hierarchical member of the Model/View proxy family. R747
//! lands the 1-D list sort/filter proxy (`hello-virtual-sort`) and R778/R783
//! the data-grid column sort/filter proxy (`hello-grid-sort` /
//! `hello-grid-filter`); this demo lands the **tree** proxy a scene-graph /
//! asset outliner needs — exactly the editor-tree search the Phase-D
//! self-hosted editor's outliner will use. The recursion is Qt's
//! `QSortFilterProxyModel` with `setRecursiveFilteringEnabled(true)`: a node
//! is kept iff it matches the query **or any descendant matches**, so a
//! match buried inside a collapsed group is *revealed* — its ancestors shown
//! as path context, auto-expanded — while every non-match sibling is pruned.
//!
//! ## What it demonstrates
//!
//! - **Recursive path-to-match filtering**
//!   ([`flat_visible_filtered`],
//!   the SSOT decision). The filtered visible-row sequence drops into
//!   [`view_virtual_tree`] and
//!   [`tree_access_nodes`](pinion_a11y::tree_access_nodes) with **no** change
//!   — the filtered tree is just another `VisibleRow` flattening, so it
//!   windows + lowers to WAI-ARIA exactly like the unfiltered tree.
//! - **Reveal-inside-collapsed**: groups 4..12 boot collapsed; an empty
//!   query hides their children, but filtering `Node09` surfaces
//!   `Group09 → Node09_*` regardless of the collapsed flag (the filter
//!   ignores expand state along match paths).
//! - **AI-first**: the primary path is RPC —
//!   [`TreeFilterExternal`]
//!   `invoke "set_filter"` / `query "visible_count"` / `query
//!   "visible_at.<pos>"`. Keyboard type-to-filter is the GUI peer (an
//!   alphanumeric char appends, Backspace pops, Escape clears).
//! - **Virtualization ⊥ filter**: only the windowed slice of the filtered
//!   rows is ever a scene node; `Node` (matches all 240 leaves) windows the
//!   whole 252-row tree, `Node03_05` windows a 2-row result.
//!
//! This demo's tree is **immutable in normal use** (a filter demo, not an
//! expand/collapse demo — `hello-virtual-tree` owns that axis): the boot
//! expand flags never change, so only the query ever drives a recompute. The
//! proxy is nonetheless backed by a reactive `Computed` (R821.1) that tracks
//! *both* the query and the source-tree `Signal`, so a live tree edit would
//! update the filtered view too — the soundness witness
//! `filter_re_derives_when_the_source_tree_mutates` mutates the tree under an
//! unchanged query to prove it. Keyboard navigation is vertical roving over
//! the visible rows (the windowed-widget keyboard model
//! `hello-virtual-select` / `hello-virtual-nav` use).

use pinion_a11y::{AccessFocus, AccessNode, WidgetA11y, tree_row_tag, windowed_tree_access_nodes};
use pinion_core::external::{
    External, ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
};
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::widgets::scroll::{ScrollState, use_scroll_state};
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::tree_filter::{TreeFilterExternal, TreeFilterState, use_tree_filter};
use pinion_core::widgets::tree_nav::{
    TreeKey, TreeNode, VisibleRow, flat_visible, flat_visible_filtered, resolve_tree_key,
};
use pinion_core::widgets::virtual_list::{compute_visible_range, scroll_offset_to_reveal};
use pinion_core::{Frame, Owner, Scene, Signal, WidgetCore};
use pinion_shell::typeahead::is_typeahead_char;
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::scrollbar::{VerticalScrollbarStyle, view_vertical_scrollbar};
use pinion_widget_paint::tree_view::{
    TreeRowClickExternal, TreeViewFocus, TreeViewStyle, view_virtual_tree,
};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTreeFilterRenderer, HelloTreeFilterRendererError);

const WIN_W: u32 = 520;
const WIN_H: u32 = 560;
const THEME_TAG: &str = "app";
/// Composite-tag prefix the windowed rows carry (`{TREE_TAG}#{id}`) and the
/// [`TreeRowClickExternal`] anchor clicks route to.
const TREE_TAG: &str = "sgtree";
/// Focusable tree-root External tag (the WAI-ARIA `tree` node + the
/// `read_state` anchor). Distinct from [`TREE_TAG`] (root External vs row
/// container), like `hello-virtual-tree`.
const ROOT_TAG: &str = "sgtree_root";
/// The filter proxy External tag — the AI-first `set_filter` / `query`
/// surface ([`TreeFilterExternal`]).
const FILTER_TAG: &str = "sgtree_filter";
/// `use_tree_filter` cache key (shared by the view, the a11y pass, the
/// keyboard handler and the [`TreeFilterExternal`]).
const FILTER_KEY: &str = "hello_tree_filter::filter";
const SCROLL_KEY: &str = "sgtree_scroll";
const SCROLLBAR_TAG: &str = "sgtree_scrollbar";
/// The search-box container tag (a plain container, addressable for the RPC
/// demo's live-pixel guard — not a routing/External tag).
const SEARCH_TAG: &str = "sgtree_search";

/// Top-level scene-graph groups.
const GROUPS: usize = 12;
/// Leaf nodes per group.
const CHILDREN_PER: usize = 20;
/// Total dataset: each group row + its leaves.
const TOTAL_NODES: usize = GROUPS * (1 + CHILDREN_PER);
/// Groups expanded at boot; the rest stay collapsed so filtering visibly
/// reveals matches buried inside a collapsed group.
const EXPANDED_AT_BOOT: usize = 4;

/// Uniform per-row vertical slot. Must equal [`TreeViewStyle::row_height`]
/// (`view_virtual_tree` derives its slot pitch from the style); asserted in
/// the view.
const ROW_PITCH: u32 = 48;
/// Rows rendered beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Gutter reserved for the scrollbar peer.
const SCROLLBAR_W: u32 = 12;
const HEADER_FONT_PX: u32 = 14;
/// Page Up / Down jump in rows (a viewport-ful, clamped via `clamp_nav`).
const NAV_PAGE: usize = 9;

/// The dotted wire form of the [`TreeRowClickExternal`] row-click intent,
/// composed via [`intent_tag!`] so the literal stays in lockstep with the
/// substrate's bare `"click"` event name (pinned in `tests`).
const CLICK_INTENT_TAG: &str = intent_tag!("sgtree", "click");

/// One scene-graph node. Carries its own `expanded` flag (the retained
/// flag-on-node storage the tree family uses). The `serde` + `PartialEq`
/// derives satisfy the §5.22 introspect bound `Signal<T>` carries.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct TreeRow {
    id: String,
    label: String,
    expanded: bool,
    children: Vec<TreeRow>,
}

impl TreeRow {
    fn leaf(id: String, label: String) -> Self {
        Self {
            id,
            label,
            expanded: false,
            children: Vec::new(),
        }
    }
}

impl TreeNode for TreeRow {
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

/// Build the synthetic scene graph. Deterministic ids (`g{s}` /
/// `g{s}-n{i}`) and labels (`Group{ss}` / `Node{ss}_{ii}`) so the RPC demo
/// can address matches by a stable, predictable query:
/// - `Group03` matches only that group (its `Node03_*` leaves do not), so a
///   matched branch with no matching child shows as a filtered leaf;
/// - `Node03` matches all 20 leaves of group 3 → the group is revealed as
///   path context;
/// - `_05` matches one leaf per group → 12 groups each revealed with 1 leaf.
fn initial_nodes() -> Vec<TreeRow> {
    (0..GROUPS)
        .map(|s| {
            let children = (0..CHILDREN_PER)
                .map(|i| TreeRow::leaf(format!("g{s}-n{i}"), format!("Node{s:02}_{i:02}")))
                .collect();
            TreeRow {
                id: format!("g{s}"),
                label: format!("Group{s:02}"),
                expanded: s < EXPANDED_AT_BOOT,
                children,
            }
        })
        .collect()
}

/// Reactive holder for the retained (immutable) tree + the keyboard cursor.
/// The tree never mutates in this demo — the filter is the only structural
/// control — so the proxy can memoize its filtered flattening on the query
/// string alone.
/// R855 — the sibling-sort coordinator anchor (`/sgtree_sort/external`). A tree
/// sibling-sort is not the list/grid `Ord`-proxy shape, so it is an example-local
/// holder (a 2nd tree-sort consumer lifts it).
const SORT_TAG: &str = "sgtree_sort";

/// R855 — the scene-outliner sibling sort, composed *atop* the R821 filter:
/// source order, or ascending / descending by label. Cycles
/// `Source → Asc → Desc → Source`.
// `Signal<TreeSort>` requires its payload to be (de)serializable for the §2 #7
// scene-as-data snapshot path (the R838 `Signal<T>` serde trap).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum TreeSort {
    Source,
    Asc,
    Desc,
}

impl TreeSort {
    fn cycle(self) -> Self {
        match self {
            Self::Source => Self::Asc,
            Self::Asc => Self::Desc,
            Self::Desc => Self::Source,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "source" => Some(Self::Source),
            "asc" => Some(Self::Asc),
            "desc" => Some(Self::Desc),
            _ => None,
        }
    }
}

/// R855 — `nodes` with every level's siblings reordered by `sort` (a clone;
/// `Source` is the identity). The flatten walks children in Vec order, so
/// reordering the children Vecs here sorts the rendered + filtered output with
/// **no substrate change** — sort → filter → flatten compose as a proxy stack.
fn sorted_tree(nodes: &[TreeRow], sort: TreeSort) -> Vec<TreeRow> {
    let mut out = nodes.to_vec();
    if sort != TreeSort::Source {
        sort_siblings(&mut out, sort);
    }
    out
}

fn sort_siblings(nodes: &mut [TreeRow], sort: TreeSort) {
    match sort {
        TreeSort::Asc => nodes.sort_by(|a, b| a.label.cmp(&b.label)),
        TreeSort::Desc => nodes.sort_by(|a, b| b.label.cmp(&a.label)),
        TreeSort::Source => {}
    }
    for node in nodes.iter_mut() {
        sort_siblings(&mut node.children, sort);
    }
}

struct TreeState {
    nodes: Signal<Vec<TreeRow>>,
    /// R855 — the active sibling sort (shared with the [`TreeSortExternal`] and
    /// read inside the filter recompute closure, so a sort change re-filters).
    sort: Signal<TreeSort>,
    /// The keyboard cursor's row id (WAI-ARIA `aria-activedescendant` / the
    /// painted focus row). `None` until the first key / click lands.
    focused_id: Signal<Option<String>>,
}

fn use_tree_state() -> Rc<TreeState> {
    let owner =
        Owner::current().expect("use_tree_state must run inside a CoreShell view / reducer wrap");
    owner.cache("hello_tree_filter::state", || TreeState {
        nodes: Signal::new(initial_nodes()),
        sort: Signal::new(TreeSort::Source),
        focused_id: Signal::new(None),
    })
}

/// Resolve the shared filter proxy. The recompute closure captures the
/// retained tree (resolved *first*, never inside `use_tree_filter`'s factory
/// — that would nest `Owner::cache`): an empty query is the "no filter"
/// fallback (the normal expand-respecting [`flat_visible`]), a non-empty
/// query a case-insensitive label substring through [`flat_visible_filtered`].
fn use_filter() -> Rc<TreeFilterState> {
    let tree = use_tree_state();
    use_tree_filter(FILTER_KEY, move || {
        Box::new(move |query: &str| {
            // R855 — sort the siblings first (reads the sort Signal, so the
            // filter's Computed re-runs on a sort change), then filter + flatten
            // the sorted tree. The proxy stack: sort → filter → flatten.
            let nodes = sorted_tree(&tree.nodes.get(), tree.sort.get());
            if query.is_empty() {
                flat_visible(&nodes)
            } else {
                let needle = query.to_lowercase();
                flat_visible_filtered(&nodes, |n| n.label().to_lowercase().contains(&needle))
            }
        })
    })
}

/// R855 §5.27 §5.12 — the AI-first sibling-sort coordinator: a tiny config
/// holder over the shared [`TreeState::sort`] `Signal`, surfaced to RPC. `query
/// sort` reads the current mode; `invoke cycle_sort` advances it; `invoke
/// set_sort` jumps to a named mode (`intervene sort` mirrors `set_sort`). It
/// emits no §5.20 intent — the `Signal::set` already repaints the subscribed
/// view (the `ViewSortFilterExternal` display-config pattern).
#[derive(Clone)]
struct TreeSortExternal {
    state: Rc<TreeState>,
}

impl TreeSortExternal {
    fn new(state: Rc<TreeState>) -> Self {
        Self { state }
    }

    fn set(&mut self, mode: TreeSort) -> IntrospectValue {
        self.state.sort.set(mode);
        IntrospectValue::Text(mode.label().to_owned())
    }
}

impl core::fmt::Debug for TreeSortExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TreeSortExternal")
            .field("sort", &self.state.sort.get().label())
            .finish()
    }
}

// R858 — the Gui+Rpc display-config-proxy External skeleton via the lifted
// pinion-core SSOT macro, instead of hand-rolling the five methods.
pinion_core::external::query_proxy_external_impl!(TreeSortExternal);

impl ExternalIntrospect for TreeSortExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("sort", "string"),
            ("cycle_sort", "string"),
            ("set_sort", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "sort" => Some(IntrospectValue::Text(
                self.state.sort.get().label().to_owned(),
            )),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        if path != "sort" {
            return Err(InterveneError::UnknownPath);
        }
        let IntrospectValue::Text(s) = value else {
            return Err(InterveneError::TypeMismatch);
        };
        let mode = TreeSort::parse(&s).ok_or(InterveneError::TypeMismatch)?;
        self.set(mode);
        Ok(())
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "cycle_sort" => Ok(self.set(self.state.sort.get().cycle())),
            "set_sort" => match args {
                IntrospectValue::Text(s) => {
                    let mode = TreeSort::parse(&s).ok_or(InvokeError::Rejected)?;
                    Ok(self.set(mode))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// Scroll the keyboard cursor's row into the window
/// ([`scroll_offset_to_reveal`]) so navigating to an off-window row scrolls
/// there and materializes it (keyboard ⊥ virtualization). No-op when nothing
/// is focused or the cursor row is not in the current (filtered) view.
fn reveal_cursor(focused: Option<&str>, rows: &[VisibleRow], scroll: &Rc<ScrollState>) {
    let Some(id) = focused else {
        return;
    };
    let Some(index) = rows.iter().position(|row| row.id == id) else {
        return;
    };
    let (_, measured_h) = scroll.measured_viewport();
    let offset = scroll_offset_to_reveal(index, scroll.offset_y(), measured_h, ROW_PITCH);
    scroll.scroll_to(0, offset);
}

/// Apply one key while the tree root is focused: type-to-filter (an
/// alphanumeric char appends to the query, Backspace pops, Escape clears),
/// then vertical roving navigation over the *visible* (filtered) rows. The
/// filtered flattening is just another `VisibleRow` sequence, so the pure
/// [`resolve_tree_key`] resolver navigates it with no special-casing.
fn apply_key_impl(key: &str) -> bool {
    let filter = use_filter();
    if let Some(ch) = is_typeahead_char(key) {
        let mut query = filter.query();
        query.push(ch);
        filter.set_query(query);
        return true;
    }
    match key {
        "Backspace" => {
            let mut query = filter.query();
            if query.pop().is_some() {
                filter.set_query(query);
            }
            true
        }
        "Escape" => {
            if filter.is_filtering() {
                filter.set_query(String::new());
                true
            } else {
                false
            }
        }
        "ArrowUp" | "ArrowDown" | "Home" | "End" | "PageUp" | "PageDown" => {
            let state = use_tree_state();
            let rows = filter.visible_rows();
            let current = state.focused_id.get();
            if let TreeKey::Focus(id) = resolve_tree_key(&rows, current.as_deref(), key, NAV_PAGE) {
                state.focused_id.set(Some(id));
                reveal_cursor(
                    state.focused_id.get().as_deref(),
                    &rows,
                    &use_scroll_state(SCROLL_KEY),
                );
            }
            true
        }
        _ => false,
    }
}

/// The search box: shows the active query (with a block caret) or a muted
/// placeholder. Display-only — input is the keyboard `apply_key` path + the
/// RPC `set_filter` channel.
fn search_box(query: &str, theme: &Theme) -> Scene {
    let (label, fg) = if query.is_empty() {
        (
            "Type to filter the scene graph\u{2026}".to_string(),
            theme.resolve(ColorRole::OnSurfaceMuted),
        )
    } else {
        (
            format!("Filter: {query}\u{2588}"),
            theme.resolve(ColorRole::OnSurface),
        )
    };
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            label,
            Rect::default(),
            TextStyle::new().with_size_px(HEADER_FONT_PX).with_fg(fg),
        ))])
        .with_tag(SEARCH_TAG)
        .with_style(
            BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh)).with_corner_radius(8),
        )
        .with_layout(LayoutStyle::new().with_padding(Rect::new(12, 9, 12, 9))),
    )
}

/// Header status line: the dataset size + the current filtered-row count.
fn header(query: &str, visible: usize, theme: &Theme) -> Scene {
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    // R855 — reading the sort here subscribes the header to it, so it repaints
    // (and the summary updates) when the sort cycles.
    let sort = use_tree_state().sort.get().label();
    let summary = if query.is_empty() {
        format!("{TOTAL_NODES} nodes \u{00B7} {visible} rows visible \u{00B7} sort: {sort}")
    } else {
        format!(
            "{TOTAL_NODES} nodes \u{00B7} filter \u{2192} {visible} match rows \u{00B7} sort: {sort}"
        )
    };
    Scene::Text(TextNode::styled(
        summary,
        Rect::default(),
        TextStyle::new().with_size_px(HEADER_FONT_PX).with_fg(muted),
    ))
}

/// view-fn (§6.3): pure sync mapping. The dataset is virtual —
/// `view_virtual_tree` builds rows only for the indices in the current
/// scroll window, taken from the *filtered* `visible_rows` flattening.
#[allow(clippy::trivially_copy_pass_by_ref)] // mirrors the WidgetCore::view `&Frame` signature
fn view(_state: ButtonState, _frame: &Frame) -> Scene {
    // `view_virtual_tree` derives its slot pitch from `style.row_height`;
    // keep ROW_PITCH (the a11y windowing math uses it) in lockstep.
    debug_assert_eq!(TreeViewStyle::m3_default().row_height, ROW_PITCH);

    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_tree_state();
    let filter = use_filter();
    let rows = filter.visible_rows();
    let query = filter.query();
    let focused = state.focused_id.get();
    let scroll = use_scroll_state(SCROLL_KEY);

    let tree = view_virtual_tree(
        TREE_TAG,
        &scroll,
        &rows,
        &TreeViewFocus {
            focused_id: focused.as_deref(),
        },
        OVERSCAN,
        &theme,
        &TreeViewStyle::m3_default(),
    );

    let (_, measured_h) = scroll.measured_viewport();
    let scrollbar_style = VerticalScrollbarStyle::material(measured_h, SCROLLBAR_TAG);
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll,
        &theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );

    // Row band: the windowed tree beside the scrollbar peer, flex-grow so it
    // fills the window below the search box + header.
    let band = Scene::Container(
        ContainerNode::new(vec![tree, scrollbar_visual]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_flex_grow(1.0),
        ),
    );

    // Invisible 0x0 root External keeps the SCXML state surface alive for
    // read_state / RPC introspect (the WAI-ARIA `tree` node also lands here).
    let invisible_root = Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(ROOT_TAG)
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(0, 0))
                    .with_focusable(true),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![
            search_box(&query, &theme),
            header(&query, rows.len(), &theme),
            band,
            invisible_root,
        ])
        .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Stretch)
                .with_justify(JustifyContent::Start)
                .with_gap(8)
                .with_padding(Rect::new(12, 12, 12 + SCROLLBAR_W, 12)),
        ),
    )
}

struct SceneGraphFilter;

impl WidgetCore for SceneGraphFilter {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn tag() -> &'static str {
        ROOT_TAG
    }

    fn title() -> &'static str {
        "pinion hello-tree-filter (R821 §5.27 recursive tree filter proxy)"
    }

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    /// `TreeRowClickExternal` sibling (composite `{TREE_TAG}#{id}` clicks →
    /// the §5.20 intent channel → [`SceneGraphFilter::update`]) + the filter
    /// proxy External + the `ScrollBarExternal` sharing the tree's
    /// `Rc<ScrollState>`.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let filter = use_filter();
        vec![
            ExtraExternal::new(TREE_TAG, Box::new(TreeRowClickExternal::new())),
            ExtraExternal::new(
                FILTER_TAG,
                Box::new(TreeFilterExternal::new(Rc::clone(&filter))),
            ),
            // R855 — the AI-first sibling-sort surface (`/sgtree_sort/external`).
            ExtraExternal::new(SORT_TAG, Box::new(TreeSortExternal::new(use_tree_state()))),
            scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG),
        ]
    }

    fn read_state(scene: &Scene) -> Self::State {
        if let Some(node) = scene.find_external_with_tag(ROOT_TAG)
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
        view(state, frame)
    }

    /// Click a row → move the keyboard cursor to it (selection-follows-click;
    /// the row stays a plain windowed node, the cursor is the AT
    /// active-descendant). Side-effect-only reducer
    /// ([[scxml-as-model-update-transient]]): the `Signal::set` is the
    /// mutation, so the command list is empty.
    fn update(_state: Self::State, intent: &Intent) -> Vec<pinion_core::command::Command> {
        if intent.tag_str() == CLICK_INTENT_TAG
            && let IntrospectValue::Text(id) = &intent.payload
        {
            use_tree_state().focused_id.set(Some(id.clone()));
        }
        Vec::new()
    }

    /// WAI-ARIA tree keyboard: type-to-filter + vertical roving over the
    /// filtered rows. Single tab stop — keys apply only while the tree root
    /// [`ROOT_TAG`] is focused. The gate is required, not cosmetic: an
    /// ungated tree would steal keys from sibling panels in a multi-pane host
    /// (the eventual self-hosted editor's outliner beside its viewport), so it
    /// honours the same `focused == Some(tag)` guard the windowed-tree /
    /// virtual-nav consumers use ([[routing-and-focus-are-separate-axes]]).
    /// The RPC demo issues `focus/set` first, like every other gated example.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let _ = scene;
        if focused != Some(ROOT_TAG) {
            return false;
        }
        apply_key_impl(key)
    }

    fn fmt_state_log(_state: &ButtonState) -> String {
        format!("scene-graph filter over {TOTAL_NODES} nodes (recursive path-to-match)")
    }
}

impl WidgetA11y for SceneGraphFilter {
    /// WAI-ARIA virtualized `tree` over the **filtered** window: one
    /// `AriaRole::Tree` root referencing only the windowed rows, plus one
    /// `AriaRole::TreeItem` per rendered row carrying its (renumbered, over
    /// the filtered sibling group) `aria-level` / `aria-posinset` /
    /// `aria-setsize` / `aria-expanded`. Built through the same
    /// `tree_access_nodes` (R812) the unfiltered virtual tree uses — the
    /// filtered flattening is just another `VisibleRow` slice, so the AT tree
    /// and the painted tree never diverge. The keyboard cursor row carries
    /// `aria-selected` (selection-follows-focus / `aria-activedescendant`).
    fn access_node(_state: &ButtonState, _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_tree_state();
        let filter = use_filter();
        let cursor = state.focused_id.get();
        let rows = filter.visible_rows();
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(
            scroll.offset_y(),
            measured_h,
            rows.len(),
            ROW_PITCH,
            OVERSCAN,
        );
        windowed_tree_access_nodes(
            ROOT_TAG,
            TREE_TAG,
            Some("Scene graph"),
            &rows,
            &window,
            cursor.as_deref(),
            cursor.as_deref(),
        )
    }

    /// R868 — composite focus: the filtered tree's keyboard cursor is the
    /// `aria-activedescendant` while the tree owns focus (the cursor row is
    /// always within the filtered set — `reveal`/clamp keep it visible).
    fn access_focus_target(_state: &ButtonState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(ROOT_TAG)
            && let Some(cursor) = use_tree_state().focused_id.get()
        {
            return Some(AccessFocus::composite(
                ROOT_TAG,
                tree_row_tag(TREE_TAG, &cursor),
            ));
        }
        focused.map(AccessFocus::atomic)
    }
}

impl WidgetView for SceneGraphFilter {
    type Renderer = HelloTreeFilterRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<SceneGraphFilter>();
}

#[cfg(test)]
mod tests {
    use super::{
        CHILDREN_PER, CLICK_INTENT_TAG, EXPANDED_AT_BOOT, GROUPS, ROOT_TAG, ROW_PITCH, SCROLL_KEY,
        SceneGraphFilter, TOTAL_NODES, TREE_TAG, TreeRow, TreeSort, TreeSortExternal,
        apply_key_impl, initial_nodes, sorted_tree, tree_row_tag, use_filter, use_tree_state, view,
    };
    use std::rc::Rc;

    use pinion_a11y::{AriaRole, WidgetA11y};
    use pinion_core::external::{ExternalIntrospect, InterveneError, IntrospectValue, InvokeError};
    use pinion_core::widgets::button::ButtonState;
    use pinion_core::widgets::scroll::use_scroll_state;
    use pinion_core::{Frame, Owner, Scene};
    use pinion_widget_paint::tree_view::{TREE_ROW_CLICK_EVENT, TreeViewStyle};

    const WIN_VIEWPORT_W: u32 = 480;
    const WIN_VIEWPORT_H: u32 = 384;

    #[test]
    fn dataset_size_matches_total_nodes() {
        let nodes = initial_nodes();
        let count: usize = nodes.iter().map(|g| 1 + g.children.len()).sum();
        assert_eq!(count, TOTAL_NODES);
        assert_eq!(nodes.len(), GROUPS);
    }

    #[test]
    fn slot_pitch_matches_style_row_height() {
        assert_eq!(TreeViewStyle::m3_default().row_height, ROW_PITCH);
    }

    #[test]
    fn click_intent_tag_matches_substrate_event() {
        assert_eq!(
            CLICK_INTENT_TAG,
            format!("{TREE_TAG}.{TREE_ROW_CLICK_EVENT}")
        );
    }

    #[test]
    fn empty_filter_shows_groups_plus_expanded_children() {
        Owner::new().run(|| {
            let filter = use_filter();
            // Boot: no filter → groups + the boot-expanded groups' children.
            let expected = GROUPS + EXPANDED_AT_BOOT * CHILDREN_PER;
            assert_eq!(filter.visible_count(), expected, "boot visible rows");
            assert!(!filter.is_filtering());
        });
    }

    #[test]
    fn filter_reveals_matches_inside_a_collapsed_group() {
        Owner::new().run(|| {
            let filter = use_filter();
            // Group09 boots collapsed (9 >= EXPANDED_AT_BOOT), so its children
            // are hidden unfiltered…
            let unfiltered_labels: Vec<String> = filter
                .visible_rows()
                .iter()
                .map(|r| r.label.clone())
                .collect();
            assert!(
                !unfiltered_labels.iter().any(|l| l == "Node09_00"),
                "a collapsed group's child is hidden unfiltered",
            );
            // …but filtering reveals Group09 → Node09_* regardless of the flag.
            assert_eq!(
                filter.set_query("Node09"),
                1 + CHILDREN_PER,
                "group 9 + its 20 leaves"
            );
            let labels: Vec<String> = filter
                .visible_rows()
                .iter()
                .map(|r| r.label.clone())
                .collect();
            assert_eq!(
                labels[0], "Group09",
                "the ancestor is revealed as path context"
            );
            assert!(
                labels.iter().any(|l| l == "Node09_00"),
                "the buried match is revealed"
            );
        });
    }

    #[test]
    fn matching_group_with_no_matching_children_is_a_filtered_leaf() {
        Owner::new().run(|| {
            let filter = use_filter();
            // "Group03" matches only the group; its Node03_* leaves do not.
            assert_eq!(filter.set_query("Group03"), 1, "just the matched group");
            let rows = filter.visible_rows();
            assert_eq!(rows[0].label, "Group03");
            assert!(
                !rows[0].has_children,
                "a matched branch with no matching child is a leaf"
            );
        });
    }

    #[test]
    fn filter_is_case_insensitive_and_clears() {
        Owner::new().run(|| {
            let filter = use_filter();
            assert_eq!(
                filter.set_query("node03"),
                1 + CHILDREN_PER,
                "lowercase matches Node03"
            );
            assert_eq!(
                filter.set_query(""),
                GROUPS + EXPANDED_AT_BOOT * CHILDREN_PER,
                "clear"
            );
        });
    }

    #[test]
    fn type_to_filter_then_backspace_and_escape() {
        Owner::new().run(|| {
            let filter = use_filter();
            // Type "Group" one char at a time (alphanumeric → appends).
            for ch in ["G", "r", "o", "u", "p"] {
                assert!(
                    apply_key_impl(ch),
                    "an alphanumeric key is consumed by the filter"
                );
            }
            assert_eq!(filter.query(), "Group");
            assert_eq!(
                filter.visible_count(),
                GROUPS,
                "every group label contains 'Group'"
            );
            // Backspace pops one char.
            assert!(apply_key_impl("Backspace"));
            assert_eq!(filter.query(), "Grou");
            // Escape clears.
            assert!(apply_key_impl("Escape"));
            assert_eq!(filter.query(), "");
            // Escape with no active filter is not consumed (bubbles).
            assert!(!apply_key_impl("Escape"));
        });
    }

    /// Count the rendered `{TREE_TAG}#<id>` row containers under the scroll
    /// wrapper.
    fn rendered_row_count(scene: &Scene) -> usize {
        fn walk(scene: &Scene, prefix: &str) -> usize {
            match scene {
                Scene::Scroll(s) => walk(s.content.as_ref(), prefix),
                Scene::Container(c) => {
                    let here = usize::from(c.tag.as_deref().is_some_and(|t| t.starts_with(prefix)));
                    here + c.children.iter().map(|ch| walk(ch, prefix)).sum::<usize>()
                }
                _ => 0,
            }
        }
        walk(scene, &format!("{TREE_TAG}#"))
    }

    #[test]
    fn view_windows_only_a_slice_of_the_filtered_rows() {
        Owner::new().run(|| {
            // Give the scroll a measured viewport so windowing renders a bounded
            // slice rather than the whole (unmeasured) column.
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_VIEWPORT_W, WIN_VIEWPORT_H);
            // Filter to all leaves (the whole 252-row tree) and assert the
            // rendered window is far smaller than the dataset.
            use_filter().set_query("Node");
            let scene = view(ButtonState::Idle, &Frame::default());
            let rendered = rendered_row_count(&scene);
            assert!(rendered > 0, "some rows render");
            assert!(
                rendered < TOTAL_NODES,
                "only a window renders, not all {TOTAL_NODES}"
            );
        });
    }

    #[test]
    fn a11y_tree_lowers_filtered_window_with_cursor_selected_and_active() {
        Owner::new().run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_VIEWPORT_W, WIN_VIEWPORT_H);
            let filter = use_filter();
            filter.set_query("Node03"); // Group03 + 20 leaves
            use_tree_state().focused_id.set(Some("g3".to_string()));
            let nodes = SceneGraphFilter::access_node(&ButtonState::Idle, None);
            assert_eq!(nodes[0].role, AriaRole::Tree, "first node is the tree root");
            assert!(
                nodes[1..].iter().all(|n| n.role == AriaRole::TreeItem),
                "windowed rows are treeitems",
            );
            // The cursor row (Group03 = id g3) carries aria-selected.
            assert!(
                nodes.iter().any(|n| n.selected == Some(true)),
                "the cursor row is aria-selected (selection-follows-focus)",
            );
            // R868 — and `aria-activedescendant`: composite focus names the
            // cursor row, and that row node carries `with_focused`.
            let focus = SceneGraphFilter::access_focus_target(&ButtonState::Idle, Some(ROOT_TAG))
                .expect("tree focused -> composite focus target");
            assert_eq!(
                focus.active_descendant.as_deref(),
                Some(tree_row_tag(TREE_TAG, "g3").as_str()),
            );
            let cursor = nodes
                .iter()
                .find(|n| n.tag == tree_row_tag(TREE_TAG, "g3"))
                .expect("cursor row");
            assert!(cursor.state.focused, "cursor row carries with_focused");
        });
    }

    // The `flat_visible_filtered` core algorithm (path-to-match recursion,
    // sibling renumbering, matching-branch-as-leaf, no-match) is unit-tested
    // next to its definition in `pinion_core::widgets::tree_nav`; the tests
    // below are this consumer's integration coverage over its real `use_filter`
    // coordinator and `TreeRow` tree.

    // ── TreeFilterExternal RPC adapter (query / invoke / intervene + guards) ──

    #[test]
    fn external_query_invoke_intervene_and_guards() {
        use pinion_core::external::{
            ExternalIntrospect, InterveneError, IntrospectValue, InvokeError,
        };
        use pinion_core::widgets::tree_filter::TreeFilterExternal;
        use std::rc::Rc;

        let boot = i64::try_from(GROUPS + EXPANDED_AT_BOOT * CHILDREN_PER).unwrap();
        let node03 = i64::try_from(1 + CHILDREN_PER).unwrap();
        Owner::new().run(|| {
            let state = use_filter();
            let mut ext = TreeFilterExternal::new(Rc::clone(&state));
            // Boot: empty query, full view.
            assert_eq!(
                ext.query("query"),
                Some(IntrospectValue::Text(String::new()))
            );
            assert_eq!(ext.query("visible_count"), Some(IntrospectValue::Int(boot)));
            // invoke set_filter returns the resulting visible_count in one trip.
            assert_eq!(
                ext.invoke("set_filter", IntrospectValue::Text("Node03".into())),
                Ok(IntrospectValue::Int(node03)),
            );
            assert_eq!(
                ext.query("query"),
                Some(IntrospectValue::Text("Node03".into()))
            );
            assert_eq!(
                ext.query("visible_at.0"),
                Some(IntrospectValue::Text("g3".into()))
            );
            assert_eq!(
                ext.query("label_at.0"),
                Some(IntrospectValue::Text("Group03".into()))
            );
            assert_eq!(
                ext.query("visible_at.999"),
                Some(IntrospectValue::Null),
                "out-of-range filtered position is present-but-empty",
            );
            assert_eq!(ext.query("nope"), None, "undeclared path is absent");
            // intervene sets the query (admin/restore); read-only + type guards.
            ext.intervene("query", IntrospectValue::Text("Group07".into()))
                .unwrap();
            assert_eq!(state.query(), "Group07");
            assert_eq!(
                ext.intervene("visible_count", IntrospectValue::Int(1)),
                Err(InterveneError::ReadOnly),
            );
            assert_eq!(
                ext.intervene("query", IntrospectValue::Int(1)),
                Err(InterveneError::TypeMismatch),
            );
            assert_eq!(
                ext.invoke("bogus", IntrospectValue::Null),
                Err(InvokeError::UnknownPath)
            );
            // Null clears via invoke → back to the full view.
            assert_eq!(
                ext.invoke("set_filter", IntrospectValue::Null),
                Ok(IntrospectValue::Int(boot)),
            );
            assert_eq!(state.query(), "");
        });
    }

    #[test]
    fn filter_re_derives_when_the_source_tree_mutates() {
        // The decisive soundness witness for the reactive `Computed` memo: a
        // tree edit *under an unchanged active filter* must update the filtered
        // view. A memo keyed on the query alone (the sort-proxy pattern, valid
        // only for an owned immutable source) would return stale rows here; the
        // `Computed` tracks the source-tree `Signal` and re-derives.
        Owner::new().run(|| {
            let filter = use_filter();
            let tree = use_tree_state();
            filter.set_query("Node03"); // Group03 + its 20 matching leaves
            let before = filter.visible_count();
            assert_eq!(before, 1 + CHILDREN_PER);
            // Append a new matching leaf to group 3 WITHOUT touching the query.
            let mut nodes = tree.nodes.get();
            nodes[3]
                .children
                .push(TreeRow::leaf("g3-nNEW".to_string(), "Node03_NEW".to_string()));
            tree.nodes.set(nodes);
            assert_eq!(
                filter.visible_count(),
                before + 1,
                "a live tree edit updates the filtered view (Computed tracks the tree, not just the query)",
            );
        });
    }

    // ── R855 sibling sort (composed atop the filter) ───────────────

    #[test]
    fn r855_sorted_tree_orders_siblings_recursively() {
        let nodes = initial_nodes();
        assert_eq!(
            sorted_tree(&nodes, TreeSort::Source),
            nodes,
            "Source is the identity"
        );
        let asc = sorted_tree(&nodes, TreeSort::Asc);
        let desc = sorted_tree(&nodes, TreeSort::Desc);
        let asc_l: Vec<&str> = asc.iter().map(|n| n.label.as_str()).collect();
        let desc_l: Vec<&str> = desc.iter().map(|n| n.label.as_str()).collect();
        assert!(
            asc_l.windows(2).all(|w| w[0] <= w[1]),
            "top-level siblings ascending: {asc_l:?}"
        );
        assert!(
            desc_l.windows(2).all(|w| w[0] >= w[1]),
            "top-level siblings descending: {desc_l:?}"
        );
        assert_ne!(
            asc_l, desc_l,
            "asc and desc differ (the sort actually reorders)"
        );
        // Recursive: a group's children are ordered too.
        let ch: Vec<&str> = asc[0].children.iter().map(|n| n.label.as_str()).collect();
        assert!(
            ch.windows(2).all(|w| w[0] <= w[1]),
            "children sorted ascending (recursive): {ch:?}"
        );
    }

    #[test]
    fn r855_sort_change_re_derives_the_filtered_view() {
        Owner::new().run(|| {
            let filter = use_filter();
            let state = use_tree_state();
            let source_first = filter.visible_rows().first().map(|r| r.label.clone());
            state.sort.set(TreeSort::Desc);
            let desc_first = filter.visible_rows().first().map(|r| r.label.clone());
            assert_ne!(
                source_first, desc_first,
                "a sort change re-derives the visible order"
            );
            // The leading visible group is the largest label under descending sort.
            let groups: Vec<String> = filter
                .visible_rows()
                .iter()
                .filter(|r| r.depth == 0)
                .map(|r| r.label.clone())
                .collect();
            assert!(
                groups.windows(2).all(|w| w[0] >= w[1]),
                "visible groups sorted descending: {groups:?}"
            );
        });
    }

    #[test]
    fn r855_sort_external_query_cycle_set_intervene_and_guards() {
        Owner::new().run(|| {
            let state = use_tree_state();
            let mut ext = TreeSortExternal::new(Rc::clone(&state));
            let text = |s: &str| Some(IntrospectValue::Text(s.to_owned()));
            assert_eq!(ext.query("sort"), text("source"), "boots in source order");
            // cycle: source -> asc -> desc -> source
            assert_eq!(
                ext.invoke("cycle_sort", IntrospectValue::Null),
                Ok(IntrospectValue::Text("asc".to_owned()))
            );
            assert_eq!(ext.query("sort"), text("asc"));
            assert_eq!(
                ext.invoke("cycle_sort", IntrospectValue::Null),
                Ok(IntrospectValue::Text("desc".to_owned()))
            );
            assert_eq!(
                ext.invoke("cycle_sort", IntrospectValue::Null),
                Ok(IntrospectValue::Text("source".to_owned()))
            );
            // set_sort jumps to a named mode; an unknown name is Rejected.
            assert_eq!(
                ext.invoke("set_sort", IntrospectValue::Text("desc".to_owned())),
                Ok(IntrospectValue::Text("desc".to_owned())),
            );
            assert_eq!(
                state.sort.get(),
                TreeSort::Desc,
                "set_sort wrote the shared signal"
            );
            assert_eq!(
                ext.invoke("set_sort", IntrospectValue::Text("bogus".to_owned())),
                Err(InvokeError::Rejected)
            );
            // intervene mirrors set_sort; a bad value is a TypeMismatch.
            ext.intervene("sort", IntrospectValue::Text("asc".to_owned()))
                .expect("intervene asc");
            assert_eq!(state.sort.get(), TreeSort::Asc);
            assert_eq!(
                ext.intervene("sort", IntrospectValue::Text("nope".to_owned())),
                Err(InterveneError::TypeMismatch)
            );
            // guards
            assert_eq!(ext.query("bogus"), None);
            assert_eq!(
                ext.invoke("bogus", IntrospectValue::Null),
                Err(InvokeError::UnknownPath)
            );
        });
    }

    #[test]
    fn r855_sort_and_filter_compose() {
        Owner::new().run(|| {
            let filter = use_filter();
            let state = use_tree_state();
            filter.set_query("Node"); // reveal groups + their matching leaves
            state.sort.set(TreeSort::Desc);
            assert!(filter.visible_count() > 0, "the filter matched");
            let rows = filter.visible_rows();
            // The filtered view is still sorted: depth-0 groups descend, and a
            // group's revealed leaves descend within it.
            let groups: Vec<String> = rows
                .iter()
                .filter(|r| r.depth == 0)
                .map(|r| r.label.clone())
                .collect();
            assert!(
                groups.windows(2).all(|w| w[0] >= w[1]),
                "filtered groups descend: {groups:?}"
            );
            // The revealed leaves under the first group (consecutive depth >= 1
            // rows after the leading header) descend too.
            let first_leaves: Vec<String> = rows
                .iter()
                .skip_while(|r| r.depth != 0)
                .skip(1)
                .take_while(|r| r.depth >= 1)
                .map(|r| r.label.clone())
                .collect();
            assert!(
                first_leaves.windows(2).all(|w| w[0] >= w[1]),
                "filtered leaves descend within a group: {first_leaves:?}",
            );
        });
    }
}
