//! R50.6.1 §5.37.6 — `GPOS` table (Glyph Positioning), kerning slice.
//!
//! Microsoft OpenType 1.9.x spec, "GPOS — Glyph Positioning Table". GPOS refines
//! the cmap+hmtx baseline ([`crate::shape::shape_run`]) with context-dependent
//! positioning. This slice implements the most universal case — **pair
//! kerning** via Lookup Type 2 (`PairPos`) reached through the `kern`
//! feature — the rest of GPOS (single/cursive/mark/contextual positioning) is
//! R50.6.x, mirroring the simple→composite raster split (R50.8 → R50.8.x).
//!
//! The `ScriptList` → Feature → `LookupList` navigation and the Coverage / `ClassDef`
//! common tables live in `crate::tables::layout` (shared with GSUB since
//! R50.7); this module keeps only the GPOS-specific `kern` resolution and
//! `PairPos` parsing. `lookupFlag` glyph-filtering (`IgnoreMarks` / mark-attach /
//! mark-filter set) is **not** applied — the baseline kerns every adjacent
//! glyph pair; correct mark skipping arrives with mark positioning (R50.6.x).
//!
//! A font with no GPOS table, or a GPOS table with no `kern` feature, yields a
//! [`Gpos`] whose [`Gpos::kern_x_advance`] is always 0 — the baseline is
//! unchanged.

mod pairpos;

use crate::error::ParseError;
use crate::reader::Reader;
use crate::tables::layout;
use pairpos::PairPos;

pub(super) const GPOS_TAG: [u8; 4] = *b"GPOS";
const KERN_FEATURE: [u8; 4] = *b"kern";
const EXTENSION_LOOKUP_TYPE: u16 = 9;
const PAIR_POS_LOOKUP_TYPE: u16 = 2;

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

        let kern_indices = layout::feature_lookup_indices(
            bytes,
            script_list_off,
            feature_list_off,
            KERN_FEATURE,
            GPOS_TAG,
        )?;
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
        let subtables = parse_lookup_pairpos(table, lookup_off)?;
        if !subtables.is_empty() {
            out.push(KernLookup { subtables });
        }
    }
    Ok(out)
}

/// Parse one Lookup table's `PairPos` (type 2, or extension→2) subtables.
fn parse_lookup_pairpos(table: &[u8], lookup_off: usize) -> Result<Vec<PairPos>, ParseError> {
    let (lookup_type, _lookup_flag, subtable_offsets) =
        layout::read_lookup(table, lookup_off, GPOS_TAG)?;
    let mut subs = Vec::new();
    for soff in subtable_offsets {
        let (real_type, real_abs) = if lookup_type == EXTENSION_LOOKUP_TYPE {
            layout::resolve_extension(table, soff, GPOS_TAG)?
        } else {
            (lookup_type, soff)
        };
        if real_type == PAIR_POS_LOOKUP_TYPE {
            subs.push(PairPos::parse(table, real_abs)?);
        }
        // Other positioning lookup types within a kern feature are deferred.
    }
    Ok(subs)
}

#[cfg(test)]
mod tests;
