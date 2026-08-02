//! Continuous value → colour maps: the sequential / diverging peer of
//! [`CategoricalPalette`](crate::CategoricalPalette).
//!
//! Where a categorical palette answers "which SERIES is this", a
//! [`ColorScale`] answers "how BIG is this" — the encoding a heatmap, a
//! severity grid, a utilisation matrix or a correlation plot needs. The two are
//! not interchangeable: reaching for categorical hues to encode magnitude is
//! the classic dataviz error (a rainbow ramp invents boundaries that the data
//! does not have), and ranking a categorical dimension along one hue implies an
//! order that does not exist.
//!
//! **Two kinds, because magnitude has two shapes.**
//!
//! * A **sequential** scale ranks low → high along ONE perceptual direction.
//! * A **diverging** scale ranks away from a meaningful MIDPOINT in two
//!   directions (a delta against a baseline, a correlation around zero, an
//!   error above / below target). Its defining property is that the midpoint
//!   lands on the neutral colour EXACTLY, even when the two sides of the domain
//!   are not the same width — [`ColorScale::map_diverging`] normalises each
//!   side independently for that reason. A diverging scale fed through the
//!   linear [`map`](ColorScale::map) would put its neutral wherever the data
//!   happened to centre, which silently mis-states which values are "normal".
//!
//! **Interpolation is linear-light.** Stops are blended with
//! [`Color::lerp`], which decodes through the exact sRGB EOTF, so a ramp does
//! not darken through its middle the way naive 8-bit interpolation does.
//!
//! **Theme-independent, like the categorical palette.** A ramp is DATA ink, not
//! chrome: it must stay stable when the UI theme flips, or the same value would
//! read as a different magnitude in light and dark mode. Consumers that want
//! theme-derived endpoints build a scale FROM theme colours themselves (the
//! crate stays free of a theme dependency — the R935 decoupling).
//!
//! The module also re-exports the contrast trio a value-labelled heatmap needs:
//! [`relative_luminance`], [`contrast_ratio`] and [`readable_ink`]. Painting a
//! number on top of a ramp is not optional decoration — an unreadable cell is
//! an unreadable chart — and the legible ink must be COMPUTED per cell, because
//! no fixed "dark ink below the halfway step" rule survives a ramp whose
//! endpoints are themselves theme-derived. R1546 moved the three to
//! [`pinion_core::contrast`] once a text run's own background gave them a
//! consumer outside this crate; the ramp-integration test for them stays here,
//! because viridis is what proves the choice cannot be a fixed threshold.

use pinion_core::style::Color;

/// Neutral fallback for a scale built with too few stops to interpolate —
/// never invisible on either theme. Mirrors the categorical palette's
/// fallback rather than panicking on a degenerate construction.
const FALLBACK: Color = Color::rgb(0x80, 0x80, 0x80);

/// The ten-colour sampling of matplotlib's **viridis**, the de-facto
/// perceptually-uniform, colour-blind-safe sequential map: monotonically
/// increasing in lightness, so magnitude survives greyscale printing and every
/// common form of colour-vision deficiency.
///
/// These are anchor stops interpolated piecewise — a faithful approximation of
/// the continuous map, not a bit-exact reproduction of its 256-entry table.
const VIRIDIS: [Color; 10] = [
    Color::rgb(0x44, 0x01, 0x54),
    Color::rgb(0x48, 0x28, 0x78),
    Color::rgb(0x3E, 0x4A, 0x89),
    Color::rgb(0x31, 0x68, 0x8E),
    Color::rgb(0x26, 0x82, 0x8E),
    Color::rgb(0x1F, 0x9E, 0x89),
    Color::rgb(0x35, 0xB7, 0x79),
    Color::rgb(0x6D, 0xCD, 0x59),
    Color::rgb(0xB4, 0xDE, 0x2C),
    Color::rgb(0xFD, 0xE7, 0x25),
];

