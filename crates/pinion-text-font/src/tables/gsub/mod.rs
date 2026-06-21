//! R50.7 §5.37.6 — `GSUB` table (Glyph Substitution), ligature slice.
//!
//! Microsoft OpenType 1.9.x spec, "GSUB — Glyph Substitution Table". GSUB runs
//! BEFORE GPOS in the shaping pipeline (cmap → GSUB → GPOS): it rewrites the
//! glyph stream — ligatures, contextual forms, composition. This slice
//! implements the most universal default-on case — **ligature substitution**
//! (Lookup Type 4, `LigatureSubst`) reached through the `liga` feature — the
//! substitution counterpart to the GPOS `kern` slice. The rest of GSUB
//! (single / multiple / alternate / contextual / reverse-chaining) is R50.7.x.
//!
//! The `ScriptList` → Feature → `LookupList` navigation and the Coverage common
//! table live in `crate::tables::layout` (shared with GPOS). `liga` lookups
//! are gathered from every script's default `LangSys` (script segmentation,
//! §5.37.5, deferred); Coverage-gating makes the cross-script union a no-op for
//! non-matching glyphs. `lookupFlag` glyph-filtering and ligature-component
//! mark attachment are deferred (R50.7.x). A font with no GSUB table, or no
//! `liga` feature, leaves the glyph stream unchanged.

mod ligature;

use crate::error::ParseError;
use crate::reader::Reader;
use crate::tables::layout;
use ligature::LigatureSubst;

pub(super) const GSUB_TAG: [u8; 4] = *b"GSUB";
const LIGA_FEATURE: [u8; 4] = *b"liga";
const EXTENSION_LOOKUP_TYPE: u16 = 7; // GSUB extension positioning is type 7 (GPOS is 9)
const LIGATURE_LOOKUP_TYPE: u16 = 4;

/// Parsed GSUB ligature view — the ordered `liga`-feature lookups, each holding
/// its `LigatureSubst` subtables. Lookups apply in sequence (the output of one
/// is the input of the next).
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Gsub {
    liga_lookups: Vec<LigaLookup>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct LigaLookup {
    subtables: Vec<LigatureSubst>,
}

impl Gsub {
    /// Parse the GSUB table, retaining only the `liga`-feature `LigatureSubst`
    /// lookups.
    ///
    /// # Errors
    ///
    /// * [`ParseError::UnsupportedTableVersion`] — majorVersion != 1.
    /// * [`ParseError::TableTooShort`] — header / offset / record past the table.
    /// * [`ParseError::InvalidTableField`] — malformed Coverage / `LigatureSubst`
    ///   / extension subtable.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, GSUB_TAG);
        let major = r.read_u16()?;
        let minor = r.read_u16()?;
        if major != 1 {
            return Err(ParseError::UnsupportedTableVersion {
                tag: GSUB_TAG,
                major,
                minor,
            });
        }
        let script_list_off = usize::from(r.read_u16()?);
        let feature_list_off = usize::from(r.read_u16()?);
        let lookup_list_off = usize::from(r.read_u16()?);

        let liga_indices = layout::feature_lookup_indices(
            bytes,
            script_list_off,
            feature_list_off,
            LIGA_FEATURE,
            GSUB_TAG,
        )?;
        let liga_lookups = parse_liga_lookups(bytes, lookup_list_off, &liga_indices)?;
        Ok(Self { liga_lookups })
    }

    /// Apply `liga` ligature substitution to a glyph-id sequence. Returns one
    /// `(glyph, origin)` per output glyph, where `origin` is the index in the
    /// input `glyphs` of the first component that produced it — so a caller can
    /// map the (possibly fewer) output glyphs back to source clusters. A
    /// non-ligated glyph maps to its own input index.
    #[must_use]
    pub fn substitute(&self, glyphs: &[u16]) -> Vec<(u16, usize)> {
        let mut glyph_ids: Vec<u16> = glyphs.to_vec();
        let mut origins: Vec<usize> = (0..glyphs.len()).collect();
        for lookup in &self.liga_lookups {
            apply_liga_lookup(&mut glyph_ids, &mut origins, &lookup.subtables);
        }
        glyph_ids.into_iter().zip(origins).collect()
    }

    /// Whether any `liga`-feature `LigatureSubst` lookup was found
    /// (test/diagnostic).
    #[must_use]
    pub fn has_ligatures(&self) -> bool {
        self.liga_lookups.iter().any(|l| !l.subtables.is_empty())
    }
}

/// Apply one lookup's ligature subtables left-to-right. At each position the
/// first subtable that forms a ligature wins; the consumed components collapse
/// to the ligature glyph (carrying the first component's origin), and scanning
/// resumes after it.
fn apply_liga_lookup(
    glyph_ids: &mut Vec<u16>,
    origins: &mut Vec<usize>,
    subtables: &[LigatureSubst],
) {
    let mut i = 0;
    while i < glyph_ids.len() {
        let mut applied = false;
        for sub in subtables {
            if let Some((lig_glyph, comp_count)) = sub.match_at(glyph_ids, i) {
                let origin = origins[i];
                glyph_ids.splice(i..i + comp_count, std::iter::once(lig_glyph));
                origins.splice(i..i + comp_count, std::iter::once(origin));
                i += 1;
                applied = true;
                break;
            }
        }
        if !applied {
            i += 1;
        }
    }
}

/// Parse the `LigatureSubst` subtables of the given `LookupList` indices. A
/// lookup index past the `LookupList` is skipped (best-effort). Non-ligature
/// lookup types within the `liga` feature are skipped (deferred).
fn parse_liga_lookups(
    table: &[u8],
    lookup_list_off: usize,
    liga_indices: &[u16],
) -> Result<Vec<LigaLookup>, ParseError> {
    if lookup_list_off == 0 || liga_indices.is_empty() {
        return Ok(Vec::new());
    }
    let lookup_offsets = layout::lookup_list_offsets(table, lookup_list_off, GSUB_TAG)?;
    let mut out = Vec::new();
    for &idx in liga_indices {
        let Some(&lookup_off) = lookup_offsets.get(usize::from(idx)) else {
            continue;
        };
        let (lookup_type, _flag, subtable_offsets) =
            layout::read_lookup(table, lookup_off, GSUB_TAG)?;
        let mut subs = Vec::new();
        for soff in subtable_offsets {
            let (real_type, real_abs) = if lookup_type == EXTENSION_LOOKUP_TYPE {
                layout::resolve_extension(table, soff, GSUB_TAG)?
            } else {
                (lookup_type, soff)
            };
            if real_type == LIGATURE_LOOKUP_TYPE {
                subs.push(LigatureSubst::parse(table, real_abs)?);
            }
            // Other substitution lookup types within liga are deferred.
        }
        if !subs.is_empty() {
            out.push(LigaLookup { subtables: subs });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
