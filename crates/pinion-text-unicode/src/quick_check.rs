//! UAX #15 §5 — Quick-check fast path.
//!
//! Each form has a derived `<form>_Quick_Check` Unicode property
//! taking values `Yes`, `No`, or `Maybe`. NFD/NFKD admit only
//! `Yes`/`No`; NFC/NFKC additionally admit `Maybe` for combining
//! marks whose composition outcome depends on context.
//!
//! The check rules (from UAX #15 §5):
//!
//! 1. If a codepoint's CCC is less than the previous codepoint's
//!    non-zero CCC, the input violates canonical order → `No`.
//! 2. If any codepoint maps to `No`, the input is definitely not
//!    in this form → `No`.
//! 3. If any codepoint maps to `Maybe` and step 1/2 didn't trigger,
//!    the input is `Maybe` — full algorithm is required to decide.
//! 4. Otherwise → `Yes` (the input is already in this form).
//!
//! The `Yes` outcome lets [`crate::normalize`] return a borrowed
//! `Cow` and skip the full pipeline entirely.

use crate::ordering::combining_class;
use crate::tables::{
    NFC_QC_FIRST_NON_YES_CP, NFC_QC_NON_YES_BMP_DATA,
    NFC_QC_NON_YES_BMP_INDEX, NFC_QC_NON_YES_SUPPLEMENTARY,
    NFD_QC_FIRST_NO_CP, NFD_QC_NO_BMP_DATA, NFD_QC_NO_BMP_INDEX,
    NFD_QC_NO_SUPPLEMENTARY, NFKC_QC_FIRST_NON_YES_CP,
    NFKC_QC_NON_YES_BMP_DATA, NFKC_QC_NON_YES_BMP_INDEX,
    NFKC_QC_NON_YES_SUPPLEMENTARY, NFKD_QC_FIRST_NO_CP,
    NFKD_QC_NO_BMP_DATA, NFKD_QC_NO_BMP_INDEX, NFKD_QC_NO_SUPPLEMENTARY,
};

/// Ternary Quick-check verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickCheck {
    /// Input is already in the requested form; identity transform.
    Yes,
    /// Input is definitely not in the requested form.
    No,
    /// Outcome depends on context; full algorithm required.
    Maybe,
}

/// R50.2.11 — shared 2-stage BMP trie lookup. Returns `None` for
/// codepoints below the build-derived `anchor` (default value), uses
/// the `(BMP_INDEX, BMP_DATA)` trie for BMP codepoints, and falls
/// back to a sparse `binary_search` for supplementary-plane
/// codepoints. Stage-2 value `0` encodes "not in table" (i.e. the
/// default `Yes` for QC tables, mapped back to `None` here); NFD/
/// NFKD callers convert any `Some(_)` to membership `true`.
///
/// All three branches (anchor / BMP / supplementary) live in one
/// function and rely on the compiler's default inlining. Earlier
/// R50.2.11 iterations tried hoisting the supplementary path into a
/// `#[cold]` helper and into a plain free function; both layouts
/// shifted regressions around without delivering a universal win on
/// the 5-scenario bench. The single-function shape is the simplest
/// textbook canonical and produced the best overall envelope.
#[inline]
fn lookup_u8_trie(
    index: &'static [u16; 256],
    data: &'static [u8],
    supplementary: &'static [(u32, u8)],
    anchor: u32,
    cp: u32,
) -> Option<u8> {
    if cp < anchor {
        return None;
    }
    if cp < 0x10000 {
        let block = index[(cp >> 8) as usize] as usize;
        let value = data[block * 256 + (cp & 0xFF) as usize];
        return if value == 0 { None } else { Some(value) };
    }
    supplementary
        .binary_search_by_key(&cp, |(c, _)| *c)
        .ok()
        .map(|idx| supplementary[idx].1)
}

/// Quick-check for `Yes`/`No`-only forms (NFD, NFKD). Combining-
/// class ordering violation is treated as `No`.
fn quick_check_yn(
    s: &str,
    index: &'static [u16; 256],
    data: &'static [u8],
    supplementary: &'static [(u32, u8)],
    anchor: u32,
) -> QuickCheck {
    let mut last_class: u8 = 0;
    for c in s.chars() {
        let cp = c as u32;
        let class = combining_class(cp);
        if last_class > class && class != 0 {
            return QuickCheck::No;
        }
        if lookup_u8_trie(index, data, supplementary, anchor, cp).is_some() {
            return QuickCheck::No;
        }
        last_class = class;
    }
    QuickCheck::Yes
}

