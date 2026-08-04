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
use std::time::Instant;

use pinion_core::ProviderSlot;

/// Number of most-recent frames the rolling window retains. ~2s at
/// 60fps — long enough to smooth single-frame jitter into a stable
/// mean/min/max, short enough that the window tracks a workload change
/// (a heavy scroll, a window resize) within a couple of seconds rather
/// than averaging it away. Matches the order of magnitude every
/// in-app profiler HUD uses (Tracy / Chrome frame history ≈ 120–300
/// frames).
pub const FRAME_TIMING_WINDOW: usize = 120;

/// (R1460 §5.16 §2 #6) Microseconds elapsed between two [`Instant`]s, the unit
/// every [`FrameTiming`] field is measured in.
///
/// Lifted out of `pinion-shell` when `pinion-tui` became the second backend to
/// record frames: two surfaces rounding the same quantity their own way is the
/// divergence class, not a style choice — a sample is comparable across
/// backends only if the lowering is one function.
///
/// `saturating_duration_since` guards the (monotonic-clock-impossible)
/// `end < start` case; the `u128 -> u64` cast saturates a frame longer than
/// ~584,000 years, which keeps clippy and the type honest without a real
/// overflow path.
#[must_use]
pub fn instant_delta_us(start: Instant, end: Instant) -> u64 {
    u64::try_from(end.saturating_duration_since(start).as_micros()).unwrap_or(u64::MAX)
}

/// (R1556 §5.16) What one frame asked the renderer to **draw** — the cost of
/// the picture, as against [`FrameTiming::scene_nodes`]'s count of it.
///
/// # Why a node count is not a cost
///
/// R1538 made the frame's node census readable and stated the scale claim this
/// axis is named for with it: per-frame work is bounded by what is *visible*,
/// not by how big the model is. A count is the right shape for that claim —
/// unlike a duration it does not move with the host — but it prices every node
/// at one. A `Container` is one node; a `Text` leaf holding four thousand
/// glyphs is one node. So two frames can report an **identical** census and
/// differ by two orders of magnitude in what they hand the GPU, and R1538's own
/// guard could not see the difference it was built to bound.
///
/// This is the other half: what was drawn, counted in the units a 2D vector
/// renderer is actually charged in — paths and their segments, clip layers,
/// glyph runs and glyphs, and the draw commands that carry them. Same property
/// as the node census (a count, not a duration; the same number on every host),
/// applied to the artifact instead of to the tree that produced it.
///
/// # Taken from the scene that was submitted
///
/// Every field is read off the encoded scene the frame handed to the renderer —
/// after the walk, after the DPI scale — so a subtree the §5.16 fragment cache
/// **replayed** is counted exactly like one encoded this frame. There is no path
/// by which drawn work escapes the count, because the count is the size of the
/// thing that runs.
///
/// That is also the reason to read it beside [`FrameTiming::encode_nodes`],
/// which is the opposite half: that is what the CPU **walked**, this is what the
/// GPU **draws**. A frame that walked nine nodes and draws four thousand glyphs
/// is a fragment cache doing its job; one that walked four thousand nodes to
/// draw the same glyphs is not, and neither number alone says which.
///
/// # What pinion does not ship
///
/// A cost to multiply these by — the [`FrameTiming::shape_misses`] policy, for
/// the same reason. What a glyph or a path segment costs moves with the GPU,
/// the driver and the resolution, so a consumer measures it on its own host and
/// multiplies. `gpu_us / path_segments` is that measurement, and nothing in-tree
/// does it on a consumer's behalf.
///
/// All-zero on a backend with no vector pipeline, which is the statement
/// [`FrameTiming::encode_us`] already makes on a terminal frame: that frame
/// really is build plus commit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DrawWork {
    /// Draw commands the frame encoded: every fill, stroke, image, blurred rect
    /// and clip boundary, in the order the rasterizer consumes them.
    ///
    /// The closest peer of a 3D renderer's draw-call count (Unreal `stat rhi`,
    /// `QQuick3DRenderStats::drawCallCount`), and the coarsest number here — it
    /// says how many things were drawn without saying how big any of them was,
    /// which is exactly the limit [`Self::path_segments`] and [`Self::glyphs`]
    /// exist to lift.
    pub draws: u32,
    /// Paths the frame encoded. One per filled or stroked shape, plus one per
    /// closed clip layer.
    ///
    /// Roughly "how many shapes", where [`Self::draws`] is "how many commands":
    /// the two move together on ordinary content and separate on a frame whose
    /// commands are mostly images or gradients.
    pub paths: u32,
    /// Line and curve segments across those paths — the frame's **geometric**
    /// size, and the number a vector renderer's per-frame cost tracks most
    /// closely (the peer of a 3D renderer's triangle count).
    ///
    /// A rounded rect and a thousand-point polyline are both one path and one
    /// draw; only this separates them. A frame that keeps [`Self::paths`] flat
    /// while this climbs is drawing the same *number* of increasingly expensive
    /// shapes — the chart / node-graph case, and the one a shape count hides.
    ///
    /// **Text is not in here.** A shaped run is encoded as positioned glyphs and
    /// its outlines are turned into paths downstream of the encoding, so a
    /// frame that draws nothing but text reports zero paths and zero segments.
    /// That makes this and [`Self::glyphs`] **disjoint** rather than overlapping
    /// — the frame's vector cost and its text cost can be read apart, instead of
    /// summed into one number from which neither can be recovered.
    pub path_segments: u32,
    /// Clip / blend layers the frame pushed.
    ///
    /// Counted apart from [`Self::draws`] because it is the one field here whose
    /// cost is not per-item: a layer is a separate render target in the coarse
    /// rasterizer, so ten layers over the same pixels is not ten draws' worth of
    /// work. A scroll viewport pushes one, and a binding that pushes one per row
    /// has found the superlinear term before it shows up as a duration.
    pub layers: u32,
    /// Glyph runs the frame issued — one per shaped run handed to the
    /// rasterizer, which is one text draw.
    ///
    /// The denominator of [`Self::glyphs`]: `glyphs / glyph_runs` is the mean
    /// run length, and a frame with many short runs pays per-run overhead a
    /// frame with few long ones does not.
    pub glyph_runs: u32,
    /// Positioned glyphs across those runs — the frame's **text** size.
    ///
    /// The dominant term in a professional 2D application and the one no scene
    /// count can reach: a `Text` leaf is one node whether it holds two glyphs or
    /// four thousand. R1531 measured the glyph-run walk at 37% of a warm-cache
    /// frame, which is the fraction of the frame this number is about.
    ///
    /// The whole of the frame's text cost, because glyph outlines never reach
    /// [`Self::path_segments`] — see that field.
    pub glyphs: u32,
}

