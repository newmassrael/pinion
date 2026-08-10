//! R1625 — the curve a line chart draws **between** its points, and whether
//! that curve invented a value.
//!
//! # The problem a spline has
//!
//! A chart that joins its samples with straight lines draws only values the
//! data contains. A smooth one does not: a cubic through
//! `(0,0) (1,0) (2,0) (3,10)` dips **below zero** before the rise, so a chart
//! of a quantity that cannot be negative — a queue depth, a byte count, a
//! price — paints one anyway, and no reader can tell that from a measurement.
//!
//! The reference toolkit's spline series is exactly this: one method, no
//! choice of it, its control points internal, and nothing that reports the
//! excursion. So the answer to "did my chart just draw a value I never
//! recorded" is to look at it.
//!
//! # What this offers instead
//!
//! [`Interpolation`] is a **declared** choice, and the two that matter are
//! opposites:
//!
//! * [`Interpolation::Monotone`] — Fritsch–Carlson tangents. Every segment
//!   stays within its own endpoints, **by construction**, so it cannot invent
//!   a value. A monotone run of samples stays monotone. The price is that the
//!   curve is only C¹ and flattens where the data turns.
//! * [`Interpolation::CatmullRom`] — the classic smooth interpolant, which
//!   looks better and **does** overshoot. Offered because sometimes that is
//!   what a reader wants, and refusing to offer it would only move the
//!   hand-rolled copy into the application.
//!
//! And whichever is chosen, [`overshoot`] answers the question: it names the
//! segments whose curve leaves the range its own endpoints span, and by how
//! much. On [`Interpolation::Monotone`] it is always empty, which is a
//! property this module tests rather than a claim it makes.
//!
//! # The x order
//!
//! A spline through samples is a function of x, so it needs strictly
//! increasing x. A series that does not have it is a *path*, not a graph, and
//! is drawn [`Interpolation::Linear`] — reported by [`is_graph`] rather than
//! silently smoothed into a shape that crosses itself.

use pinion_core::path_data;
use pinion_core::scene::{PathCommand, PathPoint};

/// How a line chart joins consecutive samples.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum Interpolation {
    /// Straight segments — the only join that draws no value the data lacks.
    /// The default, and what every chart here did before R1625.
    #[default]
    Linear,
    /// Fritsch–Carlson monotone cubic: smooth, and provably inside its own
    /// endpoints on every segment.
    Monotone,
    /// Catmull–Rom cubic: smoother, and free to overshoot. Use [`overshoot`]
    /// to find out where it did.
    CatmullRom,
}

impl Interpolation {
    /// Every interpolation, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 3] = [Self::Linear, Self::Monotone, Self::CatmullRom];

    /// Stable name, for a wire form or a caption.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Monotone => "monotone",
            Self::CatmullRom => "catmull-rom",
        }
    }

    /// Whether this interpolation can draw a value outside the range its
    /// endpoints span.
    ///
    /// A *declaration*, checked against the geometry by this module's tests
    /// rather than trusted: [`Interpolation::Monotone`] promising `false`
    /// and then overshooting would be the worst failure available here, since
    /// the promise is the reason to choose it.
    #[must_use]
    pub const fn may_overshoot(self) -> bool {
        match self {
            Self::Linear | Self::Monotone => false,
            Self::CatmullRom => true,
        }
    }
}

/// One cubic segment of an interpolated curve, in the caller's own space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveSegment {
    /// Control point leaving the previous sample.
    pub c1: (f32, f32),
    /// Control point entering `end`.
    pub c2: (f32, f32),
    /// The sample this segment lands on.
    pub end: (f32, f32),
}

/// Whether `points` is a graph — strictly increasing in x, so a smooth
/// interpolation through them is a function.
///
/// Fewer than two points is trivially a graph.
#[must_use]
pub fn is_graph(points: &[(f32, f32)]) -> bool {
    points.windows(2).all(|w| w[1].0 > w[0].0)
}

