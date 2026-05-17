//! R50.1.2 §5.37.1 — `maxp` table (maximum profile).
//!
//! Microsoft OpenType 1.9.x spec, "maxp" chapter.
//!
//! Two versions:
//!
//! * **v0.5** (`0x00005000`) — 6 bytes, CFF outlines (OpenType/PS). Only
//!   `version` + `numGlyphs`.
//! * **v1.0** (`0x00010000`) — 32 bytes, TrueType outlines. Adds 13 max
//!   counts (points, contours, function/instruction defs, etc.).

use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Maxp {
    pub version_fixed: i32,
    /// must be ≥ 1.
    pub num_glyphs: u16,
    /// v1.0 extension. v0.5 일 때 None.
    pub v1_extras: Option<MaxpV1Extras>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct MaxpV1Extras {
    pub max_points: u16,
    pub max_contours: u16,
    pub max_composite_points: u16,
    pub max_composite_contours: u16,
    pub max_zones: u16,
    pub max_twilight_points: u16,
    pub max_storage: u16,
    pub max_function_defs: u16,
    pub max_instruction_defs: u16,
    pub max_stack_elements: u16,
    pub max_size_of_instructions: u16,
    pub max_component_elements: u16,
    pub max_component_depth: u16,
}

const MAXP_TAG: [u8; 4] = *b"maxp";
const MAXP_VERSION_05: i32 = 0x0000_5000;
const MAXP_VERSION_10: i32 = 0x0001_0000;

impl Maxp {
    /// Parse the maxp table bytes.
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — v0.5 일 때 6 바이트 미만, v1.0 일 때 32 바이트 미만.
    /// * [`ParseError::UnsupportedTableVersion`] — version 이 v0.5/v1.0 외.
    /// * [`ParseError::InvalidTableField`] — numGlyphs = 0 (font 에 glyph 가 없는 건 invalid).
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, MAXP_TAG);
        let version_fixed = r.read_i32()?;
        let num_glyphs = r.read_u16()?;
        if num_glyphs == 0 {
            return Err(ParseError::InvalidTableField {
                tag: MAXP_TAG,
                field: "numGlyphs",
                value: FieldValue::Unsigned(0),
            });
        }

        let v1_extras = match version_fixed {
            MAXP_VERSION_05 => None,
            MAXP_VERSION_10 => Some(MaxpV1Extras {
                max_points: r.read_u16()?,
                max_contours: r.read_u16()?,
                max_composite_points: r.read_u16()?,
                max_composite_contours: r.read_u16()?,
                max_zones: r.read_u16()?,
                max_twilight_points: r.read_u16()?,
                max_storage: r.read_u16()?,
                max_function_defs: r.read_u16()?,
                max_instruction_defs: r.read_u16()?,
                max_stack_elements: r.read_u16()?,
                max_size_of_instructions: r.read_u16()?,
                max_component_elements: r.read_u16()?,
                max_component_depth: r.read_u16()?,
            }),
            _ => {
                #[allow(clippy::cast_sign_loss)]
                let major = (version_fixed >> 16) as u16;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let minor = (version_fixed & 0xFFFF) as u16;
                return Err(ParseError::UnsupportedTableVersion {
                    tag: MAXP_TAG,
                    major,
                    minor,
                });
            }
        };

        Ok(Self {
            version_fixed,
            num_glyphs,
            v1_extras,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_v10_maxp(num_glyphs: u16) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(32);
        bytes.extend_from_slice(&MAXP_VERSION_10.to_be_bytes());
        bytes.extend_from_slice(&num_glyphs.to_be_bytes());
        for _ in 0..13 {
            bytes.extend_from_slice(&0u16.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn parse_v10_minimal() {
        let bytes = build_v10_maxp(1234);
        let maxp = Maxp::parse(&bytes).expect("valid maxp");
        assert_eq!(maxp.version_fixed, MAXP_VERSION_10);
        assert_eq!(maxp.num_glyphs, 1234);
        assert!(maxp.v1_extras.is_some());
    }

    #[test]
    fn parse_v05_minimal() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAXP_VERSION_05.to_be_bytes());
        bytes.extend_from_slice(&500u16.to_be_bytes());
        let maxp = Maxp::parse(&bytes).expect("valid v0.5 maxp");
        assert_eq!(maxp.version_fixed, MAXP_VERSION_05);
        assert_eq!(maxp.num_glyphs, 500);
        assert!(maxp.v1_extras.is_none());
    }

    #[test]
    fn reject_v10_too_short() {
        let bytes = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x05];
        let err = Maxp::parse(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::TableTooShort {
                tag: MAXP_TAG,
                available: 6,
                ..
            }
        ));
    }

    #[test]
    fn reject_unsupported_version() {
        let mut bytes = vec![0u8; 32];
        bytes[0..4].copy_from_slice(&0x0002_0000i32.to_be_bytes());
        bytes[4..6].copy_from_slice(&100u16.to_be_bytes()); // valid num_glyphs
        let err = Maxp::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::UnsupportedTableVersion {
                tag: MAXP_TAG,
                major: 2,
                minor: 0,
            }
        );
    }

    #[test]
    fn reject_zero_num_glyphs() {
        let bytes = build_v10_maxp(0);
        let err = Maxp::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: MAXP_TAG,
                field: "numGlyphs",
                value: FieldValue::Unsigned(0),
            }
        );
    }
}
