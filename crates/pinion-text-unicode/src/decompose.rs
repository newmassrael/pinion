//! Recursive canonical decomposition (UAX #15, D2 fixed-point).
//!
//! The `CANONICAL_DECOMPOSITION` table maps each canonical
//! decomposable codepoint to its **one-step** decomposition. The
//! true canonical decomposition is the fixed point obtained by
//! repeated substitution until no codepoint decomposes further. We
//! realise this by recursive walk on each table-hit child.

use crate::hangul::decompose_hangul_syllable;
use crate::tables::CANONICAL_DECOMPOSITION;

/// Append the fully-decomposed canonical form of `c` to `out`. Non-
/// decomposable codepoints (including Hangul jamo and CJK
/// ideographs) are pushed unchanged.
pub(crate) fn decompose_canonical(c: u32, out: &mut Vec<u32>) {
    if decompose_hangul_syllable(c, out) {
        return;
    }
    match CANONICAL_DECOMPOSITION.binary_search_by_key(&c, |(cp, _)| *cp) {
        Ok(idx) => {
            for &dc in CANONICAL_DECOMPOSITION[idx].1 {
                decompose_canonical(dc, out);
            }
        }
        Err(_) => out.push(c),
    }
}

#[cfg(test)]
mod tests {
    use super::decompose_canonical;

    #[test]
    fn unmapped_starter_unchanged() {
        let mut out = Vec::new();
        decompose_canonical(0x0041, &mut out); // 'A' has no decomp
        assert_eq!(out, vec![0x0041]);
    }

    #[test]
    fn latin_a_grave_decomposes_one_step() {
        // U+00C0 (À) → U+0041 + U+0300.
        let mut out = Vec::new();
        decompose_canonical(0x00C0, &mut out);
        assert_eq!(out, vec![0x0041, 0x0300]);
    }

    #[test]
    fn hangul_syllable_decomposes_algorithmically() {
        // 한 (U+D55C) → ᄒ ᅡ ᆫ.
        let mut out = Vec::new();
        decompose_canonical(0xD55C, &mut out);
        assert_eq!(out, vec![0x1112, 0x1161, 0x11AB]);
    }

    #[test]
    fn recursive_decomposition_reaches_fixed_point() {
        // U+1E0A (Ḋ) decomposes to U+0044 + U+0307 in one step.
        // Each child is a starter / combining mark with no further
        // canonical decomposition — verify recursion terminates.
        let mut out = Vec::new();
        decompose_canonical(0x1E0A, &mut out);
        assert_eq!(out, vec![0x0044, 0x0307]);
    }
}
