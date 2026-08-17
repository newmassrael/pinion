//! R1711 §5.16 §5.32 §5.12 §2 #3 §2 #7 — **how small this screen can get, and
//! what stops it going smaller.**
//!
//! # The fact that was missing
//!
//! Every binding in this tree declares a floor for its window — 225 of them,
//! and [`SizeBounds`](crate::size_grant::SizeBounds) has enforced it since
//! R1710. Not one of those numbers was ever *checked against the screen*. A
//! floor is a promise that the screen still works at that size, and the only
//! way to keep such a promise honest is to drive the screen there and look.
//!
//! R1710 tried, with the predicate a reader reaches for first — *is every
//! declared region still painted?* — and reported five regions of the analysis
//! tool's node lab lost at its own floor. Measured again here, through
//! [`reach`](crate::reach): **`lost` is zero and all five are `scrollable`,
//! each with the offset that shows it.** The screen was fine; the question was
//! wrong. Content a reader can scroll to is not content the window is too small
//! for, which is why this module's predicate is reach and not paint.
//!
//! # What the reference does, measured
//!
//! Measured at 6.11.1 by building a probe and running it offscreen (a window
//! holding two scrolling side panes and a stretchy middle, the analysis tool's
//! shape):
//!
//! | question | answer |
//! |---|---|
//! | is the floor derived from the content? | **yes** — the layout composes it (972x260 here) |
//! | is it enforced? | **yes**, client side: `resize(1625, 200)` lands at `1625x260` |
//! | does scrollable content force it? | **no** — a pane holding 898 pixels of rows contributes 70 |
//!
//! The first two are parity and this tree now has both (R1710 took the
//! enforcement; this module takes the derivation). The third is the semantics
//! this module's predicate encodes, and it is why R1710's reading was wrong.
//!
//! What the reference cannot do, each proven by a compile error naming the
//! member rather than by a search that found nothing (5/5):
//!
//! * `minimumSizeHintReason()` — the floor is a bare size; **nothing says
//!   which child forced it**, on the widget or on the layout
//!   (`minimumSizeForcedBy()`).
//! * `wouldFitAt(<a size>)` — no way to ask about a size the window is **not
//!   at**; a caller there has to resize and look.
//! * `unreachableAt(<a size>)` — no enumeration of what a reader would lose
//!   there, at any size.
//! * `minimumSizeHintChanged` — no announcement, so a caller holding a floor
//!   cannot learn it went stale.
//!
//! Its widget, layout and scroll-area classes carry 139 properties and 73
//! methods between them; the count whose name contains a reason word is **0**.
//! A caller there learns the floor by reading a number, and can never learn
//! what put it there.
//!
//! # What this module answers, and what it deliberately does not claim
//!
//! [`measure`] returns a **boundary**, not a minimum: an extent that fits where
//! one pixel less does not, with the marks that go out of reach as the
//! evidence. It does not claim no smaller size fits, because a screen's fit is
//! not required to be monotone in its size and this module refuses to assume
//! what it has not measured. The claim it does make is checked by construction
//! — [`Measured::new`] cannot build an answer with no evidence, so "it fits
//! here and not below" and "here is what going below loses" are one fact read
//! two ways rather than two fields free to drift (the shape
//! [`Grant`](crate::size_grant::Grant) took for the same reason).
//!
//! The two axes are measured independently, each with the other held at the
//! ceiling. That is what a window minimum *is* — a pair of independent bounds,
//! the pair [`SizeBounds`](crate::size_grant::SizeBounds) already carries — and
//! it is not the same as the smallest area: a screen that reflows might fit in
//! a smaller pair than the two axis answers combined. Stated rather than
//! smoothed over.
//!
//! # The probe
//!
//! The caller supplies one closure: *what is out of reach at this size*. This
//! module never sees a scene, which is what lets it be tested against synthetic
//! predicates — including the pathological ones ([`Refused`]) that a real
//! screen produces rarely and a search must still refuse rather than answer
//! wrongly.

