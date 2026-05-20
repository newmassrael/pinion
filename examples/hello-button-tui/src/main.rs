//! R51.110.2 §5.41 — first hello-button TUI dogfood.
//!
//! Demonstrates the cell-based render mode substrate end-to-end:
//! `WidgetViewTui` binding + `pinion_tui::run::<V>()` event loop +
//! `paint::to_buffer` Scene → cell mapping + `TuiRenderer<B>`
//! commit to the live terminal.
//!
//! Run:
//!
//! ```bash
//! cargo run -p hello-button-tui
//! ```
//!
//! The terminal switches to the alternate screen, paints a
//! button-shaped text label, and waits for `Esc` to exit. Resize
//! triggers a repaint at the new dimensions; other keys / mouse
//! events are consumed silently (input dispatch lands R51.111+).
//!
//! This first-cut dogfood validates:
//! - pinion-tui's substrate compiles + links against a real
//!   crossterm-backed terminal.
//! - `paint::to_buffer` renders `TextNode` content at pixel→cell
//!   coords matching the substrate's `PIXEL_PER_CELL_*` constants.
//! - The RAII terminal restore guard handles both `Esc` exit and
//!   panic cleanup.
//!
//! Carry forward to R51.111+:
//! - Real button click via crossterm `Event::Mouse` →
//!   `InputRouter::pointer_down` (currently mouse events are
//!   ignored).
//! - SCXML statechart wire-up — the `Self::create_external` impl
//!   below returns a no-op `External` because the click dispatch
//!   path is not yet wired in.

use std::io::Stdout;

use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, RepaintOwner,
    ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::{Frame, style};
use pinion_tui::ratatui::backend::CrosstermBackend;
use pinion_tui::{TuiRenderer, WidgetViewTui};

/// The widget binding unit type. `pinion_tui::run::<HelloButtonTui>()`
/// instantiates the substrate around this binding.
struct HelloButtonTui;

/// The cached state projection. Trivial unit struct — R51.110.2
/// dogfood renders a static label; R51.111+ extends to the live
/// SCXML statechart's Idle / Pressed / Hover variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HelloState;

/// The typed widget event. Unused this round (no input dispatch);
/// R51.111+ surfaces real button events.
#[derive(Debug, Clone, Copy)]
struct HelloEvent;

/// No-op `External` implementation. The R51.110.2 dogfood does not
/// drive SCXML state transitions — the loop reads the cached state
/// once, paints once, then idles until `Esc`. R51.111+ wires the
/// actual hello-button SCXML statechart through this slot via
/// `pinion_core::widgets::button::Button::new()`.
#[derive(Debug)]
struct StubExternal;

impl External for StubExternal {
    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        None
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        None
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn backends(&self) -> BackendSupport {
        // R51.110.2 — the binding consumes the Gui backend slot
        // (the TUI substrate reuses the Gui-flagged `External`
        // pathway until a dedicated `Backend::Tui` axis lands
        // R51.111+).
        BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
    }
}

impl WidgetViewTui for HelloButtonTui {
    type State = HelloState;
    type Event = HelloEvent;
    type Renderer = TuiRenderer<CrosstermBackend<Stdout>>;

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal)
    }

    fn tag() -> &'static str {
        "hello_button_tui"
    }

    fn read_state(_scene: &Scene) -> Self::State {
        HelloState
    }

    fn view(_state: Self::State, _frame: &Frame) -> Scene {
        // Construct a Container holding two text nodes:
        // 1) The button label at cells (2, 1)..=(20, 3) (pixel
        //    8 × 16 baseline).
        // 2) The exit hint two rows below.
        //
        // Pixel coords map to cells via the substrate's standard
        // 8×16 mapping in `pinion_tui::paint::PIXEL_PER_CELL_*`.

        let mut label = TextNode::default();
        "[ Hello, button TUI! ]".clone_into(&mut label.content);
        label.rect = Rect::new(16, 32, 200, 16);
        label.style = style::TextStyle::default();

        let mut hint = TextNode::default();
        "Press Esc to exit.".clone_into(&mut hint.content);
        hint.rect = Rect::new(16, 80, 200, 16);
        hint.style = style::TextStyle::default();

        let mut container = ContainerNode::default();
        container.rect = Rect::new(0, 0, 640, 240);
        container.children.push(Scene::Text(label));
        container.children.push(Scene::Text(hint));

        Scene::Container(container)
    }

    fn event_name(_event: Self::Event) -> &'static str {
        "hello_button_tui_event"
    }

    fn title() -> &'static str {
        "pinion hello-button-tui"
    }
}

fn main() {
    if let Err(e) = pinion_tui::run::<HelloButtonTui>() {
        // The RAII guard in `pinion_tui::run` has already restored
        // the terminal by the time this prints — the user sees the
        // error message in the normal scrollback, not in the
        // alternate screen.
        eprintln!("hello-button-tui: shell error: {e}");
        std::process::exit(1);
    }
}
