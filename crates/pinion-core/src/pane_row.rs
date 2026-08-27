//! R1860 §5.11 §5.2 — **a row of panes that share a width, each declaring the
//! width below which it cannot draw what it holds.**
//!
//! # The defect this comes from
//!
//! A three-pane screen in this tree declared its minimum width as
//! `list_floor + tree_width + bytes_width`: one *derived floor* plus two
//! *design widths standing in for floors*. The two side panes had no floor at
//! all — the width the specification drew them at was doing that job — so the
//! only way to serve a window narrower than the design arrangement was to lay
//! out at the design arrangement anyway and paint past the window's edge. The
//! application it is a page of grants it 37 pixels less than that, and it did,
//! and a reader reported the right outline of a box cut off.
//!
//! The lesson the same file already recorded twice, one level further down, is
//! that **a floor somebody picks can be wrong about the thing it is a floor
//! FOR**. This is that lesson at the level of a pane.
//!
//! # What this is, and why it is not a splitter
//!
//! The mature retained-mode toolkits this project is judged against reach a
//! comparable arrangement with a splitter and per-widget minimum-size hints,
//! and two things differ:
//!
//! * a splitter's floor is whatever a person last dragged it to, or a hint an
//!   author wrote beside the widget. Here [`Pane::floor`] is a value the pane
//!   **declares**, and [`PaneRow::floor`] is the same number the screen
//!   declares to the window through its shrink policy — so the width a screen
//!   says it lays out in and the widths its panes can actually draw in cannot
//!   be two different claims;
//! * the distribution is **total**: [`PaneRow::share`] never returns a width
//!   below a pane's floor, and never returns a set of widths that fails to
//!   tile the row. Which is checkable, and [`PaneRow::share`]'s own tests check
//!   it rather than describing it.
//!
//! # The rule
//!
//! One pane of the row is **flexible** — it takes whatever is left, and has a
//! floor of its own. The rest are **fixed**: each keeps its design width while
//! there is room, and gives width back only when there is not.
//!
//! A shortfall is shared **in proportion to each pane's own slack**, so the
//! pane with less to give reaches its floor no sooner than the other one does.
//! Sharing it evenly would bottom out the tighter pane first and then have to
//! re-spread the remainder, which is the same answer by a longer route on two
//! panes and a different, worse answer on three.

/// What a row of panes cannot be built with.
///
/// Reported as a value by [`Pane::checked`]; [`Pane::new`] turns each into a
/// panic, which in the `const` context a screen declares its panes in is a
/// **compile error**.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// A pane cannot draw in the width it is drawn at.
    FloorAboveDesign {
        /// The width the design draws it at.
        design: u32,
        /// The width it says it needs.
        floor: u32,
    },
}

/// One pane of a row: the width the design draws it at, and the width below
/// which it cannot draw what it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pane {
    design: u32,
    floor: u32,
}

impl Pane {
    /// A pane that keeps `design` while there is room and gives back no further
    /// than `floor`.
    ///
    /// # Panics
    ///
    /// On any [`Fault`] — in a `const` context, at compile time.
    #[must_use]
    pub const fn new(design: u32, floor: u32) -> Self {
        match Self::checked(design, floor) {
            Ok(pane) => pane,
            Err(Fault::FloorAboveDesign { .. }) => {
                panic!("pane row: a pane's floor is above the width it is drawn at")
            }
        }
    }

    /// [`Self::new`] with the refusal as a value.
    ///
    /// # Errors
    ///
    /// [`Fault::FloorAboveDesign`] when the pane cannot draw in its own design
    /// width, which is a contradiction rather than a tight fit.
    pub const fn checked(design: u32, floor: u32) -> Result<Self, Fault> {
        if floor > design {
            return Err(Fault::FloorAboveDesign { design, floor });
        }
        Ok(Self { design, floor })
    }

