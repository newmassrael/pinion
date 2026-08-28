//! R1816 §5.27 §5.50 — **a card header's layout, and the order it gives way
//! in.**
//!
//! [`pinion_core::widgets::card`] settled what a card header *is* at R1648: a
//! closed four-affordance set in one canonical order, a content state, and a
//! remedy derived from it. What it did not settle is where any of that lands,
//! so the one shipped consumer laid its header out by hand — and the census row
//! for *a widget card with a title, settings, tear-off, maximise and close*
//! reads `app` rather than `have` for exactly that reason.
//!
//! ## What is here, and what is deliberately not
//!
//! This module answers **where the parts go**. It returns rectangles and takes
//! no theme, no colour and no glyph, because those are the half of a card
//! header that has been chosen exactly once in this tree — and freezing an
//! opinion that has one witness is how a lift becomes a fork with extra steps.
//! The consumer keeps its skin; what it stops keeping is the arithmetic.
//!
//! ## Why the arithmetic is the half worth lifting
//!
//! Because it is the half a hand-rolled copy gets *wrong*, and this tree has
//! the measurement. Before R1672 the consumer's header was one expression — a
//! title width with `.max(40)` on the end — with everything after it measured
//! from that clamped number. At one board cell (75 px) the card painted its
//! title 11 px past its own frame, its kind dot 21 px past, its ready badge
//! **65 px** past, and two affordance slots off its left edge: twenty-five
//! marks outside the card, in a state the sweep already ran, invisible because
//! a clamp had already turned an overflow into a plausible number.
//!
//! ⇒ the rule this module exists to carry: **a part that does not fit is not
//! painted, rather than painted smaller.** [`HeaderLayout`] answers `None` for
//! a title or a badge there is no room for and simply omits a slot it had to
//! drop, and `header_parts_stay_inside` asserts the whole result lies within
//! the band at every width from zero up.
//!
//! ## The order
//!
//! It is the judgement a toolbar makes, and it is the consumer's, preserved:
//!
//! 1. the affordance strip keeps as many slots as fit, dropping from the
//!    **left**, so the last-declared stays nearest the edge a hand reaches for;
//! 2. the ready badge goes before the title does;
//! 3. the title takes what is left;
//! 4. the grip is the card's identity and gives way **last**.
//!
//! ★★★★★ That fourth line said *never gives way* when this module was written,
//! copying the consumer's comment, and the property test below refuted it
//! within minutes: at a header 0 px wide the grip was placed 18 px outside it.
//! **"Last" and "never" are the same sentence until something asks about the
//! degenerate end** — and a card dragged to nothing, a pane collapsed onto its
//! splitter and a board cell mid-resize all pass through it. The order was
//! right; the exception was not.
//!
//! ## What the floor does
//!
//! Measured by building and running the reference toolkit at 6.11: its framed
//! container with a title gives the title one line and clips it, and its
//! toolbar answers an overflowing action set with a *pop-up menu* rather than
//! by dropping in a declared order — so a caller there cannot ask which parts
//! survived a narrowing, only look. Answering **which parts gave way** is the
//! capability this type has and that one does not.

use pinion_core::containment;
use pinion_core::scene::{ContainerNode, PathCommand, PathNode, PathPoint, Rect, Scene, TextNode};
use pinion_core::style::{
    BoxStyle, Color, LayoutStyle, PathStyle, Size, Stroke, TextOverflow, TextStyle,
};
use pinion_core::widgets::card::CardAffordance;

/// The ready badge's status dot, across.
const BADGE_DOT: u32 = 6;

/// The clearance between that dot and the badge's word.
const BADGE_GAP: u32 = 4;

/// The measurements a card header lays out against.
///
/// Every field is a number the one shipped consumer had as a constant. They are
/// parameters here rather than constants because the debt this module repays
/// named the trigger precisely: *the second consumer wanting a different slot
/// width is the moment you learn what the painter's parameters are.* Rather
/// than wait for that consumer to appear, the constants it would have had to
/// fight are already the argument list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardMetrics {
    /// The header band's nominal height.
    ///
    /// ⚠ Deliberately **not** read off the rectangle passed to [`lay_out`].
    /// The consumer's frame eats into that rectangle's height while its slot
    /// and dot arithmetic stayed keyed to the nominal band, so the two differ
    /// by the frame width. That is the consumer's arithmetic preserved exactly,
    /// not a new opinion — and it is the first thing a second consumer should
    /// be asked about, because one of the two is redundant.
    pub band_h: u32,
    /// One header control slot's width.
    pub slot_w: u32,
    /// How far a slot is inset from the top and bottom of the band.
    pub slot_inset_y: u32,
    /// The clearance the affordance strip keeps at the header's right edge.
    pub tail: u32,
    /// The drag grip's width.
    pub grip_w: u32,
    /// How far the grip is inset from the header's left edge and its top.
    pub grip_inset: u32,
    /// The gap between the grip's right edge and where the title starts.
    pub title_gap: u32,
    /// The narrowest a title may be and still be worth painting.
    pub min_title: u32,
    /// The ready badge's width, including the gap before it.
    pub badge_w: u32,
    /// The face the title is set in.
    ///
    /// ★★★★★ R1882 — a face lives here, with the other measurements, because
    /// **a face size IS a measurement**: it fixes the line box a run needs, and
    /// a layout that cannot see it cannot honour that floor. This module's
    /// stated exclusions are theme, colour and glyph; a size in pixels is none
    /// of the three.
    ///
    /// It used to live on [`HeaderSpec`], which only [`header_scene`] receives —
    /// so [`lay_out`] wrote the two text boxes' heights as literals (`16` and
    /// `14`) for faces it had no way to ask about, and every card in the one
    /// shipped consumer painted a title three pixels short of its own line.
    /// ⇒ *a layout cannot honour a floor on a value it never receives.*
    pub title_px: u32,
    /// The face the ready badge's word is set in.
    pub badge_px: u32,
}

