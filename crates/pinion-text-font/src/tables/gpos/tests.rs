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

/// Build a complete GPOS table with one script (DFLT) → one feature
/// (`feature_tag`) → one Lookup → one `PairPos` `(10,20) → -50`.
fn build_gpos(feature_tag: [u8; 4], use_extension: bool) -> Vec<u8> {
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

    // ── LookupList: one Lookup → PairPos (optionally extension-wrapped) ──
    let pairpos = pairpos_format1(10, 20, -50);
    let (lookup_type, subtable) = if use_extension {
        (9u16, extension_wrap(&pairpos))
    } else {
        (2u16, pairpos)
    };
    let mut lookup = Vec::new();
    lookup.extend_from_slice(&lookup_type.to_be_bytes()); // lookupType
    lookup.extend_from_slice(&0u16.to_be_bytes()); // lookupFlag
    lookup.extend_from_slice(&1u16.to_be_bytes()); // subTableCount
    lookup.extend_from_slice(&8u16.to_be_bytes()); // subtableOffset[0] (after 8-byte header)
    lookup.extend_from_slice(&subtable);

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
