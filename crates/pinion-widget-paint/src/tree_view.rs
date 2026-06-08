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
//!   Landed at R819 as [`view_virtual_tree`] (first consumer
//!   `hello-virtual-tree`): windows the [`flat_visible`] sequence through
//!   the [`crate::virtual_list`] geometry so only the rows in the scroll
//!   window become scene nodes — the Phase D editor's scene-graph / asset
//!   trees grow past the point where a full flat row walk is a paint-cycle
//!   bottleneck. Keyboard roving + scroll-into-view over the window is a
//!   later additive axis (the R730 / `hello-virtual-select` windowed-list
//!   keyboard-defer precedent).
//! - **Multi-select / drag-drop / inline rename**. Not in R671 scope.

use pinion_core::composite_tag::parse_send_payload;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::input::PointerWireEvent;
use pinion_core::intent::Intent;
use pinion_core::scene::{ContainerNode, Rect, TextNode, TextRole};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::tree_nav::{flat_visible, VisibleRow};
use pinion_core::widgets::virtual_list::{compute_visible_range, content_height};
use pinion_core::{Color, Scene};
use std::rc::Rc;

use crate::virtual_list::{assemble_windowed_flex, uniform_slots};

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

/// R811 §5.50 §5.27 — `TreeItem` is the shipped [`TreeNode`] for the
/// keyboard-navigation substrate: both inspector/tree consumers paint
/// through `TreeItem`, so flattening the painted tree
/// ([`flat_visible`](pinion_core::widgets::tree_nav::flat_visible)) and
/// resolving keys against it
/// ([`resolve_tree_key`](pinion_core::widgets::tree_nav::resolve_tree_key))
/// navigate exactly the row sequence
/// [`view_tree_focused`] renders.
impl pinion_core::widgets::tree_nav::TreeNode for TreeItem {
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
    // R811.1 §5.50 — paint the one [`flat_visible`] flattening the
    // keyboard/nav and AT sides also read (the substrate lifted at R811
    // now walks `TreeItem` directly), so the "which rows are visible, at
    // what depth" logic lives in exactly one place. Before R811.1 this
    // was a second hand-written `expanded`-gated preorder walk
    // (`append_rows_focused`) that mirrored `walk_visible` by hand — the
    // R809.1 "same visible sequence walked twice" smell.
    let rows: Vec<Scene> = flat_visible(items)
        .iter()
        .map(|row| {
            let is_focused = focus.focused_id == Some(row.id.as_str());
            build_row(tag, row, theme, style, is_focused)
        })
        .collect();
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

/// R819 §5.16 §5.27 §5.50 — **virtualized** `TreeView` paint: render only
/// the rows inside the current scroll window, not the whole visible tree.
/// The tree-virtualization axis the module doc named for the Phase D
/// editor (scene-graph / asset trees that grow past the
/// point where a full flat row walk is a paint-cycle bottleneck).
///
/// This is a pure composition of the two substrates that already exist:
/// the windowing geometry from [`crate::virtual_list`]
/// ([`compute_visible_range`] + [`content_height`] + the shared
/// [`uniform_slots`] / [`assemble_windowed_flex`] assembly) and the
/// per-row visual [`build_row`] SSOT that [`view_tree_focused`] also
/// renders — so a windowed row is byte-identical to the same row in the
/// non-virtual path, and only the slot count differs. The caller passes
/// the full [`flat_visible`] sequence; `view_virtual_tree` resolves the
/// window over its length and invokes [`build_row`] once per visible
/// index. Off-window rows never become scene nodes, so `scene/snapshot`
/// reports only the windowed `{tag}#{id}` rows (the §2 #7 witness that the
/// tree is virtualized, not merely clipped).
///
/// The clip-window height is read from the measured viewport
/// ([`ScrollState::measured_viewport`]) like [`view_flex_virtual_list`],
/// so the rendered row count follows the pane's real flex-computed size
/// (the `AutoSizer` pattern) rather than a caller const.
///
/// # Parameters
///
/// - `tag` — the tree's composite-tag prefix; each windowed row is tagged
///   `{tag}#{id}` so row clicks route through the
///   [[multi-external-substrate-extra-externals-pattern]] like the
///   non-virtual tree.
/// - `scroll` — the reactive [`ScrollState`] shared with the scrollbar
///   peer; both the offset and the measured viewport are read from it.
/// - `rows` — the full [`flat_visible`] flattening of the visible tree
///   (the same SSOT the keyboard model and AT tree read); only the
///   windowed slice is rendered.
/// - `focus` — the keyboard-focus carrier; the focused row paints the M3
///   state-layer exactly as in [`view_tree_focused`].
/// - `overscan` — rows rendered beyond the strict window on each side.
///
/// The uniform per-row slot pitch is [`TreeViewStyle::row_height`]: the
/// non-virtual [`view_tree_focused`] stacks rows at their natural
/// `row_height` with no gap, so the windowed slot pitch must equal it for
/// the two paths to render identically (deriving it rather than taking a
/// separate `row_pitch` argument removes the misalignment footgun).
///
/// [`view_flex_virtual_list`]: crate::virtual_list::view_flex_virtual_list
#[must_use]
pub fn view_virtual_tree(
    tag: &'static str,
    scroll: &Rc<ScrollState>,
    rows: &[VisibleRow],
    focus: &TreeViewFocus<'_>,
    overscan: usize,
    theme: &Theme,
    style: &TreeViewStyle,
) -> Scene {
    let row_pitch = style.row_height;
    let (measured_w, measured_h) = scroll.measured_viewport();
    let window = compute_visible_range(scroll.offset_y(), measured_h, rows.len(), row_pitch, overscan);
    let total_h = content_height(rows.len(), row_pitch);
    let slots = uniform_slots(&window, measured_w, row_pitch, |index| {
        let row = &rows[index];
        let is_focused = focus.focused_id == Some(row.id.as_str());
        build_row(tag, row, theme, style, is_focused)
    });
    assemble_windowed_flex(scroll, measured_w, total_h, slots, false)
}

/// Compose one row from its [`VisibleRow`] (the shared flattening): depth
/// indent + expand glyph + label. R811.1 §5.50 — takes the flattened row
/// rather than re-walking the `TreeItem` tree, so the glyph / indent /
/// label / composite-tag all read the same `depth` / `has_children` /
/// `expanded` the keyboard model and AT tree resolve against.
fn build_row(
    tree_tag: &'static str,
    row: &VisibleRow,
    theme: &Theme,
    style: &TreeViewStyle,
    is_focused: bool,
) -> Scene {
    let glyph = if !row.has_children {
        GLYPH_LEAF
    } else if row.expanded {
        GLYPH_EXPANDED
    } else {
        GLYPH_COLLAPSED
    };
    let label_color = theme.resolve(ColorRole::OnSurface);
    let glyph_color = theme.resolve(ColorRole::OnSurfaceMuted);
    let indent_px = row.depth * style.indent_step;
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
        row.label.as_str(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.font_size_px)
            .with_fg(label_color),
    )));
    let row_tag = composite_row_tag(tree_tag, &row.id);
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

