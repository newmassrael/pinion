//! UTC civil-calendar arithmetic — the days-to-`(year, month, day)` pair a
//! time axis needs to land its ticks on calendar boundaries (R1529).
//!
//! # Why this is here rather than in a date crate
//!
//! `pinion-chart` depends on `pinion-core` and nothing else, deliberately
//! (see the crate doc). What a time axis needs from a calendar is exactly
//! two total functions over an integer day count, plus the field breakdown
//! they compose into; a date library would bring a parser, a formatter, a
//! timezone database and an offset type for it, ~none of which this axis
//! calls — the axis's own label format is a multi-resolution cascade, not a
//! `strftime` pattern. The crate already carries canonical algorithms with
//! their citations for the same reason (Heckbert's nice numbers in
//! [`crate::ticks`], Brandes-Köpf in `pinion-graph`).
//!
//! The algorithms are Howard Hinnant's `days_from_civil` /
//! `civil_from_days` (*chrono-Compatible Low-Level Date Algorithms*,
//! <https://howardhinnant.github.io/date_algorithms.html>) — the
//! proleptic-Gregorian pair C++20's `<chrono>` is specified against. They
//! are exact inverses over the whole supported range, which is what lets a
//! tick be snapped to a month boundary and then read back as one.
//!
//! # UTC only
//!
//! Every instant here is UTC. A local-time axis would need a timezone
//! database, and — the reason it is not merely deferred — it would make
//! every axis test read the *host's* configuration, which is the failure
//! [`crate::ticks`]'s own tests exist to avoid. d3 draws the same line
//! (`scaleUtc` beside `scaleTime`); this crate has the UTC half.
//!
//! POSIX time has no leap seconds, so every UTC day is exactly
//! [`MS_PER_DAY`] and only months and years are variable-length.

/// Milliseconds in one UTC day. Exact: POSIX time carries no leap seconds.
pub(crate) const MS_PER_DAY: i64 = 86_400_000;

/// [`MS_PER_DAY`] as `f64`, for the float arithmetic the tick ladder does.
/// A separate literal rather than a cast so it can be used in a `const`
/// without a lossy-cast escape hatch; the assertion below is what keeps the
/// two spellings from drifting.
pub(crate) const MS_PER_DAY_F64: f64 = 86_400_000.0;
const _: () = assert!(MS_PER_DAY == 86_400_000 && MS_PER_DAY_F64 == 86_400_000.0);

/// Milliseconds in one hour.
pub(crate) const MS_PER_HOUR: i64 = 3_600_000;

/// Milliseconds in one minute.
pub(crate) const MS_PER_MINUTE: i64 = 60_000;

/// Milliseconds in one second.
pub(crate) const MS_PER_SECOND: i64 = 1_000;

/// The largest magnitude of epoch-millisecond this module accepts,
/// ±285,616 years — ECMAScript's time-value range, and exactly the range
/// over which an `f64` holds every whole millisecond without loss. An
/// instant beyond it is clamped rather than wrapped, so a nonsense domain
/// yields a legible axis at the boundary instead of a wrong date.
pub(crate) const MAX_TIME_MS: f64 = 8.64e15;

/// Month abbreviations, `Jan` .. `Dec`.
///
/// The C-locale convention, matching the decimal separator and SI suffixes
/// [`crate::ticks`] already emits — a chart that printed `1.2k` and
/// `3월` would be reading from two locales at once. Locale-aware month
/// and day names are a crate-wide concern (they would need the same
/// treatment in every readout), recorded as a gap rather than decided here.
const MONTH_ABBREV: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The abbreviated name of `month` (`1..=12`); empty for an out-of-range
/// index, which the constructors here cannot produce.
pub(crate) fn month_abbrev(month: u32) -> &'static str {
    MONTH_ABBREV
        .get((month as usize).wrapping_sub(1))
        .copied()
        .unwrap_or("")
}

/// One instant broken into its UTC calendar fields.
///
/// Round-trips through [`Civil::to_millis`] exactly for any instant inside
/// [`MAX_TIME_MS`], which is what makes "snap to the start of this month"
/// expressible: break down, zero the finer fields, compose again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Civil {
    /// Proleptic-Gregorian year (negative before 1 CE).
    pub year: i64,
    /// Month, `1..=12`.
    pub month: u32,
    /// Day of month, `1..=31`.
    pub day: u32,
    /// Hour, `0..=23`.
    pub hour: u32,
    /// Minute, `0..=59`.
    pub minute: u32,
    /// Second, `0..=59`.
    pub second: u32,
    /// Millisecond, `0..=999`.
    pub milli: u32,
}

