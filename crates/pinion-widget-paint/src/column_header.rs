//! R1506 R1510 §5.16 §5.27 §5.36 — header view-style column-header section
//! paint.
//!
//! # Why this is a crate and not a binding's private fn
//!
//! R1505 added the pixel guard that proves a header label's declared
//! [`TextAlign`] reaches its glyphs, and closed honestly: the guard rendered a
//! **reconstruction** of the label node, because the node was built inside
//! `hello-column-reorder` and `pinion-shell` cannot depend on an example. The
//! composition held only because a demo separately asserted the real node's
//! fields — so a binding that changed its label's box or overflow would have
//! left the guard measuring a shape production no longer paints, and passing.
//!
//! A guard that renders a copy of the thing under test is testing the copy.
//! The fix is the one R706 already established for `view_datepicker`: the paint
//! lives in this crate, the binding calls it, and the guard calls the same
//! function. There is then no second shape to drift.
//!
//! # What belongs here and what stays the binding's
//!
//! The geometry of a header section — how a section's width becomes a label
//! box, where the sort glyph sits, which parts are decoration — is
//! header view knowledge and is here. WHAT the sections are (the
//! permutation, the sizes, the labels, which column sorts) is the
//! [`ColumnLayout`](pinion_core::widgets::column_layout::ColumnLayout)
//! external's, and arrives as [`SectionPlacement`] + [`HeaderSection`].
//! This module holds no state and reads no external.

use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{
    BoxStyle, FontWeight, LayoutStyle, Size, TextAlign, TextOverflow, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::voice::Silence;
use pinion_core::widgets::column_layout::{SectionPlacement, SectionSelection};

/// Geometry + type scale for a header strip, in logical pixels.
///
/// The defaults are `hello-column-reorder`'s, which is where every one of
/// these numbers was measured against the toolkit before this module existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnHeaderStyle {
    /// Strip height, and therefore each section's.
    pub height: u32,
    /// Painted gap between neighbouring sections: a section of size `n`
    /// paints `n - section_gap` wide, leaving the strip fill as a seam.
    pub section_gap: u32,
    /// Horizontal inset from the section's edge to the label box, applied on
    /// BOTH sides. Symmetric on purpose — an alignment inside an asymmetric
    /// box is not the alignment the caller asked for.
    pub label_inset: u32,
    /// Vertical inset from the section's top to the label box, applied top and
    /// bottom (so the box is `height - 2 * label_y` tall).
    pub label_y: u32,
    /// Width reserved at the section's trailing end for the sort glyph, when
    /// one is shown.
    pub glyph_w: u32,
    /// Label font size.
    pub text_px: u32,
}

impl ColumnHeaderStyle {
    /// The measured defaults (see the struct docs).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            height: 40,
            section_gap: 2,
            label_inset: 12,
            label_y: 12,
            glyph_w: 24,
            text_px: 13,
        }
    }

    /// The width a section of logical size `size` actually paints.
    #[must_use]
    pub const fn section_width(&self, size: u32) -> u32 {
        size.saturating_sub(self.section_gap)
    }

    /// The width of the box a section's label is aligned WITHIN.
    ///
    /// This is the number the whole alignment feature rests on. `paint_text` hands a
    /// text leaf's own `rect.w` to the shaper as the width to align in, so a label
    /// pinned to its glyphs renders identically under every [`TextAlign`] — the
    /// alignment would be declared and unobservable. The box therefore spans
    /// the section less the insets, and less the glyph's reserved end when a
    /// sort indicator is showing (the toolkit reserves the same room; a
    /// centred label over an unreserved arrow reads as a collision the moment
    /// a column is sorted).
    ///
    /// Floored at 1: a zero-width text rect makes `paint_text` pass `None` for
    /// the width, which silently turns alignment back off.
    #[must_use]
    pub const fn label_box_width(&self, size: u32, has_sort_glyph: bool) -> u32 {
        let reserved = if has_sort_glyph {
            self.label_inset * 2 + self.glyph_w
        } else {
            self.label_inset * 2
        };
        let w = self.section_width(size).saturating_sub(reserved);
        if w == 0 { 1 } else { w }
    }

    /// The height of that box.
    #[must_use]
    pub const fn label_box_height(&self) -> u32 {
        self.height.saturating_sub(self.label_y * 2)
    }
}