impl Default for CardMetrics {
    /// The one shipped consumer's numbers, which are the only ones any screen
    /// in this tree has chosen.
    fn default() -> Self {
        Self {
            band_h: 34,
            slot_w: 28,
            slot_inset_y: 4,
            tail: 6,
            grip_w: 18,
            grip_inset: 4,
            title_gap: 20,
            min_title: 24,
            badge_w: 54,
            title_px: 12,
            badge_px: 10,
        }
    }
}

impl CardMetrics {
    /// The same metrics with a different slot width — the axis the debt this
    /// module repays predicted a second consumer would differ on first.
    #[must_use]
    pub const fn with_slot_width(mut self, slot_w: u32) -> Self {
        self.slot_w = slot_w;
        self
    }

    /// The same metrics with a different band height.
    #[must_use]
    pub const fn with_band_height(mut self, band_h: u32) -> Self {
        self.band_h = band_h;
        self
    }
}

/// Where one header control sits, counting from the **right**.
///
/// Separate from [`lay_out`] and public on purpose: a hit test asks this
/// question about one slot without laying the whole header out, and the two
/// must not be able to disagree — the paint and the gesture reading two facts
/// is a defect class this tree carries a standing debt for. The consumer's hit
/// test called its own copy of this arithmetic before the lift; now both call
/// this.
///
/// `n` is the index into the offered list and `count` its length, so slot `0`
/// is the leftmost and `count - 1` sits against the tail clearance.
#[must_use]
pub const fn slot_rect(header: Rect, count: u32, n: u32, metrics: CardMetrics) -> Rect {
    let from_right = count.saturating_sub(n);
    Rect::new(
        (header.x + header.w).saturating_sub(from_right * metrics.slot_w + metrics.tail),
        header.y + metrics.slot_inset_y,
        metrics.slot_w,
        metrics.band_h.saturating_sub(metrics.slot_inset_y * 2),
    )
}

/// The drag grip at the left of a header.
#[must_use]
pub const fn grip_rect(header: Rect, metrics: CardMetrics) -> Rect {
    Rect::new(
        header.x + metrics.grip_inset,
        header.y + metrics.grip_inset,
        metrics.grip_w,
        metrics.band_h.saturating_sub(metrics.grip_inset * 2),
    )
}

/// Where every part of a card header landed, and **what gave way**.
///
/// The absent cases are the point. A title that does not fit is `None` rather
/// than a rectangle too small to read, and slots that did not fit are simply
/// not in [`Self::slots`] — with [`Self::dropped`] saying how many, so a caller
/// that wants to offer them elsewhere (a pop-up, a wider layout) can tell
/// *dropped* from *never offered*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderLayout {
    grip: Option<Rect>,
    title: Option<Rect>,
    badge: Option<Rect>,
    slots: Vec<(usize, Rect)>,
    dropped: usize,
    band: Rect,
}

impl HeaderLayout {
    /// The drag grip, or `None` when even it did not fit.
    ///
    /// ★★★★★ R1816 — the consumer's rule said the grip *never* gives way, and
    /// this module's own property test refuted that within minutes of being
    /// written: at a header 0 px wide the grip was placed 18 px outside it. The
    /// rule is right about the ORDER and wrong about the exception — the grip
    /// gives way **last**, not never, and "last" and "never" are the same
    /// sentence until something asks about the degenerate end.
    ///
    /// That end is not hypothetical arithmetic. It is what a card being dragged
    /// to nothing, a pane collapsed to its splitter, or a board cell mid-resize
    /// passes through, and it is precisely where the pre-R1672 header put
    /// twenty-five marks outside the card.
    #[must_use]
    pub const fn grip(&self) -> Option<Rect> {
        self.grip
    }

    /// Where the title goes, or `None` when the header is too narrow for one
    /// worth painting.
    #[must_use]
    pub const fn title(&self) -> Option<Rect> {
        self.title
    }

    /// Where the ready badge goes, or `None` — either because the card is not
    /// ready, or because the badge gave way before the title did.
    #[must_use]
    pub const fn badge(&self) -> Option<Rect> {
        self.badge
    }

