//! UAX #15 §1.3 / §3 — Canonical Composition Algorithm.
//!
//! Applies primary-composite + Hangul algorithmic composition to a
//! buffer that has already been canonically decomposed and
//! canonically ordered. Operates in place; the result is the NFC
//! form of the input.
//!
//! The "blocked" relation (UAX #15 D6): for a starter `L` and a
//! candidate `C` later in the sequence, `C` is blocked from `L`
//! iff some character `B` between them has `CCC(B) == 0` or
//! `CCC(B) >= CCC(C)`. After canonical ordering the buffer contains
//! no `CCC == 0` intermediates between two starters, so the check
//! reduces to comparing `CCC(C)` against the maximum CCC observed
//! since `L`.
//!
//! **Complexity**: `O(n)` — the implementation uses a `read` /
//! `write` two-pointer in-place compact pattern. Consumed
//! codepoints (those merged into a composite) are simply skipped
//! by the read pointer; the write pointer trails behind, never
//! emitting an array shift. The earlier `Vec::remove(i)` variant
//! was `O(n²)` for long combining-mark runs (R50.2.7 textbook
//! debt repayment).

use crate::hangul::compose_hangul;
use crate::ordering::combining_class;
use crate::tables::{
    PRIMARY_COMPOSITES_BC_DATA, PRIMARY_COMPOSITES_BMP_DATA,
    PRIMARY_COMPOSITES_BMP_INDEX, PRIMARY_COMPOSITES_FIRST_A_CP,
    PRIMARY_COMPOSITES_LAST_A_CP, PRIMARY_COMPOSITES_SUPPLEMENTARY,
};

/// Attempt to compose `(a, b)` into a single codepoint using
/// algorithmic Hangul composition first, then the UCD-derived
/// primary-composite map. Returns `None` when no composite exists.
///
/// R50.2.9 — when `a` falls outside the `[FIRST_A_CP, LAST_A_CP]`
/// envelope of the primary-composite table, the table search is
/// skipped entirely (Hangul composition stays in-band because the
/// Hangul leading-jamo range U+1100..U+1112 sits inside the
/// envelope). UAX #44 §3 stability forbids extending the
/// primary-composite `a`-range beyond the existing envelope, so
/// the short-circuit is forward-stable.
///
/// R50.2.14 — the table search is now a two-level BMP trie: the
/// `a` codepoint indexes through
/// `PRIMARY_COMPOSITES_BMP_INDEX` / `_BMP_DATA` to a per-`a`
/// sub-table inside `PRIMARY_COMPOSITES_BC_DATA`; the sub-table
/// itself is sorted by `b` and served by `binary_search` over a
/// 1\u{2013}20 entry slice. Replaces the R50.2.2 flat
/// `binary_search` over ~1100 entries with two memory reads plus
/// a small per-`a` `binary_search` (~3\u{2013}5 iterations), which
/// is the dominant compose-pair speedup for combining-mark heavy
/// text such as Latin diacritic recomposition.
pub(crate) fn compose_pair(a: u32, b: u32) -> Option<u32> {
    if let Some(h) = compose_hangul(a, b) {
        return Some(h);
    }
    if !(PRIMARY_COMPOSITES_FIRST_A_CP..=PRIMARY_COMPOSITES_LAST_A_CP)
        .contains(&a)
    {
        return None;
    }
    if a < 0x10000 {
        let block = PRIMARY_COMPOSITES_BMP_INDEX[(a >> 8) as usize] as usize;
        let packed =
            PRIMARY_COMPOSITES_BMP_DATA[block * 256 + (a & 0xFF) as usize];
        if packed == 0 {
            return None;
        }
        let length = (packed >> 24) as usize;
        let offset = (packed & 0x00FF_FFFF) as usize;
        let sub = &PRIMARY_COMPOSITES_BC_DATA[offset..offset + length];
        return sub
            .binary_search_by_key(&b, |(b2, _)| *b2)
            .ok()
            .map(|idx| sub[idx].1);
    }
    compose_pair_supplementary(a, b)
}

/// R50.2.14 — sparse supplementary-plane fallback for
/// [`compose_pair`]. Kept `#[inline(never)]` (R50.2.12 / R50.2.13
/// pattern) because (a) it is the cold path — supplementary `a`
/// codepoints are vanishingly rare in real text — and (b)
/// folding the outer `binary_search` IR back into the BMP hot
/// path would inflate [`compose_pair`] past the LLVM inline
/// threshold, costing every compose probe on Latin / Greek / etc.
#[inline(never)]
fn compose_pair_supplementary(a: u32, b: u32) -> Option<u32> {
    let idx = PRIMARY_COMPOSITES_SUPPLEMENTARY
        .binary_search_by_key(&a, |(k, _)| *k)
        .ok()?;
    let sub = PRIMARY_COMPOSITES_SUPPLEMENTARY[idx].1;
    sub.binary_search_by_key(&b, |(b2, _)| *b2)
        .ok()
        .map(|i| sub[i].1)
}