impl Default for ColumnHeaderStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// One section's content and interaction state, as the header's model sees it.
#[derive(Debug, Clone, Copy)]
pub struct HeaderSection<'a> {
    /// The label text.
    pub label: &'a str,
    /// Where the label sits inside its box — the header's own rule, or the
    /// model's per-section exception. Resolved by the caller, because the
    /// toolkit keeps the two in different places and only the caller knows
    /// both.
    pub align: TextAlign,
    /// The sort indicator's glyph, already chosen (see
    /// [`sort_glyph`](crate::glyph::sort_glyph)), or `None` when this section
    /// is not the sorted one.
    pub sort_glyph: Option<&'a str>,
    /// This section is the one being dragged.
    pub dragged: bool,
    /// This section is the keyboard-focused one.
    pub focused: bool,
    /// R1510 — how much of the section the selection covers, **already gated**
    /// on the header's `highlightSections` rule
    /// ([`ColumnLayout::section_highlight`](pinion_core::widgets::column_layout::ColumnLayout::section_highlight)).
    ///
    /// The effective value, not the published one, for the same reason
    /// [`align`](Self::align) is resolved by the caller: two of the three
    /// inputs live somewhere this module cannot see, and a paint function that
    /// took the raw selection would have to be told the rule as well and could
    /// then be told a contradictory pair.
    pub selection: SectionSelection,
}

/// The label's own text style.
fn label_style(section: &HeaderSection, style: &ColumnHeaderStyle, theme: &Theme) -> TextStyle {
    // R1510 — the two the toolkit flags reach two properties of the LABEL, and
    // neither touches the box's fill.
    //
    // Weight is `State_On` (the selection intersects the section) and colour is `State_Sunken`
    // (it covers the whole of it). The toolkit's own mapping for the second is
    // a sunken bevel, which this theme does not have; the accent is this
    // theme's resolution, stated here rather than implied.
    //
    // WHY NOT THE FILL, and why not a border. The fill is already spoken for by
    // the drag and the keyboard cursor ([`view_header_cell`]), and a selected
    // section that swallowed the focus colour would hide the cursor exactly
    // where a user is working. A border on the section box was the first
    // choice and was measured to not paint at all: the runtime's
    // `Scene::Container` arm fills and shadows but never strokes
    // `BoxStyle::border` (only `Scene::Box` does), so 45 container-attached
    // border declarations across this workspace are silently dropped today.
    // That is a real defect and a round of its own — fixing it turns on borders
    // in ~30 files at once — and declaring one here meanwhile would have
    // shipped a rule that reaches no pixels.
    let role = if section.dragged {
        // The drag wins: it is the transient gesture, and its muted label is
        // what tells a user which section is in flight.
        ColorRole::OnSurfaceMuted
    } else if section.selection.covers() {
        ColorRole::Accent
    } else {
        ColorRole::OnSurface
    };
    let weight = if section.selection.intersects() {
        FontWeight::BOLD
    } else {
        FontWeight::NORMAL
    };
    TextStyle::new()
        .with_size_px(style.text_px)
        .with_fg(theme.resolve(role))
        .with_weight(weight)
        .with_align(section.align)
        // The box is the containment: without this a label wider than its
        // section paints over the neighbour, and under `Center` it would do so
        // at BOTH ends. The chart's `label_node` reaches for the same pair for
        // the same reason.
        .with_overflow(TextOverflow::Clip)
}

