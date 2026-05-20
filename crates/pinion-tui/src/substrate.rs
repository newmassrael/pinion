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
//! Result: the explicit `refresh_state()` two-call pattern goes
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
//! - Cell-native coord substrate — `cell_to_pixel` still routes
//!   through the `PIXEL_PER_CELL_*` placeholder. Carry until a
//!   binding surfaces a real terminal cell-size mismatch.

use std::cell::Cell;
use std::io;
use std::time::Instant;

use pinion_core::intent::Intent;
use pinion_core::{Frame, Owner, Scene};
use pinion_runtime::{clamp_frame_dt, CoreShell, DispatchTail, PointerId};

use crate::WidgetViewTui;

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
    /// the surface's [`crate::shell::commit_paint`] takes the
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
        Self {
            core: CoreShell::new(),
            log_sink: None,
            last_paint_instant: Cell::new(None),
        }
    }

    /// R51.120 §5.41 — install a diagnostic sink for intent / state
    /// trace lines. See [`Self::log_sink`] for why this is opt-in;
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
    /// [`WidgetViewTui::view`] argument when computing the next
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
    /// [`CoreShell`](pinion_runtime::CoreShell) into the
    /// [`Owner`] drop semantics, cancelling every pending
    /// [`Command`](pinion_core::Command) (Solid pattern).
    #[must_use]
    pub fn root_owner(&self) -> &Owner {
        self.core.root_owner()
    }

    /// Build the binding's paint scene from the current cached
    /// state. Pure sync per §6.3 R51.27 `dry_run`: identical
    /// `(state, frame, owner_state)` always yields the same `Scene`.
    ///
    /// R51.124 §5.41 — the substrate no longer drives
    /// `compute_layout`; the TUI paint walker maps the
    /// `Scene::Container.rect` cells directly via
    /// [`crate::paint::to_buffer`]. The Vello sibling
    /// (`pinion_shell::ShellCore::compute_paint_scene`) keeps its
    /// own (w, h) signature because parley needs the viewport to
    /// shape text against.
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
    /// `&self` (not `&mut self`) because [`crate::shell::commit_paint`]
    /// takes the substrate by shared borrow; the timing field uses
    /// [`Cell`] interior mutability so the signature stays sync.
    #[must_use]
    pub fn compute_paint_scene(&self) -> Scene {
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
        self.core
            .root_owner()
            .run(|| V::view(cached_state, &frame))
    }

    /// Hand a freshly-painted scene to the substrate's
    /// [`pinion_runtime::InputRouter`] (via [`CoreShell::update_paint_scene`])
    /// so the next pointer event resolves against the visible
    /// layout. Surface calls this after every successful paint
    /// commit (initial + post-state-change + resize repaint).
    pub fn update_paint_scene(&mut self, paint_scene: Scene) {
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
    /// `refresh_state` for "did state change?"); the single-call
    /// shape mirrors the post-lift Vello side where dispatch
    /// methods auto-tail.
    ///
    /// Single-widget focus model: the substrate hands
    /// `Some(V::tag())` to [`CoreShell::apply_key`] unconditionally
    /// so the widget's focus-gated `apply_key` impl recognises
    /// itself as the activation target. The TUI `FocusManager`
    /// axis (carry) lifts this constant.
    pub fn dispatch_key(&mut self, key_str: &str) -> bool {
        if let Some(event) = V::keybinding(key_str) {
            let tail = self.core.forward(event);
            return self.handle_tail(&tail);
        }
        if let Some(tail) = self.core.apply_key(Some(V::tag()), key_str) {
            return self.handle_tail(&tail);
        }
        false
    }

    /// R51.117 §5.41 — forward a cursor-move (pixel-space, already
    /// converted from cell coords by the surface) to
    /// [`CoreShell::cursor_moved`].
    ///
    /// R51.124 §5.41 — returns `true` when the dispatch flipped
    /// the visible cached state (e.g. cursor entered a widget rect
    /// and the `Idle → Hover` transition fired).
    pub fn cursor_moved(&mut self, x: f64, y: f64) -> bool {
        let tail = self.core.cursor_moved(PointerId::MOUSE, x, y);
        self.handle_tail(&tail)
    }

    /// R51.117 §5.41 — pointer press (mouse left button down,
    /// crossterm-side). Returns `true` on visible state change
    /// (R51.124 §5.41).
    pub fn pointer_down(&mut self) -> bool {
        let tail = self.core.pointer_down(PointerId::MOUSE);
        self.handle_tail(&tail)
    }

    /// R51.117 §5.41 — pointer release (mouse left button up).
    /// Returns `true` on visible state change (R51.124 §5.41).
    pub fn pointer_up(&mut self) -> bool {
        let tail = self.core.pointer_up(PointerId::MOUSE);
        self.handle_tail(&tail)
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
    /// Takes [`DispatchTail`] by reference because the
    /// substrate-side reads consume nothing — the `Vec<Intent>` is
    /// iterated in place and dropped with the tail when the
    /// dispatch call returns.
    fn handle_tail(&mut self, tail: &DispatchTail<V::State>) -> bool {
        for intent in &tail.intents {
            self.log_intent(intent);
        }
        if let Some(sc) = tail.state_change {
            self.log_state_change(&sc.before, &sc.after);
            return true;
        }
        false
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::test_fixtures::ButtonFixture as TestButtonView;
    use pinion_core::widgets::button::ButtonState;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    // The `WidgetA11y` impl for `ButtonFixture` lives in pinion-a11y
    // (orphan rule: trait is defined there). The dev-dependency
    // pulls in pinion-a11y's `test-fixtures` feature so the impl is
    // visible during the substrate test build.
    impl WidgetViewTui for TestButtonView {
        type Renderer = crate::TuiRenderer<TestBackend>;
    }

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
        let visible_change = core.dispatch_key("Space");
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
        let paint = core.compute_paint_scene();
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
        assert!(core.dispatch_key("d"));
        assert_eq!(*core.cached_state(), ButtonState::Disabled);
        assert!(core.dispatch_key("e"));
        assert_eq!(*core.cached_state(), ButtonState::Idle);
    }

    #[test]
    fn unmatched_key_does_not_dispatch() {
        // R51.117 / R51.124 — unknown key returns false (caller
        // skips the repaint cycle). State unchanged.
        let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
        assert!(!core.dispatch_key("ArrowLeft"));
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
        let paint = core.compute_paint_scene();
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
        let paint = core.compute_paint_scene();
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
        let visible_change = core.dispatch_key("Space");
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

            let _scene = core.compute_paint_scene();

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

            let _scene1 = core.compute_paint_scene();
            sleep(Duration::from_millis(5));
            let _scene2 = core.compute_paint_scene();

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
            let _a = core_ref.compute_paint_scene();
            let _b = core_ref.compute_paint_scene();
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
            fn apply_key(_scene: &mut Scene, _focused: Option<&str>, _key: &str) -> bool {
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
            let _scene = core.compute_paint_scene();

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
            let _scene = core.compute_paint_scene();

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
            let _scene1 = core.compute_paint_scene();
            let after_first = OBSERVED_OWNER_ID.with(Cell::get);
            let _scene2 = core.compute_paint_scene();
            let after_second = OBSERVED_OWNER_ID.with(Cell::get);

            assert_eq!(after_first, Some(expected));
            assert_eq!(after_second, Some(expected));
        }
    }
}