    /// The affordance slots that survived, as `(index into the offered list,
    /// rectangle)` in declaration order.
    #[must_use]
    pub fn slots(&self) -> &[(usize, Rect)] {
        &self.slots
    }

    /// How many affordances were offered and had to be dropped.
    #[must_use]
    pub const fn dropped(&self) -> usize {
        self.dropped
    }

    /// The band every part above lies inside.
    #[must_use]
    pub const fn band(&self) -> Rect {
        self.band
    }

    /// Every rectangle this layout placed, for a caller checking containment.
    #[must_use]
    pub fn placed(&self) -> Vec<Rect> {
        let mut out: Vec<Rect> = Vec::new();
        out.extend(self.grip);
        out.extend(self.title);
        out.extend(self.badge);
        out.extend(self.slots.iter().map(|(_, r)| *r));
        out
    }
}

/// Lay a card header out in `header`, giving way in the declared order.
///
/// `offered` is how many affordances the card's chrome offers and `ready`
/// whether its state earns the badge. Nothing here reads
/// [`pinion_core::widgets::card`] directly: the caller has already asked that
/// crate what is offered, and passing the two answers keeps this module a
/// layout question rather than a second opinion about card state.
#[must_use]
pub fn lay_out(header: Rect, offered: usize, ready: bool, metrics: CardMetrics) -> HeaderLayout {
    // The grip's rectangle is computed unconditionally because everything to
    // its right is measured from it, and PLACED only if it fits. Those are two
    // different questions and conflating them is what put a grip outside a
    // zero-width header in this module's first draft.
    let grip = grip_rect(header, metrics);
    let grip_fits = metrics.grip_inset + metrics.grip_w <= header.w;
    let text_x = grip.x + grip.w + metrics.title_gap;
    let right = header.x + header.w;

    // Rule 2. What the header can give the strip once the identity and a title
    // that says something are paid for. Derived, so the count the caller paints
    // and the width the strip is sized from are one number.
    let room_for_slots = right.saturating_sub(text_x + metrics.min_title + metrics.tail);
    let fits = if metrics.slot_w == 0 {
        offered
    } else {
        (room_for_slots / metrics.slot_w) as usize
    };
    let shown = usize::min(offered, fits);
    let dropped = offered - shown;

    let count = u32::try_from(offered).unwrap_or(u32::MAX);
    let slots: Vec<(usize, Rect)> = (dropped..offered)
        .map(|n| {
            (
                n,
                slot_rect(header, count, u32::try_from(n).unwrap_or(u32::MAX), metrics),
            )
        })
        .collect();

    // Rules 3 and 4.
    let strip_w = u32::try_from(shown).unwrap_or(u32::MAX) * metrics.slot_w;
    let text_room = right.saturating_sub(text_x + strip_w + metrics.tail);
    let show_badge = ready && text_room >= metrics.badge_w + metrics.min_title;
    let title_w = if show_badge {
        text_room - metrics.badge_w
    } else {
        text_room
    };

    // ★★★★★ R1882 — both text boxes are DERIVED from the faces they hold, and
    // both are centred on the same band, so the two cannot drift apart and
    // neither can be written too short. `line_rect_in` is `band_in` with the
    // height taken from `line_box`, and `band_in` rounds ONCE from the band's
    // own centre — so a title and a badge of different faces still share a
    // centre line exactly.
    //
    // ⚠ The band is the NOMINAL one (`metrics.band_h`), not `header.h`, for the
    // reason `CardMetrics::band_h` documents: the consumer's frame eats into
    // the rectangle while its slot and dot arithmetic stayed keyed to the
    // nominal band. Keeping to that band is what makes these two agree with the
    // grip, the dot and the slots rather than with the frame.
    let band = Rect::new(header.x, header.y, header.w, metrics.band_h);
    let title =
        (title_w > 0).then(|| containment::line_rect_in(band, text_x, title_w, metrics.title_px));
    let badge = show_badge.then(|| {
        containment::line_rect_in(
            band,
            text_x + title_w + 4,
            metrics.badge_w,
            metrics.badge_px,
        )
    });

    HeaderLayout {
        grip: grip_fits.then_some(grip),
        title,
        badge,
        slots,
        dropped,
        band: header,
    }
}

// ── The skin ────────────────────────────────────────────────────────────────

/// The four colours a card header draws with.
///
/// ★★★★★ R1817 — R1816 lifted this module's arithmetic and deliberately left
/// the skin behind, on the reasoning that glyphs and colours had been chosen
/// exactly once and a one-witness opinion frozen into a crate is a fork with
/// extra steps. That reasoning is **overridden by the standing instruction this
/// work runs under**, which is that the framework builds a capability the
/// reference class supports whether or not a second consumer exists yet, and
/// that the deliverable is a crate rather than an example. Deferring left the
/// census row saying `app` while the framework owned everything that was hard
/// about it, which is the worse of the two errors: a reader looking for a card
/// header still found none.
///
/// Colours are taken as values rather than a `Theme` so a caller with a palette
/// of its own — which the one shipped consumer has — does not have to invent a
/// theme to ask for a header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderInk {
    /// The title's colour.
    pub title: Color,
    /// The grip dots and the affordance marks.
    pub muted: Color,
    /// The ready badge's dot and word.
    pub accent: Color,
    /// The card-kind dot beside the grip.
    pub kind: Color,
}

