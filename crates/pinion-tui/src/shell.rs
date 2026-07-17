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
//!    `CoreShell::forward` (the R884 `send_to_primary` send home).
//! 3. Otherwise `V::apply_key(&mut scene, Some(V::tag()), key_str)`
//!    — widgets walk to their `Scene::External` and call
//!    `intervene` / `invoke` themselves (`Slider` arrow-key value
//!    mutation, `Button` `Space` / `Enter` keyboard activation).
//! 4. On handled input, `walk_scene_and_drain` pulls §5.20 intents
//!    out of the SCXML widget; cached state refresh via
//!    `V::read_state(&scene)` triggers a repaint when the visible
//!    state changes (mirrors the state-refresh step in
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
//!    [`crate::input::cell_to_pixel`] maps them through the R968 §5.41
//!    `CellMetric` (same inverse the paint walker uses) so the
//!    substrate's pixel-coord `Scene::Container.rect` hit-tests align
//!    with the visible cells.
//! 3. After every paint, [`InputRouter::update_paint_scene`](pinion_runtime::InputRouter::update_paint_scene) retains
//!    a fresh paint-scene snapshot so the next `MouseEvent` resolves
//!    hover targets against the current visual state (mirrors
//!    `ShellCore::finalize_frame` post-render handoff).
//! 4. `MouseEventKind::Down(Left)` → cursor sync + `pointer_down`;
//!    `Up(Left)` → `pointer_up`; `Moved` / `Drag(Left)` →
//!    `cursor_moved`; `Down(Right)` → cursor sync +
//!    `secondary_click` (R887, the context-menu press); scroll maps
//!    per-axis to `wheel`. Right-release / middle stay absorbed
//!    (terminal emulators own middle-paste at the terminal tier).
//!
//! The §5.20 intent drain + cached-state refresh + repaint cycle
//! collapses into `drain_and_repaint` — the keyboard path and the
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
//! - **Cell-native coord substrate**: the typed R968 §5.41
//!   `CellMetric` (default 8×16) replaced the `PIXEL_PER_CELL_*`
//!   placeholder. R994 landed the `Scene::TextGrid` TUI arm, which maps
//!   each grid cell 1:1 onto a character cell — a node's per-node *pixel*
//!   metric sizes Vello glyphs but is irrelevant to a character buffer, so
//!   no per-node-metric work remains on this path.
//!
//! These deferrals stay textbook substrate-incompleteness-signal
//! ([[substrate-incompleteness-signal]]) — each shell-side path
//! waits for its concrete first consumer.

