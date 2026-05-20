//! R51.110.2 / R51.111 / R51.112 §5.41 — TUI shell entry point
//! (crossterm event loop).
//!
//! [`run`] is the TUI sibling of `pinion_shell::run::<V>()`. It owns
//! the crossterm raw-mode lifecycle (enable on entry, disable via
//! RAII guard on exit), enters the alternate screen so the
//! application's paint output does not clobber the user's existing
//! terminal scrollback, sets the OSC window title, constructs the
//! `TuiRenderer` against the live `CrosstermBackend<Stdout>`, then
//! drives the paint + input + event loop until `Esc` is pressed (the
//! shell-reserved exit key, mirroring the Vello shell's `Escape →
//! window close` convention).
//!
//! ## R51.111 cut — input dispatch + SCXML wire-up
//!
//! Key events flow through the same substrate path the Vello shell's
//! `ShellCore::handle_character_key` / `handle_named_key` arms use:
//!
//! 1. `crossterm::event::KeyEvent` → W3C `KeyboardEvent.key` string
//!    via [`crate::input::key_str_from_event`].
//! 2. `V::keybinding(key_str)` → if `Some(event)`, route through
//!    `Scene::External::invoke("send", Text(V::event_name(event)))`.
//! 3. Otherwise `V::apply_key(&mut scene, Some(V::tag()), key_str)`
//!    — widgets walk to their `Scene::External` and call
//!    `intervene` / `invoke` themselves (`Slider` arrow-key value
//!    mutation, `Button` `Space` / `Enter` keyboard activation).
//! 4. On handled input, `walk_scene_and_drain` pulls §5.20 intents
//!    out of the SCXML widget; cached state refresh via
//!    `V::read_state(&scene)` triggers a repaint when the visible
//!    state changes (mirrors `ShellCore::refresh_state` in
//!    `pinion-shell`).
//!
//! Focus is implicit at this cut — the shell passes
//! `Some(V::tag())` unconditionally. The TUI `FocusManager` axis
//! lands once a second focusable TUI binding surfaces the
//! substrate-incompleteness-signal trigger (R51.113+).
//!
//! ## R51.112 cut — mouse dispatch + `InputRouter` wire-up
//!
//! Mouse events flow through the substrate's `InputRouter` (the
//! winit-free pointer routing primitive from §5.35), reusing the
//! same hover / hit-test / capture state machine the Vello shell
//! drives:
//!
//! 1. crossterm enables mouse capture (`EnableMouseCapture` ANSI
//!    sequence) on entry; the RAII guard issues `DisableMouseCapture`
//!    on exit so the user's terminal does not stay locked into mouse
//!    reporting mode after an `Esc` or panic.
//! 2. `crossterm::event::MouseEvent.column/row` are cell coords;
//!    [`crate::input::cell_to_pixel`] multiplies by the
//!    `PIXEL_PER_CELL_*` constants (same inverse the paint walker
//!    uses) so the substrate's pixel-coord `Scene::Container.rect`
//!    hit-tests align with the visible cells.
//! 3. After every paint, [`InputRouter::update_paint_scene`] retains
//!    a fresh paint-scene snapshot so the next `MouseEvent` resolves
//!    hover targets against the current visual state (mirrors
//!    `ShellCore::finalize_frame` post-render handoff).
//! 4. `MouseEventKind::Down(Left)` → cursor sync + `pointer_down`;
//!    `Up(Left)` → `pointer_up`; `Moved` / `Drag(Left)` →
//!    `cursor_moved`. Right / middle / wheel events are absorbed
//!    silently (Tier-1 widget catalogue has no semantics for them).
//!
//! The §5.20 intent drain + cached-state refresh + repaint cycle
//! collapses into [`drain_and_repaint`] — the keyboard path and the
//! mouse path both call through it so the SCXML statechart's
//! `Pressed → Hover` click intent surfaces identically whichever
//! input source produced the transition.
//!
//! ## What's still deferred to R51.113+
//!
//! - **`FocusManager` + a11y**: defer until the second focusable
//!   TUI binding lands.
//! - **`Backend::Tui` axis on `External::backends`**: today the
//!   binding declares `Backend::Gui` because no dedicated TUI flag
//!   exists; the §5.15 backend taxonomy round lifts it.
//! - **Cell-native coord substrate**: `PIXEL_PER_CELL_*` placeholder
//!   stays until the second TUI binding surfaces a real terminal
//!   cell-size mismatch.
//!
//! These deferrals stay textbook substrate-incompleteness-signal
//! ([[substrate-incompleteness-signal]]) — each shell-side path
//! waits for its concrete first consumer.

