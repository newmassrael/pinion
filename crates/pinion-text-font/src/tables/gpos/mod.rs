//! R50.6.1 / R50.6.2 / R50.6.3 §5.37.6 — `GPOS` table (Glyph Positioning): pair
//! kerning, mark-to-base, and mark-to-mark attachment.
//!
//! Microsoft OpenType 1.9.x spec, "GPOS — Glyph Positioning Table". GPOS refines
//! the cmap+hmtx baseline ([`crate::shape::shape_run`]) with context-dependent
//! positioning. This module implements the three most universal cases — **pair
//! kerning** via Lookup Type 2 (`PairPos`) through the `kern` feature,
//! **mark-to-base attachment** via Lookup Type 4 (`MarkBasePos`) through the
//! `mark` feature, and **mark-to-mark stacking** via Lookup Type 6
//! (`MarkMarkPos`) through the `mkmk` feature — the rest of GPOS (single /
//! cursive / mark-to-ligature / contextual positioning) is R50.6.x, mirroring
//! the simple→composite raster split (R50.8 → R50.8.x). The two mark lookup
//! types share one subtable parser ([`markanchor::MarkAnchorPos`]).
//!
//! The `ScriptList` → Feature → `LookupList` navigation and the Coverage / `ClassDef`
//! common tables live in `crate::tables::layout` (shared with GSUB since
//! R50.7); this module keeps only the GPOS-specific feature resolution and the
//! `PairPos` / `MarkAnchorPos` parsing. `lookupFlag` glyph-filtering
//! (`IgnoreMarks` / mark-attach class / mark-filter set) is **not** applied —
//! kerning sees every adjacent pair, a mark attaches to its
//! immediately-preceding base, and a stacking mark to the preceding mark;
//! GDEF-driven mark skipping is R50.6.x.
//!
//! A font with no GPOS table, or a GPOS table with no `kern` feature, yields a
//! [`Gpos`] whose [`Gpos::kern_x_advance`] is always 0 — the baseline is
//! unchanged.

mod markanchor;
mod pairpos;

use crate::error::ParseError;
use crate::reader::Reader;
use crate::tables::layout;
use markanchor::MarkAnchorPos;
use pairpos::PairPos;

pub(super) const GPOS_TAG: [u8; 4] = *b"GPOS";
const KERN_FEATURE: [u8; 4] = *b"kern";
const MARK_FEATURE: [u8; 4] = *b"mark";
const MKMK_FEATURE: [u8; 4] = *b"mkmk";
const EXTENSION_LOOKUP_TYPE: u16 = 9;
const PAIR_POS_LOOKUP_TYPE: u16 = 2;
const MARK_BASE_POS_LOOKUP_TYPE: u16 = 4;
const MARK_MARK_POS_LOOKUP_TYPE: u16 = 6;

