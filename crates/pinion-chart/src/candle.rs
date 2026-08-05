//! R1567 — the datum whose middle landmarks are **deliberately unordered**.
//!
//! R1553 gave this crate its first datum with extent: a
//! [`Distribution`](crate::Distribution) occupies a span of the value axis
//! and carries interior landmarks. A [`Candle`] is the other datum of that
//! shape — open / high / low / close over one trading session, the value Qt
//! spells `QCandlestickSet`. The two look alike enough that this crate's own
//! documentation claimed the candlestick would be the box plot's *second
//! consumer*.
//!
//! # Why a candle is not a distribution
//!
//! That claim was wrong, and the reason is the whole of this module.
//!
//! [`Distribution`](crate::Distribution)'s five landmarks are **totally
//! ordered by construction** — derivation cannot produce an inverted box and
//! [`from_summary`](crate::Distribution::from_summary) rejects one. A candle
//! has four landmarks and only *three* order relations among them:
//!
//! ```text
//!     low <= min(open, close)   and   max(open, close) <= high
//! ```
//!
//! Between `open` and `close` there is **no** relation, and that absence is
//! not a weaker invariant — it is the datum's entire content. Which of the
//! two is larger is what the reader came for: a session that opened at 100
//! and closed at 104 and a session that opened at 104 and closed at 100 have
//! the same four numbers, the same extent, the same box, and opposite
//! meanings. A type whose invariant is "non-decreasing" cannot hold a value
//! whose content is "which of these two is larger", so folding a candle into
//! a `Distribution` would silently discard the fact the chart exists to show.
//!
//! What the two *do* share is geometry — a band across a slot with landmarks
//! mapped through the value axis — and that is shared where it belongs, in
//! the paint, not in the datum.
//!
//! # What Qt's `QCandlestickSet` cannot say
//!
//! * **Nothing is checked.** It is five `qreal`s with `void` setters, so a
//!   transposed high and low, an open outside the extremes, or a NaN close
//!   all paint whatever geometry they imply. [`Candle::new`] refuses each and
//!   names the slot ([`CandlePosition`]).
//! * **The direction is not a value.** `QCandlestickSeries` carries
//!   `increasingColor` and `decreasingColor` — two *paint* properties on the
//!   series — and the set itself has no accessor at all, so a consumer that
//!   wants to state the direction re-derives `close > open`, a second
//!   implementation of the rule the painter already applied. Here
//!   [`Candle::direction`] is the rule, asked once.
//! * **A doji has no name.** Qt's documented rule is that the increasing
//!   colour is used "when the close value is higher than the open value", so
//!   a session that closed exactly where it opened — the *doji*, the single
//!   most-read signal in the form — is silently painted as a losing session.
//!   [`Direction::Doji`] is its own arm.
//! * **The derived quantities are absent.** The body, the two shadows, the
//!   range and the change are what a candle is read by, and `QCandlestickSet`
//!   exposes none of them, so every consumer computes its own.

use std::cmp::Ordering;
use std::fmt::Write as _;

use crate::ticks::{format_axis_tick, format_time_stamp};

/// Which of a candle's four values a message is about — the names of Qt's
/// `QCandlestickSet` properties, so a refusal can say *which* slot broke the
/// ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandlePosition {
    /// The session's first traded price.
    Open,
    /// The session's highest traded price.
    High,
    /// The session's lowest traded price.
    Low,
    /// The session's last traded price.
    Close,
}

impl CandlePosition {
    /// This slot's name, for a message or a wire field.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::High => "high",
            Self::Low => "low",
            Self::Close => "close",
        }
    }
}

impl std::fmt::Display for CandlePosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Which way a session went — the fact a candlestick chart exists to show.
///
/// Three arms, where Qt has two colours and no accessor. `QCandlestickSeries`
/// documents `increasingColor` as the brush used "when the close value is
/// higher than the open value", which makes the comparison strict and leaves
/// the equal case to the *other* branch: a Qt doji is painted as a losing
/// session, and nothing in the API can be asked otherwise.
///
/// The comparison is exact. A doji is defined by the tick data closing at the
/// price it opened at, not by closing near it, and a tolerance would make the
/// classification depend on the instrument's price scale — the same argument
/// [`QuantileMethod`](crate::QuantileMethod) makes for naming its definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// `close > open` — Qt's `increasingColor` case.
    Rising,
    /// `close < open`.
    Falling,
    /// `close == open` — a *doji*, drawn as a bodyless cross.
    Doji,
}

