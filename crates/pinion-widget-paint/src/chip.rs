//! R756 §5.38 — the Material 3 **chip** paint substrate: the genuinely-shared
//! container skin (corner radius, height, inner gap, outline width, the
//! state-layer-tinted `BoxStyle`, and the centered inner flex row) between the
//! M3 chip *variants* — the filter chip (R753, fixed-width, leading check) and
//! the input chip (R756, content-hugging, trailing `×`).
//!
//! ## Why this module exists (2nd-consumer lift)
//!
//! R753 `hello-filter-chip` was the **1st** chip-paint consumer and built its
//! chip body inline (the inline-first rule — a single consumer does not
//! pre-abstract). R756 `hello-input-chip` is the **2nd** chip consumer: a
//! removable input chip is the same M3 *container* skin — an 8 px-radius,
//! 36 px-tall rounded rectangle with a `6` px inner gap, optionally outlined,
//! tinted by the shared [`crate::state_layer`] overlay — differing only in
//! what it *fills* with, what *children* it carries (leading check vs trailing
//! `×`), and its *width strategy* (fixed vs content-hug). Two consumers of the
//! same container skin is the [[abstraction-needs-second-consumer]] gate
//! firing, so the shared core lifts here; the variant-specific ink / children /
//! fill-choice stay at each callsite (they genuinely diverge — not a copy).
//!
//! What is shared (this module): [`CHIP_RADIUS`] / [`CHIP_HEIGHT`] /
//! [`INNER_GAP`] / [`OUTLINE_W`] tokens, [`chip_style`] (state-layer-tinted,
//! rounded, optionally-bordered `BoxStyle`), [`chip_layout`] (the centered
//! inner flex row with [`INNER_GAP`], sized + optionally padded).
//!
//! What stays per-callsite (NOT lifted): the base-fill chooser (Accent-when-on
//! vs transparent for the filter chip; a surface-container tone for the input
//! chip), the leading/trailing children, and the ink colour — each variant's
//! own affordance.

use pinion_core::Color;
use pinion_core::scene::Rect;
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size,
};
use pinion_core::theme::Theme;
use pinion_core::widgets::interaction::InteractionState;

/// M3 chip corner radius — 8 px, the spec value. Deliberately *not* the
/// fully-rounded stadium a segmented button uses, so the two skins read as
/// different widgets.
pub const CHIP_RADIUS: u32 = 8;

/// M3 chip height — 36 px, the spec value.
pub const CHIP_HEIGHT: u32 = 36;

/// Gap between a chip's leading affordance, label, and trailing affordance.
pub const INNER_GAP: u32 = 6;

/// Outlined-chip border width — 1 px.
pub const OUTLINE_W: u32 = 1;

/// Shared M3 chip `BoxStyle`: `base_fill` tinted by the [`crate::state_layer`]
/// overlay for the current interaction `state`, rounded to [`CHIP_RADIUS`],
/// with an optional outline `border`.
///
/// The caller chooses `base_fill` (the resting fill — e.g. `Accent` when a
/// filter chip is selected, a surface-container tone or transparent otherwise)
/// and whether the chip is outlined (`border`). The hover / pressed / disabled
/// overlay is applied here through the single state-layer SSOT, so no chip
/// variant re-derives the raw M3 opacity literals.
#[must_use]
pub fn chip_style<S: InteractionState + Copy>(
    base_fill: Color,
    border: Option<Border>,
    state: S,
    theme: &Theme,
) -> BoxStyle {
    let fill = crate::state_layer::state_layer(base_fill, state, theme);
    let mut style = BoxStyle::filled(fill).with_corner_radius(CHIP_RADIUS);
    if let Some(border) = border {
        style = style.with_border(border);
    }
    style
}

/// Shared M3 chip inner layout: a centered flex row with [`INNER_GAP`] between
/// children, sized by `size`, with optional `padding` insets.
///
/// A fixed-width filter chip passes `Size::px(w, CHIP_HEIGHT)` + `None`; a
/// content-hugging input chip passes
/// `Size::auto().with_height(SizeValue::Px(CHIP_HEIGHT))` + horizontal
/// `padding` so the chip width tracks its label.
#[must_use]
pub fn chip_layout(size: Size, padding: Option<Rect>) -> LayoutStyle {
    let mut layout = LayoutStyle::new()
        .flex(FlexDirection::Row)
        .with_justify(JustifyContent::Center)
        .with_align_items(AlignItems::Center)
        .with_gap(INNER_GAP)
        .with_size(size);
    if let Some(insets) = padding {
        layout = layout.with_padding(insets);
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::style::SizeValue;
    use pinion_core::theme::ColorRole;
    use pinion_core::widgets::button::ButtonState;

    #[test]
    fn selected_and_unselected_styles_differ_only_by_border() {
        // R756 — the M3 filter-chip affordance: the selected chip drops the
        // outline, the unselected chip carries it. Same fill choice + idle
        // state isolates the border as the only difference.
        let theme = Theme::light();
        let fill = theme.resolve(ColorRole::Accent);
        let outlined = chip_style(
            fill,
            Some(Border::new(theme.resolve(ColorRole::Outline), OUTLINE_W)),
            ButtonState::Idle,
            &theme,
        );
        let bare = chip_style(fill, None, ButtonState::Idle, &theme);
        assert_eq!(
            outlined.fill, bare.fill,
            "same base fill, idle: identical tint"
        );
        assert_eq!(
            outlined.corner_radius, bare.corner_radius,
            "both rounded to CHIP_RADIUS",
        );
        assert!(
            outlined.border.is_some(),
            "outlined chip carries the border"
        );
        assert!(bare.border.is_none(), "bare chip drops the border");
    }

    #[test]
    fn hover_tints_the_fill_through_the_state_layer() {
        // The shared state-layer overlay is applied inside chip_style: a
        // hovered chip's fill differs from the idle resting fill.
        let theme = Theme::light();
        let base = theme.resolve(ColorRole::Accent);
        let idle = chip_style(base, None, ButtonState::Idle, &theme);
        let hover = chip_style(base, None, ButtonState::Hover, &theme);
        assert_eq!(idle.fill, base, "idle chip is the untinted base fill");
        assert_ne!(hover.fill, base, "hover chip is tinted by the state layer");
    }

    #[test]
    fn layout_carries_height_and_inner_gap() {
        // Content-hug input-chip shape: Auto width, fixed height, padded.
        let layout = chip_layout(
            Size::auto().with_height(SizeValue::Px(CHIP_HEIGHT)),
            Some(Rect::new(0, 12, 0, 12)),
        );
        assert_eq!(layout.gap, INNER_GAP, "inner gap is the shared token");
        assert_eq!(
            layout.size.height,
            SizeValue::Px(CHIP_HEIGHT),
            "height pinned"
        );
        assert_eq!(layout.size.width, SizeValue::Auto, "width hugs content");
        assert_eq!(
            layout.padding,
            Rect::new(0, 12, 0, 12),
            "horizontal insets carried"
        );
    }

    #[test]
    fn fixed_width_layout_has_no_padding() {
        // Fixed-width filter-chip shape: Px width + height, no padding.
        let layout = chip_layout(Size::px(104, CHIP_HEIGHT), None);
        assert_eq!(layout.size.width, SizeValue::Px(104));
        assert_eq!(layout.size.height, SizeValue::Px(CHIP_HEIGHT));
        assert_eq!(
            layout.padding,
            Rect::default(),
            "no insets for fixed-width chips"
        );
        assert_eq!(layout.gap, INNER_GAP);
    }
}
