//! UAX #15 §1 — Normalization Form KD (Compatibility Decomposition).
//!
//! Pipeline: recursive compatibility decomposition followed by the
//! Canonical Ordering Algorithm. Differs from NFD only in the
//! decomposition stage — uses `COMPATIBILITY_DECOMPOSITION` (the
//! superset that includes both canonical and `<tag>`-marked
//! compatibility mappings).

use crate::decompose::decompose_compatibility;
use crate::ordering::canonical_ordering;

/// Compute the NFKD (Compatibility Decomposition) of `s`.
pub(crate) fn nfkd(s: &str) -> String {
    let mut buf: Vec<u32> = Vec::with_capacity(s.len());
    for c in s.chars() {
        decompose_compatibility(c as u32, &mut buf);
    }
    canonical_ordering(&mut buf);
    buf.into_iter()
        .map(|c| {
            char::from_u32(c).expect("UCD-derived codepoints are always Unicode scalar values")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::nfkd;

    #[test]
    fn ascii_unchanged() {
        assert_eq!(nfkd("abc"), "abc");
    }

    #[test]
    fn superscript_two_normalizes_to_digit() {
        // U+00B2 (²) is a compatibility decomposition <super> 2.
        assert_eq!(nfkd("\u{00B2}"), "2");
    }

    #[test]
    fn ligature_fi_decomposes_to_fi() {
        // U+FB01 (ﬁ) → "fi".
        assert_eq!(nfkd("\u{FB01}"), "fi");
    }

    #[test]
    fn nfkd_subsumes_nfd_canonical() {
        // NFKD must reproduce the canonical decomposition for chars
        // whose decomposition is canonical (no <tag>).
        assert_eq!(nfkd("\u{00C0}"), "A\u{0300}");
    }

    #[test]
    fn nfkd_is_idempotent() {
        let s = "café \u{00C0}\u{1E0A}\u{D55C} \u{FB01}\u{00B2}";
        let once = nfkd(s);
        let twice = nfkd(&once);
        assert_eq!(once, twice);
    }

    /// UAX #15 §5.1 Pattern 4: every column NFKD-normalizes to c5.
    #[test]
    fn nfkd_conformance_sweep() {
        for case in crate::test_fixture::load_normalization_test() {
            for (label, col) in [
                ("c1", &case.source),
                ("c2", &case.nfc),
                ("c3", &case.nfd),
                ("c4", &case.nfkc),
                ("c5", &case.nfkd),
            ] {
                assert_eq!(
                    nfkd(col),
                    case.nfkd,
                    "toNFKD({label}) mismatch on {:?}",
                    case.label
                );
            }
        }
    }
}
