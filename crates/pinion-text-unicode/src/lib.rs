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
///
/// R50.2.13 — `clippy::unreadable_literal` is allowed at the
/// module level because every literal in `tables.rs` is build-
/// emitted (UCD-derived codepoints, packed `(length, offset)`
/// trie cells, dedup block indices). The lint's "help a human
/// reader scan the digits" rationale does not apply to a
/// multi-thousand-line machine-generated artifact; sprinkling
/// underscores via the codegen would add cost without buying
/// audit value. Same rationale as the per-const allow on
/// `TABLES_GENERATED_BYTES` (R50.2.12), promoted to module scope
/// now that R50.2.13 added the `BMP_DATA` / `DATA` `u32` arrays.
#[allow(dead_code, clippy::unreadable_literal)]
mod tables {
    include!(concat!(env!("OUT_DIR"), "/tables.rs"));
}

mod composition;
mod decompose;
mod hangul;
mod nfc;
mod nfd;
mod nfkc;
mod nfkd;
mod ordering;
mod quick_check;
mod range;

pub mod bidi;
pub use bidi::{BidiClass, bidi_class};

pub mod linebreak;
pub use linebreak::{BreakOpportunity, LineBreak, line_break_class, line_break_opportunities};

pub mod script;
pub use script::{Script, ScriptRun, script, script_runs};

#[cfg(test)]
mod test_fixture;

#[cfg(test)]
mod tests {
    use super::composition::compose_pair;
    use super::decompose::{decompose_canonical, decompose_compatibility};
    use super::ordering::combining_class;
    use super::tables::{
        CANONICAL_COMBINING_CLASS_BMP_DATA, CANONICAL_COMBINING_CLASS_BMP_INDEX,
        CANONICAL_COMBINING_CLASS_SUPPLEMENTARY, CANONICAL_DECOMPOSITION_BMP_DATA,
        CANONICAL_DECOMPOSITION_BMP_INDEX, CANONICAL_DECOMPOSITION_DATA,
        CANONICAL_DECOMPOSITION_ENTRY_COUNT, CANONICAL_DECOMPOSITION_SUPPLEMENTARY,
        COMPATIBILITY_DECOMPOSITION_BMP_DATA, COMPATIBILITY_DECOMPOSITION_BMP_INDEX,
        COMPATIBILITY_DECOMPOSITION_DATA, COMPATIBILITY_DECOMPOSITION_ENTRY_COUNT,
        COMPATIBILITY_DECOMPOSITION_SUPPLEMENTARY, FULL_COMPOSITION_EXCLUSION,
        PRIMARY_COMPOSITES_BC_DATA, PRIMARY_COMPOSITES_BMP_DATA, PRIMARY_COMPOSITES_BMP_INDEX,
        PRIMARY_COMPOSITES_ENTRY_COUNT, PRIMARY_COMPOSITES_SUPPLEMENTARY, TABLES_GENERATED_BYTES,
    };
    use super::{NormForm, UCD_VERSION, normalize};

    // R50.2.12 — compile-time guard against an unintended codegen
    // blow-up. `TABLES_GENERATED_BYTES` is a build-time `const`, so
    // `assert!` lowers to a const evaluation; clippy correctly
    // flagged a runtime `#[test]` as redundant (`assert!(true)`
    // optimised out). The const block fires at compile time and
    // fails the build instead of a single test target. The
    // R50.2.13 baseline lands a few hundred KiB above the R50.2.11
    // 519 KiB anchor because the BMP decomposition tries replace
    // sparse `&[(u32, &[u32])]` lists with full 64-cell Stage-2
    // blocks (per-codepoint offsets prevent dedup beyond the null
    // block). The 1.5 MiB ceiling still leaves headroom for R50.2.14
    // PRIMARY_COMPOSITES trie work without becoming a noise gate.
    const _: () = assert!(
        TABLES_GENERATED_BYTES < 1_500_000,
        "tables.rs footprint regression vs 1.5 MiB ceiling",
    );
    const _: () = assert!(
        TABLES_GENERATED_BYTES > 100_000,
        "tables.rs footprint suspiciously small (<100 KiB)",
    );

