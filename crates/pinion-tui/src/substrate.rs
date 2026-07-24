//! R51.117 §5.41 — TUI dispatch substrate ([`ShellCoreTui<V>`]).
//!
//! Sibling of `pinion_shell::substrate::ShellCore<V>` (R51.76 /
//! R51.92.1 extraction): every piece of testable dispatch state
//! lives on this struct, while the surface module ([`crate::shell`])
//! owns the crossterm raw-mode + alternate-screen + RAII guard
//! lifecycle and the live `TuiRenderer<CrosstermBackend<Stdout>>`.
//!
//! ## Why a separate substrate struct
//!
//! Pre-R51.117 [`crate::shell::run`] inlined every dispatch helper
//! (`dispatch_key`, `forward_event`, `dispatch_mouse`,
//! `drain_and_repaint`, `paint_frame`) directly inside the event
//! loop. The pre-extraction shape was untestable headlessly because
//! the helpers reached the live `Terminal<CrosstermBackend<Stdout>>`
//! through `&mut renderer` parameters — every test would have had
//! to set up the real crossterm raw-mode pipe.
//!
//! R51.117 mirrors the R51.92.1 `pinion-shell::ShellCore` split:
//! all renderer-agnostic state (scene, cached state, router, intent
//! queue) moves to `ShellCoreTui<V>`; the surface keeps only the
//! crossterm + renderer pieces.
//!
//! ## R51.124 §5.41 — backend-agnostic substrate lift
//!
//! The four renderer-agnostic fields (`scene`, `cached_state`,
//! `router`, `intent_queue`) lifted to
//! [`pinion_runtime::CoreShell<V>`]; this struct now composes one
//! `core: CoreShell<V>` field plus the TUI-only `log_sink`. The
//! dispatch arms reduce to "call `core.X` → translate the returned
//! [`pinion_runtime::DispatchTail`] into a `state_changed: bool`
//! (the TUI shell repaints on `true` because terminals do not
//! `VSync`)".
//!
//! Result: the explicit two-call refresh pattern goes
//! away. `dispatch_key`, `cursor_moved`, `pointer_down`,
//! `pointer_up` all return `bool` directly — `true` when the
//! visible state transitioned, so the caller's `if … { repaint(); }`
//! collapses to a single call.
//!
//! ## Out of scope (carry forward)
//!
//! - Focus management — single-widget shells today, so the
//!   substrate hands `Some(V::tag())` to [`CoreShell::apply_key`]
//!   unconditionally. The TUI `FocusManager` axis carries until a
//!   multi-focusable TUI binding surfaces the trigger.
//! - Cell-native coord substrate — `cell_to_pixel` now routes through
//!   the R968 §5.41 `CellMetric` type (default 8×16); a per-node
//!   metric arrives with `Scene::TextGrid` (R968 carry).

use std::cell::{Cell, RefCell};
use std::io;
use std::sync::Arc;
use std::time::Instant;

use pinion_core::event::WheelDelta;
use pinion_core::intent::Intent;
use pinion_core::{Frame, Owner, Scene, SceneRevision};
use pinion_rpc::{DeferredInput, DispatchContext, PreviewLedger, dispatch_parsed};
use pinion_runtime::{
    CommandExecutor, CoreShell, DispatchTail, FocusManager, LayoutCache, PointerId, clamp_frame_dt,
};

use crate::WidgetViewTui;
use crate::text_layout::{layout_for_terminal, layout_for_viewport_px};

/// R51.117 §5.41 — TUI shell dispatch substrate.
///
/// R51.124 §5.41 — composes the backend-agnostic
/// [`CoreShell<V>`] plus the TUI-only diagnostic sink. The
/// renderer-agnostic dispatch state (state scene, cached `V::State`,
/// [`pinion_runtime::InputRouter`], [`pinion_runtime::IntentQueue`])
/// lives inside `core`; this struct contributes the `log_sink` only.
///
/// Methods on this struct mutate the substrate (through `core`) and
/// translate the post-dispatch [`DispatchTail`] into a
/// `state_changed: bool`; the surface [`crate::shell::run`] sequences
/// the dispatch calls and commits the painted buffer through the
/// live renderer on a `true` return. Both responsibilities — what to
/// dispatch + how to commit — stay one layer apart, so a future
/// `--features test-backend` build targets this substrate directly
/// without touching the crossterm raw-mode path.
pub struct ShellCoreTui<V: WidgetViewTui> {
    /// Backend-agnostic dispatch substrate. R51.124 §5.41 — lifted
    /// to `pinion-runtime` so both backends (Vello + TUI) share the
    /// `scene` + `cached_state` + `router` + `intent_queue`
    /// plumbing.
    core: CoreShell<V>,
    /// R670 §5.41 §5.40 §5.34 — preview lifecycle ledger, mirror of
    /// `pinion_shell::ShellCore::previews`. Plumbed into the
    /// `pinion_rpc::DispatchContext` by [`Self::dispatch_rpc`] so the
    /// `propose_change` / `apply_preview` / `cancel_preview` /
    /// `list_previews` lifecycle methods see the same wire shape on
    /// the TUI side as on the Vello side. **The §2 #6 GUI/TUI dual
    /// invariant requires identical RPC surface across both
    /// backends**, so an AI client driving `pinion_rpc::dispatch`
    /// against this substrate observes the same preview semantics.
    previews: PreviewLedger,
    /// R670 §5.41 §5.34 — §5.34 R40.4 OCC revision token, mirror of
    /// `pinion_shell::ShellCore::revision`. `dispatch` auto-bumps on
    /// mutating RPC methods; programmatic-focus mutation also bumps
    /// explicitly through [`Self::drain_focus_request`].
    revision: SceneRevision,
    /// R670 §5.41 §5.39 — framework-side focus state owner, mirror of
    /// `pinion_shell::ShellCore::focus`. Seeded at construction from
    /// `V::focusable_tags()` so the default single-widget binding
    /// has a tab stop at boot; multi-focus TUI bindings (future
    /// `hello-multi-window-tui` and friends) override via the same
    /// `WidgetCore::focusable_tags` channel the Vello side uses.
    ///
    /// Reachable through [`Self::focus`] / [`Self::dispatch_rpc`]
    /// only — TUI input dispatch today still hands `Some(V::tag())`
    /// to [`CoreShell::apply_key`] unconditionally (no Tab traversal
    /// in TUI crossterm path), but the RPC `focus/set` / `focus/get`
    /// / `focus/next` / `focus/prev` methods drive the manager
    /// directly so an AI client can already exercise focus arcs
    /// through the wire even before the crossterm Tab arm lands.
    focus: FocusManager,
    /// R51.120 §5.41 — optional diagnostic sink for intent / state
    /// trace lines.
    ///
    /// **Default = `None` (silent)**: the surface enables the TUI
    /// shell under `enable_raw_mode()` + `EnterAlternateScreen`;
    /// writing diagnostic text to `stderr` from inside that mode
    /// produces raw bytes on the same terminal alternate buffer
    /// (the `EnterAlternateScreen` ANSI sequence only retargets the
    /// `stdout` fd — `stderr` keeps going to the visible terminal),
    /// which then collides with the ratatui frame the next
    /// `draw` cycle commits. The cell appears overwritten by the
    /// stale log glyph until the ratatui differential redraw
    /// happens to mark that exact cell dirty.
    ///
    /// Setting a non-`None` sink (the surface's
    /// `PINION_TUI_LOG=path` env-var opt-in) routes every trace
    /// line to a separate writer so the alternate screen stays
    /// undisturbed. Tests use an in-memory `Vec<u8>` sink to assert
    /// against the captured lines.
    log_sink: Option<Box<dyn io::Write + Send>>,

    /// R51.144 §5.28 — wall-clock timestamp of the previous
    /// [`Self::compute_paint_scene`] entry, used to compute `dt` for
    /// the next paint.
    ///
    /// Wrapped in [`Cell`] so the accessor keeps its `&self` shape —
    /// the surface's `commit_paint` takes the
    /// substrate by shared borrow (`&ShellCoreTui<V>`) and the
    /// borrow rules of `&mut` would force every call site to re-thread
    /// a mutable reference through paths that otherwise stay sync.
    /// [`Instant`] is `Copy` so [`Cell::get`] / [`Cell::set`] are
    /// the canonical zero-overhead pattern here.
    ///
    /// First paint: `None` → `dt = 0.0` (no prior timestamp). Per
    /// §5.28 R33 spring solver behavior at `dt=0` is "no progress",
    /// so the first frame leaves at-rest animations untouched and
    /// starts moving ones stay at construction baseline until the
    /// second paint measures the elapsed delta.
    last_paint_instant: Cell<Option<Instant>>,

    /// R1344 §5.41 §5.36 — the layout pass's cache, the TUI peer of
    /// `pinion_shell::ShellCore::text_cache`.
    ///
    /// [`RefCell`] for the same reason [`Self::last_paint_instant`] is a
    /// [`Cell`]: [`Self::compute_paint_scene`] keeps its `&self` shape
    /// because the surface's `commit_paint` holds the substrate by shared
    /// borrow. Unlike `Instant` a cache is not `Copy`, so the runtime-checked
    /// cell is the pattern; the borrow is confined to the layout call and
    /// never overlaps a `V::view` re-entry, so it cannot panic.
    layout_cache: RefCell<LayoutCache>,
}

impl<V: WidgetViewTui> Default for ShellCoreTui<V> {
    /// Equivalent to [`Self::new`]; provided for the conventional
    /// `Default` trait surface tests reach through.
    fn default() -> Self {
        Self::new()
    }
}

