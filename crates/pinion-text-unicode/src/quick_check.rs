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
    NFC_QC_NON_YES, NFD_QC_NO, NFKC_QC_NON_YES, NFKD_QC_NO,
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

/// `binary_search` membership probe for the `No`-only tables
/// (`NFD_QC_NO` / `NFKD_QC_NO`).
#[inline]
fn lookup_no(table: &'static [u32], cp: u32) -> bool {
    table.binary_search(&cp).is_ok()
}

/// `binary_search_by_key` for the ternary tables
/// (`NFC_QC_NON_YES` / `NFKC_QC_NON_YES`). Returns `None` when the
/// codepoint is absent (default `Yes`); `Some(value)` otherwise.
#[inline]
fn lookup_ynm(table: &'static [(u32, u8)], cp: u32) -> Option<u8> {
    table
        .binary_search_by_key(&cp, |(c, _)| *c)
        .ok()
        .map(|idx| table[idx].1)
}

/// Quick-check for `Yes`/`No`-only forms (NFD, NFKD). Combining-
/// class ordering violation is treated as `No`.
fn quick_check_yn(s: &str, table: &'static [u32]) -> QuickCheck {
    let mut last_class: u8 = 0;
    for c in s.chars() {
        let cp = c as u32;
        let class = combining_class(cp);
        if last_class > class && class != 0 {
            return QuickCheck::No;
        }
        if lookup_no(table, cp) {
            return QuickCheck::No;
        }
        last_class = class;
    }
    QuickCheck::Yes
}

/// Quick-check for ternary forms (NFC, NFKC).
fn quick_check_ynm(s: &str, table: &'static [(u32, u8)]) -> QuickCheck {
    let mut last_class: u8 = 0;
    let mut result = QuickCheck::Yes;
    for c in s.chars() {
        let cp = c as u32;
        let class = combining_class(cp);
        if last_class > class && class != 0 {
            return QuickCheck::No;
        }
        if let Some(qc) = lookup_ynm(table, cp) {
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
    quick_check_ynm(s, NFC_QC_NON_YES)
}

pub(crate) fn nfd_quick_check(s: &str) -> QuickCheck {
    quick_check_yn(s, NFD_QC_NO)
}

pub(crate) fn nfkc_quick_check(s: &str) -> QuickCheck {
    quick_check_ynm(s, NFKC_QC_NON_YES)
}

pub(crate) fn nfkd_quick_check(s: &str) -> QuickCheck {
    quick_check_yn(s, NFKD_QC_NO)
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
