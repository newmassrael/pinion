//! R50.1.2 §5.37.1 — `hmtx` table (horizontal metrics).
//!
//! Microsoft OpenType 1.9.x spec, "hmtx" chapter. Variable size depending
//! on `number_of_h_metrics` (from hhea) + `num_glyphs` (from maxp).
//!
//! Layout:
//!
//! ```text
//! longHorMetric[number_of_h_metrics]   — 4 bytes each
//!   advanceWidth     (uint16)
//!   lsb              (int16)
//!
//! leftSideBearing[num_glyphs - number_of_h_metrics]   — 2 bytes each (int16)
//! ```
//!
//! 마지막 `longHorMetric` 의 `advance_width` 는 tail glyph 들이 공유 (monospace
//! optimization). lsb 는 tail 영역에서 별도 array.

use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct LongHorMetric {
    pub advance_width: u16,
    pub lsb: i16,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Hmtx {
    pub long_metrics: Vec<LongHorMetric>,
    /// `num_glyphs - number_of_h_metrics` 길이.
    pub tail_lsbs: Vec<i16>,
}

const HMTX_TAG: [u8; 4] = *b"hmtx";

impl Hmtx {
    /// Parse the hmtx table bytes.
    ///
    /// `number_of_h_metrics` 는 hhea, `num_glyphs` 는 maxp 에서 가져옴.
    ///
    /// # Errors
    ///
    /// * [`ParseError::InvalidTableField`] — `number_of_h_metrics == 0`
    ///   또는 `number_of_h_metrics > num_glyphs`.
    /// * [`ParseError::TableTooShort`] — bytes 가 expected size 보다 짧음.
    pub fn parse(
        bytes: &[u8],
        number_of_h_metrics: u16,
        num_glyphs: u16,
    ) -> Result<Self, ParseError> {
        if number_of_h_metrics == 0 {
            return Err(ParseError::InvalidTableField {
                tag: HMTX_TAG,
                field: "numberOfHMetrics",
                value: FieldValue::Unsigned(0),
            });
        }
        if number_of_h_metrics > num_glyphs {
            return Err(ParseError::InvalidTableField {
                tag: HMTX_TAG,
                field: "numberOfHMetrics",
                value: FieldValue::from_u16(number_of_h_metrics),
            });
        }

        let mut r = Reader::new(bytes, HMTX_TAG);
        let mut long_metrics = Vec::with_capacity(usize::from(number_of_h_metrics));
        for _ in 0..number_of_h_metrics {
            let advance_width = r.read_u16()?;
            let lsb = r.read_i16()?;
            long_metrics.push(LongHorMetric { advance_width, lsb });
        }
        let tail_count = usize::from(num_glyphs) - usize::from(number_of_h_metrics);
        let mut tail_lsbs = Vec::with_capacity(tail_count);
        for _ in 0..tail_count {
            tail_lsbs.push(r.read_i16()?);
        }

        Ok(Self {
            long_metrics,
            tail_lsbs,
        })
    }

    /// `glyph_id` 의 advance width. `num_glyphs` 범위 밖이면 `None`.
    ///
    /// `glyph_id ≥ number_of_h_metrics` 인 tail 영역은 마지막 `longHorMetric` 의 advance 사용.
    #[must_use]
    pub fn advance_width(&self, glyph_id: u16) -> Option<u16> {
        let idx = usize::from(glyph_id);
        let n = self.long_metrics.len();
        let total = n + self.tail_lsbs.len();
        if idx >= total {
            return None;
        }
        if idx < n {
            Some(self.long_metrics[idx].advance_width)
        } else {
            // tail glyphs share the last advance_width (monospace optimization).
            Some(self.long_metrics[n - 1].advance_width)
        }
    }

    /// `glyph_id` 의 left side bearing.
    #[must_use]
    pub fn left_side_bearing(&self, glyph_id: u16) -> Option<i16> {
        let idx = usize::from(glyph_id);
        let n = self.long_metrics.len();
        let total = n + self.tail_lsbs.len();
        if idx >= total {
            return None;
        }
        if idx < n {
            Some(self.long_metrics[idx].lsb)
        } else {
            Some(self.tail_lsbs[idx - n])
        }
    }

    /// 총 glyph 개수 (= `number_of_h_metrics` + `tail_count`).
    #[must_use]
    pub fn num_glyphs(&self) -> usize {
        self.long_metrics.len() + self.tail_lsbs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_hmtx(metrics: &[(u16, i16)], tail_lsbs: &[i16]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (advance, lsb) in metrics {
            bytes.extend_from_slice(&advance.to_be_bytes());
            bytes.extend_from_slice(&lsb.to_be_bytes());
        }
        for lsb in tail_lsbs {
            bytes.extend_from_slice(&lsb.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn parse_pure_long_metrics() {
        // num_glyphs == number_of_h_metrics == 3 — no tail.
        let bytes = build_hmtx(&[(500, 10), (600, -20), (700, 5)], &[]);
        let hmtx = Hmtx::parse(&bytes, 3, 3).expect("valid hmtx");
        assert_eq!(hmtx.long_metrics.len(), 3);
        assert!(hmtx.tail_lsbs.is_empty());
        assert_eq!(hmtx.advance_width(0), Some(500));
        assert_eq!(hmtx.advance_width(2), Some(700));
        assert_eq!(hmtx.left_side_bearing(1), Some(-20));
    }

    #[test]
    fn monospace_optimization_repeats_last_advance() {
        // number_of_h_metrics=2, num_glyphs=5 — tail 3 glyphs share last advance.
        let bytes = build_hmtx(&[(500, 10), (600, -20)], &[15, 25, 35]);
        let hmtx = Hmtx::parse(&bytes, 2, 5).expect("valid hmtx");
        assert_eq!(hmtx.advance_width(0), Some(500));
        assert_eq!(hmtx.advance_width(1), Some(600));
        assert_eq!(hmtx.advance_width(2), Some(600)); // tail share
        assert_eq!(hmtx.advance_width(3), Some(600));
        assert_eq!(hmtx.advance_width(4), Some(600));
        assert_eq!(hmtx.left_side_bearing(2), Some(15));
        assert_eq!(hmtx.left_side_bearing(3), Some(25));
        assert_eq!(hmtx.left_side_bearing(4), Some(35));
    }

    #[test]
    fn out_of_range_glyph_returns_none() {
        let bytes = build_hmtx(&[(500, 10)], &[]);
        let hmtx = Hmtx::parse(&bytes, 1, 1).expect("valid hmtx");
        assert_eq!(hmtx.advance_width(1), None);
        assert_eq!(hmtx.left_side_bearing(99), None);
    }

    #[test]
    fn reject_zero_number_of_h_metrics() {
        let err = Hmtx::parse(&[], 0, 10).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: HMTX_TAG,
                field: "numberOfHMetrics",
                value: FieldValue::Unsigned(0),
            }
        );
    }

    #[test]
    fn reject_h_metrics_greater_than_num_glyphs() {
        let err = Hmtx::parse(&[0u8; 100], 10, 5).unwrap_err();
        assert_eq!(
            err,
            ParseError::InvalidTableField {
                tag: HMTX_TAG,
                field: "numberOfHMetrics",
                value: FieldValue::Unsigned(10),
            }
        );
    }

    #[test]
    fn reject_table_too_short() {
        let err = Hmtx::parse(&[0u8; 3], 1, 1).unwrap_err();
        assert!(matches!(
            err,
            ParseError::TableTooShort {
                tag: HMTX_TAG,
                available: 3,
                ..
            }
        ));
    }
}
