//! R50.1.4.1 §5.37.1 — `glyf` simple glyph (TrueType outline).
//!
//! Microsoft OpenType 1.9.x spec, "glyf — Glyph Data" → Simple Glyph.
//!
//! Body layout (after the 10-byte glyph header):
//!
//! ```text
//! endPtsOfContours[numberOfContours]   uint16 each   (단조 증가)
//! instructionLength                     uint16
//! instructions[instructionLength]       uint8 each   (hinting bytecode)
//! flags[]                               uint8 each   (REPEAT 압축; expand 후 num_points)
//! xCoordinates[]                        u8 또는 i16  (flag 별 가변 너비)
//! yCoordinates[]                        u8 또는 i16  (flag 별 가변 너비)
//! ```
//!
//! `num_points = endPtsOfContours.last() + 1`. flags / coords 는 REPEAT 압축 +
//! short/same 가변 너비 — spec 정통 그대로 expand. coord delta 는 누적합으로
//! 절댓값 변환 (`x[0] = dx[0]`, `x[i] = x[i-1] + dx[i]`).
//!
//! 모든 spec mandate (단조 증가 endPts, reserved flag bit = 0, count 일치)
//! strict reject — black-box tolerance 없음 (§2 invariant #2).

use super::{GLYF_TAG, GlyphHeader, GlyphPoint};
use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

/// Simple glyph — flattened contour + absolute-coordinate points.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SimpleGlyph {
    pub header: GlyphHeader,
    /// 각 contour 의 마지막 point index. 단조 증가 (spec mandate).
    /// `end_pts_of_contours.last()` = `num_points - 1`.
    pub end_pts_of_contours: Vec<u16>,
    /// TrueType hinting bytecode. 0 length 가능.
    pub instructions: Vec<u8>,
    /// 모든 point — 절댓값 좌표 (delta 합산 후) + on/off curve 표시.
    /// `points.len() == end_pts_of_contours.last() + 1`.
    pub points: Vec<GlyphPoint>,
}

// ─── Simple-glyph flag bits (Microsoft OpenType "glyf" Table 8) ──────────
const FLAG_ON_CURVE_POINT: u8 = 0x01;
const FLAG_X_SHORT_VECTOR: u8 = 0x02;
const FLAG_Y_SHORT_VECTOR: u8 = 0x04;
const FLAG_REPEAT: u8 = 0x08;
const FLAG_X_IS_SAME_OR_POSITIVE: u8 = 0x10;
const FLAG_Y_IS_SAME_OR_POSITIVE: u8 = 0x20;
const FLAG_OVERLAP_SIMPLE: u8 = 0x40;
const FLAG_RESERVED: u8 = 0x80;

/// `SimpleGlyph` body parse — header (10 byte) 는 caller 가 이미 소비함.
///
/// `num_contours` 는 header 의 `numberOfContours` (caller 가 ≥ 0 검증 완료).
///
/// # Errors
///
/// * [`ParseError::TableTooShort`] — body 안에서 cursor 가 slice 너머로 진행.
/// * [`ParseError::InvalidTableField`] — endPts 단조 증가 위반 / reserved
///   flag bit set / 0-contour 또는 0-point glyph / flag-expand count mismatch.
pub(super) fn parse_simple(
    r: &mut Reader<'_>,
    header: GlyphHeader,
    num_contours: u16,
) -> Result<SimpleGlyph, ParseError> {
    // spec: simple glyph 는 numberOfContours ≥ 1 (numberOfContours == 0 인 simple
    // glyph 는 의미 모호 — 빈 glyph 는 loca[i] == loca[i+1] 로 표현).
    if num_contours == 0 {
        return Err(ParseError::InvalidTableField {
            tag: GLYF_TAG,
            field: "simple/numberOfContours",
            value: FieldValue::Unsigned(0),
        });
    }

    let mut end_pts_of_contours = Vec::with_capacity(usize::from(num_contours));
    for _ in 0..num_contours {
        end_pts_of_contours.push(r.read_u16()?);
    }
    // spec: endPtsOfContours 는 단조 증가 (각 contour 가 ≥ 1 point).
    for window in end_pts_of_contours.windows(2) {
        if window[0] >= window[1] {
            return Err(ParseError::InvalidTableField {
                tag: GLYF_TAG,
                field: "simple/endPtsOfContours/not-ascending",
                value: FieldValue::from_u16(window[0]),
            });
        }
    }

    // num_points = endPtsOfContours.last() + 1.
    // 마지막 entry 가 0xFFFF 면 +1 overflow → 안전한 분기로 reject.
    let last_end_pt = *end_pts_of_contours
        .last()
        .expect("num_contours ≥ 1 — vec non-empty");
    let num_points = usize::from(last_end_pt).checked_add(1).ok_or(
        ParseError::InvalidTableField {
            tag: GLYF_TAG,
            field: "simple/endPtsOfContours[last]/overflow",
            value: FieldValue::from_u16(last_end_pt),
        },
    )?;
    if num_points == 0 {
        return Err(ParseError::InvalidTableField {
            tag: GLYF_TAG,
            field: "simple/num_points",
            value: FieldValue::Unsigned(0),
        });
    }

    let instruction_length = r.read_u16()?;
    let mut instructions = Vec::with_capacity(usize::from(instruction_length));
    for _ in 0..instruction_length {
        instructions.push(r.read_u8()?);
    }

    // Flag expansion — REPEAT 압축 풀고 정확히 num_points 개 채움.
    let flags = expand_flags(r, num_points)?;

    // x coordinates — flag 별 가변 너비. delta 누적 → 절댓값.
    let xs = read_coordinates(
        r,
        &flags,
        FLAG_X_SHORT_VECTOR,
        FLAG_X_IS_SAME_OR_POSITIVE,
    )?;
    // y coordinates — same logic with Y flag bits.
    let ys = read_coordinates(
        r,
        &flags,
        FLAG_Y_SHORT_VECTOR,
        FLAG_Y_IS_SAME_OR_POSITIVE,
    )?;

    let mut points = Vec::with_capacity(num_points);
    for i in 0..num_points {
        points.push(GlyphPoint {
            x: xs[i],
            y: ys[i],
            on_curve: (flags[i] & FLAG_ON_CURVE_POINT) != 0,
        });
    }

    Ok(SimpleGlyph {
        header,
        end_pts_of_contours,
        instructions,
        points,
    })
}

