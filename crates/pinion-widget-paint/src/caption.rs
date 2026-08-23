//! R1792 §5.2 §5.11 — a **box that holds its caption**: the run's rectangle is
//! derived from the box, so a caption cannot be drawn outside the rectangle a
//! reader sees around it, and where it sits inside that rectangle is
//! *declared* rather than arrived at by arithmetic.
//!
//! ## The defect this is the repair of
//!
//! A reader opened the assembled analysis tool and reported that the words in
//! the protocol chips were not centred in their rectangles. Measured through
//! the paint, it is worse than not centred:
//!
//! ```text
//! box  lab.palette.protocol.tcp = (69, 587, 36, 18)
//! run  "tcp"                    = (76, 591, 32, 12)
//!                                  ^^ +7 by hand, 32 wide in a 36 box
//!                                  => 3px hangs off the RIGHT EDGE
//! ```
//!
//! All five protocol chips do it, each by 3px. The `+7` was a hand-computed
//! offset against a box it is not inside: the caption is a **sibling** of the
//! tagged container, not a child, so nothing relates the two and no gate in
//! this tree asks. `assert_contained_ink` asks whether a mark is inside *its
//! own* box — and the caption's own box is itself, so it passes.
//!
//! Counted across the five analyzer screens at their opening size: **230**
//! caption/box pairs are drawn this way, **5** escape the box they appear in,
//! and **675 of 675** text runs declare `TextAlign::Start` — which is the
//! default, so no site anywhere has ever expressed an intention about where its
//! caption should sit. That is why this module publishes an alignment rather
//! than a fix for five chips: *off-centre* and *deliberately left* are
//! indistinguishable while nothing declares which was meant.
//!
//! ## What the floor does, built and run at 6.11
//!
//! | | the reference toolkit | here |
//! |---|---|---|
//! | a caption escaping its box | **impossible** — the widget owns its text | impossible, [`place`] derives the rect |
//! | alignment declared and readable | **yes**, round-trips | yes, [`Caption::align`] |
//! | a caption that does not fit | drawn anyway, **no report** | [`Fit::Overflows`], by name |
//! | asking where the glyphs LANDED | **no member answers it** — the policy and the box are readable, the result is not; the style can recompute it from an align flag *the caller passes in*, which is an assumption rather than a read | [`Placed::run`], the rectangle the run is actually built with |
//!
//! So two of the four are parity and two are this module's, and both of the
//! latter are the introspection axis this framework exists for: a screen that
//! cannot say where its caption landed cannot be asked whether it is right.
//!
//! ## Why not the flex idiom
//!
//! [`crate::chip`] centres a caption with a flex row, and where a caller is
//! already laying out with flex that remains the better answer. The analyzer
//! screens are not: they compute every rectangle themselves and paint at
//! absolute positions, which is what makes a hand-written `+7` possible at all.
//! This module meets that style rather than asking four screens to change how
//! they lay out before they can stop drawing outside their own boxes.

use pinion_core::Scene;
use pinion_core::measured_text_extent;
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::style::{BoxStyle, LayoutStyle, Size, TextAlign, TextStyle};
use pinion_core::voice::Silence;

use crate::run::text_run;

/// Where a caption sits along one axis of the box that holds it.
///
/// Deliberately the same three words as [`TextAlign`]'s first three, because a
/// caller who declares `Align::Center` here and reads `TextAlign::Center` off
/// the paint should not have to translate. `Justify` has no meaning for a
/// single-line caption and is absent rather than accepted-and-ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Against the leading edge, after the padding.
    #[default]
    Start,
    /// Centred in what the padding leaves.
    Center,
    /// Against the trailing edge, before the padding.
    End,
}

impl Align {
    /// Where a run of `run` sits in `room`, measured from the start edge.
    ///
    /// Saturating: a run wider than the room lands at the start rather than at
    /// a negative offset, which is the honest placement for a caption that does
    /// not fit — and [`Fit`] is what says so, so the caller is not left to infer
    /// it from a coordinate.
    #[must_use]
    pub const fn offset(self, room: u32, run: u32) -> u32 {
        let slack = room.saturating_sub(run);
        match self {
            Self::Start => 0,
            Self::Center => slack / 2,
            Self::End => slack,
        }
    }

