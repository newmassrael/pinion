//! `hello-tree-grid` — R860 §5.27 §5.50 **tree-grid (scene-outliner)**.
//!
//! A hierarchical outliner whose **frozen name column** (indent + expand
//! glyph + label) is pinned via the R859 frozen-split substrate while the
//! **metadata columns** (Type / Visible / Layer) scroll horizontally. Both
//! panes share the vertical body scroll through the R859 linked-scroll
//! [follower](pinion_core::scene::ScrollNode::as_follower), so the tree and
//! its metadata scroll in vertical lockstep. This is the self-hosted
//! editor's scene-outliner shape — the #1 Phase-B UI need.
//!
//! It is the first consumer of
//! [`pinion_widget_paint::tree_view::view_virtual_treegrid`], composing the
//! R819 tree virtualization ([`flat_visible`] windowing) with the R859
//! frozen-column data-grid.
//!
//! ## Interaction
//!
//! Click a folder row (the `{TREE_TAG}#{id}` composite-tag name cell, the
//! R674 [`TreeRowClickExternal`] path) to expand / collapse it; the visible
//! row count changes and the grid re-windows. Clicking also focuses the row
//! (highlighted across both panes). R864 — the outliner is keyboard-navigable
//! (single tab stop + `aria-activedescendant` roving cursor): Arrow Up/Down
//! move the cursor, Arrow Right expands / descends, Arrow Left collapses /
//! ascends, Enter/Space toggle, and type-ahead jumps by name — the lifted
//! `apply_tree_key` + `tree_typeahead_jump` substrate, with the cursor
//! scrolled into the body window (keyboard ⊥ virtualization). The metadata
//! columns are display cells the row cursor rides past, not separate tab
//! stops (an outliner navigates by row, not by cell).
//!
//! ## The witness (§2 #7 scene-as-data)
//!
//! `scene/snapshot` reports the windowed `{TREE_TAG}#{id}` name cells +
//! `{TREE_TAG}_drow{id}` metadata strips. `scene/scroll` on the horizontal
//! scroll (`tgrid_hscroll`) shifts the metadata columns left while the name
//! column stays put (the freeze); `scene/scroll` on the body (`tgrid_scroll`)
//! slides BOTH panes in lockstep. `scene/query` on the [`TREE_STATE_TAG`]
//! query-only introspection External reports the FULL visible-row count + per
//! row `id_at` / `level_at` / `expanded_at` (the virtualization sees only a
//! window, so the AI reads structure here, not from the painted nodes); R865 —
//! the [`CELLS_TAG`] peer reports `cell_at.<pos>.<col>` so the AI also reads the
//! off-window metadata (Type / Visible / Layer) the paint window cannot expose.
//! See `tools/demos/r860_tree_grid.py`.
//!
//! ## a11y (R863)
//!
//! The binding supplies a WAI-ARIA `treegrid` via the shared
//! [`treegrid_nodes`]: each `row` carries the tree disclosure axes
//! (`aria-level` / `aria-expanded` / `aria-posinset` / `aria-setsize`) **and**
//! holds a `rowheader` (the `{TREE_TAG}#{id}` name cell) + one `gridcell` per
//! metadata column, with the row's AT bounds spanning the frozen name pane +
//! the scrolling metadata pane (the R863 [`AccessNode::bounds_union_tags`]
//! substrate). This resolves the R860 carry — the metadata columns were
//! AT-invisible under the prior `tree` / `treeitem` topology (a `treeitem` has
//! no `gridcell` children in WAI-ARIA). R865 — the AT tree is **windowed** to
//! the rendered slice (the same `compute_visible_range` the paint uses), so it
//! exposes exactly the realized rows, not bounds-less off-window ghosts.

