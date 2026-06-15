//! R942 §5.22 §5.23 §5.27 §2 — `hello-lazy-tree`: a scene-outliner / asset
//! browser over a tree whose **children are fetched asynchronously on
//! expand**.
//!
//! The Model/View-at-scale axis so far virtualizes (`hello-virtual-tree`),
//! lazily pages a *flat* list (`hello-lazy-list`), and reparents an
//! in-memory tree (`hello-tree-reparent`). The gap this fills: a tree whose
//! structure itself is out-of-memory — each branch's children arrive
//! asynchronously the first time it is expanded, the way a real asset
//! browser reads a directory or a scene graph streams a sub-hierarchy.
//!
//! ## Architecture (unidirectional, Effect-driven fetch-on-expand)
//!
//! - A per-node child cache — [`ResourceCache`](pinion_core::ResourceCache)
//!   keyed by node id; only expanded branches are ever fetched, and each
//!   branch's children carry their own reactive [`ResourceState`].
//! - The expand set is a `Signal<BTreeSet<String>>`. Clicking a branch row
//!   (the [`TreeRowClickExternal`] composite-tag router) toggles its id.
//! - A single lifetime-held [`Effect`] subscribes to the expand set and, on
//!   every change (and its eager boot run), calls
//!   [`ResourceCache::ensure`] for the root plus each expanded branch not yet
//!   cached, through the shell-polled
//!   [`LocalTaskPump`](pinion_core::LocalTaskPump). The fetch never runs in
//!   the view, so view-fn purity holds (§6.3).
//! - The view snapshots the root + every expanded branch's child state and
//!   *flattens* the visible tree: a `Ready` branch contributes its children
//!   (recursing into the expanded ones); a branch still `Loading` contributes
//!   one skeleton placeholder. This async flattening is the only new
//!   machinery — the in-memory `flat_visible` SSOT walks a `&[TreeNode]`
//!   slice, which a lazily-fetched tree cannot provide.
//!
//! This is the same Resource + Effect + `ResourceCache` + `DeferredReady` +
//! pump substrate R923/R924 introduced (asset browser / lazy list), now keyed
//! on expand instead of scroll, over a tree instead of a flat list.
//!
//! ## ZERO-FLAKE latency model
//!
//! Each child fetch is a deterministic [`DeferredReady`] future (`Pending`
//! [`FETCH_LATENCY_POLLS`] times, then `Ready`). Every `scene/snapshot
//! from=paint` advances the pump one step (the poll lives in the shell paint
//! path), so the demo's own snapshot polling drives `skeleton → rows` with no
//! wall-clock race — `wait_snap` on the skeleton is guaranteed to catch it
//! before the children resolve (same discipline as the R923 deferred demo).

use pinion_a11y::{tree_access_nodes, tree_row_tag, AccessNode, AriaRole, WidgetA11y};
use pinion_core::command::Command;
use pinion_core::external::{External, IntrospectValue, StubExternal};
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::reactive::{DeferredReady, Effect, Owner, ResourceCache, ResourceState};
use pinion_core::scene::{ContainerNode, Rect, TextNode, TextRole};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::tree_nav::{tree_view_introspection_extra, VisibleRow};
use pinion_core::{use_local_task_pump, Frame, LocalTaskPump, Scene, Signal, WidgetCore};
use pinion_shell::{vello_renderer_impl, SizeStrategy, WidgetView};
use pinion_widget_paint::glyph::{DISCLOSURE_COLLAPSED, DISCLOSURE_EXPANDED};
use pinion_widget_paint::tree_view::{TreeRowClickExternal, TreeViewStyle};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloLazyTreeRenderer, HelloLazyTreeRendererError);

// ─── window + tag constants ────────────────────────────────────────────────

const WIN_W: u32 = 460;
const WIN_H: u32 = 520;
const THEME_TAG: &str = "app";

