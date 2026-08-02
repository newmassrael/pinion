//! `scene/frame_timings` RPC method dispatch — R907 §5.16 + §5.7.
//!
//! Exposes
//! [`pinion_runtime::FrameTimingsSnapshot`] — the
//! per-window frame-timing profiler projection the §5.16
//! [`pinion_runtime::frame_timing`] substrate produces — over the
//! JSON-RPC surface, so an AI agent can answer *"how long are frames
//! taking, and where is the time going?"* without scraping pixels or
//! guessing. This is the "measure" half of the §1 northern-star's
//! measure-first pro-tool-performance axis: a frame-budget tuning
//! round can only be evidence-based if the frame cost is readable
//! first.
//!
//! Sibling of [`scene/cache_stats`](fn@crate::cache_stats): that method
//! reports paint-fragment cache hit-rate; this one reports the
//! wall-clock cost of the build / encode / acquire / render phases the cache
//! lives inside. They share the per-window-telemetry topology
//! (embedder pre-resolves a `Copy` snapshot onto the dispatch context;
//! the handler just projects it to the wire shape) but stay separate
//! methods — the [`scene/cache_stats`](mod@crate::cache_stats) module's
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
//!       "build_us": 320, "encode_us": 110, "acquire_us": 8200,
//!       "render_us": 80, "total_us": 8740, "other_us": 30,
//!       "work_us": 510,
//!       "settle_passes": 1, "settled": true, "shape_misses": 0,
//!       "gpu_us": 640,
//!       "scene_nodes": 412, "layout_nodes": 412, "encode_nodes": 9,
//!       "access_nodes": 21
//!     },
//!     "produce": {
//!       "passes_total": 9, "shape_misses_total": 130, "nodes_total": 3708
//!     },
//!     "focus": { "derivations_total": 6, "retries_total": 3 },
//!     "mirror": {
//!       "scenes_total": 4, "passes_total": 7,
//!       "shape_misses_total": 12, "unsettled_total": 0, "nodes_total": 2884
//!     },
//!     "window": {
//!       "min_total_us": 8100, "mean_total_us": 8600, "max_total_us": 9800,
//!       "mean_build_us": 310, "mean_encode_us": 105,
//!       "mean_acquire_us": 8100, "mean_render_us": 78,
//!       "mean_gpu_us": 655, "max_gpu_us": 1180,
//!       "gpu_sample_count": 118, "gpu_timing_supported": true,
//!       "gpu_dropped_total": 0,
//!       "max_scene_nodes": 412, "max_layout_nodes": 824,
//!       "max_encode_nodes": 412, "max_access_nodes": 21
//!     },
//!     "mean_fps": 116.3,
//!     "budget_us": 8333,
//!     "over_budget_frames": 3,
//!     "worst_overrun_us": 1467,
//!     "jank_ratio": 0.025
//!   }
//! }
//! ```
//!
//! - `frame_count` is cumulative across the window's whole lifetime;
//!   `window_len` is the rolling-window size the `window` aggregates
//!   fold over (capped at [`pinion_runtime::FRAME_TIMING_WINDOW`]).
//! - `last.other_us = total_us - (build + encode + acquire + render)`
//!   — the post-paint overhead (accessibility / IME / finalize) not in
//!   a named phase. `total_us >= build + encode + acquire + render`
//!   always (disjoint sub-intervals), so a client can assert the
//!   partition.
//! - `mean_fps = 1e6 / window.mean_total_us` (`0.0` for a zero mean);
//!   echoed so a client need not re-derive the rate.
//! - **`acquire_us` is a BLOCK, not work** (R1361.1): the
//!   `get_current_texture()` wait for a swapchain image. Under
//!   `PresentMode::AutoVsync` it is the vsync pace-setter, so a healthy
//!   60fps window spends most of its `total_us` here while doing almost
//!   nothing. Read `work_us = build + encode + render` to answer "how
//!   much room do I have?", and `total_us` to answer "how long was the
//!   frame". Before R1361.1 the block was billed to `render_us`, which
//!   made an idle vsync-blocked window read as GPU-bound.
//! - `render_us` is **CPU-side GPU-submit cost** with the acquire
//!   excluded, not GPU execution wall-clock: queue submission returns
//!   before the GPU has run any of it. **`gpu_us` is that wall-clock**
//!   (R1537) — the rasterizer's compute passes plus the blit, taken with
//!   `wgpu` timestamp queries on the device pinion now owns. The two are
//!   independent: a window can be entirely GPU-bound with every CPU phase
//!   reading fast, which is precisely what nothing on this wire could say
//!   before.
//! - **`gpu_us` is not a phase of `last` and does not enter `total_us`.**
//!   A timestamp is readable only once the GPU has executed the commands
//!   that wrote it, so reading it inside the frame would stall the
//!   pipeline the measurement describes. It is polled, and therefore
//!   reports a frame a step or two back — sound as a level, unsound to add
//!   to the `last` row. Unreal's `stat gpu` has the same latency for the
//!   same reason.
//! - **`gpu_us` / `mean_gpu_us` / `max_gpu_us` are OMITTED, never zeroed,**
//!   when the host has no `TIMESTAMP_QUERY` adapter or no sample has landed
//!   yet. A `0` would assert the GPU did nothing, which reads as an
//!   excellent frame. Read the omission through
//!   `window.gpu_timing_supported`, which is ALWAYS present: `false` means
//!   this machine will never report one (stop waiting), `true` with
//!   `gpu_sample_count == 0` means the first sample is still in flight
//!   (read again in a frame) — unless `gpu_dropped_total` is rising, which
//!   means the timer is running and discarding every result. `gpu_sample_count` is also the denominator
//!   of `mean_gpu_us`, which is averaged over timed samples rather than
//!   over `window_len` — GPU samples are sparser than frames.
//! - **The node census** (R1538) — `last.scene_nodes` / `last.layout_nodes` /
//!   `last.encode_nodes`, with `window.max_*` peers — answers the question no
//!   duration on this wire can: **how big was the thing each phase walked?**
//!   `build_us` is a wall-clock measurement on one host, so a frame that is
//!   fast because the tree is small and a frame that is fast because the host
//!   is quick report the same number. The counts do not move with the host.
//!
//!   That is what makes "60fps with large scenes" checkable. The claim is not
//!   about any machine's clock, it is that per-frame work is bounded by what
//!   is **visible** rather than by how big the model is — so a binding that
//!   windows its data reports the same `scene_nodes` at a thousand rows and at
//!   a million, and one that builds every row reports the model. A CI guard
//!   can assert that invariance without a wall-clock threshold, which is the
//!   only way to assert it without flaking.
//!
//!   `access_nodes` is the FOURTH, and it counts a different walk: the AT
//!   tree `V::access_node` builds every paint. Without it a binding that
//!   windows its paint and enumerates its whole model to assistive technology
//!   satisfies every assertion the other three support while doing O(model)
//!   work each frame.
//!
//!   Read the first three as three questions: `scene_nodes` is the painted tree's size,
//!   `layout_nodes` is what the settle loop measured to get there (they differ
//!   exactly when `settle_passes > 1`), and `encode_nodes` is how much of the
//!   tree the paint had to re-walk — far below `scene_nodes` on a frame the
//!   §5.16 fragment cache served, and the thing `scene/cache_stats`' hit rate
//!   cannot state (replaying two enormous fragments and two tiny ones are both
//!   100%).
//!
//!   The `window.max_*` peers are maxima and not means on purpose: the
//!   property is an upper bound, and one frame that built the whole model is
//!   the entire defect — a mean hides it.
//!
//!   **These are counts, and pinion ships no cost to multiply them by** — the
//!   `shape_misses` policy, for the same reason: the per-node cost moves with
//!   the host, the scene and the CPU, so a consumer measures it on its own
//!   machine and multiplies. `build_us / layout_nodes` is that measurement.
//! - `budget_us` is the per-frame budget the window is judged against
//!   (`1e6 / target_fps`, set via `scene/set_fps`). **Omitted entirely**
//!   (`null`) for an unpaced window — an idle retained window has no
//!   frame deadline, so jank is undefined; the three jank fields are
//!   then all `0`. `over_budget_frames` is the window's dropped-frame
//!   count (`total_us > budget_us`, strict), `worst_overrun_us` the
//!   largest single overrun, and `jank_ratio = over_budget_frames /
//!   window_len` — the measure-first signal a frame-budget tuning round
//!   reads before optimizing (Unreal `stat unit` hitches / Chrome jank).
//!
//! ## Work that is not a frame
//!
//! Four producers run `view` fns; only one of them paints, and the ring
//! above counts only that one. The other three ride the same reply as
//! cumulative totals a client **differences across a call** to price it:
//!
//! - `produce` (R1460) — the RPC scene producer. A `scene/snapshot` settles
//!   a scene exactly as a paint does but records no frame, so introspection
//!   never manufactures a picture the user never saw. Without this pair an
//!   agent on the §2 #2 path could not see the work its OWN calls caused.
//!   A read served from the committed frame (`from: paint`) never reaches
//!   the producer, so its delta is `0` — the true price of reusing a painted
//!   frame, and the reason the group below exists.
//! - `focus` (R1464) — the focus enumeration. A request naming a node the
//!   dispatch just made paintable misses, and the binding answers by
//!   re-deriving from a pure view run, one per painted window. Read
//!   `derivations_total / retries_total`: that ratio IS the per-miss cost,
//!   and it is the painted-window count on a healthy binding.
//! - `mirror` (R1465) — the stored paint scene a mutating dispatch leaves
//!   behind for `from: paint` to read (R705). One side-effect-free view +
//!   layout **per painted window, per mutating call**, which on a
//!   multi-window binding is the largest thing a call does and was the last
//!   producer with no counter at all. `scenes_total` is the fan-out width,
//!   `passes_total / scenes_total` the settle depth, and `unsettled_total`
//!   the mirrors that spent the settle budget without converging — a stored
//!   scene an agent might read as final while it is still moving.
//!
//! All three are binding-wide, not per-window, because the producer, the focus
//! enumeration and the re-store fan-out are: the same totals ride whichever
//! window is asked. A backend without one of these producers reports its group
//! as zeros, which is the honest answer there — `pinion-tui` paints a single
//! terminal window and re-stores nothing, so its `mirror` is all-zero because
//! no such work exists, not because none was measured.
//!
//! ## Multi-window scope
//!
//! Pre-resolved by the embedder, exactly like
//! [`scene/cache_stats`](fn@crate::cache_stats):
//! `pinion-shell::ShellCore::window_scoped_rpc_reads` looks up the
//! per-window [`FrameTimingsSnapshot`] via
//! `ShellCore::frame_timings_for_window` and installs it on
//! [`DispatchContext::frame_timings`](crate::dispatch::DispatchContext::frame_timings) before dispatch. The handler
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
    /// R1361.1 — blocking wait for a swapchain image. Idle, not work;
    /// the vsync pace-setter under `AutoVsync`.
    pub acquire_us: u64,
    /// GPU command-buffer record + submit (CPU-side only, acquire
    /// excluded).
    pub render_us: u64,
    /// R1537 — GPU wall-clock µs for a recent frame: the rasterizer's
    /// compute passes plus the blit, measured by the GPU's own clock.
    ///
    /// **Not a phase of this frame, and not part of `total_us`.** Every
    /// other duration here is CPU time about *this* frame; a GPU timestamp
    /// is readable only after the GPU has run the commands that wrote it,
    /// so reading it in-frame would stall the pipeline the measurement
    /// exists to describe. It is polled instead, and therefore describes a
    /// frame a step or two back — sound to read as a level, unsound to add
    /// to this row.
    ///
    /// Omitted from the wire (`null`) on a host whose adapter has no
    /// timestamp queries, and while the first samples are still in flight.
    /// **Absent is not `0`**: a `0` would say the GPU did nothing, which
    /// reads as an excellent frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_us: Option<u64>,
    /// Whole productive frame.
    pub total_us: u64,
    /// `total - (build + encode + acquire + render)`: post-paint
    /// overhead.
    pub other_us: u64,
    /// R1361.1 — `build + encode + render`: the frame's real work, with
    /// the `acquire_us` block excluded. Echoed so a client need not
    /// re-derive the one number a capacity question actually wants.
    pub work_us: u64,
    /// R1459 — how many view + layout passes this paint spent reaching its
    /// fixed point. `1` is a frame that converged immediately; the
    /// `SETTLE_PASS_BUDGET` value is a frame that gave up and asked for
    /// another, which is the binding telling you its view and layout disagree.
    ///
    /// The first **count** on this wire, next to seven durations, because
    /// `build_us` covers the whole loop: one heavy pass and four cheap
    /// disagreeing passes cost the same microseconds and want opposite fixes.
    pub settle_passes: u32,
    /// R1459 — whether the paint reached its fixed point, or spent
    /// `settle_passes` and gave up. Read WITH `settle_passes`: a frame that
    /// converges exactly on the budget and one that gave up report the same
    /// count, and only this separates them. `false` on a frame that is still
    /// moving means the binding's view and layout disagree about a value each
    /// derives from the other.
    pub settled: bool,
    /// R1459 — how many text runs this paint handed to the shaper (layout-cache
    /// misses, not lookups). A steady-state repaint should read `0`; R1454
    /// measured one miss at 18.5µs against a 118ns hit, so this is one
    /// multiplication away from a budget answer — and it is what catches a
    /// binding that ignores a consumer-honoured sampling bound like
    /// `resize_contents_precision`.
    pub shape_misses: u64,
    /// R1538 — nodes in the tree this frame painted: the picture's **size**,
    /// as against its cost.
    ///
    /// The number a scale claim is made of. A binding that windows a million
    /// rows into a viewport reports the same value here as it does at a
    /// thousand; one that builds every row reports the model. No duration on
    /// this wire distinguishes those on a fast host.
    pub scene_nodes: u32,
    /// R1538 — nodes measured across every settle pass of this frame: what the
    /// build actually paid for.
    ///
    /// Equals [`Self::scene_nodes`] on a frame that converged immediately, and
    /// exceeds it by roughly `settle_passes` when the view and layout had to
    /// negotiate. `build_us / layout_nodes` is the per-node build cost on the
    /// reading host.
    pub layout_nodes: u32,
    /// R1538 — nodes the encode walk entered: the paint-side half.
    ///
    /// A cacheable container that hits short-circuits its subtree, so this
    /// sits far below [`Self::scene_nodes`] on a steady frame, and the ratio
    /// is what says the fragment cache did its job **on this frame**. `0` on a
    /// backend with no encode phase, which is the statement its
    /// [`Self::encode_us`] already makes there.
    pub encode_nodes: u32,
    /// R1538 — nodes the frame's accessibility walk produced.
    ///
    /// The SECOND per-frame traversal, and the one the other three cannot
    /// see: `V::access_node` builds its own tree every paint, so a binding
    /// can window its paint perfectly and still enumerate its whole model to
    /// the AT layer — every scale claim the other counts support would hold
    /// while the frame did O(model) work. `0` on a window with no AT adapter.
    pub access_nodes: u32,
}

