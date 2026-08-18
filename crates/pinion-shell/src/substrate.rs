//! R51.92 §5.40 — substrate module split from `lib.rs`.
//!
//! Houses the framework-side dispatch substrate ([`ShellCore`]) and
//! the pure planning decision struct it returns
//! ([`AccessEmitDecision`]). Pre-R51.92 these types lived in `lib.rs`
//! alongside [`crate::AppShell`]; the visibility downgrade in R51.83
//! (14 `pub(crate)` → private fields + `core: ShellCore<V>` field on
//! `AppShell`) had no type-level effect because the surface code
//! shared the same module as the substrate.
//!
//! R51.92 promotes the encapsulation from a single-file convention
//! to an enforced module boundary: every `ShellCore` field is private
//! to this module, and `AppShell` (in `crate::root`) reaches the
//! substrate only through the accessor / dispatch methods declared
//! `pub` here.
//!
//! R51.123 §5.41 — the backend-agnostic dispatch core
//! ([`pinion_runtime::CoreShell<V>`]) lives behind a single
//! `core: CoreShell<V>` field. Every dispatch method on this struct
//! now reduces to "call `core.X` → log the returned
//! [`pinion_runtime::DispatchTail`] → bump backend-specific
//! bookkeeping (OCC revision, focus, redraw flag)". The four pieces
//! the lift moved out (`scene`, `cached_state`, `router`,
//! `intent_queue`) live inside `core`; this struct keeps only the
//! Vello-specific state (focus / modifiers / `text_cache` / previews
//! / revision / `last_access_*` / `redraw_requested`).
//!
//! See `substrate-incompleteness-signal` (the R51.29 → R51.30
//! refactor that birthed the shell) and `claim-accuracy-self-audit`
//! (the R51.80 → R51.83 → R51.92 lesson that "wrapper added" /
//! "visibility downgraded" / "module split" are three different
//! substantive depths of the same encapsulation claim).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use accesskit::NodeId;
use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, PinionAccessAction, ROOT_NODE_ID, tag_to_node_id,
    translate_action,
};
use pinion_core::GesturePhase;
use pinion_core::accelerator::Chord;
use pinion_core::event::WheelDelta;
use pinion_core::reactive::{FONT_SOURCES, FontSourceReport, SelfHostedFace};
use pinion_core::size_grant::SizeBounds;
use pinion_core::{Frame, Intent, KeyPress, Scene, SceneRevision};
use pinion_rpc::{
    DeferredInput, DispatchContext, DragButton, DragPhase, KeyWireState, LayoutNode, PreviewLedger,
    Request, dispatch_parsed, parse_request,
};
use pinion_runtime::text_engine::SelfHostedTextEngine;
use pinion_runtime::{
    CommandExecutor, CoreShell, DispatchTail, FocusManager, FrameTiming, FrameTimingStats,
    FrameTimingsSnapshot, IntentQueue, MirrorWork, Modifiers, PaintWork, PanRelease, PointerId,
    ProduceWork, SETTLE_PASS_BUDGET, TextMeasure, Touch, TouchPhase, clamp_frame_dt,
    compute_layout_with_text_measure, rect_for_tag, settle_to_fixed_point,
    walk_scene_and_drain_immediate,
};
use pinion_text::LayoutCache;

use super::WidgetView;

/// (R1125 §5.51 §2 #7 PR-33) Shell-owned tag for the cross-window dock drop-zone
/// preview slot. The shell wraps the binding-supplied overlay
/// ([`WidgetView::dock_drop_preview`]) in a container carrying this tag so the
/// strip is stripped + re-pushed by ONE known tag each paint (idempotent),
/// keeping the shell widget-library-agnostic (it never names a dock type).
const CROSS_WINDOW_DROP_PREVIEW_TAG: &str = "__xwin_drop_preview";

// R1150 §5.51 — the R1137 `REDOCK_DRAG_HINT_TAG` (the on-floater redock
// schematic) was removed: it placed the preview on the dragged floater's own
// rect, which under the R1146 release-only (static) floater sat at the wrong
// place. The on-target `CROSS_WINDOW_DROP_PREVIEW_TAG` preview is the affordance.

/// R889 §5.49 — every window-scoped read `dispatch_rpc_inner`
/// pre-resolves before its split-borrow block (the substrate borrows
/// preclude resolving these once `scene_mut` is taken). Bundled in a
/// named struct so the resolver
/// ([`ShellCore::window_scoped_rpc_reads`]) stays one call inside the
/// dispatch fn's 100-line clippy budget — the R888 extraction's
/// successor shape, grown a field per axis (R682.B cache stats / R885
/// input / R888 pacing / R889 unknown-window verdict / R907 frame
/// timings).
struct WindowScopedRpcReads {
    /// R682.B — `scene/cache_stats`; `None` → `CacheStatsUnavailable`
    /// (known window that has not painted, or embedder opt-out).
    cache_stats: Option<FragmentCacheStats>,
    /// R907 — `scene/frame_timings`; `None` →
    /// `FrameTimingsUnavailable` (window has not painted yet).
    frame_timings: Option<FrameTimingsSnapshot>,
    /// R1036 — `scene/render_fidelity`; `None` →
    /// `RenderFidelityUnavailable` (window has not painted yet).
    render_fidelity: Option<pinion_runtime::RenderFidelity>,
    /// R885 — `scene/input_state`; `None` → `InputStateUnavailable`.
    input_state: Option<pinion_core::InputStateSnapshot>,
    /// R888 — `scene/pacing_state`; `None` → `PacingStateUnavailable`.
    pacing_state: Option<pinion_rpc::PacingState>,
    /// R1549 — `scene/auto_repeat`; the in-flight press census. Not an
    /// `Option`: an empty list IS the answer for "nothing is held", so
    /// there is no unavailability to distinguish.
    auto_repeat_holds: Vec<pinion_runtime::AutoRepeatHold>,
    /// R1569 — `scene/accelerators`; every accelerator live in this window
    /// with the focused widget's claim resolved against it. Not an `Option`
    /// for the same reason as the sibling above: a window declaring no
    /// accelerator truthfully has an empty list.
    accelerators: Vec<pinion_runtime::AcceleratorRow>,
    /// R1569 — the focus those shadows were resolved against.
    accelerator_focus: Option<String>,
    /// R889 — `Some(id)` rejects the whole request with `-32602
    /// unknown_window` before method routing.
    unknown_window: Option<String>,
}

/// R1537 §5.16 — what a window's render backend can say about the GPU
/// clock, as of its last paint.
///
/// Two states, not a `bool` plus a counter: a backend with no clock has no
/// discarded measurements either, and the enum makes that pair
/// unrepresentable instead of merely conventional.
///
/// [`Self::Untimed`] is also the pre-paint default, and that is
/// unobservable rather than a claim: `frame_timings_for_window` returns
/// `None` until the ring holds a sample, and the sample and this field are
/// written by the same paint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum WindowGpuTiming {
    /// The backend cannot time the GPU (or has not painted yet).
    #[default]
    Untimed,
    /// The backend times the GPU. `dropped` measurements were taken and
    /// discarded since boot — a failed staging map, or a tick pair the
    /// driver reported out of order.
    Timed {
        /// Cumulative discarded measurements.
        dropped: u64,
    },
}

/// R51.76 §5.40 — framework-side dispatch substrate, decoupled from
/// winit / wgpu / `accesskit_winit`.
///
/// [`ShellCore`] owns every piece of state that `AppShell`'s dispatch
/// path mutates: the authoritative state scene, the cached state
/// projection, the §5.20 intent queue, the §5.34 preview ledger +
/// revision token, the §5.35 input router, the §5.39 focus manager
/// and cached winit modifier state, the §5.36 text layout cache, the
/// §5.12 last-paint snapshot, and the §5.40 incremental AT-emit
/// caches (tag map / node diff / focus diff / initial-emit flag).
///
/// The split is the textbook substrate/surface separation: the
/// shell-coupled bits (winit `Window`, wgpu surface, Vello renderer,
/// `accesskit_winit::Adapter`, `EventLoopProxy`) live in
/// [`crate::AppShell`]; everything else lives here so the dispatch
/// surface is reachable from headless tests without standing up a
/// winit `EventLoop` or a real wgpu device.
///
/// `request_redraw` no longer touches a `Window` directly — it sets
/// [`ShellCore::redraw_requested`] so [`crate::AppShell`] can drain
/// the flag once per event-loop iteration and forward to
/// `Window::request_redraw` when a `Window` exists, while headless
/// tests just observe the flag.
///
/// R51.92 §5.40 — moved from `lib.rs` to its own module so every
/// field below is genuinely private to `substrate.rs` and the
/// surface module (`AppShell` in `lib.rs`) can only reach the
/// dispatch path through the `pub` methods on this `impl` block.
/// (R1193 §5.16 §5.28 §5.39) The per-window shell-side state for ONE window,
/// keyed by canonical [`WindowSpec`](crate::WindowSpec) id in
/// [`ShellCore::window_states`]. R1193 consolidated 11 parallel
/// `*_per_window: HashMap<String, T>` fields (redraw / paint-clock / pacing /
/// profiling / chrome / focus) — which were inserted and, in
/// [`ShellCore::remove_window`], removed in LOCKSTEP per window — into one map of
/// this struct, so a window's teardown drops ONE entry. That retires the exact
/// R1123.1/R681 leak class `remove_window` documented ("drop the maximized /
/// immediate cache too, else a reused id inherits stale state"): a new per-window
/// axis is now a field HERE, cleared by the single entry removal, not another map
/// a future round must remember to add to the teardown. `AppShell` already holds
/// its per-window state in a `WindowSlot`; this makes `ShellCore` symmetric.
///
/// Presence-meaningful axes are `Option<T>` (absent ≡ "never set for THIS
/// window", distinct from a default value) so an entry created for one axis never
/// makes another read as present — the pre-R1193 independent-`get` semantics are
/// preserved field-by-field. Natural-default flags stay plain (`false` ≡ absent).
#[derive(Debug, Default)]
struct WindowState {
    /// R680 atomic 2 — pending per-window redraw request, drained by
    /// [`ShellCore::take_redraw_request_for_window`]. `false` ≡ absent.
    redraw_requested: bool,
    /// R51.143 / R680 atomic 1 — wall-clock of this window's previous paint;
    /// `None` (never painted) → the next paint feeds `dt = 0.0`.
    last_paint_instant: Option<Instant>,
    /// R681 atomic 3 — per-window target-fps override; `None` = default policy.
    target_fps: Option<u32>,
    /// R681 — did this window's last painted scene carry an immediate-mode
    /// subtree (a pacing input). `false` ≡ absent (retained-tree default).
    immediate_subtree: bool,
    /// R829 — pending injected immediate-mode `dt` (a `scene/tick`), consumed
    /// (`take`) by the next paint; `None` = live wall-clock advance.
    pending_immediate_dt: Option<f32>,
    /// R831 — fixed-timestep accumulator for the immediate-mode game loop;
    /// `None` until the window's first paint lazily inserts it.
    sim_accumulator: Option<pinion_runtime::FixedTimestep>,
    /// R682 atomic 3 — last published fragment-cache observability snapshot;
    /// `None` before the first publish (presence gates
    /// [`ShellCore::fragment_cache_stat_windows`]).
    fragment_cache_stats: Option<FragmentCacheStats>,
    /// R907 — rolling frame-timing profiler ring; `None` until the first
    /// recorded frame (the bootstrap `FrameTimingsUnavailable`).
    frame_timings: Option<FrameTimingStats>,
    /// R1537 — what THIS window's render backend last reported about the
    /// GPU clock.
    ///
    /// Per-window rather than binding-wide because the backend is: a
    /// second window on a second adapter can differ, and the wire answers
    /// per window.
    ///
    /// One field rather than a `bool` beside a counter, because they are
    /// one observation written by one paint — and because the counter is
    /// meaningless for a backend that has no clock, which the enum makes
    /// unrepresentable rather than merely unlikely.
    gpu_timing: WindowGpuTiming,
    /// R1708 — resizes the window system has delivered for THIS window but
    /// which no frame has answered yet, plus the lifetime accounting of what
    /// folding them cost.
    ///
    /// Homed here rather than on the surface's own window slot because it is
    /// the same class as [`Self::frame_timings`]: a fact about the window that
    /// the wire reads and more than one surface writes. A resize arrives at
    /// pointer rate and a frame does not, so the value that reconciles the two
    /// rates has to outlive both.
    resize_batch: pinion_core::resize_batch::ResizeBatch,
    /// R1036 — last presented-frame render-fidelity record (PR-17); `None`
    /// before the first present.
    render_fidelity: Option<pinion_runtime::RenderFidelity>,
    /// R1123 — winit-reported maximized state for the client-side chrome glyph +
    /// resize-border suppression. `false` ≡ absent (not maximized).
    maximized: bool,
    /// R25.1 — the focusable tags THIS window painted (its slice of the union
    /// Tab order); `None` ≡ never painted focusables (presence gates the
    /// [`ShellCore::union_focusable_tags`] window set).
    focusable_tags: Option<Vec<String>>,
    /// R1459 / R1538 — the work the last paint of THIS window did, as counts.
    /// Written by [`ShellCore::compute_paint_scene_internal`] and read back one
    /// step later by the surface, which owns the `Instant` spans and assembles
    /// the [`FrameTiming`] sample. All-zero before the first paint, which is
    /// the same "no sample yet" the timing ring reports.
    ///
    /// Stored rather than returned because the paint-scene producer's return
    /// type is the `Scene` itself — the one thing every caller wants — and
    /// widening it would make every call site carry a measurement it does not
    /// use. See [`PaintWork`] for why it is a struct and not a tuple.
    last_frame_work: PaintWork,
}

impl WindowState {
    /// R1193 — true when every axis is at its absent-default, so the entry
    /// carries no information and may be pruned. Keeps
    /// [`ShellCore::remove_window`]'s "carried an entry" return exact after
    /// [`ShellCore::clear_target_fps_for_window`] empties the last axis.
    ///
    /// For the three plain-`bool` axes, "empty" means `false` — which IS their
    /// absent state (a drained `redraw_requested`, a `false` `immediate_subtree`
    /// / `maximized` are observationally identical to never-set, since every
    /// reader defaults absent to `false`). The only place the pre-R1193 design
    /// distinguished a present-`false` key was `remove_window`'s return count,
    /// and the sole `clear_target_fps` caller re-creates the entry
    /// (`request_redraw`) before any `remove_window`, so the prune never sees a
    /// bool-`false`-only entry in practice.
    fn is_empty(&self) -> bool {
        !self.redraw_requested
            && self.last_paint_instant.is_none()
            && self.target_fps.is_none()
            && !self.immediate_subtree
            && self.pending_immediate_dt.is_none()
            && self.sim_accumulator.is_none()
            && self.fragment_cache_stats.is_none()
            && self.frame_timings.is_none()
            && self.render_fidelity.is_none()
            && !self.maximized
            && self.focusable_tags.is_none()
            && self.last_frame_work == PaintWork::default()
    }
}

pub struct ShellCore<V: WidgetView> {
    /// R51.123 §5.41 — backend-agnostic dispatch substrate.
    ///
    /// Owns the four pieces of state the lift moved out of this
    /// struct: `scene`, `cached_state`, `router`, `intent_queue`.
    /// Every dispatch method on `ShellCore` reduces to "call `core.X`
    /// for the SCXML / router work, then handle the returned
    /// [`DispatchTail`] (log intents + log state change + bump
    /// `redraw_requested` on visible-state transition)".
    ///
    /// The TUI sibling (`pinion_tui::ShellCoreTui`) composes the same
    /// [`CoreShell`] inner struct so both backends share scene +
    /// router + intent-queue handling without duplicating the
    /// dispatch arms (R51.123 / R51.124 / R51.125 4-round split).
    core: CoreShell<V>,
    /// §5.34 preview lifecycle ledger — passed into every
    /// `pinion_rpc::dispatch` call alongside the scene. Lifecycle RPC
    /// methods read or mutate it through interior mutability;
    /// non-lifecycle methods ignore it.
    ///
    /// R51.83 §5.40 — private. Plumbed into [`DispatchContext`] by
    /// [`Self::dispatch_rpc`] only.
    previews: PreviewLedger,
    /// §5.34 R40.4 OCC revision token. `dispatch` auto-bumps on
    /// mutating RPC methods; [`ShellCore::forward`] explicitly bumps
    /// after the winit-side `invoke` since that path bypasses the
    /// dispatcher entirely.
    ///
    /// R51.83 §5.40 — private. Read-only access via
    /// [`Self::revision`] (returns the current `u64`); mutation
    /// happens through the substrate dispatch methods only.
    ///
    /// R1270 §6.3 — an `Arc` so an external-data producer thread can bump the
    /// SAME single scene version token (resolved from the root `Owner` cache
    /// via [`resolve_scene_revision`](crate::waiter::resolve_scene_revision)),
    /// and so the boot-installed wake observer wakes parked async
    /// `scene/waitFor` waiters on every bump — one scene, one version.
    revision: Arc<SceneRevision>,
    /// R51.53 §5.39 framework-side focus state owner. Tab/Shift+Tab
    /// traverses [`FocusManager::tab_order`] (R1020: re-derived from the
    /// paint scene every frame via `Scene::collect_focusable_tags`); click
    /// on a tagged widget
    /// aliases [`FocusManager::focus_set`]; click on background
    /// aliases [`FocusManager::focus_clear`]. The shell consults the
    /// manager on every key dispatch so `apply_key` runs only when
    /// the widget's own tag is focused (eliminating the broadcast
    /// aliasing the pre-R51.53 design carried).
    ///
    /// R51.83 §5.40 — private. Read-only access via [`Self::focus`];
    /// mutation routed through the substrate's focus-handling
    /// methods (`handle_focus_traverse`, `click_to_focus`, …).
    focus: FocusManager,
    /// R51.53 §5.39 — abstract [`Modifiers`] cache. Refreshed by the
    /// winit-side `WindowEvent::ModifiersChanged` (converted to the
    /// `pinion_runtime::Modifiers` shape at the `app.rs` boundary);
    /// consulted on every `KeyboardInput` for Shift detection
    /// (Shift+Tab = `focus_prev`). winit emits `KeyEvent` without
    /// modifier state, so the shell has to track it out-of-band.
    ///
    /// R51.108 §5.41 — type lifted from `winit::keyboard::ModifiersState`
    /// to the substrate-local `pinion_runtime::Modifiers` so the
    /// dispatch path stays backend-agnostic for the TUI / mobile /
    /// RPC-driven input source the §2 #6 GUI/TUI dual invariant
    /// requires.
    ///
    /// R51.83 §5.40 — private. `AppShell::handle_key_press` reads
    /// only the Shift bit via [`Self::modifiers_shift_key`];
    /// mutation happens through [`Self::set_modifiers`].
    modifiers: Modifiers,
    /// R47.3 §5.36 — owned [`LayoutCache`] (LRU 256). `paint_adapter`'s
    /// Text arm consults this cache for every `Scene::Text` it walks
    /// so the view fn's static labels shape once on first paint and
    /// hit the cache on every subsequent frame. The cache also owns
    /// parley's `FontContext` / `LayoutContext` so the shell never
    /// holds parley state directly.
    ///
    /// R51.83 §5.40 — private. The vello-side paint pipeline reaches
    /// the cache through [`Self::text_cache_and_engine`]; substrate-internal
    /// callers (`compute_paint_scene`, `dispatch_rpc`'s producer
    /// closure) use the field directly.
    text_cache: LayoutCache,

    /// R1656 §5.32 — marks already reported by
    /// [`Self::warn_about_escaped_marks`], so a screen that spills the same
    /// label every frame says so once instead of sixty times a second. Keyed by
    /// owner + content + overhang, because a label that starts spilling FURTHER
    /// is a new fact and has to be said again.
    #[cfg(debug_assertions)]
    warned_escapes: HashSet<String>,

    /// R1656 §5.15 — per-window, per-`External` last announced size, so
    /// [`Self::announce_surface_sizes`] speaks only on a change.
    external_sizes: HashMap<String, pinion_runtime::ExternalSizes>,

    /// R1460 §5.16 §5.7 §2 #2 — cumulative work done by the RPC scene
    /// PRODUCER (see [`pinion_runtime::ProduceWork`]).
    ///
    /// The producer runs the same settle loop a paint does, but records no
    /// frame: a `scene/snapshot` must never manufacture a frame the user never
    /// saw. That contract left a real hole for the §2 #2 primary path — an
    /// agent driving pinion by RPC could not see the work its OWN calls
    /// caused, because by the time the window painted, the produce had already
    /// warmed every cache and settled every bound (`r1458_tail_reveal`'s append
    /// frame reads 1 pass / 0 misses for exactly this reason).
    ///
    /// Cumulative rather than last-produce, because one dispatch may produce
    /// several times (path resolution, then the post-dispatch finalize): the
    /// answerable question is "what did my call cost", and a client answers it
    /// by differencing across the call.
    produce_work: ProduceWork,
    /// R1465 §5.16 §5.12 §2 #7 — cumulative work done by the stored paint-scene
    /// MIRROR ([`Self::compute_paint_scene_pure_internal`]), the fourth
    /// producer of view runs and the last one that was neither settled nor
    /// counted.
    ///
    /// Plain `&mut`, not a [`pinion_runtime::FocusWorkCell`]-style accumulator:
    /// the one site that records it already holds `&mut self`, and interior
    /// mutability bought for nothing is a borrow rule that later rounds have to
    /// keep obeying.
    mirror_work: MirrorWork,
    /// R1464 §5.16 §5.39 §2 #2 — cumulative view work the FOCUS ENUMERATION has
    /// caused, the third producer of view runs beside the paint and the RPC
    /// scene producer.
    ///
    /// R1463 widened [`Self::refresh_focusable_from_view`] to re-derive every
    /// painted window and documented the cost as "one view run per painted
    /// window, paid on a miss" — a claim with no way to check it, on either
    /// side of the seam. This is the check: a test reads it directly, and §2 #2
    /// reads it off `scene/frame_timings`.
    ///
    /// A [`pinion_runtime::FocusWorkCell`] because the deriving call sites hold
    /// `&self` ([`Self::union_focusable_tags`] folds while borrowing
    /// `window_states`), and a counter must not dictate the borrow shape of the
    /// code it measures.
    focus_work: pinion_runtime::FocusWorkCell,
    /// R1072 §5.37 — opt-in self-hosted text engine, the shipping consumer of
    /// the §5.37 paint + measure arms. `Some` when the `PINION_TEXT_ENGINE`
    /// environment variable selected it at construction AND a usable system font
    /// was found; `None` (the default) keeps every text leaf on parley, so the
    /// shell paints and measures byte-identically to the pre-R1072 path.
    ///
    /// Threaded through BOTH arms together (the R1070.1 wire-both contract): the
    /// measure pass ([`compute_layout_with_text_measure`]) via
    /// [`text_measure_override`] and the production paint
    /// ([`pinion_runtime::paint_adapter::to_vello_cached_with_text_engine`]) via
    /// [`Self::text_engine`]. Eligible non-editable single-line text routes to
    /// §5.37; caret-bearing (editable) text stays on parley (shared eligibility
    /// SSOT). Built once at init — its parsed font is a per-process constant, so
    /// the [`FragmentCache`](pinion_runtime::paint_adapter::FragmentCache) stays
    /// coherent across frames.
    text_engine: Option<SelfHostedTextEngine>,
    /// R51.67 §5.40 — `NodeId` → widget tag map from the most recent
    /// `TreeUpdate`. Refreshed at the end of every `render` (when an
    /// adapter is attached). Consumed by `handle_action_request` so
    /// AT-side actions arriving via `AppEvent::AccessKit` resolve
    /// back to the widget tag without recomputing the tree.
    ///
    /// R51.83 §5.40 — private. Set inside [`Self::commit_access_emit`]
    /// and consumed inside [`Self::handle_action_request`].
    last_access_tag_map: HashMap<NodeId, String>,
    /// R51.72 §5.40 — previous frame's `AccessNode` set (keyed by
    /// `tag`). The next frame diffs against this to compute the
    /// dirty subset passed to `AccessTreeBuilder::dirty_tags`.
    /// AccessKit's incremental-update guidance: "an update should
    /// only include nodes that are new or changed".
    ///
    /// R51.83 §5.40 — private. Read by [`Self::plan_access_emit`],
    /// written by [`Self::commit_access_emit`].
    last_access_nodes: HashMap<String, AccessNode>,
    /// R51.72 §5.40 — `true` until the first `TreeUpdate` has been
    /// emitted (carrying the `Tree` metadata + every node). After
    /// that, subsequent emits set `initial(false)` and pass only
    /// the dirty subset.
    ///
    /// R51.83 §5.40 — private. Read by [`Self::plan_access_emit`],
    /// cleared by [`Self::commit_access_emit`].
    access_emit_initial: bool,
    /// R51.75 §5.40 — previous frame's `AccessFocus`. Compared
    /// alongside the dirty-node diff: when neither nodes nor focus
    /// changed, `update_if_active` is skipped entirely so a
    /// steady-state animation frame costs no AT-side traffic.
    ///
    /// R51.83 §5.40 — private. Read by [`Self::plan_access_emit`],
    /// written by [`Self::commit_access_emit`].
    last_access_focus: Option<AccessFocus>,
    /// R51.76 §5.40 — flag set whenever a method on
    /// [`ShellCore`] decides the next frame should repaint. Drained
    /// by [`crate::AppShell`] after each event-loop iteration and
    /// forwarded to `Window::request_redraw` when a winit `Window`
    /// is attached; remains observable for headless tests that never
    /// spin up a `Window`. The flag-based design replaces the
    /// pre-R51.76 direct `window.request_redraw()` call buried in
    /// every dispatch method, which made the substrate untestable
    /// without a real event loop.
    ///
    /// R51.83 §5.40 — private. Read-only access via
    /// [`Self::redraw_requested`]; drain via
    /// [`Self::take_redraw_request`]; mutation via
    /// [`Self::request_redraw`].
    redraw_requested: bool,
    /// R1576 §5.16 §5.41 §2 #7 — the monitors attached at the last stamp, or
    /// `None` when nobody has looked.
    ///
    /// Stamped by the winit surface ([`crate::AppShell`]) rather than read
    /// here, for the reason the whole crate is split this way: `ShellCore` is
    /// deliberately backend-agnostic (no winit, no wgpu), and the monitor list
    /// is the window system's. The same shape [`Self::set_live_window_origins`]
    /// already uses.
    ///
    /// `None` is DISTINCT from an empty topology: the first says the surface
    /// never stamped one (a TUI backend, a fixture, a shell before its first
    /// window), the second says somebody looked and there are no monitors. The
    /// wire keeps that distinction — `scene/displays` refuses with
    /// `DisplayTopologyUnavailable` for the first and answers with an empty
    /// list for the second.
    display_topology: RefCell<Option<pinion_core::display::DisplayTopology>>,

    /// R1610 §5.16 §2 #7 — the windowing system this process is actually talking
    /// to, stamped by [`crate::AppShell`] from the live event loop.
    ///
    /// The same seam and the same `None` distinction as
    /// [`Self::display_topology`]: `None` means nobody stamped one (a TUI
    /// backend, a fixture), which is why `scene/windows` reports a window's
    /// level outcome as absent rather than inventing a backend for it. A build
    /// target would not do — one Linux binary runs on X11 or Wayland, and those
    /// two differ on exactly this axis.
    windowing_backend: RefCell<Option<pinion_core::window_level::WindowingBackend>>,

    /// R1617 §5.16 §5.41 §2 #7 — per live window, the pair
    /// `scene/windows`' `display_home` is derived from: the window's **actual**
    /// outer rectangle in physical pixels, and the display the window system
    /// itself says the window is on (`None` when it named none).
    ///
    /// Two raw readings rather than a resolved answer, on purpose. The
    /// judgment — do they agree, and what is it called when they do not — is
    /// [`pinion_core::display::DisplayHome`], a pure function of an
    /// arrangement, and computing it here would move it into the one layer no
    /// test can construct a monitor farm for. Same seam and same `None`
    /// discipline as [`Self::display_topology`] beside it.
    ///
    /// A window absent from this map publishes `display_home: null`: nobody
    /// looked, so nothing is claimed.
    window_homes: RefCell<
        std::collections::HashMap<
            String,
            (
                pinion_core::display::DisplayRect,
                Option<pinion_core::display::DisplayId>,
            ),
        >,
    >,

    /// (R1193 §5.16 §5.28 §5.39) The per-window shell-side state store — one
    /// [`WindowState`] per window, keyed by canonical
    /// [`WindowSpec`](crate::WindowSpec) id. R1193 consolidated the 11
    /// formerly-parallel `*_per_window` maps (redraw / last-paint / target-fps /
    /// immediate-subtree / pending-dt / sim-accumulator / fragment-cache-stats /
    /// frame-timings / render-fidelity / maximized / focusable-tags) into this one
    /// map, so a window's teardown drops ONE entry (see [`WindowState`] for the
    /// leak class this retires and the per-field presence rationale). Read via
    /// [`Self::window_state`] (never creates an entry — preserves each axis's
    /// pre-R1193 `get()` "absent" semantics); written via
    /// [`Self::window_state_mut`] (the lazy `entry().or_default()` write path).
    /// Coexists with the binding-wide [`Self::redraw_requested`] fan-out flag
    /// above (the per-window redraw request is now `WindowState::redraw_requested`).
    window_states: HashMap<String, WindowState>,

    /// (R1125 §5.51 §2 #7 PR-33) The window a live cross-window drag currently
    /// targets (its incoming dock drop-zone preview is painted). Tracked so a move
    /// that changes the target (the cursor crosses from one window onto another, or
    /// leaves every window) can repaint the PREVIOUS target to clear its strip —
    /// the source window's drag moves do not otherwise dirty the target. `None`
    /// when no drag maps onto another window. Set in
    /// [`Self::cursor_moved_for_window`]; the strip itself is injected by
    /// [`WindowOverlayInputs::apply_cross_window_drop_preview`] from the same
    /// resolution.
    cross_preview_target: Option<String>,

    /// R763 §5.36 §5.22 — in-progress pointer-driven text selection.
    ///
    /// Set on a press inside a focused text field — the binding's
    /// [`WidgetView::position_caret_for_point`] hook returns the byte
    /// offset of the pinned selection anchor (the press point for a
    /// plain drag, the retained far end for a Shift-click). While the
    /// button stays held, every `cursor_moved` extends the selection
    /// from this anchor to the byte under the cursor through
    /// [`WidgetView::select_drag_to_point`]; the release clears it.
    /// `None` when no text drag is active.
    ///
    /// The anchor byte is opaque to the shell — the binding produced
    /// it and the binding consumes it; the shell only owns the
    /// press → move → release gesture lifecycle (the same role it
    /// plays for the [`pinion_runtime::InputRouter`] drag sessions and
    /// modifier cache). A mouse-single-pointer model (the canonical
    /// desktop text-selection case, mirror of the R719 positionless
    /// `PointerLeave` assumption); the `pid` / `window_id` are stored
    /// so a stray cross-window or multi-pointer move cannot extend the
    /// wrong field's selection.
    text_select_drag: Option<TextSelectDrag>,

    /// R1071 PR-27 §5.39 §5.16 §5.35 — the window that currently holds the
    /// OS keyboard focus, tracked from winit `WindowEvent::Focused`
    /// ([`Self::note_os_focus`]). `None` until the first focus event (single-
    /// window startup) and whenever the focused window blurs without a new
    /// window taking focus.
    ///
    /// The keyboard dispatch gate ([`Self::is_key_dispatch_window`]) consults
    /// it so a key press is acted on ONLY when it arrives at the OS-focused
    /// window — the fix for the multi-window toggle double-dispatch (sprag
    /// dock/undock): during an undock the newly-torn window grabs OS focus,
    /// and a stray re-delivery of the same press to the now-unfocused source
    /// window is dropped instead of toggling a second time. Single-window
    /// bindings always pass the gate (the one window is the focused one, and
    /// the pre-focus-event `None` fails OPEN), so this is byte-identical to
    /// the pre-R1071 global dispatch for them.
    ///
    /// Out-of-band shell state, like the [`Self::modifiers`] cache and the
    /// held-key chord cache — it mirrors a winit/OS fact the key event itself
    /// does not carry, never scene data (§2 #7 unaffected).
    os_focused_window: Option<String>,

    /// R1073 PR-27.4 §5.39 §5.16 §5.35 — per-key snapshot of the window that
    /// owned each in-flight physical press's rising edge: keyed by the canonical
    /// W3C *dispatch* key string (every key `AppShell::handle_key_press` acts
    /// on, INCLUDING the shell-reserved `Escape` / `Tab` — a superset of the
    /// R1009 content / chord vocabularies, resolved by `dispatch_named_key_str`),
    /// valued by the OS-focused window the press was first admitted at. Set on
    /// the admitted rising edge ([`Self::admit_key_press`]) and cleared on the
    /// key's release edge ([`Self::note_key_release`]) — a lifecycle owned by
    /// the gate, NOT piggybacked on the chord cache ([`Self::note_key_state`]),
    /// so it can cover `Escape` / `Tab` without polluting the RPC-exposed chord
    /// `held_names` subset.
    ///
    /// This is the *press-time focus snapshot* that closes the R1071 gate's
    /// blind spot (the sprag dock/undock double-toggle that survived R1071): a
    /// toggle-class press whose FIRST dispatch closes the focused window moves
    /// OS focus to the successor window, so a stray re-delivery of the SAME
    /// physical press to that NOW-focused window passed R1071's live-focus gate
    /// and fired a second time. Pinning the owner at the rising edge means
    /// every later delivery of the press is gated against the window it began
    /// on, not against the focus its own side-effect just moved — so a window
    /// closing mid-dispatch can no longer re-admit the press.
    ///
    /// Lifecycle is tied to the press (release-clear), NOT to focus: blur /
    /// [`CoreShell::clear_held_keys`](pinion_runtime::CoreShell::clear_held_keys)
    /// deliberately does NOT clear it, so the gate is robust to either winit
    /// focus-event ordering on the close-driven handoff. The release-clear is
    /// keyed by the key string alone (window-agnostic), so a keyup delivered to
    /// the successor window still clears an owner whose source window has been
    /// destroyed. A keyup that reaches no window at all (the browser
    /// missed-keyup convention) is the one residual that can strand an owner
    /// until that key is next pressed-and-released anywhere — the same trust
    /// the held-key cache already places in an eventual keyup.
    ///
    /// Out-of-band shell state alongside [`Self::os_focused_window`]; never
    /// scene data (§2 #7 unaffected).
    key_press_owner: HashMap<String, String>,
    /// (R1188 §5.16 §5.49 §2 #2) Window-control presses the RPC click drain
    /// detected — `(canonical spec id, control)` per hit, in arrival order.
    ///
    /// The RPC `scene/click` drain runs inside `ShellCore` (headless, no winit
    /// handles), while executing a control (`set_minimized` / `set_maximized` /
    /// the close seam + app-exit fallback) needs `AppShell`'s winit `Window` +
    /// `ActiveEventLoop`. So the drain QUEUES the detected control here and the
    /// windowed shell drains it right after `dispatch_rpc` returns, executing
    /// through the same `apply_window_control` the winit pointer path
    /// (`try_chrome_press`) uses — one detection vocabulary
    /// ([`pinion_overlay::window_control_for_tag`]) and one execution arm for
    /// both input paths. (The arm's full roster is its `ControlProducer` enum,
    /// R1364 — deliberately not recounted here, because five copies of that
    /// count drifted and three were wrong. The detection VOCABULARY is still
    /// shared by exactly the two TAG-driven paths this queue serves, which is a
    /// different and smaller claim than the roster.)
    /// Pre-R1188 the RPC click hit the control tag and then
    /// fell into ordinary widget routing (a no-op — no widget carries the
    /// overlay tag), so the R1121 "an AI observes AND DRIVES the controls"
    /// contract held only for observation. Headless tests observe this queue
    /// directly via [`Self::take_pending_window_controls`] (there is no winit
    /// arm to fire in a headless harness — the queue IS the routing decision).
    pending_window_controls: Vec<(String, pinion_overlay::WindowControl)>,

    /// R1364 §5.55 §2 #2 — an `app/quit` arrived and the windowed shell has not
    /// drained it yet.
    ///
    /// A `bool`, not a queue, because a quit is idempotent and one per process:
    /// two `app/quit` calls in one dispatch are one request to end, whereas two
    /// window controls are two distinct operations on possibly-distinct windows.
    /// The shape mirrors the verb — §5.55's whole claim is that app lifecycle is
    /// not a window operation, and the state that carries it should not pretend
    /// otherwise.
    pending_quit: bool,
}

/// R763 §5.36 §5.22 — shell-owned state of an active pointer text
/// selection drag (see [`ShellCore::text_select_drag`]).
struct TextSelectDrag {
    /// Window whose press started the drag; a `cursor_moved` on any
    /// other window is ignored for selection extension.
    window_id: String,
    /// Pointer that pressed; only this pointer's moves extend.
    pid: PointerId,
    /// Byte offset of the pinned selection anchor (binding-opaque).
    anchor: usize,
}

/// R682.B §5.16 — re-export of the GUI-agnostic
/// [`FragmentCacheStats`] snapshot that lives in
/// [`pinion_runtime::paint_cache_stats`] (non-vello-gated submodule,
/// so peer crates without GPU support can hold the type). Pre-R682.B
/// this struct was defined locally on `ShellCore` so the
/// `pinion-rpc` peer crate could not import it without depending on
/// `pinion-shell` (cyclic). The lift puts the stats type next to its
/// source contract in the runtime crate; the re-export here
/// preserves the existing `pinion_shell::FragmentCacheStats` import
/// path used by `tests/dispatch_core.rs` and out-of-tree consumers.
pub use pinion_runtime::FragmentCacheStats;

/// R51.77 §5.40 — pure decision returned by
/// [`ShellCore::plan_access_emit`].
///
/// Carries only the emit verdict + diff metadata: the should-emit
/// flag (`should_emit`), the initial-frame flag (`initial` — forces a
/// full tree metadata emit on the first frame), and the dirty-tag
/// set. Nodes / focus stay with the caller — the decision struct
/// borrows them while planning and lets the render path consume them
/// once for `Adapter::update_if_active` (no clone for the closure).
///
/// R51.77 split: pre-R51.77 `AccessEmitPlan` bundled the decision
/// AND the consumed nodes / focus AND mutated the `ShellCore` cache
/// inside a single combined emit call (silent surprise —
/// pure-looking name but mutating). The textbook canonical shape
/// separates pure planning from the cache-update commit step. See
/// [`ShellCore::commit_access_emit`].
#[derive(Debug)]
pub struct AccessEmitDecision {
    /// `true` when the caller should invoke
    /// `Adapter::update_if_active`. `false` when the tree is
    /// byte-identical to the previous frame's emit (no dirty nodes,
    /// no focus change, not initial).
    pub should_emit: bool,
    /// `true` for the first emit (carries `Tree` metadata + every
    /// node). Subsequent emits set `false` and pass only the dirty
    /// subset via `AccessTreeBuilder::dirty_tags`.
    pub initial: bool,
    /// Set of tags whose `AccessNode` body (name / value / state /
    /// bounds / children) changed since the previous emit. Empty
    /// when only focus changed. On `initial` the set contains every
    /// node's tag (the AT has no prior state).
    pub dirty: HashSet<String>,
}

impl<V: WidgetView> ShellCore<V> {
    /// R51.76 §5.40 — construct the dispatch substrate with a
    /// freshly-built state scene and the initial cached state read
    /// through the §5.15 introspect channel.
    ///
    /// Identical bootstrapping to the pre-R51.76 `AppShell::new` minus
    /// the winit / wgpu / AccessKit surface (which lives on
    /// [`crate::AppShell`] and is constructed lazily on `resumed`).
    /// Headless tests build only this struct.
    ///
    /// R51.123 §5.41 — the scene + cached-state bootstrapping moves
    /// into [`CoreShell::new`] (composed via `core: CoreShell::new()`
    /// below); this constructor adds the Vello-specific extras
    /// (focus seeding, AccessKit caches, redraw flag, OCC token, RPC
    /// preview ledger, parley `LayoutCache`).
    #[must_use]
    pub fn new() -> Self {
        Self::with_core(CoreShell::<V>::new())
    }

    /// R999 §5.23 / R1362 PR-65 §6.3 — [`Self::new`] with the backend's boundary
    /// handles seeded into the binding's root [`Owner`](pinion_core::Owner)
    /// before its factories run, so a binding can capture the live handles for an
    /// off-thread producer: the [`RepaintSink`](pinion_core::RepaintSink) via
    /// [`use_repaint_sink`](pinion_core::use_repaint_sink) (R999) and the
    /// [`WindowControlSink`](crate::WindowControlSink) via
    /// [`use_window_control_sink`](crate::use_window_control_sink) (R1362).
    /// Delegates the window to
    /// [`CoreShell::new_with_seed`](pinion_runtime::CoreShell::new_with_seed),
    /// which is what makes "before any read" structural — see its rustdoc for
    /// the silent-Null landmine a late seed would hit.
    ///
    /// Superseded `new_with_repaint_sink` (R999-R1361): a per-handle constructor
    /// could not seed a sink whose vocabulary lives ABOVE `pinion-runtime`, and
    /// grew one variant per boundary handle.
    #[must_use]
    pub fn new_with_seed(seed: impl FnOnce(&pinion_core::Owner)) -> Self {
        Self::with_core(CoreShell::<V>::new_with_seed(seed))
    }

    /// R1448 §5.36 — [`Self::new_with_seed`] plus the application's own fonts.
    ///
    /// `app_fonts` is the bytes of each face the application ships, in the
    /// order `ShellConfig::with_application_font` declared them. The toolkit's
    /// equivalent is `addApplicationFont` called from `main()`
    /// before any widget exists; the ordering constraint is the same and here
    /// it is structural rather than advisory.
    ///
    /// Registration happens **before** the root owner is seeded, because the
    /// resulting [`FontSourceReport`] is what gets seeded: the report names the
    /// families that just became selectable and the platform-scan verdict the
    /// registration probed. Doing it the other way round would seed a report
    /// describing a cache that did not exist yet.
    ///
    /// The fonts go into the **render** cache — the same `text_cache` the
    /// layout and paint passes borrow — so a family declared here is selectable
    /// by name from any `TextStyle` the binding emits. A face registered into
    /// some other cache would be reported as available and then fail to draw,
    /// which is the desync this constructor exists to make impossible.
    #[must_use]
    pub fn new_with_seed_and_fonts(
        app_fonts: Vec<Vec<u8>>,
        default_family: Option<pinion_core::style::FontFamily>,
        seed: impl FnOnce(&pinion_core::Owner),
    ) -> Self {
        let mut text_cache = LayoutCache::new();
        let mut application_families = Vec::new();
        for data in app_fonts {
            application_families.extend(text_cache.register_font_data(data));
        }
        // R1472 §5.36 — declaring the faces and choosing which one unset text
        // uses are the two halves of the toolkit's `addApplicationFont` +
        // `setFont`, and they land here together because the
        // second is meaningless before the first: a default naming a family
        // nobody registered resolves to the tofu fallback. Setting it on the
        // same `text_cache` the layout and paint passes borrow is what makes
        // measure and paint agree about it by construction.
        text_cache.set_default_font_family(default_family.clone());
        // Probe explicitly rather than reading whatever the registrations
        // happened to leave behind. With no declared font nothing above shapes,
        // so the status would be `NotProbed` — and it would STAY that way in the
        // published report while the first frame went on to learn the truth,
        // leaving a binding reporting "not-probed" on a host proven font-less.
        // A GUI shell pays this scan on its first `Scene::Text` regardless, so
        // this buys a report that is true from frame one for no real work.
        // R1479 §5.37 — the opt-in second shaper is built here, before the
        // report, and against the SAME default family the cache just took: what
        // "unset" resolves to has to mean one thing to both shapers, and the
        // face this arm holds is a font-source fact the report owes an agent.
        let text_engine = build_text_engine_from_env(default_family.clone());
        let report = FontSourceReport {
            system: text_cache.probe_system_fonts(),
            application_families,
            default_family,
            self_hosted: self_hosted_face(text_engine.as_ref()),
        };
        let core = CoreShell::<V>::new_with_seed(move |root_owner| {
            // R1448 §5.36 — the font-source fact rides the same one seeding
            // window as the backend's boundary handles, so a binding's first
            // `view` reads a real report rather than minting the `NotProbed`
            // default (`ProviderSlot::provide` panics on a late seed, which is
            // what makes "before any read" structural here).
            FONT_SOURCES.provide(root_owner, report);
            seed(root_owner);
        });
        Self::with_core_and_cache(core, text_cache, text_engine)
    }

    /// Shared constructor body — focus seeding + Vello-side substrate fields —
    /// over an already-built [`CoreShell`], so [`Self::new`] (seeds no backend
    /// handles) and [`Self::new_with_seed`] (the backend seeds its live sinks)
    /// differ only in how `core` was constructed.
    fn with_core(core: CoreShell<V>) -> Self {
        // No `ShellConfig` reached this path, so there is no application default
        // family for the §5.37 arm to resolve unset text against (R1479).
        Self::with_core_and_cache(core, LayoutCache::new(), build_text_engine_from_env(None))
    }

    /// R1448 §5.36 — [`Self::with_core`] over a caller-supplied render cache,
    /// so the fonts-aware constructor can hand in the cache it registered the
    /// application's faces into instead of this body minting an empty one.
    ///
    /// R1479 §5.37 — and over a caller-supplied `text_engine`, for the same
    /// reason one step further: the fonts-aware constructor has to build the arm
    /// BEFORE it seeds the [`FontSourceReport`], because the face the arm holds
    /// is one of the facts that report carries. Building it here would publish a
    /// report describing an arm that did not exist yet.
    fn with_core_and_cache(
        core: CoreShell<V>,
        text_cache: LayoutCache,
        text_engine: Option<SelfHostedTextEngine>,
    ) -> Self {
        // Log the initial state read through the §5.15 introspect
        // channel — same trace line shape AppShell relied on
        // pre-R51.123 so the dogfood eprintln + RPC-side observer
        // both see the boot-time state.
        eprintln!(
            "shell: initial state = {}",
            V::fmt_state_log(core.cached_state()),
        );
        // R1270 §6.3 — resolve the ONE shared scene revision (an external-data
        // producer resolves the same `Arc` to bump it on arrival) before `core`
        // moves into the struct.
        let revision = crate::waiter::resolve_scene_revision(core.root_owner());
        let mut shell = Self {
            core,
            previews: PreviewLedger::default(),
            revision,
            focus: FocusManager::new(),
            modifiers: Modifiers::empty(),
            text_cache,
            #[cfg(debug_assertions)]
            warned_escapes: HashSet::new(),
            external_sizes: HashMap::new(),
            produce_work: ProduceWork::default(),
            mirror_work: MirrorWork::default(),
            focus_work: pinion_runtime::FocusWorkCell::default(),
            text_engine,
            last_access_tag_map: HashMap::new(),
            last_access_nodes: HashMap::new(),
            access_emit_initial: true,
            last_access_focus: None,
            redraw_requested: false,
            display_topology: RefCell::new(None),
            windowing_backend: RefCell::new(None),
            window_homes: RefCell::new(HashMap::new()),
            window_states: HashMap::new(),
            cross_preview_target: None,
            text_select_drag: None,
            os_focused_window: None,
            key_press_owner: HashMap::new(),
            pending_window_controls: Vec::new(),
            pending_quit: false,
        };
        // (R1020 §5.39) Seed the focus enumeration from the binding's first
        // view — the scene-derived source the per-frame paint refresh uses —
        // so an RPC `focus/set` or the first Tab resolves a current
        // enumeration before the first live paint. (Replaces the pre-R1020
        // `V::focusable_tags()` boot seed; the method is retired.)
        // R1270 §6.3 — install the async `scene/waitFor` wake seam on the ONE
        // scene revision (install-once): every future bump — a dispatched
        // mutation, shell input, or an external-data producer's arrival bump —
        // wakes parked waiters through this one observer, so the registry needs
        // no version counter of its own.
        // R1552 §5.7 PINION-PR83 — the SAME observer also publishes to the
        // process's change-stream subscribers. It has to be this one closure:
        // `set_observer` is install-once by design, so a second wake source
        // cannot be added beside it — and that is the right shape anyway,
        // because both are answers to one question ("the scene reached
        // revision N"), differing only in whether the client asked once or
        // asked to keep being told.
        let waiters = crate::waiter::resolve_waiter_registry(shell.core.root_owner());
        shell.revision.set_observer(move |new| {
            waiters.wake(new);
            pinion_rpc::process_registry().publish(new);
        });
        // R1335 §5.39 (PR-53) — hand the focus manager this binding's root owner
        // so its `commit_focus` funnel publishes the focused tag into the owner
        // mirror a binding reads (`pinion_core::focus_state::focused`). Attach
        // before the first `refresh_focusable_from_view` below, which can drop
        // (and therefore publish) focus.
        let root_owner = shell.core.root_owner().clone();
        shell.focus.attach_owner(root_owner);
        shell.refresh_focusable_from_view();
        shell
    }

    /// R51.76 §5.40 — borrow the focus manager. Both tests and the
    /// vello-side paint pipeline reach the focused tag through this
    /// accessor. R51.83 §5.40: substrate-internal callers use the
    /// field directly; the surface boundary forbids it (the field
    /// itself is private).
    #[must_use]
    pub fn focus(&self) -> &FocusManager {
        &self.focus
    }

    /// R1025 §5.35 — read the pointer's hover target on the addressed
    /// window (passthrough to [`pinion_runtime::CoreShell::hover_target_for_window`]).
    ///
    /// A read-only diagnostic accessor, the pointer-axis sibling of
    /// [`Self::focus`] and [`Self::redraw_requested_for_window`]: after a
    /// binding drives `cursor_moved`, it can assert what the cursor
    /// resolved to (the deepest tagged paint node) — grounding
    /// pointer-driven interaction tests in data, not pixels (§7 / the
    /// AI-first introspection posture). `None` = the window has no router
    /// yet, or the cursor is over no tagged node.
    #[must_use]
    pub fn hover_target_for_window(&self, window_id: &str, pid: PointerId) -> Option<&str> {
        self.core.hover_target_for_window(window_id, pid)
    }

    /// (R1701 §5.16 §5.49) Where the pointer is in the addressed window, in
    /// logical pixels — the position leg of [`Self::hover_target_for_window`]'s
    /// tag leg.
    ///
    /// The client-side chrome's double-click detector reads it: a title bar's
    /// press is consumed before the widget router runs, so the shell resolves
    /// the ordinal itself and has to measure it against the SAME position the
    /// hit test above resolved with.
    #[must_use]
    pub fn cursor_position_for_window(
        &self,
        window_id: &str,
        pid: PointerId,
    ) -> Option<(f64, f64)> {
        self.core.cursor_position_for_window(window_id, pid)
    }

    /// R1025 §5.35 — single-window [`Self::hover_target_for_window`]
    /// (the [`pinion_runtime::DEFAULT_WINDOW`] router), symmetric with the
    /// single-window [`Self::cursor_moved`] / [`Self::mouse_pressed`] drivers.
    #[must_use]
    pub fn hover_target(&self, pid: PointerId) -> Option<&str> {
        self.core.hover_target(pid)
    }

    /// (R1196 §5.16 §5.39) Per-window read of the hover [`CursorHint`](pinion_core::style::CursorHint) the
    /// deepest hinted node under the pointer requests — the cursor-axis sibling
    /// of [`Self::hover_target_for_window`], grounding cursor-affordance tests in
    /// data (the resolved hint) rather than a live winit cursor. `None` when the
    /// pointer is over no hinted region.
    #[must_use]
    pub fn cursor_hint_for_window(
        &self,
        window_id: &str,
        pid: PointerId,
    ) -> Option<pinion_core::style::CursorHint> {
        self.core.cursor_hint_for_window(window_id, pid)
    }

    /// (R1196 §5.16 §5.39) Single-window [`Self::cursor_hint_for_window`] (the
    /// [`pinion_runtime::DEFAULT_WINDOW`] router), symmetric with
    /// [`Self::hover_target`].
    #[must_use]
    pub fn cursor_hint(&self, pid: PointerId) -> Option<pinion_core::style::CursorHint> {
        self.core
            .cursor_hint_for_window(pinion_runtime::DEFAULT_WINDOW, pid)
    }

    /// (R1188 §5.16 §5.49 §2 #2) Drain the window-control presses the RPC
    /// click drain detected this dispatch — `(canonical spec id, control)` in
    /// arrival order; the queue is left empty.
    ///
    /// The windowed shell calls this right after `dispatch_rpc` returns and
    /// executes each entry through the same arm the winit pointer path uses
    /// (`AppShell::apply_window_control` — `set_minimized` / `set_maximized` /
    /// the [`WidgetView::window_close_requested`]
    /// close seam), so an RPC `scene/click` on a control tag and a physical
    /// left-press on the same tag take one execution path. Headless harnesses
    /// (no winit) assert the returned entries directly: the queue IS the
    /// routing decision, and the winit execution arm is the thin
    /// already-covered remainder.
    #[must_use]
    pub fn take_pending_window_controls(&mut self) -> Vec<(String, pinion_overlay::WindowControl)> {
        std::mem::take(&mut self.pending_window_controls)
    }

    /// R1364 §5.55 §2 #2 — take the pending `app/quit`, if one arrived.
    ///
    /// The peer of [`Self::take_pending_window_controls`] for app lifecycle. The
    /// windowed shell calls it after writing the RPC response and routes a `true`
    /// into `AppShell::request_quit`, so the AI's quit passes
    /// [`WidgetCore::app_quit_requested`](pinion_core::WidgetCore::app_quit_requested)
    /// exactly as `Escape` and a binding's own `QuitSink` do.
    ///
    /// Headless harnesses (no winit) assert this directly: the flag IS the
    /// routing decision, and the winit execution arm is the thin already-covered
    /// remainder — the same argument `take_pending_window_controls` makes.
    #[must_use]
    pub fn take_pending_quit(&mut self) -> bool {
        std::mem::take(&mut self.pending_quit)
    }

    /// R1025 §5.35 — read the pointer-capture lock on the addressed window
    /// (passthrough to [`pinion_runtime::CoreShell::captured_target_for_window`]).
    ///
    /// Returns the tag of the widget that captured `pid` between its press
    /// and release — a `wants_pointer_capture` External (splitter, slider,
    /// pan canvas). The read sibling of [`Self::hover_target_for_window`];
    /// lets a drag test assert that a press actually engaged capture (vs a
    /// missed hit-target or an un-capturing widget) without pixels. `None`
    /// = no widget holds `pid` on that window.
    #[must_use]
    pub fn captured_target_for_window(&self, window_id: &str, pid: PointerId) -> Option<&str> {
        self.core.captured_target_for_window(window_id, pid)
    }

    /// R1025 §5.35 — single-window [`Self::captured_target_for_window`]
    /// (the [`pinion_runtime::DEFAULT_WINDOW`] router), symmetric with the
    /// single-window [`Self::mouse_pressed`] / [`Self::mouse_released`] drivers.
    #[must_use]
    pub fn captured_target(&self, pid: PointerId) -> Option<&str> {
        self.core.captured_target(pid)
    }
}

/// `ShellCore::new()` is the canonical constructor; the
/// `Default` impl exists so the substrate composes with any
/// future builder that defaults a member field via
/// [`Default::default`] (R51.76 — workspace lints set
/// `clippy::pedantic = "deny"`, which promotes
/// `clippy::new_without_default` to a hard build error; this
/// impl is mandatory to satisfy the lint without weakening
/// the baseline).
impl<V: WidgetView> Default for ShellCore<V> {
    fn default() -> Self {
        Self::new()
    }
}

/// R1072 §5.37 — build the opt-in self-hosted text engine when the
/// `PINION_TEXT_ENGINE` environment variable selects it at construction.
///
/// `Some` only when the variable is set to a truthy value (`1` / `true` / `on` /
/// `yes`, case-insensitive) AND a usable system font is found
/// ([`SelfHostedTextEngine::from_system_font`]). Any other case — unset, falsey,
/// or no installed font — returns `None`, so the shell stays on parley (the
/// 0-regression default). A font-load failure is logged, never fatal.
///
/// R1479 §5.37 — `default_family` is the same value the render cache was given
/// (`LayoutCache::set_default_font_family`), so both shapers resolve an unset
/// `TextStyle::font_family` against one declaration. Without it the arm treated
/// every unstyled leaf as its own, silently overriding the application's
/// declared face for exactly the text R1472 exists to serve.
fn build_text_engine_from_env(
    default_family: Option<pinion_core::style::FontFamily>,
) -> Option<SelfHostedTextEngine> {
    let requested = std::env::var("PINION_TEXT_ENGINE").is_ok_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        )
    });
    if !requested {
        return None;
    }
    match SelfHostedTextEngine::from_system_font() {
        Ok(engine) => Some(engine.with_default_family(default_family)),
        Err(e) => {
            tracing::warn!(target: "pinion::shell", error = %e, "PINION_TEXT_ENGINE set but no usable system font");
            None
        }
    }
}

/// R1479 §5.37 — the arm's face as the fact the [`FontSourceReport`] publishes.
///
/// A face with no `name`-table family is [`SelfHostedFace::Unnamed`] rather than
/// absent: the arm IS running over it — which shows in the geometry of unset
/// text — it simply cannot prove it serves any named request. Collapsing that
/// into `Disabled` would report a shaper that is drawing as one that is not.
fn self_hosted_face(engine: Option<&SelfHostedTextEngine>) -> SelfHostedFace {
    engine.map_or(SelfHostedFace::Disabled, |e| {
        e.served_family()
            .map_or(SelfHostedFace::Unnamed, SelfHostedFace::Serving)
    })
}

/// R1072 §5.37 — the [`TextMeasure`] override for the layout pass, or `None` when
/// the self-hosted engine is disabled.
///
/// A free fn over a borrowed engine option (not a `&self` method) so the measure
/// call can hold the override alongside a disjoint `&mut self.text_cache` borrow —
/// a `&self` method would borrow all of `self` and conflict with the cache.
/// Callers pass `self.text_engine.as_ref()`.
fn text_measure_override(engine: Option<&SelfHostedTextEngine>) -> Option<&dyn TextMeasure> {
    engine.map(|e| e as &dyn TextMeasure)
}

/// R705 §5.39 §5.40 — resolve the focus-ring target tag (SSOT).
///
/// A roving widget (`RadioGroup`, the datepicker grid, listbox) keeps
/// shell focus on its CONTAINER tag while the *visible* focus moves to
/// an aria-activedescendant cell. Resolve through the binding's
/// `access_focus_target` — the same resolution the a11y tree lowers to
/// `set_active_descendant` — so the focus ring frames the active cell
/// (`datepicker#15`). Atomic widgets report no active descendant and
/// resolve to the focused tag unchanged.
///
/// Single source of truth for BOTH paint-scene producers — the winit
/// path (the shared [`WindowOverlayInputs::apply`] chain) and the RPC
/// `scene/snapshot from: paint` produce closure. Keeping the two on one
/// resolver is mandatory: when they diverged during R705 development
/// the live window framed the active day while `scene/snapshot` still
/// reported the container ring, defeating the very introspection the
/// ring exists to serve ([[r670b-paint-scene-producer-parity]]).
fn resolve_focus_ring_tag<V: WidgetView>(
    state: &V::State,
    focused: &str,
    owner: &pinion_core::Owner,
) -> String {
    owner
        .run(|| V::access_focus_target(state, Some(focused)))
        .and_then(|af| af.active_descendant)
        .unwrap_or_else(|| focused.to_owned())
}

/// R1010 §5.39 §5.40 — inject the framework focus ring around `ring_tag`, styled
/// (or suppressed) by the binding's [`WidgetView::focus_ring_style`]. The single
/// home shared by the winit paint path ([`WindowOverlayInputs::apply`]) and the
/// RPC produce path: `None` from the hook draws no ring (the content-surface
/// opt-out), `Some(style)` draws it; a `None` `ring_tag` (no focus) is also a
/// no-op.
fn inject_styled_focus_ring<V: WidgetView>(
    scene: Scene,
    ring_tag: Option<&str>,
    viewport: Option<(u32, u32)>,
) -> Scene {
    match ring_tag {
        Some(tag) => match V::focus_ring_style(tag) {
            // R1022 §5.39 — thread the layout viewport extent so the ring's far
            // edges clamp on-screen for a window-flush widget. The shell always
            // knows the size, so it is `Some`; `None` is the overlay crate's
            // headless / pure-geometry path.
            Some(style) => pinion_overlay::inject_focus_ring(scene, Some(tag), style, viewport),
            None => scene,
        },
        None => scene,
    }
}

/// (R1121 §5.16 §5.21) Inset the window content below a client-side chrome
/// strip of `chrome_h` logical pixels. A borderless window (`decorations:
/// false`) draws its own chrome (title bar + controls) via
/// [`pinion_overlay::inject_window_chrome`]; that strip occupies the top
/// `chrome_h` px, so the content must be laid out below it.
///
/// Wraps `content` in a `Column` flex container `[spacer(chrome_h),
/// content(flex-fill)]`: the spacer reserves the strip's height and the
/// content flex-fills the remainder, so the layout engine places the
/// content in `(0, chrome_h, w, h - chrome_h)`. The shell then injects the
/// chrome overlay onto the reserved strip post-layout. Applied identically
/// on the live paint path and the side-effect-free introspection mirror so
/// `scene/snapshot` matches the painted geometry (§2 #7).
///
/// The content's own root layout gets the `view_splitter` flex-main idiom
/// (`flex-basis: 0; flex-grow: 1; min-<main>: 0`, R1086) so a large content
/// child shrinks into the inset region instead of overflowing. This mirrors
/// `view_splitter::apply_flex_main` (now `pub(crate)`, reused by the dock
/// walker's R1205 surface wrapper as its 2nd in-crate consumer); the shell
/// keeps a local vertical-only copy because it is a separate crate. A
/// cross-crate lift to `pinion-core` is the clean consolidation once it earns
/// the churn — deferred (R1121/R1205 carry).
fn chrome_inset_wrap(content: Scene, chrome_h: u32) -> Scene {
    use pinion_core::scene::{BoxNode, ContainerNode, Rect};
    use pinion_core::style::{AlignItems, BoxStyle, Color, FlexDirection, LayoutStyle, SizeValue};
    let spacer = Scene::Box(
        BoxNode::new(Rect::new(0, 0, 0, 0), BoxStyle::filled(Color::TRANSPARENT)).with_layout(
            LayoutStyle::new()
                .with_flex_basis(SizeValue::Px(chrome_h))
                .with_flex_grow(0.0),
        ),
    );
    let filled = set_scene_flex_fill_vertical(content);
    Scene::Container(
        ContainerNode::new(vec![spacer, filled]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Stretch),
        ),
    )
}

/// (R1121) Inset `scene` below a chrome strip iff `chrome_h` is `Some` (a
/// borderless window). The one-line guard shared by every layout pass so
/// the hot paint fn stays under the line cap.
fn apply_chrome_inset(scene: Scene, chrome_h: Option<u32>) -> Scene {
    match chrome_h {
        Some(h) => chrome_inset_wrap(scene, h),
        None => scene,
    }
}

/// (R1467 §5.16 §5.21) Produce one window's view scene: run the per-window view
/// fn under the binding's reactive `owner`, then inset it below any client-side
/// chrome strip.
///
/// The step every paint-scene producer starts its settle loop from, and — since
/// R1467 put the RPC produce closure on the same footing as the winit paint and
/// the introspection mirror — the third byte-similar copy of it, so it lifts
/// (R727 / R732). Mechanical wiring with no opinion in it: the owner wrap is
/// R51.146's (`Owner::current()` must resolve inside `V::view`), the
/// `Some`/`None` split is R670.B's per-window dispatch, and the inset is
/// R1121's. Keeping them in one place is what makes "all three producers lay a
/// chromed window out the same way" a property of the code rather than of three
/// call sites agreeing.
fn window_view<V: WidgetView>(
    owner: &pinion_core::Owner,
    window_id: Option<&str>,
    state: V::State,
    frame: Frame,
    chrome_h: Option<u32>,
) -> Scene {
    apply_chrome_inset(
        owner.run(|| {
            let scene = match window_id {
                Some(id) => V::view_for_window(id, state, &frame),
                None => V::view(state, &frame),
            };
            // ★★★★★ R1714 — the pan a binding's policy declares, applied here
            // because here is where all three paint-scene producers already
            // agree. A binding that declares no pan gets the very same `Scene`
            // back, so this costs the 224 bindings that do not one comparison.
            //
            // Inside the owner wrap for the reason everything else here is: the
            // window size is a tracked read, so a resize re-runs the view and
            // the pan's viewport follows it. Before the chrome inset, because
            // the pan is a viewport onto the SCREEN and the chrome strip is
            // window furniture the shell adds around it.
            pinion_core::shrink::pan(
                V::shrink_policy(),
                V::tag(),
                pinion_core::reactive::use_viewport_size(),
                scene,
            )
        }),
        chrome_h,
    )
}

/// (R1121 §5.21) Apply the vertical flex-main idiom (`flex-basis: 0;
/// flex-grow: 1; min-height: 0`) to a `Scene`'s own root layout so it fills
/// — and can shrink within — a `Column` flex parent's main axis. Mirrors
/// `view_splitter::apply_flex_main` for the [`chrome_inset_wrap`] consumer:
/// every variant carrying a `layout` field is mutated in place; `Scroll` /
/// `Effect` (no `layout` field) auto-wrap in a thin flex Container.
fn set_scene_flex_fill_vertical(scene: Scene) -> Scene {
    use pinion_core::scene::ContainerNode;
    use pinion_core::style::{LayoutStyle, Size, SizeValue};
    fn fill(layout: LayoutStyle) -> LayoutStyle {
        layout
            .with_flex_basis(SizeValue::Px(0))
            .with_flex_grow(1.0)
            .with_min_size(Size::auto().with_height(SizeValue::Px(0)))
    }
    match scene {
        Scene::Container(mut c) => {
            c.layout = fill(c.layout);
            Scene::Container(c)
        }
        Scene::Box(mut b) => {
            b.layout = fill(b.layout);
            Scene::Box(b)
        }
        Scene::Text(mut t) => {
            t.layout = fill(t.layout);
            Scene::Text(t)
        }
        Scene::Image(mut i) => {
            i.layout = fill(i.layout);
            Scene::Image(i)
        }
        Scene::External(mut e) => {
            e.layout = fill(e.layout);
            Scene::External(e)
        }
        Scene::ImmediateModeNode(mut im) => {
            im.layout = fill(im.layout);
            Scene::ImmediateModeNode(im)
        }
        // `Scroll` / `Effect` (and any future `#[non_exhaustive]` variant
        // without a `layout` field) wrap in a thin flex Container carrying
        // the same props; no `BoxStyle` so the visual shape is unchanged.
        other => {
            Scene::Container(ContainerNode::new(vec![other]).with_layout(fill(LayoutStyle::new())))
        }
    }
}

/// (R1467 §5.16 §5.39 §2 #7) The shell-owned state the window-overlay chain
/// reads, sampled once so every paint-scene producer can dress a window the
/// same way.
///
/// Three producers exist: the winit paint
/// ([`ShellCore::compute_paint_scene_internal`]), the introspection mirror
/// ([`ShellCore::compute_paint_scene_pure_internal`]), and the RPC dispatch's
/// produce closure. §2 #7 holds only if all three dress the window identically.
/// The first two take `&self` and can call the chain as methods; the third runs
/// inside a closure that has already split `&mut self` into disjoint field
/// borrows, so it can call NO `&self` method. Pre-R1467 that borrow shape is
/// why the closure grew its own two-step chain (focus ring + drag image) and
/// skipped the other four steps — measured on a chromed window, its scene had
/// no strip, no resize border and, because the content inset rides the same
/// policy read, no inset either. Every coordinate the produce path derived for
/// such a window therefore sat [`WindowChromeStyle::height_px`](crate::WindowChromeStyle::height_px)
/// too high, and
/// `scene/click {path}` resolved through it lands that far above the widget it
/// named — on the chrome strip, for anything near the top.
///
/// Sampling the reads into a value the closure can own is what lets ONE chain
/// ([`Self::apply`]) serve all three, the same hoist `ring_tag` and the drag
/// sample already used to cross the borrow split.
struct WindowOverlayInputs {
    /// The binding's policy for the resolved window spec (chrome style +
    /// resize override); `Default` when the id resolves to no declared spec.
    policy: crate::WindowPolicy,
    /// Declared title of the resolved spec, for the chrome strip's label.
    /// `None` when the id resolves to no spec (nothing to dress).
    title: Option<String>,
    /// Whether the OS reports this window maximized (restore glyph + the
    /// resize-border suppression).
    maximized: bool,
    /// Focus-ring target, already resolved through `access_focus_target`
    /// ([`resolve_focus_ring_tag`]). `None` = nothing focused = no ring.
    ring_tag: Option<String>,
    /// An incoming cross-window dock drop that targets this window.
    cross_window_drop: Option<(String, pinion_runtime::CrossWindowDrop)>,
    /// Active drag label + window-logical cursor for the drag-image chip.
    drag_image: Option<(String, (f64, f64))>,
    /// The desktop drag-preview window is already showing this drag, so the
    /// in-window chip is suppressed (exactly one chip).
    desktop_drag_preview_active: bool,
}

impl WindowOverlayInputs {
    /// (R1121 §5.16 §5.21) Logical-pixel height the window content is inset by
    /// to clear the client-side chrome strip, or `None` for a window with no
    /// chrome. Read from the SAME policy sample the strip injection uses, so a
    /// producer cannot inset without dressing (or dress without insetting).
    fn chrome_inset(&self) -> Option<u32> {
        self.policy.chrome.map(|style| style.height_px)
    }

    /// (R1113 §5.51 §5.33) The window-level paint overlays, in z-order: resize
    /// border, chrome strip, raised top resize edge, keyboard focus ring,
    /// cross-window drop preview, drag-image follower. Applied as the final step
    /// of every paint-scene producer.
    fn apply<V: WidgetView>(
        &self,
        scene: Scene,
        w: u32,
        h: u32,
        root_owner: &pinion_core::Owner,
    ) -> Scene {
        // R1122 §5.16 §5.39 — resize border UNDER the chrome strip: a chromed
        // borderless window has no OS frame, so client-side edge / corner
        // regions restore drag-resize. Injected first so the chrome strip
        // (next) layers on top of the north edge / top corners and keeps its
        // controls clickable. No-op for a window without client-side chrome.
        let resizable = self.apply_resize_border(scene, w, h);
        // R1121 §5.16 §5.39 PR-38 — client-side window chrome (under the
        // transient ring / drag-image), so a chromed window's title bar +
        // controls paint on the strip the content was inset below. A window
        // whose `window_policy().chrome` is `None` is a no-op.
        let chromed = self.apply_chrome(resizable, w, h);
        // R1195 §5.16 §5.39 — VS Code / Win11 / GTK parity: keep a chromed
        // window's TOP EDGE a live resize band. `apply_chrome` layered the strip
        // over the north resize region (killing top-edge resize); raise the
        // north band back on top so the outermost `RESIZE_EDGE_PX` resize the
        // window (the R1189 hover cursor + R1122 press routing light up for
        // free). Self-gating on the band's presence.
        let chromed = pinion_overlay::raise_top_resize_edge(chromed, Some((w, h)));
        // R705 §5.39 §2 #1/#7 — the keyboard focus ring as an introspectable,
        // pointer-transparent overlay, so it is (a) painted by the generic box
        // path rather than an opaque vello stroke, (b) visible to
        // `scene/snapshot from: paint` (§2 #7), and (c) corner-radius-aware.
        // Pointer-transparent (R705 §5.39) so it never shadows its widget for
        // input, even though this very scene feeds
        // [`pinion_runtime::InputRouter::last_paint_scene`] hit-testing.
        let ringed = inject_styled_focus_ring::<V>(chromed, self.ring_tag.as_deref(), Some((w, h)));
        // R1125 §5.51 PR-33 — the incoming cross-window dock drop-zone preview,
        // drawn at the dock zone in the TARGET (host) window where the panel
        // will land. This is the redock affordance. (R1168 retired the static
        // dock-zone GUIDES that used to layer here: a guide outlined whole panel
        // rects, independent of `resolve_drop`, so it diverged from the cursor
        // preview — the "선≠preview" divergence. The cursor preview, derived
        // from the one `resolve_drop` SSOT, is the SOLE drop affordance now.)
        let previewed = self.apply_cross_window_drop_preview::<V>(ringed, root_owner);
        // R1150 §5.51 — the R1137 on-FLOATER hint was REMOVED here (it drew the
        // zone schematic on the dragged floater's OWN rect; under the R1146
        // release-only model the floater stays PUT during the drag, so the hint
        // sat at the floater's static spot while the panel docked at the
        // cursor's target ELSEWHERE — the user's "preview here, docks there").
        // The on-target preview above is correctly placed; the R1147 desktop
        // chip is the cursor affordance. (Hiding the floater to un-occlude was
        // rejected: unmapping releases the X11 pointer grab the drag relies on.)
        self.apply_drag_image::<V>(previewed, w, h)
    }

    /// (R1122 §5.16 §5.39 §2 #7) The eight client-side resize edge / corner hit
    /// regions, injected when [`crate::WindowPolicy::resizable`] resolves true
    /// (default: derive from chrome presence — R1186 decoupled the two so a
    /// controls-in-header floater with `chrome: None` stays resizable). The
    /// regions are introspectable [`Scene`] nodes the shell maps to a
    /// `winit::window::ResizeDirection` in `AppShell::try_chrome_press`.
    ///
    /// R1123 — dropped while maximized: a maximized window fills the work area
    /// and edge-resize is meaningless (and would fight the WM), matching how
    /// OS-decorated windows hide their border when maximized.
    fn apply_resize_border(&self, scene: Scene, w: u32, h: u32) -> Scene {
        if self.title.is_none() {
            return scene;
        }
        let has_chrome = self.policy.chrome.is_some();
        if !self.policy.resizable.unwrap_or(has_chrome) || self.maximized {
            return scene;
        }
        if has_chrome {
            // The chrome strip (injected next, over this border) reclaims the
            // top; R1195's `raise_top_resize_edge` then lifts the north edge
            // back above the strip so the top edge still resizes.
            pinion_overlay::inject_resize_border(scene, Some((w, h)))
        } else {
            // R1197 / R1198 — a content dock header owns the top and hosts the
            // window controls at its top-RIGHT. Resize from the top EDGE + BOTH
            // top corners (VS Code parity for a floating panel); the top-right
            // corner is a small edge-sized box that fits inside the header's
            // right padding, so it resizes diagonally without shadowing the
            // close button (a full corner would — the R1186 concern).
            pinion_overlay::inject_resize_border_content_header(scene, Some((w, h)))
        }
    }

    /// (R1121 §5.16 §5.39 §2 #7) The client-side chrome strip (title bar +
    /// close / minimize / maximize controls) for a window whose policy carries
    /// a [`WindowChromeStyle`](crate::WindowChromeStyle). The buttons are real,
    /// introspectable [`Scene`]
    /// nodes an AI agent observes and drives via `scene/snapshot` + a click on
    /// the composite control tag — the reason custom chrome beats OS chrome,
    /// whose controls live outside the scene tree. R1467 is what makes that
    /// claim true of the RPC produce path too, not only of a stored frame.
    fn apply_chrome(&self, scene: Scene, w: u32, h: u32) -> Scene {
        match (self.title.as_deref(), self.policy.chrome) {
            (Some(title), Some(style)) => pinion_overlay::inject_window_chrome(
                scene,
                title,
                self.maximized,
                Some((w, h)),
                style,
            ),
            _ => scene,
        }
    }

    /// (R1125 §5.51 §2 #7 PR-33) The cross-window dock drop-zone PREVIEW, drawn
    /// when a floating panel dragged in ANOTHER window currently maps onto this
    /// one. The shell — the sole holder of every window's geometry — resolved
    /// the incoming drop into [`Self::cross_window_drop`]; the RESULT region of
    /// the target panel (the split half the redock would occupy, or the whole
    /// pane for a centre tabify) is highlighted, so the user SEES where the
    /// floater will land before releasing. No-op when no drag targets this
    /// window, the binding opts out ([`WidgetView::dock_drop_preview`] `None`),
    /// or the resolved panel has no rect in this scene. Each paint re-derives
    /// it (the prior strip is stripped by tag), so it follows the cursor live.
    fn apply_cross_window_drop_preview<V: WidgetView>(
        &self,
        scene: Scene,
        root_owner: &pinion_core::Owner,
    ) -> Scene {
        let inner = self.cross_window_drop.as_ref().and_then(|(source, drop)| {
            // GENERIC half: the target panel's window-absolute rect. The
            // dock-specific zone classification + strip rendering is the
            // binding's (`V::dock_drop_preview`), so the shell stays
            // widget-agnostic.
            // (R1156) OUTER full-span dock: the perimeter zone has no panel
            // rect — hand the binding the DOCK-AREA rect so it renders a
            // full-span strip (a row/column across every pane).
            let rect = if drop.point.tag == pinion_core::external::OUTER_DOCK_ZONE_TAG {
                // (R1205) …the DOCK AREA, NOT the whole window: the target
                // window's dock content sits below any client-side chrome strip
                // / toolbar / menu, so a preview spanning `scene.rect()` painted
                // the full-span band ACROSS the title-bar controls when redocking
                // a floater onto a chromed window (the user's "붙일 때 최소화/최대화/x
                // 영역까지 preview가 보임"). Read the dock walker's
                // `DOCK_SURFACE_TAG` rect so the previewed band == where
                // `dock_panel_outer` actually lands (the topology has no chrome
                // row) — preview == result. The same `dock_surface_rect` SSOT the
                // same-window band reads; a window with no dock surface (a
                // decorated / naked one) falls back to its own rect, unchanged.
                scene.dock_surface_rect()
            } else {
                scene.rect_for_tag_absolute(&drop.point.tag)?
            };
            // (R1163b) Pass the dragged `source` so the binding resolves through
            // the SAME `resolve_drop` SSOT the release applies (preview ==
            // result). The hook reads the binding's reactive reorganizer
            // (is_panel / tabbing) via `use_*` hooks, so run it inside
            // `root_owner.run` — exactly like `collect_access_emit_inputs` wraps
            // `V::access_node` (the callback-root-owner-wrap family). This
            // overlay step runs AFTER the view's `root_owner.run` closed, so
            // without the wrap `Owner::current()` is `None` and the hook panics.
            // Read-only, so it re-subscribes nothing it did not already (no
            // re-dirty after the caller's `clear_dirty`).
            root_owner.run(|| {
                V::dock_drop_preview(
                    source,
                    &drop.point.tag,
                    rect,
                    drop.point.x_rel,
                    drop.point.y_rel,
                )
            })
        });
        // Wrap the binding's overlay in a shell-owned, pointer-transparent slot
        // so the strip is stripped + replaced by ONE known tag each paint
        // (idempotent, like every other overlay), independent of the binding
        // node's own tag.
        let slot = inner.map(|node| {
            Scene::Container(
                pinion_core::scene::ContainerNode::new(vec![node])
                    .with_tag(CROSS_WINDOW_DROP_PREVIEW_TAG.to_string())
                    .with_layout(
                        pinion_core::style::LayoutStyle::new().with_pointer_transparent(true),
                    ),
            )
        });
        pinion_overlay::inject_overlay_node(scene, CROSS_WINDOW_DROP_PREVIEW_TAG, slot)
    }

    /// (R1113 §5.51 §5.33 §2 #7) The drag-image follower — the translucent chip
    /// floated under the cursor during a drag — as an introspectable,
    /// pointer-transparent overlay. Because the chip is anchored at the cursor
    /// (window-level) it is correct no matter where the dragged widget sits,
    /// and every consumer gets it with no wiring.
    ///
    /// No-op when the R1147 cross-desktop PREVIEW window is already showing
    /// this drag (exactly one chip shows — the desktop preview, which roams the
    /// whole desktop), when no REAL drag is in flight (a pending click shows
    /// none), when the drag carries no text label (a capture-drag like a
    /// splitter resize, or a non-text payload), or when the binding's
    /// [`WidgetView::drag_image_style`] hook returns `None`.
    fn apply_drag_image<V: WidgetView>(&self, scene: Scene, w: u32, h: u32) -> Scene {
        if self.desktop_drag_preview_active {
            return scene;
        }
        let Some((label, cursor)) = self.drag_image.as_ref() else {
            return scene;
        };
        let Some(style) = V::drag_image_style(label) else {
            return scene;
        };
        pinion_overlay::inject_drag_image(scene, label, *cursor, style, Some((w, h)))
    }
}

impl<V: WidgetView> ShellCore<V> {
    /// R51.76 §5.40 — borrow the cached state projection. Tests
    /// observe widget state transitions through this accessor.
    /// R51.123 §5.41 — delegates to [`CoreShell::cached_state`].
    #[must_use]
    pub fn cached_state(&self) -> &V::State {
        self.core.cached_state()
    }

    /// R51.143 §5.28 — delegates to
    /// [`CoreShell::root_owner`](pinion_runtime::CoreShell::root_owner)
    /// so the view fn (and tests + SCE-emitted code) can attach
    /// [`Animation<T>`](pinion_core::Animation) instances and
    /// [`Effect`](pinion_core::Effect) closures to this binding's
    /// reactive scope.
    ///
    /// The animation list registered here is exactly the one
    /// [`Self::compute_paint_scene`] ticks once per paint cycle.
    /// Drop on `ShellCore` cascades through the wrapped
    /// [`CoreShell`] into the
    /// [`Owner`](pinion_core::Owner) drop semantics, cancelling every
    /// pending [`Command`](pinion_core::Command) and animation in the
    /// scope (Solid pattern, R51.137 + R51.139).
    #[must_use]
    pub fn root_owner(&self) -> &pinion_core::Owner {
        self.core.root_owner()
    }

    /// R51.76 §5.40 — current §5.34 R40.4 OCC revision counter
    /// (loaded with `Acquire` ordering — see
    /// [`SceneRevision::current`]). Mutating winit / AT-side
    /// dispatches bump it; tests assert the before/after delta when
    /// verifying that a dispatch path actually committed.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision.current()
    }

    /// R1270 §6.3 — borrow the single scene [`SceneRevision`] token for the
    /// async `scene/waitFor` decision (the ingress reads `since` against it and
    /// parks; the `scene/revision` read method reports its value). The `u64`
    /// peer [`revision`](Self::revision) is the common read.
    #[must_use]
    pub fn revision_token(&self) -> &SceneRevision {
        self.revision.as_ref()
    }

    /// R51.76 §5.40 — borrow the live state scene. Tests reach the
    /// widget External through `Scene::External(node) => node.handle`
    /// when verifying introspect side effects.
    /// R51.123 §5.41 — delegates to [`CoreShell::scene`].
    #[must_use]
    pub fn scene(&self) -> &Scene {
        self.core.scene()
    }

    /// R890 §5.12 §5.16 — project the named window's stored paint
    /// scene into a [`LayoutNode`] tree, on demand. The per-window
    /// scene the publish primitives store
    /// ([`pinion_runtime::InputRouter::last_paint_scene`]) is the ONE
    /// layout source, so a window can only ever answer with its own
    /// geometry. `None` when the window has never painted (the wire's
    /// `NoLastPaintLayout` honesty; pre-R890 a binding-wide
    /// last-writer-wins mirror answered with whichever window painted
    /// or dispatched last).
    ///
    /// R890.1 — substrate-side observability/test accessor (the
    /// `has_last_paint_scene_for_window` pattern): the dispatch path
    /// itself no longer calls this — the dispatcher projects lazily
    /// from the `DispatchContext::last_paint_scene` borrow it already
    /// threads for `scene/snapshot from: paint`, ONE channel for
    /// layout + pixel introspection. Both routes go through
    /// [`pinion_rpc::project_layout`], so this accessor is the
    /// substrate-level mirror of exactly what the wire answers.
    ///
    /// Replaces two retired caches: the `ShellCore.last_paint_layout`
    /// binding-wide mirror and the per-frame
    /// `WindowSlot.last_paint_layout` build (`AppShell::render_window`
    /// walked the paint scene EVERY winit frame to keep it fresh).
    #[must_use]
    pub fn last_paint_layout_for_window(&self, window_id: &str) -> Option<LayoutNode> {
        self.core
            .last_paint_scene_for_window(window_id)
            .map(pinion_rpc::project_layout)
    }

    /// (R684 §5.16 §5.41 §5.49) Read-only passthrough to
    /// [`pinion_runtime::CoreShell::has_last_paint_scene_for_window`]
    /// — `true` once the named window's
    /// [`pinion_runtime::InputRouter`] has received a paint scene.
    /// Used by R684 atomic 3 substrate tests to assert that the
    /// post-dispatch finalize hook populated the addressed window's
    /// router (closing the headless-RPC floating-window gap).
    #[must_use]
    pub fn has_last_paint_scene_for_window(&self, window_id: &str) -> bool {
        self.core.has_last_paint_scene_for_window(window_id)
    }

    /// R51.76 §5.40 — drain the redraw flag set by `request_redraw`.
    ///
    /// Returns `true` once for each call to `request_redraw` between
    /// drains. [`crate::AppShell`] calls this at the end of every
    /// event-loop iteration and forwards to `Window::request_redraw`
    /// on `true`; headless tests call it directly to verify that a
    /// dispatch triggered a repaint request without standing up a
    /// `Window`.
    pub fn take_redraw_request(&mut self) -> bool {
        let r = self.redraw_requested;
        self.redraw_requested = false;
        r
    }

    /// R51.76 §5.40 — `true` when a redraw has been requested since
    /// the last drain. Tests prefer [`take_redraw_request`](Self::take_redraw_request)
    /// when they want to consume the signal; this accessor is for
    /// debug logging and peek-only assertions.
    #[must_use]
    pub fn redraw_requested(&self) -> bool {
        self.redraw_requested
    }

    /// R51.76 §5.40 — note that a repaint is required.
    ///
    /// The flag is drained by [`crate::AppShell`] once per event-loop
    /// iteration, so multiple `request_redraw` calls within one
    /// dispatch collapse to a single `Window::request_redraw` call
    /// (the textbook winit idiom: redraws are coalesced).
    ///
    /// R680 atomic 2 §5.16 §5.41 — this is the binding-wide
    /// "fan out to every window" wake-up. For wake-ups that should
    /// target a specific window only (R680 atomic 3 RPC dispatch
    /// follow-ups, R681 immediate-mode game-loop polling, R683
    /// dock-panel local layout reactions) use
    /// [`Self::request_redraw_for_window`] instead.
    pub fn request_redraw(&mut self) {
        self.redraw_requested = true;
    }

    /// R1193 §5.16 §5.28 §5.39 — read-only access to a window's consolidated
    /// [`WindowState`], or `None` when the window has no entry yet. NEVER creates
    /// an entry, so every per-axis read preserves its pre-R1193 `HashMap::get()`
    /// "absent" semantics (a missing entry ≡ every axis at its absent-default).
    fn window_state(&self, window_id: &str) -> Option<&WindowState> {
        self.window_states.get(window_id)
    }

    /// R1193 §5.16 §5.28 §5.39 — mutable access to a window's consolidated
    /// [`WindowState`], lazily inserting a default entry on first touch (the
    /// pre-R1193 `entry().or_default()` / `insert()` write path). Use only on the
    /// write side; reads go through [`Self::window_state`] so a read never mints
    /// an entry.
    fn window_state_mut(&mut self, window_id: &str) -> &mut WindowState {
        self.window_states.entry(window_id.to_owned()).or_default()
    }

    /// R1193 §5.28 — take (and clear) a window's pending injected immediate-mode
    /// `dt` (the `scene/tick` one-shot), or `None` if none is queued. Never
    /// creates an entry. Extracted so `compute_paint_scene_internal` consumes the
    /// injection in one line (the pre-R1193 `HashMap::remove` call shape).
    fn take_pending_immediate_dt(&mut self, window_id: &str) -> Option<f32> {
        self.window_states
            .get_mut(window_id)
            .and_then(|s| s.pending_immediate_dt.take())
    }

    /// R680 atomic 2 §5.16 §5.41 — per-window redraw wake-up.
    ///
    /// Sets `WindowState::redraw_requested` for the named
    /// `window_id`. `crate::AppShell::drain_redraw_to_winit`
    /// drains the flag and calls
    /// `Window::request_redraw` on ONLY that window's slot.
    ///
    /// Use when the caller knows exactly which window a wake-up
    /// should target — RPC dispatch follow-ups, R681 immediate-
    /// mode subtree polling, R683 dock-panel layout reactions.
    /// For binding-wide wake-ups (`Signal::set` whose subscribers
    /// span multiple windows, post-tick `any_animation_active`
    /// check) keep using [`Self::request_redraw`] which fans out.
    ///
    /// Lazy-creates the map entry on first call per `window_id`.
    /// Multiple calls between drains collapse to a single
    /// `Window::request_redraw` per drain (idempotent on the
    /// `bool` field).
    pub fn request_redraw_for_window(&mut self, window_id: &str) {
        self.window_state_mut(window_id).redraw_requested = true;
    }

    /// R680 atomic 2 §5.16 §5.41 — drain a single window's redraw
    /// flag. Returns `true` once for each
    /// [`Self::request_redraw_for_window`] call between drains;
    /// resets the flag to `false` so the next event-loop iteration
    /// sees a clean state. Unknown `window_id` (never requested)
    /// returns `false` without allocating.
    ///
    /// `crate::AppShell::drain_redraw_to_winit` calls this for
    /// every active window slot to determine the per-window
    /// `Window::request_redraw` dispatch.
    pub fn take_redraw_request_for_window(&mut self, window_id: &str) -> bool {
        self.window_states
            .get_mut(window_id)
            .is_some_and(|s| std::mem::take(&mut s.redraw_requested))
    }

    /// R680 atomic 2 §5.16 §5.41 — peek-only probe for the
    /// per-window redraw flag. Returns `true` when
    /// [`Self::request_redraw_for_window`] has set the flag since
    /// the last [`Self::take_redraw_request_for_window`] drain.
    /// Used by tests + debug logging that want to assert "yes the
    /// caller targeted window X" without consuming the signal.
    #[must_use]
    pub fn redraw_requested_for_window(&self, window_id: &str) -> bool {
        self.window_state(window_id)
            .is_some_and(|s| s.redraw_requested)
    }

    /// R1426 §5.41 §5.28 — the window's render-time terminal-cursor blink phase
    /// for this frame: `true` when a blinking cursor should paint (its visible
    /// half), `false` on the hidden half; steady / hidden cursors resolve to
    /// `true`. The winit surface reads it after `compute_paint_scene_for_window`
    /// (which armed the clock) and threads it into the Vello paint. Delegates to
    /// [`CoreShell::grid_cursor_blink_on`](pinion_runtime::CoreShell::grid_cursor_blink_on);
    /// read here, outside the view's reactive scope, so the phase never folds
    /// into the scene (§2 #7).
    #[must_use]
    pub fn grid_cursor_blink_on(&self, window_id: &str) -> bool {
        self.core.grid_cursor_blink_on(window_id)
    }

    /// R1427 §5.41 §5.39 — arm this window's terminal-cursor blink clock from its
    /// paint scene, gated on OS focus. Forwards to
    /// [`CoreShell::arm_grid_cursor_blink`](pinion_runtime::CoreShell::arm_grid_cursor_blink)
    /// with this window's fails-open [`Self::is_key_dispatch_window`] focus
    /// verdict — the SAME predicate the hollow-render `cursor_focused` flag reads,
    /// so an unfocused window stops blinking and renders hollow consistently (the
    /// two facts can never disagree). Owning the focus read here keeps the paint
    /// loop's arm call a single line.
    pub fn arm_grid_cursor_blink(&self, window_id: &str, scene: &Scene) {
        let focused = self.is_key_dispatch_window(window_id);
        self.core.arm_grid_cursor_blink(window_id, scene, focused);
    }

    /// R681 §2 #4 atomic 2 §5.16 §5.28 — per-window last paint
    /// [`Instant`]. The substrate's
    /// `Self::compute_paint_scene_internal` writes this slot every
    /// paint cycle (R680 atomic 1 lift); the surface reads it to
    /// compute the next per-window paint deadline for the
    /// [`winit::event_loop::ControlFlow::WaitUntil`] game-loop pacing
    /// branch.
    ///
    /// Returns `None` for an unknown / never-painted window key
    /// (first-paint bootstrap case — the surface treats this as
    /// "paint ASAP" and dispatches an immediate redraw).
    #[must_use]
    pub fn last_paint_instant_for_window(&self, window_id: &str) -> Option<Instant> {
        self.window_state(window_id)
            .and_then(|s| s.last_paint_instant)
    }

    /// R681 §2 #4 atomic 3 §5.16 §5.28 — per-window target fps
    /// override for the game-loop pacing branch. Bindings call this
    /// to opt a window into 30fps (battery saver) / 144fps
    /// (high-refresh display) / 0 (paused polled window sentinel)
    /// instead of the default
    /// [`pinion_runtime::frame_pacing::DEFAULT_IMMEDIATE_MODE_FPS`].
    ///
    /// The override is consulted by
    /// [`pinion_shell::AppShell::about_to_wait`](winit::application::ApplicationHandler::about_to_wait) each event-loop
    /// iteration via
    /// [`pinion_runtime::frame_pacing::frame_budget_for_window`];
    /// re-calling this method with a different `fps` is the
    /// canonical "change pacing on the fly" surface.
    pub fn set_target_fps_for_window(&mut self, window_id: &str, fps: u32) {
        self.window_state_mut(window_id).target_fps = Some(fps);
        // R831 §2 #4 §5.28 — pausing (`fps == 0`) snaps the immediate-mode
        // accumulator back to a zero phase, so an AI client that then
        // frame-steps via `scene/tick` advances from a known fixed-step
        // boundary (the deterministic-debugging contract — a debugger
        // breaks between frames, not mid-sub-step). The discarded
        // remainder is sub-fixed (< ~8 ms of simulation). Resume keeps
        // the post-pause accumulator (fresh zero phase) so live wall-clock
        // restarts cleanly.
        if fps == 0 {
            if let Some(acc) = self
                .window_states
                .get_mut(window_id)
                .and_then(|s| s.sim_accumulator.as_mut())
            {
                acc.reset();
            }
        }
    }

    /// R888 §2 #4 §5.28 §5.49 — clear the per-window target-fps
    /// override, restoring the adaptive default policy
    /// ([`pinion_runtime::frame_pacing::default_window_frame_policy`]:
    /// 60fps while immediate-mode content is active, idle otherwise).
    /// The `scene/set_fps {"fps": null}` drain — pre-R888 the boot
    /// state was unreachable once any override landed (insert-only
    /// map), which the `scene/pacing_state` READ peer made visible.
    /// No accumulator reset: clearing is a policy hand-back, not a
    /// pause edge (the R831 zero-phase snap is the `fps == 0` arm's
    /// frame-step contract).
    pub fn clear_target_fps_for_window(&mut self, window_id: &str) {
        if let Some(state) = self.window_states.get_mut(window_id) {
            state.target_fps = None;
            // R1193 — prune an entry emptied by clearing its last axis, so
            // `remove_window`'s "carried an entry" return stays exact (the
            // pre-R1193 per-map `remove` left no lingering key).
            if state.is_empty() {
                self.window_states.remove(window_id);
            }
        }
    }

    /// R885 / R888 §5.49 — pre-resolve the per-window out-of-band READ
    /// axes (`scene/input_state` + `scene/pacing_state`) before
    /// `dispatch_rpc_inner`'s split-borrow block (the
    /// `fragment_cache_stats` pattern; extracted to respect that fn's
    /// 100-line budget).
    ///
    /// R889 §5.49 — the READ axes gate on the NAMED window-known
    /// predicate ([`pinion_runtime::CoreShell::is_window_known`], the
    /// window-owners registry) — a deliberate shared gate, not the
    /// pre-R889 piggyback where pacing availability rode on the
    /// input snapshot's router-presence resolution (routers = "has
    /// painted", a different category than "window exists"; a
    /// registered-but-unpainted R683 tear-off honored `set_fps` writes
    /// while reading back `Unavailable`). Unknown window ids never
    /// reach these axes in production (the `unknown_window` verdict
    /// rejects the request at dispatch entry first); the `None`
    /// legs remain the honest answer for direct substrate callers.
    ///
    /// R890.1 — `window_id` and the verdict's judged id are the SAME
    /// value by construction: both entries derive the dispatch scope
    /// from the request's `{window: "<id>"}` param through the one
    /// extraction home ([`pinion_rpc::Request::window_scope`]), so
    /// every field of the returned struct describes one window.
    /// Extraction + judgment glue is 1-homed in
    /// [`pinion_rpc::unknown_window_verdict`] (GUI/TUI ingress
    /// parity, the R886.1 lesson).
    fn window_scoped_rpc_reads(
        &self,
        request: &Request,
        window_id: Option<&str>,
    ) -> WindowScopedRpcReads {
        let wid = window_id.unwrap_or(pinion_runtime::DEFAULT_WINDOW);
        WindowScopedRpcReads {
            cache_stats: self.fragment_cache_stats_for_window(wid),
            frame_timings: self.frame_timings_for_window(wid),
            render_fidelity: self.render_fidelity_for_window(wid),
            input_state: self.core.input_state_snapshot(
                wid,
                Some(self.modifiers),
                Some(self.key_dispatch_focus_for_window(wid)),
            ),
            auto_repeat_holds: self.auto_repeat_holds_for_window(wid),
            accelerators: self
                .core
                .accelerator_map_for_window(wid, self.focus.focused()),
            accelerator_focus: self.focus.focused().map(str::to_owned),
            pacing_state: self.core.is_window_known(wid).then(|| {
                match self.target_fps_for_window(wid) {
                    Some(fps) => pinion_rpc::PacingState::Override(fps),
                    None => pinion_rpc::PacingState::DefaultPolicy,
                }
            }),
            unknown_window: pinion_rpc::unknown_window_verdict(request, |w| {
                self.core.is_window_known(w)
            }),
        }
    }

    /// R1549.3 §5.35 §5.12 — the in-flight press census this window would
    /// answer `scene/auto_repeat` with, and the ONE home the dispatch
    /// resolver reads it from.
    ///
    /// The `pinion-tui` peer (`ShellCoreTui::auto_repeat_holds`) landed at
    /// R1549.2 while this side kept resolving inline, which left the GUI
    /// with no way for a test to observe a hold — and that is how the live
    /// paint-clock arm came to have no coverage at all (see
    /// `tests/auto_repeat_live_clock.rs`).
    #[must_use]
    pub fn auto_repeat_holds_for_window(
        &self,
        window_id: &str,
    ) -> Vec<pinion_runtime::AutoRepeatHold> {
        self.core.auto_repeat_holds_for_window(window_id)
    }

    /// R681 §2 #4 atomic 3 §5.16 §5.28 — read the per-window target
    /// fps override, if set. `None` means "use the default policy"
    /// (60fps when immediate-mode-active, idle otherwise — see
    /// [`pinion_runtime::frame_pacing::default_window_frame_policy`]).
    #[must_use]
    pub fn target_fps_for_window(&self, window_id: &str) -> Option<u32> {
        self.window_state(window_id).and_then(|s| s.target_fps)
    }

    /// R681 §2 #4 §5.16 — publish whether `window_id`'s most recently
    /// painted scene carried an immediate-mode subtree. Called by
    /// `AppShell::render_window` every paint cycle (the sticky signal
    /// the next `about_to_wait` pacing decision + the jank profiler both
    /// read), keeping the immediate-mode flag in the same home as the
    /// `target_fps` override so the two pacing inputs never drift.
    pub fn set_immediate_subtree_for_window(&mut self, window_id: &str, has_immediate: bool) {
        self.window_state_mut(window_id).immediate_subtree = has_immediate;
    }

    /// R681 §2 #4 §5.16 — whether `window_id`'s last painted scene
    /// carried an immediate-mode subtree. `false` for an unknown or
    /// never-painted window (the retained-tree default). The pacing
    /// input both `about_to_wait` and the jank budget
    /// ([`Self::frame_timings_for_window`]) consult through
    /// [`pinion_runtime::frame_pacing::frame_budget_for_window`].
    #[must_use]
    pub fn immediate_subtree_for_window(&self, window_id: &str) -> bool {
        self.window_state(window_id)
            .is_some_and(|s| s.immediate_subtree)
    }

    /// R682 §5.16 atomic 3 — publish a [`FragmentCacheStats`]
    /// snapshot for the given window. Surface-side
    /// `AppShell::render_window` calls this after each paint cycle so
    /// the GUI-agnostic substrate can surface cache observability to
    /// RPC / tests without exposing `vello::Scene` references.
    pub fn publish_fragment_cache_stats(&mut self, window_id: &str, stats: FragmentCacheStats) {
        self.window_state_mut(window_id).fragment_cache_stats = Some(stats);
    }

    /// R907 §5.16 §5.7 — record one painted frame's phase breakdown
    /// into the window's rolling [`FrameTimingStats`] window. The
    /// surface-side `AppShell::render_window` calls this after each
    /// paint cycle with the measured build / encode / acquire / render /
    /// total microseconds (R1361.1 split the vsync block out of render). Lazily inserts a fresh accumulator on the
    /// window's first paint (the `sim_accumulator` pattern).
    pub fn record_frame_timing(&mut self, window_id: &str, timing: FrameTiming) {
        self.window_state_mut(window_id)
            .frame_timings
            .get_or_insert_with(FrameTimingStats::default)
            .record(timing);
    }

    /// R1537 §5.16 — record whether the backend that painted `window_id`
    /// can time the GPU.
    ///
    /// Separate from [`Self::record_frame_timing`] because it is not a
    /// property of a frame: it does not change from frame to frame, it
    /// belongs to the device, and a per-sample copy would be the same bit
    /// repeated `FRAME_TIMING_WINDOW` times with no way to read it before
    /// the first paint.
    pub fn set_gpu_timing_state(&mut self, window_id: &str, supported: bool, dropped: u64) {
        self.window_state_mut(window_id).gpu_timing = if supported {
            WindowGpuTiming::Timed { dropped }
        } else {
            WindowGpuTiming::Untimed
        };
    }

    /// R1708 §5.16 §5.41 — record a resize the window system just delivered for
    /// `window_id`, WITHOUT painting it.
    ///
    /// Every event reaches this call: the fold happens at the paint, never at
    /// the report, so no size is ever lost to a reader (the reference toolkit
    /// splits it the same way — measured, its window layer delivers all forty
    /// of forty queued resizes while its widget layer sees one). The returned
    /// [`Noted`](pinion_core::resize_batch::Noted) says what this event did to
    /// the pending fold, so a caller that wants to log or trace the drop has
    /// the reason in hand rather than a count to diff.
    pub fn note_window_resize(
        &mut self,
        window_id: &str,
        size: (u32, u32),
    ) -> pinion_core::resize_batch::Noted {
        self.window_state_mut(window_id).resize_batch.note(size)
    }

    /// R1708 §5.16 §5.41 — take `window_id`'s pending resize fold, if any.
    ///
    /// The caller MUST paint exactly one frame per returned fold and then say
    /// so with [`Self::note_resize_painted`]. Between the two calls the
    /// window's books are UNBALANCED on purpose — it is owed a frame it has not
    /// had — and [`Self::resize_tally_for_window`]'s balance identity is what
    /// says so. A caller that takes and never paints is exactly the break a
    /// counterfactual walked through untouched when the count lived here.
    pub fn take_pending_resize(
        &mut self,
        window_id: &str,
    ) -> Option<pinion_core::resize_batch::Fold> {
        self.window_states
            .get_mut(window_id)
            .and_then(|s| s.resize_batch.take())
    }

    /// R1708 §5.16 — `window_id`'s pending resize fold WITHOUT draining it.
    ///
    /// The event loop peeks before taking so it can decline a fold it cannot
    /// answer (a zero dimension — a minimize) and leave it pending, rather than
    /// draining one and counting a frame nothing drew.
    #[must_use]
    pub fn pending_resize(&self, window_id: &str) -> Option<pinion_core::resize_batch::Fold> {
        self.window_state(window_id)
            .and_then(|s| s.resize_batch.pending().copied())
    }

    /// R1708 §5.16 §5.41 — record that a frame answering `fold` was painted for
    /// `window_id`. The second half of [`Self::take_pending_resize`].
    pub fn note_resize_painted(&mut self, window_id: &str, fold: pinion_core::resize_batch::Fold) {
        self.window_state_mut(window_id).resize_batch.painted(fold);
    }

    /// R1708 §5.16 — the ids of every window with a resize waiting to be
    /// painted, in the map's iteration order.
    ///
    /// Returned owned so the event loop can drain and paint each one without
    /// holding a borrow of the state map across the paint.
    #[must_use]
    pub fn windows_with_pending_resize(&self) -> Vec<String> {
        self.window_states
            .iter()
            .filter(|(_, s)| s.resize_batch.is_pending())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// R1708 §5.16 §2 #7 — what `window_id`'s resizes have cost since boot.
    ///
    /// All-zero for a window that has never been resized, which is the honest
    /// reading rather than a gap.
    #[must_use]
    pub fn resize_tally_for_window(
        &self,
        window_id: &str,
    ) -> pinion_core::resize_batch::ResizeTally {
        self.window_state(window_id)
            .map(|s| *s.resize_batch.tally())
            .unwrap_or_default()
    }

    /// R1036 §5.16 §5.7 §2 #7 — record the render-fidelity fingerprint of the
    /// frame `AppShell::render_window` just ENCODED + presented for `window_id`
    /// (PR-17). `present_ok` is the `renderer.render` outcome; `viewport` is the
    /// logical size it laid out at; `scene` is the exact encoded tree.
    ///
    /// The monotonic `paint_seq` advances by one per recorded present (starting
    /// at 1), so an AI client reads the count before and after driving an
    /// interaction: no advance ⟹ no frame presented for the settled state. This
    /// is the winit-paint-path SSOT for `scene/render_fidelity` — the
    /// `last_paint_scene` the RPC dispatch overwrites is deliberately NOT
    /// consulted here, so the record cannot be contaminated by a query-time
    /// recompute.
    /// R1709 — `health` is the backend's recovery-ladder state, projected by
    /// the caller (which is the layer that holds the renderer). Passed in
    /// rather than read here so this stays the one place a frame's record is
    /// assembled.
    pub fn record_presented_frame(
        &mut self,
        window_id: &str,
        present_ok: bool,
        viewport: (u32, u32),
        scene: &pinion_core::Scene,
        health: pinion_runtime::PresentHealth,
    ) {
        let paint_seq = self
            .window_state(window_id)
            .and_then(|s| s.render_fidelity.as_ref())
            .map_or(1, |prev| prev.paint_seq.wrapping_add(1));
        self.window_state_mut(window_id).render_fidelity = Some(pinion_runtime::RenderFidelity {
            paint_seq,
            presented_at_ms: pinion_runtime::render_fidelity::elapsed_ms(),
            present_ok,
            viewport_w: viewport.0,
            viewport_h: viewport.1,
            grids: pinion_runtime::render_fidelity::grid_fidelity(scene),
            health,
        });
    }

    /// R683 §5.16 §5.41 — drop every shell-side per-window state
    /// entry for `window_id`.
    ///
    /// Drops this window's consolidated `WindowState` entry (R1193 — ONE
    /// removal for every per-window axis: redraw / paint-clock / pacing /
    /// profiling / chrome / focus), then forwards into
    /// [`pinion_runtime::CoreShell::remove_window`] which drains the
    /// runtime-side per-window state (`routers`, `window_owners`).
    ///
    /// R25.1 §5.39 — dropping the closed window's focusable contribution and
    /// re-folding the union (`Self::union_focusable_tags`) removes its tags
    /// from the Tab order / click-focus set, and the §5.39 stale-focus guard
    /// inside `update_focusable_tags` drops focus if it pointed at one of the
    /// now-unpainted widgets. R26 §5.16 — the tags actually leave the union only
    /// because `crate::AppShell::reconcile_windows`'s drop-pass removes the spec
    /// from `windows_signal` BEFORE calling this, so the closed window is also
    /// un-declared and the R26 declared-topology derivation does not re-add its
    /// tags. Calling `remove_window` on a STILL-declared window would re-derive
    /// and keep its tags enumerated (no such caller exists — the reconcile drop is
    /// the only producer).
    ///
    /// Refuses to remove the [`pinion_runtime::DEFAULT_WINDOW`]
    /// primary id — the substrate's primary scope is aliased to
    /// `root_owner` so removing it would orphan the binding's
    /// reactive state. Returns `true` when at least one map carried
    /// an entry, `false` for `DEFAULT_WINDOW` and for unknown ids.
    ///
    /// Designed for the R683 `crate::AppShell::reconcile_windows`
    /// Effect drop pass after a dock tear-off / dock-back arc
    /// resolves.
    pub fn remove_window(&mut self, window_id: &str) -> bool {
        if window_id == pinion_runtime::DEFAULT_WINDOW {
            return false;
        }
        // R1193 — ONE removal drops every per-window axis at once, the payoff of
        // the `WindowState` consolidation. The pre-R1193 body OR-ed 11 separate
        // `*_per_window.remove(...)` calls, and the R1123.1 / R681 leak class was
        // exactly "a new per-window map was added but not listed here, so a reused
        // window id inherited its stale entry (wrong maximized glyph / pacing)." A
        // new per-window axis is now a `WindowState` field, cleared by this single
        // entry removal — teardown can no longer fall out of sync with storage.
        let shell_side = self.window_states.remove(window_id).is_some();
        // R25.1 §5.39 — re-fold the focus enumeration without the closed
        // window's tags. Idempotent when the window held no focusables (the
        // union is unchanged and `update_focusable_tags` re-finds the focused
        // tag); when it DID, those tags leave the Tab order and a focus that
        // pointed at one of them is dropped by the §5.39 stale guard. Off the
        // paint path (a rare dock tear-down), so the O(windows) fold is free.
        let union = self.union_focusable_tags();
        if self.focus.update_focusable_tags(union) {
            // R1327 §5.39 — a closed window taking the focused tag with it is a focus
            // change like any other: pair it with a redraw so the focus ring and any
            // focus-derived binding state (a title naming the active pane) re-derive.
            self.revision.bump();
            self.request_redraw();
        }
        // R1073.1 PR-27.4 §5.39 §5.16 §5.35 — reconcile the OS-focus identity
        // on the one lifecycle event the shell itself controls (destruction),
        // rather than trusting a platform `Focused(false)` that a destroyed
        // window may never emit. If `os_focused_window` still names the window
        // being torn down, a stale `Some(dead)` would make
        // [`Self::is_key_dispatch_window`] return `false` for EVERY live window
        // (`dead != live`), silently swallowing all keystrokes until the next
        // focus event — worse than the double-toggle it guards. Clearing to
        // `None` re-arms the fail-open default until a real `Focused(true)`
        // lands. `key_press_owner` is deliberately NOT reconciled here: an
        // in-flight press whose own dispatch closed this window is exactly the
        // stray the gate must still drop, so its owner must survive the close
        // (it self-clears on the keyup, [`Self::note_key_release`]); clearing
        // it on destruction would re-open the now-focused-successor stray.
        if self.os_focused_window.as_deref() == Some(window_id) {
            self.os_focused_window = None;
        }
        // CoreShell::remove_window returns true on at least one
        // runtime-side removal; the OR with shell_side surfaces "any
        // per-window state existed" so the AppShell-side reconcile
        // can log / introspect cleanup actually happened.
        let runtime_side = self.core.remove_window(window_id);
        shell_side || runtime_side
    }

    /// R889 §5.16 §5.49 — register `window_id` in the substrate's
    /// window-known registry
    /// ([`pinion_runtime::CoreShell::register_window`]). The backend
    /// calls this when the OS window comes into existence
    /// (`crate::AppShell::resume_spec`, before the first paint) so
    /// availability gates ([`pinion_runtime::CoreShell::is_window_known`])
    /// and the dispatch-entry unknown-window rejection see the window
    /// from creation — not from first paint. Matching removal edge is
    /// [`Self::remove_window`].
    pub fn register_window(&mut self, window_id: &str) {
        self.core.register_window(window_id);
    }

    /// R682 §5.16 atomic 3 — read the most-recent
    /// [`FragmentCacheStats`] snapshot for the given window.
    ///
    /// `None` for windows that have not yet painted (no snapshot
    /// published yet) — bootstrap state for the first-paint frame.
    /// `Some(stats)` after every per-window paint cycle; the
    /// snapshot reflects the cache state at the moment
    /// [`pinion_runtime::paint_adapter::FragmentCache::end_paint`]
    /// completed (so the mark-and-sweep + damage publish are already
    /// applied).
    ///
    /// AI-first design note ([[ai-first-rpc-introspection-obligation]]):
    /// the R682 demo's cache-hit-rate / damage-region assertions
    /// derive from this getter, not from observing the painted
    /// pixels — `cargo test` + `tools/demos/*.py` both verify the
    /// substrate via this typed surface, not via screenshot diffing
    /// or human visual review.
    #[must_use]
    pub fn fragment_cache_stats_for_window(&self, window_id: &str) -> Option<FragmentCacheStats> {
        self.window_state(window_id)
            .and_then(|s| s.fragment_cache_stats)
    }

    /// R907 §5.16 §5.7 — project the window's rolling
    /// [`FrameTimingStats`] into a `Copy` [`FrameTimingsSnapshot`].
    ///
    /// `None` for a window that has not painted yet (no accumulator
    /// inserted) OR whose accumulator is still empty (lazy-inserted but
    /// not yet recorded) — the bootstrap state `scene/frame_timings`
    /// surfaces as `FrameTimingsUnavailable`, distinct from an all-zero
    /// snapshot. `Some` after the first paint, carrying the last
    /// frame's phases plus the window's min/mean/max + per-phase means.
    ///
    /// The O(window) aggregate fold runs here, at the AI-paced RPC
    /// read — never on the paint path (`r890` read-time projection).
    #[must_use]
    pub fn frame_timings_for_window(&self, window_id: &str) -> Option<FrameTimingsSnapshot> {
        let budget_us = self.jank_budget_us_for_window(window_id);
        self.window_state(window_id)
            .and_then(|s| s.frame_timings.as_ref())
            .and_then(|stats| stats.snapshot(budget_us))
            .map(|mut snap| {
                // R1460 §5.16 §2 #2 — the producer's cumulative work rides the
                // same read. It is NOT folded into the ring: the ring holds
                // frames the user saw, and a produce is not one. Attached here
                // so an agent gets both halves from one call instead of
                // correlating two surfaces.
                snap.produce = self.produce_work;
                // R1464 §5.16 §5.39 / R1465 §5.12 — same rule, same reason:
                // binding-wide work that painted nothing, attached to the read
                // rather than folded into the ring.
                snap.focus = self.focus_work.get();
                snap.mirror = self.mirror_work;
                // R1537 §5.16 — the backend's timing capability. Attached
                // at the read for the same reason as the three above: it
                // is not a property of any one frame in the ring.
                if let Some(WindowGpuTiming::Timed { dropped }) =
                    self.window_state(window_id).map(|w| w.gpu_timing)
                {
                    snap.gpu_timing_supported = true;
                    snap.gpu_dropped_total = dropped;
                }
                // R1708 §5.16 §2 #7 — same rule again: a resize folded into a
                // later one never became a frame, so it cannot live in the
                // ring, and it is exactly the work a drag's cost is made of.
                snap.resize = self.resize_tally_for_window(window_id);
                snap
            })
    }

    /// R1361 §5.16 §5.22 — publish the window's rolling frame-timing
    /// history + declared budget into the root owner, where a `view` fn
    /// reads it via [`pinion_runtime::use_frame_timings`] to draw an
    /// in-app profiler HUD.
    ///
    /// The GUI peer of [`Self::frame_timings_for_window`]: same two
    /// sources (the window's [`FrameTimingStats`] and the private
    /// `jank_budget_us_for_window`), different consumer — that one folds
    /// to aggregates for the AI-paced RPC read, this one hands over the
    /// *series* a chart plots. One budget source for both, so the line a
    /// HUD draws is the deadline the loop paces to.
    ///
    /// **Demand-gated**: a no-op unless a `view` already called
    /// `use_frame_timings` (which inserts the holder). A binding that
    /// does not chart itself pays nothing — no copy, no allocation — so
    /// the O(window) clone lands only on the window that asked for it.
    /// This is `publish_pane_viewports`' "no registered panes ⇒ return
    /// early" gate against a per-owner slot rather than a tag map.
    ///
    /// **Primary window only**, inheriting the R1006 rule: the holder is
    /// a single per-owner slot, so a secondary window's paint would
    /// clobber the primary's history and the HUD would chart a
    /// interleaving of two windows' frames. (The pane seam can publish
    /// per-window precisely because it is tag-keyed; this is not.) A HUD
    /// on a secondary window is a per-window-keyed holder — an additive
    /// axis, deferred until a consumer needs it rather than guessed at.
    ///
    /// Unlike the reactive publishes, this one **cannot mark the owner
    /// dirty** — see [`pinion_runtime::FrameTimingsHolder`] for why that
    /// would spin an idle window at 100% CPU. It is a plain overwrite;
    /// the next paint, whenever the window's cadence produces one,
    /// samples it.
    pub fn publish_frame_timings(&self, window_id: &str) {
        if window_id != pinion_runtime::DEFAULT_WINDOW {
            return;
        }
        let Some(holder) = pinion_runtime::FRAME_TIMINGS.get(self.root_owner()) else {
            return;
        };
        let budget_us = self.jank_budget_us_for_window(window_id);
        let stats = self
            .window_state(window_id)
            .and_then(|s| s.frame_timings.as_ref());
        let (samples, snapshot) = stats.map_or_else(
            || (Vec::new(), None),
            |stats| {
                (
                    stats.samples().copied().collect(),
                    stats.snapshot(budget_us),
                )
            },
        );
        holder.publish(pinion_runtime::FrameTimingsView { samples, snapshot });
    }

    /// R1036 §5.16 §5.7 §2 #7 — the window's last presented-frame
    /// [`pinion_runtime::RenderFidelity`] record (PR-17), or `None` before its
    /// first paint (`scene/render_fidelity` surfaces `RenderFidelityUnavailable`).
    /// A clone — the record is small (one entry per `TextGrid`) and the RPC read
    /// is AI-paced, not on the paint path.
    #[must_use]
    pub fn render_fidelity_for_window(
        &self,
        window_id: &str,
    ) -> Option<pinion_runtime::RenderFidelity> {
        self.window_state(window_id)
            .and_then(|s| s.render_fidelity.clone())
    }

    /// R925 §5.16 §5.7 — the per-frame budget (µs) the window's frame
    /// timings are judged against for jank classification, or `None`
    /// for an unpaced window (no declared frame target → no deadline →
    /// jank undefined).
    ///
    /// The budget *is* the window's pacing budget — it is computed from
    /// the SAME two inputs, through the SAME
    /// [`frame_budget_for_window`](pinion_runtime::frame_pacing::frame_budget_for_window)
    /// (R681) helper, that the render loop's `about_to_wait` uses to
    /// schedule the window's deadline: the per-window
    /// [`immediate_subtree`](Self::immediate_subtree_for_window) flag
    /// (an immediate-mode window paces at the default 60fps) and the
    /// optional [`target_fps`](Self::target_fps_for_window) override
    /// (which opts even a retained-tree window into polled `1/fps`).
    /// So the jank profiler reports frames against the very deadline the
    /// render loop schedules to, for every window — retained idle (no
    /// budget, jank undefined), retained-with-override, AND immediate-mode
    /// (R681/R827–R831, which paint today: `hello-immediate-mode-canvas`).
    ///
    /// A sub-microsecond budget (an absurdly high `target_fps` whose
    /// `1/fps` truncates below 1µs) maps to `None` rather than
    /// `Some(0)`: a zero budget would vacuously mark every non-instant
    /// frame janky.
    fn jank_budget_us_for_window(&self, window_id: &str) -> Option<u64> {
        let has_immediate = self.immediate_subtree_for_window(window_id);
        let override_fps = self.target_fps_for_window(window_id);
        pinion_runtime::frame_pacing::frame_budget_for_window(has_immediate, override_fps)
            .map(|d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
            .filter(|&us| us > 0)
    }

    /// R682 §5.16 atomic 3 — iterator over every window key that has
    /// a published [`FragmentCacheStats`] snapshot. Demo + test
    /// harness consume this to verify per-window publish wiring
    /// without invoking `WidgetView::windows()`.
    pub fn fragment_cache_stat_windows(&self) -> impl Iterator<Item = &str> + '_ {
        // R1193 — filter by the field's presence, NOT all `window_states` keys: a
        // window may hold a `WindowState` for another axis (e.g. maximized) with
        // no published fragment stats, and must not be reported here.
        self.window_states
            .iter()
            .filter(|(_, s)| s.fragment_cache_stats.is_some())
            .map(|(k, _)| k.as_str())
    }

    /// R51.83 §5.40 / R1072 §5.37 — the §5.36 [`LayoutCache`] (mutable) and the
    /// opt-in self-hosted [`SelfHostedTextEngine`] (shared) for the vello-side
    /// paint pipeline, borrowed as DISJOINT fields.
    ///
    /// `paint_adapter::to_vello_cached_with_text_engine` walks the paint scene,
    /// consults the cache for every `Scene::Text` node (shape once, hit on every
    /// subsequent frame), and routes eligible leaves through `engine` when it is
    /// `Some`. Returning both from one `&mut self` borrow lets the paint site
    /// pass the cache (`&mut`) and the engine (`&`) together without a `self`
    /// double-borrow. The single surface-side entry point: substrate-internal
    /// callers (`compute_paint_scene`'s layout call) use the fields directly so
    /// the surface boundary stays explicit.
    #[must_use]
    pub fn text_cache_and_engine(&mut self) -> (&mut LayoutCache, Option<&SelfHostedTextEngine>) {
        (&mut self.text_cache, self.text_engine.as_ref())
    }

    /// (R1557 §5.16 §5.18 §2 #2 §2 #7) Attribute this window's last painted
    /// frame to the subtrees that drew it — the `scene/draw_profile` producer.
    ///
    /// Re-encodes the retained paint scene with a
    /// [`DrawProfiler`](pinion_runtime::DrawProfiler) attached and answers the
    /// resulting tree: every node's inclusive and exclusive
    /// [`DrawWork`](pinion_runtime::DrawWork). `None` when the window has never
    /// painted, which is the only state that has no answer.
    ///
    /// # Why it re-encodes instead of profiling the live paint
    ///
    /// Three reasons, and they are the same reason from different sides.
    ///
    /// 1. **The live paint must not pay for it.** Attribution costs two census
    ///    reads and a `Vec` push per node. Small, and still the profiler failure
    ///    mode worth avoiding: an always-on measurement billed to every frame.
    ///    This runs only when the method is called.
    /// 2. **The answer must be decomposed.** A cache HIT is a node the walk
    ///    declines to descend through, so profiling a warm live paint attributes
    ///    a replayed subtree *whole* — correct in total and useless as a flame
    ///    graph. The scratch
    ///    [`FragmentCache`](pinion_runtime::paint_adapter::FragmentCache) here
    ///    is cold by construction, so
    ///    every container is walked and every node gets a row.
    /// 3. **It is checkable.** The re-encode draws what the live frame drew —
    ///    that is the fragment cache's own correctness invariant — so the root's
    ///    total must equal `scene/frame_timings`' `last.draw`. The demo asserts
    ///    exactly that, which turns "these two agree" from an assumption into a
    ///    guard, and incidentally makes the cache's invariant observable from
    ///    outside the crate for the first time.
    ///
    /// The live `LayoutCache` and `ImageCache` are used rather than scratch
    /// ones: shaping and image decode are what the caches exist to avoid
    /// repeating, and a scratch image cache would re-resolve every source — with
    /// a `memory://` producer image reachable only through the owner that
    /// registered it. Only the fragment cache is scratch, and only for reason 2.
    ///
    /// # Scoping (R1558)
    ///
    /// `scope` names the subtree to profile, `None` the whole window. It scopes
    /// the **measurement**: the re-encode above starts at the addressed node,
    /// so nothing outside it is encoded, walked or attributed and the cost of
    /// asking falls with the subtree. That is the axis `depth` cannot reach —
    /// `depth` shortens a reply after a full window has been re-encoded.
    ///
    /// The address is resolved through
    /// [`resolve::lookup_addressed_chain`](pinion_rpc::resolve::lookup_addressed_chain),
    /// the one resolver `scene/query` and `scene/invoke` use, so a scope is
    /// exactly the vocabulary a profile row already publishes. The chain it
    /// answers with — not the chain that was asked for — is what the profile
    /// records, because the resolver accepts a scene root named by its own tag
    /// and such a root sits at the empty chain.
    ///
    /// An address that reaches no node is an `Err`, resolved HERE and carried,
    /// rather than a `None` the dispatcher would report as "this window has no
    /// profile". This is the only place the painted scene and the address are
    /// both in hand, so it is the only place that can tell the two apart.
    pub fn draw_profile_for_window(
        &mut self,
        window_id: &str,
        image_cache: &mut pinion_runtime::image_cache::ImageCache,
        scope: Option<&pinion_rpc::draw_profile::ProfileScope>,
    ) -> Option<Result<pinion_runtime::DrawProfile, pinion_rpc::draw_profile::DrawProfileError>>
    {
        // Disjoint fields: the scene comes from `core`, the shaper cache and the
        // engine from this struct's own fields.
        let scene = self.core.last_paint_scene_for_window(window_id)?;
        let segments = scope.map_or(&[][..], |s| &s.segments[..]);
        let Some((target, chain)) = pinion_rpc::resolve::lookup_addressed_chain(scene, segments)
        else {
            return Some(Err(
                pinion_rpc::draw_profile::DrawProfileError::UnknownPath {
                    requested: scope.map(|s| s.requested.clone()).unwrap_or_default(),
                },
            ));
        };
        let chain = chain.to_vec();
        let mut fragments = pinion_runtime::paint_adapter::FragmentCache::new();
        let mut out = vello::Scene::new();
        let mut profiler = pinion_runtime::DrawProfiler::new();
        pinion_runtime::paint_adapter::to_vello_cached_profiled(
            target,
            &|_b: &pinion_core::scene::BoxNode| None,
            &mut self.text_cache,
            image_cache,
            &mut fragments,
            self.text_engine.as_ref(),
            &mut out,
            // The steady / focused defaults every non-animating producer passes:
            // a profile must not depend on which half of a cursor blink it was
            // taken in (§5.41 R1426 / R1427).
            true,
            true,
            &mut profiler,
        );
        Some(Ok(profiler.finish(chain)))
    }

    /// R1072 §5.37 / R1072.1 — TEST-ONLY injection of a specific self-hosted text
    /// engine with a bundled font fixture (system font discovery is
    /// environment-dependent and would skip the §5.37 arms on a font-less CI box).
    ///
    /// Deliberately `#[cfg(test)]`: production builds the engine ONCE at
    /// construction ([`build_text_engine_from_env`]), BEFORE any paint, so the
    /// per-window [`FragmentCache`](pinion_runtime::paint_adapter::FragmentCache)
    /// — keyed on `Scene::paint_hash`, which does NOT fold engine on/off — stays
    /// coherent (the engine choice is a per-process constant). A public
    /// *mid-session* mutator would defeat that: a fragment cached under one engine
    /// would replay under the other with no key change. A future embedded-brand-
    /// font need is therefore a CONSTRUCTION-time seam, not a runtime setter
    /// (R1072.1 adversarial-audit clearance: removed the public mutator footgun).
    #[cfg(test)]
    fn set_text_engine(&mut self, engine: Option<SelfHostedTextEngine>) {
        self.text_engine = engine;
    }

    /// R51.83 §5.40 — Shift modifier bit from the cached
    /// [`Modifiers`] state (R51.108 §5.41 winit-free).
    ///
    /// `AppShell::handle_key_press` reads the Shift bit to decide
    /// whether `Tab` calls [`Self::handle_focus_traverse`] in the
    /// reverse direction. Exposes only the bit the surface needs;
    /// the rest of the modifier state stays substrate-internal.
    #[must_use]
    pub fn modifiers_shift_key(&self) -> bool {
        self.modifiers.shift_key()
    }

    /// R51.45 §5.35 — abstract [`Touch`] dispatch (R51.108 §5.41
    /// winit-free, R51.122 §5.41 router-side lift into
    /// [`CoreShell::touch_event`]). Each finger mints a distinct
    /// [`PointerId::touch(finger_id)`] so two simultaneous touches
    /// drive two widgets without aliasing the capture lock. The
    /// substrate routes the phase-specific router calls inside
    /// [`CoreShell::touch_event`] (`Started` → `cursor_moved` +
    /// `pointer_down`; `Moved` → `cursor_moved`; `Ended` →
    /// `pointer_up` + `cursor_left`; `Cancelled` (R51.93 §5.35
    /// §5.13) → `pointer_cancel` + `cursor_left`); this method adds
    /// the Vello-only `click_to_focus` follow-up for the press phase
    /// (`Started`) so a tap on a tagged focusable widget aliases
    /// `FocusManager::focus_set`.
    /// R672 §5.35 §5.41 — per-window touch handler.
    fn handle_touch_for_window(&mut self, window_id: &str, touch: Touch) -> DispatchTail<V::State> {
        let phase = touch.phase;
        let pid = PointerId::touch(touch.id);
        let tail = self.core.touch_event_for_window(window_id, touch);
        if matches!(phase, TouchPhase::Started) {
            self.click_to_focus_for_window(window_id, pid);
        }
        tail
    }

    /// Translate a typed widget event into the symbolic
    /// `invoke("send", Text(<name>))` call — the same channel the RPC
    /// `scene/invoke` route uses. Failures from the statechart
    /// (`InvokeError::Rejected` etc.) are swallowed: the SCXML decides
    /// whether a given transition fires.
    ///
    /// R51.123 §5.41 — body delegates to [`CoreShell::forward`]
    /// (which performs the `invoke` + post-dispatch tail); the
    /// Vello-side post-tail bookkeeping (OCC revision bump per §5.34
    /// R40.4, transition-log eprintln, redraw flag on state change)
    /// happens in `Self::handle_tail`.
    pub fn forward(&mut self, event: V::Event) {
        let tail = self.core.forward(event);
        // §5.34 R40.4: winit-side input bypasses the RPC dispatcher,
        // so bump the OCC revision token directly. Spurious bumps for
        // SCXML-rejected events are acceptable per the
        // conservative-bump policy.
        self.revision.bump();
        self.handle_tail(&tail);
    }

    /// R51.37 §5.35 — route a key string through
    /// [`WidgetView::apply_key`](pinion_core::WidgetCore::apply_key) and, on handled
    /// (`Some(DispatchTail)` from [`CoreShell::apply_key`]), run the
    /// same post-input bookkeeping as [`Self::forward`]: bump the
    /// §5.34 revision, re-read cached state (paint on visible
    /// change), drain pending intents. Unhandled keys (`None` return)
    /// are swallowed quietly (same shape as an unmatched
    /// [`WidgetView::keybinding`](pinion_core::WidgetCore::keybinding)).
    pub fn apply_key(&mut self, key: &str) {
        self.apply_key_inner(key, false);
    }

    /// R1658 §5.13 §5.39 — open a keystroke delivery, so every key dispatched
    /// until the next open carries one arrival. See
    /// [`CoreShell::open_key_delivery`](pinion_runtime::CoreShell::open_key_delivery).
    ///
    /// Public because the **backend** is what knows where a platform handover
    /// begins, and the backend is outside this type: `AppShell::new_events`
    /// calls it once per winit event-loop iteration. A binding never does.
    pub fn open_key_delivery(&mut self) -> pinion_core::KeyArrival {
        self.core.open_key_delivery()
    }

    /// R1658 §5.13 §5.39 — the arrival of the delivery currently open.
    #[must_use]
    pub fn key_delivery(&self) -> pinion_core::KeyArrival {
        self.core.key_delivery()
    }

    /// R1071 PR-27 §5.39 §5.35 — body of [`Self::apply_key`] carrying the
    /// platform auto-repeat flag through to [`CoreShell::apply_key_repeat`]
    /// (and thence the binding's [`WidgetCore::apply_key_repeat`](pinion_core::WidgetCore::apply_key_repeat)). The
    /// public `apply_key` is the `repeat == false` wrapper; the GUI keyboard
    /// path ([`Self::handle_character_key_inner`]) and the per-window seam
    /// ([`Self::key_press_for_window`]) pass the real flag.
    pub(crate) fn apply_key_inner(&mut self, key: &str, repeat: bool) {
        let focused = self.focus.focused().map(str::to_owned);
        // R1658 §5.13 §5.39 — the keystroke carries the arrival of the
        // delivery the backend opened for this platform handover, NOT a fresh
        // one taken here: taking it here would date the key at the moment
        // this app got round to it, which is what the capability exists to
        // stop. `AppShell::new_events` is what opens it.
        let press = KeyPress::new(key, self.modifiers, repeat, self.core.key_delivery());
        if let Some(tail) = self.core.apply_key_press(focused.as_deref(), &press) {
            self.revision.bump();
            self.handle_tail(&tail);
        }
    }

    /// R56.2.a §5.13 §5.38 — route an IME [`CompositionEvent`](pinion_core::CompositionEvent) through
    /// [`WidgetView::apply_composition`](pinion_core::WidgetCore::apply_composition) and, on handled
    /// (`Some(DispatchTail)` from [`CoreShell::apply_composition`]),
    /// run the same post-input bookkeeping as [`Self::apply_key`]:
    /// bump the §5.34 revision, drain pending intents via
    /// `Self::handle_tail` (which re-reads cached state and
    /// requests a redraw on visible change).
    ///
    /// pinion-shell's `AppShell::window_event` `WindowEvent::Ime`
    /// arm converts winit 0.30's cross-platform
    /// [`Ime`](https://docs.rs/winit/0.30/winit/event/enum.Ime.html)
    /// enum (`Enabled` / `Preedit(text, range)` / `Commit(text)` /
    /// `Disabled`) into [`CompositionEvent`](pinion_core::CompositionEvent) with a `was_composing`
    /// state machine, then forwards through this method — see the
    /// `AppShell` doc comment on the arm for the mapping table.
    ///
    /// Unhandled composition events (`None` return — every widget
    /// except `TextField` and friends) are swallowed quietly; the
    /// shell does not fall through to any scroll/typeahead/other arc
    /// because composition events have no meaningful fallback (the
    /// W3C contract is "widget consumes preedit or nothing else
    /// applies").
    pub fn apply_composition(&mut self, event: &pinion_core::CompositionEvent) {
        let focused = self.focus.focused().map(str::to_owned);
        if let Some(tail) = self.core.apply_composition(focused.as_deref(), event) {
            self.revision.bump();
            self.handle_tail(&tail);
        }
    }

    /// R56.2.e §5.13 §5.22 — route a middle-*click* through
    /// [`WidgetView::apply_middle_click`](pinion_core::WidgetCore::apply_middle_click) and, on handled
    /// (`Some(DispatchTail)` from [`CoreShell::apply_middle_click`]),
    /// run the same post-input bookkeeping as [`Self::apply_key`]:
    /// bump the §5.34 revision, drain pending intents via
    /// `Self::handle_tail` (which re-reads cached state and
    /// requests a redraw on visible change).
    ///
    /// R881 §5.35 — this is the paste *funnel*, no longer a press arm:
    /// [`Self::middle_released_for_window`] calls it when the router's
    /// `DragLatch` resolves the middle press as a release-in-place
    /// ([`PanRelease::Click`]). A press that strayed into a
    /// drag-to-pan never reaches here (pre-R881 the winit
    /// `{ Middle, Pressed }` arm called this directly, pasting at
    /// press time).
    ///
    /// On the X11 / Wayland Linux desktops the canonical UX is
    /// "middle-click pastes the PRIMARY selection at the focused
    /// text widget"; `TextField::apply_middle_click` reads PRIMARY
    /// via the R56.2.e [`Clipboard::paste_from`](pinion_core::Clipboard::paste_from) extension and
    /// inserts at the caret. On macOS / Windows the
    /// [`Clipboard::paste_from`](pinion_core::Clipboard::paste_from) default impl returns `None` for
    /// `Primary` so the widget impl harmlessly produces a no-op
    /// (matching the OS-level absence of a parallel selection
    /// clipboard).
    ///
    /// Unhandled middle-clicks (`None` return — every non-text-input
    /// widget) are swallowed quietly; the shell does not fall
    /// through to any other input arc because middle-click has no
    /// meaningful fallback at the substrate level (a Vello `External`
    /// widget that wants raw `MouseButton::Middle` events can wire
    /// its own R56.2.a-style platform override, the same way IME
    /// fans out from this surface).
    pub fn middle_click(&mut self) {
        let focused = self.focus.focused().map(str::to_owned);
        if let Some(tail) = self
            .core
            .apply_middle_click(focused.as_deref(), self.modifiers)
        {
            self.revision.bump();
            self.handle_tail(&tail);
        }
    }

    /// R881 §5.35 §5.49 — winit `MouseInput { Middle, Pressed }`
    /// dispatch. Opens the router's middle-button gesture for the
    /// addressed window: pan targets (hover `External` + deepest
    /// scrollable) are pinned at the press point, and the paste that
    /// pre-R881 fired here is deferred until the [`pinion_runtime::InputRouter`]
    /// resolves the press as a click-in-place — see
    /// [`Self::middle_released_for_window`].
    ///
    /// Single-window wrapper: [`Self::middle_pressed`].
    pub fn middle_pressed_for_window(&mut self, window_id: &str, pid: PointerId) {
        self.core.middle_down_for_window(window_id, pid);
    }

    /// R881 §5.35 — single-window wrapper around
    /// [`Self::middle_pressed_for_window`].
    pub fn middle_pressed(&mut self, pid: PointerId) {
        self.middle_pressed_for_window(pinion_runtime::DEFAULT_WINDOW, pid);
    }

    /// R881 §5.35 §5.49 — winit `MouseInput { Middle, Released }`
    /// dispatch. Closes the router's middle gesture and acts on its
    /// click-vs-pan determination (the R880 `DragLatch` SSOT, judged
    /// router-side):
    ///
    /// * [`PanRelease::Click`] — press-release in place: run the
    ///   R56.2.e paste funnel ([`Self::middle_click`]). Release-paste is
    ///   the xterm / the toolkit convention and keeps a paste off every pan.
    /// * [`PanRelease::Pan`] — the drag already panned move-by-move;
    ///   nothing fires on release.
    /// * [`PanRelease::NoPress`] — spurious release, or the gesture
    ///   was revoked by `PointerCancel`; a cancelled press must not
    ///   paste.
    ///
    /// Single-window wrapper: [`Self::middle_released`].
    pub fn middle_released_for_window(&mut self, window_id: &str, pid: PointerId) {
        match self.core.middle_up_for_window(window_id, pid) {
            PanRelease::Click => self.middle_click(),
            PanRelease::Pan | PanRelease::NoPress => {}
        }
    }

    /// R881 §5.35 — single-window wrapper around
    /// [`Self::middle_released_for_window`].
    pub fn middle_released(&mut self, pid: PointerId) {
        self.middle_released_for_window(pinion_runtime::DEFAULT_WINDOW, pid);
    }

    /// R772 §5.53 §5.38 — route a secondary-button (right-click) press
    /// through [`WidgetView::apply_secondary_click`](pinion_core::WidgetCore::apply_secondary_click), anchoring a context
    /// menu at the cursor. Reads the addressed window's cached cursor
    /// position (`CoreShell::cursor_position_for_window`, the channel
    /// `position_caret_for_point` uses) and forwards it to
    /// [`CoreShell::apply_secondary_click`]; on handled
    /// (`Some(DispatchTail)`) it runs the same post-input bookkeeping as
    /// [`Self::middle_click`] — bump the §5.34 revision, drain intents via
    /// `Self::handle_tail` (re-reads cached state, redraws on change).
    ///
    /// pinion-shell's `AppShell::window_event`
    /// `WindowEvent::MouseInput { button: Right, state: Pressed, .. }` arm
    /// calls this with the event's window so the popup opens on the right
    /// surface (winit normalises X11 / Wayland / macOS / Windows
    /// secondary-button presses under one enum). A press before any
    /// `cursor_moved` (no cached position) is swallowed quietly.
    pub fn secondary_click_for_window(&mut self, window_id: &str, pid: PointerId) {
        let Some((x, y)) = self.core.cursor_position_for_window(window_id, pid) else {
            return;
        };
        #[allow(
            clippy::cast_possible_truncation,
            reason = "window-local logical-pixel cursor coords fit f32 in every realistic viewport"
        )]
        if let Some(tail) = self.core.apply_secondary_click(x as f32, y as f32) {
            self.revision.bump();
            self.handle_tail(&tail);
        }
    }

    /// R1416 §5.35 §5.15 §2 #2 §2 #6 — the ONE dispatch seam for a mouse-button
    /// EDGE (left / middle / right × press / release), shared by the native
    /// winit `MouseInput` path ([`AppShell::handle_mouse_button`](crate::AppShell))
    /// and the RPC `scene/pointer_button` drain per
    /// [[r47-class-incident-prevention]] (native and RPC MUST reach one
    /// dispatch path so their behaviour cannot diverge).
    ///
    /// A widget that owns the RAW multi-button pointer stream — a terminal pane
    /// forwarding xterm mouse reports, a game viewport (opts in via
    /// [`External::wants_raw_pointer_buttons`](pinion_core::External::wants_raw_pointer_buttons))
    /// — is offered the edge FIRST and receives it verbatim (button +
    /// press/release + held modifiers), with the GUI default suppressed. A
    /// `true` (consumed) bumps the §5.34 revision + requests a redraw (the
    /// `External` mutated its own state — no [`DispatchTail`])
    /// and returns.
    ///
    /// EVERY other widget keeps the standard per-button semantics unchanged —
    /// the non-capture invariant:
    ///
    ///   * left press / release → the pointer down / up arc (focus, select,
    ///     capture-lock, drag-and-drop, text-select),
    ///     [`Self::mouse_pressed_for_window`] / [`Self::mouse_released_for_window`].
    ///   * middle press / release → the R881 middle gesture pair (drag-to-pan,
    ///     paste-on-release-in-place), [`Self::middle_pressed_for_window`] /
    ///     [`Self::middle_released_for_window`].
    ///   * right press → the R772 own-renderer context-menu open,
    ///     [`Self::secondary_click_for_window`]. Right release has no GUI arm:
    ///     the context menu is a press-edge one-shot (winit never carried a
    ///     right-release action), so a non-raw right release is a no-op — the
    ///     pre-R1416 `_ => {}` behaviour, now a reachable arm but unchanged.
    pub fn pointer_button_for_window(
        &mut self,
        window_id: &str,
        button: pinion_core::PointerButton,
        edge: pinion_core::PointerEdge,
    ) {
        use pinion_core::{PointerButton, PointerEdge};
        // R1619 §5.35 — record the EDGE before any routing decision. This is
        // the one seam every button edge crosses (native `MouseInput` and the
        // `scene/pointer_button` RPC drain), and the routing below is
        // per-button and asymmetric — a right release has no arm at all, a raw
        // sink consumes the edge and returns early — so a set assembled from
        // the routing arms alone would be missing exactly the buttons whose
        // arms do the least. The note is idempotent, so the arms that DO reach
        // the router and note their own edge stay correct.
        self.core
            .note_pointer_button_for_window(window_id, PointerId::MOUSE, button, edge);
        if self.core.raw_pointer_button_for_window(
            window_id,
            PointerId::MOUSE,
            button,
            edge,
            self.modifiers,
        ) {
            self.revision.bump();
            self.request_redraw_for_window(window_id);
            return;
        }
        match (button, edge) {
            (PointerButton::Left, PointerEdge::Down) => {
                self.mouse_pressed_for_window(window_id, PointerId::MOUSE);
            }
            (PointerButton::Left, PointerEdge::Up) => {
                self.mouse_released_for_window(window_id, PointerId::MOUSE);
            }
            (PointerButton::Middle, PointerEdge::Down) => {
                self.middle_pressed_for_window(window_id, PointerId::MOUSE);
            }
            (PointerButton::Middle, PointerEdge::Up) => {
                self.middle_released_for_window(window_id, PointerId::MOUSE);
            }
            (PointerButton::Right, PointerEdge::Down) => {
                self.secondary_click_for_window(window_id, PointerId::MOUSE);
            }
            (PointerButton::Right, PointerEdge::Up) => {}
        }
    }

    /// R51.78 §5.39 — Tab / Shift+Tab dispatch decoupled from winit.
    ///
    /// `AppShell::handle_key_press` (winit-side) maps
    /// `Key::Named(NamedKey::Tab) + modifiers.shift_key()` into a
    /// boolean `shift` flag and forwards here. The substrate then
    /// invokes [`FocusManager::focus_next`] / [`FocusManager::focus_prev`]
    /// against the scene-derived `tab_order` and requests a
    /// redraw when the focused tag actually changed (avoiding
    /// no-op repaints when Tab cycles back to a one-tag list).
    ///
    /// Returns the underlying `FocusManager` change flag for
    /// callers / tests that want to assert on the cycle behaviour.
    pub fn handle_focus_traverse(&mut self, shift: bool) -> bool {
        let focus_before = self.focus.focused().map(str::to_owned);
        let changed = if shift {
            self.focus.focus_prev()
        } else {
            self.focus.focus_next()
        };
        if changed {
            // R56.1.h §5.38 §5.39 — Tab / Shift+Tab traversal notifies
            // the outgoing + incoming externals of the focus change
            // before requesting the redraw so the `TextField` statechart
            // (R56.1.a) drives its Focus / Blur events in the same
            // frame that paints the new focus ring.
            self.notify_focus_change(focus_before.as_deref());
            self.request_redraw();
        }
        changed
    }

    /// R51.78 §5.37 — `Key::Character` dispatch decoupled from winit.
    ///
    /// First consults [`WidgetView::keybinding`](pinion_core::WidgetCore::keybinding); on `Some(event)`
    /// routes through [`Self::forward`] (typed event channel). On
    /// `None` falls through to [`Self::apply_key`] (raw key-string
    /// dispatch). Matches the pre-R51.78 inline behaviour in
    /// `AppShell::handle_key_press` byte-for-byte.
    pub fn handle_character_key(&mut self, c: &str) {
        self.handle_character_key_inner(c, false);
    }

    /// R1071 PR-27 §5.39 §5.35 — body of [`Self::handle_character_key`]
    /// carrying the platform auto-repeat flag. The typed-event
    /// ([`Self::forward`]) branch is repeat-agnostic (a `keybinding`-mapped
    /// character has no auto-repeat distinction); the raw-key branch threads
    /// `repeat` to [`Self::apply_key_inner`] so a binding that overrides
    /// [`WidgetCore::apply_key_repeat`](pinion_core::WidgetCore::apply_key_repeat) sees it. The public method is the
    /// `repeat == false` wrapper.
    pub(crate) fn handle_character_key_inner(&mut self, c: &str, repeat: bool) {
        // R1543 §5.39 — a character held with Alt is an ACCELERATOR, not text,
        // so the mnemonic arc runs ahead of both the typed-event channel and
        // the focused widget. This is the toolkit's own precedence (shortcut
        // map runs before key delivery).
        //
        // R1569 §5.39 CORRECTION — R1543 wrote here that the toolkit's opt-out
        // event (`ShortcutOverride`) was, in pinion, "the absence of a declaration — a widget
        // that wants Alt+F for itself simply does not publish a mnemonic on
        // it". That escape is available only to a widget that OWNS the
        // colliding declaration, and the two widgets that need the hatch own
        // nothing of the sort: a text field does not declare the binding's
        // `keybinding("d")`, and a key-sequence editor does not declare the File menu's
        // Alt+F. The consequence shipped — typing `d` into `hello-textfield`'s focused field
        // disabled the field, because `V::keybinding` below runs ahead of the focused
        // widget too. Both accelerator layers are now gated on the same
        // question.
        let chord = Chord::new(c, self.modifiers);
        if self
            .core
            .accelerator_shadow(self.focus.focused(), &chord)
            .is_none()
        {
            if self.try_mnemonic(&chord) {
                return;
            }
            if let Some(ev) = V::keybinding(c) {
                self.forward(ev);
                return;
            }
        }
        self.apply_key_inner(c, repeat);
    }

    /// R1543 §5.39 — offer `c` to the §5.20 mnemonic arc, returning whether a
    /// declared accelerator claimed it.
    ///
    /// Gated on the Alt modifier read from the out-of-band
    /// [`Self::modifiers`] cache (winit's `KeyEvent` carries no modifier
    /// state), so an RPC driver sets it with
    /// [`Self::set_modifiers`] before injecting the character — the same
    /// discipline every other modifier-bearing shortcut here follows.
    ///
    /// Non-single-codepoint input cannot be a mnemonic: `c` is the
    /// `Key::Character` arc's payload, and the R666 auto-discriminator already
    /// routes multi-char strings to the named-key arc, so this rejects only
    /// malformed injections.
    fn try_mnemonic(&mut self, chord: &Chord) -> bool {
        if !chord.modifiers().alt {
            return false;
        }
        let Some(tail) = self.core.dispatch_mnemonic_for_window(
            pinion_runtime::DEFAULT_WINDOW,
            chord,
            &mut self.focus,
        ) else {
            return false;
        };
        self.revision.bump();
        self.request_redraw();
        self.handle_tail(&tail);
        true
    }

    /// R51.78 §5.37 — `Key::Named` dispatch decoupled from winit.
    ///
    /// `AppShell::handle_key_press` (winit-side) maps the winit
    /// `NamedKey` enum to the W3C `KeyboardEvent.key` string via
    /// the module-private `named_key_str` bridge and forwards the
    /// resulting `&'static str` here. The substrate routes through
    /// [`Self::apply_key`]; widgets match on the W3C string in their
    /// `apply_key` impls.
    ///
    /// # `Escape` / `Tab` reach this method from RPC only (R1364.5)
    ///
    /// A PHYSICAL `Escape` / `Tab` never arrives here. `AppShell::handle_key_press`
    /// gives each its own arm — `try_apply_key_inner("Escape" | "Tab", ..)`
    /// directly, then `request_quit` / [`Self::handle_focus_traverse`] on decline
    /// — and only the remaining named keys fall through to this method. Both
    /// still reach `V::apply_key`, just not by this door.
    ///
    /// An INJECTED one always arrives here: `Self::drain_key_for_window`, the RPC
    /// `scene/key` arm, calls this method, and `handle_scene_key` imposes no
    /// allowlist. Pinned by `dispatch_core.rs`'s
    /// `r1364_shell_reserved_keys_are_injectable`.
    ///
    /// This sentence has now been wrong twice, which is why it is spelled out.
    /// Pre-R1364 it read "`Tab` never reaches this method" — right about the
    /// winit path, wrong about RPC. R1364.3 "corrected" it to say a physical Tab
    /// arrives when the widget declines — which is FALSE, since declining is
    /// exactly what routes it to `handle_focus_traverse` instead. A half-truth
    /// was replaced by a falsehood, in the round about falsehoods; an independent
    /// reviewer caught it.
    ///
    /// Consequences per key:
    ///
    /// * `Tab` injected here reaches the focused widget and does not go on to
    ///   traverse focus. §2 #2 holds anyway: `focus/next` / `focus/prev` drive
    ///   the same `FocusManager`, so an AI asks for traversal by name.
    /// * `Escape` injected here cannot END the app: that needs an
    ///   `&ActiveEventLoop`, and this substrate is winit-free for the §2 #6 dual.
    ///   R1363 §5.55 — an unconsumed PHYSICAL Escape is an app QUIT via
    ///   `AppShell::request_quit` (through the `app_quit_requested` veto), not a
    ///   window close. The AI's peer is `app/quit`, which passes that same veto.
    pub fn handle_named_key(&mut self, key_str: &str) {
        self.handle_named_key_inner(key_str, false);
    }

    /// R1071 PR-27 §5.39 §5.35 — body of [`Self::handle_named_key`] carrying
    /// the platform auto-repeat flag through to the widget arc
    /// ([`Self::try_apply_key_inner`] → [`WidgetCore::apply_key_repeat`](pinion_core::WidgetCore::apply_key_repeat)).
    /// The scroll-routing fallback is deliberately repeat-agnostic: a held
    /// arrow / `PageDown` over a scroll container SHOULD keep scrolling, so
    /// the flag only reaches the widget that may want to suppress it. The
    /// public method is the `repeat == false` wrapper.
    pub(crate) fn handle_named_key_inner(&mut self, key_str: &str, repeat: bool) {
        // R51.187 §5.45 R55.C.3 — give `V::apply_key` the first
        // chance on the key (widget-bound shortcut: Slider's
        // arrows, Toggle's Space, Button's Enter, etc.). If the
        // widget reports unhandled, fall through to the scroll-
        // routing dispatch so an unbound arrow / page / Home / End
        // over a scroll container still scrolls. The two arcs are
        // mutually exclusive — a widget that consumes the key
        // never lets the scroll arc fire.
        if !self.try_apply_key_inner(key_str, repeat) {
            self.scroll_key(PointerId::MOUSE, key_str);
        }
    }

    /// R695 §5.35 — offer `key_str` to the focused widget's
    /// [`WidgetView::apply_key`](pinion_core::WidgetCore::apply_key) and run the post-input bookkeeping
    /// (revision bump + `Self::handle_tail`) when it handles the key.
    /// Returns whether the widget consumed it.
    ///
    /// Split out of [`Self::handle_named_key`] so the winit `Escape`
    /// arc can offer the key to the widget (the `Tooltip`'s WCAG 1.4.13
    /// dismiss, the `Dialog`'s modal cancel) *before* the shell's
    /// standalone-app quit fallback — without the scroll-routing
    /// fallthrough that an unhandled arrow / page key wants but an
    /// unhandled `Escape` does not.
    pub fn try_apply_key(&mut self, key_str: &str) -> bool {
        self.try_apply_key_inner(key_str, false)
    }

    /// R1071 PR-27 §5.39 §5.35 — body of [`Self::try_apply_key`] carrying the
    /// platform auto-repeat flag through to [`CoreShell::apply_key_repeat`]
    /// (and the binding's [`WidgetCore::apply_key_repeat`](pinion_core::WidgetCore::apply_key_repeat)). The public
    /// method is the `repeat == false` wrapper; the GUI named-key path
    /// ([`Self::handle_named_key_inner`]), the Escape / Tab offer-first arcs,
    /// and the per-window seam ([`Self::key_press_for_window`]) pass the real
    /// flag.
    pub(crate) fn try_apply_key_inner(&mut self, key_str: &str, repeat: bool) -> bool {
        let focused = self.focus.focused().map(str::to_owned);
        // R1658 — same rule as `apply_key_inner`: the arrival is the open
        // delivery's, not one taken at dispatch.
        let press = KeyPress::new(key_str, self.modifiers, repeat, self.core.key_delivery());
        if let Some(tail) = self.core.apply_key_press(focused.as_deref(), &press) {
            self.revision.bump();
            // R705.1 — `handle_tail` arms `redraw_requested` whenever the
            // dispatch dirtied a view-subscribed `Signal` (the reactive
            // `Owner::is_dirty()` bridge), so an arrow-key focused-row move
            // with no SCXML `state_change` still repaints. No manual arm
            // needed here.
            self.handle_tail(&tail);
            true
        } else {
            false
        }
    }

    /// (R51.187 §5.45 R55.C.3) Keyboard scroll dispatch — the
    /// fallback path [`Self::handle_named_key`] takes when
    /// [`WidgetView::apply_key`](pinion_core::WidgetCore::apply_key) reports the key unhandled.
    /// Forwards through [`CoreShell::scroll_key`](pinion_runtime::CoreShell::scroll_key)
    /// which walks the deepest [`Scene::Scroll`]
    /// under the pointer cursor and calls
    /// [`ScrollState::scroll_by`](pinion_core::widgets::scroll::ScrollState::scroll_by)
    /// / [`scroll_to`](pinion_core::widgets::scroll::ScrollState::scroll_to)
    /// depending on the key name. Requests a redraw on actual
    /// dispatch so the new offset paints next frame.
    pub fn scroll_key(&mut self, pid: PointerId, key: &str) {
        let (tail, dispatched) = self.core.scroll_key(pid, key);
        if dispatched {
            self.request_redraw();
        }
        self.handle_tail(&tail);
    }

    /// R770 §5.15 — run an OS file-drag/drop `WidgetView` hook in the
    /// root-owner scope and request a redraw on the addressed window if
    /// the binding handled it. Shared mechanics for the three
    /// `file_*_for_window` entry points (the winit + RPC arcs both reach
    /// these): snapshot the `Copy` cached state, run the hook under
    /// `root_owner.run` (so `use_*` reactive hooks resolve, mirroring the
    /// `V::position_caret_for_point` press path), redraw if `true`.
    ///
    /// R1437 §5.16 — the hook receives `window_id` as an argument rather
    /// than capturing it at the call site, so the id the binding routes on
    /// and the id the shell repaints are the same value by construction: a
    /// future entry point cannot hand the hook one window and redraw
    /// another.
    fn run_file_hook(
        &mut self,
        window_id: &str,
        hook: impl FnOnce(&str, &<V as pinion_core::WidgetCore>::State) -> bool,
    ) {
        let state = *self.cached_state();
        let owner = self.root_owner().clone();
        if owner.run(|| hook(window_id, &state)) {
            self.request_redraw_for_window(window_id);
        }
    }

    /// R770 §5.15 — winit `HoveredFile` / `scene/hover_file` entry: a file
    /// is dragged over `window_id`. Routes `window_id` + `path` to
    /// [`WidgetView::on_file_hover`].
    pub fn file_hover_for_window(&mut self, window_id: &str, path: &str) {
        self.run_file_hook(window_id, |wid, state| V::on_file_hover(wid, state, path));
    }

    /// R770 §5.15 — winit `HoveredFileCancelled` / `scene/hover_file_cancel`
    /// entry: a file drag left `window_id` without dropping. Routes to
    /// [`WidgetView::on_file_hover_cancel`].
    pub fn file_hover_cancel_for_window(&mut self, window_id: &str) {
        // No closure: the cancel hook's signature IS the hook shape — it
        // takes the window and the state and nothing else (the OS reports
        // neither a path nor a position on cancel).
        self.run_file_hook(window_id, V::on_file_hover_cancel);
    }

    /// R770 §5.15 — winit `DroppedFile` / `scene/drop_file` entry: a file
    /// was dropped on `window_id`. Routes `window_id` + `path` to
    /// [`WidgetView::on_file_drop`].
    pub fn file_drop_for_window(&mut self, window_id: &str, path: &str) {
        self.run_file_hook(window_id, |wid, state| V::on_file_drop(wid, state, path));
    }

    /// R51.80 §5.35 — winit `CursorMoved` dispatch decoupled from
    /// winit at the [`ShellCore`] surface. Forwards through
    /// [`CoreShell::cursor_moved`] (which performs the router walk +
    /// post-dispatch tail), then routes the tail through
    /// `Self::handle_tail`.
    ///
    /// R672 §5.35 — single-window wrapper around
    /// [`Self::cursor_moved_for_window`].
    pub fn cursor_moved(&mut self, pid: PointerId, x: f64, y: f64) {
        self.cursor_moved_for_window(pinion_runtime::DEFAULT_WINDOW, pid, x, y);
    }

    /// (R1120 §5.51 PR-39) Whether a drag this window owns is in flight — the
    /// gate `AppShell` uses before stamping the live window origins (so idle
    /// hovers skip the winit `inner_position()` query). Delegates to the runtime
    /// core; the shell layer holds the winit handle, the core holds the session.
    #[must_use]
    pub fn drag_session_active_for_window(&self, window_id: &str, pid: PointerId) -> bool {
        self.core.drag_session_active_for_window(window_id, pid)
    }

    /// (R1147 §5.51 §5.16) The dragged payload's label + window-local cursor for
    /// the drag a window owns, or `None` when none is in flight. `AppShell` reads
    /// it to drive the cross-desktop drag preview window (the same source the
    /// in-window `apply_drag_image` overlay reads). Delegates to the runtime core.
    #[must_use]
    pub fn active_drag_label_for_window(
        &self,
        window_id: &str,
        pid: PointerId,
    ) -> Option<(String, (f64, f64))> {
        self.core.active_drag_label_for_window(window_id, pid)
    }

    /// R1147 §5.51 §5.16 — flag whether the shell's cross-desktop drag PREVIEW
    /// window is currently showing the active drag, so `apply_drag_image`
    /// suppresses the in-window overlay (one chip, not two). `AppShell` drives
    /// the lifecycle; delegates to the runtime core.
    pub fn set_desktop_drag_preview_active(&self, active: bool) {
        self.core.set_desktop_drag_preview_active(active);
    }

    /// R1148 §5.51 §5.16 → R1151 — stamp every live window's ACTUAL client origin
    /// (logical px) so the LIVE cross-window redock resolution maps the desktop
    /// cursor against real positions, not the DECLARED ones (a WM-placed `"main"` has
    /// declared position `None` → `(0,0)`, which put redock off by the WM offset).
    /// `AppShell` calls this each cursor move during a drag (it holds the winit
    /// handles); delegates to the runtime core.
    pub fn set_live_window_origins(&self, origins: Vec<(String, (f64, f64))>) {
        self.core.set_live_window_origins(origins);
    }

    /// R1576 §5.16 §5.41 §2 #7 — stamp the monitors attached right now.
    ///
    /// [`crate::AppShell`] calls this at RPC dispatch entry (it holds the winit
    /// handles); the substrate is backend-agnostic and never reads the window
    /// system itself. The same seam [`Self::set_live_window_origins`] uses.
    pub fn set_display_topology(&self, topology: pinion_core::display::DisplayTopology) {
        *self.display_topology.borrow_mut() = Some(topology);
    }

    /// R1576 — the stamped topology, or `None` when the surface never stamped
    /// one. See the [`Self::set_display_topology`] doc for why the two are
    /// distinct facts.
    #[must_use]
    pub fn display_topology(&self) -> Option<pinion_core::display::DisplayTopology> {
        self.display_topology.borrow().clone()
    }

    /// R1610 §5.16 §2 #7 — stamp the windowing system this process is talking to.
    ///
    /// [`crate::AppShell`] calls this once the event loop exists (it is what
    /// knows); the substrate stays backend-agnostic. The seam
    /// [`Self::set_display_topology`] already uses — and stamped once rather
    /// than per dispatch, because unlike the monitor list, which changes when
    /// somebody plugs a cable in, a process does not migrate between X11 and
    /// Wayland while running.
    pub fn set_windowing_backend(&self, backend: pinion_core::window_level::WindowingBackend) {
        *self.windowing_backend.borrow_mut() = Some(backend);
    }

    /// R1610 — the stamped windowing backend, or `None` when nobody stamped one.
    ///
    /// `None` is a real answer, not a missing one: a TUI surface or a fixture
    /// has no windowing system, so `scene/windows` reports no level outcome
    /// rather than deriving one against a backend nobody looked at. Same
    /// discipline as [`Self::display_topology`].
    #[must_use]
    pub fn windowing_backend(&self) -> Option<pinion_core::window_level::WindowingBackend> {
        *self.windowing_backend.borrow()
    }

    /// R1617 §5.16 §5.41 §2 #7 — stamp, per live window, its actual outer
    /// rectangle and the display the window system says it is on.
    ///
    /// Replaces the whole map rather than merging: a window the surface no
    /// longer reports is one whose home nobody can vouch for any more, and
    /// leaving a stale entry behind would publish yesterday's answer as
    /// today's. The same wholesale discipline
    /// [`Self::set_live_window_origins`] uses.
    pub fn set_window_homes(
        &self,
        homes: Vec<(
            String,
            pinion_core::display::DisplayRect,
            Option<pinion_core::display::DisplayId>,
        )>,
    ) {
        *self.window_homes.borrow_mut() = homes
            .into_iter()
            .map(|(id, rect, platform)| (id, (rect, platform)))
            .collect();
    }

    /// R1617 — where `window_id` is, per both answerers, against the topology
    /// stamped alongside. `None` when nobody looked: no surface stamped a
    /// topology, or this window reported no outer position.
    ///
    /// Derived on every read rather than stored, so it cannot become a second,
    /// stale copy of where the window is — the one-fact-one-source argument
    /// `anchored` and `level_outcome` are both built on.
    #[must_use]
    pub fn window_display_home(
        &self,
        window_id: &str,
    ) -> Option<pinion_core::display::DisplayHome> {
        let topology = self.display_topology.borrow();
        let topology = topology.as_ref()?;
        let homes = self.window_homes.borrow();
        let (rect, platform) = homes.get(window_id)?;
        Some(topology.home_of(*rect, platform.clone()))
    }

    /// R1148 §5.51 §5.16 → R1151 — the stamped ACTUAL client origin of `window_id`
    /// (logical px), or `None` when unstamped. Delegates to the runtime core. The
    /// R1147 drag-preview window reads it to place the chip at the true desktop
    /// pointer (`client_origin + client_cursor`) — the SAME live-origin SSOT the
    /// cross-window redock resolves against, so the two never drift.
    #[must_use]
    pub fn live_window_origin(&self, window_id: &str) -> Option<(f64, f64)> {
        self.core.live_window_origin(window_id)
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::cursor_moved`].
    /// `AppShell::window_event` dispatches winit `CursorMoved` here
    /// with the resolved [`crate::WindowSpec::id`] so the addressed
    /// window's [`pinion_runtime::InputRouter`] handles the
    /// cursor + `refresh_hover` walk independently of other windows.
    pub fn cursor_moved_for_window(&mut self, window_id: &str, pid: PointerId, x: f64, y: f64) {
        // R1102 §5.51 PR-33 — when a drag this window owns is in flight, the
        // shell (the sole holder of every window's geometry) resolves the
        // cross-window drop for the NEW cursor and stashes it on the session, so
        // the per-window (cross-window-blind) router can fill
        // `DragUpdate.over_window` for a redock into another window. Resolved
        // BEFORE the move dispatches the drag update below. Gated on an active
        // drag, so idle hovers and single-window apps pay nothing.
        if self.core.drag_session_active_for_window(window_id, pid) {
            let cross = self.resolve_cross_window_live(window_id, (x, y));
            let new_target = cross.as_ref().map(|c| c.window.clone());
            self.core
                .set_drag_cross_window_for_window(window_id, pid, cross);
            // R1150 §5.51 — the R1137 SOURCE-window (floater) per-move repaint was
            // REMOVED with the on-floater redock hint it served: the floater's
            // content is static during its release-only drag, so repainting it each
            // move was dead work once the hint is gone. The TARGET-window repaint
            // below stays — it drives the on-target preview live.
            // R1125 §5.51 PR-33 — keep the incoming drop-zone PREVIEW live. The
            // source window's drag moves do not dirty the TARGET window, so the
            // shell repaints it here: the current target every move (its strip
            // follows the cursor / zone), and the PREVIOUS target on a change (so
            // its strip clears when the cursor crosses to another window or leaves
            // every window). No-op for a single-window drag (target stays `None`).
            if let Some(target) = &new_target {
                self.request_redraw_for_window(target);
            }
            if self.cross_preview_target.as_deref() != new_target.as_deref() {
                if let Some(prev) = self.cross_preview_target.take() {
                    self.request_redraw_for_window(&prev);
                }
                self.cross_preview_target = new_target;
            }
        }
        // R881 §5.35 §5.49 — thread the out-of-band modifier cache so a
        // live middle pan's wheel-vocabulary dispatch sees held chords
        // (`Ctrl`+middle-drag zooms a canvas exactly as `Ctrl`+wheel
        // does); the returned flag is the pan repaint cue, mirroring
        // `wheel_for_window`.
        let (tail, pan_dispatched) =
            self.core
                .cursor_moved_for_window_with_modifiers(window_id, pid, x, y, self.modifiers);
        if pan_dispatched {
            self.request_redraw();
        }
        self.handle_tail(&tail);
        // R763 §5.36 §5.22 — extend an in-flight pointer text selection
        // to the new cursor byte (no-op unless a press armed a drag).
        self.extend_text_selection_on_drag(window_id, pid, x, y);
    }

    /// R51.80 §5.35 — winit `CursorLeft` dispatch decoupled from
    /// winit at the [`ShellCore`] surface.
    ///
    /// R672 §5.35 — single-window wrapper around
    /// [`Self::cursor_left_for_window`].
    pub fn cursor_left(&mut self, pid: PointerId) {
        self.cursor_left_for_window(pinion_runtime::DEFAULT_WINDOW, pid);
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::cursor_left`].
    pub fn cursor_left_for_window(&mut self, window_id: &str, pid: PointerId) {
        let tail = self.core.cursor_left_for_window(window_id, pid);
        self.handle_tail(&tail);
    }

    /// R51.80 §5.35 — winit `MouseInput { Pressed, Left }` dispatch.
    /// Combines [`CoreShell::pointer_down`] with the §5.39
    /// click-to-focus rule (the same path
    /// [`TouchPhase::Started`] runs after a synthetic cursor move).
    ///
    /// R672 §5.35 — single-window wrapper around
    /// [`Self::mouse_pressed_for_window`].
    pub fn mouse_pressed(&mut self, pid: PointerId) {
        self.mouse_pressed_for_window(pinion_runtime::DEFAULT_WINDOW, pid);
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::mouse_pressed`].
    /// `click_to_focus_for_window` reads the addressed window's
    /// `hover_target` so focus targets the right widget on the right
    /// window — multi-window bindings without this fix would focus
    /// the *binding-wide* hover target (whichever window the cursor
    /// last hovered).
    pub fn mouse_pressed_for_window(&mut self, window_id: &str, pid: PointerId) {
        // R882 / R882.1 §5.35 §5.39 — the press routes through the substrate's
        // LEFT front door ([`CoreShell::left_press_for_window`]): the Space-hold pan chord (the design tool /
        // the raster editor hand tool) and the live-pan swallow are substrate
        // policy, owned ONCE in `CoreShell` (the R882 first cut kept the branch per
        // shell — the §2 #6 divergence class). `None` = the pan channel consumed
        // the press: no widget `PointerDown` and none of the press follow-ups below run
        // — no click-to-focus (a pan must not steal focus), no caret
        // positioning, no immediate-mode forward.
        let Some(tail) = self.core.left_press_for_window(window_id, pid) else {
            return;
        };
        self.click_to_focus_for_window(window_id, pid);
        self.position_caret_after_press(window_id, pid);
        self.forward_pointer_down_to_immediate(window_id, pid);
        self.handle_tail(&tail);
    }

    /// R830 §2 #4 §5.15 — player → game pointer input forwarding. When a
    /// press resolves (via the router's paint-scene hit-test) to an
    /// [`pinion_core::scene::ImmediateModeNode`] viewport, forward it to
    /// that driver's [`pinion_core::scene::ImmediateMode::on_pointer_down`]
    /// in VIEWPORT-LOCAL logical pixels — the same space the driver
    /// paints in. The retained input core dispatches to `Scene::External`
    /// widgets in the *state* scene (`dispatch_send` by tag); immediate
    /// drivers are paint-scene-only and not `External`, so the router's
    /// `pointer_down` no-ops for them. This shell-level branch bridges
    /// that without touching the router: it reads the router's already-
    /// resolved hit target + cursor, finds the addressed driver in the
    /// last paint scene, and dispatches. Idempotent / no-op when the hit
    /// target is a retained widget (or nothing) — `find_immediate_with_tag`
    /// returns `None`, so widget presses are unaffected.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "window-local logical-pixel cursor coords fit f32 in every realistic viewport"
    )]
    fn forward_pointer_down_to_immediate(&mut self, window_id: &str, pid: PointerId) {
        let Some(hit_tag) = self
            .core
            .hover_target_for_window(window_id, pid)
            .map(str::to_owned)
        else {
            return;
        };
        let Some((cx, cy)) = self.core.cursor_position_for_window(window_id, pid) else {
            return;
        };
        // Resolve the addressed driver handle + viewport from the paint
        // scene, then drop that borrow before mutating `self.core`.
        let resolved = self
            .core
            .last_paint_scene_for_window(window_id)
            .and_then(|paint| paint.find_immediate_with_tag(&hit_tag))
            .map(|node| (node.handle.clone(), node.viewport));
        let Some((handle, viewport)) = resolved else {
            return;
        };
        // Viewport-local logical pixels: cursor minus the viewport
        // top-left (the §5.21 layout pass resolved `viewport`).
        let local_x = (cx - f64::from(viewport.x)) as f32;
        let local_y = (cy - f64::from(viewport.y)) as f32;
        handle.borrow_mut().on_pointer_down(local_x, local_y);
        // The driver advanced its state (and may have queued a §5.20
        // intent, drained on the next paint's `tick_immediate_mode`
        // walk); repaint so the response is observed.
        self.redraw_requested = true;
        self.request_redraw_for_window(window_id);
    }

    /// R762 §5.36 §5.38 / R763 §5.22 — after a press focuses a widget,
    /// ask the binding to hit-test the press location into a text-field
    /// caret offset and (for a Shift-press) extend the selection.
    /// Reverse of the IME caret publish (`publish_ime_for_window`): the
    /// `V::position_caret_for_point` hook runs in the root-owner scope
    /// so `use_text_edit_state` / `use_text_field_layout_cache` resolve,
    /// hit-tests the cursor against the field's shaped layout, and
    /// moves the caret (`TextEditState::set_caret`) — or, when the
    /// Shift modifier is held (`self.modifiers.shift`), extends the
    /// selection from the existing anchor (`set_selection`). It returns
    /// `Some(anchor_byte)` — the pinned end the shell stores to drive a
    /// subsequent drag — when the press landed on this widget's text.
    ///
    /// R763: a returned anchor arms [`Self::text_select_drag`] so each
    /// later `cursor_moved` extends the selection while the button is
    /// held (`select_drag_to_point`); a press that resolves no text
    /// disarms it. Covers native winit clicks and the `scene/click` /
    /// `scene/drag` drains — all reach here through
    /// [`Self::mouse_pressed_for_window`].
    #[allow(
        clippy::cast_possible_truncation,
        reason = "window-local logical-pixel cursor coords fit f32 in every realistic viewport"
    )]
    fn position_caret_after_press(&mut self, window_id: &str, pid: PointerId) {
        self.text_select_drag = None;
        // R769.2 §5.36 — dispatch the press to the view even when no widget
        // is focused. A binding may handle non-caret presses that must work
        // regardless of field focus — e.g. a formatting toolbar acting on
        // the live selection (selection lives in the reactive state, not the
        // focus). Caret-only bindings stay correct: they short-circuit to
        // `None` whenever `focused != Some(<my tag>)`, so an unfocused
        // dispatch is a no-op for them (the previous early-return is folded
        // into that per-view guard).
        //
        // R801 §5.36 §5.35 — also hand the binding the router's resolved
        // hit-target so it can reject a press the router routed to a
        // *sibling* widget (a non-focusable toolbar keeps the field
        // focused, so `focused` alone cannot tell "press on the field" from
        // "press on the toolbar"). The router already hit-tested this press
        // during `pointer_down`; `hover_target_for_window` reads that exact
        // answer (`cursor_moved` always lands before the press on both the
        // native and `scene/click` paths, so it is the press-point target).
        // The binding no longer re-scans its own rect.
        let focused = self.focus.focused().map(str::to_owned);
        let hit_tag = self
            .core
            .hover_target_for_window(window_id, pid)
            .map(str::to_owned);
        let Some((cx, cy)) = self.core.cursor_position_for_window(window_id, pid) else {
            return;
        };
        let state = *self.cached_state();
        let extend = self.modifiers.shift_key();
        let owner = self.root_owner().clone();
        let anchor = {
            let Some(paint) = self.core.last_paint_scene_for_window(window_id) else {
                return;
            };
            owner.run(|| {
                V::position_caret_for_point(
                    &state,
                    paint,
                    focused.as_deref(),
                    hit_tag.as_deref(),
                    cx as f32,
                    cy as f32,
                    extend,
                )
            })
        };
        if let Some(anchor) = anchor {
            self.text_select_drag = Some(TextSelectDrag {
                window_id: window_id.to_owned(),
                pid,
                anchor,
            });
            self.request_redraw_for_window(window_id);
        }
    }

    /// R763 §5.36 §5.22 — extend the active pointer text selection.
    /// Called from [`Self::cursor_moved_for_window`] on every move
    /// while a press armed [`Self::text_select_drag`]. Hit-tests the
    /// cursor to a byte and asks the binding to set the selection from
    /// the stored anchor to that byte (`select_drag_to_point` →
    /// `TextEditState::set_selection`). A `cursor_moved` on a different
    /// window or pointer than the one that started the drag is ignored
    /// (the stored `window_id` / `pid` guard). Requests a redraw when
    /// the selection changed so the next frame repaints the band.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "window-local logical-pixel cursor coords fit f32 in every realistic viewport"
    )]
    fn extend_text_selection_on_drag(&mut self, window_id: &str, pid: PointerId, x: f64, y: f64) {
        let Some(drag) = self.text_select_drag.as_ref() else {
            return;
        };
        if drag.window_id != window_id || drag.pid != pid {
            return;
        }
        let anchor = drag.anchor;
        let Some(focused) = self.focus.focused().map(str::to_owned) else {
            return;
        };
        let state = *self.cached_state();
        let owner = self.root_owner().clone();
        let changed = {
            let Some(paint) = self.core.last_paint_scene_for_window(window_id) else {
                return;
            };
            owner.run(|| {
                V::select_drag_to_point(
                    &state,
                    paint,
                    Some(focused.as_str()),
                    anchor,
                    x as f32,
                    y as f32,
                )
            })
        };
        if changed {
            self.request_redraw_for_window(window_id);
        }
    }

    /// R51.80 §5.35 — winit `MouseInput { Released, Left }` dispatch.
    ///
    /// R672 §5.35 — single-window wrapper around
    /// [`Self::mouse_released_for_window`].
    pub fn mouse_released(&mut self, pid: PointerId) {
        self.mouse_released_for_window(pinion_runtime::DEFAULT_WINDOW, pid);
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::mouse_released`].
    pub fn mouse_released_for_window(&mut self, window_id: &str, pid: PointerId) {
        // R1125 §5.51 PR-33 — the release ends the drag, so clear the incoming
        // cross-window drop-PREVIEW target and repaint it: a redock repaints it via
        // the topology change anyway, but a non-redock release (the floater stays
        // floating) must also drop the strip. No-op when no drag targeted another
        // window (the common click / same-window case).
        if let Some(target) = self.cross_preview_target.take() {
            self.request_redraw_for_window(&target);
        }
        // (R1168 retired the static dock-zone GUIDES, so the drag-end
        // "repaint every OTHER window to strip their guides" block is gone — no
        // window paints a drag affordance unless the cursor is over it, and that
        // window's `cross_preview_target` repaint above already covers it.)
        // R882 / R882.1 §5.35 §5.39 — the release routes through the
        // substrate's LEFT front door
        // ([`CoreShell::left_release_for_window`]): a left-opened pan
        // gesture resolves in the pan channel (gesture-capture — the
        // gesture, not the current chord state, owns the routing) and
        // returns `None`. R781 §5.35 §5.41 — the routed arc carries
        // the held modifiers to the activate edge so a Shift / Ctrl
        // click reaches the composite send wire (a multi-select
        // coordinator extends / toggles; every other widget ignores
        // it). The same `scene/modifiers` cache drives keyboard
        // multi-select, so RPC `scene/modifiers` + `scene/click` and
        // a native modified click are one path.
        let Some(tail) = self
            .core
            .left_release_for_window(window_id, pid, self.modifiers)
        else {
            return;
        };
        self.handle_tail(&tail);
        // R763 §5.36 §5.22 — the press → move → release gesture ends;
        // the selection it produced persists in the TextEditState, but
        // no further move extends it.
        if self
            .text_select_drag
            .as_ref()
            .is_some_and(|d| d.window_id == window_id && d.pid == pid)
        {
            self.text_select_drag = None;
        }
    }

    /// R51.80 §5.35 — abstract touch event dispatch (R51.108 §5.41
    /// winit-free, R51.122 §5.41 router-side lift). The surface-side
    /// `AppShell` converts a `winit::event::Touch` to [`Touch`] at
    /// the window-system boundary; future TUI / mobile / RPC paths
    /// construct the same abstract event directly. Delegates to
    /// `Self::handle_touch_for_window` (which calls [`CoreShell::touch_event`]
    /// plus the Vello-only `click_to_focus` follow-up on the press
    /// phase) then routes the dispatch tail.
    ///
    /// R672 §5.35 — single-window wrapper around
    /// [`Self::touch_event_for_window`].
    pub fn touch_event(&mut self, touch: Touch) {
        self.touch_event_for_window(pinion_runtime::DEFAULT_WINDOW, touch);
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::touch_event`].
    pub fn touch_event_for_window(&mut self, window_id: &str, touch: Touch) {
        let tail = self.handle_touch_for_window(window_id, touch);
        self.handle_tail(&tail);
    }

    /// (R51.186 §5.45 R55.C.2) Mouse wheel dispatch — winit
    /// `WindowEvent::MouseWheel`. Forwards through
    /// [`CoreShell::wheel`](pinion_runtime::CoreShell::wheel), which
    /// walks the deepest [`Scene::Scroll`]
    /// under the pointer's stored cursor and calls
    /// [`ScrollState::scroll_by`](pinion_core::widgets::scroll::ScrollState::scroll_by)
    /// on the attached state. winit emits `MouseWheel` without its
    /// own position field; the substrate's
    /// [`InputRouter`](pinion_runtime::InputRouter) reuses the last
    /// `CursorMoved` position for `pid` exactly the way W3C / iOS /
    /// Android specify.
    ///
    /// Requests a repaint only when the router reports an actual
    /// dispatch — silent drops (cursor outside the window, no
    /// scroll container at the point, or the covering `ScrollNode`
    /// carries no `state` link) do not bump the redraw flag, so a
    /// wheel event over a non-scrollable region cannot regress the
    /// idle-frame-skipping the R51.147 substrate guarantees.
    pub fn wheel(&mut self, pid: PointerId, delta: WheelDelta) {
        self.wheel_for_window(pinion_runtime::DEFAULT_WINDOW, pid, delta);
    }

    /// R1703 §5.45 — [`Self::wheel_for_window`] carrying the gesture
    /// [phase](GesturePhase) winit reports beside the delta.
    pub fn wheel_phase_for_window(
        &mut self,
        window_id: &str,
        pid: PointerId,
        delta: WheelDelta,
        phase: GesturePhase,
    ) {
        let (tail, dispatched) =
            self.core
                .wheel_with_modifiers_for_window(window_id, pid, delta, self.modifiers, phase);
        if dispatched {
            self.request_redraw();
        }
        self.handle_tail(&tail);
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::wheel`].
    ///
    /// R877 §5.15 §5.49 — forwards the shell's modifier cache (winit
    /// `ModifiersChanged`, or the R763 `scene/modifiers` RPC mirror —
    /// one cache, both producers), so a hovered `External` wheel
    /// consumer (canvas pan / `Ctrl`-zoom) reads the held modifiers
    /// the same way `scene/click` Shift-extend does.
    /// The phase is [`GesturePhase::Update`] — a notched mouse wheel, which is
    /// what a caller with no phase to pass has. A producer that HAS one (the
    /// winit arm) uses [`Self::wheel_phase_for_window`].
    pub fn wheel_for_window(&mut self, window_id: &str, pid: PointerId, delta: WheelDelta) {
        self.wheel_phase_for_window(window_id, pid, delta, GesturePhase::Update);
    }

    /// R1432 §5.35 §5.15 — native PINCH (magnify) gesture into the addressed
    /// window, the wheel wrapper's sibling: carry the held `self.modifiers`
    /// (the `ModifiersChanged` cache) into the runtime
    /// `ShellCore::pinch_gesture_with_modifiers_for_window` and request a repaint
    /// when the hovered widget consumed it. The one place
    /// both the native winit `PinchGesture` arm and the `scene/pinch_gesture`
    /// RPC replay funnel through, so the modifier read + repaint gate are stated
    /// once.
    pub fn pinch_gesture_for_window(
        &mut self,
        window_id: &str,
        pid: PointerId,
        magnification: f64,
        phase: pinion_core::GesturePhase,
    ) {
        let (tail, consumed) = self.core.pinch_gesture_with_modifiers_for_window(
            window_id,
            pid,
            magnification,
            phase,
            self.modifiers,
        );
        if consumed {
            self.request_redraw();
        }
        self.handle_tail(&tail);
    }

    /// R1433 §5.35 §5.15 — native ROTATION gesture into the addressed window, the
    /// [`Self::pinch_gesture_for_window`] sibling with `rotation` (degrees) in
    /// place of `magnification`: carry the held `self.modifiers` into the runtime
    /// `ShellCore::rotation_gesture_with_modifiers_for_window` and request a
    /// repaint when the hovered widget consumed it. The one place both the native
    /// winit `RotationGesture` arm and the `scene/rotation_gesture` RPC replay
    /// funnel through, so the modifier read + repaint gate are stated once.
    pub fn rotation_gesture_for_window(
        &mut self,
        window_id: &str,
        pid: PointerId,
        rotation: f64,
        phase: pinion_core::GesturePhase,
    ) {
        let (tail, consumed) = self.core.rotation_gesture_with_modifiers_for_window(
            window_id,
            pid,
            rotation,
            phase,
            self.modifiers,
        );
        if consumed {
            self.request_redraw();
        }
        self.handle_tail(&tail);
    }

    /// R1434 §5.35 §5.15 — native PAN gesture into the addressed window, the
    /// [`Self::pinch_gesture_for_window`] sibling with a two-dimensional
    /// `(delta_x, delta_y)` in logical pixels in place of a single scalar: carry
    /// the held `self.modifiers` into the runtime
    /// `ShellCore::pan_gesture_with_modifiers_for_window` and request a repaint
    /// when the hovered widget consumed it. The one place both the native winit
    /// `PanGesture` arm and the `scene/pan_gesture` RPC replay funnel through, so
    /// the modifier read + repaint gate are stated once.
    pub fn pan_gesture_for_window(
        &mut self,
        window_id: &str,
        pid: PointerId,
        delta_x: f32,
        delta_y: f32,
        phase: pinion_core::GesturePhase,
    ) {
        let (tail, consumed) = self.core.pan_gesture_with_modifiers_for_window(
            window_id,
            pid,
            delta_x,
            delta_y,
            phase,
            self.modifiers,
        );
        if consumed {
            self.request_redraw();
        }
        self.handle_tail(&tail);
    }

    /// R1435 §5.35 §5.15 — native SMART-ZOOM gesture into the addressed
    /// window, the family's phase-less member (the toolkit `SmartZoomNativeGesture` / winit `DoubleTapGesture`):
    /// carry the held `self.modifiers` into the runtime `ShellCore::smart_zoom_gesture_with_modifiers_for_window` and request a repaint when the
    /// hovered widget consumed it. The one place both the native winit `DoubleTapGesture` arm
    /// and the `scene/smart_zoom_gesture` RPC replay funnel through, so the modifier read + repaint
    /// gate are stated once.
    pub fn smart_zoom_gesture_for_window(&mut self, window_id: &str, pid: PointerId) {
        let (tail, consumed) =
            self.core
                .smart_zoom_gesture_with_modifiers_for_window(window_id, pid, self.modifiers);
        if consumed {
            self.request_redraw();
        }
        self.handle_tail(&tail);
    }

    /// R51.195 §5.49 §5.45 — drain the deferred-input inbox `dispatch`
    /// populated. Each entry replays the input through the same
    /// `cursor_moved` / `wheel` entry points the winit and TUI
    /// surfaces use, so the [`InputRouter`](pinion_runtime::InputRouter)
    /// fires under its normal post-frame redraw rules. Called once
    /// per `dispatch_rpc` after the dispatcher's `&mut scene` borrow
    /// releases.
    /// R672 §5.35 §5.41 §5.49 — per-window deferred-input drain.
    /// Routes every replayed input
    /// through the addressed window's [`pinion_runtime::InputRouter`]
    /// so `scene/click {window: "<id>"}` against any window resolves
    /// against the *named* window's last paint scene + pointer
    /// state. Pre-R672 every drained replay went through the
    /// binding-wide router → `scene/click` against a non-last-painted
    /// window had a race condition on `last_paint_scene`.
    ///
    /// R1188 §5.16 §5.49 §2 #2 — `scope` is the dispatch's ORIGINAL window
    /// scope (`None` = the unscoped single-window entry), threaded alongside
    /// the derived router key so a window-control hit resolves the canonical
    /// spec id through the same [`Self::resolve_window_spec`] every per-window
    /// lookup uses (the R1123.1 rule — never re-derive identity from the
    /// `DEFAULT_WINDOW` fallback string, which collapses "unscoped primary"
    /// with "a window literally named main").
    /// R1430 §5.35 — the shared tail of every pointer-AXIS drain arm (pressure /
    /// tilt / twist / tangential / height): bump the revision so a reactive
    /// surface re-reads, and request a repaint so the change lands on the next
    /// frame. One helper so the five arms cannot drift on the bump/repaint pair.
    fn after_pointer_axis_change(&mut self, window_id: &str) {
        self.revision.bump();
        self.request_redraw_for_window(window_id);
    }

    // R1026 — rustfmt's reflow pushed this 1 line over the workspace
    // too_many_lines (100) ceiling it was kept just under; the body is a flat
    // per-input dispatch, not bloat. Extraction is deferred to the owner.
    #[allow(clippy::too_many_lines)]
    fn drain_deferred_inputs_for_window(&mut self, scope: Option<&str>, inputs: &[DeferredInput]) {
        let window_id = scope.unwrap_or(pinion_runtime::DEFAULT_WINDOW);
        // R1658 §5.13 §5.39 — one drain is one delivery. Every key this drain
        // dispatches carries one arrival, because the agent handed them over
        // together; a key from the NEXT request gets its own, because it was
        // sent separately. The instant is taken here, before the first
        // handler of the drain runs, for the same reason the winit path takes
        // it in `new_events`.
        self.core.open_key_delivery();
        // `DeferredInput` is `non_exhaustive`; the wildcard arm
        // covers future variants (key, cursor_only, etc.) silently
        // no-op against this drain until a follow-up round extends
        // the match.
        for input in inputs {
            match *input {
                DeferredInput::Wheel { x, y, delta, phase } => {
                    self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
                    self.wheel_phase_for_window(window_id, PointerId::MOUSE, delta, phase);
                }
                DeferredInput::Click { x, y } => {
                    self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
                    // R1188 §5.16 §5.49 §2 #2 — window-control drive parity.
                    // The winit pointer path consumes a left-press on a discrete
                    // window-control tag BEFORE widget routing
                    // (`AppShell::try_chrome_press`); pre-R1188 this RPC replay
                    // skipped that interception, so `scene/click` on a control
                    // tag fell into widget routing and no-opped (no widget
                    // carries the overlay tag) — the R1121 "AI drives the
                    // controls" contract held for observation only. Mirror the
                    // interception here: resolve the hover the cursor seed just
                    // set, and on a control hit queue `(spec, control)` for the
                    // windowed shell to execute post-dispatch (winit handles are
                    // AppShell's; headless harnesses assert the queue itself).
                    // Grip / resize tags are NOT intercepted — pointer-session
                    // gestures whose RPC peers are `scene/window_move` /
                    // `scene/resize` — so those clicks keep today's fall-through.
                    // ★ R1701 — through `window_control_for_press` with the
                    // ordinal, which is the projection the winit drain resolves
                    // through too. A single click still means what it meant.
                    let control = self
                        .hover_target_for_window(window_id, PointerId::MOUSE)
                        .and_then(|tag| pinion_overlay::window_control_for_press(tag, 1));
                    let consumed = match control {
                        Some(control) => match self.resolve_window_spec(scope) {
                            Some((spec_id, _)) => {
                                self.pending_window_controls.push((spec_id, control));
                                true
                            }
                            // No declared window resolves (a binding with an
                            // empty `windows()` list) — leave the click on the
                            // pre-R1188 widget-routing path rather than dropping
                            // it silently.
                            None => false,
                        },
                        None => false,
                    };
                    if !consumed {
                        // R1416 — route through the unified button seam (not
                        // `mouse_pressed_for_window` directly) so a `scene/click`
                        // on a raw-pointer sink reaches the raw stream exactly as
                        // the native left-click does. For every non-raw widget
                        // the seam calls the same `mouse_pressed`/`mouse_released`
                        // pair, byte-identical to before.
                        self.pointer_button_for_window(
                            window_id,
                            pinion_core::PointerButton::Left,
                            pinion_core::PointerEdge::Down,
                        );
                        self.pointer_button_for_window(
                            window_id,
                            pinion_core::PointerButton::Left,
                            pinion_core::PointerEdge::Up,
                        );
                    }
                }
                // R887 §5.49 §5.53 — `scene/click {button: "right"}`
                // mirror: seed the router's cursor cache, then take the
                // exact arc a physical `MouseInput { button: Right,
                // state: Pressed }` takes (`secondary_click_for_window`
                // reads that cache — the press-edge one-shot, no
                // release half).
                DeferredInput::SecondaryClick { x, y } => {
                    self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
                    // R1416 — through the unified seam: a raw sink gets the raw
                    // right-press edge, every other widget gets the identical
                    // `secondary_click_for_window` context-menu open (press-edge
                    // one-shot, no release half).
                    self.pointer_button_for_window(
                        window_id,
                        pinion_core::PointerButton::Right,
                        pinion_core::PointerEdge::Down,
                    );
                }
                // R1416 §5.35 §5.15 §2 #2 — `scene/pointer_button`: one raw
                // button EDGE (left / middle / right × down / up) at (x, y).
                // Seed the cursor first (so a raw sink's hover-tracked position
                // is fresh before the edge — a native `CursorMoved` precedes
                // `MouseInput` the same way), then route through the unified
                // `pointer_button_for_window` seam the native winit path also
                // reaches, so an RPC-injected edge is indistinguishable from a
                // physical one (§2 #6). Held modifiers ride the R763 out-of-band
                // `scene/modifiers` cache (`self.modifiers`), read inside the
                // seam, exactly like the wheel / click paths.
                DeferredInput::PointerButton { x, y, button, edge } => {
                    self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
                    self.pointer_button_for_window(window_id, button, edge);
                }
                // R1423 / R1429 / R1430 §5.35 §5.15 — the toolkit tablet event
                // scalar axes (pressure / tilt / twist / tangential / height):
                // set the axis on the addressed window's router, then bump +
                // repaint via the shared `after_pointer_axis_change`. Each is positionless
                // (out-of-band) — the router delivers it to the surface under
                // the pointer at once and it rides subsequent moves. The
                // AI-first source for a tablet-reactive surface, no device
                // required (§2 #2).
                DeferredInput::PointerPressure { value } => {
                    self.core
                        .set_pointer_pressure_for_window(window_id, PointerId::MOUSE, value);
                    self.after_pointer_axis_change(window_id);
                }
                DeferredInput::PointerTilt { tilt_x, tilt_y } => {
                    self.core.set_pointer_tilt_for_window(
                        window_id,
                        PointerId::MOUSE,
                        tilt_x,
                        tilt_y,
                    );
                    self.after_pointer_axis_change(window_id);
                }
                DeferredInput::PointerTwist { twist } => {
                    self.core
                        .set_pointer_twist_for_window(window_id, PointerId::MOUSE, twist);
                    self.after_pointer_axis_change(window_id);
                }
                DeferredInput::PointerTangentialPressure { tangential } => {
                    self.core.set_pointer_tangential_pressure_for_window(
                        window_id,
                        PointerId::MOUSE,
                        tangential,
                    );
                    self.after_pointer_axis_change(window_id);
                }
                DeferredInput::PointerHeight { height } => {
                    self.core
                        .set_pointer_height_for_window(window_id, PointerId::MOUSE, height);
                    self.after_pointer_axis_change(window_id);
                }
                // R1431 §5.35 §5.15 — `scene/pointer_type`: the producing device
                // (mouse / pen / eraser / touch), the W3C `pointerType` peer,
                // forwarded through the same one router seam as the scalar axes.
                DeferredInput::PointerKind { kind } => {
                    self.core
                        .set_pointer_kind_for_window(window_id, PointerId::MOUSE, kind);
                    self.after_pointer_axis_change(window_id);
                }
                // R1432 §5.35 §5.15 — `scene/pinch_gesture`: a native pinch
                // (magnify) gesture at (x, y). Seed the cursor first (so the
                // hovered target is fresh before the offer — a native
                // `CursorMoved` precedes `PinchGesture` the same way), then
                // offer the incremental magnification + phase through the ONE
                // `pinch_gesture_for_window` seam the native winit path also
                // reaches, so an RPC-injected pinch is indistinguishable from a
                // trackpad one (§2 #6). Held modifiers ride the R763 out-of-band
                // `scene/modifiers` cache, read inside the seam.
                DeferredInput::PinchGesture {
                    x,
                    y,
                    magnification,
                    phase,
                } => {
                    self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
                    self.pinch_gesture_for_window(
                        window_id,
                        PointerId::MOUSE,
                        magnification,
                        phase,
                    );
                }
                // R1433 §5.35 §5.15 — `scene/rotation_gesture`: a native rotation
                // gesture at (x, y), the pinch sibling. Seed the cursor first (a
                // native `CursorMoved` precedes `RotationGesture` the same way),
                // then offer the incremental rotation + phase through the ONE
                // `rotation_gesture_for_window` seam the native winit path also
                // reaches, so an RPC-injected rotation is indistinguishable from a
                // trackpad one (§2 #6). Held modifiers ride the R763 out-of-band
                // `scene/modifiers` cache, read inside the seam.
                DeferredInput::RotationGesture {
                    x,
                    y,
                    rotation,
                    phase,
                } => {
                    self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
                    self.rotation_gesture_for_window(window_id, PointerId::MOUSE, rotation, phase);
                }
                // R1434 §5.35 §5.15 — `scene/pan_gesture`: a native N-finger pan
                // at (x, y), the pinch sibling with a 2D delta. Seed the cursor
                // first (a native `CursorMoved` precedes `PanGesture` the same
                // way), then offer the incremental delta + phase through the ONE
                // `pan_gesture_for_window` seam the native winit path also
                // reaches, so an RPC-injected pan is indistinguishable from a
                // trackpad one (§2 #6). Held modifiers ride the R763 out-of-band
                // `scene/modifiers` cache, read inside the seam.
                DeferredInput::PanGesture {
                    x,
                    y,
                    delta_x,
                    delta_y,
                    phase,
                } => {
                    self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
                    self.pan_gesture_for_window(
                        window_id,
                        PointerId::MOUSE,
                        delta_x,
                        delta_y,
                        phase,
                    );
                }
                // R1435 §5.35 §5.15 — `scene/smart_zoom_gesture`: a native
                // two-finger double tap at (x, y). Seed the cursor first (a
                // native `CursorMoved` precedes `DoubleTapGesture` the same way),
                // then offer the toggle through the ONE
                // `smart_zoom_gesture_for_window` seam the native winit path also
                // reaches (§2 #6). No phase and no payload to carry — the anchor
                // the cursor just seeded IS the payload.
                DeferredInput::SmartZoomGesture { x, y } => {
                    self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
                    self.smart_zoom_gesture_for_window(window_id, PointerId::MOUSE);
                }
                // R663 §5.49 — `scene/double_click` mirror. Two
                // complete press/release cycles at the same coordinate
                // exercise the W3C UIEvent `detail:2` convention the
                // TasteJS TodoMVC double-click-to-edit UX expects;
                // widgets that distinguish single from double activate
                // by counting `mouse_pressed` calls within the same
                // cursor-frozen window.
                DeferredInput::DoubleClick { x, y } => {
                    self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
                    // ★★★★★ R1701 §5.16 §2 #2 — the chrome interception the
                    // `Click` arm above has had since R1188, on the arm beside
                    // it. Without it a title bar's double-click fell straight
                    // into widget routing, where no widget carries the overlay
                    // tag, so the gesture existed for a real mouse and not for
                    // an agent. Measured on a display WITH a window manager: the
                    // maximize button took the window 640x420 -> 2494x1568 and a
                    // double-click on the bar did nothing at all.
                    //
                    // R1188 paid for this exact asymmetry once, on the single
                    // click. Adding a gesture to one drain and not the other is
                    // how it comes back.
                    if let Some(control) = self
                        .hover_target_for_window(window_id, PointerId::MOUSE)
                        .and_then(|tag| pinion_overlay::window_control_for_press(tag, 2))
                        && let Some((spec_id, _)) = self.resolve_window_spec(scope)
                    {
                        self.pending_window_controls.push((spec_id, control));
                        continue;
                    }
                    // R1416 — both press/release pairs through the unified seam
                    // (a raw sink sees two down/up cycles; every non-raw widget
                    // sees the identical `mouse_pressed`/`mouse_released` cadence
                    // the router's double-click detector counts).
                    for _ in 0..2 {
                        self.pointer_button_for_window(
                            window_id,
                            pinion_core::PointerButton::Left,
                            pinion_core::PointerEdge::Down,
                        );
                        self.pointer_button_for_window(
                            window_id,
                            pinion_core::PointerButton::Left,
                            pinion_core::PointerEdge::Up,
                        );
                    }
                }
                // R695 §5.49 §5.35 — `scene/hover` mirror: a bare cursor
                // move with no press. `cursor_moved_for_window` re-resolves
                // the addressed window's hover target and fires the
                // synthetic `PointerEnter` / `PointerLeave` arc on a tag
                // transition (the `Tooltip` show/hide trigger). No
                // `mouse_pressed` follows — this is the pointer-position-
                // only peer to the `Click` arc.
                DeferredInput::Hover { x, y } => {
                    self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
                }
                // R719 §5.49 §5.35 — `scene/pointer_leave` mirror: the
                // pointer exits the window (winit `CursorLeft`). Drops
                // the cursor and rolls back any in-flight `Hover` on the
                // addressed window's router, re-running the synthetic
                // `PointerLeave` arc on whatever widget was hovered — the
                // cursor-exit peer to the `Hover` arc. No coordinate: a
                // window-leave is positionless, so unlike the other arms
                // there is no leading `cursor_moved`.
                DeferredInput::PointerLeave => {
                    self.cursor_left_for_window(window_id, PointerId::MOUSE);
                }
                // R724 §5.28 — `scene/tick`: advance this window's
                // animation clock by `dt` seconds so time-driven state
                // (springs, theme-fade, caret blink, timed dismissal)
                // is deterministically drivable by an AI client. The
                // §5.28 spring integrator is semi-implicit Euler, which
                // is only stable for small steps (real frames cap at
                // 1/30 s), so a large RPC-injected `dt` is advanced in
                // fixed sub-steps via the shared
                // [`pinion_runtime::substep`] policy — feeding `dt` as one
                // giant step would overshoot / destabilise the spring.
                // A settled spring converges to its target regardless
                // of step size, so the settled value is deterministic.
                // The mutated animation Signals mark the owner dirty,
                // so the next `scene/snapshot` re-render reflects the
                // advanced frame. `dt == 0.0` is a no-op (clock frozen).
                DeferredInput::Tick { dt } => {
                    // R1549 §5.35 §5.38 — press-and-hold auto-repeat rides
                    // the injected clock beside the animation clock, through
                    // the same substep splitter. Without it a held button is
                    // the one time-driven behaviour an agent cannot
                    // reproduce — precisely the untestability of the toolkit's
                    // basic timer repeat. Tails are collected and applied
                    // after the walk so one `scene/tick`'s repeats reach
                    // `V::update` in a single batch.
                    let mut tails = Vec::new();
                    pinion_runtime::substep(dt, |step| {
                        self.core.tick_animations_for_window(window_id, step);
                        tails.push(self.core.tick_pointer_hold_for_window(window_id, step).0);
                    });
                    for tail in &tails {
                        self.handle_tail(tail);
                    }
                    // R829 §2 #4 §5.28 — also queue this `dt` as the
                    // window's pending immediate-mode step. The animation
                    // clock advanced in-place above; immediate-mode
                    // drivers live in the paint scene, so they advance on
                    // the next paint, which this redraw request triggers
                    // even when the window is paused (`scene/set_fps 0`)
                    // and the continuous loop is therefore quiescent.
                    // `compute_paint_scene_internal` consumes the
                    // accumulated delta deterministically (substepped)
                    // instead of the wall-clock delta. `dt == 0` is a
                    // no-op (frozen clock); skip so a 0-tick does not arm
                    // a needless repaint.
                    if dt > 0.0 {
                        *self
                            .window_state_mut(window_id)
                            .pending_immediate_dt
                            .get_or_insert(0.0) += dt;
                        self.request_redraw_for_window(window_id);
                    }
                }
                // R829 §2 #4 §5.28 — `scene/set_fps`: the AI-facing peer
                // of [`Self::set_target_fps_for_window`]. Sets the §2 #4
                // game-loop pacing policy for the addressed window;
                // `fps == 0` pauses the continuous paint clock so the AI
                // can frame-step the immediate-mode loop deterministically
                // via `scene/tick`. R888 — `fps: null` clears the
                // override (restores the adaptive default policy). The
                // redraw request lets the new policy take effect (and,
                // on un-pause, restarts the loop) on the next event-loop
                // iteration.
                DeferredInput::SetTargetFps { fps } => {
                    match fps {
                        Some(fps) => self.set_target_fps_for_window(window_id, fps),
                        None => self.clear_target_fps_for_window(window_id),
                    }
                    self.request_redraw_for_window(window_id);
                }
                // R763 §5.49 §5.39 — `scene/modifiers`: the winit
                // `WindowEvent::ModifiersChanged` RPC peer. Sets the
                // shell's absolute modifier cache so a subsequent
                // `scene/click` (Shift-click extend), `scene/drag`, or
                // `scene/key` press reads the held modifiers exactly as
                // a real key-down would — modifiers are tracked
                // out-of-band (their own winit event), so the mirror is
                // a standalone state setter, not a per-click field.
                // Persists until the next `scene/modifiers` (a real
                // key-up sends an empty state); closes the R742.2
                // RPC-modifier-channel gap for every input path.
                DeferredInput::SetModifiers {
                    shift,
                    ctrl,
                    alt,
                    meta,
                } => {
                    self.set_modifiers(Modifiers {
                        shift,
                        ctrl,
                        alt,
                        meta,
                    });
                }
                // R882 §5.49 §5.39 — `state` carries the keyboard edge;
                // the shared policy lives in `drain_key_for_window`.
                DeferredInput::Key {
                    x,
                    y,
                    ref key,
                    state,
                } => {
                    self.drain_key_for_window(window_id, (x, y), key, state, false);
                }
                // R666 §5.37 — `scene/key` single-codepoint arc. The
                // dispatcher auto-detects character vs named keys by
                // `key.chars().count()`; single-codepoint strings
                // ("a", " ", "漢") arrive here so `V::keybinding`
                // (typed-event channel — hello-counter `+`/`-`,
                // listbox typeahead, vim-style chords) gets first
                // crack before the `apply_key` fallback. Closes
                // [[scene-key-character-named-gap]].
                DeferredInput::CharacterKey {
                    x,
                    y,
                    ref character,
                    state,
                } => {
                    self.drain_key_for_window(window_id, (x, y), character, state, true);
                }
                DeferredInput::Drag {
                    from_x,
                    from_y,
                    to_x,
                    to_y,
                    steps,
                    button,
                    phase,
                } => {
                    self.drain_drag_for_window(
                        window_id,
                        (from_x, from_y),
                        (to_x, to_y),
                        steps,
                        button,
                        phase,
                    );
                }
                // R770 §5.49 §5.15 — OS file drag-drop mirrors. Each runs
                // the matching `WidgetView` file hook in the root-owner
                // scope (the same arc winit's `HoveredFile` /
                // `HoveredFileCancelled` / `DroppedFile` take). Window-
                // scoped + (for drop/hover) path-carrying; no cursor move
                // since winit's file DnD reports no drop coordinate.
                DeferredInput::FileHover { ref path } => {
                    self.file_hover_for_window(window_id, path);
                }
                DeferredInput::FileHoverCancel => {
                    self.file_hover_cancel_for_window(window_id);
                }
                DeferredInput::FileDrop { ref path } => {
                    self.file_drop_for_window(window_id, path);
                }
                // R1364 §5.55 §2 #2 — `app/quit`. Recorded, never executed here:
                // ending the app needs an `&ActiveEventLoop` that only winit
                // callbacks hold, and this substrate is winit-free for the §2 #6
                // dual. `AppShell` drains it after the response write, so the
                // client sees its `result` before the process goes away.
                //
                // NOT scoped by `window_id`: a quit addresses no window. This is
                // the one arm in this per-window drain that ignores the scope,
                // and that asymmetry is §5.55 showing through the wire.
                DeferredInput::Quit => {
                    self.pending_quit = true;
                }
                _ => {}
            }
        }
    }

    /// R882 §5.49 §5.39 — the `scene/key` drain arm shared by the
    /// named-key and character-key variants (one home for the edge
    /// policy, so the two wire shapes cannot diverge): `state` carries
    /// the keyboard edge — [`KeyWireState::Down`] mirrors winit
    /// `Pressed` (held-key cache update THEN dispatch — a real
    /// key-down does both), [`KeyWireState::Up`] mirrors `Released`
    /// (cache update only; the native arm dispatches nothing on
    /// release either), and the default [`KeyWireState::Press`] is the
    /// legacy atomic keypress that never touches the cache (an atomic
    /// press cannot strand the Space pan chord). One
    /// [`Self::note_key_state`] funnel with the winit arm, so native
    /// and RPC chords cannot diverge. `character` picks the dispatch
    /// half: `handle_character_key` (the R666 single-codepoint
    /// `V::keybinding` arc) vs `handle_named_key`.
    ///
    /// R1075 §5.39 §5.16 §5.49 — the dispatching edge is gated by the SAME
    /// [`Self::admit_key_press`] the live winit arm uses, so this is no longer
    /// a gate bypass: a `Down` edge acts only on the window that holds OS focus
    /// AND owns the in-flight press (pinned at its rising edge), while the legacy
    /// atomic `Press` acts on the OS-focused window alone (it has no keyup to
    /// clear an owner, so it gates without pinning). A `Down` makes the press
    /// owner observable through `scene/input_state` (the R1074 introspection,
    /// now live for RPC keys).
    fn drain_key_for_window(
        &mut self,
        window_id: &str,
        at: (f64, f64),
        key: &str,
        state: KeyWireState,
        character: bool,
    ) {
        // The edge → cache / dispatch policy is `KeyWireState`'s own
        // (`held_edge` / `dispatches`) so the TUI drain reads the same
        // decision table. R882.1 — a release edge moves NO cursor
        // either: a physical `Released` carries no position, and the
        // leading cursor move would perturb a live pan / hover (the
        // wire's `at` is optional for `state:"up"` for the same
        // reason).
        if let Some(held) = state.held_edge() {
            self.note_key_state(key, held);
        }
        if !state.dispatches() {
            // R1075 §5.39 §5.49 — the release edge ends the physical press:
            // drop the press owner so the next press re-decides against live
            // OS focus, unifying with the winit arm's `note_key_release`
            // (only `Up` is non-dispatching, so this is the keyup edge).
            self.note_key_release(key);
            return;
        }
        // R1075 §5.39 §5.16 §5.49 — route the dispatching edge through the SAME
        // gate as the live winit `KeyboardInput` arm (`AppShell::handle_keyboard_input`),
        // so the RPC `scene/key` path is no longer a gate bypass. A `Down` edge is
        // admitted only when the window holds OS focus AND owns the in-flight press,
        // pinning the press owner at its rising edge ([`Self::admit_key_press`],
        // cleared by the matching keyup above). The legacy atomic `Press` has no
        // keyup to clear an owner, so it gates on OS focus alone, without pinning
        // ([`Self::is_key_dispatch_window`]), to avoid stranding one. With no
        // OS-focus event (the headless / single-window default) the gate fails
        // OPEN, so the DISPATCH decision is identical to the pre-R1075 ungated path
        // there; a `Down` additionally records the press owner — new introspectable
        // state, observable only through the field R1074 added (no prior observable
        // behaviour changes).
        let admit = if state.held_edge() == Some(true) {
            self.admit_key_press(window_id, key)
        } else {
            self.is_key_dispatch_window(window_id)
        };
        if !admit {
            return;
        }
        self.cursor_moved_for_window(window_id, PointerId::MOUSE, at.0, at.1);
        if character {
            self.handle_character_key(key);
        } else {
            self.handle_named_key(key);
        }
    }

    /// R660 §5.49 — the `scene/drag` drain arm: press at `from`, march
    /// the cursor linearly to `to` across `steps` frames (each one
    /// forwarded to `InputRouter::cursor_moved` under the R51.34
    /// capture lock so the receiving widget's `pointer_move` arc runs
    /// identically to a real-mouse drag), then release. `steps == 0`
    /// lands as a press / release at `from` (degenerate but
    /// well-defined — the RPC client gets exactly what it asked for).
    /// Extracted from [`Self::drain_deferred_inputs_for_window`] to
    /// keep that dispatcher under the workspace `too_many_lines`
    /// ceiling (the app.rs extract convention).
    ///
    /// R881 §5.35 §5.49 — `button` selects the gesture pair the march
    /// runs between: the left press/release arc (capture lock / `DnD` /
    /// text select), or the middle pair (drag-to-pan, paste on
    /// release-in-place) — the same shell methods the native winit
    /// `MouseInput { Middle }` arms call, per
    /// [[r47-class-incident-prevention]].
    ///
    /// R1138 §5.49 §2 #2 — `phase` gates which ends of the arc run: the
    /// leading press fires only when [`DragPhase::presses`]
    /// ([`DragPhase::Full`] / [`DragPhase::Begin`]) and the trailing release
    /// only when [`DragPhase::releases`] ([`DragPhase::Full`] /
    /// [`DragPhase::End`]); the cursor march always runs. A
    /// [`DragPhase::Begin`] therefore leaves the router's drag session OPEN
    /// across RPC calls so a follow-up paint snapshots the held mid-drag
    /// (the press-and-hold peer of a human holding a drag); a later
    /// [`DragPhase::Move`] re-aims it and [`DragPhase::End`] settles it. The
    /// session lives in the router, so the held state persists with zero
    /// extra shell bookkeeping.
    fn drain_drag_for_window(
        &mut self,
        window_id: &str,
        from: (f64, f64),
        to: (f64, f64),
        steps: u32,
        button: DragButton,
        phase: DragPhase,
    ) {
        // R1416 — the drag button maps to the unified button seam so a raw
        // sink receives the press / release edges (its `pointer_move` marches
        // supply the positions) while a non-raw widget keeps the identical
        // left-capture / middle-gesture arc `mouse_pressed`/`middle_pressed`
        // ran before. `scene/drag` has no `right` button, so only left /
        // middle reach here.
        let pbutton = match button {
            DragButton::Left => pinion_core::PointerButton::Left,
            DragButton::Middle => pinion_core::PointerButton::Middle,
        };
        self.cursor_moved_for_window(window_id, PointerId::MOUSE, from.0, from.1);
        if phase.presses() {
            self.pointer_button_for_window(window_id, pbutton, pinion_core::PointerEdge::Down);
        }
        if steps > 0 {
            for step in 1..=steps {
                let t = f64::from(step) / f64::from(steps);
                let x = from.0 + (to.0 - from.0) * t;
                let y = from.1 + (to.1 - from.1) * t;
                self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
            }
        }
        if phase.releases() {
            self.pointer_button_for_window(window_id, pbutton, pinion_core::PointerEdge::Up);
        }
    }

    /// R51.80 §5.39 — modifier-key cache update. `KeyEvent` carries no
    /// modifier state in winit; the substrate remembers the
    /// most-recent `ModifiersChanged` so the
    /// `AppShell::handle_key_press` Tab arm can branch on Shift.
    ///
    /// R51.108 §5.41 — signature lifted from
    /// `winit::keyboard::ModifiersState` to the abstract
    /// [`Modifiers`] so the substrate stays backend-agnostic for the
    /// §2 #6 GUI/TUI dual invariant.
    /// R1619 §5.35 §5.41 — the same absolute state is pushed down to every
    /// window's router, which stamps it onto **every** dispatched pointer event
    /// (press and hover included, not only the release edge the shell passes
    /// per-call). One funnel, both consumers.
    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
        self.core.set_pointer_modifiers(modifiers);
    }

    /// R882 §5.39 §5.35 — held-key absolute-state funnel: record that
    /// `key` (the canonical W3C `KeyboardEvent.key` string the winit
    /// boundary's `named_key_str` emits) is now held / released. ONE
    /// funnel for every producer — both edges of the winit
    /// `KeyboardInput` arm and the `scene/key state:"down"/"up"` RPC
    /// peer — so the native and AI-driven chord can never diverge.
    /// The cache (and the routing policy it gates) lives in the
    /// backend-agnostic substrate — R882.1 moved both into
    /// [`CoreShell`] so the GUI and TUI
    /// shells are pure edge producers with zero policy copies (§2 #6);
    /// this is the winit-side forwarding funnel.
    pub fn note_key_state(&mut self, key: &str, pressed: bool) {
        self.core.note_key_state(key, pressed);
    }

    /// R1073.1 PR-27.4 §5.39 §5.16 §5.35 — the release edge of the press-owner
    /// gate: end the physical press of `key` by dropping its
    /// `Self::key_press_owner` snapshot, so the next `Pressed` for `key` is a
    /// genuine new rising edge whose admission is re-decided against the live OS
    /// focus ([`Self::admit_key_press`]), not the prior press's owner.
    ///
    /// The lifecycle peer of [`Self::admit_key_press`] (press → pin owner;
    /// release → drop owner), deliberately SEPARATE from the chord-cache funnel
    /// [`Self::note_key_state`]: the owner gate keys on the *dispatch* vocabulary
    /// (every key `handle_key_press` acts on — including the shell-reserved
    /// `Escape` / `Tab` that the R1009 content-surface / chord vocabularies
    /// exclude), so its lifecycle cannot ride on the chord cache without
    /// polluting the RPC-exposed [`CoreShell::held_key_names`](pinion_runtime::CoreShell::held_key_names)
    /// chord subset.
    ///
    /// Window-agnostic on purpose: a keyup delivered to whatever window grabbed
    /// OS focus still clears an owner whose source window was closed by the
    /// press's own dispatch (the close-during-dispatch case). Cleared only here
    /// (and never on blur), so the gate stays robust to either winit
    /// focus-event ordering on a close-driven handoff. The live GUI calls this
    /// from the `KeyboardInput` release edge; a test drives it directly to model
    /// a keyup.
    pub fn note_key_release(&mut self, key: &str) {
        self.key_press_owner.remove(key);
    }

    /// R1074 §5.39 §5.16 — the READ projection of the multi-window
    /// key-dispatch gate (`Self::os_focused_window` +
    /// `Self::key_press_owner`) into the contract
    /// [`pinion_core::KeyDispatchFocus`], so `scene/input_state` surfaces
    /// the gate whose admit decision ([`Self::admit_key_press`]) an AI
    /// otherwise cannot observe — the AI-first introspection peer of the
    /// R1071/R1073 writes. This is the GUI-shell axis the
    /// [`CoreShell::input_state_snapshot`](pinion_runtime::CoreShell::input_state_snapshot)
    /// home leaves a `None`-able parameter for; the TUI never builds one
    /// (single OS window, no gate).
    ///
    /// The owner pairs are **sorted by key**: `key_press_owner` is a
    /// `HashMap` with no inherent order, and a snapshot a deterministic
    /// RPC demo asserts on must be stable ([[zero-flake-policy]]).
    ///
    /// `pub` for the same reason as its write peers
    /// ([`Self::admit_key_press`] / [`Self::note_os_focus`] /
    /// [`Self::note_key_release`]): the live GUI builds the gate through
    /// `winit`, so a headless `#[test]` drives the writes and observes
    /// this read directly.
    #[must_use]
    pub fn key_dispatch_focus(&self) -> pinion_core::KeyDispatchFocus {
        let mut key_press_owners: Vec<(String, String)> = self
            .key_press_owner
            .iter()
            .map(|(key, window)| (key.clone(), window.clone()))
            .collect();
        key_press_owners.sort_by(|a, b| a.0.cmp(&b.0));
        pinion_core::KeyDispatchFocus {
            os_focused_window: self.os_focused_window.clone(),
            key_press_owners,
            // The bare (global) builder cannot know a dispatch scope, so the
            // per-window verdict defaults unfocused here; the per-window
            // projection ([`Self::key_dispatch_focus_for_window`]) fills it.
            focused: false,
        }
    }

    /// R1428 §5.39 §5.16 §5.41 — [`Self::key_dispatch_focus`] PROJECTED onto a
    /// dispatch's `{window}` scope: the global gate state plus the per-window
    /// [`pinion_core::KeyDispatchFocus::focused`] verdict, derived from the SAME
    /// fails-open predicate ([`Self::is_key_dispatch_window`]) that gates key
    /// admission and the R1427 terminal-cursor render. `scene/input_state` reads
    /// this so an AI observes the exact bit predicting the cursor's
    /// filled-vs-hollow state in one call, rather than comparing
    /// [`pinion_core::KeyDispatchFocus::os_focused_window`] against a hard-coded
    /// id. Threaded through `window_scoped_rpc_reads` (which already owns the
    /// resolved `wid`); the bare builder stays for the global-fact callers.
    #[must_use]
    pub fn key_dispatch_focus_for_window(&self, window_id: &str) -> pinion_core::KeyDispatchFocus {
        pinion_core::KeyDispatchFocus {
            focused: self.is_key_dispatch_window(window_id),
            ..self.key_dispatch_focus()
        }
    }

    /// R882 §5.39 — whether the Space pan chord is currently held
    /// (see [`Self::note_key_state`]). Read by tests; press routing
    /// consults the substrate cache internally and release routing
    /// deliberately does NOT — it follows the gesture in flight
    /// (gesture-capture).
    #[must_use]
    pub fn space_held(&self) -> bool {
        self.core.space_held()
    }

    /// R51.80 §5.39 / R51.59 — winit `WindowEvent::Focused(true)`
    /// dispatch. ARIA Focus Order asks the framework to reinstate the
    /// previously-focused widget when the window regains focus (the
    /// [`FocusManager`] owns the snapshot). Sets `redraw_requested` when
    /// `restore` reports a change so the focus ring repaints.
    ///
    /// R1327 §5.39 — the restore routes through `notify_focus_change` like
    /// every other focus mutation. It did not before: a restore that actually moved
    /// focus (the saved tag re-focused after something cleared or changed focus
    /// while the window was blurred) left every
    /// [`External::on_focus_change`](pinion_core::External::on_focus_change)
    /// observer stale — the `TextField` IME bridge never re-armed, the `CaretBlink`
    /// gate never re-enabled.
    pub fn window_focused(&mut self) {
        let focus_before = self.focus.focused().map(str::to_owned);
        if self.focus.restore() {
            self.notify_focus_change(focus_before.as_deref());
            self.request_redraw();
        }
    }

    /// R51.80 §5.39 / R51.59 — winit `WindowEvent::Focused(false)`
    /// dispatch. Saves the currently-focused widget tag so a future
    /// [`Self::window_focused`] can restore it.
    ///
    /// R882 §5.39 — also clears the held-key chord cache: the matching
    /// keyup goes to whichever window stole focus, never to us (the
    /// browser missed-keyup convention), and a stranded Space chord
    /// would turn every left drag after refocus into a pan. winit
    /// re-delivers `ModifiersChanged` on refocus so `Self::modifiers`
    /// resyncs itself; held keys have no such resync event, hence the
    /// explicit clear.
    pub fn window_blurred(&mut self) {
        self.focus.save();
        self.core.clear_held_keys();
        // R1619 §5.35 §5.39 — the pointer peer of the missed-keyup rule: the
        // mouse-up after a focus loss goes to whichever window took focus, so
        // a button held at blur is a button whose release this window will
        // never see.
        self.core.clear_held_pointer_buttons();
    }

    /// R1419 §5.39 §5.16 — the SINGLE write path for
    /// [`Self::os_focused_window`]. Commits `next` and publishes it to the
    /// binding's OS-focus mirror
    /// ([`pinion_core::window_focus_state`]),
    /// the read a binding uses to derive display state from OS focus (a
    /// whole-window dim on blur, a native pane focus reporter, a caret that
    /// stops on blur).
    ///
    /// Both mutators of `os_focused_window` — [`Self::note_os_focus`] (the winit
    /// `Focused` edge) and the window-destruction reconcile (a torn-down window
    /// that still names the OS focus) — funnel through here, so the paint-path
    /// mirror cannot drift from the shell's own `os_focused_window` SSOT the
    /// keyboard gate ([`Self::is_key_dispatch_window`]) reads. This mirrors the
    /// `FocusManager::commit_focus` funnel on the focused-tag axis: publishing
    /// from the one field-owning site rather than at each winit call site means a
    /// future call site cannot forget to publish.
    ///
    /// The publish is wrapped in [`Owner::run`](pinion_core::Owner::run) (the
    /// R1006 "blocker B" discipline the viewport / focus mirrors follow): a
    /// [`Signal::set`](pinion_core::reactive::Signal::set) synchronously re-runs
    /// subscribed [`Effect`](pinion_core::reactive::Effect)s, and a woken Effect
    /// that reads an owner-scoped hook must find this scope on the stack. The
    /// publish equality-skips, so a redundant re-assert of the same window wakes
    /// no subscriber.
    fn set_os_focused_window(&mut self, next: Option<String>) {
        self.os_focused_window = next;
        let value = self.os_focused_window.clone();
        let owner = self.core.root_owner();
        owner.run(|| owner.os_focused_window_signal().set(value));
    }

    /// R1071 PR-27 §5.39 §5.16 §5.35 — record a winit `WindowEvent::Focused`
    /// edge for `window_id`, maintaining the `Self::os_focused_window` the
    /// keyboard gate reads. Called from `AppShell`'s `Focused` arm (which
    /// carries the canonical [`WindowSpec::id`](crate::WindowSpec)) alongside
    /// the existing [`Self::window_focused`] / [`Self::window_blurred`] focus
    /// save/restore — separate concerns: that pair owns the `FocusManager`
    /// snapshot, this owns the OS-focus identity for keyboard routing.
    ///
    /// Robust to either winit focus-event ordering (`blur(old)` before
    /// `focus(new)` or vice-versa): a `focus` always wins, and a `blur` only
    /// clears when it is the currently-focused window that blurred, so a
    /// `focus(new)` that already landed is never clobbered by a late
    /// `blur(old)`.
    ///
    /// R1419 §5.16 — routes through `set_os_focused_window` so the
    /// R1419 paint-path OS-focus mirror
    /// ([`pinion_core::window_focus_state`]) is
    /// published on the same edge the gate is updated on.
    pub fn note_os_focus(&mut self, window_id: &str, focused: bool) {
        if focused {
            self.set_os_focused_window(Some(window_id.to_owned()));
        } else if self.os_focused_window.as_deref() == Some(window_id) {
            self.set_os_focused_window(None);
        }
    }

    /// R1071 PR-27 §5.39 §5.16 §5.35 — whether a key press arriving at
    /// `window_id` should be dispatched. `true` when `window_id` holds the OS
    /// keyboard focus, OR when no focus is known yet (`None` — single-window
    /// startup before the first `Focused`, or a platform that omits focus
    /// events): the gate fails OPEN so it never silently swallows input.
    ///
    /// A single-window binding has exactly one window, which is the focused
    /// one, so every key passes — byte-identical to the pre-R1071 ungated
    /// global dispatch. In a multi-window binding mid-undock, a stray
    /// re-delivery of a press to the now-unfocused source window fails the
    /// gate (the torn-off window grabbed focus), so a toggle-class shortcut
    /// fires exactly once per physical press instead of bouncing.
    #[must_use]
    pub fn is_key_dispatch_window(&self, window_id: &str) -> bool {
        match &self.os_focused_window {
            Some(focused) => focused == window_id,
            None => true,
        }
    }

    /// R1073 PR-27.4 §5.39 §5.16 §5.35 — the full key-dispatch admission gate:
    /// decide whether a `Pressed` for `key` arriving at `window_id` may
    /// dispatch, and pin the press's owner window on its admitted rising edge.
    /// The single source of truth both the live GUI
    /// (`AppShell::window_event`'s `KeyboardInput` arm) and the per-window seam
    /// ([`Self::key_press_for_window`]) consult, so they can never diverge.
    ///
    /// Two regimes, distinguished by whether the key already has an in-flight
    /// press owner (`Self::key_press_owner`):
    ///
    /// - **Rising edge** (no owner): admit iff `window_id` holds OS focus
    ///   ([`Self::is_key_dispatch_window`], which fails OPEN when focus is
    ///   unknown — single-window startup / focus-eventless platforms). On
    ///   admit, snapshot `window_id` as the press owner.
    /// - **Continuation** (owner exists — auto-repeat, or a stray re-delivery
    ///   of the same physical press): admit iff the press arrives at its OWN
    ///   owner window AND that window still holds OS focus. The conjunction
    ///   catches BOTH multi-window re-delivery shapes with one rule:
    ///   - a stray to the now-UNFOCUSED *source* window (R1071's case) fails
    ///     the live-focus half (focus moved to the torn-off window), and
    ///   - a stray to the now-FOCUSED *successor* window (R1071's blind spot,
    ///     the close-during-dispatch double-toggle this round closes) fails the
    ///     owner half (`window_id` is not the window the press began on).
    ///
    /// The owner is released only on the key's keyup ([`Self::note_key_release`]
    /// — the dedicated seam R1073.1 split out of the chord cache so it can
    /// cover `Escape` / `Tab` without polluting the RPC-exposed chord subset),
    /// never on blur — so the gate is robust to either winit focus-event
    /// ordering during the close-driven handoff. Returns whether the press was
    /// ADMITTED (it then reaches the widget arc regardless of whether a widget
    /// consumes it).
    pub fn admit_key_press(&mut self, window_id: &str, key: &str) -> bool {
        let admit = match self.key_press_owner.get(key) {
            Some(owner) => owner == window_id && self.is_key_dispatch_window(window_id),
            None => self.is_key_dispatch_window(window_id),
        };
        if admit {
            // Pin the owner on the admitted rising edge only; a continuation
            // (owner already present) leaves the snapshot untouched so a
            // window-close handoff cannot re-point it at the successor window.
            self.key_press_owner
                .entry(key.to_owned())
                .or_insert_with(|| window_id.to_owned());
        }
        admit
    }

    /// R1071 PR-27 §5.39 §5.16 §5.35 — per-window named-key (shortcut)
    /// injection, the keyboard peer of the per-window pointer entries
    /// ([`Self::mouse_pressed_for_window`] et al.). Applies the full dispatch
    /// gate ([`Self::admit_key_press`] — OS focus + R1073 press-owner snapshot)
    /// then routes `key` through the named-key arc
    /// (`Self::handle_named_key_inner`) carrying `repeat`. Returns whether the
    /// gate ADMITTED the press (`true` = dispatched to the widget arc
    /// regardless of whether a widget consumed it; `false` = gated out because
    /// the press arrived at a non-focused window, or is a stray re-delivery of
    /// a press already owned by another window).
    ///
    /// This is the substrate seam that makes the multi-window toggle
    /// double-dispatch reproducible and regression-testable headlessly — a
    /// `#[test]` drives "a key (with `repeat`) arrives at window X" with no
    /// winit `EventLoop`, wgpu device, or external key injector, exactly as
    /// the pointer seam lets tests drive per-window clicks. `AppShell`'s
    /// `WindowEvent::KeyboardInput` arm drives the SAME gate for the live GUI.
    ///
    /// Modifiers are read from the out-of-band `Self::modifiers` cache (the
    /// winit `ModifiersChanged` mirror — winit's `KeyEvent` carries no
    /// modifier state, so the real path reads the cache too); a test sets them
    /// via [`Self::set_modifiers`] before driving a modifier-bearing shortcut.
    pub fn key_press_for_window(&mut self, window_id: &str, key: &str, repeat: bool) -> bool {
        if !self.admit_key_press(window_id, key) {
            return false;
        }
        self.handle_named_key_inner(key, repeat);
        true
    }

    /// R1076 PR-28 §5.39 §5.16 §5.35 — the live winit key-edge gate decision: the
    /// synthetic-aware home `AppShell::handle_keyboard_input` delegates to (it
    /// updates the passive chord cache [`Self::note_key_state`] separately, for
    /// both edges). Returns `Some(repeat)` when the edge should DISPATCH — the
    /// caller runs `handle_key_press` with that auto-repeat flag — and `None`
    /// when it is gated out, maintaining the press-owner lifecycle as the side
    /// effect. **R1078 PR-28.2**: `repeat` is DERIVED from the press-owner gate
    /// (`Some(true)` = the key was already owned = a continuation), NOT winit's
    /// `event.repeat`, which winit resets on every focus transition (see the body).
    ///
    /// `is_synthetic` is winit's flag for a focus-transition key-state
    /// notification — a `Pressed` emitted for every held key when a window GAINS
    /// OS focus, a `Released` when it LOSES focus (X11 / Windows only). These are
    /// not user intent: a held shortcut whose dispatch MOVES OS focus would
    /// otherwise self-toggle (the sprag dock/undock flap — the toggle moves
    /// focus, the focus change emits a synthetic `Pressed` of the still-held
    /// shortcut, which fires the toggle again, holding-loop). So a synthetic edge
    /// drives NEITHER the gate, NOR the press owner ([`Self::admit_key_press`]
    /// pin / [`Self::note_key_release`] clear), NOR dispatch — the physical key
    /// is unchanged across the transition, so the owner a real keydown pinned
    /// survives until the matching PHYSICAL keyup. This also stops a synthetic
    /// `Pressed` from forwarding a phantom keystroke to the newly-focused
    /// widget's External (e.g. a stray newline into a terminal PTY).
    ///
    /// For a physical edge (`is_synthetic == false`) the DISPATCH DECISION is
    /// byte-identical to the pre-R1076 inline gate: a `Pressed` admits through
    /// [`Self::admit_key_press`] (`gate_key`) or [`Self::is_key_dispatch_window`]
    /// (a key the shell does not dispatch — media / dead keys, `gate_key`
    /// `None`); a `Released` clears the press owner. The derived `repeat`
    /// (R1078) equals winit's `event.repeat` for a single-window hold and only
    /// corrects it across focus transitions. This is the headless seam that
    /// makes the synthetic exclusion AND the repeat derivation regression-testable
    /// without a winit `EventLoop` (the R1071 `key_press_for_window` discipline).
    ///
    /// **Residual (honest, R1078.1 audit):** the derived `repeat` inherits the
    /// R1073.1 missed-keyup residual. If a physical keyup never reaches this seam
    /// (it is delivered to no window — the only way an owner is left stranded,
    /// since [`Self::note_key_release`] is the sole clear path and `remove_window`
    /// deliberately does not reconcile the owner), the stranded owner makes the
    /// *next genuine first press at that same window* derive `repeat == true` and
    /// be dropped ONCE; the next physical keyup then clears the owner and a fresh
    /// press acts normally. A press at any *other* window is gated out by
    /// [`Self::admit_key_press`] (owner ≠ window), not mis-flagged. This is the
    /// same eventual-keyup trust the press-owner gate already makes; clear-on-blur
    /// stays rejected (it reintroduces the winit focus-ordering dependence the
    /// press-time snapshot removed). Far milder than the infinite flap it fixes.
    #[must_use]
    pub fn apply_key_edge(
        &mut self,
        window_id: &str,
        gate_key: Option<&str>,
        pressed: bool,
        is_synthetic: bool,
    ) -> Option<bool> {
        if is_synthetic {
            return None;
        }
        if pressed {
            match gate_key {
                Some(key) => {
                    // R1078 PR-28.2 — derive the auto-repeat flag from the press-owner
                    // gate BEFORE `admit_key_press` pins it: an owner already present
                    // means the key was physically held when this `Pressed` arrived,
                    // i.e. a continuation (auto-repeat). This is the focus-transition-
                    // robust repeat source — winit resets its own repeat detector on
                    // every focus change, so a held shortcut whose dispatch bounces OS
                    // focus would see `event.repeat == false` on each auto-repeat (the
                    // residual sprag dock flap R1076 left).
                    let is_repeat = self.key_press_owner.contains_key(key);
                    self.admit_key_press(window_id, key).then_some(is_repeat)
                }
                // A key the shell does not dispatch (media / dead key): gate on OS
                // focus, never a repeat (not owner-tracked, `handle_key_press` no-ops
                // on it anyway).
                None => self.is_key_dispatch_window(window_id).then_some(false),
            }
        } else {
            if let Some(key) = gate_key {
                self.note_key_release(key);
            }
            None
        }
    }

    /// R51.80 §5.16 §5.36 — compute one frame's paint scene from the
    /// cached state.
    ///
    /// Encapsulates the per-frame pump that the surface-side render
    /// path drives every redraw:
    ///
    /// 1. Measure `dt` against the previous paint timestamp
    ///    (`Self::last_paint_instant`) — `0.0` on the very first
    ///    paint, otherwise `now - prev` as `f32` seconds.
    /// 2. Advance every animation attached to
    ///    [`CoreShell::root_owner`](pinion_runtime::CoreShell::root_owner)
    ///    through
    ///    [`CoreShell::tick_animations`](pinion_runtime::CoreShell::tick_animations)
    ///    so the §5.22 [`Signal`](pinion_core::Signal) values the view
    ///    fn is about to read reflect the just-elapsed slice of
    ///    real time.
    /// 3. Hand the same `dt` to [`Frame::with_dt`](pinion_core::Frame::with_dt)
    ///    so deterministic-time-dependent view-fn logic (Tween
    ///    progress reads, dt-conditional layout) sees the same delta
    ///    the spring solver did.
    /// 4. Run `V::view(state, &frame)` + the §5.21 layout pass against
    ///    the shared `text_cache`.
    ///
    /// R51.83 §5.40: the underlying `cached_state` / `text_cache`
    /// fields are private, so this method (and [`Self::text_cache_and_engine`]
    /// for the paint adapter borrow) is the only way for the surface
    /// to drive the pipeline. Pure with respect to substrate state
    /// modulo the timing field + `text_cache` LRU + the root owner's
    /// animation queue — every mutation is documented + tested.
    /// R671 §5.16 — single internal paint-scene producer used by both
    /// [`Self::compute_paint_scene`] (single-window / primary path)
    /// and [`Self::compute_paint_scene_for_window`] (multi-window
    /// dispatch). `window_id == None` routes through `V::view`;
    /// `Some(id)` routes through [`WidgetView::view_for_window`].
    ///
    /// Unifying the two producers permanently rules out parity drift:
    /// every paint-pipeline side effect (tick, view, scroll-dirty
    /// re-run, animation-loop heartbeat) is written in exactly one
    /// place so future extensions (theme reactivity hooks, hot-reload
    /// triggers, post-paint owner cleanup) can not be applied to one
    /// variant only. R670.B mid-refactor regression (`todomvc_r665`
    /// animation heartbeat) was the canonical signal; see
    /// [[r670b-paint-scene-producer-parity]] for the long-form
    /// post-mortem.
    fn compute_paint_scene_internal(&mut self, window_id: Option<&str>, w: u32, h: u32) -> Scene {
        // R680 atomic 1 §5.16 §5.28 §5.41 — resolve the canonical
        // window key. `None` (single-window legacy paint entry)
        // maps to `DEFAULT_WINDOW` so the per-window last-paint
        // map + per-window animation tick both target the primary
        // slot. Single-window bindings observe bit-identical
        // behaviour because [`CoreShell::window_owner`]'s
        // `DEFAULT_WINDOW` entry is a `root_owner` clone (R680
        // atomic 0 seeding).
        let window_key = window_id.unwrap_or(pinion_runtime::DEFAULT_WINDOW);
        let now = Instant::now();
        let raw_dt = self
            .window_state(window_key)
            .and_then(|s| s.last_paint_instant)
            .map_or(0.0_f32, |prev| now.duration_since(prev).as_secs_f32());
        self.window_state_mut(window_key).last_paint_instant = Some(now);
        // R51.145 §5.28 — clamp before reaching the spring solver +
        // the view fn so background-resume / debugger-pause does not
        // destabilize the semi-implicit Euler integrator. Healthy
        // 60fps frames pass through unchanged; only paused / blocked
        // resumes get capped.
        let dt = clamp_frame_dt(raw_dt);
        // R680 atomic 1 §5.16 §5.28 — per-window animation tick.
        // Walks only `window_owners[window_key]`'s own animation
        // list (NOT the binding-wide cascade); two windows painting
        // in the same event-loop turn each call this for their own
        // key, and each one's animations advance exactly once. The
        // R670.B 9-round honest carry on multi-window animation
        // compound is closed structurally here.
        self.core.tick_animations_for_window(window_key, dt);
        // R1549 §5.35 §5.38 — advance press-and-hold auto-repeat, after the
        // animation clock and BEFORE the view: a repeat that fires here is
        // an input that arrived just before this frame, so `V::view` reads
        // the stepped value on this paint rather than the next one. The
        // tail goes through `handle_tail` like any other dispatched input,
        // so a repeated click reaches `V::update` exactly as the first did.
        //
        // This is the LIVE clock arm — the wall-clock delta, frozen to zero
        // while the window is paused (`scene/set_fps 0`) so an incidental
        // repaint cannot fast-forward a frame-stepped hold, exactly as
        // `sim_dt` below freezes the immediate-mode loop. The INJECTED arm
        // is the `DeferredInput::Tick` handler, which advances the same one
        // driver through the same `substep` splitter the animation clock
        // uses: one stepping algorithm, two clock sources — the shape
        // `tick_animations_for_window` already has.
        //
        // An injected tick must land THERE and synchronously, not on some
        // later repaint: the RPC producer runs its own view + layout
        // pipeline rather than this fn, so a client that ticked and then
        // read would be racing a repaint it has no way to observe.
        //
        // `_pure_internal` deliberately omits all of this, exactly as it
        // omits `reconcile_frame`: a dry_run / introspection paint advances
        // no clock and fires no input.
        let paused = self.target_fps_for_window(window_key) == Some(0);
        let repeat_dt = if paused { 0.0 } else { dt.max(0.0) };
        let (repeat_tail, repeat_armed) = self
            .core
            .tick_pointer_hold_for_window(window_key, repeat_dt);
        self.handle_tail(&repeat_tail);
        // R1006 §5.23 §5.22 — publish the layout viewport to the binding
        // BEFORE the view runs, so a reflow Effect (e.g. a PTY winsize ioctl)
        // reacts and the view reads the post-reflow producer state on this same
        // paint. Primary window only: the single root-seeded signal is shared,
        // so a secondary paint must not overwrite the primary's size (R1006
        // carry — per-window signal deferred). `set_viewport_size` wraps the
        // write in `root_owner.run` (blocker B) and equality-skips a same-size
        // repaint, so calling it every paint is cheap. The side-effect-free
        // paint mirror (`_pure_internal`) deliberately omits this publish.
        if window_key == pinion_runtime::DEFAULT_WINDOW {
            self.core.set_viewport_size(w, h);
            // R1047 §5.23 §6.3 — pre-view binding reconcile pass. Runs
            // AFTER the `set_viewport_size` publish (so any winsize reflow
            // Effect already updated the producer) and BEFORE the view fn,
            // inside `root_owner`, so a binding can grow + tail-follow a
            // `ScrollState` whose content extent lives in an off-thread
            // producer (a PTY's scrollback) without writing a Signal from
            // the pure view fn. Gated to the primary like
            // `set_viewport_size` (binding-wide reactive state reconciled
            // once per paint; R1006 per-window carry inherited). The
            // side-effect-free paint mirror (`_pure_internal`) omits it, so
            // an introspection / dry_run paint never reconciles.
            self.core.reconcile_frame();
        }
        let frame = Frame::with_dt(dt);
        let cached_state = *self.core.cached_state();
        // R51.146 §5.22 §5.16 — wrap the view fn in `root_owner.run(...)`.
        //
        // R680 atomic 1 design decision: view fns continue to run
        // under the binding-wide [`CoreShell::root_owner`] scope, NOT
        // the per-window child scope. The per-window
        // [`CoreShell::window_owners`] map exists for animation tick
        // decoupling + future per-window scope cleanup (R683 dock-
        // panel tear-off lifecycle), but the view-fn wrap stays on
        // root so:
        //
        // 1. Cross-window state sharing via [`Owner::cache`] keeps
        //    working without binding-level adjustment — hello-
        //    multi-window's `use_selected_path` /
        //    `use_hovered_path` slots resolve through root regardless
        //    of which window is painting, matching the RPC paint
        //    path (`dispatch_rpc`'s produce closure) which also
        //    wraps in root. Live winit-driven paint and synthetic
        //    RPC-driven paint stay observationally identical
        //    (§2 #2 + §2 #7 invariant preservation).
        // 2. Phase A bindings (the entire single-window example
        //    catalogue) observe bit-identical behaviour — same
        //    `Owner::current()` resolution from inside `V::view`,
        //    same cache slot semantics, same animation registration
        //    target.
        //
        // The compound-tick elimination (R670.B 9-round honest
        // carry) is achieved through the [`Self::tick_animations_for_window`]
        // local-walk dispatch above: ticking `DEFAULT_WINDOW` walks
        // root's own animation list once per primary paint; ticking
        // a secondary `window_id` walks ONLY that secondary's
        // child-scope animations (typically empty for current
        // bindings — all animations live on root). Two windows
        // painting in the same event-loop turn each call
        // `tick_animations_for_window(THEIR_id, …)`; the secondary's
        // walk is a no-op for root-registered animations, so
        // animations advance once per primary paint regardless of
        // how many secondary paints fire in the same turn.
        // R1458 §5.45 §5.27 — the frame runs to a FIXED POINT before it is
        // painted. A layout pass writes state the view reads back (the scroll
        // bound, the measured-row table, a pane's measured rect), so the scene
        // it just laid out can already be out of date by the time it returns:
        // the honest thing to paint is the scene of the pass that changed
        // nothing. Pre-R1458 this was two hand-rolled single-shot re-passes
        // (scroll-dirty, then pane-dirty) whose own dirty bits were DISCARDED —
        // so a frame that needed a third pass was presented mid-settle, and,
        // because the paint ends with `clear_dirty()` and the event loop is
        // `ControlFlow::Wait`, nothing asked for the frame that would have
        // finished it. The user saw a stale offset until the next keystroke
        // (the tide field report's "the pin lands, but the paint that shows it
        // never comes"). The loop below replaces both, and the budget's job is
        // only to bound a pathological non-converging binding: exceeding it
        // arms a catch-up redraw rather than presenting-and-forgetting.
        //
        // The view + every re-pass run the SAME `V::view` / `V::view_for_window`
        // dispatch; factor it into one closure so the dispatch lives in exactly
        // one place. The closure captures only `&self.core` (disjoint from the
        // `&mut self.text_cache` the layout passes borrow) and is scoped to this
        // block so its `&self.core` borrow drops before the later `&mut self`
        // paint-loop work. Folding the layout into the dispatch closure would
        // double-lay-out the first pass.
        // R1121 §5.16 §5.21 — borderless windows inset content below their
        // chrome strip (`Some(height)`); decorated windows are `None` (no-op).
        // R1467 — the inset and the strip that fills it come from ONE sample.
        let overlays = self.sample_window_overlay_inputs(window_id);
        let chrome_h = overlays.chrome_inset();
        let (mut paint_scene, paint_work) = {
            let core = &self.core;
            let run_view =
                || window_view::<V>(core.root_owner(), window_id, cached_state, frame, chrome_h);
            // R1072 §5.37 — the opt-in self-hosted measure override (None unless
            // `PINION_TEXT_ENGINE` is enabled). Borrows `self.text_engine` as a
            // field disjoint from the `&mut self.text_cache` the layout passes
            // take; `Copy`, so the same override threads through every re-pass.
            // Paired with the §5.37 paint arm at the `text_cache_and_engine`
            // site so both arms size + render eligible text identically.
            let text_measure = text_measure_override(self.text_engine.as_ref());
            // R1459 §5.16 §5.36 — the shaper-miss mark. Read before any pass so
            // the delta below is THIS paint's shape work, not the cache's
            // lifetime total; the cache is shared across windows, so only a
            // per-paint delta is attributable to a window's frame.
            let shapes_before = self.text_cache.shapes();
            // R1538 §5.16 — the node census accumulators. `scene_nodes` is
            // overwritten by each pass (so it ends holding the LAST pass's —
            // the tree that is painted); `layout_nodes` sums them (so it ends
            // holding what the settle loop paid for). A frame that converges on
            // its first pass reports the same number twice, which is the
            // correct statement that nothing was re-measured.
            let (mut scene_nodes, mut layout_nodes) = (0_u32, 0_u32);
            let text_cache = &mut self.text_cache;
            let settle = settle_to_fixed_point(run_view, |scene| {
                // R57.X.scrollbar §5.45 — first-paint chicken-and-egg. The
                // layout pass writes the post-layout
                // [`ScrollState::set_max`] *after* `V::view` has already
                // produced the scene. The scrollbar widget reads `max` inside
                // `V::view` and renders thumb size as
                // `f(viewport, viewport + max)` — on the very first paint of
                // the application's lifetime `max == 0` resolves to "content
                // fits viewport" and paints a full-track thumb the user sees
                // as "scrollbar maxed out at startup".
                //
                // R1012 §5.23 §5.22 — per-pane viewport publish, folded into the
                // same fixed point. The freshly laid-out scene carries each pane
                // Container's measured pixel rect; publishing each registered
                // pane tag's (w, h) lets a per-pane reflow Effect (a PTY winsize
                // ioctl) react, and the re-run reads the post-reflow producer
                // state on this same paint. `|`, never `||`: the publish is the
                // point of the call, so it must run on every pass, not only when
                // the layout happened to be clean.
                //
                // R1021 §5.23 §5.16 — published for EVERY painted window, NOT
                // primary-only (unlike the R1006 `set_viewport_size` publish
                // above, which stays `DEFAULT_WINDOW`-gated). The pane registry
                // is `root_owner`-scoped and tag-keyed, so it is window-agnostic:
                // each painted window publishes the rects of the tags IT draws,
                // and a tag absent from this window's scene resolves
                // `rect_for_tag_absolute → None` and is skipped (retains its last
                // measured size — a foreign window's pane is never clobbered). In
                // the dock model a pane tag is drawn in exactly one window at a
                // time, so there is no ambiguity. This is what lets a torn-off
                // (undock) pane reflow to its secondary window's size: that
                // window's content fills `(w, h)` via `compute_layout`'s
                // root-fill (the root's declared size is ignored at the top
                // level), so the pane Container's measured rect IS the window
                // rect — no per-window `use_viewport_size` is needed (the R1006
                // window-size seam stays primary-gated; sprag R37 undock is the
                // forcing consumer). The side-effect-free mirror
                // (`compute_paint_scene_pure_internal`) never reaches this fn, so
                // an introspection paint never publishes (the R1006 contract).
                // R1538 §5.16 — the node census. `nodes` is what THIS pass
                // measured, so the running sum is the layout work the whole
                // settle loop paid for and the last value written is the tree
                // that will be painted. Recorded here rather than derived after
                // the loop because only the pass knows its own size, and a
                // post-hoc walk to recover it would be the O(scene) cost the
                // census exists to bound.
                let pass = compute_layout_with_text_measure(scene, text_cache, w, h, text_measure);
                scene_nodes = pass.nodes;
                layout_nodes = layout_nodes.saturating_add(pass.nodes);
                pass.scroll_dirty | core.publish_pane_viewports(scene)
            });
            (
                settle.scene,
                PaintWork {
                    passes: settle.passes,
                    settled: settle.settled,
                    shape_misses: self.text_cache.shapes() - shapes_before,
                    scene_nodes,
                    layout_nodes,
                },
            )
        };
        // (R1020 §5.39) Re-derive the keyboard focus enumeration from the
        // freshly produced paint scene — the ratified scene-derived focus
        // model (`collect_focusable_tags` reads each node's `tag` +
        // `LayoutStyle::focusable`, both view-fn-assigned, so the post-layout
        // tree carries them unchanged). A node marked `.with_focusable(true)`
        // joins the Tab order by being painted; a conditionally-painted node
        // (a dynamic pane, an inline editor) joins / leaves automatically with
        // no binding-side list.
        //
        // R25.1 §5.39 §5.16 — refresh THIS window's contribution and re-fold the
        // union across the declared topology (R26: painted windows PLUS windows
        // declared in `windows_signal` but not yet painted). No longer
        // primary-gated: a torn-off (undock) pane drawn only in its secondary
        // window must join the Tab order / click-focus set, or it cannot receive
        // keyboard focus and its PTY never sees input (sprag undock). The union is
        // keyed per window (the R1021 `publish_pane_viewports` precedent), so a
        // secondary window's paint refreshes its own tags without clobbering the
        // primary's; the §5.39 stale-focus drop + modal guard inside
        // `update_focusable_tags` now act against the union (focus drops only when
        // the tag is in no declared window). A single-window binding has one entry
        // and no declared signal, so the union is its window's
        // `collect_focusable_tags()` verbatim — byte-identical to the pre-R25.1
        // primary-only enumeration. The side-effect-free mirror
        // (`compute_paint_scene_pure_internal`) never reaches this fn, so an
        // introspection paint never enumerates (the R1006 contract, inherited).
        self.refresh_window_focusables(window_key, paint_scene.collect_focusable_tags());
        // R681 §2 #4 atomic 1 / R831 — per-window immediate-mode tick.
        // The paint scene the view fn just produced may contain one or
        // more [`Scene::ImmediateModeNode`]s; each driver advances its
        // own state in fixed-timestep steps (R831 accumulator below), and
        // the fixed step lands in [`ImmediateModeNode::last_dt`] for AI
        // introspection. The paint adapter
        // ([`pinion_runtime::paint_adapter::to_vello`]) invokes
        // [`pinion_core::scene::ImmediateMode::paint`] later in the
        // frame (after this tick), so the painted geometry reflects
        // the post-tick driver state.
        //
        // The immediate-mode presence signal flags this binding as
        // immediate-mode-active for the rest of the paint cycle:
        // we mark `redraw_requested` so the next frame fires from
        // the per-window paint clock without waiting on input —
        // the §2 #4 game-loop contract. The
        // [`ControlFlow::WaitUntil`] frame-budget pacing on top of
        // this signal lives in
        // [`ApplicationHandler::about_to_wait`](crate::app) (R681
        // atomics 2 + 3 — per-window deadline +
        // [`pinion_runtime::frame_pacing::frame_budget_for_window`]).
        // R831 §2 #4 §5.28 — fixed-timestep accumulator (Glenn Fiedler,
        // "Fix Your Timestep!"). Whichever clock sourced this frame's
        // elapsed time, feed it into the SAME per-window
        // [`pinion_runtime::FixedTimestep`] so the immediate-mode game
        // loop advances in EXACTLY fixed steps with the sub-step
        // remainder carried forward — making the simulation
        // frame-rate-independent AND making `scene/tick` reproduce live
        // behaviour deterministically (the pre-R831 split ran live as one
        // variable wall-clock step and `scene/tick` through the
        // [`pinion_runtime::substep`] splitter — two time bases that
        // could not reproduce each other; [[verify-seed-claims-audit-first]]).
        //
        // `paused` (computed ONCE, up at the pre-view auto-repeat advance)
        // gates this frame's source clock, the continuous-loop redraw re-arm
        // further down, AND the R1549 held-press clock, so the three cannot
        // disagree on the pause state.
        // Simulation seconds to advance this frame:
        //   - a pending `scene/tick` injection (an AI client frame-stepping
        //     a — typically paused — window), if present; else
        //   - `0.0` when paused: the sim clock is FROZEN, so an incidental
        //     repaint (a click forwarding `on_pointer_down`, a focus-ring
        //     redraw, a resize) does NOT fast-forward a long-paused game;
        //   - the clamped wall-clock delta otherwise (the live loop).
        let sim_dt = if let Some(injected) = self.take_pending_immediate_dt(window_key) {
            injected
        } else if paused {
            0.0
        } else {
            dt.max(0.0)
        };
        // Drive the accumulator: it invokes `tick_immediate_mode` once per
        // WHOLE fixed step and carries the remainder. `last_dt` therefore
        // publishes the fixed simulation timestep (not the wall-clock
        // frame delta) — `Duration::ZERO` until the first whole step
        // fires. A frame whose accumulated time is still sub-fixed (or a
        // frozen/paused frame) runs zero steps and leaves the drivers
        // untouched.
        self.window_state_mut(window_key)
            .sim_accumulator
            .get_or_insert_with(pinion_runtime::FixedTimestep::default)
            .advance(sim_dt, |fixed| {
                paint_scene.tick_immediate_mode(Duration::from_secs_f32(fixed));
            });
        // R830 §2 #4 §5.20 — presence gate, decoupled from whether the sim
        // ADVANCED this frame. A pointer press buffers a §5.20 intent via
        // `on_pointer_down` independently of ticking, so the drain below
        // must run on EVERY frame that has immediate-mode nodes — even a
        // paused/sub-fixed frame that ran zero steps. `has_immediate_mode_subtree`
        // is the pure presence check (no tick, no `last_dt` side effect),
        // replacing R830's `tick(ZERO)`-for-the-count workaround.
        if paint_scene.has_immediate_mode_subtree() {
            // R829 §2 #4 §5.28 — re-arm the continuous game loop UNLESS
            // this window is paused (`scene/set_fps 0`). The immediate
            // tick self-re-arming the redraw is the R681 "first half of
            // the game-loop contract" — but a paused window must advance
            // ONLY on an explicit `scene/tick` step and then refreeze, so
            // suppressing the re-arm here is what makes the pause
            // actually stop the loop (the `about_to_wait` `WaitUntil`
            // deadline alone does not — this flag would keep firing
            // paints uncapped). The single stepped paint was already
            // triggered by the `scene/tick` drain's redraw request; not
            // re-arming lets `about_to_wait` settle to `ControlFlow::Wait`.
            if !paused {
                self.redraw_requested = true;
                self.request_redraw_for_window(window_key);
            }
            // R827 §2 #4 §5.20 — immediate → retained intent bridge.
            // Each driver just advanced its simulation in
            // `tick_immediate_mode`; harvest any §5.20 intents it
            // emitted and route them through `V::update`, the same
            // reducer arc retained widget intents flow through
            // ([`Self::handle_tail`]). This closes the §2 #4
            // dual-execution *bidirectional* contract: retained → immediate
            // shares the driver handle via the view fn; immediate →
            // retained flows here, so the game loop drives retained app
            // state (score, game-over, spawned entities). The mutated
            // reactive state reflects on the NEXT frame — the armed
            // redraw above guarantees that frame fires from the
            // per-window paint clock. Drains only `ImmediateModeNode`s
            // (not `Scene::External`, already harvested from the state
            // scene in [`CoreShell::tail`]), so no widget double-drains.
            let mut immediate_intents = IntentQueue::new();
            walk_scene_and_drain_immediate(&mut paint_scene, &mut immediate_intents);
            for intent in immediate_intents.drain() {
                let _ = self.core.route_intent_through_update(&intent);
            }
            // R830 §2 #7 — reactive catch-up redraw (the R705.1 dirty
            // bridge, applied to the immediate→retained path). `V::view`
            // ran BEFORE the routing above, so any retained `Signal` a
            // routed intent just mutated is not yet rendered. Arming a
            // redraw when the owner is dirty makes the next frame re-view
            // with the updated state. This is REACTIVE (one-shot, settles
            // when state stops changing), distinct from the CONTINUOUS
            // game-loop re-arm gated on `!paused` above — so even a PAUSED
            // window renders an immediate-intent's retained effect (e.g. a
            // click on a paused game updating a score) on exactly one
            // catch-up frame, then refreezes.
            if self.core.root_owner().is_dirty() {
                self.redraw_requested = true;
                self.request_redraw_for_window(window_key);
            }
        }
        // R1426 §5.41 §5.28 — arm this window's terminal-cursor blink clock from
        // the paint scene; the `any_animation_active` gate below then advances the
        // phase. This live fn only, never the pure mirror — a dry_run paint arms
        // no clock. R1427 — the wrapper gates the arm on OS focus (unfocused =
        // stop-blink), reading the SAME fails-open predicate as `cursor_focused`.
        self.arm_grid_cursor_blink(window_key, &paint_scene);
        // R51.147 §5.28 — keep painting while any animation registered
        // on the binding is still moving. `request_redraw` is
        // idempotent inside winit; the redraw flag is drained once per
        // event-loop iteration and forwarded to
        // `Window::request_redraw` when a window exists, otherwise
        // observed by headless tests. Once every animation settles
        // under the spring epsilon the call short-circuits and the
        // surface idles until the next input / state change.
        if self
            .core
            .any_animation_active(pinion_core::DEFAULT_REST_EPSILON)
        {
            self.redraw_requested = true;
        }
        // R1549 §5.35 §5.38 — keep painting while a press-and-hold repeat
        // is armed. This is the `Tickable::is_at_rest` peer for a gesture
        // that lives in the router rather than the owner's animation
        // registry: without it a `ControlFlow::Wait` loop would paint the
        // frame the press opened and then sleep, and the hold would fire
        // exactly once — the very behaviour the round exists to fix. The
        // flag falls to `false` the moment the widget stops declaring a
        // cadence (released, slid off, disabled, or a spin arrow at its
        // bound), so a forgotten finger does not pin the loop awake.
        if repeat_armed {
            self.redraw_requested = true;
        }
        // R761.1 §5.22 — drain the owner-scoped `LocalTaskPump` one step
        // per frame so deferred async work (a native file dialog awaiting
        // an xdg-portal reply, any `Resource::fetch_with`) advances on
        // the UI thread. We gate on whether any task existed *before* the
        // poll: if so, request another frame — either the task is still
        // pending (re-poll next frame) or it just resolved here (after
        // `V::view`), flipping its `Resource` Signal, so the next frame
        // must re-view to paint the result. The loop idles once the pump
        // drains (same "stay awake while active" contract as the
        // animation check above). v1 busy-polls with `Waker::noop`; a
        // wake-channel waker is a forward refinement (R761.1 carry).
        let task_pump = pinion_core::LOCAL_TASK_PUMP.resolve(self.core.root_owner());
        if task_pump.has_pending() {
            task_pump.poll();
            self.redraw_requested = true;
        }
        // R1459 §5.16 — publish this paint's WORK counts for the surface to
        // fold into its `FrameTiming` sample. Written before the unsettled
        // report so a budget-exhausted frame is readable as data
        // (`settle_passes == SETTLE_PASS_BUDGET`) and not only as a log line.
        self.record_paint_work(window_key, paint_work);
        // R705.1 §2 #7 — this paint just consumed the current reactive
        // state (the `V::view` run above re-subscribed `root_owner` to
        // every `Signal` it read). Reset the dirty flag so the NEXT
        // `Signal::set` re-flags it — the `handle_tail` `is_dirty()` bridge
        // then knows a real change occurred and a benign no-op did not.
        self.core.root_owner().clear_dirty();
        let dressed = overlays.apply::<V>(paint_scene, w, h, self.core.root_owner());
        self.warn_about_escaped_marks(&dressed);
        self.announce_surface_sizes(window_key, &dressed);
        dressed
    }

    /// R1656 §5.15 — tell each painted `External` the size of the rect it was
    /// laid out into, whenever that changes.
    ///
    /// The §5.15 lifecycle arm this drives had been declared and never called;
    /// [`pinion_runtime::announce_external_sizes`] carries the measurement of
    /// what that cost. Runs after the overlay chain so a widget under chrome is
    /// told the size it actually got, and takes the state scene because that is
    /// where the handle whose fields survive the frame lives.
    fn announce_surface_sizes(&mut self, window_key: &str, paint: &Scene) {
        let known = self
            .external_sizes
            .entry(window_key.to_owned())
            .or_default();
        pinion_runtime::announce_external_sizes(paint, self.core.scene_mut(), known);
    }

    /// R1656 §5.32 §5.36 §2 #7 — say so, on the frame it happens, when this
    /// paint put a mark outside the box that owns it.
    ///
    /// # Why a warning and not a compile error
    ///
    /// Because the fact is not knowable until the string, the font and the
    /// window size are all present: the same label fits at one size and spills
    /// at another, and no signature can carry that. What the type system CAN
    /// do here is make a box that ignores its own contents unrepresentable —
    /// derive the box from the rows rather than fixing it — and where a
    /// consumer does that, this never fires. So the two halves are: a shape
    /// that prevents the authoring mistake, and this, for everything that
    /// depends on the data.
    ///
    /// # Why a warning and not a panic
    ///
    /// A panic here would take down a running application over a
    /// three-pixel line box, and the measured population is not zero — this
    /// tree had fifteen escapes on one screen the day the check was written.
    /// The gate that must be red instead of merely loud is the boot-time one
    /// every demo pays, which can carry a budget and ratchet it down.
    ///
    /// # Cost
    ///
    /// Debug builds only, and once per distinct mark: a screen that spills the
    /// same label every frame says so on the first one and then stops, so a
    /// warning cannot turn into sixty lines a second. Release builds do not
    /// walk the scene at all — the read stays available there through
    /// `scene/containment`, which is asked rather than paid for.
    #[cfg(debug_assertions)]
    fn warn_about_escaped_marks(&mut self, scene: &Scene) {
        let cache = &mut self.text_cache;
        let escapes = pinion_core::containment::escapes(scene, &mut |t| {
            let max_width = if t.rect.w > 0 { Some(t.rect.w) } else { None };
            cache.ink_size(&t.content, &t.style, &t.runs, max_width)
        });
        for escape in escapes {
            let what = escape
                .content
                .as_deref()
                .or(escape.tag.as_deref())
                .unwrap_or("<a mark>");
            let key = format!("{}|{}|{}", escape.owner, what, escape.over.worst());
            if !self.warned_escapes.insert(key) {
                continue;
            }
            tracing::warn!(
                target: "pinion::containment",
                owner = %escape.owner,
                fate = escape.fate.wire_word(),
                content = %what,
                left = escape.over.left,
                top = escape.over.top,
                right = escape.over.right,
                bottom = escape.over.bottom,
                "a painted mark is {}px outside the box that owns it — ask \
                 `scene/containment` for the whole list",
                escape.over.worst()
            );
        }
    }

    /// Release builds do not walk the scene for this; see the debug arm.
    #[cfg(not(debug_assertions))]
    #[allow(
        clippy::unused_self,
        reason = "mirrors the debug arm's signature so the call site is one line in both"
    )]
    fn warn_about_escaped_marks(&self, _scene: &Scene) {}

    /// (R1467 §5.16 §5.39 §2 #7) Sample the shell-owned state the window-overlay
    /// chain reads, so one [`WindowOverlayInputs::apply`] dresses this window on
    /// whichever paint-scene producer is running.
    ///
    /// Taken ONCE per produced scene, before the view runs, and used for BOTH the
    /// content inset ([`WindowOverlayInputs::chrome_inset`]) and the overlay tail.
    /// That single read is the point, not a saved call: pre-R1467 the inset and
    /// the strip were independent reads on two of the three producers and absent
    /// on the third, which is how the RPC produce path came to lay a chromed
    /// window's content out as if the strip were not there. One sample cannot
    /// inset without dressing, or dress without insetting.
    ///
    /// `window_id` is the producer's `Option<&str>`: the `None` single-window
    /// primary resolves to the first declared spec, and to the
    /// [`pinion_runtime::DEFAULT_WINDOW`] router key for the drag lookups.
    fn sample_window_overlay_inputs(&self, window_id: Option<&str>) -> WindowOverlayInputs {
        let window_key = window_id
            .unwrap_or(pinion_runtime::DEFAULT_WINDOW)
            .to_owned();
        // R1186 — resolve identity WITHOUT requiring chrome, then read the whole
        // policy ONCE (R1190: chrome + resizable together, not two hooks).
        // R1123.1 — every per-window lookup keys off THIS single resolution, so
        // the live paint and the mirror pick the same window's state (§2 #7); a
        // `DEFAULT_WINDOW` key would diverge for a binding whose first window is
        // not named `"main"`.
        let spec = self.resolve_window_spec(window_id);
        let policy = spec
            .as_ref()
            .map_or_else(crate::WindowPolicy::default, |(id, _)| V::window_policy(id));
        let maximized = spec
            .as_ref()
            .is_some_and(|(id, _)| self.maximized_for_window(id));
        // R705 §5.39 §5.40 — resolve the ring target through the
        // active-descendant SSOT ([`resolve_focus_ring_tag`]) so a roving
        // widget's ring tracks its active cell rather than wrapping the
        // container. `None` (nothing focused) draws no ring.
        let ring_tag = self.focus.focused().map(|focused| {
            resolve_focus_ring_tag::<V>(self.core.cached_state(), focused, self.core.root_owner())
        });
        WindowOverlayInputs {
            cross_window_drop: self.core.cross_window_drag_into(&window_key),
            drag_image: self
                .core
                .active_drag_label_for_window(&window_key, PointerId::MOUSE),
            desktop_drag_preview_active: self.core.desktop_drag_preview_active(),
            title: spec.map(|(_, title)| title),
            policy,
            maximized,
            ring_tag,
        }
    }

    /// (R1123 §5.16 §5.39) Whether `window_id`'s window is currently maximized,
    /// per the cache `AppShell::note_window_resized` syncs from winit. `false`
    /// for an unknown / never-resized window (the create-time default). Read by
    /// the client-side chrome (maximize-vs-restore glyph) and the resize border
    /// (suppressed when maximized) on BOTH the live paint and the pure mirror.
    /// Named `..._for_window` to match the per-window accessor family
    /// ([`Self::target_fps_for_window`] / [`Self::immediate_subtree_for_window`]).
    #[must_use]
    pub fn maximized_for_window(&self, window_id: &str) -> bool {
        self.window_state(window_id).is_some_and(|s| s.maximized)
    }

    /// (R1123 §5.16 §5.39) Record `window_id`'s maximized state. Called by
    /// `AppShell::note_window_resized` with winit's `Window::is_maximized()` so
    /// the cache tracks the OS truth (a borderless window's chrome maximize
    /// button is the usual trigger, but a tiling WM can maximize it too). The
    /// scene producer reads it back via [`Self::maximized_for_window`].
    pub fn set_maximized_for_window(&mut self, window_id: &str, maximized: bool) {
        self.window_state_mut(window_id).maximized = maximized;
    }

    /// (R1459 §5.16) The work this window's last paint did, as counts.
    ///
    /// The surface reads this immediately after
    /// [`Self::compute_paint_scene_for_window`] and folds it into the
    /// [`FrameTiming`] sample it is already assembling, so the two live on one
    /// wire (`scene/frame_timings`) rather than in two surfaces an agent has
    /// to correlate. [`PaintWork::default`] for a window that has not painted —
    /// the same "no sample yet" the timing ring itself reports.
    #[must_use]
    pub fn last_frame_work_for_window(&self, window_id: &str) -> PaintWork {
        self.window_state(window_id)
            .map_or_else(PaintWork::default, |s| s.last_frame_work)
    }

    /// (R1459 §5.16) Close out a paint's settle accounting: publish the work
    /// counts for the surface's [`FrameTiming`] sample, and — when the frame
    /// never arrived — ask for the one that continues it.
    ///
    /// One call rather than two because they are the same fact read twice: a
    /// frame that spent its budget is both the count an agent reads and the
    /// redraw the user needs. Splitting them is how one of the two gets
    /// forgotten at a future call site.
    fn record_paint_work(&mut self, window_key: &str, work: PaintWork) {
        self.window_state_mut(window_key).last_frame_work = work;
        if !work.settled {
            self.report_unsettled_frame(window_key);
        }
    }

    /// (R1458 §5.45 §5.27) The paint's settle loop ran out of
    /// [`SETTLE_PASS_BUDGET`] with the frame still moving, so the scene about
    /// to be presented is not the one it was converging to. Ask for the frame
    /// that continues it, and SAY SO.
    ///
    /// Without the request, the paint ends at `clear_dirty()` — which erases
    /// the only other record that anything changed — and a `ControlFlow::Wait`
    /// event loop then sits on the half-settled picture until the user presses
    /// something (the symptom the tide field report measured as "the second
    /// paint never comes"). Without the warning it would be the *same* defect
    /// one level up: truncating a frame's settling and telling nobody.
    ///
    /// Reaching here means this binding's view and layout disagree about a
    /// value each derives from the other, which no scene converges out of on
    /// its own. The redraw keeps the app responsive; the log names the bug. A
    /// converged frame — every steady-state frame, by Signal equality-skip —
    /// never calls this.
    fn report_unsettled_frame(&mut self, window_key: &str) {
        tracing::warn!(
            target: "pinion::shell",
            window = window_key,
            passes = SETTLE_PASS_BUDGET,
            "paint scene did not settle within the pass budget: the view and \
             the layout pass disagree about a value each derives from the \
             other. Painting the last pass and requesting another frame",
        );
        self.redraw_requested = true;
        self.request_redraw_for_window(window_key);
    }

    /// (R1186 §5.16) Resolve the `(spec_id, title)` of the window being painted,
    /// INDEPENDENT of chrome. `window_id == None` is the primary window (the first
    /// declared spec); `Some(id)` matches by canonical id. Reads the same
    /// `windows_signal` / `windows` SSOT the chrome resolution reads (R1467 folded
    /// the former `window_chrome_for` into
    /// [`ShellCore::sample_window_overlay_inputs`]) — extracted so the R1186
    /// resize-border decoupling can key
    /// off the window identity + [`WindowPolicy::resizable`](crate::WindowPolicy::resizable)
    /// WITHOUT requiring the window to also carry chrome (a controls-in-header
    /// floating window has `chrome: None` yet is resizable).
    fn resolve_window_spec(&self, window_id: Option<&str>) -> Option<(String, String)> {
        let core = &self.core;
        let specs = core.root_owner().run(|| match V::windows_signal() {
            Some(sig) => sig.get(),
            None => V::windows(),
        });
        let spec = match window_id {
            None => specs.into_iter().next()?,
            Some(id) => specs.into_iter().find(|s| s.id.as_ref() == id)?,
        };
        Some((spec.id.to_string(), spec.title.to_string()))
    }

    /// R670.B §5.16 — per-window paint scene producer. Same pipeline
    /// as [`Self::compute_paint_scene`] but routes through
    /// [`WidgetView::view_for_window`]
    /// instead of `V::view` so multi-window bindings can render
    /// different scenes per window (main view in the primary;
    /// inspector tree in the secondary; …).
    ///
    /// `window_id` is the canonical `&'static str` from the
    /// binding's [`WindowSpec`](crate::WindowSpec) declaration —
    /// typically `"main"` for the primary, application-defined names
    /// for secondaries.
    ///
    /// R671 §5.16: thin wrapper around
    /// `Self::compute_paint_scene_internal`; the parity carry from
    /// R670.B is permanently cleared by the unified producer.
    pub fn compute_paint_scene_for_window(&mut self, window_id: &str, w: u32, h: u32) -> Scene {
        self.compute_paint_scene_internal(Some(window_id), w, h)
    }

    /// R671 §5.16 — primary / single-window paint scene producer. Thin
    /// wrapper around `Self::compute_paint_scene_internal` with
    /// `window_id == None`, which routes through `V::view` exactly
    /// like the pre-R670.B implementation.
    pub fn compute_paint_scene(&mut self, w: u32, h: u32) -> Scene {
        self.compute_paint_scene_internal(None, w, h)
    }

    /// (R684.B atomic 2 §5.16) Pure paint-scene producer — `V::view`
    /// + `compute_layout` only, no side effects.
    ///
    /// Splits the R670.B `Self::compute_paint_scene_internal`
    /// composition into the deterministic geometry half (this fn) +
    /// the side-effect half (animation tick, immediate-mode tick,
    /// scroll-dirty re-run guard, animation-active redraw arming).
    /// Use cases:
    ///
    /// * RPC dispatch's post-finalize hook (R684 atomic 3 →
    ///   R684.B atomic 1 / 2 rewrite): the producer closure that
    ///   resolved hit-test paths already ran the full
    ///   `Self::compute_paint_scene_internal` (tick, view, layout,
    ///   immediate-mode tick, animation-active flag) once during
    ///   dispatch; the finalize hook needs a fresh paint scene to
    ///   publish into the `InputRouter`, but re-running the full
    ///   pipeline would double-tick animations and re-fire
    ///   immediate-mode side effects. The pure variant produces the
    ///   same geometry as the producer closure (deterministic at
    ///   the same `cached_state` plus viewport `(w, h)`) without
    ///   the side effects.
    /// * Headless / dry-run paths that want geometry without
    ///   advancing the binding's animation / IM state.
    ///
    /// The R51.146 `root_owner.run(...)` wrap is preserved so
    /// `Owner::cache` slots resolve correctly.
    ///
    /// ## R1465 §5.45 §5.27 §2 #7 — it settles, like the paint it mirrors
    ///
    /// Pre-R1465 this ran exactly one layout pass and dropped its dirty bit,
    /// justified by "the canonical paint already produced the right shape, so
    /// re-running here would just emit redundant work". That is true of the
    /// R684 first-paint finalize, whose producer closure settled the same
    /// window at the same viewport moments earlier. It is **false** of the
    /// R705 re-store, which is the reason this method has a hot caller at all:
    /// the produce closure ran *before* the handler mutated state, so the
    /// mirror stored afterwards is the first pass of a chain that has since
    /// moved. An agent reading `scene/snapshot from: paint` then sees an
    /// earlier pass of the picture the window is settling to — a §2 #7
    /// divergence at the one seam whose whole job is to not diverge.
    ///
    /// So the R1458 fixed point applies here too, with the side-effect budget
    /// unchanged: no pane publish (the R1006 contract), no animation or
    /// immediate-mode tick, no redraw arming. Exceeding
    /// [`pinion_runtime::SETTLE_PASS_BUDGET`] is reported as data
    /// ([`pinion_runtime::MirrorWork::unsettled`]) instead of arming a frame,
    /// because an introspection render must not schedule one.
    pub fn compute_paint_scene_pure_for_window(
        &mut self,
        window_id: &str,
        w: u32,
        h: u32,
    ) -> Scene {
        self.compute_paint_scene_pure_internal(Some(window_id), w, h)
    }

    /// (R684.B atomic 2 §5.16) Single-window mirror of
    /// [`Self::compute_paint_scene_pure_for_window`]. Routes through
    /// the substrate's [`pinion_runtime::DEFAULT_WINDOW`] key + uses
    /// `V::view` (no `view_for_window` dispatch) so single-window
    /// bindings observe bit-identical behaviour to pre-R670.B
    /// `compute_paint_scene` minus the side effects.
    pub fn compute_paint_scene_pure(&mut self, w: u32, h: u32) -> Scene {
        self.compute_paint_scene_pure_internal(None, w, h)
    }

    /// (R685.C atomic 3 §5.16) Unified internal for the pure
    /// paint-scene producers — runs `V::view` (or
    /// `V::view_for_window`) then `compute_layout`, no side effects.
    /// Mirror of the [`Self::compute_paint_scene_internal`]
    /// `window_id: Option<&str>` dispatch shape so the per-window vs
    /// single-window branch lives in exactly one place.
    ///
    /// Pre-R685.C `compute_paint_scene_pure_for_window` +
    /// `compute_paint_scene_pure` each inlined the same
    /// `root_owner.run + compute_layout` body with only the
    /// `view_for_window` vs `view` dispatch differing (S10 code
    /// duplication). R685.C lifts the shared body here; the two
    /// public methods become thin `Some(id)` / `None` wrappers.
    fn compute_paint_scene_pure_internal(
        &mut self,
        window_id: Option<&str>,
        w: u32,
        h: u32,
    ) -> Scene {
        // R1468 §5.23 §5.22 §2 #3 §2 #7 — this producer is the introspection
        // MIRROR, so it runs inside the containment scope for the extent it is
        // about to lay out to: the R1006 seam publishes `(w, h)` (so a
        // `use_viewport_size`-derived child agrees with a percentage one — one
        // question, one geometry), the live extent is restored on drop, and the
        // whole run is a `SimulationGuard` scope so neither the republish's
        // Effects nor a focus / modal mailbox write can escape a paint nobody
        // is looking at. Entered BEFORE the overlay sample and the view, since
        // both read binding state the scope is meant to hold still. Free at the
        // live extent (the R684 / R705 callers), which is the common case.
        let _mirror = pinion_runtime::IntrospectionPaint::enter(
            self.core.root_owner(),
            self.core
                .introspection_viewport_signal(window_id.unwrap_or(pinion_runtime::DEFAULT_WINDOW)),
            (w, h),
        );
        let cached_state = *self.core.cached_state();
        let frame = Frame::new();
        // R1467 — one sample feeds the inset here and the overlay tail below.
        let overlays = self.sample_window_overlay_inputs(window_id);
        let chrome_h = overlays.chrome_inset();
        // R1072 §5.37 — measure the introspection mirror with the same opt-in
        // engine the production paint path uses, so `scene/layout` reports the
        // boxes that were (or would be) painted via §5.37 (Scene-as-data coherence,
        // §2 #7). `None` keeps it parley-identical.
        let text_measure = text_measure_override(self.text_engine.as_ref());
        // R1459 §5.16 §5.36 — the shaper-miss mark, read before any pass so the
        // delta below is THIS mirror's shape work and not the shared cache's
        // lifetime total.
        let shapes_before = self.text_cache.shapes();
        let core = &self.core;
        let text_cache = &mut self.text_cache;
        // R1538 §5.16 — the mirror's node census, summed across its settle
        // passes exactly as the live paint's is.
        let mut mirror_nodes = 0_u32;
        let settle = settle_to_fixed_point(
            // R1121 §5.16 §5.21 — mirror the live path's borderless content inset
            // so the introspection snapshot matches the painted geometry (§2 #7).
            || window_view::<V>(core.root_owner(), window_id, cached_state, frame, chrome_h),
            |scene| {
                let pass = compute_layout_with_text_measure(scene, text_cache, w, h, text_measure);
                mirror_nodes = mirror_nodes.saturating_add(pass.nodes);
                pass.scroll_dirty
            },
        );
        let paint_scene = settle.scene;
        // R1465 §5.16 §2 #2 — a stored mirror is not a frame, so it rides the
        // cumulative work groups rather than the ring. `unsettled` is how the
        // budget's truncation stays loud without arming a frame this
        // side-effect-free path is not allowed to ask for.
        self.mirror_work.record(
            settle.passes,
            settle.settled,
            self.text_cache.shapes() - shapes_before,
            mirror_nodes,
        );
        // R705.1 §2 #7 — see `compute_paint_scene_internal`: this auxiliary
        // (re-store / headless) render also consumed the current reactive
        // state, so reset the dirty flag in lockstep.
        self.core.root_owner().clear_dirty();
        overlays.apply::<V>(paint_scene, w, h, self.core.root_owner())
    }

    /// R51.80 §5.40 — build the inputs to
    /// [`Self::plan_access_emit`] from a freshly-computed paint
    /// scene.
    ///
    /// Runs the pipeline `V::access_node_for_window` (R813 — per-window,
    /// default-forwards to `V::access_node`) → `enrich_names_from_scene`
    /// → `rect_for_tag` → `V::access_focus_target` in one place so
    /// the surface-side render path does not have to interleave four
    /// reads against substrate-internal state. R51.83 §5.40: the
    /// underlying `cached_state` and `focus` fields are private, so
    /// this method is the only way for the surface to assemble the
    /// emit inputs. The pure paint scene + the substrate's read-only
    /// state (focus + `cached_state`) are the only inputs; nothing
    /// on `ShellCore` mutates.
    #[must_use]
    pub fn collect_access_emit_inputs(
        &self,
        window_id: &str,
        paint_scene: &Scene,
    ) -> (Vec<AccessNode>, Option<AccessFocus>) {
        let focused = self.focus.focused().map(str::to_owned);
        let cached = self.core.cached_state();
        // (R56.1.b.1 §5.40 §5.22) `V::access_node` runs inside
        // `root_owner.run(...)` for parity with `V::view` /
        // `V::apply_key` / `V::update` (the
        // [[callback-root-owner-wrap]] family). Bindings whose a11y
        // contribution reads through reactive hooks
        // ([`use_text_edit_state`] / [`use_caret_blink`] / etc.) need
        // an active `Owner` scope to resolve the cache key; without
        // the wrap, `Owner::current()` would return `None` and the
        // hook would panic. Bindings without reactive hooks (every
        // pre-R56.1.b.1 example) are unaffected: their access_node
        // impls ignore the Owner context.
        let owner = self.core.root_owner();
        // R813 §5.40 — per-window node contribution (the live AT emit always
        // names a window, so `Some(window_id)`; the default forwards to the
        // global `V::access_node` for single-window bindings). R979 — the
        // build sequence is the `build_access_tree` SSOT the `scene/access`
        // RPC dump also runs, so the dump cannot drift from the AT emit.
        build_access_tree::<V>(
            owner,
            cached,
            Some(window_id),
            focused.as_deref(),
            Some(paint_scene),
        )
    }

    /// R51.80 §5.12 §5.35 — post-render bookkeeping.
    ///
    /// Hands the rendered scene to the [`InputRouter`](pinion_runtime::InputRouter) so the next
    /// pointer event hit-tests against current geometry; refreshes
    /// cached state and drains pending intents (winit input bypasses
    /// the dispatcher, so the substrate has to close the loop here).
    ///
    /// R890 §5.12 §5.16 — no `paint_layout` parameter any more: the
    /// stored per-window paint scene IS the layout source.
    /// `dispatch_rpc_inner` projects a [`LayoutNode`] from it on
    /// demand (`pinion_rpc::project_layout`), so there is no
    /// per-frame layout build and no mirror that can alias one
    /// window's geometry onto another (pre-R890 the binding-wide
    /// `last_paint_layout` mirror was last-writer-wins ACROSS
    /// windows, and the per-slot copy fell back to it for
    /// known-but-unpainted windows).
    pub fn finalize_frame(&mut self, paint_scene: Scene) {
        self.finalize_frame_for_window(pinion_runtime::DEFAULT_WINDOW, paint_scene);
    }

    /// R672 §5.12 §5.35 §5.41 — per-window variant of
    /// [`Self::finalize_frame`]. `AppShell::render_window` calls this
    /// with the resolved [`crate::WindowSpec::id`] so the addressed
    /// window's per-slot [`pinion_runtime::InputRouter`] sees the
    /// paint scene (cross-window paint never overwrites another
    /// window's `last_paint_scene` — and since R890 that per-window
    /// scene is also the only layout source; no binding-wide mirror).
    ///
    /// (R684.B atomic 1 / R685.C atomic 4 §5.16 §5.41) Composition
    /// of two primitives: [`Self::apply_paint_for_window_with_hover_refresh`]
    /// writes the paint storage + fires the synthetic hover-arc
    /// refresh; the trailing `tail()` + `handle_tail()` drain emits
    /// any reactive intents the paint pass queued. The winit paint
    /// loop is the canonical caller.
    pub fn finalize_frame_for_window(&mut self, window_id: &str, paint_scene: Scene) {
        self.apply_paint_for_window_with_hover_refresh(window_id, paint_scene);
        let tail = self.core.tail();
        self.handle_tail(&tail);
        // R688 §5.16 §5.35 — reconcile the external set against the
        // binding's current reactive state. A live structure mutation
        // (e.g. a dock reorganize) that added / removed a surface
        // registers / drops its routable External here, before the next
        // frame's hit-test. (R689) Bindings with a static external set
        // — the default — return immediately from this call (the
        // `WidgetCore::external_set_is_dynamic` gate), so the per-frame
        // cost lands only on opt-in dynamic-set bindings.
        self.core.reconcile_externals();
    }

    /// (R685.C atomic 4 §5.16 §5.41 §5.35) Paint-storage write +
    /// synthetic hover-arc refresh, no reactive-intent drain.
    ///
    /// One of the two named paint-publish primitives the R685.C
    /// atomic 4 split crystallises (the other is
    /// [`Self::apply_paint_for_window_storage_only`]). Writes the
    /// Routes the paint scene through
    /// `InputRouter::update_paint_scene`, which fires
    /// `PointerEnter` / `PointerLeave` synthetic arcs for every
    /// active cursor whose deepest-tagged hit changed (canonical for
    /// a winit paint cycle where widgets may have moved under a
    /// stationary cursor). R890: the stored scene doubles as the
    /// `scene/layout` source (projected on demand) — no layout
    /// parameter, no binding-wide mirror.
    ///
    /// Pre-R685.C this was the bare `apply_paint_for_window`; the
    /// hover-firing side effect was implicit in the name. The
    /// dispatch hook (RPC path) deliberately did NOT use it — it
    /// inlined a storage-only write because an AI-driven RPC didn't
    /// move the cursor, so firing hover arcs would mutate widget
    /// state the RPC didn't request (the R660 `RadioGroup` regression
    /// origin). R685.C makes BOTH publish paths named primitives so
    /// the choice is explicit at every call site.
    pub fn apply_paint_for_window_with_hover_refresh(
        &mut self,
        window_id: &str,
        paint_scene: Scene,
    ) {
        self.core
            .update_paint_scene_for_window(window_id, paint_scene);
    }

    /// (R685.C atomic 4 §5.16 §5.41 §5.35) Paint-storage write with
    /// NO hover-arc refresh and NO reactive-intent drain.
    ///
    /// The storage-only twin of
    /// [`Self::apply_paint_for_window_with_hover_refresh`]. Routes
    /// the paint scene through `CoreShell::set_paint_scene_for_window`
    /// (R685 Hack 3.2 storage-only `InputRouter` primitive — no
    /// `refresh_hover` side effect). R890: the stored scene doubles
    /// as the `scene/layout` source — no layout parameter.
    ///
    /// ## Use cases
    ///
    /// * RPC dispatch's post-finalize hook (R684 atomic 3 →
    ///   R684.B atomic 1 → R685.C atomic 4 rewrite): after the
    ///   produce closure ran + the substrate's R51.123 `handle_tail`
    ///   already drained in-flight intents, the dispatch path needs
    ///   to publish the freshest paint scene so downstream RPC
    ///   hit-tests (the deferred-input drain) see current geometry.
    ///   The RPC didn't move the cursor — only the layout shifted
    ///   beneath it — so the hover-arc refresh would be incorrect.
    /// * Headless / scripted scenarios that want to seed the
    ///   `last_paint_*` mirrors without simulating a paint cycle's
    ///   drain OR firing input arcs that pollute the SCXML
    ///   transition log under assertion.
    ///
    /// Pre-R685.C the RPC dispatch hook inlined these writes — a
    /// third unnamed publish path alongside the composed finalize.
    /// R685.C lifts it into this named primitive so the dispatch hook
    /// reads declaratively.
    pub fn apply_paint_for_window_storage_only(&mut self, window_id: &str, paint_scene: Scene) {
        self.core.set_paint_scene_for_window(window_id, paint_scene);
    }

    /// R51.53 §5.39 — click → focus auto-set / background → clear.
    /// Called after every `pointer_down` (mouse Left press or touch
    /// `TouchPhase::Started`). Mirrors the W3C HTML convention:
    /// pressing on a tagged focusable widget focuses it; pressing
    /// on background blurs the focused widget. Non-focusable tagged
    /// widgets (decoration regions that respond to hover but carry no
    /// `LayoutStyle::focusable` marker) leave focus unchanged — the
    /// [`FocusManager::focus_set`] guard rejects unknown tags so
    /// the no-op falls out naturally.
    ///
    /// R56.1.h §5.38 §5.39 — focus mutation now flows through
    /// [`Self::notify_focus_change`] so any [`External`](pinion_core::External) whose
    /// focused state crossed the boundary receives
    /// [`External::on_focus_change`](pinion_core::External::on_focus_change). The `TextField` statechart
    /// (R56.1.a) consumes this hook to drive its Focus / Blur SCXML
    /// events and sync the [`CaretBlink`](pinion_core::widgets::caret_blink::CaretBlink) enabled gate (R56.1.c).
    /// R672 §5.35 §5.41 — per-window click-to-focus.
    /// Reads the addressed window's `hover_target` so focus targets
    /// the right widget — pre-R672 the binding-wide
    /// [`CoreShell::hover_target`] returned whichever window was
    /// last-painted's hover target, making multi-window
    /// click-to-focus pick the wrong widget across windows.
    fn click_to_focus_for_window(&mut self, window_id: &str, pid: PointerId) {
        let focus_before = self.focus.focused().map(str::to_owned);
        // `focus_set` / `focus_clear` return `true` only on a real focus
        // mutation (already-focused / missing-tag / already-clear all return
        // `false`), so this is the exact change boundary — the same signal the
        // programmatic `drain_focus_mailboxes` path gates its redraw on.
        let focus_changed = if let Some(target) = self
            .core
            .hover_target_for_window(window_id, pid)
            .map(str::to_owned)
        {
            // R742.3 §5.39 — the hover target is the deepest tag, which is
            // a composite sub-tag (`group#i`) for composite widgets.
            // Resolve it to the focusable tag the click lands on (the
            // sub-tag itself for per-sub-tab widgets, else the primary for
            // single-tab-stop composites). `None` means a tagged but
            // non-focusable decoration — leave focus unchanged (the W3C
            // HTML convention: only focusable elements focus on mousedown).
            match self.focus.resolve_focusable(&target) {
                Some(focusable) => self.focus.focus_set(&focusable),
                None => false,
            }
        } else {
            self.focus.focus_clear()
        };
        self.notify_focus_change(focus_before.as_deref());
        // (R1024 §5.39) A click / tap that moves focus must request a redraw,
        // mirroring `drain_focus_mailboxes`'s programmatic-focus pairing. The
        // focus ring is paint-time-injected (`WindowOverlayInputs::apply`) and a
        // `FocusManager` mutation dirties no reactive owner, so without this the
        // ring lags to the next unrelated repaint (sprag PR-13). The wake is
        // binding-wide (`request_redraw`), not per-window: the focused tag is
        // binding-wide state and the ring step reads the single
        // `FocusManager` in EVERY window's producer, so a shared-state tag
        // rendered in more than one window must repaint all of them on a focus
        // change (R1024.1: not a cross-window "steal" — the enumeration is the
        // R25.1 union across windows, so a secondary window's pane is a
        // legitimate focus target, but the focused tag is still single-valued).
        // A click that moves no focus (re-click the focused widget, a tagged
        // non-focusable decoration, an empty-background click while already
        // cleared) requests nothing.
        if focus_changed {
            self.revision.bump();
            self.request_redraw();
        }
    }

    /// R56.1.h §5.38 §5.39 — focus-change observer. Compares the
    /// pre-mutation `focus_before` snapshot to the current
    /// [`FocusManager::focused`] tag and fires
    /// [`External::on_focus_change`](pinion_core::External::on_focus_change) on the old and new externals
    /// (if either is bound to a tagged widget in the scene tree).
    ///
    /// The two-call shape (`old.on_focus_change(false)` then
    /// `new.on_focus_change(true)`) mirrors the W3C DOM `FocusEvent`
    /// dispatch order: `blur` fires on the losing focus owner
    /// before `focus` fires on the gaining owner. Widgets that hold
    /// state across the boundary (e.g. `TextField`'s IME composition
    /// commit-on-blur path, R56.1.a) rely on this ordering so the
    /// statechart raises `text_committed` on the outgoing widget
    /// before the new widget settles into its Focused state.
    ///
    /// No-op when the focused tag did not change (the `before ==
    /// after` early return preserves the pre-R56.1.h zero-cost
    /// stance for non-focus-mutating dispatches).
    fn notify_focus_change(&mut self, focus_before: Option<&str>) {
        let focus_after_owned = self.focus.focused().map(str::to_owned);
        if focus_before == focus_after_owned.as_deref() {
            return;
        }
        let scene = self.core.scene_mut();
        if let Some(tag) = focus_before {
            if let Some(node) = scene.find_external_with_tag_mut(tag) {
                node.handle.on_focus_change(false);
            }
        }
        if let Some(tag) = focus_after_owned.as_deref() {
            if let Some(node) = scene.find_external_with_tag_mut(tag) {
                node.handle.on_focus_change(true);
            }
        }
    }

    /// Dispatch one JSON-RPC frame against the LIVE state scene.
    /// `scene/invoke /external/send PointerEnter` (and friends) drive
    /// the SCXML the same way a winit click would.
    ///
    /// R47.7.2 §5.12 — `scene/layout` requests reach the framework
    /// via `DispatchContext::with_paint_producer`: the closure captures
    /// `cached_state` (`Copy`) and `text_cache` (`&mut`), runs the
    /// view fn and `compute_layout` for the hypothetical viewport,
    /// and returns the freshly-measured paint scene.
    ///
    /// R51.76 §5.40 — `resize_request` is supplied by the caller so
    /// the substrate stays winit-free. [`crate::AppShell`] constructs
    /// the production closure (calls `Window::request_inner_size` +
    /// `Window::request_redraw`); headless tests pass a no-op.
    ///
    /// Returns the optional JSON-RPC 2.0 response frame; the caller
    /// owns the IO surface (production writes to stdout; tests
    /// inspect the string).
    ///
    /// Signature note — `&mut dyn FnMut(u32, u32)` (not generic
    /// `F: FnMut`) for two reasons: (a) the downstream
    /// [`DispatchContext::with_resize_request`] takes the same
    /// `&mut (dyn FnMut + 'a)` shape, so the substrate forwards the
    /// reference straight through without re-wrapping; (b) avoids
    /// per-callsite monomorphisation of the entire dispatch body
    /// (production callsite vs test no-op closure would otherwise
    /// duplicate ~1 KiB of code).
    /// R670.B §5.16 — per-window variant of [`Self::dispatch_rpc`].
    /// Same dispatch shape but the paint producer closure routes
    /// through [`Self::compute_paint_scene_for_window`] using the
    /// supplied `window_id` (the spec id from
    /// [`crate::WindowSpec::id`]). Multi-window bindings reach this
    /// path through `crate::AppShell::dispatch_rpc`, which parses
    /// the `{window: "<id>"}` JSON-RPC frame param + resolves it
    /// against the per-window slot map before calling here.
    ///
    /// R890.1 §5.7 §5.16 — windowed dispatch entry (the `AppShell`
    /// production path; R671's `dispatch_rpc_for_window` successor).
    /// The dispatch scope is derived from the request's own
    /// `{window: "<id>"}` param through the ONE extraction home
    /// ([`Request::window_scope`]); a missing param defaults to the
    /// primary [`pinion_runtime::DEFAULT_WINDOW`]. There is no
    /// caller-supplied window argument any more — pre-R890.1 the
    /// R889 gate judged the in-band param while scoping obeyed the
    /// out-of-band argument, so a direct caller passing mismatched
    /// ids could act on a window the gate never checked. With one
    /// source the mismatch is unrepresentable.
    ///
    /// A non-string `window` param resolves to the default scope here
    /// and is then rejected by [`dispatch_parsed`]'s type gate before
    /// any handler runs.
    pub fn dispatch_rpc_scoped(
        &mut self,
        request: Request,
        resize_request: &mut dyn FnMut(u32, u32),
        screenshot: Option<pinion_rpc::Screenshot>,
        window_arenas: Option<Vec<pinion_core::memory_census::ArenaFootprint>>,
    ) -> Option<String> {
        // R1557 §5.16 — and no draw profile: attributing a frame re-encodes it
        // through the vello walk, which needs the per-window image cache the
        // `AppShell` window slots own. This entry answers
        // `scene/draw_profile` with `DrawProfileUnavailable`, the same shape
        // `origin` takes below.
        self.dispatch_rpc_scoped_from(
            request,
            resize_request,
            screenshot,
            window_arenas,
            None,
            None,
        )
    }

    /// R1552 §5.7 PINION-PR83 — [`Self::dispatch_rpc_scoped`] with the
    /// **origin** of the frame: the connection it arrived on and that
    /// connection's writer.
    ///
    /// The live-transport entry `AppShell::dispatch_rpc` uses. `origin` is what
    /// `scene/subscribe` needs and what a synthetic dispatch does not have; the
    /// bare entry above passes `None`, where the three subscription methods
    /// then answer `SubscriptionsUnavailable` by name rather than opening a
    /// stream nothing could deliver.
    #[allow(clippy::too_many_arguments)]
    pub fn dispatch_rpc_scoped_from(
        &mut self,
        request: Request,
        resize_request: &mut dyn FnMut(u32, u32),
        screenshot: Option<pinion_rpc::Screenshot>,
        window_arenas: Option<Vec<pinion_core::memory_census::ArenaFootprint>>,
        origin: Option<pinion_rpc::FrameOrigin<'_>>,
        draw_profile: Option<
            Result<pinion_runtime::DrawProfile, pinion_rpc::draw_profile::DrawProfileError>,
        >,
    ) -> Option<String> {
        let scope: String = request
            .window_scope()
            .ok()
            .flatten()
            .unwrap_or(pinion_runtime::DEFAULT_WINDOW)
            .to_owned();
        self.dispatch_rpc_inner(
            request,
            Some(&scope),
            resize_request,
            screenshot,
            window_arenas,
            origin,
            draw_profile,
        )
    }

    /// R670.B §5.7 — single-window dispatch entry. Accepts the raw
    /// JSON-RPC envelope; parses internally then forwards to
    /// `Self::dispatch_rpc_inner`. Used by single-window bindings
    /// (every non-multi-window example + the in-crate test harness).
    ///
    /// R890.1 — an explicit `{window: "<id>"}` param now scopes the
    /// dispatch (same [`Request::window_scope`] derivation as
    /// [`Self::dispatch_rpc_scoped`]; pre-R890.1 a KNOWN non-primary
    /// id was silently ignored for scoping — the residual alias
    /// smell). The entries differ only in the missing-param default:
    /// `None` here (the legacy bit-identical single-window path —
    /// `V::view`, no per-window R684 finalize) vs the primary
    /// [`pinion_runtime::DEFAULT_WINDOW`] on the windowed entry.
    pub fn dispatch_rpc(
        &mut self,
        request: &str,
        resize_request: &mut dyn FnMut(u32, u32),
    ) -> Option<String> {
        let parsed = match parse_request(request) {
            Ok(r) => r,
            // R51.83 §5.7 — parse-error frames carry id=null per the
            // JSON-RPC 2.0 spec; the helper has already built the
            // serialized response, so we just return it.
            Err(err_resp) => return Some(err_resp),
        };
        let scope: Option<String> = parsed.window_scope().ok().flatten().map(str::to_owned);
        // R1060 §5.12 — the single-window / headless entry has no live
        // surface to read back, so `scene/screenshot` gets no captured
        // frame here and falls through to `RenderBackendUnavailable`
        // (the AppShell windowed path supplies the real capture).
        // R1550 §5.7 — and no per-window arenas either: the fragment and image
        // caches live on `AppShell`'s window slots, so this entry answers
        // `scene/memory` with the shell-wide rows alone (the shape cache and
        // the process total), which is exactly what a host with no windows
        // holds.
        self.dispatch_rpc_inner(
            parsed,
            scope.as_deref(),
            resize_request,
            None,
            None,
            None,
            None,
        )
    }

    // R1026 — rustfmt's reflow pushed this 4 lines over the workspace
    // too_many_lines (100) ceiling it was kept just under; the body is a flat
    // method-dispatch match, not bloat. Extraction is deferred to the owner.
    //
    // R1557 — and one over the argument ceiling, with the profile joining
    // `screenshot` / `window_arenas` / `origin` as a fourth embedder-produced,
    // method-gated input. The hazard that lint names — a transposed argument
    // that still compiles — cannot occur here: all four are `Option`s of
    // DISTINCT types (`Screenshot`, `Vec<ArenaFootprint>`, `FrameOrigin`,
    // `DrawProfile`), so any swap is a type error. Bundling them is the shape a
    // FIFTH wants; it would churn ~20 public call sites for no safety this
    // signature does not already have, and R1557's own paint-walk bundle
    // (`PaintSession`) shows where the argument really is dangerous — that one
    // carried two positionally-interchangeable `Affine`s.
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    fn dispatch_rpc_inner(
        &mut self,
        request: Request,
        window_id: Option<&str>,
        resize_request: &mut dyn FnMut(u32, u32),
        screenshot: Option<pinion_rpc::Screenshot>,
        window_arenas: Option<Vec<pinion_core::memory_census::ArenaFootprint>>,
        origin: Option<pinion_rpc::FrameOrigin<'_>>,
        draw_profile: Option<
            Result<pinion_runtime::DrawProfile, pinion_rpc::draw_profile::DrawProfileError>,
        >,
    ) -> Option<String> {
        // R51.73 §5.40 — sample focus before dispatch so we can
        // detect `focus/set` (or any other focus-mutating method)
        // and trigger a redraw to refresh the focus ring.
        let focus_before = self.focus.focused().map(str::to_owned);
        // R682.B / R885 / R888 / R889 §5.16 §5.49 — pre-resolve every
        // window-scoped read the DispatchContext carries (cache stats,
        // input state, pacing, unknown-window verdict) before the
        // split-borrow block; see [`Self::window_scoped_rpc_reads`].
        let window_reads = self.window_scoped_rpc_reads(&request, window_id);
        // R1521 §5.36 §5.7 — the shape cache's snapshot. A GLOBAL read, not a
        // window-scoped one: this shell owns one `LayoutCache` and every
        // window shapes through it. Taken here, before the disjoint-field
        // borrow split below, and `Copy` so it costs a struct move.
        let text_cache_stats = self.text_cache.stats();
        // R1550 §5.36 §5.7 — the memory census, assembled only when asked for.
        // The embedder above hands in the per-window rows (its window slots own
        // the fragment and image caches); this adds the shell-wide shape cache
        // — which pricing WALKS, hence the method gate — and the process total
        // the arenas are read against.
        let memory_census = window_arenas.map(|arenas| {
            let mut census = pinion_core::memory_census::MemoryCensus {
                arenas,
                process_rss_bytes: pinion_platform_memory::process_rss_bytes(),
            };
            census
                .arenas
                .push(pinion_core::memory_census::MeasuredArena::arena_footprint(
                    &self.text_cache,
                ));
            census
        });
        // R1087 §5.16 §5.41 §2 #7 PR-31 — resolve the binding's declared
        // window set for `scene/windows` only (a GLOBAL read, not
        // window-scoped, so it lives here rather than in
        // `window_scoped_rpc_reads`). Computed before the disjoint-field
        // borrow split below because it reads `windows_signal` under
        // `root_owner`; gated on the method so every other dispatch pays
        // nothing.
        let declared_windows =
            (request.method == "scene/windows").then(|| self.declared_window_specs());
        // R1710 §5.16 §5.12 §2 #2 — the bounds the resized window DECLARES, for
        // `scene/resize` only. Same reason as the line above: it reads the
        // window signal under `root_owner`, so it is resolved before the
        // disjoint-field borrow split and gated on the method so every other
        // dispatch pays nothing.
        let resize_bounds =
            (request.method == "scene/resize").then(|| self.primary_window_bounds());
        // R1711 §5.16 §5.32 §2 #3 — the two reads that judge the screen at a
        // size the window is NOT at. Both live here for the reason the block
        // above does: they need `&mut self` whole (a mirror paint AND the shape
        // cache), which the disjoint-field split below takes apart, and they
        // are gated on the method so every other dispatch pays two string
        // comparisons.
        let hypothetical_reach = pinion_rpc::scroll_reach::parse_at(request.params.as_ref())
            .ok()
            .flatten()
            .filter(|_| request.method == "scene/scroll_reach")
            .map(|(w, h)| self.reach_at(window_id, w, h));
        let size_floor = (request.method == "scene/size_floor")
            .then(|| self.measure_size_floor(window_id, request.params.as_ref()));
        // R1576 §5.16 §5.41 §2 #7 — the desk, for the two methods that resolve
        // a placement against it. Gated on the method so every other dispatch
        // pays nothing, exactly as the declared-window set is.
        let display_topology =
            matches!(request.method.as_str(), "scene/displays" | "scene/windows")
                .then(|| self.display_topology())
                .flatten();
        // R1099 §5.51 §2 #7 PR-33 — pre-resolve the cross-window drop here (the
        // only place with `&self` access to every window's paint scene, before
        // the borrow split below; Scene is not Clone so it cannot be deferred
        // into a closure). Gated on the method so every other dispatch pays
        // nothing.
        let cross_window_drop = (request.method == "scene/cross_window_drop")
            .then(|| self.resolve_cross_window_drop_for_request(request.params.as_ref()))
            .flatten();
        // R1088 §5.16 §5.41 §2 #7 PR-31 — the WRITE peer of
        // `scene/windows`: `scene/window_move` writes the binding's
        // declared-position signal (the SAME `windows_signal` the read
        // `declared_window_specs` projects, so introspect + intervene
        // cannot disagree about the SSOT). Pre-resolved here, before the
        // disjoint-field borrow split below, because the resolve runs
        // under `root_owner`; gated on the method so every other dispatch
        // pays nothing. The closure captures the resolved signal (NOT
        // `self`), so it can be `&mut`-borrowed into the DispatchContext
        // inside the split block without re-borrowing `self.core`.
        // R1569 §5.39 §5.20 — `scene/accelerators`' optional `chord` parameter,
        // answered before the disjoint-field borrow split below because the
        // answer needs `self.core` (the paint scene + the focused widget's
        // claim) and `V::keybinding`. Gated on the method AND on the parameter
        // being present and readable, so every other dispatch pays nothing and
        // a malformed spelling falls through to the refusal `scene/accelerators`
        // itself produces.
        let accelerator_focus = self.focus.focused().map(str::to_owned);
        let accelerator_chord =
            pinion_rpc::resolve_chord_param(&request.method, request.params.as_ref(), |chord| {
                self.core.accelerator_for_chord(
                    pinion_runtime::DEFAULT_WINDOW,
                    accelerator_focus.as_deref(),
                    chord,
                )
            });
        // R1610 §5.16 §5.41 — generalised from R1088's position-only
        // `reposition_request`: ONE closure behind both write methods, so a
        // `scene/window_move` and a `position` patch cannot acquire different
        // semantics. `scene/window_move` builds the position-only patch.
        let declare_signal = matches!(
            request.method.as_str(),
            "scene/window_move" | "scene/window_declare"
        )
        .then(|| self.core.root_owner().run(V::windows_signal))
        .flatten();
        let mut declare_request = |patch: &pinion_rpc::WindowDeclareParams| -> bool {
            let Some(signal) = declare_signal.as_ref() else {
                return false;
            };
            let mut specs = signal.get();
            let Some(spec) = specs
                .iter_mut()
                .find(|s| s.id.as_ref() == patch.window_id.as_str())
            else {
                return false;
            };
            let before = spec.clone();
            if let Some(title) = patch.title.clone() {
                spec.title = title;
            }
            // An explicit AI reposition PINS the window (a `None`
            // WM-placed window becomes `Some` pinned) — unlike the
            // conservative user-`Moved` feedback, which only refreshes an
            // already-declared position. An explicit `null` is the inverse
            // and the only way BACK to a window-manager-placed window,
            // which R1576 makes the same argument for on the binding side.
            patch.position.apply_to(&mut spec.position);
            if let Some(display) = patch.display.set() {
                spec.display = Some(pinion_core::display::DisplayId::new(display.clone()));
            } else if patch.display == pinion_rpc::window_declare::Patch::Clear {
                spec.display = None;
            }
            if let Some(decorations) = patch.decorations {
                spec.decorations = decorations;
            }
            if let Some(level) = patch
                .level
                .as_deref()
                .and_then(pinion_core::window_level::WindowLevel::from_wire)
            {
                spec.level = level;
            }
            // Re-declaring what a window already says is an accepted
            // committing no-op: the signal's equality-skip would absorb it
            // anyway, and the guard keeps the Vec rebuild explicit. Writing
            // the signal fires the reconcile passes, which drive the live OS
            // window — declared becomes eventual-actual.
            if *spec != before {
                signal.set(specs);
            }
            true
        };
        // R1419 §5.39 §5.16 — the `scene/window_focus` drive peer of the
        // `os_focused_window` READ leg of `scene/input_state`. The closure runs
        // INSIDE the borrow-split block (no `&mut self`), so — like the R684
        // `produce_size` deferral — it only RECORDS the requested edge and
        // returns the resulting `os_focused_window` computed from a pre-block
        // snapshot; the real gate + R1419 mirror mutation is applied after the
        // block through [`Self::note_os_focus`] (the one funnel), with `&mut
        // self` restored. The target window is the request's `{window}` scope
        // (the R889 unknown-window gate already rejected an unknown scope before
        // the closure could run, so a recorded edge always names a known window).
        let is_window_focus = request.method == "scene/window_focus";
        let os_focus_target: String = window_id
            .unwrap_or(pinion_runtime::DEFAULT_WINDOW)
            .to_owned();
        let os_focus_before: Option<String> = self.os_focused_window.clone();
        let window_focus_edge: Cell<Option<bool>> = Cell::new(None);
        let mut window_focus_request = |focused: bool| -> Option<String> {
            window_focus_edge.set(Some(focused));
            // Mirror `note_os_focus` semantics for the reported resulting state:
            // a focus names the target; a blur clears only when the target IS the
            // currently-focused window (else the OS focus is unchanged).
            if focused {
                Some(os_focus_target.clone())
            } else if os_focus_before.as_deref() == Some(os_focus_target.as_str()) {
                None
            } else {
                os_focus_before.clone()
            }
        };
        // R684 atomic 3 §5.16 §5.41 §5.49 — record the viewport the
        // produce closure ran with so the post-dispatch finalize can
        // populate the addressed window's
        // [`pinion_runtime::InputRouter::last_paint_scene`] before the
        // deferred-input drain hit-tests against it. Pre-R684 a
        // freshly-spawned floating window (R683 dock tear-off; never
        // got a winit `RedrawRequested` cycle under headless RPC)
        // would have an empty per-window InputRouter, and
        // `scene/drag {window: "torn-..."}` silently no-op'd because
        // the hit-test walked an empty scene. The post-dispatch
        // finalize re-runs the paint pipeline + writes the result
        // into the InputRouter so the drag dispatch sees real
        // geometry.
        //
        // `Cell<Option<(u32, u32)>>` is the cheapest interior-
        // mutability shape that lets the `FnMut` produce closure
        // record state visible after the borrow-split block ends.
        // Scene cloning is unavailable (`ExternalNode` carries
        // `Box<dyn External>` which has no generic clone strategy
        // per [`pinion_core::scene::ExternalNode`] doc), so the
        // textbook path is to re-run the paint cycle from the
        // post-dispatch hook with `&mut self` access restored.
        let produce_size: Cell<Option<(u32, u32)>> = Cell::new(None);
        let resp = {
            // Disjoint-field split mutable borrows. R51.123 §5.41 —
            // `scene` + `cached_state` live behind
            // `self.core: CoreShell<V>`; the producer closure reads
            // `cached_state` (`Copy`) and the dispatcher takes the
            // scene mut via `core.scene_mut()`. `previews` / `revision`
            // / `focus` / `text_cache` stay on the Vello extras.
            //
            // R51.146 §5.22 — clone the root [`Owner`] handle (cheap
            // `Rc` clone) before `scene_mut` so the producer closure
            // wraps `V::view` in `root_owner.run(|| ...)`. The clone
            // aliases the same reactive scope through the underlying
            // `Rc<OwnerInner>`, so animations / effects registered
            // through `Owner::current()` inside the synthetic-paint
            // path still land on the binding's owner.
            let cached_state = *self.core.cached_state();
            let root_owner = self.core.root_owner().clone();
            // R705 §5.39 §5.40 — resolve the focus-ring target the same
            // way the shared overlay chain does for the winit paint path:
            // through `access_focus_target` so a roving widget's ring
            // tracks its aria-activedescendant cell (`datepicker#15`)
            // rather than wrapping the container. Computed once here
            // (focus is sampled pre-dispatch) and captured by the
            // produce closure, keeping `scene/snapshot from: paint`
            // byte-equal to the live winit frame (producer parity,
            // [[r670b-paint-scene-producer-parity]]).
            let ring_tag_for_paint: Option<String> = focus_before
                .as_deref()
                .map(|f| resolve_focus_ring_tag::<V>(&cached_state, f, &root_owner));
            // R51.162 §5.23 — grab the executor Arc-clone before
            // `scene_mut` reborrows `self.core` mutably. `Arc::clone`
            // is cheap (one atomic bump) and unblocks the borrow
            // split so the dispatcher can hand `&CommandExecutor`
            // into the with_commands_executor builder below.
            let executor_for_rpc: Option<Arc<CommandExecutor>> = self.core.executor().cloned();
            // R705 §5.12 §2 #7 — split borrow: the dispatcher mutates
            // the authoritative state scene while `scene/snapshot
            // from: paint` reads the addressed window's stored paint
            // scene (the displayed frame). The two live in disjoint
            // `CoreShell` fields (`scene` vs `routers`), so a single
            // method hands out `&mut Scene` + `Option<&Scene>` without
            // aliasing — replacing the pre-R705 query-time re-render
            // that drifted from the on-screen pixels
            // ([[introspection-from-paint-not-screen]]).
            let paint_window_key = window_id.unwrap_or(pinion_runtime::DEFAULT_WINDOW);
            // R1467 §5.16 §5.39 §2 #7 — sample the WHOLE window-overlay chain
            // (resize border, chrome strip + its content inset, raised top edge,
            // focus ring, cross-window drop preview, drag-image chip) BEFORE the
            // `scene_mut` reborrow, so the produce closure can dress its scene
            // with the SAME [`WindowOverlayInputs::apply`] the two `&self`
            // producers run ([[r670b-paint-scene-producer-parity]]).
            //
            // Pre-R1467 only the last two steps crossed the borrow split, each
            // hoisted by hand as its own local. That is the whole defect: a
            // chromed window's produce scene carried no strip and, worse, no
            // content inset, so every coordinate this path derived for it sat
            // `WindowChromeStyle::height_px` too high — and `scene/click {path}`
            // resolves through exactly this producer.
            //
            // `ring_tag` is then overridden with the pre-dispatch sample: a
            // focus-mutating method in THIS dispatch has not flushed when the
            // closure runs for path resolution, so the ring must reflect ENTRY
            // focus — which is what the AI client addressed.
            let overlays_for_paint = WindowOverlayInputs {
                ring_tag: ring_tag_for_paint,
                ..self.sample_window_overlay_inputs(window_id)
            };
            // R1468 §5.23 §5.22 §2 #3 §2 #7 — the third producer is a mirror
            // too, so it enters the same containment scope. Hoisted here for
            // the R1467 reason: the closure runs after `&mut self` is split, so
            // it cannot reach a `&self` accessor and must own what it needs.
            // Cloned rather than borrowed because the split hands the closure
            // no path back to `self.core` at all; `Signal` is a handle, so the
            // clone is a refcount bump onto the SAME seam the live paint
            // publishes — a copy of the value would defeat the point.
            let mirror_viewport = self
                .core
                .introspection_viewport_signal(paint_window_key)
                .cloned();
            let (scene_ptr, last_paint_scene_ref) = self
                .core
                .scene_mut_and_last_paint_for_window(paint_window_key);
            let previews = &self.previews;
            let revision = self.revision.as_ref();
            let focus_ptr = &mut self.focus;
            let text_cache_ptr = &mut self.text_cache;
            // R1546 §5.36 §5.12 — the painted background bands, collected HERE
            // because this is the one place holding both halves of the answer:
            // `last_paint_scene_ref` (what was painted) and `text_cache_ptr`
            // (what it was shaped through), as disjoint borrows. Every band is
            // a cache hit on an entry this frame's paint already derived, so
            // the wire cannot disagree with the pixels.
            //
            // Gated on the method — the `declared_windows` pattern — so every
            // other dispatch pays one string comparison and no walk. Taken
            // before `produce` captures `text_cache_ptr`.
            //
            // A shell that has not painted installs an EMPTY list, not
            // `None`: "nothing is highlighted yet" is a true answer, while
            // `None` means "this host cannot shape at all" and is for an
            // embedder that holds no cache.
            let text_backgrounds = (request.method == "scene/text_backgrounds").then(|| {
                last_paint_scene_ref.map_or_else(Vec::new, |paint| {
                    pinion_rpc::text_backgrounds::collect_bands(paint, text_cache_ptr)
                })
            });
            // R1551 §5.36 §5.12 — the same shape for `scene/text_blocks`: one
            // string comparison for every other method, and an EMPTY list (not
            // `None`) when this shell has not painted, because "no paragraphs
            // yet" is a true answer while `None` means "cannot shape at all".
            let text_blocks = (request.method == "scene/text_blocks").then(|| {
                last_paint_scene_ref.map_or_else(Vec::new, |paint| {
                    pinion_rpc::text_blocks::collect_blocks(paint, text_cache_ptr)
                })
            });
            // R1654 §5.36 §2 #7 — the same shape again: what this frame painted
            // where the policy shortened it, so an elided label is legible to
            // an agent as the thing the reader actually sees.
            let text_painted = (request.method == "scene/text_painted").then(|| {
                last_paint_scene_ref.map_or_else(Vec::new, |paint| {
                    pinion_rpc::text_painted::collect_painted(paint, text_cache_ptr)
                })
            });
            // R1656 §5.32 §5.36 §2 #7 — every mark this frame painted outside
            // the box that owns it. Collected here rather than in the paint
            // pass for the same reason its siblings above are: the answer needs
            // BOTH the painted scene and the shape cache, and only the embedder
            // holds the second.
            let containment = (request.method == "scene/containment").then(|| {
                last_paint_scene_ref.map_or_else(
                    || pinion_rpc::containment::report(&[], 0),
                    |paint| pinion_rpc::containment::collect(paint, text_cache_ptr),
                )
            });
            // R1662 §5.32 §5.36 §2 #7 — every mark this frame did NOT show, and
            // whether any scroll offset reaches it. Collected here for the same
            // reason as its sibling above: the answer needs both the painted
            // scene and the shape cache, and a text run's reachability is
            // decided by its shaped ink rather than by its promised box.
            let scroll_reach = (request.method == "scene/scroll_reach").then(|| {
                last_paint_scene_ref.map_or_else(
                    || pinion_rpc::scroll_reach::report((0, 0), &[], 0),
                    |paint| pinion_rpc::scroll_reach::collect(paint, text_cache_ptr),
                )
            });
            let produce_work_ptr = &mut self.produce_work;
            // R1072 §5.37 — the opt-in engine measure for the RPC-side producer,
            // so a `scene/snapshot from: paint` (and the post-dispatch
            // InputRouter finalize) sees the same §5.37 boxes the winit paint
            // path does. Disjoint field borrow alongside `text_cache_ptr`.
            let text_engine_ptr = &self.text_engine;
            // R670.B §5.16 — per-window view fn dispatch. Single-
            // window paths (`window_id == None`) keep using `V::view`
            // exactly as before for bit-identical legacy behaviour;
            // multi-window paths (`Some(spec_id)`) route through
            // `V::view_for_window(spec_id, state, frame)` so the
            // dispatched window's paint scene reflects the right
            // view branch.
            let producer_window_id = window_id.map(str::to_owned);
            let mut produce = |w: u32, h: u32| -> Scene {
                // R684 atomic 3 — record the viewport for the
                // post-dispatch headless-RPC finalize. Tracking the
                // last (w, h) the closure ran with lets the post-
                // dispatch hook re-run `compute_paint_scene_for_window`
                // at the same viewport so the InputRouter snapshot
                // matches the geometry the RPC handler just saw.
                produce_size.set(Some((w, h)));
                // R1468 — the containment scope, entered per CALL because the
                // extent is the caller's: `resolve_path_to_center` runs this at
                // the live viewport while `scene/layout {viewport}` runs it at a
                // hypothetical one, and it is the hypothetical case that has to
                // publish. Restored before the closure returns, so two calls in
                // one dispatch cannot leak one another's extent.
                let _mirror = pinion_runtime::IntrospectionPaint::enter(
                    &root_owner,
                    mirror_viewport.as_ref(),
                    (w, h),
                );
                let frame = Frame::new();
                // R57.X.scrollbar §5.45 / R1458 §5.45 §5.27 / R1465 — the shared
                // fixed point, for the same reason the window paint runs it
                // (producer parity: an agent reading `scene/snapshot from: paint`
                // must see the scene the window shows, not an earlier pass of
                // it). No pane publish inside the layout closure — the RPC
                // producer is the introspection mirror and never publishes (the
                // R1006 contract). Idempotent on steady-state: Signal
                // equality-skip floors the dirty bit at false on pass 1.
                let text_measure = text_measure_override(text_engine_ptr.as_ref());
                // R1460 §5.16 §2 #2 — the producer records no FRAME (it must
                // not manufacture one the user never saw), but its work is
                // still work, and an agent driving by RPC has no other way to
                // see what its own call cost. Counted cumulatively here and
                // published on `scene/frame_timings`.
                let shapes_before = text_cache_ptr.shapes();
                // R1538 §5.16 — and its node census, the size of the scene the
                // agent's own call settled.
                let mut produce_nodes = 0_u32;
                // R1121 §5.16 §5.21 / R1467 — inset the content below the chrome
                // strip INSIDE the settle loop, exactly where the two `&self`
                // producers do it: the inset changes the height the view lays out
                // into, so a window whose view reads its own measured extent must
                // settle against the inset geometry, not be shifted afterwards.
                let chrome_h = overlays_for_paint.chrome_inset();
                let settle = settle_to_fixed_point(
                    || {
                        window_view::<V>(
                            &root_owner,
                            producer_window_id.as_deref(),
                            cached_state,
                            frame,
                            chrome_h,
                        )
                    },
                    |scene| {
                        let pass = compute_layout_with_text_measure(
                            scene,
                            text_cache_ptr,
                            w,
                            h,
                            text_measure,
                        );
                        produce_nodes = produce_nodes.saturating_add(pass.nodes);
                        pass.scroll_dirty
                    },
                );
                let paint = settle.scene;
                produce_work_ptr.record(
                    settle.passes,
                    text_cache_ptr.shapes() - shapes_before,
                    produce_nodes,
                );
                // R1467 §5.16 §5.39 §2 #7 — the ONE overlay chain, the same call
                // the winit paint and the introspection mirror make. Pre-R1467
                // this tail hand-rolled two of its six steps and omitted four
                // (resize border, chrome strip, raised top edge, cross-window drop
                // preview), so `scene/snapshot from: paint` on a never-painted
                // chromed window reported a window that does not exist and
                // `scene/click {path}` aimed at coordinates the window never had.
                // R1022 §5.39 — `(w, h)` is the viewport this closure laid out to,
                // so every overlay rect matches the geometry it dresses.
                overlays_for_paint.apply::<V>(paint, w, h, &root_owner)
            };
            // R979 §5.40 §2 #7 — `scene/access` producer (the `build_access_tree`
            // SSOT the live AccessKit emit also runs; entry-focus `focus_before`).
            let access_focused = focus_before.clone();
            let mut produce_access = || {
                build_access_tree::<V>(
                    &root_owner,
                    &cached_state,
                    producer_window_id.as_deref(),
                    access_focused.as_deref(),
                    last_paint_scene_ref,
                )
            };
            let mut ctx = DispatchContext::new(scene_ptr, previews, revision)
                .with_paint_producer(&mut produce)
                .with_access_producer(&mut produce_access)
                .with_resize_request(resize_request)
                .with_focus_manager(focus_ptr);
            // R705 §5.12 §2 #7 — thread the addressed window's stored
            // paint scene so `scene/snapshot from: paint` serializes the
            // displayed frame rather than re-rendering. R890.1: the
            // dispatcher also projects `scene/layout {viewport: null}`
            // and the path→coordinate hit-test dims from this SAME
            // borrow (one channel — layout and pixel introspection
            // cannot disagree about which frame they describe; the
            // never-painted `None` surfaces `NoLastPaintLayout`).
            if let Some(paint) = last_paint_scene_ref {
                ctx = ctx.with_last_paint_scene(paint);
            }
            // R1710 §5.16 §5.12 — the declared bounds `scene/resize` resolves
            // an ask against. Absent for every other method, which is why the
            // dispatcher's default is `UNBOUNDED` rather than a guess.
            if let Some(bounds) = resize_bounds {
                ctx = ctx.with_window_bounds(bounds);
            }
            // R1546 §5.36 §5.12 — the bands collected above, for
            // `scene/text_backgrounds` only.
            if let Some(bands) = text_backgrounds {
                ctx = ctx.with_text_backgrounds(bands);
            }
            // R1551 §5.36 §5.12 — the paragraphs collected above, for
            // `scene/text_blocks` only.
            if let Some(runs) = text_painted {
                ctx = ctx.with_text_painted(runs);
            }
            // R1656 §5.32 — the escape report collected above, for
            // `scene/containment` only.
            if let Some(report) = containment {
                ctx = ctx.with_containment(report);
            }
            // R1662 §5.32 — the out-of-sight report collected above, for
            // `scene/scroll_reach` only. R1711 — unless the caller asked about
            // a size this window is not at, in which case the mirror's report
            // for THAT size is the answer and the live one would be a different
            // question quietly substituted for the one asked.
            if let Some(report) = hypothetical_reach.or(scroll_reach) {
                ctx = ctx.with_scroll_reach(report);
            }
            // R1711 §5.16 §5.32 — the measured floor, for `scene/size_floor`.
            if let Some(report) = size_floor {
                ctx = ctx.with_size_floor(report);
            }
            if let Some(blocks) = text_blocks {
                ctx = ctx.with_text_blocks(blocks);
            }
            // R670.B §5.16 — surface the resolved window id so future
            // RPC consumers can read it through `DispatchContext`.
            if let Some(id) = window_id {
                ctx = ctx.with_window(id);
            }
            // R51.161 §5.23 — surface the root [`Owner`] handle so
            // RPC methods that read reactive substrate state can do
            // so without draining (Owner is reachable via the cloned
            // handle above; lifetime ties through the borrow split).
            // First consumer was `scene/commands`; R598 §5.50 added
            // `scene/theme_tokens` reading the cached ThemeProvider.
            ctx = ctx.with_runtime_owner(&root_owner);
            // R51.162 §5.23 — also surface the CommandExecutor for
            // in-flight introspection. The `executor_for_rpc` Arc
            // clone was taken above (before the `scene_mut` reborrow);
            // hand `&CommandExecutor` (no Arc-ness leak into
            // pinion-rpc) to the builder.
            if let Some(exec_arc) = executor_for_rpc.as_ref() {
                ctx = ctx.with_commands_executor(exec_arc.as_ref());
            }
            // R51.195 §5.49 §5.45 — wire the deferred-input inbox so
            // `scene/wheel` can enqueue events. The inbox lives on
            // the stack here; we drain it after `dispatch` returns
            // (below) so each enqueued `cursor_moved` + `wheel` lands
            // outside the dispatcher's `&mut scene` borrow.
            let mut deferred_inputs: Vec<pinion_rpc::DeferredInput> = Vec::new();
            ctx = ctx.with_deferred_inputs(&mut deferred_inputs);
            // R682.B / R885 / R888 §5.16 §5.49 — install the
            // pre-resolved window-scoped reads (cache stats / input
            // state / pacing; each absent leg surfaces its
            // `*Unavailable` token).
            if let Some(stats) = window_reads.cache_stats {
                ctx = ctx.with_fragment_cache_stats(stats);
            }
            // R1521 §5.36 — unconditional: a shell always has a shape cache,
            // so `scene/text_cache_stats` always has an answer here. The
            // `Unavailable` token is for an embedder that holds no cache at
            // all (a `pinion-tui` host, which never shapes), not for a shell
            // that has not shaped yet — that one honestly reports zeros.
            ctx = ctx.with_text_cache_stats(text_cache_stats);
            // R1550 §5.7 — present only on a `scene/memory` dispatch; every
            // other method leaves the slot `None` and pays no walk.
            if let Some(census) = memory_census {
                ctx = ctx.with_memory_census(census);
            }
            // R1557 §5.7 — likewise present only on a `scene/draw_profile`
            // dispatch, and only from the windowed entry: the attribution is a
            // re-encode, which needs the window's image cache.
            if let Some(profile) = draw_profile {
                ctx = ctx.with_draw_profile(profile);
            }
            // R1552 §5.7 PINION-PR83 — the connection this frame arrived on.
            // `None` for the single-window entry and any synthetic dispatch, so
            // `scene/subscribe` there answers `SubscriptionsUnavailable` rather
            // than opening a stream nothing could deliver.
            ctx = ctx.with_frame_origin(origin);
            if let Some(timings) = window_reads.frame_timings {
                ctx = ctx.with_frame_timings(timings);
            }
            if let Some(fidelity) = window_reads.render_fidelity {
                ctx = ctx.with_render_fidelity(fidelity);
            }
            if let Some(snapshot) = window_reads.input_state {
                ctx = ctx.with_input_state(snapshot);
            }
            if let Some(pacing) = window_reads.pacing_state {
                ctx = ctx.with_pacing_state(pacing);
            }
            // R1549 §5.35 §5.38 — unconditional: an empty census is the
            // honest answer for a window with nothing held, so there is no
            // `Unavailable` leg to gate on.
            ctx = ctx.with_auto_repeat_holds(window_reads.auto_repeat_holds);
            // R1569 §5.39 §5.20 — the window's live accelerator map, plus the
            // resolver `scene/accelerators`' optional `chord` parameter needs.
            // The chord is a REQUEST parameter, so it cannot be pre-resolved
            // here; the closure is the `declare_request` shape.
            ctx = ctx.with_accelerators(window_reads.accelerators, window_reads.accelerator_focus);
            ctx = ctx.with_accelerator_chord(accelerator_chord.clone());
            // R1087 §5.16 §5.41 §2 #7 PR-31 — the declared-window set
            // (resolved above, only for `scene/windows`).
            if let Some(windows) = declared_windows {
                ctx = ctx.with_declared_windows(windows);
            }
            // R1576 §5.16 §5.41 §2 #7 — the desk `scene/displays` publishes and
            // `scene/windows` resolves each declared placement against.
            if let Some(topology) = display_topology {
                ctx = ctx.with_display_topology(topology);
            }
            if let Some(drop) = cross_window_drop {
                ctx = ctx.with_cross_window_drop(drop);
            }
            // R1088 §5.16 §5.41 §2 #7 PR-31 / R1610 — the `scene/window_move`
            // + `scene/window_declare` write peer. Its closure resolved a
            // signal above only for those methods (a harmless no-op closure
            // otherwise), so this attaches unconditionally without
            // per-dispatch cost.
            ctx = ctx.with_declare_request(&mut declare_request);
            // R1419 §5.39 §5.16 — the `scene/window_focus` OS-focus drive peer.
            // Like `declare_request`, the closure captures no `self` borrow
            // (it records the edge into a `Cell` + reads a pre-block snapshot),
            // so it attaches unconditionally without a per-dispatch cost; the
            // real mutation lands after the block.
            ctx = ctx.with_window_focus_request(&mut window_focus_request);
            // R1060 §5.12 §5.16 — the AppShell windowed entry pre-captured
            // the addressed window's live presented surface (only when the
            // method is `scene/screenshot`); hand it to the dispatcher so
            // `handle_scene_screenshot` returns real pixels instead of the
            // `RenderBackendUnavailable` stub.
            if let Some(shot) = screenshot {
                ctx = ctx.with_screenshot(shot);
            }
            // R889 §5.49 — thread the unknown-window verdict so the
            // dispatcher rejects the whole request (`-32602
            // unknown_window`) before method routing. Replaces the
            // pre-R889 AppShell-side silent alias of unknown ids onto
            // the primary spec (`resolve_spec_id`, deleted) — one
            // judgment site, shared by every READ + WRITE method.
            if let Some(supplied) = window_reads.unknown_window {
                ctx = ctx.with_unknown_window(supplied);
            }
            let resp = dispatch_parsed(&mut ctx, request);
            (resp, deferred_inputs)
        };
        let (resp, deferred_inputs) = resp;
        // R1419 §5.39 §5.16 / R1420 — apply the `scene/window_focus` edge the closure recorded,
        // now that `&mut self` is restored. This replays the shell's OWN winit `WindowEvent::Focused` arm
        // BYTE-FOR-BYTE (`AppShell::window_event`): `note_os_focus` (gate + the R1419 paint-path mirror through
        // the one `set_os_focused_window` funnel), then `window_focused` / `window_blurred` — so the drive is a FULL
        // OS-focus-edge simulation, not a gate-only stub: a blur snapshots the
        // focused widget for restore AND clears the held-key chord cache (the
        // browser missed-keyup convention, so a chord held across an alt-tab
        // cannot strand), and a refocus restores the saved widget. R1420
        // removed the earlier deferral of this half — the toolkit's window
        // deactivation likewise remembers focus and settles held state, so
        // parity demands it. The edge stays `None` unless the closure actually
        // ran (method matched, params valid, window known), so every other
        // dispatch — and a rejected `scene/window_focus` — applies nothing.
        if is_window_focus {
            if let Some(focused) = window_focus_edge.get() {
                self.note_os_focus(&os_focus_target, focused);
                if focused {
                    self.window_focused();
                } else {
                    self.window_blurred();
                }
                self.request_redraw();
            }
        }
        // R684 atomic 3 §5.16 §5.41 §5.49 — headless-RPC floating-
        // window paint cycle. When the dispatch is scoped to a
        // specific window AND the produce closure ran (i.e. an RPC
        // handler resolved a path or otherwise asked for the paint
        // scene), re-run the paint pipeline for the addressed
        // window + finalize the addressed window's
        // [`pinion_runtime::InputRouter::last_paint_scene`].
        //
        // This makes [`pinion_runtime::InputRouter`] hit-testing
        // work for floating windows that have never been driven by a
        // winit `RedrawRequested` cycle (the R683 dock tear-off
        // path under headless RPC produces such windows). Pre-R684
        // every such window's router stayed empty until winit's
        // paint loop caught up, so `scene/drag {window: "torn-..."}`
        // silently no-op'd because the deferred-input drain (below)
        // had nothing to hit-test against.
        //
        // Skipped when `window_id` is `None` (single-window
        // `Self::dispatch_rpc` path) so the legacy single-window
        // behaviour stays bit-identical — single-window bindings
        // already drive finalize through their AppShell's winit
        // paint loop, and the AppShell-side `dispatch_rpc` ALWAYS
        // threads `Some(spec_id)` here regardless of how many
        // windows the binding declared (every multi-window AppShell
        // RPC frame carries an explicit `{window: "<id>"}` param).
        //
        // Skipped when `produce_size` stayed `None` (no RPC handler
        // asked for the paint scene, e.g. `focus/get` /
        // `scene/intents` etc. — these are pure substrate reads,
        // running a full paint cycle just to populate the router
        // would be pure overhead).
        //
        // The double-paint cost (produce closure ran once for
        // path-resolution, this re-run feeds finalize) is acceptable
        // for headless-RPC: dt between the two calls is microseconds
        // so animation tick advances by ~0 (`Instant::now() -
        // Instant::now()` ≈ 0 → spring solver no-op), layout cache
        // LRU touches the same entries (no churn), and the second
        // paint observably equals the first.
        // R684 atomic 3 finalize: write the post-paint scene into the
        // addressed window's InputRouter so the downstream
        // `drain_deferred_inputs_for_window` hit-tests against real
        // geometry (and, since R890, so `scene/layout` projections
        // see this window's own frame).
        //
        // R1465 doc honesty — this said "**first-paint only**, so already-active
        // windows do NOT pay any per-RPC side-effect cost". R684 was written
        // that way; R685 (`76d5b8bf`) deliberately dropped the
        // `has_last_paint_scene_for_window` gate when it swapped the full
        // recompute for the pure variant + the storage-only publish, and the
        // comment kept the old claim. What survives is the SIDE-EFFECT half
        // (below): the storage-only publish synthesizes no hover arcs, which is
        // why running it every dispatch is safe. The COST half was false, and
        // is now measured rather than asserted — every producing dispatch
        // renders one mirror here, and a mutating one renders another per
        // painted window in the R705 re-store below, both counted into
        // `mirror_work` and readable as `scene/frame_timings.mirror`.
        //
        // ## R1466 — the render is load-bearing, and both cheaper routes are out
        //
        // R1465 measured the cost and left "can this be skipped?" open. It
        // cannot, and neither can be answered by argument:
        //
        // * **Skipping it when the window has already painted** (R684's gate,
        //   restored as an experiment) leaves the whole `pinion-shell` suite
        //   green, and every demo passing — a coverage hole that stood from
        //   R685 to R1466. `r1466_a_click_hit_tests_against_this_dispatchs_
        //   geometry_not_the_last_paints` is that hole closed: geometry that
        //   moved with no dispatch and no paint behind it (an async result, an
        //   animation, a background `Signal` write) leaves the store aiming a
        //   fresh point at an old scene, and the click lands on nothing. The
        //   demos stay green there because each drives its next step through
        //   the repaint that repairs the store — which is exactly why the
        //   deterministic unit test is the evidence and the demo suite is not.
        // * **Reusing the scene the produce closure just built** — same state,
        //   same viewport, already settled — is not the same scene, but NOT for
        //   the reason R1467 found this comment giving. R1466 wrote that the
        //   producer applies only the ring + drag image while a stored scene
        //   also gets the resize border, chrome strip, raised top edge and
        //   drop-zone preview, so reuse would strip the CHROME tag R1188's
        //   window-control interception hit-tests for. That divergence was real
        //   and is now GONE: R1467 put all three producers on the one
        //   [`WindowOverlayInputs::apply`] chain, precisely because a produce
        //   path that dressed windows differently from the paint was a §2 #7
        //   break in its own right (it also mis-placed every path-resolved
        //   coordinate on a chromed window).
        //
        //   What survives the correction is the TIMELINE, which is the stronger
        //   reason: the produce closure runs BEFORE the handler mutates state
        //   (it resolves paths, and samples focus pre-dispatch), while this
        //   re-render runs after. Same chain, different frame. The unit test
        //   named above pins that, and it is what keeps the render load-bearing
        //   now that the overlay argument no longer does.
        //
        // Why the storage-only publish:
        //
        // `update_paint_scene_for_window` calls
        // [`pinion_runtime::InputRouter::refresh_hover`] for every
        // active cursor as a side effect (a window resize or scene
        // re-layout can move widgets under a stationary cursor, and
        // `refresh_hover` synthesizes the resulting `PointerEnter` /
        // `PointerLeave` arcs so the SCXML state matches the new
        // geometry). For already-painted windows this side effect is
        // already driven by winit's `RedrawRequested` cycle on every
        // visible frame; re-driving it on every RPC dispatch would
        // double-fire hover transitions and mutate widget state in
        // ways the application did not request (the bisect signal
        // that pinpointed this: `tools/demos/todomvc_r660.py` End-
        // key assertion regressed because the post-finalize
        // refresh_hover dispatched a PointerEnter that the RadioGroup
        // statechart interpreted as a focus shift).
        //
        // We deliberately bypass [`Self::finalize_frame_for_window`]
        // because it ends with `tail()` + `handle_tail()` — the
        // intent drain belongs to the END of the dispatch cycle and
        // is performed below by the existing R51.123 §5.41
        // post-dispatch `handle_tail` call. Routing through
        // `finalize_frame_for_window` here would double-drain
        // pending intents queued by the produce-closure's paint walk.
        if let (Some(id), Some((w, h))) = (window_id, produce_size.get()) {
            // (R684.B atomic 2 §5.16) Use the pure variant — V::view
            // + compute_layout only, no animation tick / IM tick /
            // scroll-dirty re-run / animation-active redraw flag
            // mutation. The producer closure already ran the full
            // pipeline once during dispatch (for path-resolution); a
            // pure recompute produces the same geometry without the
            // double-fire side effects. Pre-R684.B the finalize hook
            // called the full `compute_paint_scene_for_window`,
            // re-firing every side effect.
            let paint = self.compute_paint_scene_pure_for_window(id, w, h);
            // (R685.C atomic 4 §5.16 §5.41 §5.35) Storage-only paint
            // publish via the named primitive. The RPC didn't move
            // the cursor (only the layout shifted beneath it), so the
            // storage-only variant — no `refresh_hover` synthetic
            // arcs (those would mutate widget state the RPC didn't
            // request — the R660 RadioGroup regression origin). The
            // hover-refreshing twin
            // (`apply_paint_for_window_with_hover_refresh`) is the
            // winit paint loop's primitive. Pre-R685.C the dispatch
            // hook inlined these writes; R685.C lifts them into
            // the named storage-only primitive for declarative reads.
            self.apply_paint_for_window_storage_only(id, paint);
        }
        // R51.195 §5.49 §5.45 — drain the deferred-input inbox.
        // `&mut scene` is released here, so calling back into
        // `ShellCore` is legal again.
        //
        // R672 §5.35 §5.41 §5.49 — when the dispatch is scoped to a
        // specific window (`dispatch_rpc_for_window` callers), the
        // drain routes through the addressed window's
        // [`pinion_runtime::InputRouter`] so `scene/click` hit-tests
        // against the named window's last paint scene. The single-
        // window path passes `None` + falls back to the
        // [`pinion_runtime::DEFAULT_WINDOW`] router exactly like
        // pre-R672 callers. (R1188 — the drain now receives the
        // ORIGINAL scope and derives the router-key fallback itself,
        // so a window-control hit resolves the canonical spec id.)
        self.drain_deferred_inputs_for_window(window_id, &deferred_inputs);
        let tail = self.core.tail();
        self.handle_tail(&tail);
        // R688 §5.16 §5.35 — reconcile the external set after the
        // dispatch. A `scene/invoke` that mutated the binding's
        // structure (e.g. a dock reorganize minting a `reorg-split-N`)
        // set its reactive topology Signal during this dispatch; the
        // freshly created surface needs a routable External before the
        // next RPC addresses it. No-op when the tag set is unchanged.
        self.core.reconcile_externals();
        // R51.73 §5.40 — `focus/set` from the AI client must trigger
        // a redraw so the focus ring repaints on the new target. The
        // before/after comparison catches every focus-mutating
        // method without enumerating method names.
        //
        // R56.1.h §5.38 §5.39 — same boundary fires the
        // [`External::on_focus_change`] notification on the old +
        // new tags so RPC-driven `focus/set` reaches the TextField
        // statechart Focus / Blur drive on a single dispatch tick
        // (mirrors the click_to_focus and Tab paths).
        if self.focus.focused().map(str::to_owned) != focus_before {
            self.notify_focus_change(focus_before.as_deref());
            self.request_redraw();
        }
        // R705 §5.12 §2 #7 — dirty-on-mutation paint-scene re-store.
        //
        // When this windowed dispatch changed visible state (the
        // `redraw_requested` flag was raised by `handle_tail`'s
        // `state_change` arm, the deferred-input drain, or the focus
        // shift above), re-render the canonical paint scene and re-store
        // it into the addressed window's `InputRouter`. This makes a
        // follow-up `scene/snapshot from: paint` reflect the just-
        // committed state *immediately*, without waiting for the winit
        // paint loop to catch up — closing the lag half of the §2 #7
        // restoration (the readback half lives in the snapshot handler).
        //
        // Every painted window ALSO repaints live (the same
        // `request_redraw` fan-out), producing a byte-identical scene
        // because both paths run the one `compute_paint_scene_pure_*`
        // pipeline at the same `cached_state` + viewport — so the
        // transient "stored ahead of GPU" skew resolves to identical
        // content. There is no second divergent renderer any more; that
        // elimination is what restores introspection == screen
        // ([[r670b-paint-scene-producer-parity]]).
        //
        // ## Why every window, not just the dispatched one
        //
        // A multi-window binding (DevTools inspector, dock editor) shares
        // one reactive state scene across windows: an `scene/invoke` on
        // the inspector window mutates a `Signal` the MAIN window's view
        // reads (the "Selected: …" banner). Re-storing only the
        // dispatched window would leave the other windows' stored paint
        // scenes stale, so their `from: paint` would lag the committed
        // state until their own winit repaint caught up — the exact
        // cross-window drift R705 set out to abolish. Iterating
        // `painted_window_sizes` re-renders each window's view at its own
        // viewport so all of them reflect the mutation atomically.
        //
        // Gated on `redraw_requested` so pure reads (`focus/get`,
        // `scene/query`, `scene/snapshot` itself) — which never raise it
        // — pay nothing. Uses the storage-only publish primitive (no
        // `refresh_hover`: the RPC did not move the cursor, only the
        // layout shifted beneath it — the R660 `RadioGroup` regression
        // origin). A never-painted window has no router entry yet and is
        // left to its first winit paint (or the R684 first-paint finalize
        // above for headless floating windows).
        //
        // `redraw_requested` is now raised for reactive-`Signal`-only
        // changes too: a handled `apply_key` ([`Self::try_apply_key`]) and
        // any drained intent ([`Self::handle_tail`]) both arm it, so a
        // listbox/tree focused-row Signal or a shared cross-window
        // selection Signal forces the repaint + re-store even with no
        // SCXML `state_change`. A benign no-op input (a click that hits
        // nothing, a pure read) arms nothing and pays zero — which keeps
        // the R682 fragment-cache warmup stable (no spurious paints).
        if self.redraw_requested {
            for (id, w, h) in self.core.painted_window_sizes() {
                let paint = self.compute_paint_scene_pure_for_window(&id, w.max(1), h.max(1));
                self.apply_paint_for_window_storage_only(&id, paint);
            }
        }
        resp
    }

    /// R51.123 §5.41 — Vello-side post-dispatch bookkeeping for a
    /// [`DispatchTail`] returned by any [`CoreShell`] dispatch
    /// method.
    ///
    /// Logs each drained §5.20 intent to stderr (the dogfood
    /// trace line shape [`crate::AppShell`] and downstream observers
    /// rely on); logs the cached-state transition (`from -> to`) and
    /// sets `redraw_requested` so the next event-loop iteration
    /// triggers a repaint when the visible state changed; both are
    /// no-ops when the tail is empty (no intents, no state change).
    /// The `scene/intents` RPC method races with this drain —
    /// whichever caller harvests first wins (poll-form,
    /// single-consumer v0 per §5.20).
    ///
    /// Takes [`DispatchTail`] by reference because the
    /// substrate-side reads (`tail.intents` iter, `tail.state_change`
    /// copy) consume nothing; the caller owns the `Vec<Intent>` and
    /// drops it on return.
    ///
    /// R51.159 §5.23 — after the existing intent + state-change
    /// bookkeeping, drain any [`pinion_core::Command`] the just-run
    /// SCXML transition queued through
    /// [`CoreShell::dispatch_pending_commands`]. With no executor
    /// installed (the default for headless tests and pre-R51.159
    /// binaries) the drain is a no-op; with an executor installed
    /// (the `run_with_handlers` entry point) every command kind with
    /// a registered handler dispatches asynchronously and the
    /// resolved [`Intent`] arrives back as
    /// [`AppEvent::IntentArrived`](crate::AppEvent::IntentArrived).
    /// Unhandled kinds are logged so a missing handler is observable.
    fn handle_tail(&mut self, tail: &DispatchTail<V::State>) {
        for intent in &tail.intents {
            // (R1166) KEEP as `eprintln!` — the `shell: initial state` / `intent` /
            // `state ->` / `final state` lines are a DELIBERATE, STABLE dogfood
            // stderr trace, NOT ad-hoc diagnostics: ~8 demos assert on
            // `shell: intent <tag> payload=...` (r691/r692/r715/r772/r805/r887/r986/
            // r988). R1166 migrated the ad-hoc shell diagnostics (window/renderer/
            // vello/etc.) to `tracing::*` but left this trace family on stderr so the
            // demo contract holds. A future move to `tracing` must also migrate those
            // demos' stderr-scrape (an RPC-surface change), not a logging tweak.
            eprintln!(
                "shell: intent {} payload={:?}",
                intent.tag_str(),
                intent.payload,
            );
            // R51.169 §5.23 R27 — every drained intent flows through
            // `V::update` so reducer-produced `Vec<Command>` from
            // widget-side state transitions (e.g. button.click,
            // toggle.changed) joins the same owner queue the
            // async-re-feed path uses. Closes the R27 dispatch loop's
            // input → drain → reducer arc.
            let _ = self.core.route_intent_through_update(intent);
        }
        // R705.1 §2 #7 — reactive-`Signal` dirty bridge (Solid.js style).
        //
        // `V::view` runs under `root_owner.run(...)`, so every
        // `Signal::get()` it reads auto-subscribes `root_owner`. When a
        // reducer / `External::invoke` later `Signal::set`s one of those —
        // a listbox/tree focused-row Signal, a shared cross-window
        // selection Signal — `root_owner.is_dirty()` flips true WITHOUT any
        // SCXML `state_change`. Arming the redraw off that single reactive
        // primitive is the textbook replacement for sprinkling manual
        // `request_redraw` across every input handler: it catches EVERY
        // signal-mutating path (keys, intents, `Effect`s) AND stays silent
        // on a benign no-op (a click that hit nothing dirties nothing —
        // the R682 fragment-cache warmup that a coarse OCC-revision gate
        // churned). The flag is reset by `clear_dirty()` after each paint
        // ([`compute_paint_scene_internal`] / `_pure_internal`).
        if self.core.root_owner().is_dirty() {
            self.request_redraw();
        }
        if let Some(sc) = tail.state_change {
            eprintln!(
                "shell: state {} -> {}",
                V::fmt_state_log(&sc.before),
                V::fmt_state_log(&sc.after),
            );
            self.request_redraw();
        }
        // R51.159 §5.23 — drain Commands the dispatch arm queued.
        for cmd in self.core.dispatch_pending_commands() {
            eprintln!(
                "shell: command unhandled kind={} payload={:?}",
                cmd.kind_str(),
                cmd.payload,
            );
        }
        // R664 R693 R1462 §5.39 — resolve the focus this dispatch leaves
        // behind: drain BOTH focus mailboxes at one point, apply the
        // modal batch (automatic policy) before the focus request
        // (explicit intent), and commit a single observable transition.
        // The single-frame guarantee both mailboxes advertise is this
        // call site: the trap installs (or lifts) and the requested tag
        // takes focus before the next paint.
        self.drain_focus_mailboxes();
    }

    /// R1462 §5.39 — resolve the focus this dispatch leaves behind,
    /// draining **both** focus mailboxes at one point and committing one
    /// observable transition.
    ///
    /// Two channels can move focus at the end of a dispatch, and they carry
    /// different *kinds* of thing:
    ///
    /// - [`pinion_core::modal_scope_request`] carries **automatic policy** —
    ///   WAI-ARIA's "focus moves into the dialog when it opens, returns to
    ///   the invoker when it closes". The binding asks for a modal; the
    ///   restore target is a snapshot the substrate took on its own.
    /// - [`pinion_core::focus_request`] carries **explicit intent** — a
    ///   binding naming the tag focus must end up on.
    ///
    /// So the modal batch is applied FIRST and the request LAST: the default
    /// lands, then a binding that knows better replaces it. Draining them the
    /// other way round (pre-R1462) made the automatic restore beat every
    /// explicit request issued in the same frame, so the toolkit-standard
    /// command palette — close the palette, run the row, focus what the row
    /// produced — always lost, landing either on the stale invoker or, when
    /// the command had removed that node, on nothing at all ([`FocusManager::pop_modal_scope`] commits its
    /// restore unconditionally, and `None` is a commit). The toolkit reaches this
    /// order by construction rather than by policy: a slot running `dialog.accept()` then
    /// `widget->setFocus()` wins with the later call.
    ///
    /// The order also makes "open a dialog focused on a *specific* control"
    /// expressible (a destructive prompt whose default is Cancel — or
    /// deliberately is not): the push installs the trap before the request
    /// is applied, so the member is in
    /// [`FocusManager::active_tab_order`] by the time `focus_set` looks.
    /// Applied the other way round the request ran while the trap was still
    /// down, and the R1020 re-derive could even let it *succeed* — which was
    /// worse than a refusal, because [`FocusManager::push_modal_scope`] then
    /// snapshotted the dialog's own control as the invoker to restore, so
    /// closing the dialog restored focus to a node the dialog took with it.
    ///
    /// One arc, one gate:
    ///
    /// - `External::on_focus_change` observers see the NET transition, never
    ///   the intermediate owner the pop transiently restored. R1456
    ///   established this for two modal edits in one frame; the same
    ///   reasoning spans the two mailboxes, because an owner the same frame
    ///   already replaced is never painted — announcing it makes a
    ///   background `TextField` commit an IME composition and a caret start
    ///   blinking off-screen.
    /// - the §5.34 revision bump + redraw fire when focus moved **or** the
    ///   modal stack was edited. A frame that only changes modal depth still
    ///   changes what is painted and what `focus/get` reports, and gating
    ///   that on a focus move alone (pre-R1462) meant a same-tag open/close
    ///   scheduled nothing.
    ///
    /// No-op — and no snapshot cost — when both mailboxes are empty, which
    /// is every quiescent frame.
    fn drain_focus_mailboxes(&mut self) {
        let modal_reqs = pinion_core::modal_scope_request::drain();
        let focus_req = pinion_core::focus_request::drain();
        if modal_reqs.is_empty() && focus_req.is_none() {
            return;
        }
        let focus_before = self.focus.focused().map(str::to_owned);
        let modal_edited = self.apply_modal_requests(modal_reqs);
        self.apply_focus_request(focus_req.as_deref());
        if self.focus.focused() != focus_before.as_deref() {
            self.notify_focus_change(focus_before.as_deref());
            self.revision.bump();
            self.request_redraw();
        } else if modal_edited {
            self.revision.bump();
            self.request_redraw();
        }
    }

    /// R693 §5.39 — apply a drained [`pinion_core::modal_scope_request`]
    /// batch via [`FocusManager::push_modal_scope`] /
    /// [`FocusManager::pop_modal_scope`]. Returns whether the modal stack
    /// was actually **edited** (an `Open`, or a `Close` that found a scope
    /// to pop) — [`Self::drain_focus_mailboxes`] gates its redraw on that,
    /// because a dialog appearing or vanishing changes the paint even when
    /// the focused tag happens to be identical either side.
    ///
    /// R1456 — the batch is applied **in order**, not just its last
    /// entry: the requests are stack edits, and one dispatch frame can
    /// legitimately carry two (a menu handing off to a confirm dialog
    /// closes itself and opens the dialog from one intent). Applying
    /// only one leaked the unapplied scope onto the modal stack
    /// permanently.
    ///
    /// R1462 — the observer arc is NOT snapshotted here. It belongs to
    /// [`Self::drain_focus_mailboxes`], which spans this batch *and* the
    /// focus request that may override its restore; an arc taken around
    /// the batch alone would announce an invoker that the same frame's
    /// explicit request has already replaced.
    fn apply_modal_requests(
        &mut self,
        reqs: Vec<pinion_core::modal_scope_request::ModalRequest>,
    ) -> bool {
        let mut edited = false;
        for req in reqs {
            match req {
                pinion_core::modal_scope_request::ModalRequest::Open { members } => {
                    edited = true;
                    self.focus.push_modal_scope(members)
                }
                pinion_core::modal_scope_request::ModalRequest::Close => {
                    // A `Close` against an empty stack pops nothing, so it is
                    // not an edit — `pop_modal_scope`'s own `false` cannot
                    // report that, since it means "focus did not move".
                    edited |= self.focus.is_modal();
                    self.focus.pop_modal_scope()
                }
            };
        }
        edited
    }

    /// R693 §5.39 — `true` while a modal focus trap is active. The
    /// winit shell consults this to keep `Escape` from terminating the
    /// event loop while a modal is up: a trapped `Escape` dismisses the
    /// modal (routed to the widget's `apply_key`) instead of the window.
    #[must_use]
    pub fn focus_is_modal(&self) -> bool {
        self.focus.is_modal()
    }

    /// R664 §5.39 — apply a drained [`pinion_core::focus_request`] entry
    /// via [`FocusManager::focus_set`]. No-op on `None` (the empty-mailbox
    /// steady state).
    ///
    /// Routes through the same `focus_set` the mouse-driven
    /// [`Self::click_to_focus_for_window`] uses, so a programmatic request
    /// and a pointer press are indistinguishable downstream — the focus ring, the
    /// `TextField` IME bridge and the
    /// [`CaretBlink`](pinion_core::widgets::caret_blink::CaretBlink) enable
    /// gate all react to one shape of focus transition.
    ///
    /// R1462 — the request is applied AFTER the frame's modal batch and
    /// carries no arc or redraw of its own; both belong to
    /// [`Self::drain_focus_mailboxes`], which owns the net transition.
    /// Being last is the whole point: an explicit request outranks the
    /// modal policy's automatic restore.
    fn apply_focus_request(&mut self, tag: Option<&str>) {
        let Some(tag) = tag else {
            return;
        };
        if !self.focus.focus_set(tag) {
            // (R1020 §5.39) The requested tag is not in the current focus
            // enumeration. Under scene-derived focus the enumeration is
            // refreshed inside the paint pass, which has NOT run since this
            // dispatch mutated reactive state — so a node painted only as a
            // RESULT of this dispatch (a conditionally-painted inline editor
            // whose reducer set `editing_id` then requested focus) is not yet
            // enumerated. Re-derive the enumeration from a fresh
            // side-effect-free view of the post-dispatch state and retry once,
            // so "focus a widget on the frame it appears" works (the pre-R1020
            // boot-seeded superset accepted such a request unconditionally).
            //
            // R1464 §5.16 — the retry is the DENOMINATOR of `focus_work`: it is
            // recorded whether or not the second `focus_set` succeeds, because
            // the cost was paid either way. Counting only successes would price
            // the enumeration at zero for the binding that misses most.
            self.focus_work.record_retry();
            self.refresh_focusable_from_view();
            // Still unknown / non-focusable — silent no-op (matches the
            // `click_to_focus` rejection arm): the requested tag is on no
            // painted focusable node, is confined out by an active modal
            // trap, or focus is already there.
            let _ = self.focus.focus_set(tag);
        }
    }

    /// R25.1 §5.39 §5.16 — record `window_key`'s focusable-tag contribution and
    /// re-fold the binding-wide [`FocusManager`] enumeration as the union across
    /// every painted window.
    ///
    /// The single `FocusManager` is binding-wide, but a focusable widget is
    /// drawn in exactly one window at a time (the dock model), so a secondary
    /// window's pane must JOIN the Tab order / click-focus set, not REPLACE the
    /// primary's — the pre-R25.1 [`pinion_runtime::DEFAULT_WINDOW`] gate dropped
    /// every secondary window's enumeration, leaving a torn-off pane unfocusable
    /// (sprag undock). Keying per window (the R1021
    /// [`pinion_runtime::CoreShell::publish_pane_viewports`] precedent) lets each
    /// window refresh only its own tags; the fold
    /// ([`Self::union_focusable_tags`]) recombines them.
    ///
    /// [`FocusManager::update_focusable_tags`] still applies the §5.39 stale-
    /// focus drop + modal guard, now against the union — focus drops only when
    /// the focused tag is in NO declared window (R26: painted OR declared-but-
    /// unpainted). A single-window binding has one entry and no declared signal,
    /// so the union equals that window's tags verbatim: byte-identical to the
    /// pre-R25.1 primary-only enumeration.
    ///
    /// R1327 §5.39 — a drop is paired with a redraw request. This runs INSIDE the
    /// paint pass, after `V::view` has read the old focus, so the frame being
    /// produced still names the tag that just died (a binding painting "active
    /// pane: `{focused()}`" — the R1327 seam's advertised use). The reactive dirty
    /// flag the publish raises is cleared before this paint returns
    /// (`clear_dirty`), so without this request nothing would schedule the
    /// correcting frame and the stale name would sit on screen until some unrelated
    /// event repainted. Every other focus mutation already pairs a redraw at its own
    /// call site (click / Tab / RPC / request-drain / modal).
    fn refresh_window_focusables(&mut self, window_key: &str, tags: Vec<String>) {
        self.window_state_mut(window_key).focusable_tags = Some(tags);
        self.refold_focusables();
    }

    /// R1463 §5.39 §5.16 — re-fold the per-window contributions into the
    /// binding-wide [`FocusManager`] enumeration and pair a dropped focus with a
    /// redraw. The tail of [`Self::refresh_window_focusables`], lifted so
    /// [`Self::refresh_focusable_from_view`] can rewrite SEVERAL windows'
    /// contributions and fold **once**: folding per window would bump the §5.34
    /// revision once per window for what is one enumeration change, and would
    /// hand [`FocusManager::update_focusable_tags`] intermediate unions in which
    /// a tag being moved between windows is momentarily in neither — a stale-focus
    /// drop of a tag that never left the binding.
    fn refold_focusables(&mut self) {
        let union = self.union_focusable_tags();
        if self.focus.update_focusable_tags(union) {
            self.revision.bump();
            self.request_redraw();
        }
    }

    /// R25.1 §5.39 / R26 §5.39 §5.16 — fold the focusable enumerations of every
    /// window in the DECLARED topology into one Tab order:
    /// [`pinion_runtime::DEFAULT_WINDOW`] first (so a single-window binding's
    /// order is its window's verbatim), then the remaining windows by sorted id,
    /// de-duplicated first-occurrence (a shared-state tag rendered in two windows
    /// joins the order once, at its earliest window).
    ///
    /// R26 §5.39 §5.16 — the window set is the union of the *painted* windows
    /// (those whose [`WindowState::focusable_tags`] is set) AND the windows the
    /// binding currently *declares* via [`WidgetView::windows_signal`]. A window the
    /// binding has just declared — a torn-off undock pane pushed into the signal
    /// — but whose OS window has not first-painted yet still contributes, via
    /// [`Self::window_focusables`] deriving its focusables from the pure
    /// [`WidgetView::view_for_window`] (§2 #7 scene-as-data: a window's focusable
    /// set is a pure function of state, knowable with no painted surface). This
    /// closes the `reconcile_windows` gap frame(s) where a torn-off pane's tag is
    /// in NO painted scene: without it [`FocusManager::update_focusable_tags`]
    /// would drop a focus the binding placed on that pane (the undock
    /// focus-follow gap) and a one-shot
    /// [`pinion_core::focus_request`] would be consumed before the pane is
    /// enumerable. A single-window binding declares no signal, so the declared
    /// set is empty and this is byte-identical to the pre-R26 painted-only union.
    ///
    /// Cross-window Tab traversal order is a pinion design choice (R25.2 leaves
    /// it open): primary-then-sorted is deterministic and keeps the single-
    /// window order unchanged. Click-to-focus is order-independent — it resolves
    /// the clicked tag against this set ([`FocusManager::resolve_focusable`]), so
    /// a secondary window's pane is focusable on click the moment it joins here.
    fn union_focusable_tags(&self) -> Vec<String> {
        let declared = self.declared_window_ids();
        let mut others: Vec<&str> = self
            .window_states
            .iter()
            .filter(|(_, s)| s.focusable_tags.is_some())
            .map(|(k, _)| k.as_str())
            .chain(declared.iter().map(String::as_str))
            .filter(|k| *k != pinion_runtime::DEFAULT_WINDOW)
            .collect();
        others.sort_unstable();
        others.dedup();

        let mut order: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for key in std::iter::once(pinion_runtime::DEFAULT_WINDOW).chain(others) {
            for tag in self.window_focusables(key) {
                if seen.insert(tag.clone()) {
                    order.push(tag);
                }
            }
        }
        order
    }

    /// R26 §5.39 §5.16 — the focusable tags of one window: its harvested paint
    /// scene if the window has painted (the materialised truth on screen),
    /// otherwise the DETERMINED scene derived from the pure view fn for a window
    /// the binding declares but has not painted yet (the undock reconcile gap).
    /// The derivation runs ONLY for that transient — a painted window
    /// short-circuits to the cache, so a steady-state multi-window binding (all
    /// declared windows painted) adds no extra view run.
    ///
    /// Harvest and derivation yield the SAME set when the window's focusable
    /// nodes are not viewport-gated: [`Scene::collect_focusable_tags`] reads only
    /// the view-assigned `tag` + [`LayoutStyle::focusable`](pinion_core::style::LayoutStyle),
    /// which layout never touches, so the harvest supersedes the derivation with
    /// no change once the window paints (and [`Self::union_focusable_tags`]
    /// de-dupes either way). CAVEAT — the *set of emitted nodes* (distinct from a
    /// painted node's marker) is the view fn's choice and may read a viewport
    /// signal: this standalone derivation publishes no pane viewport, so
    /// `use_pane_viewport_size` reads its `(0, 0)` "unmeasured" sentinel, and a
    /// view that gates focusable rows on the measured viewport (a virtualized
    /// list) derives that `(0, 0)` projection until first paint, then the harvest
    /// corrects it. The dock/undock model — one static top-level focusable per
    /// torn window — is not viewport-gated, so it derives exactly; no current
    /// consumer gates focusables on the viewport.
    ///
    /// Runs under `root_owner` for parity with the live paint's view dispatch (so
    /// `Owner::cache` hooks resolve to the same reactive state) and is
    /// side-effect-free per §6.3 — the same purity, AND the same precondition: a
    /// view fn must not create a bare `Effect::new` (it would double-execute on
    /// this extra run; §6.3 keeps a view's `Effect`s inside an `Owner::cache`
    /// factory), the invariant the boot-seed
    /// ([`Self::refresh_focusable_from_view`]) already depends on, now exercised
    /// for secondary windows too. No layout / viewport pass is needed.
    fn window_focusables(&self, window_id: &str) -> Vec<String> {
        if let Some(tags) = self
            .window_state(window_id)
            .and_then(|s| s.focusable_tags.as_ref())
        {
            return tags.clone();
        }
        self.derive_window_focusables(window_id)
    }

    /// R26 / R1463 §5.39 §5.16 — one window's focusable tags DERIVED from the
    /// pure view fn, ignoring any harvested cache: `V::view` for the primary,
    /// [`WidgetView::view_for_window`] for a secondary. Split out of
    /// [`Self::window_focusables`] (which prefers the cache) because
    /// [`Self::refresh_focusable_from_view`] needs the opposite preference — it
    /// runs precisely when the cache is known to be stale, one dispatch ahead of
    /// the paint that will refresh it. Purity and the `root_owner` scoping are
    /// documented on [`Self::window_focusables`]; both callers depend on them.
    fn derive_window_focusables(&self, window_id: &str) -> Vec<String> {
        // R1464 §5.16 — counted HERE, at the one place a view actually runs for
        // the enumeration, rather than at each caller: both callers reach it
        // (the R1020 miss-retry and the R26 fold over a declared-but-unpainted
        // window), and a caller-side count would silently miss whichever arm a
        // later round adds.
        self.focus_work.record_derivation();

        let cached_state = *self.core.cached_state();
        let frame = Frame::with_dt(0.0);
        let core = &self.core;
        core.root_owner().run(|| {
            let scene = if window_id == pinion_runtime::DEFAULT_WINDOW {
                V::view(cached_state, &frame)
            } else {
                V::view_for_window(window_id, cached_state, &frame)
            };
            scene.collect_focusable_tags()
        })
    }

    /// R26 §5.39 §5.16 — the window ids the binding currently DECLARES via
    /// [`WidgetView::windows_signal`], or empty for a single-window binding (the
    /// signal defaults to `None`). Read under `root_owner` so the opt-in
    /// `Owner::cache`-memoised signal resolves to the same handle the
    /// `reconcile_windows` Effect subscribes (and that the binding's dock
    /// tear-off mutates) — the declared topology is the SSOT the focus
    /// enumeration tracks AHEAD of paint, so a just-declared window's pane is
    /// enumerable on the same dispatch that declared it, not a few frames later
    /// when its OS window first paints.
    ///
    /// Correctness rests on the SAME `Owner::cache` identity contract
    /// [`crate::AppShell::reconcile_windows`] relies on: [`WidgetView::windows_signal`]
    /// must memoise its `Rc<Signal<..>>` (keyed `(TypeId::<V>, key)`) so this read
    /// and the AppShell-side reconcile resolve the POINTER-EQUAL signal. A binding
    /// that returned a fresh signal per call would let focus enumerate against one
    /// instance while reconcile acts on another — the two would silently diverge.
    /// Reading via `Signal::get` subscribes `root_owner` to the signal, but that
    /// subscription is idempotent (already established at `resumed`) and read-only
    /// (`get` never dirties), so it adds no paint.
    fn declared_window_ids(&self) -> Vec<String> {
        let core = &self.core;
        core.root_owner().run(|| {
            V::windows_signal()
                .map(|sig| {
                    sig.get()
                        .into_iter()
                        .map(|spec| spec.id.into_owned())
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    /// R1087 §5.16 §5.41 §2 #7 PR-31 — the windows the binding currently
    /// DECLARES, projected to the `scene/windows` wire shape
    /// ([`pinion_rpc::DeclaredWindow`]: id + title + declared geometry —
    /// position R1087 and `declared_size` R1092, each `null` when that
    /// axis is system-determined rather than declared).
    ///
    /// Reads the reactive [`WidgetView::windows_signal`] when the binding
    /// opted into one (the dock tear-off arc — the SSOT
    /// [`crate::AppShell::reconcile_windows`] also tracks), otherwise the
    /// compile-time [`WidgetView::windows`] list (every single-window +
    /// frozen multi-window binding), so the read is honest for ALL binding
    /// shapes — a single-window binding still reports its `"main"` window.
    /// Read under `root_owner` for the same `Owner::cache` signal-identity
    /// contract [`Self::declared_window_ids`] documents.
    ///
    /// This is the scene-as-data observability for the
    /// floating-panel-as-positioned-window model: the position a binding's
    /// tear-off reducer writes into the signal is read back here, so an AI
    /// observes WHERE each torn-off panel's window is **declared to sit**,
    /// not merely that it exists (the §2 #7 obligation for the new
    /// `WindowSpec::position` state). It is the DECLARED position (what the
    /// binding wrote / the shell drives toward), not a live OS read-back. R1088
    /// (`AppShell::note_window_moved`) DOES feed a user `WindowEvent::Moved`
    /// back into the signal for an already-positioned window (declared
    /// converges on actual), but a `None` WM-placed window is left WM-managed
    /// and its drag is never reflected, and the live feedback is HW-gated — so
    /// the `pinion_rpc::DeclaredWindow` naming keeps the read honest as
    /// declared intent. Resolved only
    /// for the `scene/windows` method (gated at the dispatch call site), so
    /// every other dispatch pays nothing.
    /// R1711 §5.16 §5.32 §2 #3 §2 #7 — what a reader could not get to if this
    /// window were `(w, h)`, **without the window becoming that size**.
    ///
    /// The mirror lays the screen out at the asked extent (the R1468
    /// containment scope, so no side effect escapes a paint nobody is looking
    /// at) and the same collector `scene/scroll_reach` runs judges it. One rule
    /// for the live question and the hypothetical one: a dry answer that came
    /// from a second derivation could disagree with the wet one, which is the
    /// failure §2 #3 exists to make impossible.
    fn reach_at(
        &mut self,
        window_id: Option<&str>,
        w: u32,
        h: u32,
    ) -> pinion_rpc::scroll_reach::ScrollReachOutcome {
        let scene = self.compute_paint_scene_pure_internal(window_id, w, h);
        pinion_rpc::scroll_reach::collect(&scene, &mut self.text_cache)
    }

    /// R1711 §5.16 §5.32 §2 #3 §2 #7 — the smallest window this screen was
    /// measured to work in, and what one pixel less puts out of reach.
    ///
    /// The search is [`pinion_core::size_floor::measure`]; this method is only
    /// the probe it drives, which is why the arithmetic has one home and the
    /// embedder has none. Each probe is a full mirror paint, so the answer
    /// costs roughly two dozen of them and says so on the wire.
    ///
    /// The ceiling defaults to the window's own current extent — the size the
    /// screen is expected to fit in, read off the mirror rather than from a
    /// stored number so it cannot disagree with what the screen was laid out
    /// against. `params.at` overrides it, which is what lets a caller ask "and
    /// how small could it be on a display this size".
    fn measure_size_floor(
        &mut self,
        window_id: Option<&str>,
        params: Option<&serde_json::Value>,
    ) -> pinion_rpc::size_floor::SizeFloorOutcome {
        let declared = self.primary_window_bounds();
        let live = self
            .core
            .last_paint_scene_for_window(window_id.unwrap_or(pinion_runtime::DEFAULT_WINDOW))
            .map(|paint| {
                let rect = paint.rect();
                (rect.w, rect.h)
            });
        // A window that has never painted has no extent of its own to be
        // measured against; its declared floor is the next most honest ceiling,
        // and a screen that does not fit even there is exactly the
        // `ceiling_is_short` finding rather than a special case.
        let ceiling = pinion_rpc::scroll_reach::parse_at(params)
            .ok()
            .flatten()
            .or(live)
            .or_else(|| declared.floor())
            .unwrap_or_default();
        let result = pinion_core::size_floor::measure(ceiling, |w, h| {
            let scene = self.compute_paint_scene_pure_internal(window_id, w, h);
            pinion_rpc::size_floor::cut_at(&scene, &mut self.text_cache)
        });
        // R1712 — the concession is audited at the floor the POLICY names, not
        // at the one the search found: the question is whether the binding's
        // declaration is honest about the size it lets the reader reach, and
        // the measured floor is the other half of that comparison.
        let concession = V::shrink_policy().map(|policy| {
            let floor = policy.floor();
            let scene = self.compute_paint_scene_pure_internal(window_id, floor.0, floor.1);
            pinion_rpc::size_floor::audit_at(&scene, &mut self.text_cache, policy, declared)
        });
        pinion_rpc::size_floor::report(&result, ceiling, declared, concession, &<[_]>::to_vec)
    }

    /// R1710 §5.16 §5.12 §2 #2 — the size bounds the **primary** window
    /// declares, for `scene/resize` to resolve an ask against.
    ///
    /// Primary, not the addressed scope, because the resize hook itself targets
    /// the primary window (`AppShell` documents that at the closure: per-window
    /// `scene/resize` is a follow-up axis awaiting a real consumer). Reporting
    /// another window's floor while resizing this one would be a second fact
    /// disagreeing with the first — the defect class this round is closing.
    ///
    /// Reads the same declared-window list [`Self::declared_window_specs`]
    /// projects, so the floor enforced here is the floor
    /// `SizeStrategy::min_inner_floor` declared to the window system at create.
    /// A binding declaring no window at all resolves to
    /// [`SizeBounds::UNBOUNDED`] — nothing declared, nothing enforced.
    fn primary_window_bounds(&self) -> SizeBounds {
        let core = &self.core;
        let specs = core.root_owner().run(|| match V::windows_signal() {
            Some(sig) => sig.get(),
            None => V::windows(),
        });
        specs
            .iter()
            .find(|spec| spec.id.as_ref() == pinion_runtime::DEFAULT_WINDOW)
            .or_else(|| specs.first())
            .map_or(SizeBounds::UNBOUNDED, |spec| spec.strategy.window_bounds())
    }

    fn declared_window_specs(&self) -> Vec<pinion_rpc::DeclaredWindow> {
        let core = &self.core;
        let specs = core.root_owner().run(|| match V::windows_signal() {
            Some(sig) => sig.get(),
            None => V::windows(),
        });
        // R1576 — resolved against the monitors attached right now, so
        // `anchored` reports what the declaration MEANS today rather than a
        // stored copy that could disagree with it. `None` (no stamp) leaves
        // every window's `anchored` null: nobody looked, so nothing is claimed.
        let topology = self.display_topology();
        // R1610 — stamped once at resume; `None` on a surface that has no
        // windowing system to stamp.
        let backend = self.windowing_backend();
        specs
            .into_iter()
            .map(|spec| pinion_rpc::DeclaredWindow {
                // R1576 §5.16 §5.41 — the DECLARED display and what became of
                // the declaration. Derived here rather than stored, so the
                // report cannot become a stale second copy of where the window
                // is (the one-fact-one-source argument R1575 records).
                display: spec.display.as_ref().map(|d| d.as_str().to_owned()),
                anchored: topology
                    .as_ref()
                    .zip(spec.placement())
                    .map(|(topology, placement)| {
                        let declared = placement.display.clone().unwrap_or_else(|| {
                            pinion_core::display::DisplayId::new(crate::ABSOLUTE_PLACEMENT)
                        });
                        let anchor = pinion_core::display::Anchor::new(declared, placement.offset);
                        pinion_rpc::AnchoredOutcome::from((&anchor, placement.resolve(topology)))
                    }),
                // R1617 §5.16 §5.41 — where the window ACTUALLY is, per this
                // framework's derivation from its live rectangle AND per the
                // window system's own opinion. A different question from
                // `anchored`, which is what became of the DECLARATION: a
                // WM-placed window declares nothing and still has a home, and a
                // window the user dragged has moved without the declaration
                // changing. `None` when nobody looked. Read BEFORE `id`, which
                // consumes the spec's own copy of it.
                display_home: self
                    .window_display_home(&spec.id)
                    .as_ref()
                    .map(pinion_rpc::dispatch::DisplayHomeWire::from),
                id: spec.id.into_owned(),
                title: spec.title,
                position: spec.position,
                // R1092 — declared open size from the SSOT
                // `SizeStrategy::declared_size` (`None` for a
                // content-intrinsic window, mirroring `position`'s
                // `None`-means-system-determined honesty).
                declared_size: spec.strategy.declared_size(),
                // R1115 §5.51 PR-38 — declared OS chrome (the
                // `WindowSpec::decorations` SSOT; `true` for every
                // pre-R1115 binding). Always known, never `null`.
                decorations: spec.decorations,
                // R1610 §5.16 §5.41 — the DECLARED level, and what the
                // windowing system actually running does with it. Derived
                // here against the stamped backend rather than stored, for
                // the same one-fact-one-source reason `anchored` is; `None`
                // when nobody stamped a backend, so a TUI surface claims
                // nothing rather than inventing a window system.
                level: spec.level.as_str().to_owned(),
                level_outcome: backend.map(|b| b.outcome(spec.level).into()),
            })
            .collect()
    }

    /// R1099 §5.51 §2 #7 PR-33 — resolve the cross-window drop for the absolute
    /// desktop cursor in `params` (the `scene/cross_window_drop` READ). Borrows
    /// every declared window's stored paint scene + declared outer position and
    /// runs [`pinion_runtime::resolve_cross_window_drop`].
    ///
    /// Done here, `&self`, before the dispatch borrow split, because
    /// [`pinion_core::scene::Scene`] is not `Clone` — the resolution cannot own
    /// cloned scenes, so it runs in place and the dispatch context carries only
    /// the small owned result. A `None` declared position means WM-placed at the
    /// desktop origin (the same `(0, 0)` convention `scene/windows` reports).
    /// `None` result when the request names no cursor (the handler maps that to
    /// `MissingCursor`) or the cursor lands on no window's drop target.
    fn resolve_cross_window_drop_for_request(
        &self,
        params: Option<&serde_json::Value>,
    ) -> Option<pinion_runtime::CrossWindowDrop> {
        let cursor = pinion_rpc::cross_window_drop::parse_params(params)?;
        // The RPC query is source-less — it asks "which window does this abs
        // cursor map onto", excluding nothing. R1148 — `use_actual=false`: the
        // headless RPC path has no live winit handles, so it resolves against the
        // DECLARED positions `scene/windows` reports (the AI-introspectable model).
        self.cross_window_drop_at((cursor.x, cursor.y), None, false)
    }

    /// R1102 §5.51 PR-33 — resolve a desktop-absolute logical cursor against
    /// every declared window that has a stored paint scene, optionally excluding
    /// `exclude`. The shared body of the `scene/cross_window_drop` READ
    /// (`exclude: None`) and the live-drag composition (`exclude: Some(source)` —
    /// the F5 caller-owned source exclusion the R1101 doc promised). A `None`
    /// declared position means WM-placed at the desktop origin (the `(0, 0)`
    /// convention `scene/windows` reports).
    fn cross_window_drop_at(
        &self,
        abs: (f64, f64),
        exclude: Option<&str>,
        use_actual: bool,
    ) -> Option<pinion_runtime::CrossWindowDrop> {
        let specs = self.declared_window_specs();
        let windows: Vec<(&str, &Scene, (f64, f64))> = specs
            .iter()
            .filter(|w| exclude != Some(w.id.as_str()))
            .filter_map(|w| {
                let scene = self.core.last_paint_scene_for_window(&w.id)?;
                // R1148 §5.51 — the LIVE path uses the shell-stamped ACTUAL outer
                // origin (a WM-placed window has declared position `None`, so the
                // DECLARED fallback `(0,0)` would offset the hit-test by the real
                // WM origin); the RPC path keeps declared positions.
                let pos = use_actual
                    .then(|| self.core.live_window_origin(&w.id))
                    .flatten()
                    .or_else(|| w.position.map(|(x, y)| (f64::from(x), f64::from(y))))
                    .unwrap_or((0.0, 0.0));
                Some((w.id.as_str(), scene, pos))
            })
            .collect();
        pinion_runtime::resolve_cross_window_drop(
            windows.iter().map(|&(id, scene, pos)| (id, scene, pos)),
            abs,
        )
    }

    /// R1102 §5.51 PR-33 — resolve the LIVE drag's cross-window drop: map the
    /// source window's local cursor to a desktop-absolute point (source outer
    /// position + local), then resolve it against the OTHER windows (source
    /// excluded). `None` when there is no other window, when the source window is
    /// not declared, or when the abs cursor maps onto no other window's drop
    /// target. Called per cursor move while a drag this window owns is in
    /// flight, so the per-window (cross-window-blind) router can fill
    /// [`pinion_core::external::DragUpdate::over_window`].
    fn resolve_cross_window_live(
        &self,
        source_window: &str,
        local: (f64, f64),
    ) -> Option<pinion_runtime::CrossWindowDrop> {
        let specs = self.declared_window_specs();
        if specs.len() < 2 {
            return None;
        }
        let source = specs.iter().find(|w| w.id == source_window)?;
        // R1148 §5.51 → R1151 — use the source window's ACTUAL client origin
        // (shell-stamped each move), not the DECLARED `position`. The declared fallback is `(0,0)`
        // for a WM-placed window, and even a positioned floater's declared origin
        // can lag the WM; the actual origin makes `abs = source_origin + local` the
        // true desktop pointer. (The R1120 lesson: resolve geometry against actual,
        // not lagging declared, positions.)
        let source_pos = self
            .core
            .live_window_origin(source_window)
            .or_else(|| source.position.map(|(x, y)| (f64::from(x), f64::from(y))))
            .unwrap_or((0.0, 0.0));
        let abs = (source_pos.0 + local.0, source_pos.1 + local.1);
        // R1146 §5.51 — cursor-precise, single point (the pro-tool / VS Code
        // model). The floater stays put during the drag (the window is repositioned
        // only on release), so `source_pos` is stable and `abs = source_pos + local`
        // is the true desktop pointer — the cursor resolves cleanly over the target
        // zone. The R1143 body-centre fallback was removed (band-aid for the moving
        // floater); see `docs/dock-window-move-redesign.md`. R1148 — actual origins
        // for the targets too, so a WM-placed `"main"` hit-tests at its real offset.
        self.cross_window_drop_at(abs, Some(source_window), true)
    }

    /// (R1020 §5.39) Re-derive the keyboard focus enumeration from a fresh,
    /// side-effect-free run of [`WidgetCore::view`](pinion_core::WidgetCore::view) over the current cached
    /// state, feeding [`FocusManager::update_focusable_tags`]. Used by
    /// [`Self::apply_focus_request`] to enumerate a node that this dispatch
    /// just made paintable BEFORE the next paint pass runs the per-frame
    /// refresh.
    ///
    /// No layout / viewport size is needed: [`Scene::collect_focusable_tags`]
    /// reads only each node's `tag` + `LayoutStyle::focusable`, both assigned
    /// by the view fn itself (layout resolves rects, never the focus markers).
    /// The view runs under `root_owner.run(...)` so `Owner::cache` hooks
    /// resolve to the same reactive state the live paint will read; it is pure
    /// (§6.3) so this extra invocation has no observable effect beyond the
    /// enumeration it produces.
    ///
    /// "No observable effect" rests on the §6.3 view-fn discipline that any
    /// `Effect::new` a view fn creates lives inside an [`Owner::cache`](pinion_core::reactive::Owner::cache)
    /// factory (run once per owner+key), not bare in the view body. A bare
    /// `Effect::new` would fire eagerly on this extra view run and double-
    /// execute — no current binding does that, but it is the invariant this
    /// re-derive (and the boot-seed run) depends on.
    ///
    /// R25.1 §5.39 §5.16 — refreshes each window's per-window contribution and
    /// re-folds the union ([`Self::refold_focusables`]) rather than replacing
    /// the whole `tab_order`: a programmatic focus request must not drop
    /// another window's focusable panes from the enumeration.
    ///
    /// R1463 §5.39 §5.16 — the refresh spans **every window the binding has
    /// painted**, not only [`pinion_runtime::DEFAULT_WINDOW`]. A painted window
    /// short-circuits [`Self::window_focusables`] to its harvested cache, so
    /// re-deriving the primary alone left a torn-off pane's enumeration frozen
    /// at its last paint: the identical reducer — open an inline editor, name it
    /// — landed in the primary and was silently dropped in a secondary window,
    /// its one-shot [`pinion_core::focus_request`] consumed by the miss. Windows
    /// the binding merely DECLARES need nothing here; [`Self::window_focusables`]
    /// derives those on every fold already (R26), so this adds a derivation only
    /// where a stale harvest would otherwise answer.
    ///
    /// Cost is paid on a focus MISS, not per frame: one view run per painted
    /// window, bounded by the declared topology. The steady state — a request
    /// naming an already-enumerated tag — never reaches here.
    ///
    /// R1464 §5.16 — that paragraph was prose when R1463 wrote it, and prose is
    /// not a bound. [`Self::focus_work`] now counts both halves of it, so the
    /// claim is checkable in a test and priceable over `scene/frame_timings`:
    /// `derivations / retries` IS "one view run per painted window", measured.
    ///
    /// A window with no contribution yet — including the primary before its
    /// first paint, which is when the boot seed calls this — is deliberately
    /// NOT written here: [`Self::union_focusable_tags`] always folds the
    /// primary and derives any uncached window itself, so seeding it a second
    /// time would be redundant work whose absence no assertion can see.
    ///
    /// The derivation is as faithful for a secondary window as for the primary
    /// because the pane-viewport publish is per-window (R1021), so a re-derived
    /// torn pane reads the sizes ITS window painted. The residual asymmetry is
    /// [`use_viewport_size`](pinion_core::reactive::viewport::use_viewport_size),
    /// which stays primary-gated: a secondary view that gates focusable nodes on
    /// the *window* size derives against the primary's. That predates this and is
    /// the same caveat [`Self::window_focusables`] documents for the R26
    /// declared-but-unpainted derivation.
    fn refresh_focusable_from_view(&mut self) {
        let mut painted: Vec<String> = self
            .window_states
            .iter()
            .filter(|(_, s)| s.focusable_tags.is_some())
            .map(|(k, _)| k.clone())
            .collect();
        // `window_states` is a HashMap; sort so the view runs happen in a
        // deterministic order. Fold-once makes the order irrelevant to the
        // RESULT — that is the point of `refold_focusables` — so this buys
        // reproducibility, not correctness.
        painted.sort_unstable();
        for key in painted {
            let tags = self.derive_window_focusables(&key);
            self.window_state_mut(&key).focusable_tags = Some(tags);
        }
        self.refold_focusables();
    }

    /// R51.159 §5.23 — install or replace the
    /// [`CommandExecutor`] the
    /// substrate's `Self::handle_tail` drains pending
    /// [`pinion_core::Command`]s into. Forwards to
    /// [`CoreShell::set_executor`].
    pub fn set_command_executor(
        &mut self,
        executor: std::sync::Arc<CommandExecutor>,
    ) -> Option<std::sync::Arc<CommandExecutor>> {
        self.core.set_executor(executor)
    }

    /// R51.159 §5.23 — borrow the currently-installed
    /// [`CommandExecutor`]. `None` until
    /// [`Self::set_command_executor`] runs.
    #[must_use]
    pub fn command_executor(&self) -> Option<&std::sync::Arc<CommandExecutor>> {
        self.core.executor()
    }

    /// R51.159 §5.23 — re-feed a resolved [`Intent`] (arriving via
    /// [`AppEvent::IntentArrived`](crate::AppEvent::IntentArrived)
    /// from the [`ProxyIntentSink`](crate::ProxyIntentSink)) into the
    /// SCXML `send` channel.
    ///
    /// This is the closing step of the §5.23 R27 dispatch loop:
    ///
    /// ```text
    /// Owner.dispatch_command(cmd)
    ///   → CommandExecutor::dispatch → tokio worker → Intent
    ///   → ProxyIntentSink::send → AppEvent::IntentArrived
    ///   → AppShell.user_event arm → ShellCore::dispatch_intent
    ///   → CoreShell::send_to_primary → SCXML transition.
    /// ```
    ///
    /// R51.159 first-cut routes the intent's tag through the same
    /// `invoke("send", Text(tag))` channel typed widget events use
    /// ([`CoreShell::forward`]); R884 lifted that send into the
    /// shape-agnostic [`CoreShell::send_to_primary`] home.
    ///
    /// R51.172 §5.23 R27 design clarification — the
    /// [`Intent::payload`](pinion_core::Intent) is NOT threaded into
    /// the SCXML invoke send by design. Pinion's split is:
    ///
    /// - **SCXML statechart** = `Model` mutation; transitions on
    ///   event *names* only (`PointerUp`, `KeyboardActivate`,
    ///   `echo.demo.echo`, …). The pinion SCXML wrapping does not
    ///   surface event data to transition guards.
    /// - **`WidgetCore::update`** (R51.166-171) = reducer; receives
    ///   the *full* [`Intent`] (tag **and** payload) and decides
    ///   what `Vec<Command>` to emit. Payload is consumed here.
    ///
    /// The R51.159-era "payload-aware SCXML send" carry resolves to
    /// "the reducer is where payload belongs". If a future widget
    /// needs payload-driven state transitions, the SCXML invoke
    /// contract would extend to `IntrospectValue::Json({"tag",
    /// "payload"})` and every `"send"` handler would learn the new
    /// shape — but no current widget has that need.
    pub fn dispatch_intent(&mut self, intent: &Intent) {
        eprintln!(
            "shell: intent-feedback {} payload={:?}",
            intent.tag_str(),
            intent.payload,
        );
        // R51.168 §5.23 R27 — reducer step: run `V::update` first so
        // any returned `Vec<Command>` lands on the root owner's queue,
        // then advance the SCXML statechart via `invoke("send", tag)`.
        // Mirrors the Elm/Iced ordering (Update before Cmd dispatch);
        // the substrate API queues the commands itself so dropping the
        // return value here is correct. R884 — the send routes through
        // `CoreShell::send_to_primary` (the one shape-agnostic home);
        // the pre-R884 inline bare-External root match silently skipped
        // the send for every multi-External binding.
        let _ = self.core.route_intent_through_update(intent);
        self.core.send_to_primary(intent.tag_str());
        self.revision.bump();
        let tail = self.core.tail();
        self.handle_tail(&tail);
    }

    /// R51.77 §5.40 — pure planning step for the §5.40 AccessKit
    /// emit. Borrows the freshly-computed nodes + focus, consults
    /// the substrate's incremental caches, and returns the emit
    /// verdict + dirty-tag diff. **Does not mutate any
    /// `ShellCore` state** — the caller invokes
    /// [`Self::commit_access_emit`] after the `Adapter::update_if_active`
    /// closure has consumed the nodes, completing the cache update
    /// in a separate step.
    ///
    /// Two-step rationale (R51.77 split): pre-R51.77
    /// a single combined call bundled the decision AND the cache
    /// update into one `&mut self` method named like a pure function.
    /// Reading the name without reading the body suggested
    /// idempotence; two back-to-back calls actually yielded different
    /// answers (the second saw the first's cache update). The
    /// `plan_access_emit` / `commit_access_emit` pair makes the
    /// state-machine step explicit:
    ///
    /// 1. `plan_access_emit(&nodes, focus.as_ref())` — pure decision.
    /// 2. If `decision.should_emit`, feed `nodes` + `focus` into the
    ///    closure passed to `Adapter::update_if_active`.
    /// 3. `commit_access_emit(&nodes, focus.as_ref())` — advances
    ///    the cache so the next plan sees the post-emit baseline.
    ///
    /// Tests exercise the pure planner via two back-to-back
    /// `plan_access_emit` calls separated by a `commit_access_emit`
    /// without any AccessKit adapter on hand (R51.75 no-change
    /// verification path).
    #[must_use]
    pub fn plan_access_emit(
        &self,
        nodes: &[AccessNode],
        focus: Option<&AccessFocus>,
    ) -> AccessEmitDecision {
        // R51.72 §5.40 — diff against the previous frame's node
        // cache. The initial frame emits every tag (the AT has no
        // prior state); subsequent frames emit only tags whose
        // `AccessNode` body (name / value / state / bounds / children)
        // actually changed.
        let initial = self.access_emit_initial;
        let dirty: HashSet<String> = if initial {
            nodes.iter().map(|n| n.tag.clone()).collect()
        } else {
            nodes
                .iter()
                .filter(|n| self.last_access_nodes.get(&n.tag) != Some(*n))
                .map(|n| n.tag.clone())
                .collect()
        };
        // R51.75 §5.40 — no-change frame skip. Emit only when the
        // initial-frame flag is set, the dirty set is non-empty, or
        // the focus declaration shifted. Otherwise the TreeUpdate
        // would be a pure no-op (root re-emit + identical focus).
        let focus_changed = focus != self.last_access_focus.as_ref();
        let should_emit = initial || !dirty.is_empty() || focus_changed;
        AccessEmitDecision {
            should_emit,
            initial,
            dirty,
        }
    }

    /// R51.77 §5.40 — commit step paired with
    /// [`Self::plan_access_emit`]. Advances the substrate's
    /// incremental caches to the just-emitted baseline so the next
    /// planning call diffs against this frame.
    ///
    /// Always run after the `Adapter::update_if_active` closure has
    /// consumed (or borrowed) the nodes — even when
    /// `decision.should_emit` is `false`, calling `commit_access_emit`
    /// is safe (it idempotently rewrites the cache to the same
    /// values). The textbook canonical idiom is "plan, optionally
    /// emit, always commit".
    ///
    /// R51.79 §5.40 — signature takes `nodes: Vec<AccessNode>`
    /// by-value so the Vec moves straight into `last_access_nodes`
    /// without a per-node clone. Pre-R51.79 took `&[AccessNode]` and
    /// did `nodes.iter().cloned()` internally, doubling the per-frame
    /// allocation budget (one clone for the emit closure, one clone
    /// for the cache). The new shape pairs with
    /// [`AccessTreeBuilder::add`](pinion_a11y::AccessTreeBuilder::add) taking `&AccessNode` — the emit
    /// closure borrows from `nodes`, then `commit_access_emit`
    /// consumes by-value: one clone per node, in the builder only.
    ///
    /// Update set: `last_access_tag_map` (`NodeId` → tag for AT-side
    /// action routing), `last_access_nodes` (per-tag snapshot for the
    /// next dirty diff — moved in by-value), `last_access_focus`
    /// (for the next focus-change detection), `access_emit_initial`
    /// (set to `false` after the first commit so the next plan emits
    /// incrementally).
    pub fn commit_access_emit(&mut self, nodes: Vec<AccessNode>, focus: Option<&AccessFocus>) {
        // R51.67 §5.40 — refresh the NodeId → tag map. Borrow before
        // the by-value move below.
        self.last_access_tag_map = build_tag_map(&nodes);
        // R51.79 §5.40 — move the Vec straight into the per-tag
        // HashMap. `tag.clone()` lifts only the key (a String) out;
        // each `AccessNode` itself moves without an extra clone.
        self.last_access_nodes = nodes.into_iter().map(|n| (n.tag.clone(), n)).collect();
        // Refresh the focus snapshot for the next frame's
        // focus-change check.
        self.last_access_focus = focus.cloned();
        self.access_emit_initial = false;
    }

    /// R51.67 §5.40 — translate an AccessKit `ActionRequest` into a
    /// pinion-native widget intent and dispatch it through the same
    /// focus / `apply_key` substrate the winit keyboard path uses.
    /// Returns silently when the request targets the synthetic root
    /// window or an unknown `NodeId` (stale tree, AT race).
    pub fn handle_action_request(&mut self, req: &accesskit::ActionRequest) {
        let Some(action) = translate_action(req, &self.last_access_tag_map) else {
            return;
        };
        self.dispatch_access_action(&action);
    }

    /// R51.67 §5.40 — pinion-native dispatch for one AT-driven
    /// widget action.
    ///
    /// Mapping (atomic widgets):
    /// - `Focus`          → [`FocusManager::focus_set`] + redraw
    /// - `Click` / `Default` → focus + `apply_key("Enter")`
    /// - `Increment`      → focus + `apply_key("ArrowRight")`
    /// - `Decrement`      → focus + `apply_key("ArrowLeft")`
    /// - `Other`          → silent drop
    ///
    /// R51.70 §5.40 — composite child tags (containing `#`) focus
    /// the parent and route the action through
    /// [`WidgetView::access_child_invoke`](pinion_a11y::WidgetA11y::access_child_invoke) before falling back to
    /// the atomic chain. The composite parses the sub-tag (the
    /// segment after `#`) and dispatches through its own wire-format
    /// invocation path; the shell stays composite-agnostic.
    pub fn dispatch_access_action(&mut self, action: &PinionAccessAction) {
        // R742.4 — shared `#` SSOT (an empty sub-index `"tag#"` resolves
        // to `None`, identical downstream to the old `Some("")` whose
        // `access_child_invoke` parse would fail to a no-op).
        let (parent_tag, sub_tag) = pinion_core::composite_tag::split_subindex(&action.tag);
        match action.kind {
            AccessAction::Focus => {
                let focus_before = self.focus.focused().map(str::to_owned);
                self.focus.focus_set(parent_tag);
                self.notify_focus_change(focus_before.as_deref());
                if let Some(sub) = sub_tag {
                    // R51.82 §5.40 — composite Focus routes through
                    // the same `access_child_invoke` hook the Click
                    // path uses, so the composite can update its
                    // active-descendant model (move the visually
                    // current row inside a `RadioGroup`, light up
                    // the current item in a future `ListBox` /
                    // `MenuButton` / `TreeView`) without selecting.
                    // The composite is responsible for distinguishing
                    // Focus from Click in its impl; the shell stays
                    // composite-agnostic. Return value is observed
                    // only as "intent accepted" — Focus never falls
                    // back to the atomic `apply_key` chain (no
                    // keyboard equivalent for "make this the active
                    // descendant").
                    let _ =
                        V::access_child_invoke(self.core.scene_mut(), parent_tag, sub, action.kind);
                    self.revision.bump();
                    let tail = self.core.tail();
                    self.handle_tail(&tail);
                }
                self.request_redraw();
            }
            AccessAction::Click | AccessAction::Default => {
                let focus_before = self.focus.focused().map(str::to_owned);
                self.focus.focus_set(parent_tag);
                self.notify_focus_change(focus_before.as_deref());
                if let Some(sub) = sub_tag {
                    // R51.70 §5.40 — composite child dispatch hook.
                    // The composite invokes its wire format and
                    // returns `true`; we commit the same revision /
                    // refresh / drain bookkeeping `apply_a11y_key`
                    // performs so AT-driven activation matches the
                    // keyboard path 1:1.
                    if V::access_child_invoke(self.core.scene_mut(), parent_tag, sub, action.kind) {
                        self.revision.bump();
                        let tail = self.core.tail();
                        self.handle_tail(&tail);
                        self.request_redraw();
                        return;
                    }
                    // Composite declined (unrecognised sub-tag /
                    // unsupported action) — fall through so the AT
                    // still sees activation feedback via the parent.
                }
                self.apply_a11y_key(parent_tag, "Enter");
            }
            AccessAction::Increment => self.apply_a11y_key(parent_tag, "ArrowRight"),
            AccessAction::Decrement => self.apply_a11y_key(parent_tag, "ArrowLeft"),
            AccessAction::Other => {}
        }
    }

    /// R51.67 §5.40 — focus + `apply_key` shared by `Click`,
    /// `Increment`, and `Decrement` arms. Mirrors the winit
    /// keyboard-path bookkeeping ([`Self::apply_key`]): bump the
    /// §5.34 OCC revision, re-read cached state, drain pending
    /// intents on handled, request a redraw regardless so the
    /// AT-side activation surfaces visually.
    ///
    /// R51.123 §5.41 — body delegates to [`CoreShell::apply_key`];
    /// the focus mutation happens before the dispatch so
    /// `core.apply_key` sees the just-focused tag in the
    /// `Some(tag)` argument.
    fn apply_a11y_key(&mut self, tag: &str, key: &str) {
        let focus_before = self.focus.focused().map(str::to_owned);
        self.focus.focus_set(tag);
        self.notify_focus_change(focus_before.as_deref());
        // R56.1.f.0 §5.13 — AT-driven activation (Click / Increment /
        // Decrement) maps to a *plain* keystroke; the AT side does not
        // surface modifier state, and no AT activation idiom (Click ↔
        // Enter, Increment ↔ ArrowRight, Decrement ↔ ArrowLeft)
        // implies any modifier. Empty Modifiers matches the W3C ARIA
        // canonical activation shape (W3C `AccessibilityFeatures` does
        // not carry a modifier slot for `Click` / `Increment` /
        // `Decrement` actions).
        let modifiers = pinion_core::Modifiers::empty();
        if let Some(tail) = self.core.apply_key(Some(tag), key, modifiers) {
            self.revision.bump();
            self.handle_tail(&tail);
        }
        self.request_redraw();
    }
}

/// R51.67 §5.40 — build the `NodeId` → widget tag map for a
/// freshly-collected list of `AccessNode`s.
///
/// Includes the synthetic root entry (`ROOT_NODE_ID` → `""`) so
/// `pinion_a11y::translate_action` can treat a root-targeted action
/// request as a sentinel and drop it without crossing into widget
/// dispatch.
///
/// R51.92 §5.40 — module-local helper (was free fn in `lib.rs`).
/// Sole caller is [`ShellCore::commit_access_emit`] above.
fn build_tag_map(nodes: &[AccessNode]) -> HashMap<NodeId, String> {
    let mut map = HashMap::with_capacity(nodes.len() + 1);
    map.insert(ROOT_NODE_ID, String::new());
    for node in nodes {
        map.insert(tag_to_node_id(&node.tag), node.tag.clone());
    }
    map
}

/// R863 §5.40 §5.27 §5.45 — resolve each [`AccessNode`]'s `bounds` from the
/// post-layout paint scene.
///
/// A node's visual extent may be painted across several scene fragments — a
/// frozen-split grid Row strip in both panes (`{tag}_row{id}` ∪ `{tag}_frow{id}`),
/// a tree-grid `row`'s metadata strip + frozen name cell (`{tag}_drow{id}` ∪
/// `{tag}#{id}`). The node's own [`tag`](AccessNode::tag) is the primary
/// fragment; each [`bounds_union_tags`](AccessNode::bounds_union_tags) entry is
/// an *additional* fragment whose rect unions in. A union fragment absent from
/// this paint scene is skipped (so the field is safe to populate even when the
/// split is inactive), and a node whose primary tag is absent but which has a
/// resolvable union fragment still gets bounds from the union. The single
/// coordinate-translation authority [`rect_for_tag`] is the per-fragment
/// resolver, so the unioned bounds inherit its scroll-offset translation +
/// viewport clipping.
/// R984 — bind the lifted [`pinion_a11y::resolve_access_bounds`] union-bounds
/// policy (the TUI shell's 2nd consumer) to the GUI's
/// [`rect_for_tag`] lookup. `rect_for_tag` lives
/// in `pinion_runtime`, above `pinion_a11y` in the layering, so the resolver is
/// passed in rather than depended on.
fn resolve_access_bounds(paint_scene: &Scene, nodes: &mut [AccessNode]) {
    pinion_a11y::resolve_access_bounds(nodes, |tag| rect_for_tag(paint_scene, tag));
}

/// R979 / R984 §5.40 §2 #7 — build the enriched, bounds-resolved accessibility
/// tree (the node list plus the AT focus target) for this window. The semantic
/// assembly — run [`WidgetView::access_node_for_window`] (single-window `None`
/// forwards to `access_node`), enrich names from `paint_scene`, read
/// `access_focus_target` — is the [`pinion_a11y::build_access_tree`] SSOT the
/// live AccessKit emit ([`ShellCore::collect_access_emit_inputs`]), the
/// `scene/access` RPC dump, AND the TUI shell all share (the two MUST agree, so
/// the assembly lifted to `pinion_a11y` at the TUI 2nd consumer, R984). This
/// GUI wrapper adds the layout-engine pixel bounds. `paint_scene` is `None`
/// only for a never-painted window, where names and bounds stay unresolved.
fn build_access_tree<V: WidgetView>(
    owner: &pinion_core::Owner,
    state: &V::State,
    window_id: Option<&str>,
    focused: Option<&str>,
    paint_scene: Option<&Scene>,
) -> (Vec<AccessNode>, Option<AccessFocus>) {
    let (mut nodes, focus) = pinion_a11y::build_access_tree(
        owner,
        paint_scene,
        || match window_id {
            Some(id) => V::access_node_for_window(id, state, focused),
            None => V::access_node(state, focused),
        },
        || V::access_focus_target(state, focused),
    );
    if let Some(paint) = paint_scene {
        resolve_access_bounds(paint, &mut nodes);
    }
    (nodes, focus)
}

#[cfg(test)]
mod r1006_viewport_seam_tests {
    use super::ShellCore;
    use pinion_core::test_fixtures::EchoButtonFixture;

    /// R1006.1 — the substrate publish wire: a primary-window paint
    /// (`window_key == DEFAULT_WINDOW`) publishes the layout `(w, h)` so a
    /// binding's `use_viewport_size` resolves it, and a secondary-window paint
    /// must NOT clobber the primary's published size. Guards the
    /// `spec_id == "main" == DEFAULT_WINDOW` assumption the gate rests on — a
    /// rename of either would break this without touching the seam unit tests.
    #[test]
    fn primary_paint_publishes_viewport_secondary_does_not_clobber() {
        let mut sc = ShellCore::<EchoButtonFixture>::new();
        // The shell field and the owner-cache slot are one Rc cell, so reading
        // the owner's handle observes the shell's `set_viewport_size` writes.
        let sig = pinion_core::VIEWPORT_SIZE.resolve(sc.core.root_owner());
        assert_eq!(sig.get(), (0, 0), "boot: viewport unknown");

        // Primary path (`window_id == None` -> window_key == DEFAULT_WINDOW).
        let _ = sc.compute_paint_scene(640, 480);
        assert_eq!(
            sig.get(),
            (640, 480),
            "primary paint publishes the viewport"
        );

        // The live winit path uses the explicit primary id; same gate fires.
        let _ = sc.compute_paint_scene_for_window(pinion_runtime::DEFAULT_WINDOW, 800, 600);
        assert_eq!(sig.get(), (800, 600));

        // A secondary-window paint must NOT overwrite the primary's value.
        let _ = sc.compute_paint_scene_for_window("inspector", 100, 100);
        assert_eq!(
            sig.get(),
            (800, 600),
            "secondary-window paint must not clobber the primary viewport"
        );

        // The binding-facing read resolves the same published value.
        assert_eq!(
            sc.core.root_owner().run(pinion_core::use_viewport_size),
            (800, 600),
        );
    }
}

#[cfg(test)]
mod r25_focus_union_tests {
    //! R25.1 §5.39 §5.16 — per-window keyboard-focus enumeration (sprag undock
    //! PR-25). The binding-wide `FocusManager` Tab order is the UNION of every
    //! painted window's focusable tags, not the primary window's alone, so a
    //! torn-off (undock) pane drawn only in its own secondary window is
    //! focusable on click and its `External` (a PTY) receives keyboard input.
    use super::ShellCore;
    use pinion_core::test_fixtures::EchoButtonFixture;

    /// `EchoButtonFixture`'s view (via `ButtonFixture::view`) paints one
    /// `.with_focusable(true)` Container tagged `"test_btn"`, so every painted
    /// window contributes exactly this tag to the focus enumeration.
    const FIXTURE_TAG: &str = "test_btn";

    /// The pre-R25.1 gate enumerated focus from the primary paint ONLY, so a
    /// window painted as a secondary left the Tab order empty and its widget
    /// unfocusable. With the gate removed, a secondary-window paint feeds the
    /// enumeration, so the clicked tag resolves to a focusable target (input
    /// then routes to that pane). This is the sprag-undock acceptance core.
    #[test]
    fn secondary_window_paint_enumerates_focus_with_no_primary_gate() {
        let mut sc = ShellCore::<EchoButtonFixture>::new();
        // `new()` boot-seeds the primary (`DEFAULT_WINDOW`) enumeration; a
        // secondary window has contributed nothing yet.
        assert!(
            sc.window_state("pane-0")
                .and_then(|s| s.focusable_tags.as_ref())
                .is_none(),
            "boot: the secondary window has no focus contribution",
        );

        // Paint the secondary (undock) window. Pre-R25.1 the `DEFAULT_WINDOW`
        // gate dropped this enumeration entirely; now the paint records the
        // window's focusable contribution into its `WindowState`.
        let _ = sc.compute_paint_scene_for_window("pane-0", 64, 48);

        assert_eq!(
            sc.window_state("pane-0")
                .and_then(|s| s.focusable_tags.as_ref()),
            Some(&vec![FIXTURE_TAG.to_owned()]),
            "the secondary-window paint records its focusables (no primary gate)",
        );
        // ...and the union exposes the tag as a focusable click target, so
        // `click_to_focus_for_window` would focus it and route input there.
        assert_eq!(
            sc.focus().resolve_focusable(FIXTURE_TAG),
            Some(FIXTURE_TAG.to_owned()),
            "the secondary pane's tag is a focusable click target",
        );
    }

    /// A single-window binding's enumeration is its window's tags verbatim —
    /// the union over one entry — so the primary path is byte-identical to the
    /// pre-R25.1 primary-only enumeration.
    #[test]
    fn primary_only_enumeration_is_verbatim() {
        let mut sc = ShellCore::<EchoButtonFixture>::new();
        let _ = sc.compute_paint_scene(64, 48);
        assert_eq!(
            sc.focus().tab_order().to_vec(),
            vec![FIXTURE_TAG.to_owned()]
        );
    }

    /// The side-effect-free introspection mirror must NOT enumerate focus (the
    /// R1006 contract, inherited): `compute_paint_scene_pure_*` never reaches
    /// the per-window refresh, so a `scene/*` recompute leaves focus untouched.
    #[test]
    fn pure_mirror_paint_does_not_enumerate_focus() {
        let mut sc = ShellCore::<EchoButtonFixture>::new();
        // A secondary-window run through the side-effect-free mirror must NOT
        // record a per-window focus contribution (the R1006 contract).
        let _ = sc.compute_paint_scene_pure_for_window("pane-0", 64, 48);
        assert!(
            sc.window_state("pane-0")
                .and_then(|s| s.focusable_tags.as_ref())
                .is_none(),
            "the pure paint mirror must not enumerate focus (R1006 contract)",
        );
    }

    /// The union JOINS a secondary window's pane without REPLACING the
    /// primary's docked panes — the exact clobber the pre-R25.1 gate guarded
    /// against. The secondary window contributes ONLY its own tag (as a real
    /// undock window does — it does not draw the primary's docked panes), yet
    /// the primary's focusables and its live focus both survive. Driven through
    /// the seam SSOT (`refresh_window_focusables`) with distinct per-window
    /// tags the shared fixture cannot produce.
    #[test]
    fn secondary_pane_joins_union_without_clobbering_primary() {
        let mut sc = ShellCore::<EchoButtonFixture>::new();

        // Primary window draws two docked panes; focus one (as a click would).
        sc.refresh_window_focusables(
            pinion_runtime::DEFAULT_WINDOW,
            vec!["dock0".to_owned(), "dock1".to_owned()],
        );
        assert!(sc.focus.focus_set("dock0"));

        // A torn-off pane paints in its own secondary window, contributing
        // ONLY its tag — it must join, not replace.
        sc.refresh_window_focusables("pane-2", vec!["torn".to_owned()]);

        assert_eq!(
            sc.focus().tab_order().to_vec(),
            vec!["dock0".to_owned(), "dock1".to_owned(), "torn".to_owned()],
            "secondary pane joins the union primary-first; primary panes survive",
        );
        assert_eq!(
            sc.focus().focused(),
            Some("dock0"),
            "primary focus is not dropped — its tag is still painted",
        );
        assert!(
            sc.focus.focus_set("torn"),
            "the torn pane is a focusable target, so a click on it can focus it",
        );
    }

    /// Closing a window drops its focusable contribution from the union, and
    /// the §5.39 stale-focus guard drops focus that pointed at one of its
    /// now-unpainted widgets.
    #[test]
    fn closing_a_window_drops_its_focusables_and_stale_focus() {
        let mut sc = ShellCore::<EchoButtonFixture>::new();
        sc.refresh_window_focusables(pinion_runtime::DEFAULT_WINDOW, vec!["dock0".to_owned()]);
        sc.refresh_window_focusables("pane-2", vec!["torn".to_owned()]);
        assert!(sc.focus.focus_set("torn"));

        assert!(
            sc.remove_window("pane-2"),
            "the window held shell-side state"
        );

        assert_eq!(
            sc.focus().tab_order().to_vec(),
            vec!["dock0".to_owned()],
            "the closed window's tag leaves the Tab order",
        );
        assert_eq!(
            sc.focus().focused(),
            None,
            "focus on the now-unpainted pane is dropped (§5.39 stale guard)",
        );
    }
}

#[cfg(test)]
mod r26_undock_focus_follow_tests {
    //! R26 §5.39 §5.16 — undock focus-FOLLOW (sprag PR-26). PR-25 made a
    //! torn-off pane focusable on CLICK once its own window painted; R26 makes
    //! focus AUTO-FOLLOW across the window-creation race. A pane the binding
    //! declares in `windows_signal` (the undock) contributes its focusables —
    //! derived from the pure `view_for_window` — BEFORE its OS window first
    //! paints, so a focus the binding placed on it is not dropped in the
    //! `reconcile_windows` gap and no extra click is needed.
    use super::ShellCore;
    use crate::test_fixtures::TestRenderer;
    use crate::{SizeStrategy, WidgetView, WindowSpec};
    use pinion_a11y::WidgetA11y;
    use pinion_core::external::External;
    use pinion_core::scene::ContainerNode;
    use pinion_core::style::LayoutStyle;
    use pinion_core::test_fixtures::ButtonFixture;
    use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
    use pinion_core::{Frame, Owner, Scene, Signal, WidgetCore};
    use std::rc::Rc;

    const MAIN_TAG: &str = "main_btn";
    /// The torn-off (undock) secondary window id; declared in `windows_signal`.
    const TORN_WINDOW: &str = "float.left";
    const TORN_TAG: &str = "torn_pane";
    /// The inline editor each window paints only while [`editor_open`] is set —
    /// the R1020 "focus a widget on the frame it appears" shape, one per window.
    const MAIN_EDITOR_TAG: &str = "main_editor";
    const TORN_EDITOR_TAG: &str = "torn_editor";
    /// A pane a reducer can move BETWEEN the two windows — drawn by exactly one
    /// of them per frame, per the dock model.
    const ROAMING_TAG: &str = "roaming_pane";

    /// A bare focusable Tab stop (`collect_focusable_tags` reads tag +
    /// `LayoutStyle::focusable`).
    fn focusable(tag: &'static str) -> Scene {
        Scene::Container(
            ContainerNode::new(Vec::new())
                .with_tag(tag)
                .with_layout(LayoutStyle::new().with_focusable(true)),
        )
    }

    /// The reducer-owned gate that makes each window's inline editor paintable.
    /// `Owner::cache`-memoised on the same key the view reads, so a test flipping
    /// it under `root_owner` and the view fn resolve the POINTER-EQUAL signal
    /// (the R683 identity contract `windows_signal` relies on).
    fn editor_open() -> Rc<Signal<bool>> {
        Owner::current()
            .expect("view fn and test both run under root_owner")
            .cache::<Signal<bool>, _>("dock_follow_editor", || Signal::new(false))
    }

    /// Which window currently draws [`ROAMING_TAG`] — the undock model's one
    /// invariant made movable: a pane is drawn in exactly ONE window per frame,
    /// and a reducer can move it. `false` = the primary draws it.
    fn roamer_in_torn() -> Rc<Signal<bool>> {
        Owner::current()
            .expect("view fn and test both run under root_owner")
            .cache::<Signal<bool>, _>("dock_follow_roamer", || Signal::new(false))
    }

    /// One window's scene: its permanent focusable, the inline editor while the
    /// gate is open, and the roaming pane while this window hosts it.
    fn pane_with_editor(pane: &'static str, editor: &'static str, hosts_roamer: bool) -> Scene {
        let mut kids = vec![focusable(pane)];
        if editor_open().get() {
            kids.push(focusable(editor));
        }
        if hosts_roamer {
            kids.push(focusable(ROAMING_TAG));
        }
        Scene::Container(ContainerNode::new(kids))
    }

    /// A dock binding: the primary paints one focusable (`main_btn`); ONE
    /// declared secondary window (`float.left`, the torn-off pane) whose
    /// `view_for_window` paints the focusable `torn_pane`. The fixture DECLARES
    /// the secondary in `windows_signal` but each test chooses when (if ever) to
    /// paint it — modelling the undock `reconcile_windows` gap.
    struct DockFollowFixture;

    impl WidgetCore for DockFollowFixture {
        type State = ButtonState;
        type Event = ButtonEvent;

        fn create_external() -> Box<dyn External> {
            Box::new(ButtonExternal::new())
        }
        fn tag() -> &'static str {
            MAIN_TAG
        }
        fn read_state(scene: &Scene) -> Self::State {
            <ButtonFixture as WidgetCore>::read_state(scene)
        }
        fn view(_state: Self::State, _frame: &Frame) -> Scene {
            pane_with_editor(MAIN_TAG, MAIN_EDITOR_TAG, !roamer_in_torn().get())
        }
        fn event_name(event: Self::Event) -> &'static str {
            <ButtonFixture as WidgetCore>::event_name(event)
        }
        fn title() -> &'static str {
            "DockFollow"
        }
    }

    impl WidgetA11y for DockFollowFixture {}

    impl WidgetView for DockFollowFixture {
        type Renderer = TestRenderer;

        fn initial_size_strategy() -> SizeStrategy {
            SizeStrategy::Fixed {
                width: 8,
                height: 8,
            }
        }

        fn windows_signal() -> Option<Rc<Signal<Vec<WindowSpec>>>> {
            // Memoise via `Owner::cache` so every shell-side call returns the
            // same handle (the R683 identity-stability contract).
            Owner::current().map(|owner| {
                owner.cache::<Signal<Vec<WindowSpec>>, _>("dock_follow_windows", || {
                    Signal::new(vec![
                        WindowSpec::main(
                            "DockFollow",
                            SizeStrategy::Fixed {
                                width: 8,
                                height: 8,
                            },
                        ),
                        WindowSpec::new(
                            TORN_WINDOW,
                            "Torn",
                            SizeStrategy::Fixed {
                                width: 8,
                                height: 8,
                            },
                        ),
                    ])
                })
            })
        }

        fn view_for_window(window_id: &str, state: Self::State, frame: &Frame) -> Scene {
            if window_id == TORN_WINDOW {
                pane_with_editor(TORN_TAG, TORN_EDITOR_TAG, roamer_in_torn().get())
            } else {
                Self::view(state, frame)
            }
        }
    }

    /// The core R26 seam: a window DECLARED in `windows_signal` but never
    /// painted still contributes its focusables (derived from `view_for_window`)
    /// to the enumeration. `new()` boot-seeds through the same union, so the
    /// torn pane is enumerable from boot — before any secondary paint.
    #[test]
    fn declared_but_unpainted_window_contributes_derived_focusables() {
        let sc = ShellCore::<DockFollowFixture>::new();
        let order = sc.focus().tab_order().to_vec();
        assert!(
            order.contains(&MAIN_TAG.to_owned()),
            "the primary focusable is enumerated: {order:?}",
        );
        assert!(
            order.contains(&TORN_TAG.to_owned()),
            "the declared-but-unpainted torn pane contributes its DERIVED focusable: {order:?}",
        );
    }

    /// The undock focus-follow guarantee: the binding places focus on the torn
    /// pane (as `apply_focus_request` does after `toggle_pane_floating`), then
    /// the PRIMARY repaints WITHOUT the pane (it left the dock). Pre-R26 the
    /// union was painted-only, so this repaint dropped focus to `None` (the dead
    /// undock window). With R26 the declared topology keeps the pane enumerated,
    /// so focus follows it into its new window with no extra click.
    #[test]
    fn focus_on_torn_pane_survives_the_primary_repaint_gap() {
        let mut sc = ShellCore::<DockFollowFixture>::new();
        assert!(
            sc.focus.focus_set(TORN_TAG),
            "the torn pane is a focusable target before its window paints",
        );

        // The primary repaints, contributing ONLY its own focusable — the torn
        // pane has left the dock and its secondary window has not painted yet.
        sc.refresh_window_focusables(pinion_runtime::DEFAULT_WINDOW, vec![MAIN_TAG.to_owned()]);

        assert_eq!(
            sc.focus().focused(),
            Some(TORN_TAG),
            "focus on the torn pane is NOT dropped — the declared topology keeps it enumerated",
        );
    }

    /// The derive -> harvest transition with no double count: the torn tag is
    /// present DERIVED before its window paints, and still present exactly ONCE
    /// after it paints (now harvested). The torn window is then in BOTH the
    /// declared set and the painted cache, so this also pins the declared∪painted
    /// dedup (`union_focusable_tags` folds it once via first-occurrence).
    #[test]
    fn torn_tag_survives_derive_to_harvest_transition_without_double_count() {
        let mut sc = ShellCore::<DockFollowFixture>::new();
        // Pre-paint: present via derivation (declared-but-unpainted).
        assert_eq!(
            sc.focus()
                .tab_order()
                .iter()
                .filter(|t| t.as_str() == TORN_TAG)
                .count(),
            1,
            "derived torn tag present exactly once before its window paints",
        );

        // The torn window first-paints — its harvested contribution now also
        // covers the tag (declared AND painted).
        sc.refresh_window_focusables(TORN_WINDOW, vec![TORN_TAG.to_owned()]);

        let order = sc.focus().tab_order().to_vec();
        assert_eq!(
            order.iter().filter(|t| t.as_str() == TORN_TAG).count(),
            1,
            "after paint the torn tag still appears once — harvest supersedes \
             derivation, declared∪painted deduped: {order:?}",
        );
    }

    /// R1020's guarantee, stated for the primary window — the CONTROL for the
    /// secondary-window test below. A reducer opens an inline editor and names
    /// it in the same dispatch; the paint refresh has not run, so the request
    /// misses and the re-derive is what makes it land.
    #[test]
    fn a_request_reaches_a_node_this_dispatch_made_paintable_in_the_primary() {
        let mut sc = ShellCore::<DockFollowFixture>::new();
        sc.refresh_window_focusables(pinion_runtime::DEFAULT_WINDOW, vec![MAIN_TAG.to_owned()]);
        sc.refresh_window_focusables(TORN_WINDOW, vec![TORN_TAG.to_owned()]);

        sc.core.root_owner().run(|| editor_open().set(true));
        pinion_core::focus_request::request(MAIN_EDITOR_TAG);
        sc.drain_focus_mailboxes();

        assert_eq!(
            sc.focus().focused(),
            Some(MAIN_EDITOR_TAG),
            "the primary's just-appeared editor is focusable on its own frame",
        );
    }

    /// R1463 §5.39 §5.16 — the SAME dispatch shape inside a PAINTED secondary
    /// window. Pre-R1463 the re-derive ran `V::view` only, refreshing the
    /// primary's contribution; a painted secondary short-circuits
    /// [`ShellCore::window_focusables`] to its harvested cache, so the tag the
    /// reducer just made paintable there was invisible to the retry and the
    /// one-shot request was consumed and silently dropped. Same binding, same
    /// intent, different window, different outcome — the divergence this closes.
    #[test]
    fn a_request_reaches_a_node_this_dispatch_made_paintable_in_a_secondary_window() {
        let mut sc = ShellCore::<DockFollowFixture>::new();
        sc.refresh_window_focusables(pinion_runtime::DEFAULT_WINDOW, vec![MAIN_TAG.to_owned()]);
        // The torn window has PAINTED — this is what makes its enumeration a
        // cache rather than a per-fold derivation.
        sc.refresh_window_focusables(TORN_WINDOW, vec![TORN_TAG.to_owned()]);
        assert!(
            !sc.focus().tab_order().iter().any(|t| t == TORN_EDITOR_TAG),
            "precondition: the editor is not enumerated before the reducer runs",
        );

        sc.core.root_owner().run(|| editor_open().set(true));
        pinion_core::focus_request::request(TORN_EDITOR_TAG);
        sc.drain_focus_mailboxes();

        assert_eq!(
            sc.focus().focused(),
            Some(TORN_EDITOR_TAG),
            "the torn pane's just-appeared editor is focusable on its own \
             frame, exactly as the primary's is",
        );
    }

    /// R1463 — the refresh folds ONCE, from the complete set of refreshed
    /// contributions. A reducer that moves a pane between windows takes its tag
    /// out of one window's scene and puts it in another's; if each window's
    /// contribution were folded as it was rewritten, the union between the two
    /// writes would contain the tag in NEITHER window (the primary already
    /// rewritten without it, the secondary still holding its stale harvest) and
    /// [`FocusManager::update_focusable_tags`] would drop a focus that never
    /// left the binding. The request here MISSES deliberately: a request that
    /// landed would mask the drop by moving focus itself.
    #[test]
    fn a_pane_moving_between_windows_keeps_its_focus_across_the_refresh() {
        let mut sc = ShellCore::<DockFollowFixture>::new();
        sc.refresh_window_focusables(
            pinion_runtime::DEFAULT_WINDOW,
            vec![MAIN_TAG.to_owned(), ROAMING_TAG.to_owned()],
        );
        sc.refresh_window_focusables(TORN_WINDOW, vec![TORN_TAG.to_owned()]);
        assert!(
            sc.focus.focus_set(ROAMING_TAG),
            "the roaming pane holds focus while the primary draws it",
        );

        // The reducer tears the pane off into the secondary window, then some
        // other request misses (an unknown tag), driving the re-derive.
        sc.core.root_owner().run(|| roamer_in_torn().set(true));
        pinion_core::focus_request::request("no_such_tag");
        sc.drain_focus_mailboxes();

        assert_eq!(
            sc.focus().focused(),
            Some(ROAMING_TAG),
            "focus follows the pane into its new window — the tag was in one \
             window's contribution or the other's at every point",
        );
        assert!(
            sc.focus().tab_order().iter().any(|t| t == ROAMING_TAG),
            "and it is enumerated exactly where it now paints",
        );
    }

    /// R1464 §5.16 §5.39 — the steady state is FREE. A request naming a tag the
    /// enumeration already holds is satisfied by the first `focus_set` and never
    /// reaches the re-derive, so neither counter moves.
    ///
    /// The control for the two tests below: without it, "a miss costs two view
    /// runs" would be equally consistent with "every request costs two".
    #[test]
    fn a_focus_request_that_hits_runs_no_view_at_all() {
        let mut sc = ShellCore::<DockFollowFixture>::new();
        sc.refresh_window_focusables(pinion_runtime::DEFAULT_WINDOW, vec![MAIN_TAG.to_owned()]);
        sc.refresh_window_focusables(TORN_WINDOW, vec![TORN_TAG.to_owned()]);

        let before = sc.focus_work.get();
        pinion_core::focus_request::request(MAIN_TAG);
        sc.drain_focus_mailboxes();

        assert_eq!(
            sc.focus().focused(),
            Some(MAIN_TAG),
            "precondition: the request landed, so this measured a HIT",
        );
        assert_eq!(
            sc.focus_work.get().retries,
            before.retries,
            "a hit is not a retry",
        );
        assert_eq!(
            sc.focus_work.get().derivations,
            before.derivations,
            "and it runs no view: the enumeration already answered",
        );
    }

    /// R1464 §5.16 §5.39 — R1463's cost claim, measured. Its doc says "one view
    /// run per painted window, paid on a miss"; with TWO painted windows a miss
    /// must cost exactly two derivations and one retry.
    ///
    /// This is what makes the R1463 widening observable from outside: pre-R1463
    /// the same dispatch re-derived the primary alone, and the only difference
    /// on any surface was a focus that silently failed to land in a secondary
    /// window. The delta is now a number, on both the test seam and
    /// `scene/frame_timings`.
    #[test]
    fn a_focus_miss_costs_one_view_run_per_painted_window() {
        let mut sc = ShellCore::<DockFollowFixture>::new();
        sc.refresh_window_focusables(pinion_runtime::DEFAULT_WINDOW, vec![MAIN_TAG.to_owned()]);
        sc.refresh_window_focusables(TORN_WINDOW, vec![TORN_TAG.to_owned()]);

        let before = sc.focus_work.get();
        sc.core.root_owner().run(|| editor_open().set(true));
        pinion_core::focus_request::request(TORN_EDITOR_TAG);
        sc.drain_focus_mailboxes();

        let after = sc.focus_work.get();
        assert_eq!(
            after.retries - before.retries,
            1,
            "one request missed, so one retry",
        );
        assert_eq!(
            after.derivations - before.derivations,
            2,
            "and the retry re-derived BOTH painted windows — the R1463 bound. \
             `1` here is the pre-R1463 primary-only refresh; a number above `2` \
             would mean the fold re-derived a window whose cache was just written",
        );
        assert_eq!(
            sc.focus().focused(),
            Some(TORN_EDITOR_TAG),
            "and the work bought the outcome it claims to",
        );
    }

    /// R1464 §5.16 §5.39 — the OTHER half of the count, and the reason it is not
    /// simply `retries * window_count`.
    ///
    /// A window the binding DECLARES but has not painted has no harvested cache
    /// to answer from, so R26 derives it afresh on every fold — with no focus
    /// request involved at all. A `derivations` count that climbs while nothing
    /// is being focused is that transient, and it ends at that window's first
    /// paint. Pinning it here keeps a later reader from diagnosing a leak.
    #[test]
    fn an_unpainted_declared_window_is_re_derived_on_every_fold() {
        let mut sc = ShellCore::<DockFollowFixture>::new();

        // Only the primary has painted; TORN_WINDOW is declared, unpainted.
        let before = sc.focus_work.get();
        sc.refresh_window_focusables(pinion_runtime::DEFAULT_WINDOW, vec![MAIN_TAG.to_owned()]);
        let one_fold = sc.focus_work.get();
        assert_eq!(
            one_fold.derivations - before.derivations,
            1,
            "the fold derived the declared-but-unpainted window, and only it: \
             the primary answered from the cache this very call wrote",
        );
        assert_eq!(
            one_fold.retries, before.retries,
            "no focus request was made — this cost is not a retry's",
        );

        // The torn window first-paints. Its contribution is now a cache too,
        // and the transient is over.
        sc.refresh_window_focusables(TORN_WINDOW, vec![TORN_TAG.to_owned()]);
        let painted = sc.focus_work.get();
        sc.refresh_window_focusables(pinion_runtime::DEFAULT_WINDOW, vec![MAIN_TAG.to_owned()]);
        assert_eq!(
            sc.focus_work.get().derivations,
            painted.derivations,
            "with every declared window painted, a fold derives nothing",
        );
    }

    /// The §5.39 modal guard contains the derived tag: a torn pane the declared
    /// topology enumerates is NOT focusable while a modal trap is up — `focus_set`
    /// resolves against the modal members (`active_order`), and the derived tag is
    /// not a member. R26 adds tags to `tab_order`; it does not widen the modal
    /// trap. (The guard itself lives in `FocusManager`; this pins the interaction
    /// with R26's enumeration.)
    #[test]
    fn derived_torn_tag_is_not_focusable_while_a_modal_is_active() {
        let mut sc = ShellCore::<DockFollowFixture>::new();
        // The torn tag is enumerated (derived).
        assert!(
            sc.focus()
                .tab_order()
                .iter()
                .any(|t| t.as_str() == TORN_TAG),
        );

        // Open a modal trap over the primary control only.
        sc.focus.push_modal_scope(vec![MAIN_TAG.to_owned()]);
        assert_eq!(
            sc.focus().focused(),
            Some(MAIN_TAG),
            "the modal auto-focuses its first member",
        );

        // The derived torn tag, though in tab_order, is not a modal member:
        // focus_set must reject it and the modal focus must not move.
        assert!(
            !sc.focus.focus_set(TORN_TAG),
            "a non-member derived tag cannot steal focus from the modal trap",
        );
        assert_eq!(
            sc.focus().focused(),
            Some(MAIN_TAG),
            "modal focus unchanged"
        );
    }
}

#[cfg(test)]
mod r1113_drag_image_producer_tests {
    //! R1113 §5.51 §5.33 §2 #7 — END-TO-END proof that the shell PRODUCER
    //! injects the drag-image follower during a real drag. Drives the SAME
    //! shell methods `scene/drag` drains into (`cursor_moved` → `mouse_pressed`
    //! → `cursor_moved` → `mouse_released`), but produces the paint scene
    //! BETWEEN the press-move and the release — the held mid-drag state the
    //! atomic `scene/drag` arc can never snapshot (there is no press-and-hold
    //! RPC peer), so this is the definitive headless verification of the live
    //! path.
    use super::ShellCore;
    use crate::test_fixtures::TestRenderer;
    use crate::{SizeStrategy, WidgetView};
    use pinion_a11y::WidgetA11y;
    use pinion_core::external::{
        Backend, BackendFallback, BackendSupport, DragPayload, External, IntrospectValue,
        RepaintOwner, ThreadOwnership,
    };
    use pinion_core::scene::{ContainerNode, Rect, Scene};
    use pinion_core::style::LayoutStyle;
    use pinion_core::widgets::button::{ButtonEvent, ButtonState};
    use pinion_core::{Frame, WidgetCore};
    use pinion_runtime::PointerId;

    const SRC_TAG: &str = "drag_src";
    const LABEL: &str = "panel-A";

    /// A minimal R742 drag source whose `begin_drag` always emits a TEXT
    /// payload (the dock-panel shape), so a press over it opens a labelled drag
    /// session the shell projects into the drag-image follower.
    #[derive(Debug)]
    struct TextDragSource;
    impl External for TextDragSource {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn begin_drag(&self) -> Option<DragPayload> {
            Some(DragPayload {
                kind: std::borrow::Cow::Borrowed("test-drag"),
                value: IntrospectValue::Text(LABEL.to_string()),
            })
        }
    }

    /// A binding whose primary widget is the text drag source, painted as a
    /// window-filling tagged rect so a press anywhere hit-tests to it.
    struct DragImageFixture;
    impl WidgetCore for DragImageFixture {
        type State = ButtonState;
        type Event = ButtonEvent;
        fn create_external() -> Box<dyn External> {
            Box::new(TextDragSource)
        }
        fn tag() -> &'static str {
            SRC_TAG
        }
        fn read_state(_scene: &Scene) -> Self::State {
            ButtonState::Idle
        }
        fn view(_state: Self::State, _frame: &Frame) -> Scene {
            let mut c = ContainerNode::new(Vec::new());
            c.rect = Rect::new(0, 0, 200, 200);
            c.tag = Some(std::borrow::Cow::Borrowed(SRC_TAG));
            c.layout = LayoutStyle::new();
            Scene::Container(c)
        }
        fn event_name(_event: Self::Event) -> &'static str {
            "__internal__"
        }
        fn title() -> &'static str {
            "DragImage"
        }
    }
    impl WidgetA11y for DragImageFixture {}
    impl WidgetView for DragImageFixture {
        type Renderer = TestRenderer;
        fn initial_size_strategy() -> SizeStrategy {
            SizeStrategy::Fixed {
                width: 200,
                height: 200,
            }
        }
    }

    fn has_tag(scene: &Scene, tag: &str) -> bool {
        if scene.tag() == Some(tag) {
            return true;
        }
        match scene {
            Scene::Container(c) => c.children.iter().any(|ch| has_tag(ch, tag)),
            Scene::Scroll(s) => has_tag(&s.content, tag),
            _ => false,
        }
    }

    #[test]
    fn producer_injects_drag_image_during_a_drag_and_clears_on_release() {
        let mut sc = ShellCore::<DragImageFixture>::new();
        // Produce + publish so the router has a paint scene to hit-test.
        let boot = sc.compute_paint_scene(200, 200);
        sc.finalize_frame(boot);
        // Idle: no follower.
        assert!(
            !has_tag(
                &sc.compute_paint_scene(200, 200),
                pinion_overlay::DRAG_IMAGE_TAG
            ),
            "no drag in flight = no follower in the produced scene",
        );
        // Press over the source, then drag well past the click→drag threshold.
        sc.cursor_moved(PointerId::MOUSE, 10.0, 10.0);
        sc.mouse_pressed(PointerId::MOUSE);
        sc.cursor_moved(PointerId::MOUSE, 120.0, 90.0);
        // Mid-drag: the PRODUCER injects the follower (this is what the user
        // sees follow the cursor — proven headless, not asked).
        let mid = sc.compute_paint_scene(200, 200);
        assert!(
            has_tag(&mid, pinion_overlay::DRAG_IMAGE_TAG),
            "the shell producer injects the drag-image follower during a live drag",
        );
        // Release: the session ends → the follower clears.
        sc.mouse_released(PointerId::MOUSE);
        assert!(
            !has_tag(
                &sc.compute_paint_scene(200, 200),
                pinion_overlay::DRAG_IMAGE_TAG
            ),
            "after release the follower is gone",
        );
    }

    /// R1147 §5.51 §5.16 — when the shell's cross-desktop drag PREVIEW window is
    /// showing the drag (`set_desktop_drag_preview_active(true)`), the in-window
    /// follower is SUPPRESSED so exactly one chip shows (the desktop preview,
    /// which roams the whole desktop) — never two. Toggling the flag back off
    /// restores the in-window follower (the headless / introspection chip).
    #[test]
    fn desktop_preview_active_suppresses_the_in_window_follower() {
        let mut sc = ShellCore::<DragImageFixture>::new();
        let boot = sc.compute_paint_scene(200, 200);
        sc.finalize_frame(boot);
        // Open a real drag.
        sc.cursor_moved(PointerId::MOUSE, 10.0, 10.0);
        sc.mouse_pressed(PointerId::MOUSE);
        sc.cursor_moved(PointerId::MOUSE, 120.0, 90.0);
        // Default (flag false): the in-window follower shows.
        assert!(
            has_tag(
                &sc.compute_paint_scene(200, 200),
                pinion_overlay::DRAG_IMAGE_TAG
            ),
            "default keeps the in-window follower (headless / introspection chip)",
        );
        // Desktop preview takes over: the in-window follower is suppressed.
        sc.set_desktop_drag_preview_active(true);
        assert!(
            !has_tag(
                &sc.compute_paint_scene(200, 200),
                pinion_overlay::DRAG_IMAGE_TAG
            ),
            "the desktop preview owns the chip, so the in-window one is suppressed",
        );
        // Releasing the takeover restores the in-window follower.
        sc.set_desktop_drag_preview_active(false);
        assert!(
            has_tag(
                &sc.compute_paint_scene(200, 200),
                pinion_overlay::DRAG_IMAGE_TAG
            ),
            "clearing the flag restores the in-window follower",
        );
    }

    /// R1138 §5.49 §2 #2 — the phased `scene/drag` peer: a `begin` slice
    /// drained through the same path the RPC feeds (`DeferredInput::Drag`)
    /// presses + marches but HOLDS, so the drag-image follower stays in the
    /// produced scene ACROSS the drain boundary — the held mid-drag an AI can
    /// `scene/snapshot`. A following `end` slice (no press) releases, clearing
    /// it. This is the headless proof that the held-drag primitive keeps the
    /// router session open between RPC calls (the press-and-hold gap R1114
    /// flagged), without the shell holding any extra state.
    #[test]
    fn r1138_begin_phase_holds_the_follower_until_an_end_phase_releases() {
        use pinion_rpc::{DeferredInput, DragButton, DragPhase};
        use pinion_runtime::DEFAULT_WINDOW;

        let mut sc = ShellCore::<DragImageFixture>::new();
        let boot = sc.compute_paint_scene(200, 200);
        sc.finalize_frame(boot);

        // A `begin` drag: press over the source at (10, 10), march well past
        // the click→drag threshold to (120, 90), then HOLD (no release).
        sc.drain_deferred_inputs_for_window(
            Some(DEFAULT_WINDOW),
            &[DeferredInput::Drag {
                from_x: 10.0,
                from_y: 10.0,
                to_x: 120.0,
                to_y: 90.0,
                steps: 4,
                button: DragButton::Left,
                phase: DragPhase::Begin,
            }],
        );
        // The session is HELD across the drain: a fresh paint still carries
        // the follower (the mid-drag state an `scene/snapshot` would observe).
        assert!(
            has_tag(
                &sc.compute_paint_scene(200, 200),
                pinion_overlay::DRAG_IMAGE_TAG
            ),
            "a begin phase holds the drag open — the follower persists past the drain",
        );

        // A `move` slice re-aims the held drag without releasing it.
        sc.drain_deferred_inputs_for_window(
            Some(DEFAULT_WINDOW),
            &[DeferredInput::Drag {
                from_x: 120.0,
                from_y: 90.0,
                to_x: 60.0,
                to_y: 150.0,
                steps: 2,
                button: DragButton::Left,
                phase: DragPhase::Move,
            }],
        );
        assert!(
            has_tag(
                &sc.compute_paint_scene(200, 200),
                pinion_overlay::DRAG_IMAGE_TAG
            ),
            "a move phase keeps the held drag open",
        );

        // An `end` slice (no press) releases → the session ends, follower gone.
        sc.drain_deferred_inputs_for_window(
            Some(DEFAULT_WINDOW),
            &[DeferredInput::Drag {
                from_x: 60.0,
                from_y: 150.0,
                to_x: 60.0,
                to_y: 150.0,
                steps: 0,
                button: DragButton::Left,
                phase: DragPhase::End,
            }],
        );
        assert!(
            !has_tag(
                &sc.compute_paint_scene(200, 200),
                pinion_overlay::DRAG_IMAGE_TAG
            ),
            "an end phase releases the held drag — the follower clears",
        );
    }
}

#[cfg(test)]
mod r1104_cross_window_exclusion_tests {
    //! R1104 §5.51 §2 #7 PR-33 — fast-suite coverage for the shell's
    //! cross-window drop SOURCE-EXCLUSION (the R1102.1 review SHOULD-FIX:
    //! `cross_window_drop_at` / `resolve_cross_window_live` were exercised only
    //! by the CI demo `r1102_cross_window_redock`, which a local fast run skips).
    //!
    //! Two POSITIONED declared windows — `main` at the desktop origin (WM-placed)
    //! and a floater at `(800, 100)` — each given an injected post-layout paint
    //! scene holding one opted-in drop target, let a headless `#[test]` drive the
    //! exclusion both ways without a window:
    //!   * the source window is skipped (a same-window drop must not surface as a
    //!     cross-window drop), and
    //!   * a cursor that escaped the source INTO another window resolves THAT
    //!     window — including the executable floater→main direction R1103 drives.
    use super::ShellCore;
    use crate::test_fixtures::TestRenderer;
    use crate::{SizeStrategy, WidgetView, WindowSpec};
    use pinion_a11y::WidgetA11y;
    use pinion_core::external::External;
    use pinion_core::scene::{ContainerNode, Rect};
    use pinion_core::style::LayoutStyle;
    use pinion_core::test_fixtures::ButtonFixture;
    use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
    use pinion_core::{Frame, Owner, Scene, Signal, WidgetCore};
    use std::rc::Rc;

    const FLOAT_WINDOW: &str = "float.right";
    const FLOAT_X: i32 = 800;
    const FLOAT_Y: i32 = 100;

    /// A one-window post-layout paint scene: a single opted-in drop-target panel
    /// tagged `tag` at the window-local `rect`, inside a root filling the window.
    /// Mirrors the runtime `window_with_drop_panel` helper so the shell test
    /// feeds `resolve_cross_window_drop` the same shape a real paint produces.
    fn drop_panel_scene(tag: &str, rect: Rect) -> Scene {
        let mut panel = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag(tag.to_string())
                .with_layout(LayoutStyle::new().with_drop_target(true)),
        );
        if let Scene::Container(c) = &mut panel {
            c.rect = rect;
        }
        let mut root = Scene::Container(ContainerNode::new(vec![panel]));
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, 1000, 800);
        }
        root
    }

    /// Two POSITIONED declared windows; the tests inject each window's paint scene
    /// directly (the dock panels), so `view_for_window` is only the declaration
    /// stub `windows_signal` + `declared_window_specs` need.
    struct CrossWindowFixture;

    impl WidgetCore for CrossWindowFixture {
        type State = ButtonState;
        type Event = ButtonEvent;

        fn create_external() -> Box<dyn External> {
            Box::new(ButtonExternal::new())
        }
        fn tag() -> &'static str {
            "main_btn"
        }
        fn read_state(scene: &Scene) -> Self::State {
            <ButtonFixture as WidgetCore>::read_state(scene)
        }
        fn view(_state: Self::State, _frame: &Frame) -> Scene {
            Scene::Container(ContainerNode::new(vec![]).with_tag("main_btn"))
        }
        fn event_name(event: Self::Event) -> &'static str {
            <ButtonFixture as WidgetCore>::event_name(event)
        }
        fn title() -> &'static str {
            "CrossWindow"
        }
    }

    impl WidgetA11y for CrossWindowFixture {}

    impl WidgetView for CrossWindowFixture {
        type Renderer = TestRenderer;

        fn initial_size_strategy() -> SizeStrategy {
            SizeStrategy::Fixed {
                width: 1000,
                height: 800,
            }
        }

        fn windows_signal() -> Option<Rc<Signal<Vec<WindowSpec>>>> {
            Owner::current().map(|owner| {
                owner.cache::<Signal<Vec<WindowSpec>>, _>("cross_window_windows", || {
                    Signal::new(vec![
                        WindowSpec::main(
                            "CrossWindow",
                            SizeStrategy::Fixed {
                                width: 1000,
                                height: 800,
                            },
                        ),
                        WindowSpec::new(
                            FLOAT_WINDOW,
                            "Float",
                            SizeStrategy::Fixed {
                                width: 200,
                                height: 200,
                            },
                        )
                        .with_position(FLOAT_X, FLOAT_Y),
                    ])
                })
            })
        }

        fn view_for_window(_window_id: &str, state: Self::State, frame: &Frame) -> Scene {
            Self::view(state, frame)
        }
    }

    /// A 2-window shell with each window's dock panel injected as its paint scene:
    /// `main`'s at local `(500, 400, 100, 100)`, the floater's at `(10, 10, 80, 80)`.
    fn shell_with_two_dock_windows() -> ShellCore<CrossWindowFixture> {
        let mut sc = ShellCore::<CrossWindowFixture>::new();
        sc.core.set_paint_scene_for_window(
            "main",
            drop_panel_scene("main_dock", Rect::new(500, 400, 100, 100)),
        );
        sc.core.set_paint_scene_for_window(
            FLOAT_WINDOW,
            drop_panel_scene("torn", Rect::new(10, 10, 80, 80)),
        );
        sc
    }

    #[test]
    fn cross_window_drop_excludes_the_source_window() {
        let sc = shell_with_two_dock_windows();
        // (550, 450) is over main's dock panel (main is at the desktop origin).
        let own = sc
            .cross_window_drop_at((550.0, 450.0), None, false)
            .expect("the abs cursor resolves main's dock when nothing is excluded");
        assert_eq!(own.window, "main");
        assert_eq!(own.point.tag, "main_dock");
        // Excluding the source while the cursor is over ONLY the source resolves
        // nothing — a same-window drop must not surface as a cross-window drop.
        assert!(
            sc.cross_window_drop_at((550.0, 450.0), Some("main"), false)
                .is_none(),
            "the source window is excluded and the cursor is over no OTHER window",
        );
    }

    #[test]
    fn cross_window_drop_resolves_the_other_window_the_cursor_escaped_into() {
        let sc = shell_with_two_dock_windows();
        // (840, 140) is floater-local (40, 40) — over the floater's `torn` panel,
        // and over no main panel. Excluding the source (main) resolves the floater.
        let into = sc
            .cross_window_drop_at((840.0, 140.0), Some("main"), false)
            .expect("the escaped cursor resolves the floater");
        assert_eq!(into.window, FLOAT_WINDOW);
        assert_eq!(into.point.tag, "torn");
    }

    #[test]
    fn resolve_cross_window_live_maps_source_local_to_abs_then_excludes_source() {
        let sc = shell_with_two_dock_windows();
        // Source = the floater (origin 800, 100). A captured drag whose local
        // cursor is (-250, 350) maps to abs (550, 450) — over MAIN's dock. This is
        // the executable floater→main redock direction R1103 drives.
        let back = sc
            .resolve_cross_window_live(FLOAT_WINDOW, (-250.0, 350.0))
            .expect("the floater's drag escaped into main's dock");
        assert_eq!(back.window, "main");
        assert_eq!(back.point.tag, "main_dock");
        // Source = main. A local cursor at (840, 140) escapes into the floater.
        let out = sc
            .resolve_cross_window_live("main", (840.0, 140.0))
            .expect("main's drag escaped into the floater");
        assert_eq!(out.window, FLOAT_WINDOW);
        assert_eq!(out.point.tag, "torn");
    }

    #[test]
    fn resolve_cross_window_live_is_none_when_the_cursor_stays_over_the_source() {
        let sc = shell_with_two_dock_windows();
        // Source = main, local (550, 450) over main's OWN dock: the source is
        // excluded, the cursor reaches no other window → None (a same-window drag).
        assert!(
            sc.resolve_cross_window_live("main", (550.0, 450.0))
                .is_none(),
            "a drag that stays over the source's own dock is not cross-window",
        );
    }

    #[test]
    fn live_cross_window_drop_uses_stamped_actual_origin_not_declared() {
        // R1148 §5.51 — the user-found "좌표 안 맞아" bug: a WM-placed `"main"` has
        // declared position `None` → `(0,0)`, but the WM placed it at a real
        // offset. The LIVE path must hit-test against the shell-stamped ACTUAL
        // origin, NOT the declared `(0,0)`.
        let sc = shell_with_two_dock_windows(); // main panel local (500,400,100,100)
        // The WM actually put main at desktop (200,100) (its declared pos is the
        // `(0,0)` the fixture reports — the very gap this fixes).
        sc.set_live_window_origins(vec![
            ("main".to_string(), (200.0, 100.0)),
            (FLOAT_WINDOW.to_string(), (800.0, 100.0)),
        ]);
        // Desktop cursor (750,550) → main-local via the ACTUAL origin = (550,450),
        // inside main's panel → HIT.
        let hit = sc
            .cross_window_drop_at((750.0, 550.0), None, true)
            .expect("the live path resolves main via its stamped actual origin");
        assert_eq!(hit.window, "main");
        assert_eq!(hit.point.tag, "main_dock");
        // The SAME cursor against the DECLARED `(0,0)` misses main entirely — the
        // pre-R1148 behaviour this fixes (non-tautological: the test fails if the
        // live path used declared positions).
        assert!(
            sc.cross_window_drop_at((750.0, 550.0), None, false)
                .is_none(),
            "the declared-origin path misses main — the bug R1148 fixes",
        );
    }

    // ---- R1617 §5.16 §5.41 §2 #7 — the window's home, per both answerers ----

    /// A two-panel desk: 1920x1080 primary, a second one abutting to its right.
    /// The commonest arrangement there is, and the one where "which display is
    /// this window on" has a seam to disagree about.
    fn two_panel_desk() -> pinion_core::display::DisplayTopology {
        use pinion_core::display::{DisplayInfo, DisplayRect, DisplayTopology};
        DisplayTopology::new(vec![
            DisplayInfo::new("DP-1", DisplayRect::new(0, 0, 1920, 1080)).as_primary(),
            DisplayInfo::new("DP-2", DisplayRect::new(1920, 0, 1920, 1080)),
        ])
    }

    fn homes_of(
        sc: &ShellCore<CrossWindowFixture>,
    ) -> std::collections::HashMap<String, Option<(String, String)>> {
        sc.declared_window_specs()
            .into_iter()
            .map(|w| {
                (
                    w.id,
                    w.display_home.map(|h| {
                        (
                            h.kind.to_owned(),
                            format!("{:?}/{:?}", h.derived, h.platform),
                        )
                    }),
                )
            })
            .collect()
    }

    #[test]
    fn r1617_a_window_with_no_stamp_claims_nothing() {
        // Nobody looked, so nothing is claimed — the rule `anchored` and
        // `level_outcome` already use. Distinct from "we looked and the answer
        // is nowhere", which is the `nowhere` kind.
        let sc = shell_with_two_dock_windows();
        for (_, home) in homes_of(&sc) {
            assert!(home.is_none(), "no topology and no window stamp yet");
        }
        // A topology alone is not enough: the per-window rectangle is the other
        // half, and a window the surface could not read an outer position for
        // stays absent rather than being derived from an invented rectangle.
        sc.set_display_topology(two_panel_desk());
        for (_, home) in homes_of(&sc) {
            assert!(
                home.is_none(),
                "a desk without a window rectangle says nothing"
            );
        }
    }

    #[test]
    fn r1617_agreement_and_divergence_are_both_reported_per_window() {
        use pinion_core::display::{DisplayId, DisplayRect};
        let sc = shell_with_two_dock_windows();
        sc.set_display_topology(two_panel_desk());
        sc.set_window_homes(vec![
            // Wholly on the left panel, and the platform agrees.
            (
                "main".to_owned(),
                DisplayRect::new(100, 100, 800, 600),
                Some(DisplayId::new("dp-1")),
            ),
            // Straddling the seam with the larger share on the RIGHT panel,
            // while the window system names the left one. Both rules are
            // legitimate — this is the case the wire has to be able to state.
            (
                FLOAT_WINDOW.to_owned(),
                DisplayRect::new(1820, 0, 400, 100),
                Some(DisplayId::new("dp-1")),
            ),
        ]);
        let windows: std::collections::HashMap<String, pinion_rpc::DeclaredWindow> = sc
            .declared_window_specs()
            .into_iter()
            .map(|w| (w.id.clone(), w))
            .collect();

        let main = windows["main"].display_home.as_ref().expect("stamped");
        assert_eq!(main.kind, "agreed");
        assert_eq!(main.derived.as_deref(), Some("dp-1"));
        assert_eq!(main.platform.as_deref(), Some("dp-1"));

        let float = windows[FLOAT_WINDOW]
            .display_home
            .as_ref()
            .expect("stamped");
        assert_eq!(float.kind, "diverged");
        assert_eq!(
            float.derived.as_deref(),
            Some("dp-2"),
            "300 of its 400 columns are on the right panel",
        );
        assert_eq!(
            float.platform.as_deref(),
            Some("dp-1"),
            "and the window system's own answer survives beside ours",
        );
    }

    #[test]
    fn r1617_the_home_is_the_live_rect_not_the_declaration() {
        use pinion_core::display::{DisplayId, DisplayRect};
        // The whole reason this is a second field beside `anchored`: the
        // floater DECLARES a position, and the user has since dragged it onto
        // the other panel. `anchored` still reports the declaration; the home
        // reports where the window is.
        let sc = shell_with_two_dock_windows();
        sc.set_display_topology(two_panel_desk());
        // BOTH windows stamped first, so the re-stamp below actually removes
        // something. A fixture whose first stamp is the only stamp cannot tell
        // a wholesale replacement from a merge — the map is empty either way.
        sc.set_window_homes(vec![
            (
                "main".to_owned(),
                DisplayRect::new(10, 10, 400, 300),
                Some(DisplayId::new("dp-1")),
            ),
            (
                FLOAT_WINDOW.to_owned(),
                DisplayRect::new(100, 100, 200, 200),
                Some(DisplayId::new("dp-1")),
            ),
        ]);
        assert!(
            sc.declared_window_specs()
                .into_iter()
                .all(|w| w.display_home.is_some()),
            "both windows have a home before the re-stamp",
        );
        sc.set_window_homes(vec![(
            FLOAT_WINDOW.to_owned(),
            DisplayRect::new(2400, 300, 200, 200),
            Some(DisplayId::new("dp-2")),
        )]);
        let float = sc
            .declared_window_specs()
            .into_iter()
            .find(|w| w.id == FLOAT_WINDOW)
            .expect("declared");
        assert_eq!(
            float.position,
            Some((FLOAT_X, FLOAT_Y)),
            "the declaration is untouched",
        );
        // ...which names a place on the LEFT panel. Asked of the desk rather
        // than compared against a literal, so the claim stays true of whatever
        // arrangement the fixture builds.
        assert_eq!(
            two_panel_desk()
                .display_at(FLOAT_X, FLOAT_Y)
                .map(|d| d.id().as_str()),
            Some("dp-1"),
        );
        let home = float.display_home.expect("stamped");
        assert_eq!(home.kind, "agreed");
        assert_eq!(
            home.derived.as_deref(),
            Some("dp-2"),
            "while the window itself is on the right one — the two questions \
             are different, which is why they are two fields",
        );
        // And `main`, which the latest stamp did NOT report, has stopped
        // claiming a home: the map is replaced wholesale rather than merged, so
        // a window the surface no longer vouches for cannot keep answering with
        // where it used to be.
        let main = sc
            .declared_window_specs()
            .into_iter()
            .find(|w| w.id == "main")
            .expect("declared");
        assert!(
            main.display_home.is_none(),
            "a stale entry would report yesterday's monitor as today's",
        );
    }

    #[test]
    fn r1617_a_silent_platform_is_not_agreement() {
        use pinion_core::display::DisplayRect;
        // The trap this axis's own debt named: the backend's accessor is an
        // `Option` and a hidden window may get `None` from it. That must read
        // as `platform_silent` — reporting it as agreement would manufacture a
        // cross-check that never happened.
        let sc = shell_with_two_dock_windows();
        sc.set_display_topology(two_panel_desk());
        sc.set_window_homes(vec![(
            "main".to_owned(),
            DisplayRect::new(0, 0, 800, 600),
            None,
        )]);
        let home = sc
            .declared_window_specs()
            .into_iter()
            .find(|w| w.id == "main")
            .and_then(|w| w.display_home)
            .expect("stamped");
        assert_eq!(home.kind, "platform_silent");
        assert_eq!(home.derived.as_deref(), Some("dp-1"));
        assert_eq!(home.platform, None);
    }

    #[test]
    fn r1617_a_window_where_no_monitor_reaches_says_so_from_both_sides() {
        use pinion_core::display::{DisplayId, DisplayRect};
        let sc = shell_with_two_dock_windows();
        sc.set_display_topology(two_panel_desk());
        sc.set_window_homes(vec![
            // Past the right edge of a 3840-wide desk: on no display at all,
            // and the window system still hands back a monitor — which is what
            // one of the backends underneath genuinely does.
            (
                "main".to_owned(),
                DisplayRect::new(5000, 0, 200, 200),
                Some(DisplayId::new("dp-1")),
            ),
            // Same place, and this time nobody names anything.
            (
                FLOAT_WINDOW.to_owned(),
                DisplayRect::new(5000, 0, 200, 200),
                None,
            ),
        ]);
        let windows: std::collections::HashMap<String, pinion_rpc::DeclaredWindow> = sc
            .declared_window_specs()
            .into_iter()
            .map(|w| (w.id.clone(), w))
            .collect();
        let main = windows["main"].display_home.as_ref().expect("stamped");
        assert_eq!(main.kind, "derived_nowhere");
        assert_eq!(main.derived, None);
        assert_eq!(main.platform.as_deref(), Some("dp-1"));
        let float = windows[FLOAT_WINDOW]
            .display_home
            .as_ref()
            .expect("stamped");
        assert_eq!(float.kind, "nowhere");
        assert_eq!(float.derived, None);
        assert_eq!(float.platform, None);
    }

    // (R1143 body-centre fallback test removed in R1146 — the VS Code redesign
    // keeps the floater stationary mid-drag, so the cursor-precise single point
    // resolves cleanly and the dual resolution-point band-aid is gone. See
    // `docs/dock-window-move-redesign.md`.)
}

#[cfg(test)]
mod r1138_redock_hint_injection_tests {
    //! R1138 §5.51 §2 #2 §2 #7 PR-33 → R1150 — the END-TO-END headless proof the
    //! live user test could not give: a floater whose held drag is over MAIN's
    //! dock shows the on-TARGET redock preview (`CROSS_WINDOW_DROP_PREVIEW_TAG`) in
    //! MAIN at the dock zone, so an AI `scene/snapshot` SEES where the panel will
    //! land — correctly placed. R1150 removed the R1137 on-FLOATER hint (it drew
    //! the schematic on the static floater at the wrong place: "preview here, docks
    //! there"). The drag SESSION is opened by a real press over the floater's drag
    //! source (a `begin_drag` External), then a cursor march maps the floater-local
    //! cursor to an absolute point over main's dock — the exact floater→main redock
    //! direction — so the shell stashes the cross-window drop and
    //! `apply_cross_window_drop_preview` paints it in main at the target.
    use super::ShellCore;
    use crate::test_fixtures::TestRenderer;
    use crate::{SizeStrategy, WidgetView, WindowChromeStyle, WindowPolicy, WindowSpec};
    use pinion_a11y::WidgetA11y;
    use pinion_core::external::{
        Backend, BackendFallback, BackendSupport, DragPayload, External, IntrospectValue,
        RepaintOwner, ThreadOwnership,
    };
    use pinion_core::scene::{ContainerNode, Rect, Scene};
    use pinion_core::style::LayoutStyle;
    use pinion_core::widgets::button::{ButtonEvent, ButtonState};
    use pinion_core::{Frame, Owner, Signal, WidgetCore};
    use pinion_runtime::PointerId;
    use std::rc::Rc;

    const FLOAT_WINDOW: &str = "float.right";
    const FLOAT_X: i32 = 800;
    const FLOAT_Y: i32 = 100;
    const SRC_TAG: &str = "torn";
    const PREVIEW_TAG: &str = "redock_zone_preview";

    /// A drag source whose `begin_drag` emits the dock-panel TEXT payload, so a
    /// press over it opens a labelled drag session (the floater's torn panel).
    #[derive(Debug)]
    struct TornPanelSource;
    impl External for TornPanelSource {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn begin_drag(&self) -> Option<DragPayload> {
            Some(DragPayload {
                kind: std::borrow::Cow::Borrowed("dock-panel"),
                value: IntrospectValue::Text(SRC_TAG.to_string()),
            })
        }
    }

    /// A window-local drop-target panel scene (main's dock target), injected
    /// directly as main's published paint scene (the resolution reads it).
    fn drop_panel_scene(tag: &str, rect: Rect) -> Scene {
        let mut panel = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag(tag.to_string())
                .with_layout(LayoutStyle::new().with_drop_target(true)),
        );
        if let Scene::Container(c) = &mut panel {
            c.rect = rect;
        }
        let mut root = Scene::Container(ContainerNode::new(vec![panel]));
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, 1000, 800);
        }
        root
    }

    /// The floater's primary widget IS the torn-panel drag source, painted as a
    /// window-filling tagged rect so a press anywhere in the floater hit-tests
    /// to it and opens the drag session.
    struct RedockHintFixture;
    impl WidgetCore for RedockHintFixture {
        type State = ButtonState;
        type Event = ButtonEvent;
        fn create_external() -> Box<dyn External> {
            Box::new(TornPanelSource)
        }
        fn tag() -> &'static str {
            SRC_TAG
        }
        fn read_state(_scene: &Scene) -> Self::State {
            ButtonState::Idle
        }
        fn view(_state: Self::State, _frame: &Frame) -> Scene {
            let mut c = ContainerNode::new(Vec::new());
            c.rect = Rect::new(0, 0, 200, 200);
            c.tag = Some(std::borrow::Cow::Borrowed(SRC_TAG));
            c.layout = LayoutStyle::new();
            Scene::Container(c)
        }
        fn event_name(_event: Self::Event) -> &'static str {
            "__internal__"
        }
        fn title() -> &'static str {
            "RedockHint"
        }
    }
    impl WidgetA11y for RedockHintFixture {}
    impl WidgetView for RedockHintFixture {
        type Renderer = TestRenderer;
        fn initial_size_strategy() -> SizeStrategy {
            SizeStrategy::Fixed {
                width: 200,
                height: 200,
            }
        }
        // (R1202) The MAIN window wears CSD chrome (a min / max / close strip);
        // the floater is a naked borderless drag source. This is what makes an
        // OUTER redock preview's rect matter: it must land BELOW the control strip.
        fn window_policy(window_id: &str) -> WindowPolicy {
            if window_id == FLOAT_WINDOW {
                WindowPolicy::new()
            } else {
                WindowPolicy::new().with_chrome(WindowChromeStyle::default())
            }
        }
        fn windows_signal() -> Option<Rc<Signal<Vec<WindowSpec>>>> {
            Owner::current().map(|owner| {
                owner.cache::<Signal<Vec<WindowSpec>>, _>("redock_hint_windows", || {
                    Signal::new(vec![
                        WindowSpec::main(
                            "RedockHint",
                            SizeStrategy::Fixed {
                                width: 1000,
                                height: 800,
                            },
                        ),
                        WindowSpec::new(
                            FLOAT_WINDOW,
                            "Float",
                            SizeStrategy::Fixed {
                                width: 200,
                                height: 200,
                            },
                        )
                        .with_position(FLOAT_X, FLOAT_Y),
                    ])
                })
            })
        }
        fn view_for_window(_window_id: &str, state: Self::State, frame: &Frame) -> Scene {
            Self::view(state, frame)
        }
        // The dock binding's rendering half: a redock over any zone paints a
        // tagged preview node. The shell injects it into the TARGET window at the
        // dock zone (R1150 removed the on-floater variant).
        fn dock_drop_preview(
            _source_panel: &str,
            _target_tag: &str,
            panel_rect: Rect,
            _x_rel: f32,
            _y_rel: f32,
        ) -> Option<Scene> {
            let mut c = ContainerNode::new(Vec::new())
                .with_tag(PREVIEW_TAG.to_string())
                .with_layout(LayoutStyle::new().with_pointer_transparent(true));
            c.rect = panel_rect;
            Some(Scene::Container(c))
        }
    }

    fn has_tag(scene: &Scene, tag: &str) -> bool {
        if scene.tag() == Some(tag) {
            return true;
        }
        match scene {
            Scene::Container(c) => c.children.iter().any(|ch| has_tag(ch, tag)),
            Scene::Scroll(s) => has_tag(&s.content, tag),
            _ => false,
        }
    }

    /// (R1467) Run the drop-preview overlay step on `scene` the way a producer
    /// does: through the sampled [`super::WindowOverlayInputs`], not a bypass. So
    /// these tests also witness that the sampler actually picks the incoming
    /// cross-window drag up.
    fn drop_preview<V: WidgetView>(sc: &ShellCore<V>, scene: Scene) -> Scene {
        sc.sample_window_overlay_inputs(Some("main"))
            .apply_cross_window_drop_preview::<V>(scene, sc.root_owner())
    }

    #[test]
    fn held_floater_drag_over_main_shows_the_on_target_preview() {
        let mut sc = ShellCore::<RedockHintFixture>::new();
        // Main's dock target at main-local (500, 400, 100, 100) with an EXPLICIT
        // rect, stored directly so the cross-window RESOLUTION (reads the stored
        // scene) hit-tests the abs cursor against it (a `compute` would re-run
        // layout and move the rect). The on-target preview-INJECTION reads the
        // COMPUTED scene, where `view_for_window("main")` also emits `main_dock` —
        // so `rect_for_tag` finds it and the preview is injected.
        sc.core.set_paint_scene_for_window(
            "main",
            drop_panel_scene("main_dock", Rect::new(500, 400, 100, 100)),
        );
        // Produce + publish the floater's own scene so its router can hit-test a
        // press to the drag source.
        let f = sc.compute_paint_scene_for_window(FLOAT_WINDOW, 200, 200);
        sc.finalize_frame_for_window(FLOAT_WINDOW, f);

        // The on-target preview is injected into the scene being painted at the
        // resolved dock zone (`apply_cross_window_drop_preview` reads the painted
        // scene + the cross-window stash). Drive it on a fresh main_dock scene
        // (explicit rects, no re-layout) so the assertion isolates the injection.
        let main_dock_scene = || drop_panel_scene("main_dock", Rect::new(500, 400, 100, 100));
        // Idle: nothing resolved → no preview injected.
        assert!(
            !has_tag(
                &drop_preview(&sc, main_dock_scene()),
                super::CROSS_WINDOW_DROP_PREVIEW_TAG
            ),
            "no drag in flight = no on-target redock preview",
        );

        // Press over the floater's torn panel → opens the drag session.
        sc.cursor_moved_for_window(FLOAT_WINDOW, PointerId::MOUSE, 40.0, 40.0);
        sc.mouse_pressed_for_window(FLOAT_WINDOW, PointerId::MOUSE);
        assert!(
            sc.drag_session_active_for_window(FLOAT_WINDOW, PointerId::MOUSE),
            "a press over the begin_drag source opens the floater's drag session",
        );

        // March the held drag to floater-local (-250, 350) = abs (550, 450),
        // over MAIN's dock target — the floater→main redock direction. The shell
        // resolves the cross-window drop and stashes it on the floater's session.
        sc.cursor_moved_for_window(FLOAT_WINDOW, PointerId::MOUSE, -250.0, 350.0);

        // R1125/R1150: the shell repaints the TARGET (main) every move so its
        // on-target redock preview (where the panel WILL dock) renders live. The
        // R1137 on-floater hint + its source-repaint were removed (R1150).
        assert!(
            sc.redraw_requested_for_window("main"),
            "a cross-window move repaints the TARGET (main) so its on-target preview renders live",
        );

        // MAIN now carries the on-target redock preview at the dock zone — what an
        // AI `scene/snapshot` SEES (and the user sees), correctly placed where the
        // panel will land (the R1150 fix for "preview here, docks there": the
        // misplaced on-floater schematic is gone).
        let previewed = drop_preview(&sc, main_dock_scene());
        assert!(
            has_tag(&previewed, super::CROSS_WINDOW_DROP_PREVIEW_TAG),
            "a held floater drag over main shows the on-target redock preview in main",
        );
        assert!(
            has_tag(&previewed, PREVIEW_TAG),
            "the preview wraps the binding's dock_drop_preview rendering",
        );
        // The dragged floater carries NO redock schematic (R1150 removed the
        // misplaced on-floater hint — its produced scene has no preview tag).
        assert!(
            !has_tag(
                &sc.compute_paint_scene_for_window(FLOAT_WINDOW, 200, 200),
                PREVIEW_TAG
            ),
            "no redock schematic painted on the floater (R1150 removed the on-floater hint)",
        );

        // Release ends the session → the on-target preview clears.
        sc.mouse_released_for_window(FLOAT_WINDOW, PointerId::MOUSE);
        assert!(
            !has_tag(
                &drop_preview(&sc, main_dock_scene()),
                super::CROSS_WINDOW_DROP_PREVIEW_TAG
            ),
            "after release the on-target redock preview clears",
        );
    }

    /// (R1205) A dock target scene whose panel sits inside a `DOCK_SURFACE_TAG`
    /// wrapper at `surface` — the dock area below a client-side chrome strip. The
    /// cross-window OUTER preview + the same-window band both read this wrapper's
    /// rect (`Scene::dock_surface_rect`), so the band lands in the dock area.
    fn dock_surface_scene(panel_tag: &str, panel_rect: Rect, surface: Rect) -> Scene {
        use pinion_core::external::DOCK_SURFACE_TAG;
        let mut panel = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag(panel_tag.to_string())
                .with_layout(LayoutStyle::new().with_drop_target(true)),
        );
        if let Scene::Container(c) = &mut panel {
            c.rect = panel_rect;
        }
        let mut wrapper = Scene::Container(
            ContainerNode::new(vec![panel]).with_tag(DOCK_SURFACE_TAG.to_string()),
        );
        if let Scene::Container(c) = &mut wrapper {
            c.rect = surface;
        }
        let mut root = Scene::Container(ContainerNode::new(vec![wrapper]));
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, 1000, 800);
        }
        root
    }

    /// (R1205) Redocking a floater onto a CSD-chrome window: the OUTER full-span
    /// preview must land in the DOCK AREA (below the min / max / close strip), not
    /// across the whole window over the controls — the user's "붙일 때 최소화/최대화/x
    /// 영역까지 preview가 보임". The band reads the target's `DOCK_SURFACE_TAG` rect, so
    /// preview == where `dock_panel_outer` docks (the topology has no chrome row);
    /// generalising R1202's chrome-height scalar to the surface rect also tracks a
    /// toolbar the scalar was blind to.
    #[test]
    fn r1205_outer_redock_preview_lands_on_the_dock_surface() {
        fn rect_of_tag(scene: &Scene, tag: &str) -> Option<Rect> {
            if scene.tag() == Some(tag) {
                return Some(scene.rect());
            }
            match scene {
                Scene::Container(c) => c.children.iter().find_map(|ch| rect_of_tag(ch, tag)),
                Scene::Scroll(s) => rect_of_tag(&s.content, tag),
                _ => None,
            }
        }
        // Main's dock surface is inset 32px below its chrome strip.
        let surface = Rect::new(0, 32, 1000, 768);
        let main_scene = || dock_surface_scene("main_dock", Rect::new(500, 400, 100, 100), surface);
        let mut sc = ShellCore::<RedockHintFixture>::new();
        // Main is a 1000x800 window at the desktop origin (abs == main-local); its
        // painted dock target sits inside it.
        sc.core.set_paint_scene_for_window("main", main_scene());
        let f = sc.compute_paint_scene_for_window(FLOAT_WINDOW, 200, 200);
        sc.finalize_frame_for_window(FLOAT_WINDOW, f);
        // Open the floater's drag session, then march to main's OUTER LEFT perimeter:
        // abs (5, 400) = 5px inside main's left edge = the outer band. Floater-local
        // (5 - FLOAT_X, 400 - FLOAT_Y) = (-795, 300).
        sc.cursor_moved_for_window(FLOAT_WINDOW, PointerId::MOUSE, 40.0, 40.0);
        sc.mouse_pressed_for_window(FLOAT_WINDOW, PointerId::MOUSE);
        sc.cursor_moved_for_window(FLOAT_WINDOW, PointerId::MOUSE, -795.0, 300.0);
        let previewed = drop_preview(&sc, main_scene());
        assert!(
            has_tag(&previewed, super::CROSS_WINDOW_DROP_PREVIEW_TAG),
            "a floater over main's outer band shows the full-span OUTER redock preview",
        );
        let band = rect_of_tag(&previewed, PREVIEW_TAG).expect("the OUTER band is injected");
        // ★The band starts BELOW the 32px chrome strip (not y=0 over the controls).
        assert_eq!(
            band.y, 32,
            "the OUTER band clears the min/max/close chrome strip (dock surface top)"
        );
        assert_eq!(
            band.h,
            800 - 32,
            "and is shortened to the dock surface height"
        );
        // The horizontal extent still spans the whole surface (a full-span dock).
        assert_eq!(band.x, 0);
        assert_eq!(band.w, 1000);
    }

    /// (R1322 §5.51) A window with NO dock surface hosts no dock, so it offers NO outer
    /// redock zone and therefore NO outer preview.
    ///
    /// This test previously asserted the OPPOSITE — that the band "falls back to the
    /// window rect" — and that fallback is exactly the bug: a torn-off panel's own
    /// floating window (no dock area, and its panel opts OUT of being a drop target per
    /// R1118) still advertised a full outer perimeter, so a second tear-off dragged near
    /// an existing floater redocked INTO it instead of floating, and a floater dragged
    /// back over main resolved its OWN band instead of main's cross-window redock. The
    /// dock area (`DOCK_SURFACE_TAG`, the R1205 SSOT) is what makes a window dockable.
    #[test]
    fn r1322_no_dock_surface_no_outer_redock_preview() {
        fn rect_of_tag(scene: &Scene, tag: &str) -> Option<Rect> {
            if scene.tag() == Some(tag) {
                return Some(scene.rect());
            }
            match scene {
                Scene::Container(c) => c.children.iter().find_map(|ch| rect_of_tag(ch, tag)),
                Scene::Scroll(s) => rect_of_tag(&s.content, tag),
                _ => None,
            }
        }
        let main_scene = || drop_panel_scene("main_dock", Rect::new(500, 400, 100, 100));
        let mut sc = ShellCore::<RedockHintFixture>::new();
        sc.core.set_paint_scene_for_window("main", main_scene());
        let f = sc.compute_paint_scene_for_window(FLOAT_WINDOW, 200, 200);
        sc.finalize_frame_for_window(FLOAT_WINDOW, f);
        sc.cursor_moved_for_window(FLOAT_WINDOW, PointerId::MOUSE, 40.0, 40.0);
        sc.mouse_pressed_for_window(FLOAT_WINDOW, PointerId::MOUSE);
        sc.cursor_moved_for_window(FLOAT_WINDOW, PointerId::MOUSE, -795.0, 300.0);
        let previewed = drop_preview(&sc, main_scene());
        assert!(
            !has_tag(&previewed, super::CROSS_WINDOW_DROP_PREVIEW_TAG),
            "★no dock surface → no outer redock zone, so no preview band is injected",
        );
        assert!(
            rect_of_tag(&previewed, PREVIEW_TAG).is_none(),
            "★…and nothing is painted over a window that cannot receive the dock",
        );
    }

    /// (R1205) The CRUX integration path: compose the REAL dock walker output with
    /// `chrome_inset_wrap` (the live paint path's borderless-window inset) + a real
    /// `compute_layout`, and assert the `DOCK_SURFACE` wrapper lands BELOW the
    /// chrome strip. This is the fact R1205 replaced the top-only scalar with — the
    /// surface rect is LAYOUT-derived (it would track a toolbar/menu the scalar
    /// could not), not the hard-coded rect the preview unit tests inject.
    #[test]
    fn r1205_chrome_inset_lays_the_dock_surface_below_the_strip() {
        use pinion_core::reactive::Owner;
        use pinion_runtime::layout::compute_layout;
        use pinion_text::LayoutCache;
        use pinion_widget_paint::dock::{DockTopology, view_dock_surface};
        const CHROME_H: u32 = 32; // WindowChromeStyle::default().height_px
        Owner::new().run(|| {
            let topo = DockTopology::single("a");
            let theme = pinion_core::theme::Theme::light();
            // A single-leaf surface: the split_state closure never fires, so no
            // DockSplitState construction is needed — the wrapper + one panel.
            let surface = view_dock_surface(
                &topo,
                |_| Scene::Container(ContainerNode::new(Vec::new())),
                |_, _| unreachable!("single leaf has no split"),
                |_| None,
                &theme,
            );
            // Wrap exactly as the live paint path does for a 32px-chrome window.
            let mut scene = super::chrome_inset_wrap(surface, CHROME_H);
            let mut cache = LayoutCache::new();
            compute_layout(&mut scene, &mut cache, 1000, 800);
            assert_eq!(
                scene.dock_surface_rect(),
                Rect::new(0, CHROME_H, 1000, 800 - CHROME_H),
                "chrome_inset_wrap lays the DOCK_SURFACE below the strip (layout-derived)",
            );
        });
    }

    // (R1168 removed `r1142_drag_release_repaints_the_guide_host_to_strip_its_guides`
    // and the R1144 freeze-guard note with the static dock-zone guides they tested:
    // no window now paints a drag affordance unless the cursor is over it, so the
    // drag-end "repaint every other window" backstop is gone. The cross-window
    // preview clears via its per-target repaint — see the
    // `apply_cross_window_drop_preview` clears-on-cross test above.)
}

#[cfg(test)]
mod r1072_text_engine_wiring_tests {
    //! R1072 §5.37 — the shell is the §5.37 self-hosted engine's first SHIPPING
    //! consumer: `compute_paint_scene` threads `text_measure_override(self.text_engine.as_ref())`
    //! into the layout pass, and `text_cache_and_engine` feeds the same engine to
    //! the cached paint. These tests prove the MEASURE half deterministically with
    //! the bundled `NotoSans` fixture (the paint half's pixel proof lives in the
    //! realgpu seam test `tests/text_engine_paint_seam.rs`). Both arms share the
    //! eligibility SSOT, so a caret-bearing leaf stays parley in the shell too.
    use super::{ShellCore, build_text_engine_from_env};
    use crate::test_fixtures::TestRenderer;
    use crate::{SizeStrategy, WidgetView};
    use pinion_a11y::WidgetA11y;
    use pinion_core::external::External;
    use pinion_core::scene::{ContainerNode, Rect, TextNode};
    use pinion_core::style::{AlignItems, FlexDirection, LayoutStyle, TextStyle};
    use pinion_core::test_fixtures::ButtonFixture;
    use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
    use pinion_core::{Frame, Scene, WidgetCore};
    use pinion_runtime::TextMeasure;
    use pinion_runtime::text_engine::SelfHostedTextEngine;
    use pinion_text_font::Font;

    const NOTO: &[u8] = include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");
    const NANUM: &[u8] =
        include_bytes!("../../pinion-text-font/tests/fonts/NanumGothic-Regular.ttf");

    /// ★★★★★ R1712.1 — the fixtures this test may inject, in order of
    /// preference, and it **picks the one that actually discriminates**.
    ///
    /// Step (0) below requires the injected §5.37 measure to differ from what
    /// parley answers off-path, or assertion (1) is vacuous. That is a property
    /// of the pair *(fixture, whatever this host resolves `sans-serif` to)*, and
    /// it stopped holding: measured on a build host whose resolved sans-serif is
    /// Noto Sans, the label came out **74 pixels on both arms** and the test
    /// refused the run — doing exactly what it was written to do, and its
    /// comment said what to do about it ("swap the fixture, do not weaken the
    /// proof").
    ///
    /// Swapping by hand would have moved the collision to the next host rather
    /// than removing it, so the swap is the test's own job now. What is being
    /// proven — the shell's measure pass routes eligible text through the
    /// injected engine — does not care *which* font is injected, only that its
    /// metrics are distinguishable from the host's. Picking by measurement is
    /// therefore the same proof with the host taken out of it, and a host where
    /// **no** fixture discriminates is reported by name rather than passed.
    const FIXTURES: [(&str, &[u8]); 2] = [("NotoSans", NOTO), ("NanumGothic", NANUM)];
    const LABEL: &str = "Measure";
    const FIELD: &str = "Editable";
    const PX: u32 = 18;

    /// A binding whose paint scene is a Start-aligned flex row of two text leaves:
    /// an eligible static label and a caret-bearing (editable) leaf, both measured
    /// at intrinsic single-line width (the layout shape `compute_layout`'s §5.37
    /// measure sizes exactly to the shaped advance).
    struct TextEngineFixture;

    impl WidgetCore for TextEngineFixture {
        type State = ButtonState;
        type Event = ButtonEvent;

        fn create_external() -> Box<dyn External> {
            Box::new(ButtonExternal::new())
        }
        fn tag() -> &'static str {
            "text_engine_fixture"
        }
        fn read_state(scene: &Scene) -> Self::State {
            <ButtonFixture as WidgetCore>::read_state(scene)
        }
        fn view(_state: Self::State, _frame: &Frame) -> Scene {
            let style = TextStyle::new().with_size_px(PX);
            let label = Scene::Text(TextNode::styled(LABEL, Rect::default(), style.clone()));
            let field =
                Scene::Text(TextNode::styled(FIELD, Rect::default(), style).caret_bearing());
            Scene::Container(
                ContainerNode::new(vec![label, field]).with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Start),
                ),
            )
        }
        fn event_name(event: Self::Event) -> &'static str {
            <ButtonFixture as WidgetCore>::event_name(event)
        }
        fn title() -> &'static str {
            "TextEngine"
        }
    }

    impl WidgetA11y for TextEngineFixture {}

    impl WidgetView for TextEngineFixture {
        type Renderer = TestRenderer;
        fn initial_size_strategy() -> SizeStrategy {
            SizeStrategy::Fixed {
                width: 800,
                height: 200,
            }
        }
    }

    /// Measured width of the first `Scene::Text` whose content matches.
    fn text_width(scene: &Scene, content: &str) -> Option<u32> {
        match scene {
            Scene::Text(t) if t.content == content => Some(t.rect.w),
            Scene::Container(c) => c.children.iter().find_map(|ch| text_width(ch, content)),
            Scene::Scroll(s) => text_width(&s.content, content),
            _ => None,
        }
    }

    #[test]
    fn shell_threads_engine_into_measure_and_excludes_caret_bearing() {
        let mut sc = ShellCore::<TextEngineFixture>::new();
        // Engine OFF (PINION_TEXT_ENGINE unset in the test process): parley measure.
        let off = sc.compute_paint_scene(800, 200);
        let off_label_w = text_width(&off, LABEL).expect("label text node present");
        let off_field_w = text_width(&off, FIELD).expect("field text node present");

        // R1712.1 — the fixture is CHOSEN, against what this host resolved above:
        // the first whose §5.37 measure of the label is distinguishable from
        // parley's. See [`FIXTURES`] for why that choice is the test's job.
        let style = TextStyle::new().with_size_px(PX);
        let mut tried = Vec::new();
        let picked = FIXTURES.iter().find_map(|(name, bytes)| {
            let font = Font::from_bytes(bytes.to_vec()).expect("parse the fixture font");
            let engine = SelfHostedTextEngine::from_font(font);
            // The exact §5.37 box width the shell measure should size the
            // eligible label to (the engine's own measure arm, computed before
            // the engine is moved into the shell).
            let measured = engine
                .measure_text(LABEL, &style, &[], None, false)
                .expect("the eligible label is measurable via §5.37");
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let width = measured.width.ceil() as u32;
            tried.push(format!("{name} measures {width}"));
            (width != off_label_w).then_some((*name, engine, width))
        });
        let (fixture, engine, expected_label_w) = picked.unwrap_or_else(|| {
            panic!(
                "no fixture font is distinguishable from what this host resolves \
                 `sans-serif` to (parley measured {off_label_w} for {LABEL:?} at \
                 {PX}px; {}). Assertion (1) below would be vacuous, so this is a \
                 refusal rather than a pass — add a metrically distinct fixture, \
                 do not weaken the proof.",
                tried.join(", ")
            )
        });

        // Inject the §5.37 engine via the test-only construction seam.
        sc.set_text_engine(Some(engine));
        let on = sc.compute_paint_scene(800, 200);
        let on_label_w = text_width(&on, LABEL).expect("label text node present");
        let on_field_w = text_width(&on, FIELD).expect("field text node present");

        // (0) R1072.1 — the test is DISCRIMINATING: the §5.37 measure differs
        // from the off-path parley measure, so assertion (1) below cannot pass
        // vacuously. R1712.1 — this now follows from how `fixture` was chosen,
        // and is still asserted because it is the property, not the mechanism.
        assert_ne!(
            on_label_w, off_label_w,
            "§5.37 width must differ from parley's ({fixture} injected), or \
             assertion (1) is vacuous"
        );
        // (1) The engine reached the shell's MEASURE pass: the eligible label is
        // sized to the §5.37 box width, not parley's.
        assert_eq!(
            on_label_w, expected_label_w,
            "the shell measured the eligible label through §5.37"
        );
        // (2) The caret-bearing (editable) leaf is excluded from §5.37 in the shell
        // measure too — identical box width engine on vs off (both arms together).
        assert_eq!(
            on_field_w, off_field_w,
            "the caret-bearing leaf stays on parley with the engine enabled (shell-level exclusion)"
        );
    }

    #[test]
    fn default_construction_builds_no_engine() {
        // 0-regression default: with `PINION_TEXT_ENGINE` unset the shell builds no
        // engine, so every leaf stays on parley (byte-identical to pre-R1072). Guard
        // on the env so a developer who opted in does not see a spurious failure.
        if std::env::var("PINION_TEXT_ENGINE").is_err() {
            assert!(
                build_text_engine_from_env(None).is_none(),
                "no engine without the opt-in env var"
            );
        }
    }
}

#[cfg(test)]
mod r863_bounds_union_tests {
    use super::{AccessNode, LayoutCache, Scene, rect_for_tag, resolve_access_bounds};
    use pinion_a11y::AriaRole;
    use pinion_core::scene::ContainerNode;
    use pinion_core::style::{FlexDirection, LayoutStyle, Size};
    use pinion_runtime::compute_layout;

    /// A frozen-split-shaped scene: a flex row of two fixed-width strips
    /// tagged like a frozen grid's two panes for one logical row (`g_frow0`
    /// frozen pane, `g_row0` scrolling pane).
    fn split_row_scene() -> Scene {
        let frozen = Scene::Container(
            ContainerNode::new(Vec::new())
                .with_tag("g_frow0")
                .with_layout(LayoutStyle::new().with_size(Size::px(80, 24))),
        );
        let scrolled = Scene::Container(
            ContainerNode::new(Vec::new())
                .with_tag("g_row0")
                .with_layout(LayoutStyle::new().with_size(Size::px(200, 24))),
        );
        Scene::Container(
            ContainerNode::new(vec![frozen, scrolled])
                .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
        )
    }

    #[test]
    fn union_bounds_span_both_frozen_split_panes() {
        let mut scene = split_row_scene();
        let mut cache = LayoutCache::new();
        compute_layout(&mut scene, &mut cache, 280, 24);
        let frozen_rect = rect_for_tag(&scene, "g_frow0").expect("frozen strip resolves");
        let scrolled_rect = rect_for_tag(&scene, "g_row0").expect("scrolled strip resolves");

        // A Row node painted across both panes: own tag = the scrolled strip,
        // union fragment = the frozen strip.
        let mut nodes =
            vec![AccessNode::new("g_row0", AriaRole::Row).with_bounds_union_tag("g_frow0")];
        resolve_access_bounds(&scene, &mut nodes);
        let union = nodes[0].bounds.expect("row bounds resolved");

        assert_eq!(
            union,
            frozen_rect.union(scrolled_rect),
            "bounds = union of both panes"
        );
        assert_eq!(
            union.x, frozen_rect.x,
            "union starts at the frozen pane's left"
        );
        assert!(
            union.w > scrolled_rect.w,
            "the unioned row ({union:?}) is wider than the scrolled strip alone ({scrolled_rect:?})",
        );
    }

    #[test]
    fn absent_union_fragment_is_skipped() {
        // The defining safety property: a union tag absent from the paint
        // scene contributes nothing, so a Row resolves to its own strip when
        // the split is inactive (or the fragment scrolled out / never painted).
        let mut scene = split_row_scene();
        let mut cache = LayoutCache::new();
        compute_layout(&mut scene, &mut cache, 280, 24);
        let scrolled_rect = rect_for_tag(&scene, "g_row0").unwrap();
        let mut nodes =
            vec![AccessNode::new("g_row0", AriaRole::Row).with_bounds_union_tag("g_absent")];
        resolve_access_bounds(&scene, &mut nodes);
        assert_eq!(
            nodes[0].bounds,
            Some(scrolled_rect),
            "absent fragment leaves the primary rect"
        );
    }

    #[test]
    fn no_union_tags_resolves_primary_only() {
        let mut scene = split_row_scene();
        let mut cache = LayoutCache::new();
        compute_layout(&mut scene, &mut cache, 280, 24);
        let scrolled_rect = rect_for_tag(&scene, "g_row0").unwrap();
        let mut nodes = vec![AccessNode::new("g_row0", AriaRole::Row)];
        resolve_access_bounds(&scene, &mut nodes);
        assert_eq!(
            nodes[0].bounds,
            Some(scrolled_rect),
            "single-fragment node = its own tag"
        );
    }
}

#[cfg(test)]
mod r1193_window_state_tests {
    //! R1193 §5.16 §5.28 §5.39 — the per-window `WindowState` consolidation: 11
    //! formerly-parallel `*_per_window` maps became one
    //! `HashMap<String, WindowState>`. These pin, through the PUBLIC accessors,
    //! the invariants the consolidation must preserve — per-axis presence
    //! isolation, one-shot teardown, and the empty-entry prune.
    use crate::ShellCore;
    use pinion_core::test_fixtures::EchoButtonFixture;

    /// Setting ONE axis for a window must not make another axis's presence-gated
    /// view include it — the crux of the consolidation (an entry created for one
    /// axis must not read as present for the others).
    #[test]
    fn r1193_per_window_axes_are_presence_isolated() {
        let mut sc = ShellCore::<EchoButtonFixture>::new();
        sc.set_maximized_for_window("pane-9", true);
        assert!(
            sc.maximized_for_window("pane-9"),
            "the axis we set reads back"
        );
        // The fragment-stats window set filters by fragment presence, NOT by
        // "has any WindowState" — a maximized-only window must be excluded.
        assert!(
            !sc.fragment_cache_stat_windows().any(|w| w == "pane-9"),
            "a maximized-only window is not a fragment-stats window",
        );
        // ...and its never-touched axes read their absent-defaults.
        assert_eq!(sc.fragment_cache_stats_for_window("pane-9"), None);
        assert_eq!(sc.target_fps_for_window("pane-9"), None);
        assert!(!sc.immediate_subtree_for_window("pane-9"));
        assert!(!sc.redraw_requested_for_window("pane-9"));
    }

    /// `remove_window` drops EVERY axis in one call (the payoff: no per-window
    /// map can be forgotten in teardown) and reports whether state was carried.
    #[test]
    fn r1193_remove_window_drops_every_axis_at_once() {
        let mut sc = ShellCore::<EchoButtonFixture>::new();
        sc.set_maximized_for_window("pane-9", true);
        sc.set_target_fps_for_window("pane-9", 30);
        sc.request_redraw_for_window("pane-9");
        assert!(
            sc.remove_window("pane-9"),
            "remove reports the carried state"
        );
        assert!(!sc.maximized_for_window("pane-9"));
        assert_eq!(sc.target_fps_for_window("pane-9"), None);
        assert!(!sc.redraw_requested_for_window("pane-9"));
        assert!(
            !sc.remove_window("pane-9"),
            "a re-remove finds nothing (every axis was dropped)",
        );
    }

    /// Clearing a window's SOLE axis prunes the entry, so `remove_window`'s
    /// "carried an entry" return matches the pre-R1193 per-map behavior (no
    /// lingering all-default `WindowState`).
    #[test]
    fn r1193_clearing_the_sole_axis_prunes_the_entry() {
        let mut sc = ShellCore::<EchoButtonFixture>::new();
        sc.set_target_fps_for_window("pane-9", 30);
        sc.clear_target_fps_for_window("pane-9");
        assert!(
            !sc.remove_window("pane-9"),
            "clearing the only axis leaves no stale entry",
        );
    }
}

#[cfg(test)]
mod r1121_window_chrome_tests {
    //! R1121 §5.16 §5.39 §2 #7 PR-38 — a borderless window (`decorations:
    //! false`) gets a client-side chrome strip (title + close/min/max + grip)
    //! injected into its paint scene with the content inset below it; a
    //! decorated window (the default) is byte-identical (no chrome, no inset).
    use crate::test_fixtures::TestRenderer;
    use crate::{ShellCore, SizeStrategy, WidgetView, WindowSpec};
    use pinion_a11y::WidgetA11y;
    use pinion_core::external::{External, StubExternal};
    use pinion_core::scene::{BoxNode, ContainerNode, Rect, Scene};
    use pinion_core::style::{BoxStyle, Color, LayoutStyle, Size, SizeValue};
    use pinion_core::widgets::button::{ButtonEvent, ButtonState};
    use pinion_core::{Frame, WidgetCore};
    use pinion_overlay::{WindowChromeStyle, WindowControl};

    const CONTENT_TAG: &str = "r1121-content";
    const CHROME_H: u32 = 32; // WindowChromeStyle::default().height_px

    fn content_view() -> Scene {
        let mut c = ContainerNode::new(Vec::new());
        c.tag = Some(std::borrow::Cow::Borrowed(CONTENT_TAG));
        c.layout = LayoutStyle::new().with_size(
            Size::auto()
                .with_width(SizeValue::Percent(100))
                .with_height(SizeValue::Percent(100)),
        );
        Scene::Container(c)
    }

    // R1121.1 — `$decorations` (OS frame) and `$chrome` (pinion chrome hook)
    // are ORTHOGONAL params: the fixtures cover all three live combinations.
    // R1123.1 — `$id` is the canonical window id (usually "main"); a non-"main"
    // id exercises the window-identity resolution (the maximized cache must be
    // keyed by the resolved spec id, not by `DEFAULT_WINDOW`).
    macro_rules! chrome_fixture {
        ($name:ident, $id:literal, $decorations:literal, $chrome:expr, $resizable:expr, $title:literal) => {
            struct $name;
            impl WidgetCore for $name {
                type State = ButtonState;
                type Event = ButtonEvent;
                fn create_external() -> Box<dyn External> {
                    Box::new(StubExternal)
                }
                fn tag() -> &'static str {
                    CONTENT_TAG
                }
                fn read_state(_: &Scene) -> Self::State {
                    ButtonState::Idle
                }
                fn view(_: Self::State, _: &Frame) -> Scene {
                    content_view()
                }
                fn event_name(_: Self::Event) -> &'static str {
                    "__internal__"
                }
                fn title() -> &'static str {
                    $title
                }
            }
            impl WidgetA11y for $name {}
            impl WidgetView for $name {
                type Renderer = TestRenderer;
                fn initial_size_strategy() -> SizeStrategy {
                    SizeStrategy::Fixed {
                        width: 400,
                        height: 300,
                    }
                }
                fn windows() -> Vec<WindowSpec> {
                    vec![
                        WindowSpec::new(
                            $id,
                            $title,
                            SizeStrategy::Fixed {
                                width: 400,
                                height: 300,
                            },
                        )
                        .with_decorations($decorations),
                    ]
                }
                fn window_policy(_window_id: &str) -> crate::WindowPolicy {
                    crate::WindowPolicy {
                        chrome: $chrome,
                        resizable: $resizable,
                    }
                }
            }
        };
    }
    // CSD: no OS frame + pinion chrome. `window_resizable` = None (derive from
    // chrome ⇒ resizable).
    chrome_fixture!(
        Borderless,
        "main",
        false,
        Some(WindowChromeStyle::default()),
        None,
        "My Terminal"
    );
    // OS-decorated: OS frame, no pinion chrome. None (derive ⇒ not client-resizable).
    chrome_fixture!(Decorated, "main", true, None, None, "Decorated");
    // Naked borderless (the Phase-C/D fullscreen-game surface): no OS frame AND
    // no pinion chrome — the combination R1121's decorations-coupling could not
    // express, the reason R1121.1 decoupled them. None (derive ⇒ no resize).
    chrome_fixture!(NakedBorderless, "main", false, None, None, "Naked");
    // R1123.1 — a chromed window whose canonical id is NOT "main", to prove the
    // maximized lookup keys by the resolved spec id (not `DEFAULT_WINDOW`).
    chrome_fixture!(
        BorderlessPanel,
        "panel",
        false,
        Some(WindowChromeStyle::default()),
        None,
        "Panel"
    );
    // R1186 §5.16 §5.39 — the controls-in-header floater: no OS frame, NO pinion
    // chrome (`policy.chrome == None`, its title bar is the dock HEADER), yet
    // `policy.resizable == Some(true)` ⇒ the client-side resize border is
    // decoupled from chrome and still injected. The PR-43 shape.
    chrome_fixture!(
        ResizableChromeless,
        "main",
        false,
        None,
        Some(true),
        "Header Floater"
    );
    // R1186 — the inverse override: a chromed window that opts OUT of resize
    // (`Some(false)`), e.g. a fixed-size chromed dialog. Proves the hook can
    // suppress the border the chrome gate would otherwise imply.
    chrome_fixture!(
        NonResizableChromed,
        "main",
        false,
        Some(WindowChromeStyle::default()),
        Some(false),
        "Fixed Dialog"
    );

    fn rect_of(scene: &Scene, tag: &str) -> Option<Rect> {
        scene.rect_for_tag_absolute(tag)
    }

    fn has_tag(scene: &Scene, tag: &str) -> bool {
        if scene.tag() == Some(tag) {
            return true;
        }
        match scene {
            Scene::Container(c) => c.children.iter().any(|ch| has_tag(ch, tag)),
            Scene::Scroll(s) => has_tag(&s.content, tag),
            _ => false,
        }
    }

    #[test]
    fn borderless_window_injects_chrome_strip_and_controls() {
        let mut sc = ShellCore::<Borderless>::new();
        let scene = sc.compute_paint_scene(400, 300);
        assert!(
            has_tag(&scene, pinion_overlay::WINDOW_CHROME_TAG),
            "borderless window paints a chrome strip",
        );
        for tag in [
            pinion_overlay::WINDOW_CHROME_CLOSE_TAG,
            pinion_overlay::WINDOW_CHROME_MINIMIZE_TAG,
            pinion_overlay::WINDOW_CHROME_MAXIMIZE_TAG,
            pinion_overlay::WINDOW_CHROME_GRIP_TAG,
        ] {
            assert!(has_tag(&scene, tag), "chrome control {tag} is present");
        }
    }

    #[test]
    fn borderless_window_insets_content_below_the_strip() {
        let mut sc = ShellCore::<Borderless>::new();
        let scene = sc.compute_paint_scene(400, 300);
        let content = rect_of(&scene, CONTENT_TAG).expect("content laid out");
        assert_eq!(content.y, CHROME_H, "content insets below the chrome strip");
        assert_eq!(
            content.h,
            300 - CHROME_H,
            "content fills the remaining height under the strip",
        );
        assert_eq!(content.w, 400, "content spans the full width");
    }

    #[test]
    fn decorated_window_has_no_chrome_and_no_inset() {
        let mut sc = ShellCore::<Decorated>::new();
        let scene = sc.compute_paint_scene(400, 300);
        assert!(
            !has_tag(&scene, pinion_overlay::WINDOW_CHROME_TAG),
            "a decorated window relies on OS chrome — pinion injects none",
        );
        let content = rect_of(&scene, CONTENT_TAG).expect("content laid out");
        assert_eq!(
            content.y, 0,
            "decorated content is not inset (byte-identical)"
        );
        assert_eq!(content.h, 300, "decorated content fills the full height");
    }

    #[test]
    fn naked_borderless_window_has_no_chrome_and_no_inset() {
        // R1121.1 decoupling proof: `decorations:false` (no OS frame) WITHOUT a
        // `chrome: None` = naked borderless (the fullscreen-game surface).
        // R1121's `decorations:false ⇒ chrome` coupling could not express this;
        // the hook can, so chrome is absent and content fills the full window.
        let mut sc = ShellCore::<NakedBorderless>::new();
        let scene = sc.compute_paint_scene(400, 300);
        assert!(
            !has_tag(&scene, pinion_overlay::WINDOW_CHROME_TAG),
            "a borderless window with chrome: None gets no chrome",
        );
        let content = rect_of(&scene, CONTENT_TAG).expect("content laid out");
        assert_eq!(content.y, 0, "naked borderless content is not inset");
        assert_eq!(content.h, 300, "naked content fills the full window height");
    }

    #[test]
    fn introspection_mirror_matches_the_live_paint_inset() {
        // §2 #7: scene/snapshot (the pure mirror) must agree with the painted
        // geometry — both inset the content below the chrome.
        let mut sc = ShellCore::<Borderless>::new();
        let live = sc.compute_paint_scene(400, 300);
        let mirror = sc.compute_paint_scene_pure(400, 300);
        assert_eq!(
            rect_of(&live, CONTENT_TAG),
            rect_of(&mirror, CONTENT_TAG),
            "the introspection mirror insets content identically to the live paint",
        );
        assert!(
            has_tag(&mirror, pinion_overlay::WINDOW_CHROME_TAG),
            "the chrome strip is introspectable in the pure mirror too",
        );
    }

    #[test]
    fn cursor_over_a_chrome_control_resolves_its_tag() {
        // The routing contract `AppShell::try_chrome_press` relies on: a cursor
        // over a chrome button resolves (via the SAME router hit-test the live
        // pointer uses) to that control's tag. Close button = rightmost 46px of
        // the 32px strip; centre ~ (400-23, 16).
        use pinion_runtime::PointerId;
        let mut sc = ShellCore::<Borderless>::new();
        let boot = sc.compute_paint_scene(400, 300);
        sc.finalize_frame(boot);
        sc.cursor_moved(PointerId::MOUSE, 377.0, 16.0);
        assert_eq!(
            sc.hover_target(PointerId::MOUSE),
            Some(pinion_overlay::WINDOW_CHROME_CLOSE_TAG),
            "cursor over the close button resolves to the close control tag",
        );
        // Grip (strip background, away from any button) resolves to the move handle.
        sc.cursor_moved(PointerId::MOUSE, 12.0, 16.0);
        assert_eq!(
            sc.hover_target(PointerId::MOUSE),
            Some(pinion_overlay::WINDOW_CHROME_GRIP_TAG),
            "cursor over the strip background resolves to the move grip",
        );
    }

    // ---- R1122 §5.16 §5.39 client-side resize border ----
    //
    // A chromed borderless window also gets eight resize edge / corner regions
    // (it has no OS frame to drag-resize). They are injected UNDER the chrome
    // strip so the title bar's controls keep winning at the top.

    const ALL_RESIZE_TAGS: [&str; 8] = [
        pinion_overlay::WINDOW_RESIZE_NORTH_TAG,
        pinion_overlay::WINDOW_RESIZE_SOUTH_TAG,
        pinion_overlay::WINDOW_RESIZE_WEST_TAG,
        pinion_overlay::WINDOW_RESIZE_EAST_TAG,
        pinion_overlay::WINDOW_RESIZE_NORTH_WEST_TAG,
        pinion_overlay::WINDOW_RESIZE_NORTH_EAST_TAG,
        pinion_overlay::WINDOW_RESIZE_SOUTH_WEST_TAG,
        pinion_overlay::WINDOW_RESIZE_SOUTH_EAST_TAG,
    ];

    #[test]
    fn borderless_window_injects_all_resize_regions() {
        let mut sc = ShellCore::<Borderless>::new();
        let scene = sc.compute_paint_scene(400, 300);
        for tag in ALL_RESIZE_TAGS {
            assert!(
                has_tag(&scene, tag),
                "chromed window paints resize region {tag}"
            );
        }
    }

    #[test]
    fn decorated_window_has_no_resize_border() {
        let mut sc = ShellCore::<Decorated>::new();
        let scene = sc.compute_paint_scene(400, 300);
        for tag in ALL_RESIZE_TAGS {
            assert!(
                !has_tag(&scene, tag),
                "a decorated window relies on the OS frame — no resize region {tag}",
            );
        }
    }

    #[test]
    fn naked_borderless_window_has_no_resize_border() {
        // Resize travels with chrome: a naked borderless window (no chrome
        // hook) is the fullscreen-game surface and wants no resize border.
        let mut sc = ShellCore::<NakedBorderless>::new();
        let scene = sc.compute_paint_scene(400, 300);
        for tag in ALL_RESIZE_TAGS {
            assert!(
                !has_tag(&scene, tag),
                "a naked borderless window gets no resize region {tag}"
            );
        }
    }

    /// The five side / bottom regions every resizable window keeps regardless of
    /// its top-edge treatment. (Since R1197/R1198 a chrome-less content-header
    /// floater ALSO gets a north edge + both top corners — see
    /// `content_header_floater_resizes_top_edge_and_both_corners`; this const is
    /// just the always-present side/bottom set.)
    const SIDE_BOTTOM_RESIZE_TAGS: [&str; 5] = [
        pinion_overlay::WINDOW_RESIZE_SOUTH_TAG,
        pinion_overlay::WINDOW_RESIZE_WEST_TAG,
        pinion_overlay::WINDOW_RESIZE_EAST_TAG,
        pinion_overlay::WINDOW_RESIZE_SOUTH_WEST_TAG,
        pinion_overlay::WINDOW_RESIZE_SOUTH_EAST_TAG,
    ];
    #[test]
    fn content_header_floater_resizes_top_edge_and_both_corners() {
        // R1197 / R1198 §5.16 §5.39 — a controls-in-header floater (chrome: None,
        // resizable) resizes from its TOP EDGE + BOTH top corners (VS Code parity
        // for a floating panel). The top-RIGHT corner is a small edge-sized box
        // (R1198) that clears the inset close button, so it resizes without
        // shadowing it. Sides + bottom + bottom corners are present as always.
        let mut sc = ShellCore::<ResizableChromeless>::new();
        let scene = sc.compute_paint_scene(400, 300);
        assert!(
            !has_tag(&scene, pinion_overlay::WINDOW_CHROME_TAG),
            "the controls-in-header floater draws NO shell chrome strip (one strip)",
        );
        for tag in SIDE_BOTTOM_RESIZE_TAGS {
            assert!(
                has_tag(&scene, tag),
                "★a chrome-less resizable window paints side/bottom resize region {tag}",
            );
        }
        assert!(
            has_tag(&scene, pinion_overlay::WINDOW_RESIZE_NORTH_TAG),
            "★the top edge resizes (R1197)",
        );
        assert!(
            has_tag(&scene, pinion_overlay::WINDOW_RESIZE_NORTH_WEST_TAG),
            "★the top-left corner resizes (R1197)",
        );
        assert!(
            has_tag(&scene, pinion_overlay::WINDOW_RESIZE_NORTH_EAST_TAG),
            "★the top-right corner resizes too (R1198: a small edge-sized corner \
             that clears the close button)",
        );
    }

    #[test]
    fn content_header_floater_top_edge_resizes_close_reachable_below() {
        // R1197 — the top edge resizes even over the close-button area: its
        // outermost `RESIZE_EDGE_PX` (6px) is the north resize band, and the close
        // button is reachable just below it (the top analogue of R1186's
        // side-padding contract). The top-LEFT corner resizes diagonally; the
        // top-RIGHT corner stays the close button's.
        use pinion_runtime::PointerId;
        let mut sc = ShellCore::<ResizableChromeless>::new();
        let boot = sc.compute_paint_scene(400, 300);
        sc.finalize_frame(boot);
        // Top 6px over the close-button area → the north resize band.
        sc.cursor_moved(PointerId::MOUSE, 385.0, 3.0);
        assert_eq!(
            sc.hover_target(PointerId::MOUSE),
            Some(pinion_overlay::WINDOW_RESIZE_NORTH_TAG),
            "★the top edge over the close-button area is the north resize band",
        );
        // Below the band, the close-button area reaches content (button clickable).
        sc.cursor_moved(PointerId::MOUSE, 385.0, 16.0);
        assert_eq!(
            sc.hover_target(PointerId::MOUSE),
            Some(CONTENT_TAG),
            "below the band the close-button area reaches content",
        );
        // The top-LEFT corner resizes diagonally (no controls there).
        sc.cursor_moved(PointerId::MOUSE, 3.0, 3.0);
        assert_eq!(
            sc.hover_target(PointerId::MOUSE),
            Some(pinion_overlay::WINDOW_RESIZE_NORTH_WEST_TAG),
            "the top-left corner resizes diagonally",
        );
        // R1198 — the very top-right corner (the outermost RESIZE_EDGE_PX box,
        // clear of the inset close button) resizes diagonally too.
        sc.cursor_moved(PointerId::MOUSE, 397.0, 3.0);
        assert_eq!(
            sc.hover_target(PointerId::MOUSE),
            Some(pinion_overlay::WINDOW_RESIZE_NORTH_EAST_TAG),
            "the very top-right corner resizes diagonally (near, but clear of, close)",
        );
        // The bottom-right corner IS a resize handle (sides + bottom retained).
        sc.cursor_moved(PointerId::MOUSE, 395.0, 295.0);
        assert_eq!(
            sc.hover_target(PointerId::MOUSE),
            Some(pinion_overlay::WINDOW_RESIZE_SOUTH_EAST_TAG),
            "the bottom-right corner still resizes",
        );
    }

    #[test]
    fn maximized_chromeless_resizable_window_drops_the_border() {
        // R1186 — the maximized suppression holds for the NEW chrome-less
        // `Some(true)` path too (a maximized floater fills the work area; edge
        // resize is meaningless and fights the WM), on both live + mirror.
        let mut sc = ShellCore::<ResizableChromeless>::new();
        sc.set_maximized_for_window(pinion_runtime::DEFAULT_WINDOW, true);
        let live = sc.compute_paint_scene(400, 300);
        let mirror = sc.compute_paint_scene_pure(400, 300);
        for tag in SIDE_BOTTOM_RESIZE_TAGS {
            assert!(
                !has_tag(&live, tag),
                "maximized chromeless live drops {tag}"
            );
            assert!(
                !has_tag(&mirror, tag),
                "maximized chromeless mirror drops {tag}"
            );
        }
        // Restoring brings the border back — now including the top edge +
        // top-left corner (R1197), still without the top-right (close) corner.
        sc.set_maximized_for_window(pinion_runtime::DEFAULT_WINDOW, false);
        let restored = sc.compute_paint_scene(400, 300);
        for tag in SIDE_BOTTOM_RESIZE_TAGS {
            assert!(has_tag(&restored, tag), "restored chromeless regains {tag}");
        }
        assert!(
            has_tag(&restored, pinion_overlay::WINDOW_RESIZE_NORTH_TAG),
            "restored regains the top edge",
        );
        assert!(
            has_tag(&restored, pinion_overlay::WINDOW_RESIZE_NORTH_WEST_TAG),
            "restored regains the top-left corner",
        );
        assert!(
            has_tag(&restored, pinion_overlay::WINDOW_RESIZE_NORTH_EAST_TAG),
            "restored regains the (small) top-right corner too (R1198)",
        );
    }

    #[test]
    fn window_resizable_false_suppresses_border_on_chromed_window() {
        // R1186 — the inverse override: a chromed window that returns
        // `policy.resizable == Some(false)` drops the resize border even though it
        // draws chrome (a fixed-size chromed dialog). The chrome strip itself stays;
        // only the resize regions are suppressed — proving the two are decoupled.
        let mut sc = ShellCore::<NonResizableChromed>::new();
        let scene = sc.compute_paint_scene(400, 300);
        assert!(
            has_tag(&scene, pinion_overlay::WINDOW_CHROME_TAG),
            "the chrome strip is unaffected — only resize is opted out",
        );
        for tag in ALL_RESIZE_TAGS {
            assert!(
                !has_tag(&scene, tag),
                "window_resizable=Some(false) suppresses resize region {tag} despite chrome",
            );
        }
    }

    // ---- R1188 §5.16 §5.49 §2 #2 — RPC window-control drive parity ----
    //
    // A `scene/click` on a discrete window-control tag must take the same
    // routing a physical left-press takes (`AppShell::try_chrome_press`), not
    // fall into widget routing (the pre-R1188 no-op that left the R1121 "an AI
    // observes AND DRIVES the controls" contract observation-only). ShellCore
    // is headless (no winit), so the observable is the pending-control queue
    // the windowed shell executes right after `dispatch_rpc`.

    /// Drive one `scene/click {at:{x,y}}` through the REAL headless dispatch —
    /// the identical entry the stdin RPC reader feeds — so the test exercises
    /// the full parse → deferred-inbox → drain → control-detection pipeline.
    fn rpc_click<V: WidgetView>(sc: &mut ShellCore<V>, x: u32, y: u32) {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/click","params":{{"at":{{"x":{x},"y":{y}}}}},"id":1}}"#
        );
        let _ = sc.dispatch_rpc(&req, &mut |_, _| {});
    }

    #[test]
    fn rpc_click_on_chrome_controls_queues_the_window_control() {
        let mut sc = ShellCore::<Borderless>::new();
        let boot = sc.compute_paint_scene(400, 300);
        sc.finalize_frame(boot);
        // 400-wide strip, 46px buttons right-to-left: close [354,400] /
        // maximize [308,354] / minimize [262,308]; strip is 32px tall.
        rpc_click(&mut sc, 377, 16);
        assert_eq!(
            sc.take_pending_window_controls(),
            vec![("main".to_owned(), WindowControl::Close)],
            "★an RPC click on the close tag queues the control (drive parity)",
        );
        rpc_click(&mut sc, 331, 16);
        rpc_click(&mut sc, 285, 16);
        assert_eq!(
            sc.take_pending_window_controls(),
            vec![
                ("main".to_owned(), WindowControl::Maximize),
                ("main".to_owned(), WindowControl::Minimize),
            ],
            "maximize / minimize queue in arrival order",
        );
        // `take` semantics: the queue drains empty.
        assert!(sc.take_pending_window_controls().is_empty());
    }

    #[test]
    fn rpc_click_on_content_or_grip_queues_nothing() {
        let mut sc = ShellCore::<Borderless>::new();
        let boot = sc.compute_paint_scene(400, 300);
        sc.finalize_frame(boot);
        // Content center — ordinary widget routing, no control detection.
        rpc_click(&mut sc, 200, 200);
        // The grip is NOT a discrete control (a pointer-session window-move
        // gesture whose RPC peer is `scene/window_move`) — it keeps the
        // pre-R1188 fall-through.
        rpc_click(&mut sc, 150, 16);
        assert!(
            sc.take_pending_window_controls().is_empty(),
            "neither a content click nor the grip queues a window control",
        );
    }

    /// Drive one `scene/double_click {at:{x,y}}` through the REAL headless
    /// dispatch, the same entry the stdin RPC reader feeds.
    fn rpc_double_click<V: WidgetView>(sc: &mut ShellCore<V>, x: u32, y: u32) {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/double_click","params":{{"at":{{"x":{x},"y":{y}}}}},"id":1}}"#
        );
        let _ = sc.dispatch_rpc(&req, &mut |_, _| {});
    }

    /// ★★★★★ R1701 §5.16 §5.49 §2 #2 — **the title bar's double-click reaches
    /// an agent's channel, not only a mouse's.**
    ///
    /// R1701 added the gesture to the winit press path and left this drain
    /// behind, which is R1188's asymmetry arriving a second time: the gesture
    /// worked for a hand and not for the wire. Measured on a display WITH a
    /// window manager, before this arm existed — the maximize BUTTON took the
    /// window 640x420 -> 2494x1568 (so the witness moves and the measurement is
    /// meaningful), and a double-click on the bar did nothing at all.
    #[test]
    fn rpc_double_click_on_the_grip_queues_maximize_and_a_single_click_does_not() {
        let mut sc = ShellCore::<Borderless>::new();
        let boot = sc.compute_paint_scene(400, 300);
        sc.finalize_frame(boot);

        // The grip: the strip left of the three 46px buttons, 32px tall.
        rpc_click(&mut sc, 150, 16);
        assert!(
            sc.take_pending_window_controls().is_empty(),
            "one press on a title bar is still a move handle, not a control",
        );

        rpc_double_click(&mut sc, 150, 16);
        assert_eq!(
            sc.take_pending_window_controls(),
            vec![("main".to_owned(), WindowControl::Maximize)],
            "★two presses on a title bar ask to maximise, through the wire",
        );
    }

    /// ★ The negative control: the ordinal must change what the GRIP means and
    /// nothing else, or a double-click on the close button would ask to
    /// maximise instead of closing twice.
    #[test]
    fn rpc_double_click_elsewhere_means_what_a_single_click_means() {
        let mut sc = ShellCore::<Borderless>::new();
        let boot = sc.compute_paint_scene(400, 300);
        sc.finalize_frame(boot);
        rpc_double_click(&mut sc, 377, 16);
        assert_eq!(
            sc.take_pending_window_controls(),
            vec![("main".to_owned(), WindowControl::Close)],
            "a double-click on the close button still closes",
        );
        rpc_double_click(&mut sc, 200, 200);
        assert!(
            sc.take_pending_window_controls().is_empty(),
            "and content is content however many times it is pressed",
        );
    }

    #[test]
    fn rpc_click_resolves_the_canonical_spec_not_the_router_fallback() {
        // R1123.1 rule — an UNSCOPED dispatch drains via the DEFAULT_WINDOW
        // router key ("main"), but the queued control must carry the resolved
        // canonical spec id. `BorderlessPanel`'s only window is named "panel",
        // so a "main" entry here would be the fallback-string bug.
        let mut sc = ShellCore::<BorderlessPanel>::new();
        let boot = sc.compute_paint_scene(400, 300);
        sc.finalize_frame(boot);
        rpc_click(&mut sc, 377, 16);
        assert_eq!(
            sc.take_pending_window_controls(),
            vec![("panel".to_owned(), WindowControl::Close)],
            "★the queue carries the canonical spec id, not the router fallback",
        );
    }

    /// The R1187 controls-in-header shape (the sprag floater): the control tag
    /// lives in CONTENT (the dock header hosts min/max/close), the window draws
    /// NO chrome strip at all. The 40×28 close hit-box lays out at the
    /// top-left of the full-window content container.
    struct HeaderControls;
    impl WidgetCore for HeaderControls {
        type State = ButtonState;
        type Event = ButtonEvent;
        fn create_external() -> Box<dyn External> {
            Box::new(StubExternal)
        }
        fn tag() -> &'static str {
            CONTENT_TAG
        }
        fn read_state(_: &Scene) -> Self::State {
            ButtonState::Idle
        }
        fn view(_: Self::State, _: &Frame) -> Scene {
            // A Column: a 28-tall header hosting the close control, then a
            // FOCUSABLE body that fills the rest — so a click on the body
            // exercises the fall-through (widget routing → click-to-focus)
            // the interception must NOT swallow, while the close click is
            // intercepted.
            let close = Scene::Box(
                BoxNode::new(Rect::default(), BoxStyle::filled(Color::TRANSPARENT))
                    .with_tag(pinion_overlay::WINDOW_CHROME_CLOSE_TAG)
                    .with_layout(LayoutStyle::new().with_size(Size::px(40, 28))),
            );
            let body = Scene::Box(
                BoxNode::new(Rect::default(), BoxStyle::filled(Color::TRANSPARENT))
                    .with_tag("body")
                    .with_layout(
                        LayoutStyle::new()
                            .with_size(Size::px(400, 272))
                            .with_focusable(true),
                    ),
            );
            let mut c = ContainerNode::new(vec![close, body]);
            c.tag = Some(std::borrow::Cow::Borrowed(CONTENT_TAG));
            c.layout = LayoutStyle::new()
                .flex(pinion_core::style::FlexDirection::Column)
                .with_size(
                    Size::auto()
                        .with_width(SizeValue::Percent(100))
                        .with_height(SizeValue::Percent(100)),
                );
            Scene::Container(c)
        }
        fn event_name(_: Self::Event) -> &'static str {
            "__internal__"
        }
        fn title() -> &'static str {
            "HeaderControls"
        }
    }
    impl WidgetA11y for HeaderControls {}
    impl WidgetView for HeaderControls {
        type Renderer = TestRenderer;
        fn initial_size_strategy() -> SizeStrategy {
            SizeStrategy::Fixed {
                width: 400,
                height: 300,
            }
        }
        fn windows() -> Vec<WindowSpec> {
            vec![
                WindowSpec::new(
                    "main",
                    "HeaderControls",
                    SizeStrategy::Fixed {
                        width: 400,
                        height: 300,
                    },
                )
                .with_decorations(false),
            ]
        }
        fn window_policy(_window_id: &str) -> crate::WindowPolicy {
            crate::WindowPolicy::new().with_resizable(true)
        }
    }

    #[test]
    fn rpc_click_drives_content_hosted_header_controls_too() {
        // The PR-44 sprag shape: `policy.chrome == None`, the close button is a
        // CONTENT node carrying the shared control tag (R1171/R1187
        // controls-in-header). The detection is tag-driven — same as the winit
        // path — so it must fire for header-hosted controls, not only for the
        // shell-injected chrome strip.
        let mut sc = ShellCore::<HeaderControls>::new();
        let boot = sc.compute_paint_scene(400, 300);
        sc.finalize_frame(boot);
        rpc_click(&mut sc, 20, 14);
        assert_eq!(
            sc.take_pending_window_controls(),
            vec![("main".to_owned(), WindowControl::Close)],
            "★a content-hosted (dock-header) close control queues via RPC click",
        );
        // Fall-through regression guard: a click on the focusable BODY (a
        // non-control content node) must NOT be intercepted — it reaches widget
        // routing (click-to-focus focuses the body). If the interception ever
        // over-fired for non-control tags, ordinary clicks would stop working;
        // this is the observable that would catch it.
        rpc_click(&mut sc, 200, 150);
        assert_eq!(
            sc.focus().focused(),
            Some("body"),
            "★a non-control content click falls through to widget routing (focus set)",
        );
        assert!(
            sc.take_pending_window_controls().is_empty(),
            "the body click queued no window control",
        );
    }

    #[test]
    fn resize_border_is_introspectable_in_the_pure_mirror() {
        // §2 #7: the resize regions an AI drives must appear in scene/snapshot.
        let mut sc = ShellCore::<Borderless>::new();
        let mirror = sc.compute_paint_scene_pure(400, 300);
        for tag in ALL_RESIZE_TAGS {
            assert!(
                has_tag(&mirror, tag),
                "resize region {tag} is introspectable in the mirror"
            );
        }
    }

    #[test]
    fn cursor_over_resize_regions_resolves_their_tags() {
        // The same router hit-test `try_chrome_press` reads. Window 400x300,
        // chrome strip = top 32px, resize edge = 6px, corner = 12px.
        use pinion_runtime::PointerId;
        let mut sc = ShellCore::<Borderless>::new();
        let boot = sc.compute_paint_scene(400, 300);
        sc.finalize_frame(boot);

        // Each probe: (x, y, expected resize tag). (R1204) The WHOLE outer ring is
        // raised ON TOP of the chrome strip, so all four edges AND all four
        // corners resolve — including the two TOP corners (diagonal resize, which
        // R1195 left owned by the strip). The NW corner is full 12px; the NE
        // corner is edge-sized (6px) so it grazes, not shadows, the close button.
        let probes = [
            (150.0, 3.0, pinion_overlay::WINDOW_RESIZE_NORTH_TAG),
            (200.0, 297.0, pinion_overlay::WINDOW_RESIZE_SOUTH_TAG),
            (3.0, 150.0, pinion_overlay::WINDOW_RESIZE_WEST_TAG),
            (397.0, 150.0, pinion_overlay::WINDOW_RESIZE_EAST_TAG),
            (3.0, 3.0, pinion_overlay::WINDOW_RESIZE_NORTH_WEST_TAG),
            (397.0, 3.0, pinion_overlay::WINDOW_RESIZE_NORTH_EAST_TAG),
            (3.0, 295.0, pinion_overlay::WINDOW_RESIZE_SOUTH_WEST_TAG),
            (395.0, 295.0, pinion_overlay::WINDOW_RESIZE_SOUTH_EAST_TAG),
        ];
        for (x, y, tag) in probes {
            sc.cursor_moved(PointerId::MOUSE, x, y);
            assert_eq!(
                sc.hover_target(PointerId::MOUSE),
                Some(tag),
                "cursor at ({x},{y}) resolves to resize region {tag}",
            );
        }
    }

    #[test]
    fn top_edge_is_a_resize_band_over_strip() {
        // R1195/R1204 §5.16 §5.39 — VS Code / Win11 / GTK parity: a chromed title
        // bar's outermost ring resizes. R1204 completed the ring — the NORTH edge
        // (6 px) resizes vertically along the top, and the TOP CORNERS resize
        // diagonally (the NE corner edge-sized so it grazes, not shadows, the
        // close button flush to the right edge). Below the ring the strip owns
        // move + controls.
        use pinion_runtime::PointerId;
        let mut sc = ShellCore::<Borderless>::new();
        let boot = sc.compute_paint_scene(400, 300);
        sc.finalize_frame(boot);

        // The very top-right (x>=394): the NE corner — a diagonal resize (R1204;
        // R1195 left it owned by the strip).
        sc.cursor_moved(PointerId::MOUSE, 394.0, 3.0);
        assert_eq!(
            sc.hover_target(PointerId::MOUSE),
            Some(pinion_overlay::WINDOW_RESIZE_NORTH_EAST_TAG),
            "the very top-right corner is a diagonal resize, not owned by the strip",
        );
        // Along the top edge over the close button BODY (x<394): the north band
        // (resize from the top edge even over a control — drop a few px to click).
        sc.cursor_moved(PointerId::MOUSE, 370.0, 3.0);
        assert_eq!(
            sc.hover_target(PointerId::MOUSE),
            Some(pinion_overlay::WINDOW_RESIZE_NORTH_TAG),
            "the top edge over the close button body is the north resize band",
        );
        // Below the ring, the close button BODY is a normal target (the ring
        // grazes only its outer 6 px, leaving the bulk clickable).
        sc.cursor_moved(PointerId::MOUSE, 370.0, 16.0);
        assert_eq!(
            sc.hover_target(PointerId::MOUSE),
            Some(pinion_overlay::WINDOW_CHROME_CLOSE_TAG),
            "the close button body is reachable below the top resize band",
        );
        // Top 6 px over the strip background is the north resize band.
        sc.cursor_moved(PointerId::MOUSE, 150.0, 3.0);
        assert_eq!(
            sc.hover_target(PointerId::MOUSE),
            Some(pinion_overlay::WINDOW_RESIZE_NORTH_TAG),
            "the top edge over the title bar is the north resize band",
        );
        // Below the band, the title-bar grip moves the window.
        sc.cursor_moved(PointerId::MOUSE, 150.0, 16.0);
        assert_eq!(
            sc.hover_target(PointerId::MOUSE),
            Some(pinion_overlay::WINDOW_CHROME_GRIP_TAG),
            "below the band the grip moves the window",
        );
    }

    #[test]
    fn content_center_is_not_shadowed_by_the_resize_border() {
        // The flat-sibling design: a click in the window center falls through
        // the full-window resize border to the content (a full-window resize
        // container would absorb it).
        use pinion_runtime::PointerId;
        let mut sc = ShellCore::<Borderless>::new();
        let boot = sc.compute_paint_scene(400, 300);
        sc.finalize_frame(boot);
        sc.cursor_moved(PointerId::MOUSE, 200.0, 150.0);
        let hover = sc.hover_target(PointerId::MOUSE);
        for tag in ALL_RESIZE_TAGS {
            assert_ne!(
                hover,
                Some(tag),
                "center is not snagged by resize region {tag}"
            );
        }
    }

    // ---- R1123 §5.16 §5.39 maximized-state threading ----

    #[test]
    fn window_maximized_defaults_false_and_roundtrips() {
        let mut sc = ShellCore::<Borderless>::new();
        assert!(
            !sc.maximized_for_window(pinion_runtime::DEFAULT_WINDOW),
            "an unknown / never-resized window is not maximized",
        );
        sc.set_maximized_for_window(pinion_runtime::DEFAULT_WINDOW, true);
        assert!(sc.maximized_for_window(pinion_runtime::DEFAULT_WINDOW));
        sc.set_maximized_for_window(pinion_runtime::DEFAULT_WINDOW, false);
        assert!(!sc.maximized_for_window(pinion_runtime::DEFAULT_WINDOW));
    }

    #[test]
    fn maximized_suppresses_resize_border_in_live_and_mirror() {
        // §2 #7: the maximized state drops the resize border identically on the
        // live paint and the pure introspection mirror (both read the cache).
        let mut sc = ShellCore::<Borderless>::new();
        sc.set_maximized_for_window(pinion_runtime::DEFAULT_WINDOW, true);
        let live = sc.compute_paint_scene(400, 300);
        let mirror = sc.compute_paint_scene_pure(400, 300);
        for tag in ALL_RESIZE_TAGS {
            assert!(
                !has_tag(&live, tag),
                "maximized live paint drops resize {tag}"
            );
            assert!(
                !has_tag(&mirror, tag),
                "maximized mirror drops resize {tag}"
            );
        }
        // The chrome strip itself stays (a maximized window still has controls).
        assert!(
            has_tag(&live, pinion_overlay::WINDOW_CHROME_TAG),
            "chrome stays when maximized"
        );

        // Restoring brings the resize border back.
        sc.set_maximized_for_window(pinion_runtime::DEFAULT_WINDOW, false);
        let restored = sc.compute_paint_scene(400, 300);
        for tag in ALL_RESIZE_TAGS {
            assert!(
                has_tag(&restored, tag),
                "restored window regains resize {tag}"
            );
        }
    }

    #[test]
    fn maximized_chrome_draws_the_restore_glyph() {
        // The threading proof: the maximize button's glyph Path in the PAINTED
        // scene switches from the maximize square (5 commands) to the restore
        // two-square (10) when the cache is maximized — i.e. the cached flag
        // reaches `inject_window_chrome`.
        let mut sc = ShellCore::<Borderless>::new();
        let normal = sc.compute_paint_scene(400, 300);
        let max_rect = rect_of(&normal, pinion_overlay::WINDOW_CHROME_MAXIMIZE_TAG)
            .expect("maximize button laid out");
        assert_eq!(
            path_command_count_at(&normal, max_rect),
            Some(5),
            "default chrome draws the maximize glyph",
        );

        sc.set_maximized_for_window(pinion_runtime::DEFAULT_WINDOW, true);
        let maxed = sc.compute_paint_scene(400, 300);
        assert_eq!(
            path_command_count_at(&maxed, max_rect),
            Some(10),
            "maximized chrome draws the restore glyph",
        );
    }

    #[test]
    fn maximized_glyph_parity_when_primary_window_is_not_named_main() {
        // R1123.1 §2 #7 — the primary paint (window_id == None, used by
        // `compute_paint_scene` + the pure mirror) must resolve the SAME window
        // the per-window paint (Some(id)) does, so the maximized glyph agrees.
        // The only window here is "panel" (not "main"); keying the maximized
        // cache by `DEFAULT_WINDOW` instead of the resolved spec id would make
        // the None paint miss the cache and draw the wrong glyph — the latent
        // live-vs-mirror divergence this round closed.
        let mut sc = ShellCore::<BorderlessPanel>::new();
        sc.set_maximized_for_window("panel", true);

        let none_paint = sc.compute_paint_scene(400, 300);
        let some_paint = sc.compute_paint_scene_for_window("panel", 400, 300);
        let max_rect = rect_of(&none_paint, pinion_overlay::WINDOW_CHROME_MAXIMIZE_TAG)
            .expect("maximize button laid out");

        assert_eq!(
            path_command_count_at(&none_paint, max_rect),
            Some(10),
            "the None (primary / mirror) paint resolves 'panel' and draws restore",
        );
        assert_eq!(
            path_command_count_at(&some_paint, max_rect),
            path_command_count_at(&none_paint, max_rect),
            "the Some(id) live paint and the None primary paint agree (§2 #7)",
        );
        for tag in ALL_RESIZE_TAGS {
            assert!(
                !has_tag(&none_paint, tag),
                "None paint drops resize {tag} when maximized"
            );
            assert!(
                !has_tag(&some_paint, tag),
                "Some paint drops resize {tag} when maximized"
            );
        }
    }

    #[test]
    fn closing_a_window_clears_its_maximized_cache() {
        // R1123.1 — remove_window must drop the maximized entry, else it leaks
        // and a reused window id inherits stale maximized state. (remove_window
        // refuses DEFAULT_WINDOW, so this uses a secondary id.)
        let mut sc = ShellCore::<BorderlessPanel>::new();
        sc.set_maximized_for_window("torn-1", true);
        assert!(sc.maximized_for_window("torn-1"));
        sc.remove_window("torn-1");
        assert!(
            !sc.maximized_for_window("torn-1"),
            "the maximized entry is cleared when the window closes",
        );
    }

    /// PR-55 refutation fixture — a binding whose view draws its OWN focus
    /// indicator (an accent Box inside the focused pane), driven ONLY by
    /// [`pinion_core::focus_state::focused`] (R1327 / R1335), with the framework
    /// focus ring turned OFF (`focus_ring_style -> None`). This is sprag R142's
    /// exact shape: two focusable panes, no framework ring, a view-drawn accent.
    struct FocusPanes;
    impl WidgetCore for FocusPanes {
        type State = ButtonState;
        type Event = ButtonEvent;
        fn create_external() -> Box<dyn External> {
            Box::new(StubExternal)
        }
        fn tag() -> &'static str {
            "root"
        }
        fn read_state(_: &Scene) -> Self::State {
            ButtonState::Idle
        }
        fn view(_: Self::State, _: &Frame) -> Scene {
            // The ONLY focus source a view fn has — and the exact API PR-55
            // claims returns `None` in the RPC produce path.
            let focused = pinion_core::focus_state::focused();
            let make_pane = |tag: &str| {
                let mut children: Vec<Scene> = Vec::new();
                if focused.as_deref() == Some(tag) {
                    let mut accent = BoxNode::new(
                        Rect::new(0, 0, 0, 0),
                        BoxStyle::filled(Color::rgb(0, 128, 255)),
                    );
                    accent.tag = Some(std::borrow::Cow::Owned(format!("accent-{tag}")));
                    children.push(Scene::Box(accent));
                }
                let mut c = ContainerNode::new(children);
                c.tag = Some(std::borrow::Cow::Owned(tag.to_owned()));
                c.layout = LayoutStyle::new().with_focusable(true).with_size(
                    Size::auto()
                        .with_width(SizeValue::Percent(50))
                        .with_height(SizeValue::Percent(100)),
                );
                Scene::Container(c)
            };
            let mut root = ContainerNode::new(vec![make_pane("pane.0"), make_pane("pane.1")]);
            root.tag = Some(std::borrow::Cow::Borrowed("root"));
            root.layout = LayoutStyle::new().with_size(
                Size::auto()
                    .with_width(SizeValue::Percent(100))
                    .with_height(SizeValue::Percent(100)),
            );
            Scene::Container(root)
        }
        fn event_name(_: Self::Event) -> &'static str {
            "__internal__"
        }
        fn title() -> &'static str {
            "Focus Panes"
        }
    }
    impl WidgetA11y for FocusPanes {}
    impl WidgetView for FocusPanes {
        type Renderer = TestRenderer;
        fn initial_size_strategy() -> SizeStrategy {
            SizeStrategy::Fixed {
                width: 400,
                height: 300,
            }
        }
        // sprag's exact config — the framework ring is OFF; the ONLY focus
        // indicator is the view-drawn accent (driven by `focus_state::focused()`).
        fn focus_ring_style(_focused_tag: &str) -> Option<crate::FocusRingStyle> {
            None
        }
    }

    /// Does `scene` (a `scene/snapshot` JSON response, or a live scene) carry a
    /// child tagged `accent-<pane>`? Used to assert the focus-dependent accent
    /// survived a given producer.
    fn contains_accent(scene: &Scene, pane: &str) -> bool {
        has_tag(scene, &format!("accent-{pane}"))
    }

    /// R1343 §5.39 §2 #7 (PR-55 refutation) — a view-drawn focus indicator
    /// (`focus_state::focused()` → an accent inside the focused pane) is present
    /// in EVERY paint-scene producer, byte-for-byte with the `FocusManager`. PR-55
    /// reported `focus_state::focused()` returns `None` in the RPC
    /// `scene/snapshot from: paint` / `scene/screenshot` produce path — a
    /// producer-parity break where the framework ring shows in a snapshot but the
    /// binding's own focus-dependent render does not. This test drives all three
    /// producers and proves the opposite: R1335 published the mirror on the
    /// binding's owner (funnelled through `FocusManager::commit_focus`), which
    /// every producer runs the view under (`root_owner.run`), so the read is
    /// seeded identically on each. The premise predates R1335's owner-scoped
    /// mirror (it matches the retired R1327 thread-local, which a per-thread RPC
    /// probe COULD miss); on the current substrate there is nothing to seed.
    #[test]
    fn r1343_view_drawn_focus_indicator_survives_every_producer() {
        // Producer 1 — the WINIT paint path (`compute_paint_scene_for_window`),
        // which is exactly what `scene/screenshot` re-renders through
        // (`AppShell::render_window`). Faithful click-to-focus.
        let mut sc = ShellCore::<FocusPanes>::new();
        let boot = sc.compute_paint_scene(400, 300);
        sc.finalize_frame(boot);
        rpc_click(&mut sc, 100, 150); // left half = pane.0
        assert_eq!(
            sc.focus().focused(),
            Some("pane.0"),
            "click committed focus on pane.0 (the FocusManager SSOT)",
        );
        assert_eq!(
            sc.root_owner()
                .run(pinion_core::focus_state::focused)
                .as_deref(),
            Some("pane.0"),
            "the owner mirror a view reads agrees with the manager under root_owner",
        );
        let winit = sc.compute_paint_scene_for_window("main", 400, 300);
        assert!(
            contains_accent(&winit, "pane.0"),
            "★the WINIT render path (= the scene/screenshot re-render) draws the \
             view's focus accent — PR-55's screenshot-missing-accent premise fails here",
        );
        assert!(
            !contains_accent(&winit, "pane.1"),
            "the unfocused pane draws no accent (negative control)",
        );

        // Producer 2 — the STORED paint scene `scene/snapshot from: paint`
        // serializes (the last finalized winit frame). It re-renders nothing;
        // it must already carry the accent the winit frame drew.
        sc.finalize_frame(winit);
        let stored = r#"{"jsonrpc":"2.0","method":"scene/snapshot","params":{"path":"","from":"paint","viewport":{"w":400,"h":300}},"id":1}"#;
        let stored_resp = sc.dispatch_rpc(stored, &mut |_, _| {}).unwrap_or_default();
        assert!(
            stored_resp.contains("accent-pane.0"),
            "★scene/snapshot from:paint (stored frame) carries the accent: {stored_resp}",
        );

        // Producer 3 — the RPC produce CLOSURE itself. A fresh shell whose focus
        // is set but which never finalized a frame: `scene/snapshot from: paint`
        // then falls back to running the produce closure and snapshots THAT
        // scene. This is the exact `dispatch_rpc` produce path PR-55 blames.
        let mut sc2 = ShellCore::<FocusPanes>::new();
        let set = r#"{"jsonrpc":"2.0","method":"focus/set","params":{"tag":"pane.0"},"id":1}"#;
        let _ = sc2.dispatch_rpc(set, &mut |_, _| {});
        let produced = r#"{"jsonrpc":"2.0","method":"scene/snapshot","params":{"path":"","from":"paint","viewport":{"w":400,"h":300}},"id":2}"#;
        let produced_resp = sc2
            .dispatch_rpc(produced, &mut |_, _| {})
            .unwrap_or_default();
        assert!(
            produced_resp.contains("accent-pane.0"),
            "★the RPC produce closure draws the accent (focus_state seeded there too): {produced_resp}",
        );
    }

    /// Command count of the first `Scene::Path` whose rect equals `rect` (the
    /// glyph Path that `push_control` lays under a chrome button's hit Box).
    fn path_command_count_at(scene: &Scene, rect: Rect) -> Option<usize> {
        match scene {
            Scene::Path(p) if p.rect == rect => Some(p.commands.len()),
            Scene::Container(c) => c
                .children
                .iter()
                .find_map(|ch| path_command_count_at(ch, rect)),
            Scene::Scroll(s) => path_command_count_at(&s.content, rect),
            _ => None,
        }
    }
}

#[cfg(test)]
mod r1467_producer_dresses_the_window_tests {
    //! R1467 §5.16 §5.39 §2 #7 — the RPC produce closure is the THIRD
    //! paint-scene producer, and pre-R1467 it was the only one that did not
    //! dress the window it produced: no chrome strip, no resize border, no
    //! raised top edge, no cross-window drop preview — and, because the content
    //! inset rides the same policy read as the strip, no inset either.
    //!
    //! Two consumers read that producer, and both were wrong for a chromed
    //! window:
    //!
    //! * `scene/snapshot from: paint` / `scene/layout {viewport}` fall back to
    //!   it whenever the addressed window has stored no frame yet (a headless
    //!   agent driving a binding before the first winit paint), so §2 #7's
    //!   "introspection equals the displayed frame" reported a window with no
    //!   title bar and content sitting where the title bar is.
    //! * `scene/click {path}` / `scene/hover {path}` ALWAYS resolve their
    //!   coordinate through it (`resolve_path_to_center` runs the producer; it
    //!   reads the stored frame only for the viewport size), and then the drain
    //!   hit-tests that coordinate against the STORED — inset, chromed — scene.
    //!   So a path click on a chromed window landed
    //!   [`WindowChromeStyle::height_px`] above the widget it named. For
    //!   anything in the top strip's height that is not a near miss: the
    //!   coordinate lands ON the chrome, where R1188's window-control
    //!   interception reads it as a title-bar grip.
    //!
    //! The existing R1121 module pins the same claims for the winit paint and
    //! the pure mirror; nothing pinned them for the produce path, which is why
    //! the whole `pinion-shell` suite stayed green across the defect.
    use crate::test_fixtures::TestRenderer;
    use crate::{ShellCore, SizeStrategy, WidgetView, WindowChromeStyle, WindowSpec};
    use pinion_a11y::WidgetA11y;
    use pinion_core::external::{External, StubExternal};
    use pinion_core::scene::{ContainerNode, Rect, Scene};
    use pinion_core::style::{FlexDirection, LayoutStyle, Size, SizeValue};
    use pinion_core::widgets::button::{ButtonEvent, ButtonState};
    use pinion_core::{Frame, WidgetCore};
    use pinion_runtime::PointerId;

    const CHROME_H: u32 = 32; // WindowChromeStyle::default().height_px
    const ROW_H: u32 = 40;
    const ROWS: [&str; 3] = ["r1467-row0", "r1467-row1", "r1467-row2"];
    const W: u32 = 400;
    const H: u32 = 300;

    /// Three FIXED-height rows stacked at the top of the window. Fixed height is
    /// what makes the fixture discriminating: the chrome inset shifts every row
    /// down by a whole `CHROME_H`, which is `CHROME_H / ROW_H` of a row — so a
    /// producer that forgets the inset resolves row 0 onto the chrome strip and
    /// each later row onto its predecessor. A percentage-height layout would
    /// merely squash, and every row centre would still land inside its own row.
    fn rows_view() -> Scene {
        let rows = ROWS
            .iter()
            .map(|tag| {
                Scene::Container(
                    ContainerNode::new(Vec::new())
                        .with_tag((*tag).to_owned())
                        .with_layout(
                            LayoutStyle::new().with_size(
                                Size::auto()
                                    .with_width(SizeValue::Percent(100))
                                    .with_height(SizeValue::Px(ROW_H)),
                            ),
                        ),
                )
            })
            .collect();
        Scene::Container(
            ContainerNode::new(rows).with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
        )
    }

    macro_rules! rows_fixture {
        ($name:ident, $chrome:expr) => {
            struct $name;
            impl WidgetCore for $name {
                type State = ButtonState;
                type Event = ButtonEvent;
                fn create_external() -> Box<dyn External> {
                    Box::new(StubExternal)
                }
                fn tag() -> &'static str {
                    ROWS[0]
                }
                fn read_state(_: &Scene) -> Self::State {
                    ButtonState::Idle
                }
                fn view(_: Self::State, _: &Frame) -> Scene {
                    rows_view()
                }
                fn event_name(_: Self::Event) -> &'static str {
                    "__internal__"
                }
                fn title() -> &'static str {
                    "Rows"
                }
            }
            impl WidgetA11y for $name {}
            impl WidgetView for $name {
                type Renderer = TestRenderer;
                fn initial_size_strategy() -> SizeStrategy {
                    SizeStrategy::Fixed {
                        width: W,
                        height: H,
                    }
                }
                fn windows() -> Vec<WindowSpec> {
                    vec![
                        WindowSpec::new(
                            "main",
                            "Rows",
                            SizeStrategy::Fixed {
                                width: W,
                                height: H,
                            },
                        )
                        .with_decorations($chrome.is_none()),
                    ]
                }
                fn window_policy(_window_id: &str) -> crate::WindowPolicy {
                    crate::WindowPolicy {
                        chrome: $chrome,
                        resizable: None,
                    }
                }
            }
        };
    }
    rows_fixture!(ChromedRows, Some(WindowChromeStyle::default()));
    // Negative control: the same rows in an OS-decorated window, where there is
    // no inset to forget. Every claim below must hold here BOTH before and after
    // R1467 — it is what proves the fix is about chrome and not about the
    // producer generally.
    rows_fixture!(DecoratedRows, Option::<WindowChromeStyle>::None);

    fn has_tag(scene: &Scene, tag: &str) -> bool {
        if scene.tag() == Some(tag) {
            return true;
        }
        match scene {
            Scene::Container(c) => c.children.iter().any(|ch| has_tag(ch, tag)),
            Scene::Scroll(s) => has_tag(&s.content, tag),
            _ => false,
        }
    }

    /// Snapshot the produce path: a shell that has stored NO frame, so
    /// `scene/snapshot from: paint` falls through to the producer.
    fn produced_snapshot<V: WidgetView>(sc: &mut ShellCore<V>) -> String {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/snapshot","params":{{"path":"","from":"paint","viewport":{{"w":{W},"h":{H}}}}},"id":1}}"#
        );
        sc.dispatch_rpc(&req, &mut |_, _| {}).unwrap_or_default()
    }

    /// `scene/hover {path}`: the producer resolves the tag's centre, the drain
    /// hit-tests that point against the STORED scene. The returned tag is what
    /// the coordinate actually landed on — the whole round in one read.
    fn hover_by_path<V: WidgetView>(sc: &mut ShellCore<V>, tag: &str) -> Option<String> {
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/hover","params":{{"path":"{tag}"}},"id":1}}"#
        );
        let _ = sc.dispatch_rpc(&req, &mut |_, _| {});
        sc.hover_target(PointerId::MOUSE).map(str::to_owned)
    }

    fn rect_of<V: WidgetView>(sc: &mut ShellCore<V>, tag: &str) -> Rect {
        sc.compute_paint_scene(W, H)
            .rect_for_tag_absolute(tag)
            .unwrap_or_else(|| panic!("{tag} laid out"))
    }

    #[test]
    fn a_path_hover_on_a_chromed_window_lands_on_the_widget_it_named() {
        // The consumer-visible half. The window has painted, so the drain
        // hit-tests a real chromed frame; the coordinate still comes from the
        // producer, which is the half R1467 fixed.
        let mut sc = ShellCore::<ChromedRows>::new();
        let boot = sc.compute_paint_scene(W, H);
        sc.finalize_frame(boot);
        // Row 0's painted band is [CHROME_H, CHROME_H + ROW_H); an un-inset
        // producer puts its centre at ROW_H / 2 = 20, which is INSIDE the 32px
        // chrome strip — so pre-R1467 this resolved the title-bar grip and the
        // click became a window move.
        assert_eq!(
            hover_by_path(&mut sc, ROWS[0]).as_deref(),
            Some(ROWS[0]),
            "★a path hover on the top row lands on the row, not on the chrome strip",
        );
        // The later rows fail one step less dramatically: each resolves onto its
        // PREDECESSOR, because CHROME_H (32) is under a full row (40).
        for (i, tag) in ROWS.iter().enumerate() {
            assert_eq!(
                hover_by_path(&mut sc, tag).as_deref(),
                Some(*tag),
                "★row {i} resolves to itself, not to the row above it",
            );
        }
    }

    #[test]
    fn a_path_hover_on_a_decorated_window_is_unchanged() {
        // Negative control: no chrome, no inset, nothing for the producer to
        // forget. Green before AND after R1467.
        let mut sc = ShellCore::<DecoratedRows>::new();
        let boot = sc.compute_paint_scene(W, H);
        sc.finalize_frame(boot);
        for tag in ROWS {
            assert_eq!(
                hover_by_path(&mut sc, tag).as_deref(),
                Some(tag),
                "a decorated window's path hover resolves to {tag}",
            );
        }
    }

    /// The first node in a produced scene document carrying `tag`.
    ///
    /// R1674 — so a claim about one node's geometry is read out of that node
    /// instead of matched against a rendering of its siblings' style fields.
    fn find_tagged<'a>(node: &'a serde_json::Value, tag: &str) -> Option<&'a serde_json::Value> {
        if node["tag"].as_str() == Some(tag) {
            return Some(node);
        }
        node["children"]
            .as_array()?
            .iter()
            .find_map(|child| find_tagged(child, tag))
    }

    #[test]
    fn the_produce_path_insets_a_chromed_window_like_the_paint_does() {
        // §2 #7 for the geometry: the scene an agent reads from the produce
        // fallback must be laid out where the window paints it.
        let mut sc = ShellCore::<ChromedRows>::new();
        let painted = rect_of(&mut sc, ROWS[0]);
        assert_eq!(
            painted.y, CHROME_H,
            "premise: the painted top row sits below the strip",
        );
        let mut fresh = ShellCore::<ChromedRows>::new();
        let resp = produced_snapshot(&mut fresh);
        // State the claim as the ROW's own rect, not as a substring of the
        // response: the outermost overlay container legitimately spans the whole
        // window, so "no full-window rect appears" is a different (and false)
        // claim than "the content is inset" — the first shape of this assertion
        // said the second and tested the first.
        //
        // ★ R1674 — and read out of the PARSED response rather than matched as
        // text. The second shape of this assertion pinned the row's whole
        // serialized `style` object as an anchor to disambiguate the rect, so
        // adding any field to `BoxStyle` broke a test about window chrome for
        // reasons that had nothing to do with window chrome — measured, when
        // `chrome` was added. A node is found by its tag; its rect is then a
        // value, not a substring.
        let doc: serde_json::Value =
            serde_json::from_str(&resp).expect("the produce path answers JSON");
        let row = find_tagged(&doc["result"], ROWS[0])
            .unwrap_or_else(|| panic!("the produced scene carries {}: {resp}", ROWS[0]));
        assert_eq!(
            (row["rect"]["y"].as_u64(), row["rect"]["h"].as_u64()),
            (Some(u64::from(CHROME_H)), Some(u64::from(ROW_H))),
            "★the produce path insets the top row below the chrome strip: {resp}",
        );
        assert!(
            !resp.contains(&format!(r#""rect":{{"h":{ROW_H},"w":{W},"x":0,"y":0}}"#)),
            "★no row is laid out at y=0 any more (the un-inset shape): {resp}",
        );
    }

    #[test]
    fn the_produce_path_dresses_a_chromed_window_with_its_controls() {
        // R1121 claims the chrome buttons are "real, introspectable Scene nodes
        // an AI agent observes and drives via scene/snapshot". Pre-R1467 that
        // was true only of a window that had already painted; an agent driving a
        // fresh binding saw a window with no title bar at all.
        let mut sc = ShellCore::<ChromedRows>::new();
        let resp = produced_snapshot(&mut sc);
        for tag in [
            pinion_overlay::WINDOW_CHROME_TAG,
            pinion_overlay::WINDOW_CHROME_CLOSE_TAG,
            pinion_overlay::WINDOW_CHROME_MINIMIZE_TAG,
            pinion_overlay::WINDOW_CHROME_MAXIMIZE_TAG,
            pinion_overlay::WINDOW_CHROME_GRIP_TAG,
            pinion_overlay::WINDOW_RESIZE_NORTH_TAG,
            pinion_overlay::WINDOW_RESIZE_SOUTH_EAST_TAG,
        ] {
            assert!(
                resp.contains(tag),
                "★the produce path carries {tag}: {resp}",
            );
        }
    }

    #[test]
    fn the_produce_path_leaves_a_decorated_window_naked() {
        // The gate is the policy, not the producer: a decorated window must gain
        // nothing. Without this, "dress everything" would pass the tests above.
        let mut sc = ShellCore::<DecoratedRows>::new();
        let resp = produced_snapshot(&mut sc);
        assert!(
            !resp.contains(pinion_overlay::WINDOW_CHROME_TAG),
            "a decorated window's produce path injects no chrome: {resp}",
        );
        assert!(
            !resp.contains(pinion_overlay::WINDOW_RESIZE_NORTH_TAG),
            "…and no resize border: {resp}",
        );
        assert!(
            resp.contains(r#""y":0"#),
            "…and no inset (the top row still starts at y=0): {resp}",
        );
    }

    #[test]
    fn all_three_producers_agree_on_a_chromed_window() {
        // The invariant the three previous tests are instances of, stated once:
        // the winit paint, the introspection mirror and the RPC produce closure
        // dress the same window the same way. R1467 made this true by giving all
        // three ONE chain (`WindowOverlayInputs::apply`) instead of two callers
        // of a `&self` method plus a hand-rolled partial copy behind the borrow
        // split.
        let mut sc = ShellCore::<ChromedRows>::new();
        let winit = sc.compute_paint_scene(W, H);
        let mirror = sc.compute_paint_scene_pure(W, H);
        for tag in ROWS {
            assert_eq!(
                winit.rect_for_tag_absolute(tag),
                mirror.rect_for_tag_absolute(tag),
                "paint and mirror agree on {tag} (the pre-R1467 pair)",
            );
        }
        let mut fresh = ShellCore::<ChromedRows>::new();
        let resp = produced_snapshot(&mut fresh);
        for tag in ROWS {
            let r = winit
                .rect_for_tag_absolute(tag)
                .unwrap_or_else(|| panic!("{tag} painted"));
            assert!(
                resp.contains(&format!(
                    r#""h":{},"w":{},"x":{},"y":{}"#,
                    r.h, r.w, r.x, r.y
                )),
                "★the produce path agrees with the paint on {tag} ({r:?}): {resp}",
            );
        }
        assert!(
            has_tag(&winit, pinion_overlay::WINDOW_CHROME_TAG)
                && has_tag(&mirror, pinion_overlay::WINDOW_CHROME_TAG)
                && resp.contains(pinion_overlay::WINDOW_CHROME_TAG),
            "…and all three carry the strip",
        );
    }
}

#[cfg(test)]
mod r1468_one_geometry_tests {
    //! R1468 §5.23 §5.22 §5.16 §2 #3 §2 #7 — an introspection paint answers in
    //! ONE geometry, and cannot change the world it was asked about.
    //!
    //! `scene/layout {viewport}` asks a hypothetical: lay this binding out as
    //! if the window were W×H. A binding can express an extent-derived size two
    //! equally-correct ways — through the layout engine (`height: 50%`) or
    //! through the R1006 reactive seam (`use_viewport_size`, which exists
    //! because a reflow is a side effect and so must be readable from an
    //! `Effect`). Pre-R1468 taffy was handed the hypothetical extent while the
    //! seam was never republished, so the two halves of one answer described
    //! two different windows — and R1467 had just moved the window chrome and
    //! its content inset onto the hypothetical side of that split.
    //!
    //! Publishing the hypothetical is only safe if it is contained, so the
    //! mirror runs inside [`pinion_runtime::IntrospectionPaint`]: the live
    //! extent is restored on drop, and the run is a `SimulationGuard` scope so
    //! the republish cannot fire a reflow `Effect` at a size no window has.
    //!
    //! That scope closes a second, independently-measured leak. The shell's
    //! mirror runs AFTER the dispatch's own focus-mailbox drain, so a
    //! `focus_request` written by reactive code the mirror reached sat pending
    //! and was applied by whichever LATER, unrelated RPC drained next. Both
    //! mailboxes now honour `is_simulating()`, so a paint nobody is looking at
    //! cannot queue a real focus move.
    use crate::test_fixtures::TestRenderer;
    use crate::{ShellCore, SizeStrategy, WidgetView};
    use pinion_a11y::WidgetA11y;
    use pinion_core::external::{External, StubExternal};
    use pinion_core::scene::{ContainerNode, Scene};
    use pinion_core::style::{FlexDirection, LayoutStyle, Size, SizeValue};
    use pinion_core::widgets::button::{ButtonEvent, ButtonState};
    use pinion_core::{Effect, Frame, WidgetCore, use_viewport_size};
    use std::cell::Cell;
    use std::rc::Rc;

    const W: u32 = 400;
    const H: u32 = 300;
    /// Deliberately not a multiple of [`H`], so a "half the window" child
    /// measured against the wrong extent cannot coincide with the right one.
    const HYPOTHETICAL_H: u32 = 1200;
    const BY_SEAM: &str = "r1468_by_seam";
    const BY_ENGINE: &str = "r1468_by_engine";

    thread_local! {
        /// One-shot: armed by a test, consumed by the next `V::view` run, so a
        /// single request is attributable to a single producer run.
        static ARMED_REQUEST: Cell<Option<&'static str>> = const { Cell::new(None) };
    }

    /// Half the window, expressed the two ways a binding can express it. Both
    /// are legitimate; the round is about them agreeing.
    fn half_and_half() -> Scene {
        let (_, vh) = use_viewport_size();
        let sized = |tag: &str, h: SizeValue| {
            Scene::Container(
                ContainerNode::new(Vec::new())
                    .with_tag(tag.to_owned())
                    .with_layout(
                        LayoutStyle::new().with_focusable(true).with_size(
                            Size::auto()
                                .with_width(SizeValue::Percent(100))
                                .with_height(h),
                        ),
                    ),
            )
        };
        Scene::Container(
            ContainerNode::new(vec![
                sized(BY_SEAM, SizeValue::Px(vh / 2)),
                sized(BY_ENGINE, SizeValue::Percent(50)),
            ])
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
        )
    }

    struct HalfView;
    impl WidgetCore for HalfView {
        type State = ButtonState;
        type Event = ButtonEvent;
        fn create_external() -> Box<dyn External> {
            Box::new(StubExternal)
        }
        fn tag() -> &'static str {
            BY_SEAM
        }
        fn read_state(_: &Scene) -> Self::State {
            ButtonState::Idle
        }
        fn view(_: Self::State, _: &Frame) -> Scene {
            // The forcing consumer for the mailbox half: reactive code reached
            // from a view CAN write the focus mailbox (an `Effect` created here
            // is a documented call site), so the containment must survive it.
            if let Some(tag) = ARMED_REQUEST.with(Cell::take) {
                pinion_core::focus_request::request(tag);
            }
            half_and_half()
        }
        fn event_name(_: Self::Event) -> &'static str {
            "__internal__"
        }
        fn title() -> &'static str {
            "Half"
        }
    }
    impl WidgetA11y for HalfView {}
    impl WidgetView for HalfView {
        type Renderer = TestRenderer;
        fn initial_size_strategy() -> SizeStrategy {
            SizeStrategy::Fixed {
                width: W,
                height: H,
            }
        }
    }

    fn booted() -> ShellCore<HalfView> {
        let _ = pinion_core::focus_request::drain();
        ARMED_REQUEST.with(|c| c.set(None));
        let mut sc = ShellCore::<HalfView>::new();
        let boot = sc.compute_paint_scene(W, H);
        sc.finalize_frame(boot);
        sc
    }

    fn rpc(sc: &mut ShellCore<HalfView>, req: &str) -> String {
        sc.dispatch_rpc(req, &mut |_, _| {}).unwrap_or_default()
    }

    /// `scene/layout` at the caller's hypothetical extent (runs the producer).
    fn layout_at(sc: &mut ShellCore<HalfView>, h: u32) -> String {
        rpc(
            sc,
            &format!(
                r#"{{"jsonrpc":"2.0","method":"scene/layout","params":{{"path":"","viewport":{{"width":{W},"height":{h}}}}},"id":1}}"#
            ),
        )
    }

    /// The reported height of the tagged node in a `scene/layout` answer.
    fn height_of(resp: &str, tag: &str) -> u32 {
        let at = resp
            .find(&format!(r#""tag":"{tag}""#))
            .unwrap_or_else(|| panic!("{tag} present in {resp}"));
        let rect = resp[..at]
            .rfind(r#""rect":{"h":"#)
            .unwrap_or_else(|| panic!("{tag} carries a rect in {resp}"));
        let tail = &resp[rect + r#""rect":{"h":"#.len()..];
        let end = tail.find(',').expect("h is followed by w");
        tail[..end].parse().expect("h is a number")
    }

    #[test]
    fn a_hypothetical_viewport_answers_in_one_geometry() {
        let mut sc = booted();
        let resp = layout_at(&mut sc, HYPOTHETICAL_H);
        let (seam, engine) = (height_of(&resp, BY_SEAM), height_of(&resp, BY_ENGINE));
        assert_eq!(
            engine,
            HYPOTHETICAL_H / 2,
            "premise: the layout engine already answered in the hypothetical",
        );
        assert_eq!(
            seam, engine,
            "★both halves of one answer describe the same window: {resp}",
        );
        assert_ne!(
            seam,
            H / 2,
            "★…and specifically not the LIVE extent, which is what the \
             unpublished seam used to report",
        );
    }

    #[test]
    fn the_live_extent_survives_the_hypothetical_question() {
        let mut sc = booted();
        let before = layout_at(&mut sc, H);
        let _ = layout_at(&mut sc, HYPOTHETICAL_H);
        let after = layout_at(&mut sc, H);
        assert_eq!(
            height_of(&before, BY_SEAM),
            H / 2,
            "premise: at the live extent the seam reports the live half",
        );
        assert_eq!(
            height_of(&after, BY_SEAM),
            height_of(&before, BY_SEAM),
            "★asking about another size does not leave the binding believing it",
        );
        // The paint the user is actually shown is the claim that matters, so
        // state it against the paint path and not only against another query.
        let painted = sc.compute_paint_scene(W, H);
        assert_eq!(
            painted.rect_for_tag_absolute(BY_SEAM).map(|r| r.h),
            Some(H / 2),
            "★and the next real paint is unchanged by the question",
        );
    }

    #[test]
    fn a_question_does_not_fire_the_reflow_effect() {
        let mut sc = booted();
        let runs = Rc::new(Cell::new(0_u32));
        let r = Rc::clone(&runs);
        // The R1006 consumer shape: a side-effecting reflow (a PTY's winsize
        // ioctl) subscribed to the seam. It must not be driven at a size no
        // window has — that is what makes republishing need a guard at all.
        //
        // Registered INSIDE `root_owner.run` because R1006 blocker B is not a
        // formality: `use_viewport_size` resolves `Owner::current`, which reads
        // the owner-handle stack, and the eager first run has none of its own.
        let owner = sc.root_owner().clone();
        let _reflow = owner.run(|| {
            Effect::new(&owner, move || {
                let _ = use_viewport_size();
                r.set(r.get() + 1);
            })
        });
        let at_registration = runs.get();
        assert!(at_registration >= 1, "premise: the Effect subscribed");
        let _ = layout_at(&mut sc, HYPOTHETICAL_H);
        assert_eq!(
            runs.get(),
            at_registration,
            "★neither the hypothetical publish nor its restore reflows",
        );
        // This is the claim the RESTORE carries, and the reason "a later
        // publisher overwrites it anyway" is not an argument for dropping it.
        // The live paint publishes its own extent BEFORE the view runs, outside
        // any guard. If the seam were still holding the hypothetical, that
        // publish would be a CHANGE rather than an equality-skip — so the
        // reflow the guard suppressed would simply fire one frame later, at the
        // first place the guard is not watching.
        let paint = sc.compute_paint_scene(W, H);
        sc.finalize_frame(paint);
        assert_eq!(
            runs.get(),
            at_registration,
            "★…and the next LIVE paint does not reflow either: the question left \
             the seam where it found it",
        );
        // Scoped, not sticky: a REAL resize still reaches the same Effect.
        sc.core.set_viewport_size(W, HYPOTHETICAL_H);
        assert_eq!(
            runs.get(),
            at_registration + 1,
            "★suppression ends with the paint; a real resize still reflows",
        );
    }

    #[test]
    fn a_question_before_the_first_paint_leaves_the_seam_unknown() {
        // Where the restore is the ONLY thing that puts the seam back.
        //
        // On a shell that has painted, the dispatch tail's R705 re-store is
        // itself a mirror at the live extent, so it re-publishes the live value
        // and masks a missing restore. A binding an agent drives BEFORE its
        // first paint has no stored frame for that loop to walk, so nothing
        // follows the producer — and the value it would be left holding is the
        // worst one: R1006 defines `(0, 0)` as "viewport unknown" and requires
        // consumers to skip on it, so a query that overwrote it with a
        // plausible size would make every reflow consumer act on a window that
        // does not exist yet.
        let _ = pinion_core::focus_request::drain();
        ARMED_REQUEST.with(|c| c.set(None));
        let mut sc = ShellCore::<HalfView>::new();
        let before = sc.root_owner().run(use_viewport_size);
        assert_eq!(
            before,
            (0, 0),
            "premise: no paint has happened, so the seam is honestly unknown",
        );
        let _ = layout_at(&mut sc, HYPOTHETICAL_H);
        assert_eq!(
            sc.root_owner().run(use_viewport_size),
            before,
            "★the question leaves the seam exactly as it found it",
        );
    }

    #[test]
    fn a_mirror_run_cannot_queue_a_focus_move() {
        let mut sc = booted();
        let focus_before = rpc(&mut sc, FOCUS_GET);
        ARMED_REQUEST.with(|c| c.set(Some(BY_ENGINE)));
        // A pure READ. It runs the view (through the producer for the
        // hypothetical, and again through the R705 re-store mirror), and
        // pre-R1468 the request that view wrote outlived the dispatch's own
        // drain point to be applied by whatever came next.
        let _ = layout_at(&mut sc, HYPOTHETICAL_H);
        assert_eq!(
            pinion_core::focus_request::drain(),
            None,
            "★a read leaves no request stranded in the mailbox",
        );
        assert_eq!(
            rpc(&mut sc, FOCUS_GET),
            focus_before,
            "★…and no later dispatch inherits a focus move it did not cause",
        );
    }

    #[test]
    fn a_mirror_at_the_live_extent_is_contained_too() {
        // The R705 re-store mirror runs at the LIVE extent, so it publishes
        // nothing and the geometry half of the scope is a no-op — but it is
        // still the run that measured the stranding, so the containment must
        // not be gated on the extent differing.
        let mut sc = booted();
        ARMED_REQUEST.with(|c| c.set(Some(BY_ENGINE)));
        let _ = sc.compute_paint_scene_pure(W, H);
        assert_eq!(
            pinion_core::focus_request::drain(),
            None,
            "★the side-effect-free mirror queues nothing even at the live size",
        );
    }

    #[test]
    fn a_live_paint_still_carries_a_genuine_request() {
        // The negative control the whole round hangs on: containment must not
        // cost the capability. The live paint is NOT a mirror, so a request
        // reactive code writes there is real, drains on that path, and moves
        // focus. Green before AND after R1468.
        let mut sc = booted();
        ARMED_REQUEST.with(|c| c.set(Some(BY_ENGINE)));
        let paint = sc.compute_paint_scene(W, H);
        sc.finalize_frame(paint);
        assert!(
            rpc(&mut sc, FOCUS_GET).contains(&format!(r#""focused":"{BY_ENGINE}""#)),
            "★a request from the live paint still lands",
        );
    }

    const FOCUS_GET: &str = r#"{"jsonrpc":"2.0","method":"focus/get","id":9}"#;
}

#[cfg(test)]
mod r1362_window_control_sink_seeding_tests {
    //! R1362 PR-65 §5.23 §6.3 — the SEEDING WINDOW, pinned end-to-end through
    //! the real constructor.
    //!
    //! `ShellCore::new_with_seed` promises the backend's boundary handles reach
    //! the root `Owner` BEFORE the binding factories resolve any `use_*` hook.
    //! That promise is load-bearing: a seed that lands one line too late used to
    //! be dropped without a panic or a log — `Owner::cache` is first-write-wins
    //! with a lazy Null default — leaving the binding a handle whose every call
    //! is a no-op, indistinguishable from a working app until the tray Quit does
    //! nothing. The R1366.x migrations closed that door: `REPAINT_SINK` (R1366.1),
    //! `QUIT_SINK` (R1366.2) and `WINDOW_CONTROL_SINK` (R1366.4) are all
    //! `ProviderSlot`s now, so a late seed PANICS instead of being dropped. What
    //! can still regress is the ORDER — a seed that runs after a read — which is
    //! exactly what this module pins, for all three sinks at once.
    //!
    //! The `window_control` unit tests cover the slot mechanics on a bare
    //! `Owner`; these cover the ORDER inside `ShellCore::new_with_seed`, the half
    //! that can actually regress — and which R999 left untested for the repaint
    //! sink.
    //!
    //! All THREE sinks `AppShell::new` seeds are resolved here (R1366.2 wired the
    //! quit sink into this fixture). `QuitSink` is the sink where the silent
    //! failure was worst: a dead repaint handle stutters, a dead quit handle
    //! means the app cannot be closed — the reason each drop became a panic.
    use super::ShellCore;
    use crate::test_fixtures::TestRenderer;
    use crate::window_control::WINDOW_CONTROL_SINK;
    use crate::{SizeStrategy, WidgetView, WindowControlSink};
    use pinion_a11y::WidgetA11y;
    use pinion_core::external::External;
    use pinion_core::scene::ContainerNode;
    use pinion_core::test_fixtures::ButtonFixture;
    use pinion_core::widget_core::ExtraExternal;
    use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
    use pinion_core::{Frame, QuitSink, RepaintSink, Scene, WidgetCore};
    use pinion_overlay::WindowControl;
    use std::cell::RefCell;
    use std::sync::{Arc, Mutex};

    const MAIN_TAG: &str = "main_btn";
    const EXTRA_TAG: &str = "extra";

    /// The boundary handles the fixture resolved at wiring time — one per sink
    /// `AppShell::new` seeds in its single closure.
    type ResolvedHandles = (
        Arc<dyn WindowControlSink>,
        Arc<dyn RepaintSink>,
        Arc<dyn QuitSink>,
    );

    // What the fixture's `create_extra_externals` resolved, handed back to the
    // test. A `thread_local` because the factory is an associated fn with no
    // argument to thread a channel through — the same constraint a real binding
    // works under (it stashes the handle in an `Owner::cache` slot).
    thread_local! {
        static RESOLVED: RefCell<Option<ResolvedHandles>> = const { RefCell::new(None) };
    }

    /// Recording sink standing in for `ProxyWindowControlSink`.
    #[derive(Debug, Default)]
    struct RecordingSink(Mutex<Vec<(String, WindowControl)>>);

    impl WindowControlSink for RecordingSink {
        fn request_window_control(&self, window_id: &str, control: WindowControl) {
            self.0
                .lock()
                .expect("poisoned")
                .push((window_id.to_owned(), control));
        }
    }

    /// Recording sink standing in for `ProxyRepaintSink`.
    #[derive(Debug, Default)]
    struct RecordingRepaint(Mutex<usize>);

    impl RepaintSink for RecordingRepaint {
        fn request_repaint(&self) {
            *self.0.lock().expect("poisoned") += 1;
        }
    }

    /// Recording sink standing in for `ProxyQuitSink`.
    #[derive(Debug, Default)]
    struct RecordingQuit(Mutex<usize>);

    impl QuitSink for RecordingQuit {
        fn request_quit(&self) {
            *self.0.lock().expect("poisoned") += 1;
        }
    }

    /// A binding that resolves EVERY boundary handle in `create_extra_externals`
    /// — the `hello-live-data` / `hello-tray` / sprag wiring shape (resolve at
    /// boot, hand to the producer thread).
    struct SinkCapturingFixture;

    impl WidgetCore for SinkCapturingFixture {
        type State = ButtonState;
        type Event = ButtonEvent;

        fn create_external() -> Box<dyn External> {
            Box::new(ButtonExternal::new())
        }
        fn tag() -> &'static str {
            MAIN_TAG
        }
        fn read_state(scene: &Scene) -> Self::State {
            <ButtonFixture as WidgetCore>::read_state(scene)
        }
        fn view(_state: Self::State, _frame: &Frame) -> Scene {
            Scene::Container(ContainerNode::new(Vec::new()).with_tag(MAIN_TAG))
        }
        fn event_name(event: Self::Event) -> &'static str {
            <ButtonFixture as WidgetCore>::event_name(event)
        }
        fn title() -> &'static str {
            "SinkCapturing"
        }

        fn create_extra_externals() -> Vec<ExtraExternal> {
            // Exactly what a binding does at wiring time.
            let wc = crate::use_window_control_sink();
            let rp = pinion_core::use_repaint_sink();
            let quit_sink = pinion_core::use_quit_sink();
            RESOLVED.with(|slot| *slot.borrow_mut() = Some((wc, rp, quit_sink)));
            vec![ExtraExternal {
                tag: EXTRA_TAG.into(),
                handle: Box::new(ButtonExternal::new()),
            }]
        }
    }

    impl WidgetA11y for SinkCapturingFixture {}

    impl WidgetView for SinkCapturingFixture {
        type Renderer = TestRenderer;

        fn initial_size_strategy() -> SizeStrategy {
            SizeStrategy::Fixed {
                width: 8,
                height: 8,
            }
        }
    }

    /// The production order: `AppShell::new`'s seed closure runs before the
    /// binding factories, so the handle the binding captures is the LIVE one.
    ///
    /// This is the test that would have caught a seed placed after
    /// `ShellCore::new` returned — the arrangement that compiles, runs, and
    /// silently no-ops.
    #[test]
    fn a_binding_factory_resolves_the_seeded_sinks_not_the_null_defaults() {
        RESOLVED.with(|slot| *slot.borrow_mut() = None);
        let sink = Arc::new(RecordingSink::default());
        let repaint = Arc::new(RecordingRepaint::default());
        let quit = Arc::new(RecordingQuit::default());
        let (seed_sink, seed_repaint, seed_quit) = (sink.clone(), repaint.clone(), quit.clone());
        let _sc = ShellCore::<SinkCapturingFixture>::new_with_seed(move |owner| {
            pinion_core::REPAINT_SINK.provide(owner, seed_repaint);
            WINDOW_CONTROL_SINK.provide(owner, seed_sink);
            pinion_core::QUIT_SINK.provide(owner, seed_quit);
        });

        let (wc, rp, quit_sink) = RESOLVED
            .with(|slot| slot.borrow_mut().take())
            .expect("create_extra_externals must have run during construction");

        // Drive every handle the binding captured. If the seed had lost the
        // race, these would be the Null defaults and the assertions below would
        // see nothing — the exact silent failure.
        wc.request_window_control("main", WindowControl::Close);
        rp.request_repaint();
        quit_sink.request_quit();

        assert_eq!(
            *sink.0.lock().expect("poisoned"),
            vec![("main".to_owned(), WindowControl::Close)],
            "the sink resolved inside `create_extra_externals` must be the seeded \
             one, not the Null default",
        );
        assert_eq!(
            *repaint.0.lock().expect("poisoned"),
            1,
            "R999's repaint sink rides the same seeding window",
        );
        assert_eq!(
            *quit.0.lock().expect("poisoned"),
            1,
            "R1363's quit sink rides it too — 0 here is a binding holding a \
             NullQuitSink, an app whose Quit cannot end it, which is the very \
             failure this module's prose reaches for and never drove until \
             R1366.2",
        );
    }

    /// R1448 §5.36 — the shell's published font report never says `NotProbed`,
    /// **including when the application declared no font of its own**.
    ///
    /// This is the regression net for the defect the R1448 demo's discriminator
    /// exposed and which, until this test, was pinned only by that demo. With an
    /// empty `app_fonts` nothing registers, so nothing shapes, so a report built
    /// from `system_font_status()` would read `NotProbed` — and keep reading it
    /// while the first frame went on to learn the truth. `probe_system_fonts`
    /// makes the shell look; this asserts that it does.
    ///
    /// Both halves matter. The empty case is the one that regressed; the
    /// declared case is the discriminator, since a shell that probed only when
    /// it had fonts to register would pass the second assertion alone.
    #[test]
    fn r1448_the_published_font_report_is_never_unprobed() {
        for (label, fonts) in [
            ("no application font declared", Vec::new()),
            (
                "a bogus declaration that registers nothing",
                vec![vec![0_u8; 64]],
            ),
        ] {
            RESOLVED.with(|slot| *slot.borrow_mut() = None);
            let sc = ShellCore::<SinkCapturingFixture>::new_with_seed_and_fonts(
                fonts,
                None,
                |_owner| {},
            );
            let report = sc.root_owner().run(pinion_core::reactive::font_sources);
            assert_ne!(
                report.system,
                pinion_core::reactive::SystemFontStatus::NotProbed,
                "the shell must have LOOKED before publishing ({label}): a \
                 report reading not-probed is a status line that stays wrong \
                 after the first frame has learned otherwise",
            );
            // Bytes that are not a font register nothing, and that is reported
            // rather than invented — the honest half of `register_font_data`.
            assert!(
                report.application_families.is_empty(),
                "nothing became selectable ({label}): {:?}",
                report.application_families,
            );
            assert_eq!(
                report.default_family, None,
                "a binding that declared no default keeps the platform stack \
                 ({label})",
            );
        }
    }

    /// R1472 §5.36 — a declared default reaches BOTH the published report and
    /// the cache the frame shapes with.
    ///
    /// The two halves are one claim: a report naming a family the render cache
    /// does not resolve unset text to would be a status line contradicting the
    /// pixels — the desync `new_with_seed_and_fonts` exists to make impossible,
    /// stated for the R1472 half the way R1448 stated it for registration.
    ///
    /// No face is declared alongside it on purpose. Whether the name resolves
    /// to glyphs is the shaper's business (and is measured on a host built for
    /// it, in `pinion-text`'s `font_less_host` fixture); what the shell owes is
    /// that the choice it was handed is the choice it installed.
    #[test]
    fn r1472_a_declared_default_family_reaches_the_report_and_the_render_cache() {
        let family = pinion_core::style::FontFamily::Named("Declared At Boot".into());
        let mut sc = ShellCore::<SinkCapturingFixture>::new_with_seed_and_fonts(
            Vec::new(),
            Some(family.clone()),
            |_owner| {},
        );

        let report = sc.root_owner().run(pinion_core::reactive::font_sources);
        assert_eq!(
            report.default_family,
            Some(family.clone()),
            "the binding reads back what it declared, so it can render its own \
             font state (§2 #7) instead of guessing",
        );

        let (cache, _engine) = sc.text_cache_and_engine();
        assert_eq!(
            cache.default_font_family(),
            Some(&family),
            "and the SAME cache the layout and paint passes borrow resolves \
             unset text to it — a report without this is a claim about a \
             cache nobody consulted",
        );
    }

    /// An unseeded `ShellCore::new` (headless / RPC-driven tests) still BOOTS,
    /// and the hooks a binding calls unconditionally in its factory resolve to
    /// something inert rather than panicking.
    ///
    /// Deliberately named for what it asserts, not for what it implies: it
    /// cannot prove the resolved handles ARE the Null objects (the sink traits
    /// carry no `Debug`/`Any` bound to identify them through, matching
    /// `RepaintSink`), so it is a smoke test for the `new()` →
    /// `new_with_seed(|_| {})` path, not a regression net for it. The identity
    /// half is covered on a bare `Owner` by `window_control::tests`.
    #[test]
    fn an_unseeded_shell_boots_and_its_factory_hooks_resolve_without_panicking() {
        RESOLVED.with(|slot| *slot.borrow_mut() = None);
        let _sc = ShellCore::<SinkCapturingFixture>::new();
        let (wc, rp, quit_sink) = RESOLVED
            .with(|slot| slot.borrow_mut().take())
            .expect("create_extra_externals must have run during construction");
        // Must not panic — the Null objects absorb all three.
        wc.request_window_control("main", WindowControl::Close);
        rp.request_repaint();
        quit_sink.request_quit();
    }
}

#[cfg(test)]
mod modal_tail_focus_tests {
    //! R1456 + R1462 §5.39 — what a dispatch tail does to focus when a
    //! modal is involved.
    //!
    //! Two consumer field reports, one seam, two independent defects; each
    //! survives the other's fix, so both sets of witnesses live here over
    //! one fixture.
    //!
    //! ## R1456 (PINION-PR77) — a modal **handoff** survives the drain
    //!
    //! A command palette whose destructive row hands off to a confirm
    //! dialog left a ghost scope on the modal stack, and from then on
    //! *every* `focus/set` was refused — the client's keyboard focus model
    //! was dead until restart. Root cause was a type mismatch, not a
    //! missing feature: the mailbox carried stack **edits** but inherited
    //! `focus_request`'s last-write-wins **level** policy, so `close` then
    //! `open` in one frame dropped the `close`.
    //!
    //! ## R1462 (PINION-PR78) — explicit intent outranks automatic policy
    //!
    //! From the same consumer: a palette row that closes the palette and
    //! then names where focus belongs always lost to `pop_modal_scope`'s
    //! automatic invoker-restore, because the tail drained the request
    //! mailbox *first*. The modal mailbox carries automatic policy and the
    //! focus mailbox carries explicit intent, so policy is applied first
    //! and intent last.
    //!
    //! ## Why here
    //!
    //! These drive the real [`ShellCore::drain_focus_mailboxes`] against a
    //! real [`FocusManager`], which is where both losses actually happened
    //! — the mailbox-level unit tests in `pinion_core::modal_scope_request`
    //! prove ordering, these prove the focus the shell builds from it. The
    //! fixture is shared with `pinion_tui::substrate::modal_tail_focus_tests`
    //! so the §2 #6 mirror is asserted against the same binding, not a
    //! look-alike.

    use super::ShellCore;
    use pinion_core::modal_scope_request;
    use pinion_core::test_fixtures::{
        MODAL_TAIL_CONFIRM_OK as CONFIRM_OK, MODAL_TAIL_MENU_ROW as MENU_ROW,
        MODAL_TAIL_OTHER_BG as OTHER_BG, MODAL_TAIL_TRIGGER as TRIGGER, ModalTailFixture,
        clear_focus_arc_log, focus_arc_log,
    };

    /// A booted shell with two focusable background controls and focus
    /// resting on the trigger — the state a user is in when they open a
    /// menu. Clears both mailboxes and the arc log so each test starts
    /// clean.
    fn boot() -> ShellCore<ModalTailFixture> {
        let _ = modal_scope_request::drain();
        let _ = pinion_core::focus_request::drain();
        let mut sc = ShellCore::<ModalTailFixture>::new();
        sc.refresh_window_focusables(
            pinion_runtime::DEFAULT_WINDOW,
            vec![TRIGGER.to_owned(), OTHER_BG.to_owned()],
        );
        assert!(sc.focus.focus_set(TRIGGER), "the invoker starts focused");
        clear_focus_arc_log();
        sc
    }

    /// One dispatch frame's worth of requests reaches the focus stack.
    fn open_menu(sc: &mut ShellCore<ModalTailFixture>) {
        modal_scope_request::open(vec![MENU_ROW.to_owned()]);
        sc.drain_focus_mailboxes();
    }

    /// PR-77 acceptance #1 — `close()` then `open(["b"])` in ONE dispatch
    /// frame lands at depth 1 over `["b"]`. Before R1456 the `close` was
    /// overwritten, so this was depth 2 with a ghost scope underneath.
    #[test]
    fn a_handoff_replaces_the_scope_instead_of_stacking_on_it() {
        let mut sc = boot();
        open_menu(&mut sc);
        assert_eq!(sc.focus().modal_depth(), 1, "the menu's trap is up");
        assert_eq!(sc.focus().focused(), Some(MENU_ROW));

        // The destructive row: ONE `External::invoke` body closes the menu
        // and opens the confirm dialog. Not two frames — one user action.
        modal_scope_request::close();
        modal_scope_request::open(vec![CONFIRM_OK.to_owned()]);
        sc.drain_focus_mailboxes();

        assert_eq!(
            sc.focus().modal_depth(),
            1,
            "the handoff REPLACES the menu's scope; a depth of 2 here is \
             the dismissed menu still trapping focus",
        );
        assert_eq!(
            sc.focus().active_tab_order(),
            [CONFIRM_OK.to_owned()],
            "the confirm dialog owns the active enumeration",
        );
        assert_eq!(sc.focus().focused(), Some(CONFIRM_OK));
    }

    /// PR-77 acceptance #2 — closing the handed-off modal returns to depth
    /// 0 and the BASE enumeration. This is the assertion that reproduces
    /// the reported symptom: with a ghost scope, the background stays
    /// unfocusable forever and `focus_set` refuses every tag.
    #[test]
    fn closing_the_handed_off_modal_leaves_no_ghost_scope() {
        let mut sc = boot();
        open_menu(&mut sc);
        modal_scope_request::close();
        modal_scope_request::open(vec![CONFIRM_OK.to_owned()]);
        sc.drain_focus_mailboxes();

        // R1477 — the premise this test's NAME presupposes: the handoff
        // actually happened, so what the close below closes is the confirm
        // dialog. Without it the end state is reachable two ways — the whole
        // batch applied, or only the `close` — and the assertions after the
        // cycle hold either way. Measured: under a drain that applies only the
        // first queued request, this test passed on both backends while
        // claiming to cover exactly that batch.
        assert_eq!(
            sc.focus().modal_depth(),
            1,
            "the confirm dialog is up; a depth of 0 means the `open` in the \
             same batch was dropped and the cycle below proves nothing",
        );
        assert_eq!(sc.focus().focused(), Some(CONFIRM_OK));

        // The user answers the confirm dialog.
        modal_scope_request::close();
        sc.drain_focus_mailboxes();

        assert_eq!(sc.focus().modal_depth(), 0, "no trap survives the cycle");
        assert_eq!(
            sc.focus().active_tab_order(),
            [TRIGGER.to_owned(), OTHER_BG.to_owned()],
            "the base enumeration is active again, whole",
        );
        assert_eq!(
            sc.focus().focused(),
            Some(TRIGGER),
            "focus lands back on the invoker that opened the menu",
        );
        assert!(
            sc.focus.focus_set(OTHER_BG),
            "a background control is focusable again — the live PR-77 \
             symptom was this returning false for every tag, forever",
        );
    }

    /// PR-77 acceptance #3 — nesting is untouched. Two frames, one `open`
    /// each, still stack to depth 2; the batch drain changes how many
    /// requests one frame may carry, not what a request means.
    #[test]
    fn nesting_across_separate_frames_still_stacks() {
        let mut sc = boot();
        open_menu(&mut sc);

        modal_scope_request::open(vec![CONFIRM_OK.to_owned()]);
        sc.drain_focus_mailboxes();

        assert_eq!(sc.focus().modal_depth(), 2, "B stacks ON TOP of A");
        assert_eq!(sc.focus().active_tab_order(), [CONFIRM_OK.to_owned()]);

        modal_scope_request::close();
        sc.drain_focus_mailboxes();
        assert_eq!(sc.focus().modal_depth(), 1, "popping B re-exposes A");
        assert_eq!(
            sc.focus().active_tab_order(),
            [MENU_ROW.to_owned()],
            "A is a LIVE scope here, unlike the handoff's ghost",
        );
    }

    /// The batch is one observable focus transition, not two. A handoff's
    /// pop transiently restores the invoker before the push moves focus
    /// into the dialog; that intermediate owner is never painted, so
    /// announcing it would tell a background widget it gained focus — a
    /// `TextField` would commit an IME composition, a caret would start
    /// blinking off-screen. Snapshotting the arc once around the whole
    /// batch is what keeps `TRIGGER` out of this log.
    #[test]
    fn a_handoff_reports_one_focus_arc_not_the_transient_restore() {
        let mut sc = boot();
        open_menu(&mut sc);
        clear_focus_arc_log();

        modal_scope_request::close();
        modal_scope_request::open(vec![CONFIRM_OK.to_owned()]);
        sc.drain_focus_mailboxes();

        assert_eq!(
            focus_arc_log(),
            vec![(MENU_ROW, false), (CONFIRM_OK, true)],
            "blur the outgoing member, focus the incoming one — the \
             invoker the pop transiently restored is never announced",
        );
    }

    /// A frame carrying a single request behaves exactly as before the
    /// batch drain: one arc, one scope. The overwhelming majority of
    /// frames are this shape, so it is the regression that matters most.
    #[test]
    fn a_single_request_frame_is_unchanged() {
        let mut sc = boot();
        open_menu(&mut sc);

        assert_eq!(sc.focus().modal_depth(), 1);
        assert_eq!(focus_arc_log(), vec![(TRIGGER, false), (MENU_ROW, true)]);
    }

    // ---- R1462 (PINION-PR78): explicit intent outranks the modal's
    // automatic restore, and the two mailboxes report ONE arc ------------

    /// PR-78 acceptance #1 — the reported defect. One dispatch frame closes
    /// the modal AND names where focus belongs; the name wins.
    ///
    /// Pre-R1462 the tail drained the request first, so
    /// `pop_modal_scope`'s unconditional restore landed on top of it and
    /// focus went back to the invoker — the command-palette pattern (close
    /// the palette, run the row, focus what the row produced) could not be
    /// expressed at all.
    #[test]
    fn an_explicit_request_outranks_the_modal_restore() {
        let mut sc = boot();
        open_menu(&mut sc);

        // The row runs: dismiss the palette, then put focus where the
        // command's result lives.
        modal_scope_request::close();
        pinion_core::focus_request::request(OTHER_BG);
        sc.drain_focus_mailboxes();

        assert_eq!(sc.focus().modal_depth(), 0, "the palette is dismissed");
        assert_eq!(
            sc.focus().focused(),
            Some(OTHER_BG),
            "the binding named the target; the automatic restore to the \
             invoker is the DEFAULT it overrides, not a veto",
        );
    }

    /// The automatic restore is still the default — it is only
    /// *overridable*, not demoted. The control for the test above:
    /// the identical frame minus the request lands on the invoker,
    /// WAI-ARIA's required behaviour.
    #[test]
    fn the_restore_still_wins_when_nothing_competes() {
        let mut sc = boot();
        open_menu(&mut sc);

        modal_scope_request::close();
        sc.drain_focus_mailboxes();

        assert_eq!(sc.focus().focused(), Some(TRIGGER));
    }

    /// PR-78 acceptance #2 — the branch the field report calls the worse
    /// one. The command that ran removed the invoker from the scene, so the
    /// restore has nothing to land on and `pop_modal_scope` commits `None`
    /// (a commit, not a skip). The explicit request is precisely the
    /// information needed to avoid a keyboard-focus-less window, and
    /// pre-R1462 it was consumed before that `None` overwrote it.
    #[test]
    fn a_request_lands_even_when_the_restore_target_is_gone() {
        let mut sc = boot();
        open_menu(&mut sc);
        // The command's side effect: the invoker leaves the scene. The
        // modal guard in `update_focusable_tags` keeps the trap's focus
        // alive across this, exactly as a real repaint would.
        sc.refresh_window_focusables(pinion_runtime::DEFAULT_WINDOW, vec![OTHER_BG.to_owned()]);

        modal_scope_request::close();
        pinion_core::focus_request::request(OTHER_BG);
        sc.drain_focus_mailboxes();

        assert_eq!(
            sc.focus().focused(),
            Some(OTHER_BG),
            "focus must not be nowhere: the frame that destroyed the \
             restore target also said where to go instead",
        );
    }

    /// The close-and-request frame is ONE observable transition. The
    /// invoker the pop transiently restores is never painted, so announcing
    /// it would tell a background widget it gained focus — an IME
    /// composition commits, a caret starts blinking off-screen. R1456
    /// established this across a modal batch; R1462 extends the same
    /// snapshot across BOTH mailboxes, because applying policy before
    /// intent is what creates the transient here.
    #[test]
    fn a_close_and_request_report_one_focus_arc() {
        let mut sc = boot();
        open_menu(&mut sc);
        clear_focus_arc_log();

        modal_scope_request::close();
        pinion_core::focus_request::request(OTHER_BG);
        sc.drain_focus_mailboxes();

        assert_eq!(
            focus_arc_log(),
            vec![(MENU_ROW, false), (OTHER_BG, true)],
            "blur the outgoing member, focus the named target — TRIGGER, \
             restored between the two, is never announced",
        );
    }

    /// PR-78's new capability: open a dialog focused on a **chosen** member
    /// (a destructive prompt whose default is deliberately not Cancel).
    /// The push installs the trap before the request is applied, so the
    /// member is enumerated by the time `focus_set` looks.
    ///
    /// The second half is the part that was silently corrupt rather than
    /// merely impossible: with the request applied FIRST, the R1020
    /// re-derive could let it succeed while the trap was still down, and
    /// `push_modal_scope` then snapshotted the dialog's own control as the
    /// invoker — so closing the dialog restored focus to a node the dialog
    /// took with it. Asserting that the pop still finds TRIGGER is what
    /// pins the snapshot to the real invoker.
    #[test]
    fn an_open_can_name_the_member_it_focuses_without_losing_the_invoker() {
        let mut sc = boot();

        modal_scope_request::open(vec![MENU_ROW.to_owned(), CONFIRM_OK.to_owned()]);
        pinion_core::focus_request::request(CONFIRM_OK);
        sc.drain_focus_mailboxes();

        assert_eq!(sc.focus().modal_depth(), 1);
        assert_eq!(
            sc.focus().focused(),
            Some(CONFIRM_OK),
            "the named member takes focus instead of the first one",
        );

        modal_scope_request::close();
        sc.drain_focus_mailboxes();
        assert_eq!(
            sc.focus().focused(),
            Some(TRIGGER),
            "the invoker snapshot survived: the request moved focus INSIDE \
             the trap, it did not become the thing to restore",
        );
    }

    /// Trap confinement outranks the request in turn. A request for a tag
    /// outside the modal's members is refused, so a binding cannot walk
    /// focus out of an open dialog by accident. (Pre-R1462 the request was
    /// applied while the trap was still down and the push then yanked focus
    /// back — same end state, reached by briefly leaving the trap.)
    #[test]
    fn a_request_cannot_escape_a_trap_opened_in_the_same_frame() {
        let mut sc = boot();

        modal_scope_request::open(vec![MENU_ROW.to_owned()]);
        pinion_core::focus_request::request(OTHER_BG);
        sc.drain_focus_mailboxes();

        assert_eq!(
            sc.focus().focused(),
            Some(MENU_ROW),
            "the trap confines the request; focus never leaves the dialog",
        );
    }

    /// A request with no modal in the frame is untouched by the reorder —
    /// the shape of nearly every focus request in the tree.
    #[test]
    fn a_request_without_a_modal_is_unchanged() {
        let mut sc = boot();

        pinion_core::focus_request::request(OTHER_BG);
        sc.drain_focus_mailboxes();

        assert_eq!(sc.focus().focused(), Some(OTHER_BG));
        assert_eq!(sc.focus().modal_depth(), 0);
    }
}

/// R1658 §5.13 §5.39 — the injected keystroke says when it arrived.
#[cfg(test)]
mod injected_arrival_tests {
    use super::ShellCore;
    use pinion_a11y::WidgetA11y;
    use pinion_core::{KeyPress, Scene, WidgetCore};

    use crate::test_fixtures::TestRenderer;
    use crate::{SizeStrategy, WidgetView};

    thread_local! {
        static INJECTED_ARRIVALS: std::cell::RefCell<Vec<pinion_core::KeyArrival>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    struct InjectedArrivalFixture;

    impl WidgetCore for InjectedArrivalFixture {
        type State = pinion_core::widgets::button::ButtonState;
        type Event = pinion_core::widgets::button::ButtonEvent;

        fn create_external() -> Box<dyn pinion_core::external::External> {
            Box::new(pinion_core::widgets::button::ButtonExternal::new())
        }

        fn tag() -> &'static str {
            "injected_arrival"
        }

        fn read_state(_scene: &Scene) -> Self::State {
            pinion_core::widgets::button::ButtonState::Idle
        }

        fn view(_state: Self::State, _frame: &pinion_core::Frame) -> Scene {
            Scene::Container(
                pinion_core::scene::ContainerNode::new(vec![]).with_tag("injected_arrival"),
            )
        }

        fn event_name(_event: Self::Event) -> &'static str {
            "__internal__"
        }

        fn title() -> &'static str {
            "InjectedArrival"
        }

        fn apply_key_press(
            _scene: &mut Scene,
            _focused: Option<&str>,
            press: &KeyPress<'_>,
        ) -> bool {
            INJECTED_ARRIVALS.with(|log| log.borrow_mut().push(press.arrival));
            true
        }
    }

    fn drain_one_key(sc: &mut ShellCore<InjectedArrivalFixture>, key: &str) {
        use pinion_rpc::{DeferredInput, KeyWireState};
        use pinion_runtime::DEFAULT_WINDOW;
        sc.drain_deferred_inputs_for_window(
            Some(DEFAULT_WINDOW),
            &[DeferredInput::Key {
                x: 5.0,
                y: 5.0,
                key: key.to_owned(),
                state: KeyWireState::Press,
            }],
        );
    }

    #[test]
    fn r1658_an_injected_key_carries_an_arrival_and_each_request_is_its_own() {
        // §2 #2 — every input a human makes has an RPC peer, and this round's
        // capability is no exception: a key injected over the wire carries an
        // arrival too, stamped as the drain opens rather than when the binding
        // gets round to it.
        //
        // ONE REQUEST IS ONE DELIVERY, and that is the honest answer for this
        // path rather than a limitation: the agent sent them as two calls, so
        // they did not arrive together. A test asserting the opposite would be
        // asserting a lie about how the wire works.
        INJECTED_ARRIVALS.with(|log| log.borrow_mut().clear());
        let mut sc = ShellCore::<InjectedArrivalFixture>::new();
        let boot = sc.compute_paint_scene(100, 100);
        sc.finalize_frame(boot);

        drain_one_key(&mut sc, "a");
        drain_one_key(&mut sc, "b");

        let log = INJECTED_ARRIVALS.with(|log| log.borrow().clone());
        assert_eq!(log.len(), 2, "both injected keys reached the binding");
        assert!(
            !log[0].arrived_with(log[1]),
            "two `scene/key` requests are two deliveries"
        );
    }

    #[test]
    fn r1658_keys_of_one_drain_arrived_together() {
        // The other half of the same rule: keys the agent handed over in ONE
        // drain did arrive together. This is what makes the batch axis
        // drivable over the wire at all — without it the injected path could
        // never express a gesture, only a sequence of unrelated keystrokes.
        use pinion_rpc::{DeferredInput, KeyWireState};
        use pinion_runtime::DEFAULT_WINDOW;

        INJECTED_ARRIVALS.with(|log| log.borrow_mut().clear());
        let mut sc = ShellCore::<InjectedArrivalFixture>::new();
        let boot = sc.compute_paint_scene(100, 100);
        sc.finalize_frame(boot);

        let key = |name: &str| DeferredInput::Key {
            x: 5.0,
            y: 5.0,
            key: name.to_owned(),
            state: KeyWireState::Press,
        };
        sc.drain_deferred_inputs_for_window(Some(DEFAULT_WINDOW), &[key("a"), key("b"), key("c")]);

        let log = INJECTED_ARRIVALS.with(|log| log.borrow().clone());
        assert_eq!(log.len(), 3);
        assert!(
            log[0].arrived_with(log[1]) && log[1].arrived_with(log[2]),
            "one drain is one delivery"
        );
        assert_eq!(log[0].at(), log[2].at(), "and one instant");
    }

    #[test]
    fn r1658_the_platform_key_path_takes_the_open_delivery() {
        // THE GUI PATH ITSELF, which is the one the defect was reported on and
        // the one a counterfactual found untested: every other test here drives
        // `CoreShell` or the RPC drain, and both would stay green with
        // `apply_key_inner` stamping a fresh arrival at dispatch — the exact
        // shape this round exists to remove.
        //
        // `AppShell::new_events` opens the delivery once per winit event-loop
        // iteration; this stands in for that, then sends two keys the way two
        // `WindowEvent::KeyboardInput` arms of one iteration would.
        INJECTED_ARRIVALS.with(|log| log.borrow_mut().clear());
        let mut sc = ShellCore::<InjectedArrivalFixture>::new();
        let boot = sc.compute_paint_scene(100, 100);
        sc.finalize_frame(boot);

        let opened = sc.open_key_delivery();
        sc.apply_key("a");
        sc.apply_key("b");

        let log = INJECTED_ARRIVALS.with(|log| log.borrow().clone());
        assert_eq!(log.len(), 2, "both keys reached the binding");
        assert!(
            log[0].arrived_with(opened) && log[1].arrived_with(opened),
            "both carry the delivery the backend opened, not one taken at dispatch"
        );
        assert_eq!(
            log[0].at(),
            log[1].at(),
            "and so the second key is not dated later than the first"
        );
    }

    #[test]
    fn r1658_the_offer_first_key_path_takes_the_open_delivery_too() {
        // `try_apply_key_inner` is the SECOND winit dispatch arm — the
        // offer-first path Escape and Tab take before the shell's own
        // fallbacks. It is a separate call site with its own copy of the rule,
        // so it gets its own assertion rather than being assumed to match.
        INJECTED_ARRIVALS.with(|log| log.borrow_mut().clear());
        let mut sc = ShellCore::<InjectedArrivalFixture>::new();
        let boot = sc.compute_paint_scene(100, 100);
        sc.finalize_frame(boot);

        let opened = sc.open_key_delivery();
        assert!(sc.try_apply_key("Escape"), "the fixture claims every key");
        assert!(sc.try_apply_key("Tab"));

        let log = INJECTED_ARRIVALS.with(|log| log.borrow().clone());
        assert_eq!(log.len(), 2);
        assert!(
            log[0].arrived_with(opened) && log[1].arrived_with(opened),
            "the offer-first arm reads the open delivery as well"
        );
    }

    impl WidgetA11y for InjectedArrivalFixture {}
    impl WidgetView for InjectedArrivalFixture {
        type Renderer = TestRenderer;
        fn initial_size_strategy() -> SizeStrategy {
            SizeStrategy::Fixed {
                width: 100,
                height: 100,
            }
        }
    }
}
