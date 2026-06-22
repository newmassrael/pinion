//! R50.7 §5.37.6 — GSUB end-to-end navigation tests over a hand-built table.
//!
//! The leaf parsing (`LigatureSubst`) is unit-tested in `ligature.rs`; these
//! assemble a whole GSUB table (header → `ScriptList` → `FeatureList` → `LookupList`
//! → `LigatureSubst`) to prove the shared Script→LangSys→Feature→Lookup→subtable
//! resolution (the same `layout` machinery GPOS uses), the Extension (Type 7)
//! unwrap, the `liga` feature-tag gate, and the `substitute()` origin mapping.

use super::*;

/// `LigatureSubst` format 1: 'f'(10) 'f'(10) 'i'(11) → ffi(700). 26 bytes.
fn ligature_subst() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&1u16.to_be_bytes()); // substFormat
    b.extend_from_slice(&8u16.to_be_bytes()); // coverageOffset (after 8-byte header)
    b.extend_from_slice(&1u16.to_be_bytes()); // ligatureSetCount
    b.extend_from_slice(&14u16.to_be_bytes()); // ligatureSetOffset[0] (after coverage)
    // coverage (format 1) @ 8: covers glyph 10
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&10u16.to_be_bytes());
    // ligatureSet @ 14
    b.extend_from_slice(&1u16.to_be_bytes()); // ligatureCount
    b.extend_from_slice(&4u16.to_be_bytes()); // ligatureOffset[0] (after 4-byte header)
    // Ligature @ set+4
    b.extend_from_slice(&700u16.to_be_bytes()); // ligatureGlyph
    b.extend_from_slice(&3u16.to_be_bytes()); // componentCount (f f i = 3)
    b.extend_from_slice(&10u16.to_be_bytes()); // component[0] (2nd glyph)
    b.extend_from_slice(&11u16.to_be_bytes()); // component[1] (3rd glyph)
    assert_eq!(b.len(), 26);
    b
}

/// Wrap a subtable in an Extension (Type 7) header → real type 4. 8 + body.
fn extension_wrap(body: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&1u16.to_be_bytes()); // substFormat (extension = 1)
    b.extend_from_slice(&4u16.to_be_bytes()); // extensionLookupType = Ligature
    b.extend_from_slice(&8u32.to_be_bytes()); // extensionOffset (after 8-byte header)
    b.extend_from_slice(body);
    b
}

