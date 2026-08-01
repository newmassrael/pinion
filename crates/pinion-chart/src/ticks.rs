//! Nice-number axis tick generation and label formatting.
//!
//! [`nice_ticks`] snaps the tick spacing to a human-friendly
//! `1 / 2 / 5 x 10^n` step — the "nice numbers" quantiser Heckbert
//! popularised (Graphics Gems, 1990) — but derives that step from the
//! **raw** data extent, the way Qt's `QValueAxis::applyNiceNumbers` and
//! d3's `ticks` do, rather than from Heckbert's *pre-rounded* range.
//! Pre-rounding inflates the step (extent 11 rounds to 20, giving a step
//! of 5 and stretching the axis to `0..15`); the raw extent keeps it
//! tight (`0..12`). This is what separates a professional axis
//! (`0, 1k, 2k, 3k`) from a naive one (`0, 923, 1846, ...`).
//!
//! [`format_axis_tick`] is the **axis label entry point**: it picks the
//! decimals the step implies ([`tick_decimals`] + [`format_tick`]) and
//! falls back to a compact SI suffix ([`format_si`] — `1.2k`, `3M`) for
//! the large magnitudes a monitoring dashboard favours. Use it rather
//! than `format_si` alone: SI carries at most one decimal, so a 0.05 step
//! would render `0.05 / 0.10 / 0.15` as three identical `0.1` labels.

/// Nice-number tick values covering `[lo, hi]`, aiming for `target`
/// ticks (clamped to a minimum of 2). The returned values use a
/// `1 / 2 / 5 x 10^n` spacing and extend to the nice multiples just
/// outside `[lo, hi]`, so the count is *approximately* `target`, not
/// exact — the trade every nice-tick algorithm makes for round labels.
///
/// Returns an empty vector when either endpoint is non-finite, and a
/// single `[lo]` when the range collapses to a point.
#[must_use]
pub fn nice_ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
    if !lo.is_finite() || !hi.is_finite() {
        return Vec::new();
    }
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let target = target.max(2);
    if (hi - lo).abs() < f64::EPSILON {
        return vec![lo];
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "target is a small tick count; the f64 conversion is exact for any realistic value"
    )]
    let denom = (target - 1) as f64;
    // Step from the RAW extent (Qt `applyNiceNumbers` / d3), not from a
    // pre-rounded range: pre-rounding 11 -> 20 would inflate the step and
    // stretch the axis to 0..15; the raw extent keeps it tight at 0..12.
    let spacing = nice_step((hi - lo) / denom);
    if !spacing.is_finite() || spacing <= 0.0 {
        return vec![lo, hi];
    }
    let graph_lo = (lo / spacing).floor() * spacing;
    let graph_hi = (hi / spacing).ceil() * spacing;
    let mut ticks = Vec::new();
    let mut value = graph_lo;
    // Guard bounds the loop even if float drift stalls the increment.
    let mut guard = 0;
    while value <= graph_hi + spacing * 0.5 && guard < 1000 {
        ticks.push(round_dust(value, spacing));
        value += spacing;
        guard += 1;
    }
    ticks
}

/// Snap a positive magnitude to the NEAREST nice `1 / 2 / 5 / 10 x 10^n`
/// number — the step quantiser. Non-positive / non-finite input yields 0.
///
/// Heckbert's original carries a second, round-*up* mode used only to
/// pre-round the overall range; this implementation derives the step from
/// the raw extent (see the module doc), so that mode had no caller and was
/// removed rather than left as an unreachable branch.
fn nice_step(x: f64) -> f64 {
    if x <= 0.0 || !x.is_finite() {
        return 0.0;
    }
    let exp = x.log10().floor();
    let base = 10f64.powf(exp);
    let fraction = x / base;
    let nice_fraction = if fraction < 1.5 {
        1.0
    } else if fraction < 3.0 {
        2.0
    } else if fraction < 7.0 {
        5.0
    } else {
        10.0
    };
    nice_fraction * base
}

