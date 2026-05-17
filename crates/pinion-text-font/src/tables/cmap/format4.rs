//! R50.1.3 §5.37.1 — `cmap` Format 4 (segment mapping for BMP).
//!
//! Microsoft OpenType 1.9.x spec, "cmap subtable Format 4". 16-bit
//! codepoint segments with delta or indirect-via-glyphIdArray lookup.

use super::CMAP_TAG;
use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Format4 {
    pub language: u16,
    pub seg_count: u16,
    pub end_code: Vec<u16>,
    pub start_code: Vec<u16>,
    pub id_delta: Vec<i16>,
    pub id_range_offset: Vec<u16>,
    pub glyph_id_array: Vec<u16>,
}

impl Format4 {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, CMAP_TAG);
        let format = r.read_u16()?;
        debug_assert_eq!(format, 4);
        let length = usize::from(r.read_u16()?);
        let language = r.read_u16()?;
        let seg_count_x2 = r.read_u16()?;
        if seg_count_x2 == 0 || (seg_count_x2 & 1) != 0 {
            return Err(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format4/segCountX2",
                value: FieldValue::from_u16(seg_count_x2),
            });
        }
        let seg_count = seg_count_x2 / 2;
        let search_range = r.read_u16()?;
        let entry_selector = r.read_u16()?;
        let range_shift = r.read_u16()?;
        // spec 정의 (Microsoft OpenType "cmap" format 4):
        //   searchRange = 2 * (2^floor(log2(segCount)))
        //   entrySelector = floor(log2(segCount))
        //   rangeShift = 2*segCount - searchRange
        // black-box tolerance 는 §2 invariant #2 (introspect) 위반 — strict reject.
        verify_search_params(seg_count, search_range, entry_selector, range_shift)?;

        let mut end_code = Vec::with_capacity(usize::from(seg_count));
        for _ in 0..seg_count {
            end_code.push(r.read_u16()?);
        }
        if end_code.last() != Some(&0xFFFF) {
            return Err(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format4/endCode[last]",
                value: FieldValue::from_u16(end_code.last().copied().unwrap_or(0)),
            });
        }
        // spec: endCode array must be sorted ascending — binary search 의 전제.
        for window in end_code.windows(2) {
            if window[0] > window[1] {
                return Err(ParseError::InvalidTableField {
                    tag: CMAP_TAG,
                    field: "format4/endCode/not-ascending",
                    value: FieldValue::from_u16(window[0]),
                });
            }
        }
        let reserved_pad = r.read_u16()?;
        if reserved_pad != 0 {
            return Err(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format4/reservedPad",
                value: FieldValue::from_u16(reserved_pad),
            });
        }
        let mut start_code = Vec::with_capacity(usize::from(seg_count));
        for _ in 0..seg_count {
            start_code.push(r.read_u16()?);
        }
        for (&s, &e) in start_code.iter().zip(end_code.iter()) {
            if s > e {
                return Err(ParseError::InvalidTableField {
                    tag: CMAP_TAG,
                    field: "format4/startCode>endCode",
                    value: FieldValue::Unsigned((u64::from(s) << 16) | u64::from(e)),
                });
            }
        }
        let mut id_delta = Vec::with_capacity(usize::from(seg_count));
        for _ in 0..seg_count {
            id_delta.push(r.read_i16()?);
        }
        let mut id_range_offset = Vec::with_capacity(usize::from(seg_count));
        for _ in 0..seg_count {
            id_range_offset.push(r.read_u16()?);
        }

        // glyphIdArray fills remaining 2-byte words of the subtable.
        let header_end = r.position();
        if header_end > length {
            return Err(ParseError::TableTooShort {
                tag: CMAP_TAG,
                needed: header_end,
                available: length,
            });
        }
        let glyph_array_bytes = length - header_end;
        if glyph_array_bytes % 2 != 0 {
            return Err(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format4/length-not-aligned-to-u16",
                value: FieldValue::Unsigned(length as u64),
            });
        }
        let glyph_array_words = glyph_array_bytes / 2;
        let mut glyph_id_array = Vec::with_capacity(glyph_array_words);
        for _ in 0..glyph_array_words {
            glyph_id_array.push(r.read_u16()?);
        }

        Ok(Self {
            language,
            seg_count,
            end_code,
            start_code,
            id_delta,
            id_range_offset,
            glyph_id_array,
        })
    }

    /// Format 4 lookup per OpenType spec § cmap format 4.
    ///
    /// Returns `None` if codepoint > 0xFFFF (BMP only) or unmapped.
    #[must_use]
    pub fn glyph_id(&self, codepoint: u32) -> Option<u16> {
        let cp = u16::try_from(codepoint).ok()?;

        // Binary search: first segment with endCode >= cp.
        let i = self.end_code.partition_point(|&end| end < cp);
        if i >= self.end_code.len() {
            return None;
        }
        if self.start_code[i] > cp {
            return None;
        }

        let glyph = if self.id_range_offset[i] == 0 {
            // Direct: glyph = (cp + idDelta) mod 65536
            let raw = i32::from(cp).wrapping_add(i32::from(self.id_delta[i]));
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let masked = (raw & 0xFFFF) as u16;
            masked
        } else {
            // Indirect via glyphIdArray.
            // Spec formula:
            //   *(idRangeOffset[i] / 2 + (cp - startCode[i]) + &idRangeOffset[i])
            //
            // index into glyph_id_array =
            //   i + (id_range_offset[i] / 2) + (cp - start_code[i]) - seg_count
            let i_signed = i64::try_from(i).ok()?;
            let cp_off = i64::from(cp - self.start_code[i]);
            let range_off = i64::from(self.id_range_offset[i]) / 2;
            let seg_count_signed = i64::from(self.seg_count);
            let idx = i_signed + range_off + cp_off - seg_count_signed;
            if idx < 0 {
                return None;
            }
            let idx = usize::try_from(idx).ok()?;
            let glyph_word = *self.glyph_id_array.get(idx)?;
            if glyph_word == 0 {
                return None;
            }
            let raw = i32::from(glyph_word).wrapping_add(i32::from(self.id_delta[i]));
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let masked = (raw & 0xFFFF) as u16;
            masked
        };
        if glyph == 0 { None } else { Some(glyph) }
    }
}

