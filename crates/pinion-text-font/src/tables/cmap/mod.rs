//! R50.1.3 §5.37.1 — `cmap` table (character-to-glyph mapping).
//!
//! Microsoft OpenType 1.9.x spec, "cmap" chapter. Format 4 (segment mapping
//! for BMP) + Format 12 (sequential mapping for full Unicode) parsed. 그 외
//! formats (0/2/6/8/10/13/14) 는 encoding record 만 보존 (`subtables[i]` =
//! `None`) — R50.1.X 후속에서 필요한 format 추가.
//!
//! Subtable selection priority (per Microsoft typography reference):
//!
//! 1. Format 12 + Microsoft Unicode UCS-4 (platform 3, encoding 10)
//!    또는 Unicode platform full repertoire (platform 0, encoding 4/6)
//! 2. Format 12 with any platform/encoding
//! 3. Format 4 + Microsoft Unicode BMP (platform 3, encoding 1) 또는
//!    Unicode platform BMP (platform 0, encoding 3)
//! 4. Format 4 with any platform/encoding
//!
//! R50.1.3.2 split: `cmap.rs` → `cmap/{mod, format4, format12, test_helpers}`.
//! industry precedent (read-fonts, ttf-parser) 정합. Format 별 하나의 module.

use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

mod format0;
mod format12;
mod format4;
#[cfg(test)]
mod test_helpers;

pub use format0::Format0;
pub use format12::{Format12, SequentialMapGroup};
pub use format4::Format4;

