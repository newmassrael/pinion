//! R1952 §5.27 §5.50 — **the mark a widget draws to point at a state, as a
//! path rather than as a character.**
//!
//! # The report this answers
//!
//! R1951 classified every button on the analysis shell as read by its words or
//! read by a mark, and wrote its own limit into its doc: *a speaking button's
//! words could still be a glyph the host's font does not carry.* R1952 asked
//! that question of the face this tree ships, with `Font::glyph_id_for`, and
//! the screen answered with **four characters it paints and the face cannot
//! draw** —
//! `U+2192`, `U+21AA`, `U+25BC` and `U+25BE`, across four of the eight
//! destinations. Each one is a box where a reader expects a mark.
//!
//! The face is not at fault and is not the thing to change. `NotoSans-Regular`
//! is a Latin/Greek/Cyrillic text face; arrows and geometric shapes live in
//! separate symbol faces, so **every** triangle, chevron and arrow a widget
//! draws as text is outside it by construction. R1674 had already drawn this
//! conclusion for the commonest mark in the catalog — the checkbox tick stopped
//! being `U+2713` and became a stroked polyline — and named the reason: *the
//! reference draws its check mark as a path too, which is why this class of
//! defect does not exist there.* This module is that conclusion applied to the
//! rest of the marks a widget draws.
//!
//! # Why a second vocabulary and not [`crate::control_mark`]
//!
//! [`ControlMark`](crate::control_mark::ControlMark) is *the face one CHROME
//! control draws* — what a title bar offers on a card or a panel — and
//! `ControlMark::every` is walked by gates asserting exactly that population.
//! A sort direction is not a control a band offers; it is a widget saying
//! which way its rows run. Widening that enum would put an arm in front of
//! every check that enumerates a band's controls, which is the mistake R1950
//! recorded one level down when it declined to widen
//! [`CardAffordance`](pinion_core::widgets::card::CardAffordance): a roster and
//! a face are two vocabularies, and so are two rosters.
//!
//! What the two DO share is the drawing: [`crate::control_mark`]'s `strokes`,
//! `CHEVRON` and `CROSS` are used from here, so a chevron drawn in a selector
//! and a chevron drawn on a fold control are one point set turned, not two
//! independent chances to draw a different shape.
//!
//! # Why the pairs are derived
//!
//! [`Indicator::Sort`] has two faces and [`Indicator::TakeOver`] /
//! [`Indicator::GiveBack`] are each other's mirror. Written out, each pair
//! would be two independent chances to point the wrong way, and nothing in a
//! screenshot compares them — R1697's defect, which R1950 fixed for the
//! maximise control by making the two faces two values of one derivation. Here
//! one drawing is mirrored, so `r1952_no_two_indicators_draw_the_same_mark`
//! can assert the pair comes out different **and** a reader knows why.

use pinion_core::scene::{ContainerNode, Rect, Scene};
use pinion_core::style::{Color, LayoutStyle, Size};
use pinion_core::voice::Silence;

use crate::control_mark::{CHEVRON, CROSS, fills, strokes};

/// ★★★★★ R2057 — the solid triangle this vocabulary points with, drawn once.
///
/// Apex up, base below it, so a sort arrow with `ascending` and a twisty with
/// its children showing are both this run turned toward what they mean. It was
/// a literal inside the sort arm until a second mark needed the same shape, at
/// which point two literals would have been two things free to stop looking
/// alike — the defect this tree keeps paying for one layer up, in addresses.
const TRIANGLE: [(i32, i32); 3] = [(-5, 3), (5, 3), (0, -4)];

/// ★★★★★ R2057 — the twisty's triangle, pointing along the row when folded.
///
/// NARROWER and taller than [`TRIANGLE`], and that is the design rather than an
/// accident of arithmetic: a sort arrow sits beside a word in a heading and is
/// read at a glance, while a twisty sits at the head of a row in a column only
/// as wide as itself and is pressed. Different jobs, different proportions.
///
/// ⚠ It also has to be a different run, and that constraint was MEASURED rather
/// than assumed. Drawing the twisty from `TRIANGLE` made an open one identical
/// to a descending sort — which the vocabulary's own uniqueness gate refused,
/// and rightly, for a reason stronger than the one that gate states:
/// [`face_of`] recovers a mark FROM PAINT by comparing path commands, so two
/// faces with one drawing make that reader answer the wrong one. A test asked
/// for the twisty and was handed `Sort`. Stroking instead of filling does not
/// rescue it either — `face_of` reads commands and not style.
const TWISTY: [(i32, i32); 3] = [(-3, -4), (-3, 4), (4, 0)];

