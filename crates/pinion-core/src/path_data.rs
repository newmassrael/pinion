//! R1623 §5.3 — SVG path data: the authored vocabulary, the wire
//! description derived from it, and the closed render form every
//! rasterizer consumes.
//!
//! # Why a path keeps the curve it was authored as
//!
//! [`PathCommand`] is the vocabulary an author writes; [`PathSegment`]
//! is what a rasterizer draws. They are deliberately different types.
//!
//! The reference toolkit collapses the two. Its painter-path type has
//! a quadratic builder that computes the equivalent cubic control
//! points and calls the cubic builder, an arc builder that appends
//! Béziers, and a stored element list of exactly four kinds: move-to,
//! line-to, curve-to and curve-data. So a path there cannot be asked
//! whether it holds an arc — the answer was discarded at construction,
//! and there is no inverse back to path data.
//!
//! §2 #7 (scene-as-data) makes that unacceptable here — a client
//! reading the scene must see the geometry the author declared, not a
//! rasterizer's expansion of it. So [`PathCommand::QuadTo`] and
//! [`PathCommand::ArcTo`] survive into the scene, onto the wire, and
//! back out through [`write()`], and the cubic expansion is *derived* on
//! demand by [`for_each_segment`].
//!
//! # Why the derived form is a second, CLOSED type
//!
//! [`PathCommand`] is `#[non_exhaustive]` so the vocabulary can grow.
//! Before R1623 every consumer matched it directly with a wildcard
//! arm, which made growth silently lossy in two places at once: the
//! Vello adapter skipped an unrecognised command (`_ => {}`) and the
//! RPC wire collapsed it to `"Unknown"`. A new arm would have painted
//! nothing and introspected as nothing, with nothing failing.
//!
//! [`PathSegment`] is **not** `non_exhaustive`, and the only way to
//! obtain one is [`for_each_segment`], whose match over `PathCommand`
//! is exhaustive *inside this crate* where `non_exhaustive` does not
//! apply. A new command therefore breaks the normaliser at compile
//! time and cannot reach a painter unhandled. The same holds for the
//! wire: [`PathCommand::describe`] is the single exhaustive
//! description, and `pinion-rpc` renders [`PathArgValue`] — three
//! closed arms — rather than matching commands itself.
//!
//! # What is shorthand and what is geometry
//!
//! `d` shorthand that is fully determined by what precedes it is
//! resolved at parse time and is NOT part of the vocabulary: relative
//! commands become absolute, `H`/`V` become [`PathCommand::LineTo`],
//! and the smooth forms `S`/`T` become their reflected
//! [`PathCommand::CurveTo`] / [`PathCommand::QuadTo`]. Reconstructing
//! them would be a presentation choice, and none of them names a
//! different curve.
//!
//! A quadratic and an arc are different: re-elevating a quadratic
//! discards the authored degree (and doubles the data), and an arc has
//! no exact cubic form at all. Those two are kept.

use core::fmt::Write as _;

use crate::scene::{EllipticalArc, PathCommand, PathPoint};

/// Largest number of arguments any [`PathCommand`] carries —
/// [`PathCommand::ArcTo`]'s two radii, rotation, two flags and
/// endpoint.
pub const MAX_PATH_ARGS: usize = 6;

/// Which command a [`PathCommand`] is, without its payload.
///
/// **Deliberately not `#[non_exhaustive]`**, mirroring the R1613
/// `CellRole` shape: `PathCommand` grows, this collapses it, and every
/// out-of-crate consumer that switches on a command switches on this
/// instead — so a new command breaks those consumers loudly rather
/// than falling into a wildcard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum PathCommandKind {
    /// [`PathCommand::MoveTo`] — SVG `M`.
    MoveTo,
    /// [`PathCommand::LineTo`] — SVG `L`.
    LineTo,
    /// [`PathCommand::QuadTo`] — SVG `Q`.
    QuadTo,
    /// [`PathCommand::CurveTo`] — SVG `C`.
    CurveTo,
    /// [`PathCommand::ArcTo`] — SVG `A`.
    ArcTo,
    /// [`PathCommand::Close`] — SVG `Z`.
    Close,
}

impl PathCommandKind {
    /// Every kind, in vocabulary order — the census a consumer walks
    /// when it must cover the whole vocabulary rather than the arms it
    /// happens to know about.
    ///
    /// Kept honest from two directions: the declared length rejects a
    /// *removal* at compile time, and
    /// `test_fixtures::path_command_of_kind`
    /// matches exhaustively, so a new kind cannot be added without a
    /// fixture — and the fixture is what puts it into this list's tests.
    ///
    /// **The residue, stated rather than papered over**: a new
    /// [`PathCommand`] arm that reuses an EXISTING kind is caught by
    /// neither. `describe` would compile, and two different commands
    /// would then be labelled identically on the wire. Nothing here can
    /// see that, because no census can enumerate the commands of a
    /// `non_exhaustive` enum from outside its definition — the check
    /// that would catch it is the author noticing that a new curve
    /// needs a new name.
    pub const ALL: [Self; 6] = [
        Self::MoveTo,
        Self::LineTo,
        Self::QuadTo,
        Self::CurveTo,
        Self::ArcTo,
        Self::Close,
    ];

    /// Stable name used on the RPC wire (`snapshot` path commands).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::MoveTo => "MoveTo",
            Self::LineTo => "LineTo",
            Self::QuadTo => "QuadTo",
            Self::CurveTo => "CurveTo",
            Self::ArcTo => "ArcTo",
            Self::Close => "Close",
        }
    }

    /// The **absolute** SVG path-data letter for this command.
    ///
    /// [`write()`] emits only absolute commands, so there is no relative
    /// spelling to choose between.
    #[must_use]
    pub const fn svg_letter(self) -> char {
        match self {
            Self::MoveTo => 'M',
            Self::LineTo => 'L',
            Self::QuadTo => 'Q',
            Self::CurveTo => 'C',
            Self::ArcTo => 'A',
            Self::Close => 'Z',
        }
    }
}

/// One argument's value inside a [`PathCommandDescription`].
///
/// Not `#[non_exhaustive]`: a renderer of descriptions (the RPC wire,
/// [`write()`]) must handle every shape a command argument can take, and
/// three is the whole set — a point, a plain number, a boolean flag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathArgValue {
    /// A coordinate pair, in the node-rect-relative basis (R1358).
    Point(PathPoint),
    /// A plain number — a radius, a rotation in degrees.
    Scalar(f32),
    /// An SVG arc flag; written as `0` / `1` in path data.
    Flag(bool),
}

/// One named argument of a command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathArg {
    /// Argument name — the key the RPC wire uses.
    pub name: &'static str,
    /// The argument's value.
    pub value: PathArgValue,
}

const ARG_FILLER: PathArg = PathArg {
    name: "",
    value: PathArgValue::Scalar(0.0),
};

/// A [`PathCommand`] reduced to `(kind, named arguments)`.
///
/// This is the single place a command's payload is enumerated. The RPC
/// wire form and the [`write()`] path-data spelling are both *derived*
/// from it, so those two cannot disagree about what a command carries,
/// and neither can silently omit a command the vocabulary gained.
///
/// Carries its arguments inline (no allocation) because
/// [`crate::scene::Scene::paint_hash`] describes every command of every
/// path on every frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathCommandDescription {
    kind: PathCommandKind,
    args: [PathArg; MAX_PATH_ARGS],
    len: usize,
}

impl PathCommandDescription {
    fn new(kind: PathCommandKind, args: &[PathArg]) -> Self {
        let mut buf = [ARG_FILLER; MAX_PATH_ARGS];
        let len = args.len().min(MAX_PATH_ARGS);
        buf[..len].copy_from_slice(&args[..len]);
        Self {
            kind,
            args: buf,
            len,
        }
    }

    /// Which command this describes.
    #[must_use]
    pub const fn kind(&self) -> PathCommandKind {
        self.kind
    }

    /// The command's arguments, in declaration order.
    #[must_use]
    pub fn args(&self) -> &[PathArg] {
        &self.args[..self.len]
    }
}

impl PathCommand {
    /// Describe this command as `(kind, named arguments)`.
    ///
    /// The exhaustive match here is what makes growing the vocabulary
    /// safe: it lives in `pinion-core`, where `non_exhaustive` does not
    /// apply, so a new arm fails to compile until it is described — and
    /// once described it reaches the wire and [`write()`] for free.
    #[must_use]
    pub fn describe(&self) -> PathCommandDescription {
        let point = |name, p| PathArg {
            name,
            value: PathArgValue::Point(p),
        };
        let scalar = |name, v| PathArg {
            name,
            value: PathArgValue::Scalar(v),
        };
        let flag = |name, v| PathArg {
            name,
            value: PathArgValue::Flag(v),
        };
        match *self {
            Self::MoveTo(p) => {
                PathCommandDescription::new(PathCommandKind::MoveTo, &[point("point", p)])
            }
            Self::LineTo(p) => {
                PathCommandDescription::new(PathCommandKind::LineTo, &[point("point", p)])
            }
            Self::QuadTo { c, end } => PathCommandDescription::new(
                PathCommandKind::QuadTo,
                &[point("c", c), point("end", end)],
            ),
            Self::CurveTo { c1, c2, end } => PathCommandDescription::new(
                PathCommandKind::CurveTo,
                &[point("c1", c1), point("c2", c2), point("end", end)],
            ),
            Self::ArcTo(arc) => PathCommandDescription::new(
                PathCommandKind::ArcTo,
                &[
                    scalar("rx", arc.rx),
                    scalar("ry", arc.ry),
                    scalar("x_rotation", arc.x_rotation),
                    flag("large_arc", arc.large_arc),
                    flag("sweep", arc.sweep),
                    point("end", arc.end),
                ],
            ),
            Self::Close => PathCommandDescription::new(PathCommandKind::Close, &[]),
        }
    }

