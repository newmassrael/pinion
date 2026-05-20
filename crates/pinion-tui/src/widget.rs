//! R51.110.1 §5.41 — TUI widget binding trait.
//!
//! [`WidgetViewTui`] is the TUI sibling of `pinion_shell::WidgetView`
//! (the Vello binding trait). Both traits declare a backend-coupled
//! `Renderer` associated type plus the backend-agnostic view-fn /
//! state / event surface; the substrate's `WidgetRenderer` trait
//! (`pinion_core`) unifies the two backends so the actual paint
//! pipeline matches across.
//!
//! ## Why an alternate trait, not a generic
//!
//! The textbook end-state is a single `WidgetView` trait generic over
//! the backend (`type Renderer: WidgetRenderer`). The R51.110.1 first
//! cut lands an alternate `WidgetViewTui` trait instead because:
//!
//! - The Vello binding contract has accreted backend-specific
//!   convenience hooks (`keybinding`, `apply_key`, `access_node`,
//!   `focusable_tags`, `fmt_state_log`) that don't all translate
//!   cleanly to TUI. Folding them into a generic trait would force
//!   premature decisions on each hook's TUI semantics.
//! - The first 2 visual TUI bindings (R51.110.2 hello-button-tui,
//!   R51.111+ second example) will surface the substrate-incomplete
//!   ness-signal — at which point the textbook merge to a generic
//!   `WidgetView<R: WidgetRenderer>` lands as a substrate-evolution
//!   round driven by concrete trigger, not speculation.
//!
//! This mirrors the R51.108 → R51.109 progression: every substrate
//! lift waited for the second client to surface the seam.
//!
//! ## What this trait carries (R51.110.1 minimal cut)
//!
//! - `type Renderer`: concrete backend renderer, locked to
//!   `WidgetRenderer<Frame = Buffer, Context = TuiContext> + 'static`.
//! - `type State` + `type Event`: backend-agnostic, identical shape
//!   to `WidgetView` (the application's cached projection + typed
//!   widget event).
//! - `view` + `read_state`: pure view-fn that the substrate calls per
//!   frame (purity invariant per §6.3 R51.27 `dry_run`).
//! - `tag` / `event_name` / `title` / `initial_size`: identical-shape
//!   identifiers + window hint (cells, not pixels).
//! - `create_external`: hands the substrate the live SCXML statechart
//!   for `Scene::External`-driven dispatch.
//!
//! Future R51.111+ items (input dispatch, focus management, a11y)
//! land as carries once their first concrete TUI consumer surfaces.

use pinion_core::external::External;
use pinion_core::renderer::WidgetRenderer;
use pinion_core::{Frame, Scene};
use ratatui::buffer::Buffer;

use crate::TuiContext;

