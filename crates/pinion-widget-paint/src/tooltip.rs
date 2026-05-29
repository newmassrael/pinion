//! R695 §5.16 §5.40 §5.50 — backend-agnostic `Tooltip` paint
//! composition + anchored positioning.
//!
//! Phase B widget-catalog entry — the contextual "what does this do?"
//! popup every pro DCC / IDE / CAD tool attaches to its icon buttons,
//! a direct step toward the northern-star "Unreal-class editor
//! self-hosted in pinion".
//!
//! ## Chrome, not behaviour
//!
//! This module owns only the *paint* axis: an elevated single-line
//! surface positioned against an anchor rect. The show / hide
//! *behaviour* (hover / focus trigger, WCAG 1.4.13 dismiss) lives in
//! [`pinion_core::widgets::tooltip::TooltipExternal`]; the binding reads
//! that external's `visible` slot and calls [`view_tooltip`] only while
//! it is set.
//!
//! ## Anchored positioning (inline first consumer)
//!
//! [`anchor_position`] resolves the overlay's absolute `(left, top)`
//! from the anchor rect, the tooltip size, the viewport, and a preferred
//! [`TooltipSide`], **flipping to the opposite side** when the preferred
//! side would overflow the viewport and **clamping the cross-axis** so
//! the tooltip never paints off-screen. This is the framework's first
//! anchor-relative-with-flip positioner; it stays inline here (not a
//! shared substrate) until a 2nd overlay consumer — a `Menu` dropdown
//! that flips, a `Popover`, a `ComboBox` list — needs the same math
//! (`[[abstraction-needs-second-consumer]]`; the R691 `Menu` dropdown
//! does *not* flip, so it is not yet that consumer).
//!
//! ## WCAG 1.4.13 "hoverable" — the shared-tag contract
//!
//! [`view_tooltip`] tags the overlay container with the **anchor's own
//! tag** (`tag` argument) and positions it *contiguous* with the anchor
//! edge (gap 0). Because the input router resolves a hover target by the
//! deepest paint tag under the cursor, the cursor crossing from the
//! trigger onto the tooltip body sees **no hover-target transition** —
//! the [`TooltipExternal`](pinion_core::widgets::tooltip::TooltipExternal)
//! stays `hovered`, so the tooltip does not vanish while the pointer
//! rests on it (the SC 1.4.13 hoverable obligation). A visible gap with
//! a transparent hit-bridge is a future axis if M3's 4 dp tooltip
//! offset is wanted without losing contiguity.

use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::Scene;

/// R695 §5.50 — the preferred side the tooltip opens toward, relative to
/// its anchor. [`anchor_position`] flips to the opposite side when the
/// preferred one would overflow the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TooltipSide {
    /// Open below the anchor (top of the tooltip touches the anchor's
    /// bottom edge). The common default — flips to [`Self::Above`] when
    /// the anchor sits too low for the tooltip to fit beneath it.
    Below,
    /// Open above the anchor (bottom of the tooltip touches the anchor's
    /// top edge). Flips to [`Self::Below`] when the anchor sits too high.
    Above,
}

/// R695 §5.50 — Material 3 plain-tooltip dimensions. Mirrors the
/// [`crate::dialog::DialogStyle`] / [`crate::menu::MenuStyle`] carrier
/// pattern so the widget catalog presents a uniform `Style` surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooltipStyle {
    /// Corner radius (M3 plain tooltip ≈ 4 px — a tight, low-emphasis
    /// container distinct from the dialog's 28 px).
    pub corner_radius: u32,
    /// Horizontal inner padding (each side).
    pub h_padding: u32,
    /// Vertical inner padding (each side).
    pub v_padding: u32,
    /// Label font size (M3 `body-small` ≈ 12 px).
    pub font_px: u32,
    /// Outline stroke width (1 px hairline around the elevated surface).
    pub border_width: u32,
}

impl TooltipStyle {
    /// R695 §5.50 — Material 3 plain-tooltip defaults. See the struct
    /// docs for the per-field token anchors.
    #[must_use]
    pub const fn m3_default() -> Self {
        Self {
            corner_radius: 4,
            h_padding: 10,
            v_padding: 6,
            font_px: 12,
            border_width: 1,
        }
    }

    /// Single-line tooltip height for [`Self::font_px`] + vertical
    /// padding. The line box is approximated as `font_px * 4 / 3` (the
    /// ~1.33 M3 body line-height) plus both `v_padding` edges, so the
    /// box fits the label without clipping descenders.
    #[must_use]
    pub const fn single_line_height(self) -> u32 {
        self.font_px * 4 / 3 + self.v_padding * 2
    }
}

impl Default for TooltipStyle {
    fn default() -> Self {
        Self::m3_default()
    }
}

