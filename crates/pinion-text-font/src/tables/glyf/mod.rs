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

mod simple;
#[cfg(test)]
mod test_helpers;

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

/// Composite glyph — R50.1.4.1 에서는 header + raw body 만 보존.
///
/// R50.1.4.2 에서 `components: Vec<Component>` field 추가 + body parse. 현재는
/// body 가 `raw_body: Vec<u8>` 로 그대로 — real font integration sweep 시
/// composite glyph 만나도 panic 없이 통과.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CompositeGlyph {
    pub header: GlyphHeader,
    /// glyph header (10 byte) 뒤의 나머지 raw bytes. R50.1.4.2 에서 component
    /// records / transform matrix 로 변환.
    pub raw_body: Vec<u8>,
}

/// Glyph data variant — empty / simple / composite-unparsed.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Glyph {
    /// `loca[i] == loca[i+1]` — glyph 가 시각적 표현 없음.
    Empty,
    Simple(SimpleGlyph),
    /// R50.1.4.1 placeholder — header 만 parse, body 는 R50.1.4.2 에서 합치.
    Composite(CompositeGlyph),
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

    if num_contours_raw >= 0 {
        // simple glyph
        #[allow(clippy::cast_sign_loss)]
        let num_contours = num_contours_raw as u16;
        Ok(Glyph::Simple(simple::parse_simple(
            &mut r,
            header,
            num_contours,
        )?))
    } else {
        // composite glyph — R50.1.4.1 placeholder: header 만 parse, body 보존.
        // R50.1.4.2 에서 numberOfContours == -1 strict check + component parse.
        let body_start = r.position();
        let raw_body = bytes[body_start..].to_vec();
        Ok(Glyph::Composite(CompositeGlyph { header, raw_body }))
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
    fn parse_composite_placeholder() {
        // numberOfContours = -1, bbox valid, 4-byte body remainder.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(-1i16).to_be_bytes()); // numberOfContours = -1
        bytes.extend_from_slice(&0i16.to_be_bytes());
        bytes.extend_from_slice(&0i16.to_be_bytes());
        bytes.extend_from_slice(&100i16.to_be_bytes());
        bytes.extend_from_slice(&100i16.to_be_bytes());
        bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // raw body

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
                assert_eq!(c.raw_body, vec![0xDE, 0xAD, 0xBE, 0xEF]);
            }
            other => panic!("expected Composite, got {other:?}"),
        }
    }
}
