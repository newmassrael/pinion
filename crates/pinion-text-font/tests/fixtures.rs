//! R50.1.1 §5.37.1 — real font fixture integration tests.
//!
//! Latin (Noto Sans Regular, OFL 1.1) + 한글 (Nanum Gothic Regular, OFL 1.1).
//! sfnt parser 가 production-grade 두 family 모두 정확히 parse 함을 확인.

use pinion_text_font::{SfntFlavor, parse_sfnt};
use std::fs;

#[test]
fn parse_noto_sans_regular() {
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
fn parse_nanum_gothic_regular() {
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
