//! ★★★★★ R1737 §5.35 §5.15 §2 #7 — **where a pointer arrived inside a surface,
//! in that surface's own frame, and whether the framework's two accounts of it
//! agree.**
//!
//! # The hole this closes
//!
//! §2 #7 makes a pinion screen ONE [`External`](crate::external::External), so
//! the router hands it a *fraction* of its painted rectangle and the screen
//! multiplies that fraction back out to a pixel. That pixel is the one every
//! press on the screen is resolved against — a press carries no position of its
//! own, it acts on the cursor the last
//! [`pointer_move`](crate::external::External::pointer_move) recorded — so if
//! the multiplication lands on the wrong pixel, every gesture on the screen is
//! aimed one pixel away from where the person aimed it.
//!
//! That is not hypothetical. R1736 measured exactly that defect and repaired it
//! in [`crate::external::pixel_of`]; the measurement was a real X
//! pointer walked over 600 columns and 600 rows of a running screen, asking the
//! screen where it thought the pointer was.
//!
//! **And that measurement was only possible because the screen happened to
//! publish a cursor field.** Measured across the five screens in this tree that
//! hit-test themselves: three publish one, in **two incompatible spellings** (a
//! `"x,y"` string and an `{x, y}` object), and **two publish nothing at all** —
//! so on those two the check that found the defect cannot be run. The fact is
//! the framework's; only the reporting of it was each screen's.
//!
//! # What is recorded, and why the framework is the one that can
//!
//! The router resolves the reading at exactly one place, and that place holds
//! **both** accounts of where the pointer is:
//!
//! * the cursor the window system reported, in the window's frame; and
//! * the rectangle the fraction is taken over, as the paint laid it out.
//!
//! From those two the pixel the pointer is over is `floor(cursor) − rect.origin`
//! — arithmetic with no fraction in it at all — while the pixel the *surface*
//! will resolve is `pixel_of(fraction, extent)`. They are two derivations of one
//! fact, and [`Landing`] is the framework comparing them.
//!
//! So a round-trip defect stops being something a 600-point sweep has to go
//! looking for and becomes a **verdict, published at every pointer event, for
//! every surface, whether or not the surface says anything about cursors.**
//!
//! # Where the floor stands, measured
//!
//! Built as a probe against the 6.11.1 release and run, rather than read out of
//! its documentation.
//!
//! The floor is **above** this tree on one axis and we owed it: an outside
//! observer there can ask for *any* widget where the pointer is in that widget's
//! own frame, without the widget having stored anything — measured exact over
//! 400 columns and 300 rows of a child widget, no misses. Universality was the
//! floor's and this module is the framework meeting it.
//!
//! What the floor cannot do is the part that matters here. Its answer is **where
//! the cursor is now**, not **where the event arrived**: measured, a press
//! delivered at (37, 21) inside a child, followed by moving the cursor, leaves
//! the framework answering (300, 250) — the cursor's position, with the
//! delivered position reported nowhere. Across the five types such a record
//! could live on there are **245 declared properties and 195 declared methods**,
//! of which **3 are point-typed** (all of them the widget's own position in its
//! parent) and **0 methods return a point**; none is the position an event was
//! delivered at. And for a synthesised event, which the pointer never made, the
//! position it carried is unreadable and the event is indistinguishable from a
//! real one.
//!
//! Nor does the floor ever compare its own two accounts. It is the same shape
//! R1736 recorded one level up for paint versus pick shape: both numbers are
//! held, no verdict is published, and an author who lets them part finds out
//! from a person.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::external::pixel_of;
use crate::input::PointerReading;
use crate::scene::Rect;

thread_local! {
    /// Surface tag -> every arrival that surface has been delivered.
    static ARRIVALS: RefCell<BTreeMap<String, SurfaceArrivals>> =
        const { RefCell::new(BTreeMap::new()) };
}