    /// The text alignment a run placed this way declares.
    ///
    /// ★ The run's rectangle is derived to fit its own glyphs, so this property
    /// changes nothing about where they land — it is what makes the intention
    /// **readable**, which is the half that was missing. 675 of 675 runs in the
    /// analyzer screens declare the default; a gate cannot tell a centred
    /// caption from a left one while that is true.
    #[must_use]
    pub const fn declared(self) -> TextAlign {
        match self {
            Self::Start => TextAlign::Start,
            Self::Center => TextAlign::Center,
            Self::End => TextAlign::End,
        }
    }
}

/// Whether the caption had room, said by name rather than left in a coordinate.
///
/// The floor draws an overflowing caption and reports nothing — measured at
/// 6.11, a label whose text advances 171px in a 36px box has a `sizeHint` of
/// 173 and no error anywhere. A screen that can be asked what it painted should
/// be able to answer this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// The caption fits, with this much room left over.
    Fits {
        /// Horizontal room the caption did not use.
        spare_x: u32,
        /// Vertical room the caption did not use.
        spare_y: u32,
    },
    /// The caption is wider or taller than the box's inner region, by this
    /// much. It is still placed — dropping it would leave a reader with an
    /// empty box and no reason — and it is still clipped to the box.
    Overflows {
        /// How much wider than the inner region the caption is, or 0.
        over_x: u32,
        /// How much taller than the inner region the caption is, or 0.
        over_y: u32,
    },
}

impl Fit {
    /// Whether the caption had room.
    #[must_use]
    pub const fn fits(self) -> bool {
        matches!(self, Self::Fits { .. })
    }
}

/// A caption and how it should sit in the box that holds it.
#[derive(Debug, Clone)]
pub struct Caption {
    text: String,
    /// A size the caller states instead of one being measured — `None` for the
    /// normal case, which is that the SHAPER is asked.
    ///
    /// ★★★★★ R1794 — this used to be required, and the reason written here for
    /// requiring it was FALSE: *"this crate composes scenes and does not shape
    /// text"*. `pinion_core::measured_text_extent` shapes, is callable from
    /// anywhere, and exists precisely so a view fn can size the text it is about
    /// to paint. Asking the caller instead is what produced the defect a reader
    /// reported.
    ///
    /// Measured on the shipped screen: the node lab passed `(32, 12)` for `tcp`
    /// because 32 was the number the hand-written code before it used — and 32
    /// was a BOX, not a measurement. The glyphs advance **15**. So the run
    /// rectangle was centred in the chip and the glyphs sat `Start`-aligned at
    /// the left of that rectangle, 8.5px left of the chip's centre. The gate
    /// was green because it measured rectangles; the reader was looking at
    /// glyphs.
    stated: Option<(u32, u32)>,
    style: TextStyle,
    align_x: Align,
    align_y: Align,
    pad_x: u32,
    pad_y: u32,
    silence: Option<Silence>,
}