impl Civil {
    /// Break `ms` (epoch milliseconds, UTC) into calendar fields.
    ///
    /// Non-finite input and magnitudes beyond [`MAX_TIME_MS`] are clamped
    /// into range — this is a total function, because a tick generator that
    /// could fail mid-axis would have to invent a tick anyway.
    pub(crate) fn from_millis(ms: f64) -> Self {
        let total = clamp_millis(ms);
        // Euclidean division, not truncating: an instant before 1970 has a
        // negative millisecond count, and truncation there would round the
        // day *up* and give a negative time-of-day.
        let days = total.div_euclid(MS_PER_DAY);
        let tod = total.rem_euclid(MS_PER_DAY);
        let (year, month, day) = civil_from_days(days);
        let secs = tod / MS_PER_SECOND;
        Self {
            year,
            month,
            day,
            hour: to_u32(secs / 3600),
            minute: to_u32((secs / 60) % 60),
            second: to_u32(secs % 60),
            milli: to_u32(tod % MS_PER_SECOND),
        }
    }

    /// another declarative toolkit these fields back into epoch milliseconds
    /// (UTC).
    pub(crate) fn to_millis(self) -> f64 {
        let days = days_from_civil(self.year, i64::from(self.month), i64::from(self.day));
        let tod = i64::from(self.hour) * MS_PER_HOUR
            + i64::from(self.minute) * MS_PER_MINUTE
            + i64::from(self.second) * MS_PER_SECOND
            + i64::from(self.milli);
        to_f64(days * MS_PER_DAY + tod)
    }

    /// The instant at the start of this value's UTC day.
    pub(crate) const fn start_of_day(self) -> Self {
        Self {
            hour: 0,
            minute: 0,
            second: 0,
            milli: 0,
            ..self
        }
    }

    /// The instant at the start of this value's month.
    pub(crate) const fn start_of_month(self) -> Self {
        Self {
            day: 1,
            ..self.start_of_day()
        }
    }

    /// The instant at the start of this value's year.
    pub(crate) const fn start_of_year(self) -> Self {
        Self {
            month: 1,
            ..self.start_of_month()
        }
    }
}

/// Epoch milliseconds for the start of the UTC month `count` months after
/// the month containing `ms` (`count` may be negative).
///
/// Month arithmetic cannot be done in milliseconds — months are 28 to 31
/// days — so it is done in the `(year, month)` lattice and composed back.
pub(crate) fn add_months(ms: f64, count: i64) -> f64 {
    let c = Civil::from_millis(ms).start_of_month();
    // Shift into a zero-based absolute month index so the year carry is one
    // Euclidean division rather than a wrap-around branch.
    let index = c.year * 12 + i64::from(c.month) - 1 + count;
    Civil {
        year: index.div_euclid(12),
        month: to_u32(index.rem_euclid(12)) + 1,
        ..c
    }
    .to_millis()
}

/// Epoch milliseconds for the start of the UTC year `count` years after the
/// year containing `ms`.
pub(crate) fn add_years(ms: f64, count: i64) -> f64 {
    let c = Civil::from_millis(ms).start_of_year();
    Civil {
        year: c.year + count,
        ..c
    }
    .to_millis()
}

/// Days from 1970-01-01 to the proleptic-Gregorian civil date `(y, m, d)`.
///
/// Hinnant's `days_from_civil`. `m` is `1..=12` and `d` is `1..=31`; the
/// mapping is defined (and exactly invertible by [`civil_from_days`]) for
/// every year `i64` can hold that this module's clamp admits.
#[allow(
    clippy::similar_names,
    reason = "yoe / doe / doy are Hinnant's own names for year-of-era, day-of-era and day-of-year; renaming them would break the correspondence with the cited derivation"
)]
pub(crate) const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // March-based year: putting the leap day at the END of the year makes
    // the 400-year era arithmetic below branch-free.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// The proleptic-Gregorian civil date `(y, m, d)` `z` days after