/// The diverging default's endpoints, taken from the Okabe-Ito hues the
/// [`CategoricalPalette`](crate::CategoricalPalette) already ships (blue and
/// vermillion) so the two colour systems in this crate agree, and the ramp is
/// colour-blind-safe by construction rather than by eye. The midpoint is a
/// near-neutral warm grey: light enough to read as "no deviation" against
/// either saturated end, and not white, so a mid cell is still visibly a cell.
const DIVERGING_LOW: Color = Color::rgb(0x00, 0x72, 0xB2);
const DIVERGING_MID: Color = Color::rgb(0xF0, 0xEE, 0xEA);
const DIVERGING_HIGH: Color = Color::rgb(0xD5, 0x5E, 0x00);

/// A continuous colour ramp: an ordered set of stops, evenly spaced over
/// `0.0..=1.0`, interpolated in linear light.
///
/// Construct with [`sequential`](Self::sequential) /
/// [`diverging`](Self::diverging) (or the [`viridis`](Self::viridis) /
/// [`blue_orange`](Self::blue_orange) defaults), then map data with
/// [`map`](Self::map) / [`map_diverging`](Self::map_diverging), or a
/// pre-normalised fraction with [`sample`](Self::sample).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorScale {
    /// Two or more stops at `t = i / (len - 1)`. A shorter vec is tolerated
    /// (the constructors do not panic); [`sample`](Self::sample) then returns
    /// the single stop, or [`FALLBACK`] when empty.
    stops: Vec<Color>,
}

impl ColorScale {
    /// A sequential ramp through `stops`, low → high, evenly spaced.
    ///
    /// Two stops is the minimum useful ramp; more stops describe a curve
    /// through colour space that two endpoints cannot (which is what makes a
    /// perceptual map like [`viridis`](Self::viridis) perceptually uniform —
    /// its lightness rises evenly, which a straight line between two saturated
    /// hues does not).
    #[must_use]
    pub fn sequential(stops: Vec<Color>) -> Self {
        Self { stops }
    }

    /// A diverging ramp `low → mid → high` with the neutral anchored at the
    /// centre stop. Pair with [`map_diverging`](Self::map_diverging), which is
    /// what keeps the data's midpoint ON that neutral for an asymmetric domain.
    #[must_use]
    pub fn diverging(low: Color, mid: Color, high: Color) -> Self {
        Self {
            stops: vec![low, mid, high],
        }
    }

    /// The viridis sequential default — perceptually uniform and
    /// colour-blind-safe. The right choice when the reader must rank cells,
    /// and the one to reach for instead of a hand-built two-hue ramp.
    #[must_use]
    pub fn viridis() -> Self {
        Self {
            stops: VIRIDIS.to_vec(),
        }
    }

    /// The diverging default: Okabe-Ito blue → neutral → vermillion, for data
    /// with a meaningful zero (a delta, a correlation, an error against
    /// target). Colour-blind-safe because both wings come from the same
    /// qualitative set the categorical palette uses.
    #[must_use]
    pub fn blue_orange() -> Self {
        Self::diverging(DIVERGING_LOW, DIVERGING_MID, DIVERGING_HIGH)
    }

    /// The stops, in ramp order.
    #[must_use]
    pub fn stops(&self) -> &[Color] {
        &self.stops
    }

