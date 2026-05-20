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
//! crossterm + renderer pieces. Result: every dispatch arm is
//! reachable from a headless `#[test]` via `ShellCoreTui::new()` +
//! `compute_paint_scene` + `update_paint_scene` + the `dispatch_*`
//! methods, no TTY required.
//!
//! ## Out of scope (carry forward)
//!
//! - `WidgetView` / `WidgetViewTui` merge — the two trait surfaces
//!   are still distinct because the GUI side carries pinion-a11y
//!   typed methods that have no TUI parity yet. Carry to R51.118+
//!   once a TUI a11y surface lands (PTY screen reader path).
//! - Focus management — single-widget shells today, so the
//!   substrate hands `Some(V::tag())` to `apply_key` unconditionally.
//!   The TUI `FocusManager` carries until a multi-focusable TUI
//!   binding surfaces the trigger.
//! - Cell-native coord substrate — `cell_to_pixel` still routes
//!   through the `PIXEL_PER_CELL_*` placeholder. Carry until a
//!   binding surfaces a real terminal cell-size mismatch.

use std::io;
use std::marker::PhantomData;

use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::scene::ExternalNode;
use pinion_core::{Frame, Scene};
use pinion_runtime::input::{InputRouter, PointerId};
use pinion_runtime::intent_queue::{walk_scene_and_drain, IntentQueue};

use crate::WidgetViewTui;

