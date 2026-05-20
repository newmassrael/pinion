//! R51.113 §5.41 — second hello-toggle TUI dogfood.
//!
//! The TUI sibling of `examples/hello-toggle`. Where R51.110-R51.112
//! validated the substrate with a single binding (`hello-button-tui`),
//! this binary is the **second** interactive TUI consumer — it
//! surfaces the substrate-incompleteness-signal trigger
//! ([[substrate-incompleteness-signal]]) for follow-on refactors:
//!
//! - **DRY between `WidgetView` (Vello) and `WidgetViewTui` (TUI)**:
//!   both `hello-toggle/main.rs` and this file declare nearly the
//!   same `apply_key` / `keybinding` / `event_name` impls (the only
//!   diverging field is `Renderer`). R51.114+ evaluates a single
//!   generic `WidgetView<R: WidgetRenderer>` trait now that the
//!   second TUI binding exists as concrete evidence.
//! - **Shell-side dispatch helpers**: `pinion_tui::shell::dispatch_key`
//!   replicates `pinion_shell::ShellCore::handle_character_key`.
//!   The second binding confirms the duplication is structural —
//!   lifting `ShellCore` into a backend-agnostic crate is the
//!   textbook follow-up.
//! - **Cell-native coord substrate**: this binding still routes
//!   through `PIXEL_PER_CELL_*` placeholders. A real cell mismatch
//!   here would trigger the cell-native axis.
//!
//! Run:
//!
//! ```bash
//! cargo run -p hello-toggle-tui
//! ```
//!
//! The terminal switches to the alternate screen, paints the
//! toggle's current `(state, on)` cross product as a text-rendered
//! switch, and waits for keyboard / mouse input:
//!
//! - **Space** / **Enter**: activate the toggle (WAI-ARIA Toggle
//!   Button keyboard pattern). Flips Off ↔ On and emits the
//!   `"toggle"` intent on each activate edge.
//! - **Mouse click on the `[ OFF ]` / `[ ON  ]` cells**: pointer
//!   activation. `Pressed → Hover` on `PointerUp` flips the value
//!   and emits the same intent.
//! - **d** / **e**: disable / enable the toggle. While disabled
//!   the visible label switches to `[ DIS ]` and the activate
//!   paths are silently absorbed (ARIA spec).
//! - **Esc**: graceful exit (RAII guard restores the terminal +
//!   disables mouse capture).
//!
//! The text representation is a `[ OFF ]` / `[ ON  ]` pair so the
//! state difference is visible without ANSI colour (R51.110-R51.112
//! paint walker is monochrome; colour styling lands once
//! `Scene::Box.style.fill` reaches the TUI mapping).

use std::borrow::Cow;
use std::io::Stdout;

use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{Frame, style};
use pinion_tui::ratatui::backend::CrosstermBackend;
use pinion_tui::{TuiRenderer, WidgetViewTui};

/// The widget binding unit type. `pinion_tui::run::<HelloToggleTui>()`
/// instantiates the substrate around this binding.
struct HelloToggleTui;

impl WidgetViewTui for HelloToggleTui {
    /// Joint `(interaction, value)` — same shape as the Vello
    /// `hello-toggle` binding. The substrate's `read_state` lifts
    /// both fields through the §5.15 introspect channel each frame.
    type State = (ToggleState, bool);

    type Event = ToggleEvent;

    type Renderer = TuiRenderer<CrosstermBackend<Stdout>>;

    fn create_external() -> Box<dyn External> {
        Box::new(ToggleExternal::new())
    }

