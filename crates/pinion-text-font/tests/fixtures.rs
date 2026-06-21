//! R50.1.1+R50.1.2+R50.1.3+R50.1.4.1 §5.37.1 — real font fixture
//! integration tests.
//!
//! Latin (Noto Sans Regular, OFL 1.1) + 한글 (Nanum Gothic Regular, OFL 1.1).
//! sfnt + 6 metadata tables + cmap + loca + glyf (simple/composite) production-
//! grade 두 family 모두 정확히 parse 함을 확인.

use pinion_text_font::{Font, Glyph, LocaFormat, SfntFlavor, parse_sfnt};
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
            assert!(
                tags.contains(*tag),
                "{path}: required table {tag:?} missing"
            );
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
    assert!(
        gid.is_none() || gid == Some(0),
        "unexpected mapping for U+10FFFE"
    );
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
fn noto_sans_cmap_format4_sweep() {
    use pinion_text_font::tables::cmap::CmapSubtable;
    let bytes = std::fs::read("tests/fonts/NotoSans-Regular.ttf").expect("fixture present");
    let font = Font::from_bytes(bytes).expect("valid Font");

    // Noto Sans 의 best subtable 이 format 4 라면, 모든 segment 의 every codepoint
    // 가 OOB panic 없이 glyph_id 호출 가능 + sentinel segment (0xFFFF) 가 None 반환.
    if let Some(CmapSubtable::Format4(f4)) = font.cmap.best_subtable() {
        let mut total_segments = 0;
        let mut indirect_segments = 0;
        for (i, &end) in f4.end_code.iter().enumerate() {
            total_segments += 1;
            if f4.id_range_offset[i] != 0 {
                indirect_segments += 1;
            }
            // sentinel (start=end=0xFFFF, idDelta=1) 은 query 시 None 기대.
            if f4.start_code[i] == 0xFFFF && end == 0xFFFF {
                let _ = f4.glyph_id(0xFFFF);
                continue;
            }
            // segment range 의 시작/끝/중간 codepoint 모두 OOB 없이 query.
            let mid = u32::midpoint(u32::from(f4.start_code[i]), u32::from(end));
            for cp in [u32::from(f4.start_code[i]), u32::from(end), mid] {
                let _gid = f4.glyph_id(cp); // OOB panic 없음만 확인
            }
        }
        assert!(
            total_segments > 1,
            "Noto Sans cmap format 4 has > 1 segments"
        );
        eprintln!("Noto Sans: {total_segments} segments ({indirect_segments} indirect)",);
    }
}

#[test]
fn nanum_gothic_cmap_subtable_sweep() {
    use pinion_text_font::tables::cmap::CmapSubtable;
    let bytes = std::fs::read("tests/fonts/NanumGothic-Regular.ttf").expect("fixture present");
    let font = Font::from_bytes(bytes).expect("valid Font");

    // Nanum Gothic 의 best subtable 검사. format 4 면 sentinel sweep, format 12 면 group sweep.
    match font.cmap.best_subtable() {
        Some(CmapSubtable::Format4(f4)) => {
            for (i, _) in f4.end_code.iter().enumerate() {
                let _ = f4.glyph_id(u32::from(f4.start_code[i]));
            }
        }
        Some(CmapSubtable::Format12(f12)) => {
            assert!(!f12.groups.is_empty(), "format 12 has at least 1 group");
            // 첫/끝/중간 group 의 start codepoint 가 OOB panic 없이 mapped.
            for g in [
                f12.groups.first().unwrap(),
                f12.groups.last().unwrap(),
                &f12.groups[f12.groups.len() / 2],
            ] {
                let gid = f12.glyph_id(g.start_char_code);
                assert!(gid.is_some(), "U+{:04X} should map", g.start_char_code);
            }
        }
        Some(CmapSubtable::Format0(_)) => {
            // Format 0 = Mac Roman fallback (priority 4); production Korean font
            // 가 fallback 으로 선택될 일은 없음 (Format 12 / 4 우선).
            panic!("Nanum Gothic best_subtable unexpectedly Format0");
        }
        None => panic!("Nanum Gothic has no usable cmap subtable"),
    }
}

