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
//! the paint phases with [`std::time::Instant`] spans); this
//! module owns only the typed sample, the rolling accumulator, and the
//! aggregate projection. Splitting measurement (surface) from
//! aggregation (this substrate) keeps the substrate unit-testable with
//! *injected* deterministic samples — wall-clock numbers never enter a
//! test.
//!
//! ## The four phases
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
//! - **acquire** (R1361.1) — `surface.get_current_texture()`: **waiting
//!   for the compositor to release a swapchain image**. Not work at all
//!   — the thread is blocked. Under `PresentMode::AutoVsync` (= Fifo)
//!   this is the vsync pace-setter, so it is normally the *largest*
//!   phase in a paced window and near-zero in an unpaced one.
//!
//!   It is its own phase because it answers a question no other phase
//!   can: **"am I slow, or am I merely waiting?"** Measured on one
//!   `hello-frame-profiler` window, idle machine:
//!
//!   | | total | work | build | encode | acquire | render |
//!   |---|---|---|---|---|---|---|
//!   | unpaced | 1137 | 1107 | 139 | 105 | **9** | 863 |
//!   | `set_fps 60` | 16273 | **696** | 214 | 83 | **15559** | 399 |
//!
//!   The paced frame spends 96% of itself blocked and 0.7ms working.
//!   Both rows are healthy; only the split says so.
//!
//!   What theory pins is `total ≈ one vsync interval` (`16_667µs` at
//!   60Hz) — the acquire is then whatever is *left* of that interval
//!   after the frame's own work: `15559 ≈ 16667 - 696 - other`. So the
//!   acquire shrinks as work grows, and the frame that stops meeting its
//!   target is the one whose acquire reaches 0. One machine, one run:
//!   read the ratio, not the absolutes.
//!
//! - **render** — `WidgetRenderer::render` minus the acquire:
//!   `render_to_texture` plus the blit/present command-buffer record and
//!   submit. **Not GPU execution wall-clock** — `wgpu` queue submission
//!   returns before the GPU finishes the work; true GPU timing needs
//!   timestamp queries (a deferred axis).
//!
//!   R1361 found this phase claiming "CPU-side cost only" while
//!   *containing* the blocking acquire. In the 60fps row above the
//!   pre-R1361.1 `render_us` would have read **`15_958µs`** — a window
//!   doing 0.4ms of drawing reported as 16ms of "render", i.e. exactly
//!   like a GPU-bound one. A text readout hid that for 450 rounds; the
//!   first chart drawn from it made the flat line unmissable. R1361.1
//!   split the acquire out, so `render_us` now means what it claimed.
//!
//! [`FrameTiming::total_us`] spans the whole productive frame (build
//! start through the post-paint accessibility-emit / IME-publish /
//! finalize work), so `total - (build + encode + acquire + render)` is
//! a real "other / overhead" bucket — and `total >= build + encode +
//! acquire + render` holds **by construction**: the four phases are
//! disjoint sub-intervals of the total interval, and microsecond
//! truncation preserves the inequality (`Σ⌊subᵢ⌋ <= ⌊Σsubᵢ⌋ <=
//! ⌊total⌋`). That invariant is deterministic even though every
//! individual value is wall-clock, which is exactly what lets a demo
//! assert correctness without asserting timing.
//!
//! [`FrameTiming::work_us`] is the peer worth reaching for when the
//! question is "how much room do I have?": `total` counts the vsync
//! block, `work` does not.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use pinion_core::ProviderSlot;

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
    /// R1361.1 — blocking wait for a swapchain image
    /// (`surface.get_current_texture()`), **idle time, not work**. The
    /// vsync pace-setter under `PresentMode::AutoVsync`. Large here +
    /// small elsewhere = presentation-bound with headroom to spare. See
    /// the module doc's phase list.
    pub acquire_us: u64,
    /// GPU command-buffer record + submit (`WidgetRenderer::render`
    /// **minus** [`Self::acquire_us`]). CPU-side cost only — `wgpu`
    /// queue submission returns before the GPU finishes, so this is not
    /// GPU execution wall-clock.
    ///
    /// Pre-R1361.1 this silently contained the acquire, which made a
    /// vsync-blocked idle window indistinguishable from a GPU-bound one.
    pub render_us: u64,
    /// Whole productive frame: build start through the post-paint
    /// accessibility / IME / finalize work. `>= build + encode +
    /// acquire + render` by construction.
    pub total_us: u64,
}

impl FrameTiming {
    /// Construct a sample from the measured phase durations.
    #[must_use]
    pub fn new(
        build_us: u64,
        encode_us: u64,
        acquire_us: u64,
        render_us: u64,
        total_us: u64,
    ) -> Self {
        Self {
            build_us,
            encode_us,
            acquire_us,
            render_us,
            total_us,
        }
    }

