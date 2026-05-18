//! R50.2.1 §5.37.3 `pinion-text-unicode` — self-hosted Unicode
//! normalization (UAX #15 NFC/NFD/NFKC/NFKD).
//!
//! `unicode-normalization` / `icu` / `parley` 등 black box crate 가
//! 아닌, UCD 16.x decomposition + `canonical_combining_class` +
//! `composition_exclusions` table 자체 embed (R50.2.2 `build.rs`
//! codegen). R50 정신 완전 적용 — 외부 dependency 0개.
//!
//! Crate roadmap (R50.2.x sub-phase chain per §5.37.3):
//!
//! * R50.2.0 — atomic-only §5.37.3 ratify (spec round, no impl).
//! * R50.2.1 — crate scaffold + [`NormForm`] enum (this commit).
//! * R50.2.2 — UCD 16.x source vendor + `build.rs` table codegen.
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
