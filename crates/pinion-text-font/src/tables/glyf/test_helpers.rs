//! R50.1.4.1 §5.37.1 — glyf parser test fixtures.
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
