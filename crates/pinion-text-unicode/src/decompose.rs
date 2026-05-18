//! Recursive canonical decomposition (UAX #15, D2 fixed-point).
//!
//! The `CANONICAL_DECOMPOSITION` table maps each canonical
//! decomposable codepoint to its **one-step** decomposition. The
//! true canonical decomposition is the fixed point obtained by
//! repeated substitution until no codepoint decomposes further. We
//! realise this by recursive walk on each table-hit child.

use crate::hangul::decompose_hangul_syllable;
use crate::tables::{
    CANONICAL_DECOMPOSITION, CANONICAL_DECOMPOSITION_FIRST_CP,
    COMPATIBILITY_DECOMPOSITION, COMPATIBILITY_DECOMPOSITION_FIRST_CP,
};

/// Append the fully-decomposed canonical form of `c` to `out`. Non-
/// decomposable codepoints (including Hangul jamo and CJK
/// ideographs) are pushed unchanged.
///
/// R50.2.9 — codepoints below
/// [`CANONICAL_DECOMPOSITION_FIRST_CP`] (build-time derived) are
/// pushed unchanged without consulting the table or the Hangul
/// algorithm. The anchor sits below the Hangul syllable block
/// (`U+AC00..U+D7A3`), so ordering the short-circuit before
/// [`decompose_hangul_syllable`] is sound — any codepoint passing
/// the short-circuit is by construction outside the Hangul range.
pub(crate) fn decompose_canonical(c: u32, out: &mut Vec<u32>) {
    if c < CANONICAL_DECOMPOSITION_FIRST_CP {
        out.push(c);
        return;
    }
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

/// Append the fully-decomposed **compatibility** form of `c` to
/// `out`. The compatibility decomposition is recursive over
/// `COMPATIBILITY_DECOMPOSITION` (which contains both canonical and
/// compatibility one-step entries, tag stripped). Hangul is handled
/// algorithmically.
///
/// R50.2.9 — same short-circuit policy as
/// [`decompose_canonical`], anchored on
/// [`COMPATIBILITY_DECOMPOSITION_FIRST_CP`].
pub(crate) fn decompose_compatibility(c: u32, out: &mut Vec<u32>) {
    if c < COMPATIBILITY_DECOMPOSITION_FIRST_CP {
        out.push(c);
        return;
    }
    if decompose_hangul_syllable(c, out) {
        return;
    }
    match COMPATIBILITY_DECOMPOSITION.binary_search_by_key(&c, |(cp, _)| *cp)
    {
        Ok(idx) => {
            for &dc in COMPATIBILITY_DECOMPOSITION[idx].1 {
                decompose_compatibility(dc, out);
            }
        }
        Err(_) => out.push(c),
    }
}

#[cfg(test)]
mod tests {
    use super::{decompose_canonical, decompose_compatibility};

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

    #[test]
    fn compatibility_decomposes_superscript_two() {
        // U+00B2 SUPERSCRIPT TWO has compatibility decomposition
        // <super> 0032. Tag-stripped table maps it to [0x0032].
        let mut out = Vec::new();
        decompose_compatibility(0x00B2, &mut out);
        assert_eq!(out, vec![0x0032]);
    }

    #[test]
    fn compatibility_decomposes_ligature_fi() {
        // U+FB01 LATIN SMALL LIGATURE FI has compatibility
        // decomposition <compat> 0066 0069 ("fi").
        let mut out = Vec::new();
        decompose_compatibility(0xFB01, &mut out);
        assert_eq!(out, vec![0x0066, 0x0069]);
    }

    #[test]
    fn compatibility_includes_canonical_decompositions() {
        // The compatibility table is a superset; a purely
        // canonical decomp (À → A + grave) must work via
        // decompose_compatibility too.
        let mut out = Vec::new();
        decompose_compatibility(0x00C0, &mut out);
        assert_eq!(out, vec![0x0041, 0x0300]);
    }

    #[test]
    fn compatibility_hangul_uses_algorithm() {
        // Hangul precomposed syllables have no UCD decomposition
        // mapping; the algorithmic Hangul path must still run.
        let mut out = Vec::new();
        decompose_compatibility(0xD55C, &mut out);
        assert_eq!(out, vec![0x1112, 0x1161, 0x11AB]);
    }
}