/// The mark one widget draws beside a value — the state the value is in, or
/// what the seat beside it would do.
///
/// Closed, and matched exhaustively in [`scenes`]: a new arm stops the build
/// until it has a drawing, which is what stops the next indicator reaching a
/// window as an empty box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Indicator {
    /// Which way the rows under a sorted column run.
    Sort {
        /// `true` when the smallest value is at the top.
        ascending: bool,
    },
    /// A closed selector — press it and a list opens under it.
    Selector,
    /// An arrow that drops and turns toward the reader: this row's value came
    /// from somewhere else, and the seat takes it over.
    TakeOver,
    /// The mirror of [`TakeOver`](Self::TakeOver) — a shared row's written half
    /// going back where it came from. Mirrored rather than drawn again, because
    /// the act is the other one's mirror and a reader who has learned one has
    /// learned the other (R1717).
    GiveBack,
    /// ★★★★★ R2057 — the twisty at the head of a row that has children:
    /// `open` says whether they are showing.
    ///
    /// It is the SORT triangle given a quarter turn, and reuses that point set
    /// rather than declaring a second one — because the two marks are the same
    /// idea seen twice: *a solid triangle points at where the thing is*. A sort
    /// arrow points along the rows it orders; a twisty points at its children,
    /// down the page when they are showing and along the row when they are
    /// folded away. Drawing them from one run means a change to the shape
    /// cannot leave the pair looking like two different vocabularies.
    ///
    /// ⚠ Open twisty and descending sort therefore draw ALIKE, and that is
    /// argued rather than overlooked: a twisty sits at the head of a row and a
    /// sort arrow in a column heading, so no slot ever offers both, and every
    /// toolkit this class of widget comes from draws them alike for the same
    /// reason. The marks this vocabulary keeps APART are the ones a reader
    /// meets side by side — which is why a closed selector is a chevron and not
    /// a third triangle.
    Disclosure {
        /// `true` when this row's children are showing.
        open: bool,
    },
    /// A cross — take this row out.
    ///
    /// ⚠ It draws the same cross as
    /// [`ControlMark::Close`](crate::control_mark::ControlMark::Close), from
    /// the same point set, and that is deliberate rather than a missed merge:
    /// the two vocabularies answer different questions (*what does this band's
    /// control do* and *what does this row's seat do*) and only the drawing is
    /// shared. Merging the enums would put a row's seat in front of every gate
    /// that enumerates a chrome band.
    Discard,
}

impl Indicator {
    /// The narrowest slot these marks are drawn for, in logical pixels.
    ///
    /// Every drawing below is laid out from its slot's centre and reaches at
    /// most six pixels each way, so thirteen is the size at which the whole of
    /// one still lands inside the slot — the same floor and the same reason as
    /// [`ControlMark::MIN`](crate::control_mark::ControlMark::MIN). Below it
    /// [`scenes`] draws **nothing**, which is this crate's standing rule: a
    /// part that does not fit is not painted rather than painted smaller.
    pub const MIN: u32 = 13;

    /// The face a sorted column wears, or `None` when this column is not the
    /// sorted one.
    ///
    /// Takes the answer
    /// [`col_sort_dir`](pinion_core::widgets::grid_sort::col_sort_dir) gives,
    /// so the indicator IS the sort state and a header cannot show an arrow
    /// the rows disagree with. This replaces `glyph::sort_glyph`, which
    /// answered the same question with a character.
    #[must_use]
    pub const fn of_sort(dir: Option<bool>) -> Option<Self> {
        match dir {
            Some(ascending) => Some(Self::Sort { ascending }),
            None => None,
        }
    }

    /// Every mark this module paints, in a stable order — the population every
    /// gate here walks, so an arm added to the enum joins them without anybody
    /// remembering to.
    #[must_use]
    pub fn every() -> Vec<Self> {
        vec![
            Self::Sort { ascending: true },
            Self::Sort { ascending: false },
            Self::Selector,
            Self::Disclosure { open: true },
            Self::Disclosure { open: false },
            Self::TakeOver,
            Self::GiveBack,
            Self::Discard,
        ]
    }
}

