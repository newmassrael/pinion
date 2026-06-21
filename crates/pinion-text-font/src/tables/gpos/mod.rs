//! R50.6.1 §5.37.6 — `GPOS` table (Glyph Positioning), kerning slice.
//!
//! Microsoft OpenType 1.9.x spec, "GPOS — Glyph Positioning Table". GPOS refines
//! the cmap+hmtx baseline ([`crate::shape::shape_run`]) with context-dependent
//! positioning. This slice implements the most universal case — **pair
//! kerning** via Lookup Type 2 (`PairPos`) reached through the `kern`
//! feature — the rest of GPOS (single/cursive/mark/contextual positioning) is
//! R50.6.x, mirroring the simple→composite raster split (R50.8 → R50.8.x).
//!
//! Resolution path (Script → `LangSys` → Feature → Lookup → subtable):
//!
//! * The `kern` feature's lookups are gathered from **every** script's default
//!   `LangSys` (script segmentation, §5.37.5, is deferred — a single visual run is
//!   assumed). Applying a script's kern subtables to other-script glyphs is a
//!   no-op because every subtable is Coverage-gated, so the union is safe.
//! * Extension positioning (Lookup Type 9) is unwrapped to its real subtable.
//! * `lookupFlag` glyph-filtering (`IgnoreMarks` / mark-attachment / mark-filter
//!   set) is **not** applied — the baseline kerns every adjacent glyph pair.
//!   Correct mark skipping arrives with mark positioning (R50.6.x).
//!
//! A font with no GPOS table, or a GPOS table with no `kern` feature, yields a
//! [`Gpos`] whose [`Gpos::kern_x_advance`] is always 0 — the baseline is
//! unchanged.

mod classdef;
mod coverage;
mod pairpos;

use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;
use pairpos::PairPos;

pub(super) const GPOS_TAG: [u8; 4] = *b"GPOS";
const KERN_FEATURE: [u8; 4] = *b"kern";
const EXTENSION_LOOKUP_TYPE: u16 = 9;
const PAIR_POS_LOOKUP_TYPE: u16 = 2;
const NO_REQUIRED_FEATURE: u16 = 0xFFFF;

/// Parsed GPOS kerning view — the ordered `kern`-feature lookups, each holding
/// its `PairPos` subtables. Lookups apply cumulatively; within a lookup the first
/// subtable that covers the first glyph wins (OpenType lookup semantics).
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Gpos {
    kern_lookups: Vec<KernLookup>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct KernLookup {
    subtables: Vec<PairPos>,
}

impl Gpos {
    /// Parse the GPOS table, retaining only the `kern`-feature `PairPos` lookups.
    ///
    /// # Errors
    ///
    /// * [`ParseError::UnsupportedTableVersion`] — majorVersion != 1.
    /// * [`ParseError::TableTooShort`] — header / offset / record past the table.
    /// * [`ParseError::InvalidTableField`] — malformed Coverage / `ClassDef` /
    ///   `PairPos` / extension subtable.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, GPOS_TAG);
        let major = r.read_u16()?;
        let minor = r.read_u16()?;
        if major != 1 {
            return Err(ParseError::UnsupportedTableVersion {
                tag: GPOS_TAG,
                major,
                minor,
            });
        }
        // Offsets are from the GPOS table start. v1.1 adds featureVariationsOffset
        // after these three; the kern slice never needs it, so it is not read.
        let script_list_off = usize::from(r.read_u16()?);
        let feature_list_off = usize::from(r.read_u16()?);
        let lookup_list_off = usize::from(r.read_u16()?);

        let kern_indices = kern_lookup_indices(bytes, script_list_off, feature_list_off)?;
        let kern_lookups = parse_kern_lookups(bytes, lookup_list_off, &kern_indices)?;
        Ok(Self { kern_lookups })
    }

    /// Cumulative first-glyph X-advance kern adjustment (design units) for the
    /// ordered pair `(left, right)`. 0 when no `kern` lookup covers the pair.
    #[must_use]
    pub fn kern_x_advance(&self, left: u16, right: u16) -> i16 {
        let mut total = 0i16;
        for lookup in &self.kern_lookups {
            for st in &lookup.subtables {
                if let Some(v) = st.x_advance(left, right) {
                    total = total.saturating_add(v);
                    break; // first covering subtable in this lookup wins
                }
            }
        }
        total
    }

    /// Whether any `kern`-feature `PairPos` lookup was found (test/diagnostic).
    #[must_use]
    pub fn has_kerning(&self) -> bool {
        self.kern_lookups.iter().any(|l| !l.subtables.is_empty())
    }
}