/// Parsed GPOS positioning view — the ordered `kern`-feature `PairPos` lookups,
/// `mark`-feature mark-to-base lookups, and `mkmk`-feature mark-to-mark lookups.
/// Lookups apply cumulatively; within a lookup the first subtable that covers the
/// relevant glyph wins (OpenType lookup semantics).
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Gpos {
    /// `kern`-feature pair-positioning lookups.
    kern: Vec<KernLookup>,
    /// `mark`-feature mark-to-base lookups.
    mark: Vec<MarkLookup>,
    /// `mkmk`-feature mark-to-mark lookups.
    mkmk: Vec<MarkLookup>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct KernLookup {
    subtables: Vec<PairPos>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct MarkLookup {
    subtables: Vec<MarkAnchorPos>,
}

impl Gpos {
    /// Parse the GPOS table, retaining the `kern`-feature `PairPos` lookups, the
    /// `mark`-feature mark-to-base lookups, and the `mkmk`-feature mark-to-mark
    /// lookups.
    ///
    /// # Errors
    ///
    /// * [`ParseError::UnsupportedTableVersion`] — majorVersion != 1.
    /// * [`ParseError::TableTooShort`] — header / offset / record past the table.
    /// * [`ParseError::InvalidTableField`] — malformed Coverage / `ClassDef` /
    ///   `PairPos` / `MarkAnchorPos` / anchor / extension subtable.
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
        let kern = parse_kern_lookups(bytes, lookup_list_off, &kern_indices)?;
        let mark_indices = layout::feature_lookup_indices(
            bytes,
            script_list_off,
            feature_list_off,
            MARK_FEATURE,
            GPOS_TAG,
        )?;
        let mark = parse_mark_attach_lookups(
            bytes,
            lookup_list_off,
            &mark_indices,
            MARK_BASE_POS_LOOKUP_TYPE,
        )?;
        let mkmk_indices = layout::feature_lookup_indices(
            bytes,
            script_list_off,
            feature_list_off,
            MKMK_FEATURE,
            GPOS_TAG,
        )?;
        let mkmk = parse_mark_attach_lookups(
            bytes,
            lookup_list_off,
            &mkmk_indices,
            MARK_MARK_POS_LOOKUP_TYPE,
        )?;
        Ok(Self { kern, mark, mkmk })
    }

    /// Cumulative first-glyph X-advance kern adjustment (design units) for the
    /// ordered pair `(left, right)`. 0 when no `kern` lookup covers the pair.
    #[must_use]
    pub fn kern_x_advance(&self, left: u16, right: u16) -> i16 {
        let mut total = 0i16;
        for lookup in &self.kern {
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
        self.kern.iter().any(|l| !l.subtables.is_empty())
    }

    /// Design-unit translation `(dx, dy)` (y up) placing `mark`'s anchor onto
    /// `base`'s anchor, via the GPOS `mark` feature mark-to-base lookups
    /// (§5.37.6). The first lookup/subtable that covers the pair wins; `None`
    /// when no mark-to-base lookup attaches this mark to this base.
    #[must_use]
    pub fn mark_offset(&self, base: u16, mark: u16) -> Option<(i16, i16)> {
        first_attach_offset(&self.mark, base, mark)
    }

    /// Design-unit translation `(dx, dy)` (y up) placing `mark`'s anchor onto the
    /// preceding `prev_mark`'s anchor, via the GPOS `mkmk` feature mark-to-mark
    /// lookups (§5.37.6) — how stacking diacritics pile above one another. The
    /// first lookup/subtable that covers the pair wins; `None` when no
    /// mark-to-mark lookup stacks this mark on that one.
    #[must_use]
    pub fn mark_mark_offset(&self, prev_mark: u16, mark: u16) -> Option<(i16, i16)> {
        first_attach_offset(&self.mkmk, prev_mark, mark)
    }

    /// Whether any `mark`-feature mark-to-base lookup was found (test/diagnostic).
    #[must_use]
    pub fn has_marks(&self) -> bool {
        self.mark.iter().any(|l| !l.subtables.is_empty())
    }

    /// Whether any `mkmk`-feature mark-to-mark lookup was found (test/diagnostic).
    #[must_use]
    pub fn has_mark_marks(&self) -> bool {
        self.mkmk.iter().any(|l| !l.subtables.is_empty())
    }
}

/// First covering attachment offset across an ordered lookup list — the shared
/// resolution for both mark-to-base ([`Gpos::mark_offset`]) and mark-to-mark
/// ([`Gpos::mark_mark_offset`]): try each lookup's subtables in order and return
/// the first that attaches `mark` to `attach`.
fn first_attach_offset(lookups: &[MarkLookup], attach: u16, mark: u16) -> Option<(i16, i16)> {
    for lookup in lookups {
        for st in &lookup.subtables {
            if let Some(off) = st.offset(attach, mark) {
                return Some(off);
            }
        }
    }
    None
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

/// Parse the mark-attachment subtables of the given feature's `LookupList`
/// indices, keeping only lookups of `lookup_type` — Type 4 (`MarkBasePos`, the
/// `mark` feature) or Type 6 (`MarkMarkPos`, the `mkmk` feature); both parse with
/// [`MarkAnchorPos`]. A lookup index past the `LookupList` is skipped; other
/// positioning lookup types under the feature (e.g. mark-to-ligature type 5) are
/// deferred (R50.6.x). The single SSOT for both mark passes.
fn parse_mark_attach_lookups(
    table: &[u8],
    lookup_list_off: usize,
    indices: &[u16],
    lookup_type: u16,
) -> Result<Vec<MarkLookup>, ParseError> {
    if lookup_list_off == 0 || indices.is_empty() {
        return Ok(Vec::new());
    }
    let lookup_offsets = layout::lookup_list_offsets(table, lookup_list_off, GPOS_TAG)?;
    let mut out = Vec::new();
    for &idx in indices {
        let Some(&lookup_off) = lookup_offsets.get(usize::from(idx)) else {
            continue;
        };
        let subtables = layout::collect_subtables_of_type(
            table,
            lookup_off,
            GPOS_TAG,
            EXTENSION_LOOKUP_TYPE,
            lookup_type,
            MarkAnchorPos::parse,
        )?;
        if !subtables.is_empty() {
            out.push(MarkLookup { subtables });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
