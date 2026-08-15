//! R942 R946 §5.16 §5.22 §5.23 §5.27 §2 — `hello-lazy-tree`: a
//! scene-outliner / asset browser over a tree whose **children are fetched
//! asynchronously on expand** *and* whose rows are **virtualized** — the World
//! Outliner at scale.
//!
//! The Model/View-at-scale axis virtualizes a fully-materialized tree
//! (`hello-virtual-tree`, R819), lazily pages a *flat* list (`hello-lazy-list`,
//! R924), and reparents an in-memory tree (`hello-tree-reparent`). This binding
//! **combines** the first two over a tree: a structure that is *both*
//! out-of-memory (each branch's children arrive asynchronously the first time
//! it is expanded, the way a real asset browser reads a directory or a scene
//! graph streams a sub-hierarchy) *and* large (R946 — a top-level section is a
//! folder of hundreds of entries, so only the ~viewport-sized scroll window
//! ever becomes scene nodes). That combination — lazy + windowed — is what the
//! self-hosted editor's World Outliner needs (100k-actor scenes, streamed and
//! windowed); R942 reserved it for a fresh full-budget round.
//!
//! ## Architecture (unidirectional, Effect-driven fetch-on-expand)
//!
//! - A per-node child cache — [`ResourceCache`]
//!   keyed by node id; only expanded branches are ever fetched, and each
//!   branch's children carry their own reactive [`ResourceState`].
//! - The expand set is a `Signal<BTreeSet<String>>`. Clicking a branch row
//!   (the [`TreeRowClickExternal`] composite-tag router) toggles its id.
//! - A single lifetime-held [`Effect`] subscribes to the expand set and, on
//!   every change (and its eager boot run), calls
//!   [`ResourceCache::ensure`] for the root plus each expanded branch not yet
//!   cached, through the shell-polled
//!   [`LocalTaskPump`]. The fetch never runs in
//!   the view, so view-fn purity holds (§6.3).
//! - The view snapshots the root + every expanded branch's child state and
//!   *flattens* the visible tree: a `Ready` branch contributes its children
//!   (recursing into the expanded ones); a branch still `Loading` contributes
//!   one skeleton placeholder. This async flattening is the example-local
//!   machinery — the in-memory `flat_visible` SSOT walks a `&[TreeNode]`
//!   slice, which a lazily-fetched tree cannot provide.
//!
//! This is the same Resource + Effect + `ResourceCache` + `DeferredReady` +
//! pump substrate R923/R924 introduced (asset browser / lazy list), now keyed
//! on expand instead of scroll, over a tree instead of a flat list.
//!
//! ## Virtualization (R946 — the genuinely-new axis)
//!
//! The lazy flattening — a heterogeneous `Vec<LazyRow>` of loaded node rows and
//! transient skeletons — is fed to [`view_flex_virtual_list`] (the closure
//! windowing path, `AutoSizer`-measured), so only the rows inside the scroll
//! window become scene nodes. This is the one piece the eager windowed tree
//! ([`view_virtual_tree`](pinion_widget_paint::tree_view::view_virtual_tree))
//! cannot do: that helper renders a uniform `&[VisibleRow]`, whereas a lazy
//! sequence interleaves skeleton placeholders, so the example windows the
//! `LazyRow` sequence itself (Node → row paint, Skeleton → placeholder). Every
//! other piece is N-th-consumer reuse: the AT treeitems window with the paint
//! (`hello-virtual-tree` precedent), keyboard roving scrolls the cursor into
//! the window ([`scroll_offset_to_reveal`]), and `scene/query` reports the full
//! flattening regardless of which window paints (§2 #7 — the AI omniscience a
//! windowed AT tree alone cannot give).
//!
//! ## Keyboard navigation (R944 — WAI-ARIA APG 6.13 Tree)
//!
//! The tree root is a single tab stop; while it holds focus the arrow keys
//! rove a keyboard cursor (`Signal<Option<String>>`) over the loaded rows via
//! the pure [`resolve_tree_key`] resolver — the third external-overlay
//! consumer of the `tree_nav` substrate (the inspector is the second). The one
//! axis new to a *lazy* tree: an Arrow Right that expands a not-yet-loaded
//! branch flows through the **same** expand-set mutation a click does, so the
//! existing fetch-on-expand `Effect` fires — keyboard expansion lazy-loads
//! exactly as clicking does. The cursor is conveyed as `aria-activedescendant`
//! plus an M3 focus state-layer, both gated through [`effective_cursor`] so a
//! collapsed-away cursor never dangles. R946 — because the rows are now
//! windowed, a roving move scrolls the cursor's row into the rendered window
//! ([`reveal_lazy_cursor`]), so it stays realized.
//!
//! ## ZERO-FLAKE latency model
//!
//! Each child fetch is a deterministic [`DeferredReady`] future (`Pending`
//! [`FETCH_LATENCY_POLLS`] times, then `Ready`). Every `scene/snapshot
//! from=paint` advances the pump one step (the poll lives in the shell paint
//! path), so the demo's own snapshot polling drives `skeleton → rows` with no
//! wall-clock race — `wait_snap` on the skeleton is guaranteed to catch it
//! before the children resolve (same discipline as the R923 deferred demo).