/// Bounds-checked sub-slice from an absolute offset into the GPOS table.
pub(super) fn slice_at(bytes: &[u8], off: usize) -> Result<&[u8], ParseError> {
    bytes.get(off..).ok_or(ParseError::TableTooShort {
        tag: GPOS_TAG,
        needed: off,
        available: bytes.len(),
    })
}

/// Resolve the `LookupList` indices reachable from the `kern` feature, de-duped in
/// first-seen order. A `LangSys` feature index past the `FeatureList`, or an absent
/// list, is skipped rather than fatal — best-effort kern discovery never rejects
/// an otherwise-usable font.
fn kern_lookup_indices(
    table: &[u8],
    script_list_off: usize,
    feature_list_off: usize,
) -> Result<Vec<u16>, ParseError> {
    if script_list_off == 0 || feature_list_off == 0 {
        return Ok(Vec::new());
    }
    let features = parse_feature_records(table, feature_list_off)?;
    let feature_indices = collect_default_feature_indices(table, script_list_off)?;

    let mut lookups: Vec<u16> = Vec::new();
    for fi in feature_indices {
        let Some((tag, feature_off)) = features.get(usize::from(fi)) else {
            continue;
        };
        if *tag != KERN_FEATURE {
            continue;
        }
        for l in parse_feature_lookup_indices(table, *feature_off)? {
            if !lookups.contains(&l) {
                lookups.push(l);
            }
        }
    }
    Ok(lookups)
}

/// `FeatureList` → `(featureTag, absolute feature-table offset)` per record.
fn parse_feature_records(
    table: &[u8],
    feature_list_off: usize,
) -> Result<Vec<([u8; 4], usize)>, ParseError> {
    let list = slice_at(table, feature_list_off)?;
    let mut r = Reader::new(list, GPOS_TAG);
    let feature_count = r.read_u16()?;
    let mut out = Vec::with_capacity(usize::from(feature_count));
    for _ in 0..feature_count {
        let tag = r.read_bytes::<4>()?;
        let feature_off = usize::from(r.read_u16()?); // relative to FeatureList start
        out.push((tag, feature_list_off + feature_off));
    }
    Ok(out)
}

/// One Feature table → its `LookupList` indices.
fn parse_feature_lookup_indices(table: &[u8], feature_off: usize) -> Result<Vec<u16>, ParseError> {
    let f = slice_at(table, feature_off)?;
    let mut r = Reader::new(f, GPOS_TAG);
    let _feature_params = r.read_u16()?;
    let lookup_index_count = r.read_u16()?;
    let mut out = Vec::with_capacity(usize::from(lookup_index_count));
    for _ in 0..lookup_index_count {
        out.push(r.read_u16()?);
    }
    Ok(out)
}

/// Union of feature indices from every script's default `LangSys` (+ its required
/// feature). Script segmentation (§5.37.5) is deferred, so all scripts' default
/// kern features are gathered; Coverage-gating makes cross-script union safe.
fn collect_default_feature_indices(
    table: &[u8],
    script_list_off: usize,
) -> Result<Vec<u16>, ParseError> {
    let list = slice_at(table, script_list_off)?;
    let mut r = Reader::new(list, GPOS_TAG);
    let script_count = r.read_u16()?;
    let mut script_offsets = Vec::with_capacity(usize::from(script_count));
    for _ in 0..script_count {
        let _script_tag = r.read_bytes::<4>()?;
        let script_off = usize::from(r.read_u16()?); // relative to ScriptList start
        script_offsets.push(script_list_off + script_off);
    }

    let mut indices: Vec<u16> = Vec::new();
    for script_abs in script_offsets {
        let s = slice_at(table, script_abs)?;
        let mut sr = Reader::new(s, GPOS_TAG);
        let default_langsys_off = usize::from(sr.read_u16()?); // 0 = no default LangSys
        if default_langsys_off != 0 {
            collect_langsys_features(table, script_abs + default_langsys_off, &mut indices)?;
        }
    }
    Ok(indices)
}

