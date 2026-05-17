//! R50.1.1+R50.1.2 §5.37.1 — real font fixture integration tests.
//!
//! Latin (Noto Sans Regular, OFL 1.1) + 한글 (Nanum Gothic Regular, OFL 1.1).
//! sfnt + 6 metadata tables (head/maxp/hhea/hmtx/OS2/post) production-grade
//! 두 family 모두 정확히 parse 함을 확인.

use pinion_text_font::{Font, SfntFlavor, parse_sfnt};
use std::fs;

#[test]
fn parse_noto_sans_regular_sfnt() {
    let bytes = fs::read("tests/fonts/NotoSans-Regular.ttf").expect("fixture present");
    let (header, records) = parse_sfnt(&bytes).expect("valid sfnt");

    assert_eq!(header.flavor, SfntFlavor::TrueType);
    assert_eq!(header.num_tables, 17);
    assert_eq!(records.len(), 17);

    // alphabetical order — DSIG 가 없으므로 GDEF 가 첫 record.
    assert_eq!(&records[0].tag, b"GDEF");

    // 각 record 의 offset+length 가 file 안에 들어감 (parser 가 이미 검증했지만
    // sanity check 로 한 번 더).
    for record in &records {
        let end = u64::from(record.offset) + u64::from(record.length);
        assert!(end <= bytes.len() as u64);
    }
}

#[test]
fn parse_nanum_gothic_regular_sfnt() {
    let bytes = fs::read("tests/fonts/NanumGothic-Regular.ttf").expect("fixture present");
    let (header, records) = parse_sfnt(&bytes).expect("valid sfnt");

    assert_eq!(header.flavor, SfntFlavor::TrueType);
    assert_eq!(header.num_tables, 17);
    assert_eq!(records.len(), 17);

    // Nanum Gothic 은 digitally signed — DSIG 가 alphabetical first.
    assert_eq!(&records[0].tag, b"DSIG");

    for record in &records {
        let end = u64::from(record.offset) + u64::from(record.length);
        assert!(end <= bytes.len() as u64);
    }
}

#[test]
fn both_fixtures_have_common_required_tables() {
    // OpenType spec 의 required tables (TrueType): cmap, glyf, head, hhea,
    // hmtx, loca, maxp, name, post. 두 fixture 모두 보유 검증.
    let required: &[&[u8; 4]] = &[
        b"cmap", b"glyf", b"head", b"hhea", b"hmtx", b"loca", b"maxp", b"name", b"post",
    ];

    for path in [
        "tests/fonts/NotoSans-Regular.ttf",
        "tests/fonts/NanumGothic-Regular.ttf",
    ] {
        let bytes = fs::read(path).expect("fixture present");
        let (_, records) = parse_sfnt(&bytes).expect("valid sfnt");
        let tags: std::collections::HashSet<[u8; 4]> = records.iter().map(|r| r.tag).collect();
        for tag in required {
            assert!(tags.contains(*tag), "{path}: required table {tag:?} missing");
        }
    }
}

#[test]
fn parse_noto_sans_font_full() {
    let bytes = fs::read("tests/fonts/NotoSans-Regular.ttf").expect("fixture present");
    let font = Font::from_bytes(bytes).expect("valid Font");

    // Noto Sans Regular has units_per_em = 1000 (Google default for Noto family).
    assert_eq!(font.units_per_em(), 1000);

    // glyph count > 0 + within u16 range.
    assert!(font.num_glyphs() > 0);

    // ascender > 0, descender < 0 (typographic convention).
    assert!(font.ascender() > 0);
    assert!(font.descender() < 0);

    // glyph 0 (.notdef) — every TrueType font 의 첫 glyph. advance width 존재.
    assert!(font.glyph_advance_width(0).is_some());

    // out-of-range glyph — None.
    assert!(font.glyph_advance_width(font.num_glyphs()).is_none());

    // weight class — Noto Sans Regular = 400.
    assert_eq!(font.weight_class(), 400);

    // Noto Sans 는 proportional (not monospace).
    assert!(!font.is_monospace());
}

