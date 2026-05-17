//! R50.1.1 §5.37.1 — 자체 `ParseError` enum + Display impl.
//!
//! No thiserror dep. R50 정신 완전 적용 — 외부 lib 0.

use core::fmt;

/// `InvalidTableField.value` 가 보유하는 sign-preserving payload.
///
/// `as u64` cast 으로 sign 정보를 잃는 anti-pattern 회피 — i16/u16/i32 등이
/// 각 variant 로 정확히 보존.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FieldValue {
    Unsigned(u64),
    Signed(i64),
}

impl FieldValue {
    /// Helper — sign-extend `i16` to `FieldValue::Signed`.
    #[must_use]
    pub fn from_i16(v: i16) -> Self {
        Self::Signed(i64::from(v))
    }
    /// Helper — `i32` → `Signed`.
    #[must_use]
    pub fn from_i32(v: i32) -> Self {
        Self::Signed(i64::from(v))
    }
    /// Helper — `u16` → `Unsigned`.
    #[must_use]
    pub fn from_u16(v: u16) -> Self {
        Self::Unsigned(u64::from(v))
    }
    /// Helper — `u32` → `Unsigned`.
    #[must_use]
    pub fn from_u32(v: u32) -> Self {
        Self::Unsigned(u64::from(v))
    }
}

impl fmt::Display for FieldValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsigned(v) => write!(f, "{v}"),
            Self::Signed(v) => write!(f, "{v}"),
        }
    }
}

/// OpenType binary parser 가 reject 하는 모든 case.
///
/// 각 variant 는 spec violation 1 종류 + 진단 context 보유.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ParseError {
    //
    // ── sfnt-level (R50.1.1) ───────────────────────────────────────────
    //
    /// 4-byte sfnt magic 이 알려진 OpenType 변종 5종 중 어느 것에도 매치 안 함.
    InvalidMagic { found: [u8; 4] },

    /// byte stream 이 expected offset/length 보다 짧음.
    Truncated { needed: usize, available: usize },

    /// numTables = 0 — sfnt spec 위반 (최소 1 table 필요).
    EmptyTableDirectory,

    /// numTables 가 너무 커서 table directory 자체가 file 범위 초과.
    TableCountOverflow { num_tables: u16, file_len: usize },

    /// table record 의 offset+length 가 file 범위 밖.
    TableRangeOutOfBounds {
        tag: [u8; 4],
        offset: u32,
        length: u32,
        file_len: usize,
    },

    /// searchRange / entrySelector / rangeShift 가 numTables 와 inconsistent.
    /// (Microsoft OpenType 1.9.x spec: 정의된 값이며 tolerance 안 함.)
    InconsistentSearchParams {
        num_tables: u16,
        search_range: u16,
        entry_selector: u16,
        range_shift: u16,
    },

    //
    // ── table-level (R50.1.2) ──────────────────────────────────────────
    //
    /// 요구된 table 이 sfnt directory 에 없음.
    TableNotFound { tag: [u8; 4] },

    /// table data 길이가 minimum 보다 짧음 (Reader 가 cumulative position 보고).
    TableTooShort {
        tag: [u8; 4],
        needed: usize,
        available: usize,
    },

    /// table 안의 field 가 spec range / 고정 값을 위반. sign-preserving payload.
    InvalidTableField {
        tag: [u8; 4],
        field: &'static str,
        value: FieldValue,
    },

    /// 알려진 version 외의 값.
    UnsupportedTableVersion {
        tag: [u8; 4],
        major: u16,
        minor: u16,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic { found } => write!(
                f,
                "invalid sfnt magic: found {:#010x}, expected one of 0x00010000 (TTF), OTTO (OTF), true (Apple), typ1 (Type 1), ttcf (TTC)",
                u32::from_be_bytes(*found),
            ),
            Self::Truncated { needed, available } => write!(
                f,
                "byte stream truncated: needed {needed} bytes, available {available}",
            ),
            Self::EmptyTableDirectory => {
                f.write_str("sfnt table directory is empty (numTables = 0)")
            }
            Self::TableCountOverflow {
                num_tables,
                file_len,
            } => write!(
                f,
                "table count {num_tables} overflows file ({file_len} bytes — directory would extend past EOF)",
            ),
            Self::TableRangeOutOfBounds {
                tag,
                offset,
                length,
                file_len,
            } => {
                f.write_str("table '")?;
                write_tag(f, *tag)?;
                write!(
                    f,
                    "' range [{offset}, {}] exceeds file ({file_len} bytes)",
                    u64::from(*offset) + u64::from(*length),
                )
            }
            Self::InconsistentSearchParams {
                num_tables,
                search_range,
                entry_selector,
                range_shift,
            } => write!(
                f,
                "search params inconsistent with numTables={num_tables}: searchRange={search_range} entrySelector={entry_selector} rangeShift={range_shift}",
            ),
            Self::TableNotFound { tag } => {
                f.write_str("required table '")?;
                write_tag(f, *tag)?;
                f.write_str("' missing from sfnt directory")
            }
            Self::TableTooShort {
                tag,
                needed,
                available,
            } => {
                f.write_str("table '")?;
                write_tag(f, *tag)?;
                write!(
                    f,
                    "' too short: parser advanced to byte {needed}, but table data is only {available} bytes",
                )
            }
            Self::InvalidTableField { tag, field, value } => {
                f.write_str("table '")?;
                write_tag(f, *tag)?;
                write!(f, "' field '{field}' has invalid value {value}")
            }
            Self::UnsupportedTableVersion { tag, major, minor } => {
                f.write_str("table '")?;
                write_tag(f, *tag)?;
                write!(f, "' has unsupported version {major}.{minor}")
            }
        }
    }
}

/// 4-byte ASCII tag 의 사람-읽기 표현. spec range 0x20..=0x7e 외 byte 는 backslash-x escape.
fn write_tag(f: &mut fmt::Formatter<'_>, tag: [u8; 4]) -> fmt::Result {
    use core::fmt::Write as _;
    for byte in tag {
        if (0x20..=0x7e).contains(&byte) {
            f.write_char(byte as char)?;
        } else {
            write!(f, "\\x{byte:02x}")?;
        }
    }
    Ok(())
}

impl core::error::Error for ParseError {}
