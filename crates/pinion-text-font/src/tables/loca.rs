//! R50.1.4.1 §5.37.1 — `loca` table (glyph index → glyf offset).
//!
//! Microsoft OpenType 1.9.x spec, "loca" chapter. Two formats selected by
//! `head.indexToLocFormat`:
//!
//! * **Short** (`indexToLocFormat = 0`) — `uint16` entries. 실제 byte offset
//!   = stored value × 2. 한 글리프 = 2 byte entry. 최대 표현 offset = 0x1FFFE
//!   (≈ 128 KB glyf table).
//! * **Long** (`indexToLocFormat = 1`) — `uint32` entries, raw byte offset.
//!
//! 두 format 모두 `numGlyphs + 1` entries — 마지막 entry 는 sentinel marking
//! the end of the last glyph (glyph i 의 range = `[offsets[i], offsets[i+1])`).
//!
//! 위 length / monotonicity / format dispatch 가 모두 strict reject — black-box
//! tolerance 없음 (§2 invariant #2 정신).

use crate::error::{FieldValue, ParseError};

const LOCA_TAG: [u8; 4] = *b"loca";

/// `head.indexToLocFormat` 에 의해 결정되는 entry width.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum LocaFormat {
    /// `uint16` entries, raw value × 2 = byte offset. `head.indexToLocFormat = 0`.
    Short,
    /// `uint32` entries, raw byte offset. `head.indexToLocFormat = 1`.
    Long,
}

impl LocaFormat {
    /// `head.indexToLocFormat` 를 `LocaFormat` 으로 매핑. `head` parser 가 이미
    /// `0..=1` 검증 했으므로 caller 가 `head.index_to_loc_format` 값 그대로 전달.
    ///
    /// # Errors
    ///
    /// [`ParseError::InvalidTableField`] — `head_value` 가 `0..=1` 외인 경우
    /// (head parser 검증 후 race 가 일어났을 때만 발생; defensive).
    pub fn from_head_value(head_value: i16) -> Result<Self, ParseError> {
        match head_value {
            0 => Ok(Self::Short),
            1 => Ok(Self::Long),
            _ => Err(ParseError::InvalidTableField {
                tag: LOCA_TAG,
                field: "indexToLocFormat",
                value: FieldValue::from_i16(head_value),
            }),
        }
    }

    /// 한 entry 가 byte stream 에서 차지하는 width.
    #[must_use]
    pub fn entry_size(self) -> usize {
        match self {
            Self::Short => 2,
            Self::Long => 4,
        }
    }
}

/// `loca` table — `num_glyphs + 1` 의 단조 비감소 byte offset.
///
/// `offsets[i]` 는 glyph `i` 의 glyf-table-내 시작 byte offset. `offsets[i+1]`
/// 가 sentinel (또는 다음 glyph 시작) — `offsets[i] == offsets[i+1]` 면 빈
/// glyph (예: `.notdef` 이외의 control codepoint).
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Loca {
    pub format: LocaFormat,
    /// `num_glyphs + 1` entries. 단조 비감소 (`offsets[i] <= offsets[i+1]`).
    /// short format 의 경우 raw `uint16 × 2` 후의 byte offset.
    pub offsets: Vec<u32>,
}

impl Loca {
    /// Parse the loca table bytes given format + glyph count.
    ///
    /// `num_glyphs` 는 `maxp.numGlyphs` (검증된 값 ≥ 1).
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — bytes 가 expected `(num_glyphs + 1) ×
    ///   entry_size` 보다 짧음.
    /// * [`ParseError::InvalidTableField`] — entry 가 단조 비감소 위반.
    pub fn parse(bytes: &[u8], format: LocaFormat, num_glyphs: u16) -> Result<Self, ParseError> {
        let expected_entries = usize::from(num_glyphs) + 1;
        let expected_len = expected_entries
            .checked_mul(format.entry_size())
            .ok_or(ParseError::TableTooShort {
                tag: LOCA_TAG,
                needed: usize::MAX,
                available: bytes.len(),
            })?;
        if bytes.len() < expected_len {
            return Err(ParseError::TableTooShort {
                tag: LOCA_TAG,
                needed: expected_len,
                available: bytes.len(),
            });
        }

        let mut offsets = Vec::with_capacity(expected_entries);
        match format {
            LocaFormat::Short => {
                // short format: raw u16 → byte offset (× 2). u16::MAX × 2 = 0x1FFFE
                // 이므로 u32 overflow 0.
                for chunk in bytes[..expected_len].chunks_exact(2) {
                    let raw = u16::from_be_bytes([chunk[0], chunk[1]]);
                    offsets.push(u32::from(raw) * 2);
                }
            }
            LocaFormat::Long => {
                for chunk in bytes[..expected_len].chunks_exact(4) {
                    let raw = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    offsets.push(raw);
                }
            }
        }

        // spec: entries 는 단조 비감소 (glyph i+1 의 시작 ≥ glyph i 의 시작).
        // 위반 시 glyf 의 slice 결정 불가능 → strict reject.
        for window in offsets.windows(2) {
            if window[0] > window[1] {
                return Err(ParseError::InvalidTableField {
                    tag: LOCA_TAG,
                    field: "offsets/not-monotonic",
                    value: FieldValue::from_u32(window[0]),
                });
            }
        }

        Ok(Self { format, offsets })
    }

