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
//! inner flex row with [`INNER_GAP`], sized + optionally padded), and — R1446,
//! at its 3rd consumer — [`selection_border`], the `Outline`-while-unselected
//! rule every on/off chip obeys.
//!
//! What stays per-callsite (NOT lifted): the base-fill chooser (Accent-when-on
//! for the filter chip and the model chart's field pickers; a surface-container
//! tone for the series toggle and the input chip), the leading/trailing
//! children, the width strategy, and the ink colour — each variant's own
//! affordance. The line between the two halves is whether a divergence would
//! be a **bug** (shared) or a **choice** (local).

use pinion_core::Color;
use pinion_core::Scene;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextOverflow,
    TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
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

/// An option chip's label size, px (R1568).
///
/// A token rather than an [`option_chip`] parameter, and the evidence is the
/// lift itself: all three bindings that had written that wrapper out chose
/// **13** independently. Three consumers agreeing is what separates a shared
/// token from a per-callsite choice — the line this module's header draws.
pub const CHIP_LABEL_PX: u32 = 13;

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

/// R1446 — the M3 **selection** border rule for a chip that can be on or off:
/// an unselected chip carries the `Outline` border, a selected one drops it
/// (the tonal fill is the affordance, and an outline over it reads as a second,
/// competing edge).
///
/// Lifted at the Rule of Three: `hello-filter-chip` (R753), `hello-series-toggle`
/// (R1379) and `hello-model-chart` (R1446) each spelled this same two-arm
/// chooser. Unlike the base *fill* — which genuinely diverges per variant
/// (`Accent` for a filter chip, `SurfaceContainerHigh` for a series toggle) and
/// so stays at each callsite — the border arm is the spec's rule, not a taste:
/// a divergence between the three would be a bug. Chips with no on/off state
/// (the input chip's `×`) pass `None` to [`chip_style`] directly and do not
/// call this.
#[must_use]
pub fn selection_border(theme: &Theme, selected: bool) -> Option<Border> {
    (!selected).then(|| Border::new(theme.resolve(pinion_core::ColorRole::Outline), OUTLINE_W))
}

/// Shared M3 chip inner layout: a centered flex row with [`INNER_GAP`] between
/// children, sized by `size`, with optional `padding` insets.
///
/// A fixed-width filter chip passes `Size::px(w, CHIP_HEIGHT)` + `None`; a
/// content-hugging input chip passes
/// `Size::auto().with_height(SizeValue::Px(CHIP_HEIGHT))` + horizontal
/// `padding` so the chip width tracks its label.
/// A whole **option chip**: an accent-when-selected container carrying one
/// centred label, sized to `width` (R1568).
///
/// The 3rd-consumer lift of a wrapper three chart bindings had written out
/// byte-identically (`hello-boxplot` R1553, `hello-candlestick` R1567,
/// `hello-polar` R1568). Everything this module's header calls "per-callsite"
/// is still per-callsite — the base-fill *rule* is the one thing these three
/// share and it is the M3 filter-chip rule, `Accent` when on and transparent
/// when off, with the ink following it so the label never sits on its own
/// background.
///
/// The label size is NOT a parameter: see [`CHIP_LABEL_PX`].
///
/// `focusable` is a parameter here because this function is handed one chip and
/// one chip cannot answer for its row: a radio group is ONE tab stop with a
/// roving descendant, so its cells are hit targets and not stops, while
/// independent toggles are each a stop. **R1721 made that answer derivable** —
/// a caller with a row asks
/// [`ChipGroup::is_a_stop`](pinion_core::widgets::chip_group::ChipGroup::is_a_stop),
/// which answers for the row and for its chips from ONE derivation, so a screen
/// cannot make both a stop. Measured on 2026-08-19 by driving the analysis tool:
/// one screen made every chip of an at-most-one row its own Tab stop, another
/// made none of them anything at all, and a third — correct — had simply been
/// written by somebody who knew.
#[must_use]
pub fn option_chip<S: InteractionState + Copy>(
    tag: String,
    label: &str,
    selected: bool,
    focusable: bool,
    width: u32,
    state: S,
    theme: &Theme,
) -> Scene {
    let base = if selected {
        theme.resolve(ColorRole::Accent)
    } else {
        Color::rgba(0, 0, 0, 0)
    };
    let ink = if selected {
        theme.resolve(ColorRole::OnAccent)
    } else {
        theme.resolve(ColorRole::OnSurface)
    };
    let text = Scene::Text(TextNode::styled(
        label.to_owned(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(CHIP_LABEL_PX)
            .with_fg(ink)
            // ★ R1674 — a chip is laid out at a width its CALLER picks, so a
            // label that does not fit is the ordinary case rather than the
            // exceptional one, and the default `Visible` painted it straight
            // through the chip's own outline (measured: 18px past a 140px
            // chip). Ellipsis is also what the reference does with a chip
            // label; the difference is that here the policy is in the scene,
            // where `scene/text_painted` reports the string a reader is losing.
            .with_overflow(TextOverflow::Ellipsis),
    ));
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_tag(tag)
            .with_style(chip_style(
                base,
                selection_border(theme, selected),
                state,
                theme,
            ))
            .with_layout(chip_layout(Size::px(width, CHIP_HEIGHT), None).with_focusable(focusable)),
    )
}