    /// Which command this is, without its payload.
    #[must_use]
    pub fn kind(&self) -> PathCommandKind {
        self.describe().kind()
    }
}

/// The **closed** render form: what a rasterizer or a geometry query
/// consumes after [`for_each_segment`] has normalised the authored
/// vocabulary.
///
/// Every curve here is a cubic Bézier, which is the shape every backend
/// already speaks. See the module docs for why this is a separate type
/// from [`PathCommand`] rather than a subset of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathSegment {
    /// Start a new subpath at this point.
    MoveTo(PathPoint),
    /// Straight line from the current point.
    LineTo(PathPoint),
    /// Cubic Bézier from the current point.
    CurveTo {
        /// First control point.
        c1: PathPoint,
        /// Second control point.
        c2: PathPoint,
        /// Endpoint, and the new current point.
        end: PathPoint,
    },
    /// Close the current subpath back to its start.
    Close,
}

/// Normalise an authored command stream into closed [`PathSegment`]s,
/// handing each to `sink` in order.
///
/// Callback rather than `Vec` so the paint path allocates nothing —
/// [`to_segments`] is the collecting convenience.
///
/// # Normalisation rules
///
/// * [`PathCommand::QuadTo`] elevates to the *exact* equivalent cubic
///   (`c1 = p0 + 2/3 (c - p0)`, `c2 = p1 + 2/3 (c - p1)`) — no
///   approximation is involved, only a change of representation.
/// * [`PathCommand::ArcTo`] follows SVG 1.1 F.6.5 (endpoint → centre
///   parameterisation) and F.6.6 (out-of-range radii are scaled up),
///   then emits one cubic per ≤90° of sweep with the standard
///   `k = 4/3·tan(θ/4)` control-point rule. The degenerate cases the
///   specification names are honoured: a zero-length arc is omitted
///   entirely, and a zero radius degrades to a straight line.
/// * A drawing command before any [`PathCommand::MoveTo`] gets an
///   implicit `MoveTo(0, 0)` so a hand-built command stream cannot
///   hand a backend a segment with no current point. [`parse`] rejects
///   that input outright; this arm exists for commands built in code.
pub fn for_each_segment(commands: &[PathCommand], mut sink: impl FnMut(PathSegment)) {
    let mut cur = PathPoint::new(0.0, 0.0);
    let mut start = PathPoint::new(0.0, 0.0);
    let mut opened = false;
    for cmd in commands {
        // Every drawing command needs a current point; an authored
        // stream that never moved gets the origin, once.
        if !opened && !matches!(cmd, PathCommand::MoveTo(_)) {
            sink(PathSegment::MoveTo(cur));
            start = cur;
            opened = true;
        }
        match *cmd {
            PathCommand::MoveTo(p) => {
                sink(PathSegment::MoveTo(p));
                cur = p;
                start = p;
                opened = true;
            }
            PathCommand::LineTo(p) => {
                sink(PathSegment::LineTo(p));
                cur = p;
            }
            PathCommand::QuadTo { c, end } => {
                let (c1, c2) = elevate_quadratic(cur, c, end);
                sink(PathSegment::CurveTo { c1, c2, end });
                cur = end;
            }
            PathCommand::CurveTo { c1, c2, end } => {
                sink(PathSegment::CurveTo { c1, c2, end });
                cur = end;
            }
            PathCommand::ArcTo(arc) => {
                arc_segments(cur, &arc, &mut sink);
                cur = arc.end;
            }
            PathCommand::Close => {
                sink(PathSegment::Close);
                cur = start;
            }
        }
    }
}

/// Collecting form of [`for_each_segment`].
#[must_use]
pub fn to_segments(commands: &[PathCommand]) -> Vec<PathSegment> {
    let mut out = Vec::with_capacity(commands.len());
    for_each_segment(commands, |seg| out.push(seg));
    out
}

/// Degree elevation: the cubic control points of a quadratic.
fn elevate_quadratic(p0: PathPoint, c: PathPoint, p1: PathPoint) -> (PathPoint, PathPoint) {
    const TWO_THIRDS: f32 = 2.0 / 3.0;
    (
        PathPoint::new(
            p0.x + TWO_THIRDS * (c.x - p0.x),
            p0.y + TWO_THIRDS * (c.y - p0.y),
        ),
        PathPoint::new(
            p1.x + TWO_THIRDS * (c.x - p1.x),
            p1.y + TWO_THIRDS * (c.y - p1.y),
        ),
    )
}

/// Narrow an `f64` intermediate back to the `f32` the scene stores.
///
/// The arc conversion runs in `f64` because F.6.5 divides by radii and
/// takes `acos` of a normalised dot product, where `f32` cancellation
/// is visible in the drawn curve; the result is a scene coordinate and
/// scene coordinates are `f32`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "deliberate: f64 is the working precision, f32 is the stored one"
)]
fn narrow(v: f64) -> f32 {
    v as f32
}

/// SVG 1.1 F.6.5 + F.6.6: emit an elliptical arc as ≤90° cubic pieces.
///
/// `clippy::similar_names` fires here on `x1p`/`y1p`, `cxp`/`cyp` and
/// the piece endpoints, and the names stay: they are the
/// specification's own, which is what lets a reader check this against
/// F.6.5 line by line. Renaming them for the lint would make the
/// derivation harder to audit, so the audit is moved to the tests
/// instead — a semicircle, a full circle in two arcs, a rotated
/// ellipse and both flag pairs are each checked against geometry
/// computed independently of this function.
#[allow(
    clippy::similar_names,
    reason = "the specification's variable names, kept so F.6.5 can be checked against this line by line"
)]
fn arc_segments(from: PathPoint, arc: &EllipticalArc, sink: &mut impl FnMut(PathSegment)) {
    let (x1, y1) = (f64::from(from.x), f64::from(from.y));
    let (x2, y2) = (f64::from(arc.end.x), f64::from(arc.end.y));

    // F.6.2: identical endpoints omit the arc entirely.
    if (x1 - x2).abs() < f64::EPSILON && (y1 - y2).abs() < f64::EPSILON {
        return;
    }
    // F.6.2: a zero radius makes the arc a straight line.
    let mut rx = f64::from(arc.rx).abs();
    let mut ry = f64::from(arc.ry).abs();
    if rx < f64::EPSILON || ry < f64::EPSILON {
        sink(PathSegment::LineTo(arc.end));
        return;
    }

    let phi = f64::from(arc.x_rotation).to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();

    // F.6.5.1 — the endpoint difference in the ellipse's own frame.
    let dx2 = (x1 - x2) / 2.0;
    let dy2 = (y1 - y2) / 2.0;
    let x1p = cos_phi.mul_add(dx2, sin_phi * dy2);
    let y1p = cos_phi.mul_add(dy2, -(sin_phi * dx2));

    // F.6.6 — scale radii up until the endpoints are reachable.
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }

    // F.6.5.2 — the centre, in the ellipse's frame.
    let num = (rx * rx * ry * ry) - (rx * rx * y1p * y1p) - (ry * ry * x1p * x1p);
    let den = (rx * rx * y1p * y1p) + (ry * ry * x1p * x1p);
    let mut factor = if den > 0.0 {
        (num / den).max(0.0).sqrt()
    } else {
        0.0
    };
    if arc.large_arc == arc.sweep {
        factor = -factor;
    }
    let cxp = factor * (rx * y1p) / ry;
    let cyp = -factor * (ry * x1p) / rx;

    // F.6.5.3 — back to user space.
    let cx = cos_phi.mul_add(cxp, -(sin_phi * cyp)) + f64::midpoint(x1, x2);
    let cy = sin_phi.mul_add(cxp, cos_phi * cyp) + f64::midpoint(y1, y2);

    // F.6.5.5 / F.6.5.6 — start angle and swept angle.
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let theta1 = uy.atan2(ux);
    let mut delta = {
        let dot = ux.mul_add(vx, uy * vy);
        let cross = ux.mul_add(vy, -(uy * vx));
        cross.atan2(dot)
    };
    if !arc.sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    } else if arc.sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    }

    // One cubic per quarter turn keeps the approximation error below
    // ~2.7e-4 of the radius, which is under a tenth of a pixel for any
    // icon-scale path.
    let pieces = (delta.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pieces is a positive ceil() of a bounded angle ratio"
    )]
    let piece_count = pieces as usize;
    let step = delta / pieces;
    let k = 4.0 / 3.0 * (step / 4.0).tan();

    let point_at = |theta: f64| -> (f64, f64) {
        let (s, c) = theta.sin_cos();
        (
            cos_phi.mul_add(rx * c, -(sin_phi * ry * s)) + cx,
            sin_phi.mul_add(rx * c, cos_phi * ry * s) + cy,
        )
    };
    let tangent_at = |theta: f64| -> (f64, f64) {
        let (s, c) = theta.sin_cos();
        (
            cos_phi.mul_add(-(rx * s), -(sin_phi * ry * c)),
            sin_phi.mul_add(-(rx * s), cos_phi * ry * c),
        )
    };

    for i in 0..piece_count {
        #[allow(
            clippy::cast_precision_loss,
            reason = "piece_count is at most 4 for a full turn"
        )]
        let idx = i as f64;
        let a0 = theta1 + step * idx;
        let a1 = a0 + step;
        let (px0, py0) = point_at(a0);
        let (px1, py1) = point_at(a1);
        let (tx0, ty0) = tangent_at(a0);
        let (tx1, ty1) = tangent_at(a1);
        sink(PathSegment::CurveTo {
            c1: PathPoint::new(narrow(k.mul_add(tx0, px0)), narrow(k.mul_add(ty0, py0))),
            c2: PathPoint::new(narrow(k.mul_add(-tx1, px1)), narrow(k.mul_add(-ty1, py1))),
            // The last piece lands on the authored endpoint exactly
            // rather than on the reconstruction of it.
            end: if i + 1 == piece_count {
                arc.end
            } else {
                PathPoint::new(narrow(px1), narrow(py1))
            },
        });
    }
}

