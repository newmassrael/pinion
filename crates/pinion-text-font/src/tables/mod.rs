//! R50.1.2 §5.37.1 — required table parsers (+ R50.6.1 optional GPOS).
//!
//! Microsoft OpenType 1.9.x spec compliant. 각 table 은 fixed-length
//! 또는 number-of-records 가 다른 table 에서 유도되는 variable-length.
//! `gpos` 는 optional layout table — 폰트에 없으면 `Font::gpos` = `None`.

pub mod cmap;
pub mod glyf;
pub mod gpos;
pub mod head;
pub mod hhea;
pub mod hmtx;
pub mod loca;
pub mod maxp;
pub mod name;
pub mod os2;
pub mod post;