/// Which axis an answer or a refusal is about.
///
/// Spelled here rather than borrowed from a widget module: this is a property
/// of a *window*, and the vocabulary a refusal rides on has to be nameable by
/// a caller that has no widgets at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// The horizontal extent.
    Width,
    /// The vertical extent.
    Height,
}

impl Axis {
    /// The word that rides on the wire.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Width => "width",
            Self::Height => "height",
        }
    }
}

/// One axis' answer: an extent that fits, and what one less puts out of reach.
///
/// The evidence is not decoration and not optional. A `Measured` whose
/// `forced_by` were empty would be claiming a boundary it never observed — the
/// exact shape of a check that passes by having nothing to check — so
/// [`Measured::new`] refuses to build one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measured<T> {
    extent: u32,
    forced_by: Vec<T>,
    probes: usize,
}

impl<T> Measured<T> {
    /// Build an answer, or `None` when there is no evidence for it.
    ///
    /// `forced_by` is what the probe reported at `extent - 1`. Empty means the
    /// search never saw the boundary it is about to publish, which is not an
    /// answer to soften — it is one to refuse.
    #[must_use]
    pub fn new(extent: u32, forced_by: Vec<T>, probes: usize) -> Option<Self> {
        if forced_by.is_empty() {
            return None;
        }
        Some(Self {
            extent,
            forced_by,
            probes,
        })
    }

    /// The extent that fits.
    #[must_use]
    pub const fn extent(&self) -> u32 {
        self.extent
    }

    /// The extent that does not — always one less, and named so a caller reads
    /// the boundary rather than recomputing it.
    #[must_use]
    pub const fn short_extent(&self) -> u32 {
        self.extent.saturating_sub(1)
    }

    /// What [`Self::short_extent`] puts out of reach. Never empty.
    #[must_use]
    pub fn forced_by(&self) -> &[T] {
        &self.forced_by
    }

    /// How many times the probe ran for this axis. Published because the cost
    /// of this answer is a full view and layout per probe, and a caller
    /// choosing when to ask deserves to know what it is buying.
    #[must_use]
    pub const fn probes(&self) -> usize {
        self.probes
    }
}

/// Whether the two axis answers, taken together, are a size that works.
///
/// ★★★★★ Not a formality — measured on the analysis tool the day this module
/// was written. Its node lab answers 1506 wide (measured at the full height)
/// and 360 tall (measured at the full width), and at **1506 x 360 nine marks
/// are lost**: below the floor on either axis the screen stops using the live
/// extent for *both*, so narrowing it also un-shortens it. Two of the three
/// screens failed this way.
///
/// A caveat in a doc comment would have been the honest thing and the useless
/// thing. One extra probe turns it into a field, and a caller reading `needed`
/// as a size it can use is told, in the same answer, whether that is true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairFit<T> {
    /// The two answers together are a size at which nothing is out of reach.
    Fits,
    /// They are not. These are the marks that go out of reach there — so the
    /// per-axis numbers are boundaries, not a size to resize to.
    Loses(Vec<T>),
}

impl<T> PairFit<T> {
    /// The word that rides on the wire.
    #[must_use]
    pub const fn wire_word(&self) -> &'static str {
        match self {
            Self::Fits => "fits",
            Self::Loses(_) => "loses",
        }
    }
}

/// Both axes, and whether their pair is a size in its own right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor<T> {
    /// The horizontal boundary.
    pub width: Measured<T>,
    /// The vertical boundary.
    pub height: Measured<T>,
    /// Whether [`Self::extent`] is itself a working size. See [`PairFit`].
    pub pair: PairFit<T>,
}

impl<T> Floor<T> {
    /// The pair, in the order every size in this tree is written.
    ///
    /// Read [`Self::pair`] before using it as a size: each half is a boundary
    /// measured with the other axis at the ceiling, which is what a window
    /// minimum is, and is not the same claim as "the screen works here".
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        (self.width.extent, self.height.extent)
    }

    /// What the whole measurement cost, the pair probe included.
    #[must_use]
    pub const fn probes(&self) -> usize {
        self.width.probes + self.height.probes + 1
    }
}