/// Round a tick value to the decimal precision implied by `spacing`,
/// erasing binary-float dust (`0.30000000000000004` becomes `0.3`).
fn round_dust(value: f64, spacing: f64) -> f64 {
    let decimals = tick_decimals(spacing);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "decimals is bounded to 0..=10 by tick_decimals; i32 exponent is exact"
    )]
    let factor = 10f64.powi(decimals as i32);
    (value * factor).round() / factor
}

/// The gap between consecutive ticks, or `0` for a degenerate axis (one or
/// no tick) — `tick_decimals(0.0)` is `0`, i.e. whole-number labels. The step
/// each axis formats at, shared by the line and bar builders so their labels
/// carry the same precision.
pub(crate) fn tick_step(ticks: &[f64]) -> f64 {
    match ticks {
        [a, b, ..] => (b - a).abs(),
        _ => 0.0,
    }
}

/// Decimal places a label needs to render a tick at the given `step`
/// without a spurious trailing digit — `0` for `step >= 1`, else the
/// magnitude of the fractional step. Bounded to `0..=10`.
#[must_use]
pub fn tick_decimals(step: f64) -> usize {
    if !step.is_finite() || step <= 0.0 {
        return 0;
    }
    let decimals = -step.log10().floor();
    if decimals <= 0.0 {
        0
    } else {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "decimals is positive and finite here; capped at 10"
        )]
        let d = decimals as usize;
        d.min(10)
    }
}

/// Render `value` with a fixed `decimals` count, normalising `-0` to `0`.
#[must_use]
pub fn format_tick(value: f64, decimals: usize) -> String {
    let value = if value.abs() < f64::EPSILON {
        0.0
    } else {
        value
    };
    format!("{value:.decimals$}")
}

/// Render `value` as an axis label for an axis whose tick step is `step`.
///
/// This is the formatter a chart axis (and any value readout tied to one)
/// should use. Below 1000 the label carries exactly the decimals `step`
/// implies, so a `0.05` step renders `0.00 / 0.05 / 0.10` — distinct, and
/// consistent in width. At or above 1000 it switches to the compact SI
/// form (`1.2k`, `3M`) dense dashboards favour, where the step is by
/// construction >= 1 and the dropped decimals carry no information.
///
/// Prefer this over bare [`format_si`], whose one-decimal cap collapses
/// every sub-0.1 step into the same rounded digit.
#[must_use]
pub fn format_axis_tick(value: f64, step: f64) -> String {
    format_at_decimals(value, tick_decimals(step))
}

/// Render `value` at a decimals count the caller has already chosen, switching
/// to the compact SI form at or above 1000 exactly as [`format_axis_tick`] does.
///
/// The shared body of "format a chart label": [`format_axis_tick`] derives the
/// decimals from an axis STEP, the colour bar from [`value_decimals`] over the
/// values themselves. Only that derivation differs, so the SI cutover lives here
/// once rather than being re-decided per caller.
pub(crate) fn format_at_decimals(value: f64, decimals: usize) -> String {
    if value.abs() >= 1000.0 {
        format_si(value)
    } else {
        format_tick(value, decimals)
    }
}

/// Decimal places needed to print each of `values` FAITHFULLY, capped at 3.
///
/// [`tick_decimals`] answers a different question: given an axis whose ticks are
/// multiples of `step`, how many decimals does a label need? That is right for a
/// nice-numbered axis and wrong for a set of arbitrary values, because it infers
/// the precision from the GAP rather than from the numbers. A colour bar's ticks
/// are the domain's real endpoints and its neutral — a domain of `-3.2 ..= 9.6`
/// has a 3.2 gap, from which `tick_decimals` concludes "whole numbers" and the
/// low end prints as `-3`, mislabelling the ramp's own end by 0.2 (R1439 found
/// this the moment a domain stopped landing on integers).
///
/// Capped at 3 because a legend label is a readout, not a data export; a domain
/// derived from real data rarely terminates, and four decimals of a change rate
/// would cost more width than they inform.
pub(crate) fn value_decimals(values: &[f64]) -> usize {
    const MAX: usize = 3;
    (0..MAX)
        .find(|&d| {
            values.iter().copied().filter(|v| v.is_finite()).all(|v| {
                let rounded = round_to(v, d);
                (v - rounded).abs() <= 1e-9 * v.abs().max(1.0)
            })
        })
        .unwrap_or(MAX)
}

