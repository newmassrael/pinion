//! R691 §5.16 §5.40 §5.50 — backend-agnostic `MenuBar` paint
//! composition.
//!
//! Phase B widget-catalog entry — the editor `File` / `Edit` / `View`
//! menubar primitive. Two paint fns mirror the menubar's two visible
//! regions:
//!
//! - [`view_menu_bar`] — the always-visible horizontal title strip.
//! - [`view_menu_dropdown`] — the floating command dropdown shown when
//!   one title is open. Positioned with `LayoutStyle::absolute_position`
//!   (CSS `position: absolute`) so it overlays the content below the
//!   bar instead of pushing it down; the consuming binding places it
//!   **last** in the root's child list so it paints on top.
//!
//! ## Command-class, not selection
//!
//! Items are one-shot commands (WAI-ARIA `menuitem`), so the binding
//! drives selection-free state through
//! [`MenuBarExternal`](pinion_core::widgets::menu::MenuBarExternal),
//! not a [`RadioGroupExternal`](pinion_core::widgets::radio_group::RadioGroupExternal).
//! This module owns only the *paint* axis; the binding owns the
//! External + keyboard + a11y walker (mirrors [`crate::tabs`]).
//!
//! ## Composite tags
//!
//! Top-level titles are tagged [`composite_title_tag`]
//! (`{bar}#t{index}`); dropdown items [`composite_item_tag`]
//! (`{bar}#i{index}`). The input router splits both at `#` and rewrites
//! cursor hits into `invoke("send", "t{m}:<Event>")` /
//! `invoke("send", "i{i}:<Event>")` against the single shared
//! `MenuBarExternal` — the `t` / `i` prefix discriminates title vs item
//! ([[multi-external-substrate-extra-externals-pattern]]).
//!
//! ## Fixed title slots (R691 first-slice scope)
//!
//! Titles occupy fixed-width slots ([`MenuStyle::title_slot_width`]) so
//! the dropdown's `x` anchor (`menu_index * title_slot_width`) is
//! computable in the pure view fn without a post-layout feedback pass.
//! Content-width titles + content-anchored dropdowns (reading each
//! title's painted rect from the layout cache, mirroring
//! [[post-view-layout-cache-reuse]]) are a future axis once a consumer
//! needs proportional title widths.
//!
//! ## R985 — cascading submenus
//!
//! [`view_menu_cascade`] paints the open chain: the top dropdown plus one
//! nested popup per open [`pinion_core::widgets::menu::MenuItem::Submenu`]
//! level. A submenu parent row carries a trailing chevron
//! ([`MenuItemView::submenu`]); each nested popup is absolutely positioned
//! to the right of its parent row (`x += dropdown_width`, `y` aligned to the
//! parent item). Item composite tags carry the full descent path
//! ([`composite_item_tag`] `{bar}#i{a}.{b}.…` — `{bar}#i2.0` is item 0 of
//! submenu 2), so the router rewrites a nested-item hit into
//! `send("i2.0:<Event>")`. The geometry is pure arithmetic in the view fn
//! (no post-layout feedback), matching the fixed-title-slot rationale above.
//!
//! ## Future axes (per [[abstraction-needs-second-consumer]])
//!
//! - **Leading icon / trailing accelerator label columns** per item.

use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue,
    TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::{Color, Scene};

/// R691 §5.50 — M3 hover state-layer weight for the active (keyboard /
/// hovered) dropdown item, painted as an `OnSurface` overlay over the
/// dropdown surface. A divergent state-layer consumer (active-item, not a
/// standard interaction enum), so it keeps its own arm but references the
/// shared [`crate::state_layer::HOVER`] token (R754.1 — clears an R752
/// residual where this held a raw `0.08`).
const ACTIVE_ITEM_STATE_LAYER: f32 = crate::state_layer::HOVER;

/// R805 — the leading glyph painted in a checked checkbox item's gutter
/// (U+2713 CHECK MARK). A named escaped const per the repo's no-raw-
/// non-ASCII-literal rule.
const CHECK_GLYPH: &str = "\u{2713}";

/// R985 — the trailing glyph painted on a submenu-parent row (U+25B8 BLACK
/// RIGHT-POINTING SMALL TRIANGLE), the cascade "opens a submenu" affordance.
/// A named escaped const per the repo's no-raw-non-ASCII-literal rule.
const SUBMENU_CHEVRON: &str = "\u{25B8}";

/// R805 — height of a separator's divider rule in logical pixels (a thin
/// M3 menu divider centred in an otherwise-empty item row).
const SEPARATOR_RULE_HEIGHT: u32 = 1;

/// R805 §5.40 — presentation descriptor for one dropdown row: the label
/// plus the WAI-ARIA stateful-item adornments the binding maps from its
/// [`MenuItem`](pinion_core::widgets::menu::MenuItem) model. The paint
/// layer stays decoupled from the External model type — the binding
/// projects each item into this view shape (mirrors how `tabs` /
/// `radio_composite` take presentation structs, not the coordinator).
#[derive(Clone, Copy, Debug)]
pub struct MenuItemView<'a> {
    /// The visible label (ignored for a [`Self::separator`]).
    pub label: &'a str,
    /// `Some(checked)` paints a leading checkbox gutter (the check glyph
    /// when `true`, an empty gutter when `false`); `None` is a plain
    /// command row that still indents to the gutter when the dropdown
    /// reserves one.
    pub checkmark: Option<bool>,
    /// A non-interactive divider row (a thin rule; no label / state-layer).
    pub separator: bool,
    /// A disabled row paints its label muted and never takes a state-layer.
    pub enabled: bool,
    /// R985 — a submenu parent row: paints a trailing `SUBMENU_CHEVRON`
    /// and (like a command) takes a state-layer when active. Mutually
    /// exclusive with `checkmark` / `separator`.
    pub submenu: bool,
}

impl<'a> MenuItemView<'a> {
    /// A plain enabled command row.
    #[must_use]
    pub const fn command(label: &'a str) -> Self {
        Self {
            label,
            checkmark: None,
            separator: false,
            enabled: true,
            submenu: false,
        }
    }

    /// A checkbox row with the given checked state.
    #[must_use]
    pub const fn checkbox(label: &'a str, checked: bool) -> Self {
        Self {
            label,
            checkmark: Some(checked),
            separator: false,
            enabled: true,
            submenu: false,
        }
    }

    /// A non-interactive divider row.
    #[must_use]
    pub const fn separator() -> Self {
        Self {
            label: "",
            checkmark: None,
            separator: true,
            enabled: false,
            submenu: false,
        }
    }

    /// R985 — a submenu parent row (carries a trailing chevron).
    #[must_use]
    pub const fn submenu(label: &'a str) -> Self {
        Self {
            label,
            checkmark: None,
            separator: false,
            enabled: true,
            submenu: true,
        }
    }

