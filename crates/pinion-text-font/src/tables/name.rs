//! R50.1.5 §5.37.1 — `name` table (font naming).
//!
//! Microsoft OpenType 1.9.x spec, "name" chapter. Two versions:
//!
//! * **v0** — `count` name records + `storageOffset` to string storage.
//! * **v1** — adds `langTagCount` + `langTagRecord[]` for custom language tags.
//!
//! Layout:
//!
//! ```text
//! version              uint16  (0 또는 1)
//! count                uint16  (numberOfNameRecords)
//! storageOffset        uint16  (offset to string storage from start of table)
//! nameRecord[count]    12 bytes each
//! [v1 only:]
//!   langTagCount       uint16
//!   langTagRecord[]    4 bytes each
//! [string storage at storageOffset]
//! ```
//!
//! `NameRecord` (12 bytes):
//!
//! ```text
//! platformID    uint16
//! encodingID    uint16
//! languageID    uint16
//! nameID        uint16  (semantic — 0=copyright, 1=family, 2=subfamily, ...)
//! length        uint16  (byte length of string)
//! stringOffset  uint16  (offset from storageOffset)
//! ```
//!
//! Encoding (caller responsibility):
//!
//! * Unicode platform (`platformID = 0`, any encoding) — UTF-16BE
//! * Windows platform (`platformID = 3, encodingID = 0, 1, 10`) — UTF-16BE
//! * 그 외 (Macintosh / ISO) — raw bytes 만 노출 (parser 변환 안 함)
//!
//! `NameRecord::decode_utf16be()` 가 UTF-16BE → `String` helper.

use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

const NAME_TAG: [u8; 4] = *b"name";

/// `name` table parsed view.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Name {
    pub version: u16,
    pub records: Vec<NameRecord>,
    /// v1 only — empty vec for v0.
    pub lang_tag_records: Vec<LangTagRecord>,
    /// Raw string storage — all `NameRecord::string` slices 가 여기로 reference.
    /// 항구 보존 (source-of-truth + future encoding 변환 가능).
    pub storage: Vec<u8>,
}

/// Single name record — 12-byte fixed header + extracted `string` slice.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct NameRecord {
    pub platform_id: u16,
    pub encoding_id: u16,
    pub language_id: u16,
    pub name_id: u16,
    /// Raw string bytes (encoded per (platform, encoding) — typically UTF-16BE).
    pub string: Vec<u8>,
}

/// v1-only language tag record. `tag` = raw UTF-16BE bytes (BCP 47 tag).
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct LangTagRecord {
    pub tag: Vec<u8>,
}

/// Standard `nameID` 의미 (Microsoft OpenType "name" Table 5).
///
/// `0..=25` 가 spec-defined; `26..` 는 reserved. 알 수 없는 nameID 는
/// [`NameId::Other`] 로 라우팅 — strict reject 안 함 (forward compat).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum NameId {
    CopyrightNotice,
    FontFamily,
    FontSubfamily,
    UniqueId,
    FullName,
    Version,
    PostScriptName,
    Trademark,
    Manufacturer,
    Designer,
    Description,
    VendorUrl,
    DesignerUrl,
    LicenseDescription,
    LicenseUrl,
    // 15 = reserved (skipped; Other(15) routes here)
    TypographicFamily,
    TypographicSubfamily,
    CompatibleFullName,
    SampleText,
    PostScriptCidFindFontName,
    WwsFamily,
    WwsSubfamily,
    LightBackgroundPalette,
    DarkBackgroundPalette,
    VariationsPostScriptNamePrefix,
    /// 15 (reserved) / 26.. / 알 수 없는 값. raw value 보존.
    Other(u16),
}

