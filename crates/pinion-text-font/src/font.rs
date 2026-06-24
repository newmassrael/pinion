//! R50.1.2 §5.37.1 — `Font` — sfnt + 6 metadata table 통합 view.
//!
//! `Font::from_bytes(Vec<u8>)` 가 한 번에 (1) sfnt Offset Table + Table Records
//! (R50.1.1) (2) head / OS2 / hhea / hmtx / maxp / post (R50.1.2) 까지 parse
//! 및 검증. 후속 R50.1.3 (cmap) / R50.1.4 (glyf+loca) / R50.1.5 (name) 가
//! 같은 패턴으로 field 추가.

use crate::error::ParseError;
use crate::raster::{Coverage, RasterError, rasterize_glyph_outline};
use crate::sfnt::{OffsetTable, TableRecord, find_table, parse_sfnt};
use crate::shape::ShapedRun;
use crate::tables::cmap::Cmap;
use crate::tables::gdef::{Gdef, GlyphClass};
use crate::tables::glyf::{Glyf, Glyph};
use crate::tables::gpos::Gpos;
use crate::tables::gsub::Gsub;
use crate::tables::head::Head;
use crate::tables::hhea::Hhea;
use crate::tables::hmtx::Hmtx;
use crate::tables::loca::{Loca, LocaFormat};
use crate::tables::maxp::Maxp;
use crate::tables::name::{Name, NameId};
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
    pub name: Name,
    /// GPOS positioning table — `None` when the font has no GPOS (§5.37.6).
    pub gpos: Option<Gpos>,
    /// GSUB substitution table — `None` when the font has no GSUB (§5.37.6).
    pub gsub: Option<Gsub>,
    /// GDEF definition table (glyph classes) — `None` when absent (§5.37.6).
    pub gdef: Option<Gdef>,
}

