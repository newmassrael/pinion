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
//! ## Future axes (per [[abstraction-needs-second-consumer]])
//!
//! - **Elevation shadow** under the dropdown (M3 menus carry a drop
//!   shadow). R691 reads the dropdown as elevated via a distinct
//!   surface tier ([`pinion_core::theme::ColorRole::SurfaceContainerHigh`])
//!   with an outline and corner radius; a shadow primitive lands when
//!   the paint pipeline grows one.
//! - **Leading icon / trailing accelerator label columns** per item.
//! - **Separators / section headers** within a dropdown.

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
            dropdown_width: 200,
            dropdown_v_padding: 8,
            dropdown_radius: 4,
            elevation: crate::elevation::MENU_LEVEL,
        }
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
/// (`{bar_tag}#i{index}`). Items route to the *same* `MenuBarExternal`
/// as the titles; the `i` prefix marks the sub-target as an item.
#[must_use]
pub fn composite_item_tag(bar_tag: &str, index: usize) -> String {
    format!("{bar_tag}#i{index}")
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
                    .with_size(Size::auto().with_height(SizeValue::Px(style.bar_height))),
            ),
    )
}

/// Compose one top-level title slot: a fixed-width centered label,
/// background-filled when its menu is open.
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
    let label = Scene::Text(TextNode::styled(
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
    items: &[&str],
    active: Option<usize>,
    theme: &Theme,
    style: &MenuStyle,
) -> Scene {
    let surface = theme.resolve(ColorRole::SurfaceContainerHigh);
    let mut rows: Vec<Scene> = Vec::with_capacity(items.len());
    for (i, label) in items.iter().enumerate() {
        rows.push(build_item(
            bar_tag,
            i,
            label,
            active == Some(i),
            surface,
            theme,
            style,
        ));
    }
    let x = u32::try_from(menu_index).unwrap_or(u32::MAX).saturating_mul(style.title_slot_width);
    let height = style.dropdown_height(u32::try_from(items.len()).unwrap_or(u32::MAX));
    Scene::Container(
        ContainerNode::new(rows)
            .with_tag(dropdown_tag)
            .with_style(
                BoxStyle::filled(surface)
                    .with_corner_radius(style.dropdown_radius)
                    .with_border(Border::new(theme.resolve(ColorRole::Outline), 1))
                    .with_shadows(crate::elevation::elevation(style.elevation)),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start)
                    .with_absolute_position(x, style.bar_height)
                    .with_size(Size::px(style.dropdown_width, height))
                    .with_padding(Rect::new(0, style.dropdown_v_padding, 0, style.dropdown_v_padding)),
            ),
    )
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
    let surface = theme.resolve(ColorRole::SurfaceContainerHigh);
    let mut rows: Vec<Scene> = Vec::with_capacity(items.len());
    for (i, label) in items.iter().enumerate() {
        rows.push(build_item(
            tag,
            i,
            label,
            active == Some(i),
            surface,
            theme,
            style,
        ));
    }
    let width = style.dropdown_width;
    let height = style.dropdown_height(u32::try_from(items.len()).unwrap_or(u32::MAX));
    // Clamp so the panel's far edge never leaves the window (a right-click
    // near the right/bottom edge slides the popup back on-screen).
    let x = anchor_px(placement.anchor.0, placement.window.0.saturating_sub(width));
    let y = anchor_px(placement.anchor.1, placement.window.1.saturating_sub(height));
    Scene::Container(
        ContainerNode::new(rows)
            .with_tag(panel_tag)
            .with_style(
                BoxStyle::filled(surface)
                    .with_corner_radius(style.dropdown_radius)
                    .with_border(Border::new(theme.resolve(ColorRole::Outline), 1))
                    .with_shadows(crate::elevation::elevation(style.elevation)),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start)
                    .with_absolute_position(x, y)
                    .with_size(Size::px(width, height))
                    .with_padding(Rect::new(0, style.dropdown_v_padding, 0, style.dropdown_v_padding)),
            ),
    )
}

/// Narrow a window-space logical-pixel coordinate to `[0, max]` as `u32`,
/// saturating non-finite / out-of-range inputs (the textbook f32 -> px
/// seam, mirroring `text_field::saturating_f32_to_u32`).
fn anchor_px(v: f32, max: u32) -> u32 {
    if !v.is_finite() || v <= 0.0 {
        return 0;
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "max -> f32 rounds to a single saturating ceiling for the compare"
    )]
    let ceiling = max as f32;
    if v >= ceiling {
        return max;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "v is finite and within (0, max) here, so the floor fits u32"
    )]
    let px = v as u32;
    px
}

