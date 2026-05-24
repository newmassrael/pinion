//! `hello-commands` — R51.170 §5.23 R27 reducer-driven dispatch demo.
//!
//! Sibling of [`hello-button`](../hello-button) that exercises the
//! §5.23 R27 Command pipeline end-to-end via the `WidgetCore::update`
//! reducer surface (R51.166-169 substrate land):
//!
//! ```text
//! button press
//!   → ButtonExternal SCXML transition → click intent drains
//!     → ShellCore::handle_tail (R51.169)
//!       → CoreShell::route_intent_through_update (R51.167)
//!         → CommandsView::update — matches "main_btn.click"
//!         → returns vec![Command("demo.echo", "hello pinion")]
//!         → root_owner.dispatch_command queues it
//!       → CoreShell::dispatch_pending_commands (R51.157 drain pump)
//!         → CommandExecutor::dispatch (R51.156)
//!           → TokioExecutor::spawn → tokio worker (R51.159)
//!             → echo Handler resolves (+200ms sleep) → Intent
//!             → ProxyIntentSink → AppEvent::IntentArrived
//!               → user_event arm → ShellCore::dispatch_intent
//!                 → CoreShell::route_intent_through_update — no match
//!                 → SCXML invoke("send", "echo.demo.echo") (R51.159 re-feed)
//! ```
//!
//! ## What this demo shows
//!
//! - Click the button. The Button SCXML statechart emits a
//!   `main_btn.click` intent on `PointerUp`; the R51.169
//!   `handle_tail` routing pumps that intent through
//!   [`CommandsView::update`].
//!   The reducer matches the tag and emits a `demo.echo` `Command`
//!   describing the work, which the framework runs through the
//!   registered handler.
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
//! ## Why this is the textbook dogfood
//!
//! Pre-R51.170 this binary leaned on a view-fn one-shot HACK
//! ([`Owner::cache`] + `Cell<bool>` guard) to pre-queue the demo
//! command before any input arrived. That side-stepped the §5.23 R27
//! "Update(&mut Model, Intent) -> Vec<Command>" contract — the
//! framework's `WidgetCore::update` reducer surface didn't exist
//! yet, so the example faked it from inside the view fn. With the
//! reducer substrate (R51.166-169) in place, the example replays the
//! real R27 contract: input flows in, the reducer decides what async
//! work to schedule, and the framework owns the dispatch.
//!
//! ## Running
//!
//! ```text
//! cargo run -p hello-commands
//! ```
//!
//! Expected stderr after clicking the button:
//! ```text
//! shell: initial state = Idle
//! shell: intent main_btn.click payload=Null
//! shell: state Idle -> Pressed
//! handler: demo.echo received payload=Text("hello pinion") ... after 200ms sleep
//! shell: intent-feedback echo.demo.echo payload=Text("hello pinion")
//! ```
//!
//! Press `Esc` to quit.

use std::sync::Arc;

use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Color, Command, Frame, Intent, Scene, WidgetCore};
use pinion_derive::widget;
use pinion_runtime::{Handler, HandlerFuture, HandlerRegistry};
use pinion_shell::vello_renderer_impl;

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

/// R51.170 §5.23 R27 — kind tag for the demo Command emitted by
/// [`CommandsView::update`] when the button click intent arrives.
/// Matches the handler registration in [`build_handler_registry`].
const DEMO_KIND: &str = "demo.echo";

/// R51.170 §5.23 R27 — fixed payload the reducer attaches to every
/// `demo.echo` Command. The handler echoes it back as the Intent's
/// payload.
const DEMO_PAYLOAD: &str = "hello pinion";