    /// The colour at fraction `t` of the ramp.
    ///
    /// `t` is clamped to `0.0..=1.0`, and a NaN `t` reads as `0.0` (the low
    /// end) rather than producing an arbitrary colour — the same
    /// total-function discipline [`Color::lerp`] itself follows, so a hole in
    /// the data cannot paint a random cell.
    #[must_use]
    pub fn sample(&self, t: f32) -> Color {
        match self.stops.len() {
            0 => FALLBACK,
            1 => self.stops[0],
            n => {
                let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
                // Position along the (n - 1) segments, split into the segment
                // index and the fraction within it. `t == 1.0` would index one
                // past the last segment, so the index is clamped and the
                // fraction carries the remainder.
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a stop count is a small table length; f32 represents it exactly"
                )]
                let scaled = t * (n - 1) as f32;
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "scaled is non-negative and at most n-1 after the clamp above"
                )]
                let index = (scaled.floor() as usize).min(n - 2);
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "the segment index is a small table index; f32 represents it exactly"
                )]
                let local = scaled - index as f32;
                self.stops[index].lerp(self.stops[index + 1], local)
            }
        }
    }

    /// The colour for `value` on a linear domain `min..=max`.
    ///
    /// Values outside the domain clamp to the ramp ends (an outlier reads as
    /// "at least the extreme", never as a wrapped-around colour). A degenerate
    /// domain (`max <= min` — one distinct value, or none) maps everything to
    /// the low end: with no spread there is nothing to rank, and inventing a
    /// mid-ramp colour would imply a magnitude the data does not support.
    #[must_use]
    pub fn map(&self, value: f64, min: f64, max: f64) -> Color {
        // `max > min` is the only case with a rankable spread; anything else
        // (equal, inverted, or a NaN bound) is degenerate. Written as a
        // positive predicate so a NaN bound falls into the degenerate arm
        // instead of a negated partial comparison.
        if max <= min || max.is_nan() || min.is_nan() {
            return self.sample(0.0);
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a 0.0..=1.0 domain fraction loses no meaningful precision as f32"
        )]
        let t = ((value - min) / (max - min)) as f32;
        self.sample(t)
    }

    /// The colour for `value` on a diverging domain `low..=high` whose neutral
    /// anchor is `neutral`.
    ///
    /// Each side is normalised INDEPENDENTLY — `low..=neutral` fills
    /// `0.0..=0.5` and `neutral..=high` fills `0.5..=1.0` — so it lands on the
    /// ramp's centre stop even when the two sides have different widths. That
    /// is the whole contract of a diverging scale: with a linear map, data that
    /// ran from -2 to +8 would paint 0 at a fifth of the ramp and the reader
    /// would take a positive deviation for the neutral.
    ///
    /// A side of zero width contributes its half of the ramp as the neutral
    /// (nothing on that side can be ranked), and values outside the domain
    /// clamp to the ends.
    #[must_use]
    pub fn map_diverging(&self, value: f64, low: f64, neutral: f64, high: f64) -> Color {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a 0.0..=1.0 domain fraction loses no meaningful precision as f32"
        )]
        let t = if value < neutral {
            if neutral > low {
                (0.5 * (value - low) / (neutral - low)) as f32
            } else {
                0.5
            }
        } else if high > neutral {
            (0.5 + 0.5 * (value - neutral) / (high - neutral)) as f32
        } else {
            0.5
        };
        self.sample(t)
    }

    /// R1438 — the ramp as `(domain_fraction, colour)` pairs, i.e. where each
    /// of this scale's stops sits ALONG THE DOMAIN under the given encoding.
    /// This is what a colour bar must be built from, because for a diverging
    /// encoding it is not the same thing as the scale's own even spacing.
    ///
    /// `neutral = None` is the sequential case: stop `k` of `n` sits at
    /// `k / (n - 1)`, matching [`Self::map`] exactly (both are linear in `t`,
    /// and colour is piecewise-linear between stops, so a gradient with these
    /// offsets reproduces the mapping rather than approximating it).
    ///
    /// `neutral = Some(v)` is the diverging case: the centre stop moves to
    /// `v`'s fraction of `low..=high` and each wing's stops spread over their
    /// own side, mirroring [`Self::map_diverging`]'s independent normalisation.
    /// On an asymmetric domain the centre is therefore NOT at 0.5 — which is
    /// precisely the fact a bar built from even spacing would hide.
    ///
    /// A degenerate domain collapses every stop onto `0.0`, agreeing with the
    /// maps' "nothing to rank" behaviour.
    #[must_use]
    pub fn stop_offsets(&self, domain: (f64, f64), neutral: Option<f64>) -> Vec<(f32, Color)> {
        let (low, high) = domain;
        let stops = self.stops();
        if stops.is_empty() {
            return Vec::new();
        }
        let degenerate = high <= low || high.is_nan() || low.is_nan();
        let last = stops.len().saturating_sub(1);
        stops
            .iter()
            .enumerate()
            .map(|(k, &color)| {
                if degenerate || last == 0 {
                    return (0.0, color);
                }
                let even = f64::from(u32::try_from(k).unwrap_or(u32::MAX))
                    / f64::from(u32::try_from(last).unwrap_or(u32::MAX));
                let fraction = match neutral {
                    None => even,
                    Some(neutral) => {
                        let centre = ((neutral - low) / (high - low)).clamp(0.0, 1.0);
                        if even <= 0.5 {
                            centre * (even / 0.5)
                        } else {
                            centre + (1.0 - centre) * ((even - 0.5) / 0.5)
                        }
                    }
                };
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "a 0.0..=1.0 offset loses no meaningful precision as f32"
                )]
                (fraction as f32, color)
            })
            .collect()
    }
}

