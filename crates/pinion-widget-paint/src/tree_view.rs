//! R671 §5.16 §5.38 §5.50 — backend-agnostic `TreeView` paint composition.
//!
//! Phase B widget catalog entry — `DevTools` / `Inspector` / file-tree /
//! property-grid prerequisite. The `tree_view` substrate produces a
//! flat `Scene` row sequence from a recursive [`TreeItem`] model so
//! the consuming binding wires hit-test + ARIA tree/treeitem
//! semantics with one composite-tag axis (`{tag}#{node_id}`). First
//! consumer (R671 atomic 4) is the `hello-multi-window` inspector
//! window, where the main window's paint scene is mirrored as a tree
//! of node-kind labels.
//!
//! ## Naming
//!
//! Mirrors [`crate::checkbox`] + [`crate::text_field`]: a
//! [`TreeViewStyle`] carrier struct with [`TreeViewStyle::m3_default`]
//! defaults and a [`view_tree`] fn that produces a [`Scene`] fragment.
//! Signature:
//!
//! ```rust,ignore
//! pub fn view_tree(
//!     tag: &'static str,
//!     items: &[TreeItem],
//!     theme: &Theme,
//!     style: &TreeViewStyle,
//! ) -> Scene;
//! ```
//!
//! ## Tree model
//!
//! [`TreeItem`] is recursive (`children: Vec<TreeItem>`) — the
//! `Vec<...>` carries indirection so the type is `Sized`. Each item
//! carries a stable `id` (string; used in the composite tag the input
//! router hit-tests against) + a `label` (the visible text + AT
//! enriched name) + an `expanded` flag (collapses descendants when
//! `false`, hides them entirely from the flat row sequence).
//!
//! ## Future axes (per [[abstraction-needs-second-consumer]])
//!
//! - **Keyboard navigation** (Arrow Up/Down/Left/Right + Home/End +
//!   roving tabindex active-descendant). R671's first consumer is
//!   read-only — the 2nd consumer (file-tree editor, property grid)
//!   surfaces the substrate-incompleteness signal that lifts kbd nav.
//! - **Virtualization** (`LazyVStack` pattern for N>1000 row trees).
//!   Phase D editor — when scene-graph trees grow large enough that
//!   the flat row walk becomes a paint-cycle bottleneck — surfaces
//!   this axis.
//! - **Multi-select / drag-drop / inline rename**. Not in R671 scope.

use pinion_core::scene::{ContainerNode, Rect, TextNode, TextRole};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::{Color, Scene};

/// R671 §5.16 §5.50 — Material 3 `TreeView` row dimensions. Mirrors
/// the [`crate::checkbox::CheckboxStyle`] pattern so the binding
/// catalog presents a uniform `Style` carrier surface.
///
/// Defaults track the M3 Lists spec: row height 48 px (touch-target
/// plus WCAG 2.5.5 target size); indent step 16 px (M3 `disclosure`
/// indent token); expand-glyph size 24 px; label font 16 px; row
/// padding 12 px horizontal (so the leftmost glyph / indent column
/// has breathing room from the tree's outer container fill).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeViewStyle {
    /// Row height in logical pixels. M3 Lists token = 48 px (touch
    /// target + WCAG 2.5.5).
    pub row_height: u32,
    /// Indent step per depth level, in logical pixels. M3 token = 16
    /// px (disclosure indent). Depth 0 = no leading padding; depth N
    /// = `indent_step * N` left-side padding.
    pub indent_step: u32,
    /// Expand-glyph font size in logical pixels. M3 token = 24 px so
    /// the `\u{25B6}` / `\u{25BC}` triangle reads cleanly across font
    /// fallback chains.
    pub glyph_size_px: u32,
    /// Label font size in logical pixels. M3 token ≈ `body-large`
    /// (16 px) — same as [`crate::checkbox::CheckboxStyle::font_size_px`]
    /// so a checkbox-in-tree composes visually.
    pub font_size_px: u32,
    /// Horizontal padding inside each row (leading + trailing), in
    /// logical pixels. The leading edge anchors the depth indent
    /// column; the trailing edge prevents labels from running into
    /// the container border.
    pub row_padding: u32,
    /// Gap between the expand glyph and the label in logical pixels.
    /// Matches [`crate::checkbox::CheckboxStyle::row_gap`] so rows
    /// with a checkbox prefix line up visually.
    pub glyph_label_gap: u32,
}

