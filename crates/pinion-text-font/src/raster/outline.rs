//! R50.8 §5.37.8 — TrueType outline flattening (quadratic contours → polylines).
//!
//! `glyf` simple-glyph outlines (§5.37.1) are sequences of on-curve and
//! off-curve points grouped into contours. This module turns each contour
//! into a closed device-space polyline, emitting its edges to the rasterizer:
//!
//! * **on → on**: straight edge.
//! * **on → off → on**: quadratic Bézier (the off-curve point is the control);
//!   flattened by recursive de Casteljau subdivision to `FLATTEN_TOLERANCE_PX`.
//! * **off → off**: TrueType implies an on-curve point at the midpoint, so the
//!   pair expands to two quadratics sharing that implied anchor.
//!
//! All emitted points lie *on* the true curve (de Casteljau split points are
//! on-curve), so the flattened polyline stays within the glyph's bounding box.

use super::Point;
use crate::tables::glyf::{ComponentTransform, GlyphPoint, SimpleGlyph};

/// Max deviation (device px) of a flattened segment from the true quadratic.
/// 0.2px keeps curves visually smooth at text sizes while bounding depth.
const FLATTEN_TOLERANCE_PX: f32 = 0.2;

/// Recursion-depth guard for `flatten_quad` — prevents unbounded subdivision
/// on pathological (near-degenerate) control points.
const MAX_QUAD_DEPTH: u8 = 16;

/// Affine map from a glyph's design (font) units to rasterization-buffer pixels
/// (y-down). A point `(x, y)` maps to `(a·x + c·y + e, b·x + d·y + f)` — the
/// CSS `matrix(a, b, c, d, e, f)` field convention.
///
/// The base map for a top-level glyph (no composite component) is the y-flip
/// scale `a = scale, d = -scale, b = c = 0` (font y-up → buffer y-down), with
/// the buffer origin folded into `e`/`f` by [`Affine::translated`]. A composite
/// component contributes a design-space affine ([`Affine::from_component`]) that
/// is composed on the *inside* via [`Affine::concat`], so a leaf simple glyph is
/// flattened by the single fused transform: points first by the component chain
/// (design space), then by the base scale-flip-and-offset (device space).
#[derive(Clone, Copy)]
pub(super) struct Affine {
    pub(super) a: f32,
    pub(super) b: f32,
    pub(super) c: f32,
    pub(super) d: f32,
    pub(super) e: f32,
    pub(super) f: f32,
}