/// Quick-check for ternary forms (NFC, NFKC).
fn quick_check_ynm(
    s: &str,
    index: &'static [u16; 256],
    data: &'static [u8],
    supplementary: &'static [(u32, u8)],
    anchor: u32,
) -> QuickCheck {
    let mut last_class: u8 = 0;
    let mut result = QuickCheck::Yes;
    for c in s.chars() {
        let cp = c as u32;
        let class = combining_class(cp);
        if last_class > class && class != 0 {
            return QuickCheck::No;
        }
        if let Some(qc) =
            lookup_u8_trie(index, data, supplementary, anchor, cp)
        {
            match qc {
                1 => return QuickCheck::No,
                2 => result = QuickCheck::Maybe,
                _ => {}
            }
        }
        last_class = class;
    }
    result
}

pub(crate) fn nfc_quick_check(s: &str) -> QuickCheck {
    quick_check_ynm(
        s,
        NFC_QC_NON_YES_BMP_INDEX,
        NFC_QC_NON_YES_BMP_DATA,
        NFC_QC_NON_YES_SUPPLEMENTARY,
        NFC_QC_FIRST_NON_YES_CP,
    )
}

pub(crate) fn nfd_quick_check(s: &str) -> QuickCheck {
    quick_check_yn(
        s,
        NFD_QC_NO_BMP_INDEX,
        NFD_QC_NO_BMP_DATA,
        NFD_QC_NO_SUPPLEMENTARY,
        NFD_QC_FIRST_NO_CP,
    )
}

pub(crate) fn nfkc_quick_check(s: &str) -> QuickCheck {
    quick_check_ynm(
        s,
        NFKC_QC_NON_YES_BMP_INDEX,
        NFKC_QC_NON_YES_BMP_DATA,
        NFKC_QC_NON_YES_SUPPLEMENTARY,
        NFKC_QC_FIRST_NON_YES_CP,
    )
}

pub(crate) fn nfkd_quick_check(s: &str) -> QuickCheck {
    quick_check_yn(
        s,
        NFKD_QC_NO_BMP_INDEX,
        NFKD_QC_NO_BMP_DATA,
        NFKD_QC_NO_SUPPLEMENTARY,
        NFKD_QC_FIRST_NO_CP,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        nfc_quick_check, nfd_quick_check, nfkc_quick_check,
        nfkd_quick_check, QuickCheck,
    };

    #[test]
    fn ascii_is_yes_for_all_forms() {
        let s = "Hello, world!";
        assert_eq!(nfc_quick_check(s), QuickCheck::Yes);
        assert_eq!(nfd_quick_check(s), QuickCheck::Yes);
        assert_eq!(nfkc_quick_check(s), QuickCheck::Yes);
        assert_eq!(nfkd_quick_check(s), QuickCheck::Yes);
    }

    #[test]
    fn precomposed_a_grave_is_nfc_yes_nfd_no() {
        let s = "\u{00C0}"; // À
        assert_eq!(nfc_quick_check(s), QuickCheck::Yes);
        assert_eq!(nfd_quick_check(s), QuickCheck::No);
    }

    #[test]
    fn decomposed_a_grave_is_nfc_maybe_nfd_yes() {
        let s = "A\u{0300}";
        assert_eq!(nfc_quick_check(s), QuickCheck::Maybe);
        assert_eq!(nfd_quick_check(s), QuickCheck::Yes);
    }

    #[test]
    fn out_of_order_marks_is_no() {
        // grave (CCC 230) before dot-below (CCC 220) violates
        // canonical order — any form must reject as No.
        let s = "A\u{0300}\u{0323}";
        assert_eq!(nfc_quick_check(s), QuickCheck::No);
        assert_eq!(nfd_quick_check(s), QuickCheck::No);
        assert_eq!(nfkc_quick_check(s), QuickCheck::No);
        assert_eq!(nfkd_quick_check(s), QuickCheck::No);
    }

    #[test]
    fn ligature_fi_is_nfkd_no() {
        // ﬁ has NFKD decomposition into "fi"; quick-check rejects.
        let s = "\u{FB01}";
        assert_eq!(nfkd_quick_check(s), QuickCheck::No);
        assert_eq!(nfkc_quick_check(s), QuickCheck::No);
    }

    #[test]
    fn hangul_precomposed_is_nfd_no() {
        // 한 (U+D55C) decomposes algorithmically — NFD_QC table
        // marks the Hangul syllable range as `No`.
        let s = "\u{D55C}";
        assert_eq!(nfd_quick_check(s), QuickCheck::No);
    }
}