impl Caption {
    /// A caption of `text` in `style`, **sized by the shaper**.
    ///
    /// Starts at the writing-mode start on both axes, which is the framework
    /// default and therefore the one that changes nothing for a caller who does
    /// not say.
    ///
    /// The size is not a parameter. [`place`] asks
    /// [`pinion_core::measured_text_extent`] for it, which is the same shaper
    /// the frame paints with, so the rectangle this module centres and the
    /// glyphs a reader sees are one thing rather than two that agree by luck.
    #[must_use]
    pub fn new(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: text.into(),
            stated: None,
            style,
            align_x: Align::Start,
            align_y: Align::Start,
            pad_x: 0,
            pad_y: 0,
            silence: None,
        }
    }

    /// State the size instead of measuring it.
    ///
    /// ⚠ For a caller that genuinely knows better than the shaper — a test with
    /// no font provider, or a caption whose glyphs are drawn by something other
    /// than this text stack. **Not for a caller who has a number to hand**: the
    /// number the node lab had to hand was a box, and using it is the defect
    /// this module was rewritten to make unrepresentable.
    #[must_use]
    pub const fn stating(mut self, size: (u32, u32)) -> Self {
        self.stated = Some(size);
        self
    }

    /// Declare the caption's own region silent, and why.
    ///
    /// A caption that is a child is a NEW painted region with a tag, and this
    /// tree requires every such region to be announced or to say why it is not.
    /// Carrying the reason here rather than leaving it to the call site is the
    /// same argument the silence mechanism already makes about itself: the
    /// reason travels with the node, so deleting the paint deletes the
    /// declaration. Most captions want [`Silence::name_of`] their own box —
    /// the box already says the word.
    #[must_use]
    pub fn silent(mut self, silence: Silence) -> Self {
        self.silence = Some(silence);
        self
    }

    /// Centre it on both axes — the common case, and the one the reader asked
    /// for.
    #[must_use]
    pub fn centred(mut self) -> Self {
        self.align_x = Align::Center;
        self.align_y = Align::Center;
        self
    }

    /// Where it sits horizontally.
    #[must_use]
    pub const fn align(mut self, align: Align) -> Self {
        self.align_x = align;
        self
    }

    /// Where it sits vertically.
    #[must_use]
    pub const fn align_y(mut self, align: Align) -> Self {
        self.align_y = align;
        self
    }

    /// Room kept clear inside the box, on each side of each axis.
    #[must_use]
    pub const fn padding(mut self, pad_x: u32, pad_y: u32) -> Self {
        self.pad_x = pad_x;
        self.pad_y = pad_y;
        self
    }

    /// The text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The size the glyphs want, and where that number came from.
    ///
    /// ★★★★★ The whole repair, in one place. Asking the shaper is the normal
    /// path; a caller's stated size is an escape; and when neither is available
    /// — headless, before any provider has measured anything — the answer says
    /// **`Sized::Guessed`** rather than quietly returning a number that looks
    /// like a measurement. A silent estimate is how the defect got in.
    fn wants(&self) -> (u32, u32, Sized) {
        if let Some((w, h)) = self.stated {
            return (w, h, Sized::Stated);
        }
        if let Some(extent) = measured_text_extent(&self.text, &self.style, None) {
            return (extent.width(), extent.height(), Sized::Measured);
        }
        // The deterministic fallback the framework's own doc prescribes for a
        // headless caller, and it is REPORTED as a guess so a gate can refuse
        // to draw a conclusion from it.
        let px = self.style.font_size_px.max(1);
        let width = u32::try_from(self.text.chars().count()).unwrap_or(u32::MAX);
        (width.saturating_mul(px) / 2, px, Sized::Guessed)
    }
}

/// Where a caption's size came from.
///
/// ★★★★★ Published because the difference is the defect. A measured size makes
/// the run rectangle the ink, so centring it centres the glyphs; a guessed one
/// does not, and every alignment claim built on it is about a rectangle nobody
/// drew. A gate that cannot tell them apart is the gate that was green while a
/// reader was looking at left-aligned text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sized {
    /// The shaper answered — the frame's own text stack, so the rectangle and
    /// the glyphs are one fact.
    Measured,
    /// The caller stated it, through [`Caption::stating`].
    Stated,
    /// Nothing could measure it: no provider has shaped anything yet. The
    /// number is a deterministic estimate and is not a measurement.
    Guessed,
}

/// Where a caption landed, and whether it had room.
///
/// ★ This is the answer the floor cannot give. Its style can *recompute* a text
/// rectangle from an alignment flag the caller passes in, which is a
/// recomputation from an assumption rather than a read of what happened; here
/// the rectangle in [`Placed::run`] is the one [`captioned`] builds the node
/// with, so a caller and a gate read the same fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    run: Rect,
    fit: Fit,
    sized: Sized,
}

impl Placed {
    /// The rectangle the caption is drawn in.
    ///
    /// When [`Self::sized`] is [`Sized::Measured`] this IS the ink: the shaper
    /// said the glyphs are this wide, so a rectangle centred in a box centres
    /// the glyphs. That equality is the module's whole point and it is the one
    /// thing the first draft got wrong.
    #[must_use]
    pub const fn run(self) -> Rect {
        self.run
    }

    /// Whether it had room, and by how much either way.
    #[must_use]
    pub const fn fit(self) -> Fit {
        self.fit
    }

    /// Where the size came from — and therefore whether an alignment claim
    /// built on this placement is about glyphs or about a guess.
    #[must_use]
    pub const fn sized(self) -> Sized {
        self.sized
    }
}

