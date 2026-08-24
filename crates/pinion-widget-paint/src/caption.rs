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
//! ⚠ **Those three numbers are R1792's, and their population is not the one a
//! reader now has.** Re-measured at R1812 over the *assembled* application — six
//! destinations walked as one program, which is how the tool actually ships —
//! [`Survey`] reports **170** caption/box pairs of 1,153 runs in 697 boxes,
//! **0** escaping, **154** declaring nothing. Both sets are true of what they
//! counted. Neither supersedes the other, and the reason to say so here is that
//! *230* and *170* look like the same measurement having drifted, and are not.
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
    holder: Rect,
    pad: (u32, u32),
    declares: TextAlign,
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

    /// The alignment this placement was made under, in the form a
    /// [`TextStyle`] takes.
    ///
    /// ★★★★★ R1812 — the third of the trio, and the one whose absence made the
    /// debt's *"225 of 230 sites declare nothing"* wrong in an instructive way.
    /// Measured on the node lab: sites using [`place`] to derive a rectangle
    /// **do** declare — `.centred().padding(pad, 0)`, right there in the call —
    /// and the declaration was consumed to produce a number and then dropped,
    /// because the caller builds its own run and had no way to carry the claim
    /// across without stating it a second time. So the paint said `Start` on a
    /// caption its author had centred, and every reader of the paint — a gate, a
    /// conformance check, a person asking over the wire — was told the opposite
    /// of what the code says.
    ///
    /// [`captioned`] applies this for a caller that lets this module build the
    /// node. This is the same fact for a caller that does not.
    #[must_use]
    pub const fn declares(self) -> TextAlign {
        self.declares
    }

    /// The room the caption declared it would keep clear, on each axis.
    ///
    /// ★ R1812 — carry item 3 of the debt this module repairs. Padding has been
    /// *declarable* through [`Caption::padding`] since R1792 and was not
    /// *readable*, which left the axis in the same state alignment had been in:
    /// a caller could state an intention that nothing downstream could check it
    /// against. The switch that prompted the debt was repaired by deriving the
    /// right rectangle rather than by declaring a margin, and this is what lets
    /// the next one declare instead.
    #[must_use]
    pub const fn padding(self) -> (u32, u32) {
        self.pad
    }

    /// The box the caption was placed in.
    #[must_use]
    pub const fn holder(self) -> Rect {
        self.holder
    }

    /// The gap the caption actually leaves on each side of its box — padding
    /// and alignment resolved into the four numbers a reader sees.
    ///
    /// `None` for an overflowing caption: [`place`] clamps the run to the inner
    /// region, so the *drawn* rectangle is inside, but four gaps computed from
    /// it would describe a caption that fits and this one does not. [`Fit`] is
    /// the answer for that case, by name.
    #[must_use]
    pub const fn room(self) -> Option<Room> {
        if !self.fit.fits() {
            return None;
        }
        Some(Room {
            left: self.run.x - self.holder.x,
            top: self.run.y - self.holder.y,
            right: (self.holder.x + self.holder.w) - (self.run.x + self.run.w),
            bottom: (self.holder.y + self.holder.h) - (self.run.y + self.run.h),
        })
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
    Placed {
        run,
        fit,
        sized,
        holder: box_rect,
        pad: (caption.pad_x, caption.pad_y),
        declares: caption.align_x.declared(),
    }
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
        // From the PLACEMENT rather than from the caption a second time, so
        // there is one expression in this module that turns an `Align` into the
        // claim the paint carries. `Placed::declares` is the same fact for a
        // caller that builds its own run.
        caption.style.clone().with_align(placed.declares()),
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
/// ⚠ **This is the GEOMETRY half.** A caller holding a [`Scene`] must arm
/// [`Survey`] instead: measured at R1812, this function armed over an assembled
/// six-destination application reports **seven** escapes of which every one is a
/// false positive, because a rule about rectangles cannot see that a run and a
/// box belong to different regions of the tree. It stays public because it is
/// the honest answer for a caller who has lists and no tree — a TUI back end,
/// or a check reading rectangles off the wire.
#[must_use]
pub fn escapes(boxes: &[(String, Rect)], runs: &[(String, Rect)]) -> Vec<Escape> {
    let mut out = Vec::new();
    for (text, run) in runs {
        let Some((tag, b)) = holder_of(boxes.iter().map(|(t, b)| (t, *b)), *run) else {
            continue;
        };
        let past = past_of(b, *run);
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

/// The smallest caption-scale box whose interior holds `run`'s centre.
///
/// The one geometric pairing rule in this module, so [`escapes`] and [`Survey`]
/// cannot drift into two answers about the same pair. Both halves are R1792's
/// and both were measured: the CENTRE (rather than an overlap) because overlap
/// assigned a word clipping a neighbour's corner to that neighbour and reported
/// 40 escapes of which all 40 were false, and the SCALE because a word crossing
/// a whole pane is on a pane rather than in a box.
fn holder_of<'t>(
    boxes: impl Iterator<Item = (&'t String, Rect)>,
    run: Rect,
) -> Option<(&'t String, Rect)> {
    let (cx, cy) = (run.x + run.w / 2, run.y + run.h / 2);
    boxes
        .filter(|(_, b)| cx >= b.x && cx < b.x + b.w && cy >= b.y && cy < b.y + b.h)
        .filter(|(_, b)| b.w <= run.w.saturating_mul(4))
        .min_by_key(|(_, b)| u64::from(b.w) * u64::from(b.h))
}

/// How far `run` reaches past each edge of `holder`, `(left, top, right,
/// bottom)`, each 0 where it does not.
const fn past_of(holder: Rect, run: Rect) -> (u32, u32, u32, u32) {
    (
        holder.x.saturating_sub(run.x),
        holder.y.saturating_sub(run.y),
        (run.x + run.w).saturating_sub(holder.x + holder.w),
        (run.y + run.h).saturating_sub(holder.y + holder.h),
    )
}

/// How a run and the box it appears in came to be considered a pair.
///
/// ★★★★★ R1812 — the distinction [`escapes`] does not have, and the reason it
/// cannot be armed over an assembled application on its own.
///
/// [`captioned`] makes a caption a **child** with the box's tag plus
/// [`CAPTION_SUFFIX`], so for anything built that way the pairing is a *fact
/// about the scene* and no rule about rectangles is involved. Everywhere else
/// there is nothing relating the two but where they landed, and a guess is what
/// is available.
///
/// Naming the two apart is not bookkeeping. A [`Bond::Declared`] caption
/// **cannot escape** — [`place`] clamps it — so a check that reported only
/// escapes would go quietly vacuous exactly as adoption grows: every site that
/// takes the repair leaves the population the check is looking at. That is the
/// failure this enum exists to make impossible to miss, and it is why
/// [`Survey`] reports both counts rather than one verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bond {
    /// The run is the box's own [`CAPTION_SUFFIX`] child.
    Declared,
    /// Nothing relates them but geometry and a shared region of the tree.
    Adjacent,
}

/// The room a caption leaves between itself and each edge of the box it sits in.
///
/// ★ R1812 — carry item 3 of the debt this module is the repair of: padding was
/// declarable through [`Caption::padding`] and **not readable back**, so a gate
/// could ask whether a caption was inside its box but never whether it kept the
/// room it said it would. These are the four numbers that make that askable, and
/// [`Sits`] is what turns them into a statement about intention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Room {
    /// Gap between the box's leading edge and the run's.
    pub left: u32,
    /// Gap above.
    pub top: u32,
    /// Gap between the run's trailing edge and the box's.
    pub right: u32,
    /// Gap below.
    pub bottom: u32,
}

/// Which alignments a placement on one axis could have been produced by.
///
/// ★★★★★ The measurable half, and the reason it is worth having: a screen that
/// declares nothing still *places* its captions somewhere, and where they sit
/// rules some intentions out even though it can never rule one in. `discovery
/// off` — the second case a reader reported — sat with 48px of room on its left
/// and **0** on its right: consistent with [`Align::End`], flatly inconsistent
/// with [`Align::Center`]. Nothing could say so, because nothing asked and
/// nothing had declared.
///
/// # Padding does not enter into it
///
/// Derived from [`place`]: with padding `p` and slack `s` between the box's
/// inner region and the run, a `Start` placement leaves `p` before and `p + s`
/// after, an `End` placement the mirror, and a `Center` placement `p + s/2` and
/// `p + s - s/2`. So the *difference* `after - before` is `s` for `Start`, `-s`
/// for `End` and `s % 2` for `Center` — **`p` cancels in all three**. That is
/// what lets this answer without knowing the padding, which is the thing a paint
/// sweep does not have.
///
/// A snug caption (`s == 0`) is consistent with all three, and says so. That is
/// not a weakness of the rule: with no slack the three placements *are* the same
/// placement, and a check claiming to distinguish them would be inventing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sits {
    start: bool,
    center: bool,
    end: bool,
}