    /// `build + encode + acquire + render` (saturating). The accounted
    /// part of the frame; [`Self::other_us`] is the remainder.
    #[must_use]
    pub fn phase_sum_us(self) -> u64 {
        self.build_us
            .saturating_add(self.encode_us)
            .saturating_add(self.acquire_us)
            .saturating_add(self.render_us)
    }

    /// `build + encode + render` — the frame's real **work**, excluding
    /// the [`Self::acquire_us`] block.
    ///
    /// This is the number a "can I afford more per frame?" question
    /// wants: a window can sit at `total_us = 16_666` because it is
    /// vsync-paced with 1ms of work, or because it spent 16ms working.
    /// Only this distinguishes them.
    #[must_use]
    pub fn work_us(self) -> u64 {
        self.build_us
            .saturating_add(self.encode_us)
            .saturating_add(self.render_us)
    }

    /// `total - (build + encode + acquire + render)`: the post-paint
    /// overhead (accessibility emit, IME publish, cache-stats publish,
    /// frame finalize) not captured by a named phase. Saturating, so the
    /// by-construction `total >= phase_sum` invariant never underflows
    /// even if a future caller violates it.
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
    /// peer of `FragmentCacheStats::paint_count`).
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

    /// The rolling window's per-frame samples, **oldest first** — the
    /// frame-time *history*, as opposed to [`Self::snapshot`]'s fold of
    /// it.
    ///
    /// The two projections answer different questions, and the
    /// derivation runs **one way only**: [`Self::snapshot`] is a pure
    /// fold of these samples, so it cannot reconstruct them, while they
    /// can always re-derive it. `snapshot` answers *"what is the
    /// steady-state profile?"* — min/mean/max collapse the window to
    /// `O(1)` numbers an AI client reads over `scene/frame_timings`,
    /// and that is all a text readout needs. A profiler **chart** needs
    /// the series itself: the shape of the last 120 frames (a periodic
    /// hitch, a rising ramp, one catastrophic spike) is exactly the
    /// information the fold destroys — `mean = 8ms` reads identically
    /// for a steady 8ms window and for one alternating 1ms/15ms.
    ///
    /// Borrowed + oldest-first so the caller decides the cost: a chart
    /// maps it to `(x, y)` vertices in one pass without an intermediate
    /// `Vec`, and oldest-first means the sample index *is* the x-axis
    /// position (left = oldest), which is the reading order every
    /// frame-time HUD uses (Unreal `stat unit`, Chrome frame history).
    #[must_use]
    pub fn samples(&self) -> impl ExactSizeIterator<Item = &FrameTiming> + '_ {
        self.samples.iter()
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
    ///
    /// `budget_us` is the per-frame budget the window's `total_us` is
    /// judged against — supplied by the embedder at the AI-paced RPC
    /// read (the R890 "store the source, project on read" rule: the
    /// accumulator stays budget-agnostic; the budget is a read-time
    /// input, so changing the window's target frame rate needs no
    /// re-recording). `None` leaves every budget-relative field neutral
    /// (`0`); `Some(b)` classifies each sample as over/under `b` in the
    /// same single fold that computes min/mean/max. A `Some(0)` budget
    /// is taken literally (every non-instant frame is over a zero
    /// budget) — the embedder maps sub-microsecond budgets to `None`
    /// rather than `Some(0)` so the vacuous case never reaches here.
    #[must_use]
    pub fn snapshot(&self, budget_us: Option<u64>) -> Option<FrameTimingsSnapshot> {
        let last = *self.samples.back()?;
        let len = self.samples.len() as u64; // >= 1 past the `?`
        let (mut min_total, mut max_total) = (u64::MAX, 0u64);
        let (mut sum_total, mut sum_build, mut sum_encode, mut sum_acquire, mut sum_render) =
            (0u64, 0u64, 0u64, 0u64, 0u64);
        let mut over_budget_frames: u32 = 0;
        let mut worst_overrun_us: u64 = 0;
        for s in &self.samples {
            min_total = min_total.min(s.total_us);
            max_total = max_total.max(s.total_us);
            sum_total = sum_total.saturating_add(s.total_us);
            sum_build = sum_build.saturating_add(s.build_us);
            sum_encode = sum_encode.saturating_add(s.encode_us);
            sum_acquire = sum_acquire.saturating_add(s.acquire_us);
            sum_render = sum_render.saturating_add(s.render_us);
            if let Some(budget) = budget_us {
                if s.total_us > budget {
                    over_budget_frames = over_budget_frames.saturating_add(1);
                    worst_overrun_us = worst_overrun_us.max(s.total_us - budget);
                }
            }
        }
        let mean_total = sum_total / len;
        let jank_ratio = if budget_us.is_some() {
            jank_ratio_of(over_budget_frames, len)
        } else {
            0.0
        };
        Some(FrameTimingsSnapshot {
            frame_count: self.frame_count,
            window_len: u32::try_from(self.samples.len()).unwrap_or(u32::MAX),
            last,
            min_total_us: min_total,
            mean_total_us: mean_total,
            max_total_us: max_total,
            mean_build_us: sum_build / len,
            mean_encode_us: sum_encode / len,
            mean_acquire_us: sum_acquire / len,
            mean_render_us: sum_render / len,
            mean_fps: fps_from_mean_total_us(mean_total),
            budget_us,
            over_budget_frames,
            worst_overrun_us,
            jank_ratio,
        })
    }
}