/// 1970-01-01. Hinnant's `civil_from_days`, the exact inverse of
/// [`days_from_civil`].
#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "Hinnant's decomposition bounds the month to 1..=12 and the day to 1..=31 before these casts"
)]
#[allow(
    clippy::similar_names,
    reason = "yoe / doe / doy are Hinnant's own names; see days_from_civil"
)]
pub(crate) const fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    // Undo the March-based shift the forward direction applied.
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// `ms` clamped into the representable range and narrowed to whole
/// milliseconds, rounding toward negative infinity so that truncation never
/// moves an instant across a day boundary. Non-finite input clamps to zero
/// (`NaN`) or the range ends (infinities).
#[allow(
    clippy::cast_possible_truncation,
    reason = "clamped to MAX_TIME_MS, far inside i64, before the cast"
)]
fn clamp_millis(ms: f64) -> i64 {
    if ms.is_nan() {
        return 0;
    }
    ms.clamp(-MAX_TIME_MS, MAX_TIME_MS).floor() as i64
}

/// A bounded non-negative field narrowed to `u32`.
#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "every caller passes a calendar field already bounded to 0..1000"
)]
const fn to_u32(v: i64) -> u32 {
    v as u32
}

/// A millisecond count widened to `f64` — exact inside [`MAX_TIME_MS`].
#[allow(
    clippy::cast_precision_loss,
    reason = "the value is bounded by MAX_TIME_MS, where every whole millisecond is exactly representable"
)]
fn to_f64(v: i64) -> f64 {
    v as f64
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "every instant under test is a whole millisecond inside MAX_TIME_MS, where f64 is exact; an epsilon compare would weaken assertions whose entire point is that the round-trip is exact"
)]
mod tests {
    use super::*;

    /// Epoch milliseconds for a UTC date-time, for the tests to read at.
    fn at(y: i64, mo: u32, d: u32, h: u32, mi: u32, s: u32, ms: u32) -> f64 {
        Civil {
            year: y,
            month: mo,
            day: d,
            hour: h,
            minute: mi,
            second: s,
            milli: ms,
        }
        .to_millis()
    }

    /// ★ The algorithm against instants whose epoch value is independently
    /// known — the epoch itself, the Y2K leap day (a century year divisible
    /// by 400, so a leap year, which the naive rule gets wrong), 1900-03-01
    /// (after a century year NOT divisible by 400, so not a leap year), and
    /// a pre-epoch instant where the arithmetic goes negative.
    #[test]
    fn r1529_known_epoch_values_round_trip() {
        // Reference values: `date -u -d "..." +%s` * 1000.
        for (ms, y, mo, d) in [
            (0.0_f64, 1970_i64, 1_u32, 1_u32),
            (951_782_400_000.0, 2000, 2, 29), // Y2K leap day exists
            (-2_203_891_200_000.0, 1900, 3, 1), // 1900 was NOT a leap year
            (1_772_582_400_000.0, 2026, 3, 4),
            (-86_400_000.0, 1969, 12, 31), // the day before the epoch
        ] {
            let c = Civil::from_millis(ms);
            assert_eq!((c.year, c.month, c.day), (y, mo, d), "breakdown of {ms}");
            assert!(
                (c.to_millis() - ms).abs() < f64::EPSILON,
                "{ms} round-trips"
            );
        }
        // 1900-02-29 does not exist: Feb 28 + 1 day is March 1.
        assert_eq!(
            Civil::from_millis(-2_203_891_200_000.0 - MS_PER_DAY_F64).day,
            28
        );
    }

