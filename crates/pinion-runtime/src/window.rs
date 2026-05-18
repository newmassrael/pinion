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