/// `over_budget_frames / window_len` as an `f32` fraction in
/// `[0.0, 1.0]`. `window_len >= 1` at every call site (past the
/// `samples.back()?` guard), so the ratio never divides by zero.
#[must_use]
fn jank_ratio_of(over_budget_frames: u32, window_len: u64) -> f32 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "telemetry ratio; no numeric pipeline consumes the f32"
    )]
    {
        over_budget_frames as f32 / window_len as f32
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
    /// Mean acquire-phase (vsync block) µs over the window. R1361.1.
    pub mean_acquire_us: u64,
    /// Mean render-phase µs over the window (work only, acquire excluded).
    pub mean_render_us: u64,
    /// `1e6 / mean_total_us`, `0.0` for a zero mean.
    pub mean_fps: f32,
    /// Per-frame budget the window's `total_us` is judged against, in
    /// microseconds. `None` for an unpaced window (no declared frame
    /// target — an idle retained window has no deadline, so "missed
    /// budget" is meaningless); `Some(1e6 / target_fps)` once a target
    /// is declared. The budget-relative fields below are all neutral
    /// (`0`) when this is `None`.
    pub budget_us: Option<u64>,
    /// Window samples whose `total_us` exceeded [`Self::budget_us`] —
    /// the "dropped frame" / jank count a pro-tool HUD reports (Unreal
    /// `stat unit` hitches, Chrome janky frames). `0` when no budget is
    /// set. Window-scoped, like the min/mean/max (not the cumulative
    /// [`Self::frame_count`]).
    pub over_budget_frames: u32,
    /// Largest single-frame overrun in the window — `max` over samples
    /// of `total_us.saturating_sub(budget_us)`. The worst hitch's
    /// magnitude, in microseconds. `0` when no budget is set or every
    /// frame stayed within budget.
    pub worst_overrun_us: u64,
    /// `over_budget_frames / window_len` — the fraction of the window
    /// that missed budget, in `[0.0, 1.0]`. `0.0` when no budget is
    /// set. Echoed so a client need not re-derive the ratio.
    pub jank_ratio: f32,
}

// ── R1361 §5.16 §5.22 — the in-app read seam ─────────────────────────

/// R1361 — what an **in-app** profiler HUD reads: the rolling per-frame
/// history *and* the aggregate fold of it, as one coherent value.
///
/// The GUI peer of [`FrameTimingsSnapshot`], split by *consumer* rather
/// than by content: that type is the wire projection an AI client pulls
/// over `scene/frame_timings`; this is what a `view` fn draws. The chart
/// plots [`samples`](Self::samples), the readout prints
/// [`snapshot`](Self::snapshot).
///
/// ## Why both, when the snapshot is derived
///
/// [`samples`](Self::samples) is the source of truth and `snapshot` is a
/// **memoized fold of it** (plus two facts the samples do not carry:
/// `frame_count`, which survives ring eviction, and `budget_us`, an
/// external input). Carrying the memo rather than letting the HUD re-fold
/// is deliberate: a HUD computing its own mean/fps/jank would be a second
/// *implementation* of numbers `scene/frame_timings` already answers, and
/// the two would drift. Folding once, here, means **the HUD and the AI
/// client read identical values** — including `budget_us`, which both
/// inherit from the same `jank_budget_us_for_window` source the render
/// loop paces to, so a drawn budget line cannot disagree with the
/// deadline the shell enforces.
///
/// The memo is only safe if the two halves always describe the same
/// window, which is why [`Self::of`] is the way to build one: it folds
/// both from a single [`FrameTimingStats`] in one expression, so a caller
/// cannot pair one window's samples with another's aggregates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameTimingsView {
    /// The rolling window's samples, oldest first (a copy of
    /// [`FrameTimingStats::samples`] at the last publish). **Empty**
    /// before the first measured frame, and off the live shell (headless
    /// / RPC / unit tests) where nothing publishes — one sentinel for
    /// "nothing to chart", mirroring the pane seam's `(0, 0)` =
    /// unmeasured rather than adding an `Option` layer.
    pub samples: Vec<FrameTiming>,
    /// The same aggregate projection `scene/frame_timings` returns, or
    /// `None` before the first measured frame (the
    /// `FrameTimingsUnavailable` bootstrap — [`samples`](Self::samples)
    /// is empty in exactly that case).
    ///
    /// Built by [`Self::of`] from the same accumulator as
    /// [`Self::samples`], so the two cannot describe different windows.
    ///
    /// Read `snapshot.budget_us` for the frame budget, and note that
    /// `None` there is load-bearing rather than a missing value: an idle
    /// retained window has no deadline, so "missed budget" is
    /// meaningless (see [`FrameTimingsSnapshot::budget_us`]). A HUD MUST
    /// therefore draw a budget line only when it is `Some` — a hardcoded
    /// 16.7ms rule would assert a deadline the window does not have.
    /// `Some` appears once a target is declared via `scene/set_fps`,
    /// which is also what makes the window paint continuously: the
    /// budget line and the streaming cadence are one decision, not two.
    pub snapshot: Option<FrameTimingsSnapshot>,
}

