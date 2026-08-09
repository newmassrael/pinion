//! R702 §5.16 §5.40 §5.50 — backend-agnostic modal navigation
//! `Drawer` (side sheet) paint composition.
//!
//! Phase B widget-catalog entry — the edge-anchored navigation sheet
//! every pro DCC / IDE / CAD tool ships (a left nav rail, a right
//! inspector / properties panel), and a direct step toward the
//! northern-star "engine-class editor self-hosted in pinion".
//!
//! ## Relationship to [`crate::dialog`]
//!
//! A modal drawer is the **2nd consumer** of the R693 modal substrate:
//! the same full-window scrim + `pinion_runtime::FocusManager`
//! focus-trap (`push_modal_scope` / `pop_modal_scope`, driven through
//! [`pinion_core::modal_scope_request`]) the [`crate::dialog`] uses.
//! The only paint difference is geometry: where [`crate::dialog`]
//! *centres* an elevated panel, [`view_drawer`] anchors a full-height
//! panel to one window edge ([`DrawerEdge`]). The behaviour (open/close
//! lifecycle, Tab trap, Escape-to-dismiss) is identical and lives in
//! the binding + the shared modal substrate, not here.
//!
//! ## Light-dismiss (the carry this round clears)
//!
//! R693 deferred backdrop-click light-dismiss as an opt-in awaiting a
//! consumer ("register the scrim tag as an `External`"). A modal
//! navigation drawer is exactly that consumer: Material's modal nav
//! drawer dismisses on scrim tap. So the binding registers the
//! `scrim_tag` as a real (click-only) `External` — a backdrop click
//! emits its `"<scrim_tag>.click"` intent and the reducer closes the
//! drawer. This module stays chrome-only; whether the scrim is inert
//! (dialog) or dismiss-capable (drawer) is decided by the binding
//! choosing to bind an `External` to `scrim_tag` or not.
//!
//! ## Chrome, not behaviour
//!
//! Like [`crate::dialog`], this owns only the *paint* axis: a
//! full-window scrim with an edge-anchored panel holding pre-rendered
//! navigation-item scenes (real focusable
//! [`pinion_core::external::External`] widgets the binding composes via
//! [`crate::button::view_button`]); [`view_drawer`] only lays them out
//! in a leading column. Keeping them real externals is what makes the
//! Tab trap visible — each item paints its own focus ring.

use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::{Color, Scene};

/// R702 §5.50 — which window edge the drawer panel anchors to. A left
/// rail (navigation) and a right sheet (inspector / properties) are the
/// two DCC / IDE idioms; both share the scrim + trap, differing only in
/// the scrim's cross-axis justification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawerEdge {
    /// Panel hugs the left edge (navigation rail) — the default.
    Left,
    /// Panel hugs the right edge (inspector / properties panel).
    Right,
}

impl DrawerEdge {
    /// Cross-axis justification that pins the panel to this edge inside
    /// the full-window scrim row.
    const fn justify(self) -> JustifyContent {
        match self {
            DrawerEdge::Left => JustifyContent::Start,
            DrawerEdge::Right => JustifyContent::End,
        }
    }
}

/// R702 §5.50 — Material 3 modal navigation-drawer dimensions + scrim
/// weight. Mirrors the [`crate::dialog::DialogStyle`] carrier pattern so
/// the widget catalog presents a uniform `Style` surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawerStyle {
    /// Scrim (backdrop) opacity over black, 0–255. Defaults to the
    /// shared [`crate::scrim::M3_SCRIM_ALPHA`] so the dimmed background
    /// reads identically across every modal widget.
    pub scrim_alpha: u8,
    /// Panel width in logical pixels (M3 modal nav drawer ≈ 360 max;
    /// 280 is a comfortable navigation-rail default).
    pub panel_width: u32,
    /// Panel inner padding (each edge; M3 drawer ≈ 12 px around the
    /// item list).
    pub panel_padding: u32,
    /// Vertical gap between the optional title and the item list, and
    /// between adjacent navigation items.
    pub item_gap: u32,
    /// Title font size (M3 `title-small` ≈ 14 px header label).
    pub title_font_px: u32,
    /// Which window edge the panel anchors to ([`DrawerEdge`]). Folded
    /// into the style (rather than a [`view_drawer`] argument) so the
    /// fn stays within the 7-argument `view_*` convention; a left nav
    /// rail and a right inspector differ only in this field.
    pub edge: DrawerEdge,
    /// Material elevation level the panel casts its drop-shadow at
    /// (R711 §5.50; MD3 modal nav drawer = Level 1). `0` = flat.
    pub elevation: u8,
}

