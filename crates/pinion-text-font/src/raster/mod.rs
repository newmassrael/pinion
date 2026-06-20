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
//! anti-aliasing at the outline bounds is never clipped.
//!
//! # Scope
//!
//! Simple + empty glyphs (R50.8) and composite glyphs (R50.8.x): a composite's
//! component subglyphs (§5.37.1 `CompositeGlyph`) are resolved recursively and
//! their outlines composed into one accumulation buffer via per-component affine
//! transforms (`Affine`). The parser leaves cycle detection to this
//! traversal layer, so component recursion is guarded by an ancestor set
//! ([`RasterError::CompositeCycle`]). Point-matched component placement
//! (`ARGS_ARE_XY_VALUES = 0`) and the `SCALED_COMPONENT_OFFSET` flag are a later
//! sub-round — a point-match component is reported fail-loud as
//! [`RasterError::PointMatchUnsupported`].

mod outline;

use crate::tables::glyf::{ComponentArgs, Glyph};
use core::fmt;
use outline::{Affine, for_each_edge};

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

/// Pathological-size guard (px) per bitmap axis. 4096px/axis is a generous
/// ceiling for a single glyph (e.g. ~256pt at 600 DPI ≈ 2133px); beyond it a
/// `px_per_em` is almost certainly a caller bug. The cap bounds `vec`
/// allocation and prevents `usize` saturation / `stride` overflow; exceeding it
/// is reported as [`RasterError::SizeExceeded`] (fail-loud), never silently
/// dropped.
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
    /// `glyph_id >= num_glyphs` — either the top-level id or a composite
    /// component that references a non-existent subglyph.
    GlyphNotFound(u16),
    /// A composite component places its subglyph by point-matching
    /// (`ARGS_ARE_XY_VALUES = 0`) rather than an XY offset; assembled-point
    /// alignment is a later sub-round. Payload = the composite glyph id.
    PointMatchUnsupported(u16),
    /// A composite component chain revisits an ancestor glyph (reference cycle)
    /// or nests past the depth cap — a malformed / runaway font. Payload = the
    /// glyph id at which the cycle / limit was detected.
    CompositeCycle(u16),
    /// The requested size would exceed the per-axis pixel cap — a pathological
    /// `px_per_em`. Reported (not silently dropped) so the caller sees the bug.
    SizeExceeded { width: u32, height: u32 },
}

impl fmt::Display for RasterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GlyphNotFound(gid) => write!(f, "glyph id {gid} out of range"),
            Self::PointMatchUnsupported(gid) => {
                write!(f, "composite glyph id {gid} uses point-matched components (unsupported)")
            }
            Self::CompositeCycle(gid) => {
                write!(f, "composite glyph id {gid} forms a component reference cycle")
            }
            Self::SizeExceeded { width, height } => {
                write!(f, "rasterized size {width}x{height}px exceeds the {MAX_DIM} px limit")
            }
        }
    }
}

impl core::error::Error for RasterError {}

/// Maximum composite nesting depth — a backstop beyond the ancestor-set cycle
/// check for a pathologically deep (acyclic) component chain. Real composites
/// nest one or two levels; 8 is generous.
const MAX_COMPOSITE_DEPTH: usize = 8;