/// `value` rounded to `decimals` places.
fn round_to(value: f64, decimals: usize) -> f64 {
    let factor = 10_f64.powi(i32::try_from(decimals).unwrap_or(0));
    (value * factor).round() / factor
}

// ---- R1528: the logarithmic axis ---------------------------------------

/// Decades beyond which the minor (`2..base`) subdivisions stop being drawn.
///
/// Minor gridlines are what make a log axis *readable* — without them the
/// decade lines are evenly spaced and the picture is indistinguishable from
/// a linear axis with odd labels. But nine lines per decade stop informing
/// once the axis spans more decades than a reader can resolve; d3 makes the
/// same call by subdividing only while the decade count is below the tick
/// target.
const MINOR_TICK_DECADE_LIMIT: i32 = 6;

/// Below this magnitude a log tick label switches to exponent form
/// (`1e-4`), the mirror of the SI cutover [`format_at_decimals`] makes at
/// 1000. A log axis routinely reaches magnitudes a fixed-decimal label
/// cannot print in an axis gutter.
const EXPONENT_CUTOVER: f64 = 1e-3;

/// The exponent of `value` in `base`, with float dust rounded off.
///
/// `ln(1000) / ln(10)` is `2.9999999999999996`, so a raw `floor` would put
/// 1000 in the `10^2` decade and start the axis a whole decade early. This
/// is the exponent-domain twin of [`round_dust`].
fn log_exponent(value: f64, base: f64) -> f64 {
    let e = crate::scale::log_base(value, base);
    let rounded = e.round();
    if (e - rounded).abs() < 1e-9 {
        rounded
    } else {
        e
    }
}

/// Snap a positive data extent outward to whole powers of `base` — the log
/// axis's [`nice_ticks`] equivalent, so the outer gridlines land on the plot
/// edges (Qt's `applyNiceNumbers`, in the log domain).
///
/// A collapsed or out-of-contract extent yields the unit decade
/// `(1, base)`, matching [`LogScale`](crate::LogScale)'s own fallback so the
/// domain a chart reports and the domain it maps through cannot disagree.
#[must_use]
pub fn nice_log_domain(lo: f64, hi: f64, base: f64) -> (f64, f64) {
    let base = if base.is_finite() && base > 1.0 {
        base
    } else {
        10.0
    };
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    if !crate::scale::positive(lo) || !crate::scale::positive(hi) {
        return (1.0, base);
    }
    let e_lo = log_exponent(lo, base).floor();
    let e_hi = log_exponent(hi, base).ceil();
    // A value that IS a power of the base collapses floor and ceil onto the
    // same exponent; widen by one decade so the domain is never degenerate.
    let e_hi = if e_hi <= e_lo { e_lo + 1.0 } else { e_hi };
    (base.powf(e_lo), base.powf(e_hi))
}

/// The **labelled** ticks of a log axis covering `[lo, hi]`: whole powers of
/// `base`, thinned by a whole-decade stride when the span carries more
/// decades than `target` labels.
///
/// Like [`nice_ticks`] this brackets `[lo, hi]` rather than clipping to it —
/// `crate::plot::axis_ticks` does the clipping, so both axis
/// kinds reach the chart through one filter.
///
/// Returns empty for a non-positive or non-finite extent (a log axis has no
/// ticks there), and a single `[lo]` for a collapsed one.
#[must_use]
pub fn log_ticks(lo: f64, hi: f64, base: f64, target: usize) -> Vec<f64> {
    let base = if base.is_finite() && base > 1.0 {
        base
    } else {
        10.0
    };
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    if !crate::scale::positive(lo) || !crate::scale::positive(hi) {
        return Vec::new();
    }
    if (hi - lo).abs() < f64::EPSILON {
        return vec![lo];
    }
    let (e_lo, e_hi) = exponent_span(lo, hi, base);
    let decades = e_hi - e_lo;
    // One label per decade, or per n-th decade once the decades outnumber
    // the labels the gutter can carry.
    let target = i32::try_from(target.max(2)).unwrap_or(i32::MAX);
    let stride = ((decades + target - 1) / target).max(1);
    let mut ticks = Vec::new();
    let mut e = e_lo;
    while e <= e_hi {
        ticks.push(base.powi(e));
        e += stride;
    }
    ticks
}