impl DrawerStyle {
    /// R702 §5.50 — Material 3 modal navigation-drawer defaults (left
    /// nav rail). See the struct docs for the per-field token anchors.
    #[must_use]
    pub const fn m3_default() -> Self {
        Self {
            scrim_alpha: crate::scrim::M3_SCRIM_ALPHA,
            panel_width: 280,
            panel_padding: 12,
            item_gap: 4,
            title_font_px: 14,
            edge: DrawerEdge::Left,
            elevation: crate::elevation::DRAWER_LEVEL,
        }
    }

    /// Anchor the panel to `edge` (e.g. [`DrawerEdge::Right`] for a
    /// right-hand inspector / properties sheet).
    #[must_use]
    pub const fn with_edge(mut self, edge: DrawerEdge) -> Self {
        self.edge = edge;
        self
    }

    /// The scrim fill: black at [`Self::scrim_alpha`]. Delegates to the
    /// shared [`crate::scrim::scrim_fill`] so the derivation has one home.
    #[must_use]
    pub const fn scrim_color(self) -> Color {
        crate::scrim::scrim_fill(self.scrim_alpha)
    }
}

impl Default for DrawerStyle {
    fn default() -> Self {
        Self::m3_default()
    }
}

/// R702 §5.16 §5.50 — compose a modal navigation-drawer overlay: a
/// full-window scrim with a full-height panel anchored to `edge`,
/// holding an optional title above a leading column of pre-rendered
/// navigation-item scenes.
///
/// # Arguments
///
/// - `scrim_tag` — the backdrop container tag. Unlike [`crate::dialog`]
///   (where it is inert), the binding binds an `External` to this tag so
///   a backdrop click light-dismisses (the router resolves background
///   clicks to it).
/// - `panel_tag` — the panel container tag (queryable via
///   `scene/snapshot`, §2 invariant #7).
/// - `title` — optional drawer header label; an empty `&str` omits the
///   title node.
/// - `items` — pre-rendered navigation-item scenes (real focusable
///   [`External`](pinion_core::external::External) widgets), laid out
///   top-to-bottom in a leading column.
/// - `viewport` — `(width, height)` window dimensions, so the scrim
///   fills the viewport and the panel spans its full height.
/// - `theme` / `style` — palette + [`DrawerStyle`] carrier (the carrier
///   holds the [`DrawerEdge`]).
///
/// # Returns
///
/// A [`Scene::Container`] tagged `scrim_tag`, absolutely positioned at
/// the origin and sized to the window, with the panel pinned to
/// `style.edge`. Place it **last** in the root child list so it paints
/// over (and hit-tests above) the rest of the scene.
#[must_use]
pub fn view_drawer(
    scrim_tag: &'static str,
    panel_tag: &'static str,
    title: &str,
    items: Vec<Scene>,
    viewport: (u32, u32),
    theme: &Theme,
    style: &DrawerStyle,
) -> Scene {
    let win_h = viewport.1;
    let mut panel_children: Vec<Scene> = Vec::with_capacity(items.len() + 1);
    if !title.is_empty() {
        panel_children.push(Scene::Text(TextNode::styled(
            title,
            Rect::default(),
            TextStyle::new()
                .with_size_px(style.title_font_px)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )));
    }
    panel_children.extend(items);

    let panel = Scene::Container(
        ContainerNode::new(panel_children)
            .with_tag(panel_tag)
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh))
                    .with_shadows(crate::elevation::elevation(style.elevation)),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start)
                    .with_gap(style.item_gap)
                    // Fixed width, full window height (the cross-axis
                    // stretch of the scrim row already spans height, but
                    // the explicit height keeps the panel full-bleed even
                    // if a future scrim layout drops `Stretch`).
                    .with_size(
                        Size::auto()
                            .with_width(SizeValue::Px(style.panel_width))
                            .with_height(SizeValue::Px(win_h)),
                    )
                    .with_padding(Rect::new(
                        style.panel_padding,
                        style.panel_padding,
                        style.panel_padding,
                        style.panel_padding,
                    )),
            ),
    );

    // R703 — shared scrim backdrop; the drawer pins its panel full-
    // height against `style.edge`.
    crate::scrim::scrim_backdrop(
        scrim_tag,
        style.scrim_color(),
        viewport,
        FlexDirection::Row,
        AlignItems::Stretch,
        style.edge.justify(),
        panel,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;
    use pinion_core::widgets::button::ButtonExternal;

    fn theme() -> Theme {
        Theme::light()
    }

    fn item(tag: &'static str) -> Scene {
        Scene::External(ExternalNode::new(Box::new(ButtonExternal::new())).with_tag(tag))
    }

    fn drawer(edge: DrawerEdge) -> Scene {
        view_drawer(
            "drawer_scrim",
            "drawer_panel",
            "Navigation",
            vec![item("drawer_item_0"), item("drawer_item_1")],
            (520, 360),
            &theme(),
            &DrawerStyle::m3_default().with_edge(edge),
        )
    }

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

    fn external_tags(scene: &Scene) -> Vec<String> {
        fn walk(scene: &Scene, out: &mut Vec<String>) {
            match scene {
                Scene::External(n) => {
                    if let Some(tag) = &n.tag {
                        out.push(tag.to_string());
                    }
                }
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

    #[test]
    fn r702_drawer_style_m3_defaults() {
        let s = DrawerStyle::m3_default();
        assert_eq!(s.scrim_alpha, 0x66);
        assert_eq!(s.panel_width, 280);
        assert_eq!(s.panel_padding, 12);
        assert_eq!(s.item_gap, 4);
        assert_eq!(s.title_font_px, 14);
        assert_eq!(
            s.edge,
            DrawerEdge::Left,
            "default edge is the left nav rail"
        );
        assert_eq!(
            DrawerStyle::m3_default().with_edge(DrawerEdge::Right).edge,
            DrawerEdge::Right,
            "with_edge overrides the anchor edge",
        );
    }

    #[test]
    fn r702_scrim_color_matches_dialog_weight() {
        assert_eq!(
            DrawerStyle::m3_default().scrim_color(),
            Color::rgba(0, 0, 0, 0x66),
        );
    }

    #[test]
    fn r702_scrim_fills_window_and_anchors_origin() {
        let scene = drawer(DrawerEdge::Left);
        let scrim = find_container(&scene, "drawer_scrim").expect("scrim node");
        assert_eq!(
            scrim.layout.size,
            Size::px(520, 360),
            "scrim covers viewport"
        );
        assert_eq!(
            scrim.layout.absolute_position,
            Some((0, 0)),
            "anchored at origin"
        );
        assert_eq!(scrim.style.fill, Color::rgba(0, 0, 0, 0x66));
    }

    #[test]
    fn r702_left_edge_justifies_panel_to_start() {
        let scene = drawer(DrawerEdge::Left);
        let scrim = find_container(&scene, "drawer_scrim").expect("scrim node");
        assert_eq!(scrim.layout.flex_direction, FlexDirection::Row);
        assert_eq!(scrim.layout.justify_content, JustifyContent::Start);
        assert_eq!(scrim.layout.align_items, AlignItems::Stretch);
    }

    #[test]
    fn r702_right_edge_justifies_panel_to_end() {
        let scene = drawer(DrawerEdge::Right);
        let scrim = find_container(&scene, "drawer_scrim").expect("scrim node");
        assert_eq!(scrim.layout.justify_content, JustifyContent::End);
    }

    #[test]
    fn r702_panel_full_height_fixed_width_elevated() {
        let t = theme();
        let scene = drawer(DrawerEdge::Left);
        let panel = find_container(&scene, "drawer_panel").expect("panel node");
        assert_eq!(
            panel.layout.size.width,
            SizeValue::Px(280),
            "fixed nav width"
        );
        assert_eq!(
            panel.layout.size.height,
            SizeValue::Px(360),
            "full window height"
        );
        assert_eq!(panel.style.fill, t.resolve(ColorRole::SurfaceContainerHigh));
        // R711 — MD3 modal drawer = Level 1 elevation.
        assert_eq!(
            panel.style.shadows,
            crate::elevation::elevation(crate::elevation::DRAWER_LEVEL),
            "drawer panel carries the shared MD3 L1 elevation shadow",
        );
    }

    #[test]
    fn r702_title_present_and_items_in_order() {
        let scene = drawer(DrawerEdge::Left);
        assert!(
            all_text(&scene).contains(&"Navigation".to_string()),
            "title node painted"
        );
        assert_eq!(
            external_tags(&scene),
            vec!["drawer_item_0".to_string(), "drawer_item_1".to_string()],
            "items laid out top-to-bottom in the passed order",
        );
    }

    #[test]
    fn r702_empty_title_omits_node() {
        let scene = view_drawer(
            "drawer_scrim",
            "drawer_panel",
            "",
            vec![item("drawer_item_0")],
            (400, 300),
            &theme(),
            &DrawerStyle::m3_default(),
        );
        assert!(all_text(&scene).is_empty(), "no empty title text node");
    }
}
