//! R907 §5.16 §5.7 — per-window frame-timing profiler substrate.
//!
//! The sibling of [`paint_cache_stats`](crate::paint_cache_stats):
//! that module answers *"how much of the last frame was a cache
//! hit?"*; this one answers *"how long did the last frame take, and
//! where did the time go?"* — the measurement that "measure first"
//! optimization (the §1 northern-star's pro-tool-performance axis)
//! requires before any frame-budget tuning is anything but a guess.
//!
//! Like [`FragmentCacheStats`](crate::FragmentCacheStats), every type
//! here is GUI-agnostic — pure `u64` microsecond counters with no
//! `vello::Scene` / wall-clock references — so the non-vello peer
//! crates (`pinion-rpc`, `pinion-tui`) hold the snapshot without
//! dragging in the GPU stack. The wall-clock *measurement* lives in
//! the surface (`pinion_shell::AppShell::render_window`, which brackets
//! the three paint phases with [`std::time::Instant`] spans); this
//! module owns only the typed sample, the rolling accumulator, and the
//! aggregate projection. Splitting measurement (surface) from
//! aggregation (this substrate) keeps the substrate unit-testable with
//! *injected* deterministic samples — wall-clock numbers never enter a
//! test.
//!
//! ## The three phases
//!
//! Each painted frame is bracketed into the canonical desktop-app
//! frame breakdown (cf. Unreal `stat unit` Game/Draw/GPU; Chrome
//! `DevTools` Scripting/Rendering/Painting):
//!
//! - **build** — `ShellCore::compute_paint_scene_for_window`: the
//!   `view` fn run plus the §5.36 layout pass. "Is my scene
//!   construction the bottleneck?"
//! - **encode** — `paint_adapter::to_vello_cached`: walking the
//!   structured [`Scene`](pinion_core::Scene) tree into `vello`
//!   fragments (the §5.16 fragment cache short-circuits unchanged
//!   subtrees here, so this phase and the cache hit-rate move
//!   together).
//! - **render** — `WidgetRenderer::render`: recording and submitting
//!   the GPU command buffer. **CPU-side cost only** — `wgpu` queue
//!   submission returns before the GPU finishes the work, so this is
//!   *not* GPU execution wall-clock. True GPU timing needs timestamp
//!   queries (a deferred axis); honest naming keeps a future round
//!   from mistaking this for GPU time.
//!
//! [`FrameTiming::total_us`] spans the whole productive frame (build
//! start through the post-paint accessibility-emit / IME-publish /
//! finalize work), so `total - (build + encode + render)` is a real
//! "other / overhead" bucket — and `total >= build + encode + render`
//! holds **by construction**: the three phases are disjoint
//! sub-intervals of the total interval, and microsecond truncation
//! preserves the inequality (`Σ⌊subᵢ⌋ <= ⌊Σsubᵢ⌋ <= ⌊total⌋`). That
//! invariant is deterministic even though every individual value is
//! wall-clock, which is exactly what lets a demo assert correctness
//! without asserting timing.

use std::collections::VecDeque;

/// Number of most-recent frames the rolling window retains. ~2s at
/// 60fps — long enough to smooth single-frame jitter into a stable
/// mean/min/max, short enough that the window tracks a workload change
/// (a heavy scroll, a window resize) within a couple of seconds rather
/// than averaging it away. Matches the order of magnitude every
/// in-app profiler HUD uses (Tracy / Chrome frame history ≈ 120–300
/// frames).
pub const FRAME_TIMING_WINDOW: usize = 120;

/// One painted frame's phase breakdown, in microseconds. `Copy` +
/// no wall-clock references — the surface measures with
/// [`std::time::Instant`] and lowers to this GUI-agnostic sample
/// before handing it to [`FrameTimingStats::record`].
///
/// Microseconds (not [`std::time::Duration`]) because the wire surface
/// is JSON — an integer µs count serializes uniformly across the
/// `u64` counters, and the 100µs–20ms range of real frame phases keeps
/// 4–5 significant digits without a fractional field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameTiming {
    /// `view` fn + §5.36 layout pass
    /// (`compute_paint_scene_for_window`).
    pub build_us: u64,
    /// Structured-scene → `vello` fragment encode
    /// (`to_vello_cached`); the §5.16 fragment cache short-circuits
    /// here.
    pub encode_us: u64,
    /// GPU command-buffer record + submit (`WidgetRenderer::render`).
    /// CPU-side cost only — not GPU execution wall-clock.
    pub render_us: u64,
    /// Whole productive frame: build start through the post-paint
    /// accessibility / IME / finalize work. `>= build + encode +
    /// render` by construction.
    pub total_us: u64,
}

