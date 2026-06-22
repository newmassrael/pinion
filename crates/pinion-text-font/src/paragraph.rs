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
//! [`shape_paragraph_with_fallback`] (R50.14) is the multi-font form: it splits
//! each itemised run by font coverage ([`crate::fallback::font_runs`]) before
//! shaping, so a paragraph mixing scripts no single font covers (Latin + Hangul,
//! say) shapes each codepoint in its resolved fallback font yet still reorders by
//! BIDI level as one paragraph. Single-font [`shape_paragraph`] is the
//! one-element-stack special case of the same placer. This is the unified
//! itemise→font-split→shape pipeline §5.37.10 named as its follow-up.
//!
//! Scope (honest deferrals, not silent gaps): a single paragraph (the caller
//! splits on hard breaks via [`pinion_text_unicode::bidi::iter_paragraphs`]);
//! L3 combining-mark reordering and per-script shaper selection are deferred.
//! Multi-line wrapping combined with fallback is
//! [`crate::line_layout::layout_paragraph_with_fallback`], which reuses this
//! module's [`font_split_runs`] per wrapped line.
//! A removed control is not treated as a shaping-cluster boundary, so two
//! visible codepoints separated only by one may shape together (rare; an
//! interior control within a single level+script run). This includes the
//! ZWNJ (U+200C) / ZWJ (U+200D) join controls — both Boundary Neutral — so
//! their join/no-join shaping semantics are not yet honoured: the BIDI-removed
//! set is broader than the shaping-removed set, to be split when the shaper
//! reaches the paint path. Production paint is still §5.36 swash, so
//! [`shape_paragraph`] is a test forcing-consumer until then.

use crate::Font;
use crate::fallback::font_runs;
use crate::raster::{Coverage, RasterError};
use crate::shape::{GlyphDraw, PositionedGlyph, render_glyphs, shape_run};
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

/// A run uniform in BIDI level and resolved font — the unit
/// [`shape_runs_visual`] shapes and places. `level` drives the UAX #9 L2 run
/// ordering and the within-run right-to-left glyph mirror; `font_index` selects
/// the shaping font from the stack passed alongside (always 0 for single-font
/// [`shape_paragraph`]). Script is intentionally absent — it itemises the runs
/// upstream but does not affect placement (per-script shaper selection is
/// deferred), so the placer needs only level and font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlacementRun {
    /// Resolved UAX #9 embedding level (even = LTR, odd = RTL).
    pub level: u8,
    /// Index into the font stack that shapes this run.
    pub font_index: usize,
    /// Byte offset of the run's first codepoint (paragraph-relative).
    pub start: usize,
    /// Byte offset just past the run's last codepoint.
    pub end: usize,
}

/// Map itemised (level, script) runs onto single-font placement runs (font 0) —
/// the bridge from [`itemize()`] / [`crate::line_layout`]'s clipped runs to the
/// font-stack placer. The single SSOT for the single-font case, so
/// [`shape_paragraph`] and [`crate::line_layout::layout_paragraph`] share it.
pub(crate) fn single_font_runs(runs: &[ItemRun]) -> Vec<PlacementRun> {
    runs.iter()
        .map(|r| PlacementRun {
            level: r.level,
            font_index: 0,
            start: r.start,
            end: r.end,
        })
        .collect()
}