/// Rasterize a glyph (empty, simple, or composite) to an AA coverage bitmap at
/// `px_per_em`. `glyph` is the parsed outline for `glyph_id`; `resolve` fetches a
/// composite component's subglyph by id (the recursion that composes a composite
/// from its parts).
///
/// The bitmap is sized to the **measured outline bounds** (two deterministic
/// flatten passes over all contours / components: one to measure, one to
/// deposit), not the glyph header bbox — the OpenType per-glyph bbox is advisory
/// and may be stale/loose, so trusting it for buffer sizing would let an
/// out-of-bbox point index out of range. Measured bounds guarantee every point
/// lands inside `[MARGIN, dim - MARGIN]`, so `Raster::line`'s deposits are
/// in-bounds by construction.
///
/// Returns `Ok(Coverage::empty())` when there is nothing to ink (zero
/// `units_per_em`, non-positive / non-finite size, no contours, or a zero-extent
/// outline); `Err(RasterError::SizeExceeded)` for a pathological size beyond
/// [`MAX_DIM`]; `Err(RasterError::CompositeCycle)` / `PointMatchUnsupported` /
/// `GlyphNotFound` for a malformed or not-yet-supported composite.
pub(crate) fn rasterize_glyph_outline<'f>(
    glyph_id: u16,
    glyph: &'f Glyph,
    resolve: &dyn Fn(u16) -> Option<&'f Glyph>,
    units_per_em: u16,
    px_per_em: f32,
) -> Result<Coverage, RasterError> {
    rasterize_with(units_per_em, px_per_em, |xf, emit| {
        // A fresh ancestor chain per pass — the measure and deposit walks are
        // independent traversals of the same component tree.
        let mut ancestors = vec![glyph_id];
        walk_edges(glyph_id, glyph, resolve, xf, &mut ancestors, emit)
    })
}