impl DrawWork {
    /// The per-field maximum of two censuses — the fold
    /// [`FrameTimingsSnapshot::max_draw`] is built with.
    ///
    /// **The result is not a frame.** Each field is maxed independently, so no
    /// single frame need have drawn all of them; it is six upper bounds carried
    /// in one value, for the reason `max_scene_nodes` and `max_encode_nodes` are
    /// separate maxima rather than one representative sample. A mean cannot
    /// state an upper bound, and the property being guarded is one.
    #[must_use]
    pub fn max_of(self, other: Self) -> Self {
        Self {
            draws: self.draws.max(other.draws),
            paths: self.paths.max(other.paths),
            path_segments: self.path_segments.max(other.path_segments),
            layers: self.layers.max(other.layers),
            glyph_runs: self.glyph_runs.max(other.glyph_runs),
            glyphs: self.glyphs.max(other.glyphs),
        }
    }

    /// (R1557 §5.16) The per-field sum — how [`DrawProfile`](crate::DrawProfile)
    /// accumulates a node's children before subtracting them from its total.
    ///
    /// Saturating for [`Self::draws`]' reason: a count this large is not worth
    /// preserving exactly, and must not be reported as a small one.
    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        Self {
            draws: self.draws.saturating_add(other.draws),
            paths: self.paths.saturating_add(other.paths),
            path_segments: self.path_segments.saturating_add(other.path_segments),
            layers: self.layers.saturating_add(other.layers),
            glyph_runs: self.glyph_runs.saturating_add(other.glyph_runs),
            glyphs: self.glyphs.saturating_add(other.glyphs),
        }
    }

    /// (R1557 §5.16) The per-field difference `self - earlier` — the draw work
    /// that landed in an encoded scene BETWEEN two censuses of it.
    ///
    /// This is the whole measurement principle of
    /// [`DrawProfile`](crate::DrawProfile): the encoded streams only ever grow
    /// during a paint, so a census taken before a subtree is walked and one
    /// taken after differ by exactly what that subtree contributed — whether it
    /// was encoded node by node or replayed whole from the §5.16 fragment
    /// cache.
    ///
    /// # Why the layer field subtracts exactly
    ///
    /// [`Self::layers`] is derived by `paint_adapter::draw_work_of` as
    /// `(n_clips + n_open_clips) / 2`, an integer division, and in general
    /// `⌊a/2⌋ - ⌊b/2⌋` is not `⌊(a-b)/2⌋`. Here it is, because the numerator is
    /// always even: `b` begins and `e` ends give `(b + e) + (b - e) = 2b`. So
    /// the difference of two derived layer counts is the exact layer count of
    /// the span, with no rounding to carry.
    ///
    /// Saturating rather than wrapping, so a (stream-shrink-impossible)
    /// negative difference reports `0` instead of four billion. Nothing relies
    /// on that clamp being silent: `DrawProfile`'s own balance identity —
    /// every node's `own` summing to the root's `total` — is what a clamp would
    /// break, and it is asserted rather than assumed.
    #[must_use]
    pub fn since(self, earlier: Self) -> Self {
        Self {
            draws: self.draws.saturating_sub(earlier.draws),
            paths: self.paths.saturating_sub(earlier.paths),
            path_segments: self.path_segments.saturating_sub(earlier.path_segments),
            layers: self.layers.saturating_sub(earlier.layers),
            glyph_runs: self.glyph_runs.saturating_sub(earlier.glyph_runs),
            glyphs: self.glyphs.saturating_sub(earlier.glyphs),
        }
    }
}

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
    /// (R1459 §5.16 §5.45) How many view + layout passes this paint spent
    /// reaching its fixed point — `1` when the first pass changed nothing,
    /// up to `SETTLE_PASS_BUDGET` when the frame gave up and asked for
    /// another (see `ShellCore::report_unsettled_frame`).
    ///
    /// A **count**, next to five durations, because it answers a question no
    /// duration can. [`Self::build_us`] is the whole settle loop, so a frame
    /// that costs 4ms because one pass is heavy and a frame that costs 4ms
    /// because four cheap passes disagree are the same number — and they want
    /// opposite fixes. This is also the only evidence about whether the budget
    /// itself is right: it was chosen from a survey of the settling chains
    /// that exist, not proven to bound every chain, so a binding that sits at
    /// the budget is data about pinion, not only about the binding.
    ///
    /// `0` on a sample no settle loop produced (a hand-built fixture, the
    /// `Default`), which is distinguishable from every real paint's `>= 1`.
    ///
    /// Read it with [`Self::settled`]: a frame that converges exactly ON the
    /// budget and a frame that gave up both report the budget, so the count
    /// alone cannot tell them apart.
    pub settle_passes: u32,
    /// (R1459 §5.16 §5.45) Whether the paint reached its fixed point, or spent
    /// [`Self::settle_passes`] and gave up.
    ///
    /// A separate field rather than a value encoded into the count, because
    /// they answer different questions: "how much work" is a number and only a
    /// number answers it; "did it finish" is a yes/no and only a bool answers
    /// it. Encoding the second into the first (a sentinel count, a
    /// budget-plus-one) would report passes the frame never ran.
    ///
    /// Without this, a §2 #2 agent reading `settle_passes == 4` on a 4-pass
    /// budget could not tell a converged frame from one that is repainting
    /// forever — and the only other record of that is a `tracing::warn!` the
    /// wire cannot see.
    pub settled: bool,
    /// (R1459 §5.16 §5.36) How many text runs this paint handed to the shaper
    /// — [`LayoutCache`](pinion_text::LayoutCache) misses, not lookups.
    ///
    /// R1454 measured what one miss costs (18.5µs against a 118ns hit) and
    /// bounded the worst offender with Qt's `resizeContentsPrecision`, but
    /// that bound is **consumer-honoured**: a binding that ignores it still
    /// measures every row, and nothing noticed. This is what notices. A steady
    /// state repaint should read `0`.
    ///
    /// # Why this is a count and pinion ships no cost to multiply it by
    ///
    /// R1460 — R1454's 18.5µs-per-miss was measured on one machine with one
    /// font face, and it is recorded there as the RATIONALE for a bound, not
    /// as a constant to compute with. Shipping it as an API would publish a
    /// number pinion cannot know for the host it is running on: the cost moves
    /// with the shaper, the face, the string, the CPU.
    ///
    /// So the split is deliberate and is the whole policy. **The count is
    /// portable and pinion publishes it; the cost is not and pinion refuses
    /// to.** A consumer that needs microseconds measures them on its own host
    /// and multiplies by this — which is exactly the shape of a measurement it
    /// can defend, and nothing in-tree does that multiplication on its behalf.
    pub shape_misses: u64,
    /// (R1537 §5.16) GPU wall-clock microseconds for a recent frame —
    /// the rasterizer's compute passes plus the blit — or `None` when the
    /// host cannot time the GPU or has not produced a sample yet.
    ///
    /// # Why this is not just another duration beside `render_us`
    ///
    /// Every other field on this struct is CPU time, measured by the CPU,
    /// about this frame. This one is GPU time, measured by the GPU, about
    /// *a* frame. [`Self::render_us`] is the cost of recording the frame
    /// and handing it to the driver, and `wgpu` returns from `submit` long
    /// before the GPU executes any of it — so a window can be entirely
    /// GPU-bound while every CPU phase reads fast, and nothing here could
    /// say so. That was the pro-tool-performance axis's largest stated
    /// gap, and it is the first number on this struct that a pro tool
    /// states first (Unreal's `stat gpu`).
    ///
    /// # Why it is one frame behind
    ///
    /// A timestamp is readable only once the GPU has run the commands that
    /// wrote it. Waiting for that inside the frame would serialise CPU and
    /// GPU — the exact stall a profiler exists to find, caused by the
    /// profiler — so the value is polled and lands on a later sample.
    /// It is therefore sound to read across a window, and unsound to pair
    /// with *this* sample's [`Self::build_us`] as though the two described
    /// one frame.
    ///
    /// # Why `Option` and not `0`
    ///
    /// `None` means *no measurement*; `Some(0)` means *measured, and below
    /// the timer's resolution*. A host without `TIMESTAMP_QUERY` reports
    /// the former, and collapsing it to `0` would publish "the GPU did
    /// nothing" — an absent measurement that reads as an excellent result.
    pub gpu_us: Option<u64>,
    /// (R1538 §5.16 §5.45) How many nodes were in the tree this frame
    /// painted — the size of the picture, as opposed to its cost.
    ///
    /// The number a **scale** claim is made of. "60fps with large scenes" is
    /// not a statement about one machine's wall clock, it is the statement
    /// that per-frame work is bounded by what is visible rather than by how
    /// big the model is. A binding that windows a million rows into a
    /// viewport reports the same value here as it does at a thousand; one
    /// that builds every row reports the model. Nothing on this struct could
    /// tell those apart before — [`Self::build_us`] is a duration, and a
    /// duration on a fast host says the same thing in both cases.
    ///
    /// Measured on the pass that produced the painted scene, so it describes
    /// the tree that reached the screen and not an intermediate one the
    /// settle loop discarded.
    pub scene_nodes: u32,
    /// (R1538 §5.16 §5.45) How many nodes the frame's layout work measured in
    /// total, summed across every settle pass.
    ///
    /// [`Self::scene_nodes`] is the tree that survived; this is what was paid
    /// for. They differ exactly when [`Self::settle_passes`] exceeds one, and
    /// their ratio is the per-pass cost that `build_us` alone charges to a
    /// single number — the same argument that put `settle_passes` here.
    ///
    /// `build_us / layout_nodes` is the per-node build cost on THIS host,
    /// which is the multiplication a capacity question wants and the one
    /// pinion refuses to do on a consumer's behalf (see [`Self::shape_misses`]
    /// for the policy).
    pub layout_nodes: u32,
    /// (R1538 §5.16) How many nodes the encode walk entered — the paint-side
    /// half of the census.
    ///
    /// A cacheable container that hits short-circuits its whole subtree, so on
    /// a steady-state frame this is far below [`Self::scene_nodes`], and the
    /// ratio is what says the §5.16 fragment cache did its job *on this
    /// frame*. `scene/cache_stats`' hit rate cannot say it: replaying two
    /// enormous fragments and replaying two tiny ones are both 100%.
    ///
    /// `0` on a backend with no encode phase, which is the same statement its
    /// [`Self::encode_us`] already makes there — a terminal frame really is
    /// build + commit.
    pub encode_nodes: u32,
    /// (R1538 §5.16 §5.40) How many accessibility nodes the frame's AT-tree
    /// walk produced.
    ///
    /// The **second** per-frame traversal, and the one the other three counts
    /// cannot see. `V::access_node` runs per paint and builds its own tree, so
    /// a binding can window its paint perfectly and still enumerate its whole
    /// model to the assistive-technology layer — every scale assertion the
    /// other counts support would hold while the frame did O(model) work.
    ///
    /// Counted where the emit happens, so it describes the tree that was
    /// actually assembled. `0` on a window with no AT adapter and on a backend
    /// that assembles no tree per paint, which is the same "that work does not
    /// exist here" the `mirror` group reports on a backend with no mirror.
    pub access_nodes: u32,
    /// (R1556 §5.16) What the frame asked the renderer to **draw** — see
    /// [`DrawWork`].
    ///
    /// The four counts above are all sizes of a *tree*; this is the size of the
    /// *drawing*, and the two are independent. A frame can hold its node census
    /// flat while its glyph count grows without bound, which is the case those
    /// counts were built to bound and could not see.
    pub draw: DrawWork,
}