    /// Glyph `glyph_id` 의 glyf-table-내 byte range `[start, end)`.
    ///
    /// `start == end` 이면 빈 glyph (예: control codepoint). `glyph_id ≥
    /// num_glyphs` (즉 `offsets.len() - 1` 이상) 일 때 `None`.
    #[must_use]
    pub fn glyph_range(&self, glyph_id: u16) -> Option<(u32, u32)> {
        let i = usize::from(glyph_id);
        if i + 1 >= self.offsets.len() {
            return None;
        }
        Some((self.offsets[i], self.offsets[i + 1]))
    }

    /// Glyph 개수 — `offsets.len() - 1` (sentinel 제외).
    #[must_use]
    pub fn num_glyphs(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_format_dispatch_and_parse() {
        // num_glyphs = 3 → 4 entries × 2 bytes = 8 bytes.
        // entries = [0, 5, 10, 20] (offset = stored × 2 → [0, 10, 20, 40])
        let bytes: Vec<u8> = vec![0, 0, 0, 5, 0, 10, 0, 20];
        let loca = Loca::parse(&bytes, LocaFormat::Short, 3).expect("valid short loca");
        assert_eq!(loca.format, LocaFormat::Short);
        assert_eq!(loca.offsets, vec![0, 10, 20, 40]);
        assert_eq!(loca.num_glyphs(), 3);
        assert_eq!(loca.glyph_range(0), Some((0, 10)));
        assert_eq!(loca.glyph_range(1), Some((10, 20)));
        assert_eq!(loca.glyph_range(2), Some((20, 40)));
        assert_eq!(loca.glyph_range(3), None);
    }

    #[test]
    fn long_format_dispatch_and_parse() {
        // num_glyphs = 2 → 3 entries × 4 bytes = 12 bytes.
        // entries = [0x00000010, 0x00000040, 0x000000A0]
        let bytes: Vec<u8> = vec![
            0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0xA0,
        ];
        let loca = Loca::parse(&bytes, LocaFormat::Long, 2).expect("valid long loca");
        assert_eq!(loca.format, LocaFormat::Long);
        assert_eq!(loca.offsets, vec![0x10, 0x40, 0xA0]);
        assert_eq!(loca.glyph_range(0), Some((0x10, 0x40)));
        assert_eq!(loca.glyph_range(1), Some((0x40, 0xA0)));
    }

    #[test]
    fn empty_glyph_range() {
        // 두 번째 glyph 가 빈 글리프: offsets[1] == offsets[2].
        let bytes: Vec<u8> = vec![0, 0, 0, 5, 0, 5, 0, 10];
        let loca = Loca::parse(&bytes, LocaFormat::Short, 3).expect("valid loca");
        assert_eq!(loca.glyph_range(1), Some((10, 10)));
        assert_eq!(loca.glyph_range(2), Some((10, 20)));
    }

    #[test]
    fn reject_short_too_short() {
        // num_glyphs = 5 → expects (5+1)*2 = 12 bytes, but only 6 provided.
        let bytes = vec![0u8; 6];
        let err = Loca::parse(&bytes, LocaFormat::Short, 5).unwrap_err();
        assert!(matches!(
            err,
            ParseError::TableTooShort {
                tag: LOCA_TAG,
                needed: 12,
                available: 6,
            }
        ));
    }

    #[test]
    fn reject_long_too_short() {
        // num_glyphs = 2 → expects (2+1)*4 = 12 bytes, only 8 provided.
        let bytes = vec![0u8; 8];
        let err = Loca::parse(&bytes, LocaFormat::Long, 2).unwrap_err();
        assert!(matches!(
            err,
            ParseError::TableTooShort {
                tag: LOCA_TAG,
                needed: 12,
                available: 8,
            }
        ));
    }

    #[test]
    fn reject_non_monotonic_short() {
        // num_glyphs = 3, entries = [0, 10, 5, 20] — 두 번째 → 세 번째 가 감소.
        let bytes: Vec<u8> = vec![0, 0, 0, 5, 0, 0, 0, 10];
        let err = Loca::parse(&bytes, LocaFormat::Short, 3).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: LOCA_TAG,
                field: "offsets/not-monotonic",
                value: FieldValue::Unsigned(10),
            }
        );
    }

    #[test]
    fn reject_non_monotonic_long() {
        let bytes: Vec<u8> = vec![
            0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0xA0,
        ];
        let err = Loca::parse(&bytes, LocaFormat::Long, 2).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: LOCA_TAG,
                field: "offsets/not-monotonic",
                value: FieldValue::Unsigned(0x40),
            }
        );
    }

    #[test]
    fn format_from_head_value() {
        assert_eq!(LocaFormat::from_head_value(0).unwrap(), LocaFormat::Short);
        assert_eq!(LocaFormat::from_head_value(1).unwrap(), LocaFormat::Long);
        let err = LocaFormat::from_head_value(2).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: LOCA_TAG,
                field: "indexToLocFormat",
                value: FieldValue::Signed(2),
            }
        );
    }

    #[test]
    fn entry_size_correct() {
        assert_eq!(LocaFormat::Short.entry_size(), 2);
        assert_eq!(LocaFormat::Long.entry_size(), 4);
    }

    #[test]
    fn equal_adjacent_offsets_allowed() {
        // 단조 비감소 — equal 은 OK (empty glyph).
        let bytes: Vec<u8> = vec![0, 0, 0, 5, 0, 5, 0, 5];
        let loca = Loca::parse(&bytes, LocaFormat::Short, 3).expect("equal entries valid");
        assert_eq!(loca.offsets, vec![0, 10, 10, 10]);
    }
}