/// R675 §5.16 §5.20 §5.50 — bare event name [`TreeRowClickExternal`]
/// emits on a completed Down/Up click cycle on a tree row.
///
/// The runtime intent-queue walker prefixes this with the producing
/// `Scene::External` node's tag (the tree's primary tag — typically
/// the same string passed as `tag` to [`view_tree`]) to form the
/// dotted wire form `{tree_tag}.click` that
/// [`pinion_core::WidgetCore::update`] reducers match against (per
/// [[intent-tag-dotted-wire-form]]).
///
/// Bindings compose the literal via
/// `pinion_core::intent_tag!("<tree_tag>", TREE_ROW_CLICK_EVENT)` —
/// the macro is `literal`-only at the stable-Rust layer so the tree
/// tag has to be a literal at the call site (matches the
/// `intent_tag!` contract used everywhere else).
pub const TREE_ROW_CLICK_EVENT: &str = "click";

/// R678 §5.16 §5.20 §5.50 — bare event name [`TreeRowClickExternal`]
/// emits on a hover-state transition (`PointerEnter` or `PointerLeave`
/// on a row). The payload distinguishes Enter from Leave:
///
/// * `IntrospectValue::Text(id)` — pointer entered row `id` (the new
///   `hovered_id`). Fires once per enter; idempotent re-Enter on the
///   same id is silent.
/// * `IntrospectValue::Null` — pointer left the previously-hovered
///   row (`hovered_id` is now `None`). Fires once per leave.
///
/// W3C canonical Leave-before-Enter ordering when the cursor crosses
/// directly from one row to another: the `InputRouter` dispatches
/// `PointerLeave` on the old tag first, then `PointerEnter` on the
/// new tag — the consumer sees two `hover` intents (`Null` then
/// `Text(new_id)`) in that order.
///
/// Mirrors [`TREE_ROW_CLICK_EVENT`]'s compose-via-`intent_tag!`
/// contract — `pinion_core::intent_tag!("<tree_tag>", TREE_ROW_HOVER_EVENT)`
/// at the binding's reducer match arm.
pub const TREE_ROW_HOVER_EVENT: &str = "hover";

/// R675 §5.16 §5.20 §5.50 — tree-row click + hover router External.
///
/// Lifted at R675 from the R674 binding-level `FileTreeRowExternal`
/// in `examples/hello-tree-view`. The 2nd consumer
/// (`examples/hello-multi-window` inspector window) fired the
/// [[abstraction-needs-second-consumer]] Rule-of-Three gate, so the
/// substrate moves into `pinion_widget_paint::tree_view` next to
/// [`view_tree`] / [`view_tree_focused`] — every future tree
/// consumer (`DevTools` outliner, file-tree editor, property-grid
/// expander, scene-graph inspector) registers one of these alongside
/// its [`view_tree`] paint output.
///
/// R678 §5.16 §5.49 §5.50 grew the External into a two-axis router
/// — press state (R675 contract) and hover state (R678 axis). The
/// two slots are independent (W3C canonical: a row can be hovered
/// while pressed — the normal click target). Each axis has its own
/// intent stream (`click` / `hover`) so consumers reduce them
/// independently. The `DevTools` cross-window hover-highlight overlay
/// (the 2nd visible Phase D editor feature) is the first consumer of
/// the hover axis.
///
/// ## State machine — press axis (`Idle` ↔ `Pressed`)
///
/// One internal slot — `pressed_id: Option<String>`:
///
/// * `Idle` (`pressed_id = None`) + `PointerDown(id)` →
///   `Pressed(id)` (`pressed_id = Some(id)`).
/// * `Pressed(id)` + `PointerUp(same id)` → emit `click` intent
///   carrying `Text(id)`, transition to `Idle`.
/// * `Pressed(id_a)` + `PointerUp(id_b ≠ id_a)` → silent abort
///   (W3C canonical "drag-off cancels click"), transition to `Idle`.
/// * `Pressed(id)` + `PointerLeave(id)` or `PointerCancel(id)` →
///   silent abort, transition to `Idle`.
///
/// `Down`-on-A then `Down`-on-B (multi-touch with one primary
/// pointer) overwrites the pressed slot to B; the subsequent Up-on-B
/// emits. Pinion follows the single-primary-pointer convention every
/// other composite uses today; multi-touch concurrent row presses
/// are a future axis once a real consumer surfaces the need.
///
/// ## State machine — hover axis (`NotHovered` ↔ `Hovered`)
///
/// One internal slot — `hovered_id: Option<String>`:
///
/// * `NotHovered` (`hovered_id = None`) + `PointerEnter(id)` →
///   `Hovered(id)` (`hovered_id = Some(id)`), emit `hover` intent
///   carrying `Text(id)`.
/// * `Hovered(id)` + `PointerEnter(id)` (re-Enter on same row) →
///   silent no-op (idempotent — guards against `InputRouter`
///   double-dispatch).
/// * `Hovered(id_a)` + `PointerEnter(id_b ≠ id_a)` →
///   `Hovered(id_b)`, emit `hover` intent carrying `Text(id_b)`.
///   The `InputRouter` dispatches `PointerLeave(id_a)` before
///   `PointerEnter(id_b)` (W3C canonical Leave-before-Enter), so
///   consumers normally see `Null` then `Text(id_b)`; the direct
///   A-to-B path here covers the case where Leave is swallowed
///   (defensive).
/// * `Hovered(id)` + `PointerLeave(id)` → `NotHovered`, emit
///   `hover` intent carrying `Null`.
/// * `Hovered(id_a)` + `PointerLeave(id_b ≠ id_a)` → silent no-op
///   (unrelated leave does not disturb the hover slot).
/// * `Hovered(id)` + `PointerCancel(id)` → `NotHovered`, emit
///   `hover` intent carrying `Null` (cancel = same hover-clear
///   semantics as leave; the press axis aborts silently per
///   pre-R678 contract).
///
/// ## Why intent + reducer instead of direct Signal mutation
///
/// The [`pinion_core::widgets::TodoDeleteExternal`]-class fast path
/// owns a `Rc<Signal<T>>` and mutates inside its invoke handler.
/// `TreeRowClickExternal` instead funnels through the §5.23 R27
/// reducer because tree consumers vary in their state model (one
/// holds `Vec<FileNode>`, another holds `Vec<Box<SceneTreeItem>>`,
/// another holds an interior-mutable scene-graph node by-id table);
/// owning the model state would tie this substrate to one concrete
/// shape. The intent + reducer pattern keeps the External
/// model-agnostic — the binding's reducer decides what to do with
/// the row id payload.
///
/// ## Schema slots
///
/// * `pressed_id` — currently held row id (or `Null` when Idle); AI
///   clients can read mid-press without triggering input.
/// * `hovered_id` — currently hovered row id (or `Null` when
///   `NotHovered`); read-only mirror, mid-hover AI query is safe.
/// * `send` — R51.42 §5.35 composite-tag wire format
///   (`"<id>:<EventName>"`); the canonical input path the
///   [`InputRouter`] composite walker forwards through. Handles
///   `PointerDown` / `PointerUp` / `PointerLeave` / `PointerCancel`
///   (press axis) and `PointerEnter` / `PointerLeave` /
///   `PointerCancel` (hover axis) — `PointerLeave` /
///   `PointerCancel` are shared input gates that touch both slots.
/// * `click` — typed shortcut for AI-driven single-shot commit
///   (`invoke("click", Text(<id>))` synthesises a full Down + Up
///   cycle on the same id and emits the intent in one call); mirrors
///   [`pinion_core::widgets::TodoDeleteExternal`]'s `"delete"`
///   shortcut.
/// * `hover` — typed shortcut for AI-driven hover synthesis
///   (`invoke("hover", Text(<id>))` enters row `id`,
///   `invoke("hover", Null)` leaves the currently-hovered row). Each
///   call emits exactly one `hover` intent. Mirror of `click` for
///   the hover axis.
#[derive(Debug, Default)]
pub struct TreeRowClickExternal {
    pressed_id: Option<String>,
    hovered_id: Option<String>,
    pending: Vec<Intent>,
}