use pinion_a11y::{AccessFocus, AccessNode, AriaRole, WidgetA11y, tree_access_nodes, tree_row_tag};
use pinion_core::command::Command;
use pinion_core::external::{External, IntrospectValue, StubExternal};
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::reactive::{DeferredReady, Effect, Owner, ResourceCache, ResourceState};
use pinion_core::scene::{ContainerNode, Rect, TextNode, TextRole};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::{ScrollState, use_scroll_state};
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::tree_nav::{
    TreeKey, VisibleRow, resolve_tree_key, tree_view_introspection_extra,
};
use pinion_core::widgets::virtual_list::{compute_visible_range, scroll_offset_to_reveal};
use pinion_core::{Frame, LocalTaskPump, Scene, Signal, WidgetCore, use_local_task_pump};
use pinion_shell::typeahead::tree_typeahead_jump;
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::glyph::{DISCLOSURE_COLLAPSED, DISCLOSURE_EXPANDED};
use pinion_widget_paint::scrollbar::{VerticalScrollbarStyle, view_vertical_scrollbar};
use pinion_widget_paint::tree_view::{TreeRowClickExternal, TreeViewStyle, row_focus_bg};
use pinion_widget_paint::virtual_list::view_flex_virtual_list;
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
/// R946 — the body `ScrollState` cache key + the painted `Scene::Scroll` tag,
/// shared with the scrollbar peer and the keyboard scroll-into-view.
const SCROLL_KEY: &str = "lazytree_scroll";
/// R946 — the vertical scrollbar peer tag (shares [`SCROLL_KEY`]'s state).
const SCROLLBAR_TAG: &str = "lazytree_scrollbar";
/// R946 — gutter reserved on the right for the scrollbar peer.
const SCROLLBAR_W: u32 = 12;
/// R946 — rows rendered beyond the strict scroll window on each side (the
/// virtualization overscan; matches `hello-virtual-tree`).
const OVERSCAN: usize = 4;

/// R946 — the uniform per-row vertical pitch the windowing geometry uses: the
/// tree row height. One source so the paint window, the a11y window, and the
/// keyboard scroll-into-view all derive the same pitch (a divergence would
/// scroll the cursor to the wrong row or window the AT off by rows).
fn tree_pitch() -> u32 {
    TreeViewStyle::default().row_height
}

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

/// Top-level outliner sections (the synthetic root's children) — a handful of
/// asset categories ("Scenes", "Models", …); the boot listing.
const TOP_SECTIONS: usize = 6;
/// R946 — a top-level section is a *large* asset folder: hundreds of entries,
/// so expanding one floods the visible-row count far past the rendered window.
/// This is the virtualization-at-scale case the self-hosted editor's World
/// Outliner needs (a folder of hundreds of meshes, only ~viewport rows ever
/// becoming scene nodes). Deterministic (base + an id-byte spread), so every
/// run materialises the identical tree (ZERO-FLAKE) — no RNG.
const LARGE_FOLDER_BASE: usize = 360;
/// The per-section spread added to [`LARGE_FOLDER_BASE`] (`base ..= base +
/// spread - 1` entries), varied by the section id's bytes.
const LARGE_FOLDER_SPREAD: usize = 80;

/// Leaf disclosure-column placeholder (NO-BREAK SPACE) — keeps the label
/// column of leaves aligned with branches' triangles
/// ([[non-ascii-literal-named-const-escape]]).
const GLYPH_LEAF: &str = "\u{00A0}";
/// The skeleton placeholder label (`Loading…`).
const LOADING_LABEL: &str = "Loading\u{2026}";
/// Conceptual label of the synthetic root, used in the boot status line.
const ROOT_LABEL: &str = "assets";

const FOLDER_WORDS: [&str; 6] = [
    "Scenes",
    "Models",
    "Textures",
    "Materials",
    "Audio",
    "Prefabs",
];
const FILE_WORDS: [&str; 6] = ["mesh", "albedo", "normal", "clip", "shader", "rig"];
const FILE_EXT: [&str; 6] = ["scene", "fbx", "png", "wav", "wgsl", "anim"];

/// `Owner::cache` keys for the shared per-binding state.
const CACHE_KEY: &str = "lazytree.children_cache";
const EXPANDED_KEY: &str = "lazytree.expanded";
const LOADER_KEY: &str = "lazytree.loader";
/// `Owner::cache` key for the keyboard nav cursor (`Signal<Option<String>>`).
const CURSOR_KEY: &str = "lazytree.cursor";
/// `Owner::cache` key for the type-ahead search cursor
/// ([`tree_typeahead_jump`]'s only caller-side bit).
const TYPEAHEAD_KEY: &str = "lazytree.typeahead";
/// Page Up/Down jump size — larger than the boot listing so the page arm
/// clamps to an end (mirrors `hello-tree-view`'s `NAV_PAGE`).
const NAV_PAGE: usize = 8;

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

