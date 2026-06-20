//! R50.8 §5.37.8 — self-hosted glyph rasterizer (outline → AA coverage bitmap).
//!
//! Takes a parsed TrueType simple-glyph outline (§5.37.1 `glyf`) and produces a
//! grayscale anti-aliased coverage bitmap, with **zero external dependencies**
//! (no swash / `ab_glyph`). This is §5.37's first pixel-producing layer — the
//! "vector outline → raster" step of the self-hosted text engine.
//!
//! # Algorithm — analytic signed-area accumulation (nonzero winding)
//!
//! The canonical pure-Rust method (Raph Levien's font-rs / `stb_truetype` v2):
//! exact analytic anti-aliasing, no supersampling, fully deterministic.
//!
//! 1. Each contour is flattened (`outline`) into device-space line segments.
//! 2. Every segment deposits **signed-area deltas** into an accumulation buffer
//!    `a` (one f32 per cell). For a segment crossing a pixel row, the amount
//!    `d = dir · dy` (winding sign × covered height) is distributed across the
//!    columns it passes through, weighted by the sub-pixel area to the *right*
//!    of the segment within each column. The leftover winding "carries" to the
//!    next column so a downstream prefix-sum reconstructs full coverage.
//! 3. A per-row prefix-sum of `a` yields per-pixel coverage; `abs()` realises
//!    the **nonzero winding rule** (outer contour fills, opposite-wound holes
//!    subtract), clamped to `[0, 1]` and scaled to `0..=255`.
//!
//! The buffer carries a 2-column stride slack + a 1px outline margin so an
//! edge's right-carry deposit can never spill into the next row, and edge
//! anti-aliasing at the glyph bbox border is never clipped.
//!
//! # Scope (R50.8)
//!
//! Simple + empty glyphs. Composite-glyph rasterization (§5.37.1
//! `CompositeGlyph`: component references + transforms) is a separate
//! sub-round — mirrors the parser's `R50.1.4.1` simple / `R50.1.4.2` composite
//! split — and currently returns [`RasterError::CompositeUnsupported`].

mod outline;

use crate::tables::glyf::SimpleGlyph;
use core::fmt;
use outline::{DeviceTransform, for_each_edge};

/// A device-space point (rasterization-buffer pixels, y-down).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    fn midpoint(self, other: Self) -> Self {
        Self {
            x: 0.5 * (self.x + other.x),
            y: 0.5 * (self.y + other.y),
        }
    }
}

/// Outline margin (px) added on every side of the *measured outline* bounds so
/// edge anti-aliasing is never clipped and right-carry deposits stay in-row.
const MARGIN: f32 = 1.0;

/// Pathological-size guard (px) per bitmap axis. A single glyph never needs
/// thousands of pixels; this caps `vec` allocation and prevents `usize`
/// saturation / `stride` overflow on absurd `px_per_em`. Beyond it → empty.
const MAX_DIM: f32 = 4096.0;

/// Grayscale anti-aliased coverage bitmap.
///
/// `alpha` is row-major `width × height`, each `0` (transparent) ..= `255`
/// (fully inked). `left` / `top` position the bitmap relative to the glyph pen
/// origin: to blit at a baseline pen `(pen_x, baseline_y)` (device px, y-down),
/// the bitmap's top-left pixel goes to `(pen_x + left, baseline_y + top)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    pub width: usize,
    pub height: usize,
    /// device-x of the bitmap's left column relative to the pen origin (px).
    pub left: i32,
    /// device-y (y-down, baseline = 0) of the bitmap's top row (px).
    pub top: i32,
    /// `width * height` coverage values, row-major, `0..=255`.
    pub alpha: Vec<u8>,
}

impl Coverage {
    /// An empty bitmap (e.g. a space / control glyph — nothing to paint).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            left: 0,
            top: 0,
            alpha: Vec::new(),
        }
    }

    /// `true` when the bitmap has no pixels.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Coverage at `(x, y)`; `0` outside bounds.
    #[must_use]
    pub fn at(&self, x: usize, y: usize) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.alpha.get(y * self.width + x).copied().unwrap_or(0)
    }

    /// Total coverage mass (sum of all alpha values) — diagnostic / test oracle.
    #[must_use]
    pub fn ink_sum(&self) -> u64 {
        self.alpha.iter().map(|&a| u64::from(a)).sum()
    }
}