/// REPEAT 풀어 flags vec 채우기. 정확히 `num_points` 개를 만들어야 — 더/덜
/// 채우면 spec 위반 (compressed stream 이 boundary 정합).
fn expand_flags(r: &mut Reader<'_>, num_points: usize) -> Result<Vec<u8>, ParseError> {
    let mut flags = Vec::with_capacity(num_points);
    while flags.len() < num_points {
        let flag = r.read_u8()?;
        // spec: reserved bit (0x80) must be zero. AI introspect determinism
        // 위해 strict reject — 알 수 없는 future flag 에 silent acceptance 금지.
        if (flag & FLAG_RESERVED) != 0 {
            return Err(ParseError::InvalidTableField {
                tag: GLYF_TAG,
                field: "simple/flag/reserved-bit-set",
                value: FieldValue::Unsigned(u64::from(flag)),
            });
        }
        // OVERLAP_SIMPLE 은 spec defined (paint hint), 검증만 하고 보존.
        let _ = flag & FLAG_OVERLAP_SIMPLE;

        let repeat_count = if (flag & FLAG_REPEAT) != 0 {
            usize::from(r.read_u8()?)
        } else {
            0
        };
        // (count + 1) instances — count 는 *추가* repetitions.
        let total = repeat_count
            .checked_add(1)
            .ok_or(ParseError::InvalidTableField {
                tag: GLYF_TAG,
                field: "simple/flag/repeat-count-overflow",
                value: FieldValue::Unsigned(repeat_count as u64),
            })?;
        if flags.len() + total > num_points {
            // Compressed flag stream 이 num_points 너머 expand — spec 위반.
            return Err(ParseError::InvalidTableField {
                tag: GLYF_TAG,
                field: "simple/flag/expand-exceeds-num-points",
                value: FieldValue::Unsigned((flags.len() + total) as u64),
            });
        }
        for _ in 0..total {
            flags.push(flag);
        }
    }
    debug_assert_eq!(flags.len(), num_points);
    Ok(flags)
}

