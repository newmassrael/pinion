//! R1710 §5.16 §5.12 §2 #2 §2 #3 §2 #7 — **a resize says what it granted, and
//! which declared bound moved it.**
//!
//! # The fact that was missing
//!
//! A window declares a floor for its own inner size. Something then asks for a
//! size below it. Two things have to happen and neither did: the ask has to be
//! resolved against the declared bound *by us*, so the answer does not depend
//! on which window system happens to be running, and the resolution has to be
//! **reported**, so a caller learns that it got 900 when it asked for 880 —
//! and why.
//!
//! Measured on this host before the repair, driving the analysis-tool dashboard
//! (which declares a floor of 1440x900) through `scene/resize`:
//!
//! | display | asked | window afterwards | what the wire answered |
//! |---|---|---|---|
//! | bare server, no manager | 1560x880 | **1560x880** | `height: 880` |
//! | real desktop, manager   | 1560x880 | **1560x900** | `height: 880` |
//!
//! Both rows are defects and they are different ones. The first says the
//! declared floor is enforced by *nothing* on the display every gate in this
//! tree runs on — so no test could ever have failed. The second says the wire
//! answered with a number the window does not have, on the path §2 #2 makes an
//! AI agent's primary one: ask for 880, be told 880, then read a scene that is
//! 900 and have no way to ask why.
//!
//! # What the reference does, measured
//!
//! Measured at 6.11.1 by building a probe and running it offscreen, rather than
//! by reading headers. Declare a minimum of 1440x900, then ask below it:
//!
//! | layer | `resize(1560, 880)` | `resize(1340, 900)` |
//! |---|---|---|
//! | its window layer | 1560x**880** | **1340**x900 |
//! | its widget layer | 1560x**900** | **1440**x900 |
//!
//! So the reference's *widget* layer resolves the ask against the declared
//! bounds itself, client side, and the result does not depend on the window
//! system. Its *window* layer declares a minimum and then does not enforce it —
//! which is exactly where this tree was, since a pinion shell window IS the
//! window layer. **That half is parity and this module takes it.**
//!
//! What the reference does not do, each proven by a compile error naming the
//! member rather than by a search that found nothing (5/5):
//!
//! * `resize()` answers **nothing** — `bool ok = w.resize(…)` is
//!   "void value not ignored as it ought to be".
//! * no reason for a refusal (`lastResizeRefusal`), no per-axis "which bound
//!   moved me" (`heightConstrainedBy`), no signal carrying the asked and the
//!   granted pair (`resizeConstrained`), and no way to ask **without acting**
//!   (`wouldGrantSize`).
//!
//! Its window class carries 18 properties and 52 methods; the count of those
//! whose name contains a reason word (reason / refused / clamped / constrained
//! / denied) is **0**, and of those containing a grant word, **0**. A caller
//! there discovers it was clamped by re-reading `size()` and diffing it against
//! what it asked for, and can never discover *which* declared bound did it.
//!
//! # The one property that makes this type honest
//!
//! [`Grant`] does not store the granted size. It stores what was **asked** and,
//! per axis, **which bound decided it** ([`Bound`]) — and derives the extent
//! from that pair. So a grant that says "the floor moved this axis to 900"
//! cannot also report 880: the number and the reason are the same fact read two
//! ways, not two fields that can drift. This is the shape R1706's `Selection`
//! took for the same reason (its active member is an INDEX, so pointing at a
//! non-member is inexpressible).
//!
//! [`SizeBounds::new`] refuses a pair whose floor exceeds its ceiling on either
//! axis, which is what makes [`SizeBounds::resolve`] order-independent: with
//! `floor <= ceiling` guaranteed, clamping up-then-down and down-then-up agree,
//! so the resolution has no hidden precedence rule for a reader to learn.

use serde::{Deserialize, Serialize};

/// Which declared bound decided one axis of a resolved size request.
///
/// The `at` value is the **declared extent** that moved the axis, not the
/// difference — a caller that wants the difference has the ask in
/// [`Grant::asked`], and a caller that wants to know what the surface will
/// never go below wants the bound itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Bound {
    /// Within the declared bounds: the asked-for extent is the granted one.
    AsAsked,
    /// Raised to the declared floor.
    Floor {
        /// The declared floor for this axis.
        at: u32,
    },
    /// Lowered to the declared ceiling.
    Ceiling {
        /// The declared ceiling for this axis.
        at: u32,
    },
}

impl Bound {
    /// The closed set of `kind` strings this enum serializes to, published so
    /// the wire census declares the vocabulary from the type that owns it
    /// rather than from a list retyped beside it (R1616's rule).
    pub const KINDS: &'static [&'static str] = &["as_asked", "floor", "ceiling"];

    /// The granted extent for an axis that asked for `asked`.
    #[must_use]
    pub const fn extent(self, asked: u32) -> u32 {
        match self {
            Self::AsAsked => asked,
            Self::Floor { at } | Self::Ceiling { at } => at,
        }
    }

    /// The declared extent that moved this axis, or `None` when nothing did.
    #[must_use]
    pub const fn at(self) -> Option<u32> {
        match self {
            Self::AsAsked => None,
            Self::Floor { at } | Self::Ceiling { at } => Some(at),
        }
    }

    /// Was this axis granted exactly what it asked for?
    #[must_use]
    pub const fn is_as_asked(self) -> bool {
        matches!(self, Self::AsAsked)
    }
}