impl NameId {
    /// `nameID` u16 → semantic enum.
    #[must_use]
    pub fn from_raw(value: u16) -> Self {
        match value {
            0 => Self::CopyrightNotice,
            1 => Self::FontFamily,
            2 => Self::FontSubfamily,
            3 => Self::UniqueId,
            4 => Self::FullName,
            5 => Self::Version,
            6 => Self::PostScriptName,
            7 => Self::Trademark,
            8 => Self::Manufacturer,
            9 => Self::Designer,
            10 => Self::Description,
            11 => Self::VendorUrl,
            12 => Self::DesignerUrl,
            13 => Self::LicenseDescription,
            14 => Self::LicenseUrl,
            16 => Self::TypographicFamily,
            17 => Self::TypographicSubfamily,
            18 => Self::CompatibleFullName,
            19 => Self::SampleText,
            20 => Self::PostScriptCidFindFontName,
            21 => Self::WwsFamily,
            22 => Self::WwsSubfamily,
            23 => Self::LightBackgroundPalette,
            24 => Self::DarkBackgroundPalette,
            25 => Self::VariationsPostScriptNamePrefix,
            other => Self::Other(other),
        }
    }

    /// Reverse — semantic enum → raw u16.
    #[must_use]
    pub fn as_raw(self) -> u16 {
        match self {
            Self::CopyrightNotice => 0,
            Self::FontFamily => 1,
            Self::FontSubfamily => 2,
            Self::UniqueId => 3,
            Self::FullName => 4,
            Self::Version => 5,
            Self::PostScriptName => 6,
            Self::Trademark => 7,
            Self::Manufacturer => 8,
            Self::Designer => 9,
            Self::Description => 10,
            Self::VendorUrl => 11,
            Self::DesignerUrl => 12,
            Self::LicenseDescription => 13,
            Self::LicenseUrl => 14,
            Self::TypographicFamily => 16,
            Self::TypographicSubfamily => 17,
            Self::CompatibleFullName => 18,
            Self::SampleText => 19,
            Self::PostScriptCidFindFontName => 20,
            Self::WwsFamily => 21,
            Self::WwsSubfamily => 22,
            Self::LightBackgroundPalette => 23,
            Self::DarkBackgroundPalette => 24,
            Self::VariationsPostScriptNamePrefix => 25,
            Self::Other(v) => v,
        }
    }
}

impl NameRecord {
    /// `name_id` 를 semantic enum 로 반환.
    #[must_use]
    pub fn name_id_enum(&self) -> NameId {
        NameId::from_raw(self.name_id)
    }

    /// `(platform_id, encoding_id)` 가 UTF-16BE 인 경우 raw bytes → `String`.
    ///
    /// 지원:
    /// * `(0, *)` — Unicode platform, UTF-16BE
    /// * `(3, 0 | 1 | 10)` — Windows Symbol / Unicode BMP / Unicode UCS-4
    ///
    /// 그 외 (Macintosh, ISO 등) 또는 odd-length bytes (UTF-16 invariant 위반)
    /// 면 `None`.
    #[must_use]
    pub fn decode_utf16be(&self) -> Option<String> {
        if !self.is_utf16be_encoding() {
            return None;
        }
        if self.string.len() % 2 != 0 {
            return None;
        }
        let units: Vec<u16> = self
            .string
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        // UTF-16 → String. Invalid surrogate sequence → None.
        char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .ok()
    }

    fn is_utf16be_encoding(&self) -> bool {
        matches!(
            (self.platform_id, self.encoding_id),
            (0, _) | (3, 0 | 1 | 10)
        )
    }
}

impl Name {
    /// Parse the name table bytes.
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — header / records / strings 가 bytes 너머.
    /// * [`ParseError::InvalidTableField`] — version 0/1 외 / storageOffset
    ///   범위 위반 / record string offset+length 가 storage 너머.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, NAME_TAG);
        let (version, count, storage_offset) = read_header(&mut r, bytes.len())?;
        let raw_records = read_record_headers(&mut r, count)?;
        let lang_tags_raw = if version == 1 {
            read_lang_tag_headers(&mut r)?
        } else {
            Vec::new()
        };

        // spec mandate: storage 는 records / langTagRecords 너머에 위치 —
        // R50.1.2 hhea reserved / R50.1.3.1 cmap searchParams / R50.1.4.2.1
        // composite instructions strict 정신 일관.
        let header_end = r.position();
        if storage_offset < header_end {
            return Err(ParseError::InvalidTableField {
                tag: NAME_TAG,
                field: "storageOffset/overlaps-header-or-records",
                value: FieldValue::Unsigned(storage_offset as u64),
            });
        }