/// Split each itemised (level, script) run into maximal same-font sub-runs over
/// the `fonts` stack ([`font_runs`]) — the multi-font analogue of
/// [`single_font_runs`] and the SSOT bridge shared by
/// [`shape_paragraph_with_fallback`] (the whole paragraph's runs) and
/// [`crate::line_layout::layout_paragraph_with_fallback`] (each wrapped line's
/// clipped runs). A font split never changes a run's level (so the level array
/// still drives the UAX #9 reorder), and the per-item loop rebases `font_runs`'
/// item-relative offsets onto the paragraph while keeping each item's script in
/// scope — the shape a future script-aware fallback (cmap coverage is
/// script-blind today) would grow into. `runs` must reference byte ranges within
/// `paragraph`.
pub(crate) fn font_split_runs(
    fonts: &[&Font],
    paragraph: &str,
    runs: &[ItemRun],
) -> Vec<PlacementRun> {
    let mut out: Vec<PlacementRun> = Vec::new();
    for item in runs {
        for fr in font_runs(fonts, &paragraph[item.start..item.end]) {
            out.push(PlacementRun {
                level: item.level,
                font_index: fr.font_index,
                start: item.start + fr.start,
                end: item.start + fr.end,
            });
        }
    }
    out
}

/// Shape `paragraph` at `px_per_em` into visually-ordered positioned glyphs
/// (§5.37.6: itemise → shape per run → place runs in UAX #9 L2 visual order,
/// reversing glyphs within right-to-left runs). Single font — for a font stack
/// with fallback, use [`shape_paragraph_with_fallback`]. Expects a single
/// paragraph; an empty string yields no glyphs and zero advance.
#[must_use]
pub fn shape_paragraph(font: &Font, paragraph: &str, px_per_em: f32) -> ShapedParagraph {
    let runs = single_font_runs(&itemize(paragraph));
    shape_runs_visual(&[font], paragraph, px_per_em, &runs)
}

/// Shape `paragraph` across the `fonts` stack at `px_per_em` into
/// visually-ordered positioned glyphs — the unified itemise→font-split→shape
/// pipeline (§5.37.10 + §5.37.6). Combines BIDI/script itemisation ([`itemize()`])
/// with font-coverage fallback ([`font_runs`]): each itemised (level, script) run
/// is further split into maximal same-font sub-runs, every sub-run is shaped in
/// its resolved font, and the sub-runs are placed in UAX #9 L2 visual order
/// (glyphs reversed within right-to-left runs). A font split never changes a run's
/// level, so the level array still drives the reorder and sub-runs of one item
/// run keep their logical order among themselves (reversed as a block within RTL).
///
/// Each glyph's `font_index` is the stack index of the font that shaped it (its
/// `glyph_id` is valid only there); a renderer rasterizes each glyph with
/// `fonts[g.font_index]`. Expects a single paragraph; an empty stack or empty
/// string yields no glyphs.
#[must_use]
pub fn shape_paragraph_with_fallback(
    fonts: &[&Font],
    paragraph: &str,
    px_per_em: f32,
) -> ShapedParagraph {
    if fonts.is_empty() {
        return ShapedParagraph::default();
    }
    let runs = font_split_runs(fonts, paragraph, &itemize(paragraph));
    shape_runs_visual(fonts, paragraph, px_per_em, &runs)
}

/// Rasterize a shaped paragraph into one anti-aliased coverage bitmap, each glyph
/// drawn from its own font — the first time the multi-font / BIDI paragraph output
/// ([`shape_paragraph`] / [`shape_paragraph_with_fallback`]) becomes pixels.
///
/// `fonts` must be the stack the paragraph was shaped with: each glyph is
/// rasterized by `fonts[glyph.font_index]` through that font's own glyph atlas
/// (one atlas per stack font, so the same glyph id in two fonts never collides),
/// then all glyphs are composited by same-color alpha-over into one mask at their
/// integer-snapped pen positions. The single-font [`shape_paragraph`] output
/// renders the same way (every glyph `font_index` 0). Returns [`Coverage::empty`]
/// for an empty paragraph or an empty stack. A glyph whose `font_index` is outside
/// `fonts` is skipped (defensive — the contract is the matching stack).
///
/// The one CPU coverage mask is what a GPU text path uploads; note the multi-font
/// case fills *N* per-font atlases (one per stack font), so a GPU path must still
/// decide how to bind them (a texture array, or a merge into one atlas) — that is
/// deferred. Production paint is still §5.36 swash, so this is a test
/// forcing-consumer.
///
/// # Errors
///
/// Propagates a [`RasterError`] from any glyph's rasterization (a pathological
/// `px_per_em` past the size cap, or a not-yet-supported composite glyph).
pub fn render_paragraph(
    fonts: &[&Font],
    shaped: &ShapedParagraph,
    px_per_em: f32,
) -> Result<Coverage, RasterError> {
    render_glyphs(
        fonts,
        px_per_em,
        shaped.glyphs.iter().map(|g| GlyphDraw {
            font_index: g.font_index,
            glyph_id: g.glyph_id,
            pen_x: g.x,
            pen_y: g.y,
        }),
    )
}