impl TreeRowClickExternal {
    /// Construct a fresh router with no pressed row and an empty
    /// intent buffer. Substrate calls this once at
    /// [`pinion_core::WidgetCore::create_extra_externals`] time.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// R675 §5.20 — enqueue the `click` intent for `id` on the §5.20
    /// channel. `drain_intents` ships the payload across the boundary
    /// on the next substrate drain pass.
    fn emit_click(&mut self, id: String) {
        self.pending.push(Intent::new_static(
            TREE_ROW_CLICK_EVENT,
            IntrospectValue::Text(id),
        ));
    }

    /// R678 §5.20 — enqueue the `hover` intent on the §5.20 channel
    /// with the supplied payload (`Text(id)` for Enter, `Null` for
    /// Leave). `drain_intents` ships the payload across the boundary
    /// on the next substrate drain pass.
    fn emit_hover(&mut self, payload: IntrospectValue) {
        self.pending.push(Intent::new_static(TREE_ROW_HOVER_EVENT, payload));
    }
}

impl External for TreeRowClickExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        for intent in self.pending.drain(..) {
            sink(intent);
        }
    }

    fn is_dirty(&self) -> bool {
        !self.pending.is_empty()
    }
}

impl ExternalIntrospect for TreeRowClickExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("pressed_id", "string"),
            ("hovered_id", "string"),
            ("send", "string"),
            ("click", "string"),
            ("hover", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "pressed_id" => Some(match &self.pressed_id {
                Some(id) => IntrospectValue::Text(id.clone()),
                None => IntrospectValue::Null,
            }),
            "hovered_id" => Some(match &self.hovered_id {
                Some(id) => IntrospectValue::Text(id.clone()),
                None => IntrospectValue::Null,
            }),
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        _value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        match path {
            "pressed_id" | "hovered_id" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    // R659 §5.16 — shared composite-tag parser
                    // (R675: 7th substrate consumer — 6-of-6 framework
                    // at R674 plus this substrate lift takes it to
                    // framework-level Rule-of-Three maturity).
                    // R781 — modifiers ignored (tree press has no modifier axis).
                    let (id, event_name, _): (String, &str, _) =
                        parse_send_payload(payload).ok_or(InvokeError::Rejected)?;
                    match PointerWireEvent::from_wire_name(event_name) {
                        Some(PointerWireEvent::Down) => {
                            self.pressed_id = Some(id);
                            Ok(IntrospectValue::Bool(true))
                        }
                        Some(PointerWireEvent::Up) => {
                            let armed = self
                                .pressed_id
                                .as_ref()
                                .is_some_and(|p| p == &id);
                            self.pressed_id = None;
                            if armed {
                                self.emit_click(id);
                                Ok(IntrospectValue::Bool(true))
                            } else {
                                Ok(IntrospectValue::Bool(false))
                            }
                        }
                        Some(PointerWireEvent::Enter) => {
                            // R678 hover axis: set hovered_id and emit
                            // a `hover` intent carrying `Text(id)`.
                            // Idempotent re-Enter on the same id is
                            // silent (no state change, no intent).
                            if self.hovered_id.as_deref() == Some(id.as_str()) {
                                Ok(IntrospectValue::Bool(false))
                            } else {
                                self.hovered_id = Some(id.clone());
                                self.emit_hover(IntrospectValue::Text(id));
                                Ok(IntrospectValue::Bool(true))
                            }
                        }
                        Some(PointerWireEvent::Cancel | PointerWireEvent::Leave) => {
                            // R678 hover axis: clearing the hover slot
                            // matches the W3C "left/cancel = no longer
                            // hovering" semantics. Emit a `hover`
                            // intent carrying `Null` so consumers see
                            // a deterministic Enter/Leave round trip.
                            // Unrelated leaves (different id from the
                            // currently-hovered row) are silent.
                            if self.hovered_id.as_deref() == Some(id.as_str()) {
                                self.hovered_id = None;
                                self.emit_hover(IntrospectValue::Null);
                            }
                            // Press axis (pre-R678 behaviour
                            // preserved): drop the pressed slot if
                            // the leave/cancel id matches the press,
                            // silently aborting the in-flight click.
                            if self
                                .pressed_id
                                .as_ref()
                                .is_some_and(|p| p == &id)
                            {
                                self.pressed_id = None;
                            }
                            // Pre-R678 contract: leave/cancel returns
                            // Bool(false) regardless of slot effects.
                            // The behavioural contract is the emitted
                            // intent stream; the Bool return is
                            // informational only and stays compatible
                            // with R675 test expectations.
                            Ok(IntrospectValue::Bool(false))
                        }
                        None => Ok(IntrospectValue::Bool(false)),
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "click" => match args {
                IntrospectValue::Text(id) => {
                    self.pressed_id = None;
                    self.emit_click(id);
                    Ok(IntrospectValue::Bool(true))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "hover" => match args {
                // R678 typed hover shortcut: `Text(id)` enters row
                // `id` (idempotent on the currently-hovered row),
                // `Null` leaves whatever row is hovered (no-op if
                // none). Each call emits at most one `hover` intent.
                IntrospectValue::Text(id) => {
                    if self.hovered_id.as_deref() == Some(id.as_str()) {
                        Ok(IntrospectValue::Bool(false))
                    } else {
                        self.hovered_id = Some(id.clone());
                        self.emit_hover(IntrospectValue::Text(id));
                        Ok(IntrospectValue::Bool(true))
                    }
                }
                IntrospectValue::Null => {
                    if self.hovered_id.take().is_some() {
                        self.emit_hover(IntrospectValue::Null);
                        Ok(IntrospectValue::Bool(true))
                    } else {
                        Ok(IntrospectValue::Bool(false))
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
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

#[cfg(test)]
mod r675_tree_row_click_external_tests {
    //! R675 §5.16 §5.20 §5.50 — [`TreeRowClickExternal`] substrate
    //! state-machine + intent-emission contract. Lifted from the R674
    //! `examples/hello-tree-view::r674_file_tree_row_external_tests`
    //! module — same behaviours, now pinned at the substrate layer
    //! where every future tree consumer inherits coverage.

    use super::{TreeRowClickExternal, TREE_ROW_CLICK_EVENT};
    use pinion_core::external::{
        External, ExternalIntrospect, InterveneError, IntrospectValue, InvokeError,
    };
    use pinion_core::intent::Intent;

    fn send(handler: &mut TreeRowClickExternal, payload: &str) -> IntrospectValue {
        handler
            .invoke("send", IntrospectValue::Text(payload.to_string()))
            .expect("`send` invoke must accept well-formed payload")
    }

    fn drain(handler: &mut TreeRowClickExternal) -> Vec<Intent> {
        let mut out = Vec::new();
        handler.drain_intents(&mut |i| out.push(i));
        out
    }

    #[test]
    fn r675_new_external_has_no_pressed_id_and_is_clean() {
        let handler = TreeRowClickExternal::new();
        assert_eq!(
            handler.query("pressed_id").unwrap(),
            IntrospectValue::Null,
            "fresh external must report no pressed row"
        );
        assert!(
            !handler.is_dirty(),
            "fresh external must not have queued intents"
        );
    }

    #[test]
    fn r675_pointer_down_records_pressed_id() {
        let mut handler = TreeRowClickExternal::new();
        let out = send(&mut handler, "src:PointerDown");
        assert_eq!(out, IntrospectValue::Bool(true));
        assert_eq!(
            handler.query("pressed_id").unwrap(),
            IntrospectValue::Text("src".to_string()),
        );
        assert!(
            !handler.is_dirty(),
            "PointerDown alone must not queue an intent"
        );
    }

    #[test]
    fn r675_matched_down_up_emits_click_intent_with_text_id() {
        let mut handler = TreeRowClickExternal::new();
        send(&mut handler, "tests:PointerDown");
        let up_out = send(&mut handler, "tests:PointerUp");
        assert_eq!(up_out, IntrospectValue::Bool(true));
        assert_eq!(
            handler.query("pressed_id").unwrap(),
            IntrospectValue::Null,
            "PointerUp must release the pressed slot",
        );
        let harvested = drain(&mut handler);
        assert_eq!(harvested.len(), 1, "matched Down→Up emits exactly one intent");
        assert_eq!(harvested[0].tag_str(), TREE_ROW_CLICK_EVENT);
        assert_eq!(
            harvested[0].payload,
            IntrospectValue::Text("tests".to_string()),
        );
    }

    #[test]
    fn r675_pointer_up_without_prior_down_is_silent() {
        let mut handler = TreeRowClickExternal::new();
        let out = send(&mut handler, "src:PointerUp");
        assert_eq!(out, IntrospectValue::Bool(false));
        assert!(
            drain(&mut handler).is_empty(),
            "Up without armed press is W3C canonical no-op",
        );
    }

    #[test]
    fn r675_mismatched_up_aborts_silently() {
        let mut handler = TreeRowClickExternal::new();
        send(&mut handler, "src:PointerDown");
        let up_out = send(&mut handler, "tests:PointerUp");
        assert_eq!(up_out, IntrospectValue::Bool(false));
        assert_eq!(
            handler.query("pressed_id").unwrap(),
            IntrospectValue::Null,
            "mismatched Up still releases the pressed slot",
        );
        assert!(
            drain(&mut handler).is_empty(),
            "drag-off must not emit a click intent",
        );
    }

    #[test]
    fn r675_pointer_cancel_on_pressed_row_aborts() {
        let mut handler = TreeRowClickExternal::new();
        send(&mut handler, "src:PointerDown");
        let out = send(&mut handler, "src:PointerCancel");
        assert_eq!(out, IntrospectValue::Bool(false));
        assert_eq!(
            handler.query("pressed_id").unwrap(),
            IntrospectValue::Null,
        );
        assert!(drain(&mut handler).is_empty());
    }

    #[test]
    fn r675_pointer_leave_on_pressed_row_aborts() {
        let mut handler = TreeRowClickExternal::new();
        send(&mut handler, "src:PointerDown");
        let out = send(&mut handler, "src:PointerLeave");
        assert_eq!(out, IntrospectValue::Bool(false));
        assert_eq!(
            handler.query("pressed_id").unwrap(),
            IntrospectValue::Null,
        );
        assert!(drain(&mut handler).is_empty());
    }

    #[test]
    fn r675_unrelated_leave_does_not_disturb_active_press() {
        let mut handler = TreeRowClickExternal::new();
        send(&mut handler, "src:PointerDown");
        let leave_out = send(&mut handler, "tests:PointerLeave");
        assert_eq!(leave_out, IntrospectValue::Bool(false));
        assert_eq!(
            handler.query("pressed_id").unwrap(),
            IntrospectValue::Text("src".to_string()),
            "unrelated Leave must not clear an active press",
        );
        let up_out = send(&mut handler, "src:PointerUp");
        assert_eq!(up_out, IntrospectValue::Bool(true));
        assert_eq!(drain(&mut handler).len(), 1);
    }

    #[test]
    fn r678_pointer_enter_does_not_disturb_pressed_id() {
        // R678: PointerEnter used to be a no-op on every slot — now it
        // touches the hover slot but must still leave the press slot
        // alone. Pre-R678 behaviour for the press axis is preserved
        // here so reducers that only care about clicks keep working.
        let mut handler = TreeRowClickExternal::new();
        let _ = send(&mut handler, "src:PointerEnter");
        assert_eq!(
            handler.query("pressed_id").unwrap(),
            IntrospectValue::Null,
            "PointerEnter must never set pressed_id",
        );
    }

    #[test]
    fn r675_malformed_payload_missing_separator_rejected() {
        let mut handler = TreeRowClickExternal::new();
        let result =
            handler.invoke("send", IntrospectValue::Text("PointerDown".to_string()));
        assert_eq!(result, Err(InvokeError::Rejected));
    }

    #[test]
    fn r675_empty_event_name_rejected() {
        let mut handler = TreeRowClickExternal::new();
        let result = handler.invoke("send", IntrospectValue::Text("src:".to_string()));
        assert_eq!(result, Err(InvokeError::Rejected));
    }

    #[test]
    fn r675_send_non_text_args_type_mismatch() {
        let mut handler = TreeRowClickExternal::new();
        let result = handler.invoke("send", IntrospectValue::Int(7));
        assert_eq!(result, Err(InvokeError::TypeMismatch));
    }

    #[test]
    fn r675_direct_click_shortcut_emits_intent_without_press_cycle() {
        let mut handler = TreeRowClickExternal::new();
        let out = handler
            .invoke("click", IntrospectValue::Text("docs".to_string()))
            .expect("`click` shortcut must accept Text id");
        assert_eq!(out, IntrospectValue::Bool(true));
        let harvested = drain(&mut handler);
        assert_eq!(harvested.len(), 1);
        assert_eq!(
            harvested[0].payload,
            IntrospectValue::Text("docs".to_string()),
        );
    }

    #[test]
    fn r675_drain_intents_clears_buffer_idempotent() {
        let mut handler = TreeRowClickExternal::new();
        send(&mut handler, "src:PointerDown");
        send(&mut handler, "src:PointerUp");
        assert!(handler.is_dirty());
        let first = drain(&mut handler);
        assert_eq!(first.len(), 1);
        assert!(!handler.is_dirty(), "drain must clear the buffer");
        let second = drain(&mut handler);
        assert!(second.is_empty(), "second drain on cleared buffer is no-op");
    }

    #[test]
    fn r675_intervene_pressed_id_is_read_only() {
        let mut handler = TreeRowClickExternal::new();
        let result = handler
            .intervene("pressed_id", IntrospectValue::Text("forged".to_string()));
        assert_eq!(result, Err(InterveneError::ReadOnly));
    }

    #[test]
    fn r675_invoke_unknown_path_rejected() {
        let mut handler = TreeRowClickExternal::new();
        let result = handler.invoke("ghost", IntrospectValue::Null);
        assert_eq!(result, Err(InvokeError::UnknownPath));
    }

    #[test]
    fn r675_event_name_constant_is_canonical_click() {
        // Pin the substrate constant so a future rename to e.g.
        // "activate" surfaces the lockstep break immediately. Tree
        // consumer reducers compose the dotted form via
        // intent_tag!(MY_TREE_TAG, TREE_ROW_CLICK_EVENT) literally.
        assert_eq!(TREE_ROW_CLICK_EVENT, "click");
    }
}

#[cfg(test)]
mod r678_tree_row_hover_external_tests {
    //! R678 §5.16 §5.20 §5.49 §5.50 — [`TreeRowClickExternal`] hover
    //! axis (new at R678; press axis test contract lives in the
    //! sibling `r675_tree_row_click_external_tests` module and is
    //! unchanged). The hover axis owns one slot — `hovered_id:
    //! Option<String>` — and emits one intent name —
    //! [`TREE_ROW_HOVER_EVENT`]. The `DevTools` cross-window
    //! hover-highlight overlay in `examples/hello-multi-window` is
    //! the first consumer.

    use super::{TreeRowClickExternal, TREE_ROW_CLICK_EVENT, TREE_ROW_HOVER_EVENT};
    use pinion_core::external::{
        External, ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue,
        InvokeError,
    };
    use pinion_core::intent::Intent;

    fn send(handler: &mut TreeRowClickExternal, payload: &str) -> IntrospectValue {
        handler
            .invoke("send", IntrospectValue::Text(payload.to_string()))
            .expect("`send` invoke must accept well-formed payload")
    }

    fn drain(handler: &mut TreeRowClickExternal) -> Vec<Intent> {
        let mut out = Vec::new();
        handler.drain_intents(&mut |i| out.push(i));
        out
    }

    #[test]
    fn r678_new_external_has_no_hovered_id_and_query_returns_null() {
        let handler = TreeRowClickExternal::new();
        assert_eq!(
            handler.query("hovered_id").unwrap(),
            IntrospectValue::Null,
            "fresh external must report no hover slot",
        );
    }

    #[test]
    fn r678_pointer_enter_records_hovered_id_and_emits_hover_intent() {
        let mut handler = TreeRowClickExternal::new();
        let out = send(&mut handler, "src:PointerEnter");
        assert_eq!(out, IntrospectValue::Bool(true));
        assert_eq!(
            handler.query("hovered_id").unwrap(),
            IntrospectValue::Text("src".to_string()),
            "PointerEnter must set hovered_id",
        );
        let intents = drain(&mut handler);
        assert_eq!(intents.len(), 1, "Enter emits exactly one hover intent");
        assert_eq!(intents[0].tag_str(), TREE_ROW_HOVER_EVENT);
        assert_eq!(
            intents[0].payload,
            IntrospectValue::Text("src".to_string()),
            "Enter payload carries the new hovered_id",
        );
    }

    #[test]
    fn r678_pointer_leave_clears_hovered_id_and_emits_null_hover_intent() {
        let mut handler = TreeRowClickExternal::new();
        let _ = send(&mut handler, "src:PointerEnter");
        let _ = drain(&mut handler);
        let out = send(&mut handler, "src:PointerLeave");
        // Pre-R678 contract: Leave returns Bool(false). The hover
        // axis effect is conveyed via the emitted intent stream.
        assert_eq!(out, IntrospectValue::Bool(false));
        assert_eq!(
            handler.query("hovered_id").unwrap(),
            IntrospectValue::Null,
            "PointerLeave on hovered row must clear the slot",
        );
        let intents = drain(&mut handler);
        assert_eq!(intents.len(), 1, "Leave emits exactly one hover intent");
        assert_eq!(intents[0].tag_str(), TREE_ROW_HOVER_EVENT);
        assert_eq!(
            intents[0].payload,
            IntrospectValue::Null,
            "Leave payload signals 'no row hovered' with Null",
        );
    }

    #[test]
    fn r678_pointer_enter_idempotent_on_already_hovered_row() {
        let mut handler = TreeRowClickExternal::new();
        let _ = send(&mut handler, "src:PointerEnter");
        let _ = drain(&mut handler);
        // Same id re-Enter — defensive against InputRouter
        // double-dispatch on stationary cursor + paint-cycle hover
        // refresh. Must be silent (no slot change, no intent).
        let out = send(&mut handler, "src:PointerEnter");
        assert_eq!(out, IntrospectValue::Bool(false));
        assert_eq!(
            handler.query("hovered_id").unwrap(),
            IntrospectValue::Text("src".to_string()),
        );
        assert!(
            drain(&mut handler).is_empty(),
            "idempotent re-Enter must not queue a second hover intent",
        );
    }

    #[test]
    fn r678_pointer_leave_unrelated_row_silent() {
        let mut handler = TreeRowClickExternal::new();
        let _ = send(&mut handler, "src:PointerEnter");
        let _ = drain(&mut handler);
        // Leave fires for a different id (defensive — the InputRouter
        // never normally sends this combination, but if a backend
        // synthesises it the substrate must not corrupt the live
        // hover slot).
        let out = send(&mut handler, "tests:PointerLeave");
        assert_eq!(out, IntrospectValue::Bool(false));
        assert_eq!(
            handler.query("hovered_id").unwrap(),
            IntrospectValue::Text("src".to_string()),
            "unrelated Leave must not disturb the active hover slot",
        );
        assert!(
            drain(&mut handler).is_empty(),
            "unrelated Leave must not emit a hover intent",
        );
    }

    #[test]
    fn r678_pointer_leave_without_prior_enter_silent() {
        let mut handler = TreeRowClickExternal::new();
        let out = send(&mut handler, "src:PointerLeave");
        assert_eq!(out, IntrospectValue::Bool(false));
        assert_eq!(
            handler.query("hovered_id").unwrap(),
            IntrospectValue::Null,
        );
        assert!(drain(&mut handler).is_empty());
    }

    #[test]
    fn r678_pointer_cancel_clears_hover_and_emits_null_intent() {
        let mut handler = TreeRowClickExternal::new();
        let _ = send(&mut handler, "src:PointerEnter");
        let _ = drain(&mut handler);
        let out = send(&mut handler, "src:PointerCancel");
        assert_eq!(out, IntrospectValue::Bool(false));
        assert_eq!(
            handler.query("hovered_id").unwrap(),
            IntrospectValue::Null,
            "PointerCancel on hovered row clears the slot",
        );
        let intents = drain(&mut handler);
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].tag_str(), TREE_ROW_HOVER_EVENT);
        assert_eq!(intents[0].payload, IntrospectValue::Null);
    }

    #[test]
    fn r678_w3c_leave_before_enter_emits_two_hover_intents() {
        // W3C canonical move-from-A-to-B: InputRouter dispatches
        // PointerLeave(A) then PointerEnter(B). The substrate must
        // emit two hover intents (Null then Text(B)) in that order.
        let mut handler = TreeRowClickExternal::new();
        let _ = send(&mut handler, "src:PointerEnter");
        let _ = drain(&mut handler);
        let _ = send(&mut handler, "src:PointerLeave");
        let _ = send(&mut handler, "docs:PointerEnter");
        let intents = drain(&mut handler);
        assert_eq!(intents.len(), 2, "Leave→Enter emits two ordered intents");
        assert_eq!(intents[0].tag_str(), TREE_ROW_HOVER_EVENT);
        assert_eq!(intents[0].payload, IntrospectValue::Null);
        assert_eq!(intents[1].tag_str(), TREE_ROW_HOVER_EVENT);
        assert_eq!(intents[1].payload, IntrospectValue::Text("docs".to_string()));
        assert_eq!(
            handler.query("hovered_id").unwrap(),
            IntrospectValue::Text("docs".to_string()),
            "post-W3C-transition hover_id is the new target",
        );
    }

    #[test]
    fn r678_direct_enter_to_different_row_swaps_hover_slot() {
        // Defensive — when Leave is swallowed (backend bug, dropped
        // event), an Enter on a *different* id must still update the
        // slot. Emits one hover intent carrying the new id.
        let mut handler = TreeRowClickExternal::new();
        let _ = send(&mut handler, "src:PointerEnter");
        let _ = drain(&mut handler);
        let out = send(&mut handler, "docs:PointerEnter");
        assert_eq!(out, IntrospectValue::Bool(true));
        assert_eq!(
            handler.query("hovered_id").unwrap(),
            IntrospectValue::Text("docs".to_string()),
        );
        let intents = drain(&mut handler);
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].payload, IntrospectValue::Text("docs".to_string()));
    }

    #[test]
    fn r678_hover_axis_and_press_axis_independent() {
        // Hover during a press: Enter while pressed_id is set must
        // not disturb the press slot, and vice versa. The canonical
        // "click target" state — pointer hovering over the row being
        // clicked.
        let mut handler = TreeRowClickExternal::new();
        let _ = send(&mut handler, "src:PointerDown");
        let _ = send(&mut handler, "src:PointerEnter");
        assert_eq!(
            handler.query("pressed_id").unwrap(),
            IntrospectValue::Text("src".to_string()),
            "Enter during press preserves press slot",
        );
        assert_eq!(
            handler.query("hovered_id").unwrap(),
            IntrospectValue::Text("src".to_string()),
            "Enter during press records hover slot",
        );
        // PointerUp on the same row should still fire a click and
        // leave the hover slot intact.
        let _ = send(&mut handler, "src:PointerUp");
        assert_eq!(
            handler.query("pressed_id").unwrap(),
            IntrospectValue::Null,
            "Up releases the press slot",
        );
        assert_eq!(
            handler.query("hovered_id").unwrap(),
            IntrospectValue::Text("src".to_string()),
            "Up leaves the hover slot untouched (pointer is still over the row)",
        );
        let intents = drain(&mut handler);
        // 2 intents: 1 hover Enter + 1 click Up.
        assert_eq!(intents.len(), 2);
        assert_eq!(intents[0].tag_str(), TREE_ROW_HOVER_EVENT);
        assert_eq!(intents[0].payload, IntrospectValue::Text("src".to_string()));
        assert_eq!(intents[1].tag_str(), TREE_ROW_CLICK_EVENT);
        assert_eq!(intents[1].payload, IntrospectValue::Text("src".to_string()));
    }

    #[test]
    fn r678_pointer_leave_on_pressed_and_hovered_row_clears_both_slots() {
        // PointerLeave on a row that is both pressed and hovered
        // must clear both slots; the hover axis emits a Null intent
        // (the press axis aborts silently per pre-R678 contract).
        let mut handler = TreeRowClickExternal::new();
        let _ = send(&mut handler, "src:PointerEnter");
        let _ = send(&mut handler, "src:PointerDown");
        let _ = drain(&mut handler);
        let out = send(&mut handler, "src:PointerLeave");
        assert_eq!(out, IntrospectValue::Bool(false));
        assert_eq!(handler.query("pressed_id").unwrap(), IntrospectValue::Null);
        assert_eq!(handler.query("hovered_id").unwrap(), IntrospectValue::Null);
        let intents = drain(&mut handler);
        assert_eq!(intents.len(), 1, "only the hover axis emits on combined leave");
        assert_eq!(intents[0].tag_str(), TREE_ROW_HOVER_EVENT);
        assert_eq!(intents[0].payload, IntrospectValue::Null);
    }

    #[test]
    fn r678_direct_hover_shortcut_emits_intent_without_send_cycle() {
        let mut handler = TreeRowClickExternal::new();
        let out = handler
            .invoke("hover", IntrospectValue::Text("src".to_string()))
            .expect("`hover` shortcut must accept Text id");
        assert_eq!(out, IntrospectValue::Bool(true));
        assert_eq!(
            handler.query("hovered_id").unwrap(),
            IntrospectValue::Text("src".to_string()),
        );
        let intents = drain(&mut handler);
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].tag_str(), TREE_ROW_HOVER_EVENT);
        assert_eq!(intents[0].payload, IntrospectValue::Text("src".to_string()));
    }

    #[test]
    fn r678_direct_hover_shortcut_null_leaves_currently_hovered_row() {
        let mut handler = TreeRowClickExternal::new();
        let _ = handler.invoke("hover", IntrospectValue::Text("src".to_string()));
        let _ = drain(&mut handler);
        let out = handler
            .invoke("hover", IntrospectValue::Null)
            .expect("`hover` Null must clear the slot");
        assert_eq!(out, IntrospectValue::Bool(true));
        assert_eq!(
            handler.query("hovered_id").unwrap(),
            IntrospectValue::Null,
        );
        let intents = drain(&mut handler);
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].payload, IntrospectValue::Null);
    }

    #[test]
    fn r678_direct_hover_shortcut_null_when_not_hovering_is_silent() {
        let mut handler = TreeRowClickExternal::new();
        let out = handler
            .invoke("hover", IntrospectValue::Null)
            .expect("Null on idle slot is a no-op");
        assert_eq!(out, IntrospectValue::Bool(false));
        assert!(drain(&mut handler).is_empty());
    }

    #[test]
    fn r678_direct_hover_shortcut_non_text_non_null_type_mismatch() {
        let mut handler = TreeRowClickExternal::new();
        let result = handler.invoke("hover", IntrospectValue::Int(7));
        assert_eq!(result, Err(InvokeError::TypeMismatch));
    }

    #[test]
    fn r678_intervene_hovered_id_is_read_only() {
        let mut handler = TreeRowClickExternal::new();
        let result = handler
            .intervene("hovered_id", IntrospectValue::Text("forged".to_string()));
        assert_eq!(result, Err(InterveneError::ReadOnly));
    }

    #[test]
    fn r678_schema_includes_hover_axis_slots() {
        let handler = TreeRowClickExternal::new();
        let schema: IntrospectSchema = handler.schema();
        let names: Vec<&str> = schema.fields.iter().map(|(name, _)| *name).collect();
        // Press axis (pre-R678).
        assert!(names.contains(&"pressed_id"));
        assert!(names.contains(&"send"));
        assert!(names.contains(&"click"));
        // Hover axis (R678 additions).
        assert!(names.contains(&"hovered_id"), "schema must expose hovered_id slot");
        assert!(names.contains(&"hover"), "schema must expose hover invoke channel");
    }

    #[test]
    fn r678_hover_event_constant_is_canonical_hover() {
        // Pin the substrate constant so a future rename surfaces the
        // lockstep break against every consumer. Bindings compose
        // the dotted intent name via
        // intent_tag!(MY_TREE_TAG, TREE_ROW_HOVER_EVENT) literally.
        assert_eq!(TREE_ROW_HOVER_EVENT, "hover");
    }
}