use pinion_a11y::{treegrid_nodes, AccessFocus, AccessNode, WidgetA11y};
use pinion_core::external::{
    External, IntrospectSchema, IntrospectValue, QueryOnlyIntrospect, QuerySource, StubExternal,
};
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::ContainerNode;
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::composite_tag::GridTag;
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::tree_nav::{
    flat_visible, toggle_expanded, tree_view_introspection_extra, TreeNode, VisibleRow,
};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Frame, Modifiers, Owner, Scene, Signal, WidgetCore};
use pinion_shell::typeahead::apply_windowed_tree_key;
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::table::GridScroll;
use pinion_widget_paint::tree_view::{
    view_virtual_treegrid, TreeGridData, TreeRowClickExternal, TreeViewStyle,
};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTreeGridRenderer, HelloTreeGridRendererError);

/// Initial window size. Narrower than the tree column + all metadata
/// columns so the metadata overflows and scrolls while the name column
/// stays pinned.
const WIN_W: u32 = 480;
const WIN_H: u32 = 460;
/// Shared [`ThemeProvider`] cache key.
const THEME_TAG: &str = "app";
/// Composite-tag prefix the name cells carry (`{TREE_TAG}#{id}`) and the
/// [`TreeRowClickExternal`] anchor clicks route to.
const TREE_TAG: &str = "tgrid";
/// Invisible focusable tree-root anchor (the WAI-ARIA `tree` node lands
/// here); kept distinct from [`TREE_TAG`] like `hello-virtual-tree`.
const ROOT_TAG: &str = "tgrid_root";
/// Query-only tree-state introspection External: for a *virtualized* tree
/// this is the only way the AI reads the full structure (only a window
/// paints).
const TREE_STATE_TAG: &str = "tgrid_state";
/// R865 §5.12 §5.50 — query-only introspection of the metadata CELL values.
/// The off-window read an AI needs: the virtualization paints only a window
/// and [`TREE_STATE_TAG`] reports only row STRUCTURE (id / level / expanded),
/// not the per-column metadata, so an AI inspecting an off-window object could
/// not read its Type / Visible / Layer. Binding-local because the `cell_data`
/// derivation is binding-specific (the shared `pinion-core` tree introspection
/// stays node-type agnostic until a 2nd tree-grid consumer surfaces it —
/// `[[abstraction-needs-second-consumer]]`).
const CELLS_TAG: &str = "tgrid_cells";
/// Input-router tag for the vertical body `ScrollState` (shared by both
/// panes).
const SCROLL_KEY: &str = "tgrid_scroll";
/// Input-router tag for the horizontal `ScrollState` (the metadata columns).
const H_SCROLL_KEY: &str = "tgrid_hscroll";
/// The dotted wire form of the [`TreeRowClickExternal`] row-click intent.
const CLICK_INTENT_TAG: &str = intent_tag!("tgrid", "click");

/// Metadata column headers (the scrolling pane).
const DATA_HEADERS: [&str; 3] = ["Type", "Visible", "Layer"];
/// The frozen name-column header label (shared by the paint + the a11y
/// `treegrid` so the columnheader name matches the painted header).
const TREE_HEADER: &str = "Name";
/// Frozen tree (name) column width.
const TREE_COL_W: u32 = 200;
/// Each scrolling metadata column width. `3 × 160 = 480` metadata px against
/// a `480 − 200 = 280`px scrolled viewport gives a `200`px horizontal scroll
/// range — wide enough that the freeze (and a scroll-to-max revealing the
/// rightmost column) is a meaningful witness, not a token overflow.
const DATA_COL_W: u32 = 160;
/// Rows built beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Top-level scene folders.
const FOLDERS: usize = 24;
/// Object leaves per folder.
const OBJECTS_PER: usize = 12;
/// Folders expanded at boot (so the visible row count starts well above the
/// window — the virtualization is obvious from frame one).
const EXPANDED_AT_BOOT: usize = 3;
/// Uniform per-row vertical slot pitch. Must equal
/// [`TreeViewStyle::row_height`] (`view_virtual_treegrid` derives its slot
/// pitch from the style); asserted in [`tests`] and used by the keyboard
/// scroll-into-view ([`reveal_cursor`]).
const ROW_PITCH: u32 = 48;
/// Page Up / Down jump in rows (a viewport-ful, clamped via `clamp_nav`).
const NAV_PAGE: usize = 10;
/// Owner-cache key for the type-ahead cursor (buffer + last-typed instant),
/// caller-side per the `tree_nav` purity boundary (R811).
const TYPEAHEAD_KEY: &str = "hello_tree_grid::typeahead";

