//! R50.1.2 §5.37.1 — `OS/2` table (OS/2 + Windows metrics).
//!
//! Microsoft OpenType 1.9.x spec, "OS/2" chapter. Multi-version:
//!
//! | Version | Min bytes | Extras |
//! |---|---|---|
//! | 0 | 78 | none — basic typographic + Windows metrics |
//! | 1 | 86 | + `ulCodePageRange1/2` (8 bytes) |
//! | 2 | 96 | + `sxHeight`, `sCapHeight`, `usDefaultChar`, `usBreakChar`, `usMaxContext` (10 bytes) |
//! | 3 | 96 | same fields as v2 (`fsSelection` bit semantics differ) |
//! | 4 | 96 | same fields as v2 (`fsSelection` bit semantics differ) |
//! | 5 | 100 | + `usLowerOpticalPointSize`, `usUpperOpticalPointSize` (4 bytes) |

use crate::error::ParseError;
use crate::reader::Reader;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Os2 {
    pub version: u16,
    pub x_avg_char_width: i16,
    pub us_weight_class: u16,
    pub us_width_class: u16,
    pub fs_type: u16,
    pub y_subscript_x_size: i16,
    pub y_subscript_y_size: i16,
    pub y_subscript_x_offset: i16,
    pub y_subscript_y_offset: i16,
    pub y_superscript_x_size: i16,
    pub y_superscript_y_size: i16,
    pub y_superscript_x_offset: i16,
    pub y_superscript_y_offset: i16,
    pub y_strikeout_size: i16,
    pub y_strikeout_position: i16,
    pub s_family_class: i16,
    pub panose: [u8; 10],
    pub ul_unicode_range1: u32,
    pub ul_unicode_range2: u32,
    pub ul_unicode_range3: u32,
    pub ul_unicode_range4: u32,
    pub ach_vend_id: [u8; 4],
    pub fs_selection: u16,
    pub us_first_char_index: u16,
    pub us_last_char_index: u16,
    pub s_typo_ascender: i16,
    pub s_typo_descender: i16,
    pub s_typo_line_gap: i16,
    pub us_win_ascent: u16,
    pub us_win_descent: u16,

    /// v1+ extras (None for v0).
    pub v1_extras: Option<Os2V1Extras>,
    /// v2/v3/v4 extras (None for v0/v1).
    pub v2_extras: Option<Os2V2Extras>,
    /// v5+ extras (None for v0..=v4).
    pub v5_extras: Option<Os2V5Extras>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Os2V1Extras {
    pub ul_code_page_range1: u32,
    pub ul_code_page_range2: u32,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Os2V2Extras {
    pub sx_height: i16,
    pub s_cap_height: i16,
    pub us_default_char: u16,
    pub us_break_char: u16,
    pub us_max_context: u16,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Os2V5Extras {
    pub us_lower_optical_point_size: u16,
    pub us_upper_optical_point_size: u16,
}

const OS2_TAG: [u8; 4] = *b"OS/2";
const OS2_MAX_VERSION: u16 = 5;

impl Os2 {
    /// Parse the OS/2 table bytes.
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — bytes shorter than version-required minimum.
    /// * [`ParseError::UnsupportedTableVersion`] — version > 5.
    // OS/2 spec 의 sub/super/strikeout field 들이 spec 정의대로 매우 유사한
    // 이름을 가짐 (y_subscript_x_size vs y_subscript_y_size 등). spec canonical
    // naming 보존을 위해 clippy::similar_names allow.
    #[allow(clippy::similar_names)]
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, OS2_TAG);
        let version = r.read_u16()?;
        if version > OS2_MAX_VERSION {
            return Err(ParseError::UnsupportedTableVersion {
                tag: OS2_TAG,
                major: version,
                minor: 0,
            });
        }

        let x_avg_char_width = r.read_i16()?;
        let us_weight_class = r.read_u16()?;
        let us_width_class = r.read_u16()?;
        let fs_type = r.read_u16()?;
        let y_subscript_x_size = r.read_i16()?;
        let y_subscript_y_size = r.read_i16()?;
        let y_subscript_x_offset = r.read_i16()?;
        let y_subscript_y_offset = r.read_i16()?;
        let y_superscript_x_size = r.read_i16()?;
        let y_superscript_y_size = r.read_i16()?;
        let y_superscript_x_offset = r.read_i16()?;
        let y_superscript_y_offset = r.read_i16()?;
        let y_strikeout_size = r.read_i16()?;
        let y_strikeout_position = r.read_i16()?;
        let s_family_class = r.read_i16()?;
        let panose: [u8; 10] = r.read_bytes()?;
        let ul_unicode_range1 = r.read_u32()?;
        let ul_unicode_range2 = r.read_u32()?;
        let ul_unicode_range3 = r.read_u32()?;
        let ul_unicode_range4 = r.read_u32()?;
        let ach_vend_id: [u8; 4] = r.read_bytes()?;
        let fs_selection = r.read_u16()?;
        let us_first_char_index = r.read_u16()?;
        let us_last_char_index = r.read_u16()?;
        let s_typo_ascender = r.read_i16()?;
        let s_typo_descender = r.read_i16()?;
        let s_typo_line_gap = r.read_i16()?;
        let us_win_ascent = r.read_u16()?;
        let us_win_descent = r.read_u16()?;

        let v1_extras = if version >= 1 {
            Some(Os2V1Extras {
                ul_code_page_range1: r.read_u32()?,
                ul_code_page_range2: r.read_u32()?,
            })
        } else {
            None
        };

        let v2_extras = if version >= 2 {
            Some(Os2V2Extras {
                sx_height: r.read_i16()?,
                s_cap_height: r.read_i16()?,
                us_default_char: r.read_u16()?,
                us_break_char: r.read_u16()?,
                us_max_context: r.read_u16()?,
            })
        } else {
            None
        };

        let v5_extras = if version >= 5 {
            Some(Os2V5Extras {
                us_lower_optical_point_size: r.read_u16()?,
                us_upper_optical_point_size: r.read_u16()?,
            })
        } else {
            None
        };

        Ok(Self {
            version,
            x_avg_char_width,
            us_weight_class,
            us_width_class,
            fs_type,
            y_subscript_x_size,
            y_subscript_y_size,
            y_subscript_x_offset,
            y_subscript_y_offset,
            y_superscript_x_size,
            y_superscript_y_size,
            y_superscript_x_offset,
            y_superscript_y_offset,
            y_strikeout_size,
            y_strikeout_position,
            s_family_class,
            panose,
            ul_unicode_range1,
            ul_unicode_range2,
            ul_unicode_range3,
            ul_unicode_range4,
            ach_vend_id,
            fs_selection,
            us_first_char_index,
            us_last_char_index,
            s_typo_ascender,
            s_typo_descender,
            s_typo_line_gap,
            us_win_ascent,
            us_win_descent,
            v1_extras,
            v2_extras,
            v5_extras,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_v0_os2() -> Vec<u8> {
        let mut bytes = Vec::with_capacity(78);
        bytes.extend_from_slice(&0u16.to_be_bytes()); // version
        bytes.extend_from_slice(&500i16.to_be_bytes()); // x_avg_char_width
        bytes.extend_from_slice(&400u16.to_be_bytes()); // us_weight_class (Regular)
        bytes.extend_from_slice(&5u16.to_be_bytes()); // us_width_class (Medium)
        bytes.extend_from_slice(&0u16.to_be_bytes()); // fs_type
        for _ in 0..10 {
            bytes.extend_from_slice(&0i16.to_be_bytes()); // sub/super/strike
        }
        bytes.extend_from_slice(&0i16.to_be_bytes()); // s_family_class
        bytes.extend_from_slice(&[0u8; 10]); // panose
        bytes.extend_from_slice(&0u32.to_be_bytes()); // unicode_range1
        bytes.extend_from_slice(&0u32.to_be_bytes()); // unicode_range2
        bytes.extend_from_slice(&0u32.to_be_bytes()); // unicode_range3
        bytes.extend_from_slice(&0u32.to_be_bytes()); // unicode_range4
        bytes.extend_from_slice(b"ABCD"); // ach_vend_id
        bytes.extend_from_slice(&0u16.to_be_bytes()); // fs_selection
        bytes.extend_from_slice(&0x20u16.to_be_bytes()); // first_char_index (space)
        bytes.extend_from_slice(&0xFFFFu16.to_be_bytes()); // last_char_index
        bytes.extend_from_slice(&800i16.to_be_bytes()); // typo_ascender
        bytes.extend_from_slice(&(-200i16).to_be_bytes()); // typo_descender
        bytes.extend_from_slice(&100i16.to_be_bytes()); // typo_line_gap
        bytes.extend_from_slice(&1000u16.to_be_bytes()); // win_ascent
        bytes.extend_from_slice(&300u16.to_be_bytes()); // win_descent
        assert_eq!(bytes.len(), 78);
        bytes
    }

    #[test]
    fn parse_v0_minimal() {
        let bytes = build_v0_os2();
        let os2 = Os2::parse(&bytes).expect("valid v0 OS/2");
        assert_eq!(os2.version, 0);
        assert_eq!(os2.us_weight_class, 400);
        assert_eq!(&os2.ach_vend_id, b"ABCD");
        assert_eq!(os2.us_win_ascent, 1000);
        assert!(os2.v1_extras.is_none());
        assert!(os2.v2_extras.is_none());
        assert!(os2.v5_extras.is_none());
    }

    #[test]
    fn parse_v2_extras() {
        let mut bytes = build_v0_os2();
        bytes[0..2].copy_from_slice(&2u16.to_be_bytes()); // version = 2
        // v1 extras (8 bytes)
        bytes.extend_from_slice(&0xDEAD_BEEFu32.to_be_bytes());
        bytes.extend_from_slice(&0xCAFE_F00Du32.to_be_bytes());
        // v2 extras (10 bytes)
        bytes.extend_from_slice(&500i16.to_be_bytes()); // sx_height
        bytes.extend_from_slice(&700i16.to_be_bytes()); // s_cap_height
        bytes.extend_from_slice(&0u16.to_be_bytes()); // us_default_char
        bytes.extend_from_slice(&0x20u16.to_be_bytes()); // us_break_char
        bytes.extend_from_slice(&1u16.to_be_bytes()); // us_max_context

        let os2 = Os2::parse(&bytes).expect("valid v2 OS/2");
        assert_eq!(os2.version, 2);
        assert_eq!(
            os2.v1_extras,
            Some(Os2V1Extras {
                ul_code_page_range1: 0xDEAD_BEEF,
                ul_code_page_range2: 0xCAFE_F00D,
            })
        );
        let v2 = os2.v2_extras.expect("v2 extras present");
        assert_eq!(v2.sx_height, 500);
        assert_eq!(v2.s_cap_height, 700);
        assert_eq!(v2.us_break_char, 0x20);
    }

    #[test]
    fn parse_v5_extras() {
        let mut bytes = build_v0_os2();
        bytes[0..2].copy_from_slice(&5u16.to_be_bytes()); // version = 5
        // v1 + v2 + v5 extras = 8 + 10 + 4 = 22 bytes
        bytes.resize(bytes.len() + 22, 0);
        // v5 fields at offset 96..100 (last 4 bytes)
        let len = bytes.len();
        bytes[len - 4..len - 2].copy_from_slice(&8u16.to_be_bytes()); // lower_optical
        bytes[len - 2..].copy_from_slice(&144u16.to_be_bytes()); // upper_optical

        let os2 = Os2::parse(&bytes).expect("valid v5 OS/2");
        assert_eq!(os2.version, 5);
        let v5 = os2.v5_extras.expect("v5 extras present");
        assert_eq!(v5.us_lower_optical_point_size, 8);
        assert_eq!(v5.us_upper_optical_point_size, 144);
    }

    #[test]
    fn reject_v0_too_short() {
        let mut bytes = build_v0_os2();
        bytes.truncate(70);
        let err = Os2::parse(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::TableTooShort {
                tag: OS2_TAG,
                available: 70,
                ..
            }
        ));
    }

    #[test]
    fn reject_unsupported_version() {
        let mut bytes = build_v0_os2();
        bytes[0..2].copy_from_slice(&6u16.to_be_bytes());
        let err = Os2::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::UnsupportedTableVersion {
                tag: OS2_TAG,
                major: 6,
                minor: 0,
            }
        );
    }

    #[test]
    fn reject_v2_with_insufficient_bytes() {
        let mut bytes = build_v0_os2();
        bytes[0..2].copy_from_slice(&2u16.to_be_bytes());
        // v0 base (78) + 4 bytes of v1 extras only (instead of 8) — short of v1 minimum.
        bytes.extend_from_slice(&0u32.to_be_bytes());
        let err = Os2::parse(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::TableTooShort { tag: OS2_TAG, .. }
        ));
    }
}