    /// Builder: mark this row disabled (muted, no state-layer).
    #[must_use]
    pub const fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// R691 §5.50 — Material 3 `MenuBar` dimensions. Mirrors the
/// [`crate::tabs::TabsStyle`] carrier pattern so the widget catalog
/// presents a uniform `Style` surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuStyle {
    /// Title-strip height in logical pixels (M3 dense top-app-bar /
    /// menubar ≈ 40 px).
    pub bar_height: u32,
    /// Fixed width of each top-level title slot. Fixed (not
    /// content-sized) so the dropdown `x` anchor is deterministic in
    /// the pure view fn — see module docs.
    pub title_slot_width: u32,
    /// Title label font size (M3 `label-large` ≈ 14 px).
    pub title_font_px: u32,
    /// Dropdown command-item row height (M3 dense menu item ≈ 36 px).
    pub item_height: u32,
    /// Dropdown item label font size (M3 `label-large` ≈ 14 px).
    pub item_font_px: u32,
    /// Leading inset of a dropdown item label (M3 menu padding ≈ 16 px).
    pub item_padding: u32,
    /// R805 — width of the leading check-gutter column reserved when a
    /// dropdown holds any checkbox item, so checkmarks align and command
    /// rows indent to match (M3 menu leading icon slot ≈ 24 px).
    pub check_gutter_width: u32,
    /// Dropdown container width (M3 menu min-width ≈ 200 px desktop).
    pub dropdown_width: u32,
    /// Vertical padding inside the dropdown container (top + bottom
    /// each), M3 menu list padding ≈ 8 px.
    pub dropdown_v_padding: u32,
    /// Dropdown corner radius (M3 menu container ≈ 4 px).
    pub dropdown_radius: u32,
    /// Material elevation level the dropdown casts its drop-shadow at
    /// (R711 §5.50; MD3 menu = Level 2). `0` = flat.
    pub elevation: u8,
    /// (R1020 §5.39 / R1030) Keyboard focus stop for [`view_menu_bar`] (the
    /// menubar landmark) and [`view_context_menu`] (the open panel). When
    /// `true`, the tag-carrying Container is marked `.with_focusable(true)` so
    /// the scene-derived §5.39 enumeration collects its tag as a Tab stop.
    /// Default `true` (R1030 fail-safe, web native-element model); opt out with
    /// `.with_focusable(false)` for a decorative menu. Mirrors
    /// [`ButtonStyle::focusable`](crate::button::ButtonStyle::focusable).
    pub focusable: bool,
}

impl MenuStyle {
    /// R691 §5.50 — Material 3 `MenuBar` defaults. See the struct docs
    /// for the per-field token anchors.
    #[must_use]
    pub const fn m3_default() -> Self {
        Self {
            bar_height: 40,
            title_slot_width: 96,
            title_font_px: 14,
            item_height: 36,
            item_font_px: 14,
            item_padding: 16,
            check_gutter_width: 24,
            dropdown_width: 200,
            dropdown_v_padding: 8,
            dropdown_radius: 4,
            elevation: crate::elevation::MENU_LEVEL,
            focusable: true,
        }
    }

    /// (R1020 §5.39) Mark the [`view_context_menu`] panel a keyboard focus stop
    /// (default `true`). See [`Self::focusable`].
    #[must_use]
    pub const fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Total painted height of a dropdown with `item_count` items —
    /// `dropdown_v_padding` (top + bottom) + `item_count * item_height`.
    /// Public so the binding's a11y / hit-test reasoning can match the
    /// painted geometry.
    #[must_use]
    pub const fn dropdown_height(&self, item_count: u32) -> u32 {
        self.dropdown_v_padding * 2 + item_count * self.item_height
    }
}

impl Default for MenuStyle {
    fn default() -> Self {
        Self::m3_default()
    }
}

/// R691 §5.16 §5.50 — compose a top-level title's composite tag
/// (`{bar_tag}#t{index}`). The router splits at `#`; the `t` prefix
/// marks the sub-target as a title so `MenuBarExternal` routes the
/// pointer event to its title path.
#[must_use]
pub fn composite_title_tag(bar_tag: &str, index: usize) -> String {
    format!("{bar_tag}#t{index}")
}

/// R691 §5.16 §5.50 — compose a dropdown item's composite tag
/// (`{bar_tag}#i{index}`). Items route to the *same* `MenuBarExternal` as
/// the titles; the `i` prefix marks the sub-target as an item. The flat
/// (single-index) form — the common case for the menubar dropdown and the
/// context menu; the R985 cascade uses [`composite_item_path_tag`].
#[must_use]
pub fn composite_item_tag(bar_tag: &str, index: usize) -> String {
    format!("{bar_tag}#i{index}")
}

/// R988.1 §5.16 — the decode peer of [`composite_item_tag`]: parse a flat item
/// **sub-tag** (`i{index}`, the part after `#` the `InputRouter` splits off)
/// back to its index, or `None` for an unknown kind or a non-numeric index.
/// Kept adjacent to the encode so the `i<index>` wire form has one home
/// ([[wire-form-read-write-symmetry]]); the context-menu bindings reach for it
/// instead of re-rolling the parse.
#[must_use]
pub fn parse_item_sub_tag(sub_tag: &str) -> Option<usize> {
    let mut chars = sub_tag.chars();
    if chars.next()? != 'i' {
        return None;
    }
    chars.as_str().parse().ok()
}

/// R985 §5.16 §5.40 — compose a *nested* dropdown item's composite tag from
/// its descent path *relative to the open dropdown* (`[i]` = top item `i` →
/// `{bar}#i{i}`, identical to [`composite_item_tag`]; `[2, 0]` = item 0 of
/// submenu 2 → `{bar}#i2.0`). A nested-item hit rewrites to
/// `send("i2.0:<Event>")` against the shared `MenuBarExternal`.
#[must_use]
pub fn composite_item_path_tag(bar_tag: &str, path: &[usize]) -> String {
    // R985.1 — reuse the core dotted-path encode (the one wire-codec home,
    // [[wire-form-read-write-symmetry]]) rather than re-rolling the join.
    format!("{bar_tag}#i{}", pinion_core::widgets::menu::path_text(path))
}