    /// A pane that gives nothing back: it is the same width at every size.
    ///
    /// The honest spelling of a pane whose content is a fixed lattice with no
    /// slack in it — a *declaration*, not the absence of one.
    #[must_use]
    pub const fn rigid(width: u32) -> Self {
        Self {
            design: width,
            floor: width,
        }
    }

    /// The width the design draws this pane at.
    #[must_use]
    pub const fn design(self) -> u32 {
        self.design
    }

    /// The width below which this pane cannot draw what it holds.
    #[must_use]
    pub const fn floor(self) -> u32 {
        self.floor
    }

    /// What this pane can give back before it reaches its floor.
    #[must_use]
    pub const fn slack(self) -> u32 {
        self.design - self.floor
    }
}

/// A row of fixed panes beside one flexible pane that takes what is left.
///
/// `Copy`, and it borrows a `'static` slice, so a screen declares one as a
/// `const` and every reader of any of its numbers reads *that* value — the same
/// discipline [`crate::shrink::ShrinkPolicy`] keeps, and for the same reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneRow {
    flexible_floor: u32,
    fixed: &'static [Pane],
}

impl PaneRow {
    /// A row whose flexible pane cannot draw below `flexible_floor`, beside
    /// `fixed`.
    #[must_use]
    pub const fn new(flexible_floor: u32, fixed: &'static [Pane]) -> Self {
        Self {
            flexible_floor,
            fixed,
        }
    }

    /// The width at which every pane is at its design width and the flexible
    /// one is exactly at its floor.
    ///
    /// This is the *design arrangement's* minimum, and it is **not** what a
    /// screen should declare it lays out in — see [`Self::floor`], which is.
    #[must_use]
    pub const fn design(&self) -> u32 {
        let mut total = self.flexible_floor;
        let mut n = 0;
        while n < self.fixed.len() {
            total += self.fixed[n].design;
            n += 1;
        }
        total
    }

    /// The narrowest width this row lays out in: **every pane at its own
    /// floor**.
    ///
    /// What a screen declares to the window, so that the width it says it lays
    /// out in is derived from what its panes can draw rather than from what
    /// they are drawn at.
    #[must_use]
    pub const fn floor(&self) -> u32 {
        let mut total = self.flexible_floor;
        let mut n = 0;
        while n < self.fixed.len() {
            total += self.fixed[n].floor;
            n += 1;
        }
        total
    }

    /// What the fixed panes can give back together.
    #[must_use]
    pub const fn slack(&self) -> u32 {
        self.design() - self.floor()
    }

    /// The width of the flexible pane, and of each fixed pane in order, when
    /// the row is given `total`.
    ///
    /// The widths always sum to `total` and no fixed pane is ever below its
    /// floor. Below [`Self::floor`] the row cannot be served at all — every
    /// pane is already at its floor — so the flexible pane absorbs what is left
    /// and the caller is responsible for not asking, which a window that floors
    /// at [`Self::floor`] cannot do.
    #[must_use]
    pub fn share(&self, total: u32) -> (u32, Vec<u32>) {
        let mut owed = u64::from(self.design().saturating_sub(total)).min(u64::from(self.slack()));
        let mut slack_left = u64::from(self.slack());
        let mut widths = Vec::with_capacity(self.fixed.len());
        let mut fixed_total = 0u32;
        for pane in self.fixed {
            let slack = u64::from(pane.slack());
            let gives = if slack_left == 0 {
                0
            } else {
                // ★ The invariant `owed <= slack_left` is what makes this exact:
                // rounding UP can never take more than this pane has, and it
                // leaves the rest reachable by the panes after it.
                (owed * slack).div_ceil(slack_left).min(slack)
            };
            owed -= gives;
            slack_left -= slack;
            let width = pane.design() - u32::try_from(gives).unwrap_or(pane.slack());
            fixed_total += width;
            widths.push(width);
        }
        (total.saturating_sub(fixed_total), widths)
    }
}