impl FrameTiming {
    /// Construct a sample from the four measured phase durations.
    #[must_use]
    pub fn new(build_us: u64, encode_us: u64, render_us: u64, total_us: u64) -> Self {
        Self {
            build_us,
            encode_us,
            render_us,
            total_us,
        }
    }

    /// `build + encode + render` (saturating). The measured part of
    /// the frame; [`Self::other_us`] is the unmeasured remainder.
    #[must_use]
    pub fn phase_sum_us(self) -> u64 {
        self.build_us
            .saturating_add(self.encode_us)
            .saturating_add(self.render_us)
    }

    /// `total - (build + encode + render)`: the post-paint overhead
    /// (accessibility emit, IME publish, cache-stats publish, frame
    /// finalize) not captured by a named phase. Saturating, so the
    /// by-construction `total >= phase_sum` invariant never
    /// underflows even if a future caller violates it.
    #[must_use]
    pub fn other_us(self) -> u64 {
        self.total_us.saturating_sub(self.phase_sum_us())
    }
}

/// Rolling per-window frame-timing accumulator: a fixed-capacity ring
/// of the last [`FRAME_TIMING_WINDOW`] [`FrameTiming`] samples plus a
/// lifetime frame counter.
///
/// Not `Copy` (it owns the ring), so — unlike
/// [`FragmentCacheStats`](crate::FragmentCacheStats), whose `Copy`
/// snapshot is published every paint — this accumulator stays on the
/// `ShellCore` SSOT and the `Copy` [`FrameTimingsSnapshot`] is
/// *projected at the AI-paced RPC read*, not mirrored every frame
/// (the R890 "store the source, project on read" rule: the O(window)
/// fold is paid only when an AI client actually consults
/// `scene/frame_timings`, never on the 60–144fps paint path).
#[derive(Debug, Clone, Default)]
pub struct FrameTimingStats {
    /// Most-recent samples, oldest at the front. Capped at
    /// [`FRAME_TIMING_WINDOW`] by [`Self::record`].
    samples: VecDeque<FrameTiming>,
    /// Frames recorded across the window's whole lifetime — keeps
    /// counting after the ring starts evicting (the cumulative
    /// peer of [`FragmentCacheStats::paint_count`]).
    frame_count: u64,
}

impl FrameTimingStats {
    /// A fresh accumulator with an empty ring and a zero count.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one painted frame: push it onto the ring (evicting the
    /// oldest once the window is full) and bump the lifetime counter.
    pub fn record(&mut self, timing: FrameTiming) {
        if self.samples.len() == FRAME_TIMING_WINDOW {
            self.samples.pop_front();
        }
        self.samples.push_back(timing);
        self.frame_count = self.frame_count.saturating_add(1);
    }

    /// Frames recorded across this window's whole lifetime (not the
    /// ring length — this keeps growing after eviction starts).
    #[must_use]
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Number of samples currently in the rolling window
    /// (`<= FRAME_TIMING_WINDOW`).
    #[must_use]
    pub fn window_len(&self) -> usize {
        self.samples.len()
    }

    /// `true` until the first [`Self::record`].
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Fold the rolling window into a `Copy` [`FrameTimingsSnapshot`].
    ///
    /// `None` before the first frame (no samples to aggregate) — the
    /// bootstrap state a never-painted window reports, mapped to
    /// `FrameTimingsUnavailable` at the RPC layer (distinct from an
    /// all-zero snapshot the way `scene/cache_stats` distinguishes
    /// "no data yet" from "all zeros").
    #[must_use]
    pub fn snapshot(&self) -> Option<FrameTimingsSnapshot> {
        let last = *self.samples.back()?;
        let len = self.samples.len() as u64; // >= 1 past the `?`
        let (mut min_total, mut max_total) = (u64::MAX, 0u64);
        let (mut sum_total, mut sum_build, mut sum_encode, mut sum_render) = (0u64, 0u64, 0u64, 0u64);
        for s in &self.samples {
            min_total = min_total.min(s.total_us);
            max_total = max_total.max(s.total_us);
            sum_total = sum_total.saturating_add(s.total_us);
            sum_build = sum_build.saturating_add(s.build_us);
            sum_encode = sum_encode.saturating_add(s.encode_us);
            sum_render = sum_render.saturating_add(s.render_us);
        }
        let mean_total = sum_total / len;
        Some(FrameTimingsSnapshot {
            frame_count: self.frame_count,
            window_len: u32::try_from(self.samples.len()).unwrap_or(u32::MAX),
            last,
            min_total_us: min_total,
            mean_total_us: mean_total,
            max_total_us: max_total,
            mean_build_us: sum_build / len,
            mean_encode_us: sum_encode / len,
            mean_render_us: sum_render / len,
            mean_fps: fps_from_mean_total_us(mean_total),
        })
    }
}

