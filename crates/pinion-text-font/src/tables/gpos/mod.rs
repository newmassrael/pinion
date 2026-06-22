//! R50.6.1 / R50.6.2 §5.37.6 — `GPOS` table (Glyph Positioning): pair kerning
//! and mark-to-base attachment.
//!
//! Microsoft OpenType 1.9.x spec, "GPOS — Glyph Positioning Table". GPOS refines
//! the cmap+hmtx baseline ([`crate::shape::shape_run`]) with context-dependent
//! positioning. This module implements the two most universal cases — **pair
//! kerning** via Lookup Type 2 (`PairPos`) through the `kern` feature, and
//! **mark-to-base attachment** via Lookup Type 4 (`MarkBasePos`) through the
//! `mark` feature — the rest of GPOS (single / cursive / mark-to-ligature /
//! mark-to-mark / contextual positioning) is R50.6.x, mirroring the
//! simple→composite raster split (R50.8 → R50.8.x).
//!
//! The `ScriptList` → Feature → `LookupList` navigation and the Coverage / `ClassDef`
//! common tables live in `crate::tables::layout` (shared with GSUB since
//! R50.7); this module keeps only the GPOS-specific feature resolution and the
//! `PairPos` / `MarkBasePos` parsing. `lookupFlag` glyph-filtering
//! (`IgnoreMarks` / mark-attach class / mark-filter set) is **not** applied —
//! kerning sees every adjacent pair and a mark attaches to its
//! immediately-preceding base; correct mark skipping is R50.6.x.
//!
//! A font with no GPOS table, or a GPOS table with no `kern` feature, yields a
//! [`Gpos`] whose [`Gpos::kern_x_advance`] is always 0 — the baseline is
//! unchanged.

mod markbase;
mod pairpos;

use crate::error::ParseError;
use crate::reader::Reader;
use crate::tables::layout;
use markbase::MarkBasePos;
use pairpos::PairPos;

pub(super) const GPOS_TAG: [u8; 4] = *b"GPOS";
const KERN_FEATURE: [u8; 4] = *b"kern";
const MARK_FEATURE: [u8; 4] = *b"mark";
const EXTENSION_LOOKUP_TYPE: u16 = 9;
const PAIR_POS_LOOKUP_TYPE: u16 = 2;
const MARK_BASE_POS_LOOKUP_TYPE: u16 = 4;

/// Parsed GPOS positioning view — the ordered `kern`-feature `PairPos` lookups
/// and `mark`-feature `MarkBasePos` lookups. Lookups apply cumulatively; within a
/// lookup the first subtable that covers the relevant glyph wins (OpenType lookup
/// semantics).
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Gpos {
    kern_lookups: Vec<KernLookup>,
    mark_lookups: Vec<MarkLookup>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct KernLookup {
    subtables: Vec<PairPos>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct MarkLookup {
    subtables: Vec<MarkBasePos>,
}

impl Gpos {
    /// Parse the GPOS table, retaining the `kern`-feature `PairPos` lookups and
    /// the `mark`-feature `MarkBasePos` lookups.
    ///
    /// # Errors
    ///
    /// * [`ParseError::UnsupportedTableVersion`] — majorVersion != 1.
    /// * [`ParseError::TableTooShort`] — header / offset / record past the table.
    /// * [`ParseError::InvalidTableField`] — malformed Coverage / `ClassDef` /
    ///   `PairPos` / `MarkBasePos` / anchor / extension subtable.
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

        let kern_indices = layout::feature_lookup_indices(
            bytes,
            script_list_off,
            feature_list_off,
            KERN_FEATURE,
            GPOS_TAG,
        )?;
        let kern_lookups = parse_kern_lookups(bytes, lookup_list_off, &kern_indices)?;
        let mark_indices = layout::feature_lookup_indices(
            bytes,
            script_list_off,
            feature_list_off,
            MARK_FEATURE,
            GPOS_TAG,
        )?;
        let mark_lookups = parse_mark_lookups(bytes, lookup_list_off, &mark_indices)?;
        Ok(Self {
            kern_lookups,
            mark_lookups,
        })
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

    /// Design-unit translation `(dx, dy)` (y up) placing `mark`'s anchor onto
    /// `base`'s anchor, via the GPOS `mark` feature mark-to-base lookups
    /// (§5.37.6). The first lookup/subtable that covers the pair wins; `None`
    /// when no mark-to-base lookup attaches this mark to this base.
    #[must_use]
    pub fn mark_offset(&self, base: u16, mark: u16) -> Option<(i16, i16)> {
        for lookup in &self.mark_lookups {
            for st in &lookup.subtables {
                if let Some(off) = st.mark_offset(base, mark) {
                    return Some(off);
                }
            }
        }
        None
    }

    /// Whether any `mark`-feature mark-to-base lookup was found (test/diagnostic).
    #[must_use]
    pub fn has_marks(&self) -> bool {
        self.mark_lookups.iter().any(|l| !l.subtables.is_empty())
    }
}

/// Parse the `PairPos` subtables of the given `LookupList` indices. A lookup
/// index past the `LookupList` is skipped (best-effort). Non-PairPos lookup
/// types within the kern feature are skipped (deferred).
fn parse_kern_lookups(
    table: &[u8],
    lookup_list_off: usize,
    kern_indices: &[u16],
) -> Result<Vec<KernLookup>, ParseError> {
    if lookup_list_off == 0 || kern_indices.is_empty() {
        return Ok(Vec::new());
    }
    let lookup_offsets = layout::lookup_list_offsets(table, lookup_list_off, GPOS_TAG)?;
    let mut out = Vec::new();
    for &idx in kern_indices {
        let Some(&lookup_off) = lookup_offsets.get(usize::from(idx)) else {
            continue;
        };
        // Other positioning lookup types within a kern feature are skipped.
        let subtables = layout::collect_subtables_of_type(
            table,
            lookup_off,
            GPOS_TAG,
            EXTENSION_LOOKUP_TYPE,
            PAIR_POS_LOOKUP_TYPE,
            PairPos::parse,
        )?;
        if !subtables.is_empty() {
            out.push(KernLookup { subtables });
        }
    }
    Ok(out)
}

/// Parse the `MarkBasePos` subtables of the given `mark`-feature `LookupList`
/// indices. A lookup index past the `LookupList` is skipped; non-mark-to-base
/// lookup types under the mark feature (e.g. mark-to-ligature type 5) are
/// deferred (R50.6.x).
fn parse_mark_lookups(
    table: &[u8],
    lookup_list_off: usize,
    mark_indices: &[u16],
) -> Result<Vec<MarkLookup>, ParseError> {
    if lookup_list_off == 0 || mark_indices.is_empty() {
        return Ok(Vec::new());
    }
    let lookup_offsets = layout::lookup_list_offsets(table, lookup_list_off, GPOS_TAG)?;
    let mut out = Vec::new();
    for &idx in mark_indices {
        let Some(&lookup_off) = lookup_offsets.get(usize::from(idx)) else {
            continue;
        };
        // Mark-to-ligature (5) / mark-to-mark (6) under the mark feature are skipped.
        let subtables = layout::collect_subtables_of_type(
            table,
            lookup_off,
            GPOS_TAG,
            EXTENSION_LOOKUP_TYPE,
            MARK_BASE_POS_LOOKUP_TYPE,
            MarkBasePos::parse,
        )?;
        if !subtables.is_empty() {
            out.push(MarkLookup { subtables });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
