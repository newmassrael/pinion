//! R51.121 §5.41 — TUI-specific widget binding trait.
//!
//! [`WidgetViewTui`] is the TUI sibling of `pinion_shell::WidgetView`;
//! both traits inherit the bulk of their surface from the
//! [`pinion_core::WidgetCore`] backend-free contract and the
//! [`pinion_a11y::WidgetA11y`] accessibility contract, adding only the
//! backend-specific items (`Renderer` + `initial_size` unit). The
//! supertrait chain replaces R51.110.1's parallel-trait duplication
//! after the R51.113 second TUI binding surfaced the
//! [[substrate-incompleteness-signal]] trigger.
//!
//! ## Why a supertrait split, not a single backend-generic trait
//!
//! See `pinion_core::widget_core` module docs for the full rationale.
//! Short version: cells vs pixels is the textbook initial-size split,
//! so each backend's `initial_size` lives in the language its
//! consumers actually use; the other 12 binding methods live upstream
//! exactly once.

use pinion_core::Frame;
use pinion_core::renderer::WidgetRenderer;
use ratatui::buffer::Buffer;

use crate::TuiContext;

/// R51.121 §5.41 — TUI-specific application-side widget binding.
///
/// One impl per visual TUI binary on a unit type; `pinion_tui::run::<V>()`
/// wires the substrate's repaint cycle around it.
///
/// The trait inherits the bulk of its surface via the supertrait
/// chain [`pinion_a11y::WidgetA11y`] →
/// [`pinion_core::WidgetCore`]; only the TUI-specific
/// [`Renderer`](Self::Renderer) associated type and the cell-unit
/// [`initial_size`](Self::initial_size) live here.
pub trait WidgetViewTui: pinion_a11y::WidgetA11y {
    /// Concrete TUI renderer. Locked to the `WidgetRenderer`
    /// specialization at `Frame = Buffer` + `Context = TuiContext`
    /// so the substrate's render call is invariant across bindings.
    /// `'static` so the substrate can store `Box<Self::Renderer>`
    /// across suspend / resume cycles without lifetime parameters.
    type Renderer: WidgetRenderer<Frame = Buffer, Context = TuiContext> + 'static;

    /// Default terminal size hint in **cells** (columns × rows). The
    /// substrate uses this to size the initial `ratatui::buffer::
    /// Buffer` before the first `crossterm::event::Resize` event
    /// reports the actual terminal dimensions. `(80, 24)` is the
    /// industry-baseline default (every terminal emulator's startup
    /// geometry).
    #[must_use]
    fn initial_size() -> (u16, u16) {
        (80, 24)
    }
}

