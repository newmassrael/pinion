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
//! UAX #9 rule X9 controls (the explicit embedding / override codes
//! RLE/LRE/RLO/LRO/PDF and Boundary Neutrals) are filtered out before shaping
//! and the L2 reorder ([`pinion_text_unicode::is_removed_by_x9`]): they carry no
//! glyph and a run made up entirely of them is dropped from the reorder so its
//! placeholder level cannot skew it — the run-granularity form of
//! `bidi_reorder`'s per-codepoint BN filter (its R51.24.1 fix).
//!
//! Scope (honest deferrals, not silent gaps): a single paragraph (the caller
//! splits on hard breaks via [`pinion_text_unicode::bidi::iter_paragraphs`]);
//! L3 combining-mark reordering and per-script shaper selection are deferred.
//! A removed control is not treated as a shaping-cluster boundary, so two
//! visible codepoints separated only by one may shape together (rare; an
//! interior control within a single level+script run). This includes the
//! ZWNJ (U+200C) / ZWJ (U+200D) join controls — both Boundary Neutral — so
//! their join/no-join shaping semantics are not yet honoured: the BIDI-removed
//! set is broader than the shaping-removed set, to be split when the shaper
//! reaches the paint path. Production paint is still §5.36 swash, so
//! [`shape_paragraph`] is a test forcing-consumer until then.

use crate::Font;
use crate::shape::{PositionedGlyph, shape_run};
use pinion_text_unicode::{ItemRun, is_removed_by_x9, itemize, reorder_visual};

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
    shape_runs_visual(font, paragraph, px_per_em, &itemize(paragraph))
}

