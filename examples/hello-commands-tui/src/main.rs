//! `hello-commands-tui` — R51.170 §5.23 R27 §2 #6 TUI sibling of
//! `hello-commands`.
//!
//! Demonstrates the §5.23 R27 reducer-driven Command dispatch
//! pipeline on the TUI backend (R51.170 dogfood), mirror of the
//! Vello [`hello-commands`](../hello-commands) binary:
//!
//! ```text
//! button press
//!   → ButtonExternal SCXML transition → click intent drains
//!     → ShellCoreTui::handle_tail (R51.169)
//!       → CoreShell::route_intent_through_update (R51.167)
//!         → HelloCommandsTui::update — matches "hello_commands_tui.click"
//!         → returns vec![Command("demo.echo", "hello pinion (tui)")]
//!         → root_owner.dispatch_command queues it
//!       → CoreShell::dispatch_pending_commands (R51.157 drain pump)
//!         → CommandExecutor::dispatch (R51.156)
//!           → TokioExecutor::spawn → tokio worker (R51.160 binding)
//!             → echo Handler resolves (+200ms sleep) → Intent
//!             → MpscIntentSink → shell.run loop try_recv
//!               → ShellCoreTui::dispatch_intent
//!                 → CoreShell::route_intent_through_update — no match
//!                 → SCXML invoke("send", "echo.demo.echo")
//! ```
//!
//! ## Why a separate TUI binary
//!
//! §2 invariant #6 (GUI/TUI dual: one scene, two render dispatch
//! paths). The Vello sibling exercises the
//! [`pinion_shell::run_with_handlers`] +
//! [`pinion_shell::ProxyIntentSink`] path; this binary exercises the
//! [`pinion_tui::run_with_handlers`] +
//! [`pinion_tui::MpscIntentSink`] path. Both backends route the same
//! [`Command`] through the same registry-based dispatch surface, with
//! only the runtime wake mechanism differing (winit `EventLoopProxy`
//! vs. `crossterm` poll + `mpsc::Receiver::try_recv`).
//!
//! ## Trace observation
//!
//! Under TUI raw-mode + alternate-screen, `stderr` corrupts the
//! visible buffer ([[r47-class-incident-prevention]] + the R51.120
//! alternate-screen anti-pattern). Set `PINION_TUI_LOG=/path/to/log`
//! before running to route all substrate traces (intent / state /
//! command-unhandled / intent-feedback) to a file:
//!
//! ```text
//! PINION_TUI_LOG=/tmp/cmd.log cargo run -p hello-commands-tui &
//! tail -f /tmp/cmd.log
//! ```
//!
//! Expected log lines:
//! ```text
//! tui: intent-feedback echo.demo.echo payload=Text("hello pinion")
//! ```
//!
//! Press `Esc` to quit.

use std::io::Stdout;
use std::sync::Arc;

use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{Border, BoxStyle};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Color, Command, Frame, Intent, WidgetCore, style};
use pinion_runtime::{Handler, HandlerFuture, HandlerRegistry};
use pinion_tui::ratatui::backend::CrosstermBackend;
use pinion_tui::{TuiRenderer, WidgetViewTui};

/// R51.170 §5.23 R27 — kind tag for the demo Command emitted by
/// [`HelloCommandsTui::update`] when the button click intent
/// arrives. Matches the handler registration in
/// [`build_handler_registry`].
const DEMO_KIND: &str = "demo.echo";

/// R51.170 §5.23 R27 — fixed payload the reducer attaches to every
/// `demo.echo` Command. Distinct from the Vello sibling so a
/// `PINION_TUI_LOG=path` trace makes the backend obvious.
const DEMO_PAYLOAD: &str = "hello pinion (tui)";

/// R51.170 §5.23 R27 — intent tag the reducer matches against. The
/// Button SCXML emits `click` on `PointerUp`; the §5.20 R22
/// `<widget_tag>.<kind>` prefix convention makes the full
/// wire-form tag `hello_commands_tui.click`
/// (`V::tag()` = `"hello_commands_tui"`).
const CLICK_INTENT_TAG: &str = "hello_commands_tui.click";

struct HelloCommandsTui;

