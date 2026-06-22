//! R50.10 §5.37.6 — paragraph shaping: itemise → shape each run → place in
//! visual order.
//!
//! The first cross-layer integration of the self-hosted engine: this is the
//! R50.10 point where `pinion-text-font` consumes `pinion-text-unicode`. It
//! takes a single logical-order paragraph and produces glyphs in **visual**
//! (left-to-right display) order with absolute pen positions, by:
//!
//! 1. itemising into runs uniform in BIDI level and script
//!    ([`pinion_text_unicode::itemize()`], §5.37.4 + §5.37.5);
//! 2. ordering those runs for display via UAX #9 L2
//!    ([`pinion_text_unicode::reorder_visual`] over the per-run levels);
//! 3. shaping each run's substring ([`shape_run`], cmap → GSUB → GPOS); and
//! 4. placing runs left-to-right, reversing glyph order within right-to-left
//!    runs so the logically-first glyph sits at the run's right edge.
//!
//! Kerning/ligatures are applied in logical order (inside [`shape_run`]) and the
//! run is then reversed for display — the OpenType / UAX #9 order, matching
//! `HarfBuzz`.
//!
//! Scope (honest deferrals, not silent gaps): a single paragraph (the caller
//! splits on hard breaks via [`pinion_text_unicode::bidi::iter_paragraphs`]);
//! L3 combining-mark reordering and per-script shaper selection are deferred.
//! **Explicit directional-format controls (the X9-removed RLE/LRE/RLO/LRO/PDF
//! and BN codes) are not yet filtered before the L2 run reorder**, so a
//! paragraph containing them may reorder incorrectly and emits a glyph for the
//! control itself — `bidi_reorder` filters these per-codepoint (its R51.24.1
//! fix), but doing so at run granularity here needs per-codepoint BN awareness
//! and is a follow-up. The modern isolate controls (LRI/RLI/FSI/PDI) are *not*
//! X9-removed and reorder correctly today. Production paint is still §5.36
//! swash, so [`shape_paragraph`] is a test forcing-consumer until the paint
//! path wires the self-hosted shaper.

use crate::Font;
use crate::shape::{PositionedGlyph, shape_run};
use pinion_text_unicode::{itemize, reorder_visual};

/// A shaped paragraph: glyphs in visual (left-to-right) order with absolute
/// pen-origin positions, plus the total advance.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShapedParagraph {
    /// Glyphs in visual order, `x` ascending. `x` is the pen-origin x in device
    /// px relative to the paragraph origin; `cluster` is the byte offset of the
    /// source codepoint within the whole paragraph (so it is descending across a
    /// right-to-left run).
    pub glyphs: Vec<PositionedGlyph>,
    /// Total advance width of the paragraph (device px).
    pub advance: f32,
}

/// Shape `paragraph` at `px_per_em` into visually-ordered positioned glyphs
/// (§5.37.6: itemise → shape per run → place runs in UAX #9 L2 visual order,
/// reversing glyphs within right-to-left runs). Expects a single paragraph; an
/// empty string yields no glyphs and zero advance.
#[must_use]
pub fn shape_paragraph(font: &Font, paragraph: &str, px_per_em: f32) -> ShapedParagraph {
    let runs = itemize(paragraph);
    if runs.is_empty() {
        return ShapedParagraph::default();
    }

    // UAX #9 L2 at run granularity: each run is single-level, so reordering the
    // per-run level array yields the left-to-right display order of runs.
    let run_levels: Vec<u8> = runs.iter().map(|r| r.level).collect();
    let visual_order = reorder_visual(&run_levels);

    let mut glyphs: Vec<PositionedGlyph> = Vec::new();
    let mut pen = 0.0_f32;
    for &ri in &visual_order {
        let run = runs[ri];
        let shaped = shape_run(font, &paragraph[run.start..run.end], px_per_em);
        let n = shaped.glyphs.len();
        if run.is_rtl() {
            // Mirror within the run for display: the logically-last glyph is
            // leftmost. A glyph spans `[g.x, next_x)` in the logical box
            // (`next_x` = the next glyph's pen origin, or the run advance for
            // the last logical glyph), so in the reversed box its visual origin
            // is `pen + (run_advance - next_x)`. Kerning is already folded into
            // the logical pen by `shape_run` and is preserved through the mirror.
            for i in (0..n).rev() {
                let g = shaped.glyphs[i];
                let next_x = if i + 1 < n {
                    shaped.glyphs[i + 1].x
                } else {
                    shaped.advance
                };
                glyphs.push(PositionedGlyph {
                    glyph_id: g.glyph_id,
                    x: pen + (shaped.advance - next_x),
                    cluster: run.start + g.cluster,
                });
            }
        } else {
            for g in &shaped.glyphs {
                glyphs.push(PositionedGlyph {
                    glyph_id: g.glyph_id,
                    x: pen + g.x,
                    cluster: run.start + g.cluster,
                });
            }
        }
        pen += shaped.advance;
    }

    ShapedParagraph {
        glyphs,
        advance: pen,
    }
}

#[cfg(test)]
mod tests {
    use super::{ShapedParagraph, shape_paragraph};
    use crate::Font;
    use crate::shape::shape_run;

    const NOTO: &str = "tests/fonts/NotoSans-Regular.ttf";

    fn font(path: &str) -> Font {
        let bytes = std::fs::read(path).expect("read font fixture");
        Font::from_bytes(bytes).expect("valid Font")
    }