use std::io::{self, Stdout, stdout};
use std::time::Duration;

use pinion_core::external::IntrospectValue;
use pinion_core::renderer::WidgetRenderer;
use pinion_core::scene::ExternalNode;
use pinion_core::{Frame, Scene};
use pinion_runtime::input::{InputRouter, PointerId};
use pinion_runtime::intent_queue::{walk_scene_and_drain, IntentQueue};
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::{TuiContext, TuiRenderer, WidgetViewTui};

/// R51.110.2 / R51.112 §5.41 — RAII guard restoring the terminal on
/// drop.
///
/// `Drop` runs even on panic, so the user's terminal is never left
/// in a half-broken raw-mode + alternate-screen + mouse-capture
/// state. Mirrors the `ratatui::Terminal::restore` pattern from the
/// ratatui book (chapter 2 / canonical idiom). R51.112 adds the
/// `DisableMouseCapture` exit step so the terminal's mouse reporting
/// mode is always cleared — leaving it on would make every cursor
/// move after exit produce stray escape sequences in the user's
/// shell.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Silently swallow errors — at drop time the process is
        // either exiting or panicking; surfacing IO errors here
        // would clobber the actual error the user cares about.
        let _ = crossterm::execute!(
            stdout(),
            crossterm::event::DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen,
        );
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// R51.110.2 §5.41 — run the TUI binding `V` end-to-end against the
/// live terminal.
///
/// Enables raw mode + alternate screen, sets the OSC window title,
/// constructs a [`TuiRenderer`] against
/// `CrosstermBackend<Stdout>`, paints the initial state, then loops
/// reading crossterm events until `Esc` is pressed. The terminal is
/// always restored on exit (RAII guard).
///
/// # Errors
/// Propagates `std::io::Error` from any crossterm / ratatui call:
/// raw-mode enable, alternate-screen enter, title set, terminal
/// construction, event poll, event read, or renderer commit.
///
/// # Panics
/// Does not panic. The `expect` call on
/// [`WidgetViewTui::read_state`] is unreachable because the scene
/// is constructed in this function and never moved.
pub fn run<V: WidgetViewTui<Renderer = TuiRenderer<CrosstermBackend<Stdout>>>>() -> io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::terminal::SetTitle(V::title()),
        crossterm::event::EnableMouseCapture,
    )?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout());
    let mut renderer = TuiRenderer::new(backend)?;

    // Construct the application's live state scene + read its
    // cached projection. The §5.15 `Scene::External` carries the
    // SCXML statechart; both the R51.111 keyboard path and the
    // R51.112 mouse path reach it via direct walk + InputRouter
    // hit-test against the most-recent paint scene.
    let external = V::create_external();
    let mut scene = Scene::External(ExternalNode::new(external).with_tag(V::tag()));
    let mut state = V::read_state(&scene);
    let mut intent_queue = IntentQueue::new();
    // R51.112 §5.41 — winit-free pointer router. The substrate's
    // `update_paint_scene` retains the latest visual layout so
    // mouse coords resolve hover targets across resize / state
    // changes. Mirrors `ShellCore`'s router slot in the Vello path.
    let mut router = InputRouter::new();

    let (mut cols, mut rows) = V::initial_size();

    // Initial paint. The returned paint scene seeds the router's
    // hit-test snapshot before the first mouse event reaches the
    // event loop — without this prime, a `Down(Left)` before any
    // `Moved` would see an empty hover map and miss the widget.
    let initial_paint = paint_frame::<V>(state, cols, rows, &mut renderer)?;
    router.update_paint_scene(initial_paint, &mut scene);

    // Event loop. `poll` timeout = 100ms balances responsiveness
    // (sub-frame for typical typing latency) against CPU wake
    // budget (10 polls/sec idle).
    let poll_timeout = Duration::from_millis(100);
    loop {
        if !crossterm::event::poll(poll_timeout)? {
            continue;
        }
        match crossterm::event::read()? {
            crossterm::event::Event::Key(key) => {
                // R51.111 §5.41 — only Press events drive dispatch.
                // Crossterm reports Release/Repeat on Windows + on
                // Unix when `REPORT_EVENT_TYPES` is enabled; mapping
                // those to the press path would double-fire every
                // keystroke. Mirrors `winit::event::ElementState`
                // `Pressed` filter in `pinion-shell::app`.
                if key.kind != crossterm::event::KeyEventKind::Press {
                    continue;
                }
                if key.code == crossterm::event::KeyCode::Esc {
                    // Shell-reserved exit key per §5.39 R51.53
                    // convention (Vello shell's `Escape → quit`
                    // mirrors here).
                    break;
                }
                // R51.111 §5.41 — bridge crossterm KeyEvent into
                // the abstract W3C key string and dispatch through
                // the substrate path the Vello shell uses.
                let Some(key_str) = crate::input::key_str_from_event(&key) else {
                    continue;
                };
                if dispatch_key::<V>(&mut scene, &key_str) {
                    drain_and_repaint::<V>(
                        &mut scene,
                        &mut state,
                        &mut intent_queue,
                        &mut router,
                        cols,
                        rows,
                        &mut renderer,
                    )?;
                }
            }
            crossterm::event::Event::Mouse(me) => {
                // R51.112 §5.41 — bridge crossterm cell coords into
                // the substrate's pixel-coord pointer router. The
                // SCXML statechart sees the same `PointerEnter` /
                // `PointerDown` / `PointerUp` events the Vello path
                // produces, so the `Pressed → Hover` click intent
                // arms identically.
                let (x, y) = crate::input::cell_to_pixel(me.column, me.row);
                let dispatched = dispatch_mouse(&mut router, &mut scene, me.kind, x, y);
                if dispatched {
                    drain_and_repaint::<V>(
                        &mut scene,
                        &mut state,
                        &mut intent_queue,
                        &mut router,
                        cols,
                        rows,
                        &mut renderer,
                    )?;
                }
            }
            crossterm::event::Event::Resize(new_cols, new_rows) => {
                cols = new_cols;
                rows = new_rows;
                let paint_scene = paint_frame::<V>(state, cols, rows, &mut renderer)?;
                router.update_paint_scene(paint_scene, &mut scene);
            }
            _ => {
                // Paste / focus events — ignored this round.
                // R51.113+ wires them as the substrate-incompleteness
                // -signal triggers surface.
            }
        }
    }

    Ok(())
}