    fn tag() -> &'static str {
        "hello_toggle_tui"
    }

    fn read_state(scene: &Scene) -> Self::State {
        if let Scene::External(node) = scene
            && let Some(intro) = node.handle.introspect()
        {
            let state = if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                parse_toggle_state(&name)
            } else {
                ToggleState::Idle
            };
            let value =
                matches!(intro.query("value"), Some(IntrospectValue::Bool(true)));
            return (state, value);
        }
        (ToggleState::Idle, false)
    }

    fn view(state: Self::State, _frame: &Frame) -> Scene {
        let (interaction, on) = state;
        // Label encodes the (interaction, value) cross product as
        // ASCII so the substrate's text-only paint walker
        // (R51.110.0 cut) can render every variant. The constant
        // 7-character width keeps the `Container.rect` hit-test
        // stable across state transitions — the click target stays
        // anchored at the same cells whichever variant is showing.
        let label_str: &'static str = match (interaction, on) {
            (ToggleState::Disabled, _) => "[ DIS ]",
            (ToggleState::Pressed, false) => "[ off ]",
            (ToggleState::Pressed, true) => "[ on  ]",
            (ToggleState::Hover, false) => "[<OFF>]",
            (ToggleState::Hover, true) => "[< ON>]",
            (_, false) => "[ OFF ]",
            (_, true) => "[ ON  ]",
        };

        let mut label = TextNode::default();
        label_str.clone_into(&mut label.content);
        // Label rect = pixel (16..72, 32..48) = cell (2..9, 2..3).
        // 7 cells wide; the inner Container hit-test matches exactly.
        label.rect = Rect::new(16, 32, 56, 16);
        label.style = style::TextStyle::default();

        let mut toggle_box = ContainerNode::default();
        toggle_box.rect = Rect::new(16, 32, 56, 16);
        toggle_box.tag = Some(Cow::Borrowed(Self::tag()));
        toggle_box.children.push(Scene::Text(label));

        let mut status = TextNode::default();
        let status_str = format!(
            "state: {} | value: {}",
            toggle_state_name(interaction),
            if on { "On" } else { "Off" },
        );
        status_str.clone_into(&mut status.content);
        status.rect = Rect::new(16, 64, 400, 16);
        status.style = style::TextStyle::default();

        let mut hint = TextNode::default();
        "Space/Enter/click = toggle, d/e = disable/enable, Esc = quit"
            .clone_into(&mut hint.content);
        hint.rect = Rect::new(16, 96, 480, 16);
        hint.style = style::TextStyle::default();

        let mut container = ContainerNode::default();
        container.rect = Rect::new(0, 0, 640, 240);
        container.children.push(Scene::Container(toggle_box));
        container.children.push(Scene::Text(status));
        container.children.push(Scene::Text(hint));

        Scene::Container(container)
    }

    fn event_name(event: Self::Event) -> &'static str {
        match event {
            ToggleEvent::PointerEnter => "PointerEnter",
            ToggleEvent::PointerLeave => "PointerLeave",
            ToggleEvent::PointerDown => "PointerDown",
            ToggleEvent::PointerUp => "PointerUp",
            ToggleEvent::PointerCancel => "PointerCancel",
            ToggleEvent::KeyboardActivate => "KeyboardActivate",
            ToggleEvent::Disable => "Disable",
            ToggleEvent::Enable => "Enable",
            _ => "__internal__",
        }
    }

    fn title() -> &'static str {
        "pinion hello-toggle-tui (R51.113 §5.41 second TUI binding)"
    }

    fn keybinding(key: &str) -> Option<Self::Event> {
        match key {
            "d" => Some(ToggleEvent::Disable),
            "e" => Some(ToggleEvent::Enable),
            _ => None,
        }
    }

    /// ARIA Toggle Button keyboard activation. Both `Space` and
    /// `Enter` flip Off ↔ On (toggle buttons accept both; pure ARIA
    /// checkboxes accept only `Space` — Toggle is a toggle button
    /// per WAI-ARIA APG so both keys land here).
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

fn parse_toggle_state(name: &str) -> ToggleState {
    match name {
        "Hover" => ToggleState::Hover,
        "Pressed" => ToggleState::Pressed,
        "Disabled" => ToggleState::Disabled,
        _ => ToggleState::Idle,
    }
}

fn toggle_state_name(state: ToggleState) -> &'static str {
    match state {
        ToggleState::Idle => "Idle",
        ToggleState::Hover => "Hover",
        ToggleState::Pressed => "Pressed",
        ToggleState::Disabled => "Disabled",
    }
}

fn main() {
    if let Err(e) = pinion_tui::run::<HelloToggleTui>() {
        eprintln!("hello-toggle-tui: shell error: {e}");
        std::process::exit(1);
    }
}