/// Shape an already-itemised run list into one visually-ordered line: X9-filter
/// each run, drop pure-control runs, order the survivors by UAX #9 L2, then shape
/// and place each (reversing glyphs within right-to-left runs). The core shared
/// by [`shape_paragraph`] (the whole paragraph as one line) and
/// [`crate::line_layout::layout_paragraph`] (each wrapped line, given the paragraph's
/// runs clipped to that line). `runs` must reference byte ranges within
/// `paragraph`; the returned glyph `x` is relative to this line's origin and
/// `cluster` is the paragraph byte offset of the source codepoint.
pub(crate) fn shape_runs_visual(
    font: &Font,
    paragraph: &str,
    px_per_em: f32,
    runs: &[ItemRun],
) -> ShapedParagraph {
    // Strip X9-removed controls from each run's text, keeping only runs with
    // visible content. A pure-control run is dropped entirely so its placeholder
    // level never reaches the L2 reorder (the run-granularity BN filter).
    let active: Vec<ActiveRun> = runs
        .iter()
        .copied()
        .filter_map(|run| ActiveRun::build(paragraph, run))
        .collect();
    if active.is_empty() {
        return ShapedParagraph::default();
    }

    // UAX #9 L2 at run granularity: each run is single-level, so reordering the
    // per-run level array yields the left-to-right display order of runs.
    let levels: Vec<u8> = active.iter().map(|a| a.run.level).collect();
    let visual_order = reorder_visual(&levels);

    let mut glyphs: Vec<PositionedGlyph> = Vec::new();
    let mut pen = 0.0_f32;
    for &ai in &visual_order {
        let active = &active[ai];
        let shaped = shape_run(font, &active.text, px_per_em);
        let n = shaped.glyphs.len();
        if active.run.is_rtl() {
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
                    cluster: active.origin_byte(g.cluster),
                });
            }
        } else {
            for g in &shaped.glyphs {
                glyphs.push(PositionedGlyph {
                    glyph_id: g.glyph_id,
                    x: pen + g.x,
                    cluster: active.origin_byte(g.cluster),
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

/// An itemised run with its X9-filtered shapeable text and the map back to
/// paragraph byte offsets. Built only for runs that retain visible content.
struct ActiveRun {
    run: ItemRun,
    /// The run's text with X9-removed controls stripped.
    text: String,
    /// `(byte offset in `text`, byte offset in the paragraph)` for each kept
    /// codepoint, ascending by the first — `shape_run` clusters are codepoint
    /// (or ligature-component) starts, so they hit an entry exactly.
    clusters: Vec<(usize, usize)>,
}

impl ActiveRun {
    /// Build the filtered run, or `None` if every codepoint was X9-removed.
    fn build(paragraph: &str, run: ItemRun) -> Option<Self> {
        let mut text = String::new();
        let mut clusters = Vec::new();
        for (rel, ch) in paragraph[run.start..run.end].char_indices() {
            if is_removed_by_x9(ch) {
                continue;
            }
            clusters.push((text.len(), run.start + rel));
            text.push(ch);
        }
        if text.is_empty() {
            None
        } else {
            Some(Self {
                run,
                text,
                clusters,
            })
        }
    }

    /// Map a `shape_run` cluster (byte offset into [`Self::text`]) back to the
    /// paragraph byte offset of its source codepoint.
    fn origin_byte(&self, cluster: usize) -> usize {
        let idx = self
            .clusters
            .binary_search_by_key(&cluster, |&(text_byte, _)| text_byte)
            .expect("cluster lands on a kept codepoint start");
        self.clusters[idx].1
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
    fn explicit_directional_controls_are_filtered() {
        // a LRE b PDF c — the X9-removed controls (U+202A/U+202C) must produce
        // NO glyph and must not disturb order: exactly the three visible Latin
        // letters at their paragraph byte offsets (a@0, b@4, c@8).
        let f = font(NOTO);
        let text = "a\u{202A}b\u{202C}c";
        let para = shape_paragraph(&f, text, 32.0);
        let clusters: Vec<usize> = para.glyphs.iter().map(|g| g.cluster).collect();
        assert_eq!(
            clusters,
            vec![0, 4, 8],
            "controls stripped, visible order kept"
        );
        assert_x_ascending(&para);
        // Equivalent to shaping the controls-removed text directly.
        let plain = shape_paragraph(&f, "abc", 32.0);
        assert_eq!(para.glyphs.len(), plain.glyphs.len());
        assert!((para.advance - plain.advance).abs() < 1e-4);
    }

    #[test]
    fn pure_control_run_yields_no_glyphs() {
        // A paragraph of only X9-removed controls has no visible content.
        let f = font(NOTO);
        let para = shape_paragraph(&f, "\u{202A}\u{202C}", 32.0);
        assert!(para.glyphs.is_empty());
        assert!(para.advance.abs() < 1e-4);
    }

    #[test]
    fn interior_controls_are_stripped_within_mixed_runs() {
        // a RLE alef bet PDF c: RLE (level 0) joins the leading Latin run and
        // PDF (level 1) joins the Hebrew run, so both are *interior* controls
        // stripped per-codepoint (no pure-control run here). 4 visible glyphs:
        // a (LTR) | bet, alef (RTL mirrored, clusters descend) | c (LTR).
        let f = font(NOTO);
        let text = "a\u{202B}\u{05D0}\u{05D1}\u{202C}c";
        // bytes: a@0, RLE@1..4, alef@4..6, bet@6..8, PDF@8..11, c@11
        let para = shape_paragraph(&f, text, 32.0);
        let clusters: Vec<usize> = para.glyphs.iter().map(|g| g.cluster).collect();
        assert_eq!(
            clusters,
            vec![0, 6, 4, 11],
            "interior controls stripped, Hebrew mirrored"
        );
        assert_x_ascending(&para);
    }

    #[test]
    fn visible_order_matches_bidi_reorder_oracle() {
        // The equivalence guarantee: shape_paragraph's run-granularity X9 filter
        // + L2 reorder must match the conformance-tested per-codepoint
        // `bidi_reorder` restricted to visible codepoints. This is the negative
        // control for the reorder — any skew (e.g. failing to drop a
        // pure-control run) would diverge from the oracle. Latin + Hebrew, no
        // ligature/combining triggers, so each visible codepoint is one glyph.
        use pinion_text_unicode::bidi::bidi_reorder;
        use pinion_text_unicode::is_removed_by_x9;
        let f = font(NOTO);
        for text in [
            "a\u{202B}\u{05D0}b\u{202C}c",        // RLE Hebrew embedding, mixed
            "\u{05D0}\u{202A}ab\u{202C}\u{05D1}", // Hebrew, LRE Latin, Hebrew
            "ab\u{05D0}\u{05D1}cd",               // no controls, plain mixed
        ] {
            let para = shape_paragraph(&f, text, 32.0);
            let byte_of: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
            let chars: Vec<char> = text.chars().collect();
            let oracle: Vec<usize> = bidi_reorder(text)
                .into_iter()
                .filter(|&ci| !is_removed_by_x9(chars[ci]))
                .map(|ci| byte_of[ci])
                .collect();
            let got: Vec<usize> = para.glyphs.iter().map(|g| g.cluster).collect();
            assert_eq!(
                got, oracle,
                "visible order must match bidi_reorder for {text:?}"
            );
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