impl Sits {
    /// What a run leaving `before` and `after` on one axis could have been
    /// aligned by.
    #[must_use]
    pub const fn of(before: u32, after: u32) -> Self {
        Self {
            start: after >= before,
            center: after == before || after == before + 1,
            end: before >= after,
        }
    }

    /// Whether this placement is consistent with `align`.
    #[must_use]
    pub const fn holds(self, align: Align) -> bool {
        match align {
            Align::Start => self.start,
            Align::Center => self.center,
            Align::End => self.end,
        }
    }

    /// The single alignment this placement is consistent with, when there is
    /// exactly one.
    ///
    /// `None` for a snug caption, which is consistent with all three, and
    /// `None` for a placement that is consistent with two.
    #[must_use]
    pub const fn only(self) -> Option<Align> {
        match (self.start, self.center, self.end) {
            (true, false, false) => Some(Align::Start),
            (false, true, false) => Some(Align::Center),
            (false, false, true) => Some(Align::End),
            _ => None,
        }
    }
}

/// One caption, the box it appears in, and everything measurable about the pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    box_tag: String,
    text: String,
    bond: Bond,
    run: Rect,
    holder: Rect,
    declares: TextAlign,
}

impl Placement {
    /// The tag of the box the caption appears in.
    #[must_use]
    pub fn box_tag(&self) -> &str {
        &self.box_tag
    }