/// **The mark, painted** into `rect`, read in the frame of whatever container
/// the scenes are put into.
///
/// `rect` is not required to sit at the origin: a widget's indicator usually
/// belongs in a slot at the trailing end of a cell, and both the stroked and
/// the filled arms are offset here so a caller passing that slot gets a mark
/// inside it.
///
/// Empty when `rect` is narrower or shorter than [`Indicator::MIN`] — see that
/// constant for why that is an absence of room rather than an exemption.
#[must_use]
pub fn scenes(mark: Indicator, rect: Rect, ink: Color) -> Vec<Scene> {
    if rect.w < Indicator::MIN || rect.h < Indicator::MIN {
        return Vec::new();
    }
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    match mark {
        // A solid triangle, because a sort indicator is read at a glance
        // beside a word rather than pointed at — the one place in this
        // vocabulary where weight is the point. `ascending` is the canonical
        // drawing and `descending` is it flipped, so the pair cannot disagree.
        Indicator::Sort { ascending } => {
            vec![fills(rect, &[flip_y(ascending, (cx, cy), &TRIANGLE)], ink)]
        }
        // ★★★★★ R2057 — pointed at where the children are: along the row while
        // they are folded away, down the page while they show. `TWISTY` folded
        // is the canonical drawing and open is it given one quarter turn, so
        // the pair cannot disagree — the derivation the sort pair and the
        // take-over pair both use.
        Indicator::Disclosure { open } => vec![fills(
            rect,
            &[if open {
                place((cx, cy), &TWISTY, |(dx, dy)| (-dy, dx))
            } else {
                place((cx, cy), &TWISTY, |d| d)
            }],
            ink,
        )],
        // The reference's own chevron, turned to point down. A selector opens
        // a list BELOW it, and the mark says so; drawn as a chevron rather than
        // as a second solid triangle so a closed selector and a sorted column
        // cannot be mistaken for each other.
        Indicator::Selector => vec![strokes(rect, &[turn_down((cx, cy), &CHEVRON)], ink, 1)],
        // An arrow that drops and turns right: this row's value came from
        // somewhere above it. `GiveBack` is the same drawing mirrored, which
        // is what "give my half back" means.
        Indicator::TakeOver | Indicator::GiveBack => {
            let rightwards = matches!(mark, Indicator::TakeOver);
            vec![strokes(
                rect,
                &[
                    flip_x(rightwards, (cx, cy), &[(-4, -5), (-4, 2), (4, 2)]),
                    flip_x(rightwards, (cx, cy), &[(1, -1), (4, 2), (1, 5)]),
                ],
                ink,
                1,
            )]
        }
        // The cross this tree already draws for a close control, from that
        // module's point set rather than from a second literal here.
        Indicator::Discard => vec![strokes(
            rect,
            CROSS.map(|run| place((cx, cy), &run, |d| d)).as_ref(),
            ink,
            1,
        )],
    }
}

/// A tagged, decorative box holding `mark`, filling the whole of `rect`.
///
/// The wrapper is what carries the address and the a11y declaration a text run
/// used to carry: a mark is not the fact — the direction is, and the heading it
/// sits in announces that — so the box says `decorative` with the sentence
/// naming what announces it instead (R1856's rule, unchanged by the mark
/// stopping being a character).
///
/// `reason` is that sentence. `rect` is slot-local: the box is placed at
/// `rect`, and the mark inside it is centred in the box.
///
/// ⚠ The box is **not** pointer-transparent, because the run it replaces was
/// not either: a press on a sort arrow is a press on that column's heading, and
/// making the slot transparent would move where that press lands.
#[must_use]
pub fn slot(
    tag: impl Into<String>,
    mark: Indicator,
    rect: Rect,
    ink: Color,
    reason: &str,
) -> Scene {
    let inner = Rect::new(0, 0, rect.w, rect.h);
    Scene::Container(
        ContainerNode::new(scenes(mark, inner, ink))
            .with_tag(tag.into())
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(rect.x, rect.y)
                    .with_size(Size::px(rect.w, rect.h)),
            ),
    )
    .silenced(Silence::decorative(reason.to_owned()))
}

/// The same mark as a box **in the flow**, `side` across, for a widget that
/// lays its header content out with flex rather than at absolute rectangles.
///
/// The peer of [`slot`], and it exists because the two callers genuinely
/// differ: a column header places its parts at rectangles it computes, and a
/// grid section is a flex row whose parts size themselves. A caller of this one
/// gets no address — the run it replaces carried none either, being a
/// presentational leaf inside the section's own clickable box.
///
/// `side` is floored at [`Indicator::MIN`], because a mark that does not fit is
/// not painted and a box that small in a flex row would be an empty gap where
/// the reader expects a mark. Handing it a smaller number is the caller
/// declining the mark, so it returns the empty container the flow can absorb.
#[must_use]
pub fn inline(mark: Indicator, side: u32, ink: Color, reason: &str) -> Scene {
    let inner = Rect::new(0, 0, side, side);
    Scene::Container(
        ContainerNode::new(scenes(mark, inner, ink))
            .with_layout(LayoutStyle::new().with_size(Size::px(side, side))),
    )
    .silenced(Silence::decorative(reason.to_owned()))
}

