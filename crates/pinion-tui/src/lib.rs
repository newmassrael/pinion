//! pinion TUI backend — cell-based render mode (§5.41).
//!
//! R51.109.2 substrate: ratatui-backed [`TuiRenderer`] plus the
//! `WidgetRenderer<Frame = Buffer, Context = TuiContext>` impl. The
//! `pinion_core::WidgetRenderer` trait lives at the lowest-level
//! crate so this backend depends only on `pinion-core` (no winit /
//! wgpu transitive); the Vello sibling crate `pinion-shell` keeps the
//! pixel-raster pipeline coupled to its winit / wgpu surface stack.
//!
//! The §2 invariant #6 (GUI/TUI dual: one scene, two render dispatch
//! paths) reaches first impl shape here:
//!
//! ```text
//!                pinion_core::Scene  (single source of truth)
//!                       │
//!         ┌─────────────┴─────────────┐
//!         ▼                           ▼
//!  paint_adapter::to_vello     pinion_tui::paint::to_buffer (R51.110)
//!         │                           │
//!         ▼                           ▼
//!     vello::Scene                ratatui::Buffer
//!         │                           │
//!         ▼                           ▼
//!  VelloRenderer                  TuiRenderer<B: Backend>
//!         │                           │
//!         ▼                           ▼
//!     wgpu surface                terminal cells
//! ```
//!
//! ## Backend genericity (R51.109.2)
//!
//! [`TuiRenderer`] is generic over `ratatui::backend::Backend` so
//! production code wires `CrosstermBackend<Stdout>` while tests reach
//! `TestBackend` without touching crossterm raw mode. Both paths
//! satisfy the substrate's `WidgetRenderer` trait — the shell-side
//! render loop in `pinion-tui-shell` (R51.110 carry) is generic over
//! the backend type so a `--features test-backend` build can drive
//! the same code path in CI without a real TTY.
//!
//! ## Scene → Buffer mapping (R51.110 carry)
//!
//! The `paint::to_buffer(scene, &mut buf)` walker lands in R51.110
//! alongside the first hello-button TUI dogfood. The mapping handles:
//!
//! - `Scene::Box` → border characters + bg fill cells (when the
//!   widget paints a Container, e.g. button background).
//! - `Scene::Text` → grapheme cluster cells via `unicode-segmentation`
//!   + cell width via `unicode-width`.
//! - `Scene::Container` → recurse children with the parent rect's
//!   cell-coord clip.
//! - `Scene::Path` / `Image` → placeholder block char (R51.111+
//!   evaluates unicode-art mapping).
//! - `Scene::Effect` / `External` → skip (escape hatch — §3
//!   capability boundary).
//!
//! ## Re-exports
//!
//! `ratatui` and `crossterm` are re-exported so consumer crates
//! (`pinion-tui-shell`, future `hello-button-tui`) reach the
//! backend's event / buffer / style vocabulary without duplicating
//! the dep declaration.

pub use crossterm;
pub use ratatui;

pub mod paint;
pub mod widget;

pub use widget::{WidgetViewTui, render_one_frame};

use std::io;

use pinion_core::WidgetRenderer;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::buffer::Buffer;

/// R51.109.2 §5.41 — TUI frame render context. Placeholder shape;
/// future rounds (R51.111+) carry the colour-depth / mouse-capability
/// / palette hints once the substrate-incompleteness-signal trigger
/// (first colour mismatch in the hello-button TUI dogfood) drives the
/// concrete design.
///
/// `Copy` so the shell passes by value per frame; `Default` is the
/// zero-hint baseline (terminal's default colour scheme), matching
/// how `VelloContext::default()` falls back to `PenikoColor::BLACK`.
/// The Vello sibling's `Context = VelloContext` keeps the same shape
/// contract so the substrate's `WidgetRenderer::Context: Copy` bound
/// is satisfied uniformly.
#[derive(Debug, Clone, Copy, Default)]
pub struct TuiContext {
    // R51.109.2 placeholder. R51.111+ adds palette / colour depth /
    // mouse capability fields once the substrate-incompleteness-signal
    // trigger drives the design (first hello-button TUI mismatch).
    // The `#[allow]` keeps `clippy::manual_non_exhaustive` quiet on
    // the empty-field-struct shape pinion-tui consumers see today.
    _reserved: (),
}

/// R51.109.2 §5.41 — ratatui-backed TUI renderer.
///
/// Generic over `ratatui::backend::Backend` so production binaries
/// wire `ratatui::backend::CrosstermBackend<Stdout>` (the crossterm
/// raw-mode terminal) while tests reach `ratatui::backend::TestBackend`
/// (the headless in-memory buffer) without touching real TTY setup.
/// Implements [`pinion_core::WidgetRenderer`] with
/// `Frame = ratatui::buffer::Buffer` and `Context = TuiContext`; the
/// shell-side render loop hands in the painted buffer + context once
/// per frame and the renderer commits to the backend.
pub struct TuiRenderer<B: Backend> {
    terminal: Terminal<B>,
}

