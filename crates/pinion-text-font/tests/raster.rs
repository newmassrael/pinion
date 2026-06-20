//! R50.8 §5.37.8 — real-font rasterization integration tests.
//!
//! Drives the self-hosted glyph rasterizer end-to-end on the OFL fixtures
//! (Noto Sans Latin + Nanum Gothic 한글): parse → outline → AA coverage bitmap.
//! Complements the synthetic exact-coverage unit tests in `raster/mod.rs`.

use pinion_text_font::{Font, Glyph, RasterError};
use std::fs;

fn load(path: &str) -> Font {
    let bytes = fs::read(path).expect("fixture present");
    Font::from_bytes(bytes).expect("valid Font")
}

/// First glyph id whose outline matches `pred`.
fn find_glyph(font: &Font, pred: impl Fn(&Glyph) -> bool) -> Option<u16> {
    (0..font.num_glyphs()).find(|&gid| font.glyph_outline(gid).is_some_and(&pred))
}

#[test]
fn noto_simple_glyph_rasterizes_to_ink() {
    let font = load("tests/fonts/NotoSans-Regular.ttf");
    // gid 0 (.notdef) is a simple hollow box per the parser sweep; rasterize it.
    let gid = find_glyph(&font, |g| matches!(g, Glyph::Simple(s) if !s.points.is_empty()))
        .expect("Noto has a simple glyph");
    let cov = font.rasterize_glyph(gid, 64.0).expect("simple glyph rasterizes");
    assert!(!cov.is_empty(), "bitmap has pixels");
    assert!(cov.ink_sum() > 0, "glyph leaves ink");
    assert_eq!(cov.alpha.len(), cov.width * cov.height, "alpha sized to bitmap");
}

#[test]
fn noto_capital_h_rasterizes() {
    // 'H' (U+0048) is a simple glyph (straight stems) in Noto Sans.
    let font = load("tests/fonts/NotoSans-Regular.ttf");
    let gid = font.glyph_id_for(0x0048).expect("'H' mapped");
    if let Some(Glyph::Simple(_)) = font.glyph_outline(gid) {
        let cov = font.rasterize_glyph(gid, 48.0).expect("rasterizes");
        assert!(cov.ink_sum() > 0, "'H' inks");
        // The bitmap is at most a couple px wider/taller than the 48px em.
        assert!(cov.width <= 64 && cov.height <= 64, "plausible bitmap size");
        // A cap-height glyph sits above the baseline → top row is above it.
        assert!(cov.top <= 0, "glyph top is at/above baseline, got {}", cov.top);
    }
}

#[test]
fn nanum_simple_hangul_rasterizes_to_ink() {
    let font = load("tests/fonts/NanumGothic-Regular.ttf");
    // '가' (U+AC00) — if it is a simple outline, rasterize; otherwise fall back
    // to any simple glyph so the 한글 fixture exercises the raster path.
    let gid = font.glyph_id_for(0xAC00).filter(|&g| {
        matches!(font.glyph_outline(g), Some(Glyph::Simple(s)) if !s.points.is_empty())
    });
    let gid = gid
        .or_else(|| find_glyph(&font, |g| matches!(g, Glyph::Simple(s) if !s.points.is_empty())))
        .expect("Nanum has a simple glyph");
    let cov = font.rasterize_glyph(gid, 64.0).expect("rasterizes");
    assert!(cov.ink_sum() > 0, "한글 glyph inks");
}

#[test]
fn composite_glyph_reports_unsupported() {
    // Noto Sans has accented Latin composites (parser sweep confirms > 0).
    let font = load("tests/fonts/NotoSans-Regular.ttf");
    let gid = find_glyph(&font, |g| matches!(g, Glyph::Composite(_)))
        .expect("Noto has a composite glyph");
    assert_eq!(
        font.rasterize_glyph(gid, 32.0),
        Err(RasterError::CompositeUnsupported(gid)),
    );
}

#[test]
fn out_of_range_glyph_reports_not_found() {
    let font = load("tests/fonts/NotoSans-Regular.ttf");
    let oob = font.num_glyphs();
    assert_eq!(
        font.rasterize_glyph(oob, 32.0),
        Err(RasterError::GlyphNotFound(oob)),
    );
}

#[test]
fn empty_glyph_rasterizes_to_empty_coverage() {
    // The space glyph (U+0020) is an empty outline (loca range 0) in both fonts.
    for path in [
        "tests/fonts/NotoSans-Regular.ttf",
        "tests/fonts/NanumGothic-Regular.ttf",
    ] {
        let font = load(path);
        if let Some(gid) = font.glyph_id_for(0x0020) {
            if matches!(font.glyph_outline(gid), Some(Glyph::Empty)) {
                let cov = font.rasterize_glyph(gid, 32.0).expect("empty rasterizes");
                assert!(cov.is_empty(), "{path}: space → empty coverage");
                assert_eq!(cov.ink_sum(), 0);
            }
        }
    }
}

#[test]
fn larger_size_yields_more_ink() {
    // Monotonic sanity: the same glyph at 2× the size leaves substantially
    // more coverage mass (area scales ~quadratically).
    let font = load("tests/fonts/NotoSans-Regular.ttf");
    let gid = find_glyph(&font, |g| matches!(g, Glyph::Simple(s) if s.points.len() > 8))
        .expect("a non-trivial simple glyph");
    let small = font.rasterize_glyph(gid, 24.0).expect("rasterizes").ink_sum();
    let large = font.rasterize_glyph(gid, 48.0).expect("rasterizes").ink_sum();
    assert!(large > small * 2, "2× size should ~4× ink: {small} → {large}");
}
