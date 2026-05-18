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
use crate::tables::PRIMARY_COMPOSITES;

/// Attempt to compose `(a, b)` into a single codepoint using
/// algorithmic Hangul composition first, then the UCD-derived
/// `PRIMARY_COMPOSITES` table. Returns `None` when no composite
/// exists.
fn compose_pair(a: u32, b: u32) -> Option<u32> {
    if let Some(h) = compose_hangul(a, b) {
        return Some(h);
    }
    PRIMARY_COMPOSITES
        .binary_search_by_key(&(a, b), |(k, _)| *k)
        .ok()
        .map(|idx| PRIMARY_COMPOSITES[idx].1)
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