impl Affine {
    /// The base design→device map at `scale = px_per_em / units_per_em`: scale
    /// plus the y-flip, with a zero buffer offset (folded in by `translated`).
    pub(super) fn scale_flip(scale: f32) -> Self {
        Self {
            a: scale,
            b: 0.0,
            c: 0.0,
            d: -scale,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Post-translate in device space (fold the buffer origin into the map):
    /// `translate(dx, dy) ∘ self`.
    pub(super) fn translated(self, dx: f32, dy: f32) -> Self {
        Self {
            e: self.e + dx,
            f: self.f + dy,
            ..self
        }
    }

    /// another declarative toolkit `self ∘ inner` — `inner` (a child component's design-space
    /// affine) applies first, then `self` (the accumulated parent→device map).
    pub(super) fn concat(&self, inner: &Self) -> Self {
        Self {
            a: self.a * inner.a + self.c * inner.b,
            b: self.b * inner.a + self.d * inner.b,
            c: self.a * inner.c + self.c * inner.d,
            d: self.b * inner.c + self.d * inner.d,
            e: self.a * inner.e + self.c * inner.f + self.e,
            f: self.b * inner.e + self.d * inner.f + self.f,
        }
    }

    /// A composite component's design-space affine: the F2DOT14 2×2 transform
    /// (`raw / 16384`, identity by default) plus the XY placement offset (font
    /// units). The offset is applied *unscaled* (the Microsoft default; honoring
    /// `SCALED_COMPONENT_OFFSET` is a later sub-round — accented composites use
    /// identity transforms where scaled vs unscaled is moot).
    pub(super) fn from_component(transform: ComponentTransform, dx: f32, dy: f32) -> Self {
        const F2DOT14: f32 = 1.0 / 16384.0;
        // spec 2×2 form: (x', y') = (x·xx + y·yx, x·xy + y·yy) →
        // a = xx (coeff of x in x'), c = yx (coeff of y in x'),
        // b = xy (coeff of x in y'), d = yy (coeff of y in y').
        let (a, b, c, d) = match transform {
            ComponentTransform::Identity => (1.0, 0.0, 0.0, 1.0),
            ComponentTransform::Scale { scale } => {
                let s = f32::from(scale) * F2DOT14;
                (s, 0.0, 0.0, s)
            }
            ComponentTransform::XYScale { x, y } => {
                (f32::from(x) * F2DOT14, 0.0, 0.0, f32::from(y) * F2DOT14)
            }
            ComponentTransform::Matrix { xx, xy, yx, yy } => (
                f32::from(xx) * F2DOT14,
                f32::from(xy) * F2DOT14,
                f32::from(yx) * F2DOT14,
                f32::from(yy) * F2DOT14,
            ),
        };
        Self {
            a,
            b,
            c,
            d,
            e: dx,
            f: dy,
        }
    }

    fn map(&self, p: GlyphPoint) -> Point {
        Point {
            x: self.a * f32::from(p.x) + self.c * f32::from(p.y) + self.e,
            y: self.b * f32::from(p.x) + self.d * f32::from(p.y) + self.f,
        }
    }
}

/// Flatten every contour of `glyph` into closed device-space polylines,
/// passing each directed edge `(p0, p1)` to `emit`.
pub(super) fn for_each_edge<F: FnMut(Point, Point)>(glyph: &SimpleGlyph, xf: &Affine, mut emit: F) {
    let mut start = 0usize;
    for &end in &glyph.end_pts_of_contours {
        let end = usize::from(end);
        // Parser guarantees end_pts in-range + ascending, but never index OOB.
        if end >= glyph.points.len() || end < start {
            break;
        }
        flatten_contour(&glyph.points[start..=end], xf, &mut emit);
        start = end + 1;
    }
}

/// Flatten one contour (a closed cycle of on/off-curve points).
fn flatten_contour<F: FnMut(Point, Point)>(pts: &[GlyphPoint], xf: &Affine, emit: &mut F) {
    let n = pts.len();
    if n < 2 {
        return; // single point / empty contour bounds no area.
    }

    // 1. Map to device space, inserting implied on-curve midpoints between
    //    consecutive off-curve points. After this pass an off-curve point is
    //    always followed by an on-curve point, and at least one on-curve point
    //    exists (an all-off-curve contour gains midpoints).
    let mut seq: Vec<(Point, bool)> = Vec::with_capacity(n * 2);
    for i in 0..n {
        let cur = pts[i];
        let cur_pt = xf.map(cur);
        seq.push((cur_pt, cur.on_curve));
        let next = pts[(i + 1) % n];
        if !cur.on_curve && !next.on_curve {
            seq.push((cur_pt.midpoint(xf.map(next)), true));
        }
    }

    // 2. Rotate so traversal begins at an on-curve anchor.
    let Some(start) = seq.iter().position(|&(_, on)| on) else {
        return;
    };
    let m = seq.len();
    let first = seq[start].0;
    let mut poly: Vec<Point> = vec![first];

    // 3. Walk the cycle: on-curve → line vertex, off-curve → quadratic to the
    //    following (always on-curve) anchor.
    let mut i = 1;
    while i <= m {
        let (pt, on) = seq[(start + i) % m];
        if on {
            poly.push(pt);
            i += 1;
        } else {
            let end = seq[(start + i + 1) % m].0;
            let p0 = *poly.last().expect("poly seeded with start anchor");
            flatten_quad(p0, pt, end, &mut poly, 0);
            i += 2;
        }
    }

    // 4. Emit edges of the closed polyline (poly ends back at `first`).
    for pair in poly.windows(2) {
        emit(pair[0], pair[1]);
    }
}

/// Recursive de Casteljau flattening of the quadratic `p0 → ctrl → p2`.
/// Pushes only the intermediate points and the endpoint `p2` (the caller has
/// already placed `p0`).
fn flatten_quad(p0: Point, ctrl: Point, p2: Point, out: &mut Vec<Point>, depth: u8) {
    // Max deviation of a quadratic from its chord = ½·dist(ctrl, chord-mid).
    let chord_mid = p0.midpoint(p2);
    let dx = ctrl.x - chord_mid.x;
    let dy = ctrl.y - chord_mid.y;
    if depth >= MAX_QUAD_DEPTH
        || 0.25 * (dx * dx + dy * dy) <= FLATTEN_TOLERANCE_PX * FLATTEN_TOLERANCE_PX
    {
        out.push(p2);
        return;
    }
    let p01 = p0.midpoint(ctrl);
    let p12 = ctrl.midpoint(p2);
    let mid = p01.midpoint(p12);
    flatten_quad(p0, p01, mid, out, depth + 1);
    flatten_quad(mid, p12, p2, out, depth + 1);
}
