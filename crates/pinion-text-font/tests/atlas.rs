//! R50.9 §5.37.9 — glyph atlas integration tests on the OFL fixtures.
//!
//! Drives the cache + shelf-pack end-to-end on Noto Sans: rasterize-once,
//! lossless pack, non-overlapping placement, shelf wrap, blank-glyph caching.

use pinion_text_font::{Font, GlyphAtlas};
use std::fs;

const NOTO: &str = "tests/fonts/NotoSans-Regular.ttf";

fn load(path: &str) -> Font {
    let bytes = fs::read(path).expect("fixture present");
    Font::from_bytes(bytes).expect("valid Font")
}

#[test]
fn atlas_caches_repeated_glyph() {
    // A second lookup of the same (glyph, size) is a cache hit: same entry, no
    // new packing, exactly one entry.
    let font = load(NOTO);
    let mut atlas = GlyphAtlas::new(128);
    let h = font.glyph_id_for(0x0048).expect("'H' mapped");
    let first = atlas.get_or_insert(&font, h, 48.0).expect("rasterizes");
    let dims = (atlas.width(), atlas.height(), atlas.len());
    let second = atlas.get_or_insert(&font, h, 48.0).expect("cache hit");
    assert_eq!(first, second, "repeat lookup returns the same entry");
    assert_eq!(
        (atlas.width(), atlas.height(), atlas.len()),
        dims,
        "no re-pack on a hit"
    );
    assert_eq!(atlas.len(), 1, "one distinct entry");
}

#[test]
fn atlas_subrect_is_byte_identical_to_standalone_raster() {
    // The packed sub-rect must be byte-for-byte equal to a standalone
    // rasterization — the atlas copies coverage losslessly into its bitmap.
    let font = load(NOTO);
    let mut atlas = GlyphAtlas::new(128);
    let h = font.glyph_id_for(0x0048).expect("'H' mapped");
    let entry = atlas.get_or_insert(&font, h, 48.0).expect("rasterizes");
    let standalone = font.rasterize_glyph(h, 48.0).expect("rasterizes");
    assert_eq!(
        (entry.width, entry.height),
        (standalone.width, standalone.height),
        "same size"
    );
    assert_eq!(
        (entry.left, entry.top),
        (standalone.left, standalone.top),
        "same pen offset"
    );
    let aw = atlas.width();
    for y in 0..entry.height {
        for x in 0..entry.width {
            let packed = atlas.alpha()[(entry.y + y) * aw + entry.x + x];
            let direct = standalone.alpha[y * standalone.width + x];
            assert_eq!(packed, direct, "atlas byte at ({x},{y}) is lossless");
        }
    }
}

#[test]
fn atlas_packs_distinct_glyphs_without_overlap() {
    let font = load(NOTO);
    let mut atlas = GlyphAtlas::new(256);
    let mut rects = Vec::new();
    for cp in [0x0048u32, 0x0069, 0x0078, 0x0067] {
        // H i x g
        let gid = font.glyph_id_for(cp).expect("mapped");
        rects.push(atlas.get_or_insert(&font, gid, 48.0).expect("rasterizes"));
    }
    for r in &rects {
        assert!(
            r.x + r.width <= atlas.width() && r.y + r.height <= atlas.height(),
            "sub-rect {r:?} lies inside the atlas {}x{}",
            atlas.width(),
            atlas.height(),
        );
    }
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            let (a, b) = (&rects[i], &rects[j]);
            let disjoint = a.x + a.width <= b.x
                || b.x + b.width <= a.x
                || a.y + a.height <= b.y
                || b.y + b.height <= a.y;
            assert!(disjoint, "rects {i} {a:?} and {j} {b:?} overlap");
        }
    }
}