/// R1079 §5.37 — the vertical metrics (font design units) that size and baseline
/// a single line box: ascent above the baseline, descent below it (negative, the
/// `hhea` / `OS/2` typo sign convention), and the line gap distributed around the
/// line.
///
/// Returned by [`Font::vertical_line_metrics`], which selects between the `hhea`
/// and `OS/2` typographic metrics by the same OpenType / `FreeType` rule parley
/// applies (via `skrifa::metrics::Metrics`), so a §5.37 line box registers with
/// parley's for the same font (cross-shaper box parity — the precondition for
/// flipping the §5.37 engine on by default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerticalLineMetrics {
    /// Ascent above the baseline (positive).
    pub ascender: i16,
    /// Descent below the baseline (negative).
    pub descender: i16,
    /// Line gap — extra leading distributed around the line box.
    pub line_gap: i16,
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
        let name = Name::parse(find_table(&bytes, &records, *b"name")?)?;
        // GPOS / GSUB / GDEF are optional — absent tables yield `None`, a malformed
        // present one propagates its parse error (`optional_table`).
        let gpos = optional_table(&bytes, &records, *b"GPOS", Gpos::parse)?;
        let gsub = optional_table(&bytes, &records, *b"GSUB", Gsub::parse)?;
        let gdef = optional_table(&bytes, &records, *b"GDEF", Gdef::parse)?;

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
            name,
            gpos,
            gsub,
            gdef,
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

    /// Raw `hhea` ascender (design units). This is the unselected `hhea` value
    /// (a faithful raw-table reading); for sizing / baselining a line box use
    /// [`Self::vertical_line_metrics`], which applies the `OS/2`
    /// `USE_TYPO_METRICS` selection parley uses.
    #[must_use]
    pub fn ascender(&self) -> i16 {
        self.hhea.ascender
    }

    /// Raw `hhea` descender (design units, negative). For the line-box-selected
    /// descent see [`Self::vertical_line_metrics`].
    #[must_use]
    pub fn descender(&self) -> i16 {
        self.hhea.descender
    }

    /// Raw `hhea` line gap (design units). For the line-box-selected line gap see
    /// [`Self::vertical_line_metrics`].
    #[must_use]
    pub fn line_gap(&self) -> i16 {
        self.hhea.line_gap
    }

    /// R1079 §5.37 — the vertical metrics for a single line box, selected between
    /// the `hhea` and `OS/2` typographic metrics by the OpenType / `FreeType` rule
    /// parley applies (via `skrifa::metrics::Metrics`), so a §5.37 line box
    /// matches parley's for the same font (cross-shaper box parity).
    ///
    /// The rule (skrifa 0.42.1 `metrics.rs`, after `FreeType` `sfobjs.c`):
    ///
    /// 1. If the `OS/2` `fsSelection` `USE_TYPO_METRICS` bit (bit 7) is set, use
    ///    the `OS/2` `sTypoAscender` / `sTypoDescender` / `sTypoLineGap`.
    /// 2. Otherwise use the `hhea` `ascender` / `descender` / `lineGap`.
    /// 3. If those `hhea` metrics are both zero (a malformed font), fall back to
    ///    the `OS/2` typo metrics when non-zero, else the `OS/2` Windows metrics.
    ///
    /// Many fonts (e.g. `NanumGothic`) set `USE_TYPO_METRICS` with typo metrics
    /// that differ from `hhea`, so reading `hhea` directly (the [`Self::ascender`]
    /// path) sizes the line box differently from parley. Variable-font `MVAR`
    /// metric deltas are not applied — §5.37 shapes the default instance, where
    /// skrifa applies no delta either.
    ///
    /// Single-line scope: the multi-line `line_layout` path still reads `hhea`
    /// directly (it is not wired into the production arms yet) and adopts this
    /// selector when multi-line measure/paint lands.
    #[must_use]
    pub fn vertical_line_metrics(&self) -> VerticalLineMetrics {
        select_line_metrics(&self.os2, &self.hhea)
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
    /// `Glyph::Simple` 가 TrueType simple outline, `Glyph::Composite` 가
    /// subglyph references + transform (header + parsed components + raw body
    /// source-of-truth). `glyph_id >= num_glyphs` 면 `None`.
    #[must_use]
    pub fn glyph_outline(&self, glyph_id: u16) -> Option<&Glyph> {
        self.glyf.glyph(glyph_id)
    }

    /// Rasterize a glyph to a grayscale anti-aliased coverage bitmap at
    /// `px_per_em` pixels per em (§5.37.8). Simple, empty, and composite glyphs
    /// are all supported — a composite's component subglyphs are resolved and
    /// composed into one bitmap. `Empty` glyphs (e.g. space) yield an empty
    /// [`Coverage`].
    ///
    /// # Errors
    ///
    /// * [`RasterError::GlyphNotFound`] — `glyph_id` (or a composite component)
    ///   `>= num_glyphs`.
    /// * [`RasterError::CompositeCycle`] — a composite component chain forms a
    ///   reference cycle or nests past the depth cap.
    /// * [`RasterError::PointMatchUnsupported`] — a composite uses point-matched
    ///   component placement (a later sub-round).
    /// * [`RasterError::SizeExceeded`] — `px_per_em` would produce a bitmap
    ///   larger than the per-axis limit (pathological size).
    pub fn rasterize_glyph(&self, glyph_id: u16, px_per_em: f32) -> Result<Coverage, RasterError> {
        let Some(glyph) = self.glyf.glyph(glyph_id) else {
            return Err(RasterError::GlyphNotFound(glyph_id));
        };
        rasterize_glyph_outline(
            glyph_id,
            glyph,
            &|gid| self.glyf.glyph(gid),
            self.units_per_em(),
            px_per_em,
        )
    }

    /// Apply GSUB substitution to a glyph-id sequence (§5.37.6): `ccmp` single
    /// substitution then `liga` ligatures. Returns one `(glyph, origin)` per
    /// output glyph, where `origin` is the index in `glyphs` of the first
    /// component that produced it (so a caller maps the possibly-fewer outputs
    /// back to source clusters). A font with no GSUB / neither feature returns the
    /// input glyphs 1:1 (`origin = index`).
    #[must_use]
    pub fn substitute_glyphs(&self, glyphs: &[u16]) -> Vec<(u16, usize)> {
        match &self.gsub {
            Some(g) => g.substitute(glyphs),
            None => glyphs.iter().enumerate().map(|(i, &g)| (g, i)).collect(),
        }
    }

    /// First-glyph X-advance kern adjustment in design units for the ordered
    /// glyph pair `(left, right)`, via the GPOS `kern` feature (§5.37.6).
    /// Returns 0 when the font has no GPOS table or no covering kern lookup.
    #[must_use]
    pub fn kern_x_advance(&self, left: u16, right: u16) -> i16 {
        self.gpos
            .as_ref()
            .map_or(0, |g| g.kern_x_advance(left, right))
    }

    /// Design-unit translation `(dx, dy)` (y up) placing combining `mark`'s
    /// anchor onto `base`'s anchor, via the GPOS `mark` feature mark-to-base
    /// lookups (§5.37.6). `None` when the font has no GPOS table or no
    /// mark-to-base lookup attaches this mark to this base.
    #[must_use]
    pub fn mark_offset(&self, base: u16, mark: u16) -> Option<(i16, i16)> {
        self.gpos.as_ref().and_then(|g| g.mark_offset(base, mark))
    }

    /// Design-unit translation `(dx, dy)` (y up) placing combining `mark`'s
    /// anchor onto the preceding `prev_mark`'s anchor, via the GPOS `mkmk`
    /// feature mark-to-mark lookups (§5.37.6) — stacking diacritics. `None` when
    /// the font has no GPOS table or no mark-to-mark lookup stacks this pair.
    #[must_use]
    pub fn mark_mark_offset(&self, prev_mark: u16, mark: u16) -> Option<(i16, i16)> {
        self.gpos
            .as_ref()
            .and_then(|g| g.mark_mark_offset(prev_mark, mark))
    }

    /// `glyph`'s GDEF `GlyphClassDef` category (§5.37.6), or
    /// [`GlyphClass::Unclassified`] when the font has no GDEF / no class table.
    #[must_use]
    pub fn glyph_class(&self, glyph: u16) -> GlyphClass {
        self.gdef
            .as_ref()
            .map_or(GlyphClass::Unclassified, |g| g.glyph_class(glyph))
    }

    /// Whether `glyph` is a GDEF combining mark (class 3). `false` when the font
    /// has no GDEF — callers fall back to attach-based mark recognition.
    #[must_use]
    pub fn is_mark(&self, glyph: u16) -> bool {
        self.gdef.as_ref().is_some_and(|g| g.is_mark(glyph))
    }

    /// Whether the font carries GDEF glyph classes (so [`Self::is_mark`] is
    /// authoritative rather than always `false`).
    #[must_use]
    pub fn has_glyph_classes(&self) -> bool {
        self.gdef.as_ref().is_some_and(Gdef::has_glyph_classes)
    }

    /// Shape `text` into a positioned glyph run at `px_per_em` (§5.37.6: cmap
    /// codepoint → glyph + hmtx advance, refined by GPOS `kern` pair positioning
    /// and `mark` mark-to-base attachment). See [`crate::shape::shape_run`] for
    /// the scope and determinism contract.
    #[must_use]
    pub fn shape_run(&self, text: &str, px_per_em: f32) -> ShapedRun {
        crate::shape::shape_run(self, text, px_per_em)
    }

    /// Render `text` to one composited AA coverage bitmap at `px_per_em`
    /// (§5.37.6) — the first time the engine turns a string into pixels. See
    /// [`crate::shape::render_run`].
    ///
    /// # Errors
    ///
    /// Propagates a [`RasterError`] from any glyph's rasterization (pathological
    /// size or a not-yet-supported composite).
    pub fn render_run(&self, text: &str, px_per_em: f32) -> Result<Coverage, RasterError> {
        crate::shape::render_run(self, text, px_per_em)
    }

    /// Font family name (nameID = 1, Windows Unicode BMP en-US 우선).
    #[must_use]
    pub fn family_name(&self) -> Option<String> {
        self.name.find_string(NameId::FontFamily)
    }

    /// Font subfamily / style (nameID = 2, 예: "Regular" / "Bold").
    #[must_use]
    pub fn subfamily_name(&self) -> Option<String> {
        self.name.find_string(NameId::FontSubfamily)
    }

    /// Full font name (nameID = 4, 예: "Noto Sans Regular").
    #[must_use]
    pub fn full_name(&self) -> Option<String> {
        self.name.find_string(NameId::FullName)
    }

    /// PostScript name (nameID = 6, 예: "NotoSans-Regular"). PDF/PS embedding
    /// 표준 식별자.
    #[must_use]
    pub fn postscript_name(&self) -> Option<String> {
        self.name.find_string(NameId::PostScriptName)
    }
}

