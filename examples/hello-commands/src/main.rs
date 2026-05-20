//! `hello-commands` — R51.163 §5.23 dispatch loop demo.
//!
//! Sibling of [`hello-button`](../hello-button) that exercises the
//! §5.23 R27 Command pipeline end-to-end:
//!
//! ```text
//! view fn  (one-shot)
//!   → Owner::current().dispatch_command(...)        [R51.139 substrate]
//!     → CoreShell::dispatch_pending_commands        [R51.157 drain pump]
//!       → CommandExecutor::dispatch                  [R51.156 composite]
//!         → TokioExecutor::spawn → tokio worker      [R51.159 binding]
//!           → echo Handler resolves → Intent
//!           → ProxyIntentSink → AppEvent::IntentArrived
//!             → user_event arm → ShellCore::dispatch_intent
//!               → SCXML invoke("send", tag)         [R51.159 re-feed]
//! ```
//!
//! ## What this demo shows
//!
//! - The view fn queues a `demo.echo` [`Command`] on first paint
//!   through the [`Owner::cache`] idempotent guard
//!   ([[owner-cache-substrate]] R51.150). Subsequent paints observe
//!   the cached `dispatched` cell already `true` and skip the
//!   `dispatch_command` call — the queue stays single-shot.
//! - [`pinion_shell::run_with_handlers`] installs a tokio runtime +
//!   `EventLoopProxy`-backed [`IntentSink`] so the Handler's future
//!   resolves on a worker thread and the resolved [`Intent`] reaches
//!   the UI thread via [`AppEvent::IntentArrived`].
//! - The `demo.echo` Handler echoes its payload back as
//!   `Intent("echo.demo.echo", payload)`. The Vello shell's
//!   [`ShellCore::dispatch_intent`] then `invoke("send", tag)`s the
//!   tag through the SCXML — Button rejects unknown event names, so
//!   the visible widget state stays at Idle, but the stderr trace
//!   shows the full Command → Intent round-trip.
//!
//! ## Running
//!
//! ```text
//! cargo run -p hello-commands
//! ```
//!
//! Expected stderr (truncated):
//! ```text
//! shell: initial state = Idle
//! hello-commands: queued demo.echo on first paint (scope_id=N)
//! shell: hello-commands resumed (initial size 320x200) ...
//! handler: demo.echo received payload=Text("hello pinion")
//! shell: intent-feedback echo.demo.echo payload=Text("hello pinion")
//! ```
//!
//! Press `Esc` to quit.

use std::cell::Cell;
use std::sync::Arc;

use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Color, Command, Frame, Intent, Owner, Scene, WidgetCore};
use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_runtime::{Handler, HandlerFuture, HandlerRegistry};
use pinion_shell::{vello_renderer_impl, WidgetView};

// pinion-forge codegen output — defines `HelloCommandsRenderer` +
// `HelloCommandsRendererError`. Same emit template as hello-button,
// only the struct name differs (per app.pinion.xml).
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloCommandsRenderer, HelloCommandsRendererError);

const WIN_W: u32 = 320;
const WIN_H: u32 = 200;
const BG_FILL: Color = Color::rgb(0x18, 0x28, 0x38);
const BTN_W: u32 = 240;
const BTN_H: u32 = 80;
const BTN_FILL: Color = Color::rgb(0xe0, 0xe0, 0xe8);

/// R51.163 §5.23 — owner-scoped cache key for the once-only
/// `demo.echo` dispatch guard. `&'static str` per
/// [`Owner::cache`]'s contract.
const ONE_SHOT_KEY: &str = "hello_commands::initial_dispatch_guard";

/// R51.163 §5.23 — kind tag for the demo Command. Matches the
/// handler registration in [`build_handler_registry`] below.
const DEMO_KIND: &str = "demo.echo";

/// R51.163 §5.23 — fixed payload the view fn queues alongside the
/// one-shot Command. The Handler echoes this back as the Intent's
/// payload.
const DEMO_PAYLOAD: &str = "hello pinion";