/// R691 §5.16 §5.50 — horizontal menubar title strip.
///
/// # Arguments
///
/// - `tag` — strip container tag; the router hit-tests this as the
///   `MenuBar` scope and per-title tags are [`composite_title_tag`].
/// - `titles` — one label per top-level menu; index `m` becomes the
///   title tagged `{tag}#t{m}`.
/// - `open` — the open dropdown's menu index, or `None`. `Some(m)`
///   paints title `m` with the open-state container fill.
/// - `theme` / `style` — palette + [`MenuStyle`] carrier.
///
/// # Returns
///
/// A [`Scene::Container`] tagged `tag` laying out one fixed-width title
/// slot per label left-to-right.
#[must_use]
pub fn view_menu_bar(
    tag: &'static str,
    titles: &[&str],
    open: Option<usize>,
    theme: &Theme,
    style: &MenuStyle,
) -> Scene {
    let mut slots: Vec<Scene> = Vec::with_capacity(titles.len());
    for (m, title) in titles.iter().enumerate() {
        slots.push(build_title(tag, m, title, open == Some(m), theme, style));
    }
    Scene::Container(
        ContainerNode::new(slots)
            .with_tag(tag)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start)
                    // (R1030 §5.39) A menubar is a landmark Tab stop; carry the
                    // binding's focus opt-in onto the tag-bearing menubar node
                    // itself, not only the open dropdown panel (the R1020
                    // conversion marked the dropdown but missed the bar — the
                    // `'menu'`-addressed `focus/set` saw an unfocusable tag).
                    .with_focusable(style.focusable)
                    .with_size(Size::auto().with_height(SizeValue::Px(style.bar_height))),
            ),
    )
}

/// another declarative toolkit one top-level title slot: a fixed-width
/// centered label, background-filled when its menu is open.
fn build_title(
    bar_tag: &'static str,
    index: usize,
    title: &str,
    is_open: bool,
    theme: &Theme,
    style: &MenuStyle,
) -> Scene {
    let fill = if is_open {
        theme.resolve(ColorRole::SurfaceContainerHigh)
    } else {
        Color::TRANSPARENT
    };
    // R1543 §5.39 — a title carrying the toolkit's `&` markup declares a
    // mnemonic: `"&File"` paints `File` with an underlined `F` bound to Alt+F. Parsing
    // here rather than in each binding is what makes EVERY menubar consumer
    // accelerator-capable without editing any of them; a title with no `&` is
    // byte-identical to the pre-R1543 node.
    let label = Scene::Text(TextNode::mnemonic_styled(
        title,
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.title_font_px)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(composite_title_tag(bar_tag, index))
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::px(style.title_slot_width, style.bar_height)),
            ),
    )
}

/// R691 §5.16 §5.50 — floating dropdown for the open menu.
///
/// # Arguments
///
/// - `bar_tag` — the menubar tag; dropdown items are tagged
///   [`composite_item_tag`] `{bar_tag}#i{index}` so they route to the
///   same `MenuBarExternal` as the titles.
/// - `dropdown_tag` — the dropdown container's own tag (the binding's
///   a11y `Menu` node + the snapshot anchor).
/// - `menu_index` — which top-level menu is open; the dropdown is
///   absolutely positioned at `x = menu_index * title_slot_width`,
///   `y = bar_height` (flush under its title).
/// - `items` — command labels; index `i` becomes the item tagged
///   `{bar_tag}#i{i}`.
/// - `active` — the highlighted item (keyboard / hover active
///   descendant), painted with the M3 state-layer; `None` highlights
///   nothing.
/// - `theme` / `style` — palette + [`MenuStyle`] carrier.
///
/// # Returns
///
/// An absolutely-positioned [`Scene::Container`] tagged `dropdown_tag`:
/// a fixed-width elevated surface (corner radius + outline) laying out
/// one command row per item top-to-bottom. Place this **last** in the
/// root child list so it paints over the content below the menubar.
#[must_use]
pub fn view_menu_dropdown(
    bar_tag: &str,
    dropdown_tag: &'static str,
    menu_index: usize,
    items: &[MenuItemView],
    active: Option<usize>,
    theme: &Theme,
    style: &MenuStyle,
) -> Scene {
    let x = u32::try_from(menu_index)
        .unwrap_or(u32::MAX)
        .saturating_mul(style.title_slot_width);
    build_popup(
        bar_tag,
        dropdown_tag,
        x,
        style.bar_height,
        &[],
        items,
        active,
        theme,
        style,
    )
}

/// R985 — one open menu level within a cascade: its item rows + the active
/// (highlighted / open-child) index. The binding projects each open level
/// of its [`pinion_core::widgets::menu::MenuBar`] model into one of these.
#[derive(Clone, Copy, Debug)]
pub struct MenuLevel<'a> {
    /// The level's item rows (already projected to [`MenuItemView`]).
    pub items: &'a [MenuItemView<'a>],
    /// The highlighted item; for a parent level this is the *open submenu*
    /// index the child popup cascades from.
    pub active: Option<usize>,
}

/// R985 §5.16 §5.50 — paint an open submenu cascade: the top dropdown plus
/// one nested popup per open level. Returns one [`Scene`] per rendered level
/// (up to `level_tags.len()`); the binding splices them into the **root**
/// child list (after the dismiss barrier) so each absolutely-positioned
/// popup lands in window space and paints over the content.
///
/// Geometry is pure arithmetic: level 0 is flush under its title
/// (`x = menu_index * title_slot_width`, `y = bar_height`); each deeper
/// popup opens to the right of its parent's open row
/// (`x += dropdown_width`, `y += dropdown_v_padding + active * item_height`).
/// Item tags carry the descent path (`{bar}#i2.0`), so nested hits route
/// to the shared `MenuBarExternal`.
#[must_use]
pub fn view_menu_cascade(
    bar_tag: &str,
    level_tags: &[&'static str],
    menu_index: usize,
    levels: &[MenuLevel],
    theme: &Theme,
    style: &MenuStyle,
) -> Vec<Scene> {
    let mut out: Vec<Scene> = Vec::with_capacity(levels.len().min(level_tags.len()));
    let mut x = u32::try_from(menu_index)
        .unwrap_or(u32::MAX)
        .saturating_mul(style.title_slot_width);
    let mut y = style.bar_height;
    let mut prefix: Vec<usize> = Vec::new();
    for (depth, level) in levels.iter().enumerate() {
        let Some(&tag) = level_tags.get(depth) else {
            break;
        };
        out.push(build_popup(
            bar_tag,
            tag,
            x,
            y,
            &prefix,
            level.items,
            level.active,
            theme,
            style,
        ));
        // The child popup (if any) cascades off this level's open row.
        let Some(a) = level.active.filter(|_| depth + 1 < levels.len()) else {
            break;
        };
        x = x.saturating_add(style.dropdown_width);
        let a_u32 = u32::try_from(a).unwrap_or(u32::MAX);
        y = y.saturating_add(style.dropdown_v_padding + a_u32.saturating_mul(style.item_height));
        prefix.push(a);
    }
    out
}

/// The width of the outline a menu popup strokes inside its own box.
///
/// Named because three things have to agree about it: the border, the padding
/// that reserves it, and the size that grows by it. R1674 — before this round
/// the literal `1` appeared in the border alone, in **two** builders, and
/// neither of the other two knew it existed.
const DROPDOWN_BORDER_PX: u32 = 1;

/// The box a menu popup paints in: a bordered, elevated surface whose declared
/// `content` size is the room its ROWS get, with the outline added around them.
///
/// ★★ R1674 — one rule, because there were two copies of it. `build_popup` and
/// [`view_context_menu`] each assembled this style and this layout by hand, and
/// each stretched its rows across the popup's full width over its own 1px
/// outline. The crate's frame gate found it in the first, and the second was
/// only revealed when fixing the first made a pinned size disagree — which is
/// the argument for the extraction rather than for two edits.
///
/// The border is added to the box rather than taken out of the rows: a menu row
/// whose label got a pixel shorter because the popup gained an outline would be
/// the outline deciding the type's metrics.
fn popup_box(
    rows: Vec<Scene>,
    tag: &'static str,
    at: (u32, u32),
    content: (u32, u32),
    theme: &Theme,
    style: &MenuStyle,
) -> ContainerNode {
    let frame = DROPDOWN_BORDER_PX;
    ContainerNode::new(rows)
        .with_tag(tag)
        .with_style(
            BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh))
                .with_corner_radius(style.dropdown_radius)
                .with_border(Border::new(theme.resolve(ColorRole::Outline), frame))
                .with_shadows(crate::elevation::elevation(style.elevation)),
        )
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Stretch)
                .with_justify(JustifyContent::Start)
                .with_absolute_position(at.0, at.1)
                .with_size(Size::px(content.0 + frame * 2, content.1 + frame * 2))
                .with_padding(Rect::new(
                    frame,
                    style.dropdown_v_padding + frame,
                    frame,
                    style.dropdown_v_padding + frame,
                )),
        )
}

