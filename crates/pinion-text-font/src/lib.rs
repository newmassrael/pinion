//! R50.1 §5.37.1 `pinion-text-font` — self-hosted OpenType binary parser.
//!
//! parley/swash/fontique/ttf-parser 가 아닌, 자체 OpenType binary
//! 파싱. R50 정신 완전 적용 — 외부 dependency 0개 (thiserror 도 제거).
//!
//! Crate roadmap (R50.1.x sub-phase chain per §5.37.1):
//!
//! * R50.1.1 — sfnt Offset Table + Table Records parser.
//! * R50.1.2 — head / OS2 / hhea / hmtx / maxp / post metadata.
//! * R50.1.3 — cmap parser (format 4 BMP + format 12 UCS-4).
//! * R50.1.4 — glyf + loca parser (simple TrueType outlines).
//!     * R50.1.4.1 — loca short/long + glyf simple (this commit).
//!     * R50.1.4.2 — glyf composite (subglyph + transform + cycle detection).
//! * R50.1.5 — name table parser (family / style / postscript).
//! * R50.8 §5.37.8 — glyph rasterizer (`raster`): simple-glyph outline → AA
//!   coverage bitmap via signed-area accumulation. First pixel-producing layer.
//!
//! Lineage: §5.36 (parley + swash + fontique) Phase 1 bridge →
//! R50.0 §5.37 self-hosted text engine ratify → §5.37.1 OpenType
//! parser sub-scope → R50.1.1 sfnt foundation.

pub mod atlas;
mod error;
pub mod fallback;
mod font;
pub mod line_layout;
pub mod paragraph;
pub mod raster;
mod reader;
mod sfnt;
pub mod shape;
pub mod tables;
pub mod wrap;

pub use atlas::{AtlasGlyph, GlyphAtlas};
pub use error::{FieldValue, ParseError};
pub use fallback::{FallbackRun, FontRun, FontRunRange, font_runs, shape_with_fallback};
pub use font::{Font, VerticalLineMetrics};
pub use line_layout::{
    ShapedLine, ShapedLines, layout_paragraph, layout_paragraph_with_fallback, render_lines,
};
pub use paragraph::{
    ShapedParagraph, render_paragraph, render_paragraph_atlased, shape_paragraph,
    shape_paragraph_with_fallback,
};
pub use raster::{Coverage, RasterError};
pub use sfnt::{Flavor as SfntFlavor, OffsetTable, TableRecord, find_table, parse_sfnt};
pub use shape::{PlacedGlyph, PositionedGlyph, RenderedGlyphs, ShapedRun};
pub use tables::gdef::{Gdef, GlyphClass};
pub use tables::glyf::{
    Component, ComponentArgs, ComponentTransform, CompositeGlyph, Glyf, Glyph, GlyphHeader,
    GlyphPoint, SimpleGlyph,
};
pub use tables::gpos::Gpos;
pub use tables::gsub::Gsub;
pub use tables::loca::{Loca, LocaFormat};
pub use tables::name::{LangTagRecord, Name, NameId, NameRecord};
pub use wrap::{LineRange, wrap_paragraph, wrap_paragraph_with_fallback};
