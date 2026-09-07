//! R1950 §5.27 §5.50 — **the mark a chrome control draws, over every control
//! any chrome band in this tree offers.**
//!
//! # The report this answers
//!
//! A person looked at the running window on 2026-09-01 and said a panel's
//! title-bar buttons were plain grey with no icon inside them. They were, and
//! the cause measured at R1949 was not a painter ignoring a declaration: a
//! panel's two acts — move to another edge, fold away to a strip — **were in
//! no mark vocabulary at all**. A card's four affordances were, and drew; the
//! panel's two were not, and drew an outlined box with nothing in it.
//!
//! ⇒ the repair is not an icon. It is that ONE vocabulary names every chrome
//! control this tree paints and this painter is **total over it**, so a
//! control added to any band cannot ship blank: the build stops until somebody
//! says what it looks like.
//!
//! # Why the vocabulary is not the card's
//!
//! [`CardAffordance`] is a *card's roster* — what one card's header offers —
//! and live sites read it as exactly that: `CardChrome::full` means "every
//! affordance, on one card", and the shell's board asserts its cards exercise
//! every arm of it. Widening that enum with a panel's acts would have made
//! `full()` offer a card two controls a card cannot perform. So the roster and
//! the FACE are two vocabularies, and this is the face one.
//!
//! # Why a face rather than a roster arm plus a flag
//!
//! R1697 wrote the rule on the maximise control: `restore` is that control's
//! *other face*, and a control that toggles without changing its mark tells a
//! person the same thing in both states. Carried as a `bool` beside the
//! affordance, that rule was the caller's to remember. Here the two faces are
//! two values — [`Maximize`](ControlMark::Maximize) and
//! [`Restore`](ControlMark::Restore) — so the painter has nothing to be told
//! and no caller can forget.
//!
//! # Why not [`crate::glyph`]
//!
//! That module is *text*: characters a font supplies, laid out with the run
//! around them. These are stroked paths, which is what lets a directional mark
//! be **derived** — one canonical drawing pointing at [`ChromeEdge::Left`],
//! turned onto whichever edge the control acts toward. Spelled as characters,
//! the four directions would be four independent choices that nothing
//! compares, and a flip control whose arrow points the wrong way is R1697's
//! defect one level down.

use pinion_core::edge_panel::{PanelAffordance, PanelControl};
use pinion_core::scene::{ContainerNode, PathCommand, PathNode, PathPoint, Rect, Scene};
use pinion_core::style::{BoxStyle, ChromeEdge, Color, LayoutStyle, PathStyle, Stroke};
use pinion_core::widgets::card::CardAffordance;

/// The face one chrome control draws.
///
/// Closed, and matched exhaustively in [`scenes`], which is the whole point: a
/// new arm stops the build until it has a mark, so a control cannot reach a
/// window blank the way a panel's two did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ControlMark {
    /// Three dots — open this thing's own configuration.
    Settings,
    /// A square lifting out of another — detach into a window of its own.
    TearOff,
    /// ★★★★★ R2059 — a bar across the middle: put this window out of the way.
    ///
    /// Added when the dock's window controls stopped being characters. The trio
    /// they draw — minimise, maximise, close — had two faces here already and
    /// this one only as `U+2212`; a trio drawn half in text and half in paths is
    /// a trio free to stop looking like each other, so the third joins them.
    ///
    /// ⚠ It is the widest mark in this vocabulary at its own centre line and
    /// nothing else here is a lone horizontal, which is what keeps it apart
    /// from [`Close`](Self::Close)'s two diagonals — the uniqueness gate below
    /// is what says so rather than this sentence.
    Minimize,
    /// One square — fill the board with this.
    Maximize,
    /// Two overlapping squares — the maximise control's other face, bringing
    /// back the arrangement there was before.
    Restore,
    /// A cross — take this off the board.
    Close,
    /// An arrow into a bar — move the panel to the edge the arrow points at.
    Flip {
        /// The edge the panel would land on.
        to: ChromeEdge,
    },
    /// A double chevron — fold the panel to a strip against the edge it is on.
    Fold {
        /// The edge it collapses into.
        to: ChromeEdge,
    },
}