/// Tight axis-aligned bounds of a command stream.
///
/// The reference toolkit answers this with two queries, a tight box
/// and a control-point hull. This is the tight box: cubic extrema are
/// solved rather than sampled,
/// so a curve that bulges past its control points is measured, not
/// approximated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathBounds {
    /// Smallest x reached.
    pub min_x: f32,
    /// Smallest y reached.
    pub min_y: f32,
    /// Largest x reached.
    pub max_x: f32,
    /// Largest y reached.
    pub max_y: f32,
}

impl PathBounds {
    /// Width of the box.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    /// Height of the box.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.max_y - self.min_y
    }
}

/// Tight bounds of `commands`, or `None` when the stream draws nothing.
///
/// An arc is measured through its derived cubics, so its bounds carry
/// the same ~2.7e-4·r approximation [`for_each_segment`] documents.
#[must_use]
pub fn bounds(commands: &[PathCommand]) -> Option<PathBounds> {
    let mut acc: Option<PathBounds> = None;
    let mut cur = PathPoint::new(0.0, 0.0);
    let include = |p: PathPoint, acc: &mut Option<PathBounds>| match acc {
        Some(b) => {
            b.min_x = b.min_x.min(p.x);
            b.min_y = b.min_y.min(p.y);
            b.max_x = b.max_x.max(p.x);
            b.max_y = b.max_y.max(p.y);
        }
        None => {
            *acc = Some(PathBounds {
                min_x: p.x,
                min_y: p.y,
                max_x: p.x,
                max_y: p.y,
            });
        }
    };
    for_each_segment(commands, |seg| match seg {
        PathSegment::MoveTo(p) | PathSegment::LineTo(p) => {
            include(p, &mut acc);
            cur = p;
        }
        PathSegment::CurveTo { c1, c2, end } => {
            include(cur, &mut acc);
            include(end, &mut acc);
            for t in cubic_extrema(cur.x, c1.x, c2.x, end.x) {
                include(
                    PathPoint::new(cubic_at(cur.x, c1.x, c2.x, end.x, t), cur.y),
                    &mut acc,
                );
            }
            for t in cubic_extrema(cur.y, c1.y, c2.y, end.y) {
                include(
                    PathPoint::new(cur.x, cubic_at(cur.y, c1.y, c2.y, end.y, t)),
                    &mut acc,
                );
            }
            cur = end;
        }
        PathSegment::Close => {}
    });
    acc
}

/// Value of a 1-D cubic Bézier at `t`.
fn cubic_at(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let mt = 1.0 - t;
    (mt * mt * mt).mul_add(
        p0,
        (3.0 * mt * mt * t).mul_add(p1, (3.0 * mt * t * t).mul_add(p2, t * t * t * p3)),
    )
}

/// Parameters in `(0, 1)` where a 1-D cubic Bézier turns.
fn cubic_extrema(p0: f32, p1: f32, p2: f32, p3: f32) -> Vec<f32> {
    // B'(t) = 3[ a t^2 + b t + c ] with the coefficients below.
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0f32.mul_add(p0, -(4.0 * p1)) + 2.0 * p2;
    let c = p1 - p0;
    let mut out = Vec::new();
    let mut push = |t: f32| {
        if t > 0.0 && t < 1.0 {
            out.push(t);
        }
    };
    if a.abs() < 1e-6 {
        if b.abs() > 1e-6 {
            push(-c / b);
        }
        return out;
    }
    let disc = b.mul_add(b, -(4.0 * a * c));
    if disc < 0.0 {
        return out;
    }
    let root = disc.sqrt();
    push((-b + root) / (2.0 * a));
    push((-b - root) / (2.0 * a));
    out
}

/// Scale about the origin then translate — the transform an imported
/// icon needs to sit in a widget's rect.
///
/// Uniform scale only, and that is a design boundary rather than a
/// simplification: under a non-uniform scale a rotated
/// [`PathCommand::ArcTo`] is no longer an ellipse with the same axis
/// ratio, so honouring one would mean re-solving the arc's radii and
/// rotation — SVG itself declines and carries a transform matrix
/// instead. [`fit`] is the ergonomic caller.
#[must_use]
pub fn scale_translate(commands: &[PathCommand], scale: f32, dx: f32, dy: f32) -> Vec<PathCommand> {
    let map = |p: PathPoint| PathPoint::new(p.x.mul_add(scale, dx), p.y.mul_add(scale, dy));
    commands
        .iter()
        .map(|cmd| match *cmd {
            PathCommand::MoveTo(p) => PathCommand::MoveTo(map(p)),
            PathCommand::LineTo(p) => PathCommand::LineTo(map(p)),
            PathCommand::QuadTo { c, end } => PathCommand::QuadTo {
                c: map(c),
                end: map(end),
            },
            PathCommand::CurveTo { c1, c2, end } => PathCommand::CurveTo {
                c1: map(c1),
                c2: map(c2),
                end: map(end),
            },
            PathCommand::ArcTo(arc) => PathCommand::ArcTo(EllipticalArc::new(
                arc.rx * scale,
                arc.ry * scale,
                arc.x_rotation,
                arc.large_arc,
                arc.sweep,
                map(arc.end),
            )),
            PathCommand::Close => PathCommand::Close,
        })
        .collect()
}

/// Scale `commands` to fit a `w` × `h` box, preserving aspect ratio and
/// centring the remainder.
///
/// This is the whole of importing an icon: the reference toolkit
/// applies a `viewBox` only through its whole-*document* SVG renderer,
/// and gives a bare path nothing — a caller there composes a
/// bounding-box query with a transform by hand at every call site.
///
/// Returns `None` when the path draws nothing, or when either extent is
/// zero — a degenerate box has no scale that fits it, and inventing one
/// would silently misplace the result.
#[must_use]
pub fn fit(commands: &[PathCommand], w: f32, h: f32) -> Option<Vec<PathCommand>> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let b = bounds(commands)?;
    let (bw, bh) = (b.width(), b.height());
    if bw <= 0.0 && bh <= 0.0 {
        return None;
    }
    let sx = if bw > 0.0 { w / bw } else { f32::INFINITY };
    let sy = if bh > 0.0 { h / bh } else { f32::INFINITY };
    let scale = sx.min(sy);
    if !scale.is_finite() {
        return None;
    }
    let dx = scale.mul_add(-b.min_x, (w - bw * scale) / 2.0);
    let dy = scale.mul_add(-b.min_y, (h - bh * scale) / 2.0);
    Some(scale_translate(commands, scale, dx, dy))
}

/// Render `commands` back to SVG path data.
///
/// Derived from [`PathCommand::describe`], so it cannot omit a command
/// the vocabulary gained, and cannot disagree with the RPC wire about
/// what one carries. Numbers use Rust's shortest round-tripping `f32`
/// formatting, so `parse(write(c)) == c` for every command stream
/// [`parse`] can produce.
///
/// The reference toolkit has no inverse at all: its painter path can
/// be walked element by element, but nothing turns it back into path
/// data, and the arcs and quadratics are gone by then anyway.
#[must_use]
pub fn write(commands: &[PathCommand]) -> String {
    let mut out = String::new();
    for cmd in commands {
        let desc = cmd.describe();
        if !out.is_empty() {
            out.push(' ');
        }
        out.push(desc.kind().svg_letter());
        for arg in desc.args() {
            out.push(' ');
            match arg.value {
                PathArgValue::Point(p) => {
                    let _ = write!(out, "{},{}", p.x, p.y);
                }
                PathArgValue::Scalar(v) => {
                    let _ = write!(out, "{v}");
                }
                PathArgValue::Flag(f) => out.push(if f { '1' } else { '0' }),
            }
        }
    }
    out
}