/// R1506 §5.36 — the label leaf of one header section, tagged
/// `<tag_prefix>_label#<visual>`.
///
/// Exposed separately from [`view_header_cell`] because it is the unit the
/// R1505 pixel guard renders: the declaration under test is entirely in this
/// node's style and box, and a guard that built its own would be testing its
/// own arithmetic.
///
/// The leaf is `pointer_transparent` (R1499): the toolkit's
/// `WA_TransparentForMouseEvents` / CSS's `pointer-events: none`. It is tagged
/// for snapshot assertions and the a11y walk, and nothing dispatches to it, so
/// a press landing on it must reach the section underneath. Since R1504 gave
/// the label a box that spans its section, a centred label covers the point
/// `scene/click` presses in EVERY section rather than in whichever ones the
/// string happened to be wide enough for — which is what makes this
/// declaration load-bearing rather than incidental.
///
/// ★★★★★ R1856 — the leaf declares **why it is quiet**, and the declaration is
/// derived from the tag it is a leaf of.
///
/// The section this label sits in is announced as a column heading whose NAME
/// is this very string, so a reader who was also stopped on the label would be
/// told the same word twice with no way to know it was one heading. That is a
/// [`SilenceKind::NameOf`](pinion_core::voice::SilenceKind::NameOf) — the one
/// arm whose detail is a tag, so the census can check that the peer it points
/// at speaks rather than take the claim on trust.
///
/// It is declared HERE and not by the caller because this function is what
/// knows the answer: it composes the label's tag out of `tag_prefix` and
/// `visual`, so it can compose the section's out of the same two and cannot
/// name a peer the caller did not paint. Before this round the leaf was painted
/// with no node and no reason, which is the one outcome a voice census calls
/// *undecided* — and both of this workspace's consumers of this composite
/// carried it, which is what makes it the composite's defect and not a screen's.
#[must_use]
pub fn header_label_node(
    tag_prefix: &str,
    visual: usize,
    section: &HeaderSection,
    placement_size: u32,
    style: &ColumnHeaderStyle,
    theme: &Theme,
) -> Scene {
    let w = style.label_box_width(placement_size, section.sort_glyph.is_some());
    Scene::Text(
        TextNode::styled(
            section.label,
            Rect::default(),
            label_style(section, style, theme),
        )
        .with_tag(format!("{tag_prefix}_label#{visual}"))
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(style.label_inset, style.label_y)
                .with_size(Size::px(w, style.label_box_height()))
                .with_pointer_transparent(true),
        ),
    )
    .silenced(Silence::name_of(format!("{tag_prefix}#{visual}")))
}

