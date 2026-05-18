//! `pinion-text-unicode` — self-hosted Unicode normalization
//! (UAX #15 NFC/NFD/NFKC/NFKD) over a vendored UCD 16.0.0 source.
//!
//! `unicode-normalization` / `icu` / `parley` 등 black box crate 가
//! 아닌, UCD 16.0.0 decomposition + `canonical_combining_class` +
//! `full_composition_exclusion` table 자체 embed (`build.rs`
//! codegen). R50 정신 완전 적용 — 외부 dependency 0개.
//!
//! # Quick start
//!
//! ```
//! use pinion_text_unicode::{normalize, NormForm};
//! assert_eq!(normalize("A\u{0300}", NormForm::Nfc), "\u{00C0}");
//! assert_eq!(normalize("\u{00C0}",  NormForm::Nfd), "A\u{0300}");
//! assert_eq!(normalize("\u{FB01}",  NormForm::Nfkd), "fi");
//! ```
//!
//! [`normalize`] returns `Cow<'a, str>`: already-normalized input is
//! returned as `Cow::Borrowed` (zero allocation, validated by
//! UAX #15 §5 Quick-check); only inputs that actually transform
//! cost an allocation. Internally the composition stage runs in
//! `O(n)` via a two-pointer in-place compact (R50.2.7 textbook
//! debt repayment over the earlier `O(n²)` `Vec::remove` pattern).
//!
//! # Crate roadmap (§5.37.3)
//!
//! * R50.2.0 — atomic-only §5.37.3 ratify (spec round, no impl).
//! * R50.2.1 — crate scaffold + [`NormForm`] enum.
//! * R50.2.2 — UCD 16.0.0 source vendor + `build.rs` table codegen.
//! * R50.2.3 — NFD algorithm (Canonical Decomposition + Ordering).
//! * R50.2.4 — NFC algorithm (Canonical Composition).
//! * R50.2.5 — NFKD / NFKC algorithms (Compatibility Decomposition).
//! * R50.2.6 — `pub fn normalize` + `pub UCD_VERSION` re-export.
//! * R50.2.7 — Quick-check fast path + `O(n)` compose-write +
//!   `Cow<str>` return type (3-debt repayment).
//! * R50.2.X — `text/normalize` RPC method (§5.37.2 channel).

/// UCD version pinned at build time (Unicode 16.0.0 = current).
///
/// Re-exported from the build-time codegen so consumers can declare
/// version-sensitive behaviour. Bumping the vendored UCD source
/// requires updating `ucd/*.txt`, re-running `cargo build` (the
/// table emit picks up automatically via `rerun-if-changed`), and
/// running the conformance sweep against the matching
/// `NormalizationTest.txt`.
pub use tables::UCD_VERSION;

/// One of the four Unicode normalization forms defined by UAX #15.
///
/// * [`Nfd`](Self::Nfd) — Canonical Decomposition.
/// * [`Nfc`](Self::Nfc) — Canonical Composition of the canonical
///   decomposition.
/// * [`Nfkd`](Self::Nfkd) — Compatibility Decomposition.
/// * [`Nfkc`](Self::Nfkc) — Canonical Composition of the
///   compatibility decomposition.
///
/// The four-variant set is closed by UAX #15. Additional variants
/// would be a new Unicode-level concept (e.g. `NFKC_Casefold` lives
/// in a separate UAX).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NormForm {
    /// `NFC` — Canonical Composition of the canonical decomposition.
    Nfc,
    /// `NFD` — Canonical Decomposition.
    Nfd,
    /// `NFKC` — Canonical Composition of the compatibility
    /// decomposition.
    Nfkc,
    /// `NFKD` — Compatibility Decomposition.
    Nfkd,
}