/// The **unlabelled** minor subdivisions of a log axis: `k * base^d` for
/// `k` in `2..base`, one set per whole decade in `[lo, hi]`.
///
/// Empty when `base` is not a whole number (the subdivisions would not be
/// round), when it exceeds 36 (a decade would carry more lines than pixels),
/// or when the span exceeds `MINOR_TICK_DECADE_LIMIT` decades. Callers draw
/// these as gridlines only — see [`TickFormat`], which has no minor arm
/// because a minor tick is never labelled.
#[must_use]
pub fn log_minor_ticks(lo: f64, hi: f64, base: f64) -> Vec<f64> {
    if !base.is_finite() || !(2.0..=36.0).contains(&base) || base.fract().abs() > f64::EPSILON {
        return Vec::new();
    }
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    if !crate::scale::positive(lo) || !crate::scale::positive(hi) {
        return Vec::new();
    }
    let (e_lo, e_hi) = exponent_span(lo, hi, base);
    if e_hi - e_lo > MINOR_TICK_DECADE_LIMIT {
        return Vec::new();
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "base is checked whole and <= 36 above, so the truncation is exact"
    )]
    let steps = base as i64;
    let mut out = Vec::new();
    for e in e_lo..e_hi {
        let decade = base.powi(e);
        for k in 2..steps {
            #[allow(
                clippy::cast_precision_loss,
                reason = "k is bounded by base <= 36; the f64 conversion is exact"
            )]
            let v = (k as f64) * decade;
            if v >= lo && v <= hi {
                out.push(v);
            }
        }
    }
    out
}

/// The bracketing whole-exponent span of `[lo, hi]` in `base`.
fn exponent_span(lo: f64, hi: f64, base: f64) -> (i32, i32) {
    let clamp = |e: f64| {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the exponent is clamped into i32 range before the cast"
        )]
        let v = e.clamp(-300.0, 300.0) as i32;
        v
    };
    let e_lo = clamp(log_exponent(lo, base).floor());
    let e_hi = clamp(log_exponent(hi, base).ceil());
    (e_lo, e_hi.max(e_lo))
}

/// How an axis derives its tick labels' precision.
///
/// A linear axis has ONE gap, so a single `step` determines every label's
/// decimals — which is why this was a bare `f64` until R1528. A log axis has
/// no constant gap: consecutive ticks differ by a *ratio*, so a scalar step
/// is not a lossy summary of that tick set, it is a wrong one. The scalar
/// survived because for a linear axis the two agree — the same
/// derivation-by-coincidence R1439 found between an axis's step and its
/// values.
///
/// Minor ticks have no arm here because a minor tick is never labelled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TickFormat {
    /// Linear axis — every label carries the decimals this constant step
    /// implies ([`format_axis_tick`]).
    Step(f64),
    /// Logarithmic axis — every label is derived from its own magnitude
    /// ([`format_log_tick`]).
    Log,
}

impl TickFormat {
    /// The label for one tick `value` on this axis.
    #[must_use]
    pub fn label(self, value: f64) -> String {
        match self {
            Self::Step(step) => format_axis_tick(value, step),
            Self::Log => format_log_tick(value),
        }
    }
}