/// R1460 §5.16 §2 #2 — cumulative work the RPC **scene producer** has done
/// since boot, which is deliberately not counted as frames.
///
/// A `scene/snapshot` settles a scene exactly as a paint does but records no
/// frame, so that introspection never manufactures a picture the user never
/// saw. The consequence for the §2 #2 primary path was that an agent could not
/// see the work its OWN calls caused. Difference these across a call to price
/// that call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FrameTimingsProduce {
    /// Cumulative view + layout passes run for introspection.
    pub passes_total: u64,
    /// Cumulative shaper misses run for introspection.
    pub shape_misses_total: u64,
    /// R1538 — cumulative layout nodes measured for introspection. The SIZE of
    /// what an agent's own calls settled; `passes_total` counts loop
    /// iterations, which is the same number for a forty-node scene and a
    /// forty-thousand-node one.
    pub nodes_total: u64,
}

/// R1464 §5.16 §5.39 §2 #2 — cumulative view work the **focus enumeration** has
/// caused since boot, which is deliberately not counted as frames.
///
/// A focus request naming a node this dispatch just made paintable misses the
/// enumeration, and the binding answers by re-deriving it from a fresh, pure
/// view run — one per window it has painted (R1463). That work never becomes a
/// picture, so no frame records it, and until this pair nothing on any surface
/// could see it grow with the window count.
///
/// Binding-wide, not per-window: the enumeration is one Tab order across every
/// window, so these totals are identical on whichever window's snapshot is
/// read. Difference the pair across a call to price that call —
/// `derivations_total / retries_total` is what one missed request costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FrameTimingsFocus {
    /// Cumulative view runs performed to derive a window's focusable tags.
    pub derivations_total: u64,
    /// Cumulative focus requests that missed the enumeration and forced a
    /// re-derive. `0` in the steady state.
    pub retries_total: u64,
}

