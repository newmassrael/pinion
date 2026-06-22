//! R50.13 §5.37.10 — font fallback (multi-font shaping).
//!
//! No single font covers all of Unicode, so real text is shaped over a *font
//! stack*: a primary font plus ordered fallbacks. Each codepoint resolves to the
//! first font in the stack that has a real (non-`.notdef`) glyph for it; a
//! codepoint no font covers stays on the primary so it renders the primary's
//! `.notdef`. This is the engine's analogue of `fontconfig` / `CoreText` /
//! `DirectWrite` font matching — but over an *explicit in-memory stack*. OS font
//! enumeration (discovering installed fonts) is a separate platform layer and is
//! deferred.
//!
//! [`font_runs`] itemises the text into maximal same-font byte runs;
//! [`shape_with_fallback`] shapes each run in its own font via [`shape_run`]
//! (cmap + GSUB + GPOS are per-font) and places the runs left to right on one
//! shared baseline. So `shape_run` stays single-font and fallback *composes* it
//! — the same layering as BIDI/script itemisation feeding the shaper.
//!
//! Scope (honest deferrals, not silent gaps): resolution is per *codepoint*, not
//! per grapheme cluster — a base and its combining mark could in principle land
//! in different fonts (rare; cluster-grouped fallback is a later refinement).
//! BIDI/script itemisation (§5.37.4, §5.37.5) is orthogonal and not combined here
//! (a single direction is assumed, like [`shape_run`]); the unified
//! itemise→font-split→shape pipeline is a follow-up. Production paint is still
//! §5.36 swash, so this is a test forcing-consumer until the paint path wires
//! the self-hosted engine.

use crate::Font;
use crate::shape::{PositionedGlyph, shape_run};

/// One maximal run of codepoints that resolve to a single font in the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FontRunRange {
    /// Index into the font stack passed to [`font_runs`] / [`shape_with_fallback`].
    pub font_index: usize,
    /// Byte offset of the run's first codepoint.
    pub start: usize,
    /// Byte offset just past the run's last codepoint.
    pub end: usize,
}

/// A shaped sub-run in one fallback font.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FontRun {
    /// Index into the font stack — which font shaped (and rasterizes) these glyphs.
    pub font_index: usize,
    /// Glyphs in logical order. `x` is the absolute whole-text pen; `glyph_id` is
    /// valid in `fonts[font_index]`; `cluster` is the whole-text byte offset.
    pub glyphs: Vec<PositionedGlyph>,
    /// This run's advance width (device px).
    pub advance: f32,
}

/// A fallback-shaped run: per-font sub-runs placed left to right on one baseline.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FallbackRun {
    /// The per-font sub-runs in logical (left-to-right) order.
    pub runs: Vec<FontRun>,
    /// Total advance width across all sub-runs (device px).
    pub advance: f32,
}

/// Whether `font` has a real (non-`.notdef`) glyph for `ch` — both an unmapped
/// codepoint (`None`) and an explicit map to glyph 0 count as *not covered*.
fn covers(font: &Font, ch: char) -> bool {
    matches!(font.glyph_id_for(ch as u32), Some(g) if g != 0)
}

/// The index of the first font in `fonts` that covers `ch`, or 0 (the primary)
/// when none do — so an uncovered codepoint renders the primary's `.notdef`.
fn resolve_font(fonts: &[&Font], ch: char) -> usize {
    fonts.iter().position(|&f| covers(f, ch)).unwrap_or(0)
}

/// Itemise `text` into maximal runs that each resolve to a single font in the
/// `fonts` stack (§5.37.10): each codepoint maps to the first font with a real
/// glyph for it (or the primary, font 0, when none do), and a new run begins
/// wherever that font index changes. Runs are contiguous and cover the whole
/// text; an empty stack or empty text yields no runs.
#[must_use]
pub fn font_runs(fonts: &[&Font], text: &str) -> Vec<FontRunRange> {
    if fonts.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<FontRunRange> = Vec::new();
    let mut cur: Option<FontRunRange> = None;
    for (byte, ch) in text.char_indices() {
        let font_index = resolve_font(fonts, ch);
        let end = byte + ch.len_utf8();
        match cur {
            Some(ref mut run) if run.font_index == font_index => run.end = end,
            _ => {
                if let Some(run) = cur.take() {
                    out.push(run);
                }
                cur = Some(FontRunRange {
                    font_index,
                    start: byte,
                    end,
                });
            }
        }
    }
    if let Some(run) = cur {
        out.push(run);
    }
    out
}