/// R695 §5.50 — the geometry inputs that place a tooltip against its
/// trigger. Bundled (the [`crate::dialog::DialogContent`] arg-reduction
/// convention) so [`view_tooltip`] stays within the 7-argument view-fn
/// limit and the related fields travel together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooltipPlacement {
    /// The trigger's laid-out rect (root-content coordinates).
    pub anchor: Rect,
    /// The tooltip box `(width, height)`.
    pub tip_size: (u32, u32),
    /// The preferred side; flips on viewport overflow.
    pub side: TooltipSide,
}

/// R695 §5.50 — resolve the tooltip overlay's absolute `(left, top)`
/// (in the root container's content coordinate space, the same space the
/// anchor rect is expressed in).
///
/// Vertical: the preferred [`TooltipSide`] places the tooltip flush
/// against the anchor edge (gap 0, for the shared-tag hoverable
/// contract). If that side would push the tooltip past the viewport
/// edge, it **flips** to the opposite side. Horizontal: the tooltip is
/// left-aligned to the anchor, then **clamped** so it never paints off
/// the left/right viewport edges.
///
/// `viewport` is the window `(width, height)`.
#[must_use]
pub fn anchor_position(placement: &TooltipPlacement, viewport: (u32, u32)) -> (u32, u32) {
    let TooltipPlacement {
        anchor,
        tip_size,
        side,
    } = *placement;
    let (tip_w, tip_h) = tip_size;
    let (win_w, win_h) = viewport;

    // Vertical placement, flush against the anchor edge (gap 0).
    let below_top = anchor.y + anchor.h;
    let above_top = anchor.y.saturating_sub(tip_h);
    let fits_below = below_top.saturating_add(tip_h) <= win_h;
    let fits_above = anchor.y >= tip_h;
    let top = match side {
        // Prefer below; flip up only if it overflows *and* up fits.
        TooltipSide::Below => {
            if fits_below || !fits_above {
                below_top
            } else {
                above_top
            }
        }
        // Prefer above; flip down only if it overflows *and* down fits.
        TooltipSide::Above => {
            if fits_above || !fits_below {
                above_top
            } else {
                below_top
            }
        }
    };

    // Horizontal: anchor-left aligned, clamped into [0, win_w - tip_w].
    let max_left = win_w.saturating_sub(tip_w);
    let left = anchor.x.min(max_left);

    (left, top)
}