impl<B: Backend> TuiRenderer<B> {
    /// Construct a [`TuiRenderer`] from a `ratatui::Backend` impl.
    ///
    /// # Errors
    /// Propagates `std::io::Error` from `ratatui::Terminal::new`,
    /// which queries the backend for the initial terminal size and
    /// allocates the front + back buffers. `TestBackend` never errors;
    /// `CrosstermBackend<Stdout>` can error on terminal size query
    /// failures.
    pub fn new(backend: B) -> io::Result<Self> {
        Ok(Self {
            terminal: Terminal::new(backend)?,
        })
    }
}

impl<B: Backend> core::fmt::Debug for TuiRenderer<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TuiRenderer").finish_non_exhaustive()
    }
}

impl<B: Backend> WidgetRenderer for TuiRenderer<B> {
    type Error = io::Error;
    type Frame = Buffer;
    type Context = TuiContext;

    fn render(&mut self, frame: &Buffer, _ctx: TuiContext) -> io::Result<()> {
        // ratatui's draw cycle: the closure mutates the terminal's
        // back buffer; the wrapper diffs against the front buffer and
        // emits only the changed cells to the backend, swapping the
        // buffers atomically afterwards. Copying the input frame's
        // cells into the back buffer (clamped to the overlap) gives
        // the same dirty-region commit semantics as the Vello path.
        self.terminal.draw(|target_frame| {
            let target = target_frame.buffer_mut();
            // Hoist origins into locals so the mutable-borrow index
            // expression below does not also read through `target`.
            let (tx, ty, tw, th) =
                (target.area.x, target.area.y, target.area.width, target.area.height);
            let (fx, fy, fw, fh) =
                (frame.area.x, frame.area.y, frame.area.width, frame.area.height);
            let max_x = fw.min(tw);
            let max_y = fh.min(th);
            for y in 0..max_y {
                for x in 0..max_x {
                    target[(tx + x, ty + y)] = frame[(fx + x, fy + y)].clone();
                }
            }
        })?;
        Ok(())
    }

    fn resize(&mut self, _width: u32, _height: u32) {
        // ratatui auto-tracks the terminal size via the backend's
        // `Backend::size` call inside `Terminal::autoresize`, invoked
        // at every `draw` cycle. Explicit `resize` is a no-op at this
        // layer — the size flow is `crossterm::Resize` → ratatui's
        // internal autoresize → next-frame `Buffer::area` widening.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    #[test]
    fn tui_renderer_constructs_with_test_backend() {
        // R51.109.2 — anchors the `TuiRenderer<TestBackend>` shape so
        // future paint mapping work (R51.110) can build on a known
        // headless construction surface.
        let _renderer = TuiRenderer::new(TestBackend::new(40, 10)).expect("TestBackend never errors");
    }

    #[test]
    fn render_copies_buffer_to_terminal() {
        // R51.109.2 — verifies the substrate's `render` plumbing
        // (closure-based buffer copy) does not panic on the
        // happy-path overlap. The real Scene → Buffer mapping lands
        // in R51.110 with hello-button TUI dogfood.
        let mut renderer =
            TuiRenderer::new(TestBackend::new(40, 10)).expect("TestBackend never errors");
        let area = Rect::new(0, 0, 40, 10);
        let mut src = Buffer::empty(area);
        src[(5, 3)].set_char('X');
        renderer
            .render(&src, TuiContext::default())
            .expect("TestBackend draw never errors");
    }

    #[test]
    fn widget_renderer_impl_satisfies_trait() {
        // R51.109.2 — compile-time verification: `TuiRenderer<B>`
        // satisfies the `pinion_core::WidgetRenderer` trait bound.
        // The shell-side generic dispatcher (R51.110 `run::<V>()`
        // entry point) relies on this bound holding for every
        // backend instantiation.
        fn assert_widget_renderer<R: WidgetRenderer>(_: &R) {}
        let renderer = TuiRenderer::new(TestBackend::new(10, 5)).unwrap();
        assert_widget_renderer(&renderer);
    }

    #[test]
    fn ratatui_reexport_resolves() {
        // R51.109.2 — preserved from R51.109.0; verifies the
        // `pub use ratatui` surface compiles end-to-end.
        let _: ratatui::layout::Rect = ratatui::layout::Rect::default();
    }

    #[test]
    fn crossterm_reexport_resolves() {
        // R51.109.2 — preserved from R51.109.0; verifies the
        // `pub use crossterm` surface compiles end-to-end.
        let _ = crossterm::event::Event::Resize(80, 24);
    }
}