/// R985 — shared elevated-popup composition for one menu level at an explicit
/// window-space `(x, y)`, item rows tagged with the `rel_prefix` descent.
/// The R691 menubar dropdown and each R985 nested popup are this same surface.
#[expect(
    clippy::too_many_arguments,
    reason = "popup geometry + tags + theme are all distinct inputs"
)]
fn build_popup(
    bar_tag: &str,
    popup_tag: &'static str,
    x: u32,
    y: u32,
    rel_prefix: &[usize],
    items: &[MenuItemView],
    active: Option<usize>,
    theme: &Theme,
    style: &MenuStyle,
) -> Scene {
    // R805 — reserve the leading check-gutter for *every* row when the
    // dropdown holds any checkbox, so checkmarks and command labels align.
    let reserve_gutter = items.iter().any(|it| it.checkmark.is_some());
    let mut rows: Vec<Scene> = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let mut rel = rel_prefix.to_vec();
        rel.push(i);
        rows.push(build_item(
            bar_tag,
            &rel,
            item,
            active == Some(i),
            reserve_gutter,
            theme,
            style,
        ));
    }
    let height = style.dropdown_height(u32::try_from(items.len()).unwrap_or(u32::MAX));
    Scene::Container(popup_box(
        rows,
        popup_tag,
        (x, y),
        (style.dropdown_width, height),
        theme,
        style,
    ))
}

/// Window-space placement for a floating [`view_context_menu`] popup: the
/// cursor `anchor` to open at and the `window` size to clamp the panel
/// within. Bundled so the paint fn stays under the argument-count limit
/// (the two are always supplied together — the geometry the menubar
/// dropdown derives implicitly from its title is explicit for a context
/// menu).
#[derive(Clone, Copy, Debug)]
pub struct ContextMenuPlacement {
    /// Window-space cursor anchor `(x, y)` to open the popup at.
    pub anchor: (f32, f32),
    /// Window size `(w, h)`; the panel is clamped to stay inside it.
    pub window: (u32, u32),
}

/// R772 §5.53 §5.38 — floating command popup for a [`ContextMenu`], the
/// own-renderer right-click menu (R771.1: pinion draws its own menu on
/// every platform). Reuses the exact dropdown row + elevated-surface
/// paint as [`view_menu_dropdown`]; the *only* difference is the anchor
/// — a context menu floats at an arbitrary cursor anchor (window-space
/// logical pixels) instead of flush under a menubar title, clamped so
/// the panel stays inside the window (both carried by `placement`).
///
/// # Arguments
///
/// - `tag` — the [`ContextMenuExternal`] scope tag; item rows are tagged
///   [`composite_item_tag`] `{tag}#i{i}` so left-clicks route to it (the
///   same composite-tag router the menubar uses), and the R715 dismiss
///   barrier is tagged `{tag}#barrier`.
/// - `panel_tag` — the popup container's own tag (a11y `Menu` node +
///   snapshot anchor).
/// - `items` — command labels; index `i` becomes `{tag}#i{i}`.
/// - `active` — the highlighted item, painted with the M3 state-layer.
/// - `placement` — the cursor anchor + window bounds (see
///   [`ContextMenuPlacement`]).
/// - `theme` / `style` — palette + [`MenuStyle`] carrier.
///
/// # Returns
///
/// An absolutely-positioned [`Scene::Container`] tagged `panel_tag`.
/// Place this **last** in the root child list (after a [`dismiss_barrier`]
/// over the whole window) so it paints over everything below it.
///
/// [`ContextMenu`]: pinion_core::widgets::context_menu::ContextMenu
/// [`ContextMenuExternal`]: pinion_core::widgets::context_menu::ContextMenuExternal
/// [`dismiss_barrier`]: crate::barrier::dismiss_barrier
#[must_use]
pub fn view_context_menu(
    tag: &str,
    panel_tag: &'static str,
    items: &[&str],
    active: Option<usize>,
    placement: ContextMenuPlacement,
    theme: &Theme,
    style: &MenuStyle,
) -> Scene {
    let mut rows: Vec<Scene> = Vec::with_capacity(items.len());
    // A context menu carries plain command rows (its stateful-item axis is
    // a declared-defer until a consumer needs it), so no gutter is reserved.
    for (i, label) in items.iter().enumerate() {
        let item = MenuItemView::command(label);
        rows.push(build_item(
            tag,
            &[i],
            &item,
            active == Some(i),
            false,
            theme,
            style,
        ));
    }
    let width = style.dropdown_width;
    let height = style.dropdown_height(u32::try_from(items.len()).unwrap_or(u32::MAX));
    // Clamp so the panel's far edge never leaves the window (a right-click
    // near the right/bottom edge slides the popup back on-screen). R1674 — the
    // outline is part of what has to fit, so the clamp is against the OUTER
    // size: clamping against the content and then growing the box would slide
    // the frame back off the edge the clamp exists to keep it on.
    let frame = DROPDOWN_BORDER_PX * 2;
    let x = anchor_px(
        placement.anchor.0,
        placement.window.0.saturating_sub(width + frame),
    );
    let y = anchor_px(
        placement.anchor.1,
        placement.window.1.saturating_sub(height + frame),
    );
    Scene::Container(
        popup_box(rows, panel_tag, (x, y), (width, height), theme, style).with_layout(
            // (R1020 §5.39) Carry the binding's focus-stop opt-in onto the
            // tag-carrying panel so the scene-derived enumeration collects the
            // open context-menu's tag as a Tab stop. The rest of the layout is
            // `popup_box`'s, read back and extended rather than rebuilt — a
            // second hand-written copy is what this round is removing.
            popup_box(Vec::new(), panel_tag, (x, y), (width, height), theme, style)
                .layout
                .with_focusable(style.focusable),
        ),
    )
}