impl ControlMark {
    /// The narrowest slot these marks are drawn for, in logical pixels.
    ///
    /// Every mark here is laid out from its slot's centre and reaches six
    /// pixels each way, so thirteen is the size at which the whole of one
    /// still lands inside the slot. Below it [`scenes`] draws **nothing**,
    /// which is this crate's standing rule — *a part that does not fit is not
    /// painted, rather than painted smaller* — and not an exemption from the
    /// gate: the screen-side check asks the painted window for ink inside each
    /// control it offers, so a slot that shrank under this is RED there rather
    /// than a silently distorted mark.
    pub const MIN: u32 = 13;

    /// The face a card's affordance wears, given whether its maximise control
    /// is showing its restore side.
    #[must_use]
    pub const fn of_card(affordance: CardAffordance, restore: bool) -> Self {
        match affordance {
            CardAffordance::Settings => Self::Settings,
            CardAffordance::TearOff => Self::TearOff,
            CardAffordance::Maximize if restore => Self::Restore,
            CardAffordance::Maximize => Self::Maximize,
            CardAffordance::Close => Self::Close,
        }
    }

    /// The face a panel's chrome control wears.
    ///
    /// Takes the whole [`PanelControl`] rather than its act, because the edge
    /// a control points at is decided by the policy that offered it — a flip
    /// names where the panel would GO and a fold where it would collapse to,
    /// and a caller re-deriving either would be a second answer to a question
    /// the declaration has already answered.
    #[must_use]
    pub const fn of_panel(control: PanelControl) -> Self {
        match control.act {
            PanelAffordance::Flip => Self::Flip { to: control.toward },
            PanelAffordance::Fold => Self::Fold { to: control.toward },
        }
    }

    /// Every face this tree paints, in a stable order.
    ///
    /// Derived over [`ChromeEdge::ALL`] rather than written out, so an edge
    /// added to that vocabulary grows this roster — and every gate that walks
    /// it grows with it — instead of leaving a direction nothing ever draws.
    #[must_use]
    pub fn every() -> Vec<Self> {
        let mut out = vec![
            Self::Settings,
            Self::TearOff,
            Self::Minimize,
            Self::Maximize,
            Self::Restore,
            Self::Close,
        ];
        out.extend(ChromeEdge::ALL.into_iter().map(|to| Self::Flip { to }));
        out.extend(ChromeEdge::ALL.into_iter().map(|to| Self::Fold { to }));
        out
    }
}

