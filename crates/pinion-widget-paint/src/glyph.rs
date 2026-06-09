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
//! Deliberately **not** folded in: the datepicker month-nav arrows
//! (`U+25C0` / `U+25B6`) and the data-grid sort arrows (`U+25B2` / `U+25BC`).
//! `U+25B6` and `U+25BC` recur there, but as *different semantics* (navigate /
//! sort-direction) — a glyph coincidence, not a shared affordance, so merging
//! them would be the wrong abstraction (R735.1).

/// Collapsed-state disclosure twisty — `U+25B6` BLACK RIGHT-POINTING TRIANGLE.
pub const DISCLOSURE_COLLAPSED: &str = "\u{25B6}";

/// Expanded-state disclosure twisty — `U+25BC` BLACK DOWN-POINTING TRIANGLE.
pub const DISCLOSURE_EXPANDED: &str = "\u{25BC}";
