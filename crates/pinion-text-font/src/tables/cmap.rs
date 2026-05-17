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

use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

const CMAP_TAG: [u8; 4] = *b"cmap";

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

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Format4 {
    pub language: u16,
    pub seg_count: u16,
    pub end_code: Vec<u16>,
    pub start_code: Vec<u16>,
    pub id_delta: Vec<i16>,
    pub id_range_offset: Vec<u16>,
    pub glyph_id_array: Vec<u16>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Format12 {
    pub language: u32,
    pub groups: Vec<SequentialMapGroup>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct SequentialMapGroup {
    pub start_char_code: u32,
    pub end_char_code: u32,
    pub start_glyph_id: u32,
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
        let mut prelim: Vec<(u16, u16, u32)> = Vec::with_capacity(usize::from(num_tables));
        for _ in 0..num_tables {
            let platform_id = r.read_u16()?;
            let encoding_id = r.read_u16()?;
            let subtable_offset = r.read_u32()?;
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

impl CmapSubtable {
    #[must_use]
    pub fn glyph_id(&self, codepoint: u32) -> Option<u16> {
        match self {
            Self::Format4(f) => f.glyph_id(codepoint),
            Self::Format12(f) => f.glyph_id(codepoint),
        }
    }
}

//
// ── Format 4 ────────────────────────────────────────────────────────────
//

impl Format4 {
    fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, CMAP_TAG);
        let format = r.read_u16()?;
        debug_assert_eq!(format, 4);
        let length = usize::from(r.read_u16()?);
        let language = r.read_u16()?;
        let seg_count_x2 = r.read_u16()?;
        if seg_count_x2 == 0 || (seg_count_x2 & 1) != 0 {
            return Err(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format4/segCountX2",
                value: FieldValue::from_u16(seg_count_x2),
            });
        }
        let seg_count = seg_count_x2 / 2;
        // searchRange / entrySelector / rangeShift — binary search hints,
        // strictly derivable from segCount. spec mandates specific values
        // but real-world fonts often have slop; consume + ignore here
        // (Format 4 의 lookup 은 정확한 hint 없이도 동일 결과).
        let _search_range = r.read_u16()?;
        let _entry_selector = r.read_u16()?;
        let _range_shift = r.read_u16()?;

        let mut end_code = Vec::with_capacity(usize::from(seg_count));
        for _ in 0..seg_count {
            end_code.push(r.read_u16()?);
        }
        if end_code.last() != Some(&0xFFFF) {
            return Err(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format4/endCode[last]",
                value: FieldValue::from_u16(end_code.last().copied().unwrap_or(0)),
            });
        }
        let reserved_pad = r.read_u16()?;
        if reserved_pad != 0 {
            return Err(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format4/reservedPad",
                value: FieldValue::from_u16(reserved_pad),
            });
        }
        let mut start_code = Vec::with_capacity(usize::from(seg_count));
        for _ in 0..seg_count {
            start_code.push(r.read_u16()?);
        }
        let mut id_delta = Vec::with_capacity(usize::from(seg_count));
        for _ in 0..seg_count {
            id_delta.push(r.read_i16()?);
        }
        let mut id_range_offset = Vec::with_capacity(usize::from(seg_count));
        for _ in 0..seg_count {
            id_range_offset.push(r.read_u16()?);
        }

        // glyphIdArray fills remaining 2-byte words of the subtable.
        let header_end = r.position();
        if header_end > length {
            return Err(ParseError::TableTooShort {
                tag: CMAP_TAG,
                needed: header_end,
                available: length,
            });
        }
        let glyph_array_bytes = length.saturating_sub(header_end);
        let glyph_array_words = glyph_array_bytes / 2;
        let mut glyph_id_array = Vec::with_capacity(glyph_array_words);
        for _ in 0..glyph_array_words {
            glyph_id_array.push(r.read_u16()?);
        }

        Ok(Self {
            language,
            seg_count,
            end_code,
            start_code,
            id_delta,
            id_range_offset,
            glyph_id_array,
        })
    }

    /// Format 4 lookup per OpenType spec § cmap format 4.
    ///
    /// Returns `None` if codepoint > 0xFFFF (BMP only) or unmapped.
    #[must_use]
    pub fn glyph_id(&self, codepoint: u32) -> Option<u16> {
        let cp = u16::try_from(codepoint).ok()?;

        // Binary search: first segment with endCode >= cp.
        let i = self.end_code.partition_point(|&end| end < cp);
        if i >= self.end_code.len() {
            return None;
        }
        if self.start_code[i] > cp {
            return None;
        }

        let glyph = if self.id_range_offset[i] == 0 {
            // Direct: glyph = (cp + idDelta) mod 65536
            let raw = i32::from(cp).wrapping_add(i32::from(self.id_delta[i]));
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let masked = (raw & 0xFFFF) as u16;
            masked
        } else {
            // Indirect via glyphIdArray.
            // Spec formula:
            //   *(idRangeOffset[i] / 2 + (cp - startCode[i]) + &idRangeOffset[i])
            //
            // 우리는 id_range_offset[i] 의 location 을 알고 있음 — id_range_offset
            // array 안의 i 번째 원소. glyphIdArray 의 시작은 id_range_offset 의
            // 직후. 따라서:
            //   absolute_word_index_in_subtable =
            //     id_range_offset_word_index + (id_range_offset[i] / 2) +
            //     (cp - start_code[i])
            // glyphIdArray 의 첫 word index = id_range_offset[i_last + 1] 직후
            //                                = id_range_offset 의 word offset + seg_count
            //
            // 따라서 index into glyph_id_array =
            //   i + (id_range_offset[i] / 2) + (cp - start_code[i]) - seg_count
            let i_signed = i64::try_from(i).ok()?;
            let cp_off = i64::from(cp - self.start_code[i]);
            let range_off = i64::from(self.id_range_offset[i]) / 2;
            let seg_count_signed = i64::from(self.seg_count);
            let idx = i_signed + range_off + cp_off - seg_count_signed;
            if idx < 0 {
                return None;
            }
            let idx = usize::try_from(idx).ok()?;
            let glyph_word = *self.glyph_id_array.get(idx)?;
            if glyph_word == 0 {
                return None;
            }
            let raw = i32::from(glyph_word).wrapping_add(i32::from(self.id_delta[i]));
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let masked = (raw & 0xFFFF) as u16;
            masked
        };
        if glyph == 0 { None } else { Some(glyph) }
    }
}

