//! R50.1.1 §5.37.1 — sfnt Offset Table + Table Records parser.
//!
//! Microsoft OpenType 1.9.x spec, `sfnt` chapter. Big-endian byte order.
//!
//! sfnt structure (file head):
//!
//! ```text
//! Offset Table   (12 bytes)
//!   sfntVersion       u32  — flavor magic (0x00010000 / OTTO / true / typ1 / ttcf)
//!   numTables         u16
//!   searchRange       u16  — (2^floor(log2(numTables))) * 16
//!   entrySelector     u16  — floor(log2(numTables))
//!   rangeShift        u16  — numTables * 16 - searchRange
//!
//! Table Record  (16 bytes each, numTables 개)
//!   tag               u32  — 4-byte ASCII identifier
//!   checkSum          u32
//!   offset            u32  — from start of font file
//!   length            u32
//! ```

use crate::error::ParseError;

/// sfnt flavor — 4-byte magic 으로 식별되는 OpenType 변종.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Flavor {
    /// TrueType outlines (`0x00010000`).
    TrueType,
    /// OpenType / CFF outlines (`OTTO`).
    OpenTypeCff,
    /// Apple-flavor TrueType (`true`).
    AppleTrueType,
    /// Type 1 (`typ1`).
    Type1,
    /// TrueType Collection (`ttcf`).
    Collection,
}

const MAGIC_TRUETYPE: u32 = 0x0001_0000;
const MAGIC_OTTO: u32 = u32::from_be_bytes(*b"OTTO");
const MAGIC_TRUE: u32 = u32::from_be_bytes(*b"true");
const MAGIC_TYP1: u32 = u32::from_be_bytes(*b"typ1");
const MAGIC_TTCF: u32 = u32::from_be_bytes(*b"ttcf");

impl Flavor {
    /// 4-byte magic 을 flavor 로 매핑. 알려진 magic 아니면 `None`.
    #[must_use]
    pub fn from_magic(magic: u32) -> Option<Self> {
        match magic {
            MAGIC_TRUETYPE => Some(Self::TrueType),
            MAGIC_OTTO => Some(Self::OpenTypeCff),
            MAGIC_TRUE => Some(Self::AppleTrueType),
            MAGIC_TYP1 => Some(Self::Type1),
            MAGIC_TTCF => Some(Self::Collection),
            _ => None,
        }
    }
}

/// sfnt Offset Table — font file 의 첫 12 바이트.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct OffsetTable {
    pub flavor: Flavor,
    pub num_tables: u16,
    pub search_range: u16,
    pub entry_selector: u16,
    pub range_shift: u16,
}

/// Table Record — Offset Table 직후 16*numTables 바이트 영역의 한 entry.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct TableRecord {
    pub tag: [u8; 4],
    pub checksum: u32,
    pub offset: u32,
    pub length: u32,
}