#[cfg(test)]
mod r819_virtual_tree_tests {
    //! R819 §5.16 §5.27 §5.50 — [`view_virtual_tree`] windows the
    //! [`flat_visible`] sequence so only the scroll-window rows become
    //! scene nodes (the §2 #7 virtualization witness), while reusing the
    //! [`build_row`] SSOT [`view_tree_focused`] also renders (a windowed
    //! row is byte-identical to the same row in the non-virtual path).
    use super::{
        view_virtual_tree, Color, ColorRole, Scene, SizeValue, Theme, TreeItem, TreeViewFocus,
        TreeViewStyle,
    };
    use pinion_core::widgets::scroll::ScrollState;
    use pinion_core::widgets::tree_nav::flat_visible;
    use pinion_core::Owner;
    use std::rc::Rc;

    const PITCH: u32 = 48;
    const OVERSCAN: usize = 2;
    const TREE_TAG: &str = "vtree";
    /// Total visible rows in the test tree — large enough that a small
    /// viewport windows far fewer than all of them.
    const TOTAL: usize = 60;

    /// `TOTAL` depth-0 leaves; flattening is the leaf list itself, so the
    /// windowing arithmetic is unambiguous to assert against.
    fn flat_tree() -> Vec<TreeItem> {
        (0..TOTAL)
            .map(|i| TreeItem::leaf(format!("n{i}"), format!("Node {i:02}")))
            .collect()
    }

