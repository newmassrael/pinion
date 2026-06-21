//! R50.6.1 §5.37.6 — OpenType Layout `Coverage` table.
//!
//! Microsoft OpenType 1.9.x spec, "Coverage Table" (Common Table Formats).
//! Maps a glyph id to a *coverage index* (its 0-based position among the
//! glyphs the parent lookup covers) — or `None` when the glyph is not covered.
//!
//! Format 1 = an explicit sorted glyph list; Format 2 = sorted, non-overlapping
//! glyph ranges each carrying the coverage index of its first glyph.
//!
//! This is an OpenType-Layout *common* table (GSUB shares it too). It lives
//! under `gpos/` while GPOS is its only consumer; a GSUB second consumer
//! (R50.6.x) is the trigger to lift it to a shared `layout/` module
//! (abstraction follows the second consumer, not speculation).

use super::GPOS_TAG;
use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

/// A parsed Coverage table — glyph id → coverage index.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Coverage {
    /// Format 1 — sorted, de-duplicated glyph id list; index = array position.
    Format1 { glyphs: Vec<u16> },
    /// Format 2 — sorted, non-overlapping `(start, end, startCoverageIndex)`
    /// ranges; index = `startCoverageIndex + (glyph - start)`.
    Format2 { ranges: Vec<CoverageRange> },
}

/// One Format 2 range record.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct CoverageRange {
    pub start_glyph_id: u16,
    pub end_glyph_id: u16,
    pub start_coverage_index: u16,
}