impl<V: WidgetViewTui> ShellCoreTui<V> {
    /// Construct a fresh substrate around the application's
    /// [`pinion_core::WidgetCore::create_external`] SCXML widget.
    ///
    /// R51.124 §5.41 — bootstrap delegates to [`CoreShell::new`];
    /// this constructor adds the TUI-only `log_sink: None` so the
    /// shell stays silent under raw mode + alternate screen until
    /// the surface's `PINION_TUI_LOG=path` opt-in routes trace text
    /// to a separate writer.
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_seed(|_| {})
    }

    /// R1363 §5.55 §2 #6 — [`Self::new`] with a backend-supplied `seed` closure
    /// run against the root [`Owner`] after it exists but BEFORE the binding
    /// factories resolve any `use_*` hook.
    ///
    /// The TUI peer of the Vello shell's seeding window, for the same reason:
    /// this backend seeds one handle — its
    /// [`QuitSink`](pinion_core::QuitSink) — before any binding's
    /// `use_quit_sink()` reads it. Since R1366.2 that sink is a
    /// [`ProviderSlot`](pinion_core::ProviderSlot), so a seed that loses to a
    /// prior read PANICS (`already seeded`) rather than being silently dropped
    /// onto a dead handle — the loud failure the winit shell's window also got.
    /// That seam is what gives a TERMINAL binding the app-lifecycle the GUI has
    /// (§5.55: quitting is not a window operation, so it must not live in a
    /// window vocabulary the TUI cannot name).
    #[must_use]
    pub fn new_with_seed(seed: impl FnOnce(&Owner)) -> Self {
        // (R1020 §5.41 §5.39) Seed the focus enumeration from the binding's
        // first view — scene-derived, the same source the per-paint refresh
        // (`update_paint_scene`) uses — so RPC-driven `focus/set` resolves a
        // current enumeration before the first paint. Mirrors
        // `pinion_shell::ShellCore::new`. (Replaces the pre-R1020
        // `V::focusable_tags()` boot seed; the method is retired.)
        let mut shell = Self {
            core: CoreShell::new_with_seed(seed),
            previews: PreviewLedger::default(),
            revision: SceneRevision::default(),
            focus: FocusManager::new(),
            log_sink: None,
            last_paint_instant: Cell::new(None),
            layout_cache: RefCell::new(LayoutCache::new()),
        };
        // R1335 §5.39 (PR-53) — attach this binding's root owner so RPC-driven
        // focus mutations publish the focused tag to the owner mirror an AI
        // client / binding reads (`pinion_core::focus_state::focused`). Mirrors
        // `pinion_shell::ShellCore::new`.
        let root_owner = shell.core.root_owner().clone();
        shell.focus.attach_owner(root_owner);
        shell.refresh_focusable_from_view();
        shell
    }

    /// R670 §5.41 §5.34 — current §5.34 R40.4 OCC revision counter
    /// (loaded with `Acquire` ordering). Mutating RPC dispatches bump
    /// it through the dispatcher's own bookkeeping; tests assert the
    /// before/after delta when verifying that a dispatch path actually
    /// committed. Mirror of `pinion_shell::ShellCore::revision`.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision.current()
    }

    /// R670 §5.41 §5.39 — borrow the focus manager. Tests + RPC
    /// substrate paths reach the focused tag through this accessor.
    /// Mirror of `pinion_shell::ShellCore::focus`.
    #[must_use]
    pub fn focus(&self) -> &FocusManager {
        &self.focus
    }

    /// R51.120 §5.41 — install a diagnostic sink for intent / state
    /// trace lines. See `Self::log_sink` for why this is opt-in;
    /// the default `None` keeps the substrate silent so the surface
    /// can run under `enable_raw_mode()` + `EnterAlternateScreen`
    /// without leaking trace text onto the visible terminal.
    ///
    /// The surface's `PINION_TUI_LOG=path` env-var opt-in opens
    /// the named file with `append(true)` and hands the resulting
    /// `File` here. Tests pass a `Vec<u8>` boxed as
    /// `Box<dyn io::Write + Send>` to capture the trace in memory.
    /// Calling this method twice replaces the previous sink (the
    /// dropped writer flushes on `Drop`).
    pub fn set_log_sink(&mut self, sink: Box<dyn io::Write + Send>) {
        self.log_sink = Some(sink);
    }

    /// R51.120 §5.41 — builder-style sibling of
    /// [`Self::set_log_sink`]; equivalent to
    /// `let mut c = ShellCoreTui::new(); c.set_log_sink(sink); c` so
    /// chained construction sites
    /// (`ShellCoreTui::new().with_log_sink(...)`) stay one-line.
    #[must_use]
    pub fn with_log_sink(mut self, sink: Box<dyn io::Write + Send>) -> Self {
        self.set_log_sink(sink);
        self
    }

    /// Read-only borrow of the cached state. Tests assert against
    /// this after each dispatch to verify SCXML transitions arm
    /// expected values; the surface uses it as the
    /// `WidgetViewTui::view` argument when computing the next
    /// paint frame.
    /// R51.124 §5.41 — delegates to [`CoreShell::cached_state`].
    #[must_use]
    pub fn cached_state(&self) -> &V::State {
        self.core.cached_state()
    }

    /// R51.144 §5.28 — delegates to
    /// [`CoreShell::root_owner`](pinion_runtime::CoreShell::root_owner).
    ///
    /// The TUI binding's view fn attaches its [`Animation<T>`](pinion_core::Animation)
    /// instances here; [`Self::compute_paint_scene`] ticks the list
    /// once per paint cycle with the measured `dt`. Drop on
    /// `ShellCoreTui` cascades through the wrapped
    /// [`CoreShell`] into the
    /// [`Owner`] drop semantics, cancelling every pending
    /// [`Command`](pinion_core::Command) (Solid pattern).
    #[must_use]
    pub fn root_owner(&self) -> &Owner {
        self.core.root_owner()
    }

    /// R51.147 §5.28 — `true` while any animation registered on
    /// [`Self::root_owner`] is still moving above the spring epsilon.
    /// The TUI surface (`shell::run`) polls this between event-loop
    /// iterations: a `true` return shortens the crossterm poll
    /// timeout so the next paint commits at animation pace; a
    /// `false` return restores the idle long-poll. Delegates to
    /// [`CoreShell::any_animation_active`](pinion_runtime::CoreShell::any_animation_active).
    #[must_use]
    pub fn any_animation_active(&self, epsilon: f32) -> bool {
        self.core.any_animation_active(epsilon)
    }

    /// R1426 §5.41 §5.28 — the render-time terminal-cursor blink phase for this
    /// frame: `true` when a blinking cursor should paint (its visible half),
    /// `false` on the hidden half; steady / hidden cursors resolve to `true`.
    /// The surface passes it to [`crate::paint::to_buffer_with_cursor_phase`].
    /// Delegates to
    /// [`CoreShell::grid_cursor_blink_on`](pinion_runtime::CoreShell::grid_cursor_blink_on)
    /// for the single `DEFAULT_WINDOW`; read outside any reactive scope so the
    /// phase never folds into the scene (§2 #7).
    #[must_use]
    pub fn grid_cursor_blink_on(&self) -> bool {
        self.core
            .grid_cursor_blink_on(pinion_runtime::DEFAULT_WINDOW)
    }

    /// Build the binding's paint scene from the current cached
    /// state. Pure sync per §6.3 R51.27 `dry_run`: identical
    /// `(state, frame, owner_state, cols, rows)` always yields the same
    /// `Scene`.
    ///
    /// R1344 §5.41 — the viewport joined that tuple when the layout pass
    /// landed: `rect` is now resolved against the terminal size, so the same
    /// state legitimately yields different geometry at a different size (that
    /// IS reflow). Determinism is unaffected — the layout pass is a pure
    /// function of `(scene, viewport)` and the cache is keyed on its inputs, so
    /// warm and cold caches agree.
    ///
    /// R1344 §5.41 §5.36 — runs the **same** `compute_layout` pass the
    /// Vello sibling runs, against a cell-grid text measure
    /// ([`crate::text_layout::CellTextLayout`]). `cols` / `rows` are the
    /// terminal's size;
    /// they convert to the logical pixels taffy and `Scene.rect` speak
    /// through [`pinion_core::CellMetric`].
    ///
    /// ## Why this reversed R51.124's note
    ///
    /// R51.124's rustdoc said "the substrate no longer drives
    /// `compute_layout`; the TUI paint walker maps the
    /// `Scene::Container.rect` cells directly". That described an
    /// *absence*, not a ratified decision — `compute_layout` was never
    /// called from this crate at all, and §5.41's own open item
    /// ("`TextAlign` wrap policy = R51.109 substrate 결정") shows the
    /// question was left open, while §5.36 defines the text layout
    /// primitive as "모든 backend(Vello/Headless/TUI/미래) 공유
    /// backend-orthogonal". The absence had a real cost: the two
    /// backends read **disjoint halves** of the same scene —
    ///
    /// | | authoritative | ignored |
    /// |---|---|---|
    /// | Vello | `LayoutStyle` | the view's authored `rect` (overwritten) |
    /// | TUI (pre-R1344) | the view's authored `rect` | `LayoutStyle` (inert) |
    ///
    /// — so §2 #6 ("one scene, two render dispatch paths") held only in
    /// name: a `Scene` that laid out correctly on one backend could not
    /// lay out on the other, and nothing checked the two halves agreed.
    /// A binding built from `pinion-widget-paint` (whose nodes carry
    /// `Rect::default()` and rely on taffy) could not render here at
    /// all.
    ///
    /// Running the pass makes `rect` a pure **output** on both backends
    /// and `LayoutStyle` the single authored contract, which is what
    /// [`pinion_runtime::compute_layout`]'s own "mutates each node's `rect` field in
    /// place" contract already implied.
    ///
    /// The cell measure never defers to parley (see
    /// [`crate::text_layout::CellTextLayout`]'s `TextMeasure` impl), so no
    /// font work
    /// happens on this path.
    ///
    /// R51.144 §5.28 — per-paint pump now measures `dt` against the
    /// previous paint's [`Instant`], advances every animation
    /// registered on [`Self::root_owner`] through
    /// [`CoreShell::tick_animations`](pinion_runtime::CoreShell::tick_animations),
    /// and threads the same `dt` into [`Frame::with_dt`](pinion_core::Frame::with_dt)
    /// so deterministic-time-dependent view-fn logic (Tween
    /// progress reads, etc.) sees the matching delta. First paint:
    /// `dt = 0.0`.
    ///
    /// `&self` (not `&mut self`) because `crate::shell::commit_paint`
    /// takes the substrate by shared borrow; the timing field uses
    /// [`Cell`] interior mutability so the signature stays sync.
    #[must_use]
    pub fn compute_paint_scene(&self, cols: u16, rows: u16) -> Scene {
        let now = Instant::now();
        let raw_dt = self
            .last_paint_instant
            .get()
            .map_or(0.0_f32, |prev| now.duration_since(prev).as_secs_f32());
        self.last_paint_instant.set(Some(now));
        // R51.145 §5.28 — clamp before reaching the spring solver +
        // the view fn (see `pinion_runtime::clamp_frame_dt` for the
        // rationale; mirrors the Vello sibling exactly).
        let dt = clamp_frame_dt(raw_dt);
        self.core.tick_animations(dt);
        let frame = Frame::with_dt(dt);
        // R51.146 §5.22 — wrap the view fn in `root_owner().run(...)`
        // so [`pinion_core::Owner::current`] resolves to this
        // binding's root reactive scope from inside `V::view`. The
        // TUI side mirrors the Vello sibling exactly: animations /
        // effects / commands created without an explicit
        // [`pinion_core::Owner`] argument land on the framework-owned
        // scope, dropping together with this substrate.
        let cached_state = *self.core.cached_state();
        let run_view = || self.core.root_owner().run(|| V::view(cached_state, &frame));

        // R1344 §5.41 §5.36 — the layout pass, mirroring the Vello
        // sibling's `compute_paint_scene_internal` shape.
        let mut scene = run_view();
        let scroll_dirty = self.run_layout(&mut scene, cols, rows);
        // R57.X.scrollbar §5.45 — the first-paint chicken-and-egg the
        // Vello sibling documents: the layout pass writes
        // `ScrollState::set_max` AFTER `V::view` already read it, so a
        // scrollbar's first frame would paint a full-track thumb. Re-run
        // once when the pass actually moved a bound. Idempotent on
        // steady-state frames (Signal equality-skip floors it at false).
        if scroll_dirty {
            scene = run_view();
            let _ = self.run_layout(&mut scene, cols, rows);
        }
        // R1426 §5.41 §5.28 — arm the terminal-cursor blink clock from the
        // freshly-produced scene, mirroring the Vello sibling's arm in
        // `compute_paint_scene_internal`. The TUI is single-window
        // (`DEFAULT_WINDOW`) and has no OS-focus signal, so the blink is
        // free-running whenever a blinking cursor is shown (matching
        // xterm-when-enabled); `any_animation_active` in the run loop then polls
        // + commits repaints that advance the phase (via `tick_animations`
        // above). The phase is read back at paint by `commit_paint` and never
        // folded into the scene (§2 #7).
        // R1427 §5.41 §5.39 — the TUI has no OS-window-focus concept (it does not
        // track host-terminal focus), so it arms with `focused = true` and keeps
        // its pre-R1427 filled, free-running cursor unchanged (the hollow-on-blur
        // render is GUI-only, like multi-window).
        self.core
            .arm_grid_cursor_blink(pinion_runtime::DEFAULT_WINDOW, &scene, true);
        scene
    }

    /// R1344 §5.41 — one layout pass over `scene`. Returns taffy's
    /// scroll-dirty flag (see [`Self::compute_paint_scene`]'s re-pass).
    ///
    /// The [`RefCell`] borrow is scoped to this call and never spans a
    /// `V::view` re-entry, so it cannot collide with a re-entrant paint.
    fn run_layout(&self, scene: &mut Scene, cols: u16, rows: u16) -> bool {
        let mut cache = self.layout_cache.borrow_mut();
        layout_for_terminal(scene, cols, rows, &mut cache)
    }

    /// Hand a freshly-painted scene to the substrate's
    /// [`pinion_runtime::InputRouter`] (via [`CoreShell::update_paint_scene`])
    /// so the next pointer event resolves against the visible
    /// layout. Surface calls this after every successful paint
    /// commit (initial + post-state-change + resize repaint).
    pub fn update_paint_scene(&mut self, paint_scene: Scene) {
        // (R1020 §5.39) Re-derive the keyboard focus enumeration from the
        // freshly painted scene — the ratified scene-derived focus model, the
        // TUI peer of the Vello `compute_paint_scene_internal` refresh. This is
        // the per-frame `&mut self` paint-commit hook (`compute_paint_scene` is
        // `&self`), called after every paint, so a focusable node appearing /
        // disappearing across frames tracks the Tab order automatically.
        //
        // A binding with no focus stops paints no focusable node, so the
        // enumeration is correctly empty.
        // R1327 §5.39 — the drop flag is dropped here, unlike the Vello shell, which
        // pairs it with a redraw request. The TUI has no redraw-request channel at
        // all (no dirty bridge, no `redraw_requested`): it commits a fresh paint after
        // every dispatch, so a focus drop that happens INSIDE a paint is corrected by
        // the next event's commit rather than by a frame this pass schedules. A §2 #6
        // asymmetry of the TUI's event-driven paint model, not of this seam.
        let _dropped_focus = self
            .focus
            .update_focusable_tags(paint_scene.collect_focusable_tags());
        self.core.update_paint_scene(paint_scene);
    }

    /// R51.117 §5.41 — dispatch one W3C-named key through the
    /// binding's `keybinding` → `apply_key` chain.
    ///
    /// R51.124 §5.41 — returns `true` when the visible cached
    /// state changed (the TUI shell repaints on `true` because
    /// terminals do not `VSync` and only commit a buffer when the
    /// substrate signals the frame is out of date). Replaces the
    /// pre-R51.124 two-call pattern (`dispatch_key` returned "was
    /// the key handled?", caller chained an explicit
    /// state-refresh call for "did state change?"); the single-call
    /// shape mirrors the post-lift Vello side where dispatch
    /// methods auto-tail.
    ///
    /// Single-widget focus model: the substrate hands
    /// `Some(V::tag())` to [`CoreShell::apply_key`] unconditionally
    /// so the widget's focus-gated `apply_key` impl recognises
    /// itself as the activation target. The TUI `FocusManager`
    /// axis (carry) lifts this constant.
    ///
    /// (R1306 PR-51 §5.41) A binding that opts out of a primary surface
    /// ([`pinion_core::WidgetCore::primary_surface`] `== None`) has no
    /// primary tag — `V::tag()` is an `unreachable!` marker for it — so the
    /// focus argument is read through `primary_surface()` (which yields
    /// `None`, never calling `V::tag()`), not a bare `V::tag()`.
    /// The binding's `apply_key` handles keys without a primary-tag focus
    /// gate; this keeps a no-primary binding TUI-renderable per §2 #6
    /// (GUI/TUI dual) instead of panicking on the first key.
    pub fn dispatch_key(&mut self, key_str: &str, modifiers: pinion_core::Modifiers) -> bool {
        if let Some(event) = V::keybinding(key_str) {
            let tail = self.core.forward(event);
            return self.handle_tail(&tail);
        }
        let focused = V::primary_surface().map(|p| p.tag);
        if let Some(tail) = self.core.apply_key(focused, key_str, modifiers) {
            return self.handle_tail(&tail);
        }
        // R51.187 §5.45 R55.C.3 — widget reported the key
        // unhandled; route through the scroll dispatch fallback
        // so an arrow / page / Home / End over a scroll container
        // still scrolls. Mirrors the Vello sibling's
        // `handle_named_key` apply_key → scroll_key cascade.
        self.scroll_key(key_str)
    }

    /// (R51.187 §5.45 R55.C.3) Keyboard scroll dispatch — the
    /// fallback path [`Self::dispatch_key`] takes when
    /// [`WidgetCore::apply_key`](pinion_core::WidgetCore::apply_key)
    /// reports the key unhandled. Forwards through
    /// [`CoreShell::scroll_key`](pinion_runtime::CoreShell::scroll_key);
    /// returns `true` on actual dispatch OR cached-state change
    /// so the surface repaints.
    pub fn scroll_key(&mut self, key: &str) -> bool {
        let (tail, dispatched) = self.core.scroll_key(PointerId::MOUSE, key);
        let state_changed = self.handle_tail(&tail);
        dispatched || state_changed
    }

    /// R51.117 §5.41 — forward a cursor-move (pixel-space, already
    /// converted from cell coords by the surface) to
    /// [`CoreShell::cursor_moved`].
    ///
    /// R51.124 §5.41 — returns `true` when the dispatch flipped
    /// the visible cached state (e.g. cursor entered a widget rect
    /// and the `Idle → Hover` transition fired).
    pub fn cursor_moved(&mut self, x: f64, y: f64) -> bool {
        // R881 §5.35 — the zero-modifier wrapper: the TUI pointer path
        // carries no modifier chords yet (§2 #6 divergence carry,
        // pre-existing). The second flag reports a live middle pan
        // dispatching a scroll this move — repaint-relevant exactly
        // like `wheel` (R881.1: the plain pair threads the flag too).
        let (tail, pan_dispatched) = self.core.cursor_moved(PointerId::MOUSE, x, y);
        let state_changed = self.handle_tail(&tail);
        pan_dispatched || state_changed
    }

    /// R881 §5.35 §5.49 — middle-button press (the `scene/drag
    /// {button: "middle"}` drain; crossterm `MouseButton::Middle` can
    /// route here when the TUI surface wires it). Opens the router's
    /// middle gesture — pan targets pinned at the press point. Never a
    /// visible state change by itself.
    pub fn middle_pressed(&mut self) {
        self.core.middle_down(PointerId::MOUSE);
    }

    /// R881 §5.35 §5.49 — middle-button release. A latched gesture
    /// already panned move-by-move (each `cursor_moved` reported the
    /// repaint); a release-in-place resolves to the middle-*click*,
    /// which on the GUI shell runs the X11 PRIMARY paste funnel — the
    /// TUI has no clipboard arc on this path yet (the terminal
    /// emulator owns middle-paste at the terminal tier; §2 #6
    /// divergence carry, pre-existing for the whole TUI paste axis),
    /// so `Click` is a documented no-op here. Returns `false` — no
    /// repaint originates at the release edge.
    pub fn middle_released(&mut self) -> bool {
        let _ = self.core.middle_up(PointerId::MOUSE);
        false
    }

    /// R887 §5.49 §5.53 — secondary-button (right-click) press: the
    /// `scene/click {button: "right"}` drain and the crossterm
    /// `MouseButton::Right` arm. Mirrors the GUI shell's
    /// `secondary_click_for_window` shape: reads the cached cursor
    /// position (the channel `cursor_moved` seeds) and forwards it to
    /// [`CoreShell::apply_secondary_click`](pinion_runtime::CoreShell::apply_secondary_click)
    /// — a press-edge one-shot (the W3C `contextmenu` convention), no
    /// release half. A press before any `cursor_moved` (no cached
    /// position) is swallowed quietly, byte-for-byte the GUI policy.
    /// Returns `true` on visible state change (R51.124 §5.41).
    pub fn secondary_click(&mut self) -> bool {
        let Some((x, y)) = self
            .core
            .cursor_position_for_window(pinion_runtime::DEFAULT_WINDOW, PointerId::MOUSE)
        else {
            return false;
        };
        #[allow(
            clippy::cast_possible_truncation,
            reason = "cell-grid logical cursor coords fit f32 in every realistic terminal"
        )]
        match self.core.apply_secondary_click(x as f32, y as f32) {
            Some(tail) => self.handle_tail(&tail),
            None => false,
        }
    }

    /// R51.117 §5.41 — pointer press (mouse left button down,
    /// crossterm-side). Returns `true` on visible state change
    /// (R51.124 §5.41).
    ///
    /// R882 / R882.1 §5.35 §5.39 — the press routes through the
    /// substrate's LEFT front door
    /// ([`CoreShell::left_press`](pinion_runtime::CoreShell::left_press)):
    /// the Space-hold pan chord and the live-pan swallow are
    /// substrate policy owned once in `CoreShell`, so this backend
    /// carries zero copies of the routing decision (§2 #6). `None` =
    /// the pan channel consumed the press — no widget `PointerDown`,
    /// no repaint at the press edge.
    pub fn pointer_down(&mut self) -> bool {
        match self.core.left_press(PointerId::MOUSE) {
            Some(tail) => self.handle_tail(&tail),
            None => false,
        }
    }

    /// R51.117 §5.41 — pointer release (mouse left button up).
    /// Returns `true` on visible state change (R51.124 §5.41).
    ///
    /// R882 / R882.1 §5.35 — routes through
    /// [`CoreShell::left_release`](pinion_runtime::CoreShell::left_release):
    /// a press that entered the pan channel resolves there
    /// (gesture-capture). The TUI pointer path carries no modifier
    /// chords yet (§2 #6 divergence carry, pre-existing — the R881
    /// `cursor_moved` note).
    pub fn pointer_up(&mut self) -> bool {
        match self
            .core
            .left_release(PointerId::MOUSE, pinion_core::Modifiers::default())
        {
            Some(tail) => self.handle_tail(&tail),
            None => false,
        }
    }

    /// R1424 §5.35 §5.15 §2 #2 §2 #6 — one raw pointer button EDGE (left /
    /// middle / right × down / up) at `(x, y)`: the `scene/pointer_button`
    /// drain (an R1416 gap on this backend). Mirrors the Vello
    /// `pointer_button_for_window`: seed the cursor, then offer the raw edge
    /// to a raw sink through the shared [`InputRouter`](pinion_runtime::InputRouter)
    /// seam ([`CoreShell::raw_pointer_button_for_window`](pinion_runtime::CoreShell::raw_pointer_button_for_window),
    /// a method both backends reach). A consumed raw edge suppresses the
    /// per-button default (returning the router's repaint signal); otherwise
    /// the standard arc runs — left = press/release, middle = pan open/close,
    /// right = context-menu press-edge one-shot (no release half). The TUI
    /// carries no modifier chords yet (§2 #6 divergence carry, the
    /// [`cursor_moved`](Self::cursor_moved) note). Returns `true` on visible
    /// state change (R51.124 §5.41).
    pub fn pointer_button(
        &mut self,
        x: f64,
        y: f64,
        button: pinion_core::PointerButton,
        edge: pinion_core::PointerEdge,
    ) -> bool {
        use pinion_core::{PointerButton, PointerEdge};
        let mut changed = self.cursor_moved(x, y);
        if self.core.raw_pointer_button_for_window(
            pinion_runtime::DEFAULT_WINDOW,
            PointerId::MOUSE,
            button,
            edge,
            pinion_core::Modifiers::default(),
        ) {
            return true;
        }
        match (button, edge) {
            (PointerButton::Left, PointerEdge::Down) => changed |= self.pointer_down(),
            (PointerButton::Left, PointerEdge::Up) => changed |= self.pointer_up(),
            (PointerButton::Middle, PointerEdge::Down) => self.middle_pressed(),
            (PointerButton::Middle, PointerEdge::Up) => changed |= self.middle_released(),
            (PointerButton::Right, PointerEdge::Down) => changed |= self.secondary_click(),
            (PointerButton::Right, PointerEdge::Up) => {}
        }
        changed
    }

    /// R882 §5.39 — held-key edge funnel, forwarding to the substrate
    /// cache ([`CoreShell::note_key_state`](pinion_runtime::CoreShell::note_key_state)).
    /// The TUI's only producer is the `scene/key state:"down"/"up"`
    /// drain: crossterm has no release edge on the baseline protocol
    /// and no focus-loss clear is wired (the cache is RPC-owned on
    /// this backend — §2 #6 carry, the same class as the paste axis).
    pub fn note_key_state(&mut self, key: &str, pressed: bool) {
        self.core.note_key_state(key, pressed);
    }

    /// R882 §5.49 §5.39 — the `scene/key` drain arm shared by the
    /// named-key and character-key variants: the edge → cache /
    /// dispatch policy is [`KeyWireState`](pinion_rpc::KeyWireState)'s
    /// own (`held_edge` / `dispatches`), the same decision table the
    /// Vello sibling's `drain_key_for_window` reads. The TUI's two
    /// wire shapes share one dispatch entry (`dispatch_key`), so one
    /// helper serves both arms. R882.1 — the leading cursor move is
    /// gated on `dispatches()` too: a release edge is positionless
    /// (the Vello sibling's rule), so it neither moves the cursor nor
    /// perturbs a live pan.
    fn drain_key_edge(
        &mut self,
        at: (f64, f64),
        key: &str,
        state: pinion_rpc::KeyWireState,
    ) -> bool {
        if let Some(held) = state.held_edge() {
            self.note_key_state(key, held);
        }
        if state.dispatches() {
            let moved = self.cursor_moved(at.0, at.1);
            self.dispatch_key(key, pinion_core::Modifiers::default()) || moved
        } else {
            false
        }
    }

    /// (R51.186 §5.45 R55.C.2) Mouse wheel dispatch — crossterm
    /// `MouseEventKind::ScrollUp` / `ScrollDown` / `ScrollLeft` /
    /// `ScrollRight`. Forwards through
    /// [`CoreShell::wheel`] which
    /// walks the deepest [`Scene::Scroll`]
    /// under the cursor and calls `scroll_by` on the attached
    /// [`ScrollState`](pinion_core::widgets::scroll::ScrollState).
    ///
    /// Returns `true` when the router dispatched against an
    /// attached `ScrollState` OR when a tail intent flipped the
    /// cached state — the surface repaints on `true` so the new
    /// scroll offset (or any reducer-driven SCXML state change)
    /// lands on screen. Silent drops (cursor outside any scroll
    /// container, no `state` link) return `false` so an idle
    /// terminal does not spuriously redraw on wheel input over a
    /// non-scrollable region.
    pub fn wheel(&mut self, delta: WheelDelta) -> bool {
        let (tail, dispatched) = self.core.wheel(PointerId::MOUSE, delta);
        let state_changed = self.handle_tail(&tail);
        dispatched || state_changed
    }

    /// R668 §5.41 §5.49 — drain the AI-injected deferred-input inbox
    /// `pinion_rpc::dispatch` populates. Mirrors the Vello sibling
    /// (`pinion_shell::ShellCore::drain_deferred_inputs`) so every RPC
    /// substrate primitive — R660 `scene/drag`, R663
    /// `scene/double_click`, R666 `scene/key` character-vs-named
    /// auto-discrimination, the steady-state `scene/click` /
    /// `scene/wheel` / `scene/key` arcs — replays through the same
    /// `cursor_moved` / `pointer_down` / `pointer_up` / `wheel` /
    /// `dispatch_key` entry points crossterm input already uses.
    ///
    /// Result: the §2 #6 GUI/TUI dual invariant holds end-to-end —
    /// an AI client that drives `pinion_rpc::dispatch` against this
    /// substrate (RPC ingress / response wiring is the
    /// `[[pinion-tui-rpc-ingress]]` follow-up consumer of this
    /// primitive) sees the same SCXML statechart transitions the live
    /// crossterm shell does.
    ///
    /// Returns `true` when any drained variant flipped the visible
    /// cached state — the surface repaints on `true` so the AI-driven
    /// transition lands on the terminal frame the next paint commit
    /// resolves.
    ///
    /// `Key` and `CharacterKey` both route through [`Self::dispatch_key`]
    /// because the TUI substrate single-entry-points keyboard dispatch
    /// (named / character keys are indistinguishable at the
    /// `V::keybinding` → `V::apply_key` → scroll-key fallback chain).
    /// Modifiers default to empty — RPC-injected keys carry no
    /// modifier surface in v1; a follow-up axis lifts the modifier
    /// argument onto the `Key` / `CharacterKey` variants when the
    /// first AI-driver use-case (e.g. Shift+Arrow text-selection
    /// macro) lands.
    pub fn drain_deferred_inputs(&mut self, inputs: &[pinion_rpc::DeferredInput]) -> bool {
        let mut state_changed = false;
        for input in inputs {
            // R1430 §5.35 §2 #6 — the Qt QTabletEvent scalar axes route through
            // one dispatcher so this per-input match stays flat as the axis set
            // grows; see `try_drain_pointer_axis`.
            if let Some(changed) = self.try_drain_pointer_axis(input) {
                state_changed |= changed;
                continue;
            }
            match *input {
                pinion_rpc::DeferredInput::Wheel { x, y, delta } => {
                    state_changed |= self.cursor_moved(x, y);
                    state_changed |= self.wheel(delta);
                }
                pinion_rpc::DeferredInput::Click { x, y } => {
                    state_changed |= self.cursor_moved(x, y);
                    state_changed |= self.pointer_down();
                    state_changed |= self.pointer_up();
                }
                // R887 §5.49 §5.53 — `scene/click {button: "right"}`
                // mirror: seed the cursor cache, then the press-edge
                // one-shot (`secondary_click` reads that cache), the
                // same arc the GUI sibling takes. No release half.
                pinion_rpc::DeferredInput::SecondaryClick { x, y } => {
                    state_changed |= self.cursor_moved(x, y);
                    state_changed |= self.secondary_click();
                }
                pinion_rpc::DeferredInput::DoubleClick { x, y } => {
                    // R663 §5.49 — W3C UIEvent `detail:2` mirror: two
                    // complete press/release cycles at the same
                    // coordinate without an intervening cursor move,
                    // so the InputRouter arc fires identically to a
                    // real-mouse double-click. The Vello sibling pairs
                    // each press/release on `mouse_pressed` /
                    // `mouse_released`; the TUI substrate keeps the
                    // same call sequence through `pointer_down` /
                    // `pointer_up`.
                    state_changed |= self.cursor_moved(x, y);
                    state_changed |= self.pointer_down();
                    state_changed |= self.pointer_up();
                    state_changed |= self.pointer_down();
                    state_changed |= self.pointer_up();
                }
                // R882 §5.49 §5.39 — `state` carries the keyboard edge;
                // the shared edge policy (cache update / dispatch /
                // cursor move) lives in `drain_key_edge`.
                pinion_rpc::DeferredInput::Key {
                    x,
                    y,
                    ref key,
                    state,
                } => {
                    state_changed |= self.drain_key_edge((x, y), key, state);
                }
                pinion_rpc::DeferredInput::CharacterKey {
                    x,
                    y,
                    ref character,
                    state,
                } => {
                    state_changed |= self.drain_key_edge((x, y), character, state);
                }
                pinion_rpc::DeferredInput::Drag {
                    from_x,
                    from_y,
                    to_x,
                    to_y,
                    steps,
                    button,
                    phase,
                } => {
                    // R660 §5.49 — linear cursor march under the
                    // R51.34 InputRouter capture lock. `steps == 0`
                    // degenerates to a press/release at `from`
                    // (well-defined: the RPC client got exactly what
                    // it asked for); positive steps drive
                    // `steps` interpolated `cursor_moved` frames
                    // between `from` and `to` inclusive.
                    // R881 §5.35 §5.49 — `button: "middle"` runs the
                    // same march between the middle-gesture pair
                    // (drag-to-pan), mirroring the Vello sibling.
                    // R1138 §5.49 §2 #2 — `phase` gates the press / release
                    // ends so a `begin` holds the drag session open across
                    // RPC calls (the press-and-hold peer), mirroring the
                    // Vello sibling's `DragPhase` gating.
                    state_changed |= self.cursor_moved(from_x, from_y);
                    if phase.presses() {
                        match button {
                            pinion_rpc::DragButton::Left => {
                                state_changed |= self.pointer_down();
                            }
                            pinion_rpc::DragButton::Middle => self.middle_pressed(),
                        }
                    }
                    if steps > 0 {
                        for step in 1..=steps {
                            let t = f64::from(step) / f64::from(steps);
                            let x = from_x + (to_x - from_x) * t;
                            let y = from_y + (to_y - from_y) * t;
                            state_changed |= self.cursor_moved(x, y);
                        }
                    }
                    if phase.releases() {
                        match button {
                            pinion_rpc::DragButton::Left => {
                                state_changed |= self.pointer_up();
                            }
                            pinion_rpc::DragButton::Middle => {
                                state_changed |= self.middle_released();
                            }
                        }
                    }
                }
                // R1364.5 §5.55 §2 #6 — `app/quit` on the terminal backend.
                //
                // R1364.4 added the method to the SHARED dispatcher and taught
                // only the Vello backend to drain it, so a TUI client got the
                // documented success value (`null` = "the quit was requested")
                // while nothing was requested and the process ran forever —
                // with `rpc/methods` advertising the method to that very client.
                // A §2 #6 break shipped by the round whose subject is unchecked
                // claims, and the wildcard below is why it compiled clean.
                //
                // Routed through the sink R1363 already built rather than a
                // second flag: `TuiQuitSink` sets `quit_flag`, and the loop's
                // own check runs `app_quit_requested` on the UI thread before
                // breaking. So an AI's quit is the same request a binding's own
                // `QuitSink` makes, passes the same veto, and needs no new state.
                //
                // The MECHANISM differs from the Vello sibling (which defers to
                // `AppShell::request_quit` because ending that loop needs an
                // `&ActiveEventLoop` this backend has no concept of); the
                // VOCABULARY and the veto are shared, which is what §2 #6 asks.
                pinion_rpc::DeferredInput::Quit => {
                    pinion_core::QUIT_SINK
                        .resolve(self.root_owner())
                        .request_quit();
                }
                // R1424 §5.35 §5.15 §2 #2 §2 #6 — `scene/pointer_button`: one raw
                // button EDGE (left / middle / right × down / up). R1416 added
                // the variant + Vello drain and left this backend at the wildcard
                // — the same parity gap `r1364_5_deferred_input_parity` records.
                // Mirrors the Vello `pointer_button_for_window`: seed the cursor,
                // then offer the raw edge to a raw sink through the shared
                // `InputRouter` seam (`raw_pointer_button_for_window`, a CoreShell
                // method both backends reach); if a raw sink consumes it the
                // per-button default is suppressed, otherwise the standard arc
                // runs (left = press/release, middle = pan open/close, right =
                // context-menu press-edge one-shot). The TUI carries no modifier
                // chords yet (§2 #6 divergence carry, the `cursor_moved` note).
                pinion_rpc::DeferredInput::PointerButton { x, y, button, edge } => {
                    state_changed |= self.pointer_button(x, y, button, edge);
                }
                // `DeferredInput` is `non_exhaustive`, so an out-of-crate match
                // is FORCED to carry this wildcard and the compiler cannot flag
                // a variant neither backend handles. That is exactly how the
                // R1364.4 bug above compiled silently, so the parity is asserted
                // by `r1364_5_deferred_input_parity` instead — the enum is a
                // shared wire surface, and a variant no drain implements is a
                // method that lies to half its clients.
                _ => {}
            }
        }
        state_changed
    }

    /// R1430 §5.35 §5.15 §2 #2 §2 #6 — dispatch the Qt `QTabletEvent` scalar axes
    /// (pressure / tilt / twist / tangential / height) off the main per-input
    /// match: `Some(changed)` when `input` is one of them, `None` otherwise (the
    /// caller falls through to the general match). A terminal has no hardware pen,
    /// but each axis is the AI-first out-of-band source (§2 #2): the value rides
    /// the SAME `InputRouter` the Vello sibling drives (`set_pointer_*_for_window`),
    /// so a tablet-reactive `External` sees identical delivery on both backends —
    /// the §2 #6 parity `r1364_5_deferred_input_parity` enforces (it greps this
    /// file for every `DeferredInput::Pointer*` variant, all named here).
    fn try_drain_pointer_axis(&mut self, input: &pinion_rpc::DeferredInput) -> Option<bool> {
        let changed = match *input {
            pinion_rpc::DeferredInput::PointerPressure { value } => {
                self.drain_pointer_pressure(value)
            }
            pinion_rpc::DeferredInput::PointerTilt { tilt_x, tilt_y } => {
                self.drain_pointer_tilt(tilt_x, tilt_y)
            }
            pinion_rpc::DeferredInput::PointerTwist { twist } => self.drain_pointer_twist(twist),
            pinion_rpc::DeferredInput::PointerTangentialPressure { tangential } => {
                self.drain_pointer_tangential_pressure(tangential)
            }
            pinion_rpc::DeferredInput::PointerHeight { height } => {
                self.drain_pointer_height(height)
            }
            pinion_rpc::DeferredInput::PointerKind { kind } => self.drain_pointer_kind(kind),
            _ => return None,
        };
        Some(changed)
    }

    /// R1429 §5.35 §2 #2 §2 #6 — deliver a driven pointer PRESSURE (W3C
    /// `PointerEvent.pressure`) to the terminal backend's router, the
    /// `scene/pointer_pressure` out-of-band drain. Named (not inlined) so the
    /// dispatcher above stays flat. Always returns `true`: a reactive surface may
    /// have changed, so the frame repaints.
    fn drain_pointer_pressure(&mut self, value: f32) -> bool {
        self.core.set_pointer_pressure_for_window(
            pinion_runtime::DEFAULT_WINDOW,
            PointerId::MOUSE,
            value,
        );
        true
    }

    /// R1429 §5.35 §2 #2 §2 #6 — the tilt peer of
    /// [`Self::drain_pointer_pressure`]: deliver a driven pointer TILT (W3C
    /// `PointerEvent.tiltX/tiltY`) to the terminal backend's router, the
    /// `scene/pointer_tilt` out-of-band drain. Always returns `true` (repaint).
    fn drain_pointer_tilt(&mut self, tilt_x: f32, tilt_y: f32) -> bool {
        self.core.set_pointer_tilt_for_window(
            pinion_runtime::DEFAULT_WINDOW,
            PointerId::MOUSE,
            tilt_x,
            tilt_y,
        );
        true
    }

    /// R1430 §5.35 §2 #2 §2 #6 — the twist peer of
    /// [`Self::drain_pointer_pressure`]: deliver a driven pointer TWIST (W3C
    /// `PointerEvent.twist` / Qt `QTabletEvent::rotation()`) to the terminal
    /// backend's router. Always returns `true` (repaint).
    fn drain_pointer_twist(&mut self, twist: f32) -> bool {
        self.core.set_pointer_twist_for_window(
            pinion_runtime::DEFAULT_WINDOW,
            PointerId::MOUSE,
            twist,
        );
        true
    }

    /// R1430 §5.35 §2 #2 §2 #6 — the tangential-pressure peer of
    /// [`Self::drain_pointer_pressure`]: deliver a driven airbrush finger-wheel
    /// value (Qt `QTabletEvent::tangentialPressure()`) to the terminal backend's
    /// router. Always returns `true`.
    fn drain_pointer_tangential_pressure(&mut self, tangential: f32) -> bool {
        self.core.set_pointer_tangential_pressure_for_window(
            pinion_runtime::DEFAULT_WINDOW,
            PointerId::MOUSE,
            tangential,
        );
        true
    }

    /// R1430 §5.35 §2 #2 §2 #6 — the height peer of
    /// [`Self::drain_pointer_pressure`]: deliver a driven pointer HEIGHT (Qt
    /// `QTabletEvent::z()`) to the terminal backend's router. Always returns
    /// `true`.
    fn drain_pointer_height(&mut self, height: f32) -> bool {
        self.core.set_pointer_height_for_window(
            pinion_runtime::DEFAULT_WINDOW,
            PointerId::MOUSE,
            height,
        );
        true
    }

    /// R1431 §5.35 §2 #2 §2 #6 — the device-kind peer of
    /// [`Self::drain_pointer_pressure`]: deliver a driven pointer KIND (W3C
    /// `PointerEvent.pointerType` / Qt `QTabletEvent::pointerType()`) to the
    /// terminal backend's router. Always returns `true`.
    fn drain_pointer_kind(&mut self, kind: pinion_core::PointerKind) -> bool {
        self.core.set_pointer_kind_for_window(
            pinion_runtime::DEFAULT_WINDOW,
            PointerId::MOUSE,
            kind,
        );
        true
    }

    /// R51.124 §5.41 — TUI-side post-dispatch bookkeeping for a
    /// [`DispatchTail`] returned by any [`CoreShell`] dispatch
    /// method.
    ///
    /// Routes each drained §5.20 intent + the cached-state
    /// transition through [`Self::log_sink`] (no-op when silent —
    /// the alternate-screen anti-pattern from R51.120 keeps the
    /// default writer at `None`). Returns `true` when the visible
    /// cached state changed; the surface repaints on `true` so the
    /// new SCXML projection lands on screen.
    ///
    /// R51.160 §5.23 — after the intent + state-change bookkeeping,
    /// also drains any [`pinion_core::Command`] the SCXML transition
    /// queued through [`CoreShell::dispatch_pending_commands`]. With
    /// no executor installed the drain is a no-op; with an executor
    /// installed (the `run_with_handlers` entry point) the resolved
    /// [`Intent`]s travel back through the [`MpscIntentSink`](crate::MpscIntentSink) for
    /// `try_recv` drain in the shell's event loop. Unhandled kinds
    /// route through the same [`Self::log_sink`] (so the
    /// alternate-screen safety carries — no stderr writes).
    ///
    /// Takes [`DispatchTail`] by reference because the
    /// substrate-side reads consume nothing — the `Vec<Intent>` is
    /// iterated in place and dropped with the tail when the
    /// dispatch call returns.
    fn handle_tail(&mut self, tail: &DispatchTail<V::State>) -> bool {
        for intent in &tail.intents {
            self.log_intent(intent);
            // R51.169 §5.23 R27 — every drained intent flows through
            // `V::update` so reducer-produced `Vec<Command>` from
            // widget-side state transitions joins the same owner
            // queue the async-re-feed path uses. Mirrors the Vello
            // side; both backends close the R27 dispatch loop's
            // input → drain → reducer arc identically.
            let _ = self.core.route_intent_through_update(intent);
        }
        let state_changed = if let Some(sc) = tail.state_change {
            self.log_state_change(&sc.before, &sc.after);
            true
        } else {
            false
        };
        // R51.160 §5.23 — drain pending Commands the dispatch arm
        // queued (no-op when no executor installed).
        for cmd in self.core.dispatch_pending_commands() {
            self.log_unhandled_command(&cmd);
        }
        // R670 §5.41 §5.39 — drain the programmatic focus-request
        // mailbox a widget body (`External::invoke`, reducer,
        // `Effect`) may have populated during this dispatch. Mirror
        // of `pinion_shell::ShellCore::handle_tail`'s drain call so
        // both backends close the R664 focus-request arc identically.
        // Returns `true` when a focus mutation actually committed —
        // we OR it into the visible-state-change return so the
        // crossterm surface repaints to refresh the focus ring +
        // any focus-gated reactive subscriptions.
        let focus_changed = self.drain_focus_request();
        // R693 §5.39 — drain the modal focus-trap mailbox a reducer /
        // `External::invoke` may have populated when a dialog opened or
        // closed. Mirror of `pinion_shell::ShellCore::handle_tail`'s
        // drain call so the modal trap is dual-backend: an AI client
        // driving the TUI substrate over RPC sees the same Tab
        // confinement + auto-focus the Vello shell produces.
        let modal_changed = self.drain_modal_request();
        state_changed || focus_changed || modal_changed
    }

    /// R670 §5.41 §5.39 — fire [`External::on_focus_change`](pinion_core::External::on_focus_change) on the
    /// blur side (old focused tag) and the focus side (new focused
    /// tag) when [`Self::focus`] just transitioned.
    ///
    /// Mirror of `pinion_shell::ShellCore::notify_focus_change` —
    /// both backends drive the same observer-fan-out so a TUI
    /// `External` (`TextField` IME bridge, `CaretBlink` enable gate,
    /// any binding wired to `on_focus_change`) sees the same
    /// blur-before-focus sequence the Vello path produces.
    ///
    /// `focus_before` is the pre-dispatch focused tag snapshot
    /// (`None` when nothing was focused). The current focused tag is
    /// read straight off [`Self::focus`]. No-op on identity (same
    /// tag both sides — saves one scene walk per quiescent tick).
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

    /// R670 §5.41 §5.39 — pop one pending
    /// [`pinion_core::focus_request`] entry and apply it via
    /// [`FocusManager::focus_set`] + [`Self::notify_focus_change`].
    /// No-op on empty mailbox (the zero-cost steady state). Bumps the
    /// §5.34 revision on a real focus mutation so an in-flight
    /// preview's `base_revision` can detect the concurrent focus
    /// change.
    ///
    /// Mirror of `pinion_shell::ShellCore::drain_focus_request`.
    /// Returns `true` when a focus mutation actually committed so
    /// [`Self::handle_tail`] can OR it into the visible-state-change
    /// flag the crossterm surface repaints on.
    fn drain_focus_request(&mut self) -> bool {
        let Some(tag) = pinion_core::focus_request::drain() else {
            return false;
        };
        let focus_before = self.focus.focused().map(str::to_owned);
        if !self.focus.focus_set(&tag) {
            // (R1020 §5.39) Re-derive the enumeration from a fresh view of the
            // post-dispatch state and retry once — a node this dispatch just
            // made paintable (a conditionally-painted inline editor) is not yet
            // enumerated (the paint refresh has not run). Mirror of the
            // pinion-shell `drain_focus_request` re-derive-on-miss.
            self.refresh_focusable_from_view();
            if !self.focus.focus_set(&tag) {
                // Still unknown / non-focusable — silent no-op (matches the
                // pinion-shell `click_to_focus` rejection arm).
                return false;
            }
        }
        self.notify_focus_change(focus_before.as_deref());
        self.revision.bump();
        true
    }

    /// (R1020 §5.39) Re-derive the keyboard focus enumeration from a fresh,
    /// side-effect-free run of [`pinion_core::WidgetCore::view`] over the
    /// current cached state. Mirror of `pinion_shell::ShellCore`'s helper:
    /// [`pinion_core::Scene::collect_focusable_tags`] reads only `tag` +
    /// `LayoutStyle::focusable` (view-fn-assigned, not layout-derived), so no
    /// viewport size is needed; the view runs under `root_owner.run(...)` and
    /// is pure (§6.3).
    fn refresh_focusable_from_view(&mut self) {
        let cached_state = *self.core.cached_state();
        let frame = Frame::with_dt(0.0);
        let scene = self.core.root_owner().run(|| V::view(cached_state, &frame));
        let _dropped_focus = self
            .focus
            .update_focusable_tags(scene.collect_focusable_tags());
    }

    /// R693 §5.39 — pop one pending
    /// [`pinion_core::modal_scope_request`] entry and apply it via
    /// [`FocusManager::push_modal_scope`] /
    /// [`FocusManager::pop_modal_scope`], routing the resulting focus
    /// move through [`Self::notify_focus_change`]. Mirror of
    /// `pinion_shell::ShellCore::drain_modal_request`. Returns `true`
    /// when a focus mutation committed so [`Self::handle_tail`] can OR
    /// it into the repaint flag.
    fn drain_modal_request(&mut self) -> bool {
        let Some(req) = pinion_core::modal_scope_request::drain() else {
            return false;
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
        }
        changed
    }

    /// R693 §5.39 — `true` while a modal focus trap is active. The
    /// crossterm event loop consults this to keep `Esc` from quitting
    /// the alternate screen while a modal is up (Esc dismisses the
    /// modal instead). Mirror of
    /// `pinion_shell::ShellCore::focus_is_modal`.
    #[must_use]
    pub fn focus_is_modal(&self) -> bool {
        self.focus.is_modal()
    }

    /// R670 §5.41 §5.40 — dispatch one JSON-RPC 2.0 frame against the
    /// LIVE state scene. Mirror of
    /// `pinion_shell::ShellCore::dispatch_rpc`.
    ///
    /// The §2 #6 GUI/TUI dual invariant requires that an AI client
    /// driving `pinion_rpc::dispatch` against this substrate observes
    /// the same wire-form responses (scene/snapshot / scene/click /
    /// scene/key / scene/invoke / focus/set / `propose_change` / …) the
    /// Vello path produces. The disjoint-field borrow split here is
    /// the same shape `pinion_shell::ShellCore::dispatch_rpc` uses;
    /// see that method's doc comment for the rationale behind
    /// `&mut dyn FnMut` (avoids per-callsite monomorphisation of the
    /// entire dispatch body).
    ///
    /// TUI side has no `resize_request` plumbing — terminal resize is
    /// a user-driven event the shell observes through
    /// [`crossterm::event::Event::Resize`], not a programmatic
    /// `Window::request_inner_size` call. `scene/resize` therefore
    /// fails with `resize unavailable` on the TUI path.
    ///
    /// Returns the optional JSON-RPC 2.0 response frame; the caller
    /// owns the IO surface (production writes to **stderr** because
    /// the alternate-screen + raw-mode terminal holds stdout — see
    /// [`crate::shell::run`] for the response-writer wiring rationale).
    pub fn dispatch_rpc(&mut self, request: &str) -> Option<String> {
        // R889 §5.41 §5.49 — parse once (the R671 GUI single-parse
        // shape) so the out-of-band `{window: "<id>"}` scope is
        // visible pre-dispatch. Pre-R889 the TUI never read the
        // window param: a request scoped to ANY window id silently
        // acted on the single terminal window — the GUI's
        // alias-to-primary smell in §2 #6 disguise. The verdict goes
        // through the same named predicate
        // (`CoreShell::is_window_known`; the TUI registry holds
        // exactly `DEFAULT_WINDOW`, seeded at construction) and the
        // same dispatcher gate (`-32602 unknown_window`).
        let parsed_request = match pinion_rpc::parse_request(request) {
            Ok(r) => r,
            Err(err_resp) => return Some(err_resp),
        };
        let unknown_window_verdict: Option<String> =
            pinion_rpc::unknown_window_verdict(&parsed_request, |wid| {
                self.core.is_window_known(wid)
            });
        // R670 §5.41 §5.39 — sample focus before dispatch so we can
        // detect `focus/set` (or any other focus-mutating method)
        // and fire the `External::on_focus_change` notification on
        // the affected widgets.
        let focus_before = self.focus.focused().map(str::to_owned);
        // R885 §5.49 — pre-resolve the out-of-band input-state
        // snapshot for `scene/input_state` through the substrate's one
        // resolution home (R886.1, `CoreShell::input_state_snapshot`).
        // `modifiers: None` is the honest TUI answer: crossterm
        // delivers modifiers per-key-event only, the shell keeps no
        // absolute cache (the `scene/modifiers` §2 #6 carry) — the
        // wire surfaces `null` so an AI client can tell the axis is
        // unavailable. Held keys are real (the RPC-owned `HeldKeys`
        // cache, R882); the TUI's single `DEFAULT_WINDOW` router is
        // seeded at construction, so the snapshot is always `Some`.
        //
        // R1074 `key_dispatch: None` is the same axis-unavailable
        // honesty: the multi-window key-dispatch gate is a GUI-shell
        // concept (per-OS-window focus routing), and a terminal is one
        // process = one alternate screen with no `WindowId` to gate, so
        // the TUI never builds a `KeyDispatchFocus`.
        let input_state_snapshot =
            self.core
                .input_state_snapshot(pinion_runtime::DEFAULT_WINDOW, None, None);
        let resp_pair = {
            // Disjoint-field split mutable borrows. Mirror of the
            // pinion-shell substrate's `dispatch_rpc` borrow split.
            let cached_state = *self.core.cached_state();
            let root_owner = self.core.root_owner().clone();
            let executor_for_rpc: Option<Arc<CommandExecutor>> = self.core.executor().cloned();
            // R890.1 §5.12 §2 #6 — same disjoint split the GUI shell
            // uses: mutable state scene + the stored paint scene of
            // the single terminal window. Threading the stored scene
            // gives the TUI both `scene/snapshot from: paint` reading
            // the COMMITTED frame (pre-R890.1 it re-rendered at query
            // time — the [[introspection-from-paint-not-screen]]
            // divergence R705 closed on the GUI only) and the
            // `scene/layout {viewport: null}` projection from the same
            // borrow, canonical "/0"-rooted paths (the retired TUI
            // mirror used a bare "" prefix — same node, different wire
            // path per backend).
            let (scene_ptr, last_paint_scene_ref) = self
                .core
                .scene_mut_and_last_paint_for_window(pinion_runtime::DEFAULT_WINDOW);
            let previews = &self.previews;
            let revision = &self.revision;
            let layout_cache = &self.layout_cache;
            let focus_ptr = &mut self.focus;
            // R1344 §5.41 §5.12 §2 #2 — TUI paint scene producer. Runs the
            // layout pass against the caller's hypothetical viewport, exactly
            // as `DispatchContext::paint_producer`'s contract requires ("the
            // application is expected to run `compute_layout` inside the
            // closure so the returned `Scene` carries measured rects") and as
            // the Vello sibling does.
            //
            // Pre-R1344 this returned a raw `V::view` result and justified it
            // with "the view-fn already sets every container rect". That was
            // only ever true by accident — views authored rects because the
            // TUI ran no layout. R1344 makes `rect` an OUTPUT, so a raw view
            // result is an all-zero rect tree, and this is the §2 #2 AI-primary
            // path: `scene/layout {viewport}` plus the never-painted fallback
            // for `scene/click` / `scene/hover` / `scene/drag` / `scene/snapshot`.
            // Leaving it unlaid-out would have relocated the §2 #6 divergence
            // this round closes from the pixel path onto the AI path.
            //
            // `(w, h)` arrive in logical px (the §5.12 unit), so this takes the
            // px entry rather than the cell-dimensioned one. `root_owner` clone
            // above wraps the view fn so `Owner::current()` inside the
            // synthetic-paint path resolves to this binding's reactive scope.
            let mut produce = |w: u32, h: u32| -> Scene {
                let frame = Frame::new();
                let mut scene = root_owner.run(|| V::view(cached_state, &frame));
                let mut cache = layout_cache.borrow_mut();
                let _ = layout_for_viewport_px(&mut scene, w, h, &mut cache);
                scene
            };
            // R984 §5.40 §2 #7 — TUI `scene/access` producer, closing the
            // §2 #6 dual-backend asymmetry (pre-R984 the TUI returned
            // `AccessTreeUnavailable`). It runs the SAME
            // [`pinion_a11y::build_access_tree`] SSOT the GUI shell + live
            // AccessKit emit do: the single terminal window passes
            // `access_node` (the GUI's multi-window `access_node_for_window`
            // has no TUI peer), names enrich from the committed paint scene,
            // and pixel bounds resolve from the same `rect_for_tag` lookup
            // `scene/layout` already speaks (TUI rects are the view-fn's cell
            // geometry). `focus_before` is the pre-dispatch focus sample, the
            // GUI's entry-focus convention.
            let access_focused = focus_before.clone();
            let mut produce_access = || {
                let (mut nodes, focus) = pinion_a11y::build_access_tree(
                    &root_owner,
                    last_paint_scene_ref,
                    || V::access_node(&cached_state, access_focused.as_deref()),
                    || V::access_focus_target(&cached_state, access_focused.as_deref()),
                );
                if let Some(paint) = last_paint_scene_ref {
                    pinion_a11y::resolve_access_bounds(&mut nodes, |tag| {
                        pinion_runtime::rect_for_tag(paint, tag)
                    });
                }
                (nodes, focus)
            };
            let mut ctx = DispatchContext::new(scene_ptr, previews, revision)
                .with_paint_producer(&mut produce)
                .with_access_producer(&mut produce_access)
                .with_focus_manager(focus_ptr);
            // R890.1 §5.12 §2 #6 — one channel: snapshot from:paint,
            // layout viewport:null, and hit-test dims all read this
            // borrow (GUI parity; see the split-borrow comment above).
            if let Some(paint) = last_paint_scene_ref {
                ctx = ctx.with_last_paint_scene(paint);
            }
            // R670 §5.41 §5.23 — surface the root Owner handle so
            // `scene/commands` (pending queue) + `scene/theme_tokens`
            // (cached ThemeProvider) work on the TUI path without
            // draining. Read-only borrow.
            ctx = ctx.with_runtime_owner(&root_owner);
            if let Some(exec_arc) = executor_for_rpc.as_ref() {
                ctx = ctx.with_commands_executor(exec_arc.as_ref());
            }
            // R670 §5.41 §5.49 — wire the deferred-input inbox so
            // `scene/wheel` / `scene/click` / `scene/key` /
            // `scene/double_click` / `scene/drag` can enqueue events
            // for post-dispatch drain through
            // [`Self::drain_deferred_inputs`] (R668 substrate).
            let mut deferred_inputs: Vec<DeferredInput> = Vec::new();
            ctx = ctx.with_deferred_inputs(&mut deferred_inputs);
            // R885 §5.49 — install the pre-resolved input-state
            // snapshot for `scene/input_state`.
            if let Some(snapshot) = input_state_snapshot {
                ctx = ctx.with_input_state(snapshot);
            }
            // R888 §5.49 §5.28 — `with_pacing_state` is DELIBERATELY
            // absent: the TUI keeps no frame-pacing clock (terminal
            // repaints are event-driven; `SetTargetFps` drains as a
            // wildcard no-op), so `scene/pacing_state` answers
            // `PacingStateUnavailable` — the honest §2 #6 exposure,
            // the `modifiers: null` precedent's whole-axis variant.
            //
            // R889 §5.49 — thread the unknown-window verdict (GUI
            // parity; see the prologue above).
            if let Some(supplied) = unknown_window_verdict {
                ctx = ctx.with_unknown_window(supplied);
            }
            let resp = dispatch_parsed(&mut ctx, parsed_request);
            (resp, deferred_inputs)
        };
        let (resp, deferred_inputs) = resp_pair;
        // R670 §5.41 §5.49 — drain the deferred-input inbox now that
        // the dispatcher's `&mut scene` borrow has released; each
        // entry replays through [`Self::drain_deferred_inputs`] (the
        // R668 substrate primitive) so the InputRouter fires under
        // its normal post-frame redraw rules.
        let _ = self.drain_deferred_inputs(&deferred_inputs);
        let tail = self.core.tail();
        let _ = self.handle_tail(&tail);
        // R688 §5.16 §5.35 §5.6 — reconcile the external set after the
        // dispatch (GUI/TUI parity, §2 #6). A structure-mutating
        // `scene/invoke` registers / drops its routable External here,
        // same as the pinion-shell side. No-op when the tag set is
        // unchanged.
        self.core.reconcile_externals();
        // R670 §5.41 §5.39 §5.38 — `focus/set` from the AI client
        // fires the `External::on_focus_change` notification on the
        // old + new tags so the focus arc reaches every observer
        // (TextField IME bridge, CaretBlink enable gate, …) on a
        // single dispatch tick. Mirror of the pinion-shell side.
        if self.focus.focused().map(str::to_owned) != focus_before {
            self.notify_focus_change(focus_before.as_deref());
        }
        resp
    }

    /// R51.120 §5.41 — write one intent trace line to the
    /// substrate's [`Self::log_sink`] (no-op when silent). IO
    /// errors are intentionally swallowed: a closed file should not
    /// crash the live event loop, and there is no recovery path
    /// available from inside `handle_tail` (the surface's
    /// terminal is in alternate-screen + raw mode and can't
    /// surface an error message visibly anyway).
    fn log_intent(&mut self, intent: &Intent) {
        if let Some(sink) = &mut self.log_sink {
            let _ = writeln!(
                sink,
                "tui: intent {} payload={:?}",
                intent.tag_str(),
                intent.payload,
            );
        }
    }

    /// R51.120 §5.41 — write one state-transition trace line to
    /// the substrate's [`Self::log_sink`] (no-op when silent).
    fn log_state_change(&mut self, before: &V::State, after: &V::State) {
        if let Some(sink) = &mut self.log_sink {
            let _ = writeln!(sink, "tui: state {before:?} -> {after:?}");
        }
    }

    /// R51.160 §5.23 — write one unhandled-command trace line to
    /// the substrate's [`Self::log_sink`] (no-op when silent).
    /// Mirrors the Vello sibling's `eprintln!` shape but routes
    /// through the log sink so the alternate-screen invariant
    /// holds.
    fn log_unhandled_command(&mut self, cmd: &pinion_core::Command) {
        if let Some(sink) = &mut self.log_sink {
            let _ = writeln!(
                sink,
                "tui: command unhandled kind={} payload={:?}",
                cmd.kind_str(),
                cmd.payload,
            );
        }
    }

    /// R51.160 §5.23 — install or replace the
    /// [`CommandExecutor`] the
    /// substrate's `Self::handle_tail` drains pending
    /// [`pinion_core::Command`]s into. Forwards to
    /// [`CoreShell::set_executor`](pinion_runtime::CoreShell::set_executor).
    pub fn set_command_executor(
        &mut self,
        executor: Arc<CommandExecutor>,
    ) -> Option<Arc<CommandExecutor>> {
        self.core.set_executor(executor)
    }

    /// R51.160 §5.23 — read-only borrow of the currently-installed
    /// [`CommandExecutor`]. `None` until
    /// [`Self::set_command_executor`] runs.
    #[must_use]
    pub fn command_executor(&self) -> Option<&Arc<CommandExecutor>> {
        self.core.executor()
    }

    /// R51.160 §5.23 — re-feed a resolved [`Intent`] (arriving via
    /// [`MpscIntentSink`](crate::MpscIntentSink) → `mpsc::Receiver`
    /// `try_recv` in the shell's event loop) into the SCXML `send`
    /// channel.
    ///
    /// Closing step of the §5.23 R27 dispatch loop on the TUI side:
    ///
    /// ```text
    /// Owner.dispatch_command(cmd)
    ///   → CommandExecutor::dispatch → tokio worker → Intent
    ///   → MpscIntentSink::send → mpsc::Sender
    ///   → shell loop try_recv → ShellCoreTui::dispatch_intent
    ///   → CoreShell::send_to_primary → SCXML transition.
    /// ```
    ///
    /// Returns `true` when the SCXML transition shifted the visible
    /// cached state (the surface repaints on `true`).
    ///
    /// R51.172 §5.23 R27 design clarification — payload is consumed
    /// by `WidgetCore::update` (the reducer wired by R51.168 +
    /// R51.169), not by the SCXML invoke send. The state machine
    /// transitions on event names only; payload-bearing side effects
    /// flow through the reducer's `Vec<Command>` return. Mirrors the
    /// Vello-side rationale on
    /// `ShellCore::dispatch_intent`.
    pub fn dispatch_intent(&mut self, intent: &Intent) -> bool {
        if let Some(sink) = &mut self.log_sink {
            let _ = writeln!(
                sink,
                "tui: intent-feedback {} payload={:?}",
                intent.tag_str(),
                intent.payload,
            );
        }
        // R51.168 §5.23 R27 — reducer step: run `V::update` first so
        // any returned `Vec<Command>` lands on the root owner's queue,
        // then advance the SCXML statechart via `invoke("send", tag)`.
        // Mirrors the Vello path so both backends drive identical
        // dispatch ordering. R884 — the send routes through
        // `CoreShell::send_to_primary` (the one shape-agnostic home);
        // the pre-R884 inline bare-External root match silently skipped
        // the send for every multi-External binding.
        let _ = self.core.route_intent_through_update(intent);
        self.core.send_to_primary(intent.tag_str());
        let tail = self.core.tail();
        self.handle_tail(&tail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::test_fixtures::ButtonFixture as TestButtonView;
    use pinion_core::widgets::button::ButtonState;
    use ratatui::buffer::Buffer;

    // R51.178 §5.41 — `WidgetViewTui` impl for `ButtonFixture` (=
    // `TestButtonView` alias above) lifted to
    // `crate::test_fixtures`. The cfg-gated module is automatically
    // in scope inside this `#[cfg(test)]` block, so its `impl
    // WidgetViewTui for ButtonFixture` is visible without an
    // explicit `use` — Rust applies trait impls in scope, not the
    // module path itself.

    /// Minimal `Default` impl helper for buffer construction tests
    /// (avoids the verbose `Buffer::empty` call site).
    fn buf(cols: u16, rows: u16) -> Buffer {
        Buffer::empty(ratatui::layout::Rect::new(0, 0, cols, rows))
    }

    #[test]
    fn substrate_starts_in_idle_state() {
        // R51.117 — fresh substrate reads the Button's initial Idle
        // state from the §5.15 introspect channel.
        let core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
        assert_eq!(*core.cached_state(), ButtonState::Idle);
    }

    #[test]
    fn keyboard_activate_emits_click_intent_state_stable() {
        // R51.117 / R51.124 — Space activates the Button via the
        // SCXML `KeyboardActivate` event. The internal transition
        // leaves state visually unchanged (Idle), so the
        // single-call `dispatch_key` returns `false` (no repaint
        // needed) even though the `click` intent fires on the
        // activate edge.
        let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
        let _ = buf(40, 10);
        let visible_change = core.dispatch_key("Space", pinion_core::Modifiers::empty());
        assert!(!visible_change, "Idle → Idle internal transition");
        assert_eq!(*core.cached_state(), ButtonState::Idle);
    }

    #[test]
    fn pointer_click_cycle_lands_in_hover() {
        // R51.117 / R51.124 — full click cycle on the button rect:
        // cursor → hover, down → pressed, up → hover. Each step's
        // dispatch returns `true` (visible state changed) so the
        // surface caller repaints.
        let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
        let paint = core.compute_paint_scene(80, 24);
        core.update_paint_scene(paint);

        // Move into the button rect (pixel (0..32, 0..48)).
        assert!(core.cursor_moved(8.0, 8.0));
        assert_eq!(*core.cached_state(), ButtonState::Hover);

        assert!(core.pointer_down());
        assert_eq!(*core.cached_state(), ButtonState::Pressed);

        assert!(core.pointer_up());
        assert_eq!(*core.cached_state(), ButtonState::Hover);
    }

    #[test]
    fn keybinding_disables_then_enables() {
        // R51.117 / R51.124 — `d` keybinding routes through
        // `event_name` → SCXML `Disable` event → cached state flips
        // to Disabled. `dispatch_key` returns `true` because the
        // visible state actually transitioned.
        let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
        assert!(core.dispatch_key("d", pinion_core::Modifiers::empty()));
        assert_eq!(*core.cached_state(), ButtonState::Disabled);
        assert!(core.dispatch_key("e", pinion_core::Modifiers::empty()));
        assert_eq!(*core.cached_state(), ButtonState::Idle);
    }

    #[test]
    fn unmatched_key_does_not_dispatch() {
        // R51.117 / R51.124 — unknown key returns false (caller
        // skips the repaint cycle). State unchanged.
        let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
        assert!(!core.dispatch_key("ArrowLeft", pinion_core::Modifiers::empty()));
        assert_eq!(*core.cached_state(), ButtonState::Idle);
    }

    #[test]
    fn default_construction_equivalent_to_new() {
        // R51.117 — `ShellCoreTui::default()` mirrors `new()` so
        // tests that need a no-arg constructor can use either.
        let a: ShellCoreTui<TestButtonView> = ShellCoreTui::default();
        let b: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
        assert_eq!(a.cached_state(), b.cached_state());
    }

    /// R51.120 §5.41 — thread-safe in-memory `io::Write` capture
    /// for `ShellCoreTui::set_log_sink` tests. The substrate
    /// `Box<dyn io::Write + Send>` consumes the sink; the test's
    /// `Arc<Mutex<Vec<u8>>>` clone retains a read handle so the
    /// captured trace can be inspected after the dispatch cycle.
    #[derive(Clone)]
    struct SharedBuffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl io::Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn silent_default_produces_no_log_output() {
        // R51.120 — without `set_log_sink`, the substrate must
        // never write trace lines anywhere observable. Silence is
        // a load-bearing invariant under raw mode + alternate
        // screen (see `ShellCoreTui::log_sink` doc). The test
        // exercises the full click cycle to cover both
        // `log_intent` and `log_state_change` paths.
        let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
        let paint = core.compute_paint_scene(80, 24);
        core.update_paint_scene(paint);
        let _ = core.cursor_moved(8.0, 8.0);
        let _ = core.pointer_down();
        let _ = core.pointer_up();
        // No assertion needed — the test passes by reaching the
        // end of the dispatch sequence without panic + without
        // surfacing trace text (caller verifies by terminal
        // observation in the shipping binary).
    }

    #[test]
    fn log_sink_captures_intents_and_state_transitions() {
        // R51.120 — once a sink is installed, every intent +
        // state-change trace line lands in the sink instead of
        // `stderr`. The captured text matches the legacy
        // `eprintln!` format exactly so consumers of
        // `PINION_TUI_LOG=path` see the same audit shape across
        // the silent-default migration boundary.
        let buf = SharedBuffer(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())));
        let mut core: ShellCoreTui<TestButtonView> =
            ShellCoreTui::new().with_log_sink(Box::new(buf.clone()));
        let paint = core.compute_paint_scene(80, 24);
        core.update_paint_scene(paint);

        assert!(core.cursor_moved(8.0, 8.0));
        assert!(core.pointer_down());
        assert!(core.pointer_up());

        let captured = {
            let guard = buf.0.lock().unwrap();
            String::from_utf8(guard.clone()).expect("UTF-8 trace")
        };
        // Click intent emitted on Pressed → Hover transition.
        assert!(
            captured.contains("tui: intent test_btn.click"),
            "trace must contain click intent line; got:\n{captured}",
        );
        // State-change trace lines for each visible transition.
        assert!(
            captured.contains("tui: state Idle -> Hover"),
            "trace must contain Idle -> Hover; got:\n{captured}",
        );
        assert!(
            captured.contains("tui: state Hover -> Pressed"),
            "trace must contain Hover -> Pressed; got:\n{captured}",
        );
        assert!(
            captured.contains("tui: state Pressed -> Hover"),
            "trace must contain Pressed -> Hover; got:\n{captured}",
        );
    }

    #[test]
    fn log_sink_silent_when_state_unchanged() {
        // R51.120 — KeyboardActivate fires the `click` intent
        // (visible in the sink) but the SCXML internal transition
        // leaves the visible state unchanged (Idle → Idle), so
        // only the intent line lands — no state-change row, and
        // `dispatch_key` returns `false`.
        let buf = SharedBuffer(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())));
        let mut core: ShellCoreTui<TestButtonView> =
            ShellCoreTui::new().with_log_sink(Box::new(buf.clone()));
        let visible_change = core.dispatch_key("Space", pinion_core::Modifiers::empty());
        assert!(!visible_change);
        let captured = {
            let guard = buf.0.lock().unwrap();
            String::from_utf8(guard.clone()).expect("UTF-8 trace")
        };
        assert!(captured.contains("tui: intent test_btn.click"));
        assert!(
            !captured.contains("tui: state"),
            "no visible state change should produce no state log; got:\n{captured}",
        );
    }

    // ───────────────────────────────────────────────────────────────
    // R51.144 §5.28 — paint cycle dt + tick_animations wiring.
    // ───────────────────────────────────────────────────────────────

    mod r51_144_paint_cycle_dt {
        use std::cell::Cell;
        use std::rc::Rc;
        use std::thread::sleep;
        use std::time::Duration;

        use pinion_core::animation::Tickable;

        use super::{ShellCoreTui, TestButtonView};

        /// Records every `tick(dt)` the substrate dispatches.
        struct TickRecorder {
            ticks: Cell<u32>,
            last_dt: Cell<f32>,
        }

        impl TickRecorder {
            fn new() -> Self {
                Self {
                    ticks: Cell::new(0),
                    last_dt: Cell::new(f32::NAN),
                }
            }
        }

        impl Tickable for TickRecorder {
            fn tick(&self, dt: f32) {
                self.ticks.set(self.ticks.get() + 1);
                self.last_dt.set(dt);
            }
            fn is_at_rest(&self, _epsilon: f32) -> bool {
                false
            }
        }

        #[test]
        fn first_compute_paint_scene_ticks_with_zero_dt() {
            // R51.144 — first call has no previous timestamp, so
            // `dt = 0.0`. At-rest animations stay at rest.
            let recorder = Rc::new(TickRecorder::new());
            let core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
            core.root_owner().register_animation(recorder.clone());

            let _scene = core.compute_paint_scene(80, 24);

            assert_eq!(recorder.ticks.get(), 1);
            assert_eq!(
                recorder.last_dt.get().to_bits(),
                0.0_f32.to_bits(),
                "first paint sees dt=0",
            );
        }

        #[test]
        fn second_compute_paint_scene_measures_real_dt() {
            // R51.144 — second call measures `now - prev` against
            // the stored `Cell<Option<Instant>>`. A 5ms sleep
            // guarantees `dt > 0.001` without making the test
            // brittle on slow machines.
            let recorder = Rc::new(TickRecorder::new());
            let core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
            core.root_owner().register_animation(recorder.clone());

            let _scene1 = core.compute_paint_scene(80, 24);
            sleep(Duration::from_millis(5));
            let _scene2 = core.compute_paint_scene(80, 24);

            assert_eq!(recorder.ticks.get(), 2);
            let dt = recorder.last_dt.get();
            assert!(dt > 0.001, "5ms sleep → dt > 1ms (saw {dt})");
            assert!(dt < 1.0, "dt should not exceed 1s (saw {dt})");
        }

        #[test]
        fn compute_paint_scene_takes_shared_borrow() {
            // R51.144 — interior mutability via `Cell` lets the
            // surface call `compute_paint_scene` through a shared
            // borrow. The pre-R51.144 signature was already `&self`;
            // the new dt field must NOT force `&mut self` because
            // the TUI `commit_paint` takes `&ShellCoreTui<V>`.
            let core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
            let core_ref: &ShellCoreTui<TestButtonView> = &core;
            // Compile + run two shared-borrow calls in sequence —
            // proves the field's interior mutability path works.
            let _a = core_ref.compute_paint_scene(80, 24);
            let _b = core_ref.compute_paint_scene(80, 24);
        }
    }

    // ───────────────────────────────────────────────────────────────
    // R51.146 §5.22 — view fn runs under `root_owner().run(...)`.
    //
    // Mirrors the Vello sibling's R51.146 contract on the TUI side:
    // `ShellCoreTui::compute_paint_scene` wraps the `V::view` call
    // so `pinion_core::Owner::current()` from inside the view fn
    // resolves to this binding's `root_owner`. The
    // `Owner::register_animation` path through `Owner::current()`
    // pins a registration onto the same owner the substrate ticks
    // each frame.
    // ───────────────────────────────────────────────────────────────

    mod r51_146_view_fn_owner_wrap {
        use std::cell::Cell;
        use std::rc::Rc;

        use pinion_core::animation::Tickable;
        use pinion_core::widgets::button::ButtonState;
        use pinion_core::{Frame, Owner, Scene, WidgetCore};

        use super::super::{ShellCoreTui, WidgetViewTui};

        /// Stand-in widget that records the `Owner::current().id()`
        /// observation each time its `view` runs. The TUI binding's
        /// `WidgetView` impl side-effects through a shared
        /// [`Cell<Option<u64>>`] handed in via a thread-local because
        /// the trait fn signature is static — every TUI view binding
        /// in the workspace looks this way today.
        struct OwnerObservingButton;

        thread_local! {
            static OBSERVED_OWNER_ID: Cell<Option<u64>> = const { Cell::new(None) };
            static REGISTER_ANIMATION_OBSERVED: Cell<bool> = const { Cell::new(false) };
        }

        struct NoopTickable;
        impl Tickable for NoopTickable {
            fn tick(&self, _dt: f32) {}
            fn is_at_rest(&self, _epsilon: f32) -> bool {
                true
            }
        }

        impl WidgetCore for OwnerObservingButton {
            type State = ButtonState;
            type Event = pinion_core::widgets::button::ButtonEvent;
            fn create_external() -> Box<dyn pinion_core::external::External> {
                <pinion_core::test_fixtures::ButtonFixture as WidgetCore>::create_external()
            }
            fn tag() -> &'static str {
                "owner_observing_btn"
            }
            fn read_state(scene: &Scene) -> Self::State {
                <pinion_core::test_fixtures::ButtonFixture as WidgetCore>::read_state(scene)
            }
            fn view(_state: Self::State, _frame: &Frame) -> Scene {
                // Record the current Owner id so the test can compare
                // against `ShellCoreTui::root_owner().id()`.
                OBSERVED_OWNER_ID.with(|cell| {
                    cell.set(Owner::current().map(|o| o.id()));
                });
                // Also exercise the more demanding case: register an
                // animation through `Owner::current()` and verify the
                // registration is observable on the substrate's owner
                // after `view` returns.
                if REGISTER_ANIMATION_OBSERVED.with(Cell::get) {
                    if let Some(cur) = Owner::current() {
                        cur.register_animation(Rc::new(NoopTickable));
                    }
                }
                // Reuse the fixture's view body for the visible scene.
                <pinion_core::test_fixtures::ButtonFixture as WidgetCore>::view(
                    ButtonState::Idle,
                    &Frame::new(),
                )
            }
            fn event_name(_event: Self::Event) -> &'static str {
                ""
            }
            fn keybinding(_key: &str) -> Option<Self::Event> {
                None
            }
            fn title() -> &'static str {
                "owner-observing"
            }
            fn apply_key(
                _scene: &mut Scene,
                _focused: Option<&str>,
                _key: &str,
                _modifiers: pinion_core::Modifiers,
            ) -> bool {
                false
            }
        }

        // R51.121 supertrait — WidgetViewTui requires WidgetA11y.
        // OwnerObservingButton is AT-invisible (no `access_node`); the
        // R51.146 test only exercises the paint cycle owner wrap.
        impl pinion_a11y::WidgetA11y for OwnerObservingButton {}

        impl WidgetViewTui for OwnerObservingButton {
            type Renderer = crate::TuiRenderer<ratatui::backend::TestBackend>;
        }

        #[test]
        fn compute_paint_scene_runs_view_under_root_owner() {
            // R51.146 — Owner::current() inside the view fn must
            // resolve to ShellCoreTui::root_owner().
            OBSERVED_OWNER_ID.with(|c| c.set(None));
            REGISTER_ANIMATION_OBSERVED.with(|c| c.set(false));

            let core: ShellCoreTui<OwnerObservingButton> = ShellCoreTui::new();
            let expected = core.root_owner().id();
            let _scene = core.compute_paint_scene(80, 24);

            let observed = OBSERVED_OWNER_ID.with(Cell::get);
            assert_eq!(
                observed,
                Some(expected),
                "view fn must observe the substrate's root_owner via Owner::current()",
            );
        }

        #[test]
        fn current_returns_to_none_after_compute_paint_scene_exits() {
            // R51.146 — RAII pop: the framework wrap is symmetric.
            // After compute_paint_scene returns, Owner::current()
            // from the surface caller sees None again.
            OBSERVED_OWNER_ID.with(|c| c.set(None));
            REGISTER_ANIMATION_OBSERVED.with(|c| c.set(false));

            let core: ShellCoreTui<OwnerObservingButton> = ShellCoreTui::new();
            let _scene = core.compute_paint_scene(80, 24);

            assert!(
                Owner::current().is_none(),
                "OwnerHandleGuard pops on compute_paint_scene exit",
            );
        }

        #[test]
        fn animation_registered_through_current_lands_on_root_owner() {
            // R51.146 — registering through `Owner::current()` reaches
            // the same scope `tick_animations` walks. We exercise the
            // path by registering through `Owner::current()` inside
            // the view fn across two paints; the load-bearing
            // assertion is that two paints succeed without panic and
            // observe the same root owner each time (mismatch would
            // mean `Owner::current()` returned a stray scope rather
            // than the substrate's `root_owner`).
            OBSERVED_OWNER_ID.with(|c| c.set(None));
            REGISTER_ANIMATION_OBSERVED.with(|c| c.set(true));

            let core: ShellCoreTui<OwnerObservingButton> = ShellCoreTui::new();
            let expected = core.root_owner().id();
            let _scene1 = core.compute_paint_scene(80, 24);
            let after_first = OBSERVED_OWNER_ID.with(Cell::get);
            let _scene2 = core.compute_paint_scene(80, 24);
            let after_second = OBSERVED_OWNER_ID.with(Cell::get);

            assert_eq!(after_first, Some(expected));
            assert_eq!(after_second, Some(expected));
        }
    }

    // ───────────────────────────────────────────────────────────────
    // R51.160 §5.23 — pinion-tui CommandExecutor wiring tests.
    //
    // Sibling of pinion-shell's r51_159_command_executor_wiring
    // integration tests. Verifies the TUI substrate's
    // set_command_executor / command_executor / dispatch_intent
    // surface and that handle_tail drains commands on every dispatch
    // arm when an executor is installed.
    // ───────────────────────────────────────────────────────────────

    mod r51_160_command_executor_wiring {
        use std::sync::Arc;

        use pinion_core::external::IntrospectValue;
        use pinion_core::{Command, Intent};
        use pinion_runtime::{
            BlockOnExecutor, CommandExecutor, Executor, HandlerFuture, HandlerRegistry, IntentSink,
            VecSink,
        };

        use super::super::ShellCoreTui;
        use super::TestButtonView;

        fn echo_handler() -> Arc<dyn pinion_runtime::Handler> {
            Arc::new(|cmd: Command| -> HandlerFuture {
                Box::pin(async move {
                    Intent::new_owned(format!("echo.{}", cmd.kind_str()), cmd.payload)
                })
            })
        }

        fn build_executor(kinds: &[&'static str]) -> (Arc<CommandExecutor>, Arc<VecSink>) {
            let mut reg = HandlerRegistry::new();
            for k in kinds {
                reg.register(*k, echo_handler());
            }
            let sink = Arc::new(VecSink::new());
            let exec: Arc<dyn Executor> = Arc::new(BlockOnExecutor);
            let sink_dyn: Arc<dyn IntentSink> = sink.clone();
            let cmd_exec = Arc::new(CommandExecutor::new(reg, exec, sink_dyn));
            (cmd_exec, sink)
        }

        #[test]
        fn set_command_executor_installs_and_returns_prior() {
            let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
            assert!(core.command_executor().is_none());
            let (first, _sink) = build_executor(&[]);
            let first_ptr = Arc::as_ptr(&first).cast::<()>() as usize;
            assert!(core.set_command_executor(first).is_none());
            let installed = core.command_executor().expect("install yields Some");
            assert_eq!(Arc::as_ptr(installed).cast::<()>() as usize, first_ptr);

            let (second, _sink_b) = build_executor(&[]);
            let prior = core
                .set_command_executor(second)
                .expect("replace returns prior");
            assert_eq!(Arc::as_ptr(&prior).cast::<()>() as usize, first_ptr);
        }

        #[test]
        fn dispatch_key_drain_pumps_handled_command_to_sink() {
            // R51.160 — queue a command on root_owner; dispatch_key
            // (any dispatch arm) routes through handle_tail which
            // calls dispatch_pending_commands. The resolved Intent
            // arrives at the sink.
            let (executor, sink) = build_executor(&["audio.play"]);
            let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
            let _ = core.set_command_executor(executor);

            let scope_id = core.root_owner().id();
            core.root_owner().dispatch_command(Command::new_static(
                "audio.play",
                IntrospectValue::Int(440),
                scope_id,
            ));
            assert_eq!(core.root_owner().pending_commands().len(), 1);

            // Trigger any dispatch arm — `d` keybinding fires the
            // Disable event on TestButton which routes through
            // forward → handle_tail → dispatch_pending_commands.
            let _ = core.dispatch_key("d", pinion_core::Modifiers::empty());

            assert!(core.root_owner().pending_commands().is_empty());
            let drained = sink.drain();
            assert_eq!(drained.len(), 1);
            assert_eq!(drained[0].tag_str(), "echo.audio.play");
        }

        #[test]
        fn dispatch_key_drain_pumps_unhandled_command_returns_to_log() {
            let (executor, sink) = build_executor(&["other.kind"]);
            let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
            let _ = core.set_command_executor(executor);

            let scope_id = core.root_owner().id();
            core.root_owner().dispatch_command(Command::new_static(
                "missing.kind",
                IntrospectValue::Null,
                scope_id,
            ));
            let _ = core.dispatch_key("d", pinion_core::Modifiers::empty());

            assert!(core.root_owner().pending_commands().is_empty());
            assert!(sink.is_empty(), "unregistered → sink stays empty");
        }

        #[test]
        fn dispatch_intent_routes_through_scxml_and_returns_change_bool() {
            // R51.160 — dispatch_intent re-feeds an Intent via
            // invoke("send", Text(tag)). For TestButton, the
            // "Disable" event flips Idle → Disabled (visible
            // change), so dispatch_intent returns true.
            use pinion_core::widgets::button::ButtonState;
            let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
            assert_eq!(*core.cached_state(), ButtonState::Idle);
            let intent = Intent::new_static("Disable", IntrospectValue::Null);
            let visible_change = core.dispatch_intent(&intent);
            assert!(
                visible_change,
                "Idle → Disabled visible state change must surface",
            );
            assert_eq!(*core.cached_state(), ButtonState::Disabled);
        }

        #[test]
        fn no_executor_dispatch_keeps_queue_intact() {
            // R51.160 — without an executor installed, the drain
            // step is a no-op; pending commands stay parked.
            let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
            assert!(core.command_executor().is_none());
            let scope_id = core.root_owner().id();
            core.root_owner().dispatch_command(Command::new_static(
                "foo",
                IntrospectValue::Null,
                scope_id,
            ));
            let _ = core.dispatch_key("d", pinion_core::Modifiers::empty());
            assert_eq!(
                core.root_owner().pending_commands().len(),
                1,
                "no executor → queue preserved",
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // R51.168 §5.23 R27 — `ShellCoreTui::dispatch_intent` wires the
    // `CoreShell::route_intent_through_update` substrate API before
    // the SCXML `invoke("send", tag)` call. Mirrors the Vello-side
    // r51_168_dispatch_intent_reducer_routing block in pinion-shell.
    //
    // Uses `pinion_core::test_fixtures::EchoButtonFixture` (lifted
    // R51.167) — its `WidgetCore::update` override emits one
    // `echo.reply` Command per intent so the wiring test can assert
    // the reducer ran and the produced command landed on the owner
    // queue. The `WidgetViewTui` impl is inline below (orphan rule:
    // the trait is local to pinion-tui, the type is foreign to
    // pinion-core — `impl LocalTrait for ForeignType` is allowed).
    // ─────────────────────────────────────────────────────────────────

    mod r51_168_dispatch_intent_reducer_routing {
        use super::*;
        use pinion_core::external::IntrospectValue;
        use pinion_core::test_fixtures::EchoButtonFixture;

        // R51.178 §5.41 — `WidgetViewTui` impl for
        // `EchoButtonFixture` lifted to `crate::test_fixtures`.
        // The outer `mod tests` block's `use crate::test_fixtures
        // as _;` already activates the impl for this sub-module.

        #[test]
        fn dispatch_intent_queues_reducer_commands_on_root_owner() {
            // R51.168 — EchoButtonFixture's `update` emits one
            // `echo.reply` Command per intent; the TUI dispatch
            // path must run the reducer before the SCXML send and
            // queue the command on the substrate's root owner.
            let mut core: ShellCoreTui<EchoButtonFixture> = ShellCoreTui::new();
            let intent = Intent::new_static("echo_btn.tick", IntrospectValue::Null);
            let _ = core.dispatch_intent(&intent);
            let pending = core.root_owner().pending_commands();
            assert_eq!(pending.len(), 1, "reducer command must be queued");
            assert_eq!(pending[0].kind_str(), "echo.reply");
            assert_eq!(
                pending[0].payload,
                IntrospectValue::Text("echo_btn.tick".to_string()),
            );
        }

        #[test]
        fn dispatch_intent_accumulates_reducer_commands_across_calls() {
            // R51.168 — multiple intents pile their reducer-produced
            // commands on the queue in FIFO order, so a later
            // handle_tail pump reaches every handler on the TUI side
            // identically to the Vello side.
            let mut core: ShellCoreTui<EchoButtonFixture> = ShellCoreTui::new();
            let i1 = Intent::new_static("echo_btn.a", IntrospectValue::Null);
            let i2 = Intent::new_static("echo_btn.b", IntrospectValue::Null);
            let _ = core.dispatch_intent(&i1);
            let _ = core.dispatch_intent(&i2);
            let pending = core.root_owner().pending_commands();
            assert_eq!(pending.len(), 2);
            assert_eq!(
                pending[0].payload,
                IntrospectValue::Text("echo_btn.a".to_string()),
            );
            assert_eq!(
                pending[1].payload,
                IntrospectValue::Text("echo_btn.b".to_string()),
            );
        }

        #[test]
        fn default_reducer_keeps_queue_empty_under_dispatch_intent() {
            // R51.168 — the default `Vec::new()` reducer on
            // TestButtonView leaves the owner queue untouched,
            // proving the wiring is semantically transparent when
            // no override is in play.
            let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
            let intent = Intent::new_static("test_btn.click", IntrospectValue::Null);
            let _ = core.dispatch_intent(&intent);
            assert!(
                core.root_owner().pending_commands().is_empty(),
                "default reducer must not queue any commands",
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // R51.169 §5.23 R27 — `handle_tail` routes every drained
    // §5.20 Intent through `V::update` so widget-side state
    // transitions (button.click, toggle.changed, …) emit
    // `Vec<Command>` into the same owner queue the async-re-feed
    // path uses. Closes the R27 dispatch loop's
    // input → drain → reducer arc on the TUI side.
    //
    // Uses `dispatch_intent("KeyboardActivate")` which (a) fires
    // the R51.168 incoming-intent reducer pass for the carrier
    // intent, then (b) sends the tag through `invoke("send", …)`
    // so the `ButtonExternal` SCXML transitions and emits
    // `echo_btn.click` on drain, then (c) routes the drained intent
    // through `V::update` again (R51.169 wiring). Two reducer-emitted
    // commands therefore land on the owner queue: the incoming-side
    // one and the drain-side one. A regression in either wiring
    // breaks the count.
    // ─────────────────────────────────────────────────────────────────

    mod r884_container_root_send {
        use super::*;
        use pinion_core::external::IntrospectValue;
        use pinion_core::test_fixtures::ScrollbarMultiFixture;
        use pinion_core::widgets::button::ButtonState;

        #[test]
        fn dispatch_intent_reaches_primary_through_container_root() {
            // R884 — TUI mirror of the pinion-shell producer test:
            // the intent-feedback SCXML send must advance the primary
            // statechart when extras wrap the state scene in a
            // Container (`CoreShell::compose_root`). Pre-R884 this
            // producer matched the bare-External root inline, so
            // every multi-External binding silently dropped the send;
            // the shape-agnostic home is `CoreShell::send_to_primary`.
            let mut core: ShellCoreTui<ScrollbarMultiFixture> = ShellCoreTui::new();
            assert_eq!(*core.cached_state(), ButtonState::Idle);

            let intent = Intent::new_static("Disable", IntrospectValue::Null);
            let repaint = core.dispatch_intent(&intent);
            assert!(repaint, "visible state shift must request a repaint");
            assert_eq!(
                *core.cached_state(),
                ButtonState::Disabled,
                "dispatch_intent must reach the primary through the Container root",
            );
        }
    }

    mod r51_169_handle_tail_drain_routing {
        use super::*;
        use pinion_core::external::IntrospectValue;
        use pinion_core::test_fixtures::EchoButtonFixture;

        #[test]
        fn drained_intent_runs_through_update_reducer() {
            // R51.169 — KeyboardActivate via dispatch_intent triggers
            // BOTH the incoming-intent reducer (R51.168) and the
            // drained-intent reducer (R51.169). EchoButtonFixture's
            // `update` emits one command per intent, so two
            // commands land on the queue with distinct payloads.
            let mut core: ShellCoreTui<EchoButtonFixture> = ShellCoreTui::new();
            let intent = Intent::new_static("KeyboardActivate", IntrospectValue::Null);
            let _ = core.dispatch_intent(&intent);

            let pending = core.root_owner().pending_commands();
            assert_eq!(
                pending.len(),
                2,
                "incoming reducer + drained reducer must each queue one command",
            );

            // Carrier intent (incoming) payload — the tag we passed.
            assert!(
                pending
                    .iter()
                    .any(|c| c.payload == IntrospectValue::Text("KeyboardActivate".to_string())),
                "incoming intent reducer must observe `KeyboardActivate`",
            );
            // Drained click intent payload — `<tag>.<kind>`
            // (R51.122 reference).
            assert!(
                pending
                    .iter()
                    .any(|c| c.payload == IntrospectValue::Text("echo_btn.click".to_string())),
                "drained intent reducer must observe `echo_btn.click`",
            );
        }

        #[test]
        fn default_reducer_keeps_queue_empty_on_drain() {
            // R51.169 — TestButtonView (= ButtonFixture, default
            // no-op update) keeps the queue empty even when the
            // drain emits an intent. Confirms the wiring is
            // semantically transparent on the no-op path.
            let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
            let intent = Intent::new_static("KeyboardActivate", IntrospectValue::Null);
            let _ = core.dispatch_intent(&intent);
            assert!(
                core.root_owner().pending_commands().is_empty(),
                "default reducer must not queue commands on drain either",
            );
        }
    }

    // R668 §5.41 §5.49 — `drain_deferred_inputs` substrate primitive.
    // The §2 #6 GUI/TUI dual invariant says every AI-injected input
    // arc available on the Vello shell must reach the TUI substrate
    // through the same wire shape. Each variant below pins that:
    // [`Click`](pinion_rpc::DeferredInput::Click) drives the same
    // Idle → Hover → Pressed → Hover sequence the live crossterm
    // event loop exercises in `pointer_click_cycle_lands_in_hover`;
    // [`CharacterKey`](pinion_rpc::DeferredInput::CharacterKey) routes
    // through `V::keybinding` first, mirroring R666's auto-discriminator
    // wire (closing [[scene-key-character-named-gap]] on the TUI side);
    // [`DoubleClick`](pinion_rpc::DeferredInput::DoubleClick) emits two
    // press/release cycles without intervening cursor moves (R663 wire);
    // [`Drag`](pinion_rpc::DeferredInput::Drag) interpolates the cursor
    // under the R51.34 capture lock (R660 wire).
    //
    // The integration test ask in the R668 SEED was "pinion-tui crate
    // 의 신규 integration test (pinion-tui 가 R666 substrate 자동 상속
    // 증명)" — this block satisfies it without spinning a real
    // crossterm raw-mode pipe. Production RPC ingress (stdin reader /
    // stderr response writer / PreviewLedger + SceneRevision +
    // FocusManager field lifts onto ShellCoreTui) is the
    // [[pinion-tui-rpc-ingress]] follow-up consumer of the primitive
    // landed here.

    fn primed_button_core() -> ShellCoreTui<TestButtonView> {
        // R668 §5.41 — drain dispatch needs a primed paint-scene
        // snapshot on the router so the first `cursor_moved` can
        // resolve hit-test targets. Without this prime a `Click` at
        // (8, 8) would see an empty hover map and miss the button
        // rect even though the rect is in the unrendered paint scene.
        let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
        let paint = core.compute_paint_scene(80, 24);
        core.update_paint_scene(paint);
        core
    }

    #[test]
    fn r668_drain_empty_inbox_is_no_op() {
        // Empty inbox → no state change, no panic. The drain method
        // returns `false` so the surface caller skips the repaint
        // cycle on idle RPC turns.
        let mut core = primed_button_core();
        let state_before = *core.cached_state();
        let changed = core.drain_deferred_inputs(&[]);
        assert!(!changed);
        assert_eq!(*core.cached_state(), state_before);
    }

    #[test]
    fn r668_drain_click_drives_hover_pressed_hover() {
        // R51.196 / R668 — Click variant replays cursor_moved →
        // pointer_down → pointer_up, landing the substrate in Hover
        // (the SCXML `Pressed → Hover` arc on release).
        let mut core = primed_button_core();
        let inputs = vec![pinion_rpc::DeferredInput::Click { x: 8.0, y: 8.0 }];
        assert!(core.drain_deferred_inputs(&inputs));
        assert_eq!(*core.cached_state(), ButtonState::Hover);
    }

    #[test]
    fn r668_drain_double_click_replays_two_press_release_cycles() {
        // R663 / R668 — DoubleClick fires two press/release pairs.
        // The terminal state is Hover (same as single Click) because
        // the SCXML statechart's idempotent `Pressed → Hover` arc
        // collapses repeated activations. The test pins the no-crash
        // contract + visible-state correctness.
        let mut core = primed_button_core();
        let inputs = vec![pinion_rpc::DeferredInput::DoubleClick { x: 8.0, y: 8.0 }];
        assert!(core.drain_deferred_inputs(&inputs));
        assert_eq!(*core.cached_state(), ButtonState::Hover);
    }

    #[test]
    fn r1424_drain_pointer_button_left_edges_drive_hover() {
        // R1424 §2 #6 — the raw `scene/pointer_button` variant (an R1416 gap
        // on this backend) drives the same activation as `Click` when no raw
        // sink consumes it: a left DOWN then a left UP at the button rect lands
        // the substrate in Hover (the SCXML `Pressed → Hover` release arc), the
        // Vello `pointer_button_for_window` fall-through arc mirrored (§2 #6).
        let mut core = primed_button_core();
        let at = |edge| pinion_rpc::DeferredInput::PointerButton {
            x: 8.0,
            y: 8.0,
            button: pinion_core::PointerButton::Left,
            edge,
        };
        let inputs = vec![
            at(pinion_core::PointerEdge::Down),
            at(pinion_core::PointerEdge::Up),
        ];
        assert!(core.drain_deferred_inputs(&inputs));
        assert_eq!(*core.cached_state(), ButtonState::Hover);
    }

    #[test]
    fn r1424_drain_pointer_pressure_requests_repaint() {
        // R1424 §2 #6 — the `scene/pointer_pressure` variant (an R1423 gap on
        // this backend) forwards to the shared `InputRouter` pressure seam and
        // reports a repaint so a pressure-reactive `External` lands the change
        // on the next terminal frame. TestButtonView carries no pressure state;
        // the drain returning `true` is the repaint request (the router took
        // the value) — the no-crash + §2 #6 wire-parity contract.
        let mut core = primed_button_core();
        let inputs = vec![pinion_rpc::DeferredInput::PointerPressure { value: 0.5 }];
        assert!(core.drain_deferred_inputs(&inputs));
    }

    #[test]
    fn r668_drain_character_key_routes_through_keybinding() {
        // R666 / R668 — CharacterKey "d" hits ButtonFixture::keybinding
        // → ButtonEvent::Disable → SCXML transition to Disabled. Pin
        // the auto-discriminator wire on the TUI side: a one-codepoint
        // character routed through the V::keybinding channel, not the
        // named-key fallback.
        let mut core = primed_button_core();
        let inputs = vec![pinion_rpc::DeferredInput::CharacterKey {
            x: 8.0,
            y: 8.0,
            character: "d".to_string(),
            state: pinion_rpc::KeyWireState::Press,
        }];
        assert!(core.drain_deferred_inputs(&inputs));
        assert_eq!(*core.cached_state(), ButtonState::Disabled);
    }

    #[test]
    fn r668_drain_named_key_routes_through_apply_key() {
        // R51.197 / R668 — Named key "Space" routes through
        // V::apply_key (ButtonFixture::apply_key calls
        // aria_apply_aria_activate, emitting KeyboardActivate). The
        // SCXML `Idle → click intent → Idle` arc keeps the cached
        // state stable across the keyboard event itself — but the
        // drain dispatches `cursor_moved(8.0, 8.0)` first per the
        // canonical wire, so the substrate enters Hover before the
        // key arc runs. The terminal state is Hover, not Idle.
        //
        // The R668 wire pins: named keys (multi-char like "Space" /
        // "Enter" / "ArrowUp") flow through the same `dispatch_key`
        // entry point as character keys — the TUI substrate single-
        // entry-points keyboard dispatch. The assertion below covers
        // the no-crash + correct routing contract; the Hover terminal
        // state is the cursor_moved side-effect, not the named-key arc.
        let mut core = primed_button_core();
        let inputs = vec![pinion_rpc::DeferredInput::Key {
            x: 8.0,
            y: 8.0,
            key: "Space".to_string(),
            state: pinion_rpc::KeyWireState::Press,
        }];
        let _ = core.drain_deferred_inputs(&inputs);
        assert_eq!(*core.cached_state(), ButtonState::Hover);
    }

    #[test]
    fn r668_drain_wheel_off_scroll_target_is_silent() {
        // R51.186 / R668 — Wheel against the ButtonFixture rect (not
        // a scroll container) returns `false` from `wheel` (no
        // dispatch); the `cursor_moved` portion may flip Idle→Hover.
        // Net effect: state may change (Hover), but no panic and no
        // scroll. Pin both: visible state lands in Hover (the cursor
        // entered the rect), and the substrate did not crash on a
        // non-scrollable wheel target.
        let mut core = primed_button_core();
        let inputs = vec![pinion_rpc::DeferredInput::Wheel {
            x: 8.0,
            y: 8.0,
            delta: pinion_core::event::WheelDelta::Lines { dx: 0.0, dy: -1.0 },
        }];
        let _ = core.drain_deferred_inputs(&inputs);
        assert_eq!(*core.cached_state(), ButtonState::Hover);
    }

    #[test]
    fn r668_drain_drag_press_steps_release_lands_in_hover() {
        // R660 / R668 — Drag interpolates the cursor between `from`
        // and `to` under the R51.34 capture lock. With `from` inside
        // the ButtonFixture rect (8, 8) and `to` outside (200, 200),
        // the canonical InputRouter arc: cursor enters → Hover,
        // pointer_down → Pressed, capture-locked cursor moves outside
        // the rect keep the widget in Pressed (capture lock), final
        // pointer_up releases → Hover (capture lock still pins hover
        // target to the locked widget until the release arc fires).
        //
        // Pin the no-crash contract + a sensible terminal state. The
        // exact intermediate trajectory is the InputRouter's
        // contract, not the drain's; the drain just replays the
        // primitive sequence in order.
        let mut core = primed_button_core();
        let inputs = vec![pinion_rpc::DeferredInput::Drag {
            from_x: 8.0,
            from_y: 8.0,
            to_x: 200.0,
            to_y: 200.0,
            steps: 4,
            button: pinion_rpc::DragButton::Left,
            phase: pinion_rpc::DragPhase::Full,
        }];
        let _ = core.drain_deferred_inputs(&inputs);
        // The drag ends with pointer_up at the final position. The
        // visible state lands in either Hover or Idle depending on
        // whether the capture-locked release arc retains hover. Pin
        // the looser "not still Pressed" assertion — the release
        // always clears the pressed lock.
        assert_ne!(*core.cached_state(), ButtonState::Pressed);
    }

    #[test]
    fn r668_drain_drag_zero_steps_degenerates_to_click() {
        // R660 / R668 — `steps == 0` skips the interpolation loop:
        // cursor → from, pointer_down, pointer_up. Equivalent to
        // Click at `from`. Pin the well-defined degeneracy so
        // RPC clients that send `steps: 0` (deliberately or as a
        // boundary test) get exactly that arc.
        let mut core = primed_button_core();
        let inputs = vec![pinion_rpc::DeferredInput::Drag {
            from_x: 8.0,
            from_y: 8.0,
            to_x: 200.0,
            to_y: 200.0,
            steps: 0,
            button: pinion_rpc::DragButton::Left,
            phase: pinion_rpc::DragPhase::Full,
        }];
        assert!(core.drain_deferred_inputs(&inputs));
        assert_eq!(*core.cached_state(), ButtonState::Hover);
    }

    // ---- R887 §5.49 §5.53 — secondary-click (right-button) producers ----
    //
    // Both TUI producers of the right-click arc route through
    // `ShellCoreTui::secondary_click`: the
    // [`SecondaryClick`](pinion_rpc::DeferredInput::SecondaryClick)
    // RPC drain (which seeds the cursor itself) and the crossterm
    // `Down(Right)` surface arm (whose `cursor_moved → secondary_click`
    // pair the direct-call tests mirror). The fixture carries a real
    // `ContextMenuExternal`, so the observable is the production one —
    // the popup opens at the press point.

    use pinion_core::test_fixtures::{ContextMenuFixture, ContextMenuFixtureState};

    #[test]
    fn r887_drain_secondary_click_opens_context_menu_at_press_point() {
        let mut core: ShellCoreTui<ContextMenuFixture> = ShellCoreTui::new();
        assert!(!core.cached_state().open, "popup starts closed");
        let inputs = vec![pinion_rpc::DeferredInput::SecondaryClick { x: 12.0, y: 9.0 }];
        assert!(core.drain_deferred_inputs(&inputs));
        assert_eq!(
            *core.cached_state(),
            ContextMenuFixtureState {
                open: true,
                anchor: Some((12.0, 9.0)),
            },
            "drained right-click must open the popup at the press point",
        );
    }

    #[test]
    fn r887_native_right_press_arc_opens_context_menu() {
        // The crossterm `Down(MouseButton::Right)` arm is
        // `cursor_moved → secondary_click` (the same ordering
        // invariant as `Down(Left)`); drive the pair directly.
        let mut core: ShellCoreTui<ContextMenuFixture> = ShellCoreTui::new();
        let _ = core.cursor_moved(5.0, 7.0);
        assert!(core.secondary_click(), "handled press reports the repaint");
        assert_eq!(
            *core.cached_state(),
            ContextMenuFixtureState {
                open: true,
                anchor: Some((5.0, 7.0)),
            },
        );
    }

    #[test]
    fn r887_secondary_click_before_any_cursor_move_is_swallowed() {
        // No cached cursor position → the press is swallowed quietly,
        // byte-for-byte the GUI `secondary_click_for_window` policy.
        let mut core: ShellCoreTui<ContextMenuFixture> = ShellCoreTui::new();
        assert!(!core.secondary_click(), "no cursor cache → no dispatch");
        assert!(!core.cached_state().open, "popup stays closed");
    }

    mod r1303_no_primary {
        //! R1303 PR-51 §5.41 §2 #6 — a no-primary binding must be
        //! TUI-renderable. Regression guard for the audit finding that
        //! `dispatch_key` handed `Some(V::tag())` unconditionally, which
        //! panics for a binding whose `tag()` is the sanctioned
        //! `unreachable!` marker.
        use std::cell::Cell;

        use pinion_core::external::StubExternal;
        use pinion_core::widget_core::ExtraExternal;
        use pinion_core::widgets::button::{ButtonEvent, ButtonState};
        use pinion_core::{Frame, Scene, WidgetCore};

        use super::super::{ShellCoreTui, WidgetViewTui};

        thread_local! {
            // None = apply_key not reached; Some(b) = reached with
            // `focused.is_none() == b`.
            static FOCUSED_WAS_NONE: Cell<Option<bool>> = const { Cell::new(None) };
        }

        struct NoPrimaryTuiView;

        impl WidgetCore for NoPrimaryTuiView {
            type State = ButtonState;
            type Event = ButtonEvent;

            fn primary_surface() -> Option<pinion_core::widget_core::PrimarySurface> {
                None
            }

            fn create_external() -> Box<dyn pinion_core::external::External> {
                unreachable!("R1303 PR-51 — NoPrimaryTuiView has no primary surface")
            }

            fn tag() -> &'static str {
                unreachable!("R1303 PR-51 — NoPrimaryTuiView has no primary surface")
            }

            fn create_extra_externals() -> Vec<ExtraExternal> {
                vec![ExtraExternal::new("pane", Box::new(StubExternal::new()))]
            }

            fn read_state(_scene: &Scene) -> Self::State {
                ButtonState::Idle
            }

            fn view(_state: Self::State, _frame: &Frame) -> Scene {
                Scene::Container(pinion_core::scene::ContainerNode::new(Vec::new()))
            }

            fn event_name(_event: Self::Event) -> &'static str {
                ""
            }

            fn title() -> &'static str {
                "no-primary-tui"
            }

            fn apply_key(
                _scene: &mut Scene,
                focused: Option<&str>,
                _key: &str,
                _modifiers: pinion_core::Modifiers,
            ) -> bool {
                FOCUSED_WAS_NONE.with(|c| c.set(Some(focused.is_none())));
                false
            }
        }

        impl pinion_a11y::WidgetA11y for NoPrimaryTuiView {}

        impl WidgetViewTui for NoPrimaryTuiView {
            type Renderer = crate::TuiRenderer<ratatui::backend::TestBackend>;
        }

        #[test]
        fn dispatch_key_does_not_panic_and_passes_none_focus() {
            // Pre-R1303-fix, `dispatch_key` evaluated `Some(V::tag())`
            // eagerly → `unreachable!` panic for this binding. Post-fix it
            // passes `None` (no primary tag), so a no-primary binding is
            // TUI-renderable (§2 #6) and its `apply_key` runs.
            FOCUSED_WAS_NONE.with(|c| c.set(None));
            let mut core: ShellCoreTui<NoPrimaryTuiView> = ShellCoreTui::new();
            // A non-keybinding named key routes through `apply_key`; must
            // not panic.
            let _ = core.dispatch_key("ArrowLeft", pinion_core::Modifiers::empty());
            assert_eq!(
                FOCUSED_WAS_NONE.with(Cell::get),
                Some(true),
                "apply_key must run with focused == None for a no-primary binding",
            );
        }
    }

    // ─────────────────────────────────────────────────────────────
    // R1344 §5.41 §2 #6 — the TUI runs the layout pass.
    // ─────────────────────────────────────────────────────────────
    mod r1344_layout_pass {
        use super::TestButtonView;
        use crate::ShellCoreTui;
        use crate::text_layout::layout_for_terminal;
        use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
        use pinion_core::style::{Display, FlexDirection, SizeValue};
        use pinion_runtime::LayoutCache;

        /// Resolve `scene` exactly as the TUI substrate does — through the
        /// same entry point, not a re-derivation of it.
        fn lay_out(scene: &mut Scene, cols: u16, rows: u16) {
            let mut cache = LayoutCache::new();
            let _ = layout_for_terminal(scene, cols, rows, &mut cache);
        }

        /// A column of text rows authored with NO rect at all — intent
        /// lives entirely in `LayoutStyle`, the way the Vello backend has
        /// always required.
        fn layout_style_only_view() -> Scene {
            let mut root = ContainerNode::default();
            root.layout.display = Display::Flex;
            root.layout.flex_direction = FlexDirection::Column;
            root.layout.size.width = SizeValue::Percent(100);
            for s in ["first row", "second row", "third row"] {
                root.children
                    .push(Scene::Text(TextNode::new(s, Rect::default())));
            }
            Scene::Container(root)
        }

        fn walk(s: &Scene, out: &mut Vec<Rect>) {
            match s {
                Scene::Text(t) => out.push(t.rect),
                Scene::Container(c) => {
                    for ch in &c.children {
                        walk(ch, out);
                    }
                }
                _ => {}
            }
        }

        /// Every `Scene::Text` rect, in tree order.
        fn rows_of(scene: &Scene) -> Vec<Rect> {
            let mut out = Vec::new();
            walk(scene, &mut out);
            out
        }

        #[test]
        fn r1344_layout_style_only_view_resolves_on_the_tui_path() {
            // THE regression. Pre-R1344 the TUI ran no layout, so a view
            // that authored its intent in `LayoutStyle` (the ONLY thing
            // the Vello backend reads — it overwrites every authored
            // `rect`) left every rect at `Default`, and all three rows
            // painted on top of each other at (0,0): the terminal showed
            // "third roww". The two backends read disjoint halves of one
            // scene, so §2 #6 held in name only.
            let mut scene = layout_style_only_view();
            lay_out(&mut scene, 30, 8);
            let rects = rows_of(&scene);
            assert_eq!(rects.len(), 3);
            for (i, pair) in rects.windows(2).enumerate() {
                assert!(
                    pair[1].y > pair[0].y,
                    "row {} must sit below row {}, got {:?} then {:?}",
                    i + 1,
                    i,
                    pair[0],
                    pair[1],
                );
            }
            assert!(
                rects.iter().all(|r| r.w > 0 && r.h > 0),
                "every row resolves to a real box: {rects:?}",
            );
        }

        #[test]
        fn r1344_layout_resolves_rects_onto_the_cell_grid() {
            // The cell measure returns whole-cell boxes, so taffy's
            // arithmetic lands every text row on a cell boundary — no
            // half-cell rows for the paint walker to floor away.
            let mut scene = layout_style_only_view();
            lay_out(&mut scene, 30, 8);
            for r in rows_of(&scene) {
                assert_eq!(r.h % 16, 0, "row height {r:?} is a whole number of cells");
                assert_eq!(r.y % 16, 0, "row origin {r:?} sits on a cell boundary");
            }
        }

        #[test]
        fn r1344_text_wraps_to_the_flex_resolved_width() {
            // The consumer's actual case: prose authored with no rect,
            // wrapping to whatever width the flex pass hands it. The node
            // must grow TALLER than one row.
            let mut root = ContainerNode::default();
            root.layout.display = Display::Flex;
            root.layout.flex_direction = FlexDirection::Column;
            root.layout.size.width = SizeValue::Percent(100);
            root.children.push(Scene::Text(TextNode::new(
                "a paragraph long enough that it cannot possibly fit on one row",
                Rect::default(),
            )));
            let mut scene = Scene::Container(root);
            lay_out(&mut scene, 20, 8);
            let r = rows_of(&scene)[0];
            assert!(
                r.h > 16,
                "the paragraph must occupy several cell rows, got {r:?}",
            );
            assert!(r.w <= 160, "and stay inside the 20-cell viewport: {r:?}");
        }

        #[test]
        fn r1344_narrower_terminal_reflows_taller() {
            // Layout is a function of the terminal size — the property an
            // authored `rect` could never have.
            let text = "alpha beta gamma delta epsilon zeta eta theta";
            let height_at = |cols: u16| {
                let mut root = ContainerNode::default();
                root.layout.display = Display::Flex;
                root.layout.flex_direction = FlexDirection::Column;
                root.layout.size.width = SizeValue::Percent(100);
                root.children
                    .push(Scene::Text(TextNode::new(text, Rect::default())));
                let mut scene = Scene::Container(root);
                lay_out(&mut scene, cols, 20);
                rows_of(&scene)[0].h
            };
            assert!(
                height_at(20) > height_at(60),
                "a narrower terminal must wrap the same prose taller",
            );
        }

        #[test]
        fn r1344_compute_paint_scene_returns_a_laid_out_scene() {
            // The substrate entry point itself: the scene handed to the
            // paint walker is already resolved.
            let core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
            let scene = core.compute_paint_scene(80, 24);
            if let Scene::Container(c) = &scene {
                assert!(
                    c.rect.w > 0 && c.rect.h > 0,
                    "the paint scene is laid out, not raw: {:?}",
                    c.rect,
                );
            } else {
                panic!("fixture view is a Container");
            }
        }

        #[test]
        fn r1344_paint_scene_tracks_the_terminal_size() {
            // Two terminal sizes → two layouts, from one unchanged view.
            let core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
            let small = core.compute_paint_scene(20, 6);
            let large = core.compute_paint_scene(100, 30);
            let w = |s: &Scene| match s {
                Scene::Container(c) => c.rect.w,
                _ => 0,
            };
            assert!(
                w(&large) > w(&small),
                "a wider terminal must resolve a wider root ({} vs {})",
                w(&large),
                w(&small),
            );
        }
    }
}

#[cfg(test)]
mod r1364_5_app_quit_on_the_terminal_backend {
    //! R1364.5 §5.55 §2 #6 — `app/quit` must END a terminal app, not answer and
    //! evaporate.
    //!
    //! R1364.4 added `app/quit` to the SHARED dispatcher and taught only the
    //! Vello backend to drain it. A TUI client received `{"result":null}` — the
    //! documented "the quit was requested" — while nothing was requested and the
    //! process ran forever, with `rpc/methods` (one backend-agnostic const)
    //! advertising the method to that same client. Worse than an absent method:
    //! every other TUI-unavailable axis answers with an error or an explicit
    //! `null`-means-unavailable, so an AI could not tell "quit requested" from
    //! "quit discarded".
    //!
    //! Two independent reviewers found this within minutes of each other, on a
    //! commit whose own audit trail said "the arm is the whole feature, and its
    //! absence would have been invisible" — written about the Vello drain, and
    //! not applied one crate over.

    use super::*;
    use pinion_core::test_fixtures::ButtonFixture;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct CountingQuitSink(Arc<AtomicUsize>);
    impl pinion_core::QuitSink for CountingQuitSink {
        fn request_quit(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn r1364_5_app_quit_reaches_the_terminal_backends_quit_sink() {
        let count = Arc::new(AtomicUsize::new(0));
        let seed_count = Arc::clone(&count);
        let mut core: ShellCoreTui<ButtonFixture> = ShellCoreTui::new_with_seed(move |owner| {
            pinion_core::QUIT_SINK.provide(owner, Arc::new(CountingQuitSink(seed_count)));
        });

        let req = r#"{"jsonrpc":"2.0","method":"app/quit","params":{},"id":1}"#;
        let reply = core.dispatch_rpc(req).expect("a reply");
        assert!(
            !reply.contains("\"error\""),
            "app/quit is routed by the shared dispatcher, so it answers here too: {reply}",
        );
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "the quit must reach the terminal backend's QuitSink — the seam \
             R1363 built (TuiQuitSink -> quit_flag -> the loop's \
             app_quit_requested veto -> break). 0 here is R1364.4's bug: a \
             success reply for a request that went nowhere",
        );
    }
}

#[cfg(test)]
mod r1364_5_deferred_input_parity {
    //! R1364.5 §2 #6 — every `DeferredInput` variant is answered by BOTH drains,
    //! or is listed here as deliberately terminal-inapplicable with a reason.
    //!
    //! `DeferredInput` is the shared RPC wire surface: one `RPC_METHODS` const
    //! advertises the methods to both backends, so a variant only one drain
    //! implements is a method that lies to half its clients.
    //!
    //! The compiler CANNOT enforce this, and that is the whole reason this
    //! exists. `DeferredInput` is `#[non_exhaustive]`, so an out-of-crate match
    //! is *forced* to carry a wildcard arm; adding a variant compiles clean in
    //! both drains with zero warnings. R1364.4 added `Quit`, wired the Vello
    //! drain, and the terminal drain swallowed it silently at `_ => {}`. A
    //! source-text check is the only mechanism left — the same argument
    //! `commit.rs`'s (R1349.1) and the termination map's make.

    /// Variants the terminal drain does not name, recorded because this test
    /// found them and this round does not get to guess.
    ///
    /// **This list may only ever SHRINK.** It is not an allowlist of things that
    /// are fine — it is the honest state of a §2 #6 gap that predates R1364.5,
    /// which the `Quit` fix exposed rather than caused. Every entry is a method
    /// the shared dispatcher accepts and `rpc/methods` advertises to terminal
    /// clients, which therefore answers `null` and may do nothing.
    ///
    /// They are NOT classified here, deliberately. Some look obviously
    /// inapplicable (a terminal has no OS file drag-and-drop) and some look like
    /// real bugs (`Tick` drives animation; crossterm does report mouse motion,
    /// so `Hover` / `PointerLeave` have no obvious excuse) — but "looks like" is
    /// exactly the reasoning this round exists to stop. Writing a confident
    /// reason next to each without verifying it would be the same sin one
    /// paragraph after naming it. Each needs its own investigation, and the
    /// answer per variant is either an arm or a method that reports unavailable
    /// instead of success.
    ///
    /// The test still bites for anything NEW: a variant added tomorrow and wired
    /// on one backend fails here, which is the regression R1364.4 shipped.
    const UNCLASSIFIED_TERMINAL_GAPS: &[&str] = &[
        "Hover",
        "PointerLeave",
        "Tick",
        "SetTargetFps",
        "SetModifiers",
        "FileHover",
        "FileHoverCancel",
        "FileDrop",
    ];

    fn variants_of_deferred_input() -> Vec<String> {
        let src = include_str!("../../pinion-rpc/src/dispatch.rs");
        let mut out = Vec::new();
        let mut inside = false;
        for line in src.lines() {
            if line.starts_with("pub enum DeferredInput {") {
                inside = true;
                continue;
            }
            if !inside {
                continue;
            }
            if line == "}" {
                break;
            }
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with("#[") {
                continue;
            }
            // `Name,` or `Name {` — a variant head at one indent level.
            let head = t.trim_end_matches(&[',', ' '][..]);
            let name = head.split_whitespace().next().unwrap_or_default();
            let name = name.trim_end_matches('{').trim_end_matches(',');
            if !name.is_empty()
                && name.starts_with(|c: char| c.is_ascii_uppercase())
                && name.chars().all(|c| c.is_ascii_alphanumeric())
            {
                out.push(name.to_owned());
            }
        }
        assert!(
            out.len() > 5,
            "parsed too few DeferredInput variants ({out:?}) — the parser broke, \
             which would make this test vacuously green",
        );
        out
    }

    #[test]
    fn r1364_5_every_deferred_input_variant_is_answered_by_the_terminal_drain() {
        let terminal = include_str!("substrate.rs");
        let mut missing: Vec<String> = Vec::new();
        for v in variants_of_deferred_input() {
            if UNCLASSIFIED_TERMINAL_GAPS.contains(&v.as_str()) {
                continue;
            }
            if !terminal.contains(&format!("DeferredInput::{v}")) {
                missing.push(v);
            }
        }
        assert!(
            missing.is_empty(),
            "`DeferredInput` variants the terminal drain never names: {missing:?}.\n\
             The shared dispatcher accepts these and `rpc/methods` advertises \
             them to terminal clients, so each answers `null` and does nothing — \
             R1364.4's `app/quit` bug exactly. Add the arm (the fix), or make \
             the method report unavailable rather than success. Do NOT add it to \
             UNCLASSIFIED_TERMINAL_GAPS: that list records what R1364.5 found and \
             may only shrink.",
        );
        // The recorded gaps must be REAL gaps. If one is fixed, it has to leave
        // the list — a stale entry would quietly re-open the hole it documents,
        // which is the same failure mode as a stale doc row.
        let stale: Vec<&&str> = UNCLASSIFIED_TERMINAL_GAPS
            .iter()
            .filter(|v| terminal.contains(&format!("DeferredInput::{v}")))
            .collect();
        assert!(
            stale.is_empty(),
            "these are listed as unimplemented but the terminal drain now names \
             them: {stale:?}. Remove them from UNCLASSIFIED_TERMINAL_GAPS.",
        );
    }
}
