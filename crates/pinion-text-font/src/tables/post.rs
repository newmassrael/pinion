//! R50.1.2 §5.37.1 — `post` table (PostScript naming).
//!
//! Microsoft OpenType 1.9.x spec, "post" chapter.
//!
//! Versions handled:
//!
//! * **v1.0** (`0x00010000`) — 32 byte header only, uses standard Mac glyph names.
//! * **v2.0** (`0x00020000`) — 32 byte header + custom glyph name table (deferred).
//! * **v3.0** (`0x00030000`) — 32 byte header only, no glyph names at all.
//!
//! Versions rejected:
//!
//! * **v2.5** (`0x00025000`) — deprecated by Apple.
//! * **v4.0** (`0x00040000`) — Apple-specific bitmap-only fonts.
//!
//! v2.0 glyph name array parsing is deferred to a later R50.1.X sub-round —
//! family/style/postscript-name discovery 는 `name` table (R50.1.5) 가 책임.

use crate::error::ParseError;
use crate::reader::Reader;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Post {
    pub version_fixed: i32,
    pub italic_angle_fixed: i32,
    pub underline_position: i16,
    pub underline_thickness: i16,
    /// 0 = proportional, non-zero = monospace.
    pub is_fixed_pitch: u32,
    pub min_mem_type_42: u32,
    pub max_mem_type_42: u32,
    pub min_mem_type_1: u32,
    pub max_mem_type_1: u32,
}

const POST_TAG: [u8; 4] = *b"post";
const POST_V10: i32 = 0x0001_0000;
const POST_V20: i32 = 0x0002_0000;
const POST_V25: i32 = 0x0002_5000;
const POST_V30: i32 = 0x0003_0000;
const POST_V40: i32 = 0x0004_0000;

impl Post {
    /// Parse the post table header (32 bytes).
    ///
    /// # Errors
    ///
    /// * [`ParseError::TableTooShort`] — fewer than 32 bytes.
    /// * [`ParseError::UnsupportedTableVersion`] — version ∉ {1.0, 2.0, 3.0}.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, POST_TAG);
        let version_fixed = r.read_i32()?;
        // v1.0/v2.0/v3.0 만 accept. v2.5 (deprecated), v4.0 (Apple-specific),
        // 그 외 unknown 모두 동일한 UnsupportedTableVersion 으로 reject —
        // ParseError 의 major/minor 가 진단 context 보유.
        if !matches!(version_fixed, POST_V10 | POST_V20 | POST_V30) {
            #[allow(clippy::cast_sign_loss)]
            let major = (version_fixed >> 16) as u16;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let minor = (version_fixed & 0xFFFF) as u16;
            return Err(ParseError::UnsupportedTableVersion {
                tag: POST_TAG,
                major,
                minor,
            });
        }
        // v2.5/v4.0 의 spec 의도 명시 (코드 흐름에는 영향 없음 — 위 reject 가 모두 처리):
        let _ = (POST_V25, POST_V40);

        let italic_angle_fixed = r.read_i32()?;
        let underline_position = r.read_i16()?;
        let underline_thickness = r.read_i16()?;
        let is_fixed_pitch = r.read_u32()?;
        let min_mem_type_42 = r.read_u32()?;
        let max_mem_type_42 = r.read_u32()?;
        let min_mem_type_1 = r.read_u32()?;
        let max_mem_type_1 = r.read_u32()?;

        Ok(Self {
            version_fixed,
            italic_angle_fixed,
            underline_position,
            underline_thickness,
            is_fixed_pitch,
            min_mem_type_42,
            max_mem_type_42,
            min_mem_type_1,
            max_mem_type_1,
        })
    }

    /// `true` if `is_fixed_pitch` indicates a monospace font.
    #[must_use]
    pub fn is_monospace(&self) -> bool {
        self.is_fixed_pitch != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_post(version: i32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(32);
        bytes.extend_from_slice(&version.to_be_bytes());
        bytes.extend_from_slice(&0i32.to_be_bytes()); // italic_angle (0.0)
        bytes.extend_from_slice(&(-100i16).to_be_bytes()); // underline_position
        bytes.extend_from_slice(&50i16.to_be_bytes()); // underline_thickness
        bytes.extend_from_slice(&0u32.to_be_bytes()); // is_fixed_pitch (0 = proportional)
        bytes.extend_from_slice(&0u32.to_be_bytes()); // min_mem_42
        bytes.extend_from_slice(&0u32.to_be_bytes()); // max_mem_42
        bytes.extend_from_slice(&0u32.to_be_bytes()); // min_mem_1
        bytes.extend_from_slice(&0u32.to_be_bytes()); // max_mem_1
        bytes
    }

    #[test]
    fn parse_v10_header() {
        let bytes = build_post(POST_V10);
        let post = Post::parse(&bytes).expect("valid v1.0");
        assert_eq!(post.version_fixed, POST_V10);
        assert_eq!(post.underline_position, -100);
        assert!(!post.is_monospace());
    }

    #[test]
    fn parse_v20_header() {
        let bytes = build_post(POST_V20);
        let post = Post::parse(&bytes).expect("valid v2.0 header");
        assert_eq!(post.version_fixed, POST_V20);
    }

    #[test]
    fn parse_v30_header() {
        let bytes = build_post(POST_V30);
        let post = Post::parse(&bytes).expect("valid v3.0");
        assert_eq!(post.version_fixed, POST_V30);
    }

    #[test]
    fn reject_v25() {
        let bytes = build_post(POST_V25);
        let err = Post::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::UnsupportedTableVersion {
                tag: POST_TAG,
                major: 2,
                minor: 0x5000,
            }
        );
    }

    #[test]
    fn reject_v40() {
        let bytes = build_post(POST_V40);
        let err = Post::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::UnsupportedTableVersion {
                tag: POST_TAG,
                major: 4,
                minor: 0,
            }
        );
    }

    #[test]
    fn reject_unknown_version() {
        let bytes = build_post(0x0005_0000);
        let err = Post::parse(&bytes).unwrap_err();
        assert_eq!(
            err,
            ParseError::UnsupportedTableVersion {
                tag: POST_TAG,
                major: 5,
                minor: 0,
            }
        );
    }

    #[test]
    fn reject_table_too_short() {
        // valid v1.0 magic at start so version check passes — then short reads fail.
        let mut bytes = build_post(POST_V10);
        bytes.truncate(20);
        let err = Post::parse(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::TableTooShort { tag: POST_TAG, .. }
        ));
    }

    #[test]
    fn is_monospace_detects_nonzero() {
        let mut bytes = build_post(POST_V10);
        bytes[12..16].copy_from_slice(&1u32.to_be_bytes());
        let post = Post::parse(&bytes).expect("valid");
        assert!(post.is_monospace());
    }
}