/// `1e6 / mean_total_us` frames per second, `0.0` when the mean is
/// `0` (a degenerate all-instant-frame window — avoids `1e6/0` =
/// `inf`). Derived from the *reported* (truncated) `mean_total_us` so
/// a client can re-derive it: `mean_fps ≈ 1e6 / mean_total_us`.
#[must_use]
fn fps_from_mean_total_us(mean_total_us: u64) -> f32 {
    if mean_total_us == 0 {
        return 0.0;
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "telemetry rate; no numeric pipeline consumes the f32"
    )]
    {
        1_000_000.0_f32 / mean_total_us as f32
    }
}

/// `Copy` projection of a [`FrameTimingStats`] rolling window — the
/// payload `scene/frame_timings` serializes. Carries the last frame's
/// phase breakdown plus the window's total-time min/mean/max and
/// per-phase means, so an AI client gets both "what did the most
/// recent frame cost?" and "what's the steady-state profile?" from one
/// read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameTimingsSnapshot {
    /// Frames recorded across the window's whole lifetime.
    pub frame_count: u64,
    /// Samples in the rolling window the aggregates fold over
    /// (`<= FRAME_TIMING_WINDOW`).
    pub window_len: u32,
    /// The most recently recorded frame's phase breakdown.
    pub last: FrameTiming,
    /// Smallest `total_us` in the window.
    pub min_total_us: u64,
    /// Arithmetic mean `total_us` over the window (`sum / window_len`,
    /// integer-truncated).
    pub mean_total_us: u64,
    /// Largest `total_us` in the window.
    pub max_total_us: u64,
    /// Mean build-phase µs over the window.
    pub mean_build_us: u64,
    /// Mean encode-phase µs over the window.
    pub mean_encode_us: u64,
    /// Mean render-phase µs over the window.
    pub mean_render_us: u64,
    /// `1e6 / mean_total_us`, `0.0` for a zero mean.
    pub mean_fps: f32,
}

#[cfg(test)]
mod tests {
    use super::{FrameTiming, FrameTimingStats, FRAME_TIMING_WINDOW};

    #[test]
    fn r907_empty_window_has_no_snapshot() {
        let stats = FrameTimingStats::new();
        assert!(stats.is_empty());
        assert_eq!(stats.frame_count(), 0);
        assert_eq!(stats.window_len(), 0);
        assert!(stats.snapshot().is_none());
    }

    #[test]
    fn r907_single_frame_aggregates_to_itself() {
        let mut stats = FrameTimingStats::new();
        stats.record(FrameTiming::new(300, 100, 80, 540));
        let snap = stats.snapshot().expect("one sample yields a snapshot");
        assert_eq!(snap.frame_count, 1);
        assert_eq!(snap.window_len, 1);
        assert_eq!(snap.last, FrameTiming::new(300, 100, 80, 540));
        // One sample: min == mean == max == that frame's total.
        assert_eq!(snap.min_total_us, 540);
        assert_eq!(snap.mean_total_us, 540);
        assert_eq!(snap.max_total_us, 540);
        assert_eq!(snap.mean_build_us, 300);
        assert_eq!(snap.mean_encode_us, 100);
        assert_eq!(snap.mean_render_us, 80);
    }

    #[test]
    fn r907_phase_sum_and_other_partition_total() {
        let t = FrameTiming::new(300, 100, 80, 540);
        assert_eq!(t.phase_sum_us(), 480);
        assert_eq!(t.other_us(), 60);
        // total >= phase_sum is the by-construction invariant; other
        // saturates to 0 rather than underflowing if it is violated.
        let degenerate = FrameTiming::new(300, 100, 80, 100);
        assert_eq!(degenerate.other_us(), 0);
    }

