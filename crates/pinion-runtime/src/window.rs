//! Window routing layer (§5.17 §5.18, R16 slice 2).
//!
//! Holds the app-level state machine emitted from `app.scxml` (§5.19) and
//! dispatches per-window operations against the SCE-emitted topology.
//!
//! Single-window apps short-circuit per §5.18: absent `/window[id]/` prefix
//! resolves to the first SCE-declared state. Multi-window adds perfect-hash
//! dispatch on the `WindowId` enum (later R16 slice once `<parallel>` root
//! is exercised).

use pinion_core::app::{App, AppEvent, AppState};

/// Window routing surface owning the app-level statechart.
///
/// Single-window (current shape): `current_window()` returns the sole state;
/// `dispatch(event)` forwards to the underlying engine. Multi-window keeps
/// the same surface — only the topology underneath changes.
pub struct WindowRouter {
    app: App,
}

impl WindowRouter {
    #[must_use]
    pub fn new() -> Self {
        Self { app: App::new() }
    }

    /// Current routed window state (§5.18 single-window short-circuit target).
    #[must_use]
    pub fn current_window(&self) -> AppState {
        self.app.state()
    }

    /// Forward an app-level event to the underlying statechart.
    pub fn dispatch(&mut self, event: AppEvent) {
        self.app.send(event);
    }
}

impl Default for WindowRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_initializes_to_first_window() {
        let router = WindowRouter::new();
        assert_eq!(router.current_window(), AppState::Main);
    }
}

/// (R1363 §5.55 §5.49 §2 #2) The window-op vocabulary — what a producer asks
/// pinion to do to ONE window.
///
/// Shell-neutral (no winit types), so every producer speaks it: the winit
/// pointer path (`AppShell::try_chrome_press`), the OS window manager
/// (`WindowEvent::CloseRequested`), the headless RPC click drain (`ShellCore`,
/// the §2 #2 drive-parity leg of the R1121 chrome contract), and a binding's own
/// `WindowControlSink`. Deliberately excludes the grip / resize regions: those
/// are pointer-session gestures (an OS-interactive `drag_window` /
/// `drag_resize_window` needs a live pointer) whose RPC peers are the dedicated
/// `scene/window_move` / `scene/resize` methods, not a discrete control.
///
/// # This vocabulary CANNOT exit the app (§5.55)
///
/// [`Self::Close`] closes a window. That is all it does. Until R1363 a `Close`
/// the binding did not veto fell through to `event_loop.exit()`, which welded
/// the APP lifecycle (one per process) into the WINDOW lifecycle (N per app) —
/// and the fingerprints were everywhere: `Escape` had to bypass the close veto
/// entirely in BOTH shells because neither had a `Quit` verb to route through; a
/// terminal backend could not hold the seam at all, since its app-lifecycle half
/// was welded to a vocabulary a terminal has no use for; a multi-window editor
/// closing its last document died unless every window remembered to veto. App
/// exit is now [`QuitSink`](pinion_core::QuitSink) and
/// [`pinion_core::WidgetCore::app_quit_requested`]
/// — a separate lifecycle with a separate veto, bridged by exactly one policy
/// (`WidgetView::quit_on_last_window_closed`).
///
/// # Home (R1363)
///
/// Lives beside [`DEFAULT_WINDOW`](crate::core_shell::DEFAULT_WINDOW) because
/// this crate owns window IDENTITY (the addressee) and every consumer needs both
/// halves. Pre-R1363 it lived in `pinion-overlay`, whose tag constants birthed
/// it as a click payload — which barred `pinion-runtime` from naming it and
/// forced the seam that requests it one crate too high. R1190's fusion is
/// untouched: only this TYPE moved; `pinion_overlay::chrome_tag_semantic` is
/// still the sole tag-to-semantic authority and still returns
/// `ChromeTag::Control(WindowControl)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowControl {
    /// `Window::set_minimized(true)` — iconify.
    Minimize,
    /// `Window::set_minimized(false)` — un-iconify. (R1363: `Minimize` was a
    /// one-way door — the chrome has no restore button because a minimized
    /// window has no chrome to click, but a tray "Show window" item is exactly
    /// the producer that needs the way back.)
    Restore,
    /// `Window::set_maximized(!is_maximized())` — the chrome button's toggle.
    Maximize,
    /// `Window::set_visible(true)` — map the window.
    Show,
    /// `Window::set_visible(false)` — unmap, without closing. A tray-resident
    /// app's "Hide window".
    Hide,
    /// Close THIS window: offered to
    /// `WidgetView::window_close_requested` first; an unhandled close drops the
    /// window. It does NOT exit the app (§5.55) — only the
    /// `quit_on_last_window_closed` policy can turn "the last window closed"
    /// into a `Quit`, and that `Quit` passes `app_quit_requested`.
    Close,
}
