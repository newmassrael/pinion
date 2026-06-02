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

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{Border, BoxStyle};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{Color, Frame, WidgetCore, WidgetStateName, style};
use pinion_tui::ratatui::backend::CrosstermBackend;
use pinion_tui::{TuiRenderer, WidgetViewTui};

/// The widget binding unit type. `pinion_tui::run::<HelloToggleTui>()`
/// instantiates the substrate around this binding.
struct HelloToggleTui;

impl WidgetCore for HelloToggleTui {
    /// Joint `(interaction, value)` — same shape as the Vello
    /// `hello-toggle` binding. The substrate's `read_state` lifts
    /// both fields through the §5.15 introspect channel each frame.
    type State = (ToggleState, bool);

    type Event = ToggleEvent;

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
                ToggleState::from_name_or_default(&name)
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
        // R51.115 / R51.116 — Toggle visualised as a bordered cell
        // pill carrying an `OFF` / `ON` label, with the background
        // colour encoding the (interaction, value) cross product
        // (mirrors the Vello hello-toggle's track colour scheme).

        // Track colour cross product (state × value). Off column =
        // greyscale, On column = green accent (system "active"
        // affordance). Pressed darkens; Disabled is muted brown-grey.
        let bg_fill: Color = match (interaction, on) {
            (ToggleState::Idle, false) => Color::rgb(0x40, 0x40, 0x40),
            (ToggleState::Hover, false) => Color::rgb(0x55, 0x55, 0x55),
            (ToggleState::Pressed, false) => Color::rgb(0x30, 0x30, 0x30),
            (ToggleState::Idle, true) => Color::rgb(0x30, 0xa0, 0x50),
            (ToggleState::Hover, true) => Color::rgb(0x40, 0xb0, 0x60),
            (ToggleState::Pressed, true) => Color::rgb(0x20, 0x70, 0x40),
            (ToggleState::Disabled, _) => Color::rgb(0x4a, 0x42, 0x38),
        };
        let border_color = Color::rgb(0xe0, 0xe0, 0xe0);

        let label_str: &'static str = match (interaction, on) {
            (ToggleState::Disabled, _) => " DIS ",
            (_, false) => " OFF ",
            (_, true) => " ON  ",
        };

        let mut label = TextNode::default();
        label_str.clone_into(&mut label.content);
        // Label sits inside the toggle border:
        // toggle rect cell (2..12, 2..5) = pixel (16..96, 32..80);
        // label rect cell (3..8, 3..4) = pixel (24..64, 48..64).
        label.rect = Rect::new(24, 48, 40, 16);
        label.style = style::TextStyle::default();

        let mut toggle_box = ContainerNode::default();
        // Toggle rect = 10 cells × 3 rows so the border has room.
        toggle_box.rect = Rect::new(16, 32, 80, 48);
        toggle_box.tag = Some(Cow::Borrowed(Self::tag()));
        toggle_box.style =
            BoxStyle::filled(bg_fill).with_border(Border::new(border_color, 1));
        toggle_box.children.push(Scene::Text(label));

        let mut status = TextNode::default();
        let status_str = format!(
            "state: {} | value: {}",
            interaction.as_name(),
            if on { "On" } else { "Off" },
        );
        status_str.clone_into(&mut status.content);
        status.rect = Rect::new(16, 96, 400, 16);
        status.style = style::TextStyle::default();

        let mut hint = TextNode::default();
        "Space/Enter/click = toggle, d/e = disable/enable, Esc = quit"
            .clone_into(&mut hint.content);
        hint.rect = Rect::new(16, 128, 480, 16);
        hint.style = style::TextStyle::default();

        let mut container = ContainerNode::default();
        container.rect = Rect::new(0, 0, 640, 240);
        container.children.push(Scene::Container(toggle_box));
        container.children.push(Scene::Text(status));
        container.children.push(Scene::Text(hint));

        Scene::Container(container)
    }

    fn event_name(event: Self::Event) -> &'static str {
        // R699 §5.16 — route the forward Event->name mapping through the
        // WidgetEventName SSOT (`as_name`), retiring the hand-written
        // match table. `as_name` is total over internal variants too;
        // only external events reach this path via `ShellCore::forward`.
        pinion_core::WidgetEventName::as_name(&event)
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
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, _modifiers: pinion_core::Modifiers) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }
}

impl WidgetA11y for HelloToggleTui {
    /// R51.118 §5.41 — AT-side semantic node (TUI parity with the
    /// Vello hello-toggle binding). `AriaRole::Switch` carries the
    /// On/Off value via [`AccessValue::Bool`]; `state.checked`
    /// mirrors the same boolean so AT clients reading either field
    /// see a consistent on/off state.
    fn access_node(
        state: &(ToggleState, bool),
        focused: Option<&str>,
    ) -> Vec<AccessNode> {
        let (interaction, on) = *state;
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            ..AccessState::from_interaction(interaction, Some(on))
        };
        vec![AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Switch)
            .with_value(AccessValue::Bool(on))
            .with_state(access_state)]
    }
}

impl WidgetViewTui for HelloToggleTui {
    type Renderer = TuiRenderer<CrosstermBackend<Stdout>>;
}

fn main() {
    if let Err(e) = pinion_tui::run::<HelloToggleTui>() {
        eprintln!("hello-toggle-tui: shell error: {e}");
        std::process::exit(1);
    }
}
