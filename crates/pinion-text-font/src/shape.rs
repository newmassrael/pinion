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
//! (2) GSUB **substitution** ([`Font::substitute_glyphs`], [`crate::tables::gsub`]):
//! `ccmp` single substitution then `liga` ligature collapse (f + i → ﬁ); (3) GPOS
//! `kern` **pair kerning** ([`Font::kern_x_advance`], [`crate::tables::gpos`])
//! refines the advance between adjacent glyphs, with an hmtx-advance pen;
//! (4) GPOS `mark` **mark-to-base attachment** ([`Font::mark_offset`]) places a
//! combining mark at its anchor over the preceding base, and `mkmk`
//! **mark-to-mark stacking** ([`Font::mark_mark_offset`]) piles a further mark
//! over the preceding mark, lifting each off the baseline (the source of a
//! glyph's non-zero `y`). GSUB before GPOS, per the OpenType pipeline. This
//! mirrors the simple → composite raster split (R50.8 → R50.8.x) — a foundation
//! refined incrementally.
//!
//! Deliberately NOT yet handled (each is honest deferral, not a silent gap):
//! GSUB multiple / alternate / contextual / reverse-chaining (only Lookup Types 1
//! single substitution and 4 ligature substitution are applied); GPOS single / cursive /
//! mark-to-ligature / contextual positioning (only Lookup Types 2 pair kerning,
//! 4 mark-to-base, and 6 mark-to-mark are applied); `lookupFlag` glyph filtering
//! and the rest of GDEF (marks are recognised from the GDEF `GlyphClassDef`, but
//! kern/liga still apply to every adjacent run; a mark attaches to its immediately
//! preceding base, a stacking mark to the preceding mark);
//! script segmentation (§5.37.5); BIDI reorder (§5.37.4 lives
//! in a separate crate — [`crate::paragraph`] wires it in, `shape_run` itself
//! assumes a single direction); grapheme clustering (iteration is per
//! codepoint); and line breaking (§5.37.7 — a single line is assumed).
//! Production paint is still §5.36 swash; [`render_run`] is a test
//! forcing-consumer until paint wiring lands (the §5.37.9 glyph atlas it composes
//! through already has).
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
pub(crate) const ATLAS_WIDTH: usize = 256;

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

    // Stage 2 — GSUB substitution: `ccmp` single substitution then `liga`
    // ligature collapse. Each output is (glyph, origin) where origin indexes into
    // `raw_glyphs`/clusters (a ligature carries its first component's cluster).
    // Identity for a font with no GSUB / neither feature, so the cmap+hmtx
    // baseline is preserved.
    let substituted = font.substitute_glyphs(&raw_glyphs);

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

    // Stage 4 — GPOS mark attachment: place each combining mark at its anchor
    // relative to a preceding glyph, overriding the mark's running-pen x and
    // lifting it off the baseline (y). Font design units are y-up while the pen is
    // y-down, so the vertical anchor delta is negated. Combining marks carry ~0
    // hmtx advance, so the pen accumulated above stays correct for following
    // glyphs. Two passes are folded into one left-to-right walk:
    //
    // * mark-to-mark (`mkmk`, Lookup Type 6) takes precedence: a mark that stacks
    //   on the *preceding mark* (`prev_mark`) is placed against that mark's already
    //   resolved position, so stacking diacritics pile up correctly. This overrides
    //   mark-to-base for a mark covered by both (mkmk runs after mark in OpenType).
    // * mark-to-base (`mark`, Lookup Type 4): otherwise a mark attaches to the
    //   preceding base (`last_base`).
    //
    // `prev_mark` is the most recently positioned mark (a stacking reference for the
    // next); a base resets it (a new cluster). Marks are recognised from GDEF
    // ([`Font::is_mark`], §5.37.6): a glyph the font declares a combining mark is
    // never taken as a base even when it declared no anchor here, so a following
    // mark still attaches to the real base instead of to a stray accent. A font
    // without GDEF falls back to attach-based recognition (a glyph that attaches is
    // the mark, anything else a base) — the pre-GDEF behaviour. `lookupFlag`
    // mark-attachment-class filtering remains R50.6.x.
    let mut last_base: Option<usize> = None;
    let mut prev_mark: Option<usize> = None;
    for i in 0..glyphs.len() {
        let mark = glyphs[i].glyph_id;
        // mark-to-mark first (override): stack on the preceding mark if `mkmk` covers it.
        let stacked = prev_mark.and_then(|pm| {
            let (pm_x, pm_y) = (glyphs[pm].x, glyphs[pm].y);
            font.mark_mark_offset(glyphs[pm].glyph_id, mark)
                .map(|(dx, dy)| (pm_x, pm_y, dx, dy))
        });
        if let Some((pm_x, pm_y, dx, dy)) = stacked {
            glyphs[i].x = pm_x + f32::from(dx) * scale;
            glyphs[i].y = pm_y - f32::from(dy) * scale;
            prev_mark = Some(i); // this mark becomes the reference for the next
            continue;
        }
        // mark-to-base: attach to the preceding base.
        let attached = last_base.and_then(|bi| {
            let (base_glyph, base_x) = (glyphs[bi].glyph_id, glyphs[bi].x);
            font.mark_offset(base_glyph, mark)
                .map(|(dx, dy)| (base_x, dx, dy))
        });
        if let Some((base_x, dx, dy)) = attached {
            glyphs[i].x = base_x + f32::from(dx) * scale;
            glyphs[i].y = -f32::from(dy) * scale;
            prev_mark = Some(i); // an attached mark is a stacking reference
        } else if font.is_mark(mark) {
            // A GDEF mark that declared no anchor here is still a mark, not a base:
            // keep `last_base` (a following mark attaches to the real base) and let
            // this mark be a stacking reference for any `mkmk` that follows.
            prev_mark = Some(i);
        } else {
            last_base = Some(i);
            prev_mark = None; // a base starts a fresh mark cluster
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
    // Single font (stack of one), so every glyph is atlas index 0 and the pen is
    // the shaped baseline-relative origin (mark y carried through).
    render_glyphs(
        &[font],
        px_per_em,
        run.glyphs.iter().map(|g| GlyphDraw {
            font_index: 0,
            glyph_id: g.glyph_id,
            pen_x: g.x,
            pen_y: g.y,
        }),
    )
}

/// A glyph ready for compositing: its integer-snapped pen origin, the atlas that
/// holds its pixels, and its packed sub-rect. Built by `render_glyphs_atlased`
/// (the one place all renderers rasterize + place), consumed by `composite`
/// (whole-paragraph mask) and — public since R1065 — by a per-glyph paint path
/// that draws one quad sampling [`Self::glyph`]'s sub-rect of `atlases[atlas]`.
#[derive(Debug, Clone, Copy)]
pub struct PlacedGlyph {
    /// Pen-origin x in the combined bitmap (device px, integer-snapped).
    pub pen_x: i32,
    /// Pen-origin y (device px; baseline = 0, negative above, positive downward
    /// for a combining mark).
    pub pen_y: i32,
    /// Index into [`RenderedGlyphs::atlases`] of the atlas holding this glyph's
    /// pixels.
    pub atlas: usize,
    /// The glyph's packed sub-rect and pen offset in `atlases[atlas]`.
    pub glyph: AtlasGlyph,
}

/// The pre-composite output of `render_glyphs_atlased`: one [`GlyphAtlas`] per
/// stack font plus every drawn glyph's placement into them.
///
/// This is the §5.37.9 atlas surface a per-glyph GPU text path paints — each
/// atlas uploaded once, each [`PlacedGlyph`] drawn as one quad sampling its
/// sub-rect — *before* `composite` flattens it into a single whole-paragraph
/// [`Coverage`]. R1063 wired the flattened mask to pixels (a bring-up seam);
/// R1065 exposes this un-flattened form so paint can keep the atlas.
#[derive(Debug, Clone, Default)]
pub struct RenderedGlyphs {
    /// One [`GlyphAtlas`] per stack font (so a glyph id shared across fonts never
    /// collides); index with [`PlacedGlyph::atlas`].
    pub atlases: Vec<GlyphAtlas>,
    /// Every drawn (non-blank) glyph, integer-snapped, in input order.
    pub placed: Vec<PlacedGlyph>,
}

/// Composite placed glyphs into one AA coverage bitmap by same-color alpha-over.
///
/// Each [`PlacedGlyph`] carries its integer-snapped pen origin (device px), the
/// index of the [`GlyphAtlas`] in `atlases` that rasterized it, and its packed
/// sub-rect. Returns [`Coverage::empty`] when nothing is placed (an empty or
/// all-blank run). Called only by [`render_glyphs`] — the shared core under all
/// three renderers — so the bounding-box union and the alpha-over are one SSOT.
pub(crate) fn composite(atlases: &[GlyphAtlas], placed: &[PlacedGlyph]) -> Coverage {
    if placed.is_empty() {
        return Coverage::empty();
    }

    // Union bounding box in combined-bitmap space. A glyph's pixels occupy
    // columns [pen_x + left, ...) and rows [pen_y + top, pen_y + top + height)
    // — baseline = row 0, `top` negative above it, `pen_y` the mark's vertical
    // attachment offset (0 for baseline glyphs).
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    // each glyph width/height <= MAX_DIM (4096), exact in i32.
    let (min_x, min_y, max_x, max_y) = placed.iter().fold(
        (i32::MAX, i32::MAX, i32::MIN, i32::MIN),
        |(min_x, min_y, max_x, max_y), p| {
            let g = &p.glyph;
            let (gx, gy) = (p.pen_x + g.left, p.pen_y + g.top);
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
    for p in placed {
        // Read pixels from the atlas that rasterized this glyph (per-font for
        // multi-font runs; the single atlas for `render_run`).
        let atlas = &atlases[p.atlas];
        let (atlas_w, atlas_alpha) = (atlas.width(), atlas.alpha());
        let g = &p.glyph;
        #[allow(clippy::cast_sign_loss)]
        // pen_x+left >= min_x, pen_y+top >= min_y by the fold above.
        let (off_x, off_y) = (
            (p.pen_x + g.left - min_x) as usize,
            (p.pen_y + g.top - min_y) as usize,
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
    Coverage {
        width,
        height,
        left: min_x,
        top: min_y,
        alpha,
    }
}

/// One glyph to draw: which stack font shaped it, its id, and its pen origin in
/// the combined bitmap (device px; the caller folds in any per-line baseline). The
/// per-glyph input to [`render_glyphs`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct GlyphDraw {
    /// Index into the font stack of the font that shaped this glyph.
    pub font_index: usize,
    /// Glyph id, valid in `fonts[font_index]`.
    pub glyph_id: u16,
    /// Pen-origin x in the combined bitmap (device px, pre-snap).
    pub pen_x: f32,
    /// Pen-origin y in the combined bitmap (device px, pre-snap; the caller has
    /// already added any line baseline to the glyph's baseline-relative y).
    pub pen_y: f32,
}

/// Rasterize each glyph through its stack font's atlas and composite them into one
/// AA coverage mask — the single render core under [`render_run`],
/// [`crate::paragraph::render_paragraph`], and
/// [`crate::line_layout::render_lines`]. One [`GlyphAtlas`] per stack font (so a
/// glyph id shared across fonts never collides), each glyph rasterized by
/// `fonts[font_index]`, blank glyphs (space) skipped, pens integer-snapped, then
/// [`composite`]d. A glyph whose `font_index` is outside `fonts` is skipped
/// (defensive). Returns [`Coverage::empty`] for an empty stack or no drawn glyph.
///
/// # Errors
///
/// Propagates a [`RasterError`] from any glyph's rasterization (a pathological
/// `px_per_em` past the size cap, or a not-yet-supported composite glyph).
pub(crate) fn render_glyphs(
    fonts: &[&Font],
    px_per_em: f32,
    glyphs: impl IntoIterator<Item = GlyphDraw>,
) -> Result<Coverage, RasterError> {
    let rendered = render_glyphs_atlased(fonts, px_per_em, glyphs)?;
    Ok(composite(&rendered.atlases, &rendered.placed))
}

/// Rasterize each glyph through its stack font's atlas and place it — the atlas
/// build under [`render_glyphs`], stopping *before* [`composite`] flattens the
/// per-glyph atlas into one mask. Returns the [`RenderedGlyphs`] (atlases +
/// per-glyph placements) the §5.37.9 paint path consumes (R1065).
///
/// One [`GlyphAtlas`] per stack font (so a glyph id shared across fonts never
/// collides), each glyph rasterized by `fonts[font_index]`, blank glyphs (space)
/// skipped, pens integer-snapped. A glyph whose `font_index` is outside `fonts`
/// is skipped (defensive). Empty atlases / placements for an empty stack or no
/// drawn glyph.
///
/// # Errors
///
/// Propagates a [`RasterError`] from any glyph's rasterization (a pathological
/// `px_per_em` past the size cap, or a not-yet-supported composite glyph).
pub(crate) fn render_glyphs_atlased(
    fonts: &[&Font],
    px_per_em: f32,
    glyphs: impl IntoIterator<Item = GlyphDraw>,
) -> Result<RenderedGlyphs, RasterError> {
    // One atlas per stack font; an empty stack yields no atlases (every glyph is
    // then out of range and skipped, so the result has no placements).
    let mut atlases: Vec<GlyphAtlas> = (0..fonts.len())
        .map(|_| GlyphAtlas::new(ATLAS_WIDTH))
        .collect();
    let mut placed: Vec<PlacedGlyph> = Vec::new();
    for d in glyphs {
        if d.font_index >= fonts.len() {
            continue; // font_index past the stack (caller misuse) — drop, don't panic.
        }
        let entry =
            atlases[d.font_index].get_or_insert(fonts[d.font_index], d.glyph_id, px_per_em)?;
        if entry.width == 0 {
            continue; // blank glyph (space): advances the pen, packs nothing.
        }
        #[allow(clippy::cast_possible_truncation)]
        // pen x/y are finite accumulations; round() snaps onto the raster grid.
        let (pen_x, pen_y) = (d.pen_x.round() as i32, d.pen_y.round() as i32);
        placed.push(PlacedGlyph {
            pen_x,
            pen_y,
            atlas: d.font_index,
            glyph: entry,
        });
    }
    Ok(RenderedGlyphs { atlases, placed })
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