/// Primary `StubExternal` anchor tag + the WAI-ARIA `tree` root tag + the
/// painted rows-container tag (one tag, three roles — the same convention as
/// `hello-lazy-list`'s list root).
const ROOT_TAG: &str = "lazytree";
/// The [`TreeRowClickExternal`] router tag + per-row composite prefix: each
/// row paints under `{ROW_PREFIX}#{id}` and a click on it toggles expansion.
const ROW_PREFIX: &str = "lazytree_row";
/// The `role=status` live-region band tag (async load announcement).
const STATUS_TAG: &str = "lazytree_status";
/// The transient skeleton placeholder row tag (shown beneath an expanded
/// branch while its children are in flight; not a composite `#id` tag, so the
/// click router ignores it).
const SKELETON_TAG: &str = "lazytree_skeleton";
/// The query-only tree-state introspection extra tag (`scene/query`).
const STATE_TAG: &str = "lazytree_state";

/// The dotted wire form of the row-click intent (`{ROW_PREFIX}.click`), matched
/// in [`LazyTreeView::update`] per [[intent-tag-dotted-wire-form]].
const CLICK_INTENT_TAG: &str = intent_tag!("lazytree_row", "click");

// ─── synthetic out-of-memory tree source ───────────────────────────────────

/// The synthetic root: its children are the top-level outliner nodes.
const ROOT_ID: &str = "";
/// Children at depth `>= MAX_BRANCH_DEPTH` are leaves, so the generated tree
/// terminates (a real source terminates at empty directories).
const MAX_BRANCH_DEPTH: u32 = 3;
/// `Pending` polls before a child fetch resolves — a deterministic stand-in
/// for source latency that keeps the skeleton observable across frames
/// (ZERO-FLAKE; long enough to survive the click's own repaints).
const FETCH_LATENCY_POLLS: u32 = 24;

/// Leaf disclosure-column placeholder (NO-BREAK SPACE) — keeps the label
/// column of leaves aligned with branches' triangles
/// ([[non-ascii-literal-named-const-escape]]).
const GLYPH_LEAF: &str = "\u{00A0}";
/// The skeleton placeholder label (`Loading…`).
const LOADING_LABEL: &str = "Loading\u{2026}";
/// Conceptual label of the synthetic root, used in the boot status line.
const ROOT_LABEL: &str = "assets";

const FOLDER_WORDS: [&str; 6] =
    ["Scenes", "Models", "Textures", "Materials", "Audio", "Prefabs"];
const FILE_WORDS: [&str; 6] = ["mesh", "albedo", "normal", "clip", "shader", "rig"];
const FILE_EXT: [&str; 6] = ["scene", "fbx", "png", "wav", "wgsl", "anim"];

/// `Owner::cache` keys for the shared per-binding state.
const CACHE_KEY: &str = "lazytree.children_cache";
const EXPANDED_KEY: &str = "lazytree.expanded";
const LOADER_KEY: &str = "lazytree.loader";

/// One fetched child descriptor — the serde payload a node's child
/// [`ResourceState`] carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LazyChild {
    /// Stable, path-shaped id (`"0"`, `"0/1"`, …) — the nav cursor key, the
    /// composite-tag suffix, and the brancher input.
    id: String,
    /// Visible label + AT accessible name.
    label: String,
    /// Whether this node can be expanded (its children are fetched on demand).
    is_branch: bool,
}

/// The depth of `parent_id`'s children (the root's children are depth 0),
/// derived from the path segment count so a node's depth is a pure function of
/// its id alone.
fn child_depth(parent_id: &str) -> u32 {
    if parent_id.is_empty() {
        0
    } else {
        u32::try_from(parent_id.matches('/').count())
            .unwrap_or(u32::MAX)
            .saturating_add(1)
    }
}

/// Whether the node at `id` is a branch — the single source of truth both
/// [`child_nodes`] (stamping each child) and the click reducer (gating the
/// expand toggle) read, so the two never disagree about which rows disclose.
/// Pure function of the id: a branch when not too deep and at an even sibling
/// index (so each level mixes branches and leaves).
fn id_is_branch(id: &str) -> bool {
    if id.is_empty() {
        return true; // the synthetic root is always a branch
    }
    let (parent, last) = id.rsplit_once('/').unwrap_or(("", id));
    let index: usize = last.parse().unwrap_or(0);
    child_depth(parent) < MAX_BRANCH_DEPTH && index % 2 == 0
}

/// How many children `parent_id` has — 3 at the root, else a deterministic
/// 2..=4 from the id bytes (no RNG, so every run is identical).
fn child_count(parent_id: &str) -> usize {
    if parent_id.is_empty() {
        3
    } else {
        let seed: usize = parent_id.bytes().map(usize::from).sum();
        2 + seed % 3
    }
}

