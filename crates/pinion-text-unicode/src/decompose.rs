//! Recursive canonical decomposition (UAX #15, D2 fixed-point).
//!
//! The decomposition tables map each canonical decomposable
//! codepoint to its **one-step** decomposition. The true canonical
//! decomposition is the fixed point obtained by repeated
//! substitution until no codepoint decomposes further. We realise
//! this by recursive walk on each table-hit child.

use crate::hangul::decompose_hangul_syllable;
use crate::tables::{
    CANONICAL_DECOMPOSITION_BMP_DATA, CANONICAL_DECOMPOSITION_BMP_INDEX,
    CANONICAL_DECOMPOSITION_DATA, CANONICAL_DECOMPOSITION_FIRST_CP,
    CANONICAL_DECOMPOSITION_SUPPLEMENTARY,
    COMPATIBILITY_DECOMPOSITION_BMP_DATA,
    COMPATIBILITY_DECOMPOSITION_BMP_INDEX,
    COMPATIBILITY_DECOMPOSITION_DATA,
    COMPATIBILITY_DECOMPOSITION_FIRST_CP,
    COMPATIBILITY_DECOMPOSITION_SUPPLEMENTARY,
};

/// R50.2.13 — shared 2-stage BMP trie lookup for decomposition
/// tables. Returns `None` for codepoints below the build-derived
/// `anchor` (UAX #44 §3 forward-stable), uses the `(BMP_INDEX,
/// BMP_DATA)` trie for BMP codepoints (Stage-2 value `0` encodes
/// "no decomposition" and maps back to `None` here), and falls
/// back to a sparse `binary_search` for supplementary-plane
/// codepoints. The Stage-2 value packs `(length << 24) | offset`
/// into 32 bits, with `offset` indexing into `decomp_data` and
/// `length` slicing out the one-step decomposition sequence.
#[inline]
fn lookup_decomp_trie(
    index: &'static [u16; 256],
    data: &'static [u32],
    decomp_data: &'static [u32],
    supplementary: &'static [(u32, &'static [u32])],
    anchor: u32,
    cp: u32,
) -> Option<&'static [u32]> {
    if cp < anchor {
        return None;
    }
    if cp < 0x10000 {
        let block = index[(cp >> 8) as usize] as usize;
        let packed = data[block * 256 + (cp & 0xFF) as usize];
        if packed == 0 {
            return None;
        }
        let length = (packed >> 24) as usize;
        let offset = (packed & 0x00FF_FFFF) as usize;
        return Some(&decomp_data[offset..offset + length]);
    }
    lookup_decomp_supplementary(supplementary, cp)
}

/// Sparse supplementary-plane fallback for [`lookup_decomp_trie`].
///
/// R50.2.13 — kept `#[inline(never)]` for the same reason as
/// [`crate::ordering::combining_class_supplementary`] (R50.2.12
/// asm-driven split): folding the `binary_search_by_key` IR back
/// into [`lookup_decomp_trie`] would inflate the hot path past the
/// LLVM inline threshold, breaking inlining into the per-character
/// `decompose_canonical` / `decompose_compatibility` callers.
/// Supplementary-plane decomposable text (CJK Compatibility
/// Ideographs Supplement at U+2F800..U+2FA1F) is rare in
/// normalization workloads, so paying a `callq` only on this cold
/// branch is the textbook trade.
#[inline(never)]
fn lookup_decomp_supplementary(
    supplementary: &'static [(u32, &'static [u32])],
    cp: u32,
) -> Option<&'static [u32]> {
    supplementary
        .binary_search_by_key(&cp, |(c, _)| *c)
        .ok()
        .map(|idx| supplementary[idx].1)
}

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
///
/// R50.2.13 — table lookup goes through the 2-stage BMP trie
/// (`CANONICAL_DECOMPOSITION_BMP_INDEX` / `_BMP_DATA` / `_DATA`)
/// with the supplementary-plane sparse fallback. Replaces the
/// R50.2.2 `binary_search_by_key` over `&[(u32, &[u32])]` shape
/// with two BMP memory reads + a slice into `_DATA` on hits.
pub(crate) fn decompose_canonical(c: u32, out: &mut Vec<u32>) {
    if c < CANONICAL_DECOMPOSITION_FIRST_CP {
        out.push(c);
        return;
    }
    if decompose_hangul_syllable(c, out) {
        return;
    }
    if let Some(decomp) = lookup_decomp_trie(
        CANONICAL_DECOMPOSITION_BMP_INDEX,
        CANONICAL_DECOMPOSITION_BMP_DATA,
        CANONICAL_DECOMPOSITION_DATA,
        CANONICAL_DECOMPOSITION_SUPPLEMENTARY,
        CANONICAL_DECOMPOSITION_FIRST_CP,
        c,
    ) {
        for &dc in decomp {
            decompose_canonical(dc, out);
        }
    } else {
        out.push(c);
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
///
/// R50.2.13 — same trie shape as [`decompose_canonical`], indexing
/// the `COMPATIBILITY_*` tables.
pub(crate) fn decompose_compatibility(c: u32, out: &mut Vec<u32>) {
    if c < COMPATIBILITY_DECOMPOSITION_FIRST_CP {
        out.push(c);
        return;
    }
    if decompose_hangul_syllable(c, out) {
        return;
    }
    if let Some(decomp) = lookup_decomp_trie(
        COMPATIBILITY_DECOMPOSITION_BMP_INDEX,
        COMPATIBILITY_DECOMPOSITION_BMP_DATA,
        COMPATIBILITY_DECOMPOSITION_DATA,
        COMPATIBILITY_DECOMPOSITION_SUPPLEMENTARY,
        COMPATIBILITY_DECOMPOSITION_FIRST_CP,
        c,
    ) {
        for &dc in decomp {
            decompose_compatibility(dc, out);
        }
    } else {
        out.push(c);
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

    #[test]
    fn supplementary_canonical_via_fallback() {
        // R50.2.13 — U+2F800..U+2FA1F (CJK Compatibility Ideographs
        // Supplement) are canonical decompositions in the
        // supplementary plane. U+2F800 → U+4E3D. Verifies the
        // `lookup_decomp_supplementary` cold path.
        let mut out = Vec::new();
        decompose_canonical(0x2F800, &mut out);
        assert_eq!(out, vec![0x4E3D]);
    }

    #[test]
    fn supplementary_compatibility_via_fallback() {
        // Same supplementary range surfaces in the compatibility
        // table; the trie's compatibility variant uses the same
        // sparse fallback shape.
        let mut out = Vec::new();
        decompose_compatibility(0x2F800, &mut out);
        assert_eq!(out, vec![0x4E3D]);
    }
}
