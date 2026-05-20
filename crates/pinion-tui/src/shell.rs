//! R51.110.2 §5.41 — TUI shell entry point (crossterm event loop).
//!
//! [`run`] is the TUI sibling of `pinion_shell::run::<V>()`. It owns
//! the crossterm raw-mode lifecycle (enable on entry, disable via
//! RAII guard on exit), enters the alternate screen so the
//! application's paint output does not clobber the user's existing
//! terminal scrollback, sets the OSC window title, constructs the
//! `TuiRenderer` against the live `CrosstermBackend<Stdout>`, then
//! drives the paint + event loop until `Esc` is pressed (the
//! shell-reserved exit key, mirroring the Vello shell's `Escape →
//! window close` convention).
//!
//! ## R51.110.2 minimal cut
//!
//! The first dogfood run covers the substrate end-to-end:
//!
//! - Raw mode + alternate screen lifecycle.
//! - `crossterm::event::Event` polling with a 100ms timeout (the
//!   industry-standard balance between input responsiveness and CPU
//!   wake budget).
//! - Resize redraw via [`crate::render_one_frame`] +
//!   `WidgetRenderer::render`.
//! - `Esc` → graceful exit (cleanup runs via RAII regardless of
//!   panic / normal return).
//!
//! ## What's deferred to R51.111+
//!
//! - **Input dispatch**: key / mouse events do not yet reach the
//!   widget's SCXML statechart. The substrate path
//!   (`InputRouter` → `Scene::External::invoke`) lands once the
//!   first interactive TUI binding (second hello-* TUI example, or
//!   the first button click handler) surfaces the seam.
//! - **State change → repaint**: the loop currently renders the
//!   same `state` per frame. Cached-state diff + targeted repaint
//!   (mirroring `pinion_shell::AppShell::redraw_request` flow)
//!   lands with the input dispatch carry.
//! - **Focus management + a11y**: defer to R51.111+ once
//!   `WidgetViewTui::focusable_tags` lands.
//!
//! These deferrals are textbook substrate-incompleteness-signal
//! ([[substrate-incompleteness-signal]]) — each shell-side path
//! waits for its concrete first consumer.

use std::io::{self, Stdout, stdout};
use std::time::Duration;

use pinion_core::renderer::WidgetRenderer;
use pinion_core::scene::ExternalNode;
use pinion_core::Scene;
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
    // SCXML statechart; the input dispatch path (R51.111+) reaches
    // it via `tag()` hit-test.
    let external = V::create_external();
    let scene = Scene::External(ExternalNode::new(external).with_tag(V::tag()));
    let state = V::read_state(&scene);

    let (mut cols, mut rows) = V::initial_size();

    // Initial paint.
    paint_frame::<V>(state, cols, rows, &mut renderer)?;

    // Event loop. `poll` timeout = 100ms balances responsiveness
    // (sub-frame for typical typing latency) against CPU wake
    // budget (10 polls/sec idle). Future R51.111+ tightens this
    // when input dispatch + state diff arrive.
    let poll_timeout = Duration::from_millis(100);
    loop {
        if crossterm::event::poll(poll_timeout)? {
            match crossterm::event::read()? {
                crossterm::event::Event::Key(key) => {
                    if key.code == crossterm::event::KeyCode::Esc {
                        // Shell-reserved exit key per §5.39 R51.53
                        // convention (Vello shell's `Escape → quit`
                        // mirrors here).
                        break;
                    }
                    // R51.111+ — input dispatch via the substrate's
                    // `InputRouter` + `Scene::External::invoke`
                    // path. Today the loop just consumes the key
                    // so it doesn't accumulate in the crossterm
                    // queue.
                }
                crossterm::event::Event::Resize(new_cols, new_rows) => {
                    cols = new_cols;
                    rows = new_rows;
                    paint_frame::<V>(state, cols, rows, &mut renderer)?;
                }
                _ => {
                    // Mouse / paste / focus events — ignored this
                    // round. R51.111+ wires them to InputRouter.
                }
            }
        }
    }

    Ok(())
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
