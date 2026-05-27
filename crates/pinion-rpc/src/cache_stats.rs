//! `scene/cache_stats` RPC method dispatch — R682.B §5.16 + §5.7.
//!
//! Exposes
//! [`pinion_runtime::FragmentCacheStats`](FragmentCacheStats)
//! (the per-window paint-fragment cache observability snapshot R682.A
//! landed; the type lives in
//! [`pinion_runtime::paint_cache_stats`] — a non-vello-gated
//! submodule — so this peer crate can hold a typed slot without
//! enabling the GPU stack) over the JSON-RPC surface so AI agents
//! can verify dirty-cache behaviour, measure cache hit rate over
//! multiple paints, and confirm damage-region propagation without
//! scraping pixels.
//!
//! ## Why a dedicated method
//!
//! [`scene/snapshot`](crate::snapshot) returns the structural scene
//! tree (one JSON object — the root node body). Tacking
//! `cache_stats` on as a sidecar would either break the existing
//! single-node wire shape every demo + test consumes today, or
//! force every snapshot caller to pay the cost of stats it does
//! not need. Splitting observability axes into separate methods
//! (the pattern already established by
//! [`scene/animation_state`](crate::animation_state) /
//! [`scene/scroll_state`](crate::scroll_state) /
//! [`scene/caret_state`](crate::caret_state)) keeps each surface
//! focused on a single concern and lets the AI agent pay only for
//! what it consults.
//!
//! ## Wire shape
//!
//! Request:
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "method": "scene/cache_stats",
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
//!     "hits": 240,
//!     "misses": 12,
//!     "paint_count": 5,
//!     "entries": 12,
//!     "hit_rate": 0.952,
//!     "last_damage_region": { "x": 0.0, "y": 0.0, "w": 320.0, "h": 200.0 }
//!   }
//! }
//! ```
//!
//! `last_damage_region` is `null` when the most recent paint pass
//! was 100% cache-hit (no visual delta from the previous frame).
//! `hit_rate` is the framework-computed
//! [`FragmentCacheStats::hit_rate`] (returns `0.0` when no lookup
//! has happened yet, avoiding `0/0` NaN); echoed alongside the raw
//! counters so callers can assert convergence without re-implementing
//! the ratio rule.
//!
//! ## Multi-window scope
//!
//! Pre-resolved by the embedder. `pinion-shell::ShellCore`'s
//! `dispatch_rpc_for_window` reads the `{window: "<id>"}` param
//! (or defaults to the primary spec), looks up the per-window
//! [`FragmentCacheStats`] via
//! `ShellCore::fragment_cache_stats_for_window`, and installs the
//! result on [`DispatchContext::fragment_cache_stats`] before
//! invoking the dispatcher. The `cache_stats` dispatcher itself just
//! reads the slot — no window param parsing here.
//!
//! ## Side-effect contract
//!
//! Read-only. The `FragmentCacheStats` value is a `Copy` snapshot —
//! consulting it neither extends a borrow on the runtime cache nor
//! schedules a re-paint.

use pinion_runtime::FragmentCacheStats;
use serde::Serialize;

/// Typed errors the [`cache_stats`] dispatcher can return. Every
/// variant maps onto a JSON-RPC `-32602 Invalid params` response at
/// the dispatch layer with the variant name in `error.data` so AI
/// agents can pattern-match without parsing prose.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum CacheStatsError {
    /// The embedder did not install a per-window
    /// [`FragmentCacheStats`] snapshot on the dispatch context.
    /// Production embedders publish the snapshot on every paint
    /// cycle ([`pinion-shell::AppShell::render_window`] threads the
    /// per-window value into the context); the error variant exists
    /// for the bootstrap window (no paint yet) and for headless test
    /// fixtures that opt out of cache observability.
    CacheStatsUnavailable,
}

/// Damage region from the most recently completed paint pass,
/// serialized as `{x, y, w, h}` matching the
/// [`pinion_core::scene::Rect`] integer-pixel field order. `null`
/// (skipped via `skip_serializing_if`) when the paint was 100%
/// cache-hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CacheStatsRect {
    /// Left edge in pixels (mirrors `Rect.x`).
    pub x: u32,
    /// Top edge in pixels.
    pub y: u32,
    /// Width in pixels.
    pub w: u32,
    /// Height in pixels.
    pub h: u32,
}

/// Snapshot returned by [`cache_stats`]. Mirrors
/// [`FragmentCacheStats`] field-for-field plus the framework-computed
/// [`FragmentCacheStats::hit_rate`] so callers do not re-implement
/// the ratio rule.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CacheStatsOutcome {
    /// Cumulative cache hits across the per-window cache lifetime.
    pub hits: u64,
    /// Cumulative cache misses across the per-window cache lifetime.
    pub misses: u64,
    /// Cumulative paint passes (== `FragmentCache::end_paint`
    /// invocations).
    pub paint_count: u64,
    /// Number of cached fragments currently held after the latest
    /// mark-and-sweep eviction.
    pub entries: u64,
    /// `hits / (hits + misses)`, `0.0` when no lookup has happened.
    pub hit_rate: f32,
    /// Union of cache-miss Container rects from the most recent
    /// paint pass. `None` when the paint was 100% cache-hit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_damage_region: Option<CacheStatsRect>,
}