    // R50.2.13 — cardinality regression guards. The build-emitted
    // `*_ENTRY_COUNT` consts replace the pre-trie `.len()` shape
    // introspection on `CANONICAL_DECOMPOSITION` / `COMPATIBILITY_-
    // DECOMPOSITION`. clippy correctly flags `assert!` over const
    // expressions as `assert!(true)`-equivalent for runtime tests,
    // so the check fires at compile time (R50.2.12 pattern) and
    // fails the build rather than a single test target. Lower
    // bounds (not exact equality) leave room for future minor UCD
    // versions to add canonical / compatibility entries without a
    // gratuitous test churn.
    const _: () = assert!(
        CANONICAL_DECOMPOSITION_ENTRY_COUNT >= 2000,
        "canonical decomposition table shrank below 2000 entries"
    );
    const _: () = assert!(
        COMPATIBILITY_DECOMPOSITION_ENTRY_COUNT >= 5000,
        "compatibility decomposition table shrank below 5000 entries"
    );
    // R50.2.14 — same shape, applied to the primary-composite map.
    // Unicode 16.0.0 yields ~940 (a, b) -> c entries (length-2
    // canonical decompositions minus the Full_Composition_Exclusion
    // set). The lower bound guards against derivation drift in
    // `derive_primary_composites` without freezing the exact count
    // (future UCD versions may add a handful of entries).
    const _: () = assert!(
        PRIMARY_COMPOSITES_ENTRY_COUNT >= 900,
        "primary composites table shrank below 900 entries"
    );

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
            CANONICAL_COMBINING_CLASS_BMP_DATA[..256]
                .iter()
                .all(|&v| v == 0),
            "Stage 2 block 0 must be the null block"
        );
        // Every Stage-1 index must point inside Stage 2.
        let num_blocks = u16::try_from(CANONICAL_COMBINING_CLASS_BMP_DATA.len() / 256)
            .expect("BMP CCC trie block count must fit in u16");
        for (i, &idx) in CANONICAL_COMBINING_CLASS_BMP_INDEX.iter().enumerate() {
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

    /// Validate the R50.2.13 BMP decomposition trie shape — Stage 1
    /// has exactly one cell per high byte, Stage 2 is a whole number
    /// of 256-cell blocks, block 0 is the shared null block, and
    /// every Stage 1 index resolves inside Stage 2. Catches codegen
    /// drift (e.g. an off-by-one in `build_decomp_bmp_trie`) before
    /// it slips into the lookup hot path.
    #[test]
    fn canonical_decomp_bmp_trie_well_formed() {
        assert_eq!(CANONICAL_DECOMPOSITION_BMP_INDEX.len(), 256);
        assert!(
            CANONICAL_DECOMPOSITION_BMP_DATA.len() % 256 == 0,
            "Stage 2 must be a whole number of 256-cell blocks: len = {}",
            CANONICAL_DECOMPOSITION_BMP_DATA.len()
        );
        assert!(
            CANONICAL_DECOMPOSITION_BMP_DATA[..256]
                .iter()
                .all(|&v| v == 0),
            "Stage 2 block 0 must be the null block"
        );
        let num_blocks = u16::try_from(CANONICAL_DECOMPOSITION_BMP_DATA.len() / 256)
            .expect("BMP decomp trie block count must fit in u16");
        for (i, &idx) in CANONICAL_DECOMPOSITION_BMP_INDEX.iter().enumerate() {
            assert!(
                idx < num_blocks,
                "Stage 1 entry {i} points outside Stage 2 (idx = {idx}, \
                 blocks = {num_blocks})"
            );
        }
        // Every packed cell's `offset + length` must fit inside
        // `_DATA` — the slice into `_DATA` happens unchecked in the
        // hot path so the invariant must hold by codegen.
        let data_len = CANONICAL_DECOMPOSITION_DATA.len();
        for &cell in CANONICAL_DECOMPOSITION_BMP_DATA {
            if cell == 0 {
                continue;
            }
            let length = (cell >> 24) as usize;
            let offset = (cell & 0x00FF_FFFF) as usize;
            assert!(
                offset + length <= data_len,
                "packed cell (length={length}, offset={offset}) \
                 overruns _DATA (len={data_len})"
            );
        }
    }

    #[test]
    fn compatibility_decomp_bmp_trie_well_formed() {
        assert_eq!(COMPATIBILITY_DECOMPOSITION_BMP_INDEX.len(), 256);
        assert!(
            COMPATIBILITY_DECOMPOSITION_BMP_DATA.len() % 256 == 0,
            "Stage 2 must be a whole number of 256-cell blocks: len = {}",
            COMPATIBILITY_DECOMPOSITION_BMP_DATA.len()
        );
        assert!(
            COMPATIBILITY_DECOMPOSITION_BMP_DATA[..256]
                .iter()
                .all(|&v| v == 0),
            "Stage 2 block 0 must be the null block"
        );
        let num_blocks = u16::try_from(COMPATIBILITY_DECOMPOSITION_BMP_DATA.len() / 256)
            .expect("BMP compat decomp trie block count must fit in u16");
        for (i, &idx) in COMPATIBILITY_DECOMPOSITION_BMP_INDEX.iter().enumerate() {
            assert!(
                idx < num_blocks,
                "Stage 1 entry {i} points outside Stage 2 (idx = {idx}, \
                 blocks = {num_blocks})"
            );
        }
        let data_len = COMPATIBILITY_DECOMPOSITION_DATA.len();
        for &cell in COMPATIBILITY_DECOMPOSITION_BMP_DATA {
            if cell == 0 {
                continue;
            }
            let length = (cell >> 24) as usize;
            let offset = (cell & 0x00FF_FFFF) as usize;
            assert!(
                offset + length <= data_len,
                "packed cell (length={length}, offset={offset}) \
                 overruns _DATA (len={data_len})"
            );
        }
    }

    #[test]
    fn decomp_supplementary_sorted_and_unique() {
        for window in CANONICAL_DECOMPOSITION_SUPPLEMENTARY.windows(2) {
            assert!(
                window[0].0 < window[1].0,
                "canonical decomp supplementary not sorted strictly"
            );
        }
        for (cp, _) in CANONICAL_DECOMPOSITION_SUPPLEMENTARY {
            assert!(
                *cp >= 0x10000,
                "BMP entry leaked into canonical decomp supplementary: \
                 U+{cp:04X}"
            );
        }
        for window in COMPATIBILITY_DECOMPOSITION_SUPPLEMENTARY.windows(2) {
            assert!(
                window[0].0 < window[1].0,
                "compatibility decomp supplementary not sorted strictly"
            );
        }
        for (cp, _) in COMPATIBILITY_DECOMPOSITION_SUPPLEMENTARY {
            assert!(
                *cp >= 0x10000,
                "BMP entry leaked into compat decomp supplementary: \
                 U+{cp:04X}"
            );
        }
    }

    #[test]
    fn latin_capital_a_grave_canonical_decomposition() {
        // U+00C0 (À) decomposes canonically to U+0041 A + U+0300
        // combining grave (UnicodeData.txt entry verified). R50.2.13 —
        // exercises the BMP trie + recursive fixed-point pipeline
        // end-to-end via the same API its non-test callers use.
        let mut out = Vec::new();
        decompose_canonical(0x00C0, &mut out);
        assert_eq!(out, vec![0x0041, 0x0300]);
    }

    #[test]
    fn latin_small_e_acute_canonical_decomposition() {
        // U+00E9 (é) → U+0065 e + U+0301 combining acute.
        let mut out = Vec::new();
        decompose_canonical(0x00E9, &mut out);
        assert_eq!(out, vec![0x0065, 0x0301]);
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
    fn hangul_syllable_uses_algorithmic_decomposition() {
        // Hangul precomposed syllables (AC00..D7A3) use algorithmic
        // decomposition per UAX #15 §16, not the UCD decomposition
        // table. The canonical / compatibility decompose paths must
        // delegate to the algorithmic branch before consulting the
        // trie. R50.2.13 — verified end-to-end through the public
        // decompose API instead of poking the raw table shape.
        let mut canonical = Vec::new();
        decompose_canonical(0xAC00, &mut canonical);
        // 가 (U+AC00) = L=ᄀ (U+1100) + V=ᅡ (U+1161); LV-only syllable
        // emits no trailing T (UAX #15 §16, Hangul S decomposition).
        assert_eq!(canonical, vec![0x1100, 0x1161]);

        let mut compatibility = Vec::new();
        decompose_compatibility(0xD7A3, &mut compatibility);
        // 힣 (U+D7A3) = L=ᄒ (U+1112) + V=ᅵ (U+1175) + T=ᇂ (U+11C2).
        assert_eq!(compatibility, vec![0x1112, 0x1175, 0x11C2]);
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
        // UAX #15 D5 (canonical composition). R50.2.14 — exercises
        // the two-level trie + per-`a` `binary_search` directly
        // through the same API the composition pipeline uses.
        assert_eq!(compose_pair(0x0041, 0x0300), Some(0x00C0));
    }

    #[test]
    fn primary_composite_e_acute_via_trie() {
        // (U+0065, U+0301) → U+00E9 (é). Different `a` sub-table
        // than the grave; second probe verifies the trie reaches
        // multiple `a` groups, not just the first hit.
        assert_eq!(compose_pair(0x0065, 0x0301), Some(0x00E9));
    }

    #[test]
    fn primary_composite_no_composite_returns_none() {
        // (U+0041, U+0061) is not a primary composite — `a` is in
        // the envelope (Latin range) but no `b == U+0061` entry
        // exists for this `a` sub-table.
        assert_eq!(compose_pair(0x0041, 0x0061), None);
        // (U+FFFF, U+0300) — `a` outside the envelope, anchor
        // short-circuit fires before the trie.
        assert_eq!(compose_pair(0xFFFF, 0x0300), None);
    }

    #[test]
    fn primary_composites_exclude_full_composition_set() {
        // For each (a, b) -> c, c must not be in
        // FULL_COMPOSITION_EXCLUSION (invariant from build.rs).
        // R50.2.14 — iterate via the two-level trie shape.
        for (_, _, c) in iter_primary_composites_for_test() {
            assert!(
                FULL_COMPOSITION_EXCLUSION.binary_search(&c).is_err(),
                "composite 0x{c:04X} is in exclusion set"
            );
        }
    }

    #[test]
    fn primary_composites_sub_tables_sorted_by_b() {
        // R50.2.14 — each per-`a` sub-table is `binary_search`-ed
        // by `b`, so each sub-slice in `_BC_DATA` must be sorted
        // strictly ascending on `b`. Catches `sort_by_key` drift
        // in `build_primary_composites_trie` before the lookup
        // returns a wrong composite.
        for (high, &block_idx) in PRIMARY_COMPOSITES_BMP_INDEX.iter().enumerate() {
            let block = block_idx as usize;
            let block_data = &PRIMARY_COMPOSITES_BMP_DATA[block * 256..(block + 1) * 256];
            for (low, &cell) in block_data.iter().enumerate() {
                if cell == 0 {
                    continue;
                }
                let length = (cell >> 24) as usize;
                let offset = (cell & 0x00FF_FFFF) as usize;
                let sub = &PRIMARY_COMPOSITES_BC_DATA[offset..offset + length];
                let a = u32::try_from((high << 8) | low).expect("BMP codepoint always fits in u32");
                for window in sub.windows(2) {
                    assert!(
                        window[0].0 < window[1].0,
                        "PRIMARY_COMPOSITES sub-table for U+{a:04X} not \
                         sorted by `b`: {window:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn primary_composites_bmp_trie_well_formed() {
        // R50.2.14 — Stage 1 size, Stage 2 block alignment, null
        // block sentinel, and Stage 1 indices in range. Mirrors
        // the R50.2.13 decomp trie invariant tests.
        assert_eq!(PRIMARY_COMPOSITES_BMP_INDEX.len(), 256);
        assert!(
            PRIMARY_COMPOSITES_BMP_DATA.len() % 256 == 0,
            "Stage 2 must be a whole number of 256-cell blocks: len = {}",
            PRIMARY_COMPOSITES_BMP_DATA.len()
        );
        assert!(
            PRIMARY_COMPOSITES_BMP_DATA[..256].iter().all(|&v| v == 0),
            "Stage 2 block 0 must be the null block"
        );
        let num_blocks = u16::try_from(PRIMARY_COMPOSITES_BMP_DATA.len() / 256)
            .expect("BMP primary-composites trie block count must fit in u16");
        for (i, &idx) in PRIMARY_COMPOSITES_BMP_INDEX.iter().enumerate() {
            assert!(
                idx < num_blocks,
                "Stage 1 entry {i} points outside Stage 2 (idx = {idx}, \
                 blocks = {num_blocks})"
            );
        }
        let bc_len = PRIMARY_COMPOSITES_BC_DATA.len();
        for &cell in PRIMARY_COMPOSITES_BMP_DATA {
            if cell == 0 {
                continue;
            }
            let length = (cell >> 24) as usize;
            let offset = (cell & 0x00FF_FFFF) as usize;
            assert!(
                offset + length <= bc_len,
                "packed cell (length={length}, offset={offset}) \
                 overruns _BC_DATA (len={bc_len})"
            );
        }
    }

    /// R50.2.14 — walk both the BMP two-level trie and the
    /// supplementary sparse fallback to yield every `(a, b, c)`
    /// triple. Used by the test module's exclusion guard; not
    /// exposed outside of `#[cfg(test)]`.
    fn iter_primary_composites_for_test() -> Vec<(u32, u32, u32)> {
        let mut out = Vec::with_capacity(PRIMARY_COMPOSITES_ENTRY_COUNT);
        for (high, &block_idx) in PRIMARY_COMPOSITES_BMP_INDEX.iter().enumerate() {
            let block = block_idx as usize;
            let block_data = &PRIMARY_COMPOSITES_BMP_DATA[block * 256..(block + 1) * 256];
            for (low, &cell) in block_data.iter().enumerate() {
                if cell == 0 {
                    continue;
                }
                let length = (cell >> 24) as usize;
                let offset = (cell & 0x00FF_FFFF) as usize;
                let a = u32::try_from((high << 8) | low).expect("BMP codepoint always fits in u32");
                for &(b, c) in &PRIMARY_COMPOSITES_BC_DATA[offset..offset + length] {
                    out.push((a, b, c));
                }
            }
        }
        for (a, sub) in PRIMARY_COMPOSITES_SUPPLEMENTARY {
            for &(b, c) in *sub {
                out.push((*a, b, c));
            }
        }
        // Iteration size invariant: every (a, b, c) the build
        // emitted must surface exactly once.
        assert_eq!(
            out.len(),
            PRIMARY_COMPOSITES_ENTRY_COUNT,
            "trie walker missed entries vs build-time count"
        );
        out
    }

    #[test]
    fn primary_composites_supplementary_sorted_and_well_formed() {
        // R50.2.14 — supplementary outer slice sorted by `a`,
        // each inner sub-table sorted by `b`, every `a` in the
        // supplementary plane.
        for window in PRIMARY_COMPOSITES_SUPPLEMENTARY.windows(2) {
            assert!(
                window[0].0 < window[1].0,
                "PRIMARY_COMPOSITES_SUPPLEMENTARY outer not sorted by `a`"
            );
        }
        for (a, sub) in PRIMARY_COMPOSITES_SUPPLEMENTARY {
            assert!(
                *a >= 0x10000,
                "BMP `a` leaked into supplementary: U+{a:04X}"
            );
            for window in sub.windows(2) {
                assert!(
                    window[0].0 < window[1].0,
                    "supplementary sub-table for U+{a:04X} not sorted: \
                     {window:?}"
                );
            }
        }
    }
}
