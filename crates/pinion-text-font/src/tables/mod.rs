//! R50.1.2 §5.37.1 — required table parsers.
//!
//! Microsoft OpenType 1.9.x spec compliant. 각 table 은 fixed-length
//! 또는 number-of-records 가 다른 table 에서 유도되는 variable-length.

pub mod head;
pub mod hhea;
pub mod hmtx;
pub mod maxp;
pub mod os2;
pub mod post;
