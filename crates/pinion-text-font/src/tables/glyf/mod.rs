//! R50.1.4.1 §5.37.1 — `glyf` table (TrueType glyph data).
//!
//! Microsoft OpenType 1.9.x spec, "glyf — Glyph Data". TrueType outline
//! container — per-glyph blob 의 sequential layout, loca table 의 offset 으로
//! random-access.
//!
//! Glyph type discrimination (`numberOfContours` 의 sign 으로):
//!
//! * **Empty**: `loca[i] == loca[i+1]` (해당 glyph 의 byte range = 0) —
//!   control codepoint / space 등. glyph header 자체가 없음.
//! * **Simple** (`numberOfContours >= 1`): contour 기반 outline + on/off-curve
//!   points. R50.1.4.1 에서 완전 parse.
//! * **Composite** (`numberOfContours == -1`): 다른 glyph references + 변환
//!   matrix. **R50.1.4.1 에서는 header 만 parse + raw body 보존** —
//!   R50.1.4.2 에서 components/transform 으로 fully parse.
//!
//! Folder split (R50.1.4.1 출발부터): industry precedent (read-fonts,
//! ttf-parser, fonttools) 정합 — `glyf/{mod, simple, test_helpers}`. R50.1.4.2
//! 진입 시 `compound.rs` 추가.

use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;
use crate::tables::loca::Loca;

mod compound;
mod simple;
#[cfg(test)]
mod test_helpers;

pub use compound::{Component, ComponentArgs, ComponentTransform};
pub use simple::SimpleGlyph;

pub(super) const GLYF_TAG: [u8; 4] = *b"glyf";

/// Glyph 의 bounding box — 모든 outline variant 가 공유하는 header.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct GlyphHeader {
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
}

/// Outline 의 한 control point — simple glyph 전용.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct GlyphPoint {
    /// 절댓값 design-unit x (delta 누적 후).
    pub x: i16,
    /// 절댓값 design-unit y (delta 누적 후).
    pub y: i16,
    /// `true` = on-curve (anchor), `false` = off-curve (quadratic bezier control).
    pub on_curve: bool,
}

/// Composite glyph — header + raw body bytes + parsed components.
///
/// `raw_body` 는 source-of-truth 로 항구 보존 (R50.1.4.1 invariant). R50.1.4.2
/// 가 그 옆에 `components: Vec<Component>` + `instructions: Vec<u8>` additive
/// 추가 — parsed view 와 raw source 가 모두 노출되어 re-parse / invariant 재
/// 검증 / future extension 가능.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CompositeGlyph {
    pub header: GlyphHeader,
    /// glyph header (10 byte) 뒤의 나머지 raw bytes. 항구 보존.
    pub raw_body: Vec<u8>,
    /// R50.1.4.2 — composite component records (subglyph + args + transform).
    /// Spec: 항상 ≥ 1 entry.
    pub components: Vec<Component>,
    /// R50.1.4.2 — last component's `WE_HAVE_INSTRUCTIONS` 가 set 인 경우 부착된
    /// TrueType hinting bytecode. 그 외에는 empty vec.
    pub instructions: Vec<u8>,
}

/// Glyph data variant — empty / simple / composite.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Glyph {
    /// `loca[i] == loca[i+1]` — glyph 가 시각적 표현 없음.
    Empty,
    Simple(SimpleGlyph),
    /// R50.1.4.1: header + raw body 보존. R50.1.4.2 가 components/transform
    /// parsed view 를 additive 로 채움 (`raw_body` 항구 유지).
    Composite(CompositeGlyph),
}

impl Glyph {
    /// Glyph 의 bounding box header — `Empty` 에는 header 없음 (`None`),
    /// `Simple` / `Composite` 는 `Some(header)`. caller 가 variant 패턴매칭
    /// 없이 bbox 정보 빨리 추출용.
    #[must_use]
    pub fn header(&self) -> Option<GlyphHeader> {
        match self {
            Self::Empty => None,
            Self::Simple(s) => Some(s.header),
            Self::Composite(c) => Some(c.header),
        }
    }
}