/// Which map a chart runs its value channel through — the encoding a
/// [`ColorScale`] is *used* as, as opposed to the ramp itself.
///
/// The two arms are not a style toggle: they select which of the colour scale's
/// two total maps runs, and a diverging encoding additionally carries the anchor
/// the data considers neutral (a target, a baseline, zero) — a number that
/// belongs to the DATA, not to the palette, which is why it rides here rather
/// than on [`ColorScale`].
///
/// Lifted here at R1439 from `scatter.rs`, where R1438 first wrote it, when the
/// treemap became the second chart type to colour by a measure. It belongs next
/// to [`ColorScale`] rather than in either chart: choosing between the two maps
/// (and deriving the legend's stops and ticks from that choice) is a property of
/// the encoding, not of what geometry the marks happen to be. Each chart keeps
/// only what is genuinely its own — where the bar sits, and which of its data
/// fields carries the measure.
#[derive(Debug, Clone)]
pub(crate) enum ColorBy {
    /// Rank magnitude across the domain — [`ColorScale::map`].
    Sequential(ColorScale),
    /// Rank signed deviation from `neutral`, each side normalised on its own
    /// width — [`ColorScale::map_diverging`].
    Diverging { scale: ColorScale, neutral: f64 },
}

impl ColorBy {
    /// The mark colour for `value` over `domain`.
    pub(crate) fn resolve(&self, value: f64, domain: (f64, f64)) -> Color {
        match self {
            Self::Sequential(scale) => scale.map(value, domain.0, domain.1),
            Self::Diverging { scale, neutral } => {
                scale.map_diverging(value, domain.0, *neutral, domain.1)
            }
        }
    }

    /// The bar's `(fraction, colour)` ramp over `domain` — the encoding's own
    /// stop placement, so the legend cannot disagree with the marks.
    pub(crate) fn bar_stops(&self, domain: (f64, f64)) -> Vec<(f32, Color)> {
        match self {
            Self::Sequential(scale) => scale.stop_offsets(domain, None),
            Self::Diverging { scale, neutral } => scale.stop_offsets(domain, Some(*neutral)),
        }
    }

    /// The anchor a diverging encoding pins, for the bar's extra tick.
    pub(crate) fn neutral(&self) -> Option<f64> {
        match self {
            Self::Sequential(_) => None,
            Self::Diverging { neutral, .. } => Some(*neutral),
        }
    }

    /// The bar's `(fraction, value)` ticks over `domain`: the domain ends
    /// always, plus — for a diverging encoding whose neutral falls strictly
    /// inside the domain — the neutral, at the fraction of the domain it
    /// actually occupies.
    ///
    /// Shared by every chart that draws a colour bar (R1439, at the second),
    /// because a tick that disagreed with [`Self::bar_stops`] would put the
    /// label for the neutral somewhere other than the colour change it names.
    pub(crate) fn bar_ticks(&self, domain: (f64, f64)) -> Vec<(f32, f64)> {
        let (lo, hi) = domain;
        let mut ticks = vec![(0.0_f32, lo)];
        if let Some(neutral) = self.neutral()
            && hi > lo
            && neutral > lo
            && neutral < hi
        {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a 0.0..=1.0 domain fraction loses no meaningful precision as f32"
            )]
            ticks.push((((neutral - lo) / (hi - lo)) as f32, neutral));
        }
        ticks.push((1.0, hi));
        ticks
    }
}