/// What a card header says and which of its affordances are live.
///
/// Bundled rather than passed as eight arguments, which is also what keeps
/// [`header_scene`] inside this crate's argument-count lint.
#[derive(Debug, Clone)]
pub struct HeaderSpec<'a> {
    /// The affordances the card's chrome offers, in declaration order.
    pub offered: &'a [CardAffordance],
    /// Whether the card's state earns the ready badge.
    pub ready: bool,
    /// Whether [`CardAffordance::Maximize`] should draw its RESTORE form —
    /// the card is already maximised and the control brings it back.
    pub restore: bool,
    /// The title, elided inside whatever room it gets.
    pub title: &'a str,
    /// The badge's word.
    pub badge: &'a str,
    /// The colours.
    pub ink: HeaderInk,
}

fn dot(x: u32, y: u32, size: u32, fill: Color) -> Scene {
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_style(BoxStyle::filled(fill).with_corner_radius(size / 2))
            .with_layout(absolute(Rect::new(x, y, size, size))),
    )
}

fn absolute(rect: Rect) -> LayoutStyle {
    LayoutStyle::new()
        .with_absolute_position(rect.x, rect.y)
        .with_size(Size::px(rect.w, rect.h))
        .with_pointer_transparent(true)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "header coordinates are < 2^13, exactly representable in f32"
)]
fn point(x: u32, y: u32) -> PathPoint {
    PathPoint::new(x as f32, y as f32)
}

/// A stroked polyline set in `rect`-local coordinates.
fn strokes(rect: Rect, runs: &[Vec<(u32, u32)>], ink: Color, width: u32) -> Scene {
    let mut commands = Vec::new();
    for run in runs {
        for (n, (x, y)) in run.iter().enumerate() {
            let at = point(*x, *y);
            commands.push(if n == 0 {
                PathCommand::MoveTo(at)
            } else {
                PathCommand::LineTo(at)
            });
        }
    }
    Scene::Path(
        PathNode::new(rect, commands, PathStyle::stroked(Stroke::new(ink, width)))
            .with_layout(absolute(rect)),
    )
}

/// The mark one header control draws, in its slot's local coordinates.
///
/// ★ R1697's lesson, carried here with the code it is about: `restore` is the
/// maximise control's OTHER FACE. The control toggles, and a control that
/// toggles without changing its mark tells a person the same thing in both
/// states — a capability that exists and is not drawn is one nobody can use.
#[must_use]
pub fn affordance_mark(
    affordance: CardAffordance,
    rect: Rect,
    ink: Color,
    restore: bool,
) -> Vec<Scene> {
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    match affordance {
        CardAffordance::Settings => (0..3)
            .map(|n| dot(cx - 1, cy - 5 + n * 5, 2, ink))
            .collect(),
        // A square lifting out of another.
        CardAffordance::TearOff => vec![strokes(
            rect,
            &[
                vec![
                    (cx - 5, cy - 1),
                    (cx - 5, cy + 5),
                    (cx + 1, cy + 5),
                    (cx + 1, cy - 1),
                    (cx - 5, cy - 1),
                ],
                vec![(cx - 1, cy - 5), (cx + 5, cy - 5), (cx + 5, cy + 1)],
            ],
            ink,
            1,
        )],
        // Two overlapping squares — one box come back out of another, which is
        // the form a restore control takes everywhere.
        CardAffordance::Maximize if restore => vec![strokes(
            rect,
            &[
                vec![
                    (cx - 6, cy - 2),
                    (cx + 2, cy - 2),
                    (cx + 2, cy + 6),
                    (cx - 6, cy + 6),
                    (cx - 6, cy - 2),
                ],
                vec![(cx - 2, cy - 6), (cx + 6, cy - 6), (cx + 6, cy + 2)],
            ],
            ink,
            1,
        )],
        CardAffordance::Maximize => vec![strokes(
            rect,
            &[vec![
                (cx - 5, cy - 5),
                (cx + 5, cy - 5),
                (cx + 5, cy + 5),
                (cx - 5, cy + 5),
                (cx - 5, cy - 5),
            ]],
            ink,
            1,
        )],
        CardAffordance::Close => strokes_close(rect, ink),
    }
}

fn strokes_close(rect: Rect, ink: Color) -> Vec<Scene> {
    let (w, h) = (rect.w, rect.h);
    let (x0, y0, x1, y1) = (w / 2 - 4, h / 2 - 4, w / 2 + 4, h / 2 + 4);
    vec![strokes(
        rect,
        &[vec![(x0, y0), (x1, y1)], vec![(x1, y0), (x0, y1)]],
        ink,
        1,
    )]
}