/// Which face `path` draws, or `None` when it draws something else.
///
/// ★★★★★ Matched against the drawings this module produces **at that path's own
/// size**, not against a table of point sets written somewhere. A checker with
/// its own copy of the shapes is a second source that goes stale the first time
/// a mark is redrawn — and the check would then be asserting the old drawing,
/// green, about a screen showing the new one.
///
/// This is what replaced "does the header contain the character `U+25B2`" as
/// the way a test asks whether a sorted column shows its direction.
#[must_use]
pub fn face_of(path: &pinion_core::scene::PathNode) -> Option<Indicator> {
    let side = Rect::new(0, 0, path.rect.w, path.rect.h);
    Indicator::every().into_iter().find(|face| {
        // The ink is irrelevant to the comparison — only the commands are
        // read — so any colour will do.
        scenes(*face, side, Color::TRANSPARENT)
            .iter()
            .any(|drawn| matches!(drawn, Scene::Path(p) if p.commands == path.commands))
    })
}

/// Every indicator drawn anywhere in `scene`, in paint order.
///
/// The census a gate walks: *which marks does this screen show*, answered from
/// the scene rather than from what the caller believes it asked for.
#[must_use]
pub fn marks_in(scene: &Scene) -> Vec<Indicator> {
    // Walked with the framework's own traversal rather than a private one, so
    // a node kind that grows children is not a place this census silently
    // stops looking.
    let mut out = Vec::new();
    scene.for_each_node(&mut |visit| {
        if let Scene::Path(path) = visit.node {
            out.extend(face_of(path));
        }
    });
    out
}

/// One drawing's offsets, kept as authored when `forward` and mirrored top to
/// bottom when not, placed around `centre`.
fn flip_y(forward: bool, centre: (u32, u32), run: &[(i32, i32)]) -> Vec<(u32, u32)> {
    place(
        centre,
        run,
        |(dx, dy)| if forward { (dx, dy) } else { (dx, -dy) },
    )
}

/// The same, mirrored left to right.
fn flip_x(forward: bool, centre: (u32, u32), run: &[(i32, i32)]) -> Vec<(u32, u32)> {
    place(
        centre,
        run,
        |(dx, dy)| if forward { (dx, dy) } else { (-dx, dy) },
    )
}

/// A leftward drawing turned to point down — the same quarter turn
/// [`crate::control_mark`] applies to reach `ChromeEdge::Bottom`, written here
/// because an indicator points at a list rather than at an edge of a window
/// and borrowing that vocabulary would say it does.
fn turn_down(centre: (u32, u32), run: &[(i32, i32)]) -> Vec<(u32, u32)> {
    place(centre, run, |(dx, dy)| (dy, -dx))
}