/// One outliner node — a scene folder or object. Carries its own `expanded`
/// flag (the retained flag-on-node storage). The `serde` + `PartialEq`
/// derives satisfy the §5.22 introspect bound `Signal<T>` carries.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct OutlinerNode {
    id: String,
    label: String,
    expanded: bool,
    children: Vec<OutlinerNode>,
}

impl OutlinerNode {
    fn leaf(id: String, label: String) -> Self {
        Self { id, label, expanded: false, children: Vec::new() }
    }
}

impl TreeNode for OutlinerNode {
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

/// Build the synthetic outliner. Deterministic ids (`f{folder}` /
/// `f{folder}-o{object}`) so the RPC demo can address rows by a stable
/// composite tag, and so [`cell_data`] derives stable metadata from the id.
fn initial_nodes() -> Vec<OutlinerNode> {
    (0..FOLDERS)
        .map(|f| {
            let children = (0..OBJECTS_PER)
                .map(|o| OutlinerNode::leaf(format!("f{f}-o{o}"), format!("Object {f:02}-{o:02}")))
                .collect();
            OutlinerNode {
                id: format!("f{f}"),
                label: format!("Folder {f:02}"),
                expanded: f < EXPANDED_AT_BOOT,
                children,
            }
        })
        .collect()
}

/// Metadata for a row, derived deterministically from its id (a folder id
/// has no `-`; an object id is `f{f}-o{o}`). A real editor reads these off
/// the scene object; the synthetic derivation keeps the demo dependency-free.
fn cell_data(id: &str, col: usize) -> String {
    let hash: u32 = id.bytes().map(u32::from).sum();
    let is_folder = !id.contains('-');
    match col {
        0 => {
            if is_folder {
                "Folder".to_string()
            } else {
                ["Mesh", "Light", "Camera"][usize::try_from(hash % 3).unwrap_or(0)].to_string()
            }
        }
        1 => if hash % 2 == 0 { "Yes" } else { "No" }.to_string(),
        _ => format!("L{}", hash % 4),
    }
}

/// R865 §5.12 §5.50 — the [`CELLS_TAG`] query-only introspection source: the
/// metadata cell values keyed by `(visible position, column)`, read fresh on
/// every query from the same retained tree the view windows over. It is the
/// metadata peer of the shared structural [`tree_view_introspection_extra`]:
/// where that reports `id_at` / `level_at` / `expanded_at`, this reports
/// `cell_at` so an AI reads the off-window Type / Visible / Layer the paint
/// window cannot expose ([[ai-first-rpc-introspection-obligation]]).
struct CellsIntrospect {
    rows: Box<dyn Fn() -> Vec<VisibleRow>>,
}

impl core::fmt::Debug for CellsIntrospect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CellsIntrospect")
            .field("row_count", &(self.rows)().len())
            .field("col_count", &DATA_HEADERS.len())
            .finish_non_exhaustive()
    }
}

impl QuerySource for CellsIntrospect {
    fn introspect_schema(&self) -> IntrospectSchema {
        // `col_count` — the metadata column count (int).
        // `cell_at`   — `cell_at.<pos>.<col>` visible position + metadata column
        //               index -> the cell's display value (string), Null when
        //               the position or column is out of range.
        IntrospectSchema::new(&[("col_count", "int"), ("cell_at", "string")])
    }

