//! R50.6 §5.37.6 — text-run shaping (cmap + GSUB ligatures + GPOS kerning).
//!
//! The layer above the glyph rasterizer (§5.37.8): turns a Unicode `&str` into a
//! sequence of positioned glyphs, then composites them into one anti-aliased
//! coverage bitmap. This is the first time the self-hosted text engine renders a
//! whole *string* to pixels (the rasterizer renders one glyph by id).
//!
//! # Scope — cmap + GSUB ligatures + GPOS kern & mark-to-base (R50.6 / .1 / .2, R50.7)
//!
//! Four-stage shaping: (1) cmap codepoint → glyph ([`Font::glyph_id_for`]);
//! (2) GSUB `liga` **ligature substitution** ([`Font::substitute_ligatures`],
//! [`crate::tables::gsub`]) collapses component sequences (f + i → ﬁ); (3) GPOS
//! `kern` **pair kerning** ([`Font::kern_x_advance`], [`crate::tables::gpos`])
//! refines the advance between adjacent glyphs, with an hmtx-advance pen;
//! (4) GPOS `mark` **mark-to-base attachment** ([`Font::mark_offset`]) places a
//! combining mark at its anchor over the preceding base, lifting it off the
//! baseline (the source of a glyph's non-zero `y`). GSUB before GPOS, per the
//! OpenType pipeline. This mirrors the simple → composite raster split
//! (R50.8 → R50.8.x) — a foundation refined incrementally.
//!
//! Deliberately NOT yet handled (each is honest deferral, not a silent gap):
//! GSUB single / multiple / alternate / contextual / reverse-chaining (only
//! Lookup Type 4 ligature substitution is applied); GPOS single / cursive /
//! mark-to-ligature / mark-to-mark / contextual positioning (only Lookup Types 2
//! pair kerning and 4 mark-to-base are applied); `lookupFlag` glyph filtering
//! (kern/liga apply to every adjacent run; a mark attaches to its immediately
//! preceding base); script segmentation (§5.37.5); BIDI reorder (§5.37.4 lives
//! in a separate crate — [`crate::paragraph`] wires it in, `shape_run` itself
//! assumes a single direction); grapheme clustering (iteration is per
//! codepoint); and line breaking (§5.37.7 — a single line is assumed).
//! Production paint is still §5.36 swash; [`render_run`] is a test
//! forcing-consumer until the GPU atlas + paint wiring land.
//!
//! # Determinism
//!
//! [`shape_run`] keeps exact `f32` pen positions; [`render_run`] integer-snaps
//! each glyph's pen origin onto the raster grid before compositing, realising
//! §5.37's "sub-pixel integer-snap (비결정 제거)" — the placement is fully
//! deterministic, never a fractional-coordinate AA shimmer.

use crate::Font;
use crate::atlas::{AtlasGlyph, GlyphAtlas};
use crate::raster::{Coverage, RasterError};

/// Shelf-wrap width of the per-run glyph atlas. Output is independent of it (the
/// composite is sized from each glyph's pen offset, not the atlas layout); it
/// only bounds how wide a packing row grows before wrapping.
const ATLAS_WIDTH: usize = 256;

/// One glyph in a shaped run, placed on the horizontal baseline.
///
/// `x` is the pen-origin x (device px, baseline-relative) — the exact `f32`
/// advance accumulation, **not** yet snapped to the raster grid ([`render_run`]
/// integer-snaps for a deterministic bitmap). The origin is run-relative from
/// [`shape_run`] and paragraph-relative from
/// [`crate::paragraph::shape_paragraph`]. `cluster` is the byte offset of the
/// source codepoint (into the run `&str` from [`shape_run`], into the whole
/// paragraph from `shape_paragraph` — where it descends across a right-to-left
/// run), so a caller can map a glyph back to its text (hit-testing / selection).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionedGlyph {
    /// Resolved glyph id (`.notdef` = 0 for an unmapped codepoint).
    pub glyph_id: u16,
    /// Pen-origin x (device px, baseline-relative, pre-snap).
    pub x: f32,
    /// Pen-origin y (device px, baseline-relative, **positive downward**; 0 on
    /// the baseline). Non-zero only for a GPOS-attached combining mark
    /// (§5.37.6 mark-to-base) — base glyphs and unmarked text stay on the
    /// baseline.
    pub y: f32,
    /// Byte offset of the source codepoint within the shaped `&str`.
    pub cluster: usize,
    /// Index of the font (in the shaping font stack) that shaped this glyph and
    /// in whose `glyph_id` space it is valid. Always 0 for single-font shaping
    /// ([`shape_run`] / [`crate::shape_paragraph`]); the resolved stack index for
    /// multi-font fallback ([`crate::fallback::shape_with_fallback`] /
    /// [`crate::shape_paragraph_with_fallback`]).
    pub font_index: usize,
}

/// A shaped text run: the positioned glyphs plus the total pen advance.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShapedRun {
    /// Glyphs in logical order (= visual order at this baseline scope), `x`
    /// ascending.
    pub glyphs: Vec<PositionedGlyph>,
    /// Total advance width of the run (device px) — the pen position after the
    /// last glyph.
    pub advance: f32,
}