/// Build a complete GSUB table: one script (DFLT) → one feature (`feature_tag`)
/// → one Lookup of `lookup_type` wrapping `subtable`. The shared table-assembly
/// SSOT for the `LigatureSubst` ([`build_gsub`]) and `SingleSubst` cases.
fn build_gsub_lookup(feature_tag: [u8; 4], lookup_type: u16, subtable: &[u8]) -> Vec<u8> {
    // ── ScriptList (20 bytes): DFLT → default LangSys → feature index 0 ──
    let mut script_list = Vec::new();
    script_list.extend_from_slice(&1u16.to_be_bytes()); // scriptCount
    script_list.extend_from_slice(b"DFLT"); // scriptTag
    script_list.extend_from_slice(&8u16.to_be_bytes()); // scriptOffset (rel SL)
    script_list.extend_from_slice(&4u16.to_be_bytes()); // defaultLangSysOffset (rel script)
    script_list.extend_from_slice(&0u16.to_be_bytes()); // langSysCount
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

/// A complete GSUB table with `feature_tag` → one `LigatureSubst` (ffi),
/// optionally Extension(Type 7)-wrapped.
fn build_gsub(feature_tag: [u8; 4], use_extension: bool) -> Vec<u8> {
    let subst = ligature_subst();
    if use_extension {
        build_gsub_lookup(feature_tag, 7, &extension_wrap(&subst))
    } else {
        build_gsub_lookup(feature_tag, 4, &subst)
    }
}

/// A `SingleSubst` format 1: covers `glyph`, maps it to `glyph + delta`. 12 bytes
/// (substFormat + coverageOffset + deltaGlyphID + a 6-byte single-glyph Coverage).
fn single_subst_format1(glyph: u16, delta: i16) -> Vec<u8> {
    let mut b = 1u16.to_be_bytes().to_vec(); // substFormat
    b.extend_from_slice(&6u16.to_be_bytes()); // coverageOffset (after 6-byte header)
    b.extend_from_slice(&delta.to_be_bytes()); // deltaGlyphID
    b.extend_from_slice(&1u16.to_be_bytes()); // coverageFormat 1
    b.extend_from_slice(&1u16.to_be_bytes()); // glyphCount
    b.extend_from_slice(&glyph.to_be_bytes());
    b
}

#[test]
fn resolves_ligature_through_full_navigation() {
    let gsub = Gsub::parse(&build_gsub(*b"liga", false)).unwrap();
    assert!(gsub.has_ligatures());
    // f f i → one ffi glyph, origin = index 0 (first component).
    assert_eq!(gsub.substitute(&[10, 10, 11]), vec![(700, 0)]);
    // Ligature at an offset: leading glyph 5 stays, ffi collapses at index 1.
    assert_eq!(gsub.substitute(&[5, 10, 10, 11]), vec![(5, 0), (700, 1)]);
    // Too short / wrong components → no substitution, identity origins.
    assert_eq!(gsub.substitute(&[10, 11]), vec![(10, 0), (11, 1)]);
    assert_eq!(
        gsub.substitute(&[10, 10, 99]),
        vec![(10, 0), (10, 1), (99, 2)]
    );
}

#[test]
fn resolves_ligature_through_extension_lookup() {
    // Type-7 extension must unwrap to the underlying LigatureSubst transparently.
    let gsub = Gsub::parse(&build_gsub(*b"liga", true)).unwrap();
    assert!(gsub.has_ligatures());
    assert_eq!(gsub.substitute(&[10, 10, 11]), vec![(700, 0)]);
}

#[test]
fn non_liga_feature_yields_no_substitution() {
    // Same structure under the 'calt' tag — the liga gate must reject it.
    let gsub = Gsub::parse(&build_gsub(*b"calt", false)).unwrap();
    assert!(!gsub.has_ligatures());
    assert_eq!(
        gsub.substitute(&[10, 10, 11]),
        vec![(10, 0), (10, 1), (11, 2)]
    );
}

#[test]
fn reject_major_version_other_than_one() {
    let mut table = build_gsub(*b"liga", false);
    table[0..2].copy_from_slice(&2u16.to_be_bytes());
    assert!(matches!(
        Gsub::parse(&table),
        Err(ParseError::UnsupportedTableVersion { major: 2, .. })
    ));
}

#[test]
fn empty_offsets_yield_no_substitution() {
    let mut table = Vec::new();
    table.extend_from_slice(&1u16.to_be_bytes());
    table.extend_from_slice(&0u16.to_be_bytes());
    table.extend_from_slice(&0u16.to_be_bytes()); // scriptListOffset = 0
    table.extend_from_slice(&0u16.to_be_bytes()); // featureListOffset = 0
    table.extend_from_slice(&0u16.to_be_bytes()); // lookupListOffset = 0
    let gsub = Gsub::parse(&table).unwrap();
    assert!(!gsub.has_ligatures());
    assert_eq!(
        gsub.substitute(&[10, 10, 11]),
        vec![(10, 0), (10, 1), (11, 2)]
    );
}

/// `LigatureSubst` format 1, parameterized: `first` + `components` → `lig`.
/// `22 + 2*components.len()` bytes.
fn lig_subst(first: u16, components: &[u16], lig: u16) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&1u16.to_be_bytes()); // substFormat
    b.extend_from_slice(&8u16.to_be_bytes()); // coverageOffset (after 8-byte header)
    b.extend_from_slice(&1u16.to_be_bytes()); // ligatureSetCount
    b.extend_from_slice(&14u16.to_be_bytes()); // ligatureSetOffset[0] (after coverage)
    // coverage (format 1) @ 8: covers `first`
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&first.to_be_bytes());
    // ligatureSet @ 14
    b.extend_from_slice(&1u16.to_be_bytes()); // ligatureCount
    b.extend_from_slice(&4u16.to_be_bytes()); // ligatureOffset[0]
    // Ligature @ set+4
    b.extend_from_slice(&lig.to_be_bytes());
    let component_count = u16::try_from(components.len() + 1).unwrap();
    b.extend_from_slice(&component_count.to_be_bytes());
    for &c in components {
        b.extend_from_slice(&c.to_be_bytes());
    }
    b
}

/// Wrap a subtable in a Type-4 (ligature) Lookup. `8 + subtable` bytes.
fn liga_lookup(subtable: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&4u16.to_be_bytes()); // lookupType
    b.extend_from_slice(&0u16.to_be_bytes()); // lookupFlag
    b.extend_from_slice(&1u16.to_be_bytes()); // subTableCount
    b.extend_from_slice(&8u16.to_be_bytes()); // subtableOffset[0] (after 8-byte header)
    b.extend_from_slice(subtable);
    b
}