/// Error from [`crate::Font::rasterize_glyph`].
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum RasterError {
    /// `glyph_id >= num_glyphs`.
    GlyphNotFound(u16),
    /// Composite glyph — rasterization deferred to a later sub-round (R50.8.x).
    CompositeUnsupported(u16),
}

impl fmt::Display for RasterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GlyphNotFound(gid) => write!(f, "glyph id {gid} out of range"),
            Self::CompositeUnsupported(gid) => {
                write!(f, "glyph id {gid} is composite (rasterization not yet supported)")
            }
        }
    }
}

impl core::error::Error for RasterError {}

/// Rasterize a simple glyph outline to an AA coverage bitmap at `px_per_em`.
///
/// The bitmap is sized to the **measured outline bounds**, not the glyph header
/// bbox — the OpenType per-glyph bbox is advisory and may be stale/loose, so
/// trusting it for buffer sizing would let an out-of-bbox point index out of
/// range. Deriving bounds from the flattened edges guarantees every point lands
/// inside `[MARGIN, dim-MARGIN]`, so `Raster::line`'s deposits are in-bounds by
/// construction.
///
/// Returns an empty [`Coverage`] for degenerate input (zero `units_per_em`,
/// non-positive / non-finite size, a zero-area / empty outline, or a
/// pathological size exceeding [`MAX_DIM`]).
pub(crate) fn rasterize_simple(
    glyph: &SimpleGlyph,
    units_per_em: u16,
    px_per_em: f32,
) -> Coverage {
    if units_per_em == 0 || !px_per_em.is_finite() || px_per_em <= 0.0 {
        return Coverage::empty();
    }
    let scale = px_per_em / f32::from(units_per_em);

    // Pass 1: flatten into pen-origin device space (scale only, no offset) and
    // measure the actual outline bounds.
    let pen_xf = DeviceTransform { scale, left: 0.0, top: 0.0 };
    let mut edges: Vec<(Point, Point)> = Vec::new();
    let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
    let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for_each_edge(glyph, &pen_xf, |p0, p1| {
        for p in [p0, p1] {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        edges.push((p0, p1));
    });
    if edges.is_empty() || !min_x.is_finite() {
        return Coverage::empty(); // no contours / no inkable geometry.
    }

    // Pass 2: buffer bounds = measured outline ± MARGIN. floor/ceil ± integer
    // margin keep these integer-valued, so the `as usize` widths are exact.
    let left_f = min_x.floor() - MARGIN;
    let top_f = min_y.floor() - MARGIN;
    let right_f = max_x.ceil() + MARGIN;
    let bottom_f = max_y.ceil() + MARGIN;
    let w_f = right_f - left_f;
    let h_f = bottom_f - top_f;
    if !(1.0..=MAX_DIM).contains(&w_f) || !(1.0..=MAX_DIM).contains(&h_f) {
        return Coverage::empty(); // degenerate or pathological size.
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // w_f / h_f are positive integer-valued floats in 1..=MAX_DIM (cast exact).
    let (w, height) = (w_f as usize, h_f as usize);

    let mut raster = Raster::new(w, height);
    for (p0, p1) in edges {
        raster.line(
            Point { x: p0.x - left_f, y: p0.y - top_f },
            Point { x: p1.x - left_f, y: p1.y - top_f },
        );
    }

    #[allow(clippy::cast_possible_truncation)] // left_f / top_f are small integers.
    raster.into_coverage(left_f as i32, top_f as i32)
}

/// Signed-area accumulation buffer.
struct Raster {
    /// logical bitmap width.
    w: usize,
    /// logical bitmap height.
    h: usize,
    /// memory columns per row = `w + 2` (right-carry slack so a deposit at the
    /// far-right column can never spill into the next row).
    stride: usize,
    /// `stride * h` signed-area deltas.
    a: Vec<f32>,
}

impl Raster {
    fn new(w: usize, h: usize) -> Self {
        let stride = w + 2;
        Self { w, h, stride, a: vec![0.0; stride * h] }
    }

    /// Deposit one line segment's signed-area contribution.
    ///
    /// Faithful to the font-rs accumulation method: the segment is walked
    /// scanline by scanline; within each row the covered height `dy` (signed by
    /// winding direction) is distributed across the spanned columns by the area
    /// to the right of the segment, with the remainder carried rightward.
    ///
    /// Precondition (held by `rasterize_simple`): points lie within
    /// `[MARGIN, dim - MARGIN]` because the buffer is sized to the measured
    /// outline bounds. Hence `x0i >= 0` (the `cast_sign_loss` allow is sound)
    /// and every deposit index stays within the `w + 2` stride — no clamp,
    /// no cross-row spill. `y` is additionally clamped to `[0, h)` below.
    #[allow(
        clippy::many_single_char_names,
        clippy::similar_names,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn line(&mut self, p0: Point, p1: Point) {
        if (p0.y - p1.y).abs() <= f32::EPSILON {
            return; // horizontal: no vertical winding, no area.
        }
        // Orient top→bottom (increasing y), remembering winding direction.
        let (dir, p0, p1) = if p0.y < p1.y { (1.0_f32, p0, p1) } else { (-1.0, p1, p0) };
        let dxdy = (p1.x - p0.x) / (p1.y - p0.y);

        let mut x = p0.x;
        if p0.y < 0.0 {
            x -= p0.y * dxdy; // advance x to the y = 0 crossing.
        }
        let y_start = p0.y.max(0.0) as usize;
        let y_end = self.h.min(p1.y.ceil() as usize);

        for y in y_start..y_end {
            let linestart = y * self.stride;
            let dy = ((y + 1) as f32).min(p1.y) - (y as f32).max(p0.y);
            let xnext = x + dxdy * dy;
            let d = dy * dir;

            let (x0, x1) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let x0floor = x0.floor();
            let x0i = x0floor as i32;
            let x1ceil = x1.ceil();
            let x1i = x1ceil as i32;

            if x1i <= x0i + 1 {
                // Segment confined to a single column [x0i, x0i+1).
                let xmf = 0.5 * (x + xnext) - x0floor;
                let idx = linestart + x0i as usize;
                self.a[idx] += d - d * xmf;
                self.a[idx + 1] += d * xmf;
            } else {
                // Segment spans ≥ 2 columns: ramp the area across them.
                let s = (x1 - x0).recip();
                let x0f = x0 - x0floor;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = x1 - x1ceil + 1.0;
                let am = 0.5 * s * x1f * x1f;
                let idx = linestart + x0i as usize;
                self.a[idx] += d * a0;
                if x1i == x0i + 2 {
                    self.a[idx + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - x0f);
                    self.a[idx + 1] += d * (a1 - a0);
                    for xi in x0i + 2..x1i - 1 {
                        self.a[linestart + xi as usize] += d * s;
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    self.a[linestart + (x1i - 1) as usize] += d * (1.0 - a2 - am);
                }
                self.a[linestart + x1i as usize] += d * am;
            }
            x = xnext;
        }
    }

    /// Per-row prefix-sum → coverage bytes (nonzero winding via `abs`).
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // `v ∈ [0,1]`; `v*255 + 0.5` rounds to the nearest `0..=255`.
    fn into_coverage(self, left: i32, top: i32) -> Coverage {
        let mut alpha = vec![0u8; self.w * self.h];
        for y in 0..self.h {
            let src = y * self.stride;
            let dst = y * self.w;
            let mut acc = 0.0_f32;
            for x in 0..self.w {
                acc += self.a[src + x];
                let v = acc.abs();
                let v = if v < 1.0 { v } else { 1.0 };
                alpha[dst + x] = (v * 255.0 + 0.5) as u8;
            }
        }
        Coverage { width: self.w, height: self.h, left, top, alpha }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::glyf::{GlyphHeader, GlyphPoint};

    /// Build an axis-aligned rectangle contour (clockwise in font y-up).
    fn rect_glyph(x_min: i16, y_min: i16, x_max: i16, y_max: i16) -> SimpleGlyph {
        SimpleGlyph {
            header: GlyphHeader { x_min, y_min, x_max, y_max },
            end_pts_of_contours: vec![3],
            instructions: vec![],
            points: vec![
                GlyphPoint { x: x_min, y: y_min, on_curve: true },
                GlyphPoint { x: x_min, y: y_max, on_curve: true },
                GlyphPoint { x: x_max, y: y_max, on_curve: true },
                GlyphPoint { x: x_max, y: y_min, on_curve: true },
            ],
        }
    }

    #[test]
    #[allow(clippy::naive_bytecount)] // §5.37 = 0 external deps; no bytecount crate.
    fn rect_interior_opaque_border_transparent() {
        // scale 1.0 (px == upem) maps design units to exact device pixels, so the
        // AA is EXACT: a 5×10 integer-aligned rect fills exactly 50 pixels at 255
        // with zero partial pixels, inside a 7×12 (5×10 + 1px margin) bitmap.
        let g = rect_glyph(0, 0, 5, 10);
        let cov = rasterize_simple(&g, 100, 100.0);
        assert_eq!((cov.width, cov.height), (7, 12), "measured bounds + 1px margin");
        let full = cov.alpha.iter().filter(|&&a| a == 255).count();
        assert_eq!(full, 50, "exactly 5×10 fully-opaque pixels, got {full}");
        let partial = cov.alpha.iter().filter(|&&a| a > 0 && a < 255).count();
        assert_eq!(partial, 0, "integer-aligned edges → no partial pixels");
        assert_eq!(cov.at(3, 5), 255, "interior opaque");
        // The 1px margin ring is fully transparent on all four sides.
        for x in 0..cov.width {
            assert_eq!(cov.at(x, 0), 0, "top margin");
            assert_eq!(cov.at(x, cov.height - 1), 0, "bottom margin");
        }
        for y in 0..cov.height {
            assert_eq!(cov.at(0, y), 0, "left margin");
            assert_eq!(cov.at(cov.width - 1, y), 0, "right margin");
        }
    }

    #[test]
    fn rect_fractional_right_edge_is_half_covered() {
        // Right edge at design x=250 → device x=2.5 at 10px/em: the column
        // straddling 2.5 gets ≈ 50 % coverage (analytic AA on a vertical edge).
        let g = rect_glyph(0, 0, 250, 1000);
        let cov = rasterize_simple(&g, 1000, 10.0);
        // Find a fully-interior row and inspect its rightmost inked column.
        let mid_row = cov.height / 2;
        let mut last_partial = None;
        for x in 0..cov.width {
            let a = cov.at(x, mid_row);
            if a > 0 && a < 255 {
                last_partial = Some(a);
            }
        }
        let a = last_partial.expect("a partially-covered AA column must exist");
        assert!((118..=138).contains(&a), "edge AA ~50% expected, got {a}");
    }

    #[test]
    fn right_triangle_area_is_about_half_of_rect() {
        // Analytic-area sanity on a sloped edge: a right triangle covering half
        // of a square should accumulate ≈ half the ink of the full square.
        let square = rect_glyph(0, 0, 1000, 1000);
        let sq_cov = rasterize_simple(&square, 1000, 40.0);

        // Right triangle (0,0)-(1000,0)-(1000,1000): half the square.
        let tri = SimpleGlyph {
            header: GlyphHeader { x_min: 0, y_min: 0, x_max: 1000, y_max: 1000 },
            end_pts_of_contours: vec![2],
            instructions: vec![],
            points: vec![
                GlyphPoint { x: 0, y: 0, on_curve: true },
                GlyphPoint { x: 1000, y: 0, on_curve: true },
                GlyphPoint { x: 1000, y: 1000, on_curve: true },
            ],
        };
        let tri_cov = rasterize_simple(&tri, 1000, 40.0);

        #[allow(clippy::cast_precision_loss)] // ink sums are small, well under 2^52.
        let ratio = tri_cov.ink_sum() as f64 / sq_cov.ink_sum() as f64;
        assert!((0.45..=0.55).contains(&ratio), "triangle/square ink ratio = {ratio}");
    }

    #[test]
    fn two_contour_hole_is_hollow() {
        // Outer rect CW + inner rect CCW (opposite winding) → nonzero-winding
        // hole: the ring is inked, the centre is transparent.
        let g = SimpleGlyph {
            header: GlyphHeader { x_min: 0, y_min: 0, x_max: 1000, y_max: 1000 },
            end_pts_of_contours: vec![3, 7],
            instructions: vec![],
            points: vec![
                // outer, clockwise in y-up
                GlyphPoint { x: 0, y: 0, on_curve: true },
                GlyphPoint { x: 0, y: 1000, on_curve: true },
                GlyphPoint { x: 1000, y: 1000, on_curve: true },
                GlyphPoint { x: 1000, y: 0, on_curve: true },
                // inner, counter-clockwise (reverse traversal)
                GlyphPoint { x: 300, y: 300, on_curve: true },
                GlyphPoint { x: 700, y: 300, on_curve: true },
                GlyphPoint { x: 700, y: 700, on_curve: true },
                GlyphPoint { x: 300, y: 700, on_curve: true },
            ],
        };
        let cov = rasterize_simple(&g, 1000, 40.0);
        // Centre of the hole (device ~20,20 from a 40px em) is transparent.
        let cx = cov.width / 2;
        let cy = cov.height / 2;
        assert_eq!(cov.at(cx, cy), 0, "hole centre should be transparent");
        // The ring carries ink.
        assert!(cov.ink_sum() > 0, "ring should be inked");
        // A point in the ring band (near the left wall, vertically centred).
        assert!(cov.at(2, cy) > 0, "ring wall should be inked");
    }

    #[test]
    fn quadratic_off_curve_bulges_inside_straight_chord() {
        // Region bounded on the left by x=0 and on the right by a quadratic
        // (0,0) → control (1000,1000) → (0,1000). A quadratic lies in the convex
        // hull of its 3 points, peaking at x=500 (B(0.5)), so it encloses
        // STRICTLY LESS area than the triangle (0,0)-(1000,1000)-(0,1000) you get
        // by treating the control as a straight on-curve vertex. Closed-form:
        // quad area 1/12·4e6 ≈ 333k vs triangle 500k (ratio ≈ 0.667). This is the
        // oracle that de Casteljau flattening truly curves — a straight-vertex
        // fallback bug would make the two ink masses equal.
        let curved = SimpleGlyph {
            header: GlyphHeader { x_min: 0, y_min: 0, x_max: 1000, y_max: 1000 },
            end_pts_of_contours: vec![2],
            instructions: vec![],
            points: vec![
                GlyphPoint { x: 0, y: 0, on_curve: true },
                GlyphPoint { x: 1000, y: 1000, on_curve: false }, // quadratic control
                GlyphPoint { x: 0, y: 1000, on_curve: true },
            ],
        };
        let straight = SimpleGlyph {
            points: vec![
                GlyphPoint { x: 0, y: 0, on_curve: true },
                GlyphPoint { x: 1000, y: 1000, on_curve: true }, // straight vertex
                GlyphPoint { x: 0, y: 1000, on_curve: true },
            ],
            ..curved.clone()
        };
        let curved_ink = rasterize_simple(&curved, 1000, 64.0).ink_sum();
        let straight_ink = rasterize_simple(&straight, 1000, 64.0).ink_sum();
        assert!(curved_ink > 0, "curve must ink");
        // curved/straight ≈ 0.667 < 0.8 with comfortable AA margin.
        assert!(
            curved_ink * 5 < straight_ink * 4,
            "quad {curved_ink} must ink << triangle {straight_ink} (curve bulges inside chord)",
        );
    }

    #[test]
    fn same_winding_nested_fills_solid_under_nonzero() {
        // Two SAME-wound nested rects: the inner region has winding number 2.
        // Nonzero rule → filled (this rasterizer, via abs of accumulated
        // winding); even-odd rule → hole. An inked centre therefore proves the
        // fill rule is NONZERO, not even-odd — which the opposite-wound hole
        // test alone cannot distinguish (nested opposite rects hole under both).
        let same = SimpleGlyph {
            header: GlyphHeader { x_min: 0, y_min: 0, x_max: 1000, y_max: 1000 },
            end_pts_of_contours: vec![3, 7],
            instructions: vec![],
            points: vec![
                // outer (clockwise in y-up)
                GlyphPoint { x: 0, y: 0, on_curve: true },
                GlyphPoint { x: 0, y: 1000, on_curve: true },
                GlyphPoint { x: 1000, y: 1000, on_curve: true },
                GlyphPoint { x: 1000, y: 0, on_curve: true },
                // inner — SAME traversal direction → same winding
                GlyphPoint { x: 300, y: 300, on_curve: true },
                GlyphPoint { x: 300, y: 700, on_curve: true },
                GlyphPoint { x: 700, y: 700, on_curve: true },
                GlyphPoint { x: 700, y: 300, on_curve: true },
            ],
        };
        let cov = rasterize_simple(&same, 1000, 40.0);
        let (cx, cy) = (cov.width / 2, cov.height / 2);
        assert!(
            cov.at(cx, cy) > 0,
            "winding-2 centre must be inked (nonzero), got {}",
            cov.at(cx, cy),
        );
    }

    #[test]
    fn degenerate_input_yields_empty_coverage() {
        let g = rect_glyph(0, 0, 500, 1000);
        assert!(rasterize_simple(&g, 0, 10.0).is_empty(), "zero upem");
        assert!(rasterize_simple(&g, 1000, 0.0).is_empty(), "zero size");
        assert!(rasterize_simple(&g, 1000, f32::NAN).is_empty(), "NaN size");
        assert!(rasterize_simple(&g, 1000, -5.0).is_empty(), "negative size");
    }
}