impl FrameTiming {
    /// Construct a sample from the measured phase durations, with the R1459
    /// work counts zeroed — see [`Self::with_work`].
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
            settle_passes: 0,
            settled: false,
            shape_misses: 0,
            gpu_us: None,
            scene_nodes: 0,
            layout_nodes: 0,
            encode_nodes: 0,
            access_nodes: 0,
            draw: DrawWork::default(),
        }
    }

    /// (R1556 §5.16) Attach the frame's **draw** census — what the encoded
    /// scene will ask the renderer to draw.
    ///
    /// Its own builder rather than six more `u32`s on [`Self::with_census`], and
    /// the argument that made that one a builder applies twice over here: those
    /// three were same-typed and transposable, these six are same-typed *and*
    /// plausibly-valued in each other's slots. Taking one [`DrawWork`] means the
    /// only way to build the argument is by naming every field, at the one call
    /// site that reads them off the submitted scene.
    #[must_use]
    pub fn with_draw_census(mut self, draw: DrawWork) -> Self {
        self.draw = draw;
        self
    }

    /// (R1538 §5.16 §5.40) Attach the frame's **accessibility** census.
    ///
    /// Its own builder rather than a fourth `u32` on [`Self::with_census`],
    /// for the reason [`Self::with_gpu`] is one: it describes a different
    /// pipeline. The other three count the walk that produces the picture;
    /// this counts the walk that produces the tree an assistive technology
    /// reads. They run at different points of the frame, and one backend has
    /// the first without the second.
    #[must_use]
    pub fn with_access_census(mut self, access_nodes: u32) -> Self {
        self.access_nodes = access_nodes;
        self
    }

    /// (R1538 §5.16) Attach the frame's **node census** to a sample built by
    /// [`Self::new`].
    ///
    /// Its own builder for [`Self::with_work`]'s reason, and the argument is
    /// stronger here rather than weaker: these three are all `u32` counts of
    /// nodes, so a transposed call site would compile, run, and report a
    /// plausible number. The mistake is caught instead by there being exactly
    /// one call site, next to the walks that produced the three values.
    ///
    /// `0` on a sample no paint produced (a hand-built fixture, the
    /// `Default`). Unlike a duration, that zero is distinguishable from every
    /// real frame: a painted scene has a root, so `scene_nodes >= 1`.
    #[must_use]
    pub fn with_census(mut self, scene_nodes: u32, layout_nodes: u32, encode_nodes: u32) -> Self {
        self.scene_nodes = scene_nodes;
        self.layout_nodes = layout_nodes;
        self.encode_nodes = encode_nodes;
        self
    }

    /// (R1537 §5.16) Attach the backend's GPU frame clock to a sample
    /// built by [`Self::new`].
    ///
    /// Its own builder rather than a sixth positional `u64` on `new`, for
    /// the reason [`Self::with_work`] gives — and more strongly here,
    /// because the argument is an `Option<u64>` whose `None` is load
    /// bearing. A backend with no GPU clock must be unable to express that
    /// as a duration.
    #[must_use]
    pub fn with_gpu(mut self, gpu_us: Option<u64>) -> Self {
        self.gpu_us = gpu_us;
        self
    }

    /// (R1459 §5.16) Attach the frame's **work counts** to a sample built by
    /// [`Self::new`].
    ///
    /// A separate builder rather than three more parameters on `new`: that
    /// constructor already takes five positional `u64` durations, and further
    /// ones would make a transposed call site compile silently. These three
    /// have three distinct types (`u32` / `bool` / `u64`), none of which fits
    /// a duration slot, so the shape of the API is what catches the mistake.
    #[must_use]
    pub fn with_work(mut self, settle_passes: u32, settled: bool, shape_misses: u64) -> Self {
        self.settle_passes = settle_passes;
        self.settled = settled;
        self.shape_misses = shape_misses;
        self
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
        // R1537 — folded over the samples that HAVE a GPU timing, with its
        // own count. See `mean_gpu_us`: the GPU denominator is not `len`.
        let (mut sum_gpu, mut max_gpu, mut gpu_sample_count) = (0u64, 0u64, 0u32);
        // R1538 — the census peaks. Max only: the property they guard is an
        // upper bound, and a mean cannot state one.
        let (mut max_scene_nodes, mut max_layout_nodes, mut max_encode_nodes) = (0u32, 0u32, 0u32);
        let mut max_access_nodes = 0u32;
        // R1556 — the same max-not-mean rule, six fields at once. See
        // `DrawWork::max_of`: the result is six upper bounds, not a frame.
        let mut max_draw = DrawWork::default();
        for s in &self.samples {
            max_draw = max_draw.max_of(s.draw);
            max_scene_nodes = max_scene_nodes.max(s.scene_nodes);
            max_layout_nodes = max_layout_nodes.max(s.layout_nodes);
            max_encode_nodes = max_encode_nodes.max(s.encode_nodes);
            max_access_nodes = max_access_nodes.max(s.access_nodes);
            min_total = min_total.min(s.total_us);
            max_total = max_total.max(s.total_us);
            sum_total = sum_total.saturating_add(s.total_us);
            sum_build = sum_build.saturating_add(s.build_us);
            sum_encode = sum_encode.saturating_add(s.encode_us);
            sum_acquire = sum_acquire.saturating_add(s.acquire_us);
            sum_render = sum_render.saturating_add(s.render_us);
            if let Some(gpu) = s.gpu_us {
                sum_gpu = sum_gpu.saturating_add(gpu);
                max_gpu = max_gpu.max(gpu);
                gpu_sample_count = gpu_sample_count.saturating_add(1);
            }
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
            // Filled by the backend after projection: this ring holds FRAMES,
            // and none of the producer's work, the focus enumeration's, or the
            // stored mirror's is one.
            produce: ProduceWork::default(),
            focus: FocusWork::default(),
            mirror: MirrorWork::default(),
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
            mean_gpu_us: (gpu_sample_count > 0).then(|| sum_gpu / u64::from(gpu_sample_count)),
            max_gpu_us: (gpu_sample_count > 0).then_some(max_gpu),
            max_scene_nodes,
            max_layout_nodes,
            max_encode_nodes,
            max_access_nodes,
            max_draw,
            gpu_sample_count,
            // Filled by the backend after projection — see the field doc.
            gpu_timing_supported: false,
            gpu_dropped_total: 0,
            mean_fps: fps_from_mean_total_us(mean_total),
            budget_us,
            over_budget_frames,
            worst_overrun_us,
            jank_ratio,
        })
    }
}

