//! R51.127 §5.41 — substrate-level test fixtures shared across the
//! `pinion-runtime` and `pinion-tui` test suites. Gated behind the
//! `test-fixtures` feature so the symbols never reach a production
//! binary, while still letting downstream `#[cfg(test)]` modules
//! re-export the fixtures through a `dev-dependencies` feature flag.
//!
//! The fixtures intentionally implement [`WidgetCore`] only — the
//! per-backend a11y / view-trait impls (e.g. `WidgetA11y`,
//! `WidgetViewTui`) stay in each backend's test module so the
//! `pinion-core` crate keeps its `pinion-a11y` / `pinion-tui` dep
//! direction empty (cycle invariant per [[r47-class-incident-
//! prevention]]).

use std::borrow::Cow;

use crate::command::Command;
use crate::external::{External, IntrospectValue};
use crate::intent::Intent;
use crate::scene::{ContainerNode, Rect, Scene, TextNode};
use crate::widget_core::WidgetCore;
use crate::widgets::aria;
use crate::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use crate::Frame;

/// Minimal Button binding for substrate-level tests.
///
/// Carries a [`ButtonExternal`] so the SCXML statechart stays
/// observable and intent-emitting. The view fn paints a 32×48-pixel
/// button rect tagged `test_btn` so the runtime hit-test router
/// resolves it.
///
/// The same fixture covers the TUI 4×3-cell footprint — the rect
/// `(0, 0, 32, 48)` lands inside the top-left cell of the buffer.
pub struct ButtonFixture;

impl WidgetCore for ButtonFixture {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn tag() -> &'static str {
        "test_btn"
    }

    fn read_state(scene: &Scene) -> Self::State {
        if let Scene::External(node) = scene
            && let Some(intro) = node.handle.introspect()
            && let Some(IntrospectValue::Text(name)) = intro.query("state")
        {
            return match name.as_str() {
                "Hover" => ButtonState::Hover,
                "Pressed" => ButtonState::Pressed,
                "Disabled" => ButtonState::Disabled,
                _ => ButtonState::Idle,
            };
        }
        ButtonState::Idle
    }

    fn view(_state: Self::State, _frame: &Frame) -> Scene {
        Scene::Container(ContainerNode {
            rect: Rect::new(0, 0, 32, 48),
            tag: Some(Cow::Borrowed("test_btn")),
            children: vec![Scene::Text(TextNode::default())],
            ..Default::default()
        })
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
        "Test"
    }

    fn keybinding(key: &str) -> Option<Self::Event> {
        match key {
            "d" => Some(ButtonEvent::Disable),
            "e" => Some(ButtonEvent::Enable),
            _ => None,
        }
    }

    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
        aria::apply_aria_activate(scene, focused, key, Self::tag())
    }
}

/// R51.167 §5.23 R27 — substrate-level reducer test fixture.
///
/// Reuses [`ButtonFixture`]'s External / paint / `read_state` /
/// `event_name` surface (the SCXML statechart and view geometry are
/// identical) but overrides [`WidgetCore::update`] to emit one
/// `echo.reply` [`Command`] per incoming [`Intent`]. Used by:
///
/// - `pinion-runtime::core_shell::tests` — R51.167 substrate API
///   `route_intent_through_update` assertions.
/// - `pinion-shell::substrate::tests` — R51.168 `dispatch_intent`
///   wires the reducer step BEFORE the SCXML invoke send.
/// - `pinion-tui::substrate::tests` — R51.168 TUI-side mirror.
///
/// Keeping the fixture in `pinion-core::test_fixtures` (rather than
/// duplicating it per backend) lets the three test sites assert
/// identical reducer behaviour without reimplementing the
/// `ButtonExternal` carrier each time.
pub struct EchoButtonFixture;

impl WidgetCore for EchoButtonFixture {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn tag() -> &'static str {
        "echo_btn"
    }

    fn read_state(scene: &Scene) -> Self::State {
        <ButtonFixture as WidgetCore>::read_state(scene)
    }

    fn view(state: Self::State, frame: &Frame) -> Scene {
        <ButtonFixture as WidgetCore>::view(state, frame)
    }

    fn event_name(event: Self::Event) -> &'static str {
        <ButtonFixture as WidgetCore>::event_name(event)
    }

    fn title() -> &'static str {
        "EchoBtn"
    }

    fn update(_state: &mut Self::State, intent: &Intent) -> Vec<Command> {
        vec![Command::new_static(
            "echo.reply",
            IntrospectValue::Text(intent.tag_str().to_string()),
            42,
        )]
    }
}
