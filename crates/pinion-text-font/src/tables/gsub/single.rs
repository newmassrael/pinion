//! R50.7.2 §5.37.6 — GSUB `SingleSubst` subtable (Lookup Type 1).
//!
//! Microsoft OpenType 1.9.x spec, "Single Substitution Subtable". The simplest
//! GSUB rewrite: each covered glyph maps one-to-one to another. Two formats share
//! a Coverage of the input glyphs and differ only in how the output is derived —
//! format 1 adds a single signed delta to every covered glyph id, format 2 lists
//! an explicit substitute per coverage index. Reached here through the `ccmp`
//! feature (combining-mark composition / decomposition's single-glyph swaps, e.g.
//! `i` → dotless-`i` before a mark); the same subtable serves any feature that
//! uses Type 1 (`locl`, `smcp`, …), which is why the feature gate lives in
//! [`super::Gsub`], not here.
//!
//! Offsets inside the subtable are relative to the subtable start, so the parser
//! is handed the whole GSUB table plus the subtable's absolute offset.

use super::GSUB_TAG;
use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;
use crate::tables::layout::{Coverage, slice_at};

/// A parsed `SingleSubst` subtable.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SingleSubst {
    /// Format 1 — every covered glyph maps to `glyph + delta` (mod 2^16).
    Delta {
        /// The input glyphs this subtable rewrites.
        coverage: Coverage,
        /// Signed delta added to a covered glyph id (wrapping at 2^16, per spec).
        delta: i16,
    },
    /// Format 2 — the glyph at coverage index `i` maps to `substitutes[i]`.
    List {
        /// The input glyphs this subtable rewrites.
        coverage: Coverage,
        /// Parallel to the Coverage: the replacement for each covered glyph.
        substitutes: Vec<u16>,
    },
}

impl SingleSubst {
    /// Parse a `SingleSubst` subtable. `table` = full GSUB table bytes;
    /// `subtable_off` = the subtable's offset from the GSUB table start.
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — an offset / record runs past the table.
    /// * [`ParseError::InvalidTableField`] — unknown substFormat or a malformed
    ///   Coverage.
    pub fn parse(table: &[u8], subtable_off: usize) -> Result<Self, ParseError> {
        let local = slice_at(table, subtable_off, GSUB_TAG)?;
        let mut r = Reader::new(local, GSUB_TAG);
        let format = r.read_u16()?;
        let coverage_off = usize::from(r.read_u16()?);
        match format {
            1 => {
                let delta = r.read_i16()?;
                let coverage = Coverage::parse(slice_at(local, coverage_off, GSUB_TAG)?, GSUB_TAG)?;
                Ok(Self::Delta { coverage, delta })
            }
            2 => {
                let glyph_count = r.read_u16()?;
                let mut substitutes = Vec::with_capacity(usize::from(glyph_count));
                for _ in 0..glyph_count {
                    substitutes.push(r.read_u16()?);
                }
                let coverage = Coverage::parse(slice_at(local, coverage_off, GSUB_TAG)?, GSUB_TAG)?;
                Ok(Self::List {
                    coverage,
                    substitutes,
                })
            }
            _ => Err(ParseError::InvalidTableField {
                tag: GSUB_TAG,
                field: "singlesubst/substFormat",
                value: FieldValue::from_u16(format),
            }),
        }
    }

    /// The substitute glyph for `glyph`, or `None` when `glyph` is not covered (so
    /// the caller leaves it unchanged / tries the next subtable).
    #[must_use]
    pub(super) fn substitute(&self, glyph: u16) -> Option<u16> {
        match self {
            Self::Delta { coverage, delta } => {
                coverage.index(glyph)?;
                Some(glyph.wrapping_add_signed(*delta))
            }
            Self::List {
                coverage,
                substitutes,
            } => {
                let ci = usize::from(coverage.index(glyph)?);
                // A Coverage index past the substitute array is a malformed font;
                // leave the glyph unchanged rather than panic.
                substitutes.get(ci).copied()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SingleSubst;
    use crate::error::ParseError;

    /// A format-1 single-glyph Coverage, 6 bytes.
    fn coverage1(glyph: u16) -> Vec<u8> {
        let mut b = 1u16.to_be_bytes().to_vec(); // coverageFormat
        b.extend_from_slice(&1u16.to_be_bytes()); // glyphCount
        b.extend_from_slice(&glyph.to_be_bytes());
        b
    }

    /// Format 1: substFormat(1) + coverageOffset(6) + deltaGlyphID, Coverage at +6.
    fn format1(glyph: u16, delta: i16) -> Vec<u8> {
        let mut b = 1u16.to_be_bytes().to_vec(); // substFormat
        b.extend_from_slice(&6u16.to_be_bytes()); // coverageOffset (after 6-byte header)
        b.extend_from_slice(&delta.to_be_bytes()); // deltaGlyphID
        b.extend_from_slice(&coverage1(glyph));
        b
    }

    /// Format 2: substFormat(2) + coverageOffset(2) + glyphCount(2) +
    /// substituteGlyphIDs[1](2) = 8-byte header, then the Coverage.
    fn format2(glyph: u16, substitute: u16) -> Vec<u8> {
        let mut b = 2u16.to_be_bytes().to_vec(); // substFormat
        b.extend_from_slice(&8u16.to_be_bytes()); // coverageOffset (after 8-byte header)
        b.extend_from_slice(&1u16.to_be_bytes()); // glyphCount
        b.extend_from_slice(&substitute.to_be_bytes()); // substituteGlyphIDs[0]
        b.extend_from_slice(&coverage1(glyph));
        b
    }

    #[test]
    fn format1_adds_delta_to_covered_glyphs() {
        let sub = SingleSubst::parse(&format1(10, 5), 0).unwrap();
        assert_eq!(sub.substitute(10), Some(15), "10 + delta 5");
        assert_eq!(sub.substitute(11), None, "uncovered glyph unchanged");
    }

    #[test]
    fn format1_delta_wraps() {
        // A negative delta and the 2^16 wrap are both exercised: glyph 3 with
        // delta -10 wraps to 65529.
        let sub = SingleSubst::parse(&format1(3, -10), 0).unwrap();
        assert_eq!(sub.substitute(3), Some(65529));
    }

    #[test]
    fn format2_maps_to_listed_substitute() {
        let sub = SingleSubst::parse(&format2(10, 700), 0).unwrap();
        assert_eq!(sub.substitute(10), Some(700));
        assert_eq!(sub.substitute(99), None, "uncovered glyph unchanged");
    }

    #[test]
    fn nonzero_subtable_offset() {
        let mut table = vec![0u8; 8];
        table.extend_from_slice(&format1(10, 5));
        let sub = SingleSubst::parse(&table, 8).unwrap();
        assert_eq!(sub.substitute(10), Some(15));
    }

    #[test]
    fn reject_unknown_format() {
        let mut sub = format1(10, 5);
        sub[0..2].copy_from_slice(&3u16.to_be_bytes());
        assert!(matches!(
            SingleSubst::parse(&sub, 0),
            Err(ParseError::InvalidTableField {
                field: "singlesubst/substFormat",
                ..
            })
        ));
    }
}