/// `OS/2` `fsSelection` bit 7 — when set, the typographic metrics are the
/// preferred line metrics (OpenType spec; `FreeType` / skrifa / parley honour it).
const USE_TYPO_METRICS: u16 = 1 << 7;

/// R1079 §5.37 — select the line-box vertical metrics from `os2` + `hhea`,
/// mirroring `skrifa::metrics::Metrics` (parley's metric source). See
/// [`Font::vertical_line_metrics`] for the rule and rationale. A §5.37 [`Font`]
/// always carries both tables, so skrifa's "OS/2 table exists" guards always hold
/// here (skrifa treats them as optional for fonts that may lack them).
fn select_line_metrics(os2: &Os2, hhea: &Hhea) -> VerticalLineMetrics {
    // (1) USE_TYPO_METRICS set -> OS/2 typographic metrics.
    if os2.fs_selection & USE_TYPO_METRICS != 0 {
        return VerticalLineMetrics {
            ascender: os2.s_typo_ascender,
            descender: os2.s_typo_descender,
            line_gap: os2.s_typo_line_gap,
        };
    }
    // (2) Otherwise the hhea metrics.
    let mut m = VerticalLineMetrics {
        ascender: hhea.ascender,
        descender: hhea.descender,
        line_gap: hhea.line_gap,
    };
    // (3) `FreeType` fallback for a font whose hhea ascent and descent are both
    // zero: the OS/2 typo metrics when non-zero, else the Windows metrics. skrifa
    // leaves the line gap at its hhea value in the Windows branch, so we override
    // only ascender / descender there.
    if m.ascender == 0 && m.descender == 0 {
        if os2.s_typo_ascender != 0 || os2.s_typo_descender != 0 {
            m.ascender = os2.s_typo_ascender;
            m.descender = os2.s_typo_descender;
            m.line_gap = os2.s_typo_line_gap;
        } else {
            // usWinAscent / usWinDescent are positive magnitudes; the descender
            // convention here is negative, so negate the Windows descent. Real
            // fonts keep these within i16; clamp defensively on a pathological
            // value rather than wrap.
            m.ascender = i16::try_from(os2.us_win_ascent).unwrap_or(i16::MAX);
            m.descender = i16::try_from(os2.us_win_descent).map_or(i16::MIN, |d| -d);
        }
    }
    m
}