/// (R1460 §5.16 §2 #2) Cumulative work the RPC **scene producer** has done
/// since boot — view runs that never became a frame.
///
/// The producer settles a scene exactly as a paint does, but records no sample:
/// an introspection read must never manufacture a frame the user never saw.
/// That correct contract left the §2 #2 primary path unable to see the work its
/// own calls caused — by the time the window painted, the produce had already
/// warmed the caches and settled the bounds. **Difference this across a call to
/// price that call.**
///
/// A read that is answered from the committed frame (`scene/snapshot
/// from: paint`, R890.1) never reaches the producer, so its delta is `0`. That
/// zero is the true price of reusing a painted frame, not a silence — the work
/// a mutating dispatch causes downstream of the producer is [`MirrorWork`].
///
/// (R1465) A struct rather than the pair of loose `u64`s this began as: two
/// backends now record it, and two same-typed fields written positionally at a
/// distance are the transposition R1459 refused for the same reason.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProduceWork {
    /// View + layout passes run for introspection.
    pub passes: u64,
    /// Shaper misses (layout-cache misses, not lookups) charged to those passes.
    pub shape_misses: u64,
    /// (R1538) Layout nodes measured across those passes.
    ///
    /// The size of the introspection an agent asked for. `passes` prices the
    /// call in loop iterations, which is the same number whether the scene the
    /// producer settled had forty nodes or forty thousand — and on a
    /// million-row binding that difference is the whole cost.
    pub nodes: u64,
}

