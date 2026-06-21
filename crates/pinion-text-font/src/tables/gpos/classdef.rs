//! R50.6.1 §5.37.6 — OpenType Layout `ClassDef` table.
//!
//! Microsoft OpenType 1.9.x spec, "Class Definition Table" (Common Table
//! Formats). Maps a glyph id to a *class value*; any glyph not listed is
//! class 0 (the implicit default class). Used by GPOS `PairPos` format 2 to
//! key the first/second glyph of a pair into the class-pair value matrix.
//!
//! Format 1 = a contiguous run of glyph ids starting at `startGlyphID`, each
//! with an explicit class; Format 2 = sorted `(start, end, class)` ranges.
//!
//! OpenType-Layout common table (GSUB shares it). Lives under `gpos/` until a
//! second consumer (GSUB, R50.6.x) triggers a lift to a shared `layout/`
//! module — same discipline as [`super::coverage`].

use super::GPOS_TAG;
use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

/// A parsed `ClassDef` table — glyph id → class value (default 0).
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ClassDef {
    /// Format 1 — classes for `[start_glyph_id, start_glyph_id + classes.len())`.
    Format1 {
        start_glyph_id: u16,
        classes: Vec<u16>,
    },
    /// Format 2 — sorted `(start, end, class)` ranges.
    Format2 { ranges: Vec<ClassRange> },
}

/// One Format 2 class range record.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ClassRange {
    pub start_glyph_id: u16,
    pub end_glyph_id: u16,
    pub class: u16,
}

impl ClassDef {
    /// Parse a `ClassDef` table from its own first byte (`bytes[0]` = classFormat).
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — header / records run past the slice.
    /// * [`ParseError::InvalidTableField`] — unknown format or a range with
    ///   `start > end`.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, GPOS_TAG);
        let format = r.read_u16()?;
        match format {
            1 => {
                let start_glyph_id = r.read_u16()?;
                let glyph_count = r.read_u16()?;
                let mut classes = Vec::with_capacity(usize::from(glyph_count));
                for _ in 0..glyph_count {
                    classes.push(r.read_u16()?);
                }
                Ok(Self::Format1 {
                    start_glyph_id,
                    classes,
                })
            }
            2 => {
                let range_count = r.read_u16()?;
                let mut ranges = Vec::with_capacity(usize::from(range_count));
                for _ in 0..range_count {
                    let start_glyph_id = r.read_u16()?;
                    let end_glyph_id = r.read_u16()?;
                    let class = r.read_u16()?;
                    if start_glyph_id > end_glyph_id {
                        return Err(ParseError::InvalidTableField {
                            tag: GPOS_TAG,
                            field: "classdef2/start>end",
                            value: FieldValue::Unsigned(
                                (u64::from(start_glyph_id) << 16) | u64::from(end_glyph_id),
                            ),
                        });
                    }
                    ranges.push(ClassRange {
                        start_glyph_id,
                        end_glyph_id,
                        class,
                    });
                }
                Ok(Self::Format2 { ranges })
            }
            other => Err(ParseError::InvalidTableField {
                tag: GPOS_TAG,
                field: "classdef/format",
                value: FieldValue::from_u16(other),
            }),
        }
    }

    /// Class value of `glyph` — 0 (the default class) when the glyph is not
    /// explicitly assigned.
    #[must_use]
    pub fn class_of(&self, glyph: u16) -> u16 {
        match self {
            Self::Format1 {
                start_glyph_id,
                classes,
            } => glyph
                .checked_sub(*start_glyph_id)
                .and_then(|i| classes.get(usize::from(i)).copied())
                .unwrap_or(0),
            Self::Format2 { ranges } => ranges
                .iter()
                .find_map(|rec| {
                    (glyph >= rec.start_glyph_id && glyph <= rec.end_glyph_id).then_some(rec.class)
                })
                .unwrap_or(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt1(start: u16, classes: &[u16]) -> Vec<u8> {
        let mut b = 1u16.to_be_bytes().to_vec();
        b.extend_from_slice(&start.to_be_bytes());
        b.extend_from_slice(&u16::try_from(classes.len()).unwrap().to_be_bytes());
        for &c in classes {
            b.extend_from_slice(&c.to_be_bytes());
        }
        b
    }

    fn fmt2(ranges: &[(u16, u16, u16)]) -> Vec<u8> {
        let mut b = 2u16.to_be_bytes().to_vec();
        b.extend_from_slice(&u16::try_from(ranges.len()).unwrap().to_be_bytes());
        for &(s, e, c) in ranges {
            b.extend_from_slice(&s.to_be_bytes());
            b.extend_from_slice(&e.to_be_bytes());
            b.extend_from_slice(&c.to_be_bytes());
        }
        b
    }

    #[test]
    fn format1_contiguous_run_else_default() {
        let cd = ClassDef::parse(&fmt1(50, &[1, 2, 0, 3])).unwrap();
        assert_eq!(cd.class_of(49), 0, "below run = default class 0");
        assert_eq!(cd.class_of(50), 1);
        assert_eq!(cd.class_of(51), 2);
        assert_eq!(cd.class_of(52), 0, "explicit class 0 inside run");
        assert_eq!(cd.class_of(53), 3);
        assert_eq!(cd.class_of(54), 0, "above run = default class 0");
    }

    #[test]
    fn format1_below_start_does_not_wrap() {
        // glyph < start_glyph_id must not underflow into a bogus index.
        let cd = ClassDef::parse(&fmt1(10, &[7])).unwrap();
        assert_eq!(cd.class_of(0), 0);
        assert_eq!(cd.class_of(9), 0);
        assert_eq!(cd.class_of(10), 7);
    }

    #[test]
    fn format2_ranges_else_default() {
        let cd = ClassDef::parse(&fmt2(&[(10, 12, 4), (20, 20, 9)])).unwrap();
        assert_eq!(cd.class_of(9), 0);
        assert_eq!(cd.class_of(10), 4);
        assert_eq!(cd.class_of(12), 4);
        assert_eq!(cd.class_of(13), 0);
        assert_eq!(cd.class_of(20), 9);
        assert_eq!(cd.class_of(21), 0);
    }

    #[test]
    fn reject_format2_start_gt_end() {
        let err = ClassDef::parse(&fmt2(&[(30, 10, 1)])).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                field: "classdef2/start>end",
                ..
            }
        ));
    }

    #[test]
    fn reject_unknown_format() {
        let err = ClassDef::parse(&9u16.to_be_bytes()).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                field: "classdef/format",
                ..
            }
        ));
    }
}
