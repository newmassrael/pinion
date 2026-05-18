//! Multi-window topology fixture (§5.17 §5.18, R16 slice 4).
//!
//! Validates the parallel-root branch of [`crate::topology`] under N > 1
//! windows. The underlying SCXML at `fixtures/multi_window.scxml` declares
//! `<parallel id="root">` with three child `<state>`s (main, settings,
//! debug). This module is `#[cfg(test)]`-gated because it exists only to
//! stress the topology idiom; consumer crates use `crate::app::App` for
//! their real `app.scxml`.

// `unsafe_code` intentionally absent — see `widgets/button.rs` for the
// workspace `forbid` policy rationale. sce-build codegen output does
// not use `unsafe`.
#[allow(
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
    include!(concat!(env!("OUT_DIR"), "/multi_window_sm.rs"));
}

use sce_rust_runtime::StatePolicy;

use crate::topology;

pub use sm::MultiWindowState;
use sm::MultiWindowPolicy;

/// Parallel-root window fixture exposing the same topology surface as
/// `App` but over `MultiWindowPolicy`.
pub struct MultiWindowApp;

impl MultiWindowApp {
    /// First declared window per §5.18 short-circuit (lowest doc-order
    /// region of the parallel root).
    #[must_use]
    pub fn initial_window() -> MultiWindowState {
        topology::initial_window::<MultiWindowPolicy>()
    }

    #[must_use]
    pub fn window_name(state: MultiWindowState) -> &'static str {
        <MultiWindowPolicy as StatePolicy>::get_state_name(state)
    }

    #[must_use]
    pub fn window_from_name(name: &str) -> Option<MultiWindowState> {
        topology::window_from_name::<MultiWindowPolicy>(name)
    }

    /// All declared windows (parallel regions of the root). Returns the
    /// SCE-emit static slice directly — N · O(1).
    #[must_use]
    pub fn windows() -> &'static [MultiWindowState] {
        let root = <MultiWindowPolicy as StatePolicy>::initial_state();
        <MultiWindowPolicy as StatePolicy>::get_parallel_regions(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_lists_three_parallel_regions() {
        let ws = MultiWindowApp::windows();
        assert_eq!(ws.len(), 3);
        // SCE emit sorts regions alphabetically in the slice (Debug, Main,
        // Settings); assert membership rather than order to stay robust to
        // SCE-emit sort changes.
        assert!(ws.contains(&MultiWindowState::Main));
        assert!(ws.contains(&MultiWindowState::Settings));
        assert!(ws.contains(&MultiWindowState::Debug));
    }

    #[test]
    fn initial_window_is_lowest_doc_order_region() {
        // Document order: Main=1, Settings=2, Debug=3 (Root=0 is the
        // parallel parent, excluded from regions). Main wins.
        assert_eq!(MultiWindowApp::initial_window(), MultiWindowState::Main);
    }

    #[test]
    fn each_window_name_round_trips() {
        for &w in MultiWindowApp::windows() {
            let name = MultiWindowApp::window_name(w);
            assert_eq!(MultiWindowApp::window_from_name(name), Some(w));
        }
    }

    #[test]
    fn parallel_root_id_is_not_a_window() {
        // "root" is the parallel parent's SCXML id but it is the topology
        // container, not a window. Reverse-map must not surface it.
        assert_eq!(MultiWindowApp::window_from_name("root"), None);
    }

    #[test]
    fn unknown_id_rejected() {
        assert_eq!(MultiWindowApp::window_from_name("ghost"), None);
    }
}