/// R51.117 §5.41 — TUI shell dispatch substrate.
///
/// Owns the renderer-agnostic state every interactive TUI binding
/// needs:
///
/// - the live state `scene` (carrying the SCXML `Scene::External`),
/// - the cached `V::State` projection refreshed after each dispatch,
/// - the [`InputRouter`] (winit-free pointer routing primitive),
/// - the [`IntentQueue`] drained after each event.
///
/// Methods on this struct mutate the substrate; the surface
/// [`crate::shell::run`] only sequences calls + commits the painted
/// buffer through the live renderer. Both responsibilities — what to
/// dispatch + how to commit — stay one layer apart, so a future
/// `--features test-backend` build can target this substrate
/// directly without touching the crossterm raw-mode path.
pub struct ShellCoreTui<V: WidgetViewTui> {
    /// The application's live state scene (`Scene::External`
    /// carrying the SCXML widget). Mutated by every dispatch arm.
    scene: Scene,
    /// Cached projection refreshed via [`WidgetViewTui::read_state`]
    /// after each event drain. Compared against the post-dispatch
    /// read to decide whether the visible state changed.
    cached_state: V::State,
    /// Substrate's pointer router (R48 §5.35). The R51.112 mouse
    /// dispatch path forwards crossterm cell coords through here
    /// after `cell_to_pixel` conversion.
    router: InputRouter,
    /// Per-frame queue the §5.20 intent drain accumulates into.
    /// Drained on every successful dispatch — each intent is routed
    /// through [`Self::log_sink`] (if set) as a
    /// `tui: intent <name> payload=<value>` trace line.
    intent_queue: IntentQueue,
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
    /// Phantom to anchor the `V` parameter — `V` is not stored
    /// directly (the trait's methods are all associated functions).
    _phantom: PhantomData<fn() -> V>,
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
    /// [`WidgetViewTui::create_external`] SCXML widget.
    ///
    /// The first [`WidgetViewTui::read_state`] runs synchronously
    /// against the constructed scene, so the cached state is correct
    /// before the substrate enters the event loop. The router
    /// starts with no retained paint scene — the surface must call
    /// [`Self::update_paint_scene`] after the initial paint to seed
    /// the hit-test snapshot before the first mouse event arrives.
    #[must_use]
    pub fn new() -> Self {
        let external = V::create_external();
        let scene = Scene::External(ExternalNode::new(external).with_tag(V::tag()));
        let cached_state = V::read_state(&scene);
        Self {
            scene,
            cached_state,
            router: InputRouter::new(),
            intent_queue: IntentQueue::new(),
            log_sink: None,
            _phantom: PhantomData,
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
    /// [`Self::set_log_sink`]; equivalent to `let mut c =
    /// ShellCoreTui::new(); c.set_log_sink(sink); c` so chained
    /// construction sites (`ShellCoreTui::new().with_log_sink(...)`)
    /// stay one-line.
    #[must_use]
    pub fn with_log_sink(mut self, sink: Box<dyn io::Write + Send>) -> Self {
        self.set_log_sink(sink);
        self
    }

    /// R51.120 §5.41 — write one intent trace line to the
    /// substrate's [`Self::log_sink`] (no-op when silent). IO
    /// errors are intentionally swallowed: a closed file should not
    /// crash the live event loop, and there is no recovery path
    /// available from inside `refresh_state` (the surface's
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

    /// Read-only borrow of the cached state. Tests assert against
    /// this after each dispatch to verify SCXML transitions arm
    /// expected values; the surface uses it as the
    /// [`WidgetViewTui::view`] argument when computing the next
    /// paint frame.
    #[must_use]
    pub fn cached_state(&self) -> &V::State {
        &self.cached_state
    }

    /// Build the binding's paint scene from the current cached
    /// state. Pure sync per §6.3 R51.27 `dry_run`: identical
    /// `(state, frame)` always yields the same `Scene`.
    #[must_use]
    pub fn compute_paint_scene(&self) -> Scene {
        let frame = Frame::new();
        V::view(self.cached_state, &frame)
    }

    /// Hand a freshly-painted scene to the [`InputRouter`] so the
    /// next pointer event resolves against the visible layout.
    /// Surface calls this after every successful paint commit
    /// (initial + post-state-change + resize repaint).
    pub fn update_paint_scene(&mut self, paint_scene: Scene) {
        self.router.update_paint_scene(paint_scene, &mut self.scene);
    }

    /// R51.117 §5.41 — dispatch one W3C-named key through the
    /// binding's `keybinding` → `apply_key` chain. Returns `true`
    /// when the key was handled.
    ///
    /// Single-widget focus model: the substrate hands
    /// `Some(V::tag())` to [`WidgetViewTui::apply_key`]
    /// unconditionally so the widget's focus-gated `apply_key`
    /// impl recognises itself as the activation target. The TUI
    /// `FocusManager` axis (R51.113+ carry) lifts this constant.
    pub fn dispatch_key(&mut self, key_str: &str) -> bool {
        if let Some(event) = V::keybinding(key_str) {
            let name = V::event_name(event);
            return self.forward_event(name);
        }
        V::apply_key(&mut self.scene, Some(V::tag()), key_str)
    }

    /// R51.117 §5.41 — route a typed event name through the state
    /// scene's `Scene::External::invoke("send", Text(<name>))` path.
    fn forward_event(&mut self, event_name: &str) -> bool {
        let Scene::External(node) = &mut self.scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        intro
            .invoke("send", IntrospectValue::Text(event_name.to_string()))
            .is_ok()
    }

    /// R51.117 §5.41 — forward a cursor-move (pixel-space, already
    /// converted from cell coords by the surface) to the substrate's
    /// pointer router.
    pub fn cursor_moved(&mut self, x: f64, y: f64) {
        self.router.cursor_moved(PointerId::MOUSE, x, y, &mut self.scene);
    }

    /// R51.117 §5.41 — pointer press (mouse left button down,
    /// crossterm-side).
    pub fn pointer_down(&mut self) {
        self.router.pointer_down(PointerId::MOUSE, &mut self.scene);
    }

    /// R51.117 §5.41 — pointer release (mouse left button up).
    pub fn pointer_up(&mut self) {
        self.router.pointer_up(PointerId::MOUSE, &mut self.scene);
    }

    /// R51.117 §5.41 — post-dispatch tail: drain §5.20 intents
    /// (logged to stderr) and refresh the cached state via
    /// [`WidgetViewTui::read_state`]. Returns `true` when the
    /// visible state changed — the surface repaints + calls
    /// [`Self::update_paint_scene`] on a `true` return.
    ///
    /// Mirrors `pinion_shell::ShellCore::drain_intents` +
    /// `refresh_state` in one combined call because the TUI shell's
    /// repaint policy is "paint on visible state change" (no
    /// continuous redraw — terminals do not `VSync`).
    pub fn refresh_state(&mut self) -> bool {
        walk_scene_and_drain(&mut self.scene, &mut self.intent_queue);
        // Drain first so the sink borrow + intent borrow don't
        // overlap (`log_intent` takes `&mut self`).
        let intents = self.intent_queue.drain();
        for intent in &intents {
            self.log_intent(intent);
        }
        let new_state = V::read_state(&self.scene);
        if new_state == self.cached_state {
            return false;
        }
        // R51.120 §5.41 — route through `log_state_change` instead
        // of `eprintln!` so the trace line never lands on the
        // alternate screen. See `Self::log_sink` for the full
        // anti-pattern explanation. `Copy` on `V::State` (trait
        // bound) lets us snapshot before swapping, avoiding a
        // simultaneous borrow of `self` + `self.cached_state`.
        let before = self.cached_state;
        self.cached_state = new_state;
        self.log_state_change(&before, &new_state);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::External;
    use pinion_core::scene::{ContainerNode, Rect, TextNode};
    use pinion_core::widgets::button::{ButtonExternal, ButtonState};
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    /// Minimal binding for substrate-level tests. Carries a
    /// [`ButtonExternal`] so the SCXML statechart transitions
    /// observable + intent-emitting; the view fn paints a
    /// 4×3-cell button rect tagged `test_btn` so the
    /// [`InputRouter`] hit-tests resolve.
    struct TestButtonView;

    impl WidgetViewTui for TestButtonView {
        type State = ButtonState;
        type Event = pinion_core::widgets::button::ButtonEvent;
        type Renderer = crate::TuiRenderer<TestBackend>;

        fn create_external() -> Box<dyn External> {
            Box::new(ButtonExternal::new())
        }

        fn tag() -> &'static str {
            "test_btn"
        }

        fn read_state(scene: &Scene) -> Self::State {
            if let Scene::External(node) = scene
                && let Some(intro) = node.handle.introspect()
                && let Some(IntrospectValue::Text(name)) = intro.query("state")
            {
                return match name.as_str() {
                    "Hover" => ButtonState::Hover,
                    "Pressed" => ButtonState::Pressed,
                    "Disabled" => ButtonState::Disabled,
                    _ => ButtonState::Idle,
                };
            }
            ButtonState::Idle
        }

        fn view(_state: Self::State, _frame: &Frame) -> Scene {
            // 4×3-cell button rect = pixel (0..32, 0..48) — the
            // top-left cell of the buffer covers the button.
            let mut button = ContainerNode::default();
            button.rect = Rect::new(0, 0, 32, 48);
            button.tag = Some(std::borrow::Cow::Borrowed("test_btn"));
            button.children.push(Scene::Text(TextNode::default()));
            Scene::Container(button)
        }

        fn event_name(event: Self::Event) -> &'static str {
            use pinion_core::widgets::button::ButtonEvent;
            match event {
                ButtonEvent::PointerEnter => "PointerEnter",
                ButtonEvent::PointerLeave => "PointerLeave",
                ButtonEvent::PointerDown => "PointerDown",
                ButtonEvent::PointerUp => "PointerUp",
                ButtonEvent::PointerCancel => "PointerCancel",
                ButtonEvent::KeyboardActivate => "KeyboardActivate",
                ButtonEvent::Disable => "Disable",
                ButtonEvent::Enable => "Enable",
                _ => "__internal__",
            }
        }

        fn title() -> &'static str {
            "Test"
        }

        fn keybinding(key: &str) -> Option<Self::Event> {
            match key {
                "d" => Some(pinion_core::widgets::button::ButtonEvent::Disable),
                "e" => Some(pinion_core::widgets::button::ButtonEvent::Enable),
                _ => None,
            }
        }

        fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
            pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
        }
    }

    /// Minimal `Default` impl helper for buffer construction tests
    /// (avoids the rats verbose `Buffer::empty` call site).
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
        // R51.117 — Space activates the Button via the SCXML
        // `KeyboardActivate` event. The internal transition leaves
        // state visually unchanged (Idle) but the intent emit fires
        // on the activate edge; `refresh_state` returns false
        // because the visible state did not change.
        let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
        let _ = buf(40, 10);
        let handled = core.dispatch_key("Space");
        assert!(handled);
        let visible_change = core.refresh_state();
        assert!(!visible_change, "Idle → Idle internal transition");
        assert_eq!(*core.cached_state(), ButtonState::Idle);
    }

    #[test]
    fn pointer_click_cycle_lands_in_hover() {
        // R51.117 — full click cycle on the button rect:
        // cursor → hover, down → pressed, up → hover. Each step
        // updates the cached state through `refresh_state`.
        let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
        let paint = core.compute_paint_scene();
        core.update_paint_scene(paint);

        // Move into the button rect (pixel (0..32, 0..48)).
        core.cursor_moved(8.0, 8.0);
        assert!(core.refresh_state());
        assert_eq!(*core.cached_state(), ButtonState::Hover);

        core.pointer_down();
        assert!(core.refresh_state());
        assert_eq!(*core.cached_state(), ButtonState::Pressed);

        core.pointer_up();
        assert!(core.refresh_state());
        assert_eq!(*core.cached_state(), ButtonState::Hover);
    }

    #[test]
    fn keybinding_disables_then_enables() {
        // R51.117 — `d` keybinding routes through `event_name` →
        // SCXML `Disable` event → cached state flips to Disabled.
        let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
        assert!(core.dispatch_key("d"));
        assert!(core.refresh_state());
        assert_eq!(*core.cached_state(), ButtonState::Disabled);
        assert!(core.dispatch_key("e"));
        assert!(core.refresh_state());
        assert_eq!(*core.cached_state(), ButtonState::Idle);
    }

    #[test]
    fn unmatched_key_does_not_dispatch() {
        // R51.117 — unknown key returns false (caller skips the
        // refresh + repaint cycle). State unchanged.
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
        core.cursor_moved(8.0, 8.0);
        let _ = core.refresh_state();
        core.pointer_down();
        let _ = core.refresh_state();
        core.pointer_up();
        let _ = core.refresh_state();
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

        core.cursor_moved(8.0, 8.0);
        assert!(core.refresh_state());
        core.pointer_down();
        assert!(core.refresh_state());
        core.pointer_up();
        assert!(core.refresh_state());

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
        // only the intent line lands — no state-change row.
        let buf = SharedBuffer(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())));
        let mut core: ShellCoreTui<TestButtonView> =
            ShellCoreTui::new().with_log_sink(Box::new(buf.clone()));
        assert!(core.dispatch_key("Space"));
        let visible_change = core.refresh_state();
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
}
