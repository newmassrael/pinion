//! R50.6.1 §5.37.6 — GPOS `PairPos` subtable (Lookup Type 2, formats 1 & 2).
//!
//! Microsoft OpenType 1.9.x spec, "Pair Adjustment Positioning Subtable". This
//! is the table that carries kerning: for an ordered glyph pair it supplies a
//! `ValueRecord` adjustment to the first and/or second glyph. The baseline
//! shaper (§5.37.6) applies only the first glyph's **X advance** — horizontal
//! kerning — and consumes (but does not yet apply) the other `ValueRecord` fields
//! (placement, Y advance, second-glyph value, device tables = R50.6.x).
//!
//! Format 1 = explicit `(firstGlyph via Coverage) → PairSet → (secondGlyph →
//! value)` lists. Format 2 = `(class1 of first) × (class2 of second) → value`
//! matrix, far more compact when many pairs share a delta.
//!
//! Offsets inside a `PairPos` subtable are relative to the subtable start, so the
//! parser is handed the whole GPOS table plus the subtable's absolute offset.

use super::classdef::ClassDef;
use super::coverage::Coverage;
use super::{GPOS_TAG, slice_at};
use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

/// `ValueRecord` field-presence bits (Microsoft OpenType "`ValueRecord`").
const X_PLACEMENT: u16 = 0x0001;
const Y_PLACEMENT: u16 = 0x0002;
const X_ADVANCE: u16 = 0x0004;
const Y_ADVANCE: u16 = 0x0008;
const X_PLACEMENT_DEVICE: u16 = 0x0010;
const Y_PLACEMENT_DEVICE: u16 = 0x0020;
const X_ADVANCE_DEVICE: u16 = 0x0040;
const Y_ADVANCE_DEVICE: u16 = 0x0080;

/// One `(secondGlyph, firstGlyph-x-advance)` entry of a Format 1 `PairSet`.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) struct PairValue {
    second_glyph: u16,
    x_advance1: i16,
}

/// A parsed `PairPos` subtable.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PairPos {
    /// Format 1 — `pair_sets[coverageIndex]` lists `(secondGlyph → value)`.
    Format1 {
        coverage: Coverage,
        pair_sets: Vec<Vec<PairValue>>,
    },
    /// Format 2 — `x_advance[class1 * class2_count + class2]` value matrix,
    /// gated on the first glyph being in `coverage`.
    Format2 {
        coverage: Coverage,
        class_def1: ClassDef,
        class_def2: ClassDef,
        class1_count: u16,
        class2_count: u16,
        x_advance: Vec<i16>,
    },
}

impl PairPos {
    /// Parse a `PairPos` subtable. `table` = full GPOS table bytes; `subtable_off`
    /// = the subtable's offset from the GPOS table start.
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — an offset / record runs past the table.
    /// * [`ParseError::InvalidTableField`] — unknown posFormat or a malformed
    ///   Coverage / `ClassDef`.
    pub fn parse(table: &[u8], subtable_off: usize) -> Result<Self, ParseError> {
        let local = slice_at(table, subtable_off)?;
        let mut r = Reader::new(local, GPOS_TAG);
        let pos_format = r.read_u16()?;
        match pos_format {
            1 => Self::parse_format1(local),
            2 => Self::parse_format2(local),
            other => Err(ParseError::InvalidTableField {
                tag: GPOS_TAG,
                field: "pairpos/posFormat",
                value: FieldValue::from_u16(other),
            }),
        }
    }