    fn introspect_query(&self, path: &str) -> Option<IntrospectValue> {
        if path == "col_count" {
            return Some(IntrospectValue::Int(i64::try_from(DATA_HEADERS.len()).unwrap_or(i64::MAX)));
        }
        let rest = path.strip_prefix("cell_at.")?;
        // `cell_at.<pos>.<col>` — two indices. A malformed or out-of-range
        // address reports Null (present-but-empty), the convention the tree
        // structural introspection uses for an off-the-end position.
        let Some((pos_s, col_s)) = rest.split_once('.') else {
            return Some(IntrospectValue::Null);
        };
        let (Ok(pos), Ok(col)) = (pos_s.parse::<usize>(), col_s.parse::<usize>()) else {
            return Some(IntrospectValue::Null);
        };
        if col >= DATA_HEADERS.len() {
            return Some(IntrospectValue::Null);
        }
        let rows = (self.rows)();
        Some(match rows.get(pos) {
            Some(row) => IntrospectValue::Text(cell_data(&row.id, col)),
            None => IntrospectValue::Null,
        })
    }
}

/// Reactive holder for the retained tree + the focused/selected row, lifted
/// into [`Owner::cache`] so the view-fn, the a11y pass, and the click
/// reducer read the same `Signal`s.
struct TreeState {
    nodes: Signal<Vec<OutlinerNode>>,
    focused_id: Signal<Option<String>>,
}

fn use_tree_state() -> Rc<TreeState> {
    let owner = Owner::current().expect("use_tree_state must run inside a CoreShell view / reducer wrap");
    owner.cache("hello_tree_grid::state", || TreeState {
        nodes: Signal::new(initial_nodes()),
        // R864 — boot the keyboard cursor on the first row (the WAI-ARIA tree
        // convention `hello-virtual-tree` also uses): the outliner is a single
        // tab stop, so a defined `aria-activedescendant` exists from frame one
        // and the focus highlight paints across both panes immediately.
        focused_id: Signal::new(Some(String::from("f0"))),
    })
}

/// R864 §5.27 §5.50 — apply one key to the windowed tree-grid: the lifted
/// [`apply_windowed_tree_key`] (R866) resolve → type-ahead → scroll-cursor-into-
/// window pipeline, shared verbatim with `hello-virtual-tree`. The columned grid
/// navigates exactly like the plain tree — the metadata columns are display
/// cells the cursor rides past, not separate tab stops — so it threads only its
/// own per-binding state (`use_tree_state`) + scroll + cache slot into the
/// substrate; no per-binding navigation glue.
fn apply_key_impl(key: &str) -> bool {
    let state = use_tree_state();
    apply_windowed_tree_key(
        &state.nodes,
        &state.focused_id,
        &use_scroll_state(SCROLL_KEY),
        TYPEAHEAD_KEY,
        NAV_PAGE,
        ROW_PITCH,
        key,
    )
}

/// view-fn (§6.3): pure sync mapping. The dataset is virtual —
/// `view_virtual_treegrid` builds cells only for the indices in the current
/// scroll window, re-derived from `flat_visible(&nodes)`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    // `view_virtual_treegrid` derives its slot pitch from `style.row_height`;
    // the keyboard scroll-into-view (`reveal_cursor`) uses ROW_PITCH, so keep
    // them in lockstep.
    debug_assert_eq!(TreeViewStyle::m3_default().row_height, ROW_PITCH);

    let theme = use_theme(THEME_TAG).theme_animated();
    let tree_state = use_tree_state();
    let nodes = tree_state.nodes.get();
    let rows = flat_visible(&nodes);
    let focused = tree_state.focused_id.get();
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);

    let grid = view_virtual_treegrid(
        TREE_TAG,
        GridScroll { body: &scroll, horizontal: &h_scroll },
        &TreeGridData {
            rows: &rows,
            tree_header: TREE_HEADER,
            data_headers: &DATA_HEADERS,
            tree_col_width: TREE_COL_W,
            data_col_width: DATA_COL_W,
            overscan: OVERSCAN,
        },
        focused.as_deref(),
        &theme,
        &TreeViewStyle::m3_default(),
        cell_data,
    );

    // R819 — invisible 0x0 root anchor keeps the WAI-ARIA `tree` node + the
    // focus surface alive (mirrors `hello-virtual-tree`); no visual paints.
    let invisible_root = Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(ROOT_TAG)
            .with_layout(LayoutStyle::new().with_size(Size::px(0, 0))),
    );

    Scene::Container(
        ContainerNode::new(vec![grid, invisible_root])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start),
            ),
    )
}