/// R51.170 §5.23 R27 — intent tag the reducer matches against. The
/// Button SCXML emits `click` on `PointerUp`; the §5.20 R22
/// `<widget_tag>.<kind>` prefix convention makes the full
/// wire-form tag `main_btn.click` (`V::tag()` = `"main_btn"`).
const CLICK_INTENT_TAG: &str = "main_btn.click";

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ButtonState, _frame: &Frame) -> Scene {
    let label = match state {
        ButtonState::Disabled => "Disabled",
        _ => "Click for command flow",
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

/// R654 §5.16 Cat A cascade retrofit (R642 baseline). Single
/// `ButtonState` enum — no tuple, no value sidecar, no checked
/// semantic. Macro derives `WidgetCore` / `WidgetA11y` / `WidgetView`
/// with the standard hovered/pressed/disabled state_flags; only
/// `update` (R27 reducer that emits `demo.echo` Command on the
/// `main_btn.click` intent), `apply_key`, `keybinding`, and the
/// view / read_state / event_name shims stay inherent.
#[widget(
    tag = "main_btn",
    state = ButtonState,
    event = ButtonEvent,
    title = "pinion hello-commands (R654 §5.16 #[widget] retrofit)",
    renderer = HelloCommandsRenderer,
    initial_size = (WIN_W, WIN_H),
    external = ButtonExternal::new,
    role = Button,
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
    ),
    apply_key,
    keybinding,
    update,
)]
struct CommandsView;

impl CommandsView {
    fn read_state(scene: &Scene) -> ButtonState {
        if let Scene::External(node) = scene
            && let Some(intro) = node.handle.introspect()
            && let Some(IntrospectValue::Text(name)) = intro.query("state")
        {
            return parse_button_state(&name);
        }
        ButtonState::Idle
    }

    fn view(state: ButtonState, frame: Frame) -> Scene {
        view(state, &frame)
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

    fn keybinding(key: &str) -> Option<ButtonEvent> {
        match key {
            "d" => Some(ButtonEvent::Disable),
            "e" => Some(ButtonEvent::Enable),
            _ => None,
        }
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }

    /// R51.170 §5.23 R27 — reducer-driven dogfood. Match the SCXML-
    /// emitted `main_btn.click` intent and emit a `demo.echo` Command
    /// describing the async work. The R51.169 `handle_tail` wiring
    /// drives this on every drain, so a pointer-up release or
    /// `Enter`/`Space` activation triggers the dispatch loop without
    /// the pre-R51.170 view-fn one-shot HACK.
    ///
    /// `Owner::current()` resolves to the root owner because
    /// `CoreShell::route_intent_through_update` wraps this call in
    /// `root_owner.run(...)` per [[callback-root-owner-wrap]] —
    /// tagging the Command with the producing scope id surfaces it
    /// to the `scene/commands` RPC inspector. `match` over
    /// `intent.tag_str()` is the Elm/Iced canonical reducer shape
    /// (R51.174 §5.23 R27).
    fn update(_state: ButtonState, intent: &Intent) -> Vec<Command> {
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
/// R51.165 §5.23 — sleeps 200ms before echoing to make the async
/// boundary observable in the stderr trace. The sleep runs on the
/// tokio worker thread (`pinion_shell::TokioExecutor`), so the UI
/// thread keeps painting / handling input the whole time. The gap
/// between "queued demo.echo" (view-fn one-shot) and "handler:
/// demo.echo received" demonstrates that the future actually
/// suspended at the `.await` and resumed later, not just resolved
/// synchronously.
///
/// In a real application this would be a richer impl that does
/// actual IO (HTTP, file read, clipboard write, etc.) and returns
/// an [`Intent`] describing the outcome.
fn echo_handler() -> Arc<dyn Handler> {
    Arc::new(|cmd: Command| -> HandlerFuture {
        Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            eprintln!(
                "handler: {} received payload={:?} (scope_id={}) after 200ms sleep",
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

#[cfg(test)]
mod a11y_tests {
    use super::*;

    #[test]
    fn r55_g23_view_contains_composite_paint_root_tag() {
        // R55.G.23 §5.49 — paint scene must carry the composite
        // `CommandsView::tag()` so AI-side `{path: "main_btn"}` input
        // routing and `rect_for_tag` AT bounds attach resolve.
        // CommandsView reuses the ButtonExternal SCXML / view-fn
        // shape from hello-button so the convention regression
        // surface is identical — the difference between the two
        // bindings is the §5.23 Command dispatch path, not the
        // paint scene topology.
        //
        // R55.G.22 §5.49 — pinned via the framework helper which
        // calls `V::view` under an `Owner::new()` scope and asserts
        // `Scene::contains_tag(V::tag())`.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<CommandsView>(
            ButtonState::Idle,
            &Frame::new(),
        );
    }
}