/// sfnt header + table directory parse.
///
/// `bytes` 는 font file 전체 (또는 TTC 안의 한 entry 의 시작에서 잘린 슬라이스).
/// TTC (collection) flavor 일 때는 table directory 의 offset/length 검증을
/// skip — collection 내부의 entry 들은 R50.1.X 후속 sub-round 가 처리.
///
/// # Errors
///
/// * [`ParseError::Truncated`] — 12 바이트 미만 (Offset Table 못 잡음).
/// * [`ParseError::InvalidMagic`] — magic 이 알려진 flavor 5종 외.
/// * [`ParseError::EmptyTableDirectory`] — numTables = 0.
/// * [`ParseError::InconsistentSearchParams`] — search params 가 spec 공식 위반.
/// * [`ParseError::TableCountOverflow`] — `12 + 16 * numTables > bytes.len()`.
/// * [`ParseError::TableRangeOutOfBounds`] — 한 record 의 `offset + length` 가 file 범위 밖.
pub fn parse_sfnt(bytes: &[u8]) -> Result<(OffsetTable, Vec<TableRecord>), ParseError> {
    if bytes.len() < 12 {
        return Err(ParseError::Truncated {
            needed: 12,
            available: bytes.len(),
        });
    }

    let magic = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let flavor = Flavor::from_magic(magic).ok_or(ParseError::InvalidMagic {
        found: [bytes[0], bytes[1], bytes[2], bytes[3]],
    })?;

    let num_tables = u16::from_be_bytes([bytes[4], bytes[5]]);
    if num_tables == 0 {
        return Err(ParseError::EmptyTableDirectory);
    }

    let search_range = u16::from_be_bytes([bytes[6], bytes[7]]);
    let entry_selector = u16::from_be_bytes([bytes[8], bytes[9]]);
    let range_shift = u16::from_be_bytes([bytes[10], bytes[11]]);

    verify_search_params(num_tables, search_range, entry_selector, range_shift)?;

    let directory_len = 12usize.saturating_add(usize::from(num_tables).saturating_mul(16));
    if directory_len > bytes.len() {
        return Err(ParseError::TableCountOverflow {
            num_tables,
            file_len: bytes.len(),
        });
    }

    let mut records = Vec::with_capacity(usize::from(num_tables));
    for i in 0..usize::from(num_tables) {
        let base = 12 + i * 16;
        let tag = [
            bytes[base],
            bytes[base + 1],
            bytes[base + 2],
            bytes[base + 3],
        ];
        let checksum = u32::from_be_bytes([
            bytes[base + 4],
            bytes[base + 5],
            bytes[base + 6],
            bytes[base + 7],
        ]);
        let offset = u32::from_be_bytes([
            bytes[base + 8],
            bytes[base + 9],
            bytes[base + 10],
            bytes[base + 11],
        ]);
        let length = u32::from_be_bytes([
            bytes[base + 12],
            bytes[base + 13],
            bytes[base + 14],
            bytes[base + 15],
        ]);

        // collection 의 첫 entry offset 들은 별도 의미 — 검증 skip,
        // R50.1.X 후속 sub-round 가 collection-aware parsing 추가.
        if flavor != Flavor::Collection {
            let end = u64::from(offset).saturating_add(u64::from(length));
            if end > bytes.len() as u64 {
                return Err(ParseError::TableRangeOutOfBounds {
                    tag,
                    offset,
                    length,
                    file_len: bytes.len(),
                });
            }
        }

        records.push(TableRecord {
            tag,
            checksum,
            offset,
            length,
        });
    }

    Ok((
        OffsetTable {
            flavor,
            num_tables,
            search_range,
            entry_selector,
            range_shift,
        },
        records,
    ))
}

/// spec 정의:
/// * `search_range = (2^floor(log2(numTables))) * 16`
/// * `entry_selector = floor(log2(numTables))`
/// * `range_shift = numTables * 16 - search_range`
///
/// black-box tolerance 는 §2 invariant #2 (introspect) 위반 가능 — strict reject.
fn verify_search_params(
    num_tables: u16,
    search_range: u16,
    entry_selector: u16,
    range_shift: u16,
) -> Result<(), ParseError> {
    debug_assert!(num_tables > 0, "verify_search_params requires num_tables ≥ 1");

    let lz = num_tables.leading_zeros();
    let shift = 15u32.saturating_sub(lz);
    let expected_entry_selector = u16::try_from(shift).unwrap_or(u16::MAX);
    let largest_power_of_two: u16 = 1u16 << shift;
    let expected_search_range = largest_power_of_two.saturating_mul(16);
    let expected_range_shift = num_tables
        .saturating_mul(16)
        .saturating_sub(expected_search_range);

    if search_range == expected_search_range
        && entry_selector == expected_entry_selector
        && range_shift == expected_range_shift
    {
        Ok(())
    } else {
        Err(ParseError::InconsistentSearchParams {
            num_tables,
            search_range,
            entry_selector,
            range_shift,
        })
    }
}