/// Append a `LangSys`'s required + listed feature indices (de-duped) to `indices`.
fn collect_langsys_features(
    table: &[u8],
    langsys_off: usize,
    indices: &mut Vec<u16>,
) -> Result<(), ParseError> {
    let ls = slice_at(table, langsys_off)?;
    let mut r = Reader::new(ls, GPOS_TAG);
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

/// Parse the `PairPos` subtables of the given `LookupList` indices. A lookup index
/// past the `LookupList` is skipped (best-effort). Non-PairPos lookup types within
/// the kern feature are skipped (deferred).
fn parse_kern_lookups(
    table: &[u8],
    lookup_list_off: usize,
    kern_indices: &[u16],
) -> Result<Vec<KernLookup>, ParseError> {
    if lookup_list_off == 0 || kern_indices.is_empty() {
        return Ok(Vec::new());
    }
    let list = slice_at(table, lookup_list_off)?;
    let mut r = Reader::new(list, GPOS_TAG);
    let lookup_count = r.read_u16()?;
    let mut lookup_offsets = Vec::with_capacity(usize::from(lookup_count));
    for _ in 0..lookup_count {
        lookup_offsets.push(usize::from(r.read_u16()?)); // relative to LookupList start
    }

    let mut out = Vec::new();
    for &idx in kern_indices {
        let Some(&loff) = lookup_offsets.get(usize::from(idx)) else {
            continue;
        };
        let subtables = parse_lookup_pairpos(table, lookup_list_off + loff)?;
        if !subtables.is_empty() {
            out.push(KernLookup { subtables });
        }
    }
    Ok(out)
}

/// Parse one Lookup table's `PairPos` (type 2, or extension→2) subtables.
fn parse_lookup_pairpos(table: &[u8], lookup_off: usize) -> Result<Vec<PairPos>, ParseError> {
    let l = slice_at(table, lookup_off)?;
    let mut r = Reader::new(l, GPOS_TAG);
    let lookup_type = r.read_u16()?;
    let _lookup_flag = r.read_u16()?; // glyph filtering deferred (baseline kerns all pairs)
    let subtable_count = r.read_u16()?;
    let mut subtable_offsets = Vec::with_capacity(usize::from(subtable_count));
    for _ in 0..subtable_count {
        subtable_offsets.push(usize::from(r.read_u16()?)); // relative to Lookup start
    }

    let mut subs = Vec::new();
    for soff in subtable_offsets {
        let subtable_abs = lookup_off + soff;
        let (real_type, real_abs) = if lookup_type == EXTENSION_LOOKUP_TYPE {
            resolve_extension(table, subtable_abs)?
        } else {
            (lookup_type, subtable_abs)
        };
        if real_type == PAIR_POS_LOOKUP_TYPE {
            subs.push(PairPos::parse(table, real_abs)?);
        }
        // Other positioning lookup types within a kern feature are deferred.
    }
    Ok(subs)
}

/// Unwrap an Extension Positioning subtable (Lookup Type 9) to `(realType,
/// realSubtableOffset)`. A nested extension (real type 9) is left as-is and will
/// not match `PairPos`, so it is skipped by the caller.
fn resolve_extension(table: &[u8], subtable_off: usize) -> Result<(u16, usize), ParseError> {
    let e = slice_at(table, subtable_off)?;
    let mut r = Reader::new(e, GPOS_TAG);
    let pos_format = r.read_u16()?;
    if pos_format != 1 {
        return Err(ParseError::InvalidTableField {
            tag: GPOS_TAG,
            field: "extensionpos/posFormat",
            value: FieldValue::from_u16(pos_format),
        });
    }
    let extension_lookup_type = r.read_u16()?;
    let extension_offset = r.read_u32()?; // relative to the extension subtable start
    let extension_offset =
        usize::try_from(extension_offset).map_err(|_| ParseError::TableTooShort {
            tag: GPOS_TAG,
            needed: usize::MAX,
            available: table.len(),
        })?;
    // checked_add: a u32 extension offset could overflow usize on a 32-bit
    // target; the bounds check in the caller's slice_at then catches a wrong
    // wrapped offset, but failing loud here is cleaner.
    let real_off = subtable_off
        .checked_add(extension_offset)
        .ok_or(ParseError::TableTooShort {
            tag: GPOS_TAG,
            needed: usize::MAX,
            available: table.len(),
        })?;
    Ok((extension_lookup_type, real_off))
}

#[cfg(test)]
mod tests;