/// Why an axis has no boundary to report.
///
/// Both arms are real screens rather than defensive padding: the first is a
/// screen broken at every size it can take, and the second is a probe that
/// reports nothing anywhere — which is what a surface painting nothing
/// addressable looks like, and is the failure mode a search would otherwise
/// answer with a confident `1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused<T> {
    /// Even at the ceiling something is out of reach, so no smaller extent can
    /// be a floor. The marks are carried because they are the defect: a screen
    /// that cannot show itself at its largest size does not need a floor
    /// measured, it needs repairing.
    CeilingIsShort {
        /// Which axis was being measured when the ceiling failed.
        axis: Axis,
        /// What was out of reach there.
        out_of_reach: Vec<T>,
    },
    /// Nothing is out of reach even at the smallest extent, so there is no
    /// boundary anywhere and the answer would be a number with no evidence.
    NothingIsEverLost {
        /// Which axis was being measured.
        axis: Axis,
    },
}

impl<T> Refused<T> {
    /// The axis, whichever arm this is.
    #[must_use]
    pub const fn axis(&self) -> Axis {
        match self {
            Self::CeilingIsShort { axis, .. } | Self::NothingIsEverLost { axis } => *axis,
        }
    }

    /// The word that rides on the wire.
    #[must_use]
    pub const fn wire_word(&self) -> &'static str {
        match self {
            Self::CeilingIsShort { .. } => "ceiling_is_short",
            Self::NothingIsEverLost { .. } => "nothing_is_ever_lost",
        }
    }
}

/// The smallest extent this screen was measured to work at, per axis.
///
/// `ceiling` is the largest size worth asking about — in practice the window's
/// own opening size, which a screen is expected to fit in. `probe` answers
/// *what is out of reach at `(w, h)`*; an empty answer is a fit.
///
/// # Errors
///
/// [`Refused`] when an axis has no boundary: the ceiling itself does not fit,
/// or nothing is ever out of reach. Both are answers about the screen, not
/// failures of the search.
///
/// # Cost
///
/// `2 + ceil(log2(ceiling))` probes per axis, each a full view and layout at
/// the size being tried. The count comes back in [`Measured::probes`].
pub fn measure<T, P>(ceiling: (u32, u32), mut probe: P) -> Result<Floor<T>, Refused<T>>
where
    P: FnMut(u32, u32) -> Vec<T>,
{
    let width = measure_axis(Axis::Width, ceiling, &mut probe)?;
    let height = measure_axis(Axis::Height, ceiling, &mut probe)?;
    // The pair is a THIRD question, asked because the first two do not answer
    // it — see [`PairFit`] for the screen that proved so.
    let pair = match probe(width.extent, height.extent) {
        out if out.is_empty() => PairFit::Fits,
        out => PairFit::Loses(out),
    };
    Ok(Floor {
        width,
        height,
        pair,
    })
}