/// How many children `parent_id` has — [`TOP_SECTIONS`] at the root; a *large*
/// folder ([`LARGE_FOLDER_BASE`]..) for a top-level section so virtualization is
/// observable at scale (R946); else a deterministic 2..=4 from the id bytes.
/// No RNG, so every run is identical.
fn child_count(parent_id: &str) -> usize {
    if parent_id.is_empty() {
        TOP_SECTIONS
    } else if !parent_id.contains('/') {
        // A top-level section: a large asset folder (hundreds of entries) — the
        // scale case. Expanding it yields far more visible rows than the window.
        LARGE_FOLDER_BASE + parent_id.bytes().map(usize::from).sum::<usize>() % LARGE_FOLDER_SPREAD
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
            LazyChild {
                id,
                label,
                is_branch,
            }
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

/// The keyboard navigation cursor — the visible-row id the WAI-ARIA tree's
/// roving cursor sits on (conveyed as `aria-activedescendant`, not a
/// selection). `None` until the first vertical key lands it on a row.
fn use_cursor() -> Rc<Signal<Option<String>>> {
    let owner = Owner::current().expect("use_cursor requires an active Owner scope");
    owner.cache(CURSOR_KEY, || Signal::new(None))
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

/// Set `id`'s membership in the expand set — insert to expand, remove to
/// collapse — writing the `Signal` only on an actual change (no redundant
/// repaint). The keyboard Arrow Right / Arrow Left write side, and the
/// expand-set peer of the inspector's `set_collapsed`; the same expand-set
/// mutation the click path's [`toggle_expanded`] drives, so a keyboard expand
/// of a not-yet-loaded branch fires the very same fetch-on-expand `Effect`.
fn set_expanded(expanded: &Signal<BTreeSet<String>>, id: &str, want: bool) {
    let mut set = expanded.get();
    let changed = if want {
        set.insert(id.to_owned())
    } else {
        set.remove(id)
    };
    if changed {
        expanded.set(set);
    }
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

/// The keyboard cursor id, gated to a still-visible row via the lifted
/// [`tree_nav::effective_cursor`](pinion_core::widgets::tree_nav::effective_cursor)
/// SSOT (the `Signal` read stays caller-side — the pure gate is shared with the
/// dock-panels inspector, R945.1). Reading the cursor through this gate everywhere
/// (paint, a11y, `scene/query`) keeps the three in agreement and self-heals:
/// once the cursor row is gone the resolver sees no current row, so the next
/// vertical key restarts navigation at the first visible row.
fn effective_cursor(rows: &[VisibleRow], cursor: &Signal<Option<String>>) -> Option<String> {
    let id = cursor.get()?;
    pinion_core::widgets::tree_nav::effective_cursor(rows, Some(&id)).map(ToOwned::to_owned)
}

/// Whose children are in flight, or `None` when nothing is being fetched.
///
/// ★ R1693.1 — **one predicate, two announcements.** The band says it in words
/// and the tree lowers the same moment to `aria-busy`; a screen that carried one
/// and not the other would be telling a reader two different things about the
/// same instant. The tree at boot held no `treeitem` and said nothing about why,
/// which the structural census reports as a forgotten collection — and it is not
/// one: it is a collection that has asked and is waiting.
fn loading_under(rows: &[LazyRow]) -> Option<String> {
    rows.iter().find_map(|r| match r {
        LazyRow::Skeleton { parent_label, .. } => Some(parent_label.clone()),
        LazyRow::Node(_) => None,
    })
}

/// The `role=status` band text: the loading branch while children are in
/// flight (the first skeleton's parent), else the loaded visible-row count.
fn status_text(rows: &[LazyRow]) -> String {
    if let Some(label) = loading_under(rows) {
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
        cells.push(Scene::Container(
            ContainerNode::new(Vec::new())
                .with_layout(LayoutStyle::new().with_size(Size::px(indent_px, style.row_height))),
        ));
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
    cells.push(Scene::Container(
        ContainerNode::new(vec![glyph_node]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_size(Size::px(style.glyph_size_px, style.row_height)),
        ),
    ));
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
/// it and the a11y tree + bounds enrichment attach. The row holding the
/// keyboard cursor fills with the shared M3 focus state-layer
/// ([`row_focus_bg`]) so the roving cursor is visible (WCAG 2.4.7); the rest
/// stay transparent.
fn node_row_view(
    row: &VisibleRow,
    cursor: Option<&str>,
    theme: &Theme,
    style: &TreeViewStyle,
) -> Scene {
    let glyph = if !row.has_children {
        GLYPH_LEAF
    } else if row.expanded {
        DISCLOSURE_EXPANDED
    } else {
        DISCLOSURE_COLLAPSED
    };
    let is_focused = cursor == Some(row.id.as_str());
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
        .with_layout(row_layout(style))
        .with_style(BoxStyle::filled(row_focus_bg(theme, is_focused))),
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

fn row_view(row: &LazyRow, cursor: Option<&str>, theme: &Theme, style: &TreeViewStyle) -> Scene {
    match row {
        LazyRow::Node(vr) => node_row_view(vr, cursor, theme, style),
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
    // The keyboard cursor, gated to a still-visible row so a collapsed-away
    // cursor never paints an orphaned focus highlight ([[effective_cursor]]).
    let cursor = effective_cursor(&node_rows(&rows), &use_cursor());

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

    // R946 — virtualized paint: only the rows inside the scroll window become
    // scene nodes, derived from the *whole* lazy flattening's length (loaded
    // node rows + transient skeletons alike, since both occupy one uniform-pitch
    // slot). The closure dispatches each windowed index back to the same
    // [`row_view`] the non-windowed era used, so a windowed row is byte-identical
    // to its eager counterpart — only the slot count differs. Skeletons inside
    // the window paint as placeholders; off-window rows never exist as nodes.
    let scroll = use_scroll_state(SCROLL_KEY);
    let pitch = tree_pitch();
    let tree = view_flex_virtual_list(&scroll, rows.len(), pitch, OVERSCAN, |i| {
        row_view(&rows[i], cursor.as_deref(), &theme, &style)
    });

    // The scrollbar peer shares the body `ScrollState` (its thumb reads the
    // measured viewport against the full content height the windowing sizer
    // reserves), so a huge lazy folder gets a proportional grip.
    let (_, measured_h) = scroll.measured_viewport();
    let scrollbar_style = VerticalScrollbarStyle::material(measured_h, SCROLLBAR_TAG);
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll,
        &theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );

    // Row band: the windowed tree beside its scrollbar peer, flex-grow so it
    // fills the window below the title + status. The measured-viewport
    // windowing reads its height from this flex-computed band (the `AutoSizer`).
    // It carries ROOT_TAG — the WAI-ARIA `tree` root + primary `StubExternal`
    // anchor + keyboard-focus surface. A real sized node (not the old visible
    // rows container, now moved into the scroll, nor a 0x0 placeholder a key /
    // hit-test route cannot reach): the tree region itself is the focus surface.
    let band = Scene::Container(
        ContainerNode::new(vec![tree, scrollbar_visual])
            .with_tag(ROOT_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_flex_grow(1.0)
                    .with_focusable(true),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![title, status, band])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_size(Size::px(WIN_W, WIN_H))
                    .with_gap(10)
                    .with_padding(Rect::new(12, 12, 12 + SCROLLBAR_W, 12)),
            ),
    )
}

// ─── keyboard navigation ────────────────────────────────────────────────────

/// Apply one key to the lazy tree — WAI-ARIA APG 6.13 Tree navigation over the
/// pure [`resolve_tree_key`] resolver, the third **external-overlay** consumer
/// of the `tree_nav` substrate (`hello-dock-panels`' inspector is the second;
/// both drive the resolver directly and apply the [`TreeKey`] outcome to their
/// own store — here the expand-set `Signal` + the nav cursor — rather than the
/// flag-on-node `apply_tree_key` the retained trees use). Returns `true` when
/// the key was recognised so the shell's swallow contract stays exact.
///
/// The one axis new to a *lazy* tree: an Arrow Right that expands a
/// not-yet-loaded branch routes through the same [`set_expanded`] mutation the
/// click path drives, so the existing fetch-on-expand `Effect` fires — keyboard
/// expansion lazy-loads exactly as a click does. The resolver only ever moves
/// the cursor onto a loaded (semantic) row, so a branch still fetching its
/// children contributes no cursor target until it resolves.
fn apply_key_impl(key: &str) -> bool {
    let expanded = use_expanded();
    let cache = use_children_cache();
    let cursor = use_cursor();
    // The whole lazy flattening (incl. skeletons) for the scroll-into-view
    // painted-position math; the loaded node rows for the resolver / type-ahead.
    let all = compute_rows(&expanded, &cache);
    let rows = node_rows(&all);
    let current = cursor.get();
    let outcome = resolve_tree_key(&rows, current.as_deref(), key, NAV_PAGE);
    // R945.1 — guard the lazy descent: an expanded branch whose children are
    // still loading has no child rows in the semantic set yet, so the resolver's
    // "Arrow Right on an open branch → next preorder row" would focus the next
    // *sibling*. A genuine descent only ever moves to a deeper row; when the
    // target is not deeper than the cursor the children have not arrived, so
    // stay put (Consumed) until the fetch resolves.
    if key == "ArrowRight"
        && let TreeKey::Focus(target) = &outcome
        && let Some(cur) = current.as_deref()
        && let (Some(ci), Some(ti)) = (
            rows.iter().position(|r| r.id == cur),
            rows.iter().position(|r| r.id == *target),
        )
        && rows[ti].depth <= rows[ci].depth
    {
        return true; // Consumed — the not-yet-loaded children own the descent
    }
    match outcome {
        TreeKey::Focus(id) => {
            cursor.set(Some(id));
            // R946 — keyboard ⊥ virtualization: a roving move may target a row
            // outside the rendered window; scroll it in so the cursor (and its
            // `aria-activedescendant`) stays realized.
            reveal_lazy_cursor(
                &all,
                cursor.get().as_deref(),
                &use_scroll_state(SCROLL_KEY),
                tree_pitch(),
            );
            true
        }
        TreeKey::Expand(id) => {
            set_expanded(&expanded, &id, true);
            true
        }
        TreeKey::Collapse(id) => {
            set_expanded(&expanded, &id, false);
            true
        }
        TreeKey::Toggle(id) => {
            toggle_expanded(&expanded, &id);
            true
        }
        TreeKey::Consumed => true,
        // Unrecognised printable key → type-ahead jump over the visible labels
        // (the shared `pinion-shell` SSOT; the cursor cache key is the only
        // caller-side bit). A jump that moves the cursor scrolls it into view.
        TreeKey::Unhandled => {
            let jumped = tree_typeahead_jump(&cursor, &rows, TYPEAHEAD_KEY, key);
            if jumped {
                reveal_lazy_cursor(
                    &all,
                    cursor.get().as_deref(),
                    &use_scroll_state(SCROLL_KEY),
                    tree_pitch(),
                );
            }
            jumped
        }
    }
}

/// R946 — scroll the keyboard cursor's row into the rendered window over the
/// *lazy* flattening. The cursor only ever lands on a loaded node row, so its
/// painted slot is its index in the heterogeneous [`LazyRow`] sequence
/// (transient skeletons occupy slots too, so an index over [`node_rows`] alone
/// would be off by the skeletons above it). Reuses the
/// [`scroll_offset_to_reveal`] pixel-math SSOT — the lazy peer of the lifted
/// [`reveal_row_cursor`](pinion_core::widgets::tree_nav::reveal_row_cursor),
/// which walks `&[VisibleRow]`, a shape a skeleton-interleaved sequence cannot
/// supply; the shared geometry is the free fn both call (so the row-type
/// adapter stays caller-side, R945.1's `effective_cursor` granularity).
fn reveal_lazy_cursor(rows: &[LazyRow], cursor: Option<&str>, scroll: &ScrollState, pitch: u32) {
    let Some(id) = cursor else { return };
    let Some(index) = rows
        .iter()
        .position(|r| matches!(r, LazyRow::Node(vr) if vr.id == id))
    else {
        return;
    };
    let (_, measured_h) = scroll.measured_viewport();
    let offset = scroll_offset_to_reveal(index, scroll.offset_y(), measured_h, pitch);
    scroll.scroll_to(0, offset);
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
        "pinion hello-lazy-tree (R942 R946 §5.16 §5.22 §5.23 §5.27)"
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
        let cursor = use_cursor();
        let rows_expanded = expanded.clone();
        let rows_cache = cache.clone();
        vec![
            ExtraExternal::new(ROW_PREFIX, Box::new(TreeRowClickExternal::new())),
            // R946 — the scrollbar peer shares the body scroll state (the grip
            // is the only handle on a huge lazy folder).
            scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG),
            tree_view_introspection_extra(
                STATE_TAG,
                // R946 — `scene/query` reports the *whole* lazy flattening's
                // node rows (`row_count` / `id_at` over every loaded row), not
                // just the painted window: the AI-omniscient structure surface
                // (§2 #7) that paint-scraping a virtualized tree cannot reach.
                move || node_rows(&compute_rows(&rows_expanded, &rows_cache)),
                // The live keyboard cursor, gated to a visible row so
                // `scene/query` never reports a collapsed-away cursor (the
                // same `effective_cursor` SSOT the paint + a11y read).
                move || effective_cursor(&node_rows(&compute_rows(&expanded, &cache)), &cursor),
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

    /// R944 §5.22 §5.27 — WAI-ARIA APG 6.13 Tree keyboard navigation, gated on
    /// the tree root holding focus (routing and focus are separate axes — the
    /// gate keeps the tree from stealing keys when embedded beside a sibling
    /// focusable; the RPC demo issues `focus/set` first). Arrow keys rove the
    /// cursor; Arrow Right on a collapsed branch lazy-loads through the same
    /// expand path as a click. The whole keymap is the pure
    /// [`apply_key_impl`] bridge.
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

    fn fmt_state_log(_state: &()) -> String {
        "display-only async outliner (no widget state)".to_owned()
    }

    /// Bridge the [`TreeRowClickExternal`] `click` intent into the expand-set
    /// toggle (the pointer path; the keyboard path is [`apply_key_impl`], which
    /// drives the same expand set). Side-effect-only
    /// ([[scxml-as-model-update-transient]]): the `Signal::set` inside
    /// [`toggle_expanded`] is the mutation. Leaves never toggle
    /// ([`id_is_branch`] gate), matching tree click semantics.
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
    /// (`selected_id` is `None`); the keyboard cursor is conveyed as
    /// `focused_id` (the row's `with_focused`) and, in
    /// [`Self::access_focus_target`], as the tree's `aria-activedescendant`.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let all = compute_rows(&use_expanded(), &use_children_cache());
        // R946 — window the AT treeitems with the paint: only the loaded node
        // rows inside the current scroll window become `treeitem`s (the
        // `hello-virtual-tree` windowed-AT precedent), so a screen reader walks
        // exactly the realized rows. Each carries its own `aria-posinset` /
        // `aria-setsize` from the flatten (e.g. "row 200 of 408"), correct even
        // when the window cuts a sibling group. The full structure stays
        // reachable through the query-only introspection extra. The cursor's
        // `aria-activedescendant` (in [`Self::access_focus_target`]) may name a
        // row currently off the window — e.g. a wheel scroll moves the viewport
        // without moving the cursor. R947.1 corrected the earlier claim that
        // `effective_cursor`'s in-flatten guard alone prevents a dangling
        // reference: it does not (off-window-but-in-flatten still excludes the
        // row from this windowed node set). The actual guard is the shell's
        // `AccessTreeBuilder::build`, which now drops an active-descendant tag
        // absent from the emitted nodes to atomic root focus (symmetric with
        // the focus filter) — so no dangling reference, here or in any windowed
        // roving widget. A keyboard move additionally scrolls the cursor back
        // into view ([`reveal_lazy_cursor`]), so the off-window state is
        // transient under keyboard nav.
        let cursor = effective_cursor(&node_rows(&all), &use_cursor());
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(
            scroll.offset_y(),
            measured_h,
            all.len(),
            tree_pitch(),
            OVERSCAN,
        );
        let windowed: Vec<VisibleRow> = all[window.first..window.first + window.count]
            .iter()
            .filter_map(|r| match r {
                LazyRow::Node(vr) => Some(vr.clone()),
                LazyRow::Skeleton { .. } => None,
            })
            .collect();
        let mut nodes = tree_access_nodes(
            ROOT_TAG,
            ROW_PREFIX,
            Some("Asset outliner"),
            &windowed,
            None,
            cursor.as_deref(),
        );
        // ★★★ R1693.1 — while a fetch is in flight the tree holds no `treeitem`,
        // and that is neither full nor empty. `aria-busy` is the third answer,
        // and it is the same moment [`status_text`] puts into words — read from
        // the one predicate so the two cannot disagree. Before this the opening
        // screen announced a `tree` a reader could not enter, with nothing
        // saying why.
        if loading_under(&all).is_some()
            && let Some(root) = nodes.first_mut()
        {
            root.busy = true;
        }
        nodes.push(AccessNode::new(STATUS_TAG, AriaRole::Status).with_name(status_text(&all)));
        nodes
    }

    /// R944 §5.27 §5.40 — the keyboard cursor lowers to `aria-activedescendant`
    /// (composite focus + the cursor row's `with_focused`) while the tree root
    /// holds focus, carrying no `aria-selected` (no selection model). Gated on
    /// the cursor still naming a visible row ([`effective_cursor`]) so a
    /// collapse never advertises a dangling descendant — the same dangling
    /// pointer R943.1 cleared for the data-grid Color popup.
    fn access_focus_target(_state: &(), focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(ROOT_TAG) {
            let rows = node_rows(&compute_rows(&use_expanded(), &use_children_cache()));
            if let Some(cursor) = effective_cursor(&rows, &use_cursor()) {
                return Some(AccessFocus::composite(
                    ROOT_TAG,
                    tree_row_tag(ROW_PREFIX, &cursor),
                ));
            }
        }
        focused.map(AccessFocus::atomic)
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
            assert_eq!(
                child.is_branch,
                id_is_branch(&child.id),
                "stamp matches SSOT"
            );
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
            assert!(
                any_skeleton(&pending),
                "root loads behind a skeleton at boot"
            );
            assert!(
                node_rows(&pending).is_empty(),
                "no loaded rows while root loads"
            );

            drain_pump();
            let ready = rows_now();
            assert!(!any_skeleton(&ready), "root resolved → no skeleton");
            assert_eq!(
                node_rows(&ready).len(),
                TOP_SECTIONS,
                "top-level section count after resolve",
            );
            assert_eq!(
                visible_ids(),
                vec!["0", "1", "2", "3", "4", "5"],
                "root children ids",
            );
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

    // ─── keyboard navigation (R944) ─────────────────────────────────────────

    fn cursor_now() -> Option<String> {
        use_cursor().get()
    }

    /// The keyboard cursor gated through the visible-row check the paint /
    /// a11y / query surfaces all read.
    fn effective_cursor_now() -> Option<String> {
        effective_cursor(&node_rows(&rows_now()), &use_cursor())
    }

    #[test]
    fn arrows_rove_the_cursor_over_loaded_rows() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            assert_eq!(cursor_now(), None, "no cursor until the first key");

            // First Arrow Down from an empty cursor lands on the first row.
            assert!(apply_key_impl("ArrowDown"), "ArrowDown is a nav key");
            assert_eq!(cursor_now().as_deref(), Some("0"), "lands on first row");
            apply_key_impl("ArrowDown");
            assert_eq!(cursor_now().as_deref(), Some("1"), "steps to next row");
            apply_key_impl("ArrowUp");
            assert_eq!(cursor_now().as_deref(), Some("0"), "steps back");
            apply_key_impl("ArrowUp"); // clamp at top (no wrap)
            assert_eq!(cursor_now().as_deref(), Some("0"), "clamps at first row");
            apply_key_impl("End");
            assert_eq!(cursor_now().as_deref(), Some("5"), "End → last row");
            apply_key_impl("ArrowDown"); // clamp at bottom
            assert_eq!(cursor_now().as_deref(), Some("5"), "clamps at last row");
            apply_key_impl("Home");
            assert_eq!(cursor_now().as_deref(), Some("0"), "Home → first row");

            // A non-navigation key is not swallowed (falls through to nothing
            // here — there is no label starting with the digit it types).
            assert!(!apply_key_impl("F1"), "an unrecognised key is unhandled");
        });
    }

    #[test]
    fn arrow_right_on_a_collapsed_branch_lazy_loads() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            apply_key_impl("ArrowDown"); // cursor → "0" (a branch)
            assert_eq!(cursor_now().as_deref(), Some("0"), "cursor on the branch");

            // Arrow Right on a collapsed branch expands it through the SAME
            // expand-set the click path drives → the fetch-on-expand Effect
            // fires, so the children load behind a skeleton.
            assert!(apply_key_impl("ArrowRight"), "ArrowRight is a nav key");
            assert!(
                use_expanded().get().contains("0"),
                "branch added to expand set"
            );
            assert!(
                any_skeleton(&rows_now()),
                "keyboard expand shows the loading skeleton"
            );
            assert_eq!(
                cursor_now().as_deref(),
                Some("0"),
                "cursor stays on the branch"
            );

            drain_pump();
            assert!(
                !any_skeleton(&rows_now()),
                "children resolved → skeleton gone"
            );
            assert!(
                visible_ids().iter().any(|id| id == "0/0"),
                "children lazy-loaded"
            );

            // Arrow Right again (now expanded) descends to the first child.
            apply_key_impl("ArrowRight");
            assert_eq!(
                cursor_now().as_deref(),
                Some("0/0"),
                "descends to first child"
            );

            // Arrow Left on the expanded child's branch collapses; here the
            // child "0/0" is itself a branch but collapsed, so Arrow Left
            // ascends to its parent "0".
            apply_key_impl("ArrowLeft");
            assert_eq!(
                cursor_now().as_deref(),
                Some("0"),
                "ArrowLeft ascends to parent"
            );
            // Arrow Left on the expanded parent collapses it.
            apply_key_impl("ArrowLeft");
            assert!(
                !use_expanded().get().contains("0"),
                "ArrowLeft collapses the branch"
            );
            assert!(
                !visible_ids().iter().any(|id| id.starts_with("0/")),
                "collapsed branch hides its children",
            );
        });
    }

    #[test]
    fn arrow_right_on_a_loading_branch_does_not_descend_to_a_sibling() {
        // R945.1 — a branch expanded but still fetching its children has no
        // child rows in the semantic set yet; ArrowRight must stay put, not
        // jump to the next sibling (the skeleton is filtered from node_rows, so
        // the resolver's "next preorder row" would otherwise be sibling "1").
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            apply_key_impl("ArrowDown"); // cursor → "0"
            apply_key_impl("ArrowRight"); // expand "0" → children loading
            assert!(use_expanded().get().contains("0"), "branch expanded");
            assert!(any_skeleton(&rows_now()), "children still loading");
            apply_key_impl("ArrowRight"); // while loading
            assert_eq!(
                cursor_now().as_deref(),
                Some("0"),
                "cursor stays on the loading branch, never the next sibling",
            );
            // Once the children resolve, ArrowRight descends to the first child.
            drain_pump();
            apply_key_impl("ArrowRight");
            assert_eq!(
                cursor_now().as_deref(),
                Some("0/0"),
                "descends once children load"
            );
        });
    }

    #[test]
    fn space_and_enter_toggle_the_cursor_branch() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            apply_key_impl("ArrowDown"); // cursor → "0"
            apply_key_impl("Enter"); // toggle expand
            assert!(
                use_expanded().get().contains("0"),
                "Enter expands the branch"
            );
            drain_pump();
            apply_key_impl("Space"); // toggle collapse
            assert!(
                !use_expanded().get().contains("0"),
                "Space collapses the branch"
            );
        });
    }

    #[test]
    fn cursor_reanchors_when_its_row_is_hidden() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            apply_key_impl("ArrowDown"); // "0"
            apply_key_impl("ArrowRight"); // expand "0"
            drain_pump();
            apply_key_impl("ArrowRight"); // descend → "0/0"
            assert_eq!(cursor_now().as_deref(), Some("0/0"), "cursor on a child");

            // A click-path collapse of the ancestor hides the cursor's row.
            toggle_expanded(&use_expanded(), "0");
            assert!(
                !visible_ids().iter().any(|id| id == "0/0"),
                "child row hidden"
            );
            // The raw cursor is unchanged, but the gated cursor is None — so the
            // paint highlight + aria-activedescendant never dangle.
            assert_eq!(cursor_now().as_deref(), Some("0/0"), "raw cursor unchanged");
            assert_eq!(
                effective_cursor_now(),
                None,
                "gated cursor reanchors to none"
            );

            // The next vertical key restarts navigation at the first row (the
            // resolver treats an off-list cursor as no current row).
            apply_key_impl("ArrowDown");
            assert_eq!(
                cursor_now().as_deref(),
                Some("0"),
                "self-heals to first row"
            );
        });
    }

    #[test]
    fn apply_key_gates_on_root_focus() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            let mut scene = Scene::Container(ContainerNode::new(Vec::new()));
            // The trait gate: a key while a sibling (or nothing) holds focus
            // must not navigate (routing ⊥ focus); the focused branch is the
            // demo's end-to-end coverage of the same gate via `focus/set`.
            let handled = LazyTreeView::apply_key(
                &mut scene,
                None,
                "ArrowDown",
                pinion_core::Modifiers::default(),
            );
            assert!(!handled, "no focus → key not handled");
            assert_eq!(
                cursor_now(),
                None,
                "an unfocused key leaves the cursor untouched"
            );

            let handled = LazyTreeView::apply_key(
                &mut scene,
                Some(ROOT_TAG),
                "ArrowDown",
                pinion_core::Modifiers::default(),
            );
            assert!(handled, "focus on the tree root → the key navigates");
            assert_eq!(
                cursor_now().as_deref(),
                Some("0"),
                "focused key lands on the first row"
            );
        });
    }

    // ─── virtualization at scale (R946) ─────────────────────────────────────

    #[test]
    fn expanding_a_large_folder_floods_the_flatten() {
        // The scale precondition: a top-level section is a large asset folder,
        // so expanding it yields far more loaded rows than a window holds.
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            toggle_expanded(&use_expanded(), "0"); // expand a large section
            drain_pump();
            let loaded = node_rows(&rows_now()).len();
            assert!(
                loaded > 100,
                "an expanded section is a large folder ({loaded} rows) — the scale case",
            );
        });
    }

    #[test]
    fn a_tree_still_fetching_says_it_is_busy_and_stops_when_it_lands() {
        // ★★★ R1693.1 — the opening screen announced a `tree` holding no
        // `treeitem`, which the structural census reports as a collection a
        // reader is told about and cannot enter. It is not forgotten: it has
        // asked and is waiting, and `aria-busy` is the word for that moment.
        //
        // Both halves are asserted because only the pair is the claim. Dropping
        // the flag entirely passes the first; setting it unconditionally passes
        // the second; and the screen would be lying in one direction either way.
        let owner = Owner::new();
        owner.run(|| {
            boot();
            let fetching = LazyTreeView::access_node(&(), None);
            let root = &fetching[0];
            assert_eq!(root.tag, ROOT_TAG, "the container comes first");
            assert!(
                root.busy,
                "a tree with a fetch in flight declares aria-busy",
            );
            assert!(
                !fetching.iter().any(|n| n.role == AriaRole::TreeItem),
                "…which is the whole point: it holds nothing yet",
            );
            assert!(
                loading_under(&rows_now()).is_some(),
                "and the band it shares its predicate with agrees",
            );

            drain_pump();
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 10 * tree_pitch());
            let landed = LazyTreeView::access_node(&(), None);
            assert!(
                !landed[0].busy,
                "once the children land the tree stops saying wait",
            );
            assert!(
                landed.iter().any(|n| n.role == AriaRole::TreeItem),
                "and holds what it promised",
            );
        });
    }

    #[test]
    fn access_node_windows_the_treeitems_but_query_sees_all() {
        // The AT tree exposes only the rendered window's loaded rows (far fewer
        // than the loaded total), while the query-only introspection surface
        // keeps reporting the whole flattening — the virtualized §2 #7 split.
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            toggle_expanded(&use_expanded(), "0");
            drain_pump();
            let loaded = node_rows(&rows_now()).len();
            // A ~10-row window over the large folder.
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 10 * tree_pitch());
            let nodes = LazyTreeView::access_node(&(), None);
            let treeitems = nodes
                .iter()
                .filter(|n| n.role == AriaRole::TreeItem)
                .count();
            assert!(treeitems > 0, "some treeitems exposed");
            assert!(
                treeitems < 32,
                "AT windows {treeitems} treeitems of {loaded} loaded rows",
            );
            // Each windowed treeitem still carries its WAI-ARIA axes from the
            // flatten (posinset/setsize correct even when the window cuts a
            // sibling group).
            for item in nodes.iter().filter(|n| n.role == AriaRole::TreeItem) {
                assert!(
                    item.position_in_set.is_some(),
                    "treeitem carries aria-posinset"
                );
                assert!(item.size_of_set.is_some(), "treeitem carries aria-setsize");
            }
            // The introspection surface reads the FULL flattening (`node_rows`),
            // not the window — the AI-omniscient structure.
            assert!(
                loaded > treeitems,
                "query sees the whole tree, AT sees the window"
            );
        });
    }

    #[test]
    fn keyboard_navigation_reveals_off_window_rows() {
        // Keyboard ⊥ virtualization over a *lazy* tree: roving the cursor past
        // the rendered window scrolls it in, so the cursor row (and its
        // aria-activedescendant) stays realized.
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            toggle_expanded(&use_expanded(), "0");
            drain_pump();
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 10 * tree_pitch()); // 10-row window
            // The runtime layout pass sets the bound from the sizer height; set
            // it directly here so `scroll_to` is not clamped to 0.
            scroll.set_max(0, 1_000_000);

            apply_key_impl("ArrowDown"); // cursor onto the first row
            assert_eq!(scroll.offset_y(), 0, "the top row needs no scroll");
            for _ in 0..40 {
                apply_key_impl("ArrowDown"); // descend deep into the large folder
            }
            assert!(scroll.offset_y() > 0, "navigating down scrolled the window");

            // The cursor row sits inside the re-derived window over the LAZY
            // sequence (its painted index, skeletons included).
            let cursor = cursor_now().expect("cursor set");
            let all = rows_now();
            let idx = all
                .iter()
                .position(|r| matches!(r, LazyRow::Node(vr) if vr.id == cursor))
                .expect("cursor row in the flatten");
            let (_, mh) = scroll.measured_viewport();
            let window =
                compute_visible_range(scroll.offset_y(), mh, all.len(), tree_pitch(), OVERSCAN);
            assert!(
                idx >= window.first && idx < window.first + window.count,
                "cursor row {idx} stays within the revealed window {window:?}",
            );
        });
    }
}
