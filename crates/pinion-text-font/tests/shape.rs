//! R50.6 §5.37.6 — real-font text-run shaping integration tests.
//!
//! Drives the baseline shaper end-to-end on the OFL fixtures (Noto Sans Latin +
//! Nanum Gothic 한글): `&str` → positioned glyph run → composited AA coverage.
//! Complements the pure `over()` unit tests in `shape.rs`.

use pinion_text_font::Font;
use std::fs;

fn load(path: &str) -> Font {
    let bytes = fs::read(path).expect("fixture present");
    Font::from_bytes(bytes).expect("valid Font")
}

const NOTO: &str = "tests/fonts/NotoSans-Regular.ttf";
const NANUM: &str = "tests/fonts/NanumGothic-Regular.ttf";

/// Design-units → device-px scale for a font at `px_per_em`.
fn scale(font: &Font, px_per_em: f32) -> f32 {
    px_per_em / f32::from(font.units_per_em())
}

#[test]
fn shape_run_accumulates_advances() {
    // "AV": glyph 0 sits at the origin, glyph 1 sits at exactly glyph 0's
    // advance, and the run advance is the sum — proves hmtx advances accumulate.
    let font = load(NOTO);
    let px = 48.0;
    let s = scale(&font, px);
    let a = font.glyph_id_for(0x0041).expect("'A' mapped");
    let v = font.glyph_id_for(0x0056).expect("'V' mapped");
    let adv_a = f32::from(font.glyph_advance_width(a).expect("'A' advance")) * s;
    let adv_v = f32::from(font.glyph_advance_width(v).expect("'V' advance")) * s;

    let run = font.shape_run("AV", px);
    assert_eq!(run.glyphs.len(), 2, "two codepoints → two glyphs");
    assert_eq!(run.glyphs[0].glyph_id, a);
    assert_eq!(run.glyphs[1].glyph_id, v);
    assert!((run.glyphs[0].x - 0.0).abs() < 1e-3, "first glyph at the pen origin");
    assert!((run.glyphs[1].x - adv_a).abs() < 1e-3, "second glyph at glyph 0's advance");
    assert!((run.advance - (adv_a + adv_v)).abs() < 1e-3, "run advance = sum of advances");
    assert!(adv_a > 0.0, "advances are positive (non-degenerate fixture)");
}

#[test]
fn shape_run_records_utf8_byte_clusters() {
    // "A한B": clusters are byte offsets, so the 3-byte 한 pushes 'B' to byte 4.
    // Proves the shaper iterates by codepoint and tracks source byte offsets.
    let font = load(NOTO);
    let run = font.shape_run("A한B", 32.0);
    let clusters: Vec<usize> = run.glyphs.iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, vec![0, 1, 4], "byte offsets: A@0, 한@1 (3 bytes), B@4");
}

#[test]
fn shape_run_unmapped_codepoint_is_notdef() {
    // U+10FFFF (the last code point, unassigned) maps to no glyph → .notdef (0).
    let font = load(NOTO);
    let run = font.shape_run("\u{10FFFF}", 32.0);
    assert_eq!(run.glyphs.len(), 1);
    assert_eq!(run.glyphs[0].glyph_id, 0, "unmapped codepoint resolves to .notdef");
}

#[test]
fn shape_run_empty_string_is_empty() {
    let font = load(NOTO);
    let run = font.shape_run("", 32.0);
    assert!(run.glyphs.is_empty(), "no glyphs");
    assert!((run.advance - 0.0).abs() < f32::EPSILON, "zero advance");
}