/// ★★★★★ R1737 — how the framework's two accounts of one arrival stand.
///
/// Three arms and no catch-all, for the reason [`DropStanding`] has three: a
/// fourth case is a thing to decide, and `_ =>` decides it silently and wrongly.
///
/// [`DropStanding`]: crate::drop_target::DropStanding
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landing {
    /// The pointer was inside the rectangle, and the pixel the surface resolves
    /// is the pixel the pointer was over.
    Exact,
    /// Both accounts name a pixel of the rectangle and they are **different**
    /// pixels: the round trip through the fraction lost the pointer.
    ///
    /// `by` is `resolved − inside`, so its sign says which way the press moved.
    /// R1736's defect was `(-1, 0)` and `(0, -1)` at some coordinates and not
    /// others, which is the shape a person reports as "it works and then it
    /// doesn't".
    Drifted {
        /// How far the resolved pixel is from the pixel the pointer was over.
        by: (i32, i32),
    },
    /// The pointer was outside the rectangle the fraction was taken over, which
    /// a capture lock does on purpose — it keeps forwarding moves after the
    /// cursor leaves. The resolved pixel is then a clamp and not a claim about
    /// where the pointer is, so comparing the two would manufacture a defect.
    Strayed,
}

impl Landing {
    /// Whether this landing is a defect — the one arm with no benign reading.
    ///
    /// Published as a predicate rather than left to each caller to spell,
    /// because "which verdicts are defects" is the rule a gate IS, and a rule
    /// with one copy per consumer is the shape this project keeps paying for.
    #[must_use]
    pub const fn is_defect(&self) -> bool {
        matches!(self, Self::Drifted { .. })
    }

    /// The wire word for this landing.
    #[must_use]
    pub const fn word(&self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Drifted { .. } => "drifted",
            Self::Strayed => "strayed",
        }
    }
}

/// ★★★★★ R1737 — where a pointer last arrived in one surface, as the two facts
/// the framework held when it delivered the event.
///
/// Deliberately **not** the answer alone. The answer is derived
/// ([`resolved`](Self::resolved), [`inside`](Self::inside),
/// [`landing`](Self::landing)) so a reader can see the arithmetic that produced
/// it and the census cannot report a verdict that does not follow from what was
/// delivered.
///
/// ```
/// # use pinion_core::arrival::{Landing, PointerArrival};
/// # use pinion_core::scene::Rect;
/// // A cursor 40 pixels into a 200-wide rectangle that starts at x = 100.
/// let over = Rect::new(100, 50, 200, 120);
/// let arrival = PointerArrival::new(over, (140.0, 70.0), (40.0 / 200.0, 20.0 / 120.0));
/// assert_eq!(arrival.inside(), (40, 20));
/// assert_eq!(arrival.resolved(), (40, 20));
/// assert_eq!(arrival.landing(), Landing::Exact);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerArrival {
    /// The rectangle the fraction was taken over, in the window's frame, **as
    /// the paint that produced it laid it out**.
    ///
    /// The same rectangle [`PointerReading::extent`] is the size of — carried
    /// here as a rectangle because the *origin* is what turns a window cursor
    /// into a surface pixel, and the origin is the half the router used when it
    /// normalised the cursor and then dropped. R1727 kept the extent; this keeps
    /// the origin, and the pair is what makes the comparison possible.
    pub over: Rect,
    /// The cursor as the window system reported it, in the window's frame and
    /// in logical pixels.
    pub cursor: (f64, f64),
    /// The fraction of [`over`](Self::over) that was delivered to the surface.
    ///
    /// May fall outside `[0.0, 1.0]` — see [`Landing::Strayed`].
    pub at: (f32, f32),
}

impl PointerArrival {
    /// Record an arrival of `at` over `over`, from a window cursor of `cursor`.
    #[must_use]
    pub const fn new(over: Rect, cursor: (f64, f64), at: (f32, f32)) -> Self {
        Self { over, cursor, at }
    }

    /// The reading the surface was handed, rebuilt from the rectangle it was
    /// taken over.
    ///
    /// One extent, not two: storing the reading *and* the rectangle would be
    /// two records of one size, free to part — which is the class this whole
    /// module exists to remove.
    #[must_use]
    pub fn reading(&self) -> PointerReading {
        PointerReading::new(self.at, self.extent())
    }