/// **The mark, painted** into `rect`, which is read in the frame of whatever
/// container the scenes are put into.
///
/// ⚠ `rect` is not required to sit at the origin, and R1950 found out why that
/// matters on this function's first non-card caller. A bordered button's marks
/// belong in its CONTENT box — `(1, 1, w - 2, h - 2)` for a one-pixel frame —
/// and the two arms here place themselves differently: a stroked path is laid
/// out at `rect` and carries commands relative to its own origin, while a dot
/// is a positioned box in the parent's frame. Both are offset here so that a
/// caller passing an inset rect gets a mark inside it, which is what
/// `r1950_every_mark_puts_ink_in_its_slot` asks at an offset.
///
/// Empty when `rect` is narrower or shorter than [`ControlMark::MIN`] — see
/// that constant for why that is an absence of room rather than an exemption.
#[must_use]
pub fn scenes(mark: ControlMark, rect: Rect, ink: Color) -> Vec<Scene> {
    if rect.w < ControlMark::MIN || rect.h < ControlMark::MIN {
        return Vec::new();
    }
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    match mark {
        ControlMark::Settings => (0..3)
            .map(|n| dot(rect.x + cx - 1, rect.y + cy - 5 + n * 5, 2, ink))
            .collect(),
        // A square lifting out of another.
        ControlMark::TearOff => vec![strokes(
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
        ControlMark::Restore => vec![strokes(
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
        // ★ R2059 — one horizontal bar, the same half-width the square's sides
        // reach, so the minimise and maximise controls read as one family.
        ControlMark::Minimize => vec![strokes(rect, &[vec![(cx - 5, cy), (cx + 5, cy)]], ink, 1)],
        ControlMark::Maximize => vec![strokes(
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
        ControlMark::Close => vec![strokes(
            rect,
            CROSS
                .map(|run| {
                    run.iter()
                        .map(|(dx, dy)| {
                            (
                                u32::try_from(i64::from(cx) + i64::from(*dx)).unwrap_or(0),
                                u32::try_from(i64::from(cy) + i64::from(*dy)).unwrap_or(0),
                            )
                        })
                        .collect()
                })
                .as_ref(),
            ink,
            1,
        )],
        // A bar along the edge the panel would land on, and an arrow crossing
        // the slot into it. The bar is what makes this read as *docking to that
        // side* rather than as *scrolling that way*.
        ControlMark::Flip { to } => vec![strokes(
            rect,
            &directed(
                to,
                (cx, cy),
                &[
                    &[(-6, -6), (-6, 6)],
                    &[(5, 0), (-3, 0)],
                    &[(1, -4), (-3, 0), (1, 4)],
                ],
            ),
            ink,
            1,
        )],
        // ★★★★★ R1951 — ONE chevron pointing at the edge it collapses into, and
        // no bar, which is what tells this apart from a flip at this size.
        //
        // The points are the behaviour reference's own, read out of it this
        // round: its collapse control is a 20-unit box carrying
        // `8,5 13,10 8,15` — a single chevron whose apex is five units from the
        // centre — and this is that drawing, expressed from the centre so it can
        // be turned onto any edge. R1950 drew TWO chevrons here, which was this
        // module's invention rather than a reproduction; the reference was not
        // asked until the round that first put this mark on a screen the
        // reference also draws.
        ControlMark::Fold { to } => {
            vec![strokes(rect, &directed(to, (cx, cy), &[&CHEVRON]), ink, 1)]
        }
    }
}

/// **The chevron**, as offsets from the slot's centre, pointing at
/// [`ChromeEdge::Left`].
///
/// ★★★★★ R1951 read this out of the behaviour reference rather than inventing
/// it: the reference's collapse control is a 20-unit box carrying the polyline
/// `8,5 13,10 8,15` — a single chevron whose apex is five units from the
/// centre — and this is that drawing expressed from the centre so it can be
/// turned onto any edge. R1950 had drawn two chevrons, which was this module's
/// invention; the reference was not asked until the round that first put the
/// mark on a screen the reference also draws.
///
/// ★ R1952 made it a `const` when [`crate::indicator`] needed the same shape
/// for a closed selector. A chevron drawn in two places from two literals is
/// two chances to draw a different shape, and nothing in a screenshot compares
/// them — the Rule-of-Three miss [[self-grep-count-all-sites-not-just-new-pair]]
/// names, caught at the pair.
pub(crate) const CHEVRON: [(i32, i32); 3] = [(2, -5), (-3, 0), (2, 5)];

/// **The cross**, as two runs of offsets from the slot's centre.
///
/// ★ R1952 made it a `const` when [`crate::indicator`] needed the same shape
/// for a row's discard seat — the same reason as [`CHEVRON`] above, caught at
/// the pair. The two vocabularies stay separate (a band's close control and a
/// row's seat answer different questions); only the drawing is shared, so the
/// two crosses cannot come out looking like different ideas.
pub(crate) const CROSS: [[(i32, i32); 2]; 2] = [[(-4, -4), (4, 4)], [(4, -4), (-4, 4)]];

/// One canonical drawing, pointing at [`ChromeEdge::Left`], turned to point at
/// `edge` and placed around `centre`.
///
/// ★ The four directions are ONE drawing turned, not four drawings. Written
/// out per edge they would be four independent chances to point the wrong way
/// and nothing in a screenshot compares them; derived, a gate can assert the
/// four come out different from one another.
fn directed(edge: ChromeEdge, centre: (u32, u32), runs: &[&[(i32, i32)]]) -> Vec<Vec<(u32, u32)>> {
    let (cx, cy) = centre;
    runs.iter()
        .map(|run| {
            run.iter()
                .map(|d| {
                    let (dx, dy) = turn(edge, *d);
                    (
                        u32::try_from(i64::from(cx) + i64::from(dx)).unwrap_or(0),
                        u32::try_from(i64::from(cy) + i64::from(dy)).unwrap_or(0),
                    )
                })
                .collect()
        })
        .collect()
}

/// The offset `d` of a leftward drawing, as seen once that drawing points at
/// `edge`.
const fn turn(edge: ChromeEdge, d: (i32, i32)) -> (i32, i32) {
    let (dx, dy) = d;
    match edge {
        ChromeEdge::Left => (dx, dy),
        ChromeEdge::Right => (-dx, -dy),
        ChromeEdge::Top => (-dy, dx),
        ChromeEdge::Bottom => (dy, -dx),
    }
}

/// A filled circle `size` across, at `(x, y)` in the slot's own frame.
pub(crate) fn dot(x: u32, y: u32, size: u32, fill: Color) -> Scene {
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_style(BoxStyle::filled(fill).with_corner_radius(size / 2))
            .with_layout(absolute(Rect::new(x, y, size, size))),
    )
}

/// The layout a mark, and the slot holding it, are placed with.
///
/// R2032 — the framework publishes it: this was one of nine hand copies, and
/// the screen with no copy to reach for is the one that dropped a half.
pub(crate) fn absolute(rect: Rect) -> LayoutStyle {
    LayoutStyle::decoration(rect)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "header coordinates are < 2^13, exactly representable in f32"
)]
fn point(x: u32, y: u32) -> PathPoint {
    PathPoint::new(x as f32, y as f32)
}

/// A stroked polyline set in `rect`-local coordinates.
pub(crate) fn strokes(rect: Rect, runs: &[Vec<(u32, u32)>], ink: Color, width: u32) -> Scene {
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

/// A **filled** closed polygon set in `rect`-local coordinates.
///
/// R1952 — the peer of [`strokes`], for a mark whose weight is the point: a
/// sort indicator is read at a glance beside a word, and an outlined triangle
/// at that size reads as a smudge. Each run is closed back to its own first
/// point, so a caller writes the corners and not the return.
pub(crate) fn fills(rect: Rect, runs: &[Vec<(u32, u32)>], ink: Color) -> Scene {
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
        if let Some((x, y)) = run.first() {
            commands.push(PathCommand::LineTo(point(*x, *y)));
        }
    }
    Scene::Path(PathNode::new(rect, commands, PathStyle::filled(ink)).with_layout(absolute(rect)))
}

#[cfg(test)]
mod tests {
    use super::{ControlMark, scenes};
    use pinion_core::edge_panel::{PanelAffordance, PanelControl};
    use pinion_core::scene::{Rect, Scene};
    use pinion_core::style::{ChromeEdge, Color};
    use pinion_core::widgets::card::CardAffordance;
    use std::collections::BTreeSet;

    const INK: Color = Color::rgb(0xE8, 0xEB, 0xEF);

    /// Every point a mark's paths pass through, in slot-local coordinates —
    /// the ink itself, not a count of scenes.
    ///
    /// A count is what a blank control passes: a container with no children is
    /// a scene. This walks down to the path commands and the filled dots, so
    /// "there is a mark here" means points a renderer would put ink on.
    fn ink_of(mark: ControlMark, rect: Rect) -> Vec<(u32, u32)> {
        fn walk(scene: &Scene, out: &mut Vec<(u32, u32)>) {
            match scene {
                Scene::Path(path) => {
                    // A path's commands are relative to its own resolved rect
                    // (the adapter enters that origin as a translate), so the
                    // origin is added back here. Reading them as absolute is
                    // how a mark drawn in an inset content box would look right
                    // to a checker and land a pixel out on screen.
                    for command in &path.commands {
                        match command {
                            pinion_core::scene::PathCommand::MoveTo(p)
                            | pinion_core::scene::PathCommand::LineTo(p) => {
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
                    // A container is ink only when it actually fills. The
                    // marks' dots do; an empty grouping container does not,
                    // which is exactly the difference between a control with a
                    // mark and the blank box this module was written for.
                    if container.style.fill.a > 0 {
                        let (x, y) = container.layout.absolute_position.unwrap_or((0, 0));
                        out.push((x, y));
                    }
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

    /// R1950 — **the defect this module exists for**: every face in the
    /// vocabulary puts ink inside its slot.
    ///
    /// The population is [`ControlMark::every`] rather than a list written
    /// here, so a face added to the enum joins this without anybody
    /// remembering to add it — which is the half that stops the next blank
    /// control, rather than this one.
    /// ★ The offset origins are the half that found a real defect: a chrome
    /// button with a border wants its mark in the CONTENT box, and the first
    /// caller to ask for one drew a mark a pixel outside its own button on
    /// every side — caught by a screen gate five rounds older than this module.
    #[test]
    fn r1950_every_mark_puts_ink_in_its_slot() {
        for origin in [(0, 0), (1, 1), (7, 3)] {
            for size in [ControlMark::MIN, 18, 24] {
                let rect = Rect::new(origin.0, origin.1, size, size);
                for mark in ControlMark::every() {
                    let ink = ink_of(mark, rect);
                    assert!(
                        !ink.is_empty(),
                        "{mark:?} draws nothing at {size}x{size} — a control a person \
                         cannot read"
                    );
                    for (x, y) in ink {
                        assert!(
                            x >= rect.x && x < rect.x + size && y >= rect.y && y < rect.y + size,
                            "{mark:?} puts ink at ({x}, {y}), outside the \
                             {size}x{size} slot at {origin:?}"
                        );
                    }
                }
            }
        }
    }

    /// R1950 — the faces are told APART, which is what a count of paths cannot
    /// report.
    ///
    /// R1949's lesson, applied to marks: two controls that draw the same thing
    /// are what "the direction is ignored" looks like from outside, and every
    /// gate that only counts ink passes it.
    #[test]
    fn r1950_no_two_faces_draw_the_same_mark() {
        let rect = Rect::new(0, 0, 18, 18);
        let mut seen: BTreeSet<Vec<(u32, u32)>> = BTreeSet::new();
        for mark in ControlMark::every() {
            let ink = ink_of(mark, rect);
            assert!(
                seen.insert(ink),
                "{mark:?} draws exactly what another face draws"
            );
        }
    }

    /// ★★★★★ R1951 — **the fold mark is the behaviour reference's drawing**,
    /// point for point, and not merely *a* chevron.
    ///
    /// # Why a reproduction needs a predicate and not a sentence
    ///
    /// The screen this mark landed on is one the reference also draws, and
    /// until this round it drew a twelve-by-two bar there — an invention
    /// nobody had compared with anything. The ink gate could not see that: a
    /// bar is ink, so "there is something in the box" was already true. What
    /// no gate asked was whether the something is *what the reference draws*.
    ///
    /// The reference's collapse control is a 20-unit box carrying the polyline
    /// below, measured out of it at R1951. Expressed from that box's centre it
    /// is an apex three units one way and two ends two units the other, five
    /// up and five down — which is exactly what [`scenes`] emits for
    /// [`ControlMark::Fold`], and this asserts the two are the same points
    /// rather than the same adjective.
    ///
    /// ⚠ The comparison is made at the reference's own scale. A slot bigger
    /// than 20 units draws the same mark rather than a scaled one, which is a
    /// deliberate difference and the reason this pins the SHAPE at one size
    /// instead of a ratio at every size.
    #[test]
    fn r1951_the_fold_mark_is_the_references_own_chevron() {
        /// The reference's collapse polyline, in its own 20-unit box.
        const REFERENCE_CHEVRON: [(i32, i32); 3] = [(8, 5), (13, 10), (8, 15)];
        /// The centre of that box.
        const REFERENCE_CENTRE: (i32, i32) = (10, 10);

        // The reference's chevron points RIGHT (its apex is at x=13, past the
        // centre), and its palette is the right-hand drawer — so the face to
        // compare is the one that folds toward the right edge.
        let rect = Rect::new(0, 0, 20, 20);
        let ours = ink_of(
            ControlMark::Fold {
                to: ChromeEdge::Right,
            },
            rect,
        );
        let theirs: Vec<(u32, u32)> = REFERENCE_CHEVRON
            .into_iter()
            .map(|(x, y)| {
                #[allow(
                    clippy::cast_sign_loss,
                    reason = "the reference's points are inside its own box"
                )]
                (
                    (REFERENCE_CENTRE.0 + (x - REFERENCE_CENTRE.0)) as u32,
                    (REFERENCE_CENTRE.1 + (y - REFERENCE_CENTRE.1)) as u32,
                )
            })
            .collect();
        assert_eq!(
            ours.len(),
            theirs.len(),
            "the reference draws {} point(s) and this draws {}",
            theirs.len(),
            ours.len()
        );
        // Compared as a SET: a polyline drawn end-to-start is the same chevron,
        // and the direction it is walked is not something a reader can see.
        let ours_set: BTreeSet<_> = ours.iter().copied().collect();
        let theirs_set: BTreeSet<_> = theirs.iter().copied().collect();
        assert_eq!(
            ours_set, theirs_set,
            "the fold mark is not the chevron the reference draws"
        );
    }

    /// R1950 — a mark that points somewhere points there **because of the
    /// edge**, and turning it twice brings it back.
    ///
    /// The second half is what makes the derivation checkable rather than
    /// merely present: `Left` and `Right` are opposite turns of one drawing, so
    /// a `turn` that dropped its edge would fail this while still drawing ink.
    #[test]
    fn r1950_a_directional_mark_turns_with_its_edge() {
        let rect = Rect::new(0, 0, 18, 18);
        for act in [PanelAffordance::Flip, PanelAffordance::Fold] {
            let by_edge: Vec<_> = ChromeEdge::ALL
                .into_iter()
                .map(|toward| ink_of(ControlMark::of_panel(PanelControl { act, toward }), rect))
                .collect();
            let distinct: BTreeSet<_> = by_edge.iter().collect();
            assert_eq!(
                distinct.len(),
                ChromeEdge::ALL.len(),
                "{act:?} draws the same mark for two different edges"
            );
            let (left, right) = (
                ink_of(
                    ControlMark::of_panel(PanelControl {
                        act,
                        toward: ChromeEdge::Left,
                    }),
                    rect,
                ),
                ink_of(
                    ControlMark::of_panel(PanelControl {
                        act,
                        toward: ChromeEdge::Right,
                    }),
                    rect,
                ),
            );
            for (n, ((lx, ly), (rx, ry))) in left.iter().zip(right.iter()).enumerate() {
                assert_eq!(
                    (
                        i64::from(*lx) + i64::from(*rx),
                        i64::from(*ly) + i64::from(*ry)
                    ),
                    (18, 18),
                    "{act:?}'s point {n} is not the mirror of its opposite edge's"
                );
            }
        }
    }

    /// R1950 — the maximise control's two faces are two values, so nothing has
    /// to be told which one to draw twice.
    #[test]
    fn r1950_a_cards_maximise_has_two_faces() {
        assert_eq!(
            ControlMark::of_card(CardAffordance::Maximize, false),
            ControlMark::Maximize
        );
        assert_eq!(
            ControlMark::of_card(CardAffordance::Maximize, true),
            ControlMark::Restore
        );
        let rect = Rect::new(0, 0, 18, 18);
        assert_ne!(
            ink_of(ControlMark::Maximize, rect),
            ink_of(ControlMark::Restore, rect),
            "a control that toggles without changing its mark says the same thing twice"
        );
    }

    /// R1950 — a slot too small for a mark gets none, rather than a distorted
    /// one. The screen-side gate is what refuses such a slot; here the rule is
    /// only that the painter does not invent a smaller drawing.
    #[test]
    fn r1950_a_slot_under_the_minimum_gets_no_mark() {
        let rect = Rect::new(0, 0, ControlMark::MIN - 1, ControlMark::MIN - 1);
        for mark in ControlMark::every() {
            assert!(
                scenes(mark, rect, INK).is_empty(),
                "{mark:?} drew into a slot smaller than it fits"
            );
        }
    }
}
