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

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use accesskit::NodeId;
use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, PinionAccessAction, ROOT_NODE_ID, tag_to_node_id,
    translate_action,
};
use pinion_core::event::WheelDelta;
use pinion_core::{Frame, Intent, Scene, SceneRevision};
use pinion_rpc::{
    DeferredInput, DispatchContext, DragButton, KeyWireState, LayoutNode, PreviewLedger, Request,
    dispatch_parsed, parse_request,
};
use pinion_runtime::text_engine::SelfHostedTextEngine;
use pinion_runtime::{
    CommandExecutor, CoreShell, DispatchTail, FocusManager, FrameTiming, FrameTimingStats,
    FrameTimingsSnapshot, IntentQueue, Modifiers, PanRelease, PointerId, TextMeasure, Touch,
    TouchPhase, clamp_frame_dt, compute_layout_with_text_measure, rect_for_tag,
    walk_scene_and_drain_immediate,
};
use pinion_text::LayoutCache;

use super::WidgetView;

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
    /// R889 — `Some(id)` rejects the whole request with `-32602
    /// unknown_window` before method routing.
    unknown_window: Option<String>,
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
    revision: SceneRevision,
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
    /// R1072 §5.37 — opt-in self-hosted text engine, the shipping consumer of
    /// the §5.37 paint + measure arms. `Some` when the `PINION_TEXT_ENGINE`
    /// environment variable selected it at construction AND a usable system font
    /// was found; `None` (the default) keeps every text leaf on parley, so the
    /// shell paints and measures byte-identically to the pre-R1072 path.
    ///
    /// Threaded through BOTH arms together (the R1070.1 wire-both contract): the
    /// measure pass ([`compute_layout_with_text_measure`]) via
    /// [`Self::text_measure_override`] and the production paint
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

    /// R680 atomic 2 §5.16 §5.41 — per-window redraw flag map.
    ///
    /// Keyed by canonical `WindowSpec::id`; an entry of `true` means
    /// "paint the next event-loop iteration for THIS window only".
    /// Set via [`Self::request_redraw_for_window`]; drained via
    /// [`Self::take_redraw_request_for_window`]. [`crate::AppShell`]
    /// drains EACH window's flag during
    /// [`crate::AppShell::drain_redraw_to_winit`] and calls
    /// `Window::request_redraw` only on the slots whose flag was
    /// set.
    ///
    /// ## Coexistence with `redraw_requested`
    ///
    /// The pre-R680 binding-wide [`Self::redraw_requested`] flag
    /// keeps the canonical "fan out to every window" semantic
    /// because most current state mutations (`Signal::set` in
    /// `V::update`, the `any_animation_active` check after a
    /// per-window tick) cannot reliably attribute themselves to a
    /// single window: the `Signal` might be read by view fns from
    /// multiple windows; an animation registered on `root_owner`
    /// could appear in any window's paint. The fan-out is the
    /// safe default.
    ///
    /// The per-window flag is the OPT-IN surface for callers that
    /// know exactly which window a wake-up should target —
    /// future R680 atomic 3 RPC dispatch context (per-window
    /// scoped `scene/invoke` follow-up redraws), R681 immediate-
    /// mode game-loop nodes (only the window holding the immediate
    /// subtree should poll at frame rate), R683 dock-panel
    /// resize / tear-off (only the active dock panel's window
    /// reacts to layout change). Pre-R680 callers continue using
    /// `request_redraw()` + bit-identical fan-out behaviour.
    ///
    /// ## Allocation profile
    ///
    /// Lazy-creates entries on first `request_redraw_for_window`
    /// call per `window_id` (one String allocation per first
    /// touch). Hot-path drains read the existing entry without
    /// reallocation. Mirrors the
    /// [`Self::last_paint_instants`] field shape so the two
    /// per-window state stores share the same `&str → owned key`
    /// convention.
    redraw_requested_per_window: HashMap<String, bool>,

    /// R51.143 §5.28 §5.16 §5.41 — per-window paint-clock store.
    ///
    /// Keyed by canonical `WindowSpec::id` (`"main"` for the primary
    /// window; secondary windows pick their own non-conflicting
    /// names). Each entry records the wall-clock timestamp of the
    /// previous [`Self::compute_paint_scene_internal`] call for THAT
    /// window. Missing entry (never painted) → next paint feeds
    /// `dt = 0.0`, which leaves at-rest animations untouched and
    /// starts each spring solver from its construction baseline —
    /// same shape any synthetic flush hits, so no special-case
    /// branching is needed elsewhere.
    ///
    /// ## Why `HashMap` (R680 atomic 1)
    ///
    /// Pre-R680 this was a single `Option<Instant>` field. Multi-
    /// window paint cycles (R670.B `hello-multi-window` + R672
    /// per-window `InputRouter` foundation + R675-R679 `DevTools`
    /// cascade) all wrote into one slot — whichever window painted
    /// most recently set the next paint's "prev" timestamp, so
    /// window A's dt was measured against window B's last paint
    /// when the two windows alternated. The compounding made spring
    /// solvers tick by ~2× their per-window paint rate (the R670.B
    /// honest 9-round carry).
    ///
    /// R680 atomic 1 lifts the field per-window: each entry tracks
    /// only ITS window's paint cadence. Two windows painting in the
    /// same event-loop turn each measure `dt` against their own
    /// previous paint, never each other's, so the spring solver
    /// receives exactly one tick per per-window paint cycle.
    /// [`CoreShell::tick_animations_for_window`](pinion_runtime::CoreShell::tick_animations_for_window)
    /// then walks ONLY that window's owner scope (R680 atomic 0
    /// `window_owners` substrate) so the per-window owner's
    /// `owned_animations` list advances exactly once.
    ///
    /// Per §5.28 R33 the spring solver is deterministic given
    /// `(current, velocity, target, dt, config)` — driving it from a
    /// real measured delta is what turns the synthetic substrate
    /// (which always passed `dt=0`) into a real per-frame animation
    /// pump. Per-window storage is what makes that real delta
    /// per-window instead of binding-wide.
    last_paint_instants: HashMap<String, Instant>,
    /// R681 §2 #4 atomic 3 §5.16 §5.28 — per-window target fps
    /// override map. `&'static str` window id → desired `fps`.
    /// Bindings populate via
    /// [`Self::set_target_fps_for_window`]; the surface's
    /// `about_to_wait` consults
    /// [`pinion_runtime::frame_pacing::frame_budget_for_window`]
    /// with this lookup result so per-window pacing overrides the
    /// 60fps immediate-mode default.
    ///
    /// Absent entry → use
    /// [`pinion_runtime::frame_pacing::default_window_frame_policy`]
    /// derived from [`Self::immediate_subtree_for_window`].
    target_fps_per_window: HashMap<String, u32>,
    /// R681 §2 #4 §5.16 — per-window "did the last painted scene carry a
    /// [`pinion_core::scene::Scene::ImmediateModeNode`]?" flag, the
    /// single home for the pacing input both the render loop's
    /// `about_to_wait` and the §5.16 jank profiler
    /// ([`Self::frame_timings_for_window`]) read. `AppShell::render_window`
    /// publishes it every paint via [`Self::set_immediate_subtree_for_window`]
    /// — the peer of [`Self::target_fps_per_window`] (a pacing input that
    /// also lives here, not on the slot), so the two pacing inputs share
    /// one lookup and pacing vs observability cannot derive different
    /// budgets. Absent entry → `false` (retained-tree default).
    immediate_subtree_per_window: HashMap<String, bool>,
    /// R829 §2 #4 §5.28 — per-window pending injected immediate-mode `dt`
    /// (seconds). A `scene/tick` ([`pinion_rpc::DeferredInput::Tick`])
    /// accumulates its delta here; the next per-window paint
    /// ([`Self::compute_paint_scene_internal`]) advances that window's
    /// [`pinion_core::scene::ImmediateMode`] drivers by exactly this
    /// amount (substepped) INSTEAD of the wall-clock delta, then clears
    /// the entry. Lets an AI client frame-step the §2 #4 game loop
    /// deterministically — pair with `scene/set_fps 0` to pause the
    /// continuous loop so wall-clock paints do not also advance it.
    /// Absent entry → live wall-clock advance (the R827 default).
    pending_immediate_dt_per_window: HashMap<String, f32>,
    /// R831 §2 #4 §5.28 — per-window fixed-timestep accumulator for the
    /// immediate-mode game loop. Every per-window paint feeds this
    /// window's elapsed simulation time (the clamped wall-clock delta, OR
    /// the injected `scene/tick` delta from
    /// [`Self::pending_immediate_dt_per_window`]) into one
    /// [`pinion_runtime::FixedTimestep`], which advances the
    /// [`pinion_core::scene::ImmediateMode`] drivers in EXACTLY
    /// fixed-timestep increments and carries the sub-step remainder
    /// across frames (Glenn Fiedler, "Fix Your Timestep!"). Routing live
    /// AND injected time through the SAME accumulator is what makes the
    /// loop frame-rate-independent and makes `scene/tick` reproduce live
    /// behaviour deterministically (the pre-R831 split: live ran one
    /// variable wall-clock step, `scene/tick` ran the
    /// [`pinion_runtime::substep`] splitter — two time bases that could
    /// not reproduce each other). Reset to a zero phase on pause
    /// (`scene/set_fps 0`, [`Self::set_target_fps_for_window`]) so an AI
    /// client frame-steps from a known fixed-step boundary. Absent entry
    /// → a fresh zero-phase accumulator (lazy-inserted on first paint).
    sim_accumulator_per_window: HashMap<String, pinion_runtime::FixedTimestep>,
    /// R682 §5.16 atomic 3 — per-window fragment cache observability
    /// snapshot. The surface-side `AppShell::render_window` calls
    /// [`Self::publish_fragment_cache_stats`] after each paint cycle
    /// to publish hits / misses / damage-region read off the
    /// per-`WindowSlot` `paint_adapter::FragmentCache`. AI-first RPC
    /// consumers, R682 demo assertions, and the upcoming `pinion-tui`
    /// test harness read via
    /// [`Self::fragment_cache_stats_for_window`] without crossing the
    /// substrate ↔ vello boundary (the stats struct is GUI-agnostic).
    fragment_cache_stats_per_window: HashMap<String, FragmentCacheStats>,
    /// R907 §5.16 §5.7 — per-window frame-timing profiler accumulator.
    /// The surface-side `AppShell::render_window` brackets each paint's
    /// build / encode / render phases with [`std::time::Instant`] spans
    /// and calls [`Self::record_frame_timing`] to feed the rolling
    /// window. Unlike the `Copy` [`FragmentCacheStats`] (published
    /// every frame), the non-`Copy`
    /// [`pinion_runtime::FrameTimingStats`] ring is the SSOT here and
    /// the `Copy` [`FrameTimingsSnapshot`] is projected at the AI-paced
    /// `scene/frame_timings` read ([`Self::frame_timings_for_window`]),
    /// so the O(window) fold never touches the 60–144fps paint path
    /// (the R890 "store the source, project on read" rule).
    frame_timings_per_window: HashMap<String, FrameTimingStats>,

    /// R1036 §5.16 §5.7 §2 #7 — per-window render-fidelity record (PR-17).
    /// `AppShell::render_window` writes the encoded frame's fingerprint here
    /// after `renderer.render`, on the winit paint path ONLY — never by an RPC
    /// recompute, so it stays the uncontaminated "what is actually displayed"
    /// SSOT that `scene/render_fidelity`
    /// ([`Self::render_fidelity_for_window`]) projects and compares against a
    /// fresh recompute of current state (the `last_paint_scene` it does NOT
    /// share is overwritten by the R684 post-dispatch finalize, hiding exactly
    /// the divergence this record exposes).
    render_fidelity_per_window: HashMap<String, pinion_runtime::RenderFidelity>,

    /// R25.1 §5.39 §5.16 — per-window keyboard-focus enumeration. Each painted
    /// window stores the focusable tags IT draws (its
    /// [`Scene::collect_focusable_tags`] walk); the binding-wide
    /// [`FocusManager`] `tab_order` is the UNION of these
    /// ([`Self::union_focusable_tags`]), refreshed by every window's paint
    /// ([`Self::refresh_window_focusables`]) — no longer
    /// [`pinion_runtime::DEFAULT_WINDOW`]-gated.
    ///
    /// Pre-R25.1 the enumeration was primary-only, so a focusable widget drawn
    /// solely in a secondary window (a torn-off / undock pane in its own
    /// `pane-{i}` window) was absent from the focus set: clicking it resolved to
    /// no focusable tag, so keyboard input never routed to its `External`
    /// (sprag undock, "the detached terminal eats no input"). Keying per window
    /// — the R1021 [`pinion_runtime::CoreShell::publish_pane_viewports`]
    /// precedent — lets a secondary window's pane JOIN the Tab order / click-
    /// focus set without REPLACING the primary's enumeration (a naive whole-
    /// `tab_order` replace from each window would clobber the primary's docked
    /// panes, the exact hazard the pre-R25.1 gate guarded against). A single-
    /// window binding holds one entry, so the union equals that window's tags
    /// verbatim — byte-identical to the pre-R25.1 primary-only enumeration. The
    /// matching removal edge is [`Self::remove_window`], which drops the entry
    /// and re-folds so a closed window's tags leave the union.
    focusable_tags_per_window: HashMap<String, Vec<String>>,

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
/// inside a single `compute_access_emit` call (silent surprise —
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

    /// R999 §5.23 — [`Self::new`] with the shell's
    /// [`RepaintSink`](pinion_core::RepaintSink) seeded into the binding's root
    /// [`Owner`](pinion_core::Owner) before its factories run, so a binding can
    /// capture the live sink via
    /// [`use_repaint_sink`](pinion_core::use_repaint_sink) for an off-thread
    /// producer. Delegates the seed to
    /// [`CoreShell::new_with_repaint_sink`](pinion_runtime::CoreShell::new_with_repaint_sink).
    #[must_use]
    pub fn new_with_repaint_sink(
        repaint_sink: std::sync::Arc<dyn pinion_core::RepaintSink>,
    ) -> Self {
        Self::with_core(CoreShell::<V>::new_with_repaint_sink(repaint_sink))
    }

    /// Shared constructor body — focus seeding + Vello-side substrate fields —
    /// over an already-built [`CoreShell`](pinion_runtime::CoreShell), so
    /// [`Self::new`] (Null sink) and [`Self::new_with_repaint_sink`] (live
    /// sink) differ only in how `core` was constructed.
    fn with_core(core: CoreShell<V>) -> Self {
        // Log the initial state read through the §5.15 introspect
        // channel — same trace line shape AppShell relied on
        // pre-R51.123 so the dogfood eprintln + RPC-side observer
        // both see the boot-time state.
        eprintln!(
            "shell: initial state = {}",
            V::fmt_state_log(core.cached_state()),
        );
        let mut shell = Self {
            core,
            previews: PreviewLedger::default(),
            revision: SceneRevision::default(),
            focus: FocusManager::new(),
            modifiers: Modifiers::empty(),
            text_cache: LayoutCache::new(),
            text_engine: build_text_engine_from_env(),
            last_access_tag_map: HashMap::new(),
            last_access_nodes: HashMap::new(),
            access_emit_initial: true,
            last_access_focus: None,
            redraw_requested: false,
            redraw_requested_per_window: HashMap::new(),
            last_paint_instants: HashMap::new(),
            target_fps_per_window: HashMap::new(),
            immediate_subtree_per_window: HashMap::new(),
            pending_immediate_dt_per_window: HashMap::new(),
            sim_accumulator_per_window: HashMap::new(),
            fragment_cache_stats_per_window: HashMap::new(),
            frame_timings_per_window: HashMap::new(),
            render_fidelity_per_window: HashMap::new(),
            focusable_tags_per_window: HashMap::new(),
            text_select_drag: None,
            os_focused_window: None,
        };
        // (R1020 §5.39) Seed the focus enumeration from the binding's first
        // view — the scene-derived source the per-frame paint refresh uses —
        // so an RPC `focus/set` or the first Tab resolves a current
        // enumeration before the first live paint. (Replaces the pre-R1020
        // `V::focusable_tags()` boot seed; the method is retired.)
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

    /// R1025 §5.35 — single-window [`Self::hover_target_for_window`]
    /// (the [`pinion_runtime::DEFAULT_WINDOW`] router), symmetric with the
    /// single-window [`Self::cursor_moved`] / [`Self::mouse_pressed`] drivers.
    #[must_use]
    pub fn hover_target(&self, pid: PointerId) -> Option<&str> {
        self.core.hover_target(pid)
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
fn build_text_engine_from_env() -> Option<SelfHostedTextEngine> {
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
        Ok(engine) => Some(engine),
        Err(e) => {
            eprintln!("shell: PINION_TEXT_ENGINE set but no usable system font: {e}");
            None
        }
    }
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
/// path ([`ShellCore::apply_focus_ring`]) and the RPC
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
/// home shared by the winit paint path ([`ShellCore::apply_focus_ring`]) and the
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
    /// [`CoreShell`](pinion_runtime::CoreShell) into the
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

    /// R680 atomic 2 §5.16 §5.41 — per-window redraw wake-up.
    ///
    /// Sets the per-window flag in
    /// [`Self::redraw_requested_per_window`] for the named
    /// `window_id`. [`crate::AppShell::drain_redraw_to_winit`]
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
        self.redraw_requested_per_window
            .insert(window_id.to_owned(), true);
    }

    /// R680 atomic 2 §5.16 §5.41 — drain a single window's redraw
    /// flag. Returns `true` once for each
    /// [`Self::request_redraw_for_window`] call between drains;
    /// resets the flag to `false` so the next event-loop iteration
    /// sees a clean state. Unknown `window_id` (never requested)
    /// returns `false` without allocating.
    ///
    /// [`crate::AppShell::drain_redraw_to_winit`] calls this for
    /// every active window slot to determine the per-window
    /// `Window::request_redraw` dispatch.
    pub fn take_redraw_request_for_window(&mut self, window_id: &str) -> bool {
        self.redraw_requested_per_window
            .get_mut(window_id)
            .is_some_and(std::mem::take)
    }

    /// R680 atomic 2 §5.16 §5.41 — peek-only probe for the
    /// per-window redraw flag. Returns `true` when
    /// [`Self::request_redraw_for_window`] has set the flag since
    /// the last [`Self::take_redraw_request_for_window`] drain.
    /// Used by tests + debug logging that want to assert "yes the
    /// caller targeted window X" without consuming the signal.
    #[must_use]
    pub fn redraw_requested_for_window(&self, window_id: &str) -> bool {
        self.redraw_requested_per_window
            .get(window_id)
            .copied()
            .unwrap_or(false)
    }

    /// R681 §2 #4 atomic 2 §5.16 §5.28 — per-window last paint
    /// [`Instant`]. The substrate's
    /// [`Self::compute_paint_scene_internal`] writes this slot every
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
        self.last_paint_instants.get(window_id).copied()
    }

    /// R681 §2 #4 atomic 3 §5.16 §5.28 — per-window target fps
    /// override for the game-loop pacing branch. Bindings call this
    /// to opt a window into 30fps (battery saver) / 144fps
    /// (high-refresh display) / 0 (paused polled window sentinel)
    /// instead of the default
    /// [`pinion_runtime::frame_pacing::DEFAULT_IMMEDIATE_MODE_FPS`].
    ///
    /// The override is consulted by
    /// [`pinion_shell::AppShell::about_to_wait`] each event-loop
    /// iteration via
    /// [`pinion_runtime::frame_pacing::frame_budget_for_window`];
    /// re-calling this method with a different `fps` is the
    /// canonical "change pacing on the fly" surface.
    pub fn set_target_fps_for_window(&mut self, window_id: &str, fps: u32) {
        self.target_fps_per_window.insert(window_id.to_owned(), fps);
        // R831 §2 #4 §5.28 — pausing (`fps == 0`) snaps the immediate-mode
        // accumulator back to a zero phase, so an AI client that then
        // frame-steps via `scene/tick` advances from a known fixed-step
        // boundary (the deterministic-debugging contract — a debugger
        // breaks between frames, not mid-sub-step). The discarded
        // remainder is sub-fixed (< ~8 ms of simulation). Resume keeps
        // the post-pause accumulator (fresh zero phase) so live wall-clock
        // restarts cleanly.
        if fps == 0 {
            if let Some(acc) = self.sim_accumulator_per_window.get_mut(window_id) {
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
        self.target_fps_per_window.remove(window_id);
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
            input_state: self.core.input_state_snapshot(wid, Some(self.modifiers)),
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

    /// R681 §2 #4 atomic 3 §5.16 §5.28 — read the per-window target
    /// fps override, if set. `None` means "use the default policy"
    /// (60fps when immediate-mode-active, idle otherwise — see
    /// [`pinion_runtime::frame_pacing::default_window_frame_policy`]).
    #[must_use]
    pub fn target_fps_for_window(&self, window_id: &str) -> Option<u32> {
        self.target_fps_per_window.get(window_id).copied()
    }

    /// R681 §2 #4 §5.16 — publish whether `window_id`'s most recently
    /// painted scene carried an immediate-mode subtree. Called by
    /// `AppShell::render_window` every paint cycle (the sticky signal
    /// the next `about_to_wait` pacing decision + the jank profiler both
    /// read), keeping the immediate-mode flag in the same home as the
    /// `target_fps` override so the two pacing inputs never drift.
    pub fn set_immediate_subtree_for_window(&mut self, window_id: &str, has_immediate: bool) {
        self.immediate_subtree_per_window
            .insert(window_id.to_owned(), has_immediate);
    }

    /// R681 §2 #4 §5.16 — whether `window_id`'s last painted scene
    /// carried an immediate-mode subtree. `false` for an unknown or
    /// never-painted window (the retained-tree default). The pacing
    /// input both `about_to_wait` and the jank budget
    /// ([`Self::frame_timings_for_window`]) consult through
    /// [`pinion_runtime::frame_pacing::frame_budget_for_window`].
    #[must_use]
    pub fn immediate_subtree_for_window(&self, window_id: &str) -> bool {
        self.immediate_subtree_per_window
            .get(window_id)
            .copied()
            .unwrap_or(false)
    }

    /// R682 §5.16 atomic 3 — publish a [`FragmentCacheStats`]
    /// snapshot for the given window. Surface-side
    /// `AppShell::render_window` calls this after each paint cycle so
    /// the GUI-agnostic substrate can surface cache observability to
    /// RPC / tests without exposing `vello::Scene` references.
    pub fn publish_fragment_cache_stats(&mut self, window_id: &str, stats: FragmentCacheStats) {
        self.fragment_cache_stats_per_window
            .insert(window_id.to_owned(), stats);
    }

    /// R907 §5.16 §5.7 — record one painted frame's phase breakdown
    /// into the window's rolling [`FrameTimingStats`] window. The
    /// surface-side `AppShell::render_window` calls this after each
    /// paint cycle with the measured build / encode / render / total
    /// microseconds. Lazily inserts a fresh accumulator on the
    /// window's first paint (the `sim_accumulator_per_window` pattern).
    pub fn record_frame_timing(&mut self, window_id: &str, timing: FrameTiming) {
        self.frame_timings_per_window
            .entry(window_id.to_owned())
            .or_default()
            .record(timing);
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
    pub fn record_presented_frame(
        &mut self,
        window_id: &str,
        present_ok: bool,
        viewport: (u32, u32),
        scene: &pinion_core::Scene,
    ) {
        let paint_seq = self
            .render_fidelity_per_window
            .get(window_id)
            .map_or(1, |prev| prev.paint_seq.wrapping_add(1));
        self.render_fidelity_per_window.insert(
            window_id.to_owned(),
            pinion_runtime::RenderFidelity {
                paint_seq,
                presented_at_ms: pinion_runtime::render_fidelity::elapsed_ms(),
                present_ok,
                viewport_w: viewport.0,
                viewport_h: viewport.1,
                grids: pinion_runtime::render_fidelity::grid_fidelity(scene),
            },
        );
    }

    /// R683 §5.16 §5.41 — drop every shell-side per-window state
    /// entry for `window_id`.
    ///
    /// Walks the per-window `HashMap`s lifted onto `ShellCore` since
    /// R680 atomic 2 / R681 atomic 3 / R682 atomic 3 / R829 / R831 / R25.1
    /// (`redraw_requested_per_window`, `last_paint_instants`,
    /// `target_fps_per_window`, `fragment_cache_stats_per_window`,
    /// `frame_timings_per_window`, `pending_immediate_dt_per_window`,
    /// `sim_accumulator_per_window`, `focusable_tags_per_window`) and drops the
    /// entry keyed by `window_id`. Then forwards into
    /// [`pinion_runtime::CoreShell::remove_window`] which drains the
    /// runtime-side per-window state (`routers`, `window_owners`).
    ///
    /// R25.1 §5.39 — dropping the closed window's focusable contribution and
    /// re-folding the union ([`Self::union_focusable_tags`]) removes its tags
    /// from the Tab order / click-focus set, and the §5.39 stale-focus guard
    /// inside `update_focusable_tags` drops focus if it pointed at one of the
    /// now-unpainted widgets. R26 §5.16 — the tags actually leave the union only
    /// because [`crate::AppShell::reconcile_windows`]'s drop-pass removes the spec
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
    /// Designed for the R683 [`crate::AppShell::reconcile_windows`]
    /// Effect drop pass after a dock tear-off / dock-back arc
    /// resolves.
    pub fn remove_window(&mut self, window_id: &str) -> bool {
        if window_id == pinion_runtime::DEFAULT_WINDOW {
            return false;
        }
        let shell_side = self.redraw_requested_per_window.remove(window_id).is_some()
            | self.last_paint_instants.remove(window_id).is_some()
            | self.target_fps_per_window.remove(window_id).is_some()
            | self
                .pending_immediate_dt_per_window
                .remove(window_id)
                .is_some()
            | self.sim_accumulator_per_window.remove(window_id).is_some()
            | self
                .fragment_cache_stats_per_window
                .remove(window_id)
                .is_some()
            | self.frame_timings_per_window.remove(window_id).is_some()
            | self.render_fidelity_per_window.remove(window_id).is_some()
            | self.focusable_tags_per_window.remove(window_id).is_some();
        // R25.1 §5.39 — re-fold the focus enumeration without the closed
        // window's tags. Idempotent when the window held no focusables (the
        // union is unchanged and `update_focusable_tags` re-finds the focused
        // tag); when it DID, those tags leave the Tab order and a focus that
        // pointed at one of them is dropped by the §5.39 stale guard. Off the
        // paint path (a rare dock tear-down), so the O(windows) fold is free.
        let union = self.union_focusable_tags();
        self.focus.update_focusable_tags(union);
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
    /// ([`crate::AppShell::resume_spec`], before the first paint) so
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
        self.fragment_cache_stats_per_window.get(window_id).copied()
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
    /// read — never on the paint path ([[r890]] read-time projection).
    #[must_use]
    pub fn frame_timings_for_window(&self, window_id: &str) -> Option<FrameTimingsSnapshot> {
        let budget_us = self.jank_budget_us_for_window(window_id);
        self.frame_timings_per_window
            .get(window_id)
            .and_then(|stats| stats.snapshot(budget_us))
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
        self.render_fidelity_per_window.get(window_id).cloned()
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
        self.fragment_cache_stats_per_window
            .keys()
            .map(String::as_str)
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

    /// R1072 §5.37 — inject a specific self-hosted text engine, overriding the
    /// `PINION_TEXT_ENGINE` env selection made at construction. The seam a test
    /// drives with a bundled font fixture (system font discovery is
    /// environment-dependent and would skip the §5.37 arms on a font-less
    /// machine), and the hook a future caller uses to supply an embedded brand
    /// font. Pass `None` to force the parley path.
    pub fn set_text_engine(&mut self, engine: Option<SelfHostedTextEngine>) {
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
    /// happens in [`Self::handle_tail`].
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
    /// [`WidgetView::apply_key`] and, on handled
    /// (`Some(DispatchTail)` from [`CoreShell::apply_key`]), run the
    /// same post-input bookkeeping as [`Self::forward`]: bump the
    /// §5.34 revision, re-read cached state (paint on visible
    /// change), drain pending intents. Unhandled keys (`None` return)
    /// are swallowed quietly (same shape as an unmatched
    /// [`WidgetView::keybinding`]).
    pub fn apply_key(&mut self, key: &str) {
        self.apply_key_inner(key, false);
    }

    /// R1071 PR-27 §5.39 §5.35 — body of [`Self::apply_key`] carrying the
    /// platform auto-repeat flag through to [`CoreShell::apply_key_repeat`]
    /// (and thence the binding's [`WidgetCore::apply_key_repeat`]). The
    /// public `apply_key` is the `repeat == false` wrapper; the GUI keyboard
    /// path ([`Self::handle_character_key_inner`]) and the per-window seam
    /// ([`Self::key_press_for_window`]) pass the real flag.
    pub(crate) fn apply_key_inner(&mut self, key: &str, repeat: bool) {
        let focused = self.focus.focused().map(str::to_owned);
        if let Some(tail) =
            self.core
                .apply_key_repeat(focused.as_deref(), key, self.modifiers, repeat)
        {
            self.revision.bump();
            self.handle_tail(&tail);
        }
    }

    /// R56.2.a §5.13 §5.38 — route an IME [`CompositionEvent`] through
    /// [`WidgetView::apply_composition`] and, on handled
    /// (`Some(DispatchTail)` from [`CoreShell::apply_composition`]),
    /// run the same post-input bookkeeping as [`Self::apply_key`]:
    /// bump the §5.34 revision, drain pending intents via
    /// [`Self::handle_tail`] (which re-reads cached state and
    /// requests a redraw on visible change).
    ///
    /// pinion-shell's `AppShell::window_event` `WindowEvent::Ime`
    /// arm converts winit 0.30's cross-platform
    /// [`Ime`](https://docs.rs/winit/0.30/winit/event/enum.Ime.html)
    /// enum (`Enabled` / `Preedit(text, range)` / `Commit(text)` /
    /// `Disabled`) into [`CompositionEvent`] with a `was_composing`
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
    /// [`WidgetView::apply_middle_click`] and, on handled
    /// (`Some(DispatchTail)` from [`CoreShell::apply_middle_click`]),
    /// run the same post-input bookkeeping as [`Self::apply_key`]:
    /// bump the §5.34 revision, drain pending intents via
    /// [`Self::handle_tail`] (which re-reads cached state and
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
    /// text widget"; [`TextField::apply_middle_click`] reads PRIMARY
    /// via the R56.2.e [`Clipboard::paste_from`] extension and
    /// inserts at the caret. On macOS / Windows the
    /// [`Clipboard::paste_from`] default impl returns `None` for
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
    ///   the xterm / Qt convention and keeps a paste off every pan.
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
    /// through [`WidgetView::apply_secondary_click`], anchoring a context
    /// menu at the cursor. Reads the addressed window's cached cursor
    /// position (`CoreShell::cursor_position_for_window`, the channel
    /// `position_caret_for_point` uses) and forwards it to
    /// [`CoreShell::apply_secondary_click`]; on handled
    /// (`Some(DispatchTail)`) it runs the same post-input bookkeeping as
    /// [`Self::middle_click`] — bump the §5.34 revision, drain intents via
    /// [`Self::handle_tail`] (re-reads cached state, redraws on change).
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
    /// First consults [`WidgetView::keybinding`]; on `Some(event)`
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
    /// [`WidgetCore::apply_key_repeat`] sees it. The public method is the
    /// `repeat == false` wrapper.
    pub(crate) fn handle_character_key_inner(&mut self, c: &str, repeat: bool) {
        if let Some(ev) = V::keybinding(c) {
            self.forward(ev);
        } else {
            self.apply_key_inner(c, repeat);
        }
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
    /// `Tab` never reaches this method — it is shell-reserved in
    /// `AppShell::handle_key_press` and routes through
    /// [`Self::handle_focus_traverse`]. `Escape` normally quits the
    /// window (`event_loop.exit`), but R693 §5.39 routes it *here* while
    /// a modal focus trap is active ([`Self::focus_is_modal`]) so the
    /// dialog binding's `apply_key` can map Escape → cancel instead of
    /// terminating the app.
    pub fn handle_named_key(&mut self, key_str: &str) {
        self.handle_named_key_inner(key_str, false);
    }

    /// R1071 PR-27 §5.39 §5.35 — body of [`Self::handle_named_key`] carrying
    /// the platform auto-repeat flag through to the widget arc
    /// ([`Self::try_apply_key_inner`] → [`WidgetCore::apply_key_repeat`]).
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
    /// [`WidgetView::apply_key`] and run the post-input bookkeeping
    /// (revision bump + [`Self::handle_tail`]) when it handles the key.
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
    /// (and the binding's [`WidgetCore::apply_key_repeat`]). The public
    /// method is the `repeat == false` wrapper; the GUI named-key path
    /// ([`Self::handle_named_key_inner`]), the Escape / Tab offer-first arcs,
    /// and the per-window seam ([`Self::key_press_for_window`]) pass the real
    /// flag.
    pub(crate) fn try_apply_key_inner(&mut self, key_str: &str, repeat: bool) -> bool {
        let focused = self.focus.focused().map(str::to_owned);
        if let Some(tail) =
            self.core
                .apply_key_repeat(focused.as_deref(), key_str, self.modifiers, repeat)
        {
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
    /// [`WidgetView::apply_key`] reports the key unhandled.
    /// Forwards through [`CoreShell::scroll_key`](pinion_runtime::CoreShell::scroll_key)
    /// which walks the deepest [`Scene::Scroll`](pinion_core::scene::Scene::Scroll)
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
    fn run_file_hook(
        &mut self,
        window_id: &str,
        hook: impl FnOnce(&<V as pinion_core::WidgetCore>::State) -> bool,
    ) {
        let state = *self.cached_state();
        let owner = self.root_owner().clone();
        if owner.run(|| hook(&state)) {
            self.request_redraw_for_window(window_id);
        }
    }

    /// R770 §5.15 — winit `HoveredFile` / `scene/hover_file` entry: a file
    /// is dragged over `window_id`. Routes `path` to
    /// [`WidgetView::on_file_hover`].
    pub fn file_hover_for_window(&mut self, window_id: &str, path: &str) {
        self.run_file_hook(window_id, |state| V::on_file_hover(state, path));
    }

    /// R770 §5.15 — winit `HoveredFileCancelled` / `scene/hover_file_cancel`
    /// entry: a file drag left `window_id` without dropping. Routes to
    /// [`WidgetView::on_file_hover_cancel`].
    pub fn file_hover_cancel_for_window(&mut self, window_id: &str) {
        self.run_file_hook(window_id, |state| V::on_file_hover_cancel(state));
    }

    /// R770 §5.15 — winit `DroppedFile` / `scene/drop_file` entry: a file
    /// was dropped on `window_id`. Routes `path` to
    /// [`WidgetView::on_file_drop`].
    pub fn file_drop_for_window(&mut self, window_id: &str, path: &str) {
        self.run_file_hook(window_id, |state| V::on_file_drop(state, path));
    }

    /// R51.80 §5.35 — winit `CursorMoved` dispatch decoupled from
    /// winit at the [`ShellCore`] surface. Forwards through
    /// [`CoreShell::cursor_moved`] (which performs the router walk +
    /// post-dispatch tail), then routes the tail through
    /// [`Self::handle_tail`].
    ///
    /// R672 §5.35 — single-window wrapper around
    /// [`Self::cursor_moved_for_window`].
    pub fn cursor_moved(&mut self, pid: PointerId, x: f64, y: f64) {
        self.cursor_moved_for_window(pinion_runtime::DEFAULT_WINDOW, pid, x, y);
    }

    /// R672 §5.35 §5.41 — per-window variant of [`Self::cursor_moved`].
    /// `AppShell::window_event` dispatches winit `CursorMoved` here
    /// with the resolved [`crate::WindowSpec::id`] so the addressed
    /// window's [`pinion_runtime::InputRouter`] handles the
    /// cursor + `refresh_hover` walk independently of other windows.
    pub fn cursor_moved_for_window(&mut self, window_id: &str, pid: PointerId, x: f64, y: f64) {
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
        // R882 / R882.1 §5.35 §5.39 — the press routes through the
        // substrate's LEFT front door
        // ([`CoreShell::left_press_for_window`]): the Space-hold pan
        // chord (Figma / Photoshop hand tool) and the live-pan
        // swallow are substrate policy, owned ONCE in `CoreShell`
        // (the R882 first cut kept the branch per shell — the §2 #6
        // divergence class). `None` = the pan channel consumed the
        // press: no widget `PointerDown` and none of the press
        // follow-ups below run — no click-to-focus (a pan must not
        // steal focus), no caret positioning, no immediate-mode
        // forward.
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
    /// [`Self::handle_touch`] (which calls [`CoreShell::touch_event`]
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
    /// walks the deepest [`Scene::Scroll`](pinion_core::scene::Scene::Scroll)
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

    /// R672 §5.35 §5.41 — per-window variant of [`Self::wheel`].
    ///
    /// R877 §5.15 §5.49 — forwards the shell's modifier cache (winit
    /// `ModifiersChanged`, or the R763 `scene/modifiers` RPC mirror —
    /// one cache, both producers), so a hovered `External` wheel
    /// consumer (canvas pan / `Ctrl`-zoom) reads the held modifiers
    /// the same way `scene/click` Shift-extend does.
    pub fn wheel_for_window(&mut self, window_id: &str, pid: PointerId, delta: WheelDelta) {
        let (tail, dispatched) =
            self.core
                .wheel_with_modifiers_for_window(window_id, pid, delta, self.modifiers);
        if dispatched {
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
    // R1026 — rustfmt's reflow pushed this 1 line over the workspace
    // too_many_lines (100) ceiling it was kept just under; the body is a flat
    // per-input dispatch, not bloat. Extraction is deferred to the owner.
    #[allow(clippy::too_many_lines)]
    fn drain_deferred_inputs_for_window(&mut self, window_id: &str, inputs: &[DeferredInput]) {
        // `DeferredInput` is `non_exhaustive`; the wildcard arm
        // covers future variants (key, cursor_only, etc.) silently
        // no-op against this drain until a follow-up round extends
        // the match.
        for input in inputs {
            match *input {
                DeferredInput::Wheel { x, y, delta } => {
                    self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
                    self.wheel_for_window(window_id, PointerId::MOUSE, delta);
                }
                DeferredInput::Click { x, y } => {
                    self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
                    self.mouse_pressed_for_window(window_id, PointerId::MOUSE);
                    self.mouse_released_for_window(window_id, PointerId::MOUSE);
                }
                // R887 §5.49 §5.53 — `scene/click {button: "right"}`
                // mirror: seed the router's cursor cache, then take the
                // exact arc a physical `MouseInput { button: Right,
                // state: Pressed }` takes (`secondary_click_for_window`
                // reads that cache — the press-edge one-shot, no
                // release half).
                DeferredInput::SecondaryClick { x, y } => {
                    self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
                    self.secondary_click_for_window(window_id, PointerId::MOUSE);
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
                    self.mouse_pressed_for_window(window_id, PointerId::MOUSE);
                    self.mouse_released_for_window(window_id, PointerId::MOUSE);
                    self.mouse_pressed_for_window(window_id, PointerId::MOUSE);
                    self.mouse_released_for_window(window_id, PointerId::MOUSE);
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
                    pinion_runtime::substep(dt, |step| {
                        self.core.tick_animations_for_window(window_id, step);
                    });
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
                            .pending_immediate_dt_per_window
                            .entry(window_id.to_owned())
                            .or_insert(0.0) += dt;
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
                } => {
                    self.drain_drag_for_window(
                        window_id,
                        (from_x, from_y),
                        (to_x, to_y),
                        steps,
                        button,
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
    fn drain_drag_for_window(
        &mut self,
        window_id: &str,
        from: (f64, f64),
        to: (f64, f64),
        steps: u32,
        button: DragButton,
    ) {
        self.cursor_moved_for_window(window_id, PointerId::MOUSE, from.0, from.1);
        match button {
            DragButton::Left => self.mouse_pressed_for_window(window_id, PointerId::MOUSE),
            DragButton::Middle => self.middle_pressed_for_window(window_id, PointerId::MOUSE),
        }
        if steps > 0 {
            for step in 1..=steps {
                let t = f64::from(step) / f64::from(steps);
                let x = from.0 + (to.0 - from.0) * t;
                let y = from.1 + (to.1 - from.1) * t;
                self.cursor_moved_for_window(window_id, PointerId::MOUSE, x, y);
            }
        }
        match button {
            DragButton::Left => self.mouse_released_for_window(window_id, PointerId::MOUSE),
            DragButton::Middle => self.middle_released_for_window(window_id, PointerId::MOUSE),
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
    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    /// R882 §5.39 §5.35 — held-key absolute-state funnel: record that
    /// `key` (the canonical W3C `KeyboardEvent.key` string the winit
    /// boundary's `named_key_str` emits) is now held / released. ONE
    /// funnel for every producer — both edges of the winit
    /// `KeyboardInput` arm and the `scene/key state:"down"/"up"` RPC
    /// peer — so the native and AI-driven chord can never diverge.
    /// The cache (and the routing policy it gates) lives in the
    /// backend-agnostic substrate — R882.1 moved both into
    /// [`CoreShell`](pinion_runtime::CoreShell) so the GUI and TUI
    /// shells are pure edge producers with zero policy copies (§2 #6);
    /// this is the winit-side forwarding funnel.
    pub fn note_key_state(&mut self, key: &str, pressed: bool) {
        self.core.note_key_state(key, pressed);
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
    pub fn window_focused(&mut self) {
        if self.focus.restore() {
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
    /// re-delivers `ModifiersChanged` on refocus so [`Self::modifiers`]
    /// resyncs itself; held keys have no such resync event, hence the
    /// explicit clear.
    pub fn window_blurred(&mut self) {
        self.focus.save();
        self.core.clear_held_keys();
    }

    /// R1071 PR-27 §5.39 §5.16 §5.35 — record a winit `WindowEvent::Focused`
    /// edge for `window_id`, maintaining the [`Self::os_focused_window`] the
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
    pub fn note_os_focus(&mut self, window_id: &str, focused: bool) {
        if focused {
            self.os_focused_window = Some(window_id.to_owned());
        } else if self.os_focused_window.as_deref() == Some(window_id) {
            self.os_focused_window = None;
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

    /// R1071 PR-27 §5.39 §5.16 §5.35 — per-window named-key (shortcut)
    /// injection, the keyboard peer of the per-window pointer entries
    /// ([`Self::mouse_pressed_for_window`] et al.). Applies the OS-focus gate
    /// ([`Self::is_key_dispatch_window`]) then routes `key` through the
    /// named-key arc ([`Self::handle_named_key_inner`]) carrying `repeat`.
    /// Returns whether the gate ADMITTED the press (`true` = dispatched to the
    /// widget arc regardless of whether a widget consumed it; `false` = gated
    /// out because the press arrived at a non-focused window).
    ///
    /// This is the substrate seam that makes the multi-window toggle
    /// double-dispatch reproducible and regression-testable headlessly — a
    /// `#[test]` drives "a key (with `repeat`) arrives at window X" with no
    /// winit `EventLoop`, wgpu device, or external key injector, exactly as
    /// the pointer seam lets tests drive per-window clicks. `AppShell`'s
    /// `WindowEvent::KeyboardInput` arm drives the SAME gate for the live GUI.
    ///
    /// Modifiers are read from the out-of-band [`Self::modifiers`] cache (the
    /// winit `ModifiersChanged` mirror — winit's `KeyEvent` carries no
    /// modifier state, so the real path reads the cache too); a test sets them
    /// via [`Self::set_modifiers`] before driving a modifier-bearing shortcut.
    pub fn key_press_for_window(&mut self, window_id: &str, key: &str, repeat: bool) -> bool {
        if !self.is_key_dispatch_window(window_id) {
            return false;
        }
        self.handle_named_key_inner(key, repeat);
        true
    }

    /// R51.80 §5.16 §5.36 — compute one frame's paint scene from the
    /// cached state.
    ///
    /// Encapsulates the per-frame pump that the surface-side render
    /// path drives every redraw:
    ///
    /// 1. Measure `dt` against the previous paint timestamp
    ///    ([`Self::last_paint_instant`]) — `0.0` on the very first
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
            .last_paint_instants
            .get(window_key)
            .map_or(0.0_f32, |prev| now.duration_since(*prev).as_secs_f32());
        self.last_paint_instants.insert(window_key.to_owned(), now);
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
        // The view + its two post-layout re-passes (scroll-dirty, pane-dirty)
        // all re-run the SAME `V::view` / `V::view_for_window` dispatch; factor
        // it into one closure so the dispatch lives in exactly one place. The
        // closure captures only `&self.core` (disjoint from the `&mut
        // self.text_cache` the `compute_layout`s borrow) and is scoped to this
        // block so its `&self.core` borrow drops before the later `&mut self`
        // paint-loop work. NOTE: only the dispatch is shared — the first pass
        // lays out via `compute_layout_with_scroll_dirty` (to read the dirty
        // bit), the re-passes via plain `compute_layout`; folding the layout in
        // too would double-lay-out the first pass.
        let mut paint_scene = {
            let core = &self.core;
            let run_view = || {
                core.root_owner().run(|| match window_id {
                    Some(id) => V::view_for_window(id, cached_state, &frame),
                    None => V::view(cached_state, &frame),
                })
            };
            // R1072 §5.37 — the opt-in self-hosted measure override (None unless
            // `PINION_TEXT_ENGINE` is enabled). Borrows `self.text_engine` as a
            // field disjoint from the `&mut self.text_cache` the layout passes
            // take; `Copy`, so the same override threads through every re-pass.
            // Paired with the §5.37 paint arm at the `text_cache_and_engine`
            // site so both arms size + render eligible text identically.
            let text_measure = text_measure_override(self.text_engine.as_ref());
            let mut scene = run_view();
            let scroll_dirty = compute_layout_with_text_measure(
                &mut scene,
                &mut self.text_cache,
                w,
                h,
                text_measure,
            );
            // R57.X.scrollbar §5.45 — first-paint chicken-and-egg fix.
            // The layout pass writes the post-layout
            // [`ScrollState::set_max`] *after* `V::view` has already
            // produced the scene. The scrollbar widget reads `max` inside
            // `V::view` and renders thumb size as
            // `f(viewport, viewport + max)` — on the very first paint of
            // the application's lifetime `max == 0` resolves to "content
            // fits viewport" and paints a full-track thumb the user sees
            // as "scrollbar maxed out at startup". Re-running `V::view` +
            // `compute_layout` once when the layout pass actually moved
            // a bound lets the scrollbar widget pick up the freshly-
            // written max on the same paint cycle. Idempotent on
            // steady-state frames — Signal equality-skip floors
            // `scroll_dirty` at `false` and the guard short-circuits.
            if scroll_dirty {
                scene = run_view();
                let _ = compute_layout_with_text_measure(
                    &mut scene,
                    &mut self.text_cache,
                    w,
                    h,
                    text_measure,
                );
            }
            // R1012 §5.23 §5.22 — per-pane viewport publish. The freshly laid-out
            // scene carries each pane Container's measured pixel rect; publish
            // each registered pane tag's (w, h) so a per-pane reflow Effect (a
            // PTY winsize ioctl) reacts. This is the post-layout sibling of the
            // pre-view `set_viewport_size` publish above: a pane size is
            // layout-derived (known only here, after layout), so — like the
            // scroll-dirty bit (R774) — the publish returns a dirty bit and we
            // re-run `view` + `compute_layout` once when it fires, so the re-run
            // reads the post-reflow producer state on this same paint. Idempotent
            // on steady-state frames (Signal equality-skip floors `pane_dirty` at
            // `false`).
            //
            // R1021 §5.23 §5.16 — published for EVERY painted window, NOT
            // primary-only (unlike the R1006 `set_viewport_size` publish above,
            // which stays `DEFAULT_WINDOW`-gated). The pane registry is
            // `root_owner`-scoped and tag-keyed, so it is window-agnostic: each
            // painted window publishes the rects of the tags IT draws, and a tag
            // absent from this window's scene resolves `rect_for_tag_absolute →
            // None` and is skipped (retains its last measured size — a foreign
            // window's pane is never clobbered). In the dock model a pane tag is
            // drawn in exactly one window at a time, so there is no ambiguity. This
            // is what lets a torn-off (undock) pane reflow to its secondary
            // window's size: that window's content fills `(w, h)` via
            // `compute_layout`'s root-fill (the root's declared size is ignored at
            // the top level), so the pane Container's measured rect IS the window
            // rect — no per-window `use_viewport_size` is needed (the R1006
            // window-size seam stays primary-gated; sprag R37 undock is the forcing
            // consumer). The side-effect-free mirror
            // (`compute_paint_scene_pure_internal`) never reaches this fn, so an
            // introspection paint never publishes (the R1006 contract, inherited).
            if self.core.publish_pane_viewports(&scene) {
                scene = run_view();
                let _ = compute_layout_with_text_measure(
                    &mut scene,
                    &mut self.text_cache,
                    w,
                    h,
                    text_measure,
                );
            }
            scene
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
        // `paused` (computed once) gates BOTH this frame's source clock
        // AND the continuous-loop redraw re-arm further down, so the two
        // cannot disagree on the pause state.
        let paused = self.target_fps_for_window(window_key) == Some(0);
        // Simulation seconds to advance this frame:
        //   - a pending `scene/tick` injection (an AI client frame-stepping
        //     a — typically paused — window), if present; else
        //   - `0.0` when paused: the sim clock is FROZEN, so an incidental
        //     repaint (a click forwarding `on_pointer_down`, a focus-ring
        //     redraw, a resize) does NOT fast-forward a long-paused game;
        //   - the clamped wall-clock delta otherwise (the live loop).
        let sim_dt = if let Some(injected) = self.pending_immediate_dt_per_window.remove(window_key)
        {
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
        self.sim_accumulator_per_window
            .entry(window_key.to_owned())
            .or_default()
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
        let task_pump = self.core.root_owner().local_task_pump();
        if task_pump.has_pending() {
            task_pump.poll();
            self.redraw_requested = true;
        }
        // R705.1 §2 #7 — this paint just consumed the current reactive
        // state (the `V::view` run above re-subscribed `root_owner` to
        // every `Signal` it read). Reset the dirty flag so the NEXT
        // `Signal::set` re-flags it — the `handle_tail` `is_dirty()` bridge
        // then knows a real change occurred and a benign no-op did not.
        self.core.root_owner().clear_dirty();
        self.apply_focus_ring(paint_scene, w, h)
    }

    /// R705 §5.39 §2 #1/#7 — inject the keyboard focus ring as an
    /// introspectable, pointer-transparent overlay [`Scene::Box`].
    /// Applied as the final step of every paint-scene producer
    /// ([`Self::compute_paint_scene_internal`],
    /// [`Self::compute_paint_scene_pure_internal`], and the
    /// `dispatch_rpc_inner` RPC produce closure) so the ring is
    /// (a) painted by the generic box path rather than an opaque vello
    /// stroke, (b) visible to `scene/snapshot from: paint` (§2 #7), and
    /// (c) corner-radius-aware (concentric on rounded widgets). No-op
    /// when nothing is focused. The injected box is pointer-transparent
    /// (R705 §5.39 substrate) so it never shadows its widget for input,
    /// even though the very scene returned here also feeds
    /// [`pinion_runtime::InputRouter::last_paint_scene`] hit-testing.
    fn apply_focus_ring(&self, scene: Scene, w: u32, h: u32) -> Scene {
        let Some(focused) = self.focus.focused() else {
            return scene;
        };
        // R705 §5.39 §5.40 — resolve through the active-descendant SSOT
        // ([`resolve_focus_ring_tag`]) so a roving widget's ring tracks
        // its active cell rather than wrapping the container.
        let ring_tag =
            resolve_focus_ring_tag::<V>(self.core.cached_state(), focused, self.core.root_owner());
        // R1010 §5.39 §5.40 — the binding owns the ring style for the rung tag
        // (None suppresses it; the default draws the framework ring).
        // R1022 §5.39 — `(w, h)` = the layout viewport the scene was laid out to,
        // so the ring's far edges clamp on-screen for a window-flush widget.
        inject_styled_focus_ring::<V>(scene, Some(&ring_tag), Some((w, h)))
    }

    /// R670.B §5.16 — per-window paint scene producer. Same pipeline
    /// as [`Self::compute_paint_scene`] but routes through
    /// [`WidgetView::view_for_window`](crate::WidgetView::view_for_window)
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
    /// [`Self::compute_paint_scene_internal`]; the parity carry from
    /// R670.B is permanently cleared by the unified producer.
    pub fn compute_paint_scene_for_window(&mut self, window_id: &str, w: u32, h: u32) -> Scene {
        self.compute_paint_scene_internal(Some(window_id), w, h)
    }

    /// R671 §5.16 — primary / single-window paint scene producer. Thin
    /// wrapper around [`Self::compute_paint_scene_internal`] with
    /// `window_id == None`, which routes through `V::view` exactly
    /// like the pre-R670.B implementation.
    pub fn compute_paint_scene(&mut self, w: u32, h: u32) -> Scene {
        self.compute_paint_scene_internal(None, w, h)
    }

    /// (R684.B atomic 2 §5.16) Pure paint-scene producer — `V::view`
    /// + `compute_layout` only, no side effects.
    ///
    /// Splits the R670.B [`Self::compute_paint_scene_internal`]
    /// composition into the deterministic geometry half (this fn) +
    /// the side-effect half (animation tick, immediate-mode tick,
    /// scroll-dirty re-run guard, animation-active redraw arming).
    /// Use cases:
    ///
    /// * RPC dispatch's post-finalize hook (R684 atomic 3 →
    ///   R684.B atomic 1 / 2 rewrite): the producer closure that
    ///   resolved hit-test paths already ran the full
    ///   [`Self::compute_paint_scene_internal`] (tick, view, layout,
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
    /// `Owner::cache` slots resolve correctly. The R57.X scrollbar
    /// first-paint chicken-and-egg fix is intentionally NOT
    /// retained — that fix's purpose is to let the scrollbar widget
    /// observe a `max` update written by the layout pass within the
    /// same paint cycle, but the pure variant runs as an auxiliary
    /// after the canonical paint already produced the right
    /// scrollbar shape; re-running the scroll-dirty guard here
    /// would just emit redundant work.
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
        let cached_state = *self.core.cached_state();
        let frame = Frame::new();
        let mut paint_scene = self.core.root_owner().run(|| match window_id {
            Some(id) => V::view_for_window(id, cached_state, &frame),
            None => V::view(cached_state, &frame),
        });
        // R1072 §5.37 — measure the introspection mirror with the same opt-in
        // engine the production paint path uses, so `scene/layout` reports the
        // boxes that were (or would be) painted via §5.37 (Scene-as-data coherence,
        // §2 #7). `None` keeps it parley-identical.
        let text_measure = text_measure_override(self.text_engine.as_ref());
        let _ = compute_layout_with_text_measure(
            &mut paint_scene,
            &mut self.text_cache,
            w,
            h,
            text_measure,
        );
        // R705.1 §2 #7 — see `compute_paint_scene_internal`: this auxiliary
        // (re-store / headless) render also consumed the current reactive
        // state, so reset the dirty flag in lockstep.
        self.core.root_owner().clear_dirty();
        self.apply_focus_ring(paint_scene, w, h)
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
    /// Hands the rendered scene to the [`InputRouter`] so the next
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
    /// [`Self::notify_focus_change`] so any [`External`] whose
    /// focused state crossed the boundary receives
    /// [`External::on_focus_change`]. The `TextField` statechart
    /// (R56.1.a) consumes this hook to drive its Focus / Blur SCXML
    /// events and sync the [`CaretBlink`] enabled gate (R56.1.c).
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
        // programmatic `drain_focus_request` path gates its redraw on.
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
        // mirroring `drain_focus_request`'s programmatic-focus pairing. The
        // focus ring is paint-time-injected (`apply_focus_ring`) and a
        // `FocusManager` mutation dirties no reactive owner, so without this the
        // ring lags to the next unrelated repaint (sprag PR-13). The wake is
        // binding-wide (`request_redraw`), not per-window: the focused tag is
        // binding-wide state and `apply_focus_ring` reads the single
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
    /// [`External::on_focus_change`] on the old and new externals
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
    /// path through [`crate::AppShell::dispatch_rpc`], which parses
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
    ) -> Option<String> {
        let scope: String = request
            .window_scope()
            .ok()
            .flatten()
            .unwrap_or(pinion_runtime::DEFAULT_WINDOW)
            .to_owned();
        self.dispatch_rpc_inner(request, Some(&scope), resize_request, screenshot)
    }

    /// R670.B §5.7 — single-window dispatch entry. Accepts the raw
    /// JSON-RPC envelope; parses internally then forwards to
    /// [`Self::dispatch_rpc_inner`]. Used by single-window bindings
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
        self.dispatch_rpc_inner(parsed, scope.as_deref(), resize_request, None)
    }

    // R1026 — rustfmt's reflow pushed this 4 lines over the workspace
    // too_many_lines (100) ceiling it was kept just under; the body is a flat
    // method-dispatch match, not bloat. Extraction is deferred to the owner.
    #[allow(clippy::too_many_lines)]
    fn dispatch_rpc_inner(
        &mut self,
        request: Request,
        window_id: Option<&str>,
        resize_request: &mut dyn FnMut(u32, u32),
        screenshot: Option<pinion_rpc::Screenshot>,
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
            // way `apply_focus_ring` does for the winit paint path:
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
            let (scene_ptr, last_paint_scene_ref) = self
                .core
                .scene_mut_and_last_paint_for_window(paint_window_key);
            let previews = &self.previews;
            let revision = &self.revision;
            let focus_ptr = &mut self.focus;
            let text_cache_ptr = &mut self.text_cache;
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
                let frame = Frame::new();
                let mut paint = root_owner.run(|| match producer_window_id.as_deref() {
                    Some(id) => V::view_for_window(id, cached_state, &frame),
                    None => V::view(cached_state, &frame),
                });
                // R57.X.scrollbar §5.45 — same first-paint warmup as
                // [`Self::compute_paint_scene`]. Idempotent on
                // steady-state — Signal equality-skip floors the
                // dirty bit at false.
                let text_measure = text_measure_override(text_engine_ptr.as_ref());
                if compute_layout_with_text_measure(&mut paint, text_cache_ptr, w, h, text_measure)
                {
                    paint = root_owner.run(|| match producer_window_id.as_deref() {
                        Some(id) => V::view_for_window(id, cached_state, &frame),
                        None => V::view(cached_state, &frame),
                    });
                    let _ = compute_layout_with_text_measure(
                        &mut paint,
                        text_cache_ptr,
                        w,
                        h,
                        text_measure,
                    );
                }
                // R705 §5.39 §2 #1/#7 — inject the focus ring as the
                // final paint step so `scene/snapshot from: paint` (and
                // the R684 post-dispatch InputRouter finalize, which
                // re-runs this producer) observe the same introspectable
                // pointer-transparent overlay the winit paint cycle does
                // (producer parity, [[r670b-paint-scene-producer-parity]]).
                // `focus_before` is the pre-dispatch focus sample; a
                // focus-mutating method in THIS dispatch has not yet
                // flushed when the produce closure runs for path
                // resolution, so the ring reflects entry focus — which
                // matches what the AI client addressed.
                // R1010 §5.39 §5.40 — same binding-controlled ring as the winit
                // paint path (None = no ring), through the shared SSOT.
                // R1022 §5.39 — same layout viewport `(w, h)` the produce closure
                // laid out to, so the introspected ring rect matches the winit path.
                inject_styled_focus_ring::<V>(paint, ring_tag_for_paint.as_deref(), Some((w, h)))
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
        // see this window's own frame). **First-paint only** — gated on the router
        // having no `last_paint_scene` yet — so already-active windows
        // do NOT pay any per-RPC side-effect cost.
        //
        // Why first-paint only:
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
        // pre-R672 callers.
        let drain_window = window_id.unwrap_or(pinion_runtime::DEFAULT_WINDOW);
        self.drain_deferred_inputs_for_window(drain_window, &deferred_inputs);
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
        // R664 §5.39 — drain the programmatic focus-request mailbox a
        // widget body (`External::invoke`, reducer, `Effect`) may have
        // populated during this dispatch. Routes through the same
        // `FocusManager::focus_set` + `notify_focus_change` pair the
        // mouse-driven [`Self::click_to_focus`] uses so observers
        // (`External::on_focus_change`, the `TextField` IME bridge,
        // the `CaretBlink` enable gate) see one consistent focus
        // transition arc regardless of whether the focus came from a
        // pointer press or a programmatic request.
        self.drain_focus_request();
        // R693 §5.39 — drain the modal focus-trap mailbox a reducer /
        // `External::invoke` may have populated when a dialog opened or
        // closed during this dispatch. Same single-frame guarantee as
        // the focus-request drain above: the trap installs (or lifts)
        // before the next paint, so auto-focus + Tab confinement are in
        // place the moment the dialog appears.
        self.drain_modal_request();
    }

    /// R693 §5.39 — pop one pending [`pinion_core::modal_scope_request`]
    /// entry and apply it via [`FocusManager::push_modal_scope`] /
    /// [`FocusManager::pop_modal_scope`], routing the resulting focus
    /// move through [`Self::notify_focus_change`] so the auto-focused
    /// dialog control (and, on close, the restored invoker) fire their
    /// `External::on_focus_change` observers — the dialog's buttons mark
    /// themselves focused for the focus ring exactly as a Tab traversal
    /// would. No-op on an empty mailbox.
    fn drain_modal_request(&mut self) {
        let Some(req) = pinion_core::modal_scope_request::drain() else {
            return;
        };
        let focus_before = self.focus.focused().map(str::to_owned);
        let changed = match req {
            pinion_core::modal_scope_request::ModalRequest::Open { members } => {
                self.focus.push_modal_scope(members)
            }
            pinion_core::modal_scope_request::ModalRequest::Close => self.focus.pop_modal_scope(),
        };
        if changed {
            self.notify_focus_change(focus_before.as_deref());
            self.revision.bump();
            self.request_redraw();
        }
    }

    /// R693 §5.39 — `true` while a modal focus trap is active. The
    /// winit shell consults this to keep `Escape` from terminating the
    /// event loop while a modal is up: a trapped `Escape` dismisses the
    /// modal (routed to the widget's `apply_key`) instead of the window.
    #[must_use]
    pub fn focus_is_modal(&self) -> bool {
        self.focus.is_modal()
    }

    /// R664 §5.39 — pop one pending [`pinion_core::focus_request`]
    /// entry and apply it via [`FocusManager::focus_set`] +
    /// [`Self::notify_focus_change`]. No-op on empty mailbox (the
    /// zero-cost steady state). Bumps the §5.34 revision + requests a
    /// redraw on a real focus mutation so the next paint surfaces the
    /// focus-ring highlight and any focus-gated reactive subscriptions
    /// (e.g. an `EDIT_TF_TAG`-keyed
    /// [`CaretBlink`](pinion_core::widgets::caret_blink::CaretBlink)
    /// activates) catch up before the user types.
    fn drain_focus_request(&mut self) {
        let Some(tag) = pinion_core::focus_request::drain() else {
            return;
        };
        let focus_before = self.focus.focused().map(str::to_owned);
        if !self.focus.focus_set(&tag) {
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
            self.refresh_focusable_from_view();
            if !self.focus.focus_set(&tag) {
                // Still unknown / non-focusable — silent no-op (matches the
                // `click_to_focus` rejection arm): the requested tag is on no
                // painted focusable node, or focus is already there.
                return;
            }
        }
        self.notify_focus_change(focus_before.as_deref());
        self.revision.bump();
        self.request_redraw();
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
    fn refresh_window_focusables(&mut self, window_key: &str, tags: Vec<String>) {
        self.focusable_tags_per_window
            .insert(window_key.to_owned(), tags);
        let union = self.union_focusable_tags();
        self.focus.update_focusable_tags(union);
    }

    /// R25.1 §5.39 / R26 §5.39 §5.16 — fold the focusable enumerations of every
    /// window in the DECLARED topology into one Tab order:
    /// [`pinion_runtime::DEFAULT_WINDOW`] first (so a single-window binding's
    /// order is its window's verbatim), then the remaining windows by sorted id,
    /// de-duplicated first-occurrence (a shared-state tag rendered in two windows
    /// joins the order once, at its earliest window).
    ///
    /// R26 §5.39 §5.16 — the window set is the union of the *painted* windows
    /// ([`Self::focusable_tags_per_window`]) AND the windows the binding
    /// currently *declares* via [`WidgetView::windows_signal`]. A window the
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
            .focusable_tags_per_window
            .keys()
            .map(String::as_str)
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
        if let Some(tags) = self.focusable_tags_per_window.get(window_id) {
            return tags.clone();
        }
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

    /// (R1020 §5.39) Re-derive the keyboard focus enumeration from a fresh,
    /// side-effect-free run of [`WidgetCore::view`] over the current cached
    /// state, feeding [`FocusManager::update_focusable_tags`]. Used by
    /// [`Self::drain_focus_request`] to enumerate a node that this dispatch
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
    /// `Effect::new` a view fn creates lives inside an [`Owner::cache`](crate::reactive::Owner::cache)
    /// factory (run once per owner+key), not bare in the view body. A bare
    /// `Effect::new` would fire eagerly on this extra view run and double-
    /// execute — no current binding does that, but it is the invariant this
    /// re-derive (and the boot-seed run) depends on.
    ///
    /// R25.1 §5.39 §5.16 — runs the global `V::view` (the primary
    /// [`pinion_runtime::DEFAULT_WINDOW`] enumeration), so it refreshes the
    /// primary's per-window contribution and re-folds the union
    /// ([`Self::refresh_window_focusables`]) rather than replacing the whole
    /// `tab_order`: a programmatic focus request on the primary view must not
    /// drop a secondary window's focusable panes from the enumeration.
    fn refresh_focusable_from_view(&mut self) {
        let cached_state = *self.core.cached_state();
        let frame = Frame::with_dt(0.0);
        let scene = {
            let core = &self.core;
            core.root_owner().run(|| V::view(cached_state, &frame))
        };
        self.refresh_window_focusables(
            pinion_runtime::DEFAULT_WINDOW,
            scene.collect_focusable_tags(),
        );
    }

    /// R51.159 §5.23 — install or replace the
    /// [`CommandExecutor`](pinion_runtime::CommandExecutor) the
    /// substrate's [`Self::handle_tail`] drains pending
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
    /// `compute_access_emit` bundled the decision AND the cache
    /// update into one `&mut self` call named like a pure function.
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
    /// [`AccessTreeBuilder::add`] taking `&AccessNode` — the emit
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
    /// [`WidgetView::access_child_invoke`] before falling back to
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
/// [`rect_for_tag`](pinion_runtime::rect_for_tag) lookup. `rect_for_tag` lives
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
        let sig = sc.core.root_owner().viewport_size_signal();
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
            !sc.focusable_tags_per_window.contains_key("pane-0"),
            "boot: the secondary window has no focus contribution",
        );

        // Paint the secondary (undock) window. Pre-R25.1 the `DEFAULT_WINDOW`
        // gate dropped this enumeration entirely; now the paint records the
        // window's focusable contribution into the per-window map.
        let _ = sc.compute_paint_scene_for_window("pane-0", 64, 48);

        assert_eq!(
            sc.focusable_tags_per_window.get("pane-0"),
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
            !sc.focusable_tags_per_window.contains_key("pane-0"),
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

    /// A bare focusable Tab stop (`collect_focusable_tags` reads tag +
    /// `LayoutStyle::focusable`).
    fn focusable(tag: &'static str) -> Scene {
        Scene::Container(
            ContainerNode::new(Vec::new())
                .with_tag(tag)
                .with_layout(LayoutStyle::new().with_focusable(true)),
        )
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
            focusable(MAIN_TAG)
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
                focusable(TORN_TAG)
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
    /// pane (as `drain_focus_request` does after `toggle_pane_floating`), then
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
        let font = Font::from_bytes(NOTO.to_vec()).expect("parse NotoSans fixture");
        let engine = SelfHostedTextEngine::from_font(font);
        // The exact §5.37 box width the shell measure should size the eligible
        // label to (its own measure arm, computed before the engine is moved in).
        let (w, _h) = engine
            .measure_text(LABEL, &TextStyle::new().with_size_px(PX), &[], None, false)
            .expect("the eligible label is measurable via §5.37");
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let expected_label_w = w.ceil() as u32;

        let mut sc = ShellCore::<TextEngineFixture>::new();
        // Engine OFF (PINION_TEXT_ENGINE unset in the test process): parley measure.
        let off = sc.compute_paint_scene(800, 200);
        let off_field_w = text_width(&off, FIELD).expect("field text node present");

        // Inject the §5.37 engine — the shipping wiring `set_text_engine` exposes.
        sc.set_text_engine(Some(engine));
        let on = sc.compute_paint_scene(800, 200);
        let on_label_w = text_width(&on, LABEL).expect("label text node present");
        let on_field_w = text_width(&on, FIELD).expect("field text node present");

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
                build_text_engine_from_env().is_none(),
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
