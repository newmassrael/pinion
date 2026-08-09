//! R696 §5.38 §5.50 — backend-agnostic Disclosure paint composition.
//!
//! Composes the WAI-ARIA APG **disclosure** (show/hide) visual: a
//! clickable header row carrying an expand twisty + a summary label,
//! followed — only while expanded — by the content panel the header
//! reveals. A stack of these is an accordion; one is a collapsible
//! section.
//!
//! ## Naming
//!
//! Mirrors [`crate::checkbox`]: a [`DisclosureStyle`] carrier struct
//! with [`DisclosureStyle::m3`] defaults and a [`view_disclosure`] fn
//! that produces a [`Scene`] fragment the binding's outer view-fn
//! wraps in its root container. The signature
//!
//! ```rust,ignore
//! pub fn view_disclosure(
//!     tag: &'static str,
//!     interaction: DisclosureState,
//!     expanded: bool,
//!     theme: &Theme,
//!     style: &DisclosureStyle,
//!     summary: &str,
//!     content: Scene,
//! ) -> Scene;
//! ```
//!
//! follows `checkbox`'s `(tag, state, value, theme, style, label)`
//! shape with an extra `content: Scene` for the revealed panel.
//!
//! ## Structure (why the tag is on the header, not the root)
//!
//! Only the **header** carries `tag`, so the `InputRouter` hit-tests
//! clicks against the header alone — clicking inside the revealed
//! content must not toggle the section. The content panel is therefore
//! a *sibling* of the tagged header, not a child. This also keeps the
//! AT name enrichment (`enrich_names_from_scene`) scoped to the
//! header's subtree: the twisty `TextNode` is
//! [`TextRole::Presentational`] so the DFS lands on the summary text
//! as the disclosure's accessible name, and the panel's text never
//! leaks into the header's name.

use crate::glyph::{
    DISCLOSURE_COLLAPSED as GLYPH_COLLAPSED, DISCLOSURE_EXPANDED as GLYPH_EXPANDED,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode, TextRole};
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::disclosure::DisclosureState;
use pinion_core::{Color, Scene};

/// (R696 §5.50) Material-3 disclosure paint dimensions. Mirrors the
/// [`crate::checkbox::CheckboxStyle`] carrier pattern so binding
/// callers see a uniform `Style` surface across the widget catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisclosureStyle {
    /// Section block width in logical pixels (header + panel share it
    /// so a stacked accordion's edges line up). Default 320.
    pub width: u32,
    /// Header row height in logical pixels (default 48 = M3 list-item
    /// / minimum touch-target height).
    pub header_height: u32,
    /// Header horizontal padding in logical pixels (default 16 = M3
    /// list-item leading/trailing inset).
    pub header_pad_x: u32,
    /// Expand-twisty glyph font size in logical pixels (default 16;
    /// the `\u{25B6}` / `\u{25BC}` triangles read cleanly here across
    /// the `Noto` / `DejaVu` / `Inter` fallback chain — same glyphs +
    /// size the §5.50 `tree_view` substrate uses).
    pub chevron_size_px: u32,
    /// Summary label font size in logical pixels (default 16 ≈ M3
    /// `body-large`).
    pub summary_size_px: u32,
    /// Gap between the twisty and the summary label (default 12).
    pub chevron_gap: u32,
    /// Content-panel inset on all four sides in logical pixels
    /// (default 16).
    pub content_pad: u32,
    /// Header + panel corner radius in logical pixels (default 8 = M3
    /// medium shape token).
    pub corner_radius: u32,
    /// (R1020 §5.39) Keyboard focus stop. When `true`, [`view_disclosure`]
    /// marks the disclosure's header Container `.with_focusable(true)` so
    /// the scene-derived §5.39 enumeration collects its tag as a Tab stop.
    ///
    /// Default `true` (R1030 fail-safe, web native-element model), mirroring
    /// [`crate::button::ButtonStyle::focusable`]: a disclosure header is a Tab
    /// stop by default; a decorative or modal-scoped section opts out with
    /// `.with_focusable(false)` so it stays out of the base Tab order.
    pub focusable: bool,
}

impl DisclosureStyle {
    /// (R696 §5.50) Material-3 disclosure defaults.
    #[must_use]
    pub const fn m3() -> Self {
        Self {
            width: 320,
            header_height: 48,
            header_pad_x: 16,
            chevron_size_px: 16,
            summary_size_px: 16,
            chevron_gap: 12,
            content_pad: 16,
            corner_radius: 8,
            focusable: true,
        }
    }

    /// (R1020 §5.39) Mark this disclosure header a keyboard focus stop
    /// (default `false`). Standalone disclosure headers set this true;
    /// modal-scoped or decorative sections leave it false. Mirrors
    /// [`crate::button::ButtonStyle::with_focusable`]. See
    /// [`Self::focusable`].
    #[must_use]
    pub const fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }
}

