//! R50.7 §5.37.6 — GSUB `LigatureSubst` subtable (Lookup Type 4, format 1).
//!
//! Microsoft OpenType 1.9.x spec, "Ligature Substitution Subtable". A
//! many-to-one substitution: a sequence of glyphs (first glyph in Coverage,
//! the rest listed as components) is replaced by a single ligature glyph —
//! `f` + `i` → `ﬁ`. This is the iconic default-on (`liga`) shaping refinement,
//! the substitution counterpart to GPOS pair positioning.
//!
//! Offsets inside the subtable are relative to the subtable start, so the
//! parser takes the whole GSUB table plus the subtable's absolute offset.

use super::GSUB_TAG;
use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;
use crate::tables::layout::{Coverage, slice_at};

/// One ligature rule: the glyphs `[covered, components..]` collapse to
/// `ligature_glyph`. `components` holds the 2nd..last component glyph ids (the
/// first component is the Coverage-covered glyph, not repeated here).
#[derive(Debug, PartialEq, Eq, Clone)]
struct Ligature {
    ligature_glyph: u16,
    components: Vec<u16>,
}

/// A parsed `LigatureSubst` (format 1): a Coverage of first glyphs, and per
/// covered glyph a `LigatureSet` (the ligatures starting with that glyph,
/// longest-match-first per spec authoring convention).
#[derive(Debug, PartialEq, Eq, Clone)]
pub(super) struct LigatureSubst {
    coverage: Coverage,
    /// `ligature_sets[coverageIndex]` = the ligatures whose first glyph is the
    /// glyph at that coverage index.
    ligature_sets: Vec<Vec<Ligature>>,
}

impl LigatureSubst {
    /// Parse a `LigatureSubst` subtable. `table` = full GSUB table bytes;
    /// `subtable_off` = the subtable's offset from the GSUB table start.
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — an offset / record runs past the table.
    /// * [`ParseError::InvalidTableField`] — unknown substFormat or malformed
    ///   Coverage.
    pub(super) fn parse(table: &[u8], subtable_off: usize) -> Result<Self, ParseError> {
        let local = slice_at(table, subtable_off, GSUB_TAG)?;
        let mut r = Reader::new(local, GSUB_TAG);
        let subst_format = r.read_u16()?;
        if subst_format != 1 {
            return Err(ParseError::InvalidTableField {
                tag: GSUB_TAG,
                field: "ligaturesubst/substFormat",
                value: FieldValue::from_u16(subst_format),
            });
        }
        let coverage_off = usize::from(r.read_u16()?);
        let ligature_set_count = r.read_u16()?;
        let mut set_offsets = Vec::with_capacity(usize::from(ligature_set_count));
        for _ in 0..ligature_set_count {
            set_offsets.push(usize::from(r.read_u16()?)); // rel to subtable start
        }

        let coverage = Coverage::parse(slice_at(local, coverage_off, GSUB_TAG)?, GSUB_TAG)?;

        let mut ligature_sets = Vec::with_capacity(set_offsets.len());
        for set_off in set_offsets {
            ligature_sets.push(parse_ligature_set(local, set_off)?);
        }
        Ok(Self {
            coverage,
            ligature_sets,
        })
    }

    /// If a ligature starts at `glyphs[i]`, return `(ligatureGlyph, glyphs
    /// consumed)`. `None` = no ligature here (glyph not covered, or no rule's
    /// components match the following glyphs). The first matching rule in the
    /// set wins (fonts author longest ligatures first).
    pub(super) fn match_at(&self, glyphs: &[u16], i: usize) -> Option<(u16, usize)> {
        let ci = self.coverage.index(glyphs[i])?;
        let set = self.ligature_sets.get(usize::from(ci))?;
        for lig in set {
            let comp_count = lig.components.len() + 1; // + the covered first glyph
            if i + comp_count > glyphs.len() {
                continue;
            }
            if lig
                .components
                .iter()
                .enumerate()
                .all(|(j, &c)| glyphs[i + 1 + j] == c)
            {
                return Some((lig.ligature_glyph, comp_count));
            }
        }
        None
    }
}