/// A deterministic label for a node, varied by its parent + sibling index.
fn node_label(parent_id: &str, index: usize, is_branch: bool) -> String {
    let seed: usize = parent_id.bytes().map(usize::from).sum();
    let slot = (seed + index) % FOLDER_WORDS.len();
    if is_branch {
        FOLDER_WORDS[slot].to_owned()
    } else {
        format!("{}_{index}.{}", FILE_WORDS[slot], FILE_EXT[slot])
    }
}

/// The deterministic child set of `parent_id`. The tree is generated, never
/// materialised: only expanded branches are ever asked for their children.
fn child_nodes(parent_id: &str) -> Vec<LazyChild> {
    (0..child_count(parent_id))
        .map(|index| {
            let id = if parent_id.is_empty() {
                index.to_string()
            } else {
                format!("{parent_id}/{index}")
            };
            let is_branch = id_is_branch(&id);
            let label = node_label(parent_id, index, is_branch);
            LazyChild { id, label, is_branch }
        })
        .collect()
}

/// One node's children behind the deterministic fetch latency — the future
/// the `ResourceCache` drives through the pump. Always `Ok`; the synthetic
/// source never errors (the `Error` arm is handled defensively downstream).
fn fetch_children(parent_id: &str) -> DeferredReady<Result<Vec<LazyChild>, String>> {
    DeferredReady::new(FETCH_LATENCY_POLLS, Ok(child_nodes(parent_id)))
}

// ─── shared per-binding state ───────────────────────────────────────────────

/// Per-node child cache — keyed by node id; resident children are retained for
/// the session (the tree's key space is bounded by what the user expands, so
/// the unbounded [`ResourceCache::new`] variant is correct, as in the
/// bounded-page lazy list).
type ChildrenCache = ResourceCache<String, Vec<LazyChild>, String>;

fn use_children_cache() -> Rc<ChildrenCache> {
    let owner = Owner::current().expect("use_children_cache requires an active Owner scope");
    owner.cache(CACHE_KEY, ChildrenCache::new)
}

fn use_expanded() -> Rc<Signal<BTreeSet<String>>> {
    let owner = Owner::current().expect("use_expanded requires an active Owner scope");
    owner.cache(EXPANDED_KEY, || Signal::new(BTreeSet::new()))
}

/// Toggle `id` in the expand set (add if absent, remove if present). The set
/// is the SSOT the loader `Effect` and the flattening both read.
fn toggle_expanded(expanded: &Signal<BTreeSet<String>>, id: &str) {
    let mut set = expanded.get();
    if !set.remove(id) {
        set.insert(id.to_owned());
    }
    expanded.set(set);
}

// ─── expand-driven loader Effect ───────────────────────────────────────────

/// Lifetime marker holding the expand-driven loader [`Effect`] (R665).
struct LoaderMarker {
    _effect: Effect,
}

/// Ensure the root plus every expanded branch has a child fetch in flight or
/// resolved. Idempotent (`ResourceCache::ensure` is get-or-fetch), so
/// re-running it every expand-set change re-fetches nothing already cached.
/// Owner-scoped side effect, run only from the loader [`Effect`] — never the
/// view (§6.3 purity).
fn ensure_children_loaded(
    expanded: &BTreeSet<String>,
    cache: &ChildrenCache,
    pump: &LocalTaskPump,
) {
    cache.ensure(ROOT_ID.to_owned(), pump, || fetch_children(ROOT_ID));
    for id in expanded {
        let key = id.clone();
        cache.ensure(key, pump, || fetch_children(id));
    }
}

/// Install the loader [`Effect`] once. It subscribes to the expand set and, on
/// every change (including the eager boot run that fetches the root), ensures
/// each newly-expanded branch's children start loading. Resolved via
/// `Owner::cache` so re-entry returns the same marker.
fn install_loader() -> Rc<LoaderMarker> {
    let owner = Owner::current().expect("install_loader requires an active Owner scope");
    let expanded = use_expanded();
    let cache = use_children_cache();
    let pump = use_local_task_pump();
    let owner_for_effect = owner.clone();
    owner.cache(LOADER_KEY, move || {
        let expanded_e = expanded.clone();
        let cache_e = cache.clone();
        let pump_e = pump.clone();
        let effect = Effect::new(&owner_for_effect, move || {
            let exp = expanded_e.get(); // subscribe to the expand set
            ensure_children_loaded(&exp, &cache_e, &pump_e);
        });
        LoaderMarker { _effect: effect }
    })
}