impl Direction {
    /// This direction's name, for a readout or a wire field.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rising => "rising",
            Self::Falling => "falling",
            Self::Doji => "doji",
        }
    }

    /// How the body is drawn — the direction's **second, hue-independent**
    /// encoding.
    ///
    /// The traditional Japanese form draws a rising session hollow and a
    /// falling one solid, and it predates colour entirely. Qt encodes the
    /// direction in hue alone (`increasingColor` / `decreasingColor`), so a
    /// reader with deuteranopia — or a monochrome print, or a screenshot run
    /// through a grayscale pipeline — is handed a chart whose single most
    /// important fact has been erased. Carrying the fill as a *declaration*
    /// rather than a palette choice is what makes the redundancy checkable:
    /// see [`CandlestickChart::direction_contrast`](crate::CandlestickChart::direction_contrast)
    /// for the hue half.
    ///
    /// A doji is solid, and its identity does not depend on that: its body
    /// has zero height, so it draws as a line whichever fill it is given.
    #[must_use]
    pub const fn body_fill(self) -> BodyFill {
        match self {
            Self::Rising => BodyFill::Hollow,
            Self::Falling | Self::Doji => BodyFill::Solid,
        }
    }
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Whether a candle body is drawn outlined-only or filled — see
/// [`Direction::body_fill`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyFill {
    /// Outline only: the body's fill is fully transparent.
    Hollow,
    /// Filled with the direction's colour.
    Solid,
}

impl BodyFill {
    /// This fill's name, for a readout or a wire field.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Hollow => "hollow",
            Self::Solid => "solid",
        }
    }

    /// The alpha this fill paints the body's interior at.
    ///
    /// Zero is the whole point of [`Hollow`](Self::Hollow): a "hollow" body
    /// drawn at a low but non-zero alpha is a *shade*, and a shade is a hue
    /// distinction wearing a different name — it disappears in exactly the
    /// grayscale and colour-blind cases the second encoding exists for.
    #[must_use]
    pub const fn alpha(self) -> u8 {
        match self {
            Self::Hollow => 0x00,
            Self::Solid => 0xFF,
        }
    }
}

/// Why a [`Candle`] could not be built.
///
/// Every arm names the input that was wrong, because these are all *caller*
/// errors and a caller cannot fix one it cannot locate. Qt reports none of
/// them: `QCandlestickSet` accepts any five doubles in any relation.
///
/// **No arm carries a non-finite number**, for the reason
/// [`DistributionError`](crate::DistributionError) records: `NaN != NaN`, so
/// an error holding one would not equal itself and a caller could never match
/// on it. The checks therefore run in a fixed order — the instant, then the
/// four values' finiteness, then the extremes' order, then containment — so
/// every `f64` these arms carry is already known to be finite and comparable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CandleError {
    /// The session's instant is NaN or infinite, so the candle has no place
    /// on either reading of the x-axis.
    InstantNotFinite,
    /// One of the four prices is NaN or infinite.
    NotFinite {
        /// Which slot it was.
        at: CandlePosition,
    },
    /// `high` is below `low`: the two extremes are transposed. Qt paints the
    /// resulting inverted wick in silence.
    ExtremesInverted {
        /// The value given as the session low.
        low: f64,
        /// The value given as the session high, which is below it.
        high: f64,
    },
    /// `at` (an [`Open`](CandlePosition::Open) or a
    /// [`Close`](CandlePosition::Close)) holds `value`, outside the
    /// `low ..= high` band the extremes declare — a session that traded
    /// outside its own range.
    OutsideExtremes {
        /// Which of the two middle slots it was.
        at: CandlePosition,
        /// What that slot held.
        value: f64,
        /// The session low.
        low: f64,
        /// The session high.
        high: f64,
    },
}

impl std::fmt::Display for CandleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InstantNotFinite => f.write_str("session instant is not finite"),
            Self::NotFinite { at } => write!(f, "{at} is not finite"),
            Self::ExtremesInverted { low, high } => {
                write!(f, "high {high} is below low {low}")
            }
            Self::OutsideExtremes {
                at,
                value,
                low,
                high,
            } => write!(f, "{at} is {value}, outside the range {low}\u{2013}{high}"),
        }
    }
}

impl std::error::Error for CandleError {}

/// One trading session: an instant and four prices, of which two are ordered
/// against the extremes and **not against each other**.
///
/// Build it with [`new`](Self::new), which is the only way in — the fields
/// are private because the invariant is the type's whole content, and a
/// `pub` field would let a caller reach a state the constructor refuses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candle {
    instant: f64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}