/// R51.110.1 §5.41 — render one frame of `V` into a fresh
/// `ratatui::buffer::Buffer`.
///
/// Pure helper that exercises the substrate's view-fn → paint pipe
/// without standing up a real terminal. The substrate's repaint
/// cycle (R51.110.2 `pinion_tui::run::<V>()`) wraps this in a
/// crossterm event loop; tests use it directly to assert paint
/// output without a TTY.
///
/// `cols` / `rows` are cell dimensions; the resulting buffer is
/// sized to match. The view fn receives the standard
/// `pinion_core::Frame` (a ZST sentinel — pinion-core's view-fn
/// surface intentionally carries no per-frame dimensions, the
/// scene's `rect` fields are pixel-absolute), and the paint walker
/// (`paint::to_buffer`) maps pixel coords to cells via the R968
/// §5.41 `CellMetric` (default 8×16).
#[must_use]
pub fn render_one_frame<V: WidgetViewTui>(state: V::State, cols: u16, rows: u16) -> Buffer {
    let frame = Frame::new();
    let scene = V::view(state, &frame);
    let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, cols, rows));
    crate::paint::to_buffer(&scene, &mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::WidgetCore;
    use pinion_core::external::External;
    use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};

    /// A minimal `WidgetViewTui` impl for testing the helper +
    /// trait shape end-to-end without a real SCXML widget.
    struct DummyView;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct DummyState {
        counter: u8,
    }

    #[derive(Debug, Clone, Copy)]
    struct DummyEvent;

    // The `WidgetRenderer` impl for tests — uses `TuiRenderer`
    // directly via the type identity, sized at `TestBackend`.
    type DummyRenderer = crate::TuiRenderer<ratatui::backend::TestBackend>;

    impl WidgetCore for DummyView {
        type State = DummyState;
        type Event = DummyEvent;

        fn create_external() -> Box<dyn External> {
            // Test path doesn't drive SCXML — return a no-op
            // External. R51.111+ tests use a real SCXML widget.
            #[derive(Debug)]
            struct NoopExternal;
            impl External for NoopExternal {
                fn introspect(&self) -> Option<&dyn pinion_core::external::ExternalIntrospect> {
                    None
                }
                fn introspect_mut(
                    &mut self,
                ) -> Option<&mut dyn pinion_core::external::ExternalIntrospect> {
                    None
                }
                fn repaint_ownership(&self) -> pinion_core::external::RepaintOwner {
                    pinion_core::external::RepaintOwner::Framework
                }
                fn thread_ownership(&self) -> pinion_core::external::ThreadOwnership {
                    pinion_core::external::ThreadOwnership::UiThreadSync
                }
                fn backends(&self) -> pinion_core::external::BackendSupport {
                    pinion_core::external::BackendSupport::new(
                        &[pinion_core::external::Backend::Gui],
                        pinion_core::external::BackendFallback::Skip,
                    )
                }
            }
            Box::new(NoopExternal)
        }

        fn tag() -> &'static str {
            "dummy_view"
        }

        fn read_state(_scene: &Scene) -> Self::State {
            DummyState { counter: 0 }
        }

        fn view(state: Self::State, _frame: &Frame) -> Scene {
            // Render the counter at the upper-left cell. The helper
            // converts pixel coords → cells via the standard 8×16
            // baseline, so pixel (8, 0) lands at cell (1, 0).
            let mut text = TextNode::default();
            text.rect = Rect::new(8, 0, 100, 16);
            text.content = format!("counter={}", state.counter);
            let mut container = ContainerNode::default();
            container.rect = Rect::new(0, 0, 320, 160);
            container.children.push(Scene::Text(text));
            Scene::Container(container)
        }

        fn event_name(_event: Self::Event) -> &'static str {
            "dummy_event"
        }

        fn title() -> &'static str {
            "Dummy TUI Test"
        }
    }

    impl pinion_a11y::WidgetA11y for DummyView {}

    impl WidgetViewTui for DummyView {
        type Renderer = DummyRenderer;
    }

    #[test]
    fn render_one_frame_produces_expected_buffer() {
        // 40×10 cell terminal, dummy state with counter=0.
        let buf = render_one_frame::<DummyView>(DummyState { counter: 0 }, 40, 10);
        assert_eq!(buf.area.width, 40);
        assert_eq!(buf.area.height, 10);
        // Pixel (8, 0) → cell (1, 0) — the 'c' of "counter=0".
        assert_eq!(buf[(1, 0)].symbol(), "c");
        assert_eq!(buf[(2, 0)].symbol(), "o");
        assert_eq!(buf[(3, 0)].symbol(), "u");
    }

    #[test]
    fn initial_size_default_is_80_by_24() {
        // R51.110.1 — confirms the 80×24 default matches the
        // industry baseline. Bindings override this only when their
        // view-fn needs a specific minimum.
        assert_eq!(DummyView::initial_size(), (80, 24));
    }

    #[test]
    fn rendering_different_states_produces_different_buffers() {
        let buf0 = render_one_frame::<DummyView>(DummyState { counter: 0 }, 40, 10);
        let buf1 = render_one_frame::<DummyView>(DummyState { counter: 1 }, 40, 10);
        // counter=0 → '0', counter=1 → '1' at the same cell.
        // The "=N" suffix lands at cell (9, 0) (pixel 8 → cell 1
        // for 'c', so "counter=" spans cells 1..=8, value at 9).
        assert_eq!(buf0[(9, 0)].symbol(), "0");
        assert_eq!(buf1[(9, 0)].symbol(), "1");
    }
}