impl FrameTimingsView {
    /// The only correct way to build one: fold BOTH halves from a single
    /// [`FrameTimingStats`], so the samples and the aggregates describing
    /// them always come from the same window and the same instant.
    ///
    /// The fields stay `pub` (a `view` fn destructures them freely, and a
    /// test wants a hand-made one), so this does not make the pairing
    /// unforgeable — it makes the correct pairing the path of least
    /// resistance and gives the invariant one place to live.
    #[must_use]
    pub fn of(stats: &FrameTimingStats, budget_us: Option<u64>) -> Self {
        Self {
            samples: stats.samples().copied().collect(),
            snapshot: stats.snapshot(budget_us),
        }
    }
}

/// Owner-cache holder for the published [`FrameTimingsView`].
///
/// **Deliberately not a [`Signal`](pinion_core::Signal)** — this is the
/// one seam in the reactive family that must not be reactive, and the
/// reason is a cycle rather than a preference.
///
/// A `Signal::set` marks every subscribed owner dirty, and the shell
/// bridges owner-dirty into `request_redraw`. Frame timings change every
/// frame *by construction* (they are wall-clock µs), so the equality-skip
/// that floors every other publish at "not dirty" can never fire here:
/// publish → dirty → redraw → paint → publish would spin an idle window
/// at 100% CPU forever, on a window that is supposed to sleep at
/// `ControlFlow::Wait`. **A measurement of painting must not drive
/// painting** — it is data *about* the loop, so letting it close the loop
/// is circular.
///
/// So the value is *sampled*, not subscribed: the shell overwrites the
/// holder after each recorded frame and a `view` reads whatever is there
/// at paint time. Cadence stays an independent decision — a HUD paints
/// continuously because a frame target is declared (`scene/set_fps` →
/// `frame_budget_for_window` → the `WaitUntil` re-arm), and an unpaced
/// window simply shows the last measured frames at its next natural
/// repaint. That separation of "what to draw" from "when to draw" is what
/// keeps the seam from manufacturing a cadence no one asked for.
pub struct FrameTimingsHolder {
    inner: RefCell<Rc<FrameTimingsView>>,
}

impl Default for FrameTimingsHolder {
    fn default() -> Self {
        Self {
            inner: RefCell::new(Rc::new(FrameTimingsView::default())),
        }
    }
}

impl FrameTimingsHolder {
    /// Overwrite the published view (the shell's post-record publish).
    pub fn publish(&self, view: FrameTimingsView) {
        *self.inner.borrow_mut() = Rc::new(view);
    }

    /// The currently-published view. Cheap: clones an `Rc`, never the
    /// sample vector, so a `view` fn may read it per paint.
    #[must_use]
    pub fn sample(&self) -> Rc<FrameTimingsView> {
        Rc::clone(&self.inner.borrow())
    }
}

/// R1366.8 §5.16 §5.22 — the frame-timings slot: its key, its default and its
/// **`per_scope`** verdict as one expression — the second `per_scope` slot (after
/// R1366.7's `VIEWPORT_SIZE`), declared in `pinion-runtime` since the profiler
/// lives here.
///
/// `per_scope`, NOT `inherited`: the shell PUBLISHES into this at root but only
/// for the primary window (`publish_frame_timings` returns early for any other),
/// so the root's history is *the primary window's*. Inheriting would make a
/// secondary window's HUD chart an interleaving of two windows' frames; a
/// per-scope empty [`FrameTimingsView`] is the honest "this window has no history
/// yet". [`provider_slot_tests!`](pinion_core::provider_slot_tests) asserts a
/// child scope does NOT resolve the root's.
///
/// No `provide`: the shell does not SEED a value, it OVERWRITES the holder via
/// [`FrameTimingsHolder::publish`] — so there is no late-seed panic. And the
/// publish is **demand-gated**: it reads the holder get-only (never creating it),
/// so a window whose binding never called [`use_frame_timings`] pays nothing.
/// That get-only access uses [`FRAME_TIMINGS.key()`](ProviderSlot::key) directly
/// (the slot models declare / resolve / seed, not this specialized read).
pub static FRAME_TIMINGS: ProviderSlot<FrameTimingsHolder> = ProviderSlot::per_scope(
    "__pinion.reactive.frame_timings",
    FrameTimingsHolder::default,
);