pub(super) const CMAP_TAG: [u8; 4] = *b"cmap";

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct EncodingRecord {
    pub platform_id: u16,
    pub encoding_id: u16,
    pub subtable_offset: u32,
    /// Subtable format header (uint16 at `subtable_offset`).
    pub subtable_format: u16,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum CmapSubtable {
    Format0(Format0),
    Format4(Format4),
    Format12(Format12),
}

impl CmapSubtable {
    #[must_use]
    pub fn glyph_id(&self, codepoint: u32) -> Option<u16> {
        match self {
            Self::Format0(f) => f.glyph_id(codepoint),
            Self::Format4(f) => f.glyph_id(codepoint),
            Self::Format12(f) => f.glyph_id(codepoint),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Cmap {
    pub version: u16,
    pub encodings: Vec<EncodingRecord>,
    /// Encoding index → parsed subtable. Only Format 4 / Format 12 subtables
    /// are parsed; entries for unsupported formats remain `None`.
    pub subtables: Vec<Option<CmapSubtable>>,
}

impl Cmap {
    /// Parse the cmap table bytes.
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — header / encoding records / subtable bodies short.
    /// * [`ParseError::InvalidTableField`] — version != 0 / numTables == 0 / various subtable
    ///   field violations.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, CMAP_TAG);
        let version = r.read_u16()?;
        if version != 0 {
            return Err(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "version",
                value: FieldValue::from_u16(version),
            });
        }
        let num_tables = r.read_u16()?;
        if num_tables == 0 {
            return Err(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "numTables",
                value: FieldValue::Unsigned(0),
            });
        }

        // Read encoding records (3 fields × numTables — 8 bytes each).
        // Spec: (platformID, encodingID) tuples must be unique within cmap —
        // strict reject duplicates (a font cannot have two records claiming the
        // same encoding; ambiguity violates AI introspect determinism).
        let mut prelim: Vec<(u16, u16, u32)> = Vec::with_capacity(usize::from(num_tables));
        for _ in 0..num_tables {
            let platform_id = r.read_u16()?;
            let encoding_id = r.read_u16()?;
            let subtable_offset = r.read_u32()?;
            if prelim
                .iter()
                .any(|&(p, e, _)| p == platform_id && e == encoding_id)
            {
                return Err(ParseError::InvalidTableField {
                    tag: CMAP_TAG,
                    field: "encodingRecord/duplicate(platform,encoding)",
                    value: FieldValue::Unsigned(
                        (u64::from(platform_id) << 16) | u64::from(encoding_id),
                    ),
                });
            }
            prelim.push((platform_id, encoding_id, subtable_offset));
        }

        // For each encoding, peek the subtable format and parse if supported.
        let mut encodings = Vec::with_capacity(usize::from(num_tables));
        let mut subtables: Vec<Option<CmapSubtable>> = Vec::with_capacity(usize::from(num_tables));
        for &(platform_id, encoding_id, subtable_offset) in &prelim {
            let off = subtable_offset as usize;
            if off.saturating_add(2) > bytes.len() {
                return Err(ParseError::TableTooShort {
                    tag: CMAP_TAG,
                    needed: off + 2,
                    available: bytes.len(),
                });
            }
            let format = u16::from_be_bytes([bytes[off], bytes[off + 1]]);
            let parsed = match format {
                0 => Some(CmapSubtable::Format0(Format0::parse(&bytes[off..])?)),
                4 => Some(CmapSubtable::Format4(Format4::parse(&bytes[off..])?)),
                12 => Some(CmapSubtable::Format12(Format12::parse(&bytes[off..])?)),
                _ => None,
            };
            encodings.push(EncodingRecord {
                platform_id,
                encoding_id,
                subtable_offset,
                subtable_format: format,
            });
            subtables.push(parsed);
        }

        Ok(Self {
            version,
            encodings,
            subtables,
        })
    }

    /// Pick the best subtable for character lookup.
    ///
    /// Priority order: see module-level docs. Implementation 은
    /// [`selection_score`] + `min_by_key` 1-pass — 4-pass duplicate 회피.
    #[must_use]
    pub fn best_subtable(&self) -> Option<&CmapSubtable> {
        self.encodings
            .iter()
            .zip(self.subtables.iter())
            .filter_map(|(rec, sub)| sub.as_ref().map(|s| (rec, s)))
            .min_by_key(|(rec, sub)| selection_score(rec, sub))
            .map(|(_, sub)| sub)
    }

    /// Map a Unicode codepoint to a glyph ID via the best subtable.
    /// Returns `None` if the codepoint isn't mapped or no supported subtable.
    #[must_use]
    pub fn glyph_id(&self, codepoint: u32) -> Option<u16> {
        self.best_subtable()?.glyph_id(codepoint)
    }
}

/// Subtable selection priority — lower = better.
///
/// 0. Format 12 with Microsoft Unicode UCS-4 (3, 10) or Unicode full (0, 4/6)
/// 1. Format 12 with any other platform/encoding
/// 2. Format 4 with Microsoft Unicode BMP (3, 1) or Unicode BMP (0, 3)
/// 3. Format 4 with any other platform/encoding
/// 4. Format 0 (legacy 8-bit Mac Roman) — fallback when nothing else
///
/// Function 형태 — R50.X RPC introspect 시 AI agent 가 자체 score 계산
/// 가능하도록 priority 표현체 직접 노출 friendly.
fn selection_score(rec: &EncodingRecord, sub: &CmapSubtable) -> u8 {
    match sub {
        CmapSubtable::Format12(_)
            if is_preferred_unicode_full(rec.platform_id, rec.encoding_id) =>
        {
            0
        }
        CmapSubtable::Format12(_) => 1,
        CmapSubtable::Format4(_) if is_preferred_unicode_bmp(rec.platform_id, rec.encoding_id) => {
            2
        }
        CmapSubtable::Format4(_) => 3,
        CmapSubtable::Format0(_) => 4,
    }
}

/// Microsoft Unicode UCS-4 (3, 10) or Unicode platform full (0, 4/6).
fn is_preferred_unicode_full(platform_id: u16, encoding_id: u16) -> bool {
    (platform_id == 0 && (encoding_id == 4 || encoding_id == 6))
        || (platform_id == 3 && encoding_id == 10)
}

/// Microsoft Unicode BMP (3, 1) or Unicode platform BMP (0, 3).
fn is_preferred_unicode_bmp(platform_id: u16, encoding_id: u16) -> bool {
    (platform_id == 0 && encoding_id == 3) || (platform_id == 3 && encoding_id == 1)
}

#[cfg(test)]
mod tests {
    use super::test_helpers::*;
    use super::*;

    #[test]
    fn parse_minimal_format4_direct_delta() {
        let sub = build_format4_simple(0x0041, 0x005A, 35);
        let cmap_bytes = build_cmap_with_subtable(3, 1, &sub);
        let cmap = Cmap::parse(&cmap_bytes).expect("valid cmap");
        assert_eq!(cmap.version, 0);
        assert_eq!(cmap.encodings.len(), 1);
        assert_eq!(cmap.encodings[0].subtable_format, 4);
        assert_eq!(cmap.glyph_id(0x0041), Some(100));
        assert_eq!(cmap.glyph_id(0x005A), Some(100 + 0x19));
        assert_eq!(cmap.glyph_id(0x0040), None);
        assert_eq!(cmap.glyph_id(0x005B), None);
    }

    #[test]
    fn parse_minimal_format12() {
        let group = SequentialMapGroup {
            start_char_code: 0xAC00,
            end_char_code: 0xD7A3,
            start_glyph_id: 5000,
        };
        let sub = build_format12_simple(&[group]);
        let cmap_bytes = build_cmap_with_subtable(3, 10, &sub);
        let cmap = Cmap::parse(&cmap_bytes).expect("valid cmap");
        assert_eq!(cmap.encodings[0].subtable_format, 12);
        assert_eq!(cmap.glyph_id(0xAC00), Some(5000));
        assert_eq!(cmap.glyph_id(0xAC70), Some(5000 + 0x70));
        assert_eq!(cmap.glyph_id(0xABFF), None);
        assert_eq!(cmap.glyph_id(0xD7A4), None);
    }

    #[test]
    fn selection_score_priority_ordering() {
        use super::format12::SequentialMapGroup;
        let f4 = CmapSubtable::Format4(
            Format4::parse(&build_format4_simple(0x41, 0x5A, 35)).unwrap(),
        );
        let f12 = CmapSubtable::Format12(
            Format12::parse(&build_format12_simple(&[SequentialMapGroup {
                start_char_code: 0x41,
                end_char_code: 0x5A,
                start_glyph_id: 100,
            }]))
            .unwrap(),
        );

        // priority 0: format 12 + preferred (3, 10)
        let rec_f12_preferred = EncodingRecord {
            platform_id: 3,
            encoding_id: 10,
            subtable_offset: 0,
            subtable_format: 12,
        };
        assert_eq!(selection_score(&rec_f12_preferred, &f12), 0);

        // priority 1: format 12 + any non-preferred
        let rec_f12_other = EncodingRecord {
            platform_id: 1,
            encoding_id: 0,
            subtable_offset: 0,
            subtable_format: 12,
        };
        assert_eq!(selection_score(&rec_f12_other, &f12), 1);

        // priority 2: format 4 + preferred (3, 1)
        let rec_f4_preferred = EncodingRecord {
            platform_id: 3,
            encoding_id: 1,
            subtable_offset: 0,
            subtable_format: 4,
        };
        assert_eq!(selection_score(&rec_f4_preferred, &f4), 2);

        // priority 3: format 4 + any non-preferred
        let rec_f4_other = EncodingRecord {
            platform_id: 1,
            encoding_id: 0,
            subtable_offset: 0,
            subtable_format: 4,
        };
        assert_eq!(selection_score(&rec_f4_other, &f4), 3);

        // Unicode platform variants for preferred:
        // (0, 4) and (0, 6) preferred full Unicode.
        let rec_unicode_full = EncodingRecord {
            platform_id: 0,
            encoding_id: 4,
            subtable_offset: 0,
            subtable_format: 12,
        };
        assert_eq!(selection_score(&rec_unicode_full, &f12), 0);

        // (0, 3) preferred BMP.
        let rec_unicode_bmp = EncodingRecord {
            platform_id: 0,
            encoding_id: 3,
            subtable_offset: 0,
            subtable_format: 4,
        };
        assert_eq!(selection_score(&rec_unicode_bmp, &f4), 2);
    }

    #[test]
    fn format12_preferred_over_format4() {
        let sub4 = build_format4_simple(0x0041, 0x005A, 35);
        let sub12 = build_format12_simple(&[SequentialMapGroup {
            start_char_code: 0x0041,
            end_char_code: 0x005A,
            start_glyph_id: 9999,
        }]);

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u16.to_be_bytes()); // version
        bytes.extend_from_slice(&2u16.to_be_bytes()); // numTables
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&20u32.to_be_bytes());
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes());
        let off12: u32 = 20 + u32::try_from(sub4.len()).unwrap();
        bytes.extend_from_slice(&off12.to_be_bytes());
        bytes.extend_from_slice(&sub4);
        bytes.extend_from_slice(&sub12);

        let cmap = Cmap::parse(&bytes).expect("valid cmap");
        assert_eq!(cmap.encodings.len(), 2);
        assert_eq!(cmap.glyph_id(0x0041), Some(9999));
    }

    #[test]
    fn reject_invalid_version() {
        let mut bytes = build_cmap_with_subtable(3, 1, &build_format4_simple(0x41, 0x5A, 35));
        bytes[0..2].copy_from_slice(&1u16.to_be_bytes());
        let err = Cmap::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "version",
                value: FieldValue::Unsigned(1),
            }
        );
    }

    #[test]
    fn reject_zero_num_tables() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u16.to_be_bytes()); // version
        bytes.extend_from_slice(&0u16.to_be_bytes()); // numTables = 0
        let err = Cmap::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "numTables",
                value: FieldValue::Unsigned(0),
            }
        );
    }

    #[test]
    fn unsupported_format_subtable_none() {
        // Format 6 = trimmed table (not yet supported in R50.1.x).
        let mut sub = Vec::new();
        sub.extend_from_slice(&6u16.to_be_bytes()); // format = 6
        sub.extend_from_slice(&10u16.to_be_bytes()); // length (minimal)
        sub.extend_from_slice(&0u16.to_be_bytes()); // language
        sub.extend_from_slice(&0u16.to_be_bytes()); // firstCode
        sub.extend_from_slice(&0u16.to_be_bytes()); // entryCount = 0
        let cmap_bytes = build_cmap_with_subtable(1, 0, &sub);
        let cmap = Cmap::parse(&cmap_bytes).expect("valid cmap with format 6");
        assert_eq!(cmap.encodings[0].subtable_format, 6);
        assert!(cmap.subtables[0].is_none());
        assert_eq!(cmap.glyph_id(0x0041), None);
    }

    #[test]
    fn format0_parsed_and_fallback_priority() {
        // Format 0: 262-byte fixed. 'A' (0x41) → glyph 42.
        let mut sub = Vec::new();
        sub.extend_from_slice(&0u16.to_be_bytes()); // format = 0
        sub.extend_from_slice(&262u16.to_be_bytes()); // length = 262
        sub.extend_from_slice(&0u16.to_be_bytes()); // language
        let mut array = [0u8; 256];
        array[0x41] = 42;
        sub.extend_from_slice(&array);
        let cmap_bytes = build_cmap_with_subtable(1, 0, &sub); // Macintosh platform
        let cmap = Cmap::parse(&cmap_bytes).expect("valid cmap with format 0");
        assert!(matches!(cmap.subtables[0], Some(CmapSubtable::Format0(_))));
        assert_eq!(cmap.glyph_id(0x41), Some(42));
        // format 0 is priority 4 (lowest), so still selected when sole subtable.
        assert_eq!(selection_score(&cmap.encodings[0], cmap.subtables[0].as_ref().unwrap()), 4);
    }

    #[test]
    fn reject_duplicate_encoding_record() {
        let sub = build_format4_simple(0x0041, 0x005A, 35);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u16.to_be_bytes()); // version
        bytes.extend_from_slice(&2u16.to_be_bytes()); // numTables = 2
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&20u32.to_be_bytes());
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&20u32.to_be_bytes());
        bytes.extend_from_slice(&sub);

        let err = Cmap::parse(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "encodingRecord/duplicate(platform,encoding)",
                ..
            }
        ));
    }
}