    /// ★ Exhaustive inverse over a span covering every leap rule: 1600
    /// (÷400, leap), 1700/1800/1900 (÷100 not ÷400, common), and every
    /// ordinary four-year cycle between. If either direction were off by a
    /// day anywhere in there, a month tick would land on the 31st.
    #[test]
    fn r1529_civil_and_days_are_exact_inverses() {
        let lo = days_from_civil(1583, 1, 1);
        let hi = days_from_civil(2400, 1, 1);
        for z in lo..hi {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, i64::from(m), i64::from(d)), z);
            assert!((1..=12).contains(&m) && (1..=31).contains(&d));
        }
    }

    #[test]
    fn r1529_time_of_day_survives_the_epoch_sign_change() {
        // The Euclidean-division claim: an instant 1ms before the epoch is
        // 1969-12-31 23:59:59.999, not 1970-01-01 -00:00:00.001.
        let c = Civil::from_millis(-1.0);
        assert_eq!(
            (c.year, c.month, c.day, c.hour, c.minute, c.second, c.milli),
            (1969, 12, 31, 23, 59, 59, 999)
        );
        let noon = Civil::from_millis(at(2026, 3, 2, 12, 34, 56, 789));
        assert_eq!(
            (noon.hour, noon.minute, noon.second, noon.milli),
            (12, 34, 56, 789)
        );
    }

    #[test]
    fn r1529_start_of_truncates_progressively() {
        let c = Civil::from_millis(at(2026, 7, 17, 14, 35, 22, 431));
        assert_eq!(c.start_of_day().to_millis(), at(2026, 7, 17, 0, 0, 0, 0));
        assert_eq!(c.start_of_month().to_millis(), at(2026, 7, 1, 0, 0, 0, 0));
        assert_eq!(c.start_of_year().to_millis(), at(2026, 1, 1, 0, 0, 0, 0));
    }

    /// ★ Month arithmetic is what milliseconds cannot express. Adding "one
    /// month" as a fixed 30-day duration drifts by up to 3 days a step and
    /// leaves the year boundary entirely.
    #[test]
    fn r1529_add_months_carries_the_year_and_keeps_the_boundary() {
        let nov = at(2026, 11, 20, 9, 0, 0, 0);
        assert_eq!(add_months(nov, 1), at(2026, 12, 1, 0, 0, 0, 0));
        assert_eq!(add_months(nov, 2), at(2027, 1, 1, 0, 0, 0, 0));
        assert_eq!(add_months(nov, 14), at(2028, 1, 1, 0, 0, 0, 0));
        assert_eq!(add_months(nov, -11), at(2025, 12, 1, 0, 0, 0, 0));
        // ★ The counterfactual: twelve fixed 30-day steps from January land
        // in December, not in the next January.
        let jan = at(2026, 1, 1, 0, 0, 0, 0);
        let fixed = jan + 12.0 * 30.0 * MS_PER_DAY_F64;
        assert_eq!(Civil::from_millis(fixed).month, 12, "30-day months drift");
        assert_eq!(add_months(jan, 12), at(2027, 1, 1, 0, 0, 0, 0));
        // February keeps its own length in both directions.
        assert_eq!(
            add_months(at(2028, 2, 29, 0, 0, 0, 0), 1),
            at(2028, 3, 1, 0, 0, 0, 0)
        );
    }

    #[test]
    fn r1529_add_years_lands_on_january_first() {
        let mid = at(2026, 7, 4, 18, 0, 0, 0);
        assert_eq!(add_years(mid, 0), at(2026, 1, 1, 0, 0, 0, 0));
        assert_eq!(add_years(mid, 4), at(2030, 1, 1, 0, 0, 0, 0));
        assert_eq!(add_years(mid, -26), at(2000, 1, 1, 0, 0, 0, 0));
    }

    #[test]
    fn r1529_out_of_range_instants_clamp_rather_than_wrap() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300, -1e300] {
            let c = Civil::from_millis(bad);
            assert!((1..=12).contains(&c.month), "{bad} yields a real month");
            assert!((1..=31).contains(&c.day), "{bad} yields a real day");
            assert!(c.hour < 24 && c.minute < 60 && c.second < 60 && c.milli < 1000);
        }
        assert_eq!(
            Civil::from_millis(f64::NAN).year,
            1970,
            "NaN clamps to the epoch"
        );
        assert!(Civil::from_millis(f64::INFINITY).year > 200_000);
        assert!(Civil::from_millis(f64::NEG_INFINITY).year < -200_000);
    }

    #[test]
    fn r1529_month_abbreviations_cover_the_year() {
        assert_eq!(month_abbrev(1), "Jan");
        assert_eq!(month_abbrev(7), "Jul");
        assert_eq!(month_abbrev(12), "Dec");
        assert_eq!(month_abbrev(0), "", "out of range is empty, not a panic");
        assert_eq!(month_abbrev(13), "");
    }
}