/// Shape `text` across the `fonts` stack (§5.37.10): split into font-runs via
/// [`font_runs`], shape each run in its own font ([`shape_run`]: cmap, GSUB,
/// GPOS), then place the runs left to right on a shared baseline. Each glyph has
/// an absolute whole-text pen and a whole-text byte cluster; each [`FontRun`]
/// records the font index that shaped and rasterizes it. An empty stack or empty
/// text yields an empty [`FallbackRun`].
#[must_use]
pub fn shape_with_fallback(fonts: &[&Font], text: &str, px_per_em: f32) -> FallbackRun {
    let mut runs: Vec<FontRun> = Vec::new();
    let mut pen = 0.0_f32;
    for r in font_runs(fonts, text) {
        let font = fonts[r.font_index];
        let shaped = shape_run(font, &text[r.start..r.end], px_per_em);
        let glyphs = shaped
            .glyphs
            .iter()
            .map(|g| PositionedGlyph {
                glyph_id: g.glyph_id,
                x: pen + g.x,
                y: g.y,
                cluster: r.start + g.cluster,
            })
            .collect();
        runs.push(FontRun {
            font_index: r.font_index,
            glyphs,
            advance: shaped.advance,
        });
        pen += shaped.advance;
    }
    FallbackRun { runs, advance: pen }
}

#[cfg(test)]
mod tests {
    use super::{FontRunRange, font_runs, shape_with_fallback};
    use crate::Font;

    const NOTO: &str = "tests/fonts/NotoSans-Regular.ttf";
    const NANUM: &str = "tests/fonts/NanumGothic-Regular.ttf";
    const PX: f32 = 32.0;

    fn font(path: &str) -> Font {
        let bytes = std::fs::read(path).expect("read font fixture");
        Font::from_bytes(bytes).expect("valid Font")
    }

    #[test]
    fn empty_stack_or_text_yields_nothing() {
        let noto = font(NOTO);
        assert!(font_runs(&[], "abc").is_empty(), "no fonts => no runs");
        assert!(font_runs(&[&noto], "").is_empty(), "no text => no runs");
        assert!(
            shape_with_fallback(&[], "abc", PX).runs.is_empty(),
            "no fonts => empty shaped run"
        );
        assert!(shape_with_fallback(&[&noto], "", PX).runs.is_empty());
    }

    #[test]
    fn single_font_is_one_run() {
        let noto = font(NOTO);
        let runs = font_runs(&[&noto], "abc");
        assert_eq!(
            runs,
            vec![FontRunRange {
                font_index: 0,
                start: 0,
                end: 3
            }]
        );
    }

    #[test]
    fn mixed_script_splits_on_coverage() {
        // NotoSans (Latin) lacks Hangul; NanumGothic has it. "A가B": A and B
        // resolve to font 0 (Noto), 가 (U+AC00, 3 UTF-8 bytes) to font 1 (Nanum).
        let noto = font(NOTO);
        let nanum = font(NANUM);
        // Non-vacuity guard: the split is only meaningful because the coverages
        // are complementary at 가 — pin that so a fixture change can't make this
        // pass for the wrong reason.
        assert!(
            !matches!(noto.glyph_id_for(0xAC00), Some(g) if g != 0),
            "Noto must NOT cover 가"
        );
        assert!(
            nanum.glyph_id_for(0xAC00).is_some_and(|g| g != 0),
            "Nanum covers 가"
        );
        let runs = font_runs(&[&noto, &nanum], "A가B");
        assert_eq!(
            runs,
            vec![
                FontRunRange {
                    font_index: 0,
                    start: 0,
                    end: 1
                },
                FontRunRange {
                    font_index: 1,
                    start: 1,
                    end: 4
                },
                FontRunRange {
                    font_index: 0,
                    start: 4,
                    end: 5
                },
            ]
        );
    }

