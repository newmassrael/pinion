//! pinion TUI backend — cell-based render mode (§5.41).
//!
//! R51.109.0 scaffolding round: crate skeleton + external dependency
//! pin. Lands the type identity for `TuiRenderer` so downstream rounds
//! (R51.109.1 substrate trait, R51.109.2 actual `WidgetRenderer for
//! TuiRenderer` impl, R51.110 first hello-button TUI dogfood) can
//! reference a stable path without successively breaking imports.
//!
//! The §2 invariant #6 (GUI/TUI dual: one scene, two render dispatch
//! paths) lives in this crate as the cell-based sibling of the
//! Vello pixel-raster pipeline:
//!
//! ```text
//!                pinion_core::Scene  (single source of truth)
//!                       │
//!         ┌─────────────┴─────────────┐
//!         ▼                           ▼
//!  paint_adapter::to_vello     paint_adapter::to_tui    (R51.109.1)
//!         │                           │
//!         ▼                           ▼
//!     vello::Scene                ratatui::Buffer
//!         │                           │
//!         ▼                           ▼
//!  VelloRenderer                  TuiRenderer            (R51.109.2)
//!         │                           │
//!         ▼                           ▼
//!     wgpu surface                terminal cells
//! ```
//!
//! ## Backend election
//!
//! The pinion-shell `WidgetRenderer` trait (R51.109.1) is the
//! backend-agnostic dispatch boundary — every `WidgetView` consumes a
//! renderer concretely chosen at the binary's `fn main()` (cargo
//! `--features vello` vs `--features tui`). The single-binary contract
//! holds: no `dyn WidgetRenderer` in the hot path; the trait exists to
//! make the two backends share `WidgetView` / `ShellCore` substrate.
//!
//! ## Re-exports
//!
//! `ratatui` and `crossterm` are re-exported so consumer crates
//! (`hello-button` TUI variant, R51.110) reach the backend's event /
//! buffer / style vocabulary without duplicating the dep declaration.

pub use crossterm;
pub use ratatui;

/// R51.109.0 §5.41 — TUI backend renderer (placeholder shape).
///
/// The actual `ratatui::Terminal<CrosstermBackend<Stdout>>` ownership,
/// raw-mode lifecycle, and `WidgetRenderer` impl wire-up land in
/// R51.109.2; this round reserves only the type identity so the
/// R51.109.1 substrate trait can name `TuiRenderer` in its docs and
/// callers can `use pinion_tui::TuiRenderer` without breaking once
/// the impl lands.
///
/// Future shape (preview, R51.109.2):
///
/// ```rust,ignore
/// pub struct TuiRenderer {
///     terminal: ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
/// }
/// ```
#[derive(Debug, Default)]
pub struct TuiRenderer {
    // R51.109.0 — empty placeholder. R51.109.2 wires the
    // `ratatui::Terminal<CrosstermBackend<Stdout>>` field + raw mode
    // lifecycle. Empty struct keeps the type identity public + visible
    // in the workspace while the substrate trait (R51.109.1) is
    // designed.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tui_renderer_skeleton_constructs() {
        // R51.109.0 — anchors the type identity into the test
        // surface so the next round (R51.109.1) cannot accidentally
        // remove the public name during substrate trait integration.
        let _renderer = TuiRenderer::default();
    }

    #[test]
    fn ratatui_reexport_resolves() {
        // R51.109.0 — verifies the re-export surface compiles. Once
        // the actual impl lands (R51.109.2) this test extends to
        // assert the `Backend` trait surface the impl depends on.
        let _: ratatui::layout::Rect = ratatui::layout::Rect::default();
    }

    #[test]
    fn crossterm_reexport_resolves() {
        // R51.109.0 — verifies the crossterm re-export. The
        // `Event::Resize` discriminant is the simplest non-trivial
        // constructor that exercises the dependency without standing
        // up a real terminal.
        let _ = crossterm::event::Event::Resize(80, 24);
    }
}
