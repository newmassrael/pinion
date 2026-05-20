//! R51.110.2 / R51.111 §5.41 — TUI shell entry point (crossterm event
//! loop).
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
//! substrate-incompleteness-signal trigger (R51.112+).
//!
//! ## What's still deferred to R51.112+
//!
//! - **Mouse dispatch**: `crossterm::event::MouseEvent` → cell-coord
//!   hit-test → `InputRouter::pointer_*`. The cell-native coord axis
//!   lands alongside (the GUI shell uses pixel coords from winit;
//!   the TUI shell needs the cell ↔ pixel inverse via
//!   `PIXEL_PER_CELL_*`).
//! - **`FocusManager` + a11y**: defer until the second focusable
//!   TUI binding lands.
//! - **`Backend::Tui` axis on `External::backends`**: today the
//!   binding declares `Backend::Gui` because no dedicated TUI flag
//!   exists; the §5.15 backend taxonomy round lifts it.
//!
//! These deferrals stay textbook substrate-incompleteness-signal
//! ([[substrate-incompleteness-signal]]) — each shell-side path
//! waits for its concrete first consumer.

use std::io::{self, Stdout, stdout};
use std::time::Duration;

use pinion_core::external::IntrospectValue;
use pinion_core::renderer::WidgetRenderer;
use pinion_core::scene::ExternalNode;
use pinion_core::Scene;
use pinion_runtime::intent_queue::{walk_scene_and_drain, IntentQueue};
use ratatui::backend::CrosstermBackend;

use crate::{TuiContext, TuiRenderer, WidgetViewTui, render_one_frame};

/// R51.110.2 §5.41 — RAII guard restoring the terminal on drop.
///
/// `Drop` runs even on panic, so the user's terminal is never left
/// in a half-broken raw-mode + alternate-screen state. Mirrors the
/// `ratatui::Terminal::restore` pattern from the ratatui book
/// (chapter 2 / canonical idiom).
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Silently swallow errors — at drop time the process is
        // either exiting or panicking; surfacing IO errors here
        // would clobber the actual error the user cares about.
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(stdout(), crossterm::terminal::LeaveAlternateScreen);
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
    )?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout());
    let mut renderer = TuiRenderer::new(backend)?;

    // Construct the application's live state scene + read its
    // cached projection. The §5.15 `Scene::External` carries the
    // SCXML statechart; the R51.111 input dispatch path reaches it
    // via direct walk on the state scene (single-widget TUI shell
    // today; multi-widget routing lands once the second binding
    // surfaces the trigger).
    let external = V::create_external();
    let mut scene = Scene::External(ExternalNode::new(external).with_tag(V::tag()));
    let mut state = V::read_state(&scene);
    let mut intent_queue = IntentQueue::new();

    let (mut cols, mut rows) = V::initial_size();

    // Initial paint.
    paint_frame::<V>(state, cols, rows, &mut renderer)?;

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
                let handled = dispatch_key::<V>(&mut scene, &key_str);
                if handled {
                    walk_scene_and_drain(&mut scene, &mut intent_queue);
                    for intent in intent_queue.drain() {
                        eprintln!(
                            "tui: intent {} payload={:?}",
                            intent.tag_str(),
                            intent.payload,
                        );
                    }
                    let new_state = V::read_state(&scene);
                    if new_state != state {
                        eprintln!("tui: state {state:?} -> {new_state:?}");
                        state = new_state;
                        paint_frame::<V>(state, cols, rows, &mut renderer)?;
                    }
                }
            }
            crossterm::event::Event::Resize(new_cols, new_rows) => {
                cols = new_cols;
                rows = new_rows;
                paint_frame::<V>(state, cols, rows, &mut renderer)?;
            }
            _ => {
                // Mouse / paste / focus events — ignored this
                // round. R51.112+ wires mouse to a future TUI
                // pointer router (cell-coord hit-test).
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

/// Helper that runs the paint pipeline once for the given state +
/// terminal dimensions, then commits via the renderer. Extracted
/// from [`run`] so the initial paint + resize-driven repaint share
/// the same call site.
fn paint_frame<V: WidgetViewTui<Renderer = TuiRenderer<CrosstermBackend<Stdout>>>>(
    state: V::State,
    cols: u16,
    rows: u16,
    renderer: &mut TuiRenderer<CrosstermBackend<Stdout>>,
) -> io::Result<()> {
    // Build the painted buffer from the cached state. `render_one_frame`
    // also constructs the fresh `Frame::new()` ZST that the view-fn
    // contract expects. R51.111+ extends this point with focus ring
    // overlay + paint scene retention for the InputRouter hit-test.
    let buf = render_one_frame::<V>(state, cols, rows);
    renderer.render(&buf, TuiContext::default())
}
