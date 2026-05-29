//! R703 §5.16 §5.50 — shared modal **scrim** (backdrop) primitive.
//!
//! A scrim is the full-window dim container every modal overlay places
//! behind its panel: an absolutely-positioned [`Scene::Container`] sized
//! to the viewport, filled with a translucent black, tagged so the
//! input router resolves backdrop hits to it, and laying out a single
//! child (the modal panel) per a caller-supplied flex policy.
//!
//! ## Why this module exists (R703 SSOT lift)
//!
//! R693 [`crate::dialog`] introduced the scrim; R702 [`crate::drawer`]
//! became the 2nd consumer and — at first — *reimplemented* the same
//! backdrop (the `0x66` alpha constant, the `scrim_color` derivation,
//! and the absolute-full-window container construction were all
//! duplicated verbatim). A modal scrim is a framework primitive, so two
//! independent copies violate SSOT (`[[r47-class-incident-prevention]]`)
//! the moment a 2nd consumer exists (`[[abstraction-needs-second-consumer]]`
//! "2nd impl ⇒ lift"). This module is that lift: one home for the scrim
//! weight + fill + backdrop construction, consumed by both
//! [`crate::dialog`] and [`crate::drawer`] (and any future modal: a
//! bottom sheet, a scrimmed popover, …). The only thing a consumer
//! varies is how its panel is positioned inside the scrim (centred for a
//! dialog, edge-pinned full-height for a drawer) — passed as the flex
//! triple.

use pinion_core::scene::ContainerNode;
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size};
use pinion_core::{Color, Scene};

/// R703 §5.50 — Material 3 modal scrim opacity over black, 0–255. M3
/// scrim ≈ 32 % (`0x52`); pinion uses a slightly heavier `0x66` (≈ 40 %)
/// so the dimmed background reads clearly against light surfaces. The
/// single source of truth for both [`crate::dialog`] and
/// [`crate::drawer`] defaults.
pub const M3_SCRIM_ALPHA: u8 = 0x66;

/// R703 §5.50 — the scrim fill: black at `alpha`. Modal `Style` carriers
/// (`DialogStyle::scrim_color` / `DrawerStyle::scrim_color`) delegate
/// here so the fill derivation lives in one place.
#[must_use]
pub const fn scrim_fill(alpha: u8) -> Color {
    Color::rgba(0, 0, 0, alpha)
}

/// R703 §5.16 §5.50 — build a modal scrim backdrop: a full-window
/// absolutely-positioned [`Scene::Container`] tagged `scrim_tag`, filled
/// with `fill`, laying out the single `panel` child per the
/// (`flex`, `align`, `justify`) policy.
///
/// # Arguments
///
/// - `scrim_tag` — the backdrop container tag; the input router resolves
///   background clicks to it. Whether the click is swallowed (inert
///   modal, dialog) or light-dismisses (drawer) is decided by the
///   binding choosing to bind an `External` to this tag — not here.
/// - `fill` — the scrim colour (use [`scrim_fill`]).
/// - `viewport` — `(width, height)` window dimensions; the scrim fills
///   the viewport so it is the topmost hit target everywhere except over
///   the panel.
/// - `flex` / `align` / `justify` — how the `panel` is positioned inside
///   the scrim (e.g. Column + Center + Center centres a dialog; Row +
///   Stretch + Start pins a left drawer full-height).
/// - `panel` — the pre-composed modal panel scene.
///
/// # Returns
///
/// A [`Scene::Container`] tagged `scrim_tag`, anchored at the origin and
/// sized to the window. Place it **last** in the root child list so it
/// paints over (and hit-tests above) the rest of the scene.
#[must_use]
pub fn scrim_backdrop(
    scrim_tag: &'static str,
    fill: Color,
    viewport: (u32, u32),
    flex: FlexDirection,
    align: AlignItems,
    justify: JustifyContent,
    panel: Scene,
) -> Scene {
    let (win_w, win_h) = viewport;
    Scene::Container(
        ContainerNode::new(vec![panel])
            .with_tag(scrim_tag)
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(0, 0)
                    .with_size(Size::px(win_w, win_h))
                    .flex(flex)
                    .with_align_items(align)
                    .with_justify(justify),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ContainerNode;

    #[test]
    fn r703_m3_scrim_alpha_is_0x66() {
        assert_eq!(M3_SCRIM_ALPHA, 0x66);
    }

    #[test]
    fn r703_scrim_fill_is_black_at_alpha() {
        assert_eq!(scrim_fill(0x66), Color::rgba(0, 0, 0, 0x66));
        assert_eq!(scrim_fill(0), Color::rgba(0, 0, 0, 0));
    }

    #[test]
    fn r703_backdrop_anchors_origin_fills_viewport_and_holds_panel() {
        let panel = Scene::Container(ContainerNode::new(vec![]).with_tag("panel"));
        let scene = scrim_backdrop(
            "scrim",
            scrim_fill(M3_SCRIM_ALPHA),
            (520, 360),
            FlexDirection::Column,
            AlignItems::Center,
            JustifyContent::Center,
            panel,
        );
        let Scene::Container(c) = &scene else {
            panic!("scrim is a container");
        };
        assert_eq!(c.tag.as_deref(), Some("scrim"));
        assert_eq!(c.style.fill, Color::rgba(0, 0, 0, 0x66));
        assert_eq!(c.layout.absolute_position, Some((0, 0)));
        assert_eq!(c.layout.size, Size::px(520, 360));
        assert_eq!(c.layout.flex_direction, FlexDirection::Column);
        assert_eq!(c.layout.align_items, AlignItems::Center);
        assert_eq!(c.layout.justify_content, JustifyContent::Center);
        assert_eq!(c.children.len(), 1, "scrim holds the single panel child");
    }

    #[test]
    fn r703_backdrop_honours_caller_flex_policy() {
        // Drawer-style policy: row + stretch + end (right edge).
        let panel = Scene::Container(ContainerNode::new(vec![]).with_tag("panel"));
        let scene = scrim_backdrop(
            "scrim",
            scrim_fill(M3_SCRIM_ALPHA),
            (400, 300),
            FlexDirection::Row,
            AlignItems::Stretch,
            JustifyContent::End,
            panel,
        );
        let Scene::Container(c) = &scene else {
            panic!("scrim is a container");
        };
        assert_eq!(c.layout.flex_direction, FlexDirection::Row);
        assert_eq!(c.layout.align_items, AlignItems::Stretch);
        assert_eq!(c.layout.justify_content, JustifyContent::End);
    }
}
