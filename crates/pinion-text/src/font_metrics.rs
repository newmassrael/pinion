//! R1003 §5.36 — [`LayoutCacheMonospaceMetrics`]: the parley-backed
//! [`MonospaceMetrics`] provider.
//!
//! This is the concrete impl of the pinion-core [`MonospaceMetrics`] boundary
//! (the `Executor` / `RepaintSink` boundary-trait pattern: core declares the
//! trait, this crate supplies the parley-backed impl). The shell seeds one on
//! the root `Owner` at boot so the pure, sync `view` fn can derive a
//! font-correct [`CellMetric`] for a `Scene::TextGrid` via
//! [`measured_monospace_cell`](pinion_core::measured_monospace_cell) without
//! owning a `FontContext`.

use crate::cache::LayoutCache;
use pinion_core::{CellMetric, MonospaceMetrics};
use std::cell::RefCell;

/// R1003 §5.36 — a [`MonospaceMetrics`] provider backed by a dedicated
/// [`LayoutCache`].
///
/// Holds its own cache (not the paint cache) so measurement never entangles
/// with rendering. The interior [`RefCell`] bridges the trait's `&self` to
/// [`LayoutCache::measure_monospace_cell`]'s `&mut self`; single-thread (the
/// view runs on the UI thread), so a `RefCell` suffices — no lock. Repeated
/// measurements at the same size hit the cache's LRU.
#[derive(Default)]
pub struct LayoutCacheMonospaceMetrics {
    cache: RefCell<LayoutCache>,
}

impl LayoutCacheMonospaceMetrics {
    /// Construct a provider with a fresh measurement cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache: RefCell::new(LayoutCache::new()),
        }
    }
}

impl MonospaceMetrics for LayoutCacheMonospaceMetrics {
    fn monospace_cell(&self, font_size_px: u32) -> Option<CellMetric> {
        self.cache.borrow_mut().measure_monospace_cell(font_size_px)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;
    use std::rc::Rc;

    /// The provider, seeded on an `Owner`, delivers the *same* measurement the
    /// cache produces directly — proving the seam carries the real metric into
    /// an owner scope (where a pure `view` reads it). The forcing consumer of
    /// the R1003 seam (it would catch a regression in the wiring or the bridge).
    #[test]
    fn seam_delivers_measured_cell_into_owner_scope() {
        let direct = LayoutCache::new()
            .measure_monospace_cell(32)
            .expect("32px monospace measures");

        let owner = Owner::new();
        pinion_core::MONOSPACE_METRICS.provide(&owner, Rc::new(LayoutCacheMonospaceMetrics::new()));
        let via_seam = owner.run(|| pinion_core::measured_monospace_cell(32));

        assert_eq!(
            via_seam,
            Some(direct),
            "seam must deliver the measured cell"
        );
        assert!(
            direct.cell_w() < direct.cell_h(),
            "measured cell is taller than wide"
        );
    }
}
