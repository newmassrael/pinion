//! R51.110.2 / R51.111 / R51.112 §5.41 — first hello-button TUI dogfood.
//!
//! Demonstrates the cell-based render mode substrate end-to-end:
//! `WidgetViewTui` binding + `pinion_tui::run::<V>()` event loop +
//! `paint::to_buffer` Scene → cell mapping + `TuiRenderer<B>`
//! commit to the live terminal + **R51.111 input dispatch +
//! SCXML wire-up**.
//!
//! Run:
//!
//! ```bash
//! cargo run -p hello-button-tui
//! ```
//!
//! The terminal switches to the alternate screen, paints a
//! button-shaped text label, and waits for keyboard / mouse input:
//!
//! - **Space** / **Enter**: keyboard-activate the button (mirrors
//!   WAI-ARIA Authoring Practices Button keyboard pattern). The
//!   SCXML statechart raises the internal `button.activate` event
//!   without changing visible state; `Button::detect` emits a
//!   `"click"` intent and the shell logs it to stderr.
//! - **Mouse click on the `[ Click me! ]` cells**: pointer-driven
//!   click (R51.112). The substrate's `InputRouter` resolves the
//!   cell coord against the button's tag, dispatches
//!   `PointerEnter` / `PointerDown` / `PointerUp` to the SCXML
//!   statechart, and the `Pressed → Hover` transition emits the
//!   same `"click"` intent the keyboard path produces.
//! - **d** / **e**: disable / enable the button. The visual state
//!   flips between `[ Click me! ]` and `[ Disabled ]`; while
//!   disabled, Space / Enter and mouse clicks are silently absorbed
//!   (ARIA spec).
//! - **Esc**: graceful exit (RAII guard restores the terminal +
//!   disables mouse capture).
//!
//! Resize triggers a repaint at the new dimensions; paste / focus
//! events are ignored (R51.113+ carries those once a binding
//! surfaces the trigger).
//!
//! This dogfood validates:
//! - pinion-tui's substrate compiles + links against a real
//!   crossterm-backed terminal.
//! - `paint::to_buffer` renders `TextNode` content at pixel→cell
//!   coords matching the substrate's `PIXEL_PER_CELL_*` constants.
//! - The RAII terminal restore guard handles both `Esc` exit and
//!   panic cleanup.
//! - **R51.111**: real SCXML Button statechart drives the cached
//!   state; keyboard activation round-trips through the §5.15
//!   `invoke("send", Text(<name>))` channel and the §5.20 intent
//!   drain harvests the resulting `"click"` event.
//!
//! Carry forward to R51.113+:
//! - Multi-widget focus management once a second focusable TUI
//!   binding lands ([[substrate-incompleteness-signal]] trigger).
//! - TUI a11y (PTY screen reader path or AccessKit-TUI).
//! - `Backend::Tui` axis on `External::backends` (today the binding
//!   declares `Backend::Gui` because no dedicated TUI flag exists
//!   in the §5.15 backend taxonomy).

use std::io::Stdout;

use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{Frame, style};
use pinion_tui::ratatui::backend::CrosstermBackend;
use pinion_tui::{TuiRenderer, WidgetViewTui};

/// The widget binding unit type. `pinion_tui::run::<HelloButtonTui>()`
/// instantiates the substrate around this binding.
struct HelloButtonTui;

impl WidgetViewTui for HelloButtonTui {
    /// R51.111 — cached projection of the SCXML widget's
    /// [`ButtonState`]. The shell's `read_state` hook lifts this
    /// from the live `Scene::External` each frame; on every
    /// `Space` / `Enter` keypress the state may transition (Idle/
    /// Hover/Pressed/Disabled) and the substrate repaints.
    type State = ButtonState;

    /// The shell drives typed events through
    /// [`WidgetViewTui::keybinding`]; this binding's `d` / `e`
    /// shortcuts produce raw `ButtonEvent` variants, while
    /// `Space` / `Enter` go through [`apply_key`] (the W3C-named
    /// keyboard activation path).
    type Event = pinion_core::widgets::button::ButtonEvent;