    /// The pixel inside [`over`](Self::over) that the **window system** put the
    /// pointer at: `floor(cursor) − over.origin`.
    ///
    /// Signed, because a capture lock keeps forwarding after the cursor leaves
    /// the rectangle and "17 pixels left of the rectangle" is a fact worth being
    /// able to state. [`Landing`] is what decides whether that is a defect.
    #[must_use]
    pub fn inside(&self) -> (i64, i64) {
        // `floor` and not a cast: a cast truncates toward zero, so a cursor at
        // -0.5 would land on pixel 0 and a cursor half a pixel outside the left
        // edge would read as inside. The rectangle's own `holds` draws the same
        // distinction and for the same reason (R1707).
        let px = self.cursor.0.floor();
        let py = self.cursor.1.floor();
        (
            clamped_i64(px) - i64::from(self.over.x),
            clamped_i64(py) - i64::from(self.over.y),
        )
    }

    /// The pixel the delivered fraction names inside
    /// [`over`](Self::over) — [`pixel_of`], the rounding rule R1736 measured,
    /// applied to the extent the fraction was actually taken over.
    ///
    /// # Two things left out, both on purpose
    ///
    /// **The pan.** [`layout_point`](crate::external::layout_point) adds the
    /// screen's declared window pan; here it would be added to both accounts
    /// equally and cancel, so a verdict that carried it would change when a
    /// window was scrolled and mean nothing more.
    ///
    /// **The size store.** `layout_point` divides by
    /// [`surface_size`](crate::external::surface_size), and this divides by the
    /// rectangle carried in the arrival. For a screen — capture basis
    /// [`CaptureNormalize::Primary`](crate::external::CaptureNormalize::Primary),
    /// which is the default and what every self-hit-testing screen uses — those
    /// are the same rectangle from the same frame, so the two agree. For a widget
    /// whose basis is a sub-tag they differ, and the arrival's rectangle is the
    /// right one: it is the rectangle the fraction is a fraction OF, which is
    /// what the delivered value actually means. Reading the store instead would
    /// be a second derivation of the extent, which is precisely what R1727
    /// removed by making the rectangle travel with the fraction.
    #[must_use]
    pub fn resolved(&self) -> (u32, u32) {
        (
            pixel_of(self.at.0, self.over.w),
            pixel_of(self.at.1, self.over.h),
        )
    }

    /// Whether the window system's cursor was inside the rectangle the fraction
    /// was taken over.
    ///
    /// Asked through the **rectangle's own** containment rule
    /// ([`Rect::holds`](crate::scene::Rect::holds)) rather than by comparing the
    /// relative offsets against the extent, because "is this pixel inside this
    /// rectangle" already has one definition in this workspace and a hit test is
    /// exactly where a second one puts a press somewhere nobody aimed.
    #[must_use]
    pub fn was_inside(&self) -> bool {
        let px = u32::try_from(clamped_i64(self.cursor.0.floor()));
        let py = u32::try_from(clamped_i64(self.cursor.1.floor()));
        match (px, py) {
            (Ok(x), Ok(y)) => self.over.holds(x, y),
            _ => false,
        }
    }

    /// How the two accounts stand.
    #[must_use]
    pub fn landing(&self) -> Landing {
        if !self.was_inside() {
            return Landing::Strayed;
        }
        let (ix, iy) = self.inside();
        let (rx, ry) = self.resolved();
        let (dx, dy) = (i64::from(rx) - ix, i64::from(ry) - iy);
        if dx == 0 && dy == 0 {
            Landing::Exact
        } else {
            Landing::Drifted {
                by: (narrow(dx), narrow(dy)),
            }
        }
    }

    /// The size of the rectangle the fraction was taken over, in the units
    /// [`PointerReading::extent`] states.
    #[must_use]
    pub fn extent(&self) -> (f32, f32) {
        #[allow(
            clippy::cast_precision_loss,
            reason = "a logical-pixel rect is small enough to round-trip f32 exactly — \
                      the same statement `capture_rel_coords` makes when it builds the reading"
        )]
        (self.over.w as f32, self.over.h as f32)
    }
}

/// A window-logical coordinate as an `i64`, saturating at the ends.
///
/// The cast is bounded rather than assumed because this runs on a value the
/// window system supplied, inside a dispatcher where a panic takes the whole
/// surface down.
fn clamped_i64(value: f64) -> i64 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "clamped to the i64 range on the line above"
    )]
    {
        value.clamp(-9.0e18, 9.0e18) as i64
    }
}

