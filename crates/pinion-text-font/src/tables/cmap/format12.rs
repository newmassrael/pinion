//! R50.1.3 §5.37.1 — `cmap` Format 12 (sequential mapping for full Unicode).
//!
//! Microsoft OpenType 1.9.x spec, "cmap subtable Format 12". 32-bit
//! codepoint groups with `start`/`end`/`start_glyph_id` triples. Used for fonts
//! that need to map supplementary plane codepoints (above U+FFFF).

use super::CMAP_TAG;
use crate::error::{FieldValue, ParseError};
use crate::reader::Reader;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Format12 {
    pub language: u32,
    pub groups: Vec<SequentialMapGroup>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct SequentialMapGroup {
    pub start_char_code: u32,
    pub end_char_code: u32,
    pub start_glyph_id: u32,
}

impl Format12 {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, CMAP_TAG);
        let format = r.read_u16()?;
        debug_assert_eq!(format, 12);
        let reserved = r.read_u16()?;
        if reserved != 0 {
            return Err(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format12/reserved",
                value: FieldValue::from_u16(reserved),
            });
        }
        let length = r.read_u32()?;
        let language = r.read_u32()?;
        let num_groups = r.read_u32()?;
        // spec: length = 16 (header) + 12 (group bytes) × num_groups. strict check.
        let expected_length = 16u32
            .checked_add(
                num_groups
                    .checked_mul(12)
                    .ok_or(ParseError::InvalidTableField {
                        tag: CMAP_TAG,
                        field: "format12/numGroups",
                        value: FieldValue::from_u32(num_groups),
                    })?,
            )
            .ok_or(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format12/length-overflow",
                value: FieldValue::from_u32(num_groups),
            })?;
        if length != expected_length {
            return Err(ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format12/length-inconsistent",
                value: FieldValue::from_u32(length),
            });
        }
        let cap = usize::try_from(num_groups).unwrap_or(usize::MAX);
        let mut groups = Vec::with_capacity(cap.min(1_000_000));
        for _ in 0..num_groups {
            let start_char_code = r.read_u32()?;
            let end_char_code = r.read_u32()?;
            let start_glyph_id = r.read_u32()?;
            if start_char_code > end_char_code {
                return Err(ParseError::InvalidTableField {
                    tag: CMAP_TAG,
                    field: "format12/group/start>end",
                    value: FieldValue::from_u32(start_char_code),
                });
            }
            if let Some(prev) = groups.last() {
                let prev_end: u32 = (prev as &SequentialMapGroup).end_char_code;
                if start_char_code <= prev_end {
                    return Err(ParseError::InvalidTableField {
                        tag: CMAP_TAG,
                        field: "format12/groups-not-sorted-or-overlap",
                        value: FieldValue::from_u32(start_char_code),
                    });
                }
            }
            groups.push(SequentialMapGroup {
                start_char_code,
                end_char_code,
                start_glyph_id,
            });
        }
        Ok(Self { language, groups })
    }

    /// Format 12 lookup: binary-search the group containing `codepoint`.
    #[must_use]
    pub fn glyph_id(&self, codepoint: u32) -> Option<u16> {
        let idx = self.groups.partition_point(|g| g.end_char_code < codepoint);
        let group = self.groups.get(idx)?;
        if group.start_char_code > codepoint {
            return None;
        }
        let delta = codepoint - group.start_char_code;
        let glyph = group.start_glyph_id.checked_add(delta)?;
        u16::try_from(glyph).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use super::super::{CMAP_TAG, Cmap};
    use super::*;

    #[test]
    fn format12_supplementary_plane_codepoint() {
        let group = SequentialMapGroup {
            start_char_code: 0x1_F600,
            end_char_code: 0x1_F64F,
            start_glyph_id: 2000,
        };
        let sub = build_format12_simple(&[group]);
        let cmap_bytes = build_cmap_with_subtable(3, 10, &sub);
        let cmap = Cmap::parse(&cmap_bytes).expect("valid cmap");
        assert_eq!(cmap.glyph_id(0x1_F600), Some(2000));
        assert_eq!(cmap.glyph_id(0x1_F64F), Some(2000 + 0x4F));
    }

    #[test]
    fn reject_format12_length_inconsistent() {
        let group = SequentialMapGroup {
            start_char_code: 0xAC00,
            end_char_code: 0xD7A3,
            start_glyph_id: 5000,
        };
        let mut sub = build_format12_simple(&[group]);
        sub[4..8].copy_from_slice(&999u32.to_be_bytes());
        let cmap_bytes = build_cmap_with_subtable(3, 10, &sub);
        let err = Cmap::parse(&cmap_bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format12/length-inconsistent",
                ..
            }
        ));
    }

    #[test]
    fn reject_format12_group_start_gt_end() {
        let group = SequentialMapGroup {
            start_char_code: 0xD000,
            end_char_code: 0xAC00,
            start_glyph_id: 100,
        };
        let sub = build_format12_simple(&[group]);
        let cmap_bytes = build_cmap_with_subtable(3, 10, &sub);
        let err = Cmap::parse(&cmap_bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format12/group/start>end",
                ..
            }
        ));
    }

    #[test]
    fn reject_format12_groups_not_sorted() {
        let groups = [
            SequentialMapGroup {
                start_char_code: 0xD000,
                end_char_code: 0xD0FF,
                start_glyph_id: 200,
            },
            SequentialMapGroup {
                start_char_code: 0xAC00,
                end_char_code: 0xAC0F,
                start_glyph_id: 100,
            },
        ];
        let sub = build_format12_simple(&groups);
        let cmap_bytes = build_cmap_with_subtable(3, 10, &sub);
        let err = Cmap::parse(&cmap_bytes).unwrap_err();
        assert!(matches!(
            err,
            ParseError::InvalidTableField {
                tag: CMAP_TAG,
                field: "format12/groups-not-sorted-or-overlap",
                ..
            }
        ));
    }
}