    /// What the caption says.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// How the two came to be a pair — and therefore how much this placement's
    /// verdict is worth.
    #[must_use]
    pub const fn bond(&self) -> Bond {
        self.bond
    }

    /// The rectangle the run was painted in.
    #[must_use]
    pub const fn run(&self) -> Rect {
        self.run
    }

    /// The rectangle a reader sees around it.
    #[must_use]
    pub const fn holder(&self) -> Rect {
        self.holder
    }

    /// How far the caption reaches past each edge, `(left, top, right,
    /// bottom)`; all zero when it is inside.
    #[must_use]
    pub const fn past(&self) -> (u32, u32, u32, u32) {
        past_of(self.holder, self.run)
    }

    /// Whether the caption is drawn outside the box it appears in.
    #[must_use]
    pub const fn escapes(&self) -> bool {
        !matches!(self.past(), (0, 0, 0, 0))
    }

    /// The room left on each side, or `None` when the caption is not inside the
    /// box at all and the four gaps would be a fiction.
    #[must_use]
    pub const fn room(&self) -> Option<Room> {
        if self.escapes() {
            return None;
        }
        Some(Room {
            left: self.run.x - self.holder.x,
            top: self.run.y - self.holder.y,
            right: (self.holder.x + self.holder.w) - (self.run.x + self.run.w),
            bottom: (self.holder.y + self.holder.h) - (self.run.y + self.run.h),
        })
    }

    /// Which horizontal alignments this placement is consistent with.
    #[must_use]
    pub const fn sits_x(&self) -> Option<Sits> {
        match self.room() {
            Some(room) => Some(Sits::of(room.left, room.right)),
            None => None,
        }
    }

    /// Which vertical alignments this placement is consistent with.
    #[must_use]
    pub const fn sits_y(&self) -> Option<Sits> {
        match self.room() {
            Some(room) => Some(Sits::of(room.top, room.bottom)),
            None => None,
        }
    }

    /// What the run's own style claims about where it sits — `None` when it
    /// claims nothing.
    ///
    /// ★ [`TextAlign::Start`] is **not** a claim. It is the framework default,
    /// so a run carrying it is indistinguishable from a run whose author never
    /// thought about the question — which is the whole of why a reader's report
    /// could not be turned into a gate. `Justify` is not a claim either: it has
    /// no meaning for a single-line caption, and reading it as one would let a
    /// multi-line style silently answer a question about a word.
    #[must_use]
    pub const fn claim(&self) -> Option<Align> {
        match self.declares {
            TextAlign::Center => Some(Align::Center),
            TextAlign::End => Some(Align::End),
            // `TextAlign` is `non_exhaustive`, and the wildcard leans the safe
            // way on purpose: an alignment this module has not been taught
            // counts as *silent*, which inflates the population
            // [`Survey::silent`] exists to drive down rather than quietly
            // granting a claim nobody here has reasoned about.
            TextAlign::Start | TextAlign::Justify | _ => None,
        }
    }

    /// Whether this caption claims an alignment it is not sitting at.
    ///
    /// False for a caption that claims nothing — *unanswerable* is not *fine*,
    /// and [`Survey::silent`] is where that population is counted rather than
    /// hidden inside a boolean.
    #[must_use]
    pub const fn breaks_its_claim(&self) -> bool {
        match (self.claim(), self.sits_x()) {
            (Some(align), Some(sits)) => !sits.holds(align),
            // A caption outside its box breaks more than its claim, and
            // `escapes` is what reports that.
            _ => false,
        }
    }
}

