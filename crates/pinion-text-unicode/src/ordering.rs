//! UAX #15 §3 — Canonical Ordering Algorithm.
//!
//! Within each maximal run of consecutive non-starter codepoints
//! (`CCC > 0`), stable-sort by combining class ascending. Starter
//! codepoints (`CCC == 0`) act as boundaries — they are never moved,
//! and the algorithm never crosses a starter.

use crate::tables::{
    CANONICAL_COMBINING_CLASS_BMP_DATA, CANONICAL_COMBINING_CLASS_BMP_INDEX,
    CANONICAL_COMBINING_CLASS_FIRST_CP,
    CANONICAL_COMBINING_CLASS_SUPPLEMENTARY,
};

/// Canonical combining class of `c`. Codepoints absent from the
/// table have CCC 0 (the default per UAX #44).
///
/// R50.2.9 — codepoints below
/// [`CANONICAL_COMBINING_CLASS_FIRST_CP`] bypass every table access
/// and return 0 immediately. UAX #44 §3 stability forbids back-
/// porting non-zero CCC entries below the existing minimum, so the
/// short-circuit is forward-stable.
///
/// R50.2.10 — BMP codepoints above the anchor go through a 2-stage
/// trie (`BMP_INDEX[c >> 8] -> block`; `BMP_DATA[block * 256 +
/// (c & 0xFF)]`), reducing the per-character cost to two memory
/// accesses with no branches. Supplementary-plane codepoints
/// (`c >= 0x10000`) fall back to a sparse `binary_search` over a
/// few hundred entries, which is fine because supplementary text
/// rarely dominates the hot path.
#[inline]
pub(crate) fn combining_class(c: u32) -> u8 {
    if c < CANONICAL_COMBINING_CLASS_FIRST_CP {
        return 0;
    }
    if c < 0x10000 {
        let block =
            CANONICAL_COMBINING_CLASS_BMP_INDEX[(c >> 8) as usize] as usize;
        return CANONICAL_COMBINING_CLASS_BMP_DATA
            [block * 256 + (c & 0xFF) as usize];
    }
    match CANONICAL_COMBINING_CLASS_SUPPLEMENTARY
        .binary_search_by_key(&c, |(cp, _)| *cp)
    {
        Ok(idx) => CANONICAL_COMBINING_CLASS_SUPPLEMENTARY[idx].1,
        Err(_) => 0,
    }
}

/// Apply the Canonical Ordering Algorithm to `buf` in place. Stable
/// within each non-starter run preserves UCD ordering for equal CCC
/// values (UAX #15 conformance).
pub(crate) fn canonical_ordering(buf: &mut [u32]) {
    let mut i = 0;
    while i < buf.len() {
        if combining_class(buf[i]) == 0 {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < buf.len() && combining_class(buf[j]) != 0 {
            j += 1;
        }
        buf[i..j].sort_by_key(|&c| combining_class(c));
        i = j;
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_ordering, combining_class};

    #[test]
    fn starter_ccc_zero() {
        assert_eq!(combining_class(0x0041), 0); // 'A'
        assert_eq!(combining_class(0x1100), 0); // ᄀ Hangul L
    }

    #[test]
    fn combining_grave_ccc_230() {
        assert_eq!(combining_class(0x0300), 230);
    }

    #[test]
    fn combining_dot_below_ccc_220() {
        assert_eq!(combining_class(0x0323), 220);
    }

    #[test]
    fn ordering_swaps_misordered_marks() {
        // D + dot-above (230) + dot-below (220). Algorithm must
        // move dot-below before dot-above.
        let mut buf: Vec<u32> = vec![0x0044, 0x0307, 0x0323];
        canonical_ordering(&mut buf);
        assert_eq!(buf, vec![0x0044, 0x0323, 0x0307]);
    }

    #[test]
    fn ordering_stable_for_equal_ccc() {
        // Two marks of equal CCC must preserve relative order.
        // U+0301 (acute) and U+0302 (circumflex) both have CCC 230.
        let mut buf: Vec<u32> = vec![0x0061, 0x0302, 0x0301];
        canonical_ordering(&mut buf);
        assert_eq!(buf, vec![0x0061, 0x0302, 0x0301]);
    }

    #[test]
    fn starter_blocks_run() {
        // Starter between two non-starter runs prevents the second
        // run from being sorted into the first.
        let mut buf: Vec<u32> =
            vec![0x0061, 0x0307, 0x0062, 0x0323];
        canonical_ordering(&mut buf);
        assert_eq!(buf, vec![0x0061, 0x0307, 0x0062, 0x0323]);
    }
}