/// Parse an optional sfnt table: `Ok(None)` when the tag is absent, the parsed
/// value when present, and a propagated [`ParseError`] when a present table is
/// malformed. The single bridge for the optional GPOS / GSUB / GDEF tables.
fn optional_table<T>(
    bytes: &[u8],
    records: &[TableRecord],
    tag: [u8; 4],
    parse: impl FnOnce(&[u8]) -> Result<T, ParseError>,
) -> Result<Option<T>, ParseError> {
    match find_table(bytes, records, tag) {
        Ok(table) => Ok(Some(parse(table)?)),
        // Only an absent table is `None`; any other parse error propagates so a
        // malformed-but-present table is never silently dropped. Matching the
        // variant (rather than `Err(_)`) makes that contract compiler-enforced.
        Err(ParseError::TableNotFound { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod line_metric_tests {
    use super::{Hhea, Os2, USE_TYPO_METRICS, VerticalLineMetrics, select_line_metrics};

    /// Build an `OS/2` v0 table (78 bytes) with the line-metric fields set; the
    /// rest are inert. Field order mirrors the `OS/2` spec / [`Os2::parse`].
    fn os2_table(fs_selection: u16, typo: (i16, i16, i16), win: (u16, u16)) -> Os2 {
        let mut b = Vec::with_capacity(78);
        b.extend_from_slice(&0u16.to_be_bytes()); // version 0
        b.extend_from_slice(&500i16.to_be_bytes()); // x_avg_char_width
        b.extend_from_slice(&400u16.to_be_bytes()); // us_weight_class
        b.extend_from_slice(&5u16.to_be_bytes()); // us_width_class
        b.extend_from_slice(&0u16.to_be_bytes()); // fs_type
        for _ in 0..10 {
            b.extend_from_slice(&0i16.to_be_bytes()); // sub / super / strikeout
        }
        b.extend_from_slice(&0i16.to_be_bytes()); // s_family_class
        b.extend_from_slice(&[0u8; 10]); // panose
        for _ in 0..4 {
            b.extend_from_slice(&0u32.to_be_bytes()); // ul_unicode_range1..4
        }
        b.extend_from_slice(b"TEST"); // ach_vend_id
        b.extend_from_slice(&fs_selection.to_be_bytes());
        b.extend_from_slice(&0x20u16.to_be_bytes()); // us_first_char_index
        b.extend_from_slice(&0xFFFFu16.to_be_bytes()); // us_last_char_index
        b.extend_from_slice(&typo.0.to_be_bytes()); // s_typo_ascender
        b.extend_from_slice(&typo.1.to_be_bytes()); // s_typo_descender
        b.extend_from_slice(&typo.2.to_be_bytes()); // s_typo_line_gap
        b.extend_from_slice(&win.0.to_be_bytes()); // us_win_ascent
        b.extend_from_slice(&win.1.to_be_bytes()); // us_win_descent
        assert_eq!(b.len(), 78);
        Os2::parse(&b).expect("valid v0 OS/2")
    }

    /// Build a minimal `hhea` (36 bytes) with the given line metrics.
    fn hhea_table(ascender: i16, descender: i16, line_gap: i16) -> Hhea {
        let mut b = Vec::with_capacity(36);
        b.extend_from_slice(&1u16.to_be_bytes()); // major
        b.extend_from_slice(&0u16.to_be_bytes()); // minor
        b.extend_from_slice(&ascender.to_be_bytes());
        b.extend_from_slice(&descender.to_be_bytes());
        b.extend_from_slice(&line_gap.to_be_bytes());
        b.extend_from_slice(&1500u16.to_be_bytes()); // advance_width_max
        b.extend_from_slice(&0i16.to_be_bytes()); // min_left_side_bearing
        b.extend_from_slice(&0i16.to_be_bytes()); // min_right_side_bearing
        b.extend_from_slice(&1200i16.to_be_bytes()); // x_max_extent
        b.extend_from_slice(&1i16.to_be_bytes()); // caret_slope_rise
        b.extend_from_slice(&0i16.to_be_bytes()); // caret_slope_run
        b.extend_from_slice(&0i16.to_be_bytes()); // caret_offset
        for _ in 0..4 {
            b.extend_from_slice(&0i16.to_be_bytes()); // reserved
        }
        b.extend_from_slice(&0i16.to_be_bytes()); // metric_data_format
        b.extend_from_slice(&1u16.to_be_bytes()); // number_of_h_metrics
        assert_eq!(b.len(), 36);
        Hhea::parse(&b).expect("valid hhea")
    }

    #[test]
    fn use_typo_metrics_set_selects_os2_typo() {
        // Bit set: OS/2 typo metrics win over a differing hhea (the NanumGothic
        // shape — hhea box 844 - (-156) + 0 = 1000, typo box 856 - (-144) + 250 = 1250).
        let os2 = os2_table(USE_TYPO_METRICS, (856, -144, 250), (885, 198));
        let hhea = hhea_table(844, -156, 0);
        assert_eq!(
            select_line_metrics(&os2, &hhea),
            VerticalLineMetrics {
                ascender: 856,
                descender: -144,
                line_gap: 250
            }
        );
    }

    #[test]
    fn use_typo_metrics_clear_selects_hhea() {
        // Bit clear: hhea wins, the OS/2 typo metrics are ignored.
        let os2 = os2_table(0, (856, -144, 250), (885, 198));
        let hhea = hhea_table(800, -200, 100);
        assert_eq!(
            select_line_metrics(&os2, &hhea),
            VerticalLineMetrics {
                ascender: 800,
                descender: -200,
                line_gap: 100
            }
        );
    }

    #[test]
    fn zero_hhea_falls_back_to_os2_typo() {
        // Bit clear + hhea ascent/descent both zero -> OS/2 typo (when non-zero).
        let os2 = os2_table(0, (820, -180, 90), (900, 200));
        let hhea = hhea_table(0, 0, 0);
        assert_eq!(
            select_line_metrics(&os2, &hhea),
            VerticalLineMetrics {
                ascender: 820,
                descender: -180,
                line_gap: 90
            }
        );
    }

    #[test]
    fn zero_hhea_and_zero_typo_falls_back_to_windows_metrics() {
        // Bit clear + hhea zero + typo zero -> Windows metrics; usWinDescent is a
        // positive magnitude, negated to the signed convention; line gap stays 0.
        let os2 = os2_table(0, (0, 0, 0), (1000, 300));
        let hhea = hhea_table(0, 0, 0);
        assert_eq!(
            select_line_metrics(&os2, &hhea),
            VerticalLineMetrics {
                ascender: 1000,
                descender: -300,
                line_gap: 0
            }
        );
    }
}