// ─── async flattening (example-local) ──────────────────────────────────────

/// The visible nodes' resolved child states, snapshotted once per frame.
type ChildStates = HashMap<String, ResourceState<Vec<LazyChild>, String>>;

/// One flattened outliner row: a loaded node, or the transient placeholder
/// shown beneath an expanded branch whose children are still in flight.
enum LazyRow {
    /// A loaded tree node (carries the WAI-ARIA hierarchical axes).
    Node(VisibleRow),
    /// A loading placeholder under the branch named `parent_label`.
    Skeleton {
        /// The placeholder's tree depth (its parent's depth + 1).
        depth: u32,
        /// The expanding branch's label, for the status announcement.
        parent_label: String,
    },
}

/// Recursively flatten the *visible* tree under `parent_id`. A node's children
/// appear only when it is expanded; an expanded branch whose children are
/// still loading contributes one skeleton placeholder. Example-local because
/// the children are async — the `flat_visible` SSOT walks an in-memory
/// `&[TreeNode]`, which a lazily-fetched tree cannot provide.
fn flatten(
    parent_id: &str,
    parent_label: &str,
    depth: u32,
    expanded: &BTreeSet<String>,
    states: &ChildStates,
    out: &mut Vec<LazyRow>,
) {
    match states.get(parent_id) {
        Some(ResourceState::Ready(children)) => {
            let size_of_set = u32::try_from(children.len()).unwrap_or(u32::MAX);
            for (i, child) in children.iter().enumerate() {
                let is_expanded = child.is_branch && expanded.contains(&child.id);
                out.push(LazyRow::Node(VisibleRow {
                    id: child.id.clone(),
                    label: child.label.clone(),
                    depth,
                    position_in_set: u32::try_from(i + 1).unwrap_or(u32::MAX),
                    size_of_set,
                    has_children: child.is_branch,
                    expanded: is_expanded,
                }));
                if is_expanded {
                    flatten(&child.id, &child.label, depth + 1, expanded, states, out);
                }
            }
        }
        // Loading, errored, or not-yet-requested → one placeholder row. The
        // synthetic source never errors; the `Error` arm folds in defensively.
        _ => out.push(LazyRow::Skeleton {
            depth,
            parent_label: parent_label.to_owned(),
        }),
    }
}

/// Compute the current flattened outliner rows from the expand set + the child
/// cache. Snapshots the root + every expanded branch (the only nodes whose
/// child lists the flattening reads), subscribing the caller to each. Shared
/// by the view (paint), the a11y tree, and the query-only introspection extra
/// so `scene/query` and the painted scene never diverge.
fn compute_rows(expanded: &Signal<BTreeSet<String>>, cache: &ChildrenCache) -> Vec<LazyRow> {
    let exp = expanded.get();
    let mut keys: Vec<String> = Vec::with_capacity(exp.len() + 1);
    keys.push(ROOT_ID.to_owned());
    keys.extend(exp.iter().cloned());
    let states = cache.snapshot(keys);
    let mut out = Vec::new();
    flatten(ROOT_ID, ROOT_LABEL, 0, &exp, &states, &mut out);
    out
}

/// The loaded (non-skeleton) rows — the stable semantic tree the a11y builder
/// and `scene/query` report. A skeleton is a transient paint placeholder, not
/// a `treeitem` with a stable position (the loading child count is unknown).
fn node_rows(rows: &[LazyRow]) -> Vec<VisibleRow> {
    rows.iter()
        .filter_map(|r| match r {
            LazyRow::Node(vr) => Some(vr.clone()),
            LazyRow::Skeleton { .. } => None,
        })
        .collect()
}

