//! R682.B §5.16 — GUI-agnostic snapshot of a paint-fragment cache's
//! observable state at the moment its enclosing paint pass completed.
//!
//! Lives outside [`paint_adapter`](crate::paint_adapter) because that
//! module is gated behind the `vello` feature (it holds the actual
//! `vello::Scene` fragments). The stats struct carries no GPU
//! references — pure `u64` / `usize` counters + a
//! [`pinion_core::scene::Rect`] damage region — so peer crates that
//! cannot enable `vello` (`pinion-rpc`, `pinion-tui`) can still hold
//! the typed snapshot.
//!
//! ## Lifecycle
//!
//! Published by the surface-side `AppShell::render_window` after each
//! paint cycle into `pinion-shell::ShellCore::fragment_cache_stats_per_window`
//! (via the GUI-agnostic `ShellCore::publish_fragment_cache_stats`
//! method). Consumers:
//!
//! - Unit + integration tests (`paint_adapter::tests` /
//!   `pinion-shell::tests::dispatch_core::r682*`)
//! - The §5.49 R682 demo (`tools/demos/r682_dirty_subtree_cache.py`)
//!   asserts cache-hit-rate / damage-region behaviour through the
//!   RPC surface, not pixel diffing
//! - The `scene/cache_stats` RPC method
//!   (`pinion-rpc::cache_stats`) reads it off
//!   [`pinion-rpc::DispatchContext::fragment_cache_stats`]
//!
//! ## Field semantics
//!
//! Every counter is **cumulative across the cache lifetime**, not
//! per-paint — the per-paint rate is derivable from two
//! consecutive snapshots (`Δhits / Δ(hits + misses)`).
//! `last_damage_region` is the only per-paint field — it's swapped
//! in at the close of each paint pass and reflects only the most
//! recent paint's miss-rect union.

use pinion_core::scene::Rect;

/// GUI-agnostic snapshot of the per-window paint-fragment cache.
///
/// Published every paint cycle from
/// `pinion-runtime::paint_adapter::FragmentCache::stats()`.
///
/// `Copy` + no `vello::Scene` references — consumers in non-vello
/// build profiles (TUI / headless tests / RPC) hold the snapshot
/// without dragging in the GPU stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FragmentCacheStats {
    /// Cumulative cache hits across the cache's lifetime.
    pub hits: u64,
    /// Cumulative cache misses across the cache's lifetime.
    pub misses: u64,
    /// Cumulative paint passes (`FragmentCache::end_paint`
    /// invocations) across the cache's lifetime.
    pub paint_count: u64,
    /// Number of cached fragments currently held — after the
    /// mark-and-sweep eviction at the end of the publishing paint,
    /// this matches the live cacheable Container set painted this
    /// frame.
    pub entries: usize,
    /// Damage region from the most recently completed paint pass:
    /// union of every cache-miss Container's rect. `None` when the
    /// paint was 100% cache-hit (no visual delta from the previous
    /// frame).
    pub last_damage_region: Option<Rect>,
}

impl FragmentCacheStats {
    /// `hits / (hits + misses)`. Returns `0.0` when no lookup has
    /// happened yet (avoids `0/0` `NaN`).
    #[must_use]
    pub fn hit_rate(&self) -> f32 {
        let total = self.hits.saturating_add(self.misses);
        if total == 0 {
            return 0.0;
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "telemetry ratio; numeric pipeline does not consume"
        )]
        {
            self.hits as f32 / total as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r682_zero_lookups_yield_zero_hit_rate() {
        let stats = FragmentCacheStats::default();
        assert!((stats.hit_rate() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn r682_hit_rate_matches_ratio() {
        let stats = FragmentCacheStats {
            hits: 240,
            misses: 12,
            ..Default::default()
        };
        let expected = 240.0_f32 / 252.0_f32;
        assert!((stats.hit_rate() - expected).abs() < 1e-6);
    }

    #[test]
    fn r682_default_is_zero_filled_with_no_damage() {
        let stats = FragmentCacheStats::default();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.paint_count, 0);
        assert_eq!(stats.entries, 0);
        assert!(stats.last_damage_region.is_none());
    }

    #[test]
    fn r682_copy_semantics_preserve_origin() {
        let original = FragmentCacheStats {
            hits: 5,
            misses: 1,
            paint_count: 1,
            entries: 4,
            last_damage_region: Some(Rect::new(0, 0, 100, 100)),
        };
        let copied = original;
        // Origin readable after copy — value semantics held.
        assert_eq!(original.hits, copied.hits);
        assert_eq!(original.misses, copied.misses);
    }
}