/// R695 §5.16 §5.50 — compose a single-line tooltip overlay positioned
/// against `anchor`.
///
/// # Arguments
///
/// - `tag` — the overlay container tag. **Must equal the anchor's paint
///   tag** so the hover router treats the trigger + tooltip body as one
///   contiguous region (the WCAG 1.4.13 hoverable contract, see module
///   docs).
/// - `text` — the descriptive label.
/// - `placement` — the [`TooltipPlacement`] geometry triple (anchor
///   rect, tooltip size, preferred side); the conventional `tip_size`
///   height is [`TooltipStyle::single_line_height`].
/// - `viewport` — `(width, height)` window dimensions for the flip /
///   clamp math.
/// - `theme` / `style` — palette + [`TooltipStyle`] carrier.
///
/// # Returns
///
/// A [`Scene::Container`] tagged `tag`, absolutely positioned at
/// [`anchor_position`]. Place it **last** in the root child list so it
/// paints over (and hit-tests above) the rest of the scene.
#[must_use]
pub fn view_tooltip(
    tag: impl Into<String>,
    text: &str,
    placement: &TooltipPlacement,
    viewport: (u32, u32),
    theme: &Theme,
    style: &TooltipStyle,
) -> Scene {
    let (tip_w, tip_h) = placement.tip_size;
    let (left, top) = anchor_position(placement, viewport);
    let label = Scene::Text(TextNode::styled(
        text,
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.font_px)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(tag.into())
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest))
                    .with_corner_radius(style.corner_radius)
                    .with_border(Border::new(
                        theme.resolve(ColorRole::Outline),
                        style.border_width,
                    )),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_absolute_position(left, top)
                    .with_size(Size::px(tip_w, tip_h))
                    .with_padding(Rect::new(
                        style.h_padding,
                        style.v_padding,
                        style.h_padding,
                        style.v_padding,
                    )),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::light()
    }

    fn place(anchor: Rect, tip_size: (u32, u32), side: TooltipSide) -> TooltipPlacement {
        TooltipPlacement {
            anchor,
            tip_size,
            side,
        }
    }

    // ----- anchor_position: side + flip + clamp -----

    #[test]
    fn r695_below_places_flush_under_anchor() {
        // Anchor at (100, 40) size 80x30; tooltip 120x24; roomy window.
        let pos = anchor_position(
            &place(Rect::new(100, 40, 80, 30), (120, 24), TooltipSide::Below),
            (600, 400),
        );
        // top = anchor.y + anchor.h = 70 (flush, gap 0); left = anchor.x.
        assert_eq!(pos, (100, 70));
    }

    #[test]
    fn r695_above_places_flush_over_anchor() {
        let pos = anchor_position(
            &place(Rect::new(100, 200, 80, 30), (120, 24), TooltipSide::Above),
            (600, 400),
        );
        // top = anchor.y - tip_h = 176 (flush); bottom touches anchor.y.
        assert_eq!(pos, (100, 176));
        assert_eq!(176 + 24, 200, "tooltip bottom touches the anchor top");
    }

    #[test]
    fn r695_below_flips_above_on_bottom_overflow() {
        // Anchor low in the window: below would overflow, so flip up.
        // Anchor (10, 370) 80x24 in a 400-tall window; tip 100x24.
        // below_top = 394, +24 = 418 > 400 -> overflow -> flip above.
        let pos = anchor_position(
            &place(Rect::new(10, 370, 80, 24), (100, 24), TooltipSide::Below),
            (600, 400),
        );
        assert_eq!(pos, (10, 346), "flipped above: 370 - 24 = 346");
    }

    #[test]
    fn r695_above_flips_below_on_top_overflow() {
        // Anchor near the top: above would overflow (anchor.y < tip_h),
        // so flip down.
        let pos = anchor_position(
            &place(Rect::new(10, 10, 80, 24), (100, 24), TooltipSide::Above),
            (600, 400),
        );
        assert_eq!(pos, (10, 34), "flipped below: 10 + 24 = 34");
    }

    #[test]
    fn r695_no_flip_when_neither_side_fits_keeps_preferred() {
        // Degenerate: a window shorter than the tooltip. Preferred side
        // is kept (a clamp-only fallback) rather than oscillating.
        let pos = anchor_position(
            &place(Rect::new(0, 0, 10, 10), (50, 500), TooltipSide::Below),
            (600, 100),
        );
        assert_eq!(pos.1, 10, "below kept when neither side fits");
    }

    #[test]
    fn r695_horizontal_clamps_into_right_edge() {
        // Anchor near the right edge: left would push the tooltip off
        // the right, so clamp to win_w - tip_w.
        let pos = anchor_position(
            &place(Rect::new(560, 40, 30, 24), (120, 24), TooltipSide::Below),
            (600, 400),
        );
        assert_eq!(pos.0, 480, "clamped: 600 - 120 = 480");
    }

    #[test]
    fn r695_horizontal_no_clamp_when_fits() {
        let pos = anchor_position(
            &place(Rect::new(100, 40, 30, 24), (120, 24), TooltipSide::Below),
            (600, 400),
        );
        assert_eq!(pos.0, 100, "left aligned to anchor when it fits");
    }

    // ----- style -----

    #[test]
    fn r695_style_m3_defaults() {
        let s = TooltipStyle::m3_default();
        assert_eq!(s.corner_radius, 4);
        assert_eq!(s.h_padding, 10);
        assert_eq!(s.v_padding, 6);
        assert_eq!(s.font_px, 12);
        assert_eq!(s.border_width, 1);
    }

    #[test]
    fn r695_single_line_height_accounts_for_padding() {
        let s = TooltipStyle::m3_default();
        // 12 * 4/3 = 16; + 6*2 = 28.
        assert_eq!(s.single_line_height(), 28);
    }

    // ----- view_tooltip composition -----

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

    #[test]
    fn r695_view_tooltip_tags_overlay_with_anchor_tag_for_hoverable() {
        // The shared-tag contract: the overlay carries the anchor tag so
        // the hover router never transitions when the cursor crosses
        // from trigger to tooltip body.
        let scene = view_tooltip(
            "save_tip",
            "Saves the file",
            &place(Rect::new(40, 40, 80, 30), (140, 28), TooltipSide::Below),
            (520, 360),
            &theme(),
            &TooltipStyle::m3_default(),
        );
        let overlay = find_container(&scene, "save_tip").expect("overlay tagged with anchor tag");
        assert_eq!(overlay.layout.absolute_position, Some((40, 70)));
        assert_eq!(overlay.layout.size, Size::px(140, 28));
    }

    #[test]
    fn r695_view_tooltip_is_elevated_outlined_surface() {
        let t = theme();
        let scene = view_tooltip(
            "tip",
            "Help",
            &place(Rect::new(40, 40, 80, 30), (60, 28), TooltipSide::Below),
            (520, 360),
            &t,
            &TooltipStyle::m3_default(),
        );
        let overlay = find_container(&scene, "tip").expect("overlay node");
        assert_eq!(overlay.style.fill, t.resolve(ColorRole::SurfaceContainerHighest));
        assert_eq!(overlay.style.corner_radius, 4);
        let border = overlay.style.border.expect("outlined surface");
        assert_eq!(border.color, t.resolve(ColorRole::Outline));
        assert_eq!(border.width, 1);
    }

    #[test]
    fn r695_view_tooltip_carries_label_text() {
        let scene = view_tooltip(
            "tip",
            "Saves the file",
            &place(Rect::new(40, 40, 80, 30), (140, 28), TooltipSide::Below),
            (520, 360),
            &theme(),
            &TooltipStyle::m3_default(),
        );
        let overlay = find_container(&scene, "tip").expect("overlay node");
        let Scene::Text(label) = &overlay.children[0] else {
            panic!("tooltip carries a label text node");
        };
        assert_eq!(label.content, "Saves the file");
    }
}