/// R51.111 §5.41 — dispatch one W3C-named key through the binding's
/// `keybinding` → `apply_key` chain. Returns `true` if either hook
/// reported the key handled (the caller then drains intents +
/// refreshes cached state + repaints on visible change).
///
/// Mirrors `pinion_shell::ShellCore::handle_character_key` /
/// `handle_named_key`: typed enum route first (the SCXML
/// `invoke("send", Text(<event-name>))` path), raw key-string
/// fallback second (the `apply_key` escape hatch widgets use for
/// keys without a typed event variant).
fn dispatch_key<V: WidgetViewTui<Renderer = TuiRenderer<CrosstermBackend<Stdout>>>>(
    scene: &mut Scene,
    key_str: &str,
) -> bool {
    if let Some(event) = V::keybinding(key_str) {
        let name = V::event_name(event);
        return forward_event(scene, name);
    }
    // Focus is implicit at this cut — single-widget shell. The
    // substrate hands the binding's own tag through so widgets that
    // gate on `focused == Some(Self::tag())` (the Vello catalogue's
    // `apply_key` convention) recognise themselves as the activation
    // target.
    V::apply_key(scene, Some(V::tag()), key_str)
}

/// R51.111 §5.41 — route a typed event name through the state
/// scene's `Scene::External::invoke("send", Text(<name>))` path.
/// Returns `true` when the underlying `ExternalIntrospect::invoke`
/// reports success.
fn forward_event(scene: &mut Scene, event_name: &str) -> bool {
    let Scene::External(node) = scene else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    intro
        .invoke("send", IntrospectValue::Text(event_name.to_string()))
        .is_ok()
}