impl WidgetCore for HelloCommandsTui {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn tag() -> &'static str {
        "hello_commands_tui"
    }

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
        let label_str: &'static str = match state {
            ButtonState::Idle => "Click for cmd flow",
            ButtonState::Hover => "Hovered",
            ButtonState::Pressed => "PRESSED",
            ButtonState::Disabled => "Disabled",
        };

        let mut label = TextNode::default();
        label_str.clone_into(&mut label.content);
        label.rect = Rect::new(32, 48, 88, 16);
        label.style = style::TextStyle::default();

        let mut button = ContainerNode::default();
        button.rect = Rect::new(16, 32, 128, 48);
        button.tag = Some(std::borrow::Cow::Borrowed(Self::tag()));
        let bg_fill = match state {
            ButtonState::Idle | ButtonState::Hover => Color::rgb(0xe0, 0xe0, 0xe8),
            ButtonState::Pressed => Color::rgb(0x50, 0x50, 0x50),
            ButtonState::Disabled => Color::rgb(0xb0, 0x20, 0x20),
        };
        let border_color = match state {
            ButtonState::Pressed | ButtonState::Disabled => Color::rgb(0xe0, 0xe0, 0xe0),
            _ => Color::rgb(0x40, 0x40, 0x40),
        };
        button.style =
            BoxStyle::filled(bg_fill).with_border(Border::new(border_color, 1));
        button.children.push(Scene::Text(label));

        let mut hint = TextNode::default();
        "PINION_TUI_LOG=/tmp/cmd.log to see traces, Esc = quit"
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
        match event {
            ButtonEvent::PointerEnter => "PointerEnter",
            ButtonEvent::PointerLeave => "PointerLeave",
            ButtonEvent::PointerDown => "PointerDown",
            ButtonEvent::PointerUp => "PointerUp",
            ButtonEvent::PointerCancel => "PointerCancel",
            ButtonEvent::KeyboardActivate => "KeyboardActivate",
            ButtonEvent::Disable => "Disable",
            ButtonEvent::Enable => "Enable",
            _ => "__internal__",
        }
    }

    fn title() -> &'static str {
        "pinion hello-commands-tui (R51.170 §5.23 R27 §2#6 dual)"
    }

    fn keybinding(key: &str) -> Option<Self::Event> {
        match key {
            "d" => Some(ButtonEvent::Disable),
            "e" => Some(ButtonEvent::Enable),
            _ => None,
        }
    }

    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }

    fn update(_state: Self::State, intent: &Intent) -> Vec<Command> {
        // R51.170 §5.23 R27 — TUI mirror of the Vello reducer
        // dogfood. R51.169 handle_tail routing pumps the drained
        // click through this reducer so the dispatch loop runs
        // without the pre-R51.170 view-fn one-shot HACK.
        //
        // R51.171 §5.22 R26 — `Owner::current()` resolves to the
        // substrate's root owner (route_intent_through_update wraps
        // the call in `root_owner.run(...)`); scope_id tags the
        // Command for `scene/commands` RPC inspection.
        //
        // R51.174 §5.23 R27 — Elm/Iced canonical Update reducer
        // shape: `match` over `intent.tag_str()` so each intent maps
        // to its arm; the wildcard arm explicitly opts out.
        match intent.tag_str() {
            CLICK_INTENT_TAG => {
                let scope_id = pinion_core::Owner::current().map_or(0, |o| o.id());
                vec![Command::new_static(
                    DEMO_KIND,
                    IntrospectValue::Text(DEMO_PAYLOAD.to_string()),
                    scope_id,
                )]
            }
            _ => Vec::new(),
        }
    }
}

impl WidgetA11y for HelloCommandsTui {
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

impl WidgetViewTui for HelloCommandsTui {
    type Renderer = TuiRenderer<CrosstermBackend<Stdout>>;
}

fn parse_button_state(name: &str) -> ButtonState {
    match name {
        "Hover" => ButtonState::Hover,
        "Pressed" => ButtonState::Pressed,
        "Disabled" => ButtonState::Disabled,
        _ => ButtonState::Idle,
    }
}

/// R51.164 §5.23 — application-supplied [`Handler`] for `demo.echo`.
/// Echoes its payload back as `Intent("echo.demo.echo", payload)`.
///
/// R51.165 §5.23 — sleeps 200ms before echoing so the
/// `intent-feedback` log line (when `PINION_TUI_LOG=path` is set)
/// shows up ~200ms after the launch trace, proving the future
/// actually suspended at the `.await` rather than resolving
/// synchronously. Silent (no eprintln from the worker thread —
/// same raw-mode + alternate-screen reasoning as the view fn).
fn echo_handler() -> Arc<dyn Handler> {
    Arc::new(|cmd: Command| -> HandlerFuture {
        Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            Intent::new_owned(format!("echo.{}", cmd.kind_str()), cmd.payload)
        })
    })
}

fn build_handler_registry() -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();
    registry.register(DEMO_KIND, echo_handler());
    registry
}

fn main() {
    if let Err(e) =
        pinion_tui::run_with_handlers::<HelloCommandsTui>(build_handler_registry())
    {
        eprintln!("hello-commands-tui: shell error: {e}");
        std::process::exit(1);
    }
}