#[test]
fn atlas_wraps_to_a_new_shelf() {
    // An atlas exactly one 'H' wide forces the next (distinct) glyph onto a new
    // shelf below — proving the shelf cursor advances in y, not just x.
    let font = load(NOTO);
    let h = font.glyph_id_for(0x0048).expect("'H' mapped");
    let n = font.glyph_id_for(0x004E).expect("'N' mapped");
    let h_w = font.rasterize_glyph(h, 48.0).expect("rasterizes").width;
    let mut atlas = GlyphAtlas::new(h_w); // width == one 'H' → any 2nd glyph wraps
    let a = atlas.get_or_insert(&font, h, 48.0).expect("rasterizes");
    let b = atlas.get_or_insert(&font, n, 48.0).expect("rasterizes");
    assert!(
        b.width > 0,
        "the wrapping glyph must have ink (else the wrap is vacuous)"
    );
    assert_eq!(a.y, 0, "first glyph on shelf 0");
    assert_eq!(b.x, 0, "wrapped glyph starts a fresh shelf at x=0");
    assert_eq!(
        b.y, a.height,
        "wrapped glyph sits exactly one shelf below: y={}",
        b.y
    );
}

#[test]
fn atlas_grow_preserves_prior_subrect_and_packs_new_shelf() {
    // Force `ensure_width` to widen a POPULATED row: a narrow 'i' fills shelf 0 in
    // an atlas exactly its width, then a wider 'M' wraps to shelf 1 AND grows the
    // atlas width — rewriting row 0's stride. Both glyphs' packed sub-rects must
    // read back byte-identical to standalone rasters: the growth-preservation
    // invariant (prior 'i' survives the rewrite) and a `g.y != 0` sub-rect read
    // (new 'M' on shelf 1). No same-shelf-only test exercises either path.
    let font = load(NOTO);
    let i = font.glyph_id_for(0x0069).expect("'i' mapped");
    let m = font.glyph_id_for(0x004D).expect("'M' mapped");
    let i_raw = font.rasterize_glyph(i, 48.0).expect("rasterizes");
    let m_raw = font.rasterize_glyph(m, 48.0).expect("rasterizes");
    assert!(
        m_raw.width > i_raw.width,
        "'M' must be wider than 'i' to force a widen"
    );

    let mut atlas = GlyphAtlas::new(i_raw.width); // exactly one 'i' wide
    let i_e = atlas.get_or_insert(&font, i, 48.0).expect("rasterizes");
    let m_e = atlas.get_or_insert(&font, m, 48.0).expect("rasterizes"); // wraps + widens
    assert!(
        atlas.width() >= m_raw.width,
        "atlas widened to fit the wider glyph"
    );
    assert!(
        m_e.y >= i_e.height && m_e.y > 0,
        "wider glyph on a new shelf (g.y != 0)"
    );

    let aw = atlas.width();
    // 'i' sub-rect survived the row-stride rewrite byte-for-byte.
    for y in 0..i_e.height {
        for x in 0..i_e.width {
            assert_eq!(
                atlas.alpha()[(i_e.y + y) * aw + i_e.x + x],
                i_raw.alpha[y * i_raw.width + x],
                "'i' byte ({x},{y}) corrupted by the widen",
            );
        }
    }
    // 'M' sub-rect on shelf 1 (g.y != 0) reads back byte-for-byte.
    for y in 0..m_e.height {
        for x in 0..m_e.width {
            assert_eq!(
                atlas.alpha()[(m_e.y + y) * aw + m_e.x + x],
                m_raw.alpha[y * m_raw.width + x],
                "'M' byte ({x},{y}) on shelf 1 mismatched",
            );
        }
    }
}

#[test]
fn atlas_caches_blank_glyph_as_zero_rect() {
    // A space is an empty outline: cached (not re-rasterized) but packs nothing.
    let font = load(NOTO);
    let mut atlas = GlyphAtlas::new(64);
    let space = font.glyph_id_for(0x0020).expect("Noto maps U+0020 space");
    let entry = atlas
        .get_or_insert(&font, space, 32.0)
        .expect("blank rasterizes");
    assert_eq!((entry.width, entry.height), (0, 0), "blank packs nothing");
    assert_eq!(atlas.len(), 1, "blank is still cached");
    assert_eq!(
        atlas.get_or_insert(&font, space, 32.0).expect("cache hit"),
        entry,
        "stable"
    );
}