/// Format 4 spec 공식 search params 검증.
/// segCount ≥ 1 일 때만 호출 (segCountX2 0 reject 는 호출 전 처리).
fn verify_search_params(
    seg_count: u16,
    search_range: u16,
    entry_selector: u16,
    range_shift: u16,
) -> Result<(), ParseError> {
    debug_assert!(seg_count > 0, "verify_search_params requires seg_count ≥ 1");

    let lz = seg_count.leading_zeros();
    let shift = 15u32.saturating_sub(lz);
    let expected_entry_selector = u16::try_from(shift).unwrap_or(u16::MAX);
    let largest_power_of_two: u16 = 1u16 << shift;
    let expected_search_range = largest_power_of_two.saturating_mul(2);
    let expected_range_shift = seg_count
        .saturating_mul(2)
        .saturating_sub(expected_search_range);

    if search_range == expected_search_range
        && entry_selector == expected_entry_selector
        && range_shift == expected_range_shift
    {
        Ok(())
    } else {
        Err(ParseError::InvalidTableField {
            tag: CMAP_TAG,
            field: "format4/search-params-inconsistent",
            value: FieldValue::Unsigned(
                (u64::from(search_range) << 32)
                    | (u64::from(entry_selector) << 16)
                    | u64::from(range_shift),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use super::super::{Cmap, CMAP_TAG};
    use super::*;

    #[test]
    fn format4_indirect_via_id_range_offset() {
        // Build format 4 with a segment using idRangeOffset != 0.
        // segCount=2: real segment [0x41, 0x43] via glyph_id_array, sentinel.
        let seg_count: u16 = 2;
        let length: u16 = 14 + 8 * seg_count + 2 + 2 * 3;
        let mut sub = Vec::new();
        sub.extend_from_slice(&4u16.to_be_bytes());
        sub.extend_from_slice(&length.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&(seg_count * 2).to_be_bytes());
        sub.extend_from_slice(&4u16.to_be_bytes());
        sub.extend_from_slice(&1u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&0x0043u16.to_be_bytes());
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
        sub.extend_from_slice(&0x0041u16.to_be_bytes());
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
        sub.extend_from_slice(&0i16.to_be_bytes());
        sub.extend_from_slice(&1i16.to_be_bytes());
        sub.extend_from_slice(&4u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&100u16.to_be_bytes());
        sub.extend_from_slice(&101u16.to_be_bytes());
        sub.extend_from_slice(&102u16.to_be_bytes());
        assert_eq!(sub.len(), usize::from(length));

        let cmap_bytes = build_cmap_with_subtable(3, 1, &sub);
        let cmap = Cmap::parse(&cmap_bytes).expect("valid cmap");
        assert_eq!(cmap.glyph_id(0x0041), Some(100));
        assert_eq!(cmap.glyph_id(0x0042), Some(101));
        assert_eq!(cmap.glyph_id(0x0043), Some(102));
        assert_eq!(cmap.glyph_id(0x0044), None);
    }

    #[test]
    fn format4_codepoint_above_bmp_returns_none() {
        let sub = build_format4_simple(0x0041, 0x005A, 35);
        let cmap_bytes = build_cmap_with_subtable(3, 1, &sub);
        let cmap = Cmap::parse(&cmap_bytes).expect("valid cmap");
        assert_eq!(cmap.glyph_id(0x1_F600), None);
    }

    #[test]
    fn reject_format4_search_params_inconsistent() {
        let mut sub = build_format4_simple(0x0041, 0x005A, 35);
        sub[8..10].copy_from_slice(&999u16.to_be_bytes());
        let cmap_bytes = build_cmap_with_subtable(3, 1, &sub);
        let err = Cmap::parse(&cmap_bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format4/search-params-inconsistent",
                ..
            }
        ));
    }

    #[test]
    fn reject_format4_endcode_not_ascending() {
        let seg_count: u16 = 3;
        let length: u16 = 14 + 8 * seg_count + 2;
        let mut sub = Vec::new();
        sub.extend_from_slice(&4u16.to_be_bytes());
        sub.extend_from_slice(&length.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&(seg_count * 2).to_be_bytes());
        sub.extend_from_slice(&4u16.to_be_bytes());
        sub.extend_from_slice(&1u16.to_be_bytes());
        sub.extend_from_slice(&2u16.to_be_bytes());
        sub.extend_from_slice(&0x0040u16.to_be_bytes());
        sub.extend_from_slice(&0x0030u16.to_be_bytes());
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&0x0030u16.to_be_bytes());
        sub.extend_from_slice(&0x0020u16.to_be_bytes());
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
        sub.extend_from_slice(&0i16.to_be_bytes());
        sub.extend_from_slice(&0i16.to_be_bytes());
        sub.extend_from_slice(&1i16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());

        let cmap_bytes = build_cmap_with_subtable(3, 1, &sub);
        let err = Cmap::parse(&cmap_bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format4/endCode/not-ascending",
                ..
            }
        ));
    }

    #[test]
    fn reject_format4_startcode_gt_endcode() {
        let mut sub = build_format4_simple(0x0041, 0x005A, 35);
        sub[20..22].copy_from_slice(&0x0060u16.to_be_bytes());
        let cmap_bytes = build_cmap_with_subtable(3, 1, &sub);
        let err = Cmap::parse(&cmap_bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format4/startCode>endCode",
                ..
            }
        ));
    }

    #[test]
    fn format4_endcode_must_terminate_with_ffff() {
        let mut sub = Vec::new();
        sub.extend_from_slice(&4u16.to_be_bytes());
        sub.extend_from_slice(&26u16.to_be_bytes()); // length
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&2u16.to_be_bytes()); // segCountX2=2 (segCount=1)
        sub.extend_from_slice(&2u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&0x00FFu16.to_be_bytes()); // endCode[0] not 0xFFFF
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&0x0000u16.to_be_bytes());
        sub.extend_from_slice(&1i16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        let cmap_bytes = build_cmap_with_subtable(3, 1, &sub);
        let err = Cmap::parse(&cmap_bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format4/endCode[last]",
                ..
            }
        ));
    }
}