impl Candle {
    /// A session at `instant` (epoch **milliseconds**, UTC — the unit R1529's
    /// time axis speaks) with the four prices in the conventional OHLC order.
    ///
    /// # Errors
    ///
    /// [`CandleError::InstantNotFinite`] or [`CandleError::NotFinite`] naming
    /// the slot; [`CandleError::ExtremesInverted`] when `high < low`; and
    /// [`CandleError::OutsideExtremes`] naming the first of `open` / `close`
    /// that lies outside them. `QCandlestickSet` performs none of these
    /// checks and paints the result.
    pub fn new(
        instant: f64,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
    ) -> Result<Self, CandleError> {
        use CandlePosition as P;
        if !instant.is_finite() {
            return Err(CandleError::InstantNotFinite);
        }
        for (at, value) in [
            (P::Open, open),
            (P::High, high),
            (P::Low, low),
            (P::Close, close),
        ] {
            if !value.is_finite() {
                return Err(CandleError::NotFinite { at });
            }
        }
        if high < low {
            return Err(CandleError::ExtremesInverted { low, high });
        }
        for (at, value) in [(P::Open, open), (P::Close, close)] {
            if value < low || value > high {
                return Err(CandleError::OutsideExtremes {
                    at,
                    value,
                    low,
                    high,
                });
            }
        }
        Ok(Self {
            instant,
            open,
            high,
            low,
            close,
        })
    }

    /// The session's instant, in epoch milliseconds (UTC).
    #[must_use]
    pub const fn instant(&self) -> f64 {
        self.instant
    }

    /// The session's first traded price.
    #[must_use]
    pub const fn open(&self) -> f64 {
        self.open
    }

    /// The session's highest traded price.
    #[must_use]
    pub const fn high(&self) -> f64 {
        self.high
    }

    /// The session's lowest traded price.
    #[must_use]
    pub const fn low(&self) -> f64 {
        self.low
    }

    /// The session's last traded price.
    #[must_use]
    pub const fn close(&self) -> f64 {
        self.close
    }

    /// The value at `at` — the one enumeration a report walks, so a slot
    /// cannot be added to the type and forgotten by a measurement.
    #[must_use]
    pub const fn at(&self, at: CandlePosition) -> f64 {
        match at {
            CandlePosition::Open => self.open,
            CandlePosition::High => self.high,
            CandlePosition::Low => self.low,
            CandlePosition::Close => self.close,
        }
    }

    /// The four slots in a fixed order — the enumeration
    /// [`at`](Self::at) is the accessor for.
    #[must_use]
    pub const fn positions() -> [CandlePosition; 4] {
        use CandlePosition as P;
        [P::Open, P::High, P::Low, P::Close]
    }

    /// Which way the session went. See [`Direction`] for why the equal case
    /// is its own arm and why the comparison is exact.
    ///
    /// Both values are finite by construction, so the comparison is total and
    /// the fallback arm is unreachable rather than a silent default.
    #[must_use]
    pub fn direction(&self) -> Direction {
        match self.close.partial_cmp(&self.open) {
            Some(Ordering::Greater) => Direction::Rising,
            Some(Ordering::Less) => Direction::Falling,
            _ => Direction::Doji,
        }
    }

    /// The body's extent on the value axis, ascending — `min(open, close)`
    /// to `max(open, close)`.
    ///
    /// This is the projection that *loses* the direction, which is exactly
    /// why [`direction`](Self::direction) is asked of the candle and not
    /// recovered from the geometry downstream.
    #[must_use]
    pub fn body(&self) -> (f64, f64) {
        (self.open.min(self.close), self.open.max(self.close))
    }

    /// The body's height, `|close - open|` — zero for a doji.
    #[must_use]
    pub fn body_height(&self) -> f64 {
        (self.close - self.open).abs()
    }

    /// The session's full range, `high - low`.
    #[must_use]
    pub fn range(&self) -> f64 {
        self.high - self.low
    }

    /// The upper shadow (Qt draws it as the upper half of the wick):
    /// `high - max(open, close)`. Non-negative by construction.
    #[must_use]
    pub fn upper_shadow(&self) -> f64 {
        self.high - self.body().1
    }

    /// The lower shadow: `min(open, close) - low`. Non-negative by
    /// construction.
    #[must_use]
    pub fn lower_shadow(&self) -> f64 {
        self.body().0 - self.low
    }

