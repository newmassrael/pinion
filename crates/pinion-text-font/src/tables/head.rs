//! R50.1.2 §5.37.1 — `head` table (font header).
//!
//! Microsoft OpenType 1.9.x spec, "head" chapter. 54 bytes fixed.

use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

/// font header — 54 byte fixed structure.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Head {
    pub major_version: u16,
    pub minor_version: u16,
    /// fixed 16.16 — raw i32. major = high 16 bits, minor = low 16 bits / 65536.
    pub font_revision_fixed: i32,
    pub checksum_adjustment: u32,
    /// must equal `0x5F0F_3CF5`.
    pub magic_number: u32,
    pub flags: u16,
    /// 16..=16384.
    pub units_per_em: u16,
    /// LONGDATETIME — seconds since 1904-01-01 00:00 UTC.
    pub created: i64,
    pub modified: i64,
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
    pub mac_style: u16,
    pub lowest_rec_ppem: u16,
    /// deprecated per OpenType 1.5+. value 0/±1/±2 모두 spec defined — strict reject 안 함.
    pub font_direction_hint: i16,
    /// 0 = short offsets (offset / 2), 1 = long offsets (raw).
    pub index_to_loc_format: i16,
    /// must be 0.
    pub glyph_data_format: i16,
}

const HEAD_TAG: [u8; 4] = *b"head";
const HEAD_MAGIC: u32 = 0x5F0F_3CF5;

impl Head {
    /// Parse the head table bytes.
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — fewer than 54 bytes.
    /// * [`ParseError::InvalidTableField`] — magic number 미스매치 / `units_per_em` 범위 위반 /
    ///   `index_to_loc_format` ∉ {0, 1} / `glyph_data_format` ≠ 0.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, HEAD_TAG);
        let major_version = r.read_u16()?;
        let minor_version = r.read_u16()?;
        let font_revision_fixed = r.read_i32()?;
        let checksum_adjustment = r.read_u32()?;
        let magic_number = r.read_u32()?;
        if magic_number != HEAD_MAGIC {
            return Err(ParseError::InvalidTableField {
                tag: HEAD_TAG,
                field: "magicNumber",
                value: FieldValue::from_u32(magic_number),
            });
        }
        let flags = r.read_u16()?;
        let units_per_em = r.read_u16()?;
        if !(16..=16384).contains(&units_per_em) {
            return Err(ParseError::InvalidTableField {
                tag: HEAD_TAG,
                field: "unitsPerEm",
                value: FieldValue::from_u16(units_per_em),
            });
        }
        let created = r.read_i64()?;
        let modified = r.read_i64()?;
        let x_min = r.read_i16()?;
        let y_min = r.read_i16()?;
        let x_max = r.read_i16()?;
        let y_max = r.read_i16()?;
        let mac_style = r.read_u16()?;
        let lowest_rec_ppem = r.read_u16()?;
        let font_direction_hint = r.read_i16()?;
        let index_to_loc_format = r.read_i16()?;
        if !(0..=1).contains(&index_to_loc_format) {
            return Err(ParseError::InvalidTableField {
                tag: HEAD_TAG,
                field: "indexToLocFormat",
                value: FieldValue::from_i16(index_to_loc_format),
            });
        }
        let glyph_data_format = r.read_i16()?;
        if glyph_data_format != 0 {
            return Err(ParseError::InvalidTableField {
                tag: HEAD_TAG,
                field: "glyphDataFormat",
                value: FieldValue::from_i16(glyph_data_format),
            });
        }