/// Narrow a window-space logical-pixel coordinate to `[0, max]` as `u32`:
/// the [`coord::saturating_f32_to_u32`](crate::coord::saturating_f32_to_u32)
/// seam (negative / non-finite → 0, overflow → `u32::MAX`) with an extra
/// `max` ceiling so a popup anchor never escapes the viewport. R958.1 — was
/// a re-rolled copy of the cast; now delegates to the one home.
fn anchor_px(v: f32, max: u32) -> u32 {
    crate::coord::saturating_f32_to_u32(v).min(max)
}

/// another declarative toolkit one dropdown row (R805). A [`MenuItemView::separator`] paints a thin
/// divider; otherwise a left-aligned label, optionally preceded by a
/// check-gutter (`reserve_gutter`) carrying the [`CHECK_GLYPH`] when the row is a checked checkbox,
/// and (R985) followed by a trailing [`SUBMENU_CHEVRON`] when the row opens a submenu. The
/// label is muted + state-layer-free when disabled; an enabled active
/// descendant carries the M3 state-layer. `rel_path` is the descent within the open
/// dropdown — the row's composite tag is [`composite_item_tag`]`(bar_tag, rel_path)`.
fn build_item(
    bar_tag: &str,
    rel_path: &[usize],
    item: &MenuItemView,
    is_active: bool,
    reserve_gutter: bool,
    theme: &Theme,
    style: &MenuStyle,
) -> Scene {
    if item.separator {
        return build_separator(bar_tag, rel_path, theme, style);
    }
    let fill = if is_active {
        theme
            .resolve(ColorRole::SurfaceContainerHigh)
            .lerp(theme.resolve(ColorRole::OnSurface), ACTIVE_ITEM_STATE_LAYER)
    } else {
        Color::TRANSPARENT
    };
    let fg = if item.enabled {
        theme.resolve(ColorRole::OnSurface)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };
    let mut children: Vec<Scene> = Vec::new();
    if reserve_gutter {
        // The check-gutter: the glyph for a checked checkbox, else an empty
        // fixed-width spacer so every label aligns past the gutter.
        let glyph = if item.checkmark == Some(true) {
            CHECK_GLYPH
        } else {
            ""
        };
        children.push(Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::styled(
                glyph,
                Rect::default(),
                TextStyle::new()
                    .with_size_px(style.item_font_px)
                    .with_fg(fg),
            ))])
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Start)
                    .with_size(Size::px(style.check_gutter_width, style.item_height)),
            ),
        ));
    }
    // R1543 §5.39 — a dropdown item's own mnemonic (`"&New"`). It exists only while
    // the dropdown is painted, which is exactly AccessKit's stated `access_key`
    // semantics ("for menu items, active only while the menu is active") and
    // the toolkit's — and here it falls out of the registry being derived from
    // the paint scene rather than from a registration that would have to be
    // added and removed as menus open and close.
    children.push(Scene::Text(TextNode::mnemonic_styled(
        item.label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.item_font_px)
            .with_fg(fg),
    )));
    if item.submenu {
        // R985 — a flex-grow spacer pushes the chevron to the trailing edge.
        children.push(Scene::Container(
            ContainerNode::new(Vec::new()).with_layout(LayoutStyle::new().with_flex_grow(1.0)),
        ));
        children.push(Scene::Text(TextNode::styled(
            SUBMENU_CHEVRON,
            Rect::default(),
            TextStyle::new()
                .with_size_px(style.item_font_px)
                .with_fg(fg),
        )));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(composite_item_path_tag(bar_tag, rel_path))
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Start)
                    .with_size(Size::auto().with_height(SizeValue::Px(style.item_height)))
                    .with_padding(Rect::new(style.item_padding, 0, style.item_padding, 0)),
            ),
    )
}