impl Coverage {
    /// Parse a Coverage table from its own first byte (`bytes[0]` = coverageFormat).
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — header / records run past the slice.
    /// * [`ParseError::InvalidTableField`] — unknown format, glyph list not
    ///   strictly ascending, or a range with `start > end`.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, GPOS_TAG);
        let format = r.read_u16()?;
        match format {
            1 => {
                let glyph_count = r.read_u16()?;
                let mut glyphs = Vec::with_capacity(usize::from(glyph_count));
                for _ in 0..glyph_count {
                    glyphs.push(r.read_u16()?);
                }
                // Spec: GlyphArray is sorted in increasing glyph id order — the
                // precondition for binary-search lookup. Strict reject otherwise
                // (a black-box tolerance would break introspect determinism).
                for w in glyphs.windows(2) {
                    if w[0] >= w[1] {
                        return Err(ParseError::InvalidTableField {
                            tag: GPOS_TAG,
                            field: "coverage1/glyphArray-not-ascending",
                            value: FieldValue::from_u16(w[0]),
                        });
                    }
                }
                Ok(Self::Format1 { glyphs })
            }
            2 => {
                let range_count = r.read_u16()?;
                let mut ranges = Vec::with_capacity(usize::from(range_count));
                for _ in 0..range_count {
                    let start_glyph_id = r.read_u16()?;
                    let end_glyph_id = r.read_u16()?;
                    let start_coverage_index = r.read_u16()?;
                    if start_glyph_id > end_glyph_id {
                        return Err(ParseError::InvalidTableField {
                            tag: GPOS_TAG,
                            field: "coverage2/start>end",
                            value: FieldValue::Unsigned(
                                (u64::from(start_glyph_id) << 16) | u64::from(end_glyph_id),
                            ),
                        });
                    }
                    ranges.push(CoverageRange {
                        start_glyph_id,
                        end_glyph_id,
                        start_coverage_index,
                    });
                }
                Ok(Self::Format2 { ranges })
            }
            other => Err(ParseError::InvalidTableField {
                tag: GPOS_TAG,
                field: "coverage/format",
                value: FieldValue::from_u16(other),
            }),
        }
    }

    /// Coverage index of `glyph`, or `None` if the glyph is not covered.
    #[must_use]
    pub fn index(&self, glyph: u16) -> Option<u16> {
        match self {
            Self::Format1 { glyphs } => {
                // Ascending list → binary search yields the array position,
                // which IS the coverage index by definition.
                glyphs
                    .binary_search(&glyph)
                    .ok()
                    .and_then(|i| u16::try_from(i).ok())
            }
            Self::Format2 { ranges } => {
                // Ranges are non-overlapping; a linear scan suffices (kern
                // coverage range counts are small). `start_coverage_index` is
                // authoritative — it need not equal the cumulative offset. A
                // malformed `start_coverage_index` near u16::MAX would overflow
                // the add; `checked_add` returns None (treat as not covered) so a
                // crafted font can neither panic (debug overflow-check) nor wrap
                // to a bogus index (release). A well-formed font never overflows.
                ranges.iter().find_map(|rec| {
                    if glyph >= rec.start_glyph_id && glyph <= rec.end_glyph_id {
                        rec.start_coverage_index
                            .checked_add(glyph - rec.start_glyph_id)
                    } else {
                        None
                    }
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt1(glyphs: &[u16]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&u16::try_from(glyphs.len()).unwrap().to_be_bytes());
        for &g in glyphs {
            b.extend_from_slice(&g.to_be_bytes());
        }
        b
    }

    fn fmt2(ranges: &[(u16, u16, u16)]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&2u16.to_be_bytes());
        b.extend_from_slice(&u16::try_from(ranges.len()).unwrap().to_be_bytes());
        for &(s, e, sci) in ranges {
            b.extend_from_slice(&s.to_be_bytes());
            b.extend_from_slice(&e.to_be_bytes());
            b.extend_from_slice(&sci.to_be_bytes());
        }
        b
    }

    #[test]
    fn format1_index_is_array_position() {
        let cov = Coverage::parse(&fmt1(&[3, 7, 42, 100])).unwrap();
        assert_eq!(cov.index(3), Some(0));
        assert_eq!(cov.index(7), Some(1));
        assert_eq!(cov.index(42), Some(2));
        assert_eq!(cov.index(100), Some(3));
        assert_eq!(cov.index(0), None);
        assert_eq!(cov.index(8), None);
        assert_eq!(cov.index(101), None);
    }

    #[test]
    fn format2_index_walks_range() {
        // Two ranges; second's startCoverageIndex continues past the first.
        let cov = Coverage::parse(&fmt2(&[(10, 12, 0), (20, 21, 3)])).unwrap();
        assert_eq!(cov.index(10), Some(0));
        assert_eq!(cov.index(11), Some(1));
        assert_eq!(cov.index(12), Some(2));
        assert_eq!(cov.index(13), None);
        assert_eq!(cov.index(20), Some(3));
        assert_eq!(cov.index(21), Some(4));
        assert_eq!(cov.index(22), None);
    }

    #[test]
    fn format2_overflow_start_coverage_index_is_not_covered() {
        // A malformed range whose start_coverage_index is near u16::MAX must not
        // panic (debug overflow-check) nor wrap to a bogus index — treat the
        // overflowing glyph as not covered.
        let cov = Coverage::parse(&fmt2(&[(0, 0xFFFF, 0xFFFF)])).unwrap();
        assert_eq!(
            cov.index(0),
            Some(0xFFFF),
            "exact-fit first glyph, no overflow"
        );
        assert_eq!(
            cov.index(1),
            None,
            "0xFFFF + 1 overflows → not covered, no panic"
        );
        assert_eq!(cov.index(0xFFFF), None, "far overflow → not covered");
    }

    #[test]
    fn format2_honors_explicit_start_coverage_index() {
        // A non-zero startCoverageIndex on the first range must be honored
        // (not recomputed from cumulative position).
        let cov = Coverage::parse(&fmt2(&[(5, 5, 9)])).unwrap();
        assert_eq!(cov.index(5), Some(9));
    }

    #[test]
    fn reject_format1_not_ascending() {
        let err = Coverage::parse(&fmt1(&[5, 5])).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                field: "coverage1/glyphArray-not-ascending",
                ..
            }
        ));
    }

    #[test]
    fn reject_format2_start_gt_end() {
        let err = Coverage::parse(&fmt2(&[(20, 10, 0)])).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                field: "coverage2/start>end",
                ..
            }
        ));
    }

    #[test]
    fn reject_unknown_format() {
        let err = Coverage::parse(&3u16.to_be_bytes()).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                field: "coverage/format",
                ..
            }
        ));
    }

    #[test]
    fn truncated_is_clean_error() {
        // format=1, glyphCount=4, but only one glyph present.
        let mut b = 1u16.to_be_bytes().to_vec();
        b.extend_from_slice(&4u16.to_be_bytes());
        b.extend_from_slice(&3u16.to_be_bytes());
        assert!(matches!(
            Coverage::parse(&b),
            Err(ParseError::TableTooShort { .. })
        ));
    }
}