    /// Render with the scroll offset + measured viewport seeded directly
    /// (bypass the cache for a deterministic unit shape, like the
    /// `virtual_list` tests). `viewport_rows` is the visible window height
    /// in whole rows.
    fn run(offset_rows: usize, viewport_rows: u32, focused: Option<&str>) -> Scene {
        let items = flat_tree();
        let rows = flat_visible(&items);
        let state = Rc::new(ScrollState::new());
        let pitch = i32::try_from(PITCH).expect("pitch fits i32");
        state.set_max(0, i32::try_from(rows.len()).expect("len fits i32") * pitch);
        state.set_measured_viewport(280, viewport_rows * PITCH);
        state.scroll_to(0, i32::try_from(offset_rows).expect("offset fits i32") * pitch);
        let focus = TreeViewFocus {
            focused_id: focused,
        };
        // `view_virtual_tree` derives its slot pitch from
        // `style.row_height`; `m3_default().row_height == PITCH`, so the
        // seeded scroll math (which uses `PITCH`) stays consistent.
        assert_eq!(TreeViewStyle::m3_default().row_height, PITCH);
        Owner::new().run(|| {
            view_virtual_tree(
                TREE_TAG,
                &state,
                &rows,
                &focus,
                OVERSCAN,
                &Theme::light(),
                &TreeViewStyle::m3_default(),
            )
        })
    }

