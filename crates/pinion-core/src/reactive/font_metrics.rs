//! R1003 §5.36 — `MonospaceMetrics`: the view-time "measure the resolved
//! monospace cell" boundary.
//!
//! # Why this exists
//!
//! pinion's `view` is a pure, sync function (§6.3) and cannot own a parley
//! `FontContext`. Yet a font-correct [`Scene::TextGrid`](crate::scene::Scene)
//! needs `cell_w == the monospace advance` — the R1002 measured
//! [`CellMetric`], which requires shaping. A producer that hardcodes a
//! `cell_w` renders loose (R1002 finding: only the measured metric is snug).
//!
//! [`MonospaceMetrics`] is the one edge that lets the shell — which owns the
//! font context — provide a *measurement capability* the pure view reads. It
//! is the read sibling of the [`RepaintSink`](super::repaint::RepaintSink)
//! wake edge and follows the same §6.3 boundary-trait pattern
//! (pinion-core defines the abstract trait; pinion-text supplies the
//! `LayoutCache`-backed impl), so this layer never sees parley. The shell
//! seeds it on the root `Owner` at boot via
//! [`Owner::provide_monospace_metrics`](super::owner::Owner::provide_monospace_metrics);
//! a producer reads it with [`measured_monospace_cell`].
//!
//! # Purity
//!
//! Measuring a font is **deterministic** (a property of the resolved face), so
//! reading it from `view` preserves the §6.3 purity / `dry_run` guarantee. Off
//! the live shell (headless / RPC / unit tests) no provider is seeded and the
//! [`NullMonospaceMetrics`] default returns `None`, so a producer falls back to
//! a producer-picked metric (the fit path) deterministically.
//!
//! # Single-thread
//!
//! Unlike [`RepaintSink`](super::repaint::RepaintSink) (which crosses to a
//! producer thread, hence `Send + Sync + Arc`), measurement is read only from
//! the UI-thread `view`, so this rides an `Rc` with no thread-safety bound.

use crate::cell_metric::CellMetric;
use std::rc::Rc;

/// R1003 §5.36 — the shell-supplied "measure the resolved monospace cell at a
/// font size" edge.
///
/// `monospace_cell(font_size_px)` returns the [`CellMetric`] whose `cell_w` is
/// the glyph pen advance and `cell_h` the ceil'd natural line box — pair it
/// with [`TextGridNode::with_font_size_px`](crate::scene::TextGridNode::with_font_size_px)`(font_size_px)`
/// for a grid that is snug by construction. `None` when no real face resolves.
pub trait MonospaceMetrics {
    /// Measure the monospace cell for a glyph of `font_size_px` logical pixels.
    fn monospace_cell(&self, font_size_px: u32) -> Option<CellMetric>;
}

/// R1003 §5.36 — Null Object [`MonospaceMetrics`] (the default when no shell
/// has provided a real one: headless screenshot, RPC-driven tests, unit
/// tests). Returns `None` — there is no font context to measure — so a
/// producer falls back to a producer-picked cell. [`measured_monospace_cell`]
/// is therefore callable unconditionally.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullMonospaceMetrics;

impl MonospaceMetrics for NullMonospaceMetrics {
    fn monospace_cell(&self, _font_size_px: u32) -> Option<CellMetric> {
        None
    }
}

/// Owner-cache newtype: [`Owner::cache`](super::owner::Owner::cache) stores
/// `Rc<dyn Any>`, so the trait object rides inside this holder (mirrors
/// `RepaintSinkHolder`).
pub(crate) struct MonospaceMetricsHolder(pub(crate) Rc<dyn MonospaceMetrics>);

/// Private owner-cache key for the single per-owner monospace-metrics slot
/// (mirrors the `RepaintSink` / `LocalTaskPump` private-key convention).
pub(crate) const MONOSPACE_METRICS_KEY: &str = "__pinion.reactive.monospace_metrics";

/// R1003 §5.36 — measure the monospace cell at `font_size_px` via the active
/// owner scope's provider; `None` when called outside an `Owner` scope or when
/// no shell seeded a real provider (headless). The single call a
/// [`Scene::TextGrid`](crate::scene::Scene) producer makes in its view fn:
///
/// ```ignore
/// let (cell, font) = match measured_monospace_cell(px) {
///     Some(cell) => (cell, Some(px)),     // measured: snug
///     None => (CellMetric::DEFAULT, None) // headless: producer-picked fit
/// };
/// ```
///
/// Graceful (`Option` throughout, no panic) rather than the strict
/// `use_repaint_sink` shape, because a producer's view fn may be exercised
/// directly in tests without an installed provider.
#[must_use]
pub fn measured_monospace_cell(font_size_px: u32) -> Option<CellMetric> {
    super::owner::Owner::current()?
        .monospace_metrics()
        .monospace_cell(font_size_px)
}

#[cfg(test)]
mod tests {
    use super::super::owner::Owner;
    use super::*;
    use crate::cell_metric::CellMetric;

    /// A fixed provider: returns a constant cell scaled by the requested size,
    /// so a test can assert the seam delivered *this* provider's measurement.
    struct FixedMetrics;
    impl MonospaceMetrics for FixedMetrics {
        fn monospace_cell(&self, font_size_px: u32) -> Option<CellMetric> {
            CellMetric::new(font_size_px / 2, font_size_px)
        }
    }

    #[test]
    fn null_default_measures_none() {
        // No provider seeded: the lazy default is NullMonospaceMetrics.
        let owner = Owner::new();
        assert_eq!(owner.monospace_metrics().monospace_cell(32), None);
    }

    #[test]
    fn provided_metrics_are_returned() {
        let owner = Owner::new();
        owner.provide_monospace_metrics(Rc::new(FixedMetrics));
        assert_eq!(
            owner.monospace_metrics().monospace_cell(32),
            CellMetric::new(16, 32)
        );
    }

    #[test]
    fn provide_is_first_write_wins() {
        let owner = Owner::new();
        owner.provide_monospace_metrics(Rc::new(FixedMetrics));
        owner.provide_monospace_metrics(Rc::new(NullMonospaceMetrics));
        // The first (FixedMetrics) stays installed; the second is dropped.
        assert_eq!(
            owner.monospace_metrics().monospace_cell(32),
            CellMetric::new(16, 32)
        );
    }

    #[test]
    fn measured_resolves_inside_owner_run() {
        let owner = Owner::new();
        owner.provide_monospace_metrics(Rc::new(FixedMetrics));
        let got = owner.run(|| measured_monospace_cell(40));
        assert_eq!(got, CellMetric::new(20, 40));
    }

    #[test]
    fn measured_is_none_outside_owner_scope() {
        // No active Owner scope ⇒ graceful None (producer falls back), not a
        // panic — distinct from the strict `use_*` hooks.
        assert_eq!(measured_monospace_cell(32), None);
    }
}
