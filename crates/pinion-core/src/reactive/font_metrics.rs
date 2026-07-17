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
//! seeds it on the root `Owner` at boot into [`MONOSPACE_METRICS`]; a producer
//! reads it with [`measured_monospace_cell`].
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

use super::provider_slot::ProviderSlot;
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

/// R1366.3 §5.36 — the monospace-metrics slot: its key, its Null default and
/// its inherit verdict as one expression, in the module that owns the capability.
///
/// **`Inherited`** by the mechanical predicate — the shell DRIVES this at the
/// root owner: `ShellCore::new_with_seed` seeds the `pinion-text`-backed provider
/// before any binding factory or first `view` reads it, so a child scope resolves
/// that root value through [`Owner::cache_inherited`](super::owner::Owner::cache_inherited).
///
/// Under the deferred R680 atomic — each window's `view` running in
/// `window_owner(id).run(..)` — a per-scope verdict would hand a secondary window
/// a freshly minted [`NullMonospaceMetrics`], whose measurement is `None`, so
/// every [`Scene::TextGrid`](crate::scene::Scene) in that window would silently
/// fall to a producer-picked cell and render loose — the same silent-desync class
/// R1362 fixed, with no panic and no log.
/// [`provider_slot_tests!`](crate::provider_slot_tests) EMITS the verdict from
/// this declaration, so it cannot be forgotten the way R1365 forgot five.
///
/// The payload is the `Rc<dyn MonospaceMetrics>` itself, with no newtype wrapper:
/// [`Owner::cache`](super::owner::Owner::cache) keys on `(TypeId::of::<V>(),
/// key)`, so the trait object is already its own type. R1003's
/// `MonospaceMetricsHolder` was the `Rc<dyn Any>` storage showing through — the
/// wrapper R1366.1 retired for [`REPAINT_SINK`](super::repaint::REPAINT_SINK).
/// Unlike the sinks this rides an `Rc`, not `Arc`: measurement is read only from
/// the UI-thread `view`, so it carries no `Send + Sync` bound (only `fn() -> V`
/// names the payload, and fn pointers are `Sync`, so the `static` is still sound).
pub static MONOSPACE_METRICS: ProviderSlot<Rc<dyn MonospaceMetrics>> =
    ProviderSlot::inherited("__pinion.reactive.monospace_metrics", || {
        Rc::new(NullMonospaceMetrics)
    });

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
    let owner = super::owner::Owner::current()?;
    MONOSPACE_METRICS
        .resolve(&owner)
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

    // The verdict, EMITTED from the declaration rather than remembered — the
    // generated `Inherited` check R1365 forgot for five of its slots.
    crate::provider_slot_tests!(
        r1366_3_monospace_metrics_inherits,
        super::MONOSPACE_METRICS,
        || -> Rc<dyn MonospaceMetrics> { Rc::new(FixedMetrics) }
    );

    #[test]
    fn null_default_measures_none() {
        // No provider seeded: the lazy default is NullMonospaceMetrics.
        let owner = Owner::new();
        assert_eq!(MONOSPACE_METRICS.resolve(&owner).monospace_cell(32), None);
    }

    #[test]
    fn provided_metrics_are_returned() {
        let owner = Owner::new();
        MONOSPACE_METRICS.provide(&owner, Rc::new(FixedMetrics));
        assert_eq!(
            MONOSPACE_METRICS.resolve(&owner).monospace_cell(32),
            CellMetric::new(16, 32)
        );
    }

    #[test]
    #[should_panic(expected = "already seeded")]
    fn r1366_3_a_late_seed_panics_where_it_used_to_be_dropped() {
        // The counterfactual of R1003's `provide_is_first_write_wins`, which
        // asserted a second seed was a SILENT no-op leaving every reader on the
        // first provider. A dropped metrics seed is a grid that measures nothing
        // and renders loose (R1002: only the measured metric is snug), and a
        // shell that seeds twice cannot know which provider a producer's `view`
        // resolved — a wiring bug, not an idempotent convenience.
        let owner = Owner::new();
        MONOSPACE_METRICS.provide(&owner, Rc::new(FixedMetrics));
        MONOSPACE_METRICS.provide(&owner, Rc::new(NullMonospaceMetrics));
    }

    #[test]
    fn measured_resolves_inside_owner_run() {
        let owner = Owner::new();
        MONOSPACE_METRICS.provide(&owner, Rc::new(FixedMetrics));
        let got = owner.run(|| measured_monospace_cell(40));
        assert_eq!(got, CellMetric::new(20, 40));
    }

    #[test]
    fn measured_is_none_outside_owner_scope() {
        // No active Owner scope ⇒ graceful None (producer falls back), not a
        // panic — distinct from the strict `use_*` hooks.
        assert_eq!(measured_monospace_cell(32), None);
    }

    #[test]
    fn r1365_1_a_child_scope_resolves_the_shells_real_metrics() {
        // R1365.1 §5.22 — the verdict through the BINDING's path. The generated
        // test above asserts inheritance through `resolve`; this asserts the same
        // through `measured_monospace_cell`, so a hook that stopped delegating to
        // the slot could not pass both. `Owner::new_child` is what R680 runs a
        // secondary window's view in, and the Null default here measures nothing,
        // so a TUI-side secondary would silently lose its cell metrics.
        let root = Owner::new();
        MONOSPACE_METRICS.provide(&root, Rc::new(FixedMetrics));

        let window_scope = Owner::new_child(&root);
        let got = window_scope.run(|| measured_monospace_cell(40));

        assert_eq!(
            got,
            CellMetric::new(20, 40),
            "a child scope resolved the Null metrics instead of the shell's",
        );
    }
}