//
// ── Format 12 ───────────────────────────────────────────────────────────
//

impl Format12 {
    fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, CMAP_TAG);
        let format = r.read_u16()?;
        debug_assert_eq!(format, 12);
        let reserved = r.read_u16()?;
        if reserved != 0 {
            return Err(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format12/reserved",
                value: FieldValue::from_u16(reserved),
            });
        }
        let _length = r.read_u32()?;
        let language = r.read_u32()?;
        let num_groups = r.read_u32()?;
        let cap = usize::try_from(num_groups).unwrap_or(usize::MAX);
        let mut groups = Vec::with_capacity(cap.min(1_000_000));
        for _ in 0..num_groups {
            groups.push(SequentialMapGroup {
                start_char_code: r.read_u32()?,
                end_char_code: r.read_u32()?,
                start_glyph_id: r.read_u32()?,
            });
        }
        Ok(Self { language, groups })
    }

    /// Format 12 lookup: binary-search the group containing `codepoint`.
    #[must_use]
    pub fn glyph_id(&self, codepoint: u32) -> Option<u16> {
        let idx = self
            .groups
            .partition_point(|g| g.end_char_code < codepoint);
        let group = self.groups.get(idx)?;
        if group.start_char_code > codepoint {
            return None;
        }
        let delta = codepoint - group.start_char_code;
        let glyph = group.start_glyph_id.checked_add(delta)?;
        u16::try_from(glyph).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a Format 4 subtable bytes with single segment covering [start, end] →
    /// glyph `(cp + id_delta)` and final 0xFFFF sentinel segment.
    fn build_format4_simple(start: u16, end: u16, id_delta: i16) -> Vec<u8> {
        // segCount = 2 (real segment + 0xFFFF sentinel)
        let seg_count: u16 = 2;
        let seg_count_x2 = seg_count * 2;
        // length: 14 (header: format/length/language/segCountX2/searchRange/
        //              entrySelector/rangeShift = 7 × 2) + 2*segCount (endCode) +
        //         2 (reservedPad) + 2*segCount × 3 (startCode/idDelta/idRangeOffset)
        //         + 0 (no glyphIdArray since both segments use idDelta direct)
        let length: u16 = 14 + 8 * seg_count + 2;
        // searchRange = 2 * (2^floor(log2(seg_count))) = 2 * 2 = 4
        // entrySelector = 1
        // rangeShift = 2*seg_count - searchRange = 4 - 4 = 0
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&4u16.to_be_bytes()); // format
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes()); // language
        bytes.extend_from_slice(&seg_count_x2.to_be_bytes());
        bytes.extend_from_slice(&4u16.to_be_bytes()); // searchRange
        bytes.extend_from_slice(&1u16.to_be_bytes()); // entrySelector
        bytes.extend_from_slice(&0u16.to_be_bytes()); // rangeShift
        // endCode[]: real segment end, sentinel 0xFFFF
        bytes.extend_from_slice(&end.to_be_bytes());
        bytes.extend_from_slice(&0xFFFFu16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
        // startCode[]
        bytes.extend_from_slice(&start.to_be_bytes());
        bytes.extend_from_slice(&0xFFFFu16.to_be_bytes());
        // idDelta[]
        bytes.extend_from_slice(&id_delta.to_be_bytes());
        bytes.extend_from_slice(&1i16.to_be_bytes()); // sentinel maps 0xFFFF → 0 (missing)
        // idRangeOffset[] all zero → direct via idDelta
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes
    }

    fn build_format12_simple(groups: &[SequentialMapGroup]) -> Vec<u8> {
        let num_groups = u32::try_from(groups.len()).expect("test groups < u32::MAX");
        let length: u32 = 16 + 12 * num_groups;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&12u16.to_be_bytes()); // format
        bytes.extend_from_slice(&0u16.to_be_bytes()); // reserved
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes()); // language
        bytes.extend_from_slice(&num_groups.to_be_bytes());
        for g in groups {
            bytes.extend_from_slice(&g.start_char_code.to_be_bytes());
            bytes.extend_from_slice(&g.end_char_code.to_be_bytes());
            bytes.extend_from_slice(&g.start_glyph_id.to_be_bytes());
        }
        bytes
    }

    /// Build a complete cmap table with one encoding record + one subtable.
    fn build_cmap_with_subtable(
        platform_id: u16,
        encoding_id: u16,
        subtable_bytes: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u16.to_be_bytes()); // version
        bytes.extend_from_slice(&1u16.to_be_bytes()); // numTables
        bytes.extend_from_slice(&platform_id.to_be_bytes());
        bytes.extend_from_slice(&encoding_id.to_be_bytes());
        let subtable_offset: u32 = 4 + 8; // header (4) + 1 record (8)
        bytes.extend_from_slice(&subtable_offset.to_be_bytes());
        bytes.extend_from_slice(subtable_bytes);
        bytes
    }

    #[test]
    fn parse_minimal_format4_direct_delta() {
        // Map U+0041 ('A') → glyph 100. idDelta = 100 - 0x41 = 35.
        let sub = build_format4_simple(0x0041, 0x005A, 35);
        let cmap_bytes = build_cmap_with_subtable(3, 1, &sub);
        let cmap = Cmap::parse(&cmap_bytes).expect("valid cmap");
        assert_eq!(cmap.version, 0);
        assert_eq!(cmap.encodings.len(), 1);
        assert_eq!(cmap.encodings[0].subtable_format, 4);
        assert_eq!(cmap.glyph_id(0x0041), Some(100));
        assert_eq!(cmap.glyph_id(0x005A), Some(100 + 0x19));
        assert_eq!(cmap.glyph_id(0x0040), None); // outside range
        assert_eq!(cmap.glyph_id(0x005B), None); // outside range
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
        // 한글 음절 첫 글자 '가' = U+AC00 → 5000.
        assert_eq!(cmap.glyph_id(0xAC00), Some(5000));
        // 한글 음절 '거' = U+AC70 → 5000 + 0x70 = 5112.
        assert_eq!(cmap.glyph_id(0xAC70), Some(5000 + 0x70));
        assert_eq!(cmap.glyph_id(0xABFF), None); // outside range
        assert_eq!(cmap.glyph_id(0xD7A4), None); // outside range
    }

    #[test]
    fn format12_preferred_over_format4() {
        // 두 subtable 다 있을 때 format 12 가 best.
        let sub4 = build_format4_simple(0x0041, 0x005A, 35);
        let sub12 = build_format12_simple(&[SequentialMapGroup {
            start_char_code: 0x0041,
            end_char_code: 0x005A,
            start_glyph_id: 9999,
        }]);

        // build cmap with 2 records: (3,1) → format4, (3,10) → format12
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u16.to_be_bytes()); // version
        bytes.extend_from_slice(&2u16.to_be_bytes()); // numTables
        // record 0: (3, 1) → offset 4 + 16 = 20
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&20u32.to_be_bytes());
        // record 1: (3, 10) → offset 20 + sub4.len()
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes());
        let off12: u32 = 20 + u32::try_from(sub4.len()).unwrap();
        bytes.extend_from_slice(&off12.to_be_bytes());
        bytes.extend_from_slice(&sub4);
        bytes.extend_from_slice(&sub12);

        let cmap = Cmap::parse(&bytes).expect("valid cmap");
        assert_eq!(cmap.encodings.len(), 2);
        // format 12 가 best — 0x0041 → 9999.
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
        // Subtable format 0 (Byte encoding table) — 우리 parser 가 skip,
        // EncodingRecord 만 보존.
        let mut sub = Vec::new();
        sub.extend_from_slice(&0u16.to_be_bytes()); // format = 0
        sub.extend_from_slice(&262u16.to_be_bytes()); // length (256 + 6)
        sub.extend_from_slice(&0u16.to_be_bytes()); // language
        sub.resize(sub.len() + 256, 0);
        let cmap_bytes = build_cmap_with_subtable(1, 0, &sub);
        let cmap = Cmap::parse(&cmap_bytes).expect("valid cmap with format 0");
        assert_eq!(cmap.encodings[0].subtable_format, 0);
        assert!(cmap.subtables[0].is_none());
        assert_eq!(cmap.glyph_id(0x0041), None);
    }

    #[test]
    fn format4_endcode_must_terminate_with_ffff() {
        // build a format 4 with last endCode = 0x00FF (not 0xFFFF) — reject.
        let mut sub = Vec::new();
        sub.extend_from_slice(&4u16.to_be_bytes()); // format
        sub.extend_from_slice(&26u16.to_be_bytes()); // length
        sub.extend_from_slice(&0u16.to_be_bytes()); // language
        sub.extend_from_slice(&2u16.to_be_bytes()); // segCountX2 = 2 (segCount=1)
        sub.extend_from_slice(&2u16.to_be_bytes()); // searchRange
        sub.extend_from_slice(&0u16.to_be_bytes()); // entrySelector
        sub.extend_from_slice(&0u16.to_be_bytes()); // rangeShift
        sub.extend_from_slice(&0x00FFu16.to_be_bytes()); // endCode[0] — not 0xFFFF
        sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
        sub.extend_from_slice(&0x0000u16.to_be_bytes()); // startCode[0]
        sub.extend_from_slice(&1i16.to_be_bytes()); // idDelta[0]
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset[0]
        let cmap_bytes = build_cmap_with_subtable(3, 1, &sub);
        let err = Cmap::parse(&cmap_bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format4/endCode[last]",
                ..
            }
        ));
    }

    #[test]
    fn format12_supplementary_plane_codepoint() {
        // Codepoint U+1F600 (😀) — outside BMP, format 12 only.
        let group = SequentialMapGroup {
            start_char_code: 0x1_F600,
            end_char_code: 0x1_F64F,
            start_glyph_id: 2000,
        };
        let sub = build_format12_simple(&[group]);
        let cmap_bytes = build_cmap_with_subtable(3, 10, &sub);
        let cmap = Cmap::parse(&cmap_bytes).expect("valid cmap");
        assert_eq!(cmap.glyph_id(0x1_F600), Some(2000));
        assert_eq!(cmap.glyph_id(0x1_F64F), Some(2000 + 0x4F));
    }

    #[test]
    fn format4_indirect_via_id_range_offset() {
        // Build format 4 with a segment using idRangeOffset != 0:
        // segCount=2: real segment [0x41, 0x43] → glyph_id_array,
        // sentinel [0xFFFF, 0xFFFF].
        //
        // Layout offsets (within subtable bytes):
        //   0..2   format=4
        //   2..4   length
        //   4..6   language
        //   6..8   segCountX2=4
        //   8..10  searchRange
        //   10..12 entrySelector
        //   12..14 rangeShift
        //   14..18 endCode[2] = [0x43, 0xFFFF]
        //   18..20 reservedPad=0
        //   20..24 startCode[2] = [0x41, 0xFFFF]
        //   24..28 idDelta[2] = [0, 1]
        //   28..32 idRangeOffset[2] = [4, 0]
        //   32..38 glyphIdArray[3] = [100, 101, 102]
        //
        // For cp=0x41:
        //   idx_in_glyph_array = i(0) + idRangeOffset[i]/2(=2) + (cp - start)(0) - segCount(2)
        //                      = 0 + 2 + 0 - 2 = 0
        //   → glyph_id_array[0] = 100
        let seg_count: u16 = 2;
        // length: 14 (header) + 8*seg_count (4 arrays) + 2 (reservedPad) + 2*3 (glyphIdArray)
        let length: u16 = 14 + 8 * seg_count + 2 + 2 * 3;
        let mut sub = Vec::new();
        sub.extend_from_slice(&4u16.to_be_bytes());
        sub.extend_from_slice(&length.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes()); // language
        sub.extend_from_slice(&(seg_count * 2).to_be_bytes()); // segCountX2
        sub.extend_from_slice(&4u16.to_be_bytes()); // searchRange
        sub.extend_from_slice(&1u16.to_be_bytes()); // entrySelector
        sub.extend_from_slice(&0u16.to_be_bytes()); // rangeShift
        sub.extend_from_slice(&0x0043u16.to_be_bytes()); // endCode[0]
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes()); // endCode[1]
        sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
        sub.extend_from_slice(&0x0041u16.to_be_bytes()); // startCode[0]
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes()); // startCode[1]
        sub.extend_from_slice(&0i16.to_be_bytes()); // idDelta[0]
        sub.extend_from_slice(&1i16.to_be_bytes()); // idDelta[1]
        sub.extend_from_slice(&4u16.to_be_bytes()); // idRangeOffset[0]
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset[1]
        sub.extend_from_slice(&100u16.to_be_bytes()); // glyphIdArray[0]
        sub.extend_from_slice(&101u16.to_be_bytes()); // glyphIdArray[1]
        sub.extend_from_slice(&102u16.to_be_bytes()); // glyphIdArray[2]
        assert_eq!(sub.len(), usize::from(length));

        let cmap_bytes = build_cmap_with_subtable(3, 1, &sub);
        let cmap = Cmap::parse(&cmap_bytes).expect("valid cmap");
        assert_eq!(cmap.glyph_id(0x0041), Some(100));
        assert_eq!(cmap.glyph_id(0x0042), Some(101));
        assert_eq!(cmap.glyph_id(0x0043), Some(102));
        assert_eq!(cmap.glyph_id(0x0044), None);
    }

    #[test]
    fn format4_codepoint_above_bmp_returns_none() {
        let sub = build_format4_simple(0x0041, 0x005A, 35);
        let cmap_bytes = build_cmap_with_subtable(3, 1, &sub);
        let cmap = Cmap::parse(&cmap_bytes).expect("valid cmap");
        // U+1F600 > 0xFFFF → format 4 returns None.
        assert_eq!(cmap.glyph_id(0x1_F600), None);
    }
}