/// What went wrong, and where.
///
/// The reference toolkit's own path-data parser answers a bare empty
/// optional — one bit, no position, no cause. It is also unreachable:
/// both of its declarations sit in private headers, so an application
/// cannot call either and writes its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathDataError {
    /// Byte offset into the input where the problem is.
    pub at: usize,
    /// The command letter being parsed, when one had been read.
    pub command: Option<char>,
    /// What the problem is.
    pub kind: PathDataErrorKind,
}

/// The distinct ways SVG path data can be malformed.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathDataErrorKind {
    /// Path data must begin with a `moveto`.
    ///
    /// The reference invents one: a line-to on an empty painter path
    /// runs the lazy initialiser, which seeds a move-to at the origin,
    /// so `"L10 10"` silently draws from `(0, 0)` there.
    MissingInitialMoveTo,
    /// A command letter was required and this byte was found.
    ExpectedCommand(char),
    /// A number was required and was not there.
    ExpectedNumber,
    /// An arc flag must be exactly `0` or `1`; this byte was found.
    ExpectedFlag(char),
    /// A number parsed to infinity or NaN.
    ///
    /// The reference accepts it and then drops the segment: its
    /// line-to and cubic builders bail out on a coordinate-validity
    /// guard, warning only in debug builds.
    NotFinite,
    /// The data ended in the middle of a command's arguments.
    ///
    /// The reference ignores this: a trailing letter with no arguments
    /// leaves its argument array empty, the `while (count > 0)` body
    /// never runs, and the truncated command vanishes without an error.
    UnexpectedEnd,
}

impl core::fmt::Display for PathDataError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "path data byte {}", self.at)?;
        if let Some(c) = self.command {
            write!(f, " (in command '{c}')")?;
        }
        f.write_str(": ")?;
        match self.kind {
            PathDataErrorKind::MissingInitialMoveTo => {
                f.write_str("path data must begin with a moveto")
            }
            PathDataErrorKind::ExpectedCommand(c) => {
                write!(f, "expected a command letter, found '{c}'")
            }
            PathDataErrorKind::ExpectedNumber => f.write_str("expected a number"),
            PathDataErrorKind::ExpectedFlag(c) => {
                write!(f, "expected an arc flag 0 or 1, found '{c}'")
            }
            PathDataErrorKind::NotFinite => f.write_str("number is not finite"),
            PathDataErrorKind::UnexpectedEnd => {
                f.write_str("path data ended inside a command's arguments")
            }
        }
    }
}

impl std::error::Error for PathDataError {}

/// Parse SVG path data into the authored vocabulary.
///
/// Accepts the full `d` grammar: absolute and relative forms of
/// `M L H V C S Q T A Z`, implicit repetition (with a `moveto`'s
/// repeats becoming `lineto`, per the specification), `comma_wsp`
/// separation, exponent notation, and the compact arc-flag spelling
/// where `a1 1 0 011 1` packs two flags and a coordinate into `011`.
///
/// Empty or whitespace-only data parses to an empty command stream —
/// an SVG path with empty `d` is legal and simply renders nothing.
/// (The reference reports failure for it.)
///
/// # Errors
///
/// [`PathDataError`] naming the byte offset, the command being parsed
/// and the cause. See [`PathDataErrorKind`] for the cases, three of
/// which the reference parser accepts silently.
pub fn parse(d: &str) -> Result<Vec<PathCommand>, PathDataError> {
    Parser::new(d).run()
}

struct Parser<'a> {
    src: &'a [u8],
    at: usize,
    cmd: Option<char>,
    out: Vec<PathCommand>,
    cur: PathPoint,
    start: PathPoint,
    last_cubic_c2: Option<PathPoint>,
    last_quad_c: Option<PathPoint>,
}

impl<'a> Parser<'a> {
    fn new(d: &'a str) -> Self {
        Self {
            src: d.as_bytes(),
            at: 0,
            cmd: None,
            out: Vec::new(),
            cur: PathPoint::new(0.0, 0.0),
            start: PathPoint::new(0.0, 0.0),
            last_cubic_c2: None,
            last_quad_c: None,
        }
    }

    fn err(&self, at: usize, kind: PathDataErrorKind) -> PathDataError {
        PathDataError {
            at,
            command: self.cmd,
            kind,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.at).copied()
    }