/// Shared 2-pass rasterization core: a measure pass to size the bitmap to the
/// outline bounds, then a deposit pass. `walk` emits the glyph's device-space
/// edges under a given affine — the base scale-flip for the measure pass, plus
/// the buffer origin for the deposit pass — and may fail (composite cycle /
/// point-match / a component referencing a missing glyph).
fn rasterize_with(
    units_per_em: u16,
    px_per_em: f32,
    walk: impl Fn(&Affine, &mut dyn FnMut(Point, Point)) -> Result<(), RasterError>,
) -> Result<Coverage, RasterError> {
    if units_per_em == 0 || !px_per_em.is_finite() || px_per_em <= 0.0 {
        return Ok(Coverage::empty());
    }
    let scale = px_per_em / f32::from(units_per_em);

    // Pass 1: flatten in pen-origin device space (zero buffer offset) to MEASURE
    // the outline bounds. `Affine::scale_flip` is the base map; the deposit pass
    // (below) folds in the real buffer origin via `translated`.
    let measure_xf = Affine::scale_flip(scale);
    let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
    let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut any_edge = false;
    walk(&measure_xf, &mut |p0, p1| {
        any_edge = true;
        for p in [p0, p1] {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
    })?;
    // Nothing to ink: no contours, a non-finite transform (overflowed scale), or
    // a zero-extent outline (all points colinear → zero area).
    if !any_edge
        || ![min_x, min_y, max_x, max_y].iter().all(|v| v.is_finite())
        || max_x <= min_x
        || max_y <= min_y
    {
        return Ok(Coverage::empty());
    }

    // Buffer bounds = measured outline ± MARGIN. floor/ceil ± integer margin keep
    // these integer-valued, so the `as usize` widths are exact. With a positive
    // extent, `w_f`/`h_f` are >= 3, so only the upper MAX_DIM bound can trip.
    let left_f = min_x.floor() - MARGIN;
    let top_f = min_y.floor() - MARGIN;
    let w_f = (max_x.ceil() + MARGIN) - left_f;
    let h_f = (max_y.ceil() + MARGIN) - top_f;
    if w_f > MAX_DIM || h_f > MAX_DIM {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        // positive floats; the cast only builds the diagnostic payload.
        return Err(RasterError::SizeExceeded { width: w_f as u32, height: h_f as u32 });
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // w_f / h_f are positive integer-valued floats in 3..=MAX_DIM (cast exact).
    let (w, height) = (w_f as usize, h_f as usize);

    // Pass 2: re-walk with the buffer origin folded in and deposit. Re-running the
    // same deterministic flatten (vs caching pass-1 edges) keeps `Affine` the
    // sole, honest home of the transform and avoids an intermediate edge buffer.
    let deposit_xf = measure_xf.translated(-left_f, -top_f);
    let mut raster = Raster::new(w, height);
    walk(&deposit_xf, &mut |p0, p1| raster.line(p0, p1))?;

    #[allow(clippy::cast_possible_truncation)] // left_f / top_f are small integers.
    Ok(raster.into_coverage(left_f as i32, top_f as i32))
}

/// Emit every leaf edge of `glyph` under the accumulated design→device affine
/// `xf`, recursing into composite components. `ancestors` is the chain of glyph
/// ids from the root to `glyph` (inclusive); a component referencing any of them
/// is a reference cycle (the parser leaves cycle detection to this layer).
fn walk_edges<'f>(
    glyph_id: u16,
    glyph: &'f Glyph,
    resolve: &dyn Fn(u16) -> Option<&'f Glyph>,
    xf: &Affine,
    ancestors: &mut Vec<u16>,
    emit: &mut dyn FnMut(Point, Point),
) -> Result<(), RasterError> {
    match glyph {
        Glyph::Empty => Ok(()),
        Glyph::Simple(s) => {
            for_each_edge(s, xf, &mut *emit);
            Ok(())
        }
        Glyph::Composite(c) => {
            if ancestors.len() > MAX_COMPOSITE_DEPTH {
                return Err(RasterError::CompositeCycle(glyph_id));
            }
            for comp in &c.components {
                let (dx, dy) = match comp.args {
                    #[allow(clippy::cast_precision_loss)] // small font-unit offsets.
                    ComponentArgs::Offset { x, y } => (x as f32, y as f32),
                    ComponentArgs::PointMatch { .. } => {
                        return Err(RasterError::PointMatchUnsupported(glyph_id));
                    }
                };
                let child_id = comp.glyph_index;
                if ancestors.contains(&child_id) {
                    return Err(RasterError::CompositeCycle(child_id));
                }
                let Some(child) = resolve(child_id) else {
                    return Err(RasterError::GlyphNotFound(child_id));
                };
                let child_xf = xf.concat(&Affine::from_component(comp.transform, dx, dy));
                ancestors.push(child_id);
                walk_edges(child_id, child, resolve, &child_xf, ancestors, &mut *emit)?;
                ancestors.pop();
            }
            Ok(())
        }
    }
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
    use crate::tables::glyf::{
        Component, ComponentTransform, CompositeGlyph, GlyphHeader, GlyphPoint, SimpleGlyph,
    };

    /// Rasterize a single simple glyph (a one-entry glyph table) through the
    /// unified entry, expecting success (within the size limit).
    fn raster(g: &SimpleGlyph, units_per_em: u16, px_per_em: f32) -> Coverage {
        raster_try(g, units_per_em, px_per_em).expect("rasterizes within size limit")
    }

    /// Fallible single-simple-glyph rasterization (for the size-limit assertion).
    fn raster_try(g: &SimpleGlyph, units_per_em: u16, px_per_em: f32) -> Result<Coverage, RasterError> {
        let glyphs = [Glyph::Simple(g.clone())];
        rasterize_glyph_outline(0, &glyphs[0], &|gid| glyphs.get(usize::from(gid)), units_per_em, px_per_em)
    }

    /// Rasterize glyph `top` from a synthetic glyph table (composite recursion
    /// resolves component ids against the slice).
    fn raster_composite(
        glyphs: &[Glyph],
        top: u16,
        units_per_em: u16,
        px_per_em: f32,
    ) -> Result<Coverage, RasterError> {
        rasterize_glyph_outline(
            top,
            &glyphs[usize::from(top)],
            &|gid| glyphs.get(usize::from(gid)),
            units_per_em,
            px_per_em,
        )
    }

    /// A composite glyph wrapping the given components (header bbox is advisory
    /// and unused — the rasterizer measures bounds).
    fn composite(components: Vec<Component>) -> Glyph {
        Glyph::Composite(CompositeGlyph {
            header: GlyphHeader { x_min: 0, y_min: 0, x_max: 0, y_max: 0 },
            raw_body: Vec::new(),
            components,
            instructions: Vec::new(),
        })
    }

    /// An XY-offset component referencing `glyph_index` (flags unused by the
    /// rasterizer — `args`/`transform` drive placement).
    fn xy_component(glyph_index: u16, x: i32, y: i32, transform: ComponentTransform) -> Component {
        Component {
            flags: 0,
            glyph_index,
            args: ComponentArgs::Offset { x, y },
            transform,
        }
    }

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
        let cov = raster(&g, 100, 100.0);
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
        let cov = raster(&g, 1000, 10.0);
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
        let sq_cov = raster(&square, 1000, 40.0);

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
        let tri_cov = raster(&tri, 1000, 40.0);

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
        let cov = raster(&g, 1000, 40.0);
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
        let curved_ink = raster(&curved, 1000, 64.0).ink_sum();
        let straight_ink = raster(&straight, 1000, 64.0).ink_sum();
        assert!(curved_ink > 0, "curve must ink");
        // curved/straight ≈ 0.667 < 0.8 with comfortable AA margin.
        assert!(
            curved_ink * 5 < straight_ink * 4,
            "quad {curved_ink} must ink << triangle {straight_ink} (curve bulges inside chord)",
        );
        // Closed-form pin: analytic coverage means ink_sum == 255·area(px²). The
        // enclosed quadratic-segment area is ∫x dy = 4e6/12 ≈ 333_333 design²;
        // at scale 64/1000 that is 333_333·(64/1000)² px². If de Casteljau
        // flattening reproduced the curve, curved_ink must match within a few %
        // (a wrong subdivision/midpoint would shift the area measurably).
        #[allow(clippy::cast_precision_loss)] // ink sum well under 2^52.
        let ratio = curved_ink as f64 / (333_333.0 * (64.0_f64 / 1000.0).powi(2) * 255.0);
        assert!((0.95..=1.05).contains(&ratio), "quad area ratio {ratio} off closed form");
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
        let cov = raster(&same, 1000, 40.0);
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
        for (upem, px, label) in [
            (0u16, 10.0f32, "zero upem"),
            (1000, 0.0, "zero size"),
            (1000, f32::NAN, "NaN size"),
            (1000, -5.0, "negative size"),
        ] {
            assert!(raster(&g, upem, px).is_empty(), "{label}");
        }
        // A zero-extent outline (all points colinear → zero area) inks nothing.
        let line = SimpleGlyph {
            header: GlyphHeader { x_min: 0, y_min: 0, x_max: 1000, y_max: 0 },
            end_pts_of_contours: vec![1],
            instructions: vec![],
            points: vec![
                GlyphPoint { x: 0, y: 0, on_curve: true },
                GlyphPoint { x: 1000, y: 0, on_curve: true },
            ],
        };
        assert!(raster(&line, 1000, 40.0).is_empty(), "horizontal line");
    }

    #[test]
    fn oversize_reports_size_exceeded() {
        // A glyph at an absurd px_per_em exceeds MAX_DIM → fail-loud, not silent
        // empty (a giant glyph silently vanishing would be a debugging trap).
        let g = rect_glyph(0, 0, 1000, 1000);
        let err = raster_try(&g, 1000, 100_000.0).unwrap_err();
        assert!(matches!(err, RasterError::SizeExceeded { .. }), "got {err:?}");
    }

    #[test]
    fn shallow_edge_exercises_multi_column_ramp() {
        // A wide, short right triangle (0,0)-(200,0)-(0,40): its hypotenuse has a
        // shallow slope (Δx/Δy = 5), so within one scanline it crosses ~5 columns.
        // This is the ONLY geometry that drives Raster::line's multi-column area
        // ramp (the x1i >= x0i+3 branch with the a1/a2 loop) — the exact-coverage
        // rect/edge tests only hit the single-column path. A correct ramp lays a
        // strictly-monotonic AA gradient across ≥3 columns; a mis-distributed ramp
        // (wrong a1/a2 coefficient) would break monotonicity or collapse it.
        let tri = SimpleGlyph {
            header: GlyphHeader { x_min: 0, y_min: 0, x_max: 200, y_max: 40 },
            end_pts_of_contours: vec![2],
            instructions: vec![],
            points: vec![
                GlyphPoint { x: 0, y: 0, on_curve: true },
                GlyphPoint { x: 200, y: 0, on_curve: true },
                GlyphPoint { x: 0, y: 40, on_curve: true },
            ],
        };
        let cov = raster(&tri, 200, 200.0);
        let row = cov.height / 2;
        let partials: Vec<u8> = (0..cov.width)
            .map(|x| cov.at(x, row))
            .filter(|&a| a > 0 && a < 255)
            .collect();
        assert!(
            partials.len() >= 3,
            "shallow edge must spread AA over >=3 columns, got {partials:?}",
        );
        assert!(
            partials.windows(2).all(|w| w[0] >= w[1]),
            "multi-column ramp must be monotonic, got {partials:?}",
        );
    }

    #[test]
    #[allow(clippy::cast_precision_loss)] // ink sums small, well under 2^52.
    fn composite_offset_places_each_component() {
        // A composite of two non-overlapping copies of one rect: total ink is
        // ~2x a single copy, and the bitmap spans wider than one copy. Proves a
        // component's XY offset actually displaces its subglyph in the assembly.
        let rect = Glyph::Simple(rect_glyph(0, 0, 500, 1000)); // 50x100px at 0.1 scale
        let one = composite(vec![xy_component(0, 0, 0, ComponentTransform::Identity)]);
        let two = composite(vec![
            xy_component(0, 0, 0, ComponentTransform::Identity),
            xy_component(0, 800, 0, ComponentTransform::Identity), // +80px → gap, no overlap
        ]);
        let glyphs = vec![rect, one, two];
        let single = raster_composite(&glyphs, 1, 1000, 100.0).unwrap();
        let pair = raster_composite(&glyphs, 2, 1000, 100.0).unwrap();
        let ratio = pair.ink_sum() as f64 / single.ink_sum() as f64;
        assert!((1.9..=2.1).contains(&ratio), "two copies ink ~2x, got {ratio}");
        assert!(pair.width > single.width, "the offset copy widens the bitmap");
    }

    #[test]
    #[allow(clippy::cast_precision_loss)] // ink sums small, well under 2^52.
    fn composite_scale_transform_enlarges_subglyph() {
        // A 1.5x-scaled component covers 1.5^2 = 2.25x the area → ~2.25x the ink,
        // and a larger bitmap. Exercises the F2DOT14 scale-transform path.
        let rect = Glyph::Simple(rect_glyph(0, 0, 500, 500));
        let plain = composite(vec![xy_component(0, 0, 0, ComponentTransform::Identity)]);
        let scaled = composite(vec![xy_component(
            0,
            0,
            0,
            ComponentTransform::Scale { scale: 0x6000 }, // 24576 / 16384 = 1.5
        )]);
        let glyphs = vec![rect, plain, scaled];
        let a = raster_composite(&glyphs, 1, 1000, 80.0).unwrap();
        let b = raster_composite(&glyphs, 2, 1000, 80.0).unwrap();
        let ratio = b.ink_sum() as f64 / a.ink_sum() as f64;
        assert!((2.05..=2.45).contains(&ratio), "1.5x scale → ~2.25x ink, got {ratio}");
        assert!(b.width > a.width && b.height > a.height, "scaled bitmap is larger");
    }

    #[test]
    #[allow(clippy::cast_precision_loss)] // ink sums small, well under 2^52.
    fn composite_two_by_two_flip_preserves_area() {
        // A horizontal flip (2x2 matrix [-1 0; 0 1]) reverses contour winding;
        // the nonzero-winding `abs` keeps the fill, so ink is preserved. Drives
        // the full Matrix path AND a negative-determinant component.
        let rect = Glyph::Simple(rect_glyph(0, 0, 500, 700));
        let plain = composite(vec![xy_component(0, 0, 0, ComponentTransform::Identity)]);
        let flipped = composite(vec![xy_component(
            0,
            0,
            0,
            ComponentTransform::Matrix { xx: -16384, xy: 0, yx: 0, yy: 16384 }, // -1,0,0,1
        )]);
        let glyphs = vec![rect, plain, flipped];
        let a = raster_composite(&glyphs, 1, 1000, 64.0).unwrap();
        let b = raster_composite(&glyphs, 2, 1000, 64.0).unwrap();
        assert!(b.ink_sum() > 0, "flipped component still inks (nonzero winding)");
        let ratio = b.ink_sum() as f64 / a.ink_sum() as f64;
        assert!((0.95..=1.05).contains(&ratio), "flip preserves area, got {ratio}");
    }

    #[test]
    fn composite_nested_component_resolves() {
        // glyph 2 (composite) → glyph 1 (composite) → glyph 0 (simple rect):
        // two levels of recursion still produce ink.
        let rect = Glyph::Simple(rect_glyph(0, 0, 400, 800));
        let inner = composite(vec![xy_component(0, 100, 0, ComponentTransform::Identity)]);
        let outer = composite(vec![xy_component(1, 0, 200, ComponentTransform::Identity)]);
        let glyphs = vec![rect, inner, outer];
        let cov = raster_composite(&glyphs, 2, 1000, 64.0).unwrap();
        assert!(cov.ink_sum() > 0, "nested composite resolves to ink");
    }

    #[test]
    fn composite_self_reference_is_a_cycle() {
        // glyph 0 references itself → detected at the top of the chain.
        let glyphs = vec![composite(vec![xy_component(0, 0, 0, ComponentTransform::Identity)])];
        assert_eq!(
            raster_composite(&glyphs, 0, 1000, 32.0),
            Err(RasterError::CompositeCycle(0)),
        );
    }

    #[test]
    fn composite_mutual_reference_is_a_cycle() {
        // 0 → 1 → 0: the back-reference to ancestor 0 is the cycle.
        let glyphs = vec![
            composite(vec![xy_component(1, 0, 0, ComponentTransform::Identity)]),
            composite(vec![xy_component(0, 0, 0, ComponentTransform::Identity)]),
        ];
        assert_eq!(
            raster_composite(&glyphs, 0, 1000, 32.0),
            Err(RasterError::CompositeCycle(0)),
        );
    }

    #[test]
    fn composite_point_match_is_unsupported() {
        // ARGS_ARE_XY_VALUES = 0 (point-matched placement) → fail-loud, not a
        // silently-misplaced subglyph.
        let rect = Glyph::Simple(rect_glyph(0, 0, 500, 500));
        let pm = Glyph::Composite(CompositeGlyph {
            header: GlyphHeader { x_min: 0, y_min: 0, x_max: 0, y_max: 0 },
            raw_body: Vec::new(),
            components: vec![Component {
                flags: 0,
                glyph_index: 0,
                args: ComponentArgs::PointMatch { parent: 0, child: 0 },
                transform: ComponentTransform::Identity,
            }],
            instructions: Vec::new(),
        });
        let glyphs = vec![rect, pm];
        assert_eq!(
            raster_composite(&glyphs, 1, 1000, 32.0),
            Err(RasterError::PointMatchUnsupported(1)),
        );
    }

    #[test]
    fn composite_missing_component_reports_not_found() {
        // A component referencing a non-existent subglyph id is fail-loud.
        let glyphs = vec![composite(vec![xy_component(5, 0, 0, ComponentTransform::Identity)])];
        assert_eq!(
            raster_composite(&glyphs, 0, 1000, 32.0),
            Err(RasterError::GlyphNotFound(5)),
        );
    }
}