/// Every caption in a painted scene, paired with the box a reader sees around
/// it — **populations and pairing rule both taken from the scene**.
///
/// # The recipe this replaces
///
/// Arming [`escapes`] takes two lists, and before this type every screen wrote
/// the derivation beside itself. Measured at R1812 over the assembled
/// application, that recipe reports **7** escapes across six destinations and
/// **all 7 are false**, in exactly two ways:
///
/// - **4** paired a run of the host's own chrome with a box inside a *mounted
///   screen* — a comparison across the host/guest seam that
///   `pinion_screen::layering` exists to refuse and that no rule about
///   rectangles can see. One 470px hint line at the bottom of the window
///   collected a different guest's box in each of four destinations.
/// - **2** treated a `Scene::Text` that carries its own tag as a *box*, then
///   paired it with an unrelated run: an open enum dropdown's option label,
///   painted over the form row beneath it exactly as a dropdown should be,
///   reported as escaping the key label of a different field.
///
/// Both are pairing defects rather than geometry defects, so the repair is a
/// rule about the **tree**:
///
/// 1. A box is a tagged node that **paints no text of its own**. A tagged
///    `Scene::Text` is a run; it is never a box.
/// 2. A run and a box are a pair only when they share the **same nearest tagged
///    ancestor** — they are two things inside one named region. This also
///    subsumes the hand-written filter every arming carried (*exclude boxes that
///    own runs*), because a box is never its own descendant's sibling.
/// 3. A [`Bond::Declared`] pair is matched by TAG and skips both rules: the
///    scene already says the two belong together.
///
/// # What the floor does
///
/// Nothing of the sort, and it cannot: measured at 6.11 a widget owns its text,
/// so there is no population to survey and no pairing question to get wrong —
/// and equally no way to ask where the glyphs landed. The capability this type
/// has is the one that comes with a scene that can be read.
#[derive(Debug, Clone, Default)]
pub struct Survey {
    placements: Vec<Placement>,
    runs: usize,
    boxes: usize,
}

/// A tagged node that paints no text of its own, as [`Survey::of`] reads it.
struct Held {
    tag: String,
    rect: Rect,
    /// The nearest tagged ancestor — the named region this box lives in, which
    /// is what rule 2 compares.
    home: Option<String>,
}

/// A text run, as [`Survey::of`] reads it.
struct Painted {
    text: String,
    rect: Rect,
    /// The nearest tagged ancestor. `None` for a run outside every tagged node.
    home: Option<String>,
    declares: TextAlign,
    /// Its own tag, when it carries one — the half rule 3 reads.
    tag: Option<String>,
}

impl Survey {
    /// Survey `scene`, which must already have been through the layout pass —
    /// the rectangles this reads are absolute ones.
    #[must_use]
    pub fn of(scene: &Scene) -> Self {
        let mut boxes: Vec<Held> = Vec::new();
        let mut runs: Vec<Painted> = Vec::new();
        scene.for_each_node(&mut |visit| {
            let Some(rect) = visit.absolute_rect() else {
                // Clipped entirely away: painted nowhere, so not painted.
                return;
            };
            let home = visit
                .ancestors
                .iter()
                .rev()
                .find_map(|ancestor| ancestor.tag())
                .map(str::to_owned);
            match visit.node {
                Scene::Text(text) => runs.push(Painted {
                    text: text.content.clone(),
                    rect,
                    home,
                    declares: text.style.text_align,
                    tag: visit.node.tag().map(str::to_owned),
                }),
                // Rule 1: only a node that paints no text of its own is a box.
                node => {
                    if let Some(tag) = node.tag() {
                        boxes.push(Held {
                            tag: tag.to_owned(),
                            rect,
                            home,
                        });
                    }
                }
            }
        });

        let mut placements = Vec::new();
        for run in &runs {
            // Rule 3: a caption the scene itself relates to a box is matched by
            // TAG, whatever the rectangles do.
            let declared = run
                .tag
                .as_deref()
                .and_then(|tag| tag.strip_suffix(CAPTION_SUFFIX))
                .and_then(|stem| boxes.iter().find(|held| held.tag == stem))
                .map(|held| (held.tag.clone(), held.rect, Bond::Declared));
            let paired = match declared {
                Some(pair) => Some(pair),
                None => holder_of(
                    // Rule 2: same named region, or not a pair at all.
                    boxes
                        .iter()
                        .filter(|held| held.home == run.home)
                        .map(|held| (&held.tag, held.rect)),
                    run.rect,
                )
                .map(|(tag, rect)| (tag.clone(), rect, Bond::Adjacent)),
            };
            let Some((box_tag, holder, bond)) = paired else {
                continue;
            };
            placements.push(Placement {
                box_tag,
                text: run.text.clone(),
                bond,
                run: run.rect,
                holder,
                declares: run.declares,
            });
        }

        Self {
            placements,
            runs: runs.len(),
            boxes: boxes.len(),
        }
    }