/// A drift as an `i32`, saturating — a drift wider than a screen is already a
/// defect and the exact number stops mattering.
fn narrow(value: i64) -> i32 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "clamped to the i32 range on the line above"
    )]
    {
        value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    }
}

/// ★★★★★ R1737 — every arrival one surface has been delivered, as a tally plus
/// the evidence.
///
/// # Why this is a tally and not just the last arrival
///
/// The first draft of this module kept only the last arrival, and the gate
/// written over it *claimed* to check every arrival a session caused while
/// actually checking one — the last. That is the exact shape R1736 found in
/// `scene/pointer_target`, where eight of nine probes could only ever rescue a
/// rectangle and the ninth was the middle: **a check that samples the one event
/// nobody chose is a check whose coverage is an accident.**
///
/// With a tally the framework does the counting, so a caller may drive six
/// hundred pointer positions and ask **once**, and the answer is about all six
/// hundred. It is also what makes the check affordable: asking after every move
/// was a round trip per pixel, and a probe expensive enough to load the machine
/// starts timing out the very screens it is measuring.
///
/// [`drifted_at`](Self::drifted_at) keeps the **first** drift rather than the
/// last, because that is the one that happened before anything downstream
/// reacted to it — and because a sweep that drifts everywhere reports the same
/// thing either way, while a sweep that drifts once wants exactly that one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceArrivals {
    /// The most recent arrival, whatever its landing — where this surface thinks
    /// the pointer is right now.
    pub last: PointerArrival,
    /// How many arrivals have been delivered to this surface.
    pub delivered: u64,
    /// How many of them landed on the pixel the pointer was over.
    pub exact: u64,
    /// How many arrived with the cursor outside the rectangle (a capture lock).
    pub strayed: u64,
    /// How many named a different pixel of the rectangle. Any of these is a
    /// defect.
    pub drifted: u64,
    /// The FIRST drifted arrival, kept as the evidence, or `None` when none has.
    pub drifted_at: Option<PointerArrival>,
}

impl SurfaceArrivals {
    /// The tally after one arrival, from nothing.
    fn first(arrival: PointerArrival) -> Self {
        let mut tally = Self {
            last: arrival,
            delivered: 0,
            exact: 0,
            strayed: 0,
            drifted: 0,
            drifted_at: None,
        };
        tally.count(arrival);
        tally
    }

    /// Fold one more arrival in.
    fn count(&mut self, arrival: PointerArrival) {
        self.last = arrival;
        self.delivered = self.delivered.saturating_add(1);
        match arrival.landing() {
            Landing::Exact => self.exact = self.exact.saturating_add(1),
            Landing::Strayed => self.strayed = self.strayed.saturating_add(1),
            Landing::Drifted { .. } => {
                self.drifted = self.drifted.saturating_add(1);
                if self.drifted_at.is_none() {
                    self.drifted_at = Some(arrival);
                }
            }
        }
    }

    /// Whether any arrival this surface was delivered landed on the wrong pixel.
    #[must_use]
    pub const fn is_defective(&self) -> bool {
        self.drifted > 0
    }
}

/// Record where a pointer arrived in `surface_tag` — called by the layer that
/// resolves the reading, never by a widget.
///
/// ★ Public for the reason
/// [`record_painted_regions`](crate::painted::record_painted_regions) is: the
/// recorder lives in `pinion-runtime`, one crate up, and this is the framework's
/// own bookkeeping. A surface calling it would be *declaring* where the pointer
/// arrived rather than being told — which is the arrangement this replaces.
pub fn record_pointer_arrival(surface_tag: &str, arrival: PointerArrival) {
    ARRIVALS.with(|arrivals| {
        let mut held = arrivals.borrow_mut();
        if let Some(tally) = held.get_mut(surface_tag) {
            tally.count(arrival);
        } else {
            held.insert(surface_tag.to_owned(), SurfaceArrivals::first(arrival));
        }
    });
}