/// Derive where `caption` sits inside `box_rect`, without building anything.
///
/// Separate from [`captioned`] so a gate, a test and a screen read **one**
/// derivation. The hand-written `+7` this module replaces existed because the
/// placement and the box were two independent numbers; making the placement a
/// function of the box is what makes them impossible to disagree.
///
/// The returned rectangle is in the same space as `box_rect`.
#[must_use]
pub fn place(box_rect: Rect, caption: &Caption) -> Placed {
    let room_x = box_rect.w.saturating_sub(caption.pad_x * 2);
    let room_y = box_rect.h.saturating_sub(caption.pad_y * 2);
    let (want_x, want_y, sized) = caption.wants();
    // ★ Clamped to the inner region, which is what makes an escape
    // unrepresentable rather than merely discouraged. The overflow is reported
    // instead of drawn, because a caption 3px outside its box is what a reader
    // sees and what no gate here could ask about.
    let draw_x = want_x.min(room_x);
    let draw_y = want_y.min(room_y);
    let run = Rect::new(
        box_rect.x + caption.pad_x + caption.align_x.offset(room_x, draw_x),
        box_rect.y + caption.pad_y + caption.align_y.offset(room_y, draw_y),
        draw_x,
        draw_y,
    );
    let (over_x, over_y) = (want_x.saturating_sub(room_x), want_y.saturating_sub(room_y));
    let fit = if over_x == 0 && over_y == 0 {
        Fit::Fits {
            spare_x: room_x - want_x,
            spare_y: room_y - want_y,
        }
    } else {
        Fit::Overflows { over_x, over_y }
    };
    Placed { run, fit, sized }
}

/// Whether the box itself is something a pointer resolves to.
///
/// Named rather than a bool, because both answers are ordinary and a caller
/// reading `captioned(.., true)` cannot tell which one it is asking for. A chip
/// a person clicks is a [`Pointer::Target`]; a colour key that only labels a
/// set is [`Pointer::Transparent`], and getting that backwards makes a screen
/// dead to a real mouse wherever the box is painted — which this tree's
/// pointer-transparency census reports by name, and which is how the first
/// draft of this module was caught: five decorative chips became hit targets
/// resolving to no External, in twelve screen states each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    /// A press on the box is the box's.
    Target,
    /// A press falls through to whatever is behind it.
    Transparent,
}

/// The suffix [`captioned`] gives a caption's own tag.
///
/// Published so a census can exclude it **by the framework's name for it**
/// rather than by a literal each reader spells again. A caption is part of its
/// box, not a second member of the box's family, and a count that does not know
/// that doubles: measured when this module landed, the node lab's
/// `lab.palette.protocol.` family went from 5 to 10 and its own gate said so.
pub const CAPTION_SUFFIX: &str = ".caption";

/// A tagged box **with its caption inside it**, and where the caption landed.
///
/// The run is a CHILD, which is the second half of the repair: this tree
/// attributes a painted mark to its nearest *tagged ancestor*, so a caption
/// drawn as a sibling is filed under whatever container happens to enclose both
/// — three chips' words arriving as one run of a pane, which is why two surfaces
/// of the capture viewer could not be specified at all
/// (`debt-a-word-painted-beside-its-box-is-filed-under-the-container`). A
/// caption that is a child answers to its own box.
///
/// The caption's tag is the box's, suffixed `.caption`, so a gate can ask for
/// either and neither shadows the other.
#[must_use]
pub fn captioned(
    tag: &str,
    box_rect: Rect,
    box_style: BoxStyle,
    caption: &Caption,
    pointer: Pointer,
) -> (Scene, Placed) {
    let placed = place(box_rect, caption);
    // The child's rectangle is expressed in the box's own space: an absolute
    // layout inside a container is resolved against that container.
    let inner = Rect::new(
        placed.run.x - box_rect.x,
        placed.run.y - box_rect.y,
        placed.run.w,
        placed.run.h,
    );
    let mut run = text_run(
        format!("{tag}{CAPTION_SUFFIX}"),
        caption.text.clone(),
        inner,
        caption.style.clone().with_align(caption.align_x.declared()),
    );
    if let Some(silence) = caption.silence.clone() {
        run = run.silenced(silence);
    }
    let scene = Scene::Container(
        ContainerNode::new(vec![run])
            .with_tag(tag.to_owned())
            .with_style(box_style)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(box_rect.x, box_rect.y)
                    .with_size(Size::px(box_rect.w, box_rect.h))
                    .with_pointer_transparent(pointer == Pointer::Transparent),
            ),
    );
    (scene, placed)
}