    /// Collect the `vtree#<id>` row ids that became scene nodes,
    /// descending the `Scroll` wrapper + nested containers.
    fn rendered_ids(scene: &Scene) -> Vec<String> {
        fn walk(scene: &Scene, prefix: &str, out: &mut Vec<String>) {
            match scene {
                Scene::Scroll(s) => walk(s.content.as_ref(), prefix, out),
                Scene::Container(c) => {
                    if let Some(tag) = c.tag.as_deref() {
                        if let Some(id) = tag.strip_prefix(prefix) {
                            out.push(id.to_string());
                        }
                    }
                    for child in &c.children {
                        walk(child, prefix, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(scene, &format!("{TREE_TAG}#"), &mut out);
        out
    }

    /// The fill color of the `vtree#<id>` row container, or `None` if the
    /// row was not rendered (off-window).
    fn row_fill(scene: &Scene, id: &str) -> Option<Color> {
        fn walk(scene: &Scene, want: &str) -> Option<Color> {
            match scene {
                Scene::Scroll(s) => walk(s.content.as_ref(), want),
                Scene::Container(c) => {
                    if c.tag.as_deref() == Some(want) {
                        return Some(c.style.fill);
                    }
                    c.children.iter().find_map(|ch| walk(ch, want))
                }
                _ => None,
            }
        }
        walk(scene, &format!("{TREE_TAG}#{id}"))
    }

    #[test]
    fn renders_only_the_window_not_the_whole_tree() {
        // 5-row viewport + 2 overscan each side -> ~9 rows, far below 60.
        let scene = run(0, 5, None);
        let ids = rendered_ids(&scene);
        assert!(
            ids.len() < 15,
            "windowed render is a small slice, got {} of {TOTAL}",
            ids.len()
        );
        assert!(ids.contains(&"n0".to_string()), "first window row present");
        assert!(
            !ids.contains(&"n59".to_string()),
            "off-window last row never became a scene node"
        );
    }

    #[test]
    fn window_slides_with_scroll() {
        let top = rendered_ids(&run(0, 5, None));
        let mid = rendered_ids(&run(30, 5, None));
        assert!(top.contains(&"n0".to_string()), "row 0 in the top window");
        assert!(
            !top.contains(&"n30".to_string()),
            "row 30 is below the top window"
        );
        assert!(
            mid.contains(&"n30".to_string()),
            "row 30 materializes in the scrolled window"
        );
        assert!(
            !mid.contains(&"n0".to_string()),
            "row 0 left the scrolled window (off-window rows are dropped)"
        );
    }

    #[test]
    fn sizer_height_spans_the_full_dataset_not_the_window() {
        // The scroll extent reflects all TOTAL rows so the scrollbar
        // thumb sizes against the whole tree, not the rendered slice.
        let scene = run(0, 5, None);
        let Scene::Scroll(s) = &scene else {
            panic!("root must be a Scroll");
        };
        let Scene::Container(wrapper) = s.content.as_ref() else {
            panic!("content root must be the wrapper Container");
        };
        let Scene::Container(sizer) = &wrapper.children[0] else {
            panic!("wrapper child must be the full-height sizer");
        };
        let SizeValue::Px(h) = sizer.layout.size.height else {
            panic!("sizer height must be Px");
        };
        assert_eq!(
            h,
            u32::try_from(TOTAL).expect("total fits u32") * PITCH,
            "sizer spans the full dataset height"
        );
    }

    #[test]
    fn windowed_row_reuses_build_row_focus_ssot() {
        // The focused window row paints the M3 focus fill via the same
        // `build_row` the non-virtual `view_tree_focused` uses; unfocused
        // rows stay transparent.
        let scene = run(0, 5, Some("n2"));
        let focus_bg = Theme::light().resolve(ColorRole::SurfaceContainerHighest);
        assert_eq!(
            row_fill(&scene, "n2"),
            Some(focus_bg),
            "focused window row carries the M3 focus state-layer"
        );
        assert_ne!(
            row_fill(&scene, "n1"),
            Some(focus_bg),
            "unfocused window row stays transparent"
        );
    }
}