/// The `role=status` band text: the loading branch while children are in
/// flight (the first skeleton's parent), else the loaded visible-row count.
fn status_text(rows: &[LazyRow]) -> String {
    let loading = rows.iter().find_map(|r| match r {
        LazyRow::Skeleton { parent_label, .. } => Some(parent_label.clone()),
        LazyRow::Node(_) => None,
    });
    if let Some(label) = loading {
        format!("Loading {label}\u{2026}")
    } else {
        let n = node_rows(rows).len();
        let plural = if n == 1 { "" } else { "s" };
        format!("{n} item{plural} loaded")
    }
}

// ─── view ──────────────────────────────────────────────────────────────────

/// The shared inner cells of one outliner row — a depth-indent spacer, a
/// fixed-width disclosure glyph, then the label — used by both the loaded node
/// row and the skeleton placeholder so their indent / glyph columns line up.
/// (Mirrors the private `tree_cell_content` in `pinion_widget_paint`, which a
/// lazily-fetched, skeleton-interleaved row sequence cannot reuse; lift to a
/// shared builder when a 2nd external per-row consumer appears —
/// [[abstraction-needs-second-consumer]].)
fn row_cells(
    depth: u32,
    glyph: &str,
    label: &str,
    label_role: ColorRole,
    theme: &Theme,
    style: &TreeViewStyle,
) -> Vec<Scene> {
    let mut cells: Vec<Scene> = Vec::with_capacity(3);
    let indent_px = depth * style.indent_step;
    if indent_px > 0 {
        // Presentational depth-indent spacer (no tag → AT-invisible).
        cells.push(Scene::Container(ContainerNode::new(Vec::new()).with_layout(
            LayoutStyle::new().with_size(Size::px(indent_px, style.row_height)),
        )));
    }
    // Fixed-width glyph column so leaf rows (narrow placeholder) and branch
    // rows (triangle) line up their labels; presentational so the row's AT
    // name comes from the label, not the glyph.
    let glyph_node = Scene::Text(
        TextNode::styled(
            glyph,
            Rect::default(),
            TextStyle::new()
                .with_size_px(style.glyph_size_px)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_role(TextRole::Presentational),
    );
    cells.push(Scene::Container(ContainerNode::new(vec![glyph_node]).with_layout(
        LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_align_items(AlignItems::Center)
            .with_justify(JustifyContent::Center)
            .with_size(Size::px(style.glyph_size_px, style.row_height)),
    )));
    cells.push(Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.font_size_px)
            .with_fg(theme.resolve(label_role)),
    )));
    cells
}

/// The row container layout — full cross-axis width, fixed height, glyph/label
/// gap + horizontal padding (the canonical tree-row shape).
fn row_layout(style: &TreeViewStyle) -> LayoutStyle {
    LayoutStyle::new()
        .flex(FlexDirection::Row)
        .with_align_items(AlignItems::Center)
        .with_justify(JustifyContent::Start)
        .with_gap(style.glyph_label_gap)
        .with_size(Size::height_px(style.row_height))
        .with_padding(Rect::new(style.row_padding, 0, style.row_padding, 0))
}

/// A loaded node row — tagged `{ROW_PREFIX}#{id}` so the click router toggles
/// it and the a11y tree + bounds enrichment attach.
fn node_row_view(row: &VisibleRow, theme: &Theme, style: &TreeViewStyle) -> Scene {
    let glyph = if !row.has_children {
        GLYPH_LEAF
    } else if row.expanded {
        DISCLOSURE_EXPANDED
    } else {
        DISCLOSURE_COLLAPSED
    };
    Scene::Container(
        ContainerNode::new(row_cells(
            row.depth,
            glyph,
            &row.label,
            ColorRole::OnSurface,
            theme,
            style,
        ))
        .with_tag(tree_row_tag(ROW_PREFIX, &row.id))
        .with_layout(row_layout(style)),
    )
}

/// A skeleton placeholder row — tagged [`SKELETON_TAG`] (not a composite
/// `#id`, so the click router ignores it), muted `Loading…` label.
fn skeleton_row_view(depth: u32, theme: &Theme, style: &TreeViewStyle) -> Scene {
    Scene::Container(
        ContainerNode::new(row_cells(
            depth,
            GLYPH_LEAF,
            LOADING_LABEL,
            ColorRole::OnSurfaceMuted,
            theme,
            style,
        ))
        .with_tag(SKELETON_TAG)
        .with_layout(row_layout(style)),
    )
}

