//! `scene/text_cache_stats` RPC method dispatch — R1521 §5.36 + §5.7.
//!
//! Exposes [`pinion_core::text_cache_stats::TextCacheStats`] — the §5.36
//! shape cache's cost and capacity — over the JSON-RPC surface, so an AI agent
//! can tell a frame that is slow *because the shaper is thrashing* from a frame
//! that is slow for any of the other reasons a frame is slow.
//!
//! ## Why a dedicated method
//!
//! [`scene/cache_stats`](mod@crate::cache_stats) reports the §5.16 **paint
//! fragment** cache. The two caches are not layers of one thing: the fragment
//! cache keys on painted subtrees, the shape cache on `(text, style, width)`,
//! and their verdicts routinely disagree in the direction that matters. A
//! scrolled list re-encodes fragments while every label hits the shaper's
//! cache; a data grid whose rows changed hits neither. Most sharply, the defect
//! this method exists to expose — a working set past the shape cache's capacity
//! — leaves `scene/cache_stats` reporting a perfectly healthy fragment cache
//! while the shaper re-runs parley on every string, every frame.
//!
//! So it is a separate observability axis, and it follows the pattern
//! `scene/cache_stats` itself states: one method per axis
//! ([`scene/animation_state`](fn@crate::animation_state) /
//! [`scene/scroll_state`](fn@crate::scroll_state) /
//! [`scene/caret_state`](fn@crate::caret_state)), so a caller pays only for
//! what it consults and no existing wire shape has to change.
//!
//! ## Scope: per-shell, not per-window
//!
//! Deliberately unlike [`scene/cache_stats`](mod@crate::cache_stats), which is
//! per-window. There is one `LayoutCache` per shell and every window shapes
//! through it — that is the point of it, since two windows showing the same
//! labels should shape them once. A per-window projection of a shared cache
//! would be a fiction, so this method takes no `window` param and answers for
//! the process.
//!
//! ## Wire shape
//!
//! Request:
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "method": "scene/text_cache_stats",
//!   "params": {},
//!   "id": 1
//! }
//! ```
//!
//! Response:
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "shapes": 1200,
//!     "run_builds": 1200,
//!     "entries": 1200,
//!     "capacity": 2048,
//!     "max_capacity": 8192,
//!     "growths": 3,
//!     "font_scans": 1,
//!     "at_ceiling": false
//!   }
//! }
//! ```
//!
//! `at_ceiling` is the framework-computed
//! [`TextCacheStats::at_ceiling`] predicate, echoed beside the raw
//! numbers for the same reason `scene/cache_stats` echoes `hit_rate`: the rule
//! for reading two counters together belongs to the framework, not to every
//! caller that reimplements it.
//!
//! ## Side-effect contract
//!
//! Read-only. The snapshot is `Copy`, so consulting it neither borrows the
//! cache nor shapes anything — an agent may poll it between frames without
//! perturbing the measurement it is taking.

use pinion_core::text_cache_stats::TextCacheStats;
use serde::Serialize;

/// Typed errors the [`text_cache_stats`] dispatcher can return. The variant
/// name rides in `error.data` so an agent pattern-matches rather than parsing
/// prose.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextCacheStatsError {
    /// The embedder installed no [`TextCacheStats`] snapshot on the dispatch
    /// context.
    ///
    /// Production embedders publish one on every dispatch. The variant exists
    /// for headless fixtures that hold no shape cache at all — a `pinion-tui`
    /// host, for instance, which never shapes by construction.
    TextCacheStatsUnavailable,
}