/// Normalize `s` to the requested Unicode normalization form.
///
/// Returns `Cow::Borrowed(s)` when UAX #15 §5 Quick-check confirms
/// the input is already in the requested form (zero-copy fast
/// path); otherwise returns `Cow::Owned(...)` containing the
/// normalized result.
///
/// The four supported forms are UAX #15 NFD, NFC, NFKD, and NFKC.
/// Each is fully conformant against UCD 16.0.0's
/// `NormalizationTest.txt` (~20000 invariant tuples) per the per-
/// form `*_conformance_sweep` test in this crate.
///
/// # Examples
///
/// ```
/// use pinion_text_unicode::{normalize, NormForm};
///
/// // Canonical Decomposition: precomposed Á → A + combining acute.
/// assert_eq!(normalize("\u{00C1}", NormForm::Nfd), "A\u{0301}");
///
/// // Canonical Composition: A + combining acute → Á.
/// assert_eq!(normalize("A\u{0301}", NormForm::Nfc), "\u{00C1}");
///
/// // Compatibility Decomposition: superscript 2 → digit 2.
/// assert_eq!(normalize("\u{00B2}", NormForm::Nfkd), "2");
///
/// // Compatibility Composition: ligature ﬁ → "fi" (no recomposition).
/// assert_eq!(normalize("\u{FB01}", NormForm::Nfkc), "fi");
///
/// // Already-normalized ASCII: zero-copy fast path.
/// use std::borrow::Cow;
/// assert!(matches!(
///     normalize("hello", NormForm::Nfc),
///     Cow::Borrowed("hello"),
/// ));
/// ```
#[must_use]
pub fn normalize(s: &str, form: NormForm) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    match form {
        NormForm::Nfc => {
            if matches!(
                quick_check::nfc_quick_check(s),
                quick_check::QuickCheck::Yes
            ) {
                Cow::Borrowed(s)
            } else {
                Cow::Owned(nfc::nfc(s))
            }
        }
        NormForm::Nfd => {
            if matches!(
                quick_check::nfd_quick_check(s),
                quick_check::QuickCheck::Yes
            ) {
                Cow::Borrowed(s)
            } else {
                Cow::Owned(nfd::nfd(s))
            }
        }
        NormForm::Nfkc => {
            if matches!(
                quick_check::nfkc_quick_check(s),
                quick_check::QuickCheck::Yes
            ) {
                Cow::Borrowed(s)
            } else {
                Cow::Owned(nfkc::nfkc(s))
            }
        }
        NormForm::Nfkd => {
            if matches!(
                quick_check::nfkd_quick_check(s),
                quick_check::QuickCheck::Yes
            ) {
                Cow::Borrowed(s)
            } else {
                Cow::Owned(nfkd::nfkd(s))
            }
        }
    }
}

/// UCD 16.0.0 tables emitted at build time by `build.rs`. Crate-
/// private structure; [`UCD_VERSION`] is re-exported above.
///
/// `FULL_COMPOSITION_EXCLUSION` is referenced only by the algorithm
/// derivation (build.rs) and the crate-internal table tests — it
/// has no runtime consumer in `lib.rs`. The `#[allow(dead_code)]`
/// holds the line until R50.2.X surfaces `text/exclusion_member`
/// via the RPC channel (§5.37.2).
#[allow(dead_code)]
mod tables {
    include!(concat!(env!("OUT_DIR"), "/tables.rs"));
}

mod hangul;
mod decompose;
mod ordering;
mod composition;
mod quick_check;
mod nfc;
mod nfd;
mod nfkc;
mod nfkd;

#[cfg(test)]
mod test_fixture;

#[cfg(test)]
mod tests {
    use super::ordering::combining_class;
    use super::tables::{
        CANONICAL_COMBINING_CLASS_BMP_DATA,
        CANONICAL_COMBINING_CLASS_BMP_INDEX,
        CANONICAL_COMBINING_CLASS_SUPPLEMENTARY, CANONICAL_DECOMPOSITION,
        COMPATIBILITY_DECOMPOSITION, FULL_COMPOSITION_EXCLUSION,
        PRIMARY_COMPOSITES,
    };
    use super::{normalize, NormForm, UCD_VERSION};

    #[test]
    fn ucd_version_pinned() {
        assert_eq!(UCD_VERSION, "16.0.0");
    }

    #[test]
    fn public_normalize_nfd_decomposes() {
        assert_eq!(normalize("\u{00C0}", NormForm::Nfd), "A\u{0300}");
    }

    #[test]
    fn public_normalize_nfc_composes() {
        assert_eq!(normalize("A\u{0300}", NormForm::Nfc), "\u{00C0}");
    }

    #[test]
    fn public_normalize_nfkd_strips_compat_tags() {
        assert_eq!(normalize("\u{FB01}", NormForm::Nfkd), "fi");
    }