impl ProduceWork {
    /// Add one producer run's work. `passes` is `u32` and `shape_misses` `u64`
    /// so the two cannot be swapped silently at a call site; `nodes` is `u32`
    /// and would collide with `passes`, which is why it is last and why the
    /// producer's one call site sits directly under the loop that sums it.
    pub fn record(&mut self, passes: u32, shape_misses: u64, nodes: u32) {
        self.passes = self.passes.saturating_add(u64::from(passes));
        self.shape_misses = self.shape_misses.saturating_add(shape_misses);
        self.nodes = self.nodes.saturating_add(u64::from(nodes));
    }
}

/// (R1538 §5.16 §5.45) What one **paint's** scene producer did, handed from the
/// producer to the surface that assembles the [`FrameTiming`] sample.
///
/// The producer's return type is the `Scene` itself — the one thing every
/// caller wants — so this rides a slot the surface reads back one step later
/// rather than widening that return and making every call site carry a
/// measurement it does not use.
///
/// A struct rather than the `(u32, bool, u64)` tuple it began as, for
/// [`ProduceWork`]'s reason and more sharply: R1538 added two `u32` counts to
/// it, so the tuple would have carried three same-typed numbers written
/// positionally at a distance — the transposition R1459 refused. All-zero
/// before a window's first paint, which is the same "no sample yet" the timing
/// ring reports; a real paint has a root, so `scene_nodes >= 1` distinguishes
/// them.
///
/// [`FrameTiming::encode_nodes`] is deliberately NOT here: the encode is the
/// surface's own phase, run after the producer returned, and folding it in
/// would ask the producer to report work it did not do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaintWork {
    /// View + layout passes this paint spent reaching its fixed point.
    pub passes: u32,
    /// Whether it reached that fixed point, or spent the budget.
    pub settled: bool,
    /// Text runs handed to the shaper during those passes.
    pub shape_misses: u64,
    /// Nodes in the tree the last pass produced — the painted scene's size.
    pub scene_nodes: u32,
    /// Nodes measured across every pass — the layout work actually paid for.
    pub layout_nodes: u32,
}

/// (R1465 §5.16 §5.12 §2 #7) Cumulative work the **stored paint-scene mirror**
/// has done since boot — the fourth producer of view runs, and the last one
/// that was neither settled nor counted.
///
/// An RPC dispatch that changed visible state re-renders each painted window's
/// scene and stores it, so a follow-up `scene/snapshot from: paint` reflects the
/// state that was just committed instead of waiting for the compositor to catch
/// up (R705). That render is side-effect-free — no animation tick, no
/// immediate-mode tick, no pane publish — but it is a full view + layout, once
/// per painted window, on every mutating call.
///
/// Read it as three questions the other groups cannot answer:
///
/// - **width** — [`Self::scenes`] per call is the window fan-out. A dispatch on
///   a three-window binding stores three scenes; nothing else on this wire says
///   so.
/// - **depth** — `passes / scenes` is how far the mirror had to settle. `1.0`
///   is a binding whose view and layout agree on the first pass.
/// - **failure** — [`Self::unsettled`] counts the mirrors that spent
///   [`SETTLE_PASS_BUDGET`](crate::SETTLE_PASS_BUDGET) without converging.
///   A stored scene that is still moving is a scene an agent may read as final,
///   so the budget's truncation is reported rather than silent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MirrorWork {
    /// Paint-scene mirrors rendered and stored.
    pub scenes: u64,
    /// View + layout passes spent across those mirrors.
    pub passes: u64,
    /// Shaper misses charged to those passes. Unlike the focus enumeration
    /// (which runs no layout, so shaping it is meaningless), the mirror lays
    /// out against the shared text cache and can miss.
    pub shape_misses: u64,
    /// Mirrors that hit the settle budget without reaching a fixed point.
    pub unsettled: u64,
    /// (R1538) Layout nodes measured across those mirrors.
    ///
    /// The mirror fans out per painted window, so this is the one number that
    /// grows with BOTH the window count and each window's scene size — the
    /// product a mutating call actually pays, which neither `scenes` nor
    /// `passes` states alone.
    pub nodes: u64,
}