/// A chart's whole value-colour encoding: which map, and over what domain.
///
/// Lifted at R1440, when the line chart became the THIRD chart type to colour by
/// a measure and the two fields plus their two resolver methods would have been
/// copied a third time (R1439's carry named this). Each chart now owns one of
/// these and supplies only what is genuinely its own: the function that MEASURES
/// its data's colour channel, since a series' third channel, a tile's second
/// measure and a segment's mean live in different shapes.
///
/// Deliberately not public: the chart's `color_by` / `color_by_diverging` /
/// `with_color_domain` builders are the API, and they read better at a call site
/// than assembling an encoding value would.
#[derive(Debug, Clone, Default)]
pub(crate) struct ValueEncoding {
    by: Option<ColorBy>,
    domain: Option<(f64, f64)>,
}

impl ValueEncoding {
    /// Encode on a SEQUENTIAL ramp.
    pub(crate) fn sequential(&mut self, scale: ColorScale) {
        self.by = Some(ColorBy::Sequential(scale));
    }

    /// Encode on a DIVERGING ramp anchored at `neutral`.
    pub(crate) fn diverging(&mut self, scale: ColorScale, neutral: f64) {
        self.by = Some(ColorBy::Diverging { scale, neutral });
    }

    /// Pin the domain instead of measuring it off the data.
    pub(crate) fn pin_domain(&mut self, lo: f64, hi: f64) {
        self.domain = Some((lo, hi));
    }

    /// Whether an encoding was asked for at all — the cheap check a chart makes
    /// before deciding which legend to draw or how much room to reserve.
    pub(crate) fn is_set(&self) -> bool {
        self.by.is_some()
    }

    /// The active domain: pinned, else `measure()`d off the data, else `None`
    /// when no encoding was asked for OR nothing in the data carries the
    /// channel — in which case the chart stays categorical.
    ///
    /// `measure` is a closure rather than a value so a chart that is not
    /// encoding never walks its data to find bounds it will not use.
    pub(crate) fn domain(
        &self,
        measure: impl FnOnce() -> Option<(f64, f64)>,
    ) -> Option<(f64, f64)> {
        self.by.as_ref()?;
        self.domain.or_else(measure)
    }

    /// The colour for `value`, or `None` when this chart is not encoding, has no
    /// domain to map against, or the datum carries no finite measure. A datum
    /// without the channel keeps its categorical colour: the encoding covers
    /// what has the measure and never invents one.
    pub(crate) fn color_for(
        &self,
        value: Option<f64>,
        measure: impl FnOnce() -> Option<(f64, f64)>,
    ) -> Option<Color> {
        let by = self.by.as_ref()?;
        let value = value.filter(|v| v.is_finite())?;
        Some(by.resolve(value, self.domain(measure)?))
    }

    /// The bar's ramp and ticks over `domain` — the encoding's own placement, so
    /// a legend cannot disagree with the marks. `None` when no encoding is set.
    pub(crate) fn bar(&self, domain: (f64, f64)) -> Option<BarRamp> {
        let by = self.by.as_ref()?;
        Some(BarRamp {
            stops: by.bar_stops(domain),
            ticks: by.bar_ticks(domain),
        })
    }
}

/// Everything a colour bar needs from an encoding: where the ramp's colours sit,
/// and which values get a label.
///
/// A named pair rather than a tuple because the two are both `Vec`s of `(f32, _)`
/// and a positional destructure at the call site would be one transposition away
/// from labelling the ramp with offsets meant for the strip.
pub(crate) struct BarRamp {
    /// `(fraction, colour)` — the gradient's stops.
    pub(crate) stops: Vec<(f32, Color)>,
    /// `(fraction, value)` — the labelled ticks.
    pub(crate) ticks: Vec<(f32, f64)>,
}