    /// Visual order means `x` is non-decreasing across the whole paragraph.
    fn assert_x_ascending(p: &ShapedParagraph) {
        for pair in p.glyphs.windows(2) {
            assert!(
                pair[1].x >= pair[0].x,
                "visual order requires non-decreasing x: {} then {}",
                pair[0].x,
                pair[1].x
            );
        }
    }

    #[test]
    fn pure_ltr_matches_shape_run() {
        // An all-LTR single-script paragraph is one run, so paragraph shaping
        // must equal a plain `shape_run` (same glyphs, same positions, clusters
        // unchanged because the run starts at byte 0).
        let f = font(NOTO);
        let para = shape_paragraph(&f, "office", 32.0);
        let run = shape_run(&f, "office", 32.0);
        assert_eq!(para.glyphs, run.glyphs, "single LTR run == shape_run");
        assert!((para.advance - run.advance).abs() < 1e-4);
        assert_x_ascending(&para);
    }

    #[test]
    fn empty_paragraph_is_empty() {
        let f = font(NOTO);
        let para = shape_paragraph(&f, "", 32.0);
        assert!(para.glyphs.is_empty());
        assert!(para.advance.abs() < 1e-4);
    }

    #[test]
    fn rtl_run_reverses_glyph_clusters_visually() {
        // Hebrew alef-bet-gimel resolves to one RTL run (level 1). Whatever
        // glyphs the font maps, visual order must place the logically-LAST
        // codepoint leftmost, so clusters are DESCENDING in visual order — the
        // defining property of an RTL run. (Coverage-independent: even .notdef
        // carries a real hmtx advance.)
        let f = font(NOTO);
        let para = shape_paragraph(&f, "\u{05D0}\u{05D1}\u{05D2}", 32.0);
        assert_eq!(para.glyphs.len(), 3, "three Hebrew codepoints, no ligation");
        let clusters: Vec<usize> = para.glyphs.iter().map(|g| g.cluster).collect();
        assert_eq!(
            clusters,
            vec![4, 2, 0],
            "RTL clusters descend in visual order"
        );
        assert_x_ascending(&para);
        // Leftmost glyph sits at the paragraph origin.
        assert!(para.glyphs[0].x.abs() < 1e-4);
    }

    #[test]
    fn rtl_run_pins_exact_mirrored_x_values() {
        // Order/cluster assertions alone would miss a kern-aware-mirror bug.
        // Pin interior geometry: each visual glyph's x must equal
        // `run_advance - next_logical_pen_x`, derived from `shape_run`'s own
        // logical layout (coverage-independent — true for any glyph mapping).
        let f = font(NOTO);
        let text = "\u{05D0}\u{05D1}\u{05D2}";
        let para = shape_paragraph(&f, text, 32.0);
        let logical = shape_run(&f, text, 32.0);
        let n = logical.glyphs.len();
        for vg in &para.glyphs {
            let li = vg.cluster / 2; // each Hebrew codepoint is 2 UTF-8 bytes
            let next_x = if li + 1 < n {
                logical.glyphs[li + 1].x
            } else {
                logical.advance
            };
            let expected = logical.advance - next_x;
            assert!(
                (vg.x - expected).abs() < 1e-4,
                "cluster {} visual x {} != mirror expected {expected}",
                vg.cluster,
                vg.x
            );
        }
    }

    #[test]
    fn explicit_directional_controls_do_not_panic_known_limitation() {
        // X9-removed explicit embedding controls (RLE/LRE/RLO/LRO/PDF) are NOT
        // yet filtered before the L2 run reorder (see the module deferral), so
        // this pins only the guarantee we make today: no panic, coverage stays
        // well-formed (x non-decreasing, clusters in range). When BN filtering
        // lands this test gains exact reorder assertions.
        let f = font(NOTO);
        let text = "a\u{202A}b\u{202C}c"; // a LRE b PDF c
        let para = shape_paragraph(&f, text, 32.0);
        assert!(!para.glyphs.is_empty());
        assert_x_ascending(&para);
        for g in &para.glyphs {
            assert!(g.cluster < text.len(), "cluster {} out of range", g.cluster);
        }
    }

    #[test]
    fn mixed_ltr_rtl_orders_runs_then_mirrors_within() {
        // "ab" (Latin, level 0) + Hebrew alef-bet (level 1). Base level is LTR,
        // so the Latin run is leftmost (clusters 0,1 ascending) followed by the
        // Hebrew run mirrored (clusters 4,2 descending — bytes 2 and 4).
        let f = font(NOTO);
        let para = shape_paragraph(&f, "ab\u{05D0}\u{05D1}", 32.0);
        let clusters: Vec<usize> = para.glyphs.iter().map(|g| g.cluster).collect();
        assert_eq!(clusters, vec![0, 1, 4, 2], "LTR run then mirrored RTL run");
        assert_x_ascending(&para);
    }

    #[test]
    fn rtl_run_total_advance_matches_its_shape_run() {
        // Placing/mirroring must preserve the run's total advance: the whole
        // paragraph advance equals the sum of the per-run shaped advances.
        let f = font(NOTO);
        let text = "ab\u{05D0}\u{05D1}";
        let para = shape_paragraph(&f, text, 32.0);
        let expect =
            shape_run(&f, "ab", 32.0).advance + shape_run(&f, "\u{05D0}\u{05D1}", 32.0).advance;
        assert!(
            (para.advance - expect).abs() < 1e-3,
            "paragraph advance {} != sum of run advances {expect}",
            para.advance
        );
        // Mirrored RTL glyphs stay within the paragraph box.
        for g in &para.glyphs {
            assert!(g.x >= -1e-4 && g.x <= para.advance + 1e-4, "glyph x in box");
        }
    }
}