impl MirrorWork {
    /// Add one stored mirror's work. `passes` / `settled` / `shape_misses` have
    /// distinct types, so a transposed call among those three does not compile
    /// (R1459's rule for the same reason). R1538's `nodes` shares `passes`'
    /// type and is placed last, next to the single call site's own accumulator.
    pub fn record(&mut self, passes: u32, settled: bool, shape_misses: u64, nodes: u32) {
        self.scenes = self.scenes.saturating_add(1);
        self.passes = self.passes.saturating_add(u64::from(passes));
        self.shape_misses = self.shape_misses.saturating_add(shape_misses);
        self.nodes = self.nodes.saturating_add(u64::from(nodes));
        if !settled {
            self.unsettled = self.unsettled.saturating_add(1);
        }
    }
}

/// (R1464 §5.16 §5.39 §2 #2) Cumulative view work the **focus enumeration** has
/// caused since boot — view runs that never became a frame.
///
/// The third producer of view runs, beside the paint ([`FrameTiming::settle_passes`])
/// and the RPC scene producer
/// ([`ProduceWork`]) and the stored mirror ([`MirrorWork`]). A focus enumeration is a
/// pure function of state ([`Scene::collect_focusable_tags`](pinion_core::Scene::collect_focusable_tags)
/// reads only the view-assigned tag and `focusable` flag), so the backends
/// answer "which tags can take focus?" by running the view with no layout, no
/// paint, and no sample recorded. That is what makes it invisible: the work is
/// real, it scales with the window count, and until this counter nothing on any
/// surface could see it.
///
/// **Binding-wide, not per-window** — the [`FocusManager`](crate::FocusManager)
/// enumeration it feeds is one order across every window, so the same totals
/// ride whichever window's snapshot is read. Peer of the `produce_*` totals in
/// that respect, and read the same way: **difference the pair across a call to
/// price that call.** The ratio is the interesting number —
/// `derivations / retries` is how many windows one missed focus request costs.
///
/// Deliberately NOT folded into the frame ring: that ring holds frames the user
/// saw, and none of this is one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FocusWork {
    /// View runs the focus enumeration has performed.
    ///
    /// Two call sites contribute, and the distinction matters when reading a
    /// climbing count:
    ///
    /// - **per retry** (R1020 / R1463): a [`Self::retries`] re-derive rewrites
    ///   every window the binding has *painted*, because a painted window
    ///   answers the enumeration from a harvested cache that the dispatch just
    ///   invalidated. This is the bounded, on-demand half — `retries` times the
    ///   painted-window count.
    /// - **per fold** (R26): a window the binding *declares* but has not painted
    ///   yet has no cache to answer from, so every fold derives it afresh. This
    ///   is the transient half — it repeats each frame until that window's first
    ///   paint, and a count that climbs while nothing is being focused is that
    ///   transient, not a leak.
    pub derivations: u64,
    /// Focus requests that named a tag the enumeration did not hold, and so
    /// forced a re-derive (the R1020 miss-retry).
    ///
    /// The denominator for [`Self::derivations`]. `0` in the steady state: a
    /// request naming an already-enumerated tag is satisfied by the first
    /// `focus_set` and never reaches the re-derive.
    pub retries: u64,
}

/// (R1464 §5.16 §5.39) The accumulator a backend keeps [`FocusWork`] in.
///
/// Interior-mutable because the deriving call sites hold `&self` — the shell
/// folds the enumeration while borrowing its window map, and a counter must not
/// dictate the borrow shape of the code it measures.
///
/// A type rather than a bare `Cell<FocusWork>` at each backend, because the
/// read-modify-write those call sites would otherwise spell out (`get`, mutate,
/// `set`) has a silent failure mode: drop the `set` and the count is simply
/// lost, with every test still green and the wire still answering. There are
/// four such sites across the two backends and no reason for any of them to
/// hold the sequence.
#[derive(Debug, Default)]
pub struct FocusWorkCell(core::cell::Cell<FocusWork>);

impl FocusWorkCell {
    /// The totals as of now. `Copy`, so this neither borrows nor blocks a
    /// concurrent record.
    #[must_use]
    pub fn get(&self) -> FocusWork {
        self.0.get()
    }

    /// Record one view run performed to derive a window's focusable tags.
    pub fn record_derivation(&self) {
        let mut work = self.0.get();
        work.derivations = work.derivations.saturating_add(1);
        self.0.set(work);
    }

