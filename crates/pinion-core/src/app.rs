//! Window topology entry point (§5.19, R16).
//!
//! `app.scxml` at the consumer crate root is compiled by `sce-build` and
//! included here. The wrapping `mod sm` block absorbs the inner attributes
//! that `include!()` does not permit in expansion position.
//!
//! `App` wraps `Engine<AppPolicy>` (Button widget pattern from R12) so
//! callers do not touch `sce_rust_runtime::Engine` directly. `pinion-runtime`
//! consumes this for window routing per §5.17 §5.18.

#[allow(
    unsafe_code,
    non_snake_case,
    unused_imports,
    dead_code,
    unused_variables,
    unused_mut,
    unused_labels,
    unreachable_patterns,
    unreachable_code,
    unused_assignments,
    clippy::style,
    clippy::complexity,
    clippy::pedantic,
    clippy::all,
)]
mod sm {
    include!(concat!(env!("OUT_DIR"), "/app_sm.rs"));
}

use sce_rust_runtime::{Engine, StatePolicy};

pub use sm::{AppEvent, AppState};
use sm::AppPolicy;

/// Application-level state machine driving window topology.
///
/// Per §5.17, the root state set declares window topology and SCE Forge
/// emits routing/lifecycle from it. Single root state → single-window app
/// (§5.18 short-circuit). Multi-window is declared via a `<parallel>` root
/// with N child states; that path is exercised in a later R16 slice.
pub struct App {
    engine: Engine<AppPolicy>,
}

impl App {
    #[must_use]
    pub fn new() -> Self {
        let mut engine = Engine::new(AppPolicy::new());
        engine.initialize();
        Self { engine }
    }

    pub fn send(&mut self, event: AppEvent) {
        self.engine.process_event(event);
    }

    #[must_use]
    pub fn state(&self) -> AppState {
        self.engine.get_current_state()
    }

    /// Initial window — §5.18 absent-prefix short-circuit target.
    #[must_use]
    pub fn initial_window() -> AppState {
        <AppPolicy as StatePolicy>::initial_state()
    }

    /// Forward map: state → SCE-emit id string (§5.18 RPC path token).
    #[must_use]
    pub fn window_name(state: AppState) -> &'static str {
        <AppPolicy as StatePolicy>::get_state_name(state)
    }

    /// Reverse map: SCE-emit id string → state. Returns `None` if `name`
    /// is not a declared window.
    ///
    /// Single-window today: linear compare against `initial_window`. The
    /// §5.18 perfect-hash dispatch lands when `<parallel>` root brings
    /// multi-window declarations (later R16 slice).
    #[must_use]
    pub fn window_from_name(name: &str) -> Option<AppState> {
        let initial = Self::initial_window();
        if Self::window_name(initial) == name {
            Some(initial)
        } else {
            None
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_main() {
        let app = App::new();
        assert_eq!(app.state(), AppState::Main);
    }

    #[test]
    fn topology_accessors_round_trip() {
        let initial = App::initial_window();
        assert_eq!(initial, AppState::Main);
        let name = App::window_name(initial);
        assert_eq!(name, "main");
        assert_eq!(App::window_from_name(name), Some(initial));
        assert_eq!(App::window_from_name("unknown"), None);
    }
}