impl TreeViewStyle {
    /// R671 §5.50 — Material 3 `TreeView` defaults. The `DevTools` /
    /// `Inspector` first-consumer numbers — anchored on the M3 Lists
    /// spec + the [`crate::checkbox::CheckboxStyle`] family so trees
    /// containing checkboxes (todo-tree, file-tree with select-all)
    /// retain visual continuity.
    #[must_use]
    pub const fn m3_default() -> Self {
        Self {
            row_height: 48,
            indent_step: 16,
            glyph_size_px: 24,
            font_size_px: 16,
            row_padding: 12,
            glyph_label_gap: 10,
        }
    }
}

/// R673 §5.16 §5.50 — interactive tree-view paint state.
///
/// Holds the optional focused row id (highlighted via M3 state-layer
/// overlay on the row background) so interactive consumers
/// (file-tree, property-grid, `DevTools` outliner) can render a
/// keyboard-driven focus indicator without re-implementing the
/// state-layer math in every binding.
///
/// `None` reverts to the R671 read-only behaviour — every row paints
/// with a transparent background.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeViewFocus<'a> {
    /// Currently-focused row id (matches one [`TreeItem::id`] in the
    /// flat depth-first row sequence). When `None`, no row carries
    /// the focus highlight. When `Some`, the matching row's
    /// background fills with the M3 `Secondary Container` token at
    /// the canonical focus state-layer alpha.
    pub focused_id: Option<&'a str>,
}

impl Default for TreeViewStyle {
    fn default() -> Self {
        Self::m3_default()
    }
}

/// R671 §5.16 — recursive `TreeView` model.
///
/// Each node carries:
/// - `id` — stable string used in the composite tag the input router
///   hit-tests against (`{tree_tag}#{id}`). Bindings choose the
///   namespace (path-like `0/2/1`, opaque hash, etc.) — `tree_view`
///   treats it as opaque.
/// - `label` — visible text + AT enrich-from-scene accessible name.
/// - `expanded` — when `true` (and `children` is non-empty), descend
///   into children; when `false`, descendants are *hidden* from the
///   flat row sequence the paint walk produces (the expand glyph
///   flips `\u{25BC}`/`\u{25B6}`).
/// - `children` — recursive child nodes; `Vec` over `Box<TreeItem>`
///   keeps the recursive type `Sized` through the `Vec`'s
///   indirection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeItem {
    /// Stable id; appears as the suffix of the row's composite tag.
    pub id: String,
    /// Visible label text + AT enrich-from-scene name.
    pub label: String,
    /// Expand state; `true` shows descendants in the flat row walk.
    pub expanded: bool,
    /// Recursive child nodes; `Vec` indirection keeps `TreeItem`
    /// `Sized`.
    pub children: Vec<TreeItem>,
}

impl TreeItem {
    /// Construct a leaf `TreeItem` (no children, `expanded = false`).
    #[must_use]
    pub fn leaf<I: Into<String>, L: Into<String>>(id: I, label: L) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            expanded: false,
            children: Vec::new(),
        }
    }

    /// Construct a branch `TreeItem` with the given child list and
    /// the supplied `expanded` flag.
    #[must_use]
    pub fn branch<I: Into<String>, L: Into<String>>(
        id: I,
        label: L,
        expanded: bool,
        children: Vec<TreeItem>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            expanded,
            children,
        }
    }
}