        Ok(Self {
            major_version,
            minor_version,
            font_revision_fixed,
            checksum_adjustment,
            magic_number,
            flags,
            units_per_em,
            created,
            modified,
            x_min,
            y_min,
            x_max,
            y_max,
            mac_style,
            lowest_rec_ppem,
            font_direction_hint,
            index_to_loc_format,
            glyph_data_format,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD_LEN: usize = 54;

    fn build_minimal_head() -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEAD_LEN);
        bytes.extend_from_slice(&1u16.to_be_bytes()); // major
        bytes.extend_from_slice(&0u16.to_be_bytes()); // minor
        bytes.extend_from_slice(&0x0001_0000i32.to_be_bytes()); // font_revision = 1.0
        bytes.extend_from_slice(&0u32.to_be_bytes()); // checksum_adjustment
        bytes.extend_from_slice(&HEAD_MAGIC.to_be_bytes()); // magic
        bytes.extend_from_slice(&0u16.to_be_bytes()); // flags
        bytes.extend_from_slice(&1000u16.to_be_bytes()); // units_per_em = 1000
        bytes.extend_from_slice(&0i64.to_be_bytes()); // created
        bytes.extend_from_slice(&0i64.to_be_bytes()); // modified
        bytes.extend_from_slice(&0i16.to_be_bytes()); // x_min
        bytes.extend_from_slice(&0i16.to_be_bytes()); // y_min
        bytes.extend_from_slice(&100i16.to_be_bytes()); // x_max
        bytes.extend_from_slice(&100i16.to_be_bytes()); // y_max
        bytes.extend_from_slice(&0u16.to_be_bytes()); // mac_style
        bytes.extend_from_slice(&7u16.to_be_bytes()); // lowest_rec_ppem
        bytes.extend_from_slice(&2i16.to_be_bytes()); // font_direction_hint
        bytes.extend_from_slice(&0i16.to_be_bytes()); // index_to_loc_format
        bytes.extend_from_slice(&0i16.to_be_bytes()); // glyph_data_format
        bytes
    }

    #[test]
    fn parse_minimal_valid_head() {
        let bytes = build_minimal_head();
        let head = Head::parse(&bytes).expect("valid head");
        assert_eq!(head.magic_number, HEAD_MAGIC);
        assert_eq!(head.units_per_em, 1000);
        assert_eq!(head.x_max, 100);
        assert_eq!(head.index_to_loc_format, 0);
    }

    #[test]
    fn reject_table_too_short() {
        let bytes = vec![0u8; 10];
        let err = Head::parse(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::TableTooShort {
                tag: HEAD_TAG,
                available: 10,
                ..
            }
        ));
    }

    #[test]
    fn reject_invalid_magic() {
        let mut bytes = build_minimal_head();
        bytes[12..16].copy_from_slice(&0xDEAD_BEEFu32.to_be_bytes());
        let err = Head::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: HEAD_TAG,
                field: "magicNumber",
                value: FieldValue::Unsigned(0xDEAD_BEEF),
            }
        );
    }

    #[test]
    fn reject_units_per_em_below_range() {
        let mut bytes = build_minimal_head();
        bytes[18..20].copy_from_slice(&8u16.to_be_bytes());
        let err = Head::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: HEAD_TAG,
                field: "unitsPerEm",
                value: FieldValue::Unsigned(8),
            }
        );
    }

    #[test]
    fn reject_units_per_em_above_range() {
        let mut bytes = build_minimal_head();
        bytes[18..20].copy_from_slice(&20000u16.to_be_bytes());
        let err = Head::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: HEAD_TAG,
                field: "unitsPerEm",
                value: FieldValue::Unsigned(20000),
            }
        );
    }

    #[test]
    fn reject_index_to_loc_format_out_of_range() {
        let mut bytes = build_minimal_head();
        bytes[50..52].copy_from_slice(&5i16.to_be_bytes());
        let err = Head::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: HEAD_TAG,
                field: "indexToLocFormat",
                value: FieldValue::Signed(5),
            }
        );
    }

    #[test]
    fn reject_nonzero_glyph_data_format() {
        let mut bytes = build_minimal_head();
        bytes[52..54].copy_from_slice(&1i16.to_be_bytes());
        let err = Head::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: HEAD_TAG,
                field: "glyphDataFormat",
                value: FieldValue::Signed(1),
            }
        );
    }

    #[test]
    fn accept_negative_index_to_loc_format_rejected_too() {
        let mut bytes = build_minimal_head();
        bytes[50..52].copy_from_slice(&(-1i16).to_be_bytes());
        let err = Head::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: HEAD_TAG,
                field: "indexToLocFormat",
                value: FieldValue::Signed(-1),
            }
        );
    }
}
