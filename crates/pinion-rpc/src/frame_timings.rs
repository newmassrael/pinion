//! `scene/frame_timings` RPC method dispatch — R907 §5.16 + §5.7.
//!
//! Exposes
//! [`pinion_runtime::FrameTimingsSnapshot`](FrameTimingsSnapshot) — the
//! per-window frame-timing profiler projection the §5.16
//! [`pinion_runtime::frame_timing`] substrate produces — over the
//! JSON-RPC surface, so an AI agent can answer *"how long are frames
//! taking, and where is the time going?"* without scraping pixels or
//! guessing. This is the "measure" half of the §1 northern-star's
//! measure-first pro-tool-performance axis: a frame-budget tuning
//! round can only be evidence-based if the frame cost is readable
//! first.
//!
//! Sibling of [`scene/cache_stats`](crate::cache_stats): that method
//! reports paint-fragment cache hit-rate; this one reports the
//! wall-clock cost of the build / encode / render phases the cache
//! lives inside. They share the per-window-telemetry topology
//! (embedder pre-resolves a `Copy` snapshot onto the dispatch context;
//! the handler just projects it to the wire shape) but stay separate
//! methods — the [`scene/cache_stats`](crate::cache_stats) module's
//! "one observability axis per method" rule, so a client pays only for
//! the axis it consults.
//!
//! ## Wire shape
//!
//! Request:
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/frame_timings", "params": {}, "id": 1 }
//! ```
//!
//! Response:
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "frame_count": 142,
//!     "window_len": 120,
//!     "last": {
//!       "build_us": 320, "encode_us": 110, "render_us": 80,
//!       "total_us": 540, "other_us": 30
//!     },
//!     "window": {
//!       "min_total_us": 480, "mean_total_us": 533, "max_total_us": 980,
//!       "mean_build_us": 310, "mean_encode_us": 105, "mean_render_us": 78
//!     },
//!     "mean_fps": 1876.0
//!   }
//! }
//! ```
//!
//! - `frame_count` is cumulative across the window's whole lifetime;
//!   `window_len` is the rolling-window size the `window` aggregates
//!   fold over (capped at [`pinion_runtime::FRAME_TIMING_WINDOW`]).
//! - `last.other_us = total_us - (build + encode + render)` — the
//!   post-paint overhead (accessibility / IME / finalize) not in a
//!   named phase. `total_us >= build + encode + render` always
//!   (disjoint sub-intervals), so a client can assert the partition.
//! - `mean_fps = 1e6 / window.mean_total_us` (`0.0` for a zero mean);
//!   echoed so a client need not re-derive the rate.
//! - `render_us` is **CPU-side GPU-submit cost**, not GPU execution
//!   wall-clock (queue submission returns before the GPU finishes);
//!   true GPU timing needs timestamp queries (a deferred axis).
//!
//! ## Multi-window scope
//!
//! Pre-resolved by the embedder, exactly like
//! [`scene/cache_stats`](crate::cache_stats):
//! `pinion-shell::ShellCore::window_scoped_rpc_reads` looks up the
//! per-window [`FrameTimingsSnapshot`] via
//! `ShellCore::frame_timings_for_window` and installs it on
//! [`DispatchContext::frame_timings`] before dispatch. The handler
//! here just reads the slot.
//!
//! ## Side-effect contract
//!
//! Read-only. The snapshot is a `Copy` projection — consulting it
//! neither extends a borrow on the runtime accumulator nor schedules a
//! repaint.

use pinion_runtime::FrameTimingsSnapshot;
use serde::Serialize;

/// Typed errors the [`frame_timings`] dispatcher can return. Maps onto
/// a JSON-RPC `-32602 Invalid params` with the variant name in
/// `error.data` so AI agents pattern-match without parsing prose.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameTimingsError {
    /// The embedder did not install a per-window
    /// [`FrameTimingsSnapshot`] — the window has not painted yet
    /// (bootstrap frame, no samples recorded) or the embedder opted
    /// out of frame-timing observability (headless fixture). Distinct
    /// from an all-zero snapshot the way `scene/cache_stats`
    /// distinguishes "no data yet" from "all zeros".
    FrameTimingsUnavailable,
}

/// The most-recent frame's phase breakdown, in microseconds. Mirrors
/// [`pinion_runtime::FrameTiming`] plus the derived `other_us`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FrameTimingsLast {
    /// `view` fn + layout pass.
    pub build_us: u64,
    /// Structured-scene → `vello` fragment encode.
    pub encode_us: u64,
    /// GPU command-buffer record + submit (CPU-side only).
    pub render_us: u64,
    /// Whole productive frame.
    pub total_us: u64,
    /// `total - (build + encode + render)`: post-paint overhead.
    pub other_us: u64,
}

/// Rolling-window aggregates, in microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FrameTimingsWindow {
    /// Smallest `total_us` in the window.
    pub min_total_us: u64,
    /// Arithmetic mean `total_us` over the window.
    pub mean_total_us: u64,
    /// Largest `total_us` in the window.
    pub max_total_us: u64,
    /// Mean build-phase µs over the window.
    pub mean_build_us: u64,
    /// Mean encode-phase µs over the window.
    pub mean_encode_us: u64,
    /// Mean render-phase µs over the window.
    pub mean_render_us: u64,
}