// ★★★ R1721.1 — a `chip_row(&ChipGroup, width, theme)` stood here for one round
// and is **deleted**, by the round's own 3rd-consumer self-grep: it had zero
// production callers, and it would not have fitted the one screen it was written
// for. `hello-filter-chip`'s pill carries a leading check glyph and a fill this
// gallery's palette substitutes for M3's secondary tier; [`option_chip`] paints
// neither, so adopting it would have changed the affordance rather than shared
// it. The two analysis screens keep their own tones for the same reason — the
// reference's, not Material's.
//
// What the four screens actually share is the ONE thing a chip cannot work out by
// looking at itself, and that is a question rather than a painter:
// `ChipGroup::is_a_stop`. The R1719.1 rule, on a painter instead of a wire form:
// symmetry is not a consumer.
//
// ★ R1721.2 corrects this note's own arithmetic, which was false the moment it
// was written: it said the deletion took away "a third meaning of the name
// `chip_row`, two bindings already have local ones". Measured — **one** binding
// has one (`hello-model-chart`, a different signature), because the same commit
// renamed the other to `filters_row`. So the wrapper was the second meaning, and
// after the rename there is one. The session's closing audit found it, which is
// what that audit's second question is for: a claim this round *wrote* is as much
// its output as the code, and this one contradicted an edit three files away.
//
// What went with the wrapper, stated rather than left silent: four tests were
// R1721's here and two remain. `…_read_one_derivation` is **subsumed** — the
// surviving `…_a_chip_is_a_focus_stop_exactly_when_its_row_is_not` now drives
// `option_chip` with `is_a_stop` directly, which is that pairing. `…_an_empty_row
// _paints_no_pills` is **vacuous** without a row painter: a caller iterates
// `row.chips()`, and the empty row's own answers are asserted in `pinion-core`.

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
    use pinion_core::widgets::chip_group::{Chip, ChipGroup, ChipPosture, Choice};

    fn row(choice: Choice) -> ChipGroup {
        ChipGroup::new(
            "row",
            "Saved filters",
            vec![
                Chip::new("row.0", "units only", true),
                Chip::new("row.1", "shared memory", false).with_posture(ChipPosture::Hover),
                Chip::new("row.2", "locked", false).with_posture(ChipPosture::Locked),
            ],
            choice,
        )
    }

    fn focusable_of(scene: &Scene) -> bool {
        match scene {
            Scene::Container(node) => node.layout.focusable,
            other => panic!("a chip is a container, got {other:?}"),
        }
    }

    /// ★★★★★ R1721 — the focus stop is the row's rule, not a parameter a screen
    /// guesses, and this is the pairing a caller actually writes: the row answers
    /// `is_a_stop` and the answer goes straight into the pill.
    ///
    /// R1721.1 rewrote it to drive `option_chip` directly, because the wrapper it
    /// used to drive had no production caller and was deleted. That is the point
    /// rather than an accident: what the four screens share is the QUESTION, and a
    /// test that only exercised a wrapper would have been testing the half nobody
    /// uses.
    #[test]
    fn r1721_a_chip_is_a_focus_stop_exactly_when_its_row_is_not() {
        let theme = Theme::light();
        for choice in Choice::ALL {
            let group = row(choice);
            let painted: Vec<bool> = group
                .chips()
                .iter()
                .map(|chip| {
                    focusable_of(&option_chip(
                        chip.tag.clone(),
                        &chip.label,
                        chip.on,
                        group.is_a_stop(&chip.tag),
                        120,
                        chip.posture,
                        &theme,
                    ))
                })
                .collect();
            assert_eq!(
                painted,
                [!choice.is_composite(); 3],
                "{}: a composite row is the stop, so its chips are not",
                choice.wire()
            );
        }
    }

    /// The pill carries the chip's own three facts: its tag, its on-ness (which
    /// drops the outline) and its posture (which tints the fill).
    #[test]
    fn r1721_each_pill_carries_its_own_chip() {
        let theme = Theme::light();
        let group = row(Choice::Any);
        let pills: Vec<Scene> = group
            .chips()
            .iter()
            .map(|chip| {
                option_chip(
                    chip.tag.clone(),
                    &chip.label,
                    chip.on,
                    group.is_a_stop(&chip.tag),
                    120,
                    chip.posture,
                    &theme,
                )
            })
            .collect();
        assert_eq!(pills.len(), 3, "one pill per chip");
        let tags: Vec<_> = pills
            .iter()
            .map(|scene| match scene {
                Scene::Container(node) => node.tag.clone().expect("a pill is tagged"),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(tags, ["row.0", "row.1", "row.2"]);
        let styles: Vec<_> = pills
            .iter()
            .map(|scene| match scene {
                Scene::Container(node) => node.style.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert!(
            styles[0].border.is_none(),
            "the chip that is on drops its outline"
        );
        assert!(styles[1].border.is_some(), "the ones that are off keep it");
        assert_ne!(
            styles[1].fill, styles[2].fill,
            "a hovered chip and a locked one are tinted differently"
        );
    }

    #[test]
    fn r1446_selection_border_is_the_outline_only_while_unselected() {
        let theme = Theme::light();
        assert!(
            selection_border(&theme, true).is_none(),
            "a selected chip drops its outline — the tonal fill is the affordance"
        );
        let border = selection_border(&theme, false).expect("an unselected chip is outlined");
        assert_eq!(border.width, OUTLINE_W);
        assert_eq!(border.color, theme.resolve(ColorRole::Outline));
    }

    #[test]
    fn r1446_selection_border_follows_the_theme_not_a_frozen_colour() {
        // The reason this is one definition rather than three: an `Outline`
        // that answered the light tone in a dark app would be a bug in every
        // consumer at once, and only one of them would get reported.
        let light = selection_border(&Theme::light(), false).expect("outlined");
        let dark = selection_border(&Theme::dark(), false).expect("outlined");
        assert_ne!(
            light.color, dark.color,
            "each theme resolves its own outline"
        );
    }

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

    /// ★★ R1674 — an option chip's label stays inside the outline the chip
    /// strokes. The crate gate ([`crate::frame_gate`]).
    ///
    /// Both selection states, because they are not the same shape: an
    /// unselected chip is the one that HAS an outline
    /// (`selection_border` drops it when selected), so a gate that only ran
    /// the selected arm would never see a border at all.
    #[test]
    fn r1674_a_chip_keeps_its_label_inside_its_outline() {
        let theme = Theme::light();
        for selected in [false, true] {
            crate::frame_gate::assert_frame_contained(
                &format!("option chip selected={selected}"),
                &mut |w, _h| {
                    option_chip(
                        "chip#0".to_owned(),
                        "Transport",
                        selected,
                        true,
                        w.min(140),
                        ButtonState::Idle,
                        &theme,
                    )
                },
            );
        }
    }
}
