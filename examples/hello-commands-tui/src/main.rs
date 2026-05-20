//! `hello-commands-tui` — R51.164 §5.23 §2 #6 TUI sibling of
//! `hello-commands`.
//!
//! Demonstrates the §5.23 R27 Command dispatch pipeline on the TUI
//! backend, mirror of the Vello [`hello-commands`](../hello-commands)
//! binary:
//!
//! ```text
//! view fn  (one-shot)
//!   → Owner::current().dispatch_command(...)        [R51.139 substrate]
//!     → CoreShell::dispatch_pending_commands        [R51.157 drain pump]
//!       → CommandExecutor::dispatch                  [R51.156 composite]
//!         → TokioExecutor::spawn → tokio worker      [R51.160 binding]
//!           → echo Handler resolves → Intent
//!           → MpscIntentSink → shell.run loop try_recv
//!             → ShellCoreTui::dispatch_intent
//!               → SCXML invoke("send", tag)         [R51.160 re-feed]
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

use std::cell::Cell;
use std::io::Stdout;
use std::sync::Arc;

use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{Border, BoxStyle};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Color, Command, Frame, Intent, Owner, WidgetCore, style};
use pinion_runtime::{Handler, HandlerFuture, HandlerRegistry};
use pinion_tui::ratatui::backend::CrosstermBackend;
use pinion_tui::{TuiRenderer, WidgetViewTui};

/// R51.164 §5.23 — owner-cache key for the once-only `demo.echo`
/// dispatch guard. Distinct prefix from the Vello sibling so the
/// two examples cannot collide under future shared infrastructure.
const ONE_SHOT_KEY: &str = "hello_commands_tui::initial_dispatch_guard";

/// R51.164 §5.23 — kind tag for the demo Command. Matches the
/// handler registration in [`build_handler_registry`] below.
const DEMO_KIND: &str = "demo.echo";

/// R51.164 §5.23 — fixed payload the view fn queues alongside the
/// one-shot Command.
const DEMO_PAYLOAD: &str = "hello pinion (tui)";

/// R51.164 §5.23 — idempotent one-shot guard. Identical shape to
/// the Vello sibling, only the cache key differs.
///
/// Silent (no eprintln) because the TUI shell runs under
/// raw-mode + alternate-screen — any `stderr` write corrupts the
/// visible buffer ([[r47-class-incident-prevention]]). The substrate
/// surfaces the queued command through its `log_sink` (R51.120
/// `PINION_TUI_LOG=path` opt-in) so the trace is still observable.
fn queue_one_shot_demo_command() {
    let owner = Owner::current().expect(
        "hello-commands-tui view fn must run inside ShellCoreTui::root_owner().run(...)",
    );
    let dispatched: std::rc::Rc<Cell<bool>> =
        owner.cache(ONE_SHOT_KEY, || Cell::new(false));
    if dispatched.get() {
        return;
    }
    owner.dispatch_command(Command::new_static(
        DEMO_KIND,
        IntrospectValue::Text(DEMO_PAYLOAD.to_string()),
        owner.id(),
    ));
    dispatched.set(true);
}

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
        queue_one_shot_demo_command();

        let label_str: &'static str = match state {
            ButtonState::Idle => "demo.echo queued",
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
        "pinion hello-commands-tui (R51.164 §5.23 §2#6 dual)"
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
/// Silent (no eprintln from the worker thread — same raw-mode +
/// alternate-screen reasoning as the view fn).
fn echo_handler() -> Arc<dyn Handler> {
    Arc::new(|cmd: Command| -> HandlerFuture {
        Box::pin(async move {
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