/// R1361 §5.16 §5.22 — read the live frame-timing history + declared
/// budget from a `view` fn, for an **in-app** profiler HUD.
///
/// Registers demand on first call: the shell publishes only when this
/// holder exists, so a binding that never reads it pays nothing (the
/// `publish_pane_viewports` "no registered panes ⇒ return early" gate,
/// applied to a per-owner slot instead of a tag map). The O(window) copy
/// is therefore charged to the one window that asked to chart itself.
///
/// Returns an **empty** [`FrameTimingsView`] before the first measured
/// frame and wherever nothing publishes (headless / RPC / unit tests) —
/// graceful, no panic, mirroring
/// [`measured_monospace_cell`](pinion_core::measured_monospace_cell)
/// rather than the strict `use_viewport_size` shape, for the same reason:
/// this is read from a pure `view` fn, so a bare view-fn unit test must
/// not panic for want of a shell.
///
/// # Purity (§6.3 / §2 #3)
///
/// Reading a wall-clock value from `view` looks like it breaks the
/// view-fn purity the `dry_run` guarantee rests on. It does not, and the
/// distinction is the same one animation already relies on: the view
/// stays a pure function *of published state*, and the non-determinism
/// enters at the **publish**, exactly as an animation's wall-clock `dt`
/// enters at `tick_animations_for_window` rather than inside `view`.
/// Under `dry_run` / `simulate` nothing publishes, so the value is frozen
/// and the simulation is reproducible.
///
/// The honest cost is the fragment cache: a HUD's subtree re-hashes every
/// frame, so it never caches. That is inherent — a readout that cached
/// would be a readout that stopped updating — and it is confined to the
/// subtree that reads this.
#[must_use]
pub fn use_frame_timings() -> Rc<FrameTimingsView> {
    pinion_core::Owner::current().map_or_else(
        || Rc::new(FrameTimingsView::default()),
        |owner| FRAME_TIMINGS.resolve(&owner).sample(),
    )
}

#[cfg(test)]
mod seam_tests {
    use super::{
        FrameTiming, FrameTimingStats, FrameTimingsHolder, FrameTimingsView, use_frame_timings,
    };
    use pinion_core::Owner;

    // The verdict, EMITTED from the declaration. For `per_scope` this asserts a
    // child scope does NOT resolve the root's — the shell publishes primary-only,
    // so inheriting would chart two windows' frames interleaved.
    pinion_core::provider_slot_tests!(
        r1366_8_frame_timings_is_per_scope,
        super::FRAME_TIMINGS,
        FrameTimingsHolder::default
    );

    #[test]
    fn r1361_samples_are_oldest_first_and_survive_the_fold() {
        // The property the chart's x-axis depends on: sample index ==
        // reading order, left = oldest. `snapshot` cannot supply this —
        // that is the whole reason `samples` exists.
        let mut stats = FrameTimingStats::new();
        for total in [100_u64, 300, 200] {
            stats.record(FrameTiming::new(10, 10, 0, 10, total));
        }
        let got: Vec<u64> = stats.samples().map(|s| s.total_us).collect();
        assert_eq!(got, vec![100, 300, 200], "oldest first, insertion order");

        // …and the fold genuinely destroys it: these two windows share
        // every aggregate `snapshot` reports but have opposite shapes.
        // If a chart could be drawn from `snapshot` alone, `samples`
        // would be redundant — it cannot, and this pins why.
        let mut jagged = FrameTimingStats::new();
        for total in [1_000_u64, 15_000, 1_000, 15_000] {
            jagged.record(FrameTiming::new(10, 10, 0, 10, total));
        }
        let mut steady = FrameTimingStats::new();
        for total in [1_000_u64, 15_000, 15_000, 1_000] {
            steady.record(FrameTiming::new(10, 10, 0, 10, total));
        }
        let (a, b) = (
            jagged.snapshot(None).unwrap(),
            steady.snapshot(None).unwrap(),
        );
        assert_eq!(
            (a.min_total_us, a.mean_total_us, a.max_total_us),
            (b.min_total_us, b.mean_total_us, b.max_total_us),
            "the two windows are indistinguishable through the fold",
        );
        assert_ne!(
            jagged.samples().map(|s| s.total_us).collect::<Vec<_>>(),
            steady.samples().map(|s| s.total_us).collect::<Vec<_>>(),
            "…but distinguishable through the series, which is what a chart plots",
        );
    }

