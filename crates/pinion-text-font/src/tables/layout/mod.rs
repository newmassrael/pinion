//! R50.7 §5.37.6 — OpenType Layout common machinery (GPOS + GSUB).
//!
//! GPOS and GSUB share an identical header (majorVersion, minorVersion,
//! scriptListOffset, featureListOffset, lookupListOffset) and the same
//! `ScriptList` → `LangSys` → `FeatureList` → `LookupList` → Lookup navigation. This
//! module is that shared substrate: the [`Coverage`] / [`ClassDef`] common
//! tables plus the feature-tag → lookup resolution, lifted from `tables/gpos/`
//! at R50.7 when GSUB became the second consumer (abstraction follows the
//! second consumer, not speculation).
//!
//! The lookup *type*-specific subtable parsing stays in each table's module
//! (GPOS `PairPos`, GSUB `LigatureSubst`); this module stops at the generic
//! "give me a feature's lookups, and each lookup's (type, flag, subtable
//! offsets)" boundary. Every function takes the owning table's sfnt `tag` so
//! diagnostics name GPOS vs GSUB.
//!
//! Script segmentation (§5.37.5) is deferred, so [`feature_lookup_indices`]
//! gathers the target feature from **every** script's default `LangSys`; callers
//! Coverage-gate every subtable, making the cross-script union a no-op for
//! non-matching glyphs.

pub mod classdef;
pub mod coverage;

pub use classdef::ClassDef;
pub use coverage::Coverage;

use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

/// `LangSys` `requiredFeatureIndex` sentinel for "no required feature".
const NO_REQUIRED_FEATURE: u16 = 0xFFFF;

/// Bounds-checked sub-slice from an absolute offset into a layout table.
pub fn slice_at(bytes: &[u8], off: usize, tag: [u8; 4]) -> Result<&[u8], ParseError> {
    bytes.get(off..).ok_or(ParseError::TableTooShort {
        tag,
        needed: off,
        available: bytes.len(),
    })
}

/// Resolve the `LookupList` indices reachable from `target_feature`, de-duped in
/// first-seen order. A `LangSys` feature index past the `FeatureList`, or an
/// absent list, is skipped rather than fatal — best-effort feature discovery
/// never rejects an otherwise-usable font.
pub fn feature_lookup_indices(
    table: &[u8],
    script_list_off: usize,
    feature_list_off: usize,
    target_feature: [u8; 4],
    tag: [u8; 4],
) -> Result<Vec<u16>, ParseError> {
    if script_list_off == 0 || feature_list_off == 0 {
        return Ok(Vec::new());
    }
    let features = parse_feature_records(table, feature_list_off, tag)?;
    let feature_indices = collect_default_feature_indices(table, script_list_off, tag)?;

    let mut lookups: Vec<u16> = Vec::new();
    for fi in feature_indices {
        let Some((feat_tag, feature_off)) = features.get(usize::from(fi)) else {
            continue;
        };
        if *feat_tag != target_feature {
            continue;
        }
        for l in parse_feature_lookup_indices(table, *feature_off, tag)? {
            if !lookups.contains(&l) {
                lookups.push(l);
            }
        }
    }
    Ok(lookups)
}

/// `LookupList` → absolute offset of each Lookup table (by index).
pub fn lookup_list_offsets(
    table: &[u8],
    lookup_list_off: usize,
    tag: [u8; 4],
) -> Result<Vec<usize>, ParseError> {
    let list = slice_at(table, lookup_list_off, tag)?;
    let mut r = Reader::new(list, tag);
    let lookup_count = r.read_u16()?;
    let mut out = Vec::with_capacity(usize::from(lookup_count));
    for _ in 0..lookup_count {
        out.push(lookup_list_off + usize::from(r.read_u16()?)); // rel to LookupList start
    }
    Ok(out)
}

/// Read a Lookup table header → `(lookupType, lookupFlag, absolute subtable
/// offsets)`. The caller dispatches on `lookupType` (and unwraps extension
/// subtables via [`resolve_extension`]) to its own type-specific parser.
pub fn read_lookup(
    table: &[u8],
    lookup_off: usize,
    tag: [u8; 4],
) -> Result<(u16, u16, Vec<usize>), ParseError> {
    let l = slice_at(table, lookup_off, tag)?;
    let mut r = Reader::new(l, tag);
    let lookup_type = r.read_u16()?;
    let lookup_flag = r.read_u16()?;
    let subtable_count = r.read_u16()?;
    let mut subtable_offsets = Vec::with_capacity(usize::from(subtable_count));
    for _ in 0..subtable_count {
        subtable_offsets.push(lookup_off + usize::from(r.read_u16()?)); // rel to Lookup start
    }
    Ok((lookup_type, lookup_flag, subtable_offsets))
}

