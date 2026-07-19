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
}
