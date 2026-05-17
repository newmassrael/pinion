//! R50.1.2 §5.37.1 — `Font` — sfnt + 6 metadata table 통합 view.
//!
//! `Font::from_bytes(Vec<u8>)` 가 한 번에 (1) sfnt Offset Table + Table Records
//! (R50.1.1) (2) head / OS2 / hhea / hmtx / maxp / post (R50.1.2) 까지 parse
//! 및 검증. 후속 R50.1.3 (cmap) / R50.1.4 (glyf+loca) / R50.1.5 (name) 가
//! 같은 패턴으로 field 추가.

use crate::error::ParseError;
use crate::sfnt::{OffsetTable, TableRecord, find_table, parse_sfnt};
use crate::tables::cmap::Cmap;
use crate::tables::glyf::{Glyf, Glyph};
use crate::tables::head::Head;
use crate::tables::hhea::Hhea;
use crate::tables::hmtx::Hmtx;
use crate::tables::loca::{Loca, LocaFormat};
use crate::tables::maxp::Maxp;
use crate::tables::os2::Os2;
use crate::tables::post::Post;

/// 통합 Font view — sfnt + 6 metadata + cmap + loca + glyf.
///
/// `bytes` 는 raw font binary 의 owned copy. R50.1.3+ 의 cmap/glyf/name 추가
/// 시 raw bytes 재참조 (offset/length 만 record 보관) 위함.
#[derive(Debug, Clone)]
pub struct Font {
    bytes: Vec<u8>,
    pub offset_table: OffsetTable,
    pub records: Vec<TableRecord>,
    pub head: Head,
    pub hhea: Hhea,
    pub hmtx: Hmtx,
    pub maxp: Maxp,
    pub os2: Os2,
    pub post: Post,
    pub cmap: Cmap,
    pub loca: Loca,
    pub glyf: Glyf,
}

impl Font {
    /// Parse a font from raw bytes.
    ///
    /// `bytes` 는 owned — Font 는 raw bytes 의 copy 를 lifetime 내내 유지하여
    /// 후속 R50.1.3+ table 들이 자유롭게 view 작성 가능.
    ///
    /// # Errors
    ///
    /// * [`ParseError`] — sfnt parse 또는 6 metadata table 중 한 곳에서 fail.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ParseError> {
        let (offset_table, records) = parse_sfnt(&bytes)?;

        let head = Head::parse(find_table(&bytes, &records, *b"head")?)?;
        let maxp = Maxp::parse(find_table(&bytes, &records, *b"maxp")?)?;
        let hhea = Hhea::parse(find_table(&bytes, &records, *b"hhea")?)?;
        let hmtx = Hmtx::parse(
            find_table(&bytes, &records, *b"hmtx")?,
            hhea.number_of_h_metrics,
            maxp.num_glyphs,
        )?;
        let os2 = Os2::parse(find_table(&bytes, &records, *b"OS/2")?)?;
        let post = Post::parse(find_table(&bytes, &records, *b"post")?)?;
        let cmap = Cmap::parse(find_table(&bytes, &records, *b"cmap")?)?;
        let loca_format = LocaFormat::from_head_value(head.index_to_loc_format)?;
        let loca = Loca::parse(
            find_table(&bytes, &records, *b"loca")?,
            loca_format,
            maxp.num_glyphs,
        )?;
        let glyf = Glyf::parse(find_table(&bytes, &records, *b"glyf")?, &loca)?;

        Ok(Self {
            bytes,
            offset_table,
            records,
            head,
            hhea,
            hmtx,
            maxp,
            os2,
            post,
            cmap,
            loca,
            glyf,
        })
    }

    /// raw font binary (immutable borrow).
    #[must_use]
    pub fn raw_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Design grid units per em — from head.
    #[must_use]
    pub fn units_per_em(&self) -> u16 {
        self.head.units_per_em
    }

    /// Glyph 개수 — from maxp.
    #[must_use]
    pub fn num_glyphs(&self) -> u16 {
        self.maxp.num_glyphs
    }

    /// 추천 typographic ascender — from hhea. Apple-style 동기화면 OS/2.sTypoAscender 도 동일.
    #[must_use]
    pub fn ascender(&self) -> i16 {
        self.hhea.ascender
    }

    /// 추천 typographic descender (음수) — from hhea.
    #[must_use]
    pub fn descender(&self) -> i16 {
        self.hhea.descender
    }

    /// Line gap — from hhea.
    #[must_use]
    pub fn line_gap(&self) -> i16 {
        self.hhea.line_gap
    }

    /// Glyph advance width in design units. None if `glyph_id` ≥ `num_glyphs`.
    #[must_use]
    pub fn glyph_advance_width(&self, glyph_id: u16) -> Option<u16> {
        self.hmtx.advance_width(glyph_id)
    }

    /// Glyph left side bearing in design units. None if `glyph_id` ≥ `num_glyphs`.
    #[must_use]
    pub fn glyph_left_side_bearing(&self, glyph_id: u16) -> Option<i16> {
        self.hmtx.left_side_bearing(glyph_id)
    }

    /// OS/2 weight class (1..=1000, with 400 = Regular, 700 = Bold).
    #[must_use]
    pub fn weight_class(&self) -> u16 {
        self.os2.us_weight_class
    }

    /// Monospace ↔ proportional. From `post.is_fixed_pitch`.
    #[must_use]
    pub fn is_monospace(&self) -> bool {
        self.post.is_monospace()
    }

    /// Map a Unicode codepoint to a glyph ID via the best cmap subtable.
    /// Returns `None` if the codepoint isn't mapped.
    #[must_use]
    pub fn glyph_id_for(&self, codepoint: u32) -> Option<u16> {
        self.cmap.glyph_id(codepoint)
    }

    /// Parsed glyph outline by glyph ID. `Glyph::Empty` 가 빈 글리프 표현체,
    /// `Glyph::Simple` 가 TrueType simple outline, `Glyph::Composite` 는
    /// R50.1.4.1 시점 header-only placeholder (R50.1.4.2 에서 fully parse).
    /// `glyph_id >= num_glyphs` 면 `None`.
    #[must_use]
    pub fn glyph_outline(&self, glyph_id: u16) -> Option<&Glyph> {
        self.glyf.glyph(glyph_id)
    }
}