    #[test]
    fn public_normalize_nfkc_strips_then_no_recompose() {
        assert_eq!(normalize("\u{00B2}", NormForm::Nfkc), "2");
    }

    #[test]
    fn ascii_nfc_fast_path_returns_borrowed_pointer() {
        use std::borrow::Cow;
        let s = "the quick brown fox";
        match normalize(s, NormForm::Nfc) {
            Cow::Borrowed(b) => assert_eq!(b.as_ptr(), s.as_ptr()),
            Cow::Owned(_) => {
                panic!("ASCII NFC must take the Quick-check fast path")
            }
        }
    }

    #[test]
    fn precomposed_nfc_fast_path_returns_borrowed_pointer() {
        use std::borrow::Cow;
        // À is already NFC — Quick-check must return Yes.
        let s = "caf\u{00E9}";
        match normalize(s, NormForm::Nfc) {
            Cow::Borrowed(b) => assert_eq!(b.as_ptr(), s.as_ptr()),
            Cow::Owned(_) => {
                panic!("precomposed NFC must take the fast path")
            }
        }
    }

    #[test]
    fn decomposed_nfc_takes_owned_path() {
        use std::borrow::Cow;
        // A + combining grave: Quick-check returns Maybe (grave is
        // a maybe-composer), so the full pipeline runs.
        let s = "A\u{0300}";
        match normalize(s, NormForm::Nfc) {
            Cow::Owned(o) => assert_eq!(o, "\u{00C0}"),
            Cow::Borrowed(_) => {
                panic!("decomposed input must take the Owned path")
            }
        }
    }

    #[test]
    fn canonical_decomposition_cardinality() {
        // Unicode 16.0.0 has 2081 canonical decompositions in
        // UnicodeData.txt. Lower bound guards against parse regression
        // without freezing the exact count (future minor versions may
        // add entries).
        assert!(
            CANONICAL_DECOMPOSITION.len() >= 2000,
            "canonical decomp count = {}",
            CANONICAL_DECOMPOSITION.len()
        );
    }

    #[test]
    fn compatibility_decomposition_cardinality() {
        assert!(
            COMPATIBILITY_DECOMPOSITION.len() >= 5000,
            "compat decomp count = {}",
            COMPATIBILITY_DECOMPOSITION.len()
        );
    }

    #[test]
    fn ccc_bmp_trie_well_formed() {
        // R50.2.10 — 2-stage trie invariants.
        assert_eq!(
            CANONICAL_COMBINING_CLASS_BMP_INDEX.len(),
            256,
            "Stage 1 must have exactly one entry per BMP high byte"
        );
        assert!(
            CANONICAL_COMBINING_CLASS_BMP_DATA.len() % 256 == 0,
            "Stage 2 must be a whole number of 256-byte blocks: len = {}",
            CANONICAL_COMBINING_CLASS_BMP_DATA.len()
        );
        // Block 0 must be the all-zero null block (shared sentinel).
        assert!(
            CANONICAL_COMBINING_CLASS_BMP_DATA[..256].iter().all(|&v| v == 0),
            "Stage 2 block 0 must be the null block"
        );
        // Every Stage-1 index must point inside Stage 2.
        let num_blocks = u16::try_from(
            CANONICAL_COMBINING_CLASS_BMP_DATA.len() / 256,
        )
        .expect("BMP CCC trie block count must fit in u16");
        for (i, &idx) in CANONICAL_COMBINING_CLASS_BMP_INDEX.iter().enumerate()
        {
            assert!(
                idx < num_blocks,
                "Stage 1 entry {i} points outside Stage 2 (idx = {idx}, \
                 blocks = {num_blocks})"
            );
        }
    }

    #[test]
    fn ccc_supplementary_sorted_and_unique() {
        for window in CANONICAL_COMBINING_CLASS_SUPPLEMENTARY.windows(2) {
            assert!(
                window[0].0 < window[1].0,
                "supplementary CCC not sorted strictly"
            );
        }
        for (cp, _) in CANONICAL_COMBINING_CLASS_SUPPLEMENTARY {
            assert!(
                *cp >= 0x10000,
                "BMP entry leaked into supplementary table: U+{cp:04X}"
            );
        }
    }