    fn parse_format1(local: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(local, GPOS_TAG);
        let _pos_format = r.read_u16()?;
        let coverage_off = usize::from(r.read_u16()?);
        let value_format1 = r.read_u16()?;
        let value_format2 = r.read_u16()?;
        let pair_set_count = r.read_u16()?;
        let mut pair_set_offsets = Vec::with_capacity(usize::from(pair_set_count));
        for _ in 0..pair_set_count {
            pair_set_offsets.push(usize::from(r.read_u16()?));
        }

        let coverage = Coverage::parse(slice_at(local, coverage_off)?)?;

        let mut pair_sets = Vec::with_capacity(pair_set_offsets.len());
        for off in pair_set_offsets {
            let mut pr = Reader::new(slice_at(local, off)?, GPOS_TAG);
            let pair_value_count = pr.read_u16()?;
            // No `with_capacity(pair_value_count)` blow-up risk: count is u16.
            let mut entries = Vec::with_capacity(usize::from(pair_value_count));
            for _ in 0..pair_value_count {
                let second_glyph = pr.read_u16()?;
                let x_advance1 = read_value_x_advance(&mut pr, value_format1)?;
                skip_value_record(&mut pr, value_format2)?;
                entries.push(PairValue {
                    second_glyph,
                    x_advance1,
                });
            }
            pair_sets.push(entries);
        }
        Ok(Self::Format1 {
            coverage,
            pair_sets,
        })
    }

    fn parse_format2(local: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(local, GPOS_TAG);
        let _pos_format = r.read_u16()?;
        let coverage_off = usize::from(r.read_u16()?);
        let value_format1 = r.read_u16()?;
        let value_format2 = r.read_u16()?;
        let class_def1_off = usize::from(r.read_u16()?);
        let class_def2_off = usize::from(r.read_u16()?);
        let class1_count = r.read_u16()?;
        let class2_count = r.read_u16()?;

        let coverage = Coverage::parse(slice_at(local, coverage_off)?)?;
        let class_def1 = ClassDef::parse(slice_at(local, class_def1_off)?)?;
        let class_def2 = ClassDef::parse(slice_at(local, class_def2_off)?)?;

        // The class1×class2 matrix follows the header inline. Read with `r`
        // (already positioned past class2Count). Build with plain `push` — NOT
        // `with_capacity(class1*class2)` — so a font claiming a huge matrix
        // fails fast on the Reader's bounds check instead of pre-allocating
        // gigabytes.
        let mut x_advance = Vec::new();
        for _ in 0..class1_count {
            for _ in 0..class2_count {
                let x1 = read_value_x_advance(&mut r, value_format1)?;
                skip_value_record(&mut r, value_format2)?;
                x_advance.push(x1);
            }
        }
        Ok(Self::Format2 {
            coverage,
            class_def1,
            class_def2,
            class1_count,
            class2_count,
            x_advance,
        })
    }

    /// First-glyph X-advance adjustment for the ordered pair `(left, right)`.
    ///
    /// `None` = `left` is not in this subtable's Coverage (the caller should try
    /// the next subtable in the lookup). `Some(v)` = this subtable handles
    /// `left` (`v` may be 0 when the specific pair carries no adjustment).
    #[must_use]
    pub fn x_advance(&self, left: u16, right: u16) -> Option<i16> {
        match self {
            Self::Format1 {
                coverage,
                pair_sets,
            } => {
                let ci = coverage.index(left)?;
                let set = pair_sets.get(usize::from(ci))?;
                // PairValueRecords are sorted by secondGlyph; a linear scan is
                // fine for the small per-glyph pair counts kern tables carry.
                let v = set
                    .iter()
                    .find(|p| p.second_glyph == right)
                    .map_or(0, |p| p.x_advance1);
                Some(v)
            }
            Self::Format2 {
                coverage,
                class_def1,
                class_def2,
                class1_count,
                class2_count,
                x_advance,
            } => {
                coverage.index(left)?;
                let c1 = class_def1.class_of(left);
                let c2 = class_def2.class_of(right);
                if c1 >= *class1_count || c2 >= *class2_count {
                    return Some(0);
                }
                let idx = usize::from(c1) * usize::from(*class2_count) + usize::from(c2);
                Some(x_advance.get(idx).copied().unwrap_or(0))
            }
        }
    }
}