/// Shape `text` into a positioned glyph run at `px_per_em` (§5.37.6: cmap
/// codepoint → glyph, GSUB `liga` ligature substitution, then a left-to-right
/// hmtx-advance pen refined by GPOS `kern` pair positioning; no BIDI reorder,
/// no grapheme clustering — see the module scope).
///
/// An unmapped codepoint resolves to glyph 0 (`.notdef`) per the OpenType
/// convention. A ligature carries its first component's `cluster` (so the run's
/// glyphs may be fewer than the input codepoints). A degenerate font
/// (`units_per_em == 0`) or a non-finite / `<= 0` `px_per_em` produces glyphs at
/// the origin with zero advance, never a panic or `NaN` pen.
#[must_use]
pub fn shape_run(font: &Font, text: &str, px_per_em: f32) -> ShapedRun {
    let upem = font.units_per_em();
    let scale = if upem == 0 || !px_per_em.is_finite() || px_per_em <= 0.0 {
        0.0
    } else {
        px_per_em / f32::from(upem)
    };

    // Stage 1 — cmap: each source codepoint → glyph id, tracking its byte
    // cluster. An unmapped codepoint resolves to glyph 0 (.notdef).
    let mut raw_glyphs: Vec<u16> = Vec::new();
    let mut raw_clusters: Vec<usize> = Vec::new();
    for (cluster, ch) in text.char_indices() {
        raw_glyphs.push(font.glyph_id_for(ch as u32).unwrap_or(0));
        raw_clusters.push(cluster);
    }

    // Stage 2 — GSUB substitution: collapse `liga` ligature sequences. Each
    // output is (glyph, origin) where origin indexes into `raw_glyphs`/clusters
    // (the ligature carries its first component's cluster). Identity for a font
    // with no GSUB / no `liga` feature, so the cmap+hmtx baseline is preserved.
    let substituted = font.substitute_ligatures(&raw_glyphs);

    // Stage 3 — GPOS positioning: accumulate the pen left to right, folding the
    // `kern` adjustment between adjacent glyphs (design units, same scale as the
    // hmtx advance). 0 kern keeps the exact cmap+hmtx baseline.
    let mut glyphs = Vec::with_capacity(substituted.len());
    let mut pen = 0.0_f32;
    let mut prev_glyph: Option<u16> = None;
    for (glyph_id, origin) in substituted {
        if let Some(prev) = prev_glyph {
            let kern_units = font.kern_x_advance(prev, glyph_id);
            pen += f32::from(kern_units) * scale;
        }
        glyphs.push(PositionedGlyph {
            glyph_id,
            x: pen,
            y: 0.0,
            cluster: raw_clusters[origin],
            font_index: 0,
        });
        let advance_units = font.glyph_advance_width(glyph_id).unwrap_or(0);
        pen += f32::from(advance_units) * scale;
        prev_glyph = Some(glyph_id);
    }

    // Stage 4 — GPOS mark-to-base attachment: place each combining mark at its
    // anchor relative to the preceding base glyph, overriding the mark's
    // running-pen x and lifting it off the baseline (y). Font design units are
    // y-up while the pen is y-down, so the vertical anchor delta is negated.
    // Combining marks carry ~0 hmtx advance, so the pen accumulated above stays
    // correct for following glyphs. A positioned mark is not itself a base for
    // later marks — mark-to-mark stacking (Lookup Type 6) is R50.6.x.
    let mut last_base: Option<usize> = None;
    for i in 0..glyphs.len() {
        let mark = glyphs[i].glyph_id;
        let attached = last_base.and_then(|bi| {
            let (base_glyph, base_x) = (glyphs[bi].glyph_id, glyphs[bi].x);
            font.mark_offset(base_glyph, mark)
                .map(|(dx, dy)| (base_x, dx, dy))
        });
        if let Some((base_x, dx, dy)) = attached {
            glyphs[i].x = base_x + f32::from(dx) * scale;
            glyphs[i].y = -f32::from(dy) * scale;
        } else {
            last_base = Some(i);
        }
    }

    ShapedRun {
        glyphs,
        advance: pen,
    }
}