    /// The signed change over the session, `close - open` — the quantity
    /// [`direction`](Self::direction) is the sign of.
    #[must_use]
    pub fn change(&self) -> f64 {
        self.close - self.open
    }

    /// The change as a fraction of the open, or `None` when that quotient is
    /// not a finite number — which is what a session opening at zero
    /// produces. The ratio does not *exist* there rather than being unknown,
    /// the stance [`Distribution::notch`](crate::Distribution::notch) takes
    /// for a statistic its provenance cannot support; answering with an
    /// infinity would put one in a caption.
    #[must_use]
    pub fn change_ratio(&self) -> Option<f64> {
        let ratio = self.change() / self.open;
        ratio.is_finite().then_some(ratio)
    }

    /// The extent this session occupies on the value axis — `low` to `high`,
    /// which contains the body by construction.
    #[must_use]
    pub const fn extent(&self) -> (f64, f64) {
        (self.low, self.high)
    }

    /// [`extent`](Self::extent) restricted to the strictly positive part, or
    /// `None` when nothing here is positive — the auto-domain source for a
    /// **logarithmic** value axis, which is the axis a long price history
    /// actually wants (equal *ratios* over equal pixel spans is what makes a
    /// 10% move read the same at 20 and at 200).
    #[must_use]
    pub fn positive_extent(&self) -> Option<(f64, f64)> {
        let mut span: Option<(f64, f64)> = None;
        for at in Self::positions() {
            let v = self.at(at);
            if v > 0.0 {
                span = Some(match span {
                    Some((lo, hi)) => (lo.min(v), hi.max(v)),
                    None => (v, v),
                });
            }
        }
        span
    }

    /// The session as one line, at the precision `step` implies — the text a
    /// scrub tooltip and an assistive-technology readout both take, so the
    /// two can never state different numbers.
    ///
    /// The instant takes the full UTC stamp rather than a tick label, R1529's
    /// rule for a value that stands alone: `Mar 02` is legible in a row of
    /// axis labels and ambiguous out of one.
    #[must_use]
    pub fn readout(&self, step: f64) -> String {
        let q = |v: f64| format_axis_tick(v, step);
        let mut out = format!(
            "{}: open {}, high {}, low {}, close {}",
            format_time_stamp(self.instant),
            q(self.open),
            q(self.high),
            q(self.low),
            q(self.close),
        );
        let direction = self.direction();
        let change = self.change();
        // A negative number already carries its sign; a rise has to be given
        // one, because `+4` and `4` read differently next to a direction.
        let sign = if change > 0.0 { "+" } else { "" };
        let _ = write!(out, ", {direction} {sign}{}", q(change));
        if let Some(ratio) = self.change_ratio() {
            let _ = write!(out, " ({:+.2}%)", ratio * 100.0);
        }
        out
    }
}

/// The combined value extent across `candles`, or `None` when the slice is
/// empty — the auto-domain source for a candlestick chart's value axis.
#[must_use]
pub fn candle_bounds(candles: &[Candle]) -> Option<(f64, f64)> {
    candles
        .iter()
        .map(Candle::extent)
        .reduce(|(alo, ahi), (blo, bhi)| (alo.min(blo), ahi.max(bhi)))
}