    #[test]
    fn codepoint_no_font_covers_falls_to_primary() {
        // U+1F600 (emoji) is in neither fixture; it must resolve to the primary
        // (font 0) so it renders the primary's .notdef rather than being dropped.
        let noto = font(NOTO);
        let nanum = font(NANUM);
        assert!(noto.glyph_id_for(0x1F600).is_none(), "Noto lacks the emoji");
        assert!(
            nanum.glyph_id_for(0x1F600).is_none(),
            "Nanum lacks the emoji"
        );
        let runs = font_runs(&[&noto, &nanum], "\u{1F600}");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].font_index, 0, "uncovered => primary font");
    }

    #[test]
    fn shape_with_fallback_resolves_each_run_in_its_font() {
        // The forcing consumer: 가 only renders via the Nanum fallback, A/B via
        // Noto. Exact gids per font, pen accumulates across fonts, clusters map
        // to whole-text bytes.
        let noto = font(NOTO);
        let nanum = font(NANUM);
        let stack = [&noto, &nanum];
        let shaped = shape_with_fallback(&stack, "A가B", PX);

        assert_eq!(shaped.runs.len(), 3, "Noto | Nanum | Noto");
        let (r0, r1, r2) = (&shaped.runs[0], &shaped.runs[1], &shaped.runs[2]);
        assert_eq!((r0.font_index, r1.font_index, r2.font_index), (0, 1, 0));

        // Non-vacuous: 가 has NO Noto glyph but a real Nanum glyph, so a correct
        // run-1 glyph proves fallback actually selected Nanum.
        assert!(
            !matches!(noto.glyph_id_for(0xAC00), Some(g) if g != 0),
            "Noto must NOT cover 가 (else the test wouldn't exercise fallback)"
        );
        let nanum_ga = nanum.glyph_id_for(0xAC00).expect("Nanum covers 가");
        assert_ne!(nanum_ga, 0);
        assert_eq!(r1.glyphs[0].glyph_id, nanum_ga, "가 shaped from Nanum");
        assert_eq!(
            r0.glyphs[0].glyph_id,
            noto.glyph_id_for(0x41).unwrap(),
            "A shaped from Noto"
        );
        assert_eq!(
            r2.glyphs[0].glyph_id,
            noto.glyph_id_for(0x42).unwrap(),
            "B shaped from Noto"
        );

        // Pen accumulates left to right across the (different-upem) fonts.
        assert!((r0.glyphs[0].x - 0.0).abs() < 1e-4, "run 0 at the origin");
        assert!(
            (r1.glyphs[0].x - r0.advance).abs() < 1e-4,
            "run 1 starts at run 0's advance"
        );
        assert!(
            (r2.glyphs[0].x - (r0.advance + r1.advance)).abs() < 1e-4,
            "run 2 starts after runs 0+1"
        );
        assert!(
            (shaped.advance - (r0.advance + r1.advance + r2.advance)).abs() < 1e-4,
            "total advance = sum of run advances"
        );

        // Clusters are whole-text byte offsets: A@0, 가@1, B@4.
        assert_eq!(r0.glyphs[0].cluster, 0);
        assert_eq!(r1.glyphs[0].cluster, 1);
        assert_eq!(r2.glyphs[0].cluster, 4);
    }

    #[test]
    fn covered_by_both_resolves_to_the_first_font() {
        // 'A' is in BOTH fonts with DIFFERENT gids, so the stack order alone
        // decides — pins the first-match contract (a last-match bug would pick
        // the other font). The reverse stack must pick the other font.
        let noto = font(NOTO);
        let nanum = font(NANUM);
        let noto_a = noto.glyph_id_for(0x41).unwrap();
        let nanum_a = nanum.glyph_id_for(0x41).unwrap();
        assert_ne!(noto_a, nanum_a, "the two fonts give 'A' distinct gids");

        let fwd = shape_with_fallback(&[&noto, &nanum], "A", PX);
        assert_eq!(fwd.runs.len(), 1);
        assert_eq!(fwd.runs[0].font_index, 0, "first covering font wins");
        assert_eq!(fwd.runs[0].glyphs[0].glyph_id, noto_a, "shaped from Noto");

        let rev = shape_with_fallback(&[&nanum, &noto], "A", PX);
        assert_eq!(rev.runs[0].font_index, 0);
        assert_eq!(
            rev.runs[0].glyphs[0].glyph_id, nanum_a,
            "reversed stack => Nanum"
        );
    }

    #[test]
    fn multi_glyph_run_offsets_each_glyph() {
        // Run 0 = "AB" (two Noto glyphs): each glyph keeps its intra-run x under
        // the whole-text pen offset (x = pen + g.x). With pen = 0 for run 0, the
        // run must equal a standalone shape_run — a `x: pen` bug would collapse
        // the second glyph onto the first.
        let noto = font(NOTO);
        let nanum = font(NANUM);
        let shaped = shape_with_fallback(&[&noto, &nanum], "AB가", PX);
        assert_eq!(shaped.runs.len(), 2, "AB | 가");
        let solo = noto.shape_run("AB", PX);
        assert_eq!(
            shaped.runs[0].glyphs, solo.glyphs,
            "run 0 == standalone shape_run (pen 0, cluster 0)"
        );
        assert!(
            solo.glyphs[1].x > 0.0,
            "B is offset from A — the intra-run g.x a `x: pen` bug would drop"
        );
    }
}