    /// Record one focus request that missed the enumeration and forced a
    /// re-derive.
    pub fn record_retry(&self) {
        let mut work = self.0.get();
        work.retries = work.retries.saturating_add(1);
        self.0.set(work);
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
    /// (R1537 §5.16) Mean GPU µs over the samples in this window that
    /// carry one, or `None` when none of them does.
    ///
    /// The mean is over [`Self::gpu_sample_count`], **not** over
    /// [`Self::window_len`]: GPU timings are sparser than frames (each is
    /// read back a frame or more after it was written, and a frame whose
    /// predecessor has not been harvested is skipped rather than blended),
    /// so dividing by the frame count would systematically understate the
    /// GPU by whatever fraction of frames went unmeasured.
    pub mean_gpu_us: Option<u64>,
    /// (R1537 §5.16) Largest GPU µs among the samples in this window that
    /// carry one, or `None` when none does.
    ///
    /// The peer of [`Self::max_total_us`], and the one a hitch hunt reads:
    /// a mean hides a single frame that cost 40ms, and a single frame that
    /// costs 40ms is the whole complaint.
    pub max_gpu_us: Option<u64>,
    /// (R1538 §5.16) Largest [`FrameTiming::scene_nodes`] in this window —
    /// the biggest tree the window has painted recently.
    ///
    /// A max rather than a mean, for [`Self::max_gpu_us`]'s reason and one
    /// more: the claim this field exists to guard is an upper bound ("work
    /// stays bounded by the viewport"), and a mean cannot state an upper
    /// bound. One frame that built the whole model is the entire defect, and
    /// it averages away.
    pub max_scene_nodes: u32,
    /// (R1538 §5.16) Largest [`FrameTiming::layout_nodes`] in this window —
    /// the most layout work any recent frame paid for, settle passes included.
    pub max_layout_nodes: u32,
    /// (R1538 §5.16) Largest [`FrameTiming::encode_nodes`] in this window —
    /// the deepest any recent frame's encode walk had to go.
    ///
    /// Peaks at the size of the painted tree on a frame the fragment cache
    /// could not serve (a resize, a theme change, the first paint), which is
    /// the honest worst case rather than a defect.
    pub max_encode_nodes: u32,
    /// (R1538 §5.16 §5.40) Largest [`FrameTiming::access_nodes`] in this
    /// window — the biggest AT tree any recent frame assembled.
    pub max_access_nodes: u32,
    /// (R1556 §5.16) Per-field maximum [`FrameTiming::draw`] over this window —
    /// the most drawing work any recent frame asked for, in each unit.
    ///
    /// Maxima for [`Self::max_scene_nodes`]' reason, and **not a frame**: each
    /// field is maxed on its own, so no single sample need have drawn all six
    /// (see [`DrawWork::max_of`]). The pairing that matters is this against
    /// [`Self::max_encode_nodes`] — the worst the CPU walked beside the worst
    /// the GPU was handed.
    pub max_draw: DrawWork,
    /// (R1537 §5.16) How many samples in this window carry a GPU timing.
    ///
    /// Published rather than implied because it is the *denominator* of
    /// [`Self::mean_gpu_us`], and a mean with an unstated sample count is
    /// a number nobody can weigh.
    pub gpu_sample_count: u32,
    /// (R1537 §5.16) Whether the backend that painted this window can time
    /// the GPU at all.
    ///
    /// **Filled by the backend after projection**, like [`Self::produce`] /
    /// [`Self::focus`] / [`Self::mirror`]: the ring holds frames, and a
    /// device capability is not one. `false` from the pure fold, which is
    /// correct for every consumer that never sets it — a TUI backend has
    /// no GPU to time.
    ///
    /// This is what makes an absent GPU reading *readable*. With only
    /// [`Self::gpu_sample_count`], `0` means both "this machine cannot
    /// measure" and "this window has not measured yet" — permanently
    /// impossible versus resolves-next-frame, which want opposite
    /// responses from a client.
    pub gpu_timing_supported: bool,
    /// (R1537 §5.16) GPU measurements the backend took and discarded,
    /// cumulative since boot. **Filled by the backend after projection**,
    /// like [`Self::gpu_timing_supported`].
    ///
    /// The third state a client needs. `gpu_timing_supported == true` with
    /// `gpu_sample_count == 0` reads as "the first sample is still in
    /// flight" — true for a healthy young window, and false forever on a
    /// host where every measurement fails. This separates them: a rising
    /// count here says the timer is running and throwing the results away.
    pub gpu_dropped_total: u64,
    /// `1e6 / mean_total_us`, `0.0` for a zero mean.
    pub mean_fps: f32,
    /// (R1460 §5.16 §2 #2) Cumulative work the RPC **scene producer** has done
    /// since boot — see [`ProduceWork`].
    ///
    /// `0` on a backend that does not produce (or has not yet).
    pub produce: ProduceWork,
    /// (R1464 §5.16 §5.39 §2 #2) Cumulative view work the focus enumeration has
    /// caused since boot — see [`FocusWork`].
    ///
    /// Binding-wide like the `produce` totals, and filled by the backend for
    /// the same reason: the ring holds frames, and a focus derivation is not
    /// one. All-zero on a backend that has never missed a focus request.
    pub focus: FocusWork,
    /// (R1465 §5.16 §5.12 §2 #7) Cumulative work the stored paint-scene
    /// **mirror** has done since boot — see [`MirrorWork`].
    ///
    /// All-zero on a backend that keeps no such mirror (`pinion-tui` paints one
    /// window and re-stores nothing), which is the honest reading there rather
    /// than a gap: the producer does not exist, so it has done no work.
    pub mirror: MirrorWork,
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
/// publish is **demand-gated** through [`ProviderSlot::get`]: it reads the holder
/// get-only (never creating it), so a window whose binding never called
/// [`use_frame_timings`] pays nothing. (R1366.8 reached around the type via
/// `.key()` for this; R1366.8.1's audit added the missing `get` leg.)
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
    fn r1366_8_1_a_child_scope_does_not_inherit_the_roots_frame_timings() {
        // Value-based discrimination the generated macro above cannot do — its
        // `ptr_eq` derives from `scope()`, so it passes under either verdict
        // (R1366.8.1 audit). Seed the ROOT's holder with a distinct sample; a
        // child scope must read an EMPTY view, not the root's — frame_timings is
        // per_scope (the publish is primary-only). If it were `inherited`, the
        // child would walk to root and see the sample, so this FAILS on a wrong
        // verdict.
        let root = Owner::new();
        super::FRAME_TIMINGS
            .resolve(&root)
            .publish(view_of(&[42_000], Some(16_666)));
        let child = Owner::new_child(&root);
        let seen = child.run(use_frame_timings);
        assert!(
            seen.samples.is_empty(),
            "a child scope inherited the root's frame history — the verdict must be per_scope",
        );
    }

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
    use super::{DrawWork, FRAME_TIMING_WINDOW, FrameTiming, FrameTimingStats};

    #[test]
    fn r1537_gpu_mean_is_over_timed_samples_not_over_frames() {
        // R1537 §5.16 — GPU timings are SPARSER than frames: each is read
        // back a frame or more after it was written, and a frame whose
        // predecessor has not been harvested yet is skipped rather than
        // blended. So the mean must divide by the number of samples that
        // carry a timing, not by the window length.
        //
        // This is the whole discriminating case: four frames, two timed at
        // 1000µs each. Over timed samples the mean is 1000 — the truth
        // about what the GPU costs when it runs. Over frames it would be
        // 500, which is not a smaller estimate of the same thing, it is an
        // answer to a different question ("GPU µs amortised per frame")
        // that nobody asked and that halves as the timer skips more.
        let mut stats = FrameTimingStats::new();
        stats.record(FrameTiming::new(1, 1, 1, 1, 10).with_gpu(Some(1000)));
        stats.record(FrameTiming::new(1, 1, 1, 1, 10));
        stats.record(FrameTiming::new(1, 1, 1, 1, 10).with_gpu(Some(1000)));
        stats.record(FrameTiming::new(1, 1, 1, 1, 10));
        let snap = stats.snapshot(None).expect("four samples");
        assert_eq!(snap.window_len, 4);
        assert_eq!(snap.gpu_sample_count, 2);
        assert_eq!(
            snap.mean_gpu_us,
            Some(1000),
            "mean over the TIMED samples; dividing by window_len would give 500",
        );
        assert_eq!(snap.max_gpu_us, Some(1000));
    }

    #[test]
    fn r1537_gpu_max_survives_a_mean_that_hides_it() {
        // The peer of `max_total_us`, and the reason it exists: one 40ms
        // frame among cheap ones IS the complaint, and the mean erases it.
        let mut stats = FrameTimingStats::new();
        for _ in 0..9 {
            stats.record(FrameTiming::new(1, 1, 1, 1, 10).with_gpu(Some(100)));
        }
        stats.record(FrameTiming::new(1, 1, 1, 1, 10).with_gpu(Some(40_000)));
        let snap = stats.snapshot(None).expect("ten samples");
        assert_eq!(snap.gpu_sample_count, 10);
        assert_eq!(snap.mean_gpu_us, Some(4090));
        assert_eq!(
            snap.max_gpu_us,
            Some(40_000),
            "the hitch must survive the fold that averages it away",
        );
    }

    #[test]
    fn r1537_untimed_window_reports_absence_not_zero() {
        // A host with no `TIMESTAMP_QUERY` adapter records real frames with
        // no GPU clock. Every GPU field must be ABSENT — a `Some(0)` would
        // assert the GPU did nothing, which reads as an excellent result,
        // and `gpu_sample_count` is what lets a reader tell "cannot
        // measure" from "has not measured yet".
        let mut stats = FrameTimingStats::new();
        stats.record(FrameTiming::new(300, 100, 0, 80, 540));
        let snap = stats.snapshot(None).expect("one sample");
        assert_eq!(snap.last.gpu_us, None);
        assert_eq!(snap.mean_gpu_us, None);
        assert_eq!(snap.max_gpu_us, None);
        assert_eq!(snap.gpu_sample_count, 0);
        // ...and a genuinely instant GPU frame is a DIFFERENT observation.
        let mut timed = FrameTimingStats::new();
        timed.record(FrameTiming::new(300, 100, 0, 80, 540).with_gpu(Some(0)));
        let tsnap = timed.snapshot(None).expect("one sample");
        assert_eq!(tsnap.mean_gpu_us, Some(0));
        assert_eq!(tsnap.gpu_sample_count, 1);
        assert_ne!(
            snap.mean_gpu_us, tsnap.mean_gpu_us,
            "`no measurement` and `measured zero` must not be the same value",
        );
    }

    #[test]
    fn r1537_gpu_is_not_a_phase_of_the_frame_it_rides_on() {
        // `gpu_us` is measured by a different clock, on a different device,
        // about a frame a step or two back. It must NOT enter the
        // `total >= build + encode + acquire + render` partition the wire
        // documents, or a client asserting that partition would break the
        // moment a GPU timing landed.
        let sample = FrameTiming::new(300, 100, 200, 80, 1000).with_gpu(Some(999_999));
        assert_eq!(sample.phase_sum_us(), 680);
        assert_eq!(sample.work_us(), 480);
        assert_eq!(sample.other_us(), 320);
        assert_eq!(
            sample.total_us, 1000,
            "an enormous GPU reading changes no CPU-side accounting",
        );
    }

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
    fn r1556_max_draw_is_per_field_and_is_not_any_one_frame() {
        // SIX frames, each the worst in exactly one unit, so the fold's answer
        // is pinned in every field it has. The first draft used three and
        // asserted three, and a counterfactual that folded `draws` with `min`
        // passed it — the fold can be wrong in half its fields while every
        // assertion holds, because the OTHER fields still make the composite
        // differ from any one sample.
        //
        // The expectation is a whole-struct literal rather than field
        // assertions, which is what makes the coverage structural: a field
        // added to `DrawWork` stops this compiling until it is folded here too.
        let samples = [
            DrawWork {
                draws: 700,
                ..DrawWork::default()
            },
            DrawWork {
                paths: 610,
                ..DrawWork::default()
            },
            DrawWork {
                path_segments: 40_000,
                ..DrawWork::default()
            },
            DrawWork {
                layers: 12,
                ..DrawWork::default()
            },
            DrawWork {
                glyph_runs: 88,
                ..DrawWork::default()
            },
            DrawWork {
                glyphs: 9_000,
                ..DrawWork::default()
            },
        ];
        let mut stats = FrameTimingStats::new();
        for draw in samples {
            stats.record(FrameTiming::new(1, 1, 0, 1, 10).with_draw_census(draw));
        }
        let snap = stats.snapshot(None).expect("six samples");
        assert_eq!(
            snap.max_draw,
            DrawWork {
                draws: 700,
                paths: 610,
                path_segments: 40_000,
                layers: 12,
                glyph_runs: 88,
                glyphs: 9_000,
            },
            "every field carries its own worst",
        );
        assert_eq!(
            snap.last.draw, samples[5],
            "`last` stays one real frame — only `max_draw` is the composite",
        );
        assert!(
            !samples.contains(&snap.max_draw),
            "and the composite is no frame that was painted, which is the point",
        );
    }

    #[test]
    fn r1556_a_bare_sample_draws_nothing_and_says_so() {
        // `FrameTiming::new` without `with_draw_census` — a hand-built fixture
        // and every non-vector backend. All-zero, and distinguishable from a
        // real vector frame, which has a root fill and therefore draws at least
        // one path. The `encode_nodes == 0` statement, one axis over.
        let bare = FrameTiming::new(1, 2, 3, 4, 20);
        assert_eq!(bare.draw, DrawWork::default());
        assert_eq!(bare.draw.glyphs, 0);
        let mut stats = FrameTimingStats::new();
        stats.record(bare);
        assert_eq!(stats.snapshot(None).unwrap().max_draw, DrawWork::default());
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