/// Read a `ValueRecord`, returning its X advance (0 if the field is absent).
/// Every present field is one 2-byte value, read in ascending bit order; device
/// fields are 2-byte offsets, consumed but not followed (R50.6.x).
fn read_value_x_advance(r: &mut Reader, value_format: u16) -> Result<i16, ParseError> {
    let mut x_advance = 0i16;
    if value_format & X_PLACEMENT != 0 {
        r.read_i16()?;
    }
    if value_format & Y_PLACEMENT != 0 {
        r.read_i16()?;
    }
    if value_format & X_ADVANCE != 0 {
        x_advance = r.read_i16()?;
    }
    if value_format & Y_ADVANCE != 0 {
        r.read_i16()?;
    }
    if value_format & X_PLACEMENT_DEVICE != 0 {
        r.read_u16()?;
    }
    if value_format & Y_PLACEMENT_DEVICE != 0 {
        r.read_u16()?;
    }
    if value_format & X_ADVANCE_DEVICE != 0 {
        r.read_u16()?;
    }
    if value_format & Y_ADVANCE_DEVICE != 0 {
        r.read_u16()?;
    }
    Ok(x_advance)
}

/// Consume a `ValueRecord` without extracting anything (the second glyph's value,
/// not applied by the baseline shaper).
fn skip_value_record(r: &mut Reader, value_format: u16) -> Result<(), ParseError> {
    // Field-presence bits 0x0001..=0x0080 each denote one 2-byte field; bits
    // 0x0100..=0x8000 are reserved and carry no bytes.
    let fields = (value_format & 0x00FF).count_ones() as usize;
    r.skip(fields * 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Build a PairPos format 1 with one covered first glyph and one pair.
    // Layout: header(10) + coverage + pairset, offsets from subtable start.
    fn build_format1(first: u16, second: u16, x_adv: i16) -> Vec<u8> {
        let value_format1: u16 = X_ADVANCE;
        let value_format2: u16 = 0;
        // header: posFormat, coverageOffset, vf1, vf2, pairSetCount, [offset]
        let header_len = 2 + 2 + 2 + 2 + 2 + 2; // = 12
        let coverage_off = header_len; // coverage right after header
        let coverage = {
            let mut b = 1u16.to_be_bytes().to_vec(); // format 1
            b.extend_from_slice(&1u16.to_be_bytes()); // glyphCount
            b.extend_from_slice(&first.to_be_bytes());
            b
        };
        let pair_set_off = coverage_off + coverage.len();
        let pair_set = {
            let mut b = 1u16.to_be_bytes().to_vec(); // pairValueCount
            b.extend_from_slice(&second.to_be_bytes());
            b.extend_from_slice(&x_adv.to_be_bytes()); // valueRecord1 (X_ADVANCE)
            b
        };

        let mut sub = Vec::new();
        sub.extend_from_slice(&1u16.to_be_bytes()); // posFormat
        sub.extend_from_slice(&u16::try_from(coverage_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&value_format1.to_be_bytes());
        sub.extend_from_slice(&value_format2.to_be_bytes());
        sub.extend_from_slice(&1u16.to_be_bytes()); // pairSetCount
        sub.extend_from_slice(&u16::try_from(pair_set_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&coverage);
        sub.extend_from_slice(&pair_set);
        sub
    }

    #[test]
    fn format1_pair_lookup() {
        let sub = build_format1(10, 20, -40);
        let pp = PairPos::parse(&sub, 0).unwrap();
        assert_eq!(pp.x_advance(10, 20), Some(-40), "the registered pair");
        assert_eq!(pp.x_advance(10, 99), Some(0), "covered first, unknown pair");
        assert_eq!(pp.x_advance(11, 20), None, "first glyph not covered");
    }

    #[test]
    fn format1_nonzero_subtable_offset() {
        // Place the subtable at offset 8 within the table to prove offset math.
        let sub = build_format1(10, 20, -40);
        let mut table = vec![0u8; 8];
        table.extend_from_slice(&sub);
        let pp = PairPos::parse(&table, 8).unwrap();
        assert_eq!(pp.x_advance(10, 20), Some(-40));
    }

    // Build a PairPos format 2 with classDef1/classDef2 and a value matrix.
    fn build_format2() -> Vec<u8> {
        let value_format1: u16 = X_ADVANCE;
        let value_format2: u16 = 0;
        let class1_count: u16 = 2;
        let class2_count: u16 = 2;
        // header: posFormat, covOff, vf1, vf2, cd1Off, cd2Off, c1Count, c2Count = 16 bytes
        let header_len = 16;
        // coverage: glyphs 10,11 (class1=1 region)
        let coverage = {
            let mut b = 1u16.to_be_bytes().to_vec();
            b.extend_from_slice(&2u16.to_be_bytes());
            b.extend_from_slice(&10u16.to_be_bytes());
            b.extend_from_slice(&11u16.to_be_bytes());
            b
        };
        // classDef1 format 1: start=10, classes=[1,1]
        let cd1 = {
            let mut b = 1u16.to_be_bytes().to_vec();
            b.extend_from_slice(&10u16.to_be_bytes());
            b.extend_from_slice(&2u16.to_be_bytes());
            b.extend_from_slice(&1u16.to_be_bytes());
            b.extend_from_slice(&1u16.to_be_bytes());
            b
        };
        // classDef2 format 1: start=20, classes=[1]
        let cd2 = {
            let mut b = 1u16.to_be_bytes().to_vec();
            b.extend_from_slice(&20u16.to_be_bytes());
            b.extend_from_slice(&1u16.to_be_bytes());
            b.extend_from_slice(&1u16.to_be_bytes());
            b
        };
        // The class1×class2 value matrix sits inline between the header and the
        // coverage/classDef tables: each cell is one X_ADVANCE i16 (2 bytes).
        let matrix_len = usize::from(class1_count) * usize::from(class2_count) * 2;
        let cov_off = header_len + matrix_len;
        let cd1_off = cov_off + coverage.len();
        let cd2_off = cd1_off + cd1.len();

        let mut sub = Vec::new();
        sub.extend_from_slice(&2u16.to_be_bytes()); // posFormat
        sub.extend_from_slice(&u16::try_from(cov_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&value_format1.to_be_bytes());
        sub.extend_from_slice(&value_format2.to_be_bytes());
        sub.extend_from_slice(&u16::try_from(cd1_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&u16::try_from(cd2_off).unwrap().to_be_bytes());
        sub.extend_from_slice(&class1_count.to_be_bytes());
        sub.extend_from_slice(&class2_count.to_be_bytes());
        // matrix [c1=0..2][c2=0..2], row-major. Only [1][1] is non-zero (-75).
        for c1 in 0..class1_count {
            for c2 in 0..class2_count {
                let v: i16 = if c1 == 1 && c2 == 1 { -75 } else { 0 };
                sub.extend_from_slice(&v.to_be_bytes());
            }
        }
        sub.extend_from_slice(&coverage);
        sub.extend_from_slice(&cd1);
        sub.extend_from_slice(&cd2);
        sub
    }

    #[test]
    fn format2_class_matrix_lookup() {
        let sub = build_format2();
        let pp = PairPos::parse(&sub, 0).unwrap();
        // glyph 10 → class1=1, glyph 20 → class2=1 → matrix[1][1] = -75
        assert_eq!(pp.x_advance(10, 20), Some(-75));
        // glyph 11 (covered, class1=1) with glyph 99 (class2=0) → matrix[1][0]=0
        assert_eq!(pp.x_advance(11, 99), Some(0));
        // glyph 12 not in coverage → None
        assert_eq!(pp.x_advance(12, 20), None);
    }

    #[test]
    fn reject_unknown_posformat() {
        let mut sub = build_format1(10, 20, -40);
        sub[0..2].copy_from_slice(&7u16.to_be_bytes());
        assert!(matches!(
            PairPos::parse(&sub, 0),
            Err(ParseError::InvalidTableField {
                field: "pairpos/posFormat",
                ..
            })
        ));
    }

    #[test]
    fn value_record_skips_extra_fields() {
        // valueFormat1 = X_PLACEMENT | X_ADVANCE → 2 fields; X advance is the
        // second i16. Prove the placement field is skipped, not mis-read.
        let mut r = Reader::new(&[0x00, 0x05, 0xFF, 0xD8], GPOS_TAG); // 5, -40
        let x = read_value_x_advance(&mut r, X_PLACEMENT | X_ADVANCE).unwrap();
        assert_eq!(x, -40);
    }
}