/// Shape `text` and composite every glyph's AA coverage into one bitmap, each at
/// its integer-snapped pen position — the first time §5.37 turns a string into
/// pixels. Returns [`Coverage::empty`] for an empty or all-blank run (e.g. only
/// spaces). Glyphs are rasterized once through a per-run [`GlyphAtlas`]
/// (§5.37.9), so a repeated glyph is a cache hit rather than a re-rasterization.
///
/// Glyph coverages are combined by **same-color alpha-over** (`a + b - a·b`), the
/// correct union for masks that will be filled with one text color:
/// non-overlapping glyphs add, and an overlapping AA fringe does not
/// double-darken (a plain `max` would under-fill, a plain `+` would overshoot).
///
/// # Errors
///
/// Propagates a [`RasterError`] from any glyph's rasterization (a pathological
/// `px_per_em` past the size cap, or a malformed / not-yet-supported composite).
pub fn render_run(font: &Font, text: &str, px_per_em: f32) -> Result<Coverage, RasterError> {
    let run = shape_run(font, text, px_per_em);

    // Rasterize each glyph at most once through a per-run atlas (a repeated glyph
    // is a cache hit), remembering its integer-snapped pen x. Blank glyphs (space)
    // advance the pen but pack nothing (width 0).
    let mut atlas = GlyphAtlas::new(ATLAS_WIDTH);
    let mut placed: Vec<(i32, i32, AtlasGlyph)> = Vec::new();
    for glyph in &run.glyphs {
        let entry = atlas.get_or_insert(font, glyph.glyph_id, px_per_em)?;
        if entry.width == 0 {
            continue;
        }
        #[allow(clippy::cast_possible_truncation)]
        // pen x/y are finite accumulations; round() snaps onto the raster grid
        // for deterministic placement. pen_y is non-zero only for an attached
        // mark (baseline-relative, positive downward).
        let (pen_x, pen_y) = (glyph.x.round() as i32, glyph.y.round() as i32);
        placed.push((pen_x, pen_y, entry));
    }
    if placed.is_empty() {
        return Ok(Coverage::empty());
    }

    // Union bounding box in combined-bitmap space. A glyph's pixels occupy
    // columns [pen_x + left, ...) and rows [pen_y + top, pen_y + top + height)
    // — baseline = row 0, `top` negative above it, `pen_y` the mark's vertical
    // attachment offset (0 for baseline glyphs).
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    // each glyph width/height <= MAX_DIM (4096), exact in i32.
    let (min_x, min_y, max_x, max_y) = placed.iter().fold(
        (i32::MAX, i32::MAX, i32::MIN, i32::MIN),
        |(min_x, min_y, max_x, max_y), (pen_x, pen_y, g)| {
            let (gx, gy) = (pen_x + g.left, pen_y + g.top);
            (
                min_x.min(gx),
                min_y.min(gy),
                max_x.max(gx + g.width as i32),
                max_y.max(gy + g.height as i32),
            )
        },
    );

    #[allow(clippy::cast_sign_loss)] // max_* > min_* (each placed glyph adds positive extent).
    let (width, height) = ((max_x - min_x) as usize, (max_y - min_y) as usize);
    let mut alpha = vec![0u8; width * height];
    let (atlas_w, atlas_alpha) = (atlas.width(), atlas.alpha());
    for (pen_x, pen_y, g) in &placed {
        #[allow(clippy::cast_sign_loss)]
        // pen_x+left >= min_x, pen_y+top >= min_y by the fold above.
        let (off_x, off_y) = (
            (pen_x + g.left - min_x) as usize,
            (pen_y + g.top - min_y) as usize,
        );
        for gy in 0..g.height {
            let dst_row = (off_y + gy) * width + off_x;
            let src_row = (g.y + gy) * atlas_w + g.x;
            for gx in 0..g.width {
                let src = atlas_alpha[src_row + gx];
                if src != 0 {
                    let dst = &mut alpha[dst_row + gx];
                    *dst = over(*dst, src);
                }
            }
        }
    }
    Ok(Coverage {
        width,
        height,
        left: min_x,
        top: min_y,
        alpha,
    })
}

/// Same-color alpha-over of two `0..=255` coverage values: `out = d + s − d·s`,
/// in `u8` space. `over(x, 0) = over(0, x) = x` (a clear pixel takes the other),
/// `over(255, _) = over(_, 255) = 255` (a full pixel stays full); monotone and
/// always `<= 255`, so no clamping or overflow.
#[inline]
#[allow(clippy::cast_possible_truncation)] // result is in 0..=255 (see invariants above).
fn over(d: u8, s: u8) -> u8 {
    let ds = (u16::from(d) * u16::from(s) + 127) / 255; // round(d·s / 255)
    (u16::from(d) + u16::from(s) - ds) as u8
}

#[cfg(test)]
mod tests {
    use super::over;

    #[test]
    fn over_identity_and_saturation() {
        for x in [0u8, 1, 40, 128, 200, 254, 255] {
            assert_eq!(over(x, 0), x, "clear source leaves dest unchanged");
            assert_eq!(over(0, x), x, "clear dest takes source");
            assert_eq!(over(255, x), 255, "full dest stays full");
            assert_eq!(over(x, 255), 255, "full source fills");
        }
    }

    #[test]
    fn over_is_monotone_and_bounded() {
        // Union semantics: >= each operand, never overshoots d+s, AND
        // non-decreasing in the source (more coverage never lowers the result).
        for d in (0u16..=255).step_by(17) {
            let mut prev = 0u8;
            for s in 0u16..=255 {
                #[allow(clippy::cast_possible_truncation)]
                let o = over(d as u8, s as u8);
                assert!(
                    u16::from(o) >= d && u16::from(o) >= s,
                    "over >= each operand"
                );
                assert!(u16::from(o) <= d + s, "over <= d + s (no overshoot)");
                assert!(o >= prev, "over(d, ·) non-decreasing in s: {o} < {prev}");
                prev = o;
            }
        }
    }
}