    fn skip_wsp(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n' | 0x0C)) {
            self.at += 1;
        }
    }

    /// Consume `comma_wsp`. Reports whether a comma was seen, because a
    /// comma promises another argument.
    fn skip_comma_wsp(&mut self) -> bool {
        self.skip_wsp();
        let comma = self.peek() == Some(b',');
        if comma {
            self.at += 1;
            self.skip_wsp();
        }
        comma
    }

    fn starts_number(b: u8) -> bool {
        b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.')
    }

    fn number(&mut self) -> Result<f32, PathDataError> {
        self.skip_wsp();
        let begin = self.at;
        if let Some(b) = self.peek() {
            if matches!(b, b'+' | b'-') {
                self.at += 1;
            }
        }
        let int_digits = self.digits();
        let mut frac_digits = 0;
        if self.peek() == Some(b'.') {
            self.at += 1;
            frac_digits = self.digits();
        }
        if int_digits == 0 && frac_digits == 0 {
            self.at = begin;
            return Err(self.err(
                begin,
                if begin >= self.src.len() {
                    PathDataErrorKind::UnexpectedEnd
                } else {
                    PathDataErrorKind::ExpectedNumber
                },
            ));
        }
        // An exponent counts only when it actually has digits, so
        // `1e` parses as `1` followed by a stray `e` rather than
        // swallowing the letter.
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let mark = self.at;
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                self.at = mark;
            }
        }
        let text = core::str::from_utf8(&self.src[begin..self.at])
            .map_err(|_| self.err(begin, PathDataErrorKind::ExpectedNumber))?;
        let v: f32 = text
            .parse()
            .map_err(|_| self.err(begin, PathDataErrorKind::ExpectedNumber))?;
        if v.is_finite() {
            Ok(v)
        } else {
            Err(self.err(begin, PathDataErrorKind::NotFinite))
        }
    }

    fn digits(&mut self) -> usize {
        let begin = self.at;
        while matches!(self.peek(), Some(b) if b.is_ascii_digit()) {
            self.at += 1;
        }
        self.at - begin
    }

    /// An arc flag: exactly one `0` or `1`, never a general number.
    ///
    /// This is what lets `a1 1 0 011 1` mean *large-arc 0, sweep 1,
    /// endpoint (1, 1)* — reading it as a number would take `011`
    /// whole and shift every following argument.
    fn flag(&mut self) -> Result<bool, PathDataError> {
        self.skip_wsp();
        match self.peek() {
            Some(b'0') => {
                self.at += 1;
                Ok(false)
            }
            Some(b'1') => {
                self.at += 1;
                Ok(true)
            }
            Some(b) => Err(self.err(self.at, PathDataErrorKind::ExpectedFlag(char::from(b)))),
            None => Err(self.err(self.at, PathDataErrorKind::UnexpectedEnd)),
        }
    }

    /// Read one argument after the first of a tuple: a separator may
    /// appear, and if a comma did, a number must follow.
    fn next_number(&mut self) -> Result<f32, PathDataError> {
        let had_comma = self.skip_comma_wsp();
        if had_comma && self.peek().is_none() {
            return Err(self.err(self.at, PathDataErrorKind::UnexpectedEnd));
        }
        self.number()
    }

    fn next_flag(&mut self) -> Result<bool, PathDataError> {
        self.skip_comma_wsp();
        self.flag()
    }

    fn point(&mut self, relative: bool, first: bool) -> Result<PathPoint, PathDataError> {
        let x = if first {
            self.number()?
        } else {
            self.next_number()?
        };
        let y = self.next_number()?;
        Ok(if relative {
            PathPoint::new(self.cur.x + x, self.cur.y + y)
        } else {
            PathPoint::new(x, y)
        })
    }

    fn run(mut self) -> Result<Vec<PathCommand>, PathDataError> {
        self.skip_wsp();
        if self.peek().is_none() {
            return Ok(self.out);
        }
        if !matches!(self.peek(), Some(b'M' | b'm')) {
            return Err(self.err(self.at, PathDataErrorKind::MissingInitialMoveTo));
        }
        while self.peek().is_some() {
            let letter_at = self.at;
            let byte = self.src[self.at];
            let letter = char::from(byte);
            if !letter.is_ascii_alphabetic() {
                return Err(self.err(letter_at, PathDataErrorKind::ExpectedCommand(letter)));
            }
            self.at += 1;
            self.cmd = Some(letter);
            self.command(letter, letter_at)?;
            self.skip_comma_wsp();
        }
        Ok(self.out)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one arm per SVG command letter; splitting the table hides it"
    )]
    fn command(&mut self, letter: char, letter_at: usize) -> Result<(), PathDataError> {
        let rel = letter.is_ascii_lowercase();
        let mut first = true;
        loop {
            match letter.to_ascii_uppercase() {
                'M' => {
                    let p = self.point(rel, first)?;
                    if first {
                        self.out.push(PathCommand::MoveTo(p));
                        self.start = p;
                    } else {
                        // A moveto's implicit repetitions are linetos.
                        self.out.push(PathCommand::LineTo(p));
                    }
                    self.cur = p;
                    self.smooth_none();
                }
                'L' => {
                    let p = self.point(rel, first)?;
                    self.out.push(PathCommand::LineTo(p));
                    self.cur = p;
                    self.smooth_none();
                }
                'H' => {
                    let v = if first {
                        self.number()?
                    } else {
                        self.next_number()?
                    };
                    let p = PathPoint::new(if rel { self.cur.x + v } else { v }, self.cur.y);
                    self.out.push(PathCommand::LineTo(p));
                    self.cur = p;
                    self.smooth_none();
                }
                'V' => {
                    let v = if first {
                        self.number()?
                    } else {
                        self.next_number()?
                    };
                    let p = PathPoint::new(self.cur.x, if rel { self.cur.y + v } else { v });
                    self.out.push(PathCommand::LineTo(p));
                    self.cur = p;
                    self.smooth_none();
                }
                'C' => {
                    let c1 = self.point(rel, first)?;
                    let c2 = self.point(rel, false)?;
                    let end = self.point(rel, false)?;
                    self.out.push(PathCommand::CurveTo { c1, c2, end });
                    self.cur = end;
                    self.last_cubic_c2 = Some(c2);
                    self.last_quad_c = None;
                }
                'S' => {
                    let c1 = self.reflect(self.last_cubic_c2);
                    let c2 = self.point(rel, first)?;
                    let end = self.point(rel, false)?;
                    self.out.push(PathCommand::CurveTo { c1, c2, end });
                    self.cur = end;
                    self.last_cubic_c2 = Some(c2);
                    self.last_quad_c = None;
                }
                'Q' => {
                    let c = self.point(rel, first)?;
                    let end = self.point(rel, false)?;
                    self.out.push(PathCommand::QuadTo { c, end });
                    self.cur = end;
                    self.last_quad_c = Some(c);
                    self.last_cubic_c2 = None;
                }
                'T' => {
                    let c = self.reflect(self.last_quad_c);
                    let end = self.point(rel, first)?;
                    self.out.push(PathCommand::QuadTo { c, end });
                    self.cur = end;
                    self.last_quad_c = Some(c);
                    self.last_cubic_c2 = None;
                }
                'A' => {
                    let rx = if first {
                        self.number()?
                    } else {
                        self.next_number()?
                    };
                    let ry = self.next_number()?;
                    let rot = self.next_number()?;
                    let large = self.next_flag()?;
                    let sweep = self.next_flag()?;
                    let end = self.point(rel, false)?;
                    self.out.push(PathCommand::ArcTo(EllipticalArc::new(
                        rx, ry, rot, large, sweep, end,
                    )));
                    self.cur = end;
                    self.smooth_none();
                }
                'Z' => {
                    if !first {
                        return Ok(());
                    }
                    self.out.push(PathCommand::Close);
                    self.cur = self.start;
                    self.smooth_none();
                    return Ok(());
                }
                _ => {
                    return Err(self.err(letter_at, PathDataErrorKind::ExpectedCommand(letter)));
                }
            }
            first = false;
            // Implicit repetition: another argument tuple with no
            // letter of its own.
            let mark = self.at;
            let had_comma = self.skip_comma_wsp();
            match self.peek() {
                Some(b) if Self::starts_number(b) => {}
                Some(_) if had_comma => {
                    return Err(self.err(self.at, PathDataErrorKind::ExpectedNumber));
                }
                None if had_comma => {
                    return Err(self.err(self.at, PathDataErrorKind::UnexpectedEnd));
                }
                _ => {
                    self.at = mark;
                    return Ok(());
                }
            }
        }
    }

    fn smooth_none(&mut self) {
        self.last_cubic_c2 = None;
        self.last_quad_c = None;
    }

    /// The reflected control point a smooth command implies, which is
    /// the current point when the previous command was not of the
    /// matching family.
    fn reflect(&self, prev: Option<PathPoint>) -> PathPoint {
        prev.map_or(self.cur, |c| {
            PathPoint::new(
                2.0f32.mul_add(self.cur.x, -c.x),
                2.0f32.mul_add(self.cur.y, -c.y),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> PathPoint {
        PathPoint::new(x, y)
    }

    /// Evaluate a cubic Bézier at `t`, independently of anything the
    /// module does with cubics.
    fn cubic(p0: PathPoint, c1: PathPoint, c2: PathPoint, p1: PathPoint, t: f32) -> PathPoint {
        let mt = 1.0 - t;
        let w0 = mt * mt * mt;
        let w1 = 3.0 * mt * mt * t;
        let w2 = 3.0 * mt * t * t;
        let w3 = t * t * t;
        PathPoint::new(
            w0 * p0.x + w1 * c1.x + w2 * c2.x + w3 * p1.x,
            w0 * p0.y + w1 * c1.y + w2 * c2.y + w3 * p1.y,
        )
    }

    /// Walk the derived segments, sampling every curve, so a geometric
    /// claim can be checked against points rather than control points.
    fn sample(commands: &[PathCommand], per_curve: u16) -> Vec<PathPoint> {
        let mut out = Vec::new();
        let mut cur = p(0.0, 0.0);
        let mut start = p(0.0, 0.0);
        for_each_segment(commands, |seg| match seg {
            PathSegment::MoveTo(q) => {
                out.push(q);
                cur = q;
                start = q;
            }
            PathSegment::LineTo(q) => {
                out.push(q);
                cur = q;
            }
            PathSegment::CurveTo { c1, c2, end } => {
                for i in 1..=per_curve {
                    let t = f32::from(i) / f32::from(per_curve);
                    out.push(cubic(cur, c1, c2, end, t));
                }
                cur = end;
            }
            PathSegment::Close => {
                out.push(start);
                cur = start;
            }
        });
        out
    }

    fn near(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    // ---------- the vocabulary survives ----------

    #[test]
    fn a_quadratic_parses_as_a_quadratic_and_writes_back_as_one() {
        let cmds = parse("M 0,0 Q 10,0 10,10").expect("parses");
        assert_eq!(
            cmds,
            vec![
                PathCommand::MoveTo(p(0.0, 0.0)),
                PathCommand::QuadTo {
                    c: p(10.0, 0.0),
                    end: p(10.0, 10.0),
                },
            ]
        );
        // The reference would hold two cubics here and could not
        // answer either of these.
        assert_eq!(cmds[1].kind(), PathCommandKind::QuadTo);
        assert!(write(&cmds).contains('Q'), "{}", write(&cmds));
    }

    #[test]
    fn an_arc_parses_as_an_arc_and_keeps_its_flags() {
        let cmds = parse("M 0,0 A 50,25 30 1 0 100,0").expect("parses");
        let PathCommand::ArcTo(arc) = cmds[1] else {
            panic!("expected an arc, got {:?}", cmds[1]);
        };
        assert!(near(arc.rx, 50.0, 0.0));
        assert!(near(arc.ry, 25.0, 0.0));
        assert!(near(arc.x_rotation, 30.0, 0.0));
        assert!(arc.large_arc);
        assert!(!arc.sweep);
        assert_eq!(arc.end, p(100.0, 0.0));
    }

    #[test]
    fn every_kind_describes_its_arguments_by_name() {
        let names = |c: PathCommand| -> Vec<&'static str> {
            c.describe().args().iter().map(|a| a.name).collect()
        };
        assert_eq!(names(PathCommand::MoveTo(p(0.0, 0.0))), vec!["point"]);
        assert_eq!(names(PathCommand::LineTo(p(0.0, 0.0))), vec!["point"]);
        assert_eq!(
            names(PathCommand::QuadTo {
                c: p(0.0, 0.0),
                end: p(1.0, 1.0)
            }),
            vec!["c", "end"]
        );
        assert_eq!(
            names(PathCommand::CurveTo {
                c1: p(0.0, 0.0),
                c2: p(1.0, 1.0),
                end: p(2.0, 2.0)
            }),
            vec!["c1", "c2", "end"]
        );
        assert_eq!(
            names(PathCommand::ArcTo(EllipticalArc::new(
                1.0,
                2.0,
                3.0,
                true,
                false,
                p(4.0, 5.0)
            ))),
            vec!["rx", "ry", "x_rotation", "large_arc", "sweep", "end"]
        );
        assert_eq!(names(PathCommand::Close), Vec::<&'static str>::new());
    }

    #[test]
    fn no_command_carries_more_arguments_than_the_description_holds() {
        // MAX_PATH_ARGS is a stack-array bound, so a command that
        // outgrew it would be silently truncated by
        // `PathCommandDescription::new`. Every kind is checked, and the
        // arc is the one at the bound.
        for cmd in [
            PathCommand::MoveTo(p(0.0, 0.0)),
            PathCommand::LineTo(p(0.0, 0.0)),
            PathCommand::QuadTo {
                c: p(0.0, 0.0),
                end: p(1.0, 1.0),
            },
            PathCommand::CurveTo {
                c1: p(0.0, 0.0),
                c2: p(1.0, 1.0),
                end: p(2.0, 2.0),
            },
            PathCommand::ArcTo(EllipticalArc::new(1.0, 2.0, 3.0, true, false, p(4.0, 5.0))),
            PathCommand::Close,
        ] {
            assert!(cmd.describe().args().len() <= MAX_PATH_ARGS, "{cmd:?}");
        }
        assert_eq!(
            PathCommand::ArcTo(EllipticalArc::new(1.0, 2.0, 3.0, true, false, p(4.0, 5.0)))
                .describe()
                .args()
                .len(),
            MAX_PATH_ARGS,
            "the arc is what sets the bound; if this shrinks the bound is stale"
        );
    }

    #[test]
    fn the_kind_census_names_every_command_and_agrees_with_it() {
        // `ALL` is a hand-written list, and a hand-written list of a
        // growing enum is exactly the thing that goes stale. What keeps
        // it honest is the fixture: its match is exhaustive, so a new
        // command must appear there, and this test then demands the
        // census name it too.
        for kind in PathCommandKind::ALL {
            let cmd = crate::test_fixtures::path_command_of_kind(kind);
            assert_eq!(
                cmd.kind(),
                kind,
                "fixture for {kind:?} is a different command"
            );
            assert_eq!(cmd.describe().kind(), kind);
        }
        let mut seen: Vec<PathCommandKind> = PathCommandKind::ALL.to_vec();
        seen.sort_unstable_by_key(|k| k.name());
        seen.dedup();
        assert_eq!(seen.len(), PathCommandKind::ALL.len(), "ALL repeats a kind");
    }

    /// R1630 — **no two commands can share a kind**, which is what the wire's
    /// `type` field needs and what R1623 could not promise.
    ///
    /// The map `PathCommand -> PathCommandKind` is TOTAL by the compiler:
    /// `kind()` matches exhaustively inside this crate, so a new arm cannot
    /// avoid answering. The test above proves it SURJECTIVE: every kind has a
    /// command that answers with it, read off a fixture whose own match is
    /// exhaustive. A total, surjective map between finite sets of EQUAL SIZE
    /// is injective — so the only thing left to check is the size, and that is
    /// the one fact a hand-written census could not state about a
    /// `#[non_exhaustive]` enum whose arms cannot be enumerated from outside.
    /// `#[derive(VariantCensus)]` reads it off the definition.
    ///
    /// An arm added here that reuses a kind makes `PathCommand::ARMS` one
    /// larger than `PathCommandKind::ARMS`, and this fails. An arm added with
    /// its own kind moves both, and the surjectivity test above demands a
    /// fixture for it.
    #[test]
    fn r1630_the_kind_of_a_command_is_its_own() {
        assert_eq!(
            crate::scene::PathCommand::ARMS,
            PathCommandKind::ARMS,
            "one command, one kind: a reused kind would give two commands the \
             same `type` on the wire, and both would compile"
        );
        // ...and the hand-written vocabulary list is the same size, which the
        // derive also asserts at compile time. Restated here so a reader of
        // this argument can see all three cardinalities in one place.
        assert_eq!(PathCommandKind::ALL.len(), PathCommandKind::ARMS);
    }

    #[test]
    fn kind_names_and_letters_are_unique() {
        let kinds = [
            PathCommandKind::MoveTo,
            PathCommandKind::LineTo,
            PathCommandKind::QuadTo,
            PathCommandKind::CurveTo,
            PathCommandKind::ArcTo,
            PathCommandKind::Close,
        ];
        let mut names: Vec<&str> = kinds.iter().map(|k| k.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), kinds.len());
        let mut letters: Vec<char> = kinds.iter().map(|k| k.svg_letter()).collect();
        letters.sort_unstable();
        letters.dedup();
        assert_eq!(letters.len(), kinds.len());
    }

    // ---------- the derived form ----------

    #[test]
    fn a_quadratic_elevates_to_the_exact_cubic() {
        let (p0, c, p1) = (p(0.0, 0.0), p(30.0, -40.0), p(60.0, 0.0));
        let (c1, c2) = elevate_quadratic(p0, c, p1);
        for i in 0..=20u8 {
            let t = f32::from(i) / 20.0;
            let mt = 1.0 - t;
            let quad = PathPoint::new(
                mt * mt * p0.x + 2.0 * mt * t * c.x + t * t * p1.x,
                mt * mt * p0.y + 2.0 * mt * t * c.y + t * t * p1.y,
            );
            let cube = cubic(p0, c1, c2, p1, t);
            assert!(near(quad.x, cube.x, 1e-3), "t={t} x {quad:?} {cube:?}");
            assert!(near(quad.y, cube.y, 1e-3), "t={t} y {quad:?} {cube:?}");
        }
    }

    #[test]
    fn a_semicircle_arc_stays_on_its_circle() {
        // (0,0) -> (100,0) with r = 50 has exactly one circle: centre
        // (50, 0). Every sampled point must be 50 away from it.
        let cmds = parse("M 0,0 A 50,50 0 0 1 100,0").expect("parses");
        let pts = sample(&cmds, 24);
        assert!(pts.len() > 24);
        for q in &pts {
            let d = ((q.x - 50.0).powi(2) + q.y.powi(2)).sqrt();
            assert!(near(d, 50.0, 0.05), "point {q:?} is {d} from the centre");
        }
        let last = pts.last().copied().expect("non-empty");
        assert!(
            near(last.x, 100.0, 1e-4) && near(last.y, 0.0, 1e-4),
            "{last:?}"
        );
    }

    #[test]
    fn the_sweep_flag_picks_the_other_half() {
        let up = sample(&parse("M 0,0 A 50,50 0 0 1 100,0").expect("parses"), 8);
        let down = sample(&parse("M 0,0 A 50,50 0 0 0 100,0").expect("parses"), 8);
        let mid_up = up[up.len() / 2];
        let mid_down = down[down.len() / 2];
        assert!(mid_up.y * mid_down.y < 0.0, "{mid_up:?} vs {mid_down:?}");
    }

    #[test]
    fn the_large_arc_flag_picks_the_long_way_round() {
        // Same endpoints, radius large enough that both arcs exist.
        let small = sample(&parse("M 0,0 A 50,50 0 0 1 60,0").expect("parses"), 16);
        let large = sample(&parse("M 0,0 A 50,50 0 1 1 60,0").expect("parses"), 16);
        let extent = |pts: &[PathPoint]| pts.iter().fold(0.0f32, |acc, q| acc.max((q.y).abs()));
        assert!(
            extent(&large) > extent(&small) * 2.0,
            "large {} small {}",
            extent(&large),
            extent(&small)
        );
    }

    #[test]
    fn the_four_flag_pairs_land_on_the_two_circles_the_sign_rule_chooses() {
        // FOUND BY A COUNTERFACTUAL (R1623 CF-2): the two tests above
        // exercise sweep 0/1 at large_arc 0, and large_arc 0/1 at sweep
        // 1 — so the sign rule in F.6.5.2 could be replaced by
        // `if arc.large_arc` and every one of them still passed. The
        // rule is a statement about all FOUR pairs, and this is the
        // property that says so.
        //
        // Endpoints (0,0)–(60,0) with r = 50 admit exactly two circles,
        // centred (30, ±40). Those centres are computed here from the
        // chord — half-chord 30, so the centre offset is sqrt(50² − 30²)
        // = 40 — rather than read back out of the code under test.
        const CENTRES: [(f32, f32); 2] = [(30.0, 40.0), (30.0, -40.0)];
        let circle_of = |large: u8, sweep: u8| -> usize {
            let d = format!("M 0,0 A 50,50 0 {large} {sweep} 60,0");
            let pts = sample(&parse(&d).expect("parses"), 16);
            let on = |c: (f32, f32)| {
                pts.iter().all(|q| {
                    near(
                        ((q.x - c.0).powi(2) + (q.y - c.1).powi(2)).sqrt(),
                        50.0,
                        0.05,
                    )
                })
            };
            match (on(CENTRES[0]), on(CENTRES[1])) {
                (true, false) => 0,
                (false, true) => 1,
                other => panic!("arc {large}{sweep} is on neither circle alone: {other:?}"),
            }
        };
        // F.6.5.2 negates the centre offset exactly when the two flags
        // AGREE. So the agreeing pairs share one circle and the
        // differing pairs share the other, and swapping the rule for
        // any function of one flag alone regroups them. The reference
        // toolkit's own conversion carries the identical rule
        // (`if (sweep_flag == large_arc_flag) sfactor = -sfactor`),
        // which is what makes this an independent statement rather
        // than a restatement of the code above.
        assert_eq!(
            circle_of(0, 0),
            circle_of(1, 1),
            "flags that agree share a circle",
        );
        assert_eq!(
            circle_of(0, 1),
            circle_of(1, 0),
            "flags that differ share the other",
        );
        assert_ne!(
            circle_of(0, 0),
            circle_of(0, 1),
            "and the two circles are not the same circle",
        );
    }

    #[test]
    fn the_large_arc_flag_picks_the_long_way_at_either_sweep() {
        // The companion gap CF-2 exposed: `the_large_arc_flag_picks_the_
        // long_way_round` only ever asked at sweep = 1.
        for sweep in [0u8, 1u8] {
            let extent = |large: u8| {
                let d = format!("M 0,0 A 50,50 0 {large} {sweep} 60,0");
                sample(&parse(&d).expect("parses"), 16)
                    .iter()
                    .fold(0.0f32, |acc, q| acc.max(q.y.abs()))
            };
            assert!(
                extent(1) > extent(0) * 2.0,
                "at sweep {sweep}: large {} vs small {}",
                extent(1),
                extent(0),
            );
        }
    }

    #[test]
    fn a_full_circle_drawn_as_two_arcs_closes_on_itself() {
        let cmds = parse("M 100,50 A 50,50 0 1 0 0,50 A 50,50 0 1 0 100,50").expect("parses");
        for q in sample(&cmds, 24) {
            let d = ((q.x - 50.0).powi(2) + (q.y - 50.0).powi(2)).sqrt();
            assert!(near(d, 50.0, 0.05), "point {q:?} is {d} from the centre");
        }
    }

    #[test]
    fn a_rotated_ellipse_rides_its_own_axes() {
        // rx = 40, ry = 10, rotated a quarter turn: the sampled points
        // must satisfy the rotated ellipse equation, which for 90° is
        // (dy/40)^2 + (dx/10)^2 = 1 about the centre.
        let cmds = parse("M 0,0 A 40,10 90 0 1 0,80").expect("parses");
        for q in sample(&cmds, 24) {
            let (dx, dy) = (q.x, q.y - 40.0);
            let e = (dy / 40.0).powi(2) + (dx / 10.0).powi(2);
            assert!(near(e, 1.0, 0.01), "point {q:?} gives {e}");
        }
    }

    #[test]
    fn out_of_range_radii_are_scaled_up_until_the_endpoint_is_reachable() {
        // F.6.6: r = 1 cannot span 100 units, so both radii scale by
        // 50 and the arc becomes the semicircle of radius 50.
        let cmds = parse("M 0,0 A 1,1 0 0 1 100,0").expect("parses");
        for q in sample(&cmds, 16) {
            let d = ((q.x - 50.0).powi(2) + q.y.powi(2)).sqrt();
            assert!(near(d, 50.0, 0.05), "point {q:?} is {d} from the centre");
        }
    }

    #[test]
    fn a_zero_radius_arc_degrades_to_a_line() {
        let segs = to_segments(&parse("M 0,0 A 0,50 0 0 1 100,0").expect("parses"));
        assert_eq!(
            segs,
            vec![
                PathSegment::MoveTo(p(0.0, 0.0)),
                PathSegment::LineTo(p(100.0, 0.0))
            ]
        );
    }

    #[test]
    fn an_arc_that_ends_where_it_began_is_omitted() {
        let segs = to_segments(&parse("M 10,10 A 50,50 0 1 1 10,10").expect("parses"));
        assert_eq!(segs, vec![PathSegment::MoveTo(p(10.0, 10.0))]);
    }

    #[test]
    fn a_drawing_command_before_any_move_starts_at_the_origin() {
        // parse() rejects this, so the arm exists for hand-built
        // streams; without it a backend would receive a lineTo with no
        // current point.
        let segs = to_segments(&[PathCommand::LineTo(p(5.0, 5.0))]);
        assert_eq!(
            segs,
            vec![
                PathSegment::MoveTo(p(0.0, 0.0)),
                PathSegment::LineTo(p(5.0, 5.0))
            ]
        );
    }

    #[test]
    fn close_returns_the_current_point_to_the_subpath_start() {
        // The quadratic after the close must elevate against (10,10),
        // not against (20,20) — which is only observable through the
        // derived control points.
        let segs = to_segments(&[
            PathCommand::MoveTo(p(10.0, 10.0)),
            PathCommand::LineTo(p(20.0, 20.0)),
            PathCommand::Close,
            PathCommand::QuadTo {
                c: p(10.0, 40.0),
                end: p(40.0, 40.0),
            },
        ]);
        let PathSegment::CurveTo { c1, .. } = segs[3] else {
            panic!("expected a curve, got {:?}", segs[3]);
        };
        assert!(near(c1.x, 10.0, 1e-4) && near(c1.y, 30.0, 1e-4), "{c1:?}");
    }

    // ---------- the grammar ----------

    #[test]
    fn relative_commands_resolve_against_the_current_point() {
        let cmds = parse("M 10,10 l 5,5 l -3,0").expect("parses");
        assert_eq!(
            cmds,
            vec![
                PathCommand::MoveTo(p(10.0, 10.0)),
                PathCommand::LineTo(p(15.0, 15.0)),
                PathCommand::LineTo(p(12.0, 15.0)),
            ]
        );
    }

    #[test]
    fn a_movetos_extra_pairs_are_linetos() {
        // SVG 1.1 8.3.2 — the rule that makes "M 0 0 1 1" a line.
        let cmds = parse("M 0,0 1,1 2,2").expect("parses");
        assert_eq!(
            cmds,
            vec![
                PathCommand::MoveTo(p(0.0, 0.0)),
                PathCommand::LineTo(p(1.0, 1.0)),
                PathCommand::LineTo(p(2.0, 2.0)),
            ]
        );
    }

    #[test]
    fn a_relative_movetos_extra_pairs_are_relative_linetos() {
        let cmds = parse("m 10,10 1,1 1,1").expect("parses");
        assert_eq!(
            cmds,
            vec![
                PathCommand::MoveTo(p(10.0, 10.0)),
                PathCommand::LineTo(p(11.0, 11.0)),
                PathCommand::LineTo(p(12.0, 12.0)),
            ]
        );
    }

    #[test]
    fn a_command_repeats_without_repeating_its_letter() {
        let cmds = parse("M 0,0 L 1,1 2,2 3,3").expect("parses");
        assert_eq!(cmds.len(), 4);
        assert_eq!(cmds[3], PathCommand::LineTo(p(3.0, 3.0)));
    }

    #[test]
    fn horizontal_and_vertical_shorthands_become_linetos() {
        let cmds = parse("M 5,5 H 20 V 30 h -5 v -5").expect("parses");
        assert_eq!(
            cmds,
            vec![
                PathCommand::MoveTo(p(5.0, 5.0)),
                PathCommand::LineTo(p(20.0, 5.0)),
                PathCommand::LineTo(p(20.0, 30.0)),
                PathCommand::LineTo(p(15.0, 30.0)),
                PathCommand::LineTo(p(15.0, 25.0)),
            ]
        );
    }

    #[test]
    fn a_smooth_cubic_reflects_the_previous_control_point() {
        let cmds = parse("M 0,0 C 10,10 20,10 30,0 S 50,-10 60,0").expect("parses");
        let PathCommand::CurveTo { c1, .. } = cmds[2] else {
            panic!("expected a curve, got {:?}", cmds[2]);
        };
        // reflection of (20,10) through (30,0)
        assert!(near(c1.x, 40.0, 1e-4) && near(c1.y, -10.0, 1e-4), "{c1:?}");
    }

    #[test]
    fn a_smooth_cubic_after_a_line_starts_at_the_current_point() {
        let cmds = parse("M 0,0 L 10,0 S 20,10 30,0").expect("parses");
        let PathCommand::CurveTo { c1, .. } = cmds[2] else {
            panic!("expected a curve, got {:?}", cmds[2]);
        };
        assert_eq!(c1, p(10.0, 0.0));
    }

    #[test]
    fn a_smooth_quadratic_reflects_and_chains() {
        let cmds = parse("M 0,0 Q 10,10 20,0 T 40,0").expect("parses");
        let PathCommand::QuadTo { c, end } = cmds[2] else {
            panic!("expected a quadratic, got {:?}", cmds[2]);
        };
        assert!(near(c.x, 30.0, 1e-4) && near(c.y, -10.0, 1e-4), "{c:?}");
        assert_eq!(end, p(40.0, 0.0));
    }

    #[test]
    fn arc_flags_may_be_packed_against_the_next_number() {
        // "011 1" is large-arc 0, sweep 1, then the pair (1, 1) —
        // reading the flags as numbers would take 011 whole.
        let cmds = parse("M 0,0 a 5 5 0 011 1").expect("parses");
        let PathCommand::ArcTo(arc) = cmds[1] else {
            panic!("expected an arc, got {:?}", cmds[1]);
        };
        assert!(!arc.large_arc);
        assert!(arc.sweep);
        assert_eq!(arc.end, p(1.0, 1.0));
    }

    #[test]
    fn numbers_run_together_without_separators() {
        let cmds = parse("M0 0L1-2.5.5 3").expect("parses");
        assert_eq!(
            cmds,
            vec![
                PathCommand::MoveTo(p(0.0, 0.0)),
                PathCommand::LineTo(p(1.0, -2.5)),
                PathCommand::LineTo(p(0.5, 3.0)),
            ]
        );
    }

    #[test]
    fn exponents_and_leading_dots_parse() {
        let cmds = parse("M .5,-.5 L 1e2,1.5E-1").expect("parses");
        assert_eq!(cmds[0], PathCommand::MoveTo(p(0.5, -0.5)));
        assert_eq!(cmds[1], PathCommand::LineTo(p(100.0, 0.15)));
    }

    #[test]
    fn empty_data_is_an_empty_path_not_a_failure() {
        // The reference answers `nullopt` for this; an SVG path with
        // empty `d` is legal and renders nothing.
        assert_eq!(parse(""), Ok(Vec::new()));
        assert_eq!(parse("   \t\n "), Ok(Vec::new()));
    }

    // ---------- the errors the reference does not report ----------

    #[test]
    fn data_that_does_not_begin_with_a_moveto_is_rejected() {
        // The reference invents one: a line-to on an empty painter
        // path seeds a move-to at the origin through its lazy
        // initialiser.
        let err = parse("L 10,10").expect_err("must be rejected");
        assert_eq!(err.kind, PathDataErrorKind::MissingInitialMoveTo);
        assert_eq!(err.at, 0);
    }

    #[test]
    fn a_trailing_letter_with_no_arguments_is_reported_not_dropped() {
        // The reference drops it silently: the argument array comes
        // back empty, `while (count > 0)` never runs, and the command
        // disappears with the path reported as valid.
        let err = parse("M 0,0 L 1,1 C").expect_err("must be rejected");
        assert_eq!(err.kind, PathDataErrorKind::UnexpectedEnd);
        assert_eq!(err.command, Some('C'));
    }

    #[test]
    fn a_non_finite_number_is_rejected() {
        // The reference parses this to infinity, then `cubicTo` /
        // `lineTo` bail on !hasValidCoords and the segment vanishes,
        // warning only in debug builds.
        let err = parse("M 0,0 L 1e40,1").expect_err("must be rejected");
        assert_eq!(err.kind, PathDataErrorKind::NotFinite);
        assert_eq!(err.at, 8);
    }

    #[test]
    fn an_arc_flag_that_is_not_zero_or_one_is_reported_where_it_is() {
        let err = parse("M 0,0 A 5,5 0 2 1 10,10").expect_err("must be rejected");
        assert_eq!(err.kind, PathDataErrorKind::ExpectedFlag('2'));
        assert_eq!(err.at, 14);
        assert_eq!(err.command, Some('A'));
    }

    #[test]
    fn an_unknown_command_letter_is_reported_where_it_is() {
        let err = parse("M 0,0 X 1,1").expect_err("must be rejected");
        assert_eq!(err.kind, PathDataErrorKind::ExpectedCommand('X'));
        assert_eq!(err.at, 6);
    }

    #[test]
    fn a_number_after_a_close_is_reported() {
        let err = parse("M 0,0 L 1,1 Z 5").expect_err("must be rejected");
        assert_eq!(err.kind, PathDataErrorKind::ExpectedCommand('5'));
        assert_eq!(err.at, 14);
    }

    #[test]
    fn a_separator_promises_an_argument_that_must_arrive() {
        let err = parse("M 0,0 L 1,").expect_err("must be rejected");
        assert_eq!(err.kind, PathDataErrorKind::UnexpectedEnd);
        let err = parse("M 0,0 L 1,1, Z").expect_err("must be rejected");
        assert_eq!(err.kind, PathDataErrorKind::ExpectedNumber);
    }

    #[test]
    fn an_incomplete_argument_tuple_is_reported() {
        let err = parse("M 0,0 L 5").expect_err("must be rejected");
        assert_eq!(err.kind, PathDataErrorKind::UnexpectedEnd);
        assert_eq!(err.command, Some('L'));
    }

    #[test]
    fn an_error_says_the_byte_the_command_and_the_cause() {
        let err = parse("M 0,0 A 5,5 0 2 1 10,10").expect_err("must be rejected");
        let text = err.to_string();
        assert!(text.contains("byte 14"), "{text}");
        assert!(text.contains("'A'"), "{text}");
        assert!(text.contains("flag"), "{text}");
    }

    // ---------- round trip ----------

    #[test]
    fn write_then_parse_is_the_identity_for_every_kind() {
        let cmds = vec![
            PathCommand::MoveTo(p(1.5, -2.25)),
            PathCommand::LineTo(p(10.0, 0.125)),
            PathCommand::QuadTo {
                c: p(-3.5, 4.0),
                end: p(6.0, 7.0),
            },
            PathCommand::CurveTo {
                c1: p(1.0, 2.0),
                c2: p(3.0, 4.0),
                end: p(5.0, 6.0),
            },
            PathCommand::ArcTo(EllipticalArc::new(
                12.5,
                3.25,
                45.0,
                true,
                false,
                p(0.5, 0.75),
            )),
            PathCommand::ArcTo(EllipticalArc::new(1.0, 1.0, 0.0, false, true, p(9.0, 9.0))),
            PathCommand::Close,
        ];
        let text = write(&cmds);
        assert_eq!(parse(&text), Ok(cmds), "round trip through {text:?}");
    }

    #[test]
    fn write_is_stable_under_reparsing_a_shorthand_heavy_path() {
        let d = "m 2,2 h 8 v 8 h -8 z M 20,4 q 4,-4 8,0 t 8,0 c 2,2 4,2 6,0 s 4,-2 6,0 a 3,3 0 0 1 -6,6";
        let once = parse(d).expect("parses");
        let twice = parse(&write(&once)).expect("re-parses");
        assert_eq!(once, twice);
    }

    // ---------- geometry queries ----------

    #[test]
    fn bounds_are_tight_rather_than_the_control_point_hull() {
        // The control points reach y = 90, the curve only y = 67.5.
        let cmds = parse("M 0,0 C 0,90 100,90 100,0").expect("parses");
        let b = bounds(&cmds).expect("has bounds");
        assert!(near(b.min_x, 0.0, 1e-3), "{b:?}");
        assert!(near(b.max_x, 100.0, 1e-3), "{b:?}");
        assert!(near(b.min_y, 0.0, 1e-3), "{b:?}");
        assert!(near(b.max_y, 67.5, 1e-2), "{b:?}");
    }

    #[test]
    fn bounds_reach_an_arcs_extreme() {
        let cmds = parse("M 0,0 A 50,50 0 0 1 100,0").expect("parses");
        let b = bounds(&cmds).expect("has bounds");
        assert!(near(b.min_y, -50.0, 0.05), "{b:?}");
        assert!(near(b.max_y, 0.0, 0.05), "{b:?}");
        assert!(near(b.width(), 100.0, 0.05), "{b:?}");
    }

    #[test]
    fn bounds_of_nothing_is_none() {
        assert_eq!(bounds(&[]), None);
    }

    #[test]
    fn fit_scales_and_centres_while_preserving_aspect() {
        // A 100 x 50 box fitted into 40 x 40 scales by 0.4 and centres
        // the 20-tall result vertically.
        let cmds = parse("M 0,0 L 100,0 L 100,50 L 0,50 Z").expect("parses");
        let fitted = fit(&cmds, 40.0, 40.0).expect("fits");
        let b = bounds(&fitted).expect("has bounds");
        assert!(near(b.width(), 40.0, 1e-3), "{b:?}");
        assert!(near(b.height(), 20.0, 1e-3), "{b:?}");
        assert!(near(b.min_x, 0.0, 1e-3), "{b:?}");
        assert!(near(b.min_y, 10.0, 1e-3), "{b:?}");
    }

    #[test]
    fn fit_refuses_a_degenerate_box_rather_than_inventing_a_scale() {
        let cmds = parse("M 0,0 L 10,10").expect("parses");
        assert_eq!(fit(&cmds, 0.0, 10.0), None);
        assert_eq!(fit(&cmds, 10.0, -1.0), None);
        assert_eq!(fit(&[], 10.0, 10.0), None);
    }

    #[test]
    fn scaling_scales_an_arcs_radii_with_its_endpoint() {
        let cmds = parse("M 0,0 A 10,5 30 1 0 20,0").expect("parses");
        let scaled = scale_translate(&cmds, 3.0, 100.0, 200.0);
        let PathCommand::ArcTo(arc) = scaled[1] else {
            panic!("expected an arc, got {:?}", scaled[1]);
        };
        assert!(near(arc.rx, 30.0, 1e-4));
        assert!(near(arc.ry, 15.0, 1e-4));
        assert!(
            near(arc.x_rotation, 30.0, 1e-4),
            "rotation is scale-invariant"
        );
        assert_eq!(arc.end, p(160.0, 200.0));
        assert_eq!(scaled[0], PathCommand::MoveTo(p(100.0, 200.0)));
    }
}
