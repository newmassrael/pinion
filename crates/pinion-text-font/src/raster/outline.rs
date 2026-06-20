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
use crate::tables::glyf::{GlyphPoint, SimpleGlyph};

/// Max deviation (device px) of a flattened segment from the true quadratic.
/// 0.2px keeps curves visually smooth at text sizes while bounding depth.
const FLATTEN_TOLERANCE_PX: f32 = 0.2;

/// Recursion-depth guard for `flatten_quad` — prevents unbounded subdivision
/// on pathological (near-degenerate) control points.
const MAX_QUAD_DEPTH: u8 = 16;

/// Affine map from design (font) units to rasterization-buffer pixels.
///
/// `scale = px_per_em / units_per_em`; the y-axis is flipped (font y-up →
/// buffer y-down) and the buffer's top-left origin is subtracted.
pub(super) struct DeviceTransform {
    pub scale: f32,
    /// buffer x = `x * scale - left`.
    pub left: f32,
    /// buffer y = `-(y * scale) - top`.
    pub top: f32,
}

impl DeviceTransform {
    fn map(&self, p: GlyphPoint) -> Point {
        Point {
            x: f32::from(p.x) * self.scale - self.left,
            y: -(f32::from(p.y) * self.scale) - self.top,
        }
    }
}

/// Flatten every contour of `glyph` into closed device-space polylines,
/// passing each directed edge `(p0, p1)` to `emit`.
pub(super) fn for_each_edge<F: FnMut(Point, Point)>(
    glyph: &SimpleGlyph,
    xf: &DeviceTransform,
    mut emit: F,
) {
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
fn flatten_contour<F: FnMut(Point, Point)>(
    pts: &[GlyphPoint],
    xf: &DeviceTransform,
    emit: &mut F,
) {
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