/// Unwrap an Extension subtable (GPOS Lookup Type 9 / GSUB Lookup Type 7) to
/// `(realLookupType, realSubtableOffset)`. Both have the same shape: posFormat
/// (= 1), extensionLookupType (u16), extensionOffset (Offset32 from the
/// extension subtable start). A nested extension (real type = the extension
/// type) is returned as-is and will not match a concrete subtable type, so the
/// caller skips it.
pub fn resolve_extension(
    table: &[u8],
    subtable_off: usize,
    tag: [u8; 4],
) -> Result<(u16, usize), ParseError> {
    let e = slice_at(table, subtable_off, tag)?;
    let mut r = Reader::new(e, tag);
    let format = r.read_u16()?;
    if format != 1 {
        return Err(ParseError::InvalidTableField {
            tag,
            field: "extension/format",
            value: FieldValue::from_u16(format),
        });
    }
    let extension_lookup_type = r.read_u16()?;
    let extension_offset = r.read_u32()?; // relative to the extension subtable start
    let extension_offset =
        usize::try_from(extension_offset).map_err(|_| ParseError::TableTooShort {
            tag,
            needed: usize::MAX,
            available: table.len(),
        })?;
    // checked_add: a u32 extension offset could overflow usize on a 32-bit
    // target; fail loud here rather than wrap into a wrong-but-in-bounds offset.
    let real_off = subtable_off
        .checked_add(extension_offset)
        .ok_or(ParseError::TableTooShort {
            tag,
            needed: usize::MAX,
            available: table.len(),
        })?;
    Ok((extension_lookup_type, real_off))
}

/// `FeatureList` → `(featureTag, absolute feature-table offset)` per record.
fn parse_feature_records(
    table: &[u8],
    feature_list_off: usize,
    tag: [u8; 4],
) -> Result<Vec<([u8; 4], usize)>, ParseError> {
    let list = slice_at(table, feature_list_off, tag)?;
    let mut r = Reader::new(list, tag);
    let feature_count = r.read_u16()?;
    let mut out = Vec::with_capacity(usize::from(feature_count));
    for _ in 0..feature_count {
        let feat_tag = r.read_bytes::<4>()?;
        let feature_off = usize::from(r.read_u16()?); // relative to FeatureList start
        out.push((feat_tag, feature_list_off + feature_off));
    }
    Ok(out)
}

/// One Feature table → its `LookupList` indices.
fn parse_feature_lookup_indices(
    table: &[u8],
    feature_off: usize,
    tag: [u8; 4],
) -> Result<Vec<u16>, ParseError> {
    let f = slice_at(table, feature_off, tag)?;
    let mut r = Reader::new(f, tag);
    let _feature_params = r.read_u16()?;
    let lookup_index_count = r.read_u16()?;
    let mut out = Vec::with_capacity(usize::from(lookup_index_count));
    for _ in 0..lookup_index_count {
        out.push(r.read_u16()?);
    }
    Ok(out)
}

/// Union of feature indices from every script's default `LangSys` (+ its
/// required feature). Script segmentation (§5.37.5) deferred, so all scripts'
/// default features are gathered; Coverage-gating makes the cross-script union
/// safe at the call site.
fn collect_default_feature_indices(
    table: &[u8],
    script_list_off: usize,
    tag: [u8; 4],
) -> Result<Vec<u16>, ParseError> {
    let list = slice_at(table, script_list_off, tag)?;
    let mut r = Reader::new(list, tag);
    let script_count = r.read_u16()?;
    let mut script_offsets = Vec::with_capacity(usize::from(script_count));
    for _ in 0..script_count {
        let _script_tag = r.read_bytes::<4>()?;
        let script_off = usize::from(r.read_u16()?); // relative to ScriptList start
        script_offsets.push(script_list_off + script_off);
    }

    let mut indices: Vec<u16> = Vec::new();
    for script_abs in script_offsets {
        let s = slice_at(table, script_abs, tag)?;
        let mut sr = Reader::new(s, tag);
        let default_langsys_off = usize::from(sr.read_u16()?); // 0 = no default LangSys
        if default_langsys_off != 0 {
            collect_langsys_features(table, script_abs + default_langsys_off, &mut indices, tag)?;
        }
    }
    Ok(indices)
}

/// Append a `LangSys`'s required + listed feature indices (de-duped) to `indices`.
fn collect_langsys_features(
    table: &[u8],
    langsys_off: usize,
    indices: &mut Vec<u16>,
    tag: [u8; 4],
) -> Result<(), ParseError> {
    let ls = slice_at(table, langsys_off, tag)?;
    let mut r = Reader::new(ls, tag);
    let _lookup_order = r.read_u16()?; // reserved (= NULL)
    let required_feature_index = r.read_u16()?;
    let feature_index_count = r.read_u16()?;
    if required_feature_index != NO_REQUIRED_FEATURE && !indices.contains(&required_feature_index) {
        indices.push(required_feature_index);
    }
    for _ in 0..feature_index_count {
        let fi = r.read_u16()?;
        if !indices.contains(&fi) {
            indices.push(fi);
        }
    }
    Ok(())
}