/// R51.110.1 §5.41 — application-side TUI widget binding contract.
///
/// One impl per visual TUI binary on a unit type; `pinion_tui::run::<V>()`
/// (R51.110.2 carry) wires the substrate's repaint cycle around it.
/// Minimal-cut surface: state + event + renderer + view-fn + tag +
/// title + initial size. Input dispatch / focus / a11y hooks land
/// R51.111+ alongside the second TUI binding (substrate-incomplete
/// ness-signal trigger).
pub trait WidgetViewTui: 'static {
    /// Cached projection of the live state scene. `Copy` lets the
    /// substrate clone it into the paint closure without lifetime
    /// gymnastics; `Debug` + `PartialEq` for change-detection +
    /// transition log.
    type State: Copy + core::fmt::Debug + PartialEq;

    /// Typed widget event enum — usually the SCXML-emitted
    /// `<Widget>Event`. Threaded through [`WidgetViewTui::event_name`]
    /// before reaching the §5.15 `invoke("send", Text(<name>))`
    /// channel so applications keep typed event payloads without
    /// giving up the symbolic RPC contract.
    type Event: Copy;

    /// Concrete TUI renderer. Locked to the `WidgetRenderer`
    /// specialization at `Frame = Buffer` + `Context = TuiContext`
    /// so the substrate's render call is invariant across bindings.
    /// `'static` so the substrate can store `Box<Self::Renderer>`
    /// across suspend / resume cycles without lifetime parameters.
    type Renderer: WidgetRenderer<Frame = Buffer, Context = TuiContext> + 'static;

    /// Build a fresh state scene root. Called once at substrate
    /// boot — should return `Scene::External(ExternalNode::new(<my
    /// widget>).with_tag(Self::tag()))` so the input router's
    /// hit-test on the paint-side tag routes to this node.
    fn create_external() -> Box<dyn External>;

    /// Stable identifier matching the paint-side `Container::tag` the
    /// view fn attaches to the interactive surface. The substrate's
    /// input router forwards pointer / key events to the matching
    /// `Scene::External` in the state scene.
    fn tag() -> &'static str;

    /// Extract the cached projection from the live state scene via
    /// the §5.15 introspect channel — same path an RPC
    /// `scene/query /external/<slot>` request uses, so the cached
    /// state and the AI client always see the same value.
    fn read_state(scene: &Scene) -> Self::State;

    /// Build the paint scene for the current cached state. Pure sync
    /// per §6.3 R51.27 `dry_run` invariant: same `(state, frame)`
    /// always yields the same `Scene`. The substrate calls
    /// `pinion_tui::paint::to_buffer` on the result before handing
    /// it to the renderer.
    fn view(state: Self::State, frame: &Frame) -> Scene;

    /// Convert a typed widget event into the symbolic name the §5.15
    /// `invoke("send", IntrospectValue::Text(<name>))` channel
    /// expects. SCXML-internal variants that never come from
    /// crossterm input should route through a wildcard with a
    /// sentinel name the parser rejects.
    fn event_name(event: Self::Event) -> &'static str;

    /// Terminal title string. Crossterm sets the terminal emulator's
    /// title via the `ESC ] 0 ; <title> BEL` OSC sequence on
    /// `pinion_tui::run::<V>()` boot.
    fn title() -> &'static str;

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

    /// R51.111 §5.41 — optional keyboard event mapping. The shell
    /// consults this on every key press whose W3C `KeyboardEvent.key`
    /// string the input bridge can produce; `None` means "no
    /// keybinding for this key" and the shell falls through to
    /// [`Self::apply_key`]. `Esc` / `Tab` / `BackTab` are
    /// shell-reserved and never reach this hook.
    ///
    /// Mirrors `pinion_shell::WidgetView::keybinding`. Default returns
    /// `None` so widgets without keyboard affordances need no
    /// override.
    #[must_use]
    fn keybinding(_key: &str) -> Option<Self::Event> {
        None
    }

    /// R51.111 §5.41 — escape hatch for keyboard affordances that the
    /// enum-typed [`keybinding`](Self::keybinding) channel cannot
    /// express. The shell consults this AFTER `keybinding` returns
    /// `None`. Receives the authoritative state scene `&mut` so the
    /// widget can walk to the matching [`Scene::External`] and call
    /// `ExternalIntrospect::invoke` / `intervene` — the same side
    /// door the RPC `scene/invoke` route uses, and the same path
    /// `pinion_shell::WidgetView::apply_key` writes against.
    ///
    /// `focused` carries the substrate's currently-focused tag so
    /// widgets that match against it route keys only when their own
    /// tag is the focus target. The R51.111 TUI shell passes
    /// `Some(Self::tag())` unconditionally because focus management
    /// is carry-forward (R51.112+ TUI `FocusManager`); single-widget
    /// dogfood bindings see implicit focus on their sole tag.
    ///
    /// Returns `true` if the key was handled (the shell refreshes
    /// cached state, drains intents, and repaints on visible change).
    /// Returns `false` to defer to whatever fallback the shell adds
    /// next (none today; same swallow semantics as an unmatched
    /// `keybinding`).
    ///
    /// Default returns `false` for every key — widgets without
    /// keyboard affordances beyond `keybinding` need no override.
    #[must_use]
    fn apply_key(_scene: &mut Scene, _focused: Option<&str>, _key: &str) -> bool {
        false
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
/// (`paint::to_buffer`) maps pixel coords to cells via
/// `PIXEL_PER_CELL_*` constants.
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

    impl WidgetViewTui for DummyView {
        type State = DummyState;
        type Event = DummyEvent;
        type Renderer = DummyRenderer;

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