        let storage_bytes = &bytes[storage_offset..];
        let records = resolve_records(&raw_records, storage_bytes, storage_offset, bytes.len())?;
        let lang_tag_records =
            resolve_lang_tags(&lang_tags_raw, storage_bytes, storage_offset, bytes.len())?;

        Ok(Self {
            version,
            records,
            lang_tag_records,
            storage: storage_bytes.to_vec(),
        })
    }

    /// `nameID` 와 일치하는 첫 record 의 UTF-16BE 변환 `String`.
    ///
    /// Priority: Windows Unicode BMP (3, 1) > Unicode (0, *) > 첫 match.
    /// 검색 우선순위는 ttf-parser / Microsoft typography reference 정합.
    #[must_use]
    pub fn find_string(&self, name_id: NameId) -> Option<String> {
        let raw_id = name_id.as_raw();
        // Priority 1: Windows Unicode BMP (3, 1, English/US = 0x0409).
        let preferred = self.records.iter().find(|r| {
            r.name_id == raw_id
                && r.platform_id == 3
                && r.encoding_id == 1
                && r.language_id == 0x0409
        });
        if let Some(rec) = preferred {
            if let Some(s) = rec.decode_utf16be() {
                return Some(s);
            }
        }
        // Priority 2: any Unicode-platform UTF-16BE.
        for rec in &self.records {
            if rec.name_id == raw_id {
                if let Some(s) = rec.decode_utf16be() {
                    return Some(s);
                }
            }
        }
        None
    }
}

/// Read version + count + storageOffset header (6 bytes).
fn read_header(r: &mut Reader<'_>, table_len: usize) -> Result<(u16, u16, usize), ParseError> {
    let version = r.read_u16()?;
    if version > 1 {
        return Err(ParseError::InvalidTableField {
            tag: NAME_TAG,
            field: "version",
            value: FieldValue::from_u16(version),
        });
    }
    let count = r.read_u16()?;
    let storage_offset = usize::from(r.read_u16()?);
    if storage_offset > table_len {
        return Err(ParseError::InvalidTableField {
            tag: NAME_TAG,
            field: "storageOffset/out-of-bounds",
            value: FieldValue::Unsigned(storage_offset as u64),
        });
    }
    Ok((version, count, storage_offset))
}

type RecordTuple = (u16, u16, u16, u16, u16, u16);

/// Read `count` name records (12 bytes each).
fn read_record_headers(r: &mut Reader<'_>, count: u16) -> Result<Vec<RecordTuple>, ParseError> {
    let mut raw = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let platform_id = r.read_u16()?;
        let encoding_id = r.read_u16()?;
        let language_id = r.read_u16()?;
        let name_id = r.read_u16()?;
        let length = r.read_u16()?;
        let string_offset = r.read_u16()?;
        raw.push((
            platform_id,
            encoding_id,
            language_id,
            name_id,
            length,
            string_offset,
        ));
    }
    Ok(raw)
}

/// Read v1 langTagCount + langTagRecord[] (4 bytes each).
fn read_lang_tag_headers(r: &mut Reader<'_>) -> Result<Vec<(u16, u16)>, ParseError> {
    let lang_tag_count = r.read_u16()?;
    let mut buf = Vec::with_capacity(usize::from(lang_tag_count));
    for _ in 0..lang_tag_count {
        let length = r.read_u16()?;
        let lang_tag_offset = r.read_u16()?;
        buf.push((length, lang_tag_offset));
    }
    Ok(buf)
}