/// R1465 §5.16 §5.12 §2 #7 — cumulative work the **stored paint-scene mirror**
/// has done since boot, which is deliberately not counted as frames.
///
/// A dispatch that changed visible state re-renders each painted window and
/// stores the result, so a follow-up `scene/snapshot from: paint` answers with
/// the state just committed instead of waiting for the compositor. That render
/// runs no side effects, but it is a full view + layout per painted window on
/// every mutating call — the single largest thing such a call does on a
/// multi-window binding, and until R1465 the only view producer with no counter.
///
/// Difference these across a call: `scenes_total` is how many windows it fanned
/// out to, `passes_total / scenes_total` how far each had to settle, and
/// `unsettled_total` how many gave up at the budget with a still-moving scene
/// left in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FrameTimingsMirror {
    /// Cumulative paint-scene mirrors rendered and stored.
    pub scenes_total: u64,
    /// Cumulative view + layout passes spent across those mirrors.
    pub passes_total: u64,
    /// Cumulative shaper misses charged to those passes.
    pub shape_misses_total: u64,
    /// Cumulative mirrors that hit the settle budget without converging.
    pub unsettled_total: u64,
    /// R1538 — cumulative layout nodes measured across those mirrors. The one
    /// number that grows with BOTH the window fan-out and each window's scene
    /// size, which is the product a mutating call actually pays.
    pub nodes_total: u64,
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
    /// R1361.1 — mean acquire-phase (vsync block) µs over the window.
    pub mean_acquire_us: u64,
    /// Mean render-phase µs over the window (acquire excluded).
    pub mean_render_us: u64,
    /// R1537 — mean GPU µs over the window samples that carry one.
    ///
    /// Averaged over [`Self::gpu_sample_count`], not over `window_len`:
    /// GPU timings are sparser than frames, so dividing by the frame count
    /// would understate the GPU by whatever fraction went unmeasured.
    /// Omitted (`null`) when no sample in the window carries one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_gpu_us: Option<u64>,
    /// R1537 — largest GPU µs in the window. The peer of `max_total_us`,
    /// and the field a hitch hunt reads: a mean hides the one frame that
    /// cost 40ms, and that frame is the whole complaint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_gpu_us: Option<u64>,
    /// R1537 — how many samples in this window carry a GPU timing, i.e.
    /// the denominator of `mean_gpu_us`. Always present, including `0`.
    pub gpu_sample_count: u32,
    /// R1537 — whether this window's render backend can time the GPU at
    /// all.
    ///
    /// The field that makes an omitted `gpu_us` *readable*. Without it a
    /// client sees a missing key and cannot tell a machine that will never
    /// report one (`false` — stop waiting, profile on the CPU side) from a
    /// window whose first sample is still in flight (`true` with
    /// `gpu_sample_count == 0` — read again in a frame). Always present,
    /// including `false`, because "the answer is no" is an answer and an
    /// omitted key is not.
    pub gpu_timing_supported: bool,
    /// R1537 — GPU measurements taken and discarded since boot (a failed
    /// staging map, or a tick pair the driver reported out of order).
    ///
    /// The field that keeps `gpu_timing_supported: true` with
    /// `gpu_sample_count: 0` honest. That pair reads as "the first sample
    /// is in flight, read again in a frame" — correct for a young window,
    /// and wrong forever on a host where every measurement fails. A rising
    /// value here says the timer is running and throwing results away,
    /// which is a different problem with a different fix.
    pub gpu_dropped_total: u64,
    /// R1538 — largest `scene_nodes` in the window: the biggest tree this
    /// window has painted recently.
    ///
    /// A max and not a mean, because the property it guards is an upper bound
    /// and a mean cannot state one. One frame that built the whole model is
    /// the entire defect, and it averages away.
    pub max_scene_nodes: u32,
    /// R1538 — largest `layout_nodes` in the window: the most build work any
    /// recent frame paid for, settle passes included.
    pub max_layout_nodes: u32,
    /// R1538 — largest `encode_nodes` in the window: the deepest any recent
    /// frame's encode walk had to go.
    ///
    /// Peaks at the painted tree's size on a frame the fragment cache could
    /// not serve (a resize, a theme change, the first paint) — the honest
    /// worst case rather than a defect.
    pub max_encode_nodes: u32,
    /// R1538 — largest `access_nodes` in the window: the biggest AT tree any
    /// recent frame assembled.
    pub max_access_nodes: u32,
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
    /// Per-frame budget the window's `total_us` is judged against, in
    /// microseconds — `1e6 / target_fps` once a frame target is
    /// declared via `scene/set_fps`. Omitted from the wire (`null`)
    /// when the window is unpaced (no target — an idle retained window
    /// has no deadline, so jank is undefined); the three budget-relative
    /// fields below are then all `0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_us: Option<u64>,
    /// Window samples whose `total_us` exceeded [`Self::budget_us`] —
    /// the dropped-frame / jank count (Unreal `stat unit` hitches,
    /// Chrome janky frames). `0` when no budget is set. Window-scoped,
    /// like the min/mean/max — not the cumulative `frame_count`.
    pub over_budget_frames: u32,
    /// Largest single-frame overrun in the window, in microseconds
    /// (`max(total_us - budget_us)`). The worst hitch's magnitude; `0`
    /// when no budget is set or every frame stayed within budget.
    pub worst_overrun_us: u64,
    /// `over_budget_frames / window_len` — the fraction of the window
    /// that missed budget, in `[0.0, 1.0]`. `0.0` when no budget is
    /// set; echoed so a client need not re-derive the ratio.
    pub jank_ratio: f32,
    /// R1460 — work done producing scenes for introspection, not for the user.
    pub produce: FrameTimingsProduce,
    /// R1464 — work done deriving the focus enumeration, which paints nothing.
    pub focus: FrameTimingsFocus,
    /// R1465 — work done re-rendering the stored mirror a `from: paint` read
    /// answers from. All-zero on a backend that keeps no mirror.
    pub mirror: FrameTimingsMirror,
}