/// R51.112 §5.41 — bridge one `MouseEventKind` to the substrate's
/// [`InputRouter`]. Returns `true` when an event reached the router
/// (the caller then drains intents + refreshes state + repaints);
/// `false` for events the substrate intentionally absorbs (right /
/// middle button, scroll wheel — Tier-1 widget catalogue has no
/// semantics for them).
///
/// Mirrors the `winit::event::WindowEvent::CursorMoved` /
/// `MouseInput { state, button: Left }` arms in
/// `pinion_shell::app::AppShell::window_event`: cell-space coords
/// have already been converted to pixel-space via
/// [`crate::input::cell_to_pixel`] so the router treats both inputs
/// uniformly. `Down(Left)` runs a `cursor_moved` first so the hover
/// target reflects the press location before the press dispatches
/// (otherwise a click on a widget not yet hovered would miss).
fn dispatch_mouse(
    router: &mut InputRouter,
    scene: &mut Scene,
    kind: crossterm::event::MouseEventKind,
    x: f64,
    y: f64,
) -> bool {
    use crossterm::event::{MouseButton, MouseEventKind};
    match kind {
        MouseEventKind::Moved => {
            router.cursor_moved(PointerId::MOUSE, x, y, scene);
            true
        }
        MouseEventKind::Down(MouseButton::Left) => {
            // Sync the cursor first so `pointer_down` sees the
            // correct hover target. Vello shell's
            // `cursor_moved → mouse_pressed` ordering relies on the
            // same invariant.
            router.cursor_moved(PointerId::MOUSE, x, y, scene);
            router.pointer_down(PointerId::MOUSE, scene);
            true
        }
        MouseEventKind::Up(MouseButton::Left) => {
            router.pointer_up(PointerId::MOUSE, scene);
            true
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            // Drag = cursor move with the left button held. The
            // router's capture-aware branch forwards to the
            // captured widget's `External::pointer_move` when a
            // drag-aware widget holds the lock; free-mode drags
            // refresh the hover target like a plain `Moved`.
            router.cursor_moved(PointerId::MOUSE, x, y, scene);
            true
        }
        // Right / middle / wheel — no Tier-1 widget reacts. R51.113+
        // surfaces a substrate-incompleteness-signal once a widget
        // (context menu, scroll container) needs them.
        _ => false,
    }
}

/// R51.111 / R51.112 §5.41 — shared post-dispatch tail: drain §5.20
/// intents, refresh cached state, repaint on visible change, and
/// hand the new paint scene to the router for the next round's
/// hit-test.
///
/// Both the keyboard and mouse dispatch arms route through here so
/// the SCXML statechart's emit / refresh / paint pipeline is
/// identical regardless of input source — the §6.3 `dry_run` purity
/// invariant remains intact (no input-side state leaks into the
/// view-fn output) and the `tui: intent ...` / `tui: state ...`
/// stderr trace lines align across both paths.
fn drain_and_repaint<V: WidgetViewTui<Renderer = TuiRenderer<CrosstermBackend<Stdout>>>>(
    scene: &mut Scene,
    state: &mut V::State,
    intent_queue: &mut IntentQueue,
    router: &mut InputRouter,
    cols: u16,
    rows: u16,
    renderer: &mut TuiRenderer<CrosstermBackend<Stdout>>,
) -> io::Result<()> {
    walk_scene_and_drain(scene, intent_queue);
    for intent in intent_queue.drain() {
        eprintln!(
            "tui: intent {} payload={:?}",
            intent.tag_str(),
            intent.payload,
        );
    }
    let new_state = V::read_state(scene);
    if new_state != *state {
        eprintln!("tui: state {state:?} -> {new_state:?}");
        *state = new_state;
        let paint_scene = paint_frame::<V>(*state, cols, rows, renderer)?;
        router.update_paint_scene(paint_scene, scene);
    }
    Ok(())
}

/// Helper that runs the paint pipeline once for the given state +
/// terminal dimensions, commits via the renderer, then returns the
/// freshly-painted scene so the caller can hand it to
/// [`InputRouter::update_paint_scene`]. The R51.112 mouse path needs
/// the paint scene retention (hover targets resolve against the
/// most-recent visual layout); the keyboard path consumes it on
/// state change so a Tab focus traversal sees fresh hit-test data.
fn paint_frame<V: WidgetViewTui<Renderer = TuiRenderer<CrosstermBackend<Stdout>>>>(
    state: V::State,
    cols: u16,
    rows: u16,
    renderer: &mut TuiRenderer<CrosstermBackend<Stdout>>,
) -> io::Result<Scene> {
    // Build the paint scene via the binding's view-fn, walk it
    // into the ratatui buffer, then commit via the renderer.
    // Returning the paint scene keeps the substrate's R51.112
    // InputRouter hit-test snapshot in sync with the visible state.
    let frame = Frame::new();
    let paint_scene = V::view(state, &frame);
    let mut buf = Buffer::empty(Rect::new(0, 0, cols, rows));
    crate::paint::to_buffer(&paint_scene, &mut buf);
    renderer.render(&buf, TuiContext::default())?;
    Ok(paint_scene)
}