    #[test]
    fn r907_min_mean_max_over_window() {
        let mut stats = FrameTimingStats::new();
        // totals: 400, 600, 980 -> min 400, max 980, mean 660.
        stats.record(FrameTiming::new(200, 100, 50, 400));
        stats.record(FrameTiming::new(300, 150, 70, 600));
        stats.record(FrameTiming::new(500, 200, 120, 980));
        let snap = stats.snapshot().unwrap();
        assert_eq!(snap.window_len, 3);
        assert_eq!(snap.frame_count, 3);
        assert_eq!(snap.min_total_us, 400);
        assert_eq!(snap.max_total_us, 980);
        assert_eq!(snap.mean_total_us, (400 + 600 + 980) / 3);
        assert_eq!(snap.mean_build_us, (200 + 300 + 500) / 3);
        assert_eq!(snap.mean_encode_us, (100 + 150 + 200) / 3);
        assert_eq!(snap.mean_render_us, (50 + 70 + 120) / 3);
        // last is the most recent record, not the max.
        assert_eq!(snap.last.total_us, 980);
        // min/mean/max ordering invariant (the demo asserts this too).
        assert!(snap.min_total_us <= snap.mean_total_us);
        assert!(snap.mean_total_us <= snap.max_total_us);
    }

    #[test]
    fn r907_ring_evicts_oldest_but_count_is_cumulative() {
        let mut stats = FrameTimingStats::new();
        // Fill the window with cheap frames, then overflow it with one
        // expensive frame so the cheap ones evict out.
        for _ in 0..FRAME_TIMING_WINDOW {
            stats.record(FrameTiming::new(10, 10, 10, 100));
        }
        assert_eq!(stats.window_len(), FRAME_TIMING_WINDOW);
        assert_eq!(stats.frame_count(), FRAME_TIMING_WINDOW as u64);
        // Push one more: ring stays capped, lifetime count keeps going.
        stats.record(FrameTiming::new(900, 50, 50, 2000));
        assert_eq!(stats.window_len(), FRAME_TIMING_WINDOW);
        assert_eq!(stats.frame_count(), FRAME_TIMING_WINDOW as u64 + 1);
        let snap = stats.snapshot().unwrap();
        // The freshest frame is `last`; the max reflects it; the min is
        // still a retained cheap frame.
        assert_eq!(snap.last.total_us, 2000);
        assert_eq!(snap.max_total_us, 2000);
        assert_eq!(snap.min_total_us, 100);
    }

    #[test]
    fn r907_full_window_evicts_all_old_samples_eventually() {
        let mut stats = FrameTimingStats::new();
        for _ in 0..FRAME_TIMING_WINDOW {
            stats.record(FrameTiming::new(10, 10, 10, 100));
        }
        // Overflow by a whole window of a different value: every
        // original sample must have evicted.
        for _ in 0..FRAME_TIMING_WINDOW {
            stats.record(FrameTiming::new(20, 20, 20, 300));
        }
        let snap = stats.snapshot().unwrap();
        assert_eq!(snap.window_len, u32::try_from(FRAME_TIMING_WINDOW).unwrap());
        assert_eq!(snap.frame_count, 2 * FRAME_TIMING_WINDOW as u64);
        // Window is now uniformly the second value.
        assert_eq!(snap.min_total_us, 300);
        assert_eq!(snap.mean_total_us, 300);
        assert_eq!(snap.max_total_us, 300);
    }

    #[test]
    fn r907_mean_fps_inverts_mean_total() {
        let mut stats = FrameTimingStats::new();
        // mean_total = 16_666 µs -> ~60 fps.
        stats.record(FrameTiming::new(10_000, 4_000, 2_000, 16_666));
        let snap = stats.snapshot().unwrap();
        let expected = 1_000_000.0_f32 / 16_666.0_f32;
        assert!(
            (snap.mean_fps - expected).abs() < 1e-3,
            "mean_fps {} should invert mean_total_us {}",
            snap.mean_fps,
            snap.mean_total_us,
        );
        // The client can re-derive fps from the reported mean.
        #[allow(clippy::cast_precision_loss, reason = "test re-derivation")]
        let rederived = 1_000_000.0_f32 / snap.mean_total_us as f32;
        assert!((snap.mean_fps - rederived).abs() < 1e-3);
    }

    #[test]
    fn r907_zero_total_yields_zero_fps_not_infinity() {
        let mut stats = FrameTimingStats::new();
        stats.record(FrameTiming::new(0, 0, 0, 0));
        let snap = stats.snapshot().unwrap();
        assert_eq!(snap.mean_total_us, 0);
        assert!(
            snap.mean_fps.abs() < f32::EPSILON,
            "zero mean total must not divide to infinity",
        );
    }
}
