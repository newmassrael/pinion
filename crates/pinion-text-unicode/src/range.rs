//! Shared `(start, end, idx)` range-property lookup — the common core
//! of the §5.37.4 `bidi_class`, §5.37.5 `script`, and §5.37.7
//! `line_break_class` UCD range tables.
//!
//! Each property is a table of sorted, non-overlapping
//! `(start, end, class_idx)` ranges keyed by codepoint, looked up by
//! binary search. The first two such tables (`Bidi_Class` +
//! `Line_Break`) kept the loop inline as an explicit twin; the §5.37.5
//! Script table
//! is the third site of this exact shape, so the Rule-of-Three lifts
//! the search here. The caller maps the returned discriminant through
//! its own `from_index` and supplies its own typed default for a miss.

/// Binary-search `ranges` (sorted by `start`, non-overlapping) for the
/// range containing `cp` and return its `class_idx`, or `None` if `cp`
/// falls outside every range.
#[must_use]
pub(crate) fn range_class_index(ranges: &[(u32, u32, u8)], cp: u32) -> Option<u8> {
    // Largest range whose `start <= cp`, then confirm `cp <= end`.
    let mut lo = 0usize;
    let mut hi = ranges.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let (start, end, idx) = ranges[mid];
        if cp < start {
            hi = mid;
        } else if cp > end {
            lo = mid + 1;
        } else {
            return Some(idx);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::range_class_index;

    #[test]
    fn finds_containing_range_endpoints_and_interior() {
        let ranges = &[(0x10u32, 0x20u32, 3u8), (0x30, 0x30, 7), (0x40, 0x4F, 9)];
        // Inclusive endpoints and interior of each range.
        assert_eq!(range_class_index(ranges, 0x10), Some(3));
        assert_eq!(range_class_index(ranges, 0x18), Some(3));
        assert_eq!(range_class_index(ranges, 0x20), Some(3));
        assert_eq!(range_class_index(ranges, 0x30), Some(7)); // singleton range
        assert_eq!(range_class_index(ranges, 0x40), Some(9));
        assert_eq!(range_class_index(ranges, 0x4F), Some(9));
    }

    #[test]
    fn misses_below_above_and_gaps() {
        let ranges = &[(0x10u32, 0x20u32, 3u8), (0x30, 0x30, 7), (0x40, 0x4F, 9)];
        assert_eq!(range_class_index(ranges, 0x0F), None); // below first
        assert_eq!(range_class_index(ranges, 0x21), None); // gap after first
        assert_eq!(range_class_index(ranges, 0x2F), None); // gap before singleton
        assert_eq!(range_class_index(ranges, 0x31), None); // gap after singleton
        assert_eq!(range_class_index(ranges, 0x50), None); // above last
        assert_eq!(range_class_index(&[], 0x10), None); // empty table
    }
}