/// 가변 너비 coordinate stream → 절댓값 vec.
///
/// spec (X 축 기준; Y 동일):
///
/// | `X_SHORT` | `X_SAME_OR_POSITIVE` | meaning                                |
/// |-----------|----------------------|----------------------------------------|
/// | 1         | 1                    | 1 byte unsigned, positive delta        |
/// | 1         | 0                    | 1 byte unsigned, negative delta        |
/// | 0         | 1                    | 0 byte, delta = 0 (same as previous)   |
/// | 0         | 0                    | 2 byte signed delta                    |
///
/// `flags[i]` 의 short/same 비트를 모두 검사 — bitor 가 아닌 별도 평가.
fn read_coordinates(
    r: &mut Reader<'_>,
    flags: &[u8],
    short_bit: u8,
    same_or_positive_bit: u8,
) -> Result<Vec<i16>, ParseError> {
    let mut coords = Vec::with_capacity(flags.len());
    let mut accum: i32 = 0;
    for &flag in flags {
        let short = (flag & short_bit) != 0;
        let same_or_pos = (flag & same_or_positive_bit) != 0;
        let delta: i32 = match (short, same_or_pos) {
            (true, true) => i32::from(r.read_u8()?),
            (true, false) => -i32::from(r.read_u8()?),
            (false, true) => 0,
            (false, false) => i32::from(r.read_i16()?),
        };
        // i32 overflow 도 spec 위반 — wrap-around silent acceptance 회피.
        accum = accum
            .checked_add(delta)
            .ok_or(ParseError::InvalidTableField {
                tag: GLYF_TAG,
                field: "simple/coordinate-i32-overflow",
                value: FieldValue::Signed(i64::from(accum) + i64::from(delta)),
            })?;
        // glyph coordinate 는 i16 range. 누적합이 범위 벗어나면 spec 위반.
        let clamped = i16::try_from(accum).map_err(|_| ParseError::InvalidTableField {
            tag: GLYF_TAG,
            field: "simple/coordinate-overflow",
            value: FieldValue::Signed(i64::from(accum)),
        })?;
        coords.push(clamped);
    }
    Ok(coords)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_dummy() -> GlyphHeader {
        GlyphHeader { x_min: 0, y_min: 0, x_max: 100, y_max: 100 }
    }

    /// 4-point single-contour body (header 제외 — caller 가 이미 소비함).
    fn body_4_points_long_form() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&3u16.to_be_bytes()); // endPts = 3
        b.extend_from_slice(&0u16.to_be_bytes()); // instructionLength = 0
        b.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]); // flags on-curve, long form
        // x deltas
        for v in [0i16, 100, 0, -100] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        // y deltas
        for v in [0i16, 0, 50, 0] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        b
    }

    #[test]
    fn parse_4_points_long_form_coords() {
        let body = body_4_points_long_form();
        let mut r = Reader::new(&body, *b"glyf");
        let sg = parse_simple(&mut r, header_dummy(), 1).expect("valid");
        assert_eq!(sg.points.len(), 4);
        assert_eq!((sg.points[0].x, sg.points[0].y), (0, 0));
        assert_eq!((sg.points[1].x, sg.points[1].y), (100, 0));
        assert_eq!((sg.points[2].x, sg.points[2].y), (100, 50));
        assert_eq!((sg.points[3].x, sg.points[3].y), (0, 50));
    }

    #[test]
    fn repeat_flag_expansion() {
        // 4 points all sharing flag = on-curve|REPEAT, repeat count 3 → (3+1) = 4 points.
        // After flag-byte we read repeat count then x/y coords (long form each).
        let flag_value: u8 = 0x01 | FLAG_REPEAT;
        let mut b = Vec::new();
        b.extend_from_slice(&3u16.to_be_bytes()); // endPts
        b.extend_from_slice(&0u16.to_be_bytes()); // instructionLength
        b.push(flag_value);
        b.push(3); // repeat count = 3 → 4 flags total
        // x deltas (long form, 4 × i16)
        for v in [10i16, 20, 30, -10] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        for v in [0i16, 5, 0, -5] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        let mut r = Reader::new(&b, *b"glyf");
        let sg = parse_simple(&mut r, header_dummy(), 1).expect("valid");
        assert_eq!(sg.points.len(), 4);
        assert_eq!(sg.points[0].x, 10);
        assert_eq!(sg.points[1].x, 30);
        assert_eq!(sg.points[2].x, 60);
        assert_eq!(sg.points[3].x, 50);
        assert!(sg.points.iter().all(|p| p.on_curve));
    }

    #[test]
    fn x_short_positive_and_negative() {
        // 2 points: 1st with X_SHORT + X_SAME_OR_POSITIVE (positive 1 byte),
        // 2nd with X_SHORT only (negative 1 byte).
        let mut b = Vec::new();
        b.extend_from_slice(&1u16.to_be_bytes()); // endPts = 1 → 2 points
        b.extend_from_slice(&0u16.to_be_bytes()); // instructionLength
        b.push(FLAG_X_SHORT_VECTOR | FLAG_X_IS_SAME_OR_POSITIVE | FLAG_ON_CURVE_POINT);
        b.push(FLAG_X_SHORT_VECTOR | FLAG_ON_CURVE_POINT);
        // x bytes (1 byte each)
        b.push(50);
        b.push(30);
        // y: both flags long form (no Y_SHORT, no Y_SAME) → 2 byte signed each
        // 잠깐 — flags 의 Y_SHORT/Y_SAME bit 모두 0 → 2 byte signed.
        b.extend_from_slice(&5i16.to_be_bytes());
        b.extend_from_slice(&10i16.to_be_bytes());
        let mut r = Reader::new(&b, *b"glyf");
        let sg = parse_simple(&mut r, header_dummy(), 1).expect("valid");
        // p0.x = +50, p1.x = 50 - 30 = 20
        assert_eq!(sg.points[0].x, 50);
        assert_eq!(sg.points[1].x, 20);
    }

    #[test]
    fn x_same_zero_delta() {
        // ~X_SHORT + X_SAME_OR_POSITIVE → 0 byte, delta = 0 (same as previous).
        let mut b = Vec::new();
        b.extend_from_slice(&1u16.to_be_bytes()); // endPts = 1 → 2 points
        b.extend_from_slice(&0u16.to_be_bytes());
        // p0: long-form x (2-byte), p1: x same as p0
        b.push(FLAG_ON_CURVE_POINT);
        b.push(FLAG_X_IS_SAME_OR_POSITIVE | FLAG_ON_CURVE_POINT);
        // x: 2-byte signed delta for p0, 0 byte for p1
        b.extend_from_slice(&100i16.to_be_bytes());
        // y: both long form
        b.extend_from_slice(&0i16.to_be_bytes());
        b.extend_from_slice(&0i16.to_be_bytes());
        let mut r = Reader::new(&b, *b"glyf");
        let sg = parse_simple(&mut r, header_dummy(), 1).expect("valid");
        assert_eq!(sg.points[0].x, 100);
        assert_eq!(sg.points[1].x, 100);
    }

    #[test]
    fn reject_reserved_flag_bit_set() {
        let mut b = Vec::new();
        b.extend_from_slice(&0u16.to_be_bytes()); // endPts = 0 → 1 point
        b.extend_from_slice(&0u16.to_be_bytes());
        b.push(0x80 | FLAG_ON_CURVE_POINT); // reserved bit set
        b.extend_from_slice(&0i16.to_be_bytes()); // x (long form because flags 0)
        // 실제 flag 의 X_SHORT/X_SAME 이 0 → long form (i16 each)
        b.extend_from_slice(&0i16.to_be_bytes()); // y
        let mut r = Reader::new(&b, *b"glyf");
        let err = parse_simple(&mut r, header_dummy(), 1).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "simple/flag/reserved-bit-set",
                ..
            } if tag == GLYF_TAG
        ));
    }

    #[test]
    fn reject_end_pts_not_ascending() {
        let mut b = Vec::new();
        // numContours = 2, endPts = [3, 2] (감소)
        b.extend_from_slice(&3u16.to_be_bytes());
        b.extend_from_slice(&2u16.to_be_bytes());
        let mut r = Reader::new(&b, *b"glyf");
        let err = parse_simple(&mut r, header_dummy(), 2).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "simple/endPtsOfContours/not-ascending",
                ..
            } if tag == GLYF_TAG
        ));
    }

    #[test]
    fn reject_zero_num_contours() {
        let mut r = Reader::new(&[], *b"glyf");
        let err = parse_simple(&mut r, header_dummy(), 0).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "simple/numberOfContours",
                ..
            } if tag == GLYF_TAG
        ));
    }

    #[test]
    fn reject_i16_coordinate_overflow() {
        // 누적 delta 가 i16 range (-32768..=32767) 벗어나면 spec 위반.
        // 2 points, p0.x = 30000, p1.x = 30000 + 30000 = 60000 (i16 overflow).
        let mut b = Vec::new();
        b.extend_from_slice(&1u16.to_be_bytes()); // endPts = 1 → 2 points
        b.extend_from_slice(&0u16.to_be_bytes());
        // 두 point 모두 long-form i16 delta.
        b.push(FLAG_ON_CURVE_POINT);
        b.push(FLAG_ON_CURVE_POINT);
        b.extend_from_slice(&30_000i16.to_be_bytes()); // p0.x delta
        b.extend_from_slice(&30_000i16.to_be_bytes()); // p1.x delta → 누적 60000
        // y both 0 long-form
        b.extend_from_slice(&0i16.to_be_bytes());
        b.extend_from_slice(&0i16.to_be_bytes());
        let mut r = Reader::new(&b, *b"glyf");
        let err = parse_simple(&mut r, header_dummy(), 1).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "simple/coordinate-overflow",
                ..
            } if tag == GLYF_TAG
        ));
    }

    #[test]
    fn reject_flag_expand_exceeds_num_points() {
        let mut b = Vec::new();
        b.extend_from_slice(&1u16.to_be_bytes()); // endPts = 1 → 2 points
        b.extend_from_slice(&0u16.to_be_bytes());
        b.push(FLAG_ON_CURVE_POINT | FLAG_REPEAT);
        b.push(10); // repeat count 10 → 11 instances but only 2 needed
        let mut r = Reader::new(&b, *b"glyf");
        let err = parse_simple(&mut r, header_dummy(), 1).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "simple/flag/expand-exceeds-num-points",
                ..
            } if tag == GLYF_TAG
        ));
    }
}

