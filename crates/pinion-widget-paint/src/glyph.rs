//! R873 §5.50 — shared paint-glyph SSOT.
//!
//! The **disclosure twisty** — the collapsed (`U+25B6` BLACK RIGHT-POINTING
//! TRIANGLE) / expanded (`U+25BC` BLACK DOWN-POINTING TRIANGLE) pair — is one
//! affordance used by every collapsible surface in the catalog: a
//! [`crate::disclosure`] section, a [`crate::tree_view`] branch, and a
//! [`crate::group_header`] category row all show the *same* twisty so they read
//! as the same gesture. Before R873 each module re-declared the pair privately
//! (three byte-identical copies, each doc-cross-referencing the others as "the
//! same glyph") — the Rule-of-Three SSOT miss the R758 self-grep mandate names
//! ([[self-grep-count-all-sites-not-just-new-pair]]). They lift here.
//!
//! The **column-sort direction** pair (`U+25B2` ascending / `U+25BC`
//! descending) lifts here too (R886.1) — by then FIVE same-semantic copies
//! existed (this crate's table header + four grid examples), the same
//! Rule-of-Three class as the twisty. It stays a *separate* affordance
//! from the disclosure pair: `U+25BC` recurring in both is a glyph
//! coincidence, not a shared gesture, so the two pairs are distinct
//! constants (merging them would be the R735.1 wrong abstraction). The
//! datepicker month-nav arrows (`U+25C0` / `U+25B6`) remain deliberately
//! un-lifted for the same semantics reason. A consumer's *unsorted*
//! representation (`""`, `"\u{2195}"`, a fixed-width blank) is a style
//! choice, not a shared decision — it stays per-consumer (R758).

/// Collapsed-state disclosure twisty — `U+25B6` BLACK RIGHT-POINTING TRIANGLE.
pub const DISCLOSURE_COLLAPSED: &str = "\u{25B6}";

/// Expanded-state disclosure twisty — `U+25BC` BLACK DOWN-POINTING TRIANGLE.
pub const DISCLOSURE_EXPANDED: &str = "\u{25BC}";

/// Ascending column-sort arrow — `U+25B2` BLACK UP-POINTING TRIANGLE.
pub const SORT_ASCENDING: &str = "\u{25B2}";

/// Descending column-sort arrow — `U+25BC` BLACK DOWN-POINTING TRIANGLE.
pub const SORT_DESCENDING: &str = "\u{25BC}";

/// R886.1 §5.50 — the sort-direction → glyph mapping every column header
/// paints: `Some(true)` → [`SORT_ASCENDING`], `Some(false)` →
/// [`SORT_DESCENDING`], `None` (not the active sort column) → `None` so
/// each consumer renders its own unsorted representation. Pairs with
/// `pinion_core::widgets::grid_sort::col_sort_dir` (the "is THIS column
/// active" decision) on the input side.
#[must_use]
pub const fn sort_glyph(dir: Option<bool>) -> Option<&'static str> {
    match dir {
        Some(true) => Some(SORT_ASCENDING),
        Some(false) => Some(SORT_DESCENDING),
        None => None,
    }
}