use std::env;
use std::fs::OpenOptions;
use std::io::{self, BufRead, Stdout, Write, stderr, stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use pinion_core::event::WheelDelta;
use pinion_core::renderer::WidgetRenderer;
use pinion_core::{Intent, Scene};
use pinion_rpc::{RpcFrame, RpcReply};
use pinion_runtime::{CommandExecutor, HandlerRegistry};
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::executor::build_executor_and_sink;
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

/// R51.111 idle poll timeout — sub-frame responsive for typical
/// typing latency, ~10 polls/sec while the substrate is at rest.
const IDLE_POLL_MS: u64 = 100;

/// R51.148 §5.28 — short poll timeout while any animation is moving.
/// One 60Hz frame keeps the spring transition smooth without
/// pegging a core; the substrate falls back to [`IDLE_POLL_MS`]
/// once every animation settles under [`REST_EPSILON`].
const ACTIVE_POLL_MS: u64 = 16;

/// R51.148 §5.28 — spring settlement epsilon forwarded to
/// [`pinion_core::reactive::Owner::any_animation_active`]. Matches
/// the substrate-level
/// [`pinion_core::DEFAULT_REST_EPSILON`] (R601 lift) so the shell's
/// "stop painting" threshold lines up with the spring solver's own
/// rest criterion.
const REST_EPSILON: f32 = pinion_core::DEFAULT_REST_EPSILON;

/// R51.110.2 §5.41 — run the TUI binding `V` end-to-end against the
/// live terminal.
///
/// Enables raw mode + alternate screen, sets the OSC window title,
/// constructs a [`TuiRenderer`] against
/// `CrosstermBackend<Stdout>`, paints the initial state, then loops
/// reading crossterm events until `Esc` is pressed. The terminal is
/// always restored on exit (RAII guard).
///
/// R51.160 §5.23 — no [`CommandExecutor`] is installed by this entry
/// point; pending [`pinion_core::Command`] queues stay parked on the
/// owner side and never fire. Use [`run_with_handlers`] to register
/// async [`Handler`](pinion_runtime::Handler)s and bind a tokio
/// runtime + intent-arrival mpsc channel.
///
/// # Errors
/// Propagates `std::io::Error` from any crossterm / ratatui call:
/// raw-mode enable, alternate-screen enter, title set, terminal
/// construction, event poll, event read, or renderer commit.
///
/// # Panics
/// Does not panic. The `expect` call on
/// `WidgetViewTui::read_state` is unreachable because the scene
/// is constructed in this function and never moved.
pub fn run<V: WidgetViewTui<Renderer = TuiRenderer<CrosstermBackend<Stdout>>>>() -> io::Result<()> {
    run_impl::<V>(None)
}

/// R51.160 §5.23 — variant of [`run`] that installs a
/// [`CommandExecutor`] at boot so
/// pending [`pinion_core::Command`]s queued by reducer fallout or
/// SCXML transitions reach their registered
/// [`Handler`](pinion_runtime::Handler)s asynchronously.
///
/// Composes:
///
/// - A tokio multi-thread [`TokioExecutor`](crate::TokioExecutor) (1
///   worker thread, `enable_all`) backing
///   [`Executor::spawn`](pinion_runtime::Executor).
/// - A [`MpscIntentSink`](crate::MpscIntentSink) wrapping the
///   [`mpsc::Sender<Intent>`] half of a channel; the shell's event
///   loop calls `try_recv` between every crossterm `poll` tick to
///   drain arrivals and route them through
///   [`ShellCoreTui::dispatch_intent`](crate::ShellCoreTui).
/// - The supplied `registry` of [`Handler`](pinion_runtime::Handler)
///   impls keyed by [`pinion_core::Command::kind_str`].
///
/// # Errors
/// Propagates the same set as [`run`] plus
/// [`crate::TokioExecutor::new`] failure.
///
/// # Panics
/// Same as [`run`].
pub fn run_with_handlers<V: WidgetViewTui<Renderer = TuiRenderer<CrosstermBackend<Stdout>>>>(
    registry: HandlerRegistry,
) -> io::Result<()> {
    let (executor, sink, rx) = build_executor_and_sink()?;
    let cmd_exec = Arc::new(CommandExecutor::new(registry, executor, sink));
    run_impl::<V>(Some((cmd_exec, rx)))
}

/// R51.160 §5.23 — shared event-loop body for [`run`] and
/// [`run_with_handlers`]. When `commands` is `Some`, the
/// [`CommandExecutor`] is injected into the substrate and the
/// matching [`mpsc::Receiver`] is drained on every event-loop
/// iteration so intents arriving from completed [`Handler`](pinion_runtime::Handler)
/// futures reach the SCXML `send` channel without waiting for an
/// input event.
fn run_impl<V: WidgetViewTui<Renderer = TuiRenderer<CrosstermBackend<Stdout>>>>(
    commands: Option<(Arc<CommandExecutor>, mpsc::Receiver<Intent>)>,
) -> io::Result<()> {
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
    // R1363 §5.55 — the app-lifecycle seam, seeded before the binding factories
    // resolve `use_quit_sink()` (the first-write-wins window).
    let quit_flag = Arc::new(AtomicBool::new(false));
    let seed_flag = Arc::clone(&quit_flag);
    let mut core = ShellCoreTui::<V>::new_with_seed(move |owner| {
        owner.provide_quit_sink(Arc::new(TuiQuitSink(seed_flag)));
    });
    if let Some(path) = env::var_os("PINION_TUI_LOG")
        && !path.is_empty()
        && let Ok(file) = OpenOptions::new().create(true).append(true).open(&path)
    {
        core.set_log_sink(Box::new(file));
    }

    // R51.160 §5.23 — split the optional commands tuple into the
    // executor handle (injected into the substrate) and the intent
    // receiver (drained in the loop body).
    let intent_rx: Option<mpsc::Receiver<Intent>> = if let Some((cmd_exec, rx)) = commands {
        let _ = core.set_command_executor(cmd_exec);
        Some(rx)
    } else {
        None
    };

    // R670 §5.41 §5.40 — JSON-RPC stdin ingress, mirror of
    // `pinion_shell::spawn_stdin_rpc_reader`. A background thread
    // reads `BufRead::lines` off stdin and forwards each non-blank
    // frame through an `mpsc::Sender<RpcFrame>`; the event loop drains
    // the matching `mpsc::Receiver<RpcFrame>` on every tick with
    // `try_recv` so AI-injected RPC frames reach the substrate
    // alongside live crossterm events. The §2 #6 GUI/TUI dual
    // invariant requires identical RPC ingress shape on both
    // backends — see [`crate::ShellCoreTui::dispatch_rpc`] for the
    // dispatch path the drained frames feed into.
    //
    // R-PR47 §5.7 — the channel now carries `pinion_rpc::RpcFrame`
    // (request + reply sink), the SAME winit-free seam the GUI backend
    // uses, instead of a bare `String`. The per-backend response wire
    // (stderr here, stdout on the GUI) is no longer hard-coded in the
    // drain: it is chosen per frame by the reply the producer attaches.
    // The built-in stdin reader attaches a stderr reply
    // ([`stderr_reply`]); a future injected transport would attach its
    // own, with no change to the drain.
    let (rpc_tx, rpc_rx) = mpsc::channel::<RpcFrame>();
    spawn_stdin_rpc_reader_tui(rpc_tx);

    let (mut cols, mut rows) = V::initial_size();

    // Initial paint. The returned paint scene seeds the router's
    // hit-test snapshot before the first mouse event reaches the
    // event loop — without this prime, a `Down(Left)` before any
    // `Moved` would see an empty hover map and miss the widget.
    commit_and_finalize::<V>(&mut core, cols, rows, &mut renderer)?;

    // Event loop. See module-level [`IDLE_POLL_MS`] / [`ACTIVE_POLL_MS`]
    // / [`REST_EPSILON`] for the R51.148 §5.28 adaptive-poll rationale.
    loop {
        // R1363 §5.55 §2 #6 — did anything ask the APP to end since the last
        // turn? A binding's `QuitSink` (its own logic, or a producer thread —
        // sprag's poll thread on a dead daemon socket) set the flag; the veto is
        // run HERE, on the UI thread, never on the producer's.
        //
        // Same arm as this shell's `Escape`, and the same `WidgetCore` veto the
        // Vello shell's `request_quit` uses — one vocabulary, two dispatch
        // paths. A binding that handles the quit clears the flag and lives on.
        if quit_flag.swap(false, Ordering::SeqCst) && !core.root_owner().run(V::app_quit_requested)
        {
            break;
        }
        // R51.160 §5.23 — drain any Intents that arrived from
        // completed Command futures since the previous loop turn.
        // `try_recv` is non-blocking; if the channel is empty we
        // skip the inner loop entirely. Each dispatched intent
        // routes through the SCXML `send` channel; on visible state
        // change we commit a fresh paint before the next event poll.
        if let Some(rx) = &intent_rx
            && drain_intents_into_substrate(&mut core, rx)
        {
            commit_and_finalize::<V>(&mut core, cols, rows, &mut renderer)?;
        }
        // R670 §5.41 §5.40 — drain any JSON-RPC frames the stdin
        // reader thread has buffered since the previous tick. See
        // [`drain_rpc_into_substrate`] for the stderr-response
        // rationale (alternate-screen + raw-mode terminal owns
        // stdout, so the response wire lives on stderr per the
        // canonical Unix diagnostic-stream convention).
        if drain_rpc_into_substrate(&mut core, &rpc_rx) {
            commit_and_finalize::<V>(&mut core, cols, rows, &mut renderer)?;
        }

        let poll_timeout = if core.any_animation_active(REST_EPSILON) {
            Duration::from_millis(ACTIVE_POLL_MS)
        } else {
            Duration::from_millis(IDLE_POLL_MS)
        };
        if !crossterm::event::poll(poll_timeout)? {
            // R51.148 §5.28 — timeout without an input event. If an
            // animation is still moving, commit another paint so the
            // user observes the spring transition; otherwise stay
            // idle until the next event arrives.
            if core.any_animation_active(REST_EPSILON) {
                commit_and_finalize::<V>(&mut core, cols, rows, &mut renderer)?;
            }
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
                    // R693 §5.39 — while a modal focus trap is active,
                    // Esc dismisses the modal (routed to the binding's
                    // `apply_key` as the W3C "Escape" name) rather than
                    // quitting the alternate screen. Mirrors the Vello
                    // shell's modal Escape routing so the dual backends
                    // honour the WAI-ARIA "Escape closes the dialog,
                    // not the app" contract identically.
                    if core.focus_is_modal() {
                        let modifiers = crate::input::modifiers_from_crossterm(key.modifiers);
                        if core.dispatch_key("Escape", modifiers) {
                            commit_and_finalize::<V>(&mut core, cols, rows, &mut renderer)?;
                        }
                        continue;
                    }
                    // Shell-reserved exit key per §5.39 R51.53
                    // convention (Vello shell's `Escape → quit`
                    // mirrors here).
                    //
                    // R1363 §5.55 — through the APP veto now. Pre-R1363 this was
                    // a bare `break` consulting no binding hook — the same veto
                    // bypass the Vello shell had hard-coded in its own Escape
                    // arm, arrived at independently by both backends for want of
                    // a Quit verb. `app_quit_requested` is on `WidgetCore`, so
                    // this terminal path and the GUI path share ONE veto.
                    if core.root_owner().run(V::app_quit_requested) {
                        // The binding handled it (raised a confirm modal);
                        // repaint so its answer is on screen.
                        commit_and_finalize::<V>(&mut core, cols, rows, &mut renderer)?;
                        continue;
                    }
                    break;
                }
                // R51.111 §5.41 — bridge crossterm KeyEvent into
                // the abstract W3C key string and dispatch through
                // the substrate path the Vello shell uses.
                let Some(key_str) = crate::input::key_str_from_event(&key) else {
                    continue;
                };
                // R56.1.f.0 §5.13 — forward the W3C modifier surface
                // (`shiftKey` / `ctrlKey` / `altKey` / `metaKey`)
                // alongside the key string so widgets such as
                // `TextField` (R56.1.f) can branch on Shift+Arrow
                // selection extension. The conversion drops crossterm
                // platform-specific bits (`HYPER`) onto Meta per the
                // R51.108 §5.41 winit-mirror surface.
                let modifiers = crate::input::modifiers_from_crossterm(key.modifiers);
                // R51.124 §5.41 — `dispatch_key` returns `true`
                // when the cached state changed (auto-tail
                // collapsed the pre-R51.124 explicit
                // `refresh_state` call); the surface repaints on
                // `true` so the new SCXML projection lands on
                // screen.
                if core.dispatch_key(&key_str, modifiers) {
                    commit_and_finalize::<V>(&mut core, cols, rows, &mut renderer)?;
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
                // R51.124 §5.41 — `dispatch_mouse` returns `true`
                // when the underlying [`ShellCoreTui::cursor_moved`]
                // / `pointer_down` / `pointer_up` call reported a
                // visible state transition; the surface repaints
                // on `true`.
                if dispatch_mouse(&mut core, me.kind, x, y) {
                    commit_and_finalize::<V>(&mut core, cols, rows, &mut renderer)?;
                }
            }
            crossterm::event::Event::Resize(new_cols, new_rows) => {
                cols = new_cols;
                rows = new_rows;
                commit_and_finalize::<V>(&mut core, cols, rows, &mut renderer)?;
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
/// absorbs (right-button release / middle button, scroll handled
/// per-axis below).
///
/// `Down(Left)` runs `cursor_moved` first so the substrate's hover
/// target reflects the press location before `pointer_down`
/// dispatches (otherwise a click on a widget not yet hovered would
/// miss). `Drag(Left)` reuses `cursor_moved` — the substrate's
/// capture-aware branch handles drag-aware widgets internally.
///
/// R888.1 — generic over any [`WidgetViewTui`] (the body only calls
/// `ShellCoreTui` dispatch methods; the previous
/// `TuiRenderer<CrosstermBackend<Stdout>>` bound was an accident of
/// the caller's type and made every arm untestable off a live
/// terminal — the R887 native right-press arm shipped with only a
/// hand-mirrored test because of it).
fn dispatch_mouse<V: WidgetViewTui>(
    core: &mut ShellCoreTui<V>,
    kind: crossterm::event::MouseEventKind,
    x: f64,
    y: f64,
) -> bool {
    use crossterm::event::{MouseButton, MouseEventKind};
    // R51.124 §5.41 — every `core.X` call returns the
    // state-changed bool directly; the surface needs to repaint
    // when ANY of the dispatch arms transitioned the cached state,
    // so `|` (not `||`) preserves both observations for the
    // multi-step `Down(Left)` arm.
    match kind {
        // Plain move and left-button drag both forward a cursor
        // position to the router — drag-aware capture is handled
        // inside the router so the surface arm collapses.
        MouseEventKind::Moved | MouseEventKind::Drag(MouseButton::Left) => core.cursor_moved(x, y),
        MouseEventKind::Down(MouseButton::Left) => {
            // Sync the cursor first so `pointer_down` sees the
            // correct hover target. Vello shell's
            // `cursor_moved → mouse_pressed` ordering relies on the
            // same invariant.
            let cursor_change = core.cursor_moved(x, y);
            let down_change = core.pointer_down();
            cursor_change | down_change
        }
        MouseEventKind::Up(MouseButton::Left) => core.pointer_up(),
        // R887 §5.49 §5.53 — secondary-button press: the context-menu
        // arc (the R51.118 substrate-incompleteness signal fired when
        // the R772 context menu landed). Cursor sync precedes the
        // press-edge one-shot so `apply_secondary_click` anchors at
        // the just-reported cell — the same ordering invariant as the
        // `Down(Left)` arm. Press-edge only (W3C `contextmenu`); the
        // matching `Up(Right)` stays absorbed below.
        MouseEventKind::Down(MouseButton::Right) => {
            let cursor_change = core.cursor_moved(x, y);
            let click_change = core.secondary_click();
            cursor_change | click_change
        }
        // (R51.186 §5.45 R55.C.2) crossterm wheel events. The cursor
        // sync precedes the wheel dispatch so the substrate's
        // `InputRouter` resolves the deepest `Scene::Scroll` under
        // the just-reported `(x, y)` cell-coord (the wheel
        // dispatches against the widget the cursor is currently
        // *over*, which matches Vello / W3C semantics). Each
        // crossterm scroll variant maps to one notched `Lines`
        // delta on the matching axis; `LINE_HEIGHT_PX` in the
        // substrate scales to a single content-pixel offset (cell-
        // coord granularity makes Pixels-mode reporting impossible
        // from a terminal anyway).
        MouseEventKind::ScrollUp => {
            let cursor_change = core.cursor_moved(x, y);
            let wheel_change = core.wheel(WheelDelta::Lines { dx: 0.0, dy: -1.0 });
            cursor_change | wheel_change
        }
        MouseEventKind::ScrollDown => {
            let cursor_change = core.cursor_moved(x, y);
            let wheel_change = core.wheel(WheelDelta::Lines { dx: 0.0, dy: 1.0 });
            cursor_change | wheel_change
        }
        MouseEventKind::ScrollLeft => {
            let cursor_change = core.cursor_moved(x, y);
            let wheel_change = core.wheel(WheelDelta::Lines { dx: -1.0, dy: 0.0 });
            cursor_change | wheel_change
        }
        MouseEventKind::ScrollRight => {
            let cursor_change = core.cursor_moved(x, y);
            let wheel_change = core.wheel(WheelDelta::Lines { dx: 1.0, dy: 0.0 });
            cursor_change | wheel_change
        }
        // Right release / middle — right-press is handled above
        // (R887); middle stays absorbed (terminal emulators own
        // middle-paste at the terminal tier — §2 #6 divergence
        // carry, pre-existing for the whole TUI paste axis).
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
    let paint_scene = core.compute_paint_scene(cols, rows);
    let mut buf = Buffer::empty(Rect::new(0, 0, cols, rows));
    crate::paint::to_buffer(&paint_scene, &mut buf);
    renderer.render(&buf, TuiContext::default())?;
    Ok(paint_scene)
}

/// R670 §5.41 — paint commit + router hand-off in one step, so an
/// RPC `scene/layout {viewport: null}` on the next dispatch tick
/// sees the post-paint geometry (R890: the stored router scene IS
/// the layout source — projected on demand inside `dispatch_rpc`;
/// the per-commit `finalize_paint_snapshot` mirror is retired).
/// Collapses the "commit / handoff" sequence event-loop arms repeat
/// at every redraw site into one helper call, keeping `run_impl`
/// under the workspace `clippy::too_many_lines` ceiling.
fn commit_and_finalize<V: WidgetViewTui<Renderer = TuiRenderer<CrosstermBackend<Stdout>>>>(
    core: &mut ShellCoreTui<V>,
    cols: u16,
    rows: u16,
    renderer: &mut TuiRenderer<CrosstermBackend<Stdout>>,
) -> io::Result<()> {
    let paint_scene = commit_paint::<V>(core, cols, rows, renderer)?;
    core.update_paint_scene(paint_scene);
    Ok(())
}

/// R51.160 §5.23 — drain every [`Intent`] the
/// [`CommandExecutor`] worker thread
/// has buffered since the previous tick. Returns `true` when any
/// drained intent flipped the substrate's cached state, so the
/// caller knows to commit a fresh paint before the next event poll.
/// Non-blocking; an empty queue returns `false` immediately.
fn drain_intents_into_substrate<V: WidgetViewTui>(
    core: &mut ShellCoreTui<V>,
    rx: &mpsc::Receiver<Intent>,
) -> bool {
    let mut state_changed = false;
    while let Ok(intent) = rx.try_recv() {
        if core.dispatch_intent(&intent) {
            state_changed = true;
        }
    }
    state_changed
}

/// R670 §5.41 §5.40 — drain every JSON-RPC frame the stdin reader
/// thread has buffered since the previous tick, dispatch each
/// through [`ShellCoreTui::dispatch_rpc`], and emit the optional
/// response to **stderr**.
///
/// The alternate-screen + raw-mode terminal owns stdout (ratatui
/// commits cells through that fd; any byte written from the
/// substrate side would corrupt the visible frame). The canonical
/// Unix convention for diagnostic / out-of-band streams pairs the
/// JSON-RPC response wire with the audit trace already routed to
/// `PINION_TUI_LOG` (which itself lives on stderr-or-file for the
/// same reason). A broken-pipe write silently skips so a downstream
/// consumer that disconnects mid-session does not abort the TUI
/// loop.
///
/// Returns `true` when at least one frame was drained (regardless of
/// whether the dispatch mutated cached state — RPC handlers run
/// outside the event poll so the caller must commit a fresh paint to
/// surface any AI-driven transition before the next user event).
fn drain_rpc_into_substrate<V: WidgetViewTui>(
    core: &mut ShellCoreTui<V>,
    rx: &mpsc::Receiver<RpcFrame>,
) -> bool {
    let mut any_frame = false;
    while let Ok(RpcFrame { request, reply }) = rx.try_recv() {
        // R-PR47 §5.7 — dispatch through the identical transport-agnostic
        // core, then route the response (if any) through the frame's own
        // reply sink rather than a hard-coded stderr write. For the
        // built-in stdin reader that reply IS a stderr write
        // ([`stderr_reply`]), so the drained bytes are unchanged; a
        // notification (no response) drops the reply, writing nothing.
        if let Some(response) = core.dispatch_rpc(&request) {
            reply.send(response);
        }
        any_frame = true;
    }
    any_frame
}

/// R670 §5.41 §5.40 — JSON-RPC stdin reader thread for the TUI
/// shell. Background-spawned mirror of
/// `pinion_shell::spawn_stdin_rpc_reader` — reads line-delimited
/// JSON-RPC 2.0 frames off stdin and forwards each non-blank line
/// through the supplied `mpsc::Sender<RpcFrame>` (R-PR47 §5.7 — each
/// line paired with a [`stderr_reply`] so the response routes back to
/// the TUI diagnostic wire) so the crossterm event loop drains them on
/// every tick. Blank lines are skipped
/// (so a trailing newline in a piped JSON file does not enqueue an
/// empty frame); EOF or any read error terminates the thread
/// quietly (the TUI loop keeps running so a finite RPC scenario
/// stops sending frames without forcing the binary to exit). The
/// `mpsc::Sender::send` call fails only after the receiver has
/// dropped, in which case the thread also exits.
///
/// The reader uses `stdin().lock()` so the entire stdin handle
/// belongs to this thread for its lifetime — there is no other
/// stdin consumer in the TUI shell (alternate-screen + raw-mode
/// crossterm does not read stdin itself; mouse / keyboard events
/// arrive through the kernel's terminal driver routed by
/// `crossterm::event::poll`/`read`).
/// R1363 §5.55 §2 #6 — the TUI backend's [`QuitSink`](pinion_core::QuitSink):
/// the terminal peer of `pinion_shell::ProxyQuitSink`.
///
/// A terminal has no windows, so it can never hold `WindowControlSink` — which
/// is precisely why quitting had to leave that vocabulary (§5.55). The trait
/// lives in `pinion-core`, the deepest layer BOTH backends dep, so this impl
/// exists at all: one vocabulary, two dispatch paths (§2 #6).
///
/// Sets a flag the event loop reads on its next turn rather than ending the
/// process, so the quit is offered to
/// [`pinion_core::WidgetCore::app_quit_requested`]
/// on the UI thread — a background producer (sprag's poll thread) must not run a
/// binding's veto on its own thread.
///
/// `Send + Sync` via `Arc<AtomicBool>`; the handle clones into a producer thread.
#[derive(Debug, Default)]
struct TuiQuitSink(Arc<AtomicBool>);

impl pinion_core::QuitSink for TuiQuitSink {
    fn request_quit(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn spawn_stdin_rpc_reader_tui(tx: mpsc::Sender<RpcFrame>) {
    thread::spawn(move || {
        let stdin = std::io::stdin();
        let handle = stdin.lock();
        for line in handle.lines() {
            let Ok(text) = line else {
                break;
            };
            if text.trim().is_empty() {
                continue;
            }
            if tx.send(RpcFrame::new(text, stderr_reply())).is_err() {
                break;
            }
        }
    });
}

/// R-PR47 §5.7 — the reply sink for a frame that arrived on the process's
/// own stdin under the TUI backend: its response is written to **stderr**,
/// one line. The alternate-screen + raw-mode terminal owns stdout (ratatui
/// commits cells through that fd; any byte written there would corrupt the
/// visible frame), so the JSON-RPC response wire pairs with the diagnostic
/// stream — the same rationale that routes `PINION_TUI_LOG` to
/// stderr-or-file. A broken-pipe write silently skips so a disconnecting
/// consumer does not abort the TUI loop. Pre-PR47 this stderr write was
/// hard-coded in the drain; making it the stdin reader's reply is what
/// lets an injected transport route its own responses elsewhere.
fn stderr_reply() -> RpcReply {
    RpcReply::new(|response: String| {
        let mut err = stderr().lock();
        let _ = writeln!(err, "{response}");
    })
}

#[cfg(test)]
mod tests {
    use crossterm::event::{MouseButton, MouseEventKind};
    use pinion_core::test_fixtures::{ContextMenuFixture, ContextMenuFixtureState};

    use super::dispatch_mouse;
    use crate::substrate::ShellCoreTui;

    /// R888.1 §5.49 §5.53 — drive the ACTUAL crossterm `Down(Right)`
    /// arm (not a hand-mirrored call pair): the arm must seed the
    /// cursor, run the press-edge one-shot, and report the repaint —
    /// the R887 native-arm test this surface could not host while
    /// `dispatch_mouse` was bound to the live-terminal renderer.
    #[test]
    fn r888_1_down_right_arm_opens_context_menu_at_event_cell() {
        let mut core: ShellCoreTui<ContextMenuFixture> = ShellCoreTui::new();
        let routed = dispatch_mouse(
            &mut core,
            MouseEventKind::Down(MouseButton::Right),
            6.0,
            4.0,
        );
        assert!(routed, "handled right press reports the repaint");
        assert_eq!(
            *core.cached_state(),
            ContextMenuFixtureState {
                open: true,
                anchor: Some((6.0, 4.0)),
            },
            "the native arm anchors the popup at the event cell",
        );
    }

    /// R888.1 — the matching release stays absorbed (press-edge
    /// one-shot has no release half), as does middle.
    #[test]
    fn r888_1_up_right_and_middle_press_stay_absorbed() {
        let mut core: ShellCoreTui<ContextMenuFixture> = ShellCoreTui::new();
        assert!(!dispatch_mouse(
            &mut core,
            MouseEventKind::Up(MouseButton::Right),
            6.0,
            4.0,
        ));
        assert!(!core.cached_state().open, "release alone opens nothing");
    }
}
