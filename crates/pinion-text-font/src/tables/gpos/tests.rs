//! R50.6.1 §5.37.6 — GPOS end-to-end navigation tests over a hand-built table.
//!
//! The per-table unit tests (coverage / classdef / pairpos) cover the leaves;
//! these assemble a whole GPOS table (header → `ScriptList` → `FeatureList` →
//! `LookupList` → `PairPos`) to prove the Script→LangSys→Feature→Lookup→subtable
//! resolution chain — including Extension (Type 9) unwrap and the `kern`
//! feature-tag gate.

use super::*;

const X_ADVANCE_FORMAT: u16 = 0x0004;

/// `PairPos` format 1: cover `first`, pair `(first, second)` → `x_adv`. 24 bytes.
fn pairpos_format1(first: u16, second: u16, x_adv: i16) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&1u16.to_be_bytes()); // posFormat
    b.extend_from_slice(&12u16.to_be_bytes()); // coverageOffset (after 12-byte header)
    b.extend_from_slice(&X_ADVANCE_FORMAT.to_be_bytes()); // valueFormat1
    b.extend_from_slice(&0u16.to_be_bytes()); // valueFormat2
    b.extend_from_slice(&1u16.to_be_bytes()); // pairSetCount
    b.extend_from_slice(&18u16.to_be_bytes()); // pairSetOffset[0] (after coverage)
    // coverage (format 1), 6 bytes
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&first.to_be_bytes());
    // pairSet, 6 bytes
    b.extend_from_slice(&1u16.to_be_bytes()); // pairValueCount
    b.extend_from_slice(&second.to_be_bytes());
    b.extend_from_slice(&x_adv.to_be_bytes()); // valueRecord1 = X_ADVANCE
    assert_eq!(b.len(), 24);
    b
}

/// Wrap a subtable in an `ExtensionPos` (Type 9) header → real type 2. 8 + body.
fn extension_wrap(body: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&1u16.to_be_bytes()); // posFormat
    b.extend_from_slice(&2u16.to_be_bytes()); // extensionLookupType = PairPos
    b.extend_from_slice(&8u32.to_be_bytes()); // extensionOffset (after 8-byte header)
    b.extend_from_slice(body);
    b
}

/// Build a complete GPOS table: one script (DFLT) → one feature (`feature_tag`)
/// → one Lookup of `lookup_type` wrapping `subtable`. The shared table-assembly
/// SSOT for the `PairPos` ([`build_gpos`]) and mark ([`mark_mark_subtable`]) cases.
fn build_gpos_lookup(feature_tag: [u8; 4], lookup_type: u16, subtable: &[u8]) -> Vec<u8> {
    // ── ScriptList (20 bytes): DFLT → default LangSys → feature index 0 ──
    let mut script_list = Vec::new();
    script_list.extend_from_slice(&1u16.to_be_bytes()); // scriptCount
    script_list.extend_from_slice(b"DFLT"); // scriptTag
    script_list.extend_from_slice(&8u16.to_be_bytes()); // scriptOffset (rel SL)
    // Script table @ SL+8
    script_list.extend_from_slice(&4u16.to_be_bytes()); // defaultLangSysOffset (rel script)
    script_list.extend_from_slice(&0u16.to_be_bytes()); // langSysCount
    // LangSys @ script+4
    script_list.extend_from_slice(&0u16.to_be_bytes()); // lookupOrder
    script_list.extend_from_slice(&0xFFFFu16.to_be_bytes()); // requiredFeatureIndex = none
    script_list.extend_from_slice(&1u16.to_be_bytes()); // featureIndexCount
    script_list.extend_from_slice(&0u16.to_be_bytes()); // featureIndices[0] = 0
    assert_eq!(script_list.len(), 20);

    // ── FeatureList (14 bytes): one feature → lookup index 0 ──
    let mut feature_list = Vec::new();
    feature_list.extend_from_slice(&1u16.to_be_bytes()); // featureCount
    feature_list.extend_from_slice(&feature_tag); // featureTag
    feature_list.extend_from_slice(&8u16.to_be_bytes()); // featureOffset (rel FL)
    // Feature table @ FL+8
    feature_list.extend_from_slice(&0u16.to_be_bytes()); // featureParams
    feature_list.extend_from_slice(&1u16.to_be_bytes()); // lookupIndexCount
    feature_list.extend_from_slice(&0u16.to_be_bytes()); // lookupListIndices[0] = 0
    assert_eq!(feature_list.len(), 14);

    // ── LookupList: one Lookup of `lookup_type` → `subtable` ──
    let mut lookup = Vec::new();
    lookup.extend_from_slice(&lookup_type.to_be_bytes()); // lookupType
    lookup.extend_from_slice(&0u16.to_be_bytes()); // lookupFlag
    lookup.extend_from_slice(&1u16.to_be_bytes()); // subTableCount
    lookup.extend_from_slice(&8u16.to_be_bytes()); // subtableOffset[0] (after 8-byte header)
    lookup.extend_from_slice(subtable);

    let mut lookup_list = Vec::new();
    lookup_list.extend_from_slice(&1u16.to_be_bytes()); // lookupCount
    lookup_list.extend_from_slice(&4u16.to_be_bytes()); // lookupOffset[0] (after 4-byte header)
    lookup_list.extend_from_slice(&lookup);

    // ── Header + concatenation. SL=10, FL=30, LL=44 (header is 10 bytes). ──
    let script_list_off = 10u16;
    let feature_list_off = script_list_off + u16::try_from(script_list.len()).unwrap();
    let lookup_list_off = feature_list_off + u16::try_from(feature_list.len()).unwrap();

    let mut table = Vec::new();
    table.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
    table.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
    table.extend_from_slice(&script_list_off.to_be_bytes());
    table.extend_from_slice(&feature_list_off.to_be_bytes());
    table.extend_from_slice(&lookup_list_off.to_be_bytes());
    assert_eq!(table.len(), 10);
    table.extend_from_slice(&script_list);
    table.extend_from_slice(&feature_list);
    table.extend_from_slice(&lookup_list);
    table
}