/// `records` 안에서 `tag` 매칭하는 `TableRecord` 의 byte slice 를 반환.
///
/// `parse_sfnt` 가 이미 `bytes` 범위를 검증했음을 전제 — offset+length 가
/// `bytes` 안에 있음 보장. 매치하는 tag 없으면 [`ParseError::TableNotFound`].
///
/// # Errors
///
/// * [`ParseError::TableNotFound`] — `tag` 매칭하는 record 없음.
pub fn find_table<'a>(
    bytes: &'a [u8],
    records: &[TableRecord],
    tag: [u8; 4],
) -> Result<&'a [u8], ParseError> {
    let record = records
        .iter()
        .find(|r| r.tag == tag)
        .ok_or(ParseError::TableNotFound { tag })?;
    let start = record.offset as usize;
    let end = start + record.length as usize;
    Ok(&bytes[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// numTables=1 의 유효한 sfnt header (1 dummy table record, offset=12+16=28).
    fn build_minimal_sfnt() -> Vec<u8> {
        let mut bytes = Vec::new();
        // Offset Table — TrueType, numTables=1, searchRange=16, entrySelector=0, rangeShift=0
        bytes.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // magic
        bytes.extend_from_slice(&1u16.to_be_bytes()); // numTables
        bytes.extend_from_slice(&16u16.to_be_bytes()); // searchRange
        bytes.extend_from_slice(&0u16.to_be_bytes()); // entrySelector
        bytes.extend_from_slice(&0u16.to_be_bytes()); // rangeShift
        // Table Record — tag='head', checksum=0, offset=28, length=4
        bytes.extend_from_slice(b"head");
        bytes.extend_from_slice(&0u32.to_be_bytes()); // checksum
        bytes.extend_from_slice(&28u32.to_be_bytes()); // offset
        bytes.extend_from_slice(&4u32.to_be_bytes()); // length
        // Table data — 4 bytes
        bytes.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        bytes
    }

    #[test]
    fn parse_minimal_valid_sfnt() {
        let bytes = build_minimal_sfnt();
        let (header, records) = parse_sfnt(&bytes).expect("valid sfnt");
        assert_eq!(header.flavor, Flavor::TrueType);
        assert_eq!(header.num_tables, 1);
        assert_eq!(header.search_range, 16);
        assert_eq!(header.entry_selector, 0);
        assert_eq!(header.range_shift, 0);
        assert_eq!(records.len(), 1);
        assert_eq!(&records[0].tag, b"head");
        assert_eq!(records[0].offset, 28);
        assert_eq!(records[0].length, 4);
    }

    #[test]
    fn reject_truncated_below_offset_table() {
        let bytes = vec![0x00, 0x01, 0x00, 0x00, 0x00]; // 5 bytes
        let err = parse_sfnt(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::Truncated {
                needed: 12,
                available: 5,
            }
        );
    }

    #[test]
    fn reject_invalid_magic() {
        let mut bytes = build_minimal_sfnt();
        bytes[0..4].copy_from_slice(b"junk");
        let err = parse_sfnt(&bytes).unwrap_err();
        assert_eq!(err, ParseError::InvalidMagic { found: *b"junk" });
    }

    #[test]
    fn accept_all_known_flavors() {
        for (magic_bytes, expected) in [
            (0x0001_0000u32.to_be_bytes(), Flavor::TrueType),
            (*b"OTTO", Flavor::OpenTypeCff),
            (*b"true", Flavor::AppleTrueType),
            (*b"typ1", Flavor::Type1),
        ] {
            let mut bytes = build_minimal_sfnt();
            bytes[0..4].copy_from_slice(&magic_bytes);
            let (header, _) = parse_sfnt(&bytes).expect("valid known flavor");
            assert_eq!(header.flavor, expected);
        }
    }

    #[test]
    fn collection_flavor_skips_range_check() {
        let mut bytes = build_minimal_sfnt();
        bytes[0..4].copy_from_slice(b"ttcf");
        // offset=u32::MAX 로 임의 값 (collection 은 검증 skip 이므로 통과 기대)
        bytes[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
        let (header, _) = parse_sfnt(&bytes).expect("collection skips range check");
        assert_eq!(header.flavor, Flavor::Collection);
    }

    #[test]
    fn reject_empty_table_directory() {
        let mut bytes = build_minimal_sfnt();
        bytes[4..6].copy_from_slice(&0u16.to_be_bytes()); // numTables=0
        let err = parse_sfnt(&bytes).unwrap_err();
        assert_eq!(err, ParseError::EmptyTableDirectory);
    }

    #[test]
    fn reject_table_count_overflow() {
        let mut bytes = build_minimal_sfnt();
        // numTables=1000 → directory 16012 bytes 필요, available 32 bytes
        bytes[4..6].copy_from_slice(&1000u16.to_be_bytes());
        // searchRange/entrySelector/rangeShift 도 consistent 하게 갱신 (그렇지 않으면
        // InconsistentSearchParams 가 먼저 trip — 우리는 overflow path 검증 의도).
        bytes[6..8].copy_from_slice(&8192u16.to_be_bytes()); // 2^9 * 16 = 8192
        bytes[8..10].copy_from_slice(&9u16.to_be_bytes());
        bytes[10..12].copy_from_slice(&(1000u16 * 16 - 8192).to_be_bytes());
        let err = parse_sfnt(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::TableCountOverflow {
                num_tables: 1000,
                file_len: 32,
            }
        );
    }

    #[test]
    fn reject_table_range_out_of_bounds() {
        let mut bytes = build_minimal_sfnt();
        // record 의 length 를 file 끝 초과로 변경
        bytes[24..28].copy_from_slice(&9999u32.to_be_bytes()); // length=9999 > 32
        let err = parse_sfnt(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::TableRangeOutOfBounds {
                tag: _,
                offset: 28,
                length: 9999,
                file_len: 32,
            }
        ));
    }

    #[test]
    fn reject_inconsistent_search_params() {
        let mut bytes = build_minimal_sfnt();
        // searchRange=999 — numTables=1 의 expected (16) 와 다름
        bytes[6..8].copy_from_slice(&999u16.to_be_bytes());
        let err = parse_sfnt(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InconsistentSearchParams { num_tables: 1, .. }
        ));
    }

    #[test]
    fn find_table_returns_matching_slice() {
        let bytes = build_minimal_sfnt();
        let (_, records) = parse_sfnt(&bytes).expect("valid sfnt");
        let head_data = find_table(&bytes, &records, *b"head").expect("table present");
        assert_eq!(head_data, &[0x00, 0x01, 0x00, 0x00]);
    }

    #[test]
    fn find_table_rejects_missing_tag() {
        let bytes = build_minimal_sfnt();
        let (_, records) = parse_sfnt(&bytes).expect("valid sfnt");
        let err = find_table(&bytes, &records, *b"ZZZZ").unwrap_err();
        assert_eq!(err, ParseError::TableNotFound { tag: *b"ZZZZ" });
    }

    #[test]
    fn verify_search_params_formula() {
        // numTables=17 (Noto Sans / Nanum Gothic fixture)
        // largest_power_of_two=16, searchRange=256, entrySelector=4, rangeShift=16
        assert!(verify_search_params(17, 256, 4, 16).is_ok());
        // numTables=2: largest_power_of_two=2, searchRange=32, entrySelector=1, rangeShift=0
        assert!(verify_search_params(2, 32, 1, 0).is_ok());
        // numTables=5: largest_power_of_two=4, searchRange=64, entrySelector=2, rangeShift=16
        assert!(verify_search_params(5, 64, 2, 16).is_ok());
    }
}