/// `glyf` table — `num_glyphs` 개의 Glyph (index = glyph id).
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Glyf {
    pub glyphs: Vec<Glyph>,
}

impl Glyf {
    /// Parse the glyf table bytes using loca for per-glyph byte ranges.
    ///
    /// `loca.offsets` 의 `num_glyphs + 1` entries 가 glyf bytes 의 단조 비감소
    /// slice boundary — loca parser 가 이미 검증. windows(2) 로 sentinel 까지
    /// 안전 순회 (panic 없음).
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — glyph byte range 가 bytes 너머.
    /// * [`ParseError::InvalidTableField`] — simple glyph parse 의 spec 위반
    ///   (endPts non-ascending / reserved flag bit / coordinate overflow 등).
    pub fn parse(bytes: &[u8], loca: &Loca) -> Result<Self, ParseError> {
        let mut glyphs = Vec::with_capacity(loca.num_glyphs());
        for window in loca.offsets.windows(2) {
            let start = window[0];
            let end = window[1];
            if start == end {
                glyphs.push(Glyph::Empty);
                continue;
            }
            // start <= end 는 loca parser 가 이미 단조성 검증 — 여기서 추가 검증 불필요.
            // u32 → usize widening cast: 32-bit 플랫폼에서도 usize ≥ u32 이므로 안전.
            let start_usize = start as usize;
            let end_usize = end as usize;
            if end_usize > bytes.len() {
                return Err(ParseError::TableTooShort {
                    tag: GLYF_TAG,
                    needed: end_usize,
                    available: bytes.len(),
                });
            }
            let body = &bytes[start_usize..end_usize];
            glyphs.push(parse_glyph(body)?);
        }
        Ok(Self { glyphs })
    }

    /// Glyph 의 view by id. `glyph_id >= num_glyphs` 면 `None`.
    #[must_use]
    pub fn glyph(&self, glyph_id: u16) -> Option<&Glyph> {
        self.glyphs.get(usize::from(glyph_id))
    }

    /// Glyph 개수.
    #[must_use]
    pub fn num_glyphs(&self) -> usize {
        self.glyphs.len()
    }
}

