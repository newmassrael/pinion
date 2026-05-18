//! R50.1.6 §5.37.1 — `cmap` Format 0 (byte encoding table, Mac Roman).
//!
//! Microsoft OpenType 1.9.x spec, "cmap subtable Format 0". 가장 단순한 cmap
//! format — 262-byte fixed: header 6 byte + 256-byte glyphIdArray. 1-byte
//! codepoint (0..=255) 을 직접 glyph id 로 매핑. 주로 legacy Mac Roman
//! encoding 폰트에서 사용.
//!
//! Layout:
//!
//! ```text
//! format               uint16  (= 0)
//! length               uint16  (= 262 always)
//! language             uint16
//! glyphIdArray[256]    uint8 each
//! ```
//!
//! spec strict: length 가 정확히 262 가 아니면 reject (R50.1.2/1.3.1 strict
//! 정신 일관).

use super::CMAP_TAG;
use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

const FORMAT0_LENGTH: u16 = 262;
const FORMAT0_ARRAY_LEN: usize = 256;

/// Format 0 — 256-entry byte encoding table.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Format0 {
    pub language: u16,
    /// 256 byte array indexed by codepoint (0..=255). value = glyph id.
    /// `glyph_id_array[i] == 0` means codepoint `i` is unmapped (.notdef).
    pub glyph_id_array: [u8; FORMAT0_ARRAY_LEN],
}

impl Format0 {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, CMAP_TAG);
        let format = r.read_u16()?;
        debug_assert_eq!(format, 0);
        let length = r.read_u16()?;
        if length != FORMAT0_LENGTH {
            return Err(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format0/length-not-262",
                value: FieldValue::from_u16(length),
            });
        }
        let language = r.read_u16()?;
        let mut glyph_id_array = [0u8; FORMAT0_ARRAY_LEN];
        for slot in &mut glyph_id_array {
            *slot = r.read_u8()?;
        }
        Ok(Self {
            language,
            glyph_id_array,
        })
    }

    /// Map a codepoint to glyph id. Returns `None` if `codepoint > 255` or
    /// the entry is 0 (.notdef).
    #[must_use]
    pub fn glyph_id(&self, codepoint: u32) -> Option<u16> {
        let cp = u8::try_from(codepoint).ok()?;
        let glyph = self.glyph_id_array[usize::from(cp)];
        if glyph == 0 {
            None
        } else {
            Some(u16::from(glyph))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_format0(map_a_to_glyph: u8) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(usize::from(FORMAT0_LENGTH));
        bytes.extend_from_slice(&0u16.to_be_bytes()); // format = 0
        bytes.extend_from_slice(&FORMAT0_LENGTH.to_be_bytes()); // length = 262
        bytes.extend_from_slice(&0u16.to_be_bytes()); // language
        let mut array = [0u8; FORMAT0_ARRAY_LEN];
        array[b'A' as usize] = map_a_to_glyph;
        bytes.extend_from_slice(&array);
        debug_assert_eq!(bytes.len(), usize::from(FORMAT0_LENGTH));
        bytes
    }

    #[test]
    fn parse_minimal_format0() {
        let bytes = build_format0(42);
        let f0 = Format0::parse(&bytes).expect("valid format 0");
        assert_eq!(f0.language, 0);
        assert_eq!(f0.glyph_id(u32::from(b'A')), Some(42));
        assert_eq!(f0.glyph_id(u32::from(b'B')), None); // unmapped
    }

    #[test]
    fn glyph_id_above_255_returns_none() {
        let bytes = build_format0(42);
        let f0 = Format0::parse(&bytes).expect("valid format 0");
        assert_eq!(f0.glyph_id(256), None);
        assert_eq!(f0.glyph_id(0xFFFF), None);
    }

    #[test]
    fn glyph_id_zero_is_unmapped() {
        let bytes = build_format0(0);
        let f0 = Format0::parse(&bytes).expect("valid format 0");
        assert_eq!(f0.glyph_id(u32::from(b'A')), None); // 0 = .notdef
    }

    #[test]
    fn reject_length_not_262() {
        let mut bytes = build_format0(42);
        bytes[2..4].copy_from_slice(&300u16.to_be_bytes()); // length = 300
        let err = Format0::parse(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "format0/length-not-262",
                ..
            } if tag == CMAP_TAG
        ));
    }

    #[test]
    fn reject_too_short() {
        let bytes = vec![0u8; 10];
        let err = Format0::parse(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "format0/length-not-262",
                ..
            } | ParseError::TableTooShort {
                tag,
                ..
            } if tag == CMAP_TAG
        ));
    }
}