/// Render `value` as a label on a **logarithmic** axis.
///
/// Precision comes from the value itself (`value_decimals`) rather than
/// from a step, for the reason [`TickFormat`] states. The compact forms sit
/// at both ends of the readable middle: SI at or above 1000 (`1k`, `1M` —
/// the same cutover a linear axis makes, so one reader sees one convention
/// across a dashboard), and exponent form below `EXPONENT_CUTOVER`
/// (`1e-4`), where a fixed-decimal label would be too wide for an axis
/// gutter and, past `value_decimals`'s cap, would print as `0.000`.
#[must_use]
pub fn format_log_tick(value: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    let abs = value.abs();
    if abs > 0.0 && abs < EXPONENT_CUTOVER {
        return format_exponent(value);
    }
    format_at_decimals(value, value_decimals(&[value]))
}

/// `value` in exponent form — `1e-4`, `2.5e-6`. The mantissa carries at most
/// one decimal, which is exact for every tick this crate generates (whole
/// powers of the base, and small whole multiples of them).
fn format_exponent(value: f64) -> String {
    let exp = log_exponent(value.abs(), 10.0).floor();
    let mantissa = value / 10f64.powf(exp);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the exponent is bounded by f64's own range, far inside i32"
    )]
    let e = exp as i32;
    if (mantissa - mantissa.round()).abs() < 1e-9 {
        format!("{:.0}e{e}", mantissa.round())
    } else {
        format!("{mantissa:.1}e{e}")
    }
}