/// Offsets around `centre`, after `f`. Clamped at zero the way
/// [`crate::control_mark`] clamps: a slot at the origin narrower than
/// [`Indicator::MIN`] never reaches here.
fn place(
    centre: (u32, u32),
    run: &[(i32, i32)],
    f: impl Fn((i32, i32)) -> (i32, i32),
) -> Vec<(u32, u32)> {
    let (cx, cy) = centre;
    run.iter()
        .map(|d| {
            let (dx, dy) = f(*d);
            (
                u32::try_from(i64::from(cx) + i64::from(dx)).unwrap_or(0),
                u32::try_from(i64::from(cy) + i64::from(dy)).unwrap_or(0),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Indicator, scenes, slot};
    use pinion_core::scene::{PathCommand, Rect, Scene};
    use pinion_core::style::Color;
    use std::collections::BTreeMap;

    const INK: Color = Color::rgb(0xE8, 0xEB, 0xEF);

    /// Every point a mark's paths pass through, in slot-local coordinates —
    /// the ink itself, not a count of scenes, which is what a blank box would
    /// pass.
    fn ink_of(mark: Indicator, rect: Rect) -> Vec<(u32, u32)> {
        fn walk(scene: &Scene, out: &mut Vec<(u32, u32)>) {
            match scene {
                Scene::Path(path) => {
                    for command in &path.commands {
                        match command {
                            PathCommand::MoveTo(p) | PathCommand::LineTo(p) => {
                                #[allow(
                                    clippy::cast_possible_truncation,
                                    clippy::cast_sign_loss,
                                    reason = "mark coordinates are small non-negative integers"
                                )]
                                out.push((path.rect.x + p.x as u32, path.rect.y + p.y as u32));
                            }
                            _ => {}
                        }
                    }
                }
                Scene::Container(container) => {
                    for child in &container.children {
                        walk(child, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for scene in scenes(mark, rect, INK) {
            walk(&scene, &mut out);
        }
        out
    }

    /// R1952 — **the defect this module exists for**: every face in the
    /// vocabulary puts ink inside its slot, at every origin and every size a
    /// caller can hand it.
    ///
    /// The population is [`Indicator::every`] rather than a list here, so an
    /// arm added to the enum joins this without anybody remembering.
    #[test]
    fn r1952_every_indicator_puts_ink_in_its_slot() {
        for origin in [(0, 0), (1, 1), (7, 3)] {
            for size in [Indicator::MIN, 20, 24] {
                let rect = Rect::new(origin.0, origin.1, size, size);
                for mark in Indicator::every() {
                    let ink = ink_of(mark, rect);
                    assert!(
                        !ink.is_empty(),
                        "{mark:?} draws nothing in {rect:?}, which is the blank \
                         box this module exists to make impossible",
                    );
                    for (x, y) in ink {
                        assert!(
                            x >= rect.x
                                && y >= rect.y
                                && x <= rect.x + rect.w
                                && y <= rect.y + rect.h,
                            "{mark:?} puts ink at ({x}, {y}), outside its slot {rect:?}",
                        );
                    }
                }
            }
        }
    }

    /// A slot under [`Indicator::MIN`] draws nothing rather than a mark
    /// squeezed out of shape — the rule stated on that constant, performed.
    #[test]
    fn r1952_a_slot_too_small_draws_nothing() {
        for side in 0..Indicator::MIN {
            for mark in Indicator::every() {
                assert!(
                    scenes(mark, Rect::new(0, 0, side, side), INK).is_empty(),
                    "{mark:?} drew into a {side}px slot, under the floor it declares",
                );
            }
        }
    }

    /// ★★★★★ No two faces draw the same mark.
    ///
    /// This is what the derived pairs buy: `Sort` mirrored on the wrong axis,
    /// or `Returned` authored as a copy of `Inherited`, both come out here as
    /// two faces with one point set — and neither is visible in a screenshot,
    /// because each mark looks perfectly reasonable on its own.
    #[test]
    fn r1952_no_two_indicators_draw_the_same_mark() {
        let rect = Rect::new(0, 0, 24, 24);
        // ★★★★★ R2057 — this check is STRONGER than the sentence it carries,
        // and R2057 learned that by trying to argue with it. A twisty drawn
        // from the sort triangle is not a reader's problem alone: `face_of`
        // recovers a mark from paint by comparing commands, so two faces with
        // one drawing make that reader answer the wrong one — measured, a test
        // asked for a twisty and was handed a sort arrow. So "no two faces draw
        // alike" is not an aesthetic rule that may be argued away with a
        // declared twin; it is what keeps the paint READABLE.
        let mut seen: BTreeMap<Vec<(u32, u32)>, Indicator> = BTreeMap::new();
        for mark in Indicator::every() {
            if let Some(other) = seen.insert(ink_of(mark, rect), mark) {
                panic!(
                    "{mark:?} draws the same points as {other:?}, so the two say \
                     the same thing to a reader AND `face_of` cannot tell them \
                     apart when reading a scene back",
                );
            }
        }
        assert_eq!(seen.len(), Indicator::every().len());
    }

    /// The sort face IS the sort answer: `None` shows nothing, and the two
    /// directions are two faces.
    #[test]
    fn r1952_the_sort_face_is_the_sort_answer() {
        assert_eq!(Indicator::of_sort(None), None);
        assert_eq!(
            Indicator::of_sort(Some(true)),
            Some(Indicator::Sort { ascending: true })
        );
        assert_ne!(
            Indicator::of_sort(Some(true)),
            Indicator::of_sort(Some(false))
        );
    }

    /// The slot carries the address and the a11y declaration the text run used
    /// to carry, and the mark is inside it.
    #[test]
    fn r1952_a_slot_carries_the_address_and_says_what_announces_it() {
        let scene = slot(
            "head_sort#0",
            Indicator::Sort { ascending: true },
            Rect::new(40, 2, 24, 20),
            INK,
            "the heading announces the direction",
        );
        let Scene::Container(container) = &scene else {
            panic!("a slot is a container");
        };
        assert_eq!(container.tag.as_deref(), Some("head_sort#0"));
        assert!(
            container.layout.silence.is_some(),
            "the slot declares itself decorative, or the mark is announced as \
             a second answer to a question the heading already answers",
        );
        assert!(!container.children.is_empty(), "the slot holds its mark");
    }
}