/// A complete GPOS table with `feature_tag` → one `PairPos` `(10,20) → -50`,
/// optionally Extension(Type 9)-wrapped.
fn build_gpos(feature_tag: [u8; 4], use_extension: bool) -> Vec<u8> {
    let pairpos = pairpos_format1(10, 20, -50);
    if use_extension {
        build_gpos_lookup(feature_tag, 9, &extension_wrap(&pairpos))
    } else {
        build_gpos_lookup(feature_tag, 2, &pairpos)
    }
}

/// A format-1 anchor table (`format, x, y`), 6 bytes.
fn anchor1(x: i16, y: i16) -> Vec<u8> {
    let mut b = 1u16.to_be_bytes().to_vec();
    b.extend_from_slice(&x.to_be_bytes());
    b.extend_from_slice(&y.to_be_bytes());
    b
}

/// A format-1 single-glyph Coverage, 6 bytes.
fn coverage1(glyph: u16) -> Vec<u8> {
    let mut b = 1u16.to_be_bytes().to_vec(); // coverageFormat
    b.extend_from_slice(&1u16.to_be_bytes()); // glyphCount
    b.extend_from_slice(&glyph.to_be_bytes());
    b
}

/// A `MarkMarkPos`/`MarkBasePos` format-1 subtable (one mark + one attachment
/// glyph, one mark class): `mark` carries anchor `mark_anchor`, the attachment
/// glyph `attach` carries `attach_anchor` for class 0. The byte layout serves
/// both Type 4 and Type 6 (the parser is shared) — the test picks the lookup type.
fn mark_mark_subtable(
    mark: u16,
    attach: u16,
    mark_anchor: (i16, i16),
    attach_anchor: (i16, i16),
) -> Vec<u8> {
    let mark_cov = coverage1(mark);
    let attach_cov = coverage1(attach);

    // markArray: markCount(2) + record(class 2 + anchorOff 2) = 6, anchor at +6.
    let mut mark_array = 1u16.to_be_bytes().to_vec(); // markCount
    mark_array.extend_from_slice(&0u16.to_be_bytes()); // markClass = 0
    mark_array.extend_from_slice(&6u16.to_be_bytes()); // markAnchorOffset
    mark_array.extend_from_slice(&anchor1(mark_anchor.0, mark_anchor.1));

    // attachArray (Mark2Array/BaseArray): count(2) + record(1 offset = 2) = 4, anchor at +4.
    let mut attach_array = 1u16.to_be_bytes().to_vec(); // count
    attach_array.extend_from_slice(&4u16.to_be_bytes()); // class-0 anchorOffset
    attach_array.extend_from_slice(&anchor1(attach_anchor.0, attach_anchor.1));

    let header_len = 12;
    let mark_cov_off = header_len;
    let attach_cov_off = mark_cov_off + mark_cov.len();
    let mark_array_off = attach_cov_off + attach_cov.len();
    let attach_array_off = mark_array_off + mark_array.len();

    let mut sub = 1u16.to_be_bytes().to_vec(); // posFormat
    sub.extend_from_slice(&u16::try_from(mark_cov_off).unwrap().to_be_bytes());
    sub.extend_from_slice(&u16::try_from(attach_cov_off).unwrap().to_be_bytes());
    sub.extend_from_slice(&1u16.to_be_bytes()); // markClassCount
    sub.extend_from_slice(&u16::try_from(mark_array_off).unwrap().to_be_bytes());
    sub.extend_from_slice(&u16::try_from(attach_array_off).unwrap().to_be_bytes());
    sub.extend_from_slice(&mark_cov);
    sub.extend_from_slice(&attach_cov);
    sub.extend_from_slice(&mark_array);
    sub.extend_from_slice(&attach_array);
    sub
}