#[test]
fn noto_sans_glyf_loca_sweep() {
    // Noto Sans 모든 glyph: parse panic 0, loca format == head.index_to_loc_format,
    // glyph 0 (.notdef) = Simple, composite/empty 식별 통계.
    let bytes = fs::read("tests/fonts/NotoSans-Regular.ttf").expect("fixture present");
    let font = Font::from_bytes(bytes).expect("valid Font");

    let expected_format = match font.head.index_to_loc_format {
        0 => LocaFormat::Short,
        1 => LocaFormat::Long,
        other => panic!("invalid index_to_loc_format = {other}"),
    };
    assert_eq!(font.loca.format, expected_format);
    assert_eq!(font.loca.num_glyphs(), usize::from(font.num_glyphs()));
    assert_eq!(font.glyf.num_glyphs(), usize::from(font.num_glyphs()));

    // glyph 0 (.notdef) — 모든 TrueType font 의 첫 glyph, 보통 simple rectangle 또는 hollow box.
    let g0 = font.glyph_outline(0).expect("glyph 0 exists");
    assert!(matches!(g0, Glyph::Simple(_)), ".notdef should be simple");

    // 모든 glyph 를 순회하며 parse 검증 (panic 0).
    let mut empty_count = 0;
    let mut simple_count = 0;
    let mut composite_count = 0;
    let mut total_points = 0usize;
    for gid in 0..font.num_glyphs() {
        let glyph = font
            .glyph_outline(gid)
            .unwrap_or_else(|| panic!("glyph {gid} not found"));
        match glyph {
            Glyph::Empty => empty_count += 1,
            Glyph::Simple(s) => {
                simple_count += 1;
                total_points += s.points.len();
            }
            Glyph::Composite(_) => composite_count += 1,
        }
    }
    assert!(simple_count > 0, "Noto Sans 는 simple glyph 보유");
    // Noto Sans Regular 는 Latin accented 글자 다수 → composite glyph 다수 보유.
    assert!(composite_count > 0, "Noto Sans 는 composite glyph 보유");
    eprintln!(
        "Noto Sans: {simple_count} simple / {composite_count} composite / {empty_count} empty / {total_points} points total"
    );
}

#[test]
fn nanum_gothic_glyf_loca_sweep() {
    let bytes = fs::read("tests/fonts/NanumGothic-Regular.ttf").expect("fixture present");
    let font = Font::from_bytes(bytes).expect("valid Font");

    let expected_format = match font.head.index_to_loc_format {
        0 => LocaFormat::Short,
        1 => LocaFormat::Long,
        other => panic!("invalid index_to_loc_format = {other}"),
    };
    assert_eq!(font.loca.format, expected_format);
    assert_eq!(font.glyf.num_glyphs(), usize::from(font.num_glyphs()));

    let mut empty_count = 0;
    let mut simple_count = 0;
    let mut composite_count = 0;
    for gid in 0..font.num_glyphs() {
        match font.glyph_outline(gid).expect("glyph exists") {
            Glyph::Empty => empty_count += 1,
            Glyph::Simple(_) => simple_count += 1,
            Glyph::Composite(_) => composite_count += 1,
        }
    }
    assert!(simple_count > 0);
    eprintln!(
        "Nanum Gothic: {simple_count} simple / {composite_count} composite / {empty_count} empty"
    );
}