/// [`candle_bounds`] restricted to strictly positive values — the auto-domain
/// source when the value axis is logarithmic.
#[must_use]
pub fn positive_candle_bounds(candles: &[Candle]) -> Option<(f64, f64)> {
    candles
        .iter()
        .filter_map(Candle::positive_extent)
        .reduce(|(alo, ahi), (blo, bhi)| (alo.min(blo), ahi.max(bhi)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-03-02T00:00:00Z, the instant the fixtures below are anchored on.
    const DAY: f64 = 1_772_409_600_000.0;

    fn rising() -> Candle {
        Candle::new(DAY, 100.0, 106.0, 99.0, 104.0).expect("ordered")
    }

    /// ★ The round's claim, stated as the thing a `Distribution` cannot hold:
    /// two candles with the SAME four numbers, the same extent and the same
    /// body, and opposite meanings. Any representation that orders the middle
    /// pair collapses these two into one value.
    #[test]
    fn r1567_the_same_four_numbers_are_two_opposite_sessions() {
        let up = Candle::new(DAY, 100.0, 106.0, 99.0, 104.0).expect("ordered");
        let down = Candle::new(DAY, 104.0, 106.0, 99.0, 100.0).expect("ordered");

        // Everything a totally-ordered five-landmark summary would keep:
        assert_eq!(up.extent(), down.extent());
        assert_eq!(up.body(), down.body());
        assert!((up.body_height() - down.body_height()).abs() < 1e-12);
        assert!((up.upper_shadow() - down.upper_shadow()).abs() < 1e-12);
        assert!((up.lower_shadow() - down.lower_shadow()).abs() < 1e-12);

        // ...and the one thing it would lose, which is the datum.
        assert_eq!(up.direction(), Direction::Rising);
        assert_eq!(down.direction(), Direction::Falling);
        assert!((up.change() - 4.0).abs() < 1e-12);
        assert!((down.change() + 4.0).abs() < 1e-12);
    }

    /// ★ A doji is its own arm. Qt's documented rule uses a strict `>` for
    /// the increasing colour, so a session that closed exactly where it
    /// opened is painted as a losing one and nothing can be asked otherwise.
    ///
    /// The counterfactual is the pair either side: one tick up is `Rising`
    /// and one tick down is `Falling`, so `Doji` is not "anything near
    /// equal".
    #[test]
    fn r1567_a_doji_is_named_not_folded_into_falling() {
        let doji = Candle::new(DAY, 100.0, 103.0, 97.0, 100.0).expect("ordered");
        assert_eq!(doji.direction(), Direction::Doji);
        assert!(
            doji.body_height().abs() < f64::EPSILON,
            "a doji has no body"
        );
        assert_eq!(doji.change_ratio(), Some(0.0));

        let up = Candle::new(DAY, 100.0, 103.0, 97.0, 100.000_001).expect("ordered");
        let down = Candle::new(DAY, 100.0, 103.0, 97.0, 99.999_999).expect("ordered");
        assert_eq!(up.direction(), Direction::Rising);
        assert_eq!(down.direction(), Direction::Falling);
    }

    /// ★ The direction survives the loss of hue. Rising is hollow, everything
    /// else solid, and the two fills are opaque-vs-transparent rather than
    /// two shades — a shade is a hue distinction under another name and dies
    /// in the same grayscale pipeline.
    #[test]
    fn r1567_the_fill_encodes_the_direction_without_colour() {
        assert_eq!(Direction::Rising.body_fill(), BodyFill::Hollow);
        assert_eq!(Direction::Falling.body_fill(), BodyFill::Solid);
        assert_eq!(Direction::Doji.body_fill(), BodyFill::Solid);
        assert_eq!(BodyFill::Hollow.alpha(), 0);
        assert_eq!(BodyFill::Solid.alpha(), 255);
    }

    /// ★ Every relation the type promises is refused when broken, and the
    /// refusal NAMES the slot. `QCandlestickSet` has `void` setters and
    /// checks none of this.
    #[test]
    fn r1567_a_broken_relation_is_refused_by_slot() {
        // The extremes transposed.
        assert_eq!(
            Candle::new(DAY, 100.0, 99.0, 106.0, 104.0),
            Err(CandleError::ExtremesInverted {
                low: 106.0,
                high: 99.0
            })
        );
        // An open above the session high.
        assert_eq!(
            Candle::new(DAY, 200.0, 106.0, 99.0, 104.0),
            Err(CandleError::OutsideExtremes {
                at: CandlePosition::Open,
                value: 200.0,
                low: 99.0,
                high: 106.0,
            })
        );
        // A close below the session low.
        assert_eq!(
            Candle::new(DAY, 100.0, 106.0, 99.0, 1.0),
            Err(CandleError::OutsideExtremes {
                at: CandlePosition::Close,
                value: 1.0,
                low: 99.0,
                high: 106.0,
            })
        );
        // Non-finite, per slot, and never carried in the error value.
        assert_eq!(
            Candle::new(DAY, 100.0, f64::NAN, 99.0, 104.0),
            Err(CandleError::NotFinite {
                at: CandlePosition::High
            })
        );
        assert_eq!(
            Candle::new(f64::INFINITY, 100.0, 106.0, 99.0, 104.0),
            Err(CandleError::InstantNotFinite)
        );

        // The comparability those arms exist for.
        let err = Candle::new(DAY, 100.0, f64::NAN, 99.0, 104.0).expect_err("a NaN high");
        assert_eq!(
            err, err,
            "an error a caller cannot match on is not an error"
        );

        // Counterfactual: the well-formed version of the same numbers is
        // accepted, so nothing here is "`new` always fails".
        assert!(Candle::new(DAY, 100.0, 106.0, 99.0, 104.0).is_ok());
        // A flat session — every price equal — is ordered, not broken.
        assert!(Candle::new(DAY, 7.0, 7.0, 7.0, 7.0).is_ok());
    }

    /// ★ The derived readings a `QCandlestickSet` makes every consumer
    /// recompute, and their arithmetic identity: the two shadows and the body
    /// partition the range exactly.
    #[test]
    fn r1567_the_shadows_and_the_body_partition_the_range() {
        for c in [
            rising(),
            Candle::new(DAY, 104.0, 106.0, 99.0, 100.0).expect("ordered"),
            Candle::new(DAY, 100.0, 103.0, 97.0, 100.0).expect("ordered"),
            Candle::new(DAY, 7.0, 7.0, 7.0, 7.0).expect("ordered"),
        ] {
            let sum = c.upper_shadow() + c.body_height() + c.lower_shadow();
            assert!(
                (sum - c.range()).abs() < 1e-12,
                "{c:?}: {sum} does not partition {}",
                c.range()
            );
            assert!(c.upper_shadow() >= 0.0 && c.lower_shadow() >= 0.0, "{c:?}");
        }
    }

    /// ★ The percent change does not exist where the open is zero, and says
    /// so, rather than answering with an infinity a caption would print.
    #[test]
    fn r1567_the_ratio_is_absent_where_it_does_not_exist() {
        let zeroed = Candle::new(DAY, 0.0, 4.0, 0.0, 3.0).expect("ordered");
        assert_eq!(zeroed.change_ratio(), None);
        assert!((zeroed.change() - 3.0).abs() < 1e-12, "the change still is");

        let ratio = rising().change_ratio().expect("a non-zero open");
        assert!((ratio - 0.04).abs() < 1e-12, "{ratio}");
    }

    /// ★ The positive-only extent is the log axis's domain source and it
    /// measures every slot, not just the extremes.
    #[test]
    fn r1567_positive_extent_skips_what_a_log_axis_cannot_place() {
        let straddling = Candle::new(DAY, 0.0, 4.0, -1.0, 3.0).expect("ordered");
        let (lo, hi) = straddling.positive_extent().expect("some positive slot");
        assert!(
            (lo - 3.0).abs() < 1e-12,
            "the close is the smallest positive"
        );
        assert!((hi - 4.0).abs() < 1e-12);

        let all_negative = Candle::new(DAY, -3.0, -1.0, -9.0, -5.0).expect("ordered");
        assert!(all_negative.positive_extent().is_none());
    }

    /// ★ Combined bounds span every session, and the positive-only pair skips
    /// a wholly non-positive one instead of dragging the domain to zero.
    #[test]
    fn r1567_bounds_combine_every_session() {
        let a = rising();
        let b = Candle::new(DAY, -3.0, -1.0, -9.0, -5.0).expect("ordered");
        assert_eq!(candle_bounds(&[a, b]), Some((-9.0, 106.0)));
        assert_eq!(positive_candle_bounds(&[a, b]), Some((99.0, 106.0)));
        assert_eq!(candle_bounds(&[]), None);
        assert_eq!(positive_candle_bounds(&[]), None);
    }

    /// ★ The readout names the direction and the signed change, and stamps
    /// the instant in full — a lone value cannot lean on its neighbours for
    /// the date (R1529's rule).
    #[test]
    fn r1567_the_readout_states_the_direction_and_the_change() {
        let text = rising().readout(1.0);
        assert!(text.starts_with("2026-03-02 00:00:00: "), "{text}");
        assert!(text.contains("open 100"), "{text}");
        assert!(text.contains("close 104"), "{text}");
        assert!(text.contains("rising +4"), "{text}");
        assert!(text.contains("(+4.00%)"), "{text}");

        let falling = Candle::new(DAY, 104.0, 106.0, 99.0, 100.0).expect("ordered");
        let text = falling.readout(1.0);
        assert!(text.contains("falling -4"), "{text}");
        assert!(text.contains("(-3.85%)"), "{text}");

        // A session that opened at zero states the change and omits the
        // ratio, rather than printing an infinity.
        let text = Candle::new(DAY, 0.0, 4.0, 0.0, 3.0)
            .expect("ordered")
            .readout(1.0);
        assert!(text.contains("rising +3"), "{text}");
        assert!(!text.contains('%'), "no ratio where none exists: {text}");
    }
}
