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

mod format12;
mod format4;
#[cfg(test)]
mod test_helpers;

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
    Format4(Format4),
    Format12(Format12),
}

impl CmapSubtable {
    #[must_use]
    pub fn glyph_id(&self, codepoint: u32) -> Option<u16> {
        match self {
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
    /// Priority order: see module-level docs.
    #[must_use]
    pub fn best_subtable(&self) -> Option<&CmapSubtable> {
        // Pass 1 — format 12 with preferred Unicode encoding.
        for (rec, sub) in self.encodings.iter().zip(self.subtables.iter()) {
            if matches!(sub, Some(CmapSubtable::Format12(_)))
                && is_preferred_unicode_full(rec.platform_id, rec.encoding_id)
            {
                return sub.as_ref();
            }
        }
        // Pass 2 — any format 12.
        for sub in &self.subtables {
            if matches!(sub, Some(CmapSubtable::Format12(_))) {
                return sub.as_ref();
            }
        }
        // Pass 3 — format 4 with preferred Unicode BMP encoding.
        for (rec, sub) in self.encodings.iter().zip(self.subtables.iter()) {
            if matches!(sub, Some(CmapSubtable::Format4(_)))
                && is_preferred_unicode_bmp(rec.platform_id, rec.encoding_id)
            {
                return sub.as_ref();
            }
        }
        // Pass 4 — any format 4.
        for sub in &self.subtables {
            if matches!(sub, Some(CmapSubtable::Format4(_))) {
                return sub.as_ref();
            }
        }
        None
    }

    /// Map a Unicode codepoint to a glyph ID via the best subtable.
    /// Returns `None` if the codepoint isn't mapped or no supported subtable.
    #[must_use]
    pub fn glyph_id(&self, codepoint: u32) -> Option<u16> {
        self.best_subtable()?.glyph_id(codepoint)
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
        let mut sub = Vec::new();
        sub.extend_from_slice(&0u16.to_be_bytes()); // format = 0
        sub.extend_from_slice(&262u16.to_be_bytes()); // length
        sub.extend_from_slice(&0u16.to_be_bytes()); // language
        sub.resize(sub.len() + 256, 0);
        let cmap_bytes = build_cmap_with_subtable(1, 0, &sub);
        let cmap = Cmap::parse(&cmap_bytes).expect("valid cmap with format 0");
        assert_eq!(cmap.encodings[0].subtable_format, 0);
        assert!(cmap.subtables[0].is_none());
        assert_eq!(cmap.glyph_id(0x0041), None);
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
