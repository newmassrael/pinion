//! R51.110.2 / R51.111 / R51.112 / R51.117 §5.41 — TUI shell entry
//! point (crossterm event loop).
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

use std::env;
use std::fs::OpenOptions;
use std::io::{self, Stdout, stdout};
use std::time::Duration;

use pinion_core::renderer::WidgetRenderer;
use pinion_core::Scene;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::{ShellCoreTui, TuiContext, TuiRenderer, WidgetViewTui};

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

    // R51.117 §5.41 — the dispatch substrate ([`ShellCoreTui<V>`])
    // owns the renderer-agnostic state (state scene, cached state,
    // router, intent queue). This surface only sequences calls +
    // commits the painted buffer through the live renderer.
    //
    // R51.120 §5.41 — substrate is silent by default (no `stderr`
    // writes under `enable_raw_mode()` + `EnterAlternateScreen`, see
    // `ShellCoreTui::log_sink` doc for the rationale). Setting
    // `PINION_TUI_LOG=<path>` in the environment opens the named
    // file with `append(true)` and routes intent / state trace
    // lines there. An unset / empty value, or an open error, leaves
    // the substrate silent — the live UI must not panic on a
    // missing log dir.
    let mut core = ShellCoreTui::<V>::new();
    if let Some(path) = env::var_os("PINION_TUI_LOG")
        && !path.is_empty()
        && let Ok(file) = OpenOptions::new().create(true).append(true).open(&path)
    {
        core.set_log_sink(Box::new(file));
    }

    let (mut cols, mut rows) = V::initial_size();

    // Initial paint. The returned paint scene seeds the router's
    // hit-test snapshot before the first mouse event reaches the
    // event loop — without this prime, a `Down(Left)` before any
    // `Moved` would see an empty hover map and miss the widget.
    let initial_paint = commit_paint::<V>(&core, cols, rows, &mut renderer)?;
    core.update_paint_scene(initial_paint);

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
                if core.dispatch_key(&key_str) && core.refresh_state() {
                    let paint_scene = commit_paint::<V>(&core, cols, rows, &mut renderer)?;
                    core.update_paint_scene(paint_scene);
                }
            }
            crossterm::event::Event::Mouse(me) => {
                // R51.112 §5.41 — bridge crossterm cell coords into
                // the substrate's pixel-coord pointer router via
                // [`crate::input::cell_to_pixel`]. The SCXML
                // statechart sees the same `PointerEnter` /
                // `PointerDown` / `PointerUp` events the Vello path
                // produces, so the `Pressed → Hover` click intent
                // arms identically.
                let (x, y) = crate::input::cell_to_pixel(me.column, me.row);
                if dispatch_mouse(&mut core, me.kind, x, y) && core.refresh_state() {
                    let paint_scene = commit_paint::<V>(&core, cols, rows, &mut renderer)?;
                    core.update_paint_scene(paint_scene);
                }
            }
            crossterm::event::Event::Resize(new_cols, new_rows) => {
                cols = new_cols;
                rows = new_rows;
                let paint_scene = commit_paint::<V>(&core, cols, rows, &mut renderer)?;
                core.update_paint_scene(paint_scene);
            }
            _ => {
                // Paste / focus events — ignored this round.
                // R51.118+ wires them as the substrate-incompleteness
                // -signal triggers surface.
            }
        }
    }

    Ok(())
}

/// R51.112 / R51.117 §5.41 — bridge one `MouseEventKind` into the
/// substrate's [`ShellCoreTui<V>`] dispatch methods. Returns `true`
/// when an event was routed (the caller then refreshes state +
/// repaints); `false` for events the substrate intentionally
/// absorbs (right / middle button, scroll wheel — Tier-1 widget
/// catalogue has no semantics for them).
///
/// `Down(Left)` runs `cursor_moved` first so the substrate's hover
/// target reflects the press location before `pointer_down`
/// dispatches (otherwise a click on a widget not yet hovered would
/// miss). `Drag(Left)` reuses `cursor_moved` — the substrate's
/// capture-aware branch handles drag-aware widgets internally.
fn dispatch_mouse<V: WidgetViewTui<Renderer = TuiRenderer<CrosstermBackend<Stdout>>>>(
    core: &mut ShellCoreTui<V>,
    kind: crossterm::event::MouseEventKind,
    x: f64,
    y: f64,
) -> bool {
    use crossterm::event::{MouseButton, MouseEventKind};
    match kind {
        // Plain move and left-button drag both forward a cursor
        // position to the router — drag-aware capture is handled
        // inside the router so the surface arm collapses.
        MouseEventKind::Moved | MouseEventKind::Drag(MouseButton::Left) => {
            core.cursor_moved(x, y);
            true
        }
        MouseEventKind::Down(MouseButton::Left) => {
            // Sync the cursor first so `pointer_down` sees the
            // correct hover target. Vello shell's
            // `cursor_moved → mouse_pressed` ordering relies on the
            // same invariant.
            core.cursor_moved(x, y);
            core.pointer_down();
            true
        }
        MouseEventKind::Up(MouseButton::Left) => {
            core.pointer_up();
            true
        }
        // Right / middle / wheel — no Tier-1 widget reacts. R51.118+
        // surfaces a substrate-incompleteness-signal once a widget
        // (context menu, scroll container) needs them.
        _ => false,
    }
}

/// R51.117 §5.41 — render one frame of the substrate's current
/// cached state into the live terminal + return the painted scene
/// so the caller can hand it to
/// [`ShellCoreTui::update_paint_scene`].
///
/// Pure surface helper: the substrate's `compute_paint_scene` is
/// the only thing that touches `V::view`; this function adds the
/// ratatui buffer allocation + the `WidgetRenderer::render` commit.
/// Returning the paint scene keeps the substrate's R51.112
/// `InputRouter` hit-test snapshot in sync with the visible state.
fn commit_paint<V: WidgetViewTui<Renderer = TuiRenderer<CrosstermBackend<Stdout>>>>(
    core: &ShellCoreTui<V>,
    cols: u16,
    rows: u16,
    renderer: &mut TuiRenderer<CrosstermBackend<Stdout>>,
) -> io::Result<Scene> {
    let paint_scene = core.compute_paint_scene();
    let mut buf = Buffer::empty(Rect::new(0, 0, cols, rows));
    crate::paint::to_buffer(&paint_scene, &mut buf);
    renderer.render(&buf, TuiContext::default())?;
    Ok(paint_scene)
}