/// A caption drawn outside the box a reader sees around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escape {
    /// The box's tag.
    pub box_tag: String,
    /// What the caption says.
    pub text: String,
    /// How far past each edge it reaches, `(left, top, right, bottom)`, each 0
    /// where it does not.
    pub past: (u32, u32, u32, u32),
}

/// Every caption that is drawn outside the smallest box it sits in.
///
/// ★★★★★ The gate this tree did not have. Its paint checks ask whether a mark is
/// inside **its own** box (`assert_contained_ink`) and whether two runs overlap
/// each other; neither can see a caption drawn 3px past the edge of a rectangle
/// it is not a child of, because to those checks the caption's own box is
/// itself. So five chips shipped that way and the first person to notice was a
/// reader looking at the window.
///
/// `boxes` is tag → rectangle for every box that paints no text of its own, and
/// `runs` is every text run with its rectangle. Both are what a screen's paint
/// sweep already holds.
///
/// ## Which box a word is *in*, and how the first draft got it wrong
///
/// A run is judged against the smallest box **that contains its centre**, and
/// only when that box is of a caption's own scale.
///
/// The first draft asked only for an OVERLAP, and armed against the node lab it
/// reported **40** escapes in twelve states — every one a false positive. A word
/// clipping the corner of a neighbouring small box was assigned to that box and
/// then reported as hanging 43px off its left edge. Overlap is a relation
/// between rectangles; *being in a box* is a judgment a person makes about where
/// the word sits, and the centre is that judgment written down. With it the same
/// sweep reports the five chips a reader actually saw and nothing else.
///
/// The scale rule is the other half: a word overlapping a whole pane is on a
/// pane rather than in a box, and 288 of the capture viewer's runs sit in a
/// container 1440 wide.
///
/// The floor cannot answer this at all: measured at 6.11, a widget owns its text
/// so an escape is impossible there, but nothing exposes where the glyphs landed
/// — the style can recompute a rectangle from an alignment flag the caller
/// passes in, which is an assumption rather than a read. A screen built this way
/// can be asked.
#[must_use]
pub fn escapes(boxes: &[(String, Rect)], runs: &[(String, Rect)]) -> Vec<Escape> {
    let mut out = Vec::new();
    for (text, run) in runs {
        let (cx, cy) = (run.x + run.w / 2, run.y + run.h / 2);
        let holder = boxes
            .iter()
            .filter(|(_, b)| cx >= b.x && cx < b.x + b.w && cy >= b.y && cy < b.y + b.h)
            .filter(|(_, b)| b.w <= run.w.saturating_mul(4))
            .min_by_key(|(_, b)| u64::from(b.w) * u64::from(b.h));
        let Some((tag, b)) = holder else { continue };
        let past = (
            b.x.saturating_sub(run.x),
            b.y.saturating_sub(run.y),
            (run.x + run.w).saturating_sub(b.x + b.w),
            (run.y + run.h).saturating_sub(b.y + b.h),
        );
        if past != (0, 0, 0, 0) {
            out.push(Escape {
                box_tag: tag.clone(),
                text: text.clone(),
                past,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::style::Color;

    fn style() -> TextStyle {
        TextStyle::new()
            .with_size_px(10)
            .with_fg(Color::rgb(1, 2, 3))
    }

    /// The chip the reader reported, with its real numbers.
    fn protocol_chip() -> Rect {
        Rect::new(69, 587, 36, 18)
    }

    #[test]
    fn r1792_the_reported_caption_no_longer_escapes_its_box() {
        // Measured through the paint before this module: the box is 36 wide,
        // the run 32, placed at +7, so 3px hang off the right edge. Every one
        // of the five protocol chips does it, by exactly 3.
        let chip = protocol_chip();
        let caption = Caption::new("tcp", style()).stating((32, 12)).centred();
        let placed = place(chip, &caption);
        assert!(
            placed.run.x >= chip.x && placed.run.x + placed.run.w <= chip.x + chip.w,
            "★★★★★ the caption is inside the box a reader sees around it: \
             run {:?} in box {chip:?}",
            placed.run
        );
        assert_eq!(placed.run.x - chip.x, 2, "and centred: (36 - 32) / 2");
        assert_eq!(
            placed.fit,
            Fit::Fits {
                spare_x: 4,
                spare_y: 6
            },
            "with the room it did not use reported rather than left to arithmetic"
        );
    }

    #[test]
    fn r1792_a_caption_too_wide_is_named_rather_than_drawn_outside() {
        // The floor draws it and reports nothing: measured at 6.11, a label
        // whose text advances 171px in a 36px box has sizeHint 173 and no error
        // on any member.
        let chip = protocol_chip();
        let caption = Caption::new("a caption far wider than its box", style()).stating((171, 12));
        let placed = place(chip, &caption);
        assert_eq!(
            placed.fit,
            Fit::Overflows {
                over_x: 135,
                over_y: 0
            },
            "★★ by name, and by how much"
        );
        assert!(
            placed.run.x + placed.run.w <= chip.x + chip.w,
            "★★★★★ and STILL inside the box -- an overflow is reported, never \
             drawn outside: {:?} in {chip:?}",
            placed.run
        );
    }

    #[test]
    fn r1792_padding_is_kept_on_both_sides_before_alignment() {
        let chip = protocol_chip();
        let caption = Caption::new("tcp", style())
            .stating((32, 12))
            .centred()
            .padding(4, 1);
        let placed = place(chip, &caption);
        // Room is 36 - 8 = 28, narrower than the 32 the caption wants, so it is
        // clipped to the inner region and reported over by 4.
        assert_eq!(placed.run.w, 28, "clipped to what the padding leaves");
        assert_eq!(placed.run.x - chip.x, 4, "and starts after the padding");
        assert_eq!(
            placed.fit,
            Fit::Overflows {
                over_x: 4,
                over_y: 0
            }
        );
    }

    #[test]
    fn r1792_the_three_alignments_are_three_places() {
        let chip = Rect::new(0, 0, 100, 20);
        let at = |align| {
            place(
                chip,
                &Caption::new("x", style()).stating((20, 10)).align(align),
            )
            .run()
            .x
        };
        assert_eq!(
            (at(Align::Start), at(Align::Center), at(Align::End)),
            (0, 40, 80)
        );
    }

    #[test]
    fn r1792_the_placement_is_declared_where_a_reader_can_see_it() {
        // ★ The half that was missing. The run is sized to its own glyphs, so
        // this property moves nothing -- it is what lets a gate tell a centred
        // caption from a left one, which it cannot do while every run in the
        // tree carries the default.
        assert_eq!(Align::Center.declared(), TextAlign::Center);
        assert_eq!(Align::End.declared(), TextAlign::End);
        assert_eq!(
            Align::Start.declared(),
            TextAlign::Start,
            "and Start is the default, which is why 675 runs declaring it says \
             nothing about what any of them meant"
        );
    }

    #[test]
    fn r1792_the_caption_is_a_child_so_its_own_box_answers_for_it() {
        // The other half of the repair. A sibling caption is filed under
        // whatever container encloses both, which is how three chips' words
        // arrived as one run of a pane.
        let chip = protocol_chip();
        let (scene, placed) = captioned(
            "lab.palette.protocol.tcp",
            chip,
            BoxStyle::filled(Color::rgb(9, 9, 9)),
            &Caption::new("tcp", style()).stating((32, 12)).centred(),
            Pointer::Transparent,
        );
        let Scene::Container(node) = scene else {
            panic!("a captioned box is a container")
        };
        assert_eq!(node.tag.as_deref(), Some("lab.palette.protocol.tcp"));
        assert_eq!(node.children.len(), 1, "and it holds its caption");
        let Some(Scene::Text(run)) = node.children.first() else {
            panic!("the child is the caption")
        };
        assert_eq!(run.content, "tcp");
        assert_eq!(
            run.tag.as_deref(),
            Some("lab.palette.protocol.tcp.caption"),
            "addressable in its own right, under its box's name"
        );
        assert_eq!(
            (run.rect.w, run.rect.h),
            (placed.run.w, placed.run.h),
            "the size the derivation reported"
        );
    }

    #[test]
    fn r1792_a_caption_carries_its_own_voice_decision() {
        // A child caption is a NEW region with a tag, and this tree requires
        // every such region to be announced or to say why not. Without this the
        // module would trade one defect for a screenful of undeclared regions.
        let (scene, _) = captioned(
            "probe",
            protocol_chip(),
            BoxStyle::filled(Color::rgb(9, 9, 9)),
            &Caption::new("tcp", style())
                .stating((32, 12))
                .centred()
                .silent(Silence::name_of("probe")),
            Pointer::Target,
        );
        let Scene::Container(node) = scene else {
            panic!("a captioned box is a container")
        };
        let Some(child) = node.children.first() else {
            panic!("the child is the caption")
        };
        assert!(
            child
                .layout_style()
                .and_then(|style| style.silence.as_ref())
                .is_some(),
            "the reason travels with the node, so deleting the paint deletes it"
        );
    }

    /// ★★★★★ R1794 — **the test that would have caught what a reader had to
    /// catch.**
    ///
    /// R1792 centred the RUN RECTANGLE and called it centred text. The node lab
    /// passed `(32, 12)` for `tcp` because 32 was the number the hand-written
    /// code before it used, and 32 was a BOX. Asked of the wire afterwards, the
    /// glyphs advance **15**: so a 32-wide rectangle sat centred in the 36-wide
    /// chip, the glyphs sat `Start`-aligned at the left of that rectangle, and
    /// the ink was 8.5px left of the chip's centre — exactly what the reader
    /// saw and exactly what the gate could not see, because the gate measured
    /// rectangles.
    ///
    /// The property that makes the class impossible: **a stated size is not a
    /// measurement, and a placement says which it had.** A caller who states a
    /// box gets `Sized::Stated`, and any claim about where the glyphs are can be
    /// refused on that alone.
    #[test]
    fn r1794_a_placement_says_whether_its_size_was_measured_or_asserted() {
        let chip = protocol_chip();
        let stated = place(
            chip,
            &Caption::new("tcp", style()).stating((32, 12)).centred(),
        );
        assert_eq!(
            stated.sized(),
            Sized::Stated,
            "★★★★★ the shape of the defect: a number the caller had to hand is \
             not a measurement, and a placement built on one cannot support a \
             claim about where the glyphs are"
        );

        // With no font provider installed — which is this test's situation and
        // every headless caller's — the answer is a GUESS and says so, rather
        // than a number that reads like a measurement.
        let measured = place(chip, &Caption::new("tcp", style()).centred());
        assert_eq!(
            measured.sized(),
            Sized::Guessed,
            "and where nothing can shape, the estimate is labelled rather than \
             passed off: this is the arm that used to be silent"
        );
        assert!(
            measured.run().w > 0,
            "the fallback is still deterministic and drawable"
        );
    }

    #[test]
    fn r1792_the_box_says_whether_a_pointer_resolves_to_it() {
        // ★★★★★ Written because the first draft of this module did not have the
        // parameter and defaulted to opaque. The node lab's five protocol chips
        // are a colour key, not controls: made hit targets they resolve to no
        // External and forward nothing, and the screen's own transparency
        // census reported 60 dead regions -- five chips across twelve states.
        // A default cannot be right for both kinds, so neither is a default.
        let opaque = |pointer| {
            let (scene, _) = captioned(
                "probe",
                protocol_chip(),
                BoxStyle::filled(Color::rgb(9, 9, 9)),
                &Caption::new("tcp", style()).stating((32, 12)),
                pointer,
            );
            scene
                .layout_style()
                .is_some_and(|style| !style.pointer_transparent)
        };
        assert!(opaque(Pointer::Target), "a chip a person clicks");
        assert!(
            !opaque(Pointer::Transparent),
            "a colour key that only labels"
        );
    }

    #[test]
    fn r1792_a_caption_is_part_of_its_box_and_not_a_second_member() {
        // The other half of what the gates caught: the caption's tag lives
        // under the box's, so a family census counting by prefix doubles. The
        // suffix is published so a reader excludes it by the framework's name
        // for it rather than by a literal spelled again at each census.
        let (scene, _) = captioned(
            "lab.palette.protocol.tcp",
            protocol_chip(),
            BoxStyle::filled(Color::rgb(9, 9, 9)),
            &Caption::new("tcp", style()).stating((32, 12)),
            Pointer::Transparent,
        );
        let Scene::Container(node) = scene else {
            panic!("a captioned box is a container")
        };
        let Some(Scene::Text(run)) = node.children.first() else {
            panic!("the child is the caption")
        };
        let tag = run.tag.clone().unwrap_or_default();
        assert!(tag.ends_with(CAPTION_SUFFIX), "{tag}");
        assert_eq!(
            tag.strip_suffix(CAPTION_SUFFIX),
            node.tag.as_deref(),
            "and what is left is exactly its box's tag, so a census can subtract"
        );
    }

    #[test]
    fn r1792_the_gate_finds_the_escape_that_was_reported_and_clears_the_repair() {
        // The exact numbers off the paint, before and after. `past.2` is the
        // right edge, which is the one all five chips crossed.
        let boxes = vec![("lab.palette.protocol.tcp".to_owned(), protocol_chip())];
        let before = escapes(&boxes, &[("tcp".to_owned(), Rect::new(76, 591, 32, 12))]);
        assert_eq!(before.len(), 1, "the reported defect is found");
        assert_eq!(before[0].past, (0, 0, 3, 0), "3px past the right edge");

        let placed = place(
            protocol_chip(),
            &Caption::new("tcp", style()).stating((32, 12)).centred(),
        );
        let after = escapes(&boxes, &[("tcp".to_owned(), placed.run())]);
        assert_eq!(after, vec![], "★★★★★ and the derived placement clears it");
    }

    #[test]
    fn r1792_a_word_that_merely_clips_a_neighbour_is_not_in_it() {
        // ★★★★★ The false positive the first draft produced 40 of. A run whose
        // centre is elsewhere is not "in" the box it touches, however small that
        // box is — the node lab's link labels clip a neighbouring caption's box
        // and were reported as hanging 43px off its left edge.
        let neighbour = vec![("lab.link.label.text".to_owned(), Rect::new(100, 40, 30, 12))];
        let clipping = Rect::new(57, 29, 60, 12);
        assert!(
            clipping.x < 130 && 100 < clipping.x + clipping.w,
            "the fixture really does overlap, or this test proves nothing"
        );
        assert_eq!(
            escapes(&neighbour, &[("tcp/0.0.0.0".to_owned(), clipping)]),
            vec![]
        );
    }

    #[test]
    fn r1792_a_word_on_a_pane_is_not_a_caption_in_a_box() {
        // Without the scale rule every run in a screen would be judged against
        // whatever pane encloses it, and a heading at the left of a 1440-wide
        // strip would read as an escape. Measured: 288 of the capture viewer's
        // runs sit in a container that wide.
        let pane = vec![("pv.context".to_owned(), Rect::new(0, 0, 1440, 40))];
        assert_eq!(
            escapes(&pane, &[("off".to_owned(), Rect::new(737, 112, 31, 14))]),
            vec![],
            "a word overlapping a whole pane is on a pane, not in a box"
        );
    }

    /// ★★★★★ The coordinate convention, MEASURED rather than assumed.
    ///
    /// A child's absolute layout is resolved against its container, so the
    /// caption's rectangle inside the node is box-relative while [`place`]
    /// answers in the caller's space. Getting this backwards would double the
    /// offset and put every caption outside its box — the very defect this
    /// module exists to make unrepresentable — so it is asserted rather than
    /// commented.
    #[test]
    fn r1792_the_childs_rectangle_is_relative_to_the_box_it_is_in() {
        let chip = protocol_chip();
        let caption = Caption::new("tcp", style()).stating((32, 12)).centred();
        let placed = place(chip, &caption);
        let (scene, _) = captioned(
            "probe",
            chip,
            BoxStyle::filled(Color::rgb(9, 9, 9)),
            &caption,
            Pointer::Transparent,
        );
        let Scene::Container(node) = scene else {
            panic!("a captioned box is a container")
        };
        let Some(Scene::Text(run)) = node.children.first() else {
            panic!("the child is the caption")
        };
        assert_eq!(
            (run.rect.x, run.rect.y),
            (placed.run.x - chip.x, placed.run.y - chip.y),
            "box-relative inside the node, caller-space out of `place`"
        );
        assert_eq!((run.rect.x, run.rect.y), (2, 3));
    }
}