/// The six-dot drag grip.
#[must_use]
pub fn grip_scene(tag: impl Into<String>, header: Rect, metrics: CardMetrics, ink: Color) -> Scene {
    let rect = grip_rect(header, metrics);
    Scene::Container(
        ContainerNode::new(
            (0..3)
                .flat_map(|r| (0..2).map(move |c| dot(4 + c * 5, 8 + r * 5, 2, ink)))
                .collect(),
        )
        .with_tag(tag.into())
        .with_layout(absolute(rect)),
    )
}

/// **A card header, painted.**
///
/// The scenes come back in paint order and every tagged one is addressed
/// `{tag_prefix}.{suffix}` — `.grip` for the drag handle and the affordance's
/// own wire word for each control, which is the vocabulary
/// [`CardAffordance::from_wire`] round-trips. A caller's hit test asks
/// [`slot_rect`] about the same rectangles, so what is drawn is what is pressed
/// without either side owning a second copy of the arithmetic.
///
/// Parts that do not fit are **absent**, per [`lay_out`].
#[must_use]
pub fn header_scene(
    tag_prefix: &str,
    header: Rect,
    spec: &HeaderSpec<'_>,
    metrics: CardMetrics,
) -> Vec<Scene> {
    let laid = lay_out(header, spec.offered.len(), spec.ready, metrics);
    let mut out = Vec::new();

    if let Some(grip) = laid.grip() {
        out.push(grip_scene(
            format!("{tag_prefix}.grip"),
            header,
            metrics,
            spec.ink.muted,
        ));
        out.push(dot(
            grip.x + grip.w + 4,
            header.y + metrics.band_h / 2 - 4,
            9,
            spec.ink.kind,
        ));
    }
    if let Some(title) = laid.title() {
        out.push(run(spec.title, title, metrics.title_px, spec.ink.title));
    }
    if let Some(badge) = laid.badge() {
        // ★★★★★ R1882 — the word's box is the badge's OWN slot, inset, rather
        // than a second rectangle written from the header's top. That second
        // rectangle was `Rect::new(badge.x + 10, header.y + 10, 40, 14)`, and
        // its `14` was a HEIGHT WRITTEN TWICE: once here for the word and once
        // in `lay_out` for the slot. The debt this repays named only the
        // layout's copy; measuring found this one too, which is why the word
        // now takes the slot's height instead of stating one.
        //
        // The dot is centred in that slot for the same reason — a mark placed
        // at the slot's top only looked centred while the slot happened to be
        // the dot's own size.
        let word = Rect::new(badge.x + BADGE_DOT + BADGE_GAP, badge.y, 40, badge.h);
        out.push(dot(
            badge.x,
            badge.y + badge.h / 2 - BADGE_DOT / 2,
            BADGE_DOT,
            spec.ink.accent,
        ));
        out.push(run(spec.badge, word, metrics.badge_px, spec.ink.accent));
    }
    for (n, slot) in laid.slots().iter().copied() {
        let affordance = spec.offered[n];
        out.push(Scene::Container(
            ContainerNode::new(affordance_mark(
                affordance,
                Rect::new(0, 0, slot.w, slot.h),
                spec.ink.muted,
                spec.restore,
            ))
            .with_tag(format!("{tag_prefix}.{}", affordance.wire()))
            .with_layout(absolute(slot)),
        ));
    }
    out
}

