//! UAX #15 §1 — Normalization Form D (Canonical Decomposition).
//!
//! Pipeline: recursive canonical decomposition followed by the
//! Canonical Ordering Algorithm. Hangul precomposed syllables are
//! handled algorithmically inside [`decompose_canonical`].

use crate::decompose::decompose_canonical;
use crate::ordering::canonical_ordering;

/// Compute the NFD (Canonical Decomposition) of `s`. The returned
/// `String` is guaranteed to be in Normalization Form D — every
/// canonical decomposable codepoint has been replaced by its fully
/// expanded form, and combining marks are in canonical order.
pub(crate) fn nfd(s: &str) -> String {
    let mut buf: Vec<u32> = Vec::with_capacity(s.len());
    for c in s.chars() {
        decompose_canonical(c as u32, &mut buf);
    }
    canonical_ordering(&mut buf);
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
    use super::nfd;

    #[test]
    fn ascii_unchanged() {
        assert_eq!(nfd("hello, world"), "hello, world");
    }

    #[test]
    fn latin_a_grave_decomposes() {
        // À (U+00C0) → A (U+0041) + combining grave (U+0300).
        assert_eq!(nfd("\u{00C0}"), "A\u{0300}");
    }

    #[test]
    fn latin_e_acute_decomposes() {
        // é (U+00E9) → e + combining acute.
        assert_eq!(nfd("\u{00E9}"), "e\u{0301}");
    }

    #[test]
    fn hangul_han_decomposes_algorithmically() {
        // 한 (U+D55C) → ᄒ ᅡ ᆫ.
        assert_eq!(nfd("\u{D55C}"), "\u{1112}\u{1161}\u{11AB}");
    }

    #[test]
    fn combining_marks_reordered() {
        // D + dot-above (230) + dot-below (220) → D + dot-below + dot-above.
        assert_eq!(nfd("D\u{0307}\u{0323}"), "D\u{0323}\u{0307}");
    }

    #[test]
    fn nfd_is_idempotent() {
        let s = "café \u{00C0}\u{1E0A}\u{D55C}";
        let once = nfd(s);
        let twice = nfd(&once);
        assert_eq!(once, twice);
    }

    /// UAX #15 §5.1 conformance sweep over the full
    /// `NormalizationTest.txt` fixture (UCD 16.0.0). For each test
    /// row `c1; c2; c3; c4; c5`, verifies the NFD-side invariants:
    ///
    /// - `c3 == toNFD(c1)`
    /// - `c3 == toNFD(c2)`
    /// - `c3 == toNFD(c3)` (idempotence)
    /// - `c5 == toNFD(c4)`
    /// - `c5 == toNFD(c5)` (idempotence)
    #[test]
    fn nfd_conformance_sweep() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/ucd/NormalizationTest.txt"
        );
        let text = std::fs::read_to_string(path)
            .expect("NormalizationTest.txt must be vendored");
        for case in parse_normalization_test(&text) {
            assert_eq!(
                nfd(&case.source),
                case.nfd,
                "toNFD(c1) mismatch on case {:?}",
                case.label
            );
            assert_eq!(
                nfd(&case.nfc),
                case.nfd,
                "toNFD(c2) mismatch on case {:?}",
                case.label
            );
            assert_eq!(
                nfd(&case.nfd),
                case.nfd,
                "toNFD(c3) not idempotent on case {:?}",
                case.label
            );
            assert_eq!(
                nfd(&case.nfkc),
                case.nfkd,
                "toNFD(c4) mismatch on case {:?}",
                case.label
            );
            assert_eq!(
                nfd(&case.nfkd),
                case.nfkd,
                "toNFD(c5) not idempotent on case {:?}",
                case.label
            );
        }
    }

    #[cfg(test)]
    struct NormalizationCase {
        source: String,
        nfc: String,
        nfd: String,
        nfkc: String,
        nfkd: String,
        label: String,
    }

    #[cfg(test)]
    fn parse_normalization_test(text: &str) -> Vec<NormalizationCase> {
        let mut cases = Vec::new();
        for raw in text.lines() {
            if raw.starts_with('@') || raw.starts_with('#') || raw.is_empty() {
                continue;
            }
            let (data, label) = match raw.find('#') {
                Some(pos) => (&raw[..pos], raw[pos + 1..].trim().to_owned()),
                None => (raw, String::new()),
            };
            let cols: Vec<&str> = data.split(';').collect();
            if cols.len() < 5 {
                continue;
            }
            cases.push(NormalizationCase {
                source: decode_column(cols[0]),
                nfc: decode_column(cols[1]),
                nfd: decode_column(cols[2]),
                nfkc: decode_column(cols[3]),
                nfkd: decode_column(cols[4]),
                label,
            });
        }
        cases
    }

    #[cfg(test)]
    fn decode_column(col: &str) -> String {
        col.split_whitespace()
            .map(|hex| {
                let cp = u32::from_str_radix(hex, 16)
                    .expect("hex codepoint");
                char::from_u32(cp).expect("valid Unicode scalar")
            })
            .collect()
    }
}