/// The cubic segments joining `points` under `kind`.
///
/// Returns one segment per gap, so `n` points give `n - 1` segments and fewer
/// than two points give none. A non-graph input (see [`is_graph`]) is joined
/// with straight segments whatever `kind` says — smoothing a self-crossing
/// path through a function interpolant produces a shape that is not the
/// caller's data, and silently is the wrong way to do that.
#[must_use]
pub fn curve(points: &[(f32, f32)], kind: Interpolation) -> Vec<CurveSegment> {
    if points.len() < 2 {
        return Vec::new();
    }
    if kind == Interpolation::Linear || !is_graph(points) {
        return straight(points);
    }
    let slopes = tangents(points, kind);
    points
        .windows(2)
        .zip(slopes.windows(2))
        .map(|(p, m)| {
            let (x0, y0) = p[0];
            let (x1, y1) = p[1];
            let h = (x1 - x0) / 3.0;
            CurveSegment {
                c1: (x0 + h, h.mul_add(m[0], y0)),
                c2: (x1 - h, (-h).mul_add(m[1], y1)),
                end: (x1, y1),
            }
        })
        .collect()
}

/// Whether the effective join is straight — either because that is what was
/// asked for, or because the samples are not a graph and a function
/// interpolant would draw a shape that is not the caller's data.
fn is_straight(points: &[(f32, f32)], kind: Interpolation) -> bool {
    kind == Interpolation::Linear || !is_graph(points)
}

/// Straight segments expressed as cubics, so every interpolation answers in
/// one shape and a consumer never branches on which it asked for.
fn straight(points: &[(f32, f32)]) -> Vec<CurveSegment> {
    points
        .windows(2)
        .map(|w| {
            let (x0, y0) = w[0];
            let (x1, y1) = w[1];
            CurveSegment {
                c1: (x0 + (x1 - x0) / 3.0, y0 + (y1 - y0) / 3.0),
                c2: (x1 - (x1 - x0) / 3.0, y1 - (y1 - y0) / 3.0),
                end: (x1, y1),
            }
        })
        .collect()
}

/// The tangent at each sample.
///
/// `clippy::many_single_char_names` fires here and the names stay: `m` for the
/// tangents, `h` for the spans and `s` for the secants are Fritsch and
/// Carlson's own, which is what lets this be checked against the paper line by
/// line. The audit is moved to the tests instead — the monotonicity guarantee
/// is asserted over forty pseudo-random shapes, and a plateau, a monotone run
/// and a straight line each have their own case.
#[allow(
    clippy::many_single_char_names,
    reason = "the paper's variable names, kept so the derivation can be checked against it"
)]
fn tangents(points: &[(f32, f32)], kind: Interpolation) -> Vec<f32> {
    let n = points.len();
    let secants: Vec<f32> = points
        .windows(2)
        .map(|w| (w[1].1 - w[0].1) / (w[1].0 - w[0].0))
        .collect();

    // The interior tangent both methods start from: Catmull–Rom's centred
    // difference, weighted by the neighbouring spans so an uneven x grid does
    // not tilt the curve toward the wider gap.
    let mut m = Vec::with_capacity(n);
    m.push(secants[0]);
    for i in 1..n - 1 {
        let (h0, h1) = (points[i].0 - points[i - 1].0, points[i + 1].0 - points[i].0);
        m.push(h1.mul_add(secants[i - 1], h0 * secants[i]) / (h0 + h1));
    }
    m.push(secants[n - 2]);

    if kind != Interpolation::Monotone {
        return m;
    }

    // Fritsch–Carlson. Two rules, and the second is what the guarantee rests
    // on: a tangent longer than three times its secant lets the segment leave
    // its endpoints, so the pair is scaled back onto the circle of radius 3.
    for (i, s) in secants.iter().enumerate() {
        if s.abs() < f32::EPSILON {
            // A flat run stays flat — this is the rule that stops a spline
            // bulging through a plateau.
            m[i] = 0.0;
            m[i + 1] = 0.0;
            continue;
        }
        // A tangent that disagrees in sign with its secant would turn the
        // segment back on itself.
        if m[i] * s < 0.0 {
            m[i] = 0.0;
        }
        if m[i + 1] * s < 0.0 {
            m[i + 1] = 0.0;
        }
        let (a, b) = (m[i] / s, m[i + 1] / s);
        let t = a.hypot(b);
        if t > 3.0 {
            let k = 3.0 / t;
            m[i] = k * a * s;
            m[i + 1] = k * b * s;
        }
    }
    m
}

