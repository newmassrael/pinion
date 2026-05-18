//! R50.1.4.2 §5.37.1 — `glyf` composite glyph (subglyph + transform).
//!
//! Microsoft OpenType 1.9.x spec, "glyf — Glyph Data" → Composite Glyph.
//!
//! Body layout (after the 10-byte glyph header, when `numberOfContours == -1`):
//!
//! ```text
//! loop:
//!   flags                  uint16
//!   glyphIndex             uint16
//!   argument1 / argument2  int8/int16 또는 uint8/uint16 (flags)
//!   [transformation]       F2DOT14 1/2/4 entries (flags)
//! while MORE_COMPONENTS:
//! [WE_HAVE_INSTRUCTIONS]:
//!   numInstr               uint16
//!   instr[numInstr]        uint8
//! ```
//!
//! Component arg interpretation (flags):
//!
//! * `ARGS_ARE_XY_VALUES` (`0x0002`) = 1 → arg1/arg2 = placement offset (signed)
//! * `ARGS_ARE_XY_VALUES` = 0 → arg1/arg2 = parent point index, child point
//!   index (unsigned)
//! * `ARG_1_AND_2_ARE_WORDS` (`0x0001`) = 1 → 2-byte each (i16 또는 u16)
//! * `ARG_1_AND_2_ARE_WORDS` = 0 → 1-byte each (i8 또는 u8)
//!
//! Transform variants (mutually exclusive — `count_ones() <= 1` strict):
//!
//! * `WE_HAVE_A_SCALE`           (`0x0008`) → 1 × F2DOT14 uniform scale
//! * `WE_HAVE_AN_X_AND_Y_SCALE`  (`0x0040`) → 2 × F2DOT14 x/y scale
//! * `WE_HAVE_A_TWO_BY_TWO`      (`0x0080`) → 4 × F2DOT14 2×2 matrix
//! * none                                   → Identity (no scale/rotation)
//!
//! F2DOT14 표현체: `i16` raw 보존 — caller 가 `/ 16384.0` 으로 float 변환 가능.
//! R50.1.4.2 parser 는 raw i16 만 노출 (textbook lossless representation).
//!
//! Cycle detection: parsing 단계에서는 검출 안 함 (component `glyph_index`
//! 만 record). traversal 시점 (R50.X+ layout/render layer) 에서 ancestor set
//! 검증 = separation of concerns.

use super::{CompositeGlyph, GLYF_TAG, GlyphHeader};
use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

// ─── Composite-glyph flag bits (Microsoft OpenType "glyf" Table 9) ───────
pub(super) const FLAG_ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
pub(super) const FLAG_ARGS_ARE_XY_VALUES: u16 = 0x0002;
#[allow(dead_code)] // hint bit — caller responsibility, parser 보존만
const FLAG_ROUND_XY_TO_GRID: u16 = 0x0004;
pub(super) const FLAG_WE_HAVE_A_SCALE: u16 = 0x0008;
pub(super) const FLAG_RESERVED_BIT_4: u16 = 0x0010;
pub(super) const FLAG_MORE_COMPONENTS: u16 = 0x0020;
pub(super) const FLAG_WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
pub(super) const FLAG_WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;
pub(super) const FLAG_WE_HAVE_INSTRUCTIONS: u16 = 0x0100;
#[allow(dead_code)] // hmtx metric source hint — parser 보존만
const FLAG_USE_MY_METRICS: u16 = 0x0200;
#[allow(dead_code)] // paint hint — parser 보존만
const FLAG_OVERLAP_COMPOUND: u16 = 0x0400;
#[allow(dead_code)] // offset scale hint
const FLAG_SCALED_COMPONENT_OFFSET: u16 = 0x0800;
#[allow(dead_code)] // offset scale hint
const FLAG_UNSCALED_COMPONENT_OFFSET: u16 = 0x1000;
const FLAG_RESERVED_HIGH: u16 = 0xE000;

/// Single component reference within a composite glyph.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Component {
    /// Raw 16-bit flag word (12 bits used; 0x0010 + 0xE000 are reserved=0).
    /// 보존 — caller 가 `USE_MY_METRICS` / `OVERLAP_COMPOUND` / `ROUND_XY_TO_GRID`
    /// 등 hint bit 를 직접 inspect 가능.
    pub flags: u16,
    /// 참조하는 glyph 의 index. parent (composite) glyph 의 id 와 같은 값이면
    /// 즉시 cycle = malformed font (parser 단계 검출 안 함 — traversal 시점 책임).
    pub glyph_index: u16,
    pub args: ComponentArgs,
    pub transform: ComponentTransform,
}