struct TreeGridView;

impl WidgetCore for TreeGridView {
    type State = ();
    type Event = ();

    fn tag() -> &'static str {
        ROOT_TAG
    }

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    /// The `TreeRowClickExternal` sibling (composite `{TREE_TAG}#{id}` clicks
    /// → the §5.20 intent channel → [`TreeGridView::update`]) + the query-only
    /// tree-state introspection sibling (reads the same `Owner::cache`d
    /// `TreeState` the view windows over, so `scene/query` reports the full
    /// flattening regardless of which window paints).
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let tree_state = use_tree_state();
        let struct_nodes = tree_state.nodes.clone();
        let cells_nodes = tree_state.nodes.clone();
        let focused = tree_state.focused_id.clone();
        vec![
            ExtraExternal::new(TREE_TAG, Box::new(TreeRowClickExternal::new())),
            tree_view_introspection_extra(
                TREE_STATE_TAG,
                move || flat_visible(&struct_nodes.get()),
                move || focused.get(),
            ),
            // R865 — the metadata-cell peer (off-window value read).
            ExtraExternal::new(
                CELLS_TAG,
                Box::new(QueryOnlyIntrospect::new(Rc::new(CellsIntrospect {
                    rows: Box::new(move || flat_visible(&cells_nodes.get())),
                }))),
            ),
        ]
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// Click a row → focus it + toggle its `expanded` flag (a no-op on a
    /// leaf, which `flat_visible` ignores). Side-effect-only reducer
    /// ([[scxml-as-model-update-transient]]): the `Signal::set`s are the
    /// mutation, so the command list is empty.
    fn update(_state: (), intent: &Intent) -> Vec<pinion_core::command::Command> {
        if intent.tag_str() == CLICK_INTENT_TAG
            && let IntrospectValue::Text(id) = &intent.payload
        {
            let tree_state = use_tree_state();
            tree_state.focused_id.set(Some(id.clone()));
            toggle_expanded(&tree_state.nodes, id);
        }
        Vec::new()
    }

    /// R864 — the outliner is a single keyboard tab stop (the WAI-ARIA tree
    /// convention: one tab stop + `aria-activedescendant` roving). The
    /// focusable element is the [`ROOT_TAG`] treegrid root; arrow keys then
    /// move the cursor via [`apply_key`].
    fn focusable_tags() -> Vec<&'static str> {
        vec![ROOT_TAG]
    }

    /// R864 §5.27 §5.50 — WAI-ARIA APG tree keyboard over the windowed rows:
    /// the lifted [`apply_tree_key`] resolve → flag-store bridge + caller-side
    /// type-ahead, then [`reveal_cursor`] scrolls the new cursor into the body
    /// window (keyboard ⊥ virtualization). Single tab stop: keys apply only
    /// while the treegrid root [`ROOT_TAG`] is focused — an ungated outliner
    /// would steal keys from sibling panels in the eventual self-hosted editor
    /// ([[routing-and-focus-are-separate-axes]]); the RPC demo issues
    /// `focus/set` first like every other gated example.
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, _modifiers: Modifiers) -> bool {
        let _ = scene;
        if focused != Some(ROOT_TAG) {
            return false;
        }
        apply_key_impl(key)
    }

    fn title() -> &'static str {
        "pinion hello-tree-grid (R860 §5.27 scene-outliner tree-grid)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display + click-to-expand (no widget state)".to_string()
    }
}