/// R51.163 §5.23 — idempotent one-shot guard. Called from inside the
/// view fn on every paint; the first call queues a [`Command`] on
/// the binding's root [`Owner`], subsequent calls observe
/// `dispatched.get() == true` and skip the dispatch.
///
/// view-fn purity invariant: the function is sync, has no IO, and
/// the [`Owner::cache`] returns the same `Rc<Cell<bool>>` on every
/// call — the side effect is observable only through the
/// substrate's pending-command queue, not through the returned
/// `Scene`. `dry_run` (§2 #3) still skips actual `Command` dispatch;
/// it only collects the queue for AI inspection, matching the §5.23
/// R27 contract.
fn queue_one_shot_demo_command() {
    let owner = Owner::current().expect(
        "hello-commands view fn must run inside ShellCore::root_owner().run(...)",
    );
    let dispatched: std::rc::Rc<Cell<bool>> =
        owner.cache(ONE_SHOT_KEY, || Cell::new(false));
    if dispatched.get() {
        return;
    }
    let scope_id = owner.id();
    owner.dispatch_command(Command::new_static(
        DEMO_KIND,
        IntrospectValue::Text(DEMO_PAYLOAD.to_string()),
        scope_id,
    ));
    eprintln!(
        "hello-commands: queued {DEMO_KIND} on first paint (scope_id={scope_id})",
    );
    dispatched.set(true);
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ButtonState, _frame: &Frame) -> Scene {
    queue_one_shot_demo_command();
    let label = match state {
        ButtonState::Disabled => "Disabled",
        _ => "Watch stderr for command flow",
    };
    let label_text = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(Color::rgb(0, 0, 0)),
    ));
    let button = Scene::Container(
        ContainerNode::new(vec![label_text])
            .with_tag("main_btn")
            .with_style(BoxStyle::filled(BTN_FILL))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(BTN_W, BTN_H)),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![button])
            .with_style(BoxStyle::filled(BG_FILL))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

struct CommandsView;

impl WidgetCore for CommandsView {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn tag() -> &'static str {
        "main_btn"
    }

    fn read_state(scene: &Scene) -> ButtonState {
        if let Scene::External(node) = scene
            && let Some(intro) = node.handle.introspect()
            && let Some(IntrospectValue::Text(name)) = intro.query("state")
        {
            return parse_button_state(&name);
        }
        ButtonState::Idle
    }

    fn view(state: ButtonState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(event: ButtonEvent) -> &'static str {
        match event {
            ButtonEvent::PointerEnter => "PointerEnter",
            ButtonEvent::PointerLeave => "PointerLeave",
            ButtonEvent::PointerDown => "PointerDown",
            ButtonEvent::PointerUp => "PointerUp",
            ButtonEvent::Disable => "Disable",
            ButtonEvent::Enable => "Enable",
            _ => "__internal__",
        }
    }

    fn title() -> &'static str {
        "pinion hello-commands (R51.163 §5.23 dispatch loop demo)"
    }

    fn keybinding(key: &str) -> Option<ButtonEvent> {
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

impl WidgetA11y for CommandsView {
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

impl WidgetView for CommandsView {
    type Renderer = HelloCommandsRenderer;

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
    }
}

fn parse_button_state(name: &str) -> ButtonState {
    match name {
        "Hover" => ButtonState::Hover,
        "Pressed" => ButtonState::Pressed,
        "Disabled" => ButtonState::Disabled,
        _ => ButtonState::Idle,
    }
}

/// R51.163 §5.23 — application-supplied [`Handler`] for the
/// `demo.echo` kind. Echoes the payload back as an [`Intent`] tagged
/// `echo.demo.echo`.
///
/// In a real application this would be a richer impl that does
/// actual IO (HTTP, file read, clipboard write, etc.) and returns an
/// [`Intent`] describing the outcome. For the demo we surface the
/// payload + the executor + the channel work simply by
/// stderr-tracing the handler entry then echoing back.
fn echo_handler() -> Arc<dyn Handler> {
    Arc::new(|cmd: Command| -> HandlerFuture {
        Box::pin(async move {
            eprintln!(
                "handler: {} received payload={:?} (scope_id={})",
                cmd.kind_str(),
                cmd.payload,
                cmd.scope_id,
            );
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
    pinion_shell::run_with_handlers::<CommandsView>(build_handler_registry());
}
