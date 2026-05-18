//! UAX #15 §1 — Normalization Form C (Canonical Composition).
//!
//! Pipeline: full canonical decomposition + Canonical Ordering +
//! Canonical Composition. Equivalent to running NFD then applying
//! the inverse composition step. The result is the unique NFC
//! representation of the input.

use crate::composition::canonical_composition;
use crate::decompose::decompose_canonical;
use crate::ordering::canonical_ordering;

/// Compute the NFC (Canonical Composition) of `s`.
pub(crate) fn nfc(s: &str) -> String {
    let mut buf: Vec<u32> = Vec::with_capacity(s.len());
    for c in s.chars() {
        decompose_canonical(c as u32, &mut buf);
    }
    canonical_ordering(&mut buf);
    canonical_composition(&mut buf);
    buf.into_iter()
        .map(|c| {
            char::from_u32(c).expect(
                "UCD-derived codepoints are always Unicode scalar values",
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::nfc;

    #[test]
    fn ascii_unchanged() {
        assert_eq!(nfc("hello, world"), "hello, world");
    }

    #[test]
    fn precomposed_input_round_trips() {
        // À (U+00C0) decomposes to A + grave and recomposes back.
        assert_eq!(nfc("\u{00C0}"), "\u{00C0}");
    }

    #[test]
    fn decomposed_input_composes() {
        // A + combining grave → À.
        assert_eq!(nfc("A\u{0300}"), "\u{00C0}");
    }

    #[test]
    fn hangul_decomposed_jamo_compose_to_syllable() {
        // ᄒ + ᅡ + ᆫ → 한 (U+D55C).
        assert_eq!(nfc("\u{1112}\u{1161}\u{11AB}"), "\u{D55C}");
    }

    #[test]
    fn nfc_is_idempotent() {
        let s = "café \u{00C0}\u{1E0A}\u{D55C}";
        let once = nfc(s);
        let twice = nfc(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn excluded_composite_stays_decomposed() {
        // U+0958 (DEVANAGARI LETTER QA) is in Full_Composition_
        // Exclusion. Its canonical decomp is U+0915 + U+093C. NFC
        // must NOT re-compose them.
        assert_eq!(nfc("\u{0915}\u{093C}"), "\u{0915}\u{093C}");
    }

    /// UAX #15 §5.1 conformance sweep — NFC-side invariants per
    /// `NormalizationTest.txt`:
    ///
    /// - `c2 == toNFC(c1)`
    /// - `c2 == toNFC(c2)` (idempotence)
    /// - `c2 == toNFC(c3)`
    /// - `c4 == toNFC(c4)` (idempotence)
    /// - `c4 == toNFC(c5)`
    #[test]
    fn nfc_conformance_sweep() {
        for case in crate::test_fixture::load_normalization_test() {
            assert_eq!(
                nfc(&case.source),
                case.nfc,
                "toNFC(c1) mismatch on case {:?}",
                case.label
            );
            assert_eq!(
                nfc(&case.nfc),
                case.nfc,
                "toNFC(c2) not idempotent on case {:?}",
                case.label
            );
            assert_eq!(
                nfc(&case.nfd),
                case.nfc,
                "toNFC(c3) mismatch on case {:?}",
                case.label
            );
            assert_eq!(
                nfc(&case.nfkc),
                case.nfkc,
                "toNFC(c4) not idempotent on case {:?}",
                case.label
            );
            assert_eq!(
                nfc(&case.nfkd),
                case.nfkc,
                "toNFC(c5) mismatch on case {:?}",
                case.label
            );
        }
    }
}