fn row_view(row: &LazyRow, theme: &Theme, style: &TreeViewStyle) -> Scene {
    match row {
        LazyRow::Node(vr) => node_row_view(vr, theme, style),
        LazyRow::Skeleton { depth, .. } => skeleton_row_view(*depth, theme, style),
    }
}

/// view-fn (§6.3): pure sync `() -> Scene`. Reads the expand set + each
/// expanded branch's child `Resource` state; the tree structure is fetched
/// lazily, so a freshly-expanded branch renders a skeleton until it resolves.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = TreeViewStyle::default();
    let expanded = use_expanded();
    let cache = use_children_cache();
    let rows = compute_rows(&expanded, &cache);

    let title = Scene::Text(TextNode::styled(
        "Lazy-loaded asset outliner (children fetched on expand)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(15)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let status = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            status_text(&rows),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(STATUS_TAG)
        .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    );

    let row_scenes: Vec<Scene> = rows.iter().map(|r| row_view(r, &theme, &style)).collect();
    let tree = Scene::Container(
        ContainerNode::new(row_scenes)
            .with_tag(ROOT_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_size(Size::px(WIN_W - 24, WIN_H - 96)),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![title, status, tree])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_size(Size::px(WIN_W, WIN_H))
                    .with_gap(10)
                    .with_padding(Rect::new(12, 12, 12, 12)),
            ),
    )
}

// ─── binding ────────────────────────────────────────────────────────────────

struct LazyTreeView;

impl WidgetCore for LazyTreeView {
    type State = ();
    type Event = ();

    fn tag() -> &'static str {
        ROOT_TAG
    }

    fn title() -> &'static str {
        "pinion hello-lazy-tree (R942 §5.22 §5.23 §5.27)"
    }

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    /// Register the [`TreeRowClickExternal`] composite-tag click router (the
    /// expand affordance) + the query-only tree-state introspection extra. The
    /// expand-driven loader `Effect` is installed first so the data layer is
    /// live before the first paint and the fetch never runs in `view`.
    /// `create_extra_externals` runs inside the root owner scope, so the
    /// `use_*` handles resolve here ([[callback-root-owner-wrap]]); the
    /// introspection closure captures them (rather than re-resolving via
    /// `Owner::current` at query time, which has no owner scope).
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let _loader = install_loader();
        let expanded = use_expanded();
        let cache = use_children_cache();
        vec![
            ExtraExternal::new(ROW_PREFIX, Box::new(TreeRowClickExternal::new())),
            tree_view_introspection_extra(
                STATE_TAG,
                move || node_rows(&compute_rows(&expanded, &cache)),
                || None,
            ),
        ]
    }

    fn read_state(_scene: &Scene) {}

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    /// Pointer / RPC driven: no keyboard tab stop (expansion is by click on a
    /// branch row or the `click` typed shortcut). Keyboard tree navigation
    /// over a lazy tree is a documented follow-up.
    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only async outliner (no widget state)".to_owned()
    }

    /// Bridge the [`TreeRowClickExternal`] `click` intent into the expand-set
    /// toggle. Side-effect-only ([[scxml-as-model-update-transient]]): the
    /// `Signal::set` inside [`toggle_expanded`] is the mutation. Leaves never
    /// toggle ([`id_is_branch`] gate), matching tree click semantics.
    fn update(_state: (), intent: &Intent) -> Vec<Command> {
        if intent.tag_str() == CLICK_INTENT_TAG
            && let IntrospectValue::Text(id) = &intent.payload
            && id_is_branch(id)
        {
            toggle_expanded(&use_expanded(), id);
        }
        Vec::new()
    }
}

impl WidgetA11y for LazyTreeView {
    /// The WAI-ARIA `tree` + per-row `treeitem` semantic tree (the lifted
    /// [`tree_access_nodes`] builder, fed the loaded rows), plus a `status`
    /// live region for the async load announcement. Skeleton rows are *not*
    /// `treeitem`s — a loading child count is unknown, so a phantom item would
    /// misreport `aria-setsize`; the load state is conveyed by the live region
    /// and the rendered placeholder text instead. There is no selection model
    /// and no keyboard cursor, so `selected_id` / `focused_id` are `None`.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let all = compute_rows(&use_expanded(), &use_children_cache());
        let rows = node_rows(&all);
        let mut nodes =
            tree_access_nodes(ROOT_TAG, ROW_PREFIX, Some("Asset outliner"), &rows, None, None);
        nodes.push(AccessNode::new(STATUS_TAG, AriaRole::Status).with_name(status_text(&all)));
        nodes
    }
}