/// Compose one dropdown command row: a left-aligned label, background
/// state-layered when it is the active descendant.
fn build_item(
    bar_tag: &str,
    index: usize,
    label: &str,
    is_active: bool,
    surface: Color,
    theme: &Theme,
    style: &MenuStyle,
) -> Scene {
    let fill = if is_active {
        surface.lerp(theme.resolve(ColorRole::OnSurface), ACTIVE_ITEM_STATE_LAYER)
    } else {
        Color::TRANSPARENT
    };
    let label_node = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.item_font_px)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label_node])
            .with_tag(composite_item_tag(bar_tag, index))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::light()
    }

    const TITLES: [&str; 3] = ["File", "Edit", "View"];
    const ITEMS: [&str; 3] = ["New", "Open", "Save"];

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
            &ITEMS,
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
            &ITEMS,
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
            &ITEMS,
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
        let scene = view_menu_dropdown("menu", "menu_dropdown", 2, &ITEMS, None, &theme(), &s);
        let Scene::Container(c) = &scene else {
            panic!("expected dropdown Container");
        };
        // menu_index 2 -> x = 2 * title_slot_width; y = bar_height.
        assert_eq!(
            c.layout.absolute_position,
            Some((2 * s.title_slot_width, s.bar_height))
        );
        assert_eq!(c.layout.size.width, SizeValue::Px(s.dropdown_width));
        assert_eq!(c.layout.size.height, SizeValue::Px(s.dropdown_height(3)));
    }

    fn placement(anchor: (f32, f32), window: (u32, u32)) -> ContextMenuPlacement {
        ContextMenuPlacement { anchor, window }
    }

    #[test]
    fn r772_context_menu_tags_items_and_panel() {
        let s = MenuStyle::m3_default();
        let scene = view_context_menu(
            "ctx", "ctx_panel", &ITEMS, None, placement((100.0, 80.0), (800, 600)), &theme(), &s,
        );
        let tags = collect_tags(&scene);
        assert!(tags.contains(&"ctx_panel".to_string()), "panel tag present");
        assert!(tags.contains(&"ctx#i0".to_string()), "items route to the ctx scope");
        assert!(tags.contains(&"ctx#i2".to_string()));
        assert_eq!(all_text(&scene), vec!["New", "Open", "Save"]);
    }

    #[test]
    fn r772_context_menu_anchors_at_cursor_in_bounds() {
        let s = MenuStyle::m3_default();
        let scene = view_context_menu(
            "ctx", "ctx_panel", &ITEMS, None, placement((100.0, 80.0), (800, 600)), &theme(), &s,
        );
        let Scene::Container(c) = &scene else {
            panic!("expected panel Container");
        };
        assert_eq!(c.layout.absolute_position, Some((100, 80)));
        assert_eq!(c.layout.size.width, SizeValue::Px(s.dropdown_width));
        assert_eq!(c.layout.size.height, SizeValue::Px(s.dropdown_height(3)));
    }

    #[test]
    fn r772_context_menu_clamps_to_window_edges() {
        let s = MenuStyle::m3_default();
        let win = (800, 600);
        let scene = view_context_menu(
            "ctx", "ctx_panel", &ITEMS, None, placement((790.0, 595.0), win), &theme(), &s,
        );
        let Scene::Container(c) = &scene else {
            panic!("expected panel Container");
        };
        let max_x = win.0 - s.dropdown_width;
        let max_y = win.1 - s.dropdown_height(3);
        assert_eq!(
            c.layout.absolute_position,
            Some((max_x, max_y)),
            "panel slid back on-screen near the edges"
        );
    }

    #[test]
    fn r772_context_menu_active_item_state_layered() {
        let t = theme();
        let s = MenuStyle::m3_default();
        let scene = view_context_menu(
            "ctx", "ctx_panel", &ITEMS, Some(2), placement((0.0, 0.0), (800, 600)), &t, &s,
        );
        let expected = t
            .resolve(ColorRole::SurfaceContainerHigh)
            .lerp(t.resolve(ColorRole::OnSurface), ACTIVE_ITEM_STATE_LAYER);
        assert_eq!(tag_fill(&scene, "ctx#i2"), Some(expected));
        assert_eq!(tag_fill(&scene, "ctx#i0"), Some(Color::TRANSPARENT));
    }
}