/// Snapshot returned by [`text_cache_stats`]. Mirrors [`TextCacheStats`]
/// field-for-field plus the framework-computed
/// [`TextCacheStats::at_ceiling`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TextCacheStatsOutcome {
    /// Cumulative shaper runs (cache misses) over the cache's lifetime.
    pub shapes: u64,
    /// R1531 — cumulative derivations of a shaped layout's positioned-glyph
    /// draw list.
    ///
    /// The counter that distinguishes a frame which **replayed** its text from
    /// one which rebuilt an identical draw list — indistinguishable in pixels,
    /// 2.9x apart in cost, and invisible to every other field here. `shapes`
    /// cannot answer it: a frame that re-encodes without re-shaping (a
    /// keystroke, a scroll — the §5.16 fragment cache missing while the shape
    /// cache hits) leaves `shapes` still by construction.
    pub run_builds: u64,
    /// Cached layouts held right now.
    pub entries: u64,
    /// Current capacity in entries.
    pub capacity: u64,
    /// The ceiling `capacity` will not pass.
    pub max_capacity: u64,
    /// How many times the capacity has doubled.
    pub growths: u32,
    /// How many times the platform font database was enumerated (R1447
    /// invariant: at most once).
    pub font_scans: u32,
    /// `capacity >= max_capacity` — the cache cannot grow further.
    pub at_ceiling: bool,
}

/// Project a [`TextCacheStats`] snapshot onto the wire-shaped
/// [`TextCacheStatsOutcome`].
///
/// # Errors
///
/// - [`TextCacheStatsError::TextCacheStatsUnavailable`] — the embedder
///   registered no snapshot on the dispatch context.
pub fn text_cache_stats(
    stats: Option<TextCacheStats>,
) -> Result<TextCacheStatsOutcome, TextCacheStatsError> {
    let Some(stats) = stats else {
        return Err(TextCacheStatsError::TextCacheStatsUnavailable);
    };
    Ok(TextCacheStatsOutcome {
        shapes: stats.shapes,
        run_builds: stats.run_builds,
        entries: stats.entries,
        capacity: stats.capacity,
        max_capacity: stats.max_capacity,
        growths: stats.growths,
        font_scans: stats.font_scans,
        at_ceiling: stats.at_ceiling(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r1521_missing_snapshot_errors() {
        let err = text_cache_stats(None).unwrap_err();
        assert_eq!(err, TextCacheStatsError::TextCacheStatsUnavailable);
    }

    #[test]
    fn r1521_counters_mirror_input() {
        let stats = TextCacheStats {
            shapes: 1200,
            run_builds: 900,
            entries: 1200,
            capacity: 2048,
            max_capacity: 8192,
            growths: 3,
            font_scans: 1,
        };
        let out = text_cache_stats(Some(stats)).unwrap();
        assert_eq!(out.shapes, 1200);
        assert_eq!(
            out.run_builds, 900,
            "the two counters are independent — 300 of those layouts were \
             shaped to be measured and never painted",
        );
        assert_eq!(out.entries, 1200);
        assert_eq!(out.capacity, 2048);
        assert_eq!(out.max_capacity, 8192);
        assert_eq!(out.growths, 3);
        assert_eq!(out.font_scans, 1);
        assert!(!out.at_ceiling, "2048 of 8192 has room to grow");
    }

    /// R1521 — `at_ceiling` is computed by the framework, not echoed from the
    /// input.
    ///
    /// The discriminating case: a snapshot whose capacity reached its ceiling
    /// must report `true` even though nothing in the input says so. A wire
    /// that omitted this would make every caller re-derive the comparison, and
    /// the comparison is the whole rule for telling "cannot grow further" from
    /// "has not needed to".
    #[test]
    fn r1521_at_ceiling_is_derived_not_carried() {
        let stats = TextCacheStats {
            shapes: 9000,
            run_builds: 9000,
            entries: 8192,
            capacity: 8192,
            max_capacity: 8192,
            growths: 5,
            font_scans: 1,
        };
        let out = text_cache_stats(Some(stats)).unwrap();
        assert!(
            out.at_ceiling,
            "a cache at its ceiling says so, so an agent seeing `shapes` climb \
             knows waiting will not fix it",
        );
    }
}
