//! R50.1.4.1 + R50.1.4.2 §5.37.1 — glyf parser test fixtures.
//!
//! 자체 binary builder — 외부 crate 미사용. flag/coord 표현체의 spec 정합을
//! 정확히 통제하기 위해 raw layout.

/// Minimal simple-glyph body: 1 contour, 4 corner points, on-curve, no
/// hinting instructions. Coordinates encoded as 2-byte signed deltas
/// (long form — `X_SHORT` / `X_SAME_OR_POSITIVE` 둘 다 0).
///
/// Layout (총 34 bytes):
/// * header (10 bytes): `numContours=1` + `x_min`/`y_min`/`x_max`/`y_max`
/// * endPtsOfContours[1] = `[3]` (2 bytes)
/// * instructionLength = 0 (2 bytes)
/// * flags[4] = `[0x01; 4]` (on-curve, long-form coords) (4 bytes)
/// * xCoordinates[4] (8 bytes, 2-byte signed delta each)
/// * yCoordinates[4] (8 bytes, 2-byte signed delta each)
///
/// Rectangle 4 corners (counter-clockwise from lower-left, design units):
/// `(x_min, y_min) → (x_max, y_min) → (x_max, y_max) → (x_min, y_max)`.
pub(super) fn build_simple_rectangle(x_min: i16, x_max: i16, y_min: i16, y_max: i16) -> Vec<u8> {
    // bbox 정규화 invariant — `x_max < x_min` 또는 `y_max < y_min` 이면
    // (x_max - x_min) 가 i16 underflow panic. 호출자 정합성 강제.
    debug_assert!(
        x_max >= x_min && y_max >= y_min,
        "bbox must be normalized: x_min={x_min} <= x_max={x_max}, y_min={y_min} <= y_max={y_max}"
    );
    let mut bytes = Vec::with_capacity(34);
    // header
    bytes.extend_from_slice(&1i16.to_be_bytes());
    bytes.extend_from_slice(&x_min.to_be_bytes());
    bytes.extend_from_slice(&y_min.to_be_bytes());
    bytes.extend_from_slice(&x_max.to_be_bytes());
    bytes.extend_from_slice(&y_max.to_be_bytes());
    // endPtsOfContours[0] = 3 (4 points total = indices 0..=3)
    bytes.extend_from_slice(&3u16.to_be_bytes());
    // instructionLength = 0
    bytes.extend_from_slice(&0u16.to_be_bytes());
    // flags: 4 × ON_CURVE_POINT (0x01), no REPEAT
    bytes.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]);
    // x coordinates as 2-byte signed deltas:
    //   p0.x = x_min    → delta = x_min - 0       = x_min
    //   p1.x = x_max    → delta = x_max - x_min
    //   p2.x = x_max    → delta = 0
    //   p3.x = x_min    → delta = x_min - x_max
    bytes.extend_from_slice(&x_min.to_be_bytes());
    bytes.extend_from_slice(&(x_max - x_min).to_be_bytes());
    bytes.extend_from_slice(&0i16.to_be_bytes());
    bytes.extend_from_slice(&(x_min - x_max).to_be_bytes());
    // y coordinates:
    //   p0.y = y_min    → delta = y_min
    //   p1.y = y_min    → delta = 0
    //   p2.y = y_max    → delta = y_max - y_min
    //   p3.y = y_max    → delta = 0
    bytes.extend_from_slice(&y_min.to_be_bytes());
    bytes.extend_from_slice(&0i16.to_be_bytes());
    bytes.extend_from_slice(&(y_max - y_min).to_be_bytes());
    bytes.extend_from_slice(&0i16.to_be_bytes());
    debug_assert_eq!(bytes.len(), 34);
    bytes
}

// ── R50.1.4.2 composite glyph body builder ───────────────────────────────
//
// FLAG_* const 는 `super::compound` 의 pub(super) 정의 = single source of truth.

use super::compound::{FLAG_ARG_1_AND_2_ARE_WORDS, FLAG_ARGS_ARE_XY_VALUES};

#[derive(Clone, Copy)]
pub(super) enum TransformSpec {
    Identity,
    Scale(i16),
    XYScale(i16, i16),
    Matrix(i16, i16, i16, i16),
}

#[derive(Clone, Copy)]
pub(super) struct ComponentSpec {
    pub flags: u16,
    pub glyph_index: u16,
    pub arg1: i32,
    pub arg2: i32,
    pub transform: TransformSpec,
}

/// Component sequence → composite body bytes (header 제외; caller 가 별도 작성).
///
/// `arg1` / `arg2` 의 width / sign 은 spec 의 flag bit 와 일치하도록 caller 가
/// 적정 값을 줘야 — 예: `ARG_1_AND_2_ARE_WORDS` = 0 + `ARGS_ARE_XY_VALUES` = 1 →
/// `arg1` 는 i8 범위 (-128..=127). out-of-range 는 cast wrap 으로 silent —
/// test fixture 책임.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
pub(super) fn build_composite_body(specs: &[ComponentSpec]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for spec in specs {
        bytes.extend_from_slice(&spec.flags.to_be_bytes());
        bytes.extend_from_slice(&spec.glyph_index.to_be_bytes());

        let words = (spec.flags & FLAG_ARG_1_AND_2_ARE_WORDS) != 0;
        let xy = (spec.flags & FLAG_ARGS_ARE_XY_VALUES) != 0;
        match (words, xy) {
            (true, true) => {
                bytes.extend_from_slice(&(spec.arg1 as i16).to_be_bytes());
                bytes.extend_from_slice(&(spec.arg2 as i16).to_be_bytes());
            }
            (true, false) => {
                bytes.extend_from_slice(&(spec.arg1 as u16).to_be_bytes());
                bytes.extend_from_slice(&(spec.arg2 as u16).to_be_bytes());
            }
            (false, true) => {
                bytes.push((spec.arg1 as i8) as u8);
                bytes.push((spec.arg2 as i8) as u8);
            }
            (false, false) => {
                bytes.push(spec.arg1 as u8);
                bytes.push(spec.arg2 as u8);
            }
        }

        match spec.transform {
            TransformSpec::Identity => {}
            TransformSpec::Scale(s) => bytes.extend_from_slice(&s.to_be_bytes()),
            TransformSpec::XYScale(x, y) => {
                bytes.extend_from_slice(&x.to_be_bytes());
                bytes.extend_from_slice(&y.to_be_bytes());
            }
            TransformSpec::Matrix(xx, xy, yx, yy) => {
                bytes.extend_from_slice(&xx.to_be_bytes());
                bytes.extend_from_slice(&xy.to_be_bytes());
                bytes.extend_from_slice(&yx.to_be_bytes());
                bytes.extend_from_slice(&yy.to_be_bytes());
            }
        }
    }
    bytes
}