/// R671 §5.50 — collapsed-state expand glyph (`U+25B6` BLACK
/// RIGHT-POINTING TRIANGLE). Named per [[non-ascii-literal-named-const-escape]].
const GLYPH_COLLAPSED: &str = "\u{25B6}";
/// R671 §5.50 — expanded-state expand glyph (`U+25BC` BLACK
/// DOWN-POINTING TRIANGLE).
const GLYPH_EXPANDED: &str = "\u{25BC}";
/// R671 §5.50 — leaf placeholder glyph (`U+00A0` NO-BREAK SPACE).
/// Same width-class as the triangles so leaf rows line up vertically
/// with branch rows. Renders invisible — leaves carry no disclosure
/// affordance per WAI-ARIA `treeitem` semantics.
const GLYPH_LEAF: &str = "\u{00A0}";

/// R671 §5.16 §5.50 — depth-first flat row paint of a `TreeView`
/// model.
///
/// # Arguments
///
/// - `tag` — root container tag; the input router hit-tests this as
///   the `Tree` scope. Per-row tags are the composite form
///   `{tag}#{node_id}` (the [[multi-external-substrate-extra-externals-pattern]]
///   shape — `composite_tag::parse_send_payload` 6th consumer at
///   R671 atomic 4 binding land).
/// - `items` — top-level `TreeItem` slice; each item plus its
///   recursive descendants (subject to `expanded`) becomes one row.
/// - `theme` — current [`Theme`] palette; drives label colour
///   ([`ColorRole::OnSurface`]) + container surface
///   ([`ColorRole::Surface`]).
/// - `style` — [`TreeViewStyle`] dimension carrier; pass
///   [`TreeViewStyle::m3_default`] for M3 defaults.
///
/// # Returns
///
/// A [`Scene::Container`] tagged `tag` holding one
/// [`Scene::Container`] per visible row. Each row container is
/// tagged `{tag}#{node_id}` + sized at the M3 row height. Rows
/// carry: leading depth-indent spacer (width = `depth *
/// style.indent_step`) + expand-glyph `TextNode` (presentational, AT
/// skips) + gap + label `TextNode` (the natural AT name). The outer
/// container lays children vertically (`flex Column`) so the row
/// sequence stacks top-to-bottom matching `Lists`-spec reading
/// order.
#[must_use]
pub fn view_tree(
    tag: &'static str,
    items: &[TreeItem],
    theme: &Theme,
    style: &TreeViewStyle,
) -> Scene {
    view_tree_focused(tag, items, theme, style, &TreeViewFocus::default())
}

/// R673 §5.16 §5.50 — interactive `view_tree` variant that paints
/// the focused row's background with the M3 `Secondary Container`
/// token (the canonical Material 3 keyboard-focus state-layer for
/// list rows). When `focus.focused_id == None` the output is
/// identical to [`view_tree`] (every row paints transparent), so
/// interactive bindings can adopt this entry without breaking the
/// R671 read-only consumer behaviour.
#[must_use]
pub fn view_tree_focused(
    tag: &'static str,
    items: &[TreeItem],
    theme: &Theme,
    style: &TreeViewStyle,
    focus: &TreeViewFocus<'_>,
) -> Scene {
    let mut rows: Vec<Scene> = Vec::new();
    for item in items {
        append_rows_focused(tag, item, 0, theme, style, focus, &mut rows);
    }
    Scene::Container(
        ContainerNode::new(rows)
            .with_tag(tag)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    // R673 §5.50 — stretch rows to fill the tree's
                    // cross-axis (= horizontal width). Pre-R673 the
                    // outer container's default align_items pushed
                    // rows to their content width, centering visually
                    // and hiding the depth-indent column on the left.
                    .with_align_items(AlignItems::Stretch),
            ),
    )
}

