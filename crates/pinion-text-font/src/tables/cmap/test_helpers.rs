//! R50.1.3.2 §5.37.1 — shared test fixture builders for cmap submodules.
//!
//! `cmap/{mod,format4,format12}.rs` 의 unit tests 가 함께 사용하는 byte
//! buffer 빌더. `pub(super)` 하나로 cmap 모듈 family 안에서만 노출.

use super::format12::SequentialMapGroup;

pub(super) fn build_format4_simple(start: u16, end: u16, id_delta: i16) -> Vec<u8> {
    let seg_count: u16 = 2;
    let seg_count_x2 = seg_count * 2;
    let length: u16 = 14 + 8 * seg_count + 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&4u16.to_be_bytes()); // format
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes()); // language
    bytes.extend_from_slice(&seg_count_x2.to_be_bytes());
    bytes.extend_from_slice(&4u16.to_be_bytes()); // searchRange (canonical for segCount=2)
    bytes.extend_from_slice(&1u16.to_be_bytes()); // entrySelector
    bytes.extend_from_slice(&0u16.to_be_bytes()); // rangeShift
    bytes.extend_from_slice(&end.to_be_bytes());
    bytes.extend_from_slice(&0xFFFFu16.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
    bytes.extend_from_slice(&start.to_be_bytes());
    bytes.extend_from_slice(&0xFFFFu16.to_be_bytes());
    bytes.extend_from_slice(&id_delta.to_be_bytes());
    bytes.extend_from_slice(&1i16.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes
}

pub(super) fn build_format12_simple(groups: &[SequentialMapGroup]) -> Vec<u8> {
    let num_groups = u32::try_from(groups.len()).expect("test groups < u32::MAX");
    let length: u32 = 16 + 12 * num_groups;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&12u16.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(&num_groups.to_be_bytes());
    for g in groups {
        bytes.extend_from_slice(&g.start_char_code.to_be_bytes());
        bytes.extend_from_slice(&g.end_char_code.to_be_bytes());
        bytes.extend_from_slice(&g.start_glyph_id.to_be_bytes());
    }
    bytes
}

pub(super) fn build_cmap_with_subtable(
    platform_id: u16,
    encoding_id: u16,
    subtable_bytes: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0u16.to_be_bytes()); // version
    bytes.extend_from_slice(&1u16.to_be_bytes()); // numTables
    bytes.extend_from_slice(&platform_id.to_be_bytes());
    bytes.extend_from_slice(&encoding_id.to_be_bytes());
    let subtable_offset: u32 = 4 + 8;
    bytes.extend_from_slice(&subtable_offset.to_be_bytes());
    bytes.extend_from_slice(subtable_bytes);
    bytes
}