/// Project a per-window [`FrameTimingsSnapshot`] onto the wire-shaped
/// [`FrameTimingsOutcome`].
///
/// # Errors
///
/// - [`FrameTimingsError::FrameTimingsUnavailable`] — the embedder did
///   not register a snapshot on [`DispatchContext::frame_timings`](crate::dispatch::DispatchContext::frame_timings).
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
            acquire_us: s.last.acquire_us,
            render_us: s.last.render_us,
            total_us: s.last.total_us,
            other_us: s.last.other_us(),
            work_us: s.last.work_us(),
            settle_passes: s.last.settle_passes,
            settled: s.last.settled,
            shape_misses: s.last.shape_misses,
            gpu_us: s.last.gpu_us,
            scene_nodes: s.last.scene_nodes,
            layout_nodes: s.last.layout_nodes,
            encode_nodes: s.last.encode_nodes,
            access_nodes: s.last.access_nodes,
        },
        produce: FrameTimingsProduce {
            passes_total: s.produce.passes,
            shape_misses_total: s.produce.shape_misses,
            nodes_total: s.produce.nodes,
        },
        focus: FrameTimingsFocus {
            derivations_total: s.focus.derivations,
            retries_total: s.focus.retries,
        },
        mirror: FrameTimingsMirror {
            scenes_total: s.mirror.scenes,
            passes_total: s.mirror.passes,
            shape_misses_total: s.mirror.shape_misses,
            unsettled_total: s.mirror.unsettled,
            nodes_total: s.mirror.nodes,
        },
        window: FrameTimingsWindow {
            min_total_us: s.min_total_us,
            mean_total_us: s.mean_total_us,
            max_total_us: s.max_total_us,
            mean_build_us: s.mean_build_us,
            mean_encode_us: s.mean_encode_us,
            mean_acquire_us: s.mean_acquire_us,
            mean_render_us: s.mean_render_us,
            mean_gpu_us: s.mean_gpu_us,
            max_gpu_us: s.max_gpu_us,
            gpu_sample_count: s.gpu_sample_count,
            gpu_timing_supported: s.gpu_timing_supported,
            gpu_dropped_total: s.gpu_dropped_total,
            max_scene_nodes: s.max_scene_nodes,
            max_layout_nodes: s.max_layout_nodes,
            max_encode_nodes: s.max_encode_nodes,
            max_access_nodes: s.max_access_nodes,
        },
        mean_fps: s.mean_fps,
        budget_us: s.budget_us,
        over_budget_frames: s.over_budget_frames,
        worst_overrun_us: s.worst_overrun_us,
        jank_ratio: s.jank_ratio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_runtime::{FrameTiming, FrameTimingStats};

    fn snapshot_of(samples: &[FrameTiming]) -> FrameTimingsSnapshot {
        snapshot_with_budget(samples, None)
    }

    fn snapshot_with_budget(
        samples: &[FrameTiming],
        budget_us: Option<u64>,
    ) -> FrameTimingsSnapshot {
        let mut stats = FrameTimingStats::new();
        for s in samples {
            stats.record(*s);
        }
        stats
            .snapshot(budget_us)
            .expect("non-empty window yields a snapshot")
    }

    #[test]
    fn r1459_work_counts_reach_the_wire_and_are_not_derived_from_durations() {
        // The counts ride the same sample as the durations, and nothing on the
        // wire recomputes them: a frame can be fast AND unsettled, or slow and
        // converged on its first pass. The fixture makes both independent of
        // the timings by giving two samples identical durations and different
        // work.
        let converged = FrameTiming::new(100, 10, 0, 20, 200).with_work(1, true, 0);
        let struggling = FrameTiming::new(100, 10, 0, 20, 200).with_work(4, false, 37);

        let a = frame_timings(Some(snapshot_of(&[converged]))).unwrap();
        assert_eq!(a.last.settle_passes, 1);
        assert!(a.last.settled);
        assert_eq!(a.last.shape_misses, 0);

        let b = frame_timings(Some(snapshot_of(&[struggling]))).unwrap();
        assert_eq!(b.last.settle_passes, 4);
        assert!(
            !b.last.settled,
            "a frame that spent its budget without arriving says so — the only \
             other record of it is a log line no RPC client can read",
        );
        assert_eq!(b.last.shape_misses, 37);

        // Same durations, different work: the durations cannot be the source.
        assert_eq!(a.last.build_us, b.last.build_us);
        assert_eq!(a.last.total_us, b.last.total_us);
        assert_ne!(a.last.settle_passes, b.last.settle_passes);
        assert_ne!(a.last.shape_misses, b.last.shape_misses);
    }

    #[test]
    fn r1460_producer_work_is_reported_and_is_not_frames() {
        // The producer settles scenes for introspection and records no frame,
        // which is the right contract and was also a blind spot: an agent on
        // the §2 #2 path could not see the work its own calls caused. The two
        // totals answer it WITHOUT being folded into the ring — a frame count
        // and a produce count must never be the same number.
        let mut snap = snapshot_of(&[FrameTiming::new(100, 10, 0, 20, 200).with_work(1, true, 0)]);
        snap.produce = pinion_runtime::ProduceWork {
            passes: 9,
            shape_misses: 130,
            nodes: 3_708,
        };

        let out = frame_timings(Some(snap)).unwrap();
        assert_eq!(out.produce.passes_total, 9);
        assert_eq!(out.produce.shape_misses_total, 130);
        assert_eq!(out.frame_count, 1, "one FRAME, whatever the producer did");
        assert_eq!(
            out.last.settle_passes, 1,
            "and the frame's own count is untouched by the producer's",
        );
    }

    #[test]
    fn r1464_focus_work_is_reported_and_is_neither_frames_nor_produce() {
        // The third producer of view runs. It must be readable as its OWN pair:
        // folding it into `produce` would price an agent's introspection with
        // work the user's own interaction caused, and folding it into the ring
        // would invent frames nobody saw.
        let mut snap = snapshot_of(&[FrameTiming::new(100, 10, 0, 20, 200).with_work(1, true, 0)]);
        snap.produce = pinion_runtime::ProduceWork {
            passes: 9,
            shape_misses: 0,
            nodes: 3_708,
        };
        snap.focus = pinion_runtime::FocusWork {
            derivations: 6,
            retries: 2,
        };

        let out = frame_timings(Some(snap)).unwrap();
        assert_eq!(out.focus.derivations_total, 6);
        assert_eq!(out.focus.retries_total, 2);
        assert_eq!(
            out.focus.derivations_total / out.focus.retries_total,
            3,
            "the ratio a client actually reads: three painted windows re-derived \
             per missed request",
        );
        assert_eq!(
            out.produce.passes_total, 9,
            "not folded into the producer's"
        );
        assert_eq!(out.frame_count, 1, "and not counted as frames");
    }

    #[test]
    fn r1464_a_backend_that_never_missed_a_focus_request_reads_zero() {
        // The steady state is all-zero, and that is a MEASUREMENT, not a
        // sentinel: unlike `settle_passes` (where `0` means never-measured and
        // every real paint is `>= 1`), zero focus work is the honest report of a
        // binding whose every focus request hit.
        let out = frame_timings(Some(snapshot_of(&[FrameTiming::new(1, 2, 3, 4, 20)]))).unwrap();
        assert_eq!(out.focus.derivations_total, 0);
        assert_eq!(out.focus.retries_total, 0);
    }

    #[test]
    fn r1465_mirror_work_is_reported_and_is_none_of_the_other_three() {
        // The fourth producer. Folding it into `produce` would price the
        // window fan-out a MUTATION caused as introspection the agent asked
        // for; folding it into the ring would invent pictures nobody saw.
        let mut snap = snapshot_of(&[FrameTiming::new(100, 10, 0, 20, 200).with_work(1, true, 0)]);
        snap.produce = pinion_runtime::ProduceWork {
            passes: 9,
            shape_misses: 130,
            nodes: 3_708,
        };
        snap.mirror = pinion_runtime::MirrorWork {
            scenes: 3,
            passes: 6,
            shape_misses: 12,
            unsettled: 1,
            nodes: 2_884,
        };

        let out = frame_timings(Some(snap)).unwrap();
        assert_eq!(
            out.mirror.scenes_total, 3,
            "the fan-out width: one stored scene per painted window",
        );
        assert_eq!(
            out.mirror.passes_total / out.mirror.scenes_total,
            2,
            "and the depth each had to settle to — the ratio a client reads",
        );
        assert_eq!(out.mirror.shape_misses_total, 12);
        assert_eq!(
            out.mirror.unsettled_total, 1,
            "a stored scene that was still moving when the budget ran out; the \
             mirror cannot ask for another frame, so counting is how it says so",
        );
        assert_eq!(
            out.produce.passes_total, 9,
            "not folded into the producer's"
        );
        assert_eq!(out.frame_count, 1, "and not counted as frames");
    }

    #[test]
    fn r1465_a_backend_without_a_mirror_reads_zero() {
        // `pinion-tui` has no R705 re-store — it paints one window that
        // publishes itself. Zero here is "that producer does not exist", which
        // is the same number as "it did nothing" and is honest as both.
        let out = frame_timings(Some(snapshot_of(&[FrameTiming::new(1, 2, 3, 4, 20)]))).unwrap();
        assert_eq!(out.mirror.scenes_total, 0);
        assert_eq!(out.mirror.passes_total, 0);
        assert_eq!(out.mirror.unsettled_total, 0);
    }

    #[test]
    fn r1459_a_sample_no_settle_loop_produced_reads_zero() {
        // `FrameTiming::new` without `with_work` — a hand-built fixture or the
        // `Default`. Zero passes is distinguishable from every real paint's
        // `>= 1`, so a consumer can tell "not measured" from "measured one".
        let bare = FrameTiming::new(1, 2, 3, 4, 20);
        let out = frame_timings(Some(snapshot_of(&[bare]))).unwrap();
        assert_eq!(out.last.settle_passes, 0, "the never-measured sentinel");
        assert!(!out.last.settled);
        assert_eq!(out.last.shape_misses, 0);
    }

    // ── R1538 §5.16 — the node census on the wire ──

    #[test]
    fn r1538_census_reaches_the_wire_and_is_not_derived_from_durations() {
        // Two samples with IDENTICAL durations and different censuses. If any
        // of the three counts were computed from a time, this could not hold —
        // and the whole reason the census exists is that a duration cannot
        // distinguish a small expensive tree from a large cheap one.
        let small = FrameTiming::new(100, 10, 0, 20, 200).with_census(40, 40, 3);
        let huge = FrameTiming::new(100, 10, 0, 20, 200).with_census(40_000, 82_000, 39_000);

        let a = frame_timings(Some(snapshot_of(&[small]))).unwrap();
        let b = frame_timings(Some(snapshot_of(&[huge]))).unwrap();

        assert_eq!(a.last.scene_nodes, 40);
        assert_eq!(a.last.layout_nodes, 40);
        assert_eq!(a.last.encode_nodes, 3);
        assert_eq!(b.last.scene_nodes, 40_000);
        assert_eq!(b.last.layout_nodes, 82_000);
        assert_eq!(b.last.encode_nodes, 39_000);
        assert_eq!(a.last.build_us, b.last.build_us);
        assert_eq!(a.last.total_us, b.last.total_us);
    }

    #[test]
    fn r1538_window_reports_the_peak_and_not_the_average() {
        // The guarded property is an upper bound, so one frame that built the
        // whole model must survive the fold. A mean over these three samples
        // is ~13_400 — a number that looks fine and hides the 40_000.
        let snap = snapshot_of(&[
            FrameTiming::new(100, 10, 0, 20, 200).with_census(120, 120, 4),
            FrameTiming::new(100, 10, 0, 20, 200).with_census(40_000, 40_000, 40_000),
            FrameTiming::new(100, 10, 0, 20, 200).with_census(120, 240, 4),
        ]);
        let out = frame_timings(Some(snap)).unwrap();
        assert_eq!(out.window.max_scene_nodes, 40_000);
        assert_eq!(out.window.max_encode_nodes, 40_000);
        assert_eq!(
            out.window.max_layout_nodes, 40_000,
            "the peak is per axis, so a settle-heavy frame and a big frame do \
             not have to be the same frame",
        );
        assert_eq!(
            out.last.scene_nodes, 120,
            "and `last` is still the last frame, not the worst one",
        );
    }

    #[test]
    fn r1538_a_settled_frame_reports_the_same_count_twice() {
        // `scene_nodes` is the last pass's, `layout_nodes` the sum. On a frame
        // that converged immediately they are equal, and that equality is the
        // statement that nothing was re-measured — not a redundancy.
        let out = frame_timings(Some(snapshot_of(&[FrameTiming::new(100, 10, 0, 20, 200)
            .with_work(1, true, 0)
            .with_census(412, 412, 9)])))
        .unwrap();
        assert_eq!(out.last.settle_passes, 1);
        assert_eq!(out.last.scene_nodes, out.last.layout_nodes);

        // Three passes over the same tree: the tree is unchanged, the work is
        // three times as much, and only `layout_nodes` says so.
        let out = frame_timings(Some(snapshot_of(&[FrameTiming::new(100, 10, 0, 20, 200)
            .with_work(3, true, 0)
            .with_census(412, 1_236, 9)])))
        .unwrap();
        assert_eq!(out.last.scene_nodes, 412);
        assert_eq!(out.last.layout_nodes, 1_236);
    }

    #[test]
    fn r1538_producer_and_mirror_report_their_own_node_totals() {
        // All four view producers now state their size, and the three that are
        // not frames stay out of the ring. An agent differences these across
        // its own call to price it — `passes_total` alone counts loop
        // iterations, which is the same number for a 40-node scene and a
        // 40,000-node one.
        let mut snap = snapshot_of(&[FrameTiming::new(100, 10, 0, 20, 200).with_census(40, 40, 3)]);
        snap.produce = pinion_runtime::ProduceWork {
            passes: 2,
            shape_misses: 0,
            nodes: 84_000,
        };
        snap.mirror = pinion_runtime::MirrorWork {
            scenes: 3,
            passes: 3,
            shape_misses: 0,
            unsettled: 0,
            nodes: 126_000,
        };
        let out = frame_timings(Some(snap)).unwrap();
        assert_eq!(out.produce.nodes_total, 84_000);
        assert_eq!(out.mirror.nodes_total, 126_000);
        assert_eq!(
            out.mirror.nodes_total / out.mirror.scenes_total,
            42_000,
            "per-window scene size, which neither `scenes` nor `passes` states",
        );
        assert_eq!(out.frame_count, 1, "and none of it is a frame");
        assert_eq!(out.last.scene_nodes, 40);
    }

    #[test]
    fn r1538_the_access_walk_is_counted_separately_from_the_paint() {
        // The a11y tree is a SECOND traversal. If it rode any of the other
        // three counts, a binding that windows its paint and enumerates its
        // whole model would be invisible — which is the exact failure this
        // field exists to catch, so the two must be independently settable.
        let windowed = FrameTiming::new(100, 10, 0, 20, 200)
            .with_census(63, 63, 1)
            .with_access_census(21);
        let leaking = FrameTiming::new(100, 10, 0, 20, 200)
            .with_census(63, 63, 1)
            .with_access_census(1_000_000);

        let a = frame_timings(Some(snapshot_of(&[windowed]))).unwrap();
        let b = frame_timings(Some(snapshot_of(&[leaking]))).unwrap();
        assert_eq!(a.last.access_nodes, 21);
        assert_eq!(b.last.access_nodes, 1_000_000);
        assert_eq!(
            a.last.scene_nodes, b.last.scene_nodes,
            "identical painted trees — only the AT walk differs, and only \
             this field can say so",
        );
        assert_eq!(a.window.max_access_nodes, 21);
        assert_eq!(b.window.max_access_nodes, 1_000_000);
    }

    #[test]
    fn r1538_census_serializes_under_last_and_window() {
        let out = frame_timings(Some(snapshot_of(&[FrameTiming::new(300, 100, 0, 80, 540)
            .with_census(412, 824, 9)
            .with_access_census(21)])))
        .unwrap();
        let json = serde_json::to_value(out).unwrap();
        for (group, key, want) in [
            ("last", "scene_nodes", 412),
            ("last", "layout_nodes", 824),
            ("last", "encode_nodes", 9),
            ("window", "max_scene_nodes", 412),
            ("window", "max_layout_nodes", 824),
            ("window", "max_encode_nodes", 9),
            ("last", "access_nodes", 21),
            ("window", "max_access_nodes", 21),
        ] {
            assert_eq!(
                json.get(group)
                    .and_then(|g| g.get(key))
                    .and_then(serde_json::Value::as_u64),
                Some(want),
                "{group}.{key} missing from {json}",
            );
        }
    }

    #[test]
    fn r1538_a_never_painted_sample_reads_zero_and_is_distinguishable() {
        // Unlike a duration, a zero census is not ambiguous: a painted scene
        // has a root, so every real frame reports `scene_nodes >= 1`. That is
        // what lets a client tell "no paint yet" from "a tiny paint" without a
        // sentinel.
        let out = frame_timings(Some(snapshot_of(&[FrameTiming::new(1, 2, 3, 4, 20)]))).unwrap();
        assert_eq!(out.last.scene_nodes, 0);
        assert_eq!(out.last.layout_nodes, 0);
        assert_eq!(out.last.encode_nodes, 0);
        assert_eq!(out.last.access_nodes, 0);
    }

    #[test]
    fn r907_missing_snapshot_errors() {
        let err = frame_timings(None).unwrap_err();
        assert_eq!(err, FrameTimingsError::FrameTimingsUnavailable);
    }

    #[test]
    fn r907_single_frame_projects_field_for_field() {
        let snap = snapshot_of(&[FrameTiming::new(300, 100, 0, 80, 540)]);
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
        let snap = snapshot_of(&[FrameTiming::new(200, 90, 0, 60, 400)]);
        let out = frame_timings(Some(snap)).unwrap();
        // total == build + encode + acquire + render + other, by
        // construction (this fixture pins acquire = 0).
        assert_eq!(
            out.last.total_us,
            out.last.build_us + out.last.encode_us + out.last.render_us + out.last.other_us,
        );
    }

    #[test]
    fn r907_window_ordering_invariant() {
        let snap = snapshot_of(&[
            FrameTiming::new(200, 100, 0, 50, 400),
            FrameTiming::new(300, 150, 0, 70, 600),
            FrameTiming::new(500, 200, 0, 120, 980),
        ]);
        let out = frame_timings(Some(snap)).unwrap();
        assert!(out.window.min_total_us <= out.window.mean_total_us);
        assert!(out.window.mean_total_us <= out.window.max_total_us);
        assert_eq!(out.window.min_total_us, 400);
        assert_eq!(out.window.max_total_us, 980);
    }

    #[test]
    fn r907_serialized_nests_last_and_window() {
        let snap = snapshot_of(&[FrameTiming::new(300, 100, 0, 80, 540)]);
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
        let snap = snapshot_of(&[FrameTiming::new(10_000, 4_000, 0, 2_000, 16_666)]);
        let out = frame_timings(Some(snap)).unwrap();
        #[allow(clippy::cast_precision_loss, reason = "test re-derivation")]
        let rederived = 1_000_000.0_f32 / out.window.mean_total_us as f32;
        assert!((out.mean_fps - rederived).abs() < 1e-3);
    }

    // ── R925 §5.16 §5.7 — frame-budget compliance / jank on the wire ──

    #[test]
    fn r925_no_budget_omits_budget_us_and_zeroes_jank() {
        // Unpaced window: budget_us absent from the wire, the three
        // jank fields present and zero (a client tells "untracked" from
        // "on budget" by budget_us presence).
        let snap = snapshot_of(&[FrameTiming::new(200, 100, 0, 50, 9_999)]);
        let out = frame_timings(Some(snap)).unwrap();
        assert_eq!(out.budget_us, None);
        assert_eq!(out.over_budget_frames, 0);
        assert_eq!(out.worst_overrun_us, 0);
        assert!(out.jank_ratio.abs() < f32::EPSILON);
        let json = serde_json::to_value(out).unwrap();
        assert!(
            json.get("budget_us").is_none(),
            "budget_us must be omitted (null) when unpaced; got {json}",
        );
        // The jank fields are always present (not skipped) so a client
        // can read them unconditionally.
        assert_eq!(
            json.get("over_budget_frames")
                .and_then(serde_json::Value::as_u64),
            Some(0)
        );
        assert_eq!(
            json.get("worst_overrun_us")
                .and_then(serde_json::Value::as_u64),
            Some(0)
        );
        assert!(json.get("jank_ratio").is_some());
    }

    #[test]
    fn r925_budget_projects_jank_fields_field_for_field() {
        // budget 500µs; totals 400 / 600 / 980 -> 2 over, worst 480.
        let snap = snapshot_with_budget(
            &[
                FrameTiming::new(200, 100, 0, 50, 400),
                FrameTiming::new(300, 150, 0, 70, 600),
                FrameTiming::new(500, 200, 0, 120, 980),
            ],
            Some(500),
        );
        let out = frame_timings(Some(snap)).unwrap();
        assert_eq!(out.budget_us, Some(500));
        assert_eq!(out.over_budget_frames, 2);
        assert_eq!(out.worst_overrun_us, 480);
        assert!((out.jank_ratio - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn r925_serialized_emits_budget_us_when_paced() {
        let snap = snapshot_with_budget(
            &[FrameTiming::new(8_000, 4_000, 0, 2_000, 16_666)],
            Some(16_666),
        );
        let out = frame_timings(Some(snap)).unwrap();
        let json = serde_json::to_value(out).unwrap();
        assert_eq!(
            json.get("budget_us").and_then(serde_json::Value::as_u64),
            Some(16_666),
            "budget_us must serialize when a target is declared; got {json}",
        );
        // 16_666µs frame meets a 16_666µs budget exactly -> not over.
        assert_eq!(
            json.get("over_budget_frames")
                .and_then(serde_json::Value::as_u64),
            Some(0),
        );
    }

    #[test]
    fn r925_jank_ratio_re_derives_from_over_and_window_len() {
        // A client must be able to re-derive jank_ratio from the
        // integer counts (over_budget_frames / window_len).
        let snap = snapshot_with_budget(
            &[
                FrameTiming::new(100, 50, 0, 30, 300),
                FrameTiming::new(100, 50, 0, 30, 300),
                FrameTiming::new(10, 10, 0, 10, 100),
                FrameTiming::new(10, 10, 0, 10, 100),
            ],
            Some(200),
        );
        let out = frame_timings(Some(snap)).unwrap();
        assert_eq!(out.over_budget_frames, 2);
        assert_eq!(out.window_len, 4);
        #[allow(clippy::cast_precision_loss, reason = "test re-derivation")]
        let rederived = out.over_budget_frames as f32 / out.window_len as f32;
        assert!((out.jank_ratio - rederived).abs() < 1e-6);
    }
}