    #[test]
    fn r1361_use_frame_timings_is_graceful_with_no_owner_and_no_shell() {
        // Read from a bare view-fn unit test: no Owner scope at all.
        // Must not panic (the `measured_monospace_cell` shape) — a
        // binding's view fn is exercised directly all over this repo.
        let bare = use_frame_timings();
        assert!(bare.samples.is_empty(), "nothing measured off the shell");
        assert!(bare.snapshot.is_none(), "no window ⇒ no aggregates");

        // Inside an Owner but with no shell publishing: same sentinel.
        // This is the RPC / headless / TUI case.
        let owner = Owner::new();
        let unpublished = owner.run(use_frame_timings);
        assert_eq!(*unpublished, FrameTimingsView::default());
    }

    #[test]
    fn r1361_reading_the_seam_never_marks_the_owner_dirty() {
        // THE load-bearing property of this seam. If reading subscribed
        // the owner the way every sibling `use_*` hook does, the shell's
        // dirty→redraw bridge would turn each publish into a repaint,
        // and since frame timings differ every frame by construction the
        // equality-skip could never break the cycle: an idle window
        // would spin at 100% CPU forever. A measurement of painting must
        // not drive painting.
        let owner = Owner::new();
        owner.run(|| {
            let _ = use_frame_timings();
        });
        owner.clear_dirty();

        // A publish carrying genuinely new data — the every-frame case.
        let holder = super::FRAME_TIMINGS.resolve(&owner);
        holder.publish(view_of(&[9_999], Some(16_666)));
        assert!(
            !owner.is_dirty(),
            "publishing frame timings must NOT dirty the owner — a Signal here \
             would spin the window at 100% CPU (see FrameTimingsHolder)",
        );

        // The value is still observed: sampled, not subscribed.
        let seen = owner.run(use_frame_timings);
        assert_eq!(seen.samples.len(), 1);
        assert_eq!(seen.snapshot.expect("published").budget_us, Some(16_666));
    }

    #[test]
    fn r1361_holder_sample_reflects_the_latest_publish() {
        let holder = FrameTimingsHolder::default();
        assert!(
            holder.sample().samples.is_empty(),
            "empty before any publish"
        );
        holder.publish(view_of(&[10], None));
        holder.publish(view_of(&[20, 30], Some(8_333)));
        let got = holder.sample();
        assert_eq!(
            got.samples.iter().map(|s| s.total_us).collect::<Vec<_>>(),
            vec![20, 30],
            "last publish wins",
        );
        assert_eq!(got.snapshot.expect("published").budget_us, Some(8_333));
    }