/// R1506 §5.16 — one header section cell, tagged `<tag_prefix>#<visual>` so a
/// router's `'#'` split reaches the composite external and a drop
/// classification sees a real subindex.
#[must_use]
pub fn view_header_cell(
    tag_prefix: &str,
    placement: &SectionPlacement,
    section: &HeaderSection,
    style: &ColumnHeaderStyle,
    theme: &Theme,
) -> Scene {
    let visual = placement.visual;
    let fill = if section.dragged {
        theme.resolve(ColorRole::SurfaceContainerLow)
    } else if section.focused {
        theme.resolve(ColorRole::SurfaceContainerHighest)
    } else {
        theme.resolve(ColorRole::SurfaceContainerHigh)
    };
    let sect_w = style.section_width(placement.size);

    let mut children = vec![header_label_node(
        tag_prefix,
        visual,
        section,
        placement.size,
        style,
        theme,
    )];
    if let Some(glyph) = section.sort_glyph {
        // ★★★★★ R1851 — the glyph gets a BOX, and the box is the line its own
        // face needs.
        //
        // It had a position and no size, so the layout pass resolved it to the
        // run's intrinsic height — which is smaller than the line box the face
        // requires, so the indicator's own descender was authored into a box too
        // short for it. Measured at R1851 by a per-card zero gate on the analyzer
        // shell: `"▼" at 11px in a 16px box needs 18`, in every state and at
        // every size. The workspace's ink census had it inside a screen-wide
        // ratchet, where one run of one widget is under the noise.
        //
        // Sized from [`pinion_core::containment::line_box`] rather than from a
        // number here, and WIDTH as well as height, because a run with a
        // zero-width box makes no promise about holding anything and is skipped
        // by the census that would otherwise catch the next slip.
        let box_h = pinion_core::containment::line_box(style.text_px);
        children.push(
            Scene::Text(
                TextNode::styled(
                    glyph,
                    Rect::default(),
                    TextStyle::new()
                        .with_size_px(style.text_px)
                        .with_fg(theme.resolve(ColorRole::Accent)),
                )
                .with_tag(format!("{tag_prefix}_sort#{visual}"))
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(sect_w.saturating_sub(style.glyph_w), style.label_y)
                        .with_size(Size::px(style.glyph_w, box_h)),
                ),
            )
            // ★★★★★ R1856 — ornament, and the claim is CHECKED elsewhere.
            //
            // An arrow is not the fact; the direction is, and a heading node
            // carries it as `aria-sort`. Declaring this leaf decorative is
            // therefore true only while that stays so — which is why this module's
            // own zero gate asserts a sorted heading announces its direction.
            // Without that pairing this declaration would be the worse defect: an
            // undecided region reads as unfinished, while a wrong `decorative`
            // reads as handled and silently drops the one fact a reader scanning a
            // sorted feed needs.
            .silenced(Silence::decorative(
                "the sort arrow; the heading it sits in announces the direction",
            )),
        );
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("{tag_prefix}#{visual}"))
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(placement.x, 0)
                    .with_size(Size::px(sect_w, style.height)),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::style::SizeValue;

    fn placement(size: u32) -> SectionPlacement {
        SectionPlacement {
            visual: 0,
            logical: 0,
            x: 0,
            size,
        }
    }

    fn section(align: TextAlign, glyph: Option<&'static str>) -> HeaderSection<'static> {
        HeaderSection {
            label: "Modified",
            align,
            sort_glyph: glyph,
            dragged: false,
            focused: false,
            selection: SectionSelection::Unselected,
        }
    }

    /// The arithmetic the alignment feature rests on: the box is the section
    /// less symmetric insets, and less the glyph's end when one shows.
    #[test]
    fn label_box_spans_the_section_less_its_insets() {
        let s = ColumnHeaderStyle::new();
        assert_eq!(s.section_width(150), 148);
        assert_eq!(s.label_box_width(150, false), 124);
        assert_eq!(s.label_box_width(150, true), 100);
        assert_eq!(s.label_box_height(), 16);
    }

    /// A section too narrow to hold its insets still yields a box, because a
    /// zero-width text rect turns alignment off rather than clipping it.
    #[test]
    fn label_box_never_collapses_to_zero() {
        let s = ColumnHeaderStyle::new();
        for size in 0..=(s.label_inset * 2 + s.section_gap) {
            assert!(
                s.label_box_width(size, false) >= 1,
                "size={size} must still yield a box",
            );
        }
        assert!(s.label_box_width(0, true) >= 1);
    }

    /// The declaration reaches the node: whatever the caller resolved is the
    /// leaf's own `text_align`, and the leaf is decoration.
    #[test]
    fn the_label_leaf_carries_the_declaration() {
        let theme = Theme::light();
        let style = ColumnHeaderStyle::new();
        for align in [
            TextAlign::Start,
            TextAlign::Center,
            TextAlign::End,
            TextAlign::Justify,
        ] {
            let Scene::Text(t) =
                header_label_node("colhdr", 2, &section(align, None), 150, &style, &theme)
            else {
                panic!("the label is a Text leaf");
            };
            assert_eq!(t.style.text_align, align);
            assert_eq!(t.style.overflow, TextOverflow::Clip);
            assert_eq!(t.tag.as_deref(), Some("colhdr_label#2"));
            assert!(
                t.layout.pointer_transparent,
                "the label is decoration and says so (R1499)",
            );
            assert_eq!(t.layout.size, Size::px(124, 16));
        }
    }

    /// R1510 — the two levels reach two different channels, and each reaches
    /// only its own: weight tracks the toolkit's `State_On` (the selection
    /// intersects), the border tracks `State_Sunken` (it covers).
    #[test]
    fn the_two_highlight_levels_use_two_channels() {
        let theme = Theme::light();
        let style = ColumnHeaderStyle::new();
        let expected = [
            (
                SectionSelection::Unselected,
                FontWeight::NORMAL,
                ColorRole::OnSurface,
            ),
            (
                SectionSelection::Partial,
                FontWeight::BOLD,
                ColorRole::OnSurface,
            ),
            (SectionSelection::Full, FontWeight::BOLD, ColorRole::Accent),
        ];
        for (selection, weight, role) in expected {
            let mut s = section(TextAlign::Center, None);
            s.selection = selection;
            let Scene::Container(cell) =
                view_header_cell("colhdr", &placement(150), &s, &style, &theme)
            else {
                panic!("the cell is a Container");
            };
            let Scene::Text(label) = &cell.children[0] else {
                panic!("the label comes first");
            };
            assert_eq!(
                label.style.font_weight, weight,
                "{selection} bolds the label iff the selection intersects",
            );
            assert_eq!(
                label.style.fg_color,
                theme.resolve(role),
                "{selection} accents the label iff the selection covers it",
            );
        }
    }

    /// R1510 — the drag outranks the highlight for the label's colour, because a
    /// drag is the gesture in flight and its muted label is what names the
    /// section being moved. The weight still reports the selection.
    #[test]
    fn a_dragged_section_keeps_its_muted_label_while_highlighted() {
        let theme = Theme::light();
        let style = ColumnHeaderStyle::new();
        let mut s = section(TextAlign::Center, None);
        s.dragged = true;
        s.selection = SectionSelection::Full;
        let Scene::Text(label) = header_label_node("colhdr", 0, &s, 150, &style, &theme) else {
            panic!("the label is a Text leaf");
        };
        assert_eq!(
            label.style.fg_color,
            theme.resolve(ColorRole::OnSurfaceMuted)
        );
        assert_eq!(
            label.style.font_weight,
            FontWeight::BOLD,
            "the weight is the other channel and still answers the selection",
        );
    }

    /// R1510 — the highlight leaves the fill alone, so a fully-selected section
    /// still shows the keyboard cursor. The reason the levels were mapped to
    /// weight and border rather than to the fill both of these already own.
    #[test]
    fn highlighting_does_not_take_the_fill_from_focus_or_drag() {
        let theme = Theme::light();
        let style = ColumnHeaderStyle::new();
        let fill_of = |dragged: bool, focused: bool, selection: SectionSelection| {
            let mut s = section(TextAlign::Center, None);
            s.dragged = dragged;
            s.focused = focused;
            s.selection = selection;
            let Scene::Container(cell) =
                view_header_cell("colhdr", &placement(150), &s, &style, &theme)
            else {
                panic!("the cell is a Container");
            };
            cell.style.fill
        };
        for state in [(false, false), (false, true), (true, false), (true, true)] {
            let (dragged, focused) = state;
            let plain = fill_of(dragged, focused, SectionSelection::Unselected);
            for selection in [SectionSelection::Partial, SectionSelection::Full] {
                assert_eq!(
                    fill_of(dragged, focused, selection),
                    plain,
                    "dragged={dragged} focused={focused}: {selection} must not \
                     move the fill those two decide",
                );
            }
        }
        assert_ne!(
            fill_of(false, true, SectionSelection::Full),
            fill_of(false, false, SectionSelection::Full),
            "and the cursor is still visible on a fully-selected section",
        );
    }

    /// The sort glyph takes the section's trailing end, and the label yields
    /// exactly that much — the two must not overlap.
    #[test]
    fn the_sort_glyph_and_the_label_do_not_overlap() {
        let theme = Theme::light();
        let style = ColumnHeaderStyle::new();
        let cell = view_header_cell(
            "colhdr",
            &placement(150),
            &section(TextAlign::Center, Some("\u{25b2}")),
            &style,
            &theme,
        );
        let Scene::Container(c) = cell else {
            panic!("the cell is a Container");
        };
        assert_eq!(c.children.len(), 2, "label + glyph");
        let Scene::Text(label) = &c.children[0] else {
            panic!("the label comes first");
        };
        let Scene::Text(glyph) = &c.children[1] else {
            panic!("the glyph comes second");
        };
        let SizeValue::Px(label_w) = label.layout.size.width else {
            panic!("the label box is sized in px");
        };
        let label_end = style.label_inset + label_w;
        let glyph_start = glyph.layout.absolute_position.map_or(0, |(x, _)| x);
        assert!(
            label_end <= glyph_start,
            "the label box must end before the glyph starts: {label_end} vs {glyph_start}",
        );
    }
}