impl WidgetA11y for TreeGridView {
    /// R863 — WAI-ARIA `treegrid` over the windowed rows (the shared
    /// [`treegrid_nodes`]): each `row` carries the tree disclosure axes and a
    /// `rowheader` (the `{TREE_TAG}#{id}` name cell) + one `gridcell` per
    /// metadata column, with the row's bounds spanning the frozen name pane +
    /// the scrolling metadata pane. Resolves the R860 carry (metadata columns
    /// were AT-invisible under the prior `tree`/`treeitem` topology).
    ///
    /// R865 — windows the AT tree to the rendered slice (the same
    /// `compute_visible_range` the paint uses), mirroring `hello-virtual-tree`:
    /// the AT tree exposes exactly the rows the paint realizes, so off-window
    /// rows are not announced as bounds-less ghost nodes. The keyboard cursor
    /// always sits inside this window (R864 `reveal_cursor` scrolls it in), so
    /// the `aria-selected` cursor row is always present.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let tree_state = use_tree_state();
        let rows = flat_visible(&tree_state.nodes.get());
        let focused = tree_state.focused_id.get();
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window =
            compute_visible_range(scroll.offset_y(), measured_h, rows.len(), ROW_PITCH, OVERSCAN);
        let slice: &[VisibleRow] = &rows[window.first..window.first + window.count];
        treegrid_nodes(
            ROOT_TAG,
            TREE_TAG,
            Some("Scene outliner"),
            TREE_HEADER,
            &DATA_HEADERS,
            slice,
            focused.as_deref(),
        )
    }

    /// R866 §5.40 — composite focus model (WAI-ARIA roving). When the outliner
    /// root owns focus, the keyboard cursor row is the `aria-activedescendant`:
    /// `AccessKit::TreeUpdate::focus` lands on the [`ROOT_TAG`] treegrid while
    /// the active descendant names the cursor ROW node (`{TREE_TAG}_drow{id}`),
    /// so the AT announces "you are on row X" rather than only "row X selected".
    /// Mirrors the listbox / radiogroup composite-focus pattern; the cursor row
    /// carries the matching `focused` flag from [`treegrid_nodes`].
    fn access_focus_target(_state: &(), focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(ROOT_TAG)
            && let Some(cursor) = use_tree_state().focused_id.get()
        {
            return Some(AccessFocus::composite(ROOT_TAG, GridTag::metadata_row(TREE_TAG, &cursor)));
        }
        focused.map(AccessFocus::atomic)
    }
}

impl WidgetView for TreeGridView {
    type Renderer = HelloTreeGridRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<TreeGridView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_overflows_the_window() {
        // The premise: tree column + all metadata columns exceed the window,
        // so the metadata pane genuinely scrolls horizontally past the freeze.
        // Sum the real per-column widths (a runtime fold, not a const).
        let total: u32 = TREE_COL_W + DATA_HEADERS.iter().map(|_| DATA_COL_W).sum::<u32>();
        assert!(total > WIN_W, "tree + metadata ({total}) must exceed WIN_W ({WIN_W})");
        // The frozen tree column leaves room for visible metadata beside it.
        let metadata_visible = WIN_W.saturating_sub(TREE_COL_W);
        assert!(metadata_visible > 0, "frozen column leaves room for metadata");
    }

    #[test]
    fn cell_data_distinguishes_folders_from_objects() {
        assert_eq!(cell_data("f3", 0), "Folder", "a folder id (no '-') is a Folder");
        assert_ne!(cell_data("f3-o1", 0), "Folder", "an object id is not a Folder");
        // Deterministic: same id → same metadata.
        assert_eq!(cell_data("f3-o1", 2), cell_data("f3-o1", 2));
    }

    #[test]
    fn initial_tree_boots_some_folders_expanded() {
        let nodes = initial_nodes();
        assert_eq!(nodes.len(), FOLDERS);
        let rows = flat_visible(&nodes);
        // FOLDERS folder rows + EXPANDED_AT_BOOT × OBJECTS_PER object rows.
        let expected = FOLDERS + EXPANDED_AT_BOOT * OBJECTS_PER;
        assert_eq!(rows.len(), expected, "boot visible rows = folders + expanded children");
    }

    // ── R864 keyboard roving ─────────────────────────────────────────