/// Component argument interpretation.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ComponentArgs {
    /// `ARGS_ARE_XY_VALUES = 1` — child glyph 를 parent 의 design space 에 (x, y)
    /// offset 으로 placement. i8/i16 → i32 promotion.
    Offset { x: i32, y: i32 },
    /// `ARGS_ARE_XY_VALUES = 0` — parent 의 `parent`-th point 가 child 의
    /// `child`-th point 와 일치하도록 align. u8/u16 → u32 promotion.
    PointMatch { parent: u32, child: u32 },
}

/// Component transformation matrix (F2DOT14 raw `i16` — `/16384.0` for float).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ComponentTransform {
    /// No flag set — identity (1.0 uniform scale, no rotation/skew).
    Identity,
    /// `WE_HAVE_A_SCALE` — uniform scale `scale / 16384.0` on both axes.
    Scale { scale: i16 },
    /// `WE_HAVE_AN_X_AND_Y_SCALE` — separate x/y scale.
    XYScale { x: i16, y: i16 },
    /// `WE_HAVE_A_TWO_BY_TWO` — full 2×2 matrix (`xx, xy, yx, yy`).
    /// Spec form: `(x', y') = (x·xx + y·yx, x·xy + y·yy)`.
    Matrix { xx: i16, xy: i16, yx: i16, yy: i16 },
}

/// Parse composite glyph body — header (10 byte) 는 caller 가 이미 소비함.
///
/// `raw_body` 는 caller 가 보존한 source-of-truth (R50.1.4.1 의 invariant) —
/// 그대로 `CompositeGlyph::raw_body` 로 forward, parsed view 는 components +
/// instructions 옆에.
///
/// # Errors
///
/// * [`ParseError::TableTooShort`] — body 안에서 cursor 가 slice 너머로 진행.
/// * [`ParseError::InvalidTableField`] — reserved flag bit set / mutually
///   exclusive transform bits 동시 set.
pub(super) fn parse_composite(
    r: &mut Reader<'_>,
    header: GlyphHeader,
    raw_body: Vec<u8>,
) -> Result<CompositeGlyph, ParseError> {
    let mut components = Vec::new();
    loop {
        let flags = r.read_u16()?;
        // Reserved bits — spec mandate must be 0. Silent acceptance 회피.
        if (flags & (FLAG_RESERVED_BIT_4 | FLAG_RESERVED_HIGH)) != 0 {
            return Err(ParseError::InvalidTableField {
                tag: GLYF_TAG,
                field: "composite/flags/reserved-bit-set",
                value: FieldValue::from_u16(flags),
            });
        }
        // Transform bits are mutually exclusive — count_ones <= 1.
        let xform_bits = flags
            & (FLAG_WE_HAVE_A_SCALE | FLAG_WE_HAVE_AN_X_AND_Y_SCALE | FLAG_WE_HAVE_A_TWO_BY_TWO);
        if xform_bits.count_ones() > 1 {
            return Err(ParseError::InvalidTableField {
                tag: GLYF_TAG,
                field: "composite/flags/multiple-transform-bits",
                value: FieldValue::from_u16(flags),
            });
        }

        // spec: "WE_HAVE_INSTRUCTIONS should only be set in the last component
        // of a composite glyph". non-last component 에 set 이면 ambiguity =
        // strict reject (R50.1.2 hhea reserved / R50.1.3.1 cmap searchParams
        // 와 일관). 즉 (MORE_COMPONENTS && WE_HAVE_INSTRUCTIONS) = invalid.
        if (flags & FLAG_MORE_COMPONENTS) != 0 && (flags & FLAG_WE_HAVE_INSTRUCTIONS) != 0 {
            return Err(ParseError::InvalidTableField {
                tag: GLYF_TAG,
                field: "composite/flags/instructions-on-non-last-component",
                value: FieldValue::from_u16(flags),
            });
        }

        let glyph_index = r.read_u16()?;
        let args = read_args(r, flags)?;
        let transform = read_transform(r, xform_bits)?;

        components.push(Component {
            flags,
            glyph_index,
            args,
            transform,
        });

        // Last component (no MORE_COMPONENTS) — 같은 iter 안에서 instructions 까지
        // 처리해야 last component's WE_HAVE_INSTRUCTIONS bit 가 정확한 source.
        if (flags & FLAG_MORE_COMPONENTS) == 0 {
            let instructions = if (flags & FLAG_WE_HAVE_INSTRUCTIONS) != 0 {
                let count = usize::from(r.read_u16()?);
                let mut buf = Vec::with_capacity(count);
                for _ in 0..count {
                    buf.push(r.read_u8()?);
                }
                buf
            } else {
                Vec::new()
            };
            return Ok(CompositeGlyph {
                header,
                raw_body,
                components,
                instructions,
            });
        }
    }
}