/// One axis, with the other held at the ceiling.
///
/// The bisection maintains "`lo` does not fit, `hi` does" and narrows until
/// they are adjacent, so the boundary is a pair the search **evaluated** rather
/// than one it inferred. Both ends of that invariant are established by a probe
/// before the loop, which is what the two [`Refused`] arms report.
fn measure_axis<T, P>(
    axis: Axis,
    ceiling: (u32, u32),
    probe: &mut P,
) -> Result<Measured<T>, Refused<T>>
where
    P: FnMut(u32, u32) -> Vec<T>,
{
    let at = |extent: u32| match axis {
        Axis::Width => (extent, ceiling.1),
        Axis::Height => (ceiling.0, extent),
    };
    let top = match axis {
        Axis::Width => ceiling.0,
        Axis::Height => ceiling.1,
    };
    let mut probes = 1;
    let (w, h) = at(top);
    let out_of_reach = probe(w, h);
    if !out_of_reach.is_empty() {
        return Err(Refused::CeilingIsShort { axis, out_of_reach });
    }
    // The floor of the search: an extent nothing can be laid out in. A screen
    // that loses nothing even here has no boundary to find, and answering `1`
    // would be a number with no evidence under it.
    probes += 1;
    let (w, h) = at(0);
    let mut lost = probe(w, h);
    if lost.is_empty() {
        return Err(Refused::NothingIsEverLost { axis });
    }
    let (mut lo, mut hi) = (0, top);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        probes += 1;
        let (w, h) = at(mid);
        let out = probe(w, h);
        if out.is_empty() {
            hi = mid;
        } else {
            lo = mid;
            lost = out;
        }
    }
    // `hi` fits, `lo == hi - 1` does not, and `lost` is what `lo` lost — the
    // three facts the answer is made of, every one of them observed.
    Measured::new(hi, lost, probes).ok_or(Refused::NothingIsEverLost { axis })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A screen that needs `need` on each axis: anything narrower or shorter
    /// loses the region named for the axis that came up short.
    fn needs(need: (u32, u32)) -> impl FnMut(u32, u32) -> Vec<&'static str> {
        move |w, h| {
            let mut out = Vec::new();
            if w < need.0 {
                out.push("too narrow");
            }
            if h < need.1 {
                out.push("too short");
            }
            out
        }
    }

    #[test]
    fn r1711_the_boundary_is_the_extent_that_fits_with_one_less_that_does_not() {
        let floor = measure((1625, 900), needs((1625, 360))).expect("a screen with a floor");
        assert_eq!(floor.extent(), (1625, 360));
        assert_eq!(floor.width.short_extent(), 1624);
        assert_eq!(floor.width.forced_by(), ["too narrow"]);
        assert_eq!(floor.height.forced_by(), ["too short"]);
    }

    #[test]
    fn r1711_the_evidence_is_what_one_pixel_less_loses_not_what_the_floor_loses() {
        let floor = measure((800, 600), needs((300, 200))).expect("a screen with a floor");
        // At the answer itself NOTHING is lost — that is what makes it the
        // answer. The evidence therefore has to come from below it, and a test
        // that read the answer's own probe would be reading an empty list.
        assert_eq!(floor.height.extent(), 200);
        assert_eq!(floor.height.forced_by(), ["too short"]);
        assert!(needs((300, 200))(800, 200).is_empty());
    }

    #[test]
    fn r1711_a_screen_that_does_not_fit_at_the_ceiling_is_refused() {
        let refused =
            measure((1000, 400), needs((1200, 300))).expect_err("the ceiling is too narrow");
        assert_eq!(refused.axis(), Axis::Width);
        assert_eq!(refused.wire_word(), "ceiling_is_short");
        match refused {
            Refused::CeilingIsShort { out_of_reach, .. } => {
                assert_eq!(out_of_reach, ["too narrow"]);
            }
            Refused::NothingIsEverLost { .. } => panic!("wrong arm"),
        }
    }

    #[test]
    fn r1711_a_probe_that_never_loses_anything_is_refused_rather_than_answered_one() {
        let refused = measure((640, 480), |_, _| Vec::<&str>::new())
            .expect_err("a surface that reports nothing has no floor");
        assert_eq!(refused.wire_word(), "nothing_is_ever_lost");
        assert_eq!(refused.axis(), Axis::Width);
    }

    #[test]
    fn r1711_an_answer_cannot_be_built_without_the_evidence_for_it() {
        assert!(Measured::<&str>::new(900, Vec::new(), 3).is_none());
        assert!(Measured::new(900, vec!["a mark"], 3).is_some());
    }

    #[test]
    fn r1711_the_pair_of_two_boundaries_is_checked_and_not_assumed() {
        // The analysis tool's shape, reduced: below the floor on EITHER axis
        // the screen stops using the live extent for BOTH, so the pair of the
        // two answers is a size at which the vertical content is cut. Measured
        // on the node lab as (1506, 360) losing nine marks; here as a predicate
        // small enough to read.
        let floor = measure((1600, 900), |w, h| {
            // The binding's own rule: use the live extent, or fall back to the
            // design size when EITHER axis is under the layout's clamp.
            let laid_out = if w >= 1600 && h >= 340 {
                (w, h)
            } else {
                (1600, 900)
            };
            let mut out = Vec::new();
            if laid_out.0 - 119 > w {
                out.push("cut on the right");
            }
            if laid_out.1 > h {
                out.push("cut off the bottom");
            }
            out
        })
        .expect("a screen with a floor");
        assert_eq!(floor.extent(), (1481, 340));
        assert_eq!(floor.pair.wire_word(), "loses");
        match &floor.pair {
            PairFit::Loses(marks) => assert_eq!(marks, &["cut off the bottom"]),
            PairFit::Fits => panic!("the pair does not fit and the type must say so"),
        }
    }

    #[test]
    fn r1711_a_screen_whose_axes_are_independent_reports_a_pair_that_fits() {
        let floor = measure((1600, 900), needs((1200, 500))).expect("a screen with a floor");
        assert_eq!(floor.pair, PairFit::Fits);
        assert_eq!(floor.pair.wire_word(), "fits");
    }

    #[test]
    fn r1711_the_search_costs_a_logarithm_and_says_how_many_probes_it_took() {
        let mut calls = 0;
        let floor = measure((2048, 2048), |w, h| {
            calls += 1;
            let mut out = Vec::new();
            if w < 1000 {
                out.push("narrow");
            }
            if h < 1000 {
                out.push("short");
            }
            out
        })
        .expect("a screen with a floor");
        assert_eq!(floor.extent(), (1000, 1000));
        // Two guards plus eleven halvings, per axis, plus the pair — and the
        // published count is the count, not an estimate of it.
        assert_eq!(floor.probes(), calls);
        assert!(floor.probes() <= 2 * (2 + 12) + 1, "{}", floor.probes());
    }

    #[test]
    fn r1711_each_axis_is_measured_with_the_other_at_the_ceiling() {
        // A screen whose height requirement depends on width would otherwise
        // make the pair meaningless. Recording the sizes the search asked about
        // is how the contract is checked rather than asserted in prose.
        let mut asked = Vec::new();
        let floor = measure((1000, 800), |w, h| {
            asked.push((w, h));
            if w < 400 || h < 200 {
                vec!["gone"]
            } else {
                Vec::new()
            }
        })
        .expect("a screen with a floor");
        assert_eq!(floor.extent(), (400, 200));
        assert_eq!(
            asked.pop(),
            Some((400, 200)),
            "the pair is asked about last"
        );
        let (width_probes, height_probes) = asked.split_at(floor.width.probes());
        assert!(
            width_probes.iter().all(|&(_, h)| h == 800),
            "{width_probes:?}"
        );
        assert!(
            height_probes.iter().all(|&(w, _)| w == 1000),
            "{height_probes:?}"
        );
    }

    #[test]
    fn r1711_a_screen_that_only_just_fits_reports_the_ceiling_itself() {
        let floor = measure((500, 500), needs((500, 500))).expect("a screen with a floor");
        assert_eq!(floor.extent(), (500, 500));
        assert_eq!(floor.width.short_extent(), 499);
    }

    #[test]
    fn r1711_the_answer_holds_for_a_predicate_that_is_not_monotone() {
        // A screen with a hole in it: it fails at 300..=399 as well as below
        // 200. The search does not claim to have found the smallest fitting
        // extent — it claims the pair it evaluated, and that pair is real.
        let floor = measure((800, 800), |w, h| {
            if w < 200 || (300..400).contains(&w) || h < 100 {
                vec!["gone"]
            } else {
                Vec::new()
            }
        })
        .expect("a screen with a floor");
        let found = floor.width.extent();
        assert!(found == 200 || found == 400, "{found}");
        assert!(
            (found < 200) || (300..400).contains(&(found - 1)) || found - 1 < 200,
            "the extent below the answer is one the probe reported lost"
        );
    }
}