/// Project a per-window [`FragmentCacheStats`] snapshot onto the
/// wire-shaped [`CacheStatsOutcome`].
///
/// `entries` widens to `u64` so the wire shape is uniform across
/// every counter (JSON has no distinct integer width; native `usize`
/// would platform-dependently serialize).
///
/// # Errors
///
/// - [`CacheStatsError::CacheStatsUnavailable`] — the embedder did
///   not register a snapshot on [`DispatchContext::fragment_cache_stats`].
pub fn cache_stats(
    stats: Option<FragmentCacheStats>,
) -> Result<CacheStatsOutcome, CacheStatsError> {
    let Some(stats) = stats else {
        return Err(CacheStatsError::CacheStatsUnavailable);
    };
    let entries_u64 = u64::try_from(stats.entries).unwrap_or(u64::MAX);
    Ok(CacheStatsOutcome {
        hits: stats.hits,
        misses: stats.misses,
        paint_count: stats.paint_count,
        entries: entries_u64,
        hit_rate: stats.hit_rate(),
        last_damage_region: stats.last_damage_region.map(|r| CacheStatsRect {
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::Rect;

    #[test]
    fn r682b_missing_snapshot_errors() {
        let err = cache_stats(None).unwrap_err();
        assert_eq!(err, CacheStatsError::CacheStatsUnavailable);
    }

    #[test]
    fn r682b_zero_stats_yield_zero_hit_rate() {
        let stats = FragmentCacheStats::default();
        let outcome = cache_stats(Some(stats)).unwrap();
        assert_eq!(outcome.hits, 0);
        assert_eq!(outcome.misses, 0);
        assert_eq!(outcome.paint_count, 0);
        assert_eq!(outcome.entries, 0);
        assert!((outcome.hit_rate - 0.0).abs() < f32::EPSILON);
        assert!(outcome.last_damage_region.is_none());
    }

    #[test]
    fn r682b_counters_mirror_input() {
        let stats = FragmentCacheStats {
            hits: 240,
            misses: 12,
            paint_count: 5,
            entries: 12,
            last_damage_region: None,
        };
        let outcome = cache_stats(Some(stats)).unwrap();
        assert_eq!(outcome.hits, 240);
        assert_eq!(outcome.misses, 12);
        assert_eq!(outcome.paint_count, 5);
        assert_eq!(outcome.entries, 12);
        let expected = 240.0_f32 / 252.0_f32;
        assert!((outcome.hit_rate - expected).abs() < 1e-6);
    }

    #[test]
    fn r682b_damage_region_round_trip() {
        let rect = Rect::new(8, 16, 320, 200);
        let stats = FragmentCacheStats {
            hits: 1,
            misses: 1,
            paint_count: 1,
            entries: 1,
            last_damage_region: Some(rect),
        };
        let outcome = cache_stats(Some(stats)).unwrap();
        let dmg = outcome.last_damage_region.expect("damage carried");
        assert_eq!(dmg.x, 8);
        assert_eq!(dmg.y, 16);
        assert_eq!(dmg.w, 320);
        assert_eq!(dmg.h, 200);
    }

    #[test]
    fn r682b_serialized_omits_none_damage_region() {
        let outcome = cache_stats(Some(FragmentCacheStats::default())).unwrap();
        let json = serde_json::to_value(outcome).unwrap();
        assert!(json.get("last_damage_region").is_none());
        assert_eq!(json.get("hits").and_then(serde_json::Value::as_u64), Some(0));
    }

    #[test]
    fn r682b_serialized_emits_some_damage_region() {
        let outcome = cache_stats(Some(FragmentCacheStats {
            hits: 0,
            misses: 1,
            paint_count: 1,
            entries: 1,
            last_damage_region: Some(Rect::new(0, 0, 320, 200)),
        }))
        .unwrap();
        let json = serde_json::to_value(outcome).unwrap();
        let dmg = json.get("last_damage_region").expect("damage rect emitted");
        assert_eq!(dmg.get("w").and_then(serde_json::Value::as_u64), Some(320));
    }

    #[test]
    fn r682b_huge_entries_saturate_to_u64_max() {
        let stats = FragmentCacheStats {
            entries: usize::MAX,
            ..Default::default()
        };
        let outcome = cache_stats(Some(stats)).unwrap();
        // usize::MAX → u64 on 64-bit platforms is identity; on
        // smaller platforms try_from succeeds. Either way the field
        // is u64-max within platform precision (no panic, no
        // truncation surprise).
        assert!(outcome.entries >= u64::from(u32::MAX));
    }
}
