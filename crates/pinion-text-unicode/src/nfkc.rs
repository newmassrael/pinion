//! UAX #15 §1 — Normalization Form KC (Compatibility Composition).
//!
//! Pipeline: recursive compatibility decomposition + Canonical
//! Ordering + Canonical Composition. Reuses the NFD-derived
//! ordering and the NFC-derived composition stages; only the
//! decomposition source differs (compatibility table).

use crate::composition::canonical_composition;
use crate::decompose::decompose_compatibility;
use crate::ordering::canonical_ordering;

/// Compute the NFKC (Compatibility Composition) of `s`.
pub(crate) fn nfkc(s: &str) -> String {
    let mut buf: Vec<u32> = Vec::with_capacity(s.len());
    for c in s.chars() {
        decompose_compatibility(c as u32, &mut buf);
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
    use super::nfkc;

    #[test]
    fn ascii_unchanged() {
        assert_eq!(nfkc("abc"), "abc");
    }

    #[test]
    fn superscript_two_normalizes_to_digit() {
        assert_eq!(nfkc("\u{00B2}"), "2");
    }

    #[test]
    fn ligature_fi_decomposes_to_fi() {
        // NFKC of ﬁ → "fi" (no recomposition since 'f' + 'i' has
        // no primary composite).
        assert_eq!(nfkc("\u{FB01}"), "fi");
    }

    #[test]
    fn decomposed_input_composes_back() {
        // A + combining grave → À even via the NFKC pipeline.
        assert_eq!(nfkc("A\u{0300}"), "\u{00C0}");
    }

    #[test]
    fn hangul_jamo_compose_to_syllable() {
        assert_eq!(nfkc("\u{1112}\u{1161}\u{11AB}"), "\u{D55C}");
    }

    #[test]
    fn nfkc_is_idempotent() {
        let s = "café \u{00C0}\u{1E0A}\u{D55C} \u{FB01}\u{00B2}";
        let once = nfkc(s);
        let twice = nfkc(&once);
        assert_eq!(once, twice);
    }

    /// UAX #15 §5.1 Pattern 3: every column NFKC-normalizes to c4.
    #[test]
    fn nfkc_conformance_sweep() {
        for case in crate::test_fixture::load_normalization_test() {
            for (label, col) in [
                ("c1", &case.source),
                ("c2", &case.nfc),
                ("c3", &case.nfd),
                ("c4", &case.nfkc),
                ("c5", &case.nfkd),
            ] {
                assert_eq!(
                    nfkc(col),
                    case.nfkc,
                    "toNFKC({label}) mismatch on {:?}",
                    case.label
                );
            }
        }
    }
}