    type Renderer = TuiRenderer<CrosstermBackend<Stdout>>;

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn tag() -> &'static str {
        "hello_button_tui"
    }

    /// Lift the SCXML widget's current state through the §5.15
    /// introspect channel — same path the JSON-RPC
    /// `scene/query /external/<slot>/state` route uses.
    fn read_state(scene: &Scene) -> Self::State {
        if let Scene::External(node) = scene
            && let Some(intro) = node.handle.introspect()
            && let Some(IntrospectValue::Text(name)) = intro.query("state")
        {
            return parse_button_state(&name);
        }
        ButtonState::Idle
    }

    fn view(state: Self::State, _frame: &Frame) -> Scene {
        // R51.111 — paint a different label per state so a
        // keystroke's effect is visually verifiable in the live
        // terminal.
        // R51.112 — wrap the label in an inner `Container` carrying
        // the binding's tag so the substrate's `InputRouter`
        // hit-tests only the button's visual surface, not the entire
        // outer background. Mouse coords outside this inner rect
        // resolve to no widget (the substrate's free-mode default),
        // matching the Vello hello-button's "button-shaped click
        // area" UX.

        let label_str: &'static str = match state {
            ButtonState::Idle => "[ Click me!  ]",
            ButtonState::Hover => "[ Hovered    ]",
            ButtonState::Pressed => "[ PRESSED    ]",
            ButtonState::Disabled => "[ Disabled   ]",
        };

        let mut label = TextNode::default();
        label_str.clone_into(&mut label.content);
        // Label rect = pixel (16..128, 32..48) = cell (2..16, 2..3).
        // 14 cells wide, 1 row tall — covers the 14-char label.
        label.rect = Rect::new(16, 32, 112, 16);
        label.style = style::TextStyle::default();

        let mut button = ContainerNode::default();
        // Button rect matches the label rect exactly so the
        // InputRouter's hit-test maps clicks on the visible label
        // cells to the SCXML statechart.
        button.rect = Rect::new(16, 32, 112, 16);
        button.tag = Some(std::borrow::Cow::Borrowed(Self::tag()));
        button.children.push(Scene::Text(label));

        let mut hint = TextNode::default();
        "Space/Enter/click = activate, d/e = disable/enable, Esc = quit"
            .clone_into(&mut hint.content);
        hint.rect = Rect::new(16, 80, 512, 16);
        hint.style = style::TextStyle::default();

        let mut container = ContainerNode::default();
        container.rect = Rect::new(0, 0, 640, 240);
        container.children.push(Scene::Container(button));
        container.children.push(Scene::Text(hint));

        Scene::Container(container)
    }

    fn event_name(event: Self::Event) -> &'static str {
        use pinion_core::widgets::button::ButtonEvent;
        match event {
            ButtonEvent::PointerEnter => "PointerEnter",
            ButtonEvent::PointerLeave => "PointerLeave",
            ButtonEvent::PointerDown => "PointerDown",
            ButtonEvent::PointerUp => "PointerUp",
            ButtonEvent::PointerCancel => "PointerCancel",
            ButtonEvent::KeyboardActivate => "KeyboardActivate",
            ButtonEvent::Disable => "Disable",
            ButtonEvent::Enable => "Enable",
            // SCXML-internal variants the crossterm input bridge
            // never produces (the wildcard absorbs future
            // `#[non_exhaustive]` additions defensively).
            _ => "__internal__",
        }
    }

    fn title() -> &'static str {
        "pinion hello-button-tui (R51.111 §5.41 input dispatch)"
    }

    /// `d` disables, `e` re-enables — mirrors the Vello
    /// hello-button binding so the same authoring pattern works on
    /// both backends.
    fn keybinding(key: &str) -> Option<Self::Event> {
        use pinion_core::widgets::button::ButtonEvent;
        match key {
            "d" => Some(ButtonEvent::Disable),
            "e" => Some(ButtonEvent::Enable),
            _ => None,
        }
    }

    /// R51.111 §5.41 — ARIA Button keyboard activation. Space /
    /// Enter on the focused button fires a `KeyboardActivate`
    /// SCXML event; `Button::detect` emits a `"click"` intent the
    /// shell logs to stderr. `Disabled` ignores activation per the
    /// ARIA spec (the SCXML transition is absent from that state).
    ///
    /// Identical surface to the Vello hello-button's `apply_key`
    /// — once a multi-binding TUI shell lands, both backends share
    /// this impl through the generic `WidgetView` merge.
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
        if focused != Some(Self::tag()) {
            return false;
        }
        if !matches!(key, "Space" | "Enter") {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        intro
            .invoke("send", IntrospectValue::Text("KeyboardActivate".to_string()))
            .is_ok()
    }
}

fn parse_button_state(name: &str) -> ButtonState {
    match name {
        "Hover" => ButtonState::Hover,
        "Pressed" => ButtonState::Pressed,
        "Disabled" => ButtonState::Disabled,
        // "Idle" + anything unexpected — defensive default.
        _ => ButtonState::Idle,
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
