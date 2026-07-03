//! R50.1.2 §5.37.1 — `hhea` table (horizontal header).
//!
//! Microsoft OpenType 1.9.x spec, "hhea" chapter. 36 bytes fixed.

use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Hhea {
    pub major_version: u16,
    pub minor_version: u16,
    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
    pub advance_width_max: u16,
    pub min_left_side_bearing: i16,
    pub min_right_side_bearing: i16,
    pub x_max_extent: i16,
    pub caret_slope_rise: i16,
    pub caret_slope_run: i16,
    pub caret_offset: i16,
    /// must be 0 — TrueType / OpenType.
    pub metric_data_format: i16,
    /// hmtx 의 `longHorMetric` 개수. `1..=num_glyphs`.
    pub number_of_h_metrics: u16,
}

const HHEA_TAG: [u8; 4] = *b"hhea";

impl Hhea {
    /// Parse the hhea table bytes.
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — 36 바이트 미만.
    /// * [`ParseError::UnsupportedTableVersion`] — major != 1.
    /// * [`ParseError::InvalidTableField`] — reserved `int16[4]` 가 모두 0 이 아님 /
    ///   `metric_data_format` != 0.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, HHEA_TAG);
        let major_version = r.read_u16()?;
        let minor_version = r.read_u16()?;
        if major_version != 1 {
            return Err(ParseError::UnsupportedTableVersion {
                tag: HHEA_TAG,
                major: major_version,
                minor: minor_version,
            });
        }
        let ascender = r.read_i16()?;
        let descender = r.read_i16()?;
        let line_gap = r.read_i16()?;
        let advance_width_max = r.read_u16()?;
        let min_left_side_bearing = r.read_i16()?;
        let min_right_side_bearing = r.read_i16()?;
        let x_max_extent = r.read_i16()?;
        let caret_slope_rise = r.read_i16()?;
        let caret_slope_run = r.read_i16()?;
        let caret_offset = r.read_i16()?;
        // reserved int16[4] — spec mandates set to 0. strict reject if violated.
        for i in 0..4 {
            let reserved = r.read_i16()?;
            if reserved != 0 {
                return Err(ParseError::InvalidTableField {
                    tag: HHEA_TAG,
                    field: match i {
                        0 => "reserved[0]",
                        1 => "reserved[1]",
                        2 => "reserved[2]",
                        _ => "reserved[3]",
                    },
                    value: FieldValue::from_i16(reserved),
                });
            }
        }
        let metric_data_format = r.read_i16()?;
        if metric_data_format != 0 {
            return Err(ParseError::InvalidTableField {
                tag: HHEA_TAG,
                field: "metricDataFormat",
                value: FieldValue::from_i16(metric_data_format),
            });
        }
        let number_of_h_metrics = r.read_u16()?;

        Ok(Self {
            major_version,
            minor_version,
            ascender,
            descender,
            line_gap,
            advance_width_max,
            min_left_side_bearing,
            min_right_side_bearing,
            x_max_extent,
            caret_slope_rise,
            caret_slope_run,
            caret_offset,
            metric_data_format,
            number_of_h_metrics,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_minimal_hhea() -> Vec<u8> {
        let mut bytes = Vec::with_capacity(36);
        bytes.extend_from_slice(&1u16.to_be_bytes()); // major
        bytes.extend_from_slice(&0u16.to_be_bytes()); // minor
        bytes.extend_from_slice(&800i16.to_be_bytes()); // ascender
        bytes.extend_from_slice(&(-200i16).to_be_bytes()); // descender
        bytes.extend_from_slice(&100i16.to_be_bytes()); // line_gap
        bytes.extend_from_slice(&1500u16.to_be_bytes()); // advance_width_max
        bytes.extend_from_slice(&(-50i16).to_be_bytes()); // min_lsb
        bytes.extend_from_slice(&(-30i16).to_be_bytes()); // min_rsb
        bytes.extend_from_slice(&1200i16.to_be_bytes()); // x_max_extent
        bytes.extend_from_slice(&1i16.to_be_bytes()); // caret_slope_rise
        bytes.extend_from_slice(&0i16.to_be_bytes()); // caret_slope_run
        bytes.extend_from_slice(&0i16.to_be_bytes()); // caret_offset
        for _ in 0..4 {
            bytes.extend_from_slice(&0i16.to_be_bytes()); // reserved
        }
        bytes.extend_from_slice(&0i16.to_be_bytes()); // metric_data_format
        bytes.extend_from_slice(&255u16.to_be_bytes()); // number_of_h_metrics
        bytes
    }

    #[test]
    fn parse_minimal_valid_hhea() {
        let bytes = build_minimal_hhea();
        let hhea = Hhea::parse(&bytes).expect("valid hhea");
        assert_eq!(hhea.major_version, 1);
        assert_eq!(hhea.ascender, 800);
        assert_eq!(hhea.descender, -200);
        assert_eq!(hhea.line_gap, 100);
        assert_eq!(hhea.number_of_h_metrics, 255);
    }

    #[test]
    fn reject_table_too_short() {
        // major_version=1 로 setup해서 version check 통과 후 short read 로 fail 시도.
        let mut bytes = vec![0u8; 20];
        bytes[0..2].copy_from_slice(&1u16.to_be_bytes());
        let err = Hhea::parse(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::TableTooShort {
                tag: HHEA_TAG,
                available: 20,
                ..
            }
        ));
    }

    #[test]
    fn reject_unsupported_major_version() {
        let mut bytes = build_minimal_hhea();
        bytes[0..2].copy_from_slice(&2u16.to_be_bytes());
        let err = Hhea::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::UnsupportedTableVersion {
                tag: HHEA_TAG,
                major: 2,
                minor: 0,
            }
        );
    }

    #[test]
    fn reject_nonzero_metric_data_format() {
        let mut bytes = build_minimal_hhea();
        bytes[32..34].copy_from_slice(&1i16.to_be_bytes());
        let err = Hhea::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: HHEA_TAG,
                field: "metricDataFormat",
                value: FieldValue::Signed(1),
            }
        );
    }

    #[test]
    fn reject_nonzero_reserved() {
        let mut bytes = build_minimal_hhea();
        // reserved[0] offset = 24 (after caret_offset at 22)
        bytes[24..26].copy_from_slice(&1i16.to_be_bytes());
        let err = Hhea::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: HHEA_TAG,
                field: "reserved[0]",
                value: FieldValue::Signed(1),
            }
        );
    }

    #[test]
    fn reject_nonzero_reserved_3() {
        let mut bytes = build_minimal_hhea();
        // reserved[3] offset = 30
        bytes[30..32].copy_from_slice(&(-1i16).to_be_bytes());
        let err = Hhea::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: HHEA_TAG,
                field: "reserved[3]",
                value: FieldValue::Signed(-1),
            }
        );
    }
}