/// Render `value` with an SI magnitude suffix (`k` / `M` / `G`) and at
/// most one decimal — the compact readout dense dashboards use for
/// throughput / counts. Values below 1000 render as-is.
///
/// Lossy by design: `format_si(0.05) == "0.1"`. For an axis label use
/// [`format_axis_tick`], which only reaches for SI where the step makes
/// the rounding lossless.
#[must_use]
pub fn format_si(value: f64) -> String {
    let abs = value.abs();
    let (scaled, suffix) = if abs >= 1e9 {
        (value / 1e9, "G")
    } else if abs >= 1e6 {
        (value / 1e6, "M")
    } else if abs >= 1e3 {
        (value / 1e3, "k")
    } else {
        (value, "")
    };
    if scaled.fract().abs() < f64::EPSILON {
        format!("{scaled:.0}{suffix}")
    } else {
        format!("{scaled:.1}{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_to_thirtysix_hundred_gives_round_thousands() {
        let ticks = nice_ticks(0.0, 3600.0, 5);
        assert_eq!(ticks, vec![0.0, 1000.0, 2000.0, 3000.0, 4000.0]);
    }

    #[test]
    fn spacing_is_a_one_two_five_multiple() {
        let ticks = nice_ticks(0.0, 47.0, 6);
        // 47 / 5 -> nice 10 spacing: 0,10,20,30,40,50.
        assert_eq!(ticks, vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0]);
    }

    #[test]
    fn fractional_range_stays_dust_free() {
        // target 5 -> nice spacing 0.2 (a 1/2/5 x 10^n step), six ticks.
        // The exact assert_eq is itself the dust check: an un-rounded
        // accumulation would land on 0.30000000000000004 and mismatch.
        let ticks = nice_ticks(0.0, 1.0, 5);
        assert_eq!(ticks, vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
    }

    #[test]
    fn ticks_bracket_the_data_range() {
        let ticks = nice_ticks(3.0, 17.0, 4);
        assert!(ticks.first().copied().unwrap() <= 3.0);
        assert!(ticks.last().copied().unwrap() >= 17.0);
    }

    #[test]
    fn reversed_range_is_normalised() {
        assert_eq!(nice_ticks(3600.0, 0.0, 5), nice_ticks(0.0, 3600.0, 5));
    }

    #[test]
    fn collapsed_range_returns_single_tick() {
        assert_eq!(nice_ticks(5.0, 5.0, 5), vec![5.0]);
    }

    #[test]
    fn non_finite_returns_empty() {
        assert!(nice_ticks(f64::NAN, 1.0, 5).is_empty());
        assert!(nice_ticks(0.0, f64::INFINITY, 5).is_empty());
    }

    #[test]
    fn decimals_track_step_magnitude() {
        // Only 1/2/5 x 10^n steps are ever produced by nice_num; those
        // render cleanly at -floor(log10(step)) decimals.
        assert_eq!(tick_decimals(1000.0), 0);
        assert_eq!(tick_decimals(1.0), 0);
        assert_eq!(tick_decimals(0.5), 1);
        assert_eq!(tick_decimals(0.2), 1);
        assert_eq!(tick_decimals(0.1), 1);
        assert_eq!(tick_decimals(0.05), 2);
        assert_eq!(tick_decimals(0.0), 0);
    }

    #[test]
    fn format_tick_normalises_negative_zero() {
        assert_eq!(format_tick(-0.0, 0), "0");
        assert_eq!(format_tick(1234.0, 0), "1234");
        assert_eq!(format_tick(0.25, 2), "0.25");
    }

    #[test]
    fn axis_labels_stay_distinct_below_a_tenth() {
        // The R1354 defect: `format_si` was wired to both axes, so every
        // sub-0.1 step collapsed to one rounded digit. y in [0, 0.2] with
        // 5 target ticks -> step 0.05 -> five gridlines that rendered as
        // `0, 0.1, 0.1, 0.1, 0.2`. Labels must be distinct and truthful.
        let ticks = nice_ticks(0.0, 0.2, 5);
        let step = ticks[1] - ticks[0];
        let labels: Vec<String> = ticks.iter().map(|t| format_axis_tick(*t, step)).collect();
        assert_eq!(labels, ["0.00", "0.05", "0.10", "0.15", "0.20"]);
        let distinct: std::collections::BTreeSet<&String> = labels.iter().collect();
        assert_eq!(distinct.len(), labels.len(), "one label per gridline");
    }

    #[test]
    fn axis_labels_keep_si_for_large_magnitudes() {
        // The compact form must survive: 0..3600 step 1000 stays 0/1k/../4k.
        let ticks = nice_ticks(0.0, 3600.0, 5);
        let step = ticks[1] - ticks[0];
        let labels: Vec<String> = ticks.iter().map(|t| format_axis_tick(*t, step)).collect();
        assert_eq!(labels, ["0", "1k", "2k", "3k", "4k"]);
    }

    #[test]
    fn axis_label_never_rounds_a_value_away() {
        // format_si is lossy by contract; format_axis_tick must not be.
        assert_eq!(format_si(0.05), "0.1", "SI is lossy (documented)");
        assert_eq!(format_axis_tick(0.05, 0.05), "0.05", "axis label is not");
        assert_eq!(format_axis_tick(5.3, 0.5), "5.3");
    }

    #[test]
    fn si_suffixes() {
        assert_eq!(format_si(500.0), "500");
        assert_eq!(format_si(1500.0), "1.5k");
        assert_eq!(format_si(2000.0), "2k");
        assert_eq!(format_si(3_400_000.0), "3.4M");
        assert_eq!(format_si(2_000_000_000.0), "2G");
    }

    /// ★ R1439 — precision from the VALUES, not from the gap between them.
    ///
    /// The colour bar prints a domain's real endpoints, which are not multiples
    /// of any step. Inferring decimals from the gap (what [`tick_decimals`]
    /// does, correctly, for a nice-numbered axis) silently truncates them.
    #[test]
    fn r1439_value_decimals_keeps_every_value_faithful() {
        assert_eq!(
            value_decimals(&[-3.0, 0.0, 9.0]),
            0,
            "whole numbers need none"
        );
        assert_eq!(value_decimals(&[-3.2, 0.0, 9.6]), 1, "one tenth is enough");
        assert_eq!(value_decimals(&[0.0, 0.25]), 2);
        assert_eq!(value_decimals(&[0.0, 0.125]), 3);
        assert_eq!(
            value_decimals(&[0.0, 1.0 / 3.0]),
            3,
            "a non-terminating value caps at the display bound"
        );
        assert_eq!(
            value_decimals(&[f64::NAN, 2.5]),
            1,
            "a non-finite value is skipped, not propagated"
        );

        // ★ The counterfactual: the step-derived route gets this domain wrong,
        // which is exactly the label the demo caught printing as "-3".
        let ends = [-3.2_f64, 0.0, 9.6];
        assert_eq!(
            format_axis_tick(ends[0], tick_step(&ends)),
            "-3",
            "the axis formatter truncates an off-step endpoint"
        );
        assert_eq!(
            format_at_decimals(ends[0], value_decimals(&ends)),
            "-3.2",
            "the value formatter does not"
        );
    }

    // ---- R1528: the logarithmic axis -----------------------------------

    #[test]
    fn r1528_log_domain_snaps_outward_to_whole_decades() {
        assert_eq!(nice_log_domain(0.4, 730.0, 10.0), (0.1, 1000.0));
        assert_eq!(nice_log_domain(2.0, 5.0, 10.0), (1.0, 10.0));
        assert_eq!(nice_log_domain(730.0, 0.4, 10.0), (0.1, 1000.0), "reversed");
        // ★ The dust guard. `ln(1000)/ln(10)` is 2.9999999999999996, so a raw
        // floor would put a domain starting at 1000 into the 10^2 decade and
        // open the axis a whole decade below the data.
        assert_eq!(
            nice_log_domain(1000.0, 100_000.0, 10.0),
            (1000.0, 100_000.0)
        );
        // A power-of-base point extent still yields a usable decade.
        assert_eq!(nice_log_domain(100.0, 100.0, 10.0), (100.0, 1000.0));
        // Out of contract -> the same unit decade LogScale falls back to.
        assert_eq!(nice_log_domain(0.0, 10.0, 10.0), (1.0, 10.0));
        assert_eq!(nice_log_domain(-5.0, -1.0, 10.0), (1.0, 10.0));
        assert_eq!(nice_log_domain(1.0, 64.0, 2.0), (1.0, 64.0), "base 2");
    }

    #[test]
    fn r1528_log_ticks_are_the_decades() {
        assert_eq!(
            log_ticks(0.1, 1000.0, 10.0, 6),
            vec![0.1, 1.0, 10.0, 100.0, 1000.0]
        );
        assert_eq!(
            log_ticks(1.0, 64.0, 2.0, 8),
            vec![1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0]
        );
        // More decades than labels -> a whole-decade stride, never a
        // fractional one (a tick at 10^1.5 is not a round number).
        let wide = log_ticks(1e-6, 1e6, 10.0, 5);
        assert_eq!(wide, vec![1e-6, 1e-3, 1.0, 1e3, 1e6]);
        // A log axis has no ticks where the logarithm is undefined.
        assert!(log_ticks(0.0, 100.0, 10.0, 5).is_empty());
        assert!(log_ticks(-10.0, -1.0, 10.0, 5).is_empty());
        assert!(log_ticks(f64::NAN, 1.0, 10.0, 5).is_empty());
        assert_eq!(log_ticks(5.0, 5.0, 10.0, 5), vec![5.0]);
    }

    #[test]
    fn r1528_minor_ticks_subdivide_each_decade() {
        let minors = log_minor_ticks(1.0, 100.0, 10.0);
        assert_eq!(
            minors,
            vec![
                2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0,
                90.0
            ]
        );
        // None of them is a labelled tick — the two sets are disjoint.
        let majors = log_ticks(1.0, 100.0, 10.0, 5);
        assert!(
            minors.iter().all(|m| !majors.contains(m)),
            "a minor tick is never a major one"
        );
        // Past the legibility limit the subdivisions stop rather than
        // crowding the plot with hundreds of lines.
        assert!(log_minor_ticks(1e-4, 1e4, 10.0).is_empty(), "8 decades");
        assert!(!log_minor_ticks(1e-2, 1e4, 10.0).is_empty(), "6 decades");
        // A fractional base has no round subdivisions.
        assert!(log_minor_ticks(1.0, 100.0, 2.5).is_empty());
        assert!(log_minor_ticks(0.0, 100.0, 10.0).is_empty());
    }

    /// ★ R1528 — the scalar step is a LINEAR-only summary of a tick set.
    ///
    /// The linear arm must reproduce the pre-R1528 labels byte for byte
    /// (that is what makes the change a generalisation rather than a
    /// rewrite), and the log arm must be right where the scalar is wrong.
    #[test]
    fn r1528_tick_format_generalises_without_moving_the_linear_labels() {
        for (lo, hi, target) in [
            (0.0, 0.2, 5),
            (0.0, 3600.0, 5),
            (3.0, 17.0, 4),
            (0.0, 1.0, 5),
        ] {
            let ticks = nice_ticks(lo, hi, target);
            let step = tick_step(&ticks);
            let old: Vec<String> = ticks.iter().map(|&t| format_axis_tick(t, step)).collect();
            let new: Vec<String> = ticks
                .iter()
                .map(|&t| TickFormat::Step(step).label(t))
                .collect();
            assert_eq!(old, new, "linear labels unchanged for {lo}..{hi}");
        }

        // ★ The counterfactual, in the direction the measurement showed
        // rather than the one guessed: a log axis's gaps GROW, so the scalar
        // is the smallest of them, and stamping it on the whole axis
        // over-specifies every tick above the first.
        let log = log_ticks(0.001, 1000.0, 10.0, 8);
        let via_step: Vec<String> = log
            .iter()
            .map(|&t| TickFormat::Step(tick_step(&log)).label(t))
            .collect();
        assert_eq!(
            via_step,
            [
                "0.001", "0.010", "0.100", "1.000", "10.000", "100.000", "1k"
            ],
            "one gap's precision stamped on every tick"
        );
        let via_log: Vec<String> = log.iter().map(|&t| TickFormat::Log.label(t)).collect();
        assert_eq!(via_log, ["0.001", "0.01", "0.1", "1", "10", "100", "1k"]);

        // ★★ And once the decades are strided, that same smallest gap is
        // itself larger than the low ticks, so they round to zero — on an
        // axis where zero is the one value that cannot exist.
        let wide = log_ticks(1e-6, 1e6, 10.0, 5);
        let wide_via_step: Vec<String> = wide
            .iter()
            .map(|&t| TickFormat::Step(tick_step(&wide)).label(t))
            .collect();
        assert_eq!(wide_via_step, ["0.0000", "0.0010", "1.0000", "1k", "1M"]);
        // The compact forms sit at both ends of the readable middle, which is
        // why `1e-3` prints as `0.001` (it is ON the cutover, not below it)
        // while `1e-6` and `1e6` take their compact forms.
        let wide_via_log: Vec<String> = wide.iter().map(|&t| TickFormat::Log.label(t)).collect();
        assert_eq!(wide_via_log, ["1e-6", "0.001", "1", "1k", "1M"]);
    }

    #[test]
    fn r1528_log_labels_stay_compact_at_both_extremes() {
        // SI above 1000 — the same convention the linear axis uses, so one
        // dashboard does not mix `1k` and `1e3`.
        assert_eq!(format_log_tick(1000.0), "1k");
        assert_eq!(format_log_tick(1e6), "1M");
        // Exponent form below the cutover, where a fixed-decimal label would
        // be too wide for a gutter and would hit value_decimals' cap.
        assert_eq!(format_log_tick(1e-4), "1e-4");
        assert_eq!(format_log_tick(1e-7), "1e-7");
        assert_eq!(format_log_tick(2e-5), "2e-5");
        // The readable middle prints plainly, and faithfully.
        assert_eq!(format_log_tick(0.001), "0.001");
        assert_eq!(format_log_tick(0.25), "0.25");
        assert_eq!(format_log_tick(1.0), "1");
        assert_eq!(format_log_tick(64.0), "64");
        // ★ Exactly what the value_decimals cap would have done at 1e-4.
        assert_eq!(
            format_at_decimals(1e-4, value_decimals(&[1e-4])),
            "0.000",
            "the plain route loses the tick entirely"
        );
        assert_eq!(format_log_tick(f64::NAN), "");
    }
}