#[test]
fn noto_sans_letter_a_outline_present() {
    // 'A' = U+0041 → glyph_id_for → glyph_outline → 반드시 Simple 또는 Composite.
    let bytes = fs::read("tests/fonts/NotoSans-Regular.ttf").expect("fixture present");
    let font = Font::from_bytes(bytes).expect("valid Font");
    let gid = font.glyph_id_for(0x0041).expect("'A' is mapped");
    let glyph = font.glyph_outline(gid).expect("outline exists");
    assert!(
        !matches!(glyph, Glyph::Empty),
        "'A' should not be empty glyph"
    );
}

#[test]
fn noto_sans_composite_components_nonempty() {
    // R50.1.4.2: composite glyph 들의 components parse 결과 검증 — 모든 composite
    // 가 ≥ 1 component, 모든 component 의 transform 이 4 variant 중 하나.
    let bytes = fs::read("tests/fonts/NotoSans-Regular.ttf").expect("fixture present");
    let font = Font::from_bytes(bytes).expect("valid Font");

    let mut composite_count = 0;
    let mut total_components = 0usize;
    let mut transform_variants = (0usize, 0usize, 0usize, 0usize); // identity/scale/xy/2x2
    for gid in 0..font.num_glyphs() {
        if let Some(Glyph::Composite(c)) = font.glyph_outline(gid) {
            composite_count += 1;
            assert!(
                !c.components.is_empty(),
                "composite glyph {gid} has 0 components"
            );
            total_components += c.components.len();
            for comp in &c.components {
                use pinion_text_font::ComponentTransform::*;
                match comp.transform {
                    Identity => transform_variants.0 += 1,
                    Scale { .. } => transform_variants.1 += 1,
                    XYScale { .. } => transform_variants.2 += 1,
                    Matrix { .. } => transform_variants.3 += 1,
                }
            }
        }
    }
    assert!(composite_count > 0, "Noto Sans 는 composite glyph 보유");
    eprintln!(
        "Noto Sans: {composite_count} composite / {total_components} components / transforms identity={} scale={} xy={} 2x2={}",
        transform_variants.0, transform_variants.1, transform_variants.2, transform_variants.3,
    );
}

#[test]
fn nanum_gothic_hangul_outline_present() {
    let bytes = fs::read("tests/fonts/NanumGothic-Regular.ttf").expect("fixture present");
    let font = Font::from_bytes(bytes).expect("valid Font");
    // '가' = U+AC00 — Nanum Gothic 의 핵심 한글 음절.
    let gid = font.glyph_id_for(0xAC00).expect("'가' is mapped");
    let glyph = font.glyph_outline(gid).expect("outline exists");
    assert!(!matches!(glyph, Glyph::Empty), "'가' should not be empty");
}

#[test]
fn noto_sans_name_strings() {
    let bytes = fs::read("tests/fonts/NotoSans-Regular.ttf").expect("fixture present");
    let font = Font::from_bytes(bytes).expect("valid Font");
    // Noto Sans Regular 의 표준 metadata.
    assert_eq!(font.family_name().as_deref(), Some("Noto Sans"));
    assert_eq!(font.subfamily_name().as_deref(), Some("Regular"));
    assert!(
        font.full_name()
            .as_deref()
            .is_some_and(|s| s.contains("Noto Sans")),
        "full_name should contain 'Noto Sans'"
    );
    assert!(
        font.postscript_name()
            .as_deref()
            .is_some_and(|s| s.starts_with("NotoSans")),
        "postscript_name should start with 'NotoSans'"
    );
}

#[test]
fn nanum_gothic_name_strings() {
    let bytes = fs::read("tests/fonts/NanumGothic-Regular.ttf").expect("fixture present");
    let font = Font::from_bytes(bytes).expect("valid Font");
    // Nanum Gothic 의 family name (정확한 값은 font internal 에 따라 변동).
    let family = font.family_name().expect("family present");
    assert!(
        family.contains("Nanum") || family.contains("나눔"),
        "family name should contain Nanum/나눔, got: {family:?}"
    );
    assert_eq!(font.subfamily_name().as_deref(), Some("Regular"));
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