impl WidgetView for LazyTreeView {
    type Renderer = HelloLazyTreeRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<LazyTreeView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the owner-scoped `LocalTaskPump` to completion — what the shell
    /// does once per frame. A fetch is deferred `FETCH_LATENCY_POLLS` polls.
    fn drain_pump() {
        for _ in 0..(FETCH_LATENCY_POLLS + 8) {
            if !use_local_task_pump().poll() {
                break;
            }
        }
    }

    fn boot() {
        let _ = install_loader();
    }

    fn rows_now() -> Vec<LazyRow> {
        compute_rows(&use_expanded(), &use_children_cache())
    }

    fn visible_ids() -> Vec<String> {
        node_rows(&rows_now()).into_iter().map(|r| r.id).collect()
    }

    fn any_skeleton(rows: &[LazyRow]) -> bool {
        rows.iter().any(|r| matches!(r, LazyRow::Skeleton { .. }))
    }

    #[test]
    fn id_is_branch_is_pure_and_terminates() {
        assert!(id_is_branch(""), "synthetic root is a branch");
        assert!(id_is_branch("0"), "even root-level node is a branch");
        assert!(!id_is_branch("1"), "odd sibling is a leaf");
        // Depth 3 children are leaves regardless of index (the tree terminates).
        assert!(!id_is_branch("0/0/0/0"), "depth-3 node is a leaf");
        // child_nodes stamps is_branch from the same SSOT.
        for child in child_nodes("0") {
            assert_eq!(child.is_branch, id_is_branch(&child.id), "stamp matches SSOT");
        }
    }

    #[test]
    fn boot_shows_skeleton_then_root_children() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            // Before the pump drains, the root is Loading → one skeleton, no
            // loaded rows.
            let pending = rows_now();
            assert!(any_skeleton(&pending), "root loads behind a skeleton at boot");
            assert!(node_rows(&pending).is_empty(), "no loaded rows while root loads");

            drain_pump();
            let ready = rows_now();
            assert!(!any_skeleton(&ready), "root resolved → no skeleton");
            assert_eq!(node_rows(&ready).len(), 3, "3 top-level nodes after resolve");
            assert_eq!(visible_ids(), vec!["0", "1", "2"], "root children ids");
        });
    }

    #[test]
    fn expanding_a_branch_lazy_loads_then_collapse_retains() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            assert!(id_is_branch("0"), "node 0 is a branch in this fixture");

            // Expand node 0 → its children load behind a skeleton.
            toggle_expanded(&use_expanded(), "0");
            let loading = rows_now();
            assert!(
                any_skeleton(&loading),
                "freshly-expanded branch shows a skeleton while its children fetch",
            );
            assert!(
                !visible_ids().iter().any(|id| id.starts_with("0/")),
                "no children visible until the fetch resolves",
            );

            drain_pump();
            let ready = rows_now();
            assert!(!any_skeleton(&ready), "children resolved → skeleton gone");
            assert!(
                visible_ids().iter().any(|id| id == "0/0"),
                "node 0's children appear after the lazy fetch: {:?}",
                visible_ids(),
            );

            // Collapse → children hidden but the cache retains them.
            toggle_expanded(&use_expanded(), "0");
            assert!(
                !visible_ids().iter().any(|id| id.starts_with("0/")),
                "collapse hides the children",
            );

            // Re-expand → children reappear with NO skeleton (cache hit, no
            // re-fetch): the retained child set resolves the same frame.
            toggle_expanded(&use_expanded(), "0");
            let recached = rows_now();
            assert!(
                !any_skeleton(&recached),
                "re-expanding a cached branch shows no skeleton (no re-fetch)",
            );
            assert!(
                visible_ids().iter().any(|id| id == "0/0"),
                "cached children reappear immediately on re-expand",
            );
        });
    }
}