    /// A published view built the way the shell builds one: the samples
    /// and the snapshot both come from ONE `FrameTimingStats`, so the
    /// aggregates always describe the series beside them.
    fn view_of(totals: &[u64], budget_us: Option<u64>) -> FrameTimingsView {
        let mut stats = FrameTimingStats::new();
        for &total in totals {
            stats.record(FrameTiming::new(1, 1, 0, 1, total));
        }
        FrameTimingsView {
            samples: stats.samples().copied().collect(),
            snapshot: stats.snapshot(budget_us),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FRAME_TIMING_WINDOW, FrameTiming, FrameTimingStats};

    #[test]
    fn r907_empty_window_has_no_snapshot() {
        let stats = FrameTimingStats::new();
        assert!(stats.is_empty());
        assert_eq!(stats.frame_count(), 0);
        assert_eq!(stats.window_len(), 0);
        assert!(stats.snapshot(None).is_none());
    }

    #[test]
    fn r907_single_frame_aggregates_to_itself() {
        let mut stats = FrameTimingStats::new();
        stats.record(FrameTiming::new(300, 100, 0, 80, 540));
        let snap = stats.snapshot(None).expect("one sample yields a snapshot");
        assert_eq!(snap.frame_count, 1);
        assert_eq!(snap.window_len, 1);
        assert_eq!(snap.last, FrameTiming::new(300, 100, 0, 80, 540));
        // One sample: min == mean == max == that frame's total.
        assert_eq!(snap.min_total_us, 540);
        assert_eq!(snap.mean_total_us, 540);
        assert_eq!(snap.max_total_us, 540);
        assert_eq!(snap.mean_build_us, 300);
        assert_eq!(snap.mean_encode_us, 100);
        assert_eq!(snap.mean_render_us, 80);
        // No budget supplied: every budget-relative field is neutral.
        assert_eq!(snap.budget_us, None);
        assert_eq!(snap.over_budget_frames, 0);
        assert_eq!(snap.worst_overrun_us, 0);
        assert!(snap.jank_ratio.abs() < f32::EPSILON);
    }

    #[test]
    fn r1361_1_acquire_is_counted_by_total_but_not_by_work() {
        // The whole point of splitting the phase. Two frames with the
        // SAME total: one vsync-blocked doing almost nothing, one
        // genuinely working. Pre-R1361.1 the acquire was billed to
        // render, so these were indistinguishable.
        let blocked = FrameTiming::new(400, 100, 15_000, 200, 15_800);
        let working = FrameTiming::new(400, 100, 0, 15_200, 15_800);
        assert_eq!(blocked.total_us, working.total_us, "same frame duration");
        assert_eq!(
            blocked.phase_sum_us(),
            working.phase_sum_us(),
            "both account for the same span",
        );
        // …but the WORK differs by two orders of magnitude, which is the
        // only thing that answers "can I afford more per frame?".
        assert_eq!(blocked.work_us(), 700, "blocked: idle, headroom to spare");
        assert_eq!(working.work_us(), 15_700, "working: no headroom left");
        assert!(
            blocked.work_us() < working.work_us() / 10,
            "the split must separate 'waiting' from 'slow'",
        );
        // The partition still closes over the four phases.
        assert_eq!(blocked.other_us(), 100);
        assert_eq!(
            blocked.phase_sum_us() + blocked.other_us(),
            blocked.total_us,
        );
    }

    #[test]
    fn r1361_1_a_zero_acquire_frame_has_work_equal_to_phase_sum() {
        // The TUI / stub-renderer shape: no swapchain, so nothing blocks
        // (`WidgetRenderer::last_acquire_us` defaults to 0) and the two
        // sums coincide. This is also every pre-R1361.1 sample's shape,
        // which is why the migration preserved their assertions.
        let t = FrameTiming::new(300, 100, 0, 80, 540);
        assert_eq!(t.work_us(), t.phase_sum_us());
        assert_eq!(t.work_us(), 480);
    }

    #[test]
    fn r907_phase_sum_and_other_partition_total() {
        let t = FrameTiming::new(300, 100, 0, 80, 540);
        assert_eq!(t.phase_sum_us(), 480);
        assert_eq!(t.other_us(), 60);
        // total >= phase_sum is the by-construction invariant; other
        // saturates to 0 rather than underflowing if it is violated.
        let degenerate = FrameTiming::new(300, 100, 0, 80, 100);
        assert_eq!(degenerate.other_us(), 0);
    }

    #[test]
    fn r907_min_mean_max_over_window() {
        let mut stats = FrameTimingStats::new();
        // totals: 400, 600, 980 -> min 400, max 980, mean 660.
        stats.record(FrameTiming::new(200, 100, 0, 50, 400));
        stats.record(FrameTiming::new(300, 150, 0, 70, 600));
        stats.record(FrameTiming::new(500, 200, 0, 120, 980));
        let snap = stats.snapshot(None).unwrap();
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
            stats.record(FrameTiming::new(10, 10, 0, 10, 100));
        }
        assert_eq!(stats.window_len(), FRAME_TIMING_WINDOW);
        assert_eq!(stats.frame_count(), FRAME_TIMING_WINDOW as u64);
        // Push one more: ring stays capped, lifetime count keeps going.
        stats.record(FrameTiming::new(900, 50, 0, 50, 2000));
        assert_eq!(stats.window_len(), FRAME_TIMING_WINDOW);
        assert_eq!(stats.frame_count(), FRAME_TIMING_WINDOW as u64 + 1);
        let snap = stats.snapshot(None).unwrap();
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
            stats.record(FrameTiming::new(10, 10, 0, 10, 100));
        }
        // Overflow by a whole window of a different value: every
        // original sample must have evicted.
        for _ in 0..FRAME_TIMING_WINDOW {
            stats.record(FrameTiming::new(20, 20, 0, 20, 300));
        }
        let snap = stats.snapshot(None).unwrap();
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
        stats.record(FrameTiming::new(10_000, 4_000, 0, 2_000, 16_666));
        let snap = stats.snapshot(None).unwrap();
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
        stats.record(FrameTiming::new(0, 0, 0, 0, 0));
        let snap = stats.snapshot(None).unwrap();
        assert_eq!(snap.mean_total_us, 0);
        assert!(
            snap.mean_fps.abs() < f32::EPSILON,
            "zero mean total must not divide to infinity",
        );
    }

    // ── R925 §5.16 §5.7 — frame-budget compliance / jank ─────────────

    #[test]
    fn r925_no_budget_leaves_jank_fields_neutral() {
        // A window with no declared frame target: budget-relative
        // fields stay neutral regardless of how the frames timed.
        let mut stats = FrameTimingStats::new();
        stats.record(FrameTiming::new(200, 100, 0, 50, 9_999));
        let snap = stats.snapshot(None).unwrap();
        assert_eq!(snap.budget_us, None);
        assert_eq!(snap.over_budget_frames, 0);
        assert_eq!(snap.worst_overrun_us, 0);
        assert!(snap.jank_ratio.abs() < f32::EPSILON);
    }

    #[test]
    fn r925_budget_classifies_over_and_under() {
        // budget 500µs; totals 400 / 600 / 980. Two of three exceed it.
        let mut stats = FrameTimingStats::new();
        stats.record(FrameTiming::new(200, 100, 0, 50, 400));
        stats.record(FrameTiming::new(300, 150, 0, 70, 600));
        stats.record(FrameTiming::new(500, 200, 0, 120, 980));
        let snap = stats.snapshot(Some(500)).unwrap();
        assert_eq!(snap.budget_us, Some(500));
        assert_eq!(snap.over_budget_frames, 2, "600 and 980 exceed 500");
        // Worst overrun is the largest single excess: 980 - 500 = 480.
        assert_eq!(snap.worst_overrun_us, 480);
        // jank_ratio == over / window_len == 2/3.
        assert!((snap.jank_ratio - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn r925_all_within_budget_is_zero_jank() {
        // Every frame under a generous budget: zero jank, zero overrun,
        // but the budget itself is still reported (distinct from "no
        // budget" — a client can tell "on budget" from "untracked").
        let mut stats = FrameTimingStats::new();
        stats.record(FrameTiming::new(200, 100, 0, 50, 400));
        stats.record(FrameTiming::new(300, 150, 0, 70, 600));
        let snap = stats.snapshot(Some(1_000_000)).unwrap();
        assert_eq!(snap.budget_us, Some(1_000_000));
        assert_eq!(snap.over_budget_frames, 0);
        assert_eq!(snap.worst_overrun_us, 0);
        assert!(snap.jank_ratio.abs() < f32::EPSILON);
    }

    #[test]
    fn r925_frame_exactly_at_budget_is_not_over() {
        // The boundary is strict-greater: a frame whose total equals the
        // budget met it (a 60fps target is hit by a 16_666µs frame, not
        // missed). Only total > budget counts as a dropped frame.
        let mut stats = FrameTimingStats::new();
        stats.record(FrameTiming::new(8_000, 4_000, 0, 2_000, 16_666));
        let snap = stats.snapshot(Some(16_666)).unwrap();
        assert_eq!(snap.over_budget_frames, 0, "== budget is on-budget");
        assert_eq!(snap.worst_overrun_us, 0);
        // One µs over the budget flips it to a dropped frame.
        let snap_over = stats.snapshot(Some(16_665)).unwrap();
        assert_eq!(snap_over.over_budget_frames, 1);
        assert_eq!(snap_over.worst_overrun_us, 1);
    }

    #[test]
    fn r925_all_frames_over_budget_saturate_to_full_jank() {
        // A budget tighter than every frame: jank_ratio == 1.0.
        let mut stats = FrameTimingStats::new();
        for _ in 0..4 {
            stats.record(FrameTiming::new(100, 50, 0, 30, 300));
        }
        let snap = stats.snapshot(Some(100)).unwrap();
        assert_eq!(snap.over_budget_frames, 4);
        assert_eq!(snap.window_len, 4);
        assert_eq!(snap.worst_overrun_us, 200, "300 - 100");
        assert!((snap.jank_ratio - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn r925_jank_is_window_scoped_not_cumulative() {
        // over_budget_frames counts the rolling window, not all time —
        // an evicted over-budget frame stops contributing, exactly like
        // min/mean/max. Fill the window with over-budget frames, then
        // overflow it with under-budget ones: the count drains to 0
        // while frame_count keeps climbing.
        let mut stats = FrameTimingStats::new();
        for _ in 0..FRAME_TIMING_WINDOW {
            stats.record(FrameTiming::new(10, 10, 0, 10, 900));
        }
        let busy = stats.snapshot(Some(500)).unwrap();
        assert_eq!(
            busy.over_budget_frames,
            u32::try_from(FRAME_TIMING_WINDOW).unwrap()
        );
        for _ in 0..FRAME_TIMING_WINDOW {
            stats.record(FrameTiming::new(10, 10, 0, 10, 100));
        }
        let calm = stats.snapshot(Some(500)).unwrap();
        assert_eq!(calm.over_budget_frames, 0, "over-budget frames evicted");
        assert_eq!(calm.worst_overrun_us, 0);
        assert_eq!(calm.frame_count, 2 * FRAME_TIMING_WINDOW as u64);
    }
}
