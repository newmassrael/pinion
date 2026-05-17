//! R50.1 §5.37.1 `pinion-text-font` — self-hosted OpenType binary parser.
//!
//! parley/swash/fontique/ttf-parser 가 아닌, 자체 OpenType binary
//! 파싱. R50 정신 완전 적용 — 외부 dependency 0개 (thiserror 도 제거).
//!
//! Crate roadmap (R50.1.x sub-phase chain per §5.37.1):
//!
//! * R50.1.1 — sfnt Offset Table + Table Records parser (this commit).
//! * R50.1.2 — head / OS2 / hhea / hmtx / maxp / post metadata.
//! * R50.1.3 — cmap parser (format 4 BMP + format 12 UCS-4).
//! * R50.1.4 — glyf + loca parser (simple TrueType outlines).
//! * R50.1.5 — name table parser (family / style / postscript).
//!
//! Lineage: §5.36 (parley + swash + fontique) Phase 1 bridge →
//! R50.0 §5.37 self-hosted text engine ratify → §5.37.1 OpenType
//! parser sub-scope → R50.1.1 sfnt foundation (this).

mod error;
mod sfnt;

pub use error::ParseError;
pub use sfnt::{Flavor as SfntFlavor, OffsetTable, TableRecord, parse_sfnt};