    #[test]
    fn slot_pitch_matches_style_row_height() {
        // `view_virtual_treegrid` derives its pitch from the style; the
        // keyboard scroll-into-view const must track it.
        assert_eq!(TreeViewStyle::m3_default().row_height, ROW_PITCH);
    }

    #[test]
    fn boot_cursor_sits_on_the_first_row() {
        Owner::new().run(|| {
            // WAI-ARIA tree convention: a defined aria-activedescendant from
            // frame one, so the focus highlight + AT cursor exist at boot.
            assert_eq!(use_tree_state().focused_id.get().as_deref(), Some("f0"));
        });
    }

    #[test]
    fn arrow_keys_move_the_row_cursor() {
        Owner::new().run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 10 * ROW_PITCH);
            let state = use_tree_state();
            // f0 boots expanded → ArrowDown descends to its first child.
            assert_eq!(state.focused_id.get().as_deref(), Some("f0"));
            assert!(apply_key_impl("ArrowDown"), "ArrowDown handled");
            assert_eq!(state.focused_id.get().as_deref(), Some("f0-o0"), "Down -> first child");
            assert!(apply_key_impl("ArrowUp"), "ArrowUp handled");
            assert_eq!(state.focused_id.get().as_deref(), Some("f0"), "Up -> back to parent");
            // A non-navigation, non-typeahead key is unhandled (falls through).
            assert!(!apply_key_impl("F5"), "F5 is not a tree key");
        });
    }

    #[test]
    fn arrow_right_left_expand_collapse_a_folder() {
        Owner::new().run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 10 * ROW_PITCH);
            let state = use_tree_state();
            // Focus a collapsed folder (f10), then ArrowRight expands it.
            state.focused_id.set(Some(String::from("f10")));
            let before = flat_visible(&state.nodes.get()).len();
            assert!(apply_key_impl("ArrowRight"), "ArrowRight handled");
            let after = flat_visible(&state.nodes.get()).len();
            assert_eq!(after, before + OBJECTS_PER, "ArrowRight expanded f10");
            // ArrowLeft on the expanded branch collapses it again.
            assert!(apply_key_impl("ArrowLeft"), "ArrowLeft handled");
            assert_eq!(flat_visible(&state.nodes.get()).len(), before, "ArrowLeft collapsed f10");
        });
    }

    #[test]
    fn keyboard_reveals_an_off_window_cursor() {
        Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 10 * ROW_PITCH); // 10-row window
            scroll.set_max(0, 1_000_000); // unclamp so scroll_to is honoured
            // Drive the cursor well past the bottom of the window.
            for _ in 0..40 {
                apply_key_impl("ArrowDown");
            }
            assert!(scroll.offset_y() > 0, "navigating down scrolled the body window");
            // The cursor row is inside the re-derived window (scroll-into-view).
            let state = use_tree_state();
            let cursor = state.focused_id.get().expect("cursor set");
            let rows = flat_visible(&state.nodes.get());
            let idx = rows.iter().position(|r| r.id == cursor).expect("cursor row visible");
            let (_, mh) = scroll.measured_viewport();
            let window = compute_visible_range(scroll.offset_y(), mh, rows.len(), ROW_PITCH, OVERSCAN);
            assert!(
                idx >= window.first && idx < window.first + window.count,
                "cursor row {idx} stays within the revealed window {window:?}",
            );
        });
    }

    #[test]
    fn apply_key_gates_on_root_focus() {
        // Single tab stop: keys apply ONLY when the treegrid root is focused —
        // an ungated outliner would steal keys from sibling panels.
        Owner::new().run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 10 * ROW_PITCH);
            let mut scene = Scene::Container(pinion_core::scene::ContainerNode::new(Vec::new()));
            assert!(
                !TreeGridView::apply_key(&mut scene, None, "ArrowDown", Modifiers::default()),
                "no focus -> key dropped",
            );
            assert!(
                !TreeGridView::apply_key(&mut scene, Some("other"), "ArrowDown", Modifiers::default()),
                "focus elsewhere -> key dropped",
            );
            assert!(
                TreeGridView::apply_key(&mut scene, Some(ROOT_TAG), "ArrowDown", Modifiers::default()),
                "focus on the treegrid root -> key applies",
            );
        });
    }

    // ── R865 a11y windowing + cell_at introspection ──────────────────

    #[test]
    fn access_node_windows_the_treegrid_to_the_rendered_slice() {
        // The AT tree exposes only the rendered window of rows, not the full
        // boot flattening (the bounds-less off-window ghost rows R863 emitted).
        let owner = Owner::new();
        let nodes = owner.run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 8 * ROW_PITCH);
            TreeGridView::access_node(&(), None)
        });
        assert_eq!(nodes[0].role, pinion_a11y::AriaRole::TreeGrid, "container is a treegrid");
        let rows = nodes.iter().filter(|n| n.role == pinion_a11y::AriaRole::Row).count();
        let visible = flat_visible(&initial_nodes()).len();
        assert!(visible > 30, "boot has many visible rows ({visible})");
        // header row + windowed data rows, far fewer than the full flattening.
        assert!(rows < visible, "AT rows {rows} window the {visible} visible rows");
        // The boot cursor (f0) is in the top window, so its row is present.
        assert!(nodes.iter().any(|n| n.tag == "tgrid_drowf0"), "cursor row f0 windowed in");
    }

    #[test]
    fn cell_at_reads_metadata_by_position_and_column() {
        // The off-window read: `cell_at.<pos>.<col>` resolves the visible row
        // at <pos> and reports its <col> metadata exactly as `cell_data` does.
        let src = CellsIntrospect { rows: Box::new(|| flat_visible(&initial_nodes())) };
        let rows = flat_visible(&initial_nodes());
        // col_count mirrors the metadata header count.
        assert_eq!(
            src.introspect_query("col_count"),
            Some(IntrospectValue::Int(i64::try_from(DATA_HEADERS.len()).unwrap())),
        );
        // An in-range cell reports the same string the painted cell shows.
        let pos = 5.min(rows.len() - 1);
        let expected = cell_data(&rows[pos].id, 1);
        assert_eq!(
            src.introspect_query(&format!("cell_at.{pos}.1")),
            Some(IntrospectValue::Text(expected)),
        );
        // Out-of-range column / position / malformed address -> Null.
        assert_eq!(src.introspect_query("cell_at.0.99"), Some(IntrospectValue::Null), "col OOR");
        assert_eq!(src.introspect_query("cell_at.99999.0"), Some(IntrospectValue::Null), "pos OOR");
        assert_eq!(src.introspect_query("cell_at.0"), Some(IntrospectValue::Null), "no column");
        assert_eq!(src.introspect_query("bogus"), None, "unknown path");
    }

    // ── R866 aria-activedescendant (roving composite focus) ──────────

    #[test]
    fn focused_outliner_returns_composite_activedescendant() {
        // When the treegrid owns focus, the keyboard cursor row is conveyed via
        // aria-activedescendant (focus on the container, active-descendant = the
        // cursor ROW node), not aria-selected alone.
        Owner::new().run(|| {
            use_tree_state().focused_id.set(Some(String::from("f0-o3")));
            let target = TreeGridView::access_focus_target(&(), Some(ROOT_TAG))
                .expect("focused outliner -> composite focus");
            assert_eq!(target.focus_tag, ROOT_TAG, "focus lands on the treegrid root");
            assert_eq!(
                target.active_descendant.as_deref(),
                Some("tgrid_drowf0-o3"),
                "aria-activedescendant = the cursor ROW node",
            );
        });
    }

    #[test]
    fn unfocused_outliner_returns_atomic_for_a_sibling() {
        // Focus elsewhere -> atomic on the sibling, no active descendant (the
        // outliner does not claim the cursor while a sibling panel is focused).
        Owner::new().run(|| {
            let target = TreeGridView::access_focus_target(&(), Some("other_widget"))
                .expect("sibling-focused -> atomic");
            assert_eq!(target.focus_tag, "other_widget");
            assert!(target.active_descendant.is_none());
        });
    }
}