    #[test]
    fn latin_capital_a_grave_canonical_decomposition() {
        // U+00C0 (À) decomposes canonically to U+0041 A + U+0300
        // combining grave (UnicodeData.txt entry verified).
        let idx = CANONICAL_DECOMPOSITION
            .binary_search_by_key(&0x00C0_u32, |(cp, _)| *cp)
            .expect("U+00C0 must be in canonical decomposition table");
        assert_eq!(CANONICAL_DECOMPOSITION[idx].1, &[0x0041, 0x0300]);
    }

    #[test]
    fn latin_small_e_acute_canonical_decomposition() {
        // U+00E9 (é) → U+0065 e + U+0301 combining acute.
        let idx = CANONICAL_DECOMPOSITION
            .binary_search_by_key(&0x00E9_u32, |(cp, _)| *cp)
            .expect("U+00E9 must be in canonical decomposition table");
        assert_eq!(CANONICAL_DECOMPOSITION[idx].1, &[0x0065, 0x0301]);
    }

    #[test]
    fn combining_grave_ccc_is_230() {
        // U+0300 COMBINING GRAVE ACCENT has CCC 230 per UCD; goes
        // through the BMP trie.
        assert_eq!(combining_class(0x0300), 230);
    }

    #[test]
    fn supplementary_ccc_via_fallback() {
        // U+101FD PHAISTOS DISC SIGN COMBINING OBLIQUE STROKE has
        // CCC 220; verifies the supplementary-plane binary_search
        // fallback path.
        assert_eq!(combining_class(0x101FD), 220);
    }

    #[test]
    fn ascii_ccc_short_circuit_is_zero() {
        // R50.2.9 anchor short-circuit returns 0 below the first
        // non-zero CCC codepoint without touching the trie.
        for c in 0_u32..0x80 {
            assert_eq!(combining_class(c), 0, "ASCII U+{c:04X}");
        }
    }

    #[test]
    fn hangul_syllable_has_no_table_decomposition() {
        // Hangul precomposed syllables (AC00..D7A3) use algorithmic
        // decomposition per UAX #15 §16, not the UCD table. Verify no
        // table entry leaks.
        assert!(
            CANONICAL_DECOMPOSITION
                .binary_search_by_key(&0xAC00_u32, |(cp, _)| *cp)
                .is_err()
        );
        assert!(
            COMPATIBILITY_DECOMPOSITION
                .binary_search_by_key(&0xD7A3_u32, |(cp, _)| *cp)
                .is_err()
        );
    }

    #[test]
    fn full_composition_exclusion_contains_devanagari_qa() {
        // 0958..095F Script Specifics — Devanagari Letter Qa..Yya are
        // primary composition exclusions (CompositionExclusions.txt
        // §1, surfaced in Full_Composition_Exclusion).
        assert!(FULL_COMPOSITION_EXCLUSION.binary_search(&0x0958).is_ok());
        assert!(FULL_COMPOSITION_EXCLUSION.binary_search(&0x095F).is_ok());
    }

    #[test]
    fn full_composition_exclusion_includes_singletons() {
        // U+0374 (Greek Numeral Sign) is a singleton-decomposition
        // exclusion. Singletons are excluded per UAX #44 derivation.
        assert!(FULL_COMPOSITION_EXCLUSION.binary_search(&0x0374).is_ok());
    }

    #[test]
    fn primary_composite_a_grave_round_trip() {
        // (U+0041, U+0300) must round-trip-compose to U+00C0 per
        // UAX #15 D5 (canonical composition).
        let idx = PRIMARY_COMPOSITES
            .binary_search_by_key(&(0x0041_u32, 0x0300_u32), |(k, _)| *k)
            .expect("(0x0041, 0x0300) must compose");
        assert_eq!(PRIMARY_COMPOSITES[idx].1, 0x00C0);
    }

    #[test]
    fn primary_composites_exclude_full_composition_set() {
        // For each (a, b) → c, c must not be in
        // FULL_COMPOSITION_EXCLUSION (invariant from build.rs).
        for ((_, _), c) in PRIMARY_COMPOSITES {
            assert!(
                FULL_COMPOSITION_EXCLUSION.binary_search(c).is_err(),
                "composite 0x{c:04X} is in exclusion set"
            );
        }
    }

    #[test]
    fn primary_composites_sorted_strictly() {
        for window in PRIMARY_COMPOSITES.windows(2) {
            assert!(window[0].0 < window[1].0, "PRIMARY_COMPOSITES not sorted");
        }
    }
}