fn run(text: &str, rect: Rect, px: u32, fg: Color) -> Scene {
    Scene::Text(
        TextNode::styled(
            text,
            rect,
            TextStyle::new()
                .with_size_px(px)
                .with_fg(fg)
                .with_overflow(TextOverflow::Ellipsis),
        )
        .with_layout(absolute(rect)),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        CardMetrics, HeaderInk, HeaderLayout, HeaderSpec, containment, grip_rect, header_scene,
        lay_out, slot_rect,
    };
    use pinion_core::scene::Rect;
    use pinion_core::style::Color;

    fn band(w: u32) -> Rect {
        Rect::new(10, 20, w, 33)
    }

    /// ★★★★★ **Nothing this layout places lies outside the band it was given**,
    /// at every width from zero up.
    ///
    /// This is R1672's defect stated as a property rather than as a repair. The
    /// consumer's pre-R1672 header put twenty-five marks outside the card at one
    /// board cell, and the sweep that ran that state could not see them, because
    /// the overflow had already been laundered into a plausible number by a
    /// `.max(40)`. A property over the whole width range is what makes that
    /// unrepresentable instead of merely fixed once.
    #[test]
    fn r1816_header_parts_stay_inside_at_every_width() {
        let metrics = CardMetrics::default();
        for w in 0..400 {
            for offered in 0..=4 {
                for ready in [false, true] {
                    let head = band(w);
                    let laid = lay_out(head, offered, ready, metrics);
                    for part in laid.placed() {
                        assert!(
                            part.x >= head.x
                                && part.x + part.w <= head.x + head.w
                                && part.y >= head.y,
                            "w={w} offered={offered} ready={ready}: {part:?} escapes {head:?}"
                        );
                    }
                }
            }
        }
    }

    /// ★★★★★ **No text part is ever placed in a box shorter than its own
    /// face's line**, at every width and at every face this layout can be given.
    ///
    /// R1816's sibling above is the same doctrine on the other axis. That one
    /// says *a part that does not fit across is absent rather than painted
    /// smaller*; this one says the same thing about DOWN — a box shorter than
    /// the line it holds **is** a part painted smaller, and the module had been
    /// doing it on every card since the lift.
    ///
    /// ⚠ It is a property over faces, not a check of two numbers, because the
    /// defect was never a wrong constant — it was a layout that could not see
    /// the faces at all and therefore wrote a literal for whatever it was
    /// handed. Sweeping the face is what makes that unrepresentable rather than
    /// merely corrected once.
    ///
    /// ⚠ And it compares against [`containment::line_box`]'s own output rather
    /// than re-spelling `px * 3 / 2 + 2`. A gate that re-spells its rule becomes
    /// the rule's second author, and this project has measured what that costs.
    #[test]
    fn r1882_no_header_text_sits_in_a_box_shorter_than_its_face() {
        for title_px in [8, 10, 12, 13, 16, 20, 24] {
            for badge_px in [8, 10, 12, 16] {
                let metrics = CardMetrics {
                    title_px,
                    badge_px,
                    ..CardMetrics::default()
                };
                for w in 0..400 {
                    let head = band(w);
                    let laid = lay_out(head, 4, true, metrics);
                    if let Some(title) = laid.title() {
                        assert!(
                            title.h >= containment::line_box(title_px),
                            "w={w} title_px={title_px}: {title:?} holds a \
                             {title_px}px face needing {}",
                            containment::line_box(title_px),
                        );
                    }
                    if let Some(badge) = laid.badge() {
                        assert!(
                            badge.h >= containment::line_box(badge_px),
                            "w={w} badge_px={badge_px}: {badge:?} holds a \
                             {badge_px}px face needing {}",
                            containment::line_box(badge_px),
                        );
                    }
                }
            }
        }
    }

    /// ★ A title and a badge of DIFFERENT faces still share one centre line.
    ///
    /// This is the property `band_in` exists for and the reason the two boxes
    /// are derived from the same band rather than each from its own offset: the
    /// naive `(outer.h - h) / 2` rounds twice and two faces of different parity
    /// then sit a pixel apart, which reads as a crooked header and is invisible
    /// to any check that looks at one part at a time.
    #[test]
    fn r1882_title_and_badge_share_a_centre_line_at_every_face_pair() {
        for title_px in [9, 10, 11, 12, 13] {
            for badge_px in [9, 10, 11, 12, 13] {
                let metrics = CardMetrics {
                    title_px,
                    badge_px,
                    ..CardMetrics::default()
                };
                let laid = lay_out(band(400), 4, true, metrics);
                let title = laid.title().expect("400px fits a title");
                let badge = laid.badge().expect("400px fits a badge");
                assert_eq!(
                    title.y + title.h / 2,
                    badge.y + badge.h / 2,
                    "title_px={title_px} badge_px={badge_px}: {title:?} and \
                     {badge:?} do not share a centre",
                );
            }
        }
    }

    /// A part that does not fit is ABSENT, not small — and the counts say which.
    #[test]
    fn r1816_a_part_that_does_not_fit_is_absent_rather_than_clamped() {
        let metrics = CardMetrics::default();
        let narrow = lay_out(band(75), 4, true, metrics);
        assert!(
            narrow.dropped() > 0,
            "a 75px header cannot hold four slots and a title: {narrow:?}"
        );
        assert_eq!(
            narrow.slots().len() + narrow.dropped(),
            4,
            "every offered affordance is either placed or counted as dropped"
        );
        assert!(
            narrow.badge().is_none(),
            "the badge gives way before the title does"
        );

        let wide = lay_out(band(400), 4, true, metrics);
        assert_eq!(wide.dropped(), 0);
        assert!(wide.title().is_some() && wide.badge().is_some());
    }

    /// Slots drop from the LEFT, so the last-declared affordance is the one
    /// that survives a narrowing.
    #[test]
    fn r1816_the_strip_drops_from_the_left() {
        let metrics = CardMetrics::default();
        let laid = lay_out(band(150), 4, false, metrics);
        let kept: Vec<usize> = laid.slots().iter().map(|(n, _)| *n).collect();
        assert!(!kept.is_empty(), "something survives a 150px header");
        assert_eq!(
            kept.last().copied(),
            Some(3),
            "the last-declared affordance stays nearest the edge a hand reaches \
             for: kept {kept:?}"
        );
        assert_eq!(
            kept,
            (laid.dropped()..4).collect::<Vec<_>>(),
            "and what survives is a suffix of the declaration order"
        );
    }

    /// ★★★★★ **Where the parts actually are**, as literal rectangles — and this
    /// test exists because a counterfactual PASSED without it.
    ///
    /// R1816 shifted the shared slot rectangle two pixels down and every check
    /// in this module stayed green. The containment property still held (a slot
    /// two pixels lower is still inside the band) and
    /// `r1816_the_hit_tests_slot_is_the_layouts_slot` still held, because it
    /// compares two callers of the SAME function and they moved together.
    ///
    /// ⇒ the same shape as the defect this round's sibling repaired one screen
    /// over: **an agreement between two things that move together cannot see a
    /// shared shift.** Agreement is worth asserting and it is not enough; some
    /// test has to name a number that came from somewhere else.
    ///
    /// These numbers came from somewhere else — they are the shipped consumer's
    /// arithmetic as it stood before the lift, so this doubles as the proof that
    /// the lift preserved its pixels rather than merely compiling.
    #[test]
    fn r1816_the_default_metrics_reproduce_the_consumers_own_rectangles() {
        let metrics = CardMetrics::default();
        let head = Rect::new(10, 20, 300, 33);

        // grip_rect: `Rect::new(header.x + 4, header.y + 4, 18, CARD_HDR - 8)`
        assert_eq!(
            grip_rect(head, metrics),
            Rect::new(14, 24, 18, 26),
            "the grip is where the consumer put it"
        );

        // affordance_rect: x = (header.x + header.w) - (count - n) * 28 - 6,
        // y = header.y + 4, h = CARD_HDR - 8.
        assert_eq!(
            slot_rect(head, 4, 3, metrics),
            Rect::new(10 + 300 - 28 - 6, 24, 28, 26),
            "the last-declared slot sits against the tail clearance"
        );
        assert_eq!(
            slot_rect(head, 4, 0, metrics),
            Rect::new(10 + 300 - 4 * 28 - 6, 24, 28, 26),
            "and the first-declared is three slots further left"
        );

        // The title's X is still the consumer's: text_x = grip.x + grip.w + 20.
        let laid = lay_out(head, 4, false, metrics);
        let title = laid.title().expect("300px fits a title");
        assert_eq!(
            title.x,
            14 + 18 + 20,
            "the title starts where it always did"
        );
        assert_eq!(
            title.x + title.w,
            slot_rect(head, 4, 0, metrics).x,
            "the title ends exactly where the strip begins"
        );

        // ★★★★★ R1882 — the title's HEIGHT is deliberately NOT the consumer's
        // any more, and this is the one assertion in the module that has to say
        // so. It was `Rect::new(text_x, rect.y + 9, title_w, 16)` — a 16px box
        // for a 12px face that needs 20 — so preserving it would have been
        // preserving a defect. What replaces the literal is the DERIVATION's
        // own output rather than a second number chosen here: a gate that
        // re-spells the rule becomes its second author, and this module has
        // already paid for that once.
        assert_eq!(
            (title.y, title.h),
            (
                containment::line_rect_in(
                    Rect::new(head.x, head.y, head.w, metrics.band_h),
                    title.x,
                    title.w,
                    metrics.title_px,
                )
                .y,
                containment::line_box(metrics.title_px),
            ),
            "the title's box is one line of its own face, on the nominal band"
        );
        // And the centre is unmoved, which is what makes this a repair rather
        // than a reposition: the old box ran 9..25 of a 34px band and the new
        // one runs 7..27 — same centre line, four more pixels of room.
        assert_eq!(
            title.y + title.h / 2,
            head.y + metrics.band_h / 2,
            "the title still sits on the band's centre line"
        );
    }

    /// ★ The standalone slot arithmetic a hit test uses answers the same
    /// rectangle the layout placed — the paint and the gesture reading ONE fact,
    /// asserted across the seam rather than assumed.
    ///
    /// ⚠ Necessary and NOT sufficient: both sides call `slot_rect`, so this
    /// cannot see a change that moves the slot itself. That is what
    /// `r1816_the_default_metrics_reproduce_the_consumers_own_rectangles` is
    /// for, and a counterfactual is what proved the difference.
    #[test]
    fn r1816_the_hit_tests_slot_is_the_layouts_slot() {
        let metrics = CardMetrics::default();
        let head = band(400);
        let laid = lay_out(head, 4, false, metrics);
        for (n, rect) in laid.slots() {
            assert_eq!(
                *rect,
                slot_rect(head, 4, u32::try_from(*n).unwrap(), metrics),
                "slot {n}"
            );
        }
    }

    /// The metrics are parameters, and changing one moves what it names and
    /// nothing else.
    #[test]
    fn r1816_a_wider_slot_costs_the_title_and_not_the_grip() {
        let head = band(300);
        let narrow_slots = lay_out(head, 3, false, CardMetrics::default());
        let wide_slots = lay_out(
            head,
            3,
            false,
            CardMetrics::default().with_slot_width(28 + 10),
        );
        assert_eq!(
            narrow_slots.grip(),
            wide_slots.grip(),
            "the grip is the card's identity and does not move"
        );
        assert!(
            narrow_slots.grip().is_some(),
            "300px is not the degenerate end"
        );
        let (a, b) = (
            narrow_slots.title().expect("fits"),
            wide_slots.title().expect("still fits"),
        );
        assert!(
            b.w + 30 == a.w,
            "three slots each 10px wider cost the title 30px: {a:?} then {b:?}"
        );
    }

    /// ★★★★★ The grip gives way LAST, and that it gives way at all is this
    /// round's own finding rather than the consumer's rule.
    ///
    /// The consumer's comment said the grip *never* gives way; this module
    /// copied that, and the property test above refuted it at a zero-width
    /// header before the round was an hour old. Pinned here as a stated
    /// capability so it cannot quietly become an accident again — and pinned
    /// with the ORDER, because "last" is the part of the original rule that was
    /// right.
    #[test]
    fn r1816_the_grip_gives_way_last_and_does_give_way() {
        let metrics = CardMetrics::default();
        // Wide enough for the grip and nothing else: everything after it is
        // already gone while the grip is still placed.
        let only_grip = lay_out(band(metrics.grip_inset + metrics.grip_w), 4, true, metrics);
        assert!(
            only_grip.grip().is_some(),
            "the grip outlasts every other part"
        );
        assert!(only_grip.title().is_none());
        assert!(only_grip.badge().is_none());
        assert_eq!(only_grip.dropped(), 4, "and the whole strip has gone");

        // One pixel narrower and even the grip is absent rather than outside.
        let none = lay_out(
            band(metrics.grip_inset + metrics.grip_w - 1),
            4,
            true,
            metrics,
        );
        assert!(
            none.grip().is_none(),
            "a grip that does not fit is not painted: {none:?}"
        );
        assert!(
            none.placed().is_empty(),
            "and nothing at all is placed rather than something outside"
        );
    }

    /// ★★★★★ **A whole card header, painted through the public API alone** —
    /// the proof the census row `dashboard.t0.4` cites for its `have`.
    ///
    /// R1602's standing rule is that a `have` costs a test exercising the
    /// capability through the PUBLIC surface, because a wrong `have` inflates a
    /// number silently while a wrong `app` self-corrects. So this builds a
    /// header the way any caller would — no internals — and asserts the things
    /// a caller depends on: that every control is addressable by the
    /// affordance's own wire word, that the grip is addressable, and that a
    /// narrowing removes controls from the scene rather than moving them
    /// somewhere a press cannot follow.
    #[test]
    fn r1817_a_whole_header_is_painted_through_the_public_api() {
        use pinion_core::widgets::card::CardAffordance::{Close, Maximize, Settings, TearOff};

        let ink = HeaderInk {
            title: Color::rgb(0xff, 0xff, 0xff),
            muted: Color::rgb(0x88, 0x88, 0x88),
            accent: Color::rgb(0x00, 0xc0, 0x80),
            kind: Color::rgb(0xc0, 0x40, 0x40),
        };
        let offered = [Settings, TearOff, Maximize, Close];
        let spec = HeaderSpec {
            offered: &offered,
            ready: true,
            restore: false,
            title: "ingest",
            badge: "LIVE",
            ink,
        };
        let metrics = CardMetrics::default();

        let wide = header_scene("card.a", band(400), &spec, metrics);
        let tags = tags_of(&wide);
        for want in [
            "card.a.grip",
            "card.a.settings",
            "card.a.tear_off",
            "card.a.maximize",
            "card.a.close",
        ] {
            assert!(
                tags.iter().any(|t| t == want),
                "a caller addresses `{want}` and the header painted {tags:?}"
            );
        }

        // Narrowed until the strip has to give way: the controls that went are
        // ABSENT from the scene, not drawn somewhere a press cannot reach.
        let narrow = header_scene("card.a", band(150), &spec, metrics);
        let narrow_tags = tags_of(&narrow);
        assert!(
            narrow_tags.len() < tags.len(),
            "a 150px header cannot carry what a 400px one does: {narrow_tags:?}"
        );
        assert!(
            narrow_tags.iter().any(|t| t == "card.a.close"),
            "and what survives is the last-declared control, nearest the edge a \
             hand reaches for: {narrow_tags:?}"
        );
        assert!(
            !narrow_tags.iter().any(|t| t == "card.a.settings"),
            "while the first-declared has gone rather than been squeezed"
        );
    }

    /// Every tag in a scene tree, so a test can ask what a caller can address.
    fn tags_of(scenes: &[pinion_core::scene::Scene]) -> Vec<String> {
        fn walk(scene: &pinion_core::scene::Scene, out: &mut Vec<String>) {
            match scene {
                pinion_core::scene::Scene::Container(node) => {
                    if let Some(tag) = node.tag.as_deref() {
                        out.push(tag.to_owned());
                    }
                    for child in &node.children {
                        walk(child, out);
                    }
                }
                pinion_core::scene::Scene::Text(node) => {
                    if let Some(tag) = node.tag.as_deref() {
                        out.push(tag.to_owned());
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for scene in scenes {
            walk(scene, &mut out);
        }
        out
    }

    /// A layout is `PartialEq`, so a caller can assert one whole rather than
    /// field by field — and this pins that an empty chrome is not a special
    /// case, only the ordinary one with nothing offered.
    #[test]
    fn r1816_no_affordances_is_the_ordinary_case() {
        let metrics = CardMetrics::default();
        let none: HeaderLayout = lay_out(band(300), 0, false, metrics);
        assert!(none.slots().is_empty());
        assert_eq!(none.dropped(), 0);
        assert!(none.title().is_some(), "the title takes the whole band");
    }
}
