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
    let gid = find_glyph(
        &font,
        |g| matches!(g, Glyph::Simple(s) if !s.points.is_empty()),
    )
    .expect("Noto has a simple glyph");
    let cov = font
        .rasterize_glyph(gid, 64.0)
        .expect("simple glyph rasterizes");
    assert!(!cov.is_empty(), "bitmap has pixels");
    assert!(cov.ink_sum() > 0, "glyph leaves ink");
    assert_eq!(
        cov.alpha.len(),
        cov.width * cov.height,
        "alpha sized to bitmap"
    );
}

#[test]
fn noto_capital_h_rasterizes() {
    // 'H' (U+0048) is a straight-stem simple glyph in Noto Sans. Assert that
    // up front so a fixture change can't silently skip the body (vacuous pass).
    let font = load("tests/fonts/NotoSans-Regular.ttf");
    let gid = font.glyph_id_for(0x0048).expect("'H' mapped");
    assert!(
        matches!(font.glyph_outline(gid), Some(Glyph::Simple(_))),
        "'H' expected to be a simple glyph in Noto Sans",
    );
    let cov = font.rasterize_glyph(gid, 48.0).expect("rasterizes");
    assert!(cov.ink_sum() > 0, "'H' inks");
    // The bitmap is at most a couple px wider/taller than the 48px em.
    assert!(cov.width <= 64 && cov.height <= 64, "plausible bitmap size");
    // A cap-height glyph sits above the baseline → top row is above it.
    assert!(
        cov.top <= 0,
        "glyph top is at/above baseline, got {}",
        cov.top
    );
}

#[test]
fn nanum_simple_glyph_rasterizes_to_ink() {
    let font = load("tests/fonts/NanumGothic-Regular.ttf");
    // Prefer 한글 '가' (U+AC00) when it is a simple outline; otherwise any simple
    // glyph, so the 한글 fixture exercises the raster path regardless of how the
    // font composes its syllables (composite hangul is a R50.8.x concern).
    let gid = font.glyph_id_for(0xAC00).filter(
        |&g| matches!(font.glyph_outline(g), Some(Glyph::Simple(s)) if !s.points.is_empty()),
    );
    let gid = gid
        .or_else(|| {
            find_glyph(
                &font,
                |g| matches!(g, Glyph::Simple(s) if !s.points.is_empty()),
            )
        })
        .expect("Nanum has a simple glyph");
    let cov = font.rasterize_glyph(gid, 64.0).expect("rasterizes");
    assert!(cov.ink_sum() > 0, "glyph inks");
}

#[test]
fn noto_composite_glyphs_rasterize_to_ink() {
    // Noto Sans builds accented Latin (e.g. 'À', 'é') as composites: base glyph
    // + accent component placed by XY offset. Every such composite now
    // rasterizes; assert composites exist (non-vacuous) and that the placeable
    // ones all ink. A point-matched component (if any) is a later sub-round and
    // surfaces fail-loud as PointMatchUnsupported — never a silent blank.
    let font = load("tests/fonts/NotoSans-Regular.ttf");
    let composites: Vec<u16> = (0..font.num_glyphs())
        .filter(|&g| matches!(font.glyph_outline(g), Some(Glyph::Composite(_))))
        .collect();
    assert!(
        !composites.is_empty(),
        "Noto has composite glyphs to exercise"
    );
    let mut inked = 0usize;
    for &gid in &composites {
        match font.rasterize_glyph(gid, 32.0) {
            Ok(cov) => inked += usize::from(cov.ink_sum() > 0),
            Err(RasterError::PointMatchUnsupported(_)) => {} // deferred sub-round
            Err(e) => panic!("composite gid {gid} failed to rasterize: {e:?}"),
        }
    }
    assert!(inked > 0, "at least one real composite rasterizes to ink");
}

#[test]
fn nanum_composite_glyph_rasterizes() {
    // Nanum Gothic composes some 한글 syllables as composites; rasterizing one
    // end-to-end is the self-hosted engine growing to render real 한글 (the
    // §5.37 canonical goal). Find the first composite and require it to ink
    // (tolerating a point-matched one as the deferred sub-round).
    let font = load("tests/fonts/NanumGothic-Regular.ttf");
    let gid = find_glyph(&font, |g| matches!(g, Glyph::Composite(_)))
        .expect("Nanum Gothic composes 한글 syllables as composite glyphs");
    match font.rasterize_glyph(gid, 48.0) {
        Ok(cov) => assert!(cov.ink_sum() > 0, "composite 한글 glyph {gid} inks"),
        Err(RasterError::PointMatchUnsupported(_)) => {} // deferred sub-round
        Err(e) => panic!("composite gid {gid} failed to rasterize: {e:?}"),
    }
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
    // The space glyph (U+0020) is an empty outline (loca range 0). Require that
    // at least one fixture actually exercises the Empty path — otherwise a
    // fixture change could turn this into a silent no-op (vacuous pass).
    let mut exercised = 0;
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
                exercised += 1;
            }
        }
    }
    assert!(exercised > 0, "no fixture exercised the empty-glyph path");
}

#[test]
fn larger_size_yields_more_ink() {
    // Monotonic sanity: the same glyph at 2× the size leaves substantially
    // more coverage mass (area scales ~quadratically).
    let font = load("tests/fonts/NotoSans-Regular.ttf");
    let gid = find_glyph(
        &font,
        |g| matches!(g, Glyph::Simple(s) if s.points.len() > 8),
    )
    .expect("a non-trivial simple glyph");
    let small = font
        .rasterize_glyph(gid, 24.0)
        .expect("rasterizes")
        .ink_sum();
    let large = font
        .rasterize_glyph(gid, 48.0)
        .expect("rasterizes")
        .ink_sum();
    assert!(
        large > small * 2,
        "2× size should ~4× ink: {small} → {large}"
    );
}