/// Build a GSUB whose `liga` feature references TWO lookups in order:
/// lookup 0 (f f → ff/600), lookup 1 (ff i → ffi/700). Exercises sequential
/// chaining (lookup 0's output feeds lookup 1) and origin propagation.
fn build_gsub_two_lookups() -> Vec<u8> {
    // ScriptList (20): DFLT → default LangSys → feature 0.
    let mut script_list = Vec::new();
    script_list.extend_from_slice(&1u16.to_be_bytes());
    script_list.extend_from_slice(b"DFLT");
    script_list.extend_from_slice(&8u16.to_be_bytes());
    script_list.extend_from_slice(&4u16.to_be_bytes());
    script_list.extend_from_slice(&0u16.to_be_bytes());
    script_list.extend_from_slice(&0u16.to_be_bytes());
    script_list.extend_from_slice(&0xFFFFu16.to_be_bytes());
    script_list.extend_from_slice(&1u16.to_be_bytes());
    script_list.extend_from_slice(&0u16.to_be_bytes());
    assert_eq!(script_list.len(), 20);

    // FeatureList (16): liga → lookup indices [0, 1].
    let mut feature_list = Vec::new();
    feature_list.extend_from_slice(&1u16.to_be_bytes()); // featureCount
    feature_list.extend_from_slice(b"liga");
    feature_list.extend_from_slice(&8u16.to_be_bytes()); // featureOffset
    feature_list.extend_from_slice(&0u16.to_be_bytes()); // featureParams
    feature_list.extend_from_slice(&2u16.to_be_bytes()); // lookupIndexCount
    feature_list.extend_from_slice(&0u16.to_be_bytes()); // lookupListIndices[0]
    feature_list.extend_from_slice(&1u16.to_be_bytes()); // lookupListIndices[1]
    assert_eq!(feature_list.len(), 16);

    // LookupList: two ligature lookups, chained.
    let lookup0 = liga_lookup(&lig_subst(10, &[10], 600)); // f f → ff
    let lookup1 = liga_lookup(&lig_subst(600, &[11], 700)); // ff i → ffi
    let mut lookup_list = Vec::new();
    lookup_list.extend_from_slice(&2u16.to_be_bytes()); // lookupCount
    let off0 = 6u16; // after 2 + 2*2 header
    let off1 = off0 + u16::try_from(lookup0.len()).unwrap();
    lookup_list.extend_from_slice(&off0.to_be_bytes());
    lookup_list.extend_from_slice(&off1.to_be_bytes());
    lookup_list.extend_from_slice(&lookup0);
    lookup_list.extend_from_slice(&lookup1);

    let script_list_off = 10u16;
    let feature_list_off = script_list_off + u16::try_from(script_list.len()).unwrap();
    let lookup_list_off = feature_list_off + u16::try_from(feature_list.len()).unwrap();
    let mut table = Vec::new();
    table.extend_from_slice(&1u16.to_be_bytes());
    table.extend_from_slice(&0u16.to_be_bytes());
    table.extend_from_slice(&script_list_off.to_be_bytes());
    table.extend_from_slice(&feature_list_off.to_be_bytes());
    table.extend_from_slice(&lookup_list_off.to_be_bytes());
    table.extend_from_slice(&script_list);
    table.extend_from_slice(&feature_list);
    table.extend_from_slice(&lookup_list);
    table
}

#[test]
fn sequential_lookups_chain_and_preserve_origin() {
    // lookup 0: f f → ff(600); lookup 1: ff i → ffi(700). Lookup 0's output feeds
    // lookup 1, and the final glyph must carry the FIRST input component's origin
    // (0) through both substitutions — the load-bearing cluster invariant.
    let gsub = Gsub::parse(&build_gsub_two_lookups()).unwrap();
    assert!(gsub.has_ligatures());
    assert_eq!(
        gsub.substitute(&[10, 10, 11]),
        vec![(700, 0)],
        "f f i → ffi across 2 chained lookups, origin propagated to 0"
    );
    // A leading glyph 5 stays; the chain runs from index 1, origin 1.
    assert_eq!(gsub.substitute(&[5, 10, 10, 11]), vec![(5, 0), (700, 1)]);
    // Partial chain: only lookup 0 fires when lookup 1 can't complete (no 'i').
    assert_eq!(gsub.substitute(&[10, 10]), vec![(600, 0)]);
}

#[test]
fn resolves_ccmp_single_substitution() {
    // ccmp feature → Lookup Type 1 SingleSubst: glyph 10 maps to 10 + delta 5 = 15.
    // Proves the Script→Feature(ccmp)→Lookup(1)→SingleSubst chain and that
    // substitute() applies it one-to-one (origins preserved).
    let gsub = Gsub::parse(&build_gsub_lookup(
        *b"ccmp",
        1,
        &single_subst_format1(10, 5),
    ))
    .unwrap();
    assert!(gsub.has_ccmp(), "ccmp reachable from the default script");
    assert!(!gsub.has_ligatures(), "no liga lookups");
    assert_eq!(
        gsub.substitute(&[10, 20]),
        vec![(15, 0), (20, 1)],
        "10 composed to 15 in place, 20 untouched, origins unchanged"
    );
}

#[test]
fn ccmp_gate_rejects_wrong_feature_and_type() {
    // The SAME SingleSubst under the `liga` feature is picked up by neither channel:
    // ccmp is absent, and the liga collector keeps only Type 4 (skips Type 1).
    let gsub = Gsub::parse(&build_gsub_lookup(
        *b"liga",
        1,
        &single_subst_format1(10, 5),
    ))
    .unwrap();
    assert!(
        !gsub.has_ccmp(),
        "single sub under liga is not the ccmp channel"
    );
    assert!(
        !gsub.has_ligatures(),
        "liga collects Type 4 only, skips Type 1"
    );
    assert_eq!(
        gsub.substitute(&[10]),
        vec![(10, 0)],
        "no substitution applied"
    );
}