/// R1546 §5.3 — the contrast trio moved to [`pinion_core::contrast`] when the
/// §7 text-run background gave it a second consumer on the other side of the
/// dependency edge. Re-exported here because a heatmap reaching for
/// [`readable_ink`] next to [`ColorScale`] is the call site that motivated
/// them, and the import path it has always used still reads correctly.
///
/// WCAG 2.x asks `4.5` for body text and `3.0` for large text; a heatmap label
/// is small text on a saturated field, so `4.5` is the bar worth clearing.
pub use pinion_core::contrast::{contrast_ratio, readable_ink, relative_luminance};

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Color = Color::rgb(0, 0, 0);
    const WHITE: Color = Color::rgb(0xFF, 0xFF, 0xFF);
    /// A mid stop distinct from both ends, so a three-stop ramp's centre is
    /// identifiable by colour alone (R1438's offset tests compare stops).
    const GREY: Color = Color::rgb(0x80, 0x80, 0x80);

    #[test]
    fn sample_hits_every_stop_exactly() {
        // A stop must be reproduced EXACTLY at its own fraction: an anchor that
        // drifts by a channel would mean the published ramp is not the ramp
        // being painted.
        let scale = ColorScale::viridis();
        let n = scale.stops().len();
        for (i, stop) in scale.stops().iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let t = i as f32 / (n - 1) as f32;
            assert_eq!(scale.sample(t), *stop, "stop {i} at t={t}");
        }
    }

    #[test]
    fn sample_clamps_out_of_range_and_nan() {
        let scale = ColorScale::viridis();
        let low = scale.stops()[0];
        let high = scale.stops()[scale.stops().len() - 1];
        assert_eq!(
            scale.sample(-3.0),
            low,
            "below the ramp clamps to the low end"
        );
        assert_eq!(
            scale.sample(4.0),
            high,
            "above the ramp clamps to the high end"
        );
        assert_eq!(scale.sample(f32::NAN), low, "NaN reads as the low end");
    }

    #[test]
    fn a_degenerate_scale_never_panics() {
        assert_eq!(ColorScale::sequential(vec![]).sample(0.5), FALLBACK);
        let one = Color::rgb(1, 2, 3);
        assert_eq!(ColorScale::sequential(vec![one]).sample(0.5), one);
    }

    #[test]
    fn map_ranks_linearly_and_clamps_outliers() {
        let scale = ColorScale::sequential(vec![BLACK, WHITE]);
        assert_eq!(scale.map(0.0, 0.0, 100.0), BLACK);
        assert_eq!(scale.map(100.0, 0.0, 100.0), WHITE);
        assert_eq!(scale.map(-50.0, 0.0, 100.0), BLACK, "outlier clamps low");
        assert_eq!(scale.map(500.0, 0.0, 100.0), WHITE, "outlier clamps high");
        // The midpoint of a black→white ramp is the linear-light mid, which is
        // LIGHTER than 8-bit 128 — the whole reason interpolation happens in
        // linear space (a naive byte lerp would give 128 and a muddy ramp).
        let mid = scale.map(50.0, 0.0, 100.0);
        assert!(mid.r > 180, "linear-light midpoint is light, got {}", mid.r);
        assert_eq!(mid.r, mid.g, "a grey ramp stays neutral");
    }

    #[test]
    fn a_degenerate_domain_maps_to_the_low_end() {
        let scale = ColorScale::sequential(vec![BLACK, WHITE]);
        assert_eq!(
            scale.map(7.0, 7.0, 7.0),
            BLACK,
            "no spread, nothing to rank"
        );
        assert_eq!(
            scale.map(7.0, 9.0, 3.0),
            BLACK,
            "inverted domain is degenerate"
        );
    }

    #[test]
    fn diverging_anchors_the_midpoint_on_an_asymmetric_domain() {
        // The contract that separates `map_diverging` from `map`: with the data
        // running -2..+8, zero must land on the NEUTRAL stop, not a fifth of
        // the way up the ramp.
        let scale = ColorScale::blue_orange();
        let neutral = scale.stops()[1];
        assert_eq!(scale.map_diverging(0.0, -2.0, 0.0, 8.0), neutral);
        // The linear map is what gets this wrong — asserted so the difference
        // is a fact of the test suite, not a claim in prose.
        assert_ne!(scale.map(0.0, -2.0, 8.0), neutral);
        // Each wing still reaches its own end.
        assert_eq!(scale.map_diverging(-2.0, -2.0, 0.0, 8.0), scale.stops()[0]);
        assert_eq!(scale.map_diverging(8.0, -2.0, 0.0, 8.0), scale.stops()[2]);
        // A value halfway up the SHORT side and one halfway up the long side
        // are equally saturated — each side is normalised on its own width.
        assert_eq!(
            scale.map_diverging(-1.0, -2.0, 0.0, 8.0),
            scale.sample(0.25)
        );
        assert_eq!(scale.map_diverging(4.0, -2.0, 0.0, 8.0), scale.sample(0.75));
    }

    #[test]
    fn diverging_tolerates_a_one_sided_domain() {
        let scale = ColorScale::blue_orange();
        let neutral = scale.stops()[1];
        // No room below the midpoint: everything at or under it is neutral,
        // and the upper wing still ranks.
        assert_eq!(scale.map_diverging(-5.0, 0.0, 0.0, 10.0), neutral);
        assert_eq!(scale.map_diverging(10.0, 0.0, 0.0, 10.0), scale.stops()[2]);
        // No room on either side.
        assert_eq!(scale.map_diverging(3.0, 3.0, 3.0, 3.0), neutral);
    }

    #[test]
    fn viridis_lightness_increases_monotonically() {
        // The property that makes it a legitimate sequential map: rank survives
        // greyscale and colour-vision deficiency because LIGHTNESS carries the
        // magnitude, not hue alone.
        let scale = ColorScale::viridis();
        let mut prev = -1.0_f32;
        for (i, stop) in scale.stops().iter().enumerate() {
            let l = relative_luminance(*stop);
            assert!(l > prev, "stop {i} luminance {l} must exceed {prev}");
            prev = l;
        }
    }

    /// The WCAG anchors themselves (black/white luminance, the 21.0 maximum,
    /// symmetry, self-contrast 1.0) moved to `pinion_core::contrast` with the
    /// functions in R1546. What stays here is the part that is about a RAMP.
    #[test]
    fn readable_ink_picks_the_higher_contrast_and_survives_the_whole_ramp() {
        assert_eq!(readable_ink(WHITE, BLACK, WHITE), BLACK);
        assert_eq!(readable_ink(BLACK, BLACK, WHITE), WHITE);
        // Ties prefer the first candidate (a caller's stated preference).
        assert_eq!(readable_ink(WHITE, WHITE, WHITE), WHITE);
        // The real job: every step of a viridis ramp gets an ink clearing the
        // WCAG 4.5 small-text bar with one of the two theme inks. A fixed
        // half-way threshold does NOT hold here — viridis's dark end is deep
        // enough that the flip happens past the middle, which is exactly why
        // the choice is computed.
        let scale = ColorScale::viridis();
        for step in 0..=20_u8 {
            let t = f32::from(step) / 20.0;
            let bg = scale.sample(t);
            let ink = readable_ink(bg, BLACK, WHITE);
            assert!(
                contrast_ratio(bg, ink) >= 4.5,
                "t={t} bg={bg:?} ink={ink:?} ratio={}",
                contrast_ratio(bg, ink)
            );
        }
    }

    /// R1438 — a SEQUENTIAL ramp's stops are evenly spaced, and the spacing
    /// agrees with `map`: the colour at a stop's offset IS that stop.
    #[test]
    fn r1438_sequential_stop_offsets_are_even_and_agree_with_map() {
        let scale = ColorScale::sequential(vec![BLACK, GREY, WHITE]);
        let offsets = scale.stop_offsets((0.0, 10.0), None);
        assert_eq!(offsets.len(), 3);
        assert!((offsets[0].0 - 0.0).abs() < 1e-6);
        assert!((offsets[1].0 - 0.5).abs() < 1e-6);
        assert!((offsets[2].0 - 1.0).abs() < 1e-6);
        // The bar and the map are one encoding: the value at each offset's
        // fraction of the domain is painted that offset's colour.
        for &(offset, color) in &offsets {
            let value = 0.0 + f64::from(offset) * 10.0;
            assert_eq!(scale.map(value, 0.0, 10.0), color, "offset {offset}");
        }
    }

    /// ★ R1438 — a DIVERGING ramp over an ASYMMETRIC domain moves its centre
    /// stop to the neutral's fraction, and the wings spread over their own
    /// widths. This is the number a bar built from even spacing gets wrong.
    #[test]
    fn r1438_diverging_stop_offsets_follow_the_neutral_not_the_midpoint() {
        let scale = ColorScale::diverging(BLACK, GREY, WHITE);
        // -2 ..= +8 about 0: the low wing is a fifth of the domain.
        let offsets = scale.stop_offsets((-2.0, 8.0), Some(0.0));
        assert!((offsets[0].0 - 0.0).abs() < 1e-6, "low end anchors at 0");
        assert!(
            (offsets[1].0 - 0.2).abs() < 1e-6,
            "the neutral sits at ITS fraction (0.2), not 0.5 — got {}",
            offsets[1].0
        );
        assert!((offsets[2].0 - 1.0).abs() < 1e-6, "high end anchors at 1");
        // Counterfactual: the sequential placement of the same scale over the
        // same domain does NOT agree, so the assertion above discriminates.
        let even = scale.stop_offsets((-2.0, 8.0), None);
        assert!((even[1].0 - 0.5).abs() < 1e-6);

        // And it agrees with the map it describes.
        for &(offset, color) in &offsets {
            let value = -2.0 + f64::from(offset) * 10.0;
            assert_eq!(
                scale.map_diverging(value, -2.0, 0.0, 8.0),
                color,
                "offset {offset}"
            );
        }
    }

    /// A symmetric domain is the case where the two placements COINCIDE — the
    /// boundary that shows the diverging arm is not just "always different".
    #[test]
    fn r1438_a_symmetric_domain_puts_the_neutral_at_the_midpoint() {
        let scale = ColorScale::diverging(BLACK, GREY, WHITE);
        let offsets = scale.stop_offsets((-5.0, 5.0), Some(0.0));
        assert!((offsets[1].0 - 0.5).abs() < 1e-6);
    }

    /// Degenerate inputs collapse rather than dividing by zero or panicking,
    /// matching the maps' own "nothing to rank" behaviour.
    #[test]
    fn r1438_stop_offsets_are_total() {
        let scale = ColorScale::diverging(BLACK, GREY, WHITE);
        for domain in [(5.0, 5.0), (8.0, 2.0), (f64::NAN, 1.0)] {
            let offsets = scale.stop_offsets(domain, Some(0.0));
            assert_eq!(offsets.len(), 3);
            assert!(offsets.iter().all(|&(o, _)| (o - 0.0).abs() < 1e-6));
        }
        // A neutral outside the domain clamps instead of running off the bar.
        let clamped = scale.stop_offsets((0.0, 10.0), Some(50.0));
        assert!((clamped[1].0 - 1.0).abs() < 1e-6);
        assert!(
            ColorScale::sequential(Vec::new())
                .stop_offsets((0.0, 1.0), None)
                .is_empty()
        );
        assert_eq!(
            ColorScale::sequential(vec![BLACK]).stop_offsets((0.0, 1.0), None),
            vec![(0.0, BLACK)]
        );
    }
}
