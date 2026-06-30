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

// (R1171 §5.16) Window-control glyphs for a floating dock panel's HEADER controls
// (minimize / maximize / close). Text glyphs — the widget-layer convention (like
// the disclosure twisty above) — so they lay out with the header font + flex and
// auto-size to the header height, NOT a fixed-pixel shell overlay the binding has
// to dimension-match (the R1170 smell the controls-in-header redesign cleared).
// Chosen from blocks the bundled fonts cover (the geometric-shapes `U+25A1` pairs
// with the disclosure triangles already in use; `U+2212` / `U+00D7` are basic math).

/// Minimize control — `U+2212` MINUS SIGN (a centred bar reads as minimize).
pub const WINDOW_MINIMIZE: &str = "\u{2212}";

/// Maximize / restore control — `U+25A1` WHITE SQUARE.
pub const WINDOW_MAXIMIZE: &str = "\u{25A1}";

/// Close control — `U+00D7` MULTIPLICATION SIGN.
pub const WINDOW_CLOSE: &str = "\u{00D7}";

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