/// Shape an already-itemised run list into one visually-ordered line: X9-filter
/// each run, drop pure-control runs, order the survivors by UAX #9 L2, then shape
/// and place each in its run's font (reversing glyphs within right-to-left runs).
/// The core shared by [`shape_paragraph`] (the whole paragraph as one line, a
/// one-element stack), [`shape_paragraph_with_fallback`] (font-split runs over a
/// stack) and [`crate::line_layout::layout_paragraph`] (each wrapped line, given
/// the paragraph's runs clipped to that line). `runs` must reference byte ranges
/// within `paragraph` and carry a `font_index` valid in `fonts`; the returned
/// glyph `x` is relative to this line's origin and `cluster` is the paragraph
/// byte offset of the source codepoint. An empty stack yields no glyphs.
pub(crate) fn shape_runs_visual(
    fonts: &[&Font],
    paragraph: &str,
    px_per_em: f32,
    runs: &[PlacementRun],
) -> ShapedParagraph {
    if fonts.is_empty() {
        return ShapedParagraph::default();
    }
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
    let levels: Vec<u8> = active.iter().map(|a| a.level).collect();
    let visual_order = reorder_visual(&levels);

    let mut glyphs: Vec<PositionedGlyph> = Vec::new();
    let mut pen = 0.0_f32;
    for &ai in &visual_order {
        let active = &active[ai];
        let shaped = shape_run(fonts[active.font_index], &active.text, px_per_em);
        let n = shaped.glyphs.len();
        if active.is_rtl() {
            // Mirror within the run for display: the logically-last glyph is
            // leftmost. A glyph spans `[g.x, next_x)` in the logical box
            // (`next_x` = the next glyph's pen origin, or the run advance for
            // the last logical glyph), so in the reversed box its visual origin
            // is `pen + (run_advance - next_x)`. Kerning is already folded into
            // the logical pen by `shape_run` and is preserved through the mirror.
            // A GPOS-attached mark's x is anchor-overridden (not advance-
            // monotonic), so this advance-based mirror does not re-attach marks
            // within a right-to-left run — RTL mark positioning is R50.6.x; the
            // y offset is direction-independent and carries through unchanged.
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
                    y: g.y,
                    cluster: active.origin_byte(g.cluster),
                    font_index: active.font_index,
                });
            }
        } else {
            for g in &shaped.glyphs {
                glyphs.push(PositionedGlyph {
                    glyph_id: g.glyph_id,
                    x: pen + g.x,
                    y: g.y,
                    cluster: active.origin_byte(g.cluster),
                    font_index: active.font_index,
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
    /// Resolved UAX #9 embedding level (drives the L2 reorder + RTL mirror).
    level: u8,
    /// Index into the font stack that shapes (and rasterizes) this run.
    font_index: usize,
    /// The run's text with X9-removed controls stripped.
    text: String,
    /// `(byte offset in `text`, byte offset in the paragraph)` for each kept
    /// codepoint, ascending by the first — `shape_run` clusters are codepoint
    /// (or ligature-component) starts, so they hit an entry exactly.
    clusters: Vec<(usize, usize)>,
}

impl ActiveRun {
    /// Build the filtered run, or `None` if every codepoint was X9-removed.
    fn build(paragraph: &str, run: PlacementRun) -> Option<Self> {
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
                level: run.level,
                font_index: run.font_index,
                text,
                clusters,
            })
        }
    }

    /// `true` if the run is right-to-left (odd embedding level).
    const fn is_rtl(&self) -> bool {
        self.level % 2 == 1
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
    use super::{
        ShapedParagraph, render_paragraph, shape_paragraph, shape_paragraph_with_fallback,
    };
    use crate::Font;
    use crate::raster::Coverage;
    use crate::shape::shape_run;

    const NOTO: &str = "tests/fonts/NotoSans-Regular.ttf";
    const NANUM: &str = "tests/fonts/NanumGothic-Regular.ttf";

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

    // ---- shape_paragraph_with_fallback (R50.14 unified pipeline) ----

    #[test]
    fn empty_stack_or_text_yields_no_glyphs() {
        let f = font(NOTO);
        assert_eq!(
            shape_paragraph_with_fallback(&[], "abc", 32.0),
            ShapedParagraph::default(),
            "no fonts => empty"
        );
        assert_eq!(
            shape_paragraph_with_fallback(&[&f], "", 32.0),
            ShapedParagraph::default(),
            "no text => empty"
        );
    }

    #[test]
    fn single_font_stack_equals_shape_paragraph() {
        // A one-element stack must be byte-identical to single-font
        // `shape_paragraph` (font_index all 0) — fallback generalises the
        // single-font placer, it does not diverge from it. Checked on pure-LTR
        // and on a mixed-direction (bidi-reordered) paragraph the one font
        // covers, so the reorder + mirror path is exercised too.
        let noto = font(NOTO);
        for text in ["office", "ab\u{05D0}\u{05D1}"] {
            let fb = shape_paragraph_with_fallback(&[&noto], text, 32.0);
            let sp = shape_paragraph(&noto, text, 32.0);
            assert_eq!(
                fb.glyphs, sp.glyphs,
                "1-font stack == shape_paragraph: {text:?}"
            );
            assert!((fb.advance - sp.advance).abs() < 1e-4);
            assert!(
                fb.glyphs.iter().all(|g| g.font_index == 0),
                "all glyphs font 0"
            );
        }
    }

    #[test]
    fn ltr_fallback_splits_by_coverage_and_tags_each_glyph_font() {
        // "A가B" over [Noto, Nanum]: A/B resolve to Noto (font 0), 가 (U+AC00)
        // only Nanum (font 1). The forcing-consumer payoff: each glyph is shaped
        // by its resolved font AND tagged with its stack index, so a renderer
        // knows which font to rasterize it with.
        let noto = font(NOTO);
        let nanum = font(NANUM);
        // Non-vacuity: the split is only meaningful because the coverages are
        // complementary at 가.
        assert!(
            !matches!(noto.glyph_id_for(0xAC00), Some(g) if g != 0),
            "Noto must NOT cover 가"
        );
        let nanum_ga = nanum.glyph_id_for(0xAC00).expect("Nanum covers 가");
        assert_ne!(nanum_ga, 0);

        let para = shape_paragraph_with_fallback(&[&noto, &nanum], "A가B", 32.0);
        // Bytes: A@0, 가@1..4, B@4. LTR, so visual == logical order.
        let clusters: Vec<usize> = para.glyphs.iter().map(|g| g.cluster).collect();
        let fonts: Vec<usize> = para.glyphs.iter().map(|g| g.font_index).collect();
        let gids: Vec<u16> = para.glyphs.iter().map(|g| g.glyph_id).collect();
        assert_eq!(
            clusters,
            vec![0, 1, 4],
            "A | 가 | B in logical/visual order"
        );
        assert_eq!(fonts, vec![0, 1, 0], "A=Noto, 가=Nanum, B=Noto");
        assert_eq!(gids[0], noto.glyph_id_for(0x41).unwrap(), "A from Noto");
        assert_eq!(
            gids[1], nanum_ga,
            "가 from Nanum (proves fallback selected it)"
        );
        assert_eq!(gids[2], noto.glyph_id_for(0x42).unwrap(), "B from Noto");
        // x ascending (visual order), 가's advance comes from Nanum's own hmtx.
        for pair in para.glyphs.windows(2) {
            assert!(pair[1].x >= pair[0].x, "visual order: non-decreasing x");
        }
        assert!(para.glyphs[0].x.abs() < 1e-4, "first glyph at origin");
    }

    #[test]
    fn bidi_reorder_and_font_fallback_compose() {
        // "A" (Latin LTR) + alef,bet (Hebrew RTL) + 가 (Hangul LTR), base LTR.
        // Bytes: A@0, alef@1..3, bet@3..5, 가@5..8. Levels [0,1,1,0] => the
        // Hebrew run reorders+mirrors (bet before alef) between the two LTR
        // pieces, while 가 still falls back to Nanum. Proves the BIDI reorder and
        // the font split combine in one paragraph.
        let noto = font(NOTO);
        let nanum = font(NANUM);
        assert!(
            !matches!(noto.glyph_id_for(0xAC00), Some(g) if g != 0),
            "Noto must NOT cover 가 (else fallback isn't exercised)"
        );

        let para =
            shape_paragraph_with_fallback(&[&noto, &nanum], "A\u{05D0}\u{05D1}\u{AC00}", 32.0);
        let clusters: Vec<usize> = para.glyphs.iter().map(|g| g.cluster).collect();
        let fonts: Vec<usize> = para.glyphs.iter().map(|g| g.font_index).collect();
        // A@0, then Hebrew mirrored (bet@3, alef@1), then 가@5.
        assert_eq!(
            clusters,
            vec![0, 3, 1, 5],
            "Hebrew mirrors between the LTR pieces"
        );
        // A and the Hebrew run resolve to Noto (Hebrew uncovered => primary 0),
        // 가 to Nanum. font_index travels through the RTL mirror unchanged.
        assert_eq!(fonts, vec![0, 0, 0, 1], "Latin+Hebrew=Noto, Hangul=Nanum");
        assert_eq!(
            para.glyphs[3].glyph_id,
            nanum.glyph_id_for(0xAC00).unwrap(),
            "가 shaped from Nanum after a bidi-reordered run"
        );
        for pair in para.glyphs.windows(2) {
            assert!(pair[1].x >= pair[0].x, "visual order: non-decreasing x");
        }
    }

    #[test]
    fn font1_glyph_in_an_rtl_run_keeps_index_through_the_mirror() {
        // The font≠0 RTL case the other tests miss. Neither fixture covers a whole
        // RTL script (NotoSans lacks Hebrew too), so build a mixed-font RTL run: a
        // Hebrew alef (neither font covers → primary 0) followed by a combining
        // acute (Inherited script, so it joins the Hebrew run; NSM bidi class
        // inherits the alef's R, so the run is RTL level 1). The acute resolves to
        // Noto (font 1), the alef to Nanum (font 0). The two same-level sub-runs
        // reorder (acute leftmost) and the acute is placed via the RTL mirror
        // branch — so its glyph must carry font_index 1, not a hardcoded 0.
        let nanum = font(NANUM);
        let noto = font(NOTO);
        assert!(
            !matches!(nanum.glyph_id_for(0x0301), Some(g) if g != 0),
            "Nanum must NOT cover combining acute"
        );
        let noto_acute = noto
            .glyph_id_for(0x0301)
            .expect("Noto covers combining acute");
        assert_ne!(noto_acute, 0);

        // Bytes: alef@0..2, U+0301@2..4.
        let para = shape_paragraph_with_fallback(&[&nanum, &noto], "\u{05D0}\u{0301}", 32.0);
        assert_eq!(para.glyphs.len(), 2);
        let clusters: Vec<usize> = para.glyphs.iter().map(|g| g.cluster).collect();
        let fonts: Vec<usize> = para.glyphs.iter().map(|g| g.font_index).collect();
        // RTL: the acute sub-run reorders before the alef sub-run.
        assert_eq!(
            clusters,
            vec![2, 0],
            "acute (logically last) is leftmost — RTL"
        );
        assert_eq!(
            fonts,
            vec![1, 0],
            "acute=Noto(1) via the mirror branch, alef=Nanum(0)"
        );
        assert_eq!(
            para.glyphs[0].glyph_id, noto_acute,
            "leftmost glyph shaped by Noto"
        );
        assert!(
            para.glyphs[0].x <= para.glyphs[1].x,
            "visual order: non-decreasing x"
        );
    }

    #[test]
    fn font_split_within_one_item_run_rebases_offsets() {
        // A single itemised run that splits into two fonts — the intra-run path
        // the script-boundary tests don't reach. Stack [Nanum, Noto], "가e\u{0301}":
        // itemise makes one Latin run "e\u{0301}" (the Inherited combining acute
        // joins 'e'); within it, Nanum has 'e' (font 0) but not U+0301, which only
        // Noto covers (font 1). So font_runs splits one item run at a non-zero
        // offset that must be rebased onto the paragraph (a missing `item.start`
        // would slice mid-가 and panic; a missing `fr.start` would mis-cluster).
        let nanum = font(NANUM);
        let noto = font(NOTO);
        assert!(
            nanum.glyph_id_for(0xAC00).is_some_and(|g| g != 0),
            "Nanum covers 가"
        );
        assert!(
            nanum.glyph_id_for(0x65).is_some_and(|g| g != 0),
            "Nanum covers e"
        );
        assert!(
            !matches!(nanum.glyph_id_for(0x0301), Some(g) if g != 0),
            "Nanum must NOT cover combining acute (else no within-run split)"
        );
        assert!(
            noto.glyph_id_for(0x0301).is_some_and(|g| g != 0),
            "Noto covers combining acute"
        );

        // Bytes: 가@0..3, e@3, U+0301@4..6.
        let para = shape_paragraph_with_fallback(&[&nanum, &noto], "\u{AC00}e\u{0301}", 32.0);
        let clusters: Vec<usize> = para.glyphs.iter().map(|g| g.cluster).collect();
        let fonts: Vec<usize> = para.glyphs.iter().map(|g| g.font_index).collect();
        assert_eq!(
            clusters,
            vec![0, 3, 4],
            "rebased to paragraph bytes 가, e, acute"
        );
        assert_eq!(
            fonts,
            vec![0, 0, 1],
            "가/e=Nanum, acute=Noto — split inside one item run"
        );
        for pair in para.glyphs.windows(2) {
            assert!(pair[1].x >= pair[0].x, "visual order: non-decreasing x");
        }
    }

    // ---- render_paragraph (R1051 multi-font paragraph rasterization) ----

    #[test]
    fn render_paragraph_empty_yields_empty_coverage() {
        let noto = font(NOTO);
        let empty = shape_paragraph(&noto, "", 32.0);
        assert_eq!(
            render_paragraph(&[&noto], &empty, 32.0).unwrap(),
            Coverage::empty(),
            "empty paragraph => empty coverage"
        );
        // Empty stack also yields empty (defensive).
        let shaped = shape_paragraph(&noto, "A", 32.0);
        assert_eq!(
            render_paragraph(&[], &shaped, 32.0).unwrap(),
            Coverage::empty(),
            "empty stack => empty coverage"
        );
    }

    #[test]
    fn render_paragraph_single_font_matches_render_run() {
        // For single-run text shape_paragraph == shape_run, so rendering the
        // paragraph over a one-font stack must be byte-identical to render_run — the
        // generalization adds the font dimension without disturbing the raster. The
        // combining-mark string also drives the non-zero pen_y (mark-to-base)
        // composite path through render_paragraph.
        let noto = font(NOTO);
        for text in ["office", "e\u{0301}"] {
            let para = shape_paragraph(&noto, text, 32.0);
            let from_para = render_paragraph(&[&noto], &para, 32.0).unwrap();
            let from_run = noto.render_run(text, 32.0).unwrap();
            assert_eq!(
                from_para, from_run,
                "render_paragraph(&[font]) == render_run: {text:?}"
            );
            assert!(
                from_para.width > 0 && from_para.height > 0,
                "non-empty ink: {text:?}"
            );
        }
    }

    #[test]
    fn render_paragraph_draws_each_glyph_from_its_own_font() {
        // The forcing payoff: 가 (only in Nanum) must be rasterized via Nanum, not
        // the primary Noto. shape_paragraph_with_fallback tags it font 1, and
        // render_paragraph must produce Nanum's 가 — byte-identical to rendering 가
        // directly in Nanum. That equality IS the discriminator: had render_paragraph
        // ignored font_index and used Noto (fonts[0]), it would feed Nanum's glyph id
        // into Noto's outline table and yield different (or empty) ink.
        let noto = font(NOTO);
        let nanum = font(NANUM);
        assert!(
            !matches!(noto.glyph_id_for(0xAC00), Some(g) if g != 0),
            "Noto must NOT cover 가"
        );
        let shaped = shape_paragraph_with_fallback(&[&noto, &nanum], "\u{AC00}", 32.0);
        assert_eq!(shaped.glyphs.len(), 1);
        assert_eq!(
            shaped.glyphs[0].font_index, 1,
            "가 resolved to Nanum (font 1)"
        );

        let rendered = render_paragraph(&[&noto, &nanum], &shaped, 32.0).unwrap();
        let nanum_ga = nanum.render_run("\u{AC00}", 32.0).unwrap();
        assert!(
            !rendered.alpha.is_empty(),
            "Nanum 가 has real ink (non-vacuous)"
        );
        assert_eq!(
            rendered, nanum_ga,
            "rasterized via Nanum, its resolved font"
        );
    }

    #[test]
    fn render_paragraph_composites_mixed_font_string() {
        // "A가B" over [Noto, Nanum]: three glyphs from two fonts composite into one
        // mask spanning all three (wider than a single Noto glyph) — the multi-atlas
        // path produces a coherent non-empty bitmap.
        let noto = font(NOTO);
        let nanum = font(NANUM);
        let shaped = shape_paragraph_with_fallback(&[&noto, &nanum], "A\u{AC00}B", 32.0);
        let cov = render_paragraph(&[&noto, &nanum], &shaped, 32.0).unwrap();
        assert!(cov.width > 0 && cov.height > 0, "non-empty composite");
        let a_only = noto.render_run("A", 32.0).unwrap();
        assert!(cov.width > a_only.width, "spans more than a single glyph");
    }

    #[test]
    fn render_paragraph_composites_bidi_mixed_font() {
        // Integration of R1050 (BIDI reorder + fallback) and R1051 (render): a
        // BIDI-reordered, multi-font paragraph rasterizes without panic into a
        // non-empty mask. "A" (Latin) + Hebrew (RTL, reordered) + 가 (Nanum) —
        // render_paragraph places each glyph at its already-absolute visual x, so
        // the reorder is transparent to it while 가 still comes from Nanum.
        let noto = font(NOTO);
        let nanum = font(NANUM);
        let shaped =
            shape_paragraph_with_fallback(&[&noto, &nanum], "A\u{05D0}\u{05D1}\u{AC00}", 32.0);
        let cov = render_paragraph(&[&noto, &nanum], &shaped, 32.0).unwrap();
        assert!(
            cov.width > 0 && cov.height > 0,
            "non-empty bidi+fallback composite"
        );
        let a_only = noto.render_run("A", 32.0).unwrap();
        assert!(cov.width > a_only.width, "spans more than a single glyph");
    }
}