/// Resolve record offsets into actual string slices with bounds check.
fn resolve_records(
    raw: &[RecordTuple],
    storage: &[u8],
    storage_offset: usize,
    table_len: usize,
) -> Result<Vec<NameRecord>, ParseError> {
    let mut records = Vec::with_capacity(raw.len());
    for &(platform_id, encoding_id, language_id, name_id, length, string_offset) in raw {
        let so = usize::from(string_offset);
        let len = usize::from(length);
        let end = so.checked_add(len).ok_or(ParseError::InvalidTableField {
            tag: NAME_TAG,
            field: "nameRecord/offset+length-overflow",
            value: FieldValue::Unsigned(u64::from(string_offset) + u64::from(length)),
        })?;
        if end > storage.len() {
            return Err(ParseError::TableTooShort {
                tag: NAME_TAG,
                needed: storage_offset + end,
                available: table_len,
            });
        }
        let string = storage[so..end].to_vec();
        records.push(NameRecord {
            platform_id,
            encoding_id,
            language_id,
            name_id,
            string,
        });
    }
    Ok(records)
}

/// Resolve langTagRecord offsets into actual tag slices.
fn resolve_lang_tags(
    raw: &[(u16, u16)],
    storage: &[u8],
    storage_offset: usize,
    table_len: usize,
) -> Result<Vec<LangTagRecord>, ParseError> {
    let mut tags = Vec::with_capacity(raw.len());
    for &(length, lang_tag_offset) in raw {
        let so = usize::from(lang_tag_offset);
        let len = usize::from(length);
        let end = so.checked_add(len).ok_or(ParseError::InvalidTableField {
            tag: NAME_TAG,
            field: "langTagRecord/offset+length-overflow",
            value: FieldValue::Unsigned(u64::from(lang_tag_offset) + u64::from(length)),
        })?;
        if end > storage.len() {
            return Err(ParseError::TableTooShort {
                tag: NAME_TAG,
                needed: storage_offset + end,
                available: table_len,
            });
        }
        let tag = storage[so..end].to_vec();
        tags.push(LangTagRecord { tag });
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a minimal name table v0 with 1 record.
    fn build_v0_single(platform: u16, encoding: u16, name_id: u16, string: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        // header
        bytes.extend_from_slice(&0u16.to_be_bytes()); // version
        bytes.extend_from_slice(&1u16.to_be_bytes()); // count
        let storage_offset: u16 = 6 + 12; // header(6) + 1 record(12)
        bytes.extend_from_slice(&storage_offset.to_be_bytes());
        // record
        bytes.extend_from_slice(&platform.to_be_bytes());
        bytes.extend_from_slice(&encoding.to_be_bytes());
        bytes.extend_from_slice(&0x0409u16.to_be_bytes()); // language = en-US
        bytes.extend_from_slice(&name_id.to_be_bytes());
        bytes.extend_from_slice(&u16::try_from(string.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes()); // stringOffset = 0
        // string storage
        bytes.extend_from_slice(string);
        bytes
    }

    /// Build UTF-16BE bytes for an ASCII string.
    fn utf16be_ascii(s: &str) -> Vec<u8> {
        let mut buf = Vec::with_capacity(s.len() * 2);
        for c in s.chars() {
            let u = c as u16;
            buf.extend_from_slice(&u.to_be_bytes());
        }
        buf
    }

    #[test]
    fn parse_minimal_v0_with_family_name() {
        let family = utf16be_ascii("Test Family");
        let bytes = build_v0_single(3, 1, 1, &family); // (3, 1) = Windows Unicode BMP
        let name = Name::parse(&bytes).expect("valid name");
        assert_eq!(name.version, 0);
        assert_eq!(name.records.len(), 1);
        assert_eq!(name.records[0].name_id, 1);
        assert_eq!(name.records[0].platform_id, 3);
        assert_eq!(name.find_string(NameId::FontFamily).as_deref(), Some("Test Family"));
    }

    #[test]
    fn name_id_enum_round_trip() {
        for raw in [0u16, 1, 6, 14, 16, 25, 26, 100, 0xFFFF] {
            let id = NameId::from_raw(raw);
            assert_eq!(id.as_raw(), raw, "round-trip nameID {raw}");
        }
    }

    #[test]
    fn name_id_other_for_unknown() {
        assert_eq!(NameId::from_raw(15), NameId::Other(15)); // 15 = reserved per spec
        assert_eq!(NameId::from_raw(100), NameId::Other(100));
    }

    #[test]
    fn reject_unsupported_version() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2u16.to_be_bytes()); // version = 2 (only 0/1 valid)
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&6u16.to_be_bytes());
        let err = Name::parse(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "version",
                ..
            } if tag == NAME_TAG
        ));
    }

    #[test]
    fn reject_storage_offset_out_of_bounds() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&100u16.to_be_bytes()); // storageOffset = 100 > 6
        let err = Name::parse(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "storageOffset/out-of-bounds",
                ..
            } if tag == NAME_TAG
        ));
    }

    #[test]
    fn reject_storage_offset_overlaps_records() {
        // storageOffset = 6 (header end) but count = 2 → records 가 storage 와 overlap.
        // record 들이 12 byte × 2 = 24 byte 필요하므로 storage start 가 30 이상 이어야.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u16.to_be_bytes()); // version
        bytes.extend_from_slice(&2u16.to_be_bytes()); // count = 2
        bytes.extend_from_slice(&6u16.to_be_bytes()); // storageOffset = 6 (header end, overlap)
        // first record 12 bytes (filler)
        bytes.extend_from_slice(&[0u8; 12]);
        bytes.extend_from_slice(&[0u8; 12]);
        let err = Name::parse(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "storageOffset/overlaps-header-or-records",
                ..
            } if tag == NAME_TAG
        ));
    }

    #[test]
    fn reject_record_string_out_of_storage() {
        // Build with storageOffset = 6 + 12 = 18 but record claims string [50, 50+10]
        // → past storage.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u16.to_be_bytes()); // version
        bytes.extend_from_slice(&1u16.to_be_bytes()); // count
        bytes.extend_from_slice(&18u16.to_be_bytes()); // storageOffset = 18
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes()); // length = 10
        bytes.extend_from_slice(&50u16.to_be_bytes()); // stringOffset = 50 (out of bounds)
        // storage = 4 bytes (insufficient)
        bytes.extend_from_slice(&[0u8; 4]);
        let err = Name::parse(&bytes).unwrap_err();
        assert!(matches!(err, ParseError::TableTooShort { .. }));
    }

    #[test]
    fn decode_utf16be_for_unicode_platform() {
        let bytes = utf16be_ascii("Hello");
        let rec = NameRecord {
            platform_id: 0,
            encoding_id: 4,
            language_id: 0,
            name_id: 1,
            string: bytes,
        };
        assert_eq!(rec.decode_utf16be().as_deref(), Some("Hello"));
    }

    #[test]
    fn decode_utf16be_rejects_macintosh_platform() {
        // Macintosh platform = 1 — UTF-16BE 변환 안 함.
        let rec = NameRecord {
            platform_id: 1,
            encoding_id: 0,
            language_id: 0,
            name_id: 1,
            string: b"Hello".to_vec(),
        };
        assert_eq!(rec.decode_utf16be(), None);
    }

    #[test]
    fn decode_utf16be_rejects_odd_length() {
        let rec = NameRecord {
            platform_id: 3,
            encoding_id: 1,
            language_id: 0,
            name_id: 1,
            string: vec![0x00, 0x48, 0x00], // odd length
        };
        assert_eq!(rec.decode_utf16be(), None);
    }

    #[test]
    fn parse_v1_with_lang_tag_records() {
        // version 1, count 0, langTagCount 1.
        let lang_tag = utf16be_ascii("en-US-custom");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u16.to_be_bytes()); // version
        bytes.extend_from_slice(&0u16.to_be_bytes()); // count
        let storage_offset: u16 = 6 + 2 + 4; // header(6) + langTagCount(2) + langTagRecord(4)
        bytes.extend_from_slice(&storage_offset.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes()); // langTagCount
        // lang tag record: length + offset
        bytes.extend_from_slice(&u16::try_from(lang_tag.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes()); // offset = 0 from storage
        // storage
        bytes.extend_from_slice(&lang_tag);

        let name = Name::parse(&bytes).expect("valid v1");
        assert_eq!(name.version, 1);
        assert_eq!(name.lang_tag_records.len(), 1);
        assert_eq!(name.lang_tag_records[0].tag, lang_tag);
    }
}