/// Parse one `LigatureSet` (the ligatures sharing a first glyph) from its
/// offset within the subtable.
fn parse_ligature_set(local: &[u8], set_off: usize) -> Result<Vec<Ligature>, ParseError> {
    let set_local = slice_at(local, set_off, GSUB_TAG)?;
    let mut sr = Reader::new(set_local, GSUB_TAG);
    let ligature_count = sr.read_u16()?;
    let mut lig_offsets = Vec::with_capacity(usize::from(ligature_count));
    for _ in 0..ligature_count {
        lig_offsets.push(usize::from(sr.read_u16()?)); // rel to LigatureSet start
    }

    let mut ligatures = Vec::with_capacity(lig_offsets.len());
    for lig_off in lig_offsets {
        let lig_local = slice_at(set_local, lig_off, GSUB_TAG)?;
        let mut lr = Reader::new(lig_local, GSUB_TAG);
        let ligature_glyph = lr.read_u16()?;
        let component_count = lr.read_u16()?;
        // componentCount counts the first (covered) glyph too; the on-disk array
        // holds the remaining componentCount-1. componentCount == 0 is malformed
        // (a ligature has at least its first component) — skip it rather than
        // store a surprise 1-glyph substitution that would replace any covered
        // glyph with `ligature_glyph`.
        if component_count == 0 {
            continue;
        }
        let mut components = Vec::with_capacity(usize::from(component_count - 1));
        for _ in 1..component_count {
            components.push(lr.read_u16()?);
        }
        ligatures.push(Ligature {
            ligature_glyph,
            components,
        });
    }
    Ok(ligatures)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Build a LigatureSubst format 1: first glyph `first` + `components` →
    // `lig_glyph`. One ligature set with one ligature.
    fn build(first: u16, components: &[u16], lig_glyph: u16) -> Vec<u8> {
        // header: substFormat, coverageOffset, ligatureSetCount, setOffset[0]
        let header_len = 2 + 2 + 2 + 2; // = 8
        let coverage = {
            let mut b = 1u16.to_be_bytes().to_vec(); // format 1
            b.extend_from_slice(&1u16.to_be_bytes()); // glyphCount
            b.extend_from_slice(&first.to_be_bytes());
            b
        };
        let cov_off = header_len;
        let set_off = cov_off + coverage.len();
        // LigatureSet: ligatureCount, ligatureOffset[0], then the Ligature.
        let lig_set = {
            let mut b = 1u16.to_be_bytes().to_vec(); // ligatureCount
            b.extend_from_slice(&4u16.to_be_bytes()); // ligatureOffset[0] (after 4-byte header)
            // Ligature: ligatureGlyph, componentCount, componentGlyphIDs[count-1]
            b.extend_from_slice(&lig_glyph.to_be_bytes());
            let component_count = u16::try_from(components.len() + 1).unwrap();
            b.extend_from_slice(&component_count.to_be_bytes());
            for &c in components {
                b.extend_from_slice(&c.to_be_bytes());
            }
            b
        };

        let mut sub = Vec::new();
        sub.extend_from_slice(&1u16.to_be_bytes()); // substFormat
        sub.extend_from_slice(&u16::try_from(cov_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&1u16.to_be_bytes()); // ligatureSetCount
        sub.extend_from_slice(&u16::try_from(set_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&coverage);
        sub.extend_from_slice(&lig_set);
        sub
    }

    #[test]
    fn matches_full_ligature_sequence() {
        // 'f'(10) + 'f'(10) + 'i'(11) → ffi(700).
        let sub = build(10, &[10, 11], 700);
        let lsub = LigatureSubst::parse(&sub, 0).unwrap();
        assert_eq!(
            lsub.match_at(&[10, 10, 11], 0),
            Some((700, 3)),
            "full match"
        );
        assert_eq!(lsub.match_at(&[10, 11, 99], 0), None, "components differ");
        assert_eq!(lsub.match_at(&[10, 10], 0), None, "sequence too short");
        assert_eq!(lsub.match_at(&[99, 10, 11], 0), None, "first not covered");
    }

    #[test]
    fn nonzero_subtable_offset() {
        let sub = build(10, &[11], 500);
        let mut table = vec![0u8; 6];
        table.extend_from_slice(&sub);
        let lsub = LigatureSubst::parse(&table, 6).unwrap();
        assert_eq!(lsub.match_at(&[10, 11], 0), Some((500, 2)));
    }

    #[test]
    fn reject_unknown_subst_format() {
        let mut sub = build(10, &[11], 500);
        sub[0..2].copy_from_slice(&3u16.to_be_bytes());
        assert!(matches!(
            LigatureSubst::parse(&sub, 0),
            Err(ParseError::InvalidTableField {
                field: "ligaturesubst/substFormat",
                ..
            })
        ));
    }
}