/// Every arrival `surface_tag` has been delivered, or `None` for a surface no
/// pointer has reached.
///
/// `None` is the truthful answer and is **not** the same as an arrival at the
/// origin: a screen nobody has pointed at is not a screen the pointer arrived
/// at (0, 0), and collapsing the two would let an unexercised screen read as a
/// checked one — the distinction
/// [`surface_size`](crate::external::surface_size) and
/// [`PointerTarget::Unanswered`](crate::external::PointerTarget::Unanswered)
/// both draw.
#[must_use]
pub fn pointer_arrival(surface_tag: &str) -> Option<SurfaceArrivals> {
    ARRIVALS.with(|arrivals| arrivals.borrow().get(surface_tag).copied())
}

/// Forget a surface's arrivals, so a screen that is gone cannot answer for the
/// next one with the same tag.
pub fn forget_pointer_arrival(surface_tag: &str) {
    ARRIVALS.with(|arrivals| {
        arrivals.borrow_mut().remove(surface_tag);
    });
}

/// Every surface a pointer has arrived in, in tag order, with its tally.
///
/// For the census, which must be able to say what it did **not** cover: a
/// report built by asking only the surfaces it already knows about cannot
/// notice one nobody thought to name.
#[must_use]
pub fn arrivals() -> Vec<(String, SurfaceArrivals)> {
    ARRIVALS.with(|arrivals| {
        arrivals
            .borrow()
            .iter()
            .map(|(tag, tally)| (tag.clone(), *tally))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Landing, PointerArrival, arrivals, forget_pointer_arrival, pointer_arrival,
        record_pointer_arrival,
    };
    use crate::scene::Rect;

    /// The round trip R1736 repaired, asserted here as the framework's own
    /// verdict rather than as a screen's observation: every pixel of a few
    /// awkward extents arrives as itself.
    #[test]
    fn r1737_every_pixel_of_a_rectangle_lands_exactly() {
        for (w, h) in [
            (1_u32, 1_u32),
            (37, 5),
            (200, 120),
            (1440, 900),
            (2494, 1287),
        ] {
            let over = Rect::new(11, 7, w, h);
            for px in 0..w {
                #[allow(clippy::cast_precision_loss)]
                let frac = px as f32 / w as f32;
                let arrival = PointerArrival::new(
                    over,
                    (f64::from(over.x + px), f64::from(over.y)),
                    (frac, 0.0),
                );
                assert_eq!(
                    arrival.landing(),
                    Landing::Exact,
                    "column {px} of {w} resolved {:?} for a pointer over {:?}",
                    arrival.resolved(),
                    arrival.inside(),
                );
            }
            for py in 0..h {
                #[allow(clippy::cast_precision_loss)]
                let frac = py as f32 / h as f32;
                let arrival = PointerArrival::new(
                    over,
                    (f64::from(over.x), f64::from(over.y + py)),
                    (0.0, frac),
                );
                assert_eq!(arrival.landing(), Landing::Exact, "row {py} of {h}");
            }
        }
    }

    /// ★★★★★ The gate must be able to CONVICT, and the fixture proving it is
    /// the defect R1736 found: the fraction says one pixel and the cursor says
    /// the next one.
    ///
    /// Without this the check above is satisfied by a verdict that answers
    /// `Exact` unconditionally, which is the shape this repository has recorded
    /// four times — a fixture that cannot tell two expressions apart is the
    /// same as no gate.
    #[test]
    fn r1737_a_fraction_that_names_the_wrong_pixel_is_a_defect() {
        let over = Rect::new(0, 0, 600, 400);
        // The cursor is over pixel 434; the fraction delivered is the one a
        // truncating cast produced, which names 433.
        let arrival = PointerArrival::new(over, (434.0, 10.0), (433.0 / 600.0, 10.0 / 400.0));
        assert_eq!(arrival.inside(), (434, 10));
        assert_eq!(arrival.resolved(), (433, 10));
        assert_eq!(arrival.landing(), Landing::Drifted { by: (-1, 0) });
        assert!(arrival.landing().is_defect());
        assert_eq!(arrival.landing().word(), "drifted");
    }

    /// A cursor outside the rectangle is what a capture lock produces on
    /// purpose, so the two accounts differing there is not a defect — and the
    /// verdict says which case it is rather than folding it into either
    /// neighbour.
    #[test]
    fn r1737_a_cursor_that_left_the_rectangle_strayed_rather_than_drifted() {
        let over = Rect::new(100, 100, 50, 50);
        for cursor in [(80.0, 120.0), (120.0, 80.0), (400.0, 120.0), (120.0, 400.0)] {
            let arrival = PointerArrival::new(over, cursor, (-0.4, 0.4));
            assert_eq!(arrival.landing(), Landing::Strayed, "cursor {cursor:?}");
            assert!(!arrival.landing().is_defect());
        }
        // And the last pixel INSIDE is not strayed — the half-open edge is the
        // rectangle's own rule, not a second one written here.
        let inside = PointerArrival::new(over, (149.0, 149.0), (49.0 / 50.0, 49.0 / 50.0));
        assert_eq!(inside.landing(), Landing::Exact);
    }

    /// A degenerate rectangle is not a rectangle to arrive in, and the router
    /// collapses the fraction to zero rather than dividing by zero — so the
    /// verdict must not read that collapse as an exact landing at the origin.
    #[test]
    fn r1737_a_rectangle_of_no_extent_is_strayed_not_exact() {
        let arrival = PointerArrival::new(Rect::new(10, 10, 0, 0), (10.0, 10.0), (0.0, 0.0));
        assert_eq!(arrival.landing(), Landing::Strayed);
    }

    /// The reading is rebuilt from the rectangle, so a caller reading the
    /// arrival and a surface reading the delivered reading divide by the same
    /// extent by construction.
    #[test]
    fn r1737_the_reading_is_rebuilt_from_the_one_rectangle() {
        let over = Rect::new(4, 8, 320, 240);
        let arrival = PointerArrival::new(over, (100.0, 100.0), (0.3, 0.4));
        let reading = arrival.reading();
        assert_eq!(reading.extent, (320.0, 240.0));
        assert_eq!(reading.at, (0.3, 0.4));
        assert_eq!(arrival.extent(), reading.extent);
    }

    /// A surface no pointer has reached answers `None`, and it is not the same
    /// answer as an arrival at the origin.
    #[test]
    fn r1737_an_unpointed_surface_answers_nothing_rather_than_the_origin() {
        forget_pointer_arrival("r1737.probe");
        assert!(pointer_arrival("r1737.probe").is_none());
        let at_origin = PointerArrival::new(Rect::new(0, 0, 10, 10), (0.0, 0.0), (0.0, 0.0));
        record_pointer_arrival("r1737.probe", at_origin);
        let tally = pointer_arrival("r1737.probe").expect("just recorded");
        assert_eq!(tally.last, at_origin);
        assert_eq!(tally.delivered, 1);
        assert!(
            arrivals().iter().any(|(tag, _)| tag == "r1737.probe"),
            "the census lists every surface that has been pointed at"
        );
        forget_pointer_arrival("r1737.probe");
        assert!(pointer_arrival("r1737.probe").is_none());
    }

    /// ★★★★★ The tally is about EVERY arrival, not the last one.
    ///
    /// The check this replaces sampled the final event, so a sweep of six
    /// hundred positions with a drift at the third would have reported clean —
    /// which is R1736's own finding one level up (a gate whose coverage is an
    /// accident of which point it happened to look at). One drift among many
    /// exact ones must survive to the answer, and the evidence kept must be the
    /// FIRST one.
    #[test]
    fn r1737_one_drift_among_many_exact_arrivals_still_convicts() {
        forget_pointer_arrival("r1737.tally");
        let over = Rect::new(0, 0, 600, 400);
        let exact = |px: u32| {
            #[allow(clippy::cast_precision_loss)]
            PointerArrival::new(
                over,
                (f64::from(px), 10.0),
                (px as f32 / 600.0, 10.0 / 400.0),
            )
        };
        for px in 100..110 {
            record_pointer_arrival("r1737.tally", exact(px));
        }
        // The drift, in the middle, and then more exact ones after it.
        let drift = PointerArrival::new(over, (434.0, 10.0), (433.0 / 600.0, 10.0 / 400.0));
        record_pointer_arrival("r1737.tally", drift);
        let second_drift = PointerArrival::new(over, (500.0, 10.0), (499.0 / 600.0, 10.0 / 400.0));
        record_pointer_arrival("r1737.tally", second_drift);
        for px in 200..210 {
            record_pointer_arrival("r1737.tally", exact(px));
        }
        let tally = pointer_arrival("r1737.tally").expect("recorded");
        assert_eq!(tally.delivered, 22);
        assert_eq!(tally.exact, 20);
        assert_eq!(tally.drifted, 2);
        assert_eq!(tally.strayed, 0);
        assert!(
            tally.is_defective(),
            "two drifts among twenty exact convict"
        );
        assert_eq!(
            tally.drifted_at,
            Some(drift),
            "the evidence is the FIRST drift, not the last"
        );
        // And the last arrival is still readable, because "where does this
        // surface think the pointer is now" is a different question from "did
        // any arrival go wrong".
        assert_eq!(tally.last.landing(), Landing::Exact);
        assert_eq!(tally.last.resolved(), (209, 10));
        forget_pointer_arrival("r1737.tally");
    }

    /// ★ The published predicate and the counting rule are the same rule.
    ///
    /// [`Landing::is_defect`] is what a consumer holding ONE arrival reads
    /// ([`SurfaceArrivals::last`], [`SurfaceArrivals::drifted_at`]), and
    /// [`SurfaceArrivals::count`] decides the same thing with an exhaustive
    /// match — kept exhaustive on purpose, because a fourth arm falling silently
    /// into `exact` is worse than the small duplication. This is what holds the
    /// two together: for every arm, "is a defect" and "increments `drifted`" must
    /// agree. Without it the predicate could drift from the tally and each
    /// caller would be right about a different rule.
    #[test]
    fn r1737_the_defect_predicate_and_the_tally_agree_on_every_arm() {
        let over = Rect::new(0, 0, 600, 400);
        let cases = [
            // exact, drifted, strayed — one of each arm.
            PointerArrival::new(over, (100.0, 10.0), (100.0 / 600.0, 10.0 / 400.0)),
            PointerArrival::new(over, (434.0, 10.0), (433.0 / 600.0, 10.0 / 400.0)),
            PointerArrival::new(over, (900.0, 10.0), (1.5, 10.0 / 400.0)),
        ];
        let mut seen = Vec::new();
        for arrival in cases {
            forget_pointer_arrival("r1737.agree");
            record_pointer_arrival("r1737.agree", arrival);
            let tally = pointer_arrival("r1737.agree").expect("recorded");
            assert_eq!(
                arrival.landing().is_defect(),
                tally.drifted == 1,
                "the predicate and the tally disagree about {:?}",
                arrival.landing()
            );
            assert_eq!(
                arrival.landing().is_defect(),
                tally.is_defective(),
                "and about the surface"
            );
            seen.push(arrival.landing());
        }
        forget_pointer_arrival("r1737.agree");
        // ★ And the fixture reaches all three arms, so this cannot pass by
        // testing one case three times — the shape that has made a gate
        // meaningless four times in this repository.
        assert_eq!(
            seen,
            [
                Landing::Exact,
                Landing::Drifted { by: (-1, 0) },
                Landing::Strayed
            ]
        );
    }

    /// A strayed arrival is counted apart and does not convict.
    #[test]
    fn r1737_strayed_arrivals_are_counted_and_are_not_defects() {
        forget_pointer_arrival("r1737.stray");
        let over = Rect::new(100, 100, 50, 50);
        record_pointer_arrival(
            "r1737.stray",
            PointerArrival::new(over, (120.0, 120.0), (20.0 / 50.0, 20.0 / 50.0)),
        );
        record_pointer_arrival(
            "r1737.stray",
            PointerArrival::new(over, (900.0, 120.0), (16.0, 20.0 / 50.0)),
        );
        let tally = pointer_arrival("r1737.stray").expect("recorded");
        assert_eq!(
            (tally.delivered, tally.exact, tally.strayed, tally.drifted),
            (2, 1, 1, 0)
        );
        assert!(!tally.is_defective());
        assert!(tally.drifted_at.is_none());
        forget_pointer_arrival("r1737.stray");
    }
}