impl Default for DisclosureStyle {
    fn default() -> Self {
        Self::m3()
    }
}

/// (R696 §5.50) Header surface ramp. Anchors on
/// [`ColorRole::SurfaceContainerHigh`] (the M3 "raised list row" tone)
/// and layers the canonical state-layer overlays (hover 0.08 / pressed
/// 0.12 toward `OnSurface`, disabled 0.38 toward `Surface`) — identical
/// weights to [`crate::checkbox::checkbox_accent_for`] so the catalog's
/// interactive surfaces share one state-layer language.
#[must_use]
pub fn disclosure_header_for(theme: &Theme, state: DisclosureState) -> Color {
    let base = theme.resolve(ColorRole::SurfaceContainerHigh);
    crate::state_layer::state_layer(base, state, theme)
}

/// (R696 §5.50) another declarative toolkit the M3 disclosure paint scene
/// fragment.
///
/// # Arguments
///
/// - `tag` — dispatch tag carried on the **header** container (the
///   click target); paired with the binding's
///   [`pinion_core::WidgetCore::tag`].
/// - `interaction` — current [`DisclosureState`] statechart
///   projection; drives the header state-layer ramp + text muting.
/// - `expanded` — value-sidecar projection (the §5.16 `bool_field(1)`
///   slot on `(DisclosureState, bool)`); selects the twisty glyph and
///   gates whether the content panel is present.
/// - `theme` — current [`Theme`] palette; the binding resolves it via
///   [`pinion_core::theme::use_theme`] and forwards.
/// - `style` — [`DisclosureStyle`] dimension carrier.
/// - `summary` — header label text; doubles as the AT accessible name
///   (the twisty is presentational, so enrichment lands here).
/// - `content` — the panel body shown only while `expanded`.
///
/// # Returns
///
/// A [`Scene::Container`] (Column) holding the tagged header row and,
/// when `expanded`, the content panel beneath it.
#[must_use]
pub fn view_disclosure(
    tag: &'static str,
    interaction: DisclosureState,
    expanded: bool,
    theme: &Theme,
    style: &DisclosureStyle,
    summary: &str,
    content: Scene,
) -> Scene {
    let text_color = if matches!(interaction, DisclosureState::Disabled) {
        theme.resolve(ColorRole::OnSurfaceMuted)
    } else {
        theme.resolve(ColorRole::OnSurface)
    };
    let glyph = if expanded {
        GLYPH_EXPANDED
    } else {
        GLYPH_COLLAPSED
    };
    let chevron = Scene::Text(
        TextNode::styled(
            glyph,
            Rect::default(),
            TextStyle::new()
                .with_size_px(style.chevron_size_px)
                .with_fg(text_color),
        )
        // Presentational so enrich_names_from_scene skips the twisty
        // and lands on the linguistic summary label.
        .with_role(TextRole::Presentational),
    );
    let summary_node = Scene::Text(TextNode::styled(
        summary,
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.summary_size_px)
            .with_fg(text_color),
    ));
    let header = Scene::Container(
        ContainerNode::new(vec![chevron, summary_node])
            .with_tag(tag)
            .with_style(
                BoxStyle::filled(disclosure_header_for(theme, interaction))
                    .with_corner_radius(style.corner_radius),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(style.chevron_gap)
                    .with_padding(Rect::new(style.header_pad_x, 0, style.header_pad_x, 0))
                    .with_size(Size::px(style.width, style.header_height))
                    // (R1020 §5.39) Carry the binding's focus-stop opt-in
                    // onto the header (the tag carrier) so the
                    // scene-derived enumeration collects this disclosure's
                    // tag as a Tab stop.
                    .with_focusable(style.focusable),
            ),
    );
    let mut children = vec![header];
    if expanded {
        let panel = Scene::Container(
            ContainerNode::new(vec![content])
                .with_style(BoxStyle::filled(
                    theme.resolve(ColorRole::SurfaceContainerLow),
                ))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Column)
                        .with_padding(Rect::new(
                            style.content_pad,
                            style.content_pad,
                            style.content_pad,
                            style.content_pad,
                        ))
                        // Width pinned to the section block; height
                        // follows the content (Auto).
                        .with_size(Size::width_px(style.width)),
                ),
        );
        children.push(panel);
    }
    Scene::Container(
        ContainerNode::new(children).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_size(Size::width_px(style.width)),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn light() -> Theme {
        Theme::light()
    }

    fn text_summary() -> Scene {
        Scene::Text(TextNode::styled(
            "panel body",
            Rect::default(),
            TextStyle::new().with_size_px(14),
        ))
    }

    /// Count direct + one-level-nested children of the root container.
    fn root_child_count(scene: &Scene) -> usize {
        match scene {
            Scene::Container(c) => c.children.len(),
            _ => 0,
        }
    }

    #[test]
    fn r696_style_m3_constants() {
        let s = DisclosureStyle::m3();
        assert_eq!(s.width, 320);
        assert_eq!(s.header_height, 48);
        assert_eq!(s.header_pad_x, 16);
        assert_eq!(s.chevron_size_px, 16);
        assert_eq!(s.summary_size_px, 16);
        assert_eq!(s.chevron_gap, 12);
        assert_eq!(s.content_pad, 16);
        assert_eq!(s.corner_radius, 8);
    }

    #[test]
    fn r696_collapsed_has_header_only() {
        let scene = Owner::new().run(|| {
            view_disclosure(
                "section",
                DisclosureState::Idle,
                false,
                &light(),
                &DisclosureStyle::m3(),
                "Details",
                text_summary(),
            )
        });
        // Collapsed: only the header child (no content panel).
        assert_eq!(root_child_count(&scene), 1);
    }

    #[test]
    fn r696_expanded_has_header_plus_panel() {
        let scene = Owner::new().run(|| {
            view_disclosure(
                "section",
                DisclosureState::Idle,
                true,
                &light(),
                &DisclosureStyle::m3(),
                "Details",
                text_summary(),
            )
        });
        // Expanded: header + content panel.
        assert_eq!(root_child_count(&scene), 2);
    }

    #[test]
    fn r696_carries_dispatch_tag_on_header() {
        let scene = Owner::new().run(|| {
            view_disclosure(
                "section",
                DisclosureState::Idle,
                false,
                &light(),
                &DisclosureStyle::m3(),
                "Details",
                text_summary(),
            )
        });
        assert!(scene.contains_tag("section"));
        // The tag lives on the header child, never on the bare root
        // (so content clicks don't toggle). Verify the header (first
        // child) is the tagged node.
        if let Scene::Container(root) = &scene {
            assert!(matches!(
                &root.children[0],
                Scene::Container(h) if h.tag.as_deref() == Some("section")
            ));
            assert!(root.tag.is_none(), "root column must be untagged");
        } else {
            panic!("root must be a Container");
        }
    }

    #[test]
    fn r696_twisty_glyph_flips_with_expanded() {
        // Collapsed → right triangle; expanded → down triangle. The
        // twisty is the first child of the header.
        let collapsed = Owner::new().run(|| {
            view_disclosure(
                "section",
                DisclosureState::Idle,
                false,
                &light(),
                &DisclosureStyle::m3(),
                "Details",
                text_summary(),
            )
        });
        let expanded = Owner::new().run(|| {
            view_disclosure(
                "section",
                DisclosureState::Idle,
                true,
                &light(),
                &DisclosureStyle::m3(),
                "Details",
                text_summary(),
            )
        });
        assert_eq!(header_twisty_glyph(&collapsed), GLYPH_COLLAPSED);
        assert_eq!(header_twisty_glyph(&expanded), GLYPH_EXPANDED);
    }

    fn header_twisty_glyph(scene: &Scene) -> &str {
        let Scene::Container(root) = scene else {
            panic!("root container")
        };
        let Scene::Container(header) = &root.children[0] else {
            panic!("header container")
        };
        let Scene::Text(t) = &header.children[0] else {
            panic!("twisty text")
        };
        // Twisty is Presentational so the AT name lands on the summary.
        assert_eq!(t.role, Some(TextRole::Presentational));
        // Borrow the glyph string slice out of the TextNode content.
        glyph_of(t)
    }

    fn glyph_of(t: &TextNode) -> &str {
        // `content` is the rendered string; both glyphs are single
        // scalar values so an exact compare is unambiguous.
        if t.content == GLYPH_EXPANDED {
            GLYPH_EXPANDED
        } else if t.content == GLYPH_COLLAPSED {
            GLYPH_COLLAPSED
        } else {
            panic!("unexpected twisty glyph: {:?}", t.content);
        }
    }

    #[test]
    fn r696_header_ramp_idle_resolves_to_surface_container_high() {
        let theme = light();
        assert_eq!(
            disclosure_header_for(&theme, DisclosureState::Idle),
            theme.resolve(ColorRole::SurfaceContainerHigh),
        );
    }

    #[test]
    fn r696_header_hover_overlay_lerps_toward_on_surface() {
        let theme = light();
        let expected = theme
            .resolve(ColorRole::SurfaceContainerHigh)
            .lerp(theme.resolve(ColorRole::OnSurface), 0.08);
        assert_eq!(
            disclosure_header_for(&theme, DisclosureState::Hover),
            expected
        );
    }
}