/// Apply the Canonical Composition Algorithm to `buf` in place.
/// The input must already be in canonical decomposition +
/// canonical ordering form (i.e. the output of
/// [`crate::nfd::nfd`]'s pipeline up to but not including this
/// stage). Runs in `O(n)`: a `read` cursor walks the buffer once
/// while a `write` cursor trails — composed codepoints overwrite
/// their starter in place, retained codepoints are copied down to
/// the write position.
pub(crate) fn canonical_composition(buf: &mut Vec<u32>) {
    if buf.is_empty() {
        return;
    }

    // Leading non-starters (CCC > 0 before the first starter) are
    // copied through untouched; composition starts from the first
    // starter.
    let mut first_starter = 0;
    while first_starter < buf.len() && combining_class(buf[first_starter]) != 0
    {
        first_starter += 1;
    }
    if first_starter >= buf.len() {
        return; // All non-starters; nothing to compose.
    }

    // `starter_pos` indexes the most recent starter in the output
    // (the "compose-into" target). `read` walks input; `write`
    // emits retained output. They start one past the first
    // starter, which itself occupies `first_starter` in both.
    let mut starter_pos = first_starter;
    let mut max_class_since_starter: u8 = 0;
    let mut has_intermediate = false;
    let mut read = first_starter + 1;
    let mut write = first_starter + 1;

    while read < buf.len() {
        let c = buf[read];
        let c_class = combining_class(c);
        let blocked =
            has_intermediate && max_class_since_starter >= c_class;

        if !blocked {
            if let Some(p) = compose_pair(buf[starter_pos], c) {
                buf[starter_pos] = p;
                read += 1; // consume c; `write` stays
                continue;
            }
        }

        if c_class == 0 {
            // c becomes the new composition target. After we
            // write it to `write`, that position is the starter.
            starter_pos = write;
            max_class_since_starter = 0;
            has_intermediate = false;
        } else {
            has_intermediate = true;
            if c_class > max_class_since_starter {
                max_class_since_starter = c_class;
            }
        }
        buf[write] = c;
        write += 1;
        read += 1;
    }
    buf.truncate(write);
}

#[cfg(test)]
mod tests {
    use super::canonical_composition;

    #[test]
    fn empty_buffer_no_op() {
        let mut buf: Vec<u32> = Vec::new();
        canonical_composition(&mut buf);
        assert!(buf.is_empty());
    }

    #[test]
    fn ascii_unchanged() {
        let mut buf: Vec<u32> = "hello".chars().map(|c| c as u32).collect();
        let original = buf.clone();
        canonical_composition(&mut buf);
        assert_eq!(buf, original);
    }

    #[test]
    fn composes_a_grave_back() {
        // A (U+0041) + combining grave (U+0300) → À (U+00C0).
        let mut buf: Vec<u32> = vec![0x0041, 0x0300];
        canonical_composition(&mut buf);
        assert_eq!(buf, vec![0x00C0]);
    }

    #[test]
    fn composes_hangul_l_v_t() {
        // ᄒ + ᅡ + ᆫ → 한 (U+D55C) via two algorithmic steps.
        let mut buf: Vec<u32> = vec![0x1112, 0x1161, 0x11AB];
        canonical_composition(&mut buf);
        assert_eq!(buf, vec![0xD55C]);
    }

    #[test]
    fn first_compatible_mark_consumes_then_second_stays() {
        // Canonical-ordered input [A, dot-below (220), grave (230)].
        // The algorithm composes the first adjacent pair (A,
        // dot-below) → Ạ (U+1EA0). grave then probes the new
        // composed starter, but Ạ̀ has no precomposed codepoint, so
        // grave stays. Result: [Ạ, grave].
        let mut buf: Vec<u32> = vec![0x0041, 0x0323, 0x0300];
        canonical_composition(&mut buf);
        assert_eq!(buf, vec![0x1EA0, 0x0300]);
    }

    #[test]
    fn second_mark_with_no_composite_left_alone() {
        // [A, acute, grave]. (A, acute) → Á (U+00C1). Á + grave has
        // no primary composite, so grave remains.
        let mut buf: Vec<u32> = vec![0x0041, 0x0301, 0x0300];
        canonical_composition(&mut buf);
        assert_eq!(buf, vec![0x00C1, 0x0300]);
    }

    #[test]
    fn skip_unmapped_advances_to_next_starter() {
        // Two unrelated starters with marks between: A grave B
        // acute → À B acute (separate composition runs).
        let mut buf: Vec<u32> = vec![0x0041, 0x0300, 0x0042, 0x0301];
        canonical_composition(&mut buf);
        assert_eq!(buf, vec![0x00C0, 0x0042, 0x0301]);
    }
}
