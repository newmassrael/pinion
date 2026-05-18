//! R50.2.x §5.37.3 `pinion-text-unicode` — self-hosted Unicode
//! normalization (UAX #15 NFC/NFD/NFKC/NFKD).
//!
//! `unicode-normalization` / `icu` / `parley` 등 black box crate 가
//! 아닌, UCD 16.0.0 decomposition + `canonical_combining_class` +
//! `full_composition_exclusion` table 자체 embed (`build.rs`
//! codegen). R50 정신 완전 적용 — 외부 dependency 0개.
//!
//! Crate roadmap (R50.2.x sub-phase chain per §5.37.3):
//!
//! * R50.2.0 — atomic-only §5.37.3 ratify (spec round, no impl).
//! * R50.2.1 — crate scaffold + [`NormForm`] enum.
//! * R50.2.2 — UCD 16.0.0 source vendor + `build.rs` table codegen.
//! * R50.2.3 — NFD algorithm (Canonical Decomposition + Ordering).
//! * R50.2.4 — NFC algorithm (Canonical Composition).
//! * R50.2.5 — NFKD / NFKC algorithms (Compatibility Decomposition).
//! * R50.2.6 — Quick-check optimization (UAX #15 §5).
//! * R50.2.X — `text/normalize` RPC method (§5.37.2 channel).

/// One of the four Unicode normalization forms defined by UAX #15.
///
/// * [`Nfd`](Self::Nfd) — Canonical Decomposition.
/// * [`Nfc`](Self::Nfc) — Canonical Composition of the canonical
///   decomposition.
/// * [`Nfkd`](Self::Nfkd) — Compatibility Decomposition.
/// * [`Nfkc`](Self::Nfkc) — Canonical Composition of the
///   compatibility decomposition.
///
/// The four-variant set is closed by UAX #15 and frozen at the
/// algorithm level; additional variants would be a new Unicode-level
/// concept (e.g. `NFKC_Casefold` lives in a separate UAX). pinion adds
/// algorithm-side helpers in R50.2.3+ as the consuming algorithm
/// crystallises (additive — public API breaking 0).
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

/// UCD 16.0.0 tables emitted at build time by `build.rs`. Crate-private
/// — exposed only to the algorithm modules in R50.2.3+.
///
/// `#[allow(dead_code)]` holds the line until the algorithm slice
/// (R50.2.3 NFD) becomes the first runtime consumer. Lint reactivates
/// then.
#[allow(dead_code)]
mod tables {
    include!(concat!(env!("OUT_DIR"), "/tables.rs"));
}

#[cfg(test)]
mod tests {
    use super::tables::{
        CANONICAL_COMBINING_CLASS, CANONICAL_DECOMPOSITION,
        COMPATIBILITY_DECOMPOSITION, FULL_COMPOSITION_EXCLUSION,
        PRIMARY_COMPOSITES, UCD_VERSION,
    };

    #[test]
    fn ucd_version_pinned() {
        assert_eq!(UCD_VERSION, "16.0.0");
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
    fn ccc_cardinality_and_sorted() {
        assert!(
            CANONICAL_COMBINING_CLASS.len() >= 900,
            "CCC count = {}",
            CANONICAL_COMBINING_CLASS.len()
        );
        // Must be strictly ascending by codepoint for binary_search.
        for window in CANONICAL_COMBINING_CLASS.windows(2) {
            assert!(window[0].0 < window[1].0, "CCC not sorted strictly");
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
        // U+0300 COMBINING GRAVE ACCENT has CCC 230 per UCD.
        let idx = CANONICAL_COMBINING_CLASS
            .binary_search_by_key(&0x0300_u32, |(cp, _)| *cp)
            .expect("U+0300 must be in CCC table");
        assert_eq!(CANONICAL_COMBINING_CLASS[idx].1, 230);
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