/// R805 — a separator row: a thin [`ColorRole::Outline`] rule centred in an
/// item-height row, inset by the item padding so it floats inside the
/// dropdown. Tagged like any item so the index addressing stays 1:1 (an
/// outside click on it is inert — the model never navigates here).
fn build_separator(bar_tag: &str, rel_path: &[usize], theme: &Theme, style: &MenuStyle) -> Scene {
    let rule = Scene::Container(
        ContainerNode::new(Vec::new())
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Outline)))
            .with_layout(
                LayoutStyle::new()
                    .with_flex_grow(1.0)
                    .with_size(Size::auto().with_height(SizeValue::Px(SEPARATOR_RULE_HEIGHT))),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![rule])
            .with_tag(composite_item_path_tag(bar_tag, rel_path))
            .with_style(BoxStyle::filled(Color::TRANSPARENT))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::auto().with_height(SizeValue::Px(style.item_height)))
                    .with_padding(Rect::new(style.item_padding, 0, style.item_padding, 0)),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::light()
    }

    #[test]
    fn r988_1_parse_item_sub_tag_inverts_composite_item_tag() {
        // The decoder is the inverse of the `i<index>` sub-tag the encoder emits.
        for index in [0_usize, 1, 7, 42] {
            let tag = composite_item_tag("ctx", index);
            let (_bar, sub) = tag.split_once('#').expect("composite tag carries a '#'");
            assert_eq!(
                parse_item_sub_tag(sub),
                Some(index),
                "round-trips item {index}"
            );
        }
        // Rejects a non-item sub-tag, a non-numeric index, and the empty string.
        assert_eq!(parse_item_sub_tag("barrier"), None, "non-item sub-tag");
        assert_eq!(parse_item_sub_tag("ix"), None, "non-numeric index");
        assert_eq!(parse_item_sub_tag(""), None, "empty sub-tag");
    }

    /// ★ R1674 — why a popup's box is two pixels bigger than its declared
    /// metrics on each axis.
    ///
    /// The declared `dropdown_width` / `dropdown_height` are what the ITEMS
    /// get; the popup is that plus the outline it strokes around them. Before
    /// this round the box was exactly `dropdown_width` and the rows stretched
    /// across all of it, painting over the border down both sides — measured by
    /// this crate's frame gate on its first run, one pixel each side, in every
    /// dropdown in the tree.
    ///
    /// Growing the box rather than squeezing the rows is the direction the
    /// floor takes too (a menu's size hint there is its contents PLUS its
    /// frame), and the alternative would let an outline decide a menu row's
    /// text metrics.
    const WHY_THE_BOX_GREW: &str = "a popup's declared metrics are its items' room; the outline is added \
         to the box rather than taken out of the rows";

    const TITLES: [&str; 3] = ["File", "Edit", "View"];
    const ITEMS: [&str; 3] = ["New", "Open", "Save"];
    /// The R691 dropdown tests drive all-command rows; R805 changed
    /// `view_menu_dropdown` to take [`MenuItemView`]s.
    const CMD_ITEMS: [MenuItemView; 3] = [
        MenuItemView::command("New"),
        MenuItemView::command("Open"),
        MenuItemView::command("Save"),
    ];

    fn collect_tags(scene: &Scene) -> Vec<String> {
        fn walk(scene: &Scene, out: &mut Vec<String>) {
            if let Scene::Container(c) = scene {
                if let Some(tag) = &c.tag {
                    out.push(tag.to_string());
                }
                for child in &c.children {
                    walk(child, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(scene, &mut out);
        out
    }

    fn all_text(scene: &Scene) -> Vec<String> {
        fn walk(scene: &Scene, out: &mut Vec<String>) {
            match scene {
                Scene::Text(t) => out.push(t.content.clone()),
                Scene::Container(c) => {
                    for child in &c.children {
                        walk(child, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(scene, &mut out);
        out
    }

    /// Background fill of a container tagged `tag`, or `None`.
    fn tag_fill(scene: &Scene, tag: &str) -> Option<Color> {
        if let Scene::Container(c) = scene {
            if c.tag.as_deref() == Some(tag) {
                return Some(c.style.fill);
            }
            for child in &c.children {
                if let Some(f) = tag_fill(child, tag) {
                    return Some(f);
                }
            }
        }
        None
    }

    #[test]
    fn r691_menu_style_m3_default_constants() {
        let s = MenuStyle::m3_default();
        assert_eq!(s.bar_height, 40);
        assert_eq!(s.title_slot_width, 96);
        assert_eq!(s.title_font_px, 14);
        assert_eq!(s.item_height, 36);
        assert_eq!(s.item_font_px, 14);
        assert_eq!(s.item_padding, 16);
        assert_eq!(s.check_gutter_width, 24);
        assert_eq!(s.dropdown_width, 200);
        assert_eq!(s.dropdown_v_padding, 8);
        assert_eq!(s.dropdown_radius, 4);
    }

    #[test]
    fn r691_dropdown_height_formula() {
        let s = MenuStyle::m3_default();
        // 8*2 + 3*36 = 124
        assert_eq!(s.dropdown_height(3), 124);
        assert_eq!(s.dropdown_height(0), 16);
    }

    #[test]
    fn r691_composite_tag_helpers() {
        assert_eq!(composite_title_tag("menu", 1), "menu#t1");
        assert_eq!(composite_item_tag("menu", 2), "menu#i2");
        // R985 — the path form joins a descent with dots; a single-element
        // path matches the flat form.
        assert_eq!(composite_item_path_tag("menu", &[2]), "menu#i2");
        assert_eq!(composite_item_path_tag("menu", &[2, 0]), "menu#i2.0");
    }

    #[test]
    fn r691_menu_bar_tags_and_labels() {
        let scene = view_menu_bar("menu", &TITLES, None, &theme(), &MenuStyle::m3_default());
        assert_eq!(
            collect_tags(&scene),
            vec![
                "menu".to_string(),
                "menu#t0".to_string(),
                "menu#t1".to_string(),
                "menu#t2".to_string(),
            ]
        );
        assert_eq!(all_text(&scene), vec!["File", "Edit", "View"]);
    }

    #[test]
    fn r691_open_title_filled_others_transparent() {
        let t = theme();
        let scene = view_menu_bar("menu", &TITLES, Some(1), &t, &MenuStyle::m3_default());
        assert_eq!(tag_fill(&scene, "menu#t0"), Some(Color::TRANSPARENT));
        assert_eq!(
            tag_fill(&scene, "menu#t1"),
            Some(t.resolve(ColorRole::SurfaceContainerHigh))
        );
        assert_eq!(tag_fill(&scene, "menu#t2"), Some(Color::TRANSPARENT));
    }

    #[test]
    fn r691_no_open_title_all_transparent() {
        let scene = view_menu_bar("menu", &TITLES, None, &theme(), &MenuStyle::m3_default());
        for m in 0..3 {
            assert_eq!(
                tag_fill(&scene, &format!("menu#t{m}")),
                Some(Color::TRANSPARENT)
            );
        }
    }

    #[test]
    fn r691_dropdown_tags_and_labels() {
        let scene = view_menu_dropdown(
            "menu",
            "menu_dropdown",
            0,
            &CMD_ITEMS,
            None,
            &theme(),
            &MenuStyle::m3_default(),
        );
        assert_eq!(
            collect_tags(&scene),
            vec![
                "menu_dropdown".to_string(),
                "menu#i0".to_string(),
                "menu#i1".to_string(),
                "menu#i2".to_string(),
            ]
        );
        assert_eq!(all_text(&scene), vec!["New", "Open", "Save"]);
    }

    #[test]
    fn r711_dropdown_carries_md3_l2_elevation() {
        let scene = view_menu_dropdown(
            "menu",
            "menu_dropdown",
            0,
            &CMD_ITEMS,
            None,
            &theme(),
            &MenuStyle::m3_default(),
        );
        let Scene::Container(dropdown) = &scene else {
            panic!("dropdown root is a container");
        };
        assert_eq!(
            dropdown.style.shadows,
            crate::elevation::elevation(crate::elevation::MENU_LEVEL),
            "dropdown carries the shared MD3 L2 elevation shadow",
        );
    }

    #[test]
    fn r691_active_item_state_layered_others_transparent() {
        let t = theme();
        let scene = view_menu_dropdown(
            "menu",
            "menu_dropdown",
            0,
            &CMD_ITEMS,
            Some(2),
            &t,
            &MenuStyle::m3_default(),
        );
        assert_eq!(tag_fill(&scene, "menu#i0"), Some(Color::TRANSPARENT));
        assert_eq!(tag_fill(&scene, "menu#i1"), Some(Color::TRANSPARENT));
        let expected = t
            .resolve(ColorRole::SurfaceContainerHigh)
            .lerp(t.resolve(ColorRole::OnSurface), ACTIVE_ITEM_STATE_LAYER);
        assert_eq!(tag_fill(&scene, "menu#i2"), Some(expected));
    }

    #[test]
    fn r691_dropdown_absolute_position_anchors_under_open_title() {
        let s = MenuStyle::m3_default();
        let scene = view_menu_dropdown("menu", "menu_dropdown", 2, &CMD_ITEMS, None, &theme(), &s);
        let Scene::Container(c) = &scene else {
            panic!("expected dropdown Container");
        };
        // menu_index 2 -> x = 2 * title_slot_width; y = bar_height.
        assert_eq!(
            c.layout.absolute_position,
            Some((2 * s.title_slot_width, s.bar_height))
        );
        assert_eq!(
            c.layout.size.width,
            SizeValue::Px(s.dropdown_width + DROPDOWN_BORDER_PX * 2),
            "{WHY_THE_BOX_GREW}"
        );
        assert_eq!(
            c.layout.size.height,
            SizeValue::Px(s.dropdown_height(3) + DROPDOWN_BORDER_PX * 2)
        );
    }

    fn placement(anchor: (f32, f32), window: (u32, u32)) -> ContextMenuPlacement {
        ContextMenuPlacement { anchor, window }
    }

    #[test]
    fn r772_context_menu_tags_items_and_panel() {
        let s = MenuStyle::m3_default();
        let scene = view_context_menu(
            "ctx",
            "ctx_panel",
            &ITEMS,
            None,
            placement((100.0, 80.0), (800, 600)),
            &theme(),
            &s,
        );
        let tags = collect_tags(&scene);
        assert!(tags.contains(&"ctx_panel".to_string()), "panel tag present");
        assert!(
            tags.contains(&"ctx#i0".to_string()),
            "items route to the ctx scope"
        );
        assert!(tags.contains(&"ctx#i2".to_string()));
        assert_eq!(all_text(&scene), vec!["New", "Open", "Save"]);
    }

    #[test]
    fn r772_context_menu_anchors_at_cursor_in_bounds() {
        let s = MenuStyle::m3_default();
        let scene = view_context_menu(
            "ctx",
            "ctx_panel",
            &ITEMS,
            None,
            placement((100.0, 80.0), (800, 600)),
            &theme(),
            &s,
        );
        let Scene::Container(c) = &scene else {
            panic!("expected panel Container");
        };
        assert_eq!(c.layout.absolute_position, Some((100, 80)));
        assert_eq!(
            c.layout.size.width,
            SizeValue::Px(s.dropdown_width + DROPDOWN_BORDER_PX * 2),
            "{WHY_THE_BOX_GREW}"
        );
        assert_eq!(
            c.layout.size.height,
            SizeValue::Px(s.dropdown_height(3) + DROPDOWN_BORDER_PX * 2)
        );
    }

    #[test]
    fn r772_context_menu_clamps_to_window_edges() {
        let s = MenuStyle::m3_default();
        let win = (800, 600);
        let scene = view_context_menu(
            "ctx",
            "ctx_panel",
            &ITEMS,
            None,
            placement((790.0, 595.0), win),
            &theme(),
            &s,
        );
        let Scene::Container(c) = &scene else {
            panic!("expected panel Container");
        };
        // ★ R1674 — against the OUTER size. What has to stay on screen is the
        // panel, and the panel is its rows plus the outline around them; a
        // clamp computed from the content alone slides the frame back off the
        // very edge the clamp exists to keep it on. Two pixels, and invisible
        // to every assertion here until the frame became part of the box.
        let outer = DROPDOWN_BORDER_PX * 2;
        let max_x = win.0 - (s.dropdown_width + outer);
        let max_y = win.1 - (s.dropdown_height(3) + outer);
        assert_eq!(
            c.layout.absolute_position,
            Some((max_x, max_y)),
            "panel slid back on-screen near the edges, frame included"
        );
    }

    #[test]
    fn r772_context_menu_active_item_state_layered() {
        let t = theme();
        let s = MenuStyle::m3_default();
        let scene = view_context_menu(
            "ctx",
            "ctx_panel",
            &ITEMS,
            Some(2),
            placement((0.0, 0.0), (800, 600)),
            &t,
            &s,
        );
        let expected = t
            .resolve(ColorRole::SurfaceContainerHigh)
            .lerp(t.resolve(ColorRole::OnSurface), ACTIVE_ITEM_STATE_LAYER);
        assert_eq!(tag_fill(&scene, "ctx#i2"), Some(expected));
        assert_eq!(tag_fill(&scene, "ctx#i0"), Some(Color::TRANSPARENT));
    }

    // ----- R805: stateful item paint -----

    /// Find the container tagged `tag` anywhere in `scene`.
    fn find_container<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        if let Scene::Container(c) = scene {
            if c.tag.as_deref() == Some(tag) {
                return Some(c);
            }
            for child in &c.children {
                if let Some(found) = find_container(child, tag) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// The `fg_color` of the last [`TextNode`] under `tag` (the label —
    /// past any empty gutter spacer).
    fn label_fg(scene: &Scene, tag: &str) -> Option<Color> {
        fn last_text(scene: &Scene, out: &mut Option<Color>) {
            match scene {
                Scene::Text(t) => *out = Some(t.style.fg_color),
                Scene::Container(c) => {
                    for child in &c.children {
                        last_text(child, out);
                    }
                }
                _ => {}
            }
        }
        let c = find_container(scene, tag)?;
        let mut out = None;
        for child in &c.children {
            last_text(child, &mut out);
        }
        out
    }

    fn rich_items() -> [MenuItemView<'static>; 4] {
        [
            MenuItemView::command("Undo"),
            MenuItemView::checkbox("Show Grid", true),
            MenuItemView::separator(),
            MenuItemView::command("Redo").disabled(),
        ]
    }

    #[test]
    fn r805_checked_checkbox_paints_glyph_unchecked_empty() {
        let items = [
            MenuItemView::checkbox("On", true),
            MenuItemView::checkbox("Off", false),
        ];
        let scene = view_menu_dropdown(
            "menu",
            "menu_dropdown",
            0,
            &items,
            None,
            &theme(),
            &MenuStyle::m3_default(),
        );
        let texts = all_text(&scene);
        assert_eq!(
            texts.iter().filter(|t| *t == CHECK_GLYPH).count(),
            1,
            "exactly the checked checkbox paints the check glyph"
        );
        assert!(texts.contains(&"On".to_string()) && texts.contains(&"Off".to_string()));
    }

    #[test]
    fn r805_gutter_reserved_only_when_a_checkbox_is_present() {
        let style = MenuStyle::m3_default();
        // All-command dropdown: no gutter, each row holds just its label.
        let cmds = [MenuItemView::command("A"), MenuItemView::command("B")];
        let plain = view_menu_dropdown("menu", "menu_dropdown", 0, &cmds, None, &theme(), &style);
        assert_eq!(find_container(&plain, "menu#i0").unwrap().children.len(), 1);
        // With a checkbox present, every row (incl. commands) gets a gutter.
        let mixed = view_menu_dropdown(
            "menu",
            "menu_dropdown",
            0,
            &rich_items(),
            None,
            &theme(),
            &style,
        );
        assert_eq!(
            find_container(&mixed, "menu#i0").unwrap().children.len(),
            2,
            "command row gains an empty gutter so labels align with checkmarks"
        );
    }

    #[test]
    fn r805_separator_paints_outline_rule() {
        let t = theme();
        let scene = view_menu_dropdown(
            "menu",
            "menu_dropdown",
            0,
            &rich_items(),
            None,
            &t,
            &MenuStyle::m3_default(),
        );
        let sep = find_container(&scene, "menu#i2").expect("separator row tagged like an item");
        assert_eq!(
            sep.style.fill,
            Color::TRANSPARENT,
            "separator row is transparent"
        );
        assert_eq!(sep.children.len(), 1, "separator holds a single rule child");
        let Scene::Container(rule) = &sep.children[0] else {
            panic!("separator child is the rule container");
        };
        assert_eq!(
            rule.style.fill,
            t.resolve(ColorRole::Outline),
            "rule is the Outline divider"
        );
    }

    #[test]
    fn r805_disabled_item_label_is_muted() {
        let t = theme();
        let scene = view_menu_dropdown(
            "menu",
            "menu_dropdown",
            0,
            &rich_items(),
            None,
            &t,
            &MenuStyle::m3_default(),
        );
        // Item 3 ("Redo") is disabled -> muted; item 0 ("Undo") enabled.
        assert_eq!(
            label_fg(&scene, "menu#i3"),
            Some(t.resolve(ColorRole::OnSurfaceMuted))
        );
        assert_eq!(
            label_fg(&scene, "menu#i0"),
            Some(t.resolve(ColorRole::OnSurface))
        );
    }

    // ----- R985: cascading submenu paint -----

    #[test]
    fn r985_submenu_row_paints_trailing_chevron() {
        let items = [
            MenuItemView::command("New"),
            MenuItemView::submenu("Open Recent"),
        ];
        let scene = view_menu_dropdown(
            "menu",
            "menu_dropdown",
            0,
            &items,
            None,
            &theme(),
            &MenuStyle::m3_default(),
        );
        let texts = all_text(&scene);
        assert!(texts.contains(&"Open Recent".to_string()));
        assert_eq!(
            texts.iter().filter(|t| *t == SUBMENU_CHEVRON).count(),
            1,
            "exactly the submenu row paints a chevron",
        );
        // The plain command row carries no chevron.
        let cmd = find_container(&scene, "menu#i0").unwrap();
        let mut cmd_texts = Vec::new();
        for c in &cmd.children {
            if let Scene::Text(tn) = c {
                cmd_texts.push(tn.content.clone());
            }
        }
        assert!(
            !cmd_texts.iter().any(|t| t == SUBMENU_CHEVRON),
            "command row has no chevron"
        );
    }

    #[test]
    fn r985_cascade_renders_one_popup_per_open_level() {
        let s = MenuStyle::m3_default();
        let top = [
            MenuItemView::command("New"),
            MenuItemView::submenu("Open Recent"),
        ];
        let sub = [
            MenuItemView::command("a.txt"),
            MenuItemView::command("b.txt"),
        ];
        let levels = [
            MenuLevel {
                items: &top,
                active: Some(1),
            }, // submenu 1 open
            MenuLevel {
                items: &sub,
                active: Some(0),
            },
        ];
        let popups = view_menu_cascade(
            "menu",
            &["menu_dropdown", "menu_sub0"],
            0,
            &levels,
            &theme(),
            &s,
        );
        assert_eq!(popups.len(), 2, "top dropdown + one nested popup");
        // Top dropdown anchored under its title.
        let Scene::Container(top_c) = &popups[0] else {
            panic!("top container")
        };
        assert_eq!(top_c.layout.absolute_position, Some((0, s.bar_height)));
        // Nested popup opens to the right of the parent, aligned to row 1.
        let Scene::Container(sub_c) = &popups[1] else {
            panic!("sub container")
        };
        let expect_x = s.dropdown_width;
        // Parent's open row is index 1 → y offset = v_padding + 1 * item_height.
        let expect_y = s.bar_height + s.dropdown_v_padding + s.item_height;
        assert_eq!(sub_c.layout.absolute_position, Some((expect_x, expect_y)));
    }

    #[test]
    fn r985_nested_item_tags_carry_descent_path() {
        let s = MenuStyle::m3_default();
        let top = [
            MenuItemView::command("New"),
            MenuItemView::submenu("Open Recent"),
        ];
        let sub = [
            MenuItemView::command("a.txt"),
            MenuItemView::command("b.txt"),
        ];
        let levels = [
            MenuLevel {
                items: &top,
                active: Some(1),
            },
            MenuLevel {
                items: &sub,
                active: None,
            },
        ];
        let popups = view_menu_cascade(
            "menu",
            &["menu_dropdown", "menu_sub0"],
            0,
            &levels,
            &theme(),
            &s,
        );
        let sub_scene = &popups[1];
        // Items of the nested popup carry the `1.<i>` descent in their tag.
        assert!(
            find_container(sub_scene, "menu#i1.0").is_some(),
            "submenu item 0 tag"
        );
        assert!(
            find_container(sub_scene, "menu#i1.1").is_some(),
            "submenu item 1 tag"
        );
    }

    #[test]
    fn r985_cascade_stops_when_no_child_open() {
        let s = MenuStyle::m3_default();
        let top = [MenuItemView::command("New"), MenuItemView::submenu("More")];
        // Only the top level is open (no active child to cascade from).
        let levels = [MenuLevel {
            items: &top,
            active: None,
        }];
        let popups = view_menu_cascade(
            "menu",
            &["menu_dropdown", "menu_sub0"],
            0,
            &levels,
            &theme(),
            &s,
        );
        assert_eq!(popups.len(), 1, "no nested popup without an open child");
    }

    /// ★★ R1674 — a menu bar's titles and a dropdown's items stay inside the
    /// outlines those surfaces stroke. The crate gate ([`crate::frame_gate`]).
    ///
    /// Both entry points, because the border in this module is on the POPUP
    /// (two sites), and the bar is what a consumer opens it from.
    #[test]
    fn r1674_a_menu_keeps_its_items_inside_its_popup() {
        crate::frame_gate::assert_frame_contained("menu bar open", &mut |_w, _h| {
            view_menu_bar("menu", &TITLES, Some(0), &theme(), &MenuStyle::m3_default())
        });
        // A dropdown is ANCHORED, not bound (R1672): it is absolutely positioned
        // and may extend past whatever is behind it, so the question here is
        // whether its ITEMS stay inside IT. Judging it against a 180px window
        // would only report that a 200px menu is wider than 180 pixels, which is
        // true and is not this gate's question.
        crate::frame_gate::assert_frame_contained_at(
            "menu dropdown",
            &[(420, 260), (320, 200)],
            &mut |_w, _h| {
                view_menu_dropdown(
                    "menu",
                    "menu_dropdown",
                    0,
                    &CMD_ITEMS,
                    Some(1),
                    &theme(),
                    &MenuStyle::m3_default(),
                )
            },
        );
    }
}