#[cfg(test)]
mod tests {
    use super::{Fault, Pane, PaneRow};

    const LIST_FLOOR: u32 = 759;
    const FIXED: &[Pane] = &[Pane::new(348, 305), Pane::new(318, 288)];
    const ROW: PaneRow = PaneRow::new(LIST_FLOOR, FIXED);

    #[test]
    fn a_rows_two_widths_are_the_arrangement_and_the_floors() {
        assert_eq!(ROW.design(), 759 + 348 + 318);
        assert_eq!(ROW.floor(), 759 + 305 + 288);
        assert_eq!(ROW.slack(), (348 - 305) + (318 - 288));
    }

    #[test]
    fn with_room_to_spare_the_flexible_pane_takes_all_of_it() {
        let (flexible, fixed) = ROW.share(2494);
        assert_eq!(fixed, [348, 318]);
        assert_eq!(flexible, 2494 - 348 - 318);
    }

    /// The case the round was written for: 37 short of the design arrangement.
    #[test]
    fn a_shortfall_is_taken_from_the_fixed_panes_and_not_from_the_flexible_one() {
        let (flexible, fixed) = ROW.share(1388);
        assert_eq!(flexible, LIST_FLOOR, "the flexible pane keeps its floor");
        assert_eq!(
            fixed.iter().sum::<u32>() + flexible,
            1388,
            "the panes tile the row"
        );
        assert!(fixed[0] < 348 && fixed[1] < 318, "both panes gave");
    }

    #[test]
    fn every_width_from_the_floor_up_tiles_the_row_and_clears_every_floor() {
        for total in ROW.floor()..=ROW.design() + 200 {
            let (flexible, fixed) = ROW.share(total);
            assert_eq!(
                flexible + fixed.iter().sum::<u32>(),
                total,
                "the panes do not tile the row at {total}"
            );
            assert!(
                flexible >= LIST_FLOOR,
                "the flexible pane is below its floor at {total}"
            );
            for (pane, width) in FIXED.iter().zip(&fixed) {
                assert!(
                    *width >= pane.floor() && *width <= pane.design(),
                    "a pane is outside [{}, {}] at {total}: {width}",
                    pane.floor(),
                    pane.design(),
                );
            }
        }
    }

    /// ★ The shortfall is shared in proportion to slack, so neither pane reaches
    /// its floor while the other still has room.
    #[test]
    fn a_pane_reaches_its_floor_only_when_the_row_does() {
        let (_, fixed) = ROW.share(ROW.floor());
        assert_eq!(
            fixed,
            [305, 288],
            "at the row's floor every pane is at its own"
        );
        for total in ROW.floor() + 1..ROW.design() {
            let (_, fixed) = ROW.share(total);
            assert!(
                fixed[0] > 305 || fixed[1] > 288,
                "both panes bottomed out at {total}, above the row's floor",
            );
        }
    }

    #[test]
    fn a_rigid_pane_gives_nothing() {
        const RIGID: &[Pane] = &[Pane::rigid(200), Pane::new(300, 250)];
        const ROW: PaneRow = PaneRow::new(100, RIGID);
        let (_, fixed) = ROW.share(ROW.floor());
        assert_eq!(fixed, [200, 250]);
    }

    #[test]
    fn a_row_with_no_slack_still_tiles() {
        const RIGID: &[Pane] = &[Pane::rigid(200)];
        const ROW: PaneRow = PaneRow::new(100, RIGID);
        assert_eq!(ROW.slack(), 0);
        let (flexible, fixed) = ROW.share(300);
        assert_eq!((flexible, fixed), (100, vec![200]));
    }

    #[test]
    fn a_pane_that_cannot_draw_in_its_own_design_width_is_refused() {
        assert_eq!(
            Pane::checked(100, 200),
            Err(Fault::FloorAboveDesign {
                design: 100,
                floor: 200
            })
        );
    }
}