/// The bounds a surface declares for its own inner size.
///
/// `None` on either side is "no declared bound on this axis pair", which is a
/// different statement from a bound at zero or at [`u32::MAX`]: a surface that
/// declares nothing lets the window system decide, and this type says so
/// instead of inventing an extreme that would read as a declaration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SizeBounds {
    floor: Option<(u32, u32)>,
    ceiling: Option<(u32, u32)>,
}

impl SizeBounds {
    /// No declared bound on either axis — every ask is granted as asked.
    pub const UNBOUNDED: Self = Self {
        floor: None,
        ceiling: None,
    };

    /// Only a floor, the shape a window that pins its minimum declares.
    #[must_use]
    pub const fn floored(floor: (u32, u32)) -> Self {
        Self {
            floor: Some(floor),
            ceiling: None,
        }
    }

    /// A floor and a ceiling, or `None` when they contradict each other.
    ///
    /// # Errors as a refusal
    ///
    /// A pair whose floor exceeds its ceiling **on either axis** is refused
    /// rather than silently normalised, because both normalisations are
    /// defensible ("the floor wins" / "the ceiling wins") and a caller that
    /// declared such a pair has a bug in its declaration, not a size question.
    /// Refusing here is what lets [`Self::resolve`] have no precedence rule.
    #[must_use]
    pub fn new(floor: Option<(u32, u32)>, ceiling: Option<(u32, u32)>) -> Option<Self> {
        if let (Some(lo), Some(hi)) = (floor, ceiling) {
            if lo.0 > hi.0 || lo.1 > hi.1 {
                return None;
            }
        }
        Some(Self { floor, ceiling })
    }

    /// The declared floor, if any.
    #[must_use]
    pub const fn floor(self) -> Option<(u32, u32)> {
        self.floor
    }

    /// The declared ceiling, if any.
    #[must_use]
    pub const fn ceiling(self) -> Option<(u32, u32)> {
        self.ceiling
    }

    /// Resolve an ask against these bounds, changing nothing.
    ///
    /// This is the whole of the rule, in one place, and it is what both the
    /// real path and the dry run call — so "what would you grant" and "what did
    /// you grant" cannot answer differently (§2 #3's guarantee, applied to a
    /// size instead of a signal).
    #[must_use]
    pub const fn resolve(self, asked: (u32, u32)) -> Grant {
        Grant {
            asked,
            width: self.axis(asked.0, true),
            height: self.axis(asked.1, false),
        }
    }

    /// One axis of [`Self::resolve`]. `is_width` picks the tuple element, so
    /// the rule below is written once rather than mirrored.
    const fn axis(self, asked: u32, is_width: bool) -> Bound {
        if let Some(hi) = self.ceiling {
            let at = if is_width { hi.0 } else { hi.1 };
            if asked > at {
                return Bound::Ceiling { at };
            }
        }
        if let Some(lo) = self.floor {
            let at = if is_width { lo.0 } else { lo.1 };
            if asked < at {
                return Bound::Floor { at };
            }
        }
        Bound::AsAsked
    }
}

/// What a size request resolved to: what was asked, and which declared bound
/// decided each axis.
///
/// The granted size is [`Self::size`], **derived** from the two bounds — see
/// this module's header for why it is not a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    asked: (u32, u32),
    width: Bound,
    height: Bound,
}

impl Grant {
    /// The size that was asked for.
    #[must_use]
    pub const fn asked(self) -> (u32, u32) {
        self.asked
    }

    /// Which bound decided the width.
    #[must_use]
    pub const fn width(self) -> Bound {
        self.width
    }

    /// Which bound decided the height.
    #[must_use]
    pub const fn height(self) -> Bound {
        self.height
    }

    /// The granted size — the size the surface will actually be asked to take.
    #[must_use]
    pub const fn size(self) -> (u32, u32) {
        (
            self.width.extent(self.asked.0),
            self.height.extent(self.asked.1),
        )
    }