/// Snapshot returned by [`frame_timings`]. Projects
/// [`FrameTimingsSnapshot`] onto the nested wire shape (last frame +
/// window aggregates + cumulative count).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FrameTimingsOutcome {
    /// Frames recorded across the window's whole lifetime.
    pub frame_count: u64,
    /// Samples in the rolling window the aggregates fold over.
    pub window_len: u32,
    /// The most recently recorded frame's phase breakdown.
    pub last: FrameTimingsLast,
    /// Rolling-window min/mean/max + per-phase means.
    pub window: FrameTimingsWindow,
    /// `1e6 / window.mean_total_us`, `0.0` for a zero mean.
    pub mean_fps: f32,
}

/// Project a per-window [`FrameTimingsSnapshot`] onto the wire-shaped
/// [`FrameTimingsOutcome`].
///
/// # Errors
///
/// - [`FrameTimingsError::FrameTimingsUnavailable`] — the embedder did
///   not register a snapshot on [`DispatchContext::frame_timings`].
pub fn frame_timings(
    snapshot: Option<FrameTimingsSnapshot>,
) -> Result<FrameTimingsOutcome, FrameTimingsError> {
    let Some(s) = snapshot else {
        return Err(FrameTimingsError::FrameTimingsUnavailable);
    };
    Ok(FrameTimingsOutcome {
        frame_count: s.frame_count,
        window_len: s.window_len,
        last: FrameTimingsLast {
            build_us: s.last.build_us,
            encode_us: s.last.encode_us,
            render_us: s.last.render_us,
            total_us: s.last.total_us,
            other_us: s.last.other_us(),
        },
        window: FrameTimingsWindow {
            min_total_us: s.min_total_us,
            mean_total_us: s.mean_total_us,
            max_total_us: s.max_total_us,
            mean_build_us: s.mean_build_us,
            mean_encode_us: s.mean_encode_us,
            mean_render_us: s.mean_render_us,
        },
        mean_fps: s.mean_fps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_runtime::{FrameTiming, FrameTimingStats};

    fn snapshot_of(samples: &[FrameTiming]) -> FrameTimingsSnapshot {
        let mut stats = FrameTimingStats::new();
        for s in samples {
            stats.record(*s);
        }
        stats.snapshot().expect("non-empty window yields a snapshot")
    }

    #[test]
    fn r907_missing_snapshot_errors() {
        let err = frame_timings(None).unwrap_err();
        assert_eq!(err, FrameTimingsError::FrameTimingsUnavailable);
    }

    #[test]
    fn r907_single_frame_projects_field_for_field() {
        let snap = snapshot_of(&[FrameTiming::new(300, 100, 80, 540)]);
        let out = frame_timings(Some(snap)).unwrap();
        assert_eq!(out.frame_count, 1);
        assert_eq!(out.window_len, 1);
        assert_eq!(out.last.build_us, 300);
        assert_eq!(out.last.encode_us, 100);
        assert_eq!(out.last.render_us, 80);
        assert_eq!(out.last.total_us, 540);
        assert_eq!(out.last.other_us, 60);
        assert_eq!(out.window.min_total_us, 540);
        assert_eq!(out.window.mean_total_us, 540);
        assert_eq!(out.window.max_total_us, 540);
    }

    #[test]
    fn r907_phase_partition_holds_on_wire() {
        let snap = snapshot_of(&[FrameTiming::new(200, 90, 60, 400)]);
        let out = frame_timings(Some(snap)).unwrap();
        // total == build + encode + render + other, by construction.
        assert_eq!(
            out.last.total_us,
            out.last.build_us + out.last.encode_us + out.last.render_us + out.last.other_us,
        );
    }

    #[test]
    fn r907_window_ordering_invariant() {
        let snap = snapshot_of(&[
            FrameTiming::new(200, 100, 50, 400),
            FrameTiming::new(300, 150, 70, 600),
            FrameTiming::new(500, 200, 120, 980),
        ]);
        let out = frame_timings(Some(snap)).unwrap();
        assert!(out.window.min_total_us <= out.window.mean_total_us);
        assert!(out.window.mean_total_us <= out.window.max_total_us);
        assert_eq!(out.window.min_total_us, 400);
        assert_eq!(out.window.max_total_us, 980);
    }

    #[test]
    fn r907_serialized_nests_last_and_window() {
        let snap = snapshot_of(&[FrameTiming::new(300, 100, 80, 540)]);
        let out = frame_timings(Some(snap)).unwrap();
        let json = serde_json::to_value(out).unwrap();
        assert_eq!(
            json.get("last")
                .and_then(|l| l.get("build_us"))
                .and_then(serde_json::Value::as_u64),
            Some(300),
        );
        assert_eq!(
            json.get("window")
                .and_then(|w| w.get("max_total_us"))
                .and_then(serde_json::Value::as_u64),
            Some(540),
        );
        assert!(json.get("frame_count").is_some());
        assert!(json.get("mean_fps").is_some());
    }

    #[test]
    fn r907_mean_fps_inverts_mean_total_on_wire() {
        let snap = snapshot_of(&[FrameTiming::new(10_000, 4_000, 2_000, 16_666)]);
        let out = frame_timings(Some(snap)).unwrap();
        #[allow(clippy::cast_precision_loss, reason = "test re-derivation")]
        let rederived = 1_000_000.0_f32 / out.window.mean_total_us as f32;
        assert!((out.mean_fps - rederived).abs() < 1e-3);
    }
}
