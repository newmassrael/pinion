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

use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::animation::SpringConfig;
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{Border, BoxStyle};
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{Animation, Color, Frame, Owner, WidgetCore, style};
use pinion_tui::ratatui::backend::CrosstermBackend;
use pinion_tui::{TuiRenderer, WidgetViewTui};

/// R51.150 §5.22 — owner-cache key for the TUI hover-progress
/// [`Animation`]. Distinct prefix from the Vello sibling
/// (`hello_button::*`) so the two examples can run side-by-side under
/// future shared infrastructure without key collision.
const HOVER_ANIM_KEY: &str = "hello_button_tui::hover_progress";

/// R51.148 §5.28 + R51.150 §5.22 — drive the hover progress animation
/// and return the displayed value in `[0.0, 1.0]`. Hover targets
/// `1.0`; every other state targets `0.0`. Mirrors the Vello
/// `hello-button` pattern verbatim, with the same R51.150 owner-cache
/// replacement of the pre-R51.150 `thread_local OnceCell` workaround
/// (see `hello-button/src/main.rs` for the long-form rationale).
fn drive_hover_progress(state: ButtonState) -> f32 {
    let owner = Owner::current().expect(
        "hello-button-tui view fn must run inside ShellCoreTui::root_owner().run(...)",
    );
    let anim: std::rc::Rc<Animation<f32>> = owner.cache(HOVER_ANIM_KEY, || {
        Animation::new(&owner, 0.0_f32, SpringConfig::default())
    });
    let target = if matches!(state, ButtonState::Hover) { 1.0 } else { 0.0 };
    anim.set_target(target);
    anim.value()
}

/// R51.151 §5.28 — idle (white) and hover (gray) lightness endpoints
/// for the Idle ↔ Hover spring fade. Mirror of `hello-button` (Vello
/// sibling) so the two backends paint the same gradient. The
/// terminal's truecolor path (24-bit ANSI) lands the linear-space
/// gradient on modern terminals (kitty / alacritty / foot /
/// windows-terminal); legacy 16-colour terminals collapse the
/// gradient to nearest-palette steps.
const BTN_FILL_IDLE: Color = Color::rgb(0xff, 0xff, 0xff);
const BTN_FILL_HOVER: Color = Color::rgb(0xd0, 0xd0, 0xd0);

/// The widget binding unit type. `pinion_tui::run::<HelloButtonTui>()`
/// instantiates the substrate around this binding.
struct HelloButtonTui;

impl WidgetCore for HelloButtonTui {
    /// R51.111 — cached projection of the SCXML widget's
    /// [`ButtonState`]. The shell's `read_state` hook lifts this
    /// from the live `Scene::External` each frame; on every
    /// `Space` / `Enter` keypress the state may transition (Idle/
    /// Hover/Pressed/Disabled) and the substrate repaints.
    type State = ButtonState;

    /// The shell drives typed events through
    /// [`WidgetCore::keybinding`]; this binding's `d` / `e`
    /// shortcuts produce raw `ButtonEvent` variants, while
    /// `Space` / `Enter` go through [`apply_key`] (the W3C-named
    /// keyboard activation path).
    type Event = pinion_core::widgets::button::ButtonEvent;

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
        // R51.111 — paint a different label per state.
        // R51.112 — wrap the label in an inner `Container` carrying
        // the binding's tag so the substrate's `InputRouter`
        // hit-tests only the button's visual surface.
        // R51.115 / R51.116 — apply a `BoxStyle` to the button
        // container: state-coloured background fill + a single-cell
        // light box-drawing border. Now the visible state matches
        // the Vello hello-button's colour scheme on a terminal:
        // white-ish Idle, light-grey Hover, dark Pressed, red
        // Disabled.

        // Button colour scheme — same RGB triples the Vello
        // hello-button uses so the two backends paint with visual
        // parity.
        //
        // R51.148 §5.28 — Idle↔Hover lerps via a spring-driven
        // progress value (0.0 = Idle, 1.0 = full Hover) so the
        // transition is smooth in truecolor terminals; Pressed and
        // Disabled retain their discrete fills (no animation; the
        // shell's adaptive `poll_timeout` keeps the substrate idle
        // once the spring settles).
        let hover_progress = drive_hover_progress(state);
        let bg_fill: Color = match state {
            ButtonState::Idle | ButtonState::Hover => {
                BTN_FILL_IDLE.lerp(BTN_FILL_HOVER, hover_progress)
            }
            ButtonState::Pressed => Color::rgb(0x50, 0x50, 0x50),
            ButtonState::Disabled => Color::rgb(0xb0, 0x20, 0x20),
        };
        let border_color: Color = match state {
            ButtonState::Pressed | ButtonState::Disabled => {
                Color::rgb(0xe0, 0xe0, 0xe0)
            }
            _ => Color::rgb(0x40, 0x40, 0x40),
        };

        let label_str: &'static str = match state {
            ButtonState::Idle => "Click me!",
            ButtonState::Hover => "Hovered",
            ButtonState::Pressed => "PRESSED",
            ButtonState::Disabled => "Disabled",
        };

        let mut label = TextNode::default();
        label_str.clone_into(&mut label.content);
        // Label sits inside the button border:
        // button rect cell (2..18, 2..5) = pixel (16..144, 32..80);
        // label rect cell (4..15, 3..4) = pixel (32..120, 48..64).
        label.rect = Rect::new(32, 48, 88, 16);
        label.style = style::TextStyle::default();

        let mut button = ContainerNode::default();
        // Button rect = 16 cells × 3 rows so the border has room
        // for distinct corners + edges + interior text.
        button.rect = Rect::new(16, 32, 128, 48);
        button.tag = Some(std::borrow::Cow::Borrowed(Self::tag()));
        button.style = BoxStyle::filled(bg_fill).with_border(Border::new(border_color, 1));
        button.children.push(Scene::Text(label));

        let mut hint = TextNode::default();
        "Space/Enter/click = activate, d/e = disable/enable, Esc = quit"
            .clone_into(&mut hint.content);
        hint.rect = Rect::new(16, 96, 512, 16);
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
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }
}

impl WidgetA11y for HelloButtonTui {
    /// R51.118 §5.41 — AT-side semantic node (TUI parity with the
    /// Vello hello-button binding). Same `AriaRole::Button` + state
    /// flags shape; consumed by the future PTY screen reader /
    /// AccessKit-TUI integration once the §5.41 a11y adapter lands.
    fn access_node(state: &ButtonState, focused: Option<&str>) -> Vec<AccessNode> {
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            disabled: matches!(state, ButtonState::Disabled),
            hovered: matches!(state, ButtonState::Hover),
            pressed: matches!(state, ButtonState::Pressed),
            checked: None,
        };
        vec![
            AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Button)
                .with_state(access_state),
        ]
    }
}

impl WidgetViewTui for HelloButtonTui {
    type Renderer = TuiRenderer<CrosstermBackend<Stdout>>;
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