/// R673 §5.16 §5.50 — focus-aware variant of `append_rows`. Each row
/// receives the current focus state so the `build_row` helper can
/// decide whether to paint the background highlight.
fn append_rows_focused(
    tree_tag: &'static str,
    item: &TreeItem,
    depth: u32,
    theme: &Theme,
    style: &TreeViewStyle,
    focus: &TreeViewFocus<'_>,
    out: &mut Vec<Scene>,
) {
    let is_focused = focus.focused_id == Some(item.id.as_str());
    out.push(build_row(tree_tag, item, depth, theme, style, is_focused));
    if item.expanded {
        for child in &item.children {
            append_rows_focused(tree_tag, child, depth + 1, theme, style, focus, out);
        }
    }
}

/// Compose one row: depth indent + expand glyph + label.
fn build_row(
    tree_tag: &'static str,
    item: &TreeItem,
    depth: u32,
    theme: &Theme,
    style: &TreeViewStyle,
    is_focused: bool,
) -> Scene {
    let glyph = if item.children.is_empty() {
        GLYPH_LEAF
    } else if item.expanded {
        GLYPH_EXPANDED
    } else {
        GLYPH_COLLAPSED
    };
    let label_color = theme.resolve(ColorRole::OnSurface);
    let glyph_color = theme.resolve(ColorRole::OnSurfaceMuted);
    let indent_px = depth * style.indent_step;
    let mut row_children: Vec<Scene> = Vec::new();
    if indent_px > 0 {
        // Empty container as a depth indent spacer. Width = depth ×
        // indent_step; height = row_height. The container carries no
        // tag (presentational) so the AT layer doesn't expose it.
        row_children.push(Scene::Container(
            ContainerNode::new(Vec::new()).with_layout(
                LayoutStyle::new().with_size(Size::px(indent_px, style.row_height)),
            ),
        ));
    }
    // R673 §5.50 — wrap the expand glyph in a fixed-width container
    // so leaf rows (NO-BREAK SPACE placeholder, narrow) and branch
    // rows (BLACK TRIANGLE glyphs, wider) line up label columns
    // identically. The container's width = style.glyph_size_px;
    // height = row_height (the glyph is vertically centered by the
    // row's `AlignItems::Center`).
    let glyph_node = Scene::Text(
        TextNode::styled(
            glyph,
            Rect::default(),
            TextStyle::new()
                .with_size_px(style.glyph_size_px)
                .with_fg(glyph_color),
        )
        // R51.81 — presentational so enrich_names_from_scene skips
        // the glyph and lands on the label TextNode.
        .with_role(TextRole::Presentational),
    );
    row_children.push(Scene::Container(
        ContainerNode::new(vec![glyph_node]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_size(Size::px(style.glyph_size_px, style.row_height)),
        ),
    ));
    row_children.push(Scene::Text(TextNode::styled(
        item.label.as_str(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.font_size_px)
            .with_fg(label_color),
    )));
    let row_tag = composite_row_tag(tree_tag, &item.id);
    // R673 §5.50 — focused row fills with the M3
    // `SurfaceContainerHighest` tier (the canonical Material 3
    // list-row focus state-layer). Non-focused rows stay transparent
    // so the tree's outer Surface fill shows through.
    let row_bg = if is_focused {
        theme.resolve(ColorRole::SurfaceContainerHighest)
    } else {
        Color::TRANSPARENT
    };
    Scene::Container(
        ContainerNode::new(row_children)
            .with_tag(row_tag)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Start)
                    .with_gap(style.glyph_label_gap)
                    // R673 §5.50 — fixed row height matches the M3
                    // Lists row token; width = Auto so the parent
                    // container's `AlignItems::Stretch` extends the
                    // row to the cross-axis full width (the focus
                    // highlight + indent spacer rely on the row
                    // filling the available width). The `Size` type
                    // is `#[non_exhaustive]`, so build via default +
                    // overwrite height — width stays at
                    // `SizeValue::Auto`.
                    .with_size({
                        let mut s = Size::default();
                        s.height = SizeValue::Px(style.row_height);
                        s
                    })
                    .with_padding(Rect::new(
                        style.row_padding,
                        0,
                        style.row_padding,
                        0,
                    )),
            )
            .with_style(BoxStyle::filled(row_bg)),
    )
}