/// One segment that left the range its endpoints span.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Overshoot {
    /// Index of the gap, so the samples are `points[segment]` and
    /// `points[segment + 1]`.
    pub segment: usize,
    /// The extreme value the curve actually reached.
    pub extreme: f32,
    /// How far past its endpoints it went — always positive.
    pub beyond: f32,
    /// `true` when the excursion is above both endpoints.
    pub above: bool,
}

/// Every segment whose curve leaves the range its own endpoints span.
///
/// The answer a chart owes a reader who asks whether a smooth line drew a
/// value that was never measured. Empty for [`Interpolation::Linear`] and
/// [`Interpolation::Monotone`] — the second by construction, and this module
/// proves it over random data rather than asserting it.
///
/// The extremes are solved rather than sampled: each segment is handed to
/// `pinion_core::path_data::bounds`, which finds a cubic's turning points
/// exactly, so a bulge between two samples cannot be stepped over.
#[must_use]
pub fn overshoot(points: &[(f32, f32)], kind: Interpolation) -> Vec<Overshoot> {
    /// A tolerance, because the extrema are solved in `f32`: a curve that
    /// touches its own endpoint is not an excursion.
    const EPS: f32 = 1e-3;
    let mut out = Vec::new();
    let segments = curve(points, kind);
    for (i, seg) in segments.iter().enumerate() {
        let (x0, y0) = points[i];
        let (lo, hi) = (y0.min(seg.end.1), y0.max(seg.end.1));
        let commands = [
            PathCommand::MoveTo(PathPoint::new(x0, y0)),
            PathCommand::CurveTo {
                c1: PathPoint::new(seg.c1.0, seg.c1.1),
                c2: PathPoint::new(seg.c2.0, seg.c2.1),
                end: PathPoint::new(seg.end.0, seg.end.1),
            },
        ];
        let Some(b) = path_data::bounds(&commands) else {
            continue;
        };
        if b.max_y > hi + EPS {
            out.push(Overshoot {
                segment: i,
                extreme: b.max_y,
                beyond: b.max_y - hi,
                above: true,
            });
        }
        if b.min_y < lo - EPS {
            out.push(Overshoot {
                segment: i,
                extreme: b.min_y,
                beyond: lo - b.min_y,
                above: false,
            });
        }
    }
    out
}

/// R1628 — append the cubics that walk `points` from its **last** point back
/// to its first, continuing an open path rather than starting one.
///
/// What an area fill needs: a band's outline runs forward along the upper
/// curve and backwards along the lower one, and reversing the point list and
/// re-interpolating would not do — a descending x is not a graph
/// ([`is_graph`]), so it would fall back to straight segments and leave the
/// band's lower edge visibly flat under a curved upper one.
///
/// The reversal is EXACT rather than a re-estimate: a cubic from `p` to `q`
/// with controls `(c1, c2)` is the same curve as one from `q` to `p` with
/// `(c2, c1)`, so the forward segments are walked in reverse with their
/// controls swapped. No new tangent is chosen, so the two edges of a band
/// cannot disagree about the curve they share.
pub fn append_reversed(points: &[(f32, f32)], kind: Interpolation, out: &mut Vec<PathCommand>) {
    if is_straight(points, kind) {
        for &(x, y) in points.iter().rev().skip(1) {
            out.push(PathCommand::LineTo(PathPoint::new(x, y)));
        }
        return;
    }
    let segments = curve(points, kind);
    for (i, seg) in segments.iter().enumerate().rev() {
        let start = points[i];
        out.push(PathCommand::CurveTo {
            c1: PathPoint::new(seg.c2.0, seg.c2.1),
            c2: PathPoint::new(seg.c1.0, seg.c1.1),
            end: PathPoint::new(start.0, start.1),
        });
    }
}