/// arg1 / arg2 read — flag bits 에 따라 (i8/i16 signed offset 또는 u8/u16 index).
fn read_args(r: &mut Reader<'_>, flags: u16) -> Result<ComponentArgs, ParseError> {
    let words = (flags & FLAG_ARG_1_AND_2_ARE_WORDS) != 0;
    let xy_values = (flags & FLAG_ARGS_ARE_XY_VALUES) != 0;
    if xy_values {
        let (x, y) = if words {
            (i32::from(r.read_i16()?), i32::from(r.read_i16()?))
        } else {
            // 1-byte signed.
            #[allow(clippy::cast_possible_wrap)] // u8 reinterpretation as i8 explicit
            let x = i32::from(r.read_u8()? as i8);
            #[allow(clippy::cast_possible_wrap)]
            let y = i32::from(r.read_u8()? as i8);
            (x, y)
        };
        Ok(ComponentArgs::Offset { x, y })
    } else {
        let (parent, child) = if words {
            (u32::from(r.read_u16()?), u32::from(r.read_u16()?))
        } else {
            (u32::from(r.read_u8()?), u32::from(r.read_u8()?))
        };
        Ok(ComponentArgs::PointMatch { parent, child })
    }
}

/// Transform read — `xform_bits` 가 정확히 0 또는 한 비트만 set (caller 검증 후).
fn read_transform(r: &mut Reader<'_>, xform_bits: u16) -> Result<ComponentTransform, ParseError> {
    match xform_bits {
        0 => Ok(ComponentTransform::Identity),
        FLAG_WE_HAVE_A_SCALE => Ok(ComponentTransform::Scale {
            scale: r.read_i16()?,
        }),
        FLAG_WE_HAVE_AN_X_AND_Y_SCALE => Ok(ComponentTransform::XYScale {
            x: r.read_i16()?,
            y: r.read_i16()?,
        }),
        FLAG_WE_HAVE_A_TWO_BY_TWO => Ok(ComponentTransform::Matrix {
            xx: r.read_i16()?,
            xy: r.read_i16()?,
            yx: r.read_i16()?,
            yy: r.read_i16()?,
        }),
        // caller 가 count_ones <= 1 을 이미 검증 — 도달 불가.
        _ => unreachable!("xform_bits checked mutually exclusive by caller"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use super::*;

    fn header_dummy() -> GlyphHeader {
        GlyphHeader { x_min: 0, y_min: 0, x_max: 100, y_max: 100 }
    }

    #[test]
    fn single_component_identity_offset_i8() {
        // 1 component: ARGS_ARE_XY_VALUES (no words → i8), no transform, no MORE.
        let body = build_composite_body(&[ComponentSpec {
            flags: FLAG_ARGS_ARE_XY_VALUES, // i8 signed offset, identity, no more
            glyph_index: 7,
            arg1: 10,
            arg2: -5,
            transform: TransformSpec::Identity,
        }]);
        let body_clone = body.clone();
        let mut r = Reader::new(&body, *b"glyf");
        let composite = parse_composite(&mut r, header_dummy(), body_clone).unwrap();
        assert_eq!(composite.components.len(), 1);
        let c = composite.components[0];
        assert_eq!(c.glyph_index, 7);
        assert_eq!(c.args, ComponentArgs::Offset { x: 10, y: -5 });
        assert_eq!(c.transform, ComponentTransform::Identity);
        assert!(composite.instructions.is_empty());
        assert_eq!(composite.raw_body, body); // R50.1.4.1 source-of-truth 유지
    }

    #[test]
    fn single_component_offset_i16_words() {
        let body = build_composite_body(&[ComponentSpec {
            flags: FLAG_ARGS_ARE_XY_VALUES | FLAG_ARG_1_AND_2_ARE_WORDS,
            glyph_index: 42,
            arg1: 5000,
            arg2: -3000,
            transform: TransformSpec::Identity,
        }]);
        let body_clone = body.clone();
        let mut r = Reader::new(&body, *b"glyf");
        let composite = parse_composite(&mut r, header_dummy(), body_clone).unwrap();
        assert_eq!(
            composite.components[0].args,
            ComponentArgs::Offset { x: 5000, y: -3000 }
        );
    }

    #[test]
    fn point_match_unsigned_bytes() {
        // ARGS_ARE_XY_VALUES = 0 → PointMatch unsigned indices.
        let body = build_composite_body(&[ComponentSpec {
            flags: 0, // bytes, point-match
            glyph_index: 3,
            arg1: 200, // i32 → fit in u8 unsigned (200 > i8 max but OK as u8)
            arg2: 50,
            transform: TransformSpec::Identity,
        }]);
        let body_clone = body.clone();
        let mut r = Reader::new(&body, *b"glyf");
        let composite = parse_composite(&mut r, header_dummy(), body_clone).unwrap();
        assert_eq!(
            composite.components[0].args,
            ComponentArgs::PointMatch { parent: 200, child: 50 }
        );
    }

    #[test]
    fn scale_transform() {
        let body = build_composite_body(&[ComponentSpec {
            flags: FLAG_ARGS_ARE_XY_VALUES | FLAG_WE_HAVE_A_SCALE,
            glyph_index: 1,
            arg1: 0,
            arg2: 0,
            transform: TransformSpec::Scale(0x4000), // 1.0 in F2DOT14
        }]);
        let body_clone = body.clone();
        let mut r = Reader::new(&body, *b"glyf");
        let composite = parse_composite(&mut r, header_dummy(), body_clone).unwrap();
        assert_eq!(
            composite.components[0].transform,
            ComponentTransform::Scale { scale: 0x4000 }
        );
    }

    #[test]
    fn xy_scale_transform() {
        let body = build_composite_body(&[ComponentSpec {
            flags: FLAG_ARGS_ARE_XY_VALUES | FLAG_WE_HAVE_AN_X_AND_Y_SCALE,
            glyph_index: 1,
            arg1: 0,
            arg2: 0,
            transform: TransformSpec::XYScale(0x2000, -0x2000),
        }]);
        let body_clone = body.clone();
        let mut r = Reader::new(&body, *b"glyf");
        let composite = parse_composite(&mut r, header_dummy(), body_clone).unwrap();
        assert_eq!(
            composite.components[0].transform,
            ComponentTransform::XYScale { x: 0x2000, y: -0x2000 }
        );
    }

    #[test]
    fn matrix_2x2_transform() {
        let body = build_composite_body(&[ComponentSpec {
            flags: FLAG_ARGS_ARE_XY_VALUES | FLAG_WE_HAVE_A_TWO_BY_TWO,
            glyph_index: 1,
            arg1: 0,
            arg2: 0,
            transform: TransformSpec::Matrix(0x4000, 0, 0, 0x4000), // identity 2x2
        }]);
        let body_clone = body.clone();
        let mut r = Reader::new(&body, *b"glyf");
        let composite = parse_composite(&mut r, header_dummy(), body_clone).unwrap();
        assert_eq!(
            composite.components[0].transform,
            ComponentTransform::Matrix { xx: 0x4000, xy: 0, yx: 0, yy: 0x4000 }
        );
    }

    #[test]
    fn multiple_components_loop() {
        let body = build_composite_body(&[
            ComponentSpec {
                flags: FLAG_ARGS_ARE_XY_VALUES | FLAG_MORE_COMPONENTS,
                glyph_index: 1,
                arg1: 0,
                arg2: 0,
                transform: TransformSpec::Identity,
            },
            ComponentSpec {
                flags: FLAG_ARGS_ARE_XY_VALUES | FLAG_MORE_COMPONENTS,
                glyph_index: 2,
                arg1: 10,
                arg2: 10,
                transform: TransformSpec::Identity,
            },
            ComponentSpec {
                flags: FLAG_ARGS_ARE_XY_VALUES, // last
                glyph_index: 3,
                arg1: 20,
                arg2: 20,
                transform: TransformSpec::Identity,
            },
        ]);
        let body_clone = body.clone();
        let mut r = Reader::new(&body, *b"glyf");
        let composite = parse_composite(&mut r, header_dummy(), body_clone).unwrap();
        assert_eq!(composite.components.len(), 3);
        assert_eq!(composite.components[0].glyph_index, 1);
        assert_eq!(composite.components[1].glyph_index, 2);
        assert_eq!(composite.components[2].glyph_index, 3);
    }

    #[test]
    fn we_have_instructions_appended() {
        let mut body = build_composite_body(&[ComponentSpec {
            flags: FLAG_ARGS_ARE_XY_VALUES | FLAG_WE_HAVE_INSTRUCTIONS,
            glyph_index: 1,
            arg1: 0,
            arg2: 0,
            transform: TransformSpec::Identity,
        }]);
        // append: numInstr=3 + 3 bytes
        body.extend_from_slice(&3u16.to_be_bytes());
        body.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        let body_clone = body.clone();
        let mut r = Reader::new(&body, *b"glyf");
        let composite = parse_composite(&mut r, header_dummy(), body_clone).unwrap();
        assert_eq!(composite.instructions, vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn reject_reserved_bit_4_set() {
        let body = build_composite_body(&[ComponentSpec {
            flags: FLAG_ARGS_ARE_XY_VALUES | FLAG_RESERVED_BIT_4, // 0x0010
            glyph_index: 1,
            arg1: 0,
            arg2: 0,
            transform: TransformSpec::Identity,
        }]);
        let body_clone = body.clone();
        let mut r = Reader::new(&body, *b"glyf");
        let err = parse_composite(&mut r, header_dummy(), body_clone).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "composite/flags/reserved-bit-set",
                ..
            } if tag == GLYF_TAG
        ));
    }

    #[test]
    fn reject_reserved_high_bits_set() {
        let body = build_composite_body(&[ComponentSpec {
            flags: FLAG_ARGS_ARE_XY_VALUES | 0x2000, // bit in 0xE000
            glyph_index: 1,
            arg1: 0,
            arg2: 0,
            transform: TransformSpec::Identity,
        }]);
        let body_clone = body.clone();
        let mut r = Reader::new(&body, *b"glyf");
        let err = parse_composite(&mut r, header_dummy(), body_clone).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "composite/flags/reserved-bit-set",
                ..
            } if tag == GLYF_TAG
        ));
    }

    #[test]
    fn reject_instructions_on_non_last_component() {
        // MORE_COMPONENTS + WE_HAVE_INSTRUCTIONS 동시 set = spec violation
        // (instructions 는 last component 에만).
        let body = build_composite_body(&[
            ComponentSpec {
                flags: FLAG_ARGS_ARE_XY_VALUES
                    | FLAG_MORE_COMPONENTS
                    | FLAG_WE_HAVE_INSTRUCTIONS,
                glyph_index: 1,
                arg1: 0,
                arg2: 0,
                transform: TransformSpec::Identity,
            },
            ComponentSpec {
                flags: FLAG_ARGS_ARE_XY_VALUES, // last
                glyph_index: 2,
                arg1: 0,
                arg2: 0,
                transform: TransformSpec::Identity,
            },
        ]);
        let body_clone = body.clone();
        let mut r = Reader::new(&body, *b"glyf");
        let err = parse_composite(&mut r, header_dummy(), body_clone).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "composite/flags/instructions-on-non-last-component",
                ..
            } if tag == GLYF_TAG
        ));
    }

    #[test]
    fn reject_multiple_transform_bits() {
        // WE_HAVE_A_SCALE + WE_HAVE_AN_X_AND_Y_SCALE = mutually exclusive.
        let body = build_composite_body(&[ComponentSpec {
            flags: FLAG_ARGS_ARE_XY_VALUES
                | FLAG_WE_HAVE_A_SCALE
                | FLAG_WE_HAVE_AN_X_AND_Y_SCALE,
            glyph_index: 1,
            arg1: 0,
            arg2: 0,
            transform: TransformSpec::Scale(0x4000),
        }]);
        let body_clone = body.clone();
        let mut r = Reader::new(&body, *b"glyf");
        let err = parse_composite(&mut r, header_dummy(), body_clone).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "composite/flags/multiple-transform-bits",
                ..
            } if tag == GLYF_TAG
        ));
    }
}