/// Compose the row's composite tag — the
/// [[multi-external-substrate-extra-externals-pattern]] form
/// `{tree_tag}#{id}`. Bindings parse this through
/// `pinion_core::composite_tag::parse_send_payload` (6th consumer at
/// R671 atomic 4) to address individual rows from RPC dispatch.
///
/// Public for tests + future bindings that need to look up a row by
/// id without descending the scene tree.
#[must_use]
pub fn composite_row_tag(tree_tag: &str, node_id: &str) -> String {
    format!("{tree_tag}#{node_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light_theme() -> Theme {
        Theme::light()
    }

    /// Walk `scene` collecting every tag on `Scene::Container` nodes.
    fn collect_tags(scene: &Scene) -> Vec<String> {
        let mut out = Vec::new();
        collect_tags_inner(scene, &mut out);
        out
    }

    fn collect_tags_inner(scene: &Scene, out: &mut Vec<String>) {
        if let Scene::Container(c) = scene {
            if let Some(tag) = &c.tag {
                out.push(tag.to_string());
            }
            for child in &c.children {
                collect_tags_inner(child, out);
            }
        }
    }

    /// Count the number of `Scene::Container` rows directly under the
    /// tree root (i.e., one per visible tree row).
    fn count_row_children(scene: &Scene) -> usize {
        match scene {
            Scene::Container(c) => c
                .children
                .iter()
                .filter(|s| matches!(s, Scene::Container(_)))
                .count(),
            _ => 0,
        }
    }

    /// R673 §5.50 — recursive walk for the row's presentational
    /// (glyph) `TextNode`. Pre-R673 the glyph was a direct child of
    /// the row Container; R673 wraps it in a fixed-width Container
    /// so glyph columns align across leaves + branches. Walk
    /// descends into nested containers to find the Presentational
    /// Text wherever it sits.
    fn find_glyph_in_row(row: &Scene) -> Option<String> {
        if let Scene::Text(t) = row {
            if matches!(t.role, Some(TextRole::Presentational)) {
                return Some(t.content.clone());
            }
        }
        if let Scene::Container(c) = row {
            for child in &c.children {
                if let Some(found) = find_glyph_in_row(child) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn find_label_in_row(row: &Scene) -> Option<String> {
        if let Scene::Text(t) = row {
            if !matches!(t.role, Some(TextRole::Presentational)) {
                return Some(t.content.clone());
            }
        }
        if let Scene::Container(c) = row {
            for child in &c.children {
                if let Some(found) = find_label_in_row(child) {
                    return Some(found);
                }
            }
        }
        None
    }

    #[test]
    fn r671_tree_view_style_m3_default_constants() {
        let s = TreeViewStyle::m3_default();
        assert_eq!(s.row_height, 48);
        assert_eq!(s.indent_step, 16);
        assert_eq!(s.glyph_size_px, 24);
        assert_eq!(s.font_size_px, 16);
        assert_eq!(s.row_padding, 12);
        assert_eq!(s.glyph_label_gap, 10);
    }

    #[test]
    fn r671_tree_view_empty_items_produces_root_only() {
        // Empty tree → root container with the tag but no row
        // children. The container surface still paints (consistent
        // hover/empty-state target) but no row tags exist for the
        // input router to hit-test.
        let scene = view_tree("tree", &[], &light_theme(), &TreeViewStyle::m3_default());
        assert_eq!(count_row_children(&scene), 0);
        let tags = collect_tags(&scene);
        assert_eq!(tags, vec!["tree".to_string()]);
    }

    #[test]
    fn r671_tree_view_single_leaf_produces_one_row() {
        let items = vec![TreeItem::leaf("root", "Hello")];
        let scene = view_tree(
            "tree",
            &items,
            &light_theme(),
            &TreeViewStyle::m3_default(),
        );
        assert_eq!(count_row_children(&scene), 1);
        let tags = collect_tags(&scene);
        // Root + one row. Indent spacer is absent at depth 0 (no
        // leading container child for indent = 0).
        assert_eq!(tags, vec!["tree".to_string(), "tree#root".to_string()]);
    }

    #[test]
    fn r671_tree_view_expanded_branch_walks_children() {
        // Branch with two children, expanded → 1 branch row + 2 leaf
        // rows = 3 rows total at the root level.
        let items = vec![TreeItem::branch(
            "root",
            "Root",
            true,
            vec![TreeItem::leaf("a", "Child A"), TreeItem::leaf("b", "Child B")],
        )];
        let scene = view_tree(
            "tree",
            &items,
            &light_theme(),
            &TreeViewStyle::m3_default(),
        );
        assert_eq!(count_row_children(&scene), 3);
    }

    #[test]
    fn r671_tree_view_collapsed_branch_hides_children() {
        // Same branch with expanded=false → only the branch row;
        // descendants are hidden from the flat row sequence.
        let items = vec![TreeItem::branch(
            "root",
            "Root",
            false,
            vec![TreeItem::leaf("a", "Child A"), TreeItem::leaf("b", "Child B")],
        )];
        let scene = view_tree(
            "tree",
            &items,
            &light_theme(),
            &TreeViewStyle::m3_default(),
        );
        assert_eq!(count_row_children(&scene), 1);
        let tags = collect_tags(&scene);
        assert_eq!(tags, vec!["tree".to_string(), "tree#root".to_string()]);
    }

    #[test]
    fn r671_tree_view_expanded_glyph_is_down_triangle() {
        let items = vec![TreeItem::branch(
            "root",
            "Root",
            true,
            vec![TreeItem::leaf("a", "A")],
        )];
        let scene = view_tree(
            "tree",
            &items,
            &light_theme(),
            &TreeViewStyle::m3_default(),
        );
        if let Scene::Container(c) = &scene {
            let row = &c.children[0];
            assert_eq!(find_glyph_in_row(row).as_deref(), Some(GLYPH_EXPANDED));
        } else {
            panic!("expected Container root");
        }
    }

    #[test]
    fn r671_tree_view_collapsed_glyph_is_right_triangle() {
        let items = vec![TreeItem::branch(
            "root",
            "Root",
            false,
            vec![TreeItem::leaf("a", "A")],
        )];
        let scene = view_tree(
            "tree",
            &items,
            &light_theme(),
            &TreeViewStyle::m3_default(),
        );
        if let Scene::Container(c) = &scene {
            let row = &c.children[0];
            assert_eq!(find_glyph_in_row(row).as_deref(), Some(GLYPH_COLLAPSED));
        } else {
            panic!("expected Container root");
        }
    }

    #[test]
    fn r671_tree_view_leaf_glyph_is_nbsp_placeholder() {
        // Leaves carry no disclosure affordance — render an invisible
        // NO-BREAK SPACE so the column width still lines up with
        // branch rows.
        let items = vec![TreeItem::leaf("leaf", "Leaf")];
        let scene = view_tree(
            "tree",
            &items,
            &light_theme(),
            &TreeViewStyle::m3_default(),
        );
        if let Scene::Container(c) = &scene {
            let row = &c.children[0];
            assert_eq!(find_glyph_in_row(row).as_deref(), Some(GLYPH_LEAF));
        } else {
            panic!("expected Container root");
        }
    }

    #[test]
    fn r671_tree_view_depth_indent_grows_with_nesting() {
        // Root expanded + child expanded with grandchild — the
        // grandchild row should carry a depth-2 indent (= 32 px with
        // M3 defaults), the child row a depth-1 indent (16 px), and
        // the root row no indent.
        let items = vec![TreeItem::branch(
            "root",
            "Root",
            true,
            vec![TreeItem::branch(
                "child",
                "Child",
                true,
                vec![TreeItem::leaf("grand", "Grand")],
            )],
        )];
        let scene = view_tree(
            "tree",
            &items,
            &light_theme(),
            &TreeViewStyle::m3_default(),
        );
        if let Scene::Container(c) = &scene {
            assert_eq!(c.children.len(), 3);
            // Indent column count per row:
            // row 0 (root, depth 0) — no indent spacer → first child
            // is the glyph TextNode → "first Container" count = 0.
            // row 1 (child, depth 1) — leading Container (the
            // indent spacer of width 16).
            // row 2 (grand, depth 2) — leading Container (width 32).
            assert_eq!(count_leading_indent_spacer(&c.children[0]), 0);
            assert_eq!(count_leading_indent_spacer(&c.children[1]), 16);
            assert_eq!(count_leading_indent_spacer(&c.children[2]), 32);
        } else {
            panic!("expected Container root");
        }
    }

    #[test]
    fn r671_tree_view_label_is_visible_text() {
        let items = vec![TreeItem::leaf("only", "Hello, world")];
        let scene = view_tree(
            "tree",
            &items,
            &light_theme(),
            &TreeViewStyle::m3_default(),
        );
        if let Scene::Container(c) = &scene {
            let row = &c.children[0];
            assert_eq!(
                find_label_in_row(row).as_deref(),
                Some("Hello, world")
            );
        } else {
            panic!("expected Container root");
        }
    }

    #[test]
    fn r671_tree_view_composite_tags_unique_per_row() {
        let items = vec![
            TreeItem::branch(
                "a",
                "A",
                true,
                vec![TreeItem::leaf("a1", "A1"), TreeItem::leaf("a2", "A2")],
            ),
            TreeItem::leaf("b", "B"),
        ];
        let scene = view_tree(
            "tree",
            &items,
            &light_theme(),
            &TreeViewStyle::m3_default(),
        );
        let tags = collect_tags(&scene);
        assert_eq!(
            tags,
            vec![
                "tree".to_string(),
                "tree#a".to_string(),
                "tree#a1".to_string(),
                "tree#a2".to_string(),
                "tree#b".to_string(),
            ]
        );
    }

    #[test]
    fn r671_tree_view_composite_row_tag_helper_matches_emit() {
        // The helper consumers (binding-side row lookup, RPC scope
        // resolution) call must produce the same string the view-fn
        // emits.
        assert_eq!(composite_row_tag("tree", "node-42"), "tree#node-42");
        assert_eq!(composite_row_tag("inspector", "0/1/2"), "inspector#0/1/2");
    }

    /// Helper: pull the layout width of the depth-indent spacer at
    /// the front of `row`. Depth 0 rows omit the spacer (glyph
    /// container is the first child); deeper rows carry a leading
    /// Container whose only purpose is to push the rest of the row
    /// right by `depth × indent_step` px. The spacer's width is
    /// always `SizeValue::Px(n)`; the glyph container's width is
    /// `SizeValue::Px(glyph_size_px)` (24 px at M3 defaults). The
    /// discriminator: spacer carries zero children, glyph container
    /// carries one Text child. Returns 0 when the row has no
    /// leading spacer (depth 0).
    fn count_leading_indent_spacer(row: &Scene) -> u32 {
        let Scene::Container(c) = row else { return 0 };
        let Some(Scene::Container(first)) = c.children.first() else {
            return 0;
        };
        // Glyph container holds the Text node; spacer is empty.
        if !first.children.is_empty() {
            return 0;
        }
        let SizeValue::Px(w) = first.layout.size.width else {
            return 0;
        };
        w
    }
}