    /// Every caption/box pair the scene holds.
    #[must_use]
    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }

    /// How many text runs the scene painted — the denominator [`Self::pairs`]
    /// is a fraction of.
    ///
    /// ★ R1800's rule: a gate that reports a count without its denominator
    /// cannot tell *nothing is wrong* from *nothing was looked at*.
    #[must_use]
    pub const fn runs(&self) -> usize {
        self.runs
    }

    /// How many boxes it painted.
    #[must_use]
    pub const fn boxes(&self) -> usize {
        self.boxes
    }

    /// How many caption/box pairs were found.
    #[must_use]
    pub fn pairs(&self) -> usize {
        self.placements.len()
    }

    /// Pairs the scene itself relates, through [`captioned`].
    #[must_use]
    pub fn bound(&self) -> usize {
        self.count(|placement| placement.bond == Bond::Declared)
    }

    /// Pairs held together by nothing but geometry and a shared region.
    #[must_use]
    pub fn adjacent(&self) -> usize {
        self.count(|placement| placement.bond == Bond::Adjacent)
    }

    /// Captions that say where they sit.
    #[must_use]
    pub fn claiming(&self) -> usize {
        self.count(|placement| placement.claim().is_some())
    }

    /// Captions that say nothing, and about which *off-centre* and *deliberately
    /// left* are therefore the same picture.
    ///
    /// This is the number the debt behind this module carried as prose — and
    /// prose does not ratchet. A gate that prints it is what turns "most sites
    /// still do not declare" into something that can only get smaller.
    #[must_use]
    pub fn silent(&self) -> usize {
        self.count(|placement| placement.claim().is_none())
    }

    /// Captions drawn outside the box a reader sees around them.
    #[must_use]
    pub fn escaped(&self) -> Vec<&Placement> {
        self.placements.iter().filter(|p| p.escapes()).collect()
    }

    /// Captions sitting somewhere other than where they claim to.
    #[must_use]
    pub fn broken(&self) -> Vec<&Placement> {
        self.placements
            .iter()
            .filter(|p| p.breaks_its_claim())
            .collect()
    }

    fn count(&self, mut pred: impl FnMut(&Placement) -> bool) -> usize {
        self.placements.iter().filter(|p| pred(p)).count()
    }

    /// Fold `other`'s pairs into this survey, so a walk over many frames
    /// answers as one population.
    ///
    /// A caller surveying an application visits every destination and several
    /// poses of some of them; keeping a running total here rather than at each
    /// call site is what stops the per-frame numbers from being added up two
    /// different ways in two different screens.
    pub fn absorb(&mut self, other: Self) {
        self.placements.extend(other.placements);
        self.runs += other.runs;
        self.boxes += other.boxes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::TextNode;
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

    // ── R1812 — the pairing, and the seven false positives that shaped it ────

    /// A tagged container at an absolute position, holding `children`.
    fn region(tag: &str, rect: Rect, children: Vec<Scene>) -> Scene {
        Scene::Container(
            ContainerNode::new(children)
                .with_tag(tag.to_owned())
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(rect.x, rect.y)
                        .with_size(Size::px(rect.w, rect.h)),
                ),
        )
    }

    /// A tagged box that paints nothing of its own, at an absolute position.
    fn empty_box(tag: &str, rect: Rect) -> Scene {
        region(tag, rect, vec![])
    }

    /// A bare run at an absolute position, optionally carrying its own tag.
    fn loose_run(text: &str, rect: Rect, tag: Option<&str>) -> Scene {
        let mut node = TextNode::styled(text.to_owned(), Rect::default(), style()).with_layout(
            LayoutStyle::new()
                .with_absolute_position(rect.x, rect.y)
                .with_size(Size::px(rect.w, rect.h)),
        );
        if let Some(tag) = tag {
            node = node.with_tag(tag.to_owned());
        }
        Scene::Text(node)
    }

    /// Lay a scene out the way a window does, so the survey reads real absolute
    /// rectangles rather than the ones this file wrote down.
    fn laid_out(scene: Scene, size: (u32, u32)) -> Scene {
        let mut scene = scene;
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, size.0, size.1);
        scene
    }

    /// Tag/rectangle pairs, as every hand-written arming of [`escapes`] holds
    /// them.
    type Listed = Vec<(String, Rect)>;

    /// The lists the hand-written arming builds, so a test can show what the
    /// geometry-only check answers about the same scene.
    fn geometric_lists(scene: &Scene) -> (Listed, Listed) {
        let mut owners = std::collections::BTreeSet::new();
        let mut tags = Vec::new();
        let mut runs = Vec::new();
        scene.for_each_node(&mut |visit| {
            let Some(rect) = visit.absolute_rect() else {
                return;
            };
            if let Some(tag) = visit.node.tag() {
                tags.push((tag.to_owned(), rect));
            }
            if let Scene::Text(text) = visit.node {
                if let Some(owner) = visit.ancestors.iter().rev().find_map(|a| a.tag()) {
                    owners.insert(owner.to_owned());
                }
                runs.push((text.content.clone(), rect));
            }
        });
        tags.retain(|(tag, _)| !owners.contains(tag));
        (tags, runs)
    }

    /// ★★★★★ **Four of the seven.** A run of the host's own chrome, and a box
    /// inside a screen the host is showing, are not a caption and its box.
    ///
    /// Measured at R1812 over the assembled application: one 470px hint line
    /// along the bottom of the window collected a *different* guest's box in
    /// each of four destinations and was reported as escaping all four. It is
    /// the host/guest seam, which `pinion_screen::layering` exists to refuse and
    /// which no rule about rectangles can see.
    ///
    /// The counterfactual is inside the test: the geometry-only check is armed
    /// over the same scene and **must** report the escape. Without it this would
    /// pass on a scene where nothing overlaps anything.
    #[test]
    fn r1812_a_host_run_and_a_guest_box_are_not_a_pair() {
        let scene = laid_out(
            region(
                "shell",
                Rect::new(0, 0, 1440, 900),
                vec![
                    region(
                        "guest",
                        Rect::new(0, 0, 1200, 890),
                        vec![region(
                            "guest.strip",
                            Rect::new(531, 838, 457, 44),
                            vec![empty_box("guest.lane", Rect::new(0, 0, 457, 44))],
                        )],
                    ),
                    loose_run(
                        "drag a header · e edit · Esc restore",
                        Rect::new(662, 853, 470, 14),
                        None,
                    ),
                ],
            ),
            (1440, 900),
        );

        let (boxes, runs) = geometric_lists(&scene);
        assert_eq!(
            escapes(&boxes, &runs).len(),
            1,
            "the counterfactual: geometry alone DOES pair these two, which is \
             what made the assembled application report four of these"
        );

        let survey = Survey::of(&scene);
        assert!(
            survey.escaped().is_empty(),
            "a host run and a guest box share no named region and are not a \
             pair: {:?}",
            survey.escaped()
        );
    }

    /// ★★★★★ **Two of the seven.** A run that carries its own tag is a run.
    ///
    /// Measured: an open enum dropdown's option label — painted over the form
    /// row beneath it, exactly as a dropdown should be — was reported as
    /// escaping *the key label of a different field*, because the hand-written
    /// arming excluded only tags that OWN runs and a text node is not its own
    /// ancestor. So the field's key text entered the survey as a box.
    #[test]
    fn r1812_a_tagged_text_node_is_a_run_and_never_a_box() {
        let scene = laid_out(
            region(
                "form",
                Rect::new(0, 0, 400, 200),
                vec![
                    loose_run(
                        "discovery.multicast",
                        Rect::new(10, 20, 100, 16),
                        Some("form.key"),
                    ),
                    loose_run(
                        "peer_to_peer",
                        Rect::new(11, 21, 76, 18),
                        Some("form.option"),
                    ),
                ],
            ),
            (400, 200),
        );

        let (boxes, runs) = geometric_lists(&scene);
        assert!(
            boxes.iter().any(|(tag, _)| tag == "form.key"),
            "the counterfactual: the hand-written arming really does put a \
             tagged run in the box list — {boxes:?}"
        );
        assert_eq!(
            escapes(&boxes, &runs).len(),
            1,
            "and really does report the overlap as an escape"
        );

        let survey = Survey::of(&scene);
        assert!(
            survey.escaped().is_empty(),
            "a node that paints text of its own is a run: {:?}",
            survey.escaped()
        );
    }

    /// A caption `captioned` built is paired by TAG, so it is judged however the
    /// rectangles fall.
    ///
    /// This is the arm that keeps the check from going vacuous as adoption
    /// grows: a `Bond::Declared` caption's box is the run's own nearest tagged
    /// ancestor, so the sibling rule alone would drop exactly the sites that
    /// took the repair.
    #[test]
    fn r1812_a_declared_caption_is_paired_by_tag() {
        let chip = protocol_chip();
        let caption = Caption::new("tcp", style()).stating((32, 12)).centred();
        let (chip_scene, _) = captioned(
            "lab.chip.tcp",
            chip,
            BoxStyle::filled(Color::rgb(9, 9, 9)),
            &caption,
            Pointer::Transparent,
        );
        let scene = laid_out(
            region("palette", Rect::new(0, 0, 300, 900), vec![chip_scene]),
            (300, 900),
        );

        let survey = Survey::of(&scene);
        let placement = survey
            .placements()
            .iter()
            .find(|p| p.text() == "tcp")
            .expect("the caption is paired with the box it is a child of");
        assert_eq!(placement.bond(), Bond::Declared);
        assert_eq!(placement.box_tag(), "lab.chip.tcp");
        assert!(!placement.escapes(), "{:?}", placement.past());
        assert_eq!(survey.bound(), 1);
        assert_eq!(survey.adjacent(), 0);
    }

    /// ★★★★★ The reader's second case, turned into a statement.
    ///
    /// `discovery off` sat with 48px of room on its left and **0** on its right.
    /// Nothing in this tree could say anything about that; these are the numbers
    /// that say what it rules out.
    #[test]
    fn r1812_a_placement_rules_out_the_alignment_it_is_not_sitting_at() {
        let sits = Sits::of(48, 0);
        assert!(sits.holds(Align::End), "flush right is consistent with End");
        assert!(
            !sits.holds(Align::Center),
            "and flatly inconsistent with Center, which is what a reader saw"
        );
        assert!(!sits.holds(Align::Start));
        assert_eq!(sits.only(), Some(Align::End));

        // A snug caption is consistent with all three and says so rather than
        // inventing a distinction that is not in the picture.
        let snug = Sits::of(0, 0);
        assert_eq!(snug.only(), None);
        for align in [Align::Start, Align::Center, Align::End] {
            assert!(snug.holds(align), "{align:?}");
        }

        // Odd slack: `place` gives the extra pixel to the trailing side.
        assert!(Sits::of(2, 3).holds(Align::Center), "slack 5 centred");
        assert!(!Sits::of(3, 2).holds(Align::Center), "the other way round");
    }

    /// ★★★★★ **The gate the whole debt is about**, in one scene: a caption that
    /// SAYS it is centred and is not.
    ///
    /// Every earlier check in this tree is green on this scene — the run is
    /// inside its box, overlaps nothing, and is reachable. It is wrong anyway,
    /// and it is wrong in the one way that can be *proved* wrong rather than
    /// merely suspected: the scene contains both the claim and the placement.
    #[test]
    fn r1812_a_caption_that_claims_centre_and_sits_at_the_edge_is_broken() {
        let hold = Rect::new(69, 647, 202, 58);
        let scene = laid_out(
            region(
                "palette",
                Rect::new(0, 0, 300, 900),
                vec![
                    empty_box("palette.switch", hold),
                    Scene::Text(
                        TextNode::styled(
                            "discovery off".to_owned(),
                            Rect::default(),
                            style().with_align(TextAlign::Center),
                        )
                        .with_layout(
                            LayoutStyle::new()
                                .with_absolute_position(117, 657)
                                .with_size(Size::px(154, 13)),
                        ),
                    ),
                ],
            ),
            (300, 900),
        );

        let survey = Survey::of(&scene);
        let broken = survey.broken();
        assert_eq!(broken.len(), 1, "{:?}", survey.placements());
        assert_eq!(broken[0].text(), "discovery off");
        assert_eq!(broken[0].box_tag(), "palette.switch");
        assert!(
            !broken[0].escapes(),
            "and it is INSIDE its box, which is why nothing here could see it"
        );
        let room = broken[0].room().expect("inside, so it has room");
        assert_eq!((room.left, room.right), (48, 0));
        assert_eq!(survey.claiming(), 1);
        assert_eq!(survey.silent(), 0);
    }

    /// The same scene with the claim removed is *unanswerable*, not *fine* —
    /// and the survey counts it rather than passing it.
    ///
    /// This is the counterfactual for the test above: without it, `broken()`
    /// returning nothing would look like success on a screen that declares
    /// nothing — which is the state **154 of the 170** caption/box pairs in the
    /// assembled analysis tool were measured to be in at R1812.
    #[test]
    fn r1812_a_caption_that_claims_nothing_is_counted_not_passed() {
        let hold = Rect::new(69, 647, 202, 58);
        let scene = laid_out(
            region(
                "palette",
                Rect::new(0, 0, 300, 900),
                vec![
                    empty_box("palette.switch", hold),
                    loose_run("discovery off", Rect::new(117, 657, 154, 13), None),
                ],
            ),
            (300, 900),
        );

        let survey = Survey::of(&scene);
        assert!(survey.broken().is_empty(), "there is no claim to break");
        assert_eq!(
            survey.silent(),
            1,
            "but it is counted: {:?}",
            survey.placements()
        );
        assert_eq!(survey.claiming(), 0);
        assert_eq!(
            survey.placements()[0].sits_x().and_then(Sits::only),
            Some(Align::End),
            "and where it sits is still measurable"
        );
    }

    /// Carry item 3: the padding a caption declares is readable back off the
    /// placement, and so is the room it actually leaves.
    #[test]
    fn r1812_padding_and_room_are_readable_off_a_placement() {
        let hold = Rect::new(10, 20, 100, 40);
        let placed = place(
            hold,
            &Caption::new("ok", style()).stating((20, 10)).padding(6, 4),
        );
        assert_eq!(placed.padding(), (6, 4));
        assert_eq!(placed.holder(), hold);
        let room = placed.room().expect("it fits");
        assert_eq!(
            (room.left, room.top),
            (6, 4),
            "a Start caption sits exactly its padding in"
        );
        assert_eq!(
            (room.right, room.bottom),
            (100 - 6 - 20, 40 - 4 - 10),
            "and leaves the padding plus the slack on the far side"
        );

        // Centred, the padding cancels out of the difference — which is the
        // property `Sits` is built on, asserted rather than trusted.
        let centred = place(
            hold,
            &Caption::new("ok", style())
                .stating((20, 10))
                .padding(6, 4)
                .centred(),
        );
        let room = centred.room().expect("it fits");
        assert!(Sits::of(room.left, room.right).holds(Align::Center));
        assert_eq!(room.left, room.right, "even slack, so exactly centred");

        // An overflowing caption has no room to report, and says so by name.
        let over = place(hold, &Caption::new("ok", style()).stating((400, 10)));
        assert_eq!(over.room(), None);
        assert!(!over.fit().fits());
    }

    /// A survey reports the denominators its counts are fractions of.
    ///
    /// R1800's rule: without them a `0` cannot be told from *nothing was
    /// looked at*, and this check's population shrinks as the repair spreads.
    #[test]
    fn r1812_a_survey_reports_its_denominators() {
        let scene = laid_out(
            region(
                "palette",
                Rect::new(0, 0, 300, 900),
                vec![
                    empty_box("palette.switch", Rect::new(69, 647, 202, 58)),
                    loose_run("discovery off", Rect::new(117, 657, 154, 13), None),
                    loose_run("far away", Rect::new(0, 0, 40, 13), None),
                ],
            ),
            (300, 900),
        );
        let survey = Survey::of(&scene);
        assert_eq!(survey.runs(), 2, "both runs counted, paired or not");
        assert_eq!(survey.boxes(), 2, "the palette and the switch");
        assert_eq!(survey.pairs(), 1, "only one run is in a caption-scale box");
        assert_eq!(survey.bound() + survey.adjacent(), survey.pairs());
        assert_eq!(
            survey.claiming() + survey.silent(),
            survey.pairs(),
            "every pair is in exactly one of the two"
        );
    }

    /// ★★★★★ A placement carries the claim it was made under, so a caller that
    /// builds its own run does not have to state the alignment twice — and the
    /// scene stops declaring the opposite of what the call site says.
    ///
    /// Measured on the node lab at R1812: three sites derived a rectangle with
    /// `.centred()` and painted a run carrying `TextAlign::Start`, because
    /// `place` returned a number and the claim had nowhere to travel.
    #[test]
    fn r1812_a_placement_carries_the_claim_it_was_made_under() {
        let hold = Rect::new(0, 0, 100, 20);
        let centred = place(
            hold,
            &Caption::new("go", style()).stating((20, 10)).centred(),
        );
        assert_eq!(centred.declares(), TextAlign::Center);

        // The counterfactual: a caller who says nothing gets the default, so
        // this is a claim being carried rather than one being invented.
        let quiet = place(hold, &Caption::new("go", style()).stating((20, 10)));
        assert_eq!(quiet.declares(), TextAlign::Start);
        assert_eq!(
            place(
                hold,
                &Caption::new("go", style())
                    .stating((20, 10))
                    .align(Align::End)
            )
            .declares(),
            TextAlign::End
        );

        // And it is the SAME expression `captioned` paints with — one place in
        // this module turns an `Align` into the claim the paint carries.
        let caption = Caption::new("go", style()).stating((20, 10)).centred();
        let (scene, placed) = captioned(
            "probe",
            hold,
            BoxStyle::filled(Color::rgb(1, 1, 1)),
            &caption,
            Pointer::Transparent,
        );
        let Scene::Container(node) = scene else {
            panic!("a captioned box is a container")
        };
        let Some(Scene::Text(run)) = node.children.first() else {
            panic!("the child is the caption")
        };
        assert_eq!(run.style.text_align, placed.declares());
    }

    /// Two surveys fold into one population, so a walk over an application adds
    /// its frames up one way.
    #[test]
    fn r1812_surveys_of_two_frames_fold_into_one_population() {
        let frame = |tag: &str| {
            laid_out(
                region(
                    tag,
                    Rect::new(0, 0, 300, 900),
                    vec![
                        empty_box("switch", Rect::new(69, 647, 202, 58)),
                        loose_run("discovery off", Rect::new(117, 657, 154, 13), None),
                    ],
                ),
                (300, 900),
            )
        };
        let mut all = Survey::of(&frame("a"));
        let before = all.pairs();
        all.absorb(Survey::of(&frame("b")));
        assert_eq!(all.pairs(), before * 2);
        assert_eq!(all.silent(), 2);
        assert_eq!(all.runs(), 2);
    }
}
