//! R870 — the shared floating-popup **surface** skin.
//!
//! The panel container of a combobox / select dropdown and the property-grid
//! choice + colour popups all draw the same surface: a `SurfaceContainer`
//! fill, a 6 px corner radius, a 1 px outline, and the `MENU_LEVEL` elevation
//! shadow. That recipe was copy-pasted byte-identical across four callsites
//! (`hello-combobox`, `hello-combobox-editable`, and the property grid's two
//! popups) — a Rule-of-Three lift the colour popup pushed to a 4th copy, so
//! the surface SSOT lifts here.
//!
//! Only the *surface* lifts here: each callsite still owns its panel **layout**
//! (anchor position, size, flex direction, padding). The one shared slice of
//! that layout — the anchored-overlay **vertical flip** (drop below, flip above
//! on overflow) — lifted separately to [`crate::anchor::flip_y`] once the
//! property-grid + data-grid dropdowns became its 2nd/3rd consumers (R1378),
//! the 2nd-binding gate this doc earmarked. The cross-axis clamp and per-panel
//! anchor `x` stay per-callsite (still single-consumer).

use pinion_core::style::{Border, BoxStyle};
use pinion_core::theme::{ColorRole, Theme};

use crate::elevation::{MENU_LEVEL, elevation};

/// The surface skin of a floating popup panel — `SurfaceContainer` fill, a
/// 6 px corner radius, a 1 px outline, and the `MENU_LEVEL` elevation shadow.
/// The caller owns the panel's layout (anchor / size / flex / padding).
#[must_use]
pub fn popup_surface(theme: &Theme) -> BoxStyle {
    BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainer))
        .with_corner_radius(6)
        .with_border(Border::new(theme.resolve(ColorRole::Outline), 1))
        .with_shadows(elevation(MENU_LEVEL))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Scene;

    #[test]
    fn popup_surface_is_the_floating_panel_skin() {
        let s = popup_surface(&Theme::light());
        assert!(s.border.is_some(), "popup panel has a 1px outline");
        assert!(
            !s.shadows.is_empty(),
            "popup panel casts the MENU_LEVEL elevation shadow"
        );
        assert_eq!(s.corner_radius, 6, "6 px corner radius");
    }

    /// ★★ R1674 — the floating-panel skin, used as its own docs describe,
    /// keeps its content inside the outline it strokes. The crate gate
    /// ([`crate::frame_gate`]).
    ///
    /// This module publishes a STYLE rather than a scene, so the gate has to
    /// assemble the documented usage. That is the honest form of the question
    /// here: the border belongs to this skin, and a consumer laying content at
    /// the panel's full box would cover it — which is what the gate would say.
    #[test]
    fn r1674_the_popup_skin_keeps_its_content_inside_its_outline() {
        crate::frame_gate::assert_frame_contained("popup panel", &mut |_w, _h| {
            let theme = Theme::light();
            let style = popup_surface(&theme);
            let inset = style.border.map_or(0, |b| b.width);
            Scene::Container(
                pinion_core::scene::ContainerNode::new(vec![Scene::Text(
                    pinion_core::scene::TextNode::styled(
                        "Rename",
                        pinion_core::scene::Rect::default(),
                        pinion_core::style::TextStyle::new()
                            .with_size_px(13)
                            .with_fg(theme.resolve(ColorRole::OnSurface))
                            .with_overflow(pinion_core::style::TextOverflow::Ellipsis),
                    ),
                )])
                .with_tag("popup")
                .with_style(style)
                .with_layout(
                    pinion_core::style::LayoutStyle::new()
                        .flex(pinion_core::style::FlexDirection::Column)
                        .with_size(pinion_core::style::Size::px(160, 40))
                        // The skin's own outline is reserved, which is the
                        // arithmetic `containment::content_of` performs and the
                        // check performs — one rule, both sides.
                        .with_padding(pinion_core::scene::Rect::new(inset, inset, inset, inset)),
                ),
            )
        });
    }
}