/// The [`PathCommand`] stream for `points` under `kind`, ready for a
/// [`pinion_core::Scene::Path`].
///
/// Emits real cubics rather than a densely sampled polyline, which is the
/// difference between a scene that says "a curve through these samples" and
/// one that says "two hundred short lines" — and R1623 is why the difference
/// is expressible.
#[must_use]
pub fn commands(points: &[(f32, f32)], kind: Interpolation) -> Vec<PathCommand> {
    let Some(&(x0, y0)) = points.first() else {
        return Vec::new();
    };
    let mut out = vec![PathCommand::MoveTo(PathPoint::new(x0, y0))];
    // R1628 — a straight join publishes as `LineTo`, not as a degenerate
    // cubic. `curve` answers in cubics for every interpolation because a
    // uniform shape is what its callers want to measure; the SCENE is a
    // different audience. Under §2 #7 a client reads what was authored, and a
    // straight segment authored straight must not arrive claiming to be a
    // curve — which is also why every pre-existing filled chart's command
    // stream is byte-unchanged by this round.
    if is_straight(points, kind) {
        for &(x, y) in points.iter().skip(1) {
            out.push(PathCommand::LineTo(PathPoint::new(x, y)));
        }
        return out;
    }
    for seg in curve(points, kind) {
        out.push(PathCommand::CurveTo {
            c1: PathPoint::new(seg.c1.0, seg.c1.1),
            c2: PathPoint::new(seg.c2.0, seg.c2.1),
            end: PathPoint::new(seg.end.0, seg.end.1),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic pseudo-random walk — no `rand` dependency, and the
    /// seed is in the test so a failure is reproducible.
    fn walk(n: usize, seed: u64) -> Vec<(f32, f32)> {
        let mut s = seed;
        let mut y = 50.0f32;
        (0..n)
            .map(|i| {
                s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                #[allow(clippy::cast_precision_loss, reason = "0..41 is exact in f32")]
                let step = ((s >> 33) % 41) as f32 - 20.0;
                y += step;
                #[allow(clippy::cast_precision_loss, reason = "n is small in tests")]
                let x = i as f32 * 7.0;
                (x, y)
            })
            .collect()
    }

    fn sample(seg: &CurveSegment, from: (f32, f32), t: f32) -> f32 {
        let mt = 1.0 - t;
        (mt * mt * mt).mul_add(
            from.1,
            (3.0 * mt * mt * t).mul_add(
                seg.c1.1,
                (3.0 * mt * t * t).mul_add(seg.c2.1, t * t * t * seg.end.1),
            ),
        )
    }

    #[test]
    fn a_curve_passes_through_every_sample() {
        let pts = walk(12, 7);
        for kind in Interpolation::ALL {
            let segs = curve(&pts, kind);
            assert_eq!(segs.len(), pts.len() - 1, "{kind:?}");
            for (i, seg) in segs.iter().enumerate() {
                assert_eq!(
                    seg.end,
                    pts[i + 1],
                    "{kind:?} segment {i} lands on its sample"
                );
            }
        }
    }

    /// ★ The guarantee the monotone interpolant exists for, checked against
    /// the geometry over many shapes rather than asserted once.
    #[test]
    fn the_monotone_curve_never_invents_a_value() {
        for seed in 0..40u64 {
            let pts = walk(9, seed);
            let found = overshoot(&pts, Interpolation::Monotone);
            assert!(found.is_empty(), "seed {seed} overshot: {found:?}");
        }
        // ...and the declaration agrees with the measurement.
        for kind in Interpolation::ALL {
            if !kind.may_overshoot() {
                for seed in 0..20u64 {
                    assert!(
                        overshoot(&walk(9, seed), kind).is_empty(),
                        "{kind:?} declares it cannot overshoot",
                    );
                }
            }
        }
    }

    /// ★ The counterpart: the smooth interpolant DOES overshoot, and the
    /// report finds it. Without this the test above would pass for an
    /// implementation that returned straight lines for everything.
    #[test]
    fn the_smooth_curve_overshoots_and_the_report_names_where() {
        // Three flat samples then a jump: the classic case.
        let pts = [(0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (30.0, 100.0)];
        let found = overshoot(&pts, Interpolation::CatmullRom);
        assert!(!found.is_empty(), "the smooth curve dips below the plateau");
        let dip = found.iter().find(|o| !o.above).expect("a dip below");
        assert!(dip.beyond > 0.0, "{dip:?}");
        assert!(dip.extreme < 0.0, "it goes below zero: {dip:?}");
        assert!(dip.segment < pts.len() - 1, "{dip:?}");

        // The monotone curve on the same data does not.
        assert!(overshoot(&pts, Interpolation::Monotone).is_empty());
        // Nor does the straight one.
        assert!(overshoot(&pts, Interpolation::Linear).is_empty());
    }

    #[test]
    fn a_monotone_run_stays_monotone() {
        let pts = [(0.0, 0.0), (10.0, 1.0), (20.0, 30.0), (30.0, 31.0)];
        let segs = curve(&pts, Interpolation::Monotone);
        for (i, seg) in segs.iter().enumerate() {
            let mut prev = pts[i].1;
            for k in 1..=20 {
                #[allow(clippy::cast_precision_loss, reason = "k is small")]
                let t = k as f32 / 20.0;
                let y = sample(seg, pts[i], t);
                assert!(y >= prev - 1e-3, "segment {i} turned back at t={t}");
                prev = y;
            }
        }
    }

    #[test]
    fn a_plateau_stays_flat() {
        let pts = [(0.0, 5.0), (10.0, 5.0), (20.0, 5.0), (30.0, 50.0)];
        let segs = curve(&pts, Interpolation::Monotone);
        for k in 0..=10 {
            #[allow(clippy::cast_precision_loss, reason = "k is small")]
            let t = k as f32 / 10.0;
            assert!(
                (sample(&segs[0], pts[0], t) - 5.0).abs() < 1e-3,
                "the flat run is flat at t={t}",
            );
        }
    }

    #[test]
    fn a_series_that_is_not_a_graph_is_drawn_straight() {
        // x turns back: this is a path, not a function of x.
        let pts = [(0.0, 0.0), (10.0, 10.0), (5.0, 20.0)];
        assert!(!is_graph(&pts));
        for kind in Interpolation::ALL {
            assert_eq!(
                curve(&pts, kind),
                curve(&pts, Interpolation::Linear),
                "{kind:?} does not smooth a non-graph",
            );
        }
        assert!(is_graph(&[(0.0, 0.0), (1.0, 9.0)]));
        assert!(is_graph(&[]));
    }

    #[test]
    fn straight_segments_are_the_line_they_replace() {
        let pts = [(0.0, 0.0), (30.0, 60.0)];
        let seg = curve(&pts, Interpolation::Linear)[0];
        for k in 0..=10 {
            #[allow(clippy::cast_precision_loss, reason = "k is small")]
            let t = k as f32 / 10.0;
            let y = sample(&seg, pts[0], t);
            assert!((y - t * 60.0).abs() < 1e-3, "t={t} gives {y}");
        }
    }

    /// ★ R1628 — the reversed walk is the SAME curve, not a re-estimate.
    ///
    /// Sampled at matching parameters from both directions: a band's two edges
    /// share a curve, so if this were a fresh interpolation of a reversed point
    /// list the two would disagree (and a descending x would fall back to
    /// straight segments, which is the bug this exists to avoid).
    #[test]
    fn r1628_the_reversed_walk_retraces_the_forward_curve() {
        let pts = walk(7, 11);
        // Linear is excluded on purpose: it publishes `LineTo`, not cubics,
        // so "the controls swap" is not a statement about it. Its straight
        // reversal is checked below.
        for kind in [Interpolation::Monotone, Interpolation::CatmullRom] {
            let forward = curve(&pts, kind);
            let mut back = Vec::new();
            append_reversed(&pts, kind, &mut back);
            assert_eq!(back.len(), forward.len(), "{kind:?}: one cubic per gap");
            for (k, cmd) in back.iter().enumerate() {
                // `back[k]` retraces `forward[len-1-k]`.
                let f = forward[forward.len() - 1 - k];
                let start = pts[forward.len() - 1 - k];
                let PathCommand::CurveTo { c1, c2, end } = *cmd else {
                    panic!("{kind:?}: a reversed walk is all cubics, got {cmd:?}");
                };
                assert_eq!((c1.x, c1.y), f.c2, "{kind:?} {k}: controls swap");
                assert_eq!((c2.x, c2.y), f.c1, "{kind:?} {k}: controls swap");
                assert_eq!((end.x, end.y), start, "{kind:?} {k}: lands on the sample");
                // And the midpoints agree, which is the geometric statement.
                // The reversed segment starts where the forward one ended.
                let mid_back = sample(
                    &CurveSegment {
                        c1: f.c2,
                        c2: f.c1,
                        end: start,
                    },
                    f.end,
                    0.5,
                );
                let mid_fwd = sample(&f, start, 0.5);
                assert!(
                    (mid_back - mid_fwd).abs() < 1e-2,
                    "{kind:?} {k}: same curve, {mid_back} vs {mid_fwd}",
                );
            }
        }
        // A straight reversal walks the samples back as lines, one per gap.
        let mut flat = Vec::new();
        append_reversed(&pts, Interpolation::Linear, &mut flat);
        assert_eq!(flat.len(), pts.len() - 1, "one line per gap");
        for (k, cmd) in flat.iter().enumerate() {
            let PathCommand::LineTo(q) = *cmd else {
                panic!("a straight reversal is all lines, got {cmd:?}");
            };
            let expected = pts[pts.len() - 2 - k];
            assert_eq!((q.x, q.y), expected, "{k}: lands on the sample");
        }
        // A single point has no gap and therefore no reversal.
        let mut none = Vec::new();
        append_reversed(&[(0.0, 0.0)], Interpolation::Monotone, &mut none);
        assert!(none.is_empty());
        let mut none_flat = Vec::new();
        append_reversed(&[(0.0, 0.0)], Interpolation::Linear, &mut none_flat);
        assert!(none_flat.is_empty());
    }

    #[test]
    fn commands_start_with_a_move_and_hold_one_curve_per_gap() {
        let pts = walk(5, 3);
        let cmds = commands(&pts, Interpolation::Monotone);
        assert!(matches!(cmds[0], PathCommand::MoveTo(_)));
        assert_eq!(cmds.len(), pts.len(), "one move plus one curve per gap");
        assert!(
            cmds[1..]
                .iter()
                .all(|c| matches!(c, PathCommand::CurveTo { .. }))
        );
        assert!(commands(&[], Interpolation::Monotone).is_empty());
    }

    #[test]
    fn every_interpolation_has_a_distinct_name() {
        let mut names: Vec<&str> = Interpolation::ALL.iter().map(|k| k.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Interpolation::ALL.len());
    }
}