/// 단일 glyph body parse — header 10 byte + variant body.
fn parse_glyph(bytes: &[u8]) -> Result<Glyph, ParseError> {
    let mut r = Reader::new(bytes, GLYF_TAG);
    let num_contours_raw = r.read_i16()?;
    let header = GlyphHeader {
        x_min: r.read_i16()?,
        y_min: r.read_i16()?,
        x_max: r.read_i16()?,
        y_max: r.read_i16()?,
    };
    // spec: x_min <= x_max, y_min <= y_max (bounding box invariant).
    if header.x_min > header.x_max || header.y_min > header.y_max {
        return Err(ParseError::InvalidTableField {
            tag: GLYF_TAG,
            field: "header/bbox-inverted",
            value: FieldValue::Signed(i64::from(header.x_min) - i64::from(header.x_max)),
        });
    }

    // spec: numberOfContours >= 0 → simple, -1 → composite. -2 이하는 spec
    // violation (silent acceptance = R50.1.2 hhea reserved 와 일관 strict reject
    // 정신 위반). textbook canonical = exact -1 match.
    match num_contours_raw {
        n if n >= 0 => {
            #[allow(clippy::cast_sign_loss)] // n >= 0 → 캐스트 안전
            let num_contours = n as u16;
            Ok(Glyph::Simple(simple::parse_simple(
                &mut r,
                header,
                num_contours,
            )?))
        }
        -1 => {
            // composite glyph — header 후 body 를 raw 로 capture + parsed view 생성.
            // raw_body 는 source-of-truth 항구 보존, components + instructions 는
            // R50.1.4.2 의 parsed view (additive).
            let body_start = r.position();
            let raw_body = bytes[body_start..].to_vec();
            Ok(Glyph::Composite(compound::parse_composite(
                &mut r, header, raw_body,
            )?))
        }
        other => Err(ParseError::InvalidTableField {
            tag: GLYF_TAG,
            field: "header/numberOfContours-invalid",
            value: FieldValue::Signed(i64::from(other)),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::*;
    use super::*;
    use crate::tables::loca::LocaFormat;

    #[test]
    fn parse_simple_rectangle_glyph() {
        let bytes = build_simple_rectangle(0, 100, 0, 50);
        let loca = Loca {
            format: LocaFormat::Long,
            offsets: vec![0, u32::try_from(bytes.len()).unwrap()],
        };
        let glyf = Glyf::parse(&bytes, &loca).expect("valid glyf");
        assert_eq!(glyf.num_glyphs(), 1);
        match glyf.glyph(0).expect("glyph 0 exists") {
            Glyph::Simple(s) => {
                assert_eq!(
                    s.header,
                    GlyphHeader { x_min: 0, y_min: 0, x_max: 100, y_max: 50 }
                );
                assert_eq!(s.end_pts_of_contours, vec![3]);
                assert_eq!(s.points.len(), 4);
                assert!(s.points.iter().all(|p| p.on_curve));
                // delta 누적 후 절댓값.
                assert_eq!((s.points[0].x, s.points[0].y), (0, 0));
                assert_eq!((s.points[1].x, s.points[1].y), (100, 0));
                assert_eq!((s.points[2].x, s.points[2].y), (100, 50));
                assert_eq!((s.points[3].x, s.points[3].y), (0, 50));
            }
            other => panic!("expected Simple, got {other:?}"),
        }
    }

    #[test]
    fn parse_empty_glyph_via_loca_range_zero() {
        // 2 glyphs: g0 = simple rectangle, g1 = empty (loca[1] == loca[2]).
        let g0 = build_simple_rectangle(0, 100, 0, 50);
        let len = u32::try_from(g0.len()).unwrap();
        let loca = Loca {
            format: LocaFormat::Long,
            offsets: vec![0, len, len],
        };
        let glyf = Glyf::parse(&g0, &loca).expect("valid glyf");
        assert_eq!(glyf.num_glyphs(), 2);
        assert!(matches!(glyf.glyphs[0], Glyph::Simple(_)));
        assert_eq!(glyf.glyphs[1], Glyph::Empty);
    }

    #[test]
    fn glyph_lookup_out_of_range() {
        let bytes = build_simple_rectangle(0, 100, 0, 50);
        let loca = Loca {
            format: LocaFormat::Long,
            offsets: vec![0, u32::try_from(bytes.len()).unwrap()],
        };
        let glyf = Glyf::parse(&bytes, &loca).expect("valid glyf");
        assert!(glyf.glyph(0).is_some());
        assert!(glyf.glyph(1).is_none());
    }

    #[test]
    fn reject_glyf_too_short_for_loca_range() {
        // loca 가 [0, 100] 을 말하지만 bytes 가 10밖에 안 됨.
        let bytes = vec![0u8; 10];
        let loca = Loca {
            format: LocaFormat::Long,
            offsets: vec![0, 100],
        };
        let err = Glyf::parse(&bytes, &loca).unwrap_err();
        assert!(matches!(
            err,
            ParseError::TableTooShort {
                tag: GLYF_TAG,
                needed: 100,
                available: 10,
            }
        ));
    }

    #[test]
    fn reject_glyph_inverted_bbox() {
        // x_min > x_max
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1i16.to_be_bytes()); // numberOfContours
        bytes.extend_from_slice(&100i16.to_be_bytes()); // x_min
        bytes.extend_from_slice(&0i16.to_be_bytes()); // y_min
        bytes.extend_from_slice(&50i16.to_be_bytes()); // x_max < x_min
        bytes.extend_from_slice(&50i16.to_be_bytes()); // y_max
        bytes.extend_from_slice(&0u16.to_be_bytes()); // endPts dummy
        let loca = Loca {
            format: LocaFormat::Long,
            offsets: vec![0, u32::try_from(bytes.len()).unwrap()],
        };
        let err = Glyf::parse(&bytes, &loca).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag: GLYF_TAG,
                field: "header/bbox-inverted",
                ..
            }
        ));
    }

    #[test]
    fn reject_numberofcontours_minus_two() {
        // -1 만 composite spec 정합. -2 / -3 등은 spec violation.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(-2i16).to_be_bytes()); // numberOfContours = -2
        bytes.extend_from_slice(&0i16.to_be_bytes());
        bytes.extend_from_slice(&0i16.to_be_bytes());
        bytes.extend_from_slice(&100i16.to_be_bytes());
        bytes.extend_from_slice(&100i16.to_be_bytes());
        let loca = Loca {
            format: LocaFormat::Long,
            offsets: vec![0, u32::try_from(bytes.len()).unwrap()],
        };
        let err = Glyf::parse(&bytes, &loca).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag,
                field: "header/numberOfContours-invalid",
                value: FieldValue::Signed(-2),
            } if tag == GLYF_TAG
        ));
    }

    #[test]
    fn glyph_header_accessor() {
        // Empty → None / Simple → Some / Composite → Some.
        let empty = Glyph::Empty;
        assert_eq!(empty.header(), None);

        let simple = Glyph::Simple(SimpleGlyph {
            header: GlyphHeader { x_min: 1, y_min: 2, x_max: 3, y_max: 4 },
            end_pts_of_contours: vec![],
            instructions: vec![],
            points: vec![],
        });
        assert_eq!(
            simple.header(),
            Some(GlyphHeader { x_min: 1, y_min: 2, x_max: 3, y_max: 4 })
        );

        let composite = Glyph::Composite(CompositeGlyph {
            header: GlyphHeader { x_min: -10, y_min: -20, x_max: 100, y_max: 200 },
            raw_body: vec![],
            components: vec![],
            instructions: vec![],
        });
        assert_eq!(
            composite.header(),
            Some(GlyphHeader { x_min: -10, y_min: -20, x_max: 100, y_max: 200 })
        );
    }

    #[test]
    #[allow(clippy::cast_sign_loss)] // (-5i8) as u8 = explicit signed-byte serialization
    fn parse_composite_single_component() {
        // numberOfContours = -1, bbox valid, 1 component (i8 args, identity, no more).
        // Component body: flags(2) + glyphIndex(2) + arg1(1) + arg2(1) = 6 bytes.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(-1i16).to_be_bytes()); // numberOfContours = -1
        bytes.extend_from_slice(&0i16.to_be_bytes());
        bytes.extend_from_slice(&0i16.to_be_bytes());
        bytes.extend_from_slice(&100i16.to_be_bytes());
        bytes.extend_from_slice(&100i16.to_be_bytes());
        // component: flags = ARGS_ARE_XY_VALUES (0x0002), glyph=5, arg1=10, arg2=-5
        bytes.extend_from_slice(&0x0002u16.to_be_bytes());
        bytes.extend_from_slice(&5u16.to_be_bytes());
        bytes.push(10u8);
        bytes.push((-5i8) as u8);

        let loca = Loca {
            format: LocaFormat::Long,
            offsets: vec![0, u32::try_from(bytes.len()).unwrap()],
        };
        let glyf = Glyf::parse(&bytes, &loca).expect("valid composite");
        match &glyf.glyphs[0] {
            Glyph::Composite(c) => {
                assert_eq!(
                    c.header,
                    GlyphHeader { x_min: 0, y_min: 0, x_max: 100, y_max: 100 }
                );
                assert_eq!(c.raw_body.len(), 6); // source-of-truth 그대로
                assert_eq!(c.components.len(), 1);
                assert_eq!(c.components[0].glyph_index, 5);
                assert_eq!(
                    c.components[0].args,
                    ComponentArgs::Offset { x: 10, y: -5 }
                );
                assert!(c.instructions.is_empty());
            }
            other => panic!("expected Composite, got {other:?}"),
        }
    }
}