#[test]
fn resolves_kern_through_full_navigation() {
    let gpos = Gpos::parse(&build_gpos(*b"kern", false)).unwrap();
    assert!(gpos.has_kerning());
    assert_eq!(gpos.kern_x_advance(10, 20), -50, "the registered kern pair");
    assert_eq!(
        gpos.kern_x_advance(10, 99),
        0,
        "covered first, no such pair"
    );
    assert_eq!(gpos.kern_x_advance(11, 20), 0, "first glyph not covered");
}

#[test]
fn resolves_kern_through_extension_lookup() {
    // Type-9 extension must unwrap to the underlying PairPos transparently.
    let gpos = Gpos::parse(&build_gpos(*b"kern", true)).unwrap();
    assert!(gpos.has_kerning());
    assert_eq!(gpos.kern_x_advance(10, 20), -50);
}

#[test]
fn non_kern_feature_yields_no_kerning() {
    // Same structure, but the feature is 'liga' — the kern gate must reject it.
    let gpos = Gpos::parse(&build_gpos(*b"liga", false)).unwrap();
    assert!(!gpos.has_kerning());
    assert_eq!(gpos.kern_x_advance(10, 20), 0);
}

#[test]
fn reject_major_version_other_than_one() {
    let mut table = build_gpos(*b"kern", false);
    table[0..2].copy_from_slice(&2u16.to_be_bytes());
    assert!(matches!(
        Gpos::parse(&table),
        Err(ParseError::UnsupportedTableVersion { major: 2, .. })
    ));
}

#[test]
fn empty_offsets_yield_no_kerning() {
    // A GPOS header whose script/feature/lookup offsets are all 0 is valid and
    // simply carries no positioning — kern is a no-op, not an error.
    let mut table = Vec::new();
    table.extend_from_slice(&1u16.to_be_bytes());
    table.extend_from_slice(&0u16.to_be_bytes());
    table.extend_from_slice(&0u16.to_be_bytes()); // scriptListOffset = 0
    table.extend_from_slice(&0u16.to_be_bytes()); // featureListOffset = 0
    table.extend_from_slice(&0u16.to_be_bytes()); // lookupListOffset = 0
    let gpos = Gpos::parse(&table).unwrap();
    assert!(!gpos.has_kerning());
    assert_eq!(gpos.kern_x_advance(10, 20), 0);
}

#[test]
fn resolves_mkmk_through_full_navigation() {
    // mkmk feature → Lookup Type 6 MarkMarkPos: the combining mark glyph 31 stacks
    // on the preceding mark glyph 30. attachAnchor (200,900) - markAnchor (50,100)
    // = delta (150, 800). Proves Script→Feature(mkmk)→Lookup(6)→subtable navigation.
    let sub = mark_mark_subtable(31, 30, (50, 100), (200, 900));
    let gpos = Gpos::parse(&build_gpos_lookup(*b"mkmk", 6, &sub)).unwrap();
    assert!(
        gpos.has_mark_marks(),
        "mkmk reachable from the default script"
    );
    assert!(!gpos.has_marks(), "mkmk is not the mark (Type 4) channel");
    assert_eq!(
        gpos.mark_mark_offset(30, 31),
        Some((150, 800)),
        "attachAnchor - markAnchor",
    );
    assert_eq!(gpos.mark_mark_offset(30, 99), None, "mark not covered");
    assert_eq!(
        gpos.mark_mark_offset(99, 31),
        None,
        "preceding mark not covered"
    );
    assert_eq!(gpos.mark_offset(30, 31), None, "no mark-to-base lookup");
}

#[test]
fn mark_and_mkmk_are_distinct_channels() {
    // The SAME subtable layout under a `mark` (Type 4) feature must populate
    // mark-to-base, not mark-to-mark — the two features are separate channels and
    // must not bleed into each other.
    let sub = mark_mark_subtable(31, 30, (50, 100), (200, 900));
    let gpos = Gpos::parse(&build_gpos_lookup(*b"mark", 4, &sub)).unwrap();
    assert!(gpos.has_marks(), "mark feature populates mark-to-base");
    assert!(!gpos.has_mark_marks(), "mark feature does not feed mkmk");
    assert_eq!(
        gpos.mark_offset(30, 31),
        Some((150, 800)),
        "mark-to-base offset"
    );
    assert_eq!(gpos.mark_mark_offset(30, 31), None, "no mkmk lookups");
}