    /// Was the whole ask granted as asked?
    #[must_use]
    pub const fn is_as_asked(self) -> bool {
        self.width.is_as_asked() && self.height.is_as_asked()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r1710_an_unbounded_ask_is_granted_as_asked() {
        let g = SizeBounds::UNBOUNDED.resolve((1560, 880));
        assert_eq!(g.size(), (1560, 880));
        assert!(g.is_as_asked());
        assert_eq!(g.width(), Bound::AsAsked);
        assert_eq!(g.height(), Bound::AsAsked);
        assert_eq!(g.asked(), (1560, 880));
    }

    #[test]
    fn r1710_the_floor_raises_only_the_axis_below_it() {
        // The measured case: the dashboard's floor is 1440x900 and the ask was
        // 1560x880. The width is fine and the height is not, and a grant that
        // could not say which would be no better than the bare number.
        let g = SizeBounds::floored((1440, 900)).resolve((1560, 880));
        assert_eq!(g.size(), (1560, 900));
        assert!(!g.is_as_asked());
        assert_eq!(g.width(), Bound::AsAsked);
        assert_eq!(g.height(), Bound::Floor { at: 900 });
        assert_eq!(g.height().at(), Some(900));
    }

    #[test]
    fn r1710_the_floor_raises_both_axes_when_both_are_below() {
        let g = SizeBounds::floored((1440, 900)).resolve((1340, 880));
        assert_eq!(g.size(), (1440, 900));
        assert_eq!(g.width(), Bound::Floor { at: 1440 });
        assert_eq!(g.height(), Bound::Floor { at: 900 });
    }

    #[test]
    fn r1710_asking_for_exactly_the_floor_is_as_asked() {
        // Not a pedantic case: it is the size the shell forwards after a
        // clamp, and if that round-trip reported `floor` the wire would say a
        // request was constrained every time one had already been resolved.
        let g = SizeBounds::floored((1440, 900)).resolve((1440, 900));
        assert!(g.is_as_asked());
        assert_eq!(g.size(), (1440, 900));
    }

    #[test]
    fn r1710_the_ceiling_lowers_the_axis_above_it() {
        let b = SizeBounds::new(Some((100, 100)), Some((800, 600))).unwrap();
        let g = b.resolve((1000, 500));
        assert_eq!(g.size(), (800, 500));
        assert_eq!(g.width(), Bound::Ceiling { at: 800 });
        assert_eq!(g.height(), Bound::AsAsked);
    }

    #[test]
    fn r1710_a_contradictory_pair_is_refused() {
        assert!(SizeBounds::new(Some((900, 100)), Some((800, 600))).is_none());
        assert!(SizeBounds::new(Some((100, 700)), Some((800, 600))).is_none());
        assert!(SizeBounds::new(Some((800, 600)), Some((800, 600))).is_some());
        // A one-sided declaration can never contradict anything.
        assert!(SizeBounds::new(Some((9000, 9000)), None).is_some());
        assert!(SizeBounds::new(None, Some((1, 1))).is_some());
    }

    #[test]
    fn r1710_resolution_is_order_independent() {
        // What the refusal above buys: with `floor <= ceiling` guaranteed, an
        // ask outside both bounds is impossible, so there is no axis on which
        // "clamp up first" and "clamp down first" could disagree. Asserted by
        // exhausting a small grid rather than by argument.
        let b = SizeBounds::new(Some((10, 20)), Some((30, 40))).unwrap();
        for w in 0..40_u32 {
            for h in 0..50_u32 {
                let (gw, gh) = b.resolve((w, h)).size();
                assert_eq!(gw, w.clamp(10, 30), "width {w}");
                assert_eq!(gh, h.clamp(20, 40), "height {h}");
            }
        }
    }

    #[test]
    fn r1710_a_grant_cannot_report_a_size_its_bound_denies() {
        // The property the type exists for: the granted extent is DERIVED from
        // the bound, so no construction can produce a grant that names the
        // floor and reports the asked number. Exhausted over the vocabulary.
        for bound in [
            Bound::AsAsked,
            Bound::Floor { at: 900 },
            Bound::Ceiling { at: 900 },
        ] {
            let asked = 880;
            let extent = bound.extent(asked);
            match bound.at() {
                Some(at) => assert_eq!(extent, at, "a bounded axis reports its bound"),
                None => assert_eq!(extent, asked, "an unbounded axis reports the ask"),
            }
        }
    }

    #[test]
    fn r1710_the_published_kinds_are_the_serialized_ones() {
        // R1616's rule, checked rather than trusted: the census reads
        // `Bound::KINDS`, so a new arm that forgets to extend it would publish
        // a vocabulary the wire does not use.
        let mut seen = Vec::new();
        for bound in [
            Bound::AsAsked,
            Bound::Floor { at: 1 },
            Bound::Ceiling { at: 1 },
        ] {
            let v = serde_json::to_value(bound).unwrap();
            seen.push(v["kind"].as_str().unwrap().to_owned());
            // `at` is absent, not null, on the arm that has no bound — the
            // census declares it optional for exactly that reason.
            assert_eq!(bound.at().is_some(), v.get("at").is_some());
        }
        assert_eq!(seen, Bound::KINDS);
    }

    #[test]
    fn r1710_a_grant_survives_the_wire() {
        let g = SizeBounds::floored((1440, 900)).resolve((1340, 880));
        let back: Grant = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
        assert_eq!(back, g);
        assert_eq!(back.size(), (1440, 900));
    }
}