#[test]
fn render_run_positions_two_latin_glyphs_side_by_side() {
    // "HH": two non-overlapping stems. The composited bitmap is wider than one
    // 'H', carries ~2x the ink (alpha-over adds for non-overlapping glyphs), and
    // has ink in BOTH its left and right thirds — proving the second glyph was
    // placed at an advance, not stacked on the first.
    let font = load(NOTO);
    let px = 48.0;
    let one = font.render_run("H", px).expect("renders");
    let two = font.render_run("HH", px).expect("renders");
    assert!(one.ink_sum() > 0, "single 'H' inks");
    assert!(two.width > one.width, "two glyphs wider than one: {} vs {}", two.width, one.width);
    assert!(
        two.ink_sum() > one.ink_sum() * 3 / 2,
        "two glyphs ≈ 2x ink, got {} vs {}",
        two.ink_sum(),
        one.ink_sum(),
    );
    // Exact positioning oracle: "HH" is one 'H' bitmap plus a second copy shifted
    // right by exactly the integer-snapped advance, so the composite width is
    // round(advance) + one-'H' width. A partial-advance bug (glyph 2 at a
    // fraction of the advance) changes this width — the ink-thirds check below
    // alone would not catch it.
    let advance_px = font.shape_run("HH", px).glyphs[1].x;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let expected_w = advance_px.round() as usize + one.width;
    assert_eq!(two.width, expected_w, "second 'H' placed at exactly the advance");
    let third = two.width / 3;
    let ink_in = |xs: std::ops::Range<usize>| {
        (0..two.height).any(|y| xs.clone().any(|x| two.at(x, y) > 0))
    };
    assert!(ink_in(0..third), "ink in the left third (first glyph)");
    assert!(ink_in(two.width - third..two.width), "ink in the right third (second glyph)");
}

#[test]
fn render_run_aligns_mixed_height_glyphs_on_one_baseline() {
    // 'H' (cap height) is taller than 'x' (x-height): they ink above the baseline
    // with different `top`. The composite must place both on the shared baseline
    // (min_y = the topmost glyph's top, each blitted at top − min_y), so 'H' ink
    // reaches a higher row (smaller y) than 'x' ink. A min_y bug (max instead of
    // min, or per-glyph y offset dropped) would misalign them — same-height runs
    // like "HH" cannot detect that.
    let font = load(NOTO);
    let px = 48.0;
    let hx = font.render_run("Hx", px).expect("renders");
    let third = hx.width / 3;
    let top_row = |xs: std::ops::Range<usize>| {
        (0..hx.height).find(|&y| xs.clone().any(|x| hx.at(x, y) > 0))
    };
    let h_top = top_row(0..third).expect("'H' inks in the left third");
    let x_top = top_row(hx.width - third..hx.width).expect("'x' inks in the right third");
    assert!(h_top < x_top, "cap 'H' rises above x-height 'x' on the baseline: H@{h_top} x@{x_top}");
}

#[test]
fn render_run_blank_run_is_empty() {
    // A space is an empty outline: it advances the pen but inks nothing, so a
    // run of only spaces (and the empty string) composites to an empty bitmap.
    let font = load(NOTO);
    assert!(font.render_run("", 32.0).expect("renders").is_empty(), "empty string");
    assert!(font.glyph_id_for(0x0020).is_some(), "Noto maps U+0020 space");
    assert!(font.render_run("   ", 32.0).expect("renders").is_empty(), "all spaces ink nothing");
}

#[test]
fn render_run_space_widens_the_run() {
    // "H H" must be wider than "HH": the interior space advances the pen, opening
    // a gap between the two inked glyphs even though it leaves no ink itself.
    let font = load(NOTO);
    assert!(font.glyph_id_for(0x0020).is_some(), "Noto maps U+0020 space");
    let px = 48.0;
    let tight = font.render_run("HH", px).expect("renders");
    let spaced = font.render_run("H H", px).expect("renders");
    assert!(
        spaced.width > tight.width,
        "the space widens the run: {} vs {}",
        spaced.width,
        tight.width,
    );
}

#[test]
fn render_run_wide_cjk_places_two_syllables() {
    // 가 (U+AC00) is a full-width 한글 syllable Nanum renders to ink. A run of two
    // copies is wider than one and carries ~2x the ink — the self-hosted engine
    // rendering a real CJK *string*, not just one glyph.
    let font = load(NANUM);
    let px = 48.0;
    // Guard: the fixture must map 가 and rasterize it (fail loud, never skip).
    let ga = font.glyph_id_for(0xAC00).expect("Nanum maps 가 (U+AC00)");
    let single_ink = font
        .rasterize_glyph(ga, px)
        .expect("가 rasterizes (simple or composite, no point-match)")
        .ink_sum();
    assert!(single_ink > 0, "가 inks");

    let one = font.render_run("가", px).expect("renders");
    let two = font.render_run("가가", px).expect("renders");
    assert!(two.width > one.width, "two 가 wider than one: {} vs {}", two.width, one.width);
    assert!(
        two.ink_sum() > one.ink_sum() * 3 / 2,
        "two 가 ≈ 2x ink, got {} vs {}",
        two.ink_sum(),
        one.ink_sum(),
    );
}