#[test]
fn parse_nanum_gothic_font_full() {
    let bytes = fs::read("tests/fonts/NanumGothic-Regular.ttf").expect("fixture present");
    let font = Font::from_bytes(bytes).expect("valid Font");

    // Nanum Gothic units_per_em (Naver: 1000 default).
    assert_eq!(font.units_per_em(), 1000);

    // 한글 family — glyph count 가 Latin family 보다 훨씬 많음 (한자 + 한글 음절 다수).
    assert!(font.num_glyphs() > 10_000);

    assert!(font.ascender() > 0);
    assert!(font.descender() < 0);

    // 한글 음절 '가' (U+AC00) 의 glyph index 는 cmap 통해서 (R50.1.3) — 다만
    // glyph_id=1 (.notdef 다음 glyph) 의 advance 는 존재.
    assert!(font.glyph_advance_width(1).is_some());

    // weight class — Nanum Gothic Regular = 400.
    assert_eq!(font.weight_class(), 400);
}

#[test]
fn noto_sans_cmap_ascii_letters() {
    let bytes = std::fs::read("tests/fonts/NotoSans-Regular.ttf").expect("fixture present");
    let font = Font::from_bytes(bytes).expect("valid Font");

    // ASCII letters (U+0041 'A' .. U+005A 'Z') 모두 mapped (non-zero glyph).
    for cp in 0x0041u32..=0x005A {
        let gid = font.glyph_id_for(cp);
        assert!(gid.is_some(), "U+{cp:04X} not mapped in Noto Sans");
        assert_ne!(gid, Some(0), "U+{cp:04X} maps to .notdef");
    }
    // ASCII digits.
    for cp in 0x0030u32..=0x0039 {
        let gid = font.glyph_id_for(cp).unwrap_or(0);
        assert_ne!(gid, 0, "U+{cp:04X} not mapped");
    }
    // Unassigned codepoint (private use area) → None or .notdef.
    let gid = font.glyph_id_for(0x10_FFFE);
    assert!(gid.is_none() || gid == Some(0), "unexpected mapping for U+10FFFE");
}

#[test]
fn nanum_gothic_cmap_hangul_block() {
    let bytes = std::fs::read("tests/fonts/NanumGothic-Regular.ttf").expect("fixture present");
    let font = Font::from_bytes(bytes).expect("valid Font");

    // 한글 음절 blocks (U+AC00 '가' .. U+AC0F) 모두 mapped — Nanum Gothic 의
    // 핵심 coverage area.
    for cp in 0xAC00u32..=0xAC0F {
        let gid = font.glyph_id_for(cp);
        assert!(gid.is_some(), "U+{cp:04X} not mapped in Nanum Gothic");
        assert_ne!(gid, Some(0), "U+{cp:04X} maps to .notdef");
    }

    // 한글 음절 마지막 '힣' (U+D7A3) 도 mapped.
    let gid = font.glyph_id_for(0xD7A3);
    assert!(gid.is_some());
    assert_ne!(gid, Some(0));
}

#[test]
fn both_fixtures_map_basic_ascii() {
    for path in [
        "tests/fonts/NotoSans-Regular.ttf",
        "tests/fonts/NanumGothic-Regular.ttf",
    ] {
        let bytes = std::fs::read(path).expect("fixture present");
        let font = Font::from_bytes(bytes).expect("valid Font");
        // 'A' = U+0041 maps to non-zero glyph in both fonts.
        let gid = font.glyph_id_for(0x0041);
        assert!(gid.is_some() && gid != Some(0), "{path}: 'A' not mapped");
    }
}

#[test]
fn font_metadata_consistency() {
    // hhea.number_of_h_metrics 가 hmtx 의 long_metrics.len() 와 일치 검증.
    for path in [
        "tests/fonts/NotoSans-Regular.ttf",
        "tests/fonts/NanumGothic-Regular.ttf",
    ] {
        let bytes = fs::read(path).expect("fixture present");
        let font = Font::from_bytes(bytes).expect("valid Font");
        assert_eq!(
            font.hmtx.long_metrics.len(),
            font.hhea.number_of_h_metrics as usize,
            "{path}: hmtx.long_metrics 와 hhea.number_of_h_metrics 불일치",
        );
        assert_eq!(
            font.hmtx.num_glyphs(),
            font.maxp.num_glyphs as usize,
            "{path}: hmtx total glyph count 와 maxp.num_glyphs 불일치",
        );
    }
}
